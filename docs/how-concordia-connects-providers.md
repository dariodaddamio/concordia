# Provider processes, accounts, sessions and approvals

This page follows one turn from provider selection to process exit. The source of truth is
`crates/concordia/src/provider/`; feature names in older design docs are not assumed to exist.

## Executable discovery and account homes

Concordia locates an explicit override first (`CONCORDIA_CODEX_BIN`, `CONCORDIA_CLAUDE_BIN`, or
`CONCORDIA_AGENT_BIN`), then a safe absolute PATH entry, then provider-specific standard Windows
locations. Relative PATH entries, the current directory and binaries planted below a project
workspace are rejected. Source:
[`provider/mod.rs` (`LocateEnv::from_process`, `locate`,
`is_untrusted_path_dir`)](../crates/concordia/src/provider/mod.rs).

Built-in accounts point at the user's existing provider homes and are discovered without claiming
ownership. `/use add` creates later accounts below `<state>/homes/<account-id>`, records that exact
home, and opens the provider's own visible login command. Codex receives `CODEX_HOME`; Claude
receives `CLAUDE_CONFIG_DIR`; the first Cursor account uses `agent login`, while extra Cursor slots
are explicit local API-key folders. Source:
[`accounts.rs` (`builtin_accounts`, `list_all_accounts`)](../crates/concordia/src/accounts.rs) and
[`connect.rs` (`pick_slot`, `plan_login`, `add_account_with`)](../crates/concordia/src/connect.rs).

Only a home below Concordia's `homes` directory and registered as Concordia-owned is eligible for
deletion. A personal/built-in home can be hidden or unregistered, never recursively removed by the
account-removal path. Source:
[`connect.rs` (`is_owned_home`, `removable`, `remove_account`)](../crates/concordia/src/connect.rs).

## Standard text transports

The normal planners make one argument vector, keep the prompt last and set the selected home in the
environment. `provider::launch::prepare` then decides whether that final prompt is safe as an
argument. A real executable accepts up to 24,000 prompt bytes through argv; a `.cmd`/`.bat` shim
gets only a conservative 4,000-byte ASCII-safe subset. Other prompts use stdin for Codex/Claude or
bypass a known Cursor shim through its adjacent PowerShell script. No prompt becomes a shell string.
Source: [`provider/launch.rs` (`BATCH_ARGV_MAX`, `EXE_ARGV_MAX`, `prompt_needs_stdin`,
`prepare`)](../crates/concordia/src/provider/launch.rs).

| Provider | Standard launch | Session binding | Structured output |
| --- | --- | --- | --- |
| Codex | `codex exec --json ... PROMPT` | `resume SESSION` inside `exec` | Codex JSONL |
| Claude | `claude -p --output-format stream-json --verbose ... PROMPT` | `--session-id UUID` on first turn; `--resume SESSION` later | stream-json |
| Cursor | `agent --print --output-format stream-json --stream-partial-output ... PROMPT` | `agent create-chat`, then `--resume CHAT`; exact existing ids use `--resume CHAT` | stream-json |

Exact construction is in
[`provider/codex.rs` (`plan_turn_argv_with`)](../crates/concordia/src/provider/codex.rs),
[`provider/claude.rs` (`plan_claude_turn_with`)](../crates/concordia/src/provider/claude.rs), and
[`provider/cursor.rs` (`plan_cursor_turn_with`, `create_chat`)](../crates/concordia/src/provider/cursor.rs).

Those are the full-capability planner vectors. Concordia caches bounded `--version`/`--help`
probes per binary revision and may remove unsupported streaming/partial-output flags, while keeping
the prompt last. A known CLI that cannot enforce Plan mode fails closed instead of silently running
with broader trust. Source:
[`provider/compat.rs` (`detect`, `degrade_planned`,
`ensure_plan_supported`)](../crates/concordia/src/provider/compat.rs).

If Cursor cannot create a chat id, the standard turn can run unbound; it is not later treated as a
known resumable session. Source:
[`provider/turn.rs` (`run_provider_turn_inner`, Cursor `create_chat` branch)](../crates/concordia/src/provider/turn.rs).

## Native interactive transports

`/native ACCOUNT on` saves a transport preference for future tasks. Cursor ACP is the default for
Cursor; Codex app-server and Claude native stdio are opt-in. `/native ... off` selects standard
text transport. The preference is bound to provider/account/home and capped at 16 KiB. Source:
[`native_input.rs` (`Mode`, `default_for`, `load`, `command`)](../crates/concordia/src/native_input.rs).

These are not silent fallbacks to a different model path:

- **Cursor ACP** launches `agent [trust flags] acp`, negotiates protocol 1, creates or loads an
  exact session, sets mode/model, then sends `session/prompt`. It is used for Cursor images and by
  the default Cursor preference. A missing saved session may fall back to standard print mode only
  for a text-only turn; other ACP failures are reported. Source:
  [`provider/turn_acp.rs` (`acp_argv`, `run`)](../crates/concordia/src/provider/turn_acp.rs) and
  [`provider/turn.rs` (`run_provider_turn_inner`)](../crates/concordia/src/provider/turn.rs).
- **Codex app-server** is version-gated to the qualified `codex-cli 0.166.0`. It verifies that the
  initialized app-server reports the expected `CODEX_HOME`, starts/resumes the exact thread,
  verifies cwd and approval policy, and sends `turn/start`. A transport failure does not start a
  second fallback turn. Source:
  [`provider/turn_appserver.rs` (`run`)](../crates/concordia/src/provider/turn_appserver.rs).
- **Claude native stdio** uses the Agent SDK stdio control protocol and is version-gated by
  `turn_claude::SUPPORTED_VERSION`. It carries native permission/question traffic and waits for
  tracked background work to settle. Source:
  [`provider/turn_claude.rs` (`SUPPORTED_VERSION`, `initialize_request`, `user_message`,
  `run`)](../crates/concordia/src/provider/turn_claude.rs).

## Trust and approval translation

Trust modes have an ordered local meaning, but providers do not expose the same controls.

| Concordia mode | Codex standard | Claude | Cursor standard/ACP |
| --- | --- | --- | --- |
| Plan | `--skip-git-repo-check -s read-only` | `--permission-mode plan` | `--mode plan`; ACP sets session mode `plan` |
| Ask | same read-only Codex sandbox (Codex exec has no separate plan/ask flag) | `--permission-mode manual` | no extra print flag; ACP uses agent mode and native permission requests |
| Workspace | `--skip-git-repo-check --approve-for-me` | `--permission-mode acceptEdits` | default workspace sandbox; never `--sandbox disabled` |
| Full | `--skip-git-repo-check --dangerously-bypass-approvals-and-sandbox` | `--dangerously-skip-permissions` | `--force` |

The planners are
[`codex::plan_turn_argv_with`](../crates/concordia/src/provider/codex.rs),
[`claude::plan_claude_turn_with`](../crates/concordia/src/provider/claude.rs), and
[`cursor::plan_cursor_turn_with`](../crates/concordia/src/provider/cursor.rs).

Codex app-server derives a native `sandboxPolicy` and `approvalPolicy` from the final enforced
plan. Read-only uses `approvalPolicy: never`; Workspace uses workspace-write plus `on-request` and
keeps the user as reviewer; Full uses danger-full-access. Native requests are scoped to the exact
connection/run/attempt before the UI can reply. Source:
[`provider/turn_appserver.rs` (`run`)](../crates/concordia/src/provider/turn_appserver.rs) and
[`provider/interaction.rs` (`NativeEnvelope`, `NativeRequest::respond_owned`)](../crates/concordia/src/provider/interaction.rs).

A project containing the locked-code policy can lower a requested write-capable turn to read-only
unless a current Harness verification receipt matches the provider, home and policy revision.
Planning then checks that the resulting CLI argv is genuinely read-only before launch. Source:
[`provider/enforcement.rs` (`effective_trust`, `ensure_read_only`)](../crates/concordia/src/provider/enforcement.rs)
and [`provider/turn.rs` (`plan_provider_turn`)](../crates/concordia/src/provider/turn.rs).

## Images, documents and PDFs

One message has at most four attachments. Each image is at most 4 MiB; UTF-8 documents share a
64 KiB message budget; each PDF is at most 8 MiB and PDFs share a 16 MiB message budget. Imported
files become hash-named snapshots under `<state>/attachments`, so a later source-file edit cannot
silently change a queued attachment. Source:
[`attachments.rs` (`MAX_IMAGES`, `MAX_BYTES`, `Message::validate`,
`import`)](../crates/concordia/src/attachments.rs) and
[`attachment_documents.rs` (`MAX_DOCUMENT_BYTES`, `MAX_PDF_BYTES`,
`MAX_PDF_TOTAL_BYTES`)](../crates/concordia/src/attachment_documents.rs).

- Codex standard adds repeated `--image PATH` arguments; app-server sends data-URL image blocks.
- Claude standard converts image/document content to stream-json stdin; native stdio uses content
  blocks. Qualified Claude native transport is required for PDFs.
- Cursor images require ACP and advertised `promptCapabilities.image`.

Source:
[`provider/turn.rs` (`attach_images`, `pdf_message`)](../crates/concordia/src/provider/turn.rs),
[`provider/turn_appserver.rs` (`run`)](../crates/concordia/src/provider/turn_appserver.rs),
[`provider/turn_claude.rs` (`run`)](../crates/concordia/src/provider/turn_claude.rs), and
[`provider/turn_acp.rs` (`run`)](../crates/concordia/src/provider/turn_acp.rs).

## Resume and session safety

Native resume occurs only when provider, normalized home and a nonempty explicit session id all
match. Otherwise the session id is cleared. Claude chooses a UUID before its first turn, Cursor ACP
gets the id from `session/new`, Cursor standard attempts `create-chat`, and Codex reads the explicit
thread id from its structured stream/app-server response. Source:
[`context.rs` (`should_attempt_native_resume`, `plan_continuity_capped`)](../crates/concordia/src/context.rs),
[`provider/claude.rs` (`plan_claude_turn_with`)](../crates/concordia/src/provider/claude.rs), and
[`provider/turn.rs` (`TurnProgress::Session` emission)](../crates/concordia/src/provider/turn.rs).

The runner refuses ambiguous convenience forms: Codex `--last`/`--continue`, Claude
`--continue`/`-c` or valueless `--resume`, and Cursor latest-chat `resume`, `--last`, `--continue`
or valueless `--resume`. Source: each provider's `assert_safe_*_argv` function in the planner files
linked above.

## Cancellation, errors and limits

`RunningTurn` holds a cancellation flag, process id, owned child and wake notification. Cancellation
is idempotent. On Windows `kill_process_tree` invokes `taskkill /PID <id> /T /F` as an argument
array, waits up to two seconds, then kills/reaps the direct child; readers also wake immediately.
Only the pid recorded for that turn is targeted. Source:
[`provider/turn.rs` (`RunningTurn`, `kill_process_tree`, `KILL_GRACE`,
`REAP_TIMEOUT`)](../crates/concordia/src/provider/turn.rs).

The standard stream keeps at most 12 stderr lines, bounds an individual output line at 1 MiB and
the raw Codex fallback buffer at 4 MiB. Structured terminal-failure events are distinct from a
nonzero exit code. Source:
[`provider/turn.rs` (`STDERR_KEEP_LINES`, `LINE_MAX`, `RAW_MAX`,
`terminal_failure`)](../crates/concordia/src/provider/turn.rs).

`provider::failure::classify_failure` prefers structured provider error codes, then bounded stderr
and result text, and distinguishes usage exhaustion, server overload/capacity, context exhaustion,
authentication and ordinary failure. The TUI pauses queued work, preserves the unfinished request,
and stops automatic continuation for cancellation, uncertain tool completion or storage failure.
Recognized context exhaustion permits one fresh-session rollover with exact-source pointers;
usage-limit account/model handoff is manual by default. Source:
[`provider/failure.rs` (`classify_failure`)](../crates/concordia/src/provider/failure.rs) and
[`tui/continuation.rs` (`prepare_usage_continuation`, `resume_deferred_usage`,
`continue_usage`)](../crates/concordia/src/tui/continuation.rs).

Same-provider automatic account rotation is separately opt-in and also requires recorded
provider-specific terms acceptance. Automatic targets are bounded by tried account/model pairs and
a per-account daily cap. Source:
[`continuity.rs` (`automatic_rules_respect_same_provider_opt_in`)](../crates/concordia/src/continuity.rs),
[`store.rs` (`same_provider_automatic`, `same_provider_terms_accepted`,
`automatic_handoff_allowed`)](../crates/concordia/src/store.rs), and
[`config.rs` (`Continuation`)](../crates/concordia/src/config.rs).
