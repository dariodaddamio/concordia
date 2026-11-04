//! Render only provider-reported edit evidence; never reconstruct history from today's file.
use super::stream_cards::StreamCard;
use serde_json::Value;
use std::path::Path;

const MAX_BYTES: usize = 256 * 1024;
const UNAVAILABLE: &str = "Provider reported a change; line diff unavailable.";

fn path_key(path: &str, cwd: &Path) -> String {
    let path = Path::new(path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let absolute = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    let text = absolute.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

pub(crate) fn matches_reported_paths(input: &str, cwd: &Path, reported: &[String]) -> bool {
    let mut paths = std::collections::BTreeSet::new();
    let mut current = None;
    for line in input.lines() {
        if let Some(path) = line
            .strip_prefix("*** Update File: ")
            .or_else(|| line.strip_prefix("*** Add File: "))
            .or_else(|| line.strip_prefix("*** Delete File: "))
        {
            if let Some(previous) = current.replace(path_key(path, cwd)) {
                paths.insert(previous);
            }
        } else if let Some(path) = line.strip_prefix("*** Move to: ") {
            current = Some(path_key(path, cwd));
        }
    }
    if let Some(path) = current {
        paths.insert(path);
    }
    !paths.is_empty() && paths == reported.iter().map(|path| path_key(path, cwd)).collect()
}

pub(super) fn needs_codex_patch(cards: &[StreamCard]) -> bool {
    cards.iter().any(|card| {
        card.status == super::CardStatus::Done
            && card.title == "Edit"
            && card
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains(UNAVAILABLE))
    })
}

pub(super) fn changes(value: &Value) -> Option<String> {
    let changes = value.as_array()?;
    if changes.len() > 64 || value.to_string().len() > MAX_BYTES {
        return None;
    }
    let mut out = Vec::new();
    for change in changes {
        let path = change.get("path")?.as_str()?;
        if path.contains(['\n', '\r']) || path.len() > 4096 {
            return None;
        }
        let mut text = format!("File: {path}\n");
        if let Some(diff) = change.get("diff").and_then(Value::as_str) {
            text.push_str(diff);
        } else if let (Some(old), Some(new)) = (
            change.get("old").and_then(Value::as_str),
            change.get("new").and_then(Value::as_str),
        ) {
            text.push_str(&replacement(old, new));
        } else {
            text.push_str(UNAVAILABLE);
        }
        out.push(text);
    }
    (!out.is_empty()).then(|| out.join("\n\n"))
}

/// `codex exec --json` reports changed paths without patch bodies. Attach only successful
/// apply_patch input recovered from this exact native session and current attempt.
pub(crate) fn enrich_codex(
    cards: &mut [StreamCard],
    inputs: &[String],
    cwd: &Path,
) -> Vec<StreamCard> {
    let mut patches: Vec<(String, String)> = Vec::new();
    for input in inputs.iter().filter(|input| input.len() <= MAX_BYTES) {
        if input.lines().next() != Some("*** Begin Patch")
            || !input.trim_end().ends_with("*** End Patch")
        {
            continue;
        }
        let mut path: Option<String> = None;
        let mut body = String::new();
        for line in input.lines().skip(1) {
            if let Some(next) = line
                .strip_prefix("*** Update File: ")
                .or_else(|| line.strip_prefix("*** Add File: "))
                .or_else(|| line.strip_prefix("*** Delete File: "))
            {
                if let Some(path) = path.take() {
                    if !body.is_empty() {
                        patches.push((path, std::mem::take(&mut body)));
                    }
                }
                body.clear();
                path =
                    (!next.is_empty() && next.len() <= 4096 && !next.chars().any(char::is_control))
                        .then(|| next.to_owned());
            } else if let Some(next) = line.strip_prefix("*** Move to: ") {
                path =
                    (!next.is_empty() && next.len() <= 4096 && !next.chars().any(char::is_control))
                        .then(|| next.to_owned());
            } else if line == "*** End Patch" {
                if let Some(path) = path.take() {
                    if !body.is_empty() {
                        patches.push((path, std::mem::take(&mut body)));
                    }
                }
            } else if line.starts_with(['+', '-', ' ']) || line.starts_with("@@") {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    let mut changed = Vec::new();
    for card in cards {
        if card.status != super::CardStatus::Done || card.title != "Edit" {
            continue;
        }
        let Some(raw) = card
            .detail
            .as_deref()
            .filter(|raw| raw.contains(UNAVAILABLE))
        else {
            continue;
        };
        let mut detail = raw.to_owned();
        for line in raw.lines() {
            let Some(path) = line.strip_prefix("File: ") else {
                continue;
            };
            let path_key = path_key(path, cwd);
            let body: String = patches
                .iter()
                .filter(|(patch_path, _)| self::path_key(patch_path, cwd) == path_key)
                .map(|(_, body)| body.as_str())
                .collect();
            if !body.is_empty() && detail.len() + body.len() <= MAX_BYTES {
                detail = detail.replace(
                    &format!("File: {path}\n{UNAVAILABLE}"),
                    &format!("File: {path}\n{body}"),
                );
            }
        }
        if detail != raw {
            card.detail = Some(detail);
            changed.push(card.clone());
        }
    }
    changed
}

fn replacement(old: &str, new: &str) -> String {
    // A substring replacement has no reliable file line number. Do not invent one.
    let mut out = String::from("@@ reported replacement (file line numbers unavailable) @@\n");
    for (prefix, text) in [('-', old), ('+', new)] {
        for line in text.lines() {
            out.push(prefix);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

pub(super) fn normalize(card: &mut StreamCard) {
    if !matches!(
        card.title.to_ascii_lowercase().as_str(),
        "edit" | "str_replace" | "replace"
    ) {
        return;
    }
    let Some(raw) = card.detail.as_deref().filter(|s| s.len() <= MAX_BYTES) else {
        return;
    };
    if raw.starts_with("File: ") {
        return;
    }
    let input = raw.strip_prefix("input: ").unwrap_or(raw);
    let Some(Ok(value)) = serde_json::Deserializer::from_str(input)
        .into_iter::<Value>()
        .next()
    else {
        return;
    };
    let Some(path) = value
        .get("file_path")
        .or_else(|| value.get("path"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let (Some(old), Some(new)) = (
        value
            .get("old_string")
            .or_else(|| value.get("old_str"))
            .and_then(Value::as_str),
        value
            .get("new_string")
            .or_else(|| value.get("new_str"))
            .and_then(Value::as_str),
    ) else {
        return;
    };
    if path.contains(['\n', '\r']) || path.len() > 4096 {
        return;
    }
    card.detail = Some(format!(
        "File: {path}\n{}\nNative tool input and output:\n{raw}",
        replacement(old, new)
    ));
}
