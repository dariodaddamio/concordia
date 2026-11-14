# MCP registry and delivery

Concordia stores its MCP registry in `<state>/mcp.toml`. `/mcp` reads and changes this file. The
registry is separate from provider homes. Concordia never writes a user's `.claude`, `.codex` or
`.cursor` home.

Each server has an enabled switch and one transport. Stdio entries contain a command, arguments
and environment references. Remote entries contain a URL and header references. A reference is
`{ env = "NAME" }` or `{ credential = "TARGET" }`. Literal secret values are not accepted.
Project switches live under `projects` and can turn a global server off.

| Provider | Launch interface | Current boundary |
| --- | --- | --- |
| Codex | `-c mcp_servers.<name>...` overrides | Stdio and remote entries are projected per turn. Remote calls use the local Concordia proxy. |
| Cursor | ACP `session/new` and `session/load` `mcpServers` | Uses the ACP projection; HTTP upstreams use the local Concordia proxy. |
| Grok Build | ACP `session/new` and `session/load` `mcpServers` | Uses the ACP projection; HTTP upstreams use the local Concordia proxy. |
| opencode | ACP `session/new` and `session/load` `mcpServers` | Uses the ACP projection; HTTP upstreams use the local Concordia proxy. |
| Claude Code | `--mcp-config <JSON file>` plus `--strict-mcp-config` | Delivered per launch through a secret-free projection under Concordia state. No provider-home file is written; secret references remain references. |

`/mcp` lists state for Claude Code, Codex, Cursor, Grok Build and opencode. `/mcp add NAME stdio
COMMAND [ARGS...]`, `/mcp remove NAME`, `/mcp on NAME` and `/mcp off NAME` mutate the registry.
Bare `/mcp` shows the guided panel. `/mcp import` reads supported provider files without writing
them and reports entries that can be reviewed for addition.

`concordia mcp` serves the registry's remote and stdio MCP servers through one Concordia-owned MCP
endpoint. Remote bearer references are resolved from Windows Credential Manager only for the
outbound request; they are never written to the registry, provider homes, logs or transcript. The
proxy re-exports upstream `tools/list` entries with a `SERVER/TOOL` name and unwraps the upstream
result for `tools/call`. Windows HTTPS uses WinHTTP. A missing credential shows as a sign-in action.
OAuth browser sign-in and upstream HTTP session persistence are not yet shipped, so OAuth-only
servers remain a live qualification gap and must not be described as complete.

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
calling agent or its harness; Concordia does not apply a routing table. The child stays within the
parent chat's permission mode. Task text is the Agents view title, never raw task JSON.

The server does not expose chat history, memory, usage or integrations.
History remains available through the `concordia chat` CLI and handoff pointers. The per-turn
`ask_user` bridge is separate and unchanged. MCP registration in Concordia-created provider homes
remains controlled by `[threads] mcp = true`; personal provider homes are never edited.

MCP never reads credentials or launches a provider during catalog listing. Cached usage is labeled
with its freshness. Account and agent ids are explicit, and malformed or oversized requests are
rejected at the JSON-RPC boundary.

Lifecycle records are local Concordia metadata; provider sessions and approvals remain provider-owned.
