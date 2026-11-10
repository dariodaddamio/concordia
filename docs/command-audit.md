# Command and state audit

The current command surface is maintained by the [source registry](../crates/concordia/src/commands.rs),
its generated [README table](../README.md), [configuration reference](config.md) and [MCP reference](mcp.md).
Those documents own current behavior; the previous duplicated command inventory is preserved in
[the historical audit](archive/command-audit.md).

The 2026-11-08 repository audit reviews parsing, dispatch, native permissions, identity, drafts,
background completion and storage boundaries. Its coverage and verification are preserved in
[the historical cleanup journal](e2e-status.md#earlier-evidence). A fixture proves its stated path, not every command,
provider version or possible state ordering.
