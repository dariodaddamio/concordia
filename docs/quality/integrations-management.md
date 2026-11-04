# F10 integration management

Build `2026-11-03x` candidate. F10 preserves the composer draft and shows the selected integration,
its revision, bindings and declared tools. Up/Down or Tab changes the selection; C reconnects,
V verifies, D disconnects a selected provider/home/project binding, N selects a new local
manifest, R refreshes and Esc closes. New paths support typing, backspace and terminal/clipboard
paste without editing the composer. Discovery never runs manifest commands.

Enter on a selected local manifest first shows the full action review: conversation/account,
provider, home, project, manifest path/revision, effect and expanded command. A second Enter
confirms. Escape cancels review. Changed source revisions or target bindings require reselection.
Command-backed manifests remain discoverable, but mutation from this UI requires an inspectable
local manifest and command; agent-facing Harness commands retain their existing contract.

Actions reuse the existing Harness command and receipt owners in bounded background work.
Cancellation controls the external command tree; navigation suppresses stale publication. A
cancelled or failed adapter may already have applied effects: the UI requests reconciliation
rather than asserting it was undone or automatically retrying. Disconnect removes only the
selected binding and its matching Concordia enforcement receipt; native installation/configuration
is retained. Receipt updates use a cross-process lock and atomic replacement.

The existing terminal journey covers local selection, stale review rejection, successful connect,
selected disconnect and draft preservation. Existing receipt checks cover stale identity refusal
and preservation of other bindings. These are fake/local adapter checks, not live provider parity.
Native MCP registration remains inspected read-only; this slice adds no marketplace, auth broker,
or outbound tool executor.
