---
name: concordia-harness-connect
description: Connect a user's own harness (hooks, rules, skills, policy tooling) to the providers Concordia launches, and verify it with a real probe. Use when the user wants Concordia to work with their harness, asks how to write a harness adapter manifest, or sees a locked-code project forced into read-only mode.
---

# Connect a harness to Concordia

A **harness** is any tool the user wants wired into a provider's isolated home: hooks that block unsafe edits, rules, skills, policy checks. Concordia does not know your harness. You describe it once in a small manifest, and `/harness` runs the commands you declare.

## What you can do

| Command | Result |
| --- | --- |
| `/harness connect <folder or harness.json>` | Runs the manifest's `connect` command for the current account and project. Add `--all` to do it for every account. |
| `/harness connect <program>` | Runs `<program> concordia-manifest` and reads the manifest from its output. |
| `/harness status` | Lists each connection, account, project and whether it is verified. |
| `/harness verify [name] [provider]` | Runs the manifest's `verify` command and records the outcome. |
| `/harness disconnect <name>` | Forgets the connection and withdraws the receipts it wrote. It does not undo changes the harness made in a provider home; use the harness's own uninstall for that. |
| `/harness skill` | Installs this skill into the current account's home (one folder, no credentials). |

## Write the manifest

Create `harness.json` next to your tool:

```json
{
  "name": "my-harness",
  "version": "1.0.0",
  "skill": "skills/my-harness",
  "providers": {
    "claude": {
      "connect": ["./bin/my-harness", "connect", "{provider}", "{home}", "{project}"],
      "verify": ["./bin/my-harness", "verify", "{provider}", "{home}", "{project}"]
    },
    "codex": { "connect": ["./bin/my-harness", "connect", "{provider}", "{home}", "{project}"] }
  }
}
```

Rules Concordia enforces:

- Commands are argv arrays and run **without a shell**. `a;b` or `$(x)` stay literal text.
- Placeholders: `{provider}` (`codex`, `claude` or `cursor`), `{home}` (the account's isolated provider home), `{project}` (the project folder). Any other `{...}` is rejected.
- A command that needs `{home}` will not run for an account that has no isolated home.
- A program written with a path separator (`./bin/x`) is relative to the manifest folder; a bare name is looked up on PATH.
- Unknown fields are errors, so typos do not pass silently.
- Each command has 60 seconds and its output is capped.

## Make `connect` do the right thing

`connect` should be idempotent and touch only the given provider home: install hooks and rules there, never anything outside it, and never read or copy login files (`auth.json`, tokens, keychain entries).

## Make `verify` prove something

`verify` is how a locked-code project is allowed to leave read-only mode. It must attempt a real edit of a protected region through the provider and observe that it was blocked. When it saw the denial it exits 0 and prints one JSON object as the last line:

```json
{"result":"denied-live-probe","provider":"claude","home":"<the {home} it was given>","policyRevision":"<sha256 of docs/quality/locked-code.json, CRLF normalized>"}
```

Concordia accepts that only if the provider, home and policy revision match what it expects right now. If the harness cannot prove it, print `{"result":"not-supported","reason":"..."}` and exit 0, or exit non-zero. Concordia then records **unverified**, keeps the project read-only, and shows the reason.

## Hard rules

- Never write or edit a receipt by hand and never ask the user to. Receipts exist only because `/harness verify` saw a real denial.
- Never fake a probe, skip the probe, or report `denied-live-probe` without attempting the edit.
- Do not copy credentials between provider homes.
- If verification fails, fix the harness or tell the user why it cannot be proven. Do not work around the read-only mode.

## Step by step

1. Write `harness.json` and the connect and verify commands.
2. Run `/harness connect <folder>`. Fix any error it prints.
3. Run `/harness verify`. A project with `docs/quality/locked-code.json` stays read-only until this prints verified.
4. Run `/harness status` to confirm.
