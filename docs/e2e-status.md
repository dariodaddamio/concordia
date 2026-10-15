# Everyday E2E status (MVP A)

**Verdict (2026-10-15):** **Partial.** Codex chat in the Rust TUI works for an already-logged-in home (`/use` → type → stream/cards → cancel), but **account connect** is missing and **stale install/launch** often hid newer builds. A stranger cannot complete onboarding→chat inside `concordia` alone.

Audit source: [Audit Concordia E2E gaps](16e13859-a1bd-4119-b4d3-51fc94665a4e).

## Why iteration felt stuck

Symptom patches (double-Enter, usage, path/Notepad) without a forced acceptance loop: no build stamp, npm/`WezTerm` preferred old slots, no `/update` re-exec until late, MVP A never re-checked end-to-end.

## Working today

| Loop | Evidence |
| --- | --- |
| Rust TUI shell | `crates/concordia/src/tui/mod.rs`, `main.rs` |
| Slash surface | `/use` `/model` `/usage` `/resume` `/new` `/config` `/trust` `/split` `/update` `/version` `/exit` — `commands.rs` |
| `/use` switch (no turn) | pickers + `store.rs` / `accounts.rs` (logged-in only) |
| Codex turns | `provider/codex.rs`, `stream_cards.rs`, `turn.rs` |
| Cancel | Ctrl+C → tracked child |
| Usage / resume / trust / model caches | `usage.rs`, `sessions.rs`, `catalog.rs` |
| Continuity pack | `context.rs` |
| `/update` + `current` pointer | `update.rs`, wezterm.lua, `scripts/concordia-launcher.cmd` |
| Tab, paste, mouse select/copy, click-to-caret, wheel + PgUp/PgDn scroll | `tui/{mod,editor,transcript,text,clipboard}.rs`; 37 TestBackend/unit tests — **pane must run stamp ≥ `2026-10-15e`; not yet hand-verified in live WezTerm** |

## Gaps (ranked) — next implement order

| # | Pri | Gap | Owned paths |
| --- | --- | --- | --- |
| 1 | P0 | **Prove install** — `install-dev.ps1` green; `current` + stamp in header; smoke PASS | `scripts/install-dev.ps1`, `concordia-launcher.cmd`, `version.rs` |
| 2 | ~~P0~~ | ~~`/use add` + slot create + login launch~~ **Done in source (`2026-10-15f`)**: `connect.rs`, picker + `/use add [provider]`, login-complete watcher. **Needs a live login check** (opens a real window) | `connect.rs`, `tui/mod.rs`, `commands.rs` |
| 3 | ~~P0~~ | ~~Copy + paste~~ **Done in source (`2026-10-15e`)**: drag-select, Ctrl+C copies a selection, right-click, `/copy`. Needs a live-pane check | `tui/` |
| 4 | ~~P1~~ | ~~Prompt history, PgUp/PgDn/wheel scroll~~ **Done (`2026-10-15g`)** | `tui/mod.rs` |
| 5 | P1 | Persist Concordia chat/launch + Claude/Cursor session-id bind after turns | `store.rs`, `turn.rs`, `claude.rs`/`cursor.rs`, `sessions.rs` |
| 6 | P1 | Exhaustion → other Codex account with explicit session bind | account policy + TUI |
| 7 | P2 | Picker ← back / login submenu; approvals UX (double-tap Ctrl+C exit **done**) | TUI + trust |

Out of scope for MVP A: OSS publish, context sharing, crew delegation.

## Acceptance for “MVP A feels real”

1. Header shows stamp `2026-10-15n`+; `/version` matches `%LOCALAPPDATA%\concordia\bin\current`.
2. Cold path: `/use add` creates/connects an account without leaving Concordia (unit + TUI-flow tested; real login window not yet exercised).
3. Prompt: click to place the caret; drag/double/triple-click select; Tab completes slash; Ctrl+V pastes; Ctrl+C copies a selection; long transcript scrolls (wheel / PgUp / PgDn).
4. Type a prompt → text and/or command cards; usage % after a prior Codex session.
5. `/update` rebuilds and re-execs same pane on a new stamp.
6. `scripts/smoke-e2e-everyday.ps1` exits 0 after every user-facing change (bump `BUILD_STAMP` in `version.rs`).
