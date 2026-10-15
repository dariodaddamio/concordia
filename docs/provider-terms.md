# Provider terms and how Concordia stays inside them

> This is a good-faith engineering summary of public documents, not legal advice. Terms change;
> re-read the linked originals before relying on any statement here, and before publishing Concordia.

Concordia drives the official Codex, Claude Code and Cursor CLIs under **your own** logins. This page
records what each provider's public documents say about that, what Concordia does to respect it, and
what remains unresolved. Retrieved 2026-10-15.

## The use case

One person who pays for several subscriptions (Codex, Claude, Cursor, and sometimes more than one
account with the same provider) and manages them in one interface instead of several. Nobody else
uses those accounts, and nothing is shared, resold or pooled across people. The analysis below
separates three situations, because they are not equally clear:

| Situation | Reading |
| --- | --- |
| **A.** One interface over your separate subscriptions with *different* providers | Clearly fine. Each provider is used within its own terms; nothing of one provider's limits is bypassed by using another product you pay for. |
| **B.** Several of your own accounts with the *same* provider, switched by you (`/use`) | Not addressed by the documents found. You choose the account; nothing is automated. |
| **C.** Concordia *automatically* moving to another same-provider account when a limit is hit | The only grey area: it can be read as circumventing a rate limit. This is the feature to treat carefully (see below). |

## What the documents say

### Anthropic (Claude Code) - primary sources read

From the [Claude Code legal and compliance page](https://code.claude.com/docs/en/legal-and-compliance):

- Advertised Pro and Max limits "assume ordinary, individual usage of Claude Code and the Agent SDK."
- OAuth sign-in "is intended exclusively for purchasers of Claude Free, Pro, Max, Team, and
  Enterprise subscription plans and is designed to support ordinary use of Claude Code and other
  native Anthropic applications."
- Anthropic "does not permit third-party developers to offer Claude.ai login into their own
  applications, or to route requests through Free, Pro, or Max plan credentials on behalf of their
  users. Moreover, developers may not collect, store, or intermediate Claude.ai credentials or
  session tokens - sign-in to a Claude account must complete through Anthropic's own flow."
- It does **not** prevent "an end user from signing in to the unmodified Claude Code binary with
  their own Claude subscription, including where a platform hosts Claude Code."
- Claude Code "must be installed and run as published by Anthropic" (unmodified).

From the [Consumer Terms](https://www.anthropic.com/legal/consumer-terms):

- Section 3: no access to the Services "through automated or non-human means, whether through a
  bot, script, or otherwise" - "except when you are accessing our Services via an Anthropic API
  Key or where we otherwise explicitly permit it."
- Section 2: "You may not share your Account login information, Anthropic API key, or Account
  credentials with anyone else," nor "make your Account available to anyone else."
- No clause was found on multiple accounts or on usage limits specifically; Section 3 forbids
  "bypassing any of our systems or protective measures."

Context: in February and April 2026 Anthropic clarified and then enforced that subscription OAuth
credentials may not be used by third-party harnesses outside Claude Code itself
([coverage](https://gigazine.net/gsc_news/en/20260220-anthropic-third-party-block),
[coverage](https://alternativeto.net/news/2026/4/anthropic-ends-free-claude-access-for-third-party-tools-like-openclaw)).
These are secondary sources; they matter because they show the direction of enforcement.

### OpenAI (Codex) - partial

From the [Codex authentication docs](https://learn.chatgpt.com/docs/auth):

- Cached credentials live in `auth.json` under `CODEX_HOME`; "treat `~/.codex/auth.json` like a
  password: it contains access tokens. Don't commit it, paste it into tickets, or share it in chat."
- "Use API key authentication for programmatic Codex CLI workflows, such as CI/CD jobs." ChatGPT
  sign-in tokens are "intended for trusted scripts, schedulers, and private CI runners."
- Copying credentials is documented only "to machines you control."
- Running several accounts side by side via `CODEX_HOME` is not addressed either way.

OpenAI's own Terms of Use page returns HTTP 403 to automated fetches, and the web-archive copy is
also blocked, so the current page could not be read directly. The December 11, 2024 revision of
the terms was read through a [third-party mirror](https://open.windriver.com/info/uni-license-list/licenses/openai-tou-20241211.html)
(a dated copy; wording may have changed since):

- "Automatically or programmatically extract data or Output" is prohibited. This targets scraping
  the service; `codex exec` is OpenAI's own sanctioned programmatic interface, which Concordia only runs.
- Prohibited: attempts to "circumvent any rate limits or restrictions or bypass any protective
  measures or safety mitigations." (Relevant to situation C above.)
- "You may not share your account credentials or make your account available to anyone else."
  (Not what Concordia does: accounts stay yours, on your machine.)
- No clause on multiple accounts in that revision.

**Still needs a manual read of the current page** before publishing.

### Cursor - secondary only

Cursor's own terms were not retrieved. Staff replies on the
[community forum](https://forum.cursor.com/t/can-multiple-people-use-one-cursor-account/158868)
say sharing an account between people is against the terms and that concurrent use may be
restricted. Nothing found addresses CLI automation or one person holding several accounts.
**Needs a manual read of Cursor's terms.**

## What Concordia does (and must keep doing)

| Rule | Why | Where enforced |
| --- | --- | --- |
| Run the **unmodified** provider CLIs, as published | Anthropic requires an unmodified binary | `provider/*` spawn the installed CLI |
| Sign-in is the provider's own flow, in its own window | "must complete through Anthropic's own flow" | `connect.rs` launches `codex login` / `claude login` |
| Never collect, store, copy or transmit credentials or tokens | Anthropic: no collecting/storing/intermediating; OpenAI: treat as a password | Login checks look at file existence/size only. To label accounts, Concordia reads the account's email and plan from its own local files in memory (Codex `id_token`, Claude plan); nothing is stored, logged or sent |
| One person's own accounts only; no credential sharing | Consumer terms forbid sharing or making an account available | accounts are local homes under the user's profile |
| No network access of its own except `/update` | nothing to intermediate | `release.rs` |
| Isolated homes (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`) | keeps each login separate and untouched | `connect.rs`, `provider/*` |

## Context sharing without infringing, and with few tokens

Sharing context between providers needs no credentials and no provider data:

- The context pack is built **locally from Concordia's own recorded conversation**, never from
  provider-internal files, and is only sent to the provider the user switches to.
- It is bounded (character budget), shown to the user with an estimated size, and only attached
  after an explicit switch. No model is called to summarise it (no extra tokens, no extra data
  leaving the machine).
- Provider sessions stay provider-native; Concordia binds to an explicit session id and never guesses.

## Open decision: automatic handoff on exhaustion

Situation A (move to a *different provider* you also pay for) needs no special care beyond
telling the user. Situation C (automatic rotation among *same-provider* accounts) is the one
place this could cross a line: it sits close to OpenAI's "circumvent any rate limits" and
Anthropic's "bypassing any of our systems or protective measures" and "ordinary, individual
usage." Until the originals are read and a decision is made, the design constraints are:

1. **Ask first.** On exhaustion Concordia proposes the switch and the user confirms; no silent rotation.
2. **Your own accounts only** (already the case: nothing is shared with anyone), each signed in
   through its own provider flow. Prefer proposing a *different provider* first.
3. **No concurrency multiplication**: one active turn per conversation, no parallel fan-out
   across accounts to multiply throughput.
4. **Explicit session binding** and a bounded context pack, as above.
5. **No hiding**: Concordia does not spoof identity, rotate credentials underneath running agents, or
   evade detection.

## To do before publishing

- [ ] Read OpenAI's Terms of Use and Service Terms for automation, credential sharing and limits.
- [ ] Read Cursor's terms for CLI automation and multiple accounts.
- [ ] Re-read Anthropic's Consumer Terms Section 3 as it stands on the day of release.
- [ ] Decide whether failover ships at all, and in what form (see above).
