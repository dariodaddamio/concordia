# Changelog

### Changed (`2026-10-21d`)
- Compact tool rows foreground the command instead of PowerShell launcher paths, retain complete expanded output, and show output line counts. Completed reasoning is collapsed to one preview line.
- Terminal titles use the terminal API, including the Windows console path, and follow the generated session title.
- Usage-limit continuation defaults to manual (`/continue ACCOUNT`), with opt-in automatic handoff through configured accounts. Attempts are bounded, native session ids stay account-local, drafts and queued work are preserved, and uncertain tool results stop automatic handoff.

All notable changes to Concordia. The format follows [Keep a Changelog](https://keepachangelog.com/);
Concordia has not had a tagged release yet, so everything is under Unreleased. Each user-visible change
also bumps the build stamp shown by `concordia --version`.

## [Unreleased]

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
