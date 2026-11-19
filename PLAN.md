# Concordia work plan

Current phase: parity and polish; completion-depth reconciliation (2026-11-17).

## Current instruction

Parity and polish come before new features (owner, 2026-11-14). Concordia reuses each provider's
own features and adds augmentations; it never rebuilds what a provider already does. Everything a
provider does natively must stay reachable from Concordia. Work the plan below in order, one item
per task, each proven in the running app and with a live turn where a provider is involved.
Publication remains on hold.

## Completion-depth audit and Claude resume - 2026-11-17

Owner instruction: inspect how thoroughly existing capabilities work, benchmark current
competitors, and leave the interrupted agent an accessible continuation. Table stakes cover
complete expected workflows and their interaction details. Feature names, source presence,
passing unrelated tests and terminal escape hatches do not establish that completeness.
This is an audit and proposed repair order, not approved new product copy or permission to
publish, change authentication, grant hook trust or implement unresolved product choices.

### Resume the interrupted work

Latest source WIP: `2026-11-18b`. Codex app-server now offers steer-or-queue for a plain-text draft
during an active turn. Steering binds to the exact thread/turn, preserves the draft until acceptance
and never automatically retries uncertain input. Cursor 2026.11.17 usage now submits with CSI-u
Return; a real local-only fetch passed. Habena's eight sealed lock fixes were approved by the owner.

Installed 2026-11-19 at the owner's request (`2026-11-18b, 40897e1+0723`, uncommitted tree). A live
Cursor test of the installed build ran on an isolated state copy. The header chip opened the model
and effort dropdowns. On the Haiku API quota error, Concordia showed the error, kept the prompt and
retried on the same session with Auto. A turn completed, and a Tab-queued message was sent after
the running turn. A restart reopened the chat with its history, and the model recalled the first
prompt. Doctor lists 15 unopenable old chats (moved Codex 3 home, missing handoff maps); see
`concordia chat repair --dry-run`.
Claude defaults to the existing stdio adapter
that handles `AskUserQuestion`; an explicit saved off choice remains respected. Standing context
names Concordia and the question tool. Owned launches supply a bounded client/provider/tool/timing
declaration for Habena; ownerless launches remove inherited declarations. Ordinary owned Codex
turns already route through app-server; no fresh Codex delay was reproduced or separately fixed.
The existing standing-context and `images` integration cases pass. Live Claude/Codex mid-turn
question/answer verification remains open; deterministic fixtures do not close it. Installed 15c
and global Habena are unchanged. Continue with the related existing Habena preference plan for
personal opt-in, native lifecycle review/distribution and context-cost measurement; never grant
native trust automatically. Prefer deterministic mechanics and compact context over model work.

Implementation continuation: `2026-11-17a` fixes ACP discovery within C3; other providers'
catalog discovery remains open. Current scoped qualification is recorded in
[verification](docs/e2e-status.md). The audit baseline identities below are historical observations.
Candidate `2026-11-17b` implements C1/C2's local plugin and durable clarification/FIFO flow.
Synthetic native-window checks and 32 existing scoped cases pass. After explicit approval of
the scratch workspace through Cursor's native prompt, two tiny Auto turns passed the live
question/delayed-answer flow on one exact session. Installed copies remain unchanged.
The earlier isolated-startup failure was reused synthetic state containing the inherited Codex
home. Clean seeded state starts with the guard enabled; no product startup fix was required.

- Branch `campaign/ui-logic-1115`, HEAD `40897e1`; source stamp `2026-11-16a`.
  The uncommitted `doctor.rs` change counts signed-in and total accounts using the same account
  owners as `/use`. It is complete; its reused proof is in [verification](docs/e2e-status.md).
- The installed executable still reports `2026-11-15c`. Existing debug output reports
  `2026-11-16a, 40897e1+1536`; release output reports `2026-11-16a, df17303`.
  These are distinct candidates. Do not rebuild or install merely to update this audit.
- The interrupted campaign's receipts remain in `target/overnight/2026-11-15-ui-logic/`.
  Its manifest and the recovered Claude session contain later evidence than the historical
  15a status below. The reconciled results and their limits are in
  [verification](docs/e2e-status.md#2026-11-17-evidence-reconciliation).
- Preserve the separate Habena checkout's dirty anti-slop integration and preference-program
  changes. Its current completion/enforcement findings belong in its existing preference plan.
  Do not infer that an identical package version means identical installed policy bytes.

### Audit method and completion condition

For each row below, trace entry point, state owner, provider transport and return path. Inspect
initial, loading, empty, successful, failed, cancelled and recovered states, plus keyboard,
focus, resizing, background operation and simultaneous chats where applicable. A non-applicable
state needs a concrete reason. A passing happy path does not close the other states.

Each result must name the candidate, provider version, transport, account/home binding and
specific scenario where relevant. Classify evidence as source inspection, dated execution,
fresh execution or unavailable. A fixture is not a live provider turn; a live adapter test is
not native-window proof; a focused rerun does not erase an earlier suite failure.

Use existing owners, fixtures and journeys. Run the smallest relevant check after a change;
reuse it while its relevant inputs are unchanged. Follow AGENTS.md for live-turn scope and
release qualification. Do not add per-component hooks, duplicate reports or a new test suite.
For an existing broken capability, bring the observed impact to the owner before continuing
enhancements. Material product choices use the interactive question tool; independent reads
can continue while dependent mutations wait.

### Source-confirmed gaps and finite acceptance

Paths in this table are under `crates/concordia/src/`. These are inspected implementation
limits, not fresh claims that every corresponding installed interaction is broken.

| ID | Gap and owner | Required acceptance |
| --- | --- | --- |
| C1 | Candidate 17b adds a session-local question plugin to Cursor Standard. Native-window and real-provider qualification are tracked below. | Fresh chat and exact-session resume expose questions without changing default transport or native session identity. |
| C2 | Candidate 17b persists conversational question data through existing pane recovery and FIFO replies. Native approvals remain live/run-bound. | Delayed/partial replies, multiple bundles, restart, account handoff and save failure preserve decisions without replaying approvals. |
| C3 | ACP metadata reaches the session-bound picker (17a). Claude stdio now announces its slash commands and descriptions per session (2026-11-18, fake-proven). Codex app-server 0.166.0 has no command-list RPC, so Codex keeps its static list. | Advertise before the first prompt, change availability and switch accounts. Discovery and invocation must retain exact session ownership. |
| C4 | 2026-11-18: `/fork` calls `thread/fork` from the installed 0.166.0 schema and adopts the new thread after workspace and approval checks; proven against the fake app-server (Concordia live testing is Cursor-only). | Fork preserves the original; subsequent prompts, history, approvals and native terminal bind to the selected new thread only. |
| C5 | 18b adds Codex app-server steering with explicit Enter confirmation or Tab queue, exact thread/turn IDs and acceptance-bound draft clearing. Other transports and attachments retain existing behavior. | Qualify acceptance, refusal, late completion and queue choice separately; never replay uncertain input. |
| C6 | `native_history.rs` bounds the display to 200 messages/1 MiB; Cursor restoration is text-only. Native sources remain separate. | Long and truncated chats explain the limit and offer exact older-source access; resume preserves native identity and tool-history limitations are visible. |
| C7 | `tui/managed_workspaces.rs` has an action handler with no caller found; lifecycle library code alone does not prove reachability. | Reach creation, inspection and reviewed integration from the product. Changed heads, conflicts, dirty files and active ownership invalidate unsafe actions. |
| C8 | Native diffs and Codex `review/start` exist; a connected result-to-review/check/PR flow was not traced. | Follow one delegated change through precise feedback, review, explicit integration, applicable checks and PR readiness; retain task/file/revision identity. |
| C9 | Public cross-provider `agents` in `mcp.rs` exposes list/start/status/result, without follow-up/cancel/watch actions. Native tools differ. | The lead can inspect and control its worker, receive completion and distinguish quota, crash, cancellation and success without rerunning uncertain work. |
| C10 | `tui/settings.rs` inventories local skill/hook sources and explicitly disclaims full enumeration or active execution. | Distinguish configured, discovered, enabled, actually exposed and freshly proven for the current provider/project; opening a source proves none of the latter. |
| C11 | Owner, 2026-11-18: providers stay current and nothing should break. A Codex newer than the qualified 0.166.0 is tried on the app-server; a failure before the prompt is sent reruns that turn on the standard transport with one notice (fake-proven). Owner: `provider/turn_appserver.rs`, `provider/turn.rs`. | Capability decisions use the qualified installed protocol. An upgrade produces an explicit compatibility result and preserves the draft/session on refusal. |
| C12 | Guided provider installation remains incomplete in `connect.rs`; login and diagnostics exist. | A missing CLI has an actionable setup path; cancel, failed install, native login and unverified versions retain user control. |
| C13 | 18b uses CSI-u Return for Cursor 2026.11.17. One real local-only usage fetch returned a parsed table; LF failed before that correction. Owner: `cursor_usage.rs`. | Preserve no-model-prompt behavior, owned process cleanup and explicit usage-unavailable errors when the screen changes. |

C1 must preserve the default native-terminal resume contract while making questions dependable.
An earlier tiny native Cursor print-mode prototype loaded a session-local plugin, called `ask_user` and
received the synthetic callback answer. This qualifies that transport route, not product UI
integration or pending-question persistence. Candidate 17b's scoped implementation evidence is
recorded in verification. No global plugin registration or blanket MCP approval
was used. The ordinary `mcp list` subcommand did not enumerate the session plugin, so that list
alone is not a readiness check for this route.
Changing the default transport still requires an owner decision; do not silently switch to ACP.
C2's timeout bridge is a real improvement; it is not proof that a model cannot end the turn
while a question is unanswered. Native approvals and conversational replies remain different.
Owner decision (2026-11-17): unanswered clarifications and eventual answers follow the task
when the user selects a replacement agent/account. Preserve the original provider, account and
session as visible provenance. Native file, command and plan approvals remain bound to their
original live run; never restore or transfer them as grants. Implement this through the existing
question panel, pane recovery record and FIFO reply queue, without changing the default transport.
C5 first needs installed-protocol capability evidence and any remaining parity decision;
upstream main alone does not establish CLI 0.166.0 support.

C7/C8 preserve the existing owner decision: multi-agent review follows the provider/harness;
code review uses provider change views and the user's Git/editor tools. The journey may cross
those surfaces. It does not authorize the rejected unified Concordia changes panel or a new
PR subsystem. Trace and repair Concordia's responsibility for identity, reachability and return.

### Remaining surfaces to qualify

These obligations stay open; they are not newly reproduced defects or a silently reduced scope.

| Surface and existing owner | Specific outstanding qualification |
| --- | --- |
| Home, tabs, settings; `tui/landing.rs`, `tui/tabs.rs`, `tui/settings.rs` | Repeated navigation, draft retention, selected-tab visibility, keyboard/mouse agreement, narrow layouts and stale footer state. |
| Composer and recovery; `tui/editor.rs`, `tui/persist.rs`, `tui/recovery.rs` | Unicode selection, large/multiline paste, undo/redo, prompt history, attachments and queued edits across close/restart. Benchmark external-editor/draft-stash behavior explicitly. |
| Transcript; `tui/markdown.rs`, `tui/transcript.rs`, `provider/stream_cards.rs` | Long streaming/tool output, expansion, selection/copy, scroll anchoring, errors and resumed event fidelity. |
| Inputs and previews; `attachments.rs`, `attachment_documents.rs`, `tui/document_preview.rs` | Unsupported provider/transport, corrupt/oversized input, changed file, mixed attachments and cancel/retry. PDF preview exists; native PDF submission is Claude-only. |
| Models, effort, usage; `models.rs`, `usage.rs`, `tui/app/pickers.rs` | Unknown/stale catalogs, disappearing model, unsupported effort, per-model quotas, same snapshot across surfaces and failure without draft loss. |
| Permissions and attention; `tui/questions.rs`, `tui/worker_input.rs`, `provider/interaction.rs` | Queued requests, free text, multi-select, stale/foreign answers, concurrent chats, dismissal, background notification and explicit cancellation. |
| Terminal and editor; `tui/local_terminal.rs`, `tui/workspace_edit.rs`, `file_document.rs` | Native fallback cold/warm startup, correct session, returning history, busy turns, shell lifetime, external file conflict and unsaved-close behavior. |
| Memory and handoff; `memory_retrieval.rs`, `provider/standing_context.rs`, `tui/reference_journal.rs` | Exact source readback, stale/missing source, bounded context, account handoff, compaction and quota recovery; no token-saving claim without measurements. |
| Integrations; `tui/integrations.rs`, `mcp_registry.rs` | Disabled/unavailable server, config changes, tool exposure by transport, reconnect, ownership and bounded failure; preserve native settings. |
| Accessibility and input; `tui/native_window.rs`, `config.rs` | Full keyboard path, focus restoration, scaling/contrast, screen-reader interoperability, notification delivery and shortcut configuration. |
| Update, restart and cleanup; `update.rs`, `tui/update_ui.rs`, `install_watch.rs` | Running turn, unsaved edits, multiple windows, failed download/hash, interrupted install, current-build identity and owned-process cleanup. |
| Wider competitive coverage | Keep cloud/SSH/mobile, scheduling, collaboration, export and data-retention expectations visible. Map to an existing native/external route or an explicit owner decision. |

Current benchmarks, checked 2026-11-17: [Claude Desktop](https://code.claude.com/docs/en/desktop.md),
[Claude interaction](https://code.claude.com/docs/en/interactive-mode.md),
[checkpoint recovery](https://code.claude.com/docs/en/checkpointing.md),
[Cursor CLI](https://cursor.com/docs/cli/using), [Cursor ACP](https://cursor.com/docs/cli/acp),
[OpenCode interactions](https://opencode.ai/docs/cli/tui),
[Conductor review](https://www.conductor.build/docs/reference/diff-viewer) and
[checks](https://www.conductor.build/docs/reference/checks).
Use [ACP v1 command advertisements](https://agentclientprotocol.com/protocol/v1/slash-commands)
for the actual integration contract; v2 is a draft.
Current [Codex app-server source](https://github.com/openai/codex/blob/67d8266e06e469e5aef78a23881c7e19c37030b0/codex-rs/app-server/README.md)
and [method registry](https://github.com/openai/codex/blob/67d8266e06e469e5aef78a23881c7e19c37030b0/codex-rs/app-server-protocol/src/protocol/common.rs)
include fork/steer and replace removed `thread/rollback` with `thread/revert`.
Upstream main is a research reference, not the qualified schema of installed CLI 0.166.0.

### Proposed execution order

1. The doctor edit is complete (uncommitted); preserve exact candidate identities. Use clean seeded synthetic
   state for native-window checks; the reused-state startup blocker is resolved without guard changes.
2. C1/C2's scoped default-Cursor question/FIFO qualification is complete in candidate 17b;
   retain its installed/source distinction and native approval boundary. Wider provider and
   surface obligations remain open; the checks do not establish a mutation hook.
3. Close C3/C4/C11 using versioned provider capability discovery and exact-session return paths.
4. Close the delegated-change acceptance journey across C7/C8/C9, then run the remaining
   surface obligations. Preserve earlier fixes; do not restart the campaign from stale bugs.
5. Bind the resulting accepted obligations and evidence to Habena's proposed task guard.
   Its implementation and native activation are separate work, not established by this plan.

## Historical parity and polish observations (2026-11-14)

Recheck these observations against the checkpoint above before treating them as open defects.
Current source already has same-session terminal resume/fallback, PDF/SVG preview and a
minimum-version Claude check. The historical exact-version and raw-preview descriptions below
must not trigger duplicate implementations. The old `_work/audit/out/` source is absent locally.

Source: the audit in `_work/audit/out/` (lanes U1 to U6, CL, CX, CU, OR), run on build
2026-11-14b. Grok and opencode are out of testing scope because they are not installed.

### 1. Turns must start and finish

- A turn can stay at "starting the turn" forever: Cursor (CU-2), Codex (U4-3), and a Claude chat
  that dropped into a paused usage view (U5-3). On a timeout or a closed transport, show an error
  with the provider and session, keep the prompt, and offer retry.
- Input sent while a turn runs is concatenated into the draft instead of becoming a queue item
  (U3-2), and `/use` typed during a turn becomes part of the next message (U3-4).

### 2. `/model` lists the provider's own models

- Claude model metadata is rejected because Concordia requires exactly CLI 2.1.319 and 2.1.325 is
  installed (CL-1, U4-2, U6-5). Accept the pinned version or newer.
- Every provider shows display name, full model ID and the provider's effort levels; Cursor's
  list is empty today. In progress as task FIX1 on `fix/model-picker`.

### 3. Native features are reachable on the same session

- `/terminal native` starts a separate provider session (CL-3, U2-3). It must resume the chat's
  own native session for Claude, Codex and Cursor.
- A provider command that Concordia cannot run through its transport opens in the native terminal
  on that same session, instead of being disabled (U4-1, CX-2 to CX-10).
- Command pickers come from what the provider reports (Claude `initialize`, Cursor ACP
  `available_commands_update`, the Codex app-server schema), not from hard-coded tables (CL-2,
  CU-1).
- Codex requests that exist but are unused get wired where they map one to one, starting with
  `thread/fork` (CX-3, CX-11).
- Claude's native permission modes are selectable, not only Concordia's four presets (CL-4).

### 4. Screens that are wrong or unstable

- PNG, SVG and PDF open as one line of raw text, and a corrupt image shows no error (U5-1, U5-2).
- Home and `/usage` show different numbers, and Home omits Cursor (U1-1). Both read one snapshot.
- `/usage` flips between "cached; refreshing" and the settled text while idle (U4-4); show the last
  good reading with its age and never swap it for an "unavailable" line.
- F1, `/config` and other overlays cut text mid-word and draw borders past the window edge at
  narrow widths (U2-1, U6-1, U6-2). F1 also changes while idle (U6-2).
- Mojibake in settings and terminal labels (U2-2, U6-3, U6-4). Part of FIX1.
- Claude usage readings time out; read the usage data Claude Code already writes instead of
  asking for it.

### 5. Handoff: fewest tokens, readable display

- The agent receives one compact block: goal, state, changed files, last error, last reply, and
  the exact read command plus source ids. No section prose, no one line per turn (OR-5).
- The user sees a card: from which account and provider to which, the pointer count and token
  cost, expandable into the grouped pointers.

### 6. Orchestration the lead agent can rely on

- Every provider process is killed when its turn ends, by design (OR-1). Work that must outlive a
  turn runs under a Concordia-owned supervisor with recovery state, not under the turn.
- The `agents` tool reports a durable watch record per worker: heartbeat, last event, process and
  session ids, how it ended, and log pointers; cancel and retry are available (OR-3).
- A delegated worker that hits a usage limit records that reason and can resume on another account
  (OR-2).
- `concordia.log` tells a crash, a kill, a hang and a normal exit apart (OR-4).

### 7. Test channel gaps that block automated UI checks

- `resize` yields a smaller grid than requested (U1-2); add clipboard read and a way to drive
  detached windows (U3-3, U5-4). Every later task proves its screens through this channel.

Shared command names (owner, 2026-11-14): when a command name exists in Concordia and in a
provider (for example `/mcp`, `/model`, `/usage`, `/config`, `/doctor`, `/resume`), running it opens
a picker to choose who runs it: Concordia, or a specific account's own provider (`claude`, `codex`,
`codex-2`, `codex-3`, `cursor`). The provider choice runs the native command on that account,
through the transport or the native terminal on the same session. This replaces the 2026-11-11 rule
that every shared name belongs to the provider, and settles the `/mcp` conflict (CX-1).

## Cleanup closeout

The audited implementation and its previous documentation are preserved in local commit
`6fea4a9`. This is a preservation checkpoint, not full release qualification.

- One worktree remains. The current work branch and `main` are retained. No obsolete build cache,
  disposable repository archives or unused installer images were found in the final inventory.
- The 2026-11-09g executable identity and scoped checks are historical. Current installed/source
  observations and later campaign evidence are in [verification status](docs/e2e-status.md).
- The owner completed manual removal of the older executable, matching receipt and four synthetic
  Temp folders. Their absence was verified on 2026-11-09; those cleanup exceptions are closed.
- Native histories, credentials, drafts, queues and unique recovery records are preserved.

Retained-snapshot inspection, broader provider/host qualification, native hook enforcement and
missing provider signals remain unfinished roadmap work. They are paused, not cleanup deletions.

## Documentation and product interview

Concordia currently provides a local Windows interface for the official Codex, Claude Code and
Cursor CLIs, with conversations, source retrieval, workspaces and worker coordination. Providers
own authentication, native sessions, tools and approvals. Product direction must distinguish
these implemented contracts from future ambitions.

The owner's broad AI IDE intent is input to refinement, not approved canonical wording.
Show proposed definitions and material decisions in chat for review before writing them into
product documentation. Distinguish agreement on the broad intent from approval of its exact scope
and wording. Do not mark a raw description as a finalized product definition.

Grill: elicit concrete workflows and probe their needs and tradeoffs before drafting the product
definition. The owner wants a Socratic interview, not a request to write or fix the definition.
Show refined wording after the underlying decisions are clear, then obtain approval before making
it canonical. Do not advance to another topic by treating an unreviewed draft as settled.

Interview scope: define Concordia's behavior, supported capabilities and how it accommodates
user-chosen workflows. Do not prescribe which tools users adopt or how their agents perform
domain work. Examples such as Blender MCP illustrate compatibility needs, not mandatory tool
choices. Ask about Concordia's observable behavior, integration boundaries and failure handling;
investigate technical integration details rather than asking the owner to design them.

Keep Concordia relatively slim. Supporting a workflow does not imply reproducing every tool's
interface or adding a dedicated feature. Evaluate a clear unmet Concordia responsibility before
proposing additions; retain approved capabilities and revisit unresolved scope explicitly.

Lock use is deferred at the owner's request. Feature work remains paused. Interview notes below
record answers and open proposals; they are not approved final product wording or shipped claims.

### Latest interview inputs (not approved product copy)

- Initial launch targets Windows. Support for other operating systems after that is undecided.
- Initial launch is public: an open-source tool distributed through GitHub, with GitHub Pages
  for documentation and feature showcases. This is distribution intent, not publication approval.
- First-run onboarding offers guided provider setup: help users choose and install supported
  provider tools and complete their native login. Check underlying provider tools for updates
  as well as Concordia itself. Users may explicitly install provider versions whose compatibility
  with Concordia is unverified; they are not restricted to verified releases. Concordia does not
  install updates automatically: it notifies the user about new Concordia and provider releases,
  and the user installs them through `/update` or the provider's own updater.
- Vision input: the main place for development using AI, including code changes, creative
  assets and coordinated multi-agent work. A ten-subagent audit/change is an explicit use case.
- Definition decision (2026-11-11): Concordia is an augmenter, as defined in the Harness
  architecture model (`docs/architecture-model.md`, draft on branch `docs/architecture-model`).
  Use its role terms (provider, augmenter, harness, project, warehouse); do not call Concordia
  a workspace. The MVP runs the official provider CLIs unmodified under the user's accounts,
  coordinates chats and agents, gathers questions and approvals, hands work between providers
  and accepts a pluggable harness and memory. The existing built-in file editor, previews,
  Mermaid and image rendering stay in the MVP. A wider interface (dockable panels, embedding the
  user's own editor, Asset Lab, browser-to-chat feedback, custom panels) is presented publicly as Concordia's
  intended direction, not as missing work. Final README wording awaits owner approval.
- MVP scope (2026-11-11), superseding narrower entries elsewhere in this list:
  - MVP: onboarding with provider setup and update notices; the Projects panel; the Agents panel;
    Needs attention with reliable questions and approvals across providers; the taskbar status
    dot and notifications; close, background and resume behavior; `/model`; `@chat`; per-chat
    PowerShell; attachment previews; live usage; the built-in editor, previews, Mermaid and image
    rendering; private-notes and Harness support through the pluggable memory/harness extension point;
    providers Claude Code, Codex, Cursor, Grok Build and Grok Bot; concise GitHub Pages
    documentation.
  - Intended direction: fully dockable panels with per-chat layouts (MVP: show, hide and resize
    in one shared layout); embedding the user's own editor; Asset Lab; browser-to-chat feedback;
    custom tool panels; shared named terminals; `/skills` editing and propagation (MVP: list with
    on/off); chats in several projects at once (MVP: one project per chat); provider-history
    deletion.
  - Out of Concordia: Goals; the private-notes reaction-poller fix (external repository backlog).
  - Open: a parity review against Claude Code, Codex and Cursor may add MVP features.
    Review findings (2026-11-11, awaiting owner decisions): the main gap is that provider-native
    slash commands (`/compact`, `/init`, `/mcp`, `/diff`, `/fork` and others) fail as unknown in
    Concordia (`commands.rs`). Further candidates: rewind/undo a turn, fork a chat, on-demand
    compact and clear, provider MCP server status, permission rule display, a Vim composer mode
    and conversation export. Whether each provider's programmatic mode accepts native commands is
    unverified.
  - Command-surface audit (2026-11-11, README command table and `commands.rs`): overlapping
    entry points for multi-agent work (`/delegate`, `/workers`, `/crew`, `/worker-panel`, plus the
    planned Agents panel), account management (`/use add`, `/login`, `/connect`, `/workers add`),
    file preview (`/open`, `/preview`), shared setup (`/skills`, `/hooks`, `/harness`) and recovery
    (inline retry/account handoff, automatic draft/queue restoration). The permission mode is
    named Edit; `/worktree` "Git workspaces" remains a separate filesystem concept.
  - Grok research (2026-11-11, official xAI docs, partly unverified): Grok Build is a beta `grok`
    CLI with headless streaming JSON, session resume and an ACP agent (`grok agent stdio`), so it
    can likely reuse the Cursor ACP path. Grok Bot is a cloud-hosted assistant used through chat,
    Slack, X and desktop apps, with no documented CLI, API or ACP; it does not fit the augmenter
    model as specified. Owner decision (2026-11-11): Grok Bot stays in the MVP. The owner's use
    case is research, adding notes to a private notes service and social-media growth for Goodman and Poolaris,
    not coding. The integration mechanism is unresolved. Follow-up research (official
    docs.x.ai pages, partly unverified): Grok Bot has no documented API, CLI, webhook or ACP. It
    is a confirmed MCP client: its desktop app adds custom remote HTTPS connectors, reached from
    xAI's cloud machine, so a private notes server would need a public, authenticated endpoint. Other
    routes (Slack relay with the user's token, the Bot email inbox) are speculative. Grok Build
    may cover research through web search tools (unverified in official docs). Third pass:
    Grok Bot routines accept an authenticated webhook trigger (HTTP POST with a bearer key;
    `200` means the run started, with no result returned), and a routine's instructions decide
    where results go. Cursor's Cloud Agents API does not reach Grok Bot. A two-way route is
    therefore webhook in, MCP connector or email out. Owner decision: two-way support.
    Concordia starts Grok Bot routines through their webhook and receives results and questions
    through a one-tool connector; Grok Bot appears in the Agents tree and Needs attention. The
    webhook key is stored in Windows Credential Manager, used only by Concordia when triggering
    the routine and never exposed to agents. This changes two README promises (Concordia never
    handles credentials, and network traffic comes only from provider CLIs) and both need
    accurate rewording. How results return to Concordia is open. Findings (docs.x.ai
    computer-and-apps, troubleshooting, files-and-results): results land in the Bot's own chat,
    which no API or export can read; a Bot can save files to connected Google Drive, OneDrive or
    SharePoint; with "Execution on this computer" enabled (default "Ask every time"), a Bot can
    run commands on the user's PC, but whether routines can use it is undocumented, unattended
    approvals expire after about 30 minutes, and a known Cursor-side bug can report it as off.
    Owner decision: results and questions return through a callback endpoint exposed by a tunnel
    the user chooses and runs (ngrok, Cloudflare Tunnel or Tailscale Funnel); Concordia guides
    setup and never ships or runs tunnel software. Safety and efficiency requirements: the
    listener is off unless Grok Bot is configured and binds only to localhost; it exposes only
    the callback (no chat history, private notes or other tools); every call needs a Concordia-generated
    bearer token stored in Windows Credential Manager and rotatable; Concordia accepts results
    only for run IDs it started; requests are size- and rate-limited; returned content is treated
    as untrusted external text; nothing polls, so there is one call per finished routine. When
    the tunnel is unreachable, the Grok Bot agent shows that state, and the result remains in
    Grok Bot's own chat.
    Credential limits stated honestly: Concordia never passes the key to agents, through
    prompts, tool results or environment variables, but Windows Credential Manager protects
    against other users and disk theft, not against processes running as the same user. An
    agent with unrestricted shell access could read it deliberately, as it could read provider
    CLI tokens today; provider sandboxes, permission modes and harness hooks are the defence.
    Keys are stored and looked up by explicit identity (provider, account and routine IDs),
    never by display name, so two entries with the same name cannot be confused. Connected
    routines are global: any chat in any project can trigger them, and the picker shows enough
    detail (routine name, account, description) to choose correctly.
- Documentation site survey (2026-11-11, 17 projects): GitHub Pages is the most common host.
  Rust CLIs split between MkDocs (wezterm, atuin, uv) and mdBook (helix); AI agent CLIs mostly
  use JavaScript or hosted generators (Mintlify, Starlight, Docusaurus). The common navigation
  is overview, install, quick start, usage, configuration, reference and FAQ, with the docs as
  the homepage for smaller projects. Generator choice awaits the owner.
- Documentation must have its own distinctive look at the quality of leading projects, not a
  copy of another tool's theme. Decision (2026-11-11): build it with Astro Starlight on GitHub
  Pages, themed with the vendored studio theme. Constraint found: the source theme package 1.3.0 is
  MIT-licensed but published with restricted access on GitHub Packages, and the source theme is not yet
  public, so a public Concordia docs build cannot install it without credentials. Decision:
  The source theme stays unpublished until after Concordia ships, so the docs vendor the studio
  theme (tokens, CSS and any components used) at a pinned version with a sync script, keeping the
  look consistent with the owner's portfolio. Switch to the published package once it is public.
  It must also explain Windows Defender and SmartScreen warnings
  for the unsigned executable. Research (2026-11-11; Microsoft SmartScreen and reputation docs,
  NousResearch/hermes-agent README, Upscayl, yt-dlp, espanso): the docs page "Windows security
  warnings" covers why SmartScreen warns on files without reputation (not a malware verdict);
  "More info" then "Run anyway" for the official release file only; verification before running
  (published SHA-256 checksums, `Get-FileHash`, `gh attestation verify`, building from source);
  Smart App Control, which can block unsigned files with no override; Defender false positives,
  folder exclusion as the user's own choice with its risk, and reporting through
  microsoft.com/wdsi/filesubmission; and the signing plan through SignPath Foundation, noting
  warnings can persist for weeks after signing and SignPath's required "Code signing policy"
  wording. Releases must publish checksums and build attestations.
- Tabs: each chat tab shows the same status icons as the Agents view (working, needs attention,
  finished, failed). Ctrl+T new-tab latency remains a reported problem.
- Chat rendering: markdown tables are hard to read in the terminal because rows have no
  separators. Improve table rendering (row rules or alternating shading, wrapped cells).
- Integrations stay lean: no Slack relay. Concordia's own MCP server should expose as few tools
  as possible, because every registered tool definition costs provider context on every turn.
  Cloud-agent integrations such as Grok Bot are optional; not every user has them.
- Provider command passthrough (2026-11-11): the `/` picker lists each provider's own commands
  alongside Concordia's, and every command name means exactly one thing. Ownership follows the
  architecture model's test: Concordia owns commands whose job spans providers, accounts or
  sessions (`/model`, `/usage`, `/resume`, Concordia's `/config`, `/doctor`, `/skills`, `/hooks`);
  every other name, including `/mcp`, `/compact`, `/init` and `/fork`, goes to the provider.
  Where a Concordia command replaces a provider one, it must cover the provider's options for
  the active provider (for example, `/model` exposes the provider's effort and speed settings),
  so no provider feature becomes unreachable. A published table records each name's owner.
- Legacy compatibility decision (2026-11-11): Concordia launches with no legacy paths. Before 1.0,
  back up and migrate the owner's data once: move the numbered provider homes (`~/.codex-2`,
  `~/.codex-3`) into the current account layout and convert old history, the shared draft and
  queue, token and usage records, and version-1 worker ledgers. After verifying, delete every
  legacy reader, `legacy_migration.rs`, the unscoped MCP mode and `/recover legacy`. The owner
  rejects compatibility code for a pre-release product; a Harness gate should catch new
  compatibility paths, because the written rule alone did not prevent about 195 references.
- Recovery decision (2026-11-11): remove `/recover`, `/continue` and the `/use` continue row.
  Interruptions are handled where they happen: "continue where you left off" after a crash or
  reboot, with interrupted chats kept in Needs attention until resumed or dismissed; an inline
  "continue on another account" when a chat hits a usage limit; an inline Retry for a failed
  request; automatic restoration of drafts and queued prompts. A chat whose provider session or
  Concordia record cannot be opened shows a repair action when opened, and `/doctor` lists such
  chats.
- Usage-limit continuation decision (2026-11-13): keep the existing inline continuation path.
  Open it only for provider-owned structured quota signals from Claude Code, Codex, Cursor, Grok
  Build or opencode. Put the role-configured fallback model first and keep the default policy as
  ask. Account usage and reset times come from provider snapshots. The continuation picker owns
  available and disabled model rows, wait-for-reset, stop, and the per-role fallback editor.
- Y3 completion: the continuation picker now orders configured fallback, available account/model
  rows, disabled exhausted rows, wait-for-reset and stop. Reset waiting resumes from cached
  provider usage, and `/config` exposes role fallbacks alongside the ask/automatic setting.
  Provider-facing live behavior remains unverified.
- Command consolidation following the picker principle: account switching, adding and removing
  live in one `/use` picker (`/login` and `/connect` go); `/open` and `/preview` merge into one
  `/open` that previews read-only with an edit action.
- MCP surface decision (2026-11-11): Concordia registers one always-on MCP tool, `agents` (list
  accounts, providers and models with usage; start a delegated agent; read its status and
  result), replacing today's ten tools (about 3,300 characters, roughly 850 tokens per turn).
  Chat history is reached through the `concordia chat` CLI, described only in handoff maps and
  `@chat` pointers that need it. The question bridge stays per-turn for providers without a
  native question tool; the Grok Bot callback is a separate connector only Grok Bot sees.
- Product framing (2026-11-11): most of Concordia's augmentation serves the agents. Concordia
  makes agents more reliable and more consistent by giving them better inputs: deterministic
  handoffs, context and directions built from recorded state, and access to cross-provider
  tools (`agents`, chat history). It cannot make a model deterministic; it makes what the model
  receives deterministic, so the same state produces the same handoff. The interface for the
  user (tabs, Agents tree, Needs attention, usage) is the other half. Evaluate memory and
  context features by whether they improve an agent's next action.
- Handoff decision (2026-11-11): the handoff map gains a deterministic "where things stand"
  section built from structured data, never summarised: files changed and the repository's
  uncommitted changes (from Git, the same for every provider), commands run with pass or fail
  status and the last error (from the normalised tool events Concordia already parses for
  display), and the previous agent's last reply. Anti-bloat rules: one fixed character budget
  for the section; counts plus the top few items, with a `concordia chat` pointer to the rest;
  long replies capped with a pointer to the full text; empty parts omitted; no new
  per-provider parsers. It replaces `thread_map`, file facts and the `/memory` link, lineage,
  group and goal-relation features, so memory code shrinks overall. Success is measured per
  task: the next agent should need fewer discovery turns.
- Handoff research (2026-11-11; sources include Chroma context-rot, "Lost in the Middle",
  JetBrains "Complexity Trap", Factory's compression evaluation, Anthropic context-engineering
  and long-running-harness posts, Manus, Cognition, Amp handoff): short focused context beats
  long context; observation masking matched LLM summarisation at half the cost; structured
  summaries with fixed sections beat free-form ones, and every summary method loses the file
  trail, which a deterministic index keeps; compression should stay restorable through
  pointers; keep errors; place the goal at the start and the current state at the end;
  never carry a previous agent's "done" claim unchecked. No public evaluation compares a map,
  a summary and an agent-written note across providers. Candidate designs: an enriched
  deterministic map; the map plus a note the outgoing agent keeps updated (nudged at context
  or usage thresholds, with the map as the guaranteed fallback); optionally an Amp-style
  drafted starting prompt when the old session is still usable. Owner decision: the enriched
  deterministic map only. Concordia does not ask agents to keep a note, because that would
  impose work on the agent and a session can run out mid-task before writing it. Map sections:
  the user's goal (first ask and latest redirect), files the chat edited (from tool events), the
  last error verbatim, the previous agent's final message (start and end), and pointers;
  goal first and current state last; a previous agent's completion claim is labelled as a claim.
- `/context` and `/worktree` decision (2026-11-11): remove the `/context` command and the
  compaction overrides; providers keep their native compaction. Proactive rollover (start a fresh
  session with the handoff map at a context threshold) becomes an off-by-default `/config`
  setting, Codex-only until other providers report a verified context measurement. Remove
  `/worktree` and the managed worktree code; isolating parallel agents is left to providers and
  the user's harness.
- Extension contract (2026-11-11): `harness.json` is the single extension point for harnesses
  and memory systems such as private notes (connect and verify per provider, skill, pinned decisions,
  tools). One `/harness` picker handles connect, status, verify and disconnect. `/skills` and
  `/hooks` remain views (with skill on/off) and lose their duplicate connect actions. Harness
  tools are listed in `/harness` for the user to see and test-run; agents reach them through
  the harness's own skill, not through Concordia.
- Host decision (2026-11-11): Concordia runs only in its native Windows window. Remove
  `--terminal` mode and the WezTerm host (`host/wezterm.rs`, `--host`, the `wezterm` subcommand
  and WezTerm-dependent `/edit` tabs). Custom title bar, taskbar dot, scrollbar, images and
  future panels all assume the native window.
- Quick calls (2026-11-11): keep the per-colour theme overrides; keep the CPU and memory footer
  as a `/config` show or hide setting. Remove the awareness block Concordia writes into provider
  `AGENTS.md` / `CLAUDE.md` files and the `threads.awareness` setting; every map instead carries
  its own one-line instruction for reading pointers. Keep user-defined shortcut commands
  (`commands/<name>.md` becomes `/<name>`, working on every provider); Concordia's built-in names
  cannot be taken, and a shortcut that shadows a provider command is reported by `/doctor`.
  Remaining commands: remove `/split`, `/edit` and every other WezTerm-dependent command, and
  remove `/trust` (duplicate of `/mode`); keep `/rename` and `/close`.
- Command picker rule (2026-11-11): every available command (built-in, user shortcut and provider
  passthrough) appears in the `/` autocomplete picker with its description. Reported bug:
  `/rename` does not appear or autocomplete today.
- Pins decision (2026-11-11): drop pins (Concordia's redacted backup copies of referenced
  snippets). Pointers rely only on provider history. When a pointer cannot be read, Concordia
  states the detected cause rather than a bare "unavailable": file deleted or moved, file
  replaced, content at that range rewritten or truncated, session header changed, account home
  changed, or record too large, with the time of the change when known and what the agent or
  user can do next. Concordia does not try to recover or substitute other content.
- Test policy (2026-11-11): releases should not wait on rerunning everything. Each change runs
  only the tests for the modules it touches; the full suite runs before a release and on pull
  requests into the integration branch. Removing a feature removes its tests in the same change.
  Agents do not add new unit or integration tests unless the owner asks. A later pass measures
  the slowest suites (for example the TUI journey tests) and trims overlapping coverage.
- UI test driver (2026-11-11): end-to-end UI tests drive the native window, not terminal mode.
  The native window gains a test control channel (enabled only by an environment variable set
  by the test harness) that accepts real key and mouse input and returns the rendered cell grid
  and window state, so journeys stay deterministic without screen scraping. The journey tests
  move to it before terminal mode is removed.
- Reported (2026-11-11): a Claude Code turn that leaves background tasks running stays on
  "putting the answer together" after its final reply, and the next message kills those tasks.
  Concordia should mark the turn answered when the final reply arrives and show remaining
  background tasks as subagents in the Agents tree rather than holding the turn open. Observed
  the same day: processes an agent starts detached (`Start-Process`) are also killed when the
  turn ends, which suggests Concordia terminates the whole process tree or job. Long-running
  work an agent deliberately detaches should survive, or Concordia should show and own it.
- Requested (2026-11-11): Concordia's own system messages (for example "cancelled - queued work
  paused; completed actions were not undone") must stand out from chat text through colour, icon
  and plain wording. Tabs show an I-beam cursor over the title so renaming is discoverable.
  Queued messages: a way to send one now, double-click to edit, and copy from the edit box.
- Testing decision (2026-11-12): use deterministic tests wherever they can prove the behaviour,
  including a regression test for each bug fix, and run live provider turns wherever a change
  depends on real provider behaviour (questions, approvals, transports, usage, images, command
  passthrough, new providers). Fake-provider runs never count as verification of provider
  behaviour.
- Roles and models (2026-11-12, owner direction): users choose which model fills each role, for
  example orchestrator, implementer, reviewer, re-reviewer and general helper. The Agents view
  shows each agent's role, model, effort, context use and turn count, as in the owner's
  reference screenshot. How this relates to the earlier "routing lives in the user's skill"
  decision, and BYOK support for API-key models such as DeepSeek and other open models, are
  open questions.
- Idea (2026-11-12, owner interest; not decided): token savings in the style of Spotify's Portal
  "shunt" plugin (engineering.atspotify.com, Sep 2026), which reports about 90% fewer tokens on
  bulk file reads. A Claude Code hook blocks large file reads, a cheaper model reads the files
  and returns a structured answer, and generated code is written straight to disk. The article
  states the limits: editing and reasoning cannot be delegated, and each delegation adds
  10 to 30 seconds. A Concordia version would route bulk reading through the `agents` tool to a
  cheaper model on the user's own account (Concordia makes no model call itself) and return
  exact line ranges as pointers so the main agent reads only those lines. The read-size hook
  is policy, so it belongs to the user's harness.
- Tabs and panes (2026-11-11): manage them browser-style with both drag and keyboard: drag tabs
  to reorder, keyboard shortcuts for switching, moving and closing, built so the same
  interactions extend to Photoshop-style docking later.
- Time header: Concordia already prefixes every turn with local time (`prompt_time.rs`). Make it
  a setting, on by default.
- Agent-adjustable configuration (2026-11-11): agents must be able to read and change Concordia
  settings through a validated `concordia config get/set` interface, and `/doctor` reports
  configuration problems with the exact command or setting that fixes each one.
- Determinism principle (2026-11-11, refined): Concordia is deterministic in what it does itself:
  it reads structured provider events rather than prose, injects context from recorded state, and
  never calls an LLM. It does not impose rules on how users or their agents work, because an
  augmenter adds to the user's tools rather than overriding them. Enforcement comes only from the
  user's own harness and `lock`; Concordia honours their verified receipts
  (`provider/enforcement.rs`).
- Routing decision: which provider and model handle a delegated task is set by the user's own
  routing skill, read by the agent. Concordia does not apply a routing table. When no routing
  skill is installed, Concordia shows a one-time, dismissible suggestion the first time an agent
  delegates; detection is a deterministic check of installed skills.
- Untracked provider runs (2026-11-11): when an agent starts another provider CLI itself (for
  example, a shell `codex exec`), Concordia does not block it. It recognises the command
  deterministically from the structured tool-call event and shows the run in the Agents tree as
  a subagent of the agent that started it, with status from the command's lifecycle and, where
  the provider records a native session, a link to that session's history. Such runs cannot
  receive messages or route questions to the user; the row says so.
- Reported bug (2026-11-11): scrolling up in a chat jumps to the very top instead of scrolling
  gradually. Requested: a visible scrollbar on the right side of chats, and a custom-coloured
  window title bar matching Concordia's theme instead of the default Windows title bar.
- Requested: when a message refers to an earlier attachment (for example, "your screenshot"),
  the user should be able to view that image directly in the chat, such as through a clickable
  reference that opens the original attachment.
- Scope audit (2026-11-11, read-only, awaiting owner decisions). Follow-through implied by
  existing decisions: cut the `/delegate`-era stack (routing table and policy, crews,
  `max_workers`, `/scope` reservations, worker ledger recovery; about 4,000 lines); cut the
  MCP surface to `agents`; shrink `connect.rs` into the `/use` picker; fold `/integrations` into
  `/doctor`; rename the Workspace permission mode; remove the `question_popups` setting, which
  could stop requests reaching the user. Items needing decisions: memory relations, groups and
  ranking (goal-based) and the size of `native_memory.rs` against the pointer-only `@chat`
  design; continuation and context-rollover settings (`/context`, `/continuation`, the fresh
  mode, handoff caps); one connect entry point and the extension-point contract for harness and
  memory; `/worktree` managed worktrees; the native window versus WezTerm host; per-colour theme
  overrides; the system stats footer; `/search` and project-wide replace; custom slash
  commands; the default-on awareness block written into provider homes; `thread`, `sessions`
  and `feed` CLI subcommands versus `chat`; the source-checkout update path; overlapping usage
  and token tracking modules; the remaining config keys.
- README and docs opening: state a problem and then Concordia's answer, in third person with no
  "I", and without augmenter terminology. Wording waits for the Harness writing skill and gate
  (owner request, 2026-11-11) so the copy is checked against them before approval.
  Writing tooling decisions (Harness work, prerequisite for Concordia copy): a general `writing`
  skill for any writing, built on the Authored copy register, with research-based rules and a
  draft, mark, rewrite, verify process. Project voice files (`docs/voice.md`) are an optional
  layer used only for website and marketing copy; the skill does not depend on them; and an
  extended `authored-copy-gate.mjs` with pattern-detectable tells (banned phrases such as
  "you already have/use", "seamless", "unlock", "all in one place", "serves as", "not just";
  bold-labelled bullets; em dashes). The gate blocks new text that contains a tell, except
  inside `lock` LOCKED regions, where it warns instead; an inline allow marker covers quoted
  examples. Existing text is not rechecked. Rollout: every Harness project at once, after the
  skill content is settled. Coverage: all human-readable
  text outside chat, including README, docs, site copy, UI strings, release notes, changelogs,
  PR descriptions and commit messages. Commit messages and PR bodies are checked through the
  shell policy hook and a commit-msg Git hook, because they are not written as files. Chat
  replies are excluded and keep the Caveman chat profile; the Authored copy register's scope
  line in Harness prefs must drop "assistant chat replies" to match.
- Slash commands remain available while a turn is running. Concordia's own commands act
  immediately; whether provider-native commands that change the session (for example, compaction)
  run immediately or wait for the turn to finish depends on each provider and is unverified.
- The agents panel shows elapsed runtime and lets users open an agent's activity, send it messages
  or interrupt it while preserving its reporting relationship to the main agent. Questions and
  blockers must be easy to find and answer/resolve. Further panel details remain under discussion.
- Delegation decision (2026-11-11): delegation is agent-initiated. Remove `/delegate`, `/workers`,
  `/crew` and `/worker-panel`. Concordia's delegate tool is on by default, targets any connected
  account, provider and model without separate worker registration, and keeps each delegated
  agent within the parent chat's permission mode. Users steer through the main agent and watch,
  message or interrupt agents in the Agents panel. Concordia exposes the available accounts,
  providers and models; which model handles which kind of task is harness policy, not
  Concordia configuration. Evidence: a Cursor chat could not start a Codex subagent because the
  harness expected Codex-native collaboration and Concordia's delegate tool was off by default,
  so the agent fell back to an untracked `codex exec` shell call.
- Interaction principle (2026-11-11): choices default to a small picker navigated with the arrow
  keys and Enter, instead of requiring typed arguments. Typed arguments remain an accelerator.
- Agents view (Ctrl+G) feedback: Ctrl+G must toggle the view open and closed. The current view is
  visually poor: rows show the raw task JSON (`{ "description": ... }`) and a generic "Native
  agent task" label. Redesign it with color, icons and established interface practices.
  Layout decision: a grouped tree. Agents nest under the chat or agent that started them, with
  compact rows (status icon, task title, elapsed time); each parent shows status counts and
  collapses. Provider, model, account and recent activity appear when a row is opened.
- Multi-agent result review follows the provider's behavior or configured harness. Concordia
  exposes that review flow rather than imposing its own individual-versus-consolidated process.
- Use provider change views and the user's Git/editor tools for reviewing code changes. The owner
  chose this over a new unified Concordia changes panel spanning chats, agents and repositories.
- Provide a dockable Needs attention panel gathering approval requests and questions across chats
  and subagents, alongside their original in-chat presentation. Show project, chat, agent, request
  and waiting time, with context and a link to the source conversation. Support responding in the
  panel where the provider permits; otherwise open the appropriate interaction. Both views share
  the same request state. Reading a request does not resolve it.
- Reported performance issue: opening a chat tab with Ctrl+T is very slow on every new tab,
  not just the first after launch. This is a user report, not a measured latency or established
  cause; inspect tab creation before proposing a fix.
- Attachment display: show previews for images and files with readily available previews;
  otherwise show a generic attachment label. Both display the filename. Snipping-tool captures
  retain their existing display behavior. Draft/sent-message interaction remains to be clarified.
- Asset Lab is intended direction, not MVP (2026-11-11). It is intended only inside Concordia; the earlier standalone portfolio/browser idea was
  withdrawn. AI generates or revises shapes; mouse/keyboard controls adjust styling through sliders
  and precise fields. Reuse existing Asset Lab capabilities where suitable.
- Bust quality example: find a suitable image reference and recreate it as a detailed 3D asset.
  The earlier crude geometry did not meet expectations. Use step-by-step human review for this
  workflow. Approval depth varies by task; its broader policy is still open.
- Shared development practices should use Harness, skills and hooks effectively. Investigate
  limitations and improve the appropriate owner rather than silently narrowing product scope.
- Harness is optional. Users can work with their existing provider setup without installing it.
  In the absence of explicit user rules, the model uses its judgment within existing provider
  permissions. Skill and hook management remains in scope as user control over that setup.
- Modular memory and harness integration is an MVP requirement: users can plug a memory system
  (such as a private notes service) or a harness into Concordia through a defined extension point. Concordia owns
  that extension point; a specific integration's setup belongs to that integration, not to
  Concordia's core. This does not authorize publishing or changing vault data.
- Needs attention status and Windows notifications are reserved for items with a user action.
  Passive idle notices remain available in the local picker but do not raise taskbar status or
  notifications. Questions, approvals, paused queues, signed-out confirmations and worker results
  remain actionable; each item notification is durable by item identity and clicking it focuses
  its owning chat and item.
- Question and approval requests must work reliably across every supported provider, despite
  differing native mechanisms (native question tools, Cursor ACP extensions, the MCP question
  bridge). Every request an agent makes must reach the user in the chat and in Needs attention.
  Concordia cannot surface a question the agent only reasons about without making a request;
  preventing unrequested actions such as commits belongs to provider permissions and the harness.
- Needs attention decision (2026-11-13): a Windows notification is emitted once per actionable
  item ID after Windows accepts delivery, and the told-item ledger persists across restarts. Native
  item IDs use the provider session and request identity, never a transient connection ID. The
  notification names the chat and waiting action, and its click captures the item before
  foregrounding, then selects that chat and focuses the item. Closed, finished, cancelled or
  resolved worker-native items do not contribute to the status. `/config` can disable Windows
  notifications while retaining the taskbar status. Test-controlled windows emit neither tray nor
  taskbar shell signals. When a restart restores an unfinished request without a live native
  question panel, it is represented as one actionable interrupted-request item until the user
  reviews or resumes it. Windows balloons are delivered serially while their shared tray click
  target is live, because the shell callback does not identify queued balloons.
- Reported regressions (2026-11-11, user reports, causes not yet inspected): attachment image
  previews render squished again; streamed reasoning text is joined without separators
  (for example, "Await again.All 107 scenarios passed").
- Private-notes operations finding: the reaction poller converts failed reaction reads into empty user
  lists. A local audit observed seven Discord HTTP 429 responses; bounded GET retries processed
  14 already-human-approved cards. That mitigation was used for catch-up only. A permanent
  polling fix remains outstanding; failed reads must not be reported as confirmed lack of approval.
- `/skills` needs clear folders and scope, on/off controls, instruction editing and application
  conditions. Provider targeting and project scope are separate controls. Root skills
  are always available across all providers and projects; their instructions apply when relevant,
  rather than accompanying every request unconditionally. Disabling a skill stops automatic use;
  users can still explicitly request it for a task. Every skill edit has an explicit propagation
  choice, regardless of its scope. All skill edits are permanent; there are no temporary edits.
  Propagation determines where an edit takes effect, not its lifetime. Without propagation,
  the current project permanently uses the revised skill while other projects retain their
  existing versions.
- Concordia groups chats and related resources by project (one repository) or warehouse
  (a project made of several repositories). Grouping organizes work; a chat may work across
  repositories or access other projects.
- Project labels are many-to-many: one conversation can appear under several projects. Labels
  are added automatically and can be added/removed manually. Fresh activity in a project restores
  its removed automatic label. Label membership does not itself define execution permissions.
- Reopening a project should restore the chats left open. Search should find chats by sent
  prompts. Project navigation groups chats rather than scattering them across the interface.
- Closing Concordia while work is active shows a confirmation with choices to stop and exit,
  cancel closing, or close the window and continue the work in the background.
- Background work that needs user input sends a Windows notification and waits. It does not
  automatically reopen the window.
- Closing a chat tab with active work shows stop-and-close, close-and-continue and cancel choices.
  Offer an optional remembered action to skip that confirmation on later tab closes; let users
  change the remembered behavior in settings. This preference concerns tab closing.
- After a restart, crash or reboot interrupts active turns, Concordia does not resume them on its
  own. It offers one user-initiated "continue where you left off" action that resumes all
  interrupted chats together, including chats whose last tool result is uncertain. Each resumed
  chat first reconciles its repositories and recorded actions before repeating any step, under
  the provider's existing approval rules.
- Any chat across Concordia can be explicitly referenced with `@`, regardless of project.
  Cross-chat content loads only when the user references that chat; do not automatically retrieve
  other conversations or inject a global chat memory. Stored history is distinct from active
  model context. This corrects the earlier overbroad automatic-continuity interpretation.
  An `@chat` supplies a source pointer for on-demand retrieval of relevant parts, rather than
  loading the entire referenced conversation into the prompt. The pointer is live: later reads
  can retrieve new messages added to the referenced chat; updates are not automatically injected.
- Chat deletion offers an explicit option to delete underlying provider history where supported,
  with a separate confirmation explaining what will be removed. Provider-history deletion is
  optional and must not be represented as available when the provider does not support it.
- Selecting a chat should select its relevant shell context. Users must be able to run ordinary
  PowerShell commands through Concordia. A chat's shell always opens in the folder where the chat
  started, including multi-repository chats; the user changes directory from there.
- Use a Photoshop-style arrangement of movable, resizable, dockable panels for the interface.
  Save layouts per chat by default; let users choose a layout shared across a project instead.
  Preserve underlying work when panels move, hide or restore.
- User-added tool panels are desirable, but scope and integration mechanism remain unresolved.
  The owner expects Blender interaction through Blender MCP and questioned the scope of embedding
  native applications. Native Blender window embedding is not an accepted requirement.
- The editor shown inside Concordia should be the user's preferred editor, including choices
  such as Vim or Neovim, retaining their configuration. Do not treat an external editor window
  as satisfying this in-app requirement. Onboarding includes editor selection, offers a ready-to-use
  default as an option and explains how to change editors afterward. Integration depth remains
  undecided; identical IDE integration across all editors is not established.
- For web development, agents should inspect/interact with components through headless tooling;
  users view the running application on localhost in their own browser. This workflow does not
  require an embedded browser preview inside Concordia. Tool integration details remain open.
- Offer an optional browser-to-chat connection: users select elements and attach comments in
  their browser, then send that context into a chosen Concordia chat. The owner selected this
  integrated workflow over relying solely on manual copy/paste or user-managed tooling.
  Compare existing tools before settling interaction details; no particular dependency is chosen.
- The owner's criticism of Lavish-style output concerns agent/harness design-system use,
  quality and token efficiency; it is context, not a Concordia policy requirement. Agent design
  methods belong to the configured harness, skills and provider. The proposed question about
  enforcing component reuse is withdrawn from this product interview. Direct visual property
  controls alongside agent feedback remain a tentative preference.
- Working navigation direction: a dockable Projects panel exposes the full tree with
  collapsible project/warehouse/chat groups, associated resources, activity indicators, title/sent-prompt
  search and an optional project filter. Keep the selected conversation visible beside it.
  Multi-project entries open the same chat. Detailed layout remains open.
- Goals are removed from Concordia: the per-project goal store, MCP change proposals and the
  landing-screen section. Objectives live in chats, repository files or the user's own tracker.
  Removal happens when feature work resumes.
- Replace the Windows taskbar loading indicator with a status dot that has three states: working
  while any agent runs, needs attention while an approval, question or interrupted chat waits on
  the user, and no dot when idle. Needs attention takes precedence over working.
- Apply the general edit rule throughout: change only what was requested; preserve unrelated work.
- Requested usage behavior: update credits, API usage and other usage displays automatically,
  including every listed `/model` item, without hover, selection changes or manual refresh.
  Distinguish source update cadence from UI refresh; unavailable or stale values must be explicit.
- `/model` must let users change model reasoning effort using the options supported by the
  selected provider/model. This is a requested capability, not a verified shipped behavior.
  Model and effort selections must persist across application restarts. New chats always start
  from a default set in settings; changing a chat's model or effort never changes that default.
- Do not require finishing or cancelling an active turn before switching conversations or
  selecting another provider/model. Navigation leaves the active turn running in its original
  conversation; provider/model changes take effect on the next prompt, preserving the active
  turn's original settings and ownership.
- Returning to unfinished work: the owner's priorities are understanding current status first,
  remembering where work was left second, and finding the right conversation third. These are
  user needs to address. Agents already report progress; Concordia makes those reports easy to
  find and revisit rather than adding its own progress tracker or summary mechanism.
- When an account reaches a usage limit, follow the user's configured handling policy. Whether
  work waits or switches accounts/providers is governed by that configuration.
- Requested provider coverage: Claude Code, Codex, Cursor Agent, Grok Build and Grok Bot. The owner
  explicitly wants both Grok products. Their integration contracts are unqualified; they are not
  shipped support.
- Shells: the MVP gives each chat its own user PowerShell session, created on first use, retained
  across tab switches and isolated from other chats. Optional named shared terminals that any chat
  can open (for example, a dev server) are the intended later addition, not MVP scope.
  Provider-owned agent execution remains separate. Do not imply live process restoration after an
  application restart.
- Note-taking: update this section as answers arrive, show the added/changed wording in chat and
  link to it. Review synthesized product wording with the owner before making it canonical.

### Coverage checklist

Owner instruction, 2026-11-09: review the whole product, including every feature, expected use
case, edge case, vision and scope. Ask one material decision at a time. Investigate implementation
facts directly instead of asking the owner to research them. Do not repeat settled questions.

Coverage remains open until each area has explicit decisions or an owner-approved exclusion:

- [ ] Purpose, audience, primary workflows, success criteria and scope boundaries.
- [ ] Projects, conversations, tabs, naming, search, history and account/provider switching.
- [ ] Memory, source retrieval, prompt context, attachments, handoff and context capacity.
- [ ] Models, routing, accounts, usage, limits, costs and provider capability differences.
- [ ] Goals, agents, delegation, worktrees, coordination and result integration.
- [ ] Skills, instructions, hooks, Harness, MCP, tools and slash commands.
- [ ] Files, editing, Git workflows, shell commands, terminal/native hosts and dockable panels.
- [ ] Approvals, ownership, cancellation, concurrent work, restart, recovery and uncertain effects.
- [ ] Configuration scopes, persistence, privacy, security, diagnostics and data retention.
- [ ] Installation, updates, migration, compatibility, accessibility and performance.
- [ ] Documentation, distribution, licensing, support and release acceptance.

Expand this coverage from the implementation inventory and the owner's workflows. For each
feature, settle its user and purpose, normal journey, failure/restart/concurrency cases, state
ownership, provider differences and observable acceptance. Distinguish implemented behavior,
desired behavior and explicitly excluded behavior. Keep one canonical home for each decision;
the coverage checklist points to those decisions rather than copying their text.

Harness, skills and hooks should support Concordia's requirements; their current limitations
must not silently define product scope. Inspect what is available, connected, trusted and actually
verified separately. Identify reusable improvements for shared Harness and product-specific
responsibilities for Concordia, without bypassing native consent or claiming unproven enforcement.
Their exact responsibility split remains an interview topic.

The prevention goal is ongoing consistency: connect settled requirements to focused flow checks,
keep documentation changes with behavior changes, and revisit affected decisions when contracts
change. An installed skill or hook alone does not establish this coverage. Feature implementation
remains paused during this interview and documentation phase.

Rewrite requirements:

- One clear user entry point and one canonical page per subject. Remove repeated explanations,
  superseded plans and copied historical journals from the working tree after preserving them in Git.
- Keep practical commands, source/approval/recovery boundaries and honest provider limitations.
- Retain licenses, notices, provenance, required policy files and the runtime changelog contract.
- Keep current verification concise; Git preserves the detailed pre-rewrite history at `6fea4a9`.
- Keep the paused [backlog](docs/plans/README.md), including the accepted dockable-panel request.
- Repair links and code/tooling documentation references when consolidating pages.

## Next build plan (2026-11-12, awaiting owner go)

### N5 opencode and Claude base-URL profiles - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/provider/{opencode,turn_acp,claude,turn}.rs` | ok: 1, 4, 6, 10 | Reuse the existing ACP runner and keep provider authentication, sessions, approvals and environment ownership at the provider boundary. |
| `crates/concordia/src/{store,accounts,connect,catalog,identity,models,native_input}.rs` | ok: 4, 5, 6 | Add one provider identity and account path so `/use`, model discovery and roles read the same provider kind. |
| `crates/concordia/src/provider/interaction.rs`, `crates/concordia/src/provider/turn_acp.rs` | ok: 2, 4, 9 | Map OpenCode's documented ACP permission and elicitation messages through the existing interaction owner; no test-only transport branch. |
| `crates/concordia/tests/install.rs` | ok: 4, 5, 9 | Keep the installer regression in the existing child-process test and provide its required isolated guard roots; no production test branch. |
| `docs/provider-opencode.md`, `docs/how-concordia-connects-providers.md`, `docs/README.md`, `README.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Extend canonical provider catalogs and record confirmed behavior and known limits in place. |

Decisions recorded on 2026-11-12:
- Roles: users set a default orchestrator and default models (with effort) per role, such as
  implementer, reviewer, re-reviewer and general helper. The `agents` tool applies these defaults
  when an agent asks for a role. A user's skill can describe what each role does and override a
  default for a task.
- BYOK: add opencode as an ACP provider (it covers DeepSeek, Qwen, Kimi, GLM, OpenRouter, Ollama
  and LM Studio through its own configuration), and let the Claude Code provider run with the
  user's own `ANTHROPIC_BASE_URL` environment profile. Concordia stores no keys. Codex custom
  providers are left out because Codex now accepts only the Responses API. Pending owner
  confirmation.
- Docs: Starlight with the vendored studio theme, published on GitHub Pages. The repository goes
  public after a history scan.
- Keyboard: Ctrl+O expands the last collapsed item in the last message; Ctrl+Shift+O expands
  every item in it.

Lead tasks (not delegated):
1. Apply the four Harness guard approvals once the owner has run them, add opening-paragraph
   guidance to the writing skill, and commit `feat/writing-skill`.
2. Make `harness map --install-task` create a hidden task.
3. Register `~/.codex-2` and `~/.codex-3` as ordinary accounts before the legacy code goes.
4. Run live verification with cheap models (Cursor `auto`, Claude `haiku`, Codex
   `gpt-5.6-luna` on low effort; at most about 10 turns per item) for questions and Needs
   attention, command passthrough, usage-limit continuation, Grok Build and, after it lands,
   opencode.
5. Run the full suite outside the sandbox after every worker and send failures straight back to
   that worker.

Worker steps, in order, each gated on a build and a passing full suite:
1. Delete the legacy compatibility code.
2. Test hygiene: stop tests rewriting `docs/img`, and add deterministic regression tests for
   fixes from 2026-11-11 and 2026-11-12 that lack one.
3. Roles and defaults: the settings, a roles picker, and role-aware `agents` requests.
4. Agents view matching the owner's reference screenshot:
   - A numbered list of agents, each row showing a status icon, role, model, effort, context
     bar and turn count.
   - A detail pane showing role, model and effort, tokens used against the limit with a bar,
     the percentage of the compaction reference, and tokens remaining.
   - The detail pane also lists each task with its duration, origin (for example "From
     Orchestrator") and status.
5. opencode provider over ACP, plus the Claude Code base-URL environment profile.
6. Keyboard and tabs: Ctrl+O and Ctrl+Shift+O expansion, and drag and keyboard tab management.
7. Docs and release:
   - The README opening and a Starlight content pass, with the publishing workflow enabled.
   - Checksums and build attestations on releases.

Going public:
- Scan the full Git history for secrets and personal data.
- Check the license and notices.
- After the owner's final go, make the repository public and enable Pages.

## This slice

### Y3 review-2 correction - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/provider/failure.rs` | ok: 1, 3, 4, 5, 10 | Extend the existing structured provider signal visitor to nested ACP HTTP 429 fields; no prose fallback or new classifier. |
| `crates/concordia/src/tui/continuation.rs` | ok: 1, 4, 5, 6, 10 | Keep reset waiting in the continuation owner and bind its existing target to provider, account, and model. |
| `crates/concordia/src/tui/app/pickers.rs` | ok: 1, 4, 5, 10 | Correct the existing disabled-row predicate at the picker construction owner for cached zero usage. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 10 | Record the correction and advance the user-facing build identity. |

### Y6 ACP compaction review correction - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/tests/support/fake_provider.rs`, `crates/concordia/tests/tui_journeys.rs` | ok: 1, 4, 5, 9, 10 | Extend the existing ACP fixture and native journey to emit and observe one completed compaction with a duplicate event; no production test-only branch or second connection owner. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs`, `PLAN.md` | ok: 5, 10 | Record the review correction and advance the required build identity in existing metadata owners. |

### Q10 Concordia-wide MCP registry - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/mcp_registry.rs` | ok: 1, 4, 5, 6, 9, 10 | One state-owned registry validates references and builds provider launch projections; no proxy is added without a complete client owner. |
| `crates/concordia/src/provider/{turn,turn_acp}.rs`, `commands.rs`, `tui/app/slash.rs` | ok: 1, 4, 5, 7 | Reuse existing launch and slash-command owners for Codex and ACP delivery and keep provider homes read-only. |
| `docs/mcp.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Document shipped boundaries and missing Claude/proxy work without claiming unsupported delivery. |

### Q9 provider session context - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/provider/{standing_context,turn,claude,codex,turn_acp}.rs`, `crates/concordia/src/{mcp,tui/run,delegate}.rs` | ok: 1, 2, 4, 5, 6, 9, 10 | Keep one deterministic context renderer and deliver it through existing provider launch/session owners. Read tool names from the MCP registry and use tagged fallback text only when the provider lacks a durable channel. |
| `crates/concordia/tests/control_plane.rs`, provider unit tests | ok: 5, 9, 10 | Prove launch arguments, bounded context construction and the no-duplicate tag rule at existing provider test owners. |
| `docs/provider-session-context.md`, `docs/README.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Document the implemented channel per provider, live checks and build identity. |

### Q8 chat repair - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/{chat_cli,sessions,store,doctor}.rs` | ok: 1, 2, 4, 6, 10 | Keep diagnosis in sessions, persistence in store and the single CLI mutation in chat_cli; remove only damaged task links or empty tasks after a native-message check. |
| `crates/concordia/src/tui/app/pickers.rs`, `crates/concordia/src/main.rs` | ok: 1, 4, 5, 9 | Expose the existing repair owner through the existing picker and command table; no second UI repair implementation. |
| `crates/concordia/src/**tests**`, `CHANGELOG.md`, `docs/continuity.md`, `crates/concordia/src/version.rs` | ok: 5, 9, 10 | Add deterministic synthetic coverage and record the shipped command and boundary in canonical documentation. |

### Q7 Windows attention notifications - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/{needs_you,store,config}.rs` | ok: 1, 4, 6, 10 | Keep item identity and the durable told-item ledger at existing state owners; do not add a notification service. |
| `crates/concordia/src/tui/{run,settings,appearance,state,pickers,input_stream,display_backend,native_window}.rs` | ok: 1, 4, 5, 6, 9, 10 | Route status, settings, native signals and click focus through their existing owners; test coverage uses the same native event path. |
| `README.md`, `docs/{config,tui-states}.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Record the implemented notification boundary, setting and build identity in canonical documents. |

### N9 usage-limit continuation regression - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/provider/failure.rs` | ok: 1, 3, 4, 5, 10 | Keep structured quota classification at the provider failure boundary and extend only the missing Claude code used by its fixture. |
| `crates/concordia/src/tui/continuation.rs`, `crates/concordia/src/tui/demo_gifs.rs` | ok: 1, 4, 5, 9, 10 | Repair stale deterministic callers to provide the structured signal required by the shipped contract; do not restore prose matching. |
| `crates/concordia/tests/support/fake_provider.rs`, `crates/concordia/tests/fixtures/streams/**` | ok: 1, 4, 5, 9, 10 | Make provider fixtures carry native limit codes so the existing end-to-end paths prove structured detection. |
| `PLAN.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Record the decision, regression correction and build identity in the existing metadata owners. |

### Q1b full-suite closeout - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/harness/command.rs` | ok: 1, 4, 5, 6, 9, 10 | Preserve guided no-argument behavior and update command tests to exercise explicit mutation targets. |
| `crates/concordia/src/provider/stream_cards.rs` | ok: 1, 4, 5, 10 | Enforce the existing compact-row bound at the card-summary owner; keep full provider output on the card. |
| `crates/concordia/src/tui/{native_agents.rs,tests/draw.rs}` | ok: 1, 4, 5, 9, 10 | Align stale assertions with the recorded Agents row and Ctrl+O/Ctrl+Shift+O decisions. |
| `docs/img/*.svg`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Refresh only synthetic screenshot baselines affected by the shipped render contract and record the build identity. |

### Q1 render lane - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/{catalog,models,usage}.rs`, `crates/concordia/src/tui/{icons,transcript,tests}.rs` | ok: 1, 4, 5, 10 | Repair the merged opencode types and test fixtures at their existing owners. No new abstraction or compatibility path. |
| `crates/concordia/src/tui/{questions,native_window,run,app/turn}.rs`, `crates/concordia/tests/tui_journeys.rs` | ok: 1, 4, 5, 6, 9, 10 | Complete the existing popup, retained-render, activation, image and tab behavior with deterministic regressions in the owning UI tests. |
| `crates/concordia/src/process_ui.rs` | ok: 1, 4, 5, 9 | Keep one existing child-process owner and its source scan as the regression boundary. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 10 | Record the user-facing render changes and advance the required build stamp. |

### N4 Agents view follow-up - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/tui/native_agents.rs` | ok: 1, 4, 5, 6, 10 | Keep provider-event projection and rendering in the existing Agents owner; correct compact status glyphs and visible row numbering without adding state. |
| `crates/concordia/src/tui/session_ui_tests.rs` | ok: 5, 9, 10 | Update the existing regression to assert the shipped compact-row contract and retain coverage for live picker refresh. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 10 | Record the user-facing correction and advance the build stamp. |

### N2 test hygiene and regression tests - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/tui/screenshots.rs` | ok: 1, 4, 5, 10 | Keep screenshot generation in its existing test owner; compare through a temporary directory and require an explicit regeneration environment variable for committed-image writes. |
| `crates/concordia/src/tui/native_window.rs` | ok: 1, 4, 5, 9 | Apply the existing test-control state at the native HWND boundary so controlled windows stay hidden, nonactivating and absent from the taskbar while using the same renderer and input path. |
| `crates/concordia/src/tui/run.rs`, `crates/concordia/src/tui/ux_tests.rs` | ok: 1, 5, 9, 10 | Add focused regressions beside the existing owners for Ctrl+T, palette history and reasoning chunk boundaries; do not add a test-only production interface. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 10 | Record the user-facing test-driver and regression coverage change and advance the existing build stamp. |

### W15 documentation site scaffold - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `site/**` | ok: 1, 4, 6, 7, 10 | Add one isolated Starlight publication layer, keep the Rust crate independent, and derive pages from current canonical docs. |
| `site/src/styles/studio/**`, `site/scripts/sync-studio.mjs` | ok: 1, 5, 6, 10 | Vendor only the studio token and CSS needed by Starlight, with one refresh owner and pinned provenance. |
| `.github/workflows/docs.yml` | ok: 1, 4, 10 | Add one GitHub Pages build and deployment path without enabling Pages or changing release workflows. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs`, `PLAN.md` | ok: 5, 10 | Record the user-facing documentation scaffold and its build identity in the existing metadata owners. |

### W9c Needs attention app-server journey - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/tests/support/fake_provider.rs` | ok: 1, 4, 7, 9, 10 | Extend the existing app-server fixture with one concrete question-then-approval sequence and a bounded filesystem completion gate; no production test branch or second provider simulator. |
| `crates/concordia/tests/tui_journeys.rs` | ok: 1, 4, 5, 6, 9, 10 | Reuse the native control channel, existing snapshot/status state and existing journey session; the new assertions exercise the user flow rather than duplicate UI state. |

### W14 Grok Bot two-way integration - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/grok_bot.rs` | ok: 1, 2, 4, 5, 6, 8 | One concrete routine/callback owner validates both HTTP trust boundaries; secrets stay in Credential Manager and no polling or duplicate MCP surface is added. |
| `crates/concordia/src/mcp.rs`, `tui/native_agents.rs`, `tui/app/pickers.rs` | ok: 4, 5, 6, 10 | Reuse the existing `agents` record and Agents-tree projection; Grok Bot is one provider branch and `/use` remains the account/integration picker owner. |
| `README.md`, `docs/provider-grok-bot.md`, `docs/README.md`, `CHANGELOG.md`, `version.rs` | ok: 5, 10 | Correct the two stale safety promises and document the implemented callback boundary and limits. |

### W11 provider command passthrough - anti-slop gate

| path | tendency_check | justification |
| --- | -------------- | ------------- |
| `crates/concordia/src/provider/mod.rs`, `tui/palette.rs` | ok: 1, 4, 5, 6, 10 | Keep provider command metadata and transport support at the existing provider/palette owners; no second command registry or generic adapter. |
| `crates/concordia/src/provider/turn_appserver.rs`, `turn_acp.rs` | ok: 1, 2, 4, 5, 7 | Invoke only documented RPC/prompt methods over the existing verified session connection; retain explicit thread/session identity and bounded reads. |
| `docs/command-audit.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Replace the stale transport premise and record confirmed versus unverified behavior in the canonical ownership table. |

### W3e final delegation cleanup - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/{delegate,delegate_recovery,config,capacity}.rs` | ok: 1, 4, 10 | Delete compiled-out delegation/routing/crew code and leave one active owner per lifecycle. |
| `crates/concordia/src/store.rs`, `tui/workers.rs`, `delegate_dependencies.rs` | ok: 4, 6, 10 | Remove persisted worker registration state; active jobs use explicit account identity and native catalog checks. |
| `crates/concordia/src/tui/workspace_scope.rs`, `tui/app/state.rs`, `commands.rs` | ok: 1, 4, 10 | Remove the public scope surface while retaining internal main-turn admission used by run/checkpoint owners. |
| `README.md`, `docs/`, `CHANGELOG.md`, `version.rs` | ok: 5, 10 | Update canonical behavior docs and release metadata; preserve historical plans as plans. |

### W3f Ctrl+G Agents projection - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/tui/native_agents.rs` | ok: 1, 4, 6, 10 | Keep the existing Agents projection as the owner; treat unmatched provider sender IDs as root rows and rebuild it from the restored transcript. |
| `crates/concordia/src/tui/window_session.rs` | ok: 4, 6 | Rehydrate the existing projection immediately after the existing saved-message owner restores a tab; no second store is added. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Record the user-facing Ctrl+G correction and bump the build stamp. |

### W4b table rows and system messages - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/tui/markdown_tables.rs` | ok: 1, 4, 5, 10 | Add the missing row rule in the existing Markdown table renderer; no new layout owner. |
| `crates/concordia/src/tui/draw/mod.rs`, `app/turn.rs` | ok: 1, 4, 5, 10 | Style existing system blocks and revise their cancellation wording at the current message owner. |
| `crates/concordia/src/tui/queue_tests.rs` | ok: 9, 10 | Align the stale assertion with the existing recovery message; do not disable the failing test. |
| `crates/concordia/src/version.rs`, `CHANGELOG.md` | ok: 10 | Record the user-facing rendering and wording change under the required release metadata. |

### W2d small removals and settings - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/integration.rs`, `config.rs`, `connect.rs` | ok: 1, 4, 6, 10 | Delete provider-home awareness ownership and its setting; MCP registration remains the sole integration owner. |
| `crates/concordia/src/capacity.rs`, `ui/app/slash.rs`, `config.rs` | ok: 1, 4, 10 | Remove the `/context` command and compaction overrides while retaining only the requested Codex proactive-rollover setting. |
| `crates/concordia/src/feed.rs`, `workspace_replace.rs`, command and MCP callers | ok: 1, 5, 9, 10 | Remove obsolete surfaces and their tests rather than replacing them with compatibility wrappers. |
| `crates/concordia/src/main.rs`, `chat_cli.rs`, `threads.rs`, `sessions.rs` | ok: 1, 4, 7 | Make `chat` the single public history entry point and keep the existing bounded readers as implementation owners. |
| `crates/concordia/src/config.rs`, `ui` time-header callers | ok: 1, 6 | Add one persisted setting at the existing config owner, defaulting on, with no duplicate UI state. |
| `README.md`, `docs/`, `CHANGELOG.md`, catalogs, `version.rs` | ok: 5, 10 | Update canonical catalogs and shipped-behavior docs with each deletion/settings change. |

### Documentation and direction - anti-slop gate

| Paths | Checks | Ownership |
| --- | --- | --- |
| Root Markdown and docs Markdown | 1, 4, 6, 10 | Consolidate current facts; remove duplicate pages rather than add another documentation layer. |
| Documentation links in code and scripts | 5, 9 | Repair references only; feature implementation remains paused. |
| Legal, notices, policy and generated files | 2, 3 | Preserve required contracts and provenance; inspect dependencies before removing anything. |

Acceptance: current docs are concise and consistent; all local references resolve; documentation
checks pass; purpose and scope reflect the owner's answers. Native provider ownership, recovery,
approval and privacy rules in [AGENTS.md](AGENTS.md) remain in force throughout the rewrite.

### W0 native-window journey driver - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/tui/native_window.rs` | ok: 1, 2, 4, 5, 6, 7, 8, 9, 10 | One opt-in, loopback-only test channel owned by the native message thread; no production listener or alternate renderer. |
| `crates/concordia/src/tui/display_backend.rs`, `crates/concordia/src/tui/run.rs` | ok: 4, 5, 6, 9 | Forward test state from the existing backend and app loop; no duplicate UI state owner. |
| `crates/concordia/tests/tui_journeys.rs` | ok: 1, 5, 7, 9, 10 | Replace PTY transport with the requested native driver, preserve journey assertions, and omit only `/edit`. |
| `docs/tui-states.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 1, 5, 10 | Document the shipped boundary and bump the required user-facing build stamp. |

### W3 MCP agents and delegation - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/mcp.rs` | ok: 1, 4, 5, 6, 10 | One `agents` surface owns account discovery and delegated-agent requests; removed tool registrations and legacy tests do not remain public. |
| `crates/concordia/src/tui/input_dispatch.rs`, `crates/concordia/src/tui/workers.rs` | ok: 1, 4, 10 | Reuses the existing delegation lifecycle and makes Ctrl+G the single Agents-view toggle; no second scheduler or routing policy. |
| `README.md`, `docs/README.md`, `docs/mcp.md`, `docs/delegation.md`, `CHANGELOG.md` | ok: 1, 5, 10 | Updates canonical catalogs and behavior references in place; does not add a duplicate guide. |

### W4 daily bugs and small interface fixes - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/tui/transcript.rs`, `crates/concordia/src/tui/draw/mod.rs` | ok: 1, 4, 5, 10 | Keeps transcript ownership in the existing view, bounds rehydration, and adds one visual scrollbar without a second scroll model. |
| `crates/concordia/src/tui/run.rs` | ok: 1, 5, 8, 10 | Removes the synchronous recovery write from new-tab creation; normal persistence remains the owner of recovery retries. |
| `crates/concordia/src/tui/app/turn.rs`, `crates/concordia/src/delegate.rs` | ok: 1, 4, 7, 10 | Adds separators only at reasoning-delta boundaries, preserving assistant delta semantics and existing provider ownership. |
| `crates/concordia/src/tui/app/state.rs`, `crates/concordia/src/tui/pointer.rs` | ok: 1, 4, 6, 10 | Derives tab status from existing chat state and uses the existing pointer owner; no duplicate status store or interaction layer. |
| `crates/concordia/src/tui/queue.rs` | ok: 1, 2, 9, 10 | Reuses the existing queue editor and clipboard writer; Ctrl+C copies its text instead of changing queue state. |
| `crates/concordia/src/tui/native_window.rs`, `crates/concordia/Cargo.toml` | ok: 1, 5, 10 | Applies the active theme's existing RGB value through the Windows caption API; no custom window host or theme source is added. |

### W5j remaining UI journeys - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/tests/tui_journeys.rs` | ok: 1, 4, 5, 9, 10 | Delete journeys for removed `/delegate`, `/workers`, `/recover`, `/context`, `/memory`, standalone preview and ConPTY-only behavior; retain the sandbox-only file replacement journey and adjust the native driver assertions in place. |

### W5k full test-suite closeout - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| `crates/concordia/src/**`, `crates/concordia/src/tui/**`, `README.md` | ok: 1, 4, 9, 10 | Classify existing failures against PLAN decisions; remove tests for removed goals/delegation/recovery surfaces, update stale expectations, and fix only confirmed product regressions. No new tests or compatibility layers. |

### W6a recovery, mode and connect consolidation - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/commands.rs`, `src/tui/app/{state,slash,pickers}.rs`, `src/tui/recovery_ui.rs` | ok: 1, 4, 10 | Remove public recovery commands and the `/use` continuation row; retain the existing continuation state only for inline retry/account handoff behavior. |
| `crates/concordia/src/store.rs`, `src/config.rs`, provider planners, `src/tui/**`, tests | ok: 1, 4, 5, 10 | Rename the permission enum and serialized/UI vocabulary to `edit` at the existing trust owner; no compatibility alias. |
| `crates/concordia/src/commands.rs`, `src/harness/**`, `src/tui/**` | ok: 1, 4, 10 | Make `/harness` the sole connect/status/verify/disconnect action owner; `/skills` and `/hooks` remain views and `/skills` keeps switches. |
| `crates/concordia/src/user_commands.rs`, `src/doctor.rs`, `src/tui/**` | ok: 1, 4, 6, 10 | Keep one bounded shortcut scan and report provider-command shadows without adding a second command registry. |
| `README.md`, `docs/**`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Synchronize catalogs and user-facing documentation with the code and record the shipped change. |

### W10 Grok Build provider - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/provider/{grok,turn_acp,turn}.rs` | ok: 1, 4, 6, 10 | Reuse one ACP runner for Cursor and Grok; keep exact session identity and native interaction ownership in the provider transport. |
| `crates/concordia/src/{accounts,connect,identity,models,catalog,store}.rs` | ok: 1, 2, 4, 6 | Add only the Grok account/home and bounded metadata seams; login detection checks markers without reading token contents. |
| `crates/concordia/tests/{images.rs,live_providers.rs,support/fake_provider.rs}` | ok: 5, 9, 10 | Exercise the existing ACP fixture path and retain real-provider tests ignored; never add a fake-only production branch. |
| `README.md`, `docs/{README.md,how-concordia-connects-providers.md,provider-grok-build.md}`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Update the canonical provider catalog and record confirmed versus unverified behavior. |

### W9b taskbar status and Windows notifications - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/tui/native_window.rs`, `tui/display_backend.rs`, `tui/theme.rs` | ok: 1, 4, 5, 6, 9 | Keep status calculation in the existing runner and native HWND as the sole owner of taskbar, tray and icon handles; no second status store or test-only production branch. |
| `crates/concordia/Cargo.toml` | ok: 1, 5 | Enable only the Windows COM surface needed by the existing native window. |
| `README.md`, `docs/tui-states.md`, `CHANGELOG.md`, `crates/concordia/src/version.rs` | ok: 5, 10 | Document the shipped native APIs and user-visible boundary in canonical docs with the required build stamp. |

### W12e final three failing tests - anti-slop gate

| path | tendency_check | justification |
| ---- | -------------- | ------------- |
| `crates/concordia/src/mcp.rs`, `crates/concordia/src/tui/persist.rs` | ok: 1, 4, 5, 9, 10 | Keep redaction at the MCP persistence boundary and update the security test to the sole live `agents` surface; no legacy tool compatibility is restored. |
| `crates/concordia/src/tui/app/state.rs` | ok: 1, 4, 6, 10 | Use the existing per-App Needs-attention queue as the picker owner; retain the global registry only for publication and status aggregation. |
| `CHANGELOG.md`, `crates/concordia/src/version.rs`, `PLAN.md` | ok: 5, 10 | Record the shipped security and paused-queue focus corrections with the required build stamp and scope ledger. |
