# Active dispatch

| Plan | Slice | Owner/model | Status | Next action |
| --- | --- | --- | --- | --- |
| campaign/ui-logic-1115 | Tabbed settings screen, review fixes, build revision (15d) | Claude Fable lead; Luna writer packet R7 | implemented, driven in a sandbox window; awaiting commit | Commit, then remaining campaign queue |

## This slice

### Settings screen and review fixes (15d) - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| crates/concordia/src/tui/settings.rs, tui/app/state.rs | ok: 1, 4, 6, 10 | Replaces the flat `/config` picker instead of adding beside it; `PickerKind::Settings` is deleted. Rows read the saved store once on open and after a change; mutation stays in the existing `choose` functions. |
| crates/concordia/src/tui/draw/mod.rs, tui/theme.rs | ok: 5, 7 | Drawing only. Three role helpers name the theme aliases the draw layer already used; `apply` stays the single owner of the palette. |
| crates/concordia/src/tui/input_dispatch.rs, tui/app/clipboard.rs | ok: 2, 10 | Bare R and L update keys are deleted because automatic restart made them harmful, not guarded further. Pasted text goes to the filter while the screen is open. |
| crates/concordia/src/tui/app/state.rs (restart) | ok: 4 | `restart_blocked_by_work` is the single idle gate and now includes unsaved file edits, matching `update_blocked_by_work`. |
| crates/concordia/src/tui/native_window.rs | ok: 2 | One flag stops the caret timer being re-armed on every paint. |
| crates/concordia/build.rs, src/version.rs | ok: 6, 9 | The build revision is derived from git at build time; `BUILD_STAMP` remains the hand-written changelog key. A test-only branch in `short_stamp` was removed (tendency 9). |
| crates/concordia/src/install_watch.rs | rejected: 3, 8 | A 5-second retry loop for a locked binary was removed: the reviewer rated the case plausible only, with no observed failure. |
| crates/concordia/src/tui/tests/run.rs, tests/input_dispatch.rs | ok: 9 | Two existing tests updated to the new contract (automatic restart, settings screen); no test added. |
