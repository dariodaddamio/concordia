# Local worktree retirement

The three older Home, hidden-console and settings worktrees were retired after renewed cleanup
authorization. Fresh checks found clean source, no untracked work, ignored files confined to each
generated target, no linked target paths and no active executable or Cargo child under those roots.
Their feature work is already integrated through the Home hygiene (#134), hidden-process/shortcut
(#133), and settings/motion (#135) squash commits. Do not reapply those older implementations over
the later identity, storage and recovery repairs.

## Completed cleanup

Canonical Cargo cleanup used each exact sibling manifest and an explicit matching target directory.
It removed 29,412,145,663 logical bytes of duplicate generated builds. Git then removed all three
clean registered worktrees, for 29,436,757,242 logical bytes including their checked-out source.
Their three remaining obsolete local branches were deleted; the three older merged local branches
were already absent. Only main and the active working branch remain locally. Remote refs and all
current pending implementation were preserved. Physical disk reclamation can differ from file sizes.

Earlier raw recursive removal requests were rejected before execution. The renewed canonical Cargo
cleanup and Git retirement completed through normal tools without an override. The machine-local
receipt in `target/roadmap-cleanup.json` distinguishes those attempts from the successful cleanup.

## Preservation and remaining artifacts

The existing complete-history bundle was reverified before branch retirement. After retirement,
every bundled tip was freshly proved reachable through main ancestry or preserved origin refs;
`git rev-list` found no archived commit absent from current reachable Git objects. The redundant
4.45 MB bundle and its two duplicate inventory sidecars were then removed, followed by their empty
archive directory nonrecursively. No new backup was created. The original bundle SHA-256 and exact
reachability proof remain recorded in the machine-local cleanup receipt as historical provenance.

The installer had already pruned two older receipt-verified builds and their receipts (53.62 MB).
The remaining unreceipted legacy executable was independently identified as build 2026-11-04n,
matched to its expected SHA-256, checked for links and active dependencies, then removed (23.22 MB).
At this cleanup snapshot, installed build 2026-11-08h and its newest verified 08g rollback were kept.
The existing user process was proved to map the retained 08g rollback image and was not controlled
or closed. Those are point-in-time installation facts, not claims about later builds.

Four previously inventoried scratch benchmark binaries/symbols were already absent before this
retirement pass and were not deleted again. Current native/source probes, receipts, the baseline
patch and flow evidence remain through active qualification. All 16 screenshots total less than
0.84 MB and represent distinct Home, draft, question, queue and preview flows; no image was
demonstrated redundant. No credentials, native histories, drafts, queues or account state were removed.

## Current package cache inventory

Metadata-only inventory measured approximately 5.42 GB in the current debug tree and 2.18 GB in
release. Package fingerprints and integration-crate names attribute roughly 4.50 GB to Concordia
and its generated test/package outputs; this is an estimate rather than a Cargo dry-run result.
After final 09a installation and qualification, main-target inventory measured 15,433 files and
7,747,775,516 logical bytes, including generated agent probes/receipts. Full and package-specific
Cargo clean both refused the missing/invalid CACHEDIR.TAG. Direct recursive removal of the exact
validated, unlinked, inactive target was rejected by automatic approval review as blocked by policy.
No deletion occurred in the main target and no marker was fabricated to bypass the guard.
That was the 09a retirement snapshot. The owner subsequently deleted the remaining main target;
its absence was verified before 09b implementation. That also retired the ignored scratch probes,
screenshots and raw receipts, including `target/roadmap-cleanup.json`. The portable disposition
above remains the record. 09b qualification uses one new release tree with a valid Cargo cache tag.
