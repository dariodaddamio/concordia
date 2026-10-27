# Internal terminal workspace

Status: first read-only preview slice implemented in source build `2026-10-21a` and included in
locally installed build `2026-10-21b`.
`/open` and `/preview` use internal panes; `/edit` preserves the external-editor route. The
editing/recovery and search/replacement stages below remain requirements. Keep the stable
installed binary usable while developing this feature. No live providers are required here.

Continuation `2026-10-21b` compiles and tests the recovered `file_document` foundation separately:
exact UTF-8 bytes, BOM/CRLF/tab preservation, delta undo/redo, bounded edit history and staged
compare-before-replace saves. Windows replacement retains a backup when a concurrent change is
detected at replacement. This is groundwork; it does not expose internal editing controls or
claim completed draft/layout recovery. Read-only panes continue to be the user-facing slice.

Concordia should provide a graphical workspace inside the terminal: chat and files share one
screen, with mouse operations and slash commands controlling the same pane state. Harness owns
shared workflow and routing policy; Concordia implements the workspace and execution mechanisms.
The project's design system owns applicable UI principles. Neither provider sessions nor authentication are replaced.

## User-visible contract

| Action | Required behavior |
| --- | --- |
| `/open FILE[:LINE[:COLUMN]]` | Resolve relative to the conversation cwd and open an internal file pane beside chat. Honor explicit location references and show errors without losing the prompt |
| Another `/open` | Add another split when space and limits permit. Give every pane a visible stable id. Reopening an existing file focuses its pane rather than creating conflicting buffers |
| Click pane | Focus it, with a visible indicator. Keys target the focused surface; file navigation must never submit a chat prompt |
| Click top-right X | Close that file pane; use the same close operation as the command |
| `/close <pane-id>` | Close the identified file pane. Bare `/close` closes the focused file pane; if chat is focused, show usage. Invalid ids change nothing |
| Drag divider | Resize adjacent panes within minimum dimensions, retaining content, selection and focus |
| Small terminal or pane limit | Keep chat usable; explain a refused additional split. Never create invisible panes with live hit targets. Tabs may be added later |
| `/preview FILE` | Open an internal read-only pane; clearly label read-only state. Standalone CLI preview can remain available |
| Edit/save/close | Later slice: show dirty state, save explicitly, protect unsaved edits on close and detect external changes before overwriting |
| Search/replace | Later slice: search the cwd with visible scope/ignore rules. Preview replacement diffs and apply selected changes explicitly |

Initial layout: chat occupies the left column; file panes share the right column. The first slice
has at most three file panes, a 2 MiB/50,000-line UTF-8 limit per file and small-screen refusal.
These are bounded initial defaults, to be checked against measured use before expanding.
Errors for binary, oversized, missing, inaccessible or non-regular files must remain visible.
Do not evaluate terminal control characters from file contents. Do not start an editor process
per internal pane or scan the entire repository during rendering.

The first implementation provides preview only, labels it and must not claim editing,
restored layout or project replacement support until those behaviors are implemented and tested.
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

For each source slice: bump `BUILD_STAMP`, update the changelog, run appropriate provider-free
tests and clippy, and record actual results. Do not rewrite historical validation as proof of a
new build. Installation/activation and real-provider tests are separate from source verification.
For this first slice, see [source preview validation](e2e-status.md#source-preview-validation-2026-10-21a).

## Resource and lifecycle requirements

Measure Concordia and child processes separately and together: idle memory, multiple open files,
sustained streams, input latency, startup and child cleanup. Existing metrics do not measure idle
memory. Pane count and file limits must bound memory; closing panes must release retained content.

File panes are not agent worktrees. A later task/worktree registry needs explicit task, run,
branch and path ownership, active-session checks, and preservation of dirty or unique commits.
Only an explicitly authorized lifecycle policy can retire managed workspaces; age or a model's
completion message alone is insufficient. Existing manually created worktrees need reconciliation
before enrollment. Never automatically clean unrelated user work.
