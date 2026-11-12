# Concordia

Concordia is a Windows-first native app that runs the official Codex, Claude Code, Cursor and Grok Build
CLIs under your own accounts, with bounded handoff between them. It keeps one conversation
navigable while each provider remains responsible for its own sessions, approvals and tools.

For the short view of what works, what remains unproven and the intended direction, read the
[project plan](PLAN.md). The [documentation index](docs/README.md) separates current references,
planned work and historical evidence. Concordia is currently private and pre-release.

Concordia is independent and unofficial. It is not affiliated with or endorsed by OpenAI,
Anthropic, Anysphere, Cursor or xAI. It runs their official, unmodified CLIs using your own accounts.
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
/use                 # choose or connect Codex, Claude Code, Cursor or Grok Build
Ask your question and press Enter
/usage               # inspect provider-reported usage
/resume              # continue a recorded conversation
/model               # choose a model from the active provider
```

`concordia` opens the native app window. Inside it, `/terminal`
or F11 opens a local shell in the project; F11 returns to chat with its draft intact.
`/terminal hide` keeps that shell alive, and `/terminal close` stops it.

The Windows taskbar shows a theme-coloured overlay dot while work runs or user input is waiting;
the waiting state takes precedence and idle clears the dot. If background work needs input while
the window is unfocused, Windows shows a notification. Selecting it focuses the existing window
and opens Needs attention; the window does not reopen itself.

<!-- COMMANDS:START -->
| Command | Arguments | Description | View |
| --- | --- | --- | --- |
| `/terminal` | `[open\|hide\|close]` | open the local PowerShell terminal | everyday |
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
| `/open` | `[PATH]` | preview files mentioned in this chat | everyday |
| `/copy` | `` | copy the last reply | everyday |
| `/doctor` | `` | check your setup | everyday |
| `/update` | `` | update Concordia | everyday |
| `/exit` | `` | exit Concordia | everyday |
<!-- COMMANDS:END -->

`/skills` and `/hooks` show the selected account's local sources. Use `/harness connect PATH`
to apply a declared setup across registered accounts;

The slash picker also lists the active provider's native commands. Concordia owns cross-provider,
account and session commands; native commands unavailable in the provider's programmatic mode are
shown disabled, and any other slash name is passed to the active provider. See the [command
ownership table](docs/command-audit.md#slash-command-ownership).
later accounts inherit that connection in this project before their first turn. Use `--current`
for an account-specific setup. `/skills install [PATH]` links a skill folder or collection into provider homes; the default
source is `~/.agents/skills`. Later accounts inherit shared skill sources across projects.
Links keep one current source, and existing same-named skills are preserved.
`/skills remove PATH` stops sharing and removes matching directory links across accounts;
source files and other skill directories are preserved.
`/harness skill` installs Concordia's connection instructions.
`/harness status` shows shared sources and per-account connection results; `/harness verify`
runs declared verification probes and `/harness tools` lists and test-runs connected tools.
Native hook consent must be reviewed in each provider's own client. A connected configuration
does not prove that hooks are trusted, active or able to protect every source language.

Use `/doctor` inside Concordia, or `concordia doctor` in a terminal, when setup or provider
discovery needs diagnosis.

## Safety at a glance

- Plan, Ask and Edit modes limit what a provider may do. Full access requires explicit
  `/mode full` confirmation.
- Delegated agents never exceed the mode selected for their parent conversation.
- Concordia itself makes no LLM calls; memory and handoff are deterministic local logic.
- Nothing leaves your machine through Concordia. Network traffic comes from the provider CLIs'
  own supported connections (and any explicitly requested update or integration).
- Native provider authentication, approvals and session state remain authoritative. See
  [provider terms and boundaries](docs/provider-terms.md).

## Learn more

- [How Concordia works, end to end](docs/how-concordia-works.md)
- [Current capabilities and limits](PLAN.md)
- [Architecture and data boundaries](docs/architecture.md)
- [Deterministic handoff map layout](docs/continuity-map-v2.md)
- [Verified behavior and remaining limits](docs/e2e-status.md)
- [Contributing](CONTRIBUTING.md)
- [License](LICENSE)

The walkthrough links the detailed provider, memory, input and configuration references.
