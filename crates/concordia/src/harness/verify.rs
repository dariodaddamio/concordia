//! `/harness verify`: turn an adapter's live probe into the receipt the launch gate reads.
//!
//! A receipt is written only when the adapter's verify command exits 0 and prints a JSON report
//! saying `denied-live-probe` for exactly this provider, this isolated home and the project's
//! current policy revision. Anything else is `unverified`, withdraws an earlier receipt, and says
//! why. A model turn never reaches this code: only the slash command does.

use super::{expand, resolve_program, Context, Manifest, RunOutcome};
use crate::provider::enforcement;
use serde_json::Value;
use std::fs;
use std::path::Path;

const PROVEN: &str = "denied-live-probe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A receipt was written; the launch gate now accepts this provider and home.
    Verified,
    /// Nothing was proven; the text says why. Any earlier receipt was withdrawn.
    Unverified(String),
    /// The project has no locked-code policy, so there is nothing to prove.
    NothingToVerify(String),
}

fn tail(text: &str) -> String {
    let text = text.trim();
    let start = text.len().saturating_sub(200);
    let start = (start..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    text[start..].replace('\n', " ")
}

/// The report: the last stdout line that is a JSON object, else the whole output.
fn parse_report(stdout: &str) -> Option<Value> {
    let object = |v: Value| v.is_object().then_some(v);
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .find_map(|l| serde_json::from_str(l).ok().and_then(object))
        .or_else(|| serde_json::from_str(stdout.trim()).ok().and_then(object))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn write_receipt(path: &Path, body: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(body)?)?;
    replace_file(&temporary, path)
}

fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        }
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
}

/// Run the adapter's verify command and write a receipt under `store` only for a real denial.
pub fn verify_in(
    store: &Path,
    manifest: &Manifest,
    manifest_path: &Path,
    manifest_sha256: &str,
    ctx: &Context<'_>,
    run: &dyn Fn(&[String]) -> RunOutcome,
) -> Verdict {
    let provider = ctx.provider.as_str();
    let Some(root) = enforcement::project_root(ctx.project) else {
        return Verdict::NothingToVerify(
            "this project has no docs/quality/locked-code.json, so there is nothing to verify"
                .into(),
        );
    };
    let slot = enforcement::slot(store, &root);
    if enforcement::has_reparse_component(store, &slot) {
        return Verdict::Unverified("the enforcement state path contains a symlink or junction; refusing to write a receipt".into());
    }
    let receipt = slot.join(format!("{provider}.json"));
    let unverified = |why: String| {
        let _ = fs::remove_file(&receipt);
        Verdict::Unverified(why)
    };
    if ctx.home.is_some_and(|h| enforcement::is_inside(h, &root)) {
        return unverified(format!(
            "{provider}: the provider home is inside the project, so it is not isolated and cannot be verified"
        ));
    }
    let Some(commands) = manifest.providers.get(provider) else {
        return unverified(format!(
            "{} does not declare {provider}, so it cannot be verified there",
            manifest.name
        ));
    };
    let Some(verify) = &commands.verify else {
        return unverified(format!(
            "{} has no verify command for {provider}, so enforcement cannot be proven",
            manifest.name
        ));
    };
    let mut argv = match expand(verify, ctx) {
        Ok(argv) => argv,
        Err(why) => return unverified(why),
    };
    argv[0] = resolve_program(&argv[0], &manifest.dir)
        .to_string_lossy()
        .into_owned();
    let policy_path = root.join(enforcement::POLICY_FILE);
    let before = fs::read(&policy_path)
        .map(|bytes| enforcement::policy_revision(&bytes))
        .unwrap_or_default();
    if before.is_empty() {
        return unverified("the locked-code policy could not be read".into());
    }
    let outcome = run(&argv);
    if outcome.timed_out {
        return unverified("the verify command timed out".into());
    }
    if outcome.exit != Some(0) {
        let code = outcome
            .exit
            .map_or("no exit code".to_string(), |c| format!("exit {c}"));
        return unverified(format!(
            "the verify command failed ({code}): {}",
            tail(&outcome.stderr)
        ));
    }
    let Some(report) = parse_report(&outcome.stdout) else {
        return unverified("the verify command did not print a JSON report".into());
    };
    let result = report.get("result").and_then(Value::as_str).unwrap_or("");
    if result != PROVEN {
        let reason = report
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("no reason given");
        return unverified(format!("{} reported `{result}`: {reason}", manifest.name));
    }
    let current = fs::read(&policy_path)
        .map(|bytes| enforcement::policy_revision(&bytes))
        .unwrap_or_default();
    let reported_home = report
        .get("home")
        .and_then(Value::as_str)
        .map(|h| enforcement::path_key(Path::new(h)));
    let matches = report.get("provider").and_then(Value::as_str) == Some(provider)
        && report.get("policyRevision").and_then(Value::as_str) == Some(before.as_str())
        && !before.is_empty()
        && reported_home == ctx.home.map(enforcement::path_key);
    if !matches {
        return unverified(
            "the report does not match this provider, isolated home and current policy revision"
                .into(),
        );
    }
    if current != before {
        return unverified("the locked-code policy changed while the adapter was running".into());
    }
    let body = serde_json::json!({
        "provider": provider,
        "home": ctx.home.map(|h| h.to_string_lossy().into_owned()),
        "policyRevision": before,
        "result": PROVEN,
        "harness": manifest.name,
        "harnessVersion": manifest.version,
        "harnessManifestPath": manifest_path.to_string_lossy(),
        "harnessManifestSha256": manifest_sha256,
        "verifiedAt": now(),
    });
    match write_receipt(&receipt, &body) {
        Ok(()) => Verdict::Verified,
        Err(e) => Verdict::Unverified(format!("could not write the receipt: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{connections, parse_manifest, Context, Manifest, RunOutcome};
    use crate::provider::enforcement::{self, Gate};
    use crate::store::ProviderKind;
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;

    const POLICY: &str = "{\"controls\":[]}\n";

    fn manifest(with_verify: bool) -> Manifest {
        let verify = if with_verify {
            r#","verify":["h","verify","{provider}","{home}","{project}"]"#
        } else {
            ""
        };
        parse_manifest(
            &format!(
                r#"{{"name":"h1","version":"2.0","providers":{{"claude":{{"connect":["h","connect"]{verify}}}}}}}"#
            ),
            Path::new("/opt/h"),
        )
        .unwrap()
    }

    struct Fixture {
        store: tempfile::TempDir,
        project: tempfile::TempDir,
        home: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let f = Fixture {
            store: tempfile::tempdir().unwrap(),
            project: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(f.project.path().join("docs/quality")).unwrap();
        fs::write(
            f.project.path().join("docs/quality/locked-code.json"),
            POLICY,
        )
        .unwrap();
        f
    }

    fn revision() -> String {
        enforcement::policy_revision(POLICY.as_bytes())
    }

    fn denial(provider: &str, home: &Path, revision: &str) -> RunOutcome {
        let body = serde_json::json!({
            "result": "denied-live-probe",
            "provider": provider,
            "home": home.to_string_lossy(),
            "policyRevision": revision,
        });
        RunOutcome {
            exit: Some(0),
            stdout: format!("probe log line\n{body}\n"),
            stderr: String::new(),
            timed_out: false,
        }
    }

    fn run_verify(f: &Fixture, m: &Manifest, out: RunOutcome) -> (Verdict, Vec<Vec<String>>) {
        let seen = RefCell::new(Vec::new());
        let manifest_path = f.project.path().join("harness.json");
        fs::write(&manifest_path, "{}").unwrap();
        let manifest_path_text = manifest_path.to_string_lossy().into_owned();
        let manifest_sha = enforcement::policy_revision(b"{}");
        connections::record(
            f.store.path(),
            &connections::Identity {
                name: &m.name,
                version: m.version.as_deref(),
                manifest_path: &manifest_path_text,
                manifest_sha256: &manifest_sha,
                skill: None,
            },
            connections::Entry {
                provider: "claude".into(),
                home: Some(f.home.path().to_string_lossy().into_owned()),
                project: f.project.path().to_string_lossy().into_owned(),
                connect_ok: true,
                connect_note: "ok".into(),
                verify: "not-run".into(),
                verify_note: String::new(),
                at: 0,
            },
        )
        .unwrap();
        let ctx = Context {
            provider: ProviderKind::Claude,
            home: Some(f.home.path()),
            project: f.project.path(),
        };
        let verdict = verify_in(
            f.store.path(),
            m,
            &manifest_path,
            &manifest_sha,
            &ctx,
            &|argv: &[String]| {
                seen.borrow_mut().push(argv.to_vec());
                out.clone()
            },
        );
        (verdict, seen.into_inner())
    }

    fn gate(f: &Fixture) -> Gate {
        enforcement::gate_in(
            f.store.path(),
            f.project.path(),
            ProviderKind::Claude,
            Some(f.home.path()),
        )
    }

    #[test]
    fn a_real_denial_for_this_provider_home_and_policy_writes_a_receipt_that_opens_the_gate() {
        let f = fixture();
        assert!(matches!(gate(&f), Gate::ReadOnly(_)));
        let out = denial("claude", f.home.path(), &revision());
        let (verdict, seen) = run_verify(&f, &manifest(true), out);
        assert_eq!(verdict, Verdict::Verified);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0][1], "verify");
        assert_eq!(seen[0][2], "claude");
        assert_eq!(seen[0][3], f.home.path().to_string_lossy());
        assert_eq!(
            gate(&f),
            Gate::Open,
            "the existing gate accepts the receipt"
        );
    }

    fn assert_unverified(f: &Fixture, verdict: Verdict, needle: &str) {
        match verdict {
            Verdict::Unverified(why) => assert!(why.contains(needle), "{why}"),
            other => panic!("expected unverified, got {other:?}"),
        }
        assert!(matches!(gate(f), Gate::ReadOnly(_)), "no receipt may exist");
    }

    #[test]
    fn a_denial_with_a_failing_exit_code_proves_nothing() {
        let f = fixture();
        let mut out = denial("claude", f.home.path(), &revision());
        out.exit = Some(1);
        let (verdict, _) = run_verify(&f, &manifest(true), out);
        assert_unverified(&f, verdict, "exit");
    }

    #[test]
    fn a_forged_or_mismatched_report_is_unverified() {
        for (provider, home_ok, rev) in [
            ("codex", true, revision()),
            ("claude", false, revision()),
            ("claude", true, "0".repeat(64)),
        ] {
            let f = fixture();
            let home = if home_ok {
                f.home.path().to_path_buf()
            } else {
                f.home.path().join("elsewhere")
            };
            let out = denial(provider, &home, &rev);
            let (verdict, _) = run_verify(&f, &manifest(true), out);
            assert_unverified(&f, verdict, "does not match");
        }
    }

    #[test]
    fn a_result_that_is_not_a_denial_is_unverified_with_the_adapter_reason() {
        let f = fixture();
        let out = RunOutcome {
            exit: Some(0),
            stdout: r#"{"result":"not-supported","reason":"this client loads no hooks"}"#.into(),
            stderr: String::new(),
            timed_out: false,
        };
        let (verdict, _) = run_verify(&f, &manifest(true), out);
        assert_unverified(&f, verdict, "this client loads no hooks");
    }

    #[test]
    fn a_timeout_or_unreadable_output_is_unverified() {
        let f = fixture();
        let timed_out = RunOutcome {
            exit: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
        };
        let (verdict, _) = run_verify(&f, &manifest(true), timed_out);
        assert_unverified(&f, verdict, "timed out");
        let garbage = RunOutcome {
            exit: Some(0),
            stdout: "all good, trust me".into(),
            stderr: String::new(),
            timed_out: false,
        };
        let (verdict, _) = run_verify(&f, &manifest(true), garbage);
        assert_unverified(&f, verdict, "JSON");
    }

    #[test]
    fn an_adapter_without_a_verify_command_cannot_verify() {
        let f = fixture();
        let out = denial("claude", f.home.path(), &revision());
        let (verdict, seen) = run_verify(&f, &manifest(false), out);
        assert_unverified(&f, verdict, "no verify command");
        assert!(seen.is_empty(), "nothing was run");
    }

    #[test]
    fn a_project_without_a_locked_code_policy_has_nothing_to_verify() {
        let f = fixture();
        fs::remove_file(f.project.path().join("docs/quality/locked-code.json")).unwrap();
        let out = denial("claude", f.home.path(), &revision());
        let (verdict, seen) = run_verify(&f, &manifest(true), out);
        assert!(
            matches!(verdict, Verdict::NothingToVerify(_)),
            "{verdict:?}"
        );
        assert!(seen.is_empty());
        let receipts = f.store.path().join("enforcement");
        assert!(!receipts.exists() || fs::read_dir(receipts).unwrap().next().is_none());
    }

    #[test]
    fn a_home_inside_the_project_is_never_verified() {
        let f = fixture();
        let inside = f.project.path().join(".claude-home");
        let ctx = Context {
            provider: ProviderKind::Claude,
            home: Some(&inside),
            project: f.project.path(),
        };
        let out = denial("claude", &inside, &revision());
        let verdict = verify_in(
            f.store.path(),
            &manifest(true),
            Path::new("/opt/h1/harness.json"),
            "fixture-manifest",
            &ctx,
            &|_: &[String]| out.clone(),
        );
        match verdict {
            Verdict::Unverified(why) => assert!(why.contains("isolated"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_failed_check_withdraws_an_earlier_receipt() {
        let f = fixture();
        let good = denial("claude", f.home.path(), &revision());
        assert_eq!(run_verify(&f, &manifest(true), good).0, Verdict::Verified);
        assert_eq!(gate(&f), Gate::Open);
        let bad = RunOutcome {
            exit: Some(2),
            stdout: String::new(),
            stderr: "probe could not run".into(),
            timed_out: false,
        };
        let (verdict, _) = run_verify(&f, &manifest(true), bad);
        assert_unverified(&f, verdict, "probe could not run");
    }

    #[test]
    fn a_policy_changed_by_the_adapter_is_unverified_without_a_receipt() {
        let f = fixture();
        let original = revision();
        let ctx = Context {
            provider: ProviderKind::Claude,
            home: Some(f.home.path()),
            project: f.project.path(),
        };
        let verdict = verify_in(
            f.store.path(),
            &manifest(true),
            Path::new("command:h1"),
            "fixture-manifest",
            &ctx,
            &|_: &[String]| {
                fs::write(
                    f.project.path().join("docs/quality/locked-code.json"),
                    "changed\n",
                )
                .unwrap();
                denial("claude", f.home.path(), &original)
            },
        );
        assert!(matches!(verdict, Verdict::Unverified(why) if why.contains("changed")));
        assert!(matches!(gate(&f), Gate::ReadOnly(_)));
    }

    #[test]
    fn verifying_twice_replaces_the_receipt_and_a_write_failure_keeps_the_old_one() {
        let f = fixture();
        let m = manifest(true);
        let ctx = Context {
            provider: ProviderKind::Claude,
            home: Some(f.home.path()),
            project: f.project.path(),
        };
        let good = || denial("claude", f.home.path(), &revision());
        assert_eq!(
            verify_in(
                f.store.path(),
                &m,
                Path::new("/opt/h1/harness.json"),
                "fixture-manifest",
                &ctx,
                &|_| good(),
            ),
            Verdict::Verified
        );
        assert_eq!(
            verify_in(
                f.store.path(),
                &m,
                Path::new("/opt/h1/harness.json"),
                "fixture-manifest",
                &ctx,
                &|_| good(),
            ),
            Verdict::Verified
        );
        let receipt = enforcement::slot(f.store.path(), f.project.path()).join("claude.json");
        let old = fs::read_to_string(&receipt).unwrap();
        fs::create_dir(receipt.with_extension("json.tmp")).unwrap();
        let verdict = verify_in(
            f.store.path(),
            &m,
            Path::new("/opt/h1/harness.json"),
            "fixture-manifest",
            &ctx,
            &|_| good(),
        );
        assert!(matches!(verdict, Verdict::Unverified(why) if why.contains("write")));
        assert_eq!(fs::read_to_string(receipt).unwrap(), old);
    }
}
