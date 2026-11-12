# How Concordia works

Concordia is one local Rust application around the official Codex, Claude Code and Cursor CLIs.
It provides a shared interface, original-source retrieval and worker coordination across your
accounts. The provider still owns its login, model session, tools and approvals.

## One request, end to end

1. Choose an account/model and write a message. Selection prepares local state; it starts no model
   turn. Home and browser-style tabs keep their own drafts, queues and running work.
2. On submission, Concordia validates input and workspace ownership. It prepares explicit file/chat
   references and any required handoff context in the background.
3. The provider adapter resumes the exact native session when valid, or starts a new one with
   bounded context. A cross-provider switch cannot transfer native tools or approval state.
4. The provider CLI runs. Concordia renders streamed text, reasoning and tool/approval events,
   tracks cancellation and records references/usage where supported.
5. Completion finalizes the turn. Failure pauses queued work; configured continuation can choose
   an eligible target within its limits. Uncertain tool completion requires review.

Source owners: [TUI runner](../crates/concordia/src/tui/run.rs),
[input submission](../crates/concordia/src/tui/input.rs),
[provider runner](../crates/concordia/src/provider/turn.rs),
[context planner](../crates/concordia/src/context.rs),
[continuation](../crates/concordia/src/tui/continuation.rs).

## Processes and data

Browser tabs share one process; each retains a complete conversation state and provider channels.
Providers, workers and explicit external actions run as tracked children. Internal file
previews/editors belong to their conversation project.

`<state>` defaults to `%LOCALAPPDATA%/concordia` (`CONCORDIA_STATE_DIR` overrides it).
User configuration is `config.toml`; saved account/settings choices are `.ao/store.json`.
Provider-native histories remain in provider homes. Concordia keeps indexes, usage caches,
attachment snapshots and pane recovery separately. Optional thread recording and bounded pinned
source spans add local retention; drafts and unfinished work may retain full text. This is not a
claim that Concordia stores no message bodies. [Storage and bounds](native-memory.md).

## What the extra layer buys you

One place to continue tasks across provider accounts, retrieve the exact recorded sources,
coordinate workers and inspect/edit workspace files. Routing and maps use local deterministic
logic, with no summarizer model. Their usefulness does not by itself prove token savings or
better memory than the provider's own session.

## Read next

- [Current scope and direction](../PLAN.md)
- [Architecture and ownership](architecture.md)
- [Exactly what is added to prompts](how-concordia-builds-prompts.md)
- [Provider connections](how-concordia-connects-providers.md)
- [Memory and delegation details](how-concordia-remembers-and-delegates.md)
- [Runtime and data details](how-concordia-runtime.md)
- [Verification status](e2e-status.md)
- [Commands and controls](ui-glossary.md), [configuration](config.md), [images/PDFs](images-and-diagrams.md)
- [Historical provider research](archive/capabilities.md), [measured performance scope](metrics.md)
