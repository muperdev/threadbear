// The scenarios: threads that each show something a person sees (a card,
// its notes, follow-ups, pictures in a message), played the same way by the
// unit tests (scenarios.test.ts, which compare this runner, another Mac and
// the phone in memory) and by the parity run (scripts/parity-seed.ts, which
// writes them into a real throwaway backend for the screenshots). Add one
// when a change alters what a thread shows.
import fs from 'node:fs';
import path from 'node:path';
import type { Play } from './routes.ts';
import { makePng } from './png.ts';
import { tmpDir } from './fakes.ts';

// shows: words each surface must show for it, which the parity run checks
// on the website and the Mac app as well as taking their pictures.
export type Scenario = { name: string; play: (p: Play) => void; shows: string[] };

export const SCENARIOS: Scenario[] = [
  {
    name: 'a decision card, with what each answer does and a pick',
    shows: ['Same thread or new?', 'One thread that moves', 'The old one is archived'],
    play: (p) => {
      p.say('Two ways to go.');
      p.endTurn({ needs: 'you', asks: [{ question: 'Same thread or new?', options: [{ label: 'Same', does: 'One thread that moves' }, { label: 'New', does: 'The old one is archived' }], suggested: 1 }] });
    },
  },
  {
    name: 'two asks, one with a link to look at',
    shows: ['Deploy the fix?', 'See the diff', 'Plug in the phone and say when.'],
    play: (p) => {
      p.say('Two things.');
      p.endTurn({ needs: 'you', asks: [
        { question: 'Deploy the fix?', options: [{ label: 'Deploy' }, { label: 'Wait' }], links: [{ title: 'See the diff', url: 'https://github.com/mikecann/threadbear/pull/1' }] },
        { question: 'Plug in the phone and say when.' },
      ] });
    },
  },
  {
    name: 'a card with a command to run',
    shows: ['Backfill the titles on prod?', 'npx convex run --prod migrations:backfill'],
    play: (p) => {
      p.say('It needs your OK for prod.');
      p.runAsks();
      p.endTurn({ needs: 'you', asks: [{ question: 'Backfill the titles on prod?', run: 'npx convex run --prod migrations:backfill' }] });
    },
  },
  {
    name: 'a done card with follow-ups',
    shows: ['Landed it.', 'Make the phone show Stop on a running thread', 'Check the website draws the new card'],
    play: (p) => {
      p.say('Landed it.');
      p.endTurn({ needs: 'nothing', next: ['Make the phone show Stop on a running thread', 'Check the website draws the new card'] });
    },
  },
  {
    // The box itself needs 12 quiet hours, which a seeded thread doesn't
    // have, so `shows` is the reply and the routes compare the recap.
    name: 'a thread the librarian has written a recap for',
    shows: ['Parked until Nipunn replies.'],
    play: (p) => {
      p.say('Parked until Nipunn replies.');
      p.endTurn({ needs: 'nothing' });
      p.librarianWrites('- Asked Nipunn how to go ahead\n- Nothing goes out until he replies', 'A community port of the clients, and what to say about it.');
    },
  },
  {
    name: 'a reply with no card',
    shows: ['Just an answer.'],
    play: (p) => {
      p.say('Just an answer.');
      p.endTurn({ needs: 'nothing' });
    },
  },
  {
    name: 'Mike sends two pictures placed in his words',
    shows: ['should look like', 'They differ in colour.'],
    play: (p) => {
      p.say('Ready.');
      p.endTurn({ needs: 'nothing' });
      const dir = tmpDir('tb-scenario-');
      const pics = [1, 2].map((i) => {
        const file = path.join(dir, `p${i}.png`);
        fs.writeFileSync(file, makePng(4, 4));
        return { path: file, mediaType: 'image/png' };
      });
      p.send('this [Picture 1] should look like [Picture 2]', pics);
      p.say('They differ in colour.');
      p.endTurn({ needs: 'nothing' });
    },
  },
];
