# L2 — Claude and Cursor proactive capacity

Read the [common prompt](README.md), [capacity provenance](../../quality/capacity-provenance.md),
`crates/concordia/src/capacity.rs`, and the P5 section of
[the provider plan](../memory-provider-completion.md).

## Existing behavior

Codex exact turn/model/source-bound observations and proactive policy already exist. Reactive
capacity-error recovery and process-local compaction controls also exist. Claude/Cursor current
retained-context signals remain unknown. Quota percentages and cumulative billed tokens are
different data; do not use them as a context-window measurement.

## Assignment

Maintain separate Claude and Cursor subtasks. Find a documented per-launch headless Claude
source or an actually negotiated/reported Cursor source with current usage and a positive
context window. A schema field or an interactive statusline is insufficient. Record version,
session/turn/model identity and source/connection invalidation semantics before enabling anything.

Own `crates/concordia/src/capacity.rs`, provider-specific evidence reader modules agreed with the
coordinator, and the capacity evidence update. Reuse existing `Measurement`/`Observation` types;
add fields only for a demonstrated gap. `native_memory.rs`, provider stream/ACP transports,
shared capability types and before-turn TUI/delegate hooks are coordinator integration surfaces.

First deliver one real supported sensor with a small integration patch into the existing policy.
Do not rewrite Codex rollover, the native memory index, or add another observation framework.

## Acceptance and blocker

- Exact provider/account/home/session/turn/model plus source revision or connection generation.
- Fresh current-window usage; missing, stale, malformed, cumulative-only and unrefreshed
  post-compaction data remain unknown and cannot trigger a rollover.
- Threshold crossing acts before the next turn, preserving original source pointers, pinned
  decisions, draft, cancellation and explicit native-session identity.
- Account/model switches invalidate observations; existing Codex behavior remains working.
- Extend existing capacity/proactive fixtures; live parity is separately budgeted.

If no supported signal exists, return a separate evidence record for each provider and the exact
missing interface. Do not install global hooks, infer model windows, or count a clearer unknown
label as completion. Free the slot while the coordinator continues other ready work.
