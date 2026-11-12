# Delegation

Agents initiate delegation through the always-on MCP `agents` tool. A parent supplies the task and
connected account, provider and model. Each child inherits the parent's permission mode and keeps
an explicit parent identity. Concordia does not choose a model through a routing table.

The Agents view (Ctrl+G) groups children beneath the chat or agent that started them. Compact rows
show a themed status icon, task title and elapsed time; parent rows include child status counts.
Use left/right on a selected parent to collapse or expand its children. Enter opens the existing
activity detail, where provider, model, account and recent activity remain available. Provider
runs started by an agent's structured shell call are displayed as untracked subagents; their rows
say that they cannot receive messages or questions and do not offer messaging controls.

The old `/delegate`, `/workers`, `/crew`, `/worker-panel` and `/scope` entry points are removed.
When a provider reports a usage limit, the chat offers an inline account picker. It does not apply
a Concordia routing policy. Native provider approvals and authentication remain provider-owned.
