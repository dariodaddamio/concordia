use crate::{redact, store::ProviderKind};
use serde::Serialize;
use std::{
    fs::{create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Recorder(Arc<Mutex<std::fs::File>>, ProviderKind);

#[derive(Serialize)]
struct Entry<'a> {
    provider: &'a str,
    cli_version: &'a str,
    timestamp: u64,
    stream: &'a str,
    line: String,
}

impl Recorder {
    pub fn from_env(provider: ProviderKind) -> Option<Self> {
        let dir = std::env::var_os("CONCORDIA_RECORD_STREAMS")?;
        let dir = PathBuf::from(dir);
        create_dir_all(&dir).ok()?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        let path = dir.join(format!("{}-{stamp}.jsonl", provider.as_str()));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Self(Arc::new(Mutex::new(file)), provider))
    }
    pub fn line(&self, stream: &str, line: &str) {
        let version = super::compat::version_for(self.1);
        let entry = Entry {
            provider: self.1.as_str(),
            cli_version: &version,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            stream,
            line: redact::redact(line).into_owned(),
        };
        if let Ok(mut file) = self.0.lock() {
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{json}");
            }
        }
    }
}
