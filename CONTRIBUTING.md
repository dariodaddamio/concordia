# Contributing

## Development (Windows)

1. Install Rust (the toolchain is pinned by `rust-toolchain.toml`) and the Visual Studio Build
   Tools C++ workload.
2. During development, reproduce the changed flow and verify it end to end with a concrete
   artifact. Use the existing fake provider for free, repeatable UI checks. Live-provider checks
   follow the small-model scope in AGENTS.md. Do not rerun the full suite on each edit.
3. Do not add unit or integration tests unless the owner requests a specific case. Preserve
   useful recovery, approval, source-identity and storage checks; delete tests that only mirror
   implementation without catching a real bug beyond the end-to-end flow.
4. For an explicitly requested commit/PR/release qualification, run the full tests, Clippy,
   formatting and applicable delivery gates. Clippy checks Rust code without running the app.
   The 48 opt-in PTY journeys run separately with
   `cargo test -p concordia --test tui_journeys -- --ignored`.
5. When installed-build qualification is requested, install the candidate with
   `powershell -NoProfile -File .\scripts\install-dev.ps1 -Install`, then run
   `powershell -NoProfile -File .\scripts\smoke-e2e-everyday.ps1`. Smoke checks the installed
   build stamp and runs the full suite; it does not install the candidate.

`scripts\install-dev.ps1` builds and runs `concordia install`, which makes your build the `concordia` on your
PATH. Open a new terminal afterwards; panes already running keep their old image.

## Rules

- Never use `resume --last` / `--continue` for recovery.
- No live model turns in tests or CI. Use the fake provider and fixtures.
- Never collect, store or copy provider credentials (see `docs/provider-terms.md`).
- Keep personal data (names, usernames, local paths) out of the tree and out of fixtures.
- Bump `BUILD_STAMP` for every user-facing change, and add a line to CHANGELOG.md.

## Pull requests

Keep PRs focused. CI (fmt, clippy, tests, smoke, `cargo deny`) must pass on `windows-latest`.
