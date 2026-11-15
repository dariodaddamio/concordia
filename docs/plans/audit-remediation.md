---
title: Concordia audit remediation
slug: audit-remediation
status: active
objective: Repair the two audit passes and carry selected effort to native providers.
stop_condition: Focused checks, bounded runtime probes and independent review pass with live limits recorded.
settled_decisions: Keep native authentication and approvals; no Claude inference while quota is exhausted.
scope: F1-F12, G1-G15 and effort transport; preserve account data and unrelated work.
dependencies: Native Cargo toolchain and installed provider CLIs; no package tooling or hosted services.
slices: Implementation and qualification rows below.
assigned: Coordinating Codex with scoped Luna medium workers and independent read-only reviewers.
proof: Existing focused checks, isolated MCP probes, one tiny Codex flow, source gates and board findings.
completion_checklist: Record checks, resolve board failures, update behavior docs and archive this plan.
owned_paths: crates/concordia/src/, crates/concordia/tests/, scripts/, README.md, CHANGELOG.md, docs/
forbidden_paths: Credentials, provider histories, global hooks, unrelated worktrees, installed executables.
---

# Concordia audit remediation

Baseline: `a35932b`. The user requested parallel fixes for both audit passes. This work uses native collaboration tools and isolated writer worktrees. No Claude inference, installation, authentication migration or publication is part of the repair.

## Slices

| Slice | Status | Owner/model | Owned paths | Forbidden paths | Proof/report |
| --- | --- | --- | --- | --- | --- |
| MCP boundaries | done | Luna medium, root integration | MCP registry/records/transports | provider credentials | Independent bounded synthetic probes |
| Lifecycle and effort | open | Luna medium, root integration | provider and TUI flow | native approval policy | Existing queue/continuation/provider checks |
| Qualification | open | Root; independent review seats | source gates and canonical docs | installed binary and remote publication | [Verification](../e2e-status.md) |

## Decisions and boundaries

Canonical agent records are a leaf module; MCP and the UI consume them. Worker identity is bound and saved before launch. Native history parsing is below CLI discovery. The cycle check is a static source-reference approximation with exact historical edges, explicit extraction migrations and documented macro/cfg limits.

Per-exchange peer I/O bounds address reproduced stalls/oversized frames. Tests use synthetic state; no actual credential values are read or copied. Claude live qualification stays pending while the user's quota is exhausted. One tiny Codex low-effort turn is within project authorization.

## Worktree disposition

| Branch | Owner | Disposition |
| --- | --- | --- |
| fix/audit-remediation | Root | Integrated repair branch |
| fix/audit-mcp | MCP worker | Selected commits integrated; incomplete transport prototypes superseded by root; retained for inspection |
| fix/audit-lifecycle | Lifecycle worker | Selected commits integrated; root completed unresolved checks; retained with copied build context uncommitted |
| fix/audit-providers | Provider and documentation writers, sequentially | Provider commits integrated; docs integration pending; retained for inspection |
| fix-everyday-1 | Existing unrelated work | Untouched; not owned by this task |

Worktrees are retained deliberately; no cleanup of unowned work is authorized or needed.

## Completion checklist

- [ ] Repair and verify the reported transitions
- [ ] Record native provider and deterministic check outcomes
- [ ] Resolve independent process, architecture, UI/logic and security findings
- [ ] Update canonical behavior documents and build identity
- [ ] Archive this plan and remove its dispatch row
