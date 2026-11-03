# Feature completion audit: 2026-11-03n

Internal raw editing, default formatted documents, file recovery, command-screen and pinned-command
improvements are implemented. The complete
memory-management/orchestration roadmap remains unfinished. This audit does not mark planned
features complete because their UI exists or because a synthetic demonstration passes.

The [remaining-feature execution plan](plans/completion-roadmap.md) covers every open area below,
including implementation order, parallel ownership and provider/host/release evidence. Plan
creation does not change the implementation status recorded here.

## Closed in this build

- Managed cross-worktree dependencies wait for explicit result-only or already-integrated
  receipts, with source/repository revalidation and retirement references. [Scope](quality/dependency-delivery.md).

- Ordered text-document snapshots and explicit F9 pasted-path attachment preserve drafts/queues
  and exact native text data. [Input scope](images-and-diagrams.md). Binary transports remain unavailable.
- Context observations bind exact native turn/model and source revision, including same-model
  invalidation and specific unsupported-provider reasons. [Evidence](quality/capacity-provenance.md).
- Native usage records have a separate bounded coverage view, excluded from combined totals.
  Attempts, outcomes and full workspaces remain distinguishable. [Scope](quality/native-usage-coverage.md).
- F10 and agent MCP discovery share a read-only integration catalog with namespaced tools and
  revision/native-registration status. [Scope](quality/integrations-catalog.md).

- Unified file actions and an independent Concordia editor share pane saves, conflict checks
  and owned recovery. Native GUI editor configuration is explicit; defaults remain internal preview.
  [Behavior and verification limits](quality/file-actions.md).

- Selected project replacement uses exact byte-bound previews, ownership/pane preflight and
  the existing editor save path. Durable receipts support explicit reconciliation without replay.
  [Behavior and limits](quality/project-replacement.md).

- Opt-in Codex app-server execution now connects native command/file decisions and bounded
  questions to foreground/worker review with exact thread/turn ownership and resolution handling.
  [Scope and limitations](quality/codex-native-input.md). CLI 0.166.0 is the matched protocol;
  Claude bridging, free-form native input and live parity remain separate.

- Worker-owned native input, opt-in Cursor ACP text and explicit native plan decisions now use
  exact live request ownership. Edit cards, file detection, activity icons, agent inspection,
  queue return and quota/credit cache fixes are implemented in the current candidate.
  [Scope and verification](quality/native-worker-input.md). Claude interactive execution,
  live parity and the broader workspace/provider roadmap remain separate work.

- A versioned synthetic retrieval corpus and provider-free collector measure scoped relevance,
  exact excerpt resolution, visible gaps, read cost and resource observations through existing
  checks. [Evidence and prepared live experiment](quality/memory-evaluation.md). Model-retention
  trials remain unrun and require an explicit provider/task budget; ranking stays opt-in.

- Explicit related-chat groups retain overlapping membership and provenance. A separate discovery
  screen preserves drafts and landing selection, with cross-project context suggestions labelled.
  [Behavior and evidence](quality/related-chat-groups.md). Corpus evaluation remains open.

- Opt-in ranking explains verified goal/handoff/file/text/time signals with exact native
  pointers. Historical file facts never read the referenced files or claim an edit completed.
  [Behavior and evidence](quality/memory-ranking.md). Corpus evaluation remains open;
  lexical recall remains the default.

- Explicit managed Git worktrees bind a clean committed base, repository, owner/execution
  directories and worker dispatch. Interrupted creation is retained and explicitly reconciled;
  worker returns label unintegrated edits. [Behavior and evidence](quality/managed-workspaces.md).
  Candidate W3 adds reviewed native integration, retirement and separate branch deletion.
  Native filesystem confinement remains open.

- Cooperative workspace reservations coordinate main/worker admission and internal saves across
  panes, with explicit declarations, conflict owners and retained uncertain-exit claims.
  [Behavior and limits](quality/workspace-reservations.md). Scheduler claims do not enforce native
  write boundaries; native filesystem confinement remains open.

- Local routing evaluation, ordered persisted account/model overrides and inspection share the
  worker/main launch rules. Task approval freezes sequences and identity bindings; changed bindings
  pause dispatch, and failover reasons survive recovery. No capacity probe or retry-budget increase
  is introduced. See [routing evidence and limits](quality/routing-policies.md).

- Main and worker attempts share attributable, idempotent usage recording, including failover,
  cancelled/failed attempts and accounting-only crash reconciliation. The token screen and landing
  totals use bounded background reads, explicit coverage and account/home-aware session grouping.
  See [accounting evidence and native limits](quality/worker-accounting.md). External source
  observations are separate coverage; broader live native-provider verification remains open.

- Explicit goal/source associations reuse the existing goal store, verified native pointers and
  one bounded relationship service. CLI/TUI mutation is explicit; MCP exposes reads only.
  See [association evidence and limits](quality/goal-source-associations.md).

- Approved shared-workspace dependencies now dispatch through the existing durable worker queue.
  `/delegate after ID[,ID] ACCOUNT TASK` binds exact runs; `/delegate graph` explains readiness.
  Failed prerequisites block dependants, stale results pause and restart dispatches nothing.
  See [second-wave evidence](quality/completion-wave2.md). Cross-worktree dependency integration
  is now receipt-bound; native filesystem confinement remains a separate capability.

- First completion wave adds bounded read-only project search plus dependency/recovery and native
  interaction contract foundations. See [scope and verification](quality/completion-wave1.md).
  These first-wave foundations alone do not complete dependency dispatch, workspace isolation, native worker
  approvals or provider parity. Existing Cursor ACP permissions/questions now bind to the exact
  conversation/run/attempt/connection, with native options preserved.

- Session handoff selection through `/use` or `/continuation`: pause and choose, automatic
  selection, or a preferred connected account. Session choices preserve the saved default;
  retries remain bounded and uncertain tool completion still stops automatic continuation.
- Conversational choices require a closing question/invitation and never take typing focus.
  Free replies retain their exact text and line breaks; typed digits do not send a highlighted
  choice. The complete assistant reply remains visible, and native permission responses stay
  separate. Regression tests cover the unattended-work instruction that previously became a choice.
- Later turns resume the new native session after a handoff. Model server-capacity failures offer
  account/model continuation, while generic native history-write errors retain explicit recovery.
- File underlines and click targets cover existing file references without including preceding prose.

- Paste line-ending and whitespace preservation through send/queue/edit, plus explicit recovery
  after native Codex history-recording failure without automatic replay.

- Native Windows text clipboard, accurate/failure-safe cuts and faster active-output polling.
- Per-worker native models, batched result collection and opt-in automatic handback, bound to
  the launching conversation and using normal dispatch/recovery. Persistent worker state and
  bounded quota failover and approved shared-workspace dependency scheduling are implemented;
  managed worktree creation and candidate reviewed lifecycle integration are implemented; native confinement remains open. See
  [orchestration and latency](orchestration-and-latency.md).
- Stable completed-diagram rendering in long histories and inspectable patch-failure diagnostics.

- Local bounded Mermaid terminal rendering, shared by chat and formatted Markdown.
- Native image payloads for Codex/Claude/Cursor, composer capture/preview/remove chips, immutable
  snapshots and draft/queue/recovery/handoff references. Cursor requires advertised ACP image
  capability. UTF-8 documents and explicit attachment of pasted file paths are implemented;
  inline raster thumbnails and verified binary document transports remain open.
- Cursor image-turn native permission/choice responses. Ordinary text adapters and other native
  approval paths have not been migrated; this is not full provider interaction parity.
- Draft-preserving automatic quota continuation, deferred retry after command screens/queued edits,
  completed-error versus still-running-tool classification, and broader terminal quota-event parsing.
- Non-clickable timing/token metadata, full subtle user-message backgrounds, consistent paused
  composer spacing and incoming/current-only default update notes with H history expansion.


- Selectable conversational question replies, draft preservation and paused queues; these are
  follow-up messages, not native approval RPC support.
- Chat/recent/workspace file discovery with bounded ignored-path-aware traversal, quoted absolute
  and cwd-relative completion, and canonical deduplication.
- Explicit Send now queue controls; the prior turn stops before one selected item starts.
- Below-chat run status/Esc, persistent completion timing, user-row highlights, conditional Bottom
  and consistent command-list padding.
- Fresh read-only Codex quota requests with identity-bound caching and recorded fallback labels.
  Structured quota-code handoff and explicit automatic-pause reasons.


- Formatted-by-default Markdown/JSON; editable raw source with syntax colors, selection,
  undo/redo, explicit save, conflict detection and dirty-close/reload/exit decisions.
- Pane-owned recovery for file buffers, cursor/mode/layout. Interrupted successful saves are
  reconciled against current disk bytes; external changes retain the recovered buffer.
- Windows headless provider launches request no console window. Node used by an installed
  provider remains legitimate subprocess work; no unrelated Node process is terminated.
- Continuation keeps the original request in chat instead of echoing internal handoff instructions.

- Command pickers, usage, accounts/models, file/recovery lists, token/update screens and activity
  output have shared X exit controls. Mouse close uses a matching press/release; dragging away
  does not close. Update close cancels its own job through the same action as Esc.
- The message box shows Esc/X and explains paused chat ownership. Regular list typing filters;
  activity typing searches output. The original draft stays in the editor and returns on close.
- The pinned expanded-tool collapse control uses the same command/argument preview and output
  count as the compact card, so scrolling retains the command subject rather than only `shell`.
- Explicit GIF/PNG/JPEG/WebP references use the system viewer, preserving the draft and leaving
  text previews internal. Viewer launchers are bounded/reaped, and arguments do not pass through
  a command shell. Tests replace the external opener; physical viewer behavior is not certified.

Implementation: `tui/modal.rs`, `tui/mod.rs`, `tui/pointer.rs`, `tui/files.rs`.
Interaction evidence is in `tui/tests.rs` and the isolated `tui_journeys.rs` modal journey.

## Existing local features retained

The validation suite covers account/model selection and isolation, provider launch planning,
native session binding, drafts/queues/recovery, goal/chat navigation and wrapping, fresh native
usage reads, bounded quota/model fallback, exact recent-source readback, cancellation guards,
explicit workers and reported native subagents, connected Harness pins, reactive capacity-error
recovery, Markdown tables/links, chat/recent/workspace file suggestions, raw/formatted previews, search,
reload/change indication, independent input scrolling, update/install controls and read-only MCP.
The latest run evidence and its concurrency conditions belong in [E2E status](e2e-status.md).
This is fixture/automated evidence, not a claim to have tested every possible real-provider event.

## Core roadmap status after the memory/orchestration implementation

These items are not a completed feature set. Repeated UI polishing did not finish the core
roadmap, and an audit is not implementation. Local engineering can proceed without owner input.

1. Whole-history native indexing is implemented with immutable bounded pages and stable snapshot
   ordinals. Oversized/malformed records remain explicit gaps; recent indexing remains the default.
2. Cursor readback uses supported, capability-checked ACP replay and exact session/account binding.
   Bounds and fixture evidence do not establish parity for every installed Cursor version.
3. Bounded lexical retrieval and explicit/observed lineage are implemented. Suggestions remain
   separate from confirmed relationships; richer goal/file relationship ranking remains open.
4. Optional Codex proactive rollover and process-local Codex/Claude compaction controls are
   implemented. Claude/Cursor proactive capacity remains unknown; live validation remains open.
5. Worker UUIDs, pane-owned ledgers, paused crash recovery, exact result pointers, delivery
   reservations and bounded quota failover are implemented. Uncertain work requires explicit
   reconciliation; no process, tool or result is automatically replayed at restart.
6. Live provider/auth/approval parity and public-release gates. Real provider model turns remain
   prohibited without an explicit scope and budget. Synthetic fixtures do not clear these gates.
7. Visible custom pointer shapes in the installed WezTerm 2024 host. Its parser does not support
   the requested control; app-side shape-selection tests do not close that host limitation.

Managed worktrees and cwd search/replacement are implemented within their documented guards.
Image/document input and explicit pasted-path attachment are implemented; inline raster thumbnails
and binary-document transports remain open. Signing and platform expansion
have their own release/roadmap status. They are not silently
included in the two GIFs. See [PLAN](../PLAN.md), [native memory](native-memory.md),
[workspace requirements](workspace-panes.md), [release blockers](release-blockers.md), and
[host evidence](chat-rendering.md).

## What the owner can unblock

- Local memory/orchestration implementations and their fixture evidence are recorded in
  [the implementation ledger](quality/memory-orchestration-progress.md). Cwd replacement and
  broader workspace ownership/isolation features remain engineering work, not permission blockers.
- Live provider/auth/approval parity needs an explicit provider/task scope and usage budget under
  `AGENTS.md`. No model turns have been used to clear this gate.
- Unsupported proactive capacity requires verified native signals for installed versions.
  Unknown signals must remain unknown; documentation/read-only inspection
  can continue without asking the owner to research them.
- Visible pointer shapes require a terminal host that supports the requested control. The
  installed WezTerm 2024 build does not; demo cursor overlays do not change this.
- Publishing, signing identity and distribution choices are release decisions, not reasons to
  stop implementing local features.

## GIF scope

Two replacement GIFs show file review/edit/save and an explicit account continuation after a
simulated limit. Their frames come from the actual `draw_ui` renderer and keyboard/mouse/continuation
handlers, using a fake dispatch channel. Data and replies are synthetic, and each GIF says so.
Cursor movement/click feedback are editorial overlays. No real provider
turn, real account mutation, benchmark comparison, release publication or social-media post is involved.

Reproduction and files: [media](media/README.md). These drafts do not clear the full-feature
[launch gates](launch-plan.md).
