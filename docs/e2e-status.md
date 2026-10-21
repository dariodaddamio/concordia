# Everyday E2E status (MVP A)

**Verdict (2026-10-21):** **Partial.** Wave 1 and wave 2 are landed in source on `feat/wave-2` (build stamp `2026-10-16e`, `crates/concordia/src/version.rs`). They are covered by automated tests that use the fake provider. They have **not** been proven end to end on a clean install: the install script and the everyday smoke script have not been run live against this stamp, and real provider turns, a real account login window and the live WezTerm pane have not been exercised.

Audit source: [Audit Concordia E2E gaps](16e13859-a1bd-4119-b4d3-51fc94665a4e). Detailed audits: `command-audit.md`, `interaction-audit.md`, `file-navigation-audit.md`, `provider-activity-audit.md`, `usage-freshness-audit.md`.

## What landed

- **Wave 1:** thread log and map, token line and tips, `config.toml` and user commands, `/use add` account connect (`connect.rs`), copy/paste/selection, prompt history, scrolling.
- **Wave 2:** prompt queue (`Tab` queues while busy, explicit resume), undo/redo, drafts and recovery records, activity cards with full detail (`Ctrl+E`, `Ctrl+O`), file links and `/open` `/preview`, read-only MCP server and bounded chat reads, provider compatibility checks, process stats, vendored Crossterm for Windows emoji input.

## Verified by tests vs only built

| Area | Verified by automated tests (fake provider, TestBackend, unit) | Only built, not verified live |
| --- | --- | --- |
| TUI shell, slash surface, pickers | yes (`cargo test --workspace --locked`: 558 unit tests plus integration suites passed on this branch) | the live WezTerm pane with the current stamp |
| Queue, undo, drafts, recovery, activity cards | yes | long real sessions |
| Stream parsing (Codex, Claude, Cursor JSON) | yes, from recorded and synthetic lines | real provider turns (never run by agents; no real CLI turns in tests) |
| `/use add` and slot creation | unit and TUI-flow tests | a real login window |
| Copy, paste, mouse, scroll | TestBackend and clipboard unit tests | hand use in the live pane |
| MCP server, chat read/search | `tests/mcp.rs` and unit tests | a real MCP client |
| `/update` re-exec, `current` pointer | unit tests (isolated pointer) | an install via `install-dev.ps1` |
| Install and launch path | script exists | **not run live** |

## Open items (ranked)

| # | Pri | Item | Status |
| --- | --- | --- | --- |
| 1 | P0 | **Prove install**: run `scripts/install-dev.ps1`, confirm `current` and the stamp in the header, then run `scripts/smoke-e2e-everyday.ps1` and require exit 0 | **Not yet run live** |
| 2 | P1 | Persist chat/launch and bind Claude/Cursor session ids after turns | Open |
| 3 | P1 | Account exhaustion: fall back to another Codex account with an explicit session bind | Open |
| 4 | P2 | Picker back navigation and login submenu; approvals UX (double-tap Ctrl+C exit is done) | Open |

Out of scope for MVP A: OSS publish, context sharing, crew delegation beyond what `delegation.md` describes.

## Acceptance for "MVP A feels real"

1. Header shows stamp `2026-10-16e` or later; `/version` matches `%LOCALAPPDATA%\concordia\bin\current`.
2. Cold path: `/use add` creates or connects an account without leaving Concordia (tests only so far; a real login is unverified).
3. Prompt: click to place the caret; drag/double/triple-click select; Tab completes slash; Ctrl+V pastes; Ctrl+C copies a selection; long transcript scrolls.
4. Type a prompt: text and/or command cards stream; usage % shows after a prior Codex session.
5. `/update` rebuilds and re-execs the same pane on a new stamp.
6. `scripts/smoke-e2e-everyday.ps1` exits 0 after every user-facing change (bump `BUILD_STAMP` in `version.rs`).

Items 1 to 6 are unchecked until someone runs them live and records the result here.
