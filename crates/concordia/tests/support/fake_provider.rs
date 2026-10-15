//! Stand-in for the codex/claude/agent CLIs so the real turn runner can be exercised end to end
//! without a provider or a subscription. Behavior is chosen by `FAKE_PROVIDER_MODE`:
//!
//! - `stream`: print the JSONL file named by `FAKE_PROVIDER_STREAM`, exit 0.
//! - `hang`:   write our pid to `FAKE_PROVIDER_PID`, print a session line, then sleep.
//! - `fail`:   print to stderr and exit 3.
//!
//! In every mode, if `FAKE_PROVIDER_OUT` is set we record what we received: one `ARG:<hex>` line
//! per argument (hex so newlines and quotes survive), the isolating env vars, our cwd, and the
//! length of anything piped on stdin.

use std::io::{Read, Write};

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Ok(out) = std::env::var("FAKE_PROVIDER_OUT") {
        let mut rec = String::new();
        for a in &args {
            rec.push_str(&format!("ARG:{}\n", hex(a)));
        }
        for key in ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "CURSOR_API_KEY"] {
            if let Ok(v) = std::env::var(key) {
                rec.push_str(&format!("ENV:{key}={}\n", hex(&v)));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            rec.push_str(&format!("CWD:{}\n", hex(&cwd.to_string_lossy())));
        }
        if std::env::var("FAKE_PROVIDER_READ_STDIN").is_ok() {
            let mut buf = String::new();
            let _ = std::io::stdin().read_to_string(&mut buf);
            rec.push_str(&format!("STDIN:{}\n", hex(&buf)));
        }
        let _ = std::fs::write(out, rec);
    }

    // `agent create-chat`: print what FAKE_PROVIDER_CHAT_OUT says (a chat id by default).
    if args.first().map(String::as_str) == Some("create-chat") {
        if std::env::var("FAKE_PROVIDER_CHAT_FAIL").is_ok() {
            eprintln!("fake provider: create-chat failed");
            std::process::exit(1);
        }
        let out = std::env::var("FAKE_PROVIDER_CHAT_OUT")
            .unwrap_or_else(|_| "11111111-aaaa-bbbb-cccc-222222222222".into());
        println!("{out}");
        return;
    }

    let mode = std::env::var("FAKE_PROVIDER_MODE").unwrap_or_else(|_| "stream".into());
    let mut stdout = std::io::stdout();
    match mode.as_str() {
        "stream" => {
            if let Ok(path) = std::env::var("FAKE_PROVIDER_STREAM") {
                let body = std::fs::read_to_string(path).unwrap_or_default();
                for line in body.lines() {
                    let _ = writeln!(stdout, "{line}");
                    let _ = stdout.flush();
                }
            }
        }
        "hang" => {
            if let Ok(path) = std::env::var("FAKE_PROVIDER_PID") {
                let _ = std::fs::write(path, std::process::id().to_string());
            }
            let _ = writeln!(
                stdout,
                r#"{{"type":"thread.started","thread_id":"99999999-2222-3333-4444-555555555555"}}"#
            );
            let _ = stdout.flush();
            std::thread::sleep(std::time::Duration::from_secs(120));
        }
        // Megabytes of stderr before and after normal output: deadlocks a runner that never
        // drains the pipe (the OS pipe buffer is only ~64 KB).
        "flood" => {
            let mut stderr = std::io::stderr();
            for i in 0..40_000 {
                let _ = writeln!(stderr, "flood line {i} {}", "x".repeat(60));
            }
            if let Ok(path) = std::env::var("FAKE_PROVIDER_STREAM") {
                let body = std::fs::read_to_string(path).unwrap_or_default();
                for line in body.lines() {
                    let _ = writeln!(stdout, "{line}");
                }
                let _ = stdout.flush();
            }
            let _ = writeln!(stderr, "flood line final");
        }
        // Invalid UTF-8 mid-stream, then a normal stream: the turn must survive the bad bytes.
        "badutf8" => {
            let _ = stdout.write_all(b"\xff\xfe not utf8 \xc3\x28\n");
            if let Ok(path) = std::env::var("FAKE_PROVIDER_STREAM") {
                let body = std::fs::read_to_string(path).unwrap_or_default();
                for line in body.lines() {
                    let _ = writeln!(stdout, "{line}");
                }
            }
            let _ = stdout.flush();
        }
        // A 3 MB line with no newline until the end, then a normal stream.
        "longline" => {
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..48 {
                let _ = stdout.write_all(&chunk);
            }
            let _ = stdout.write_all(b"\n");
            if let Ok(path) = std::env::var("FAKE_PROVIDER_STREAM") {
                let body = std::fs::read_to_string(path).unwrap_or_default();
                for line in body.lines() {
                    let _ = writeln!(stdout, "{line}");
                }
            }
            let _ = stdout.flush();
        }
        "fail" => {
            eprintln!("fake provider: simulated failure");
            std::process::exit(3);
        }
        other => {
            eprintln!("fake provider: unknown mode {other}");
            std::process::exit(2);
        }
    }
}
