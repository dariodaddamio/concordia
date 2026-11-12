# Chat rendering and input

Guide baseline: build `2026-11-08h`. See [verification](e2e-status.md) for the current build and checked scope.
Build history belongs in [CHANGELOG.md](../CHANGELOG.md); scoped verification and its
limits are in [verification](e2e-status.md), which also records cleanup closeout and historical retrieval.

## Conversation and input ownership

Windows uses an app-owned GDI window around the shared Ratatui layout and event loop.
The native window uses the same conversation/provider logic with the application renderer. Native
windows own clipboard, drop, mouse, focus and resize events. They are not a complete
native widget or accessibility toolkit. Pixel images use smooth scaling in the native
window; terminal previews use cells. Rendering does not alter provider attachments.

Home navigation reuses an unbound Home tab and returns to the originating chat. Left/Right,
Escape and the header button navigate; Ctrl+T explicitly creates a new tab. Drafts, screenshots,
queues and running turns remain with their original tab. Resuming history closes Home. Selecting
an account prepares its binding without sending a message or context; selecting the same account
retains its model. Completed background history reads request repaint. Reopened tabs accept
new provider handles; closing a tab with active workers requires that tab's own confirmation.
After submission, `Context for this message` shows the context attached to that request.

Closing and reopening restores the most recently closed window for the same workspace:
open-tab order, selected tab, account/model bindings, scroll position, saved conversation views
and each tab's independent draft/queue. A held window lock keeps concurrent windows independent.
Unfinished requests remain paused alongside the conversation; reopening never starts a provider
turn or restores native approval grants. Update restart releases the old recovery owners first.

Window layouts live under the workspace's pane-recovery root and reference exact pane record IDs.
Display snapshots retain at most 1 MiB per tab and share an 8 MiB text budget; omissions are visible
and original native sources remain available. A restored snapshot shows the view left on close;
use explicit session resume to reload later changes made through an external native CLI.
Layout encoding/reads are capped at 32 MiB and 512
tabs. Unsupported or unreadable layouts remain unchanged with a warning. Pre-layout builds did
not save tab order; an existing interrupted record can recover its exact bound history, while
multiple older orphaned records remain retained and are restored into their own tabs when capacity allows.

Open modal controls own their keys. Esc dismisses saved-draft offers for the saved revision
without deleting work. Attention and
Ctrl+N open pending questions, approvals, queued work, worker results or account controls.

While a turn runs, Enter first preserves the draft and explains the interrupt action. A second
Enter within five seconds interrupts and sends after settlement; Tab queues instead. Editing,
another key, mouse input or a changed turn resets confirmation. Failed/cancelled turns pause
queued sends. [Queue and retry ownership](tui-states.md).

Clipboard and bracketed paste preserve logical LF, CRLF and CR-only line breaks, indentation,
blank lines and trailing whitespace. Prompt tabs expand to four spaces; raw files preserve tabs.
The composer scrolls independently: wheel or Alt+PageUp/Down changes its viewport, while editing
and caret movement restore caret following. Rich HTML/RTF paste formatting is not supported.

## Questions and attachments

Completed Codex assistant messages keep their native boundaries. A completed conversational
question can offer reply choices while later work continues; subsequent commentary stays outside
the options. Inferred choices require a question/invitation followed immediately by top-level
choices. Example JSON and ordinary status prose remain content; only question-tool cards parse
structured question JSON.

Conversational choices never take keyboard focus automatically. F8 or a click enters the panel;
arrows/Enter select an answer. The note row accepts typing/paste, Shift+Enter inserts a line,
and Esc returns to the composer while retaining the note. Sending a note alone preserves its
text; selecting an option after writing a note includes both. Composer drafts remain intact.
Replies are follow-up user messages. Native questions and permissions retain their own response
channel, identity and approval rules. [Provider connections](how-concordia-connects-providers.md).

F9 imports an offered attachment, pastes a clipboard image, or opens the file chooser. Standalone
pasted PNG/JPEG/WebP paths attach automatically. Cards show thumbnails and remove controls;
clicking opens the enlarged viewer. Queue editing reuses those controls. Enter saves and Esc
cancels; the separate composer text and attachments are restored in either case. Restart recovery
retains committed queue content, unfinished edits and composer attachments separately. Saving
waits for a pending import. [Formats and limits](images-and-diagrams.md).

## Transcript and historical evidence

User text, provider replies, thinking, tools and system notices remain distinct. Recorded thinking
expands inline by heading/gutter; Ctrl+O toggles activity. Short thinking shows whole when a toggle
would hide nothing. Ctrl+E inspects raw tool output. Worked-for metadata shows duration, stop time
and usage. No missing or provider-private encrypted reasoning is reconstructed.

Tool titles retain recorded status, paths and expandable details; links keep their own actions.
Diffs use reported hunk positions, syntax colors and subdued addition/removal backgrounds.
Unavailable positions stay unnumbered. Codex patch enrichment uses successful bounded inputs from
the exact native session/attempt. Ambiguous, failed or stale evidence stays unavailable; current
disk contents never become a historical diff. File-output catalogs identify reported artifacts
and do not by themselves prove a line diff was supplied.

History restoration is read-only and starts no provider work. Codex and Claude restore recorded
tool/thinking content; Cursor's current history reader restores text only. Reads are bounded to
the recent 8 MiB, 200 entries and 1 MiB of displayed history; omissions are reported as truncated.
Live text/thinking keep bounded projections with explicit omissions. At a safe idle boundary,
a verified captured conversation retains its latest 2,048 rows/16 MiB; native originals remain
available separately. Active inspections, unsaved work and unavailable-source views remain
protected. The reopen cache keeps eight clean conversations, retaining unfinished closed tabs.
Async image, search, memory and history completion requests repaint.
Model catalogs, integration readers, artifact details, worker results, file saves/highlighting,
native request expiry and Mermaid completion also request repaint without another input event.
Folded rows check source content and diagram revision even when message count is unchanged;
clearing a selection reconciles source updates, and Bottom/End returns to the actual tail.

Native recording failures preserve unfinished work and pause the queue. Reconcile partial actions
before using the inline continue action; recording failure is not a quota signal. [Recovery](continuity.md).

## Markdown and file navigation

Replies and formatted files share a small Markdown renderer. Tables wrap with borders or use
labelled fields in narrow panes, bounded to 16 columns and 256 rows per group. Table previews stop
at 4,096 rendered rows with an omission notice; original source remains available. Fenced code stays
literal; complete Mermaid fences render locally. This is not full CommonMark/HTML support.

Explicit Markdown destinations remain attached to their labels. HTTP(S) opens in the default
browser after a click; unsupported schemes stay literal. Local supported files open internally.
GIF/PNG/JPEG/WebP links use the system image viewer. Chat bare file references must exist relative
to the workspace. Explicit basename links also try its direct `docs` directory; there is no
recursive search. Formatted files resolve relative paths from the document directory and verify
bare candidates before underlining them, including inside tables. `/C:/...` chat destinations
resolve as Windows drive paths. Plain user prompts and raw file mode stay literal.

Supported documents default to formatted read-only view. Raw mode supports explicit editing and
save with syntax colors. [Workspace controls](workspace-panes.md). Appearance and icon preferences
are documented in [configuration](config.md); runtime update/install behavior is documented in
[provider/runtime reference](how-concordia-runtime.md).

## Mouse pointer shapes

Terminal mode requests pointer/text/resize shapes through OSC 22. Requested intent does not prove
the visible host cursor changed. The qualified `wezterm 20240203-110809-5046fc22` revision lacks
this parser support: see its version-pinned [OSC parser](https://github.com/wezterm/wezterm/blob/20240203-110809-5046fc22/termwiz/src/escape/osc.rs)
and [mouse handling](https://github.com/wezterm/wezterm/blob/20240203-110809-5046fc22/wezterm-gui/src/termwindow/mouseevent.rs).
Visible pointer behavior requires a supporting host and a physical-host check.
