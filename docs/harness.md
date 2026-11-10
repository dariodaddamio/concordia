# Harness adapters

Concordia can connect any user-owned harness to its isolated Codex, Claude, or Cursor homes.

```json
{
  "name": "my-harness",
  "version": "1.0.0",
  "skill": "skills/my-harness",
  "providers": {
    "claude": {
      "connect": ["./bin/harness", "connect", "{provider}", "{home}", "{project}"],
      "verify": ["./bin/harness", "verify", "{provider}", "{home}", "{project}"],
      "env": ["HARNESS_ENDPOINT"]
    }
  },
  "tools": [
    {"name": "knowledge", "description": "Query the local knowledge base", "argv": ["./bin/harness", "knowledge", "{project}"]}
  ]
}
```

Run `/harness connect <folder|harness.json|program>`, `/harness status`, `/harness verify [provider]`,
`/harness tools`, `/harness tools knowledge`, or `/harness disconnect <name>`. Programs receive argv
directly, with no shell, a 60-second timeout, bounded output, and a minimal environment. PATH and
standard platform directory/locale variables are passed; `env` optionally names additional variables
to pass through for that provider's commands. Relative programs resolve next to
the manifest. A bare program is asked for JSON with `concordia-manifest`.

`verify` must exit zero and print a final JSON object with `result: "denied-live-probe"`, the exact
provider and isolated home, and the current SHA-256 policy revision. Other results remain
unverified and do not write a receipt. Connecting never copies authentication files; the optional
`skill` is metadata and `/harness skill` installs Concordia's connection skill across registered
provider homes. Use `--current` to restrict setup to the selected account.

`/skills` and `/hooks` inspect the selected account's native sources. Their `connect PATH` action
uses `/harness connect PATH`, which defaults to all registered accounts, including logged-out
and hidden homes. Supported providers receive separate connection results. Shared project intent
and receipts live in Concordia's existing connection store. Later accounts connect before their
first turn in that project. Source revision changes require explicit reconnect; failed adapter
effects are not automatically retried. Native hook trust and live enforcement remain separate.
The version 2 setup store reads existing version 1 records; older binaries reject version 2
instead of overwriting the new shared-source and future-account settings.

`/skills install [PATH]` links a skill folder or collection into the provider homes. The default
source is `~/.agents/skills`. Shared sources apply across projects and reach later accounts before
their first turn. Directory links retain one current source; same-named existing skills are kept.
`/skills remove PATH` stops propagation and removes matching links across registered homes,
preserving source files and unrelated skill directories. Missing sources can still be unshared.
`/hooks status` reports shared sources and per-account setup; `/hooks verify` runs adapter probes.

## Exact pinned decisions (2026-10-31s)

File-backed manifests may declare `"pinned_decisions": ["Keep this rule verbatim."]`. Entries are
limited to 16 nonempty strings / 4096 UTF-8 bytes total. Explicitly reconnect the manifest to bind
its current SHA-256. Matching provider/home/project new-session launches, including workers,
receive the rules verbatim; native resumes retain their existing context. Changed pinned manifests
must be reconnected before a new session starts. Command-backed manifest pins are not supported.
See [native memory](native-memory.md) for the complete delivery and failure contract.
