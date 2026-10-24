// The event fold: daemon notifications in, the console's state out. Pure, so it is tested without a terminal.
import { sanitize, type AgentState } from './theme.ts';

export type Block = { id: number; kind: 'user' | 'reply' | 'note' | 'card' | 'tool'; role?: string; text: string; at: number; status?: string; ref?: string; detail?: string };
export type Agent = { state: AgentState; doing: string };
export type Approval = { id: string; role: string; tool: string; tier: string; summary: string; preview: string; blake3: string; payload: unknown; at: number; timeoutS: number };
export type State = {
  blocks: Block[];
  live: string;
  spans: Record<string, { role: string; depth: number }>;
  agents: Record<string, Agent>;
  tokens: number;
  approvals: Record<string, Approval>;
  alert: { role: string; until: number } | null;
  lastActivity: number;
  nextId: number;
  council: boolean;
  /** Tasks the daemon is working on, by task id. */
  running: Record<string, { since: number }>;
};

export const initial = (now: number): State => ({
  blocks: [], live: '', spans: {}, agents: {}, tokens: 0, approvals: {}, alert: null, lastActivity: now, nextId: 1, council: false, running: {},
});

const ROLES = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout', 'investigator'];

export function addBlock(s: State, b: Omit<Block, 'id'>): State {
  return { ...s, blocks: [...s.blocks, { ...b, id: s.nextId }], nextId: s.nextId + 1 };
}

const without = (r: Record<string, { since: number }>, id?: string) => { if (!id) return r; const { [id]: _gone, ...rest } = r; return rest; };

/** One short line for a tool call's arguments: the path or pattern if there is one, else the first string. */
export function briefArgs(a: any): string {
  const pick = ['path', 'pattern', 'role'].map((k) => a?.[k]).find((v) => typeof v === 'string') ?? Object.values(a ?? {}).find((v) => typeof v === 'string') ?? '';
  const t = sanitize(String(pick)).replace(/\s+/g, ' ');
  return t.length > 60 ? t.slice(0, 59) + '…' : t;
}

/** The event's own time when it has one (history), else the time it arrived. */
const stamp = (e: any, now: number) => { const t = Date.parse(e.timestamp ?? ''); return Number.isNaN(t) ? now : t; };

/** Past events into blocks, for reopening a mission: what was said and done, not what was pending. */
export function replay(s: State, events: any[]): State {
  const KEEP = new Set(['task.created', 'task.output', 'task.failed', 'task.cancelled', 'dag.node.added', 'council.started', 'council.barrier.released']);
  const done = events.filter((e) => KEEP.has(e.type)).reduce((a, e) => fold(a, { method: 'event', params: e }, stamp(e, Date.now())), s);
  return { ...done, running: {}, agents: {} }; // what is running now comes from the daemon's snapshot, not from old events
}

const setAgent = (s: State, role: string, a: Agent): State => ({ ...s, agents: { ...s.agents, [role]: a } });
const spanNum = (k: string) => Number(k.split('_').pop()) || 0;

export function fold(s: State, n: { method: string; params?: any }, now: number): State {
  const p = n.params ?? {};
  if (n.method === 'delta.frame') {
    const spans = (p.spans ?? {}) as Record<string, { text?: string }>;
    let add = '';
    for (const k of Object.keys(spans).sort()) {
      if (s.council && s.spans[k]) continue; // a council's round one stays blind
      add += spans[k]?.text ?? '';
    }
    if (!add) return s;
    return { ...s, live: (s.live + sanitize(add)).slice(-8000), tokens: s.tokens + Math.ceil(add.length / 4), lastActivity: now };
  }
  if (n.method !== 'event') return s;
  const e = p, pl = e.payload ?? {};
  switch (e.type) {
    case 'dag.node.added': {
      const role = String(pl.role ?? '?');
      const next = { ...s, spans: { ...s.spans, [String(pl.node_id)]: { role, depth: Number(pl.depth ?? 0) } }, lastActivity: now };
      return setAgent(next, role, { state: 'running', doing: 'working' });
    }
    case 'task.created': {
      const run = e.task_id ? { ...s.running, [e.task_id]: { since: now } } : s.running;
      if (pl.flow === 'build' && !pl.prompt) return { ...s, running: run };
      return addBlock({ ...s, running: run, lastActivity: now }, { kind: 'user', text: sanitize(String(pl.prompt ?? '')), at: stamp(e, now) });
    }
    case 'council.started': return { ...s, council: true, lastActivity: now };
    case 'council.barrier.released': return { ...s, council: false };
    case 'task.output': {
      const tops = Object.entries(s.spans).filter(([, v]) => v.depth === 0).sort((a, b) => spanNum(b[0]) - spanNum(a[0]));
      const role = tops[0]?.[1].role ?? 'orchestrator';
      const idle: Record<string, Agent> = {};
      for (const [r, a] of Object.entries(s.agents)) idle[r] = a.state === 'waiting on you' ? a : { state: 'idle', doing: '' };
      const escalated = pl.status === 'escalated';
      const next = addBlock({ ...s, live: '', council: false, agents: idle, running: without(s.running, e.task_id), lastActivity: now }, {
        kind: 'reply', role, text: sanitize(String(pl.text ?? '')), at: stamp(e, now), status: escalated ? 'escalated' : 'done',
      });
      return escalated ? setAgent(next, role, { state: 'escalated', doing: '' }) : next;
    }
    case 'task.cancelled': {
      const idle: Record<string, Agent> = {};
      for (const r of Object.keys(s.agents)) idle[r] = { state: 'idle', doing: '' };
      return addBlock({ ...s, live: '', council: false, agents: idle, running: without(s.running, e.task_id), alert: null, lastActivity: now }, { kind: 'note', text: '■ stopped', at: stamp(e, now) });
    }
    case 'agent.block.complete': {
      const b = pl.block;
      if (b?.type !== 'tool_call') return s;
      return addBlock(s, { kind: 'tool', role: e.agent, text: `${b.name}(${briefArgs(b.arguments)})`, at: now, status: 'pending', ref: String(b.id ?? '') });
    }
    case 'tool.result': {
      const ref = String(pl.tool_call_id ?? '');
      const detail = sanitize(String(pl.content ?? '')).split('\n').slice(0, 40).join('\n');
      return { ...s, blocks: s.blocks.map((x) => (x.kind === 'tool' && x.ref === ref ? { ...x, status: pl.is_error ? 'error' : 'ok', detail } : x)) };
    }
    case 'task.failed': return addBlock({ ...s, live: '', running: without(s.running, e.task_id), lastActivity: now }, { kind: 'note', text: '✖ task failed: ' + sanitize(String(pl.error ?? '')), at: now });
    case 'approval.requested': {
      const id = String(pl.approval_id ?? '');
      const role = String(pl.role ?? 'orchestrator');
      const secs = Number(pl.timeout_secs ?? pl.timeout_s ?? pl.expires_in_s ?? 600);
      const card: Approval = { id, role, tool: String(pl.tool ?? ''), tier: String(pl.tier ?? 'amber'), summary: sanitize(String(pl.summary ?? '')), preview: sanitize(String(pl.payload?.preview ?? '')), blake3: String(pl.payload_blake3 ?? ''), payload: pl.payload, at: now, timeoutS: secs };
      const next = { ...s, approvals: { ...s.approvals, [id]: card }, lastActivity: now };
      const shown = addBlock(next, { kind: 'card', role, text: [card.summary, ...card.preview.split('\n').slice(0, 6).map((l) => '  ' + l.slice(0, 100))].filter(Boolean).join('\n'), at: now, status: card.tier, ref: id });
      return setAgent({ ...shown, alert: { role, until: now + secs * 1000 } }, role, { state: 'waiting on you', doing: 'decision needed' });
    }
    case 'approval.resolved': {
      const { [String(pl.approval_id)]: gone, ...left } = s.approvals;
      const ids = Object.values(left);
      const alert = ids.length ? { role: ids[0]!.role, until: s.alert?.until ?? now } : null;
      const said = addBlock({ ...s, approvals: left, alert, lastActivity: now }, { kind: 'note', text: `approval ${pl.approval_id}: ${pl.resolution ?? 'resolved'}${pl.by ? ` (by ${pl.by})` : ''}`, at: now });
      const next = said;
      return gone && !ids.some((a) => a.role === gone.role) ? setAgent(next, gone.role, { state: 'running', doing: 'working' }) : next;
    }
    default: return s;
  }
}

/** What line one of the header shows. */
export type Status =
  | { kind: 'alert'; role: string; secs: number }
  | { kind: 'active'; role: string; doing: string }
  | { kind: 'strip' };

export function status(s: State, now: number): Status {
  if (s.alert) return { kind: 'alert', role: s.alert.role, secs: Math.max(0, Math.round((s.alert.until - now) / 1000)) };
  const running = Object.entries(s.agents).filter(([, a]) => a.state === 'running');
  if (running.length && now - s.lastActivity < 30_000) {
    const [role, a] = running.sort((x, y) => ROLES.indexOf(x[0]) - ROLES.indexOf(y[0]))[running.length > 1 ? running.length - 1 : 0]!;
    return { kind: 'active', role, doing: a.doing };
  }
  return { kind: 'strip' };
}
