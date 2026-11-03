# Draft UI GIFs

- `concordia-file-preview.gif`: 12.7 seconds, 1284 × 696. Open a chat link, review formatted
  Markdown, switch to raw, change one status, save, and review the updated table. Draft preserved.
- `concordia-command-controls.gif`: 11.2 seconds, 1284 × 852. A simulated provider limit pauses
  a task; explicit account continuation keeps the original request and completed checks in view.

Both are looping GIFs under 1 MB, generated from Concordia's actual renderer and input/continuation
handlers. Larger text, detail crops, captions, cursor movement and click feedback are editorial
overlays. Pointer shapes use real hit targets; the overlay does not certify host cursor support.
The visible data, provider limit and replies are synthetic; dispatch goes to an in-memory fake
channel. No model turn or real account change is performed. These are draft demonstrations, not recordings
of live provider success and not proof of whole-history memory or proactive rollover.

Rebuild from the repository root, using the local Rust toolchain and Python/Pillow:

```
cargo test -p concordia --lib write_gif_frames -- --ignored
python scripts/render-demo-gifs.py
```

Intermediate frame JSON and review sheets stay under ignored `target/gif-frames/`. The Python
renderer checks looping animation, frame counts and the 5 MB export target. Fonts come from the
standard Windows font directory; no paid service or inference is used. Nothing is published.
