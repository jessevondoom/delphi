// Agents and skills, and the daemon's settings, as the window lists them (spec 111 §5.5). Pure.
import { MARK_ROLES } from './avatar.ts';
import { sourceWords, type DocRow } from './kb.ts';

/** `flag` marks a role the operator has forbidden; nothing else about a role's standing is shown (operator, 2026-11-14). */
export type AgentRow = { role: string; path: string; flag: '' | 'not allowed' };
export type SkillRow = { name: string; path: string; source: string };

const names = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

/** One row per role with a manifest under `agents/<role>/`, the six of the mark first. */
export function teamRows(docs: DocRow[], session: Record<string, unknown>): AgentRow[] {
  const byRole = new Map<string, string>();
  for (const d of docs) {
    const m = /^agents\/([^/]+)\/([^/]+)$/.exec(d.rel_path);
    if (!m) continue;
    // The manifest is `AGENT.md`; another file there is listed only when the role has no manifest.
    if (!byRole.has(m[1]!) || m[2] === 'AGENT.md') byRole.set(m[1]!, d.rel_path);
  }
  const forbidden = names(session.forbidden_agents);
  const rank = (r: string) => { const i = MARK_ROLES.indexOf(r); return i < 0 ? MARK_ROLES.length : i; };
  return [...byRole].sort((a, b) => rank(a[0]) - rank(b[0]) || a[0].localeCompare(b[0])).map(([role, path]) => ({
    role, path,
    flag: forbidden.includes(role) ? 'not allowed' : '',
  }));
}

/** One row per skill (`skills/<name>/SKILL.md`), by name. */
export function skillRows(docs: DocRow[]): SkillRow[] {
  const out: SkillRow[] = [];
  for (const d of docs) {
    const m = /^skills\/([^/]+)\/SKILL\.md$/.exec(d.rel_path);
    if (m) out.push({ name: m[1]!, path: d.rel_path, source: sourceWords(d.source_type) });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

export type Fact = { label: string; value: string };
const money = (v: unknown): string => (typeof v === 'number' && Number.isFinite(v) ? `$${v.toFixed(2)}` : '');

/** What the daemon says about itself in `session.snapshot`, as label and value pairs; a field it did not send is left out. */
export function daemonFacts(session: Record<string, unknown>): Fact[] {
  const facts: Fact[] = [];
  const add = (label: string, value: string) => { if (value) facts.push({ label, value }); };
  add('Mission', typeof session.mission_id === 'string' ? session.mission_id : '');
  add('Approvals', typeof session.mode === 'string' ? session.mode : '');
  const budget = (session.budget ?? {}) as Record<string, unknown>;
  add('Budget cap', money(budget.cap_usd));
  add('Spent', money(budget.spent_usd));
  add('Still available', money(budget.available_usd));
  if (typeof session.solo_mode === 'boolean') add('Solo mode', session.solo_mode ? 'on' : 'off');
  const running = Array.isArray(session.running_tasks) ? session.running_tasks.length : null;
  if (running !== null) add('Tasks running', String(running));
  const waits = Array.isArray(session.pending_approvals) ? session.pending_approvals.length : null;
  if (waits !== null) add('Waiting on you', String(waits));
  return facts;
}
