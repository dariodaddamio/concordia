# Command and state audit

The current command surface is maintained by the [source registry](../crates/concordia/src/commands.rs),
its generated [README table](../README.md), [configuration reference](config.md) and [MCP reference](mcp.md).
Those documents own current behavior; the previous duplicated command inventory is preserved in
[the historical audit](archive/command-audit.md).

The 2026-11-08 repository audit reviews parsing, dispatch, native permissions, identity, drafts,
background completion and storage boundaries. Its coverage and verification are preserved in
[the historical cleanup journal](e2e-status.md#earlier-evidence). A fixture proves its stated path, not every command,
provider version or possible state ordering.

## Slash-command ownership

Concordia owns commands that span providers, accounts or sessions. Provider-native commands are
listed for the active provider in the `/` picker. Operations with a documented transport
invocation use it; terminal-only operations open the provider's native interface on the same
session. Typing an unlisted slash name still sends it to the active provider instead of
producing an unknown-command error. Shared names open the owner picker; `/model` and `/effort`
already operate on the selected provider and keep their direct entry.

Source candidate `2026-11-17a` uses ACP `available_commands_update` snapshots for the selected
session, including advertisements received during session setup. A catalog becomes usable only
after successful exact-session binding and stays available after a successful turn. Provider,
account, home, project and conversation must still match. A new connection, failed/cancelled
turn, account/transport change or native-terminal handoff invalidates that snapshot.

An advertised empty list removes provider suggestions. Before a valid snapshot, ACP baseline
suggestions say availability is not yet reported; other provider catalogs remain static.
Malformed metadata produces an explicit notice. Custom templates cannot replace advertised
native names, and shared picker choices are revalidated before invocation. Changes that remove
or change the owner of a selected suggestion dismiss that suggestion rather than invoking a
different action. This is scoped ACP discovery, not universal provider parity.

| Command names | Owner | During a turn |
| --- | --- | --- |
| `/model`, `/usage`, `/resume`, `/config`, `/doctor`, `/skills`, `/hooks` | Concordia | Immediate; state changes apply to the next prompt where applicable |
| `/use`, `/new`, `/mode`, `/open`, `/copy`, `/update`, `/harness`, `/exit` | Concordia | Immediate |
| Codex: `/compact`, `/review`, `/fork` | Codex app-server | `thread/compact/start`, `review/start`, `thread/fork` on the active thread; review covers uncommitted changes; fork moves the chat to a new thread |
| Codex: `/diff`, `/init`, `/mcp`, `/plan` | Native interface | Open on the current native session |
| Codex: `/status`, `/theme`, `/vim`, `/clear`, `/export` | Native interface | Open on the current native session |
| Claude: `/compact`, `/clear`, `/init`, `/mcp` | Claude Agent SDK stream-json | Sent as streaming user input; Claude dispatches its built-ins and skills |
| Claude: `/memory`, `/review`, `/rewind`, `/help` | Claude Agent SDK stream-json | Sent as streaming user input; Claude dispatches its built-ins and skills |
| Claude: `/vim` | Native interface | Interactive terminal-only operation opens the current native session |
| Cursor, Grok Build and opencode ACP commands | ACP | Advertised session commands populate suggestions and invoke through `session/prompt` |
| Any other slash name | Active provider | Passed through as the next provider prompt |

Provider references: [Codex app-server protocol](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md),
[Claude Agent SDK streaming input](https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode),
[Claude SDK commands and skills](https://code.claude.com/docs/en/agent-sdk/skills),
[ACP slash commands](https://agentclientprotocol.com/protocol/v1/slash-commands),
and [ACP prompt turns](https://agentclientprotocol.com/protocol/v1/prompt-turn).

The Codex app-server protocol does not confirm every terminal slash command as an RPC. `/fork`
uses `thread/fork` from the installed 0.166.0 schema; Concordia adopts the returned thread only
after checking its workspace and approval policy. Cursor/Grok command names remain
unverified until their active ACP session advertises them.
