# Production readiness work plan

Baseline: installed build `2026-10-16b`, 591 enabled tests. Those checks are not an exhaustive
audit of every command or state. Preserve all existing uncommitted work.

The primary user combines a university/education subscription with personal subscriptions.
Concordia should make their own accounts, CLI chats and supported tools easier to access while
remaining a thin native orchestrator. Provider entitlements, authentication, approvals and
session formats remain authoritative. Never imply that an education subscription includes a
provider CLI unless that account actually supports it. No inferred free capacity or automatic
cross-account replay. Cross-account transcript reading and context sharing must be explicit.

## Scoped work and acceptance

| Work | Ownership | Acceptance |
| --- | --- | --- |
| Command/state audit | commands.rs and a new command-audit document/test file; parent owns TUI integration | Inventory every top-level CLI, slash command, alias and MCP method; classify actual functional coverage and unsupported states. Exercise invalid arguments, empty/missing/corrupt state, busy/cancel/update/restart, account changes and stale cached values. Report uncovered combinations honestly. No login, real provider turns, real account mutation or publishing |
| Activity parity | provider/stream_cards.rs, provider/stream_json.rs and their tests | Preserve full provider-supplied shell commands/results, MCP names/arguments/results/errors, file paths and edit/diff details; distinguish running/done/error. No invented diffs or filesystem scans that fabricate provenance. Test each provider plus unknown/malformed events and reused IDs |
| File navigation | new file_links.rs and file_open.rs modules; parent owns TUI/config integration | Resolve plain paths and Markdown links with spaces/Unicode and line/column references from user and assistant text. Explicit click opens a file in a separate terminal editor without blocking chat; Markdown has a preview route. Use argument arrays, never evaluate model text as shell commands; errors remain visible and mouse text selection still works |
| Limit warnings and freshness | usage.rs plus new TUI warning helper; parent owns drawing | Show low 5h/7d remaining allowance above chat with local reset time/date. Respect selected account identity, missing/unknown data, cache age, refresh failure and reset boundaries; never present stale snapshots as current or reset windows as confirmed available capacity |
| Cross-account chat CLI | sessions.rs plus new CLI helper; parent owns main.rs wiring | List/search/read the user's own native CLI chats across registered accounts with explicit provider/account/session labels and bounded output. Read transcript data only, preserve native homes, never infer newest-session ownership or silently resume/share between accounts |
| Performance and release audit | isolated benchmark/test file and release-blockers.md; parent runs gates | Measure typing during sustained fake streams with long transcripts/drafts, not only cached draw speed. Check subprocess cleanup, startup, memory, idle work and bounded output. Run full gates; distinguish measured results, unsupported behavior and human/provider verification |

All delegated workers use `gpt-6-luna` with medium reasoning, no nested delegation, disjoint
file ownership. Parent integrates, audits findings, fixes regressions, bumps the stamp and
records validation. No global formatting while agents edit shared files.

## Continuation

The user requested continuation at **06:05 America/Phoenix on 2026-10-16** (13:05 UTC).
No scheduling capability is exposed to the current chat, so this continuation is **not scheduled**.
The requested time has since passed. After the user's `proceed` message, the active session
retried the workers successfully and continued this work. No unattended task was created.
The brief below can be used with a scheduling facility if one is available. This file does not
schedule execution; only the scheduler's actual state establishes an unattended run.

### Ready-to-use task brief

Continue Concordia production readiness in the existing agent-orchestrator project. Read
AGENTS.md, PLAN.md, docs/production-plan.md, docs/interaction-audit.md and the current Git status.
Preserve all uncommitted work and existing agent results; inspect progress before duplicating work.
Use gpt-6-luna at medium reasoning for scoped subagents, with disjoint ownership and no nested
agents. Audit every enumerated CLI/slash/MCP command and meaningful state transitions, fixing
stale account/model/usage values and functional defects. Finish provider activity parity (commands,
MCP and actual file edits), clickable file navigation into a separate terminal editor with Markdown
preview, low-limit warnings with truthful freshness, explicit cross-account native-chat CLI access,
and sustained-stream typing performance checks. Keep Concordia thin: no inference for routing,
summaries, indexing or polling; no service dependency or reimplementation of provider context/auth.
Use only fake providers and synthetic accounts/homes; no live model turns, quota probes, sign-ins,
credential reads, publishing or remote changes. Audit subagent results and add meaningful regressions;
run tests, clippy -D warnings, formatting, OSS/dependency gates, screenshots, performance checks and
installed-binary smoke if implementation is ready. Record uncovered cases and release blockers;
do not claim exhaustive proof of zero defects or full native-client parity without evidence.

## Progress

- Command inventory and targeted argument validation completed; see command-audit.md. This
  enumerates the surface but does not exhaust every command/state combination.
- Luna medium workers completed activity, file navigation and usage freshness audits. Parent
  integrated the features and ran a second audit: fixed raw result cards replacing merged command
  details, duplicate result replays, and hashed identities accepting a hypothetical collision.
- Cross-account CLI, file-click/editor/preview integration and warning drawing implemented.
- Provider-free terminal journeys and long-draft streaming performance checks added. Final
  release verification and measurements are recorded in interaction-audit.md and metrics.md.
- Final build `2026-10-16e` is installed. All 651 enabled project tests passed, including 18
  terminal journeys against that installed release. Lint, dependency/OSS, release smoke,
  screenshots and explicit performance checks passed. Native Windows paste framing uses wide
  VT console input after the repeat audit found the limitations of key-record timing inference.

## Next release checks

| Priority | Work | Observable acceptance |
| --- | --- | --- |
| Release gate | Supported CLI event and approval compatibility | Capture versioned fixtures from explicitly authorized, budgeted live checks; shell/MCP/edit status, approval denial and cancellation match what actually happened. Missing event data stays explicit. |
| Release gate | Real Windows host and editor pass | In a supported WezTerm window verify multiline paste, Unicode selection, click versus drag, separate editor launch at a position, preview navigation and provider cancellation. No accidental prompt dispatch or lost draft. |
| High | Complete remaining state matrix | Table-driven CLI/slash/MCP validation plus missing/corrupt/permission-denied state, concurrent account edits and restart timing. Reject malformed optional MCP arguments consistently. Document each uncovered combination. |
| High | Native history coverage | Versioned Codex/Claude fixtures and a documented Cursor reader when its supported storage interface permits it; bounded scans report limits rather than false absence. Search scope remains visible. |
| Medium | Resource budgets | Measure startup and input/draw without concurrent builds; measure idle/streaming memory and process cleanup in synthetic sessions. Keep dependencies and background polling within documented budgets. |

These are explicit remaining checks, not claims of production readiness. This pass does not
establish zero defects, native-client parity across every version, or an open-source ranking.
