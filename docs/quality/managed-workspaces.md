# Explicit managed Git worktrees

Candidate build `2026-11-02s` implements orchestration W2 on top of cooperative reservations.
Git worktrees organize separate working files; they are not filesystem sandboxes. Native
provider confinement remains unverified, and an enforced mode is unavailable. Existing native
trust and approval rules remain authoritative. Integration and retirement are separate W3 work.

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

## Persistence and bounds

`managed-workspaces.json` v1 is locked and atomically replaced, with a 1 MiB/64-record bound.
Creation intent is durable before invoking Git. Canonical owner/common-Git/execution directories,
native directory identities, UUID, base object ID, generated branch and binding revision establish
ownership. Unsupported/corrupt metadata remains intact and refuses dispatch. Registry capacity
does not trigger automatic pruning.

Workspace registry v2 and worker ledger v6 read their earlier supported versions. Binding v2
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
