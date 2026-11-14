# Guided commands

Every Concordia slash command with an argument form is listed here. Running the command without
arguments opens a guided panel. The panel explains the command, shows detected choices where the
command has local state, validates a choice before applying it, describes the change, and gives an
undo or cancellation path. Keyboard arrows and Enter work alongside mouse selection. Esc closes
the panel without changing state.

| Command | Argument forms | Detected choices and effect |
| --- | --- | --- |
| `/terminal` | `open`, `native`, `hide`, `close` | Local PowerShell and the selected provider session; opening starts a pane, hiding keeps it running, and closing stops it. |
| `/delegate` | `input ID`, `fallback on\|off`, `recover ID`, `retry ID reconciled` | Waiting worker input and recovered worker records detected from local state; choosing an input reviews it, fallback changes automatic return policy, recover reads the exact native result, and retry starts only after explicit reconciliation. |
| `/use` | `ACCOUNT`, `add PROVIDER`, `remove ACCOUNT`, `rename ACCOUNT NICKNAME` | Connected accounts, supported providers, removable accounts, and saved nicknames from local state; selection changes only the selected account or explicit account record. |
| `/model` | `MODEL` | Models read from the selected provider account; selection changes the saved model and the provider default remains available. |
| `/rename` | `TITLE` | The current chat title; setting replaces it and clearing restores automatic naming. |
| `/config` | `KEY VALUE` | Saved settings and detected account, model, skill, hook, integration, and update state; each setting panel describes its saved scope and reversal. |
| `/mode` | `plan`, `ask`, `edit`, `full` | Permission modes with their current value; selection applies to the next turn and the existing mode can be selected again to undo. |
| `/usage` | `all`, `refresh` | Provider account usage snapshots; choosing an account changes only the displayed account, and refresh rereads provider snapshots. |
| `/help` | `--all` | The command registry; choose everyday commands or every registered command, and Esc closes without changing state. |
| `/harness` | `connect`, `status`, `verify`, `disconnect`, `skill`, `tools` | Detected local manifests, connected receipts, and declared tools; review confirms the files and receipts changed and shows the matching undo. |
| `/skills` | `list`, `install [PATH]`, `remove PATH` | Detected skill folders and account links; install or removal validates `SKILL.md` and can be reversed by the opposite action. |
| `/hooks` | `list` | Detected native hook sources; the panel is read-only and native trust remains with the provider. |
| `/mcp` | `add`, `remove`, `on`, `off`, `import` | Servers in the local registry; add asks for a name and command, previews the enabled stdio entry, then saves only after confirmation; toggle, remove, or inspect changes only the Concordia registry and shows its inverse action. |
| `/open` | `PATH` | Files mentioned in the current chat and related files detected on disk; selection opens a read-only preview, and Esc closes it without editing. |
| `/resume` (`/chats`) | `SESSION_ID` | Recorded tasks and native sessions detected in local state; choosing one resumes that explicit conversation, and Esc leaves the current chat unchanged. |
| `/native` | `ACCOUNT on\|off\|status` | Connected accounts and their saved transport state; choosing status reads it, on enables native transport, and off restores print mode. |
| `/queue` | `resume\|clear\|send [N]` | Queued prompts detected in the current chat; review shows them, resume sends them in order, and clear removes queued prompts. |
| `/close` | `PANE_ID` | Open file panes detected in the current chat; choosing one closes that pane, and reopening the file is the undo. |

Provider-owned slash commands are passed through to the active provider and are not Concordia
commands. Their argument panels remain provider-owned.
