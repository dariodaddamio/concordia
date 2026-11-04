# Dependency and terminal-output security

## Native workspace delivery, 2026-11-04k

Shared file windows, descriptive tool wrappers and expanded icon resources reuse existing Rust
and Windows dependencies. The initial optimized artifact is 23,105,536 bytes, 137,216 bytes above
04j (about 0.6%). The reviewed distribution ceiling increases from 22 to 23 MiB; exact final
bytes/hash and checks remain in [E2E status](../e2e-status.md). No dependency or advisory exception
is added. REA is an isolated development inspection tool, not a shipped runtime dependency.

## Native raster dependency review, 2026-11-03p

`image` is pinned to 0.25.10 (MIT OR Apache-2.0), with defaults disabled and only PNG, JPEG and
WebP codecs. No AVIF/TIFF/GIF parser, Rayon pool, codec executable or browser is introduced.
The lockfile records transitive versions. Upstream defines strict edge limits and a non-strict
allocation limit; Concordia additionally checks dimensions/output bytes before pixel allocation.
Decode work is serialized off the input thread; close prevents stale publication and retained
derivatives are bounded. See [image limits](../images-and-diagrams.md), the upstream
[feature graph](https://docs.rs/crate/image/0.25.10/features) and
[limits contract](https://docs.rs/image/0.25.10/image/struct.Limits.html).
`cargo deny check` passes advisory, license and source checks (existing duplicate-version
warnings remain). The unsigned release is 21,580,800 bytes, 733,696 bytes above build o.
The distribution ceiling is deliberately revised to 22 MiB; no advisory/license exception is added.
The existing store/renderer fixture, three-provider ConPTY image journey, all-target clippy and
release build pass. Evidence: ignored `target/raster-p-{proof-final,terminal-final,clippy-final,
dependencies,release}.log`. No model turn or physical-host image-protocol claim is included.

The 2026-10-31 review covers Concordia's Cargo lockfile and the terminal display path.
Concordia has no JavaScript runtime, CDN scripts or browser rendering library. Keep the
application native; do not add external JavaScript to implement terminal controls.
Avoid unnecessary dependencies. Removing a library or writing a replacement does not
by itself establish security.

Ratatui 0.29 pulled in `lru` 0.12.5, affected by RUSTSEC-2026-0002 and
RUSTSEC-2026-0253, plus unmaintained `paste` 1.0.15 (RUSTSEC-2024-0436).
Ratatui 0.30.2 resolves to `lru` 0.18.5 and removes `paste`. Unicode width is 0.2.3.
The application still uses its documented Crossterm 0.28.1 input adapter; Ratatui's
output backend uses Crossterm 0.29.0. The local input patch remains necessary for
Windows paste framing and surrogate handling; see `vendor/crossterm/CONCORDIA-PATCH.md`.

Run `cargo audit --deny warnings` against a freshly fetched RustSec database and
`cargo deny check`. Do not treat a zero vulnerability count as success if the report
contains soundness or maintenance warnings. `deny.toml` checks transitive maintenance
status and rejects yanked crates. Current audit: no vulnerabilities and no warnings,
database revision `2295466c4e9df26647fd67d8fa1c705ac2fc7f81` (2026-10-30).

Provider output is untrusted text. Transcript rendering removes CSI, OSC, DCS and
related escape sequences, expands tabs before wrapping, and maps style ranges to the
cleaned text. The final frame guard prevents raw controls in all widgets from reaching
the terminal. Stored provider records are preserved; this filtering applies to display.

This is a checked scope, not proof that every vulnerability is eliminated. Provider
CLIs, editor extensions, external MCP services and unrelated project dependencies have
separate owners and update mechanisms. See `browser-security.md` for browser scope.

## Verification for installed build 2026-10-31i

- Main suite: 733 library tests and all enabled integration tests pass; five library
  checks remain intentionally ignored (including screenshot/benchmark helpers).
- All 26 Windows ConPTY journeys pass with isolated fake providers. This includes
  draft switching, both drafts surviving restart, a real mouse click on `Bottom`,
  multiline paste, modal activity, cancellation and queue recovery.
- The new draft/button journey also passes against the installed release executable.
- The deterministic input-parser check reproduces and prevents literal `[200~` leaking
  after Esc immediately followed by bracketed paste; the affected modal journey passes.
- Formatting, clippy with warnings denied, `cargo audit --deny warnings`,
  `cargo deny check`, release smoke and installed everyday smoke pass. Cargo-deny
  retains informational duplicate-version warnings for existing dependency families.
- Updated UI screenshots were regenerated from synthetic state. No live provider turn
  was run. Physical-host rendering and external provider tooling remain separate checks.

## Native Mermaid dependency review, 2026-11-01e

The pinned native renderer is mermansi 0.1.6 (MIT OR Apache-2.0), requiring Rust 1.95; the local
compiler is 1.99. Parser/layout work is bounded and off the render thread; no runtime Node,
browser, remote rendering service or model call is introduced. Its HTML-label dependencies
cssparser, cssparser-macros, dtoa-short and selectors are unmodified MPL-2.0 crates. deny.toml
records four crate-specific exceptions following the existing scoped option-ext approach; MPL
is not added to the global license allow-list. Public distribution obligations/notices remain
part of release review. The license check passes with these explicit entries.

The advisory check still fails on pre-existing syntect dependencies bincode 1.3.3
(RUSTSEC-2025-0141) and yaml-rust 0.4.5 (RUSTSEC-2024-0320), both unmaintained advisories.
The dependency graph confirms those paths belong to syntect, which preceded this change.
No advisory is ignored and no security policy is weakened. This is not a clean advisory audit;
upgrading/replacing those syntax-highlighting dependencies remains release-gate work.
Evidence: ignored target/dependencies-2026-11-01e.log and target/licenses-2026-11-01e-final.log.

## CI dependency review, 2026-11-02j

Syntect now enables only `default-syntaxes`, `default-themes` and `regex-fancy`.
Concordia uses embedded grammars and themes; it does not import user-supplied grammar/theme
files or serialized dumps. Disabling YAML/plist/HTML import support removes yaml-rust and its
maintenance advisory, plus unused plist/XML dependencies, without changing highlighting.

Syntect 5.3.0 is still the published release and requires Bincode 1.3.3 for embedded assets.
[RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html) is an informational
maintenance advisory with no patched version. `deny.toml` records an exception for that exact
advisory, reviewed against the embedded-only use. User file text goes to the syntax highlighter,
not the dump decoder. All other maintenance, vulnerability, yanked-crate, license and source
checks remain enabled. This accepts maintenance risk; it does not fix upstream maintenance.
Remove the exception when Syntect provides a maintained serialization path. Re-review it before
any external grammar/theme/dump loading is added. See [Syntect's feature graph](https://docs.rs/crate/syntect/5.3.0/features).
