//! Bounded local overrides and approval-time route bindings. Config files remain read-only.
use super::*;
use std::fs::File;
use std::io::Read;

const POLICY_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policies {
    pub version: u8,
    pub main: Option<Vec<Target>>,
    pub workers: Option<Vec<Target>>,
}

pub fn validate_targets(targets: &[Target]) -> Result<(), String> {
    if targets.is_empty() || targets.len() > MAX_CANDIDATES {
        return Err("routing needs 1..32 ordered targets".into());
    }
    let mut seen = BTreeSet::new();
    for target in targets {
        if target.account.is_empty()
            || target.account.len() > 128
            || target.account != normalize_account_id(&target.account)
            || target
                .account
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace() || ch == ':')
            || target.model.as_ref().is_some_and(|model| {
                model.is_empty()
                    || model.len() > 256
                    || model
                        .chars()
                        .any(|ch| ch.is_control() || ch.is_whitespace())
            })
            || !seen.insert((target.account.clone(), target.model.clone()))
        {
            return Err("invalid or duplicate routing target; original policy kept".into());
        }
    }
    Ok(())
}

impl Policies {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported routing policy version; original kept".into());
        }
        if let Some(targets) = &self.main {
            validate_targets(targets)?;
        }
        if let Some(targets) = &self.workers {
            validate_targets(targets)?;
        }
        Ok(())
    }
}

pub fn load_policies(root: &Path) -> Result<Policies, String> {
    let path = root.join("routing-policy.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Policies {
                version: 1,
                ..Default::default()
            })
        }
        Err(error) => return Err(error.to_string()),
    };
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if !metadata.is_file() || linked || metadata.len() > POLICY_BYTES {
        return Err("routing policy must be a regular file within 64 KiB; original kept".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(POLICY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > POLICY_BYTES {
        return Err("routing policy exceeds 64 KiB".into());
    }
    let policies: Policies = serde_json::from_slice(&bytes)
        .map_err(|e| format!("routing policy unavailable: {e}; original kept"))?;
    policies.validate()?;
    Ok(policies)
}

pub fn policy_command(root: &Path, workers: bool, command: &str) -> Result<String, String> {
    if command.is_empty() {
        let policies = load_policies(root)?;
        return Ok(format!("Ordered routing policy: {:?}. Use policy set ACCOUNT[:MODEL] ... or policy legacy. Changes apply to future requests.", if workers { policies.workers } else { policies.main }));
    }
    let targets = if command == "legacy" {
        None
    } else if let Some(rest) = command.strip_prefix("set ") {
        let targets: Vec<_> = rest
            .split_whitespace()
            .take(MAX_CANDIDATES + 1)
            .map(|word| {
                let (account, model) = word
                    .split_once(':')
                    .map(|(a, m)| (a, Some(m.to_owned())))
                    .unwrap_or((word, None));
                Target {
                    account: normalize_account_id(account),
                    model,
                }
            })
            .collect();
        validate_targets(&targets)?;
        Some(targets)
    } else {
        return Err("usage: policy [set ACCOUNT[:MODEL] ...|legacy]".into());
    };
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    crate::state_io::with_lock(&root.join("routing-policy.lock"), || {
        let mut policies = load_policies(root).map_err(std::io::Error::other)?;
        if workers {
            policies.workers = targets;
        } else {
            policies.main = targets;
        }
        let bytes = serde_json::to_vec(&policies).map_err(std::io::Error::other)?;
        crate::state_io::write_atomic(&root.join("routing-policy.json"), &bytes)
    })
    .map_err(|e| e.to_string())?;
    Ok(
        "Routing policy saved for future tasks; approved tasks keep their captured sequence."
            .into(),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedTarget {
    pub target: Target,
    pub provider: Option<ProviderKind>,
    pub home_id: Option<String>,
    pub login_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approved {
    pub version: u8,
    pub revision: String,
    pub owner: String,
    pub failover: bool,
    pub targets: Vec<ApprovedTarget>,
}

impl Approved {
    pub fn capture(
        root: &Path,
        mut targets: Vec<Target>,
        owner: &str,
        failover: bool,
    ) -> Result<Self, String> {
        let mut seen = BTreeSet::new();
        for target in &mut targets {
            target.account = normalize_account_id(&target.account);
        }
        targets.retain(|target| seen.insert((target.account.clone(), target.model.clone())));
        // An empty legacy worker set is valid and authorizes no failover.
        if !targets.is_empty() {
            validate_targets(&targets)?;
        }
        let targets: Vec<_> = targets
            .into_iter()
            .map(|target| {
                let account = get_account(root, &target.account);
                let provider = account.as_ref().map(|account| account.provider);
                let home = account
                    .and_then(|account| account.home_path)
                    .and_then(|home| home.canonicalize().ok());
                let login_id = provider
                    .zip(home.as_ref())
                    .and_then(|(provider, home)| identity_key(provider, home))
                    .map(|identity| format!("{:x}", Sha256::digest(identity.as_bytes())));
                ApprovedTarget {
                    target,
                    provider,
                    home_id: crate::token_log::Attribution::bound_home(home.as_deref()),
                    login_id,
                }
            })
            .collect();
        let revision = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(&targets, owner, failover)).map_err(|e| e.to_string())?
            )
        );
        Ok(Self {
            version: 1,
            revision,
            owner: owner.into(),
            failover,
            targets,
        })
    }

    pub fn valid(&self) -> bool {
        self.version == 1
            && self.owner.len() <= 128
            && !self
                .owner
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
            && self.targets.len() <= MAX_CANDIDATES
            && self.targets.iter().all(|target| {
                [&target.home_id, &target.login_id].into_iter().all(|id| {
                    id.as_ref().is_none_or(|id| {
                        id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                })
            })
            && (self.targets.is_empty()
                || validate_targets(
                    &self
                        .targets
                        .iter()
                        .map(|target| target.target.clone())
                        .collect::<Vec<_>>(),
                )
                .is_ok())
            && self.revision
                == format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&(&self.targets, &self.owner, self.failover))
                            .unwrap_or_default()
                    )
                )
    }

    pub fn evaluate(&self, root: &Path, context: &Context) -> Result<Decision, String> {
        if !self.valid() {
            return Err("invalid approved routing snapshot; reconcile explicitly".into());
        }
        let mut decision = capture(
            root,
            &self
                .targets
                .iter()
                .map(|target| target.target.clone())
                .collect::<Vec<_>>(),
            context,
        );
        decision.policy_revision = self.revision.clone();
        for row in &mut decision.candidates {
            let expected = self
                .targets
                .iter()
                .find(|target| target.target == row.candidate.target)
                .ok_or("approved route missing")?;
            let login_id = row
                .candidate
                .provider
                .zip(row.candidate.home.as_ref())
                .and_then(|(provider, home)| identity_key(provider, home))
                .map(|identity| format!("{:x}", Sha256::digest(identity.as_bytes())));
            if row.candidate.provider != expected.provider
                || row.candidate.home_id != expected.home_id
                || login_id != expected.login_id
            {
                row.blocked.push(Reason::BindingChanged);
                row.eligible = false;
            }
        }
        Ok(decision)
    }

    pub fn verify_binding(&self, root: &Path, target: &Target) -> Result<(), String> {
        if !self.valid() {
            return Err("invalid approved routing snapshot".into());
        }
        let expected = self
            .targets
            .iter()
            .find(|row| &row.target == target)
            .ok_or("target was not approved in this routing snapshot")?;
        let current = Self::capture(root, vec![target.clone()], &self.owner, self.failover)?;
        let current = current.targets.first().ok_or("target unavailable")?;
        if expected.provider != current.provider
            || expected.home_id != current.home_id
            || expected.login_id != current.login_id
        {
            return Err("account/home/login binding changed since approval".into());
        }
        Ok(())
    }
}

pub fn worker_targets(root: &Path) -> Result<Vec<Target>, String> {
    if let Some(targets) = load_policies(root)?.workers {
        return Ok(targets);
    }
    let settings = crate::store::get_settings(root);
    Ok(settings
        .workers
        .iter()
        .map(|account| Target {
            account: normalize_account_id(account),
            model: settings.worker_models.get(account).cloned(),
        })
        .collect())
}
