---
title: Configuration
description: Configure projects, providers and trust behavior.
---

Concordia keeps configuration in its local Windows state directory. Provider authentication remains in provider-owned homes. Concordia does not copy provider tokens into its own configuration.

## Project policy

Project policy controls the working directory, trust mode and locked-code checks. A verification receipt must identify the provider, home and policy revision before a write-capable turn can proceed under a locked-code policy.

## Provider accounts

Each account has an explicit provider and home identity. Account display names do not select credentials. A provider's own login flow creates or updates its home.

## Agent roles

`/config` lists the saved agent roles. Each role can name a provider, account, model and reasoning
effort. The starting roles are `orchestrator`, `implementer`, `reviewer`, `re-reviewer` and
`general`. The `agents` tool accepts `role`; an explicit provider or model overrides that role's
default. Select a role in Settings to open the model picker for the current provider account;
choosing a model then saves its provider-reported effort with the role. Role records are stored
with Concordia's local orchestrator settings.

General settings also control whether worker results return automatically. This preference is
stored directly in the orchestrator settings; no removed worker-return command is used.

## Continuation

Concordia asks before changing providers after usage exhaustion. Same-provider automatic handoff is separately opt-in, bounded and tied to recorded provider terms acceptance. It never fans work across accounts to multiply throughput.

## Grok Bot callback

The callback listener starts only when a routine is configured and binds to `127.0.0.1`. Its bearer token and webhook keys use Windows Credential Manager. The listener exposes one callback tool and accepts only run IDs Concordia started.
