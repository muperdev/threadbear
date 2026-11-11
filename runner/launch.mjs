#!/usr/bin/env node
// The LaunchAgent's program when the runner runs from releases (see
// scripts/install-runner.sh and docs/dev-tools.md). install-runner.sh copies
// it to <home>/runner/launch.mjs, next to releases/, current and
// state.json, and it runs from there on its own, so it imports nothing but
// Node built-ins. scripts/deploy.mjs imports the state helpers from here so
// both agree on the file format.
//
// It supervises: each runner is a child process, so during a handover a
// new release serves while the old one finishes the turns it still has
// (main() below). Each start records a boot. A release that keeps dying at
// startup (3 boots in 10 minutes that neither stayed up long enough for
// main.ts to mark it good nor stopped cleanly) is swapped for the last good
// one. A release that hasn't started listening within 90 seconds is
// stopped, so one that hangs at startup counts as crashing too.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
// THREADBARE_* was the old name for these variables (config.ts, adoptOldNames).
for (const [k, v] of Object.entries(process.env)) {
  const name = k.startsWith('THREADBARE_') ? 'THREADBEAR_' + k.slice(11) : undefined;
  if (name && process.env[name] === undefined) process.env[name] = v;
}

export const CRASH_BOOTS = 3;
// Long enough to hold three starts that each hang until the watchdog below
// stops them (launchd waits 5 s between starts). It can't catch a healthy
// release: one marked good is never rolled back, and clean stops (a
// restart, launchctl kickstart, a reboot) don't count.
export const CRASH_WINDOW_MS = 10 * 60_000;
export const START_TIMEOUT_MS = 90_000;
export const KEEP_BOOTS = 20;

export function emptyState() {
  return { current: null, previous: null, lastGood: null, boots: [] };
}

// A missing or unreadable state.json starts afresh rather than stopping the
// runner from booting; the current symlink still says what to run.
export function readState(dir) {
  try {
    const s = JSON.parse(fs.readFileSync(path.join(dir, 'state.json'), 'utf8'));
    if (s && typeof s === 'object' && !Array.isArray(s)) {
      return { ...emptyState(), ...s, boots: Array.isArray(s.boots) ? s.boots : [] };
    }
  } catch {
    // fall through
  }
  return emptyState();
}

// Temp file then rename, so a crash mid-write never leaves half a file.
// The temp name matches the pattern the runner's own atomic writes use.
export function writeState(dir, state) {
  const file = path.join(dir, 'state.json');
  const tmp = path.join(dir, `.state.json.${process.pid}.${crypto.randomBytes(4).toString('hex')}.tmp`);
  fs.writeFileSync(tmp, JSON.stringify(state, null, 2) + '\n');
  fs.renameSync(tmp, file);
}

// The release `current` points at, or null when there's no link yet.
export function readCurrentLink(dir) {
  try {
    return path.basename(fs.readlinkSync(path.join(dir, 'current')));
  } catch {
    return null;
  }
}

// A new link under a temp name renamed over the old one: rename is atomic,
// so `current` always names a whole release. The target is relative so the
// data dir can move.
export function pointCurrent(dir, id) {
  const link = path.join(dir, 'current');
  const tmp = `${link}.${process.pid}.tmp`;
  fs.rmSync(tmp, { force: true });
  fs.symlinkSync(path.join('releases', id), tmp);
  fs.renameSync(tmp, link);
}

export function releaseExists(dir, id) {
  return typeof id === 'string' && id !== '' && fs.existsSync(path.join(dir, 'releases', id, 'src', 'main.ts'));
}

// Decides what this boot starts, without touching the disk. `state.current`
// should already agree with the symlink. Returns the state to save, the
// release to start, and `rolledBack` when it went back to lastGood.
//
// Only boots since current was last activated count (deploy-runner.sh
// records `activated`), so `--rollback --force` onto a release that crashed
// a few minutes ago gives it fresh tries instead of going straight back.
//
// `configBroken` (runnerJsonBroken below) holds back the rollback: a runner
// that won't start on a broken runner.json isn't a broken release, and one
// from before 9 Nov reads that file as {} and writes over it.
export function planBoot(state, now, exists, pid = process.pid, configBroken = false) {
  const at = new Date(now).toISOString();
  const current = state.current;
  const activatedAt = state.activated && state.activated.release === current ? Date.parse(state.activated.at) : NaN;
  const since = Math.max(now - CRASH_WINDOW_MS, Number.isFinite(activatedAt) ? activatedAt - 1 : -Infinity);
  const earlier = state.boots.filter((b) => b && b.release === current && !b.clean && Date.parse(b.at) > since).length;
  // This boot counts: the third start in the window is the one that
  // switches, so a broken release gets two tries.
  const crashLooping = Boolean(current) && current !== state.lastGood && earlier + 1 >= CRASH_BOOTS;
  const canRollBack = crashLooping && !configBroken && exists(state.lastGood);
  const start = canRollBack ? state.lastGood : current;
  const next = { ...state, boots: [...state.boots, { release: start, at, pid }].slice(-KEEP_BOOTS) };
  let rolledBack = null;
  if (canRollBack) {
    rolledBack = { from: current, to: start, at };
    // previous names the bad release, so `deploy-runner.sh --rollback` can
    // try it again on purpose.
    Object.assign(next, { current: start, previous: current, rolledBack });
  }
  return { state: next, start, rolledBack, stuck: crashLooping && !canRollBack && !configBroken, heldForConfig: crashLooping && configBroken };
}

// Whether <home>/runner.json is there but doesn't read as a JSON object
// (the runner's config.ts, RunnerJsonError, has the reasons). Missing is a
// first run, not broken.
export function runnerJsonBroken(home) {
  const file = path.join(home, 'runner.json');
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (e) {
    if (e && e.code === 'ENOENT') {
      try {
        return fs.lstatSync(file).isSymbolicLink();
      } catch {
        return false;
      }
    }
    return true;
  }
  try {
    const json = JSON.parse(text);
    return !json || typeof json !== 'object' || Array.isArray(json);
  } catch {
    return true;
  }
}

// Marks this process's boot as a clean stop: the runner listened and then
// exited 0 (a planned restart, launchctl kickstart, a reboot). planBoot
// doesn't count those, so restarting a new release a few times in a row
// isn't mistaken for a crash loop.
export function markBootClean(dir, at, pid = process.pid) {
  const state = readState(dir);
  const boot = state.boots.findLast((b) => b && b.at === at && b.pid === pid);
  if (!boot || boot.clean) return false;
  boot.clean = true;
  writeState(dir, state);
  return true;
}

function log(level, msg, extra) {
  // Same line format as the runner's log (src/log.ts); launchd sends
  // stderr to the runner's log file. Written straight to the fd so a line
  // logged from the 'exit' handler isn't lost.
  const line = `${new Date().toISOString()} ${level} launch: ${msg}${extra ? ' ' + JSON.stringify(extra) : ''}\n`;
  try {
    fs.writeSync(2, line);
  } catch {
    // no stderr to write to
  }
}

// How long after a child that didn't come up the supervisor tries again,
// when it can't leave that to launchd (another runner is still finishing
// its turns). launchd's ThrottleInterval is 5 s too.
export const CRASH_RETRY_MS = 5_000;
// A runner that restarted on purpose (exit 0 after listening) is started
// again this soon.
export const RESTART_MS = 500;
// After SIGTERM, how long a child that hasn't exited gets before SIGKILL:
// the watchdog's, and the supervisor's own stop (the plist's ExitTimeOut
// allows for it).
export const KILL_AFTER_MS = 5_000;
export const STOP_GRACE_MS = 20_000;
// Printed by every runner once it's serving (main.ts, and releases from
// before the IPC message), so either tells the supervisor it's up.
export const LISTENING_LINE = /threadbear-runner listening on \d+/;

function msEnv(name, fallback) {
  const v = Number(process.env[name]);
  return Number.isFinite(v) && v >= 0 ? v : fallback;
}

// The supervisor: launchd's program. Each runner is a child process
// (`node <release>/src/main.ts`, THREADBEAR_SUPERVISED=1, an IPC channel),
// so a new release can start while an older one finishes the turns it
// still has (a handover, docs/dev-tools.md). Only the newest child is
// "the runner"; older ones are draining and are only waited for.
//
// Each start records a boot. The newest child exiting 0 after it listened
// is a planned restart and is started again straight away; any other exit
// is a failed start. With no other child left the supervisor exits then
// (code 1) and launchd starts it again, as before handovers; with a
// draining child still running it tries again itself after CRASH_RETRY_MS.
async function main() {
  const dir = path.dirname(fileURLToPath(import.meta.url));
  const children = new Set();
  let newest = null;
  let stopping = false;
  let retry = null;

  const startTimeoutMs = msEnv('THREADBEAR_START_TIMEOUT_MS', START_TIMEOUT_MS);
  const crashRetryMs = msEnv('THREADBEAR_CRASH_RETRY_MS', CRASH_RETRY_MS);
  const restartMs = msEnv('THREADBEAR_RESTART_MS', RESTART_MS);
  const killAfterMs = msEnv('THREADBEAR_KILL_AFTER_MS', KILL_AFTER_MS);
  const stopGraceMs = msEnv('THREADBEAR_STOP_GRACE_MS', STOP_GRACE_MS);

  // A closed stdout (a test that stopped reading) must not take the
  // supervisor down with its children.
  process.stdout.on('error', () => {});

  // Plans a boot and starts it as the newest child. Returns the child, or
  // null when there's nothing to start.
  function start() {
    const state = readState(dir);
    // The symlink is what deploy-runner.sh activates; state.json follows it.
    const linked = readCurrentLink(dir);
    if (linked) state.current = linked;
    if (!state.current) {
      log('ERROR', 'no release to start; build one with scripts/deploy-runner.sh or reinstall with scripts/install-runner.sh', { dir });
      return null;
    }
    const plan = planBoot(state, Date.now(), (id) => releaseExists(dir, id), 0, runnerJsonBroken(path.dirname(dir)));
    if (plan.heldForConfig) log('WARN', `release ${plan.start} keeps stopping at startup, but runner.json is broken, so not going back to an older release (it would write over the file); fix runner.json and it starts`);
    if (plan.rolledBack) {
      log('WARN', `release ${plan.rolledBack.from} failed to start ${CRASH_BOOTS} times in ${CRASH_WINDOW_MS / 60_000} minutes without being marked good; going back to ${plan.start}`);
      try {
        pointCurrent(dir, plan.start);
      } catch (e) {
        // Bookkeeping must never stop the runner from starting: lastGood
        // still starts from its folder, and the next boot sees the link
        // still on the bad release and goes back again.
        log('ERROR', 'could not point current at the release it went back to', { release: plan.start, error: String(e) });
      }
    } else if (plan.stuck) {
      log('WARN', `release ${plan.start} keeps crashing at startup and there's no other good release to go back to`, { lastGood: state.lastGood });
    }
    const root = path.join(dir, 'releases', plan.start);
    const proc = spawn(process.execPath, [path.join(root, 'src', 'main.ts')], {
      cwd: root,
      env: { ...process.env, THREADBEAR_SUPERVISED: '1' },
      stdio: ['ignore', 'pipe', 'inherit', 'ipc'],
    });
    const boot = plan.state.boots.at(-1);
    boot.pid = proc.pid;
    try {
      writeState(dir, plan.state);
    } catch (e) {
      // Bookkeeping must never stop the runner from starting.
      log('WARN', 'could not record the boot', { error: String(e) });
    }
    log('INFO', 'starting release', { release: plan.start, pid: proc.pid });
    const child = { proc, pid: proc.pid, release: plan.start, at: boot.at, startedMs: Date.now(), listened: false, draining: false, watchdog: null, kill: null, exited: false };
    children.add(child);
    newest = child;

    // Forwarded unchanged; only scanned for the listening line.
    let partial = '';
    proc.stdout.on('data', (chunk) => {
      process.stdout.write(chunk);
      if (child.listened) return;
      partial += chunk.toString('utf8');
      if (LISTENING_LINE.test(partial)) listening(child);
      else partial = partial.slice(partial.lastIndexOf('\n') + 1);
    });
    proc.on('message', (m) => {
      if (!m || typeof m !== 'object') return;
      if (m.type === 'listening') listening(child);
      else if (m.type === 'handover') handover(child);
    });
    // A spawn that fails (no node at that path) gets 'error' and then, or
    // instead, 'exit'.
    proc.on('error', (e) => {
      log('ERROR', 'could not start release', { release: child.release, error: String(e) });
      exited(child, 1, null);
    });
    proc.on('exit', (code, signal) => exited(child, code, signal));

    // A release that hasn't started listening in time is stopped, so one
    // that hangs at startup counts as crashing too.
    child.watchdog = setTimeout(() => {
      if (child.listened || child.exited) return;
      log('ERROR', `release ${child.release} wasn't listening after ${Math.round(startTimeoutMs / 1000)} s; stopping it so it counts as a failed start`);
      child.timedOut = true;
      stopChild(child);
    }, startTimeoutMs);
    return child;
  }

  function listening(child) {
    if (child.listened) return;
    child.listened = true;
    clearTimeout(child.watchdog);
  }

  // SIGTERM, then SIGKILL if it's still there.
  function stopChild(child, graceMs = killAfterMs) {
    if (child.exited) return;
    try {
      child.proc.kill('SIGTERM');
    } catch {
      // gone
    }
    clearTimeout(child.kill);
    child.kill = setTimeout(() => {
      if (!child.exited) {
        try {
          child.proc.kill('SIGKILL');
        } catch {
          // gone
        }
      }
    }, graceMs);
  }

  function reply(child, msg) {
    try {
      if (child.proc.connected) child.proc.send(msg);
    } catch {
      // the child is going; it falls back to a plain restart or exits
    }
  }

  // The newest runner asks for a newer one to take over while it finishes
  // its turns. It becomes draining and nothing starts it again.
  function handover(child) {
    const no = (error) => {
      log('WARN', 'handover refused', { release: child.release, pid: child.pid, error });
      reply(child, { type: 'handover', ok: false, error });
    };
    if (stopping) return no('the supervisor is stopping');
    if (child !== newest) return no('only the newest runner can hand over');
    if (!child.listened) return no("this runner hasn't started listening yet");
    child.draining = true;
    newest = null;
    const next = start();
    if (!next) {
      child.draining = false;
      newest = child;
      return no('there is no release to start');
    }
    log('INFO', 'handover: a new runner takes over while the old one finishes its turns', { from: child.release, fromPid: child.pid, to: next.release, pid: next.pid });
    reply(child, { type: 'handover', ok: true, pid: next.pid });
  }

  function exited(child, code, signal) {
    if (child.exited) return;
    child.exited = true;
    clearTimeout(child.watchdog);
    clearTimeout(child.kill);
    children.delete(child);
    const clean = code === 0 && child.listened && !child.timedOut;
    if (clean) {
      try {
        markBootClean(dir, child.at, child.pid);
      } catch {
        // the next boot just counts this one
      }
      if (child.draining) log('INFO', 'a draining runner finished and exited', { release: child.release, pid: child.pid });
    } else {
      // A release that calls process.exit leaves nothing else in the log,
      // and after a crash rollback an agent reading tb logs needs to see
      // which release stopped, when and how.
      let good = false;
      try {
        good = readState(dir).lastGood === child.release;
      } catch {
        // unreadable: say it counts
      }
      const secs = Math.round((Date.now() - child.startedMs) / 1000);
      const how = code !== null && code !== undefined ? `code ${code}` : `signal ${signal}`;
      log('ERROR', `release ${child.release} exited with ${how} after ${secs} s${child.listened ? '' : ', before it was listening'}${good ? '' : ', not marked good, so it counts as a failed start'}${child.draining ? ' (it was finishing its turns after a handover)' : ''}`);
    }
    if (stopping) {
      if (children.size === 0) process.exit(0);
      return;
    }
    if (child !== newest) return;
    newest = null;
    if (clean) {
      later(restartMs);
      return;
    }
    if (children.size === 0) {
      // Nothing else is running: launchd starts the supervisor again after
      // its ThrottleInterval, as it did before handovers.
      process.exit(1);
    }
    later(crashRetryMs);
  }

  function later(ms) {
    clearTimeout(retry);
    retry = setTimeout(() => {
      retry = null;
      if (stopping || newest) return;
      if (!start() && children.size === 0) process.exit(1);
    }, ms);
  }

  function stopAll(signal) {
    if (stopping) return;
    stopping = true;
    clearTimeout(retry);
    log('INFO', 'stopping', { signal, children: [...children].map((c) => c.pid) });
    if (children.size === 0) process.exit(0);
    for (const c of children) stopChild(c, stopGraceMs);
  }
  for (const sig of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(sig, () => stopAll(sig));
  // If the supervisor itself goes some other way, its children don't
  // outlive it (they also stop when the IPC channel closes).
  process.on('exit', () => {
    for (const c of children) {
      try {
        c.proc.kill('SIGTERM');
      } catch {
        // gone
      }
    }
  });

  if (!start()) process.exit(1);
}

// Only when run as the program, so deploy.mjs and the tests can import the
// helpers above without starting a runner.
function isMain() {
  try {
    return fs.realpathSync(process.argv[1] ?? '') === fs.realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (isMain()) {
  main().catch((e) => {
    log('ERROR', 'release failed to start', { error: e?.stack ?? String(e) });
    process.exit(1);
  });
}
