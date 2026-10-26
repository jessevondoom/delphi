// The event fold: daemon notifications in, the console's state out. Pure, so it is tested without a terminal.
import { citations, noteSources, type Source } from './research.ts';
import { classify, stripWrapper, type Cause } from './attention.ts';
import { sanitize, type AgentState } from './theme.ts';

/**
 * An `attention` block is a task that stopped and needs the operator (an escalation or a failure): `text` is the
 * headline, `reason` the engine's reason without its wrapper, `cause` which stop it is (`attention.ts`).
 */
export type Block = { id: number; kind: 'user' | 'reply' | 'note' | 'card' | 'tool' | 'attention'; role?: string; text: string; at: number; status?: string; ref?: string; detail?: string; reason?: string; cause?: Cause;
  /** For a note: how it is marked in the chat (`·` information, `✖` error, `✔` success; `005e` §10.6). */
  tone?: 'info' | 'error' | 'ok' };
export type Agent = {
  state: AgentState; doing: string;
  /** When it entered `escalated`, so the strip can let go of the face (`005e` §6.2). */
  since?: number;
  /** When its last reasoning delta arrived: a running agent with one in the last 1.5 s is thinking (`005e` §10.2). */
  thinkingAt?: number;
  /** When its task last finished: the badge shows success for 3 s. */
  successAt?: number;
};
export type Approval = { id: string; task: string; role: string; tool: string; tier: string; summary: string; preview: string; blake3: string; payload: unknown; at: number; timeoutS: number };
/**
 * One agent's work in progress (`005e` §8): its reasoning and its words so far (the tail only; a long stream never
 * grows without bound), the tool call it is waiting on, and what it has done that a summary can name.
 */
export type Span = {
  span: string; task: string; role: string; depth: number; since: number;
  thinking: string; text: string;
  /** Which of the two the agent is doing now. */
  last: 'thinking' | 'text';
  tool: string | null; toolId: string | null;
  wrote: string | null; read: string | null;
  error: string | null;
  /** A council's blind round: no text is kept (`003` §3). */
  blind: boolean;
};
/** A finished span, as the history of the activity deck shows it: a chip, who, what, how long. */
export type Done = { id: number; role: string; what: string; elapsed: number; at: number; ok: boolean };
export type Activity = { running: Span[]; history: Done[] };
export const HISTORY_MAX = 500;
export const emptyActivity = (): Activity => ({ running: [], history: [] });
export type State = {
  blocks: Block[];
  live: string;
  spans: Record<string, { role: string; depth: number }>;
  /** Spans that have ended. Streaming frames and events travel on different sockets, so a frame can arrive after its task's last event; it must not bring the finished reply back as a live one. */
  ended: Record<string, true>;
  agents: Record<string, Agent>;
  tokens: number;
  approvals: Record<string, Approval>;
  alert: { role: string; until: number } | null;
  lastActivity: number;
  nextId: number;
  council: boolean;
  /** Where answers came from: files the agents read (this session) and documents their replies cite. */
  sources: Record<string, Source>;
  /** The left deck: what is running now, and what has finished. In memory only; never written anywhere. */
  activity: Activity;
  /** What each task was asked, to say what a span answered. */
  prompts: Record<string, string>;
  /** Tasks the daemon is working on, by task id. */
  running: Record<string, { since: number }>;
  /** The operator has said something in this mission: the welcome text never comes back (`/new` does not bring it back). */
  started: boolean;
  /** The number of the session on the screen (`session.started`); 0 before any (`specs/030`). */
  session: number;
};

export const initial = (now: number): State => ({
  blocks: [], live: '', spans: {}, ended: {}, agents: {}, tokens: 0, approvals: {}, alert: null, lastActivity: now, nextId: 1, council: false, running: {}, activity: emptyActivity(), prompts: {}, sources: {}, started: false, session: 0,
});

const ROLES = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout', 'investigator'];

export function addBlock(s: State, b: Omit<Block, 'id'>): State {
  return { ...s, blocks: [...s.blocks, { ...b, id: s.nextId }], nextId: s.nextId + 1, started: s.started || b.kind === 'user' };
}

const TAIL = 4000;
const tail = (t: string) => (t.length > TAIL ? t.slice(-TAIL) : t);

function startSpan(a: Activity, span: string, task: string, role: string, depth: number, now: number): Activity {
  if (a.running.some((r) => r.span === span)) return a;
  return { ...a, running: [...a.running, { span, task, role, depth, since: now, thinking: '', text: '', last: 'thinking', tool: null, toolId: null, wrote: null, read: null, error: null, blind: false }] };
}

const touch = (a: Activity, span: string, f: (r: Span) => Span): Activity => (a.running.some((r) => r.span === span) ? { ...a, running: a.running.map((r) => (r.span === span ? f(r) : r)) } : a);

/** One short phrase for what a span did, for the history (`wrote crates/queue.rs`, `answered "table or file?"`). */
function whatDid(r: Span, prompts: Record<string, string>): string {
  if (r.wrote) return `wrote ${r.wrote}`;
  const asked = r.depth === 0 ? prompts[r.task] : undefined;
  if (asked) return `answered "${asked.replace(/\s+/g, ' ').trim().slice(0, 28)}${asked.trim().length > 28 ? '…' : ''}"`;
  if (r.read) return `read ${r.read}`;
  return r.blind ? 'took its seat' : 'finished';
}

/** A span ended: it leaves the running zone and one entry joins the history, in the same step. */
function endSpan(a: Activity, span: string, ok: boolean, now: number, prompts: Record<string, string>): Activity {
  const r = a.running.find((x) => x.span === span);
  if (!r) return a;
  const id = (a.history[a.history.length - 1]?.id ?? 0) + 1;
  const entry: Done = { id, role: r.role, what: ok ? whatDid(r, prompts) : 'needs you', elapsed: Math.max(0, now - r.since), at: now, ok };
  const history = [...a.history, entry];
  return { running: a.running.filter((x) => x.span !== span), history: history.length > HISTORY_MAX ? history.slice(-HISTORY_MAX) : history };
}

/** Spans of a task that is over without having said so (cancelled, failed): they go without a chip. */
/** Remember that these spans are over (the newest thousand: a session can run for days). */
const endedWith = (ended: State['ended'], ids: string[]): State['ended'] => {
  if (!ids.length) return ended;
  const all = { ...ended, ...Object.fromEntries(ids.map((i) => [i, true as const])) };
  const keys = Object.keys(all);
  return keys.length > 2000 ? Object.fromEntries(keys.slice(-1000).map((k) => [k, true as const])) : all;
};
/** The running spans of `task` (every running span when there is none), by id. */
const spansOf = (a: Activity, task?: string): string[] => a.running.filter((r) => !task || r.task === task).map((r) => r.span);

const dropTask = (a: Activity, task?: string): Activity => (task ? { ...a, running: a.running.filter((r) => r.task !== task) } : { ...a, running: [] });

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
  const KEEP = new Set(['session.started', 'task.created', 'task.output', 'task.failed', 'task.cancelled', 'dag.node.added', 'council.started', 'council.barrier.released']);
  const done = events.filter((e) => KEEP.has(e.type)).reduce((a, e) => fold(a, { method: 'event', params: e }, stamp(e, Date.now())), s);
  // Every span of a task that has finished is over. A streaming frame can still be in flight when a resync reloads the
  // history (the daemon sends one to a client that fell behind), and without this it would bring the finished reply back as a
  // "writing" block that never ends. Spans of a task that has not finished stay open: they are still streaming.
  const taskOf: Record<string, string> = {}, over = new Set<string>();
  for (const e of events) {
    if (e.type === 'dag.node.added' && e.payload?.node_id != null) taskOf[String(e.payload.node_id)] = String(e.task_id ?? '');
    if ((e.type === 'task.output' || e.type === 'task.failed' || e.type === 'task.cancelled') && e.task_id) over.add(String(e.task_id));
  }
  const ended = endedWith(s.ended, Object.keys(taskOf).filter((k) => over.has(taskOf[k]!)));
  return { ...done, running: {}, agents: {}, ended, activity: emptyActivity() }; // what is running now comes from the daemon's snapshot, not from old events
}

/** The block for a task that stopped: the reason without the engine's wrapper, and the console's own words for it. */
function attention(role: string, text: string, at: number): Omit<Block, 'id'> {
  const reason = stripWrapper(text);
  const a = classify(reason);
  return { kind: 'attention', role, text: a.headline, reason, cause: a.cause, at, status: 'escalated' };
}

/** Who is speaking now: the role of the newest top-level span (the orchestrator before any has started). */
export function speakerOf(s: State): string {
  const tops = Object.entries(s.spans).filter(([, v]) => v.depth === 0).sort((a, b) => spanNum(b[0]) - spanNum(a[0]));
  return tops[0]?.[1].role ?? 'orchestrator';
}

const setAgent = (s: State, role: string, a: Agent): State => ({ ...s, agents: { ...s.agents, [role]: a } });
const spanNum = (k: string) => Number(k.split('_').pop()) || 0;

export function fold(s: State, n: { method: string; params?: any }, now: number): State {
  const p = n.params ?? {};
  if (n.method === 'delta.frame') {
    const spans = (p.spans ?? {}) as Record<string, { text?: string; thinking?: string }>;
    let add = '';
    let activity = s.activity;
    let agents = s.agents;
    for (const k of Object.keys(spans).sort()) {
      if (s.ended[k]) continue; // late: the span is over and its words are already in a block
      const who = s.spans[k]?.role;
      // Reasoning is only noted as "thinking", never shown, so even a blind council round pans its badges.
      if (who && spans[k]?.thinking && agents[who]) agents = { ...agents, [who]: { ...agents[who]!, thinkingAt: now } };
      const th = spans[k]?.thinking ? sanitize(spans[k]!.thinking!) : '', tx = spans[k]?.text ? sanitize(spans[k]!.text!) : '';
      if (s.council && s.spans[k]) { activity = touch(activity, k, (r) => ({ ...r, blind: true, last: 'thinking' })); continue; } // a council's round one stays blind
      if (th || tx) activity = touch(activity, k, (r) => ({ ...r, thinking: th ? tail(r.thinking + th) : r.thinking, text: tx ? tail(r.text + tx) : r.text, last: tx ? 'text' : 'thinking', tool: tx ? null : r.tool }));
      add += spans[k]?.text ?? '';
    }
    if (!add && activity === s.activity && agents === s.agents) return s;
    return { ...s, agents, activity, live: (s.live + sanitize(add)).slice(-8000), tokens: s.tokens + Math.ceil(add.length / 4), lastActivity: now };
  }
  if (n.method !== 'event') return s;
  const e = p, pl = e.payload ?? {};
  switch (e.type) {
    case 'dag.node.added': {
      const role = String(pl.role ?? '?');
      const next = { ...s, spans: { ...s.spans, [String(pl.node_id)]: { role, depth: Number(pl.depth ?? 0) } }, activity: startSpan(s.activity, String(pl.node_id), String(e.task_id ?? ''), role, Number(pl.depth ?? 0), now), lastActivity: now };
      return setAgent(next, role, { state: 'running', doing: 'working' });
    }
    case 'dag.node.state': {
      if (pl.state !== 'done' && pl.state !== 'escalated') return s;
      return { ...s, ended: endedWith(s.ended, [String(pl.node_id)]), activity: endSpan(s.activity, String(pl.node_id), pl.state === 'done', now, s.prompts) };
    }
    case 'task.created': {
      const run = e.task_id ? { ...s.running, [e.task_id]: { since: now } } : s.running;
      if (e.task_id && pl.prompt) s = { ...s, prompts: { ...s.prompts, [e.task_id]: sanitize(String(pl.prompt)) } };
      // A new task starts clean: the face of an earlier escalation does not linger in the strip.
      const agents = Object.fromEntries(Object.entries(s.agents).map(([r, a]) => [r, a.state === 'escalated' ? { state: 'idle' as const, doing: '' } : a]));
      if (pl.flow === 'build' && !pl.prompt) return { ...s, running: run, agents };
      return addBlock({ ...s, running: run, agents, lastActivity: now }, { kind: 'user', text: sanitize(String(pl.prompt ?? '')), at: stamp(e, now) });
    }
    // A new session (`specs/030`): the screen starts empty and the sources start over. What is still running, and what
    // waits on the operator, carries on; the welcome text does not come back.
    case 'session.started': {
      const n = Number(String(pl.session_id ?? '').replace(/\D/g, ''));
      // The same session announced again changes nothing. A console that reconnects after a daemon restart is told twice: its
      // history replay contains the announcement, and so does the live stream. Clearing for the second would wipe the
      // "reconnected" note that was added in between, before it could be drawn.
      if (n && n === s.session) return s;
      return { ...s, blocks: [], live: '', sources: {}, started: true, session: n || s.session + 1 };
    }
    // A task moved to another model (`specs/035`): said once in the chat, so the operator is never surprised by who is answering.
    case 'model.promoted': {
      const pair = (v: any) => `${sanitize(String(v?.endpoint_id ?? '?'))}/${sanitize(String(v?.model_id ?? '?'))}`;
      return addBlock(s, { kind: 'note', text: `continuing on ${pair(pl.to)} (${pair(pl.from)} is not available)`, at: stamp(e, now), tone: 'info' });
    }
    case 'council.started': return { ...s, council: true, lastActivity: now };
    case 'council.barrier.released': return { ...s, council: false };
    case 'task.output': {
      const role = speakerOf(s);
      const idle: Record<string, Agent> = {};
      for (const [r, a] of Object.entries(s.agents)) idle[r] = a.state === 'waiting on you' ? a : { state: 'idle', doing: '' };
      const escalated = pl.status === 'escalated';
      const said = sanitize(String(pl.text ?? ''));
      const base = { ...s, live: '', council: false, agents: idle, running: without(s.running, e.task_id), ended: endedWith(s.ended, spansOf(s.activity, e.task_id)), activity: dropTask(s.activity, e.task_id), lastActivity: now };
      if (escalated) return setAgent(addBlock(base, attention(role, said, stamp(e, now))), role, { state: 'escalated', doing: '', since: now });
      const done = addBlock({ ...base, sources: noteSources(s.sources, citations(said), 'cited', role, stamp(e, now)) }, { kind: 'reply', role, text: said, at: stamp(e, now), status: 'done' });
      return setAgent(done, role, { state: 'idle', doing: '', successAt: now });
    }
    case 'task.cancelled': {
      const idle: Record<string, Agent> = {};
      for (const r of Object.keys(s.agents)) idle[r] = { state: 'idle', doing: '' };
      return addBlock({ ...s, live: '', council: false, agents: idle, running: without(s.running, e.task_id), ended: endedWith(s.ended, spansOf(s.activity, e.task_id)), activity: dropTask(s.activity, e.task_id), alert: null, lastActivity: now }, { kind: 'note', text: '■ stopped', at: stamp(e, now) });
    }
    case 'agent.block.complete': {
      const b = pl.block;
      if (b?.type !== 'tool_call') return s;
      const line = `${b.name}(${briefArgs(b.arguments)})`;
      const read = b.name === 'read_file' && typeof b.arguments?.path === 'string' ? [String(b.arguments.path)] : [];
      const sid = String(e.span_id ?? '');
      const path = typeof b.arguments?.path === 'string' ? String(b.arguments.path) : null;
      const activity = touch(s.activity, sid, (r) => ({ ...r, tool: sanitize(line), toolId: String(b.id ?? ''), error: null, wrote: path && /^(write|edit|create|apply|patch|replace)/i.test(String(b.name)) ? path : r.wrote, read: path && b.name === 'read_file' ? path : r.read }));
      const withAct = { ...s, sources: noteSources(s.sources, read, 'read', String(e.agent ?? '?'), now), activity };
      return addBlock(withAct, { kind: 'tool', role: e.agent, text: line, at: now, status: 'pending', ref: String(b.id ?? '') });
    }
    case 'tool.result': {
      const ref = String(pl.tool_call_id ?? '');
      const detail = sanitize(String(pl.content ?? '')).split('\n').slice(0, 40).join('\n');
      const act = touch(s.activity, String(e.span_id ?? ''), (r) => (r.toolId === ref ? { ...r, tool: null, toolId: null, error: pl.is_error ? detail.split('\n')[0]!.slice(0, 120) : null } : r));
      return { ...s, activity: act, blocks: s.blocks.map((x) => (x.kind === 'tool' && x.ref === ref ? { ...x, status: pl.is_error ? 'error' : 'ok', detail } : x)) };
    }
    case 'task.failed': {
      return addBlock({ ...s, live: '', running: without(s.running, e.task_id), ended: endedWith(s.ended, spansOf(s.activity, e.task_id)), activity: dropTask(s.activity, e.task_id), lastActivity: now }, attention(speakerOf(s), sanitize(String(pl.error ?? '')), now));
    }
    case 'approval.requested': {
      const id = String(pl.approval_id ?? '');
      const role = String(pl.role ?? 'orchestrator');
      const secs = Number(pl.timeout_secs ?? pl.timeout_s ?? pl.expires_in_s ?? 600);
      const card: Approval = { id, task: String(e.task_id ?? pl.task_id ?? ''), role, tool: String(pl.tool ?? ''), tier: String(pl.tier ?? 'amber'), summary: sanitize(String(pl.summary ?? '')), preview: sanitize(String(pl.payload?.preview ?? '')), blake3: String(pl.payload_blake3 ?? ''), payload: pl.payload, at: now, timeoutS: secs };
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
