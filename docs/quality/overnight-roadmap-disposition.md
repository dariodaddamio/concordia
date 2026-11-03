# Roadmap implementation disposition, 2026-11-03s

This is the current implementation map for the completion campaign. The detailed plans retain
their original design baselines; their proposed field names and bounds do not override shipped
contracts below. Fixture, installed, native/live, physical-host and release evidence stay separate.
The final candidate record is in [E2E status](../e2e-status.md).

| Plan area | Implemented contract and evidence | Remaining boundary |
| --- | --- | --- |
| Orchestration D1-D3 | Durable graph, approved dispatch, paused restart/reconciliation and inspection; [dependency delivery](dependency-delivery.md) includes reviewed cross-worktree results/integration receipts. | Uncertain work is inspected, never automatically replayed. |
| Orchestration W1-W3 | [Reservations](workspace-reservations.md), [managed worktree creation/integration/retirement](managed-workspaces.md), exact owner/repository/head/status checks and dependant references. | Reservations are cooperative. Native filesystem confinement is provider policy, not a new Concordia OS sandbox. |
| Routing R1-R3 | [Local evaluator, saved ordered policies and worker/main inspection](routing-policies.md), exact frozen binding and bounded fallback. | Native approvals and unsupported model/auth/capacity signals remain authoritative. |
| Accounting U1-U3 | [Run/attempt accounting](worker-accounting.md), [bounded native-source coverage](native-usage-coverage.md), workspace/outcome/account/home distinctions and separate quota/credit data. | Native snapshots are not combined with overlapping Concordia counters. Unreported costs/request identities and Cursor import remain unknown. |
| Memory M0-M2 | [Goal/source links](goal-source-associations.md), [file facts/explainable ranking](memory-ranking.md), [explicit groups/discovery](related-chat-groups.md), [reviewed goal proposals](goal-proposals.md). | Suggested links are not confirmed relationships; historical tool requests are not proof of edits. |
| Memory M3 | [Versioned synthetic retrieval corpus and collector](memory-evaluation.md). | Model-retention/token-saving experiments need an explicit provider/task budget and have not run. |
| Provider P0-P3 | [Worker interaction ownership/routing](native-worker-input.md), [opt-in Codex native transport and text replies](codex-native-input.md), Cursor ACP text/images/permission/plan decisions. | Current matched Codex protocol is 0.166.0. No universal native/live parity claim. |
| Provider P4 | Claude bridge reports unavailable with the provenance blocker, retaining the existing native adapter. | A model-callable named MCP callback is not authenticated native permission provenance. No complete supported installed-CLI contract is established; internal SDK control messages do not close this gate. |
| Provider P5 | [Exact turn/model/source-bound Codex capacity](capacity-provenance.md) and invalidation; reactive capacity-error recovery remains available. | Verified headless Claude/Cursor current-window signals remain unavailable. No inferred windows or global statusline/hooks. |
| Workspace W1-W4 | Bounded cwd search, [selected replacement/reconciliation](project-replacement.md), [shared file actions and independent editor](file-actions.md). | Save conflicts/uncertainty preserve user buffers and receipts; no rollback/replay. |
| Workspace W5 / input I2 | [Bounded inline color-cell previews](../images-and-diagrams.md) with standard frame redraw and explicit GIF/system-viewer fallback. | Pixel image protocols and visible pointer shape support require exact physical-host evidence. WezTerm 2024 does not establish that support. |
| Input I1/I3 | [Versioned ordered image/text snapshots and explicit pasted-path import](../images-and-diagrams.md). | Ordinary paste remains text until the explicit import action. |
| Input I4 | Unsupported binary document import fails visibly with the draft retained. | No verified native PDF/binary transport is established. No hidden OCR/base64-text conversion is substituted. |
| Input I5 | [Typed structured Codex file outputs](file-output-artifacts.md), source/directory identity, bounded metadata, revision deduplication and explicit current-file actions. | Unsupported provider output formats, remote resources and prose are not promoted to verified outputs. |
| Integration I6 | [F10/MCP catalog](integrations-catalog.md), namespaced bounded lazy discovery; existing Harness connect/verify/disconnect commands and owned-home registration remain the management path. | The catalog does not install an outbound MCP runtime, marketplace or unsupported native plugin API. |
| Validation V1/V2 | Exact-candidate collector, source/hash-bound receipts and existing fake-process/terminal suites. | Automation does not prove a physical terminal or real model's behavior. |
| Distribution D1/D2 | [Signing policy rejection checks and explicit scan/package workflow](../release-blockers.md), locked dependency/notices gates and existing install/update failure checks. | Signing identity, trusted timestamp, external upload/publication and a fresh Windows release rehearsal remain separate owner/release gates. |

The original user-visible additions are implemented in the same candidate: +/− edit cards,
file path/open actions, agent-facing advanced commands, retained worker/native-agent inspection,
quota/credit freshness distinctions, phase-aware activity/tab labels, meaningful tool subjects
and queue-to-composer Down navigation. [Native worker input](native-worker-input.md) records that slice.

## Candidate collector

`scripts/collect-candidate-evidence.ps1 -Exe <candidate> -ExpectedSha256 <sha256> -BuildCommit <commit>`
creates a new immutable JSON record under ignored `target/candidate-evidence`. It verifies the
candidate before launching only `--version`, verifies the declared application's tracked source,
records compiler/OS/PowerShell metadata and rechecks source/hash at completion. It does not
build, install, run a provider, read chat bodies, configure accounts or publish anything.

Optional `-ChecksPath <target/receipts.json>` accepts at most 32 bounded caller-supplied results.
Each has `name`, `scope` (`source`/`binary`), integer `exit_code`, `log`, and either `build_commit`
or `artifact_sha256`. Receipt/log inputs stay under that checkout's target directory, are regular
files and are bounded to 64 KiB/16 MiB. Logs are fingerprinted without copying their contents.
The record labels these as caller-supplied results, not independently executed checks. Missing
receipts/native CLI versions stay missing, and no record claims release qualification.

An application-source match is not a reproducible-build attestation: compiler flags, environment
and packaging can affect bytes. The executable hash remains the exact candidate identity.
