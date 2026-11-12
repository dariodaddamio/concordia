# Configuration and custom commands

## Provider questions

Provider questions always reach the user through the question broker and native provider paths.

Cursor ACP receives a per-turn `concordia-ask` MCP server in both new and loaded sessions.
Codex uses its app-server request channel because `codex exec --json`
cannot answer a pending `request_user_input` event. Claude's native stdio transport normalizes
`AskUserQuestion` directly into the same popup broker.

## Limit continuation and unfinished work

In `/use`, **next provider for this session** opens choices to pause at a limit, choose
automatically, or try a specific connected provider account first. `/continuation` opens the
same screen. These choices apply to the current window without switching the active turn or
changing the saved default. A selected account takes priority over configured model/account
preferences; if it also reaches a limit, bounded automatic selection continues through the
remaining targets. `/continuation manual|automatic` changes the session mode directly and clears
the session's preferred account. Choosing a policy does not send an already paused request;
use `/continue` to review and continue it.

The limit handoff row in `/use` pauses or automatically continues when usage runs out. A paused
request offers account and cached-model choices. The toggle is saved for future windows. Persistent defaults:

```toml
[continuation]
mode = "manual" # manual | automatic
accounts = ["codex-2", "claude-1"] # choose ids shown by /use, in preferred order
models = [] # optional model ids from /model; tried on the current account first
context = "provider" # provider (native compaction, default) or fresh
codex_context = "provider" # provider | fresh; per-provider override
claude_context = "provider" # provider | fresh; per-provider override
cursor_context = "provider" # provider | fresh; fresh is disabled without a native usage signal
context_threshold = 85 # fresh-session threshold, 50..=95 percent
same_provider_automatic = false # opt in only after accepting the provider terms notice
automatic_handoff_cap = 3 # maximum automatic handoffs into one account per local day
```

`fresh` uses only the provider's reported current-turn context usage and rotates at a turn
boundary. The new native session receives the bounded map and resolvable source pointers. If a
provider does not report a verified context window, fresh mode stays disabled and the reason is
shown in `/config` and the turn notice. The setting is available in `/config` → Continuation;
native provider compaction remains the default.

A provider error explicitly reporting an exhausted usage limit triggers quota handoff; ordinary
errors, cancellation and model prose do not. A recognized terminal context-capacity error can
instead roll over once on the same account, with native-source pointers. It does not disable
provider compaction or infer capacity from billed tokens. Each account/model pair is tried once, with at most
three automatic handoffs per request. With an empty account list, automatic mode tries the other
connected accounts; when both preference lists are empty, it can then try another model from the
current account's native catalog. Explicit model preferences must exist in that catalog. Unknown
capacity is not treated as free capacity: another model may share the same quota and fail too. An unsent draft is preserved across automatic continuation. An open picker, background job or
queued edit defers handoff until it closes; still-running/uncertain tool completion pauses it with an explicit reason. Inspect uncertain results before continuing. Structured quota codes are retained even when the accompanying provider message is generic.

Handoff keeps the conversation and title, starts a new native session in the selected account's
home and sends the bounded map plus revision-checked native-source pointers where supported,
with an instruction to reconcile completed actions. Unfinished requests survive restart separately
from newer drafts and queues, paused for explicit continuation. `/continue` reviews the request;
`/continue ACCOUNT [MODEL]` chooses a target. See [native memory](native-memory.md).
Native tool/approval state is not transferred. Queued prompts remain paused for explicit resume.
There are no model calls for quota checks or summarization. Codex limits use the supported read-only `account/rateLimits/read` interface; no thread or turn is created. `/continue ACCOUNT MODEL` explicitly chooses a
cached model after a usage failure. The `/use` handoff toggle is stored locally and overrides the
config default for future launches.

## Titles, resume and agent reports

Session titles are generated deterministically from the first substantive request, without a
model call. They are shown in the header, terminal title and resume picker, and saved under
`<state>/.ao/session-titles/` independently of `[threads].logging`. Each record is bound to the
provider, account, normalized home and native session id; provider-owned files are not rewritten.

`/resume` loads up to 200 recent Codex/Claude user/assistant messages within 1 MiB of visible
text, reading at most an 8 MiB native tail. Loading runs off the input/render thread; the draft
is retained and sending waits for completion. A truncated view is labeled. Cursor uses bounded
read-only ACP session replay when available. Loaded scrollback is for display and is not replayed into
the selected native session. Native tools and approvals are not restored from this text.

Ctrl+G lists reported native child-agent activity and Concordia worker jobs. Enter
opens the report; no model call is made by inspection. Native report detail depends on provider
events. Delegated-agent account selection is owned by the always-on Agents MCP tool.

Cancellation leaves the prompt box unchanged. Use sent history to recall a submitted request
explicitly; queued work stays paused until resumed.

## Internal file panes and terminal editor

Click an underlined file reference in a sent user message or assistant reply to open it in an
internal file pane; a drag still selects transcript text. Alt+click also previews. Paths can
include a line/column (`src/main.rs:12:3`) or Markdown line fragment (`#L12`). Markdown labels
keep the explicit destination. `/open PATH` opens up to three file panes beside
chat; reopening a file focuses its existing pane. Click to focus, F6 cycles focus, Esc returns to
chat. Markdown/JSON default to formatted, read-only view; Alt+V or the view bar opens raw editing.
Code always opens raw with syntax colors. Ctrl+S saves; Ctrl+Z/Ctrl+Y undo/redo; Ctrl+F finds.
The title shows a stable pane id and a star for unsaved edits. Click X to close; unsaved work
asks S save / D discard / Esc cancel. Drag the main divider or
the bottom border between file panes to resize. Files are read asynchronously, limited to 2 MiB
and 50,000 lines each, and must be UTF-8 text. In raw mode typing/Enter/paste edit the file;
formatted mode consumes those events without editing. Small terminals hide panes without losing
content. Pane-owned recovery restores file buffers, cursor, mode and split layout; external
changes are retained and block conflicting saves. Recovery does not restore the undo stack.

The file action picker can open a configured GUI editor. Missing paths, preview errors or launch
failures are shown in Concordia. References never run as shell commands.

Concordia finds an installed `nvim`, `hx` or `vim`, or uses your explicit executable and argv:

```toml
[editor]
program = "nvim"
args = ["+{line}", "--", "{file}"]
```

`{file}`, `{line}` and `{column}` expand within individual arguments; the executable is one
path, not a shell command string. If `{file}` is absent the file is appended as one argument.
Input is UTF-8 and capped at 2 MiB. See [workspace requirements](workspace-panes.md)
for the editing/recovery and search/replacement stages.

## Native chat access from the CLI

`concordia chat list --json` lists recent native chats across the user's discovered/registered
accounts, labeled with provider, account and session ID. Filter with `--account ID` or
`--provider codex|claude|cursor`; `--limit N` is capped at 1000. `chat search TEXT` searches
preview/ID metadata only. `chat read SESSION --account ID` reads Codex/Claude user/assistant
text with optional `--tail N`, `--grep TEXT`, and `--max-chars N`. It does not select an account,
resume a session, copy credentials, or transfer context to a provider. Explicit reads resolve in
the named account rather than the newest-list window. Discovery caps at 20,000 entries, 5,000
files and 32 accounts; listing summarizes at most 1,000 records, reading at most 64 KiB per summary.
Native UUID reads filter filenames before summarizing. A scan that cannot establish a unique
match reports its bound rather than claiming absence. List JSON preserves exact identity fields,
clips display metadata and caps output at 5 MiB. Reads use at most the last 8 MiB of a transcript. Tool records are
omitted. Cursor history uses the bounded read-only ACP session replay path; missing or unsupported
native history stays explicitly unavailable.

Concordia reads two optional things you write by hand, both under the state directory
(`%LOCALAPPDATA%\concordia`, or `CONCORDIA_STATE_DIR`). It never writes either of them.

## `config.toml`

`<state>\config.toml` (override the path with `CONCORDIA_CONFIG`). A missing file or key means the
default. A malformed file is ignored (defaults) and reported by `concordia doctor` with its line and
column; unknown keys and invalid values are warnings too (invalid values fall back to the default or are
clamped). Nothing here is ever fatal.

```toml
[ui]
theme = "default"       # default | dark | light | terminal | opifex
# background = "#101318" # optional #RRGGBB override
# foreground = "#e5e7eb"
# accent = "#78b9ef"
# brand_color = "#FFB000" # Running-window icon
motion = true           # reserved appearance setting; Home portrait is static
token_line = true        # per-turn "in 12.3k out 1.4k 8.2s" under each reply
tips = true              # a rotating tip while idle
persist_history = true   # native-source recall references; legacy prompt history remains readable
stats = false            # footer: mem 41 MB - cpu 1% (and the provider processes)

[threads]
logging = "off"          # opt-in metadata/native references under <state>/threads
map_chars = 1600         # size of the [concordia map] block, 200..=8000
awareness = true         # tell the CLIs in Concordia-created homes about [concordia map] (docs/mcp.md)
mcp = false              # register `concordia mcp` in Concordia-created homes (opt-in, costs tokens)

[mode]
default = "ask"          # plan | ask | workspace | full

[delegation]
protocol = false         # let the orchestrator propose delegations (you still approve)

[turn]
idle_timeout_minutes = 10 # notify after this much provider silence; 0 disables it
```

| Key | Default | Notes |
| --- | --- | --- |
| `ui.theme` | `"opifex"` | `default`, `dark`, `light`, `terminal` or `opifex`; also selectable live in `/config` |
| `ui.brand_color` | Amber `#FFB000` | Optional running-window icon color |
| `ui.motion` | `true` | Retained for config compatibility; the Home portrait is static |
| `ui.background` | Theme default | Optional quoted `#RRGGBB` background, including empty rows and native client margins |
| `ui.foreground` | Theme default | Optional quoted `#RRGGBB` main text color |
| `ui.accent` | Theme default | Optional quoted `#RRGGBB` links, focus and navigation color |
| `ui.token_line` | `true` | Dim usage line after each reply |
| `ui.tips` | `true` | Tips while the prompt is idle |
| `ui.persist_history` | `true` | Stores native prompt-recall references (last 500); `false` keeps new recall in memory only |
| `ui.stats` | `false` | Footer memory and CPU for Concordia and the agents it started (`/stats` toggles it) |
| `threads.logging` | `"off"` | Explicit `"on"` enables metadata/native-reference recording; existing logs are preserved |
| `threads.map_chars` | `1600` | Out of range is clamped to 200..8000 |
| `threads.mcp` | `false` | Registers the `concordia mcp` server in those homes; `false` removes the entry (see [mcp.md](mcp.md)) |
| `mode.default` | `"ask"` | Anything else falls back to `ask` |
| `delegation.protocol` | `false` | Enables approval-gated delegation proposals from an orchestrator reply |
| `turn.idle_timeout_minutes` | `10` | Notification interval for provider stdout/stderr silence; `0` disables notifications. |

### Themes and custom colors

Use **`/config`** to open Settings, then Appearance (or `/config appearance`). Select Theme or Concordia color with the mouse or
arrow keys and Enter. Changes apply immediately and survive reopening. The color menu includes
presets and a custom `#RRGGBB` input; it changes the running-window icon; Home retains the original gold portrait.
The installed executable's Explorer icon remains the bundled original.

Themes are local TOML files under `<state>/themes`; there is no marketplace or network lookup. A
theme file is at most 16 KiB and must contain nonempty `name` and `author` strings plus
`background`, `foreground`, `accent`, `dim`, `success`, `warning`, `error`, `selection`, and
`border` colors as `#RRGGBB`.
`portrait_tint` is optional. Unknown keys and malformed colors are rejected with a line-aware
error. Built-in Dark, Light, and Opifex Studio themes use this same parser. Foreground contrast
below WCAG AA 4.5:1, or dim contrast below 3:1, loads with a warning in the picker.

In `/config` → Appearance → Theme, choose a built-in or imported library theme. **Import theme...**
validates a `.toml` before copying it into `<state>/themes`; **Export current** writes the active
theme there or to a path supplied by the user, including configured palette overrides. Default
and Terminal do not expose the host terminal palette; select a named theme before exporting.
Opifex Studio is the default for new installs. Loading appearance settings preserves explicit
theme/config choices without an old-theme migration or startup file rewrite.

Direct commands are `/config theme opifex`, `/config color #78BFFF` and `/config motion off`.
**Opifex Studio** uses black `#000000`, amber `#FFB000` and bone `#D8D0C0`
palette. Build 05d restores the original static gold Home portrait and removes the 3D renderer.
Small windows omit the portrait to preserve chat space. Saved motion preferences remain readable
for compatibility and do not animate the portrait. Build 05e uses the motion preference for
the native window icon's activity orbit, with Windows reduced-motion taking precedence.

### Settings scope and persistence (05e)

`/config` includes response usage, idle tips, CPU/memory display, icon animation, permissions,
accounts, models, continuation, worker delivery, file-link behavior, skills, hooks and integrations.
Display switches save immediately in `.ao/appearance.json` and apply on reopening. Existing
account/model, permission, worker-return and file-action controls keep their existing app-owned
storage. Environment switches such as `CONCORDIA_TIPS=0` still take precedence. Changes to
`config.toml` apply after restart; the UI never rewrites that manually maintained file.

Default limit handoff is saved across sessions; **This session's handoff** is explicitly temporary.
Changing a default does not grant tools, alter an active turn's permissions or submit pending work.
Automatic worker-result delivery can start a main-agent turn when idle, as its row explains.

`/skills` and `/hooks` open searchable local source inventories for the selected provider.
**Project** entries apply to that repository; **User / selected account** entries belong to the
selected native provider home and apply across its projects. Shared-user and installed-system
skill locations are labeled separately. These sources are owned by the provider, not a new
Concordia hook/skill runtime. Enter opens a source file for inspection; it invokes nothing.

Skills scan provider project directories up to the repository root, selected-home skills,
Codex's installed system folder and shared `.agents/skills` user folder. At most 512 directory
entries are visited (256 per folder); source files are bounded to 64 KiB. Linked sources are
not followed. Hooks show event/group metadata from Claude settings or Cursor hook files without
printing or executing commands. Codex hooks have an explicit unverified-catalog notice.
Configured sources do not prove activation in an existing native session. Plugin, managed-policy,
remote and dynamically loaded sources are not exhaustively enumerated. Reopening refreshes locally.

Provider scope references: [Codex skills](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills),
[Claude skills](https://code.claude.com/docs/en/skills#choose-where-skills-load) and
[Claude hook locations](https://code.claude.com/docs/en/hooks#hook-locations).

UI choices are saved atomically under `<state>/.ao/appearance.json`; Concordia never rewrites
your `config.toml`. Saved appearance choices override corresponding config defaults. Choosing a
theme preset replaces configured background/foreground/accent overrides; changing just the
Concordia color preserves the chosen UI palette. **Use config defaults** clears appearance choices
and restores the hand-edited defaults. Unsupported saved versions are reported and preserved.

`default` preserves the existing appearance and terminal host colors. `dark` supplies an explicit
near-black background; `light` supplies a warm light background with dark text. The same palette
covers chat, menus, file panes, detached file windows and native client margins. Added/removed diff
rows keep distinct green/red tints; syntax colors are adjusted for readable contrast. Original
image pixels are preserved, with transparent image areas composited onto the selected background.

For a custom dark palette:

```toml
[ui]
theme = "dark"
background = "#101318"
foreground = "#e5e7eb"
accent = "#78b9ef"
```

Restart Concordia after editing the file. Omit any override to use its preset. Invalid names/colors
produce `doctor` warnings and fall back. Against a known background, foreground contrast below
4.5:1 and accent contrast below 3:1 are rejected; the rendered palette also adjusts muted and syntax
text for contrast rather than dimming it twice. Bold, italic and selection still carry meaning.

`terminal` keeps host colors and applies only explicit overrides to ordinary UI defaults and links;
it cannot measure a terminal's background. Native Windows has no terminal host and uses its normal
native colors for omitted overrides. `NO_COLOR` takes precedence over all themes and overrides.
This changes display colors only, without editing provider settings or authentication.

### Precedence: store vs config

Config supplies defaults only. `store.json` holds what you did inside Concordia, and a value saved
there wins:

- Permission mode: `[mode] default` applies only until a mode has been saved (first launch). After
  that `/mode` and Shift+Tab own it.
Delegation protocol writes the store; until you use it, the config value applies. Provider child processes get
`CARGO_BUILD_JOBS`, `MAKEFLAGS`, `npm_config_jobs`, and `CMAKE_BUILD_PARALLEL_LEVEL` defaults
defaults of one to four build jobs, based on logical cores, only when the user or provider has not set them. Host capacity is read
locally; no credentials or hosted service is involved.

Read the effective values with the `config` and `commands` rows of `concordia doctor`.

## Custom commands

`<state>\commands\<name>.md` becomes `/<name>`. Names are `[a-z0-9-]`, 1 to 32 characters; at most 64
files of at most 16 KB are read (others are skipped with a doctor warning). Front matter is optional:

```
---
description: review the staged diff
---
Review the staged diff for bugs and missing tests. Focus: $ARGS
```

`/review the parser` shows as typed and sends the body with `$ARGS` replaced by `the parser`, as an
ordinary prompt (history, trust mode and providers behave as for any message). If the template has no
`$ARGS`, typed text is appended as a new paragraph. Only `$ARGS` is replaced: no shell, no includes, no
environment expansion. Commands appear in the `/` palette with a dim `custom` marker. Built-in commands
always win a name clash; `concordia doctor` lists clashes. Files are scanned when Concordia starts.

## Question choices, queue sending and file discovery (`2026-11-01d`)

Recognized conversational questions with numbered/lettered/bulleted options and question-tool card
inputs show a reply panel below chat without taking typing focus. Click an option, or use F8 then
arrows and Enter. Digits typed in conversational replies remain text. Typing/pasting in the panel
selects the note-only row; move to an option afterward to send both. Notes preserve line breaks
and Shift+Enter inserts one. Esc returns to the composer while retaining the note.
The answer is a normal follow-up message,
queued durably and sent after any active turn stops. The existing draft remains intact. This
panel does not answer native provider approval RPCs or transfer their tool state.

Every queued row has Send now. Ctrl+Enter or Alt+Enter on a focused, unedited row and
`/queue send [N]` perform the same action (N starts at 1, default first). Cancellation finishes
before dispatch; exactly one selected item starts, and the rest stay paused. A queued edit must
be finished or cancelled first.

`/open` browses current-chat references, workspace-specific recent previews and bounded workspace
files/folders. Relative paths start at Concordia's cwd; quoted absolute paths are supported.
`/open docs/` and `@docs/` suggest paths. Tab completes; Enter opens `/open` results and inserts
`@` references. Ctrl+P previews the selected `@` file without sending the draft. Browsing does not
change the provider cwd or account. Recent history is app-owned path metadata, scoped to a
canonical workspace; sensitive paths are excluded. Workspace traversal respects ignore rules,
skips generated/provider folders and symlinks, and is capped at 12,000 visited entries, 4,096
results and depth 14 (one level without a project marker). Explicit paths remain available.

Run status and elapsed time sit below chat with a styled clickable Esc control. Completed
responses retain Worked for and a local stopped timestamp, including when token metrics are off.

## Images, diagrams and update history (`2026-11-01e`)

File actions now share a resolver. F4 on a file-browser row or focused pane, or Alt-click on a
transcript file link, offers Preview here and Open in editor. Always choices persist in
Concordia-owned settings. `[ui].file_action = "preview" | "editor"` supplies a default. The
configured editor opens as a native GUI executable. See [file actions and independent recovery](quality/file-actions.md).

`/attach` opens the image chooser; `/attach PATH` imports a snapshot; `/attach clear` removes draft
images. Ctrl+V reads clipboard images in the ordinary composer, with text paste as fallback.
Chips preview in the system viewer and have X removal controls. Draft/queue recovery stores a
versioned message envelope with image references, not image bytes in text. See
[image transports and limits](images-and-diagrams.md).

Mermaid renders automatically for complete fences in chat and formatted Markdown, with bounded
background work and source fallback. Metadata timing/token rows are deliberately not linkified.

Update pages keep earlier history hidden by default. H toggles it; o opens a configured release
page when available. Enter retains its existing install/restart action. Color distinguishes fixes
from additions/changes and removal/deprecation notes without repeating progress in the composer.
