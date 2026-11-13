//! Deterministic, local redaction for values which must not enter Concordia records.
//!
//! Input readers own their byte limits. Every supplied line is scanned, including long
//! serialized records, so a scan boundary cannot copy a credential verbatim into storage.

use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::sync::{OnceLock, RwLock};

static RUNTIME_SECRETS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static INSTALL_KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Register an exact runtime secret. The value is retained only in this process.
pub fn register_runtime_secret(value: impl Into<String>) {
    let value = value.into();
    if !value.is_empty() {
        let mut secrets = RUNTIME_SECRETS
            .get_or_init(|| RwLock::new(Vec::new()))
            .write()
            .expect("runtime redaction lock poisoned");
        if !secrets.contains(&value) {
            secrets.push(value);
        }
    }
}

/// Redact credentials and high-entropy values from text.
pub fn redact(text: &str) -> Cow<'_, str> {
    let mut rest = text;
    let mut output = String::new();
    while let Some((begin, end)) = pem_range(rest) {
        output.push_str(&redact_lines(&rest[..begin]));
        output.push_str(&token("pem_private_key", &rest[begin..end]));
        rest = &rest[end..];
    }
    if rest.len() == text.len() {
        return redact_lines(text);
    }
    output.push_str(&redact_lines(rest));
    Cow::Owned(output)
}

fn redact_lines(text: &str) -> Cow<'_, str> {
    let mut output = String::with_capacity(text.len());
    let mut changed = false;
    let mut start = 0;
    for (line_start, _) in text.match_indices('\n') {
        let end = line_start + 1;
        changed |= redact_line(&text[start..end], &mut output);
        start = end;
    }
    changed |= redact_line(&text[start..], &mut output);
    if changed {
        Cow::Owned(output)
    } else {
        Cow::Borrowed(text)
    }
}

pub fn redact_bytes(bytes: &[u8]) -> Vec<u8> {
    fn strings(value: &mut serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(text) => match redact(text) {
                Cow::Owned(safe) => {
                    *text = safe;
                    true
                }
                Cow::Borrowed(_) => false,
            },
            serde_json::Value::Array(values) => values
                .iter_mut()
                .fold(false, |changed, value| strings(value) || changed),
            serde_json::Value::Object(values) => values
                .values_mut()
                .fold(false, |changed, value| strings(value) || changed),
            _ => false,
        }
    }
    if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) {
        return if strings(&mut value) {
            // Value serialization cannot fail for an already parsed JSON tree.
            serde_json::to_vec(&value).expect("serialize redacted JSON")
        } else {
            bytes.to_vec()
        };
    }
    std::str::from_utf8(bytes)
        .map(|text| redact(text).into_owned().into_bytes())
        .unwrap_or_else(|_| bytes.to_vec())
}

fn redact_line(line: &str, output: &mut String) -> bool {
    let scan = line;
    let mut ranges = Vec::new();
    if let Some(secrets) = RUNTIME_SECRETS.get() {
        for secret in secrets
            .read()
            .expect("runtime redaction lock poisoned")
            .iter()
        {
            find_all(scan, secret, &mut ranges, "runtime_secret");
        }
    }
    find_pem(scan, &mut ranges);
    let bytes = scan.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some((begin, end, kind)) = candidate_at(scan, i) {
            ranges.push((begin, end, kind));
            i = end;
        } else {
            i += scan[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    ranges.sort_by_key(|r| (r.0, std::cmp::Reverse(r.1)));
    let mut cursor = 0;
    let mut changed = false;
    for (begin, end, kind) in ranges {
        if begin < cursor || end > scan.len() {
            continue;
        }
        output.push_str(&scan[cursor..begin]);
        output.push_str(&token(kind, &scan[begin..end]));
        cursor = end;
        changed = true;
    }
    output.push_str(&scan[cursor..]);
    changed
}

fn candidate_at(s: &str, i: usize) -> Option<(usize, usize, &'static str)> {
    let rest = &s[i..];
    if s[..i].starts_with("data:image/") {
        return None;
    }
    if boundary_before(s, i) {
        if let Some((begin, end, kind)) = header_value(rest, "authorization:") {
            return Some((i + begin, i + end, kind));
        }
        if let Some((begin, end, kind)) =
            header_value(rest, "cookie:").or_else(|| header_value(rest, "set-cookie:"))
        {
            return Some((i + begin, i + end, kind));
        }
        if let Some((begin, end, kind)) = url_secret(s, i) {
            return Some((begin, end, kind));
        }
        if let Some((begin, end, kind)) = assignment_secret(s, i) {
            return Some((begin, end, kind));
        }
    }
    let (prefix, kind) = [
        ("sk-ant-", "provider_key"),
        ("sk-proj-", "provider_key"),
        ("sk_live_", "stripe_key"),
        ("rk_live_", "stripe_key"),
        ("sk-", "provider_key"),
        ("ghp_", "github_token"),
        ("gho_", "github_token"),
        ("ghu_", "github_token"),
        ("ghs_", "github_token"),
        ("ghr_", "github_token"),
        ("github_pat_", "github_token"),
        ("xox", "slack_token"),
        ("AIza", "google_api_key"),
        ("npm_", "npm_token"),
        ("AKIA", "aws_access_key"),
        ("ASIA", "aws_access_key"),
        ("eyJ", "jwt"),
    ]
    .iter()
    .find(|(p, _)| rest.starts_with(p))?;
    if !boundary_before(s, i) {
        return None;
    }
    if *prefix == "xox"
        && !matches!(
            rest.as_bytes().get(3),
            Some(b'a' | b'b' | b'p' | b'o' | b's' | b'r')
        )
    {
        return None;
    }
    if *prefix == "eyJ" && !looks_like_jwt(rest) {
        return None;
    }
    let end = value_end(s, i);
    (end > i + prefix.len() + 2).then_some((i, end, *kind))
}

fn header_value(s: &str, header: &str) -> Option<(usize, usize, &'static str)> {
    if s.len() < header.len()
        || !s.is_char_boundary(header.len())
        || !s[..header.len()].eq_ignore_ascii_case(header)
    {
        return None;
    }
    let colon = header.len();
    let mut p = colon;
    while s.as_bytes().get(p).is_some_and(|b| b.is_ascii_whitespace()) {
        p += 1;
    }
    if header == "authorization:" {
        let scheme_end = s[p..].find(char::is_whitespace)? + p;
        let scheme = &s[p..scheme_end];
        if !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "bearer" | "basic" | "token"
        ) {
            return None;
        }
        p = scheme_end;
        while s.as_bytes().get(p).is_some_and(|b| b.is_ascii_whitespace()) {
            p += 1;
        }
    }
    let end = value_end(s, p);
    (end > p + 2).then_some((
        p,
        end,
        if header == "authorization:" {
            "authorization"
        } else {
            "cookie"
        },
    ))
}

fn url_secret(s: &str, i: usize) -> Option<(usize, usize, &'static str)> {
    let rest = &s[i..];
    let scheme = [
        "http://",
        "https://",
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "amqp://",
    ]
    .iter()
    .find(|p| rest.starts_with(**p))?;
    let end = value_end(s, i);
    if end - i > 64 * 1024 {
        return Some((i, end, "oversized_url"));
    }
    let url = &s[i..end];
    if let Some(at) = url.find('@') {
        if let Some(colon) = url[..at].find(':') {
            let begin = i + colon + 1;
            return Some((
                begin,
                i + at,
                if scheme.contains("://") && !scheme.starts_with("http") {
                    "database_password"
                } else {
                    "url_credentials"
                },
            ));
        }
    }
    let query = url.find('?')?;
    let query = &url[query + 1..];
    for pair in query.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "token" | "key" | "sig" | "access_token" | "api_key"
        ) && value.len() >= 3
        {
            return Some((
                i + url.find('?').unwrap() + 1,
                i + url.len(),
                "url_query_secret",
            ));
        }
    }
    None
}

fn assignment_secret(s: &str, i: usize) -> Option<(usize, usize, &'static str)> {
    let bytes = s.as_bytes();
    let mut p = i;
    while p - i <= 256
        && bytes
            .get(p)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        p += 1;
    }
    if p == i || !bytes.get(p).is_some_and(|b| *b == b'=') {
        return None;
    }
    let name = s[i..p].to_ascii_lowercase();
    if ![
        "key",
        "token",
        "secret",
        "password",
        "pwd",
        "credential",
        "auth",
    ]
    .iter()
    .any(|part| name.contains(part))
    {
        return None;
    }
    p += 1;
    while bytes.get(p).is_some_and(|b| b.is_ascii_whitespace()) {
        p += 1;
    }
    let end = value_end(s, p);
    let value = s.get(p..end)?;
    let quoted = value.strip_prefix('"').or_else(|| value.strip_prefix('\''));
    let value = quoted.unwrap_or(value).trim_end_matches(['"', '\'']);
    if value.starts_with("[REDACTED:") {
        return None;
    }
    let entropy = shannon_entropy(value);
    (value.len() >= 12 && (entropy >= 3.5 || known_prefix(value))).then_some((
        p + (quoted.is_some() as usize),
        p + (quoted.map_or(value.len(), |_| value.len() + 1)),
        "assignment_secret",
    ))
}

fn known_prefix(s: &str) -> bool {
    ["sk-", "gh", "xox", "AKIA", "ASIA", "AIza", "npm_", "eyJ"]
        .iter()
        .any(|p| s.starts_with(p))
}
fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let n = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c != 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

fn looks_like_jwt(s: &str) -> bool {
    let mut parts = s.splitn(4, |c: char| {
        c == '.' || c.is_whitespace() || c == '"' || c == '\''
    });
    parts.next().is_some_and(|p| p.len() >= 8 && base64url(p))
        && parts.next().is_some_and(base64url)
        && parts.next().is_some_and(base64url)
}

fn base64url(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn boundary_before(s: &str, i: usize) -> bool {
    i == 0 || !s.as_bytes()[i - 1].is_ascii_alphanumeric()
}
fn value_end(s: &str, start: usize) -> usize {
    let bytes = s.as_bytes();
    let mut end = start;
    while end < bytes.len()
        && !matches!(
            bytes[end],
            b' ' | b'\t' | b'\r' | b'\n' | b'"' | b'\'' | b'`' | b',' | b';' | b')' | b']' | b'}'
        )
    {
        end += s[end..].chars().next().map_or(1, char::len_utf8);
    }
    end
}

fn find_pem(s: &str, ranges: &mut Vec<(usize, usize, &'static str)>) {
    if let Some((begin, end)) = pem_range(s) {
        ranges.push((begin, end, "pem_private_key"));
    }
}

fn pem_range(s: &str) -> Option<(usize, usize)> {
    let begin = s.find("-----BEGIN")?;
    let key = s[begin..].find("PRIVATE KEY-----")?;
    let body = begin + key + "PRIVATE KEY-----".len();
    let end = body + s[body..].find("-----END")?;
    let trailer_end = end + s[end..].find("PRIVATE KEY-----")? + "PRIVATE KEY-----".len();
    Some((begin, trailer_end))
}

fn find_all(
    s: &str,
    needle: &str,
    ranges: &mut Vec<(usize, usize, &'static str)>,
    kind: &'static str,
) {
    if needle.len() < 3 {
        return;
    }
    let mut from = 0;
    while let Some(pos) = s[from..].find(needle) {
        let begin = from + pos;
        ranges.push((begin, begin + needle.len(), kind));
        from = begin + needle.len();
    }
}

fn token(kind: &str, secret: &str) -> String {
    let digest = hmac_sha256(&key(), secret.as_bytes());
    format!(
        "[REDACTED:{kind}:{:02x}{:02x}{:02x}{:02x}]",
        digest[0], digest[1], digest[2], digest[3]
    )
}

fn key() -> [u8; 32] {
    *INSTALL_KEY.get_or_init(|| {
        // Keep token generation independent of process-wide test/application environment.
        // The state directory is intentionally not consulted here: lib tests run concurrently
        // and some isolated tests temporarily alter CONCORDIA_STATE_DIR while exercising
        // storage.  Reading it during lazy initialization made the redaction key depend on which
        // test happened to win the race (and could create redact.key in another test's state).
        let digest =
            Sha256::digest(format!("concordia-redaction-{}", std::process::id()).as_bytes());
        let mut key = [0; 32];
        key.copy_from_slice(&digest);
        key
    })
}

fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..32 {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner);
    outer.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static TEST_KEY_LOCK: Mutex<()> = Mutex::new(());

    fn with_key(f: impl FnOnce()) {
        let _guard = TEST_KEY_LOCK.lock().unwrap();
        let _ = f;
        f();
    }
    fn assert_kind(input: &str, kind: &str) {
        let out = redact(input);
        assert!(
            out.contains(&format!("[REDACTED:{kind}:")),
            "{input} => {out}"
        );
        assert!(!out.contains(input));
    }

    #[test]
    fn true_positive_table() {
        with_key(|| {
            for (input, kind) in [
                ("sk-ant-abcdefghijklmnop", "provider_key"),
                ("ghp_abcdefghijklmnop", "github_token"),
                ("gho_abcdefghijklmnop", "github_token"),
                ("github_pat_abcdefghijklmnop", "github_token"),
                ("AKIAABCDEFGHIJKLMNOP", "aws_access_key"),
                ("ASIAABCDEFGHIJKLMNOP", "aws_access_key"),
                ("xoxb-1234567890-secret", "slack_token"),
                ("sk_live_abcdefghijklmnop", "stripe_key"),
                ("rk_live_abcdefghijklmnop", "stripe_key"),
                ("AIzaSyAbcdefghijklmnop", "google_api_key"),
                ("npm_abcdefghijklmnop", "npm_token"),
                ("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.signature", "jwt"),
                ("Authorization: Bearer sk-abcdefghijklmnop", "authorization"),
                ("Cookie: session=abcdefghijklmnop", "cookie"),
                ("https://user:passphrase@example.test", "url_credentials"),
                (
                    "https://example.test/?api_key=abcdefghijklmnop",
                    "url_query_secret",
                ),
                (
                    "postgres://user:passphrase@db.test/app",
                    "database_password",
                ),
                ("MY_SECRET=7fA!2qZ9#mL4@xP8", "assignment_secret"),
                (
                    "-----BEGIN RSA PRIVATE KEY-----\nabc\n-----END RSA PRIVATE KEY-----",
                    "pem_private_key",
                ),
            ] {
                assert_kind(input, kind);
            }
        });
    }

    #[test]
    fn negative_corpus_has_zero_hits() {
        let s = "commit 0123456789abcdef0123456789abcdef01234567\nUUID 550e8400-e29b-41d4-a716-446655440000\nsha 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\ndata:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAE=";
        assert_eq!(redact(s), s);
    }
    #[test]
    fn redaction_is_idempotent() {
        let once = redact("key=sk-abcdefghijklmnop").into_owned();
        assert_eq!(redact(&once), once);
    }
    #[test]
    fn never_panics_on_arbitrary_utf8() {
        for n in 0..1000 {
            let s: String = (0..(n % 70))
                .map(|i| char::from_u32((i * 7919 + n) as u32 % 0x10ffff).unwrap_or('界'))
                .collect();
            let _ = redact(&s);
        }
    }
    #[test]
    fn never_more_than_doubles_length() {
        let s = "sk-abcdefghijklmnop ".repeat(5000);
        assert!(redact(&s).len() <= s.len() * 2);
    }
    #[test]
    fn same_secret_same_token() {
        assert_eq!(redact("sk-abcdefghijklmnop"), redact("sk-abcdefghijklmnop"));
    }
    #[test]
    fn runtime_secret_is_redacted() {
        register_runtime_secret("runtime-only-value");
        assert_kind("runtime-only-value", "runtime_secret");
    }
}
