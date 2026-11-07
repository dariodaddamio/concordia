# Provider terms and how Concordia stays inside them

> This is a good-faith engineering summary of public documents, not legal advice. Terms change;
> re-read the linked originals before relying on any statement here, and before publishing Concordia.
> Anything marked UNVERIFIED could not be read directly and is not a conclusion.

Concordia drives the official Codex, Claude Code and Cursor CLIs under **your own** logins. This page
records what each provider's public documents say about that, what Concordia does to respect it, and
what remains unresolved. All pages below were fetched on 2026-10-15 unless stated otherwise. The
account-rotation decision was re-read on 2026-11-05.

## The use case

One person who pays for several subscriptions (Codex, Claude, Cursor, and sometimes more than one
account with the same provider) and manages them in one interface instead of several. Nobody else
uses those accounts, and nothing is shared, resold or pooled across people. The analysis separates
three situations, because they are not equally clear:

| Situation | Reading |
| --- | --- |
| **A.** One interface over your separate subscriptions with *different* providers | Compatible on everything read. Each provider is used within its own terms. |
| **B.** Several of your own accounts with the *same* provider, switched by you (`/use`) | No clause on multiple accounts found in any provider's terms read. You choose the account; nothing is automated. |
| **C.** Concordia *automatically* moving to another same-provider account when a limit is hit | The only grey area: it can be read as circumventing a rate limit (OpenAI wording) or "bypassing protective measures" (Anthropic wording). Treat carefully (see below). |

## What the documents say

### Anthropic (Claude Code)

Sources:
- Claude Code legal and compliance: <https://code.claude.com/docs/en/legal-and-compliance> (fetched 2026-10-15)
- Consumer Terms: <https://www.anthropic.com/legal/consumer-terms> ("Effective October 8, 2025", fetched 2026-10-15)
- Commercial Terms and Usage Policy: not fetched, UNVERIFIED. Commercial Terms only apply to Team,
  Enterprise and API users; the Usage Policy is referenced by the legal page as governing Claude Code.

Clauses that matter:

- Unmodified binary: "The Claude Code binary must not be modified. Claude Code must be installed and
  run as published by Anthropic". Concordia only spawns the installed CLI.
- Wrapping / hosting: Anthropic "does not permit third-party developers to offer Claude.ai login into
  their own applications, or to route requests through Free, Pro, or Max plan credentials on behalf
  of their users. Moreover, developers may not collect, store, or intermediate Claude.ai credentials
  or session tokens - sign-in to a Claude account must complete through Anthropic's own flow."
- Explicit carve-out: this "does not prevent an end user from signing in to the unmodified Claude
  Code binary with their own Claude subscription, including where a platform hosts Claude Code".
- Reselling: "Customers may not pay for, resell, or intermediate Claude usage on their end users'
  behalf. Each end user must authenticate with their own ... Claude subscription plan credentials".
  Consumer Terms Section 3 also lists a ban on using the Services to "resell the Services".
- Usage level: "Advertised usage limits for Pro and Max plans assume ordinary, individual usage of
  Claude Code and the Agent SDK." OAuth "is designed to support ordinary use of Claude Code and other
  native Anthropic applications."
- Account sharing (Consumer Terms Section 2): "You may not share your Account login information,
  Anthropic API key, or Account credentials with anyone else."
- Automation (Consumer Terms Section 3): no access "through automated or non-human means, whether
  through a bot, script, or otherwise", with an exception "via an Anthropic API Key or where we
  otherwise explicitly permit it".
- Multiple accounts: no clause found. Limits: no clause on usage limits, but Section 3 forbids
  "bypassing any of our systems or protective measures".
- Enforcement: "Anthropic reserves the right to take measures to enforce these restrictions and may
  do so without prior notice."
- Naming: you may say in plain text that a product runs Claude Code; you may not use the Claude Code
  or Anthropic names or logos as part of your product name or imply endorsement.

Assessment: Situations A and B are compatible on the text read. Concordia launches the unmodified
binary, sign-in goes through Anthropic's own flow, and no credential is collected, stored or sent.
Risks: (1) the Section 3 "automated ... script" clause is broad. Concordia drives the interactive
CLI the user would otherwise run, and the legal page explicitly allows an end user signing in to the
unmodified binary where a platform hosts it, but whether a TUI that sends prompts counts is not
stated. UNVERIFIED. (2) Situation C is the feature closest to "bypassing protective measures" and
"ordinary, individual usage". (3) Context: in February and April 2026 Anthropic enforced against
third-party harnesses using subscription OAuth outside Claude Code
([coverage](https://gigazine.net/gsc_news/en/20260220-anthropic-third-party-block),
[coverage](https://alternativeto.net/news/2026/4/anthropic-ends-free-claude-access-for-third-party-tools-like-openclaw);
secondary sources). Concordia differs because it runs Claude Code itself, but it sits in the area
enforcement has targeted, so keep the design strictly as described below.

### OpenAI (Codex)

Sources:
- Codex authentication docs: <https://learn.chatgpt.com/docs/auth> (read earlier; redirect target of
  developers.openai.com/codex/auth)
- Terms of Use: <https://openai.com/policies/terms-of-use/> and
  <https://openai.com/policies/row-terms-of-use/> returned HTTP 403 to automated fetches on
  2026-10-15. The current text is **UNVERIFIED**. The Services Agreement and usage policies were not read.
- Fallback: the December 11, 2024 revision of the Terms through a
  [third-party mirror](https://open.windriver.com/info/uni-license-list/licenses/openai-tou-20241211.html)
  (a dated copy; wording may have changed since).

Clauses that matter:

- Credentials: "treat `~/.codex/auth.json` like a password: it contains access tokens. Don't commit
  it, paste it into tickets, or share it in chat."
- Automation: "Use API key authentication for programmatic Codex CLI workflows, such as CI/CD jobs."
  ChatGPT sign-in tokens are "intended for trusted scripts, schedulers, and private CI runners."
  Copying credentials is documented only "to machines you control."
- Terms (2024 copy): prohibited to "Automatically or programmatically extract data or Output";
  prohibited to "circumvent any rate limits or restrictions or bypass any protective measures or
  safety mitigations"; "You may not share your account credentials or make your account available
  to anyone else."
- Multiple accounts: not addressed in anything read. Several accounts via `CODEX_HOME`: not addressed.
- Reselling: not read, UNVERIFIED for the current Terms.

Assessment: Situations A and B look compatible. The extraction clause targets scraping; `codex exec`
and the interactive CLI are OpenAI's own interfaces and Concordia only runs them. Situation C is the
risk: "circumvent any rate limits" is close to automatic account rotation on exhaustion.
The current Terms of Use must be read by a person before publishing.

### Cursor

Source: Cursor Terms of Service, <https://cursor.com/terms-of-service> ("Last updated September 3,
2026", fetched 2026-10-15). Only the summary below was returned by the fetch tool, not the full
text; quotes are verbatim from that extraction.

- Credentials: "You are solely responsible for maintaining the confidentiality of your account and
  password." No explicit account-sharing ban found in the text returned. Staff replies on the
  [community forum](https://forum.cursor.com/t/can-multiple-people-use-one-cursor-account/158868)
  say sharing between people is against the terms and concurrent use may be restricted (secondary).
- Automation: no clause on scripted access. Section 1.5 restricts conduct such as "harvest, scrape,
  or extract data from the Service" and "probe, scan or attempt to penetrate the Service".
- Multiple accounts: no clause found. Limits: no clause on circumventing usage limits.
- Reselling: Section 1.5(iii): you may not "rent, lease, lend, or sell the Service."
- Third-party tools: Section 6, use of third-party services "is subject to the terms provided by that
  Third-Party Service." No CLI-specific clause.

Assessment: A and B look compatible; nothing found forbids driving the Cursor CLI or holding several
own accounts. UNVERIFIED: the fetch was a model-generated extraction, not the raw text, and Cursor
may have separate usage or plan policies that were not read.

## What Concordia does (and must keep doing)

| Rule | Why | Where enforced |
| --- | --- | --- |
| Run the **unmodified** provider CLIs, as published | Anthropic requires an unmodified binary | `provider/*` spawn the installed CLI |
| Sign-in is the provider's own flow, in its own window | "must complete through Anthropic's own flow" | `connect.rs` launches `codex login` / `claude login` |
| Never collect, store, copy or transmit credentials or tokens | Anthropic: no collecting/storing/intermediating; OpenAI: treat as a password | Login checks look at file existence/size only. To label accounts, Concordia reads the account's email and plan from its own local files in memory (Codex `id_token`, Claude plan); nothing is stored, logged or sent |
| One person's own accounts only; no credential sharing | Consumer terms forbid sharing or making an account available | accounts are local homes under the user's profile |
| No network access of its own except `/update` | nothing to intermediate | `release.rs` |
| Isolated homes (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`) | keeps each login separate and untouched | `connect.rs`, `provider/*` |
| Do not use provider names or logos as Concordia's own branding | Anthropic's naming rule | docs and UI wording |

## Context sharing without infringing, and with few tokens

Sharing context does not collect credentials. The current engineering data flow is:

- The compact map is built locally from Concordia conversation events. Native-source indexes
  separately store metadata, identities and byte-range hashes for user-owned Codex/Claude session
  files. Handoffs can carry these pointer IDs; exact excerpts are read on demand and may then be
  included in a user-started provider turn. Native originals are not rewritten, and credential files
  are excluded. See [native memory](native-memory.md). This updates implementation facts, not the
  dated legal assessments above.
- It is bounded (character budget), shown to the user with an estimated size, and only attached
  after an explicit switch. No model is called to summarise it.
- Provider sessions stay provider-native; Concordia binds to an explicit session id and never guesses.

## Open decision: automatic handoff on exhaustion

As of 2026-11-05, same-provider automatic handoff is disabled by default. Enabling it requires a
one-time, provider-specific notice: the provider's terms may restrict using multiple accounts to
extend limits, and the user is responsible for accepting that risk. The default path asks first
and proposes a different provider before another account of the same provider. Automatic handoffs
into any account are capped at three per local day; workers are never fanned out across accounts
to multiply throughput. Accounts must be the user's own, one per person, and must not be shared.

Situation A (move to a *different provider* you also pay for) needs no special care beyond telling
the user. Situation C (automatic rotation among *same-provider* accounts) is the one place this could
cross a line: it sits close to OpenAI's "circumvent any rate limits" and Anthropic's "bypassing any
of our systems or protective measures" and "ordinary, individual usage". Design constraints:

1. **Ask first.** On exhaustion Concordia proposes the switch and the user confirms; no silent rotation.
2. **Your own accounts only**, each signed in through its own provider flow. Prefer proposing a
   *different provider* first.
3. **No concurrency multiplication**: one active turn per conversation, no parallel fan-out
   across accounts to multiply throughput.
4. **Explicit session binding** and a bounded context pack, as above.
5. **No hiding**: Concordia does not spoof identity, rotate credentials underneath running agents, or
   evade detection.

## To do before publishing

- [ ] A person reads OpenAI's current Terms of Use and Services Agreement (403 to automated fetch).
- [ ] A person reads Cursor's full Terms of Service text, and any Cursor usage policy.
- [ ] Read Anthropic's Usage Policy and re-read Consumer Terms Section 3 on release day.
- [ ] Decide whether same-provider failover (situation C) ships at all, and in what form.
