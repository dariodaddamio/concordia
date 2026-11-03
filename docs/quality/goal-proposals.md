# Goal change proposals

Build `2026-11-03o`. The existing `goals` MCP tool reads goals/source links and accepts typed
add/close proposals. Proposals bind a canonical UUID request id, exact goal identity/text and
observed goal-list revision. The MCP handler cannot apply a change. Requester/provider/account/
session attribution remains unverified; client names and `clientInfo` cannot establish it.
No native permission, worker launch or source-association mutation is exposed by this route.

Goal proposals share the existing bounded proposal owner and foreground review queue. Worker
proposals retain the delegation-mode gate; goal proposals do not require enabling workers. The
UI waits while a goal entry or conflicting picker/file surface is active, then shows the entire
proposed goal, action, revision and request. **F7** focuses the choice; **Y** approves and **N/Esc**
declines. Ordinary message typing cannot approve or decline. The unsent draft is preserved even
while answering a focused review. Other edits/modal focus disarm the choice. Enter cannot send
an existing draft while goal review is focused. If the full text cannot fit, approval is disabled.

The one shared `goals.json` store retains stable ids, immutable text, monotonic completion, its
2 MiB bound and atomic goal lock. Optional add/close attribution distinguishes user input from
MCP proposals approved locally; legacy attribution stays unknown. Unknown per-goal metadata is
preserved. Goal reads do not migrate data. An explicit save writes a version-2 envelope, which
reads legacy arrays while making older binaries fail closed instead of dropping new attribution.
Unsupported/corrupt future formats remain unchanged. Open MCP-origin goals display a diamond
and an explicit locally-approved-MCP label rather than claiming a verified native author.

Before approval applies, the service revalidates the exact proposal and revision under the
shared goal lock. It writes a bounded `prepared` receipt before saving the goal, then finalizes
the receipt. Stale proposals cannot overwrite concurrent manual changes. A duplicate request is
idempotent; reused ids with different intent are refused. Closing an already completed exact
goal changes no goal or original close attribution. Declined requests cannot silently requeue.

Unconfirmed receipts never replay a change, including after restart. `goals` action `status`
reports the receipt and whether the current goal carries that exact request attribution. A
successful goal save followed by failed receipt finalization is explicitly unconfirmed. Receipts
contain bounded goal proposal metadata, not provider chat copies or credentials. No second goal
database, relationship graph, authentication flow or inference service is introduced.

Bounds: 512 characters/2048 bytes of readable proposal text, 16 KiB per pending proposal/receipt,
10 pending proposals, the existing foreground confirmation cap, 256 receipts and 256 entries in
the pending directory. Invalid/oversized/future pending metadata is quarantined without following
links or reading oversized bodies. Full/corrupt state is retained and explained; no automatic
pruning or replay occurs. MCP registration remains opt-in and the nine-tool schema stays under
4,000 bytes. No runtime or package dependency was added.

Evidence: the extended existing goal-store fixture covers legacy reads, older-writer refusal,
unknown metadata preservation, immutable text, stale/concurrent changes, duplicate add/close,
decline, prepared receipts before/after goal save and future-format refusal. The existing MCP
fixture verifies proposal-only behavior, stable-request deduplication, delegation-off operation,
forged actor rejection and status without requeue. The extended real-terminal goal journey
checks full review, ordinary yes-shaped typing, preserved drafts, approval, decline, exact close
attribution and stale approval rejection with zero provider prompts. Live model behavior and
native requester identity are not claimed by these fixtures.
