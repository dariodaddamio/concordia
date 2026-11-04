# Luna roadmap execution plan

Status: execution started 2026-11-03. Prepared 2026-11-03 against build
`2026-11-03w`, source `0caec481a42428f92c024c9429715202cd4e4e74`.
Three Luna planning reviewers audited the current contracts. This document does not claim
implementation or live qualification of the remaining features.

Current execution baseline: `5dbac3e`; coordinator branch `feat/luna-roadmap-completion`.
Host-native Luna workers `l5_integrations`, `l4_file_outputs` and `l3_binary_inputs`
own the first wave. Coordinator owns shared integration and candidate delivery. These are
implementation assignments. The 2026-11-04b candidate integrates the approved source changes and
is installed-qualified by the coordinator; qualification results remain separate from live use.
The 2026-11-03z receipt remains historical.

## Objective and accountability

Finish the remaining product capabilities in the [completion audit](../../feature-completion-audit.md),
using the [shipped-contract map](../../quality/overnight-roadmap-disposition.md) as the baseline.
Do not restart completed foundations from the older area plans. Whole-history retrieval,
ranking/groups, managed worktrees, dependency delivery, routing, ordinary accounting, image/text
attachments, file editing, native Codex/Cursor input and the integration catalog already exist.

The previous campaign repeatedly returned after a local slice or a test run. The coordinator
must own the whole queue, integrate delivered code and immediately assign the next ready slice.
An unsupported message, a written plan, or a passing fixture does not complete a missing feature.
A vendor-blocked requirement stays open until the contract becomes available or the owner
explicitly changes the product scope. Do not silently redefine the roadmap as complete.

## Six Luna assignments

| ID | Owner and brief | First deliverable | Current dependency |
| --- | --- | --- | --- |
| L1 | [Claude native approvals](01-claude-approvals.md) | Authenticated native-request adapter into the existing broker | Establish the installed CLI's permission provenance contract first |
| L2 | [Claude/Cursor context capacity](02-context-capacity.md) | One supported exact current-window sensor, integrated with existing rollover | A documented, actually reported signal; schema-only fields are insufficient |
| L3 | [Native binary/PDF inputs](03-binary-inputs.md) | One bounded, capability-gated native PDF transport with durable snapshots | Verified native media payload and limits for the selected provider/version |
| L4 | [Provider file outputs](04-file-outputs.md) | One additional provider in the existing artifact card/catalog path | Verified completed-event correlation and local-path semantics |
| L5 | [Integration management](05-integrations.md) | F10 connect/verify/disconnect flows using the existing command owner | Buildable now; explicit action review and revision checks |
| L6 | [External native usage](06-native-usage.md) | One newly supported, separately attributed native usage source | Stable native event identity/counters; missing data stays unknown |

Use Luna for these assignments. The planning host currently exposes `gpt-5.6-luna`; a different
execution host must resolve its actual Luna model identifier rather than silently choosing another
model. Do not launch native provider CLIs merely to create these workers. Use the host's approved
agent mechanism. Each worker receives the common instructions below plus its numbered brief.

## Dispatch order and concurrency

There are four available agent slots: one coordinator and at most three workers. No recursive
spawning. There are six workstreams, not six simultaneous workers.

1. Start **L5 integrations**, **L4 file outputs**, and **L3 binary inputs**. These have independent
   implementation surfaces; L3 must verify the transport contract before editing payload code.
2. As slots free, start **L1 approvals**, **L2 capacity**, then **L6 usage**. A precise vendor
   blocker frees a slot immediately; it does not halt the rest of the campaign.
3. Split each feature into independently reviewable code deliveries. After one provider is
   delivered, keep the other promised provider cells open and assign the next supported cell.
4. A blocked worker returns evidence and a concrete next dependency. The coordinator assigns
   another ready task; do not spend a whole run repeatedly rediscovering the same missing API.
5. After integration, prepare a single exact candidate and its qualification record. Continue
   independent implementation while background CI runs. Investigate actual failures, not elapsed time.

Provider research can move earlier if the coordinator has an idle slot. Do not create extra
framework work to keep a blocked agent occupied.

## Shared-file rule

Workers own the files named in their briefs and their feature-specific documentation. The
coordinator alone integrates shared dispatch and canonical contracts:

- `crates/concordia/src/tui/mod.rs`, `tui/questions.rs`, `tui/queue.rs`, `tui/tokens.rs`,
  `tui/files.rs`, `tui/artifacts.rs` and picker/command registration.
- `crates/concordia/src/provider/turn.rs`, `provider/turn_appserver.rs`, `provider/turn_acp.rs`,
  `provider/stream_json.rs`, `provider/stream_cards.rs`, `provider/interaction.rs`,
  `provider/compat.rs`, `provider/claude.rs` and provider module registration.
- `crates/concordia/src/native_memory.rs`, `native_input.rs`, `delegate.rs`, `integration.rs`,
  `mcp.rs`, shared capability/persistence migrations and the common fake-provider process.
- `BUILD_STAMP`, `CHANGELOG.md`, `PLAN.md`, the completion audit, this queue and release workflow.

Workers return a small integration patch against the exact baseline for a shared file, or request
a temporary exclusive handover. They must not edit it concurrently. Register a proposed new
module with the coordinator before relying on it; uncompiled orphan files are not a delivery.
Each implementation branch/worktree has one owner. Do not share a writable Cargo target between
independent builds; the coordinator serializes shared-target Cargo commands to avoid lock churn
and overwriting the candidate. Never remove another worker's checkout or user work.

## Common worker prompt

> You are Luna, implementing the numbered brief supplied with this prompt. Read AGENTS.md,
> .cursor/rules/workflow.mdc, this plan, your brief, and its current evidence documents.
> Reconcile the live baseline before editing. Preserve already shipped behavior, especially
> draft/queue recovery, private header email, concurrent foreground chats and Home navigation.
> Own only assigned files. Extend the existing store/broker/adapter; do not create a competing
> executor, approval queue, artifact registry, accounting ledger or authentication layer.
> First establish any required native contract using official documentation and bounded read-only
> installed-CLI inspection. Do not run real provider model turns, read credentials, install global
> hooks, change authentication, or perform release/account actions. Fake-provider implementation
> checks are allowed. Extend existing test scenarios/helpers; new unit/integration test functions
> or files require separate authorization. Do not add tests that merely mirror the implementation.
> Implement the smallest complete user-visible slice, not just a capability stub. Use focused
> checks; do not wait for the whole suite before continuing independent work. Return the delivery
> record below. If a required source/contract is missing, return a precise blocker and stop that
> slice without calling it done. Do not spawn agents or self-authorize native usage.

## Delivery record and coordinator board

Every handoff includes: task/subtask ID; baseline/head commits; changed paths; migration/rollback
notes; callable integration API and shared-file patch; observable user behavior; actual focused
check commands/results; artifact identity if built; unresolved limits; and the next ready task.
The coordinator reviews the diff and ownership before merging and updates the existing audit
and relevant evidence document. A new planning document is not an implementation receipt.

| Task | Initial state | Completion condition |
| --- | --- | --- |
| L1 Claude approvals | Integrated: 2026-11-04b / CLI 2.1.318 | Source d322 passes 26 native unit cases and extended background/cancel/foreign/EOF process fixtures; local_agent/local_workflow retention is bounded at 10 minutes. Live turns, remote/shell/monitor lifecycles and HTML previews remain excluded |
| L2 Claude capacity | Integrated: 2026-11-04b / CLI 2.1.318 | Completed-turn control capture and exact source/model/settings reuse remain; no speculative pre-prompt sensor |
| L2 Cursor capacity | Blocked-vendor; [source audit](../../quality/capacity-provenance.md) | Supported current-window request/event is absent from the documented Cursor transport |
| L3 binary inputs | Integrated: 2026-11-04b / CLI 2.1.318 | Bounded Claude PDF snapshots/dispatch and process checks are integrated; live processing, physical clipboard/viewer behavior remain unqualified; other-provider rejection is explicit |
| L4 Claude outputs | Integrated: Luna `l3_binary_inputs` + coordinator, z | Built-in Write/Edit correlation plus successful native result metadata; unsupported formats stay excluded |
| L4 Cursor outputs | Integrated: Luna `l4_file_outputs` + coordinator, z | Exact-session completed write/edit success path through the existing registry |
| L5 integrations | Integrated: Luna `l5_integrations` + coordinator, z | New local manifest and reviewed reconnect/verify/selected disconnect; draft retained |
| L6 native usage | Blocked-vendor; [Cursor evidence](../../quality/native-usage-coverage.md) | No supported per-event counters; quotas remain distinct and no fabricated totals were added |
| Numeric credit balance when native response is null | Vendor/data blocked | Requires a supported numeric response; availability is not an amount |
| Live parity and memory-retention trials | Partial: Cursor live continuation exercised | Owner-authorized bounded Cursor read-only/exact-session trials ran; replay defect reproduced and corrected in d. Other provider/parity trials remain open |
| Signing and publication | Owner/release gated | Identity, distribution decisions and exact-artifact release evidence |

Use `ready`, `building`, `review`, `integrated`, `blocked-vendor`, `blocked-owner` for work state.
Track fixture, installed, live and release evidence separately. Blocked is not integrated, and
integrated is not live-qualified. The board must name the actual owner and commit when work starts.

## Qualification, release, and stop conditions

The coordinator owns candidate assembly using the existing evidence collector, smoke/signing
scripts and release workflow. Do not build another qualification framework. Reconcile stale
claims in the older release plan instead of reimplementing the shipped collector/signing checks.

For a production slice: inspect the diff, run focused checks and applicable lint/build checks,
integrate the complete behavior, and keep full CI running without idling the implementation queue.
Do not repeatedly rebuild an unchanged candidate. Required repository protections still apply;
do not bypass them with administrator flags. Planning-only edits need link/diff/privacy checks,
not Cargo builds. For an integrated release candidate, bind all receipts to its source and hash.

Live model validation remains disabled until a provider/task scope and usage budget are explicitly
given. The owner's reaffirmed Cursor authorization covers bounded synthetic Cursor live checks;
read-only/exact-session results are recorded in `docs/e2e-status.md`. Preparing other cases and
synthetic fixtures is autonomous. No certificate provisioning,
paid services, vendor submissions, external uploads, tags, releases or visibility changes are
authorized by this plan. Prepare the concrete local artifact before requesting any final release action.

Do not stop the campaign at the end of a slice, at a long CI run, or because one provider is
blocked while another ready task exists. When no ready work remains, report shipped features,
exact open blockers and the smallest required owner/vendor decisions. Claim full roadmap
completion only when the agreed capability matrix and its required evidence are actually closed.
