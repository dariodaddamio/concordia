# Concordia design system

This is Concordia's project identity layer. The portable layers come first and are not repeated
here: the principles, design method and lookup sequence in
[preferences/ui-core.md](preferences/ui-core.md), and the interaction rules in
[preferences/ui.md](preferences/ui.md) (search them with `habena run ui:search <topic>`).
Concordia renders a grid of character cells in a native Windows window, so every measure below is
in cells, and color is always a semantic role from [`tui/theme.rs`](../crates/concordia/src/tui/theme.rs),
never a raw color.

Status: draft for owner review (2026-11-14). Items marked *decision* are open.

## Grid and spacing

One cell is the unit. The scale has three steps: 0, 1 and 2 cells.

- The window has a 1-column margin on the left and right. Nothing touches the window edge except
  full-width rules and the composer border.
- Inside a bordered surface, content has 1 column of padding on each side and no blank padding row.
- Related lines sit together with no gap. Separate groups (two transcript turns, two sections of a
  list) with exactly one blank row. Two blank rows in a row are a defect.
- In a key or label column, the column is as wide as its longest entry plus 2 columns of gap.
- A surface fills the space it owns. A panel either ends at the composer or takes only the rows its
  content needs and leaves the rest of the transcript visible; it never floats with dead rows above
  or below it.

## Color roles

| Role | Use | Never use for |
| --- | --- | --- |
| `foreground` | Body text and headings | Secondary detail |
| `muted` | Secondary text: timestamps, metadata, hints, placeholders | Anything the user must act on |
| `accent` | Interactive things: keys, links, the selected row's border, focus | Decoration or headings |
| `success`, `warning`, `error` | Real state: limits, failures, completion | Emphasis or headings |
| `purple` | Code and command text | Prose |
| `panel` | Raised surfaces: code blocks, selected rows | Large backgrounds |
| `brand` | The logo and window icon only | Text |

The background is pure black unless the user picks another theme. Emphasis inside text is bold
foreground. Italic is reserved for placeholders in empty inputs.

## Type roles

A terminal has one font size, so hierarchy comes from weight, role color and position.

- Title: bold `foreground`, one per surface, in the border title when the surface has one.
- Section heading: bold `foreground`, followed by its rows with no gap, preceded by one blank row.
- Body: `foreground`.
- Secondary: `muted`, placed after or below the body it qualifies, never before it.
- Key: `accent`, in a key column.

## Text fitting

- Text never runs past a border or the window edge. Long text wraps at word boundaries; a single
  line that must stay one line ends with an ellipsis (`…`) at a word boundary when possible.
- Never cut inside a word without an ellipsis. Never cut a border, a key or a number.
- Column layouts compute their widths from the content and the available cells, then degrade in a
  fixed order: hide secondary text first, then wrap descriptions, then stack columns.

## Components

Each component has one implementation. A screen composes these; it does not restyle them.

- **Header bar** (row 0): navigation, build, provider and account, model. Values are full names
  (`Claude · Opus · claude-opus-5-5`), never aliases.
- **Status lines** under the header: at most one line, and only when it carries state the user
  needs now (*decision*: today three lines repeat the subscription, a typing tip and the account).
- **Transcript turn**: `You ›` line, then the provider and model line, then the reply, then one
  `muted` metadata line. One blank row between turns.
- **Card** (tool call, handoff): one line collapsed with a state icon, a title and `muted` detail;
  expands in place on click or Ctrl+O. The title says what happened, the detail never repeats it.
- **Table**: box-drawing borders, bold header row, a divider between rows, no divider after the
  last row.
- **Code block**: `purple` on `panel`, language name in `muted` above it.
- **List picker** (`/model`, `/use`, `/config`): bordered, title in the border, one row per item,
  the selected row marked by `>` and `accent`. Item label first, `muted` detail after a 2-column gap.
- **Grid picker** (`/usage`): cards in columns; arrow keys move in the direction pressed.
- **Footer**: one row of `Key action` pairs separated by ` · `. It is the only place that lists
  keys; panels and the composer do not restate them.
- **Composer**: bordered input; placeholder in italic `muted`.

## Motion and repainting

- The window repaints only when its content changed. Idle repaints are a defect.
- Images are painted once per change and never re-blitted on unrelated repaints.
- Animation (the running-tab indicator) is the only periodic change, and follows the Windows
  reduced-motion setting.

## How a change is checked

Every UI change is proven in the running app with `_work\briefs\chat-drive.ps1` screenshots at
150×44 and 90×30, and `_work\briefs\qa.ps1` for performance. Targets:

- 0 repaints per second while idle.
- Keystroke to paint: median 16 ms or less, worst 50 ms or less.
- No frame changes while idle.
- Every screenshot reviewed against the rules above.
