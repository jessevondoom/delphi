// The dashboard view of the narrator (`specs/030` §6.5): the operational picture of the mission at a glance. Sessions
// as chips, everything running now grouped by the session that asked for it (work from an earlier session stays on
// the screen while it runs), what needs the operator, the pace, and the latest happenings filling the rest of the
// space. Pure: the daemon's folded state in, rows out.
import chalk from 'chalk';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';
import type { OModel, OState } from './observe-screen.ts';

type Task = OState['tasks'][string];
const taskNum = (id: string) => Number(id.replace(/\D/g, '')) || 0;
type Node = OState['nodes'][string];
type Paint = (s: string) => string;

const dim = (s: string) => T.dim(s);
const clean = (s: string, n: number) => { const t = sanitize(s).replace(/\s+/g, ' ').trim(); return [...t].length <= n ? t : [...t].slice(0, Math.max(0, n - 1)).join('') + '…'; };
const rule = (m: OModel, w: number, title: string) => { const head = `── ${title} `; return T.rule(m.pal)(head + '─'.repeat(Math.max(0, w - [...head].length))); };
const mmss = (ms: number) => { const s = Math.max(0, Math.round(ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
const kilo = (n: number) => (n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(Math.round(n)));
const spark = (v: number[], w: number) => { const r = v.slice(-w), max = Math.max(0, ...r); return max <= 0 ? '▁'.repeat(r.length) : r.map((x) => '▁▂▃▄▅▆▇█'[Math.round((x / max) * 7)]).join(''); };
const num = (id: string) => Number(id.replace(/\D/g, '')) || id;

/** What needs the operator, from the daemon's own lists, one line each. */
export function needs(st: OState): string[] {
  const items: string[] = [];
  for (const summary of Object.values(st.pending_approvals ?? {})) items.push(clean(summary, 80));
  for (const c of st.councils) if (c.escalation && c.phase !== 'round 1' && c.phase !== 'round 2') items.push(`council ${c.task_id} needs a decision`);
  if (st.pending_proposals > 0) items.push(`${st.pending_proposals} proposal(s) await /ratify`);
  return items;
}

/** The mission's work grouped by session: the current one first, then earlier ones that still have something running. */
export type Group = { id: string; label: string; current: boolean; asks: number; running: Task[]; finished: number };
export function groups(st: OState): Group[] {
  const tasks = Object.values(st.tasks);
  const sessions = st.sessions ?? [];
  if (!sessions.length) {
    return [{ id: '', label: 'this mission', current: true, asks: tasks.filter((t) => t.flow !== 'build').length, running: tasks.filter((t) => t.status === 'running'), finished: tasks.filter((t) => t.status !== 'running').length }];
  }
  const out: Group[] = sessions.map((s, i) => {
    const mine = tasks.filter((t) => (t.session ?? '') === s.id);
    return { id: s.id, label: `session ${num(s.id)}`, current: i === sessions.length - 1, asks: s.turns, running: mine.filter((t) => t.status === 'running'), finished: mine.filter((t) => t.status !== 'running').length };
  });
  const before = tasks.filter((t) => !t.session);
  if (before.some((t) => t.status === 'running')) out.unshift({ id: '', label: 'before sessions', current: false, asks: 0, running: before.filter((t) => t.status === 'running'), finished: before.filter((t) => t.status !== 'running').length });
  return out;
}

function chips(m: OModel, w: number, only = false): string[] {
  const gs = only ? groups(m.st).filter((g) => g.current) : groups(m.st);
  const shown = gs.filter((g) => g.current || g.running.length).concat(gs.filter((g) => !g.current && !g.running.length).slice(-3));
  const uniq = [...new Map(shown.map((g) => [g.id + g.label, g])).values()].sort((a, b) => gs.indexOf(a) - gs.indexOf(b));
  const parts = uniq.map((g) => {
    const body = `${g.label} · ${g.asks} ask${g.asks === 1 ? '' : 's'}${g.running.length ? ` · ${g.running.length} running` : ''}`;
    return g.current ? T.accent('● ') + chalk.bold(body) : dim('○ ' + body);
  });
  const out: string[] = [];
  let row = ' ';
  for (const p of parts) {
    if (row.trim() && T.width(row) + T.width(p) + 3 > w) { out.push(row); row = ' '; }
    row += (row.trim() ? dim('   ') : '') + p;
  }
  if (row.trim()) out.push(row);
  return out;
}

function taskRow(m: OModel, t: Task, w: number): string {
  const nodes = Object.values(m.st.nodes).filter((n: Node) => n.task_id === t.id);
  const live = nodes.filter((n) => n.state === 'running');
  const top = nodes.find((n) => n.depth === 0) ?? live[0];
  const role = top?.role ?? t.flow;
  const waiting = live.map((n) => n.waiting ?? '').find((x) => x);
  const starts = nodes.map((n) => n.started_ms).filter((x) => x > 0);
  const el = m.now && starts.length ? mmss(m.now - Math.min(...starts)) : '';
  const who = T.roleName(role, m.info) + (live.length > 1 ? dim(` +${live.length - 1}`) : '');
  const tag = t.flow === 'build' || t.flow === 'council' ? dim(`${t.flow} `) : '';
  const state = waiting ? T.warn(clean(waiting, Math.max(24, Math.min(60, Math.floor(w * 0.4))))) : T.accent('working');
  const head = ` ${waiting ? T.warn('►') : T.accent('●')} ${chalk.bold(who)}  ${dim(t.id)}  ${dim(el.padStart(5))}  ${state}  `;
  const room = Math.max(0, w - T.width(head) - 1);
  return T.clip(head + tag + (room > 8 ? dim(clean(t.prompt, Math.max(0, room - T.width(tag)))) : ''), w);
}

function running(m: OModel, w: number): string[] {
  const gs = groups(m.st).filter((g) => g.running.length);
  if (!gs.length) return [' ' + T.ok('✔') + dim(' nothing is running: the system is idle')];
  const many = gs.length > 1 || !gs[0]!.current;
  const out: string[] = [];
  // The current session first, then the earlier ones that still have work running.
  for (const g of [...gs].sort((a, b) => Number(b.current) - Number(a.current))) {
    if (many) out.push(' ' + (g.current ? chalk.bold(g.label) + dim(' (current)') : dim(g.label + ' (earlier, still running)')));
    for (const t of g.running) out.push(taskRow(m, t, w));
  }
  return out;
}

const eta = (ms: number) => { const mins = Math.max(1, Math.round(ms / 60_000)); return mins >= 60 ? `${Math.floor(mins / 60)}h ${String(mins % 60).padStart(2, '0')}m` : `${mins}m`; };

/** Which models are out and until when, and which tasks run on a stand-in and who agreed (`specs/035` D-7). Empty when all is well. */
export function failover(m: OModel, w: number): string[] {
  const now = m.now ?? 0;
  const out = (m.st.models_out ?? []).filter((x) => !x.back_ms || !now || x.back_ms > now);
  const rows: string[] = [];
  for (const x of out) {
    const back = x.back_ms && now ? ` · back in ${eta(x.back_ms - now)}` : '';
    rows.push(' ' + T.alert('✖') + ' ' + chalk.bold(`${x.endpoint_id}/${x.model_id}`) + dim(`  ${x.reason}${back}`));
  }
  const who = { operator: 'you agreed', policy: 'to finish the task, as your policy allows', automatic: 'an equal model, no question asked' } as const;
  for (const s of m.st.standins ?? []) {
    rows.push(' ' + T.warn('⇄') + ` ${T.roleName(s.role || 'orchestrator', m.info)} ${dim(`(${s.task})`)} on ${chalk.bold(s.to)} instead of ${s.from}` + dim(` · ${who[s.by] ?? s.by}`));
  }
  return rows.map((r) => T.clip(r, w));
}

function pace(m: OModel, w: number): string[] {
  const st = m.st;
  const per = st.pace.length ? st.pace[st.pace.length - 1]! : 0;
  const active = Object.values(st.agents).filter((a) => a.state === 'running').length;
  const total = st.tokens.input + st.tokens.cache_read + st.tokens.cache_write + st.tokens.output + st.tokens.thinking;
  const cells = [
    `${spark(st.pace, 12)} ${kilo(per)} tokens/min`,
    `${kilo(total)} tokens`,
    `cache ${Math.round((st.cache_hit_ratio || 0) * 100)}%`,
    `${st.retries} retr${st.retries === 1 ? 'y' : 'ies'}`,
    `${active} agent${active === 1 ? '' : 's'} working`,
  ];
  const out: string[] = [];
  let row = ' ';
  for (const c of cells) {
    if (row.trim() && T.width(row) + c.length + 5 > w) { out.push(dim(row)); row = ' '; }
    row += (row.trim() ? '  ·  ' : '') + c;
  }
  if (row.trim()) out.push(dim(row));
  return out;
}

/** The tasks that ended most recently, newest first: how each ended, which session asked, and the first line of what it said. */
function finished(m: OModel, w: number, limit: number): string[] {
  const done = Object.values(m.st.tasks).filter((t) => t.status !== 'running').sort((a, b) => taskNum(b.id) - taskNum(a.id)).slice(0, limit);
  const sessions = m.st.sessions ?? [];
  return done.map((t) => {
    const mark = t.status === 'done' ? T.ok('✔') : T.alert('✖');
    const where = t.session && sessions.length > 1 ? dim(`s${num(t.session)}  `) : '';
    const said = clean((t.output ?? '').split('\n').find((l) => l.trim()) ?? '', 200);
    const head = ` ${mark} ${dim(t.id)}  ${where}`;
    const room = Math.max(0, w - T.width(head) - 2);
    const ask = clean(t.prompt, Math.min(room, 36));
    return T.clip(head + ask + (said && room - T.width(ask) > 12 ? dim('  →  ' + clean(said, room - T.width(ask) - 5)) : ''), w);
  });
}

/** The dashboard: sections in order of importance; finished work and the latest happenings take whatever rows are left. */
export function dashboard(m: OModel, w: number, h: number): string[] {
  const out: string[] = [];
  // A short window keeps what is happening first and the current session as one chip; a tall one has room for all of it.
  const short = h < 16;
  const sections: [string, string[]][] = [];
  const items = needs(m.st);
  const ask: [string, string[]] = ['needs you', items.slice(0, 4).map((i) => ' ' + T.warn('►') + ' ' + clean(i, w - 4)).concat(items.length > 4 ? [dim(`   +${items.length - 4} more`)] : [])];
  const models = failover(m, w);
  if (short) sections.push(['running now', running(m, w)], ['session', chips(m, w, true)]);
  else sections.push(['sessions', chips(m, w)], ['running now', running(m, w)]);
  if (items.length) sections.push(ask);
  if (models.length) sections.push(['models', models]);
  if (!short) sections.push(['pace', pace(m, w)]);
  for (const [title, rows] of sections) {
    if (out.length && !short) out.push('');
    out.push(rule(m, w, title), ...rows);
  }
  let left = h - out.length - 2;
  if (left >= 3) {
    const rows = finished(m, w, Math.min(6, Math.max(1, Math.floor(left / 2)) - 1));
    if (rows.length) { out.push('', rule(m, w, 'finished lately'), ...rows); left -= rows.length + 2; }
  }
  if (left >= 3) {
    const feed = m.st.narrative.slice(-(left - 1)).map((l) => ' ' + dim(clean(l, w - 3)));
    if (feed.length) out.push('', rule(m, w, 'recent'), ...feed);
  }
  return out.slice(0, h);
}
