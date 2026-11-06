# Installing Concordia on Windows

Concordia is a native Rust Windows app with an optional terminal mode. Every route below ends with a `concordia` command on your
PATH. None of them needs administrator rights. Routes are listed from simplest to most manual.

The public install contract is prepared below. This repository is currently private and has no published release assets; the public command becomes available after the owner publishes the repository and first release. No publication is performed by these instructions.

## Updating from inside Concordia

Release installations check the built-in official GitHub feed, with no environment configuration or source checkout required. Developer source installations retain source rebuilds. An explicit install-channel marker prevents old checkout hints from changing a release installation back into a source build.

`/update` checks for an update and opens its changelog. The green action strip stays
visible while scrolling: **Enter installs and restarts this tab**. Concordia builds
or downloads the update, saves the draft and queued messages, and restarts on the new
build. Other tabs keep running their existing build. Esc closes the preview without
updating; while installation is running, Esc cancels it.

Release downloads are cached by SHA-256 and rechecked against fresh release checksums before
reuse. The installer and updater prune a bounded number of verified, owned obsolete artifacts,
retaining the selected release and one rollback. Active Windows executables and unknown files
are preserved. Installing identical bytes avoids another executable swap. Source installations
use Cargo's existing freshness checks to reuse build work.

While this tab's chat or background workers are active, or file edits remain unsaved, the action
strip turns yellow and says installation is blocked. Finish/cancel work and save or close edited files, then press Enter
again. Enter does not cancel a task or queue an update for later. New chat/worker sends
are paused while installation is running.

## Updating provider CLIs

Concordia's `/update` updates Concordia itself. It does not install, pin or upgrade Codex,
Claude Code or Cursor. Update each official CLI through its own supported installation method,
with active turns stopped, then restart Concordia and check `/doctor`.
Concordia reads the CLI version and probes advertised flags; help results are cached per version.
A new version gets a new capability check. This is not full event-schema or approval compatibility
certification. Breaking provider changes can require a Concordia adapter update. There is no
automatic CLI rollback or migration of authentication/history.

## 1. One-liner (recommended)

In Windows PowerShell 5.1 or PowerShell 7:

```powershell
irm https://github.com/dariodaddamio/concordia/raw/main/scripts/install.ps1 | iex
```

The script reads the newest published release, downloads the Windows zip and its `.sha256`,
verifies the SHA-256 and archive paths, checks the executable version, and runs `concordia.exe install`. Concordia opens immediately; the command works in the current PowerShell session and future terminals. No Rust, SDK, administrator access, signing account or package manager is required. Provider CLIs and their accounts are configured separately.

Options (use the scriptblock form, because `iex` cannot take arguments):

```powershell
& ([scriptblock]::Create((irm https://github.com/dariodaddamio/concordia/raw/main/scripts/install.ps1))) -Version 0.1.0
```

| Option | Meaning |
| --- | --- |
| `-Version 0.1.0` | install this release instead of the newest (`v` prefix optional) |
| `-Dir C:\tools\concordia` | install into this folder; only the current session PATH is changed |
| `-NoLaunch` | install without opening Concordia (automation) |
| `-DryRun` | print what would happen; no download, no install, no network |
| `-Repo owner/name` | use a different repository (mirrors and forks) |

To see the plan first without running anything:

```powershell
& ([scriptblock]::Create((irm https://github.com/dariodaddamio/concordia/raw/main/scripts/install.ps1))) -DryRun
```

## 2. winget

```powershell
winget install OWNER.Concordia
```

Available once the manifests are accepted into the winget community repository. Until then, use
route 1 or 5. winget installs the same verified zip and links `concordia` for you.

## 3. Scoop

```powershell
scoop bucket add <bucket> https://github.com/<bucket-owner>/<bucket-repo>
scoop install concordia
```

The bucket is a repository that holds the manifest rendered by each release (see
`packaging/README.md`). Scoop checks the SHA-256 from the manifest.

## 4. cargo binstall

```powershell
cargo binstall concordia
```

Downloads the same release zip (matched by `[package.metadata.binstall]` in
`crates/concordia/Cargo.toml`). This route needs the crate to be published, which is not the case
yet, so it is not available today. Use route 1 or 5.

## 5. Release zip, then `concordia install`

1. Download `concordia-x86_64-pc-windows-msvc.zip` and `concordia-x86_64-pc-windows-msvc.zip.sha256`
   from the release page.
2. Check the hash (see [Verifying checksums](#verifying-checksums)).
3. Extract the zip to any folder, then run its exe:

```powershell
.\concordia.exe install
```

`concordia install` copies the exe to `%LOCALAPPDATA%\concordia\bin`, adds that folder to your user
PATH, and points the `current` launcher at it. Open a new terminal and run `concordia --version`.

## 6. From source

Source updates honor Cargo's selected output directory and verify the rebuilt source version.
If Cargo does not report the executable or it fails verification, the current install is retained.

Needs the Rust toolchain pinned by `rust-toolchain.toml` and the Visual Studio C++ build tools.

```powershell
git clone https://github.com/OWNER/concordia
cd concordia
.\scripts\install-dev.ps1 -Install
```

This preflights and installs missing Rust, MSVC, and Windows SDK dependencies, then builds
`concordia` in release mode and runs the same `concordia install` as route 5. Use
`\.\scripts\install-dev.ps1 -CheckOnly` to inspect prerequisites without building. If the linker
reports `dbghelp.lib` missing, the Windows SDK is not installed or is still installing.

## What `concordia install` changes

- Copies `concordia.exe` into `%LOCALAPPDATA%\concordia\bin` (a previous copy is swapped out, and a
  copy that is still running is kept until it exits).
- Adds that folder to the **user** PATH (`HKCU\Environment`) if it is not already there. The machine
  PATH and other users are not touched, and no admin rights are used.
- If you used the pre-rename `orc` build, it removes that folder from your user PATH. Its files are
  left in place for you to delete.
- If an older `orc` state folder exists, it copies your settings and registered accounts over to
  the Concordia state folder (it does not delete the old one).

With `-Dir`, only the copy happens: PATH is not changed.

## Uninstall

1. Remove the program: `scoop uninstall concordia`, `winget uninstall OWNER.Concordia`, or delete
   the folder you extracted or passed to `-Dir`.
2. Remove the install folder: `Remove-Item -Recurse "$env:LOCALAPPDATA\concordia\bin"`.
3. Remove the PATH entry: open *Edit environment variables for your account*, select `Path`, and
   delete the `...\concordia\bin` line. Then open a new terminal.
4. Optional, your data: Concordia's settings and state live under `%LOCALAPPDATA%\concordia`, outside
   the `bin` folder. Delete that folder only if you do not want to keep them.

## Verifying checksums

Each release has `concordia-x86_64-pc-windows-msvc.zip.sha256` and a `SHA256SUMS` file. Compare the
hash of the file you downloaded with the published value:

```powershell
$expected = ((Get-Content concordia-x86_64-pc-windows-msvc.zip.sha256 -Raw).Trim() -split '\s+')[0]
$actual = (certutil -hashfile concordia-x86_64-pc-windows-msvc.zip SHA256 | Select-Object -Index 1).Replace(' ', '')
if ($actual -eq $expected) { 'OK' } else { 'MISMATCH: do not run this file' }
```

The one-liner does this check for you and stops on a mismatch. The checksum comes from the same
release as the file, so it catches corrupted or truncated downloads. It does not prove who published
the release; TLS to GitHub does that.

## Troubleshooting

- **`concordia` is not found after install.** Open a new terminal (PATH changes do not reach
  existing windows). Check with `$env:Path -split ';' | Select-String concordia`. If you used
  `-Dir`, add that folder to PATH yourself.
- **`concordia` runs an old build.** `concordia --version` shows the build stamp and the exe path.
  Run the installer again, or `concordia install` from the new exe. Panes that were already open keep
  the old build until they are restarted.
- **`checksum mismatch` from the one-liner.** Nothing was installed. Retry once; if it repeats,
  the download is being altered (proxy, antivirus) or the release is bad. Report it rather than
  bypassing the check.
- **`release vX has no concordia-x86_64-pc-windows-msvc.zip`.** That release predates the zip, or its
  upload is still running. Use `-Version` with an earlier release, or wait and retry.
- **`no published release found`, or a 403 from the API.** GitHub rate-limits anonymous requests
  (60 per hour per IP). Wait, or use route 5 with the release page.
- **Execution policy errors.** The one-liner runs in memory and needs no policy change. For a saved
  script, use `powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1`.
- **Antivirus blocks `concordia.exe`.** Stop installation and report the release version,
  detection name and SHA-256 to the maintainer. Keep protection enabled. A detected artifact
  blocks release until the detection is resolved; see [Windows signing](windows-signing.md).
- **Corporate proxy.** Windows PowerShell uses the system proxy for `Invoke-WebRequest`. If GitHub is
  blocked, use the release zip from a machine that can reach it.
