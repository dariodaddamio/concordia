# Active dispatch

| Plan | Slice | Owner/model | Status | Next action |
| --- | --- | --- | --- | --- |
| [Audit remediation](audit-remediation.md) | Qualification | Codex and independent reviewers | open | Finish focused checks and record evidence |

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
