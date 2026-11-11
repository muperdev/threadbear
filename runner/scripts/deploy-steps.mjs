// The build steps deploy.mjs runs before it makes a release (typecheck, unit
// tests, npm ci), apart from the script so test/unit/deploy-steps.test.ts can
// drive them with fake processes. deploy.mjs runs on import, so it can't be
// imported by a test itself.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

// A failure with a message for whoever ran the script; no stack trace.
export class Fail extends Error {}

// A step that didn't exit 0, with where its output went.
export class StepFailed extends Fail {
  constructor(message, logFile, status) {
    super(message);
    this.logFile = logFile;
    this.status = status;
  }
}

// Why a spawnSync run didn't succeed, in words. A process that couldn't start
// (r.error: EAGAIN with too many processes, ENOENT) or was killed (r.signal)
// writes nothing to its log, so without this the cause was lost: on 4 Nov the
// self-update's unit tests "failed" in 0.1s with an empty log and nothing said
// why (Sentry THREADBEAR-M).
export function whyFailed(r, cmd) {
  const what = path.basename(cmd);
  if (r.error) return `${what} couldn't run: ${r.error.message}`;
  if (r.signal) return `${what} was stopped by ${r.signal}`;
  return `${what} exited with ${r.status}`;
}

// How many times the Mac has gone to sleep since it started, from pmset's
// counters (it reads them and changes nothing), or undefined off macOS or if
// pmset can't say. A sleep in the middle of a step stops the clock for its
// tests' deadlines and the smoke test's waits but not the wall clock, so a
// step that failed across one says nothing about the code: the self-update
// then marked a good main as broken and held the runner off it until main
// moved on (a fork's, 8 Nov).
export function sleepCount() {
  if (process.platform !== 'darwin') return undefined;
  const r = spawnSync('/usr/bin/pmset', ['-g', 'stats'], { encoding: 'utf8', timeout: 5000 });
  const m = r.status === 0 ? /^Sleep Count:\s*(\d+)/m.exec(r.stdout ?? '') : null;
  return m ? Number(m[1]) : undefined;
}

// What a failure across a sleep throws instead. deploy.mjs exits with
// SLEPT_EXIT for it, and update.ts (SLEPT_EXIT there too) tries the same
// commit again later. An exit code rather than words in the log, as the log
// has the tests' output too, and a failing test that printed the same words
// would have made a broken main look like a sleep (Codex, 9 Nov).
// The Node a LaunchAgent's ProgramArguments (or a systemd unit's ExecStart)
// starts launch.mjs with: the argument just before it. ["Threadbear
// Runner", node, launch.mjs] on a Mac, [node, launch.mjs] on Linux.
// undefined when it doesn't start `launch` or names no Node first.
export function nodeBeforeLaunch(args, launch, same = (a, b) => a === b) {
  const at = args.findIndex((a) => typeof a === 'string' && same(a, launch));
  const node = at > 0 ? args[at - 1] : undefined;
  return typeof node === 'string' && path.isAbsolute(node) ? node : undefined;
}

export const SLEPT_EXIT = 75; // EX_TEMPFAIL
export class SleptThrough extends Fail {}
export function checkSlept(before, after, what) {
  if (before !== undefined && after !== undefined && after > before)
    throw new SleptThrough(`The Mac slept during ${what}, so it's tried again later`);
}

function tail(file, lines = 40) {
  try {
    return fs.readFileSync(file, 'utf8').trimEnd().split('\n').slice(-lines).join('\n');
  } catch {
    return `(no log at ${file})`;
  }
}

// Runs a step with its output in a log file, printed only if it fails.
// io: { say, write, error, spawn, tmpdir, slept } for tests; the defaults are
// the console, spawnSync and pmset's sleep count.
export function step(name, cmd, args, opts, io = {}) {
  const { say = console.log, write = (s) => process.stdout.write(s), error = console.error, spawn = spawnSync, tmpdir = os.tmpdir(), slept = sleepCount } = io;
  write(`${name}... `);
  const started = Date.now();
  // The name is cut short for the file: a rerun of a dozen failed files made
  // a name past macOS's 255 bytes, and the update died with ENAMETOOLONG (7 Nov).
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').slice(0, 60).replace(/-+$/, '');
  const logFile = path.join(tmpdir, `threadbear-deploy-${process.pid}-${slug}.log`);
  const fd = fs.openSync(logFile, 'w');
  const sleepsBefore = slept();
  const r = spawn(cmd, args, { ...opts, stdio: ['ignore', fd, fd] });
  fs.closeSync(fd);
  const secs = `${((Date.now() - started) / 1000).toFixed(1)}s`;
  if (r.status === 0) {
    say(`ok (${secs})`);
    fs.rmSync(logFile, { force: true });
    return;
  }
  const why = whyFailed(r, cmd);
  // In the log too, so the file named in the error says what happened.
  fs.appendFileSync(logFile, `\n[deploy] ${why}\n`);
  say(`failed (${secs}): ${why}`);
  error(tail(logFile));
  // Not a StepFailed, so unitTests doesn't rerun it either.
  checkSlept(sleepsBefore, slept(), `${name} (${why}; the output is in ${logFile})`);
  throw new StepFailed(`${name} failed (${why}); the whole output is in ${logFile}`, logFile, r.status);
}

// The check hosts in a runner.json, as check.sh reads them: "checkHosts" (a
// list) over "checkHost", each a plain ssh host name. Empty when there's none.
export function checkHostsFrom(json) {
  const h = Array.isArray(json?.checkHosts) ? json.checkHosts : json?.checkHost;
  return (Array.isArray(h) ? h : [h]).filter((x) => typeof x === 'string' && /^[A-Za-z0-9][A-Za-z0-9._@-]*$/.test(x));
}

// The self-update's typecheck and unit tests, on a check host when the
// runner has one. Mike asked on 9 Nov, with his M5 at a load of 50: every
// runner update (six that morning) ran the whole unit suite on it for 10
// minutes, the one suite that never went to Tinker Desk with the others on
// 6 Nov. check.sh --runner sends them there (scripts/check-remote.sh), or to
// a Linux host, and runs them here at low priority only when no host can be
// reached, as it does for a thread's checks. --runner rather than --quick:
// only what the runner's own tests cover, so a website or iPhone test can't
// hold the runner back from main (update.ts keys a failure by runner/'s tree,
// so fixing one of those wouldn't even retry it). With no check host (a fork,
// a sandbox), the steps run here as before. Returns whether it ran them.
//
// No host with room, or none free for three hours before the check could
// start, is the hosts' trouble, not main's: check-remote.sh exits with
// THREADBEAR_CHECK_BUSY_EXIT for those (an exit code it's told, not words
// in the output, which a check's own output could fake or leave out), and
// that's ChecksUnavailable, which update.ts tries again later rather than
// keeping as a failure until runner/ changes. Anything else is a failed
// check. That gets one more whole run, as unitTests() gives one, since
// check.sh only reruns up to 8 named files; not when it ran here, which
// would be the full suite on this Mac twice.
export const UNAVAILABLE_EXIT = 69; // EX_UNAVAILABLE
export class ChecksUnavailable extends Fail {}
export function remoteChecks(hosts, root, opts, io = {}) {
  const { say = console.log, node = process.execPath } = io;
  const checkSh = path.join(root, 'check.sh');
  if (!hosts.length || !fs.existsSync(checkSh)) return false;
  // The deploy's own Node, which check.sh would otherwise take as the pinned
  // nvm path (an install with --node elsewhere has none there), here and as
  // what it uses on a fallback run.
  const env = { ...opts.env, THREADBEAR_CHECK_HOST: hosts.join(','), THREADBEAR_NODE: node, THREADBEAR_CHECK_BUSY_EXIT: String(UNAVAILABLE_EXIT) };
  // Each try is classified the same way, the second included (Codex, 9 Nov).
  const run = (name) => {
    try {
      step(name, checkSh, ['--runner'], { ...opts, cwd: root, env }, io);
    } catch (e) {
      if (e instanceof StepFailed && e.status === UNAVAILABLE_EXIT) throw new ChecksUnavailable(`${e.message}. No check host could take it, so it's tried again later`);
      throw e;
    }
  };
  const name = `Typecheck and unit tests (on ${hosts.join(' or ')})`;
  try {
    run(name);
    return true;
  } catch (e) {
    if (!(e instanceof StepFailed) || e instanceof ChecksUnavailable) throw e;
    if (/run on this Mac instead/.test(readOr(e.logFile))) throw e;
    say(`Running them once more, in case that was a flaky test.`);
    run(`${name}, again`);
    say(`The second run passed (first run's output: ${e.logFile})`);
    return true;
  }
}

// The full suite runs files in parallel on a Mac that's usually busy with
// threads, so a test with a timing assumption can fail for no real reason
// and hold the runner back from main. Files that failed get one more go, one
// at a time; a real break fails both runs and still stops the update.
// A run that failed without naming a test file (npm couldn't start, was
// killed, or died before the tests reported) gets the whole suite once more,
// as that isn't a test failing, and update.ts won't try this tree again.
// Both runs give up on a test after 5 minutes, and end once the tests are
// done even if one left a server open (package.json's test script has the
// same flags). Without them a test that failed with its socket still open
// held the run, and so the runner, off main for 48 minutes on 29 Oct, until
// it was stopped by hand.
export const TEST_LIMITS = ['--test-timeout=300000', '--test-force-exit'];
export function unitTests(c, opts, io = {}) {
  const { say = console.log } = io;
  try {
    step('Unit tests', c.npm, ['test'], opts, io);
  } catch (e) {
    if (!(e instanceof StepFailed)) throw e;
    const files = failedTestFiles(readOr(e.logFile));
    if (!files.length) {
      say(`The unit tests failed without naming a test file; running them once more.`);
      step('Unit tests, again', c.npm, ['test'], opts, io);
      say(`The second run passed (first run's output: ${e.logFile})`);
      return;
    }
    step(`Rerunning ${files.join(', ')}`, c.node ?? process.execPath, ['--import=./test/helpers/clean-env.ts', '--test', '--test-reporter=spec', '--test-concurrency=1', ...TEST_LIMITS, ...files], opts, io);
    say(`Flaky: ${files.join(', ')} failed in the full run and passed alone (first run's output: ${e.logFile})`);
  }
}

function readOr(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

// node --test's summary names each failure as "test at <file>:<line>:<col>".
export function failedTestFiles(output) {
  return [...new Set([...output.matchAll(/^test at (\S+?\.test\.ts):\d+:\d+$/gm)].map((m) => m[1]))];
}

// Whether the smoke test gives up on a release that hasn't said it's
// listening after `waited` ms, with the Mac's one-minute load per core what
// it is now. A healthy release listens in seconds; 90 s covers a busy Mac.
// Past that it keeps waiting only while the Mac is overloaded (more than
// twice as much to run as it has cores), up to ten minutes: on 6 Nov a load
// of 94 on 18 cores failed a release that started in 7 s once it eased, and
// a release that crashes is caught at once whatever the wait.
export function smokeTimedOut(waited, loadPerCore) {
  if (waited <= 90_000) return false;
  return loadPerCore <= 2 || waited > 10 * 60_000;
}

// The smoke test's hello, asked again until it's answered, the release
// exits, or the wait it had to start listening (`smokeTimedOut`, from when
// it was started) runs out. Saying it's listening comes first, but on a busy
// Mac the handshake after it timed out once and failed a healthy release,
// which only one try didn't allow for (emilsbauga's fork, 8 Nov).
// io: { probe, waited, load, exited, sleep }, all but sleep plain getters.
export async function smokeHello(io) {
  for (;;) {
    const exited = io.exited();
    if (exited) throw new Error(`it exited (${exited.code ?? exited.signal}) before hello`);
    const p = await io.probe();
    if (p.state === 'up') return p;
    const waited = io.waited();
    if (smokeTimedOut(waited, io.load())) throw new Error(`hello got no answer after ${Math.round(waited / 1000)} s`);
    await io.sleep(250);
  }
}
