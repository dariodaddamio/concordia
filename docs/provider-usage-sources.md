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

## Structured usage-limit signals

The inline continuation path accepts only these provider-owned structured signals. Limit wording
without one of these fields does not open the continuation prompt.

| Provider | Structured signal |
| --- | --- |
| Claude Code stream-json | `error.type = rate_limit_error` |
| Codex app-server | `codexErrorInfo.UsageLimitExceeded`, `error.code = usage_limit_reached` |
| Cursor ACP | `error.code = rate_limit_exceeded` or `quota_exceeded` |
| Grok Build ACP | `error.code = rate_limit_exceeded` or `quota_exceeded` |
| opencode | `error.type = rate_limit` or `quota_exceeded` |

The adapter preserves the structured code in its terminal error. Reset times and remaining usage
come from the provider usage snapshot used by `/usage`; Concordia does not infer them.
