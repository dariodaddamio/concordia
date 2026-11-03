# Selected project replacement

Workspace/input W2 extends the existing literal project search and editor save implementation.
Search remains read-only until matches are explicitly selected and a replacement preview is
confirmed. The chat draft, queued messages and active provider turn keep their existing owners.

In project search, Space marks/unmarks the selected result. F2 (or Ctrl+R) opens the replacement field;
an empty replacement deletes the matched substring. Enter prepares an exact preview. Up/Down
and Page Up/Down scroll it; F6 (or Ctrl+Enter) applies that displayed preview. Esc dismisses it or stops
an applying batch before the next file. Changing the query/case invalidates selections and plans.

Each preview binds its UUID, canonical root, conversation/generation, selected byte ranges and
original/replacement bytes and hashes. Limits are 1,000 selected matches, 32 files, 4 KiB replacement,
2 MiB/50,000 lines per file, 8 MiB aggregate before/after text and 256 KiB readable preview.
Existing search exclusions and bounded exact-source verification apply. BOM, CRLF and Unicode
remain byte-accurate. No regex interpretation, hidden scan or provider call is introduced.

Before the first write, every selected file is checked for regular identity, containment,
read-only state and unchanged original bytes. Dirty, saving or loading panes block their files.
The existing cooperative workspace lock guards the entire root against active/uncertain main,
worker and managed-worktree reservations through the batch. This deliberately blocks unrelated
concurrent writes in that root too; it does not claim to constrain external processes.

Saves run serially through `file_document::save`, preserving its compare-before-replace and
retained staging/backup behavior. A failure leaves earlier completed files saved and remaining
files untouched. No multi-file atomicity, automatic rollback, retry or replay is claimed. Clean
affected panes reload asynchronously after completion; dirty buffers are retained.

Before saving, a version-1 receipt is persisted with exact plan/root/conversation identity and
per-file hashes/status. Write-ahead states expose an interrupted save. Receipts contain metadata,
not copies of project text. The store retains at most 128 receipts, 128 KiB each, with explicit
full/corrupt/future-version errors and no automatic pruning. A used plan cannot execute again.

F3 (or Ctrl+H) opens a metadata-only receipt picker for this root; choose one and press Enter to compare
current file hashes. It reports saved/original/changed/unavailable bytes without changing files
or resuming a batch. An exact receipt UUID can also be pasted into the picker. Restart never
automatically continues a replacement. Closing/changing conversation cancels the owning job;
any already persisted receipt remains available for explicit inspection.

Existing search checks exercise selected Unicode replacement, preservation of BOM/CRLF, stale
preflight, pane blocking, ownership conflicts, cancellation, duplicate-plan refusal, receipt
readback without mutation and forged ranges. UI checks prove previewing does not save and only
the explicit confirmation emits an apply action. No real provider task is needed.
