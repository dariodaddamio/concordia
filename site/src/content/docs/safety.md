---
title: Safety
description: Security boundaries for accounts, approvals, files and external text.
---

## Provider accounts

Concordia runs the unmodified provider CLIs under your own accounts. Login happens through each provider's supported flow. Concordia does not collect, share or transmit provider credentials.

## Approvals

Native provider approvals stay visible to you. Concordia scopes a response to the exact provider connection, run and attempt. It does not approve uncertain tool actions or silently replay them.

## Files and processes

Read-only, Edit and Full modes translate into provider-native sandbox and approval settings. Cancellation targets the process owned by the active turn. File edits remain subject to provider permissions and project policy.

## External text

Grok Bot callback content is untrusted external text. Concordia fences it before storage and does not expose chat history, Brain or general-purpose MCP tools to the callback.

## Privacy boundary

Memory reads are local, bounded and explicit. Native session sources remain in their original locations. Concordia does not add a hosted memory backend or an inference service.
