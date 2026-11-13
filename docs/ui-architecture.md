# UI architecture

Concordia's native Windows window renders the ratatui application.
Native tabs retain independent conversation state. Provider CLIs supply structured events, not a
hidden native TUI to scrape.

## Ownership and input

- `tui/run.rs` owns tab lifecycle, provider channels, polling and redraw scheduling.
- `tui/window_session.rs` owns OS-locked workspace window layouts, exact pane references and
  bounded saved conversation projections; native approval/process state is never serialized.
- `tui/app/` owns the conversation, draft, queue, native binding and turn finalization.
- `tui/input_dispatch.rs` routes input to the active control before the composer or Home.
- `tui/draw/` renders cells; `tui/native_window.rs` renders those cells/images on Windows.
- `tui/transcript.rs` owns wrapped transcript rows, scrolling and selection.
- `tui/workspace.rs` owns file panes; preview/edit/save use the file modules.

Home reuses an unbound tab and returns to the originating chat; each tab keeps its own draft,
attachments, queue and running work. Ctrl+T explicitly creates a new tab.
Restart restores the last closed workspace window's open tabs, selected conversation and drafts;
held window and pane locks prevent concurrent windows from claiming the same saved ownership.
Choosing a recorded chat resumes its explicit session and reads history without starting a model.
An open picker owns navigation, Enter and Escape even when Home remains underneath it.

## Rendering and status

The prompt editor, transcript view and workspace panes have separate layout/selection state.
The composer keeps up to 200 undo steps with a 32 MiB budget per direction; an oversized newest
step remains protected. Clipboard helpers share the process-lifetime deadline/output owner.
Background pollers explicitly report accepted visible changes to the runner. Folded transcripts
reuse exact source keys while their estimated structs and payload fit an 8 MiB budget; larger
projections use a full content fingerprint. Both paths check content, presentation, link roots
and diagram revisions. Initial wrapping and oversized fingerprint comparisons remain linear;
static review does not establish a latency guarantee.
Provider events update text, reasoning and tool cards. The named activity phase explains current
output; it does not decide whether a process is running or a tool is safe to replay.
[State ownership](tui-states.md) describes those separate responsibilities.

Themes apply one semantic palette to the cell buffer; source image pixels remain image data.
Native image padding and client margins use the selected canvas. Header account, model, usage and
Attention controls refer to the active tab. Unknown usage/credit values stay unknown.

Behavior details: [chat/input rendering](chat-rendering.md), [workspace panes](workspace-panes.md),
[images and diagrams](images-and-diagrams.md). Keybindings and commands: [UI glossary](ui-glossary.md).
