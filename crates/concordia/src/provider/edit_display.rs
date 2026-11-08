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

pub(crate) fn needs_codex_patch(cards: &[StreamCard]) -> bool {
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
    let mut patches: Vec<(String, String, usize)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (batch, input) in inputs
        .iter()
        .enumerate()
        .filter(|(_, input)| input.len() <= MAX_BYTES)
    {
        if !seen.insert(input) {
            continue;
        }
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
                    patches.push((path, std::mem::take(&mut body), batch));
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
                    patches.push((path, std::mem::take(&mut body), batch));
                }
            } else if line.starts_with(['+', '-', ' ']) || line.starts_with("@@") {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    let card_paths = cards
        .iter()
        .map(|card| {
            card.detail
                .as_deref()
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.strip_prefix("File: "))
                .map(|path| path_key(path, cwd))
                .collect::<std::collections::BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let mut changed = Vec::new();
    for (card_index, card) in cards.iter_mut().enumerate() {
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
        let paths = &card_paths[card_index];
        let candidates = patches
            .iter()
            .filter(|(path, _, _)| paths.contains(&path_key(path, cwd)))
            .map(|(_, _, batch)| *batch)
            .filter(|batch| {
                paths.iter().all(|path| {
                    patches.iter().any(|(reported, _, candidate)| {
                        candidate == batch && path_key(reported, cwd) == *path
                    })
                })
            })
            .collect::<std::collections::BTreeSet<_>>();
        let peers = card_paths
            .iter()
            .enumerate()
            .filter(|(_, other)| *other == paths)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        // One complete native patch batch produces one completed file-change event. Where
        // a file changes repeatedly, map only a complete one-to-one ordered set. Never paste
        // every later same-file patch into an earlier edit row.
        let batch = if candidates.len() == 1 {
            candidates.iter().next().copied()
        } else if candidates.len() == peers.len() {
            peers
                .iter()
                .position(|index| *index == card_index)
                .and_then(|ordinal| candidates.iter().nth(ordinal).copied())
        } else {
            None
        };
        let Some(batch) = batch else {
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
                .filter(|(patch_path, _, candidate)| {
                    *candidate == batch && self::path_key(patch_path, cwd) == path_key
                })
                .map(|(_, body, _)| body.as_str())
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
    if normalize_completed_wrapper(card) {
        return;
    }
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

/// Convert ACP's provider-reported diff payload into the common edit-card format.
pub(crate) fn normalize_acp(card: &mut StreamCard, value: &Value) {
    let Some(path) = value.get("path").and_then(Value::as_str) else {
        return;
    };
    let Some(diff) = value.get("diff").and_then(Value::as_str) else {
        return;
    };
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return;
    }
    let detail = if diff.is_empty() {
        replacement(
            value
                .get("oldText")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            value
                .get("newText")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
    } else {
        diff.to_owned()
    };
    card.title = "Edit".into();
    card.detail = Some(format!("File: {path}\n{detail}"));
}

#[cfg(test)]
mod acp_tests {
    use super::*;

    #[test]
    fn acp_diff_becomes_the_common_edit_card_evidence() {
        let mut card = StreamCard {
            id: "acp-edit".into(),
            title: "tool_call".into(),
            status: super::super::CardStatus::Done,
            detail: None,
        };
        normalize_acp(
            &mut card,
            &serde_json::json!({"path":"src/main.rs","diff":"@@ -1 +1 @@\n-old\n+new"}),
        );
        assert_eq!(card.title, "Edit");
        let detail = card.detail.unwrap();
        assert!(detail.contains("File: src/main.rs"));
        assert!(detail.contains("-old"));
        assert!(detail.contains("+new"));
    }
}

fn normalize_completed_wrapper(card: &mut StreamCard) -> bool {
    if card.status != super::CardStatus::Done {
        return false;
    }
    let Some(raw) = card
        .detail
        .as_deref()
        .filter(|raw| raw.len() <= MAX_BYTES && !raw.starts_with("File: "))
    else {
        return false;
    };
    let Some((input, output)) = raw.split_once("\nOutput: ") else {
        return false;
    };
    let input = input.strip_prefix("input: ").unwrap_or(input);
    let Some(calls) = super::stream_cards::literal_wrapper_calls(input) else {
        return false;
    };
    let output = serde_json::from_str(output).unwrap_or_else(|_| Value::String(output.to_owned()));
    let mut joined = String::from("*** Begin Patch\n");
    let mut paths = Vec::new();
    for (index, (name, value)) in calls.into_iter().enumerate() {
        if name != "apply_patch"
            || !crate::native_memory::successful_patch_output(&output, Some(index))
        {
            continue;
        }
        let Some(patch) = value.as_str().filter(|patch| {
            patch.starts_with("*** Begin Patch\n") && patch.trim_end().ends_with("*** End Patch")
        }) else {
            continue;
        };
        for line in patch
            .lines()
            .skip(1)
            .take_while(|line| *line != "*** End Patch")
        {
            joined.push_str(line);
            joined.push('\n');
            if let Some(path) = line
                .strip_prefix("*** Update File: ")
                .or_else(|| line.strip_prefix("*** Add File: "))
                .or_else(|| line.strip_prefix("*** Delete File: "))
            {
                paths.push(path.to_owned());
            } else if let Some(path) = line.strip_prefix("*** Move to: ") {
                if let Some(previous) = paths.last_mut() {
                    *previous = path.to_owned();
                }
            }
        }
    }
    if paths.is_empty() {
        return false;
    }
    joined.push_str("*** End Patch\n");
    paths.dedup();
    let mut preview = [StreamCard {
        id: card.id.clone(),
        title: "Edit".into(),
        status: card.status,
        detail: Some(
            paths
                .iter()
                .map(|path| format!("File: {path}\n{UNAVAILABLE}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    }];
    if enrich_codex(&mut preview, &[joined], Path::new(".")).is_empty() {
        return false;
    }
    card.detail = Some(format!(
        "{}\n\nNative tool input and output:\n{raw}",
        preview[0].detail.as_deref().unwrap_or_default()
    ));
    true
}
