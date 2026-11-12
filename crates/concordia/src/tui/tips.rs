//! One short tip per launch, shown dimmed while the transcript is only the welcome line and while
//! the prompt has sat idle and empty for a while. A static list: no I/O, no threads.

pub const TIPS: &[&str] = &[
    "Shift+Tab cycles plan, ask and workspace; use /mode full with confirmation.",
    "/usage all shows the limits of every account at a glance.",
    "/use switches account or provider; the conversation carries over.",
    "/delegate hands a task to a background worker; the main chat stays free.",
    "Esc interrupts the running turn without leaving Concordia.",
    "Drag with the mouse to select text; Ctrl+C copies it (or the last reply).",
    "/resume picks up an earlier conversation in this folder.",
    "/doctor checks your setup: CLIs, logins and homes.",
    "Start a line with / to see the slash commands and their descriptions.",
];

/// Idle ticks (200 ms each) of an empty prompt before a tip appears mid-session: 20 s.
pub const IDLE_TICKS: u64 = 100;

/// Index of this launch's tip: a pure function of the seed.
pub fn pick(seed: u64) -> usize {
    // splitmix-style scramble so consecutive seeds (pids, nearby clock values) differ.
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) % TIPS.len() as u64) as usize
}

pub fn tip_for(seed: u64) -> &'static str {
    TIPS[pick(seed)]
}

#[derive(Debug, Default)]
pub struct RateLimiter {
    shown: std::collections::HashMap<&'static str, usize>,
    max: usize,
}

impl RateLimiter {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            ..Self::default()
        }
    }
    pub fn take(&mut self, tip: &'static str) -> Option<&'static str> {
        let count = self.shown.entry(tip).or_default();
        if *count >= self.max {
            None
        } else {
            *count += 1;
            Some(tip)
        }
    }
}

pub fn contextual_tip(
    queue_non_empty: bool,
    busy: bool,
    limit_near: bool,
    seed: u64,
) -> &'static str {
    let registry_tip = |name: &str| {
        crate::commands::command_registry()
            .iter()
            .find(|command| command.name == name)
            .and_then(|command| command.contextual_tip)
            .unwrap_or("")
    };
    if queue_non_empty {
        return registry_tip("/workers");
    }
    if limit_near {
        return registry_tip("/use");
    }
    if busy {
        return registry_tip("/resume");
    }
    tip_for(seed)
}

/// Launch seed from the clock and pid. Read once at startup.
pub fn launch_seed() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    t ^ (u64::from(std::process::id()) << 20)
}

/// `ui.tips` in config.toml; `CONCORDIA_TIPS=0` turns it off regardless.
pub fn enabled(preference: bool) -> bool {
    preference && std::env::var("CONCORDIA_TIPS").map_or(true, |v| v.trim() != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tips_are_short_and_deterministic_by_seed() {
        assert!(TIPS.len() >= 4 && TIPS.len() <= 14);
        for t in TIPS {
            assert!(t.chars().count() <= 90, "{t}");
            assert!(!t.is_empty(), "{t}");
        }
        assert_eq!(pick(7), pick(7));
        assert_eq!(tip_for(123), tip_for(123));
        let distinct: std::collections::HashSet<_> = (0..200u64).map(pick).collect();
        assert!(distinct.len() > 5);
    }

    #[test]
    fn tips_are_contextual_and_rate_limited() {
        assert!(contextual_tip(true, true, true, 1).starts_with("Tab"));
        assert!(contextual_tip(false, false, true, 1).contains("/use"));
        let mut limiter = RateLimiter::new(2);
        assert!(limiter.take("x").is_some());
        assert!(limiter.take("x").is_some());
        assert!(limiter.take("x").is_none());
    }

    #[test]
    fn tips_respect_setting() {
        assert!(enabled(true));
        assert!(!enabled(false));
    }
}
