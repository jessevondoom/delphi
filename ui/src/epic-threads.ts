// The epic as threads (`005g`): everything the mission is doing, grouped by what it is about instead of by who is doing it.
// Each thread is a three-beat checklist (done, now, next); the thread the cursor is on opens into the full checklist,
// with a build's gates and findings under the step that produced them. Threads that depend on one another say so
// in their header, so you can see how one piece of work moves another. Pure: the daemon's folded state in, `h` lines out.
import chalk from 'chalk';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';
import type { OModel, OState } from './observe-screen.ts';
import { clockOf, plan, type Item } from './epic-plan.ts';
import { positions } from './weave.ts';

type Node = OState['nodes'][string];
type Council = OState['councils'][number];
type Build = OState['builds'][number];
type Paint = (s: string) => string;

export type Status = 'done' | 'running' | 'queued' | 'stuck';
/** One line of a thread's checklist. `gist` is the short form the done and next beats are built from. */
export type Entry = { status: Status; text: string; gist: string; detail: string; role: string | null; at: number; since: number; sub: Entry[] };
/** `step` is the finished-or-not step of the thread that is fed (for `waits`) or that feeds (for `feeds`). */
export type Tie = { thread: string; label: string; dir: 'waits' | 'feeds'; open: boolean; step: string };
export type State = 'needs-you' | 'active' | 'ahead' | 'settled';
export type Thread = { id: string; label: string; tasks: string[]; entries: Entry[]; state: State; ties: Tie[]; done: number; total: number; eta: number };

const STOP = new Set('the a an and or of to for in on at by with from into is are be as it its this that we our your add make use new via per then than not can should will'.split(' '));
const VERB: Record<string, [string, string]> = {
  architect: ['design', 'designing'], strategist: ['plan', 'planning'], builder: ['build', 'building'], 'red-team': ['audit', 'auditing'],
  orchestrator: ['wrap up', 'wrapping up'], scout: ['research', 'researching'], investigator: ['investigate', 'investigating'],
};
const verb = (role: string) => VERB[role]?.[0] ?? role;
const verbing = (role: string) => VERB[role]?.[1] ?? role;

const clean = (s: string, n: number) => { const t = sanitize(s).replace(/\s+/g, ' ').trim(); return [...t].length <= n ? t : [...t].slice(0, Math.max(0, n - 1)).join('') + '…'; };
const span = (ms: number) => { const s = Math.max(0, Math.round(ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
const stems = (s: string): Set<string> => new Set(
  sanitize(s).toLowerCase().split(/[^a-z0-9]+/).filter((x) => x.length > 2 && !STOP.has(x)).map((x) => (x.length > 4 ? x.replace(/(ing|ed|es|s)$/, '') : x)),
);

/** Which tasks are about the same thing: a union of tasks whose words overlap enough. The daemon's own grouping (`st.threads`) wins where it speaks. */
function cluster(st: OState): string[][] {
  const ids = new Set<string>();
  for (const k of Object.keys(st.tasks)) ids.add(k);
  for (const n of Object.values(st.nodes)) if (n.task_id) ids.add(n.task_id);
  for (const c of st.councils) ids.add(c.task_id);
  for (const b of st.builds) ids.add(b.task_id);
  const list = [...ids];
  const words = new Map(list.map((id) => [id, stems([st.tasks[id]?.prompt ?? '', ...st.councils.filter((c) => c.task_id === id).map((c) => c.proposition)].join(' '))]));
  const parent = new Map(list.map((id) => [id, id]));
  const find = (x: string): string => (parent.get(x) === x ? x : (parent.set(x, find(parent.get(x)!)), parent.get(x)!));
  const join = (a: string, b: string) => { const ra = find(a), rb = find(b); if (ra !== rb) parent.set(rb, ra); };
  for (const t of st.threads ?? []) for (const id of t.tasks.slice(1)) if (parent.has(id) && parent.has(t.tasks[0]!)) join(t.tasks[0]!, id);
  for (let i = 0; i < list.length; i++) for (let j = i + 1; j < list.length; j++) {
    const a = words.get(list[i]!)!, b = words.get(list[j]!)!;
    const shared = [...a].filter((x) => b.has(x)).length;
    if (shared >= 2 && (shared / Math.min(a.size, b.size) >= 0.5 || shared / (a.size + b.size - shared) >= 0.4)) join(list[i]!, list[j]!);
  }
  const groups = new Map<string, string[]>();
  for (const id of list) (groups.get(find(id)) ?? groups.set(find(id), []).get(find(id))!).push(id);
  return [...groups.values()];
}

const nodeStatus = (n: Node): Status => (n.state === 'done' ? 'done' : n.state === 'running' ? 'running' : n.state === 'escalated' || n.state === 'failed' ? 'stuck' : 'queued');
const gateStatus = (s: string): Status => (s === 'green' || s === 'merged' ? 'done' : s === 'running' ? 'running' : s === 'red' || s === 'failed' ? 'stuck' : 'queued');
const entry = (status: Status, text: string, over: Partial<Entry> = {}): Entry => ({ status, text, gist: text, detail: '', role: null, at: 0, since: 0, sub: [], ...over });

function buildSub(b: Build): Entry[] {
  const sub: Entry[] = b.gates.map((g) => entry(gateStatus(g.state), g.name, { detail: g.detail, gist: g.detail ? `${g.name} ${g.detail}` : g.name }));
  for (const [sev, title] of b.findings) sub.push(entry(sev === 'P0' || sev === 'P1' ? 'stuck' : 'queued', `audit ${sev}`, { detail: title }));
  if (b.flaky) sub.push(entry('queued', 'flaky', { detail: 'a test failed once and passed on a rerun' }));
  return sub;
}

function councilEntry(c: Council): Entry {
  const pos = positions(c), n = c.seats.length;
  const prompt = `council: ${clean(c.proposition, 40)}`;
  if (c.phase === 'round 1' || c.phase === 'round 2') {
    const answered = c.stances.filter((s) => s.answered).length;
    return entry('running', prompt, { gist: c.phase === 'round 1' ? `council is answering blind, ${answered} of ${n}` : 'council is debating', detail: pos.tally.join(' · ') });
  }
  if (c.escalation) return entry('stuck', prompt, { gist: `council is split: ${clean(c.options.map(([o]) => o).join(' or ') || c.escalation, 40)}`, detail: 'needs you' });
  const verdict = pos.majority ? `chose ${clean(pos.majority.option, 24)} ${pos.majority.n} of ${n}` : 'closed with no majority';
  return entry('done', prompt, { gist: verdict, detail: verdict });
}

/** The threads, in the order a person should look at them: needs you, in flight, ahead, then settled. */
export function threads(m: OModel, now: number): Thread[] {
  const st = m.st;
  const items = new Map<string, Item>(plan(st, now).map((i) => [i.node.id, i]));
  const groups = cluster(st);
  const out: Thread[] = [];
  const threadOfTask = new Map<string, string>();

  for (const tasks of groups) {
    const set = new Set(tasks);
    const forced = (st.threads ?? []).find((t) => t.tasks.some((id) => set.has(id)));
    const first = [...tasks].sort((a, b) => (firstStart(st, a, items) - firstStart(st, b, items)) || a.localeCompare(b))[0]!;
    // A step of the thread's first task needs no tag; a step of a later task says which task it belongs to.
    const ofTask = (id: string) => (id === first ? '' : clean(st.tasks[id]?.prompt ?? id, 28));
    const entries: Entry[] = [];
    const nodes = Object.values(st.nodes).filter((n) => set.has(n.task_id));
    const buildsLeft = new Map(st.builds.filter((b) => set.has(b.task_id)).map((b) => [b.task_id, b]));
    for (const n of nodes) {
      const it = items.get(n.id);
      const tag = ofTask(n.task_id);
      const e = entry(nodeStatus(n), tag ? `${verb(n.role)} · ${tag}` : verb(n.role), { role: n.role, at: it?.start ?? 0, since: n.started_ms, gist: verb(n.role) });
      const b = buildsLeft.get(n.task_id);
      if (b && n.role === 'builder') { e.sub = buildSub(b); buildsLeft.delete(n.task_id); }
      entries.push(e);
    }
    for (const b of buildsLeft.values()) entries.push(entry(b.merge === 'merged' ? 'done' : 'running', `build ${b.branch || b.task_id}`, { sub: buildSub(b), at: now }));
    for (const c of st.councils.filter((c) => set.has(c.task_id))) entries.push({ ...councilEntry(c), at: Math.min(now, ...nodes.filter((n) => n.task_id === c.task_id).map((n) => n.started_ms).filter((x) => x > 0)) });
    entries.sort((a, b) => a.at - b.at);

    const label = clean(forced?.label ?? st.tasks[first]?.prompt ?? st.councils.find((c) => c.task_id === first)?.proposition ?? first, 44);
    const active = entries.some((e) => e.status === 'running'), stuck = entries.some((e) => e.status === 'stuck');
    const done = entries.filter((e) => e.status === 'done').length;
    const state: State = stuck ? 'needs-you' : active ? 'active' : entries.length && done === entries.length ? 'settled' : 'ahead';
    const id = forced?.id ?? first;
    for (const t of tasks) threadOfTask.set(t, id);
    // Minutes until the last step that has not finished is expected to end (`plan`'s estimates).
    const eta = Math.max(0, ...nodes.filter((n) => n.state !== 'done').map((n) => (items.get(n.id)?.end ?? now) - now));
    out.push({ id, label, tasks, entries, state, ties: [], done, total: entries.length, eta });
  }

  // Ties: a step that waits on a step in another thread. The waiting thread says what it waits on; the other says what it feeds.
  const byId = new Map(out.map((t) => [t.id, t]));
  const seen = new Set<string>();
  for (const n of Object.values(st.nodes)) {
    for (const pid of [...n.after, ...(n.parent ? [n.parent] : [])]) {
      const p = st.nodes[pid];
      const a = threadOfTask.get(n.task_id), b = p && threadOfTask.get(p.task_id);
      if (!p || !a || !b || a === b || seen.has(`${a}>${b}`)) continue;
      seen.add(`${a}>${b}`);
      const open = p.state !== 'done';
      byId.get(a)!.ties.push({ thread: b, label: byId.get(b)!.label, dir: 'waits', open, step: verb(p.role) });
      byId.get(b)!.ties.push({ thread: a, label: byId.get(a)!.label, dir: 'feeds', open, step: verb(n.role) });
    }
  }
  const rank: Record<State, number> = { 'needs-you': 0, active: 1, ahead: 2, settled: 3 };
  return out.sort((a, b) => rank[a.state] - rank[b.state] || (a.entries[0]?.at ?? 0) - (b.entries[0]?.at ?? 0) || a.id.localeCompare(b.id));
}

const firstStart = (st: OState, task: string, items: Map<string, Item>) => Math.min(Infinity, ...Object.values(st.nodes).filter((n) => n.task_id === task).map((n) => items.get(n.id)?.start ?? Infinity));

const STATE_GLYPH: Record<State, [string, Paint]> = { 'needs-you': ['▼', T.warn], active: ['◐', T.accent], ahead: ['○', T.muted], settled: ['✔', T.ok] };
const STEP_GLYPH: Record<Status, [string, Paint]> = { done: ['✔', T.ok], running: ['▶', T.accent], queued: ['○', T.muted], stuck: ['✖', T.alert] };
/** What a step is doing right now: its detail, else the gist of the gate of its build that is running. */
const running = (e: Entry): string => e.detail || e.sub.find((x) => x.status === 'running')?.gist || '';
const meter = (done: number, of: number, n: number) => { const f = of > 0 ? Math.round((n * done) / of) : 0; return '▰'.repeat(f) + '▱'.repeat(n - f); };

/** Parts joined with a separator to fit `room` columns. Those that do not fit give way to `+n`: the oldest, or with `keep: 'first'` the last. */
function fit(parts: string[], room: number, sep = ' · ', keep: 'first' | 'last' = 'last'): string {
  let kept = parts.slice();
  const text = () => (keep === 'last' ? (kept.length < parts.length ? `+${parts.length - kept.length}${sep}` : '') + kept.join(sep) : kept.join(sep) + (kept.length < parts.length ? ` +${parts.length - kept.length}` : ''));
  while (kept.length > 1 && [...text()].length > room) kept = keep === 'last' ? kept.slice(1) : kept.slice(0, -1);
  return text();
}

/** What a step in flight is doing, in a few words: who and for how long, then what its build is up to; a step with no agent (a council) says its own gist. */
const doingOf = (m: OModel, now: number) => (e: Entry): string => {
  const who = e.role && e.status === 'running' ? `${T.roleName(e.role, m.info)} ${verbing(e.role)} ${span(now - e.since)}` : '';
  const what = e.role ? running(e) : [e.gist, e.detail].filter(Boolean).join(' · ');
  return [who, what].filter(Boolean).join(' · ') || verb(e.role ?? e.text);
};

export function epicThreads(m: OModel, w: number, h: number): string[] {
  const now = clockOf(m);
  const dim = (s: string) => T.dim(s), text = T.text(m.pal);
  const list = threads(m, now);
  if (!list.length) return ['', dim('  Nothing is in progress yet. Ask for something in the console and it will appear here,'), dim('  grouped by what it is about.')];

  // At the top the pane is an overview of every thread; each press of down opens the next thread's full checklist.
  const focus = m.scroll <= 0 ? -1 : Math.min(m.scroll - 1, list.length - 1);
  const name = (role: string | null) => (role ? T.roleColor(role, m.info)(T.roleName(role, m.info)) : '');
  const room = Math.max(10, w - 12);
  const rows: string[] = [], starts: number[] = [];

  const head = (t: Thread): string => {
    const [g, paint] = STATE_GLYPH[t.state];
    const progress = w >= 56 && t.total ? dim(`${meter(t.done, t.total, 6)} ${t.done}/${t.total}${w >= 80 && t.eta > 0 ? ` · ~${Math.max(1, Math.ceil(t.eta / 60_000))}m` : ''}`) : '';
    const tie = t.ties.find((x) => x.dir === 'waits' && x.open) ?? t.ties.find((x) => x.dir === 'feeds' && x.open) ?? t.ties[0];
    const tieText = !tie || w < 64 ? '' : (tie.dir === 'waits' ? (tie.open ? '⇠ waits on ' : '⇠ built on ') : '⇢ feeds ') + clean(tie.label, 16);
    const tiePaint = tie?.open ? T.warn : T.muted;
    const right = [tieText ? tiePaint(tieText) : '', progress].filter(Boolean).join('  ');
    return T.spread(` ${paint(g)} ${t.state === 'settled' ? dim(t.label) : chalk.bold(text(t.label))}`, right ? right + ' ' : '', w);
  };
  const beat = (status: Status, word: string, body: string, bright = false): string => {
    const [g, paint] = STEP_GLYPH[status];
    return `   ${paint(g)} ${dim(word.padEnd(5))}${bright ? text(body) : dim(body)}`;
  };

  list.forEach((t, idx) => {
    starts.push(rows.length);
    if (idx > 0) rows.push('');
    const open = idx === focus;
    rows.push(head(t));
    if (t.state === 'settled' && !open) {
      const gist = fit(t.entries.filter((e) => e.detail).map((e) => e.detail), room);
      if (gist) rows.push('   ' + dim(gist));
      return;
    }
    if (open) { // the full checklist, a build's gates and findings under the step that made them
      const check = (e: Entry, depth: number) => {
        const [g, paint] = STEP_GLYPH[e.status];
        const who = e.role && e.status === 'running' ? `  ${name(e.role)} ${dim(verbing(e.role))} ${dim(span(now - e.since))}` : '';
        const more = e.detail && depth === 0 ? dim(`  ${clean(e.detail, Math.max(10, w - 30))}`) : '';
        const body = depth ? (e.detail ? `${e.text} ${dim(clean(e.detail, Math.max(10, w - 24)))}` : e.text) : e.text;
        rows.push(`${' '.repeat(3 + depth * 3)}${paint(g)} ${e.status === 'queued' || e.status === 'done' ? dim(body) : text(body)}${who}${more}`);
        for (const s of e.sub) check(s, depth + 1);
      };
      for (const e of t.entries) check(e, 0);
      for (const x of t.ties) {
        const text2 = x.dir === 'waits' ? `${x.open ? 'waits on' : 'built on'} the ${x.step} in ${clean(x.label, Math.max(12, w - 36))}` : `${x.open ? 'holds up' : 'fed'} the ${x.step} in ${clean(x.label, Math.max(12, w - 36))}`;
        rows.push('   ' + (x.open ? T.warn : T.muted)(`${x.dir === 'waits' ? '⇠' : '⇢'} ${text2}`));
      }
      return;
    }
    const done = t.entries.filter((e) => e.status === 'done');
    const run = t.entries.filter((e) => e.status === 'running' || e.status === 'stuck');
    const next = t.entries.filter((e) => e.status === 'queued');
    if (done.length) rows.push(beat('done', 'done', fit([...done.filter((e) => e.detail), ...done.filter((e) => !e.detail)].map((e) => e.detail || e.gist), room, ' · ', 'first')));
    if (run.length) rows.push(beat(run.some((e) => e.status === 'stuck') ? 'stuck' : 'running', 'now', fit(run.map(doingOf(m, now)), room), true));
    if (next.length) rows.push(beat('queued', 'next', fit(next.map((e) => e.gist), room, ' → ')));
  });

  const pace = T.spread(' ' + dim(focus < 0 ? '↑ ↓  open a thread' : '↑ ↓  another thread'), m.st.pace?.length ? dim('pace  ') + T.accent(paceSpark(m.st.pace)) + ' ' : '', w);
  const room2 = Math.max(1, h - 1);
  const start = rows.length <= room2 || focus < 0 ? 0 : Math.min(starts[focus]!, rows.length - room2);
  const shown = rows.slice(start, start + room2);
  return [...shown, ...Array(Math.max(0, h - shown.length - 1)).fill(''), pace].slice(0, h).map((l) => T.clip(l, w));
}

const paceSpark = (v: number[]): string => {
  const r = v.slice(-12), max = Math.max(0, ...r);
  return r.map((x) => (max <= 0 ? '▁' : '▁▂▃▄▅▆▇█'[Math.round((x / max) * 7)]!)).join('');
};
