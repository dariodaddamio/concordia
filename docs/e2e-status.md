# Verification

## 2026-11-18b takeover qualification

Debug candidate: **2026-11-18b, 40897e1+2214**, SHA-256
`39ccf69428382cd99e6c1d6d7eb86b784a06a637ccb19abcdc73bc22a91d0cf1`.
Installed 15c remains unchanged. No provider-home registration, trust or authentication changed.

Codex app-server steering is bound to the active thread and expected turn. First Enter arms the
choice, second Enter steers, Tab queues. Only plain text is steered. Acceptance clears only that
draft; refusal, a mismatched acknowledgement, connection loss and late completion retain it without
automatic retry. Key hints stay in the footer. The pre-prompt fallback also covers a failed
`initialized` notification; account/workspace/policy refusals still stop the turn.

Checks on this source: 11 existing `tui::ux_tests` cases and six `cursor_usage`-filtered cases passed;
existing `fake_provider` (23), `images` (one) and `provider_runner` (two) integration cases passed.
The existing ignored native-window workspace/send-now journey passed with added steering scenarios:
Tab queues, Enter arms, accepted steering clears, refusal retains, and exactly one steer/start is
recorded. All providers in that journey are synthetic. No live Codex/Claude model turn was run,
consistent with the owner's Cursor-only live qualification preference. No new test case functions
were added. The original full 18a receipts below remain dated evidence, not an 18b full-suite run.

Cursor 2026.11.17-f532c45 accepted CSI-u Return and returned a parsed local usage table without a
model prompt. A preceding LF probe timed out; it is not counted as successful. The temporary probe
source was removed. Changed/unresponsive screens now explicitly report usage unavailable. Existing
raw-input fixture checks cover table parsing, timeout, missing program and process cleanup.

The focused qualification windows closed. Distribution remains at the owner's requested end of
the combined work, after the eight Habena protected fixes receive separate human approval/application.
Native Habena pending-pause execution/trust review remains open. No universal parity claim is made.

## 2026-11-18 mid-turn question default WIP

Earlier debug snapshot reports **2026-11-18a, 40897e1+0832**. Installed 15c is unchanged.
Claude defaults to its existing question-capable stdio adapter; explicit saved off remains
respected. Owned launches declare bounded host/provider/question capability for opt-in Habena
context; ownerless launches clear inherited declarations. The declaration is not native hook proof.
No fresh Codex delay was reproduced; its ordinary owned app-server route was already present.

Checks run on this source: `cargo test --locked -p concordia --lib provider::standing_context::tests`
(one existing case) and `cargo test --locked -p concordia --test images` (one existing integration
case), both passed. These qualify deterministic context/protocol fixtures, not a live Claude/Codex
turn or the changed default UI flow. Those tiny live checks and Habena distribution/native review
remain pending. Do not transfer the following 17b hash or live proof to the newer candidate.

Owner decisions on 2026-11-18: an open question pauses queued prompts, and no question is dismissed
by a typed instruction. `on_turn_done` pauses the queue again; the journey asserting the old dismissal
now asserts the question stays open. Restored questions match earlier notifications by turn identity,
so `outstanding_attention_is_not_notified_again_after_restart` passes again. `ask_user` results add
`structuredContent.questionState` with a declared output schema. Checks on this source: full library
1,022 passed; 14 integration suites passed (install skipped, unchanged); default journeys 3 passed;
live Cursor `question_tool_reaches_the_popup_broker` and `every_provider_asks_through_the_popup` passed.
A palette case now uses a non-colliding `/audit` fixture, since 17a resolves provider collisions on submit.

Afternoon additions on 2026-11-18, same uncommitted tree: header model/effort dropdown (written by a
Haiku subagent, then made compact and extended to the effort follow-up), Codex `/fork` adoption, Codex
update fallback (protocol failures before the prompt only; identity and policy refusals stand), and Claude
slash-command catalogs from the stdio init event. Checks: full library 1,022 passed; `images`,
`provider_runner` and `fake_provider` suites passed; 177 picker/draw/input cases passed; throwaway probes
(deleted) proved fork, fallback and the Claude catalog against the fake providers; the dropdown, toggle,
outside click, Escape and effort follow-up were checked in the dev window. The full native journey run on
this source was 41/50; the nine failures were the journey setup not pinning Claude's print transport after
the 18a default changed (fixed in setup) plus one window-lookup flake while the dev window was open; all
nine pass on rerun. Cursor usage readings time out on Cursor 2026.11.17 (plan C13).

The interrupted `doctor.rs` account-count edit is now complete. Its proof is dated 2026-11-16
(Claude session, 15:37 UTC): 30 existing `doctor` library cases passed and the native window showed
`4 of 6 account(s) signed in`. `doctor.rs` and `accounts.rs` are unchanged since, so that proof is reused.

Changed-document and scoped portable/citations/file-catalog/prose checks passed. The process check
used the locally built Habena CLI; this Cargo-only checkout has no local `scripts/check-process.mjs`.
Whitespace and exact stale-current-candidate sweeps were clean.

## 2026-11-18 durable clarification qualification

Previously qualified source candidate: **2026-11-17b**, `40897e1+0642`, SHA-256
`d848e8101c104d12b2c7cb1575283102d73a3de4e3b6df62c669ccac38a36d22`.
Installed 15c is unchanged. The original doctor edit remains unmodified.

The owner chose task-level clarification ownership: explicitly selecting another agent/account
transfers pending questions and eventual answers, retaining original provenance. Native approvals
remain live/run-bound. The source uses the existing pane recovery record and FIFO reply queue;
it adds no global plugin registration, hook-trust grant or authentication change.

Observed in the native Windows app with isolated synthetic accounts and the existing fixture:

- Cursor Standard prepares a local plugin; two bundles receive a saved acknowledgement only
  after the question snapshot is durable. Both remain after the provider finishes.
- Mouse multi-select, a four-question bundle, completed partial answers and an unfinished note
  survive a forced owned-process exit/restart and tab switching. An independent follow-up
  preserves the pending questions.
- Explicit Cursor-to-Codex selection supplies pending clarification context to the replacement.
  The completed original bundle reaches that replacement once through FIFO while the second
  bundle remains visible. This observes enqueue/delivery in the fixture, not exactly-once native
  tool execution after every possible crash.
- An oversized recovery record makes reply submission fail visibly without removing the
  question or dispatching the reply. A separate bridge probe receives two explicit not-saved
  acknowledgements and admits no unsaved question. The synthetic oversized file was restored.
- A question record with an empty task identity stays byte-identical through startup and a dirty
  persistence tick. Source review also verified that failed adoption clears/suppresses its panic
  mirror, preventing that path from overwriting the rejected original.
- Starting a new conversation archives its old pending question in recoverable task state and
  retains the clean file pane. It does not display that question in the unrelated new chat.

The main recovery/handoff journey first ran on 17b+0628; the save-failure, invalid-identity and
file-pane checks ran on the later candidates carrying their respective fixes. Existing checks:
13 question cases, five persistence cases, six recovery cases and eight launch cases pass. The
question permission case now also exercises conversational suspension and approval exclusion;
the existing launch case covers a plugin path with spaces/metacharacters and exactly one
PowerShell adapter. No new unit/integration cases were added. A window-session test filter
matched zero cases and provides no evidence.

The real native-app check first needed **native Cursor workspace review**. The first launch
attempt exposed an unsafe-option batch refusal, fixed through the existing beside-script
PowerShell adapter. The next candidate reaches Cursor, which refuses the untrusted scratch
directory before model inference and keeps the draft. The owner explicitly approved that exact
scratch workspace. Its native trust option was then applied through `/terminal native`; no trust
file, hook approval or authentication was edited by the agent. Cursor's UI remained at Trusting
workspace, so the owned native shell was closed and a fresh normal launch checked the saved
provider state. That launch called the local question tool and rendered the durable question in
the actual app. Two tiny Auto turns passed: the first called `ask_user` and completed while the
question remained; the answer was submitted more than 90 seconds after durable observation.
The second turn acknowledged option A and completed. The task's native session identity stayed
unchanged, the completed question cleared, and the app returned to idle. This qualifies the
named default-Cursor native-window question/follow-up journey, not every native provider flow.
One driver attempt reached the local shell instead of the composer after closing the native
pane; it produced command-not-found and consumed no model turn. The corrected driver verifies
chat focus before submission. Earlier fixture/save failures and native trust refusals remain
separate from the eventual live result.

Remaining limits: no universal all-provider or release qualification; pending-question read-only
guidance is not Habena's proposed mutation hook. Persistence and routing do not establish model
obedience. The installed Habena package still lacks its separately verified source gate fix.

## 2026-11-17 ACP discovery qualification

ACP qualification candidate: **2026-11-17a**, `40897e1+0239` (dirty debug build).
The installed application remains **2026-11-15c**. No installation, publication, authentication
or hook-trust change occurred. The interrupted `doctor.rs` account-count edit is preserved.

Candidate executable SHA-256:
`7be5a681149009d6e1ceea6dd412335bc95027e8c2e865a98dc4465724d36128`.

Existing palette checks pass: seven cases, zero failures. The native Windows control channel
drove the actual candidate with isolated synthetic accounts and the existing fake-provider
engine. Observed scenarios: advertisements before session response, idle retention, foreign
session exclusion, authoritative empty/removal, static fallback suppression, native/custom
collision, exact command and argument delivery, builtin alias ownership, rejected session load,
reconnect and actual account switching. A gated owner transition showed the local suggestion,
dismissed it on an ownership change, then exposed the provider action only after fresh input.
The stale shared-choice probe selected Cursor by its displayed identity before removal and
observed refusal with nothing sent from that choice.

Early probes assumed a fixed header row, sufficient fixed delays and a one-account picker.
Those assumptions failed after tab/account state changed. Corrected observations used the
provider header and selected account identity; the owner-transition sequence was inspected
after its gated event. These probe failures are not recorded as product defects or hidden
behind an aggregate pass count. No new unit/integration cases or E2E framework were added;
the existing fixture engine gained command advertisements and gated updates.

The existing real Cursor `text_turn_then_resume_the_same_session` check passed with two tiny
Auto-model turns on the earlier 17a candidate, before final UI-only selection/copy refinements.
That establishes the named adapter continuity path; it does not prove real-provider catalog
rendering, native-window questions, every provider transport or the final binary's entire UI.
No Claude or Codex inference ran for this slice.

Build, changed-source rustfmt, documentation, applicable process checks and the OSS privacy
gate are scoped separately. Workspace-wide rustfmt still reports differences in untouched
`cursor_usage.rs` and `tui/queue_tests.rs`. The installed process file-catalog check fails because
this host-only Cargo project has no catalog. Habena's canonical root and packaged-preset source
now respect explicit `tooling.packageTooling: false`; ten bounded profile/catalog probes and the
canonical preset's full portable/citations/file-catalog/prose run against Concordia pass.
The installed package has not received that source fix. No duplicate catalog or package tooling
was added to conceal the mismatch.

The original native-window startup blocker was the audit fixture: its first launch persisted
inherited `CODEX_HOME`, and subsequent launches reused that selection. Clean seeded synthetic
state opens successfully with the real-state guard enabled. Keep the guard and seed or isolate
the selection before first launch; no product startup fix was necessary.

## 2026-11-17 evidence reconciliation

At the read-only audit's start, source was `40897e1`, stamp **2026-11-16a**, with an existing
uncommitted doctor account-count change. Those read-only executable probes returned:

| Candidate | Reported identity |
| --- | --- |
| Installed application | `2026-11-15c` |
| Existing debug binary | `2026-11-16a, 40897e1+1536` |
| Existing release binary | `2026-11-16a, df17303` |

The interrupted Claude campaign's retained evidence is under
`target/overnight/2026-11-15-ui-logic/`. The recovered native session also contains later command
results. Its private session identity and bounded read commands are retained in the requesting
Claude project's existing `project-harness-single-source` memory. These are inspected dated
receipts, not checks rerun for this reconciliation:

- `journeys-all-final.log`: 50 passed, zero failed, three filtered, 2026-11-16.
- `all-tests-final.log`: successful library/integration results. A later session tool result at
  2026-11-16 15:33:46 UTC reports 1,022 library tests passed, four ignored, zero failed.
- `live-cursor-final.log`: three passed and the question case failed. The session records a
  later focused question-case pass in 14 seconds on 2026-11-16 at 14:48:30 UTC, followed by three passing
  `live_providers` cases at 14:49:18 UTC. Preserve the failed aggregate and the narrower rerun;
  the manifest's shorthand `live_cursor 4/4` is not a fresh clean four-case suite receipt.

No new library/integration suite, live provider turn, rebuild, installation or trust change
was performed in that read-only audit. CLI metadata observed locally: Codex 0.166.0, Claude Code 2.1.326,
Cursor `2026.11.13-eacade1`. Current Codex source still qualifies exactly CLI 0.166.0;
current Claude source accepts its supported minimum version or newer.

The initial isolated launches stopped on a real-profile selection retained in reused audit
state. The clean-state qualification above resolves that fixture blocker without disabling the
guard. This was not an ordinary startup crash or proof credentials were read. Broader visual,
focus, latency and default-provider interaction qualification remains open.

The synthetic `%TEMP%/concordia-parity-audit-smy35u7h` directory remains after automatic policy
review rejected its recursive cleanup. No user application was killed.
The next agent should inspect ownership before any cleanup retry.

See the [current completion-depth obligations](../PLAN.md#completion-depth-audit-and-claude-resume---2026-11-17).
Later dated checks do not automatically qualify every byte of the current dirty source or
installed executable. The qualification below applies to its stated historical candidates.

## Historical 15a qualification

The **2026-11-15a** candidate was based on audit repair `3239abe`. That qualification
did not install or publish it. Local runtime logs confirmed the earlier **14p** installation.

## Model, effort and update feedback qualification

| Check | Covered behavior | Result |
| --- | --- | --- |
| Native Windows journey | Model opens effort; Esc preserves the pair; keyboard commit; `/effort` and direct value; invalid value preservation; persisted selection; header hand cursor | Pass |
| 54 update-related library checks | Worker progress, structured diagnostics, restart/failure, Escape and close-button cancellation, preview controls and background readiness | Pass |
| Existing update presentation check | Progress/details/failure at 40×12, 90×28 and 150×45; no-color; scroll to diagnostics; long-model effort visibility | Pass |
| 5 model-picker and 11 command checks | Reported catalogs, filtering, command parsing and README registry synchronization | Pass |
| 19 release integration checks | Real loopback HTTP/PowerShell, hashing, verified installation into isolated sandboxes and failure preservation | Pass |
| Native Codex app-server | One tiny `gpt-5.6-luna` low-effort turn; reply `OK` | Pass |

Compiler warnings no longer replace progress. Cargo reports existing unused-variable, unnecessary
`mut` and dead-code warnings; successful compilation is not a warnings-clean claim. Diagnostics
remain bounded/redacted, and failed Cargo JSON is projected as readable errors. The existing
process cancellation check also cancels an actual PowerShell HTTP request against a delayed
loopback peer. Release checks, downloads and hashing use that process owner.

Observable artifacts are generated under `target/preview/model-effort-update`: native-window
text snapshots for model/effort and rendered-buffer SVGs for update progress/details. The SVGs
use synthetic job state; they are not screenshots of a real installation. The screenshot helper
now preserves spaces explicitly in SVG text so browser rendering retains terminal cell geometry.

The broader 28-case `tui::tests::run` group exposed four unrelated unresolved expectations:
landing arrow navigation, streamed command expansion, wait-card expansion and thinking-card
expansion. Their owning behavior is unchanged by this slice; no baseline execution or full-suite
pass is claimed. Two update-specific stale fixtures were corrected and the update checks pass.
No Claude inference ran, no credentials changed, and no real installation was performed.
Formatting, Cargo check, documentation, OSS privacy, hidden-process and static cycle gates pass.

## Audit repair qualification

| Check | Covered behavior | Result |
| --- | --- | --- |
| 125 focused existing library tests | Queue, continuation, attention, models, history, provider events, MCP/storage, redaction and process constructors | Pass |
| Bounded MCP process probes | Disabled/project-off enforcement, traversal rejection, oversized-frame resynchronization, initialized notification and matching IDs | Pass |
| Quiet and streaming MCP peers | Quiet child returns after about 10 seconds and is reaped; lowercase session headers and matching chunked SSE succeed | Pass |
| Fresh library probes | Config preservation, exact completion, owned proposals, secret-free projections, four concurrent mutations and consent/cap admission | Pass |
| 2 provider process tests | Closed-start receiver cancellation; duplicate native requests and answer replay | Pass |
| Windows native-window journey | Question, approval, notification and idle restart with synthetic native history | Pass |
| Native Codex standard turn | `gpt-5.6-luna`, low effort, one-line prompt, read-only trust, reply `OK` | Pass |
| Native Codex app-server turn | Same model/effort/prompt; accepted native `effort` field, reply `OK` | Pass |
| Independent architecture, UI/logic and security review | Canonical seams, transition fixes, exact historical source-reference gate and boundary probes | Pass |

Counts: queue 44, continuation 17, approval 1, model picker 5, history 10, provider failure/interaction/Claude/replay 14,
registry/record/schema 9, config/redaction 22 and process constructors 3.

The empty `provider::codex::tests` and `delegate::tests` filters are excluded from the 125 count.
The history failure was an invalid fixture: queue inspection stayed focused and later fixture
run IDs were not UUIDs. The corrected journey dispatches ordinary prompts with actual run IDs.
Other stale picker fixtures now use provider-reported models and usage and select by explicit IDs.
No new unit or integration test cases were added.

Formatting, `cargo check --locked -p concordia`, documentation, OSS privacy, hidden-process and
static cycle gates pass. The final independent process review is recorded in the
[archived repair plan](plans/archived/2026-11-15-audit-remediation.md).
The privacy check recognizes two existing public identifiers in exact files; it still scans personal
paths and every other private token. The native-window journey required fixing the initial snapshot
status and supplying the fake provider's missing native history; runtime recovery protection remained intact.
The module-cycle check discovers nested/inline modules and source use trees, then checks exact
historical ownership edges. It is a static approximation: macros, feature evaluation and compiler
name resolution are outside its scope. Path attributes must be adjacent to their module declaration.

No Claude inference ran while its quota was exhausted. Claude effort delivery was inspected and
fixture-checked; a fresh live Claude turn remains unqualified. Cursor/Grok/opencode, credential-backed
MCP delivery and TLS certificate/server combinations did not receive new live qualification.
The HTTP/SSE mock uses loopback HTTP; Windows HTTP and HTTPS share the WinHTTP request owner.
These checks establish the named flows, not universal provider parity or native hook enforcement.

## Historical installed-build record

Build **2026-11-09g** was a private Windows build. Its audited runtime source and pre-rewrite
documentation are preserved in local Git checkpoint `6fea4a9`.

Installed executable SHA-256:

```text
2ad5dfd44b31f1cab3e1f52555cc8a57a5bafdb7c8f87ac06ebf439f250ceef8
```

## Checks run for 09g

| Check | Covered behavior | Result |
| --- | --- | --- |
| 9 interaction checks | Ownership, cancellation, connection identity, reply limits and duplicate requests | Pass |
| 10 command checks | Customization routing, arguments and local command definitions | Pass |
| 10 completion checks | Discoverability, valid command branches, descriptions and selection | Pass |
| 2 setup process cases | Shared setup and skills across provider homes, later accounts, failures and source changes | Pass |
| Native request process case | One prompt/answer for identical retransmissions; no replay after native resolution | Pass |
| Windows terminal journey | Completion, skill install/list/remove, hook inventory/status and invalid arguments | Pass on the exact optimized candidate |
| Direct stock Codex smoke | Codex 0.166.0 app-server, gpt-5.6-luna, low effort, one OK reply | Pass; no tool grants |
| Installation | Installed executable matches the checked candidate hash | Pass |

Setup cases also cover linked-source updates, existing-name preservation, stale-source removal,
v1-to-v2 storage migration and unknown-version preservation. The terminal journey starts no
provider turn. The stock Codex smoke is separate from Concordia's process-fixture evidence.

Formatting, whitespace, module-cycle and supplemental lock checks passed. The lock scanner
checked zero comment locks; this does not establish native hook enforcement. Independent
process, architecture, UI and logic reviews passed for the 09g changes.

## Boundaries still unqualified

- Broader live approval, worker, input/output and lifecycle coverage across provider versions and
  Windows native/terminal hosts. A fixture or one live Codex reply is not universal parity.
- Native hook trust and a real denial test for each supported provider/home/project binding.
- Cursor current-window capacity, external per-event usage and omitted numeric credit balances.
- Memory superiority, token savings and retention advantage. Existing experiments are scoped.
- OS-enforced workspace confinement and public release/feed qualification, both owner-held.
- Read-only inspection of retained duplicate-chat display snapshots remains a paused feature.

## Cleanup closeout

There is one worktree and no Cargo target directory. Earlier obsolete worktrees, build trees,
archives and superseded source paths were removed. The 09g verification cache removal freed
1.8 GiB. Unused installer images were removed only after ownership and content checks.

The owner manually removed the older executable, matching receipt and four synthetic Temp
directories previously blocked by automatic approval review. Their absence was verified on
2026-11-09. Native histories, credentials, drafts, queues and unique recovery records remain.

## Documentation rewrite

Checks run for this rewrite slice: `habena docs check --plain`, `habena run check:docs --changed`,
`scripts/verify-oss-gates.ps1` and `git diff --check`, all passing. A changed-files-only check cannot
certify untouched pages. No runtime rebuild or new provider turn was run for this documentation
work; the entire rewrite and product interview remain in progress.

## Earlier evidence

Detailed build journals, audit scopes and historical receipts are preserved in Git. For example:

```text
git show 6fea4a9:docs/e2e-status.md
git show 6fea4a9:docs/quality/e2e-history.md
git show 6fea4a9:docs/agent/cleanup-review.md
```

The old journal's named sections remain intact: "Scoped native questions, cancellations and PDF
reading on 05c" contains the PDF/native-input receipts; "Source preview validation (`2026-10-21a`)"
contains the first preview check. Working-tree links now lead here for Git retrieval rather than
duplicating those receipts in current documentation.

Earlier results qualify their dated source and artifacts. They do not qualify later changes or
a public release. The documentation rewrite does not change that boundary.
