# Duplication and current-chat file audit - 2026-10-31r

This is a scoped source and deterministic-test audit, covering the flows below. It does not prove
zero duplication across every provider version, physical terminal, or arbitrary event ordering.
Provider-native logs and intentional repeated user requests are preserved.

## Findings and corrections

| Flow | Finding and resulting behavior | Evidence |
| --- | --- | --- |
| Current-chat file discovery | Canonical file identity collapses path aliases, Windows case variants, links and line references. Discovery keeps the preview location without walking the repository. | `related_files::tests::discovery_deduplicates_aliases_and_locations_and_only_uses_chat_references` |
| Current-chat preview | `/open` lists current-chat references; `@` mixes files and chats. Tab/Enter inserts a reference; Ctrl+P or click previews it without dispatching the draft. | `chatref::tests::current_chat_files_complete_and_preview_without_dispatch_or_draft_loss` |
| New/resumed conversations | Reset the file index on conversation replacement; discard results from an earlier discovery generation. | `related_files::tests::late_discovery_cannot_leak_files_into_another_conversation`, `chatref::tests::file_picker_deduplicates_and_new_conversation_clears_its_index` |
| Missing sources | Deleted references fail visibly; a same-named file elsewhere is never substituted. | `chatref::tests::deleted_suggested_file_is_unavailable_without_opening_a_same_named_file_elsewhere` |
| Suggestion arrival | New file results preserve the selected chat identity instead of changing what Enter would choose. | `chatref::tests::adding_file_suggestions_keeps_the_selected_chat_identity` |
| Completed tool replay | An exact card completion replay in the same turn returns before duplicate logging. The same ID in a different turn remains a separate action. | `tests::replayed_tool_completion_is_logged_once_but_reused_ids_in_new_turns_are_preserved` |
| Complete provider snapshots | Exact identified snapshots deduplicate within 128 entries / 4 MiB per turn. Terminal usage counts once; distinct IDs, partial text and repeated deltas remain intact. | `provider::turn::replay_tests` |
| Handoff request fields | Identical first/latest requests were emitted in both `asked` and `now`. Omit only the redundant `now` field; retain source events and their count. | `threads::tests::map_omits_repeated_request_field_and_nonadjacent_identical_errors` |
| Handoff errors | Adjacent-only deduplication missed a repeated error separated by a different one. Keep one copy of each displayed error, in original order. | same map regression |
| File/chat ambiguity | Path-shaped hexadecimal references stay literal. Identical unresolved-reference notices display once. | `chatref::tests::file_paths_with_hex_prefixes_are_never_expanded_as_chats_and_notices_are_unique` |
| Active-chat self-reference | The current recorded conversation is omitted from related-chat suggestions; other chats retain their explicit IDs even when titles match file names. | `chatref::tests::current_conversation_is_not_offered_as_its_own_related_chat` |
| Repeated chat references | Multiple references to one stored chat already expand its map once. Preserve those explicit references in the user's text. | existing `chatref::tests::at_most_three_references_and_repeats_expand_once` |
| Native resume | Same provider/home with an explicit session uses native history without adding another context pack. | existing `context::tests::native_omits_pack` |
| Streamed replies and tool output | Existing tests check streamed commentary/final-answer replay, one reply heading per group, and repeated tool-output chunks. | Existing TUI stream-order, reply-grouping and stream-card replay checks; exact names below. |

Stream-order evidence includes `tests::streamed_commentary_and_commands_keep_arrival_order_without_duplicate_final_reply`,
`tests::tool_cards_stay_grouped_under_one_reply_label`, and
`stream_cards::tests::replaying_the_same_output_does_not_duplicate_it`.

## Bounds and intentional distinctions

Discovery considers at most 128 recent in-memory sources / 256 KiB, resolves at most 512 candidates,
and returns at most 64 existing files. It includes user/assistant text and tool-card detail, excludes
automatic credential/environment/key suggestions, and refreshes on changes or after five seconds.
It does not search unrelated project files, infer that a mention is an actual edit, or attach file
contents to provider prompts. File contents are read only through an explicit preview/open action.
The existing 3-pane, 2 MiB and 50,000-line preview limits still apply.

Repeated user instructions, the same operation in separate turns, and distinct messages with
identical wording are real history. Account identity in a status bar and in a historical reply
label has different scope. Current-account quota, all-time token totals and per-turn token counts
are different measurements. These are retained rather than collapsed by text similarity.

## Remaining planned work

The canonical roadmap remains [PLAN.md](../../PLAN.md), with acceptance in
[context-handoff-plan.md](../plans/context-handoff-plan.md). Suggested order:

1. Native-session source pointers and exact, freshness-checked readback. This supplies reliable
   history even when optional Concordia transcript recording is off.
2. Explicit handoff lineage and pinned decisions/goals, then related-chat discovery. Keep verified
   ancestry separate from lexical suggestions.
3. Durable task/run recovery and target eligibility explanations. Preserve the interrupted request
   across restart and show why each fallback is unavailable.
4. Context-capacity rollover after source readback and lineage are proven. Current quota fallback
   is separate; capacity and compaction support must be verified per provider/version.

Internal editing and managed worktrees remain later workspace slices. A useful small preview
follow-up is explicit reload with a changed-on-disk indicator and in-pane search.

## Verification

Build `2026-10-31r` is installed locally. Clippy with warnings denied and formatting pass.
The current-chat preview journey passes against that installed release through Windows ConPTY,
using isolated homes/state and fake providers. It checks suggestion, formatted preview, raw toggle,
draft preservation, completion, and existing-pane reuse; its journal contains one initial fake task
and no unintended sends. The ordinary library/integration checks and installation smoke status are
maintained in [the current E2E record](../e2e-status.md).

## Follow-up in 2026-10-31s

The first native-pointer/readback slice, unfinished-request recovery, exact file-backed Harness
pins, reactive capacity-error rollover, and preview search/reload/change indication are now built.
[Native memory](../native-memory.md) defines supported providers and bounds. Handoff boilerplate
is emitted once for all sources, same-home/session aliases are collapsed in the source directory,
and identical pinned rules are delivered once. Missing originals are explicitly labelled to both
the user and the next provider; no history or completed action is invented. Whole-history indexing,
related-chat discovery, broader worker ownership and proactive rollover remain planned.

The installed `2026-10-31s` release passes five isolated fake-provider terminal journeys covering
native handoff, restart recovery, capacity rollover, pinned-rule delivery and current-chat file
preview. Source-directory and pinned-rule deduplication also have focused unit coverage; this
does not establish that every possible provider output is free of repeated information.


## Audit remediation in 2026-11-14p

The two follow-up passes used baseline `a35932b`. Their identified defects and stale fixtures
are addressed by the following canonical owners. [Current qualification](../e2e-status.md)
distinguishes source inspection, deterministic flows and the two small native Codex turns.

| Findings | Corrected owner and behavior |
| --- | --- |
| F1 | Native terminals bind conversation, workspace, provider, account, home and native session; local shells cannot receive native commands. |
| F2, F8 | Queue resume respects native approvals; cancellation preserves independent drafts. |
| F3 | Handoff context stays available until a provider launch handle is accepted. |
| F4, F5 | Structured quota and terminal context errors reach continuation; context rollover occurs once with source pointers. |
| F6, F7 | Final worker cards upsert by ID; unconfirmed running cards do not become successful. Invocation begins with the owned child handle. |
| F9 | Nested source-reference checking uses exact historical edges and explicit history-extraction migrations; broad module exemptions are removed. |
| F10, F11 | The approval attention check runs and the stale resolved item is withdrawn; unreachable editor branching is removed. |
| F12 | Existing sent-history fixture closes inspection, dispatches prompts and uses valid distinct run IDs; two references survive restart. |
| G1, G2 | MCP discovery and execution enforce effective project policy; ID/record boundaries reject traversal, links and oversized files. |
| G3, G4, G9 | Proposals claim only their parent/workspace/run; binding and recovery save precede launch; completion requires exact agent/run IDs. Rejections are terminal and visible. |
| G5, G14 | Invalid configuration stays unchanged; UI registry edits serialize read-modify-write with atomic publication. |
| G6, G7, G8 | MCP initialization, ID matching, HTTP sessions/SSE, framing, deadlines, traffic budgets and child cleanup share owned transport paths. |
| G10, G11 | Recovery help points to reachable controls. Same-provider automatic handoff requires consent and enabled policy; daily admission is atomic. |
| G12, G13, G15 | All provider projections use server-bound proxies; references resolve only for the intended child/request. Redaction contains no value-derived fingerprint. |
| Effort | Foreground and worker turns carry effort. Codex standard/app-server and supported Claude flags receive it; ACP providers retain native variant IDs. |

`agent_records` owns durable lifecycle records; `native_history` owns bounded readback below CLI
discovery; `text_safe` owns shared text boundaries. Provider-specific parsers stay separate where
their protocol semantics differ. Identical task text is not a run identity or a deduplication key.

The static gate retains historical cyclic edges; it does not claim the repository is acyclic.
OAuth and broader native-provider qualification remain outside this repair evidence.
