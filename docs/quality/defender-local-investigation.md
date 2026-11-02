# Local Defender investigation, 2026-11-02

The current installed build and an independent clean build pass Defender's custom scan with
protection enabled. No runtime source changes, signing, compiler-flag changes, exclusions or
security-setting changes were needed for these results. The earlier detection cannot currently
be reproduced on these builds. This is local evidence, not a Microsoft determination about the
original detected file or a guarantee for other hosts.

## Evidence

Source: commit `f9a6c6e4f707f491d132c3079f9bafa96188c79c`, build `2026-11-02d`.
Host: Windows 11 Home, x64, build 26200. Defender platform: `4.18.26100.4`.
Antivirus, real-time protection and behavior monitoring were enabled. Inspection found no
Concordia-specific path/process exclusions, executable-extension exclusion, or configured
threat-action override. This does not certify every possible enterprise policy.

The local Defender event log records `Trojan:Win32/Bearfoos.A!ml` under intelligence
`1.463.84.0`, with engine `1.1.26100.4`. The successful scans below used intelligence
`1.463.94.0` and the same engine version.

| Artifact | SHA-256 | Result |
| --- | --- | --- |
| Earlier failing release, recorded before recovery | `C575DFCE37F617A7EBA29EEE33CA9351557CC74F6E36D81355A18801BF76A648` | Launch blocked with Windows error 225 |
| Current release and installed copy | `E363CC47480B386F5605906807987904AA8AC89121210032952B398D50B5B49E` | Explicit scan: no threats; installed terminal checks pass |
| Independent clean build | `0EE81D1B3F32882884A8E9B7E8F2390AC190C53C2A6671177D0C0EA5A308D6D0` | Explicit scan: no threats; version command succeeds |

Both successful artifacts are unsigned. Hashes differ, so these scans do not establish that
updating intelligence alone clears the exact earlier artifact. Listing quarantine through the
supported command returned access denied; no quarantine files were restored or executed.

## Method and source review

A detached clean worktree at the recorded commit was built into a separate, initially empty
target directory. All dependencies were compiled locally with the existing lockfile and cached
sources; offline mode prevented dependency downloads:

```powershell
# From target/av-investigation/source
cargo build -p concordia --bin concordia --release --locked --offline --target-dir ../build
```

Toolchain: Rust/Cargo `1.99.0`. Cargo.lock SHA-256:
`1569B901D661A62DEFF16C6C6F4758A37432A6F68008615860AE56FF4E7E597D`.
Windows-filtered dependency metadata contains 291 registry packages and the two local packages
(Concordia and its documented Crossterm patch), with no Git-source dependencies.

Scans used `MpCmdRun.exe -Scan -ScanType 3 -File <exact-artifact> -DisableRemediation`.
This scan option ignores file exclusions and reports detections without removing the file;
it does not turn off real-time protection. Output explicitly reported no threats, exit status
was zero, and the file hash was unchanged. See the
[Microsoft command-line reference](https://learn.microsoft.com/en-us/defender-endpoint/command-line-arguments-microsoft-defender-antivirus).

Targeted review covered provider process launch/cancellation, updater downloads/install paths,
and the local Crossterm patch. The provider/update paths and dependency lockfile did not change
between `bea2805` and this source revision. This review did not identify a source defect that
explains the verdict; it is not a complete security audit of every dependency.

## Runtime verification and conclusion

Six isolated terminal journeys passed against the exact installed artifact: unattended free-text
instructions while choices are visible, option selection with a retained draft, session-selected
provider handoff without changing the default, file editing during a stream, repeated compact/full
resizes, and unbracketed CRLF paste with a retained trailing newline. Providers were synthetic;
no real model turn was started. The three broader test corrections retain the intended contracts:
check the compact header's origin marker, preserve the trailing newline, and wait for asynchronous
draft persistence before asserting its saved value.

The observations are consistent with a Defender classification change after the intelligence
update and ordinary rebuild. They do not identify a particular source function as the cause.
There is no evidence here that a paid certificate or a runtime code change is needed to resolve
this local case. Keep the exact hashes and results; do not change code merely to obtain a different
scan verdict. No manual sample upload or external analysis-service submission was performed.
