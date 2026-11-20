---
title: Quick start
description: Open a project, connect a provider and send a first turn.
---

## 1. Open a project

Start `concordia` in a project directory. Concordia uses that directory as the project context for the chat.

## 2. Connect a provider

Open the provider setup flow and complete login in the provider's own CLI. Concordia runs the installed CLI without modifying it. It keeps provider homes separate when you choose multiple accounts.

## 3. Choose the trust mode

Use Read-only when a turn should inspect files. Use Edit when the provider can change files under the project policy. Native approval requests remain visible to you. Full access is a provider option with greater risk and should be chosen deliberately.

## 4. Send a turn

Type a prompt and press Enter. Use `@chat` to refer to another chat when the command is available. Use `/model` to choose a model exposed by the active provider.

## 5. Handle a question

Concordia pauses for provider questions and approvals. Review the native request, answer it, or cancel the turn. A cancelled or uncertain tool action is not replayed automatically.

## 6. Resume by identity

Concordia resumes a saved session only when the provider, account home and explicit native session ID match. It does not use a last-session shortcut.
