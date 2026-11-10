# Terminal host support

This folder began as the October terminal-host experiment. The standalone spike launcher is
retired: it guessed the control pane from the highest pane ID and could target an unrelated
window. It has no product callers; its original source remains in Git history.

The retained `wezterm/wezterm.lua` is a production input embedded by
[`host/wezterm.rs`](../../crates/concordia/src/host/wezterm.rs). It resolves Concordia's installed
executable, provides explicit split/new-tab commands, and preserves ordinary keyboard handling.
Do not remove or regenerate it as experimental debris.

Use the product's `concordia wezterm` / `/split` commands for host operations. Provider login,
account/session ownership and live qualification remain governed by the repository instructions.
WezTerm is optional and is never installed silently by the development bootstrap.
