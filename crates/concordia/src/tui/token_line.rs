//! The dim line under each reply: `in 12.3k - out 1.4k - cached 9.8k - 8.2s`.
//! Only fields the provider reported appear; the wall-clock turn time is always there.

use crate::provider::stream_cards::TurnUsage;
use std::time::Duration;

/// `ui.token_line` in config.toml; `CONCORDIA_TOKEN_LINE=0` turns it off regardless.
pub fn ui_token_line_enabled() -> bool {
    std::env::var("CONCORDIA_TOKEN_LINE").map_or(true, |v| v.trim() != "0")
        && crate::config::ui_token_line()
}

/// 999, 1.2k, 12k, 123k, 1.2M.
pub fn compact(n: u64) -> String {
    fn one_decimal(tenths: u64, suffix: &str) -> String {
        if tenths.is_multiple_of(10) {
            format!("{}{suffix}", tenths / 10)
        } else {
            format!("{}.{}{suffix}", tenths / 10, tenths % 10)
        }
    }
    if n < 1000 {
        return n.to_string();
    }
    if n < 100_000 {
        let tenths = (n + 50) / 100;
        if tenths < 1000 {
            return one_decimal(tenths, "k");
        }
    }
    if n < 999_500 {
        return format!("{}k", (n + 500) / 1000);
    }
    one_decimal((n + 50_000) / 100_000, "M")
}

fn elapsed_text(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        let s = d.as_secs();
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

fn cost_text(c: f64) -> String {
    if c > 0.0 && c < 0.01 {
        format!("${c:.4}")
    } else {
        format!("${c:.2}")
    }
}

pub fn format_line(usage: &TurnUsage, elapsed: Option<Duration>) -> String {
    let mut parts = Vec::new();
    if let Some(n) = usage.input {
        parts.push(format!("in {}", compact(n)));
    }
    if let Some(n) = usage.output {
        parts.push(format!("out {}", compact(n)));
    }
    if let Some(n) = usage.cached {
        parts.push(format!("cached {}", compact(n)));
    }
    if let Some(c) = usage.cost_usd {
        parts.push(cost_text(c));
    }
    if let Some(d) = elapsed {
        parts.push(elapsed_text(d));
    }
    parts.join(" - ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_compact() {
        for (n, want) in [
            (0, "0"),
            (999, "999"),
            (1000, "1k"),
            (1234, "1.2k"),
            (12_000, "12k"),
            (12_345, "12.3k"),
            (99_960, "100k"),
            (123_456, "123k"),
            (999_600, "1M"),
            (1_234_567, "1.2M"),
        ] {
            assert_eq!(compact(n), want, "{n}");
        }
    }

    #[test]
    fn only_reported_fields_appear() {
        let u = TurnUsage {
            input: Some(12_345),
            output: Some(1400),
            cached: Some(9800),
            cost_usd: None,
        };
        assert_eq!(
            format_line(&u, Some(Duration::from_millis(8200))),
            "in 12.3k - out 1.4k - cached 9.8k - 8.2s"
        );
        let only_out = TurnUsage {
            output: Some(5),
            ..Default::default()
        };
        assert_eq!(
            format_line(&only_out, Some(Duration::from_secs(75))),
            "out 5 - 1m 15s"
        );
        assert_eq!(
            format_line(&TurnUsage::default(), Some(Duration::from_secs(2))),
            "2.0s"
        );
    }
}
