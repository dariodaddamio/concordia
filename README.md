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
| `/terminal` | `[open\|native\|help\|hide\|close]` | open the local terminal or the selected provider's native interface | everyday |
| `/use` | `ACCOUNT\|add PROVIDER\|remove ACCOUNT` | switch, add or remove accounts | everyday |
| `/model` | `[MODEL]` | choose the model | everyday |
| `/effort` | `[LEVEL]` | choose reasoning effort | everyday |
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

`/model` lists only models reported for the selected account. Choosing a model with multiple effort
levels opens its effort choices next; Enter applies the model and effort together, and Esc keeps
the previous pair. `/effort` opens those choices for the current model, or `/effort high` applies a
reported level directly. The header model and effort controls have dropdown arrows and a hand cursor.
Codex sends the standard `model_reasoning_effort` setting and app-server `effort` field, Claude
uses native cached `--effort` capability (with the provider's supported-value cap), while Cursor and Grok retain the
provider's model-variant IDs. This is a projection of provider catalogs, not a universal effort API.

In `/config` under Models and roles, choose a role to set its model and reported effort for the
current provider account. Worker result delivery is saved directly from its General setting;
no removed worker-return command is advertised. Ctrl+O expands the last collapsed
item in the latest message, while Ctrl+Shift+O expands every item there.

During `/update`, the progress page shows the actual Prepare, Build/Download, Verify, Install and
Restart stages. Warnings stay separate from progress; press D for compiler details and Esc to cancel.

Ctrl+G is the current recovery surface for native agent reports and Concordia worker runs. The
`/delegate` recovery view inspects or retries a selected run; older stop/use/collect command names
are not advertised. Same-provider automatic account handoff is disabled unless the user accepts
the provider-specific terms; manual continuation remains available. Automatic handoff reserves its
daily cap before dispatch.

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
