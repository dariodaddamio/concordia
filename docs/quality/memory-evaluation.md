# Synthetic retrieval evaluation and retention experiment gates

Corpus v1 is in `crates/concordia/tests/fixtures/memory-evaluation-v1.json`. It is fixed before
further ranking tuning and runs inside existing native-memory tests; it adds no test function or
production behavior. Ranking remains opt-in. These fixtures establish retrieval contracts, not
model recall superiority or a general retrieval benchmark.

## Reproduce

Run `powershell -File scripts/evaluate-memory.ps1`. The collector builds the existing library test
executable, then runs its nine native-memory checks directly with one test thread. It makes no
provider/model call and does not index user homes. Compilation is outside the measured run.
The output is a new ignored `target/memory-evaluation-UUID.json` artifact; an existing report is
never overwritten. `-Output PATH` selects another new output file.

The report binds corpus SHA-256, exact test executable hash, source commit/dirty state, compiler,
per-case precision/recall at the twelve-hit ceiling, first-result precision, excerpt resolution,
coverage flags, successful native read bytes, elapsed microseconds and retained serialized report
bytes. Native read counters include repeated source verification but exclude metadata, discovery
and index construction. Both arms search the same explicit recent page; this measures retrieval
after indexing, not whole-app discovery performance. Native record limits remain unchanged.

Serialized metadata measures retained report payloads and excludes allocator/container overhead.
The collector also samples the test process's native peak working set, including all nine checks;
this is not a per-query heap measurement. Latency and process memory are host observations and
do not establish performance on another machine. No raw native bodies, local paths or home IDs
are written to the report.

## Contracts and interpretation

The sixteen comparisons cover Codex and Claude: Unicode, explicit goal associations with no
keyword, historical file requests with no keyword, contradictory versions with an explicit
reference instant, malformed records, compaction markers, recent-window omissions and oversized
records. The time case tests declared ordering; proximity is not evidence that a version is true.

Every returned excerpt resolves to its original native record and hash. Each case also rejects
another account and withholds a rewritten source, then checks deleted-source unavailability.
Existing integrity checks additionally cover equal session IDs in different homes, rewrites beyond
the header, all 400 records in each provider's whole-history pagination and oversized history gaps.
The collector refuses missing integrity fixtures or an incomplete set of sixteen result rows.

Recall denominators describe relevant items inside the declared source page. The recent-window
case deliberately seeds a globally relevant item outside that page: neither arm can return it,
both must report incomplete coverage, and the whole-history checks establish separate reachability.
An empty scoped relevant set produces null recall rather than an invented perfect score.
Malformed/oversized records likewise require explicit incomplete coverage. Zero cross-account
substitution, zero stale acceptance and zero silent gaps are required assertions, not estimates.

The small hand-authored corpus intentionally exercises context features. It cannot justify a
default ranking change on its own. Broader relevance judgments, discovery-limit coverage and
retention/task-success evidence must be recorded separately before changing product claims.

## Initial local observation

The first successful run produced all sixteen rows and passed all nine native-memory checks.
Lexical recall returned ten relevant excerpts among twelve results (83.3% precision, 62.5% scoped
recall); opt-in ranking returned sixteen relevant excerpts among eighteen results (88.9% precision,
100% scoped recall). All thirty excerpts resolved exactly. Six of sixteen rows explicitly report
partial scope. These aggregate scores use the declared page scope and must be read with that
coverage rate. Both arms keep the same twelve-hit, one-page ceiling; context evidence differs.

Candidate corpus SHA-256:
`490e6483fd9bd8c015527d6c05e3e0ee3bbc635ea1059b010447bc678423d84f`.
Clean source `3d20be0` reproduces all sixteen rows and all nine native-memory checks in 9.837
seconds. Its test executable SHA-256 is
`3a347ee0fe3baffe2024d14222beda29c2a8f5ff152c43f8c598b85844a502c6`.
The observed process peak working set was 50,892,800 bytes; the largest retained serialized
report pair was 5,413 bytes. The slowest ranked query took 15,434 microseconds, with at most
196,759 successful native read bytes in a paired query. These values retain the measurement
scope above. All-target Clippy with warnings denied passes. Corpus line endings are pinned to
LF so its byte identity survives checkout. No ranking weights or defaults were tuned.

After the workspace-alias and source-update corrections, clean source `e29078b` reproduces
all sixteen comparisons, all thirty exact excerpts and all nine integrity checks in 9.143 seconds.
Its test executable SHA-256 is
`7082b85ef646f6de819204771da4dd20a5571fb05b0f0c7aa7bcf7402573a228`.
The observed process peak working set is 63,864,832 bytes and the largest retained serialized
report pair remains 5,413 bytes. Relevance and incomplete-scope outcomes remain unchanged.

## One authorized live handoff smoke, 2026-11-04

The owner authorized up to one synthetic model turn. The conservative allowance was **one turn
total**, and it is now consumed. A fake-provider rehearsal passed before the sole live attempt.
There were no automatic retries, account switches or additional provider turns.

Concordia indexed an isolated synthetic native history, built its normal handoff directory and
resolved three exact, hash-verified excerpts. Those excerpts were supplied to one fresh Codex
session through the existing provider-turn adapter with native read-only permissions. The selected
provider's existing default was preserved: the new native session reports **gpt-5.6-sol**, with
**codex-cli 0.166.0**. No user conversation was resumed, credential copied, or guard enabled.

All six response checks passed: prefer current port 7443 over superseded 4100; preserve draft 17
over old proposal 99; name only `notes.txt` for a future separately approved edit; decline unapproved
integration; decline replay of an already-applied action; and identify the exact source index.
The native process exited zero, reported no failed/cancelled turn, made zero tool calls and left
all three workspace files and an outside sentinel unchanged. Observed duration was 6.185 seconds.
Reported usage was **16,127 input tokens, 73 output tokens, zero cached tokens**; monetary cost
was not reported and is not inferred.

The private ignored receipt directory is selected by `target/live-handoff-current.txt`. It retains
the fixture, verified handoff, before/after outcome, native session/account binding, answer, usage
and hash-bound source/driver evidence. The original receipt's version probe returned the Node
launcher version; `provider-metadata.json` separately records the read-only CLI-version check and
the exact new session's model metadata. This correction used no additional model turn.

This is one **source-use/handoff smoke**, not a retention benchmark: the model received already
verified excerpts and performed no pointer-fetch tool call. It does not exercise native compaction,
compare experimental arms, measure token savings, establish another provider/model's behavior or
justify changing ranking defaults. Windows work and GitHub publication remain on hold by owner
instruction. Broader live experiments require a new explicit allowance.

## Authorized native smoke comparison, 2026-11-04

The owner authorized up to 18 additional subscription-only requests and an alternative Codex
account. The isolated run conservatively consumed 14 dispatch slots, including one question
request rejected before generation; four were unused. Codex CLI 0.166.0 used the account's
configured gpt-6.1-sol through its native ChatGPT Plus login. Read-only before/after metadata
reported zero credits, and the selected Concordia chat account was unchanged. No automatic
driver retry, account switch, credential copy or billing change occurred.

One independently seeded trial per arm used five fixed decisions: current port, numbered draft,
allowed future edit file, no unapproved integration and no replay. All memory-task files and
sentinels were unchanged. This is a bounded smoke, not an effectiveness benchmark:

| Arm | Observed decisions | Source-fetch evidence |
| --- | --- | --- |
| Native compaction and exact continuation | Five of five correct | Native continuation retained the decisions. |
| Fresh session with exact source pointers | Three unknown; both action refusals correct | Native policy rejected launching the source command before Concordia executed. |
| Compaction and pointers | Five of five correct | The attempted source command was also blocked; no benefit from pointers is established. |

App-server ordinary item events omitted the custom tool executions. Reading only each exact new
synthetic native session's originals established the `CreateProcess` policy rejection. A separate
supported, explicitly initiated native `command/exec` call ran the actual Concordia excerpt
reader in read-only mode and returned exit zero, the exact synthetic marker, source byte range
and SHA. The reader works in that scoped check; model-driven fetching under the tested policy
does not. Unknown values and a blocked fetch are not scored as successful retention.

Other dispatches qualified native image recognition, same-thread image continuation and an
exact owned offered allow-once decision for a harmless fixture echo. Structured-question setup
was rejected because the experiment's direct client lacked the experimental capability; the
application transport already advertises it, so this does not establish an application defect.
The denial took effect, but the direct driver sent a protocol-valid decision absent from the
offered list; offered-choice preservation remains unqualified. These direct native checks do
not qualify the application UI/worker broker, Claude PDF or physical hosts.

Hash-bound receipts, native usage snapshots and the request ledger remain under ignored
`target/live-qualification-20261104/`; `manifest.json` fingerprints the retained reports and
drivers. Monetary costs remain unknown. Compaction snapshots overlap turn totals, so they are
not summed into invented savings. No pointer advantage, token-saving claim or ranking-default
change follows from this run.

## Larger controlled comparison gates

### MCP policy and driver qualification, 2026-11-05b

Requests 16 and 17 used the supported local MCP reader instead of the earlier blocked subprocess.
Direct MCP and an auth-free native MCP rehearsal succeeded, but both model reads were denied with
`MCP tool call requires approval, but approval policy is never`. The 05a reader omitted effect hints;
version-matched native Auto policy classified it as requiring approval. B scored two of five
constraints; compacted C retained five of five from existing context. Neither fetched the facts.

05b declares truthful standard read-only/non-destructive/idempotent/closed-world hints for the
native reader without changing provider approval policy or marking proposal tools read-only.
Request 18 then successfully invoked the reader through the model under the same read-only/never
policy. Its returned record SHA and exact byte range independently match the native original.
This qualifies the changed model-to-MCP read path, not a manual-RPC substitute for that path.

The fact trial is **invalid**: the experiment driver requested message 1, which was environment
context. The decision seed was message 2. The model correctly left missing facts unknown; its
two-of-five result is retained. A no-model read verified all five facts in the actual seed, and is
not counted as model qualification. The corrected future runner requires a unique exact seed-text
match and all expected facts from the actual selected source before reserving any request. The
original as-run driver and failed/partial results remain in the ignored evidence folder.

At the end of that allowance all 18 authorized slots were consumed; no retry, account switch or
additional model call followed before the owner's subsequent authorization below.
Original fixture files and the selected chat account remain unchanged. No retention benefit or
token-saving claim follows. Reports are under ignored
`target/live-qualification-20261104/mcp-{memory-readonly,postfix-request18}/`.

An additional ordinary-application storage check in build 2026-11-04r consumed request 15 of 18,
using the same explicitly authorized alternative account and subscription-only funding. Native
completion produced body-free user/assistant thread references and one recall reference, with
exact original Unicode/whitespace verified in native history. This qualifies that bounded
persistence path, not comparative retrieval effectiveness or savings. At that checkpoint three
slots remained; requests 16 through 18 subsequently consumed them as recorded above.

### Corrected source-use smoke under doubled allowance, November 5

The owner doubled the total dispatch cap from 18 to 36 at 10:02 local time, retaining
subscription-only funding, no automatic retries and no automatic account switching. Publication
remains held until ready; new Windows confinement remains deferred.

Request 19 used the installed 05b executable (SHA256
`0f7e639ec39d4493d674864e4b9ceb36d271a8333dcedc15bce335e238f0eee9`) in a fresh native
session. A bounded no-model preflight first identified the unique exact original seed and
verified all five expected facts; the evaluation prompt supplied its pointer and actual ordinal,
not the answers. The model invoked Concordia's native MCP reader and answered all five constraints
correctly. Native original tool-call/output evidence confirms the accepted decision marker was
returned. The fixture files and selected main account were unchanged. Funding metadata reported
included subscription capacity and zero purchased credits; the native never/read-only/Auto
approval contract was unchanged.

This is one successful source-use smoke against an existing synthetic seed, not a new randomized
three-arm comparison, application broker/physical-host qualification, retention advantage or
token-saving result. The invalid request 18 remains preserved. The ledger now records 19 of 36
consumed requests, leaving 17. Hash-bound evidence and the no-retry driver are under ignored
`target/live-qualification-20261104/request19-corrected-final05b/` and
`request19-corrected.py`; the explicit new allowance is in `authorization-20261105.json`.

Use three arms on the same synthetic committed repository and pinned constraints: native resume
with provider compaction, a fresh native session with bounded verified handoff pointers, and
compaction plus those pointers. A trial must bind provider executable/version, model, account/home,
native thread/run/attempt, corpus revision, compaction controls and identical tool/approval policy.
Never carry an approval grant from one arm to another.

Seed exact decision constraints: preserve the numbered draft, modify only the approved file,
retain an explicit port/version decision, ask before integrating a worktree, and reconcile any
uncertain tool result before retry. Include contradictory older versions, an exact current source
pointer and a tool action interrupted after a durable side effect. Score constraint compliance,
task success, source correctness, duplicate side effects, native token/cost coverage and elapsed
time independently. Inspect the final Git diff and side-effect receipt; assistant claims alone
cannot establish success. Unknown token/cost fields remain unknown.

Prepare each arm in a separate clean worktree with fake-provider protocol checks first. Further
trials must stay within an explicit provider/task allowance, trial count and stopping cap;
the smoke above does not authorize an uncapped comparison. Record every attempted/failed trial,
keep native source evidence, and reconcile uncertain side effects before another attempt. No live
result or provider-specific recall advantage is claimed by the synthetic evaluation.
