//! Deterministic local route evaluation. Eligibility is distinct from native capacity.
use crate::accounts::{get_account, identity_key, is_logged_in, normalize_account_id};
use crate::store::ProviderKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[path = "routing_policy.rs"]
mod policy;
pub use policy::{load_policies, policy_command, worker_targets, Approved, Policies};

pub const MAX_CANDIDATES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub account: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Worker,
    WorkerFailover,
    MainContinuation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Default,
    Available,
    CatalogUnavailable,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    MissingAccount,
    MissingHome,
    MissingLogin,
    Orchestrator,
    SameHome,
    SameLogin,
    AlreadyAttempted,
    RunningWorker,
    QueueFull,
    InvalidModel,
    CatalogUnavailable,
    UnknownCapacity,
    CachedQuotaExhausted,
    StaleQuota,
    BindingChanged,
    NotWorker,
    SameProviderAutomaticDisabled,
}

impl Reason {
    pub fn label(&self) -> &'static str {
        match self {
            Self::MissingAccount => "account unavailable",
            Self::MissingHome => "explicit home unavailable",
            Self::MissingLogin => "native login missing",
            Self::Orchestrator => "current orchestrator",
            Self::SameHome => "same home as the previous target",
            Self::SameLogin => "same native login as the previous target",
            Self::AlreadyAttempted => "already attempted",
            Self::RunningWorker => "worker already running",
            Self::QueueFull => "worker queue full",
            Self::InvalidModel => "model absent from local native catalog",
            Self::CatalogUnavailable => "native catalog unavailable for explicit model",
            Self::UnknownCapacity => "capacity unknown; no native probe",
            Self::CachedQuotaExhausted => "cached quota exhausted; availability not verified",
            Self::StaleQuota => "cached quota observation is stale",
            Self::BindingChanged => "account/home/login binding changed since approval",
            Self::NotWorker => "account is not enrolled as a worker",
            Self::SameProviderAutomaticDisabled => {
                "same-provider automatic handoff is off; confirm explicitly"
            }
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct QuotaObservation {
    pub observed_at: Option<u64>,
    pub fresh: bool,
    pub exhausted: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub target: Target,
    pub provider: Option<ProviderKind>,
    pub home_id: Option<String>,
    #[serde(skip)]
    pub home: Option<PathBuf>,
    pub registered: bool,
    pub logged_in: bool,
    pub model_status: ModelStatus,
    pub orchestrator: bool,
    pub same_home: bool,
    pub same_login: bool,
    pub attempted: bool,
    pub running: bool,
    pub queued: usize,
    pub quota: QuotaObservation,
    pub remaining_pct: Option<f64>,
    pub model_tier: crate::models::ModelTier,
}

#[derive(Debug, Clone)]
pub struct Context {
    pub purpose: Purpose,
    pub owner_account: String,
    pub previous_provider: ProviderKind,
    pub previous_home: Option<PathBuf>,
    pub previous_account: String,
    pub previous_model: Option<String>,
    pub attempted_accounts: BTreeSet<String>,
    pub attempted_pairs: BTreeSet<(String, String)>,
    pub running: BTreeSet<String>,
    pub queued: BTreeMap<String, usize>,
    pub queue_limit: usize,
    pub allow_same_target: bool,
    pub allow_same_provider_automatic: bool,
    pub workers: Option<BTreeSet<String>>,
}

impl Context {
    pub fn new(
        purpose: Purpose,
        account: &str,
        provider: ProviderKind,
        home: Option<PathBuf>,
        model: Option<String>,
    ) -> Self {
        Self {
            purpose,
            owner_account: account.into(),
            previous_provider: provider,
            previous_home: home,
            previous_account: account.into(),
            previous_model: model,
            attempted_accounts: BTreeSet::new(),
            attempted_pairs: BTreeSet::new(),
            running: BTreeSet::new(),
            queued: BTreeMap::new(),
            queue_limit: crate::delegate::MAX_QUEUED_PER_WORKER,
            allow_same_target: false,
            allow_same_provider_automatic: false,
            workers: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Evaluated {
    pub candidate: Candidate,
    pub eligible: bool,
    pub blocked: Vec<Reason>,
    pub observations: Vec<Reason>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub policy_revision: String,
    pub observed_at: u64,
    pub purpose: Purpose,
    pub candidates: Vec<Evaluated>,
    pub incomplete: bool,
}

pub fn evaluate(candidate: Candidate, context: &Context) -> Evaluated {
    let mut blocked = Vec::new();
    if context
        .workers
        .as_ref()
        .is_some_and(|workers| !workers.contains(&candidate.target.account))
    {
        blocked.push(Reason::NotWorker);
    }
    if !candidate.registered {
        blocked.push(Reason::MissingAccount);
    }
    if context.purpose == Purpose::MainContinuation
        && candidate.provider == Some(context.previous_provider)
        && candidate.target.account != context.previous_account
        && !context.allow_same_provider_automatic
    {
        blocked.push(Reason::SameProviderAutomaticDisabled);
    }
    if candidate.home.is_none() {
        blocked.push(Reason::MissingHome);
    }
    if !candidate.logged_in {
        blocked.push(Reason::MissingLogin);
    }
    if candidate.orchestrator && context.purpose != Purpose::MainContinuation {
        blocked.push(Reason::Orchestrator);
    }
    let same_account_model_change = matches!(
        context.purpose,
        Purpose::MainContinuation | Purpose::WorkerFailover
    ) && candidate.target.account == context.previous_account
        && candidate.target.model.is_some()
        && candidate.target.model != context.previous_model;
    if !same_account_model_change && !context.allow_same_target {
        if candidate.same_home {
            blocked.push(Reason::SameHome);
        }
        if candidate.same_login {
            blocked.push(Reason::SameLogin);
        }
    }
    if candidate.attempted {
        blocked.push(Reason::AlreadyAttempted);
    }
    if candidate.running && context.purpose == Purpose::WorkerFailover {
        blocked.push(Reason::RunningWorker);
    }
    if context.purpose == Purpose::Worker && candidate.queued >= context.queue_limit {
        blocked.push(Reason::QueueFull);
    }
    match candidate.model_status {
        ModelStatus::Absent => blocked.push(Reason::InvalidModel),
        ModelStatus::CatalogUnavailable => blocked.push(Reason::CatalogUnavailable),
        _ => {}
    }
    let mut observations = vec![Reason::UnknownCapacity];
    if candidate.quota.observed_at.is_some() && !candidate.quota.fresh {
        observations.push(Reason::StaleQuota);
    }
    if candidate.quota.exhausted {
        observations.push(Reason::CachedQuotaExhausted);
    }
    Evaluated {
        eligible: blocked.is_empty(),
        candidate,
        blocked,
        observations,
    }
}

pub fn capture(root: &Path, targets: &[Target], context: &Context) -> Decision {
    let previous_home = context
        .previous_home
        .as_ref()
        .and_then(|home| home.canonicalize().ok());
    let previous_login = previous_home
        .as_ref()
        .and_then(|home| identity_key(context.previous_provider, home));
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    for target in targets.iter().take(MAX_CANDIDATES) {
        let target = Target {
            account: normalize_account_id(&target.account),
            model: target.model.clone(),
        };
        if !seen.insert((target.account.clone(), target.model.clone())) {
            continue;
        }
        let account = get_account(root, &target.account);
        let provider = account.as_ref().map(|account| account.provider);
        let home = account
            .as_ref()
            .and_then(|account| account.home_path.as_ref())
            .and_then(|home| home.canonicalize().ok());
        let logged_in = provider
            .zip(home.as_ref())
            .is_some_and(|(provider, home)| is_logged_in(provider, home));
        let login = provider
            .zip(home.as_ref())
            .and_then(|(provider, home)| identity_key(provider, home));
        let model_status = match (&target.model, provider.zip(home.as_ref())) {
            (None, _) => ModelStatus::Default,
            (Some(model), Some((provider, home))) => {
                let models = crate::catalog::list_models(provider, home);
                if models.contains(model) {
                    ModelStatus::Available
                } else if models.is_empty() {
                    ModelStatus::CatalogUnavailable
                } else {
                    ModelStatus::Absent
                }
            }
            _ => ModelStatus::CatalogUnavailable,
        };
        let cached = provider.zip(home.as_ref()).and_then(|(provider, home)| {
            crate::usage::peek_cached_usage(root, provider, &target.account, home)
        });
        let remaining_pct = cached.as_ref().and_then(|o| o.remaining_pct);
        let quota = cached
            .map(|observation| QuotaObservation {
                observed_at: Some(observation.observed_at),
                fresh: observation.fresh,
                exhausted: observation.exhausted,
                source: "home-bound cached native usage; login freshness not verified".into(),
            })
            .unwrap_or_else(|| QuotaObservation {
                source: "no cached observation; no probe requested".into(),
                ..Default::default()
            });
        let candidate = Candidate {
            registered: account.is_some(),
            provider,
            home_id: crate::token_log::Attribution::bound_home(home.as_deref()),
            same_home: home.is_some() && home == previous_home,
            same_login: provider == Some(context.previous_provider)
                && previous_login.is_some()
                && login == previous_login,
            orchestrator: target.account == context.owner_account,
            attempted: (context.attempted_accounts.contains(&target.account)
                && target.model.is_none())
                || target.model.as_ref().is_some_and(|model| {
                    context
                        .attempted_pairs
                        .contains(&(target.account.clone(), model.clone()))
                }),
            running: context.running.contains(&target.account),
            queued: context.queued.get(&target.account).copied().unwrap_or(0),
            model_tier: target
                .model
                .as_deref()
                .map(crate::models::catalog_tier)
                .unwrap_or_default(),
            target,
            home,
            logged_in,
            model_status,
            quota,
            remaining_pct,
        };
        candidates.push(evaluate(candidate, context));
    }
    rank_candidates(&mut candidates);
    let policy =
        serde_json::to_vec(&(context.purpose, targets, context.queue_limit)).unwrap_or_default();
    Decision {
        policy_revision: format!("{:x}", Sha256::digest(policy)),
        observed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        purpose: context.purpose,
        candidates,
        incomplete: targets.len() > MAX_CANDIDATES,
    }
}

pub fn rank_candidates(candidates: &mut [Evaluated]) {
    candidates.sort_by(|a, b| {
        b.candidate
            .remaining_pct
            .is_some()
            .cmp(&a.candidate.remaining_pct.is_some())
            .then_with(|| {
                b.candidate
                    .remaining_pct
                    .partial_cmp(&a.candidate.remaining_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| b.candidate.model_tier.cmp(&a.candidate.model_tier))
    });
}

impl Decision {
    pub fn summary(&self) -> String {
        let selected = self
            .selected()
            .map(|row| {
                format!(
                    "{} / {}",
                    row.candidate.target.account,
                    row.candidate
                        .target
                        .model
                        .as_deref()
                        .unwrap_or("native default")
                )
            })
            .unwrap_or_else(|| "none".into());
        let mut lines = vec![format!(
            "Routing selected {selected}; capacity remains unknown."
        )];
        for row in &self.candidates {
            if !row.eligible {
                lines.push(format!(
                    "Skipped {} / {}: {}",
                    row.candidate.target.account,
                    row.candidate
                        .target
                        .model
                        .as_deref()
                        .unwrap_or("native default"),
                    row.blocked
                        .iter()
                        .map(Reason::label)
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        }
        lines
            .join("\n")
            .chars()
            .filter(|ch| !ch.is_control() || *ch == '\n')
            .take(8000)
            .collect()
    }

    pub fn selected(&self) -> Option<&Evaluated> {
        if self.purpose == Purpose::Worker {
            self.candidates
                .iter()
                .find(|candidate| candidate.eligible && !candidate.candidate.running)
                .or_else(|| {
                    self.candidates
                        .iter()
                        .find(|candidate| candidate.eligible && candidate.candidate.queued == 0)
                })
        } else {
            self.candidates.iter().find(|candidate| candidate.eligible)
        }
    }

    /// One compact, deterministic explanation suitable for a manual picker row.
    pub fn suggestion_reason(row: &Evaluated) -> String {
        let quota = row
            .candidate
            .remaining_pct
            .map(|pct| format!("{pct:.0}%"))
            .unwrap_or_else(|| "??".into());
        let tier = match row.candidate.model_tier {
            crate::models::ModelTier::Frontier => "frontier",
            crate::models::ModelTier::Standard => "standard",
            crate::models::ModelTier::Fast => "fast",
        };
        format!("5h {quota} left · {tier}")
    }

    pub fn report(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "Local routing snapshot {:?} · policy {} · observed {}{}",
            self.purpose,
            &self.policy_revision[..12],
            self.observed_at,
            if self.incomplete {
                " · incomplete candidate scope"
            } else {
                ""
            }
        )];
        for row in &self.candidates {
            let reasons: Vec<_> = row
                .blocked
                .iter()
                .chain(&row.observations)
                .map(Reason::label)
                .collect();
            lines.push(format!(
                "{} / {}: {} · {}",
                row.candidate.target.account,
                row.candidate
                    .target
                    .model
                    .as_deref()
                    .unwrap_or("native default"),
                if row.eligible { "eligible" } else { "skipped" },
                reasons.join("; ")
            ));
            lines.push(format!(
                "  home {} | quota observation {} | {}",
                row.candidate.home_id.as_deref().unwrap_or("unknown"),
                row.candidate
                    .quota
                    .observed_at
                    .map(|at| at.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                row.candidate.quota.source
            ));
        }
        lines
            .into_iter()
            .map(|line| {
                line.chars()
                    .filter(|ch| !ch.is_control())
                    .take(2000)
                    .collect()
            })
            .collect()
    }
}

/// Recheck the exact registered launch binding. This function never probes native quota/model APIs.
pub fn validate_launch(
    root: &Path,
    account: &str,
    provider: ProviderKind,
    home: Option<&Path>,
    model: Option<&str>,
) -> Result<(), String> {
    if !crate::store::get_settings(root)
        .workers
        .iter()
        .any(|worker| normalize_account_id(worker) == normalize_account_id(account))
    {
        return Err("account is no longer enrolled as a worker".into());
    }
    let native = get_account(root, account).ok_or("account unavailable before launch")?;
    let captured = home
        .and_then(|home| home.canonicalize().ok())
        .ok_or("captured launch home unavailable")?;
    let registered = native
        .home_path
        .as_ref()
        .and_then(|home| home.canonicalize().ok())
        .ok_or("registered home unavailable")?;
    if native.provider != provider || captured != registered {
        return Err("account/home binding changed before launch".into());
    }
    if !is_logged_in(provider, &registered) {
        return Err("native login unavailable before launch".into());
    }
    if model.is_some_and(|model| {
        !crate::catalog::list_models(provider, &registered)
            .iter()
            .any(|native| native == model)
    }) {
        return Err("explicit model cannot be verified in the current native catalog".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelTier;

    fn row(account: &str, remaining_pct: Option<f64>, tier: crate::models::ModelTier) -> Evaluated {
        Evaluated {
            candidate: Candidate {
                target: Target {
                    account: account.into(),
                    model: Some("fixture".into()),
                },
                provider: Some(ProviderKind::Codex),
                home_id: None,
                home: None,
                registered: true,
                logged_in: true,
                model_status: ModelStatus::Available,
                orchestrator: false,
                same_home: false,
                same_login: false,
                attempted: false,
                running: false,
                queued: 0,
                quota: QuotaObservation::default(),
                remaining_pct,
                model_tier: tier,
            },
            eligible: true,
            blocked: Vec::new(),
            observations: Vec::new(),
        }
    }

    #[test]
    fn ranking_prefers_remaining_quota_then_tier() {
        let mut rows = vec![
            row("standard-more", Some(80.0), ModelTier::Standard),
            row("frontier-less", Some(60.0), ModelTier::Frontier),
            row("frontier-more", Some(80.0), ModelTier::Frontier),
        ];
        rank_candidates(&mut rows);
        assert_eq!(
            rows.iter()
                .map(|r| r.candidate.target.account.as_str())
                .collect::<Vec<_>>(),
            ["frontier-more", "standard-more", "frontier-less"]
        );
    }

    #[test]
    fn unknown_quota_ranks_last() {
        let mut rows = vec![
            row("unknown", None, ModelTier::Frontier),
            row("known", Some(1.0), ModelTier::Fast),
        ];
        rank_candidates(&mut rows);
        assert_eq!(rows[1].candidate.target.account, "unknown");
    }

    #[test]
    fn suggestion_reason_uses_single_separators() {
        let row = row("fixture", Some(77.0), ModelTier::Frontier);
        let reason = Decision::suggestion_reason(&row);
        assert_eq!(reason, "5h 77% left · frontier");
        assert!(!reason.contains(&format!("{0}{0}", '·')));
    }
}
