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
listed for the active provider in the `/` picker. They are enabled when the active transport has a
documented invocation; unsupported operations remain visible but disabled with their transport
limitation. Typing an unlisted slash name still sends it to the active provider instead of
producing an unknown-command error.

| Command names | Owner | During a turn |
| --- | --- | --- |
| `/model`, `/usage`, `/resume`, `/config`, `/doctor`, `/skills`, `/hooks` | Concordia | Immediate; state changes apply to the next prompt where applicable |
| `/use`, `/new`, `/mode`, `/open`, `/copy`, `/update`, `/harness`, `/exit` | Concordia | Immediate |
| Codex: `/compact`, `/review` | Codex app-server | `thread/compact/start` and `review/start` on the exact active thread; review targets uncommitted changes |
| Codex: `/diff`, `/fork`, `/init`, `/mcp`, `/plan`, `/status`, `/theme`, `/vim`, `/clear`, `/export` | Codex app-server | Disabled: no matching RPC, or thread-fork identity handoff is not wired |
| Claude: `/compact`, `/clear`, `/init`, `/mcp`, `/memory`, `/review`, `/rewind`, `/help` | Claude Agent SDK stream-json | Sent as user text in a streaming input message; built-ins and user-invoked skills are dispatched by Claude |
| Claude: `/vim` | Claude Agent SDK stream-json | Disabled: interactive terminal-only command is not exposed by the SDK session |
| Cursor and Grok Build ACP commands | ACP | Enabled through `session/prompt` when the agent advertises the name in `available_commands_update`; the advertised set is session-dependent |
| Any other slash name | Active provider | Passed through as the next provider prompt |

Provider references: [Codex app-server protocol](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md),
[Claude Agent SDK streaming input](https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode),
[Claude SDK commands and skills](https://code.claude.com/docs/en/agent-sdk/skills),
[ACP slash commands](https://agentclientprotocol.com/protocol/v1/slash-commands),
and [ACP prompt turns](https://agentclientprotocol.com/protocol/v1/prompt-turn).

The Codex app-server protocol does not confirm every terminal slash command as an RPC. In
particular, `/fork` is intentionally unverified as a Concordia operation because the provider
returns a new thread identity that this UI does not yet adopt. Cursor/Grok command names remain
unverified until their active ACP session advertises them.
