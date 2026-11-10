# Context handoff plan

Status, 2026-10-31: memory management and orchestration are the product's core scope, reaffirmed
by the owner. This document describes the target native-source memory manager and its acceptance
criteria. Bounded maps, explicit native resume, optional thread logs, token statistics and quota
handoff are already implemented. Build `2026-10-31s` adds recent Codex/Claude source pointers,
exact chunk readback, unfinished-request recovery, file-backed Harness pins and bounded recovery
from explicit capacity errors; see [the shipped slice and limits](../native-memory.md).
Build `2026-11-02i` adds bounded whole-history pages, lexical retrieval/explicit lineage, supported
Cursor ACP replay, optional Codex proactive rollover, documented native compaction controls and
durable worker recovery/quota failover. [Current bounds and evidence](../quality/memory-orchestration-progress.md)
distinguish fixture proof from live-provider parity. The rest of this document remains target
design; historical research is not proof of unsupported provider capabilities.

## The idea

Concordia wraps the provider CLIs, so it can own what happens when a chat fills up. The
providers can compact history themselves. The target design preserves exact source readback
across sessions and gives the user a verified compaction policy:

1. Turn the providers' own compaction off for the CLIs that Concordia launches.
2. When a chat's context is nearly full, Concordia starts a new chat at once.
3. The new chat starts with the thread map Concordia already builds, and a pointer to the
   full log of the previous chat. The model reads what it needs from that log, or from other
   related logs, instead of working from a summary.
4. Chats are linked and grouped by what they are about, so "the previous related chat" can be
   found without the user naming it.

Provider-owned session files remain the source of conversation text; Concordia owns pointers,
lineage, retrieval budgets and the map.

## Authorized scope

On 2026-10-31 the owner explicitly reaffirmed memory management and orchestration as Concordia's
main purpose. `AGENTS.md`, `PLAN.md` and the README now reflect that scope. Provider-native tool
execution, authentication and approvals remain authoritative. Memory work may index session
sources and coordinate handoff, with bounded reads and explicit identities. It must not introduce
an external memory service, summarizer calls or silent changes to compaction policy.

The pointer-only design remains a target invariant across all storage. Build 2026-11-04r uses
verified native references for new thread bodies and persistent prompt recall, and for completed,
collected worker inputs when exact binding succeeds. Legacy logs and pending/uncertain recovery
remain preserved. Source-size/format limits can leave protected recovery payloads; see
[the implemented boundary](../native-memory.md). This plan does not authorize deleting or migrating
existing logs, drafts or queued work automatically.
Build 2026-11-05b implements streaming larger captures and explicit, source-bound legacy migration
with original-byte backups. Intentional legacy/pending retention and declared finite limits remain;
the identified capture/migration implementation gaps are closed within that supported contract.

## Decisions so far (owner, 2026-10-30)

- Concordia owns context and memory management on purpose. The scope is authorized; each feature
  must still meet its source, isolation and recovery acceptance criteria before it is described as built.
- Concordia keeps **no copy of any conversation**. It is a memory manager: an index of
  pointers into each provider's own session files, and the map. The provider files stay the
  only record of what was said.
- Secrets: Concordia ignores `.env`-style files and other well-known secret files by
  default, and the ignore list is editable in the config screen. Beyond that, what a person
  pastes into a chat is that person's responsibility, and the README says so plainly.
- No hosted memory service. Retrieved excerpts included in a model turn go to the selected
  provider through its official CLI; local retrieval itself adds no inference call.
- No model, no embeddings, no summarization. Supermemory was considered and rejected: it
  extracts "memories" with a model and its MCP server is hosted.
- Written in Rust, inside the existing crate.

## Where each part lives

Concordia already has a harness adapter boundary (`docs/harness.md`): any user-owned harness
connects through a manifest, with no shell, and can offer tools. That boundary is the line.

| Concordia (Rust, the runtime) | Harness (the project's knowledge and policy) |
| --- | --- |
| Provider processes, accounts, usage | Rules, preferences, skills |
| The memory manager: pointer index, groups, time, handoff, compaction control | Checks and gates, plans, docs sync |
| The UI | Evidence notes |

- The memory manager lives in Concordia, because it must own the provider processes and read
  the providers' session files. Harness is per project and TypeScript.
- Harness supplies the **pinned decisions**: the short set of always-true rules. Concordia
  delivers them word for word to every provider in every new session. That is how rules like
  "no co-author lines" reach Codex and Cursor, not only Claude.
- `lock` stays its own small binary and repository. Concordia installs its hooks into every
  account home it creates, and shows locked regions in the UI. Harness distributes lock's
  configuration downstream. It is not rewritten inside Concordia: lock is public and usable
  without Concordia, and provider hooks run it as a process either way.

## Interface rule

Every command has one stated job that a user could not reach another way. Choices are made in
the interface (pickers you click or arrow through, as `/use` and `/usage` already work), and
commands are optional shortcuts to the same screens, listed in one visible commands list.
The owner's interface principles apply to the TUI: purposeful cells, no layout shift when a picker opens,
no scroll hijacking, the mouse as a first-class input, and clear layering of a picker over the
transcript.

## Research findings (2026-10-30, read from real session files and the providers' own pages)

Field names and counts only; no message content was read.

| Question | Finding |
| --- | --- |
| Does each provider report context size? | **Codex:** yes, in its session file: `last_token_usage`, `total_token_usage`, and `model_context_window`. **Claude Code:** per assistant message `usage` with `input_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`; the window is not in the file, so it is mapped from the model name. **Cursor:** its transcript file has no usage; its streamed result event carries `inputTokens` and `cacheReadTokens`; the window of the `auto` model is unknown |
| Do providers write titles? | **Claude Code:** yes, `ai-title` entries. **Codex:** a `name` field in its payload, to be confirmed. **Cursor:** none seen. Concordia already has a free fallback, `title_from_prompt` |
| Do providers compact, and how often? | **Claude Code:** 20 compactions across 10 of 65 sessions; most automatic, near 967k tokens (a 1M window), some at 250k to 640k, one manual. **Codex:** 177 compactions across 46 of 158 sessions, up to 57 in one session; median 223k input tokens just before one, maximum 450k. **Cursor:** not found. Figures include this project's own heavy sessions |
| Do old messages stay in the file after compaction? | **Claude Code:** yes. The boundary entry is appended after thousands of earlier lines, so pointers to original messages stay valid. **Codex:** to be verified; compaction is recorded as an event with a replacement history |

## Decisions (owner, 2026-10-30, second round)

- Round 1 runs on Cursor with `auto`; the model it picked is recorded per run.
- Minimum support is Claude Code, Codex and Cursor. The MVP is Windows only.
- Compaction is a setting. Default: off, with the handoff. The other choice, provider
  compaction, still works with the memory manager, because originals stay in the file and
  pointers keep resolving; Concordia then adds pointers and time to the compacted chat.
  A compaction-plus-pointers arm is added to round 1 if the budget allows.
- Time is fuzzy: "yesterday" ranks chats by distance from a window centered on 24 hours ago
  and on yesterday's calendar day, instead of cutting at a boundary.
- Group names: the provider's own title when its file has one, else Concordia's own
  first-prompt title. No model, no cost.
- Provider terms: the Concordia terms note says the context pack never uses provider files.
  Pointers into the user's own session files change that, so the note must be revised before
  this ships. The current Anthropic and Cursor clauses on scraping concern their services,
  not the user's own saved chats. OpenAI's page could not be fetched (HTTP 403), so its
  current wording is unread. To read it: open https://openai.com/policies/terms-of-use in a
  browser and search for "extract" and "Output".

## What exists today

| Piece | State |
| --- | --- |
| Thread map, deterministic, no model (`threads::build_map`) | Built, 1600-character budget |
| Thread log and `concordia thread read <id> --tail / --grep / --range` | Built, off by default (`CONCORDIA_THREADS=1` or the `thread_log` setting) |
| MCP server for chat read and search (`mcp.rs`) | Built; opt-in registration in Concordia-owned homes through `[threads] mcp` |
| Short awareness note in Concordia-owned homes | Built; controlled by `[threads] awareness`, without copying user-owned provider setup |
| Explicit session binding on resume | Built |

## Implementation milestones and observable acceptance

| Milestone | Deliverable | Acceptance |
| --- | --- | --- |
| Source identity | Provider/account/session/file/revision/range pointer records | Same session id in two homes remains distinct; paths stay within configured session sources; credentials and unrelated files are never indexed |
| Fresh readback | Bounded exact excerpt reads from native originals | Replacement, truncation, compaction, deletion and malformed input produce an explicit stale/unavailable result; no unrelated fallback or unbounded scan |
| Lineage and discovery | Parent-run links, project/file/time metadata and lexical suggestions | Explicit handoff ancestry is certain; suggested relationships remain labelled; results identify their exact source and obey count/byte limits |
| Account/model policy | Durable ordered fallback targets with model choices and eligibility reasons | Restart preserves choices; no quota probes or inferred capacity; unavailable targets explain why they were skipped; attempts remain bounded |
| Worker ownership | Durable task/run identity and workspace ownership | Cancellation and result adoption target the right run; concurrent writes require an explicit policy; incomplete tool results are reconciled before retry |
| Capacity rollover | Supported per-version context signals and visible compaction policy | Unknown capacity stays unknown; no threshold guessed from cumulative billed tokens; originals and pinned decisions remain readable across a rollover |
| OSS evidence | Current-source documentation, fixtures and release report | Public claims match implemented capabilities; security/provider/host gaps remain explicit; publishing and live model spend retain their separate authorization |

Build and prove these in order: source identity, exact readback, lineage, durable orchestration,
then capacity rollover. Synthetic histories cover collisions, stale offsets, moved/deleted files,
unknown model capacity, interruption and repeated quota failures. Live provider experiments need
an explicit usage budget; earlier proposed experiment sizes below are not spend authorization.

## Design

### 1. Compaction off, per provider

Concordia starts each CLI as a child process under an isolated account home, so it can set
these without reading the user's own settings.

| Provider | Control found in the installed binary | To verify |
| --- | --- | --- |
| Claude Code | `DISABLE_AUTO_COMPACT` and `DISABLE_COMPACT` environment variables, an `autoCompactEnabled` setting, a `PreCompact` hook | That the variable stops auto-compaction in a headless turn, and what the CLI does at the context limit with it off |
| Codex | `model_auto_compact_token_limit` and `compact_prompt` in config, a `PreCompact` hook | The limit's unit and whether a very high value disables it |
| Cursor | None found | Ask the CLI's own help; if there is no control, Concordia hands off before the limit and Cursor never reaches it |

### 2. Knowing when a chat is nearly full

The handoff needs the context size. Each provider's stream reports token usage in different
fields. Concordia reads what is reported and never counts tokens with a model. Open question:
which fields give the current context size, not only the cost of a turn. If one provider gives
none, Concordia uses a conservative character estimate (`chars / 4`, as the pack does today)
and says so in the status line.

### 3. The handoff

- A new native session, never a resume, bound by explicit id as everywhere else.
- It receives the map and the `more:` pointer to the previous chat, plus the pointers of any
  linked chats.
- The map grows past 1600 characters. The right budget is a number to decide, with a default
  near 4000 characters, shown in the status line.
- Open question: also include the last few turns verbatim? A map names what happened, and the
  last turns keep the exact wording of the current step.
- The new chat's first prompt tells the model that it can read the previous chat in full and
  how. Until the opt-in note exists, this is the only place the model learns that.

### 4. Giving every provider the log

Register Concordia's MCP server in each account home that Concordia creates, so Codex, Claude
Code and Cursor can search and read any recorded chat. Concordia writes only into homes that it
created. It never edits a user's own `.codex` or `.claude`. Thread logging must be on by
default for this to work.

### 5. Grouping chats by content

Rules, in order of strength, with no model and no embeddings (`AGENTS.md` forbids a resident
inference model):

1. **Lineage.** A chat that was created by a handoff has an explicit parent. This is certain.
2. **Same work.** Chats that touched the same files, the same git branch, or the same commit
   ids are related. The log already records files and commands.
3. **Same words.** Chats that share rare terms (identifiers, file names, error text) are
   suggested as related, ranked by overlap. Plain term matching over the local log, computed on
   demand.

Each chat gets a group id from rules 1 and 2, which are certain. Rule 3 only suggests. The
user can confirm or detach a suggestion. `/threads` lists chats by group. A handoff lists the
group's other chats in the pointer, newest first, so the model can search all of them.

## Related evidence

One note applies: "Improving agent harness token efficiency" (Eric Zakariasson). Its relevant
points are to keep mostly static context up front and make everything else discoverable when
needed, to measure cost per completed task rather than per request, and to treat history and
summaries as named cost sources. That fits this design: a short map and a search pointer
replace a long summary, and the measurement below checks it. Nothing found covers grouping
chats or handoff between sessions.

## Method: hypotheses before code

Two kinds of claim need two kinds of evidence.

- **Correctness of a data structure** (a pointer resolves to the exact bytes it was made from
  or is reported stale; the offset index never overlaps; groups form a partition). These are
  proved or checked by property tests, in the spirit of Proof Driven Development (GitHub
  Next's Verus-based PDD Kit), not covered by piles of unit tests. Published evaluations also
  report that agent-written unit tests do not raise task success.
- **Effectiveness** (speed, tokens, accuracy). These are measured in an experiment that is
  written down before it runs.

Draft experiment, to be agreed before any real provider turn:

| Arm | Strategy after the chat is long |
| --- | --- |
| H0 | The provider's own compaction (baseline) |
| H1 | Compaction off, one long chat |
| H2 | Fresh session with the map only |
| H3 | H2 plus the user's pinned decisions, word for word |
| H4 | H3 plus the last K exchanges, word for word |
| H5 | H4 plus snippets injected at session start, before an edit, and after a failure |
| H6 | H3 plus pointers only, with the model searching through MCP |

Metrics: task success against a fixed rubric; price-weighted tokens (output over uncached
input over cached input); turns; wall time; whether the needed turn was retrieved (precision
and recall at k); stale-pointer rate. Each arm runs the same recorded tasks several times,
because model output varies, and results are reported with intervals, not single runs.
Hypotheses and pass thresholds are written before the first run. Real provider turns need an
explicit scope and budget from the owner.

### Round 1: the short list (owner, 2026-10-30)

Usage is limited, so round 1 tests only the top idea, on Cursor (model: auto, and the model
that auto picked is recorded for every run), which has the most room left. The time feature
is not an experiment: it is required, and is checked by acceptance tests, not by a trial.

| Experiment | Arms | Size |
| --- | --- | --- |
| Handoff | A: the provider's own handling. B: fresh session, map, pinned decisions, last K exchanges verbatim. C: B plus paging, meaning pointers that are paged in at session start, before an edit, and after a failure | 3 arms x 8 tasks x 3 repeats = 72 runs |

Paging is the operating-system idea: the context is memory and the logs are disk. The risk
The owner named is a miss. There are two kinds, and both are measured:

- A **page fault**: the model needs content that is not in its context. The cost is an extra
  turn and extra tokens. Metric: faults per task, and how many cost a turn.
- A **prompt-cache miss**: the provider's cached prefix is not reused, so the input is billed
  at the uncached price. Metric: cached versus uncached input tokens per run.

Mitigation to test in arm C: prefetch by group, so pages that the group's chats and files
point to are loaded before the model asks, and a small pinned set that never leaves.

Everything else (the remaining arms above, and the data structures) waits for round 1.

### Who does the building

Coding steps that are clearly outlined go to Luna across `codex`, `codex2` and `codex3`, each
in its own worktree off `main`, one small pure function or module each (date-range parser,
byte-offset index, pointer with hash check, group merging). Runs against Cursor are driven by
Concordia's fake provider first, and real turns only within the budget the owner sets.

### Product rule: few commands

Memory works without commands. Browsing groups happens on the landing page (below), and the
ignore list and wipe live in the config screen, so memory adds no command to the 12 that the
owner chose on 2026-10-30. Concordia ships with the owner's preferences as defaults, and every one of
them can be removed or wiped from the config screen.

A custom data structure is only added when a paper or a measurement says it earns its place.

## Folder layout idea

Chats grouped in folders of manifests, never of conversations:

    memory/2026-10-29-auth-rewrite/
      group.toml     one-line name, first and last active date, branch, files
      chats.jsonl    one pointer per chat: provider, session id, file, byte range, hash, times

The folder is a view that can be rebuilt from the providers' files. Open question: who writes
the one-line name when no model is allowed (first line of the first prompt, or the user).

## Time as a first-class key

Concordia can add a short timestamp line to each prompt it sends, and can turn "yesterday"
or "last week" into a date range itself, so the search covers that day's chats and the ones
around it. Open question: the cost in tokens per prompt, and whether it leaves the cached
prefix intact, to be measured in the experiment above.

The prompt side is built in `2026-10-31e` (`prompt_time.rs`): a stamp line, then one line per
relative time, before the context pack and the prompt. The fuzzy ranking of chats by time
belongs to the search and is not built.

## Landing page (built in `2026-10-31a`)

Opening Concordia, or starting a chat with `/new`, shows a landing page. The historical sketch
below is superseded where current UI behavior differs: drafting keeps the lists visible until
a message is sent, and pressing Esc can close them explicitly.

    ┌ chats ──────────────────────────────┐┌ goals ──────────────────────────┐
    │> ▾ concordia (2)              today││  ○ ship the 1.0                │
    │    everyday chat fixes           2h││  ○ Concordia usable daily      │
    │    context handoff plan          9h││                                │
    │  ▸ lock (6)               yesterday││open to-dos                     │
    │  ▸ design (3)                Oct 29││concordia · 2                   │
    │                                    ││  ○ token statistics            │
    │                                    ││  ○ handoff experiment          │
    └────────────────────────────────────┘└────────────────────────────────┘
     Enter open chat · Tab goals · Esc chat

- **Chats.** Grouped as in "Grouping chats by content". Until grouping exists, the group is
  the project folder. Each row shows the title (the provider's own, else Concordia's
  first-prompt title) and how long ago it was active. Enter resumes that chat by its
  explicit session id.
- **Open to-dos.** Each project's open rows, read from its Harness plan ledger, read-only.
  Plans belong to Harness (`docs/architecture-model.md` in Harness), so Concordia shows them
  and does not keep a second copy. A project without Harness shows none.
- **Goals.** A short list of cross-project goals that Concordia keeps, because it spans
  projects and chats. The person adds and closes them on this page; agents can add or close them
  through Concordia's MCP tool, and each change shows who made it.
- Clickable and keyboard-driven, following the principles in "Interface rule".

Needs from Harness: open plan rows are printed only as text today, by a repository script
(`check-active-plan.mjs --open`). Harness adds `harness plan open --json`, and Concordia reads it
through the adapter, as it reads `version --json` and `tools --json`. Open: the limit on goals.

## How to know it works

- A fake-provider test that fills a chat past the threshold and checks that the new chat is
  created, bound by explicit id, and carries the map and pointer, with no summary text.
- A test that a handoff from one provider to another keeps the same group id and a working
  read pointer.
- A per-task measurement of tokens and turns with the handoff versus the provider's own
  compaction, on the same recorded task, so the claim that it saves tokens and keeps accuracy
  is measured. Never run on a real provider without an explicit scope and budget from the owner.
- A fixture where the needed detail sits only in the previous chat, and a fake-provider test
  that the new chat finds it by following the pointer.

## Token statistics (built in `2026-10-31b`)

Concordia sees every turn's usage, so it can show what was spent through it and where. No model
and no estimate when the provider reports real numbers; a reading the provider did not report
is shown as unknown.

- **Totals:** input, cached input, output and, where reported, cost, for today, the last 7 days
  and all time.
- **Breakdown:** by provider, account, model, project and chat, and inside a chat by turn.
- **Where the tokens go:** the share spent re-reading cached context versus new input versus
  output, which is what the handoff experiment needs to measure.
- **Source:** each turn's usage as it already arrives in the stream (`StreamPart::Usage`),
  appended to a local log with the chat, account and project it belongs to. Usage from chats
  run outside Concordia comes from the providers' own session files, where they record it.
- **Where it shows:** a row on the landing page (today's total) and a screen behind `/usage`,
  so it adds no command.

Open: how long the log is kept, and whether outside-Concordia chats are counted by default.

## Delegation by intent (owner, 2026-10-31)

The person should not have to type `/delegate`: saying "create subagents", or anything like it,
is enough. Concordia does not interpret the request itself (no model of its own); it offers
delegation as a tool through its MCP server, so the provider agent that already understands
the request calls `delegate` with the task and, if it wants, the worker account. Workers stay
explicit and visible in the interface, and the person can stop them. Depends on registering
the MCP server in every account home (order of work, step 4).

## Pain points, for the owner's own setup (owner, 2026-10-31)

Concordia sees every prompt across providers, so it can notice friction: a correction ("no",
"I said", "stop"), a cancelled turn, a retried prompt, a provider error, a slow turn. With a
setting that is off by default (the open-source build ships it off), it appends each signal to
a local log with the chat it came from. Two things follow:

- "Fix the pain points" works as a prompt: the agent reads the log through Concordia's tools
  and the linked chats, and proposes changes.
- "See how you did this in that chat; do it this way instead" works: the agent reads that
  chat through the same tools. Turning such a correction into a lasting rule is the
  harness's job (its recurring-correction programme); Concordia supplies the evidence.

## Concordia builds Concordia (owner, 2026-10-31)

From batch B on, changes to Concordia are made by prompting Concordia in its own repository,
with its agents doing the work and every change reviewed and tested before a pull request.
Friction met along the way goes into the pain-point log.

## Order of work

1. Everyday chat works for real (done in `2026-10-30a`), then the 12-command cut (done in
   `2026-10-30b`).
2. Landing page: chats by project, Harness to-dos, goals (done in `2026-10-31a`).
3. Token statistics, logged per turn, shown on the landing page and behind `/usage` (done in
   `2026-10-31b`; worker turns and chats run outside Concordia are not counted yet).
4. Thread log on by default, and the MCP server registered in created homes, with
   `delegate` and goals as tools; then the pain-point log.
5. The opt-in "inside Concordia" note, with timestamps on prompts.
6. Context-size reading per provider, then the compaction controls.
7. The handoff, then grouping by content, shown on the landing page.
8. A UI parity audit against Claude Code, as a separate plan.


## Research review, 2026-10-31

Reviewed accepted research notes through 2026-10-31; primary references are linked below. The implementation remains the bounded-map design described above; this review does
not enable or implement the proposed pointer-only memory manager.

- [STAIR](https://arxiv.org/html/2609.03874v1), reviewed on October 29, uses document
  structure and a table of contents for retrieval. Its retriever is a trained LLM. The useful
  design direction here is a small hierarchical index with exact source pointers and selective
  reads. Its benchmark scores do not establish performance for chat logs, and using the trained
  retriever would conflict with Concordia's no-model memory policy.
- [rederive](https://github.com/dmustapha/rederive), accepted on October 25, is a different
  application with metered services. Its change-based invalidation is worth evaluating locally:
  bind pointers to provider/session identity and source revision, then refresh changed sections
  and detect unavailable or replaced originals. This does not require adopting its service stack.
- An older knowledge-graph critique is commentary, not evidence that a graph is required.
  Start with explicit continuation lineage, project/file identity, timestamps and lexical search.
  Measure retrieval correctness before introducing a graph or changing the architecture.

The following paragraphs describe the historical research baseline, not the current feature
inventory. Native-file indexing, related-chat retrieval and capability-gated context rollover have
since shipped; build 2026-11-04r adds reference-backed recording/recall. See
[native memory](../native-memory.md) and the [current audit](../feature-completion-audit.md) for exact
bounds, preserved legacy/recovery payloads and missing effectiveness evidence. Provider-native
compaction remains active unless an explicit supported control changes it.

Usage exhaustion is a separate trigger from context capacity. Automatic quota continuation uses
an ordered account allowlist and at most three handoffs. Account switching resets the explicit
model selection and resolves the target home's native model; there is no account/model fallback
policy yet. A future policy should display each fallback's account, provider and model before use,
without transferring native approval state or treating uncertain tool results as completed.

## Compaction decision status, 2026-11-01e

There is no controlled live comparison establishing whether native compaction or fresh-session
handoff has better task completion, constraint retention, latency or reported usage in Concordia.
Native compaction defaults remain unchanged. Compaction addresses context size, not subscription
quota; an exhausted account still requires eligible capacity or its reset. Preserve pinned
constraints and exact source pointers across both paths; do not treat a compact map as the full
history. A bounded fixture proves implementation behavior, not model recall quality.

An eventual comparison should use the same synthetic repository/task, check retained constraints
and duplicate side effects, and measure completion plus reported input/cache/output usage across
native resume/compaction and a fresh handoff. Real provider trials require explicit scope/budget
under AGENTS.md. No such trial was run in this change.

Official behavior: [Claude context/compaction](https://code.claude.com/docs/en/how-claude-code-works)
and [Codex thread compaction](https://learn.chatgpt.com/docs/app-server).
