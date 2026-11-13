---
title: Features
description: Shipped Concordia features and the boundary of the current build.
---

This page lists behavior shipped in the current build. It separates that behavior from the
intended direction so the site does not promise unfinished work.

## Provider coordination

- Run Claude Code, Codex, Cursor, Grok Build and opencode through their supported interfaces.
- Configure explicit provider, account, model and effort defaults for delegation roles.
- Delegate work through the `agents` MCP tool and inspect provider-native and delegated runs in
  the Agents view.
- Continue a task on another configured account after a structured provider usage-limit signal.
  The default policy asks before handoff and role-configured fallback models are tried first.
- Keep provider authentication, native sessions, tools, approvals and model catalogs owned by
  each provider.

## Context and continuity

- Build bounded handoff maps with explicit provider, account, project and session identity.
- Include exact pointers into provider history, structured file and Git state, command results,
  the last error and the previous agent's bounded reply.
- Read user-requested ranges from provider-owned session sources without model summarization.
- Refer to another chat with `@chat` when the command is available.

## Chat and project work

- Guide provider setup during onboarding and show Concordia and provider update notices.
- Keep projects in the Projects panel and show delegated and provider runs in the Agents panel.
- Keep multiple chats in tabs with background, close and resume behavior.
- Show provider questions and approvals in Needs attention without replaying uncertain actions.
- Run a per-chat PowerShell terminal, preview files with `/open`, and edit through the built-in
  editor.
- Render Markdown, Mermaid diagrams, images and attachment previews.
- Show live provider usage, model and effort controls, taskbar status and Windows notifications.
- Support `@chat` references and the `/model` command for active provider models.
- Connect local Harness and memory extensions through `/harness`, `/skills` and `/hooks`.

## Safety and release integrity

- Run provider CLIs under the user's accounts with explicit account and session bindings.
- Keep Read-only, Plan, Ask, Edit and Full behavior visible at the Concordia boundary while
  provider-native approval behavior remains authoritative.
- Keep memory reads local, bounded and explicit. Do not use a hosted memory backend or resident
  inference model.
- Publish SHA-256 checksums and GitHub Actions build-provenance attestations for release assets.
- Support optional Grok Bot routines through a Credential Manager-backed webhook and a localhost
  callback that accepts only bounded results for runs Concordia started.

## Intended direction

The following items are direction, not shipped features: fully dockable per-chat layouts,
embedding the user's editor, Asset Lab, browser-to-chat feedback, custom tool panels, shared named
terminals, `/skills` editing and propagation, multiple projects in one chat, and provider-history
deletion.
