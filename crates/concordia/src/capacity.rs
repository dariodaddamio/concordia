//! Explicit per-account context policy. Native measurements are never inferred from billing.
use crate::{native_memory::Binding, provider::PlannedTurn, store::ProviderKind};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    if policy.rollover_percent.is_some() && provider != ProviderKind::Codex {
        return Err(
            "proactive capacity is unavailable for this provider; native capacity remains unknown"
                .into(),
        );
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

pub fn set(root: &Path, binding: &Binding, command: &str) -> Result<String, String> {
    let mut policy = load(root, binding)?;
    let args: Vec<_> = command.split_whitespace().collect();
    match args.as_slice() {
        [] | ["status"] => {}
        ["rollover", "off"] => policy.rollover_percent = None,
        ["rollover", percent] => {
            policy.rollover_percent = Some(
                percent
                    .parse()
                    .map_err(|_| "use /context rollover 50..95 or off")?,
            )
        }
        ["compact", "native"] => policy.compaction = Compaction::Native,
        ["compact", "automatic-off"] => policy.compaction = Compaction::ClaudeAutomaticOff,
        ["compact", "tokens", tokens] => {
            policy.compaction = Compaction::CodexThreshold(
                tokens
                    .parse()
                    .map_err(|_| "compaction tokens must be an integer")?,
            )
        }
        _ => {
            return Err(
                "/context [status | rollover 50..95|off | compact native|automatic-off|tokens N]"
                    .into(),
            )
        }
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
    Ok(format!("Context policy for {}: proactive rollover {}; native compaction {:?}. Applies to future turns in this account/home. Capacity is unknown until its exact native session supplies a valid measurement. /context rollover off and /context compact native restore defaults. No provider configuration files are edited.", binding.account, policy.rollover_percent.map_or("off".into(), |n| format!("at {n}%")), policy.compaction))
}

/// Apply only documented per-process controls. No credential/config mutation or trust changes.
pub fn apply(plan: &mut PlannedTurn, compaction: &Compaction) -> Result<(), String> {
    validate(
        plan.provider,
        &Policy {
            rollover_percent: None,
            compaction: compaction.clone(),
        },
    )?;
    match compaction {
        Compaction::Native => {}
        Compaction::ClaudeAutomaticOff => {
            plan.env.push(("DISABLE_AUTO_COMPACT".into(), "1".into()))
        }
        Compaction::CodexThreshold(n) => {
            plan.argv.splice(
                1..1,
                ["-c".into(), format!("model_auto_compact_token_limit={n}")],
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Measurement {
    pub used: u64,
    pub window: u64,
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
    fn native_compaction_controls_are_process_local_and_reversible() {
        let dir = tempfile::tempdir().unwrap();
        for provider in [
            ProviderKind::Codex,
            ProviderKind::Claude,
            ProviderKind::Cursor,
        ] {
            let mut plan = PlannedTurn {
                provider,
                command: "fake".into(),
                argv: vec!["exec".into(), "literal prompt".into()],
                prompt: "literal prompt".into(),
                new_session_id: None,
                env: Vec::new(),
                cwd: dir.path().display().to_string(),
            };
            apply(&mut plan, &Compaction::Native).unwrap();
            assert!(plan.env.is_empty());
            let control = if provider == ProviderKind::Claude {
                Compaction::ClaudeAutomaticOff
            } else {
                Compaction::CodexThreshold(8000)
            };
            if provider == ProviderKind::Cursor {
                assert!(apply(&mut plan, &control).is_err());
                continue;
            }
            apply(&mut plan, &control).unwrap();
            assert_eq!(plan.argv.last().unwrap(), "literal prompt");
            if provider == ProviderKind::Claude {
                assert_eq!(plan.env, vec![("DISABLE_AUTO_COMPACT".into(), "1".into())]);
            } else {
                assert!(plan
                    .argv
                    .iter()
                    .any(|a| a == "model_auto_compact_token_limit=8000"));
            }
        }
    }
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
}
