---
title: Model effort and update feedback
slug: model-effort-update-polish
status: completed
objective: Make effort selection discoverable and update stages readable with inspectable warnings.
stop_condition: Model-to-effort keyboard/mouse flow, cancellation/persistence and update warning/progress states pass actual UI checks.
settled_decisions: Use provider-reported effort choices and existing native model IDs; show warnings without replacing progress.
scope: Model/effort selection, header cursor feedback and in-app update progress/diagnostics.
dependencies: Existing model catalog, JobControl and native-window test channel.
slices: Effort interaction, update feedback and qualification.
assigned: Root Codex UI; Luna medium update backend; independent review seats.
proof: Existing focused checks and isolated native-window journeys; no real installation needed.
completion_checklist: Record actual proof, review outcomes and archive the plan.
owned_paths: crates/concordia/src/update.rs, crates/concordia/src/tui/, crates/concordia/src/commands.rs, docs/, README.md, CHANGELOG.md, version.rs
forbidden_paths: Authentication, provider histories, global hooks, live installation and publication.
---

# Model effort and update feedback

## Slices

| Slice | Status | Owner/model | Owned paths | Forbidden paths | Proof/report |
| --- | --- | --- | --- | --- | --- |
| Effort interaction | complete | Root Codex | model picker, command routing, pointer | provider authentication | Native journey: model to effort; Escape preserves selection; direct command; persisted pair; header pointer |
| Update feedback | complete | Luna medium backend; root UI | update.rs, release.rs, update preview | live installation | 54 update checks and 19 isolated release checks; real HTTP cancellation; bounded diagnostics |
| Qualification | complete | Independent process/architecture/UI/logic reviewers | read-only | source edits | Focused checks and visible artifacts |

The model/effort pair commits together after an effort choice. Esc cancels the uncommitted choice.
A model without reported effort choices commits directly. `/effort` reaches the same choices and
`/effort LEVEL` accepts only a current provider-reported value. Header affordances use the existing pointer owner.
Update progress shows actual stages and preserves diagnostics separately; it never invents percentages.

## Completion checklist

- [x] Verify the requested effort flow and pointer feedback
- [x] Verify progress/warnings/error/cancel and readable narrow layouts
- [x] Update canonical docs and build identity
- [x] Resolve independent review findings
- [x] Archive plan and remove dispatch

## Qualification record

Process steward, architecture, craft/UI and logic/tests independent seats passed.
The architecture review found raw JSON in failed-build errors and delayed release cancellation;
both were repaired and re-reviewed. Craft review added reserved effort space in narrow headers.
The final narrow-page check added Home/End navigation and verified diagnostic scrolling.
Current evidence and limits are recorded in [Verification](../../e2e-status.md).
No real installation or publication occurred.

## This slice

### Model effort and update feedback - anti-slop gate

| path | tendency_check | justification |
| --- | --- | --- |
| crates/concordia/src/tui/app/, crates/concordia/src/tui/input_dispatch.rs, crates/concordia/src/tui/pointer.rs, crates/concordia/src/commands.rs | ok: 1, 2, 3, 4, 5, 6, 7, 9, 10 | One transactional model/effort choice and one cursor owner; fix the user's reproduced flow. |
| crates/concordia/src/update.rs, crates/concordia/src/release.rs, crates/concordia/src/tui/draw/, crates/concordia/src/tui/update_ui.rs, crates/concordia/src/tui/mod.rs, crates/concordia/src/tui/native_window.rs | ok: 1, 2, 3, 4, 5, 6, 7, 9, 10 | Keep real progress and diagnostics in one bounded snapshot, using existing update ownership and pointer state; presentation only reads it. |
| crates/concordia/tests/, crates/concordia/src/tui/tests/, crates/concordia/src/tui/screenshots.rs | ok: 1, 3, 5, 9 | Exercise reported model/effort, cancellation and update feedback cases through current flows; reuse the existing buffer-to-SVG evidence renderer. |
| README.md, CHANGELOG.md, crates/concordia/src/version.rs, docs/ | ok: 1, 4, 6, 10 | Record implemented controls, exact proof and qualification limits in canonical documents. |

All ten tendencies considered. No new timeout policy or reusable exemption is introduced.
