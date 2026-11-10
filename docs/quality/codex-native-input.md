# Codex native input

Engineering slice P2, based on main `196bc67`. Explicit `/native ACCOUNT on` selects
Codex app-server for future foreground turns and newly approved workers in that isolated home.
The standard text adapter stays the default. `/native ACCOUNT off` restores it. Running or
paused worker attempts retain their captured transport. No global provider configuration changes.

The adapter uses the unmodified native CLI over stdio. Its current protocol evidence is the
installed `codex-cli 0.166.0` and its generated JSON schema, inspected without a model turn.
Other versions fail visibly before prompting. Windows npm resolution requires the existing
verified stock bridge; custom batch launchers are refused. The implementation adds no runtime,
SDK, credentials reader or authentication service.

Initialization verifies the returned account home. Thread creation/resume verifies the exact
saved thread ID, execution directory and requested approval policy. Approval requests remain
unavailable during history loading. Early turn events are buffered until turn/start returns its
exact turn ID. Foreign thread/turn requests receive errors. Stream closure before a matching
terminal event is a failure, with no print fallback or automatic replay.

09e separates the transport frame budget from diagnostic/display lines: a complete JSON frame
may contain up to 64 MiB, with one queued frame and at most 64 MiB/256 early events. Larger frames
fail explicitly and are never parsed as truncated requests. Large-frame parsing runs off the
async executor; text progress is split at UTF-8 boundaries into at most 64 KiB per event. Tool
cards retain their existing 64 KiB display bound, and native request payloads keep their own
unchanged ownership/byte limits. Original native session data remains authoritative.

A confirmed provider exit does not establish that an interrupted tool completed. If its tool
completion is missing, the reservation stays retained for inspection and explicit reconciliation;
the warning distinguishes this case from an unconfirmed provider exit. No failed request is
automatically replayed.

Trust comes from the final enforcement plan: read-only remains read-only without escalation,
workspace mode requests native user approvals with workspace writes and no default network,
and explicit full trust retains its existing meaning. Local compaction overrides remain process
arguments. Image bytes are hash-verified and sent as native data URLs over stdin.

Command/file approval choices preserve native decision payloads: once, session, decline/cancel,
or an explicitly offered saved command/network policy. Policy choices visibly say they save a
rule and show its native payload. Review cards retain the command, directory, reported file diff
and native request. F8 explicitly focuses choices; opening review cannot grant permission.
Bounded questions preserve question IDs and answer labels. Build `2026-11-03r` also supports
free-form-only questions and an Other reply when the native question offers it. F8 focuses
foreground/worker input; typing/paste goes to the reply field, Shift+Enter adds a line and Enter
submits. Offered choices can include a typed note. Replies are capped at 4096 UTF-8 bytes with
control filtering; oversized paste retains existing text. Permission decisions never accept text.
Secret-input requests remain visibly unsupported; no answer or credential is invented. Unsupported
permission-profile requests, elicitation, dynamic tools and authentication callbacks receive
errors. Native authentication remains the CLI's responsibility.

Eight live requests fit each connection; the existing aggregate byte and application limits
also apply. Foreground requests queue without replacing an unanswered choice. Worker review
keeps exact conversation/run/attempt/account/home/workspace/thread/turn/connection/request
ownership. Native resolution and matching terminal events invalidate pending choices without
recording user approval. Cancellation or connection loss invalidates channels; uncertainty
retains reservations. All native-mode workers pause rather than automatically failing over.

Command, edit, MCP and search items have meaningful labels. Native subagent events enter the
same retained agent inspector as the standard Codex adapter. Only matching current-turn events
reach the transcript. Cumulative thread usage and the last model-response usage are not presented
as complete per-turn totals. Existing verified native-source accounting and quota/credit readers
remain authoritative; unsupported measurements stay unknown.

Protocol source: [official OpenAI app-server documentation](https://learn.chatgpt.com/docs/app-server).
The native response shape is also checked against the generated installed protocol and the
official [question types](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/request_user_input.rs)
and [native reply handling](https://github.com/openai/codex/blob/main/codex-rs/tui/src/bottom_pane/request_user_input/mod.rs).
Text replies use the native `user_note:` convention, not a synthetic offered-option identifier.
Synthetic process fixtures exercise exact resume, image payloads, decline, questions, early
events, provider resolution, foreign thread/turn rejection and premature closure. These are
fixture results, not live-provider or physical-host approval parity. No real model turn was run.

Build `2026-11-03g` passes all 983 enabled checks and all-target Clippy. The immutable release
candidate passes release smoke: 19,877,376 bytes, SHA-256
`ab64db60582d95aa528b018687867f8720d865daf8f28c68cac8b6b4dfc29ed4`.
Installed terminal and remote CI evidence are recorded separately when completed.

Text-reply evidence: the existing native foreground/worker fixture covers mixed questions, exact
responses, explicit focus, bounded/unarmed paste, multiline answers, unchanged main ownership
and draft, and refused secret/malformed flags. The real fake-process fixture covers native
free-form RPC responses. Logs: `target/native-text-r-{proof-final,process,clippy-final,release}.log`.
These checks do not establish live native-provider parity.

## Scoped 05c native evidence

Production-transport checks qualify Codex offered cancellation and Claude structured Review
answers with wrong-owner and duplicate-response rejection. The cancelled command was not granted;
sentinel files and the main selection stayed unchanged. These helper-driven calls do not establish
physical worker-window parity. A no-model worker-window rehearsal stopped at its fake executable's
unverified version gate before issuing a native request; no fallback or live retry was started.
Original driver failures and separate manual corrections are retained in
[the current evidence record](e2e-history.md#scoped-native-questions-cancellations-and-pdf-reading-on-05c).
