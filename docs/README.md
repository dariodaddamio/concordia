# Concordia documentation

Concordia is a local Windows application around the official Codex, Claude Code and Cursor
CLIs. The [work plan](../PLAN.md) records the active documentation rewrite and product interview.
Feature implementation is paused; purpose and scope are being settled with the owner.

## Use Concordia

- [Install and update](install.md).
- [Configuration and commands](config.md).
- [How it works](how-concordia-works.md).
- [Workspace and input](workspace-panes.md).
- [Images and diagrams](images-and-diagrams.md).
- [Verification and known limits](e2e-status.md).

## Technical references

- [Architecture](architecture.md) and [directory map](directory-map.md).
- [Provider connections](how-concordia-connects-providers.md), including [Grok Build](provider-grok-build.md).
- [Agents MCP](mcp.md) and [delegation](delegation.md).
- [Native source pointers](native-memory.md).
- [Prompt construction](how-concordia-builds-prompts.md).
- [Runtime and security](how-concordia-runtime.md).
- [Provider terms](provider-terms.md).
- [Contributing](../CONTRIBUTING.md).

These pages describe implemented behavior within their stated limits. The [paused backlog](plans/README.md)
and [context handoff design](plans/context-handoff-plan.md) describe future work, not shipped guarantees.

## Historical evidence

[Verification](e2e-status.md) owns the current build identity, checks and cleanup exceptions.
Detailed pre-rewrite journals and task records are preserved in local Git checkpoint `6fea4a9`:

```text
git show 6fea4a9:docs/quality/e2e-history.md
git show 6fea4a9:docs/agent/cleanup-review.md
```

Historical results qualify their dated source and artifacts. They do not establish later
correctness or release readiness. Remaining dated audits are being reconciled during this rewrite.
