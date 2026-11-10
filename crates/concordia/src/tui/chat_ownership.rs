//! Open-chat leases complement, rather than replace, native per-turn writer locks.
use super::*;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Key {
    Native {
        provider: ProviderKind,
        home: PathBuf,
        session: String,
    },
    Conversation(String),
}

impl Key {
    fn native(provider: ProviderKind, home: &Path, session: &str) -> Result<Self, String> {
        if session.is_empty() || session.len() > 512 || session.chars().any(char::is_control) {
            return Err("chat has no valid exact native session identity".into());
        }
        let home = home
            .canonicalize()
            .map_err(|error| format!("chat home unavailable: {error}"))?;
        if !home.is_dir() {
            return Err("chat home is not a directory".into());
        }
        Ok(Self::Native {
            provider,
            home: PathBuf::from(crate::file_links::normalized_local_path(
                &home.to_string_lossy(),
            )),
            session: session.to_owned(),
        })
    }
    pub(super) fn for_resume(root: &Path, account: &str, session: &str) -> Result<Self, String> {
        let account = get_account(root, account).ok_or("chat account is no longer registered")?;
        Self::native(
            account.provider,
            Path::new(&account.home_path.ok_or("chat account has no native home")?),
            session,
        )
    }
    pub(super) fn for_app(app: &App) -> Option<Self> {
        let home = app.home_path.as_deref()?;
        if let Some(session) = app.native_session_id.as_deref() {
            return Self::native(app.provider, home, session).ok();
        }
        if !app.keep.turn_recovery_notice {
            return None;
        }
        let source = app.native_sources.last()?;
        let key = Self::native(source.provider, &source.home, &source.session_id).ok()?;
        (source.provider == app.provider
            && source.account == app.account
            && Self::native(app.provider, home, &source.session_id)
                .ok()
                .as_ref()
                == Some(&key))
        .then_some(key)
    }
    pub(super) fn conversation(app: &App) -> Self {
        Self::Conversation(app.deleg.d.conversation_id().to_owned())
    }
    fn filename(&self) -> String {
        let value = match self {
            Self::Native {
                provider,
                home,
                session,
            } => format!(
                "native\n{}\n{}\n{}",
                provider.as_str(),
                home.display(),
                session
            ),
            Self::Conversation(id) => format!("conversation\n{id}"),
        };
        format!("{:x}.lock", Sha256::digest(value.as_bytes()))
    }
}

pub(super) struct Lease {
    key: Key,
    _lock: File,
}
impl Lease {
    pub(super) fn check_resume_writer(root: &Path, key: &Key) -> Result<(), String> {
        if let Key::Native { home, session, .. } = key {
            let guard = crate::provider::session_guard::acquire(root, Some(home), session)
                .map_err(|error| error.to_string())?;
            drop(guard);
        }
        Ok(())
    }
    pub(super) fn key(&self) -> &Key {
        &self.key
    }
    pub(super) fn acquire(root: &Path, key: &Key) -> Result<Self, String> {
        let directory = root.join("open-chats");
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        crate::workspaces::directory_id(&directory)?;
        let directory = crate::fs_safe::resolved_directory(&directory)
            .ok_or("chat ownership directory unavailable")?;
        let path = directory.join(key.filename());
        if path.exists() && !crate::fs_safe::regular_owned_file(&path, &directory) {
            return Err("chat ownership lock is not a regular owned file".into());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let file = options.open(path).map_err(|error| {
            if matches!(error.raw_os_error(), Some(32 | 33)) {
                "This chat is already open in another Concordia tab or window.".into()
            } else {
                format!("Chat ownership unavailable: {error}")
            }
        })?;
        file.try_lock().map_err(|error| {
            format!("This chat is already open in another Concordia tab or window ({error}).")
        })?;
        Ok(Self {
            key: key.clone(),
            _lock: file,
        })
    }
}
