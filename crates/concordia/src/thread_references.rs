//! Versioned, body-free journal records. Native source text is hydrated only in memory.
use super::{cap_chars, EventCard, EventUsage, ThreadEvent};
use crate::native_memory::references::{hydrate_text, CaptureScope, Reference};
use crate::native_memory::Binding;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(super) const RAW_TOOL_LABEL: &str = "Native tool records (source data; not replayed):\n";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    version: u32,
    t: u64,
    k: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    card: Option<CardMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<EventUsage>,
    source: Source,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CardMetadata {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Source {
    Metadata,
    Native {
        reference: Reference,
    },
    Unavailable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<CaptureScope>,
        reason: String,
    },
}

fn short(value: &Option<String>, limit: usize) -> Option<String> {
    value.as_deref().map(|v| cap_chars(v, limit).to_owned())
}

fn reason_code(reason: &str) -> &'static str {
    match reason {
        "missing" => "missing",
        "stale" => "stale",
        "unsupported" => "unsupported",
        "unbound" => "unbound",
        _ => "capture_failed",
    }
}

impl Record {
    fn base(ev: &ThreadEvent, source: Source) -> Result<Self> {
        if !matches!(
            ev.k.as_str(),
            "user" | "assistant" | "card" | "switch" | "note" | "usage"
        ) {
            bail!("unsupported thread event kind");
        }
        if (ev.k == "card") != ev.card.is_some() {
            bail!("thread card metadata does not match event kind");
        }
        Ok(Self {
            version: 2,
            t: ev.t,
            k: ev.k.clone(),
            provider: short(&ev.provider, 32),
            account: short(&ev.account, 128),
            model: short(&ev.model, 128),
            card: ev.card.as_ref().map(|c| CardMetadata {
                title: cap_chars(&c.title, 256).to_owned(),
                status: short(&c.status, 32),
            }),
            usage: ev.usage.clone(),
            source,
        })
    }

    fn bind(&mut self, binding: &Binding) -> Result<()> {
        if self
            .provider
            .as_deref()
            .is_some_and(|p| p != binding.provider.as_str())
            || self
                .account
                .as_deref()
                .is_some_and(|a| a != binding.account)
        {
            bail!("thread event owner differs from exact native source binding");
        }
        self.provider = Some(binding.provider.as_str().to_owned());
        self.account = Some(binding.account.clone());
        Ok(())
    }

    pub(super) fn metadata(ev: &ThreadEvent) -> Result<Self> {
        if ev.text.as_deref().is_some_and(|t| !t.is_empty())
            || ev
                .card
                .as_ref()
                .and_then(|c| c.detail.as_deref())
                .is_some_and(|t| !t.is_empty())
            || !matches!(ev.k.as_str(), "switch" | "note" | "usage")
        {
            bail!(
                "thread body requires a verified native reference or explicit unavailable record"
            );
        }
        Self::base(ev, Source::Metadata)
    }

    pub(super) fn referenced(root: &Path, ev: &ThreadEvent, reference: Reference) -> Result<Self> {
        let role = if ev.k == "card" {
            "tool"
        } else {
            ev.k.as_str()
        };
        if !matches!(role, "user" | "assistant" | "tool") || reference.role() != role {
            bail!("thread event role differs from exact native source projection");
        }
        if role == "tool" && reference.native_call_id().is_none_or(str::is_empty) {
            bail!("thread tool reference has no exact native call identity");
        }
        let projected = hydrate_text(root, &reference)
            .map_err(|_| anyhow!("native thread reference failed verification"))?;
        let image_only_user =
            role == "user" && ev.text.as_deref() == Some("") && !projected.is_empty();
        if role != "tool" && !image_only_user && ev.text.as_deref() != Some(projected.as_str()) {
            bail!("thread body differs from exact native source projection");
        }
        let mut record = Self::base(
            ev,
            Source::Native {
                reference: reference.clone(),
            },
        )?;
        record.bind(reference.binding())?;
        Ok(record)
    }

    pub(super) fn unavailable(
        ev: &ThreadEvent,
        scope: Option<&CaptureScope>,
        reason: &str,
    ) -> Result<Self> {
        let mut record = Self::base(
            ev,
            Source::Unavailable {
                scope: scope.cloned(),
                reason: reason_code(reason).to_owned(),
            },
        )?;
        if let Some(scope) = scope {
            record.bind(scope.binding())?;
        }
        Ok(record)
    }

    pub(super) fn hydrate(self, root: &Path) -> Result<ThreadEvent> {
        if self.version != 2 {
            bail!(
                "unsupported referenced thread event version {}",
                self.version
            );
        }
        if !matches!(
            self.k.as_str(),
            "user" | "assistant" | "card" | "switch" | "note" | "usage"
        ) || (self.k == "card") != self.card.is_some()
            || (matches!(&self.source, Source::Metadata)
                && !matches!(self.k.as_str(), "switch" | "note" | "usage"))
        {
            bail!("invalid referenced thread event metadata");
        }
        let mut ev = ThreadEvent {
            t: self.t,
            k: self.k,
            provider: self.provider,
            account: self.account,
            model: self.model,
            text: None,
            usage: self.usage,
            card: self.card.map(|c| EventCard {
                title: c.title,
                detail: None,
                status: c.status,
            }),
        };
        let body = match self.source {
            Source::Metadata => return Ok(ev),
            Source::Unavailable { scope, reason } => {
                let owner_matches = scope.as_ref().is_none_or(|s| {
                    ev.provider.as_deref() == Some(s.binding().provider.as_str())
                        && ev.account.as_deref() == Some(s.binding().account.as_str())
                });
                let reason = if owner_matches {
                    reason_code(&reason)
                } else {
                    "binding_mismatch"
                };
                format!("[native source unavailable: {reason}]")
            }
            Source::Native { reference } => {
                let role = if ev.k == "card" {
                    "tool"
                } else {
                    ev.k.as_str()
                };
                let owner_matches = ev.provider.as_deref()
                    == Some(reference.binding().provider.as_str())
                    && ev.account.as_deref() == Some(reference.binding().account.as_str());
                if !owner_matches
                    || reference.role() != role
                    || (role == "tool" && reference.native_call_id().is_none_or(str::is_empty))
                {
                    "[native source unavailable: binding_mismatch]".to_owned()
                } else {
                    match hydrate_text(root, &reference) {
                        Ok(text) if role == "tool" => format!("{RAW_TOOL_LABEL}{text}"),
                        Ok(text) => text,
                        Err(_) => "[native source unavailable: verification_failed]".to_owned(),
                    }
                }
            }
        };
        if let Some(card) = &mut ev.card {
            card.detail = Some(body);
        } else {
            ev.text = Some(body);
        }
        Ok(ev)
    }
}
