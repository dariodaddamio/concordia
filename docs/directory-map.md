# Directory map

Concordia is a Cargo workspace with one product crate. This map describes repository ownership;
provider account data and runtime records live outside it. There is no JavaScript app workspace.

## Tree

| Path | Purpose | Notes |
| --- | --- | --- |
| `crates/concordia/src` | Product, CLI and local coordination | [Architecture](architecture.md) identifies module owners |
| `crates/concordia/src/provider` | Native transport, launch and structured event adapters | Native authentication/approvals remain authoritative |
| `crates/concordia/src/tui` | Shared ratatui UI, native window, input and conversation state | Each conversation tab retains its own App |
| `crates/concordia/src/harness` | Local setup, shared project connections and verification | Native consent stays provider-owned |
| `crates/concordia/assets` | Bundled artwork, icon and model catalog | Asset provenance is recorded beside the files |
| `crates/concordia/tests` | Integration, fixture and opt-in live checks | Support executables are never installed |
| `fixtures/control-plane` | Synthetic transport fixtures | No credentials or personal session data |
| `scripts` | Cargo/PowerShell verification, install and release tooling | Run only the checks applicable to the task |
| `docs` | Current product, operation and architecture references | Start with [PLAN.md](../PLAN.md) and [how it works](how-concordia-works.md) |
| `docs/quality` | Scoped audits and qualifications | Current checks and Git history pointers are in [verification](e2e-status.md) |
| `docs/plans` | Proposed work and acceptance criteria | Backlog, not shipped feature status |
| `docs/archive` | Superseded strategy, campaign and dated records | Historical context only; not current status |
| `docs/agent` | Harness process metadata | Retired task journals are preserved in Git checkpoint `6fea4a9` |
| `packaging` | Windows distribution metadata/templates | Publication remains owner-controlled |
| `skills` | Concordia-owned integration instructions | Separate from provider-native skill stores |
| `experiments/terminal` | Retained WezTerm configuration from the host experiment | Lua is embedded by the production host adapter; the obsolete pane-guessing launcher is retired |
| `vendor/crossterm` | Patched terminal dependency | Not unused just because it is vendored |
| `.github` | Repository workflows | Local checks and remote qualification are distinct |
| `.cursor` | Project rules and host workflow support | Does not add a runtime/package-manager dependency |

`target`, generated artifacts, provider homes, caches and personal recovery data are not source
ownership directories. Keep module moves and this map synchronized; do not create placeholder
folders or remove active work merely to reduce the directory count.
