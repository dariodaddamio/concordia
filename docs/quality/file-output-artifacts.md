# Structured file output records

Build `2026-11-03x` candidate extends the original q slice. The shared registry records completed
Codex `file_change` / native `fileChange` items with explicit add/update/delete changes. Failed,
unknown, unowned and prose events do not create records. Cursor's documented `stream-json`
`tool_call` completed events are now promoted for `writeToolCall` and `editToolCall` only when
`result.success.path` is present; `call_id` is retained as the native event identity and the
whole structured event is the source revision. The official Cursor schema documents the same
session id on each event and distinguishes completed tool results from started calls
([output format](https://docs.cursor.com/en/cli/reference/output-format)). Claude built-in Write/Edit calls are correlated by native tool-use ID with a successful native
user tool result and file-result metadata. Both envelopes must name the exact session; request
arguments or prose alone never create a record. Write is conservatively an update, because the
result does not prove file creation. Failed, mismatched, ambiguous and uncorrelated results are
refused. The official [SDK message types](https://platform.claude.com/docs/en/agent-sdk/python)
and [tool callbacks](https://code.claude.com/docs/en/agent-sdk/hooks) describe this correlation.
Remote resource URLs and embedded generated-image blobs are not promoted without a verified
adapter.

The provider's verified launch owner supplies conversation, account, home, workspace, execution
directory, run and attempt. Native session/item identities and native turn id when available
come from the exact connection. A SHA-256 revision describes the structured source event, never
the current disk bytes. Repeated identical owner/session/event/path/revision records deduplicate;
different revisions retain distinct identities. The execution directory's filesystem identity
also binds later opening. Reports are metadata, not proof of file authorship or current existence.

Cards show the reported path/change and source identities. The file browser has **Reported file
outputs**. Enter inspects full provenance; O offers the existing preview/split/editor choices,
R returns to the refreshed catalog, and Esc restores the draft. Inspection reads metadata only.
Opening revalidates a regular file under the exact execution root, exclusions, link/canonical
identity and the retained execution-directory identity. Missing/replaced/outside-root files give
an explanation. The action explicitly reads the current disk version; a source-event revision
does not establish whether later file bytes changed. Shared image previews and text panes retain
their usual limits. HTML/SVG/scripts are never evaluated inside Concordia, and catalog discovery
does not download, execute, attach, relaunch a provider or replay a producing tool.

Bounds: 32 file paths per supported event and 32 records per attempt; 256 retained records and
2 MiB of metadata per conversation. Older metadata can leave the bounded catalog; authoritative
native source logs and file bytes are not deleted. A per-conversation lock and atomic version-1
index writes prevent concurrent clobbering. Linked, corrupt, oversized and future metadata is
retained and refused. The UI reads one bounded index in a background task, ignores stale
conversation/picker results and never scans a whole workspace for artifact contents.

Existing fixtures cover native process ownership, exact replay deduplication, metadata persistence,
source revision changes, missing/excluded/outside-root files, future-index preservation, catalog
inspection/current-file actions and draft retention. Evidence is in ignored
`target/artifacts-q-{proof,process,clippy-final,release}.log`; candidate smoke and complete CI are
recorded separately. No model turns or physical-host/live-provider parity claims are included.
