# Native thread recording diagnostic, 2026-11-02

Reported symptom: Codex exited 1 with `failed to record rollout items: thread ... not found`.
The selected native home contains the matching rollout, state row, history turns/items and
projection row. A read-only native Codex 0.165.0 app-server initialization followed by
`thread/read` succeeded for that exact id with status `notLoaded`. No session start/resume,
turn start, model request, auth migration or direct database edit was issued. The helper was
closed after this read. Evidence is summarized without private ids/paths in ignored
`target/native-thread-read-2026-11-02a.json`.

This rules out a simply absent saved chat in the examined home. It does not identify the exact
native runtime/history-writer cause or prove that another model turn can be recorded. The
installed version is newer than Concordia's recorded 0.163.0 Codex fixture baseline. Neither
version mismatch nor wrapper session routing is thereby established as the cause. Reproduction
with a real model needs an explicit task/budget; no such turn was run.

The existing failure's local timing record reported setup 7 ms, spawn 11 ms, first output 716 ms,
parser work about 18 ms, and total turn duration 316.471 s. These values distinguish the observed
startup/parse work from the complete native run; they are not a native-versus-wrapper benchmark.

Concordia now classifies this diagnostic separately from quota exhaustion. The unfinished
request is kept, queued work pauses, and explicit `/continue ACCOUNT` starts a fresh session
with source pointers and reconciliation instructions. It never deletes native history or
automatically replays uncertain edits because a persistence write failed.

The [official app-server documentation](https://learn.chatgpt.com/docs/app-server) describes
`thread/read` as inspecting stored data without loading/resuming a thread. It does not establish
the root cause of this particular recording failure.
