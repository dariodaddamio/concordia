//! Native image inputs and a backwards-compatible envelope for durable draft/queue strings.
//! Bytes live in app-owned, content-addressed storage, never in prompts or history labels.
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const PREFIX: &str = "\u{1e}concordia-message-v1\n";
pub const MAX_IMAGES: usize = 4;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Image {
    pub path: PathBuf,
    pub name: String,
    pub mime: String,
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Message {
    pub text: String,
    #[serde(default)]
    pub images: Vec<Image>,
}
impl Message {
    pub fn text(raw: &str) -> &str {
        raw.strip_prefix(PREFIX)
            .and_then(|s| s.split_once('\n'))
            .map_or(raw, |(_, text)| text)
    }
    pub fn decode(raw: &str) -> Self {
        raw.strip_prefix(PREFIX)
            .and_then(|s| s.split_once('\n'))
            .filter(|(header, _)| header.len() <= 16 * 1024)
            .and_then(|(images, text)| {
                serde_json::from_str::<Vec<Image>>(images)
                    .ok()
                    .map(|images| Self {
                        text: text.into(),
                        images,
                    })
            })
            .filter(|m| m.images.len() <= MAX_IMAGES)
            .unwrap_or_else(|| Self {
                text: raw.into(),
                images: vec![],
            })
    }
    pub fn encode(&self) -> String {
        if self.images.is_empty() {
            self.text.clone()
        } else {
            format!(
                "{PREFIX}{}\n{}",
                serde_json::to_string(&self.images).unwrap(),
                self.text
            )
        }
    }
    pub fn display(&self) -> String {
        let mut text = self.text.clone();
        for image in &self.images {
            let name: String = image
                .name
                .chars()
                .filter(|c| !"[]<>".contains(*c))
                .collect();
            let path = image.path.to_string_lossy();
            let path = path
                .strip_prefix(r"\\?\")
                .unwrap_or(&path)
                .replace('\\', "/");
            text.push_str(&format!(
                "\n[Image: {name} · {} KB](<{path}>)",
                image.bytes.div_ceil(1024)
            ));
        }
        text
    }
}
fn mime(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "image file unavailable")?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|_| "could not read image")?;
    if data.len() > MAX_BYTES {
        return Err("image exceeds 4 MiB; resize it before attaching".into());
    }
    mime(&data).ok_or("attach a PNG, JPEG or WebP image")?;
    Ok(data)
}
pub fn import(root: &Path, path: &Path) -> Result<Image, String> {
    let data = read(path)?;
    let (mime, ext) = mime(&data).unwrap();
    let hash = format!("{:x}", Sha256::digest(&data));
    let dir = root.join("attachments");
    std::fs::create_dir_all(&dir).map_err(|_| "image storage unavailable")?;
    let target = dir.join(format!("{hash}.{ext}"));
    // Same bytes can share one immutable snapshot. Existing bytes are verified at dispatch.
    if !target.exists() {
        crate::state_io::write_atomic(&target, &data).map_err(|_| "could not store image")?;
    }
    Ok(Image {
        path: target
            .canonicalize()
            .map_err(|_| "stored image unavailable")?,
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect(),
        mime: mime.into(),
        bytes: data.len(),
        sha256: hash,
    })
}
impl Image {
    pub fn data(&self) -> Result<String, String> {
        let data = read(&self.path)?;
        if format!("{:x}", Sha256::digest(&data)) != self.sha256
            || mime(&data).map(|m| m.0) != Some(self.mime.as_str())
        {
            return Err("attached image changed; attach it again".into());
        }
        Ok(base64::engine::general_purpose::STANDARD.encode(data))
    }
    pub fn claude(&self) -> Result<serde_json::Value, String> {
        Ok(
            serde_json::json!({"type":"image","source":{"type":"base64","media_type":self.mime,"data":self.data()?}}),
        )
    }
    pub fn acp(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"type":"image","mimeType":self.mime,"data":self.data()?}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots_survive_original_changes_and_envelopes_keep_bytes_out_of_text() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("image.png");
        std::fs::write(&original, b"\x89PNG\r\n\x1a\nsynthetic image").unwrap();
        let image = import(&dir.path().join("state"), &original).unwrap();
        std::fs::write(&original, "changed").unwrap();
        assert!(image.data().is_ok());
        let m = Message {
            text: "Inspect this".into(),
            images: vec![image],
        };
        let encoded = m.encode();
        assert!(!encoded.contains("synthetic image"));
        let decoded = Message::decode(&encoded);
        assert_eq!(decoded.images, m.images);
        assert!(decoded.display().contains("Image: image.png"));
        std::fs::write(&decoded.images[0].path, "replaced").unwrap();
        assert!(decoded.images[0].data().is_err());
    }
}
