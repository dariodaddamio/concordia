# Worker notes

## Changed

- Replaced the workspace changelog heading with build `2026-11-05h`, the next stamp after main's `2026-11-05g`, and fixed the missing blank line before the 05g entry.
- Made appearance defaults clear saved token-line, idle-tip and stats overrides as well as theme, color and motion.
- Made active-turn status glyphs stop cycling when the app motion setting is off while preserving static tool-specific glyphs.
- Made native icon animation require both the app motion setting and a successful Windows client-area-animation query that reports animation enabled.
- Added regression assertions for display-default reset and static status glyph behavior.

## Verification

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `cargo clippy -p concordia --all-targets -- -D warnings`: could not complete; Cargo repeatedly reported `Blocking waiting for file lock on build directory` while no local Cargo/rustc process was visible. The command was stopped after the lock remained held.
- `cargo test -p concordia`: could not start for the same shared build-directory lock; Cargo reported `Blocking waiting for file lock on build directory`, then the command was stopped.

No provider turns or credential access were used. Changes remain uncommitted.
