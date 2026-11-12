# Grok Build provider

Concordia launches xAI's local `grok` CLI through its Agent Client Protocol (ACP). The regular
test suite never launches the real CLI; provider-facing behavior uses the existing synthetic ACP
fixture.

## Confirmed interface

The official [Grok Build overview](https://docs.x.ai/build/overview) documents interactive,
headless and ACP entry points, and names the Windows user configuration location as
`%USERPROFILE%\\.grok\\config.toml`. [Settings](https://docs.x.ai/build/settings) documents
`GROK_HOME`, which Concordia uses for an explicitly selected account home.

The official [ACP guide](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/15-agent-mode.md)
confirms `grok agent stdio`, JSON-RPC ACP, `initialize`, `session/new`, `session/load`,
`session/prompt`, streamed `session/update` notifications, tool-call updates and permission
requests. Its [session guide](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/17-sessions.md)
confirms explicit session-id loading and `session/set_config_option` for model selection.
`session/new` accepts `cwd` and `mcpServers`.

The official [headless guide](https://docs.x.ai/build/cli/headless-scripting) shows auth-method
discovery and `authenticate` with `cached_token` after `grok login`; Concordia sends that ACP
request when advertised. It currently checks only metadata-file existence (`config.toml` plus
`auth.json`) and never reads or copies their contents. The exact cached-token filename is not
treated as a contract.

The [CLI reference](https://docs.x.ai/build/cli/reference) confirms `grok login`, `grok models`,
`grok sessions`, `grok inspect`, `grok mcp`, `grok export`, `grok update` and `grok agent stdio`.
Concordia uses native login, reads a configured default model for its bounded catalog, and reports
usage unavailable: no supported read-only CLI/ACP usage signal was verified. The [enterprise
guide](https://docs.x.ai/build/enterprise) confirms provider-owned authentication methods and
inference/auth boundaries.

## Concordia boundary

| Capability | Grok Build behavior | Concordia boundary |
| --- | --- | --- |
| Account home | `%USERPROFILE%\\.grok` or `GROK_HOME` | Explicit account/home identity; no credential copying |
| New/resumed turn | ACP `session/new` or `session/load` | Resume requires the stored exact native session id |
| Questions/permissions | Native ACP requests where advertised | Existing native interaction UI; MCP bridge remains available |
| MCP | `mcpServers` in ACP session creation | Only Concordia's question bridge is injected when needed |
| Models | `grok models` is documented; config default is cataloged | No invented model list when metadata is absent |
| Usage | No supported CLI/ACP usage signal verified | Explicitly unavailable |
| Native history | Persistence is documented; reader schema is not | Resume by id works; history indexing is not claimed |

This page records W10 research. The real `grok` CLI remains unverified in this worktree by policy.
