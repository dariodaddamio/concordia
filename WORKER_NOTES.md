# Worker notes

- Made `update::reexec_into_mode` a marked visible spawn so the replacement interactive process retains its console. Converted login and Explorer exceptions to line-level `visible-spawn` markers and tightened `check-hidden-process-spawns.ps1` accordingly.
- Added deterministic release-cache isolation through `CONCORDIA_RELEASE_CACHE_DIR`; release integration sandboxes set and clean that root alongside their install root.
- Added `concordia shortcut install|remove [--desktop]` for the per-user Start menu (or opt-in Desktop) shortcut, using the running executable as target and icon and a hidden helper process.
- Extended the fake-provider Windows shim test with a titled grandchild console and top-level window enumeration assertion. Updated `docs/command-audit.md` and `CHANGELOG.md`.

Checks:

- Hidden-process gate: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy -p concordia --all-targets -- -D warnings`: PASS.
- `cargo test -p concordia`: FAILED after 880 passed, 5 failed, 6 ignored. The failures were
  unrelated pre-existing Windows temp-file/workspace tests (`file_document`, two `tui::workspace`,
  `workspace_paths`, and `workspace_search`); the release-update and process/shortcut changes
  compiled and the run completed without a new failure attributed to them.

No provider agent turns, credentials, global configuration, or git write operations were used.
