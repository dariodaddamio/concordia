# Delegation

Agents initiate delegation through the always-on MCP `agents` tool. A parent supplies the task and
may supply a role. Roles are stored in Concordia's orchestrator settings and contain a default
provider, account, model and reasoning effort. An explicit provider or model in the request wins.
The shipped roles are `orchestrator`, `implementer`, `reviewer`, `re-reviewer` and `general`. Each
role can use any connected provider, including opencode and a Claude Code base-URL profile.
Each child inherits the parent's permission mode and keeps an explicit parent identity.

The Agents view (Ctrl+G) groups children beneath the chat or agent that started them. Compact rows
show a themed status icon, task title and elapsed time; parent rows include child status counts.
Use left/right on a selected parent to collapse or expand its children. Enter opens the existing
activity detail, where provider, model, account and recent activity remain available. Provider
runs started by an agent's structured shell call are displayed as untracked subagents; their rows
say that they cannot receive messages or questions and do not offer messaging controls.

Former slash-command delegation entry points are not available. Recovery help currently advertises
Ctrl+G and `/delegate` recovery only; removed stop/use/collect-style commands are not documented.
Each proposal is bound to its exact parent conversation, canonical workspace and parent run. The
worker record is validated and bound before launch; completion must match the exact agent/run IDs.
Invalid IDs and records are rejected with bounded reads.

When a provider reports a usage limit, the chat offers an inline account picker. Same-provider
automatic cross-account handoff is blocked unless the user has accepted that provider's terms and
enabled the setting; manual continuation remains possible. The daily automatic-handoff cap is
reserved atomically before dispatch and released if dispatch does not proceed. Concordia does not
apply an automatic routing policy; native provider approvals and authentication remain
provider-owned.
