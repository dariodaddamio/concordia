# Source-update artifact identity

Build `2026-11-03c` fixes source updates when Cargo writes to a custom output directory.
Previously the build could succeed while Concordia selected an older executable from the default
target directory. The update now requests Cargo's structured artifact report for the Concordia
binary and matches its exact source manifest. Environment/config-selected directories and the
existing explicit side-build directory use that same artifact path. There is no guessed fallback
to an older output if the reported executable is missing.

The rebuilt executable must report the source package version and build stamp captured before
the build; changing that identity while updating refuses installation. The staged copy is checked
again before the old install is moved aside. Failed validation retains the existing executable.
Build diagnostics remain readable; raw artifact metadata is not displayed as update progress.
Existing cancellation, argument arrays and native process-tree cleanup remain authoritative.

The existing source-root fixture covers a custom artifact directory, wrong target kinds, wrong
manifests and missing Cargo evidence. The existing real source-update install fixture now requires
the selected source version, rather than accepting any Concordia binary. Source `7b97d63` passes
all 983 enabled checks through portable everyday smoke, all-target Clippy with warnings denied,
release smoke and OSS gates (329 tracked text files). The install suite passes all five checks;
the source-update case runs a real local Cargo rebuild. GitHub CI is green, including all 48
terminal journeys. Release/installed build c is 18,804,224 bytes; SHA-256
`FF76B9A4B9F6CE70058BCECD5F19D3FAE90AB00D049E33B709B24B1AD98EB2EC`.
PR 32 was merged and that exact binary installed. No provider/model turn or release publication
is part of this evidence.

Interface: [Cargo JSON artifact messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages).
