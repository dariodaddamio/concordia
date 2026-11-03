//! `concordia mcp` driven as a real child process over stdin/stdout, against a fake state folder.
//! No provider CLI is involved.

use concordia::threads::{ThreadEvent, ThreadLog, ThreadMeta};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

const CONCORDIA: &str = env!("CARGO_BIN_EXE_concordia");
const ID: &str = "c0ffee00-0000-4000-8000-000000000003";

fn seed(state: &std::path::Path) {
    let log = ThreadLog::open(state, ID);
    log.write_meta(&ThreadMeta {
        id: ID.into(),
        started: concordia::threads::now_secs(),
        title: "fix the login redirect".into(),
        cwd: String::new(),
        account: Some("claude-2".into()),
    })
    .unwrap();
    log.append(&ThreadEvent::with_text("user", "fix the login redirect"))
        .unwrap();
    log.append(&ThreadEvent::with_text(
        "assistant",
        "The redirect keeps the query string now.",
    ))
    .unwrap();
}

struct Client {
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
}

impl Client {
    fn send(&mut self, req: Value) {
        writeln!(self.stdin, "{req}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn ask(&mut self, req: Value) -> Value {
        self.send(req);
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad reply {line:?}: {e}"))
    }
}

#[test]
fn a_client_can_initialize_list_tools_and_call_them() {
    let state = tempfile::tempdir().unwrap();
    seed(state.path());
    let mut child = Command::new(CONCORDIA)
        .arg("mcp")
        .env("CONCORDIA_STATE_DIR", state.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut c = Client {
        stdin: child.stdin.take().unwrap(),
        out: BufReader::new(child.stdout.take().unwrap()),
    };

    let init = c.ask(json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-03-26","capabilities":{},
                  "clientInfo":{"name":"test","version":"0"}}}));
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["serverInfo"]["name"], "concordia");
    // A notification gets no reply: the next reply must belong to the next request.
    c.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));

    let list = c.ask(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    assert_eq!(list["id"], 2);
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "threads",
            "thread_map",
            "thread_read",
            "native_read",
            "usage",
            "goals",
            "delegate"
        ]
    );

    let map = c.ask(json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"thread_map","arguments":{"id":"c0ff"}}}));
    let text = map["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("[concordia map] thread c0ffee00-"),
        "{text}"
    );
    assert_eq!(map["result"]["isError"], false);

    let read = c.ask(json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
        "params":{"name":"thread_read","arguments":{"id":ID,"grep":"query string"}}}));
    assert!(read["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("keeps the query string"));

    // Delegation is off by default: the tool refuses and writes nothing.
    let del = c.ask(json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
        "params":{"name":"delegate","arguments":{"to":"codex-2","task":"t"}}}));
    assert_eq!(del["result"]["isError"], true);
    assert!(!state.path().join("proposals").exists());

    let bad = c.ask(json!({"jsonrpc":"2.0","id":6,"method":"nope"}));
    assert_eq!(bad["error"]["code"], -32601);

    drop(c);
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "the server exits cleanly when stdin closes"
    );
}
