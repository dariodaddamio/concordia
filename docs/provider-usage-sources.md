# Provider usage sources

Concordia displays fields only when the selected provider reports them for the selected account.

| Provider | Limit buckets and reset fields | Credits | Source |
| --- | --- | --- | --- |
| Codex | Primary and secondary windows, plus every `rateLimitsByLimitId` bucket. Each bucket uses the provider limit name, model slug or limit ID and its reset timestamp. | `hasCredits`, `unlimited`, `balance` | Account-bound Codex rate-limit events and the native usage RPC. |
| Claude Code | The windows printed by the local `/usage` command. | Not reported by the command. | `claude -p /usage`, read locally without a model turn. |
| Cursor | The windows printed by the local interactive `/usage` screen. | Not reported by the command. | Cursor's native `/usage` screen read through the bounded pseudo-console. |
| Grok Build | No bucket is invented. The current ACP path does not expose usage windows. | Not reported. | No provider usage endpoint is exposed by the current Grok Build integration. |
| opencode | No bucket is invented. The current ACP path does not expose usage windows. | Not reported. | No provider usage endpoint is exposed by the current opencode integration. |

The Home meters and `/usage` picker use the same `UsageSnapshot`. A missing or expired value is
shown as unavailable, not estimated.
