# Provider activity parser coverage

This audit describes the event shapes the current parser code accepts. The examples in unit tests
are synthetic regression fixtures; they do not establish that a particular installed CLI version
emits every shape. No provider process was launched for this audit.

| Provider | Received and displayed | Remaining parser gaps |
| --- | --- | --- |
| Codex | JSONL `item.*` events for recognized tool-like item types; call IDs, command, path, structured `input`/`arguments`, content, diff/change arrays, result/output, MCP server/tool labels, errors and exit status. `response_item` function/custom calls and outputs retain structured arguments/results. Repeated IDs update one card, identical output replays are idempotent, and output events preserve the original tool label and input. | The parser only extracts text from recognized message/item shapes; it does not decode every Responses API delta/event. Function-call outputs that arrive without their call event have no recoverable tool name. File-change details are only what the provider included in the event; no workspace lookup or diff reconstruction occurs. |
| Claude Code | `assistant` content blocks for text, thinking and tool use; `user` tool results; final `result`; and nested `stream_event` content-block start/input-JSON-delta shapes. Tool-use IDs are reused for result updates. Structured arguments and structured/multiblock result content are retained. Input deltas are capped at 256 KiB per block and show a truncation marker. | `stream_event` handling is limited to tool-use starts and `input_json_delta`; other delta kinds, message lifecycle events and control/approval events are not represented as cards. The parser relies on the `assistant`/`user` block form or the nested event form it recognizes. |
| Cursor | The parser's `tool_call` `started`/`completed` shape with call ID and a single tool-call variant; structured args and results are retained. Result/error status and provider-supplied file paths, old/new text or patches remain in the detail. | Other event envelopes, tool-call variants that do not use this union shape, approval/control states and streaming partial arguments are ignored. The generic JSON line parser does not infer file changes from text. |

For all providers, malformed JSON is either ignored (Codex) or shown as plain text (Claude and
Cursor); unknown valid event objects produce no fabricated card. A single card has one text detail
field and a three-state status (`running`, `done`, `error`), so it cannot preserve typed payload
structure or distinguish approval, cancellation, timeout and other native lifecycle states. The
expanded transcript can show full received details, while its compact line is intentionally
truncated for layout. Tool names, output, errors and edits are not independently verified against
provider state.
