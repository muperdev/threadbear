// Loaded before every unit test file (`--import`, in package.json's test
// script and deploy-steps.mjs's rerun). Setup and deploys are often started
// from inside a Claude Code session, whose CLAUDECODE, CLAUDE_* and
// ANTHROPIC_* variables the tests would otherwise inherit: the desktop app's
// CLAUDE_CODE_TERMINAL_MCP_TOOLS failed the adapter's end_turn test and
// stopped setup (David, 8 Nov 2026). The installed runner, a LaunchAgent,
// never has them, so the tests run as it does: without them. They go back
// as they were when the test process exits.
const SESSION_VAR = /^(CLAUDECODE$|CLAUDE_|ANTHROPIC_)/;

const saved: Record<string, string> = {};
for (const [name, value] of Object.entries(process.env)) {
  if (value === undefined || !SESSION_VAR.test(name)) continue;
  saved[name] = value;
  delete process.env[name];
}

process.on('exit', () => {
  for (const [name, value] of Object.entries(saved)) process.env[name] = value;
});
