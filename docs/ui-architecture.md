# UI architecture

## Product

Concordia is a **terminal app** you talk to. It should feel like a polished agent client. Codex, Claude
and Cursor are **backends** (your subscriptions and their tools), not the thing you live in.

## Non-negotiables

- One input for chat and orchestrator commands (`/use`, `/config`, ...).
- One logical conversation can continue across providers and accounts with **bounded** context
  handoff (connected context, minimal tokens).
- Real text editing in the prompt: click to place the caret, drag to select, copy and paste with
  visible success or failure.
- Multi-session through WezTerm panes and tabs; each pane runs its own Concordia.
- Clean-room look and feel: no provider UI source is copied.

## Backend

Concordia draws the transcript, prompt and slash UI. Turns go through the programmatic CLIs
(`codex exec`, `claude -p`, `agent --print`). Richer tool and command cards come from their
structured event streams, rendered by Concordia.

## Rendering model

The transcript is a list of wrapped rows (hanging gutters, soft-wrap aware) cached per message, so
a very long session redraws quickly. Selection, scrolling and hit-testing operate on those rows;
copied text rejoins soft wraps. The prompt is a separate multi-line editor with its own layout.

Turn state, its colours and animation, and the transcript separation rules are in [tui-states.md](tui-states.md).
