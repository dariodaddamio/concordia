//! User-defined slash commands: `<state>/commands/<name>.md` becomes `/<name>`.
//!
//! A command is a prompt template, nothing more: optional `---` front matter with a
//! `description:`, then a body in which `$ARGS` is replaced by what the user typed after the
//! name. No shell, no includes, no environment expansion. Built-in commands always win a name
//! clash (reported by `concordia doctor`). Scanning is bounded: at most [`MAX_FILES`] files of at
//! most [`MAX_BYTES`] each, names `[a-z0-9-]{1,32}`; anything else is skipped with a warning.

use crate::commands::{parse_slash, SlashCommand};
use std::path::{Path, PathBuf};

pub const MAX_FILES: usize = 64;
pub const MAX_BYTES: u64 = 16 * 1024;
pub const MAX_NAME: usize = 32;
const MAX_EXPANDED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCommand {
    pub name: String,
    pub command: String,
    /// Front-matter description, or a generic one.
    pub description: String,
    pub template: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub commands: Vec<UserCommand>,
    pub warnings: Vec<String>,
    /// Names of files that were dropped because a built-in command has the same name.
    pub clashes: Vec<String>,
}

pub fn commands_dir(state_root: &Path) -> PathBuf {
    state_root.join("commands")
}

pub fn valid_name(name: &str) -> bool {
    (1..=MAX_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `/name` is handled by Concordia itself (including aliases like `/quit`).
pub fn is_builtin(name: &str) -> bool {
    !matches!(
        parse_slash(&format!("/{name}")),
        Some((SlashCommand::Unknown(_), _)) | None
    )
}

/// Split optional `---` front matter from the body; returns (description, body).
pub fn parse_file(text: &str) -> (Option<String>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split('\n');
    if lines.next().map(|l| l.trim_end()) != Some("---") {
        return (None, text.trim().to_string());
    }
    let rest: Vec<&str> = lines.collect();
    let Some(end) = rest.iter().position(|l| l.trim_end() == "---") else {
        // No closing fence: not front matter after all.
        return (None, text.trim().to_string());
    };
    let mut description = None;
    for l in &rest[..end] {
        if let Some((k, v)) = l.split_once(':') {
            if k.trim() == "description" {
                let v = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                if !v.is_empty() {
                    description = Some(v.chars().take(120).collect::<String>());
                }
            }
        }
    }
    let body = rest[end + 1..].join("\n").trim().to_string();
    (description, body)
}

/// Fill the template. Only `$ARGS` is replaced, in a single pass (typed text is never
/// re-expanded). A template without `$ARGS` keeps the typed text by appending it as a paragraph.
pub fn render(template: &str, args: &str) -> String {
    let args = args.trim();
    if template.contains("$ARGS") {
        template.replace("$ARGS", args)
    } else if args.is_empty() {
        template.to_string()
    } else {
        format!("{template}\n\n{args}")
    }
}

/// Read `<state>/commands/*.md`. Never fails: problems become warnings.
pub fn scan(state_root: &Path) -> Scan {
    let dir = commands_dir(state_root);
    let mut out = Scan::default();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return out;
    };
    let mut scanned = 0;
    let mut files: Vec<(String, PathBuf)> = rd
        .take(4096)
        .inspect(|_| scanned += 1)
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .filter(|(n, _)| n.to_ascii_lowercase().ends_with(".md"))
        .collect();
    if scanned == 4096 {
        out.warnings.push("commands: reached the 4096-entry discovery limit; additional entries were not inspected".into());
    }
    files.sort();
    if files.len() > MAX_FILES {
        out.warnings.push(format!(
            "commands: {} files found, only the first {MAX_FILES} are read",
            files.len()
        ));
        files.truncate(MAX_FILES);
    }
    for (file, path) in files {
        let name = file[..file.len() - 3].to_string();
        if !valid_name(&name) {
            out.warnings.push(format!(
                "commands/{file}: name must be [a-z0-9-], 1 to {MAX_NAME} characters; skipped"
            ));
            continue;
        }
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if len > MAX_BYTES {
            out.warnings.push(format!(
                "commands/{file}: {len} bytes is over the {MAX_BYTES} byte limit; skipped"
            ));
            continue;
        }
        let Some(text) = crate::fs_safe::read_text_bounded(&path, MAX_BYTES as usize) else {
            out.warnings.push(format!(
                "commands/{file}: not readable as UTF-8 text; skipped"
            ));
            continue;
        };
        if is_builtin(&name) {
            out.clashes.push(name);
            continue;
        }
        let (description, template) = parse_file(&text);
        if template.is_empty() {
            out.warnings
                .push(format!("commands/{file}: empty template; skipped"));
            continue;
        }
        let description = description.unwrap_or_else(|| "custom prompt".into());
        out.commands.push(UserCommand {
            command: format!("/{name}"),
            description,
            name,
            template,
        });
    }
    out
}

/// If `line` is `/name [args]` for a user command (and `name` is not a built-in), the prompt to
/// send. The caller shows the original line and submits this as an ordinary user turn.
pub fn expand(commands: &[UserCommand], line: &str) -> Result<Option<String>, String> {
    let Some(body) = line.trim().strip_prefix('/') else {
        return Ok(None);
    };
    let (name, args) = match body.find(char::is_whitespace) {
        Some(i) => (&body[..i], &body[i..]),
        None => (body, ""),
    };
    let name = name.to_ascii_lowercase();
    if is_builtin(&name) {
        return Ok(None);
    }
    let Some(command) = commands.iter().find(|c| c.name == name) else {
        return Ok(None);
    };
    let repeats = command.template.matches("$ARGS").count();
    let bytes = if repeats == 0 {
        command
            .template
            .len()
            .checked_add(args.trim().len())
            .and_then(|bytes| bytes.checked_add(2))
    } else {
        args.trim()
            .len()
            .checked_mul(repeats)
            .and_then(|bytes| bytes.checked_add(command.template.len() - repeats * 5))
    };
    if bytes.is_none_or(|bytes| bytes > MAX_EXPANDED_BYTES) {
        return Err("Command expansion exceeds 4 MiB; shorten its template or arguments. Original draft retained.".into());
    }
    Ok(Some(render(&command.template, args)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn state_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let cmds = commands_dir(dir.path());
        fs::create_dir_all(&cmds).unwrap();
        for (n, c) in files {
            fs::write(cmds.join(n), c).unwrap();
        }
        dir
    }

    #[test]
    fn front_matter_and_body_are_parsed() {
        let (d, b) = parse_file("---\ndescription: review the diff\n---\nLook at $ARGS\n");
        assert_eq!(d.as_deref(), Some("review the diff"));
        assert_eq!(b, "Look at $ARGS");
        let (d, b) = parse_file("---\r\ndescription: \"quoted\"\r\nother: x\r\n---\r\nbody\r\n");
        assert_eq!(d.as_deref(), Some("quoted"));
        assert_eq!(b, "body");
        let (d, b) = parse_file("just a body\n");
        assert!(d.is_none());
        assert_eq!(b, "just a body");
        let (d, b) = parse_file("---\nno closing fence\n");
        assert!(d.is_none());
        assert!(b.contains("no closing fence"));
    }

    #[test]
    fn render_replaces_only_args() {
        assert_eq!(render("Fix $ARGS now", " the bug "), "Fix the bug now");
        assert_eq!(render("$ARGS and $ARGS", "x"), "x and x");
        assert_eq!(render("Fix $ARGS now", ""), "Fix  now");
        // No other expansion, and typed text is not re-expanded.
        assert_eq!(
            render("$HOME %PATH% $(whoami) {{x}} $ARGS", "$ARGS"),
            "$HOME %PATH% $(whoami) {{x}} $ARGS"
        );
        // Without a placeholder the typed text is kept, not dropped.
        assert_eq!(render("Review it", ""), "Review it");
        assert_eq!(
            render("Review it", "focus on tests"),
            "Review it\n\nfocus on tests"
        );
    }

    #[test]
    fn scan_reads_valid_commands_and_skips_bad_ones() {
        let big = "x".repeat(MAX_BYTES as usize + 1);
        let dir = state_with(&[
            (
                "review.md",
                "---\ndescription: review the staged diff\n---\nReview. $ARGS",
            ),
            ("plain.md", "no front matter"),
            ("Bad_Name.md", "x"),
            ("toolong-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.md", "x"),
            ("huge.md", &big),
            ("empty.md", "---\ndescription: d\n---\n"),
            ("notes.txt", "ignored"),
        ]);
        let s = scan(dir.path());
        let names: Vec<_> = s.commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["plain", "review"]);
        assert_eq!(s.commands[1].description, "review the staged diff");
        assert_eq!(s.commands[0].description, "custom prompt");
        assert_eq!(s.commands[1].command, "/review");
        assert_eq!(s.warnings.len(), 4, "{:?}", s.warnings);
        assert!(s.clashes.is_empty());
    }

    #[test]
    fn at_most_64_files_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let cmds = commands_dir(dir.path());
        fs::create_dir_all(&cmds).unwrap();
        for i in 0..70 {
            fs::write(cmds.join(format!("c{i:02}.md")), "body").unwrap();
        }
        let s = scan(dir.path());
        assert_eq!(s.commands.len(), MAX_FILES);
        assert!(s.warnings[0].contains("70 files"));
    }

    #[test]
    fn builtin_names_win_and_are_reported() {
        let dir = state_with(&[
            ("doctor.md", "mine"),
            ("quit.md", "alias"),
            ("ship.md", "go"),
        ]);
        let s = scan(dir.path());
        assert_eq!(s.clashes, ["doctor", "quit"]);
        assert_eq!(s.commands.len(), 1);
        assert!(expand(&s.commands, "/doctor").unwrap().is_none());
        assert!(is_builtin("model") && !is_builtin("ship"));
    }

    #[test]
    fn expand_matches_name_case_insensitively_and_passes_args() {
        let dir = state_with(&[("ship.md", "Ship it: $ARGS")]);
        let s = scan(dir.path());
        assert_eq!(
            expand(&s.commands, "/SHIP the release\nplease")
                .unwrap()
                .as_deref(),
            Some("Ship it: the release\nplease")
        );
        assert_eq!(
            expand(&s.commands, "/ship").unwrap().as_deref(),
            Some("Ship it: ")
        );
        assert!(expand(&s.commands, "/nope").unwrap().is_none());
        assert!(expand(&s.commands, "ship").unwrap().is_none());
    }

    #[test]
    fn a_missing_folder_is_empty_and_quiet() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(scan(dir.path()), Scan::default());
    }
}
