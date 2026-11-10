# Cleanup and product review

The owner requested an in-depth audit with three scoped reviewers, followed by concrete repairs
and necessary simplification before discussing product direction. Earlier audit/release narratives
are preserved in the [dated journal](../quality/e2e-history.md#cleanup-audit-record-through-2026-11-08e).
Current scope is in [PLAN.md](../../PLAN.md); unimplemented work has one [backlog](../plans/README.md).
The subsequently approved restart/provider/worker work and cleanup disposition are recorded in
[08h follow-through](continuity-follow-through.md).

## Cleanup follow-through, 2026-11-09a

The owner postponed step 7 (product direction) and renewed the full cleanup request. Source
pruning removes stale execution fallbacks: silent provider-flag downgrades, unstructured-output
rescue, unbound chat startup and shipped/cross-account model fallback. Context maps and session
picker rows share their current owners; the obsolete reference writer, automatic pre-rename state
adoption, startup theme migration and old logging alias are retired. Original user data remains
preserved; retaining records does not retain obsolete execution or compatibility behavior.
Explicit data-conversion tools remain separate from automatic adoption.

Cleanup completed through bounded, normal Cargo and Git operations: three integrated worktrees
and their remaining local branches were retired. Duplicate generated builds accounted for
29,412,145,663 logical bytes; total checked-out worktrees accounted for 29,436,757,242 bytes.
The redundant recovery bundle/sidecars and independently identified inactive 04n executable were
also removed, bringing this retirement pass to **29,464,442,588 logical bytes (29.46 GB)**.
Every archived commit remained reachable through existing main/origin refs before the redundant
bundle was removed. Earlier raw recursive requests were rejected before execution; the later
canonical cleanup succeeded without an override. No additional backup was created.

The installer separately removed older verified builds; current installation, the live rollback
image, native histories, credentials, drafts, queues and unique pending work were preserved.
Current package caches and distinct-flow evidence remain through qualification. Exact actions
and attribution are in the local `target/roadmap-cleanup.json` receipt and the portable
[worktree retirement](worktree-retirement.md) summary.

Final **09a is built, verified and installed**. The current [verification status](../e2e-status.md)
records 155 scoped deterministic checks, two live default-transport replies, both exact-binary
checkpoint restart probes and the older-session/draft/title probe. Installed SHA-256 matches the
checked optimized executable. The existing user window remains open on its retained prior image.

The main target contains **7,747,775,516 logical bytes** at the final cleanup inventory. Full and
package-specific Cargo clean both refused its missing/invalid CACHEDIR.TAG. Automatic approval
review separately rejected validated direct recursive removal as blocked by policy; no deletion
occurred there, and no cache marker was fabricated to bypass that guard. This remaining cleanup
is incomplete. Current source changes remain local and uncommitted; no GitHub publication occurred.
The owner subsequently removed that target tree; its absence was verified at the start of 09b.
Its ignored raw logs/probes were temporary artifacts, not versioned source or user conversations.
The portable summaries remain here. 09b verification uses one fresh release tree with a valid
Cargo cache marker. Earlier 08h/08g results below qualify their dated executables only.
Step 7 remains postponed.

## Chat ownership cleanup, 2026-11-09c

The reported missing interrupted chat remained present in its exact native source and unfinished
request checkpoint. Existing-layout startup skipped records absent from that layout. It now
appends eligible unfinished requests paused, retaining selection and original sources. Request
checkpoints remain authoritative even before a native session exists; an older snapshot cannot
supply a stale source, conversation view or Home screen over that recovered request.

All production resume entry points now request navigation through the tab runner. An existing
owner is focused without cancellation or draft replacement; conversation/native-chat leases
prevent duplicate ownership across windows sharing the state directory. Distinct duplicate
snapshots remain in the canonical window record rather than generating backup copies. Their
nonempty drafts and requests remain eligible for `/recover`; retained display-only snapshots do
not yet have a dedicated inspection UI. No additional whole user feature was proven unused.

First-send checkpoints now retain conversation identity without requiring a context map and
persist the request before the empty composer. The state inventory records these transitions;
it remains an inventory rather than a formal exhaustive state graph. Verification is scoped in
[current status](../e2e-status.md). Product direction remains postponed.

09c is installed with the exact verified executable hash. Normal Cargo clean removed the single
temporary build tree (2,482 files, about 1 GiB); its valid cache marker was Cargo-created. Portable
results remain in the status document. Automatic approval review rejected recursive deletion of
four explicitly identified synthetic test directories under Windows Temp as "blocked by policy"
without further detail. They remain; the rejected operation was not retried through another path.
The existing user window and original conversation files were left untouched.

## Cleanup closeout, 2026-11-09d

Current closeout after 09d: obsolete worktrees/build trees, redundant archives and demonstrably
superseded implementations have been retired. Inline rename now owns a separate title editor;
the composer-swap implementation is removed. The installed native-window flow is verified.
Source changes still need a Git checkpoint; four previously policy-blocked synthetic Temp
directories remain (about 57 KB total). Retained display-only duplicate snapshots also lack a
dedicated inspection UI. These limitations do not justify deleting unique user recovery data.
The state inventory is not a formal exhaustive state-machine proof, and deferred product/release
qualification remains outside cleanup completion.

## Whole-repository audit, 2026-11-08g

Three reviewers and the parent completed the expanded source review. The final ledger accounts
for 520 tracked/unignored paths: 505 present files and 15 pending deletions/moves. Every present
file has an explicit review disposition. Generated build products, native histories, credentials
and other ignored user state are outside that repository count. Existing pending work is preserved.

Review depth is explicit: first-party production implementations and scripts received complete
contract reads; integration fixtures received purpose/invariant review with selected existing
scenarios executed; historical documents received scope/link reconciliation rather than a rerun
of their old claims. The 70-file local crossterm patch was compared against its cached upstream
archive: six identical files, 47 line-ending-only differences, 16 substantive differences and one
patch note. Thirty ignored managed Harness scripts matched installed-manifest provenance; this
does not independently audit that external tool's implementation.

### Repairs and necessary pruning

- Storage: exclusively create unpredictable staging files, synchronize writes and publish
  adoption create-only. Read stores completely within 32 MiB or report an error while preserving
  the original. Explicit journal deletion checks all exact app-owned targets before removal and
  holds journal/pin locks; native sources and stable lock files remain.
- Provider/account boundaries: preserve existing unregistered account homes, validate physical
  containment and shared registrations before explicit removal, and preserve registrations on
  filesystem failure. Parse actual native option positions rather than treating prompt or model
  values as permission flags. Bound account/config/attachment/manifest reads and auxiliary child
  capture. Redaction scans complete caller-bounded input and preserves recovery JSON structure.
- Continuity/UI: preserve editor state during title rename/cancel, persist explicit title changes,
  retain unsent queue entries and the independent composer on dispatch failure, and refuse sends
  after the last account is removed. Apply exact-session lookup beyond Home's recent-list window;
  require recovery ownership before replacing an App. Completed asynchronous work requests redraw.
  Repair tiny-window arithmetic, Unicode hit-testing, undo history bounds and theme save/load/export.
- Transcript: invalidate on changed text, styles, links, diagram revision and resolution root even
  when message count is unchanged. Reuse exact folded comparison keys within an estimated 8 MiB
  budget; larger sources use the fingerprint fallback. The unchanged existing 5,000-row speed
  assertion improved from 125.58 ms to 9.55 ms per frame in the final rerun (8.94 ms in the
  first repaired run), against its unchanged 100 ms limit.
- Workers/usage: preserve the approved worker sequence after generic routing evaluation, recheck
  enrollment under lock and poll finite event batches. Bind usage cache reads to registered account,
  physical home and timestamps, then reuse that interpretation in MCP. Refuse oversized structured
  MCP output as a whole rather than truncating JSON.
- Support: consolidate process capture, atomic state I/O, bounded file reads and usage parsing;
  remove dead warning/resource algebra, redundant process ownership and unreachable parser code.
  Preserve owned temporary-file cleanup, environment and evidence isolation in scripts. Correct
  release URL/version validation, cancellation boundaries, diagnostic backup identity and UTF-16
  decoding. Preserve intentional vendor modifications; the narrow Unix tty repair remains unbuilt.
- Documentation: reconcile current contracts and move the stale duplicate command inventory to
  the archive. Remove the uncalled experimental WezTerm launcher that guessed the highest pane ID;
  retain the embedded production Lua. No user conversations, provider records or credentials were
  pruned. No broad dependency/framework replacement was introduced.

### Verification and limits

Final execution receipts are reconciled in `target/full-audit-verification.json`; individual
coverage, final file hashes and focused logs are in `target/full-audit-*.json`/`.log`.
These local artifacts are ignored and contain machine-specific paths; they are not publication
content. Current installed/build evidence is summarized in [verification status](../e2e-status.md).

The final Windows 08g executable is installed locally; its SHA-256 matches the checked release
binary. All-target compilation, formatting, whitespace, dependency policy and the three script
gates pass. The final 93 contract checks and 30 selected process/harness/usage/update integration
checks pass, supplementing the earlier focused runtime/UI and attachment-transport checks. One
live Codex `gpt-5.6-luna` low-effort turn returned OK with feed output. The unchanged long-session
fixture passes after profiling removal. Initial security/fixture failures and their passing
corrections remain in the receipts rather than being discarded.

The exact release's native-window probe restored a session older than 125 newer sources in
1.98 seconds without further input. Question-mark input, F2 cancel/save and durable title changes
preserved the independent draft; no provider turn started and the source hash stayed unchanged.
Probe corrections are retained: single-chat headers do not display chat titles, asynchronous
input must settle, and feed screenshots sample at most once per second. Modal boundaries were
paced for that sampler; the native history repaint measurement required no extra input.

The audit does not establish universal provider parity or absence of defects. Only the Windows
MSVC target is installed; Unix vendor behavior remains static-review evidence. Initial large
transcript wrapping and oversized-source fingerprint work remain linear. Unsaved/unfinished work
and unavailable-source recovery remain protected retention exceptions. Destructive account changes
check current-process work and physical ownership, but cannot prove another process is idle.
An earlier concurrent native-history latency timeout remains recorded; successful serial checks
do not erase it. Historical/legal pages were not externally recertified. Broad live approval,
Claude/Cursor/host parity, effectiveness claims and publication remain separate backlog items.

## Earlier follow-through scope, 2026-11-08f

The owner clarified that the requested audit covers the entire repository. The 08f record below
is earlier scoped evidence, not completion of that request. The expanded 08g scope was:
review every first-party production module, scripts, packaging and current documentation; identify
upstream vendor material separately; repair concrete defects and prune demonstrated duplication.
Preserve existing work and native records. Reconcile per-file coverage, run focused checks and
observable flow probes, and report outstanding limits before requesting any product direction.
Final coverage is recorded in `target/full-audit-ledger.json` and reviewer reports alongside it.

Preserve pending user work, provider-owned sources, approvals, exact conversation/account/session
identity, drafts and unfinished recovery. Acceptance: finite progress/visible projections, owned
locks, bounded useful retrieval, visible read failures, fair async redraw, safe closed-tab retention,
and focused observable proof. No new unit/integration tests, account migration or publication.

| Paths | Why the change is necessary |
| --- | --- |
| Provider transports, parser/cards, artifact collector, worker runner, `tui/run.rs` | One backpressured progress path; finite display projections; constant-memory Cursor duplicate detection; cancellation stays responsive and native interaction payloads stay exact. |
| `store.rs`, `state_io.rs`, `threads.rs`, `sessions.rs`, `mcp.rs` | Reuse OS lock ownership and the canonical paged journal reader; remove timed lock stealing, repeated full ordinal scans and provider-file deletion races. |
| TUI recovery, tabs, log, background pollers and reference journal | Bound allocation and resting display retention while preserving unfinished work and original sources; propagate completion into redraw; avoid stale row identities. |
| Privacy gate and current documentation | Reconcile exact owner-approved public-theme references and renamed paths; remove unnecessary internal attribution and repeated current-status narration. Other privacy rules remain active. |

These choices address observed code paths. No bulk abstraction, new queue system, speculative
feature removal, hosted memory component or provider compaction policy was introduced.

## Reviewed coverage

Three reviewers covered UI lifecycle, providers/processes, and memory/storage/workers. The parent
covered support code, configuration/diagnostics/update/install, packaging, dependencies/assets,
MCP consumers and documentation. Deep inspection centered on changed ownership/boundary paths;
rendering/input/attachments and delivery/dependency contracts received focused inspection. Other
scheduler/accounting, managed-workspace, file/attachment/security and vendor internals received
inventory/caller/diff triage, not exhaustive branch review. Passing fixtures do not establish
universal provider parity or prove that no defects remain.

## Repair disposition

- Provider and worker progress queues hold 64 events and await consumers. Cancellation remains
  available while blocked; original approvals, questions and artifacts are never replaced by
  display summaries. Internal startup/answer channels have separate finite lifecycle limits.
- Turn/worker/live text projections keep up to 4 MiB and say when earlier output is omitted.
  Latest tool-card projections retain 256 cards with bounded detail. Cursor repeats use a hash
  instead of a second full text buffer; completed tool entries leave its bounded map.
- Resting transcript views keep recent rows/text only after native sources, recovery, reference
  recording and indexed inspections settle. Unsaved/unfinished work and active selection are
  protected exceptions. Eight safely evictable closed tabs remain in the reopen cache.
- Store mutations use the shared OS-held lock. Read failures appear in diagnostics and the UI;
  corrupt/future-version originals stay untouched. An old timestamp cannot seize a live lock.
- Journals read in bounded cursor pages, tail selection streams within a finite scan budget,
  maps identify partial coverage, and original event ordinals survive filtering. Derived ordinal
  metadata avoids rescanning the growing journal for every referenced append. Pins remain bounded.
- Recovery reads/serialization are bounded; invalid/oversized originals remain discoverable with
  an explanation. Completed image/history/memory/search/decode work requests repaint.
- Empty native-session cleanup reports candidates without removing provider-owned files. That
  removes the append-versus-delete race; source deletion requires provider-owned lifecycle support.
- Removed production dependence on legacy recovery import helpers and dead atomic-write wrapper,
  stopped writing nonexistent task-map pointers when thread logging is off, shared MCP journal
  retrieval, and removed the already-unused static Windows resource in the preceding slice.
- Privacy scanning covers tracked and unignored candidates. Its exact public-theme exceptions
  now match the implementing files and archived paths. Publication remains held by the owner.

## Verification

Final results and exact-build limitations are recorded in [verification status](../e2e-status.md).
Receipts and manual probes live under `target/audit-08f-*`. Earlier 08e qualification is historical
and does not by itself qualify these later changes. The parent runs Cargo sequentially against the
combined changes; reviewers do not count static formatting as feature proof.

Final 08f evidence:

- Focused checks recorded 278 passing executions, including reruns after the lock/protocol and
  history changes; this is not 278 distinct tests or a quality percentage. Existing checks were
  adapted for fail-closed legacy import, stable OS locks and non-destructive native cleanup.
  Parser, worker, recovery, queue, session, continuation, cancellation, MCP, native permissions,
  image transport, process cleanup and quota boundaries passed. No new tests or full-suite run.
- A separate production-runner probe deliberately filled a four-event progress channel, then
  delivered all 81 recognized text events from 5 MiB output. Its projection was 3,145,839 bytes,
  explicitly omitted earlier output and retained the final result. It also verified that an
  unrecognized legacy lock leaves store bytes untouched and a newly marked lock works.
  Early harness attempts used unsupported Claude text-delta fixtures and an already-used scratch
  baseline; these were corrected without product changes. A concurrent build temporarily made
  standalone link artifacts unavailable; the stable final release library passed.
- Final native history restored without further input in 0.16 seconds, with no provider turn or
  source change. A prior five-second timeout under simultaneous native fixtures passed unchanged
  when rerun alone (0.20 seconds). The failed run had already saved the session title, so it was
  not shown to be an identity rejection. Latency under that concurrent load remains unqualified.
  Independently, current-generation binding mismatches now terminate loading with a visible
  notice; stale generations still cannot overwrite a new selection.
- Journal proof read 256 then 144 records, retained tail ordinals 398-400, labelled a partial map,
  and preserved journal/native-source hashes. Large 70,198-byte metadata passed list/index/excerpt;
  invalid-store diagnostics preserved the original bytes. Native long-output/close and 17 MiB
  app-server telemetry/question/click-to-follow-up probes passed on the final executable.
- One real `gpt-5.6-luna` low-effort turn passed the existing ignored live-provider runner and
  produced feed output. A temporary argument-only launcher supplied the supported effort option
  to the official unmodified CLI; authentication/configuration were not copied or changed.
  No Claude/Cursor model turn or live native approval was claimed.
- The exact release passed the existing four-file installer fixtures. Privacy, hidden-process,
  allowed-module-cycle, formatting and diff-whitespace gates pass; 563 document file links have
  valid destinations. Automatic approval review rejected the optional temporary privacy-probe
  file; it was skipped, not retried through another route. The normal privacy scan passed.
- Installed build `2026-11-08f` matches release SHA-256
  `5be6e7f148b64b62fbcbb79f78f5d67e356169a745f22f6809adc118b24cf10e`.
  Installation used the existing explicit binary directory, skipping account adoption and PATH
  changes. No user window was closed, native source deleted, commit made or publication performed.
  Detailed receipts: `target/audit-08f-verification.json` and its named supporting artifacts.

## Remaining direction and qualification

The [canonical backlog](../plans/README.md#audit-follow-ups) distinguishes unresolved reproductions,
protected-work retention exceptions and broader provider/host/release qualification. The older
Claude/newer Codex stale-map symptom requires an exact reproduction; no newest-session fallback
was added. Release audience, product emphasis and support scope remain owner decisions after this
repair pass. No public release or claim of broad memory superiority is implied.
