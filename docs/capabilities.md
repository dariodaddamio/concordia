# P0 provider capability matrix

Package: **P0** (Integration investigator). Dependencies: none.  
Owned paths: `docs/capabilities.md`, `docs/evidence/p0/**`.  
Research date: 2026-10-14. Host: Windows 11 (10.0.26200).

## Versions checked (local)

| Binary | Version | Path (observed) |
| --- | --- | --- |
| `codex` | `codex-cli 0.162.1` | `%AppData%\Roaming\npm\codex.ps1` → npm `@openai/codex` |
| `claude` | `2.1.300 (Claude Code)` | `%USERPROFILE%\.local\bin\claude.exe` |
| `agent` / `cursor-agent` | `2026.10.14-0f9edde` | `%LOCALAPPDATA%\cursor-agent\` |

Auth presence (no secrets captured): Codex `login status` → ChatGPT login on default home; Cursor `status`/`about` → authenticated Pro. No live model turns or quota-consuming probes were run.

Raw command outputs: `docs/evidence/p0/`.

## Evidence sources

| Source | Role |
| --- | --- |
| [Codex app-server](https://learn.chatgpt.com/docs/app-server) | Official JSON-RPC: threads, turns, approvals, account/rate limits |
| [Claude headless / programmatic](https://code.claude.com/docs/en/headless) | `-p`, resume by session id, permissions, interrupt, cost fields |
| [Claude env vars](https://code.claude.com/docs/en/env-vars) | `CLAUDE_CONFIG_DIR` multi-account isolation |
| [Cursor ACP](https://cursor.com/docs/cli/acp) | `agent acp`, session/new|load, permissions, cancel |
| Local `--help` / `--version` | Flags actually present on installed binaries |
| `CODEX_HOME` temp-dir probe | Account home isolation without writing user credentials |

Third-party tools (`codexswitch`, `ccswap`, `agent-intern`) are **reuse candidates only**; not validated as orchestrator dependencies. `codexswitch` recovery via `resume --last` is an **anti-pattern** for this product.

---

## Capability matrix

Legend: **S** supported (docs +/or local flags), **U** unknown/unproven on this machine, **N** unsupported or anti-pattern for our contract.

### 1. Native UI vs programmatic protocol

| Provider | Native interactive | Programmatic / deep integration | Same runtime? |
| --- | --- | --- | --- |
| **Codex** | TUI (`codex`, `codex resume …`) | **app-server** JSON-RPC (stdio/unix/ws); also `codex exec` JSONL | Docs: app-server powers rich clients (e.g. VS Code). CLI can attach via `--remote`. Shared local daemon exists (`app-server daemon`, `agents`). |
| **Claude Code** | Interactive REPL | `-p` / `--print` + Agent SDK (Python/TS); stream-json | Same agent loop/tools per docs; `-p` is non-interactive mode of the same CLI. |
| **Cursor** | Interactive `agent` | **ACP** (`agent acp`, JSON-RPC over stdio); also `--print` | Docs: ACP for custom clients; interactive CLI for normal terminal workflows. |

**Pane-only structured control:** **N** for all three as a sole continuation channel. A terminal pane shows the native UI but does not expose turn/permission/cancel events to the orchestrator. Reliable child-agent orchestration needs the programmatic surfaces above.

### 2. Account isolation (separate auth / process env)

| Provider | Mechanism | Status | Notes |
| --- | --- | --- | --- |
| **Codex** | `CODEX_HOME` per process; auth under that home | **S** (local probe) | Fresh temp `CODEX_HOME` → `codex login status` = `Not logged in` while default home remains ChatGPT-logged-in. Do **not** overwrite the user’s `~/.codex` / `.codex-2`. Profiles: `-p` / `--profile` layers `$CODEX_HOME/<name>.config.toml`. |
| **Claude** | `CLAUDE_CONFIG_DIR` (absolute path); optional API key env | **S** (docs) | Docs: settings, session history, plugins under config dir; credentials follow on Linux/Windows. Concurrent dual-login **not** exercised here (would mutate auth). Avoid global `ANTHROPIC_API_KEY` override when using subscription profiles. |
| **Cursor** | `agent login` store; `CURSOR_API_KEY` / `--api-key`; `CURSOR_AUTH_TOKEN` | **S** (partial) | Per-process API key can isolate scripts. Forum evidence: env API key can override logged-in account. Multi-home directory isolation for interactive login stores **U** (no official “CURSOR_HOME” observed in `--help`). |

### 3. Exact session capture and resume by ID

| Provider | Capture | Resume by explicit ID | Anti-patterns |
| --- | --- | --- | --- |
| **Codex** | Thread/session UUID from TUI/app-server/`exec` | **S**: `codex resume <SESSION_ID>`; `codex exec resume <SESSION_ID>`; app-server `thread/resume` with `threadId` | `resume --last`, newest-chat, focused-pane heuristics |
| **Claude** | `session_id` in `--output-format json` / stream result | **S**: `--resume <session-id>` (or absolute `.jsonl` path); `--session-id` for new | `--continue` (most recent); ambiguous name pickers without stored ID |
| **Cursor** | `create-chat` returns chat ID; ACP `session/new` → `sessionId` | **S**: `--resume [chatId]`; ACP `session/load`; interactive `ls` | `agent resume` = **latest** chat; `--continue` previous session |

### 4. Usage / quota visibility without model-prompt probes

| Provider | Read-only surface | Status |
| --- | --- | --- |
| **Codex** | app-server `account/rateLimits/read`, `account/usage/read`, notify `account/rateLimits/updated`; thread `thread/tokenUsage/updated` | **S** (official docs). Not invoked live (would touch auth/network; no mock server in-repo). |
| **Claude** | Per-run `total_cost_usd` / cost in json/stream-json; `--max-budget-usd` for run cap | **S** for **run spend after a turn**. Subscription rate-limit / pool remaining **U** without a documented CLI read-only quota command (do not invent probes). |
| **Cursor** | `agent status` / `about` → auth + plan tier (observed: Pro); no usage % in those payloads | **U/N** for quota: no official CLI usage subcommand observed. Third-party dashboard cookie scraping is **out of scope** and not a supported interface. |

Unknown usage must be displayed as **unknown**, never inferred as free capacity.

### 5. Approvals and interruption

| Provider | Approvals | Interrupt / cancel |
| --- | --- | --- |
| **Codex** | app-server server→client `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`, `item/permissions/requestApproval`. CLI: `-a` / `--ask-for-approval`, sandbox modes. Bypass flags exist but are **not** equivalent to interactive approvals. | `turn/interrupt` → turn `status: "interrupted"`. Waiting state includes `waitingOnApproval`. |
| **Claude** | Interactive prompts; `-p` with `--permission-mode`, `--allowedTools`, `--permission-prompt-tool` (MCP host), `--permission-prompts host|none`. Agent SDK `canUseTool`. Force/bypass ≠ interactive. | Docs: prefer SIGINT / SDK `interrupt()` to finish turn; SIGTERM → exit 143, unfinished turn. Resume leaves interrupted turn unless `CLAUDE_CODE_RESUME_INTERRUPTED_TURN=1`. |
| **Cursor** | ACP `session/request_permission` → `allow-once` / `allow-always` / `reject-once`. CLI `--force`/`--yolo` auto-allows (not interactive). | ACP `session/cancel`. Interactive Escape/Ctrl-C behavior is pane-native (**U** for orchestrator-driven cancel without ACP). |

### 6. Terminal pane alone vs programmatic children

| Need | Pane-only TUI | Programmatic child |
| --- | --- | --- |
| Exact ID resume after crash/switch | Fragile (user must type ID; orchestrator cannot observe events) | Required |
| Approval callbacks into coordinator | No structured channel | Required (app-server / permission-prompt-tool|SDK / ACP) |
| Usage-aware account switch | No | Codex: app-server; others: limited/U |
| Bounded delegation + cancel | No | Required |
| Human day-to-day coding UX | **Preferred** (preserve native UI) | Optional companion |

---

## Topology recommendation

**Adopt: native interactive panes + programmatic child agents** (no replacement UI unless a later gap forces a user decision).

1. **Human agents:** WezTerm/Windows Terminal panes running native TUI (`codex`, `claude`, `agent`) with **explicit** session IDs recorded at start/resume. Pane is for input/display, not for orchestrator event buses.
2. **Orchestrated / child agents:**  
   - Codex → `codex app-server` (or managed daemon) JSON-RPC with isolated `CODEX_HOME`.  
   - Claude → `claude -p` / Agent SDK with stream-json; isolated `CLAUDE_CONFIG_DIR` or API-key process env.  
   - Cursor → `agent acp` (preferred for permissions/cancel) or `--print` for simpler scripts; explicit `sessionId` / `chatId`.
3. **Account switch:** Spawn or rebind a **new process env** to the target account home/key; resume the **same logical conversation** only via stored native session ID. Never rotate credentials under a live peer session. Never `resume --last`.
4. **Wrapper UI:** Only if a provider cannot preserve interactive approvals while supporting failover—**flag for user decision** before building. Current evidence does **not** force a full replacement UI.

---

## Account-switch semantics (provisional)

| Step | Behavior |
| --- | --- |
| Bind | `logicalConversationId` → `{provider, accountConnectionId, nativeSessionId, paneOrChildId}` |
| Exhaustion | Mark run `exhausted` from provider rate-limit signals (Codex: documented) or explicit user/policy; **unknown** otherwise |
| Same-provider switch | New process with alternate account env; `resume`/`thread/resume`/`session/load` with **stored** native ID when provider allows cross-account transcript continuity—**continuity across accounts is U** until P5/P8 prove it; if unsupported, open new native session and transfer bounded recorded context only |
| Cross-provider | Ask unless configured; never claim identical tool/approval state |

---

## Gaps and unproven items

- Live `account/rateLimits/read` against user’s Codex login (intentionally not run).
- Whether Codex thread IDs resume under a **different** `CODEX_HOME`/account (likely no shared transcript store)—must be proven in P5 with mocks first.
- Claude subscription quota remaining without a model turn.
- Cursor official usage API via CLI/ACP (none observed).
- Cursor multi-login directory isolation equivalent to `CODEX_HOME` / `CLAUDE_CONFIG_DIR`.
- Whether interactive Codex TUI and an independently managed app-server can share the same live thread without races (daemon/`--remote` exist; concurrency contract still **U**).
- Windows signal behavior for `claude -p` interrupt (docs describe SIGINT/SIGTERM; Win32 mapping **U**).
- Selective reuse of `ccswap` / `agent-intern` permission semantics (not inspected deeply in P0).

---

## Next dependency

- **P4 (Coordinator)** can consume this matrix for connection/run/session schemas and capability flags (`supportsExactResume`, `supportsRateLimitRead`, `approvalChannel`, `isolationEnvVar`).
- **P5 (Codex adapter)** is the first implementation spike: dual `CODEX_HOME`, exact `thread/resume` or `codex resume <id>`, simulated exhaustion, approval interrupt—**mocks before live**.
- **P8 (Claude + Cursor adapters)** after P4 (+ P5 contract validation): launch/resume/interrupt + honest capability reporting for weaker usage surfaces.
- **P1** remains independent for terminal host choice; topology assumes panes exist but does not depend on P1 APIs.

## Open questions for user (non-blocking for P4 design)

None blocking. Optional later decisions:

1. Accept Codex-first usage-aware switching, with Claude/Cursor usage shown as **unknown** until a supported read-only API appears.
2. If Cursor cannot isolate two interactive logins without API keys, prefer API-key children for second account vs. sequential login (worse).
