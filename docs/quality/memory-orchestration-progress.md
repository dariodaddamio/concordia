# Remaining memory and orchestration implementation

This branch continues the explicitly requested unfinished features after the UI/native-chat
batch. Work proceeds without real provider model turns. This record distinguishes completed
interfaces from work still being implemented; it does not mark the complete roadmap finished.

## Scope and acceptance

1. **Whole-history indexing and pagination:** source-bound resumable scans reach the beginning
   of long logs, keep snapshot ordinals stable under append, and report any skipped data.
2. **Related-chat retrieval and lineage:** bounded local retrieval returns exact source pointers;
   explicit parent links remain distinct from lexical/project/file suggestions.
3. **Cursor exact readback:** use the supported ACP history replay with exact account/session
   binding, capability checks, bounded reads, no prompts/tool execution, and no stored chat copy.
4. **Proactive context rollover and compaction policy:** use verified native context signals;
   unknown capacity stays unknown. Policy is visible/reversible and native compaction controls
   are used only where supported. Handoff preserves original-source/pinned-decision access.
5. **Durable worker recovery and quota failover:** persist explicit run/owner/workspace identities,
   prevent duplicate execution/results, pause uncertain crash recovery, and bound retries.

## Implemented and checked

- `native_memory` now owns whole-history pages alongside legacy recent-window pointers. CLI and
  MCP call its same validation/readback functions; neither caller implements a second reader.
- Each page stores byte ranges and hashes only. The snapshot length is fixed, cursors bind the
  exact account/home/session/file identity, and source rewrites fail closed. Oversized or malformed
  records are explicit gaps. Whole-history records up to 8 MiB retain chunked UTF-8 readback.
- Native range listings paginate beyond twenty records. `native_read` can expand an exact pointer
  into the next bounded whole-history page without a provider process or inference call.
- Validation: eight native-memory tests, 19 MCP tests, the public CLI/MCP integration test, and
  all-target clippy with warnings denied pass. The cases include 400 records, sources beyond the
  old tail bound, appended records, Unicode, source rewrites and cross-account cursor misuse.

- Cursor readback now uses supported ACP `session/load` for exact account/session replay. The
  same reader serves pointer indexing, `chat read` and asynchronous resume scrollback. No prompt,
  login, tool execution or conversation copy is involved. Replay is bounded to 16 MiB/25 seconds;
  unsupported capabilities, source changes and unexpected client requests fail closed.
- Cursor validation: public CLI integration (including exact Unicode text, config-home isolation,
  source mutation, unsupported capability, wrong session and rejected tool requests), nine chat
  reader tests and all-target clippy pass. This is fixture evidence, not live Cursor parity.

- Bounded lexical retrieval and explicit/observed lineage are implemented. `/memory` uses background
  readers and drops late results after conversation changes. CLI source-page search reaches older
  whole-history pages. Exact pointers remain authoritative; suggestions never imply ancestry.
- Retrieval validation covers Codex and Claude exact hit readback, account mismatch, mutation,
  confirmed-link removal, no stored text, draft preservation and stale TUI results. Clippy passes.

- `/context` policy now implements between-turn Codex proactive rollover using the last native
  request's context measurement, with original-source verification and normal cancellation/pack
  delivery. Claude and Cursor capacity remain explicitly unavailable. Per-process Codex token
  threshold and Claude automatic-compaction controls are visible, opt-in and reversible.
- Policy checks cover unknown capacity, threshold boundary, overflow, provider isolation, exact
  native measurement, cumulative-usage exclusion, model changes and compaction invalidation.

- Durable workers now have atomic pane-owned metadata, OS locks, stable UUIDs, exact native source
  references, paused crash recovery and explicit reconciled retry. Delivery is reserved before
  dispatch. Quota failover is opt-in, bounded and refuses partial/uncertain/cancelled work.
- Recovery checks cover competing owners, workspace mismatch, no saved native text/context pack,
  failed writes, paused restart and delivery reservations. The fake-provider integration passes
  Codex-to-Claude quota failover and proves restart does not execute another task.
- The Windows terminal proactive-rollover journey passes: exact native measurement, explicit model,
  enabled threshold, fresh-session argv, original source pointers and reversible policy.

Whole-app and final installed-terminal checks remain for final integration. These implementations
are bounded interfaces, not blanket live-provider parity or public-release clearance.

## Supported-interface evidence

- [Cursor ACP](https://cursor.com/docs/cli/acp) documents `agent acp` and explicit session loading.
- [ACP session setup](https://agentclientprotocol.com/protocol/v1/session-setup) requires the
  `loadSession` capability before loading and defines history replay during `session/load`.
- [Cursor configuration](https://cursor.com/docs/cli/reference/configuration) documents
  `CURSOR_CONFIG_DIR`; the provider can read its own account configuration without Concordia
  copying credentials.
- [Claude environment variables](https://code.claude.com/docs/en/env-vars) documents
  `DISABLE_AUTO_COMPACT=1` for automatic compaction and `DISABLE_COMPACT=1` for all compaction.
  Finding a documented flag does not by itself establish live-provider validation.
