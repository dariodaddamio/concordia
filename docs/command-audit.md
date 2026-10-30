# Command and state audit

Audit scope: repository source and tests as present on 2026-10-16. This is an inventory and a
targeted behavior audit, not proof over every possible input or every provider state. All tests
mentioned here use pure parser/dispatch paths, temporary synthetic state, or existing fake-based
suites. No login, real provider turn, real account change, or update/restart was run.

Follow-up integration on build `2026-10-16c` added native-chat/preview CLI routes, strict extra
argument checks before top-level actions, stricter slash/delegation validation, and usage identity/
freshness checks. Local build/install checks are recorded separately in interaction-audit.md;
the inventory's limitations are not an assertion that every listed behavior was exercised live.

## Top-level CLI

Defined in `crates/concordia/src/main.rs`:

| Invocation | Behavior | Evidence / limitation |
| --- | --- | --- |
| `concordia` | Starts TUI | TUI journeys and fake-provider suites cover selected flows; no live terminal/provider verification here. |
| `--host`, `host` | Alias for TUI | Source branch only; no dedicated process-level alias test found. |
| `--help`, `-h`, `help` | Prints help and active paths | Can be run read-only. Help omits several TUI commands documented below. |
| `--version`, `-V`, `version` | Prints build stamp and executable | Read-only behavior is documented in source; smoke script covers binary contract. |
| `install` | Installs executable, may alter PATH/adopt old state | Not invoked; this changes machine state. Release smoke exercises controlled install fixtures. |
| `wezterm` | Launches a WezTerm window | Not invoked; no direct command behavior test found. |
| `doctor`, `--doctor` | Prints diagnostics and exits nonzero when unhealthy | Fast-report tests exist; process exit contract is in smoke scripts. |
| `thread list [--json]` | Lists recorded Concordia threads | `threads` unit tests exercise formatting and errors; list accepts only `--json`. |
| `thread map ID [--max-chars N]` | Bounded map of a recorded thread | `threads` tests cover resolver, map, limits and invalid options. |
| `thread read ID [--tail N] [--grep TEXT] [--range A-B] [--max-chars N]` | Bounded transcript read | `threads` tests cover range/grep/tail parsing and output. Duplicate options currently use the last value; combinations are not exhaustively covered. |
| `mcp` | JSON-RPC stdio server | `tests/mcp.rs` launches a child against synthetic state and exercises initialize/list/calls/unknown method. |
| `chat list|search|read` | Explicit native-account chat access | Synthetic two-account tests bind reads to account plus session ID, including identical native IDs. Search is metadata-only; Cursor text reads are unsupported. Argument validation happens before discovery. |
| `preview FILE` | Read-only terminal text/Markdown viewer | ConPTY journey verifies top-of-file, scrolling and exit with no provider work; UTF-8, 2 MiB cap. |
| Other arguments | Error, exit code 2 | Source branch; no broad process-level matrix found. |

CLI argument arrays and exit statuses are owned by `main.rs`/`threads.rs`, outside this task's
implementation scope. Help says thread reads are opt-in with `CONCORDIA_THREADS=1`; MCP thread
read tools read already-recorded state and do not start provider work.

## TUI slash surface

`commands.rs::SLASH_COMMANDS` exposes exactly these twelve screens/actions. Former subcommands
remain accepted only as migration routes and print where their behavior moved.

| Command | Accepted forms / aliases | Actual route |
| --- | --- | --- |
| `/use` | Switch, add or remove accounts; the usage-handoff toggle and continuation picker live here. |
| `/model` | Model picker. |
| `/mode` | Permission mode picker/setter, including former trust arguments. |
| `/usage` | All-account usage screen; `r` refreshes. |
| `/new` | Starts a fresh chat. |
| `/resume` | Explicit session picker; Delete confirms thread removal. |
| `/delegate` | Worker screen/actions; task text remains a shortcut. |
| `/harness` | Harness connect and tool dispatch. |
| `/open` | File side pane; Esc or X closes it. |
| `/copy` | Copies the last assistant reply. |
| `/doctor` | Setup, version and configuration diagnostics. |
| `/update` | Update check and confirmation/install flow. |

Additional TUI intercepts in `tui/mod.rs::handle_slash` do not pass through the parser:

| Command family | Behavior and evidence |
| --- | --- |
| Removed command routes | `/trust`, `/continuation`, `/continue`, `/threads`, `/preview`, `/config`, `/version`, `/agents`, `/usage all`, `/usage refresh`, `/update check` and `/exit` print one line naming their replacement and route to it. `/workers`, `/queue`, `/edit`, `/close`, `/split`, `/stats` and `/recover` are unlisted but still work, until the screens that replace them exist. |
| `/queue`, `/queue resume`, `/queue clear`, queue controls | Lists, resumes or clears queued prompts. `queue_controls` and TUI tests cover busy/cancel/edit blocking and state changes. |
| `/recover`, `/recover legacy`, `/recover retry` | Workspace draft/queue recovery, explicit legacy import, retry local persistence. Recovery TUI tests cover selected missing/corrupt/retry states. |
| `/stats` | Toggles local process counters; special-cased after the parser returns unknown. TUI tests cover the toggle. |
| `/open FILE`, `/preview FILE`, `/close ID`, `/edit FILE` | Source `2026-10-21a`: bounded internal read-only split panes, explicit close, and separate external editor via `/edit`. Synthetic input/render tests exercise focus, resize and close; real host/editor interaction still needs a human pass. |

Palette entries are `commands.rs::SLASH_COMMANDS`; this list is not the complete parser surface:
it omits aliases and, currently, `/doctor` palette/help consistency has changed over time. Custom
commands are separate from built-ins: files at `<state>/commands/<name>.md`, names restricted to
`[a-z0-9-]{1,32}`, at most 64 files and 16 KiB per file. Optional `description` front matter is
supported; `$ARGS` is substituted once, otherwise typed arguments are appended. Built-ins win
name clashes. `user_commands.rs` tests cover parsing, bounds, substitutions, clashes and absent
directories. Scanning can leak command/description strings for process lifetime by design.

## MCP surface

`mcp.rs::handle_line` implements JSON-RPC methods `initialize`, `ping`, `tools/list`, and
`tools/call`. Unknown methods return `-32601`; invalid tool name/argument shape returns `-32602`;
tool-level failures return a text result with `isError: true`. Requests cap at 64 KiB and tool
text results at 20,000 characters. Notifications receive no response. Batch requests are rejected.

`tools/list` advertises exactly these tools:

| Tool | Data/action | Existing evidence / gap |
| --- | --- | --- |
| `threads` | List recorded thread id/age/title, requested limit clamped to 1..30 | MCP unit and child-process tests cover basic listing; malformed `limit` types silently use default. |
| `thread_map` | Bounded summary for explicit unique thread id prefix | MCP tests cover matching and missing IDs. Ambiguous prefix and corrupt log behavior is not broadly table-tested. |
| `thread_read` | Read explicit thread, optional tail/grep/range/max chars | MCP tests cover a grep read. Wrong-typed optional fields are ignored/defaulted; cross-option interactions are not exhaustive. |
| `usage` | Reads cached `<state>/usage-cache/*.json`; no refresh | MCP tests cover availability; source labels recent/stale/unknown-age and identity; see usage-freshness-audit.md. Malformed files are skipped. |
| `delegate` | Writes a bounded proposal file for user confirmation; does not execute task | MCP tests verify off-by-default refusal and no proposal written; source checks nonempty target/task, max lengths and proposal count. Concurrency/race behavior is not stress-tested. |

## Findings and coverage limits

- Fixed a safety-relevant parser fallthrough: `/update typo` and `/update check now` previously
  started an update; `/usage typo` displayed ordinary usage; `/split typo` silently split
  horizontally. `SlashCommand::Invalid` now produces a visible argument error for those forms.
  Integration tests cover these cases and preserve valid update aliases/orientations.
- The follow-up now rejects extra words for simple commands (`/model`, `/resume`, `/trust`, `/copy`,
  `/doctor`, `/new`, `/mode`, and `/use`). Top-level commands without operands reject extras before effects. This audit
  does not claim these are all desirable or that their combinations are verified.
- Follow-up regressions reject `/delegate protocol nonsense` and `/workers nonsense`; invalid
  values no longer map to a query/list. Dedicated parser tests cover strict
  argument errors.
- Busy checks before conversation/provider/model switching are in `tui/mod.rs`; update rejects a
  busy turn and duplicate update jobs. Existing TUI tests cover selected busy/cancel/update/restart
  paths. Slash operations not listed in the guard remain available while busy; account removal,
  usage refresh and trust/mode changes merit explicit policy review.
- Account and session pickers are UI state machines; tests cover selected picker and removal flows,
  not every account disappearance, provider cache invalidation, concurrent state edit, or native
  CLI session ambiguity. Live-version validation of Claude/Cursor native session-id binding remains an acknowledged product
  limitation in the release blockers.
- Usage values come from provider-written/cache files. Current-account display and explicit refresh
  have selected tests, and MCP is intentionally cached-only. The follow-up usage audit covers
  identity, late replies, age/failure/reset/future data. Further timing and corruption combinations
  still need coverage.
- Tests by name are not proof of all parser permutations, state transitions, platform behavior, or
  provider compatibility. No exhaustive Cartesian test over every command argument, account state,
  cache age, busy/cancel timing, or malformed filesystem state was run.

## Historical release evidence

`docs/release-blockers.md` retains earlier CI/release rehearsal results as historical evidence.
This pass did not independently rerun remote workflows. Local tests and build checks do not
establish native approval parity for headless execution; the updated release blockers keep
those distinctions and the remaining command/state coverage gaps explicit.
