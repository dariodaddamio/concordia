# L5: integration management from F10

Read the [common worker instructions](README.md), [catalog evidence](../../quality/integrations-catalog.md)
and the integration phases in [workspace and inputs](../workspace-input-completion.md).

## Outcome and ownership

The read-only F10 catalog and harness commands already exist. Make Connect/reconnect, Verify
and Disconnect available from that catalog through the existing mutation owner. This lane is
ready for implementation; it does not require a new marketplace or outbound MCP runtime.

Own `crates/concordia/src/tui/integrations.rs`, `harness/catalog.rs`, `harness/command.rs`,
`harness/connections.rs` and feature evidence. The coordinator owns shared TUI dispatch, MCP
registration and `integration.rs`. Do not duplicate receipt persistence or command semantics.

## Work sequence

1. Add selected-item actions using existing `connect_in`, `verify_in`, `disconnect_cmd` and
   `tools_cmd` paths. Define an explicit local manifest-selection flow for a new connection.
   Discovery and selection must not automatically execute a manifest's command.
2. Capture an immutable action target: conversation, integration ID, manifest revision,
   provider/home/project binding and catalog generation. Recheck it before mutation; changed
   targets require refresh and reselection rather than acting on a new implicit target.
3. Show a full confirmation for mutations with the actual target and effect. Run bounded work
   away from the input loop, preserve the composer draft and report success/failure clearly.
4. Cancel or suppress stale UI publication on navigation. Cancellation after a mutation may
   leave an applied or uncertain result: reconcile through existing receipts without replay.
5. Disconnect withdraws Concordia receipts; it is not native uninstallation. If adding native
   registration refresh, make it a separate reviewed action using existing
   `integration::sync_mcp(write=true)` and only Concordia-owned homes.

## Acceptance and handoff

Extend existing fake-adapter scenarios for connect, verification, reconnect, disconnect,
failure/timeout and changed revisions. Demonstrate draft preservation, responsiveness, stale
target rejection and accurate applied/pending/uncertain outcomes after cancellation.

Keep agent-facing discovery read-only and use existing approved mutation paths. No changes to
global/user provider authentication or homes, no new auth broker, no marketplace dependency.
Return the observable F10 flow, focused evidence and exact shared-file integration patch using
the common delivery record. Finish this complete UI slice before proposing adjacent systems.
