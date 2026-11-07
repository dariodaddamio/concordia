# Changelog

## 2026-11-07d

- Pickers, footers, usage, turn summaries and flash messages show `·`, `—` and arrows again
  instead of garbled `Ã‚·` / `Ã¢â‚¬` text.
- Cursor's model picker lists every model your plan allows (Claude, GPT, Grok, Gemini, Composer
  and their effort/fast variants) from `agent models`, refreshed automatically when the picker
  opens with no list or a day-old one.
- Cursor turns run over ACP by default, so permission prompts and plan approvals open as popups
  and images work; `/native ACCOUNT off` goes back to print mode. Full trust carries `--force`,
  and flat model ids are translated to the session's parameterized variants.
- Codex's "Ignoring unknown features requirement" notices no longer appear as error cards.
- A Cursor chat started in print mode still continues after the ACP switch: when ACP has no
  session with that id, the turn falls back to print mode.
- Tabs actually work now (they were never connected): a browser-style strip on the top row
  shows each tab, a click switches to it, and Ctrl+T / Ctrl+W / Ctrl+Shift+T / Ctrl+Tab /
  Ctrl+1..9 also work in the native window.
- Images imported with a `\\?\` long-path prefix are sent with a plain path, so Codex can read them.
- Home page: Up/Down move through the chats even after you started typing; long goals are no
  longer cut off; the usage box shows each account's plan, credits and every limit window on its
  own row and refreshes stale accounts; usage is no longer shown twice when the goals pane has focus.

## 2026-11-07c

- Chat titles also skip Concordia's own awareness note, so a chat is named after what you typed.

## 2026-11-07b

- Chat titles no longer show context that Codex injects into the first message (plugin lists,
  AGENTS.md instructions, environment blocks); titles are a single line of what you actually
  typed, and sessions with only injected context count as empty.
- `concordia doctor` shows the state-folder permission fix once, as a PowerShell-ready command.

## 2026-11-07a

### Tabs
- Browser-style tabs: Ctrl+T opens a new tab, Ctrl+W closes one (asking only if it has an unsent
  draft or a running turn), Ctrl+Shift+T reopens the last closed tab, Ctrl+Tab or Ctrl+PgDn/PgUp
  switch. Each tab is an independent session, and background turns keep running.

### Screen
- Each fact appears once on screen: the usage reset time sits in the header's subscription
  segment, and on the home page the usage box replaces the header's Subscription segment.
- Provider questions with choices open a popup (keyboard or mouse, "other" always available),
  but never over a draft you are typing: then an inline notice waits for you (Ctrl+K).
- One "needs you" queue collects everything waiting on you across sessions (approvals,
  questions, paused queues, quiet turns, finished worker results, signed-out warnings); a header
  badge appears only when it is not empty.
- A worker side panel shows workers live without ever taking focus; crews (`[crews.<name>]` in
  config, `/crew use`) set a lead model and enroll workers with theirs.
- One command registry drives the palette, `/help`, the README command table and tips; a key
  sheet lists every binding, and tips are contextual and rate-limited.
- The screen redraws only when something changed; long sessions keep syntax highlighting and the
  transcript within memory bounds.

### Continuity
- A task outlives its sessions: `/resume` lists tasks with the chain of sessions each one used
  across accounts and providers.
- Handoffs carry workspace state: branch, HEAD, uncommitted files (capped) and running workers.
- Concordia map v2: deterministic labelled pointers (never summaries), and the transcript spans
  they point at are pinned (redacted) so they survive provider pruning.
- Near the context limit, choose provider compaction (default) or a fresh session seeded with the
  map, per provider.
- Same-provider automatic handoff is off by default; turning it on shows the terms notice once
  and caps automatic handoffs per account per day. The next account is ranked by remaining quota,
  then model tier.
- Usage limits are recognised from structured provider fields first, with wording only as a last
  resort.

### Health and setup
- `/doctor` can apply confirmed fixes (PATH, state-folder permissions, a stale legacy install),
  each with a before/after, an explicit yes and a backup first. `concordia doctor` stays read-only.
- The resource governor caps worker concurrency and build parallelism by RAM and cores and warns
  on low disk.
- `concordia` subcommands and `--help` come from one table.
- Releases: a fast everyday build profile and a smaller published one, plus an SBOM, build
  provenance and a Windows version resource.

### Under the hood
- `tui/mod.rs` was split from 8,381 lines into draw, input, run-loop and app modules (about 360 left); continuity decisions are pure
  functions outside the UI; thread logging is a setting; a CI check blocks new module cycles.

## 2026-11-06b

### Home
- Usage has its own box above goals, one line per account (never wrapped), so the goals box keeps
  its "add a goal" prompt. A reset already passed reads "reset at <time>", an upcoming one
  "resets <time>".
- Chats from folders that no longer exist (for example deleted git worktrees) are hidden from
  home; they stay reachable with `/resume` and the `h` show-all toggle.
- Opifex Studio (near-black, amber, bone) is the default theme; existing installs switch once,
  and any theme you pick afterwards sticks. Themes can be imported and exported as TOML files
  from `/config`, with a contrast check.

### Security
- Secrets are redacted before anything is written (history, drafts, queue, logs, thread logs,
  card text, editor recovery) and again when served over MCP: API keys, tokens, JWTs, private
  keys, passwords in URLs and connection strings, and Concordia's own Cursor key.
- The MCP server only reads the account and workspace (and its managed worktrees) it was
  registered for, and serves nothing without a scope.
- The editor and previews stay inside the workspace: a link pointing outside is not opened or
  saved through, more secret files are treated as sensitive (SSH private keys, cloud and package
  credentials, keystores, `*secret*`), and Windows device names like `CON` are rejected.
- Worker output handed to another agent is wrapped in a random per-run fence it cannot close.
- Concordia's state folder is created owner-only; `doctor` warns if an existing one isn't, and
  if `CONCORDIA_STATE_DIR` points outside your profile. Update-source overrides need
  `CONCORDIA_DEV=1`, and a shell `CURSOR_API_KEY` no longer reaches Cursor accounts without a key.

### Reliability
- The cached `--version` probe now notices CLI updates behind unchanged `.cmd` launchers (npm,
  Cursor agent).
- `doctor` no longer suggests running `doctor` from inside `doctor`.
- README is now a short user guide; internals moved to `docs/`.
- Tests are deterministic again: no test changes the shared process environment.
## 2026-11-06a

### Works on a fresh Windows machine
- `.\scripts\install-dev.ps1 -Install` takes a clean machine from clone to a running Concordia: it
  checks Rust, the pinned toolchain, MSVC, the Windows SDK, a running Visual Studio installer and
  WezTerm, installs what is missing, builds, installs, adds the Start menu shortcut and runs
  `concordia doctor`. `-CheckOnly` reports without changing anything.
- Concordia finds the Codex, Claude Code and Cursor agent CLIs at their standard install folders
  even when those folders are not on PATH (for example Claude Code's `%USERPROFILE%\.local\bin`).
- `concordia doctor` shows where each CLI was found and prints one concrete next action for every
  warning or failure: an install command, the PATH fix, `/use` to sign in, `concordia install`, or
  the WezTerm install command. It finds WezTerm even when it is not on PATH, and it stays read-only.
- First launch with no accounts shows a short welcome with a local check of which CLIs were found
  and "/use to add an account".
- `concordia shortcut install [--desktop]` adds a Start menu (or Desktop) shortcut that opens
  Concordia in WezTerm with no console flash, or in your default terminal without WezTerm.

### Safety
- A delegated worker can never do more than you can: its mode is capped at your own.
- Plan mode fails closed: if an installed CLI can't enforce Plan, the turn is refused with an
  explanation instead of silently running under the CLI's default behaviour.
- Cursor's Workspace mode no longer turns Cursor's sandbox off.
- Shift+Tab cycles Plan, Ask and Workspace only. Full needs `/mode full`, asks once per session,
  and its badge is red.
- Provider CLIs are never launched by bare name, and copies planted in a project folder or the
  current directory are ignored.
- Provider processes die with Concordia, even if Concordia crashes (a kill-on-close job object on
  Windows, process groups on Unix).
- Logs never contain panic payload text; provider stderr and per-turn tool bookkeeping are
  bounded; bidi and zero-width spoofing characters are stripped from file names and card text.

### Reliability
- A turn that produces no output for 10 minutes (`[turn] idle_timeout_minutes`, 0 turns it off)
  shows "No output from <provider> for N minutes", updating in place. Concordia never cancels on
  its own: Esc cancels, or click the notice (Ctrl+K) for keep waiting, cancel or hand off to
  another signed-in account. Quiet workers show "silent N min".
- No more console windows flashing on Windows: every background child process starts hidden.
- Closing a pane mid-edit can no longer crash Concordia; state files are flushed to disk before
  they replace the old copy.
- A signed-out account warns before sending ("press Enter again to send anyway") and keeps your
  prompt.
- One patched crossterm (0.29) for the whole UI, so emoji typed on Windows and bracketed paste
  behave the same everywhere.
- Launch no longer runs `--version` for every provider each time; results are cached per binary.

### Home, settings and models
- Home shows only chats with a real message, folder names without `\\?\` prefixes, hides test,
  temp and `target` workspaces until `h` (or a click on the footer), folds short older chats into
  a "Quick questions" row per folder, and shows a usage meter per account under goals.
- `concordia sessions prune --empty` removes empty native sessions after an exact `yes`, and
  Claude usage reads no longer create "untitled session" entries.
- `/config` is a full settings screen; `/skills` and `/hooks` list project and account sources.
- Shorter window titles and an animated window icon that respects the motion setting.
- `/model` lists every model each provider offers: Cursor's own list, the models Codex and Claude
  Code have cached, and a shipped catalog, grouped by family with where each came from. Type to
  filter, `r` to refresh.

### For contributors
- Tests can no longer touch your real Concordia state or provider homes: defaults are redirected
  to temp folders and a guard fails any test that reaches the real ones.
- All 48 PTY journeys pass again (a usage-probe check in the test fake, and a stale header text).

## 2026-11-05g

- Images in the native window no longer flicker when the caret blinks or the prompt changes.
  The window now clips image areas out of the text-layer copy instead of briefly showing the
  blank cells underneath.
- Short thinking is shown whole without an Expand toggle that revealed nothing new.
- The header says "view email" instead of "email · click to show".
- An empty goals pane explains what goals are and how to add one.
- The `[concordia map]` line shown after a provider switch expands to the full map that was
  handed to the next provider.

## 2026-11-05f

- `/model` works before the first chat. It lists current Claude models by name (Opus 5.5,
  Sonnet 5.5, Haiku 5.5, Fable 5.1), the `latest` aliases, and models cached by any account
  for the same provider. You can also type any model id and choose `use <id>`. The empty "no cached
  models" dead end is gone.

## 2026-11-05d

- Restored the original gold Concordia portrait on Home at the owner’s request. Removed the
  3D mesh, renderer and extra animation wakeups. Appearance themes, icon tint and simpler titles remain.

## 2026-11-05c

- `/config` now opens live appearance settings with immediate, persistent theme, Concordia color
  and motion controls; it no longer redirects to diagnostics. Hand-edited config files stay intact.
- Added Opifex Studio's near-black/amber/bone palette and a rotating, depth-tested classical
  Concordia bust with local scan/glitch treatment. Motion-off and Windows animation preferences
  are respected. The compact CC0 source and deterministic mesh conversion have recorded provenance.
- Concordia color presets/custom hex values tint both the sculpture and running-window icon.
- Simplified window/tab titles to application, task and short active phase, with Home labeled clearly.

### Changed (`2026-11-05b`)

- Long native turns use separate streaming, body-free capture manifests. Large attachment records
  and irrelevant tool bodies no longer exhaust the small interactive-history index budget.
  Existing references remain readable, and sealed captures exclude subsequent turns.
- Explicit legacy migration previews bind an exact native source, preserve uncertain rows and
  original-byte backups, and apply through revision-checked transactions with status reconciliation.
- Legacy and migrated prompt recall hydrates in the background, preserves newly sent history,
  and reports unavailable sources without rewriting original files or blocking the composer.
- All seven pure MCP read tools declare standard effect hints, so native clients can
  distinguish it from unclassified or destructive tools. Provider approval policy stays authoritative.

### Fixed and added (`2026-11-05a`)

- Recover successful static code-mode patch batches into the original inline edit rows; retain
  exact historical patch content instead of adding a separate file-metadata card where matched.
- Configurable dark/light/terminal palettes and background/foreground/accent colors share the
  existing chat, menu, diff, image and native-window rendering paths.
- Grouped and individual tool rows use consistent status markers and bold action/subject text.
  Native checks/status marks render independently of font fallback, with style-specific glyph caches.
- Original gold/bronze Concordia goddess artwork replaces the active geometric icon, with a compact
  Home portrait on roomy screens. Installation remains one Rust executable with bundled assets.

### Changed (`2026-11-04r`)

- New recorded message/tool bodies and persistent prompt recall use verified native-source
  references. Legacy logs remain readable and untouched; missing sources are explicit.
- Completed, collected workers with verified references retain source pointers and attachment
  descriptors instead of duplicate task/prompt bodies. Pending or unbindable recovery stays intact.
- Source ranges seal before the next turn; ordered background writes keep recovery until durable
  persistence completes. Chat references preserve the original input and hydrate off the UI thread.

### Fixed (`2026-11-04q`)

- Recorded thinking expands directly in chat, including restored history. The completed-turn
  duration/token footer no longer offers a separate activity-review popup.
- Inline edits show reported line numbers, syntax colors and full-width muted red/green
  backgrounds, while preserving original diff text and exact historical file identities.

### Fixed (`2026-11-04p`)

- Tool summaries show filenames, readable command purposes and output counts instead of wrapper
  JavaScript. Shortened filenames retain their exact clickable targets and full expanded output.
- Multi-question choice bundles remain selectable despite explanatory text and progress notes,
  with clear question progress and mouse/keyboard selection across the whole form.
- The enlarged image preview has the shared top-right X close control.
- While a turn runs, Enter explains the next action; a second deliberate Enter interrupts and
  sends the follow-up after the old turn settles. Editing or changing turns resets confirmation.
- Updates reuse checksum-verified downloads, avoid replacing identical installed bytes and prune
  bounded owned obsolete artifacts while preserving current/running versions and a rollback.

### Fixed (`2026-11-04o`)

- Held arrows and navigation/deletion keys repeat normally, including Ctrl/Shift movement.
  Action shortcuts remain single-press controls.
- Reopened chats retain their thinking and expandable tool cards without synthetic
  "Recorded turn" review messages.
- Routine GitHub CI now runs only by manual dispatch to conserve the exhausted Actions allowance.
  Existing gates remain available; explicit release-tag jobs are unchanged.

### Fixed (`2026-11-04n`)

- Thinking renders Markdown consistently, and shortened previews avoid broken formatting.
- Explicit documentation links can open a missing root basename from the workspace's `docs`
  directory; wrapped labels retain their original clickable destinations.
- Tool headers distinguish the action, status, file links, additions/removals and disclosure with
  color. Expanded cards retain their readable action and summary.
- The footer permissions menu separates mode names from complete descriptions and clearly marks
  the current mode, with responsive rows and the existing selection colors.

### Fixed (`2026-11-04m`)

- Permissions open in a compact footer popover, preserving the visible composer. Clicking
  outside returns directly to the underlying control without changing the permission mode.
- Completed tool headers show Expand/Collapse and respond across their header text; links keep
  their file/web actions and dragging still selects text.
- Reopened Codex and Claude chats restore recorded native tool inputs/results/status and readable
  thinking, with a completed-turn activity review. Encrypted reasoning stays unavailable; Cursor
  saved-history restoration remains text-only. Imported running calls retain their status.

### Fixed (`2026-11-04l`)

- Detached file windows identify their file relative to the chat workspace and retain the dirty
  marker. Their footer names Ctrl+Shift+D to dock, including formatted views.

### Added (`2026-11-04k`)

- A single unsigned Windows installer downloads verified prebuilt bytes, makes `concordia`
  available in the current shell and opens it. Release updates use the official feed, independently
  of old developer checkout hints. Public installation still requires the first published release.
- An original shared logo and task-first window titles identify Concordia and the active work.
  The bottom-right permissions control opens the existing mode picker with mouse selection.
- Native file panes detach into separate windows and return to the workspace with the same
  document, save checks and undo history. Horizontal scrolling keeps line numbers fixed.
- Native web-search calls and mixed tool wrappers show meaningful names and retained details.
  Consecutive completed inspections and checks group into expandable summaries.
- Large counts use millions, billions, trillions and scientific notation through the full range.

### Fixed (`2026-11-04j`)

- Native patch recovery accepts static JavaScript tool argument objects and structured output
  blocks while retaining exact attempt/source ownership and successful-patch checks.
- Home excludes explicit native subagent sessions from user chat history and deduplicates exact
  session identities while retaining genuine user forks and source lookup.
- Native symbols use fallback fonts and fitted glyph bounds; Concordia has its own window and
  executable icon. Window titles identify the account/model, actual phase and task.
- Esc uses the existing subdued text style. Mixed attachments share responsive horizontal cards
  with close controls above their previews; image cards retain sharp native pixels.

### Fixed (`2026-11-04i`)

- Native Windows image previews draw original decoded pixels with smooth scaling instead of
  character-cell mosaics. Compact thumbnails stay small; the enlarged viewer retains screenshot
  detail beyond the old 256-pixel thumbnail limit. Provider attachments retain their original bytes.

### Fixed (`2026-11-04h`)

- Composer image previews use four rows and twelve columns of pixels, with concise dimensions
  and format beside them. Clicking the thumbnail or attachment chip still enlarges the image.
- The Windows window caches rendered rows, reuses its drawing surface and combines mouse movement
  within a cell. Native typing no longer waits for terminal paste inference.
- Updated stock npm launchers for Codex and Claude run their declared entrypoints through Node
  without shell argument parsing. Image inputs no longer depend on a particular bridge hash.
- Input or launch failures before a provider starts release the current workspace reservation.
  Confirmed startup still retains ownership until the native process is known to have exited.

### Added (`2026-11-04g`)

- Windows starts Concordia's own native window by default, reusing the existing chat interface.
  Its own keyboard and drop events provide Ctrl+V, Ctrl+Shift+V and image-file drops without
  changing terminal settings, intercepting other applications' keys or installing hooks.
- Clipboard images and copied file lists are captured through bounded in-process Windows APIs,
  avoiding PowerShell startup delays for paste. Attachments retain their immutable snapshots.
- `concordia --terminal` keeps explicit terminal-pane mode; bundled WezTerm launchers and updates
  preserve that choice. Provider authentication, conversations and native approval ownership are
  shared by the existing application loop.

### Fixed (`2026-11-04f`)

- Completed turns reconcile final tool cards even after earlier cards streamed. A clickable
  Review activity row and Ctrl+E reopen reported thinking and edit details after completion.
- Async option prompts accept the host's title/string-choice schema and preserve normal replies.
- PNG-only clipboard images are recognized. Delivered empty paste events can capture clipboard
  images, and unbracketed dropped image paths attach automatically. Terminal shortcuts still
  need to reach the application; host-level paste interception is a separate configuration.

### Fixed (`2026-11-04e`)

- Standalone pasted image paths attach and preview automatically. Typing while an image imports
  no longer discards the attachment. Copied Explorer images are recognized. F9 pastes a clipboard
  image or opens the attachment chooser.
- Codex edit cards recover successful patch bodies from the exact current native session when
  its progress events report only changed paths, showing actual added and removed lines.

### Fixed (`2026-11-04d`)

- Cursor ACP continuation keeps replayed session history out of the new reply, thinking stream
  and active tool cards. The exact native session is still loaded before sending the new prompt.

### Fixed (`2026-11-04c`)

- `/update` ignores unrelated Rust workspaces and terminal configuration trees when choosing
  Concordia's source checkout. The recorded install source remains available from those projects.

### Fixed (`2026-11-04b`)

- Claude native stdio waits for local background agents and workflows to settle, including
  follow-up output and late approval requests. Interrupted or uncertain tasks require reconciliation.
- Agent inspection tracks native task completion instead of treating a background launch receipt
  as a finished child. Native task descriptions and result summaries remain available to inspect.

### Fixed (`2026-11-04a`)

- Qualifies Claude CLI 2.1.318 against the matching official SDK control contract. Approval,
  question, PDF and context gates share one version identity; cached 2.1.317 measurements become
  unknown until refreshed. The installed CLI patch no longer leaves these features unavailable.

### Added (`2026-11-03z`)

- Claude 2.1.317 native `AskUserQuestion` uses foreground and worker review, including up to four
  questions, two to four choices, multi-select and free-text replies. Space/click toggles and
  Enter confirms. Original questions and native request identities are retained; answers return
  as strings. Malformed and cancelled requests receive no grant. HTML previews are not rendered.

### Added (`2026-11-03y`)

- Native PDF attachments for qualified Claude CLI 2.1.317, including mixed ordered image,
  text and PDF content, durable queued snapshots, F9 file-path attachment and explicit viewer
  opening. Limits are 8 MiB per PDF, 16 MiB of PDFs and four attachments per message.
- PDF dispatch verifies snapshot hashes and refuses unsupported providers before a model launch.
  Native payload fixtures establish transport behavior; live PDF model processing remains unverified.

### Fixed (`2026-11-03x`)

- Windows installation retries temporary sharing/lock violations during executable replacement,
  with a bounded deadline and rollback to the previous installation on failure.

### Added (`2026-11-03x`)

- F10 manages selected local integrations: connect a manifest, reconnect, verify, or disconnect
  one binding after reviewing its revision, target and command. Work runs in the background;
  stale reviews stop, drafts survive, and cancelled actions require reconciliation.
- Opt-in Claude native stdio on CLI 2.1.317 routes allow-once/deny decisions through foreground
  and worker review, with connection ownership, cancellation and duplicate protection.
- Claude context policy can use post-turn native measurements bound to an explicit model and
  unchanged native transcript. Changed sources/settings invalidate them; no metadata probe sends
  a model prompt. Cursor capacity and unverified PDF transports remain unavailable.
- Completed structured Claude Write/Edit and Cursor write/edit results join the existing file
  output catalog. Opening still revalidates the current local file and its execution directory.

### Added (`2026-11-03w`)

- Left Arrow opens Home when the composer is empty; Right Arrow returns to the chat. Alt+Left
  and Alt+Right navigate with a draft present. Ordinary cursor movement, focused file/queue
  editors, goal input and native/modal decisions retain their own arrow handling.

### Added (`2026-11-03v`)

- A header arrow opens Home from an existing chat without discarding its messages, draft, or
  running turn. The return arrow or Esc restores the chat. Clicking the credit row opens balance,
  availability, native-source and refresh details, with a direct Codex balance-dashboard action.
  Missing numeric balances are labelled explicitly rather than reduced to an availability label.

### Changed (`2026-11-03u`)

- Header emails are hidden until clicked and can be hidden again; account changes reset disclosure.
  The redundant animated header icon is removed. Foreground chats can run concurrently in the
  same workspace, with the same or different providers. Explicit scheduler scopes and per-run
  recovery checks remain enforced. Native Codex usage prefers its current per-limit bucket and
  links to the provider dashboard for credit details that the native response omits.

### Fixed (`2026-11-03t`)

- Image attachments now show a thumbnail above the editable composer automatically, with a
  monochrome fallback and a click target for the enlarged view. Credits have their own persistent
  header row beside subscription limits; missing balances and separately unavailable credit spend
  are stated explicitly. App branding and update activity use the new geometric glyphs.

## Earlier implementation notes

- GIF links retain the explicit system-viewer fallback. A read-only candidate evidence collector
  binds executable hashes, build source, environment and bounded check receipts without publishing.

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
