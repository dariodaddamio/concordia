# Native source memory

Concordia keeps a bounded, local index of provider-owned session sources. It stores source identity,
byte ranges, hashes and metadata, not copied conversation bodies. Providers retain authentication,
sessions, tools and approvals.

## CLI

Use the explicit native source commands through `concordia chat`:

- `concordia chat index SESSION --account ID --provider codex|claude|cursor|grok`
- `concordia chat sources INDEX [--from-message N --limit N]`
- `concordia chat excerpt INDEX --message N [--offset N --max-bytes N]`
- `concordia chat recall TEXT [--source INDEX] [--account ID] [--workspace DIR]`

Search is bounded lexical retrieval. Exact source readback reopens and verifies the native source
before returning the requested UTF-8 range. Handoff maps retain explicit provider/account/session
identity and observed transitions; they do not infer provider ancestry.

## Pointer failures

A pointer is unavailable when the source was deleted or moved, replaced or truncated, its indexed
range was rewritten, its original header changed, the account home changed, or a record is too
large. The error reports the specific cause and includes the observed time when the native source
metadata provides one. Concordia does not substitute another chat or invent missing content.

Goals, goal/source relations, groups, ranking, evaluation corpora, manual links and `/memory` have
been removed. Objectives belong in chats, repository files or the user's own tracker. Plain search
remains reachable through `concordia chat`.

