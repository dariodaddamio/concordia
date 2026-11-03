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

Claude's documented interactive statusline is not verified per-launch headless context evidence.
Cursor schema fields do not establish a negotiated live context signal. Both capacity values
remain unknown and proactive policies are refused with that explanation.

The Claude permission MCP boundary also remains unavailable. Its documented tool name and
process ownership do not distinguish native callbacks from arbitrary model-originated calls.
Enabling a permission bridge requires verified native provenance/correlation and exact version,
request/result, launch and cancellation contracts; no generic approval MCP tool was installed.

Evidence: the existing native-context fixture covers fresh usage, exact source metadata,
same-model boundaries, cumulative-only input, model change, compaction, missing native turn id,
malformed records and unsupported providers. The existing terminal rollover journey exercises
the exact-turn fixture through the actual UI. These fake-provider checks do not certify live
provider parity.

Primary references inspected 2026-11-03:
- [Codex native turn-context construction](https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn_context.rs)
- [Claude CLI permission-tool boundary](https://code.claude.com/docs/en/cli-reference)
- [Claude interactive statusline](https://code.claude.com/docs/en/statusline)
