//! Render only provider-reported edit evidence; never reconstruct history from today's file.
use super::stream_cards::StreamCard;
use serde_json::Value;

const MAX_BYTES: usize = 256 * 1024;

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
            text.push_str("Provider reported a change; line diff unavailable.");
        }
        out.push(text);
    }
    (!out.is_empty()).then(|| out.join("\n\n"))
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
