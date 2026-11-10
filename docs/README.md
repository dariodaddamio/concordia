# Concordia documentation

This is the canonical documentation entry point. Read [PLAN.md](../PLAN.md) for the
current product scope and implemented capability summary. The links below separate
current behavior from proposed work and historical evidence; a plan or audit is not
proof that a feature is shipped.

## Current behavior and operating contracts

- [How Concordia works](how-concordia-works.md) — request flow and boundaries.
- [Architecture](architecture.md) — ownership and invariants.
- [Verification status](e2e-status.md) — checked behavior and known limits.
- [Directory map](directory-map.md) — repository ownership and where to look next.
- [Configuration and commands](config.md) — settings and user-facing commands.
- [Provider connections](how-concordia-connects-providers.md) — processes, accounts,
  sessions, approvals and limits.
- [Memory and delegation](how-concordia-remembers-and-delegates.md) — native-source
  retrieval, continuity and workers.
- [Native source pointers](native-memory.md) — current bounded indexing, readback,
  recovery and explicit limitations.
- [Prompt construction](how-concordia-builds-prompts.md) — text, references,
  attachments and MCP inputs.
- [Runtime and security](how-concordia-runtime.md) — state, integrations, usage,
  UI and provider boundaries.
- [Workspace and input](workspace-panes.md) — file panes, previews and editing.
- [Images and diagrams](images-and-diagrams.md) — supported attachment paths.
- [Provider terms](provider-terms.md) — credential and provider ownership rules.

## Proposed or incomplete work

These documents describe targets, acceptance criteria or remaining work. They must
not be cited as current implementation status:

- [Canonical backlog](plans/README.md), with links to detailed acceptance references.
- [Context handoff plan](plans/context-handoff-plan.md).
- [Feature completion audit](feature-completion-audit.md).
- [Release blockers](release-blockers.md).
- [Overnight roadmap disposition](quality/overnight-roadmap-disposition.md).

## Evidence, audits and history

Superseded strategy and campaign records, including the old production plan, live in
the [documentation archive](archive/README.md).

The `quality/` directory contains scoped audits, fixture or installed-build
qualification, and dated research. Start with [e2e-status](e2e-status.md); read
[the historical verification journal](quality/e2e-history.md) only when a dated
record is needed. Documents named `*-audit.md`, campaign/wave records, and dated
investigations are evidence with explicit scope, not general capability claims.

## Contributor and task records

- [Contributing](../CONTRIBUTING.md) — development rules.
- [Contributor internals](contributing-internals.md) — repository-facing notes.
- `agent/` — local task scopes and review records; preserve as historical working
  evidence unless a task explicitly reconciles them.
- [Changelog](../CHANGELOG.md) — historical build changes.

When adding documentation, update this index and label the document at its top as
current behavior, proposed work, or historical/scoped evidence. Keep one canonical
page per topic and link older plans to the current source of truth rather than
duplicating implementation claims.
