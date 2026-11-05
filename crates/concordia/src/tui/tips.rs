//! One short tip per launch, shown dimmed while the transcript is only the welcome line and while
//! the prompt has sat idle and empty for a while. A static list: no I/O, no threads.

/// Every tip names a feature that exists today.
pub const TIPS: &[&str] = &[
    "Shift+Tab cycles the mode: plan, ask, auto, full.",
    "/usage all shows the limits of every account at a glance.",
    "/use switches account or provider; the conversation carries over.",
    "/delegate hands a task to a background worker; the main chat stays free.",
    "Esc interrupts the running turn without leaving Concordia.",
    "Drag with the mouse to select text; Ctrl+C copies it (or the last reply).",
    "/resume picks up an earlier conversation in this folder.",
    "/split opens a second pane so two agents can work side by side.",
    "/mode plan | ask | auto | full sets how much the agent may do on its own.",
    "/doctor checks your setup: CLIs, logins and homes.",
    "Start a line with / to see the slash commands and their descriptions.",
    "/new starts a fresh conversation in the same folder.",
    "/version shows which build you are running.",
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
        assert!(TIPS.len() >= 10 && TIPS.len() <= 14);
        for t in TIPS {
            assert!(t.chars().count() <= 90, "{t}");
            assert!(!t.contains('@'), "{t}");
        }
        assert_eq!(pick(7), pick(7));
        assert_eq!(tip_for(123), tip_for(123));
        let distinct: std::collections::HashSet<_> = (0..200u64).map(pick).collect();
        assert!(distinct.len() > 5);
    }
}
