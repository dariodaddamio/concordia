use super::super::*;

impl App {
    pub(in crate::tui) fn handle_slash(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed == "/terminal" || trimmed.starts_with("/terminal ") {
            local_terminal::command(self, trimmed.strip_prefix("/terminal").unwrap_or_default());
            return true;
        }
        if trimmed == "/rename" || trimmed.starts_with("/rename ") {
            let name = trimmed.strip_prefix("/rename").unwrap_or_default().trim();
            if !self.rename_session(name) {
                return true;
            }
            self.push_system(if self.session_ui.title.is_some() {
                "Chat renamed."
            } else {
                "Chat title reset to automatic."
            });
            return true;
        }
        if let Some(id) = trimmed
            .strip_prefix("/resume ")
            .or_else(|| trimmed.strip_prefix("/chats "))
            .map(str::trim)
            .filter(|id| !id.is_empty() && !id.chars().any(char::is_whitespace))
        {
            self.push_command(line);
            self.resume_explicit_session(id);
            return true;
        }
        if line.trim() == "/native" || line.trim().starts_with("/native ") {
            let parts: Vec<_> = line.split_whitespace().collect();
            let result = match parts.as_slice() {
                ["/native", account] | ["/native", account, "status"] => {
                    crate::native_input::command(&self.state_root, account, "status")
                }
                ["/native", account, mode @ ("on" | "off")] => {
                    let account = crate::accounts::normalize_account_id(account);
                    if self.busy && self.account == account
                        || self.deleg.d.running_for(&account).is_some()
                    {
                        Err("finish or cancel that account's active native turn before changing transport".into())
                    } else {
                        crate::native_input::command(&self.state_root, &account, mode)
                    }
                }
                _ => Err("/native ACCOUNT on|off|status".into()),
            };
            self.push_system(result.unwrap_or_else(|error| format!("native input: {error}")));
            return true;
        }
        if line.trim() == "/delegate input" {
            self.flash("usage: /delegate input ID");
            return true;
        }
        if let Some(id) = line.trim().strip_prefix("/delegate input ") {
            match id.parse::<u32>() {
                Ok(id) => worker_input::open(self, id),
                Err(_) => self.flash("usage: /delegate input ID"),
            }
            return true;
        }
        let trimmed = line.trim();
        if ["/delegate fallback", "/delegate recover", "/delegate retry"]
            .iter()
            .any(|p| trimmed == *p || trimmed.starts_with(&format!("{p} ")))
        {
            landing::close(self);
            self.push_command(line);
            self.worker_recovery_command(trimmed.strip_prefix("/delegate ").unwrap_or_default());
            return true;
        }
        if trimmed == "/threads" || trimmed.starts_with("/threads ") {
            self.push_command(line);
            self.push_system("/threads is now /resume");
            self.open_resume_picker();
            return true;
        }
        if trimmed == "/agents" {
            self.push_command(line);
            self.push_system("/agents is now Ctrl+G");
            self.open_agents_picker();
            return true;
        }
        if trimmed == "/config" || trimmed.starts_with("/config ") {
            let args = trimmed.strip_prefix("/config").unwrap_or("").trim();
            match args {
                "" => settings::open(self),
                "appearance" => appearance::open(self),
                "skills" => settings::catalog(self, false),
                "hooks" => settings::catalog(self, true),
                _ => appearance::command(self, args),
            }
            return true;
        }
        if trimmed == "/version" || trimmed == "/ver" || trimmed == "/about" {
            self.push_command(line);
            self.push_system(format!("{} is now /doctor", trimmed));
            for report_line in crate::doctor::run_fast_report().lines() {
                self.push_system(report_line.to_owned());
            }
            return true;
        }
        if trimmed == "/use rename" || trimmed.starts_with("/use rename ") {
            let args = trimmed
                .strip_prefix("/use rename")
                .unwrap_or_default()
                .trim();
            let (account, name) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            if account.is_empty() {
                self.push_system("usage: /use rename ACCOUNT NICKNAME (omit nickname to reset)");
            } else {
                match crate::accounts::set_nickname(&self.state_root, account, name) {
                    Ok(()) => {
                        self.account_names =
                            crate::store::load_store(&self.state_root).account_nicknames;
                        self.flash("account nickname saved");
                    }
                    Err(error) => self.push_system(format!("nickname: {error}")),
                }
            }
            return true;
        }
        if trimmed == "/usage all" || trimmed == "/usage refresh" {
            self.push_system(format!(
                "{} is now /usage (use r inside the screen to refresh)",
                trimmed
            ));
        }
        // /workers, /queue, /close and /stats remain available until their screens
        // replace them.
        if line.trim() == "/open" {
            self.open_related_files();
            return true;
        }
        if let Some(reference) = line.trim().strip_prefix("/open ") {
            self.file_command(reference, false, false);
            return true;
        }
        if line.trim() == "/close" || line.trim().starts_with("/close ") {
            let target = line
                .trim()
                .strip_prefix("/close")
                .unwrap_or_default()
                .trim();
            let id = if target.is_empty() {
                self.workspace.focused
            } else {
                target.parse::<u32>().ok()
            };
            match id {
                Some(id) if self.workspace.close(id) => {
                    if self.workspace.panes.iter().any(|p| p.id == id) {
                        self.flash("Unsaved file: S saves, D discards, Esc cancels");
                    } else {
                        self.flash(format!("closed file pane #{id}"));
                    }
                }
                _ => self.flash("usage: /close PANE_ID (or /close with a file pane focused)"),
            }
            return true;
        }
        if let Some(arg) = queue::slash_arg(line) {
            self.push_command(line);
            let notices = if arg == "resume" {
                queue_controls::resume(self)
            } else {
                self.queue_command(arg)
            };
            for l in notices {
                self.push_system(l);
            }
            return true;
        }
        let Some((cmd, _)) = parse_slash(line) else {
            return false;
        };
        if self.busy
            && matches!(
                &cmd,
                SlashCommand::New
                    | SlashCommand::Resume
                    | SlashCommand::Use
                    | SlashCommand::UseRemove(_)
                    | SlashCommand::UseAccount(_)
                    | SlashCommand::ModelSet(_)
                    | SlashCommand::Model
            )
        {
            self.push_system("finish or cancel the current turn before switching conversation, provider or model");
            return true;
        }
        if let SlashCommand::UseAccount(account) = &cmd {
            let normalized = crate::accounts::normalize_account_id(account);
            if !accounts_with_auth(&self.state_root)
                .iter()
                .any(|a| a.account == normalized)
            {
                self.push_system(
                    "account is not connected; /use lists accounts and /use add connects one",
                );
                return true;
            }
            self.show_picker(
                PickerKind::Use,
                "accounts".into(),
                vec![PickerRow {
                    id: format!("use:{normalized}"),
                    label: normalized,
                    detail: String::new(),
                }],
                false,
            );
            self.apply_picker_selection(None);
            return true;
        }
        if let SlashCommand::ModelSet(model) = &cmd {
            if !valid_model_id(model) {
                self.push_system("model id contains unsupported characters");
                return true;
            }
            self.show_picker(
                PickerKind::Model,
                "model".into(),
                vec![PickerRow {
                    id: format!("model:{model}"),
                    label: model.clone(),
                    detail: String::new(),
                }],
                false,
            );
            self.apply_picker_selection(None);
            return true;
        }
        if let SlashCommand::UseRemove(named) = &cmd {
            self.open_remove_picker(named.clone());
            return true;
        }
        if let SlashCommand::Mode(arg) = &cmd {
            self.push_command(line);
            self.run_mode_command(arg.as_deref());
            return true;
        }
        if let SlashCommand::Customization(hooks) = &cmd {
            settings::catalog(self, *hooks);
            return true;
        }
        if let SlashCommand::Harness(arg) = &cmd {
            self.push_command(line);
            integrations::command(self, arg);
            return true;
        }
        if matches!(&cmd, SlashCommand::Unknown(n) if n == "stats") {
            self.push_command(line);
            self.toggle_stats();
            return true;
        }
        let is_new = matches!(cmd, SlashCommand::New);
        if is_new && !self.save_turn_checkpoint(true) {
            self.keep.turn_recovery_notice = true;
            return true;
        }
        let ctx = CommandContext {
            state_root: &self.state_root,
            cwd: &self.cwd,
            provider: self.provider,
            account: &self.account,
            trust: self.trust,
        };
        let (outcome, lines) = dispatch_slash(cmd, &ctx);
        if is_new {
            self.native_owner = None;
            self.deleg.change_conversation();
            self.keep.turn_recovery_notice = false;
            self.continuation.invalidate();
            self.native_sources.clear();
            self.native_agents = native_agents::State::default();
            self.session_ui.invalidate();
            self.session_ui.title = None;
            self.messages.clear();
            self.related_files.reset();
            self.questions.clear();
            self.activity = activity::Activity::default();
            self.expanded_activity = false;
            self.view.follow_tail();
            self.clear_selections();
            self.native_session_id = None;
            self.task_id = None;
            self.session_chip = None;
            self.usage_block = None;
            self.session_input = 0;
            self.session_output = 0;
            self.session_cost = 0.0;
            self.need_context_pack = false;
            self.thread = crate::threads::start(&self.state_root);
            self.thread_meta_done = false;
            landing::open(self);
        }
        if !lines.is_empty() {
            self.push_command(line);
        }
        for l in lines {
            self.push_system(l);
        }
        match outcome {
            CommandOutcome::Exit => self.should_quit = true,
            CommandOutcome::OpenResume => self.open_resume_picker(),
            CommandOutcome::OpenUse => {
                for l in crate::integration::refresh_summary(&self.state_root) {
                    self.push_system(l);
                }
                self.open_use_picker();
            }
            CommandOutcome::OpenAddProvider => self.open_add_provider_picker(),
            CommandOutcome::OpenRemovePicker => self.open_remove_picker(None),
            CommandOutcome::AddAccount(p) => self.begin_add_account(p),
            CommandOutcome::OpenModel => self.open_model_picker(),
            CommandOutcome::OpenDoctor => self.open_doctor(),
            CommandOutcome::ShowUsage => self.show_usage(false),
            CommandOutcome::ShowUsageRefresh => self.show_usage(true),
            CommandOutcome::OpenUsage => self.open_usage_picker(),
            CommandOutcome::CopyLastReply => match self.last_assistant_text() {
                Some(t) => {
                    self.write_clipboard(&t);
                }
                None => self.flash("nothing to copy yet"),
            },
            CommandOutcome::Update => self.open_update_preview(),
            CommandOutcome::Ok => {}
        }
        true
    }
}
