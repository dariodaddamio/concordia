---
title: Install
description: Install and update Concordia on Windows.
---

Concordia targets Windows. The current repository has source-install support. Published release assets will provide the release installer and checksum after publication.

## Source install

Install Rust with the official Rust installer, then build from this repository:

```powershell
cargo build -p concordia --release
```

Run the resulting executable from `target/release` and configure provider CLIs through their own supported login flows.

## Release install

When a release exists, download the official zip and its `.sha256` file from the GitHub release.
Verify the archive before running it. The release also publishes `SHA256SUMS` for the executable
and zip. The release installer does not require administrator rights.

## Windows security warnings

Windows SmartScreen uses file reputation. A new unsigned executable can show a warning because Microsoft has not seen enough copies to establish reputation. That warning is not a malware verdict.

For the official release file, select **More info**, then **Run anyway** only after checking the release page, checksum and artifact name. Do not use that action for a file from an unknown source.

### Verify the file

Use the published SHA-256 checksum in PowerShell. Compare the output with the line for the same
filename in `SHA256SUMS` or its per-file `.sha256` file:

```powershell
Get-FileHash .\concordia-x86_64-pc-windows-msvc.exe -Algorithm SHA256
```

For an attested release, verify the executable or zip against the GitHub Actions build provenance:

```powershell
gh attestation verify .\concordia-x86_64-pc-windows-msvc.exe -o OWNER/REPOSITORY
gh attestation verify .\concordia-x86_64-pc-windows-msvc.zip -o OWNER/REPOSITORY
```

You can also build from source and compare the source revision with the release tag.

Smart App Control can block unsigned files without an override. Do not weaken Windows security globally to run Concordia. If Defender reports a false positive, submit the exact artifact through [Microsoft Security Intelligence](https://www.microsoft.com/en-us/wdsi/filesubmission) and keep the detection name and artifact hash.

### Signing status

The current distribution plan uses unsigned builds. The project may use SignPath Foundation later. Signing will not remove every warning immediately. Reputation can take weeks to develop, and the release process must publish its code signing policy when signing begins.

## Update

`/update` updates Concordia from its official release feed. It does not install provider CLIs or change provider authentication. Update each provider through its own supported updater, then run `/doctor`.
