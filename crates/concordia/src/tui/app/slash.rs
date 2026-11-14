use super::super::*;

impl App {
    fn run_mcp_command(&mut self, args: &str) {
        let words: Vec<_> = args.split_whitespace().collect();
        if words.is_empty() {
            for line in crate::mcp_registry::provider_status_lines(&self.state_root, &self.cwd) {
                self.push_system(line);
            }
            if crate::mcp_registry::load(&self.state_root).map_or(true, |r| r.servers.is_empty()) {
                self.push_system("/mcp add NAME stdio COMMAND [ARGS...] · secrets must be env:NAME or credential:TARGET");
            }
            return;
        }
        let action = words[0].to_ascii_lowercase();
        if action == "import" {
            self.begin_mcp_import();
            return;
        }
        let Some(name) = words.get(1).map(|s| (*s).to_owned()) else {
            self.push_system("/mcp add NAME stdio COMMAND [ARGS...] | /mcp remove NAME | /mcp on NAME | /mcp off NAME | /mcp import");
            return;
        };
        let mut registry = match crate::mcp_registry::load(&self.state_root) {
            Ok(registry) => registry,
            Err(error) => {
                self.push_system(error);
                return;
            }
        };
        match action.as_str() {
            "on" | "off" => match registry.servers.get_mut(&name) {
                Some(server) => {
                    server.enabled = action == "on";
                }
                None => {
                    self.push_system(format!("MCP server not found: {name}"));
                    return;
                }
            },
            "remove" => {
                registry.servers.remove(&name);
            }
            "add" => {
                if words.get(2).map(|s| *s == "stdio").unwrap_or(false) && words.len() >= 4 {
                    registry.servers.insert(
                        name.into(),
                        crate::mcp_registry::Server {
                            enabled: true,
                            transport: crate::mcp_registry::Transport::Stdio {
                                command: words[3].into(),
                                args: words[4..].iter().map(|s| (*s).into()).collect(),
                                env: Default::default(),
                            },
                        },
                    );
                } else {
                    self.push_system("/mcp add NAME stdio COMMAND [ARGS...] · use the guided panel for remote servers and secret references");
                    return;
                }
            }
            _ => { self.push_system("/mcp add|remove|on|off|import"); return; }
        }
        match crate::mcp_registry::save(&self.state_root, &registry) {
            Ok(()) => self.push_system("MCP registry saved. Provider homes were not changed."),
            Err(error) => self.push_system(error),
        }
    }

    pub(in crate::tui) fn handle_slash(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed == "/terminal" || trimmed.starts_with("/terminal ") {
            let args = trimmed.strip_prefix("/terminal").unwrap_or_default().trim();
            if args.is_empty() {
                self.open_guided_command("/terminal", vec![
                    PickerRow { id: "terminal:open".into(), label: "Open local PowerShell".into(), detail: "validated for this project; starts a terminal pane; undo: close the pane".into() },
                    PickerRow { id: "terminal:native".into(), label: "Open the provider's native interface".into(), detail: "validated for the selected account; opens a separate provider session; undo: close that session".into() },
                    PickerRow { id: "terminal:hide".into(), label: "Hide the running terminal".into(), detail: "validated when a terminal is running; keeps its process alive; undo: choose Open again".into() },
                    PickerRow { id: "terminal:close".into(), label: "Close the terminal".into(), detail: "validated when a terminal exists; stops its process; undo: open a new terminal".into() },
                ]);
            } else {
                local_terminal::command(self, args);
            }
            return true;
        }
        if trimmed == "/rename" || trimmed.starts_with("/rename ") {
            let name = trimmed.strip_prefix("/rename").unwrap_or_default().trim();
            if name.is_empty() {
                self.open_guided_command("/rename", vec![
                    PickerRow { id: "rename:set".into(), label: "Set a title".into(), detail: "validated in the title editor; changes this chat title; undo: run /rename again with the previous title".into() },
                    PickerRow { id: "rename:clear".into(), label: "Restore automatic title".into(), detail: "validated for the current chat; clears its custom title; undo: set the previous title again".into() },
                ]);
            } else if !self.rename_session(name) {
                return true;
            } else { self.push_system(if self.session_ui.title.is_some() {
                "Chat renamed."
            } else {
                "Chat title reset to automatic."
            }); }
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
            if line.trim() == "/native" {
                let mut rows = Vec::new();
                for account in accounts_with_auth(&self.state_root) {
                    for (action, label, effect) in [("status", "Inspect native transport", "reads the saved transport state; undo: not needed"), ("on", "Enable native transport", "uses the provider native interface; undo: choose Disable"), ("off", "Disable native transport", "uses print mode; undo: choose Enable")] {
                        rows.push(PickerRow {
                            id: format!("native:{}:{}", account.account, action),
                            label: format!("{} · {}", account.label, label),
                            detail: format!("validated for account {}; {effect}", account.account),
                        });
                    }
                }
                if rows.is_empty() {
                    rows.push(PickerRow { id: "native:none".into(), label: "No connected accounts".into(), detail: "connect an account with /use add; undo: Esc".into() });
                }
                self.open_guided_command("/native", rows);
                return true;
            }
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
        if line.trim() == "/delegate" {
            let mut rows = vec![
                PickerRow {
                    id: "delegate:fallback:on".into(),
                    label: "Enable automatic worker returns".into(),
                    detail: "validated against the current recovery policy; enables automatic returns; undo: choose Disable automatic worker returns".into(),
                },
                PickerRow {
                    id: "delegate:fallback:off".into(),
                    label: "Disable automatic worker returns".into(),
                    detail: "validated against the current recovery policy; pauses automatic returns; undo: choose Enable automatic worker returns".into(),
                },
            ];
            for job in &self.deleg.d.jobs {
                let state = format!("{:?}", job.state);
                if !job.native_inputs().is_empty() {
                    rows.push(PickerRow {
                        id: format!("delegate:input:{}", job.id),
                        label: format!("Review input for worker #{}", job.id),
                        detail: format!("validated for worker #{} ({}); opens its pending native request; undo: Esc", job.id, state),
                    });
                }
                rows.push(PickerRow {
                    id: format!("delegate:recover:{}", job.id),
                    label: format!("Recover worker #{}", job.id),
                    detail: format!("validated for worker #{} ({}); reads its exact native result; undo: Esc", job.id, state),
                });
                rows.push(PickerRow {
                    id: format!("delegate:retry:{}", job.id),
                    label: format!("Retry worker #{} after reconciliation", job.id),
                    detail: format!("validated for worker #{} ({}); starts only the explicitly reconciled retry; undo: stop the new worker", job.id, state),
                });
            }
            self.open_guided_command("/delegate", rows);
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
                "" => { settings::open(self); self.guide_open_picker("/config", "inspect or change saved settings", "reselect the previous setting"); },
                "appearance" => appearance::open(self),
                "skills" => settings::catalog(self, false),
                "hooks" => settings::catalog(self, true),
                _ => appearance::command(self, args),
            }
            return true;
        }
        if trimmed == "/mcp" || trimmed.starts_with("/mcp ") {
            let args = trimmed.strip_prefix("/mcp").unwrap_or_default().trim();
            if args.is_empty() {
                let servers = crate::mcp_registry::load(&self.state_root).ok().map(|r| r.servers.len()).unwrap_or(0);
                let mut choices = crate::mcp_registry::provider_status_lines(&self.state_root, &self.cwd).into_iter().map(|line| PickerRow { id: format!("mcp:status:{line}"), label: "MCP server status".into(), detail: line }).collect::<Vec<_>>();
                choices.extend([
                    PickerRow { id: "mcp:add".into(), label: "Add a server".into(), detail: format!("validated against a command and argument array; adds one registry entry ({} detected); undo: /mcp remove NAME", servers) },
                    PickerRow { id: "mcp:toggle".into(), label: "Enable or disable a server".into(), detail: "validated against a detected server; changes delivery state only; undo: choose the previous state".into() },
                    PickerRow { id: "mcp:remove".into(), label: "Remove a server".into(), detail: "validated against a detected server; removes only the Concordia registry entry; undo: add it again".into() },
                    PickerRow { id: "mcp:import".into(), label: "Import from provider configuration".into(), detail: "reads provider files without writing them, then offers each server for confirmation; undo: not needed".into() },
                ]);
                self.open_guided_command("/mcp", choices);
            } else {
                let safe_line = sanitize_mcp_command(line);
                self.push_command(&safe_line);
                self.run_mcp_command(args);
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
        if trimmed == "/help" {
            self.open_guided_command("/help", vec![
                PickerRow { id: "help:everyday".into(), label: "Show everyday commands".into(), detail: "validated against the command registry; shows the common commands and their arguments; undo: Esc closes the panel".into() },
                PickerRow { id: "help:all".into(), label: "Show every command".into(), detail: "validated against the command registry; shows every registered command and its arguments; undo: Esc closes the panel".into() },
            ]);
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
            self.guide_open_picker("/open", "preview files mentioned in this chat", "Esc closes without editing");
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
            if target.is_empty() {
                let rows = self.workspace.panes.iter().map(|pane| PickerRow {
                    id: format!("close:{}", pane.id),
                    label: format!("Close file pane #{}", pane.id),
                    detail: "validated against this open pane; closes it; undo: reopen the file".into(),
                }).collect::<Vec<_>>();
                if rows.is_empty() {
                    self.open_guided_command("/close", vec![PickerRow { id: "close:none".into(), label: "No open file panes".into(), detail: "nothing will change; undo: not needed".into() }]);
                } else {
                    self.open_guided_command("/close", rows);
                }
                return true;
            }
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
        if line.trim() == "/queue" {
            let count = self.queue.items.len();
            self.open_guided_command("/queue", vec![
                PickerRow { id: "queue:list".into(), label: "Review queued prompts".into(), detail: format!("validated; shows {count} queued prompt(s); undo: Esc") },
                PickerRow { id: "queue:resume".into(), label: "Resume the queue".into(), detail: "validated when prompts are queued; resumes automatic delivery; undo: pause the queue".into() },
                PickerRow { id: "queue:clear".into(), label: "Clear the queue".into(), detail: "validated before deletion; removes queued prompts; undo: not available".into() },
            ]);
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
            let offered =
                crate::models::available_at(&self.state_root, self.provider, &self.account)
                    .into_iter()
                    .any(|entry| {
                        entry.id == *model
                            || entry
                                .efforts
                                .iter()
                                .any(|effort| effort.model_id.as_deref() == Some(model.as_str()))
                    });
            if !offered {
                self.push_system("model is not offered by the selected provider account; use /model to list available models");
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
        if matches!(&cmd, SlashCommand::Use) {
            self.open_use_picker();
            self.guide_open_picker("/use", "switch or manage accounts", "select the previous account");
            return true;
        }
        if matches!(&cmd, SlashCommand::Model) {
            self.open_model_picker();
            self.guide_open_picker("/model", "choose the active provider model", "choose the previous model");
            return true;
        }
        if let SlashCommand::Mode(arg) = &cmd {
            if arg.is_none() { self.open_trust_picker(); self.guide_open_picker("/mode", "set permission for the next turn", "select the previous mode"); } else { self.push_command(line); self.run_mode_command(arg.as_deref()); }
            return true;
        }
        if let SlashCommand::Customization(hooks) = &cmd {
            settings::catalog(self, *hooks);
            self.guide_open_picker(if *hooks { "/hooks" } else { "/skills" }, if *hooks { "inspect native hook sources" } else { "inspect selected skill sources" }, "Esc leaves sources unchanged");
            return true;
        }
        if let SlashCommand::Harness(arg) = &cmd {
            if arg.is_empty() { integrations::open(self); self.guide_open_picker("/harness", "inspect connected local harnesses", "Esc leaves receipts and provider homes unchanged"); } else { self.push_command(line); integrations::command(self, arg); }
            return true;
        }
        if matches!(&cmd, SlashCommand::Unknown(n) if n == "stats") {
            self.push_command(line);
            self.toggle_stats();
            return true;
        }
        // Unknown slash names are provider-owned. Returning false deliberately sends the
        // original line through the active provider turn instead of manufacturing a Concordia
        // "unknown command" dead end. Native execution is unavailable in print/exec mode; the
        // picker explains that boundary while direct input remains provider-visible.
        if matches!(&cmd, SlashCommand::Unknown(_)) {
            return false;
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
            CommandOutcome::OpenResume => { self.open_resume_picker(); self.guide_open_picker("/resume", "continue an explicit recorded conversation", "Esc leaves the current chat unchanged"); },
            CommandOutcome::OpenUse => {
                for l in crate::integration::refresh_summary(&self.state_root) {
                    self.push_system(l);
                }
                self.open_use_picker();
            }
            CommandOutcome::OpenAddProvider => self.open_add_provider_picker(),
            CommandOutcome::OpenRemovePicker => self.open_remove_picker(None),
            CommandOutcome::AddAccount(p) => self.begin_add_account(p),
            CommandOutcome::OpenModel => { self.open_model_picker(); self.guide_open_picker("/model", "choose the active provider model", "choose the previous model"); },
            CommandOutcome::OpenDoctor => self.open_doctor(),
            CommandOutcome::ShowUsage => self.show_usage(false),
            CommandOutcome::ShowUsageRefresh => self.show_usage(true),
            CommandOutcome::OpenUsage => { self.open_usage_picker(); self.guide_open_picker("/usage", "inspect provider-reported limits", "Esc closes without changing provider state"); self.footer_left = super::usage_view::USAGE_FOOTER.into(); },
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

fn sanitize_mcp_command(line: &str) -> String {
    let mut words = line.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    let secret_flag = |word: &str| matches!(word.to_ascii_lowercase().as_str(), "--token" | "--access-token" | "--api-key" | "--apikey" | "--secret" | "--password" | "-t");
    for index in 0..words.len() {
        if secret_flag(&words[index]) {
            if words[index].contains('=') {
                let equals = words[index].find('=').unwrap();
                words[index].truncate(equals + 1);
                words[index].push_str("[redacted]");
            } else if let Some(value) = words.get_mut(index + 1) {
                *value = "[redacted]".into();
            }
        }
    }
    words.join(" ")
}
