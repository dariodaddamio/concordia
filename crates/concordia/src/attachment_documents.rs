//! Explicit UTF-8 document snapshots. Binary formats are not converted or hidden in prompts.
use super::*;
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub path: PathBuf,
    pub name: String,
    pub mime: String,
    pub bytes: usize,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<std::ops::Range<usize>>,
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    if crate::workspace_paths::sensitive(path) {
        return Err("Document is excluded by workspace policy".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "Document file unavailable")?;
    if crate::workspace_paths::sensitive(&canonical) {
        return Err("Document is excluded by workspace policy".into());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "Document metadata unavailable")?;
    if !metadata.is_file()
        || crate::workspaces::linked(&metadata)
        || metadata.len() > MAX_DOCUMENT_BYTES as u64
    {
        return Err("Attach a regular UTF-8 document of at most 64 KiB".into());
    }
    let mut bytes = Vec::new();
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "Document could not be opened")?;
    let opened = file
        .metadata()
        .map_err(|_| "Document metadata unavailable")?;
    if !opened.is_file()
        || crate::workspaces::linked(&opened)
        || path.canonicalize().ok().as_ref() != Some(&canonical)
    {
        return Err("Document identity changed before reading; choose it again".into());
    }
    file.take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Document could not be read")?;
    if bytes.len() > MAX_DOCUMENT_BYTES
        || bytes.contains(&0)
        || std::str::from_utf8(&bytes).is_err()
    {
        return Err(
            "Attach UTF-8 text/Markdown/JSON of at most 64 KiB; no binary conversion is available"
                .into(),
        );
    }
    Ok(bytes)
}
impl Document {
    pub fn validate(&self) -> Result<(), String> {
        if self.bytes > MAX_DOCUMENT_BYTES
            || self.name.len() > 512
            || self.name.chars().any(char::is_control)
            || self.path.to_string_lossy().len() > 4096
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !matches!(
                self.mime.as_str(),
                "text/plain" | "text/markdown" | "application/json"
            )
            || self
                .range
                .as_ref()
                .is_some_and(|range| range.start > range.end || range.end > self.bytes)
        {
            return Err("Invalid document descriptor; original retained".into());
        }
        Ok(())
    }
    pub fn import(root: &Path, path: &Path) -> Result<Self, String> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime=match extension.as_str() {"txt"=>"text/plain","md"|"markdown"=>"text/markdown","json"=>"application/json",_=>return Err("Document attachments currently support .txt, .md and .json; binary transport is unavailable".into())};
        let bytes = read(path)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let directory = root.join("attachments");
        std::fs::create_dir_all(&directory).map_err(|_| "Attachment storage unavailable")?;
        crate::workspaces::directory_id(&directory)?;
        let target = directory.join(format!("{hash}.txt"));
        if !target.exists() {
            crate::state_io::write_atomic(&target, &bytes)
                .map_err(|_| "Document snapshot could not be saved")?;
        }
        let document = Self {
            path: target
                .canonicalize()
                .map_err(|_| "Document snapshot unavailable")?,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .filter(|character| !character.is_control())
                .take(120)
                .collect(),
            mime: mime.into(),
            bytes: bytes.len(),
            sha256: hash,
            range: None,
        };
        document.text()?;
        Ok(document)
    }
    pub fn text(&self) -> Result<String, String> {
        self.validate()?;
        let bytes = read(&self.path)?;
        if bytes.len() != self.bytes || format!("{:x}", Sha256::digest(&bytes)) != self.sha256 {
            return Err("Document snapshot changed; attach it again before sending".into());
        }
        let text = String::from_utf8(bytes).map_err(|_| "Document is no longer UTF-8")?;
        if let Some(range) = &self.range {
            return text
                .get(range.clone())
                .map(str::to_owned)
                .ok_or_else(|| "Document selection is invalid; original retained".into());
        }
        Ok(text)
    }
}
