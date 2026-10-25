// The epic as a rail (`005e` §9.2): one row per node (a unit of work an agent was given), time running left to right,
// the waterline marking now, bars for finished, running and queued work, elbows for dependencies, and the critical
// path in the warning colour. Pure: the daemon's folded state in, `h` lines of `w` columns out.
import * as T from './theme.ts';
import { sanitize } from './theme.ts';
import type { OModel, OState } from './observe-screen.ts';

type Node = OState['nodes'][string];
type Paint = (s: string) => string;

const MIN = 60_000;
/** What a node that has not run is assumed to take when no node of its role has finished (`005e` §9.2). */
const DEFAULT_ESTIMATE = 3 * MIN;
const LABEL = 22;
const FINISHED = ['done', 'escalated', 'failed'];

export type Item = { node: Node; kind: 'done' | 'running' | 'queued'; start: number; end: number; critical: boolean };

const median = (v: number[]): number => {
  const s = [...v].sort((a, b) => a - b), m = s.length >> 1;
  return s.length % 2 ? s[m]! : (s[m - 1]! + s[m]!) / 2;
};

/** Times, kinds and the critical path of every node. The estimate of a node that has not finished is the median
 *  duration of the finished nodes of its role in this mission, else three minutes. */
export function plan(st: OState, now: number): Item[] {
  const nodes = Object.values(st.nodes).sort((a, b) => (a.started_ms || now) - (b.started_ms || now) || a.id.localeCompare(b.id));
  const spans: Record<string, number[]> = {};
  for (const n of nodes) if (FINISHED.includes(n.state) && n.started_ms > 0 && n.ended_ms != null && n.ended_ms >= n.started_ms) (spans[n.role] ??= []).push(n.ended_ms - n.started_ms);
  const estimate = (n: Node) => (spans[n.role]?.length ? median(spans[n.role]!) : DEFAULT_ESTIMATE);

  const items = new Map<string, Item>();
  const visiting = new Set<string>();
  const build = (n: Node): Item => {
    const have = items.get(n.id);
    if (have) return have;
    visiting.add(n.id);
    let item: Item;
    if (FINISHED.includes(n.state)) {
      const end = n.ended_ms ?? now;
      item = { node: n, kind: 'done', start: n.started_ms > 0 ? n.started_ms : end - estimate(n), end, critical: false };
    } else if (n.state === 'running') {
      const start = n.started_ms > 0 ? n.started_ms : now;
      item = { node: n, kind: 'running', start, end: Math.max(start + estimate(n), now + 15_000), critical: false };
    } else {
      const preds = n.after.map((id) => st.nodes[id]).filter((p): p is Node => !!p && !visiting.has(p.id)).map(build);
      const start = Math.max(now, ...preds.map((p) => p.end));
      item = { node: n, kind: 'queued', start, end: start + estimate(n), critical: false };
    }
    visiting.delete(n.id);
    items.set(n.id, item);
    return item;
  };
  const list = nodes.map(build);

  // The critical path: back from whatever finishes last, through the predecessor that finishes last, to the work
  // that is running or queued now (finished work is not on it).
  let cur: Item | undefined = list.reduce<Item | undefined>((best, i) => (!best || i.end > best.end ? i : best), undefined);
  while (cur && cur.kind !== 'done') {
    cur.critical = true;
    const preds = cur.node.after.map((id) => items.get(id)).filter((p): p is Item => !!p);
    cur = preds.reduce<Item | undefined>((best, i) => (!best || i.end > best.end ? i : best), undefined);
  }
  return list;
}

const glyphOf = (i: Item): [string, Paint] =>
  i.node.state === 'escalated' || i.node.state === 'failed' ? ['✖', T.alert] : i.kind === 'done' ? ['✔', T.ok] : i.kind === 'running' ? ['▶', T.accent] : ['○', T.muted];

const spark = (v: number[]): string => {
  const r = v.slice(-12), max = Math.max(0, ...r);
  return r.map((x) => (max <= 0 ? '▁' : '▁▂▃▄▅▆▇█'[Math.round((x / max) * 7)]!)).join('');
};

const clean = (s: string, n: number) => { const t = sanitize(s).replace(/\s+/g, ' ').trim(); return [...t].length <= n ? t : [...t].slice(0, Math.max(0, n - 1)).join('') + '…'; };
const span = (ms: number) => { const s = Math.max(0, Math.round(ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };

/** Which unit of work is "now" when the window has no clock (a test, an old caller): the newest thing it knows of. */
export const clockOf = (m: OModel): number => m.now && m.now > 1e12 ? m.now : Math.max(0, ...Object.values(m.st.nodes).flatMap((n) => [n.started_ms, n.ended_ms ?? 0])) + 1000;

export function epicRail(m: OModel, w: number, h: number): string[] {
  const now = clockOf(m);
  const items = plan(m.st, now);
  const dim = (s: string) => T.dim(s);
  const text = T.text(m.pal);
  const name = (i: Item) => T.roleName(i.node.role, m.info);
  const out: string[] = [];
  if (!items.length) return ['', dim('  No agents have run yet in this mission.')];

  const pace = m.st.pace?.length ? T.spread('', dim('pace  ') + T.accent(spark(m.st.pace)) + ' ', w) : '';
  const finish = (rows: string[], tail: string[]): string[] => {
    const body = [...rows, ...Array(Math.max(0, h - rows.length - tail.length)).fill(''), ...tail];
    return body.slice(0, h);
  };

  // Narrower than 72 columns there is no room for a rail: one row per node.
  if (w < 72) {
    const rows = items.map((i) => {
      const [g, paint] = glyphOf(i);
      const elapsed = i.kind === 'done' ? span(i.end - i.start) : i.kind === 'running' ? span(now - i.start) : 'queued';
      return T.clip(` ${paint(g)} ${T.roleColor(i.node.role, m.info)(name(i).padEnd(12))} ${text(i.node.state.padEnd(9))} ${dim(elapsed)}`, w);
    });
    const room = Math.max(1, h - (pace ? 1 : 0));
    const start = Math.min(m.scroll, Math.max(0, rows.length - room));
    return finish(rows.slice(start, start + room), pace ? [pace] : []);
  }

  // The window: from the start of the oldest work shown (at most 20 minutes back) to the estimated end of the last.
  const R = w - LABEL, W = Math.round(0.52 * R);
  const known = items.map((i) => i.start).filter((t) => t > 0);
  const earlier = Math.min(now - MIN, Math.max(Math.min(...known), now - 20 * MIN));
  const later = Math.max(now + 3 * MIN, ...items.map((i) => i.end));
  // The waterline is a fixed column, so each side of it has its own scale.
  const cell = (t: number): number => Math.max(0, Math.min(R - 1, t <= now ? Math.round(((t - earlier) / (now - earlier)) * W) : W + Math.round(((t - now) / (later - now)) * (R - 1 - W))));

  const grid: { c: string; p: Paint }[][] = items.map(() => Array.from({ length: R }, () => ({ c: ' ', p: (s: string) => s })));
  const put = (row: number, col: number, c: string, p: Paint) => { if (col >= 0 && col < R) grid[row]![col] = { c, p }; };
  const draw = (row: number, from: number, to: number, c: string, p: Paint) => { for (let x = Math.min(from, to); x <= Math.max(from, to); x++) put(row, x, c, p); };
  const lastQueued = items.reduce<Item | undefined>((b, i) => (i.kind === 'queued' && (!b || i.end > b.end) ? i : b), undefined);
  const barPaint = (i: Item): Paint => (i.critical ? T.warn : T.muted);

  items.forEach((i, r) => {
    const s = cell(i.start), e = cell(i.end);
    if (i.kind === 'done') draw(r, s, e, '━', T.muted);
    else if (i.kind === 'running') {
      draw(r, Math.min(s, W - 1), W - 1, '━', T.accent);
      put(r, W, '●', T.accent);
      draw(r, W + 1, e, i.critical ? '═' : '╌', barPaint(i));
    } else {
      draw(r, s, e, i.critical ? '═' : '╌', barPaint(i));
      if (i === lastQueued) put(r, e, '▸', barPaint(i));
    }
  });

  // A dependency is an elbow: `╮` where the predecessor ends, `╰` on the dependent's row at the same column.
  const row = new Map(items.map((i, r) => [i.node.id, r]));
  items.forEach((i, r) => {
    const preds = i.node.after.map((id) => items.find((p) => p.node.id === id)).filter((p): p is Item => !!p && row.get(p.node.id)! < r);
    const pred = preds.reduce<Item | undefined>((b, p) => (!b || p.end > b.end ? p : b), undefined);
    if (!pred) return;
    const col = cell(pred.end), pr = row.get(pred.node.id)!;
    if (cell(i.start) < col) return; // it started before the predecessor ended: no clean elbow
    const paint = pred.kind === 'done' ? T.muted : barPaint(pred);
    put(pr, col, '╮', paint);
    for (let k = pr + 1; k < r; k++) if (grid[k]![col]!.c === ' ') put(k, col, '│', T.muted);
    put(r, col, '╰', paint);
  });

  // The waterline runs down every row that has nothing in that column.
  grid.forEach((cells) => { if (cells[W]!.c === ' ') cells[W] = { c: '┆', p: T.muted }; });

  const labelRow = (i: Item): string => {
    const [g, paint] = glyphOf(i);
    return ' ' + paint(g) + ' ' + T.roleColor(i.node.role, m.info)(name(i).slice(0, 17).padEnd(19));
  };
  const rows = items.map((i, r) => labelRow(i) + grid[r]!.map((x) => x.p(x.c)).join(''));

  // `earlier`, `now` and `later` over the rail.
  const axis = Array.from({ length: R }, () => ' ');
  const lay = (word: string, at: number) => [...word].forEach((ch, k) => { if (at + k >= 0 && at + k < R) axis[at + k] = ch; });
  lay('earlier', 0); lay('later', R - 5); lay('now', Math.max(0, W - 1));
  const header = ' '.repeat(LABEL) + dim(axis.join('').replace(/\s+$/, ''));

  // Narrative: what is happening, what comes next, and the critical path with its estimated length.
  const running = items.find((i) => i.kind === 'running');
  const queued = items.filter((i) => i.kind === 'queued').sort((a, b) => a.start - b.start);
  const task = running ? m.st.tasks[running.node.task_id] : undefined;
  const nowLine = running
    ? ` ${dim('now ')}  ${T.badge(running.node.role, 'running', m.info, m.pal)} ${name(running)} is working${task ? `: ${clean(task.prompt, Math.max(10, w - 40))}` : ''}`
    : ` ${dim('now ')}  ${dim('nothing is running')}`;
  const nextItem = queued[0];
  const after = nextItem && nextItem.node.after.map((id) => items.find((p) => p.node.id === id)).find((p) => p && p.kind !== 'done');
  const nextLine = nextItem
    ? ` ${dim('next')}  ${T.badge(nextItem.node.role, 'idle', m.info, m.pal)} ${name(nextItem)} ${after ? `starts when ${name(after)} ends` : 'is queued'}`
    : ` ${dim('next')}  ${dim('nothing queued')}`;
  const chain = items.filter((i) => i.critical).sort((a, b) => a.start - b.start);
  const minutes = Math.max(1, Math.ceil((Math.max(...chain.map((i) => i.end), now) - now) / MIN));
  const pathLine = chain.length ? ` ${dim('path')}  ${text(chain.map(name).join(' → '))}  ${dim(`about ${minutes} min`)}` : ` ${dim('path')}  ${dim('everything has finished')}`;

  const narrative = [nowLine, nextLine, pathLine];
  const fixed = 1 + 1 + narrative.length + (pace ? 1 : 0); // axis, a blank, the narrative, the pace row
  const room = Math.max(1, h - fixed);
  const start = Math.min(m.scroll, Math.max(0, rows.length - room));
  const shown = rows.slice(start, start + room);
  out.push(header, ...shown, '', ...narrative);
  return finish(out.map((l) => T.clip(l, w)), pace ? [pace] : []);
}

