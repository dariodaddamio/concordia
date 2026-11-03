# Workspace, input and integration completion plan

Planning baseline: `c6d3b7c`, inspected 2026-11-02. This document describes future slices; it
does not claim they are implemented. Scope is local Windows workspace operations, richer
attachments, host presentation, artifact discovery and native integration management. Provider
approval protocols belong to the [memory/provider plan](memory-provider-completion.md); managed
worker worktrees and their lifecycle belong to the [orchestration plan](orchestration-completion.md).

## Existing behavior to retain

Progress: [selected project replacement](../quality/project-replacement.md) implements W2 over
the shipped W1 search, including exact preview approval, guarded saves and receipt inspection.
Remaining file-opening, input, rendering and integration slices retain the boundaries below.

[File actions and independent editors](../quality/file-actions.md) implement W3/W4 through the
existing file panes, save and recovery engine. Native GUI launches are explicit; host/editor
physical evidence remains a separate W5 gate.

[Text documents and explicit pasted-path attachment](../images-and-diagrams.md) implement I1
and I3: ordered versioned snapshots, exact UTF-8 data, queue/recovery retention, and an explicit
F9 import offer that preserves pasted text. I4 binary transports remain unavailable.

[Integration discovery](../quality/integrations-catalog.md) implements the first I6 catalog
slice, shared between F10 and agent MCP discovery. It executes no integration commands;
explicit setup/removal remains with the existing Harness/native configuration owners.

| Surface | Source evidence | Status and boundary |
| --- | --- | --- |
| File discovery | `tui/related_files.rs`, `file_links.rs` | Chat/recent/workspace references, ignored-path-aware traversal, canonical deduplication and directory browsing exist. Discovery does not search file contents. |
| Internal workspace | `tui/workspace.rs`, `tui/workspace_edit.rs`, `file_document.rs`, `tui/syntax.rs` | Three panes; UTF-8 text up to 2 MiB/50,000 lines; formatted Markdown/JSON; raw edit/save/undo; exact-byte conflict protection; pane-owned recovery exist. |
| Separate editor | `file_open.rs`, `tui/files.rs`, `config.rs` | `/edit` plans an argument-array launch through WezTerm, using a configured terminal editor. Unified file actions, direct GUI editor launch and a dedicated Concordia editor process do not exist. |
| Image input | `attachments.rs`, `tui/images.rs`, provider adapters | Up to four PNG/JPEG/WebP snapshots of 4 MiB each, content hashes, queue/draft recovery, chips and native payloads exist. Chip activation opens the system viewer. |
| Diagrams | `tui/diagrams.rs`, formatted-document/chat rendering | Bounded native Mermaid exists. Inline raster previews, drag/drop imports, document attachments and generated-artifact cards remain open. |
| Integrations | `harness/{mod,command,connections,run,verify,pinned}.rs`, `mcp.rs` | Generic manifest tools, explicit connection/verification and pins exist. Concordia's MCP server exposes reads plus delegation proposals; it is not a general outbound MCP client or plugin runtime. |
| Goals and open work | `tui/landing.rs`, `mcp.rs` | User goal add/close, locked atomic goal storage and Harness open-work parsing exist. MCP goals and goal-change attribution remain missing; do not reimplement the landing list or Harness plan reader. |
| Configuration | `config.rs`, `store.rs` | User-written config is read-only; store writes are atomic and locked. Interactive preferences must not rewrite the user's config. |

All source paths below are relative to `crates/concordia/src/` unless otherwise stated.
The plan reuses these mechanisms instead of introducing a second editor, save path, attachment
store, manifest runner or provider authentication system.

## Shared contracts and proposed bounds

Each asynchronous request captures canonical workspace identity, conversation id, request
generation and the relevant draft/pane revision. A result is published only if those identities
still match. Closing a surface or switching conversation invalidates its generation and cancels
its work; late results never attach to whichever pane or draft happens to be focused.

One reusable workspace path policy should own ignored/sensitive paths and canonical containment.
Extract the policy currently embedded in `tui/related_files.rs` into a small non-TUI module before
reusing it for content search or artifact import. Automatic discovery/search excludes provider
homes, credentials, secret-pattern files, build outputs and links escaping the selected root.
Do not inspect credential contents to classify them. A regular file must be revalidated at use,
because a path can become a symlink, directory or different file after discovery.

Proposed initial search limits are 4,096 examined regular files, 2 MiB/50,000 lines per file,
64 MiB total bytes read, 1,000 matches, depth 14 and a five-second scan deadline. Stop at the first
limit and label the result incomplete, with the exact limiting reason. One search generation per
conversation and at most two scan jobs per app keep UI work bounded. Check cancellation between
files and at most every 64 KiB during reads. These are implementation defaults to measure against
synthetic repositories, not performance claims.

Replacement previews initially cover at most 32 selected files and 8 MiB aggregate before/after
text. Existing per-file limits remain unchanged. Image import keeps its present four-item/4 MiB
limits; new attachment kinds must fit an explicit aggregate budget instead of bypassing it.
All display text passes through the existing control-sequence sanitizer. Only a typed, app-owned
render operation may emit a terminal image sequence; provider text cannot become one.

## W1: read-only project content search — first shippable slice

Add a project-search mode to the existing file browser. Keep file-pane Ctrl+F as in-file search;
the browser provides a distinct, labelled project-search action. The exact keyboard shortcut is
chosen against the existing binding table during UI implementation; no chat Enter binding changes.
Begin with literal, case-sensitive/case-insensitive search and explicit root/ignore indicators.
Regex and replacement are separate modes, so a typed search is never interpreted as a write.

Implementation boundaries:

- Add `workspace_search.rs` for immutable `SearchRequest`, `SearchHit`, `SearchSummary` and
  cancellation-aware scanning. Return path identity, original-byte match range, line/column,
  bounded display context and the file revision/hash read. Preserve BOM, CRLF and Unicode
  offsets; display columns must not be mistaken for UTF-8 byte offsets.
- Add `tui/workspace_search.rs` for the browser mode, query/scope editor, results and generation
  management. Integrate through narrow calls in `tui/related_files.rs`, `tui/files.rs` and
  `tui/mod.rs`; do no filesystem reads from rendering.
- Reuse `ignore::WalkBuilder` already present in the dependency tree and the extracted path
  policy. No ripgrep installation, shell command, model call or additional service is required.
- Opening a result re-resolves its exact path and uses the existing workspace open/location
  operation. A changed result is marked stale; navigating to it never implies permission to edit.

The visible query owns its keys while active. Esc closes search and restores the original chat
draft and queue edit. An active provider may continue streaming; search never submits a turn.
Closing search releases retained file text and results. A full new scan is required after changing
root, query or ignore policy; old generations cannot partially populate the new query.

W1 is independent of new provider protocols, managed worktrees and attachment migration. It is
the recommended first workspace implementation because it can ship without a write operation.

## W2: previewed project replacement

Build on W1 after byte-range and stale-result behavior is proven. A `ReplacementPlan` contains a
stable id, canonical root, scope/query options, selected match ids and per-file original hash,
replacement hash and exact staged text. Its preview shows each selected change and skipped file.
Changing the replacement, selection or source revision creates a new plan; approval applies to
one displayed plan id only. This is a direct user action, not a provider tool invocation.

Use `file_document::save` as the single compare-before-replace implementation. Add a batch
coordinator in `workspace_search.rs`, and keep the existing conflict/backup/uncertain-write
semantics. Before any write, preflight every selected file: regular-file identity, root
containment, writeability, matching bytes, pane state and any active worker ownership lease.
Dirty or saving panes block that file until the user saves/discards/re-previews; never replace a
dirty in-memory buffer or silently reload it. Clean affected panes invalidate their old snapshot
and reload asynchronously after a confirmed save.

Apply selected files serially. Cancellation stops before the next file, then reports what changed.
A failure leaves completed files completed and unstarted files untouched. There is no claim of
cross-file atomicity, automatic rollback or blind retry. A bounded operation receipt records plan
id and per-file completion/hash/status without copying project text into global history. On
restart, reconcile current hashes and report completed/changed/uncertain files; do not resume a
batch automatically. An uncertain Windows replacement retains its existing staged/backup files.

Dependency: the orchestration workspace registry must expose a read-only ownership query before
W2 permits replacement in a managed worker workspace. Until that API exists, W1 can ship and W2
must refuse managed/actively owned roots rather than guess that no owner exists.

## W3: unified file-opening choices

Introduce one `FileAction` resolver in `file_open.rs` with the accepted labels **Preview here**,
**Split off** and **Open in editor**. Route chat links, browser rows, search results and artifact
cards through it. Text preview uses the existing internal pane; images retain explicit image
view behavior until inline preview exists. Split off starts Concordia's bounded preview/editor
mode in a separate terminal through a supported host adapter. Open in editor supports either a
configured GUI executable directly or the existing terminal-editor route. The resolver explains
an unavailable host/editor action and keeps the original draft.

Add an optional opening preference to `store.rs`, with absence retaining today's Preview here
behavior. User-written defaults in `config.rs` remain read-only and follow the existing
config/store precedence. Do not reinterpret existing `[editor].program` as a GUI executable;
add an explicit launch kind and preserve the current terminal default. Argument templates expand
inside individual arguments, with path/line/column passed as data. Executable resolution, Windows
shim validation and creation flags must use the existing launch protections. No shell association
is used to launch an editor executable.

Launching an external viewer/editor does not save a dirty internal pane. Show its disk-version
status and retain the dirty buffer; normal save conflict checks protect later edits. Track/reap
bounded launcher children and stderr. A successfully detached user's GUI editor is not killed when
the modal closes or Concordia exits. Failed launches remain visible; there is no silent editor
installation or provider-home/config change.

## W4: dedicated Concordia file-editor process

Extend the standalone `preview` entry point in `main.rs` and `tui/preview.rs` with an explicit
editing mode that reuses `file_document.rs`, `tui/workspace.rs` and `tui/workspace_edit.rs`.
The split-off process receives a canonical file/location and its own pane recovery identity; it
does not launch a provider or inherit a conversation's execution ownership. Refactor shared file
surface code before exposing this mode, rather than maintaining two save/recovery engines.

An existing dirty pane triggers an explicit disk-versus-buffer explanation before an independent
editor is opened. Separate editors may coexist, but every save still validates its own baseline.
Starting/closing one editor cannot close another or claim its recovery lease. Parent launcher
success must not be presented as proof of child save completion.

## W5: host capability and physical-host evidence

Keep separate capabilities for images, file-drop text, pointer shapes and terminal spawning in
`host/mod.rs` and `host/wezterm.rs`. A host name or environment variable is a hint, not proof that
every protocol works. Unknown capability uses the existing text chip/viewer and cursor fallback.
Do not upgrade or rewrite terminal configuration automatically.

The installed WezTerm 2024 pointer-shape limitation remains open. Its app-side hit-test coverage
cannot prove a visible shape change. Record a physical-host matrix for that build and any newer
user-selected host/version, including ConPTY/multiplexer paths, resize, focus regain and cleanup.
A newer host may establish support only for the exact path tested.

## I1: attachment envelope and explicit text documents

Extend `attachments.rs` to a versioned attachment descriptor with image/text-document kinds,
immutable snapshot id/hash, media type, byte count, display name and optional selected text range.
Continue reading the existing v1 image envelope. Old drafts/queues/recovery records remain
readable; new fields default conservatively. Unknown versions/kinds fail visibly before dispatch
and preserve the draft, rather than sending an internal envelope as prompt text. Do not migrate
or delete old attachment files merely because they are not in the current draft.

Add bounded explicit UTF-8 text/Markdown/JSON import first. The user selects **Attach document**
and previews the exact included text/range, with encoding/size errors and any truncation shown.
Reuse existing UTF-8 and control-display handling. Inclusion is labelled untrusted document data;
it is never promoted into system instructions or read as an integration manifest. Initial text
inclusion is capped at 64 KiB aggregate per submitted message, within current dispatch/context
budgets. Preserve original bytes and hash separately from any newline-normalized transport view.

`tui/images.rs` can become a thin attachment-composer surface after storage/model changes are
stable. `tui/persist.rs`, queue edits, request recovery and continuation must preserve the same
descriptor snapshot. Missing/changed snapshots stop submission visibly. Sensitive-path filtering
applies before all imports, including explicit selections, under the never-read-secrets contract.
No credential-file fixtures or real credential reads are needed.

## I2: inline raster previews

Add `tui/raster.rs` as a bounded decode/downsample/cache service and a typed terminal presentation
adapter. Keep original attachments unchanged; cached thumbnails are derived from verified
snapshots, keyed by hash, viewport, scale and host capability generation. Start with composer and
selected-file previews; then support visible history rows. Never re-decode an entire history
after an unrelated message arrives.

Proposed bounds: two decoder jobs, 16 megapixels decoded per image, 32 cache entries and 32 MiB
aggregate decoded-cache memory. Decode unsupported/malformed/oversized images to a visible text
fallback. A compressed 4 MiB image is not proof that its decoded dimensions are safe. Derivatives
must fit a separate output budget; cancellation/eviction releases them when panes close.

Terminal image output is a distinct host operation after cell rendering, with explicit clipping,
cursor restoration and invalidation on scroll, resize, modal display, focus redraw and close.
No raw image escapes enter transcript text or ordinary Ratatui cells. A protocol needs both
capability evidence and repaint/cleanup proof before activation. Text chips and the system viewer
remain functional on unsupported or unverified hosts.

## I3: file drag/drop without changing paste semantics

WezTerm documents file drops as pasted path text, with configurable platform quoting. Therefore a
path-shaped paste cannot safely be assumed to be an attachment or a distinguished drag event.
Keep ordinary paste byte-for-byte. A bounded path-only paste may offer **Attach files** as an
explicit action; it does not import until selected. An optional attach/drop mode can interpret
only documented host framing and is confined to the focused composer.

Parse Windows quoting as a bounded path-list grammar, not a shell command. Reject remote URLs,
directories, unreadable files and lists above the attachment count before importing. Preserve
order, deduplicate by content hash, report per-item errors and retain the original text if the
user cancels. Raw file editors and queue editors keep their current paste ownership. No action
dispatches a provider turn, runs a dropped file or changes host drop settings.

## I4: native binary document transports

After I1, add PDF or other binary-document kinds only through verified native provider interfaces.
The provider capability registry in the memory/provider plan is the dependency: it must identify
provider, CLI version, account/session/model constraints, supported media types and limits. The
current image transports do not prove document transport support.

Unsupported or unknown types remain selectable for a local metadata/view operation but cannot
be sent through an invented payload, hidden OCR, remote conversion or base64-in-prompt fallback.
Provider changes during continuation revalidate every attachment kind; incompatible content
pauses with the draft/checkpoint intact. Any later local conversion requires a separately reviewed
format/parser/dependency budget and a visible derived-text preview with original provenance.

## I5: generated-artifact cards

Add typed artifact references to the provider/card normalization boundary
(`provider/stream_json.rs`, `provider/stream_cards.rs`, card ingestion in `tui/mod.rs`, `tui/files.rs`). First
accept exact local-file outputs from known structured tool events; arbitrary prose stays a link
suggestion. Bind a card to provider/account/session/run/workspace, producing event id, canonical
path and observed revision. Deduplicate repeated events without conflating revisions.

Cards show name/type, generation status and supported actions from W3. Opening reads the current
file and explains a changed/missing artifact; attaching imports a fresh verified snapshot only
after an explicit action. Generated HTML, SVG, scripts and executable content are never evaluated
inside Concordia. Text uses the existing preview, raster images use I2 or the system viewer, and
unsupported formats retain metadata/open choices. No automatic launch/download/attachment occurs.
Initial bound: 32 artifact references per turn and 256 retained per conversation, with explicit
overflow and lazy file reads. Recovery retains references and observed revisions, not an
unbounded second artifact store. Provider-native/source output remains authoritative.

## I6: broader tool/plugin discovery and management

First ship a read-only integrations view over existing Harness connections, manifest tools and
Concordia MCP registration state. Show source, manifest revision, provider/home/project binding,
available actions and verification status. This is discoverability for existing integrations,
not a second runtime. Names are namespaced by integration id; tool descriptions/annotations are
untrusted display data and do not establish permission or safety.

Subsequent explicit connect/disconnect actions reuse `harness/command.rs`, manifest validation,
minimal environment, argument arrays, output/time bounds and native provider configuration APIs.
Keep each provider's supported plugin/MCP registration format behind its adapter. Capability
inspection is read-only; installing/configuring an integration is a separate visible action.
Only Concordia-owned homes may receive Concordia-managed registration. Authentication stays in
the native provider/integration flow; Concordia never stores tokens or migrates credentials.

The existing inbound MCP server advertises revisions through 2025-06-18. Verify a provider's
actual negotiated protocol before adding newer tools/resources/elicitation behavior. Broader
outbound MCP execution, marketplace fetching or arbitrary plugin code is not implied by this
catalog slice; it needs an explicit adapter contract and the native approval/cancellation work.
Use lazy discovery and bounded schemas (proposed 64 tools/256 KiB per integration, 16 integrations
per workspace, 60-second inspection/launch deadlines) so enabling discovery does not add every
tool schema to every model turn. External calls retain provider-native approval and result-source
labels. An interrupted uncertain call is inspected, never replayed to obtain a cleaner UI.

### MCP goals and tool-extension disposition

The context-handoff plan calls for agents to read/add/close goals with visible change origin.
This remains missing from `mcp.rs`. Memory/provider M0 owns the shared goal service, bounded MCP
goal reads and goal/thread/file associations. It must reuse the current `tui/landing.rs` store's
stable ids, 2 MiB ceiling, atomic lock, immutable goal text and monotonic completion behavior;
the integration catalog must not invent a second goal database or relationship graph.

A later goal-change tool writes typed add/close proposals through the existing delegation
proposal/approval boundary, with exact goal id, observed revision and request id. User review
applies once to the displayed proposal/revision. Duplicate close is idempotent; stale/corrupt
state or failed receipt writes preserve existing data and never trigger automatic restart replay.
Optional attribution fields default for old records and distinguish observed source, verified
thread/account binding and approving actor. Client-supplied names or MCP `clientInfo` cannot
authenticate an actor; absent trusted launch binding remains unknown. This mutation slice shares
the normal approval work in the memory/provider plan and does not broaden I6 catalog execution.

Other future tool extensions, including pain-point-log access, need individually scoped read or
proposal schemas, bounded storage and the same provenance boundary. Existing `[threads].mcp`
opt-in stays in force; goals tools do not turn logging on or register into every provider home.
Generic integration management reuses provider-native MCP configuration and permissions. There
is no plugin credential broker, arbitrary filesystem-write tool or second native tool executor.

## Acceptance matrix and evidence

The following is a required contract matrix, not a claim that new automated tests were added or
run for this plan. Reuse existing fake-provider, TestBackend, synthetic-file and terminal harnesses;
new unit/integration tests require the project's normal explicit authorization. Extend existing
scenarios or run bounded manual fixture journeys when implementation changes the corresponding
contract.

| Slice | Scenarios | Observable acceptance |
| --- | --- | --- |
| W1 | Ignored/sensitive files, symlink outside root, binary/invalid UTF-8, huge line, Unicode/CRLF/BOM, every scan limit | Only allowed files read; exact ranges open correctly; visible complete/incomplete/error status; no silent skipped scope. |
| W1 | Query/root change, modal close, conversation switch, active streaming, cancellation | Old generations discarded; input/draft/queue preserved; no provider dispatch; bounded work/memory after close. |
| W2 | Changed/deleted/read-only target, new symlink, dirty/saving pane, worker lease | Preflight blocks unsafe targets; displayed preview never authorizes unseen bytes; unsaved text retained. |
| W2 | Mid-batch failure, cancellation, crash before/after acknowledged save | Exact completed/pending/conflict/uncertain report; no automatic rollback or replay; retained Windows backups remain inspectable. |
| W3/W4 | Spaces, Unicode, shell metacharacters, placeholder-like path text, absent host/editor, child failure | One safe argument array; correct file/location; actionable error; no shell evaluation or silent config change. |
| W3/W4 | Dirty internal pane, two editor processes, parent exit/restart | Independent baseline checks and recovery identities; no forced save, overwritten buffer or unrelated child termination. |
| W5/I2 | WezTerm 2024 and newer selected host, direct/ConPTY/mux, resize/scroll/modal/focus/close | Exact-host capabilities/evidence recorded; no image ghosts or pointer support claim inferred from unit state. |
| I1/I4 | v1 image draft/queue/recovery, v2 mixed content, unknown version/type, corrupt/missing snapshot, provider handoff | Old records still work; exact snapshots retained; unsupported media pauses explicitly before model execution. |
| I2 | Invalid/decompression-bomb image, repeated same image, history growth, cache eviction | Decoder/cache limits enforced; UI remains responsive; text fallback usable; memory released. |
| I3 | Ordinary quoted/path-like paste, multi-file drop offer, declined import, raw/queue edit focus | Original paste unchanged unless explicit attach action; no execution, accidental submission or cross-surface import. |
| I5 | Replayed tool event, changed/missing artifact, outside-root output, malicious HTML/control text | Provenance and revision visible; exact deduplication; no automatic fetch/evaluation; safe fallback/actions. |
| I6 | Malformed/changed manifest, name collisions, unsupported protocol, timed-out child, disconnect during run | Correct binding and stale-state refusal; caps/cancellation; native approvals remain authoritative; no uncertain retry. |
| M0/I6 extension boundary | Legacy goals, concurrent manual close, stale proposal, duplicate approval, forged client actor | One shared locked store and relationship service; preserved monotonic completion; exact proposal applies once; honest attribution and no silent mutation/replay. |

Existing anchors: `tui/workspace_tests.rs` covers lossless save/conflicts, recovery, input ownership,
stale loads, pane limits and display controls; `attachments.rs` and `tui/images.rs` cover snapshot
immutability and durable envelopes; `tests/images.rs` covers adapter payloads; `file_open.rs`
covers safe argv/templates; `tests/harness_command.rs`, `harness` modules and `tests/mcp.rs` cover
manifest/RPC bounds and proposal-only writes. `tests/tui_journeys.rs` supplies the installed
fake-provider file, image, queue, modal and recovery journeys. Its terminal emulator is not proof
that physical pixels or pointer shapes render correctly.

At implementation delivery, run formatting, all-target clippy with warnings denied, the affected
existing fixture journeys, `cargo test -p concordia --tests --locked`, required everyday/release
smoke and OSS/dependency gates on the exact candidate. Re-run installed/physical-host checks for
the host paths changed. Bump BUILD_STAMP and changelog for user-facing slices; record source,
fixture, installed-host and live-provider evidence separately. No model turn is authorized by
this plan.

## Dependencies, implementation waves and decisions

| Wave | Ready scope | Prerequisites and completion boundary |
| --- | --- | --- |
| 1 | W1 only | First workspace owner in the campaign's three-agent wave. Fix the shared path-policy API before integration; one owner handles TUI entry points. |
| 2 | W3; I1; W5 capability inventory | Preserve existing config/envelopes. Agree attachment descriptor and host capability types before parallel edits. |
| 3 | W2; W4; I5 metadata cards | W2 needs W1 and managed ownership query. W4 shares W3 launch semantics. I5 uses W3 and exact provider event identity. |
| 4 | I2; I3; I4; I6 explicit management | I2 needs W5 physical-host path; I3 needs I1 and explicit paste semantics; I4/I6 execution need verified native capability/approval/cancellation interfaces. |

I6 read-only catalog is separately schedulable after the first campaign wave; it is not an
additional simultaneous worker. MCP goal reads follow memory M0; goal mutations follow its
proposal/approval slice. The coordinator enforces at most three concurrent implementation owners
across the whole campaign, not three owners per plan.

The coordinator owns `tui/mod.rs`, command bindings, `config.rs`, `store.rs`, BUILD_STAMP and the
changelog. Workspace and input agents own their new isolated modules, then submit narrow central
integration patches. This prevents concurrent changes to input precedence or multiple incompatible
attachment formats. Do not start all waves in parallel merely because agents are available.

Routine implementation choices above have conservative defaults and do not require owner
research. Material decisions still to record before affected slices ship are the host image
protocol/dependency choice and supported host matrix; whether a default GUI editor is configured
or selection is always explicit; binary-document formats supported by real installed providers;
and whether any outbound integration execution beyond native adapters is in product scope.
These do not block W1, the read-only integration catalog or fixture-only contract work.

## Primary interface references

Inspected 2026-11-02; documentation describes possible interfaces, not installed-host proof.

- [WezTerm iTerm image protocol](https://wezterm.org/imgcat.html) documents inline images and an
  explicit multiplexer limitation. I2 therefore requires proof for the exact terminal/ConPTY/mux
  route, not merely a recognized terminal name.
- [WezTerm dropped-file quoting](https://wezterm.org/config/lua/config/quote_dropped_files.html)
  documents Windows file-drop support and configurable path quoting. I3 treats that data as
  input text; automatic distinction from ordinary paste is not established by this interface.
- [MCP 2025-06-18 tools](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)
  describes names, schemas, discovery, result resources and untrusted annotations. The integration
  view preserves visible tool provenance and permission controls; it does not infer authorization
  from tool-provided metadata. Newer protocol behavior must be negotiated and implemented as a
  separate compatibility slice.
