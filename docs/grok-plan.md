# Grok threads

Grok Build (xAI's `grok` CLI) as a third provider beside Claude Code and
Codex, behind the `grok` experiment until it has been tried.

## Why

Behrooz has a grok.com login and wants Grok threads in the same sidebar, on
the same rules: his own login, the official binary, permission modes, the
threadbear tools, usage per turn.

## The wire (verified on grok 1.0.77, 8 Nov 2026)

`grok agent --no-leader stdio` speaks ACP (agentclientprotocol.com) as
newline-delimited JSON-RPC 2.0 on stdin and stdout. Two spikes against
`grok-4.7` in a temp folder showed:

- `initialize` `{protocolVersion: 1, clientCapabilities: {fs: false, terminal:
  false}}` answers `agentVersion` in `_meta`, `loadSession: true`,
  `sessionCapabilities.resume`, `promptCapabilities.image: true` and
  `mcpCapabilities.http: true`. With fs and terminal off, Grok uses its own
  tools in its own process, as Codex does.
- `session/new` `{cwd, mcpServers, _meta: {rules, yoloMode, autoMode}}`
  returns the id Grok picked (Grok assigns ids, like Codex), the models with
  their effort levels and context windows, and `configOptions`. The agent's
  `-m` flag doesn't reach the session: the model and effort go through
  `session/set_config_option` `{sessionId, configId: 'model' |
  'reasoning_effort', value: '<id>'}`, the value a plain string (Grok's own
  docs show `{value: '<id>'}`, which it refuses with -32602).
- `session/prompt` `{sessionId, prompt: [{type: 'text'}, {type: 'image'}]}`
  answers when the turn ends: `stopReason` (`end_turn`, `cancelled`, ...)
  and `_meta.usage` (tokens, cached reads, reasoning, `costUsdTicks`).
  A second prompt while one runs is queued by Grok (`_x.ai/queue/changed`).
- `session/cancel` (a notification) ends the running prompt with
  `stopReason: cancelled`, its running command stopped.
- `session/resume` `{sessionId, cwd, mcpServers}` in a new process picks the
  conversation up. A session that doesn't exist answers -32603
  `data.code: FS_NOT_FOUND`: SessionNotFoundError.
- `session/update` carries `agent_message_chunk`, `agent_thought_chunk`,
  `tool_call` (title, rawInput, `_meta["x.ai/tool"].kind`),
  `tool_call_update` (kind, title, `content` text or `diff` with
  oldText/newText, `rawOutput`, status), `plan`, `session_info_update`
  (title) and `available_commands_update` (slash commands).
  `_x.ai/session_notification` adds `model_changed`, `turn_completed`
  (usage), `background_tasks`, `response_completed`.
- `session/request_permission` (a request) offers `allow-once`,
  `always-allow` / `allow-edits-session`, `reject-once`, `reject-always`;
  the answer is `{outcome: {outcome: 'selected', optionId}}` or
  `{outcome: {outcome: 'cancelled'}}`.
- Logins: `$GROK_HOME` (default `~/.grok`) holds `auth.json`; `grok login`
  with another `GROK_HOME` makes a second login, as `CODEX_HOME` does.
  Threadbear never reads `auth.json`, only checks it's there.

## Shape

`runner/src/adapters/grok/`:

- `process.ts`: one `grok agent --no-leader stdio` per live thread, the
  JSON-RPC (requests, notifications, its requests to us), stderr tail,
  exit. Same rules as Codex's `appserver.ts`: process group, closing stdin
  ends it, retried spawn.
- `normalize.ts`: ACP updates to protocol events. Pure, tested with
  hand-made fixtures shaped like the spikes' traces (never the traces).
- `adapter.ts`: `GrokAdapter` and `GrokSession`: start or resume, one
  prompt at a time (messages sent during a turn wait for it, then go as
  one turn), interrupt, model, effort and permission mode, models list.
- `approvals.ts`: `session/request_permission` to the approval card, and
  permission modes: Full answers allow-once itself, Ask before edits asks
  for edits and commands, Auto lets Grok's `autoMode` decide, Plan first
  denies edits.

The threadbear tools go in as an `http` MCP server on `session/new`: the
runner serves them on 127.0.0.1 at a path with a per-thread secret
(`mcp-http.ts`), using the same handlers and approval rules Codex's
dynamic tools use.

Elsewhere: `Provider` gains `grok` (protocol first), config
(`grokPath`, `grokMainDir`, `grokArgs`), account discovery (`grok-main`
from `~/.grok` when its `auth.json` exists, `grok-<n>` folders under the
accounts dir), the pool title, end_turn's wording (Codex's: the final
answer ends the turn), slash commands, `net-probe`, and the app's labels
and logo.

## Left out at first

Rewind (Grok has `x.ai/rewind/*`; the runner reverses diffs instead),
sandboxed threads (refused, as Codex does), CLI updates (Grok updates
itself), rate-limit windows (`grok usage` can't report them headless yet),
token tally from transcripts, subagent cards.
