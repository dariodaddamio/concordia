# UI glossary

This glossary lists each
visible run phase, tool toggle, worker mark, transcript gutter, notice and terminal-tab state.
The phase text is authoritative when a terminal font cannot render a symbol.

## Account identity

The top bar shows the active nickname and provider/account, followed by its email. If the provider
has not reported an email, it says `email unavailable`; Concordia never guesses it from a nickname.
Model and build branding yield space to identity on smaller terminals. Very narrow terminals can
still clip long identities. `/usage` shows email and subscription plan beside each account name.
Workspace/organization is separate from plan. Missing fields say `not reported`.

`/use rename codex Work` saves a local display name. `/use rename codex` resets it. Nicknames can
contain spaces (up to 48 characters), but not control characters. Stable IDs, login homes, native
session ownership and configured handoff order remain unchanged. The header, account lists,
completion descriptions and active worker strip use the nickname. Historical replies retain their
original labels. Email is account metadata read through the existing local identity reader; it is
cached for the selected binding rather than read during rendering.

## Animation and elapsed time

Waiting means no first/next structured output has arrived. Thinking and tool phases reflect
provider events. Writing means answer text is arriving. The spinner cannot establish whether a
silent provider is computing, waiting on its service, or stalled. The timer is elapsed wall time
for the current turn, displayed as `20s`, `8m 20s`, or `1h 02m 03s`. It resets for each new turn,
not each phase, and is neither a countdown nor a billing measure.

## File and chat references

The [workspace pane guide](workspace-panes.md) covers both routes. Clickable
paths and `/open FILE[:LINE[:COLUMN]]` share the same internal file pane. Prefixing a path with `@`
is accepted, including `/open @README.md`. A click opens a pane; a drag selects text. File references
in the prompt remain provider-visible text, rather than silently reading/attaching local contents.
The file pane is read-only, bounded to three files, 2 MiB and 50,000 UTF-8 lines per file.
`/open` starts in raw mode; `/preview` requests formatted Markdown or pretty-printed JSON.
Press `v` or click the view bar to toggle. Unsupported formats stay raw. Terminal controls are
removed before rendering. Divider hover/drag requests `ew-resize` or `ns-resize`; close and view
controls request `pointer`. Unsupported terminal hosts may ignore these OSC 22 requests.

Each `/usage` and `/use` invocation requests fresh limits, coalescing only an already running
check. Open rows update when the background result arrives, keeping filter and selection.
Cached values are marked while checking or after an error. Codex re-reads provider-written
local files; server changes are only available after the CLI writes another reading.

Prompt `@` completion combines current-chat file references with recorded Concordia chats.
`/open` without an argument shows the current-chat file list. Ctrl+P or a file-row click previews;
Tab/Enter inserts a literal reference and leaves the draft unsent. Discovery uses bounded recent
messages/tool cards, deduplicates canonical paths and excludes automatic credential suggestions.
It does not scan the repository or silently attach file contents. Sending a selected chat
reference supplies its bounded map; a side-by-side chat viewer and repository-wide file search
remain planned. See [workspace requirements](workspace-panes.md) for implemented preview
controls and planned editing, and [memory design](context-handoff-plan.md) for native-source retrieval.

## Unfinished requests and previews

Restart restores unfinished work paused, with its original source identities and a separate newer
draft. `/continue` reviews the request and target choices; `/use` also has a continue row. No turn
is sent merely by reopening the app. In a focused file pane, `/` searches, `n`/`N` navigate matches,
and `r` reloads. A changed-on-disk indicator comes from background metadata checks. These controls
never send a chat prompt. [Native memory](native-memory.md) defines the limits and recovery rules.
