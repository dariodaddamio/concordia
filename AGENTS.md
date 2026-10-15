# Agent implementation instructions

Read PLAN.md before work. Concordia is a thin local orchestrator that drives the Codex, Claude Code and
Cursor CLIs from one terminal app. The product is the Rust crate in `crates/concordia`.

State the task, the paths you will touch, your assumptions and observable acceptance criteria
before implementing. Do not broaden scope to replace provider tools, context management,
authentication, or terminal rendering.

Delegation is allowed for scoped work: assign disjoint ownership and return evidence. Do not spawn
agents recursively merely because a slot is available. Research does not authorize live provider
tasks or account changes.

**Never run real provider agents.** Use the fake provider (`tests/support/fake_provider.rs`) and
fixtures. Real turns spend the user's subscription usage and need an explicit scope and budget
from the user first. Read-only calls that do not run a model (`--version`, `--help`) are fine.

Preserve native approvals and keyboard behavior. Bind recovery to explicit conversation and
native-session ids; never use `resume --last` or focused-pane heuristics. Isolate account
credentials; never overwrite a user's existing `.codex` or `.claude` homes.

Use supported interfaces first. No hosted paid services, automatic summarization, quota probes
that consume model tokens, resident inference models, or Docker dependency.

Do not read or copy secret files, credentials or tokens, and keep personal data (names, usernames,
local paths) out of the tree and out of fixtures. Prefer synthetic fixtures. See
`docs/provider-terms.md`: Concordia must keep running the unmodified provider CLIs under the user's own
logins and must never collect, store or share credentials.

Use one owner for capture and recovery, bounded retries, and tracked process cancellation. Never
blindly replay uncertain tool actions or automatically reset, delete, merge, or clean user work.
Use argument arrays (or stdin) for prompts, never shell strings; validate Windows `.cmd` shims.

Do not install hooks or change global authentication for experiments. Start with isolated
directories and mock providers.

Run meaningful checks for what you changed (`cargo test -p concordia --tests`, clippy with
`-D warnings`, `scripts/smoke-e2e-everyday.ps1`). Bump `BUILD_STAMP` in
`crates/concordia/src/version.rs` for every user-facing change. Record notable changes in CHANGELOG.md.

No publishing, remote repository creation, account migrations, or release actions are authorized
by this file alone.
