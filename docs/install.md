# Installing Concordia on Windows

Concordia is a Windows terminal app. Every route below ends with a `concordia` command on your
PATH. None of them needs administrator rights. Routes are listed from simplest to most manual.

Replace `OWNER` with the GitHub owner of the release you install from.

## 1. One-liner (recommended)

In Windows PowerShell 5.1 or PowerShell 7:

```powershell
irm https://github.com/OWNER/concordia/raw/main/scripts/install.ps1 | iex
```

The script reads the newest published release, downloads the Windows zip and its `.sha256`,
verifies the SHA-256 and runs `concordia.exe install`. Open a new terminal afterwards and run
`concordia --version`.

Options (use the scriptblock form, because `iex` cannot take arguments):

```powershell
& ([scriptblock]::Create((irm https://github.com/OWNER/concordia/raw/main/scripts/install.ps1))) -Version 0.1.0
```

| Option | Meaning |
| --- | --- |
| `-Version 0.1.0` | install this release instead of the newest (`v` prefix optional) |
| `-Dir C:\tools\concordia` | install into this folder; your PATH is then not changed |
| `-DryRun` | print what would happen; no download, no install, no network |
| `-Repo owner/name` | use a different repository (mirrors and forks) |

To see the plan first without running anything:

```powershell
& ([scriptblock]::Create((irm https://github.com/OWNER/concordia/raw/main/scripts/install.ps1))) -DryRun
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

Needs the Rust toolchain pinned by `rust-toolchain.toml` and the Visual Studio C++ build tools.

```powershell
git clone https://github.com/OWNER/concordia
cd concordia
.\scripts\install-dev.ps1
```

This builds `concordia` in release mode and runs the same `concordia install` as route 5.

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
- **Antivirus blocks `concordia.exe`.** The exe is unsigned for now. Add an exception for
  `%LOCALAPPDATA%\concordia\bin`, or install from source.
- **Corporate proxy.** Windows PowerShell uses the system proxy for `Invoke-WebRequest`. If GitHub is
  blocked, use the release zip from a machine that can reach it.
