// What each screen shows, computed from the daemon's state and events (spec 111 §3.3, §5). Pure: state in, rows out.
import { MARK_ROLES, activityOf } from './avatar.ts';
import type { Council, Envelope, OState } from './types.ts';

/** A model's family, for the roster's seat label: the provider the daemon names, else what the model id says. */
export function familyOf(state: OState, endpoint: string, model: string): string {
  const p = state.providers?.find((x) => x.endpoint === endpoint);
  if (p?.family) return p.family;
  const m = model.toLowerCase();
  if (m.startsWith('claude')) return 'claude';
  if (m.startsWith('gpt') || m.startsWith('o1') || m.startsWith('o3') || m.startsWith('codex')) return 'gpt';
  if (m.startsWith('gemini')) return 'gemini';
  return '';
}

export type RosterRow = { role: string; activity: string; doing: string; family: string; waiting: boolean };

/** The agents of the mission: the six of the mark first, in its order, then the rest by name. */
export function roster(state: OState): RosterRow[] {
  const rank = (r: string) => { const i = MARK_ROLES.indexOf(r); return i < 0 ? MARK_ROLES.length : i; };
  return Object.values(state.agents)
    .sort((a, b) => rank(a.role) - rank(b.role) || a.role.localeCompare(b.role))
    .map((a) => {
      const task = state.tasks[a.task_id];
      const waiting = a.state === 'waiting on you';
      const doing = waiting ? 'Waiting on you' : a.state === 'running' ? task?.prompt || 'Working' : a.state === 'idle' ? 'Idle' : a.state;
      return { role: a.role, activity: activityOf(a.state), doing, family: familyOf(state, a.endpoint, a.model), waiting };
    });
}

export type WaitingRow = { id: string; role: string; summary: string; since_ms: number };
/** What waits on the operator, oldest first. */
export function waiting(state: OState): WaitingRow[] {
  return Object.entries(state.approvals ?? {}).map(([id, w]) => ({ id, ...w })).sort((a, b) => a.since_ms - b.since_ms);
}

export type Entry = {
  key: string; at_ms: number; role: string; slot?: string;
  /** `said` an agent's words; `asked` what a task was given; `read` a tool call; `recorded` a change; `waiting`/`answered` an approval; `council` a council's step. */
  kind: 'said' | 'asked' | 'read' | 'recorded' | 'waiting' | 'answered' | 'council';
  text: string; code?: string; task?: string;
  /** For `asked`: who was asked. */
  to?: string;
  /** A seat's words in a council's blind round: the entry says the seat answered, and `text` is empty. */
  sealed?: boolean;
};

const str = (v: unknown): string => (typeof v === 'string' ? v : '');
const clip = (s: string, n: number): string => (s.length > n ? s.slice(0, n - 1) + '…' : s);

/** Which role each task was given to: what its `task.created` named, else the flow the state records for it. */
export function taskRoles(events: Envelope[], state: OState | null): Map<string, string> {
  const m = new Map<string, string>();
  for (const [id, t] of Object.entries(state?.tasks ?? {})) if (t.flow) m.set(id, t.flow);
  for (const e of events) if (e.type === 'task.created' && e.task_id && str(e.payload?.target)) m.set(e.task_id, str(e.payload.target));
  return m;
}

/**
 * One stream entry for an event, or nothing for a kind the stream leaves out (usage, budget, graph bookkeeping).
 * The daemon does not name the speaker on a task's output, so `roleOfTask` says who the task was given to.
 */
export function entryOf(e: Envelope, i: number, roleOfTask: (task?: string) => string | undefined = () => undefined): Entry | null {
  const p = e.payload ?? {};
  const base = { key: `${e.seq ?? e.timestamp}-${i}`, at_ms: Date.parse(e.timestamp) || 0, role: str(p.role) || e.agent || roleOfTask(e.task_id) || 'orchestrator', slot: e.pool_slot, task: e.task_id };
  switch (e.type) {
    case 'task.created': return { ...base, role: str(p.source) || e.agent || 'operator', kind: 'asked', text: str(p.prompt), to: str(p.target) || undefined };
    case 'task.output': return str(p.text) ? { ...base, kind: 'said', text: str(p.text) } : null;
    case 'task.failed': return { ...base, kind: 'said', text: `Failed: ${str(p.error) || 'no reason given'}` };
    case 'change.recorded': {
      const files = Array.isArray(p.files) ? p.files.filter((f): f is string => typeof f === 'string') : [];
      return { ...base, kind: 'recorded', text: str(p.summary), code: files.length ? files.join('  ') : undefined };
    }
    case 'approval.requested': return { ...base, kind: 'waiting', text: str(p.summary) || 'Asked for your approval.' };
    case 'approval.resolved': return { ...base, kind: 'answered', text: `Approval ${str(p.resolution) || 'answered'}${str(p.by) ? ` by ${str(p.by)}` : ''}.` };
    case 'agent.block.complete': {
      const b = (p.block ?? {}) as Record<string, unknown>;
      if (b.type !== 'tool_call') return null;
      const args = typeof b.arguments === 'string' ? b.arguments : JSON.stringify(b.arguments ?? {});
      return { ...base, kind: 'read', text: '', code: clip(`${str(b.name)} ${args}`, 240) };
    }
    case 'council.started': return { ...base, role: 'orchestrator', kind: 'council', text: `Council opened: ${str(p.proposition)}` };
    case 'council.barrier.released': return { ...base, role: 'orchestrator', kind: 'council', text: 'Every seat has answered. The barrier is released.' };
    case 'council.verdict': return { ...base, role: 'orchestrator', kind: 'council', text: 'The council has a verdict.' };
    default: return null;
  }
}

/**
 * The activity stream, newest first: the events worth reading, plus recorded changes the events no longer hold.
 * `isRole` says whether a name is a role the window can draw; a task given to something else (a flow such as `build`) is the
 * orchestrator's to speak for.
 */
export function stream(events: Envelope[], state: OState | null, limit = 200, isRole: (r: string) => boolean = () => true): Entry[] {
  const roles = taskRoles(events, state);
  const roleOfTask = (task?: string) => { const r = task ? roles.get(task) : undefined; return r && isRole(r) ? r : undefined; };
  const out = events.map((e, i) => entryOf(e, i, roleOfTask)).filter((x): x is Entry => x !== null);
  const seen = new Set(out.filter((x) => x.kind === 'recorded').map((x) => `${x.role}|${x.text}`));
  for (const [i, c] of (state?.changes ?? []).entries()) {
    if (seen.has(`${c.role}|${c.summary}`)) continue;
    out.push({ key: `change-${c.at_ms}-${i}`, at_ms: c.at_ms, role: c.role, kind: 'recorded', text: c.summary, code: c.files.length ? c.files.join('  ') : undefined, task: c.task });
  }
  return out.sort((a, b) => b.at_ms - a.at_ms).slice(0, limit);
}

/** A path with the home folder written as `~`, for a row that has little room. */
export const tilde = (path: string): string => path.replace(/^\/(?:Users|home)\/[^/]+/, '~');

/** `14:02`, in the operator's time zone unless one is given. */
export function clock(ms: number, timeZone?: string): string {
  if (!ms) return '';
  return new Intl.DateTimeFormat('en-GB', { hour: '2-digit', minute: '2-digit', hour12: false, timeZone }).format(new Date(ms));
}

// ---- Council (spec 111 §5.2) ----

export const STAGES = ['Framed', 'Round 1 · blind', 'Barrier released', 'Round 2 · critique', 'Verdict'];
/** Which stage a council is at, as an index into [`STAGES`]. */
export const stageOf = (c: Council): number => (c.phase === 'round 1' ? 1 : c.phase === 'round 2' ? 3 : 4);
export const isBlind = (c: Council): boolean => c.phase === 'round 1';
export const isDecided = (c: Council): boolean => c.phase !== 'round 1' && c.phase !== 'round 2';
export const phaseNote = (c: Council): string => (c.phase === 'round 1' ? 'live · round 1, blind' : c.phase === 'round 2' ? 'live · round 2' : c.phase === 'escalated' ? 'needs your decision' : 'verdict in');

export type SeatView = {
  role: string; family: string; lens: string;
  /** `writing`: nothing in yet; `sealed`: answered, words hidden while the round is blind; `shown`: the words. */
  position: { state: 'writing' | 'sealed' | 'shown'; text: string };
  critique: { recommendation: string; concur: boolean | null; findings: [string, string][]; text: string } | null;
};

/** One column per seat. In the blind round a seat's words are not in what this returns, so no caller can show them. */
export function seatsOf(c: Council): SeatView[] {
  const blind = isBlind(c);
  return c.seats.map((seat) => {
    const stance = c.stances.find((x) => x.role === seat.role);
    const cr = c.critiques.find((x) => x.role === seat.role);
    return {
      role: seat.role, family: seat.family, lens: c.lenses.find((l) => l[0] === seat.role)?.[1] ?? '',
      position: !stance?.answered ? { state: 'writing', text: '' } : blind ? { state: 'sealed', text: '' } : { state: 'shown', text: stance.text },
      critique: cr && !blind ? { recommendation: cr.recommendation, concur: cr.concur, findings: cr.findings, text: cr.text } : null,
    };
  });
}

/** A council's own activity, newest first: its steps, what its seats said and read. Blind-round words are sealed. */
export function councilEntries(events: Envelope[], c: Council): Entry[] {
  const blind = isBlind(c);
  const out: Entry[] = [];
  events.forEach((e, i) => {
    if (e.task_id !== c.task_id) return;
    const p = e.payload ?? {};
    const base = { key: `${e.seq ?? e.timestamp}-${i}`, at_ms: Date.parse(e.timestamp) || 0, role: str(p.role) || e.agent || 'orchestrator', slot: e.pool_slot, task: e.task_id };
    switch (e.type) {
      case 'council.started': out.push({ ...base, role: 'orchestrator', kind: 'council', text: 'Framed the question and seated the council.' }); break;
      case 'council.lens_assigned': out.push({ ...base, role: 'orchestrator', kind: 'council', text: str(p.note) || 'Gave each seat a lens.' }); break;
      case 'council.stance':
        if (p.status === 'answered') out.push(blind ? { ...base, kind: 'said', text: '', sealed: true } : { ...base, kind: 'said', text: `Position: ${str(p.text)}` });
        break;
      case 'council.barrier.released': out.push({ ...base, role: 'orchestrator', kind: 'council', text: 'Every seat has answered. The barrier is released; each seat now critiques another.' }); break;
      // No critique exists before the barrier; if one arrives while the phase still says blind, it is sealed like a stance.
      case 'council.critique': out.push(blind ? { ...base, kind: 'said', text: '', sealed: true } : { ...base, kind: 'said', text: `Critique${str(p.recommendation) ? `, recommending ${str(p.recommendation)}` : ''}: ${str(p.text)}` }); break;
      case 'council.verdict': out.push({ ...base, role: 'orchestrator', kind: 'council', text: str(p.escalated) ? 'The seats did not settle it. The decision is yours.' : 'Verdict framed.' }); break;
      case 'agent.block.complete': { const t = entryOf(e, i); if (t) out.push(t); break; }
      default: break;
    }
  });
  return out.sort((a, b) => b.at_ms - a.at_ms);
}

/** The council to show first: one still running, else the newest. */
export function defaultCouncil(state: OState): number {
  const live = state.councils.findIndex((c) => !isDecided(c));
  return live >= 0 ? live : state.councils.length - 1;
}
