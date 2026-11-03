# Explicit managed Git worktrees

Build `2026-11-02s` implements orchestration W2 on top of cooperative reservations;
candidate `2026-11-03f` adds reviewed W3 integration and retirement.
Git worktrees organize separate working files; they are not filesystem sandboxes. Native
provider confinement remains unverified, and an enforced mode is unavailable. Existing native
trust and approval rules remain authoritative.

## Controls and binding

`/worktree create BASE` creates a new owned branch and worktree from one explicit commit/ref.
Select the repository worktree root first. The source must be clean, including untracked files;
uncommitted changes are neither copied nor stashed. There is no implicit enrollment of existing
worktrees. Missing Git, non-Git roots, unsupported paths, collisions and uncertain creation fail
visibly without reset or cleanup.

`/worktree list` lists up to 64 retained records for this owner. `/worktree inspect ID` shows its
phase, owner/execution folders, repository identity, approved base, branch and current binding.
One background operation per app keeps Git off the input/render thread. `/worktree cancel`
interrupts the owned Git process tree; changing conversation invalidates publication and requests
cancellation. Closing the app also requests cancellation. A record remains inspectable after an
uncertain interruption; startup never reissues Git creation.

`/worktree reconcile ID` performs explicit read-only Git inspection of an uncertain creation.
It can mark ready only the exact recorded repository/branch/base and clean execution folder.
A missing, changed or incomplete worktree stays retained for inspection. Reconciliation never
recreates, repairs or deletes work. A changed binding revision invalidates old task approval.

`/delegate worktree ID ACCOUNT TASK` approves a worker in that exact managed execution folder.
The ordinary worker queue, routing, trust and attempt limits still apply. Admission binds owner
and execution folders separately and verifies the managed record before launching. Git repository,
branch, base ancestry and directory identities are checked again off-thread before a native turn.
Cancellation during that preparation prevents provider invocation. Old nonterminal records restore
paused, with workspace reservations retained until stopped-process reconciliation when needed.
An explicit reconciled retry captures the original declared paths and managed execution folder
for its new run; it does not silently fall back to shared-folder execution.

Worker returns identify the managed execution folder and label edits as unintegrated result data.
The main agent keeps its original cwd. Cross-worktree dependency dispatch remains refused until
explicit integration/result-only receipts exist; successful execution does not imply that edits
are present in a downstream folder. Neither completion nor automatic result return merges work.

## Reviewed integration and retirement

`/worktree review ID ACTION` shows base/head, dirty state (including ignored artifacts), changed
paths, conflicts, unique commits, workspace reservations and worker/result references. Actions are
`integrate`, `pick`, `retire` and `delete-branch`. Each review returns a receipt for exactly that
action and snapshot. `/worktree ACTION ID RECEIPT` is separate explicit execution; changed heads,
metadata, references or reservations require another review. Completion never authorizes it.

`integrate` uses native Git merge; `pick` applies only the reviewed non-merge worker HEAD commit.
Native `merge-tree` supplies the preview, and the result must match its tree. Custom merge drivers,
unsupported tree entries, existing Git operations and conflicts retain the workspace for native
inspection. Git hooks, filters, recursive submodules and automatic maintenance do not run. No
stash, reset, forced update or inferred identity is supplied. Native Git still owns merge behavior.

Retirement requires an owned, clean execution folder with no unique commits, active/uncertain
reservation or unfinished worker reference. Ignored worker artifacts prevent deletion. Ignored
owner artifacts remain untouched. Worktree removal never deletes its branch; branch deletion has
its own review and checks the retired branch head and other worktrees. Historical records/results
remain. The bounded 64-record store never prunes itself.

Execution revalidates under the managed-record and workspace scheduler locks and persists whole-folder
reservation runs before spawning Git. Interrupted or unconfirmed exits retain those claims.
Explicit `/worktree stopped ID` acknowledges the process only after the user inspects it; it refuses a live
Concordia operation lock and clears only that operation's exact claims. The record shows their IDs.
Interrupted actions remain retained with their exact operation/head receipt. Explicit reconciliation inspects current
native state and never reissues the action. Lifecycle states are separate from worker execution.
External Git/processes remain outside cooperative scheduler enforcement; this is not confinement.

## Persistence and bounds

`managed-workspaces.json` v2 reads v1 and is locked and atomically replaced, with a 1 MiB/64-record bound.
Creation intent is durable before invoking Git. Canonical owner/common-Git/execution directories,
native directory identities, UUID, base object ID, generated branch and binding revision establish
ownership. Unsupported/corrupt metadata remains intact and refuses dispatch. Registry capacity
does not trigger automatic pruning.

Workspace registry v2 and worker ledger v7 read their earlier supported versions. Binding v2
adds explicit managed provenance; ordinary shared-folder bindings retain v1. A legacy reader
cannot treat a managed folder as an ordinary owner cwd. Unfinished main requests retain their
existing shared-folder schema because this slice changes worker execution only.

Git uses argument arrays, no shell, a resolved native executable on Windows, tracked cancellation,
a 60-second operation deadline and at most 64 KiB per output stream. Oversized/ambiguous metadata
refuses creation. Base-tree inspection rejects sensitive paths, symlinks and submodules before
checkout. Git hooks, fsmonitor execution, checkout filters and recursive submodules are disabled
for these commands without rewriting source configuration. Up to 32 named filter configurations
are handled; unsupported identities refuse creation. Very large base-tree listings and verbatim
network paths remain unsupported. Ordinary Git line-ending conversion still follows repository
configuration; committed content is checked out through Git, not copied from dirty source files.

## Evidence

The W3 candidate extends the same real-Git fixture for active/queued ownership, dirty and stale
reviews, HEAD cherry-pick, diverged merge, conflict previews, ignored-artifact preservation,
separate retirement/branch deletion, durable lifecycle claims and explicit crash reconciliation.
The preceding lifecycle fixture passes in 62.04 seconds; final candidate gates include the added
stopped-process control and terminal review/execute path. No real provider turn is requested.

Existing synthetic fixtures exercise real local Git creation, source-configuration preservation,
worker scope binding, uncertain creation reconciliation without a second worktree, stale binding
revisions, dirty-source refusal and cancellation before creation. The Windows DOS-path adaptation
was checked against the installed Git executable; canonical metadata retains native identities.
Production source `656d905` passes all 983 enabled checks, all-target Clippy with warnings denied,
release smoke, portable everyday smoke and OSS gates (325 tracked text files). All 48 installed
ConPTY journeys pass in 305.84 seconds, including actual managed worker cwd, original main cwd,
result-only provenance and draft preservation. The affected journey also passes alone in 14.60
seconds. GitHub CI is green on that source; later evidence documentation changes no production code.

Release and portable-installed build `2026-11-02s`: 18,499,584 bytes; SHA-256
`95DD3A392587C1E363AFC029B309239A5430EDA89758423D686ADEC71E12EFBF`.
No real model turn, remote Git operation, integration or retirement is part of these fixtures.

Interface references: [Git worktree](https://git-scm.com/docs/git-worktree) and
[Git configuration](https://git-scm.com/docs/git-config). Documented interfaces alone do not prove
native provider confinement or process ownership for later retirement.
