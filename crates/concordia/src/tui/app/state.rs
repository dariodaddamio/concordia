use super::super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum Drag {
    None,
    Transcript,
    Input,
}

/// Screen rectangles from the last draw, for mouse hit-testing.
#[derive(Default, Clone, Copy)]
pub(in crate::tui) struct Areas {
    pub(in crate::tui) header_home: Option<Rect>,
    pub(in crate::tui) header_credits: Option<Rect>,
    pub(in crate::tui) header_email: Option<Rect>,
    pub(in crate::tui) transcript: Rect,
    pub(in crate::tui) input: Rect,
    pub(in crate::tui) bottom_button: Rect,
    pub(in crate::tui) footer: Rect,
    pub(in crate::tui) run_cancel: Rect,
    pub(in crate::tui) permissions: Rect,
    pub(in crate::tui) permissions_popup: Rect,
    pub(in crate::tui) pinned_card: Option<(Rect, usize)>,
    pub(in crate::tui) modal_close: Option<(Rect, modal::Target)>,
    pub(in crate::tui) input_text_w: usize,
    pub(in crate::tui) palette: Rect,
}

#[derive(Default)]
pub(in crate::tui) struct ClickTracker {
    pub(in crate::tui) last: Option<(u16, u16, Instant, u8)>,
}

impl ClickTracker {
    pub(in crate::tui) fn register(&mut self, x: u16, y: u16, now: Instant) -> u8 {
        let n = match self.last {
            Some((lx, ly, at, n))
                if lx == x && ly == y && now.duration_since(at) <= MULTI_CLICK =>
            {
                n % 3 + 1
            }
            _ => 1,
        };
        self.last = Some((x, y, now, n));
        n
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum MsgRole {
    User,
    Assistant,
    System,
    Command,
    Card,
    Thinking,
    Meta,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::tui) enum Phase {
    Waiting,
    Thinking,
    Tool(String),
    Writing,
}

impl Phase {
    pub(in crate::tui) fn color(&self) -> Color {
        match self {
            Phase::Waiting => Color::Cyan,
            Phase::Thinking => Color::Magenta,
            Phase::Tool(_) => Color::Yellow,
            Phase::Writing => Color::Green,
        }
    }

    pub(in crate::tui) fn frames(&self) -> &'static [&'static str] {
        match self {
            Phase::Waiting => &["\u{2727}", "\u{2726}", "\u{2727}", "\u{2726}"],
            Phase::Thinking => &["\u{25c7}", "\u{25c8}", "\u{25c6}", "\u{25c8}"],
            Phase::Tool(_) => &["\u{203a}", "\u{00bb}", "\u{203a}", "\u{00b7}"],
            Phase::Writing => &[
                "\u{2581}", "\u{2582}", "\u{2583}", "\u{2584}", "\u{2583}", "\u{2582}",
            ],
        }
    }

    pub(in crate::tui) fn frame(&self, tick: u64) -> &'static str {
        if let Phase::Tool(name) = self {
            return crate::provider::stream_cards::tool_icon(name);
        }
        let f = self.frames();
        f[(tick as usize) % f.len()]
    }

    pub(in crate::tui) fn frame_with_motion(&self, tick: u64, motion: bool) -> &'static str {
        if motion || matches!(self, Phase::Tool(_)) {
            self.frame(tick)
        } else {
            self.frames()[0]
        }
    }
}

#[derive(Clone)]
pub(in crate::tui) struct ChatLine {
    pub(in crate::tui) role: MsgRole,
    pub(in crate::tui) text: String,
    pub(in crate::tui) card: Option<StreamCard>,
    pub(in crate::tui) meta: Option<(ProviderKind, String)>,
}

impl ChatLine {
    pub(in crate::tui) fn is_map(&self) -> bool {
        self.role == MsgRole::System
            && self.text.starts_with("[concordia map]")
            && self.text.contains('\n')
    }

    pub(in crate::tui) fn toggles(&self) -> bool {
        match self.role {
            MsgRole::Card => true,
            MsgRole::Thinking => thinking_expandable(&self.text),
            _ => self.is_map(),
        }
    }
}

pub(in crate::tui) type ConnectFn = Box<dyn Fn(&Path, ProviderKind) -> Result<AddResult>>;

pub(in crate::tui) struct PendingLogin {
    pub(in crate::tui) account: String,
    pub(in crate::tui) provider: ProviderKind,
    pub(in crate::tui) label: String,
    pub(in crate::tui) home: PathBuf,
    pub(in crate::tui) ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum PickerKind {
    Doctor,
    DoctorConfirm,
    Settings,
    FilePreferences,
    Customization,
    Idle,
    Appearance,
    Theme,
    BrandColor,
    BrandCustom,
    ThemeImport,
    ThemeExport,
    CreditDetails,
    Tokens,
    Integrations,
    Related,
    Agents,
    Recovery,
    Files,
    FileActions,
    Artifacts,
    ArtifactDetail,
    Resume,
    Trust,
    FullModeConfirm,
    Use,
    AddProvider,
    Remove,
    RemoveConfirm,
    Model,
    Usage,
    Continuation,
    HandoffSettings,
    ResumeDeleteConfirm,
    Update,
    NeedsYou,
}

pub(in crate::tui) struct Picker {
    pub(in crate::tui) kind: PickerKind,
    pub(in crate::tui) title: String,
    pub(in crate::tui) rows: Vec<PickerRow>,
    pub(in crate::tui) filter: String,
    pub(in crate::tui) state: ListState,
}

pub(in crate::tui) struct DoctorUi {
    pub(in crate::tui) checks: Vec<crate::doctor::Check>,
    pub(in crate::tui) fixes: Vec<crate::doctor::Fix>,
    pub(in crate::tui) fixed: HashMap<usize, String>,
}

pub(in crate::tui) struct ModelRefresh {
    pub(in crate::tui) provider: ProviderKind,
    pub(in crate::tui) account: String,
    pub(in crate::tui) rx: std_mpsc::Receiver<Vec<crate::models::ModelEntry>>,
}

impl Picker {
    pub(in crate::tui) fn filtered(&self) -> Vec<&PickerRow> {
        let q = self.filter.to_ascii_lowercase();
        if q.is_empty() {
            return self.rows.iter().collect();
        }
        self.rows
            .iter()
            .filter(|r| {
                r.label.to_ascii_lowercase().contains(&q)
                    || r.detail.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    pub(in crate::tui) fn sync_typed_model(&mut self) {
        if self.kind != PickerKind::Model {
            return;
        }
        self.rows.retain(|r| !r.id.starts_with(TYPED_MODEL_PREFIX));
        let typed = self.filter.trim();
        let listed = self
            .rows
            .iter()
            .any(|r| r.id.strip_prefix("model:") == Some(typed));
        if valid_model_id(typed) && !listed {
            self.rows.push(PickerRow {
                id: format!("{TYPED_MODEL_PREFIX}{typed}"),
                label: format!("use {typed}"),
                detail: "typed model id".into(),
            });
        }
    }

    pub(in crate::tui) fn select_clamp(&mut self) {
        self.sync_typed_model();
        let n = self.filtered().len();
        if n == 0 {
            self.state.select(None);
            return;
        }
        let i = self.state.selected().unwrap_or(0).min(n - 1);
        self.state.select(Some(i));
    }
}

pub(in crate::tui) struct BusySendConfirmation {
    pub(in crate::tui) prompt: String,
    pub(in crate::tui) conversation: String,
    pub(in crate::tui) turn_started: Option<Instant>,
    pub(in crate::tui) armed: Instant,
}

pub(crate) struct App {
    pub(in crate::tui) appearance: appearance::State,
    pub(in crate::tui) continuation: continuation::State,
    pub(in crate::tui) native_owner: Option<crate::provider::interaction::InteractionOwner>,
    pub(in crate::tui) native_usage_model: Option<String>,
    pub(in crate::tui) native_usage_session: Option<String>,
    pub(in crate::tui) native_accounting_owner:
        Option<crate::provider::interaction::InteractionOwner>,
    pub(in crate::tui) scopes: workspace_scope::State,
    pub(in crate::tui) managed_workspaces: managed_workspaces::State,
    pub(in crate::tui) project_search: workspace_search::State,
    pub(in crate::tui) file_action_target: Option<(crate::file_links::ResolvedFile, PathBuf)>,
    pub(in crate::tui) continuation_target: Option<String>,
    pub(in crate::tui) state_root: PathBuf,
    pub(in crate::tui) cwd: PathBuf,
    pub(in crate::tui) account_names: std::collections::BTreeMap<String, String>,
    pub(in crate::tui) account_email: Option<String>,
    pub(in crate::tui) header_email_revealed: bool,
    pub(in crate::tui) header_email_pressed: bool,
    pub(in crate::tui) header_home_pressed: bool,
    pub(in crate::tui) permissions_pressed: bool,
    pub(in crate::tui) permission_rows: Vec<(Rect, usize)>,
    pub(in crate::tui) permission_row_pressed: Option<usize>,
    pub(in crate::tui) card_header_pressed: Option<usize>,
    pub(in crate::tui) header_credits_pressed: bool,
    pub(in crate::tui) messages: Vec<ChatLine>,
    pub(in crate::tui) idle_notice: Option<ProviderKind>,
    pub(in crate::tui) handoff_after_cancel: bool,
    pub(in crate::tui) editor: Editor,
    pub(in crate::tui) submitted_prompt: Option<String>,
    pub(in crate::tui) expanded_activity: bool,
    pub(in crate::tui) activity: activity::Activity,
    pub(in crate::tui) palette: Palette,
    pub(in crate::tui) key_sheet: bool,
    pub(in crate::tui) chatref: chatref::ChatRef,
    pub(in crate::tui) related_files: related_files::State,
    pub(in crate::tui) questions: questions::State,
    pub(in crate::tui) images: images::State,
    pub(in crate::tui) raster: raster::State,
    pub(in crate::tui) history: Vec<String>,
    pub(in crate::tui) hist_pos: Option<usize>,
    pub(in crate::tui) hist_draft: String,
    pub(in crate::tui) parked_draft: Option<String>,
    pub(in crate::tui) fresh_draft: bool,
    pub(in crate::tui) quit_armed: Option<Instant>,
    pub(in crate::tui) busy_send_confirmation: Option<BusySendConfirmation>,
    pub(in crate::tui) full_mode_confirmed: bool,
    pub(in crate::tui) input_scroll: usize,
    pub(in crate::tui) input_scroll_anchor: Option<(u64, usize, usize, usize)>,
    pub(in crate::tui) view: TranscriptView,
    pub(in crate::tui) areas: Areas,
    pub(in crate::tui) drag: Drag,
    pub(in crate::tui) file_jobs: Vec<mpsc::UnboundedReceiver<String>>,
    pub(in crate::tui) workspace: workspace::Workspace,
    pub(in crate::tui) file_click: Option<transcript::Pos>,
    #[cfg(test)]
    pub(in crate::tui) last_file_open: Option<crate::file_links::ResolvedFile>,
    pub(in crate::tui) clicks: ClickTracker,
    pub(in crate::tui) modal_close_pressed: Option<modal::Target>,
    pub(in crate::tui) flash: Option<(String, Instant)>,
    pub(in crate::tui) busy: bool,
    pub(in crate::tui) pulse_on: bool,
    pub(in crate::tui) cancel_requested: bool,
    pub(in crate::tui) tick_count: u64,
    pub(in crate::tui) should_quit: bool,
    pub(in crate::tui) provider: ProviderKind,
    pub(in crate::tui) account: String,
    pub(in crate::tui) home_path: Option<PathBuf>,
    pub(in crate::tui) model: String,
    pub(in crate::tui) trust: TrustMode,
    pub(in crate::tui) native_session_id: Option<String>,
    pub(in crate::tui) task_id: Option<String>,
    pub(in crate::tui) session_chip: Option<String>,
    pub(in crate::tui) session_ui: session_ui::State,
    pub(in crate::tui) native_agents: native_agents::State,
    pub(in crate::tui) footer_left: String,
    pub(in crate::tui) picker: Option<Picker>,
    pub(in crate::tui) doctor: Option<DoctorUi>,
    pub(in crate::tui) doctor_applier: Box<dyn crate::doctor::FixApplier>,
    pub(in crate::tui) model_refresh: Option<ModelRefresh>,
    pub(in crate::tui) token_reads: tokens::State,
    pub(in crate::tui) integrations: integrations::State,
    pub(in crate::tui) artifacts: artifacts::State,
    pub(in crate::tui) landing: Option<landing::Landing>,
    pub(in crate::tui) update_preview: Option<UpdatePreview>,
    pub(in crate::tui) update_rows: usize,
    pub(in crate::tui) update_view: usize,
    pub(in crate::tui) cancel_handle: Option<Arc<RunningTurn>>,
    pub(in crate::tui) preparing_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(in crate::tui) native_sources: Vec<crate::native_memory::Binding>,
    pub(in crate::tui) prior_provider: ProviderKind,
    pub(in crate::tui) prior_home: Option<String>,
    pub(in crate::tui) need_context_pack: bool,
    pub(in crate::tui) task_map: Option<String>,
    pub(in crate::tui) thread: Option<crate::threads::ThreadLog>,
    pub(in crate::tui) thread_meta_done: bool,
    pub(in crate::tui) reference_journal: reference_journal::State,
    pub(in crate::tui) usage_summary: String,
    pub(in crate::tui) usage_credits: String,
    pub(in crate::tui) usage_reset: Option<String>,
    pub(in crate::tui) usage_tone: UsageTone,
    pub(in crate::tui) usage_warning: Option<String>,
    pub(in crate::tui) usage_fetcher: UsageFetcher,
    pub(in crate::tui) usage_block: Option<usize>,
    pub(in crate::tui) stats: Option<crate::sysstats::StatsHandle>,
    pub(in crate::tui) compat_rx: Option<compat_ui::CompatRx>,
    pub(in crate::tui) picker_tones: HashMap<String, UsageTone>,
    pub(in crate::tui) live_cards: Vec<StreamCard>,
    pub(in crate::tui) live_text: String,
    pub(in crate::tui) streamed_text_committed: bool,
    pub(in crate::tui) live_thinking: String,
    pub(in crate::tui) phase: Phase,
    pub(in crate::tui) job: Option<Job>,
    pub(in crate::tui) no_color: bool,
    pub(in crate::tui) turn_started: Option<Instant>,
    pub(in crate::tui) turn_usage: crate::provider::stream_cards::TurnUsage,
    pub(in crate::tui) session_input: u64,
    pub(in crate::tui) session_output: u64,
    pub(in crate::tui) session_cost: f64,
    pub(in crate::tui) idle_ticks: u64,
    pub(in crate::tui) tip_seed: u64,
    pub(in crate::tui) tip_limiter: tips::RateLimiter,
    pub(in crate::tui) pending_login: Option<PendingLogin>,
    pub(in crate::tui) connect: ConnectFn,
    pub(in crate::tui) reexec_path: Option<PathBuf>,
    pub(in crate::tui) deleg: workers::Panel,
    pub(in crate::tui) worker_panel_open: bool,
    pub(in crate::tui) worker_panel_explicit: Option<bool>,
    /// Labels of every open tab and the index of this one; empty with a single tab. The runner
    /// fills it before each frame, and the drawn label rects come back for mouse hits.
    pub(in crate::tui) tab_strip: Vec<String>,
    pub(in crate::tui) tab_index: usize,
    pub(in crate::tui) tab_hits: Vec<Rect>,
    pub(in crate::tui) tab_close_hits: Vec<Rect>,
    pub(in crate::tui) rename_active: bool,
    pub(in crate::tui) user_cmds: Vec<crate::user_commands::UserCommand>,
    pub(in crate::tui) queue: queue::Queue,
    pub(in crate::tui) last_sent: Option<String>,
    pub(in crate::tui) keep: persist::Keep,
    pub(in crate::tui) signed_out_confirmation: Option<(String, String)>,
    pub(in crate::tui) signed_out_overrides: HashSet<String>,
    pub(in crate::tui) needs_you: crate::needs_you::Queue,
}

impl App {
    #[allow(dead_code)]
    pub(crate) fn is_busy(&self) -> bool {
        self.busy
    }

    #[allow(dead_code)]
    pub(crate) fn has_unsent_work(&self) -> bool {
        !self.editor.text().is_empty() || !self.queue.items.is_empty() || self.queue.edit.is_some()
    }

    pub(in crate::tui) fn continuity_home(&self) -> Option<String> {
        self.home_path
            .as_ref()
            .and_then(|home| home.canonicalize().ok())
            .map(|home| home.to_string_lossy().into_owned())
    }
    pub(in crate::tui) fn refresh_binding(&mut self) {
        self.account_names = crate::store::load_store(&self.state_root).account_nicknames;
        if let Some(sel) = get_use_selection(&self.state_root) {
            if self.account != sel.account
                || self.provider != sel.provider
                || self
                    .home_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    != sel.home_path
            {
                self.session_ui.invalidate();
                self.header_email_revealed = false;
                self.header_email_pressed = false;
            }
            self.provider = sel.provider;
            self.account = sel.account;
            self.home_path = sel.home_path.map(PathBuf::from);
            self.model = sel
                .model
                .filter(|m| !m.trim().is_empty())
                .or_else(|| {
                    self.home_path
                        .as_ref()
                        .and_then(|h| inspect_provider(self.provider, h).model)
                })
                .unwrap_or_else(|| "?".into());
        }
        let email = self
            .home_path
            .as_ref()
            .and_then(|h| inspect_provider(self.provider, h).login)
            .filter(|login| login.contains('@'))
            .map(|email| email.chars().filter(|c| !c.is_control()).collect());
        if self.account_email != email {
            self.header_email_revealed = false;
            self.header_email_pressed = false;
        }
        self.account_email = email;
        self.trust = get_settings(&self.state_root).trust;
        self.refresh_usage();
    }

    /// Short label for the tab strip: the chat title, a busy marker and a needs-input marker.
    pub(in crate::tui) fn tab_label(&self) -> String {
        let title = if self.landing.is_some() {
            "Home"
        } else {
            self.session_ui.title.as_deref().unwrap_or("New chat")
        };
        let mut label: String = title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .filter(|c| !c.is_control())
            .take(24)
            .collect();
        if self.busy && self.questions.waiting() {
            label.push_str(" ?");
        } else if self.busy {
            label.push_str(" …");
        }
        label
    }

    pub(in crate::tui) fn window_title(&self) -> String {
        let status = if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) {
            Some("Updating".to_owned())
        } else if self.cancel_requested && self.busy {
            Some("Stopping".to_owned())
        } else if worker_input::active(self) || (self.busy && self.questions.waiting()) {
            Some("Needs input".to_owned())
        } else if self.busy {
            Some(match &self.phase {
                Phase::Thinking => "Thinking".into(),
                Phase::Writing => "Writing".into(),
                Phase::Waiting => "Working".into(),
                Phase::Tool(name) => {
                    let name = name.to_ascii_lowercase();
                    if name.contains("patch") || name.contains("edit") || name.contains("write") {
                        "Editing"
                    } else if name.contains("web") {
                        "Searching the web"
                    } else if name.contains("search") || name.contains("find") {
                        "Searching"
                    } else if name.contains("read") {
                        "Reading"
                    } else if name.contains("shell")
                        || name.contains("exec")
                        || name.contains("command")
                    {
                        "Running command"
                    } else {
                        "Working"
                    }
                    .into()
                }
            })
        } else if self.continuation.waiting() {
            Some("Paused".to_owned())
        } else if self.deleg.d.any_active() {
            Some("Workers running".to_owned())
        } else {
            None
        };
        let project = self
            .cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "New chat".into());
        let task = if self.landing.is_some() {
            "Home"
        } else {
            self.session_ui.title.as_deref().unwrap_or(&project)
        };
        let clean = |value: &str, max: usize| -> String {
            value
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace(['\u{2014}', '\u{2013}'], "-")
                .chars()
                .filter(|c| !c.is_control())
                .take(max)
                .collect()
        };
        let task = clean(task, 40);
        let task = if task.chars().count() == 40 {
            format!("{}...", task.trim_end())
        } else {
            task
        };
        match status {
            Some(status) => format!("Concordia | {} | {task}", clean(&status, 24)),
            None => format!("Concordia | {task}"),
        }
    }

    pub(in crate::tui) fn run_status_label(&self) -> String {
        if self.cancel_requested {
            return if self.queue.force_pending {
                "Stopping to send the queued message\u{2026}".into()
            } else {
                "Stopping this turn\u{2026}".into()
            };
        }
        match &self.phase {
            Phase::Waiting if self.cancel_requested => "Stopping this turn\u{2026}".into(),
            Phase::Waiting if self.live_cards.is_empty() => {
                format!("{} \u{b7} starting the turn", self.account_label())
            }
            Phase::Waiting => "Reviewing the latest tool results".into(),
            Phase::Thinking => "Working through the next step".into(),
            Phase::Writing => "Putting the answer together".into(),
            Phase::Tool(name) => self
                .live_cards
                .iter()
                .rev()
                .find(|c| c.status == CardStatus::Running)
                .map(crate::provider::stream_cards::format_card_line)
                .unwrap_or_else(|| format!("Running {name}")),
        }
    }

    /// Suggestions for the prompt as typed (empty when closed, dismissed, or not a slash prompt).
    pub(in crate::tui) fn palette_entries(&self) -> Vec<palette::Entry> {
        if self.editor.text().starts_with("/open ") || self.editor.text().starts_with("/preview ") {
            return Vec::new();
        }
        // Not while a recalled prompt is on screen: up/down must keep walking history.
        if self.palette.dismissed || self.picker.is_some() || self.hist_pos.is_some() {
            return Vec::new();
        }
        let input = self.editor.text();
        let lower = input.to_ascii_lowercase();
        if palette::has_argument_menu(&input) {
            let accounts = if lower.starts_with("/use") || lower.starts_with("/workers") {
                accounts_with_auth(&self.state_root)
                    .into_iter()
                    .map(|a| (a.account.clone(), a.label))
                    .collect()
            } else {
                Vec::new()
            };
            let models = if lower.starts_with("/model") {
                self.home_path
                    .as_deref()
                    .map(|h| list_models(self.provider, h))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            return palette::matching_arguments(&input, &accounts, &models);
        }
        palette::matching_with(&input, &self.user_cmds)
    }

    /// Put the chosen command in the prompt.
    pub(in crate::tui) fn complete_palette(&mut self, entry: palette::Entry) {
        let complete_branch = self.editor.text().eq_ignore_ascii_case(&entry.command)
            && palette::is_branch(&entry.command);
        self.editor.set_text(&if complete_branch {
            format!("{} ", entry.command)
        } else {
            entry.command
        });
        self.palette.reset();
    }

    /// Tab: complete the highlighted suggestion.
    pub(in crate::tui) fn tab_complete_slash(&mut self) {
        let entries = self.palette_entries();
        if let Some(e) = entries.get(self.palette.selected.min(entries.len().saturating_sub(1))) {
            let e = e.clone();
            self.complete_palette(e);
        }
    }

    pub(in crate::tui) fn remember_prompt(&mut self, line: &str) {
        self.hist_pos = None;
        self.push_history(line);
    }

    pub(in crate::tui) fn history_prev(&mut self) {
        if self.hist_pos.is_none() && self.fresh_draft {
            let draft = self
                .parked_draft
                .replace(self.editor.text())
                .unwrap_or_default();
            self.restore_composer(&draft);
            self.fresh_draft = false;
            return;
        }
        if self.history.is_empty() {
            return;
        }
        let pos = match self.hist_pos {
            None => {
                self.hist_draft = self.composer_value();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.hist_pos = Some(pos);
        self.restore_composer(&self.history[pos].clone());
    }

    pub(in crate::tui) fn history_next(&mut self) {
        let Some(i) = self.hist_pos else {
            if !self.fresh_draft
                && (!self.editor.text().is_empty()
                    || self.parked_draft.as_ref().is_some_and(|s| !s.is_empty()))
            {
                let next = self
                    .parked_draft
                    .replace(self.editor.text())
                    .unwrap_or_default();
                self.restore_composer(&next);
                self.fresh_draft = true;
                self.flash("Fresh prompt; Up on the first row returns to your draft");
            }
            return;
        };
        if i + 1 < self.history.len() {
            self.hist_pos = Some(i + 1);
            self.restore_composer(&self.history[i + 1].clone());
        } else {
            self.hist_pos = None;
            let draft = std::mem::take(&mut self.hist_draft);
            self.restore_composer(&draft);
        }
    }

    pub(in crate::tui) fn paste_clipboard(&mut self) {
        if !cfg!(test)
            && clipboard::has_attachment()
            && self.picker.is_none()
            && self.workspace.focused.is_none()
            && self.queue.edit.is_none()
            && !self.questions.focused
            && self.activity.detail.is_none()
            && !worker_input::active(self)
            && !self.project_search.active()
        {
            self.start_image_import(None, true);
            return;
        }
        match clipboard::get_text() {
            Ok(t) if !t.is_empty() => self.paste_text(&t),
            Ok(_) => {}
            Err(e) => self.flash(format!("paste failed — {e}")),
        }
    }

    /// Copy the current selection. With `fall_back_to_reply`, copy the last assistant reply
    /// when nothing is selected. Returns whether anything was copied.
    pub(in crate::tui) fn copy(&mut self, fall_back_to_reply: bool) -> bool {
        let text = self.selected_text().or_else(|| {
            fall_back_to_reply
                .then(|| self.last_assistant_text())
                .flatten()
        });
        match text {
            Some(t) => self.write_clipboard(&t),
            None => {
                self.flash("nothing to copy — drag to select, or /copy for the last reply");
                false
            }
        }
    }

    pub(in crate::tui) fn push_system(&mut self, text: impl Into<String>) {
        let text = text.into();
        // A provider often reports one failure several ways (an event, a result, stderr).
        // Say it once.
        if self
            .messages
            .last()
            .is_some_and(|m| m.role == MsgRole::System && m.text.trim() == text.trim())
        {
            return;
        }
        self.messages.push(ChatLine {
            role: MsgRole::System,
            text,
            card: None,
            meta: None,
        });
    }

    pub(in crate::tui) fn clear_idle_notice(&mut self) {
        if self.idle_notice.take().is_some() {
            if let Some(index) = self.messages.iter().position(|line| {
                line.meta
                    .as_ref()
                    .is_some_and(|(_, label)| label == "__concordia_idle_notice__")
            }) {
                self.messages.remove(index);
            }
        }
    }

    pub(in crate::tui) fn idle_handoff(&mut self) {
        let has_other = accounts_with_auth(&self.state_root)
            .iter()
            .any(|a| a.account != self.account && a.home_path.is_some());
        if !has_other {
            self.push_system("hand off disabled: no other signed-in account");
            return;
        }
        let Some(prompt) = self
            .submitted_prompt
            .clone()
            .or_else(|| self.last_sent.clone())
        else {
            self.push_system("hand off unavailable: the original request is not available");
            return;
        };
        self.continuation.prepare_manual_handoff(prompt);
        self.handoff_after_cancel = true;
        self.request_cancel();
    }

    pub(in crate::tui) fn open_idle_picker(&mut self) {
        let has_other = accounts_with_auth(&self.state_root)
            .iter()
            .any(|a| a.account != self.account && a.home_path.is_some());
        self.show_picker(
            PickerKind::Idle,
            "turn is quiet".into(),
            vec![
                PickerRow {
                    id: "idle:wait".into(),
                    label: "keep waiting".into(),
                    detail: "leave the notice and continue waiting".into(),
                },
                PickerRow {
                    id: "idle:cancel".into(),
                    label: "cancel".into(),
                    detail: "stop this turn".into(),
                },
                PickerRow {
                    id: "idle:handoff".into(),
                    label: "hand off".into(),
                    detail: if has_other {
                        "choose another account".into()
                    } else {
                        "disabled: no other signed-in account".into()
                    },
                },
            ],
            false,
        );
    }

    pub(in crate::tui) fn needs_you_session(&self) -> String {
        self.questions
            .native_session_id()
            .or_else(|| self.native_session_id.clone())
            .or_else(|| self.task_id.clone())
            .unwrap_or_else(|| "chat".into())
    }

    pub(in crate::tui) fn sync_needs_you(&mut self) {
        use crate::needs_you::{Item, ItemKind, SessionState};
        self.questions.refresh_native();
        let session = self.needs_you_session();
        let state = if self.busy {
            SessionState::Running
        } else if self.signed_out_confirmation.is_some()
            || self.questions.waiting()
            || self.queue.paused
        {
            SessionState::Waiting
        } else {
            SessionState::Idle
        };
        self.needs_you.set_state(session.clone(), state);
        let approval = self.questions.native_approval_waiting();
        let current = [
            (
                "question",
                self.questions.waiting() && !approval,
                ItemKind::Question,
                "answer provider question",
            ),
            (
                "approval",
                approval,
                ItemKind::Approval,
                "answer permission request",
            ),
            (
                "paused-queue",
                self.queue.paused,
                ItemKind::PausedQueue,
                "resume paused prompt queue",
            ),
            (
                "idle",
                self.idle_notice.is_some(),
                ItemKind::IdleNotice,
                "turn is quiet",
            ),
            (
                "signed-out",
                self.signed_out_confirmation.is_some(),
                ItemKind::SignedOut,
                "confirm signed-out account",
            ),
        ];
        for (suffix, present, kind, title) in current {
            self.needs_you.remove_suffix(suffix);
            let id = format!("{session}:{suffix}");
            if present {
                self.needs_you.add(Item {
                    id,
                    session_id: session.clone(),
                    title: title.into(),
                    kind,
                });
            }
        }
        for job in &self.deleg.d.jobs {
            let id = format!("worker:{}", job.id);
            if job.state == crate::delegate::WorkerState::Done && !job.collected {
                self.needs_you.add(Item {
                    id,
                    session_id: job.session_id.clone().unwrap_or_else(|| job.id.to_string()),
                    title: format!("worker {} finished", job.id),
                    kind: ItemKind::WorkerResult,
                });
            } else {
                self.needs_you.remove(&id);
            }
        }
    }

    pub(in crate::tui) fn open_needs_you_picker(&mut self) {
        self.sync_needs_you();
        let rows = self
            .needs_you
            .items()
            .map(|item| PickerRow {
                id: format!("needs:{}", item.id),
                label: item.title.clone(),
                detail: format!("{} · {}", item.kind.label(), item.session_id),
            })
            .collect();
        self.show_picker(PickerKind::NeedsYou, "needs you".into(), rows, false);
    }

    pub(in crate::tui) fn idle_provider_name(provider: ProviderKind) -> &'static str {
        match provider {
            ProviderKind::Codex => "Codex",
            ProviderKind::Claude => "Claude Code",
            ProviderKind::Cursor => "Cursor",
        }
    }

    pub(in crate::tui) fn poll_model_refresh(&mut self) {
        let Some(refresh) = self.model_refresh.as_ref() else {
            return;
        };
        let Ok(models) = refresh.rx.try_recv() else {
            return;
        };
        let provider = refresh.provider;
        let account = refresh.account.clone();
        self.model_refresh = None;
        if self.provider != provider || self.account != account {
            return;
        }
        let Some(picker) = self.picker.as_mut().filter(|p| p.kind == PickerKind::Model) else {
            return;
        };
        let filter = picker.filter.clone();
        let selected = picker.state.selected().unwrap_or(0);
        let chosen = Some(self.model.trim()).filter(|m| !m.is_empty() && *m != "?");
        picker.rows = model_picker_rows(models, chosen);
        picker.title = "model".into();
        picker.filter = filter;
        picker.state.select(Some(selected));
        picker.select_clamp();
        self.footer_left = "up/down Enter · r refresh · type to filter · Esc".into();
    }

    pub(in crate::tui) fn update_running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update)
    }

    pub(in crate::tui) fn update_blocked_by_work(&self) -> bool {
        self.busy
            || self.deleg.d.any_active()
            || self.deleg.has_pending_prompt()
            || self.workspace.has_unsaved()
    }

    pub(in crate::tui) fn start_job(&mut self, kind: JobKind) {
        if self.job.is_some() {
            self.push_system("an update is already running - Esc cancels it");
            return;
        }
        if kind == JobKind::Update && self.update_blocked_by_work() {
            self.push_system("Cannot update while work or unsaved file edits remain. Finish or cancel chat/workers and save or close edited files, then press Enter again. The update has not been queued.");
            return;
        }
        crate::logging::info(&format!("job-start {kind:?}"));
        self.job = Some(match kind {
            JobKind::Update => start_update(self.cwd.clone()),
            JobKind::Check => start_check(self.cwd.clone()),
        });
    }

    pub(in crate::tui) fn open_update_preview(&mut self) {
        self.update_preview = None;
        self.show_picker(PickerKind::Update, "update".into(), Vec::new(), false);
        self.start_job(JobKind::Check);
    }

    /// Deliver a finished background job (called on every tick).
    pub(in crate::tui) fn poll_job(&mut self) {
        let Some(result) = self.job.as_ref().and_then(Job::poll) else {
            return;
        };
        let cancelled = self.job.as_ref().is_some_and(Job::is_cancelled);
        self.job = None;
        if matches!(result, JobResult::Update(_)) {
            self.close_update_screen();
        }
        match result {
            JobResult::Update(Ok(UpdateOutcome::Installed(result))) => {
                crate::logging::info(&format!(
                    "update-installed {}",
                    result.install_path.display()
                ));
                if result.summary.is_empty() {
                    self.push_system(format!(
                        "built \u{2192} {} \u{b7} restarting into {}",
                        result.built_from.display(),
                        result.install_path.display()
                    ));
                } else {
                    self.push_system(format!(
                        "{} \u{b7} restarting into {}",
                        result.summary,
                        result.install_path.display()
                    ));
                }
                for line in result.log_tail.lines().take(4) {
                    self.push_system(line.to_string());
                }
                self.reexec_path = Some(result.install_path);
                self.should_quit = true;
            }
            JobResult::Update(Ok(UpdateOutcome::UpToDate(msg))) => self.push_system(msg),
            JobResult::Update(Err(e)) if cancelled || e.to_string() == "cancelled" => {
                crate::logging::info("update-cancelled");
                self.push_system("update cancelled");
            }
            JobResult::Update(Err(e)) => {
                crate::logging::error(&format!("update-failed {e:#}"));
                self.push_system(format!("update failed - {e:#}"));
            }
            JobResult::Check(Ok(preview)) => {
                self.footer_left = if preview.install_available {
                    "Enter installs \u{b7} \u{2191}\u{2193} scroll \u{b7} Esc close"
                } else {
                    "\u{2191}\u{2193} scroll \u{b7} Esc close"
                }
                .into();
                self.update_preview = Some(preview);
            }
            JobResult::Check(Err(e)) => {
                let message = format!("update check failed - {e:#}");
                if self
                    .picker
                    .as_ref()
                    .is_some_and(|p| p.kind == PickerKind::Update)
                {
                    self.update_preview = Some(UpdatePreview {
                        history: Vec::new(),
                        history_expanded: false,
                        lines: vec![
                            crate::update::PreviewLine::Text(message),
                            crate::update::PreviewLine::Hint("Esc returns to your chat".into()),
                        ],
                        install_available: false,
                        release_url: None,
                    });
                    self.footer_left = "Esc close".into();
                } else {
                    self.push_system(message);
                }
            }
        }
    }

    pub(in crate::tui) fn submit_input(&mut self, turn_tx: &mpsc::UnboundedSender<String>) {
        if self.images.importing() {
            self.flash("Wait for the attachment to finish importing");
            return;
        }
        let original_prompt = self.composer_value();
        let mut line = self.editor.text();
        // Commands retain their forgiving whitespace handling; prompts retain formatting.
        if line.trim_start().starts_with('/') && !line.contains('\n') {
            line = line.trim().to_owned();
        }
        if line.trim().is_empty() && self.images.count() > 0 {
            line = self.images.default_prompt().into();
        }
        if line.trim().is_empty() {
            return;
        }
        let user_prompt = crate::user_commands::expand(&self.user_cmds, &line);
        if self.session_ui.loading() && (!line.starts_with('/') || user_prompt.is_some()) {
            self.flash("recent messages are loading; your draft is kept");
            return;
        }
        if self.busy && (!line.starts_with('/') || user_prompt.is_some()) {
            let confirmed = self.busy_send_confirmation.take().is_some_and(|armed| {
                armed.prompt == original_prompt
                    && armed.conversation == self.deleg.d.conversation_id()
                    && armed.turn_started == self.turn_started
                    && armed.armed.elapsed() <= Duration::from_secs(5)
            });
            if !confirmed {
                self.busy_send_confirmation = Some(BusySendConfirmation {
                    prompt: original_prompt,
                    conversation: self.deleg.d.conversation_id().to_owned(),
                    turn_started: self.turn_started,
                    armed: Instant::now(),
                });
                self.flash("Enter again to interrupt and send this message; Tab queues it.");
                return;
            }
            if self.queue.force_pending {
                self.flash("Stopping the active turn before sending the pending message…");
                return;
            }
            let index = self.queue.items.len();
            let mut followup = crate::attachments::Message::decode(&original_prompt);
            followup.text = line;
            self.queue.items.push(followup.encode());
            let notice = self.force_queued(index);
            if self.queue.force_pending {
                self.editor.clear();
                self.images.draft.clear();
                self.clear_selections();
                self.persist_draft_now();
            } else {
                self.queue.items.remove(index);
            }
            self.flash(notice);
            return;
        }
        self.busy_send_confirmation = None;
        if !self.busy {
            self.cancel_requested = false;
        }
        if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Update) && !line.starts_with('/') {
            self.push_system("an update is in progress - Esc cancels it, or wait for it to finish");
            return;
        }
        if !self.busy
            && (!line.starts_with('/') || user_prompt.is_some())
            && self.reference_journal.backlogged()
        {
            self.flash("Finishing history references; your draft is kept.");
            return;
        }
        let dispatch = if line.starts_with('/') && user_prompt.is_none() {
            line.clone()
        } else {
            crate::attachments::Message {
                text: line.clone(),
                attachments: std::mem::take(&mut self.images.draft),
                invalid: None,
            }
            .encode()
        };
        self.remember_prompt(&dispatch);
        self.editor.clear();
        self.clear_selections();
        self.persist_draft_now();
        let was_busy = self.busy;
        if line != "/config" && !line.starts_with("/config ") {
            landing::close(self);
        }
        self.run_line(dispatch, turn_tx);
        if !was_busy && self.busy && self.submitted_prompt.is_some() {
            self.submitted_prompt = Some(original_prompt);
        }
    }

    /// Run a prompt that has left the box: a slash command, or a turn. Typed prompts and queued
    /// ones both come through here, so trust mode, account and provider apply at send time.
    pub(in crate::tui) fn run_line(
        &mut self,
        line: String,
        turn_tx: &mpsc::UnboundedSender<String>,
    ) {
        let original_line = line;
        let incoming = crate::attachments::Message::decode(&original_line);
        if let Err(error) = incoming.validate() {
            self.restore_composer(&original_line);
            self.persist_draft_now();
            self.flash(error);
            return;
        }
        if incoming.text.starts_with('/') && !incoming.attachments.is_empty() {
            self.restore_composer(&original_line);
            self.persist_draft_now();
            self.flash("Keep attachments with a message; commands leave the draft intact");
            return;
        }
        if incoming.has_pdfs() && self.provider != ProviderKind::Claude {
            self.restore_composer(&original_line);
            self.persist_draft_now();
            self.flash("Native PDF attachments require Claude; select Claude before sending. The draft is retained");
            return;
        }
        let line = incoming.text.clone();
        if let Some(arg) = line
            .strip_prefix("/attach")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.image_command(arg);
            return;
        }
        if !line.starts_with('/') {
            self.questions.clear();
        }
        if let Some(arg) = line
            .strip_prefix("/continue")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            if arg.trim().is_empty() {
                self.show_unfinished_request();
                return;
            }
            if self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.kind == PickerKind::Continuation)
            {
                self.dismiss_picker();
            }
            self.continue_usage(arg.trim(), turn_tx);
            return;
        }
        if let Some(arg) = line
            .strip_prefix("/worktree")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.managed_workspace_command(arg.trim());
            return;
        }
        if let Some(arg) = line
            .strip_prefix("/scope")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.scope_command(arg.trim());
            return;
        }
        if let Some(arg) = line
            .strip_prefix("/continuation")
            .filter(|arg| arg.is_empty() || arg.starts_with(' '))
        {
            self.continuation_command(arg.trim());
            return;
        }
        if self.session_ui.loading() && !line.starts_with('/') {
            self.flash("recent messages are loading; wait before sending");
            if self.editor.text().is_empty() {
                self.editor.set_text(&line);
            }
            return;
        }
        self.view.follow_tail();
        // A user command shows as typed and sends its rendered template as an ordinary turn.
        let mut send = line.clone();
        if let Some(prompt) = crate::user_commands::expand(&self.user_cmds, &line) {
            send = prompt;
        } else if self.handle_slash(&line) {
            self.flush_delegate_prompt(turn_tx);
            return;
        }
        let selected_account = get_use_selection(&self.state_root).is_some_and(|selection| {
            selection.provider == self.provider && selection.account == self.account
        });
        let signed_out_key = format!("{}:{}", self.provider.as_str(), self.account);
        if !line.starts_with('/')
            && selected_account
            && self
                .home_path
                .as_ref()
                .is_some_and(|home| !is_logged_in(self.provider, home))
            && !self.signed_out_overrides.contains(&signed_out_key)
        {
            let confirmed = self
                .signed_out_confirmation
                .as_ref()
                .is_some_and(|(key, prompt)| key == &signed_out_key && prompt == &original_line);
            if !confirmed {
                self.signed_out_confirmation = Some((signed_out_key, original_line.clone()));
                self.push_system(format!(
                    "{} looks signed out. Use /use to sign in, or press Enter again to send anyway.",
                    self.account_label()
                ));
                self.restore_composer(&original_line);
                self.persist_draft_now();
                return;
            }
            self.signed_out_confirmation = None;
            self.signed_out_overrides.insert(signed_out_key);
        } else if !line.starts_with('/') {
            self.signed_out_confirmation = None;
        }
        let handoff = self.continuation.dispatch_request().is_some();
        let request = self
            .continuation
            .dispatch_request()
            .unwrap_or(&original_line)
            .to_owned();
        self.submitted_prompt = Some(request.clone());
        self.continuation
            .begin(&self.account, &self.model, &original_line);
        self.freeze_continuation_routes();
        if let Err(error) = self.bind_main_scope() {
            self.push_system(format!(
                "workspace scope not approved: {error}; request kept"
            ));
            self.restore_composer(&request);
            self.submitted_prompt = None;
            return;
        }
        self.last_sent = Some(request.clone());
        self.ensure_session_title(&line);
        if !line.starts_with('/') && self.task_id.is_none() {
            self.task_id = Some(uuid::Uuid::new_v4().to_string());
        }
        if !self.save_turn_checkpoint(false) {
            self.restore_composer(&request);
            self.submitted_prompt = None;
            self.persist_draft_now();
            return;
        }
        if !handoff {
            self.push_user(incoming.display());
        }
        self.turn_started = Some(Instant::now());
        let feed = crate::feed::FeedWriter::new(
            &self.state_root,
            self.thread_id().unwrap_or("main"),
            crate::config::for_root(&self.state_root).feed.enabled,
        );
        let _ = feed.emit("turn_start", &request);
        self.phase = Phase::Waiting;
        self.live_thinking.clear();
        self.busy = true;
        self.pulse_on = true;
        let _ = turn_tx.send(
            crate::attachments::Message {
                text: send,
                attachments: incoming.attachments,
                invalid: None,
            }
            .encode(),
        );
    }

    pub(in crate::tui) fn request_cancel(&mut self) {
        self.questions.cancel_native();
        self.native_owner = None;
        if self.cancel_requested {
            return;
        }
        self.cancel_requested |= self.submitted_prompt.is_some();
        self.submitted_prompt = None;
        self.pause_queue();
        if let Some(cancel) = &self.preparing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(h) = &self.cancel_handle {
            h.cancel();
            self.push_system("cancelling...");
        } else if self.cancel_requested {
            // Keep the active turn busy until dispatch observes this flag or a handle arrives.
            self.push_system("cancelling...");
        } else {
            self.busy = false;
            self.push_system("cancelled");
        }
    }

    /// Record the process handle and stop it immediately if cancellation raced with startup.
    pub(in crate::tui) fn accept_cancel_handle(&mut self, handle: Arc<RunningTurn>) {
        if self.cancel_requested {
            handle.cancel();
        }
        self.cancel_handle = Some(handle);
    }

    /// Consume a queued turn without starting a provider after an early cancellation request.
    pub(in crate::tui) fn discard_cancelled_pending_turn(&mut self) -> bool {
        if !self.cancel_requested {
            return false;
        }
        self.finalize_turn(TurnOutcome {
            cancelled: true,
            ..Default::default()
        });
        self.native_accounting_owner = None;
        self.busy = false;
        self.pulse_on = false;
        self.cancel_handle = None;
        self.cancel_requested = false;
        true
    }
}
impl App {
    pub(in crate::tui) fn new(state_root: PathBuf, cwd: PathBuf) -> App {
        let related_files = related_files::State::new(&state_root, &cwd);
        let state_root_for_usage = state_root.clone();
        let state_root_for_cmds = state_root.clone();
        App {
            appearance: appearance::State::load(&state_root),
            continuation: continuation::State::new(&state_root),
            continuation_target: None,
            state_root,
            cwd,
            account_names: Default::default(),
            account_email: None,
            header_email_revealed: false,
            header_email_pressed: false,
            header_home_pressed: false,
            permissions_pressed: false,
            permission_rows: Vec::new(),
            permission_row_pressed: None,
            card_header_pressed: None,
            header_credits_pressed: false,
            messages: vec![ChatLine {
                role: MsgRole::System,
                text: "Type a message and press Enter. Start with / for commands. Drag to select; Ctrl+C copies; Esc interrupts."
                    .into(),
                card: None,
            meta: None,
            }],
            idle_notice: None,
            handoff_after_cancel: false,
            editor: Editor::default(),
            submitted_prompt: None,
            expanded_activity: false,
            activity: activity::Activity::default(),
            palette: Palette::default(),
            key_sheet: false,
            chatref: chatref::ChatRef::default(),
            related_files,
            questions: questions::State::default(),
            native_owner: None,
            native_usage_model: None,
            native_usage_session: None,
            native_accounting_owner: None,
            scopes: workspace_scope::State::default(),
            managed_workspaces: managed_workspaces::State::default(),
            project_search: workspace_search::State::default(),
            file_action_target: None,
            images: images::State::default(),
            raster: raster::State::default(),
            history: Vec::new(),
            hist_pos: None,
            hist_draft: String::new(),
            parked_draft: None,
            fresh_draft: false,
            quit_armed: None,
            busy_send_confirmation: None,
            full_mode_confirmed: false,
            input_scroll: 0,
            input_scroll_anchor: None,
            view: TranscriptView::default(),
            areas: Areas::default(),
            drag: Drag::None,
            file_jobs: Vec::new(),
            workspace: workspace::Workspace::default(),
            file_click: None,
            #[cfg(test)]
            last_file_open: None,
            clicks: ClickTracker::default(),
            modal_close_pressed: None,
            flash: None,
            busy: false,
            pulse_on: false,
            cancel_requested: false,
            tick_count: 0,
            should_quit: false,
            provider: ProviderKind::Codex,
            account: "codex".into(),
            home_path: None,
            model: "?".into(),
            trust: TrustMode::Workspace,
            native_session_id: None,
            task_id: None,
            session_chip: None,
            session_ui: session_ui::State::default(),
            native_agents: native_agents::State::default(),
            footer_left: FOOTER_HINT.into(),
            picker: None,
            doctor: None,
            doctor_applier: Box::new(crate::doctor::DoctorAppliers),
            model_refresh: None,
            token_reads: tokens::State::default(),
            integrations: integrations::State::default(),
            artifacts: artifacts::State::default(),
            landing: None,
            update_preview: None,
            update_rows: 0,
            update_view: 0,
            cancel_handle: None,
            preparing_cancel: None,
            native_sources: Vec::new(),
            prior_provider: ProviderKind::Codex,
            prior_home: None,
            need_context_pack: false,
            task_map: None,
            thread: crate::threads::start(&state_root_for_usage),
            reference_journal: reference_journal::State::default(),
            thread_meta_done: false,
            usage_summary: "limits unknown".into(),
            usage_credits: "Credits: not reported".into(),
            usage_reset: None,
            usage_tone: UsageTone::Mute,
            usage_warning: None,
            usage_fetcher: UsageFetcher::new(state_root_for_usage.clone()),
            usage_block: None,
            stats: appearance::State::load(&state_root_for_usage).ui.stats.then(crate::sysstats::StatsHandle::start),
            compat_rx: None,
            picker_tones: HashMap::new(),
            live_cards: Vec::new(),
            live_text: String::new(),
            streamed_text_committed: false,
            live_thinking: String::new(),
            phase: Phase::Waiting,
            job: None,
            no_color: crate::tui::theme::env_no_color(),
            turn_started: None,
            turn_usage: Default::default(),
            session_input: 0,
            session_output: 0,
            session_cost: 0.0,
            idle_ticks: 0,
            tip_seed: tips::launch_seed(),
            tip_limiter: tips::RateLimiter::new(3),
            pending_login: None,
            connect: Box::new(add_account),
            reexec_path: None,
            deleg: workers::Panel::default(),
            worker_panel_open: false,
            worker_panel_explicit: crate::store::get_settings(&state_root_for_cmds).worker_panel_open,
            tab_strip: Vec::new(),
            tab_index: 0,
            tab_hits: Vec::new(),
            tab_close_hits: Vec::new(),
            rename_active: false,
            user_cmds: crate::user_commands::scan(&state_root_for_cmds).commands,
            queue: queue::Queue::default(),
            last_sent: None,
            keep: persist::Keep::new(&state_root_for_cmds),
            signed_out_confirmation: None,
            signed_out_overrides: HashSet::new(),
            needs_you: crate::needs_you::Queue::default(),
        }
    }
}
pub(in crate::tui) fn select_account(state_root: &Path, account_id: &str) -> anyhow::Result<()> {
    let Some(acct) = get_account(state_root, account_id) else {
        anyhow::bail!("unknown account {account_id}");
    };
    set_use_selection(
        state_root,
        UseSelection {
            provider: acct.provider,
            account: acct.account,
            home_path: acct
                .home_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            model: None,
            updated_at: now_stamp(),
        },
    )
}

pub(in crate::tui) const TYPED_MODEL_PREFIX: &str = "typed-model:";

pub(in crate::tui) fn model_source_tag(source: crate::models::ModelSource) -> &'static str {
    match source {
        crate::models::ModelSource::ListedByCli => "reported by CLI",
        crate::models::ModelSource::Cached => "cached",
        crate::models::ModelSource::Catalog => "catalog",
        crate::models::ModelSource::Typed => "typed",
    }
}

pub(in crate::tui) fn model_picker_rows(
    models: Vec<crate::models::ModelEntry>,
    chosen: Option<&str>,
) -> Vec<PickerRow> {
    let mut rows = vec![PickerRow {
        id: "model:".into(),
        label: format!(
            "provider default{}",
            if chosen.is_none() { " · current" } else { "" }
        ),
        detail: "the CLI picks its own default model".into(),
    }];
    let mut families = std::collections::BTreeMap::<String, Vec<crate::models::ModelEntry>>::new();
    for model in models {
        families
            .entry(model.family.clone())
            .or_default()
            .push(model);
    }
    for (family, mut entries) in families {
        entries.sort_by_key(|m| (m.deprecated, m.display_name.to_ascii_lowercase()));
        rows.push(PickerRow {
            id: format!("model-group:{family}"),
            label: family,
            detail: String::new(),
        });
        for model in entries {
            let current = chosen == Some(model.id.as_str());
            rows.push(PickerRow {
                id: format!("model:{}", model.id),
                label: format!(
                    "{}{}",
                    model.display_name,
                    if current { " · current" } else { "" }
                ),
                detail: format!(
                    "{} · {}{}",
                    model.id,
                    model_source_tag(model.source),
                    if model.deprecated {
                        " · deprecated"
                    } else {
                        ""
                    }
                ),
            });
        }
    }
    rows
}

/// Typed model ids become one `--model` argv entry; keep them to model-id characters.
pub(in crate::tui) fn valid_model_id(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 100
        && !model.contains("..")
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:/[]".contains(c))
}

pub(in crate::tui) fn set_model(state_root: &Path, model: &str) -> anyhow::Result<()> {
    let mut sel = get_use_selection(state_root).ok_or_else(|| anyhow::anyhow!("no selection"))?;
    sel.model = Some(model.to_string()).filter(|m| !m.is_empty());
    sel.updated_at = now_stamp();
    set_use_selection(state_root, sel)
}

pub(in crate::tui) fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", dur.as_secs(), dur.subsec_millis())
}
