---
title: Harness and memory
description: Understand the local extension points and bounded source retrieval.
---

## Harness

The harness extension point connects project policy and checks to a provider turn. A project can lower a write-capable request to Read-only when its locked-code policy lacks a current verification receipt. Concordia checks the final provider arguments before launch.

Harness checks are local. Concordia does not add a hosted control plane or a resident inference model.

## Memory

Concordia keeps local conversation events and can index metadata for user-owned provider session sources. Native source files stay provider-owned and are not rewritten. A memory pointer identifies a source and bounded byte range; Concordia reads the range only when the user asks for context.

The context pack has a character budget and shows an estimate before attachment. Concordia does not call a model to summarize it. Credential files are excluded.

## Handoffs

A handoff carries explicit provider, account, project and session identity. The receiving provider gets a bounded context pack only after the user starts or confirms the turn. Ambiguous session shortcuts are rejected.
