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
| Budget | **6000** chars (a character budget, not a tokenizer) |
| Selection | User + Assistant only; walk from end; stop before exceeding budget |
| Skip | System / meta lines |
| Token estimate | `ceil(chars / 4)` — not a real tokenizer |
| Omit when | `native_resume == true` |
| Label | States tools/approvals **not** transferred; includes char + est tok counts |

## Verify

```powershell
cargo test -p concordia -- context
cargo test -p concordia --test control_plane
```

Evidence: `docs/evidence/continuity/latest.json`.
