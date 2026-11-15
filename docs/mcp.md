# MCP registry and delivery

Concordia stores its MCP registry in `<state>/mcp.toml`. `/mcp` reads and changes this file. The
registry is separate from provider homes. Concordia never writes a user's `.claude`, `.codex` or
`.cursor` home. Registry mutations are serialized; an unreadable config is preserved and the
requested change is refused.

Each server has an enabled switch and one transport. Stdio entries contain a command, arguments
and environment references. Remote entries contain a URL and header references. A reference is
`{ env = "NAME" }` or `{ credential = "TARGET" }`. Literal secret values are not accepted.
Project switches live under `projects` and can turn a global server off.

| Provider | Launch interface | Current boundary |
| --- | --- | --- |
| Codex | `-c mcp_servers.<name>...` overrides | Every configured server is projected through its own local proxy; project switches are checked for discovery and call. |
| Cursor | ACP `session/new` and `session/load` `mcpServers` | Every server is a server-bound local Concordia proxy process. |
| Grok Build | ACP `session/new` and `session/load` `mcpServers` | Every server is a server-bound local Concordia proxy process. |
| opencode | ACP `session/new` and `session/load` `mcpServers` | Every server is a server-bound local Concordia proxy process. |
| Claude Code | `--mcp-config <JSON file>` plus `--strict-mcp-config` | A per-launch projection contains only server-bound local proxies; no provider-home file is written. |

`/mcp` lists state for Claude Code, Codex, Cursor, Grok Build and opencode. `/mcp add NAME stdio
COMMAND [ARGS...]`, `/mcp remove NAME`, `/mcp on NAME` and `/mcp off NAME` mutate the registry.
Bare `/mcp` shows the guided panel. `/mcp import` reads supported provider files without writing
them and reports entries that can be reviewed for addition.

`concordia mcp` serves the registry's remote and stdio MCP servers through one Concordia-owned
local proxy per server. A proxy is bound to its selected server and project, so references resolve
only for that child/request. Secret values are injected into the child environment or outbound
headers and never appear in argv, registry files, provider homes, logs or transcript. A missing
credential shows as a sign-in action. Redaction markers say only that a value was removed; they do
not contain value-derived fingerprints.

Project switches are checked during discovery and again during `tools/call`; a disabled server is
not reachable by naming it directly. Stdio exchanges have a 10-second deadline, 256 KiB frame
limit, 2 MiB response budget and 256-frame cap, then clean up the child process. Remote HTTP has a
10-second total deadline, 2 MiB response budget and 16 KiB header limit, preserves the negotiated
MCP session ID and accepts matching JSON or SSE responses. Windows HTTPS uses native WinHTTP;
non-Windows HTTPS is intentionally unsupported. OAuth browser sign-in and upstream HTTP session
persistence are not shipped, so OAuth-only servers remain a live qualification gap.

## Existing Concordia MCP server

`concordia mcp` serves JSON-RPC over newline-delimited stdin/stdout. It registers exactly one
always-on tool, `agents`, with four actions:

| Action | Result |
| --- | --- |
| `list` | Connected accounts, providers, discovered models and cached usage. |
| `start` | Starts a delegated provider run and returns its lifecycle id. |
| `status` | Current lifecycle status (`queued`, `running`, `finished` or `failed`) for an explicit id. |
| `result` | The bounded final result and recent activity for an explicit id. |

`start` accepts a task and any connected account. Provider and model choices are supplied by the
calling agent or its harness; Concordia does not apply a routing table. Each proposal carries the
exact parent conversation, canonical workspace and parent run IDs. The child stays within the
parent chat's permission mode. Task text is the Agents view title, never raw task JSON.

The server does not expose chat history, memory, usage or integrations.
History remains available through the `concordia chat` CLI and handoff pointers. The per-turn
`ask_user` bridge is separate and unchanged. MCP registration in Concordia-created provider homes
remains controlled by `[threads] mcp = true`; personal provider homes are never edited.

MCP never reads credentials or launches a provider during catalog listing. Cached usage is labeled
with its freshness. Account and agent IDs are explicit; malformed IDs, invalid parent/workspace
bindings and oversized requests are rejected with bounded records at the JSON-RPC boundary. The
worker record is bound before launch and completion updates require the exact agent/run identity.

Lifecycle records are local Concordia metadata; provider sessions and approvals remain provider-owned.
