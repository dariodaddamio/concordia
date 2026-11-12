# Concordia work plan

Current phase: cleanup closeout, documentation rewrite and product interview (2026-11-09).

## Current instruction

Feature roadmap execution is paused, including the dockable panels. Finish cleanup, then
rewrite the documentation while using grill-me to settle Concordia's purpose and direction.
Do the factual consolidation and product interview together. Publication remains on hold.

## Cleanup closeout

The audited implementation and its previous documentation are preserved in local commit
`6fea4a9`. This is a preservation checkpoint, not full release qualification.

- One worktree remains. The current work branch and `main` are retained. No obsolete build cache,
  disposable repository archives or unused installer images were found in the final inventory.
- The installed build is 2026-11-09g. Its executable identity and scoped checks are recorded in
  [verification status](docs/e2e-status.md).
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
    rendering; Brain and Harness support through the pluggable memory/harness extension point;
    providers Claude Code, Codex, Cursor, Grok Build and Grok Bot; concise GitHub Pages
    documentation.
  - Intended direction: fully dockable panels with per-chat layouts (MVP: show, hide and resize
    in one shared layout); embedding the user's own editor; Asset Lab; browser-to-chat feedback;
    custom tool panels; shared named terminals; `/skills` editing and propagation (MVP: list with
    on/off); chats in several projects at once (MVP: one project per chat); provider-history
    deletion.
  - Out of Concordia: Goals; the Brain reaction-poller fix (Brain repository backlog).
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
    case is research, adding notes to Brain and social-media growth for Goodman and Poolaris,
    not coding. The integration mechanism is unresolved. Follow-up research (official
    docs.x.ai pages, partly unverified): Grok Bot has no documented API, CLI, webhook or ACP. It
    is a confirmed MCP client: its desktop app adds custom remote HTTPS connectors, reached from
    xAI's cloud machine, so a Brain server would need a public, authenticated endpoint. Other
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
    the callback (no chat history, Brain or other tools); every call needs a Concordia-generated
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
  Pages, themed with the Opifex Studio theme. Constraint found: `@dariodaddamio/opifex` 1.3.0 is
  MIT-licensed but published with restricted access on GitHub Packages, and Opifex is not yet
  public, so a public Concordia docs build cannot install it without credentials. Decision:
  Opifex stays unpublished until after Concordia ships (it is the owner's next project), so the
  docs vendor the Opifex Studio theme (tokens, CSS and any components used) at a pinned Opifex
  version with a sync script, keeping the look consistent with Opifex Studio and the portfolio.
  Switch to the published package once Opifex is public. It must also explain Windows Defender and SmartScreen warnings
  for the unsigned executable (Hermes documents a similar case on GitHub; not yet reviewed).
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
  and memory systems such as Brain (connect and verify per provider, skill, pinned decisions,
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
  (such as Brain) or a harness into Concordia through a defined extension point. Concordia owns
  that extension point; a specific integration's setup belongs to that integration, not to
  Concordia's core. This does not authorize publishing or changing vault data.
- Question and approval requests must work reliably across every supported provider, despite
  differing native mechanisms (native question tools, Cursor ACP extensions, the MCP question
  bridge). Every request an agent makes must reach the user in the chat and in Needs attention.
  Concordia cannot surface a question the agent only reasons about without making a request;
  preventing unrequested actions such as commits belongs to provider permissions and the harness.
- Reported regressions (2026-11-11, user reports, causes not yet inspected): attachment image
  previews render squished again; streamed reasoning text is joined without separators
  (for example, "Await again.All 107 scenarios passed").
- Brain operations finding: the reaction poller converts failed reaction reads into empty user
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

## This slice

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
