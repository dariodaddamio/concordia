# Windows write guard: mechanism proof and product decision

Status: local prototype, 2026-11-04. No production provider guard is shipped or enabled.
Managed worktrees and reservations remain cooperative; the existing native provider permission
modes keep their documented behavior.

The remaining OS-confinement implementation now has a working Windows AppContainer mechanism.
An isolated full Rust fixture exercised a fresh GUID-named profile with `internetClient` and an
additive package-SID Modify grant on its owned workspace. Its results were:

- Workspace create/write succeeded; sibling creation, overwrite and ACL changes failed with
  Windows error 5. No outside test file was created or overwritten.
- BCrypt random generation returned success and public TCP connection to port 443 succeeded.
  This is not a TLS, authentication or provider-model qualification.
- Closing the broker's kill-on-close job terminated the parent and an already-running descendant.
- The medium-integrity workspace needed no integrity-label change. The existing user home DACL
  hash was unchanged. All four temporary AppContainer profiles were deleted successfully.

A final separate profile ran an exact unmodified copy of the installed native Codex executable
with a fake owned `CODEX_HOME`. `--version` exited zero and reported `codex-cli 0.166.0` through
explicit inherited standard handles. Source/copy SHA-256 was
`9d563ee4e0441ff9d59d612013e0460ddf8e5709735c2d9f6b98b7ef8ca4eb42`.
This qualifies startup/version output only. The subsequent help capture hit a fixture console-
encoding error, so no complete help receipt is claimed. No provider authentication or model
processing is qualified by the version check.

The earlier `WRITE_RESTRICTED` token passed basic file checks but failed DLL initialization once
the fixture linked crypto. It is not a valid fallback. One AppContainer attempt failed with a
minimal synthetic environment; inheriting the planned environment resolved the startup failure.
No environment values, credentials, model turns, firewall rules or loopback exemptions were used
or copied into the evidence. No real project or provider-home permissions were changed.

Exact local proof source and result are under ignored `target/sandbox-proof`:
`appcontainer_profile.py`, `probe_appcontainer.rs`, `appcontainer-profile-result.json` and
`production-plan.md`. These are research artifacts, not a production adapter or passing CI claim.

## Decision before production integration

The owner has been asked whether the optional guard should cover only Concordia-managed
worktrees/accounts, existing projects/provider folders too, or remain delegated to native provider
permission modes. Existing-folder support needs explicit Windows ACL grants, runtime read access
and crash cleanup. The prototype does not authorize those real-folder mutations.

A production adapter needs exact path review, a default-off policy, bounded inventory, durable
grant intent and owner/session identity, and a private broker that preserves native argument
vectors, environment, stdin/output and cancellation. It must start the child suspended, whitelist
only its standard handles, assign its job before resuming and fail closed without unrestricted
fallback. Environment values must not be persisted in launch manifests.

Read/execute grants can be necessary for user-installed runtimes. Credential Manager/DPAPI,
native CLI helpers, local HTTP MCP/proxies and actual provider authentication are unqualified.
No loopback exemptions should be silently installed. Protected descendants remain protected.
ACL propagation can traverse existing trees, so activation must not block the UI or imply a hard
cancellation guarantee. Cleanup removes only the lease SID's ACEs and preserves other changes;
deleting a profile alone does not remove all explicit ACEs on child-created files.

These implementation, permission and qualification decisions remain open. A successful fixture
does not close the roadmap's native filesystem-confinement workstream.

Platform contracts: [Microsoft AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)
and [job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
