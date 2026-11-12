# Concordia preview: X series draft

Prepared against build `2026-10-31t`. Draft copy and recording storyboards only; GIFs have not been recorded and nothing has been posted.

The owner's 2026-11-01 direction is to implement the improvements before the campaign. The
[implementation-gated X/LinkedIn launch plan](launch-plan.md) now sets the launch bar and lead
post strategy; this eight-part draft is follow-up material, not an authorization to announce
unfinished features or availability.

## How far from the plan

The execution surface and first memory/orchestration slices exist. The complete memory-manager vision is still incomplete; feature count would disguise the larger remaining work. There is no measured basis for a percentage or a completion date.

| Area | Implemented | Remaining acceptance |
| --- | --- | --- |
| Provider workspace | Official CLI launch paths, account/model controls, native resume, structured activity, cancellation | Live supported-version and approval interaction parity; physical-host evidence |
| Usage and continuation | Fresh local usage reads on /use and /usage, bounded configured account/model fallbacks | Broader eligibility explanations and live limit evidence; provider files may themselves lag |
| Native memory | Revision-checked recent Codex/Claude pointers and exact bounded excerpts | Whole-history indexing, Cursor exact readback, retrieval-quality evaluation |
| Relationships and goals | Source chain for handoffs; exact rules from explicitly connected file-backed Harness manifests | Related-chat discovery, richer lineage, goals/decisions workflow beyond connected manifest pins |
| Durable work | Pane-owned unfinished-request recovery, separate draft/queue, explicit workers | Broader durable worker/run ownership and uncertain-completion reconciliation |
| Capacity | One fresh-session retry for recognized terminal capacity errors | Proactive near-full rollover and verified per-provider compaction controls |
| Files | Relevant-chat suggestions, formatted Markdown/JSON, raw editing with syntax colors, save/undo/conflict checks and pane recovery | Cwd replacement and managed worktrees remain later stages |
| Public release | Local build/install and synthetic tests | Current timing/lock stress failures, live checks and the other release gates in release-blockers.md |

Practical sequence: (1) stabilize current release checks and prove the demo paths; (2) complete deeper native memory and related-chat retrieval; (3) prove proactive capacity policy and broader orchestration. Public-preview storytelling can begin before the full roadmap is done. A broad stable-release claim cannot.

Sources of product truth: [PLAN](../../PLAN.md), [native memory](../native-memory.md), [handoff target](context-handoff-plan.md), [delegation](../delegation.md), [verification](../e2e-status.md), [release gates](../release-blockers.md).

## Positioning and format

Lead with the benefit: less time rebuilding context and juggling sessions/accounts. Demonstrate concrete behavior instead of claiming better models, universal reliability, unlimited memory, zero token overhead, or superiority over untested competitors. Bookkeeping adds no model calls; handoff text and retrieved excerpts still enter the next model's context.

Use a connected eight-post series, with each post also understandable alone. Keep one action per GIF, 8-12 seconds, large terminal text, a short pause on the result, and a clean loop. A practical export target is 960px wide, 10-12 fps, under 5 MB; that is a production target, not an asserted platform limit. Check readability at phone width. Use isolated synthetic names, homes, histories and fixtures. Simulated quota/provider events must be visibly labeled. Record the actual app, not a mock UI.

Free X accounts use standard posts up to 280 characters; longer posts require a subscription. GIF attachments do not consume the text character allowance. Official references: [post types](https://help.x.com/en/using-x/types-of-posts), [photos/GIFs](https://help.x.com/en/using-x/posting-gifs-and-pictures). All copy below is ASCII and has no URL; counts include the sequence number. Recheck the composer after adding links or changing copy.

A suggested cadence is one installment per day, replying to the previous installment. Pin the introduction and put any eventual download link where the release status and limitations are visible. Do not announce public availability before publication.

## Drafts and recording beats

### 1. The problem (221 characters)

1/8 I use more than one AI coding CLI. The hard part is keeping track of the work between them. I'm building Concordia: a local Windows terminal workspace for Codex, Claude Code and Cursor. Here's what the preview can do.

**GIF:** 8-10s: show one synthetic conversation, open /use, select a different provider, return to the same Concordia workspace. Opening/selection only; this must not imply native sessions or full tool state transfer.

### 2. Accounts and usage (208 characters)

2/8 Which account can I use next? Concordia puts account selection and usage in the same workspace. /use and /usage reread provider-written usage data each time. No extra model call just to check your limits.

**GIF:** 8-10s: open /usage, close, update synthetic provider usage, reopen /use and show the changed value. Keep the account names synthetic. Provider-written usage can itself lag.

### 3. Quota handoff (242 characters)

3/8 A usage limit shouldn't mean retyping the task. Concordia can try your configured account/model fallbacks with the original request and a bounded handoff. Retries are limited; native sessions, tools and approvals stay with their provider.

**GIF:** 10-12s: use the fake provider's structured usage-limit fixture. Show original task, explicit limit, configured continuation, next response. Label the clip 'Simulated usage limit'. This demonstrates configured policy, not bypassing subscription limits.

### 4. Original source memory (242 characters)

4/8 A handoff needs sources. Concordia can give the next session pointers into recent Codex and Claude history, then read exact excerpts on demand. It checks the original source before reading. This is bounded retrieval, not unlimited memory.

**GIF:** 10-12s: show a handoff source ID, then chat sources/excerpt for that exact synthetic Codex/Claude source. Highlight the same original line. Label the clip 'Fixture demo'. Do not imply Cursor readback, whole-history indexing, or that the model actually requested an excerpt unless the journal proves it.

### 5. Restart recovery (215 characters)

5/8 An unfinished task, a newer draft and a queue are different things. Concordia saves them separately. After restart, unfinished work comes back paused in its tab, with an inline action to choose where to pick it up.

**GIF:** 10-12s: fail a synthetic turn, retain a newer draft, restart isolated Concordia, show the paused unfinished request tab and its inline choices. Stop before a provider dispatch. Keep draft and request separately visible.

### 6. Files beside the chat (217 characters)

6/8 Keep the file beside the conversation. Open formatted Markdown, switch to raw to edit, and save explicitly. Source code has syntax colors. Unsaved changes get a decision before closing, and conflicts retain your edits.

**GIF:** 8-12s: current-chat @ file suggestion -> split preview -> raw/formatted toggle -> in-pane search -> reload indicator. Use a small Markdown or JSON file and legible text.

### 7. Explicit workers (247 characters)

7/8 Sometimes I want a second account working on a separate task. Concordia supports explicit background workers, visible progress and result handoff. You choose the task and when to use the result. The configured limit supports up to twelve workers, with concurrency limited by available resources.

**GIF:** 10-12s: explicitly launch a fake worker task, show its progress, open its result. Stop before any orchestrator dispatch unless that is part of the recorded scenario. Label the clip 'Fixture demo'.

### 8. Honest roadmap (230 characters)

8/8 Concordia is a Windows preview. The next work is deeper history retrieval, related-chat discovery, proactive context rollover and reliability/provider validation. The goal: less time rebuilding context between coding sessions.

**GIF:** 8-10s: show a simple 'Available / Next' card: available recent Codex/Claude source readback, recovery, previews; next whole-history retrieval, related chats, proactive rollover. Clearly distinguish roadmap from demos.


