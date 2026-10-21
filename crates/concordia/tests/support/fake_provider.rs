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
    if matches!(args.first().map(String::as_str), Some("cli" | "start")) {
        if let Ok(path) = std::env::var("FAKE_FILE_LAUNCH_JOURNAL") {
            std::fs::write(path, serde_json::to_vec(&args).unwrap()).unwrap();
            return;
        }
    }

    // Full-TUI journeys use this mode. Capability/usage probes never count as model turns.
    if std::env::var("FAKE_PROVIDER_MODE").as_deref() == Ok("interaction") {
        if args.iter().any(|a| a == "--version") {
            println!("fake-cli 1.0.0");
            return;
        }
        if args.iter().any(|a| a == "--help") {
            println!("Usage: fake [OPTIONS]\n--json --sandbox --output-format stream-json --permission-mode manual plan acceptEdits --mode plan --include-partial-messages --stream-partial-output");
            return;
        }
        if args.is_empty() {
            println!("Usage ? Pro\nIncluded 1% used\nOn-Demand Disabled");
            return;
        }
        if args.last().is_some_and(|a| a == "/usage") {
            println!("Current session\n1% used\nCurrent week\n2% used");
            return;
        }
        if args.first().is_some_and(|a| a == "create-chat") {
            println!("11111111-aaaa-bbbb-cccc-222222222222");
            return;
        }
        interaction(&args);
        return;
    }

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

fn interaction(args: &[String]) {
    use serde_json::json;
    let provider = if args.first().is_some_and(|a| a == "exec") {
        "codex"
    } else if args.first().is_some_and(|a| a == "-p") {
        "claude"
    } else {
        "cursor"
    };
    let prompt = args.last().cloned().unwrap_or_default();
    let journal = |kind: &str| {
        if let Ok(path) = std::env::var("FAKE_PROVIDER_JOURNAL") {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(
                    file,
                    "{}",
                    json!({"kind":kind,"provider":provider,"prompt":prompt})
                );
            }
        }
    };
    let emit = |event: serde_json::Value| {
        println!("{event}");
        let _ = std::io::stdout().flush();
    };
    journal("start");
    let thought = format!(
        "reasoning-start {} reasoning-end",
        "checking the requested changes. ".repeat(30)
    );
    let command = format!("synthetic-command {} command-tail", "argument ".repeat(15));
    match provider {
        "codex" => {
            emit(
                json!({"type":"thread.started","thread_id":"99999999-2222-3333-4444-555555555555"}),
            );
            emit(
                json!({"type":"item.completed","item":{"type":"reasoning","id":"reason","text":thought}}),
            );
            emit(
                json!({"type":"item.started","item":{"type":"command_execution","id":"tool","command":command}}),
            );
        }
        "claude" => {
            emit(
                json!({"type":"system","subtype":"init","session_id":"11111111-2222-3333-4444-555555555555"}),
            );
            emit(
                json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":thought},{"type":"tool_use","id":"tool","name":"Bash","input":{"command":command}}]}}),
            );
        }
        _ => {
            emit(
                json!({"type":"system","subtype":"init","session_id":"11111111-aaaa-bbbb-cccc-222222222222"}),
            );
            emit(
                json!({"type":"tool_call","subtype":"started","call_id":"tool","tool_call":{"shellToolCall":{"args":{"command":command}}}}),
            );
        }
    }
    if prompt.contains("[wait]") {
        let gate =
            std::path::PathBuf::from(std::env::var("FAKE_PROVIDER_GATE").expect("fake gate"));
        let start = std::time::Instant::now();
        while !gate.exists() && start.elapsed() < std::time::Duration::from_secs(30) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    if prompt.contains("[fail]") {
        journal("failed");
        eprintln!("fake provider: deliberate interaction failure");
        std::process::exit(3);
    }
    let reply = format!("FAKE REPLY: {prompt}");
    match provider {
        "codex" => {
            emit(
                json!({"type":"item.completed","item":{"type":"command_execution","id":"tool","command":command,"aggregated_output":"tool-output-start\ntool-output-end"}}),
            );
            emit(
                json!({"type":"item.completed","item":{"type":"agent_message","id":"reply","text":reply}}),
            );
            emit(json!({"type":"turn.completed","usage":{"input_tokens":123,"output_tokens":45}}));
        }
        "claude" => {
            emit(
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool","content":"tool-output-start\ntool-output-end"}]}}),
            );
            emit(json!({"type":"assistant","message":{"content":[{"type":"text","text":reply}]}}));
            emit(
                json!({"type":"result","subtype":"success","result":reply,"usage":{"input_tokens":123,"output_tokens":45}}),
            );
        }
        _ => {
            emit(
                json!({"type":"tool_call","subtype":"completed","call_id":"tool","tool_call":{"shellToolCall":{"args":{"command":command},"result":{"success":{"output":"tool-output-start\ntool-output-end"}}}}}),
            );
            emit(json!({"type":"assistant","message":{"content":[{"type":"text","text":reply}]}}));
            emit(json!({"type":"result","subtype":"success","result":reply}));
        }
    }
    journal("done");
}
