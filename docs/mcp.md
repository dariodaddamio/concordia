# Thread references and MCP

Type `@` in a prompt to choose a recorded thread. Concordia expands the reference to its
bounded map and shows that map with the submitted prompt. Recording is opt-in through
`[threads] enabled = true`. A provider can retrieve more with `concordia thread read <id>`.

`concordia mcp` serves JSON-RPC over newline-delimited stdin/stdout. It exposes `threads`,
`thread_map`, `thread_read`, `usage`, and `delegate`. The first four read local Concordia data.
`delegate` writes a proposal; the user must approve it in the TUI before any worker starts.
It is available only when delegation protocol is enabled.

Set `[threads] mcp = true` in `config.toml` to register the server in Concordia-created
Claude and Codex homes. Registration is off by default; schemas add provider context usage.
Existing personal provider homes are never edited. Cursor registration is not supported.

`[threads] awareness = true` adds a short explanation of Concordia maps to instructions in
Concordia-created homes. Turn it off to remove Concordia's marked block. `/use` refreshes
these integration settings; `/doctor` reports their status without changing files.
