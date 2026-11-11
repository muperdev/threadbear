// scripts/check-remote.sh losing the race for a free check host. On 6 Nov
// an app check's copy took 22 minutes over a slow uplink, another check took
// Tinker Desk meanwhile, and the run deleted its finished copy and copied
// again. Now a lost race keeps the copy: after a slow copy it waits on that
// host, after a quick one it chooses again and only copies if it moves.
// Runs the real script from a temp checkout, with a fake ssh that answers
// the probe and the lock from files and runs everything else locally, and
// an rsync that copies to "host:path" as plain local paths.
// The fake hosts behind the check-remote tests, shared by the
// check-remote-*.test.ts files. It's one suite split across files because each
// test spawns the real script and waits for it, so in one file they ran one
// after another: 222 s of the unit suite's 227 s on Tinker Desk (9 Nov), with
// its other cores idle, and that was most of what every landing waited for.
import { after } from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { tmpDir } from './fakes.ts';

export const SCRIPT = path.join(import.meta.dirname, '..', '..', '..', 'scripts', 'check-remote.sh');

// ssh: drops its -o options, takes the host, and answers the probe and
// the lock from files: a host's state file says free or busy, its "lose"
// file how many lock-without-waiting tries to refuse (75), each of which
// leaves the host busy, as another check has it. Each host's home is a
// folder under $FAKE, and everything else (mkdir, the copy from the
// mirror, rm -rf, find) really runs there. A run says whether its copy is there.
// rsync: drops -e and its command, takes host:path as path, and logs an
// upload to a host's mirror as that host's copy.
const FAKE_RSYNC = `#!/bin/bash
args=()
while [ $# -gt 0 ]; do
  case "$1" in
    -e) shift ;;
    *:/*) h="\${1%%:*}"; p="\${1#*:}"; case "$p" in */mirror/*) echo "$h" >>"$FAKE/copies.log"; [ -z "$RSYNC_PEEK" ] || bash -c "$RSYNC_PEEK" >>"$FAKE/rsync-peek"; [ "$RSYNC_FAILS_ON" != "$h" ] || { [ ! -f "$FAKE/$h/df-after" ] || cp "$FAKE/$h/df-after" "$FAKE/$h/df-out"; exit 11; } ;; esac; args+=("$p") ;;
    *) args+=("$1") ;;
  esac
  shift
done
exec /usr/bin/rsync "\${args[@]}"
`;

const FAKE_SSH = `#!/bin/bash
while [ "$1" = -o ]; do echo "$2" >>"$FAKE/ssh-opts.log"; shift 2; done
host="$1"; shift
cmd="$*"
d="$FAKE/$host"
echo "$host $cmd" >>"$FAKE/ssh.log"
[ -d "$d" ] || exit 255
# The watch's ping: fails once the host's "dead" file is there.
if [ "$cmd" = true ]; then [ -f "$d/dead" ] && exit 255; exit 0; fi
run_copy() {
  # A host's "hang" file: it stops answering as the check starts.
  if [ -f "$d/hang" ]; then : >"$d/dead"; sleep 300; exit 0; fi
  # "stubborn": the same, but the hung check ignores SIGTERM.
  if [ -f "$d/stubborn" ]; then : >"$d/dead"; trap '' TERM; sleep 300; exit 0; fi
  # A host's "drop" file: the connection goes during the check (ssh's 255).
  [ ! -f "$d/drop" ] || exit 255
  script="$(printf '%s' "$cmd" | grep -oE "$d/home/[^ \\\\']*/scripts/check-remote.sh" | head -1)"
  if [ -n "$script" ] && [ -f "$script" ] && [ -f "$(dirname "$script")/../.check-changed" ]; then
    echo "ran on $host $1 from $script"
    [ -z "$PEEK" ] || (cd "$(dirname "$script")/.." && bash -c "$PEEK") >"$FAKE/peek"
    # A failed step's log path, as check.sh prints it there, and one that
    # only looks like it.
    if [ -n "$PRINT_LOG" ]; then
      id="$(printf '%s' "$cmd" | grep -oE 'runs/[0-9]+-[0-9]+-[0-9]+/' | head -1 | cut -d/ -f2)"
      b="$d/home/.cache/threadbear-check"
      echo "log at $b/tmp-$id/threadbear-check.Ab12/step.log"
      echo "near $d/home/Xcache/threadbear-check/tmp-$id/threadbear-check.Ab12/step.log"
    fi
    # The check failing, and the disk as a host's "df-after" file has it then.
    if [ -n "$RUN_FAILS" ]; then
      [ ! -f "$d/df-after" ] || cp "$d/df-after" "$d/df-out"
      # What keep_logs wrote there, as it failed.
      if [ -n "$KEPT_KB$KEPT_EMPTY" ]; then
        id="$(printf '%s' "$cmd" | grep -oE 'runs/[0-9]+-[0-9]+-[0-9]+/' | head -1 | cut -d/ -f2)"
        mkdir -p "$d/home/.cache/threadbear-check/kept-$id"
        printf '%s' "$KEPT_KB" >"$d/home/.cache/threadbear-check/kept-$id/free-kb"
      fi
      exit 1
    fi
  else echo "no copy on $host for: $cmd"; exit 1; fi
}
# The host's commands find a df that prints its "df-out" file; the real awk
# reads that, from the script's own command.
PATH="$d/bin:$PATH"
case "$cmd" in
  *"mkdir -p"*mirror*)
    # A host's "hang-copy" file: it stops answering during the copy.
    if [ -f "$d/hang-copy" ]; then : >"$d/dead"; sleep 300; exit 0; fi
    bash -c "$cmd" ;;
  *"uname -s"*)
    # A host's "probe-fails" file: it answers, but the probe's mkdir fails.
    [ ! -f "$d/probe-fails" ] || exit 1
    # A host's "base" file says what folder it reports, if not its own.
    b="$d/home/.cache/threadbear-check"; [ -f "$d/base" ] && b="$(cat "$d/base")"
    # A host's "os" file says what system it is, if not a Mac.
    printf '%s\\n%s\\n%s\\n' "$b" "$(cat "$d/os" 2>/dev/null || echo Darwin)" "$(cat "$d/state")"
    # What the probe runs after the lock's "fi && ", for real.
    (cd "$d/home" && bash -c "\${cmd##*fi && }") ;;
  *"lockf -t 0 -k"*)
    n="$(cat "$d/lose" 2>/dev/null || echo 0)"
    if [ "$n" -gt 0 ]; then echo $((n - 1)) >"$d/lose"; [ ! -f "$d/df-after" ] || cp "$d/df-after" "$d/df-out"; [ -f "$d/looks-free" ] || echo busy >"$d/state"; exit 75; fi
    run_copy now ;;
  *"lockf -k"*) run_copy "after waiting" ;;
  *"flock -n -E 75"*) run_copy now ;;
  *"flock "*) run_copy "after waiting" ;;
  *) bash -c "$cmd" ;;
esac
`;

// looksFree: the probe still says free after each lost race (another check
// keeps grabbing it between the probe and the lock).
// This Mac's home for the run: short and under /tmp, so the ssh socket's
// folder fits (a socket's path is short), and never the real one. Its
// TMPDIR is a folder of the test's own, to see what's left there.
const homes: string[] = [];
after(() => homes.forEach((h) => fs.rmSync(h, { recursive: true, force: true })));
export function shortHome(): string {
  const h = fs.mkdtempSync('/tmp/tbcr-');
  homes.push(h);
  return h;
}

// df -Pk as each system prints it, the lines copied from Tinker Desk and the
// cloud box on 7 Nov, with the given KiB available.
const whole = (kb: number | string) => (typeof kb === 'number' ? Math.ceil(kb) : kb);
export const macDf = (kb: number | string) => `Filesystem   1024-blocks      Used Available Capacity  Mounted on\n/dev/disk3s5   482797652 383744568 ${whole(kb)}    84%    /System/Volumes/Data\n`;
export const linuxDf = (kb: number | string) => `Filesystem     1024-blocks     Used Available Capacity Mounted on\n/dev/sda1         78397936 52179404  ${whole(kb)}      70% /\n`;
export const GiB = 1024 * 1024;

// df: what the host's df prints (plenty free, if not given). dfAfter: what it
// prints once a copy or a run has failed there. os: Linux, if not a Mac.
type Host = { state: string; lose?: number; looksFree?: boolean; base?: string; df?: string; dfAfter?: string; os?: string; hang?: boolean; hangCopy?: boolean; drop?: boolean; stubborn?: boolean };
export function setup(hosts: Record<string, Host>, opts: { home?: string } = {}) {
  const tmp = tmpDir('check-remote-');
  const home = opts.home ?? shortHome();
  const tmpdir = path.join(tmp, 'tmpdir');
  fs.mkdirSync(tmpdir);
  const fake = path.join(tmp, 'fake');
  const bin = path.join(tmp, 'bin');
  const repo = path.join(tmp, 'repo');
  fs.mkdirSync(bin, { recursive: true });
  fs.mkdirSync(path.join(repo, 'scripts'), { recursive: true });
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  fs.writeFileSync(path.join(bin, 'ssh'), FAKE_SSH, { mode: 0o755 });
  fs.writeFileSync(path.join(bin, 'rsync'), FAKE_RSYNC, { mode: 0o755 });
  for (const [h, s] of Object.entries(hosts)) {
    fs.mkdirSync(path.join(fake, h, 'home'), { recursive: true });
    fs.writeFileSync(path.join(fake, h, 'state'), s.state);
    if (s.lose) fs.writeFileSync(path.join(fake, h, 'lose'), String(s.lose));
    if (s.looksFree) fs.writeFileSync(path.join(fake, h, 'looks-free'), '');
    if (s.base !== undefined) fs.writeFileSync(path.join(fake, h, 'base'), s.base);
    if (s.os) fs.writeFileSync(path.join(fake, h, 'os'), s.os);
    if (s.hang) fs.writeFileSync(path.join(fake, h, 'hang'), '');
    if (s.stubborn) fs.writeFileSync(path.join(fake, h, 'stubborn'), '');
    if (s.drop) fs.writeFileSync(path.join(fake, h, 'drop'), '');
    if (s.hangCopy) fs.writeFileSync(path.join(fake, h, 'hang-copy'), '');
    if (s.dfAfter !== undefined) fs.writeFileSync(path.join(fake, h, 'df-after'), s.dfAfter);
    fs.writeFileSync(path.join(fake, h, 'df-out'), s.df ?? macDf(100 * GiB));
    fs.mkdirSync(path.join(fake, h, 'bin'));
    fs.writeFileSync(path.join(fake, h, 'bin', 'df'), `#!/bin/bash\n[ "$*" = "-Pk ." ] || exit 9\ncat "${path.join(fake, h, 'df-out')}"\n`, { mode: 0o755 });
  }
  fs.copyFileSync(SCRIPT, path.join(repo, 'scripts', 'check-remote.sh'));
  const git = (...args: string[]) => execFileSync('git', ['-C', repo, ...args], { stdio: 'ignore' });
  git('init', '-q', '-b', 'main');
  git('add', '-A');
  git('-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-qm', 'x');
  return {
    fake,
    repo,
    home,
    tmpdir,
    // peek: a command run in the copy as its check starts, its output kept.
    // env: more for the script's environment (PRINT_LOG, RSYNC_PEEK).
    run(hostList: string, staySecs: number, peek = '', env: Record<string, string> = {}) {
      const base: NodeJS.ProcessEnv = { ...process.env };
      delete base.XDG_CACHE_HOME;
      const r = spawnSync('bash', [path.join(repo, 'scripts', 'check-remote.sh'), hostList, '--quick'], {
        encoding: 'utf8',
        env: { ...base, HOME: home, TMPDIR: tmpdir, PATH: `${bin}:${process.env.PATH}`, FAKE: fake, PEEK: peek, THREADBEAR_CHECK_STAY_SECS: String(staySecs), ...env },
      });
      const read = (f: string) => (fs.existsSync(path.join(fake, f)) ? fs.readFileSync(path.join(fake, f), 'utf8') : '');
      // The copies left on each host afterwards.
      const left = (h: string) => {
        const runs = path.join(fake, h, 'home', '.cache', 'threadbear-check', 'runs');
        return fs.existsSync(runs) ? fs.readdirSync(runs) : [];
      };
      return { ...r, ssh: read('ssh.log'), sshOpts: read('ssh-opts.log'), rsyncPeek: read('rsync-peek'), copies: read('copies.log').trim().split('\n').filter(Boolean), left, peek: read('peek') };
    },
  };
}
