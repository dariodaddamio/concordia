# Cross-provider / cross-account continuity

Package: **continuity**. Dependencies: P0 capability notes, PLAN identity contract.  
Owned: `crates/concordia/src/context.rs`, TUI switch paths, `docs/evidence/continuity/**`, fixture tests.  
No live model turns. No LLM summarization.

## Sources (primary)

| Source | Continuity-relevant fact |
| --- | --- |
| Codex app-server / CLI | Resume by explicit `threadId` / `SESSION_ID`; `--last` anti-pattern; history lives under `CODEX_HOME` |
| Claude headless | `--resume <session-id>`; `--continue` = most recent (refuse); config under `CLAUDE_CONFIG_DIR` |
| Cursor ACP / CLI | `--resume <chatId>`; bare `resume` / `--continue` = latest (refuse) |
| PLAN / capabilities | Cross-account transcript continuity **unproven** → new native session + bounded recorded context only |

## Continuity matrix

| Switch class | Native resume? | Session id passed? | Pack? | Explicitly lost |
| --- | --- | --- | --- | --- |
| Same provider + same home + explicit id | **Yes** | Yes (that id) | **Omitted** | Nothing from native thread |
| Same provider + **other** home + id | No | **None** (must not leak) | Yes if history | Tool state, approvals, native transcript — **live-proven 2026-10-15**: `thread/resume` → `no rollout found for thread id` under alternate `CODEX_HOME` ([resume-probe.json](evidence/continuity/resume-probe.json)) |
| Cross provider + any id | No | **None** | Yes if history | Tool state, approvals, provider-native memory |
| No session id | No | None | Yes if history + switch flagged | Same as new session |
| `/new` | N/A | Cleared | No | Prior bind |

Planner: `plan_continuity` in `crates/concordia/src/context.rs`.

## Pack contract

| Rule | Value |
| --- | --- |
| Content | The thread **map** (`threads::build_map`), never a transcript: header, asked, now, files, ran, errors, last reply tail, `more:` pointer. Deterministic, no model |
| Budget | **1600** chars (`threads::MAP_CHARS`); delegate workers use `min(3000, 1600)`. A character budget, not a tokenizer; never exceeded |
| Drop order | `ran`, `files`, `errors`, then shorten `last reply`, then `asked`/`now` |
| Source | The thread log when recording is on (`CONCORDIA_THREADS=1` or store setting `thread_log`); otherwise the in-memory transcript (user, assistant and tool cards), and the `more:` line says the full text is not stored |
| Pointer | `more: concordia thread read <id> [--tail N \| --grep TEXT \| --range A-B]` (read-only) |
| Token estimate | `ceil(chars / 4)` - not a real tokenizer |
| Omit when | `native_resume == true` |
| Label | The header line states tools/approvals **not** transferred; the TUI shows it with the est tok count |

## Verify

```powershell
cargo test -p concordia -- context
cargo test -p concordia --test control_plane
```

Evidence: `docs/evidence/continuity/latest.json`.
