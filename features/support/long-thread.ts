// A fixture folder for the long-thread scenarios: "A very long thread" (longer
// than one page of events, so paging has somewhere to go) and one short demo
// thread to go to and come back from. Made once per machine and kept, so the
// runs that use it don't leave a copy behind each.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

export function longThreadFolder(): string {
  const dir = path.join(fs.realpathSync(os.tmpdir()), 'tb-long-thread-fixture');
  if (fs.existsSync(path.join(dir, 'long_thread', 'thread.json'))) return dir;
  // Made under a temporary name and renamed into place whole, as another run
  // may be doing the same and only one of them should win.
  const making = fs.mkdtempSync(`${dir}-making-`);
  // 250 turns, about 1,750 events: three pages' worth.
  execFileSync(process.execPath, [path.join(root, 'desktop/scripts/make-long-thread.mjs'), making, '250'], { stdio: 'ignore' });
  fs.cpSync(path.join(root, 'app/Tools/mock-runner/fixtures/demo-dark-mode'), path.join(making, 'demo-dark-mode'), { recursive: true });
  try {
    fs.renameSync(making, dir);
  } catch {
    // Another run got there first; its copy is the same as this one.
    fs.rmSync(making, { recursive: true, force: true });
  }
  return dir;
}
