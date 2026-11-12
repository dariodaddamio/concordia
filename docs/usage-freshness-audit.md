# Usage cache identity and freshness audit

Current contract reconciled 2026-11-08 (earlier build observations below are historical).

Build `2026-10-31g`: account pickers show one wrapped line per usage window, each with its own
remaining capacity and reset. Unknown reset times are labeled unavailable. Cursor's table header
reset is shared by its monthly categories. Expired readings continue to show usage as unknown.

Scope: `usage.rs`, the read-only Codex RPC reader, and MCP historical usage. Deterministic tests use fake runners, synthetic caches and a fake app-server; live-read evidence is reported separately.

## Cache binding

Codex, Claude and Cursor cache entries are bound to the exact tuple of Concordia account id, provider,
and normalized home path. The cache filename uses the safe account label, provider, and a
standard-library hash of the home string; the serialized entry stores the full identity including
the normalized home path. In-memory key equality and cache validation compare that full path, so a
filename hash collision cannot make one home's values acceptable for another. Identical account
labels under separate providers or homes therefore use different in-memory entries.

The app-owned cache now contains the normalized provider home path as identity metadata. This is
local cache data and is not sent to MCP clients; MCP displays only provider and account labels.

Fetch completion carries the full identity. A delayed worker can only update the matching entry;
changing selection while a fetch runs does not put its result in the selected account's cache. A
matching cache survives process restart. Prior `<account>.json` entries have no identity binding
and are not loaded into the live fetcher; a fresh fetch is requested instead. MCP also excludes these unbound entries.

Codex uses the same account/provider/home-bound fetch cache for live limits. Build
`2026-11-01d` starts the official CLI's app-server, initializes it, then sends only
`account/rateLimits/read`. No thread/start, turn/start, login, logout, reset consumption or email
operation is issued. The official [Codex App Server documentation](https://learn.chatgpt.com/docs/app-server)
defines the quota read and its window/reset fields. Output is bounded to 512 KiB and 128 response
lines per reply, with a 12-second timeout and cleanup of that reader's process tree. The initialize
reply must report the requested physical Codex home before the quota request is sent.

Native recorded files remain a labeled fallback while refreshing or if a live check fails. The
UI retains the recorded reset date/time and last percentage, but does not present an expired
percentage as current usage. The error reason appears in detail. A custom overridden provider
binary disables automatic live reads; fixtures use an explicit test seam. The standalone
`read_codex_usage` and MCP historical reads remain file-only. Codex camelCase server fields and
snake_case recorded fields are both accepted.

Existing home paths are canonicalized, stripped of the Windows extended-path prefix, slash-normalized
and case-folded on Windows. Missing paths retain their normalized lexical identity. Repointing an
alias to a different physical home causes a safe cache miss. Filename hashes can collide and the standard-library hash
algorithm is not a stable cross-version format contract. A collision can overwrite a cache file,
but exact serialized identity validation rejects the wrong entry; a changed hash can cause a safe
cache miss and another fetch.

An entry with a future timestamp is not used by the fetcher and causes a new fetch. MCP excludes future timestamps through the same canonical cache reader.

## Freshness behavior

The fetcher TTL is 120 seconds. A cached value is rendered with age; when expired it is marked
old. If a refresh is pending, the UI-facing summary/detail says `cached; refreshing` and states
that the values are the last successful reading. If refresh fails, it says `cached; refresh failed`
and keeps the last successful values with their age. With no prior value, failure reports
unavailable/not shown. A forced refresh does not erase the last successful cache. The separate
TUI warning helper handles its own account-scoped refresh status; this audit does not assert every
rendering state in that helper.

MCP `usage` never refreshes. It labels entries `recent cache` when their timestamp is less than
120 seconds old, otherwise `stale cache`. It iterates registered accounts and reads only the exact
provider/account/home-bound cache through `cached_usage_view`; it neither scans arbitrary cache filenames
nor starts a provider. Unbound, malformed, mismatched and future entries are excluded. Each read is
bounded to 64 KiB and validates regular owned storage plus bounded, finite percentage metadata. Expired windows
are rendered as current usage unknown rather than as a percentage of capacity after reset. MCP
continues to show stale cached percentages because it is a read-only historical view, with the
stale label beside them.

## Regression evidence

- Existing fake-runner tests cover asynchronous fetch, warm cache on restart, cached-value reuse,
  runner failure and Cursor parsing.
- Added coverage holds a home A fetch while home B finishes for the same account id, then releases
  A and verifies B remains selected and warm after restart. The same case verifies provider changes
  have separate identities.
- Added coverage forces a failed refresh after a success and verifies the successful values remain
  visibly identified as cached.
- MCP tests cover recent and stale bound entries, exclusion of unbound/future entries, reset
  windows without a fabricated percentage, and the read-only behavior (no files are added or
  removed during the call).

These tests establish the listed deterministic paths. They do not exercise concurrent processes
writing the same cache, symlink aliases, filesystem permission failures, arbitrary cache corruption
combinations, or every selected-account timing interleaving. Cache timestamps are wall-clock based;
clock skew causes future entries to be excluded and the fetcher to request a fresh reading.

## Explicit command refresh (2026-10-31q)

`/usage` and `/use` force a fresh check on each invocation (an in-flight fetch is shared).
A background completion rebuilds an open account/usage list while preserving its filter and
selection. Picker rows show the source/refresh status, including when numeric values come from
a previous successful reading. Since build `2026-11-01d`, Codex requests a new server reading on each invocation;
native files are a labeled fallback. This is not an inference call.
The fake-runner test invokes both commands twice and checks four distinct readings in the open list.

RPC regression tests verify home binding, the exact initialize/initialized/quota-read sequence,
server errors, early EOF and oversized output. A separate fake-runner test forces two Codex reads
and observes changed metrics rather than reusing the first response.
