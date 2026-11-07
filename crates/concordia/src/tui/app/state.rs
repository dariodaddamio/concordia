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

pub(in crate::tui) struct App {
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
    pub(in crate::tui) user_cmds: Vec<crate::user_commands::UserCommand>,
    pub(in crate::tui) queue: queue::Queue,
    pub(in crate::tui) last_sent: Option<String>,
    pub(in crate::tui) keep: persist::Keep,
    pub(in crate::tui) signed_out_confirmation: Option<(String, String)>,
    pub(in crate::tui) signed_out_overrides: HashSet<String>,
    pub(in crate::tui) needs_you: crate::needs_you::Queue,
}
