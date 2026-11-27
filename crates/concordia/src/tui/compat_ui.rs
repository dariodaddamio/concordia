use super::App;
use crate::provider::compat::{self, Compat};
use crate::store::ProviderKind;
use std::sync::mpsc::Receiver;

pub(super) type CompatRx = Receiver<(ProviderKind, Compat)>;

impl App {
    pub(super) fn poll_compat(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.compat_rx {
            while let Ok((provider, found)) = rx.try_recv() {
                changed = true;
                compat::set_compat(provider, &found);
            }
        }
        changed
    }
}
