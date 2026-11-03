# Delegation (orchestrator and workers)

Build `2026-11-02p` adds shared routing explanations and saved ordered account/model policies.
`/delegate routes` inspects current eligibility; `/delegate routes ID` shows the task's captured
policy and recorded failover reasons. `/delegate policy set ACCOUNT[:MODEL] ...` changes future
tasks only; `policy legacy` restores the existing worker order. Approved and queued tasks retain
their account/home/login bindings. A changed binding pauses dispatch. Ledger v4 migrates earlier
versions without replay. [Routing behavior and limits](quality/routing-policies.md).

Build `2026-11-02o` records worker usage per native attempt, including failure, cancellation and
interruption. `/usage` → tokens groups worker/main origins and runs, with unknown counters and
reported-cost coverage. Worker inspection shows every attempt. Recovery reconciles accounting
without running a provider. Ledger v3 migrates v1/v2; older v2-only executables reject v3.
See [accounting bounds and evidence](quality/worker-accounting.md).

Build `2026-11-02i` persists worker request/identity/state under the pane's recovery lease, with
an additional worker OS lock. UUIDs bind owner, conversation and run; workspace, exact account home,
model, attempts, native session and revision-checked result pointers are retained. The ledger is
atomic and bounded to 8 MiB/64 runs; finished history retains its existing pruning bound.
It stores delegated requests but no copied native replies, thinking, tools or context packs.
Storage failure stops new dispatch and result delivery. Recovery also offers abandoned panes
whose only remaining work is worker metadata.

Restart never relaunches queued or interrupted workers. Those records reopen paused; tools may have
run before the crash, and the previous provider process may still exist. Inspect its native session
and workspace first. `/delegate recover ID` reads a known completed result from its exact recorded
pointer in the background. Changed, missing or unsupported sources are unavailable. A result
without a durably recorded successful completion remains paused even when some text exists.
`/delegate retry ID reconciled` explicitly starts a fresh task through normal account/trust checks.
Native approvals do not transfer. Recovered results need explicit `/delegate use`; ownership is
never inferred from the currently focused conversation.

Result delivery is reserved durably before dispatch. A crash in that window can leave a reservation
without confirmed delivery; inspect main-request recovery before explicitly sending it again.
Automatic duplicate delivery is suppressed. This is at-most-once automatic dispatch, not a claim
of exactly-once provider execution.

`/delegate fallback on` opts into quota failover; `off` disables it. Eligibility follows configured
worker order and native model settings, requires a free logged-in account and valid configured
model, excludes the current orchestrator and same-home/login target, and preserves trust mode.
At most four distinct accounts are attempted. Only explicit terminal quota errors with a confirmed
exit, no tools and no partial answer can reroute. Crash, cancellation, connection failure, unknown
exit and uncertain completion stop automatic retry. Fake-provider checks do not establish live
provider/auth/approval parity.

One of your accounts is the **orchestrator**: the conversation you type into. Up to three other
accounts can be **workers**. You hand a worker one task; it runs in the background, in the same
folder, as a separate turn of that account's own CLI. You see it live under the transcript and
decide what happens with its result.

Worker starts require your command or approval. Result handback can be manual or explicitly automatic.
[Model selection, return modes and remaining limitations](orchestration-and-latency.md).

Native provider subagents are also visible when the CLI reports them. `Ctrl+G`
opens a browser of those reports and Concordia worker jobs; Enter opens details. Codex
`collab_tool_call` items identify children, tasks, states and reported messages. Completion of
the spawn tool does not mark the child complete. Claude Agent/Task tool cards are accessible
through the same browser. This view displays provider-reported information; it does not create
additional provider turns or introduce child controls unsupported by that provider.

Native subagents do not require cross-account worker enrollment. Worker account actions are inside `/delegate`.
registers a separate account for explicit `/delegate` tasks. Registration and task execution
remain separate actions.

## Commands

| Command | What it does |
| --- | --- |
| `/delegate` | Lists workers and provides add, remove, stop, result handoff, protocol and help choices. |
| `/delegate <account\|auto> <task>` | Starts one worker turn as a shortcut. `auto` picks the first idle worker. |
| `/delegate after ID[,ID] ACCOUNT TASK` | Approves a task that waits for the named successful runs in this workspace. |
| `/delegate graph` | Shows task readiness and exact prerequisite run identities. |

Dependency tasks accept up to eight prerequisite IDs from the current conversation. For example,
`/delegate after 1,2 codex Review the completed changes` approves that review now; it runs only
after both exact runs succeed and the worker is free. Waiting tasks count toward the existing
two-task queue limit but do not occupy a running account. Failure, cancellation or uncertainty
blocks dependants while unrelated work can continue. Stop a waiting task through the worker row
or `/delegate stop ACCOUNT`.

Dispatch rechecks the worker registration, login, home, model and workspace. Results come from
verified native source snapshots, labelled untrusted and bounded to about 3,000 excerpt characters
plus source pointers. Missing or stale sources pause the task for inspection. Referenced result
metadata survives ordinary history pruning. A restart pauses unfinished dependent tasks; it does
not run them. An explicitly reconciled retry creates a new task and never retargets old edges.
This slice supports the shared workspace only. Worktree creation and cross-worktree integration
remain separate work; the scheduler never merges edits.

An account can be a worker when it exists, is logged in, is not the orchestrator, is not the same
login as the orchestrator or another worker (checked by the same identity used for duplicate
accounts), and fewer than three workers exist. Workers are stored in `store.json`
(`settings.workers`) with the other settings.

## What a delegate gets

- **A fresh native session** every time. No `resume --last`, no session id borrowed from the
  orchestrator.
- **A bounded context pack**: your task plus the last few orchestrator messages (at most 6
  messages and about 3000 characters), built by the same planner as account switches. Tools and
  approvals are **not** transferred; the pack says so.
- **Your trust mode**, never more. A worker runs with the mode you currently have (or the
  `childTrust` setting when stricter). It is `full` only when your own mode is `full`.
- The **current folder**. A worker and the orchestrator can therefore edit the same files. Give
  workers separate concerns (for example "write tests for X" while you work on Y), and read what
  they did before you rely on it.
- **One task at a time per worker.** More tasks for a busy worker queue behind it (at most 2 per
  worker); beyond that the command is refused with a message.

## The worker strip

When a worker is running or recently finished, rows appear above the prompt (hidden otherwise;
at most four rows, then `+N more`). Each row: an animated mark, the account, elapsed time, state
and the latest tool, thinking or text line. States use the same colours and animations as the main
wait: starting (cyan), thinking (magenta), tool (yellow), writing (green), plus queued, done
(green check), failed (red cross) and cancelled. With `NO_COLOR` the colours go away and the
bold focus marker stays.

- `Alt+Up` / `Alt+Down` or a click focuses a row. Past either end the focus is released.
- `Enter` (empty prompt) opens the worker's transcript: task, tool cards, output. `Esc` closes it.
- `Esc` on a focused running worker cancels it (it kills the provider's whole process tree).

## Results

When a worker finishes, a block appears in the orchestrator transcript: worker, provider, state,
duration, tool-card count, task and final text, marked **untrusted worker output**. Nothing is
sent to the orchestrator and nothing in it is executed. `/delegate use`, `/delegate collect`,
or opt-in automatic returns determine when it reaches
the orchestrator: you choose it, it is capped at 6000 characters and wrapped in a label telling
the orchestrator it is data from another agent, not instructions.

## Optional: orchestrator-proposed delegation

Off by default (`settings.delegationProtocol`, or `/delegate protocol on`). When on, a fenced
block in the orchestrator's reply:

````
```concordia-delegate
{"to":"codex-2","task":"write unit tests for parse_slash"}
```
````

shows `Orchestrator wants to delegate to <account>: <task> [y/n]`. Only an explicit `y` runs it.
The key answers only when the prompt is empty and the question has been on screen for 1.5
seconds, so typing ahead cannot answer it. Targets must already be your workers; at most 3
proposals are pending; malformed blocks are ignored. `/delegate help` prints a ready-to-paste
snippet that teaches an orchestrator the format. Events are logged without the task text.

## Safety model

1. **User-initiated.** Every worker turn starts from your command or your `y`.
2. **Bounded.** Small context pack, capped task, capped results, 3 workers, small queues.
3. **No recursion.** Worker output is never parsed for delegation. Only the orchestrator's own
   replies are, and only when you opted in. A worker cannot start another worker.
4. **Explicit session binding.** Fresh session per delegate; never `--last`, `--continue` or a
   foreign id.
5. **No credential movement.** Each worker runs the unmodified provider CLI in that account's own
   home. Concordia never reads or copies tokens beyond the existing "is this login present and
   which one" check.
6. **Tools and approvals are not transferred.** The worker's approvals are its own CLI's, at the
   trust level above.
7. **Untrusted output.** Labelled, shown, never executed.
8. **Cancellation is real.** It kills the process tree; leaving Concordia cancels all workers.

## Terms

Each worker is one of your own accounts, signed in by you through the provider's own login, driven
through the unmodified official CLI, using the same headless interface a script would. Concordia
does not rotate accounts to get around a limit, share one login across several people, or run
workers you did not start. The duplicate-login check exists to keep it that way: two worker slots
cannot be the same subscription. See [provider-terms.md](provider-terms.md).
