# Continuity map v2

The handoff map is a deterministic table of contents. Concordia never summarizes provider
output and never makes an inference call. The map is UTF-8 text with a bounded character budget
(the configured `threads.map_chars`, capped by the continuity pack budget).

Entries are emitted in source order:

1. The v2 header and explicit thread id.
2. `user turn N:` entries. These are the user's typed messages, collapsed only for whitespace and
   truncated at the entry limit; they are the ground truth of intent.
3. `assistant pointer`, `tool pointer`, and structural-event pointers. Each names its turn ordinal
   and resolves through `concordia thread read THREAD --range N-N`.
4. The bounded `more:` pointer, or the explicit `native_memory fallback` marker when no local log
   is available.

The stable pointer format is:

`<label>: turn <ordinal> -> concordia thread read <thread-id> --range <ordinal>-<ordinal>`

The ordering and truncation are deterministic for identical inputs. Older callers may continue to
use the legacy `build_map` layout; handoff uses `build_map_v2` and carries the version in its
header.

## Pinned spans

When a verified native reference is appended to a thread, its exact plaintext projection is
written, after `crate::redact`, to the thread's `.pins.jsonl` journal. Pins contain provider,
account, native session id, role, and ordinal metadata, plus the redacted body needed to resolve
the pointer after provider pruning. The journal retains at most 128 spans and 256 KiB, removing
the oldest ordinal first. A missing pin falls back to the native-backed thread record; no native
session is guessed and no `resume --last` behavior is used.

Pinning is local and bounded. Provider transcript files remain authoritative for native history,
and redaction happens before either the journal record or pin is written.
