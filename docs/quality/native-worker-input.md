# Native worker input and activity presentation

Baseline: main `c5e34f3`. This slice connects the existing interaction contract to worker
execution and makes activity, edits, agent results and account limits easier to inspect.

## Native execution

`/native ACCOUNT on|off|status` controls future Cursor text turns for that exact isolated home.
The default stays the existing adapter. Opt-in tasks capture their transport in worker ledger v7;
v1–v6 ledgers still load. Preferences are versioned, bounded, locked and atomically saved. Unknown
or corrupt preferences are retained and fail visibly. No authentication or global provider
configuration changes. Runtime protocol negotiation remains authoritative.

Main and worker Cursor ACP turns support text and images, exact session load, native permissions,
questions and documented `cursor/create_plan` acceptance/rejection. Session-less Cursor extensions
require a negotiated protocol-1 connection, established session and native tool-call identity;
an explicit wrong session is refused. Requests during history load cannot obtain approval.
Plan text is available for review before an explicit choice. Unsupported methods receive errors.
See the [native extension contract](https://cursor.com/docs/cli/acp#cursor-extension-methods).

Workers enter `waiting for native input`. Select a worker and press F8 to review, then F8 to enter
its options. Merely opening, typing a digit or closing the review does not grant permission.
Escape leaves the request pending and preserves the main draft. Every choice checks conversation,
run, attempt, account/home, execution workspace, native session, connection and request identity.
Eight pending requests per worker and 24 across workers preserve foreground capacity. Cancellation,
owner changes and connection loss invalidate channels. Restart pauses work and never replays grants.
All native-mode tasks disable automatic worker failover; uncertain completion retains scope.
Explicit cancellation with a confirmed process exit can release it.

[Codex app-server input](codex-native-input.md) extends this contract behind explicit account/home
selection. A provenance-checked Claude permission bridge remains separate. No real provider turn
or live parity claim is part of these implementations.

## Activity and navigation

Codex structured file changes show exact reported paths, added/removed counts and readable diffs.
Native Edit old/new strings render as replacements while retaining original input/output. Missing
file line numbers stay unavailable. Click a card to review colored additions/removals; click its
underlined `File:` row to open the current file. Deleted or unavailable files keep their historical
diff. The current preview does not claim to be the historical version.

JSON-quoted Windows paths are decoded before resolution, including spaces and extensionless paths.
Resolution still uses the exact absolute path or current execution directory, without basename
search or filesystem crawling. Tool labels describe reads, edits, searches and commands while
retaining native identities. The terminal title and activity indicator distinguish thinking,
writing, reading, searching, editing and native input.

Ctrl+G opens the agent inspector as a right-side panel on wide terminals. It retains tasks, latest
progress and completion; worker details lead with the result and state whether it was returned to
the main agent. Native provider subagents retain their own result ownership. Narrow terminals use
the available width. No executor or result delivery is added by inspection.

Down from the last queued item returns to the composer. Inline queue editing and Alt+Down reorder
keep their existing meanings. Intricate memory/context/scope/worktree/Harness commands stay available
by explicit prefix but are absent from the bare slash menu. Agents use the read-only `memory` MCP
tool; relationship confirmation and native grants remain explicit user decisions.

## Account limits

The live Codex quota cache now retains native credits and exact reset/limit fields. Credit balance
is distinct from subscription percentages and turn-token accounting; spent credits are not inferred.
Idle UI refresh revisits the bounded quota TTL, and finished workers invalidate their account's
reading. Cached or failed refreshes retain freshness labels. No model calls or estimated pricing.

## Verification

Production source `24e6d9d`, build `2026-11-03e`, passes all 983 enabled checks through everyday
smoke, all-target Clippy and OSS gates (340 tracked text files). The isolated installed candidate
is 19,097,088 bytes, SHA-256 `19dd5d649822015c9750edfcc4321cf37f92f45ed7eb2ffb4a0d0dfd6f1b2097`.
Forty-seven installed terminal journeys pass in the full run; the extended worker journey passes
separately in 26.11 seconds after correcting its selected-worker gesture and waiting for the
existing debounced draft persistence before asserting on disk. Those corrections change only the
test. Its native permission and session-less plan rejection both preserve the main draft, and no
main turn or print fallback is launched. Final remote CI remains a separate recorded gate.
