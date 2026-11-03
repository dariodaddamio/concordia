# Completion campaign: approved dependency execution

Baseline: `14d6150` plus the reconciled PR #18 UX fixes. Build `2026-11-02m`.
State: fixture-verified. All 885 enabled library tests and five delegation integration tests pass.
The delegation fixture verifies successful dependent dispatch and exact run/source provenance,
stale-source refusal and cancellation during asynchronous verification without another launch.
Library fixtures cover bounded waiting queues, failed prerequisite blocking and paused restart.
Final candidate checks and installed/remote verification remain separate.

## Scope and observable acceptance

- `/delegate after ID[,ID] ACCOUNT TASK` approves a task and up to eight exact prerequisite
  runs from the current conversation in the owned shared workspace. `/delegate graph` inspects
  readiness and run identities. No worker output can authorize dependencies.
- Existing per-worker queue and concurrency limits apply. Waiting/failed prerequisites never
  dispatch work or occupy a running account. Exact successful receipts release eligible tasks
  in stable queue order; independent tasks remain eligible.
- Durable reservation precedes source verification/provider dispatch. Source verification runs
  off the input thread. Cancellation while verification is pending launches nothing. Account,
  login, home, model and cwd are rechecked before launch.
- Native results are hash-verified, bounded and marked untrusted. The graph stores identities,
  not result text. Missing/stale sources pause visibly. Referenced job metadata is retained.
- Existing v2 ledger recovery pauses unfinished tasks and dispatches nothing. Explicit retry
  creates a new task; old edges never silently follow it.

No managed worktree, file ownership registry, automatic integration, expanded native approval
transport or real-provider compatibility claim is included. Those remain roadmap phases.
