//! Explicit account/home transport preference. Runtime capability evidence stays on its connection.
use crate::{native_memory::Binding, store::ProviderKind};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Standard,
    CursorAcp,
    GrokAcp,
    CodexAppServer,
    ClaudeStdio,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u8,
    binding: Binding,
    mode: Mode,
}

fn path(root: &Path, binding: &Binding) -> PathBuf {
    use sha2::{Digest, Sha256};
    let key = format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\n{}\n{}",
            binding.provider.as_str(),
            binding.account,
            binding.home.display()
        ))
    );
    root.join(".ao/native-input").join(format!("{key}.json"))
}

fn regular(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || crate::workspaces::linked(&metadata) => {
            Err("native-input metadata must remain a regular owned file".into())
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

/// Cursor questions, plans, permission grants and images only exist over ACP, so it is Cursor's
/// default; `/native ACCOUNT off` saves an explicit opt-out.
fn default_for(provider: ProviderKind) -> Mode {
    match provider {
        ProviderKind::Cursor => Mode::CursorAcp,
        ProviderKind::Grok => Mode::GrokAcp,
        ProviderKind::Codex | ProviderKind::Claude => Mode::Standard,
    }
}

pub fn load(root: &Path, binding: &Binding) -> Result<Mode, String> {
    let path = path(root, binding);
    if !regular(&path)? {
        return Ok(default_for(binding.provider));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(16385)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 16384 {
        return Err("native-input preference exceeds 16 KiB; original kept".into());
    }
    let saved: Saved = serde_json::from_slice(&bytes)
        .map_err(|_| "invalid native-input preference; original kept")?;
    if saved.version != 1
        || saved.binding.provider != binding.provider
        || saved.binding.account != binding.account
        || saved.binding.home != binding.home
        || saved.mode == Mode::CursorAcp && binding.provider != ProviderKind::Cursor
        || saved.mode == Mode::GrokAcp && binding.provider != ProviderKind::Grok
        || saved.mode == Mode::CodexAppServer && binding.provider != ProviderKind::Codex
        || saved.mode == Mode::ClaudeStdio && binding.provider != ProviderKind::Claude
    {
        return Err("native-input preference version/account/home mismatch; original kept".into());
    }
    Ok(saved.mode)
}

pub fn for_home(
    root: &Path,
    provider: ProviderKind,
    account: &str,
    home: Option<&Path>,
) -> Result<Mode, String> {
    match home {
        Some(home) => load(
            root,
            &Binding::new(provider, account, home, "native-input-policy")?,
        ),
        None => Ok(default_for(provider)),
    }
}

pub fn command(root: &Path, account: &str, command: &str) -> Result<String, String> {
    let account = crate::accounts::normalize_account_id(account);
    let registered =
        crate::accounts::get_account(root, &account).ok_or("unknown native-input account")?;
    let home = registered
        .home_path
        .ok_or("native-input account home is unavailable")?;
    let binding = Binding::new(registered.provider, &account, &home, "native-input-policy")?;
    let current = load(root, &binding)?;
    let mode = match command {
        "status" => current,
        "on" if registered.provider == ProviderKind::Cursor => Mode::CursorAcp,
        "on" if registered.provider == ProviderKind::Grok => Mode::GrokAcp,
        "on" if registered.provider == ProviderKind::Codex => Mode::CodexAppServer,
        "on" if registered.provider == ProviderKind::Claude => Mode::ClaudeStdio,
        "off" => Mode::Standard,
        _ => return Err("/native ACCOUNT on|off|status".into()),
    };
    if command != "status" {
        let file = path(root, &binding);
        let directory = file
            .parent()
            .ok_or("native-input directory is unavailable")?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        crate::workspaces::directory_id(directory)?;
        let lock = file.with_extension("lock");
        regular(&lock)?;
        crate::state_io::with_lock(&lock, || {
            load(root, &binding).map_err(std::io::Error::other)?;
            if !file.exists() && fs::read_dir(directory)?.take(513).count() > 511 {
                return Err(std::io::Error::other(
                    "native-input preference directory is full; no automatic pruning",
                ));
            }
            let bytes = serde_json::to_vec(&Saved {
                version: 1,
                binding: binding.clone(),
                mode,
            })
            .map_err(std::io::Error::other)?;
            if bytes.len() > 16384 {
                return Err(std::io::Error::other(
                    "native-input preference exceeds 16 KiB; original kept",
                ));
            }
            crate::state_io::write_atomic(&file, &bytes)
        })
        .map_err(|error| error.to_string())?;
    }
    let description: String = match mode {
        Mode::CursorAcp => "on (Cursor ACP)".into(),
        Mode::GrokAcp => "on (Grok Build ACP)".into(),
        Mode::CodexAppServer => "on (Codex app-server)".into(),
        Mode::ClaudeStdio => format!(
            "on (Claude native stdio; CLI {} control interface required)",
            crate::capacity::claude_capacity::CLI_VERSION
        ),
        Mode::Standard => "off (standard text transport)".into(),
    };
    Ok(format!("{account}: native text input {description}. Future tasks capture this account/home preference; existing worker attempts keep theirs. Native capabilities are checked on connection."))
}
