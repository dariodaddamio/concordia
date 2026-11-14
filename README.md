# Concordia

Concordia allows any supported provider to share context, continue the same task on another account after a usage limit, delegate work across providers with a model per role, and hand off exact pointers into provider history instead of summaries, among others ([full feature list](site/src/content/docs/features.md)).

Concordia runs official provider CLIs under your accounts. Providers keep ownership of their
authentication, sessions, approvals and tools. Concordia is independent and unofficial.

## Install

From a source checkout, run this in Windows PowerShell:

```powershell
.\scripts\install-dev.ps1 -Install
```

This builds and installs the local executable on your user PATH. See the [installation guide](site/src/content/docs/install.md).

## Quick start

```text
concordia
/use
Ask your question and press Enter
/usage
/resume
/model
```

<!-- COMMANDS:START -->
| Command | Arguments | Description | View |
| --- | --- | --- | --- |
| `/terminal` | `[open\|native\|hide\|close]` | open the local PowerShell terminal | everyday |
| `/use` | `ACCOUNT\|add PROVIDER\|remove ACCOUNT` | switch, add or remove accounts | everyday |
| `/model` | `[MODEL]` | choose the model | everyday |
| `/rename` | `[TITLE]` | rename the current chat | advanced |
| `/config` | `[KEY VALUE]` | settings: display, permissions, accounts and behavior | everyday |
| `/mode` | `[plan\|ask\|edit\|full]` | how much the agent may do without asking | everyday |
| `/usage` | `[all\|refresh]` | limits for every account | everyday |
| `/new` | `` | start a fresh chat | everyday |
| `/resume` | `` | continue an earlier chat | everyday |
| `/harness` | `ACTION` | connect and run harness tools | advanced |
| `/skills` | `[list\|install [PATH]\|remove PATH]` | inspect skills and toggle their availability | advanced |
| `/hooks` | `[list]` | inspect native hooks | advanced |
| `/mcp` | `[add\|remove\|on\|off\|import] [NAME]` | manage Concordia-wide MCP servers | advanced |
| `/open` | `[PATH]` | preview files mentioned in this chat | everyday |
| `/copy` | `` | copy the last reply | everyday |
| `/doctor` | `` | check your setup | everyday |
| `/update` | `` | update Concordia | everyday |
| `/exit` | `` | exit Concordia | everyday |
<!-- COMMANDS:END -->

## Providers

Claude Code, Codex, Cursor, Grok Build, opencode and optional Grok Bot routines are supported.
See the [provider guide](site/src/content/docs/providers.md).

Needs attention uses the taskbar status and Windows notifications for actionable questions and
approvals. Each item is notified once across focus changes and restarts. `/config` can turn off
Windows notifications while keeping the taskbar status.

## Site

- [Overview](site/src/content/docs/index.md)
- [Features](site/src/content/docs/features.md)
- [Quick start](site/src/content/docs/quick-start.md)
- [Windows security warnings](site/src/content/docs/install.md#windows-security-warnings)
