# Orchestration and latency

Main continuation and workers share [local routing policies](quality/routing-policies.md),
including skipped-target reasons, ordered overrides and approval-time identity bindings.
Inspection never probes provider capacity. Policy edits affect future tasks.

Concordia runs the unmodified native provider CLIs. It does not run a second model to relay each
reply. Standard turns start a native CLI process and consume its structured output directly;
Cursor images use its ACP connection. Process startup, native context loading, tools, model
latency and Concordia rendering are separate costs. No live speed or cost advantage has been
established. Persistent native connections may reduce startup, but require full permission,
cancellation and recovery support before replacing the existing adapters.

## Fast local actions

Windows text copy, cut and paste use Unicode clipboard APIs, without starting PowerShell.
Clipboard contention retries stop after three 5 ms waits. A failed cut retains text and selection;
a successful cut says `cut N chars`. Image capture/pickers remain background helpers. Native image
format detection avoids starting them for ordinary text paste. Active provider/worker channels
have their input-poll wait capped at 33 ms instead of waiting for the 200 ms animation tick; painting and
scheduler work can still add time. Completed diagrams keep their wrapped rows when an earlier
message changes. At most 16 diagrams render per message; extra diagrams retain source.

The local event log includes `provider-timing`: setup, process spawn, first stdout event, parser
time, event count and total duration for standard turns. These contain no prompts or replies.
First event is not necessarily the first answer token. ACP timing and a controlled native CLI vs
wrapper benchmark remain open. Synthetic timings cannot establish provider inference speed.

## Separate main and worker models

The current account/model is the main agent. Register another native account with `/workers add ACCOUNT`.
Providers can differ. `/delegate model ACCOUNT` lists that worker's locally recorded model ids;
`/delegate model ACCOUNT MODEL` persists its model for newly submitted jobs, including approved
proposals. `default` restores the native default. Running/queued jobs retain the selection captured
when submitted. Catalog ids are validated; models are never invented or silently substituted.

`/delegate ACCOUNT TASK` starts a fresh native worker session with bounded context. Up to three
workers can run concurrently; each has a queue of at most two. The strip and Ctrl+G browser show
model/state, and inspection shows task, trust, session, output and result-delivery state. Different
concerns should own different files: shared-folder workers have no automatic merge isolation.

`/delegate collect` sends up to three new successful results together, capped at 3000 result
characters each. `/delegate use` can select/resend a result with the larger existing cap. Both
preserve the unsent composer. `/delegate returns auto` opts into collecting when workers and the
main agent are idle, no command/job/queue edit is open, queued user work is absent, and no quota
handoff waits. `/delegate returns manual` disables it. Automatic handbacks consume a main-agent
turn. Result dispatch uses normal checkpoints/context/quota recovery and labels output as
untrusted data. Delivery is reserved durably before dispatch. Opening another
conversation prevents old workers from posting/automatically returning into it; Ctrl+G retains
inspection. Native approvals are never transferred. Orchestrator proposals still require approval;
worker output cannot spawn further workers.

This is a bounded supervisor/worker loop. Pane-owned worker ledgers, paused restart recovery and
opt-in bounded quota failover are implemented; see [recovery controls and limits](delegation.md).
Approved shared-workspace task dependencies are implemented with exact-run receipts and verified
result pointers; see `/delegate after` and `/delegate graph` in the recovery controls. File ownership
enforcement/worktree isolation and external native-session usage import remain open. New main and
worker attempts now share [attributable accounting](quality/worker-accounting.md), with bounded
background reads, reported-counter/cost coverage and idempotent recovery.
Background workers cannot yet answer native interactive requests through a dedicated UI.
A stronger planner plus cheaper workers can improve economics when tasks are independent and
reviews stay short. Extra worker context, reviews and retries can also increase total usage;
Concordia has no measured token-saving claim yet.

## Provider exits and patch errors

`apply_patch verification failed` means expected text did not match the current file. It can
come from stale reads, a concurrent edit or an overly broad patch. It is not a quota signal and
does not establish the final reason a native provider exited. Concordia keeps these retained
stderr diagnostics in an inspectable card and explains that distinction. Generic failures keep
the request recoverable; uncertain edits must be inspected rather than automatically replayed.
Structured worker turn failures remain failed even if the CLI exits zero; an unconfirmed exit
is never labelled successful.

## Local transport probe, 2026-11-01f

Twelve alternating direct/wrapped fake-process launches measured 19.713 ms / 21.484 ms medians
and 24.115 ms / 24.875 ms p90. The approximately 1.8 ms median difference is one local sample;
it excludes native CLI context/auth startup, inference and the TUI. It is not a speed guarantee.
Reproduce with `cargo test -p concordia --test latency -- --ignored --nocapture`.
Evidence: ignored `target/latency-2026-11-01f-final.log`. No model turn is invoked by this probe.
