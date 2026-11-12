//! Read real login/model from provider homes — never invent identities.

use crate::store::ProviderKind;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProviderIdentity {
    pub provider: ProviderKind,
    pub home_path: PathBuf,
    pub login: Option<String>,
    pub team: Option<String>,
    pub plan: Option<String>,
    pub model: Option<String>,
    pub logged_in: bool,
}

pub fn format_identity_label(id: &ProviderIdentity) -> String {
    match (&id.login, &id.team) {
        (Some(login), Some(team)) if !team.is_empty() => format!("{login} · {team}"),
        (Some(login), _) => login.clone(),
        (None, Some(team)) if !team.is_empty() => team.clone(),
        _ if id.logged_in => "logged in".into(),
        _ => "logged out".into(),
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&crate::fs_safe::read_text_bounded(path, 1024 * 1024)?).ok()
}

fn jwt_claims(token: &str) -> Option<Value> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let b64 = parts[1].replace('-', "+").replace('_', "/");
    let padded = match b64.len() % 4 {
        2 => format!("{b64}=="),
        3 => format!("{b64}="),
        _ => b64,
    };
    let decoded = decode_b64(&padded)?;
    serde_json::from_slice(&decoded).ok()
}

fn email_from_jwt(token: &str) -> Option<String> {
    let v = jwt_claims(token)?;
    v.get("email")
        .or_else(|| {
            v.get("https://api.openai.com/profile")
                .and_then(|p| p.get("email"))
        })
        .or_else(|| v.get("preferred_username"))
        .or_else(|| v.get("upn"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn decode_b64(s: &str) -> Option<Vec<u8>> {
    // Minimal URL-safe base64 decode without extra crate.
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for b in bytes {
        if b == b'=' {
            break;
        }
        let val = T.iter().position(|&c| c == b)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

pub fn parse_toml_model(toml: &str) -> Option<String> {
    toml::from_str::<toml::Value>(toml)
        .ok()?
        .get("model")?
        .as_str()
        .map(str::to_owned)
}

pub fn inspect_codex_home(home: &Path) -> ProviderIdentity {
    let auth = read_json(&home.join("auth.json"));
    let model = crate::fs_safe::read_text_bounded(&home.join("config.toml"), 1024 * 1024)
        .and_then(|t| parse_toml_model(&t));
    let tokens = auth.as_ref().and_then(|a| a.get("tokens"));
    let id_token = tokens
        .and_then(|t| t.get("id_token"))
        .and_then(|x| x.as_str());
    let access = tokens
        .and_then(|t| t.get("access_token"))
        .and_then(|x| x.as_str());
    let logged_in = access.map(|s| !s.is_empty()).unwrap_or(false)
        || id_token.map(|s| !s.is_empty()).unwrap_or(false);
    let login = id_token.and_then(email_from_jwt).or_else(|| {
        if logged_in {
            let acct = tokens
                .and_then(|t| t.get("account_id"))
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            Some(format!(
                "chatgpt:{}",
                acct.chars().take(8).collect::<String>()
            ))
        } else {
            None
        }
    });
    ProviderIdentity {
        provider: ProviderKind::Codex,
        home_path: home.to_path_buf(),
        login,
        team: None,
        plan: id_token
            .and_then(jwt_claims)
            .and_then(|v| {
                v.get("https://api.openai.com/auth")?
                    .get("chatgpt_plan_type")?
                    .as_str()
                    .map(str::to_string)
            })
            .filter(|p| p != "unknown"),
        model,
        logged_in,
    }
}

pub fn inspect_claude_home(home: &Path) -> ProviderIdentity {
    let cred_path = if home.join(".credentials.json").is_file() {
        home.join(".credentials.json")
    } else {
        home.join("credentials.json")
    };
    let cred = read_json(&cred_path);
    let oauth = cred.as_ref().and_then(|c| c.get("claudeAiOauth"));
    let logged_in = oauth
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|t| !t.is_empty())
        .unwrap_or(false);
    let mut login = cred
        .as_ref()
        .and_then(|c| c.get("email"))
        .and_then(|e| e.as_str())
        .map(|s| s.to_string());
    let mut plan = oauth
        .and_then(|o| o.get("subscriptionType"))
        .and_then(|t| t.as_str())
        .filter(|t| *t != "unknown")
        .map(|s| s.to_string());
    let mut team = None;
    if let Some(raw) = read_json(&home.join(".claude.json")) {
        if let Some(oa) = raw.get("oauthAccount") {
            if let Some(e) = oa.get("emailAddress").and_then(|x| x.as_str()) {
                login = Some(e.to_string());
            }
            if plan.is_none() {
                plan = oa
                    .get("subscriptionType")
                    .and_then(Value::as_str)
                    .filter(|p| *p != "unknown")
                    .map(str::to_string);
            }
            if let Some(t) = oa.get("organizationName").and_then(|x| x.as_str()) {
                team = Some(t.to_string());
            }
        }
    }
    let mut model = None;
    if let Some(raw) = read_json(&home.join("settings.json")) {
        model = raw
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string());
    }
    ProviderIdentity {
        provider: ProviderKind::Claude,
        home_path: home.to_path_buf(),
        login,
        team,
        plan,
        model,
        logged_in,
    }
}

pub fn cursor_api_key_for_home(home: &Path) -> Option<String> {
    for name in ["concordia-api-key", "api-key"] {
        let p = home.join(name);
        if let Some(t) = crate::fs_safe::read_text_bounded(&p, 16 * 1024) {
            let t = t.trim();
            if !t.is_empty() {
                crate::redact::register_runtime_secret(t);
                return Some(t.to_string());
            }
        }
    }
    None
}

pub fn inspect_cursor_home(home: &Path) -> ProviderIdentity {
    let cfg = read_json(&home.join("cli-config.json"));
    let email = cfg
        .as_ref()
        .and_then(|c| c.get("authInfo"))
        .and_then(|a| a.get("email"))
        .and_then(|e| e.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let model = cfg
        .as_ref()
        .and_then(|c| {
            c.get("model")
                .and_then(|m| {
                    m.get("displayModelId")
                        .or_else(|| m.get("modelId"))
                        .or_else(|| m.get("displayName"))
                })
                .or_else(|| c.get("selectedModel").and_then(|m| m.get("modelId")))
        })
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let key = cursor_api_key_for_home(home);
    let logged_in = email.is_some() || key.is_some();
    ProviderIdentity {
        provider: ProviderKind::Cursor,
        home_path: home.to_path_buf(),
        login: email.or_else(|| key.map(|_| "api-key".into())),
        team: None,
        plan: None,
        model,
        logged_in,
    }
}

pub fn inspect_grok_home(home: &Path) -> ProviderIdentity {
    let model = crate::fs_safe::read_text_bounded(&home.join("config.toml"), 1024 * 1024)
        .and_then(|text| parse_toml_model(&text));
    let logged_in = home.join("config.toml").is_file() && home.join("auth.json").is_file();
    ProviderIdentity { provider: ProviderKind::Grok, home_path: home.to_path_buf(), login: logged_in.then(|| "cached-token".into()), team: None, plan: None, model, logged_in }
}

pub fn inspect_provider(provider: ProviderKind, home: &Path) -> ProviderIdentity {
    crate::test_guard::check_not_real_state(home);
    match provider {
        ProviderKind::Codex => inspect_codex_home(home),
        ProviderKind::Claude => inspect_claude_home(home),
        ProviderKind::Cursor => inspect_cursor_home(home),
        ProviderKind::Grok => inspect_grok_home(home),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_model() {
        assert_eq!(
            parse_toml_model("model = \"gpt-5.6-sol\"\n"),
            Some("gpt-5.6-sol".into())
        );
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn codex_profile_reads_only_public_email_and_plan_claims() {
        let root = tempfile::tempdir().unwrap();
        let token = "e30.eyJlbWFpbCI6ICJtZW1iZXJAZXhhbXBsZS5pbnZhbGlkIiwgImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6IHsiY2hhdGdwdF9wbGFuX3R5cGUiOiAicGx1cyJ9fQ.fixture";
        std::fs::write(
            root.path().join("auth.json"),
            serde_json::json!({"tokens":{"id_token":token}}).to_string(),
        )
        .unwrap();
        let profile = inspect_codex_home(root.path());
        assert_eq!(profile.login.as_deref(), Some("member@example.invalid"));
        assert_eq!(profile.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn claude_workspace_name_does_not_replace_the_subscription_plan() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"fixture","subscriptionType":"pro"}}"#,
        )
        .unwrap();
        std::fs::write(root.path().join(".claude.json"), r#"{"oauthAccount":{"emailAddress":"member@example.invalid","organizationName":"Fixture Team"}}"#).unwrap();
        let profile = inspect_claude_home(root.path());
        assert_eq!(profile.plan.as_deref(), Some("pro"));
        assert_eq!(profile.team.as_deref(), Some("Fixture Team"));
        assert_eq!(profile.login.as_deref(), Some("member@example.invalid"));
    }
}
