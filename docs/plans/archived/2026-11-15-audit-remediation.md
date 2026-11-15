---
title: Concordia audit remediation
slug: audit-remediation
status: completed
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
| Lifecycle and effort | done | Luna medium, root integration | provider and TUI flow | native approval policy | Existing queue/continuation/provider checks |
| Qualification | done | Root; independent review seats | source gates and canonical docs | installed binary and remote publication | [Verification](../../e2e-status.md) |

## Decisions and boundaries

Canonical agent records are a leaf module; MCP and the UI consume them. Worker identity is bound and saved before launch. Native history parsing is below CLI discovery. The cycle check is a static source-reference approximation with exact historical edges, explicit extraction migrations and documented macro/cfg limits.

Per-exchange peer I/O bounds address reproduced stalls/oversized frames. Tests use synthetic state; no actual credential values are read or copied. Claude live qualification stays pending while the user's quota is exhausted. One tiny Codex low-effort turn is within project authorization.

## Worktree disposition

| Branch | Owner | Disposition |
| --- | --- | --- |
| fix/audit-remediation | Root | Integrated repair branch |
| fix/audit-mcp | MCP worker | Selected commits integrated; incomplete transport prototypes superseded by root; retained for inspection |
| fix/audit-lifecycle | Lifecycle worker | Selected commits integrated; root completed unresolved checks; retained with copied build context uncommitted |
| fix/audit-providers | Provider and documentation writers, sequentially | Provider commits integrated; documentation commit integrated; retained for inspection |
| fix-everyday-1 | Existing unrelated work | Untouched; not owned by this task |

Worktrees are retained deliberately; no cleanup of unowned work is authorized or needed.

## Completion checklist

- [x] Repair and verify the reported transitions
- [x] Record native provider and deterministic check outcomes
- [x] Resolve independent process, architecture, UI/logic and security findings
- [x] Update canonical behavior documents and build identity
- [x] Archive this plan and remove its dispatch row

## Observed proof

The 125 focused library checks and three process/window checks passed. Two native Codex turns
(standard and app-server, `gpt-5.6-luna`, low effort) returned `OK`; Claude model turns: zero.
Fresh synthetic boundary probes cover enablement/project policy, traversal, oversized framing,
initialized notification, wrong response IDs, quiet-child cleanup, HTTP session headers and a
matching chunked SSE response. Fresh library probes cover exact completion, retained foreign
proposals, invalid config preservation, four concurrent registry writers, secret-free launch
projections, consent/cap admission and fingerprint-free redaction.

Architecture review: pass; canonical record/history/text owners and exact historical static
ownership gate, with macro/cfg/path-attribute limits documented.
UI/logic review: pass; previous context, approval-attention and model-picker failures rerun green.
Security review: pass; exact binding before launch, constrained secret delivery, bounded records
and independent process probes. Process review: pass; durable dispatch, scoped worktree disposition, canonical behavior docs, observed proof and all six source gates independently confirmed.

Source formatting normalizes pre-existing style in the crate so the configured formatting gate
passes. This is not presented as additional behavior work. Existing warnings remain visible.
Canonical verification and limits are in [Verification](../../e2e-status.md).

## This slice

### Audit remediation - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| crates/concordia/src/mcp.rs, crates/concordia/src/mcp_registry.rs, crates/concordia/src/mcp/ | ok: 1, 2, 3, 4, 5, 6, 7, 9, 10; bounded peer I/O: 8 | One authoritative MCP session, policy and record boundary; replace the proven permissive/broken paths. Bounds address observed external-peer failures. |
| crates/concordia/src/tui/, crates/concordia/src/delegate*.rs | ok: 1, 2, 3, 4, 5, 6, 7, 9, 10 | Retain exact conversation/run ownership; repair existing queue, approval, handoff, terminal and completion transitions without another application state store. |
| crates/concordia/src/provider/, crates/concordia/src/models.rs, crates/concordia/src/catalog.rs | ok: 1, 2, 3, 4, 5, 6, 7, 9, 10 | Carry selected effort and typed error meaning through existing provider interfaces. Provider capability evidence owns supported values. |
| crates/concordia/src/config.rs, crates/concordia/src/redact.rs, crates/concordia/src/state_io.rs, crates/concordia/src/store.rs | ok: 1, 2, 3, 4, 5, 6, 7, 9, 10 | Reuse checked storage mutation and remove predictable redaction fingerprints; preserve native authentication and unrelated settings. |
| scripts/check-module-cycles.ps1, scripts/module-cycles-allowlist.txt, scripts/lib/ | ok: 1, 3, 4, 5, 7, 9, 10 | Correct production-source discovery and report exact cycles; avoid accepting new cycles solely to pass the gate. |
| crates/concordia/src/commands.rs, crates/concordia/src/main.rs, crates/concordia/src/version.rs | ok: 1, 4, 5, 6, 10 | Restore reachable recovery controls and truthful command help; record the integrated build identity. |
| README.md, CHANGELOG.md, PLAN.md, docs/tui-states.md, docs/e2e-status.md, docs/quality/duplication-audit.md, docs/config.md, docs/delegation.md, docs/mcp.md | ok: 1, 4, 5, 6, 10 | Update existing behavior documents with actual implementation and checks; do not turn audit inventory into claimed proof. |

All ten tendencies were considered. No reusable policy exemption is created. No extra layers, silent defaults or compatibility branches are justified merely by the audit; each change must close a documented failure. Existing checks and bounded external probes own verification. New unit/integration cases require a specific owner request under AGENTS.md.

| crates/concordia/src/agent_records.rs, crates/concordia/src/native_history.rs, crates/concordia/src/text_safe.rs, crates/concordia/src/lib.rs | ok: 1, 4, 5, 6, 7, 9, 10 | Move existing responsibilities into leaf owners; retain public compatibility re-exports. |
| crates/concordia/tests/, crates/concordia/src/usage.rs, crates/concordia/src/chat_cli.rs | ok: 1, 5, 9, 10 | Update existing request constructors and synthetic fixtures for actual catalogs, usage and valid run identity; no new test cases. |
| crates/concordia/src/ | ok: 1, 4, 10 | Cargo formatting also normalizes pre-existing Rust formatting needed by the configured gate; no additional behavior is introduced. |
| docs/plans/audit-remediation.md, docs/e2e-status.md, docs/quality/duplication-audit.md, docs/directory-map.md | ok: 1, 4, 6, 10 | Keep dispatch separate from durable outcomes and record exact evidence with qualification limits. |
| scripts/verify-oss-gates.ps1 | ok: 1, 3, 10 | Recognize the existing public documentation URL and public brand CLI in two exact files; keep path and other-token scanning active. |
