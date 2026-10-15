# Release blockers

What stands between this private repo and a public `v0.1.0`. Items marked **needs you** cannot be
decided or verified by the code alone.

## Done and enforced

| Gate | Where |
| --- | --- |
| `cargo fmt --check`, `clippy -D warnings`, all tests `--locked` | `.github/workflows/ci.yml` |
| Pinned toolchain so lints cannot drift | `rust-toolchain.toml` |
| Release binary smoke (size budget, stamp, read-only `--version`, help surface, `doctor` contract) | `scripts/smoke-ci.ps1` |
| Dependency licenses, advisories, sources | `cargo deny check` (`deny.toml`) |
| Third-party notices shipped with every release | `cargo about` -> `THIRD-PARTY-LICENSES.txt` |
| Tag must equal the crate version; gates re-run before publishing | `.github/workflows/release.yml` |
| Stranger-safe update path (checksum + runs + right version, else nothing installed) | `crates/concordia/src/release.rs` |
| No mentions of private projects, no personal data (username) in any tracked file | `scripts/verify-oss-gates.ps1` |
| Rust-only product (the TypeScript prototype and research spikes are removed) | `scripts/verify-oss-gates.ps1` |

## Blockers

### 1. Git history still contains the removed material - needs you

The working tree is clean, but earlier commits in this local repository still contain the removed
files (private project names, the TypeScript prototype, evidence dumps with a username and local
paths). Before the first push, publish from a fresh history: create an orphan branch from the
current tree (keeping the old history only in your private clone), or squash. Nothing has been
pushed yet.

### 2. Product name - decided: Concordia

Renamed from `orc` (Latin *concordia*, "harmony, agreeing together": many providers in concord).
The command is `concordia`. `concordia` is free on crates.io. Still to do before publishing: check
the name against trademarks and existing products, and pick the GitHub organization and repository.

### 3. Provider terms - needs you

See [provider-terms.md](provider-terms.md). Anthropic's text is read and Concordia's design fits it;
OpenAI's Terms of Use and Cursor's terms still need a manual read. The failover feature must not
ship until that is done.

### 4. Live verification - needs you

Covered by tests with fakes, never run against the real thing. (Nothing here asks an agent to
spend tokens except the last item, which needs your explicit budget.)

- [ ] `/use add codex` opens a real login window and the "connected" announcement fires
- [ ] Mouse selection, click-to-caret, wheel and right-click in a live WezTerm pane
- [ ] `concordia install` from a fresh download, then `concordia --version` in a new terminal
- [ ] `/update` from a real GitHub release (needs a published repo)
- [ ] The GitHub workflows run green on a real runner (no remote exists yet)
- [ ] One short real turn with a multi-line prompt (stdin path), under a budget you set

### 5. Not built yet (product, not packaging)

- Account handoff on exhaustion (user-confirmed, explicit session bind), after item 3
- Claude and Cursor session-id binding after turns
- Approval prompts for headless runs (only `ask` / `workspace` / `full` trust modes exist)
- A shipped WezTerm config (today `/split` needs a source checkout to find it)
- Image paste; picker mouse support
- Signed releases (checksums protect against corruption, not a compromised release)
- Linux and macOS (not claimed; CI is Windows-only)

## Re-scan

```powershell
powershell -NoProfile -File .\scripts\verify-oss-gates.ps1    # content scan of every tracked file
git archive HEAD | tar -t | Select-String -Pattern "\.ao/|\.env|node_modules"
gitleaks detect --source . -v                                  # secrets (install gitleaks first)
```
