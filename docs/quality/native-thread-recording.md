# Native thread recording diagnostic, 2026-11-02

Reported symptom: Codex exited 1 with `failed to record rollout items: thread ... not found`.
The selected native home contains the matching rollout, state row, history turns/items and
projection row. A read-only native Codex 0.165.0 app-server initialization followed by
`thread/read` succeeded for that exact id with status `notLoaded`. No session start/resume,
turn start, model request, auth migration or direct database edit was issued. The helper was
closed after this read. Evidence is summarized without private ids/paths in ignored
`target/native-thread-read-2026-11-02a.json`.

The native rollout for that turn ends with a failed task whose error is "Selected model is at
capacity" (`server_overloaded`). The provider rejected the model partway through the turn while a
background shell command was still running. When that command finished, Codex had already closed
the thread, so it could not record the result and wrote the stderr line. Quota was not the cause,
and the saved chat is intact: later turns recorded to the same rollout normally.

The existing failure's local timing record reported setup 7 ms, spawn 11 ms, first output 716 ms,
parser work about 18 ms, and total turn duration 316.471 s. These values distinguish the observed
startup/parse work from the complete native run; they are not a native-versus-wrapper benchmark.

Concordia now reads the turn's own error first. A model-at-capacity error is treated like an
exhausted model: the request is kept and continuation offers another model or account, and
automatic continuation still stops while a tool result is uncertain. The stderr diagnostic is
reported only when the turn carries no such error.

When it does appear on its own, Concordia classifies it separately from quota exhaustion. The unfinished
request is kept, queued work pauses, and explicit `/continue ACCOUNT` starts a fresh session
with source pointers and reconciliation instructions. It never deletes native history or
automatically replays uncertain edits because a persistence write failed.

The [official app-server documentation](https://learn.chatgpt.com/docs/app-server) describes
`thread/read` as inspecting stored data without loading/resuming a thread.
