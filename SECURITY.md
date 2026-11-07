# Security Policy

## Supported versions

Windows release binaries tagged `v0.1.x` and later on the default branch.

## Reporting a vulnerability

Email the maintainer privately (do not open a public issue with exploit details).
Include: affected version/commit, reproduction steps, and impact.

Concordia never requests your provider credentials; it launches local CLIs under
isolated homes (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, Cursor API key files).
Do not paste secrets into issues or fixtures.

## Unsigned release trust model

Concordia releases are intentionally unsigned because the project has no signing certificate.
Each release publishes SHA-256 checksums, a CycloneDX SBOM, and (for tagged public releases) a
GitHub build-provenance attestation. Verify the checksum and, when available, the provenance with
`gh attestation verify <artifact> -R OWNER/concordia`. These records establish what was built and
what dependencies were included; they do not make SmartScreen or Smart App Control treat an
unsigned binary as trusted. Windows may still show reputation warnings, especially for a new
release. Do not bypass a Defender detection; retain the hash and report it privately.
