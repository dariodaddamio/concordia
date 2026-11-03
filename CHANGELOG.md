# Changelog

- Native Codex questions can accept bounded text replies in foreground and worker review when
  offered by the protocol. F8 focuses the reply; paste and multiline input preserve the composer.
  Exact request ownership and native secret-input refusal remain enforced.

- Structured Codex file outputs have source-bound cards and a metadata catalog in the file browser.
  Inspect exact run/session/event provenance, then explicitly choose actions on the current file.
  Repeated events deduplicate; missing, replaced and excluded paths retain their source report.

- Image chips and image file previews open bounded inline color-cell rasters. Decode work stays
  off the input loop; Esc preserves the draft and E explicitly opens the system viewer.

- Agents can propose goal additions/closures through MCP for explicit full-text local review.
  F7 focuses goal choices without consuming the message draft. Revision-bound receipts and visible
  source attribution preserve monotonic completion and prevent replay of uncertain changes.

- Managed cross-worktree dependencies wait for explicit delivery review. Owners can accept only
  the verified result text or a matching completed integration receipt. Changed source/head/status
  evidence pauses dispatch, and pending dependants prevent premature worktree retirement.

- F10 opens a read-only integrations catalog without consuming the composer draft. Connected
  manifests show revision status, provider/home/project bindings and namespaced tools. Agents
  discover the same catalog through a bounded, paginated MCP tool without executing adapters.

- The usage screen offers a separate, paginated native-source coverage view with verified
  counter records and metadata-only checkpoints. Overlapping native snapshots are excluded from
  Concordia totals. Recorded totals now separate attempts, outcomes and full workspace paths.

- Context observations include exact account/home/session/turn/model and source revision evidence.
  Same-model new turns, malformed records and legacy records without turn ids invalidate capacity.
  Claude permission and Claude/Cursor capacity controls explain their native-interface blockers.

- Explicit UTF-8 document attachments preserve verified snapshots, mixed attachment order and
  queue/draft recovery. Pasted file paths stay text and offer an explicit F9 attach action.

- Unified file actions offer Preview here, Split off and Open in editor, with explicit defaults
  and native GUI-editor configuration. Independent Concordia file editors reuse pane saves and
  owned crash recovery without starting a provider.

- Project search supports selected literal replacements with exact previews, conflict/ownership
  preflight, serial editor saves and durable receipts. Ctrl+H inspects interrupted batches without replay.

- Opt-in Codex app-server text/image turns preserve exact thread/turn ownership, native approval
  scope and offered question answers. Pending choices queue and expire on native resolution;
  unsupported protocols stop visibly without fallback. Native subagents use the retained inspector.

- Managed worktrees now support separately reviewed native merge, HEAD cherry-pick, retirement
  and branch deletion. Receipts bind exact heads, paths, conflicts, worker references and leases;
  changed state, dirty files, unique commits or uncertain execution preserve the workspace.

- Worker-owned native input review and opt-in Cursor ACP text/plan decisions preserve main drafts,
  native scopes and exact run/attempt ownership. Recovery pauses unresolved requests.
- Edit cards show reported paths and readable additions/removals, with file navigation; JSON
  Windows paths and extensionless references resolve correctly. Tool labels and activity icons
  describe the operation, and Ctrl+G opens a persistent-results agent inspector at the side.
- Fix Down returning from the queue, idle quota refresh and worker quota invalidation. Preserve
  live Codex credit balances separately from subscription limits through cache/restart.
- Read-only MCP memory discovery replaces intricate user commands for agents. The bare slash menu
  focuses on everyday controls; advanced commands remain available by prefix.

### Validation
- A versioned synthetic retrieval corpus compares lexical and opt-in ranking through existing
  native-memory checks. `scripts/evaluate-memory.ps1` records exact source/test/corpus identity,
  relevance, verified excerpts, explicit incomplete coverage, read costs and measured resource bounds.
  It runs no model turns and changes no production ranking defaults.

### Fixed (`2026-11-03d`)
- Related-chat groups include the parent source-update artifact fix and current-workspace alias
  correction. Group membership, provenance and discovery controls retain their existing behavior.

### Fixed (`2026-11-03b`)
- Related-chat groups include the current-workspace alias correction from the parent memory
  ranking slice. Existing group membership and the landing-page shortcut remain unchanged.

### Added (`2026-11-02u`)
- Explicit related-chat groups: `/memory group-create SOURCE`, `groups`, `group-members GROUP`,
  `group-link GROUP SOURCE` and `group-unlink GROUP SOURCE`. Overlapping membership retains
  exact native source identities and detach provenance; stale members remain visibly unavailable.
- F6 on the landing page opens related work while preserving its selection and the draft.
  `/memory related-file`, `related-goal`, `related-at` and `related-from` show evidence-backed
  suggestions separately from confirmed groups. Context-link discovery can label cross-project scope.
- Relationship store v2 reads earlier goal associations unchanged and serializes group and goal
  changes under the same lock. Group discovery never starts Cursor native replay implicitly.

### Fixed (`2026-11-03c`)
- Source updates install the executable reported by Cargo for the selected Concordia manifest,
  including custom target directories. Missing artifact evidence or a mismatched source version
  retains the existing install. A staged install is verified again before replacing the old binary.

### Fixed (`2026-11-03a`)
- Goal-aware ranking resolves the explicitly selected current workspace before comparing its
  stored canonical association. Windows short paths and directory aliases retain confirmed-goal
  reasons; historical source paths are not resolved or opened.

### Added (`2026-11-02t`)
- Opt-in `/memory ranked` search explains verified goal, handoff, historical-file, literal-text
  and native-time signals. Context controls include `ranked-file`, `ranked-goal`, `ranked-from`,
  `ranked-at` and `ranked-source`; ordinary recall keeps its existing behavior.
- `/memory file-facts SOURCE [OFFSET REVISION]` exposes bounded historical paths from verified
  Codex/Claude native tool requests. It never opens referenced files or treats requested edits as
  completed work. CLI and TUI share validation, exact source pointers and snapshot pagination.

### Added (`2026-11-02s`)
- `/worktree create BASE`, `list`, `inspect ID`, `reconcile ID` and `cancel` manage explicit Git
  worktree creation from a clean committed source. Creation intent and exact identities survive
  interruption without automatic replay, integration or deletion.
- `/delegate worktree ID ACCOUNT TASK` binds a worker to a verified managed execution folder.
  Result returns identify unintegrated edits while the main agent retains its original cwd.
  Git worktrees organize files; native filesystem confinement remains unverified.
- Workspace registry v2 and worker ledger v6 preserve distinct owner/execution bindings and
  conservatively migrate earlier metadata. Unknown native processes still require reconciliation.

### Added (`2026-11-02r`)
- Main turns and workers reserve their execution folder across Concordia panes. Explicit
  `/scope set read:PATH write:PATH` and `/delegate scoped ACCOUNT read:PATH write:PATH -- TASK`
  declarations allow disjoint work; overlapping writes and stable reads wait or pause before launch.
- `/scope leases` and `/delegate scope [ID]` inspect bounded durable reservations. Uncertain
  native exits retain claims until explicit stopped-process reconciliation; recovery never replays work.
  These are cooperative scheduler claims, not filesystem sandboxes or managed Git worktrees.
- Internal file saves hold the same admission lock and retain unsaved buffers on ownership conflict.
  Worker ledger v5 and unfinished-request v3 preserve exact workspace/run/conversation bindings.

### Fixed (`2026-11-02q`)
- Main-turn completion retains its captured launch owner until usage is recorded. Failed or
  cancelled launches with no reported counters retain explicit unknown coverage instead of
  disappearing or losing attempt attribution.

### Added (`2026-11-02p`)
- Main continuation and workers share local route eligibility and explicit skipped-target reasons.
  `/continuation routes` and `/delegate routes [ID]` inspect candidates and captured failover decisions.
  Cached quota observations remain separate from unknown capacity; inspection makes no provider calls.
- `/continuation policy set ACCOUNT[:MODEL] ...` and `/delegate policy set ACCOUNT[:MODEL] ...`
  save ordered overrides for future tasks. `policy legacy` restores existing configured ordering.
  Approved tasks retain their sequence and account/home/login bindings across policy edits and recovery.
- Worker ledger v4 and unfinished-request v2 preserve routing snapshots, migrate earlier metadata
  without replay, and pause changed bindings. Main/worker retry caps and manual defaults remain intact.

### Added (`2026-11-02o`)
- Main and worker usage records bind native launch attempts to their run, account/home, model,
  workspace and reported counters. Repeated terminal snapshots do not accumulate twice. Failed,
  cancelled and interrupted attempts retain explicit coverage and unknown fields.
- The token view reads bounded pages in the background, groups main/worker runs separately,
  distinguishes equal native session IDs across accounts, and shows reported-cost coverage.
  N advances an exact snapshot page; R refreshes. Landing totals also load in the background.
- Worker ledger v3 preserves attempt accounting across recovery, migrating v1/v2 without replay.
  Resumed Claude cost is unknown when a restored session total cannot be separated from this attempt.

### Added (`2026-11-02n`)
- Explicit goal/source associations: `/memory goals`, `/memory goal-link GOAL SOURCE`,
  `/memory goal-sources GOAL` and `/memory goal-unlink GOAL SOURCE`. Links retain source identities
  and provenance without copying conversation bodies. Read-only MCP goal discovery uses the same service.
- Goal and relationship pagination binds later pages to the returned revision. Concurrent changes
  require restarting the listing; stale native pointers and corrupt metadata fail visibly.

### Added (`2026-11-02m`)
- `/delegate after ID[,ID] ACCOUNT TASK` approves a bounded task with exact prerequisite runs.
  Successful same-workspace prerequisites release it through the existing durable worker queue;
  failed/cancelled prerequisites block only their dependants. `/delegate graph` explains readiness.
- Dependency results are verified against native source snapshots before dispatch. Missing/stale
  sources pause visibly, and restart never replays queued dependency work automatically.

### Fixed (`2026-11-02l`)
- Filter native background notices from readable user history and session previews/titles.
- Show selection copy controls and clearer permission labels in the footer. Retain the newer
  focus repaint, word deletion, independent draft scrolling and draft-visible landing page.

### Changed (`2026-10-21d`)

### Added (`2026-11-02k`)
- Read-only project content search (`/search`, or Ctrl+F in the file browser), with literal/case
  controls, bounded cancellable scans, exact source hashes and draft-preserving input ownership.
- Worker dependency graph and recovery foundation. Metadata validation/readiness does not dispatch
  dependencies; interrupted work remains paused and only exact successful run receipts satisfy edges.
- Typed native interaction/capability foundation; existing Cursor ACP requests retain native option
  IDs/scopes and bind responses to their conversation, run, attempt and live connection. Cancelling
  or completing a turn invalidates its pending UI requests. Other adapters and worker approvals remain open.

- Compact tool rows foreground the command instead of PowerShell launcher paths, retain complete expanded output, and show output line counts. Completed reasoning is collapsed to one preview line.
- Terminal titles use the terminal API, including the Windows console path, and follow the generated session title.
- Usage-limit continuation defaults to manual (`/continue ACCOUNT`), with opt-in automatic handoff through configured accounts. Attempts are bounded, native session ids stay account-local, drafts and queued work are preserved, and uncertain tool results stop automatic handoff.

All notable changes to Concordia. The format follows [Keep a Changelog](https://keepachangelog.com/);
Concordia has not had a tagged release yet, so everything is under Unreleased. Each user-visible change
also bumps the build stamp shown by `concordia --version`.

## [Unreleased]

### Fixed (`2026-11-02j`)
- Enter closes a completed update screen when there is nothing to install, preserving the chat draft.
- CI uses a reviewed 20 MiB Windows artifact budget for the current native feature set.
- Disable unused Syntect grammar/theme import features, removing yaml-rust. Review the remaining
  bincode maintenance advisory narrowly for embedded assets; vulnerability checks remain enforced.

### Added (`2026-11-02i`)
- Durable pane-owned worker runs with explicit UUIDs, native session/source references, paused
  crash recovery, source inspection/reconciled retry, and reserved result delivery.
- Opt-in bounded worker quota failover before tools or partial output. Cancellation, uncertain
  exits and interrupted work stop automatic failover.

### Added (`2026-11-02h`)
- Per-account `/context` policy: optional Codex proactive rollover between turns from native
  context measurements; documented process-local Codex/Claude compaction controls. Unknown
  capacity stays unknown, and original-source verification is required for proactive handoff.

### Added (`2026-11-02g`)
- Bounded lexical native-history retrieval with exact excerpt pointers, confirmed links and
  observed context handoffs. `/memory` performs background reads and preserves drafts.

### Changed (`2026-11-02f`)
- Cursor native readback uses capability-checked ACP session loading with exact account/session binding. Pointer excerpts, whole-history pages within the bounded replay budget, CLI chat reads and resume scrollback use the same reader. Replay sends no prompt, starts no new session, rejects tool requests, preserves native auth in the selected config home, and persists only pointer metadata. Cursor pointers explicitly identify protocol replay rather than claiming local JSONL-file provenance.

### Changed (`2026-11-02e`)
- Native source indexing can scan whole Codex/Claude histories through immutable bounded pages, with resumable identity-checked cursors, snapshot-stable ordinals, explicit malformed/oversized-record gaps, and no stored conversation bodies. CLI and MCP range listings now page beyond the first twenty records; MCP can expand a known pointer into whole-history pages.
### Distribution
- Windows releases and rehearsals now require a trusted, timestamped signature matching the configured publisher and a clean Defender scan before packaging. Added local signing and verification scripts. Removed antivirus-exclusion installation advice; the detected private build remains blocked pending review and publisher-signing setup.

### Fixed (`2026-11-02d`)
- Ordinary status updates mentioning "select an option" no longer turn later feature bullets into unrelated reply choices. Conversational choices require an explicit closing question or invitation followed immediately by options; example JSON is interpreted only in question-tool cards.
- Conversational options never take keyboard focus automatically. Digits in a focused conversational reply are text, typing/pasting selects the note-only row, and note-only replies send exactly the user's text. Notes preserve pasted line breaks, support Shift+Enter, and survive Esc back to the composer. The complete assistant response stays in the transcript for context and long-option inspection; native approval shortcuts remain separate.

### Changed (`2026-11-02c`)
- `/use` adds a session handoff screen: pause and choose at a limit, select automatically, or prefer a specific connected provider account. `/continuation` opens the same screen. Session choices preserve saved defaults and never start a provider turn by themselves.
- Notes entered in the question picker's note row can start with a digit without accidentally selecting an option.

### Fixed (`2026-11-02b`)
- After a provider or account handoff, later turns resume the new native session instead of starting a fresh one each time and re-sending the handoff map.
- A model-at-capacity error (`server_overloaded`) is recognized and offers continuation on another model or account. The turn's own error now takes priority over the Codex rollout-recording stderr line it can cause.
- Question choices appear only for a closing question with top-level choices; list items that quote a question no longer become a picker, and choosing no longer sends half a sentence. Choices are numbered plainly, long ones end with an ellipsis, and a note row lets you send a note with a choice or on its own.
- File underlines cover only references to existing files, so words before a filename and other path-shaped prose are no longer underlined or clickable.

### Fixed (`2026-11-02a`)
- Pasted CR-only, CRLF and LF line endings retain their logical line breaks. Sending, queueing and editing queued prompts preserve leading indentation, blank lines and trailing spaces instead of trimming the message. Prompt tabs retain the existing four-space display policy; raw file editing retains tabs.
- Codex native rollout-recording failures are distinguished from quota exhaustion. The unfinished request is kept for explicit fresh-session continuation with source pointers; partial actions are not automatically replayed and native history is not removed or rewritten.

### Changed (`2026-11-01f`)
- Windows text clipboard actions use native Unicode APIs instead of starting PowerShell on the UI thread. Cut reports `cut`, preserves selected text if copying fails, and file-pane cuts share the feedback. Ordinary text paste skips image helpers.
- Active provider/worker output is checked every 33 ms instead of waiting for the 200 ms animation tick. Standard provider turns log setup/spawn/first-event/parser timings without content; live speed parity is not claimed.
- Workers can select a distinct native model from their own catalog with `/delegate model ACCOUNT MODEL`. Models appear in worker inspection and the agent browser. `/delegate collect` batches new results; opt-in `/delegate returns auto` returns finished batches once when idle, preserves drafts and binds delivery to the owning conversation.
- Worker result turns use normal checkpoints and quota recovery. Structured failures cannot become successful worker results through a zero process exit; unknown exits and interrupted tools are labelled honestly. Retained patch-verification diagnostics are inspectable without flooding chat or claiming an exit cause.
- Completed Mermaid blocks reuse their rows when earlier messages change. Bounded FIFO caching and per-message render limits prevent long diagram histories from cycling through background renders.

### Changed (`2026-11-01e`)
- Mermaid fences render locally as Unicode terminal diagrams in chat and formatted Markdown panes. Parsing/layout run on bounded background workers, with a small cache and source fallback; no Node/browser renderer or model call is started.
- Image attachments support clipboard paste, a file chooser (`/attach`), explicit paths and removable preview chips. Immutable content-addressed snapshots follow drafts, queues, recovery and quota handoffs. Native input uses Codex image-file arguments, Claude streaming image blocks, or Cursor ACP image blocks with capability negotiation.
- Cursor image turns bind exact sessions and answer native permission/multiple-choice requests through the UI. Unsupported client methods receive explicit protocol errors; capabilities are checked before starting a model turn.
- Completed error cards no longer count as still-running tools for quota recovery. Unsent drafts survive automatic handoff; open command screens/queued edits defer recovery until they close instead of permanently abandoning it. Error-only quota events, CamelCase quota codes and Claude error arrays are recognized.
- Timing/token metadata is excluded from file-link detection. Bare numeric measurements no longer become filename links. User-message backgrounds fill blank/trailing cells and use a subtler shade; paused composer hints share the normal prompt inset.
- Update screens show incoming/current changes by default, with earlier history behind H and restrained category colors.

### Changed (`2026-11-01d`)
- Conversational question choices support clicks, arrows/Enter and digits, with F8 returning to choices and Esc returning to typing. Replies preserve drafts and pause queued work. These are follow-up messages, not native approval RPC responses.
- `/open` and `@` include current-chat references, recent previews and bounded workspace discovery, with folder browsing, quoted absolute paths and relative paths. Canonical paths deduplicate slash variants; generated/ignored/sensitive files are excluded from discovery.
- Queued rows add Send now, focused Ctrl+Enter/Alt+Enter, and `/queue send [N]`. An active turn stops before one selected message starts; remaining messages stay paused.
- Run status, elapsed time and a styled clickable Esc control move below chat. Completed replies keep duration and stop time, user rows have a background highlight, Bottom disappears at the bottom, and command lists have consistent inner padding.
- Each `/usage` and `/use` requests fresh Codex limits through read-only `account/rateLimits/read`, bound to the selected home. Refresh/failure provenance and actual recorded reset dates remain visible; expired data never implies free capacity.
- Structured quota error codes survive generic/missing messages and can trigger bounded automatic continuation. Paused handoffs explain their guard rather than silently stopping.
- Verified stock Windows npm Codex launchers resolve to their unmodified native executable, avoiding the Node bridge. Unknown/custom bridge content falls back to normal launch; no unrelated processes are stopped.

### Changed (`2026-11-01c`)
- Markdown/JSON panes open formatted by default. Raw mode provides internal editing, explicit Ctrl+S save, undo/redo, selection and syntax highlighting; source code always opens in that editable view.
- Unsaved file close/reload/exit requires a save/discard/cancel decision. Byte-exact conflict checks protect changed files, and pane-owned recovery retains edits, cursor and layout across interruption.
- Headless provider processes use Windows `CREATE_NO_WINDOW`, matching background update children. Interactive sign-in remains a separate flow.
- Account continuation no longer echoes internal handoff instructions into chat or restores those instructions as the user's retry draft.
- Replaced the GIF menu tour with an edit/save workflow and a synthetic account-handoff demonstration, with larger text, cursor movement and click feedback.

### Changed (`2026-11-01b`)
- Command screens explain Esc/X and input ownership in the message box. Pickers, update/token screens and activity output have an X close button; closing an active update cancels that update, and drafts remain intact.
- The pinned collapse control includes the command preview and output count instead of only the tool name.
- Explicit GIF/image links open in the system viewer instead of a text pane. Added two reproducible GIF drafts driven by the actual renderer and input handlers with synthetic data and no provider calls.

### Changed (`2026-11-01a`)
- Chat and formatted Markdown file panes render pipe tables with wrapped cells or a narrow-pane labeled layout. Markdown links show clickable labels with explicit file or HTTP(S) destinations; file-pane links resolve relative to the document.
- The message box supports independent mouse-wheel and Alt+PageUp/PageDown scrolling. Redraws retain the chosen viewport; editing or moving the caret resumes caret following.
- Update progress has one owner: the update page while open, otherwise the footer. The message box shows a paused state instead of duplicating update progress.
- Documented the installed WezTerm 2024 build's pointer-shape limitation and added an implementation-gated X/LinkedIn launch plan.

### Changed (`2026-10-31t`)
- `/update` opens a full page that hides the chat, draft, queue and file panes until dismissed. Enter still confirms installation when available. Removed the redundant `/update check` command and completion.

### Changed (`2026-10-31s`)
- Added revision-checked native-source indexes and exact UTF-8 chunk readback for recent Codex/Claude JSONL history, available through `chat index|sources|excerpt` and MCP `native_read`. Handoffs can supply source pointers even with Concordia transcript recording off.
- Pane-owned unfinished requests preserve run/workspace/source identities separately from drafts and queues. Restart restores paused work for explicit continuation. Cancellation wins over a pending limit handoff, including cancellation during source preparation.
- Recognized terminal context-capacity errors may start one fresh native session on the same account. Proactive near-capacity rollover and compaction controls remain planned; unknown capacity is not guessed.
- Explicitly connected file-backed Harness manifests can supply exact pinned decisions for matching new-session launches, including workers, with manifest revision checking.
- File panes add `/` search, `n`/`N` match navigation, asynchronous `r` reload and a changed-on-disk indicator. Current-chat suggestions include a recovered unfinished request's file references.

### Changed (`2026-10-31r`)
- `/open` lists files mentioned in the current chat; `@` offers those files alongside recorded chats. Tab/Enter completes, Ctrl+P or a file-row click previews without sending the draft. Discovery is bounded, runs off the render loop and deduplicates canonical file identities.
- Identical completed tool replays no longer append duplicate thread-log events. Identified complete provider snapshots are deduplicated within a bounded per-turn cache; repeated deltas and distinct messages remain intact.
- Handoff maps omit duplicate first/latest request fields and nonadjacent repeated errors. File paths with hexadecimal prefixes no longer expand as chat references, and repeated identical missing-reference notices appear once. Audit scope and next planned features are recorded in docs/quality/duplication-audit.md.

### Changed (`2026-10-31q`)
- Every `/usage` and `/use` requests a fresh check; open account lists update in place and show reading provenance/refresh status. Codex re-reads the latest native files without claiming a server refresh.
- Internal file panes toggle raw/formatted Markdown and JSON with `v` or the view bar. `/preview` starts formatted; unsupported types stay raw. Resize dividers now request matching pointer shapes, including during dragging.
- The usage-handoff toggle persists. Automatic mode without an account preference list tries connected accounts and then native-catalog models, with at most three handoffs. Explicit model preferences and manual account/model continuation are supported; uncertain tools and unsent drafts still stop automatic sending.

### Changed (`2026-10-31p`)
- Run timers omit unused leading units. README now documents icons, phase animations and terminal-tab states.
- `/use rename ACCOUNT NICKNAME` persists a display name without changing account bindings. Header, account choices, completion descriptions and running worker rows use it; the active email appears in the header.
- `@path/to/file` transcript references and `/open @FILE` open the existing internal file split. Documented current file preview, chat-reference controls and remaining completion/viewer gaps.

### Changed (`2026-10-31o`)
- Usage headings place email and plan beside the account name, with limits below. Narrow windows wrap the heading without dropping metadata.
- Completing a command such as `/use` immediately reveals its actions and argument help. Nested choices appear after their parent subcommand is complete; Enter still opens the command and Tab advances into its arguments.
- The prompt's waiting label names the account and whether first or next output is awaited. The adjacent elapsed timer always shows hours, minutes and seconds.
- The README, architecture and project guidance center memory management and orchestration, explain existing continuity and worker behavior, and separate the native-source memory roadmap from shipped features and OSS release gates.

### Changed (`2026-10-31n`)
- `/usage` and `/use` show each account's email, subscription tier and workspace name when reported. Unknown fields remain explicit. Claude workspace names no longer replace subscription tiers, and Cursor's reported plan survives cached usage reads.
- Slash completion includes connected account ids, add/remove provider arguments and the active account's cached model ids. Tab completion also works while the landing lists remain visible. `/use ACCOUNT` and `/model MODEL` apply completed choices through the existing picker binding paths.
- Reply labels show the provider/account name once, followed by the model. The generated first-message title remains in the terminal tab title and resume picker, and is removed from the main status header.

### Changed (`2026-10-31m`)
- Run, worker and update timers show hours, minutes and seconds instead of a growing seconds counter.
- Expanded tool output keeps a clickable header pinned at the top while its original header is above the viewport. Individual cards can collapse even in full-activity mode.
- Terminal titles use a short chat or project name with distinct idle, waiting, thinking, tool, writing, stopping, usage-limit and updating status icons.
- Usage warnings explain that Codex readings update after a response, replacing the vague `last reported` suffix. Saved provider readings are labelled explicitly.

### Changed (`2026-10-31l`)
- Chats and goals remain visible while typing or pasting a draft; Enter sends it. Arrow-key list navigation wraps at both ends.
- The landing page shows all-time and rolling 24-hour token usage through Concordia, refreshed every minute from its persistent local log.
- Copy keeps selections highlighted, retries temporary Windows clipboard contention, and treats clipboard helper errors as failures. Clipboard reads preserve text without adding a newline.
- Clickable list rows, buttons, tool toggles and file links request a hand cursor using OSC 22, with text cursors over editable/selectable regions and reset on exit. Host support is required.

### Fixed (`2026-10-31k`)
- Windows exit restores the shell even when keyboard enhancement is unsupported. Cleanup attempts each terminal reset independently and erases the alternate screen before leaving, preventing chat artifacts after exit.
- Frames use synchronized updates, resize handling queries the current dimensions before clearing, and Windows resize polling continues during continuous input. A ConPTY journey checks repeated half/full width and height changes, draft alignment, and return to a parent shell.

### Fixed (`2026-10-31j`)
- The landing page scrolls selected goals and their confirmation/input into view. On narrow terminals, Tab switches the visible chat/goals pane. Wrapped and scrolled goal rows keep correct mouse targets, and goal-entry navigation/undo cannot edit the prompt underneath.
- Goal saves merge additions and completed status across panes under a bounded file lock. Unreadable existing goal files are preserved and reported instead of overwritten; new goals use unique ids.
- Chat groups use repository paths, including worktree roots, so unrelated projects with identical folder names stay separate.
- Concurrent token-log writers serialize complete records. Oversized token counters saturate safely in totals, session accounting and display formatting instead of overflowing.
- Added deterministic editor, resize, transport, persistence, concurrent-write and real-terminal stress checks, plus a bounded provider-free reliability runner (`scripts/test-reliability.ps1`).

### Fixed and added (`2026-10-31i`)
- Esc immediately followed by a paste no longer leaks a literal `[200~` marker into the prompt. The Windows input parser preserves the second escape sequence instead of consuming both escape keys together.
- A centered `Bottom` button on the prompt's top border jumps to the newest output and resumes following it without changing the draft. Active-turn status and cancellation guidance use the bottom border so they do not overlap the button.
- Down on the last prompt row opens a fresh unsent buffer; Up on its first row restores the previous draft. Both buffers survive switching and recovery. Multiline movement, selection and sent-prompt history remain available.
- Provider escape sequences are removed before transcript wrapping, tabs expand consistently, and raw control characters are blocked in every rendered widget. Text cannot emit terminal screen, title or clipboard commands through the display path.
- Updated Ratatui to 0.30.2, Unicode width handling to 0.2.3 and compatible locked dependencies. This removes the old LRU memory-safety advisories and the unmaintained `paste` dependency. Dependency policy also checks transitive unmaintained crates and rejects yanked versions.

### Changed (`2026-10-31h`)
- `/update` keeps a bold green `Enter — Install + restart this tab` action above the scrolling changelog, with an explanation that the draft and queue are saved and other tabs keep running. A yellow action strip explains when a task blocks installation; emphasis remains when terminal colors are disabled.
- Update installation is blocked while this tab's chat or background workers are active. Pressing Enter leaves the explanation open, does not cancel work, and does not schedule an automatic update. New worker tasks and worker-result sends pause during installation.

### Fixed (`2026-10-31g`)
- Pasting a message or command from the landing page now returns focus to chat, so Enter sends the message or opens `/resume` instead of activating a landing-page row. Pasting into an open goal entry stays in that entry.
- `/use` and the usage account picker put each limit on its own wrapped line with its remaining percentage and its own reset time. Cursor's monthly reset applies to every row in its monthly usage table. Missing reset times are explicit.
- Windows word-backspace preserves word deletion for Ctrl+H and Ctrl+W encodings through both terminal input readers; ordinary DEL remains single-character Backspace.
- Terminal focus return and resize invalidate the screen buffer and repaint the interface. Startup clears the alternate screen, and exit disables focus reporting.
- The isolated Windows terminal journey helper now uses Ctrl+C twice to exit and separates the timestamp prefix from user-text assertions, matching the current interface.

### Added (`2026-10-31e`)
- Every prompt Concordia sends to a provider, from a chat or a worker, opens with the local date and time, for example `[Fri Oct 30 2026, 2:05pm local]`. When the prompt mentions today, yesterday, this morning, last night, last week, N days ago, or a weekday (`on Monday`, `last Monday`), a line after it gives the date, for example `"yesterday" = Thu Oct 29 2026`. `on Monday` names both the past and the coming Monday. Concordia works these out itself, without a model. The lines are not shown in the transcript, and resumed chats, chat titles and the chat list leave them out.

### Changed (`2026-10-31e`)
- The `/update` screen says what Enter does at the top, under the build numbers: Enter installs, Concordia builds or downloads the update and restarts on it by itself, and Esc closes without updating. Before, that line came after the list of changes, so a long list pushed it out of view. The footer reads `Enter installs` whenever there is something to install.

### Fixed (`2026-10-31d`)
- Cursor usage refreshes again. Concordia types `/usage` into Cursor's own screen, and Cursor ignores a bare carriage return there, so every fetch timed out and the header kept showing an old reading. It now sends a real Enter key event.
- Claude tool cards name what they act on (`Read calc.py`) instead of showing `{`: the preview read the whole detail as JSON, but Claude's detail is the input followed by the tool's output. Paths inside the session folder are shown relative to it, as Cursor's already were.

### Changed (`2026-10-31d`)
- After Enter on the `/update` screen, the screen stays open and shows the update's progress (the current step and elapsed time) with `Esc cancels the update`, and closes when the update finishes. Closing any other screen no longer touches a running update.

### Changed (`2026-10-31c`)
- While an update builds, the message box says so: its title reads `updating Concordia` with the elapsed time, and an empty box reads `Messages are paused until Concordia restarts on the new build`. A draft typed meanwhile is kept.
- The `/update` screen lists every earlier build with its changes, newest first, formatted like the newest entries, so the whole changelog reads in one place. The `c` key that opened the raw changelog in a side pane is gone.

### Added (`2026-10-31b`)
- Concordia keeps a local log of every turn's tokens. `/usage` then `t` shows totals for 24 hours, 7 days and all time, where the tokens went (re-read from the cache, new input, output), and breakdowns by provider, model, account, project and chat. The landing page shows the last 24 hours. Providers count cached input differently, so the log stores the same three parts for each.

### Fixed (`2026-10-31b`)
- Cursor turns show their token counts: Cursor reports them in camelCase (`inputTokens`, `cacheReadTokens`), which Concordia did not read.

### Added (`2026-10-31a`)
- A landing page opens with Concordia and with `/new` when chats have been recorded: recent chats grouped by project (a git worktree's chats join its repository, and Concordia's own usage-probe sessions are left out), the current project first and expanded; Enter or a double-click resumes a chat or expands a project. A goals pane keeps your goals (`Tab`, then `a` adds, Enter marks done) and, with a Harness connected, each project's open work from `harness run plan:open --json`. Typing anything starts a message and Esc goes to the chat.
- A focused file pane closes with `q` (or its X). `Ctrl+W`, which terminals use to close tabs, is no longer bound.
- Harness programs installed as npm `.cmd` shims (for example a global `harness`) now start, so `/harness connect harness` works on Windows.

### Changed (`2026-10-30c`)
- `/update` shows what an update contains before installing it: the installed and available build, where it comes from (the source checkout with its branch and uncommitted files, or the GitHub release with its date), and the changelog entries in between or the release notes. Only Enter installs; `c` opens the full changelog and `o` the release page. A checkout behind the running build is shown but not offered for install.
- Published releases carry the matching `CHANGELOG.md` section as their notes, and a release without a changelog entry fails.

### Fixed (`2026-10-30b`)
- Cursor text written just before a tool call no longer appears twice: Cursor sends it in fragments and then whole, and the whole copy is now recognised.

### Changed (`2026-10-30b`)
- The command list has 12 commands, each opening one screen: `/use`, `/model`, `/mode`, `/usage`, `/new`, `/resume`, `/delegate`, `/harness`, `/open`, `/copy`, `/doctor`, `/update`. `/trust` became `/mode`; `/threads` became `/resume`; `/usage all` and `/usage refresh` became `/usage` (with `r` to refresh); `/version` and `/config` became `/doctor`; usage handoff is a row in `/use`, and a paused request offers an account picker. Typing a removed command names its replacement. `/workers`, `/queue`, `/edit`, `/close`, `/split`, `/stats` and `/recover` still work but are no longer listed.

### Fixed (`2026-10-30a`)
- Cursor tool cards show the tool's name (glob, read, edit) instead of `completedatms`, which current Cursor CLIs caused by adding bookkeeping fields beside the tool.
- Cursor replies stream as they are written (`--stream-partial-output`, when the installed CLI lists it), and reasoning and reply fragments join into one paragraph instead of one line per fragment. The full copy Cursor sends after streamed text is not shown twice.
- Finished Cursor edits show their unified diff; reads show the path and line count; globs show the matched files. Paths inside the session folder are shown relative to it.
- Diff details collapse to a `+N -M` summary and expand with added lines green, removed lines red and headers dimmed.
- Usage resets read the same for every provider (`resets in 3h 20m`, `resets Nov 6, 9am`) and are measured against the current time, so a reading taken before a reset shows `reset since last read` instead of its old percentage.
- Session notices are plain sentences, and the normal start of a chat shows none.

### Changed
- Concordia now owns its built-in Harness adapter: it validates `harness version --json`, discovers registered tools, and uses the same deterministic manifest for connection and receipt refresh.

### Fixed
- Harness adapters now run with an allowlisted environment and whole-process timeout cleanup; verification binds policy and manifest revisions, protects existing receipts, and status rechecks live state.
- Enforcement receipts now bind to the connected harness name, version, manifest path and manifest hash; file manifests are re-hashed before a protected launch so stale receipts close the gate.

### Added (next build)
- Generic `/harness` connect, verify, status, tool listing/execution and disconnect flows for user-owned adapters.
- Locked-code launch gate. In a project that carries `docs/quality/locked-code.json`, a turn that asks for write-capable trust (ask, workspace or full) runs read-only (plan) with a visible notice unless a verification receipt in Concordia's state folder proves a live probe saw the denial for that provider, isolated home and policy revision. The check runs before any provider process, including Cursor's `create-chat`. Receipts live in Concordia's state directory, not in the project, a project that was locked stays locked if its policy file is deleted, a provider home inside the project never counts as isolated, and a turn is refused if a capability fallback would drop its read-only switch. Projects without locked regions are unchanged.

### Added (`2026-10-21c`)
- Automatic session titles in the header, terminal title and resume picker, persisted separately from optional transcript logging and generated without model calls.
- `/resume` restores bounded recent Codex/Claude messages as scrollback and binds the explicitly selected session across accounts. History loading preserves the draft and does not dispatch a provider turn. Unsupported Cursor history is reported explicitly.
- `/agents` and Ctrl+G inspect native child-agent reports and Concordia worker jobs. Codex collaboration events show each child's reported task, status and result; Claude Agent/Task tool cards are also accessible.

### Changed (`2026-10-21c`)
- Cancelling leaves the prompt box unchanged; it no longer restores the submitted request. Sent history remains available for explicit recall. Failure retry behavior remains separate.

### Fixed (`2026-10-21b`)
- Commentary, reasoning and individual tool cards retain their arrival order while streaming; completed turns no longer collect all commentary below a block of commands.
- Cancellation retains partial replies and unsent drafts, pauses queued work, suppresses duplicate stop requests and recognizes a stop requested while waiting for process exit.
- Codex usage accepts text credit balances, selects the newest eligible session instead of the second-newest, compares session/cache freshness, tolerates partial UTF-8 at a bounded tail boundary and updates an existing `/usage` block after a turn.
- Everyday smoke uses Cargo exit codes when output is captured by Windows PowerShell and cleans up its generated usage test on errors.

### Development
- Finished and compiled the lossless UTF-8 document/undo/save foundation from interrupted work. Internal panes remain read-only; editing controls and recovery are a later slice.

### Documentation
- Reconciled installed `2026-10-16e` readiness with recorded smoke and fake-provider terminal evidence, preserving outstanding live host/provider checks. Defined staged internal file-pane, editing/recovery and self-development acceptance in the canonical plan.

### Fixed
- Windows VT input preserves framed pastes across chunks with a Unicode console reader, keeping Tab/Enter actions separate, Ctrl+Z usable, emoji intact, and paste undo atomic. Bounded readers wait for startup readiness, join on shutdown and restore console settings; fallback key input retains CRLF and undo continuity across source gaps.
- Input/render errors persist the draft and cancel provider work; a failed startup handle handoff cancels its provider instead of leaving it running.
- Interrupted or incomplete tool calls retain their command/details and explain the missing completion instead of appearing successful. Explicit chat reads target the requested account beyond the newest-list window; metadata scans and output are bounded.
- Invalid arguments no longer silently run updates or choose fallback slash/CLI actions. Permission changes made during a turn explain that they apply to the next turn.
- Past Codex reset times show current usage as unknown, rather than inventing 100% remaining capacity; cached refreshes and failures are labeled.
- Distinct rapid Escape/navigation presses are retained when terminal input is drained after a paste; backend release events are filtered by their event kind instead of a processing-time debounce.
- Unsaved upper queue edits survive restart separately from the main draft and committed queue. Outside clicks cannot discard them; narrow and short panels clip editor hit targets and caret positions.
- Recovery save failures stay visible and dirty, retry on bounded ticks, and support `/recover retry` after local storage is repaired. Importing legacy files preserves the current pane record.
- Long and multiline paste stays inside activity search or picker filters while those views are open. Activity search reuses cached wrapped rows, and `Alt+Up/Down` navigates older/newer cards.
- Pane and workspace recovery records use independent ownership locks, preventing concurrent panes from restoring or overwriting each other's active drafts. Multiple abandoned records require an explicit choice through `/recover`.
- Queue resume is explicit (`Ctrl+R` or `/queue resume`); sending typed text leaves a paused queue paused.
- Same-process state writers serialize before the cross-process lock deadline, preventing the concurrent-writer stress test's intermittent lock timeout. The re-exec unit test now isolates its install pointer and environment instead of changing the real installation.
- Real Windows terminal journeys caught pasted LF arriving as Ctrl+Enter: it now inserts a newline, preserving blank lines and CRLF without sending separate prompts.
- A queued prompt being edited waits for save/cancel before automatic dispatch. Chunked terminal pastes undo in one step.
- Conversation, provider and model changes wait until a running turn finishes or is cancelled, preserving its session and output identity.
- Running slash commands during a turn preserves its original cancellation recovery prompt; reused tool IDs update only the current turn's cards.
- Cursor tool results retain their full received output. A narrowly patched, vendored Crossterm 0.28.1 preserves Windows emoji input.
- Full tool input/output and reasoning are retained; Ctrl+O toggles compact or expanded activity, including while a turn runs.
- Provider session locks prevent concurrent Concordia resumes; Codex active-writer errors explain recovery, restore the prompt and pause queued work without replaying it.
- Paste bursts drain before repainting and terminal titles update only when changed.
- Multiline pastes delivered as Windows key events stay in the prompt, including blank lines and trailing newlines; a separate Enter sends the complete prompt.
- Cancelling a turn preserves an unsent draft and restores the submitted prompt when the input is empty, so it can be edited or retried.
### Added
- Internal read-only `/open` and `/preview` panes beside chat, with stable ids, repeated-open focus, mouse X and `/close`, F6/Esc focus, scrolling and divider resizing. Reads are asynchronous and capped at three panes, 2 MiB and 50,000 UTF-8 lines per file. `/edit` preserves external-editor launching; editing and layout recovery remain future work.
- `concordia chat list|search|read` exposes bounded native CLI chat metadata/text across the user's own accounts; reads bind an explicit account and session, without switching or resuming. Cursor transcript reading remains unsupported.
- Clickable file references in sent messages and responses, plus `/open` for a separate terminal editor and `/preview` (Alt+click) for a built-in read-only Markdown preview. Editor executable/argv are configurable; command text is never evaluated as a shell string.
- Low 5h/7d usage warnings above chat, with reset time and reading freshness; Codex reset timestamps use the OS local date/time.
- Expanded provider-reported MCP arguments/results, file changes/diffs and tool error states, plus bounded Claude partial tool-input streaming. Missing provider edit data is not fabricated.
- An explicit release-mode typing/streaming stress benchmark with a 500-message transcript and a long draft.
- Optional `/stats` footer with Concordia and provider process-tree RAM/CPU; provider version/help detection with cached capabilities, text fallback and restored `/usage` colors.
- `@chat` thread references, awareness instructions in Concordia-created homes, and an opt-in MCP server with read-only tools and delegation proposals requiring confirmation.
- Prompt queue: `Tab` queues while a turn runs; `Ctrl+Up` focuses the upper panel without replacing a draft. Enter opens/saves a multiline upper editor, Esc cancels, `Ctrl+Down` returns after save/cancel, `Alt+Up/Down` reorders, and Delete removes. Clean completion sends queued prompts in order; cancel/failure/restart pauses them until `Ctrl+R` or `/queue resume`. `/queue` lists and `/queue clear` clears committed items.
- Undo and redo in the prompt (`Ctrl+Z`, `Ctrl+Shift+Z` / `Ctrl+Y`, 200 steps; undo right after a send brings the sent text back).
- Draft and queue recovery use pane-owned records under `<state>\pane-recovery\<workspace-key>\`; `/recover` selects abandoned work or imports older shared files. Sent prompts use `<state>\history.jsonl` (last 500; `[ui] persist_history = false` turns it off). A failed or cancelled turn puts its prompt back in an empty box.
- Individual activity expansion via gutter clicks, plus `Ctrl+E` full detail with Unicode search, highlighted matches, scrolling and full-card copy. Modal controls preserve the draft beneath.

### Changed
- Switching account or provider now carries a deterministic map of the thread (at most 1600 chars: what was asked, files touched, commands run, errors, the last reply, and a pointer to `concordia thread read`) instead of a 6000-char transcript dump. Delegated workers get the same map. Native resume is unchanged.
### Added
- A dim token line under each reply (`in 12.3k - out 1.4k - cached 9.8k - 8.2s`): only what the provider reports (Claude `result`, Codex `turn.completed`; Cursor reports none) plus the turn time. It is not part of the context pack or `/copy`. `CONCORDIA_TOKEN_LINE=0` hides it.
- One tip per launch, dimmed, on the empty start screen and after 20 s idle with an empty prompt. `CONCORDIA_TIPS=0` hides it.
### Added
- Optional `<state>\config.toml` (UI switches, default mode, threads, delegation defaults; never written, malformed files fall back to defaults) and custom prompt commands in `<state>\commands\*.md` that appear in the `/` palette. `concordia doctor` gains `config` and `commands` rows. See docs/config.md.

### Fixed
- Limits are fresher and plainer: the `[####]` bars are gone; Claude and Cursor readings are re-checked after every turn and every 2 minutes, and show their age; a Codex window that has reset since the last chat shows as reset. The README now says plainly what Concordia is and is not, and lists the behaviours worth knowing.
- `/usage` is now one titled block that reads the cache and rewrites itself in place when the background check finishes (it used to leave a stale "asking..." line). `/usage refresh` forces a new check. A Workflows section in the README explains switching accounts and continuing a chat.
- The bottom hint line adapts to the width: whole hints are dropped by priority and labels shortened (`Ctrl+C copy/stop`) instead of being cut mid-word, and the mode column never overlaps them.
- Claude and Cursor turns now stream (`stream-json`) like Codex: tool cards and replies appear as they happen instead of all at once at the end.
- The wait shows what the model is doing (waiting, thinking, running a tool, writing) with a per-phase colour, animation and timer; reasoning is shown live as dim lines and kept after the turn, never sent as context.
- A failure reported as an event, a result and stderr is shown once, and `Reading additional input from stdin...` is hidden.
- The prompt marker, palette and picker highlight use plain ASCII `>` and `...`, whose width is the same in every terminal font; ambiguous-width glyphs could shift the caret.
- After `/update`, the old process now stays as the parent until the new one exits. Before, it handed the console back to the shell early, so the shell and Concordia both read the keyboard and the prompt box lost or scrambled keystrokes.

### Added
- Thread log (opt-in, `CONCORDIA_THREADS=1` or store setting `thread_log`): conversations are recorded to `<state>/threads/<id>.jsonl`; `/threads` lists recent ones and `/threads delete <id>` removes one. `concordia thread list [--json] | map <id> | read <id> [--tail N] [--grep TEXT] [--range A-B] [--max-chars N]` prints them read-only so a model with a shell can fetch the part it needs.
- Reproducible screenshots and speed metrics: `cargo test -p concordia --lib screenshots -- --ignored`
  renders the real UI on a test backend into `docs/img/*.svg` (synthetic data, no provider run);
  `scripts/metrics.ps1` measures binary size, `--version` cold start and `draw_ui` frame time into
  `docs/metrics.md`. README restructured around them.
- Shift+Tab cycles the permission mode plan, ask, workspace, full (colour-coded in the footer; moves the palette selection up while the palette is open). New `plan` mode maps to Claude `--permission-mode plan`, Codex `-s read-only` (no separate plan mode) and Cursor `--mode plan`. `/mode [plan|ask|auto|full]` shows or sets it.
- Slash-command output is a dim block with a left bar and the command as its title, with `+`, `!`, `i`, `x` level icons; agent replies and their tool cards sit under a `Provider · account · model` label in the provider colour. Provider-coloured header badge and dimmer palette descriptions.
- Usage display: limits say used and left everywhere (footer `5h 59% left`, `/use` and `/usage all` rows
  `5h 41% used / 59% left`). `/usage` shows only the current account with a bar per window and reset
  time; `/usage all` lists every account. Claude limits come from `claude -p "/usage"` on a background
  thread, cached 5 minutes under `usage-cache/`. Cursor says "not shown by Cursor CLI"; Codex with no
  data says "no data yet - chat once". Rows and footer are coloured by how much is used.
- Cursor limits (Included, Auto, API, On-Demand, with plan and reset date): Cursor's CLI has no usage
  command, so Concordia opens the interactive agent in a pseudo-console (`portable-pty`, MIT) in an
  empty temp folder, types only `/usage` (never a prompt, no tokens spent), reads the table and kills
  the process tree. Same background thread and 5-minute cache as Claude; on any failure or timeout
  (15 s) the account says "not shown by Cursor CLI".
- Native Rust TUI: streaming chat with tool and command cards, account picker, model picker,
  usage, explicit session resume, trust modes, WezTerm splits.
- Mouse support: click to place the caret, drag / double-click / triple-click selection in the
  prompt and the transcript, wheel and `PgUp`/`PgDn` scrolling, right-click copy or paste.
- A real prompt editor: multi-line, Shift-selection, word moves, cut/copy/paste, bracketed paste,
  prompt history, `Ctrl+C` copy-or-cancel, double-tap exit.
- `/use add [codex|claude|cursor]`: creates an isolated account home and opens the provider's own
  login; announces when it connects.
- `concordia doctor` and `/doctor`: install, state, provider CLIs with versions, accounts, PATH, update mode.
- `concordia install`: one install story (puts the exe where `concordia` on PATH finds it, edits the user PATH
  safely, refreshes the launcher shim).
- `/update`: rebuilds from a source checkout, or downloads the newest release (checksum, runs,
  right version, else nothing is installed). `/update check` previews.
- Build stamp in the header and `--version`; fake-provider and TUI end-to-end test harnesses.
- CI (fmt, clippy `-D warnings`, tests, release smoke, `cargo deny`), release workflow with
  third-party license generation, pinned toolchain.

- Slash-command palette: typing `/` shows matching commands with descriptions (arrows, Tab, Enter, click).
- `/use remove`: remove an account. Concordia-created accounts can be deleted with their login data; your own
  homes can only be hidden. Signing in to the same login twice is detected and the duplicate removed;
  `/use` and `doctor` flag existing duplicates; the picker names each account (Codex 2, Codex 3).
- Claude and Cursor now keep their conversation between turns (explicit session ids), like Codex.
- Markdown rendering for replies (headings, lists, code, inline code), a status-bar header, grouped tool
  cards, spacing between turns, a spinner while working, trust shown in colour (full is flagged).
- `/update` rebuilds from any pane (the install remembers its source checkout), runs in the background with
  live progress, and Esc cancels it (the whole build process tree is stopped); the UI stays usable.
- `concordia wezterm` opens WezTerm running Concordia; `/split` works for installed copies (the WezTerm
  config is bundled in the binary).
- A log file in the state folder (`concordia.log`, events only, rotates at 1 MB); a crash records its
  backtrace there. `NO_COLOR` is honored; `CONCORDIA_LOG=off` disables the log.
- Delegation: `/workers [add|remove]` picks up to 3 worker accounts; `/delegate <account|auto> <task>` runs one
  background turn on a worker (fresh session, bounded context, your trust mode, never full unless yours is);
  a worker strip above the prompt shows each worker live (Alt+Up/Down or click to focus, Enter opens its
  transcript, Esc cancels); finished results arrive as a labelled untrusted block and reach the orchestrator
  only through `/delegate use`. Optional opt-in protocol (`/delegate protocol on`) lets the orchestrator
  propose a delegation that still needs your `y`. See docs/delegation.md.

### Fixed
- The prompt marker, palette and picker highlight use plain ASCII `>` and `...`, whose width is the same in every terminal font; ambiguous-width glyphs could shift the caret.
- `concordia` on PATH could resolve to a months-old binary: the installer now always replaces `concordia.exe`,
  even while an older Concordia is running from it (the old file is moved aside), and `doctor` reports
  when PATH resolves to anything other than the active build.
- Multi-line, `%`, `!`, non-ASCII and very long prompts failed or were rewritten behind Windows
  `.cmd` shims; such prompts now go over stdin.
- `Ctrl+C` killed only the shim and left the real agent running; it now kills the whole process tree.
- A failing provider showed an empty reply; stderr is now captured and shown with the exit code.
- Provider output with invalid UTF-8 truncated replies; a huge line could exhaust memory.
- `concordia --version` rewrote the install pointer to whichever exe ran it.
- Shared state could be lost when two panes saved at once, or destroyed when corrupt; writes are
  now atomic, locked, and a corrupt file is backed up first.
- `Ctrl+C` inside a picker quit the app; it now closes the picker. Clipboard calls could freeze the UI; they time out.
- Long sessions made drawing slow (about 500 ms per frame at 40k rows); rows are now cached (~17 ms).

### Changed
- The npm shim and the `-a/-b/-next` install slots are gone: `concordia.exe` is always the
  newest build.
- `Esc` no longer quits (it clears a selection or interrupts a running turn).
- The TypeScript prototype and all research spikes were removed; the product is Rust only.
