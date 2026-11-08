# State, integrations, MCP, usage, UI and security

This page covers Concordia's local control plane around provider turns. None of these subsystems is
a hosted memory service.

## State ownership and retention

Concordia's root defaults to `%LOCALAPPDATA%\concordia` and may be overridden by
`CONCORDIA_STATE_DIR`. `.ao/store.json` contains registered accounts, selections and saved settings;
`config.toml` is a user-authored input and is never populated with defaults by the configuration
loader. Source:
[`store.rs` (`default_store_root`, `store_path`, `modify_store`)](../crates/concordia/src/store.rs)
and [`config.rs` (`Config::parse`, `for_root`)](../crates/concordia/src/config.rs).

Retention is intentionally not described as “body-free everywhere”:

- provider native transcripts remain in provider homes;
- thread logging is optional and new records prefer native references, but legacy thread records
  and pinned spans can contain redacted text;
- pane recovery can contain drafts, queues and unfinished requests;
- attachment snapshots contain the user-selected file bytes;
- token logs contain counters/identity metadata, not prompt/reply bodies;
- the event log contains redacted operational events, not prompt/reply text by design.

Source:
[`threads.rs` (`ThreadLog`, `PinnedSpan`)](../crates/concordia/src/threads.rs),
[`tui/recovery.rs` (`RecoveryData`, `RecoveryLease::save`)](../crates/concordia/src/tui/recovery.rs),
[`attachments.rs` (`import`)](../crates/concordia/src/attachments.rs),
[`token_log.rs` (`Entry`)](../crates/concordia/src/token_log.rs), and
[`logging.rs` (`log_path`, `append`)](../crates/concordia/src/logging.rs).

`concordia.log` rotates to `.log.1` at 1 MiB and can be disabled with `CONCORDIA_LOG=off`. It flattens
events to one line and runs local redaction before writing. Source:
[`logging.rs` (`MAX_BYTES`, `disabled`, `append`)](../crates/concordia/src/logging.rs).

## Harness integrations

`src/harness/` is an explicit adapter protocol for a user-owned local integration. A
`harness.json` manifest declares:

- a lowercase name and optional version;
- provider-specific `connect` argv and optional `verify` argv;
- environment-variable **names** that may pass to those commands;
- optional local skill path and exact pinned decisions;
- optional named tools, each with a description and argv.

Only `{provider}`, `{home}` and `{project}` placeholders exist. Commands contain 1–32 arguments;
each argument is at most 4,096 bytes; manifests are at most 256 KiB. Relative programs resolve
beside the manifest. Source:
[`harness/mod.rs` (`MAX_ARGV`, `MAX_ARG_LEN`, `PLACEHOLDERS`, `parse_manifest`,
`expand`)](../crates/concordia/src/harness/mod.rs) and
[`harness/command.rs` (`MAX_MANIFEST_BYTES`, `load_manifest`)](../crates/concordia/src/harness/command.rs).

### What discovery does

F10/`/integrations` and MCP `integrations` call the catalog reader. It reads already recorded
connections and file-backed manifests, validates the saved SHA-256 revision, displays bounded
bindings and eight tools per page, and labels descriptions as untrusted manifest data. It does not
run connect, verify, tool or command-backed-manifest commands. A command-backed manifest stays
“not refreshed” until an explicit Harness action. Source:
[`harness/catalog.rs` (`inspect`, `tool`, `rows`)](../crates/concordia/src/harness/catalog.rs).

### What can execute

Execution happens only for a reviewed `/harness`/integration action:

- `connect` runs the selected manifest's connect argv for the chosen provider/home/project;
- `verify` runs its verification argv and writes a locked-code receipt only for an exact successful
  denial proof;
- `tools NAME` explicitly runs that declared tool;
- a bare program supplied to `connect` may be invoked once with `concordia-manifest` to ask for its
  manifest.

Source:
[`harness/command.rs` (`connect_in`, `verify_in`, `tools_cmd`,
`connect_selected`)](../crates/concordia/src/harness/command.rs) and
[`tui/integrations.rs` (`review`, `start_action`)](../crates/concordia/src/tui/integrations.rs).

The runner passes an argument array without a shell, closes stdin, clears the environment, restores
only a small platform/PATH set plus manifest-allowlisted names, caps stdout and stderr at 64 KiB
each, and kills the tracked process tree after 60 seconds or cancellation. Source:
[`harness/run.rs` (`DEFAULT_TIMEOUT`, `MAX_OUTPUT`, `run_command_cancellable`)](../crates/concordia/src/harness/run.rs).

Harness tools never become an autonomous in-process plugin runtime. Catalog discovery never grants
permission, and provider-native approvals remain authoritative. Pinned decisions are the one model-
context path and are described in [Exactly what Concordia adds to prompts](how-concordia-builds-prompts.md).

## `concordia mcp`

The MCP server is newline-delimited JSON-RPC 2.0 over stdin/stdout. It answers protocol revisions
`2024-11-05`, `2025-03-26` and `2025-06-18`, refuses request lines above 64 KiB, does not support
batches, and caps text results at 20,000 characters. It has no network listener and does not spawn
provider turns. Source:
[`mcp.rs` (`LATEST_PROTOCOL`, `KNOWN_PROTOCOLS`, `MAX_REQUEST_BYTES`,
`MAX_RESULT_CHARS`, `serve`, `handle_line_scoped`)](../crates/concordia/src/mcp.rs).

The nine tools are:

| Tool | Effect |
| --- | --- |
| `threads` | list recent Concordia thread metadata |
| `thread_map` | build one bounded map |
| `thread_read` | bounded tail/grep/range read |
| `native_read` | inspect/read revision-checked native-source indexes |
| `usage` | read cached account limits; never refresh |
| `memory` | local recall/ranking/file/group discovery |
| `goals` | read, or record add/close proposals for local review |
| `delegate` | record a worker proposal for local approval |
| `integrations` | discover bounded Harness catalog metadata; execute nothing |

The seven pure read tools declare read-only, non-destructive, idempotent, closed-world annotations.
`goals` and `delegate` are not marked read-only because they can write proposal files, but neither
applies the proposal. Source:
[`mcp.rs` (`tool_list`, `tool_goals`, `record_pending`, `tool_delegate`)](../crates/concordia/src/mcp.rs).

### Scope and registration

Registration is off by default. With `[threads] mcp = true`, Concordia adds one server entry to
Concordia-owned Codex `config.toml` or Claude `.claude.json`, pointing at the active Concordia
executable with argument `mcp`. It also sets `CONCORDIA_STATE_DIR` and a JSON
`CONCORDIA_MCP_SCOPE` containing the registered account id and canonical workspace. Cursor and
personal provider homes are not modified. Source:
[`integration.rs` (`SERVER_NAME`, `STATE_ENV`, `SCOPE_ENV`, `sync_mcp_for_account`)](../crates/concordia/src/integration.rs).

If scope is missing/malformed, scoped history reads fail closed. Account filters must match the
registered account, and requested workspaces must remain below the registered workspace. Some
global metadata actions are unavailable through scoped MCP. Source:
[`mcp.rs` (`Scope::from_env`, `tool_memory`, `call_tool`)](../crates/concordia/src/mcp.rs).

The serialized tool catalog is tested below 4,000 bytes and has terse descriptions because the
provider may place schemas in model context every turn. See
[`mcp.rs` test `tools_list_has_exactly_the_declared_tools_with_terse_schemas`](../crates/concordia/src/mcp.rs).

## Usage, quotas and token accounting

“Usage” means two different data sets:

1. **Provider quota windows/credits** answer “how much plan allowance remains?”
2. **Turn token events** answer “what did provider streams report for turns run through
   Concordia?”

They are never converted into each other.

### Provider quota sources

| Provider | Current source | Does it send a model prompt? |
| --- | --- | --- |
| Codex | native `codex app-server`: `initialize`, then `account/rateLimits/read`; local session/tmp rate-limit files remain fallback evidence | No |
| Claude | `claude -p /usage --no-session-persistence` in the exact `CLAUDE_CONFIG_DIR` | The code treats this provider-native usage command as a local usage read; no user task is sent |
| Cursor | a private PTY starts the installed agent in a fixed empty probe folder and types interactive `/usage` | No ordinary prompt; it drives the CLI's usage screen |

Source:
[`usage_codex.rs` (`fetch_with`)](../crates/concordia/src/usage_codex.rs),
[`usage.rs` (`run_claude_usage`, `read_codex_usage`)](../crates/concordia/src/usage.rs), and
[`cursor_usage.rs` (`fetch_cursor_usage_text`, `run_usage_in_pty`)](../crates/concordia/src/cursor_usage.rs).

Claude and Cursor reads have a 20-second outer fetch timeout; Cursor's PTY interaction has a
15-second bound. Codex RPC has a 12-second bound and 512 KiB output cap. Successful results are
cached under `<state>/usage-cache`, keyed by provider/account/normalized home and treated as fresh
for 120 seconds. A stale result is labelled stale/refreshing rather than silently presented as
current. The MCP `usage` tool reads cache only. Source:
[`usage.rs` (`USAGE_TTL_SECS`, `FETCH_TIMEOUT`, `UsageIdentity::cache_path`,
`UsageFetcher::snapshot`, `peek_cached_usage`)](../crates/concordia/src/usage.rs),
[`usage_codex.rs` (`fetch_with`)](../crates/concordia/src/usage_codex.rs), and
[`cursor_usage.rs` (`PTY_TIMEOUT`)](../crates/concordia/src/cursor_usage.rs).

The `/usage` screen requests a refresh, account pickers can show cached/background status, and the
header shows provider-reported availability/credits without inventing a numeric balance. Current
refresh behavior is the code above; the separately discussed “live usage refresh” work is not yet
documented as shipped. Source:
[`tui/app/usage_view.rs` (`show_usage`, `open_usage_picker`,
`request_account_usage`)](../crates/concordia/src/tui/app/usage_view.rs)
and [`usage.rs` (`UsageSnapshot`, `credit_balance_text`)](../crates/concordia/src/usage.rs).

### Turn tokens

Structured provider stream usage updates `TurnUsage`; completed main and worker attempts append
provider/account/model/project/chat-bound records to `token-log.jsonl`. Counts retain input,
output, cache read/write and reported cost only where the provider supplied them. Cursor coverage
is not inferred when its transport reports no counters, and Claude native-subagent/resumed-session
coverage is explicitly not invented. Source:
[`provider/stream_cards.rs` (`TurnUsage`)](../crates/concordia/src/provider/stream_cards.rs),
[`token_log.rs` (`Entry`, `record`, `totals`)](../crates/concordia/src/token_log.rs),
[`delegate_accounting.rs` (`finish_worker_attempt`)](../crates/concordia/src/delegate_accounting.rs),
and [`tui/tokens.rs`](../crates/concordia/src/tui/tokens.rs).

Native usage import can scan verified Codex/Claude provider history with checkpoints; Cursor import
is rejected because this transport has no verified usage-event source. Source:
[`native_usage.rs` (`import_page`)](../crates/concordia/src/native_usage.rs).

## UI behavior and keys

The application renders provider text, reasoning and structured tool events itself. A transcript
cache folds after 5,000 rendered rows while retaining a rehydration affordance. Tool cards preserve
full bounded received detail; Ctrl+O toggles expansion and Ctrl+E opens the searchable activity
view. Source:
[`tui/transcript.rs` (`MAX_RENDERED_ROWS`, `Transcript::rebuild`)](../crates/concordia/src/tui/transcript.rs),
[`tui/activity.rs`](../crates/concordia/src/tui/activity.rs), and
[`tui/draw/mod.rs`](../crates/concordia/src/tui/draw/mod.rs).

The major UI surfaces are:

- Home/resume and model/account pickers;
- chat tabs and a separate multiline composer;
- internal preview/raw-editor panes;
- attachment strip and image viewer;
- queue editor and pane-owned recovery chooser;
- worker strip/result viewer and `/agents` picker;
- native approval/plan/question choices, which pause automatic queue dispatch;
- usage, integrations, settings, update and doctor screens.

Source entry points are
[`tui/app/state.rs` (`App`)](../crates/concordia/src/tui/app/state.rs),
[`tui/app/pickers.rs`](../crates/concordia/src/tui/app/pickers.rs),
[`tui/questions.rs`](../crates/concordia/src/tui/questions.rs), and
[`tui/workspace.rs`](../crates/concordia/src/tui/workspace.rs).

For user-facing keys and screen behavior, see [chat rendering](chat-rendering.md),
[TUI states](tui-states.md), [workspace panes](workspace-panes.md), and the current help text in
[`main.rs` (`print_help`)](../crates/concordia/src/main.rs). Important global rules are: Ctrl+C
copies a selection or cancels the owned turn; Shift+Enter/Ctrl+J inserts a newline; Tab completes or
queues while busy; Ctrl+Up/Down moves between composer and queue; Ctrl+G opens agents; Esc closes the
top UI layer before it can affect an underlying draft. Focused editors and native decisions retain
their own arrow/digit handling. Source:
[`tui/input_dispatch.rs` (`dispatch_key_event`,
`dispatch_key_burst`)](../crates/concordia/src/tui/input_dispatch.rs) and
[`tui/questions.rs` (`handle_key`)](../crates/concordia/src/tui/questions.rs).

The proposed `ask-mcp` question bridge is not part of this description. Current structured native
questions arrive over Codex app-server, Claude stdio or Cursor ACP; ordinary conversational choices
are follow-up messages, not approval RPCs.

## Security and provider terms

### Process and filesystem boundaries

- Prompts and adapter commands use argument arrays or stdin, never interpolated shell strings.
- Provider executable discovery rejects project-planted binaries.
- Account/home/session ids are explicit; latest-session heuristics are refused.
- Provider cancellation targets the recorded child tree.
- State writes use bounded inputs, regular-file/reparse checks where needed, locks and atomic
  replacement; destructive account removal is limited to Concordia-owned homes.
- Native provider approvals remain the authority; MCP/Harness discovery grants none.

Source:
[`provider/launch.rs` (`prepare`)](../crates/concordia/src/provider/launch.rs),
[`provider/mod.rs` (`locate`)](../crates/concordia/src/provider/mod.rs),
[`provider/session_guard.rs`](../crates/concordia/src/provider/session_guard.rs),
[`state_io.rs`](../crates/concordia/src/state_io.rs), and
[`connect.rs` (`remove_account`)](../crates/concordia/src/connect.rs).

### Credentials: the precise current behavior

Concordia never copies a Codex/Claude credential file into another home, stores provider access
tokens in its own store/logs, or sends credentials to a Concordia service. Authentication happens
in the official CLI's own login flow, and each turn points the unmodified CLI at the selected home.

However, “Concordia never reads credentials” is not literally true of current source. Login
presence checks are mostly file existence/shape checks, but `identity.rs` opens bounded provider
JSON to derive in-memory email/plan/login labels, decodes selected Codex ID-token claims, checks
Claude access-token presence, and reads an explicit Cursor key-account file. The Cursor key is
registered with runtime redaction and passed only as `CURSOR_API_KEY` to that account's child; an
inherited unrelated `CURSOR_API_KEY` is removed. These values are not written into `store.json`.
Source:
[`accounts.rs` (`is_logged_in`)](../crates/concordia/src/accounts.rs),
[`identity.rs` (`read_json`, `inspect_codex_home`, `inspect_claude_home`,
`cursor_api_key_for_home`)](../crates/concordia/src/identity.rs), and
[`provider/cursor.rs` (`plan_cursor_turn_with`)](../crates/concordia/src/provider/cursor.rs).

Redaction recognizes registered runtime secrets, common provider/token prefixes, authorization and
cookie headers, URL credentials/query secrets, password-like assignments, JWTs and PEM private
keys. It replaces matches with deterministic keyed labels before supported Concordia records and
MCP results are written/returned. Scanning is bounded to the first 64 KiB of each line; therefore
redaction is defense in depth, not permission to put secrets in prompts or worker tasks. Source:
[`redact.rs` (`LINE_LIMIT`, `register_runtime_secret`, `redact`,
`candidate_at`)](../crates/concordia/src/redact.rs).

### Terms boundary

The engineering contract is: run the official unmodified CLIs under the user's own logins; do not
resell, pool or intermediate credentials; preserve provider approvals; and make same-provider
automatic limit handoff separately opt-in with a terms notice. Concordia does not claim that this
engineering design is legal advice. The dated source review and unresolved publication checks are
in [provider terms](provider-terms.md). The enforcing implementation is in
[`connect.rs` (`plan_login`)](../crates/concordia/src/connect.rs),
[`provider/turn.rs` (`run_provider_turn_inner`)](../crates/concordia/src/provider/turn.rs), and
[`store.rs` (same-provider consent functions)](../crates/concordia/src/store.rs).
