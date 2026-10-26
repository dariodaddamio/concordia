# Everyday E2E status (MVP A)

**Verdict (2026-10-21): Partial, with installation and fake-provider verification established.**
The locally installed build is `2026-10-21d`, including session titles, resumed scrollback,
compact activity and opt-in usage-limit continuation, alongside the earlier pane, stream-order,
cancellation and usage-reader fixes recorded below. Earlier versions of this page
incorrectly said installation and everyday smoke had never run. The final validation recorded in
[interaction-audit.md](interaction-audit.md#production-readiness-follow-up-build-2026-10-16e)
and [production-plan.md](production-plan.md#progress) supersedes that claim.

The earlier 2026-10-21 read-only reconciliation found an installed `current` target reporting
`concordia 0.0.1 (2026-10-16e)`. Its SHA-256 matched `target/release/concordia.exe`:
`5F7D955E6388685FFAECAF049E99E91FF3E9E480BDB088687995E5D5753012D9`.
This confirms the installed artifact now; it does not rerun historical tests or establish live
provider or physical host behavior. No install, smoke or model turn was run during reconciliation.

Detailed audits: `command-audit.md`, `interaction-audit.md`, `file-navigation-audit.md`, `provider-activity-audit.md`, `usage-freshness-audit.md`.

## What landed

- **Wave 1:** thread log and map, token line and tips, `config.toml` and user commands, `/use add` account connect (`connect.rs`), copy/paste/selection, prompt history, scrolling.
- **Wave 2:** prompt queue (`Tab` queues while busy, explicit resume), undo/redo, drafts and recovery records, activity cards with full detail (`Ctrl+E`, `Ctrl+O`), file links and `/open` `/preview`, read-only MCP server and bounded chat reads, provider compatibility checks, process stats, vendored Crossterm for Windows emoji input.

## Verified by tests vs only built

| Area | Verified by automated tests (fake provider, TestBackend, unit) | Only built, not verified live |
| --- | --- | --- |
| TUI shell, slash surface, pickers | recorded 651 tests: 558 unit and 93 integration; 18 installed fake-provider ConPTY journeys | human interaction in the live WezTerm pane |
| Queue, undo, drafts, recovery, activity cards | yes | long real sessions |
| Stream parsing (Codex, Claude, Cursor JSON) | yes, from recorded and synthetic lines | real provider turns (never run by agents; no real CLI turns in tests) |
| `/use add` and slot creation | unit and TUI-flow tests | a real login window |
| Copy, paste, mouse, scroll | TestBackend and clipboard unit tests | hand use in the live pane |
| MCP server, chat read/search | `tests/mcp.rs` and unit tests | a real MCP client |
| `/update` re-exec, `current` pointer | isolated pointer unit tests; installed artifact verified | real tagged-release update |
| Install and launch path | recorded installed everyday and CI smoke passed; current pointer/version/hash rechecked read-only | future candidates need their own activation evidence |

The recorded smoke is provider-free. It does not verify real login, native approvals or physical
mouse behavior. Tests were not rerun during reconciliation. [Metrics](metrics.md) establish startup
and draw/input timings within their stated synthetic scope; idle/streaming process-tree memory
remains unmeasured. Current continuation evidence is recorded below separately from that history.

## Open items (ranked)

| # | Pri | Item | Status |
| --- | --- | --- | --- |
| 1 | P0 | **Internal workspace**: bounded preview panes, `/open` splits, repeated opens, X and `/close`, focus and resizing | Implemented in source; validation recorded below; editing/recovery still open. [Acceptance](workspace-panes.md) |
| 2 | P1 | Persist chat/launch and bind Claude/Cursor session ids after turns | Open |
| 3 | P1 | Account exhaustion: bounded fallback with explicit account/home binding | Local opt-in manual/automatic map handoff implemented; fake-provider verification below. Real-provider parity and release terms remain open. |
| 4 | P2 | Picker back navigation and login submenu; approvals UX (double-tap Ctrl+C exit is done) | Open |

Bounded context handoff and explicit workers already exist. `auto` chooses the first idle worker;
policy-based routing and managed worktree cleanup remain planned. OSS publishing is deferred.
See [PLAN.md](../PLAN.md) for the current priority order.

## Acceptance for "MVP A feels real"

1. Header shows stamp `2026-10-16e` or later; `/version` matches `%LOCALAPPDATA%\concordia\bin\current`.
2. Cold path: `/use add` creates or connects an account without leaving Concordia (synthetic coverage exists; a real login is unverified).
3. Prompt: click to place the caret; drag/double/triple-click select; Tab completes slash; Ctrl+V pastes; Ctrl+C copies a selection; long transcript scrolls.
4. Type a prompt: text and/or command cards stream; usage % shows after a prior Codex session.
5. `/update` rebuilds and re-execs the same pane on a new stamp.
6. `scripts/smoke-e2e-everyday.ps1` exits 0 after every user-facing change (bump `BUILD_STAMP` in `version.rs`).

Recorded installed/fake-provider evidence covers the artifact check and provider-free smoke in
items 1 and 6. Human host interactions, a real login/turn and real tagged-release update remain
unverified. Do not treat one smoke exit code as proof of every item.

The next readiness milestone is a bounded Concordia development task completed inside the stable
application, including interruption recovery. Internal read-only panes are the first source slice;
editing and layout recovery remain staged in [workspace-panes.md](workspace-panes.md).
Real provider turns still require an explicit scope and usage budget under `AGENTS.md`.

## Source preview validation (`2026-10-21a`)

- Eight workspace tests cover rendering, stable-id close, duplicate opens, focus and paste,
  mouse/keyboard scrolling, both divider directions, bounded reads, shrinking and late read results.
- All 19 fake-provider ConPTY journeys passed against the debug candidate, including internal
  split/close without provider dispatch and the retained external-editor route via `/edit`.
  The first full run exposed that external-launch test's stale `/open` expectation; after updating
  it, the full journey rerun passed. Non-journey groups passed in the initial full provider-free run.
- A subsequent review identified file-focused y/n/Esc reaching an armed worker confirmation.
  The guard now requires chat focus; a synthetic regression checks that preview input cannot
  accept/reject work and that explicit approval still works after returning to chat. Workspace and
  worker tests are checked again after that guard; the 19-journey evidence predates this focused fix.
- Candidate evidence logs are `target/workspace-preview-unit.log`,
  `target/workspace-preview-workers.log`, `target/workspace-preview-journeys.log` and
  `target/workspace-preview-clippy.log`. These generated local artifacts are not versioned.

That preview candidate was `target/debug/concordia.exe`; its installation status above is
historical. The continuation below records the later activation. Physical Windows mouse/clipboard
behavior, real provider checks, editing, layout recovery and process-tree memory measurements
remain outstanding.

## Continuation validation (`2026-10-21b`)

- The release binary is locally installed and `current` reports `concordia 0.0.1 (2026-10-21b)`.
  Its SHA-256 matches `target/release/concordia.exe`:
  `B19F05980294704721FDB72E0350ACD286CF545329DD567D8C7B1913ED45C9D8`.
  A pre-activation executable backup is retained outside the repository. Existing running windows
  keep their original executable until reopened.
- All 19 ConPTY journeys passed again, including the workspace and worker-focus guard.
  The final core run passed 577 tests with five ignored; clippy passed for all targets with
  `-D warnings`. The installed everyday smoke passed all 673 enabled project tests, including
  the additional token-line regression, plus its generated local usage-reading test.
- A gated fake-provider test proves a shell-start event is received while the provider is still
  blocked awaiting completion. Further fake tests cover direct and `.cmd`-shim cancellation,
  including cancellation after stdout closes. TUI regressions verify commentary/card order,
  retained partial replies/drafts, paused queues and refreshing an existing Codex usage block.
- The recovered document module now compiles and tests BOM/CRLF/tab preservation, bounded undo,
  byte-exact conflict checks and staged saves. It is groundwork for later internal editing.
- Evidence logs: `target/continuation-unit-final.log`, `target/continuation-tests-final.log`,
  `target/continuation-clippy.log`, `target/continuation-release.log` and
  `target/continuation-smoke.log`. These generated files are local artifacts, not versioned.

The installed everyday smoke completed with `smoke-e2e-everyday PASS`; the real-home read showed
the expired 5h window as unknown and retained the reported weekly value. The smoke wrapper now
uses Cargo exit codes when Windows PowerShell captures native stderr, and its generated test is
removed in a `finally` block. Real provider turns were not run.

## Titles, output and usage continuation (`2026-10-21d`)

- The installed executable and release candidate report `2026-10-21d` and have matching SHA-256:
  `12790DC05100F4A0228C5DDF17217430BEB9BBD35836C313BCCE8865EC966F1C`.
  Running windows retain their loaded binary until reopened; there is no code hot reload.
- The new ConPTY journey exhausts fake Codex, continues on configured fake Claude, checks the
  conversation map and original request, and rejects a foreign native-session resume argument.
- Synthetic state tests cover bounded account attempts, preserved titles/trust/queues, manual
  continuation, draft and cancellation guards, uncertain tools/process exit, and terminal failure
  versus a recovered diagnostic. No real provider turns or quota probes were run.
- Compact rows expose commands instead of PowerShell launchers, keep original expanded details,
  and report output line counts. Regenerated SVG screens were visually inspected through a PNG
  render. Terminal title updates use Crossterm's Windows-compatible title command; physical host
  title behavior remains a human check.
- Local evidence: `target/usage-continuation-tests.log`, `target/usage-continuation-install.log`
  and `target/usage-continuation-smoke.log`. A concurrent validation attempt encountered a Windows
  linker file lock on a running test executable; final validation is run sequentially.
- Final sequential everyday smoke passed: 599 core tests passed (five ignored), all integration
  groups passed, including all 22 ConPTY journeys, and the read-only usage check passed. Clippy
  passed for all targets with `-D warnings`. The quota-handoff and resumed-scrollback journeys
  also passed against the installed executable via `CONCORDIA_JOURNEY_EXE`; evidence is in
  `target/usage-continuation-installed.log` and `target/usage-continuation-clippy.log`.

## Cross-provider running-command and cancellation check

The existing ConPTY cancellation/recovery journey now runs against fake Codex,
Claude and Cursor. It requires the compact command and expanded command tail
to appear while the provider is blocked behind an unopened completion gate.
It then queues a follow-up, types a newer draft, cancels, abruptly closes the
application and restarts. The draft and paused queue must survive without
completing or replaying the gated turn.

This strengthened journey passed against the installed `2026-10-21d` executable
on 2026-10-21 for all three fake backends. Run it with:

```powershell
$env:CONCORDIA_JOURNEY_EXE = Join-Path $env:LOCALAPPDATA 'concordia\bin\concordia.exe'
cargo test -p concordia --test tui_journeys cancellation_pauses_queued_work_and_restart_restores_both_queue_and_draft -- --exact --nocapture --test-threads=1
Remove-Item Env:\CONCORDIA_JOURNEY_EXE
```

Four additional installed journeys passed in this continuation: framed paste
plus Tab; cancellation after slash commands leaving an empty draft; expanded
activity navigation preserving the draft; and native-agent reports before and
after completion. The new checks establish intermediate screen visibility and
recovery with synthetic backends. They do not establish physical clipboard/mouse
behavior or native provider/version approval parity. Application behavior and
the installed executable were unchanged in this verification slice.

## Internal preview during a gated provider turn

The new ConPTY journey
`internal_preview_remains_responsive_during_gated_stream` passed against the
installed `2026-10-21d` executable in both the implementation run and an
independent repeat. It uses fake Codex: a running command must be visible before
completion, `/open` must load the internal preview during the gated turn, and
preview typing plus Enter must not dispatch a prompt or alter the chat draft.
After returning to chat, a new draft must persist while the turn is gated and
survive completion alongside the preview content.

Run the named journey with `CONCORDIA_JOURNEY_EXE` pointing to the installed
binary, using the scoped Cargo command format above. This proves focus and
draft behavior during an unfinished synthetic turn; the fake provider pauses
at a completion gate, so it does not measure responsiveness under sustained
output. No application code, installed executable or real provider was changed.

## Picker cancellation with an armed delegation prompt

`picker_cancel_does_not_answer_armed_approval_during_gated_stream` passed
against installed `2026-10-21d` in three serial implementation runs and an
independent coordinator repeat. Fake Codex remains behind its completion gate
while a synthetic MCP proposal reaches the real delegation-confirmation UI.
The test waits 1800 ms, beyond the 1500 ms arming boundary, opens `/trust`, types
`y` into the permissions picker, and verifies that Escape dismisses the picker
without answering the pending delegation prompt. A new chat draft survives
picker dismissal and provider completion, with no additional provider prompt.

The first test draft did not reliably cross the arming boundary; the stronger
version also exposed an Escape/paste synchronization race in the test harness.
Waiting for the picker to disappear before pasting resolves that race. No
production input-routing change was needed. `/use` remains subject to the
existing busy-turn guard; this case exercises the available `/trust` picker.
It does not establish real-provider approval parity or physical host behavior.

## Corrected ConPTY input/render benchmark (`2026-10-22`)

Corrected run3, independently reviewed, measured the installed `2026-10-21d` executable
with a synthetic fake provider in isolated Windows ConPTY sessions. The
installed executable and release copy match SHA-256
`12790DC05100F4A0228C5DDF17217430BEB9BBD35836C313BCCE8865EC966F1C`.
The fake provider binary hash is
`25F5A2FAF929E3BC480999060C45D88F9B6E998A936945224C9EE96506B35DE6`.

All three workloads have 10 samples. The timer starts immediately before the
raw bracketed-PTY write/flush, and completion is the unique marker observed in
the reconstructed rendered VT screen, polled every 5 ms. p50 is the lower
middle sample for an even N; p95 is nearest rank (the maximum for N=10).

| Workload | p50 / p95 | Working Set min–max | Private Bytes min–max |
| --- | ---: | ---: | ---: |
| idle | 16.963 / 28.255 ms | 9.76–9.79 MiB | 2.60–2.60 MiB |
| gated-active | 25.458 / 32.998 ms | 14.24–15.09 MiB | 3.52–4.41 MiB |
| retained-history | 15.499 / 30.883 ms | 10.32–10.96 MiB | 2.77–3.60 MiB |

Memory sums live process `WorkingSet64` and `PrivateMemorySize64` bytes across
the Concordia root PID and its current Win32 child closure, then converts to
MiB. The gated-active phase requires exact unique provider start, holds the
fake turn behind a gate while sampling, then requires the exact unique reply
and done records. Retained-history completes 10 unique turns before measuring
the next draft; its history fixture contains 10 unique entries.

This is input-to-observed-rendered-VT evidence, not physical host display or
compositor latency and not continuous stress. Gated-active is a blocked-turn
interaction case, not sustained provider-output stress. No real provider,
network, account, credential, model, or installation action was used. The
earlier run2 is invalid for latency claims because its clock started after the
helper's 90 ms send delay; corrected run3 supersedes it. Host: Windows 11 Home
10.0.26200 64-bit, 11th Gen Intel Core i7-1165G7 @ 2.80 GHz, 4 cores / 8
logical processors, 12,674,457,600 bytes installed RAM. Raw samples and
harness evidence remain in the external dated backup.
