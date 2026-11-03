# Provider, host and release validation plan

Current implementation/evidence map: [campaign disposition](../quality/overnight-roadmap-disposition.md).
The baseline below predates the integrated signing policy and candidate collector. Publication,
live-provider and physical-host qualification remain separate gates.

Status: planned, 2026-11-02. Owner: integration/release lane. Baseline: `c6d3b7c`.
Scope: produce evidence for supported behavior and an exact distributable candidate. This plan
does not run real provider turns, provision signing, submit binaries or publish a release.

## Current evidence and limits

The baseline has green PR CI and local source/installed checks: 960 enabled tests, 48 installed
terminal journeys, formatting, clippy, release smoke, everyday smoke, dependency and privacy gates.
The installed artifact hash is recorded in the current E2E status. Fake providers establish
adapter behavior; they do not establish native authentication, model behavior or approval parity.

The committed release workflow builds, checks, generates notices and packages Windows artifacts.
Its manual-dispatch path is a non-publishing rehearsal; tag pushes publish. An earlier successful
rehearsal predates the current feature set and is not candidate evidence. Separate uncommitted
signing/Defender/workflow drafts exist in the maintainer workspace. Preserve that work and review
it on its own branch; those drafts are not implemented in this baseline.

Code and records: `.github/workflows/{ci,release}.yml`, `scripts/smoke-ci.ps1`,
`scripts/smoke-e2e-everyday.ps1`, `scripts/verify-oss-gates.ps1`, `scripts/test-install.ps1`,
`crates/concordia/tests/{install,release_update,tui_journeys}.rs`, `NOTICE`, `about.toml`,
`deny.toml`, `docs/provider-terms.md`, `docs/quality/dependency-security.md`,
`docs/quality/native-thread-recording.md`, `docs/quality/defender-local-investigation.md`.

## V1: exact-candidate evidence record

Create one record for each candidate, containing commit/tree state, compiler/CLI/host versions,
artifact hashes, relevant configuration/capabilities, command exit codes, fixture/live/physical-host
classification and remaining gaps. Record sanitized metadata, never credentials or native chat
bodies. Preserve existing historical records instead of rewriting them as new evidence.

Bind the record to executable bytes before installation and again after installation, scanning,
signing and packaging as appropriate. A signed file has different bytes from its unsigned build;
hashes/checksums for customers must describe the signed packaged artifact. Refuse an unexpected
change rather than silently substituting a new candidate. Keep evidence reads and artifact size bounded.

Acceptance: a record cannot label results from another commit/hash as current; missing evidence
stays visible; source-only, installed, physical-host and live results are separate. The first
independently shippable slice is the candidate-record format and a provider-free collector that
only invokes existing checks and reads version/help/capability metadata. It must not start model turns.

## V2: deterministic repeated journeys and failure matrix

Run the existing fake-provider suite on each integrated source candidate, not on every plan edit.
Extend the acceptance matrix alongside feature slices: normal completion, native approval wait,
cancel during startup/tools, quota before/after partial work, crash/restart, conversation/workspace
change, missing/mutated source, model change and host resize/focus/paste. Record unsupported cases.

For source changes, use the project's native gates:

```text
cargo fmt --all --check
cargo clippy -p concordia --all-targets --locked -- -D warnings
cargo test -p concordia --tests --locked --no-fail-fast
cargo test -p concordia --test tui_journeys --locked --no-fail-fast -- --ignored
cargo build -p concordia --release --locked
cargo deny check
```

Then run release smoke and OSS gates. Install a candidate only through the existing explicit
install operation and run everyday smoke plus installed terminal journeys against its exact path.
Record hash stability. Use synthetic projects and isolated account/state homes. Never use newest
session selection or a destructive clean/reset to make a journey pass. Repeat checks when a new
change or failure justifies it; a green unchanged candidate does not need an unbounded soak.

Acceptance: failures identify a specific state transition/artifact; the candidate leaves drafts,
queues, native sources and unrelated processes intact; cancellation terminates its tracked children.

## V3: live provider and physical-host matrix

Prepare a bounded protocol for each provider/version before asking for execution authorization:
synthetic repository/task, exact account/session binding, chosen model, maximum turns/usage,
per-case timeout/cancellation, expected result and evidence fields. Keep native authentication and
approval UI under provider control. A read-only capability probe does not authorize a prompt.

| Case | Observable evidence |
| --- | --- |
| Fresh turn and exact-session resume | Correct native session identity; no accidental new/foreign session |
| Tool permission allow/deny/cancel | Native request id and run/attempt binding; exact-once response; no approval transfer |
| Image/document input | Capability checked, exact snapshot used, rejection explicit; no silent text conversion |
| Quota and server-capacity failure | Original request retained; attempts bounded; uncertain work not replayed |
| Context rollover/compaction | Verified current-context measurement and policy; original pointers/pins remain readable |
| Interrupt/restart and worker recovery | No duplicate tools/results, native request cleared, explicit reconciliation for uncertainty |
| Native history recording failure | Distinguish readable past source from successful new recording; preserve diagnostics |

For physical Windows hosts, record terminal and OS versions. Exercise text/image paste, Unicode,
focus, wheel/mouse selection, resize, dirty file decisions, viewer/editor launch and child cleanup.
The installed WezTerm 2024 pointer-shape limitation remains explicit until a supported host is
observed. Do not change global host settings or replace the user's terminal to satisfy a claim.

Acceptance is provider/version/host-specific. If authorization is absent, leave the live row
pending and continue local engineering; do not spend subscriptions or simulate live clearance.

## V4: comparative quality and resource measurements

Use the synthetic workload specified in the memory/provider plan to compare exact-source
retrieval, native resume/compaction and fresh handoff. Measure task success, constraint retention,
missed evidence, duplicate side effects and provider-reported usage. Mark missing values unknown.
Only explicitly authorized live runs can establish model recall/compaction quality.

Measure Concordia separately from children: startup, idle memory, first event, active-stream
input latency, multiple file panes, cancellation and retained memory after close. Existing fake
transport timings exclude real provider startup/inference. State sample size, host, bounds and
method. A smaller wrapper delay does not prove lower model latency or token savings.

Acceptance: raw sanitized measurements can reproduce the stated statistic; there is no blanket
best-in-class, token-saving, real-provider speed or compaction-quality claim without evidence.

## D1: signing and dependency/notices integration

Review the separate signing draft before integration. Require a publisher policy, trusted
certificate/signature verification, timestamp verification, failure behavior, exact-hash scanning
and checksum ordering. Keep private keys/identity documents outside the repository. Preserve
Defender and real-time protection; neither exclusions nor a new trusted root clears a release gate.

Do not silently select or provision a paid/cloud signer. Certificate identity, existing signing
capability, billing/service constraints and distribution policy need an explicit owner decision
before operational use. Source review, synthetic rejection checks and workflow design can proceed.
Do not import personal publisher metadata into general fixtures or weaken the privacy scan.

Generate third-party notices from the locked Windows graph, including scoped MPL dependencies.
Re-review the Bincode exception against the actual embedded-only use and retain the 20 MiB byte
limit unless a separate documented size decision changes it. Regenerate notices if dependencies change.

Acceptance: missing/wrong/expired/untrusted signing identity, missing timestamp, changed artifact,
scan failure and missing license text prevent release qualification. Scanner results state exact
hash and intelligence version; they do not promise universal antivirus acceptance. Artifact upload
to an external analysis/submission service needs separate authorization.

## D2: non-publishing rehearsal and release decision

On the exact reviewed candidate, rehearse build, notices, signing policy if enabled, artifact scan,
package-manifest rendering, post-signing checksums, download, install, startup and update/rollback
failure paths on an isolated fresh Windows environment. Verify only the product executable ships,
not fake-provider/harness binaries or native account data. Keep publication conditions separate
from rehearsal. No tag, release, feed, repository visibility or store submission is implied here.

Record outstanding terms/namespace/trademark review from `docs/provider-terms.md` as owner review
tasks; this engineering plan is not legal clearance. Review current provider terms and public
claims before publication, especially automatic same-provider account handoff. No blanket change
to local user opt-in behavior is implied by planning a release policy.

Acceptance: fresh-download checksums and installed bytes match the candidate; update failure
retains the usable prior version; all availability claims match the supported provider/host
matrix. An explicitly scoped Windows preview can omit unfinished future features while listing
its limits. Full-product claims require their feature plans and evidence to be complete.
