# Concordia

Concordia is a Windows-first terminal app that runs the official Codex, Claude Code and Cursor
CLIs under your own accounts, with bounded handoff between them. It keeps one conversation
navigable while each provider remains responsible for its own sessions, approvals and tools.

Concordia is independent and unofficial. It is not affiliated with or endorsed by OpenAI,
Anthropic, Anysphere or Cursor. It runs their official, unmodified CLIs using your own accounts.
Each CLI keeps its own login; Concordia only checks that a login exists. The one secret it
handles itself is a Cursor API key, if you choose to use one: it is kept in that Cursor
account's own folder, passed only to the Cursor CLI, and redacted from everything Concordia
writes (logs, history, drafts).

## Install

From a source checkout, run this in Windows PowerShell:

```powershell
.\scripts\install-dev.ps1 -Install
```

This builds and installs the local executable on your user PATH. The public installer command
will be available after the first release; see [the full installation guide](docs/install.md).

## Quick start

```text
concordia
/use                 # choose or connect Codex, Claude Code or Cursor
Ask your question and press Enter
/usage               # inspect provider-reported usage
/resume              # continue a recorded conversation
/model               # choose a model from the active provider
```

<!-- COMMANDS:START -->
| Command | Arguments | Description | View |
| --- | --- | --- | --- |
| `/use` | `ACCOUNT\|add PROVIDER\|remove ACCOUNT` | switch, add or remove accounts | everyday |
| `/model` | `[MODEL]` | choose the model | everyday |
| `/config` | `[KEY VALUE]` | settings: display, permissions, accounts and behavior | everyday |
| `/mode` | `[plan\|ask\|auto\|full]` | how much the agent may do without asking | everyday |
| `/usage` | `[all\|refresh]` | limits for every account | everyday |
| `/new` | `` | start a fresh chat | everyday |
| `/resume` | `` | continue an earlier chat | everyday |
| `/delegate` | `ACCOUNT TASK` | run tasks on worker accounts | everyday |
| `/workers` | `[add\|remove]` | manage worker accounts | everyday |
| `/crew` | `[list\|use NAME\|clear]` | choose a saved lead and worker preset | everyday |
| `/worker-panel` | `` | toggle the worker side panel | everyday |
| `/harness` | `ACTION` | connect and run harness tools | advanced |
| `/open` | `[PATH]` | preview files mentioned in this chat | everyday |
| `/preview` | `PATH` | preview a file read-only | advanced |
| `/search` | `TEXT` | search project file contents | advanced |
| `/scope` | `...` | declare task paths and reservations | advanced |
| `/worktree` | `...` | manage explicit Git workspaces | advanced |
| `/memory` | `...` | search original chats and source links | advanced |
| `/context` | `` | inspect context rollover policy | advanced |
| `/integrations` | `` | inspect connected integrations | advanced |
| `/copy` | `` | copy the last reply | everyday |
| `/doctor` | `` | check your setup | everyday |
| `/update` | `` | update Concordia | everyday |
| `/exit` | `` | exit Concordia | everyday |
<!-- COMMANDS:END -->

Use `/doctor` inside Concordia, or `concordia doctor` in a terminal, when setup or provider
discovery needs diagnosis.

## Safety at a glance

- Plan, Ask and Workspace modes limit what a provider may do. Full access requires explicit
  `/mode full` confirmation.
- Workers never exceed the mode selected for their parent conversation.
- Concordia itself makes no LLM calls; routing, memory and handoff are deterministic local logic.
- Nothing leaves your machine through Concordia. Network traffic comes from the provider CLIs'
  own supported connections (and any explicitly requested update or integration).
- Native provider authentication, approvals and session state remain authoritative. See
  [provider terms and boundaries](docs/provider-terms.md).

## Learn more

- [Concepts and capabilities](docs/capabilities.md)
- [Architecture and data boundaries](docs/architecture.md)
- [Memory and context handoff](docs/context-handoff-plan.md)
- [Commands, controls and UI glossary](docs/ui-glossary.md)
- [Configuration](docs/config.md)
- [Images and diagrams](docs/images-and-diagrams.md)
- [Contributing](CONTRIBUTING.md)
- [Internal development notes](docs/contributing-internals.md)
- [Metrics](docs/metrics.md)
- [License](LICENSE)
