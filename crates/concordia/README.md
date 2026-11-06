# Concordia (Rust — public binary)

Thin local orchestration TUI. This crate is the **only** supported install path.

## Build

```powershell
cargo build -p concordia --release
.\scripts\install-concordia-rust.ps1
```

Needs Rust + MSVC linker (VS Build Tools C++ workload).

## TUI

`/use` `/model` `/usage` `/resume` `/trust` `/config` `/new` `/split` `/exit`

Footer: hints left, **auth {mode}** right. Ctrl+C cancels the tracked provider child.

Home records a chat only after the first user message. Empty provider session artifacts stay hidden;
`concordia sessions prune --empty` lists them and removes them only after you type `yes`.
