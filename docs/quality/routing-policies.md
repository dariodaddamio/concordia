# Local routing and approved fallback policies

Baseline: merged `ea4377d`. Production source: `6d637dd`. Build `2026-11-02p`.
State: fixture-verified and portable-installed-verified. Live native-provider evidence is separate.

Repository audit 2026-11-08: approved evaluations now preserve the captured sequence while
rechecking eligibility and binding. The prior implementation accidentally reapplied quota/tier
sorting during every evaluation, contrary to the ordered-policy contract. Automatic candidate
discovery retains its ranking before approval; this correction does not grant a new target.

## Shared decisions

Main continuation, worker selection and worker quota failover use the same local evaluator.
Eligibility is separate from capacity. It reports missing accounts/homes/login markers,
unenrolled workers, orchestrator exclusion, duplicate homes/logins, busy/full queues, attempted
targets and unavailable explicit models. A cached quota observation includes its time and source;
it never establishes known available capacity or verifies the freshness of the native login.
Inspection starts no provider, network request or model turn.

`/delegate routes` inspects future worker candidates. `/continuation routes` inspects the current
approved main sequence, or the future sequence before a request starts. Each snapshot includes
opaque home identity, observation time and policy revision. One background inspection runs per
app; late results from an earlier conversation are discarded. Native metadata reads are bounded.

`/delegate routes ID` and the existing worker detail view show its captured policy and recorded
failover decisions. Automatic main continuation shows the selected and skipped targets in chat.
An inspection is a local observation, not a reservation: dispatch rechecks registrations, login
presence and explicit models. A changed captured account/home/login binding blocks that target.

## Ordered overrides and approval ownership

Use `/delegate policy set WORKER[:MODEL] ...` or
`/continuation policy set ACCOUNT[:MODEL] ...` to save an ordered sequence for future requests.
`policy` reads the saved override; `policy reset` clears it and uses configured ordering.
Models are exact native IDs; an absent/unavailable explicit ID is skipped, never substituted.
Policy editing does not enable automatic continuation or worker failover.

The typed version-one override is stored transactionally in the state directory's
`routing-policy.json`, separately from the account store. `config.toml` remains read-only.
The file is limited to 64 KiB; each sequence has at most 32 distinct account/model pairs,
with 128-byte account IDs and 256-byte model IDs. Unknown/corrupt/linked/oversized policy files
are preserved and automatic routing fails visibly. An invalid policy is not silently replaced
by the legacy fallback. Explicit ordinary main requests remain available.

Without an override, existing worker order/model settings and main config order are retained.
A session's explicitly chosen preferred account takes precedence for main handoff. An explicit
manual worker account takes precedence over automatic ordering, retaining its configured model.
Choosing a new session handoff mode/account explicitly rebinds that session's route snapshot.

Approval captures the candidate sequence, revision, owner and opaque account/home/login bindings.
Policy edits affect future requests; they do not silently retarget approved or queued work.
Worker failover requires both approval-time opt-in and the current global opt-in; disabling it
still stops further automatic failover. Removing worker enrollment also prevents a queued launch.

## Recovery and execution limits

Worker ledger v4 migrates v1/v2/v3 metadata and retains the approved sequence and bounded decision
history. Older v3-only executables reject v4. Main unfinished-request v2 adds its route snapshot
and reads v1 as paused legacy recovery. Recovery starts no provider and transfers no native
approval state. Unknown or invalid metadata is retained for explicit reconciliation.

The existing bounds remain: one running turn per worker account, two queued tasks per worker,
four worker launch attempts, at most three main handoffs/four attempted pairs, and one explicit
context-error rollover. A different explicit model on the same account can be a distinct approved
candidate. An attempted pair is not automatically replayed. Cancellation, unknown completion,
tools/partial worker output and non-quota worker errors still prevent automatic failover.
Trust and native approval behavior are unchanged; this slice does not create filesystem isolation.

## Verification

Existing continuation and worker fixtures exercise saved ordering, future-policy edits during an
approved task, unchanged config/draft/paused queues, replaced home bindings, corrupt policy
preservation and failover decision recovery. The provider runner uses only fake executables.
All 983 enabled tests (885 library checks) and all-target Clippy with warnings denied pass.
The worker fixture runs two distinct model attempts on one account before another account,
counts all three attempts once, and preserves the captured sequence and decisions after recovery.
Main fixtures verify future-policy precedence, unchanged config/drafts/queues, replaced bindings
and corrupt policy preservation. Release build/smoke, portable everyday smoke and OSS/privacy gates
pass. All 48 ConPTY journeys pass against the exact portable-installed release in 290.32 seconds,
including policy editing/inspection without provider turns and the existing selected-account handoff.
Size: 18,098,688 bytes (17.26 MiB). Release and installed SHA-256 match and are unchanged afterward:
`7E9DC3A9939E120D90E7CECCE263072FE96EDF681C1F48FADFFD5218E05CA914`.
No real model turn ran. Remote CI remains a separate merge gate.
