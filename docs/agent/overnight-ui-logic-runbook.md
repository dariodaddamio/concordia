# Overnight UI and logic audit and repair

This is an execution brief for a future campaign, not evidence that one has started. The user
requested Claude Opus as coordinator, smart use of Luna/Cursor workers, Firstmate, recoverable
overnight execution and cleanup of the campaign's worktrees. Product rules remain in
[AGENTS.md](../../AGENTS.md), [PLAN.md](../../PLAN.md) and the
[shared orchestration procedure](crew-orchestration.md).

No prompt guarantees uninterrupted provider, network or machine availability. The objective is
useful, reviewable progress that survives interruptions, with an honest final disposition.

## Observed starting point

Preparation inspected source commit `1e16b8d`, build `2026-11-15a`. The checkout was clean.
Re-read HEAD/status at launch: do not replace newer work or start from an older default branch.
The recent model/effort and updater changes must remain in the campaign baseline.

- Harness reports version `0.10.0`. `harness worker status` selects native Codex collaboration
  in the current Codex host; it reports Firstmate inactive because `FM_HOME` is unset.
- No Firstmate executable or checkout was found in the checked PATH/common locations. This is
  unverified availability, not proof that Firstmate is absent everywhere. No Firstmate launch
  command or automatic supervisor capability has been established.
- Installed Claude Code reports `2.1.325`; Cursor Agent reports `2026.11.13-eacade1`.
  These identify the provider CLIs, not the Cursor editor/launcher. Use the actual `agent`
  executable for Cursor worker capabilities and record its resolved path/version/hash at launch.
- Cursor's read-only live catalog reports `composer-2.5`, `gpt-5.6-luna-medium`,
  `gpt-5.6-luna-high`, `claude-opus-5-5-medium` and `claude-opus-5-5-high`, among other models.
  Catalog presence is not a successful authenticated worker launch or a quota guarantee.
- Native Codex Luna and native Windows journeys passed in the previous qualification; that
  evidence does not prove a Firstmate-to-provider route in a different host.
- Five secondary worktrees pre-existed this brief. Inventory them at launch; they are not
  automatically owned by the new campaign. Historical cleanup statements in older verification
  records are not current inventory.

The [verification report](../e2e-status.md) records four unresolved UI expectations. Start by
reproducing and classifying them; do not assume either a product bug or a stale test:

1. `landing_chats_wrap_up_and_down`
2. `streamed_commentary_and_commands_keep_arrival_order_without_duplicate_final_reply`
3. `the_wait_names_what_the_model_is_doing_and_the_colour_follows_it`
4. `thinking_is_kept_dimmed_after_the_turn_and_never_sent_as_context`

## Coordinator and model routing

Opus owns decomposition, priority, disputed behavior, shared-state contracts, integration
decisions and final synthesis. It reads compact findings and selected diffs. Workers own searches,
reproductions and bounded repairs. This is a task-routing judgment, not a comparative coding
benchmark. The user's Opus choice overrides Captain's default coordinator model for this run only.
If the proven fallback policy selects a different lead, that explicitly recorded designated
coordinator assumes the same responsibilities. Escalations below go to the designated coordinator;
they must not wait indefinitely for an unavailable Opus session.

| Role | Initial route | Escalation trigger |
| --- | --- | --- |
| Coordinator | Available Opus, medium for routine dispatch; high for ambiguous decisions | Conflicting evidence, shared-state changes, risky recovery semantics |
| Investigator / bounded logic writer | Native Codex Luna medium, or reported Cursor Luna medium | Two failed repair attempts or a demonstrated reasoning gap |
| UI writer | Reported Composer 2.5 or Luna medium, after a successful workflow probe | Broken interaction proof or a repair crossing state owners |
| Independent reviewer | A different available model from the author when practical | Failed receipt, disputed behavior or cross-owner change |
| Qualification executor | One worker on the native Windows host | Infrastructure failure, changed candidate or missing UI access |

Use actual model IDs and supported effort controls from that provider. Cursor is a provider/tool,
not a model: record the selected model. If `auto` is necessary, record the resolved model when
reported and otherwise mark it unknown. Do not equate Cursor Auto with Luna or Opus. Prefer
explicit non-fast model IDs; do not enable a premium mode, API billing or extra credits.

Begin with three worker slots plus the coordinator, at most two simultaneous writers. Scale down
for resource pressure. Increase only after measured independence and available runtime capacity.
An audit worker cannot recursively create a swarm. Do not use additional Claude workers by
default while preserving the coordinator's five-hour subscription allowance.

Record the campaign's budget explicitly before dispatch: eight hours of wall time, three worker
slots, two writers, at most two unsuccessful repair attempts per finding, and a 45-minute task
checkpoint before renewing or splitting a packet. The runtime must enforce the final dispatch
and shutdown deadlines independently of the model. These bounds apply to worker/coordinator
inference; the ten tiny product-verification turns below are a separate cap. Retain existing
account spending limits. No numeric token/dollar budget was supplied: record available native
limits and unknowns instead of inventing an enforced monetary ceiling. Do not buy capacity.

If one model fails, inspect the failure before changing effort. A broken PATH, permission denial
or missing model is not a reasoning problem. Permit one evidence-based repair on the same route,
then one eligible alternative route. After two unsuccessful code repairs, require a coordinator
diagnosis rather than another blind retry. Re-review every fallback author's result.

## Before leaving the run unattended

Finish this gate while a person is available. Reserve about 20 minutes; do not spend the night
installing orchestration infrastructure.

1. Read local instructions and relevant UI/architecture/anti-slop skills. Establish a clean,
   isolated campaign branch from the actual current HEAD. Preserve unrelated dirty work; do not
   stash, reset or clean it automatically. Record repository identity, baseline commit, branch,
   original worktrees, tool versions, available disk and the native Windows execution host.
2. Discover the real Firstmate checkout, its instructions, supported provider adapters and native
   lifecycle tools. Verify `FM_HOME` and actual tool registration. Start from its supported host
   environment; do not set a marker just to make Harness say it is active. Harness reports and
   plans dispatch; its repository scripts do not start an agent runtime.
3. Prove the selected route: dispatch a tiny worker in a disposable scratch workspace, have it
   return a verifiable file/artifact, inspect its actual model/effort/session and reap it. Prove
   a different-provider review route too. A catalog query or `--version` alone is insufficient.
4. Prove cancellation and coordinator recovery using the installed runtime's normal lifecycle.
   Stop/restart a disposable coordinator, reconcile a checkpoint, and verify a completed task is
   not rerun. Check what happens when the lead exits while workers remain. Verify restart and
   fallback support rather than assuming Firstmate provides it.
5. Resolve native approvals for the exact workspace and required Cargo/Git/UI commands before
   unattended operation. A noninteractive tool rejection must be surfaced and classified. Do not
   use blanket permission bypass, `--force`, hook-trust edits, new credentials or global settings
   as a remedy. LOCKED regions use the existing checked-edit broker and human approval flow.
6. Verify native Windows GUI journeys in the same host/session that will stay available at night.
   A WSL/Linux coordinator needs a working, approved Windows execution bridge. Linux checks or
   browser-rendered SVGs cannot certify the Windows application. Confirm machine power/session
   stability without silently changing system power, update or account settings.
7. Establish an independent runtime owner for checkpoint recovery and owned-process cleanup. A
   prompt running inside an exhausted Opus session cannot restart itself. If Firstmate has no
   proven lifecycle recovery, disclose that limitation before unattended execution.
8. Prove that useful unmerged WIP can be preserved in a durable recovery branch or verified bundle
   while its disposable checkout is removed. Record the actual catalog and worker-route receipts
   in the run artifacts, including requested/resolved model and effort. A future fallback record
   must also name provider, account/home, route, budget eligibility and reason for switching.

If Firstmate fails this gate, do not improvise a shell-based agent scheduler. Use an already
working native Codex collaboration host only when it is actually available to the fallback lead.
Opus can supply the initial plan and review later. Claude's ordinary Agent tool is not a native
Codex collaboration interface. If neither route is proven, stop at a saved ready-to-run brief;
an unstarted overnight campaign must not be reported as running.

The existing local command forms are:

```powershell
harness version --json
harness worker status
claude --version
claude --help
agent --version
agent --help
agent --list-models
git rev-parse HEAD
git status --short
git worktree list --porcelain
```

Run Harness commands in the project root or pass its supported `--root` option. This installed
version does not support `--json` on `worker status`. Do not invent `fm` flags. Capture verified
Firstmate commands in the run's capability record after reading that installation's help.

## Work queue, ownership and audit coverage

Reuse Firstmate's durable task/attempt registry when present. Keep one authoritative queue. If
the supported host lacks such a registry, use one small run manifest under ignored
`target/overnight/<run-id>` with one coordinator writer; workers return separate result files.
Do not build a new orchestration subsystem. Keep artifacts outside disposable worktrees.

Every task needs an ID, severity, user symptom, baseline reproduction, owner/module, allowed paths,
dependencies, acceptance journey, expected artifact and disposition. An assignment additionally
records its base commit, worktree/branch, worker/provider/model/effort, attempt/session identity,
allowed actions and completion/recovery pointer. Exact provider/account/home bindings stay in
local metadata; never copy credentials or private histories into the repository.

Use states that separate assignment from proof: proposed, reproduced, assigned, running,
returned, independently verified, integrated, qualified, blocked or deferred. Record a specific
reason for blocked/deferred. An exit code, a new commit or a worker's "done" message is not proof.
Persist the next action after every return, integration, failure or ownership transfer.

Initial read-only lanes may run together:

| Lane | Audit focus | Proof |
| --- | --- | --- |
| Interaction and visual craft | Header/model/effort, Home, commands, F1/config/usage, update feedback, focus, pointer, resize | Actual key/mouse journeys and before/after UI artifacts |
| Conversation and event logic | Turn admission, preparation, cancellation, progress drain, queue, overlay/tab transitions | Controlled event order plus visible state and draft/queue retention |
| Persistence and boundaries | Account/model settings, session identity, recovery, metadata freshness, duplicate effects | Isolated restart/late-result journeys and exact identity evidence |

The coordinator turns findings into disjoint repair packets. One writer owns each path. Central
`app/state.rs`, `app/pickers.rs`, `input_dispatch.rs`, `draw/mod.rs` and shared test harness files
are overlap zones, not five independent write lanes. If two repairs need the same file, serialize
them or assign them to one owner; ask other workers for read-only advice. Do not create empty
abstractions or split modules solely to manufacture parallelism.
Before a repair is assigned, the ownership manifest must name exactly one writer and one reviewer
for each central file it touches. Reassignment closes the previous owner's write assignment first.

Prioritize data loss, wrong-account/session actions, duplicate sends/approvals, hangs and broken
cancel/recovery first; then blocked core interactions; then visual polish and warning debt. Every
finding names a real trigger and expected behavior. Static suspicion can be investigated but
must not become an unverified refactor.

Acceptance coverage must include:

- The four known failing expectations, with baseline versus candidate evidence. Correct a stale
  fixture only after the product contract and actual interaction establish why it is stale.
  Assign each an explicit product-defect, stale-expectation, environment/host or unqualified
  disposition before creating its repair packet; record remaining uncertainty for the latter.
- Model choice opening reported effort choices; Esc preserves the pair; Enter persists both;
  direct `/effort` values reject unsupported options without mutation; next launch retains choice.
- Header/control hand cursors, click targets, keyboard equivalents and focus return; no invisible
  actionable region, overlay click-through, accidental send or lost draft.
- Config edit/cancel/reopen; usage freshness and partial/unknown labels; stable cached display
  during refresh; Home/header/usage reading the same account-specific observation.
- Update stage/diagnostic separation, detail scrolling, cancel, failure, background readiness and
  blocked installation while work is active. Installation tests use isolated sandboxes.
- Start/queue/cancel during preparation, execution and completion drain; late results after
  model/account/tab changes; closing/reopening tabs; no automatic resend after uncertain effects.
- Recovery retains drafts, attachments, queue and exact native-session identity. Never use
  focused-pane heuristics or `resume --last`. Inspect duplicated display separately from replayed
  execution and immutable event/accounting duplication.
- Narrow 40x12, normal 90x28 and wide 150x45 layouts, no-color and resize. Test smaller supported
  sizes if baseline evidence identifies a boundary. Keep existing theme roles and settled behavior;
  no overnight visual redesign requiring a new product decision.

Each worker packet is small enough to complete/review in roughly 20-45 minutes. Scope is a
reviewable behavior repair, not "fix the whole UI". If a packet grows across owners, return its
evidence and split/reassign it rather than editing beyond the declared boundary.

## Verification and integration

One native Windows executor owns expensive Cargo compilation and the shared artifact directory.
Do not run concurrent worktree builds into one target folder. Workers can inspect/search in
parallel; the executor schedules scoped builds/tests for their actual tree. If separate caches are
necessary, name them in the worktree manifest and account for their disk/cleanup cost.

At campaign qualification, authorize one baseline enabled-suite inventory/run and one final
enabled-suite qualification; do not repeat full tests or all-target Clippy after every edit.
Inspect ignored/live cases before running them. Never use a blanket `--ignored` across providers.
Use focused existing checks for each reproduced defect. New tests must target the specific
acceptance cases above and add protection absent from existing journeys, not mirror code.

The existing source gates are:

```powershell
cargo fmt --all -- --check
cargo check --locked -p concordia
harness run check:docs --changed
powershell -NoProfile -File scripts/verify-oss-gates.ps1
powershell -NoProfile -File scripts/check-hidden-process-spawns.ps1
powershell -NoProfile -File scripts/check-module-cycles.ps1
```

For the explicitly qualified baseline/final campaign, inspect test inventory first, then use the
project's enabled Cargo suite and all-target Clippy. Keep live opt-in variables disabled for the
regular suite. Existing warning debt is classified; do not blanket-suppress warnings or weaken a
gate to obtain green output. Every failing baseline check gets a disposition, and every new
candidate failure is investigated. Do not silently accept a red gate as "pre-existing".

```powershell
cargo test --locked -p concordia -- --list
cargo test --locked -p concordia -- --test-threads=1
cargo clippy --locked -p concordia --all-targets
```

The existing native journey entry point for model/effort is:

```powershell
cargo test --locked -p concordia --test tui_journeys usage_profiles_and_account_model_completion_work_in_the_real_terminal -- --exact --ignored --test-threads=1
```

Other ignored journeys are selected by exact name after inspecting their behavior and isolation.
For provider-facing changes, reserve at most ten tiny live Concordia verification turns across
the whole campaign under AGENTS.md, not ten per worker or retry. Count them centrally. Preserve
Claude allowance for coordination; do not probe its quota with model requests. Grok/opencode or
any uninstalled/unavailable provider stays explicitly unqualified. Worker coding inference is
tracked separately from product verification turns; neither category is unlimited.

Every completed repair returns changed paths, commit, root cause, visible behavior change, exact
checks/exit codes, artifact paths, remaining failures, process/session disposition and cleanup
readiness. UI proof distinguishes actual native-window results from synthetic rendered buffers.
The reviewer inspects artifacts as well as code; a screenshot cannot prove persistence, and a
unit test cannot establish visual polish.
For every repaired journey, open and inspect at least one actual native-window artifact, with
dimensions and executable/build identity recorded. A path in a worker summary is not that review.

Integrate only independently verified, dependency-ready results, serially, into the campaign
branch. Reconcile overlapping changes before integration. Keep failed experiments outside the
qualified candidate. Prefer ancestry-preserving integration when appropriate; otherwise retain
source branches until patch equivalence and preservation are verified. Do not auto-merge a
worker's uncertain result or interpret its summary as permission for another native action.

Use the independent process steward, architecture, craft/UI and logic/tests seats from the
shared procedure. No author grades its own change. A failing seat reopens the affected task.
The designated coordinator settles conflicting evidence, not reviewer popularity. Do not force a speculative redesign
through overnight by repeatedly asking reviewers until one approves it.

The integrator owns build-stamp/changelog/canonical-doc updates. Freeze the candidate before
final qualification. Bind receipts to the tested commit/source tree and, for binary journeys, the
actual executable path/hash/build stamp. The installed application is a separate artifact.
Use the existing candidate collector after reading its contract; it records evidence, not a
fresh test execution. Do not substitute a stale executable or a docs-only HEAD for the tested
candidate identity.

## Recovery, quota and bounded progress

Checkpoint on completed tasks and before context rotation. During long work, use supported native
runtime status/events and a periodic checkpoint, roughly every ten minutes, without model calls
just to say "still working". Store bounded logs, current action, worker/attempt, worktree/head,
artifacts and next safe action. A worker receives pointers, not the entire campaign transcript.

On coordinator restart: take the runtime's single coordinator ownership, read the authoritative
queue and recent receipts, list actual live workers, inspect task worktrees and reconcile returned
results before dispatch. A missing heartbeat means inspect; it does not mean rerun. If live
workers cannot be identified, do not create replacements that may write the same files.
Use the runtime's persisted lease/generation or equivalent native ownership mechanism to fence
the old coordinator from subsequent writes. If this cannot be proven, a replacement cannot take
write ownership while the previous coordinator may still be alive. Do not implement a second
lease system in the project to manufacture that proof.

| Failure | Response |
| --- | --- |
| Claude quota / Opus unavailable | Persist decisions and handoff; use only an already-proven eligible coordinator route |
| Auth / native permission denial | Block that route, retain work, continue independent authorized tasks; no credential or trust changes |
| Transient provider transport error | Inspect actual effects/session first; resume the exact safe attempt or create a recorded new attempt |
| No progress for about 15 minutes | Inspect native status, owned processes, build logs and Git changes before one nudge/cancel decision |
| Repeated repair failure | Stop retries, give Opus the reproduction and contrary evidence; split, escalate once or defer |
| Build pressure / Cargo lock / low disk | Serialize compiler work, reduce concurrency and remove only owned disposable artifacts |
| Uncertain tool effects or worker death | Retain evidence and reconcile; never replay an uncertain tool action or approval |
| Supervisor/host unavailable | Record the interruption and exact resume action; never report an unattended run as healthy |

These are operational intervention thresholds, not new arbitrary timeouts in production code.
A legitimate long compile is not killed solely because no source file changed. Checkpoint quota
reset information only when the provider reports it; do not invent a five-hour restart time.
An approved Cursor route to an available Opus model is a different provider route and budget,
not proof of unlimited Claude capacity. Record the switch and do not create accounts, enable
extra spending or change credentials to continue.

## Eight-hour allocation and stop conditions

| Time from launch | Work |
| --- | --- |
| 0:00-0:20 | Prove runtime/approvals/recovery, record inventory and capability matrix |
| 0:20-1:15 | Baseline checks and concurrent read-only audits; deduplicate/prioritize findings |
| 1:15-5:45 | Small repair/review waves; integrate qualified independent fixes continuously |
| 5:45-6:45 | Stop new scope; freeze candidate; integrated native/UI/source qualification |
| 6:45-8:00 | Resolve qualification blockers within scope; board, cleanup and morning report |

Use an actual start/deadline in local time and UTC, not an assumed overnight start. Reserve at
least the final hour for preservation and cleanup. Stop dispatching when no task can finish with
time to verify and clean it. Finish early if acceptance is satisfied; do not invent work to fill
the night. If a core blocker remains, deliver a partial, resumable result instead of claiming done.

## Worktree cleanup is a deliverable

Before creating a worktree, record its exact absolute path, repository/common Git identity, base,
branch, owner and run ID in the campaign manifest. Capture the original worktree inventory first.
Use explicit base commits: provider-created worktrees may otherwise start from a default branch.
Do not rely on automatic worktree cleanup after a writer has changed files.

For every worktree created by this campaign:

1. Stop/reap the exact owned worker processes; verify no process, pending integration or managed
   dependency/reservation still uses it. Reconcile uncertain Git/native operations first.
2. Inspect HEAD, branch, tracked/untracked files, ignored build artifacts and merge/rebase state.
   Never clean a path merely because its name contains `audit`, `tmp` or the current date.
3. Preserve useful unintegrated work in a named local recovery branch/commit and an inspected
   result receipt. If required, create and verify a Git bundle outside the worktree. Preserve
   owned untracked source files too. A WIP commit is labelled unqualified and is not integrated.
4. Verify the retained commit exists after removal planning and can be recovered independently
   of the checkout. An unmerged commit is not deleted along with its only reference. If work
   cannot be preserved or its ownership is uncertain, record a cleanup blocker; do not force it.
5. Use the runtime's normal retirement flow for managed trees, respecting current receipts and
   dependent references. For plain Git trees, verify resolved paths stay inside the campaign's
   declared worktree parent, then use `git worktree remove <exact-path>` without force.
6. Remove fully integrated throwaway branches with normal checked Git deletion only. Retain and
   list any recovery branch. Do not use `git branch -D`, `git clean -fdx`, recursive deletion of
   guessed paths, or global worktree pruning to hide failures.
7. Re-list worktrees and compare to the original inventory. The campaign-created worktree delta
   must be zero for cleanup to pass. Pre-existing trees remain intact unless separately reviewed
   and authorized. Keep logs/artifacts and recovery refs outside disposable trees.

This permits checkout cleanup while preserving failed/unmerged work in durable Git references;
it does not authorize deleting that work. If safe cleanup is impossible, report the exact retained
path/reason and mark cleanup blocked. Never claim zero leftovers without the final inventory.

## Morning handoff

Produce one concise report linking the durable run record and artifacts:

- Exact tested commit, source build stamp, candidate executable/hash and installed-build status.
- User-visible before/after results, not just edited files or a count of agents.
- All findings with severity and fixed, disproven, deferred or blocked disposition.
- Checks actually run, baseline/candidate failures, native versus synthetic proof and coverage gaps.
- Model/provider/effort routes used, fallbacks, known usage and unknowns; no invented cost totals.
- Independent board verdicts, candidate branch and a resume instruction for unfinished work.
- Before/after worktree inventories, zero campaign leftovers or explicit cleanup blockers,
  retained recovery refs and confirmation that owned worker processes ended.

The campaign prepares local reviewable commits. Do not publish, push/merge protected branches,
install the candidate over the user's app or change global authentication. A deployment/install
approval, if subsequently requested, is based on the concrete qualified candidate.

## External capability references

Anthropic's [subagent documentation](https://code.claude.com/docs/en/sub-agents) describes explicit
model/effort selection and inheritance. Native Claude subagent configuration is not a generic
cross-provider model router. Check the installed tool schema before relying on a field.

Anthropic documents [agent-team recovery limitations](https://code.claude.com/docs/en/agent-teams#limitations).
Do not make experimental in-memory team state the only overnight recovery record.

The [programmatic execution documentation](https://code.claude.com/docs/en/headless) describes
print-mode lifecycle and background-work behavior. A successful top-level exit must not be
confused with independently verified worker completion. Avoid bare mode when relying on normal
project instruction, skill or tool discovery.

Cursor documents [headless approval behavior](https://cursor.com/docs/cli/headless): actions needing
approval can be rejected in print mode. Prove the actual required workflow before bed rather
than adding blanket bypass flags when it stalls.

Anthropic's [long-running harness work](https://www.anthropic.com/research/effective-harnesses-for-long-running-agents)
supports incremental tasks, durable progress and end-to-end verification; it does not guarantee
that this installation's supervisor, providers or desktop will remain available.
