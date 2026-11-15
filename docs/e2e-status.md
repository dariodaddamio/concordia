# Verification

The current source candidate is **2026-11-15a**, based on audit repair `3239abe`. This qualification
does not install or publish it. Local runtime logs confirm the earlier **14p** installation succeeded.

## Model, effort and update feedback qualification

| Check | Covered behavior | Result |
| --- | --- | --- |
| Native Windows journey | Model opens effort; Esc preserves the pair; keyboard commit; `/effort` and direct value; invalid value preservation; persisted selection; header hand cursor | Pass |
| 54 update-related library checks | Worker progress, structured diagnostics, restart/failure, Escape and close-button cancellation, preview controls and background readiness | Pass |
| Existing update presentation check | Progress/details/failure at 40×12, 90×28 and 150×45; no-color; scroll to diagnostics; long-model effort visibility | Pass |
| 5 model-picker and 11 command checks | Reported catalogs, filtering, command parsing and README registry synchronization | Pass |
| 19 release integration checks | Real loopback HTTP/PowerShell, hashing, verified installation into isolated sandboxes and failure preservation | Pass |
| Native Codex app-server | One tiny `gpt-5.6-luna` low-effort turn; reply `OK` | Pass |

Compiler warnings no longer replace progress. Cargo reports existing unused-variable, unnecessary
`mut` and dead-code warnings; successful compilation is not a warnings-clean claim. Diagnostics
remain bounded/redacted, and failed Cargo JSON is projected as readable errors. The existing
process cancellation check also cancels an actual PowerShell HTTP request against a delayed
loopback peer. Release checks, downloads and hashing use that process owner.

Observable artifacts are generated under `target/preview/model-effort-update`: native-window
text snapshots for model/effort and rendered-buffer SVGs for update progress/details. The SVGs
use synthetic job state; they are not screenshots of a real installation. The screenshot helper
now preserves spaces explicitly in SVG text so browser rendering retains terminal cell geometry.

The broader 28-case `tui::tests::run` group exposed four unrelated unresolved expectations:
landing arrow navigation, streamed command expansion, wait-card expansion and thinking-card
expansion. Their owning behavior is unchanged by this slice; no baseline execution or full-suite
pass is claimed. Two update-specific stale fixtures were corrected and the update checks pass.
No Claude inference ran, no credentials changed, and no real installation was performed.
Formatting, Cargo check, documentation, OSS privacy, hidden-process and static cycle gates pass.

## Audit repair qualification

| Check | Covered behavior | Result |
| --- | --- | --- |
| 125 focused existing library tests | Queue, continuation, attention, models, history, provider events, MCP/storage, redaction and process constructors | Pass |
| Bounded MCP process probes | Disabled/project-off enforcement, traversal rejection, oversized-frame resynchronization, initialized notification and matching IDs | Pass |
| Quiet and streaming MCP peers | Quiet child returns after about 10 seconds and is reaped; lowercase session headers and matching chunked SSE succeed | Pass |
| Fresh library probes | Config preservation, exact completion, owned proposals, secret-free projections, four concurrent mutations and consent/cap admission | Pass |
| 2 provider process tests | Closed-start receiver cancellation; duplicate native requests and answer replay | Pass |
| Windows native-window journey | Question, approval, notification and idle restart with synthetic native history | Pass |
| Native Codex standard turn | `gpt-5.6-luna`, low effort, one-line prompt, read-only trust, reply `OK` | Pass |
| Native Codex app-server turn | Same model/effort/prompt; accepted native `effort` field, reply `OK` | Pass |
| Independent architecture, UI/logic and security review | Canonical seams, transition fixes, exact historical source-reference gate and boundary probes | Pass |

Counts: queue 44, continuation 17, approval 1, model picker 5, history 10, provider failure/interaction/Claude/replay 14,
registry/record/schema 9, config/redaction 22 and process constructors 3.

The empty `provider::codex::tests` and `delegate::tests` filters are excluded from the 125 count.
The history failure was an invalid fixture: queue inspection stayed focused and later fixture
run IDs were not UUIDs. The corrected journey dispatches ordinary prompts with actual run IDs.
Other stale picker fixtures now use provider-reported models and usage and select by explicit IDs.
No new unit or integration test cases were added.

Formatting, `cargo check --locked -p concordia`, documentation, OSS privacy, hidden-process and
static cycle gates pass. The final independent process review is recorded in the
[archived repair plan](plans/archived/2026-11-15-audit-remediation.md).
The privacy check recognizes two existing public identifiers in exact files; it still scans personal
paths and every other private token. The native-window journey required fixing the initial snapshot
status and supplying the fake provider's missing native history; runtime recovery protection remained intact.
The module-cycle check discovers nested/inline modules and source use trees, then checks exact
historical ownership edges. It is a static approximation: macros, feature evaluation and compiler
name resolution are outside its scope. Path attributes must be adjacent to their module declaration.

No Claude inference ran while its quota was exhausted. Claude effort delivery was inspected and
fixture-checked; a fresh live Claude turn remains unqualified. Cursor/Grok/opencode, credential-backed
MCP delivery and TLS certificate/server combinations did not receive new live qualification.
The HTTP/SSE mock uses loopback HTTP; Windows HTTP and HTTPS share the WinHTTP request owner.
These checks establish the named flows, not universal provider parity or native hook enforcement.

## Historical installed-build record

Build **2026-11-09g** was a private Windows build. Its audited runtime source and pre-rewrite
documentation are preserved in local Git checkpoint `6fea4a9`.

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
