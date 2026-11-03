# L1 — Claude native approvals

Read the [common prompt and ownership rules](README.md),
[native input contracts](../../quality/native-worker-input.md),
[Codex broker evidence](../../quality/codex-native-input.md) and the P4 section of
[the provider plan](../memory-provider-completion.md).

## Existing behavior

`provider/claude.rs` uses isolated native CLI print/stream-json execution.
`provider/interaction.rs` already supplies exact owner identity, bounded requests, cancellation
and one-shot responses. `native_input.rs` deliberately refuses Claude activation because a
model-callable MCP permission tool alone does not authenticate a native approval request.

## Assignment

1. Establish the exact installed CLI/version contract for native permission requests, response
   routing, session/request correlation and cancellation. Cite official documentation and record
   bounded read-only observations. Internal SDK messages or a named MCP callback are not proof.
2. If the contract provides native provenance, implement a per-launch adapter into the existing
   broker. Preserve the standard adapter and native trust defaults when capability is absent.
3. Deliver foreground and worker approval routing through the coordinator's existing review UI;
   keep native options/restrictions and at-most-once response semantics.

Own a Claude-specific bridge module under `crates/concordia/src/provider/` (proposed new file;
coordinator registers it) and the Claude-specific evidence update. The coordinator owns edits to
`provider/claude.rs`, `provider/interaction.rs`, `provider/turn.rs`, `native_input.rs`,
`delegate.rs`, `tui/mod.rs` and `tui/questions.rs`. Return small integration patches.

## Acceptance and precise blocker

- Correct request reaches only its conversation/run/attempt/account/home/session and connection.
- Duplicate, late, wrong-owner, cancelled or arbitrary model-originated requests obtain no grant.
- One native response at most; process loss leaves uncertain work paused, never replayed.
- Unknown capability leaves the current working Claude adapter intact.
- Existing fake-provider/interaction cases prove parser and ownership behavior; native legitimacy
  and authorized live parity have separate evidence.

If supported native provenance cannot be established, return the CLI version, exact missing
contract, reviewed sources and any viable supported alternative. The refusal message already
exists: another unsupported stub is not a feature delivery. Mark the requirement vendor-blocked
and release the worker slot for another ready task. No native model turn or global hook is allowed.
