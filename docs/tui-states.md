# Showing agent turn state in a terminal UI

A design pattern for a terminal chat tool that drives an AI agent. It covers how to show what a
long-running turn is doing, how the transcript separates kinds of output, and how to keep the
interface readable without colour. The code excerpts come from the reference implementation
(`tui/mod.rs`, `provider/stream_json.rs`), trimmed.

## 1. The problem

An agent turn can run for minutes. It may think, call tools, edit files, stream text, or stall.
A single spinner with no words tells the user only that *something* is happening. They cannot
tell a healthy long think from a hung tool, and they reach for Ctrl+C too early or too late.

The fix is to keep a **named state** on screen for the whole turn, and to keep it honest: the
state changes when the provider's events change, and the elapsed time keeps counting so a stall
is visible.

## 2. The state model

A turn is always in exactly one state:

| State | Meaning |
| --- | --- |
| waiting | Request sent, no event back yet. |
| thinking | Provider streams reasoning. |
| tool | A tool call is running. The label names the tool. |
| writing | Provider streams answer text. |
| done | Turn finished with an answer. Not a live state; the UI returns to idle. |
| failed | Provider exited non-zero or could not start. Shown as transcript rows. |
| cancelled | User interrupted. Shown as a transcript row. |

Only waiting, thinking, tool and writing are *live* states with their own animation. Done, failed
and cancelled are *outcomes*: the live title goes away and the result is written into the
transcript. Keep that split in your own code; it keeps the live area small.

### Per-state table

Colour uses the 256-colour index first. The 16-colour fallback is the named ANSI colour, which is
the same index 0-15 in a 256-colour palette, so a terminal with only 16 colours shows the same hue.

| State | Colour (ANSI-256) | Colour (16-colour fallback) | Animation frames (width-1 braille) | Label text | Border and title | Elapsed timer |
| --- | --- | --- | --- | --- | --- | --- |
| waiting | 6 | Cyan | 10 frames, U+280B U+2819 U+2839 U+2838 U+283C U+2834 U+2826 U+2827 U+2807 U+280F | `awaiting provider output` | border in state colour; title: frame, bold label | shown |
| thinking | 5 | Magenta | 8 frames, U+2801 to U+2808 (dot patterns that rise and fall) | `thinking` | as waiting | shown |
| tool | 3 | Yellow | 8 frames, U+28FE U+28FD U+28FB U+28BF U+287F U+28DF U+28EF U+28F7 | `running <tool name>` | as waiting | shown |
| writing | 2 | Green | 12 frames, U+2809 to U+2834 and back (a wave) | `writing` | as waiting | shown |
| done | n/a (idle border 240) | n/a (idle: DarkGray) | none | none; title reads `message` | idle border | hidden |
| failed | 1 (suggested; the reference uses the transcript's system colour) | Red | none | transcript row: `provider exited with code N` or `provider could not run`, then stderr tail | idle border | hidden |
| cancelled | 8 (bright black) | DarkGray | none | transcript row: `cancelled` | idle border | hidden |

Notes on the table:

- **Frames are width-1 braille only** (U+2800 to U+28FF). Some terminals render ambiguous-width
  glyphs (many symbols, emoji, and some box or arrow characters) two cells wide, or disagree with
  the application about their width. A two-cell glyph in the title pushes the caret of the prompt
  on the same screen row out of place. Braille cells are one cell in every terminal we tested, so
  the animation cannot shift anything. Braille patterns have East Asian Width "Neutral", so they are
  one cell wide under the usual width tables. Do not use emoji spinners.
- **Motion and the state are both signals.** Each live state has its own frame set, so a user can
  tell states apart by motion as well as by colour and label.
- **Idle** is the border at 240 (grey) and the title `message`, with no timer.

## 3. Rules

1. **One state visible at a time.** Only one title, one spinner and one label. Never show two
   states at once (for example "thinking" and "running a tool" in the same title).
2. **State follows the latest event.** Each provider event sets the state. A tool result sets it
   back to waiting; a text delta sets it to writing; a reasoning delta sets it to thinking. Do not
   keep a state alive on a timer alone.
3. **Never animate faster than about 10 fps.** The reference ticks every 200 ms (5 fps) and
   advances the frame index once per tick. Faster motion is distracting and costs redraws over
   SSH.
4. **Emphasis must survive `NO_COLOR`.** When `NO_COLOR` is set (non-empty), reset the foreground
   and background of every cell after drawing, but keep modifiers. Emphasis uses bold, italic and
   reverse only. The label is bold, so it stays emphasised without colour.
5. **Reasoning is shown dim and italic.** The live reasoning block shows the last **3 lines** only,
   in dim grey (index 244) italic, behind a `~` gutter. When the turn ends, collapse it into a
   single reasoning row in the same style, and keep it out of the context you send back to the
   model. Condense whitespace before display: collapse runs of spaces in each line, drop empty
   lines, and cap the total at about 600 characters, keeping the tail with a `...` prefix.
6. **Timer.** Show whole seconds since the turn started, redrawn every tick. Include the cancel
   hint in the same title (for example `Esc to cancel`).

## 4. Command output versus agent reply

Users must tell at a glance what the tool said from what the model said. Use three cues, and
never rely on colour alone:

| Kind | Gutter | Colour | Body |
| --- | --- | --- | --- |
| Agent reply | `●` | warm (index 179) | rendered as markdown |
| Command or system output | `·` | dark grey | plain text, no markdown |
| Tool card | two spaces | status colour (cyan running, red error, muted green done) | one-line title plus a short detail |
| Reasoning | `~` | magenta gutter, dim italic body | plain text |

Put a blank row between turns so each turn reads as one block. The gutter glyph carries the
meaning when colour is off.

## 5. Repeated-error de-duplication

Providers often report one failure several ways: as an event, as a final result, and again on
stderr. Without care the transcript fills with the same line.

The reference `push_system` compares the trimmed new text with the **last** transcript row. If it
is a system row with the same text, it drops the new one. This is simple and covers the common
case. Its limit is that it only catches *consecutive* repeats. If another row lands between two
identical errors, both show. If you need that, keep a count on the last row and render `x3`
instead of appending a new row.

## 6. Code

### Rust (ratatui), trimmed from the reference

State and presentation, one enum with the four live states:

```rust
#[derive(Clone, PartialEq, Eq)]
enum Phase {
    Waiting,
    Thinking,
    Tool(String),
    Writing,
}

impl Phase {
    fn label(&self) -> String {
        match self {
            Phase::Waiting => "awaiting provider output".into(),
            Phase::Thinking => "thinking".into(),
            Phase::Tool(name) => format!("running {name}"),
            Phase::Writing => "writing".into(),
        }
    }

    fn color(&self) -> Color {
        match self {
            Phase::Waiting => Color::Cyan,
            Phase::Thinking => Color::Magenta,
            Phase::Tool(_) => Color::Yellow,
            Phase::Writing => Color::Green,
        }
    }

    fn frames(&self) -> &'static [&'static str] {
        match self {
            Phase::Waiting => &["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}",
                                "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}"],
            Phase::Thinking => &["\u{2801}", "\u{2802}", "\u{2804}", "\u{2840}",
                                 "\u{2880}", "\u{2820}", "\u{2810}", "\u{2808}"],
            Phase::Tool(_) => &["\u{28fe}", "\u{28fd}", "\u{28fb}", "\u{28bf}",
                                "\u{287f}", "\u{28df}", "\u{28ef}", "\u{28f7}"],
            Phase::Writing => &["\u{2809}", "\u{2819}", "\u{281a}", "\u{2812}", "\u{2832}",
                                "\u{2834}", "\u{2824}", "\u{2834}", "\u{2832}", "\u{2812}",
                                "\u{281a}", "\u{2819}"],
        }
    }
}

// Each event sets the state (state follows the latest event).
fn apply_event(&mut self, ev: Event) {
    match ev {
        Event::Thinking(t) => { self.phase = Phase::Thinking; self.live_thinking.push_str(&t); }
        Event::Text(t)     => { self.phase = Phase::Writing;  self.live_text.push_str(&t); }
        Event::Tool { title, running } => {
            self.phase = if running { Phase::Tool(title) } else { Phase::Waiting };
        }
        _ => {}
    }
}

// Title of the input box while busy. `tick` advances every 200 ms.
fn title(phase: &Phase, tick: u64, elapsed_secs: u64) -> Line<'static> {
    let color = phase.color();
    let f = phase.frames();
    Line::from(vec![
        Span::styled(f[tick as usize % f.len()], Style::default().fg(color)),
        Span::styled(format!(" {}", phase.label()),
                     Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" \u{b7} {elapsed_secs}s \u{b7} Esc to cancel "),
                     Style::default().fg(Color::Gray)),
    ])
}

// Repeated-error de-duplication: skip when the last row is the same system text.
fn push_system(&mut self, text: String) {
    if self.messages.last().is_some_and(|m| m.role == MsgRole::System
        && m.text.trim() == text.trim()) {
        return;
    }
    self.messages.push(ChatLine { role: MsgRole::System, text, card: None });
}
```

### Framework-agnostic pseudo-code

```
STATES = { WAITING, THINKING, TOOL, WRITING }        # live
#  DONE / FAILED / CANCELLED are outcomes written to the transcript

on turn_start:
    state = WAITING; turn_started = now(); tick = 0

on event(e):                                  # state follows the latest event
    if e is reasoning:      state = THINKING; live_reasoning += e.text
    if e is answer_text:    state = WRITING;  live_text += e.text
    if e is tool_started:   state = TOOL(e.name)
    if e is tool_finished:  state = WAITING
    if e is error:          push_system_once(e.text)

every 200 ms while busy:                      # <= 5 fps, well under the 10 fps ceiling
    tick += 1
    draw_title(
        frame   = FRAMES[state][tick mod len(FRAMES[state])],   # braille only
        colour  = COLOUR[state],
        label   = LABEL[state],                 # "running <name>" for TOOL
        secs    = floor(now() - turn_started),
        hint    = "Esc to cancel")
    draw_live_reasoning(last_lines(condense(live_reasoning), 3))  # dim italic

on turn_end(outcome):
    live_title = none
    if outcome.reasoning:  append_row(REASONING, condense(live_reasoning))  # collapsed
    if outcome.cancelled:  push_system_once("cancelled")
    elif outcome.failed:   push_system_once(outcome.message)
    else:                  append_row(AGENT_REPLY, outcome.text)

push_system_once(text):
    if last_row is SYSTEM and last_row.text.trim() == text.trim(): return
    append_row(SYSTEM, text)

condense(s):  collapse spaces per line, drop empty lines, keep last 600 chars with "..." prefix

render(frame):
    if NO_COLOR is set: for each cell, reset fg and bg; keep bold, italic, reverse
```

## 7. Accessibility

- **Never colour alone.** Every state has a text label, a distinct frame set and a gutter glyph.
  Colour is a reinforcement.
- **Respect `NO_COLOR`.** Emphasis uses bold, italic and reverse, which survive a colourless
  terminal.
- **Low-motion option.** Offer a setting that freezes the frame and keeps the label and timer
  updating once per second. Motion-sensitive users and screen-reader users benefit.
- **Screen readers.** Put the state in plain text in the title, not only in the glyph. Avoid
  announcing every tick; announce on state change or at most every few seconds.
- **Contrast.** The dim reasoning colour (index 244) needs a background with enough contrast on
  both light and dark terminals. Check it on each theme you ship.
- **Predictable timing.** The timer counts real time, so a slow provider looks slow rather than
  frozen.


## Live status and elapsed time (2026-10-31)

The main prompt names the account while waiting: `Codex: awaiting first output` before any tool
card, or `Codex: awaiting next output` after a tool completes. These labels describe observable
output rather than assuming that an unreported provider is thinking or connected. Reasoning,
running tools and answer text still drive their own phase labels.

The timer immediately to the right is elapsed wall-clock time for the whole turn, always in
only necessary leading units, for example `8m 20s` or `1h 02m 03s`. It resets for a new turn, not for each phase.
It is not billed usage, a countdown or an estimate of remaining work.

The [UI glossary](ui-glossary.md) documents animations, account nicknames, active email and file/chat reference controls.
