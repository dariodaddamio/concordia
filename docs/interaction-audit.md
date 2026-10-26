# Fake-provider interaction audit

Audited and installed build: `2026-10-16b` (previous baseline `2026-10-16a`). The real Concordia executable was driven through Windows ConPTY;
all three provider paths pointed to `concordia-fake-provider`. Accounts, homes, state and
workspaces were synthetic and isolated. No real model turn, login or account change ran.

The harness inspects the terminal's final cells, saved drafts/queue and a journal of prompts
received by the fake provider. It answers ConPTY's initial cursor-position query, like a
terminal emulator. Provider completion is controlled by a gate file so edits and cancellation
can be tested during an active turn.

## Journeys

| Interaction | Checked behavior |
| --- | --- |
| Codex, Claude, Cursor | Multiline paste with blank lines, quotes, accented text, a wide character and emoji stays in the draft; a separate Enter sends exactly one complete prompt; Ctrl+O exposes full command and tool output |
| Queue | Tab enqueues; Up selects; Enter edits in the upper panel; Alt+Up reorders; automatic sends retain their order and preserve a separate draft |
| Completion during queue editing | No stale prompt is sent; saving the edit permits automatic dispatch |
| Cancellation and abrupt restart | Active turn stops, queued work stays paused, a newer draft survives, and neither prompt is replayed automatically |
| Provider failure | Error is visible; queued work pauses; a newer draft remains intact |
| Long paste and resize | A paste spanning multiple input chunks is one undo step; redo restores it; resizing to 60x18 preserves the draft |
| Unbracketed CRLF | Blank lines and trailing newline survive normalization; nothing sends until a separate Enter |
| Two concurrent panes | Active panes keep independent draft records and cannot claim or overwrite each other's recovery |
| Queue focus with nonempty draft | Ctrl+Up opens the upper editor; multiline save and Ctrl+Down return preserve the main draft and automatic send order |
| Explicit resume | Restart retains the draft and paused queue; Ctrl+R sends one queued item through the normal turn path |
| Several abandoned panes | Startup presents a chooser instead of guessing; another workspace remains separate |
| Activity detail | Ctrl+E opens full tool output; search, navigation, undo and queue shortcuts do not edit the draft beneath |
| Slash command during a turn | `/stats` does not replace the original prompt used for cancellation recovery |

Regression tests also check that reused tool IDs cannot overwrite earlier turns, and that
`/new`, provider switches, session resumes and model switches cannot move a running turn's
output into another conversation or identity.

## Bugs found and fixed

- Windows delivered pasted LF as **Ctrl+Enter**, bypassing the original paste buffer and
  triggering submit. It is now treated as a newline in both editors; CRLF normalizes once.
- A turn completing during an inline queue edit sent the old item and discarded the edit.
  Dispatch now waits while the editor owns the queue, then continues after save/cancel.
- Overflow chunks of one paste created multiple undo steps. They now share one paste undo group.
- Emoji vanished before reaching Concordia's key handler. Crossterm's general Windows
  surrogate branch paired key-release halves with key-down halves. The vendored 0.28.1 has
  a five-line behavioral patch; the special Alt-code path stays unchanged. See the
  [upstream report](https://github.com/crossterm-rs/crossterm/issues/1072) and
  [patch notes](../vendor/crossterm/CONCORDIA-PATCH.md).
- Cards were matched by tool ID across the entire transcript. IDs reused on subsequent turns
  overwrote old cards. Updates are now bounded by the latest user message.
- A slash command issued during a turn replaced its saved recovery prompt. It now preserves
  that prompt. Conversation/provider/model changes wait until the turn finishes or is cancelled.
- Cursor completed-tool results retained arguments but omitted result output. Results are now
  retained for expanded viewing.
- The second-round overflow-paste journey exposed a processing-time key debounce swallowing
  successive Escape presses. Input now trusts backend event kinds, preserving distinct presses
  even when processed together. A regression closes activity detail then the upper editor.
- Fresh legacy-import records had no JSON file yet; import now accepts that state while rejecting
  attempts to overwrite a nonempty record. App-level import switches to a new record, retaining
  the prior pane's saved work. Save failures block recovery switches and remain visible.
- The existing re-exec unit test persisted `where.exe` into the real install pointer and User
  environment. Its install directory and process environment are now isolated and restored.
  The installed pointer was repaired before continuing validation.

## Delegated improvement rounds

The five recommendations from the baseline audit were planned and delegated separately:
independent recovery, queue focus, multiline upper editing, individual activity controls and
explicit resume. The available model was `gpt-6-luna`, medium reasoning; the requested name
“luna 6.1” was not exposed. Acceptance criteria and ownership are in
[the improvement plan](improvement-plan.md).

All 13 first-round terminal journeys passed. Review then identified three additional tasks:

- Recovery errors were silent; pending upper edits were absent from saved records; import
  could overwrite the current pane. Round two adds failure feedback, bounded retries and
  separate pending-edit recovery while preserving current records.
- Outside clicks discarded upper edits; short/narrow panels could expose invalid caret/hit
  coordinates. Round two holds edits until explicit save/cancel and shares clipped layout.
- Large/multiline input bursts bypassed the activity modal; search scrolling approximated a
  different word wrapper; older cards needed keyboard access. Round two centralizes routing,
  shares search/render wrapping and adds previous/next-card controls.

Two more real-terminal journeys cover pending-edit crash recovery and modal paste overflow
with both a main draft and upper edit present. Unit checks cover save failures and repair,
mouse targeting, tiny panel bounds, Unicode search, copy and activity navigation.

## Further improvements

1. Add a sustained-stream typing-latency benchmark with a large transcript and long draft.
   The measured cached frame time below is not an end-to-end input-latency measurement.
2. Add explicit recovery-record management (rename, inspect and delete abandoned entries).
   Recovery currently preserves abandoned/corrupt records; it does not silently prune them.
3. Continue the planned attachment work: drag/drop, clipboard screenshots and artifact chips,
   using the now-tested paste and upper-editor routing.

Markdown viewport anchors use offsets after Markdown rendering and can shift around formatting
transformations on resize. Plain activity cards map directly to their source text. Very tall
activity detail uses a bounded 16-bit scroll offset; streamed provider text remains subject to
the runner's existing output bounds. Live terminal-host clipboard behavior and provider
approvals still need a human check.

The store's concurrent-writer stress test repeatedly hit its five-second lock timeout during
full-suite execution, while passing alone. Same-process writers now serialize before entering
the timed cross-process lock, preventing local writer starvation. Cross-process locking retains
its existing bounded wait and stale-lock behavior; this is separate from pane-scoped recovery.

## Validation

Build `2026-10-16b` passed 591 enabled tests: 506 unit and 85 integration tests, including 15
real-terminal fake-provider journeys. Four pre-existing unit tests remain ignored by the normal
suite. Installed-binary smoke also passed its optional read-only usage-file check; no live model
turn or quota probe ran. Clippy with warnings denied, formatting/diff checks, OSS gates and
dependency checks passed. Screenshots were regenerated. The install pointer and User executable
setting both point to Concordia, with the expected stamp, after the tests.

[Performance measurements](metrics.md): 30 `--version` runs had a median process startup of
38.4 ms; 500 cached busy-transcript frames averaged 370 us (480 us p95). These synthetic
measurements do not establish end-to-end latency under sustained provider output. Changes
remain local and uncommitted.

## Production-readiness follow-up: build 2026-10-16e

Luna medium workers audited command arguments, provider activity, file navigation and usage
freshness with disjoint ownership. Parent integrated and repeated the audit. New coverage includes
explicit-account native chat reads (including identical IDs across accounts and an older native
UUID outside the recent-list window), file click versus drag/drag-return, exact terminal launcher
argv, Markdown preview start/scroll/exit, selected-account warning clearing and cache identity.

The repeat audit found and fixed raw tool results replacing merged commands, duplicate result
replay, incomplete calls being marked successful, home-hash equality accepting a hypothetical
collision, global recent-list limits hiding explicit reads, and unbounded metadata discovery/output.
Incomplete tools now retain details with an explicit interrupted/provider-failed/incomplete reason;
the stream does not prove the actual command's final outcome.

The initial full terminal run exposed genuine input failures: drawing/host batching merged queue
actions with pasted text, and source gaps broke CRLF normalization and whole-paste undo. Timing
alone could not resolve intent. The Windows VT source now preserves explicit paste framing with a
wide console reader and the existing ANSI parser; quiet markers remain for unframed/fallback input.
Console byte reads treated Ctrl+Z as EOF and replaced non-BMP characters, so the final source uses
`ReadConsoleW` plus stateful surrogate handling. Startup waits for configured input modes; shutdown
cancels and joins the reader and restores settings. Input errors also cancel provider work and
persist the draft. A closed startup-handoff receiver cancels the provider instead of orphaning it.

Journey actions now wait for observable draft/queue/editor acknowledgements; a separate regression
sends framed paste, Tab, and another paste in one terminal write, proving framing without quiet
gaps. Application-queue emptiness alone never establishes paste completion. The first overlapping
smoke invocation hit Windows' executable linker lock; final build/test/install checks run sequentially.

CLI scans cap directory entries/files/accounts and summary I/O; exact native UUID reads narrow by
filename before parsing. JSON retains exact account/session identities and rejects oversized output.
See [configuration](config.md), [command audit](command-audit.md), [provider activity audit](provider-activity-audit.md),
[file navigation audit](file-navigation-audit.md) and [usage audit](usage-freshness-audit.md).

Final validation passed on build `2026-10-16e`: **651 enabled project tests** (558 unit and 93
integration), including **18 ConPTY journeys against the installed release executable** using
`CONCORDIA_JOURNEY_EXE`. Five opt-in unit checks are excluded from the ordinary suite; the
release draw benchmark, typing benchmark and screenshot generator were also run explicitly.
Installed everyday smoke and CI smoke passed, including the optional read-only usage-file check.
No live provider model turn or token-consuming quota probe ran.

Clippy with warnings denied, formatting/diff checks, OSS gates and `cargo deny` passed. The
installed binary matches the release SHA-256; the install pointer and User executable setting
both select it. Screenshots were regenerated. All changes remain local and uncommitted.

[Metrics](metrics.md): 4118 KiB (4.02 MiB) release binary; 34 ms median and 48.8 ms p95 process
startup for `--version`; cached draw p95 343 us. The explicit streaming/input benchmark used 250
samples, 500 transcript messages and a 29,850-character draft: input plus draw p95 **6.715 ms**,
maximum **15.946 ms**, with all draft characters retained. This uses TestBackend and does not
measure physical terminal/network latency or idle memory. Native host/editor interaction and
live provider approval/version parity remain release gates in production-plan.md.

## Repeat checks

```powershell
cargo test -p concordia --test tui_journeys -- --nocapture --test-threads=1
cargo test -p concordia --tests
cargo clippy -p concordia --all-targets -- -D warnings
```

To check the shipping executable while keeping fake accounts/providers, set
`CONCORDIA_JOURNEY_EXE` to the absolute release/installed binary path before the journey command,
then remove that process environment override afterward. The normal suite otherwise uses the
debug executable supplied by Cargo.

These checks establish behavior against scripted streams and a real Windows pseudo-console.
They do not verify live provider approvals, actual provider updates or physical mouse/clipboard
interaction in WezTerm. The existing key-handler tests cover clipboard routing and selection.
