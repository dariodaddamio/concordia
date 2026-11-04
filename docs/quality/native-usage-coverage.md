# Native usage source coverage

Build `2026-11-03l`. Open `/usage`, choose the recorded-tokens view with **T**, then **E native
source**. The view captures the selected provider, account, canonical home and exact native
session. No latest-session lookup, credential scan, model request, pricing lookup or provider-log
mutation occurs. **N** reads the next bounded page; **R** starts a fresh immutable snapshot;
**C** returns to Concordia attempt accounting. No import runs automatically on restart.

The reader reuses native memory's whole-history owner, immutable metadata pointers and original
source verification. Each request parses at most one 8 MiB history page, 2048 records and 256
usage observations, with cancellation checks and verification before publication. Creating or
advancing a pointer and checking source revisions can read up to four such page buffers in total
plus bounded identity headers. The page buffers are released between operations. A cursor binds
the exact page and record boundary. Changed, replaced or truncated sources and foreign
account/home/session cursors are refused. Appends do not extend an in-progress snapshot; restart
the scan to include them. Oversized, malformed and boundary-split records are skipped explicitly.

Claude assistant usage and Codex `token_count.info.last_token_usage` are retained as **source
snapshots**. Metadata includes counter presence, exact source range/hash, observed Claude message
id/model where available, and source kind. Native text is parsed transiently and never copied into
the usage catalog. Unknown counters and model/event identities stay unknown. Session-cumulative
Codex counters are ignored. Cursor remains unavailable: as inspected 2026-11-03, the official
Cursor stream-json output contract documents stable `session_id`, optional `request_id`, terminal
`result`, timing fields, assistant/tool events and correlation IDs, but no input, output, cache,
or total-token counters. The official ACP contract likewise documents JSON-RPC session lifecycle,
`session/update`, permission requests and cancellation, but no usage event, usage request, context
window, or token fields. Session/request IDs identify events but do not provide usage. Quota or
plan/status data is not per-event token usage and is not imported. Cursor therefore remains
`blocked-vendor` for this slice until an official native event with stable event identity and
counter semantics is published.

The separate coverage view has no aggregate token, request or dollar total. Repeated assistant
snapshots and Codex last-request records can overlap one another or foreground/worker accounting;
Concordia does not possess a verified cross-channel request key for every observation. These
records therefore never enter recorded-turn totals, landing totals or billing estimates.
Exact file identity/range/hash deduplicates reimport within a bound catalog, without token/time
heuristics. Native message ids are reported evidence, not a claim of authenticated actor identity.

Catalogs live in owned `.ao/native-usage` state, use the existing atomic lock/write path and retain
metadata-only source offsets/checkpoints. Bounds are 1024 observations, 32 source pages and
512 KiB per binding, with at most 256 binding catalogs and one shared catalog lock.
Corrupt or full catalogs remain intact and require an explicit next decision; no pruning, replay
or inferred migration runs. Refreshing scans from the start with exact-record deduplication.

Recorded Concordia accounting additionally groups attempts, outcomes and full workspace paths,
alongside origin, run, provider, model and account. Account/home/provider/session remain part of
chat identity. Cancellation/failure counters stay in their original attempt. Percentage display
uses widened arithmetic and cannot overflow on large native counts.

Evidence: the existing whole-history fixture exercises both providers through 600 observations,
pagination, repeat import, foreign cursors, cancellation, changed sources and absence of native
body text in metadata. Existing rendering checks cover attempt/outcome/workspace groups and
large percentage values. The existing proactive-context terminal journey opens native source
coverage before any prompt and verifies that returning leaves Concordia totals empty. No real
provider turn was run for this work.

Primary Cursor sources inspected 2026-11-03:

- [Cursor CLI output format](https://cursor.com/docs/cli/reference/output-format) (JSON/stream-json event fields and terminal result schema)
- [Cursor CLI ACP](https://cursor.com/docs/cli/acp) (initialize/session/prompt/update/cancel schema)
