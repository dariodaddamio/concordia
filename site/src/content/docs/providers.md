---
title: Providers
description: Provider boundaries and current integrations.
---

Concordia starts official provider tools under your accounts. Provider tools own login, native sessions, tools, model catalogs and native approval behavior. Concordia stores explicit provider and account identities and passes only the context needed for a user-started turn.

| Provider | Transport | Current boundary |
| --- | --- | --- |
| Claude Code | Agent SDK stream-json | Native login and session behavior remain with Claude Code. |
| Codex | App Server or standard CLI | Native thread identity and approvals remain with Codex. |
| Cursor | Agent Client Protocol or standard CLI | ACP capabilities come from the active session. |
| Grok Build | Agent Client Protocol | `grok agent stdio` provides session and permission messages. |
| opencode | Agent Client Protocol | ACP capabilities and model metadata come from the active session. |
| Grok Bot | Webhook and localhost callback | Concordia starts a registered routine and accepts one bounded callback. |

## Claude Code

Concordia launches the unmodified Claude Code executable. Sign in through Claude's own flow. Native session IDs are required for resume. Provider commands that Claude advertises pass through its stream-json input.

## Codex

Concordia uses Codex app-server when the installed version supports the required protocol. It binds turns to the explicit thread ID and maps native questions and approvals into the existing interaction UI.

## Cursor

Concordia uses Cursor's ACP session when available. Cursor commands are enabled only when the active session advertises them. A standard CLI path remains available where the installed version supports it.

## Grok Build

Concordia launches `grok agent stdio` through ACP. The account home is explicit, and login checks inspect metadata without copying credentials. Usage is shown as unavailable when no supported signal exists.

## opencode

Concordia launches opencode through its ACP transport. Provider-run records remain visible in
the Agents view, and provider model and usage metadata stays unknown when opencode does not
report it.

## Grok Bot

Grok Bot is a cloud assistant rather than a local CLI. Concordia posts a task to a registered routine webhook. A user-run tunnel can expose the localhost callback. The callback accepts only a Concordia-generated run ID, a bounded payload and a bearer token.
