//! Bounded, cached Mermaid rendering off the input/render thread. No Node or browser.
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, OnceLock,
};

type Key = (String, usize);
type Rendered = Result<String, String>;
#[derive(Default)]
struct Cache {
    entries: BTreeMap<Key, Option<Rendered>>,
    order: VecDeque<Key>,
    active: usize,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
static REVISION: AtomicU64 = AtomicU64::new(0);
pub(super) fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

fn render(source: &str, width: usize) -> Rendered {
    let options = mermansi::MermansiOptions::unicode()
        .with_output_mode(mermansi::OutputMode::Concise)
        .with_max_width(width.clamp(12, 200))
        .with_max_height(128);
    std::panic::catch_unwind(|| mermansi::render_source(source, &options))
        .map_err(|_| "diagram renderer failed".to_owned())?
        .map_err(|_| "unsupported or invalid Mermaid; source retained".into())
        .and_then(|text| {
            if text.len() > 128 * 1024 {
                Err("diagram exceeds preview limit".into())
            } else {
                Ok(text)
            }
        })
}

pub(super) fn preview(source: &str, width: usize) -> Option<Rendered> {
    if source.len() > 8192 || source.lines().count() > 128 || source.matches(';').count() > 256 {
        return Some(Err("diagram exceeds preview limit; source retained".into()));
    }
    let key = (source.to_owned(), width.clamp(12, 200));
    let cache = CACHE.get_or_init(Default::default);
    let mut state = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(value) = state.entries.get(&key) {
        return value.clone();
    }
    if state.active >= 2 {
        return None;
    }
    if state.entries.len() >= 32 {
        let n = state.order.len();
        for _ in 0..n {
            if let Some(old) = state.order.pop_front() {
                if state.entries.get(&old).is_some_and(Option::is_some) {
                    state.entries.remove(&old);
                    break;
                }
                state.order.push_back(old);
            }
        }
    }
    state.order.push_back(key.clone());
    state.entries.insert(key.clone(), None);
    state.active += 1;
    let job_key = key.clone();
    if std::thread::Builder::new()
        .name("concordia-mermaid".into())
        .spawn(move || {
            let result = render(&job_key.0, job_key.1);
            let mut state = CACHE
                .get()
                .unwrap()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            state.entries.insert(job_key, Some(result));
            state.active = state.active.saturating_sub(1);
            REVISION.fetch_add(1, Ordering::Relaxed);
        })
        .is_err()
    {
        state.entries.insert(
            key,
            Some(Err("diagram worker unavailable; source retained".into())),
        );
        state.active -= 1;
        REVISION.fetch_add(1, Ordering::Relaxed);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flowcharts_render_as_boxes_and_arrows_and_cache_completion_is_observable() {
        let source = "flowchart LR\n A[Prompt] --> B[Provider]";
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            if let Some(result) = preview(source, 80) {
                let rendered = result.unwrap();
                assert!(rendered.contains("Prompt") && rendered.contains("Provider"));
                assert!(rendered.contains('\u{2500}'));
                assert!(!rendered.contains("flowchart LR"));
                break;
            }
            assert!(std::time::Instant::now() < end);
            std::thread::yield_now();
        }
        assert!(preview(&"x".repeat(8193), 80).unwrap().is_err());
        assert!(render("not a diagram", 80).is_err());
    }
}
