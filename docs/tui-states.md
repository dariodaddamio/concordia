# UI state ownership

Concordia does not have one application-wide state enum. It has cooperating state machines,
including implicit machines represented by options, booleans, channel ownership and persisted
records. This inventory maps their owners and important transitions. It is **not a formal proof
that every combination of fields is reachable, safe or tested**, and does not establish that all
state machines in the repository have been enumerated.

Paths below are relative to [`crates/concordia/src`](../crates/concordia/src). The detailed lifecycle
and coupling tables were checked against the current source on 2026-11-09. The secondary-owner
inventory identifies concrete source boundaries; it does not imply exhaustive transition coverage
inside every subsystem. Source remains authoritative, including changes under qualification.

## Runtime and conversation ownership

### Native-window journey control

Journey tests use the native Windows backend when `CONCORDIA_TEST_CONTROL` is set to
`<loopback-address>;<random-token>`. The backend binds only for that opt-in process and
authenticates every line-delimited JSON request with the token. `key`, `text`, `mouse` and
`resize` requests are posted to the window message thread; keys call the same event emitter and
mouse requests call the same cell-coordinate conversion used by Win32 messages. `snapshot`
returns the backend's ratatui cell grid, title, focused tab label and current picker/workspace
overlays. The channel is absent, has no listener and adds no work when the environment variable
is not present. It is a test driver, not a second renderer or a provider transport.

### Windows status and background attention

The native window applies the process-wide status projection to the Windows taskbar through
[`ITaskbarList3::SetOverlayIcon`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-itaskbarlist3-setoverlayicon).
Working uses the current theme success colour; Needs attention uses the current theme warning
colour and wins over working; idle clears the overlay. Icons are generated in memory and owned by
the native window thread.

When Needs attention arrives while the window is unfocused, Concordia uses the unpackaged desktop
shell notification API
[`Shell_NotifyIconW`](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw)
with an `NIF_INFO` balloon. This API does not require MSIX packaging or a package identity. A
notification click foregrounds the existing window and opens the existing Needs attention picker;
Concordia never reopens the window or foregrounds it without that click. The picker and the source
conversation read the same request record, so answering in either path resolves the same native
request.

| Boundary and source | States / events | Guard and completion owner | Persistence |
| --- | --- | --- | --- |
| Tab double click: `tui/run.rs` | No prior click; first tab-label click; second click on the same tab/cell within the OS double-click interval; reset | Close targets take precedence. Keyboard input, non-tab clicks and structural tab actions reset the tracker. A double click selects the exact tab, hides its shell view and invokes the existing guarded F2 title editor. | Transient click tracking is not persisted; saved titles use the existing conversation/native binding. |
| Tab runner: `tui/run.rs`, `tui/tabs.rs` | Open, selected, inactive, closed-but-retained, reopened, evicted; new/select/close/reopen/exit | Each `RunTab` owns App and its own turn/handle/progress/completion channels. Switching tabs does not transfer a run. Shutdown cancels preparation, foreground and workers and stops the local shell. Closed eviction waits for channels, pending completion and App work to clear. | `tui/window_session.rs` saves bounded open order, active index, stable tab IDs, pane recovery identity, exact provider/account/home/session/source binding and a display snapshot. |
| Foreground admission: `tui/app/state.rs`, `tui/app/turn.rs`, `tui/run.rs` | Idle; preparing; executing; cancellation requested; completion received but progress still draining; finalized | `busy`, `preparing_cancel`, `cancel_handle`, `cancel_requested` and `RunTab.pending_done` cooperate. Cancellation is a request, not proof that the process exited. Runner finalizes only after preceding progress is consumed. | Draft/recovery and unfinished checkpoint persist intent; process handles and live channels do not survive restart. |
| Activity: `tui/app/state.rs`, `tui/app/turn.rs` | `Phase::{Waiting, Thinking, Tool, Writing}`; provider progress and timer ticks | Presentation only. Phase, spinner and elapsed time cannot admit, cancel, retry or prove completion. | A saved transcript is a bounded projection, not executable state. |
| Native response: `provider/interaction.rs`, `tui/questions.rs` | Pending, answered, invalidated; queued native requests advance into the visible panel | Response uses the exact owned request/envelope and native capability. Conversation/run/attempt/session identity is authority; focus and card title are not. Answer delivery is once-only; stale or wrong-owner responses are rejected. | Native tool/approval handles are not transferred by memory maps or reconstructed from rendered cards. |
| Workers: `delegate.rs`, `delegate_recovery.rs`, `tui/workers.rs`, `tui/worker_input.rs` | `Queued`, `Starting`, `Thinking`, `Tool`, `Writing`, `WaitingInput`, `Done`, `Failed`, `Cancelled`, `Paused`; submit/start/progress/input/cancel/result/restart | Scheduler and owned attempt decide execution. `WaitingInput` still holds a worker slot. `Paused` is finished for scheduling, but does not establish successful effects. Worker input review validates job/run/attempt/request ownership. | Recovery ledger preserves job/run/workspace/result references; uncertain interrupted effects require reconciliation rather than replay of native input. |
| Workspace reservations and managed Git work: `workspaces.rs`, `managed_workspaces.rs`, `managed_lifecycle.rs`, `tui/managed_workspaces.rs` | Admission/reservation/release; review pending; preview receipt; explicit execute; success/error/cancel | Cooperative reservation owner and exact reviewed lifecycle receipt. Preview does not execute. User/native Git actions remain outside cooperative enforcement. | Registry and lifecycle records survive restart; transient review job/cancellation flag does not authorize a later action by itself. |

The distinction between completion arrival and finalization matters: an outcome can be waiting in
`pending_done` while the tab is still busy. Closing or reopening a tab must keep that outcome and
its preceding progress together, without starting another queued request during shutdown.

## Drafts, queue and continuation

| Boundary and source | States / transitions | Guard / effects | Durable meaning |
| --- | --- | --- | --- |
| Composer: `tui/app/state.rs`, `tui/input_dispatch.rs`, `tui/images.rs` | Independent draft, submitted prompt, parked draft, attachment import, temporary editor ownership | A submitted task is separate from new typing. Rejected expansion/admission keeps the original attachment envelope and draft. Rename and queue editing save/restore composer ownership. | Pane recovery records draft, queue and edit data; importing or partially prepared content is not proof of submission. |
| Queue: `tui/queue.rs` | `Dispatch::{Idle, Automatic, Explicit}` plus items, paused, focus and edit | Clean completion arms Automatic. Failure/cancel clears automatic intent and pauses items. Automatic dispatch requires idle, unpaused, no queue editor/history load and ordinary admission. Explicit selected send intentionally may cancel a foreground turn; it is a separate user action. | Queue contents, pause/edit recovery belong to the pane. An in-memory dispatch enum is not permission to replay work after restart. |
| Queue rejection: `dispatch_queued` | Remove candidate, attempt normal submission, reject/retain | Rejected submission reinserts the queued request, restores the independent composer and pauses the queue. Empty queue clamping resets selection, focus, edit and pause. | Rejected work remains recoverable rather than being silently consumed. |
| Main continuation: `tui/continuation.rs`, `routing.rs` | `Retry::{Ready, Deferred, RequiresReview}`; begin, bounded target selection, deferred resume, invalidate, recover | Eligibility is distinct from `automatic`, `dispatching`, `recovered`, `context_limited` and rollover count. Review cannot be waived by toggling automatic handoff or closing a picker. Uncertain tool effects, unresolved workspace reconciliation or unproven exit prevent automatic retry. | Run/checkpoint provenance, exact account/home/session and routing approval constrain recovery. Restart does not prove earlier effects completed. |
| Account/model switch: `context.rs`, `tui/app/pickers.rs`, `tui/app/login.rs`, `tui/app/state.rs` | Selected binding, pending selection, continuity decision, applied or rejected | Account/provider/home determine exact resume versus a new native session with bounded context. Active work blocks destructive account removal. Duplicate login identity is not authority to delete an existing home. | Store settings and native-source bindings persist. Removing the final account leaves dispatch blocked pending an explicit valid selection. |

### Clarifications are two different transitions

`questions::choose` sends a genuine native request through `respond_owned` and does not enqueue a
replacement prompt. A conversational card inferred from rendered `request_user_input_async` or
AskUserQuestion content has no invented native response channel. Its answer is appended in FIFO
order as an ordinary follow-up, preserving the current task, continuation, independent composer
and existing pause state.

Answering that inferred panel no longer calls `force_queued` or `request_cancel`. While busy the
answer waits. After a clean foreground finish it follows normal queue admission; an already paused
queue remains paused until resumed. This cannot unblock a provider waiting for a native response
that its transport never exposed. The correction establishes Concordia's local behavior; it does
not prove the cause of an external host's cancellation without the corresponding receipt.

## Loading, controls and derived views

| Boundary and source | State / event / owner | Guard and terminal transition | Persistence |
| --- | --- | --- | --- |
| History and memory view: `tui/session_ui.rs` | Generation `next/pending`, background jobs, separate memory pending/screen generation | Results are checked against generation and provider/account/home/expected session. Invalidation removes display authority even if a read worker is still finishing. Success applies bounded history; stale/error/disconnected results cannot substitute another chat. | Exact native source remains authoritative; omitted display content is explicitly marked. |
| Rename: `tui/input_dispatch.rs`, `tui/session_ui.rs` | `rename_editor: Option<Editor>` owns an independent inline tab title editor | Commit persists title; Enter saves and Esc cancels. The composer remains untouched and visible; paste targets only the title while renaming. | Title and recoverable independent draft have separate records. |
| Input precedence: `tui/input.rs`, `tui/input_dispatch.rs`, `tui/modal.rs`, `tui/draw/mod.rs` | Local shell, modal/picker/editor, Home, queue, composer; mouse pressed targets and drag state | Input and rendering precedence choose an owner. Multiple stored options are not automatically mutually exclusive. Closing an overlay must not accidentally submit its content. | Most focus, pressed rectangles and drag fields are transient. |
| Home: `tui/landing.rs`, `tui/run.rs` | Landing present/absent; grouped chats, goals, open work, return origin | Home navigation uses a stable originating tab ID; selected recent chat retains its explicit source binding. Landing visibility does not make an active run idle. | Window snapshot can record Home visibility and origin; current native sources must still validate. |
| Files and editing: `tui/workspace.rs`, `tui/workspace_edit.rs`, `tui/file_editor.rs` | Loading, generation, dirty/changed, search, selecting, saving, confirmation/action prompt, attached/detached pane | File snapshots check before/after stamps. Reload/save and close prompts preserve dirty work and explicit target identity. Background result generation cannot silently replace another pane's document. | Explicit save updates the file; recovery/edits do not authorize an automatic save over changed disk content. |
| Attachments and raster: `tui/images.rs`, `tui/raster.rs`, `tui/related_files.rs` | Import jobs, ready attachments, selected preview/editor, close/save/cancel | Immutable attachment snapshots and explicit target govern dispatch; raster focus has its own input owner. Failure returns to recoverable composition. | Attachment envelope and file identity are distinct from transient preview/selection. |
| Artifact projection: `tui/artifacts.rs`, `tui/artifacts_inline.rs` | List/detail/activity/inline jobs pending, records/selection available | Conversation/source keys bind results. Projection-pending checks include every job before idle pruning; idle derived state can be cleared without deleting original artifacts. | Native artifact metadata/originals remain; displayed cards are derived projections. |
| Login and metadata jobs: `tui/app/login.rs`, `tui/app/state.rs`, `tui/tokens.rs`, `tui/integrations.rs` | Pending receiver/job, completed result, error/disconnected, invalidated selection | Polling applies an appropriate current binding. Model refresh is an explicit Result and validates selected home; failure cannot claim a fresh successful catalog. Doctor/update jobs have their own explicit preview/apply controls. | Registered account/config/cache observations persist; pending receivers do not. |
| Display and auxiliary controls: `tui/appearance.rs`, `tui/settings.rs`, `tui/activity.rs`, `tui/chatref.rs`, `tui/native_agents.rs`, `tui/reference_journal.rs` | Open/focus/selection, cached projection or saved preference | These are separate owners, not foreground execution authority. Their internal transition products are not exhaustively enumerated here. | Settings/references may persist; screen geometry, presses, selection and most job state are transient. |

## Local terminal

`tui/local_terminal.rs` owns one explicitly opened local shell per tab. `View::{Chat, Shell}` is
independent of `session: Option<Session>` and `Phase::{Running, Exited}`. Shell bytes never enter
provider prompt or progress channels.

| Combination / event | Behavior |
| --- | --- |
| Chat + no session; open/toggle | Start shell in the tab workspace and show it; launch failure can show Shell with no session and a notice. |
| Shell + Running; hide/toggle | Return to chat while the same shell continues. Hidden tabs/shells are still polled. |
| Chat + existing session; open | Show the existing Running or Exited session; opening does not implicitly restart it. |
| Running; process exits | Record exit status, close UI input and kill remaining owned descendants. Retain the final screen until close. |
| Shell + Exited | Input is not sent. Hide and close remain local navigation/actions. |
| Any session; close/tab shutdown | Stop shell and process tree, discard transient terminal view; no provider cancellation is implied by shell keystrokes. |
| Pending input below bound | Retain FIFO bytes and retry bounded channel delivery. |
| Input retention limit exceeded | Refuse further ordinary input, including Enter; Ctrl+C clears pending local input and clears refusal. This cannot retract bytes already written to the OS. |
| Oversized paste | Reject the entire paste visibly; text paste strips control characters except tab/newline and honors observed bracketed-paste mode. |

Output uses a bounded queue, fixed read chunks, bounded parser control-sequence storage and 2000
scrollback rows. A separate zero-scrollback parser in the reader is the sole protocol responder;
shared resize dimensions keep it aligned. Status replies have a separate bounded priority channel.
On UI receiver closure the reader continues parsing and draining/discarding until EOF, with the
writer available for inherited-cursor/status replies while the master closes. This avoids coupling
normal ConPTY teardown to a dropped UI parser or full user-input queue.

Shutdown also has an explicit `closing` signal. The writer services the initial cursor query, then
closes the OS input pipe independently of reader EOF; waiting for reader EOF would form a teardown
cycle. If a platform never issues that query, shutdown waits at most one second before closing input.
The output reader keeps draining while the master closes. Constructor failures before pipe handles
can be extracted and OS failures remain separate qualification limits, not proof of successful cleanup.

Shell process/session, screen, focus and pending bytes are intentionally transient: a saved chat
layout does not resume arbitrary local commands after restart. OS handle-extraction failures and
pathological protocol-reply queue overflow remain explicit qualification limits; finite buffers
are not proof of complete terminal-emulator parity.

## Actual combinations and remaining holes

The following are real combinations supported or guarded by current source, not a proposal to
replace all fields with a giant enum.

| Combination | Meaning / guard |
| --- | --- |
| Busy + new composer text + queued replies | Valid independent work. Answering a clarification must not overwrite/cancel the submitted task. |
| Busy + native question | Valid waiting state; answer the owned request while keeping the foreground task busy. |
| Busy + no cancel handle yet | Valid preparation/handle-delivery interval; preparation cancellation flag covers it. `busy == handle.is_some()` would be a false invariant. |
| Cancellation requested + live process | Valid interval until owned completion; never treat the request as a successful exit. |
| Completion received + progress remaining | Valid runner state; finalization is deferred until prior progress drains. |
| Automatic dispatch + paused queue | Valid after clean finish of a task while an independently paused queue exists; pause still blocks sending. |
| Explicit dispatch + busy | Intentional selected-send action stopping the current run; inferred clarification replies must never enter this state. |
| Queue edit + attachment state | Queue editor temporarily owns its attachments; close restores the separate composer images. Dispatch waits for the editor to close. |
| Retry Deferred + open local control | Resume only after the control closes and normal admission passes. RequiresReview is a separate retry variant. |
| History invalidated + old jobs still running | Result completion is not display authority; generation and identity checks remain required. |
| Closed retained tab + pending completion | Must preserve progress/outcome ownership until reopen/final reconciliation; cannot evict merely because it is invisible. |
| Chat view + Running shell | Shell remains active and drains output; chat is not the shell process state. |

Remaining audit obligations are cross-owner combinations: overlay precedence while late jobs
complete, tab closure during preparation/completion/terminal startup, queue pause versus deferred
continuation, file-edit prompts during account/session transitions, and recovery after uncertain
worker effects. Source guards and focused regression receipts cover specific paths; there is no
exhaustive reachability graph, generated transition specification or formal proof for the full
product. Arbitrary combinations of all App flags have not been tested. This document therefore
answers where the machines live, not that every possible implicit machine has been eliminated.

## Unused features versus removable implementations

Chat opening now queues an exact resume request for the tab runner. The runner focuses an existing
owner before mutating any App, or claims the native chat before opening a new tab. Conversation
and native-chat file leases prevent simultaneous ownership across windows using the same state
directory. Closed tabs reacquire ownership before reopening. Provider per-turn writer locks remain
the execution guard; view ownership does not authorize dispatch.

Window restoration reads the saved layout and also inspects unclaimed unfinished request records.
It appends at most eight eligible paused requests without changing the selected tab. Duplicate
saved chat snapshots remain in the canonical window record, with their recovery records available
through automatic tab restoration. First-send checkpoints include conversation identity even without a context
map; the original request is persisted before the cleared composer is saved. Restoring either
record never replays uncertain actions.

No usage telemetry is available here. A feature with few visible callers or an optional UI route
cannot be called unused by users. Removal needs a demonstrable source fact: no production
references, superseded implementation, unsupported interface, or dead persisted shape with a
reviewed migration contract.

The current cleanup removed concrete implementation redundancy: static selectable model catalogs
and cross-account model unions, speculative provider cache readers, silent capability/argument
rewrites and plaintext output downgrade. Those were redundant or unsupported paths, not evidence
that model selection, provider tools or recovery were unwanted features. Existing source-backed
model observations, explicit typed model IDs and native defaults remain distinct supported roles.
No additional whole user-facing feature is proven removable by this state inventory. Remaining
coupled fields are refactoring candidates only after inspecting every read/write and recovery
boundary; deleting them because they are booleans or test-only helpers would not establish safety.

Behavior and physical-host limits: [chat rendering](chat-rendering.md),
[current verification](e2e-status.md). [Architecture](architecture.md) maps the other owners.

Shared setup uses the existing connection store: a project connection records inheritance intent
separately from each account's result; shared skill sources record global intent without copied
trees. An unconfigured account becomes pending before its next turn, then connected, failed or
preserved on a name collision. Adapter failure requires explicit retry; source drift stops adapter
propagation. Skill removal withdraws intent and matching links while retaining original files.
Setup commands run off the UI thread. Result delivery belongs to the originating account and
conversation, and setup/provider execution share one cancellation owner.

Native request identity is connection plus request ID, with session, method and complete parameters
fingerprinted. Identical retransmission retains one response channel, including after resolution;
changed content is rejected. Stored identity never contains a cached approval or replayable answer.
