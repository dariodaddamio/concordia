use super::super::*;

impl App {
    pub(in crate::tui) fn poll_pending_login(&mut self) {
        let Some(p) = self.pending_login.as_mut() else {
            return;
        };
        p.ticks += 1;
        if p.ticks % LOGIN_POLL_TICKS != 0 {
            return;
        }
        if is_logged_in(p.provider, &p.home) {
            let p = self.pending_login.take().expect("checked above");
            self.finish_login(p);
        } else if p.ticks >= LOGIN_GIVE_UP_TICKS {
            let p = self.pending_login.take().expect("checked above");
            self.push_system(format!(
                "{} is not signed in yet - run /use add {} to try again",
                p.label,
                p.provider.as_str()
            ));
        }
    }
    pub(in crate::tui) fn finish_login(&mut self, p: PendingLogin) {
        crate::logging::info(&format!(
            "login-complete account={} provider={}",
            p.account,
            p.provider.as_str()
        ));
        let who = inspect_provider(p.provider, &p.home)
            .login
            .unwrap_or_else(|| "this login".into());
        let Some(existing) = crate::accounts::duplicate_of(&self.state_root, &p.account) else {
            self.push_system(format!(
                "{} connected as {who} - /use to switch to it",
                p.label
            ));
            self.flash(format!("{} connected", p.label));
            return;
        };
        let owned = crate::connect::removable(&self.state_root)
            .iter()
            .any(|r| r.id == p.account && r.owned);
        let advice = format!(
            "to add another account, sign in with a different one (/use add {})",
            p.provider.as_str()
        );
        if owned {
            match crate::connect::remove_account(
                &self.state_root,
                &p.account,
                crate::connect::RemoveMode::DeleteData,
            ) {
                Ok(_) => self.push_system(format!(
                    "{who} is already connected as {}, so nothing new was added and the duplicate slot was removed - {advice}",
                    existing.label
                )),
                Err(e) => self.push_system(format!(
                    "{} is the same login as {} ({who}) but could not be removed: {e} - use /use remove",
                    p.label, existing.label
                )),
            }
        } else {
            self.push_system(format!(
                "{} is the same login as {} ({who}) - use /use remove to drop one of them",
                p.label, existing.label
            ));
        }
        self.flash("duplicate account");
    }
}
