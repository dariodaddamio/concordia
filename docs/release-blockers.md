# Release readiness, 2026-11-02

Concordia is a working private Windows build. It is not ready to advertise as a proven,
complete context manager/orchestrator. A limited public preview has a smaller product scope,
but still needs the concrete gates below. This document does not authorize publication.

## CI repairs and candidate requirements

| Gate | Current evidence | Needed before release |
| --- | --- | --- |
| Dependency advisories | Unused Syntect YAML/plist import features are removed. Bincode remains under one reviewed maintenance-advisory exception for embedded assets. | Keep all other advisory checks enabled; remove the exception when the upstream dump format migrates. See [dependency review](quality/dependency-security.md). |
| Artifact size | The 2026-11-02i remote release measured 16.56 MiB. The old 6 MiB limit predates native Mermaid, syntax assets, image support and memory orchestration. | The deliberately revised distribution budget is 20 MiB, about 21% headroom above that measured artifact. `smoke-ci.ps1` compares exact bytes and fails above it. This is a distribution decision, not a performance or memory claim. Re-run on the exact candidate. |
| Privacy preflight | Public repository/package identifiers are now exempt only as exact identifiers in listed metadata/adapter/installer files. A private-project reference in the implementation ledger was removed. | Continue scanning the remaining contents of those files and every other tracked text file. New identifiers require an explicit review. |
| Source snapshot | Core memory/orchestration work is committed in the stacked PR; separate signing work remains in another worktree. | CI must pass on the exact committed candidate. Do not include unrelated signing changes or claim them as shipped. |

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
- **Release choices.** Confirm version, public namespace and signing/distribution policy.
  Packaging generates checksums; executable signing is not implemented. Explicit Windows-only
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
ownership and bounded worker quota failover are implemented with fixture evidence. Still open:
richer goal/file ranking, cross-worktree dependencies/integration, file/worktree isolation, combined worker usage/cost
accounting and native interactive approval parity. Claude/Cursor proactive capacity remains unknown.
General tool/plugin integration and unified file-open choices also remain unfinished. See the
[feature audit](feature-completion-audit.md), [orchestration](orchestration-and-latency.md)
and [workspace requirements](workspace-panes.md).

No controlled comparison proves best-in-class quality, token savings, compaction quality or live
inference speed. These are claim/evaluation gaps, not a requirement to finish every future feature
for a labelled preview. Demo editing, custom terminal pointers and broader platform support are
separate work; they do not resolve the concrete gate failures above.
