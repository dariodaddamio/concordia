//! The narrow two-way bridge for cloud-hosted Grok Bot routines.
//!
//! Routine metadata is local state; webhook keys and callback tokens are Windows Credential
//! Manager secrets. The callback listener intentionally has no chat, memory, or provider tools.

use crate::mcp::{update_agent, AgentRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_CALLBACK_BYTES: usize = 64 * 1024;
pub const MAX_CALLBACKS_PER_MINUTE: u64 = 30;
static CALLBACKS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub provider: String,
    pub account: String,
    pub routine_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub webhook_url: String,
}

fn routines_path(root: &Path) -> PathBuf {
    root.join("grok-bot-routines.json")
}
fn secret_name(r: &Routine) -> String {
    format!(
        "Concordia/GrokBot/{}/{}/{}",
        r.provider, r.account, r.routine_id
    )
}
fn callback_secret_name(root: &Path) -> String {
    let root_text = root.to_string_lossy();
    let hash = crate::redact::redact(&root_text);
    format!("Concordia/GrokBot/Callback/{}", sha256_hex(hash.as_bytes()))
}

pub fn list(root: &Path) -> Vec<Routine> {
    std::fs::read(routines_path(root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn register(root: &Path, mut routine: Routine, bearer_key: &str) -> Result<(), String> {
    if routine.provider != "grok-bot"
        || routine.account.trim().is_empty()
        || routine.routine_id.trim().is_empty()
    {
        return Err("Grok Bot identity needs provider=grok-bot, account and routine ID".into());
    }
    validate_url(&routine.webhook_url)?;
    if bearer_key.trim().is_empty() {
        return Err("Grok Bot webhook key cannot be empty".into());
    }
    routine.webhook_url = routine.webhook_url.trim().into();
    let mut entries = list(root);
    entries.retain(|r| {
        !(r.provider == routine.provider
            && r.account == routine.account
            && r.routine_id == routine.routine_id)
    });
    put_secret(&secret_name(&routine), bearer_key)?;
    entries.push(routine);
    crate::state_io::write_atomic(
        &routines_path(root),
        &serde_json::to_vec_pretty(&entries).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    ensure_callback_token(root)?;
    start_listener(root);
    Ok(())
}

pub fn callback_token(root: &Path) -> Result<String, String> {
    ensure_callback_token(root)?;
    get_secret(&callback_secret_name(root))
}

pub fn rotate_callback_token(root: &Path) -> Result<String, String> {
    let token = new_token();
    put_secret(&callback_secret_name(root), &token)?;
    Ok(token)
}

fn ensure_callback_token(root: &Path) -> Result<(), String> {
    if get_secret(&callback_secret_name(root)).is_err() {
        put_secret(&callback_secret_name(root), &new_token())?;
    }
    Ok(())
}

pub fn start(root: &Path, routine_id: &str, task: &str, parent: &str) -> Result<String, String> {
    let routine = list(root)
        .into_iter()
        .find(|r| r.routine_id == routine_id)
        .ok_or_else(|| format!("unknown Grok Bot routine: {routine_id}"))?;
    let key = get_secret(&secret_name(&routine))?;
    let agent_id = uuid::Uuid::new_v4().to_string();
    let record = AgentRecord {
        agent_id: agent_id.clone(),
        account: routine.account.clone(),
        provider: "grok-bot".into(),
        model: None,
        task: task.into(),
        parent: parent.into(),
        status: "starting".into(),
        result: None,
        activity: vec!["Grok Bot routine webhook requested".into()],
    };
    crate::mcp::write_agent(root, &record)?;
    match post_json(
        &routine.webhook_url,
        &key,
        &json!({"task": task, "concordia_run_id": agent_id}),
    ) {
        Ok(()) => {
            update_agent(root, &agent_id, "waiting", None, "waiting for Grok Bot");
            Ok(agent_id)
        }
        Err(error) => {
            update_agent(
                root,
                &agent_id,
                "unreachable",
                None,
                &format!("Grok Bot unreachable: {error}; result remains in Grok Bot chat"),
            );
            Err(error)
        }
    }
}

pub fn start_listener(root: &Path) {
    if list(root).is_empty() {
        return;
    }
    let root = root.to_path_buf();
    let _ = thread::Builder::new()
        .name("concordia-grok-callback".into())
        .spawn(move || {
            let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
                return;
            };
            let _ = listener.set_nonblocking(false);
            let _ = std::fs::write(
                root.join("grok-bot-callback-address"),
                format!(
                    "http://{}/mcp\n",
                    listener
                        .local_addr()
                        .map(|a| a.to_string())
                        .unwrap_or_default()
                ),
            );
            for stream in listener.incoming().flatten() {
                let root = root.clone();
                let _ = thread::spawn(move || {
                    let _ = serve_callback(stream, &root);
                });
            }
        });
}

fn serve_callback(mut stream: TcpStream, root: &Path) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    while bytes.len() <= MAX_CALLBACK_BYTES {
        let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    let response = callback_http(&bytes, root);
    stream
        .write_all(response.as_bytes())
        .map_err(|e| e.to_string())
}

pub fn callback_http(request: &[u8], root: &Path) -> String {
    if request.len() > MAX_CALLBACK_BYTES {
        return http_error(413, "request too large");
    }
    if CALLBACKS.fetch_add(1, Ordering::Relaxed) >= MAX_CALLBACKS_PER_MINUTE {
        CALLBACKS.store(0, Ordering::Relaxed);
        return http_error(429, "rate limit exceeded");
    }
    let text = String::from_utf8_lossy(request);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return http_error(400, "malformed request");
    };
    let auth = head
        .lines()
        .find_map(|l| {
            l.strip_prefix("Authorization: ")
                .or_else(|| l.strip_prefix("authorization: "))
        })
        .unwrap_or("");
    let expected = callback_token(root).unwrap_or_default();
    if expected.is_empty() || auth != format!("Bearer {expected}") {
        return http_error(401, "invalid callback token");
    }
    let msg: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return http_error(400, "invalid JSON"),
    };
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let result = if msg.get("method").and_then(Value::as_str) == Some("tools/list") {
        Ok(
            json!({"tools":[{"name":"grok_bot_callback","description":"Post one untrusted Grok Bot result or question for a Concordia run.","inputSchema":{"type":"object","additionalProperties":false,"required":["run_id","kind","content"],"properties":{"run_id":{"type":"string"},"kind":{"enum":["result","question"]},"content":{"type":"string"}}}}]}),
        )
    } else if msg.get("method").and_then(Value::as_str) == Some("tools/call") {
        callback_call(root, msg.get("params").unwrap_or(&Value::Null))
    } else {
        Err("method not found".into())
    };
    let body = match result {
        Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":e}}),
    };
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.to_string().len(), body)
}

fn callback_call(root: &Path, params: &Value) -> Result<Value, String> {
    if params.get("name").and_then(Value::as_str) != Some("grok_bot_callback") {
        return Err("unknown tool".into());
    }
    let args = params.get("arguments").unwrap_or(&Value::Null);
    let run_id = args
        .get("run_id")
        .and_then(Value::as_str)
        .ok_or("run_id required")?;
    let kind = args
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("kind required")?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or("content required")?;
    if content.chars().count() > MAX_CALLBACK_BYTES {
        return Err("content too large".into());
    }
    let record = crate::mcp::read_agent(root, run_id)?;
    if record.provider != "grok-bot" {
        return Err("run was not started by Grok Bot".into());
    }
    let safe = crate::delegate::fence_untrusted(content);
    match kind {
        "result" => update_agent(
            root,
            run_id,
            "finished",
            Some(safe),
            "Grok Bot result received",
        ),
        "question" => update_agent(
            root,
            run_id,
            "needs_attention",
            Some(safe),
            "Grok Bot question received",
        ),
        _ => return Err("kind must be result or question".into()),
    }
    Ok(json!({"accepted":true}))
}

fn validate_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        Err("webhook URL must use http or https".into())
    } else {
        Ok(())
    }
}
fn post_json(url: &str, key: &str, payload: &Value) -> Result<(), String> {
    validate_url(url)?;
    if url.starts_with("https://") {
        return post_json_https(url, key, payload);
    }
    let without = url.strip_prefix("http://").unwrap();
    let (hostport, path) = without
        .split_once('/')
        .map_or((without, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
    let mut parts = hostport.split(':');
    let host = parts.next().unwrap_or("");
    let port = parts.next().and_then(|p| p.parse().ok()).unwrap_or(80);
    let mut stream = TcpStream::connect((host, port)).map_err(|e| e.to_string())?;
    let body = payload.to_string();
    let request = format!("POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    if response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        Err(format!(
            "webhook returned {}",
            response.lines().next().unwrap_or("unknown response")
        ))
    }
}
#[cfg(windows)]
fn post_json_https(url: &str, key: &str, payload: &Value) -> Result<(), String> {
    use std::iter::once;
    use windows_sys::Win32::Networking::WinHttp::*;
    let without = url
        .strip_prefix("https://")
        .ok_or("invalid HTTPS webhook URL")?;
    let (hostport, path) = without
        .split_once('/')
        .map_or((without, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
    let mut parts = hostport.split(':');
    let host = parts.next().unwrap_or("");
    let port = parts.next().and_then(|p| p.parse().ok()).unwrap_or(443);
    let wide = |s: &str| s.encode_utf16().chain(once(0)).collect::<Vec<u16>>();
    let agent = wide("Concordia");
    let host_w = wide(host);
    let path_w = wide(&path);
    let verb = wide("POST");
    let session = unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    if session.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let connection = unsafe { WinHttpConnect(session, host_w.as_ptr(), port, 0) };
    if connection.is_null() {
        unsafe {
            WinHttpCloseHandle(session);
        }
        return Err(std::io::Error::last_os_error().to_string());
    }
    let request = unsafe {
        WinHttpOpenRequest(
            connection,
            verb.as_ptr(),
            path_w.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    };
    if request.is_null() {
        unsafe {
            WinHttpCloseHandle(connection);
            WinHttpCloseHandle(session);
        }
        return Err(std::io::Error::last_os_error().to_string());
    }
    let headers = wide(&format!(
        "Authorization: Bearer {key}\r\nContent-Type: application/json\r\n"
    ));
    let body = payload.to_string();
    let ok = unsafe {
        WinHttpSendRequest(
            request,
            headers.as_ptr(),
            u32::MAX,
            body.as_ptr() as _,
            body.len() as u32,
            body.len() as u32,
            0,
        ) != 0
            && WinHttpReceiveResponse(request, std::ptr::null_mut()) != 0
    };
    let mut status = 0u32;
    let mut status_len = std::mem::size_of::<u32>() as u32;
    if ok {
        unsafe {
            WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                &mut status as *mut _ as _,
                &mut status_len,
                std::ptr::null_mut(),
            );
        }
    }
    unsafe {
        WinHttpCloseHandle(request);
        WinHttpCloseHandle(connection);
        WinHttpCloseHandle(session);
    }
    if ok && status == 200 {
        Ok(())
    } else if ok {
        Err(format!("webhook returned HTTP {status}"))
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}
#[cfg(not(windows))]
fn post_json_https(_: &str, _: &str, _: &Value) -> Result<(), String> {
    Err("HTTPS Grok Bot webhooks require Windows WinHTTP".into())
}
fn http_error(code: u16, message: &str) -> String {
    let body = json!({"error":message}).to_string();
    format!("HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}
fn new_token() -> String {
    format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        uuid::Uuid::new_v4()
    )
}
fn put_secret(name: &str, value: &str) -> Result<(), String> {
    credential_write(name, value)
}
fn get_secret(name: &str) -> Result<String, String> {
    credential_read(name)
}
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = <sha2::Sha256 as sha2::Digest>::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(windows)]
fn credential_write(name: &str, value: &str) -> Result<(), String> {
    use std::iter::once;
    use windows_sys::Win32::Security::Credentials::*;
    let target: Vec<u16> = name.encode_utf16().chain(once(0)).collect();
    let mut blob = value.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as _,
        Comment: std::ptr::null_mut(),
        LastWritten: windows_sys::Win32::Foundation::FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: std::ptr::null_mut(),
    };
    if unsafe { CredWriteW(&cred, 0) } == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
#[cfg(windows)]
fn credential_read(name: &str) -> Result<String, String> {
    use std::iter::once;
    use windows_sys::Win32::Security::Credentials::*;
    let target: Vec<u16> = name.encode_utf16().chain(once(0)).collect();
    let mut ptr = std::ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let cred = unsafe { &*ptr };
    let value = unsafe {
        std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)
    };
    let out = String::from_utf8(value.to_vec()).map_err(|e| e.to_string());
    unsafe { CredFree(ptr as _) };
    out
}
#[cfg(not(windows))]
fn credential_write(_: &str, _: &str) -> Result<(), String> {
    Err("Grok Bot credentials require Windows Credential Manager".into())
}
#[cfg(not(windows))]
fn credential_read(_: &str) -> Result<String, String> {
    Err("Grok Bot credentials require Windows Credential Manager".into())
}
