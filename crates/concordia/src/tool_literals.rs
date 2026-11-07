//! Deterministic parser for the literal host-call syntax used by provider output.
use serde_json::Value;

pub(crate) fn literal_tool_argument(input: &str) -> Option<(Value, usize)> {
    let mut at = 0;
    let mut json = String::new();
    let mut depth = Vec::new();
    while at < input.len() {
        let byte = input.as_bytes()[at];
        if byte == b')' && depth.is_empty() {
            break;
        }
        match byte {
            b'"' => {
                let mut values =
                    serde_json::Deserializer::from_str(&input[at..]).into_iter::<String>();
                values.next()?.ok()?;
                let end = at + values.byte_offset();
                json.push_str(&input[at..end]);
                at = end;
            }
            b'{' | b'[' => {
                if depth.len() >= 64 {
                    return None;
                }
                depth.push(byte);
                json.push(byte as char);
                at += 1;
            }
            b'}' | b']' => {
                let expected = if byte == b'}' { b'{' } else { b'[' };
                if depth.pop()? != expected {
                    return None;
                }
                json.push(byte as char);
                at += 1;
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = at;
                while input
                    .as_bytes()
                    .get(at)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    at += 1;
                }
                let word = &input[start..at];
                if input[at..].trim_start().starts_with(':') {
                    json.push_str(&serde_json::to_string(word).ok()?);
                } else if matches!(word, "true" | "false" | "null") {
                    json.push_str(word);
                } else {
                    return None;
                }
            }
            b'0'..=b'9' | b'-' | b'+' | b'.' | b':' | b',' | b' ' | b'\n' | b'\r' | b'\t' => {
                json.push(byte as char);
                at += 1;
            }
            _ => return None,
        }
    }
    if !depth.is_empty() {
        return None;
    }
    Some((serde_json::from_str(&json).ok()?, at))
}

pub(crate) fn literal_wrapper_calls(input: &str) -> Option<Vec<(String, Value)>> {
    if input.len() > 256 * 1024 {
        return None;
    }
    let mut rest = input.trim();
    if rest.starts_with("// @exec:") {
        rest = rest.split_once('\n')?.1.trim_start();
    }
    let mut calls = Vec::new();
    while !rest.is_empty() {
        if calls.len() >= 8 {
            return None;
        }
        let statement = rest.strip_prefix("text(await tools.")?;
        let end = statement.find('(')?;
        let name = &statement[..end];
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
        let argument = &statement[end + 1..];
        let (value, end) = literal_tool_argument(argument)?;
        rest = argument
            .get(end..)?
            .trim_start()
            .strip_prefix("))")?
            .trim_start();
        if let Some(next) = rest.strip_prefix(';') {
            rest = next.trim_start();
        } else if !rest.is_empty() {
            return None;
        }
        calls.push((name.into(), value));
    }
    (!calls.is_empty()).then_some(calls)
}
