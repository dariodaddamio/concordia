//! Slash-command suggestions shown while typing `/...` in the prompt.
//!
//! Pure logic (no terminal I/O): which commands match the text so far, and what selecting one
//! should put in the prompt. Drawing and key handling live in `tui/mod.rs`.

use crate::commands::SLASH_COMMANDS;
use crate::user_commands::UserCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub command: &'static str,
    pub help: &'static str,
    /// A user command from `<state>/commands` (drawn with a dim "custom" marker).
    pub custom: bool,
}

/// Commands whose full text starts with what the user has typed. Only a single-line prompt that
/// begins with `/` gets suggestions; everything else is ordinary chat text.
#[cfg(test)]
pub fn matching(input: &str) -> Vec<Entry> {
    matching_with(input, &[])
}

/// Like [`matching`], with the user's commands listed after the built-ins.
pub fn matching_with(input: &str, user: &[UserCommand]) -> Vec<Entry> {
    if !input.starts_with('/') || input.contains('\n') {
        return Vec::new();
    }
    let typed = input.to_ascii_lowercase();
    let builtin = SLASH_COMMANDS
        .iter()
        .filter(|(cmd, _)| cmd.starts_with(&typed))
        .map(|&(command, help)| Entry {
            command,
            help,
            custom: false,
        });
    let custom = user
        .iter()
        .filter(|c| c.command.starts_with(&typed))
        .map(|c| Entry {
            command: c.command,
            help: c.description,
            custom: true,
        });
    let mut entries: Vec<_> = builtin.chain(custom).collect();
    if let Some(index) = entries.iter().position(|entry| entry.command == input) {
        let exact = entries.remove(index);
        entries.insert(0, exact);
    }
    entries
}

#[derive(Debug, Default)]
pub struct Palette {
    pub selected: usize,
    /// Closed with Esc; stays closed until the text changes.
    pub dismissed: bool,
}

impl Palette {
    pub fn reset(&mut self) {
        self.selected = 0;
        self.dismissed = false;
    }

    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub fn up(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + len - 1) % len;
        }
    }

    pub fn down(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(input: &str) -> Vec<&'static str> {
        matching(input).into_iter().map(|e| e.command).collect()
    }

    #[test]
    fn a_bare_slash_lists_every_command() {
        let all = cmds("/");
        assert_eq!(all.len(), SLASH_COMMANDS.len());
        assert_eq!(
            all,
            SLASH_COMMANDS
                .iter()
                .map(|(command, _)| *command)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn typing_narrows_by_prefix_case_insensitively() {
        assert_eq!(cmds("/up"), ["/update"]);
        assert_eq!(cmds("/UP"), ["/update"]);
        assert_eq!(cmds("/use a"), Vec::<&str>::new());
        assert_eq!(cmds("/zzz"), Vec::<&str>::new());
    }

    #[test]
    fn only_a_single_line_slash_prompt_gets_suggestions() {
        assert!(cmds("hello /use").is_empty());
        assert!(cmds("").is_empty());
        assert!(cmds("/use\nsecond line").is_empty());
    }

    #[test]
    fn every_command_has_a_description_and_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for (cmd, help) in SLASH_COMMANDS {
            assert!(cmd.starts_with('/'), "{cmd}");
            assert!(!help.trim().is_empty(), "{cmd} needs a description");
            assert!(seen.insert(*cmd), "duplicate {cmd}");
        }
    }

    #[test]
    fn user_commands_are_listed_after_built_ins_and_marked_custom() {
        let dir = tempfile::tempdir().unwrap();
        let cmds = crate::user_commands::commands_dir(dir.path());
        std::fs::create_dir_all(&cmds).unwrap();
        std::fs::write(
            cmds.join("ship.md"),
            "---
description: ship it
---
go",
        )
        .unwrap();
        let user = crate::user_commands::scan(dir.path()).commands;
        let all = matching_with("/", &user);
        assert_eq!(all.len(), SLASH_COMMANDS.len() + 1);
        let last = all.last().unwrap();
        assert_eq!(
            (last.command, last.help, last.custom),
            ("/ship", "ship it", true)
        );
        assert!(all[..all.len() - 1].iter().all(|e| !e.custom));
        assert_eq!(matching_with("/sh", &user).len(), 1);
        assert!(matching_with("/sh", &[]).is_empty());
    }

    #[test]
    fn selection_wraps_and_clamps() {
        let mut p = Palette::default();
        p.up(3);
        assert_eq!(p.selected, 2);
        p.down(3);
        assert_eq!(p.selected, 0);
        p.selected = 9;
        p.clamp(4);
        assert_eq!(p.selected, 3);
        p.clamp(0);
        assert_eq!(p.selected, 0);
        p.dismissed = true;
        p.reset();
        assert!(!p.dismissed);
    }
}
