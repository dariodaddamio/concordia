# Native worker input and activity presentation

Scoped source record: `2026-11-09a`; see [verification](../e2e-status.md) for the current build.
That source pins Claude's native-stdio contract to CLI `2.1.319`. An
initialize-only exchange was observed; live 2.1.319 permission/question, background-task and
capacity parity remain unqualified. Earlier 2.1.318 process/live receipts below remain historical.

This feature record began at main `c5e34f3`, connecting the interaction contract to workers and
making activity, edits, agent results and account limits easier to inspect.

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

Native background completion is retained only for the SDK's bounded `local_agent` and
`local_workflow` follow-up lifecycles. A foreground/worker owner may keep the completion channel
open for at most 10 minutes while those local workflows finish; expiry, disconnect or an
unreconciled first result is reported as uncertain/failed and requires explicit reconciliation,
never a success claim or automatic replay. Long-lived remote tasks, shell commands and monitors
are outside this retention contract.

[Codex app-server input](codex-native-input.md) extends this contract behind explicit account/home
selection. Claude has a separately gated stdio contract. The historical installed 2026-11-04b
candidate used Claude CLI `2.1.318`; the matching Python SDK v0.2.178 defined the same
`--input-format stream-json --output-format stream-json
--permission-prompt-tool stdio`; after an `initialize` control exchange, top-level
`control_request`/`can_use_tool` frames carry a request id, tool name, original input and
tool-use id, and the only native response is a matching `control_response`. The adapter keeps
the original input for an allow-once decision and denies unsupported/cancelled requests. This
is sourced from the official Claude Agent SDK transport/types and the [native permission
documentation](https://code.claude.com/docs/en/agent-sdk/user-input); no model turn was run.
The bridge remains opt-in and version/capability gated. An unsupported opted-in version stops
before dispatch; `/native ACCOUNT off` selects the existing standard adapter. It never silently
falls back from an approval-capable transport. Duplicate IDs receive no second grant. Native
cancellation invalidates pending review while stdout continues being consumed; closing or losing
the provider leaves uncertain completion paused. Image content is preserved. Structured Claude
`AskUserQuestion` is supported in the broker using the versioned shape exercised by the historical
2.1.318 fixtures; the initialize-only 2.1.319 observation does not qualify live question handling.
The broker preserves original question text/options and returns answers as strings; multi-select
answers are joined with `, `. Malformed, unsupported or cancelled payloads are denied, and no HTML
preview is rendered. This follows the official [user-input contract](https://platform.claude.com/docs/en/agent-sdk/user-input)
and [AskUserQuestion preview example](https://github.com/anthropics/claude-agent-sdk-demos/tree/main/ask-user-question-previews).
The review surface supports up to four questions, each with two to four choices. Space toggles
the focused choice and Enter confirms; mouse clicks toggle choices and the confirm control submits
the reviewed set. A free-text `Other` response reuses the existing note row rather than opening
HTML or a second editor. The same bounded ownership, cancellation and at-most-once response
rules apply to these answers.
The historical d322 native source checks passed 26 native unit cases plus extended foreground/worker process
fixtures, including background, cancellation, foreign-owner and EOF cases. No live model turn or
physical-host parity is claimed;
exact historical artifact evidence is in [E2E status](../e2e-status.md). Source evidence: [Python
v0.2.178 CLI version](https://raw.githubusercontent.com/anthropics/claude-agent-sdk-python/v0.2.178/src/claude_agent_sdk/_cli_version.py),
[query control methods](https://raw.githubusercontent.com/anthropics/claude-agent-sdk-python/v0.2.178/src/claude_agent_sdk/_internal/query.py),
and [TypeScript v0.3.318 release parity](https://github.com/anthropics/claude-agent-sdk-typescript/releases). That historical build source was `22b4da5`; exact artifact identity and receipt remain in E2E status.

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
keep their existing meanings. Provider-native commands stay available
by explicit prefix but are absent from the bare slash menu. Agents use the read-only `memory` MCP
tool; relationship confirmation and native grants remain explicit user decisions.

## Account limits

The live Codex quota cache now retains native credits and exact reset/limit fields. Credit balance
is distinct from subscription percentages and turn-token accounting; spent credits are not inferred.
Idle UI refresh revisits the bounded quota TTL, and finished workers invalidate their account's
reading. Cached or failed refreshes retain freshness labels. No model calls or estimated pricing.

## Verification

Historical production source `24e6d9d`, build `2026-11-03e`, passed all 983 enabled checks through everyday
smoke, all-target Clippy and OSS gates (340 tracked text files). The isolated installed candidate
is 19,097,088 bytes, SHA-256 `19dd5d649822015c9750edfcc4321cf37f92f45ed7eb2ffb4a0d0dfd6f1b2097`.
Forty-seven installed terminal journeys pass in the full run; the extended worker journey passes
separately in 26.11 seconds after correcting its selected-worker gesture and waiting for the
existing debounced draft persistence before asserting on disk. Those corrections change only the
test. Its native permission and session-less plan rejection both preserve the main draft, and no
main turn or print fallback is launched. Final remote CI remains a separate recorded gate.

## Scoped 05c native evidence

Historical 05c production-transport checks qualified Codex offered cancellation and Claude structured Review
answers with wrong-owner and duplicate-response rejection. The cancelled command was not granted;
sentinel files and the main selection stayed unchanged. These helper-driven calls do not establish
physical worker-window parity. A no-model worker-window rehearsal stopped at its fake executable's
unverified version gate before issuing a native request; no fallback or live retry was started.
Original driver failures and separate manual corrections are retained in
[the Git-preserved historical journal](../e2e-status.md#earlier-evidence).
