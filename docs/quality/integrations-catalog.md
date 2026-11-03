# Integration catalog

Build `2026-11-03m`. F10 opens **Integrations and agent tools** from the composer without
consuming the unsent draft; `/integrations` is also available. R refreshes, Esc closes, and the
existing shared preview supports scrolling and mouse close. A single background job captures
the conversation; closing/switching invalidates its publication and cancellation is checked
between bounded reads. No catalog job launches a provider or adapter command.

The catalog reuses Harness connections, manifest validation and Concordia's existing owned-home
MCP registration checker. It shows integration id/name/source, recorded manifest revision,
current revision status, provider/home/project binding and recorded connection verification.
Tools use stable integration-id namespaces. Descriptions are untrusted display data and do not
establish safety or permissions. The tool list is suppressed after manifest revision/identity
changes. Command-backed manifests are labelled unrefreshed and never executed during inspection.

Native registration inspection is confined to homes Concordia already owns, uses the existing
provider configuration adapter in read-only mode, and returns only status. No credentials,
tokens, environment values, native configuration bodies or user-owned homes are exposed. Native
configuration reads are now bounded to 1 MiB and refuse linked files. Registration is still
opt-in; inspection does not install hooks, edit homes or enable MCP.

Bounds: 16 integrations, 16 bindings each, 64 tools per manifest, 256 KiB per manifest and 16
native home status rows. The connection reader refuses unsupported versions, more than 512
connections/256 bindings each or a file over 2 MiB. Full/changed/unavailable inputs explain the
limit rather than silently claiming complete discovery. Filesystem calls remain local OS calls;
cancellation is observed between them, not a guarantee that a blocked device read is interruptible.

The agent-facing `integrations` MCP tool returns compact summaries, then up to eight tools per
explicit id/offset request. Later pages require the recorded revision. Native-home and binding
summaries have explicit omission counts; result text is bounded before returning JSON. Discovery
adds one terse tool schema, rather than attaching every integration tool schema to every turn.
No outbound MCP client, marketplace fetch, tool executor or authentication broker is introduced.

Existing explicit `/harness connect`, `verify`, `tools` and `disconnect` remain owned by the
Harness command path and preserve its argument arrays, minimal environment, time/output limits
and native approvals. Disconnect withdraws Concordia connection/verification receipts; provider
home changes need the integration's own uninstall. Catalog inspection never misrepresents receipt
removal as native uninstallation or automatically attempts it.

Evidence: existing Harness fixtures exercise verified and changed revisions, namespaced tool
pages, cursor rejection, command-backed nonexecution, cancellation and oversized metadata.
Existing MCP checks enforce exact tool names, schema budget and invalid argument rejection.
Existing native-registration fixtures retain merge/preservation behavior. The existing source
retrieval terminal journey opens F10 with an unsent draft and closes it without a provider prompt.
This does not establish a new provider plugin execution contract or live integration parity.
