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
- Source candidate `2026-11-17a`: discover ACP slash commands from the selected session,
  including updates before the first prompt. Empty lists remove provider suggestions, and
  stale shared-command choices cannot invoke a removed command.

## Context and continuity

- Build bounded handoff maps with explicit provider, account, project and session identity.
- Include exact pointers into provider history, structured file and Git state, command results,
  the last error and the previous agent's bounded reply.
- Read user-requested ranges from provider-owned session sources without model summarization.
- Refer to another chat with `@chat` when the command is available.

## Chat and project work

- Open the provider picker directly from Home during onboarding. Home's footer changes with the
  selected row, including when Enter expands a folder or opens a chat.
- Show Concordia and provider update notices.
- Keep projects in the Projects panel and show delegated and provider runs in the Agents panel.
- Keep multiple chats in tabs with background, close and resume behavior.
- In local source candidate `2026-11-25a`, Home skips recent chats that Doctor reports as
  unopenable, and the header account picker respects hidden accounts.
- Show provider questions and approvals in Needs attention without replaying uncertain actions.
- Start question cards with the prompt, show a short session ID alongside it, and keep the full
  ID below when it was shortened.
- Local source candidate `2026-11-18b`: offer steering or queueing text during a Codex app-server
  turn. Confirm against the exact thread/turn before clearing the draft; never retry uncertain input.
- Read Cursor 2026.11.17 usage through its local screen using CSI-u Return, without a model prompt.
- Local source candidate `2026-11-18a`: default Claude to its question-capable stdio adapter,
  preserving explicit off. Declare owned host/question capability for Habena context. Live
  Claude/Codex qualification and installed distribution remain pending.
- Source candidate `2026-11-17b`: expose `ask_user` in default Cursor sessions through a
  session-local plugin. Preserve pending clarifications and partial replies across restart and
  chosen agent handoff; keep the original source visible and native approvals bound to their run.
- Queue completed clarification replies after successful recovery storage, even when another
  question bundle remains open. Keep input editable and show an explicit error if saving fails.
- Run a per-chat PowerShell terminal, preview files with `/open`, and edit through the built-in
  editor.
- Render Markdown, Mermaid diagrams, images and attachment previews.
- Keep failure details expandable under the turn's provider header.
- Open the permissions menu above the chat footer chip with the draft still visible on the left.
  In Settings, the menu opens next to its value and leaves nearby content visible outside the popup.
- Show live provider usage, model and effort controls, taskbar status and Windows notifications.
- Clear covered rows across the chat pane when account, model or effort menus open from the header.
- Open quiet-turn options beside the visible notice, or near the composer when the notice is offscreen.
- Show countdowns for quota resets less than 24 hours away and dates for later resets. For Cursor,
  show API usage as usage-based when on-demand billing is enabled.
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
