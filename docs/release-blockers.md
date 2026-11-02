# Release readiness, 2026-11-02

Concordia is a working private Windows build. It is not ready to advertise as a proven,
complete context manager/orchestrator. A limited public preview has a smaller product scope,
but still needs the concrete gates below. This document does not authorize publication.

## Confirmed gate failures

| Gate | Current evidence | Needed before release |
| --- | --- | --- |
| Dependency advisories | `cargo deny check advisories` fails on bincode 1.3.3 and yaml-rust 0.4.5 through syntect; both are unmaintained advisories. | Replace/update the affected paths or explicitly review an appropriate policy decision. No ignore was added. |
| Artifact size | The installed 2026-11-02a release artifact is 16.02 MiB; `scripts/smoke-ci.ps1` enforces 6 MiB and fails. | Reduce the artifact or deliberately review the distribution budget against the new features. Do not silently loosen the gate. Re-run on the exact candidate. |
| Privacy preflight | `scripts/verify-oss-gates.ps1` flags functional namespace references in `crates/concordia/Cargo.toml`, `src/harness/adapter.rs`, `scripts/install.ps1` and `scripts/test-install.ps1`. | Finalize public repository/package identifiers and reconcile intentional public metadata with the privacy policy. These hits do not themselves establish leaked credentials. |
| Source snapshot | Extensive modified and untracked source remains local. CI checks out committed source; publishing existing HEAD would omit current implementation. | Review and commit a complete candidate, including new modules/tests, then run gates on that exact snapshot. Preserve unrelated work. |

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

Still open: whole-history indexing/pagination, related-chat retrieval and verified lineage,
Cursor exact native readback, proactive context/compaction policy, durable worker ownership
across restart, task dependencies, file/worktree isolation, worker quota failover, combined
worker usage/cost accounting and native interactive approval parity. General tool/plugin
integration and unified file-open choices also remain unfinished. See the
[feature audit](feature-completion-audit.md), [orchestration](orchestration-and-latency.md)
and [workspace requirements](workspace-panes.md).

No controlled comparison proves best-in-class quality, token savings, compaction quality or live
inference speed. These are claim/evaluation gaps, not a requirement to finish every future feature
for a labelled preview. Demo editing, custom terminal pointers and broader platform support are
separate work; they do not resolve the concrete gate failures above.
