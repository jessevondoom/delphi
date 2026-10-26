// A mission that never ends, for trying the narrator's monitoring panes without a daemon or any model. A pure simulation: `tick()`
// moves the world one second and returns what a daemon would have sent (live frames and events); `state()` is `observe.state`.
// Built on the fixture the tests use, with its times moved to the present so the clocks agree.
import type { OState } from '../src/observe-screen.ts';
import { monitorsWorld } from '../test/monitors-fixture.ts';
import { NOW } from '../test/epic-fixture.ts';

export type Frame = Record<string, { tokens?: number; est?: number }>;
export type Envelope = { type: string; timestamp: string; task_id?: string; span_id?: string; agent?: string; payload: Record<string, unknown> };
export type Opts = {
  /** No approval raised, no loop, no clash: the quiet picture. */
  calm?: boolean;
  /** Seed for the small random choices, so a run can be repeated. */
  seed?: number;
};

const SENTENCES = [
  'added a visibility timeout to the claim loop',
  'wired the timeout into the config and documented its default',
  'reworked the permission model to be less intrusive',
  'removed ratify requirements for explicit updates',
  'split the retry queue into its own module',
  'fixed a race where two claims could take one row',
  'wrote the migration for the new status column',
  'tightened the gate checks and added a test for each',
];
const FILES = ['crates/queue/src/claim.rs', 'crates/queue/src/retry.rs', 'specs/040-retry.md', 'crates/queue/tests/claim.rs', 'README.md'];

/** The fixture's times (Unix ms, around `NOW`) moved by `by` ms, so "now" is the real now. */
export function shiftTimes(st: OState, by: number): OState {
  const s: OState = JSON.parse(JSON.stringify(st));
  for (const n of Object.values(s.nodes)) { n.started_ms += by; if (n.ended_ms !== null) n.ended_ms += by; }
  for (const c of s.changes ?? []) c.at_ms += by;
  for (const a of Object.values(s.approvals ?? {})) a.since_ms += by;
  if (s.since_you) s.since_you.since_ms += by;
  for (const f of s.files ?? []) f.last_ms += by;
  for (const t of Object.values(s.tools ?? {})) if (t.open) t.open.since_ms += by;
  for (const x of s.sessions ?? []) x.started_ms += by;
  return s;
}

export class Preview {
  st: OState;
  private t = 0;
  private rnd: () => number;
  private live = 150_000;
  private est = 0;
  private approvalN = 0;
  private resolveAt = -1;
  private now: number;

  constructor(now: number, private opts: Opts = {}) {
    this.now = now;
    let x = (opts.seed ?? 7) >>> 0;
    this.rnd = () => { x = (Math.imul(x, 1664525) + 1013904223) >>> 0; return x / 2 ** 32; };
    const st = shiftTimes(monitorsWorld(), now - NOW);
    st.token_cap = 200_000;
    // An API seat working alongside the subscription builder: the scout, whose usage is logged each turn.
    st.nodes['n7'] = { id: 'n7', role: 'scout', parent: null, depth: 0, task_id: 't_1', state: 'running', tokens: 0, cost_usd: 0, started_ms: now - 3 * 60_000, ended_ms: null, after: [] };
    st.agents['scout']!.state = 'running';
    if (opts.calm) {
      st.approvals = {}; st.pending_approvals = {}; st.agents['red-team']!.state = 'idle';
      st.tools!['builder']!.streak = { tool: 'read_file', n: 1, failed: 0 }; st.tools!['builder']!.open = null;
      for (const f of st.files ?? []) { f.clash = false; f.roles = { architect: f.writes }; }
    }
    this.st = st;
  }

  /** `observe.state` as the daemon would answer it now. */
  state(): OState { return this.st; }

  private env(type: string, agent: string, span: string, payload: Record<string, unknown>): Envelope {
    return { type, timestamp: new Date(this.now).toISOString(), task_id: 't_1', span_id: span, agent, payload };
  }

  /** One second of the mission: the frames a daemon would stream, and the events it would log. */
  tick(now: number = this.now + 1000): { frames: Frame; events: Envelope[] } {
    this.now = now;
    this.t++;
    const st = this.st, frames: Frame = {}, events: Envelope[] = [];
    // The subscription builder's running total climbs towards the cap and holds just under it.
    this.live = Math.min(Math.floor((st.token_cap ?? 200_000) * 0.985), this.live + 1800 + Math.floor(this.rnd() * 900));
    frames['n3'] = { tokens: this.live };
    // The API scout's estimate grows through a turn; each turn ends with its usage logged and the estimate dropped.
    this.est += 120 + Math.floor(this.rnd() * 200);
    frames['n7'] = { est: this.est };
    if (this.t % 25 === 0) {
      const a = st.agents['scout']!;
      a.tokens = { ...a.tokens, output: a.tokens.output + this.est, input: a.tokens.input + 4000 };
      events.push(this.env('agent.usage', 'scout', 'n7', { input_tokens: 4000, output_tokens: this.est }));
      this.est = 0;
    }
    // Tool calls: the builder works through a few tools, and keeps running the same command (a loop forming) unless calm.
    if (this.t % 3 === 0) {
      const b = st.tools!['builder']!;
      const tool = this.opts.calm ? ['read_file', 'edit_file'][this.t % 2]! : this.t % 9 === 0 ? 'edit_file' : 'run_command';
      b.calls++; b.last = tool;
      const c = (b.by_tool[tool] ??= { calls: 0, errors: 0 }); c.calls++;
      if (!this.opts.calm && tool === 'run_command' && this.rnd() < 0.4) { c.errors++; b.errors++; }
      b.streak = b.streak && b.streak.tool === tool ? { tool, n: b.streak.n + 1, failed: this.rnd() < 0.4 ? b.streak.failed + 1 : 0 } : { tool, n: 1, failed: 0 };
    }
    // A change recorded now and then.
    if (this.t % 15 === 0) {
      const role = this.rnd() < 0.7 ? 'builder' : 'architect';
      const summary = SENTENCES[Math.floor(this.rnd() * SENTENCES.length)]!;
      const files = [FILES[Math.floor(this.rnd() * FILES.length)]!];
      (st.changes ??= []).push({ session: 's_0001', at_ms: now, role, task: 't_1', summary, files });
      const rt = st.tools![role] ??= { calls: 0, errors: 0, last: '', by_tool: {} };
      (rt.by_tool['record_change'] ??= { calls: 0, errors: 0 }).calls++;
      if (st.since_you) st.since_you.changes++;
      events.push(this.env('change.recorded', role, role === 'builder' ? 'n3' : 'n1', { summary, role, files }));
    }
    // A file touched, sometimes by a second role, which lights up the clash flag.
    if (this.t % 10 === 0) {
      const path = FILES[this.t % FILES.length]!;
      const role = this.opts.calm || this.t % 30 === 0 ? 'architect' : 'builder';
      const files = (st.files ??= []);
      const f = files.find((x) => x.path === path) ?? (files[files.push({ path, roles: {}, writes: 0, last_ms: now, clash: false }) - 1]!);
      f.roles[role] = (f.roles[role] ?? 0) + 1; f.writes++; f.last_ms = now; f.clash = Object.keys(f.roles).length > 1;
      files.sort((a, b) => Number(b.clash) - Number(a.clash) || b.last_ms - a.last_ms);
      files.length = Math.min(files.length, 12);
    }
    // An approval raised, left waiting a while, then answered; answering restarts the "since you last acted" digest.
    if (!this.opts.calm && this.t % 45 === 20 && this.resolveAt < 0) {
      const id = `ap_p${++this.approvalN}`;
      st.approvals = { ...(st.approvals ?? {}), [id]: { role: 'builder', summary: 'run cargo test -p queue', since_ms: now } };
      st.pending_approvals = { ...(st.pending_approvals ?? {}), [id]: 'run cargo test -p queue' };
      st.agents['builder']!.state = 'waiting on you';
      if (st.since_you) st.since_you.approvals++;
      this.resolveAt = this.t + 18;
      events.push(this.env('approval.requested', 'builder', 'n3', { approval_id: id, role: 'builder', summary: 'run cargo test -p queue' }));
    } else if (this.resolveAt > 0 && this.t >= this.resolveAt) {
      const id = Object.keys(st.approvals ?? {}).find((k) => k.startsWith('ap_p'));
      if (id) {
        delete st.approvals![id]; delete st.pending_approvals![id];
        st.agents['builder']!.state = 'running';
        st.since_you = { since_ms: now, changes: 0, done: 0, failed: 0, approvals: 0 };
        events.push(this.env('approval.resolved', 'builder', 'n3', { approval_id: id, decision: 'approved' }));
      }
      this.resolveAt = -1;
    }
    return { frames, events };
  }
}
