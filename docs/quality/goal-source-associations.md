# Goal/source associations

Baseline: dependency candidate `0963dbc`. Build `2026-11-02n`.
Scope: shared existing goal service, explicit goal/source metadata and read-only MCP discovery.
State: fixture-verified. Existing library checks and the CLI/MCP native-memory fixtures pass.
The association fixture covers concurrent attachment, idempotent duplicate attachment, revision
paging, corrupt/future store preservation, stale-source refusal and equal native session IDs in
different account homes. Native-memory and MCP integration suites pass (three tests total).
Final candidate and installed/remote checks remain separate.

One `goals.json` service supplies the landing page and memory APIs. Goal text/creation are
immutable and completion is monotonic. Association changes do not rewrite the goal file.

Relations retain canonical workspace, exact native pointer and goal identities, explicit
user-confirmed provenance and attach/detach events. Native replies and goal text are not copied
into this store. Duplicate attachment is idempotent. Detachment keeps provenance and does not
delete source logs, index pages or goals. Closing a goal retains its links.

The global metadata file is limited to 2 MiB, 1,024 relation identities and 4,096 changes. This
initial global bound is stricter than the plan's per-workspace proposal. Atomic writes and one
OS lock prevent concurrent lost updates. Unsupported/corrupt state is retained and reported.
Attachment verifies native source identity and every retained record hash under the mutation lock.

CLI/TUI/MCP reads use pages of at most ten. Relationship responses also stop at 12,000 encoded
row bytes. Subsequent pages require the exact revision; a changed snapshot requires restarting.
Goal text is a bounded display preview with an explicit truncation flag. Metadata listings do
not replay a native transport or imply current source availability. Exact readback stays native.
The opt-in MCP schema now contains seven tools within a reviewed 3,000-byte budget.

File relationships/ranking, groups and model-requested goal mutations remain later slices.
No real provider turns or provider-home changes are part of this work.
