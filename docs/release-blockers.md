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

### 1. Fresh history - done

Published as a single clean root commit to a **private** repository
(the private `concordia` repository). The old history (which contained the removed material and a username) is
kept only locally: the `archive/pre-release` branch and `concordia-pre-release-history.bundle` next to the
repo. It was never pushed. Keep it that way: push only `main`.

### 2. Product name - decided: Concordia

The name is Latin *concordia*, "harmony, agreeing together": many providers in concord.
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
- [ ] `/update` from a real GitHub release (needs a tagged release in the repo)
- [x] The GitHub workflows run green on a real runner (CI and a release rehearsal both passed; the first run
      caught a real bug: `Get-FileHash` missing on a clean machine, now replaced by .NET hashing)
- [x] A CI-built release artifact verified as a stranger would: checksum matches, runs, installs
- [ ] One short real turn with a multi-line prompt (stdin path), under a budget you set

### 5. Product and release gaps

- Account handoff on exhaustion (user-confirmed, explicit session bind), after item 3
- Verify Claude/Cursor session binding against live supported CLI versions; synthetic stream session events are parsed and bound, but that does not establish full version parity
- Native approval interaction parity for headless runs; `plan`, `ask`, `workspace` and `full` modes exist, but modes alone do not prove every native approval flow works
- Image paste; picker mouse support
- Signed releases (checksums protect against corruption, not a compromised release)
- Linux and macOS (not claimed; CI is Windows-only)

### 6. Current production-readiness audit

See [production-plan.md](production-plan.md) and [command-audit.md](command-audit.md).
The previous command audit was not exhaustive; tests do not prove zero defects. Public release
remains blocked until known high-impact findings and required host/provider checks are resolved.

- WezTerm config is embedded in the binary and extracted on demand; the earlier source-checkout
  requirement is no longer an implementation blocker.
- Native-chat CLI listing and explicit-account reads support the discovered Codex/Claude text
  transcript formats. Cursor metadata can be listed, but native transcript reading is explicitly
  unsupported. Search currently searches previews/IDs, not every message in every transcript.
- File links and Markdown preview have synthetic/terminal checks; actual configured editor
  launch, clipboard/selection behavior and keyboard behavior across supported Windows hosts
  still need a human pass. WezTerm and an installed/configured terminal editor are required for
  separate editor tabs/windows through `/edit`. Installed `2026-10-21b` includes internal read-only
  `/open`/`/preview` panes with a 2 MiB/50,000-line cap, without an editor/WezTerm requirement.
  Internal editing, layout recovery and human host verification remain open.
- Expanded tool details retain provider-supplied data. A CLI event that omits a patch cannot
  produce native-quality edited-line diffs; Concordia must say what is missing, not fabricate
  before/after content from unrelated workspace changes. Complete native UI/MCP/edit parity
  against currently supported provider versions remains unverified.
- Invalid-argument, stale-cache, account-switch, busy/update, restart and corrupt-state coverage
  still needs to be completed for the enumerated surface. The command matrix records gaps.
- On Windows hosts exposing a paste as individual key events, paste inference still depends on
  source-observed quiet intervals. A bounded input reader now preserves those intervals across
  slow frames; extreme source scheduling/channel saturation remains a host stress-check gap.
- Worker limits interrupted the initial pass; workers were retried successfully after the
  requested 06:05 America/Phoenix time, when the user continued the active session. No unattended
  task was scheduled; this chat has no exposed scheduling capability.

## Re-scan

```powershell
powershell -NoProfile -File .\scripts\verify-oss-gates.ps1    # content scan of every tracked file
git archive HEAD | tar -t | Select-String -Pattern "\.ao/|\.env|node_modules"
gitleaks detect --source . -v                                  # secrets (install gitleaks first)
```
