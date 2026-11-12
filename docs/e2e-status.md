# Verification

The installed baseline is **2026-11-09g**, a private Windows build. The audited runtime source
and pre-rewrite documentation are preserved in local Git checkpoint `6fea4a9`. The current
working changes are documentation and product-definition work; the feature roadmap is paused.

Installed executable SHA-256:

```text
2ad5dfd44b31f1cab3e1f52555cc8a57a5bafdb7c8f87ac06ebf439f250ceef8
```

## Checks run for 09g

| Check | Covered behavior | Result |
| --- | --- | --- |
| 9 interaction checks | Ownership, cancellation, connection identity, reply limits and duplicate requests | Pass |
| 10 command checks | Customization routing, arguments and local command definitions | Pass |
| 10 completion checks | Discoverability, valid command branches, descriptions and selection | Pass |
| 2 setup process cases | Shared setup and skills across provider homes, later accounts, failures and source changes | Pass |
| Native request process case | One prompt/answer for identical retransmissions; no replay after native resolution | Pass |
| Windows terminal journey | Completion, skill install/list/remove, hook inventory/status and invalid arguments | Pass on the exact optimized candidate |
| Direct stock Codex smoke | Codex 0.166.0 app-server, gpt-5.6-luna, low effort, one OK reply | Pass; no tool grants |
| Installation | Installed executable matches the checked candidate hash | Pass |

Setup cases also cover linked-source updates, existing-name preservation, stale-source removal,
v1-to-v2 storage migration and unknown-version preservation. The terminal journey starts no
provider turn. The stock Codex smoke is separate from Concordia's process-fixture evidence.

Formatting, whitespace, module-cycle and supplemental lock checks passed. The lock scanner
checked zero comment locks; this does not establish native hook enforcement. Independent
process, architecture, UI and logic reviews passed for the 09g changes.

## Boundaries still unqualified

- Broader live approval, worker, input/output and lifecycle coverage across provider versions and
  Windows native/terminal hosts. A fixture or one live Codex reply is not universal parity.
- Native hook trust and a real denial test for each supported provider/home/project binding.
- Cursor current-window capacity, external per-event usage and omitted numeric credit balances.
- Memory superiority, token savings and retention advantage. Existing experiments are scoped.
- OS-enforced workspace confinement and public release/feed qualification, both owner-held.
- Read-only inspection of retained duplicate-chat display snapshots remains a paused feature.

## Cleanup closeout

There is one worktree and no Cargo target directory. Earlier obsolete worktrees, build trees,
archives and superseded source paths were removed. The 09g verification cache removal freed
1.8 GiB. Unused installer images were removed only after ownership and content checks.

The owner manually removed the older executable, matching receipt and four synthetic Temp
directories previously blocked by automatic approval review. Their absence was verified on
2026-11-09. Native histories, credentials, drafts, queues and unique recovery records remain.

## Documentation rewrite

Checks run for this rewrite slice: `harness docs check --plain`, `harness run check:docs --changed`,
`scripts/verify-oss-gates.ps1` and `git diff --check`, all passing. A changed-files-only check cannot
certify untouched pages. No runtime rebuild or new provider turn was run for this documentation
work; the entire rewrite and product interview remain in progress.

## Earlier evidence

Detailed build journals, audit scopes and historical receipts are preserved in Git. For example:

```text
git show 6fea4a9:docs/e2e-status.md
git show 6fea4a9:docs/quality/e2e-history.md
git show 6fea4a9:docs/agent/cleanup-review.md
```

The old journal's named sections remain intact: "Scoped native questions, cancellations and PDF
reading on 05c" contains the PDF/native-input receipts; "Source preview validation (`2026-10-21a`)"
contains the first preview check. Working-tree links now lead here for Git retrieval rather than
duplicating those receipts in current documentation.

Earlier results qualify their dated source and artifacts. They do not qualify later changes or
a public release. The documentation rewrite does not change that boundary.
