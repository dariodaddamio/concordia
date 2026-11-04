# Release readiness, 2026-11-03z candidate

Owner direction, 2026-11-04: defer Windows/release work and wait to publish on GitHub.
The repository remains private; no tag, release, upload or visibility change is authorized.
Existing installed builds and their recorded local evidence remain available.

Concordia is a working private Windows build. It is not ready to advertise as a proven,
complete context manager/orchestrator. A limited public preview has a smaller product scope,
but still needs the concrete gates below. This document does not authorize publication.

## CI repairs and candidate requirements

| Gate | Current evidence | Needed before release |
| --- | --- | --- |
| Dependency advisories | Unused Syntect YAML/plist import features are removed. Bincode remains under one reviewed maintenance-advisory exception for embedded assets. | Keep all other advisory checks enabled; remove the exception when the upstream dump format migrates. See [dependency review](quality/dependency-security.md). |
| Artifact size | The 04j baseline measured 22,968,320 bytes. Shared native file windows, tool descriptions/grouping and larger icon resources add roughly 134 KiB without new runtime dependencies. | The reviewed ceiling is 23 MiB for 04k. `smoke-ci.ps1` compares exact bytes; the final measured candidate is recorded in E2E status. |
| Privacy preflight | Public repository/package identifiers are now exempt only as exact identifiers in listed metadata/adapter/installer files. A private-project reference in the implementation ledger was removed. | Continue scanning the remaining contents of those files and every other tracked text file. New identifiers require an explicit review. |
| Source snapshot | Memory/orchestration work is committed; the unsigned release workflow builds precompiled assets and checksum/notices bundles. | CI must pass on the exact committed candidate. Configured workflows do not establish release qualification. |
| Windows antivirus / publisher trust | Defender quarantined the installed 2026-11-02d executable and release dependency executable as `Trojan:Win32/Bearfoos.A!ml` on 2026-11-02. A locked rebuild passed local scans and installed successfully after intelligence update to `1.463.94.0`; it remains unsigned. | The owner selected unsigned distribution on 2026-11-04; no certificate or paid signing account is required. Pass Defender/install/update checks on the exact final artifact, and pursue vendor review for any detected artifact. Public-feed rehearsal remains pending publication. See [Windows signing](windows-signing.md) for the recovered artifact hash and limits. No antivirus exclusion is a release prerequisite. |

The privacy script scans tracked files only; new files also need the final privacy/secrets review.
No credential files were read or copied for this audit. Evidence: ignored
`target/advisories-2026-11-02a.log`, `target/artifact-smoke-2026-11-02a-final.log` and
`target/oss-gates-2026-11-02a.log`.

## Validation and release decisions still open

- **Native providers and physical hosts.** Fixtures and selected installed terminal journeys
  pass, but do not prove live model, image, approval and session compatibility for every CLI
  version. The reported Codex history-write failure still needs an upstream cause or controlled
  reproduction. A read-only diagnostic on Codex 0.165.0 found the existing rollout/state/history
  records and successfully read the exact thread without starting a turn. Saved history is
  readable now; recording/retry is not thereby verified.
- **Real-turn test scope.** `AGENTS.md` requires explicit provider/task scope and a usage budget
  for real model turns. Local implementation and synthetic tests need no further permission.
  No live model turn was run for this audit.
- **Terms and notices.** `provider-terms.md` retains unresolved provider review, including
  automatic same-provider account switching. Historical legal reading is not current clearance.
  Regenerate and check third-party notices for the exact artifact, including the renderer's
  scoped MPL dependencies.
- **Candidate rehearsal.** Run current CI/release workflows on the complete candidate; earlier
  green rehearsals predate it. Verify checksum/install/update from a fresh download. A public
  release feed cannot be certified from local source rebuilds alone.
- **Release choices.** Confirm version and publication; the owner chose unsigned distribution.
  Packaging generates checksums and notices; the public repository and first release remain
  unpublished. Explicit Windows-only
  support is valid for a Windows preview; Linux/macOS are not prerequisites.

Keep the archived pre-release branch and history bundle local. They are not part of a public
candidate. Publishing/tagging/remote visibility changes remain separate explicit actions.

## Full product versus preview

Implemented: recent bounded Codex/Claude source identity/readback, source pointers, interrupted
request recovery, explicit quota/capacity-error recovery, selectable worker models, bounded
supervisor/worker result handback, Markdown/files, native images and local Mermaid.
Image paste and internal editing are no longer unimplemented blockers.

Whole-history indexing/pagination, bounded lexical retrieval and confirmed/observed lineage,
Cursor ACP readback, optional Codex proactive rollover, native compaction controls, durable worker
ownership, bounded worker quota failover, cross-worktree dependencies and unified file actions are
implemented with fixture evidence. Still open: richer goal/file ranking, file/worktree isolation,
external native usage import and native interactive approval parity. Main/worker attempt accounting
has fixture evidence with explicit counter/cost coverage. Claude post-turn capacity is candidate-
qualified with source/settings checks; Cursor proactive capacity remains unknown. Reviewed F10
integration management is implemented; a general outbound MCP/plugin runtime remains intentionally
out of scope. See the
[feature audit](feature-completion-audit.md), [orchestration](orchestration-and-latency.md)
and [workspace requirements](workspace-panes.md).

No controlled comparison proves best-in-class quality, token savings, compaction quality or live
inference speed. These are claim/evaluation gaps, not a requirement to finish every future feature
for a labelled preview. Demo editing, custom terminal pointers and broader platform support are
separate work; they do not resolve the concrete gate failures above.
