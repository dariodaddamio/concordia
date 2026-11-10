# Concordia work plan

Current phase: cleanup closeout, documentation rewrite and product interview (2026-11-09).

## Current instruction

Feature roadmap execution is paused, including the dockable workspace. Finish cleanup, then
rewrite the documentation while using grill-me to settle Concordia's purpose and direction.
Do the factual consolidation and product interview together. Publication remains on hold.

## Cleanup closeout

The audited implementation and its previous documentation are preserved in local commit
`6fea4a9`. This is a preservation checkpoint, not full release qualification.

- One worktree remains. The current work branch and `main` are retained. No obsolete build cache,
  disposable repository archives or unused installer images were found in the final inventory.
- The installed build is 2026-11-09g. Its executable identity and scoped checks are recorded in
  [verification status](docs/e2e-status.md).
- The older executable still mapped by an open window is retained. Four synthetic Temp folders
  previously blocked by automatic approval review remain a recorded exception; no alternate
  deletion route is authorized by this plan.
- Native histories, credentials, drafts, queues and unique recovery records are preserved.

Retained-snapshot inspection, broader provider/host qualification, native hook enforcement and
missing provider signals remain unfinished roadmap work. They are paused, not cleanup deletions.

### Remaining cleanup actions after restart

These are recorded exceptions, not a claim that every leftover has already been deleted.

1. Close all older Concordia windows or reboot, then reopen the installed 2026-11-09g build.
   In File Explorer, open `%LOCALAPPDATA%\concordia\bin`. Remove only
   `concordia.exe.old-21228-1794268162761637800` and its matching `.receipt.json` after the old
   process exits. The image is currently locked by a running process. Leave `concordia.exe`
   and the install markers intact. Startup cleanup keeps the latest rollback image, so restarting
   alone need not delete this final copy.
2. In File Explorer, open `%TEMP%` and remove these four synthetic recovery-test directories:
   `concordia-09c-recovery-3ybkffdr`, `concordia-09c-recovery-lpvtnubq`,
   `concordia-09c-recovery-wpseb7db` and `concordia-09c-recovery-z8y6ft08`.
   Their combined file size was rechecked as 56,889 bytes. Automatic approval review previously
   rejected agent deletion as "blocked by policy" without further detail. This is not evidence
   of a Windows permissions problem; manual deletion is the remaining user action. Do not remove
   similarly named directories or any real provider home as part of this step.

## Documentation and product interview

Concordia currently provides a local Windows interface for the official Codex, Claude Code and
Cursor CLIs, with conversations, source retrieval, workspaces and worker coordination. Providers
own authentication, native sessions, tools and approvals. Product direction must distinguish
these implemented contracts from future ambitions.

Grill: decide whether Concordia primarily owns the complete coding workspace, coordinates AI work
alongside an editor, or serves as a general AI workbench. The previous working direction was a
fast TUI IDE across providers; it is not a claim of a finished IDE or proven cost/speed advantage.
Record settled decisions before writing definitive product promises. Continue consolidating
implementation facts while awaiting the answer.

Rewrite requirements:

- One clear user entry point and one canonical page per subject. Remove repeated explanations,
  superseded plans and copied historical journals from the working tree after preserving them in Git.
- Keep practical commands, source/approval/recovery boundaries and honest provider limitations.
- Retain licenses, notices, provenance, required policy files and the runtime changelog contract.
- Keep current verification concise; Git preserves the detailed pre-rewrite history at `6fea4a9`.
- Keep the paused [backlog](docs/plans/README.md), including the accepted dockable-panel request.
- Repair links and code/tooling documentation references when consolidating pages.

## This slice

### Documentation and direction - anti-slop gate

| Paths | Checks | Ownership |
| --- | --- | --- |
| Root Markdown and docs Markdown | 1, 4, 6, 10 | Consolidate current facts; remove duplicate pages rather than add another documentation layer. |
| Documentation links in code and scripts | 5, 9 | Repair references only; feature implementation remains paused. |
| Legal, notices, policy and generated files | 2, 3 | Preserve required contracts and provenance; inspect dependencies before removing anything. |

Acceptance: current docs are concise and consistent; all local references resolve; documentation
checks pass; purpose and scope reflect the owner's answers. Native provider ownership, recovery,
approval and privacy rules in [AGENTS.md](AGENTS.md) remain in force throughout the rewrite.
