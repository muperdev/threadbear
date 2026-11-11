// A team's hub (docs/multiplayer-design.md): the deployment of the member
// who made the team holds its members, shared projects, log, board and the
// messages and handovers between members' agents. Each member's runner
// calls these with its member key; the key's sha256 is all that's stored.
//
// Nothing here reads or writes the tables sync.ts, phone.ts or relay.ts
// keep, and a member key isn't a runner key or a phone token (or the other
// way round): being on someone's team never reaches their threads. Only
// team:create takes the owner key, to make the team on the host's own
// deployment.
import { v, ConvexError } from 'convex/values';
import { mutation, query, type MutationCtx, type QueryCtx } from './_generated/server';
import { sha256 } from './lib';
import { isOwnerKey } from './runnerAccess';
import { ANSWERED_KEPT_MS, CARDS_MAX, CARD_STATUSES, DONE_KEPT_MS, INVITE_DAYS_MAX, LOG_MAX, LOG_SHOWN, MEMBERS_MAX, MEMBER_NAME_MAX, NOTES_MAX, PROJECTS_MAX, SEEN_EVERY_MS, SHORT_MAX, TEAM_NAME_MAX, TEXT_MAX, TITLE_MAX, WAITING_MAX, checkLinks, checkRepos, checkText, cleanLine, mayAnswer, sameName, validHash, validId } from './teamAccess';

const link = v.object({ title: v.string(), url: v.string() });

function fail(kind: string, message: string): ConvexError<{ kind: string; message: string }> {
  return new ConvexError({ kind, message });
}

// A rule from teamAccess.ts broken: its message, as badArgs.
function checked<T>(f: () => T): T {
  try {
    return f();
  } catch (e) {
    if (e instanceof ConvexError) throw e;
    throw fail('badArgs', e instanceof Error ? e.message : String(e));
  }
}

async function teamRow(ctx: QueryCtx | MutationCtx) {
  return ctx.db.query('teamInfo').first();
}

// The member whose key this is, or an auth error.
async function memberOf(ctx: QueryCtx | MutationCtx, key: string) {
  if (typeof key !== 'string' || key.length < 20) throw fail('auth', 'not a member key');
  const hash = await sha256(key);
  const row = await ctx.db
    .query('teamMembers')
    .withIndex('by_hash', (q) => q.eq('keyHash', hash))
    .first();
  if (!row || row.revokedAt) throw fail('auth', 'not a member of this team (the key is unknown, or they left or were removed)');
  return row;
}

async function liveMembers(ctx: QueryCtx | MutationCtx) {
  return (await ctx.db.query('teamMembers').collect()).filter((m) => !m.revokedAt);
}

async function projectRow(ctx: QueryCtx | MutationCtx, id: string) {
  const p = await ctx.db
    .query('teamProjects')
    .withIndex('by_projectId', (q) => q.eq('id', id))
    .first();
  if (!p || p.removedAt) throw fail('notFound', `no shared project ${id}`);
  return p;
}

function memberView(m: { memberId: string; name: string; host: boolean; joinedAt: number; seenAt?: number }) {
  return { memberId: m.memberId, name: m.name, host: m.host, joinedAt: m.joinedAt, ...(m.seenAt ? { seenAt: m.seenAt } : {}) };
}

// ------------------------------------------------------------------ joining

// Makes the team on this deployment, with its owner key: the host's runner,
// which becomes the first member.
export const create = mutation({
  args: { ownerKey: v.string(), name: v.string(), memberId: v.string(), memberName: v.string(), keyHash: v.string() },
  handler: async (ctx, a) => {
    if (!isOwnerKey(a.ownerKey, [process.env.RUNNER_KEY, process.env.RUNNER_KEY_NEXT])) throw fail('auth', "only this deployment's own runner (its owner key) can make a team on it");
    if (await teamRow(ctx)) throw fail('exists', 'this deployment already hosts a team');
    const name = checked(() => cleanLine('The team name', a.name, TEAM_NAME_MAX));
    const memberName = checked(() => cleanLine('Your name', a.memberName, MEMBER_NAME_MAX));
    if (!validId(a.memberId) || !validHash(a.keyHash)) throw fail('badArgs', 'bad member id or key');
    const now = Date.now();
    await ctx.db.insert('teamInfo', { name, createdAt: now });
    await ctx.db.insert('teamMembers', { memberId: a.memberId, name: memberName, keyHash: a.keyHash, host: true, joinedAt: now, seenAt: now });
    return { name, memberId: a.memberId };
  },
});

// An invite code for one join, made on the host's Mac (only its hash comes here).
export const invite = mutation({
  args: { key: v.string(), codeHash: v.string(), days: v.optional(v.number()) },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    if (!me.host) throw fail('forbidden', 'only the person hosting the team can invite');
    if (!validHash(a.codeHash)) throw fail('badArgs', 'bad invite code');
    const days = Math.min(Math.max(a.days ?? 3, 1 / 24), INVITE_DAYS_MAX);
    const now = Date.now();
    // Spent and expired invites go as new ones are made.
    for (const old of await ctx.db.query('teamInvites').collect()) if (old.usedAt || old.expiresAt < now) await ctx.db.delete(old._id);
    await ctx.db.insert('teamInvites', { codeHash: a.codeHash, createdAt: now, expiresAt: now + days * 24 * 3600_000 });
    return { expiresAt: now + days * 24 * 3600_000 };
  },
});

// A new member, with an invite. Their runner made the key; only its hash is sent.
export const join = mutation({
  args: { codeHash: v.string(), memberId: v.string(), name: v.string(), keyHash: v.string() },
  handler: async (ctx, a) => {
    const team = await teamRow(ctx);
    if (!team) throw fail('notFound', 'this deployment hosts no team');
    if (!validHash(a.codeHash) || !validHash(a.keyHash) || !validId(a.memberId)) throw fail('badArgs', 'bad invite or key');
    const inv = await ctx.db
      .query('teamInvites')
      .withIndex('by_hash', (q) => q.eq('codeHash', a.codeHash))
      .first();
    const now = Date.now();
    if (!inv || inv.usedAt || inv.expiresAt < now) throw fail('auth', 'that invite is unknown, used or expired: ask for a new one');
    const name = checked(() => cleanLine('Your name', a.name, MEMBER_NAME_MAX));
    const members = await liveMembers(ctx);
    if (members.length >= MEMBERS_MAX) throw fail('full', `a team has at most ${MEMBERS_MAX} people`);
    if (members.some((m) => sameName(m.name, name))) throw fail('exists', `someone on the team is already called ${name}: join with another name`);
    if ((await ctx.db.query('teamMembers').withIndex('by_memberId', (q) => q.eq('memberId', a.memberId)).first()) !== null) throw fail('exists', 'that member id is taken');
    await ctx.db.insert('teamMembers', { memberId: a.memberId, name, keyHash: a.keyHash, host: false, joinedAt: now, seenAt: now });
    await ctx.db.patch(inv._id, { usedAt: now, usedBy: a.memberId });
    return { name: team.name, memberId: a.memberId, memberName: name };
  },
});

async function dropMember(ctx: MutationCtx, memberId: string, why: string) {
  const now = Date.now();
  for (const c of await ctx.db.query('teamCards').collect()) if (c.memberId === memberId) await ctx.db.delete(c._id);
  for (const m of await ctx.db
    .query('teamMessages')
    .withIndex('by_to', (q) => q.eq('to', memberId).eq('status', 'waiting'))
    .collect()) {
    await ctx.db.patch(m._id, { status: m.kind === 'handover' ? 'declined' : 'dismissed', answeredAt: now, answer: why });
  }
}

export const leave = mutation({
  args: { key: v.string() },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    if (me.host) throw fail('forbidden', "the host can't leave the team they host");
    await ctx.db.patch(me._id, { revokedAt: Date.now() });
    await dropMember(ctx, me.memberId, `${me.name} left the team`);
    return {};
  },
});

export const remove = mutation({
  args: { key: v.string(), memberId: v.string() },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    if (!me.host) throw fail('forbidden', 'only the host can remove someone');
    const them = await ctx.db
      .query('teamMembers')
      .withIndex('by_memberId', (q) => q.eq('memberId', a.memberId))
      .first();
    if (!them || them.revokedAt) throw fail('notFound', 'no such member');
    if (them.host) throw fail('forbidden', "the host can't be removed");
    await ctx.db.patch(them._id, { revokedAt: Date.now() });
    await dropMember(ctx, them.memberId, `${them.name} is no longer on the team`);
    return {};
  },
});

// The runner's "still here", for the dots in the members list.
export const heartbeat = mutation({
  args: { key: v.string() },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const now = Date.now();
    if (!me.seenAt || now - me.seenAt >= SEEN_EVERY_MS) await ctx.db.patch(me._id, { seenAt: now });
    return {};
  },
});

// ------------------------------------------------------------------ reading

// The team, its members and its shared projects with their recent log.
// Followed by each member's runner; changes when a project or member does.
export const state = query({
  args: { key: v.string() },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const team = await teamRow(ctx);
    const members = (await liveMembers(ctx)).map(memberView);
    const projects = [];
    for (const p of await ctx.db.query('teamProjects').collect()) {
      if (p.removedAt) continue;
      const log = await ctx.db
        .query('teamLog')
        .withIndex('by_project', (q) => q.eq('projectId', p.id))
        .order('desc')
        .take(LOG_SHOWN);
      const { _id, _creationTime, ...rest } = p;
      projects.push({ ...rest, log: log.reverse().map((l) => ({ memberId: l.memberId, text: l.text, at: l.at, ...(l.threadTitle ? { threadTitle: l.threadTitle } : {}) })) });
    }
    return { team: { name: team?.name ?? '' }, me: memberView(me), members, projects };
  },
});

// What changes often: the board, what's waiting for this member, and how
// their own messages were answered.
export const live = query({
  args: { key: v.string() },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const cards = (await ctx.db.query('teamCards').collect()).map(({ _id, _creationTime, ...c }) => c);
    const inbox = (
      await ctx.db
        .query('teamMessages')
        .withIndex('by_to', (q) => q.eq('to', me.memberId).eq('status', 'waiting'))
        .collect()
    ).map(({ _id, _creationTime, ...m }) => m);
    const answered = (
      await ctx.db
        .query('teamMessages')
        .withIndex('by_from', (q) => q.eq('from', me.memberId).eq('toldSender', undefined))
        .collect()
    )
      .filter((m) => m.status !== 'waiting')
      .map(({ _id, _creationTime, ...m }) => m);
    return { cards, inbox, answered };
  },
});

// ------------------------------------------------------------------ projects

const projectInput = v.object({
  id: v.string(),
  title: v.string(),
  goals: v.string(),
  state: v.string(),
  notes: v.string(),
  links: v.array(link),
  repos: v.array(v.string()),
});

export const shareProject = mutation({
  args: { key: v.string(), project: projectInput },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const p = a.project;
    if (!validId(p.id)) throw fail('badArgs', 'bad project id');
    const existing = await ctx.db
      .query('teamProjects')
      .withIndex('by_projectId', (q) => q.eq('id', p.id))
      .first();
    if (existing && !existing.removedAt) throw fail('exists', 'that project is already shared');
    if ((await ctx.db.query('teamProjects').collect()).filter((x) => !x.removedAt).length >= PROJECTS_MAX) throw fail('full', `a team shares at most ${PROJECTS_MAX} projects`);
    const now = Date.now();
    const row = checked(() => ({
      id: p.id,
      title: cleanLine('The title', p.title, TITLE_MAX),
      goals: checkText('goals', p.goals, SHORT_MAX),
      state: checkText('state', p.state, SHORT_MAX),
      notes: checkText('notes', p.notes, NOTES_MAX),
      links: checkLinks(p.links),
      repos: checkRepos(p.repos),
      sharedBy: me.memberId,
      createdAt: now,
      updatedAt: now,
      updatedBy: me.memberId,
    }));
    if (existing) await ctx.db.replace(existing._id, row);
    else await ctx.db.insert('teamProjects', row);
    return { id: p.id, updatedAt: now };
  },
});

// Any member may change a shared project; the last write wins, by this
// deployment's clock. addRepos adds to the repos rather than replacing them,
// as each member only knows their own folders' remotes.
export const updateProject = mutation({
  args: {
    key: v.string(),
    id: v.string(),
    title: v.optional(v.string()),
    goals: v.optional(v.string()),
    state: v.optional(v.string()),
    notes: v.optional(v.string()),
    links: v.optional(v.array(link)),
    addRepos: v.optional(v.array(v.string())),
  },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const p = await projectRow(ctx, a.id);
    const patch = checked(() => {
      const out: Record<string, unknown> = {};
      if (a.title !== undefined) out.title = cleanLine('The title', a.title, TITLE_MAX);
      if (a.goals !== undefined) out.goals = checkText('goals', a.goals, SHORT_MAX);
      if (a.state !== undefined) out.state = checkText('state', a.state, SHORT_MAX);
      if (a.notes !== undefined) out.notes = checkText('notes', a.notes, NOTES_MAX);
      if (a.links !== undefined) out.links = checkLinks(a.links);
      if (a.addRepos !== undefined) {
        const repos = checkRepos([...p.repos, ...a.addRepos]);
        if (repos.join() !== p.repos.join()) out.repos = repos;
      }
      return out;
    });
    // Nothing that differs: no write, so followers don't all re-run.
    const changed = Object.entries(patch).filter(([k, x]) => JSON.stringify(x) !== JSON.stringify((p as Record<string, unknown>)[k]));
    if (!changed.length) return { updatedAt: p.updatedAt, changed: false };
    const now = Date.now();
    await ctx.db.patch(p._id, { ...Object.fromEntries(changed), updatedAt: now, updatedBy: me.memberId });
    return { updatedAt: now, changed: true };
  },
});

// Stops sharing it: the person who shared it, or the host. Members' linked
// copies stay on their Macs as their own projects.
export const unshareProject = mutation({
  args: { key: v.string(), id: v.string() },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const p = await projectRow(ctx, a.id);
    if (p.sharedBy !== me.memberId && !me.host) throw fail('forbidden', 'only whoever shared it, or the host, can stop sharing it');
    const now = Date.now();
    await ctx.db.patch(p._id, { removedAt: now, updatedAt: now, updatedBy: me.memberId });
    for (const c of await ctx.db.query('teamCards').withIndex('by_project', (q) => q.eq('projectId', p.id)).collect()) await ctx.db.delete(c._id);
    for (const l of await ctx.db.query('teamLog').withIndex('by_project', (q) => q.eq('projectId', p.id)).collect()) await ctx.db.delete(l._id);
    return {};
  },
});

export const addLog = mutation({
  args: { key: v.string(), projectId: v.string(), text: v.string(), threadTitle: v.optional(v.string()) },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    await projectRow(ctx, a.projectId);
    const text = a.text.trim();
    if (!text) throw fail('badArgs', 'the note is empty');
    if (text.length > LOG_MAX) throw fail('badArgs', `a log entry is at most ${LOG_MAX} characters`);
    const now = Date.now();
    await ctx.db.insert('teamLog', { projectId: a.projectId, memberId: me.memberId, text, at: now, ...(a.threadTitle ? { threadTitle: a.threadTitle.slice(0, TITLE_MAX * 2) } : {}) });
    // Only the newest are ever shown; older ones go.
    const all = await ctx.db.query('teamLog').withIndex('by_project', (q) => q.eq('projectId', a.projectId)).collect();
    for (const old of all.slice(0, Math.max(0, all.length - LOG_SHOWN * 3))) await ctx.db.delete(old._id);
    return { at: now };
  },
});

// ------------------------------------------------------------------ the board

const cardInput = v.object({
  projectId: v.string(),
  threadId: v.string(),
  title: v.string(),
  status: v.string(),
  headline: v.optional(v.string()),
  branch: v.optional(v.string()),
  repo: v.optional(v.string()),
  handedTo: v.optional(v.string()),
});

// The caller's own cards: new or changed ones, and ones whose thread has
// gone or left the project.
export const putCards = mutation({
  args: { key: v.string(), cards: v.array(cardInput), gone: v.optional(v.array(v.object({ projectId: v.string(), threadId: v.string() }))) },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    if (a.cards.length > 100 || (a.gone?.length ?? 0) > 200) throw fail('badArgs', 'too many cards at once');
    const now = Date.now();
    const projects = new Map<string, boolean>();
    const live = async (id: string) => {
      if (!projects.has(id)) {
        const p = await ctx.db.query('teamProjects').withIndex('by_projectId', (q) => q.eq('id', id)).first();
        projects.set(id, Boolean(p && !p.removedAt));
      }
      return projects.get(id)!;
    };
    for (const g of a.gone ?? []) {
      const row = await ctx.db
        .query('teamCards')
        .withIndex('by_member_thread', (q) => q.eq('memberId', me.memberId).eq('threadId', g.threadId))
        .collect();
      for (const r of row) if (r.projectId === g.projectId) await ctx.db.delete(r._id);
    }
    for (const c of a.cards) {
      if (!validId(c.threadId)) throw fail('badArgs', 'bad thread id');
      if (!(CARD_STATUSES as readonly string[]).includes(c.status)) throw fail('badArgs', `status is one of ${CARD_STATUSES.join(', ')}`);
      if (!(await live(c.projectId))) continue;
      const doc = {
        projectId: c.projectId,
        memberId: me.memberId,
        threadId: c.threadId,
        title: c.title.trim().slice(0, TITLE_MAX * 2) || 'Untitled',
        status: c.status as (typeof CARD_STATUSES)[number],
        ...(c.headline?.trim() ? { headline: c.headline.trim().slice(0, 200) } : {}),
        ...(c.branch?.trim() ? { branch: c.branch.trim().slice(0, 200) } : {}),
        ...(c.repo && checkRepos([c.repo])[0] ? { repo: checkRepos([c.repo])[0] } : {}),
        ...(c.handedTo ? { handedTo: c.handedTo } : {}),
        updatedAt: now,
      };
      const rows = await ctx.db
        .query('teamCards')
        .withIndex('by_member_thread', (q) => q.eq('memberId', me.memberId).eq('threadId', c.threadId))
        .collect();
      const row = rows.find((r) => r.projectId === c.projectId);
      if (row) await ctx.db.replace(row._id, doc);
      else await ctx.db.insert('teamCards', doc);
    }
    // The caller's done cards past a week go, and past the cap the oldest.
    const mine = (await ctx.db.query('teamCards').collect()).filter((c) => c.memberId === me.memberId);
    for (const c of mine) if (c.status === 'done' && now - c.updatedAt > DONE_KEPT_MS) await ctx.db.delete(c._id);
    const kept = mine.filter((c) => !(c.status === 'done' && now - c.updatedAt > DONE_KEPT_MS)).sort((x, y) => x.updatedAt - y.updatedAt);
    for (const c of kept.slice(0, Math.max(0, kept.length - CARDS_MAX))) await ctx.db.delete(c._id);
    return {};
  },
});

// ------------------------------------------------------------------ messages

export const send = mutation({
  args: {
    key: v.string(),
    message: v.object({
      id: v.string(),
      projectId: v.string(),
      kind: v.union(v.literal('message'), v.literal('handover')),
      to: v.string(),
      toThread: v.optional(v.string()),
      fromThread: v.string(),
      fromTitle: v.string(),
      text: v.string(),
      repo: v.optional(v.string()),
      branch: v.optional(v.string()),
      links: v.optional(v.array(link)),
    }),
  },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const m = a.message;
    if (!validId(m.id) || !validId(m.fromThread) || (m.toThread !== undefined && !validId(m.toThread))) throw fail('badArgs', 'bad id');
    await projectRow(ctx, m.projectId);
    const to = await ctx.db
      .query('teamMembers')
      .withIndex('by_memberId', (q) => q.eq('memberId', m.to))
      .first();
    if (!to || to.revokedAt) throw fail('notFound', 'they are not on the team');
    if (to.memberId === me.memberId) throw fail('badArgs', "that's you: message your own thread with send_message as usual");
    const text = m.text.trim();
    if (!text) throw fail('badArgs', 'the message is empty');
    if (text.length > TEXT_MAX) throw fail('badArgs', `a message is at most ${TEXT_MAX} characters`);
    if ((await ctx.db.query('teamMessages').withIndex('by_messageId', (q) => q.eq('id', m.id)).first()) !== null) throw fail('exists', 'that message was already sent');
    const waiting = (await ctx.db.query('teamMessages').withIndex('by_from', (q) => q.eq('from', me.memberId).eq('toldSender', undefined)).collect()).filter((x) => x.status === 'waiting');
    if (waiting.length >= WAITING_MAX) throw fail('full', `you have ${WAITING_MAX} messages and handovers still waiting for answers`);
    const now = Date.now();
    const links = m.links ? checked(() => checkLinks(m.links)) : undefined;
    await ctx.db.insert('teamMessages', {
      id: m.id,
      projectId: m.projectId,
      kind: m.kind,
      from: me.memberId,
      fromThread: m.fromThread,
      fromTitle: m.fromTitle.trim().slice(0, TITLE_MAX * 2) || 'Untitled',
      to: to.memberId,
      ...(m.toThread ? { toThread: m.toThread } : {}),
      text,
      ...(m.repo && checkRepos([m.repo])[0] ? { repo: checkRepos([m.repo])[0] } : {}),
      ...(m.branch ? { branch: m.branch.slice(0, 200) } : {}),
      ...(links?.length ? { links } : {}),
      createdAt: now,
      status: 'waiting',
    });
    if (m.kind === 'handover') {
      const card = (await ctx.db.query('teamCards').withIndex('by_member_thread', (q) => q.eq('memberId', me.memberId).eq('threadId', m.fromThread)).collect()).find((c) => c.projectId === m.projectId);
      if (card) await ctx.db.patch(card._id, { handedTo: to.memberId, updatedAt: now });
    }
    return { at: now, toName: to.name };
  },
});

// The recipient's answer: passed on or dismissed (a message), taken or
// declined (a handover).
export const answer = mutation({
  args: { key: v.string(), id: v.string(), status: v.string(), answer: v.optional(v.string()) },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const row = await ctx.db
      .query('teamMessages')
      .withIndex('by_messageId', (q) => q.eq('id', a.id))
      .first();
    if (!row) throw fail('notFound', 'no such message');
    const why = mayAnswer(row, me.memberId, a.status);
    if (why) throw fail('forbidden', why);
    const now = Date.now();
    await ctx.db.patch(row._id, { status: a.status as 'passed' | 'dismissed' | 'taken' | 'declined', answeredAt: now, ...(a.answer?.trim() ? { answer: a.answer.trim().slice(0, 500) } : {}) });
    if (row.kind === 'handover' && a.status === 'declined') {
      const card = (await ctx.db.query('teamCards').withIndex('by_member_thread', (q) => q.eq('memberId', row.from).eq('threadId', row.fromThread)).collect()).find((c) => c.projectId === row.projectId);
      if (card && card.handedTo === me.memberId) await ctx.db.patch(card._id, { handedTo: undefined, updatedAt: now });
    }
    return {};
  },
});

// The sender's runner has told its threads how these were answered: they
// leave its `answered` list, and go for good after a few days.
export const told = mutation({
  args: { key: v.string(), ids: v.array(v.string()) },
  handler: async (ctx, a) => {
    const me = await memberOf(ctx, a.key);
    const now = Date.now();
    for (const id of a.ids.slice(0, 200)) {
      const row = await ctx.db
        .query('teamMessages')
        .withIndex('by_messageId', (q) => q.eq('id', id))
        .first();
      if (row && row.from === me.memberId && row.status !== 'waiting') await ctx.db.patch(row._id, { toldSender: true });
    }
    for (const old of await ctx.db.query('teamMessages').withIndex('by_from', (q) => q.eq('from', me.memberId).eq('toldSender', true)).collect()) {
      if (old.answeredAt && now - old.answeredAt > ANSWERED_KEPT_MS) await ctx.db.delete(old._id);
    }
    return {};
  },
});
