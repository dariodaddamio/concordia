# Grok Bot routines

Grok Bot is a cloud assistant, not a local CLI. Concordia can trigger a registered Grok Bot
routine and wait for one callback containing either a result or a question.

## Setup

Open `/use`, choose `+ configure Grok Bot routine`, and complete the picker form. Enter the account
ID, routine ID, display name, HTTPS webhook URL and bearer key. The key field is masked and is
never added to the transcript, command history, logs, crash reports or state files. Concordia
stores only the non-secret routine metadata in its state directory. The webhook key is written to Windows Credential Manager under
an identity containing `grok-bot`, the account ID and the routine ID; it is never selected by
display name and is never placed in a prompt, tool result or environment variable.

Selecting a configured routine opens actions to edit it, remove it, or rotate the callback token.
Editing replaces the stored webhook key only after the form is submitted. When the first routine
is registered, Concordia starts a localhost-only callback listener and
shows its address, a generated callback bearer token and setup instructions. Expose that address
through a tunnel you run yourself (ngrok, Cloudflare Tunnel or Tailscale Funnel). Concordia does
not ship or launch tunnel software. Rotate the callback token from the routine setup screen if it
is disclosed.

## Run and return path

The `agents` MCP tool accepts `provider: "grok-bot"` and a routine ID. Concordia POSTs the task and
a Concordia-generated run ID to the routine webhook. A successful HTTP 200 is only a start
acknowledgement. The Agents tree then shows `waiting for Grok Bot`.

Grok Bot connects to the callback endpoint as an MCP client. The callback server exposes exactly
one tool, `grok_bot_callback`, with `run_id`, `kind` (`result` or `question`) and `content`.
Concordia accepts only run IDs it created, limits request size and callback rate, and fences the
returned text as untrusted external content before storing it. It does not poll Grok Bot. A
question becomes Needs attention; a result finishes the row. If the webhook or callback tunnel is
unreachable, the row says so and the result remains in Grok Bot's own chat.

## Safety and limits

- The listener binds to `127.0.0.1` and is started only when at least one routine is configured.
- The callback token and webhook keys are Credential Manager secrets. Credential Manager protects
  against other users and disk theft, not a process already running as the same Windows user.
- No chat history, Brain, provider credentials or general-purpose MCP tools are exposed.
- Concordia runs no tunnel and makes no unattended approval decisions for Grok Bot.
- The focused fake-endpoint and callback tests cover the local HTTP contract, identity checks,
  body/rate limits, listener binding and state-directory redaction; they never contact xAI.
