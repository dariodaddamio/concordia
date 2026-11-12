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
listed for the active provider in the `/` picker. They are disabled there because the integrations
use non-interactive transports; typing an unlisted slash name still sends it to the active provider
instead of producing an unknown-command error.

| Command names | Owner | During a turn |
| --- | --- | --- |
| `/model`, `/usage`, `/resume`, `/config`, `/doctor`, `/skills`, `/hooks` | Concordia | Immediate; state changes apply to the next prompt where applicable |
| `/use`, `/new`, `/mode`, `/open`, `/copy`, `/update`, `/harness`, `/exit` | Concordia | Immediate |
| Codex: `/compact`, `/diff`, `/fork`, `/init`, `/mcp`, `/plan`, `/review`, `/status`, `/theme`, `/vim`, `/clear`, `/export` | Codex | Disabled in `codex exec --json`; direct typing is passed as provider prompt |
| Claude: `/compact`, `/clear`, `/init`, `/mcp`, `/memory`, `/review`, `/rewind`, `/vim`, `/help` | Claude Code | Disabled in `claude -p --output-format stream-json`; direct typing is passed as provider prompt |
| Cursor: `/auto-run`, `/new-chat`, `/vim`, `/feedback`, `/copy-req-id`, `/logout`, `/quit` | Cursor Agent | Disabled in `cursor-agent --print --output-format stream-json`; direct typing is passed as provider prompt |
| Any other slash name | Active provider | Passed through as the next provider prompt |

Provider references: [Codex slash-command source](https://github.com/openai/codex/blob/main/codex-rs/tui/src/slash_command.rs),
[Claude CLI reference](https://docs.anthropic.com/en/docs/claude-code/cli-usage),
[Cursor slash commands](https://docs.cursor.com/en/cli/reference/slash-commands),
[Cursor print mode](https://docs.cursor.com/en/cli/reference/parameters).
