// The epic as threads (`005g`): the highlights of everything the mission is doing, grouped by what the work is about
// (named by the orchestrator, else read from the requests) instead of by who does it. Each thread is a short checklist
// of what happened, what is happening and what is next. The critical path, the chain of work that decides when it is
// all done, is summarised once at the top and drawn through the threads it passes through: those get a bar down the
// left edge and their steps on the path are bold, so the eye goes there first. Pure: the daemon's folded state in.
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
/** One step of a thread. `gist` is the short form the beats are built from; `detail` is a verdict or what a build is doing. */
export type Entry = { status: Status; text: string; gist: string; detail: string; role: string | null; at: number; since: number; critical: boolean; sub: Entry[] };
export type Tie = { thread: string; label: string; dir: 'waits' | 'feeds'; open: boolean };
export type State = 'needs-you' | 'active' | 'ahead' | 'settled';
export type Thread = { id: string; label: string; tasks: string[]; entries: Entry[]; state: State; ties: Tie[]; done: number; total: number; critical: boolean };
export type Path = { threads: Thread[]; steps: number; eta: number };

const MIN = 60_000;
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

/** Which tasks are about the same thing: a union of tasks whose words overlap enough. The orchestrator's own threads (`st.threads`) join the tasks they list. */
function cluster(st: OState): string[][] {
  const ids = new Set<string>();
  for (const k of Object.keys(st.tasks)) ids.add(k);
  for (const n of Object.values(st.nodes)) if (n.task_id) ids.add(n.task_id);
  for (const c of st.councils) ids.add(c.task_id);
  for (const b of st.builds) ids.add(b.task_id);
  const list = [...ids];
  const named = new Set((st.threads ?? []).flatMap((t) => t.tasks));
  const words = new Map(list.map((id) => [id, stems([st.tasks[id]?.prompt ?? '', ...st.councils.filter((c) => c.task_id === id).map((c) => c.proposition)].join(' '))]));
  const parent = new Map(list.map((id) => [id, id]));
  const find = (x: string): string => (parent.get(x) === x ? x : (parent.set(x, find(parent.get(x)!)), parent.get(x)!));
  const join = (a: string, b: string) => { const ra = find(a), rb = find(b); if (ra !== rb) parent.set(rb, ra); };
  for (const t of st.threads ?? []) { const known = t.tasks.filter((id) => parent.has(id)); for (const id of known.slice(1)) join(known[0]!, id); }
  for (let i = 0; i < list.length; i++) for (let j = i + 1; j < list.length; j++) {
    const x = list[i]!, y = list[j]!;
    if (named.has(x) || named.has(y)) continue; // the orchestrator has placed these; words do not move them
    const a = words.get(x)!, b = words.get(y)!;
    const shared = [...a].filter((w) => b.has(w)).length;
    if (shared >= 2 && (shared / Math.min(a.size, b.size) >= 0.5 || shared / (a.size + b.size - shared) >= 0.4)) join(x, y);
  }
  const groups = new Map<string, string[]>();
  for (const id of list) (groups.get(find(id)) ?? groups.set(find(id), []).get(find(id))!).push(id);
  return [...groups.values()];
}

const nodeStatus = (n: Node): Status => (n.state === 'done' ? 'done' : n.state === 'running' ? 'running' : n.state === 'escalated' || n.state === 'failed' ? 'stuck' : 'queued');
const gateStatus = (s: string): Status => (s === 'green' || s === 'merged' ? 'done' : s === 'running' ? 'running' : s === 'red' || s === 'failed' ? 'stuck' : 'queued');
const entry = (status: Status, text: string, over: Partial<Entry> = {}): Entry => ({ status, text, gist: text, detail: '', role: null, at: 0, since: 0, critical: false, sub: [], ...over });

function buildSub(b: Build): Entry[] {
  const sub: Entry[] = b.gates.map((g) => entry(gateStatus(g.state), g.name, { detail: g.detail, gist: g.detail ? `${g.name} ${g.detail}` : g.name }));
  for (const [sev, title] of b.findings) if (sev === 'P0' || sev === 'P1') sub.push(entry('stuck', `audit ${sev}`, { detail: title, gist: `audit ${sev}: ${title}` }));
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

const firstStart = (st: OState, task: string, items: Map<string, Item>) => Math.min(Infinity, ...Object.values(st.nodes).filter((n) => n.task_id === task).map((n) => items.get(n.id)?.start ?? Infinity));

/** The threads in the order to look at them: needs you, on the critical path, in flight, ahead, settled; and the critical path itself. */
export function threads(m: OModel, now: number): { list: Thread[]; path: Path } {
  const st = m.st;
  const plan_ = plan(st, now);
  const items = new Map<string, Item>(plan_.map((i) => [i.node.id, i]));
  const out: Thread[] = [];
  const threadOfTask = new Map<string, string>();

  for (const tasks of cluster(st)) {
    const set = new Set(tasks);
    const named = (st.threads ?? []).find((t) => t.tasks.some((id) => set.has(id)));
    const first = [...tasks].sort((a, b) => (firstStart(st, a, items) - firstStart(st, b, items)) || a.localeCompare(b))[0]!;
    // A step of the thread's first task needs no tag; a step of a later task says which task it belongs to.
    const ofTask = (id: string) => (id === first ? '' : clean(st.tasks[id]?.prompt ?? id, 28));
    const entries: Entry[] = [];
    const nodes = Object.values(st.nodes).filter((n) => set.has(n.task_id));
    const buildsLeft = new Map(st.builds.filter((b) => set.has(b.task_id)).map((b) => [b.task_id, b]));
    for (const n of nodes) {
      const it = items.get(n.id);
      const tag = ofTask(n.task_id);
      const e = entry(nodeStatus(n), tag ? `${verb(n.role)} · ${tag}` : verb(n.role), { role: n.role, at: it?.start ?? 0, since: n.started_ms, gist: verb(n.role), critical: !!it?.critical });
      const b = buildsLeft.get(n.task_id);
      if (b && n.role === 'builder') { e.sub = buildSub(b); buildsLeft.delete(n.task_id); }
      entries.push(e);
    }
    for (const b of buildsLeft.values()) entries.push(entry(b.merge === 'merged' ? 'done' : 'running', `build ${b.branch || b.task_id}`, { sub: buildSub(b), at: now }));
    for (const c of st.councils.filter((c) => set.has(c.task_id))) entries.push({ ...councilEntry(c), at: Math.min(now, ...nodes.filter((n) => n.task_id === c.task_id).map((n) => n.started_ms).filter((x) => x > 0)) });
    entries.sort((a, b) => a.at - b.at);

    const label = clean(named?.label ?? st.tasks[first]?.prompt ?? st.councils.find((c) => c.task_id === first)?.proposition ?? first, 44);
    const active = entries.some((e) => e.status === 'running'), stuck = entries.some((e) => e.status === 'stuck' || e.sub.some((x) => x.status === 'stuck'));
    const done = entries.filter((e) => e.status === 'done').length;
    const state: State = stuck ? 'needs-you' : active ? 'active' : entries.length && done === entries.length ? 'settled' : 'ahead';
    const id = named?.id ?? first;
    for (const t of tasks) threadOfTask.set(t, id);
    out.push({ id, label, tasks, entries, state, ties: [], done, total: entries.length, critical: entries.some((e) => e.critical) });
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
      byId.get(a)!.ties.push({ thread: b, label: byId.get(b)!.label, dir: 'waits', open });
      byId.get(b)!.ties.push({ thread: a, label: byId.get(a)!.label, dir: 'feeds', open });
    }
  }

  // The critical path, in the order its work happens, as the threads it passes through.
  const chain = plan_.filter((i) => i.critical).sort((a, b) => a.start - b.start);
  const onPath: Thread[] = [];
  for (const i of chain) { const t = byId.get(threadOfTask.get(i.node.task_id) ?? ''); if (t && !onPath.includes(t)) onPath.push(t); }
  const path: Path = { threads: onPath, steps: chain.length, eta: chain.length ? Math.max(...chain.map((i) => i.end)) - now : 0 };

  const rank = (t: Thread) => (t.state === 'needs-you' ? 0 : t.state === 'settled' ? 4 : t.critical ? 1 : t.state === 'active' ? 2 : 3);
  const order = (t: Thread) => (t.critical ? onPath.indexOf(t) : 0);
  return { list: out.sort((a, b) => rank(a) - rank(b) || order(a) - order(b) || (a.entries[0]?.at ?? 0) - (b.entries[0]?.at ?? 0) || a.id.localeCompare(b.id)), path };
}

const STEP_GLYPH: Record<Status, [string, Paint]> = { done: ['✔', T.ok], running: ['▶', T.accent], queued: ['○', T.muted], stuck: ['✖', T.alert] };
const STATE_GLYPH: Record<State, [string, Paint]> = { 'needs-you': ['▼', T.warn], active: ['◐', T.accent], ahead: ['○', T.muted], settled: ['✔', T.ok] };
const meter = (done: number, of: number, n: number) => { const f = of > 0 ? Math.round((n * done) / of) : 0; return '▰'.repeat(f) + '▱'.repeat(n - f); };
/** What a step is doing right now: its detail, else the gist of the gate of its build that is running or has failed. */
const doingNow = (e: Entry): string => e.detail || e.sub.find((x) => x.status === 'stuck')?.gist || e.sub.find((x) => x.status === 'running')?.gist || '';

/** Parts joined with `sep` to fit `room` columns; the ones that do not fit give way to `+n`. */
function fit(parts: string[], room: number, sep = ' · '): string {
  let kept = parts.slice();
  const text = () => kept.join(sep) + (kept.length < parts.length ? ` +${parts.length - kept.length}` : '');
  while (kept.length > 1 && [...text()].length > room) kept = kept.slice(0, -1);
  return text();
}

export function epicThreads(m: OModel, w: number, h: number): string[] {
  const now = clockOf(m);
  const dim = (s: string) => T.dim(s), text = T.text(m.pal);
  const { list, path } = threads(m, now);
  if (!list.length) return ['', dim('  Nothing is in progress yet. Ask for something in the console and it will appear here,'), dim('  grouped by what it is about.')];

  const room = Math.max(10, w - 10);
  const bar = (on: boolean) => (on ? T.warn('▌') : ' ');
  const rows: string[] = [];

  // The critical path: where the finish is decided, and how long until it, in the warning colour the threads on it share.
  if (path.threads.length) {
    const minutes = Math.max(1, Math.ceil(path.eta / MIN));
    const chain = path.threads.map((t) => clean(t.label, path.threads.length > 2 ? 16 : 26)).join(' → ');
    rows.push(T.spread(` ${T.warn('▌')} ${T.warn(chalk.bold('critical path'))}  ${text(chain)}`, dim(`about ${minutes} min `), w), '');
  } else rows.push(` ${dim('no critical path: nothing is waiting on anything else')}`, '');

  const settled = list.filter((t) => t.state === 'settled');
  for (const t of list.filter((x) => x.state !== 'settled')) {
    const [g, paint] = STATE_GLYPH[t.state];
    const progress = w >= 56 && t.total ? dim(`${meter(t.done, t.total, 6)} ${t.done} of ${t.total}`) : '';
    const tie = t.ties.find((x) => x.dir === 'waits' && x.open) ?? t.ties.find((x) => x.dir === 'feeds' && x.open);
    const tieText = !tie || w < 64 ? '' : (tie.dir === 'waits' ? '⇠ waits on ' : '⇢ feeds ') + clean(tie.label, 16);
    const right = [tieText ? T.warn(tieText) : '', progress].filter(Boolean).join('  ');
    rows.push(T.spread(`${bar(t.critical)}${paint(g)} ${t.critical ? chalk.bold(text(t.label)) : text(t.label)}`, right ? right + ' ' : '', w));

    const line = (status: Status, body: string, strong: boolean) => `${bar(t.critical)}  ${STEP_GLYPH[status][1](STEP_GLYPH[status][0])} ${strong ? text(body) : dim(body)}`;
    const done = t.entries.filter((e) => e.status === 'done');
    const verdicts = done.filter((e) => e.detail).map((e) => e.detail), plain = done.length - verdicts.length;
    if (done.length) rows.push(line('done', fit([...verdicts, ...(plain ? [`${plain} step${plain === 1 ? '' : 's'} done`] : [])], room), false));
    const now_ = t.entries.filter((e) => e.status === 'running' || e.status === 'stuck');
    if (now_.length) rows.push(line(now_.some((e) => e.status === 'stuck' || e.sub.some((x) => x.status === 'stuck')) ? 'stuck' : 'running', fit(now_.map((e) => {
      const who = e.role && e.status === 'running' ? `${T.roleName(e.role, m.info)} ${verbing(e.role)} ${span(now - e.since)}` : '';
      const what = e.role ? doingNow(e) : [e.gist, e.detail].filter(Boolean).join(' · ');
      return [who, what].filter(Boolean).join(' · ') || verb(e.role ?? e.text);
    }), room), true));
    const next = t.entries.filter((e) => e.status === 'queued');
    if (next.length) {
      const shown = next.slice(0, 3), more = next.length - shown.length;
      const named = shown.map((e) => (e.critical ? chalk.bold(T.warn(e.gist)) : dim(e.gist)));
      rows.push(`${bar(t.critical)}  ${T.muted('○')} ${named.join(dim(' → '))}${more ? dim(` +${more}`) : ''}`);
    }
    rows.push('');
  }
  if (settled.length) rows.push(`  ${T.ok('✔')} ${dim('settled  ' + fit(settled.map((t) => clean(t.label, 30)), Math.max(10, w - 14)))}`);
  while (rows.length && rows.at(-1) === '') rows.pop();

  const pace = T.spread(' ' + dim(rows.length > h - 1 ? '↑ ↓  more' : ''), m.st.pace?.length ? dim('pace  ') + T.accent(paceSpark(m.st.pace)) + ' ' : '', w);
  const roomRows = Math.max(1, h - 1);
  const start = Math.min(Math.max(0, m.scroll), Math.max(0, rows.length - roomRows));
  const shown = rows.slice(start, start + roomRows);
  return [...shown, ...Array(Math.max(0, h - shown.length - 1)).fill(''), pace].slice(0, h).map((l) => T.clip(l, w));
}

const paceSpark = (v: number[]): string => {
  const r = v.slice(-12), max = Math.max(0, ...r);
  return r.map((x) => (max <= 0 ? '▁' : '▁▂▃▄▅▆▇█'[Math.round((x / max) * 7)]!)).join('');
};
