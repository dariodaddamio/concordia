---
title: Commands
description: Command ownership and the active command surface.
---

Concordia owns commands that span providers, accounts or sessions. Provider-native commands stay with the active provider. The slash picker shows the command owner and disables operations that the active transport cannot expose.

| Command names | Owner | Behavior |
| --- | --- | --- |
| `/model`, `/usage`, `/resume`, `/config`, `/doctor` | Concordia | Cross-provider state or inspection. |
| `/skills`, `/hooks`, `/harness` | Concordia | Local extension and policy surfaces. |
| `/terminal`, `/use`, `/new`, `/rename`, `/mode`, `/open`, `/copy`, `/update`, `/exit` | Concordia | Immediate local actions. |
| `/compact`, `/review` in Codex | Codex | Sent to the Codex app-server thread. |
| Claude native commands | Claude Code | Sent through Claude's stream-json session. |
| Cursor, Grok Build and opencode commands | ACP provider | Enabled only when the session advertises them. |
| Any other slash name | Active provider | Passed as provider input. |

The source registry lives in `crates/concordia/src/commands.rs`. The current command audit is [in the repository](https://github.com/dariodaddamio/concordia/blob/main/docs/command-audit.md).
