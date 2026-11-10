# Local output feed

Concordia can publish a bounded, redacted JSONL feed at `<state>/feed.jsonl`. The file is local,
rotates at 2 MiB, and contains event timestamps, pane identity, kind, and terminal-safe text.
`feed.jsonl.1` is the previous segment. Screen snapshots are written to `screen.txt` only when
the `[feed] screen = true` setting is enabled by the caller. Snapshots sample changed frames at
most once per second; a frame inside that interval is skipped, and idle time alone does not
refresh the file. The file is observation evidence, not a guaranteed current-frame capture.

Use `concordia feed`, `concordia feed --follow`, or `concordia feed --json`. `--since N` skips
the first N events, `--pane ID` filters a pane, and `--screen` reads the latest snapshot.

The MCP `feed` read tool supports `cursor`, `limit`, and `pane`; it is subject to the same local
scope rules as other history reads.

This is an observation feed, not a durable transcript cursor. Event positions can move when
segments rotate. Stream batches are limited to 64 KiB before redaction and carry an explicit
omission notice when over budget; event text remains limited to 4096 characters. Screen snapshots
are limited to 2 MiB and written atomically. Readback refuses files exceeding the rotation budget
plus one 64 KiB record, preserving the files for inspection.
