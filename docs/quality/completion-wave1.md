# Completion campaign: first implementation wave

Started 2026-11-02 from `eb9deb4`, the merge of the remaining-feature plans into the
memory/orchestration branch. This record distinguishes implementation, source verification,
installed verification and live-provider evidence. No real provider turns are authorized here.

## Ownership and scope

| Lane | Scope | State |
| --- | --- | --- |
| Orchestration D1 | Bounded dependency graph, read-only readiness, versioned recovery persistence | Implementing |
| Provider P0 | Owner-bound typed native interactions, tri-state capability evidence, current ACP conversion | Implementing |
| Workspace W1 | Bounded cancellable literal project search, source revision checking and draft-preserving UI | Implementing |
| Integration | Shared TUI/command wiring, documentation/build identity and checks | Implementing |

Three implementation agents use separate worktrees. Shared TUI dispatch, command registration,
build identity and final integration have one owner. Preserve native tools, authentication,
approval decisions and defaults. Preserve unrelated uncommitted signing work.

## Observable acceptance

- Dependency graph mutations are bounded and atomic; cycles, invalid/foreign IDs, missing
  prerequisites and duplicate nodes/edges fail visibly. Only an accepted successful run can
  satisfy its dependency. Restart loads nonterminal work paused and dispatches nothing.
- Native request types retain provider option IDs/scopes and one-response channels. An old,
  cancelled, wrong-run/attempt/session or unknown request cannot approve a new action. Capability
  uncertainty never broadens trust or silently changes transport.
- Project search has explicit workspace/query/case/scope and complete/incomplete/error state.
  Reads, matches, time and retained memory are bounded. Credential paths and escapes are excluded.
  Exact original-byte ranges and source hashes bind opening; stale results cannot open unrelated
  content. Search input and closing preserve chat drafts, queues and running-provider ownership.

D1 and P0 are foundations. Usable dependency scheduling still requires approved submission and
dispatch (D2); native worker approvals require interaction routing and a verified adapter.
Project replacement, worktrees, image rendering and later phases are outside this wave.

## Existing pull request stack

At the start of this wave, #21 was merged into #20's branch, not into the default branch.
The existing stack is #20 into #19, #19 into #17, and #17 into the default branch. #20's runtime
CI repair passed before the plan merge; merging the plans triggered a fresh run. Older #19/#17
failures predate that repair and receive it when the stack is integrated in that order.

#18 is a separate UX branch with a unique commit. Some behavior exists in the newer branch
(focus/resize redraw, VT word-delete normalization and independent draft scrolling); other
changes need reconciliation (selection copy hint, permission labels, landing return and native
notice filtering). It must not be discarded as an empty duplicate or merged blindly over the
newer TUI. Existing PRs remain for owner review; no remote merges are performed by this wave.

## Validation

Pending implementation integration. Record exact commands, results and limitations here before
marking any lane verified. Use existing fixtures and native checks; no new unit/integration test
files are implied. Installed and live-provider claims require their own evidence.

## Persistence and implementation limits

Worker ledger version 2 adds dependency metadata. Version 1 records migrate with task IDs equal to
existing run UUIDs and no edges. Version 2 graph nodes bind exact conversation/run/state receipts;
invalid graphs and unknown versions preserve original bytes. Older executables reject version 2
recovery. Only successful exact-run receipts satisfy prerequisites; retries do not retarget edges.
Interrupted work and metadata-only proposals load paused. No dependency submission command,
automatic scheduler or managed worktree execution is provided in D1.

Native interaction ownership reuses the delegate conversation UUID and main continuation run UUID;
each launch has a fresh attempt UUID. Canonical workspace/account-home paths and the live protocol
connection/session/request complete the binding. Capability fields default to unknown; native
negotiation and observed validated requests carry their own evidence. Current integration remains
Cursor ACP image turns. Codex/Claude text protocols and worker routing require later adapters.

Project search opens only a verified exact-byte snapshot, never a second unverified revision.
Existing open panes must be closed before opening a search snapshot. Scans stop at the first limit,
read/traversal failure or oversized text file and visibly report incomplete results. Non-UTF-8 and
binary files are counted and skipped. Case-insensitive search uses Unicode lowercase mapping;
regex, locale collation and replacement are outside W1. No background index or provider runs occur.
