import type { Runner } from './runner.ts';

// A thread the runner starts by itself: bringing in Mike's release
// (bring-in.ts) or fixing a failed update to main (fix-update.ts). It
// starts on the install's own default mode, like any new thread, not
// always on full access (Mike, 9 Nov 2026, after the security review: these
// threads are handed words from upstream's commits or a build log and run
// code from a commit nobody has read yet, so on an install that starts
// threads on Auto they get Auto's checks too). On Ask, or Codex's Auto
// (which asks before edits), what it can't do without a yes waits on a
// card for the owner; the briefs say so. An install from before 9 Nov has
// no default set, and those threads stay on full access there.
export function startOwnThread(runner: Pick<Runner, 'create'>, r: { text: string; title: string; why: string }, cwd: string | undefined): string {
  return runner.create({ text: r.text, title: r.title, cwd, startedBy: { kind: 'runner', label: 'Threadbear', why: r.why } }).thread.id;
}
