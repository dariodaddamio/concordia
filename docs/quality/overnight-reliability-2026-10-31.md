# Overnight reliability pass, 2026-10-31

Scope: Concordia's native Windows feature surface and the installed executable. All
provider turns use fake executables, synthetic homes and temporary workspaces. No
real inference, account migration, remote release, credential access or deployment is
part of this pass. Existing uncommitted work is preserved.

Baseline executable: build `2026-10-31i`, SHA-256
`10F1CAE82B75EB887EB45377D26D660C065CD07C536519B0F4BCDB5D601A2635`.
All 26 existing Windows terminal journeys passed against that frozen executable.
Evidence and the pre-pass patch are under `target/overnight-2026-10-31/` (local,
unversioned). The candidate is build `2026-10-31j`.

## Reproduced failures and fixes

| Failure | Fix and regression evidence |
| --- | --- |
| Goal selection/confirmation and new entry disappear below a long list | Goal rows scroll into view, using the same visual rows for rendering and mouse hit-testing |
| Narrowing a window hides the focused goals pane/input | Tab selects which pane is visible when there is room for only one |
| Two stale goal snapshots erase additions or reopen completed goals | Saves merge immutable goal ids and monotonic completion under a bounded cross-process lock |
| Adding a goal overwrites an unreadable goals file | Bounded reads and save validation preserve the original file and report failure |
| Concurrent token writers interleave records; only 557 of 1,600 test records survived | Complete records are serialized under the shared state lock; thread/process stress checks count every record |
| Extreme provider/log token counts overflow totals and display formatting | Saturating counters and overflow-safe rounding; live-turn finalization is exercised too |
| Identically named folders merge unrelated projects' chats | Group identity uses the repository path; duplicate display names include their path |
| Goal-entry arrows/undo change the underlying prompt | Goal entry owns keyboard input until save/cancel |

The added goal-entry ownership check initially swallowed clipboard/quit shortcuts.
A further regression reproduced that before activation; Ctrl+V and explicit Ctrl+C
handling are preserved, and AltGr characters stay in goal entry. An earlier passing
candidate journey log is retained as `pre-clipboard-journeys.log`. Final repeatability
checks use the corrected executable, rather than reusing that earlier result.

New goal ids use UUIDs so fast or simultaneous additions cannot collide on a clock
timestamp. The file-lock helper returns an error after two seconds rather than waiting
indefinitely. A held-lock test checks the timeout and successful writing after release.
The resumed full run exposed contention in the 200-save stress test: a tight loop
can hold other writers off for that entire budget. The stress test now retries only
`WouldBlock`, with a 15-second deadline per goal, and still requires all 200 records.
Other I/O errors fail immediately. Production locking and its two-second bound are
unchanged. The interrupted failure is preserved in `interrupted-all-tests.log` and
`interrupted-summary.json`.

## Feature coverage

| Feature family | Checks |
| --- | --- |
| Prompt editor and terminal input | 40,000 deterministic edits against an independent text model; undo/redo at every changed step; five wrap widths; bracketed/unbracketed paste, Unicode, word deletion, focus and resize journeys |
| Drafts and queues | 100 repeated two-buffer recoveries; crash recovery, distinct panes/workspaces, ordering, pause/resume, editing/reordering, cancellation and preservation during live fake turns |
| Chat rendering/activity | Streaming commentary/cards, reasoning, diffs, selection/copy routing, modal search/navigation, reused tool ids, long lines, escape filtering and Bottom-button mouse input |
| Landing page/goals | Long lists, wrapped click targets, narrow windows, completion, additions, independent writers, corrupt files, project grouping and a real-terminal save/complete journey |
| Accounts, sessions, models and permissions | Synthetic account discovery/connect/remove, duplicate logins, explicit resume ids, foreign-session rejection, native history, model/mode pickers, busy guards and approval isolation |
| Providers and workers | Fake Codex/Claude/Cursor launch, stdin/shims, streamed failures, process-tree cancellation, delegation, native subagents, manual/opt-in continuation and uncertain completion |
| Usage and token log | Parser fixtures, cache identity/freshness/reset boundaries, displays, concurrent writes from 16 threads and eight processes, malformed log lines and integer extremes |
| Files/workspace | Read-only previews, exact path/line/column, bounded reads, split/close/focus/dividers, resizing and opening while output streams |
| CLI/custom commands | Alias and malformed-argument matrices; 240 extra-argument/whitespace combinations; bounded literal substitution and argument-array launch |
| MCP/Harness | All tools and protocol errors, request/output bounds, 1 MiB oversized line followed by invalid UTF-8/deep JSON and 1,000 valid requests, 42 malformed tool calls, delegation proposals off by default, adapter timeout/descendant cleanup and trust verification |
| Install/update/security | Isolated install/source/release/download/checksum/restart tests, active-job guards, release smoke, format/clippy, advisories, license/source policy and OSS gates |
| Idle/resource behavior | A 30-second real-terminal focus/caret soak measures the child process tree, verifies continued responsiveness and recovery, and forbids provider dispatch |

Tiny-window checks cover 140 width/height combinations from 0x0 through 180x40,
then restore the original draft and landing state. Tests use the application's real
key/mouse handlers and renderer, plus Windows ConPTY for process-level journeys.

## Reproduction and evidence

Run `powershell -NoProfile -File scripts/test-reliability.ps1 -JourneyPasses 3`.
The runner has bounded stage timeouts, records exit codes and durations in
`target/overnight-2026-10-31/summary.json`, freezes and hashes the release candidate,
and repeats the terminal and concurrent-state checks against that exact executable.
Functional failures stop the wall and preserve logs. Release-content scan failures are
recorded while independent local tests continue; the overall exit status remains failed.
No test step runs a real provider agent.

Resumed validation rebuilt and froze candidate SHA-256
`BCB952F7C9FDB1C3CE12386CBBB83F1B9AFDBFD69C346AE97681FE1F4D849124`.
This differs from the pre-clipboard executable; old passing results are not reused.
The full suite passed 748 library tests and 92 integration tests. Formatting,
clippy with warnings denied, refreshed dependency audit, dependency policy,
release build and CLI smoke passed. Five library helpers, the subprocess helper,
and 28 opt-in terminal journeys are excluded from the default suite; the relevant
helpers and journeys are run explicitly by the wall.

The release performance checks passed: input p95 4.887 ms / max 7.350 ms with
500 transcript messages and a 29,850-character draft; draw p95 0.312 ms across
500 frames. These are measurements on this machine, not universal latency limits.

All 28 ConPTY journeys passed in each of three consecutive runs against the
frozen candidate (84 journey executions; 180.78, 179.04 and 179.79 seconds).
The seven enabled transport/state stress tests passed after every run. Screenshot
generation and synthetic-screen checks also passed. The wall recorded 17 passing
checks and one failing check: the pre-existing OSS release-content scan. Its overall
exit status remains failure; this is not public-release clearance.

The frozen candidate was installed locally as build `2026-10-31j`, with its installed
SHA-256 verified equal to the candidate. The previous installed binary was retained
as `concordia-installed-before-activation.exe` in the evidence directory. Installation
used the existing explicit install directory, avoiding PATH or legacy-state changes;
already running tabs retain their previous executable. No remote release was made.
The installed everyday smoke (`scripts/smoke-e2e-everyday.ps1`) passed, including
another full enabled test suite, installed version/help checks and the read-only
usage-file check. Its log is `installed-everyday-smoke.log`. No real model turn
was used in this work.

The first broad test run passed 747 library tests plus 92 integration tests (including
seven reliability/state checks). Dependency audit and policy checks passed. The release
content scan flags pre-existing author/project references in `harness/adapter.rs`,
`provider/enforcement.rs`, Cargo package metadata and install scripts. These files were
already present at baseline. The scan is retained as failing; no public release is made.

## Limits

This is broad automated coverage, not proof of every input/state combination or every
possible vulnerability. Real provider approvals, provider-version parity, actual login
flows, physical mouse/clipboard behavior in each terminal host, remote release feeds
and unsupported operating systems remain separate checks. No real provider usage budget
was supplied, so live turns are deliberately excluded. The idle soak is a bounded local
measurement, not an assertion that the app ran unattended for an entire night.
