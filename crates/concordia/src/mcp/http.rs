//! One MCP HTTP session. Native HTTP framing and TLS remain owned by WinHTTP on Windows.
use super::{KNOWN_PROTOCOLS, LATEST_PROTOCOL, MAX_UPSTREAM_BYTES};
use crate::mcp_registry::SecretRef;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEADERS: usize = 16 * 1024;

pub(super) struct HttpSession<'a> {
    url: &'a str,
    headers: &'a BTreeMap<String, SecretRef>,
    session_id: Option<String>,
    protocol: String,
    deadline: Instant,
}

impl<'a> HttpSession<'a> {
    pub(super) fn new(
        url: &'a str,
        headers: &'a BTreeMap<String, SecretRef>,
    ) -> Result<Self, String> {
        Endpoint::parse(url)?;
        Ok(Self {
            url,
            headers,
            session_id: None,
            protocol: LATEST_PROTOCOL.into(),
            deadline: Instant::now() + TIMEOUT,
        })
    }

    pub(super) fn request(&mut self, request: &Value) -> Result<Value, String> {
        let endpoint = Endpoint::parse(self.url)?;
        let mut headers = format!("Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {}\r\n", self.protocol);
        if let Some(id) = &self.session_id {
            headers.push_str(&format!("Mcp-Session-Id: {id}\r\n"));
        }
        for (key, reference) in self.headers {
            if key.is_empty()
                || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || matches!(
                    key.to_ascii_lowercase().as_str(),
                    "host"
                        | "content-length"
                        | "transfer-encoding"
                        | "mcp-session-id"
                        | "mcp-protocol-version"
                        | "accept"
                )
            {
                return Err("invalid or reserved MCP header name".into());
            }
            let value = crate::mcp_registry::resolve_secret(reference)
                .ok_or("remote MCP credential unavailable")?;
            if value.bytes().any(|b| b < 32 || b == 127) {
                return Err("invalid MCP header value".into());
            }
            crate::redact::register_runtime_secret(value.clone());
            headers.push_str(&format!("{key}: {value}\r\n"));
        }
        if headers.len() > MAX_HEADERS {
            return Err("MCP request headers exceed the size limit".into());
        }
        let body = serde_json::to_vec(request).map_err(|_| "MCP request could not be encoded")?;
        if body.len() > super::MAX_REQUEST_BYTES {
            return Err("MCP request exceeds the size limit".into());
        }
        #[cfg(windows)]
        let (head, value) = windows_request(&endpoint, &headers, &body, request, self.deadline)?;
        #[cfg(not(windows))]
        let (head, value) = plain_request(&endpoint, &headers, &body, request, self.deadline)?;
        if let Some(id) = header(&head, "mcp-session-id") {
            if id.is_empty() || id.len() > 1024 || !id.bytes().all(|b| (33..=126).contains(&b)) {
                return Err("invalid MCP session identifier".into());
            }
            self.session_id = Some(id.into());
        }
        if request.get("method").and_then(Value::as_str) == Some("initialize") {
            self.protocol = negotiated_protocol(&value)?;
        }
        Ok(value)
    }
}

pub(super) fn negotiated_protocol(value: &Value) -> Result<String, String> {
    let protocol = value
        .pointer("/result/protocolVersion")
        .and_then(Value::as_str)
        .filter(|protocol| KNOWN_PROTOCOLS.contains(protocol))
        .ok_or("MCP initialization failed or negotiated an unsupported protocol")?;
    Ok(protocol.into())
}

fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or("MCP request timed out".into())
}
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}

struct Endpoint {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}
impl Endpoint {
    fn parse(url: &str) -> Result<Self, String> {
        let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err("unsupported MCP URL".into());
        };
        if rest.chars().any(|c| c.is_control() || c.is_whitespace()) || rest.contains(['@', '#']) {
            return Err("invalid MCP URL".into());
        }
        let index = rest.find(['/', '?']).unwrap_or(rest.len());
        let authority = &rest[..index];
        let path = match &rest[index..] {
            "" => "/".into(),
            suffix if suffix.starts_with('?') => format!("/{suffix}"),
            suffix => suffix.into(),
        };
        let default = if secure { 443 } else { 80 };
        let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
            let (host, suffix) = bracketed.split_once(']').ok_or("invalid MCP host")?;
            let port = if suffix.is_empty() {
                default
            } else {
                suffix
                    .strip_prefix(':')
                    .ok_or("invalid MCP port")?
                    .parse()
                    .map_err(|_| "invalid MCP port")?
            };
            (host, port)
        } else if let Some((host, port)) = authority.split_once(':') {
            (host, port.parse().map_err(|_| "invalid MCP port")?)
        } else {
            (authority, default)
        };
        if host.is_empty() || port == 0 {
            return Err("invalid MCP host or port".into());
        }
        Ok(Self {
            secure,
            host: host.into(),
            port,
            path,
        })
    }
}

/// Return a matching response as soon as a complete SSE event arrives; an open SSE stream
/// need not reach EOF. Notifications and other response IDs cannot satisfy this request.
fn payload(
    bytes: &[u8],
    head: &str,
    request: &Value,
    complete: bool,
) -> Result<Option<Value>, String> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) if !complete && error.error_len().is_none() => return Ok(None),
        Err(_) => return Err("MCP response is not UTF-8".into()),
    };
    if header(head, "content-type")
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
    {
        let normalized = text.replace("\r\n", "\n");
        for event in normalized.split_inclusive("\n\n") {
            if !event.ends_with("\n\n") {
                continue;
            }
            let data = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|data| data.strip_prefix(' ').unwrap_or(data))
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() {
                continue;
            }
            let value: Value =
                serde_json::from_str(&data).map_err(|_| "MCP SSE data is not JSON")?;
            if value.get("id") == request.get("id") && request.get("id").is_some() {
                return Ok(Some(value));
            }
        }
        if complete {
            return Err("MCP SSE response had no matching response ID".into());
        }
        return Ok(None);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(value) => {
            if request.get("id").is_some() && value.get("id") != request.get("id") {
                return Err("MCP response ID mismatch".into());
            }
            Ok(Some(value))
        }
        Err(error) if !complete && error.is_eof() => Ok(None),
        Err(_) => Err("MCP response is not JSON".into()),
    }
}

#[cfg(windows)]
fn windows_request(
    endpoint: &Endpoint,
    headers: &str,
    body: &[u8],
    request: &Value,
    deadline: Instant,
) -> Result<(String, Value), String> {
    use windows_sys::Win32::Networking::WinHttp::*;
    struct Handle(*mut std::ffi::c_void);
    impl Handle {
        fn new(raw: *mut std::ffi::c_void) -> Result<Self, String> {
            if raw.is_null() {
                Err("MCP HTTP handle could not open".into())
            } else {
                Ok(Self(raw))
            }
        }
    }
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let agent = wide("Concordia");
    let session = Handle::new(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    })?;
    let host = wide(&endpoint.host);
    let connection =
        Handle::new(unsafe { WinHttpConnect(session.0, host.as_ptr(), endpoint.port, 0) })?;
    let path = wide(&endpoint.path);
    let verb = wide("POST");
    let handle = Handle::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            if endpoint.secure {
                WINHTTP_FLAG_SECURE
            } else {
                0
            },
        )
    })?;
    let redirect = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
    if unsafe {
        WinHttpSetOption(
            handle.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            (&redirect as *const u32).cast(),
            4,
        )
    } == 0
    {
        return Err("MCP redirect policy could not be set".into());
    }
    let set_timeout = || -> Result<(), String> {
        let millis = remaining(deadline)?.as_millis().clamp(1, i32::MAX as u128) as i32;
        if unsafe { WinHttpSetTimeouts(handle.0, millis, millis, millis, millis) } == 0 {
            return Err("MCP HTTP timeout could not be set".into());
        }
        Ok(())
    };
    set_timeout()?;
    let headers = wide(headers);
    if unsafe {
        WinHttpSendRequest(
            handle.0,
            headers.as_ptr(),
            u32::MAX,
            body.as_ptr().cast(),
            body.len() as u32,
            body.len() as u32,
            0,
        )
    } == 0
    {
        return Err("MCP HTTP send failed or timed out".into());
    }
    set_timeout()?;
    if unsafe { WinHttpReceiveResponse(handle.0, std::ptr::null_mut()) } == 0 {
        return Err("MCP HTTP response failed or timed out".into());
    }
    remaining(deadline)?;
    let mut size = 0;
    unsafe {
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
        );
    }
    if size == 0 || size as usize > MAX_HEADERS {
        return Err("MCP HTTP headers exceed the size limit or are unavailable".into());
    }
    let mut raw = vec![0u16; (size as usize).div_ceil(2)];
    if unsafe {
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            std::ptr::null(),
            raw.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err("MCP HTTP headers unavailable".into());
    }
    let head = String::from_utf16(&raw[..size as usize / 2])
        .map_err(|_| "invalid MCP HTTP headers")?
        .trim_end_matches('\0')
        .to_string();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or("MCP HTTP status unavailable")?;
    if !(200..300).contains(&status) {
        return Err(format!("remote MCP server returned HTTP {status}"));
    }
    if request.get("id").is_none() {
        if status != 202 && status != 204 && status != 200 {
            return Err("MCP notification was rejected".into());
        }
        return Ok((head, json!({})));
    }
    let mut bytes = Vec::new();
    loop {
        set_timeout()?;
        let mut available = 0;
        if unsafe { WinHttpQueryDataAvailable(handle.0, &mut available) } == 0 {
            return Err("MCP HTTP body failed or timed out".into());
        }
        remaining(deadline)?;
        let mut chunk = [0u8; 8192];
        let mut read = 0;
        set_timeout()?;
        if unsafe {
            WinHttpReadData(
                handle.0,
                chunk.as_mut_ptr().cast(),
                available.min(chunk.len() as u32),
                &mut read,
            )
        } == 0
        {
            return Err("MCP HTTP body failed or timed out".into());
        }
        remaining(deadline)?;
        if bytes.len().saturating_add(read as usize) > MAX_UPSTREAM_BYTES {
            return Err("MCP HTTP response exceeds the size limit".into());
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
        if let Some(value) = payload(&bytes, &head, request, read == 0)? {
            return Ok((head, value));
        }
        if read == 0 {
            return Err("MCP HTTP response ended without a result".into());
        }
    }
}

#[cfg(not(windows))]
fn plain_request(
    endpoint: &Endpoint,
    headers: &str,
    body: &[u8],
    request: &Value,
    deadline: Instant,
) -> Result<(String, Value), String> {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::{TcpStream, ToSocketAddrs},
    };
    if endpoint.secure {
        return Err("HTTPS MCP proxy requires Windows WinHTTP".into());
    }
    // DNS resolution uses the OS resolver; connect/read/write waits share the session deadline.
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| "MCP host could not resolve")?
        .next()
        .ok_or("MCP host could not resolve")?;
    let mut stream = TcpStream::connect_timeout(&address, remaining(deadline)?)
        .map_err(|_| "MCP HTTP connection failed or timed out")?;
    stream
        .set_write_timeout(Some(remaining(deadline)?))
        .map_err(|_| "MCP HTTP timeout unavailable")?;
    let wire = format!("POST {} HTTP/1.1\r\nHost: {}:{}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n", endpoint.path, endpoint.host, endpoint.port, body.len());
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(body))
        .map_err(|_| "MCP HTTP send failed")?;
    let mut input = BufReader::new(stream);
    let mut head = String::new();
    loop {
        input
            .get_ref()
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|_| "MCP HTTP timeout unavailable")?;
        let line = super::bounded_line(&mut input, MAX_HEADERS)
            .map_err(|_| "MCP HTTP headers failed")?
            .ok_or("MCP HTTP headers missing")?;
        if line.is_empty() || head.len() + line.len() > MAX_HEADERS {
            return Err("MCP HTTP headers exceed the limit".into());
        }
        if line == b"\r\n" {
            break;
        }
        head.push_str(std::str::from_utf8(&line).map_err(|_| "invalid MCP HTTP headers")?);
    }
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or("MCP HTTP status unavailable")?;
    if !(200..300).contains(&status) {
        return Err(format!("remote MCP server returned HTTP {status}"));
    }
    if request.get("id").is_none() {
        return Ok((head, json!({})));
    }
    let chunked = header(&head, "transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
    let length = header(&head, "content-length")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| "invalid MCP HTTP length")
        })
        .transpose()?;
    if length.is_some_and(|length| length > MAX_UPSTREAM_BYTES) {
        return Err("MCP HTTP response exceeds the size limit".into());
    }
    let mut bytes = Vec::new();
    let mut chunk_left = 0usize;
    loop {
        input
            .get_ref()
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|_| "MCP HTTP timeout unavailable")?;
        if chunked && chunk_left == 0 {
            let line = super::bounded_line(&mut input, 128)
                .map_err(|_| "MCP HTTP chunk failed")?
                .ok_or("MCP HTTP chunk missing")?;
            chunk_left = usize::from_str_radix(
                std::str::from_utf8(&line)
                    .map_err(|_| "invalid MCP chunk")?
                    .trim()
                    .split(';')
                    .next()
                    .unwrap_or_default(),
                16,
            )
            .map_err(|_| "invalid MCP chunk length")?;
            if chunk_left == 0 {
                return payload(&bytes, &head, request, true)?
                    .map(|value| (head, value))
                    .ok_or("MCP response missing".into());
            }
            if bytes.len().saturating_add(chunk_left) > MAX_UPSTREAM_BYTES {
                return Err("MCP HTTP response exceeds the size limit".into());
            }
        }
        let mut chunk = [0u8; 8192];
        let max = if chunked {
            chunk_left.min(chunk.len())
        } else {
            length.map_or(chunk.len(), |length| {
                length.saturating_sub(bytes.len()).min(chunk.len())
            })
        };
        let read = input
            .read(&mut chunk[..max])
            .map_err(|_| "MCP HTTP body failed or timed out")?;
        remaining(deadline)?;
        if bytes.len().saturating_add(read) > MAX_UPSTREAM_BYTES {
            return Err("MCP HTTP response exceeds the size limit".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if chunked {
            chunk_left -= read;
            if chunk_left == 0 {
                let mut crlf = [0u8; 2];
                input
                    .read_exact(&mut crlf)
                    .map_err(|_| "MCP HTTP chunk ended early")?;
                if crlf != *b"\r\n" {
                    return Err("invalid MCP HTTP chunk delimiter".into());
                }
            }
        }
        let complete = read == 0 || length == Some(bytes.len());
        if let Some(value) = payload(&bytes, &head, request, complete)? {
            return Ok((head, value));
        }
        if complete {
            return Err("MCP response missing".into());
        }
    }
}
