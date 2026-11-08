# Memory, continuity and orchestration

Concordia has two different kinds of memory and two different kinds of subagent. Keeping those
separate makes the implementation much easier to reason about:

- the provider owns native sessions and their original history;
- Concordia owns local pointers, maps, relationships, recovery records and routing decisions;
- Concordia workers are separate account-bound turns scheduled by Concordia;
- native subagents are children reported by Codex or Claude inside one provider turn.

## Native session discovery and indexing

Account discovery begins with an explicit provider/account/home. Session readers then enumerate
provider-owned history below that home and return stable metadata such as provider, account,
session id, cwd, title/preview and source path. They do not select “the latest” session to resume.
Source: [`sessions.rs` (`list_native_records_bounded`, `list_recent_sessions`,
`find_native_record`)](../crates/concordia/src/sessions.rs)
and [`cursor_history.rs` (`read`)](../crates/concordia/src/cursor_history.rs).

For file-backed Codex and Claude history, a native-source index binds:

- provider, account, canonical home and explicit session id;
- the relative source file and file identity;
- source length plus a hash of its bounded header;
- each indexed message's byte start, length, role and SHA-256.

The recent index scans at most an 8 MiB tail, accepts at most 128 messages and bounds one source
record at 256 KiB. An excerpt is at most 8,192 bytes. Whole-history pages remain immutable,
content-addressed pages rather than a growing cursor over changing bytes. Source:
[`native_memory.rs` (`TAIL_BYTES`, `HEAD_BYTES`, `LINE_BYTES`, `MAX_MESSAGES`,
`SourceIndex`, `index_bound`, `history_from_pointer`, `read`)](../crates/concordia/src/native_memory.rs).

Cursor has no local JSONL contract in this code path. Its read-only history implementation starts
the unmodified ACP interface, requires advertised `loadSession`, loads the exact saved session and
rejects tool requests while collecting bounded history into the same index format. No prompt or
new session is sent. Source:
[`cursor_history.rs` (`open`, `Replay::rpc`)](../crates/concordia/src/cursor_history.rs).

Every read revalidates account/home ownership, source identity, length, header hash and requested
message hash. A missing, replaced, truncated or compacted source produces an explicit error; it
never falls through to a similarly named or newer session. Source:
[`native_memory.rs` (`Binding::verify`, `open_index_source`, `read`)](../crates/concordia/src/native_memory.rs).

## Thread log versus native history

The optional Concordia thread log lives under `<state>/threads`. New records are metadata or
verified references; `ThreadLog::append_referenced` binds a body to a captured native range, and an
unavailable body gets a fixed reason. Legacy text-bearing records remain readable for compatibility.
Source: [`threads.rs` (`enabled`, `ThreadLog::append`, `ThreadLog::append_referenced`,
`ThreadLog::read_all`)](../crates/concordia/src/threads.rs) and
[`thread_references.rs` (`Record`, `hydrate`)](../crates/concordia/src/thread_references.rs).

Thread logging is on when `CONCORDIA_THREADS=1`, the saved `thread_log` setting is on, or either
the current/legacy config switch is on. With logging off, a switch can still build a transient map
from the current in-memory transcript, and native-source pointers can still preserve exact provider
history. Source: [`threads.rs` (`enabled`, `events_from_messages`)](../crates/concordia/src/threads.rs)
and [`context.rs` (`build_context_pack_with_workspace`)](../crates/concordia/src/context.rs).

## Continuity decision

`context::plan_continuity_capped` is the central rule:

```text
same provider + same normalized home + explicit session id
    -> native resume; old id is passed; no map
anything else
    -> fresh native session; old id is cleared; bounded map if requested
```

Cross-provider and cross-account handoffs therefore carry information, not execution state.
Provider tools, pending approvals and sandbox state never transfer. Source:
[`context.rs` (`should_attempt_native_resume`, `plan_continuity_capped`,
`continuity_reason`)](../crates/concordia/src/context.rs).

After a successful fresh handoff, the new provider reports its own native session id; subsequent
turns resume that new session instead of resending the pack. Source:
[`tui/app/turn.rs` (`apply_progress`, `TurnProgress::Session`)](../crates/concordia/src/tui/app/turn.rs)
and [`tui/run.rs` (`prior_provider`, `prior_home`, `session_out`)](../crates/concordia/src/tui/run.rs).

## Handoffs, limits and uncertainty

There are three related recovery cases:

1. **Explicit switch** — `/use` or resume selection changes account/provider. The next turn gets a
   fresh session and the bounded map/pointers unless exact native resume still applies.
2. **Context capacity** — a recognized provider context-exhaustion signal can make one fresh
   session on the same account with exact-source pointers. Optional Codex proactive rollover uses a
   source-bound native measurement and stops if sources cannot be verified.
3. **Usage exhaustion or server capacity** — the request and queue are paused. Manual `/continue`
   chooses an account/model. Automatic account/model routing is opt-in and refuses uncertain tool
   completion, cancellation, storage failure or exhausted attempt bounds.

Source:
[`provider/failure.rs` (`FailureKind`, `classify_failure`)](../crates/concordia/src/provider/failure.rs),
[`capacity.rs` (`Policy`, `Measurement`, `apply`)](../crates/concordia/src/capacity.rs), and
[`tui/continuation.rs` (`prepare_usage_continuation`, `resume_deferred_usage`,
`continue_usage`)](../crates/concordia/src/tui/continuation.rs).

The continuation prompt is explicit about reconciliation:

```text
Continue the interrupted request in this new native session. First inspect the workspace and
recorded completed actions. Reconcile any uncertain tool result before proceeding; do not blindly
repeat commands or edits. Follow the current provider's approval rules. Original request:
…
```

It is built only for an explicit/allowed continuation from the retained original request. Source:
[`tui/continuation.rs` (`continue_usage`)](../crates/concordia/src/tui/continuation.rs).

Same-provider automatic handoff is a second opt-in beyond automatic mode and requires recorded
terms acceptance for that provider. The default remains manual; different-provider routing does
not imply consent to same-provider account rotation. Source:
[`continuity.rs` (`automatic_rules_respect_same_provider_opt_in`)](../crates/concordia/src/continuity.rs),
[`store.rs` (`same_provider_automatic`, `same_provider_terms_accepted`)](../crates/concordia/src/store.rs),
and [`config.rs` (`Continuation::default`)](../crates/concordia/src/config.rs).

## Concordia workers

`/workers add ACCOUNT` registers a different logged-in account as an eligible worker. The account
cannot be the orchestrator or another slot for the same detected login. There are at most three
workers and at most two queued jobs per worker. A worker trust request is capped at the user's
current trust; it can never escalate it. Source:
[`delegate.rs` (`MAX_WORKERS`, `MAX_QUEUED_PER_WORKER`, `check_add_worker`,
`worker_trust`)](../crates/concordia/src/delegate.rs).

`/delegate ACCOUNT TASK` creates one fresh provider turn with:

- the selected worker's own provider home and native model catalog;
- a task capped at 2,000 characters;
- bounded map context from the last six orchestrator messages;
- no orchestrator session id and no transferred approvals;
- a no-recursion worker rule;
- a tracked run/attempt owner and independent cancellation handle.

Source:
[`delegate.rs` (`build_worker_context`, `Submit`, `Delegates::submit`)](../crates/concordia/src/delegate.rs)
and [`provider/turn.rs` (`run_provider_turn_with_progress`)](../crates/concordia/src/provider/turn.rs).

### Routing and fallback

`auto` does not mean “pick any account.” Routing captures at most 32 candidates and records whether
each account is registered/logged in, model availability, same home/login, current running/queue
state, prior attempts, quota provenance/freshness and policy eligibility. Ranking prefers usable
known remaining quota and model tier; unknown quota is not treated as free capacity. Source:
[`routing.rs` (`MAX_CANDIDATES`, `Context`, `capture`, `evaluate`,
`rank_candidates`)](../crates/concordia/src/routing.rs).

Optional routing policies are read from a bounded 64 KiB file and captured into an approved
snapshot containing target bindings and revision. Launch revalidates account/home/model against
that snapshot. Worker quota failover is separately opt-in; a failed attempt is recorded before a
different eligible target is tried. Source:
[`routing_policy.rs` (`POLICY_BYTES`, `Approved::capture`, `Approved::evaluate`,
`Approved::verify_binding`)](../crates/concordia/src/routing_policy.rs),
[`delegate_recovery.rs` (`worker_failover`)](../crates/concordia/src/delegate_recovery.rs), and
[`delegate_accounting.rs` (`finish_worker_attempt`)](../crates/concordia/src/delegate_accounting.rs).

### Durable ledger and dependency graph

Each pane owns a `worker-runs/workers.json` ledger below its pane-recovery directory. The ledger is
capped at 8 MiB and binds conversation, workspace, run, attempt, account/home/model, routing
snapshot, native source references, dependency state and delivery state. A recovered active job is
paused; recovery never restarts its provider turn. Storage failure prevents dispatch/delivery
rather than allowing unrecorded work. Source:
[`delegate_recovery.rs` (`LEDGER_BYTES`, `attach_recovery`, `save_recovery`,
`reconcile_interrupted_accounting`)](../crates/concordia/src/delegate_recovery.rs).

`/delegate after ID[,ID] ACCOUNT TASK` creates dependency edges against exact successful runs.
There are at most eight prerequisites. Before dispatch, Concordia rechecks the prerequisite's
account/home/session/native-source binding and reads its verified assistant result. Missing, stale,
failed or foreign dependencies pause the child. Cycles and invalid graphs are rejected. Source:
[`delegate_dependencies.rs` (`MAX_PREREQUISITES`, `validate_dependencies`,
`dependency_status`, `verified_prefix`)](../crates/concordia/src/delegate_dependencies.rs) and
[`delegate/schedule.rs` (`Graph::validate`, `Graph::readiness`)](../crates/concordia/src/delegate/schedule.rs).

Managed cross-worktree edits are not silently merged. Dependency delivery presents observed base,
owner head, worker head and bounded status excerpts, then requires an exact receipt for
result-only or verified integration. Source:
[`delegate_delivery.rs` (`Review::describe`, `Review::receipt`,
`Delegates::accept_delivery`)](../crates/concordia/src/delegate_delivery.rs).

### Result fencing and return

Completion posts a labelled, visibly untrusted result. Only an explicit `/delegate use` or the
separate opt-in automatic collection path sends a result into the orchestrator's next model turn.
Randomized fences are collision-resistant and fence-looking worker text is escaped. The result is
information, never an automatically executed command. Source:
[`delegate.rs` (`fence_untrusted`, `result_block`, `use_prompt`)](../crates/concordia/src/delegate.rs)
and [`tui/workers.rs` (`delegate_use`, collection handling)](../crates/concordia/src/tui/workers.rs).

### Optional proposal protocol

When delegation protocol is off, orchestrator output cannot schedule workers. When on, Concordia
parses at most three fenced `concordia-delegate` JSON proposals from the **orchestrator's own reply**,
never from worker output. Every proposal opens a local yes/no confirmation; parsing it is not
approval and the orchestrator does not automatically receive the eventual result. Source:
[`delegate.rs` (`MAX_PENDING_CONFIRMS`, `extract_delegate_blocks`,
`block_from`)](../crates/concordia/src/delegate.rs) and
[`tui/workers.rs` (`queue_proposals`)](../crates/concordia/src/tui/workers.rs).

## Native provider subagents

Native subagents are provider behavior inside the active turn:

- Codex structured `collab_tool_call` events report child thread ids, task, sender, tool, state and
  message. Concordia merges those events into `native-agent:<id>` cards. A completed `spawn_agent`
  or wait tool does not itself prove the child is complete; the reported child state does.
- Claude native stdio turns Agent/Task lifecycle messages into native-agent cards and tracks local
  background work before declaring the parent turn settled.

Source:
[`provider/stream_cards.rs` (`classify_collab_tool_call`)](../crates/concordia/src/provider/stream_cards.rs)
and [`provider/turn_claude.rs` (`native_task_card`, `run`)](../crates/concordia/src/provider/turn_claude.rs).

`/agents`/Ctrl+G displays both provider-native cards and Concordia worker jobs in one picker, but
labels their state and result-return status separately. It is a read-only display; it does not turn
a native child into a cross-account worker or vice versa. Source:
[`tui/native_agents.rs` (`State::record`, `App::open_agents_picker`)](../crates/concordia/src/tui/native_agents.rs).
