//! Provider capability detection results and degradation notices, polled on tick.

use super::App;
use crate::provider::compat::{self, Compat};
use crate::store::ProviderKind;
use std::sync::mpsc::Receiver;

pub(super) type CompatRx = Receiver<(ProviderKind, Compat)>;

impl App {
    /// Take finished detections (so planning can degrade) and show queued notices. Never blocks.
    pub(super) fn poll_compat(&mut self) {
        if let Some(rx) = &self.compat_rx {
            while let Ok((provider, found)) = rx.try_recv() {
                compat::set_compat(provider, &found);
            }
        }
        for notice in compat::take_notices() {
            self.push_system(notice);
        }
    }
}
