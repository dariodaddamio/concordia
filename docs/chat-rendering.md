# Chat rendering and input, 2026-11-01f

Build `2026-11-05a` recovers verified static code-mode patch batches into their original edit rows
and uses one status/bold-heading convention for grouped and individual tool calls. "Reported file
changes" is the separate metadata catalog: a catalog entry identifies a reported file and its
native provenance; it does not by itself establish that a historical line diff was supplied.
Unavailable or ambiguous patches remain explicit rather than being reconstructed from current files.

`/config` opens live appearance settings: Default, Dark, Light, Terminal and Opifex Studio,
custom running-window icon colors. Choices persist separately from `config.toml`.
Themes also use `[ui] theme` and optional background/foreground/accent overrides in `config.toml`;
[configuration](config.md) lists the presets and examples. Native checks/status marks are drawn as
simple strokes, avoiding font-dependent checkmark substitution. Real source Unicode is preserved.
The compact Home portrait and application icon use [Concordia artwork](brand-direction.md).

Build `2026-11-04r` preserves the exact typed request when expanding `@chat` references. Retrieved
maps are labelled source context, appended once during background preparation and shown through
the existing context notices. Native history hydration and document preparation stay off the UI
thread. Persistent Up/Down recall uses verified native references while leaving old history readable.

Build `2026-11-04q` keeps recorded thinking in individual chat accordions. Click its heading or
gutter to expand the full recorded text in place; Ctrl+O toggles all activity. Restored history
uses the same rendering and actions. The Worked for footer is duration/time/usage metadata,
without an activity-review button. Ctrl+E remains a raw tool-output inspector, not a thinking
viewer. Provider-private encrypted reasoning remains unavailable.

Build `2026-11-05g` shows short thinking whole, without a toggle, when the collapsed preview would
hide nothing. The `[concordia map]` line shown after a provider switch is an accordion over the full
map handed to the next provider. Native-window images are clipped out of the text-layer copy so
they no longer flicker on caret blinks or prompt edits.

Expanded inline edit cards show line numbers from reported unified hunk ranges, syntax colors,
and muted red/green backgrounds spanning removed/added rows. Context remains neutral. Unknown
hunk positions stay unnumbered rather than being inferred from the current disk file. Display
decoration does not change the copied diff or its original historical source identity.

Build `2026-11-04p` shows compact tool headings such as
`Edit File example.rs + Ran build - 22 output lines - Expand`. File labels retain their exact
full targets; expanding retains the raw tool details. Static wrapper calls are summarized only
when the entire bounded batch can be parsed without executing it.

Multi-question conversational forms support descriptions and a trailing progress note, display
question progress and keep mouse/keyboard selection across questions. F8 or a click enters the
choices. These replies never grant native tool approval. The enlarged image viewer uses the
shared top-right X close control. During work, the first Enter preserves the draft and explains
that another Enter within five seconds interrupts and sends it after the active turn settles;
Tab queues instead. Editing, another key, a mouse action or a changed turn resets confirmation.

Release updates refresh trusted checksums and reuse matching cached bytes. Cleanup is bounded
to owned artifacts, preserves a rollback and excludes active binaries; unknown files remain.
Installing identical bytes avoids another executable swap. Source updates retain Cargo's own
incremental freshness checks rather than trusting a version string alone.

Build `2026-11-04o` accepts held navigation and deletion keys, including Ctrl/Shift arrows,
without repeating send/cancel/toggle shortcuts. Restored native chats preserve thinking and tool
cards without appending synthetic "Recorded turn" messages. Ctrl+O and tool headers still expand
recorded work, and Ctrl+E opens raw tool-output details.

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
readable provider thinking. Encrypted reasoning remains
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
subdued Esc text. Build 05c simplifies titles to `Concordia — Task · Thinking`, with Home shown
on the landing page, short active phases and no redundant ready/account/model fields. Account and
model remain in the header. Build 05d restores the original static gold Home portrait and removes
the 05c 3D renderer and animation wakeups. Opifex Studio retains its palette.
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

Thinking and tool/edit details remain in the chat after completion and native-history restoration.
Thinking headings and gutters expand recorded text inline; completed tool cards retain their own
expansion, file links and raw-output inspector. No reasoning is synthesized or added to provider
context. Final tool cards reconcile by identity even when earlier cards streamed, so late edit
details remain visible after completion. This does not enable transcript recording or invent
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
