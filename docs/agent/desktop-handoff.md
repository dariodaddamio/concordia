# Desktop handoff — 2026-11-08g

Historical transfer note: the owner cancelled the desktop transition and approved continued work
here. Follow current PLAN.md and verification status; the state below describes its preparation.

The owner is moving work to the desktop. The whole-repository audit and necessary repairs/pruning
are complete in this working tree. The fixes are already implemented; do not start by reapplying
the findings. Preserve all pending work, including changes that preceded the audit.

## Transfer state

At handoff preparation, the branch is `fix/chat-continuity-and-product-review`, with HEAD
`d404778` (`fix: complete provider UI and worker follow-ups`). The audit implementation and current
documentation remain uncommitted, including untracked documentation directories. No push was
performed. Pulling that HEAD alone will not transfer the audited implementation. Transfer the
complete working changes, including additions and deletions, or deliberately commit and push
them before pulling on the desktop. Do not reset or clean either machine's work.

The installed executable is local to this machine; it does not travel with Git. Credentials,
provider homes, native conversation sources and local app state must not be committed or copied
into fixtures. Establish the desktop's own existing provider/account state before running flows.

## What is finished

- Full audit snapshot: 520 repository paths, 505 present files and 15 pending deletions/moves;
  every present file has a review disposition. Production contract reads, selective fixture
  execution, historical-document review and vendor/provenance checks have different stated depth.
- Concrete repairs and pruning are described in [cleanup review](cleanup-review.md) and
  [CHANGELOG](../../CHANGELOG.md). Current build stamp: `2026-11-08g`.
- Windows optimized executable built and installed; checked and installed SHA-256:
  `edc152b0cacf1c5b91e51f8a1dc9935f0f3c031b912bfd0eb7a0a6f1289926d3`.
- All-target compilation, formatting, whitespace, dependency policy, OSS/privacy gate,
  hidden-process gate and module-cycle policy passed. The cycle policy permits 22 named
  starting modules; this is not an acyclic-code claim.
- Final 93 contract checks and 30 selected process/harness/usage/update integration checks
  passed, supplementing earlier focused checks and the attachment-transport integration.
  Initial failures and their passing corrections are retained in local receipts.
- Existing 5,000-row redraw fixture: 125.58 ms/frame before the repair, 9.55 ms/frame in the
  final rerun; its 100 ms limit was unchanged. Temporary profiling was removed.
- Exact release native-window probe: resumed the exact session older than 125 newer sources
  in 1.98 seconds without additional input; preserved question-mark input and drafts through
  F2 cancel/save; persisted the renamed title; preserved source hash; started no provider turn.
  Modal actions were paced for the one-second feed screenshot sampler.
- One live Codex `gpt-5.6-luna` low-effort turn returned OK with feed. This is the live scope,
  not qualification of every provider, approval path or host.

## Evidence and next work

Read [PLAN](../../PLAN.md), [AGENTS](../../AGENTS.md), [cleanup review](cleanup-review.md),
[verification status](../e2e-status.md) and the [backlog](../plans/README.md).
Machine-local raw evidence is ignored under `target/`: `full-audit-verification.json`,
`full-audit-ledger.json`, reviewer coverage reports, focused logs and native/manual probes.
Those files will not arrive through Git. The portable summary is this note and the linked reports;
do not invent raw receipts if they are unavailable on the desktop.

First confirm the desktop has the transferred working changes and stamp. Build there and verify
the relevant flows against its own installation; do not assume this machine's executable hash
or test outcomes establish desktop behavior. Avoid repeatedly running the full suite; follow
the repository's focused verification policy. Do not spend more live turns merely to recreate
this handoff's already recorded evidence.

Remaining limitations are explicit: Windows MSVC was the installed target; Unix vendor repair
was not built. Initial wrapping and large-source fingerprint work remain linear. Protected
unfinished/unsaved/unavailable-source recovery may exceed resting budgets. Cross-process account
quiescence is not proven. Earlier concurrent history-load latency remains recorded. Broader
provider/approval/host qualification, effectiveness claims and publication are future work.
No product direction was selected after audit completion, and no publication is authorized.
