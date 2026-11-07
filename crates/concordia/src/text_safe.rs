//! Small text transformations shared by providers, storage, and the UI.

/// Remove control and bidirectional/invisible characters that can spoof terminal output.
pub(crate) fn terminal_safe(text: &str) -> String {
    text.chars()
        .filter(|c| (!c.is_control() || matches!(*c, '\n' | '\t')) && !is_terminal_unsafe(*c))
        .collect()
}

fn is_terminal_unsafe(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{061c}' | '\u{feff}')
}

#[cfg(test)]
mod tests {
    use super::terminal_safe;

    #[test]
    fn terminal_safe_strips_bidi_and_zero_width() {
        assert_eq!(
            terminal_safe("invoice\u{202e}fdp.exe\u{200b}"),
            "invoicefdp.exe"
        );
    }
}
