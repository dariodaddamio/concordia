# Internal terminal workspace

Status: build `2026-11-04k` adds horizontal scrolling and shared-document native file windows.
Build `2026-11-01d` adds bounded chat/recent/workspace file discovery and directory browsing.
Build `2026-11-01c` adds internal editing and pane-owned file recovery to the existing
preview/search/reload/current-chat suggestion surface. Markdown/JSON open formatted by default;
Alt+V or the view bar switches modes. Formatted view is read-only. Source code opens raw,
editable and syntax-colored. Ctrl+S saves; Ctrl+Z/Ctrl+Y undo/redo; Ctrl+F finds; F3/Shift+F3
navigate matches. Raw letters such as `v`, `r`, `/` and `q` are file content.
Visible pointer shapes depend on the native renderer; unsupported shapes do not
implement the requested control. [Chat rendering](chat-rendering.md) records the evidence and
the newer table/link rendering and input-scroll behavior.
The initial preview slice shipped in local build `2026-10-21b`.
`/open` uses internal panes and the file action picker can launch the configured GUI editor. The
search/replacement stage below is implemented with explicit source-bound previews. Keep the stable
installed binary usable while developing this feature. No live providers are required here.

Internal editing uses exact UTF-8 bytes, preserves BOM/CRLF/tabs, bounds delta history, and stages
compare-before-replace saves. Windows replacement retains a backup if a concurrent change is
detected at replacement. Closing, reloading or quitting with edits asks S save / D discard / Esc
cancel. Save errors retain the buffer. Editing is paused if recovery storage is unavailable.
File drafts, baseline, cursor, mode and split layout are stored in the pane's recovery lease,
debounced 250 ms and flushed at normal exit. Recovery checks canonical workspace/path bindings;
changed or missing originals never silently replace recovered edits. Recovery retains the newest
buffer, not the prior undo stack. The existing `/recover` flow claims interrupted pane records.

Concordia should provide a graphical workspace inside the terminal: chat and files share one
screen, with mouse operations and slash commands controlling the same pane state. Harness owns
shared workflow and routing policy; Concordia implements the workspace and execution mechanisms.
The project's design system owns applicable UI principles. Neither provider sessions nor authentication are replaced.

Raw files have a horizontal scrollbar when a line exceeds the viewport. Shift+wheel,
horizontal trackpad/wheel gestures, or dragging the scrollbar moves the viewport without
moving the caret. Line numbers stay fixed. In the native window, **Detach** or dragging a
file title beyond the main window opens a separate file window; **Return**, its window close
button, or Ctrl+Shift+D docks it again. Ctrl+Shift+D detaches a focused docked pane. These
windows share the same document, undo history, save checks and recovery lease; they do not
start a provider or create another file buffer. Restart recovery restores file panes docked.
The existing pane and file limits apply across all windows.

## User-visible contract

| Action | Required behavior |
| --- | --- |
| `/open` | Browse chat references, workspace recent previews and bounded workspace files/folders; Enter opens without starting a provider turn |
| `@path` suggestions | Tab/Enter inserts a literal reference; Ctrl+P or a file-row click opens the preview while retaining the draft |
| `/open FILE[:LINE[:COLUMN]]` | Resolve relative to the conversation cwd and open an internal file pane beside chat. Honor explicit location references and show errors without losing the prompt |
| Another `/open` | Add another split when space and limits permit. Give every pane a visible stable id. Reopening an existing file focuses its pane rather than creating conflicting buffers |
| Click pane | Focus it, with a visible indicator. Keys target the focused surface; file navigation must never submit a chat prompt |
| Click top-right X | Close that file pane; use the same close operation as the command |
| Ctrl+F in a focused file pane | Search; F3/Shift+F3 navigate. Formatted view also supports `/` and `n`/`N`; raw letters edit the file |
| Ctrl+R in a focused file pane | Reload asynchronously, keeping the pane and draft; formatted view also supports `r`. Dirty reload requires a decision |
| Drag divider | Resize adjacent panes within minimum dimensions, retaining content, selection and focus |
| Small terminal or pane limit | Keep chat usable; explain a refused additional split. Never create invisible panes with live hit targets. Tabs may be added later |
| `/open FILE` | Open an internal pane, formatted for Markdown/JSON and raw for source code. Standalone CLI preview remains read-only |
| Edit/save/close | Raw mode edits; explicit save, dirty decisions, external-change checks and bounded pane-owned recovery |
| Search/replace | Search the cwd with visible scope/ignore rules. Preview hash-bound replacement diffs and apply selected changes explicitly |

Initial layout: chat occupies the left column; file panes share the right column. The first slice
has at most three file panes, a 2 MiB/50,000-line UTF-8 limit per file and small-screen refusal.
These are bounded initial defaults, to be checked against measured use before expanding.
Errors for binary, oversized, missing, inaccessible or non-regular files must remain visible.
Do not evaluate terminal control characters from file contents. Do not start an editor process
per internal pane or scan the entire repository during rendering.

The preview, editing/recovery and selected project replacement slices are implemented.
Keep the external-editor launch path explicitly accessible when changing `/open` semantics; never
silently depend on an editor installation for an internal preview.

## Implementation slices and acceptance

1. **Pane state and preview.** Add one bounded pane collection with stable ids, focus, scroll
   offsets, rectangles and a single close operation. Reuse file-reference resolution. Integrate
   drawing and hit testing into the existing ratatui loop. Commands and X share state transitions.
   Verify repeated opens, exact-target close, focus and scrolling, invalid files, small terminals,
   divider resizing, modal precedence and transcript selection using synthetic files/TestBackend.
   A split must not erase or send a typed/queued prompt. Keep file reads bounded and outside the
   render path; slow storage must not stall streaming or cancellation.
2. **Editing and recovery.** Add explicit save/undo and dirty-close decisions, atomic writes where
   supported, external-change conflict detection and bounded recovery owned by workspace and pane
   ids. Restore drafts/layout after interruption without resuming an uncertain provider action.
   Verify failed saves, external changes, concurrent agents, restart and permission errors.
3. **Cwd search and replacement.** Reuse supported search tooling without model calls. Bound
   results and reads, show scope, and validate selected replacements against the version previewed.
   Verify ignored files, Unicode, changed files, partial failures and cancellation.
4. **Self-development journey.** Through the stable application: open code, delegate a disjoint
   change to a fake worker, inspect edits, run checks, review the result, interrupt and recover.
   Build a candidate separately. Activating it is explicit; failure leaves the stable app usable.

For each source slice: bump `BUILD_STAMP`, update the changelog, follow the scoped verification
rules in [AGENTS.md](../AGENTS.md), and record actual results. Do not rewrite historical validation as proof of a
new build. Installation/activation and real-provider tests are separate from source verification.
The first slice's dated source-preview validation is in the [Git-preserved journal](e2e-status.md#earlier-evidence).

## Resource and lifecycle requirements

Measure Concordia and child processes separately and together: idle memory, multiple open files,
sustained streams, input latency, startup and child cleanup. Existing metrics do not measure idle
memory. Pane count and file limits must bound memory; closing panes must release retained content.

File panes are not agent worktrees. A later task/worktree registry needs explicit task, run,
branch and path ownership, active-session checks, and preservation of dirty or unique commits.
Only an explicitly authorized lifecycle policy can retire managed workspaces; age or a model's
completion message alone is insufficient. Existing manually created worktrees need reconciliation
before enrollment. Never automatically clean unrelated user work.

## Accepted file-opening choices

The owner chose the labels **Preview here**, **Split off** (a new terminal), and **Open in editor**.
The shared file-action resolver supports these choices, direct configured GUI editor launch and
a dedicated terminal editor process; [file actions](quality/file-actions.md) records their bounds.
Native **Detach** differs from **Split off**: it moves the existing document view into another
window with shared undo/save state, rather than opening an independent editor from disk.

## Project content search

Use `/search`, or Ctrl+F while the `/open` file browser is visible, to search UTF-8 project file
contents. The search surface owns its query separately from the chat draft. Enter scans or opens a
selected result; Esc closes it. Case controls and completeness/limit reasons are shown on screen.
File-pane Ctrl+F remains in-file search. Existing queues and running provider turns are preserved.

Scans exclude sensitive/provider paths, ignored/build directories and paths escaping the root.
Results retain original byte offsets and hashes. Opening verifies the exact source on a background
worker and displays the verified snapshot; changed results must be searched again. An already-open
file must be closed before a search snapshot can open, preserving unsaved pane buffers. Replacement
is implemented through explicit hash-bound selected previews; regex search remains unsupported.
See [project replacement](quality/project-replacement.md).
