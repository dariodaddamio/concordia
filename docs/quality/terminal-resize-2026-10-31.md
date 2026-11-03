# Terminal resize and exit verification, 2026-10-31

Build `2026-10-31k` fixes Windows terminal restoration. Crossterm 0.28's
`PopKeyboardEnhancementFlags` returns `Unsupported` on Windows. It was the first
command in a batched best-effort cleanup, so that error skipped paste/focus/mouse
reset and `LeaveAlternateScreen`. A new ConPTY journey with a surviving parent
shell reproduced the missing alternate-screen exit before the fix.

Cleanup now attempts each reset independently. Normal shutdown clears the TUI
before leaving its alternate screen; panic unwinding preserves diagnostics that
the panic hook has already printed on the primary screen. Frames use synchronized
updates. Resize handling queries the latest dimensions before invalidating the
screen, and Windows input polling checks size during continuous input too.

The new journey passed against the installed release executable. It checks header,
draft and bottom-border positions through these column/row transitions:

`120x40 -> 60x40 -> 120x40 -> 60x20 -> 160x50 -> 40x12 -> 120x40 -> 180x55 -> 60x55 -> 120x40`.

It then resizes at 25 ms intervals through `55x18 -> 180x55 -> 75x35 -> 100x30`
and checks the final screen alignment. On exit it requires an erased alternate
screen, the alternate-screen leave sequence, restoration of the parent shell's
original text, a new shell output marker, and no remaining chat UI. All state and
provider paths are synthetic; no real model turn is used.

An initial rapid-resize assertion incorrectly required a fresh complete output
frame when the final size matched the starting size. The terminal can coalesce
those changes and retain a correct existing screen. The final case ends at a
different size so the fresh-frame assertion is meaningful.

Verification: standard library/integration tests, clippy with warnings denied,
formatting and installed everyday smoke passed. The final resize/exit journey
passed against the installed executable through both debug and release test
harnesses. The other 28 ConPTY journeys passed in the broader run; that run still
contained the earlier rapid-resize assertion, which failed before the corrected
journey was rebuilt and rerun separately.

These checks exercise Windows ConPTY and the installed binary. They do not certify
physical window maximization, snapping or fullscreen transitions in every terminal
host. Existing Concordia processes retain the old executable until restarted.
