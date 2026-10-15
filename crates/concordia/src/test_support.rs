//! Helpers shared by unit tests.

use std::fs;
use std::path::Path;

fn b64url(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// A Codex home signed in as `email` (the id_token carries the address, as the real CLI's does).
pub fn codex_login(home: &Path, email: &str) {
    fs::create_dir_all(home).unwrap();
    let payload = b64url(format!(r#"{{"email":"{email}"}}"#).as_bytes());
    let jwt = format!("e30.{payload}.sig");
    fs::write(
        home.join("auth.json"),
        format!(
            r#"{{"tokens":{{"id_token":"{jwt}","access_token":"x","account_id":"acct-{email}"}}}}"#
        ),
    )
    .unwrap();
}
