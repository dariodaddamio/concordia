//! Screenshot generator and draw benchmark (test-only).
//!
//! Renders the real `draw_ui` on a ratatui `TestBackend` with synthetic state and converts the
//! cell buffer into a standalone SVG. Regenerate after any UI change:
//!
//! ```text
//! cargo test -p concordia --lib screenshots -- --ignored
//! ```
//!
//! That writes `docs/img/{hero,palette,usage,waiting}.svg`. The ignored `bench_draw_ui` test prints
//! a `DRAW_UI_BENCH` line that `scripts/metrics.ps1` collects. Everything here is synthetic: no
//! provider CLI runs and no real account, path or name appears in the output.

use super::*;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use unicode_width::UnicodeWidthStr;

const CELL_W: u32 = 9;
const CELL_H: u32 = 19;
const PAD: u32 = 14;
const TITLE_H: u32 = 36;
const DEFAULT_FG: &str = "#d4d4d4";
const DEFAULT_BG: &str = "#14161b";
const FONT: &str = "Cascadia Mono, Consolas, monospace";

#[path = "demo_gifs.rs"]
mod demo_gifs;

// ---------------------------------------------------------------- buffer -> SVG

const ANSI16: [&str; 16] = [
    "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
    "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff",
];

fn indexed_hex(i: u8) -> String {
    match i {
        0..=15 => ANSI16[i as usize].to_string(),
        16..=231 => {
            let n = i - 16;
            let lvl = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            format!(
                "#{:02x}{:02x}{:02x}",
                lvl(n / 36),
                lvl((n / 6) % 6),
                lvl(n % 6)
            )
        }
        _ => {
            let g = 8 + 10 * (i - 232);
            format!("#{g:02x}{g:02x}{g:02x}")
        }
    }
}

/// `None` means the terminal default.
fn color_hex(c: Color) -> Option<String> {
    Some(match c {
        Color::Reset => return None,
        Color::Black => ANSI16[0].into(),
        Color::Red => ANSI16[1].into(),
        Color::Green => ANSI16[2].into(),
        Color::Yellow => ANSI16[3].into(),
        Color::Blue => ANSI16[4].into(),
        Color::Magenta => ANSI16[5].into(),
        Color::Cyan => ANSI16[6].into(),
        Color::Gray => ANSI16[7].into(),
        Color::DarkGray => ANSI16[8].into(),
        Color::LightRed => ANSI16[9].into(),
        Color::LightGreen => ANSI16[10].into(),
        Color::LightYellow => ANSI16[11].into(),
        Color::LightBlue => ANSI16[12].into(),
        Color::LightMagenta => ANSI16[13].into(),
        Color::LightCyan => ANSI16[14].into(),
        Color::White => ANSI16[15].into(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(i) => indexed_hex(i),
    })
}

fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o
}

#[derive(Clone, PartialEq, Eq)]
struct TextStyle {
    fg: String,
    bold: bool,
    italic: bool,
    underline: bool,
    dim: bool,
}

struct SvgCell {
    col: u32,
    width: u32,
    sym: String,
    bg: Option<String>,
    style: TextStyle,
}

fn resolve_cells(buf: &Buffer) -> Vec<Vec<SvgCell>> {
    let area = buf.area;
    let mut rows = Vec::new();
    for y in 0..area.height {
        let mut row = Vec::new();
        let mut skip = 0u32;
        for x in 0..area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let cell = &buf[(area.x + x, area.y + y)];
            let sym = cell.symbol().to_string();
            let width = (UnicodeWidthStr::width(sym.as_str()) as u32).clamp(1, 2);
            skip = width - 1;
            let m = cell.modifier;
            let mut fg = color_hex(cell.fg);
            let mut bg = color_hex(cell.bg);
            if m.contains(Modifier::REVERSED) {
                let f = fg.take().unwrap_or_else(|| DEFAULT_FG.into());
                let b = bg.take().unwrap_or_else(|| DEFAULT_BG.into());
                fg = Some(b);
                bg = Some(f);
            }
            row.push(SvgCell {
                col: x as u32,
                width,
                sym,
                bg,
                style: TextStyle {
                    fg: fg.unwrap_or_else(|| DEFAULT_FG.into()),
                    bold: m.contains(Modifier::BOLD),
                    italic: m.contains(Modifier::ITALIC),
                    underline: m.contains(Modifier::UNDERLINED),
                    dim: m.contains(Modifier::DIM),
                },
            });
        }
        rows.push(row);
    }
    rows
}

/// Converts a rendered buffer into a standalone SVG with window chrome.
fn buffer_to_svg(buf: &Buffer, title: &str) -> String {
    let cols = buf.area.width as u32;
    let nrows = buf.area.height as u32;
    let w = cols * CELL_W + 2 * PAD;
    let h = nrows * CELL_H + TITLE_H + PAD;
    let ox = PAD;
    let oy = TITLE_H;
    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" role=\"img\" aria-label=\"{}\">\n",
        xml_escape(title)
    ));
    s.push_str(&format!(
        "<title>{}</title>\n<defs><clipPath id=\"win\"><rect width=\"{w}\" height=\"{h}\" rx=\"12\"/></clipPath></defs>\n",
        xml_escape(title)
    ));
    s.push_str("<g clip-path=\"url(#win)\">\n");
    s.push_str(&format!(
        "<rect width=\"{w}\" height=\"{h}\" fill=\"{DEFAULT_BG}\"/>\n<rect width=\"{w}\" height=\"{TITLE_H}\" fill=\"#23262e\"/>\n"
    ));
    for (i, c) in ["#ff5f57", "#febc2e", "#28c840"].iter().enumerate() {
        s.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"6\" fill=\"{c}\"/>\n",
            PAD + 6 + i as u32 * 20,
            TITLE_H / 2
        ));
    }
    s.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-family=\"{FONT}\" font-size=\"12\" fill=\"#8b909c\" text-anchor=\"middle\">{}</text>\n",
        w / 2,
        TITLE_H / 2 + 4,
        xml_escape(title)
    ));
    let rows = resolve_cells(buf);
    // Backgrounds: merge adjacent cells with the same fill.
    for (y, row) in rows.iter().enumerate() {
        let mut i = 0;
        while i < row.len() {
            let Some(bg) = row[i].bg.clone() else {
                i += 1;
                continue;
            };
            let start = row[i].col;
            let mut end = row[i].col + row[i].width;
            let mut j = i + 1;
            while j < row.len() && row[j].bg.as_deref() == Some(bg.as_str()) {
                end = row[j].col + row[j].width;
                j += 1;
            }
            s.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{CELL_H}\" fill=\"{bg}\"/>\n",
                ox + start * CELL_W,
                oy + y as u32 * CELL_H,
                (end - start) * CELL_W
            ));
            i = j;
        }
    }
    // Text: one <text> per run of same-style cells that starts on ink.
    s.push_str(&format!(
        "<g font-family=\"{FONT}\" font-size=\"15\" xml:space=\"preserve\">\n"
    ));
    for (y, row) in rows.iter().enumerate() {
        let base = oy + y as u32 * CELL_H + CELL_H - 5;
        let mut i = 0;
        while i < row.len() {
            if row[i].sym.trim().is_empty() {
                i += 1;
                continue;
            }
            let style = row[i].style.clone();
            let start = row[i].col;
            let mut text = String::new();
            let mut end = start;
            let mut j = i;
            while j < row.len() && row[j].style == style && row[j].col == end {
                text.push_str(&row[j].sym);
                end += row[j].width;
                j += 1;
            }
            let trimmed = text.trim_end();
            let ink_cols = UnicodeWidthStr::width(trimmed) as u32;
            let mut attrs = format!("fill=\"{}\"", style.fg);
            if style.bold {
                attrs.push_str(" font-weight=\"700\"");
            }
            if style.italic {
                attrs.push_str(" font-style=\"italic\"");
            }
            if style.underline {
                attrs.push_str(" text-decoration=\"underline\"");
            }
            if style.dim {
                attrs.push_str(" fill-opacity=\"0.6\"");
            }
            s.push_str(&format!(
                "<text x=\"{}\" y=\"{base}\" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\" {attrs}>{}</text>\n",
                ox + start * CELL_W,
                ink_cols * CELL_W,
                xml_escape(trimmed)
            ));
            i = j;
        }
    }
    s.push_str("</g>\n</g>\n");
    s.push_str(&format!(
        "<rect x=\"0.5\" y=\"0.5\" width=\"{}\" height=\"{}\" rx=\"12\" fill=\"none\" stroke=\"#3a3f4b\"/>\n</svg>\n",
        w - 1,
        h - 1
    ));
    s
}

// ---------------------------------------------------------------- scenes

struct Shot {
    app: App,
    term: Terminal<TestBackend>,
    _dir: tempfile::TempDir,
    _guard: std::sync::MutexGuard<'static, ()>,
    _base: tempfile::TempDir,
}

impl Shot {
    fn new(w: u16, h: u16) -> Self {
        let guard = crate::accounts::TEST_ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let base = tempfile::tempdir().unwrap();
        std::env::set_var("CONCORDIA_CODEX_HOME_BASE", base.path());
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        app.no_color = false;
        app.provider = ProviderKind::Codex;
        app.account = "codex-3".into();
        app.account_email = Some("member@example.invalid".into());
        app.model = "gpt-5.1-codex".into();
        app.usage_summary = "5h 59% left \u{b7} 7d 41% left".into();
        let term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut s = Shot {
            app,
            term,
            _dir: dir,
            _guard: guard,
            _base: base,
        };
        s.draw();
        s
    }

    fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| draw_ui(f, app)).unwrap();
    }

    fn key(&mut self, c: char) {
        let (tx, _rx) = mpsc::unbounded_channel();
        handle_key(
            &mut self.app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            &tx,
        );
        self.draw();
    }

    fn conversation(&mut self) {
        self.app
            .push_user("Add a /doctor check for duplicate accounts and show me the diff".into());
        for (id, title, detail, status) in [
            ("1", "shell", "rg -n duplicate src", CardStatus::Done),
            ("2", "edit", "src/doctor.rs", CardStatus::Done),
            ("3", "shell", "cargo test -p concordia", CardStatus::Running),
        ] {
            self.app.push_card(StreamCard {
                id: id.into(),
                title: title.into(),
                detail: Some(detail.into()),
                status,
            });
        }
        self.app.push_assistant(
            "## What I changed\n\nI added a **duplicate check** to `doctor`. It groups accounts by provider and email, so two slots signed in to the same login are reported once.\n\n- `accounts::duplicate_groups` finds the groups\n- `doctor` prints a warning per group, naming both accounts and the fix\n- the `/use` picker marks each side with *same login as ...*\n\n```rust\nfor group in duplicate_groups(root) {\n    warn(&group);\n}\n```\n\n1. Run `/doctor` to see it\n2. Use `/use remove` to drop one".into(),
        );
        self.app.push_user("looks good, thanks".into());
        self.draw();
    }

    fn svg(&self, title: &str) -> String {
        buffer_to_svg(self.term.backend().buffer(), title)
    }
}

const W: u16 = 110;
const H: u16 = 32;

fn hero() -> String {
    let mut s = Shot::new(W, H);
    s.conversation();
    s.svg("concordia")
}

fn attachment() -> String {
    let mut s = Shot::new(W, H);
    s.conversation();
    s.app.usage_credits = "Credits: 12.5 balance".into();
    let path = s._dir.path().join("diagram.png");
    image::RgbaImage::from_fn(64, 32, |x, y| {
        if (6..25).contains(&x) && (8..24).contains(&y) {
            image::Rgba([47, 175, 202, 255])
        } else if (39..58).contains(&x) && (8..24).contains(&y) {
            image::Rgba([167, 116, 233, 255])
        } else if (25..39).contains(&x) && (15..18).contains(&y) {
            image::Rgba([220, 224, 235, 255])
        } else {
            image::Rgba([25, 29, 39, 255])
        }
    })
    .save(&path)
    .unwrap();
    s.app.images.draft = vec![crate::attachments::Attachment::Image(
        crate::attachments::import(&s.app.state_root, &path).unwrap(),
    )];
    s.app.editor.set_text("Explain this diagram");
    s.app.ensure_inline_raster();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        s.app.raster.poll();
        s.draw();
        if s.term
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.symbol() == "▀")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "automatic preview did not appear"
        );
        std::thread::yield_now();
    }
    s.svg("concordia · attachment preview")
}

fn palette() -> String {
    let mut s = Shot::new(W, H);
    s.conversation();
    for ch in "/use".chars() {
        s.key(ch);
    }
    s.svg("concordia \u{2014} command options")
}

fn snap(windows: &[(&str, f64, &str)]) -> crate::usage::UsageSnapshot {
    let windows: Vec<crate::usage::UsageWindow> = windows
        .iter()
        .map(|(l, u, r)| crate::usage::UsageWindow {
            label: (*l).into(),
            used_pct: *u,
            resets: Some((*r).into()),
        })
        .collect();
    crate::usage::UsageSnapshot {
        credits: None,
        plan: None,
        summary: String::new(),
        detail: String::new(),
        primary_pct: windows.first().map(|w| w.used_pct),
        weekly_pct: windows.get(1).map(|w| w.used_pct),
        windows,
        limit_hit: false,
    }
}

fn usage() -> String {
    let mut s = Shot::new(W, H);
    s.conversation();
    // Rows come from the real picker_row formatter; the numbers are synthetic.
    let entries = [
        (
            "codex-3",
            ProviderKind::Codex,
            snap(&[("5h", 41.0, "3h"), ("7d", 59.0, "3d")]),
        ),
        (
            "claude-1",
            ProviderKind::Claude,
            snap(&[("5h", 23.0, "4h"), ("7d", 64.0, "2d")]),
        ),
        (
            "claude-2",
            ProviderKind::Claude,
            snap(&[("5h", 92.0, "1h")]),
        ),
        (
            "cursor-1",
            ProviderKind::Cursor,
            crate::usage::cursor_usage(),
        ),
    ];
    let mut rows = Vec::new();
    for (id, provider, sn) in entries {
        let identity = crate::identity::ProviderIdentity {
            provider,
            home_path: "synthetic".into(),
            login: Some("member@example.invalid".into()),
            plan: Some(
                if provider == ProviderKind::Codex {
                    "plus"
                } else {
                    "pro"
                }
                .into(),
            ),
            team: None,
            model: None,
            logged_in: true,
        };
        let r = crate::usage::profiled_picker_row(id, provider, &sn, &identity);
        s.app.picker_tones.insert(format!("usage:{}", r.id), r.tone);
        rows.push(PickerRow {
            id: format!("usage:{}", r.id),
            label: r.label,
            detail: r.detail,
        });
    }
    s.app
        .show_picker(PickerKind::Usage, "usage".into(), rows, false);
    s.draw();
    s.svg("concordia \u{2014} usage")
}

fn waiting() -> String {
    let mut s = Shot::new(W, H);
    s.app
        .push_user("Why does the nightly sync skip rows with a null region?".into());
    for (id, title, detail, status) in [
        ("1", "shell", "rg -n region src/sync", CardStatus::Done),
        ("2", "read", "src/sync/filter.rs", CardStatus::Done),
        ("3", "shell", "cargo test sync::filter", CardStatus::Running),
    ] {
        s.app.push_card(StreamCard {
            id: id.into(),
            title: title.into(),
            detail: Some(detail.into()),
            status,
        });
    }
    s.app.busy = true;
    s.app.pulse_on = true;
    s.app.tick_count = 3;
    s.app.phase = Phase::Thinking;
    s.app.turn_started = Some(Instant::now() - Duration::from_secs(500));
    s.app.live_thinking = "Checking how the region filter treats a null before it compares.\nThe comparison runs first, so a missing region never matches and the row is dropped.\nRunning the filter tests to confirm before proposing a fix.".into();
    s.draw();
    s.svg("concordia \u{2014} working")
}

// ---------------------------------------------------------------- validation

/// Minimal well-formedness check: balanced tags, no stray `<` or bare `&`, one root.
fn check_xml(doc: &str) -> Result<(), String> {
    let b: Vec<char> = doc.chars().collect();
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    let mut roots = 0;
    while i < b.len() {
        match b[i] {
            '<' => {
                let end = b[i..]
                    .iter()
                    .position(|&c| c == '>')
                    .ok_or("unterminated tag")?
                    + i;
                let inner: String = b[i + 1..end].iter().collect();
                if inner.contains('<') {
                    return Err(format!("stray < in tag: {inner}"));
                }
                if let Some(name) = inner.strip_prefix('/') {
                    if stack.pop().as_deref() != Some(name.trim()) {
                        return Err(format!("mismatched close {name}"));
                    }
                } else if !inner.starts_with('?') && !inner.starts_with('!') {
                    let name: String = inner
                        .chars()
                        .take_while(|c| !c.is_whitespace() && *c != '/')
                        .collect();
                    if stack.is_empty() {
                        roots += 1;
                    }
                    if !inner.ends_with('/') {
                        stack.push(name);
                    }
                }
                i = end + 1;
            }
            '&' => {
                let rest: String = b[i..(i + 6).min(b.len())].iter().collect();
                if !["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"]
                    .iter()
                    .any(|e| rest.starts_with(e))
                {
                    return Err(format!("bare ampersand near {rest:?}"));
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    if !stack.is_empty() {
        return Err(format!("unclosed {stack:?}"));
    }
    if roots != 1 {
        return Err(format!("expected one root element, found {roots}"));
    }
    Ok(())
}

fn scenes() -> Vec<(&'static str, String)> {
    vec![
        ("hero", hero()),
        ("palette", palette()),
        ("usage", usage()),
        ("waiting", waiting()),
        ("attachment", attachment()),
    ]
}

#[test]
fn svg_structure_is_valid_and_carries_the_screen_text() {
    let expected = [
        ("hero", "What I changed"),
        ("palette", "/use"),
        ("usage", "Cursor 1"),
        ("waiting", "Working through the next step"),
        ("attachment", "Image 1: diagram.png"),
    ];
    for ((name, svg), (n2, needle)) in scenes().into_iter().zip(expected) {
        assert_eq!(name, n2);
        check_xml(&svg).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(svg.starts_with("<svg xmlns="), "{name}");
        assert!(svg.contains("Cascadia Mono, Consolas, monospace"), "{name}");
        assert!(svg.contains("textLength="), "{name}");
        assert_eq!(svg.matches("<circle").count(), 3, "{name}: window dots");
        assert!(svg.contains(needle), "{name}: missing {needle:?}");
    }
    let s = buffer_to_svg(
        &{
            let mut b = Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 1));
            b.set_string(
                0,
                0,
                "a<&>\u{4e2d}",
                Style::default().fg(Color::Indexed(196)),
            );
            b
        },
        "t",
    );
    check_xml(&s).unwrap();
    assert!(
        s.contains("a&lt;&amp;&gt;\u{4e2d}") && s.contains("#ff0000"),
        "{s}"
    );
    assert!(check_xml("<a><b></a>").is_err());
}

#[test]
fn xterm_palette_matches_known_entries() {
    assert_eq!(indexed_hex(16), "#000000");
    assert_eq!(indexed_hex(231), "#ffffff");
    assert_eq!(indexed_hex(232), "#080808");
    assert_eq!(indexed_hex(255), "#eeeeee");
    assert_eq!(indexed_hex(114), "#87d787");
}

/// Writes docs/img/*.svg. Run: cargo test -p concordia --lib screenshots -- --ignored
#[test]
#[ignore]
fn write_screenshots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/img");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, svg) in scenes() {
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
}

/// Times `draw_ui` over 500 frames of the busy transcript. Prints one `DRAW_UI_BENCH` line.
/// Run: cargo test --release -p concordia --lib screenshots::bench_draw_ui -- --ignored --nocapture
#[test]
#[ignore]
fn bench_draw_ui() {
    let mut s = Shot::new(W, H);
    s.conversation();
    s.app.busy = true;
    for _ in 0..20 {
        s.draw();
    }
    let mut us: Vec<u128> = Vec::with_capacity(500);
    for i in 0..500u64 {
        s.app.tick_count = i;
        let t = Instant::now();
        s.draw();
        us.push(t.elapsed().as_micros());
    }
    us.sort_unstable();
    let mean = us.iter().sum::<u128>() as f64 / us.len() as f64;
    let p95 = us[(us.len() * 95 / 100).min(us.len() - 1)];
    println!("DRAW_UI_BENCH frames=500 size={W}x{H} mean_us={mean:.1} p95_us={p95}");
}
