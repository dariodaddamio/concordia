# Explicit related-chat groups

Build `2026-11-03d` adds M2 group metadata and a read-only related-work screen. Groups
are user-confirmed associations, independent of project folder grouping and observed handoff links.
No discovery result creates membership automatically. No provider model turn names a group.

## Controls

`/memory group-create SOURCE` deliberately creates a UUID group with the selected verified source
as its first member. Its name uses an existing deterministic session title or the account/session
fallback. Two groups may contain the same source. `/memory group-link GROUP SOURCE` confirms
another exact source; `/memory group-unlink GROUP SOURCE` detaches that membership alone.
Duplicate attach/detach is idempotent. Detach keeps provenance, goals and native logs intact.

`/memory groups [OFFSET REVISION]` shows confirmed groups in the current main-repository context.
F6 opens this screen from the landing page, retaining its selection, project and draft.
`/memory group-members GROUP [OFFSET REVISION]` checks up to eight members and retains missing
or stale entries with an unavailable reason. Cursor replay requires explicit source readback;
group discovery never starts its native history client. Exact account/home/session identities
prevent equal native session IDs in different homes from replacing each other.

`/memory related-file PATH QUERY`, `related-goal GOAL QUERY`, `related-at RFC3339 QUERY`,
`related-source SOURCE QUERY` and `related-from SOURCE QUERY` open ranked suggestions separately.
They reuse M1 reason/excerpt validation and budgets. Related-from allows cross-project context
suggestions and labels that scope; inspect each recorded workspace. These are direct verified
context neighbors, not recursive ancestry or inferred confirmed groups, so cycles cannot recurse.

The view scrolls with arrows/Page Up/Page Down and closes with Esc or X. Native approvals remain
owned by their run. Typing/pasting into this screen cannot dispatch a model turn or alter the chat
draft. Closing it or changing conversation invalidates late asynchronous publication. Follow a
displayed pagination command with both offset and revision; changed metadata requires restarting.
CLI exposes the same group commands through `concordia chat`, with JSON responses.

## Storage and limits

Groups and membership history live in `relations.json` v2 under the existing relationship lock
and atomic writer. Earlier v1 goal links load unchanged; group mutations migrate to v2 without
rewriting `goals.json`. Goal changes preserve the new fields. Unknown versions/corruption refuse
mutation and retain the original. Older readers reject v2 rather than silently dropping membership.

The shared store stays within 2 MiB, with 128 retained groups and 4096 membership events in
addition to existing goal limits. There is no automatic pruning, inferred detach or source deletion.
Pages contain up to eight rows and 28,000 formatted JSON bytes. Metadata revisions bind cursors;
native availability is rechecked per displayed member rather than frozen by the metadata revision.
The main-repository/worktree identity uses the same bounded Git-pointer resolver as landing groups.

## Evidence

Existing source fixtures cover v1 migration, equal session IDs across homes, overlapping groups,
idempotent membership, detach isolation, goal preservation, worktree-root equivalence, pagination
and stale members. Existing UI checks cover dismissal/conversation invalidation, modal input and
draft preservation. The installed memory journey includes explicit group creation and discovery.
The earlier `2026-11-02u` source `a17dcc3` passes all 983 enabled checks, all-target Clippy,
release smoke, portable everyday smoke and OSS gates (330 tracked text files). All 48 installed
ConPTY journeys pass in 314.35 seconds; the focused memory journey passes alone in 12.10 seconds.
That portable candidate is 18,980,352 bytes, SHA-256
`22469DFF9A06BD8836223758031159EB80AE179245E9004A9AEB7E1AEAF66D63`.
Corrected production source `858b48f` (same production tree after main merge `69248d1`) includes
the parent workspace-alias and source-update artifact fixes. It passes all 983 enabled checks,
portable everyday smoke, all-target Clippy with warnings denied, release smoke and OSS gates
(331 tracked text files) in an isolated Cargo target directory. All 48 installed ConPTY
journeys pass in 313.21 seconds. Build d is 18,916,864 bytes; SHA-256
`91D861BD911EB32EB923800D18B7A74C36FAE06C0448FAB58273A7B5A7A11DA4`.
An earlier debug-build attempt exhausted host disk space before testing completed; Cargo's
package-scoped cache cleanup and a fresh run with incremental compilation disabled resolved it.
No live provider or physical-host parity is claimed.
