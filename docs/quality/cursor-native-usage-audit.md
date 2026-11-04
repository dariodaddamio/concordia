# L6 Cursor native usage audit

Status: blocked-vendor/data (2026-11-03).

The official Cursor ACP contract documents JSON-RPC `initialize`, `authenticate`,
`session/new`/`session/load`, `session/prompt`, `session/update`, permission requests and
cancellation, but does not define usage-token events or cumulative usage counters:
<https://prod.cursor.com/docs/cli/acp>.

The official CLI overview and usage pages describe interactive/headless operation and modes, not
a supported machine-readable usage accounting source:
<https://cursor.com/docs/cli/overview>
and <https://prod.cursor.com/docs/cli/using>.

Therefore Concordia must not infer Cursor usage from streamed text, ACP notifications, elapsed
time, model names, or availability. No L6 code was added, no quota probe or model call was made,
and no credentials were inspected. Re-open this lane only when a stable official event/schema
provides exact non-overlapping counters with identity and reset semantics.
