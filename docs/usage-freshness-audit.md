# Usage cache identity and freshness audit

Scope: `usage.rs` fetch/cache state and the MCP `usage` tool. UI snapshot/window fields remain
unchanged. Verification uses fake runner closures and temporary synthetic cache files; it does not
launch a provider CLI or probe live quota.

## Cache binding

Claude and Cursor cache entries are bound to the exact tuple of Concordia account id, provider,
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
and are not loaded into the live fetcher; a fresh fetch is requested instead. They remain visible
in MCP as legacy data with an explicit unknown identity label.

Codex remains read directly from provider-written home files and does not use this fetcher/cache.
The existing `UsageSnapshot`, `UsageWindow`, picker row and TUI consumers retain their fields.
`invalidate(account)` marks all in-memory entries with that Concordia account id dirty, so a later
read of each provider/home binding refreshes it.

The normalized path is slash-normalized and case-folded on Windows; it does not resolve symlinks.
Pointing two path aliases at one physical home may create separate cache entries, which can cost a
refresh but does not mix identities. Filename hashes can collide and the standard-library hash
algorithm is not a stable cross-version format contract. A collision can overwrite a cache file,
but exact serialized identity validation rejects the wrong entry; a changed hash can cause a safe
cache miss and another fetch.

An entry with a future timestamp is not used by the fetcher and causes a new fetch. MCP retains it
only as a cache record with `cache age unknown` so it does not present the clock-skewed entry as
recent.

## Freshness behavior

The fetcher TTL is 120 seconds. A cached value is rendered with age; when expired it is marked
old. If a refresh is pending, the UI-facing summary/detail says `cached; refreshing` and states
that the values are the last successful reading. If refresh fails, it says `cached; refresh failed`
and keeps the last successful values with their age. With no prior value, failure reports
unavailable/not shown. A forced refresh does not erase the last successful cache. The separate
TUI warning helper handles its own account-scoped refresh status; this audit does not assert every
rendering state in that helper.

MCP `usage` never refreshes. It labels entries `recent cache` when their timestamp is less than
120 seconds old, otherwise `stale cache`; absent or future timestamps are `cache age unknown`.
Bound entries identify provider and account. Legacy entries say `identity unknown`. Expired windows
are rendered as current usage unknown rather than as a percentage of capacity after reset. MCP
continues to show stale cached percentages because it is a read-only historical view, with the
stale label beside them.

## Regression evidence

- Existing fake-runner tests cover asynchronous fetch, warm cache on restart, cached-value reuse,
  runner failure and Cursor parsing.
- Added coverage holds a home A fetch while home B finishes for the same account id, then releases
  A and verifies B remains selected and warm after restart. The same case verifies provider changes
  have separate identities.
- Added coverage seeds an old unbound cache and verifies the fetcher requests a bound reading
  instead of showing the legacy percentage.
- Added coverage forces a failed refresh after a success and verifies the successful values remain
  visibly identified as cached.
- MCP tests cover recent, stale and unknown-age cache entries, bound versus legacy identity, reset
  windows without a fabricated percentage, and the read-only behavior (no files are added or
  removed during the call).

These tests establish the listed deterministic paths. They do not exercise concurrent processes
writing the same cache, symlink aliases, filesystem permission failures, arbitrary cache corruption
combinations, or every selected-account timing interleaving. Cache timestamps are wall-clock based;
clock skew can result in the deliberately conservative `cache age unknown` label for future data.
