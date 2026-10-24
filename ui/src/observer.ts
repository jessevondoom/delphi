// The Observer's view model, ported from crates/delphi-engine/src/observe.rs: a fold over event envelopes. The daemon
// serves recent history (`observe.snapshot`), the console applies live events on top with the same `apply`, so a
// reopened console rebuilds exactly what it had. Reads only; nothing here starts work or approves anything.
// Text from models and tools is shown, never obeyed: it goes through `sanitize` before it can reach a terminal.
import { sanitize } from './theme.ts';

const NARRATIVE_CAP = 300;
const TEXT_CAP = 6000;

export type Tokens = { input: number; cacheRead: number; cacheWrite: number; output: number; thinking: number };
export const noTokens = (): Tokens => ({ input: 0, cacheRead: 0, cacheWrite: 0, output: 0, thinking: 0 });
export const totalTokens = (t: Tokens) => t.input + t.cacheRead + t.cacheWrite + t.output + t.thinking;
const addTokens = (t: Tokens, p: any) => {
  const n = (k: string) => Number(p?.[k]) || 0;
  t.input += n('input_tokens'); t.cacheRead += n('cache_read_tokens'); t.cacheWrite += n('cache_write_tokens'); t.output += n('output_tokens'); t.thinking += n('thinking_tokens');
};

export type AgentRow = { role: string; state: string; taskId: string; endpoint: string; tier: string; model: string; upscaled: boolean; tokens: Tokens; costUsd: number; calls: number };
export type Node = { id: string; role: string; parent: string | null; depth: number; taskId: string; state: string; tokens: number; costUsd: number };
export type Task = { id: string; flow: string; prompt: string; status: string; output: string };
export type Seat = { role: string; family: string };
export type Stance = { role: string; answered: boolean; text: string };
export type Critique = { role: string; recommendation: string; concur: boolean | null; findings: [string, string][]; text: string };
export type Council = {
  taskId: string; proposition: string; seats: Seat[]; homogeneous: boolean; phase: string; stances: Stance[]; critiques: Critique[];
  escalation: string | null; path: string; unresolved: [string, string, string][]; options: [string, string[]][]; synthesis: string;
};
export type GateRow = { name: string; state: string; detail: string };
export type Build = { taskId: string; branch: string; attempts: string[]; sha: string; gates: GateRow[]; findings: [string, string][]; flaky: boolean; merge: string };

export type ConnectorTool = { name: string; class: string; pinned: boolean };
/** A configured connector (from the snapshot) and what its events say about it. `idle` until it has been used. */
export type ConnectorRow = {
  name: string; url: string; account: string | null; login: string | null; roles: string[]; tools: ConnectorTool[];
  calls: number; results: number; errors: number; denied: number; lastTool: string; lastAt: number;
  state: 'idle' | 'ready' | 'needs login' | 'unavailable'; reason: string;
};

const str = (v: unknown): string => (typeof v === 'string' ? v : '');
const cap = (v: unknown, n: number) => sanitize(str(v)).slice(0, n);
const sevTitle = (a: unknown): [string, string][] => (Array.isArray(a) ? a.map((f: any) => [str(f?.severity), sanitize(str(f?.title))] as [string, string]) : []);
const RANK: Record<string, number> = { fast: 0, workhorse: 1, frontier: 2 };

export class Observer {
  lastSeq = 0;
  agents = new Map<string, AgentRow>();
  nodes = new Map<string, Node>();
  tasks = new Map<string, Task>();
  councils: Council[] = [];
  builds: Build[] = [];
  pendingApprovals = new Map<string, string>();
  spentUsd = 0;
  tokens = noTokens();
  retries = 0;
  narrative: string[] = [];
  /** Cost of each model call, oldest first (the spend sparkline); the newest 240. */
  series: number[] = [];
  /** Live text per span while it streams (cleared when the node finishes). Memory only: never written anywhere. */
  live = new Map<string, string>();
  connectors = new Map<string, ConnectorRow>();

  constructor(roles: string[] = []) {
    for (const r of roles) this.agents.set(r, { role: r, state: 'idle', taskId: '', endpoint: '', tier: '', model: '', upscaled: false, tokens: noTokens(), costUsd: 0, calls: 0 });
  }

  /** The configured connectors, from `observe.snapshot`. Counters already gathered from events are kept. */
  setConnectors(list: unknown) {
    if (!Array.isArray(list)) return;
    for (const c of list as any[]) {
      const name = str(c?.name);
      if (!name) continue;
      const row = this.connector(name);
      row.url = sanitize(str(c.url)); row.account = typeof c.account === 'string' ? c.account : null; row.login = typeof c.login === 'string' ? c.login : null;
      row.roles = Array.isArray(c.roles) ? c.roles.map(str) : [];
      row.tools = Array.isArray(c.tools) ? c.tools.map((t: any) => ({ name: str(t?.name), class: str(t?.class), pinned: t?.pinned === true })) : [];
    }
  }
  private connector(name: string): ConnectorRow {
    let r = this.connectors.get(name);
    if (!r) { r = { name, url: '', account: null, login: null, roles: [], tools: [], calls: 0, results: 0, errors: 0, denied: 0, lastTool: '', lastAt: 0, state: 'idle', reason: '' }; this.connectors.set(name, r); }
    return r;
  }

  private say(line: string) {
    this.narrative.push(line);
    if (this.narrative.length > NARRATIVE_CAP) this.narrative.shift();
  }
  private council(task: string): Council | undefined { return [...this.councils].reverse().find((c) => c.taskId === task); }
  private build(task: string): Build {
    const found = [...this.builds].reverse().find((b) => b.taskId === task);
    if (found) return found;
    const b: Build = { taskId: task, branch: '', attempts: [], sha: '', gates: [], findings: [], flaky: false, merge: '' };
    this.builds.push(b);
    return b;
  }

  /** Text streaming from a span (a `delta.frame`): shown live, bounded. */
  applyFrame(params: any) {
    const spans = params?.spans;
    if (!spans || typeof spans !== 'object') return;
    for (const [id, v] of Object.entries<any>(spans)) {
      const t = str(v?.text);
      if (!t) continue;
      this.live.set(id, (this.live.get(id) ?? '') + sanitize(t));
      const buf = this.live.get(id)!;
      if (buf.length > TEXT_CAP) this.live.set(id, buf.slice(buf.length - TEXT_CAP));
    }
  }

  /** Fold one event envelope. Events at or below `lastSeq` are ignored, so a snapshot and the live stream can overlap. */
  apply(e: any) {
    const seq = Number(e?.seq) || 0;
    if (seq !== 0) {
      if (seq <= this.lastSeq) return;
      this.lastSeq = seq;
    }
    const kind = str(e?.type), p = e?.payload ?? {}, task = str(e?.task_id), role = str(e?.agent);
    switch (kind) {
      case 'task.created': {
        const flow = str(p.flow) || str(p.target) || 'task';
        const prompt = cap(p.prompt, 300);
        this.say(`${task} started (${flow}): ${prompt.slice(0, 80)}`);
        this.tasks.set(task, { id: task, flow, prompt, status: 'running', output: '' });
        break;
      }
      case 'task.output': {
        const t = this.tasks.get(task);
        if (t) t.output = cap(p.text, TEXT_CAP);
        const c = this.council(task);
        if (c) c.synthesis = cap(p.text, TEXT_CAP);
        break;
      }
      case 'task.completed': {
        const st = str(p.status);
        const t = this.tasks.get(task);
        if (t) t.status = st || 'done';
        this.say(`${task} ${st === 'escalated' ? 'needs you' : 'finished'}`);
        break;
      }
      case 'task.failed': {
        const t = this.tasks.get(task);
        if (t) t.status = 'failed';
        this.say(`${task} failed`);
        break;
      }
      case 'task.cancelled': {
        const t = this.tasks.get(task);
        if (t) t.status = 'cancelled';
        this.say(`${task} stopped by the operator`);
        break;
      }
      case 'dag.node.added': {
        const id = str(p.node_id), r = str(p.role), parent = str(p.parent);
        this.nodes.set(id, { id, role: r, parent: parent && parent !== 'operator' ? parent : null, depth: Number(p.depth) || 0, taskId: task, state: 'pending', tokens: 0, costUsd: 0 });
        const a = this.agents.get(r);
        if (a) { a.state = 'running'; a.taskId = task; }
        break;
      }
      case 'dag.node.state': {
        const id = str(p.node_id), st = str(p.state);
        const n = this.nodes.get(id);
        const r = n?.role ?? '';
        if (n) n.state = st;
        if (st === 'done' || st === 'escalated') {
          this.live.delete(id);
          const a = this.agents.get(r);
          if (a) {
            const busy = [...this.nodes.values()].some((x) => x.role === r && x.state === 'running'); // another node of the role may still run
            a.state = busy ? 'running' : st === 'escalated' ? 'escalated' : 'idle';
          }
        }
        if (r) this.say(`${r} is ${st}`);
        break;
      }
      case 'agent.usage': {
        const cost = Number(p.cost_usd) || 0;
        this.spentUsd += cost;
        addTokens(this.tokens, p);
        this.series.push(cost);
        if (this.series.length > 240) this.series.shift();
        const tier = str(e.model_tier), model = str(e.model_id), ep = str(e.endpoint_id), span = str(e.span_id);
        const n = this.nodes.get(span);
        if (n) { n.costUsd += cost; n.tokens += (Number(p.input_tokens) || 0) + (Number(p.output_tokens) || 0); }
        const r = role || n?.role || '';
        const a = this.agents.get(r);
        if (a) {
          const rank = (t: string) => RANK[t] ?? -1;
          a.upscaled = a.tier !== '' && rank(tier) > rank(a.tier);
          if (a.upscaled) this.retries++;
          if (tier) a.tier = tier;
          if (model) a.model = model;
          if (ep) a.endpoint = ep;
          addTokens(a.tokens, p);
          a.costUsd += cost;
          a.calls++;
        }
        break;
      }
      case 'approval.requested': {
        this.pendingApprovals.set(str(p.approval_id), str(p.summary));
        const a = this.agents.get(str(p.role));
        if (a) a.state = 'waiting on you';
        this.say(`waiting on you: ${cap(p.summary, 80)}`);
        break;
      }
      case 'approval.resolved': {
        this.pendingApprovals.delete(str(p.approval_id));
        for (const a of this.agents.values()) if (a.state === 'waiting on you') a.state = 'running';
        break;
      }
      case 'build.attempt': {
        const n = Number(p.attempt) || 0;
        const line = `attempt ${n} · ${str(p.tier) || '?'} · ${str(p.family) || '?'}${p.homogeneous_retry === true ? ' (same family)' : ''}`;
        if ((Number(p.attempt) || 1) > 1) this.say(`${task}: retrying (${line})`);
        this.build(task).attempts.push(line);
        break;
      }
      case 'worktree.created': this.build(task).branch = str(p.branch); break;
      case 'gates.evaluated': {
        const b = this.build(task);
        b.sha = str(p.sha).slice(0, 12);
        b.gates = Array.isArray(p.gates) ? p.gates.map((g: any) => ({ name: str(g?.name), state: str(g?.state), detail: cap(g?.detail, 300) })) : [];
        break;
      }
      case 'gate.flaky': this.build(task).flaky = true; break;
      case 'audit.recorded': this.build(task).findings = sevTitle(p.findings); break;
      case 'merge.approved': this.build(task).merge = 'approved'; break;
      case 'action.result': {
        const id = str(p.action_id);
        if (id.startsWith('merge-') || id.startsWith('pull_request-')) this.build(task).merge = p.ok === true ? 'merged' : 'refused';
        break;
      }
      case 'council.started': {
        const seats = Array.isArray(p.seats) ? p.seats.map((x: any) => ({ role: str(x?.role), family: str(x?.family) })) : [];
        this.councils.push({ taskId: task, proposition: cap(p.proposition, 600), seats, homogeneous: p.homogeneous === true, phase: 'round 1', stances: [], critiques: [], escalation: null, path: '', unresolved: [], options: [], synthesis: '' });
        this.say(`${task}: council convened`);
        break;
      }
      case 'council.barrier.released': {
        const c = this.council(task);
        if (c) c.phase = 'round 2';
        this.say(`${task}: round 1 is in`);
        break;
      }
      case 'council.stance': this.council(task)?.stances.push({ role: str(p.role), answered: p.status === 'answered', text: cap(p.text, TEXT_CAP) }); break;
      case 'council.critique':
        this.council(task)?.critiques.push({ role: str(p.role), recommendation: str(p.recommendation), concur: typeof p.concur === 'boolean' ? p.concur : null, findings: sevTitle(p.findings), text: cap(p.text, TEXT_CAP) });
        break;
      case 'council.verdict': {
        const esc = typeof p.escalated === 'string' ? p.escalated : null;
        const c = this.council(task);
        if (c) {
          c.phase = esc !== null ? 'escalated' : 'verdict';
          c.escalation = esc;
          c.path = str(p.path);
          c.unresolved = Array.isArray(p.unresolved) ? p.unresolved.map((u: any) => [str(u?.severity), str(u?.seat), sanitize(str(u?.title))] as [string, string, string]) : [];
          c.options = Array.isArray(p.options) ? p.options.map((o: any) => [str(o?.label), Array.isArray(o?.seats) ? o.seats.map(str) : []] as [string, string[]]) : [];
        }
        this.say(`${task}: council ${esc !== null ? 'needs your decision' : 'reached a verdict'}`);
        break;
      }
      case 'connector.call': { const r = this.connector(str(p.connector)); r.calls++; r.lastTool = str(p.tool); r.lastAt = Date.parse(str(e.timestamp)) || r.lastAt; break; }
      case 'connector.result': {
        const r = this.connector(str(p.connector));
        r.results++;
        if (p.is_error === true) r.errors++; else { r.state = 'ready'; r.reason = ''; }
        break;
      }
      case 'connector.denied': { const r = this.connector(str(p.connector)); r.denied++; r.reason = cap(p.reason, 200); this.say(`${r.name} refused ${str(p.tool)}: ${r.reason}`); break; }
      case 'connector.unavailable': { const r = this.connector(str(p.connector)); r.state = 'unavailable'; r.reason = cap(p.reason, 200); this.say(`${r.name} is unavailable: ${r.reason}`); break; }
      case 'connector.reauth_required': { const r = this.connector(str(p.connector)); r.state = 'needs login'; r.reason = cap(p.reason, 200); this.say(`${r.name} needs you to sign in again (delphi auth login)`); break; }
      case 'endpoint.throttled': this.say(`${str(p.endpoint_id)} is throttled; trying the next endpoint`); break;
      case 'model.recovered': this.say(`${str(p.endpoint_id)} is back (${str(p.model_id)})`); break;
      case 'model.exhausted': this.say(`${str(p.endpoint_id)} is out of quota (${str(p.reason)}); trying the next endpoint`); break;
      case 'harness.fallback': this.say(`${str(p.from)} fell back to the API`); break;
      default: break;
    }
  }

  /** The cache-read share of all input-side tokens, 0 to 1. */
  cacheHitRatio(): number {
    const d = this.tokens.input + this.tokens.cacheRead + this.tokens.cacheWrite;
    return d === 0 ? 0 : this.tokens.cacheRead / d;
  }
  children(id: string): Node[] { return [...this.nodes.values()].filter((n) => n.parent === id); }
  roots(): Node[] { return [...this.nodes.values()].filter((n) => n.parent === null); }

  /** The view to open on when none is asked for: a Council in progress, a build in progress, else the Epic. */
  defaultView(): 'council' | 'forge' | 'epic' {
    if (this.councils.some((c) => c.phase === 'round 1' || c.phase === 'round 2')) return 'council';
    if (this.builds.some((b) => b.merge === '' || b.merge === 'approved') && [...this.tasks.values()].some((t) => t.flow === 'build' && t.status === 'running')) return 'forge';
    return 'epic';
  }
}

/** The agents in the order people think of them: the orchestrator first, then the roster's order. */
const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout', 'investigator'];
export function ordered(o: Observer): AgentRow[] {
  const rank = (r: string) => { const i = ORDER.indexOf(r); return i < 0 ? ORDER.length : i; };
  return [...o.agents.values()].sort((a, b) => rank(a.role) - rank(b.role) || a.role.localeCompare(b.role));
}
