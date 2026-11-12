# Continuity map v2

The handoff map is a deterministic table of contents. Concordia never summarizes provider
output and never makes an inference call. The map is UTF-8 text with a bounded character budget
(the handoff planner currently caps it at 1,600 characters; `threads.map_chars` separately controls
explicit chat-reference maps).

Entries are emitted in source order:

1. The v2 header and explicit thread id.
2. `asked:` for the first user message, then `user turn N:` entries. First/latest intent is
   collapsed for whitespace and bounded; intermediate user turns retain source pointers.
3. `assistant pointer`, `tool pointer`, and structural-event pointers. Stored-thread ordinals
   resolve through `concordia chat thread read THREAD --range N-N`.
4. The bounded `more:` instruction. With logging off, the map identifies in-memory turn numbers
   and directs retrieval to attached native-source indexes or explicit `concordia chat index`;
   it does not invent a stored thread or native message ordinal.

The stable pointer format is:

`<label>: turn <ordinal> -> concordia chat thread read <thread-id> --range <ordinal>-<ordinal>`

The ordering and truncation are deterministic for identical inputs. `threads::build_map` is the
single current formatter for handoff, CLI/MCP maps and explicit chat references; its header carries
format v2. Partial-journal notices enter the header. Compaction reserves the header and source
instruction before shortening user intent when that budget can fit both lines.

## Pinned spans

When a verified native reference is appended to a thread, its exact plaintext projection is
written to the thread journal. References contain provider,
account, native session id, role, and ordinal metadata, plus a bounded local redacted excerpt.
The journal retains at most 128 spans and 256 KiB, removing
the oldest ordinal first. Current journal reads verify the native-backed record; unavailable or
stale sources produce an explicit gap. Pins are retained independently, not used as a silent
replacement for failed native verification. No session is guessed or resumed with `--last`.

Pinning is local and bounded. Provider transcript files remain authoritative for native history,
and redaction happens before the journal record is written.
