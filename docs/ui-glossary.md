# UI glossary

This glossary lists each visible run phase, tool toggle, worker mark, transcript gutter and notice.
The phase text is authoritative when a terminal font cannot render a symbol.

## Tabs and terminals

Tabs have padded close controls and a hand cursor over clickable labels/close areas. Double-click
a tab label to rename that conversation; the title editor preserves the independent message draft.

F11 or `/terminal` opens a local shell in the workspace; F11 returns to chat. Hiding keeps the shell
alive, while `/terminal close` or Ctrl+Shift+W in the shell stops it. Ctrl+C inside the shell interrupts
the shell command; it does not cancel the model's task. Shell sessions are transient and are stopped
when their owning tab/window closes.

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
File panes are bounded to three files, 2 MiB and 50,000 UTF-8 lines per file.
`/open` shows formatted Markdown or pretty-printed JSON; source files open raw.
Alt+V or the view bar switches modes. Formatted view is read-only; raw mode edits with explicit
Ctrl+S save and dirty-close decisions. Terminal controls are
removed before rendering. Divider hover/drag requests `ew-resize` or `ns-resize`; close and view
controls request `pointer`. Unsupported terminal hosts may ignore these OSC 22 requests.

Each `/usage` and `/use` invocation requests fresh limits, coalescing only an already running
check. Open rows update when the background result arrives, keeping filter and selection.
Cached values are marked while checking or after an error. Codex re-reads provider-written
local files; server changes are only available after the CLI writes another reading.

Prompt `@` completion combines current-chat file references with recorded Concordia chats.
`/open` without an argument browses chat references, recent previews and bounded workspace files.
Ctrl+P or a file-row click previews;
Tab/Enter inserts a literal reference and leaves the draft unsent. Discovery uses bounded recent
messages/tool cards, deduplicates canonical paths and excludes automatic credential suggestions.
Reference completion does not silently attach file contents. Sending a selected chat reference
supplies its bounded map. File-pane find searches the open file with visible bounds and explicit
replacement previews. A side-by-side chat viewer remains planned. See
[workspace controls](workspace-panes.md) and [memory design](plans/context-handoff-plan.md).

## Unfinished requests and previews

Restart restores unfinished work paused, with its original source identities and a separate newer
draft. The inline continue action reviews the request and target choices; `/use` contains account actions only. No turn
is sent merely by reopening the app. In a focused file pane, `/` searches, `n`/`N` navigate matches,
and `r` reloads. A changed-on-disk indicator comes from background metadata checks. These controls
never send a chat prompt. [Native memory](native-memory.md) defines the limits and recovery rules.

Double-click a tab or press F2 to edit its title in place. The title has its own editor; the message
draft, attachments and editor state stay visible and unchanged. Enter saves; Esc cancels.
Switching tabs or clicking outside cancels the pending title edit. `/rename TITLE` changes the same local title;
`/rename` resets it to automatic naming. F1 opens shortcuts; `?` opens them only from an empty,
idle composer, so question marks remain ordinary text in drafts and focused editors.
