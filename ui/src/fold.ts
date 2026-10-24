// The event fold: daemon notifications in, the console's state out. Pure, so it is tested without a terminal.
import { sanitize, type AgentState } from './theme.ts';

export type Block = { id: number; kind: 'user' | 'reply' | 'note'; role?: string; text: string; at: number; status?: string };
export type Agent = { state: AgentState; doing: string };
export type State = {
  blocks: Block[];
  live: string;
  spans: Record<string, { role: string; depth: number }>;
  agents: Record<string, Agent>;
  tokens: number;
  approvals: Record<string, { role: string; tool: string; at: number; tier: string }>;
  alert: { role: string; until: number } | null;
  lastActivity: number;
  nextId: number;
  council: boolean;
};

export const initial = (now: number): State => ({
  blocks: [], live: '', spans: {}, agents: {}, tokens: 0, approvals: {}, alert: null, lastActivity: now, nextId: 1, council: false,
});

const ROLES = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout', 'investigator'];

export function addBlock(s: State, b: Omit<Block, 'id'>): State {
  return { ...s, blocks: [...s.blocks, { ...b, id: s.nextId }], nextId: s.nextId + 1 };
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
    case 'council.started': return { ...s, council: true, lastActivity: now };
    case 'council.barrier.released': return { ...s, council: false };
    case 'task.output': {
      const tops = Object.entries(s.spans).filter(([, v]) => v.depth === 0).sort((a, b) => spanNum(b[0]) - spanNum(a[0]));
      const role = tops[0]?.[1].role ?? 'orchestrator';
      const idle: Record<string, Agent> = {};
      for (const [r, a] of Object.entries(s.agents)) idle[r] = a.state === 'waiting on you' ? a : { state: 'idle', doing: '' };
      const escalated = pl.status === 'escalated';
      const next = addBlock({ ...s, live: '', council: false, agents: idle, lastActivity: now }, {
        kind: 'reply', role, text: sanitize(String(pl.text ?? '')), at: now, status: escalated ? 'escalated' : 'done',
      });
      return escalated ? setAgent(next, role, { state: 'escalated', doing: '' }) : next;
    }
    case 'task.failed': return addBlock({ ...s, live: '', lastActivity: now }, { kind: 'note', text: '✖ task failed: ' + sanitize(String(pl.error ?? '')), at: now });
    case 'approval.requested': {
      const id = String(pl.approval_id ?? '');
      const role = String(pl.role ?? 'orchestrator');
      const secs = Number(pl.timeout_s ?? pl.expires_in_s ?? 600);
      const next = { ...s, approvals: { ...s.approvals, [id]: { role, tool: String(pl.tool ?? ''), at: now, tier: String(pl.tier ?? '') } }, lastActivity: now };
      return setAgent({ ...next, alert: { role, until: now + secs * 1000 } }, role, { state: 'waiting on you', doing: 'decision needed' });
    }
    case 'approval.resolved': {
      const { [String(pl.approval_id)]: gone, ...left } = s.approvals;
      const ids = Object.values(left);
      const alert = ids.length ? { role: ids[0]!.role, until: s.alert?.until ?? now } : null;
      const next = { ...s, approvals: left, alert, lastActivity: now };
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
