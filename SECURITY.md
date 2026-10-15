# Security Policy

## Supported versions

Windows release binaries tagged `v0.1.x` and later on the default branch.

## Reporting a vulnerability

Email the maintainer privately (do not open a public issue with exploit details).
Include: affected version/commit, reproduction steps, and impact.

Concordia never requests your provider credentials; it launches local CLIs under
isolated homes (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, Cursor API key files).
Do not paste secrets into issues or fixtures.
