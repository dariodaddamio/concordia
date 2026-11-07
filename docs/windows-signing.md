# Windows distribution and antivirus validation

The owner chose unsigned Windows distribution on 2026-11-04. Public installation does not
require a certificate, legal identity verification, Azure account, paid signing service,
Windows SDK or administrator rights. The release workflow no longer requires signing secrets,
OIDC access or a signing environment. Unsigned downloads can still show Windows publisher
reputation prompts; do not describe them as signed or Windows-trusted.

The release workflow builds with the lockfile, updates Defender intelligence, scans the exact
executable, runs smoke checks, and packages that same executable with license notices. SHA-256
checksums detect changed downloads; they do not independently establish publisher identity.
The installer validates bounded root-only archive entries and the executable version before
installation. The updater uses the official feed by default and verifies the executable hash.

```powershell
Update-MpSignature
./scripts/scan-release.ps1 -Exe ./target/dist/concordia.exe
```

A clean scan is evidence for that hash and intelligence version. Rehearse download, install,
startup and update from published assets before claiming the public command is available.
The repository is currently private with no published releases; source support is implemented,
but publication and a clean public-host rehearsal have not happened.

The existing `sign-release.ps1`, `verify-signature.ps1` and signing-policy fixtures are optional
legacy tooling for a future explicit signing decision. They are not prerequisites for installing,
updating or releasing the chosen unsigned build. No signing service is provisioned automatically.

## Incorrect detection review

Local recovery on 2026-11-02: after updating Defender intelligence from `1.463.84.0`
to `1.463.94.0`, a normal locked release rebuild of the existing `2026-11-02d` source
passed the exclusion-ignoring scan. Installation and `concordia --version` succeeded;
the installed copy passed the same scan. Both copies had SHA-256
`e363cc47480b386f5605906807987904aa8ac89121210032952b398d50b5b49e`.
No runtime source, compiler flags, certificate trust or antivirus settings were changed
for this recovery. This is local scan evidence, not a Microsoft false-positive determination
for the quarantined artifact, and the recovered development build is still unsigned.

For the 2026-11-02 detection, collect the exact artifact hash, build identity, detection
name `Trojan:Win32/Bearfoos.A!ml` and intelligence version `1.463.84.0`. Submit the detected
artifact through [Microsoft Security Intelligence](https://www.microsoft.com/en-us/wdsi/filesubmission)
as **Software developer**, **Microsoft Defender Antivirus**, **incorrectly detected**.
Upload of private pre-release binaries requires owner authorization. Keep the submission
identifier and final determination with the release evidence; do not claim Microsoft
cleared the artifact before receiving that result.

Do not instruct customers to add exclusions or disable antivirus. Do not rename, repack
or change code merely to escape a verdict. Resolve the detection with Microsoft and
rescan the final candidate with current intelligence before release.
