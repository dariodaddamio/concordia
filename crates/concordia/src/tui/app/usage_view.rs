use super::super::*;

impl App {
    pub(in crate::tui) fn refresh_usage(&mut self) {
        let home = self.home_path.clone().or_else(|| {
            get_use_selection(&self.state_root)
                .and_then(|s| s.home_path)
                .map(PathBuf::from)
        });
        let Some(home) = home else {
            self.usage_summary = "no account yet".into();
            self.usage_credits = "Credits: not reported".into();
            self.usage_reset = None;
            self.usage_tone = UsageTone::Mute;
            self.usage_warning = None;
            return;
        };
        let snap = self
            .usage_fetcher
            .snapshot(self.provider, &self.account, &home);
        self.set_usage_footer(&snap);
    }
    pub(in crate::tui) fn set_usage_footer(&mut self, snap: &crate::usage::UsageSnapshot) {
        self.usage_summary = snap.summary.clone();
        self.usage_credits = snap.credits_text();
        self.usage_reset = snap
            .windows
            .iter()
            .find(|window| window.label.starts_with("7d"))
            .or_else(|| snap.windows.first())
            .and_then(|window| window.resets.as_deref())
            .map(crate::usage::reset_phrase);
        self.usage_tone = usage_tone(snap);
        self.usage_warning = crate::usage::low_limit_warning(snap, self.provider);
    }
    pub(in crate::tui) fn show_usage(&mut self, force: bool) {
        let Some(home) = self.home_path.clone() else {
            self.push_system("usage - no account selected; try /use");
            return;
        };
        let snap = if force {
            self.usage_fetcher
                .snapshot_refresh(self.provider, &self.account, &home)
        } else {
            self.usage_fetcher
                .snapshot(self.provider, &self.account, &home)
        };
        self.set_usage_footer(&snap);
        self.push_command(if force { "/usage refresh" } else { "/usage" });
        self.messages.push(ChatLine {
            role: MsgRole::System,
            text: self.usage_block_text(&snap),
            card: None,
            meta: None,
        });
        self.usage_block = Some(self.messages.len() - 1);
    }
    pub(in crate::tui) fn usage_block_text(&self, snap: &crate::usage::UsageSnapshot) -> String {
        let mut details = format!(
            "{}\nCredit spend: not reported separately\n\nSubscription limits\n{}",
            snap.credits_text(),
            snap.detail.trim()
        );
        if self.provider == ProviderKind::Codex {
            details.push_str(&format!(
                "\nCredit balance details: {}",
                crate::usage::CREDIT_DASHBOARD_URL
            ));
        }
        let body = details.trim();
        format!("{} - limits\n{}", self.account_label(), body)
    }
    pub(in crate::tui) fn update_usage_block(&mut self) {
        let Some(idx) = self.usage_block else {
            return;
        };
        let Some(home) = self.home_path.clone() else {
            return;
        };
        if self
            .messages
            .get(idx)
            .is_none_or(|m| m.role != MsgRole::System)
        {
            self.usage_block = None;
            return;
        }
        let snap = self
            .usage_fetcher
            .snapshot(self.provider, &self.account, &home);
        self.messages[idx].text = self.usage_block_text(&snap);
    }
    pub(in crate::tui) fn account_label(&self) -> String {
        self.named_account(self.provider, &self.account)
    }
    pub(in crate::tui) fn named_account(&self, provider: ProviderKind, account: &str) -> String {
        let default = display_account_name(provider, account);
        self.account_names
            .get(account)
            .map(|name| format!("{name} ({default})"))
            .unwrap_or(default)
    }
    pub(in crate::tui) fn open_usage_picker(&mut self) {
        self.request_account_usage();
        self.build_usage_picker();
    }
    pub(in crate::tui) fn request_account_usage(&mut self) {
        for account in accounts_with_auth(&self.state_root) {
            if let Some(home) = account.home_path {
                self.usage_fetcher
                    .snapshot_refresh(account.provider, &account.account, &home);
            }
        }
    }
    pub(in crate::tui) fn update_usage_picker(&mut self) {
        let Some(old) = self.picker.take() else {
            return;
        };
        match old.kind {
            PickerKind::Use => self.build_use_picker(),
            PickerKind::Usage => self.build_usage_picker(),
            PickerKind::CreditDetails => self.show_credit_details(false),
            _ => {
                self.picker = Some(old);
                return;
            }
        }
        if let Some(picker) = &mut self.picker {
            picker.filter = old.filter;
            picker.state = old.state;
        }
    }
    pub(in crate::tui) fn show_credit_details(&mut self, force: bool) {
        let mut balance = "Not reported by provider".to_string();
        let mut availability = "Not reported".to_string();
        if let Some(home) = self.home_path.clone() {
            let snapshot = if force {
                self.usage_fetcher
                    .snapshot_refresh(self.provider, &self.account, &home)
            } else {
                self.usage_fetcher
                    .snapshot(self.provider, &self.account, &home)
            };
            balance = snapshot.credit_balance_text();
            availability = snapshot.credit_availability_text().into();
            self.set_usage_footer(&snapshot);
        }
        let mut rows = vec![
            PickerRow {
                id: "balance".into(),
                label: format!("Balance: {balance}"),
                detail: "Exact amount from the native provider; no estimate".into(),
            },
            PickerRow {
                id: "availability".into(),
                label: format!("Availability: {availability}"),
                detail: String::new(),
            },
            PickerRow {
                id: "source".into(),
                label: format!("Source: {} native quota response", self.provider.as_str()),
                detail: self.usage_summary.clone(),
            },
        ];
        if self.provider == ProviderKind::Codex {
            rows.push(PickerRow {
                id: "credit-dashboard".into(),
                label: "Open full credit balance and purchase details".into(),
                detail: format!(
                    "{} Ã‚· select the same account",
                    crate::usage::CREDIT_DASHBOARD_URL
                ),
            });
        }
        rows.push(PickerRow {
            id: "credit-refresh".into(),
            label: "Refresh native balance".into(),
            detail: "Read-only; no model turn".into(),
        });
        self.show_picker(
            PickerKind::CreditDetails,
            format!("Credit balance Ã‚· {}", self.account_label()),
            rows,
            false,
        );
    }
    pub(in crate::tui) fn build_usage_picker(&mut self) {
        let ready = accounts_with_auth(&self.state_root);
        let accounts: Vec<_> = ready
            .into_iter()
            .filter_map(|a| a.home_path.map(|h| (a.account, a.provider, h)))
            .collect();
        let rows_data = usage_rows_for_accounts(&mut self.usage_fetcher, &accounts, &self.account);
        let rows: Vec<PickerRow> = if rows_data.is_empty() {
            vec![PickerRow {
                id: "none".into(),
                label: "no accounts".into(),
                detail: String::new(),
            }]
        } else {
            rows_data
                .into_iter()
                .map(|r| {
                    self.picker_tones.insert(format!("usage:{}", r.id), r.tone);
                    PickerRow {
                        id: format!("usage:{}", r.id),
                        label: self
                            .account_names
                            .get(&r.id)
                            .map(|name| {
                                let default = get_account(&self.state_root, &r.id)
                                    .map(|a| display_account_name(a.provider, &r.id))
                                    .unwrap_or_else(|| r.id.clone());
                                r.label
                                    .replacen(&default, &format!("{name} ({default})"), 1)
                            })
                            .unwrap_or(r.label),
                        detail: r.detail,
                    }
                })
                .collect()
        };
        self.show_picker(PickerKind::Usage, "usage".into(), rows, false);
        self.footer_left = "Ã¢â€ â€˜Ã¢â€ â€œ Enter Ã‚· r refresh Ã‚· t tokens Ã‚· Esc".into();
    }
}
