# Worker notes

## Finished: Part 1

Root cause: Home scans provider-native session artifacts, and the Codex/Claude/Cursor summarizers
previously returned metadata-only artifacts as sessions. Their missing preview was rendered as
`untitled session`, so opening or initializing a provider could create visible junk before a real
user turn. Concordia's own `ThreadLog::open` was already lazy and performs no write until an event
is appended.

Changed spawn sites: none. No process spawning was refactored or added.

The native summarizers now count real, non-command user turns. Home listing filters records with
zero user turns. `concordia sessions prune --empty` lists provider/account/session/path candidates,
requires the exact `yes` confirmation, and removes the native file or Cursor directory. This is
the only destructive exception and is explicitly requested by the task; cancellation leaves all
artifacts intact. Existing child-agent deduplication and bounded scans remain unchanged.

Gate: a session is visible only when its provider-native artifact contains at least one real user
message. Existing metadata-only artifacts are hidden, not merged or rewritten.

Checks attempted:

- `cargo fmt --all` completed.
- `cargo fmt --all -- --check` initially identified only the formatter layout in the new function;
  formatting was then applied.
- `NO_COLOR` was unset before `cargo test -p concordia --lib sessions:: -- --nocapture`.
- The first test compile found missing `user_messages` fields in fixture constructors; those were
  fixed. A rebuilt test invocation was started but did not return a final result in this worker
  environment.
- `git diff --check` passed.

The Part 1 changes are uncommitted because Git could not create
`.git/worktrees/concordia-home/index.lock` (`Permission denied`) in this managed worktree.

## Remaining

- Part 2: group-name/path hygiene, `\\?\` display stripping, hidden temp/target/test workspaces,
  and the `h`/footer show-all toggle.
- Part 3: usage meters beneath goals using the existing fetcher/cache cadence.
- Full clippy, full crate tests, smoke E2E, and final acceptance fixture rendering.
