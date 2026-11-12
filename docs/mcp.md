# Agents MCP

`concordia mcp` serves JSON-RPC over newline-delimited stdin/stdout. It registers exactly one
always-on tool, `agents`, with four actions:

| Action | Result |
| --- | --- |
| `list` | Connected accounts, providers, discovered models and cached usage. |
| `start` | An explicit delegated agent id and its queued status. |
| `status` | Current lifecycle status (`queued`, `running`, `finished` or `failed`) for an explicit id. |
| `result` | The bounded final result and recent activity for an explicit id. |

`start` accepts a task and any connected account. Provider and model choices are supplied by the
calling agent or its harness; Concordia does not apply a routing table. The child stays within the
parent chat's permission mode. Task text is the Agents view title, never raw task JSON.

The server does not expose chat history, memory, usage, integrations or legacy delegation tools.
History remains available through the `concordia chat` CLI and handoff pointers. The per-turn
`ask_user` bridge is separate and unchanged. MCP registration in Concordia-created provider homes
remains controlled by `[threads] mcp = true`; personal provider homes are never edited.

MCP never reads credentials or launches a provider during catalog listing. Cached usage is labeled
with its freshness. Account and agent ids are explicit, and malformed or oversized requests are
rejected at the JSON-RPC boundary.

Lifecycle records are local Concordia metadata; provider sessions and approvals remain provider-owned.
