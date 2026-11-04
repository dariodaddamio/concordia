# Context capacity provenance

Build `2026-11-03k` extends the existing identity-bound Codex native rollout reader. Each fresh
observation reports provider/account/canonical home/native session, requested and reported model,
native turn id, transport and evidence version. Source metadata includes file identity, snapshot
length, bounded-tail SHA-256 and the exact usage record byte range/hash. It stores no source text
or additional transcript copy and runs no provider process.

Only `last_token_usage.total_tokens` plus a positive `model_context_window` are accepted after
a matching model and exact native `turn_context.turn_id`. Cumulative totals never substitute.
Every new turn context invalidates prior usage, including the same model. Task start, compaction
without refreshed usage, malformed/oversized records, missing turn/model identity and model
changes make capacity unknown with an explicit reason. Legacy records without turn ids remain
readable as source history but cannot trigger proactive rollover. Source replacement, mutation
or append during measurement rejects that observation. Percent comparisons use widened integer
arithmetic. Observations are re-read before a future turn rather than cached across launches.

Before-turn notices expose the observation's identity and revision. `/context` explains the
selected account's evidence requirement; policy changes retain their account/home binding.
Native compaction defaults and the separately implemented one-attempt reactive capacity-error
recovery are preserved. No in-flight turn is interrupted or replayed to implement this policy.

Build `2026-11-03x` adds Claude's version-matched Agent SDK `get_context_usage` control
request on the opt-in native stdio adapter (CLI 2.1.317). Capture happens **after a completed
turn on that same live connection**. No pre-prompt resumed-process reading is allowed to trigger
rollover: the official SDK does not explicitly guarantee that prior context has loaded then.
The response must match the explicit selected model and contain valid current tokens, effective
and raw windows and auto-compaction state. Cumulative billed tokens and display percentages are
not policy inputs.

The existing native source owner checkpoints account/home/session, file identity, source length
and bounded-tail hash before and after that request. A source change refuses capture. At most
256 bounded 16 KiB metadata records live in `.ao/context-observations`, behind a shared lock and
atomic writes. They contain no conversation body. Before a later turn, the exact source, model,
qualified CLI contract version and compaction setting must still match. Append, replacement,
compaction or malformed/future metadata leaves capacity unknown; cached observations never
constitute a live connection capability. Connection/request UUIDs identify the original capture,
not a fabricated native turn ID. Explicit unknown/default models cannot trigger rollover.
The normal before-turn handoff preserves original source pointers and never replays a turn.

Cursor's official ACP contract documents `initialize`, `session/new`/`session/load`,
`session/prompt`, streaming `session/update`, and `session/cancel`, but no usage/window request,
notification, or `session/prompt` usage result. Its advertised schema therefore cannot establish
a negotiated current-context signal. Cursor capacity remains unknown and proactive policy is
refused with that explanation.

Claude approvals use the separate native stdio control channel described in
[native input](native-worker-input.md). A model-callable permission MCP tool still supplies no
native callback authority, and no such tool or global statusline/hook was installed.

Evidence: the existing native-context fixture covers fresh usage, exact source metadata,
same-model boundaries, cumulative-only input, model change, compaction, missing native turn id,
malformed records and unsupported providers. The existing terminal rollover journey exercises
the exact-turn fixture through the actual UI. These fake-provider checks do not certify live
provider parity.

Primary references inspected 2026-11-03:
- [Codex native turn-context construction](https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn_context.rs)
- [Claude CLI permission-tool boundary](https://code.claude.com/docs/en/cli-reference)
- [Claude interactive statusline](https://code.claude.com/docs/en/statusline)
- [Claude Agent SDK `get_context_usage`](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/client.py)
- [Claude Agent SDK context usage types](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/types.py)
- [Claude Agent SDK v0.2.177 release (bundled CLI 2.1.317)](https://github.com/anthropics/claude-agent-sdk-python/releases/tag/v0.2.177)
- [Cursor ACP transport and session schema](https://cursor.com/docs/cli/acp)
