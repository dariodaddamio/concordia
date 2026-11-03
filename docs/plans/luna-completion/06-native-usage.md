# L6: fuller external native usage

Read the [common worker instructions](README.md), [usage coverage](../../quality/native-usage-coverage.md)
and the accounting phases in [orchestration](../orchestration-completion.md).

## Outcome and ownership

Claude assistant snapshots and Codex last-token observations already have bounded native usage
coverage. Add one newly verified source without double-counting Concordia attempts. Cursor quota
limits are not per-event token usage. Numeric credits are a separate source requirement.

Own `crates/concordia/src/native_usage.rs`, source-specific helpers and, when necessary,
`usage.rs`/`usage_codex.rs`. The coordinator owns shared native-memory readers, provider streams,
TUI token views and MCP registration. Reuse the existing accounting and observation stores.

## Work sequence

1. Verify a supported read-only source with exact provider/account/home/session/event identity,
   counter meaning, revisions and pagination. Record which fields are cumulative versus per-event.
   Do not treat `/usage` percentages, current context capacity or schema declarations as that source.
2. Import bounded observations using stable event/revision identities. Recheck existing page,
   record, binding and catalog limits before implementation; retain bounded reads and cancellation.
3. Expose source coverage, freshness and explicit unknowns. Keep imported observations separate
   from overlapping Concordia attempt totals; never sum both as additional spend.
4. Preserve separate meanings for quota limits, current context, imported native tokens,
   Concordia attempt usage and credit balances. Do not infer money or spent credits from tokens.

## Acceptance and handoff

Extend existing scenarios for repeated imports, revised events, partial pagination, stale sources,
provider/home mismatches and cancellation. No conversation bodies, secrets or credential reads
belong in the usage import. Report exact supported coverage and remaining providers separately.

The currently observed native credit response can report availability while its numeric balance
is null. That is not a locally computable amount. A numeric balance requires a supported numeric
source; retain the honest unknown and official account Usage link otherwise. Do not scrape browser
cookies, private billing endpoints or estimate a balance from pricing.

If the required source is unavailable, return provider/version, evidence and exact missing fields
as `blocked-vendor`, then free the slot. Do not repeatedly re-probe the same unsupported interface.
Follow the common delivery record; no live model turns or account changes are authorized.
