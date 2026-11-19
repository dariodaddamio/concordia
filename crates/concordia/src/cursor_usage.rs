//! Cursor limits without spending tokens.
//!
//! Cursor's CLI has no non-interactive usage command (`agent -p "/usage"` would go to the model),
//! but its interactive screen answers `/usage` locally. This runs that screen in a pseudo-console,
//! types only `/usage`, reads the table and kills the process tree. No prompt is ever sent.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// CSI-u Return reaches Ink's submit handler; Win32 input records no longer do on Cursor 2026.11.17.
const ENTER_KEY: &[u8] = b"\x1b[13u";

/// Whole fetch budget: start, ready, `/usage`, table.
pub const PTY_TIMEOUT: Duration = Duration::from_secs(15);
/// Output quiet this long (or this long after first output) means the prompt is waiting for input.
const QUIET_READY: Duration = Duration::from_millis(700);
const READY_CAP: Duration = Duration::from_secs(6);

/// What to run in the pseudo-console.
#[derive(Debug, Clone)]
pub struct PtySpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

/// A fixed empty folder, so Cursor records its workspace trust once rather than per fetch. The
/// sessions Cursor records there are Concordia's, not the person's.
pub fn probe_dir() -> PathBuf {
    std::env::temp_dir().join("concordia-usage-probe")
}

/// Program, args and env for the interactive agent behind `bin`. A `.cmd` shim is bypassed through
/// the PowerShell script it would run (as in `provider::launch`), else run through `cmd.exe /c`.
pub fn interactive_launch(bin: &str) -> (String, Vec<String>, Vec<(String, String)>) {
    use crate::provider::launch::{cursor_ps1_beside, is_batch_shim};
    if !is_batch_shim(bin) {
        return (bin.to_string(), Vec::new(), Vec::new());
    }
    let shim = Path::new(bin);
    match cursor_ps1_beside(shim) {
        Some(ps1) => {
            let name = shim
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "agent.cmd".into());
            (
                "powershell.exe".into(),
                vec![
                    "-NoProfile".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-File".into(),
                    ps1.to_string_lossy().into_owned(),
                ],
                vec![("CURSOR_INVOKED_AS".into(), name)],
            )
        }
        None => (
            "cmd.exe".into(),
            vec!["/c".into(), bin.to_string()],
            Vec::new(),
        ),
    }
}

/// The real fetch for one Cursor account home: the installed agent, the account's API key if it
/// has one, an empty temp working directory.
pub fn fetch_cursor_usage_text(home: &Path) -> Result<String, String> {
    if crate::provider::bin_override("CONCORDIA_AGENT_BIN").is_some() {
        // A test or fake provider stands in for the agent: never drive it interactively.
        return Err("agent binary is overridden".into());
    }
    let bin = crate::provider::cursor::find_agent_bin()?;
    let (program, args, mut env) = interactive_launch(&bin);
    if let Some(key) = crate::identity::cursor_api_key_for_home(home) {
        env.push(("CURSOR_API_KEY".into(), key));
    }
    let cwd = probe_dir();
    std::fs::create_dir_all(&cwd).map_err(|e| format!("temp dir: {e}"))?;
    let started = std::time::SystemTime::now();
    let text = run_usage_in_pty(
        &PtySpec {
            program,
            args,
            env,
            cwd: cwd.clone(),
        },
        PTY_TIMEOUT,
    );
    if let Some(chats) = dirs::home_dir().map(|home| home.join(".cursor").join("chats")) {
        remove_probe_chats(&chats, &cwd, started);
    }
    text
}

/// Cursor records every interactive start as a chat, so each usage check would leave an empty
/// chat behind. Remove only the ones this check made: started in the probe folder, holding no
/// conversation, and created since the check began.
fn remove_probe_chats(chats: &Path, probe: &Path, since: std::time::SystemTime) {
    let since_ms = since
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let probe = probe
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_lowercase();
    let Ok(projects) = std::fs::read_dir(chats) else {
        return;
    };
    for project in projects.flatten() {
        let Ok(entries) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for chat in entries.flatten() {
            let dir = chat.path();
            let recent = chat
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| modified >= since);
            if !recent || dir.join("store.db").exists() {
                continue;
            }
            let Some(meta) = std::fs::read_to_string(dir.join("meta.json"))
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            else {
                continue;
            };
            let made_here = meta["cwd"]
                .as_str()
                .is_some_and(|cwd| cwd.trim_end_matches('\\').to_lowercase() == probe);
            let created = meta["createdAtMs"].as_u64().map_or(0, u128::from);
            if made_here && meta["hasConversation"] == false && created >= since_ms {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
}

struct Shared {
    buf: Vec<u8>,
    overflow: bool,
    first: Option<Instant>,
    last: Instant,
}

/// Start `spec` in a pseudo-console, wait for it to settle, type `/usage` + Enter, return the
/// screen text once the table is on it. Always kills the process tree.
pub fn run_usage_in_pty(spec: &PtySpec, timeout: Duration) -> Result<String, String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 150,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("no pseudo-console: {e}"))?;
    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    // Account-specific credentials, when supplied, are added below. Never let
    // an unrelated inherited API key select the account for this observation.
    cmd.env_remove("CURSOR_API_KEY");
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("could not start the Cursor agent: {e}"))?;
    let tree = match crate::process_lifetime::own_pty_process(child.as_ref()) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            return Err(error);
        }
    };
    drop(pair.slave);
    let pid = child.process_id().unwrap_or(0);
    let master = pair.master;
    let (mut reader, mut writer) = match (master.try_clone_reader(), master.take_writer()) {
        (Ok(r), Ok(w)) => (r, w),
        _ => {
            crate::provider::turn::kill_process_tree(pid);
            let _ = child.kill();
            return Err("pseudo-console io failed".into());
        }
    };
    let shared = Arc::new(Mutex::new(Shared {
        buf: Vec::new(),
        overflow: false,
        first: None,
        last: Instant::now(),
    }));
    let sink = Arc::clone(&shared);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let Ok(mut s) = sink.lock() else { break };
            if s.buf.len().saturating_add(n) > 1024 * 1024 {
                s.overflow = true;
                break;
            }
            s.buf.extend_from_slice(&chunk[..n]);
            let now = Instant::now();
            s.last = now;
            s.first.get_or_insert(now);
        }
    });

    let result = drive(&shared, &mut writer, child.as_mut(), timeout);

    drop(tree);
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    // Closing a pseudo-console can block until its output is drained; never let that stall us.
    std::thread::spawn(move || drop(master));
    result
}

fn drive(
    shared: &Mutex<Shared>,
    writer: &mut dyn Write,
    child: &mut (dyn portable_pty::Child + Send + Sync),
    timeout: Duration,
) -> Result<String, String> {
    let start = Instant::now();
    let mut sent = false;
    let mut enters = 0u8;
    let mut sent_at = Instant::now();
    let mut trusted = false;
    let mut dsr_answered = 0usize;
    // Only output after this offset/instant counts (everything before it was a dialog we answered).
    let mut base = 0usize;
    let mut epoch: Option<Instant> = None;
    loop {
        if start.elapsed() >= timeout {
            return Err(format!("Cursor changed its usage screen or did not respond within {}s; usage is unavailable", timeout.as_secs()));
        }
        let (bytes, first, last) = {
            let s = shared.lock().map_err(|_| "poisoned")?;
            if s.overflow {
                return Err("Cursor usage output exceeds 1 MiB".into());
            }
            (s.buf.clone(), s.first, s.last)
        };
        // ConPTY may ask where the cursor is and wait for the answer.
        let asked = count_sub(&bytes, b"\x1b[6n");
        while dsr_answered < asked {
            let _ = writer.write_all(b"\x1b[1;1R");
            let _ = writer.flush();
            dsr_answered += 1;
        }
        let text = screen_text(&bytes[base.min(bytes.len())..]);
        if sent && table_complete(&text) {
            std::thread::sleep(Duration::from_millis(250));
            let bytes = shared.lock().map_err(|_| "poisoned")?.buf.clone();
            return Ok(screen_text(&bytes[base.min(bytes.len())..]));
        }
        if let Ok(Some(_)) = child.try_wait() {
            return Err("the Cursor agent exited before showing usage".into());
        }
        // Once answered, the old dialog text lingers in the stream (clears are not tracked), so
        // a stuck dialog shows up as the overall timeout instead.
        if !trusted && is_trust_prompt(&text) {
            // The working directory is an empty folder Concordia made for this; the prompt shows
            // `[a] Trust this workspace`, which is the only key sent for it.
            let _ = writer.write_all(b"a");
            let _ = writer.flush();
            trusted = true;
            sent = false;
            base = bytes.len();
            epoch = Some(Instant::now());
        } else {
            let since = epoch.or(first);
            let ready = since.is_some_and(|e| {
                (last > e && last.elapsed() >= QUIET_READY) || e.elapsed() >= READY_CAP
            });
            if !sent && ready {
                let _ = writer.write_all(b"/usage");
                let _ = writer.flush();
                std::thread::sleep(Duration::from_millis(500));
                let _ = writer.write_all(ENTER_KEY);
                let _ = writer.flush();
                sent = true;
                enters = 1;
                sent_at = Instant::now();
            } else if sent && enters < 3 && sent_at.elapsed() >= Duration::from_secs(4) {
                // Enter was lost (the screen loads usage over the network first, so give it time).
                // With the command already run, Enter on an empty prompt does nothing.
                let _ = writer.write_all(ENTER_KEY);
                let _ = writer.flush();
                enters += 1;
                sent_at = Instant::now();
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn count_sub(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn is_trust_prompt(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("trust this workspace") || t.contains("workspace trust")
}

fn table_complete(text: &str) -> bool {
    text.contains("Included") && text.contains("% used") && text.contains("On-Demand")
}

/// Terminal bytes to plain lines: cursor moves become newlines or spaces, everything else
/// (colours, modes, titles) is dropped.
pub fn screen_text(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\u{1b}' => match it.peek().copied() {
                Some('[') => {
                    it.next();
                    let mut last = ' ';
                    let mut params = String::new();
                    for n in it.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&n) {
                            last = n;
                            break;
                        }
                        params.push(n);
                    }
                    match last {
                        'H' | 'f' | 'B' | 'E' | 'd' => out.push('\n'),
                        'C' => {
                            let n = params.parse::<usize>().unwrap_or(1).clamp(1, 200);
                            out.extend(std::iter::repeat_n(' ', n));
                        }
                        _ => {}
                    }
                }
                Some(']') => {
                    it.next();
                    while let Some(n) = it.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            it.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    it.next();
                }
                None => {}
            },
            '\r' => {}
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "Usage \u{2022} Pro                 Resets Nov 6\n Category  Current  Usage\n Included  27% used  ###\n On-Demand  Disabled\n";

    /// A fake raw-input agent: accepts the same local command and Return sequence as Cursor.
    fn fake_agent(dir: &Path, table: bool, hang_after: bool) -> PtySpec {
        let sample = dir.join("sample.txt");
        std::fs::write(&sample, if table { TABLE } else { "nothing useful\n" }).unwrap();
        #[cfg(windows)]
        let (script, body) = (
            dir.join("fake-agent.ps1"),
            format!(
                r#"Add-Type 'using System; using System.Runtime.InteropServices; public static class ProbeInput {{ [DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int n); [DllImport("kernel32.dll")] public static extern bool SetConsoleMode(IntPtr h, uint m); }}'
[ProbeInput]::SetConsoleMode([ProbeInput]::GetStdHandle(-10), 0x0200) | Out-Null
[Console]::WriteLine('Cursor Agent ready')
$stream = [Console]::OpenStandardInput()
$bytes = New-Object byte[] 11
$offset = 0
while ($offset -lt 11) {{ $n = $stream.Read($bytes, $offset, 11 - $offset); if ($n -eq 0) {{ exit 1 }}; $offset += $n }}
if ([Text.Encoding]::UTF8.GetString($bytes) -ne "/usage$([char]27)[13u") {{ exit 2 }}
[Console]::Write([IO.File]::ReadAllText('{}'))
{}"#,
                sample.display().to_string().replace('\'', "''"),
                if hang_after { "Start-Sleep 60" } else { "" }
            ),
        );
        #[cfg(unix)]
        let (script, body) = (
            dir.join("fake-agent.sh"),
            format!(
                "#!/bin/sh\nstty -icanon -echo\necho 'Cursor Agent ready'\ndd bs=1 count=11 of=/dev/null 2>/dev/null\ncat '{}'\n{}",
                sample.display(),
                if hang_after { "sleep 60\n" } else { "" }
            ),
        );
        std::fs::write(&script, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        let (program, args, env) = (
            "powershell.exe".to_string(),
            vec![
                "-NoProfile".into(),
                "-File".into(),
                script.to_string_lossy().into_owned(),
            ],
            Vec::new(),
        );
        #[cfg(unix)]
        let (program, args, env) = interactive_launch(script.to_str().unwrap());
        PtySpec {
            program,
            args,
            env,
            cwd: dir.to_path_buf(),
        }
    }

    #[test]
    fn reads_the_table_from_a_fake_agent_and_kills_it() {
        let dir = tempfile::tempdir().unwrap();
        let spec = fake_agent(dir.path(), true, true);
        let started = Instant::now();
        let text = run_usage_in_pty(&spec, Duration::from_secs(20)).expect("table");
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "did not wait out the hang"
        );
        let snap = crate::usage::parse_cursor_usage(&text).expect(&text);
        assert_eq!(snap.windows[0].label, "Included");
    }

    #[test]
    fn times_out_when_no_table_appears() {
        let dir = tempfile::tempdir().unwrap();
        let spec = fake_agent(dir.path(), false, true);
        let e = run_usage_in_pty(&spec, Duration::from_secs(4)).unwrap_err();
        assert!(
            e.contains("usage is unavailable") || e.contains("exited"),
            "{e}"
        );
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_hang() {
        let dir = tempfile::tempdir().unwrap();
        let spec = PtySpec {
            program: "concordia-no-such-agent".into(),
            args: vec![],
            env: vec![],
            cwd: dir.path().to_path_buf(),
        };
        assert!(run_usage_in_pty(&spec, Duration::from_secs(4)).is_err());
    }

    #[test]
    fn screen_text_turns_cursor_moves_into_lines() {
        let raw = "\u{1b}[?25l\u{1b}[2;1HIncluded\u{1b}[5C27% used\u{1b}[3;1H\u{1b}[1mAuto\u{1b}[0m\u{1b}]0;title\u{7}";
        let t = screen_text(raw.as_bytes());
        assert_eq!(t, "\nIncluded     27% used\nAuto");
    }

    #[test]
    fn shim_launch_uses_cmd_or_the_ps1_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("agent.cmd");
        let (p, a, _) = interactive_launch(shim.to_str().unwrap());
        assert_eq!(p, "cmd.exe");
        assert_eq!(a[0], "/c");
        std::fs::write(dir.path().join("cursor-agent.ps1"), "").unwrap();
        let (p, _, env) = interactive_launch(shim.to_str().unwrap());
        assert_eq!(p, "powershell.exe");
        assert_eq!(env[0].0, "CURSOR_INVOKED_AS");
        let (p, a, e) = interactive_launch("agent.exe");
        assert_eq!((p.as_str(), a.len(), e.len()), ("agent.exe", 0, 0));
    }
}
