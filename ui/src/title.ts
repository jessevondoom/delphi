// The terminal's title: "⟦•─•⟧ delphi v0.2.6: idle", or how many agents are working across how many missions, or
// how many things wait for the operator. The daemon's events cover every mission in the workspace, so this counts
// them all; the fold (`fold.ts`) only describes the mission on screen.
import { sanitize } from './theme.ts';

const MARK = '⟦•─•⟧';

type Env = { mission_id?: string; type?: string; task_id?: string; span_id?: string; payload?: any };

/** Agents working (running spans), missions with a task in flight, and approvals waiting, from the event stream. */
export class Activity {
  private spans = new Map<string, { mission: string; task: string }>();
  private tasks = new Map<string, string>(); // task id -> mission
  private approvals = new Map<string, string>(); // approval id -> mission

  /** Fold one daemon notification; true when a count changed. */
  feed(n: any): boolean {
    if (n?.method !== 'event') return false;
    const e: Env = n.params ?? {};
    const mission = String(e.mission_id ?? ''), task = String(e.task_id ?? ''), pl = e.payload ?? {};
    const before = this.key();
    switch (e.type) {
      case 'task.created': if (task) this.tasks.set(task, mission); break;
      case 'dag.node.added': this.spans.set(String(pl.node_id ?? e.span_id), { mission, task }); if (task && !this.tasks.has(task)) this.tasks.set(task, mission); break;
      case 'dag.node.state': if (pl.state === 'done' || pl.state === 'escalated') this.spans.delete(String(pl.node_id)); break;
      case 'approval.requested': this.approvals.set(String(pl.approval_id ?? ''), mission); break;
      case 'approval.resolved': this.approvals.delete(String(pl.approval_id ?? '')); break;
      case 'task.output': case 'task.failed': case 'task.cancelled': this.end(task); break;
    }
    return this.key() !== before;
  }

  /** Something the console already knows is waiting (a mission reopened with a pending approval). */
  seedApproval(id: string, mission: string) { this.approvals.set(id, mission); }

  /** A lost connection means missed events: start from nothing rather than show a stale count. */
  reset() { this.spans.clear(); this.tasks.clear(); this.approvals.clear(); }

  private end(task: string) {
    this.tasks.delete(task);
    for (const [k, s] of this.spans) if (s.task === task) this.spans.delete(k);
  }

  get agents() { return this.spans.size; }
  get missions() { return new Set(this.tasks.values()).size; }
  get waiting() { return this.approvals.size; }
  private key() { return `${this.agents}/${this.missions}/${this.waiting}`; }
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** What follows the version in the title. */
export function status(agents: number, missions: number, waiting: number): string {
  if (waiting > 0) return `${agents > 0 ? `${plural(agents, 'agent', 'agents')} working, ` : ''}${waiting} need${waiting === 1 ? 's' : ''} your attention`;
  if (agents > 0) return `${plural(agents, 'agent', 'agents')} working, ${plural(Math.max(missions, 1), 'mission', 'missions')}`;
  return 'idle';
}

export function title(version: string, a: Activity): string {
  const v = /^\d/.test(version) ? `v${version}` : version;
  return sanitize(`${MARK} delphi ${v}: ${status(a.agents, a.missions, a.waiting)}`);
}
