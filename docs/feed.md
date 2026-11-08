# Local output feed

Concordia can publish a bounded, redacted JSONL feed at `<state>/feed.jsonl`. The file is local,
rotates at 2 MiB, and contains event timestamps, pane identity, kind, and terminal-safe text.
`feed.jsonl.1` is the previous segment. Screen snapshots are written to `screen.txt` only when
the `[feed] screen = true` setting is enabled by the caller.

Use `concordia feed`, `concordia feed --follow`, or `concordia feed --json`. `--since N` skips
the first N events, `--pane ID` filters a pane, and `--screen` reads the latest snapshot.

The MCP `feed` read tool supports `cursor`, `limit`, and `pane`; it is subject to the same local
scope rules as other history reads.
