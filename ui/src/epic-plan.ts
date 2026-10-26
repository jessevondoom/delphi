// When each unit of work ran or will run (`005e` §9.2, kept for `005g`): finished work from its recorded times, queued work
// estimated from its role's history and placed after its predecessors, and the chain that decides when the mission ends.
// Pure: the daemon's folded state in, items out.
import type { OModel, OState } from './observe-screen.ts';

type Node = OState['nodes'][string];

const MIN = 60_000;
/** What a node that has not run is assumed to take when no node of its role has finished (`005e` §9.2). */
const DEFAULT_ESTIMATE = 3 * MIN;
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

/** Which unit of work is "now" when the window has no clock (a test, an old caller): the newest thing it knows of. */
export const clockOf = (m: OModel): number => m.now && m.now > 1e12 ? m.now : Math.max(0, ...Object.values(m.st.nodes).flatMap((n) => [n.started_ms, n.ended_ms ?? 0])) + 1000;
