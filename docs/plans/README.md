# Planned work and backlog

This is the single canonical backlog for work that is not part of the current
shipped contract. [PLAN.md](../../PLAN.md) and [verification status](../e2e-status.md)
remain authoritative for current behavior. A design document, passing fixture, or
integrated slice does not by itself make a capability generally qualified.

## Current backlog

Reconciled 2026-11-09 against installed 09g, the source owners and dated verification records.
Most planned capability groups have implementations; the entire roadmap is not complete.
There is no stable weighted denominator for an overall percentage. In the final six-workstream
campaign, L1 approvals, L3 selected binary inputs, L4 structured outputs and L5 integrations are
integrated within their documented provider contracts. L2 capacity is partial (Codex/Claude
observations, no qualified Cursor sensor); L6 external usage has bounded coverage with missing
Cursor counters. Integrated does not mean every provider, transport and physical host is verified.

The source-pruning and obsolete-build cleanup passes are complete within their recorded scope.
09g adds shared setup/command repairs and native-request retransmission handling. The verification
build cache is removed; the older executable still used by an open window is intentionally kept.
The audited source still needs a Git checkpoint. Four previously policy-blocked synthetic Temp
directories also remain; their disposition is in [cleanup closeout](../agent/cleanup-review.md#cleanup-closeout-2026-11-09d).

The dockable workspace below is the new feature requested in the November 9 UI discussion.
The other open items predate that request. Finish implementation and its verification before the
broader documentation, product direction and support discussion; that discussion remains postponed.

| Group | Remaining work | State and completion condition |
| --- | --- | --- |
| Existing | Inspect retained duplicate-tab display snapshots | Open local implementation. Provide read-only inspection without creating another chat owner or dispatching work. |
| Existing | Git checkpoint and candidate qualification | Open closeout. Commit the audited source and bind candidate checks to its source and executable. No publication is implied. |
| Existing | Native hooks and provider/host verification | Partial. Verify native trust and real denial where supported; close remaining Windows host, approval, worker and lifecycle cases. |
| Existing | Missing native signals | Previously vendor/data blocked. Recheck supported Cursor capacity/usage and omitted numeric credits; unavailable values remain unknown. |
| New | Dockable workspace and runtime panels | Planned. One configurable panel layout with saved project layouts and the acceptance requirements below. |
| Later | Documentation and product/support scope | After implementation closeout. Consolidate guides and formatting debt; consider GitHub Pages. Broader direction/support discussion stays postponed. |
| Held | Deferred scope and public release | Owner-held. New OS confinement, publication and exact-artifact release qualification remain separate. |

Native hook installation is separate from native consent and proven enforcement. Source state
inventories and selected fixtures are not exhaustive state-machine or provider/host proofs.
The missing-signal investigation is not a newly verified provider capability.

## Dockable workspace and runtime panels

Requested 2026-11-09; planned, not implemented. Replace the fixed file splits and separate worker
sidebar with one Photoshop-style docking system. Keep chat as the default main surface.

- Add/remove panels through a Panels menu; drag, dock/undock, resize, tab-stack and collapse them.
  Save named layouts per project and restore their placement, size and visibility after restart.
- Prompt context shows skills, instructions, files and references bound to the request. Distinguish
  available, explicitly attached and provider-confirmed used skills; unreported usage stays unknown.
- Goals show the objective, progress, blockers and completion criteria. Agents show native subagents
  and Concordia workers with their tasks, status and results.
- Tools/approvals show activity, pending decisions and unresolved effects. Files, terminal and usage
  can be added as panels within the same layout system.

Layout owns presentation and stable panel identities; the existing chat, worker, goal, document and
approval owners retain runtime state. Moving or hiding a panel must preserve drafts, active runs,
pending decisions and exactly one owner for each chat. Hidden pending decisions need a visible
attention indicator. Verify add/remove/reorder/resize and restart through actual UI input, including
small-window behavior. Host limitations must remain explicit.

Historical design briefs: [L1 approvals](luna-completion/01-claude-approvals.md),
[L2 capacity](luna-completion/02-context-capacity.md), [L3 inputs](luna-completion/03-binary-inputs.md),
[L4 outputs](luna-completion/04-file-outputs.md), [L5 integrations](luna-completion/05-integrations.md)
and [L6 usage](luna-completion/06-native-usage.md). Evidence remains in the
[implementation disposition](../quality/overnight-roadmap-disposition.md) and
[verification status](../e2e-status.md); [release gates](../release-blockers.md) are separate.

## Audit follow-ups

The owner approved the six-step local follow-through, then product direction as step 7.
The tab-restoration implementation, scoped provider/worker qualification and cleanup blockers
are recorded in [08h follow-through](../agent/continuity-follow-through.md).

The implementation repair pass is recorded in [cleanup review](../agent/cleanup-review.md).
Build 2026-11-08g completes the whole-repository source audit and necessary repairs/pruning;
the remaining items below are explicit qualification limits or future work, not deferred
production modules awaiting that audit. Per-file depth and execution evidence are recorded
separately so fixture success is not presented as universal correctness.
Progress/text/card budgets, OS-owned store locking, checked read warnings, paged journals,
recovery bounds, async redraw and safe closed-tab retention now have concrete implementations.
Native session cleanup reports candidates and preserves the sources. Privacy scan exceptions
now match exact approved public-theme references; this does not authorize publication.

The remaining distinctions are:

- Reproduce the historical stale-map report using the exact conversation/task/account/session
  binding if it recurs. Current identity/continuation paths are verified within their stated scope;
  no newest-session substitution was introduced. The original older-Claude/newer-Codex symptom
  is not claimed reproduced or universally fixed.
- Protected-work retention is deliberate: unsaved/unfinished closed tabs, active inspections and
  unavailable-source views can exceed resting projection budgets. A finite queue/projection is
  not a global small-process-memory claim. Originals and recovery are kept rather than evicted.
- Investigate the recorded five-second history-load timeout under concurrent native fixture load
  if reproducible; unchanged serial reruns and the final build passed. The failed run passed its
  identity guards, so the defensive mismatch notice is not claimed to fix that latency.
- Qualify the owner's chosen provider/host/release matrix. Fixture success and one small live
  Codex turn do not establish every approval, worker, transport, physical host or source shape.
- Mixed-version writers need compatible locking semantics. Windows new locks own their pathname;
  unknown legacy lock records fail closed. Do not infer Unix old-writer exclusion from Windows
  evidence or automatically remove an unrecognized lock file.

Product emphasis, first-release audience and publication/support scope are the next owner
choices; no additional hosted service, automatic summarization, provider compaction change or
public support promise is implied by the cleanup.

## Design and acceptance references

- [Context handoff design](context-handoff-plan.md) — target behavior and research; not a status page.
- [Completion roadmap](completion-roadmap.md) — historical acceptance references.
- [Memory/provider plan](memory-provider-completion.md) — historical phase design.
- [Orchestration plan](orchestration-completion.md) — historical phase design.
- [Workspace/input plan](workspace-input-completion.md) — historical phase design.
- [Validation/release plan](validation-release-completion.md) — historical gates, with current unsigned policy linked inside.
- [Launch strategy](launch-plan.md) and [social draft](social-series.md) — publication drafts only; no announcement authorization.

Archived campaign records and superseded strategy are in [archive](../archive/README.md).
