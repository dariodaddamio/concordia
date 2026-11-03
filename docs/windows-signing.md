# Windows release signing and antivirus validation

The expected publisher is configured through `CONCORDIA_RELEASE_PUBLISHER` in the protected
release-signing environment, or an explicit local `-PolicyPath`. The tracked policy intentionally
contains no personal identity. Its timestamp endpoint stays in `scripts/release-signing.json`. Every public
release must have a Windows-trusted Authenticode signature under that exact publisher,
an RFC 3161 timestamp, and a clean Defender scan. Signing establishes provenance; it
does not guarantee that antivirus software will never detect a file.

## Provision publisher signing

No signing certificate or cloud signing account is provisioned by these scripts.
Self-signed certificates and installing a new local trusted root do not establish
publisher trust for customers and are not part of this release process.

The release workflow uses Microsoft Azure Artifact Signing with GitHub OIDC. The owner
must complete Public Trust individual identity validation using the legal publisher
name in the protected publisher setting. This requires an Azure subscription, supported region,
government identity verification and a paid signing service. Account creation, billing
and identity verification are separate owner actions; no identity documents belong in
this repository. Microsoft documents the prerequisites in its
[Artifact Signing quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart).

Create a Public Trust certificate profile and grant the federated GitHub identity only
the Artifact Signing Certificate Profile Signer role scoped to that profile. Bind its
federated credential to this repository's `release-signing` GitHub environment; restrict
that environment to approved release tags and rehearsal branches. Configure environment
variables `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`,
`AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE`, and
`CONCORDIA_RELEASE_PUBLISHER`.
No client secret or exportable signing key is needed. Follow the
[official signing action](https://github.com/Azure/artifact-signing-action) for federation.

For local signing with an already-issued trusted certificate and its hardware/provider
key accessible in `Cert:\CurrentUser\My`:

```powershell
./scripts/sign-release.ps1 -Exe ./target/release/concordia.exe -Thumbprint YOUR_CERTIFICATE_THUMBPRINT
```

The script uses Windows SDK SignTool and refuses a missing, expired, untrusted or
wrong-publisher certificate. It never exports keys or adds certificates to trust stores.
For cloud signing, sign the local build with the same verified Artifact Signing profile,
or download the signed rehearsal artifact; verify it with `scripts/verify-signature.ps1`.

For local use, set the publisher environment variable for that process, or pass a private JSON
policy outside the repository with `publisher` and `timestampUrl`. Both signing and verification
accept `-PolicyPath`. These settings are identity metadata, never a private key or credential.

## Exact-artifact release checks

The workflow builds with the lockfile, signs only `concordia.exe`, checks its publisher
and timestamp, updates Defender intelligence and scans it before executing the release
smoke checks or packaging. Checksums are generated after signing. The packaged executable
must retain those exact bytes. Unsigned rehearsals also fail rather than producing an
artifact that could accidentally be presented as release-ready.

```powershell
./scripts/verify-signature.ps1 -Exe ./target/release/concordia.exe
Update-MpSignature
./scripts/scan-release.ps1 -Exe ./target/release/concordia.exe
```

The custom scan ignores exclusions, leaves real-time protection enabled, and fails on a
detection, scanner error, stale intelligence or changing/missing artifact. A clean scan
is point-in-time evidence for that hash and intelligence version, not a universal guarantee.
Rehearse download, installation, startup and update on a clean Windows host with Defender
enabled before publishing. The existing product, dependency and license gates still apply.

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
rescan the final signed candidate with current intelligence before release.
