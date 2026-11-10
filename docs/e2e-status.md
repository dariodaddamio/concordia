# Verification status

Status is scoped to a build and a test path. Passing fixtures does not certify all provider
versions, native approval flows or physical hosts. No public release is implied.

Installed 09g: 29 focused interaction/command/completion checks pass. Two real-process
setup cases cover all providers, later accounts, source drift, skill-link updates, name collisions,
removal, missing-source cleanup, v1-to-v2 migration and unknown-version preservation. The native retransmission process case confirms one prompt
and one answer, with no replay after native resolution. The Windows terminal journey exercises
completion, install, source inventory, hook inventory, status, invalid arguments and removal
without starting a provider turn. Process checks used the local verification build; the terminal
journey also passes against the exact final optimized executable before installation.

One direct stock Codex 0.166.0 app-server turn on `gpt-5.6-luna` at low effort returned `OK`;
no tools were granted. This is a native protocol smoke, separate from Concordia's fixture proof.
Independent process, architecture, UI and logic reviews pass. Module-cycle and supplemental-lock
guards pass; zero source comment locks were checked. Documentation still has 139 existing
table-density findings. The installed executable matches the verified candidate SHA-256:
`2ad5dfd44b31f1cab3e1f52555cc8a57a5bafdb7c8f87ac06ebf439f250ceef8`.
Existing older windows remain open until the owner reopens them. The temporary Cargo tree was
removed (1.8 GiB). The unused previous installer image was removed after receipt, hash and
exclusive-open checks; the image still mapped by the running window was preserved.

| Scope | Evidence | Limit |
| --- | --- | --- |
| Installed 2026-11-09f restored account metadata and recovery discovery | Exact optimized executable passes a native-window journey with two synthetic accounts: restored email belongs to the tab's home, stays private until revealed, and does not change global selection; an account without reported email has no broken label. A partial saved snapshot reloads exact native history and clears its stale omission marker. Routine startup chatter is absent, other saved work appears in the footer, and `/rec` completes `/recover` and opens intact saved drafts. Reopen preserves history; unavailable sources retain the cached view and genuine warnings. Native source hash unchanged; zero provider turns. Formatting/whitespace and three repository guards pass. Installed SHA-256 is `c0834990f02ace626a55669b20ad3394dcf4c0dcb1d6b225fb65919a39cd2968` | Synthetic Windows metadata/UI/storage proof. History reads retain existing finite bounds; truly partial or unavailable sources remain labelled. No authentication migration, model call or automatic request replay. Existing user window remains open; reopen to load 09f |
| Installed 2026-11-09e Codex large app-server events | Optimized real-process integration passes 2 MiB output plus UTF-8 text chunk preservation, 17 MiB early output followed by a native question, and explicit failure above the 64 MiB frame cap with no replay. Existing image/permission/identity cases pass in the same integration. Exact executable passes two native-window journeys: large event/question/reply releases its reservation and preserves a newer draft; real stream closure restores its failed prompt and retains an uncertain tool reservation with the corrected warning. One direct stock Codex 0.166.0 app-server live turn, `gpt-5.6-luna`/low, read-only, no tools, returns `OK`. Formatting/whitespace and three repository guards pass. Installed SHA-256 is `9d9dec2f1df85e58a5e7f059a41175a4a97cb1e5af76de541d0709a4374a12b4` | Large frames and application UI use synthetic provider processes; the one live turn checks native protocol compatibility, not a live oversized response. Transport cap is 64 MiB; UI/request budgets remain separate. Unknown tool effects still require explicit reconciliation. Existing user windows and failed requests remain untouched |
| Installed 2026-11-09d inline tab rename | Exact optimized executable passes native-window double-click on an inactive tab, title editing in the tab row with the independent draft visible, Enter save, F2/Escape cancellation, outside-click cancellation, both drafts preserved, restart persistence and single-tab F2 editing. No provider turn dispatched. Formatting, whitespace and three repository guards pass. Installed SHA-256 is `32c45701ab2e0a14ec28f0febd8af44ed72874178a3b1ccc254a63713d9b126d` | Synthetic Windows UI proof; no new live-provider qualification. Native prompts and other modal owners retain priority. Probe state lives inside the temporary Cargo tree and is retired with it. Existing user window remains running; reopen to use 09d |
| Installed 2026-11-09c chat ownership and interrupted-send recovery | 21 focused runner/history/checkpoint/recovery checks pass. Final optimized executable passes six native-window scenarios: orphan request/history restoration, duplicate navigation without draft replacement, canonical duplicate snapshot retention, close/reopen, cross-window ownership rejection and source-empty checkpoint authority over a stale saved session/view. Original synthetic native sources and checkpoints retain their byte hashes; zero provider turns. Formatting, whitespace and three repository guards pass. Installed SHA-256 is `7c7b5bcccc2cf0dd624d9daeb734cc0194ba3012ba7dec47f3e5bba05a4e0088` | Synthetic Windows UI/storage proof, not new live-provider qualification or exhaustive state coverage. View leases coordinate windows sharing a state directory. Diagnostic screen output is shared/rate-limited; cross-window preservation is checked against each canonical saved layout. Distinct display-only duplicate snapshots remain retained but have no dedicated inspection UI. Existing user window is left running; update applies on reopen |
| Installed 2026-11-09b tabs, local shell and clarification admission | 65 focused registry/question/queue/history/tab checks pass. Exact optimized executable passes the native-window probe for inactive-tab double-click rename, draft preservation, persistent PowerShell variables across hide/show, padded close target, remaining draft/layout and source preservation. The terminal-host probe passes chat UI via `--terminal`, embedded PowerShell, Ctrl+C interruption, resize, return to chat and `/exit` while owning a shell. Formatting, dependency policy and three script gates pass; installed SHA-256 matches | No model inference ran in this pass. Inferred tool-card replies are ordinary queued follow-ups and cannot unblock an absent native response channel; native requests keep their owned channel. Windows was exercised; no universal host/OS or exhaustive state-reachability claim. Feed snapshots are rate-limited: the native probe uses DPI-aware coordinates and a harmless repaint when observing a throttled final frame. Raw probe artifacts are temporary and are retired after the portable summary is saved |
| Installed 2026-11-09a cleanup | 117 focused contract checks, 23 runner checks, 13 control-plane checks, attachment-transport and MCP checks pass. All-target compilation, formatting and three script gates pass. Exact optimized executable passes both v2/v3 tab restart probes and older-session/draft/title probe; installed SHA-256 matches. Two tiny live replies pass: Codex Standard and Cursor ACP. Three integrated worktrees/branches, redundant bundle and inactive old build retired: 29.46 GB of logical file sizes | Main target still contains 7.75 GB: Cargo refuses its missing/invalid cache marker, and automatic approval review rejected direct recursive removal. No deletion occurred there. Cursor Standard hit its native workspace-trust prompt; Claude 319 has initialize-only evidence, not new live question/tool qualification. User data and existing window preserved; step 7 postponed |
| Installed 2026-11-08h tab continuity and workflow follow-through | 105 focused checks, followed by 13 final restore/usage checks, and 34 selected provider/worker integrations pass. Exact optimized executable passes both v2/v3 checkpoint native-window probes: three tabs, order/active/conversation identity, drafts/queues, paused work, newer source bindings, sticky omissions, concurrent window locks and visible corrupt-layout warning. Exact older-session history appears in 0.14s with no further input; draft/rename/source-preservation checks pass. Native worker/retirement and 8 MiB tail-boundary probes pass. Installed SHA-256 matches. Live Codex reply/edit+diff and Cursor reply/exact continuation pass. Three merged local references and 56.8 MB of obsolete generated artifacts retired safely | Focused executions overlap; no full-suite or universal parity claim. Claude weekly quota and native-control version preflight block live qualification; Codex/Cursor live question prompts produce no broker question. Prior concurrent history latency remains unexplained. The old worktree/build-directory and redundant-bundle cleanup was subsequently completed; see the 09a row. Current evidence and native/user data were preserved. Pre-layout tab order was never recorded; cached views require explicit resume to load external native CLI changes |
| Installed 2026-11-08g whole-repository audit | 520 paths accounted with explicit review depth; focused runtime/UI checks and all-target compilation pass. Final 93 contract checks, 30 process/harness/usage/update integrations, attachment transport fixture and one live Codex low-effort OK turn pass. Final unchanged 5,000-row speed check: 9.55 ms/frame. Exact release resumes a session older than the recent list in 1.98s without additional input, preserves drafts through question-mark input and F2 cancel/save, durably renames, preserves source hash and starts no provider turn; installed hash matches | Fixtures and one small Codex turn do not establish universal parity. Unix repair unbuilt; initial wrapping and large-source fingerprint work remain linear. Protected recovery, cross-process account quiescence and earlier concurrent history latency limits remain. Native probe paced modal actions for the one-second feed snapshot sampler; no full suite or publication qualification |
| Installed 2026-11-08f follow-through | Focused storage/parser/worker/UI/MCP and native transport checks pass; one low-effort live Codex runner turn returned OK with feed. Final native history appeared in 0.16s; a deliberately stalled four-event channel delivered all 81 text events with a bounded explicit projection; paged journal/source-preservation, native large-output/question and installer checks pass | One concurrent fixture history timeout remains a latency limitation; serial same-probe reruns passed. Unknown legacy locks and unavailable-source/unfinished work fail closed or remain protected. No full suite or broad host/provider/release parity |
| Installed 2026-11-08e audit repairs | 106 existing checks across ten focused modules, 18 updater checks, two process-job unit checks and four process/runner/quota integration checks passed. Exact release passed native history repaint without further input, native stream/close, 17 MiB app-server question replay, large-metadata list/index/excerpt and corrupt-store preservation probes; four-file installer fixtures passed; installed SHA-256 matches | One direct official Codex `gpt-5.6-luna` low-effort turn returned OK, separately from app transport fixtures. No full suite, physical-host matrix, live app-server question or native closed-tab-reopen proof. OSS privacy scan still fails on private-reference matches; no publication |
| Installed 2026-11-08d Home navigation | Native 08c reproduction grew from one to 13 recovery records after 12 arrow cycles. Final 08d release stays at one for an empty tab and two for chat plus Home after repeated arrows/header clicks. Both saved-chat and unsent-only drafts survive independently; Escape returns to the chat. Existing tab and Home recovery checks passed; installed SHA-256 matches the release build | Isolated native fixtures; source hash unchanged and no provider turn started. No new live-provider or whole-host qualification |
| Installed 2026-11-08c chat discovery | Read-only probe against existing histories returned zero Home chats on 08b and 40 across four accounts on 08c; native Home capture shows the restored list; all 13 existing session checks passed; release build and installed SHA-256 match | No provider turn sent; summary reads remain bounded, so setup beyond 1 MiB remains outside discovery coverage |
| Installed 2026-11-08b | Release build passed; installed SHA-256 matches the checked executable; 46 existing queue checks passed | Focused checks only; the earlier full suite does not qualify later edits |
| Native questions on 08b | Synthetic Codex app-server replay sent 17 MiB of telemetry per turn; three choices were selectable while busy, commentary remained separate, and a click delivered a second user turn | Replays the reported message shape; not live-provider or broad host parity |
| Queue attachments on 08b | Native main-window add/remove through F9/cards, save/cancel and explicit restart recovery preserved independent queue/edit/composer snapshots | Synthetic provider and local PNGs; does not prove every binary transport |
| Live streaming on 08b | One Codex `gpt-5.6-luna` turn returned OK with feed output through the existing live-provider runner | Standard transport; no live app-server question, Claude or Cursor qualification in this continuation |
| Earlier installed 2026-11-08a | 1,113 unit tests passed (six ignored); full integration suite, everyday smoke and all-target Clippy passed; installed hash matched that candidate | Historical qualification for 08a only |
| Native window on 07g | Debug/release/installed probes: sampled Home canvas and margins were #000000; synthetic reply completed; WM_CLOSE worked | Does not certify every UI interaction or real-provider response |
| Native preview on 08a | The reported spurious table underlines reproduced on 07h; repaired release captures show the long table after resize and PageDown; document-relative link regression passed | Synthetic local document; no provider turn or broad host-matrix claim |
| Context and recovery | Tests cover source identity, deferred context delivery, persistent dismissal, history resume and separate tab drafts | Native originals must remain available; uncertain actions require reconciliation |
| Live providers | Previous scoped Codex, Claude and Cursor checks are recorded in the journal below | Each result applies to its named version, transport and request; not universal parity |
| Memory effectiveness | Small source-retrieval/retention trials exist | Broad memory superiority and token savings remain unproven |

The installed cleanup is tracked in [cleanup review](agent/cleanup-review.md). The earlier 07h smoke
image fixture timed out across an apparent host suspension; the targeted fixture and complete
sequential smoke recheck passed. Both logs are retained. The 08a full suite and installed smoke also
passed; native-preview checks supplement the earlier 07g Home/streaming probes. The later 08b
checks are scoped separately above. A preceding live Haiku question check hit the account's
weekly limit and does not establish a passing native question flow.

## Remaining release decisions

Choose the first-release audience, primary workflow and provider/host support promise with the
owner. Resolve the exact candidate's [release gates](release-blockers.md) afterward. Missing
native capacity/usage data remains distinct from implementation defects.

## Evidence

The installed 09b executable SHA-256 is
`354aa83907a74016d3c5171f1e5493222803b55a5f6b162527b1b6d2fa164864`.
The native probe preserves original synthetic native-source hashes and the independent chat draft.
An actual Windows shell runs behind the existing PTY/process owner; hiding it keeps its variables,
while closing its tab/window stops it. The host probe exercised Ctrl+C against a running sleep,
then executed another command, resized the host, returned to chat and exited cleanly with an owned
shell. Question regression checks include busy and paused inferred replies preserving the original
task, queue order, draft and cancellation state; exact native answers remain separate.
Initial probe failures included an exit-status wording mismatch, non-DPI-aware click coordinates,
console-output encoding and a stale rate-limited screen projection. Persisted tab layout and
lifecycle markers disambiguated those observations; they are not new provider-parity evidence.
The source review also removed the stale UI override that made the documented `/exit` a no-op.
The [state inventory](tui-states.md) records inspected transitions and remaining coupled states.
After installation/hash verification, canonical Cargo clean removed the single temporary release
tree: 2,502 files, 1,128,409,313 logical bytes. Its valid cache tag was created by Cargo during the
fresh build. The target directory is absent; the installed executable and existing user window remain.

The installed 09a executable SHA-256 is
`adfcb91bb7c405fabb6a420c21401b017f0384ade167796e0470d24219c1dc22`.
Both native restart probes used that exact executable: three tabs, order/active tab, conversation
identity, drafts/queues, paused requests, newer checkpoint bindings, sticky omissions, independent
window locks and a visible corrupt-layout warning. Original synthetic native sources remained
unchanged and no provider turn started. Older-session history appeared in 0.14 seconds without
further input; question-mark input and rename cancel/save preserved the independent draft.
The 09a receipts were `target/current-cleanup-*`. The owner subsequently removed that build tree,
including its ignored raw receipts, before 09b work; these portable summaries remain the record.
The 155 existing deterministic checks are scoped checks, not the full suite or universal parity.
The initial stale model-cache and unbound-Cursor fixture expectations were corrected to enforce
the new contracts, then their checks passed. The live check used Codex `gpt-5.6-luna` low effort
and Cursor `auto`, each returning OK. The separate Cursor Standard attempt stopped at native
workspace trust before a model response; no permission bypass or global configuration change followed.

- [Dated verification journal](quality/e2e-history.md): the complete previous status file,
  including superseded designs, failures and retained qualification limits.
- [Chat repair evidence](agent/chat-repair.md): the 07f/07g repairs and installed checks.
- [Completion audit](feature-completion-audit.md): detailed outstanding work.
- [Images and diagrams](images-and-diagrams.md): input/rendering limits.

Use `concordia --version` to identify an executable. A running window retains its old build
until reopened. Current scope is in [PLAN.md](../PLAN.md); historical build notes belong in
[CHANGELOG.md](../CHANGELOG.md).
