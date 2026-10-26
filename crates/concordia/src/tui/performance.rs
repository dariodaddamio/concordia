//! Reproducible, provider-free input/render stress measurement. Run in release mode explicitly.
use super::*;
use ratatui::backend::TestBackend;

#[test]
#[ignore = "explicit release-mode performance measurement"]
fn typing_while_output_streams_retains_draft_and_reports_latency() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().into(), dir.path().into());
    for i in 0..500 {
        app.messages.push(ChatLine {
            role: MsgRole::Assistant,
            text: format!(
                "Reply {i}\n{}\nSee src/example.rs:12 for details.",
                "A long transcript with Unicode 雪 and some **formatting**. ".repeat(12)
            ),
            card: None,
            meta: None,
        });
    }
    let original = "unfinished draft text with Unicode 雪\n".repeat(800);
    app.editor.insert_paste_chunk(&original, false);
    app.busy = true;
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| draw_ui(f, &mut app)).unwrap();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut samples = Vec::new();
    for i in 0..250 {
        let start = Instant::now();
        app.live_text
            .push_str("streaming output including a file src/example.rs and more text\n");
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            &tx,
        );
        app.tick_count = i;
        terminal.draw(|f| draw_ui(f, &mut app)).unwrap();
        samples.push(start.elapsed().as_micros());
    }
    assert_eq!(app.editor.text(), original + &"x".repeat(250));
    assert_eq!(app.messages.len(), 501);
    samples.sort_unstable();
    let p95 = samples[samples.len() * 95 / 100];
    let max = samples[samples.len() - 1];
    println!("STREAM_INPUT_BENCH samples={} transcript_messages=500 draft_chars={} p95_us={p95} max_us={max}",samples.len(),app.editor.chars().len());
    assert!(
        p95 < 100_000,
        "release-mode input plus draw exceeded 100ms p95: {p95}us"
    );
}
