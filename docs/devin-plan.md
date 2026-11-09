# Devin threads

The Devin CLI (Cognition's `devin`) as a fourth provider beside Claude
Code, Codex and Grok Build, behind the `devin` experiment until it has
been tried.

## Why

Behrooz has a Devin login and ran Devin through T3 Code; he wants Devin
threads in the same sidebar, on the same rules: his own login, the
official binary, permission modes, the threadbear tools, usage per turn.

## The wire (verified on devin 3000.12.32, 9 Nov 2026)

`devin acp` speaks ACP (agentclientprotocol.com) as newline-delimited
JSON-RPC 2.0 on stdin and stdout, as Grok's agent does, so the Grok
adapter drives it with Devin's dialect (`runner/src/adapters/devin/`).
Spikes against `swe-2-flash-medium` in a temp folder showed:

- `initialize` answers `loadSession: true`, `promptCapabilities.image`,
  `mcpCapabilities.http` and `agentInfo.version` `0.0.0-dev` (useless: the
  version comes from the install's folder name instead).
- `session/new` `{cwd, mcpServers}` returns the id Devin picked ("sweet-
  otter" style), `modes` and `configOptions`: `mode` (accept-edits, ask,
  plan, bypass, and smart once a turn has run), `model` (SWE-2 and the
  other models the account offers; ids like `claude-opus-5-5-medium`
  overlap Claude's), `thought_level` (medium, high, max) and `speed`.
  Model, effort and mode all go through `session/set_config_option`
  `{sessionId, configId, value}`. Devin ignores `_meta.rules`, so
  Threadbear's instructions lead the first prompt in a
  `<threadbear_instructions>` block. Devin also reads AGENTS.md and
  CLAUDE.md itself, and loads the MCP servers in its own config.
- `session/prompt` answers when the turn ends: `stopReason` and a
  top-level `usage` (`inputTokens`, `outputTokens`, `cachedReadTokens`).
  `usage_update` (`used`, `size`) is the context meter.
- Tools come as `tool_call` with `_meta["cognition.ai/inferenceToolName"]`
  (`exec`, `read`, `write`, `edit`, `mcp_list_tools`, MCP tools as
  `mcp__<server>__<tool>`) and a `rawInput` already in Claude's names
  (`command`, `file_path`, `old_string`, `content`). A command's output
  comes in its `in_progress` updates, each with all of it so far; its exit
  code in `_meta.terminal_exit.exit_code`; the `completed` update has no
  content. Edits carry `diff` content (`path`, `oldText`, `newText`).
- `session/request_permission` offers `allow_once`, several
  `allow_always` kinds and `reject_once`; its `toolCall` has only the id.
- `session/resume` isn't there (-32601). `session/load` replays the whole
  conversation as `session/update`s before it answers; the adapter drops
  them. A session that doesn't exist answers -32016 with
  `data["cognition.ai/errorKind"]: "session_not_found"`.
- `session/cancel` ends the prompt with `stopReason: cancelled`.
- Logins: `~/.local/share/devin/credentials.toml`. Threadbear only checks
  it's there.

## Modes

Over ACP Devin offers Code (accept-edits), Smart, Ask (no code changes),
Plan and Bypass, not its terminal's Normal mode. Full access is Bypass,
Auto is Smart, Plan first is Plan, and Ask before edits is Code: commands
come to Threadbear's approval card, workspace edits run. Devin's own
permission rules (`~/.config/devin/config.json`) are applied first.

## Updates

`devin update` needs a terminal (headless it exits 130), so
`runner/src/updates/devin.ts` does what `cli.devin.ai/install.sh` does:
reads `static.devin.ai/cli/current/manifest.json`, downloads the bundle
for this platform (only from static.devin.ai), checks its sha256, unpacks
it into `<data>/devin/cli/_versions/<version>`, and swaps the `current`
link in one rename. It keeps the previous version for Go back and removes
older ones no process runs. Devin threads start the version folder's
binary (the link's real path), so an update never moves files under them.
An install from Homebrew or anywhere else is listed but left alone.

## Left out at first

More than one Devin login (devin-main only), Devin Cloud (`devin acp
--cloud`), sandboxed threads (refused, as Grok's are), token tally from
transcripts, subagent cards.
