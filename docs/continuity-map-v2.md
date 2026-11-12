# Continuity map v2

The handoff map is a deterministic table of contents. Concordia never summarizes provider
output and never makes an inference call. The map is UTF-8 text with a bounded character budget
(the handoff planner currently caps it at 1,600 characters; `threads.map_chars` separately controls
explicit chat-reference maps).

The v2 handoff layout is fixed and section-ordered:

1. Header and explicit thread id.
2. `goal`: the first user message and, when different, the latest user redirect, verbatim after
   whitespace collapsing and bounded per line.
3. `files edited`: paths extracted from the existing structured tool cards, with edit counts,
   sorted by descending count then path. At most eight paths are shown; a thread-read pointer
   identifies the complete tool-event list when entries are capped.
4. `last error`: the last error-status tool card's bounded detail, verbatim after whitespace
   collapsing, with its exact source pointer.
5. `previous agent final message`: the last assistant event, showing its bounded start and end
   plus an exact pointer. If the recorded text contains a completion claim, the heading says
   `agent claim`; this is not verification.
6. `pointers`: all remaining events and the one-line `read pointers with:` instruction. Stored
   ordinals resolve through `concordia chat thread read THREAD --range N-N`; in-memory maps use
   the explicit native-source index instruction.
7. `current state`: workspace facts supplied by the existing Git projection, or the recorded
   event boundary when no workspace projection is available.

Every map uses the caller's single character budget, including headings and pointers. If it is
too small, trimming is deterministic: extra pointer lines, final-message detail, the error,
file rows, and latest redirect are removed in that order; the first goal and final state retain
their positions. Empty sections are omitted. No section is summarized or inferred.

Example from a recorded fixture:

```text
[concordia map] thread fixture - v2 - tools/approvals NOT transferred
goal
goal: fix the login redirect
latest redirect: now the logout
files edited
src/a.rs (2 edits)
src/b.rs (1 edit)
last error
turn 7 -> concordia chat thread read fixture --range 7-7 (verbatim): boom
previous agent final message (agent claim)
turn 9 -> concordia chat thread read fixture --range 9-9: done with it
pointers
assistant: turn 2 -> concordia chat thread read fixture --range 2-2
read pointers with: concordia chat thread read fixture [--range A-B | --tail N | --grep TEXT]
current state: recorded through turn 9
```

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
