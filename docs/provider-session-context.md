# Provider session context

Concordia builds one short standing context from the recorded provider, account, role, registered
tool names and handoff map. It is attached to the session before the user prompt. The text is
tagged so a fallback prompt can remove it from ordinary retrieval context and avoid sending it
twice in one prompt.

| Provider and transport | Durable channel | Resume and compaction behavior |
| --- | --- | --- |
| Claude Code text and native stdio | `--append-system-prompt`, passed on every launch | Claude receives the same context on explicit resume and its native session owns compaction. |
| Codex exec and app-server | per-launch `-c developer_instructions=<TOML string>` | The setting is passed on every explicit thread launch and resume. |
| Cursor ACP | ACP `session/new` or `session/load` `systemPrompt` when advertised | ACP keeps the system prompt across the session. Without the capability, Concordia sends the tagged context in the first prompt and after a reported compaction. |
| Cursor print mode | first prompt | Concordia prepends the context only when the standard print transport is used. |
| Grok Build ACP | ACP `systemPrompt` when advertised | The same fallback applies when the agent does not advertise the system-prompt capability. |
| opencode ACP | ACP `systemPrompt` when advertised | The same fallback applies when the agent does not advertise the system-prompt capability. |

The registered tool names come from Concordia's MCP `tools/list` registry. The current MCP tool is
`agents` for managed Claude and Codex homes. ACP providers also receive the native question bridge
when that transport enables it. Provider-owned subagents keep their provider instructions; the
standing context tells the parent to use Concordia's `agents` tool for Concordia delegation.

## Needs live check

- Claude Code: ask which app and tools it has before and after compaction.
- Codex: ask which app and tools it has before and after compaction.
- Cursor: ask which app and tools it has before and after compaction in ACP and print mode.
- Grok Build: ask which app and tools it has before and after compaction.
- opencode: ask which app and tools it has before and after compaction.
