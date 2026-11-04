//! Bounded pasted-path grammar. Parsing never imports, expands environment variables or runs files.
use std::path::PathBuf;
pub fn parse(text: &str) -> Option<Vec<PathBuf>> {
    let text = text.trim();
    if text.is_empty()
        || text.len() > 16 * 1024
        || text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
    {
        return None;
    }
    let mut paths = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for character in text.chars() {
        if let Some(open) = quote {
            if character == open {
                quote = None;
            } else {
                word.push(character);
            }
        } else if matches!(character, '\'' | '"') && word.is_empty() {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !word.is_empty() {
                paths.push(std::mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if quote.is_some() {
        return None;
    }
    if !word.is_empty() {
        paths.push(word);
    }
    // An unquoted single path can contain spaces. Multiple paths must be quoted.
    if paths.len() > 1
        && !text.contains(['\r', '\n', '\'', '"'])
        && paths.iter().skip(1).all(|path| !absolute(path))
    {
        paths = vec![text.into()];
    }
    let paths = paths
        .into_iter()
        .map(|path| crate::file_links::normalized_local_path(&path).to_owned())
        .collect::<Vec<_>>();
    if paths.is_empty()
        || paths.len() > 4
        || paths.iter().any(|path| {
            !absolute(path)
                || path.starts_with(r"\\")
                || path.starts_with("//")
                || path.contains("://")
                || path.len() > 4096
                || crate::workspace_paths::sensitive(std::path::Path::new(path))
                || !PathBuf::from(path).extension().is_some_and(|extension| {
                    matches!(
                        extension.to_string_lossy().to_ascii_lowercase().as_str(),
                        "png"
                            | "jpg"
                            | "jpeg"
                            | "webp"
                            | "txt"
                            | "md"
                            | "markdown"
                            | "json"
                            | "pdf"
                    )
                })
        })
    {
        return None;
    }
    Some(paths.into_iter().map(PathBuf::from).collect())
}
fn absolute(path: &str) -> bool {
    PathBuf::from(path).is_absolute()
        || (path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && path.as_bytes().get(1) == Some(&b':')
            && path
                .as_bytes()
                .get(2)
                .is_some_and(|byte| matches!(byte, b'/' | b'\\')))
}
