# L4: broader provider file outputs

Read the [common worker instructions](README.md), [artifact evidence](../../quality/file-output-artifacts.md)
and [file actions](../../quality/file-actions.md).

## Outcome and ownership

Completed Codex structured file changes already reach the artifact registry. Add one additional
provider's verified structured local-file reports to that same registry, card and file catalog.
Keep Claude and Cursor coverage as separate subtasks; completing one does not complete both.

Own `crates/concordia/src/artifacts.rs`, agreed artifact-domain helpers and feature evidence.
The coordinator owns shared stream normalization, provider parsers, `tui/artifacts.rs`,
`tui/files.rs` and TUI dispatch. Return an exact-baseline patch for those files.

## Work sequence

1. Verify a supported completed-event schema and its tool/request/session correlation, success
   and error semantics, execution directory and local-path provenance. A tool request, arbitrary
   prose or plausible filename does not establish that a file was produced.
2. Normalize supported completed reports into the existing artifact owner and persistence path.
   Preserve provider/account/conversation/run/attempt identity and native source revision.
3. Reuse existing bounds and recheck their current constants: 32 paths per event, 32 records per
   attempt, 256 per conversation and 2 MiB metadata. Do not create another artifact store.
4. Deduplicate repeated reports of the same revision while retaining genuinely new revisions.
   Revalidate current-file ownership and revision when opening or inspecting changes.

## Acceptance and handoff

Extend existing fixtures with completed, failed, duplicate, changed-revision and foreign-owner
events. Cover missing/replaced files, sensitive or outside-workspace paths and stale provenance.
The existing click-through must show the supported current-file/diff behavior or explain precisely
why the original revision is unavailable. Never fabricate historical bytes from the current file.
Do not automatically execute/open reported files or fetch remote outputs.

Return observable provider coverage, fixture evidence, resource bounds and the coordinator patch.
If a provider has no verified usable completed report, record its exact missing schema as
`blocked-vendor` and continue the other supported provider. A generic unsupported stub is not work
completed; no real provider model turns are authorized.
