//! Stand-in for the codex/claude/agent CLIs so the real turn runner can be exercised end to end
//! without a provider or a subscription. Behavior is chosen by `FAKE_PROVIDER_MODE`:
//!
//! - `stream`: print the JSONL file named by `FAKE_PROVIDER_STREAM`, exit 0.
//! - `hang`:   write our pid to `FAKE_PROVIDER_PID`, print a session line, then sleep.
//! - `silent-hang`: write our pid to `FAKE_PROVIDER_PID`, then sleep without output.
//! - `fail`:   print to stderr and exit 3.
//! - app-server fixtures: answerable Codex question/approval requests from
//!   `appserver-fixture.json`, including an optional question-then-approval sequence.
//!
//! In every mode, if `FAKE_PROVIDER_OUT` is set we record what we received: one `ARG:<hex>` line
//! per argument (hex so newlines and quotes survive), the isolating env vars, our cwd, and the
//! length of anything piped on stdin.

use std::io::{Read, Write};

#[cfg(windows)]
#[allow(clippy::zombie_processes)]
fn maybe_probe_grandchild() {
    if std::env::var_os("FAKE_PROVIDER_GRANDCHILD").is_some() {
        use windows_sys::Win32::System::Console::GetConsoleWindow;
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        let hwnd = unsafe { GetConsoleWindow() };
        let visible = !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) != 0 };
        let out = std::env::var("FAKE_PROVIDER_GRANDCHILD_OUT").unwrap();
        std::fs::write(out, format!("GRANDCHILD_VISIBLE={}\n", u8::from(visible))).unwrap();
        std::process::exit(0);
    }
    if std::env::var_os("FAKE_PROVIDER_GRANDCHILD_OUT").is_some() {
        let mut child_env = std::env::vars().collect::<Vec<_>>();
        child_env.push(("FAKE_PROVIDER_GRANDCHILD".into(), "1".into()));
        std::process::Command::new(std::env::current_exe().unwrap())
            .envs(child_env)
            .spawn()
            .unwrap();
    }
}

#[cfg(not(windows))]
fn maybe_probe_grandchild() {}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn fake_acp() {
    use std::io::BufRead;
    let root = std::env::current_dir().unwrap();
    let fixture: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("acp-fixture.json"))
            .expect("ACP fake requires an explicit fixture"),
    )
    .unwrap();
    if fixture["history"].is_array() {
        std::fs::write(
            root.join("acp-environment.json"),
            serde_json::json!({
                "config_dir":std::env::var("CURSOR_CONFIG_DIR").ok().or_else(|| std::env::var("GROK_HOME").ok()),
                "api_key_present":std::env::var_os("CURSOR_API_KEY").is_some(),
                "auth_token_present":std::env::var_os("CURSOR_AUTH_TOKEN").is_some(),
            })
            .to_string(),
        )
        .unwrap();
    }
    let emit = |value: serde_json::Value| {
        println!("{value}");
        std::io::stdout().flush().unwrap();
    };
    let finish = |session_id: &str| {
        emit(
            serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session_id,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":fixture["reply"].as_str().unwrap_or("Native image received")}}}}),
        );
        emit(serde_json::json!({"jsonrpc":"2.0","id":5,"result":{"stopReason":"end_turn"}}));
    };
    let mut session_id = "image-session".to_owned();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        let mut journal = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(root.join("acp-calls.jsonl"))
            .unwrap();
        writeln!(journal, "{line}").unwrap();
        if value["id"] == 100 && value.get("method").is_none() {
            finish(&session_id);
            continue;
        }
        let result = match value["method"].as_str().unwrap_or("") {
            "initialize" => {
                serde_json::json!({"protocolVersion":1,"agentCapabilities":{"loadSession":fixture["load_session"]!=false,"promptCapabilities":{"image":fixture["image"]!=false}}})
            }
            "session/new" => serde_json::json!({"sessionId":session_id}),
            "session/load" => {
                session_id = value["params"]["sessionId"].as_str().unwrap().to_owned();
                if let Some(history) = fixture["history"].as_array() {
                    for entry in history {
                        let update = if entry["role"] == "user" {
                            "user_message_chunk"
                        } else {
                            "agent_message_chunk"
                        };
                        let replay_session = if fixture["wrong_history_session"] == true {
                            "another-session"
                        } else {
                            &session_id
                        };
                        emit(
                            serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":replay_session,"update":{"sessionUpdate":update,"messageId":entry["id"],"content":{"type":"text","text":entry["text"]}}}}),
                        );
                    }
                }
                if fixture["history_tool_request"] == true {
                    emit(
                        serde_json::json!({"jsonrpc":"2.0","id":100,"method":"terminal/create","params":{"sessionId":session_id,"command":"must-not-run"}}),
                    );
                    continue;
                }
                if fixture["history_activity"] == true {
                    for update in [
                        serde_json::json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Historical thinking"}}),
                        serde_json::json!({"sessionUpdate":"tool_call","toolCallId":"historical-tool","title":"Historical read","status":"completed"}),
                    ] {
                        emit(
                            serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session_id,"update":update}}),
                        );
                    }
                }
                serde_json::json!({})
            }
            "authenticate" | "session/set_mode" | "session/set_model" | "session/set_config_option" => serde_json::json!({}),
            "session/prompt" => {
                if fixture["quota"] == true {
                    emit(
                        serde_json::json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":429,"message":"UsageLimitExceeded"}}),
                    );
                    continue;
                }
                if fixture["plan"] == true {
                    emit(
                        serde_json::json!({"jsonrpc":"2.0","id":100,"method":"cursor/create_plan","params":{"toolCallId":"plan-review","name":"Review worker plan","plan":"Inspect the parser.\nEdit only the requested function.\nRun the scoped check.","todos":[{"id":"inspect","content":"Inspect parser","status":"pending"}]}}),
                    );
                } else if fixture["permission"] == true {
                    emit(
                        serde_json::json!({"jsonrpc":"2.0","id":100,"method":"session/request_permission","params":{"sessionId":session_id,"toolCall":{"title":fixture["permission_title"].as_str().unwrap_or("Read attached image"),"toolCallId":"image-read","rawInput":{"path":"synthetic-input"}},"options":[{"optionId":"yes","name":"Allow once","kind":"allow_once"},{"optionId":"no","name":"Decline","kind":"reject_once"}]}}),
                    );
                } else {
                    finish(&session_id);
                }
                continue;
            }
            _ => panic!("unexpected ACP method"),
        };
        emit(serde_json::json!({"jsonrpc":"2.0","id":value["id"],"result":result}));
    }
}

fn fake_appserver() {
    use serde_json::{json, Value};
    use std::io::BufRead;
    let root = std::env::current_dir().unwrap();
    let fixture: Value =
        serde_json::from_slice(&std::fs::read(root.join("appserver-fixture.json")).unwrap())
            .unwrap();
    let emit = |value: Value| {
        println!("{value}");
        std::io::stdout().flush().unwrap();
    };
    let mut thread = "native-fixture-thread".to_owned();
    let mut question_answered = false;
    let mut approval_sent = false;
    let large_tool = |thread: &str| {
        if let Some(bytes) = fixture["large_tool_bytes"].as_u64() {
            emit(
                json!({"method":"item/completed","params":{"threadId":thread,"turnId":"native-turn","item":{"id":"large-output","type":"commandExecution","command":"synthetic large output","cwd":root,"aggregatedOutput":"x".repeat(bytes as usize),"status":"completed","exitCode":0}}}),
            );
        }
    };
    let finish = |thread: &str, answered: bool| {
        if let Some(gate) = fixture["finish_wait_for"].as_str() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !root.join(gate).exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "native message fixture completion gate timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        if answered && fixture["question"] != true {
            emit(
                json!({"method":"item/completed","params":{"threadId":thread,"turnId":"native-turn","item":{"id":"command-1","type":"commandExecution","command":"synthetic command","cwd":root,"status":"completed","exitCode":0}}}),
            );
        }
        if fixture["artifact"] == true {
            std::fs::write(root.join("native-output.md"), "Synthetic native output\n").unwrap();
            let event = json!({"method":"item/completed","params":{"threadId":thread,"turnId":"native-turn","item":{"id":"native-output-event","type":"fileChange","status":"completed","changes":[{"path":"native-output.md","kind":{"type":"add"},"diff":"+Synthetic native output"}]}}});
            emit(event.clone());
            emit(event);
        }
        let reply = if fixture["large_text"] == true {
            format!("{}Native approval handled", "é".repeat(100_000))
        } else {
            "Native approval handled".into()
        };
        emit(
            json!({"method":"item/agentMessage/delta","params":{"threadId":thread,"turnId":"native-turn","itemId":"reply","delta":reply}}),
        );
        emit(
            json!({"method":"turn/completed","params":{"threadId":thread,"turn":{"id":"native-turn","status":"completed"}}}),
        );
    };
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let value: Value = serde_json::from_str(&line).unwrap();
        let mut journal = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("appserver-calls.jsonl"))
            .unwrap();
        writeln!(journal, "{line}").unwrap();
        if value.get("method").is_none()
            && ((value["id"] == 100) || (approval_sent && value["id"] == 101))
        {
            if fixture["question_then_approval"] == true && !question_answered {
                question_answered = true;
                approval_sent = true;
                emit(json!({
                    "id": 101,
                    "method": "item/commandExecution/requestApproval",
                    "params": {
                        "threadId": thread,
                        "turnId": "native-turn",
                        "itemId": "command-1",
                        "command": "synthetic command",
                        "cwd": root,
                        "availableDecisions": ["accept", "acceptForSession", "decline", "cancel"]
                    }
                }));
            } else {
                finish(&thread, true);
            }
            continue;
        }
        let result = match value["method"].as_str().unwrap_or("") {
            "initialize" => {
                json!({"codexHome":std::env::var("CODEX_HOME").unwrap(),"userAgent":"codex-cli 0.166.0","platformFamily":"windows","platformOs":"windows"})
            }
            "initialized" => continue,
            "thread/start" | "thread/resume" => {
                if let Some(id) = value["params"]["threadId"].as_str() {
                    thread = id.into();
                }
                if fixture["wrong_thread"] == true {
                    thread = "foreign-thread".into();
                }
                json!({"thread":{"id":thread},"cwd":root,"approvalPolicy":value["params"]["approvalPolicy"],"approvalsReviewer":"user"})
            }
            "turn/start" => {
                // Deliberately arrive before the turn/start response: the client must buffer.
                // Question fixtures do not start a command. Permission fixtures keep
                // command lifetime distinct and complete it only after a native answer.
                if fixture["question"] != true {
                    emit(
                        json!({"method":"item/started","params":{"threadId":thread,"turnId":"native-turn","item":{"id":"command-1","type":"commandExecution","command":"synthetic command","cwd":root,"status":"inProgress"}}}),
                    );
                }
                if fixture["early_large"] == true {
                    large_tool(&thread);
                }
                emit(
                    json!({"id":value["id"],"result":{"turn":{"id":"native-turn","status":"inProgress"}}}),
                );
                if fixture["early_large"] != true {
                    large_tool(&thread);
                }
                if fixture["closed"] == true {
                    return;
                }
                if let Some(messages) = fixture["messages"].as_array() {
                    if let Some(bytes) = fixture["wire_bytes"].as_u64() {
                        let padding = "x".repeat(32 * 1024);
                        for _ in 0..bytes.div_ceil(padding.len() as u64) {
                            emit(
                                json!({"method":"synthetic/telemetry","params":{"threadId":thread,"turnId":"native-turn","padding":padding}}),
                            );
                        }
                    }
                    for (index, message) in messages.iter().enumerate() {
                        let id = format!("message-{index}");
                        let text = message.as_str().unwrap();
                        emit(
                            json!({"method":"item/started","params":{"threadId":thread,"turnId":"native-turn","item":{"id":id,"type":"agentMessage","text":""}}}),
                        );
                        emit(
                            json!({"method":"item/agentMessage/delta","params":{"threadId":thread,"turnId":"native-turn","itemId":id,"delta":text}}),
                        );
                        emit(
                            json!({"method":"item/completed","params":{"threadId":thread,"turnId":"native-turn","item":{"id":id,"type":"agentMessage","text":text}}}),
                        );
                    }
                    if let Some(gate) = fixture["wait_for"].as_str() {
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(30);
                        while !root.join(gate).exists() {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "native message fixture gate timed out"
                            );
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                    emit(
                        json!({"method":"turn/completed","params":{"threadId":thread,"turn":{"id":"native-turn","status":"completed"}}}),
                    );
                    continue;
                }
                let method = if fixture["question"] == true {
                    "item/tool/requestUserInput"
                } else {
                    "item/commandExecution/requestApproval"
                };
                let mut params = json!({"threadId":thread,"turnId":"native-turn","itemId":"command-1","command":"synthetic command","cwd":root,"availableDecisions":["accept","acceptForSession","decline","cancel"]});
                if fixture["question"] == true {
                    params["questions"] = json!([{"id":"route","header":"Route","question":"Choose the route","isSecret":false,"isOther":false,"options":[{"label":"Direct","description":"Use direct routing"},{"label":"Review","description":"Review first"}]}]);
                    if fixture["freeform"] == true {
                        params["questions"][0]["options"] = Value::Null;
                    }
                }
                if fixture["wrong_turn"] == true {
                    params["turnId"] = json!("foreign-turn");
                }
                let request = json!({"id":100,"method":method,"params":params});
                emit(request.clone());
                if fixture["duplicate"] == true {
                    emit(request.clone());
                }
                if fixture["resolved"] == true {
                    emit(
                        json!({"method":"serverRequest/resolved","params":{"threadId":thread,"requestId":100}}),
                    );
                    if fixture["duplicate"] == true {
                        emit(request);
                    }
                    finish(&thread, false);
                }
                continue;
            }
            _ => panic!("native fixture received an unexpected RPC"),
        };
        emit(json!({"id":value["id"],"result":result}));
    }
}

fn fake_claude_control(args: &[String]) {
    use serde_json::{json, Value};
    use std::io::BufRead;
    let root = std::env::current_dir().unwrap();
    let fixture: Value =
        serde_json::from_slice(&std::fs::read(root.join("claude-fixture.json")).unwrap()).unwrap();
    let session = args
        .windows(2)
        .find(|pair| matches!(pair[0].as_str(), "--resume" | "--session-id"))
        .map(|pair| pair[1].clone())
        .unwrap();
    let emit = |value: Value| {
        if fixture["background"] == true {
            let mut events = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(root.join("claude-events.jsonl"))
                .unwrap();
            writeln!(events, "{value}").unwrap();
        }
        println!("{value}");
        std::io::stdout().flush().unwrap();
    };
    let finish = || {
        emit(
            json!({"type":"result","subtype":"success","session_id":session,
        "is_error":false,"result":"Native Claude review handled","usage":{"input_tokens":12,"output_tokens":3}}),
        )
    };
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let value: Value = serde_json::from_str(&line).unwrap();
        let mut journal = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(root.join("claude-calls.jsonl"))
            .unwrap();
        writeln!(journal, "{line}").unwrap();
        if value["type"] == "control_request" {
            let subtype = value["request"]["subtype"].as_str().unwrap();
            let response = match subtype {
                "initialize" => {
                    json!({"claude_code_version":"2.1.319","session_state":"idle","capabilities":{}})
                }
                "get_context_usage" => {
                    json!({"totalTokens":fixture["context_used"].as_u64().unwrap_or(120),
                    "maxTokens":1000,"rawMaxTokens":1000,"percentage":fixture["context_used"].as_u64().unwrap_or(120) as f64 / 10.0,
                    "model":fixture["context_model"].as_str().unwrap_or("fixture-claude"),"sessionId":session,"isAutoCompactEnabled":true})
                }
                other => panic!("unexpected Claude control request: {other}"),
            };
            emit(
                json!({"type":"control_response","response":{"subtype":if fixture["bad_init"] == true {"error"} else {"success"},
                "request_id":value["request_id"],"response":response}}),
            );
        } else if value["type"] == "user" {
            if fixture.get("context_used").is_some() || fixture.get("context_model").is_some() {
                let home = std::path::PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap());
                let folder = home.join("projects/fake-provider");
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(folder.join(format!("{session}.jsonl")), format!("{}\n{}\n",
                    json!({"type":"user","sessionId":session,"cwd":root,"message":{"role":"user","content":[{"type":"text","text":"Synthetic context fixture"}]}}),
                    json!({"type":"assistant","sessionId":session,"message":{"role":"assistant","content":[{"type":"text","text":"Completed fixture"}]}}))).unwrap();
            }
            emit(
                json!({"type":"system","subtype":"init","session_id":session,"model":"fixture-claude"}),
            );
            if fixture["background"] == true {
                emit(
                    json!({"type":"system","subtype":"task_started","task_id":"task-1","description":"Synthetic background agent","uuid":"task-start-1","session_id":session,"tool_use_id":"task-tool-1","task_type":"local_agent"}),
                );
                emit(
                    json!({"type":"result","subtype":"success","session_id":session,"is_error":false,"result":"Native Claude main result","usage":{"input_tokens":12,"output_tokens":3}}),
                );
                std::thread::sleep(std::time::Duration::from_millis(2100));
                emit(
                    json!({"type":"system","subtype":"session_state_changed","session_id":session,"state":"running","uuid":"state-running-1"}),
                );
                emit(
                    json!({"type":"control_request","request_id":"late-permission","request":{"subtype":"can_use_tool","tool_name":"Write","tool_use_id":"late-write-1","input":{"file_path":"background-output.md","content":"Synthetic background bytes"}}}),
                );
                if fixture["cancel"] == true {
                    emit(json!({"type":"control_cancel_request","request_id":"late-permission"}));
                    return;
                }
                if fixture["closed"] == true {
                    return;
                }
                continue;
            }
            let request_id = if fixture["questions"] == true {
                "question-1"
            } else {
                "permission-1"
            };
            let mut native = json!({"type":"control_request","request_id":request_id,"request":{
                "subtype":"can_use_tool","tool_name":"Write","tool_use_id":"write-1",
                "input":{"file_path":"synthetic-output.md","content":"Synthetic bytes"}}});
            if fixture["questions"] == true {
                native["request"]["tool_name"] = json!("AskUserQuestion");
                native["request"]["tool_use_id"] = json!("ask-1");
                native["request"]["input"] = if fixture["malformed"] == true {
                    json!({"questions":[{"question":"Choose a route","multiSelect":"yes","options":[]}]})
                } else {
                    if fixture["four_questions"] == true {
                        json!([
                        {"question":"Choose a route","header":"Route","multiSelect":false,
                         "options":[{"label":"Direct","description":"Use direct routing"},{"label":"Review","description":"Review first"}]},
                        {"question":"Choose colors","header":"Colors","multiSelect":true,
                         "options":[{"label":"Red","description":"Use red"},{"label":"Blue","description":"Use blue"},{"label":"Green","description":"Use green"},{"label":"Yellow","description":"Use yellow"}]},
                        {"question":"Add context","header":"Context","multiSelect":false,
                         "options":[{"label":"Keep","description":"Keep context"},{"label":"Drop","description":"Drop context"}]},
                        {"question":"Route note","header":"Note","multiSelect":true,
                         "options":[{"label":"One","description":"First"},{"label":"Two","description":"Second"}]}
                        ])
                    } else {
                        json!([
                        {"question":"Choose a route","header":"Route","multiSelect":false,
                         "options":[{"label":"Direct","description":"Use direct routing"},{"label":"Review","description":"Review first"}]},
                        {"question":"Choose colors","header":"Colors","multiSelect":true,
                         "options":[{"label":"Red","description":"Use red"},{"label":"Blue","description":"Use blue"},{"label":"Green","description":"Use green"}]}
                        ])
                    }
                };
                if fixture["malformed"] != true {
                    native["request"]["input"] =
                        json!({"questions": native["request"]["input"].clone()});
                }
            }
            if fixture["background"] == true {
                continue;
            }
            if fixture["unknown"] == true {
                native["request"]["subtype"] = json!("future_native_method");
            }
            if fixture["model_only"] == true {
                emit(
                    json!({"type":"assistant","message":{"content":[{"type":"text","text":native.to_string()}]}}),
                );
                finish();
            } else {
                if fixture["fragmented"] == true {
                    let text = native.to_string();
                    let split = text.len() / 2;
                    print!("{}", &text[..split]);
                    std::io::stdout().flush().unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(35));
                    println!("{}", &text[split..]);
                    std::io::stdout().flush().unwrap();
                } else {
                    emit(native.clone());
                }
                if fixture["duplicate"] == true {
                    emit(native);
                }
                if fixture["cancel"] == true {
                    emit(json!({"type":"control_cancel_request","request_id":request_id}));
                    finish();
                }
                if fixture["closed"] == true {
                    return;
                }
            }
        } else if value["type"] == "control_response" {
            if fixture["background"] == true {
                assert_eq!(value["response"]["request_id"], "late-permission");
                if fixture["foreign"] != true {
                    emit(
                        json!({"type":"system","subtype":"task_updated","task_id":"task-1",
                        "session_id":session,"patch":{"status":"completed"}}),
                    );
                }
                emit(
                    json!({"type":"system","subtype":"task_notification","task_id":"task-1","status":"completed","output_file":"background-output.md","summary":"Synthetic background task completed","uuid":"task-done-1","session_id":if fixture["foreign"] == true { "foreign-session" } else { &session },"tool_use_id":"task-tool-1","usage":{"total_tokens":7,"tool_uses":1,"duration_ms":2100}}),
                );
                if fixture["foreign"] == true {
                    continue;
                }
                emit(
                    json!({"type":"result","subtype":"success","session_id":session,"is_error":false,"result":"Native Claude follow-up result","usage":{"input_tokens":8,"output_tokens":2}}),
                );
                emit(
                    json!({"type":"system","subtype":"session_state_changed","session_id":session,"state":"idle","uuid":"state-idle-1"}),
                );
                continue;
            }
            let expected_request_id = if fixture["questions"] == true {
                "question-1"
            } else {
                "permission-1"
            };
            assert_eq!(value["response"]["request_id"], expected_request_id);
            finish();
        }
    }
}

fn main() {
    maybe_probe_grandchild();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--version")
        && std::env::var("FAKE_PROVIDER_NATIVE_CLAUDE").as_deref() == Ok("1")
    {
        println!("2.1.319 (Claude Code)");
        return;
    }
    if args.iter().any(|arg| arg == "--permission-prompt-tool")
        && std::env::current_dir()
            .unwrap()
            .join("claude-fixture.json")
            .is_file()
    {
        fake_claude_control(&args);
        return;
    }
    if args.first().is_some_and(|arg| arg == "--version")
        && std::env::var("FAKE_PROVIDER_NATIVE_CODEX").as_deref() == Ok("1")
    {
        println!("codex-cli 0.166.0");
        return;
    }
    if args.first().is_some_and(|a| a == "acp")
        || args.windows(2).any(|pair| pair == ["agent", "stdio"])
    {
        fake_acp();
        return;
    }
    if args.first().is_some_and(|a| a == "app-server") {
        if std::env::current_dir()
            .unwrap()
            .join("appserver-fixture.json")
            .is_file()
        {
            fake_appserver();
            return;
        }
        use std::io::BufRead;
        let home = std::path::PathBuf::from(std::env::var_os("CODEX_HOME").unwrap());
        let fixture: serde_json::Value = serde_json::from_slice(
            &std::fs::read(home.join("rpc-fixture.json")).unwrap_or_default(),
        )
        .unwrap_or(serde_json::Value::Null);
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            let mut journal = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(home.join("rpc-calls.jsonl"))
                .unwrap();
            writeln!(journal, "{line}").unwrap();
            match request["method"].as_str().unwrap_or("") {
                "initialize" => println!(
                    "{}",
                    serde_json::json!({"id":request["id"],"result":{"userAgent":"synthetic","codexHome":home}})
                ),
                "initialized" => continue,
                "account/rateLimits/read" => {
                    if fixture["mode"] == "closed" {
                        return;
                    }
                    if fixture["mode"] == "flood" {
                        println!("{}", "x".repeat(600 * 1024));
                        return;
                    }
                    if fixture["mode"] == "error" {
                        println!(
                            "{}",
                            serde_json::json!({"id":request["id"],"error":{"code":-1,"message":"synthetic auth required"}})
                        );
                    } else {
                        println!(
                            "{}",
                            serde_json::json!({"id":request["id"],"result":fixture})
                        );
                    }
                }
                _ => panic!("quota probe attempted a forbidden RPC"),
            }
            std::io::stdout().flush().unwrap();
        }
        return;
    }
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
        if args.iter().any(|a| a == "/usage") {
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
        "stream" | "gated-stream" => {
            if let Ok(path) = std::env::var("FAKE_PROVIDER_STREAM") {
                let body = std::fs::read_to_string(path).unwrap_or_default();
                for line in body.lines() {
                    let _ = writeln!(stdout, "{line}");
                    let _ = stdout.flush();
                    if mode == "gated-stream" && line.contains("item.started") {
                        let gate = std::env::var("FAKE_PROVIDER_GATE").expect("fake stream gate");
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(15);
                        while !std::path::Path::new(&gate).exists() {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "fake stream gate timed out"
                            );
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                }
            }
        }
        "hang" | "silent-hang" | "eof-hang" => {
            #[cfg(windows)]
            if mode == "eof-hang" {
                use std::ffi::c_void;
                #[link(name = "kernel32")]
                extern "system" {
                    fn GetStdHandle(kind: u32) -> *mut c_void;
                    fn CloseHandle(handle: *mut c_void) -> i32;
                }
                // SAFETY: this fake intentionally closes its own stdout so the runner sees
                // EOF while the process remains alive. No further output uses that handle.
                unsafe {
                    CloseHandle(GetStdHandle(-11_i32 as u32));
                }
                if let Ok(path) = std::env::var("FAKE_PROVIDER_PID") {
                    let _ = std::fs::write(path, std::process::id().to_string());
                }
                std::thread::sleep(std::time::Duration::from_secs(120));
                return;
            }
            if let Ok(path) = std::env::var("FAKE_PROVIDER_PID") {
                let _ = std::fs::write(path, std::process::id().to_string());
            }
            if mode == "hang" {
                let _ = writeln!(
                    stdout,
                    r#"{{"type":"thread.started","thread_id":"99999999-2222-3333-4444-555555555555"}}"#
                );
                let _ = stdout.flush();
            }
            std::thread::sleep(std::time::Duration::from_secs(120));
        }
        "drip-stdout" | "drip-stderr" => {
            if mode == "drip-stderr" {
                let mut stream = std::io::stderr();
                for i in 0..12 {
                    let _ = writeln!(stream, "drip {i}");
                    let _ = stream.flush();
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            } else {
                for i in 0..12 {
                    let _ = writeln!(stdout, "drip {i}");
                    let _ = stdout.flush();
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(120));
        }
        // Megabytes of stderr before and after normal output: deadlocks a runner that never
        // drains the pipe (the OS pipe buffer is only ~64 KB).
        "flood" | "stderr-flood-no-newline" => {
            let mut stderr = std::io::stderr();
            if mode == "stderr-flood-no-newline" {
                let _ = write!(stderr, "{}", "x".repeat(3 * 1024 * 1024));
            } else {
                for i in 0..40_000 {
                    let _ = writeln!(stderr, "flood line {i} {}", "x".repeat(60));
                }
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
    let mut prompt = args.last().cloned().unwrap_or_default();
    let mut image_count = args.iter().filter(|a| *a == "--image").count();
    if args
        .windows(2)
        .any(|v| v == ["--input-format", "stream-json"])
    {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        let input: serde_json::Value = serde_json::from_str(&input).unwrap();
        let content = input["message"]["content"].as_array().unwrap();
        prompt = content
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        image_count = content.iter().filter(|c| c["type"] == "image").count();
    } else if prompt == "-" {
        prompt.clear();
        std::io::stdin().read_to_string(&mut prompt).unwrap();
    }
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
                    json!({"kind":kind,"provider":provider,"prompt":prompt,"image_count":image_count,"args":args,"cwd":std::env::current_dir().ok(),"home":std::env::var(if provider == "claude" { "CLAUDE_CONFIG_DIR" } else { "CODEX_HOME" }).ok()})
                );
            }
        }
    };
    let emit = |event: serde_json::Value| {
        println!("{event}");
        let _ = std::io::stdout().flush();
    };
    journal("start");
    if prompt.contains("[context]")
        && provider == "codex"
        && !prompt.contains("Continue the interrupted request in this new native session")
    {
        emit(json!({"type":"thread.started","thread_id":"capacity-source-session"}));
        emit(
            json!({"type":"turn.failed","error":{"code":"context_length_exceeded","message":"Synthetic input capacity exhausted"}}),
        );
        journal("limited");
        std::process::exit(1);
    }
    if prompt.contains("[quota]") && provider == "codex" {
        emit(json!({"type":"thread.started","thread_id":"quota-source-session"}));
        if prompt.contains("[coded-limit]") {
            emit(
                json!({"type":"turn.failed","error":{"code":-32000,"codexErrorInfo":"usageLimitExceeded","message":"Request cannot be completed"}}),
            );
        } else {
            emit(json!({"type":"turn.failed","error":{"message":"You've hit your usage limit"}}));
        }
        journal("limited");
        std::process::exit(1);
    }
    let thought = format!(
        "reasoning-start {} reasoning-end",
        "checking the requested changes. ".repeat(30)
    );
    let command = format!("synthetic-command {} command-tail", "argument ".repeat(15));
    let tool_output = if prompt.contains("[long-tool]") {
        format!(
            "tool-output-start\n{}\ntool-output-end",
            (0..120)
                .map(|i| format!("synthetic output line {i}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    } else {
        "tool-output-start\ntool-output-end".into()
    };

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
    if provider == "codex" && prompt.contains("[agents]") {
        emit(
            json!({"type":"item.completed","item":{"type":"collab_tool_call","id":"spawn-child","tool":"spawn_agent","sender_thread_id":"parent","receiver_thread_ids":["child-reviewer"],"prompt":"Inspect the graph parser","agents_states":{"child-reviewer":{"status":"running","message":null}},"status":"completed"}}),
        );
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
    // Keep the raw provider prompt in the journal, but echo the user's text in the reply.
    let reply = if concordia::prompt_time::strip_header(&prompt).trim() == "[mermaid]" {
        "```mermaid\nflowchart LR\n A[Prompt] --> B[Provider]\n```".into()
    } else if concordia::prompt_time::strip_header(&prompt).trim() == "[choices]" {
        "Which approach do you prefer?\n1. Small patch\n2. Full refactor".into()
    } else {
        format!(
            "FAKE REPLY: {}",
            concordia::prompt_time::strip_header(&prompt)
        )
    };
    if provider == "codex" && prompt.contains("[agents]") {
        emit(
            json!({"type":"item.completed","item":{"type":"collab_tool_call","id":"wait-child","tool":"wait","sender_thread_id":"parent","receiver_thread_ids":["child-reviewer"],"agents_states":{"child-reviewer":{"status":"completed","message":"GRAPH_REVIEW_FINISHED"}},"status":"completed"}}),
        );
    }
    match provider {
        "codex" => {
            emit(
                json!({"type":"item.completed","item":{"type":"command_execution","id":"tool","command":command,"aggregated_output":tool_output}}),
            );
            emit(
                json!({"type":"item.completed","item":{"type":"agent_message","id":"reply","text":reply}}),
            );
            emit(json!({"type":"turn.completed","usage":{"input_tokens":123,"output_tokens":45}}));
        }
        "claude" => {
            emit(
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool","content":tool_output}]}}),
            );
            emit(json!({"type":"assistant","message":{"content":[{"type":"text","text":reply}]}}));
            emit(
                json!({"type":"result","subtype":"success","result":reply,"usage":{"input_tokens":123,"output_tokens":45}}),
            );
        }
        _ => {
            emit(
                json!({"type":"tool_call","subtype":"completed","call_id":"tool","tool_call":{"shellToolCall":{"args":{"command":command},"result":{"success":{"output":tool_output}}}}}),
            );
            emit(json!({"type":"assistant","message":{"content":[{"type":"text","text":reply}]}}));
            emit(json!({"type":"result","subtype":"success","result":reply}));
        }
    }
    journal("done");
}
