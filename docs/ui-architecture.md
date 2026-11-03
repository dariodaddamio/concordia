# UI architecture

## Product

Concordia is a **terminal app** you talk to. It should feel like a polished agent client. Codex, Claude
and Cursor are **backends** (your subscriptions and their tools), not the thing you live in.

Image attachments show a bounded snapshot thumbnail above the composer without requiring a click.
The first attached image previews automatically; every image chip can open the enlarged viewer.
The thumbnail leaves typing active, and monochrome terminals use luminance characters. Pasted file
paths retain their text and offer explicit attachment through F9; a path alone is not an attachment.

The second header row separates native credit balance/availability from subscription limits. A
missing native balance reads `Credits: not reported`; the usage view also states when credit spend
is not reported separately. App title glyphs describe activity. The graphical Windows Terminal tab
icon belongs to the host profile and is independent of Concordia's title text.

The header email is hidden by default. Click its label to show or hide it; changing accounts
resets disclosure. The app name/version has no duplicate animated activity glyph. The usage view
links to the provider dashboard when more credit-balance detail is needed; absent native amounts
remain explicitly unreported.

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
