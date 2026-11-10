# Chat repair scope

## This slice

### Chat reliability - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `src/feed.rs` | 1, 4, 6, 10 | Remove recursive locking; the existing delta buffer remains the only owner. |
| `src/native_memory.rs`, `src/tool_literals.rs` | 4, 6, 10 | Delete the unused copy of the literal parser; retain the parser used by native patch extraction and tool cards. |
| `src/tui/`, `src/config.rs`, `src/provider/stream_cards.rs` | 1–10 | Repair existing recovery, tabs, history, palette and dispatch paths. No new subsystem, provider protocol, inference or sync store. Test observable reported failures. |
| `src/delegate.rs`, `src/delegate/tests.rs`, `tests/live_providers.rs` | 4, 6 | Distinguish the result-collection bound from configured worker capacity; repair stale tests. One tiny live streaming check uses the existing opt-in runner. |
| `src/ask_bridge.rs` | 2 | Use the existing Option return to satisfy the required clippy check without changing behavior. |
| `src/provider/edit_display.rs` | 5 | Move its test module after production functions for the required clippy check. No behavior changes. |
| `tests/mcp.rs` | 6 | Bring the tool-list fixture into line with the already-implemented feed tool on main. |
| `docs/chat-rendering.md`, `CHANGELOG.md`, `src/version.rs` | 1, 5, 6, 10 | Document repaired behavior and verification. Product direction discussion is deferred at the owner's request until the app works. |

Paths above are relative to `crates/concordia` for source and the repository root for docs.
Assumptions: preserve all native chats, credentials and unfinished work; Home owns a fresh
composer; selecting an account prepares context locally and sends nothing until submission.
Acceptance: streamed replies finish without blocking input; recovery dismissal survives restart
and can be reversed with `/recover`; history is visible after resume; attention items open their
actual controls; Home does not inherit another chat's draft; context appears at send time only.

## Verification

Follow-up 07g repairs picker ownership with Home visible, including Escape dismissal,
restart and explicit recovery. Startup offers retain the filtered list. PLAN.md now summarizes
capabilities, limits and direction; the source and fixture cleanup from 07f remains preserved.
The fetched origin/main still matches `d404778`.

- All 25 Home-related regression tests passed, including native portrait/canvas black, independent
  drafts, model selection without dispatch and persistent dismissal. Resume now tests Enter
  through the real key handler. Clippy passed with all targets and `-D warnings`.
- The full standalone test run passed; the final unit suite includes 1,116 passing tests and
  six intentionally ignored tests.
- Isolated native-window probes passed on debug, release and installed 07g executables: sampled
  Home/client-margin pixels were #000000, the synthetic reply completed and WM_CLOSE worked.
- The installed SHA-256 matches the release executable. No new live model turns were run for 07g.
- The sequential `scripts/smoke-e2e-everyday.ps1` run passed, including the final full test suite,
  installed build/help checks and read-only native usage. The initial overlapping smoke attempt
  hit a Windows test-executable lock; running it after the standalone suite resolved that failure.

Prior 07f evidence:

- Started from `d404778`, matching the fetched `origin/main`; existing local work was preserved.
- Clippy with all targets and `-D warnings` passed.
- The full `cargo test -p concordia --tests` suite and `scripts/smoke-e2e-everyday.ps1`
  passed. The unit suite reported 1,112 passed and six intentionally ignored; integration
  checks included source updates, replacing a running executable, MCP and read-only native usage.
- One opt-in live Codex turn on `gpt-5.6-luna` returned OK through the native provider transport
  with feed capture enabled. No Claude or Cursor live turn was run.
- An isolated native-window probe used a synthetic Claude stream, completed a reply and handled
  `WM_CLOSE` successfully in both the release executable and the installed `2026-11-07f` build.
- The installed executable's SHA-256 matches the final release build. Formatting and diff checks passed.
- Product direction and the broader documentation/architecture review remain deferred until
  after the repairs, per the owner's clarification. No release or publication was performed.
