# Paused roadmap

The owner paused feature implementation on 2026-11-09. Documentation consolidation and the
product-purpose interview are active in [PLAN.md](../../PLAN.md). Cleanup and current verification
are recorded in [verification status](../e2e-status.md). This backlog is not a shipped capability list.

## Remaining work

- Inspect retained duplicate-chat display snapshots without opening a second chat owner or
  dispatching a provider turn.
- Broaden provider/version and Windows host verification for approvals, workers, input/output and
  lifecycle behavior. Verify native hook trust and real denial separately for each supported binding.
- Recheck supported Cursor capacity/usage signals and omitted numeric credit balances. Missing
  signals remain unknown; do not infer them from estimates.
- Qualify the intended release candidate and exact artifact. The source preservation checkpoint
  `6fea4a9` is complete; it is not full release qualification.
- Add the requested dockable workspace described below.

OS-enforced workspace confinement and public publication remain owner-held. Scope, audience,
support and priorities will follow the product interview. No overall completion percentage is
claimed: implemented slices have different provider and host verification limits.

## Dockable workspace

Requested 2026-11-09; planned, not implemented. Replace fixed file splits and the separate worker
sidebar with one Photoshop-style layout system. Keep chat as the default main surface.

- Add/remove panels through a Panels menu. Drag, dock/undock, resize, tab-stack and collapse them.
  Save named layouts per project and restore placement, size and visibility after restart.
- Prompt context shows skills, instructions, files and references bound to the request. Distinguish
  available, explicitly attached and provider-confirmed used skills; unreported usage stays unknown.
- Goals show objectives, progress, blockers and completion criteria. Agents show native subagents
  and Concordia workers with their tasks, status and results.
- Tools/approvals show activity, pending decisions and unresolved effects. Files, terminal and usage
  can use the same layout system.

Layout owns presentation. Existing chat, worker, goal, document and approval owners retain runtime
state. Moving or hiding a panel must preserve drafts, active runs, pending decisions and exactly one
owner per chat. Hidden pending decisions need a visible attention indicator. Verify actual input,
restart and small-window behavior; keep host limitations explicit.

## References

[Context handoff design](context-handoff-plan.md) records target behavior, not current qualification.
Earlier campaign plans and audit detail are preserved at Git checkpoint `6fea4a9`. Reconcile them
against the implementation and the owner's settled direction before reprioritizing features.
