use std::path::{Path, PathBuf};

const ALLOWED_TOPIC: &str = "lock";

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn comments(src: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let at = |i: usize, s: &str| {
        s.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let mut found = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let advance = |from: usize, to: usize, line: &mut usize| {
        *line += chars[from..to.min(n)]
            .iter()
            .filter(|&&c| c == '\n')
            .count();
    };
    while i < n {
        let c = chars[i];
        let prev = if i == 0 { ' ' } else { chars[i - 1] };
        if at(i, "//") {
            let end = (i..n).find(|&j| chars[j] == '\n').unwrap_or(n);
            found.push((line, chars[i..end].iter().collect()));
            i = end;
        } else if at(i, "/*") {
            let (start, start_line) = (i, line);
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if at(i, "/*") {
                    depth += 1;
                    i += 2;
                } else if at(i, "*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            advance(start, i, &mut line);
            found.push((start_line, chars[start..i].iter().collect()));
        } else if (c == 'r' || (c == 'b' && at(i + 1, "r"))) && !is_ident(prev) {
            let mut k = i + if c == 'b' { 2 } else { 1 };
            let hashes = (k..n).take_while(|&j| chars[j] == '#').count();
            k += hashes;
            if chars.get(k) == Some(&'"') {
                let close: String = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                let mut j = k + 1;
                while !at(j, &close) {
                    j += 1;
                }
                advance(i, j, &mut line);
                i = j + close.chars().count();
            } else {
                i += 1;
            }
        } else if c == '"' {
            let mut j = i + 1;
            while chars[j] != '"' {
                j += if chars[j] == '\\' { 2 } else { 1 };
            }
            advance(i, j, &mut line);
            i = j + 1;
        } else if c == '\'' {
            if chars.get(i + 1) == Some(&'\\') {
                i += 2;
                while chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if is_ident(c) {
            while i < n && is_ident(chars[i]) {
                i += 1;
            }
        } else {
            if c == '\n' {
                line += 1;
            }
            i += 1;
        }
    }
    found
}

fn mentions_allowed_topic(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.match_indices(ALLOWED_TOPIC).any(|(at, _)| {
        lower[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
    })
}

#[test]
fn rust_sources_carry_no_comments_except_about_locks() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_files(crates, &mut files);
    let mut violations = Vec::new();
    for file in files {
        let src = std::fs::read_to_string(&file).unwrap();
        let found = comments(&src);
        let mut block: Vec<&(usize, String)> = Vec::new();
        let mut flush = |block: &mut Vec<&(usize, String)>| {
            if !block.is_empty() && !block.iter().any(|(_, text)| mentions_allowed_topic(text)) {
                for (line, text) in block.iter() {
                    violations.push(format!("{}:{line}: {}", file.display(), text.trim()));
                }
            }
            block.clear();
        };
        for comment in &found {
            if block.last().is_some_and(|(line, _)| line + 1 != comment.0) {
                flush(&mut block);
            }
            block.push(comment);
        }
        flush(&mut block);
    }
    assert!(
        violations.is_empty(),
        "Comments are only allowed when they explain a lock. Make the code say it instead \
         (names, types, a test or a check that fails):\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_scanner_sees_comments_but_not_strings() {
    let src = "let a = \"// no\";\nlet b = r#\"/* no */\"#;\n// yes\nlet c = '\"'; /* yes */\n";
    let found: Vec<usize> = comments(src).into_iter().map(|(line, _)| line).collect();
    assert_eq!(found, vec![3, 4]);
    assert!(mentions_allowed_topic("// hold the lock"));
    assert!(!mentions_allowed_topic("// the block clock"));
}
