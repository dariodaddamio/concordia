# Native source pointers and unfinished work

Build `2026-10-31s` adds a bounded pointer index for Codex and Claude JSONL sessions. It works
with Concordia transcript recording off. Build `2026-11-02f` also reads Cursor histories through
the provider's supported ACP history replay; unsupported capabilities report unavailable rather
than guessing a source or database format.

## Identity and exact readback

Build `2026-11-02e` adds forward whole-history scans. Existing recent-window indexes and pointers
remain compatible. Each operation reads a bounded page and persists ranges/hashes, not text:

```text
concordia chat index SESSION --account ACCOUNT --provider codex --scope whole
concordia chat index SESSION --account ACCOUNT --provider codex --scope whole --cursor PREVIOUS_INDEX
concordia chat sources INDEX --from-message NEXT_ORDINAL --limit 20
```

Follow `history.next_cursor` until it is null. Each scan page contains at most 128 records and
8 MiB of source bytes; continuation also verifies the preceding page. The initial source length
stays fixed during pagination, so appends cannot shift ordinals or silently extend the scan.
`history.complete` is true only at the snapshot end with no skipped records. Oversized records
that exceed a scan page and malformed JSON are explicitly counted in `skipped_records`;
these scans do not claim complete readback. Cursors still advance across oversized records.
Whole-history records up to 8 MiB can be read through the same bounded UTF-8 excerpt API.

`native_read` accepts `from_message` and `limit` for range listings. `{ "id": INDEX, "history": true }`
starts whole-history indexing from a recent pointer, or follows the next page from a history pointer.
Message ordinals are global within that history snapshot; each returned page names its first ordinal.
No provider process or inference call is used for these Codex/Claude file operations.

Every saved index binds the provider, registered account, canonical home, explicit native session,
relative session-file path, OS file identity, captured file length, SHA-256 of a bounded header,
and the byte range/hash of each indexed event. The index ID is a SHA-256 of this metadata.
It stores roles and pointers, not conversation bodies, under `<state>/.ao/native-memory/`.

Discovery uses the existing bounded native-session resolver and verifies the exact session in its
header. Indexing reads at most the last 8 MiB and first 64 KiB, retaining up to 128 recent message
or tool events, with at most 256 KiB per event. Truncated/malformed/skipped data is reported.
This first slice is a recent-window index, not a claim to index every event in an unlimited log.

```
concordia chat index SESSION --account ACCOUNT --provider codex
concordia chat sources INDEX_ID
concordia chat excerpt INDEX_ID --message 1 --max-bytes 4096
concordia chat excerpt INDEX_ID --message 1 --offset NEXT_OFFSET --max-bytes 4096
```

Use `claude` for a Claude source. IDs are exact; there is no newest-chat or same-name fallback.
`sources` lists up to 20 range descriptors and the total indexed count; any indexed ordinal can
be read directly. Excerpts return exact UTF-8 native JSONL bytes in a JSON `raw` string, with a
half-open byte range and `next_offset` when another chunk is needed. Chunk boundaries never split
UTF-8. Defaults are 4096 bytes per excerpt, maximum 8192; serialized responses are capped at 16000
characters. The optional MCP tool `native_read` uses the same implementation: omit `message` to
list ranges, or supply `id`, `message`, `offset` and `max_bytes` for a chunk.

An append preserves existing pointers. Changed account/home bindings, moved/deleted files,
replacement (even identical bytes), truncation, changed header or changed indexed range fail
visibly. Compaction that leaves the original bytes in place does not invalidate exact originals;
a rewrite that changes them does. No credentials or unrelated project files are indexed, and
provider-owned logs are never edited. Exact source readback can contain material the user or
provider previously wrote into that session; it is not a redacted summary.

## Handoff and restart

### Cursor protocol sources

Cursor pointers use `native_protocol: "acp/session.load"` and `file: "acp-history"`. Their bytes
are the provider's original replay notifications, not a claim to read an undocumented local
JSONL file. The selected account's canonical config home is passed through documented
`CURSOR_CONFIG_DIR`; inherited API-key/token variables are removed. Concordia neither copies
credentials nor starts login. The provider remains responsible for its own authentication.

The reader requires native `loadSession` capability and existing exact session/workspace metadata.
It sends only `initialize` and `session/load`, with no new session, model prompt, mode change or
MCP server supplied by Concordia. Tool/client requests are rejected. A tracked process guard,
25-second replay timeout, 1 MiB frame bound and 16 MiB wire budget bound the read; larger replays
report unavailable instead of returning an incomplete history as complete. No replay body is
persisted. Fresh replay revalidates pointer prefixes/ranges; changed content fails stale.

`chat index|sources|excerpt`, MCP native reads, `chat read`, and resume scrollback share this
transport. UI display keeps its existing 200-message/1 MiB text budget and sanitizes terminal
controls. Unsupported native versions, missing workspace metadata and authentication errors
remain explicit. Fixture validation does not establish live-provider compatibility for every version.

Sources: [Cursor ACP](https://cursor.com/docs/cli/acp),
[Cursor config homes](https://cursor.com/docs/cli/reference/configuration),
[ACP load/replay contract](https://agentclientprotocol.com/protocol/v1/session-setup).

After a switch/continuation, a cancellable preparation step indexes up to three distinct recent
native sources. The next provider receives a bounded directory of pointer IDs and readback
instructions alongside the existing map and original request. Native session/tool/approval state
stays with its account. Pointer preparation adds no model call. Source failures are shown; they
do not select another chat silently. Native resume omits this extra directory.

A pane-owned `turn.json` saves the unfinished request, run ID, workspace, title, source bindings,
tried targets and rollover count. This is request recovery, not a copied conversation transcript.
It shares the pane's existing OS ownership lock but is separate from the draft and queue. It is
written before dispatch and cleared after success or acknowledged cancellation. A failed initial
save prevents dispatch and preserves the draft. Later save failures remain visible.

After restart, the request and its exact source identities are restored paused. A newer draft and
queued work remain separate. No provider turn, account switch or uncertain tool replay occurs
on restore. `/continue` reviews the original request and opens target choices; `/use` also has a
continue row. `/continue ACCOUNT [MODEL]` explicitly starts a fresh native session, even on the
same account after restart. Save/clear a different newer draft before continuing. `/new` explicitly
starts over. Existing logs and recovery records are preserved.

## Context capacity

Quota failures may use the configured account/model order. An explicit terminal provider capacity
failure (`context_length_exceeded`, `context_window_exceeded`, or the recognized maximum-context
error text) can instead start one fresh native session on the same account. It retains the original
request and source directory. A second capacity failure pauses for explicit choice. Drafts, active
edits/modals/jobs and unfinished/error tool states still block automatic continuation. A pending
cancel wins even if the provider's failure arrived first.

This is reactive capacity-error recovery. Preemptive near-capacity rollover and verified per-version
compaction controls remain planned. Cumulative billed tokens are never treated as context size;
unknown capacity stays unknown, and provider compaction settings are not changed.

## Pinned decisions

An explicitly connected **file-backed** Harness manifest may declare `pinned_decisions`, an array
of up to 16 exact strings totaling at most 4096 UTF-8 bytes. All provider launch paths, including
workers, deliver matching rules verbatim once when starting a new native session. The binding is
provider/home/project specific. Duplicate identical rules from connected harnesses are delivered
once. A changed/unavailable pinned manifest blocks the new-session launch until reconnected;
its recorded SHA-256 must match. Command-backed manifest pins are not supported in this slice.
No Harness command is run automatically to fetch rules, and no provider-home instructions are
rewritten for this feature. Existing connections without pins keep their behavior.

## File previews

In a focused file pane, `/` starts case-insensitive search, Enter leaves the query active, and
`n`/`N` move between matching rows. `r` reloads that pane asynchronously, preserving its identity,
view mode and chat draft. Background metadata checks show `changed on disk`; content is not
silently reloaded. Metadata checks are an indicator, not a full content integrity guarantee.
