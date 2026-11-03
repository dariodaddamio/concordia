//! Metadata-only native usage observations. They never enter Concordia attempt totals.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_OBSERVATIONS: usize = 256;
const MAX_RECORDS: usize = 2048;
const STORE_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub source: String,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub key: String,
    pub source: String,
    pub start: u64,
    pub bytes: usize,
    pub sha256: String,
    pub native_event: Option<String>,
    pub model: Option<String>,
    pub kind: String,
    pub input: Option<u64>,
    pub cached: Option<u64>,
    pub cache_write: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u8,
    binding: Binding,
    observations: Vec<Observation>,
    checkpoints: Vec<Cursor>,
}

pub struct Page {
    pub binding: Binding,
    pub observations: Vec<Observation>,
    pub next: Option<Cursor>,
    pub source: String,
    pub revision: String,
    pub skipped: usize,
    pub retained: usize,
}

fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("native usage read cancelled".into())
    } else {
        Ok(())
    }
}

/// The native history owner creates immutable page pointers; this reader only extracts counters.
pub fn import_page(
    root: &Path,
    binding: &Binding,
    cursor: Option<&Cursor>,
    cancel: &AtomicBool,
) -> Result<Page, String> {
    cancelled(cancel)?;
    if binding.provider == ProviderKind::Cursor {
        return Err("Cursor native usage import unavailable: no verified usage-event source for this transport".into());
    }
    let mut source = match cursor {
        Some(cursor) => cursor.source.clone(),
        None => index_history(root, binding, None)?.id,
    };
    let mut index = load(root, &source)?;
    let home = binding.account_home(root)?;
    if index.binding
        != (Binding {
            home,
            ..binding.clone()
        })
    {
        return Err("native usage cursor belongs to another account/home/session".into());
    }
    if cursor.is_some_and(|cursor| {
        index
            .history
            .as_ref()
            .is_some_and(|page| cursor.offset == page.end && page.next.is_some())
    }) {
        source = index_history(root, binding, Some(&source))?.id;
        index = load(root, &source)?;
    }
    let history = index
        .history
        .as_ref()
        .ok_or("usage import needs a whole-history source page")?;
    let offset = cursor.map_or(history.start, |cursor| cursor.offset);
    if offset < history.start || offset > history.end {
        return Err("native usage cursor is outside its source page".into());
    }
    let mut file = open_index_source(root, &index)?;
    let starts_inside_record =
        history.start > 0 && read_at(&mut file, history.start - 1, 1)? != b"\n";
    let bytes = read_at(
        &mut file,
        history.start,
        (history.end - history.start) as usize,
    )?;
    if digest(&bytes) != history.sha256 {
        return Err("native usage source page changed; re-index explicitly".into());
    }
    let local = (offset - history.start) as usize;
    if local > 0 && bytes.get(local - 1) != Some(&b'\n') {
        return Err("native usage cursor is not on a record boundary".into());
    }
    let mut position = offset;
    let mut observations = Vec::new();
    let mut skipped = 0;
    for (count, raw) in bytes[local..]
        .split_inclusive(|byte| *byte == b'\n')
        .enumerate()
    {
        cancelled(cancel)?;
        if count >= MAX_RECORDS || observations.len() >= MAX_OBSERVATIONS {
            break;
        }
        let start = position;
        position += raw.len() as u64;
        // A boundary-split oversized line is not a complete event; the next page owns its skip.
        if raw.len() > LINE_BYTES
            || (start == history.start && starts_inside_record)
            || !raw.ends_with(b"\n")
        {
            skipped += 1;
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(raw) else {
            skipped += 1;
            continue;
        };
        let native_id = |value: &Value| {
            value
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
                .map(str::to_owned)
        };
        let (usage, native_event, kind) = match binding.provider {
            ProviderKind::Claude if value["type"] == "assistant" => {
                if value["sessionId"].as_str() != Some(binding.session_id.as_str()) {
                    skipped += 1;
                    continue;
                }
                (
                    &value["message"]["usage"],
                    native_id(&value["message"]["id"]),
                    "claude_message_snapshot",
                )
            }
            ProviderKind::Codex
                if value["type"] == "event_msg" && value["payload"]["type"] == "token_count" =>
            {
                (
                    &value["payload"]["info"]["last_token_usage"],
                    None,
                    "codex_last_request_snapshot",
                )
            }
            _ => continue,
        };
        if !usage.is_object() {
            skipped += 1;
            continue;
        }
        let number = |claude, codex| {
            usage[if binding.provider == ProviderKind::Claude {
                claude
            } else {
                codex
            }]
            .as_u64()
        };
        let input = number("input_tokens", "input_tokens");
        let output = number("output_tokens", "output_tokens");
        let cached = number("cache_read_input_tokens", "cached_input_tokens");
        let cache_write = (binding.provider == ProviderKind::Claude)
            .then(|| usage["cache_creation_input_tokens"].as_u64())
            .flatten();
        if input.is_none() && output.is_none() && cached.is_none() && cache_write.is_none() {
            skipped += 1;
            continue;
        }
        let sha256 = digest(raw);
        // Exact record identity deduplicates reimports, never token/time similarity.
        let key = digest(
            format!(
                "{}\n{}\n{}\n{start}\n{sha256}",
                index.binding.account, index.binding.session_id, index.file_identity
            )
            .as_bytes(),
        );
        observations.push(Observation {
            key,
            source: source.clone(),
            start,
            bytes: raw.len(),
            sha256,
            native_event,
            model: (binding.provider == ProviderKind::Claude)
                .then(|| native_id(&value["message"]["model"]))
                .flatten(),
            kind: kind.into(),
            input,
            cached,
            cache_write,
            output,
        });
    }
    cancelled(cancel)?;
    let page_bytes = bytes.len();
    drop(bytes);
    let mut check = open_index_source(root, &index)?;
    if digest(&read_at(&mut check, history.start, page_bytes)?) != history.sha256 {
        return Err("native usage source changed during read; nothing imported".into());
    }
    let next = if position < history.end || history.next.is_some() {
        Some(Cursor {
            source: source.clone(),
            offset: position,
        })
    } else {
        None
    };
    let checkpoint = Cursor {
        source: source.clone(),
        offset: position,
    };
    let retained = retain(root, &index.binding, &observations, checkpoint, cancel)?;
    Ok(Page {
        binding: index.binding,
        observations,
        next,
        source,
        revision: history.sha256.clone(),
        skipped,
        retained,
    })
}

fn retain(
    root: &Path,
    binding: &Binding,
    observations: &[Observation],
    checkpoint: Cursor,
    cancel: &AtomicBool,
) -> Result<usize, String> {
    let directory = root.join(".ao/native-usage");
    fs::create_dir_all(&directory).map_err(|_| "native usage metadata directory unavailable")?;
    crate::workspaces::directory_id(&directory)?;
    let key = digest(&serde_json::to_vec(binding).map_err(|_| "invalid usage binding")?);
    let path = directory.join(format!("{key}.json"));
    let lock = directory.join("catalog.lock");
    for file in [&path, &lock] {
        if let Ok(metadata) = fs::symlink_metadata(file) {
            if !metadata.is_file() || crate::workspaces::linked(&metadata) {
                return Err("native usage metadata must use regular owned files".into());
            }
        }
    }
    cancelled(cancel)?;
    crate::state_io::with_lock(&lock, || {
        if !path.exists() && fs::read_dir(&directory)?.take(258).count() > 256 {
            return Err(std::io::Error::other(
                "native usage metadata directory budget reached; no automatic pruning",
            ));
        }
        let mut catalog = match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(STORE_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > STORE_BYTES {
                    return Err(std::io::Error::other(
                        "native usage metadata exceeds 512 KiB",
                    ));
                }
                let catalog: Catalog =
                    serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
                if catalog.version != 1
                    || catalog.binding != *binding
                    || catalog.observations.len() > 1024
                    || catalog.checkpoints.len() > 32
                {
                    return Err(std::io::Error::other(
                        "native usage metadata identity/version/bounds mismatch",
                    ));
                }
                catalog
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Catalog {
                version: 1,
                binding: binding.clone(),
                observations: Vec::new(),
                checkpoints: Vec::new(),
            },
            Err(error) => return Err(error),
        };
        for observation in observations {
            if let Some(old) = catalog
                .observations
                .iter()
                .find(|old| old.key == observation.key)
            {
                if old.start != observation.start
                    || old.sha256 != observation.sha256
                    || old.input != observation.input
                    || old.output != observation.output
                    || old.cached != observation.cached
                    || old.cache_write != observation.cache_write
                {
                    return Err(std::io::Error::other(
                        "native usage event identity conflict; original retained",
                    ));
                }
            } else {
                catalog.observations.push(observation.clone());
            }
        }
        if let Some(saved) = catalog
            .checkpoints
            .iter_mut()
            .find(|saved| saved.source == checkpoint.source)
        {
            saved.offset = saved.offset.max(checkpoint.offset);
        } else {
            catalog.checkpoints.push(checkpoint);
        }
        let bytes = serde_json::to_vec(&catalog).map_err(std::io::Error::other)?;
        if catalog.observations.len() > 1024
            || catalog.checkpoints.len() > 32
            || bytes.len() as u64 > STORE_BYTES
        {
            return Err(std::io::Error::other(
                "native usage metadata budget reached; original retained, no automatic pruning",
            ));
        }
        cancelled(cancel).map_err(std::io::Error::other)?;
        crate::state_io::write_atomic(&path, &bytes)?;
        Ok(catalog.observations.len())
    })
    .map_err(|error| error.to_string())
}
