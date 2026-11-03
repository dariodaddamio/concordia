# Orchestration, routing and accounting completion plan

Status: proposed, based on source revision `c6d3b7c` (2026-11-02). This document is a plan,
not a claim that the capabilities below have shipped. Planning changes only this file.

The outcome is a bounded task graph whose workers have explicit workspace ownership, visible
routing decisions and attributable usage. Existing native authentication, approval, cancellation,
recovery and source-pointer behavior remains authoritative. No provider turns, account changes,
production edits or new tests are required to prepare this plan.

## What already exists

Source paths below are under `crates/concordia/src/` unless fully qualified. The core
execution/recovery pair is `crates/concordia/src/delegate.rs` and
`crates/concordia/src/delegate_recovery.rs`; its UI owner is
`crates/concordia/src/tui/workers.rs`.

| Capability | Source evidence | Remaining gap |
| --- | --- | --- |
| Three workers, one active task and two queued tasks per worker; selected native model and inherited/tighter trust | `crates/concordia/src/delegate.rs`, `store.rs`, `tui/workers.rs` | Dependencies and workspace isolation are absent. Worker prompts are advisory instructions. |
| Stable run/conversation UUIDs, pane-owned OS lock, atomic worker ledger, source pointers and reserved delivery | `delegate_recovery.rs` | No graph edges, workspace registry or attempt UUID is persisted. |
| Restart pauses uncertain work and explicit reconciled retry creates a new task | `delegate_recovery.rs::attach_recovery`, `recovered_task`; `tui/workers.rs::worker_recovery_command` | Dependency retries must preserve the distinction between the prior run and a new execution. |
| Opt-in worker quota failover, at most four distinct accounts, before tools or partial output | `delegate_recovery.rs::worker_failover` | Selection and exclusion reasons are embedded in the execution path rather than inspectable policy output. |
| Main-account/model continuation, session preference and explicit context rollover | `tui/continuation.rs::next_usage_target`, `config.rs::Continuation` | No single persisted ordered account/model policy with complete eligibility explanations. Existing main and worker retry budgets differ. |
| Local main-turn token log and provider/account/model/project/chat breakdowns | `token_log.rs`, `tui/tokens.rs`, `tui/mod.rs::record_tokens` | Worker `TurnProgress::Usage` is ignored in `delegate.rs::apply_progress`; entries lack execution/provenance IDs. External sessions are not consolidated. |

Current limits remain the starting contract: three enrolled workers, two queued tasks per
worker, six context messages/about 3,000 context characters, 64 persisted worker runs/8 MiB,
eight finished jobs in the ordinary viewer, four worker failover attempts and three main
quota handoffs. More graph nodes must not silently increase running-provider concurrency.

## Shared contracts before implementation

Use the following identities across the workstreams, with names finalized once by the integrator.
These are proposed types, not existing APIs.

| Identity/record | Meaning and invariant |
| --- | --- |
| `TaskId` | Stable graph node UUID. A user-approved task and its dependencies are separate from an execution. Legacy jobs acquire a task binding without changing their existing run UUID. |
| `RunId` | Existing worker execution UUID. An explicit reconciled retry receives a new run and points to its predecessor; a dependency cannot silently retarget to it. Numeric viewer IDs are never durable references. |
| `AttemptId` | New UUID for each native provider launch within a run, including failover. Records exact account/home identity, provider/model, source/session and outcome. Its purpose is attribution and deduplication, not automatic replay. |
| `WorkspaceBinding` | Owner workspace identity plus execution workspace identity, canonical repository/common-Git-dir identity, canonical cwd, optional managed-worktree ID, branch/base/head and registry revision. Account home and execution cwd remain different fields. |
| `RouteDecision` | Ordered candidates, chosen target, policy revision, per-candidate eligibility/reasons and observation freshness. Unknown quota/cost/capability remains unknown. |
| `UsageEvent` | Origin, run/attempt or native-source event key, account/home/provider/model/session, reported counters/cost, completeness and provenance. No native transcript text. |

The worker ledger currently requires every saved `JobSpec.cwd` to canonicalize to the owning
workspace. A child-worktree cwd therefore needs an explicit versioned workspace binding and
validation migration; changing `cwd` alone would break recovery. The new schema must load
version 1 records as shared-workspace tasks with no dependencies and preserve the original on
validation/write failure. Unsupported future versions remain unavailable rather than guessed.

Storage mutations use the existing atomic/locked state helpers. Persist a launch reservation
before creating a process. A lost acknowledgement or crash yields paused/uncertain state;
restoring a graph never runs a provider. Changing policy, account registration, workspace or
model catalog must cause eligibility to be evaluated again immediately before launch.

## Workstream D: task dependencies

### D1 — durable graph and eligibility (medium, first independent foundation slice)

Add a small UI-independent scheduler in proposed `crates/concordia/src/delegate/schedule.rs`.
Keep `delegate.rs` responsible for execution and `delegate_recovery.rs` responsible for its
existing owner lease; neither TUI drawing nor provider output may mutate dependency edges.

Persist at most 64 task nodes and 256 edges within the existing 8 MiB ledger budget. Refuse
overflow, duplicate IDs, unknown prerequisites, self-links and cycles before modifying durable
state. Edges bind the task and the accepted successful run UUID. A failure, cancellation,
uncertain run or newly retried run never satisfies an earlier edge implicitly.

Separate dependency readiness from `WorkerState`: `waiting_dependencies`, `ready`,
`blocked_dependency`, `paused_recovery` and an explicit block reason are scheduler metadata.
Do not treat `WorkerState::Paused.is_finished()` as successful dependency completion. Resolve
ready tasks in stable submission order, subject to the existing worker/account/queue caps.
Blocked tasks do not reserve an account or start a process.

The foundation slice supplies persistence, validation and read-only eligibility output without
automatic dispatch or new UI commands. Its observable result is a reloadable bounded graph that
explains why a node is ready or blocked. It can be reviewed before worktrees, routing changes or
native approval transport work, but must not be advertised as a usable dependency feature until
D2 supplies approved submission and dispatch.

### D2 — approved submission and bounded dispatch (medium, depends on D1)

Expose dependency selection through the existing `/delegate` command family and worker detail
screen. Finalize the command grammar with `commands.rs`/`tui/workers.rs` in one integration
change; do not create a second queue. A proposal remains a proposal until the user approves the
whole task and its dependencies. Dependency completion cannot authorize new work the user did
not approve. Worker output never creates further workers or graph edges.

Launch only when all prerequisites have durable successful outcomes, the task is approved,
workspace requirements are satisfied, and normal routing/trust checks pass. Reserve the run
before launch. Result input uses exact verified native pointers and existing bounded context
delivery; do not store parent output text in the graph or assume a source pointer is fresh.

For dependencies across isolated worktrees, successful execution alone does not mean its edits
exist in the downstream cwd. Add an explicit `integration_required` block with a reviewed
base/head receipt. The owner explicitly integrates changes or deliberately accepts a
result-only dependency. The scheduler never merges or cherry-picks automatically.

Cancelling a prerequisite blocks its dependants with a visible reason, without cancelling
unrelated running work. Cancelling a waiting node launches nothing. Explicit removal of a
referenced node is refused until dependants are removed or their edges are explicitly edited.
Keep lightweight terminal dependency receipts while referenced; ordinary eight-job viewer
pruning must not erase prerequisites or turn a missing record into success.

### D3 — restart/reconciliation and inspection (medium, depends on D2)

Restore every nonterminal graph execution paused under the existing pane lease. Never infer
completion from exit-like text, an existing branch, process absence or the newest native chat.
Inspection shows accepted prerequisite run IDs, pending integration, native source identity,
account/model, routing reason and workspace. Explicit reconciled retry creates a new run;
retargeting blocked dependants to that run is a separate visible graph update. Reserved result
delivery remains at-most-once automatic dispatch, not exactly-once native execution.

## Workstream W: file ownership and managed workspaces

### W1 — declarations, conflict reservations and registry (medium, independent of D2)

Add proposed `crates/concordia/src/workspaces.rs` for the bounded registry and canonical binding
validation; keep rendering in existing `tui/workers.rs`/workspace modules. A run can declare
read paths and write ownership of explicit files or directory subtrees. Start with at most 64
declarations per task and no glob language. Normalize path identity under the canonical
workspace, handle Windows case rules and reject traversal, junction/symlink escapes, `.git`
internals and ambiguous missing-path parents.

An OS-locked registry serializes conflicting scheduler reservations across Concordia panes.
Overlapping writes, or a write intersecting another task's declared stable read snapshot,
block dispatch with the owning task/run ID. Release a reservation after a confirmed process
exit; uncertainty retains a reconciliation requirement rather than freeing ownership on a timer.

These declarations enforce scheduler admission only. They do not prevent arbitrary native
tools, an external editor or a full-trust provider from writing elsewhere. Label shared-folder
claims advisory with respect to filesystem access; neither a prompt nor a Git worktree is a
filesystem sandbox. Do not advertise enforced file isolation on this slice.

### W2 — explicit managed Git worktree creation (large, depends on W1/schema binding)

Use Git's supported CLI with executable/argument arrays, bounded output/time and tracked
cancellation. Bind the canonical repository and common Git directory, an explicit base commit,
new branch and absolute managed path to a registry ID. Reject branch/path collisions. Preserve
existing dirty work; do not stash, copy, reset or enroll existing worktrees implicitly. For a
first slice, require a clean committed base and report that uncommitted source changes are not
included. Missing Git/non-Git workspaces receive an explicit unsupported-isolation result.

Persist `creating` before invoking Git; then verify the created worktree identity and head before
allowing a run. A crash or cancellation during creation leaves `reconcile_required`; resuming
does not reissue the mutation blindly. Recovery validates the execution cwd against its recorded
owner/worktree binding and registry revision, not the original owner cwd equality rule.

Native provider sandbox capabilities are a separate contract owned with the provider workstream.
Where the verified provider/version can confine writes to the selected worktree, apply its
supported settings and report the actual boundary. Where it cannot, expose the limitation or
refuse a requested enforced mode; never substitute an advisory claim silently. Full trust
cannot simultaneously promise filesystem confinement. Shared-folder write enforcement remains
unsupported unless an actual supported boundary can be verified.

### W3 — reviewed integration and retirement (large, depends on W2)

Show base/head, changed paths and conflicts before explicit integration. Any selected merge or
cherry-pick is a distinct user-authorized action with version revalidation and normal native
approval; task completion never authorizes it. Do not build a parallel Git merge engine.

Separate `active`, `paused`, `review_required`, `retained` and `retirable` lifecycle states from
worker execution state. Default to retain. A retirement preview identifies the exact owned
worktree/branch, active process/lease checks, dirty files, unique commits and dependency/result
references. Re-evaluate the same identities/revisions immediately before an explicitly authorized
retirement. Refuse dirty or unique-commit removal, active native sessions, external worktrees and
paths outside the registry root. Do not use force flags or age-based deletion. Deleting branches
is a separate action. Unknown native process ownership means retained/reconciliation required.

## Workstream R: deterministic routing and eligibility explanations

### R1 — read-only route evaluator (medium, independent first slice)

Introduce proposed `crates/concordia/src/routing.rs` returning `RouteDecision` from an immutable
snapshot. Extract the checks currently in `tui/continuation.rs`, `delegate.rs` and
`delegate_recovery.rs`; the policy function does not spawn, log in, probe a model or write state.
The same evaluator powers candidate explanations and actual dispatch preflight.

Candidate reasons include missing account/home, missing native login, duplicate home/login,
orchestrator exclusion, running worker, invalid local model ID, already-attempted target,
trust/workspace/capability restriction, known exhausted quota, stale observation and unknown
capacity. Eligibility and capacity are distinct: an authenticated target with unknown quota may
be eligible under explicit policy, but must never be labelled as known available capacity.
Include account-home identity, observation timestamp/source and policy revision in the snapshot.

This slice can ship as inspection output while preserving selection behavior. Acceptance is
that shown reasons exactly match the launch preflight for the same snapshot, with no hidden
model/network call and no change to manual continuation defaults.

### R2 — persisted ordered account/model policies (medium, depends on R1)

Keep user-written `config.toml` read-only. Add a versioned typed policy and persisted TUI
override in `store.rs`, respecting the established config-default/store-override/session-choice
precedence. Translate legacy `continuation.accounts`, `continuation.models`, worker order and
`worker_models` into an equivalent candidate sequence when no new policy exists. Do not rewrite
the user's config or reorder existing preferences during migration.

Capture the effective candidate sequence/revision on task approval. At dispatch revalidate
identity, capability and availability; a newer policy applies to future tasks unless explicitly
rebound. Keep context exhaustion, quota exhaustion, cancellation and uncertain completion as
separate triggers. Preserve the current three-main-handoff/four-worker-attempt limits and
one explicit context-error rollover; a common evaluator does not unify or enlarge retry budgets.
An explicit configured model must be validated in that account's local catalog and never
silently replaced. Policy edits and inspection do not consume provider turns.

### R3 — UI and worker/main parity (medium, depends on R2 and provider capability contract)

Use existing handoff and worker detail surfaces to show selected target and skipped-candidate
reasons before approval and after automatic failover. Freeze the original request identity,
preserve drafts/queues, and refuse uncertain replay. Worker failover still requires confirmed
terminal quota failure before tools/partial text; other outcomes pause. Native approval state
never follows a candidate switch. Keep live compatibility claims separate from fixture proof.

## Workstream U: attributable usage and cost

### U1 — per-attempt worker accounting (medium, independent first slice)

Extend `token_log.rs` with versioned event identity/provenance and bounded paginated reads.
Capture `TurnProgress::Usage` per `AttemptId` in `delegate.rs` instead of ignoring it. Audit
provider adapter semantics first: distinguish cumulative snapshots from deltas, and update only
the intended fields. Do not sum repeated snapshots. Reset the accumulator on every new native
attempt and record each attempt before failover, including failed/cancelled attempts that
reported usage. Missing fields remain unknown in the new schema, not invented zero usage.

Use an idempotent bounded event store or append log with a durable event index; a task's result
delivery/collection must not write usage again. If a crash interrupts final accounting, record
an explicit incomplete/unknown event or reconcile from a verified native usage event; do not
relaunch work to obtain a number. Keep the existing main-turn log readable, preserving legacy
entries with explicit unknown attribution. A one-time migration marker prevents reimport.

Start with a 32 MiB read page, 10,000 events/page, resumable exact cursor and visible partial
totals at a bound; tune only from measurements. Reads/aggregation run off the render/input
thread. Keep a complete/partial/unknown coverage label and separate reported-cost coverage from
token coverage. Existing `Totals.cost_usd` sums only reported costs; do not display that subset
as the complete bill. Subscription quota percentages are not dollar prices.

The first slice reports new worker attempts in the existing tokens screen by origin/run/account
with native counters and known-cost coverage. It needs no graph scheduling or worktree mutation.

### U2 — bounded external native-session import (large, depends on U1 and memory source APIs)

Read only supported usage records through identity-bound native-source pagination. Never scan
credential files, mutate provider logs or infer billing from text/model names. Bind imported
events to provider, account home, exact native session and source event/range revision. Track
append checkpoints and source invalidation without storing native conversation text.

Deduplicate a native event that also has a Concordia main/worker observation using a verified
session and stable native request/event key. If a provider lacks a reliable key, show separate
external coverage or refuse a combined total rather than counting twice heuristically. Do not
deduplicate by matching token numbers/timestamps. Session-cumulative counters require explicit
reset/compaction-aware deltas or remain unavailable. Cursor/other unsupported native usage
streams remain labelled unavailable until a supported interface supplies them.

### U3 — consolidated views (small/medium, depends on U1; external filter depends on U2)

Extend `tui/tokens.rs` and landing totals to group by origin, task/run, provider, model, account,
canonical workspace and native session identity. Do not group different accounts' equal session
IDs into one chat. Provide source/coverage detail, cancellation totals and all-attempt totals so
failover never hides spent usage. No price lookup or inferred dollar estimate is needed; any
future configured price estimator is separately labelled and must not overwrite reported cost.

## Acceptance matrix and delivery evidence

Use existing `crates/concordia/src/delegate/tests.rs` and `delegate_recovery.rs` scenarios,
`crates/concordia/tests/delegate.rs`, `crates/concordia/tests/continuity.rs`,
`crates/concordia/tests/reliability.rs`, `crates/concordia/tests/native_memory.rs`,
`crates/concordia/src/token_log.rs`/TUI fixtures and
`crates/concordia/tests/tui_journeys.rs` as the verification surface. Do not add
agent-written unit/integration tests without the
required instruction; implementers can document and exercise the state matrix below through
existing fixtures and integrity checks. A docs-only plan does not require Cargo execution.

| Contract/state matrix | Observable acceptance |
| --- | --- |
| Dependency success/failure/cancel/uncertain; cycle/self/unknown/cross-conversation edge; reference pruning | Only the exact accepted successful run unblocks a node; rejected updates leave durable state unchanged. |
| Full worker queue, blocked task, duplicate completion, restart before/after launch reservation | Existing concurrency caps hold; no duplicate launch; restart launches nothing until explicit reconciliation. |
| Child cwd, renamed/replaced worktree, repo mismatch, conflicting claims across panes | Exact workspace binding validates or blocks visibly; advisory claims are never described as a sandbox. |
| Dirty source/base, cancellation during Git mutation, branch/path collision, linked/junction path escape | Existing work is preserved; interrupted mutation requires reconciliation; no force/reset/clean commands occur. |
| Retirement with dirty files, unique commits, active/unknown process ownership, referenced outputs | Retirement is refused and the owned workspace is retained; external workspaces are untouched. |
| Quota/context/connection/unknown exit; invalid model; same login/home; fresh/stale/unknown observation | Explanation and dispatch agree; retry budgets/trust remain intact; unknown capacity stays unknown. |
| Repeated usage snapshots/deltas, failover, failure/cancel, collection twice, crash/reimport | Each attributable attempt is counted once; incomplete counters and cost coverage remain explicit. |
| Legacy token log/worker ledger; oversized/corrupt/newer-version state | Existing records stay readable where supported; originals are preserved; bounds are visible and no replay occurs. |
| Main/worker and imported native observation of one event; same session ID on two accounts | Verified duplicates count once; ambiguous events are separated; distinct account/home identities never collapse. |

For each production slice run formatting, relevant existing fixture checks, clippy with warnings
denied, required everyday smoke and the affected Windows journeys. At integration, run the full
enabled suite and CI against the exact commit; record build stamp, artifact identity and real
results. Bump `BUILD_STAMP`/changelog for user-visible changes. A fixture pass establishes local
state behavior, not live native sandbox/auth/approval parity or token/cost savings. Live model
validation requires a separately specified task/provider scope and usage budget under `AGENTS.md`.

## Order, ownership and decisions

1. **Campaign wave 1:** finalize shared Run/Attempt/Workspace contracts. This lane starts D1;
   the other two campaign owners start the provider interaction/capability foundation and cwd
   search model. Run at most three implementation owners in total, with one integrator for
   `delegate.rs`, ledger migration and shared TUI code. R1 and U1 are independently schedulable
   later candidates within this lane, not requests for three additional parallel agents.
   U1 is the smallest immediately useful accounting improvement; D1 is the scheduler foundation,
   and D1 plus D2 form its first user-usable dependency milestone.
2. **Wave 2:** D2/D3, R2/R3 and W1. Integrate once the migration and provider-capability contracts
   are settled. No worker may automatically acquire a broader trust/workspace boundary.
3. **Wave 3:** W2, then explicit integration/retirement W3. External usage import U2/U3 can proceed
   independently after U1 and verified memory-source event identity are available.

Relative sizes describe integration risk, not calendar promises. The principal cross-plan
dependencies are native approval/sandbox capability reporting, exact source event identity, and
the workspace/editor plan's conflict-safe file mutations. Central TUI files need a single owner.

Default decisions are conservative and implementable: retain worktrees; no automatic merge or
retirement; shared claims are advisory; unsupported enforced isolation is refused; native
reported cost only; unknown data stays unknown; legacy defaults remain unchanged. Public release,
signing identity, real-provider spend and destructive lifecycle policy are separate decisions.
The first implementation wave should not wait on those release decisions.
