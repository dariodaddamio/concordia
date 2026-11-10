# Concordia delegated improvement plan

Baseline: installed `2026-10-16a`; existing changes are preserved. All provider tests use
synthetic homes and the fake provider. No real provider turns, credentials, publishing or
remote changes are authorized. Agents use the available `gpt-6-luna` at medium reasoning.

## Round 1

| Task | Implementation plan | Observable acceptance |
| --- | --- | --- |
| 1. Independent recovery | Give each running pane an OS-locked recovery record under a workspace key; restore only an explicitly available record, with a chooser when several exist; migrate legacy files without stealing active records; keep shared history separate | Two panes cannot restore or overwrite each other's active draft/queue; restart can claim an abandoned record; other workspaces stay separate; corrupt data remains inspectable |
| 2. Queue keyboard focus | Add Ctrl+Up to focus queued prompts without clearing the draft; offer clear focus/navigation hints and a safe return path | A nonempty multiline draft and its caret survive keyboard queue selection/editing and return; empty queue is harmless; shortcut survives real Windows input |
| 3. Multiline queued editor | Grow the upper editor to a bounded number of wrapped rows; add vertical navigation, selection, newline and scrolling while keeping the main draft separate; visibly hold automatic dispatch during editing | Long and multiline queued prompts can be reviewed and edited; resize and Unicode stay usable; save/cancel preserve expected text; completion never sends stale edits |
| 4. Individual activity controls | Add per-card expansion, a focused detail view with search and copy, and viewport preservation; retain Ctrl+O for expand-all compatibility | Expanding/copying/searching one card leaves other cards and the draft intact; received output is complete; streaming updates keep the chosen card stable; scroll does not jump unexpectedly |
| 5. Explicit queue resume | Add an explicit queue resume command/shortcut independent of the draft; sending typed text does not silently resume a paused queue; make hints describe focus and pause state | Cancel/failure/restart keep the queue paused; resume starts exactly one queued item through normal submit while preserving the draft; no misleading Enter hint or silent replay |

Ownership: recovery agent owns persistence/recovery modules; focus agent owns its new shortcut
module; activity agent owns activity/transcript modules and TUI wiring. The multiline and resume
agents run as slots free, with disjoint queue-editor/queue-control ownership. The parent owns
integration, terminal journeys, build stamp, changelog, installation and final gates. Each agent
returns changed paths, targeted checks and integration notes. No nested delegation.

## Audit and round 2

After integration, review state ownership, keyboard conflicts, cancellation races, data loss and
rendering. Extend the real ConPTY fake-provider journeys for the new behavior, run meaningful
tests and clippy, then delegate another bounded round of improvements found by that audit.
Round 2 tasks and acceptance criteria will be recorded here before implementation. Integrate and
audit that round again; report remaining limitations and install only a checked build.

## Round 1 audit and round 2 plan

Round 1 is integrated. Terminal journeys exercise all three fake provider adapters, Ctrl+Up
with a nonempty draft, multiline queue editing, explicit resume, live pane separation,
abandoned-record selection and workspace separation. Code review found the following gaps:

| Task | Evidence and implementation plan | Observable acceptance |
| --- | --- | --- |
| Recovery feedback and pending edits | Writes currently discard errors and advance saved revisions; queue edits are omitted from recovery; legacy import overwrites the current record. Add visible save-failure status, bounded retry and a backward-compatible optional pending-edit snapshot; preserve the previous record during explicit recovery/import | Failed writes remain dirty and recover after the filesystem is repaired; restart restores an upper edit separately from its committed queue item and main draft, with dispatch paused; legacy import preserves the current pane record |
| Queue editor loss and clipping | Clicking another row or outside the panel calls unfocus and drops the edit; edit cursor/rect can exceed a short viewport; carving uses a different prefix width. Hold the edit until explicit save/cancel, share layout measurements and clip hit targets/caret | Outside clicks cannot discard edits; tiny/short/resized terminals keep the caret and hit targets in bounds; multiline navigation and dispatch hold still pass |
| Activity modal routing and navigation | Buffered multiline/large paste calls paste_text_chunk directly and bypasses the modal; detail scroll approximates word wrapping; only the newest card is keyboard-accessible. Centralize modal paste routing, render/search from one wrapping plan and add discoverable previous/next-card shortcuts | CRLF and overflow paste never changes the underlying draft or queue; selected matches appear in the viewport; older cards can be opened, searched and copied from the keyboard without losing the draft |

Round 2 ownership: recovery agent owns persist.rs/recovery.rs and recovery tests, including the
pending-edit data schema; queue editor agent owns queue.rs and queue editor tests; activity agent
owns activity.rs/activity_tests.rs/transcript.rs and its mod.rs input/rendering sections. Parent
owns recovery UI integration, terminal journeys, docs and final gates. All use gpt-6-luna medium;
no nested agents or live provider turns. Targeted tests are followed by one central full audit.

## Status

- Round 1: implemented and integrated.
- Round 1 audit: all 13 terminal journeys passed; findings above delegated as round 2.
- Round 2: implemented and integrated. Final review also removed a processing-time key debounce
  that dropped distinct Escape presses after long paste; a real-terminal failure and a focused
  regression test verified the fix. Fresh-record legacy import and modal caret visibility were
  corrected during integration. Final checks passed: 506 enabled unit tests and 85 integration
  tests (including 15 real-terminal journeys), installed-binary smoke, clippy with warnings denied,
  formatting/diff checks and dependency/OSS gates. Screenshots and metrics were regenerated;
  installed build is `2026-10-16b`. Changes remain local and uncommitted.
