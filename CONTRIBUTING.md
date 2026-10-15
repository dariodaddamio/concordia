# Contributing

## Development (Windows)

1. Install Rust (the toolchain is pinned by `rust-toolchain.toml`) and the Visual Studio Build
   Tools C++ workload.
2. `cargo test -p concordia --tests`
3. `cargo clippy -p concordia --all-targets -- -D warnings` and `cargo fmt --all --check`
4. `powershell -NoProfile -File .\scripts\smoke-e2e-everyday.ps1` (installs your build; see below)
5. `powershell -NoProfile -File .\scripts\verify-oss-gates.ps1`

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
