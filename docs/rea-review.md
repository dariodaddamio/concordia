# REA review and remaining interface contracts

Reviewed 2026-11-04. This is evidence from the existing **2026-11-04j** release binary,
not qualification of the premium-workspace candidate being built afterward.

REA 6.8.0 was acquired with package scripts and optional dependencies disabled, into ignored
`target/rea-work`. No `rea setup`, agent registration, global package/runtime installation,
authentication changes, decompiler installation or provider/model turn was performed.
The system Node 22.20.0 is below REA's supported range. The isolated official Node 22.23.4
Windows x64 ZIP was checked against its published SHA-256:
`cbc74b5e0982c9dbb41525fd8fb5dcd32fbee237863f7829072f4639a1ce5f20`.
Only its runtime executable was needed for analysis. Acquisition files and complete results
remain ignored under `target/rea-work`.

[REA's official repository](https://github.com/morluto/rea) documents the runtime requirements
and distinguishes local analysis from optional agent setup.
[Official Node checksums](https://nodejs.org/dist/v22.23.4/SHASUMS256.txt) identify the ZIP.

## Actual read-only operations

All four commands completed against `target/release/concordia.exe`, using the isolated
Node executable to run REA's `scripts/rea.mjs` entry point:

```text
inspect-pe-resources target/release/concordia.exe --format json
inspect-pe-imports target/release/concordia.exe --format json
inspect-pe-certificates target/release/concordia.exe --format json
inspect-rust-binary target/release/concordia.exe --format json
```

The analyzed artifact is PE32+ AMD64, **22,968,320 bytes**, SHA-256
`eb45d24e0d273ebf4a183fac4bf71b496d93659829452f4858a583631be7b552`.
Its own `--version` reports `concordia 0.0.1 (2026-11-04j)`.

| Result file | Observed result | Practical implication |
| --- | --- | --- |
| `resources.json` | Three resources; one icon group references 16x16 and 32x32, 32-bit images. Both declared payload sizes match. | Larger icon variants are absent from this artifact. Add appropriately sized frames for the premium icon, then inspect the final candidate again. |
| `imports.json` | 20 import descriptors, 301 function entries, no delay imports, no reported parsing issues. Clipboard, DPI, window creation, file-drop and job-object APIs are present. | Confirms static native API linkage; it cannot prove successful Ctrl+V, DPI rendering, drag/drop or process cleanup. |
| `certificates.json` | No PE security-directory table or embedded certificates. | Consistent with the chosen unsigned delivery. This is not signature-chain verification or a catalog-signature check. |
| `rust.json` | No `.dep-v0` cargo-auditable metadata. | This workflow cannot recover a dependency inventory from the artifact. It does not establish that dependencies are absent. |

[REA's portable PE inspection contracts](https://github.com/morluto/rea/blob/main/docs/binary-diagnostics.md)
explain the static resource/certificate/import boundaries. Icon-table relationships do not
establish actual shell/window image loading; imported functions do not establish runtime reachability.
The local JSON retains exact evidence identities, byte ranges and parser limitations.

REA's native accessibility/screenshot observation targets a Mach-O application and its helper
uses macOS ScreenCaptureKit/AX. It cannot qualify Concordia's Windows window. Its source/package
contract was inspected; no unsupported UI capture was presented as successful. Our native Windows
checks remain necessary. See the
[official native observation guide](https://github.com/morluto/rea/blob/main/docs/native-investigation.md).

## Remaining roadmap: supported interfaces

### Exact 04k resource follow-up

REA's `inspect-pe-resources` was rerun on the final installed 04k candidate: **23,105,536 bytes**,
SHA-256 `51ef2fe75425084e9b47f051596ce961282dd9f70f5bc834af0c4459eb344c46`.
It reports five resources and one icon group with 16/32/48/256-pixel frames (zero dimension bytes
represent 256 in ICO), all declared payload sizes matching. The exact result remains ignored at
`target/rea-work/resources-04k.json`. Actual Windows rendering is qualified separately in
[E2E status](e2e-status.md); REA static inspection does not establish it.

The following recheck does not turn documentation or static inspection into live-provider proof.

- **Cursor context and external token usage:** the currently published ACP page documents session
  lifecycle, permissions and Cursor extensions, without a current-context/token-counter method.
  The output-format page documents result/session/request identities and timings, without usage
  counters. No newly documented source was found that closes either gap. General ACP capabilities
  alone do not prove Cursor implements an optional extension. Continue reporting unknown rather
  than deriving tokens from quotas or timing. Sources:
  [Cursor ACP](https://cursor.com/docs/cli/acp),
  [Cursor output format](https://cursor.com/docs/cli/reference/output-format).
- **Native filesystem confinement:** Windows job objects manage process groups, limits and
  termination. They do not by themselves establish a writable-path sandbox. AppContainer supplies
  file/network/credential isolation, requiring explicit grants; compatibility with the providers'
  existing login homes and helper processes would need a separate design and fixture qualification.
  This is an implementation gap, not closed by worktrees or cooperative reservations. Sources:
  [Microsoft job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
  [Microsoft AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation).
- **Numeric credits:** local `usage.rs` already accepts a numeric `balance` and displays it when
  present. Availability alone supplies no amount. No account endpoint, credential file or model
  request was accessed in this review, so no additional numeric source is claimed.
- **Live parity and memory effectiveness:** REA's static artifact analysis supplies neither model
  retention/token-saving measurements nor live approvals/PDF/accounting qualification. Those
  remain separately scoped work under the existing provider budget requirement.
- **Distribution:** unsigned delivery is the owner's selected route. A signing identity is not a
  missing product requirement. Exact final-artifact antivirus/install/update checks, publication
  decisions and public-feed rehearsal remain separate from this older-binary inspection.
