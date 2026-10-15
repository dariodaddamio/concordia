# Changelog

All notable changes to Concordia. The format follows [Keep a Changelog](https://keepachangelog.com/);
Concordia has not had a tagged release yet, so everything is under Unreleased. Each user-visible change
also bumps the build stamp shown by `concordia --version`.

## [Unreleased]

### Fixed
- Claude and Cursor turns now stream (`stream-json`) like Codex: tool cards and replies appear as they happen instead of all at once at the end.
- The wait shows what the model is doing (waiting, thinking, running a tool, writing) with a per-phase colour, animation and timer; reasoning is shown live as dim lines and kept after the turn, never sent as context.
- A failure reported as an event, a result and stderr is shown once, and `Reading additional input from stdin...` is hidden.
- The prompt marker, palette and picker highlight use plain ASCII `>` and `...`, whose width is the same in every terminal font; ambiguous-width glyphs could shift the caret.
- After `/update`, the old process now stays as the parent until the new one exits. Before, it handed the console back to the shell early, so the shell and Concordia both read the keyboard and the prompt box lost or scrambled keystrokes.

### Added
- Native Rust TUI: streaming chat with tool and command cards, account picker, model picker,
  usage, explicit session resume, trust modes, WezTerm splits.
- Mouse support: click to place the caret, drag / double-click / triple-click selection in the
  prompt and the transcript, wheel and `PgUp`/`PgDn` scrolling, right-click copy or paste.
- A real prompt editor: multi-line, Shift-selection, word moves, cut/copy/paste, bracketed paste,
  prompt history, `Ctrl+C` copy-or-cancel, double-tap exit.
- `/use add [codex|claude|cursor]`: creates an isolated account home and opens the provider's own
  login; announces when it connects.
- `concordia doctor` and `/doctor`: install, state, provider CLIs with versions, accounts, PATH, update mode.
- `concordia install`: one install story (puts the exe where `concordia` on PATH finds it, edits the user PATH
  safely, refreshes the launcher shim).
- `/update`: rebuilds from a source checkout, or downloads the newest release (checksum, runs,
  right version, else nothing is installed). `/update check` previews.
- Build stamp in the header and `--version`; fake-provider and TUI end-to-end test harnesses.
- CI (fmt, clippy `-D warnings`, tests, release smoke, `cargo deny`), release workflow with
  third-party license generation, pinned toolchain.

- Slash-command palette: typing `/` shows matching commands with descriptions (arrows, Tab, Enter, click).
- `/use remove`: remove an account. Concordia-created accounts can be deleted with their login data; your own
  homes can only be hidden. Signing in to the same login twice is detected and the duplicate removed;
  `/use` and `doctor` flag existing duplicates; the picker names each account (Codex 2, Codex 3).
- Claude and Cursor now keep their conversation between turns (explicit session ids), like Codex.
- Markdown rendering for replies (headings, lists, code, inline code), a status-bar header, grouped tool
  cards, spacing between turns, a spinner while working, trust shown in colour (full is flagged).
- `/update` rebuilds from any pane (the install remembers its source checkout), runs in the background with
  live progress, and Esc cancels it (the whole build process tree is stopped); the UI stays usable.
- `concordia wezterm` opens WezTerm running Concordia; `/split` works for installed copies (the WezTerm
  config is bundled in the binary).
- A log file in the state folder (`concordia.log`, events only, rotates at 1 MB); a crash records its
  backtrace there. `NO_COLOR` is honored; `CONCORDIA_LOG=off` disables the log.

### Fixed
- The prompt marker, palette and picker highlight use plain ASCII `>` and `...`, whose width is the same in every terminal font; ambiguous-width glyphs could shift the caret.
- `concordia` on PATH could resolve to a months-old binary: the installer now always replaces `concordia.exe`,
  even while an older Concordia is running from it (the old file is moved aside), and `doctor` reports
  when PATH resolves to anything other than the active build.
- Multi-line, `%`, `!`, non-ASCII and very long prompts failed or were rewritten behind Windows
  `.cmd` shims; such prompts now go over stdin.
- `Ctrl+C` killed only the shim and left the real agent running; it now kills the whole process tree.
- A failing provider showed an empty reply; stderr is now captured and shown with the exit code.
- Provider output with invalid UTF-8 truncated replies; a huge line could exhaust memory.
- `concordia --version` rewrote the install pointer to whichever exe ran it.
- Shared state could be lost when two panes saved at once, or destroyed when corrupt; writes are
  now atomic, locked, and a corrupt file is backed up first.
- `Ctrl+C` inside a picker quit the app; it now closes the picker. Clipboard calls could freeze the UI; they time out.
- Long sessions made drawing slow (about 500 ms per frame at 40k rows); rows are now cached (~17 ms).

### Changed
- The npm shim and the `-a/-b/-next` install slots are gone: `concordia.exe` is always the
  newest build.
- `Esc` no longer quits (it clears a selection or interrupts a running turn).
- The TypeScript prototype and all research spikes were removed; the product is Rust only.
