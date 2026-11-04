# Chat rendering and input, 2026-11-01f

Build `2026-11-04n` renders reported thinking as Markdown, with plain readable shortened previews.
Tool headings keep their readable action when expanded and distinguish action, status, linked
paths, green additions, red removals and subdued Expand/Collapse controls. The footer permissions
menu separates each mode from its explanation and marks the current mode without clipped prose.
Explicit Markdown basename links resolve at the chat workspace first, then its direct `docs`
directory if the root file is absent. Subdirectory paths and plain text references stay exact;
there is no recursive file search. This does not repair already-corrupted provider text.

Build `2026-11-04m` keeps permissions in a compact popover attached to the footer mode control.
It overlays the bottom-right corner using the existing picker style, preserves the visible
composer, and dismisses on an outside click. Mode changes retain the existing next-turn policy.

Completed tool cards show Expand/Collapse on their title row; the title text and gutter expand
the recorded details. File/web links keep their actions and dragging title text selects it.
Reopening a Codex or Claude chat restores recorded native tool IDs, inputs/results/status and
readable provider thinking, with Review activity for completed turns. Encrypted reasoning remains
opaque. Cursor's current saved-history reader restores text only. Restoration is read-only and
starts no provider work. Source reads remain bounded to the recent 8 MiB, 200 entries and 1 MiB
of displayed history; omitted data is reported as truncated.

Windows defaults to an app-owned native window using the same Ratatui layout and event handlers.
It paints the existing cells, including thumbnail pixels, and owns clipboard paste, drops, mouse,
focus and resize events. `--terminal` remains available for terminal-pane workflows. Provider
turns, account/session identities and native approvals use the same application loop.
Image previews in the Windows window use original decoded pixels with smooth scaling; terminal
mode retains cell-based previews. Preview size and rendering do not alter provider attachments.

Build `2026-11-04j` uses responsive mixed-attachment cards with top-edge close controls,
subdued Esc text, and window titles ordered as application, account/model, actual phase and task.
The native renderer fits symbol/emoji fallback fonts inside their allocated cells. The same
Concordia mark appears in the titlebar, taskbar and Windows executable resource.

The owned window supplies default clipboard/drop ownership and pixel images while sharing the
existing provider and conversation UI. Its cell renderer remains a lightweight GDI host, not a
complete native widget/accessibility toolkit. DirectWrite remains a possible incremental text
backend: Microsoft documents font fallback and [GDI interoperability](https://learn.microsoft.com/en-us/windows/win32/directwrite/interoperating-with-gdi).
No frontend replacement is implied by these rendering fixes.

Codex patch recovery accepts bounded static JavaScript objects and structured tool output blocks.
Historical file-output inspection binds a unique successful immutable attempt receipt to its
completed native turn, then matches the exact sibling file set for the reported event. Selected
file-output cards recover asynchronously by full artifact identity; ambiguous or failed evidence
stays unavailable. Current disk contents never become an invented historical diff.

Build `2026-11-01f` renders complete Mermaid fences locally in chat/Markdown panes and adds native
image attachment chips. Timing/token metadata and reply headers are not file-link candidates;
bare measurements such as 12.3k and 8.2s are not guessed filenames. Full user rows, including
blank/trailing cells, have a subdued background and normal-weight body text. Paused input hints
reserve the same left inset as the normal prompt. Update history is collapsed behind H.

Build `2026-11-01d` gives user messages a background and `You` gutter. Live run status is below
chat, with elapsed time and a styled clickable Esc button. Status describes the current phase or
actual tool preview. Completed replies retain duration and local stop time. Bottom is absent
when already at the bottom. Command lists share horizontal inner padding. File-browser headings retain the end of a long
directory path with a leading ellipsis in narrow split layouts.

Conversational question choices are clickable and keyboard-selectable (F8, then arrows/Enter).
They never take focus from the message box automatically. They are offered only when a reply ends
with a question or explicit invitation immediately followed by top-level choices. Mentioning
"select an option" in a status update does not create a question. Intervening/trailing prose,
list items quoting a question, and example JSON remain content. Only question-tool cards interpret
structured question JSON. The complete original reply stays in the transcript, including long options.

A note row takes text after explicitly entering the panel with F8 or a click. Typing or pasting
selects this row, including when the text starts with a digit. Enter sends the note alone,
preserving its text without prepending the inferred question. To include an option, move to it
after writing the note and press Enter, or click it. Paste preserves logical line breaks;
Shift+Enter inserts one. Esc returns to the message box and retains the note. Existing composer
drafts remain intact when an option/note is sent. These replies are follow-up messages, not native
approval RPCs; native permission requests retain their explicit response and digit shortcuts.

Chat underlines a file reference only when it names an existing file relative to the chat's
working directory, or is an explicit Markdown link. Prose before a filename is not underlined.

The stock npm Codex bridge from package 0.165.0 is verified by exact shim shape, manifest and JS
SHA-256 before resolving its own unmodified vendor executable. Unknown or changed bridge content
uses the normal launcher. This avoids the verified Node entrypoint; physical disappearance of
all transient console windows is not established by process-plan tests. No unrelated Node
processes are terminated.

Build `2026-11-01c` defaults supported documents to formatted view. Raw source supports editing
and syntax colors; formatted content stays read-only. See [workspace controls](workspace-panes.md).
Continuation no longer repeats internal provider handoff instructions as a user chat bubble.
Headless provider launches request Windows `CREATE_NO_WINDOW`; interactive sign-in is separate.

Build `2026-11-01b` adds shared modal exit controls. Command screens replace the message-box
contents with an Esc/X cue and explain whether typing filters or searches; the actual draft is
preserved. The X requires a press and release on the control and performs the same close/cancel
action as Esc. The update page includes this paused message box. Pinned expanded tool headers
show the command preview and output count. Explicit GIF/PNG/JPEG/WebP links use the system
image viewer, while supported text files keep internal previews.

Assistant replies and formatted Markdown files share a renderer. Pipe tables show bordered,
wrapped columns when space permits and labeled fields in very narrow panes. Table recognition
is bounded to 16 columns and 256 data rows per group. Fenced code stays literal. This remains
a small Markdown renderer, not full CommonMark or HTML support.

Inline Markdown links show their labels, with the explicit destination retained for hit testing.
Local file links open internal panes; HTTP(S) links open the default browser only after a click.
Unsupported URL schemes remain literal. A file-shaped label never overrides an explicit web
destination. In formatted file panes, relative file links resolve from the source document's
directory. Plain user prompts remain literal and raw file mode retains the original syntax.
Windows chat destinations using `/C:/...` are accepted as the corresponding absolute drive path;
ordinary rooted paths and unsupported schemes are not reinterpreted.

The message box accepts wheel scrolling independently of transcript scrolling. Alt+PageUp and
Alt+PageDown page the input viewport. Scrolling changes neither the text nor the caret. A redraw
keeps the chosen viewport; editing, caret movement or a changed wrapping width restores caret
following. The terminal caret is hidden when its row is outside the visible input viewport.

Update progress belongs to the update page while open, and to the footer when the page is closed.
The input box can say it is paused but repeats neither progress nor elapsed time. Update-page
footers show controls, not a second copy of progress.

## Mouse pointer shapes

Concordia requests pointer, text and resize shapes through OSC 22. This is a host capability,
separate from mouse events, hit testing and the blinking text caret. Emitting the request is
not evidence that the visible host cursor changed.

The locally installed host reports `wezterm 20240203-110809-5046fc22`. That revision's OSC parser
does not implement a pointer-shape command, and its terminal-pane mouse handler chooses pointer
for terminal hyperlinks, default while an application grabs the mouse, or text otherwise.
Consequently Concordia cannot promise custom resize/hover cursor shapes in that installed host.
No host replacement, global cursor modification or WezTerm configuration change is made here.

Version-pinned upstream evidence:

- [OSC parser](https://github.com/wezterm/wezterm/blob/20240203-110809-5046fc22/termwiz/src/escape/osc.rs)
- [Mouse handling](https://github.com/wezterm/wezterm/blob/20240203-110809-5046fc22/wezterm-gui/src/termwindow/mouseevent.rs)

The shape-selection unit tests establish requested intent only. A supporting terminal and a
physical-host check are required to claim working visible pointer shapes.

## Paste formatting, 2026-11-02a

### Image paste and edit evidence, 2026-11-04e

Build `2026-11-04f` adds a clickable **Review activity** completion row. It opens the turn's
provider-reported thinking and full tool/edit details, with file links and search; Ctrl+E opens
the latest review. Thinking rows also have a clickable gutter. No reasoning is synthesized or
added to provider context. Final tool cards reconcile by identity even when earlier cards streamed,
so late edit details remain visible after completion. The aggregate review is bounded to 256 KiB;
individual cards remain available above it. This does not enable transcript recording or invent
activity missing from a provider's native history.

Standalone pasted PNG/JPEG/WebP paths attach and show a compact four-row composer thumbnail
automatically, with pixels limited to twelve columns. Click the thumbnail or chip to enlarge it.
Normal typing while import runs keeps the attachment. F9 imports a pending file offer, pastes a
clipboard image, or opens the attachment chooser. The footer exposes this key even when a host
intercepts Ctrl+V. Clipboard bitmaps still require an input action delivered to Concordia.

Codex exec's path-only edit events are enriched after settlement with successful `apply_patch`
inputs from the exact account/home/native session and current attempt. Cards then show the native
added/removed lines and file path. A missing, stale, unsuccessful or out-of-bounds patch record
stays explicitly unavailable; current file contents are never presented as the historical diff.

Clipboard and bracketed paste preserve LF, CRLF and CR-only logical line breaks, including blank
lines. Sending, queueing and editing queued prompts retain leading indentation and trailing
whitespace. Prompt tabs keep four-space expansion; raw files preserve tabs. This preserves plain
text formatting, not rich HTML/RTF styling. A native Codex history-write failure retains the
unfinished request and pauses the queue. After inspecting partial actions, `/continue ACCOUNT`
explicitly starts a fresh native session with source pointers. Saved history is retained;
recording errors are not quota signals.
