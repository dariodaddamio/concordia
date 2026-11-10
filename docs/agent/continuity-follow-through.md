# Continuity and workflow follow-through â€” 2026-11-08h

The owner approved the six-step roadmap as one effort, followed by product direction discussion.
The immediate acceptance criterion is reopening prior tabs and conversations instead of an empty
Home/recovery screen. The earlier whole-repository audit and its implemented repairs remain
recorded in [cleanup review](cleanup-review.md). This is its follow-through, not a second claim
that every provider/host path is qualified.

## Implemented

Window layouts now persist open-tab identity/order, selected tab, exact pane recovery references,
provider/account/home/model bindings, scroll position and bounded conversation snapshots. Each
workspace window holds an OS-owned layout lock; each tab retains its own recovery lease.
Concurrent windows cannot claim each other's layout or draft. Normal close writes the latest
window even if its content is unchanged, so reopening selects the last closed available window.
Update restart drops old window/pane owners before waiting for its replacement.

Unfinished requests stay paused with their conversation visible. A newer validated checkpoint's
source bindings and explicit conversation UUID take precedence over the periodic window snapshot.
Older checkpoints without a UUID use the validated saved window identity. A cached view from a
different, older conversation is discarded and its exact native source is refreshed instead.
No provider process, native approval grant, tool request or automatic queued dispatch is restored.
Independent drafts, pending queue edits, attachments and file recovery use their existing owners.

Saved display content is bounded and omissions remain visible across repeated restarts. If an
account is unavailable, its saved view/draft remain readable and its native session is not rebound
to another account; its footer cannot fall back to another selected account's usage. Native-history
and snapshot omission notices share one marker so a partial view stays labelled after restart.
Corrupt or unsupported layouts remain unchanged with an explicit warning.
Pre-layout builds never recorded open-tab order; existing interrupted source bindings can restore
their history, while multiple older orphaned records remain explicit `/recover` choices.
See [chat behavior](../chat-rendering.md) for bounds and storage contracts.

Native discovery now reads larger first metadata headers through one lightweight typed Codex
summary path, preserving workspace/child metadata. Exact reads reject contradictory explicit
session IDs in the bounded identity header. Bounded history tails preserve a complete first record
at a line boundary and skip only actual partial records. Managed-workspace recovery-reference
enumeration has a finite entry budget and reports unknown coverage rather than allowing retirement.

## Verification disposition

Machine-local receipts are retained under `target/roadmap-*`; they are ignored build evidence,
not publication content. Final executable/install status is in [verification status](../e2e-status.md).

Final 08h is installed locally and matches the optimized executable's SHA-256:
`e84c49aabf663bdb6a705ec8576f46f0b54d7b0ac755bc7525d5fd8a5175a828`.
The exact final binary passes both checkpoint variants of the native close/reopen probe,
including a visible warning for the newest corrupt layout without silently substituting an
older Home window. The source/history/draft/rename flow also passes at 0.14 seconds for exact
older-session history repaint. There were 105 focused checks followed by 13 final restore/usage
checks (overlapping executions), plus 34 integration cases. All-target compilation and the
format, whitespace, OSS/privacy, hidden-spawn and module-cycle gates pass. Dependency lock and
policy are unchanged from the preceding passing dependency check; its maintenance exception remains.

| Work | Observable evidence | Limit |
| --- | --- | --- |
| Tab restart | Native window, actual mouse selection/draft edit, repeated close/reopen, three exact tab IDs/order/active/drafts/queues, paused checkpoint, newer sources and sticky omission; concurrent independent window and corrupt-layout preservation | Synthetic state and owned windows; release-specific receipts identify the checked executable |
| Native history | 100/100 large-header workspace fields retained instead of zero; conflicting IDs rejected; all 800 messages reachable over seven pages; older-page search resolves; source hashes unchanged and indices body-free | Debug and release timings are labelled separately; no cross-profile speedup claim |
| Tail boundaries | Exact 8 MiB tail retains its first complete message; mid-record tail skips the fragment and retains the next message; source bytes unchanged | Controlled synthetic sources |
| Provider/worker transports | Existing selected integration batch: 34 passing cases covering dispatch, cancellation, streams, idle handling, images/native brokers and process ownership | Fixtures do not prove every installed CLI/model contract |
| Worker lifecycle | Manual public-API probe: exact question/permission ownership, one-shot responses, cancellation/stale answer rejection, restart without replay, clean managed retirement, retained branch/registry and stale receipt rejection | Scratch repositories and synthetic providers |
| Live Codex | Tiny low-effort Luna reply and native app-server file edit with the expected line diff pass | Live question prompt returned without a broker question; popup flow remains unqualified |
| Live Cursor | Auto reply and two-turn exact native continuation pass | Live question prompt reported no ask-question tool; broker popup remains unqualified |
| Live Claude | Standard transport returned an explicit weekly-limit response; native-control preflight rejected installed 2.1.319 against its verified 2.1.318 contract | No successful Claude model response or native popup qualification; no quota retries after diagnosis or global CLI/config changes |

The live budget used nine model-turn attempts, including two rejected Claude quota attempts,
and one native-control preflight rejected before model dispatch. No more live calls are needed
for this pass. Question tests remain failed qualification evidence; their assertions were not
weakened to turn unavailable interactions into passes.

The isolated concurrent history probe did not reproduce the historical five-second timeout.
Early broad-profile timings were contaminated by inherited home overrides and are explicitly
discarded in the receipts. This does not establish a root cause for the original latency report.
Initial very large transcript wrapping and above-budget fingerprint work remain linear;
unfinished/unsaved/unavailable-source recovery stays protected.

## Cleanup disposition (updated 2026-11-09a)

The earlier audit removed demonstrated dead/duplicate code and the obsolete pane-guessing launcher.
The 08h pass removed an unused history-load field and reconciled the cancelled desktop transition.
The owner then postponed step 7 and renewed cleanup; the earlier blocked-removal status is superseded.
Canonical Cargo cleanup retired the three integrated worktrees' 29.41 GB of duplicate generated
builds, followed by their clean registered worktrees and remaining obsolete local branches.

The redundant bundle and duplicate sidecars were removed after every archived commit was proved
reachable through existing main/origin refs. The inactive legacy 04n executable was independently
identified and removed. This retirement pass totals 29.46 GB of logical file sizes; physical disk
reclamation can differ. Earlier merged-ref, scratch-benchmark and installer cleanup are separately
attributed in `target/roadmap-cleanup.json` and [worktree retirement](worktree-retirement.md).

Current installation and its live rollback, current qualification artifacts, distinct-flow
screenshots, native histories, credentials, drafts, queues and unique work remain preserved.
User-data preservation does not retain obsolete compatibility or execution fallbacks. The 09a
source-pruning scope and pending final qualification are in [cleanup review](cleanup-review.md).
The successful 08h checks above remain dated 08h evidence.

## Next discussion

The owner has postponed step 7 (product direction). It remains a later discussion about the
daily chat/source-continuity workflow, workspace editing and delegation; current work is cleanup
and its final 09a verification.
No new provider, hosted service, automatic compaction policy, support promise or publication was
approved through this follow-through. The working changes remain local and uncommitted.
