//! Account connect: give a provider a fresh concordia-owned home and open its native login.
//!
//! Rules (AGENTS.md / PLAN): never write to the user's existing `.codex` / `.codex-2` / `.claude`
//! homes, never read or ask for credentials ourselves, never interpolate paths into a shell
//! script. The login command is a constant per provider; the isolated home travels through the
//! child's environment (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`) exactly as it does for chat turns.

use crate::accounts::{is_logged_in, list_all_accounts};
use crate::store::{upsert_stored_account, ProviderKind, StoredAccount};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where concordia-owned account homes live: `<state_root>/homes/<account id>`.
pub fn homes_dir(state_root: &Path) -> PathBuf {
    state_root.join("homes")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub id: String,
    pub label: String,
    pub home: PathBuf,
    /// An earlier concordia-owned slot that never finished login, reused instead of piling up.
    pub reused: bool,
}

fn capitalized(p: ProviderKind) -> &'static str {
    match p {
        ProviderKind::Codex => "Codex",
        ProviderKind::Claude => "Claude",
        ProviderKind::Cursor => "Cursor",
        ProviderKind::Grok => "Grok Build",
    }
}

/// Choose the account slot for a new connection. Pure over disk state; creates nothing.
pub fn pick_slot(state_root: &Path, provider: ProviderKind) -> Slot {
    let accounts = list_all_accounts(state_root);
    let homes = homes_dir(state_root);

    // Reuse an abandoned concordia-owned slot (created earlier, login never completed).
    let mut abandoned: Vec<_> = accounts
        .iter()
        .filter(|a| a.custom && a.provider == provider)
        .filter_map(|a| a.home_path.as_ref().map(|h| (a, h)))
        .filter(|(_, h)| h.starts_with(&homes) && !is_logged_in(provider, h))
        .collect();
    abandoned.sort_by(|a, b| a.0.account.cmp(&b.0.account));
    if let Some((a, h)) = abandoned.first() {
        return Slot {
            id: a.account.clone(),
            label: a.label.clone(),
            home: (*h).clone(),
            reused: true,
        };
    }

    let taken: HashSet<&str> = accounts.iter().map(|a| a.account.as_str()).collect();
    let mut n = 1;
    loop {
        let id = format!("{}-{n}", provider.as_str());
        let home = homes.join(&id);
        let in_use = taken.contains(id.as_str()) || home.exists();
        if !in_use {
            return Slot {
                label: format!("{} {n}", capitalized(provider)),
                id,
                home,
                reused: false,
            };
        }
        n += 1;
    }
}

/// A fully specified login launch. `script` is a constant; the home is only in `env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPlan {
    pub provider: ProviderKind,
    pub script: &'static str,
    pub env: Vec<(String, String)>,
    pub bin: &'static str,
}

/// Plan the native login. `has_bin` reports whether a CLI is on PATH.
pub fn plan_login(
    provider: ProviderKind,
    home: Option<&Path>,
    has_bin: &dyn Fn(&str) -> bool,
) -> Result<LoginPlan, String> {
    let (bin, script, env_key): (&'static str, &'static str, Option<&str>) = match provider {
        ProviderKind::Codex => ("codex", "codex login", Some("CODEX_HOME")),
        ProviderKind::Claude => ("claude", "claude login", Some("CLAUDE_CONFIG_DIR")),
        ProviderKind::Cursor => {
            if has_bin("agent") {
                ("agent", "agent login", None)
            } else {
                ("cursor-agent", "cursor-agent login", None)
            }
        }
        ProviderKind::Grok => ("grok", "grok login", Some("GROK_HOME")),
    };
    if !has_bin(bin) {
        return Err(format!(
            "{bin} not found on PATH — install the {} CLI, then try again",
            capitalized(provider)
        ));
    }
    let env = match (env_key, home) {
        (Some(k), Some(h)) => vec![(k.to_string(), h.to_string_lossy().into_owned())],
        _ => Vec::new(),
    };
    Ok(LoginPlan {
        provider,
        script,
        env,
        bin,
    })
}

/// The process to spawn for a plan: WezTerm window when available, else a new console.
pub fn login_command(plan: &LoginPlan) -> Command {
    let ps = ["-NoLogo", "-NoProfile", "-NoExit", "-Command", plan.script];
    // Provider login must be visible so the user can complete native authentication.
    let mut cmd = Command::new("powershell.exe");
    cmd.args(ps);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    for (k, v) in &plan.env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

fn launch_login(plan: &LoginPlan) -> Result<(), String> {
    login_command(plan)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open a login window: {e}"))
}

/// Command a user can paste to log in by hand when the window could not be opened.
pub fn manual_login_hint(plan: &LoginPlan) -> String {
    let env: String = plan
        .env
        .iter()
        .map(|(k, v)| format!("$env:{k}='{}'; ", v.replace('\'', "''")))
        .collect();
    format!("{env}{}", plan.script)
}

/// How an account can be taken out of Concordia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveMode {
    /// Unregister it **and delete its login folder**. Only for homes Concordia created.
    DeleteData,
    /// Unregister it but leave every file where it is.
    Unregister,
    /// Hide it from Concordia. For homes Concordia does not own: nothing is touched.
    Hide,
}

#[derive(Debug, Clone)]
pub struct Removable {
    pub id: String,
    pub provider: ProviderKind,
    pub label: String,
    pub home: Option<PathBuf>,
    /// Created by Concordia, under its own homes folder.
    pub owned: bool,
    /// Registered in Concordia's store (as opposed to discovered on disk).
    pub stored: bool,
}

impl Removable {
    /// The choices that are safe for this account, most thorough first.
    pub fn modes(&self) -> Vec<RemoveMode> {
        if self.owned {
            vec![RemoveMode::DeleteData, RemoveMode::Unregister]
        } else if self.stored {
            vec![RemoveMode::Unregister]
        } else {
            vec![RemoveMode::Hide]
        }
    }
}

fn is_owned_home(state_root: &Path, home: &Path) -> bool {
    if crate::workspaces::directory_id(&homes_dir(state_root)).is_err()
        || crate::workspaces::directory_id(home).is_err()
    {
        return false;
    }
    let Ok(homes) = fs::canonicalize(homes_dir(state_root)) else {
        return false;
    };
    let Ok(h) = fs::canonicalize(home) else {
        return false;
    };
    h.parent() == Some(homes.as_path())
}

/// Every account the user can remove, with what removing it would do.
pub fn removable(state_root: &Path) -> Vec<Removable> {
    list_all_accounts(state_root)
        .into_iter()
        // A built-in slot whose home does not exist or is not signed in is not an account yet.
        .filter(|a| {
            a.custom
                || a.home_path
                    .as_ref()
                    .is_some_and(|h| is_logged_in(a.provider, h))
        })
        .map(|a| {
            let owned = a.custom
                && a.home_path
                    .as_ref()
                    .is_some_and(|h| is_owned_home(state_root, h));
            Removable {
                id: a.account,
                provider: a.provider,
                label: a.label,
                home: a.home_path,
                owned,
                stored: a.custom,
            }
        })
        .collect()
}

/// Remove an account. `DeleteData` is refused unless the home is inside Concordia's own homes
/// folder (after resolving links), so a stray path in the store can never delete a user's files.
pub fn remove_account(state_root: &Path, id: &str, mode: RemoveMode) -> Result<String> {
    let want = crate::accounts::normalize_account_id(id);
    let acct = removable(state_root)
        .into_iter()
        .find(|r| r.id == want)
        .with_context(|| format!("no account named {id}"))?;
    if !acct.modes().contains(&mode) {
        anyhow::bail!(
            "{} cannot be removed that way ({})",
            acct.label,
            match mode {
                RemoveMode::DeleteData => "its files are not Concordia's to delete",
                RemoveMode::Unregister => "it is not registered with Concordia",
                RemoveMode::Hide => "Concordia owns its files; delete or unregister it instead",
            }
        );
    }
    match mode {
        RemoveMode::DeleteData => {
            let home = acct.home.clone().context("account has no home folder")?;
            let meta =
                fs::symlink_metadata(&home).with_context(|| format!("stat {}", home.display()))?;
            if crate::workspaces::linked(&meta) || !is_owned_home(state_root, &home) {
                anyhow::bail!(
                    "refusing to delete {}: not a plain folder inside Concordia's homes",
                    home.display()
                );
            }
            let canonical = home.canonicalize()?;
            anyhow::ensure!(
                !list_all_accounts(state_root).iter().any(|other| {
                    other.account != acct.id
                        && other.home_path.as_ref().is_some_and(|path| {
                            path.canonicalize()
                                .is_ok_and(|path| path.starts_with(&canonical))
                        })
                }),
                "another account references this home; unregister it without deleting shared files"
            );
            crate::store::load_store_checked(state_root)?;
            fs::remove_dir_all(&home).with_context(|| format!("delete {}", home.display()))?;
            crate::store::remove_stored_account(state_root, &acct.id)
                .context("account files were removed, but registration could not be updated")?;
            Ok(format!(
                "removed {} and deleted its login folder",
                acct.label
            ))
        }
        RemoveMode::Unregister => {
            crate::store::remove_stored_account(state_root, &acct.id)?;
            Ok(format!(
                "removed {} from Concordia (its files were left in place)",
                acct.label
            ))
        }
        RemoveMode::Hide => {
            crate::store::hide_account(state_root, &acct.id)?;
            Ok(format!(
                "{} is hidden from Concordia (its files were not touched)",
                acct.label
            ))
        }
    }
}

fn has_bin_on_path(name: &str) -> bool {
    which::which(name).is_ok()
}

#[derive(Debug, Clone)]
pub struct AddResult {
    pub account: String,
    pub provider: ProviderKind,
    pub label: String,
    pub home: PathBuf,
    /// Lines for the transcript.
    pub message: Vec<String>,
    /// Whether the TUI should watch this home for the login to complete.
    pub watch: bool,
}

/// Side effects injected so tests can observe a launch without opening windows.
pub struct Effects<'a> {
    pub has_bin: &'a dyn Fn(&str) -> bool,
    pub launch: &'a dyn Fn(&LoginPlan) -> Result<(), String>,
}

/// Connect a new account for `provider` using the real PATH and a real login window.
pub fn add_account(state_root: &Path, provider: ProviderKind) -> Result<AddResult> {
    add_account_with(
        state_root,
        provider,
        &Effects {
            has_bin: &has_bin_on_path,
            launch: &launch_login,
        },
    )
}

pub fn add_account_with(
    state_root: &Path,
    provider: ProviderKind,
    fx: &Effects<'_>,
) -> Result<AddResult> {
    // The first Cursor account is the CLI's own global login; extra ones are key files.
    if provider == ProviderKind::Cursor {
        if let Some(main) = crate::accounts::builtin_accounts()
            .into_iter()
            .find(|a| a.provider == ProviderKind::Cursor)
        {
            let ready = main
                .home_path
                .as_ref()
                .map(|h| is_logged_in(ProviderKind::Cursor, h))
                .unwrap_or(false);
            if !ready {
                return begin_login(
                    fx,
                    provider,
                    main.account,
                    main.label,
                    main.home_path.unwrap_or_default(),
                    false,
                );
            }
        }
    }

    let slot = pick_slot(state_root, provider);

    if provider == ProviderKind::Cursor {
        return add_cursor_key_slot(state_root, slot);
    }

    // Plan first: a missing CLI must not leave an empty slot behind.
    let plan = plan_login(provider, Some(&slot.home), fx.has_bin).map_err(anyhow::Error::msg)?;
    register_slot(state_root, provider, &slot)?;
    finish_login(fx, plan, slot.id, slot.label, slot.home, slot.reused)
}

fn register_slot(state_root: &Path, provider: ProviderKind, slot: &Slot) -> Result<()> {
    fs::create_dir_all(&slot.home).with_context(|| format!("create {}", slot.home.display()))?;
    if !slot.reused {
        upsert_stored_account(
            state_root,
            StoredAccount {
                id: slot.id.clone(),
                provider,
                home_path: Some(slot.home.to_string_lossy().into_owned()),
                label: slot.label.clone(),
                created_at: epoch_stamp(),
            },
        )?;
    }
    // Awareness block / MCP entry for Concordia-created homes (skips everything else).
    let _ = crate::integration::refresh(state_root, &crate::update::resolve_active_exe());
    Ok(())
}

fn begin_login(
    fx: &Effects<'_>,
    provider: ProviderKind,
    id: String,
    label: String,
    home: PathBuf,
    reused: bool,
) -> Result<AddResult> {
    let plan = plan_login(provider, Some(&home), fx.has_bin).map_err(anyhow::Error::msg)?;
    finish_login(fx, plan, id, label, home, reused)
}

fn finish_login(
    fx: &Effects<'_>,
    plan: LoginPlan,
    id: String,
    label: String,
    home: PathBuf,
    reused: bool,
) -> Result<AddResult> {
    let mut message = vec![format!(
        "{} {label}",
        if reused { "reconnecting" } else { "adding" }
    )];
    match (fx.launch)(&plan) {
        Ok(()) => message.push(
            "login window opened — finish signing in there; concordia will tell you when it connects"
                .into(),
        ),
        Err(e) => {
            message.push(e);
            message.push(format!("run it yourself: {}", manual_login_hint(&plan)));
        }
    }
    Ok(AddResult {
        account: id,
        provider: plan.provider,
        label,
        home,
        message,
        watch: true,
    })
}

fn add_cursor_key_slot(state_root: &Path, slot: Slot) -> Result<AddResult> {
    register_slot(state_root, ProviderKind::Cursor, &slot)?;
    let key = slot.home.join("concordia-api-key");
    Ok(AddResult {
        account: slot.id,
        provider: ProviderKind::Cursor,
        message: vec![
            format!("adding {}", slot.label),
            "extra Cursor accounts use an API key file (the CLI's login is global)".into(),
            format!("save the key as a single line in {}", key.display()),
            "concordia will notice and connect it".into(),
        ],
        label: slot.label,
        home: slot.home,
        watch: true,
    })
}

fn epoch_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", d.as_secs(), d.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::get_account;
    use std::cell::RefCell;

    struct Sandbox {
        _guard: std::sync::MutexGuard<'static, ()>,
        base: tempfile::TempDir,
        state: tempfile::TempDir,
    }

    impl Sandbox {
        fn new() -> Self {
            let guard = crate::accounts::TEST_ENV
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let base = tempfile::tempdir().unwrap();
            let state = tempfile::tempdir().unwrap();
            std::env::set_var("CONCORDIA_CODEX_HOME_BASE", base.path());
            for k in [
                "AO_CODEX_HOME",
                "AO_CODEX_HOME_A",
                "AO_CODEX_HOME_2",
                "AO_CODEX_HOME_B",
                "AO_CLAUDE_CONFIG_DIR",
                "AO_CURSOR_HOME",
            ] {
                std::env::remove_var(k);
            }
            Sandbox {
                _guard: guard,
                base,
                state,
            }
        }
        fn state(&self) -> &Path {
            self.state.path()
        }
        fn base(&self) -> &Path {
            self.base.path()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            std::env::remove_var("CONCORDIA_CODEX_HOME_BASE");
        }
    }

    fn all_bins(_: &str) -> bool {
        true
    }

    fn run(
        sb: &Sandbox,
        provider: ProviderKind,
        has_bin: &dyn Fn(&str) -> bool,
    ) -> (Result<AddResult>, Vec<LoginPlan>) {
        let launched = RefCell::new(Vec::new());
        let launch = |p: &LoginPlan| {
            launched.borrow_mut().push(p.clone());
            Ok(())
        };
        let r = add_account_with(
            sb.state(),
            provider,
            &Effects {
                has_bin,
                launch: &launch,
            },
        );
        (r, launched.into_inner())
    }

    fn mark_logged_in(provider: ProviderKind, home: &Path) {
        fs::create_dir_all(home).unwrap();
        match provider {
            ProviderKind::Codex => fs::write(home.join("auth.json"), "{}").unwrap(),
            ProviderKind::Cursor => fs::write(home.join("concordia-api-key"), "k").unwrap(),
            ProviderKind::Claude => fs::write(
                home.join(".credentials.json"),
                r#"{"claudeAiOauth":{"accessToken":"x"}}"#,
            )
            .unwrap(),
            ProviderKind::Grok => {
                fs::write(home.join("config.toml"), "[models]\ndefault = \"grok-build\"\n").unwrap();
                fs::write(home.join("auth.json"), "{}").unwrap();
            }
        }
    }

    #[test]
    fn new_homes_live_under_concordia_state_never_the_users_profile() {
        let sb = Sandbox::new();
        let slot = pick_slot(sb.state(), ProviderKind::Codex);
        assert!(slot.home.starts_with(homes_dir(sb.state())));
        assert!(!slot.home.starts_with(sb.base()));
    }

    #[test]
    fn add_creates_concordia_owned_home_stores_account_and_launches_login() {
        let sb = Sandbox::new();
        let (r, launched) = run(&sb, ProviderKind::Codex, &all_bins);
        let r = r.unwrap();
        assert_eq!(r.account, "codex-1");
        assert!(r.watch);
        assert!(r.home.is_dir());
        assert!(r.home.starts_with(homes_dir(sb.state())));

        // Registered, so /use can see it once logged in.
        let acct = get_account(sb.state(), "codex-1").unwrap();
        assert!(acct.custom);
        assert_eq!(acct.home_path.as_deref(), Some(r.home.as_path()));

        // Exactly one launch; the isolated home is passed via env only.
        assert_eq!(launched.len(), 1);
        assert_eq!(launched[0].script, "codex login");
        assert_eq!(
            launched[0].env,
            vec![(
                "CODEX_HOME".to_string(),
                r.home.to_string_lossy().into_owned()
            )]
        );

        // The user's own homes were not created or touched.
        assert!(!sb.base().join(".codex").exists());
        assert!(!sb.base().join(".codex-2").exists());
    }

    #[test]
    fn claude_login_isolates_via_config_dir() {
        let sb = Sandbox::new();
        let (r, launched) = run(&sb, ProviderKind::Claude, &all_bins);
        let r = r.unwrap();
        assert_eq!(launched[0].script, "claude login");
        assert_eq!(launched[0].env[0].0, "CLAUDE_CONFIG_DIR");
        assert_eq!(r.account, "claude-1");
    }

    #[test]
    fn missing_cli_fails_cleanly_without_leaving_anything_behind() {
        let sb = Sandbox::new();
        let (r, launched) = run(&sb, ProviderKind::Codex, &|_| false);
        let err = r.unwrap_err().to_string();
        assert!(err.contains("codex not found on PATH"), "{err}");
        assert!(launched.is_empty());
        assert!(!homes_dir(sb.state()).exists());
        assert!(crate::store::list_stored_accounts(sb.state()).is_empty());
    }

    #[test]
    fn an_abandoned_slot_is_reused_instead_of_piling_up() {
        let sb = Sandbox::new();
        let first = run(&sb, ProviderKind::Codex, &all_bins).0.unwrap();
        // Login never completes; adding again reuses the pending slot.
        let (again, launched) = run(&sb, ProviderKind::Codex, &all_bins);
        let again = again.unwrap();
        assert_eq!(again.account, first.account);
        assert_eq!(again.home, first.home);
        assert_eq!(launched.len(), 1);
        assert_eq!(crate::store::list_stored_accounts(sb.state()).len(), 1);
        assert!(again.message[0].starts_with("reconnecting"));
    }

    #[test]
    fn a_connected_slot_is_kept_and_the_next_add_gets_a_new_one() {
        let sb = Sandbox::new();
        let first = run(&sb, ProviderKind::Codex, &all_bins).0.unwrap();
        mark_logged_in(ProviderKind::Codex, &first.home);
        let second = run(&sb, ProviderKind::Codex, &all_bins).0.unwrap();
        assert_eq!(first.account, "codex-1");
        assert_eq!(second.account, "codex-3");
        assert_eq!(crate::store::list_stored_accounts(sb.state()).len(), 2);
    }

    #[test]
    fn launch_failure_keeps_the_account_and_shows_a_manual_command() {
        let sb = Sandbox::new();
        let launch = |_: &LoginPlan| Err("could not open a login window: boom".to_string());
        let r = add_account_with(
            sb.state(),
            ProviderKind::Codex,
            &Effects {
                has_bin: &all_bins,
                launch: &launch,
            },
        )
        .unwrap();
        let text = r.message.join("\n");
        assert!(text.contains("boom"));
        assert!(text.contains("$env:CODEX_HOME='"));
        assert!(text.contains("codex login"));
        assert!(get_account(sb.state(), "codex-1").is_some());
    }

    #[test]
    fn first_cursor_account_uses_the_cli_login_with_no_new_slot() {
        let sb = Sandbox::new();
        let (r, launched) = run(&sb, ProviderKind::Cursor, &all_bins);
        let r = r.unwrap();
        assert_eq!(r.account, "cursor");
        assert_eq!(launched[0].script, "agent login");
        assert!(launched[0].env.is_empty());
        assert!(crate::store::list_stored_accounts(sb.state()).is_empty());
    }

    #[test]
    fn extra_cursor_accounts_are_api_key_files_and_never_launch_a_login() {
        let sb = Sandbox::new();
        mark_logged_in(ProviderKind::Cursor, &sb.base().join(".cursor"));
        let (r, launched) = run(&sb, ProviderKind::Cursor, &all_bins);
        let r = r.unwrap();
        assert!(launched.is_empty());
        assert_eq!(r.account, "cursor-1");
        let text = r.message.join("\n");
        assert!(text.contains("concordia-api-key"), "{text}");
        assert!(r.home.is_dir());
        // The key file is the user's to create; concordia must not invent one.
        assert!(!r.home.join("concordia-api-key").exists());
    }

    #[test]
    fn cursor_prefers_agent_but_falls_back_to_cursor_agent() {
        let only_alt = |n: &str| n == "cursor-agent";
        let plan = plan_login(ProviderKind::Cursor, None, &only_alt).unwrap();
        assert_eq!(plan.script, "cursor-agent login");
        let none = |_: &str| false;
        assert!(plan_login(ProviderKind::Cursor, None, &none).is_err());
    }

    #[test]
    fn login_command_never_interpolates_the_home_into_the_script() {
        let home = PathBuf::from(r"C:\Users\o'brien\concordia\homes\codex-3");
        let plan = plan_login(ProviderKind::Codex, Some(&home), &all_bins).unwrap();

        let cmd = login_command(&plan);
        assert_eq!(cmd.get_program(), "powershell.exe");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-NoLogo",
                "-NoProfile",
                "-NoExit",
                "-Command",
                "codex login"
            ]
        );
        assert!(
            args.iter().all(|a| !a.contains("brien")),
            "home leaked into argv"
        );
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.iter().any(|(k, v)| {
            *k == "CODEX_HOME" && v.map(|v| v.to_string_lossy().contains("o'brien")) == Some(true)
        }));
    }

    #[test]
    fn manual_hint_escapes_single_quotes() {
        let home = PathBuf::from("C:\\o'brien");
        let plan = plan_login(ProviderKind::Codex, Some(&home), &all_bins).unwrap();
        assert_eq!(
            manual_login_hint(&plan),
            "$env:CODEX_HOME='C:\\o''brien'; codex login"
        );
    }

    #[test]
    fn mcp_registration_is_opt_in_and_writes_the_codex_table() {
        let sb = Sandbox::new();
        let r = run(&sb, ProviderKind::Codex, &all_bins).0.unwrap();
        fs::write(sb.state().join("config.toml"), "[threads]\nmcp = true\n").unwrap();
        let _cfg = crate::config::env_guard();
        let exe = sb.base().join("concordia.exe");
        crate::integration::refresh(sb.state(), &exe);
        let toml = fs::read_to_string(r.home.join("config.toml")).unwrap();
        assert!(toml.contains("[mcp_servers.concordia]") && toml.contains("args = [\"mcp\"]"));
    }

    #[test]
    fn doctor_rows_report_each_created_home_and_flag_unparseable_files() {
        let sb = Sandbox::new();
        let r = run(&sb, ProviderKind::Codex, &all_bins).0.unwrap();
        let _cfg = crate::config::env_guard();
        let exe = Path::new("c.exe");
        let rows = crate::integration::doctor_rows(sb.state(), exe);
        assert!(rows
            .iter()
            .any(|(warn, n, d)| !warn && *n == "mcp" && d.ends_with("off")));
        fs::write(sb.state().join("config.toml"), "[threads]\nmcp = true\n").unwrap();
        fs::write(r.home.join("config.toml"), "[broken").unwrap();
        let rows = crate::integration::doctor_rows(sb.state(), exe);
        assert!(rows
            .iter()
            .any(|(warn, n, d)| *warn && *n == "mcp" && d.contains("left alone")));
        assert_eq!(
            fs::read_to_string(r.home.join("config.toml")).unwrap(),
            "[broken"
        );
    }

    // ---- removal and duplicates -------------------------------------------------------------

    use crate::test_support::codex_login;

    fn register(sb: &Sandbox, id: &str, home: &Path) {
        upsert_stored_account(
            sb.state(),
            StoredAccount {
                id: id.into(),
                provider: ProviderKind::Codex,
                home_path: Some(home.to_string_lossy().into_owned()),
                label: id.into(),
                created_at: "0".into(),
            },
        )
        .unwrap();
    }

    #[test]
    fn removing_an_owned_account_deletes_its_login_folder_and_unregisters_it() {
        let sb = Sandbox::new();
        let r = run(&sb, ProviderKind::Codex, &all_bins).0.unwrap();
        codex_login(&r.home, "a@example.com");
        let msg = remove_account(sb.state(), &r.account, RemoveMode::DeleteData).unwrap();
        assert!(msg.contains("deleted its login folder"), "{msg}");
        assert!(!r.home.exists());
        assert!(get_account(sb.state(), &r.account).is_none());
        assert!(crate::store::list_stored_accounts(sb.state()).is_empty());
    }

    #[test]
    fn unregistering_keeps_every_file() {
        let sb = Sandbox::new();
        let r = run(&sb, ProviderKind::Codex, &all_bins).0.unwrap();
        codex_login(&r.home, "a@example.com");
        remove_account(sb.state(), &r.account, RemoveMode::Unregister).unwrap();
        assert!(
            r.home.join("auth.json").is_file(),
            "files must be left in place"
        );
        assert!(get_account(sb.state(), &r.account).is_none());
    }

    #[test]
    fn a_users_own_home_is_never_deleted_only_hidden() {
        let sb = Sandbox::new();
        let own = sb.base().join(".codex");
        codex_login(&own, "me@example.com");
        assert!(get_account(sb.state(), "codex").is_some());

        let rm = removable(sb.state());
        let r = rm.iter().find(|r| r.id == "codex").unwrap();
        assert!(!r.owned && !r.stored);
        assert_eq!(r.modes(), [RemoveMode::Hide]);

        for bad in [RemoveMode::DeleteData, RemoveMode::Unregister] {
            let e = remove_account(sb.state(), "codex", bad)
                .unwrap_err()
                .to_string();
            assert!(e.contains("cannot be removed that way"), "{e}");
            assert!(own.join("auth.json").is_file());
        }
        remove_account(sb.state(), "codex", RemoveMode::Hide).unwrap();
        assert!(
            get_account(sb.state(), "codex").is_none(),
            "hidden from Concordia"
        );
        assert!(
            own.join("auth.json").is_file(),
            "but nothing on disk changed"
        );
    }

    #[test]
    fn a_registered_home_outside_concordias_folder_can_be_unregistered_but_never_deleted() {
        let sb = Sandbox::new();
        let elsewhere = sb.base().join("precious");
        codex_login(&elsewhere, "x@example.com");
        register(&sb, "codex-9", &elsewhere);

        let r = removable(sb.state())
            .into_iter()
            .find(|r| r.id == "codex-9")
            .unwrap();
        assert!(r.stored && !r.owned);
        assert_eq!(r.modes(), [RemoveMode::Unregister]);
        let e = remove_account(sb.state(), "codex-9", RemoveMode::DeleteData).unwrap_err();
        assert!(e.to_string().contains("cannot be removed that way"), "{e}");
        assert!(elsewhere.join("auth.json").is_file());
        remove_account(sb.state(), "codex-9", RemoveMode::Unregister).unwrap();
        assert!(elsewhere.join("auth.json").is_file());
    }

    #[test]
    fn a_path_trick_in_the_store_cannot_aim_deletion_at_a_parent_or_the_homes_folder() {
        let sb = Sandbox::new();
        let homes = homes_dir(sb.state());
        fs::create_dir_all(&homes).unwrap();
        // `homes\codex-7\..` resolves to the homes folder itself; it must not count as owned.
        let sneaky = homes.join("codex-7").join("..");
        fs::create_dir_all(homes.join("codex-7")).unwrap();
        register(&sb, "codex-7", &sneaky);
        let r = removable(sb.state())
            .into_iter()
            .find(|r| r.id == "codex-7")
            .unwrap();
        assert!(!r.owned, "the homes folder itself is not an account home");
        assert!(remove_account(sb.state(), "codex-7", RemoveMode::DeleteData).is_err());
        assert!(homes.is_dir());
    }

    #[test]
    fn removing_an_unknown_account_is_a_clear_error() {
        let sb = Sandbox::new();
        let e = remove_account(sb.state(), "codex-42", RemoveMode::Hide).unwrap_err();
        assert!(e.to_string().contains("no account named codex-42"), "{e}");
    }

    #[test]
    fn the_same_login_in_two_slots_is_a_duplicate_regardless_of_case() {
        let sb = Sandbox::new();
        codex_login(&sb.base().join(".codex"), "Me@Example.com");
        let slot = homes_dir(sb.state()).join("codex-3");
        codex_login(&slot, "me@example.COM");
        register(&sb, "codex-3", &slot);

        let groups = crate::accounts::duplicate_groups(sb.state());
        assert_eq!(groups.len(), 1);
        let ids: Vec<_> = groups[0].iter().map(|a| a.account.as_str()).collect();
        assert_eq!(
            ids,
            ["codex", "codex-3"],
            "built-in first, the added slot second"
        );
        // The added slot is the duplicate of the original, not the other way round.
        assert_eq!(
            crate::accounts::duplicate_of(sb.state(), "codex-3")
                .unwrap()
                .account,
            "codex"
        );
        assert_eq!(
            crate::accounts::duplicate_of(sb.state(), "codex")
                .unwrap()
                .account,
            "codex-3"
        );
    }

    #[test]
    fn different_logins_or_logged_out_homes_are_not_duplicates() {
        let sb = Sandbox::new();
        codex_login(&sb.base().join(".codex"), "one@example.com");
        let two = homes_dir(sb.state()).join("codex-3");
        codex_login(&two, "two@example.com");
        register(&sb, "codex-3", &two);
        // A slot that is still waiting for its login has no identity.
        let empty = homes_dir(sb.state()).join("codex-4");
        fs::create_dir_all(&empty).unwrap();
        register(&sb, "codex-4", &empty);
        assert!(crate::accounts::duplicate_groups(sb.state()).is_empty());
        assert!(crate::accounts::duplicate_of(sb.state(), "codex-3").is_none());
    }
}
