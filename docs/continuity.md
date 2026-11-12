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

`context.rs` owns pack planning and the binding decision, and `tui/run.rs` prepares
submitted turns. `tui/continuation.rs` coordinates bounded retry using `routing.rs`. Its review state
prevents automatic replay after uncertain tool completion. [State ownership](tui-states.md).

Verify using the context/continuity unit tests and `tests/control_plane.rs`; live provider evidence
remains scoped to the versions and requests in [verification status](e2e-status.md).
