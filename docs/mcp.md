# Thread references and MCP

Type `@` in a prompt to choose a recorded thread. Concordia expands the reference to its
bounded map and shows that map with the submitted prompt. Recording is opt-in through
`[threads] enabled = true`. A provider can retrieve more with `concordia thread read <id>`.

`concordia mcp` serves JSON-RPC over newline-delimited stdin/stdout. It exposes `threads`,
`thread_map`, `thread_read`, `native_read`, `usage`, `memory`, `goals`, `delegate` and `integrations`. Read tools stay local;
`native_read` resolves revision-checked native-session pointers without transcript recording.
`delegate` writes a proposal; the user must approve it in the TUI before any worker starts.
It is available only when delegation protocol is enabled.

Browser MCP servers configured in a provider are separate from this server and retain their
own executables and update policies. See the [browser security audit](quality/browser-security.md)
for the checked scope, old local automation installations, and the Harness audit command.

Set `[threads] mcp = true` in `config.toml` to register the server in Concordia-created
Claude and Codex homes. Registration is off by default; schemas add provider context usage.
Existing personal provider homes are never edited. Cursor registration is not supported.

`[threads] awareness = true` adds a short explanation of Concordia maps to instructions in
Concordia-created homes. Turn it off to remove Concordia's marked block. `/use` refreshes
these integration settings; `/doctor` reports their status without changing files.

`native_read` accepts an exact index `id`, optional `message` ordinal (1-based), `offset` in bytes
and `max_bytes` (1..8192, default 4096). Without `message`, it returns range metadata. Generate an
index with `concordia chat index SESSION --account ACCOUNT --provider codex|claude`; handoffs can
supply index IDs automatically. Stale/unavailable sources produce an error, never another session.
See [native memory](native-memory.md) for source validation, budgets and Cursor limitations.

`goals` reads the existing goal store in pages of ten, with bounded display text. Supply the
returned `next_offset` and `revision` for a later page; changed state requires a fresh listing.
With `goal` and an exact `workspace`, it reads confirmed goal/source association metadata instead.
Metadata listings launch no provider or native replay and do not claim that a source is currently
available; use `native_read` for revision-checked readback. Source links cannot be mutated through
MCP. The nine-tool schema stays below 4,000 bytes; MCP registration remains opt-in.

Goal changes use `action: "propose_add"` with `text`, or `action: "propose_close"` with an exact
`goal` id. Both require a canonical UUID `request_id` and the current goal-list `revision`.
These calls only queue proposals. The local UI shows the full proposed goal, observed revision
and unverified requester, then requires F7 focus and Y/N. Existing message drafts remain intact.
`action: "status"` with `request_id` reads the result/uncertainty without replay. Goal proposals
do not require enabling worker delegation and never authorize native tools or workers.
[Goal change contracts and recovery](quality/goal-proposals.md).

`integrations` discovers existing Harness connection metadata and Concordia-owned MCP registration
status without executing adapters. With an integration `id`, it returns eight namespaced tool
descriptions at a time. Later `offset` pages require the exact returned manifest `revision`.
Descriptions are untrusted display data and grant no permissions. Changed manifests expose no
current tool list; command-backed manifests need an explicit Harness refresh. The same catalog
opens with F10 or `/integrations`, preserving the message draft. See [catalog limits](quality/integrations-catalog.md).

`memory` gives agents structured read-only discovery without asking the user to type `/memory`.
Choose `recall` (literal search), `ranked` (opt-in evidence ranking), `files` (historical file facts),
`groups` or `members`. Search accepts `query`, optional exact `source`, `account` and `workspace`;
ranking also accepts `goal` and `file`. Groups require `workspace`, members require `group`, and
files require `source`. Paged metadata accepts the returned `offset` and `revision`. Native freshness,
scope, provenance and coverage stay in the result. Oversized results are refused with narrowing
instructions, rather than cutting through JSON or an exact source pointer. This tool cannot confirm
relationships, change memberships, approve native requests or start a provider.
