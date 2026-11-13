# Conversation continuity

Concordia's context planner decides whether a submitted turn can reuse a native session.
The provider's own session and approval state are never inferred or copied across identities.

| Target binding | Behavior |
| --- | --- |
| Same provider, matching normalized home and explicit session ID | Resume that native session; omit the handoff pack |
| Different account home or provider | Start a new native session; attach bounded source context if available |
| No valid session ID | Start a new native session; include context only when the conversation requires handoff |
| New conversation | Clear prior binding and do not attach the old conversation |

Account/model selection prepares local bindings. Context is built and delivered when a message is
submitted, not while browsing targets. The handoff pack uses deterministic map v2, constrained by
the planner's fixed 1,600-character map ceiling. Explicit chat-reference maps use the separate
configured map budget. It contains typed intent and source pointers; it does not transfer
native tool state, approvals or a complete transcript. [Map format](continuity-map-v2.md).

Concordia records exact source bindings and checks revisions for readback. Optional thread logs,
Native-source pointers have bounded availability; a missing source
must be reported rather than replaced with another chat. [Native memory](native-memory.md).

## Unopenable chats

Task-backed chats are diagnosed before Concordia replaces the current tab. The diagnostic reuses
the native pointer causes: file deleted or moved, file replaced, record rewritten or truncated,
header changed, account home changed, record too large, session rejected by the provider, and
record unreadable. `/resume` presents the cause and one repair action. The action starts a fresh
native session and sends the stored deterministic handoff map with the next prompt; it never
replays the old provider session. The composer draft remains intact.

`/doctor` reports the same failed chat rows and repair action. A missing or damaged handoff map is
reported as `record unreadable`, and Concordia keeps the original state for inspection.

`concordia chat repair --dry-run` reports the same damaged task records without writing. The repair
command writes each changed task to a dated backup under `.ao`, removes a task only when its native
record has no user message, and materializes a current-format handoff journal at the existing pointer
when native content exists. A missing native source remains unreadable. Running the command again
makes no further changes.

`context.rs` owns pack planning and the binding decision, and `tui/run.rs` prepares
submitted turns. `tui/continuation.rs` coordinates bounded retry with an explicit account picker. Its review state
prevents automatic replay after uncertain tool completion. [State ownership](tui-states.md).

Verify using the context/continuity unit tests and `tests/control_plane.rs`; live provider evidence
remains scoped to the versions and requests in [verification status](e2e-status.md).
