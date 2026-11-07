# Packaging (maintainers)

Templates for the package managers. Nothing here is published by hand: the release workflow
(`.github/workflows/release.yml`) renders them with the real version and hash and attaches the
results to the GitHub release as `concordia.json` (scoop) and the three `*.yaml` winget manifests.

## Placeholders

| Token | Meaning | Filled by |
| --- | --- | --- |
| `OWNER/concordia` | repository | rendered from `GITHUB_REPOSITORY` |
| `OWNER` | GitHub owner; also the winget publisher and package-identifier prefix | rendered |
| `REPLACE_VERSION` | crate version (`crates/concordia/Cargo.toml`) | rendered |
| `REPLACE_SHA256` | SHA-256 of `concordia-x86_64-pc-windows-msvc.zip` | rendered from the built zip |

The templates are still valid JSON/YAML with the tokens in place, so editors and linters work. The
workflow fails if a token is left unrendered or the scoop JSON does not parse.

## Filling in by hand (for a bucket or winget-pkgs submission)

1. Pick the release tag `vX.Y.Z` and download `concordia-x86_64-pc-windows-msvc.zip.sha256` from
   that release. Its first field is the hash. Do not trust a hash copied from anywhere else.
2. Scoop: copy the rendered `concordia.json` into the bucket's `bucket/` folder (or fill the
   template: `REPLACE_VERSION`, `REPLACE_SHA256`, `OWNER`). Check it with
   `scoop install <bucket>/concordia` on a test machine, and `scoop checkver concordia`.
3. Winget: the three rendered files go under `manifests/<first letter of owner>/<Owner>/Concordia/<version>/`
   in a fork of `microsoft/winget-pkgs`, then open a pull request. Validate first with
   `winget validate --manifest <folder>`. `wingetcreate` can also generate and submit the set;
   check its current help for the exact flags before running it.

The zip layout is fixed: `concordia.exe`, `LICENSE`, `THIRD-PARTY-LICENSES.txt` and `bom.json` at
its root. The scoop `bin` entry and the winget `NestedInstallerFiles` entry both depend on
`concordia.exe` being at the root, so do not change the zip layout without updating these templates
and the release workflow.

## Not in this folder

- `scripts/install.ps1` is the one-liner installer. It has its own placeholder (`$Repo`).
- Cargo package installation is intentionally not offered: the crate is `publish = false`, and
  the supported distribution is the hash-checked Windows release zip.
