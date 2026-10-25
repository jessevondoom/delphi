// Everything the console learns when it attaches or re-attaches: what was said, what is running, what waits.
import { fold, initial, replay, type State } from './fold.ts';
import type { Rpc } from './rpc.ts';

export async function loadHistory(cmd: Rpc, mission: string): Promise<State> {
  const snap = await cmd.call('observe.snapshot', { mission_id: mission }).catch(() => ({ events: [] }));
  let s = replay(initial(Date.now()), snap.events ?? []);
  const waiting = await cmd.call('approval.list', {}).catch(() => []);
  for (const a of Array.isArray(waiting) ? waiting : []) s = fold(s, { method: 'event', params: { type: 'approval.requested', task_id: a.task_id, payload: a } }, Date.now());
  const live = await cmd.call('session.snapshot', { mission_id: mission }).catch(() => ({ running_tasks: [] }));
  for (const t of live.running_tasks ?? []) s.running[String(t.task_id)] = { since: Date.now() - Number(t.seconds ?? 0) * 1000 };
  return s;
}
