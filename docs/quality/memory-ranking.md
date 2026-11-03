# Historical file facts and explainable ranking

Build `2026-11-02t` adds opt-in M1 ranking. Ordinary `/memory recall` remains lexical.
No provider turn, transcript copy, embedding or referenced-file crawl is needed.

## Controls

`/memory ranked QUERY` searches the current workspace across accounts, retaining each exact
native account/home/session binding. Additional controls accept one
context value followed by the literal query: `ranked-file PATH QUERY`, `ranked-goal GOAL QUERY`,
`ranked-from SOURCE QUERY`, `ranked-at RFC3339 QUERY` and `ranked-source SOURCE QUERY`.
Double quotes group a context value containing spaces; they do not enable shell syntax.
Results explain individual signals and provide exact source/message excerpt commands. Background
publication uses the existing conversation generation checks and leaves the numbered draft intact.

CLI: `concordia chat ranked QUERY [--source ID] [--account ACCOUNT] [--workspace ROOT]
[--goal ID] [--file PATH] [--from ID] [--at RFC3339]`. File context requires an explicit
workspace or source. Explicit source/account mismatches fail. A context source can anchor an
observed cross-account handoff; the result account filter still applies.

`/memory file-facts SOURCE [OFFSET REVISION]` (CLI: `concordia chat file-facts ...`) lists
at most 16 historical path facts per response. Follow the returned offset and revision together.
The native source is reverified on every page; changed snapshots require restarting.

## Evidence and bounds

Only verified Codex/Claude workspace headers and supported structured native tool requests yield
file facts. Shell command strings and arbitrary prose are not path evidence. A tool request does
not establish completion or current disk state. Deleted/nonexistent files remain historical facts.
Sensitive paths, traversal and paths outside the recorded workspace are excluded. Windows ordinal
case comparison respects the recorded namespace; equal filenames in different roots do not match.
Historical aliases are not inferred. Cursor file facts are explicitly unavailable because its
current history adapter does not establish a verified workspace header.

Each source page retains at most 64 facts; overflow and unsupported tool records mark the result
incomplete. The derived metadata cache is version 1, at most 128 KiB per page, contains pointers
and paths rather than transcript bodies, and cannot substitute for source verification. Corrupt,
unknown or conflicting cache data is retained and fails visibly. File-fact responses stay within
28,000 formatted JSON bytes. Source IDs and original native logs remain unchanged.

Ranking inspects at most eight candidate pages, 64 index metadata probes, 64 MiB of indexed
record bytes and twelve hits. Formatted JSON is limited to 64 KiB. Discovery, index construction
and bounded native verification involve separate reads and are explicitly excluded from the
indexed-record byte counter. Limits and unavailable/stale pages mark incomplete coverage.

Scores: confirmed selected-goal association 10,000; explicit or observed handoff 3,000; exact
historical file 1,000; literal case-insensitive text 100; explicit native-time proximity 0 to 49.
Time alone never makes a record relevant. Ties use source ID then message ordinal. Exact native
binding, file identity, span and hash deduplicate overlapping pages. Goal/handoff metadata is
checked again before publication; no signal bypasses native freshness verification.

Native timestamps require an explicit RFC3339 date/offset, with up to nine fractional digits.
Malformed dates, timezone-less values and leap seconds receive no inferred date. Unicode paths
retain their exact source excerpts. Ranking never writes a suggested association as confirmed.

## Validation

Existing Codex/Claude fixtures cover irrelevant keyword collisions, explicit goals and handoffs,
same-named files in different roots, Unicode, timezone/calendar boundaries, deterministic ordering,
stale sources, immutable cache preservation, pagination and paths that do not exist on disk.
Production source `0374dde` passes the enabled 983-test suite, all-target Clippy with warnings
denied, release smoke, portable everyday smoke and OSS gates (328 tracked text files). All
48 installed ConPTY journeys pass in 310.86 seconds; the focused memory journey passes alone
in 9.32 seconds. Release/portable build `2026-11-02t` is 18,792,960 bytes with SHA-256
`226ABF48791F5FD1DA29DD78B573308B13F8BE431B8DFD7AD2C220DE6845098C`.
No real model turn or claim of improved model recall is part of this evidence.
Related-chat grouping (M2) and a versioned retrieval evaluation corpus (M3) remain separate work.
