# Attributable worker and main accounting

Baseline: `1301b11`. Build `2026-11-02o`. State: fixture-verified; final candidate checks follow.

## Identity, persistence and coverage

Every new native main/worker attempt has an origin, run/attempt UUID, captured home identity,
provider/account/model/workspace, native session when reported, outcome and per-field coverage.
Worker records also retain an available native result pointer. Records contain numeric metadata,
not prompts, replies, credentials or inferred prices. Legacy entries remain readable with unknown
attribution; they are not rewritten or reimported.

Attributed events are immutable, keyed by origin/run/attempt and written under one OS lock.
Repeating the same record is idempotent; conflicting counters preserve the original and fail
visibly. The event store has a 10,000-record/16 KiB-per-record bound. A full or failed store keeps
existing events; finalized worker metadata remains available for reconciliation and is not pruned.
No automatic cleanup is introduced.

Worker ledger v3 captures each attempt before dispatch and checkpoints usage before failover.
It migrates v1/v2 without inventing old attempt IDs or replaying execution. Old executables that
only understand v2 reject v3. Recovery can finish an interrupted accounting write idempotently;
it never runs a provider to obtain a number. Interrupted attempts preserve known fields and mark
the rest unknown. Result collection and recovery do not write duplicate usage.

## Snapshot semantics and native limits

Adapters report terminal snapshots, so repeated notifications replace supplied fields rather than
adding again. A new native launch gets a fresh accumulator. Codex's input includes cached tokens;
the existing three-part normalization keeps those tokens counted once. Claude fresh-input totals
include separately reported cache creation when both fields are present.

Primary reference: [Codex event types](https://github.com/openai/codex/blob/main/sdk/typescript/src/events.ts).
[Claude's usage documentation](https://code.claude.com/docs/en/agent-sdk/cost-tracking) describes
client-side cost estimates, restored session cost on resume and main-loop/subagent scope differences.
Concordia therefore leaves resumed-Claude per-attempt cost unknown without a verified delta and
does not infer complete native subagent token coverage. No subscription percentage becomes a price.

The token view shows reported-cost coverage and incomplete/legacy counters. A known subset is
not labelled the complete bill. External native-session import and cross-origin native event
deduplication remain U2 work; this slice totals Concordia observations only.

## Bounded reads and UI ownership

Reads cover at most 32 MiB and 10,000 records per page, including malformed records in the count.
New attributed events precede legacy records so recent main/worker accounting is not hidden
behind a large historical legacy log.
Legacy offsets and the immutable-event catalog bind the exact snapshot revision. A changed
snapshot requires a restart. Oversized/malformed records produce partial coverage, not invented
zero usage. Managed event/legacy paths reject links and unexpected file types.

At most two cancellable usage reads run per app. The token screen and landing totals read and
aggregate off the input/render thread; late results must match their view generation. Closing or
switching a view cancels its work. N reads the next page and R restarts the snapshot. Page-limited
totals are explicit; account/home identity prevents equal session IDs from merging.

## Evidence

All 885 enabled library checks, five delegate integrations and seven enabled reliability checks
pass at the initial accounting tree. Existing fixtures cover snapshot repetition, immutable
deduplication, a 10,001-record page boundary, stale cursors, cancellation and mixed legacy records.
Targeted crash accounting and failover recovery checks pass. The provider runner fixture verifies
fresh versus resumed Claude cost handling without a model turn. Main attribution remains bound
to the captured launch identity even if the focused account changes; closed-view results preserve
the draft and cannot reopen the view. Final candidate and installed checks remain separate.
