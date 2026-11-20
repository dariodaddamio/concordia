# Cooperative workspace reservations

Build `2026-11-02r` implements orchestration W1. This is scheduler admission across Concordia
panes sharing one state root, not a filesystem sandbox. External editors, older app versions and
native tools can write outside declarations. Managed Git worktrees and their lifecycle remain W2/W3.

## Admission behavior

- Ordinary foreground chats record their workspace ownership without blocking another foreground
  chat in the same folder. This applies across instances and providers. Worker admission continues
  to coordinate intersecting write claims.
Main-turn admission and delegated-agent admission use the same cooperative registry internally.
There is no public command for editing or inspecting reservations. An uncertain native process is
reconciled by the owning recovery flow; acknowledgment never retries a provider turn.

Read/read and disjoint claims can coexist. A write intersecting another read or write waits
for workers and pauses explicitly scoped main dispatch before provider launch. Default foreground
chats may proceed concurrently; their own uncertain previous attempts still require reconciliation.
Internal file saves hold the same
registry lock across ownership checking and compare-before-replace; a refusal keeps the buffer.
The native provider retains its own trust, approval and filesystem-access rules. Explicit
declarations are included as task intent; they grant no native permission.

## Persistence and recovery

`workspace-registry.json` is version 2, OS-locked and atomically replaced. It holds metadata,
not file contents: canonical execution/owner folder, native directory/file identities,
conversation/run/attempt UUIDs and declarations. Worker ledger v5 and unfinished-request v3
capture those bindings; older supported records remain readable and paused. Unknown or corrupt
versions remain untouched and block unsafe dispatch.

Confirmed native exits release claims. A launch rejected before invoking the provider also
releases them. Uncertain exits, failed receipt writes and crashes retain reconciliation state;
there is no timer expiry, inferred successful exit or automatic native replay. Late completions
cannot release a newer attempt. A new attempt may refresh file identities only after the prior
attempt was released and only within its unchanged approved scope.

Windows comparisons use the OS ordinal case table, including missing write leaves. Existing
explicit file claims also compare native file IDs to detect hard-link aliases. Junctions and
symlinks in declarations, traversal, sensitive paths, `.git`, alternate streams, reserved device
names and ambiguous/missing parents are rejected. Root/file identities are revalidated at admission.
The lock coordinates app admission and app saves; it cannot make external filesystem changes atomic.

Bounds: 64 declarations and 8 KiB declaration text per task; 64 active reservations;
4,096 retained records and 2 MiB registry; inspection up to 16 rows/30,000 bytes per page.
Full registries refuse new work without pruning recovery metadata. Inspection runs off the input
thread, labels partial pages and discards results for an old conversation.

## Verification

Existing synthetic fixtures cover overlapping trees/read snapshots, disjoint claims, read/read,
explicit acknowledgment, stale atomic replacements, guarded saves, a real Windows junction,
case aliases, hard-link aliases, invalid declarations and preserved corrupt metadata. The existing
two-process reliability fixture races independent processes for the same folder and requires
exactly one admission and one durable record. Main checkpoint recovery and worker terminal
recovery verify no replay. No model turn is used for these checks.

Final candidate validation and artifact identity are recorded in `docs/e2e-status.md` after execution.
These checks do not establish live-provider parity, filesystem confinement or physical-host behavior.
