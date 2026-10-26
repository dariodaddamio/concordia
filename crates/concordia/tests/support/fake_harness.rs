//! Stand-in for a user's harness adapter in tests. Never installed or released.
//!
//! First argument selects the behavior:
//! - `connect <provider> <home> <project>`: record the call in `<home>/harness-connected`
//!   (exit 3 with a message when `FAKE_HARNESS_FAIL=1`).
//! - `verify <provider> <home> <project>`: print a probe log line and a JSON report. The report
//!   depends on `FAKE_HARNESS_VERIFY`: `denied` (default), `forged` (wrong policy revision),
//!   `unsupported` (not a denial).
//! - `manifest`: print `FAKE_HARNESS_MANIFEST`.
//! - `echo-args`: print each argument on its own line.
//! - `hang`: sleep for a minute. `flood`: print about 300 KB.

use sha2::{Digest, Sha256};

fn revision(project: &str) -> String {
    let text = std::fs::read_to_string(format!("{project}/docs/quality/locked-code.json"))
        .unwrap_or_default()
        .replace("\r\n", "\n");
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    match arg(0).as_str() {
        "connect" => {
            if std::env::var("FAKE_HARNESS_FAIL").as_deref() == Ok("1") {
                eprintln!("fake harness refused to connect");
                std::process::exit(3);
            }
            let note = format!("{}\n", args[1..].join("|"));
            std::fs::write(format!("{}/harness-connected", arg(2)), note).unwrap();
        }
        "verify" => {
            println!("probing {} ...", arg(1));
            let mode = std::env::var("FAKE_HARNESS_VERIFY").unwrap_or_else(|_| "denied".into());
            let report = match mode.as_str() {
                "unsupported" => {
                    serde_json::json!({"result": "not-supported", "reason": "fake client loads no hooks"})
                }
                other => serde_json::json!({
                    "result": "denied-live-probe",
                    "provider": arg(1),
                    "home": arg(2),
                    "policyRevision": if other == "forged" { "0".repeat(64) } else { revision(&arg(3)) },
                }),
            };
            println!("{report}");
        }
        "manifest" => print!(
            "{}",
            std::env::var("FAKE_HARNESS_MANIFEST").unwrap_or_default()
        ),
        "echo-args" => {
            for a in &args[1..] {
                println!("{a}");
            }
        }
        "hang" => std::thread::sleep(std::time::Duration::from_secs(60)),
        "flood" => {
            let line = "x".repeat(99);
            for _ in 0..3000 {
                println!("{line}");
            }
        }
        _ => {
            eprintln!("unknown fake harness mode");
            std::process::exit(2);
        }
    }
}
