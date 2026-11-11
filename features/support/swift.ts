// The Swift Mac app, through app/Tools/dev-app.sh: a debug build in a temp
// bundle, on the mock runner, with its window kept behind everything.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Driver, Dump } from './driver.ts';

const run = promisify(execFile);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const script = path.join(root, 'app/Tools/dev-app.sh');
let instances = 0;

export class SwiftDriver implements Driver {
  readonly name = 'swift' as const;
  private id = `spec-${process.pid}-${++instances}`;

  private async script(args: string[], timeoutSeconds = 60): Promise<string> {
    const { stdout } = await run(script, args, {
      timeout: timeoutSeconds * 1000,
      maxBuffer: 32 * 1024 * 1024,
      // The other Macs' demo threads are their own feature's business, and
      // leaving them out makes the inbox the same for both apps. A scenario
      // about them sets TB_MOCK_REMOTE (@remote-threads).
      env: { ...process.env, TB_MOCK_NO_REMOTE: process.env.TB_MOCK_REMOTE ? '' : '1' },
    });
    return stdout.trim();
  }

  async start(): Promise<void> {
    // The build is done once, up front (features/README.md): `start` would
    // otherwise spend several seconds finding nothing to build each scenario.
    const fixtures = process.env.TB_MOCK_FIXTURES ? ['--fixtures', process.env.TB_MOCK_FIXTURES] : [];
    await this.script(['start', '--mock', ...fixtures, '--no-build', '--id', this.id, '--size', '1180x800'], 120);
  }

  async do(cmd: Record<string, unknown>, timeoutSeconds = 30): Promise<string> {
    const out = await this.script(['--id', this.id, 'do', JSON.stringify(cmd), '--timeout', String(timeoutSeconds)], timeoutSeconds + 10);
    return out;
  }

  async dump(): Promise<Dump> {
    const dump = JSON.parse(await this.script(['--id', this.id, 'dump', '--json'])) as Dump;
    // No thread open means no transcript in its dump.
    dump.transcript ??= { rows: [] };
    return dump;
  }

  async shot(file: string): Promise<void> {
    await this.script(['--id', this.id, 'shot', file]);
  }

  async relaunch(): Promise<void> {
    await this.script(['--id', this.id, 'rebuild', '--no-build'], 120);
  }

  async stop(): Promise<void> {
    await this.script(['--id', this.id, 'stop'], 60).catch(() => {});
  }
}
