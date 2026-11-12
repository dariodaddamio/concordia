//! Explicit per-account context policy. Native measurements are never inferred from billing.
use crate::{native_memory::Binding, store::ProviderKind};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[path = "provider/claude_capacity.rs"]
pub mod claude_capacity;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Compaction {
    #[default]
    Native,
    ClaudeAutomaticOff,
    CodexThreshold(u64),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Policy {
    pub rollover_percent: Option<u8>,
    pub compaction: Compaction,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    binding: Binding,
    policy: Policy,
}

fn key(binding: &Binding) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\n{}\n{}",
            binding.provider.as_str(),
            binding.account,
            binding.home.display()
        ))
    )
}

fn validate(provider: ProviderKind, policy: &Policy) -> Result<(), String> {
    if policy
        .rollover_percent
        .is_some_and(|n| !(50..=95).contains(&n))
    {
        return Err("rollover percentage must be 50..95".into());
    }
    if policy.rollover_percent.is_some() && provider == ProviderKind::Cursor {
        return Err(format!(
            "proactive capacity unavailable: {}",
            unavailable_reason(provider)
        ));
    }
    match (&policy.compaction, provider) {
        (Compaction::Native, _) | (Compaction::ClaudeAutomaticOff, ProviderKind::Claude) => Ok(()),
        (Compaction::CodexThreshold(n), ProviderKind::Codex) if (1024..=10_000_000).contains(n) => {
            Ok(())
        }
        _ => Err("unsupported native compaction control for this provider".into()),
    }
}

pub fn load(root: &Path, binding: &Binding) -> Result<Policy, String> {
    use std::io::Read;
    let path = root
        .join(".ao/context-policy")
        .join(format!("{}.json", key(binding)));
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Policy::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(16385)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16384 {
        return Err("context policy exceeds budget".into());
    }
    let saved: Saved = serde_json::from_slice(&bytes).map_err(|_| "invalid context policy")?;
    if saved.binding.account != binding.account
        || saved.binding.provider != binding.provider
        || saved.binding.home != binding.home
    {
        return Err("context policy account/home mismatch".into());
    }
    validate(binding.provider, &saved.policy)?;
    Ok(saved.policy)
}

/// Persist the retained rollover setting for synthetic and UI configuration paths.
/// Compaction overrides are provider-owned and no longer configurable here.
pub fn set(root: &Path, binding: &Binding, command: &str) -> Result<String, String> {
    let mut policy = load(root, binding)?;
    let args: Vec<_> = command.split_whitespace().collect();
    match args.as_slice() {
        ["rollover", "off"] => policy.rollover_percent = None,
        ["rollover", percent] => {
            policy.rollover_percent = Some(
                percent
                    .parse()
                    .map_err(|_| "rollover percentage must be 50..95")?,
            )
        }
        ["status"] | [] => {}
        _ => return Err("only rollover 50..95|off is supported".into()),
    }
    validate(binding.provider, &policy)?;
    if !args.is_empty() && args != ["status"] {
        let bytes = serde_json::to_vec(&Saved {
            binding: binding.clone(),
            policy: policy.clone(),
        })
        .map_err(|e| e.to_string())?;
        crate::state_io::write_atomic(
            &root
                .join(".ao/context-policy")
                .join(format!("{}.json", key(binding))),
            &bytes,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(format!(
        "Proactive rollover for {}: {}",
        binding.account,
        policy
            .rollover_percent
            .map_or("off".into(), |n| format!("at {n}%"))
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement {
    pub used: u64,
    pub window: u64,
}

/// An observation is evidence for one exact source snapshot, never an account-wide cache.
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub binding: Binding,
    pub requested_model: String,
    pub native_model: Option<String>,
    pub native_turn_id: Option<String>,
    pub transport: &'static str,
    pub evidence_version: &'static str,
    pub source: Option<SourceEvidence>,
    pub control: Option<ControlEvidence>,
    pub measurement: Option<Measurement>,
    pub invalidation_reason: Option<String>,
}

/// A native control response has connection/request provenance, rather than a file identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlEvidence {
    pub connection_id: String,
    pub request_id: String,
    pub response_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub file_identity: String,
    pub source_bytes: u64,
    pub tail_start: u64,
    pub tail_sha256: String,
    pub record_start: Option<u64>,
    pub record_bytes: Option<usize>,
    pub record_sha256: Option<String>,
}

pub fn unavailable_reason(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Codex => "no fresh, exact-turn Codex rollout last_token_usage/model_context_window evidence",
        ProviderKind::Claude => "requires native stdio on the qualified Claude CLI and a fresh exact-session/model get_context_usage response",
        ProviderKind::Cursor => "no negotiated current-context/window signal from the selected Cursor transport; schema fields alone are not capability evidence",
        ProviderKind::Grok => "Grok ACP does not currently expose a supported current-context/window usage signal",
    }
}

impl Observation {
    pub fn notice(&self, rollover: bool) -> String {
        let status = self.measurement.map_or_else(
            || {
                format!(
                    "Native context unknown: {}; proactive rollover skipped",
                    self.invalidation_reason
                        .as_deref()
                        .unwrap_or("no verified measurement")
                )
            },
            |measurement| {
                format!(
                    "Native context: {} / {} tokens{}",
                    measurement.used,
                    measurement.window,
                    if rollover {
                        "; starting a fresh session before this turn"
                    } else {
                        ""
                    }
                )
            },
        );
        let revision = self
            .source
            .as_ref()
            .map(|source| source.tail_sha256.as_str())
            .or_else(|| {
                self.control
                    .as_ref()
                    .map(|control| control.response_sha256.as_str())
            })
            .unwrap_or("unavailable");
        format!("{status}\nEvidence {} · {} · account {} · session {} · turn {} · model {} · source revision {}",
            self.evidence_version, self.transport, self.binding.account, self.binding.session_id,
            self.native_turn_id.as_deref().unwrap_or("unknown"), self.native_model.as_deref().unwrap_or("unknown"), revision)
    }
}

impl Policy {
    pub fn rollover(&self, measurement: Option<Measurement>) -> bool {
        match (self.rollover_percent, measurement) {
            (Some(percent), Some(m)) if m.window > 0 => {
                (m.used as u128) * 100 >= (m.window as u128) * (percent as u128)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_and_cumulative_usage_never_trigger_policy_and_controls_are_provider_bound() {
        let policy = Policy {
            rollover_percent: Some(80),
            compaction: Compaction::Native,
        };
        assert!(!policy.rollover(None));
        assert!(!policy.rollover(Some(Measurement {
            used: 799,
            window: 1000
        })));
        assert!(policy.rollover(Some(Measurement {
            used: 800,
            window: 1000
        })));
        assert!(!policy.rollover(Some(Measurement {
            used: u64::MAX,
            window: 0
        })));
        assert!(validate(ProviderKind::Cursor, &policy).is_err());
        assert!(validate(
            ProviderKind::Claude,
            &Policy {
                rollover_percent: None,
                compaction: Compaction::CodexThreshold(8000)
            }
        )
        .is_err());
    }

    #[test]
    fn fresh_triggers_at_threshold_with_map_seed() {
        let policy = Policy {
            rollover_percent: Some(85),
            compaction: Compaction::Native,
        };
        assert!(!policy.rollover(Some(Measurement {
            used: 849,
            window: 1000
        })));
        assert!(policy.rollover(Some(Measurement {
            used: 850,
            window: 1000
        })));
    }

    #[test]
    fn unreported_context_disables_fresh_with_reason() {
        let policy = Policy {
            rollover_percent: Some(85),
            compaction: Compaction::Native,
        };
        assert!(!policy.rollover(None));
        assert!(unavailable_reason(ProviderKind::Cursor).contains("no negotiated"));
    }
}
