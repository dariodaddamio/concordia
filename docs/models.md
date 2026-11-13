# Model discovery

Model choices come only from the selected account's physical provider home and an explicit CLI list
captured for that same provider/account/home identity. Concordia neither merges other accounts' model
caches nor supplies a built-in model catalog. No data produces an empty discovered list; the provider
default and an explicitly typed ID remain available. A typed ID is a user request, not evidence that
the provider offers it. Native caches and CLI output retain their reported IDs, including native
aliases or older-looking names; Concordia does not rename or remove an ID based on its age.

`available` performs bounded local reads. Codex model metadata comes from `models_cache.json` using
its current object `models` array with required `slug`, `display_name` and `visibility` fields.
Only native `visibility: list` entries enter discovery. Hidden/unknown visibility stays unlisted;
missing visibility, root arrays, ID aliases and bare-string cache layouts are unsupported.
Claude choices come from the current native SDK initialize response, not guessed cache filenames.
Cursor's local config contributes
only an explicitly stored model ID. It no longer invents `auto` when metadata is absent or invalid.
An explicitly reported or configured `auto` remains valid native data.

Cursor's explicit refresh uses its supported `agent models` command, bounded to 30 seconds and 1 MiB
of combined output with owned process-tree cleanup. Extra account homes use their existing native
API-key handoff. Without a key, only the real default Cursor home can be refreshed; Concordia does
not assume an undocumented profile-selection environment variable or query another account instead.
No model inference is performed to discover choices.

Claude 2.1.319 exposes native model `value` and `displayName` entries through the official SDK
initialization interface (`initializationResult` / `supportedModels`). Its refresh sends only an
initialize control frame with an exact request ID and version check; it sends no user message.
The process receives the selected CLAUDE_CONFIG_DIR, uses bare mode with empty settings sources,
strict MCP selection and no session persistence, and is bounded to 30 seconds / 1 MiB by the shared
process owner. Ambient API-key/token overrides are removed. These are CLI-declared choices, not a
server entitlement or quota check. Concordia neither extracts account details nor turns resolvedModel
metadata into additional selectable IDs. The obsolete Claude cache-layout readers were removed.

Refresh returns an explicit success/error result. CLI failure, missing/invalid metadata, version
mismatch, and cache publication failure are visible; they never become a successful refresh of an
old list. Existing cached rows may remain as previous observations. Codex reads its current native
cache and labels that operation as a native-cache read. Routing, dependencies, continuation and
worker controls share the same account/home-bound model read path.

Concordia's list cache is bounded to 4 MiB and published atomically. Its versioned metadata binds the
exact provider, account and canonical home; filenames also distinguish physical homes. Unbound old
cache records and records for a rebound home are not accepted or automatically deleted. Refresh
eligibility uses the validated identity and recorded age; future timestamps are rejected. Cached
native metadata is an observation, not a guarantee of current server availability.

The model picker lists only the model IDs and display names returned for the selected account. It
does not offer typed IDs or source labels. Each row shows the effort names returned with that model.
Model-tier classification remains a separate local routing/display heuristic; it neither creates
selectable IDs nor establishes price, availability or native context capacity.

Provider sources are explicit. Codex reads its account-bound `models_cache.json`; Claude reads the
official initialize response; Cursor runs `agent models`; Grok Build reads its account-owned
`config.toml` default, and opencode reads its account-owned `opencode.json` or `opencode.jsonc`
model. Neither integration currently exposes a native complete model catalog, so Concordia shows
no additional invented choices. The selected provider remains the authority on whether a request
succeeds.
