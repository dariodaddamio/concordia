# Cross-worktree dependency delivery

Build `2026-11-03n` extends the existing dependency graph, worker queue and managed workspace
lifecycle. `/delegate after ID[,ID] ACCOUNT TASK` can reference a managed prerequisite belonging
to the same conversation and owner workspace. An external/unmanaged cwd is refused. Successful
execution leaves the downstream job queued as **integration_required**; it reserves no running
worker or workspace while waiting for that decision. Failed/cancelled/uncertain prerequisites
retain their existing block reasons.

Open the queued worker detail and press **D**, or use `/delegate delivery JOB`, to inspect the
exact prerequisite runs/attempts, native source ids, account/home/session, managed registry
revisions, base/current heads and bounded Git status. Opening review preserves the composer.
The displayed heads are observed repository state, not proof that a worker authored each commit.
Review runs in one cancellable background job; `/delegate delivery cancel` interrupts it.

The preview provides two distinct receipt-bound acceptance commands:

- `/delegate delivery JOB result-only RECEIPT` accepts verified prerequisite result text. This
  transfer moves no files, performs no integration and makes no claim that edits exist in the
  downstream cwd. It can be selected deliberately even if an earlier integration already exists.
- `/delegate delivery JOB integrated RECEIPT` is available only when the managed lifecycle has
  a finished merge/cherry-pick receipt for the exact observed worker head and current clean owner
  tree. It verifies an already completed operation; it never executes Git integration. Later
  owner commits or dirty state conservatively require a fresh integration review or result-only
  acceptance. Task completion cannot authorize the Git operation.

Acceptance is bound to the queued downstream run, conversation, exact prerequisite attempts and
sources, canonical native home/session, managed directory/common-Git identities, registry
revision, base/heads/tree, Git status hashes and selected delivery mode. Source history remains
in the native reader; receipts contain metadata, not response bodies. Status excerpts contain
at most 4096 characters per owner/worktree. Result-only receipts do not bind the contents of
uncommitted files and never transfer those files.

The UI recomputes review evidence before accepting the exact receipt and rejects late results
after cancellation, conversation change or job/source identity change. The graph persists the
acceptance atomically under the normal worker lease; failed persistence restores the old graph.
Dispatch verifies the same native source snapshots and repository receipt again before provider
launch, then applies ordinary routing, quota, trust and workspace admission. Changed evidence
pauses rather than replaying or merging. Native approvals remain authoritative and prerequisite
text is labelled untrusted data. Receipt acceptance cannot create unapproved tasks or graph edges.

The existing eight-prerequisite/64-task/256-edge limits remain. Git inspection shares a 60-second
deadline and tracked cancellation across its commands. Native source reads retain their own
bounded reader contract. Worker ledger v8 reads supported v1–v7 data with no inferred acceptance;
nonterminal recovery stays paused and never replays a delivery or Git operation automatically.

Managed lifecycle previews now include dependent-run references. Pending/running/uncertain
dependants block retirement and branch deletion, while explicitly reviewed integration can
proceed to satisfy them. Completed/cancelled dependants retain provenance without automatically
preventing a later explicit clean retirement. Native process and dirty/unique-commit guards remain.

Evidence: the existing process fixture uses real temporary Git repositories and fake providers
to verify initial blocking, distinct delivery modes, changed/corrupt receipt rejection, result-only
delivery without a file transfer, an explicit native integration before integrated delivery,
pending-dependency retirement refusal, post-acceptance changes and paused recovery without launch.
The existing terminal worker journey verifies blocked review, wrong receipt rejection, cancellation
without a provider turn and the subsequent existing integration/retirement path. No real provider
turn, native permission bypass, remote Git change or automatic merge is used.
