# Contributor and internal notes

This page retains repository-facing guidance that used to be mixed into the public README.
For the supported development workflow, start with [CONTRIBUTING.md](../CONTRIBUTING.md).

## Verification and release scope

The project remains a private pre-release. Source builds and local installation are supported;
public release assets must not be assumed. Automated tests use synthetic accounts and fake
providers. They establish local transport, binding, UI and recovery behavior; they do not certify
every provider version, real login flow or physical terminal host.

Track release criteria in [PLAN.md](../PLAN.md), [release blockers](release-blockers.md), and
the [evidence ledger](e2e-status.md). Query `concordia --version` for the actual installed build.

## How it works

Provider adapters spawn the official CLI with a safe argument array and an isolated per-account
home, then parse structured output into cards. Native state stays native: usage, sessions and
models are read from files the CLIs already write. One tracked child process owns a turn;
cancellation and recovery never replay uncertain tool actions.

More detail: [continuity](continuity.md), [UI architecture](ui-architecture.md),
[provider capabilities](archive/capabilities.md), and [provider terms](provider-terms.md).

## Maintainer decisions and roadmap

The detailed roadmap, completion audits, launch plan, and release decisions are internal working
documents. They are linked from [PLAN.md](../PLAN.md) and [the release blockers](release-blockers.md);
they are not promises about the current public build.
