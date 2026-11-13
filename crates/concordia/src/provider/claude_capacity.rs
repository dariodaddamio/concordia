//! Reader for Claude Agent SDK's bidirectional `get_context_usage` response.
//!
//! This module deliberately does not spawn Claude or invent source/file provenance. The
//! persistent transport owns connection generation and turn correlation; it passes those exact
//! identities into `read` and the coordinator converts the result into the existing Observation.

use crate::store::ProviderKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// Current official control schema matched against the installed CLI and initialize-only receipt.
/// This identifies protocol support, not a claim of live model-turn qualification.
pub const CLI_VERSION: &str = "2.1.319";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsage {
    pub binding: ContextBinding,
    pub total_tokens: u64,
    pub effective_max_tokens: u64,
    pub raw_max_tokens: u64,
    pub model: String,
    pub percentage_basis: u64,
    pub auto_compact_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBinding {
    pub provider_version: String,
    pub account: String,
    pub home: PathBuf,
    pub session_id: String,
    pub control_request_id: String,
    pub connection_generation: String,
    pub requested_model: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireUsage {
    total_tokens: u64,
    max_tokens: u64,
    raw_max_tokens: u64,
    percentage: f64,
    model: String,
    is_auto_compact_enabled: bool,
}

/// Parse one exact SDK control response. The caller must retain the binding and connection
/// generation; a parsed response is never valid for another session, model, turn, or connection.
pub fn read(response: &[u8], binding: ContextBinding) -> Result<ContextUsage, String> {
    if response.is_empty() || response.len() > MAX_RESPONSE_BYTES {
        return Err("Claude context response is missing or oversized".into());
    }
    if binding.provider_version.trim().is_empty()
        || binding.account.trim().is_empty()
        || binding.home.as_os_str().is_empty()
        || binding.session_id.trim().is_empty()
        || binding.control_request_id.trim().is_empty()
        || binding.connection_generation.trim().is_empty()
        || binding.requested_model.trim().is_empty()
    {
        return Err("Claude context response has incomplete identity".into());
    }
    let wire: WireUsage = serde_json::from_slice(response)
        .map_err(|_| "Claude context response is malformed".to_string())?;
    if wire.model.trim().is_empty()
        || wire.model != binding.requested_model
        || wire.max_tokens == 0
        || wire.raw_max_tokens < wire.max_tokens
        || wire.total_tokens > wire.max_tokens
        || !wire.percentage.is_finite()
        || !(0.0..=100.0).contains(&wire.percentage)
    {
        return Err("Claude context response failed model/window validation".into());
    }
    // The displayed percentage is metadata; policy uses the widened exact token ratio.
    let percentage_basis = ((wire.total_tokens as u128 * 100) / wire.max_tokens as u128) as u64;
    Ok(ContextUsage {
        binding,
        total_tokens: wire.total_tokens,
        effective_max_tokens: wire.max_tokens,
        raw_max_tokens: wire.raw_max_tokens,
        model: wire.model,
        percentage_basis,
        auto_compact_enabled: wire.is_auto_compact_enabled,
    })
}

/// Compile-time marker for coordinator capability registration.
pub const TRANSPORT: &str = "claude-agent-sdk-control/get_context_usage";
pub const PROVIDER: ProviderKind = ProviderKind::Claude;

const CACHE_VERSION: u8 = 1;
const CACHE_BYTES: usize = 16 * 1024;
const CACHE_FILES: usize = 256;

#[derive(Debug, Serialize, Deserialize)]
struct SavedObservation {
    version: u8,
    provider_version: String,
    compaction: crate::capacity::Compaction,
    binding: crate::native_memory::Binding,
    model: String,
    source: crate::capacity::SourceEvidence,
    measurement: crate::capacity::Measurement,
    control: crate::capacity::ControlEvidence,
}

fn cache_path(
    root: &Path,
    binding: &crate::native_memory::Binding,
    model: &str,
) -> Result<PathBuf, String> {
    if binding.provider != ProviderKind::Claude || model.trim().is_empty() {
        return Err("Claude observation identity is invalid".into());
    }
    let key = format!(
        "{}\n{}\n{}\n{}\n{}",
        binding.provider.as_str(),
        binding.account,
        binding.home.display(),
        binding.session_id,
        model
    );
    Ok(root
        .join(".ao")
        .join("context-observations")
        .join(format!("{:x}.json", Sha256::digest(key.as_bytes()))))
}

fn regular(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta)
            if !meta.is_file()
                || crate::workspaces::linked(&meta)
                || meta.len() > CACHE_BYTES as u64 =>
        {
            Err("Claude context metadata must remain a bounded regular file".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn directory(root: &Path, create: bool) -> Result<(), String> {
    for path in [root.join(".ao"), root.join(".ao/context-observations")] {
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_dir() || crate::workspaces::linked(&meta) => {
                return Err("Claude context metadata directory must not be linked".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if create {
                    std::fs::create_dir(&path).map_err(|error| error.to_string())?;
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn unknown(
    binding: &crate::native_memory::Binding,
    model: &str,
    reason: impl Into<String>,
) -> crate::capacity::Observation {
    crate::capacity::Observation {
        binding: binding.clone(),
        requested_model: model.into(),
        native_model: None,
        native_turn_id: None,
        transport: TRANSPORT,
        evidence_version: "sdk-control-cache-v1",
        source: None,
        control: None,
        measurement: None,
        invalidation_reason: Some(reason.into()),
    }
}

/// Persist one post-turn control response and its exact native-memory checkpoint.
pub fn save(
    root: &Path,
    binding: &crate::native_memory::Binding,
    model: &str,
    source: crate::capacity::SourceEvidence,
    measurement: crate::capacity::Measurement,
    control: crate::capacity::ControlEvidence,
    compaction: &crate::capacity::Compaction,
) -> Result<(), String> {
    let path = cache_path(root, binding, model)?;
    let dir = path
        .parent()
        .ok_or("Claude observation cache path is invalid")?;
    directory(root, true)?;
    let bytes = serde_json::to_vec(&SavedObservation {
        version: CACHE_VERSION,
        provider_version: CLI_VERSION.into(),
        compaction: compaction.clone(),
        binding: binding.clone(),
        model: model.into(),
        source,
        measurement,
        control,
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() > CACHE_BYTES {
        return Err("Claude observation cache record exceeds its byte limit".into());
    }
    let lock = dir.join("catalog.lock");
    regular(&lock)?;
    crate::state_io::with_lock(&lock, || {
        regular(&path).map_err(std::io::Error::other)?;
        if path.exists() {
            let mut old = Vec::new();
            std::fs::File::open(&path)?
                .take((CACHE_BYTES + 1) as u64)
                .read_to_end(&mut old)?;
            if old.len() > CACHE_BYTES {
                return Err(std::io::Error::other(
                    "oversized Claude context metadata retained",
                ));
            }
            let old: SavedObservation =
                serde_json::from_slice(&old).map_err(std::io::Error::other)?;
            if old.version != CACHE_VERSION {
                return Err(std::io::Error::other(
                    "unknown Claude context metadata version; original retained",
                ));
            }
        }
        if !path.exists() {
            let count = std::fs::read_dir(dir)?.take(CACHE_FILES * 2 + 1).count();
            if count >= CACHE_FILES {
                return Err(std::io::Error::other(
                    "Claude observation cache reached its file limit",
                ));
            }
        }
        crate::state_io::write_atomic(&path, &bytes)
    })
    .map_err(|e| e.to_string())
}

/// Load a cached post-turn measurement only when the current native-memory checkpoint is exact.
pub fn load(
    root: &Path,
    binding: &crate::native_memory::Binding,
    model: &str,
    current: &crate::capacity::SourceEvidence,
) -> Result<crate::capacity::Observation, String> {
    let path = cache_path(root, binding, model)?;
    directory(root, false)?;
    regular(&path)?;
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(unknown(
                binding,
                model,
                "no post-turn Claude context observation is cached",
            ))
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take((CACHE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > CACHE_BYTES {
        return Ok(unknown(
            binding,
            model,
            "cached Claude context observation exceeds its byte limit",
        ));
    }
    let saved: SavedObservation = serde_json::from_slice(&bytes)
        .map_err(|_| "cached Claude context observation is malformed".to_string())?;
    if saved.version != CACHE_VERSION
        || saved.provider_version != CLI_VERSION
        || saved.compaction != crate::capacity::load(root, binding)?.compaction
        || saved.binding != *binding
        || saved.model != model
        || saved.source != *current
        || saved.measurement.window == 0
        || saved.measurement.used > saved.measurement.window
        || uuid::Uuid::parse_str(&saved.control.connection_id).is_err()
        || uuid::Uuid::parse_str(&saved.control.request_id).is_err()
        || saved.control.response_sha256.len() != 64
        || !saved
            .control
            .response_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Ok(unknown(
            binding,
            model,
            "cached Claude context observation is stale or belongs to another source",
        ));
    }
    Ok(crate::capacity::Observation {
        binding: binding.clone(),
        requested_model: model.into(),
        native_model: Some(model.into()),
        native_turn_id: None,
        transport: TRANSPORT,
        evidence_version: "sdk-control-cache-v1",
        source: Some(saved.source),
        control: Some(saved.control),
        measurement: Some(saved.measurement),
        invalidation_reason: None,
    })
}
