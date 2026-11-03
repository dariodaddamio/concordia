# UX reconciliation

PR #18 was reviewed against the first completion wave (`14d6150`). Focus/resize repaint,
Windows word-backspace handling and independent draft scrolling already exist in that wave.
The native notice filtering, selection copy hint and permission labels are carried forward
in build `2026-11-02l`.

Keep the newer landing behavior: chats and goals remain visible during drafting, with draft
keys owned by the composer. The older hide-while-typing implementation is superseded.
Do not restore caret-moving wheel scrolling or diagnostic logging of typed keys.
Native provider source records remain unchanged; filtering applies to readable user history
and session previews/titles. Exact source excerpts continue to return original records.

Validation: all 885 enabled library tests and all-target Clippy with warnings denied pass.
Full suite, installed terminal checks and remote CI remain separate candidate checks.
No real provider turns are used.
