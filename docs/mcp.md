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
| Codex | `-c mcp_servers.<name>...` overrides | Stdio and URL entries are projected per turn. |
| Cursor | ACP `session/new` and `session/load` `mcpServers` | Uses the ACP projection. |
| Grok Build | ACP `session/new` and `session/load` `mcpServers` | Uses the ACP projection. |
| opencode | ACP `session/new` and `session/load` `mcpServers` | Uses the ACP projection. |
| Claude Code | `--mcp-config` JSON file | Not delivered yet. No provider-home or secret-bearing temporary file is written. |

`/mcp` lists state. `/mcp add NAME stdio COMMAND [ARGS...]`, `/mcp remove NAME`, `/mcp on NAME`
and `/mcp off NAME` mutate the registry. Bare `/mcp` shows the guided syntax. Import discovery is
read-only at the registry API boundary; the interactive import panel is not shipped yet.

The registry does not implement a proxy or remote OAuth sign-in. `concordia mcp` still serves the
existing `agents` tool only. Remote servers that need provider-specific authentication therefore
show a missing sign-in or unsupported boundary instead of being represented as complete.

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
