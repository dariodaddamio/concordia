# opencode provider

Concordia launches the official OpenCode CLI as `opencode acp`. OpenCode speaks ACP over newline-delimited JSON on standard input and output. Concordia keeps the native session id and uses ACP `session/load` for an explicit resume when OpenCode advertises `loadSession`.

OpenCode owns authentication, model configuration, permissions and provider credentials. Concordia checks only bounded file existence and safe model fields in the selected configuration. It does not read `auth.json`, API keys or tokens. OpenCode documents `opencode auth login`, `opencode debug paths`, and the Windows configuration locations. If the installed release uses a different data path, set `AO_OPENCODE_HOME` to the directory reported by `opencode debug paths data` before starting Concordia.

The ACP session reports model options. Concordia exposes those reported models in `/model`, `/use`, and the role picker. OpenCode permission requests use the normal ACP allow-once, allow-always and reject choices. OpenCode form questions appear as Concordia questions and return to OpenCode as an ACP elicitation response.

Confirmed official sources:

- [ACP](https://opencode.ai/docs/cli/acp/) confirms `opencode acp`, `session/load`, model options, authentication errors, permissions, and questions.
- [CLI paths and commands](https://opencode.ai/docs/cli/) documents `opencode debug paths`, `opencode auth`, and the Windows CLI distribution.
- [Configuration](https://opencode.ai/docs/config) documents project and system configuration, model selection, and environment substitutions.
- [Permissions](https://opencode.ai/docs/permissions) documents ordered allow, deny and ask rules and Windows path matching.
- [Providers](https://opencode.ai/docs/cli/providers/) documents provider login and the provider-owned credential store.

Concordia has not run a live OpenCode turn in this change. Live verification must cover login/configuration detection, model listing, a permission request, a form question, image input, and exact session resume.
