// Writes every scenario (test/helpers/scenario-list.ts) into a real backend,
// as threads of one made-up runner, for the parity run's screenshots
// (scripts/parity-run.sh): the same threads the unit tests compare in
// memory, now read by the real website, iPhone app and Mac app. Prints
// JSON: a phone token for the backend, the runner's id, and each
// scenario's thread id.
//
//   node runner/scripts/parity-seed.ts <convex url> <owner key>
import { convexPoster } from '../src/sync.ts';
import { playInto } from '../test/helpers/routes.ts';
import { SCENARIOS } from '../test/helpers/scenario-list.ts';

const [url, key] = process.argv.slice(2);
if (!url || !key) {
  console.error('usage: parity-seed.ts <convex url> <owner key>');
  process.exit(2);
}
const runnerId = 'parityrun001';
const post = convexPoster(url);
const { sync, ids } = await playInto(post, { key, runnerId, name: 'Parity runner' }, SCENARIOS.map((s) => s.play));
await sync.stop();
const { token } = (await post('mutation', 'sync:issueToken', { key, kind: 'phone', name: 'parity run' })) as { token: string };
console.log(JSON.stringify({ token, runnerId, threads: SCENARIOS.map((s, i) => ({ name: s.name, shows: s.shows, threadId: ids[i] })) }, null, 2));
