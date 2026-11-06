# Model discovery

Round A uses only local, deterministic discovery. `available` never invokes a provider; `refresh`
is explicit and writes Concordia's state cache. Provider-written caches are read from every account
home for that provider, then the embedded catalog is used as a fallback. Duplicate ids retain the
first source's metadata and priority.

Research performed 2026-11-06 with help/version only:

| Provider | CLI/version | Credential-free listing surface | Local cache/catalog |
| --- | --- | --- | --- |
| Codex | `codex-cli 0.155.1` | `codex --help` has no `models`/`--list-models` surface | `models_cache.json`; embedded catalog fallback |
| Claude Code | `2.1.319 (Claude Code)` | `claude --help` has no model-listing command or option | `cache/model-catalog.json` and `cache/additionalModelOptionsCache.json`; embedded catalog fallback |
| Cursor | `2026.10.22-27dcf5f` | `agent --list-models` is documented and exits after listing; refresh uses it | `cli-config.json` current model; embedded `auto` fallback |

The Cursor list command is the only discovered provider-owned listing command and is run with a
five-second timeout, hidden through Concordia's process wrapper. It is never run by `available`.
Catalog entries are intentionally conservative: the Claude entries preserve the prior shipped
list, Cursor's `auto` is documented by the CLI help, and Codex's `gpt-5`/`o3` are verified model
arguments in the current provider surface but are not a complete live service catalog. Future
refresh output or provider caches supersede them. `opus`, `sonnet`, and `haiku` remain valid typed
Claude aliases but are not catalog claims.
