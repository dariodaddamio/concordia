# Concordia patch

Base: crates.io `crossterm` **0.28.1**, with its source, examples, documentation and MIT license.
`src/event/sys/windows/parse.rs` fixes a key-record behavior. The manifest also recognizes
existing upstream cfg names and suppresses its unused internal event-filter diagnostic,
which were suppressed when Cargo built the registry dependency.

Windows sends key-down and key-up records for each UTF-16 surrogate half. Passing release
halves to the pairing buffer consumes two high halves together, then two low halves together,
silently dropping emoji. Ignore release records in the general surrogate branch; keep the
special Alt-code branch unchanged.

Upstream report: https://github.com/crossterm-rs/crossterm/issues/1072

The real ConPTY test `multiline_paste_and_expand_work_through_the_real_terminal_for_each_provider`
checks both a wide BMP character and an emoji through the full app. Remove this override when
a compatible upstream release includes the fix and that test passes without it. Do not modify
the machine's Cargo registry cache.

Concordia also compiles the existing ANSI input parser on Windows and exposes a hidden
`event::parse_windows_vt_event` adapter (event.rs and event/sys/windows.rs). It returns whether
an incremental event buffer was consumed plus an optional public event; terminal replies are
consumed without becoming keystrokes. A Windows-local internal-event adapter and raw-mode query
bridge platform types; the CSI cursor reply parser rejects zero coordinates instead of underflowing.
Other ANSI parsing logic is unchanged.
Concordia's separate Windows VT byte-stream source uses this adapter, because the default
Windows INPUT_RECORD backend cannot retain bracketed-paste framing. Retire this adapter when
an upstream supported Windows byte-stream source preserves the same paste/key/mouse behavior
and the full ConPTY suite passes without it. No registry cache is modified.
