# File navigation audit

Historical scoped evidence from 2026-10-21. The editor/layout limits and URL exclusions
below describe that implementation, not the current product. For current navigation and
editing behavior, use [workspace controls](workspace-panes.md) and
[chat rendering](chat-rendering.md).

Source follow-up `2026-10-21a`: `/open`, `/preview` and clicked links now use internal read-only
file panes. `/edit` retains the separate-terminal editor described below. Internal panes read at
most 2 MiB/50,000 UTF-8 lines off the UI thread, bind results to stable pane ids, and support
mouse/keyboard focus, scroll, divider resize, X and `/close`. No internal editing or layout
recovery is claimed. See [workspace requirements](workspace-panes.md) and the new workspace tests.
The original parser/external-launch audit below remains applicable to those modules.

File navigation is triggered by an explicit click or `/open` / `/preview` command. Parsing returns
source byte ranges and candidate destinations; it does not read files or launch processes. Click
coordinates are converted by the TUI to source character ranges, then a candidate is resolved
relative to the active conversation working directory. Resolution canonicalizes the exact path,
requires a regular file, and keeps optional one-based line and column references. A missing or
invalid candidate produces a visible TUI message.

The parser examines at most 64 KiB of a text row and 1 KiB per candidate. It recognizes Markdown
`[label](target)` links (including angle-bracket destinations, nested parentheses, and escaped
spaces/parentheses), inline-code paths, rooted and drive paths, relative paths with separators,
and filename-shaped references. Markdown label ranges identify the visible clickable text while
the target remains separate. It accepts `#L12`, `:12`, and `:12:3`; zero line or column values are
rejected. Unicode source ranges are UTF-8 byte ranges, so callers must convert the TUI character
offset before matching.

URLs and UNC destinations are excluded. Plain text with ambiguous spaces relies on path-shaped
syntax and exact filesystem resolution; a bare filename containing spaces is best expressed in
Markdown or inline code. Plain detection uses a short lookahead, so prose near a path can produce
overlapping candidates. The TUI must resolve candidates covering the clicked span and prefer the
longest exact existing candidate. When two real files make a prose-prefixed path and intended path
both exact matches, plain text cannot disambiguate them; Markdown links provide an explicit target.
Escaped Markdown title syntax, reference-style Markdown links, tilde expansion, and general URL
unescaping are outside the current parser contract.

Opening uses WezTerm's supported argument-array interfaces: [`wezterm cli spawn`](https://wezterm.org/cli/cli/spawn.html)
with `--pane-id ID --cwd DIR -- PROGRAM ...` creates a new tab from a pane, while [`wezterm start`](https://wezterm.org/cli/general.html)
with `--cwd DIR -- PROGRAM ...` opens a separate terminal when Concordia has no WezTerm pane id. Configured editor executable and
each argument remain separate values; `{file}`, `{line}`, and `{column}` expand within an argument
without reprocessing inserted file text. If no `{file}` placeholder is configured, the path is
appended as one argument. Launching occurs asynchronously and no editor or preview command is run
while parsing or rendering. Markdown/text preview passes an existing resolved file path as an argv
element to Concordia's read-only bounded Markdown/text preview command; the viewer owns UTF-8 and
size validation, and it never evaluates the document.

Evidence in the module tests covers Unicode byte ranges, Markdown targets with spaces and nested
parentheses, escaped spaces, line/column parsing and zero rejection, URL/UNC exclusion, exact
working-directory resolution, and argument boundaries for whitespace, Unicode, shell metacharacters,
position placeholders, and preview dispatch. The tests build launch plans only; they do not launch
WezTerm, editors, provider CLIs, or Markdown content.
