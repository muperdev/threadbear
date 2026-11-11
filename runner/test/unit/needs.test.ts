import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { PersistedEvent } from '../../src/protocol.ts';
import { runCommandProblem, offersToTakeItBack, TRY_IT_FIRST, archiveFromTool, snoozeFromTool, bylineFromTool, endedThisTurn, CODEX_END_TURN_DESCRIPTION, CODEX_ONE_REPLY_DESCRIPTION, END_TURN_DESCRIPTION, END_TURN_FULL_NAME, endTurnDescription, needsFromText, needsFromTool, ONE_REPLY_DESCRIPTION, replyWithoutRepeat, splitReplySection, summaryFromTool, summaryMarkdown, turnReply } from '../../src/needs.ts';
import { END_TURN_TOOL } from '../../src/needs.ts';
import { SERVER_NAME } from '../../src/threadbear-mcp.ts';
import { ClaudeNormalizer, fileContext } from '../../src/adapters/claude/normalize.ts';

test('end_turn input: nothing, you with asks, and junk falls back', () => {
  assert.deepEqual(needsFromTool({ needs: 'nothing', asks: [{ action: 'ignored' }] }), { needs: 'nothing' });
  assert.deepEqual(
    needsFromTool({ needs: 'you', asks: [{ action: ' Paste the key ', where: 'Settings › Models', reply: 'done' }, { action: '' }, 'x', { action: 'Pick a budget', where: '' }] }),
    { needs: 'you', asks: [{ action: 'Paste the key', where: 'Settings › Models', reply: 'done' }, { action: 'Pick a budget' }] },
  );
  assert.deepEqual(needsFromTool({ needs: 'you' }), { needs: 'you' });
  assert.equal(needsFromTool({ needs: 'maybe' }), undefined);
  // archive only counts with needs "nothing": a thread that asks for something stays in the inbox.
  assert.equal(archiveFromTool({ needs: 'nothing', archive: true }), true);
  assert.equal(archiveFromTool({ needs: 'you', archive: true }), false);
  assert.equal(archiveFromTool({ needs: 'nothing', archive: 'yes' }), false);
  assert.equal(archiveFromTool({ needs: 'nothing' }), false);
  assert.equal(archiveFromTool(undefined), false);
  assert.equal(snoozeFromTool({ needs: 'nothing', snooze: ' 2h ' }), '2h');
  assert.equal(snoozeFromTool({ needs: 'you', snooze: '2h' }), undefined);
  assert.equal(snoozeFromTool({ needs: 'nothing', snooze: 'off' }), undefined);
  assert.equal(snoozeFromTool({ needs: 'nothing', snooze: '' }), undefined);
  assert.equal(snoozeFromTool({ snooze: 'when the phone arrives' }), 'when the phone arrives');
  assert.equal(needsFromTool(null), undefined);
  assert.equal(needsFromTool({ needs: 'you', asks: Array.from({ length: 20 }, (_, i) => ({ action: `a${i}` })) })!.asks!.length, 10);
});

test('end_turn input: next steps go with needs nothing only', () => {
  // Mike, 30 Oct: follow-ups in the prose got lost under a done card.
  assert.deepEqual(needsFromTool({ needs: 'nothing', next: [' Show Bruce pictures ', '', 7, 'Show Bruce pictures', 'Start Bruce threads'] }), {
    needs: 'nothing',
    next: ['Show Bruce pictures', 'Start Bruce threads'],
  });
  assert.deepEqual(needsFromTool({ summary: ['Done.'], next: ['Do the next bit'] }), { needs: 'nothing', next: ['Do the next bit'] });
  assert.equal(needsFromTool({ needs: 'nothing', next: ['a', 'b', 'c', 'd'] })!.next!.length, 3);
  assert.equal(needsFromTool({ needs: 'nothing', next: ['x'.repeat(500)] })!.next![0].length, 120);
  assert.deepEqual(needsFromTool({ needs: 'nothing', next: 'not a list' }), { needs: 'nothing' });
  // A card of asks has its own buttons.
  assert.deepEqual(needsFromTool({ needs: 'you', asks: [{ action: 'Pick one' }], next: ['Later'] }), { needs: 'you', asks: [{ action: 'Pick one' }] });
});

test('end_turn input: needs left out comes from the asks', () => {
  // Agents filling in a summary forgot needs 2 times in 9 (29 Oct 2026).
  assert.deepEqual(needsFromTool({ asks: [{ action: 'Paste the key', why: 'For the deploy.' }] }), { needs: 'you', asks: [{ action: 'Paste the key', why: 'For the deploy.' }] });
  assert.deepEqual(needsFromTool({ summary: ['Fixed it.'] }), { needs: 'nothing' });
  assert.deepEqual(needsFromTool({ asks: [{ action: '' }] }), { needs: 'nothing' });
  assert.deepEqual(needsFromTool({}), { needs: 'nothing' });
  assert.equal(archiveFromTool({ archive: true }), true);
  assert.equal(archiveFromTool({ archive: true, asks: [{ action: 'Look' }] }), false);
});

test('end_turn summary: bullets kept tidy, a few at most, none is undefined', () => {
  assert.deepEqual(summaryFromTool({ summary: ['- Fixed the slug bug.', ' • [Tests pass](work:npm test) ', '', 3, '* Committed as `b08255b`.'] }), ['Fixed the slug bug.', '[Tests pass](work:npm test)', 'Committed as `b08255b`.']);
  assert.equal(summaryFromTool({ summary: [] }), undefined);
  assert.equal(summaryFromTool({ summary: 'Fixed it.' }), undefined);
  assert.equal(summaryFromTool({ needs: 'nothing' }), undefined);
  assert.equal(summaryFromTool({ summary: Array.from({ length: 12 }, (_, i) => `b${i}`) })!.length, 8);
  assert.equal(summaryFromTool({ summary: ['x'.repeat(600)] })![0].length, 400);
  assert.equal(summaryMarkdown(['One.', 'Two.']), '- One.\n- Two.');
});

test("end_turn's description asks for a short summary with links into the thread", () => {
  for (const d of [END_TURN_DESCRIPTION, CODEX_END_TURN_DESCRIPTION]) {
    assert.match(d, /summary/);
    assert.match(d, /work:/);
    assert.match(d, /thread:/);
  }
});

test('end_turn input: a decision keeps its why, options and a valid pick', () => {
  assert.deepEqual(
    needsFromTool({ needs: 'you', asks: [{ action: 'Merge #52?', why: ' Preview is green. ', options: ['Merge', ' ', 'Test locally', 3], suggested: 1 }] }),
    { needs: 'you', asks: [{ action: 'Merge #52?', why: 'Preview is green.', options: ['Merge', 'Test locally'], suggested: 1 }] },
  );
  // A pick that isn't one of the options is dropped, and so is one with no options.
  assert.deepEqual(needsFromTool({ needs: 'you', asks: [{ action: 'a', options: ['x'], suggested: 4 }] })!.asks, [{ action: 'a', options: ['x'] }]);
  assert.deepEqual(needsFromTool({ needs: 'you', asks: [{ action: 'a', options: [], suggested: 0 }] })!.asks, [{ action: 'a' }]);
  assert.equal(needsFromTool({ needs: 'you', asks: [{ action: 'a', options: Array.from({ length: 9 }, (_, i) => `o${i}`) }] })!.asks![0].options!.length, 6);
});

test('end_turn input: links to go and do the thing, web or local, a few at most', () => {
  const asks = needsFromTool({
    needs: 'you',
    asks: [{
      action: 'Log in to Hugging Face',
      links: [
        { title: ' Open login page ', url: 'https://huggingface.co/login' },
        { title: 'Notes', url: '~/dev/notes.md' },
        { title: 'Spec', url: '/Users/m/spec.md' },
        { title: 'Bad scheme', url: 'javascript:alert(1)' },
        { title: '', url: 'https://example.com' },
        { title: 'No url' },
        'junk',
        { title: 'Fourth good one', url: 'http://localhost:3000' },
      ],
    }],
  })!.asks!;
  assert.deepEqual(asks[0].links, [
    { title: 'Open login page', url: 'https://huggingface.co/login' },
    { title: 'Notes', url: '~/dev/notes.md' },
    { title: 'Spec', url: '/Users/m/spec.md' },
  ]);
  assert.equal(needsFromTool({ needs: 'you', asks: [{ action: 'a', links: [] }] })!.asks![0].links, undefined);
});

test('end_turn input: a link can open one of the thread\'s reviews', () => {
  const asks = needsFromTool({
    needs: 'you',
    asks: [{
      action: 'Pick a banner',
      links: [
        { title: 'Compare banners', url: 'review:5f1c2a90-1b2c-4d5e-8f90-0a1b2c3d4e5f' },
        { title: 'Empty', url: 'review:' },
        { title: 'Odd', url: 'review:<script>' },
      ],
    }],
  })!.asks!;
  assert.deepEqual(asks[0].links, [{ title: 'Compare banners', url: 'review:5f1c2a90-1b2c-4d5e-8f90-0a1b2c3d4e5f' }]);
});

test('the section: a bold heading then a numbered list, with a reply phrase', () => {
  const text = [
    'The Settings window is in.',
    '',
    '**What I need from you**',
    '',
    '1. Open Settings > Models, paste your OpenRouter key, then reply "key set".',
    '   It only needs a small credit limit.',
    '',
    '2. Say whether **$1 a day** is right.',
    '',
    "Meanwhile I'll keep going on the app side.",
  ].join('\n');
  assert.deepEqual(needsFromText(text), {
    needs: 'you',
    asks: [
      { action: 'Open Settings > Models, paste your OpenRouter key, then reply "key set". It only needs a small credit limit.', reply: 'key set' },
      { action: 'Say whether $1 a day is right.' },
    ],
  });
});

test('the section: "nothing needed" under the heading, on its line, or with no heading', () => {
  assert.deepEqual(needsFromText('Done.\n\n**What I need from you**\n\nNothing needed from you right now. It is finished.'), { needs: 'nothing' });
  assert.deepEqual(needsFromText('Done.\n\n**What I need from you:** nothing right now. Still building.'), { needs: 'nothing' });
  assert.deepEqual(needsFromText('Done.\n\nNothing needed from you right now, the work is done. Next step: archive it.'), { needs: 'nothing' });
  assert.deepEqual(needsFromText('I wrote the file. The task is finished, and nothing is needed from you right now.'), { needs: 'nothing' });
});

test('the section: follow-ups listed after "nothing needed" are the done card\'s next', () => {
  // Thread 13096d04's last message (30 Oct 2026), which skipped end_turn.
  const text = [
    "Meeting Archive is running again on your Mac. I've checked it myself: it's up.",
    '',
    'So everything from this thread is now live:',
    '- **On your Mac:** the app records calls again.',
    '- **On Bruce:** new calls get their speakers labelled by Nemotron 3 Diarization.',
    '',
    'Nothing needed from you right now; the work is finished. Two follow-ups are worth doing:',
    "- **Check your next call:** confirm Bruce actually used Nemotron on it. Its background service is the one setup I couldn't test before deploying.",
    '- **Try a bigger Whisper model:** the transcribed words are now the weaker part of the notes.',
  ].join('\n');
  assert.deepEqual(needsFromText(text), {
    needs: 'nothing',
    next: ['Check your next call: confirm Bruce actually used Nemotron on it', 'Try a bigger Whisper model: the transcribed words are now the weaker part of the notes'],
  });
  // A line of lead-in, a cap of three, and long ones cut at a word.
  const many = ['Nothing needed from you right now.', '', 'Worth doing next:', '', '1. One', '2. Two', '3. Three', '4. Four'].join('\n');
  assert.deepEqual(needsFromText(many), { needs: 'nothing', next: ['One', 'Two', 'Three'] });
  const long = needsFromText(`Nothing needed from you right now.\n- ${'word '.repeat(40)}`)!;
  assert.ok(long.next![0].length <= 120 && long.next![0].endsWith('word'));
  // Prose after it, or a list before it, is no follow-up.
  assert.deepEqual(needsFromText('Nothing needed from you right now. If you want, I can compare them.'), { needs: 'nothing' });
  assert.deepEqual(needsFromText('Done:\n- a\n- b\n\nNothing needed from you right now.'), { needs: 'nothing' });
  assert.deepEqual(needsFromText('Nothing needed from you right now.\n\nThe tests pass and it is on main.\nMore prose.\n- not a follow-up'), { needs: 'nothing' });
  // Under the heading too: "nothing" and then a list is follow-ups, not asks.
  assert.deepEqual(needsFromText('**What I need from you**\n\nNothing right now. Worth doing next:\n- Try the bigger model.'), { needs: 'nothing', next: ['Try the bigger model'] });
});

test('the section: the last heading wins, a mention in a sentence does not count', () => {
  const text = [
    'Agents end with a **What I need from you** section, which we could parse.',
    '',
    '**What I need from you**',
    '',
    '- Reply "go" to build it.',
  ].join('\n');
  assert.deepEqual(needsFromText(text), { needs: 'you', asks: [{ action: 'Reply "go" to build it.', reply: 'go' }] });
  assert.equal(needsFromText('It reads the "What I need from you" heading. I will report back.'), undefined);
  assert.equal(needsFromText('All done here.'), undefined);
});

test('the section: a # heading, and prose under it with no list is one ask', () => {
  assert.deepEqual(needsFromText('## What I need from you\n\n1. Say yes or no.'), { needs: 'you', asks: [{ action: 'Say yes or no.' }] });
  assert.deepEqual(needsFromText('**What I need from you**\n\nRestart the app when you get a minute.'), { needs: 'you', asks: [{ action: 'Restart the app when you get a minute.' }] });
});

test('the section: choices under an ask stay with it, and a choice is not a reply', () => {
  const text = [
    '**What I need from you**',
    '',
    '1. Pick a fix and reply with it:',
    '   - "fix it": I make the tests work.',
    '   - "leave it": nothing changes.',
    '2. Decide on the filter. Reply "filter" or "wait".',
  ].join('\n');
  assert.deepEqual(needsFromText(text), {
    needs: 'you',
    asks: [
      { action: 'Pick a fix and reply with it: "fix it": I make the tests work. "leave it": nothing changes.' },
      { action: 'Decide on the filter. Reply "filter" or "wait".', options: ['filter', 'wait'] },
    ],
  });
});

// Fable skipped end_turn and wrote this; the card had one button (28 Oct 2026).
test('the section: every quoted answer in a list is a choice', () => {
  const ask = (line: string) => needsFromText(`**What I need from you**\n\n1. ${line}`)!.asks![0];
  assert.deepEqual(
    ask('Say whether to keep "Browser" or rename the row to "Website". Reply with "Land it", "Website, then land it", or what else to change.').options,
    ['Land it', 'Website, then land it'],
  );
  assert.deepEqual(ask('Reply "merge", "wait" or "close".').options, ['merge', 'wait', 'close']);
  assert.deepEqual(ask('Reply “yes” / “no”.').options, ['yes', 'no']);
  // A quote that isn't straight after the reply word isn't one of them.
  assert.deepEqual(ask('Reply "go" when the "Export" step is done.'), { action: 'Reply "go" when the "Export" step is done.', reply: 'go' });
});

test('the section: a code block in an ask stays in it', () => {
  const text = '**What I need from you**\n\n1. Run this:\n```\n- not an item\n```\nThen reply "done".';
  assert.deepEqual(needsFromText(text), { needs: 'you', asks: [{ action: 'Run this: ``` - not an item ``` Then reply "done".', reply: 'done' }] });
});

// ------------------------------------------------------------ turnReply

// Events as the thread's log has them, seq in order, with only the fields
// the check reads.
function log(...items: (Record<string, unknown> & { type: string })[]): PersistedEvent[] {
  return items.map((e, i) => ({ seq: i + 1, ts: '2026-10-28T06:10:00Z', threadId: 't', ...e }) as unknown as PersistedEvent);
}
const END = END_TURN_FULL_NAME;
const user = (text = 'hi') => ({ type: 'user.message', text, images: [] });
const started = { type: 'turn.started', turnId: 'turn-1' };
const text = (text: string, itemId = 'm1') => ({ type: 'assistant.text', itemId, text });
const thought = (itemId = 'r1') => ({ type: 'reasoning', itemId, text: 'Custom firmware would not help much.' });
const call = (itemId: string, name: string) => ({ type: 'tool.call', itemId, name, kind: 'mcp', title: name, input: {} });
const result = (itemId: string) => ({ type: 'tool.result', itemId, ok: true, text: '', truncated: false, images: [] });

test('turnReply: the full tool name is the one the server registers', () => {
  assert.equal(END, `mcp__${SERVER_NAME}__${END_TURN_TOOL}`);
});

test('turnReply: a message, then end_turn, is a reply; thinking and tool calls alone are not', () => {
  assert.deepEqual(turnReply(log(user(), started, thought(), text('No, the camera is not where the delay is.'), call('e1', END))), { landed: true, replied: true });
  // What happened on 28 Oct: the answer drafted in thinking, two commands, then end_turn.
  assert.deepEqual(turnReply(log(user(), started, thought(), call('b1', 'Bash'), result('b1'), call('b2', 'Bash'), result('b2'), thought('r2'), call('e1', END))), { landed: true, replied: false });
  // A progress line, then more work, then end_turn: the line isn't the answer.
  assert.deepEqual(turnReply(log(user(), started, text('Found it, checking one thing.'), call('s1', 'mcp__agent-archive__search_archive'), result('s1'), call('e1', END))), { landed: true, replied: false });
  // Nothing at all.
  assert.deepEqual(turnReply(log(user(), started, call('e1', END))), { landed: true, replied: false });
});

test("turnReply: the thread's own tools after the message don't count as work, nor does a refused end_turn", () => {
  const own = `mcp__${SERVER_NAME}__pin`;
  assert.deepEqual(turnReply(log(user(), started, text('Done, pinned.'), call('p1', own), result('p1'), call('e1', END))), { landed: true, replied: true });
  // Loading end_turn's deferred schema isn't work either (a Haiku thread was refused for it, 28 Oct).
  assert.deepEqual(turnReply(log(user(), started, text('Hello.'), call('ts', 'ToolSearch'), result('ts'), call('e1', END))), { landed: true, replied: true });
  // Refused once, the reply written, called again.
  assert.deepEqual(turnReply(log(user(), started, call('b1', 'Bash'), result('b1'), call('e1', END), result('e1'), text('All green.'), call('e2', END))), { landed: true, replied: true });
  // A tool still running (no result) isn't work done after the message.
  assert.deepEqual(turnReply(log(user(), started, call('b1', 'Bash'), text('Started the build; back when it finishes.'), call('e1', END))), { landed: true, replied: true });
});

test('turnReply: only the current turn counts, and a message from Mike mid-turn wants a reply of its own', () => {
  // Last turn's answer doesn't carry over.
  assert.deepEqual(turnReply(log(user(), { type: 'turn.started', turnId: 'turn-0' }, text('Earlier answer.'), { type: 'turn.completed', turnId: 'turn-0', outcome: 'completed', durationMs: 1 }, user('again'), started, call('e1', END))), { landed: true, replied: false });
  assert.deepEqual(turnReply(log(user(), started, text('Working on it.'), user('also do X'), call('e1', END))), { landed: true, replied: false });
  assert.deepEqual(turnReply(log(user(), started, text('Working on it.'), user('also do X'), text('Did X too.', 'm2'), call('e1', END))), { landed: true, replied: true });
});

test('endedThisTurn: a message after the taken end_turn opens it again, even one sent while the call ran', () => {
  const noted = { ...result('e1'), text: 'Noted: this thread is done.' } as never;
  assert.equal(endedThisTurn(log(user(), started, call('e1', END), noted)), true);
  assert.equal(endedThisTurn(log(user(), started, call('e1', END), noted, user('Archive'))), false);
  // Sent before the result came back: the agent called it without having seen the message.
  assert.equal(endedThisTurn(log(user(), started, call('e1', END), user('Archive'), noted)), false);
  // And one taken after the message ends it again.
  assert.equal(endedThisTurn(log(user(), started, call('e1', END), noted, user('Archive'), call('e2', END), { ...result('e2'), text: 'Noted: this thread is done.' } as never)), true);
});

test("turnReply: not landed until the call is in the events; a Codex child's items are ignored", () => {
  assert.deepEqual(turnReply(log(user(), started, text('Done.'))), { landed: false, replied: true });
  assert.deepEqual(turnReply(log(user(), started, call('b1', 'Bash'), result('b1'))), { landed: false, replied: false });
  // The child's text and its own end_turn are not the thread's.
  assert.deepEqual(turnReply(log(user(), started, text('Done.'), call('child/b1', 'Bash'), result('child/b1'), call('child/e1', END), call('e1', END))), { landed: true, replied: true });
});

// The 29 Oct refusals (threads 1edccf6f and ec75fe8b): the reply came as a
// thinking block signed as narration, straight before end_turn, after a
// create_thread result. Run the CLI's frames through the Claude normaliser
// as the runner does: that reply counts, and a plain thought still doesn't.
test('turnReply: a reply the CLI sent as a narration block counts; a thought before end_turn still does not', () => {
  const sig = (kind: string) => Buffer.concat([Buffer.from('0804128e110a11081318023801', 'hex'), Buffer.from([0x42, kind.length]), Buffer.from(kind)]).toString('base64');
  const turn = (kind: string) => {
    const n = new ClaudeNormalizer(fileContext('/work', '/nowhere/media', '/nowhere/blobs'));
    n.sent();
    const frame = (id: string, content: unknown[]) => ({ type: 'assistant', message: { id, content }, parent_tool_use_id: null });
    const evs = [
      frame('m1', [{ type: 'tool_use', id: 'c1', name: 'mcp__threadbear__create_thread', input: {} }]),
      { type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'c1', content: 'Started thread 3fee4428' }] }, parent_tool_use_id: null },
      frame('m2', [{ type: 'thinking', thinking: 'Wrap up.', signature: sig('thinking') }]),
      frame('m2', [{ type: 'thinking', thinking: "I've started a thread to fix the refusals.", signature: sig(kind) }]),
      frame('m2', [{ type: 'tool_use', id: 'e1', name: END, input: { needs: 'nothing' } }]),
    ]
      .flatMap((m) => n.handle(m))
      .flatMap((o) => (o.kind === 'event' ? [o.event as Record<string, unknown> & { type: string }] : []));
    return turnReply(log(user(), started, ...evs));
  };
  // Its text is only the API's summary of what the agent wrote.
  assert.deepEqual(turn('narration'), { landed: true, replied: true, narrationOnly: true });
  assert.deepEqual(turn('thinking'), { landed: true, replied: false });
});

test('turnReply: a reply in words after a narration block is not narration-only; one before work does not count', () => {
  const narrated = (itemId: string) => ({ type: 'assistant.text', itemId, text: 'I sent the steps.', narration: true });
  assert.deepEqual(turnReply(log(user(), started, narrated('a'), call('e1', END))), { landed: true, replied: true, narrationOnly: true });
  assert.deepEqual(turnReply(log(user(), started, narrated('a'), text('Step 1: ...', 'b'), call('e1', END))), { landed: true, replied: true });
  assert.deepEqual(turnReply(log(user(), started, text('Step 1: ...', 'b'), narrated('a'), call('e1', END))), { landed: true, replied: true });
  // Words before work, then only narration since: narration-only.
  assert.deepEqual(turnReply(log(user(), started, text('Checking.', 'b'), call('b1', 'Bash'), result('b1'), narrated('a'), call('e1', END))), { landed: true, replied: true, narrationOnly: true });
  // A status line, then Threadbear's own read tools, then the answer as
  // narration (29 Oct 2026): the status line isn't the reply.
  const read = 'mcp__threadbear__read_thread';
  assert.deepEqual(turnReply(log(user(), started, text('Looking into it.', 'b'), call('r1', read), result('r1'), narrated('a'), call('e1', END))), { landed: true, replied: true, narrationOnly: true });
  // The same with ToolSearch loading end_turn between them.
  assert.deepEqual(turnReply(log(user(), started, text("I'll lay out three designs.", 'b'), call('t1', 'ToolSearch'), result('t1'), narrated('a'), call('e1', END))), { landed: true, replied: true, narrationOnly: true });
  // Words, then a pin, then end_turn: the words are the reply.
  assert.deepEqual(turnReply(log(user(), started, text('Step 1: ...', 'b'), call('p1', 'mcp__threadbear__pin'), result('p1'), call('e1', END))), { landed: true, replied: true });
});

test("end_turn's description asks for the words naming a link's target to be that link", () => {
  // Mike, 29 Oct: plain "the screenshots in Highlights" over a button for the same screenshot.
  for (const d of [END_TURN_DESCRIPTION, CODEX_END_TURN_DESCRIPTION]) assert.match(d, /make those words a link to the same url/);
});

test('with bullet replies off, end_turn asks for a normal message first', () => {
  assert.equal(endTurnDescription('claude', true), END_TURN_DESCRIPTION);
  assert.equal(endTurnDescription('codex', true), CODEX_END_TURN_DESCRIPTION);
  assert.match(endTurnDescription('claude', false), /Write your reply as an ordinary message/);
  assert.match(endTurnDescription('claude', false), /refused until the turn has that message/);
  assert.match(endTurnDescription('codex', false), /Write your reply as your final answer/);
  for (const provider of ['claude', 'codex'] as const) {
    assert.doesNotMatch(endTurnDescription(provider, false), /the bullets are the reply|one short line/);
    assert.match(endTurnDescription(provider, false), /make those words a link to the same url/);
  }
});

test('a byline is one short line, or nothing', () => {
  assert.equal(bylineFromTool({ byline: ' network blip,\n sorted ' }), 'network blip, sorted');
  assert.equal(bylineFromTool({ byline: '   ' }), undefined);
  assert.equal(bylineFromTool({ byline: 3 }), undefined);
  assert.equal(bylineFromTool(undefined), undefined);
  assert.equal(bylineFromTool({ byline: 'x'.repeat(200) })?.length, 80);
});

test('the card text is cut at a word with an ellipsis, never mid-word', () => {
  const why = 'Or step away from the keyboard and say "go", and I\'ll do the picker myself, though you might see it move. '.repeat(8);
  const ask = needsFromTool({ asks: [{ action: 'Add the runner to Full Disk Access.', why }] })?.asks?.[0];
  assert.ok(ask?.why && ask.why.length <= 500);
  assert.match(ask!.why!, /\w…$/);
  assert.doesNotMatch(ask!.why!, / \w…$/, 'not a single letter before the ellipsis');
});

test('one reply: its description says the reply comes after end_turn, and keeps the card out of it', () => {
  assert.equal(endTurnDescription('claude', true, true), ONE_REPLY_DESCRIPTION);
  assert.equal(endTurnDescription('codex', false, true), CODEX_ONE_REPLY_DESCRIPTION);
  assert.match(ONE_REPLY_DESCRIPTION, /write your reply as ordinary text straight after it/);
  assert.match(ONE_REPLY_DESCRIPTION, /Never write a "What I need from you" section/);
  assert.match(CODEX_ONE_REPLY_DESCRIPTION, /your final answer/);
  // Far shorter than the bullets' one.
  assert.ok(ONE_REPLY_DESCRIPTION.length < END_TURN_DESCRIPTION.length * 0.6);
});

test('one reply: a closing section comes off the reply, bold, # or a bare line', () => {
  const bare = splitReplySection("Fixed it.\n\nWhat I need from you:\n1. Tell me whether to deploy.\n2. Check the logs.");
  assert.equal(bare.reply, 'Fixed it.');
  assert.equal(bare.needs?.needs, 'you');
  assert.equal(bare.needs?.asks?.length, 2);
  const heading = splitReplySection('Fixed it.\n\n## What I need from you\n1. Run it with your key.');
  assert.equal(heading.reply, 'Fixed it.');
  assert.equal(heading.needs?.asks?.[0].action, 'Run it with your key.');
  const nothing = splitReplySection('Fixed and committed.\n\nNothing needed from you right now.\n\n- Check it on production tomorrow');
  assert.equal(nothing.reply, 'Fixed and committed.');
  assert.deepEqual(nothing.needs, { needs: 'nothing', next: ['Check it on production tomorrow'] });
  // A mention in a sentence isn't a section, and a reply with none is kept whole.
  const none = splitReplySection('The script says nothing needed from you is wrong, so I fixed it.');
  assert.equal(none.needs, undefined);
  assert.equal(none.reply, 'The script says nothing needed from you is wrong, so I fixed it.');
});

// Thread 7ac017b7 (3 Nov 2026): six of eight replies under a card went on to
// write the card out again, and that looked like not using the card at all.
test('one reply: a section repeating the card comes off the reply, and one saying more stays', () => {
  const card = { needs: 'you' as const, asks: [{ action: 'Press Bring Them In.' }] };
  const repeat = 'It is ready.\n\n**What I need from you**\n1. On Tinker Desk, press Bring Them In, then tell me.';
  assert.equal(replyWithoutRepeat(repeat, card), 'It is ready.');
  // What comes after the list isn't on the card, so it stays.
  assert.equal(replyWithoutRepeat(`${repeat}\n  Say how it went.\n\nOne follow-up: try a clash next.`, card), 'It is ready.\n\nOne follow-up: try a clash next.');
  // Two things asked and one on the card: the second is only in the reply.
  const more = 'It is ready.\n\n**What I need from you**\n1. Press Bring Them In.\n2. Send me the log.';
  assert.equal(replyWithoutRepeat(more, card), more);
  // Saying nothing is needed under a card isn't a repeat of it.
  const nothing = 'It is ready. Nothing needed from you right now.';
  assert.equal(replyWithoutRepeat(nothing, card), nothing);
  // No section, and no asks on the card: left alone.
  assert.equal(replyWithoutRepeat('It is ready.', card), 'It is ready.');
  assert.equal(replyWithoutRepeat(repeat, { needs: 'nothing', next: ['Try it'] }), repeat);
});

// Mike pressed ? on rows in the agent's shorthand, with no reason given
// (~/dev/me/misc/end-turn-eval/next-rows). Every form of end_turn has to say
// how to write one, as agents are on all of them.
test('every form of end_turn says how to write a follow-up row', () => {
  for (const provider of ['claude', 'codex'] as const) {
    for (const [bullets, oneReply] of [[true, false], [false, false], [false, true]] as const) {
      const d = endTurnDescription(provider, bullets, oneReply);
      assert.match(d, /with this thread forgotten/);
      assert.match(d, /No names from the code or your working/);
      assert.match(d, /with the reason in the same line/);
      assert.doesNotMatch(d, /about 8 words/);
    }
  }
});

test('every form of end_turn says to try a step first', () => {
  for (const provider of ['claude', 'codex'] as const) {
    for (const [bullets, oneReply] of [[true, false], [false, false], [true, true]]) {
      const d = endTurnDescription(provider, bullets, oneReply);
      assert.ok(d.includes(TRY_IT_FIRST), `${provider} ${bullets} ${oneReply}`);
      assert.doesNotMatch(d, /when you could plausibly do it yourself/);
    }
  }
});

test('offersToTakeItBack knows the answers real cards used, and leaves others alone', () => {
  const offers = (o: string) => offersToTakeItBack([{ action: 'x', options: [o] }]);
  for (const o of ['Can you do it?', 'Can you do it in Chrome?', 'Connected, you do it', 'Do it for me later', 'Can you do the copies?', 'Try it yourself']) assert.ok(offers(o), o);
  for (const o of ['Done', 'Signed in', 'Deploy it', "I can't do it", 'Not yet', 'Hold off']) assert.ok(!offers(o), o);
  assert.ok(!offersToTakeItBack([{ action: 'Can you do it?' }]));
});

test('a command on an ask has to be what it looks like', () => {
  const asks = (run: string) => needsFromTool({ needs: 'you', asks: [{ question: 'Run it?', run }] })?.asks;
  assert.equal(asks('npm test\n')?.[0].run?.command, 'npm test');
  assert.equal(asks('a\n\tb')?.[0].run?.command, 'a\n\tb');
  for (const bad of ['echo\u001b[201~', 'rm​ x', 'echo ‮oc', 'a b', ' ', 'x'.repeat(8001)]) {
    assert.ok(runCommandProblem(bad), JSON.stringify(bad));
    assert.equal(asks(bad)?.[0].run, undefined);
  }
});
