// The monitoring panes (`specs/034`, a spike): `timeline` (who worked when, with the changes
// they recorded) with `tools` (what the team calls, how often it fails, and who reports their work) below it, on one page. Pure: the
// folded state and a few in-memory samples in, `h` lines of `w` columns out. Read-only, no boxes, no reasoning, no text a
// model wrote: names, counts, times and states only.
import * as T from './theme.ts';
import { bar, colored, LOOP_CALLS, LOOP_FAILS, spark } from './meters.ts';
import { clockOf, plan } from './epic-plan.ts';
import type { OModel, OState } from './observe-screen.ts';

const dim = T.dim;
const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout', 'investigator'];
const byRole = (a: string, b: string) => (ORDER.indexOf(a) + 99) % 99 - (ORDER.indexOf(b) + 99) % 99;
const hhmm = (ms: number) => { const d = new Date(ms); return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`; };
const pad = (s: string, n: number) => s + ' '.repeat(Math.max(0, n - T.width(s)));
const head = (m: OModel, title: string, right: string, w: number) => T.spread(' ' + T.accent(title), right ? dim(right + ' ') : '', w);
const calm = (...lines: string[]) => ['', ...lines.map((l) => dim('  ' + l))];

/** `6m`, `1h 05m`: how long ago, coarse. */
const ago = (ms: number) => { const m = Math.max(0, Math.round(ms / 60_000)); return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`; };

/** Cells of a lane: which of `n` columns each of the role's runs covers, drawn by how the run ended. */
function lane(nodes: OState['nodes'][string][], start: number, end: number, n: number): string[] {
  const cells = Array<string>(n).fill(' ');
  const at = (ms: number) => Math.max(0, Math.min(n - 1, Math.floor(((ms - start) / (end - start)) * n)));
  for (const nd of nodes) {
    if (nd.state === 'pending' || !nd.started_ms) continue;
    const a = at(nd.started_ms), b = at(nd.ended_ms ?? end);
    const paint = nd.state === 'running' ? T.accent : nd.state === 'failed' || nd.state === 'escalated' ? T.alert : T.muted;
    const ch = nd.state === 'running' ? '▓' : nd.state === 'failed' || nd.state === 'escalated' ? '▒' : '█';
    for (let i = a; i <= b; i++) cells[i] = paint(ch);
  }
  return cells;
}

/**
 * timeline: a lane per role across the last stretch of the mission (up to an hour), each run drawn where it happened,
 * with a ✎ wherever that role recorded a change. Silent gaps and long runs are visible at a glance.
 */
export function timeline(m: OModel, w: number, h: number): string[] {
  const st = m.st, now = m.now ?? 0;
  const nodes = Object.values(st.nodes).filter((n) => n.state !== 'pending' && n.started_ms);
  const out: string[] = [];
  if (!nodes.length || !now) return [head(m, 'timeline', '', w), ...calm('', 'no work has started yet', 'a lane per agent appears here: each run where it happened, and a ✎ for every change it recorded')];
  const start = Math.max(Math.min(...nodes.map((n) => n.started_ms)), now - 60 * 60_000);
  const span = Math.max(now - start, 5 * 60_000);
  const labelW = 13, n = Math.max(10, w - labelW - 3);
  const from = now - span;
  out.push(head(m, 'timeline', `${Math.round(span / 60_000)} min · ${hhmm(from)} to now`, w), '');
  const roles = [...new Set(nodes.map((x) => x.role))].sort(byRole);
  for (const r of roles) {
    const cells = lane(nodes.filter((x) => x.role === r), from, now, n);
    for (const c of st.changes ?? []) if (c.role === r && c.at_ms >= from) cells[Math.max(0, Math.min(n - 1, Math.floor(((c.at_ms - from) / span) * n)))] = T.ok('✎');
    out.push(' ' + pad(T.roleColor(r, m.info).bold(T.roleName(r, m.info)), labelW) + cells.join(''));
  }
  const ticks = Array<string>(n).fill(' ');
  for (let k = 0; k <= 4; k++) { const label = hhmm(from + (span * k) / 4); const x = Math.min(n - label.length, Math.floor((n * k) / 4)); [...label].forEach((c, i) => { ticks[x + i] = c; }); }
  out.push(' ' + ' '.repeat(labelW) + dim(ticks.join('')));
  const total = (st.changes ?? []).filter((c) => c.at_ms >= from).length;
  out.push('', ' ' + dim(`${T.accent('▓')} running  █ done  ${T.alert('▒')} stopped  ${T.ok('✎')} change recorded`) + dim(`  ·  ${total} change${total === 1 ? '' : 's'} in this window`));
  // Momentum: changes recorded per five minutes over the window, how long since the last, and when the plan says it ends.
  const buckets = Array<number>(12).fill(0);
  for (const c of st.changes ?? []) if (c.at_ms >= now - 60 * 60_000) buckets[Math.min(11, Math.floor((c.at_ms - (now - 60 * 60_000)) / (5 * 60_000)))]!++;
  const last = (st.changes ?? []).reduce((t, c) => Math.max(t, c.at_ms), 0);
  const open = plan(st, clockOf(m)).filter((i) => i.kind !== 'done');
  const finish = open.length ? Math.max(...open.map((i) => i.end)) : 0;
  out.push('', ' ' + dim(pad('momentum', labelW)) + T.accent(spark(buckets, 12)) + dim(`  ${buckets.reduce((a, b) => a + b, 0)} in the last hour${last ? ` · last ${ago(now - last)} ago` : ''}${finish > now ? ` · plan ends ~${hhmm(finish)} (in ${ago(finish - now)})` : ''}`));
  // Files being changed: who has written what, and where two agents are on the same file.
  const files = st.files ?? [];
  if (files.length) {
    out.push('', T.rule(m.pal)('── files being changed ' + '─'.repeat(Math.max(0, w - 24))));
    for (const f of files) {
      const who = Object.entries(f.roles).map(([r, n]) => `${T.roleName(r, m.info)}×${n}`).join(' ');
      out.push('  ' + pad(T.text(m.pal)(f.path.length > 38 ? '…' + f.path.slice(-37) : f.path), 40) + dim(pad(who, 28)) + (f.clash ? colored('⚠ two agents ', T.warn) : '') + dim(hhmm(f.last_ms)));
    }
  }
  return out.slice(0, h);
}

/**
 * tools: what the team calls, busiest first, with failures; then which models report their work (the share of runs that
 * changed something and recorded it), and any agent going round in circles right now.
 */
export function toolsBoard(m: OModel, w: number, h: number): string[] {
  const st = m.st;
  const all = new Map<string, { calls: number; errors: number }>();
  let calls = 0;
  for (const t of Object.values(st.tools ?? {})) {
    calls += t.calls;
    for (const [k, c] of Object.entries(t.by_tool)) { const x = all.get(k) ?? { calls: 0, errors: 0 }; x.calls += c.calls; x.errors += c.errors; all.set(k, x); }
  }
  const out: string[] = [head(m, 'tools', `${calls} call${calls === 1 ? '' : 's'}`, w)];
  if (!calls) return [...out, ...calm('', 'no tool has been called yet', 'what the agents call, how often it fails, and who reports their work appears here')];
  const rows = [...all.entries()].sort((a, b) => b[1].calls - a[1].calls || a[0].localeCompare(b[0]));
  const max = rows[0]![1].calls, bw = Math.max(6, Math.min(24, w - 46));
  const room = Math.max(3, h - 12);
  out.push('');
  for (const [name, c] of rows.slice(0, room)) out.push('  ' + pad(T.text(m.pal)(name.slice(0, 22)), 24) + T.accent(bar(c.calls / max, bw)) + ' ' + dim(pad(String(c.calls), 5)) + (c.errors ? colored(`✗ ${c.errors}`, T.alert) : ''));
  if (rows.length > room) out.push(dim(`  … ${rows.length - room} more`));
  const ad = st.adoption;
  const models = Object.entries(ad?.by_model ?? {}).filter(([, r]) => r.worked > 0);
  out.push('', T.rule(m.pal)('── reports its work ' + '─'.repeat(Math.max(0, w - 20))), dim('  runs that changed something and recorded a change'));
  if (!models.length) out.push(dim('  no run has changed anything yet'));
  for (const [name, r] of models.sort((a, b) => b[1].worked - a[1].worked)) {
    const f = r.recorded / r.worked;
    const paint = f < 0.5 ? T.alert : f < 0.8 ? T.warn : T.ok;
    out.push('  ' + pad(T.text(m.pal)(name.slice(0, 22)), 24) + colored(bar(f, bw), paint) + ' ' + dim(`${Math.round(f * 100)}%  ${r.recorded}/${r.worked} runs`));
  }
  const looping = Object.entries(st.tools ?? {}).filter(([role, t]) => t.streak && (t.streak.n >= LOOP_CALLS || t.streak.failed >= LOOP_FAILS) && st.agents[role]?.state === 'running').map(([role, t]) => `${T.roleName(role, m.info)} ${t.streak!.tool} ×${t.streak!.n}`);
  if (looping.length) out.push('', colored('  ⟳ going round in circles: ' + looping.join(', '), T.warn));
  return out.slice(0, h);
}

/** The timeline page: the lanes first, then the tools board below it. The page scrolls (`observeFrame`), so neither is cut to the window. */
export function timelineAndTools(m: OModel, w: number, _h: number): string[] {
  return [...timeline(m, w, Infinity), '', ...toolsBoard(m, w, 40)];
}
