# Release binary size

Measured on 2026-11-06 in this Windows worktree with `CARGO_BUILD_JOBS=4`, `NO_COLOR` unset,
and the existing native Mermaid feature set. Both builds use the same application features.

| Build | Bytes | MiB | Build time |
| --- | ---: | ---: | ---: |
| Release (`cargo build -p concordia --release`): symbols stripped | 27,439,616 | 26.168 | 120.9 s |
| Dist (`cargo build -p concordia --profile dist`): thin LTO, one codegen unit | 24,057,344 | 22.943 | 913.7 s |

The release profile keeps only `strip = "symbols"` (and `panic = "unwind"`) so installs and
`/update` remain fast. The `dist` profile inherits release and adds thin LTO plus one codegen unit;
it is used only for published artifacts. The release budget is **28,811,597 bytes (27.48 MiB)**
(27,439,616 plus 5%, rounded up). The dist budget is **25,260,212 bytes (24.09 MiB)**
(24,057,344 plus 5%, rounded up). The release workflow passes the dist budget to `smoke-ci.ps1`.

Mermaid remains in-process. It is a core feature, and this change does not move it to a sidecar or
drop any Mermaid-related dependency/features. A sidecar would reduce the main executable but would
add a second shipped executable and a new crash/update boundary; that trade is deferred to an
owner decision.

The release workflow uses `cargo auditable` for the dist build, produces a CycloneDX 1.5 SBOM from
locked `cargo metadata`, and requests GitHub build-provenance attestation for tagged release
executables.
