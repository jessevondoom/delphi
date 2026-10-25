// A world for the epic rail: six units of work in a chain, the first two finished, one running, three queued.
// Times are Unix ms from `T0`; the window's clock is `NOW`.
import { emptyState, type OState } from '../src/observe-screen.ts';
import type { Info } from '../src/theme.ts';

export const T0 = 1_000_000_000_000;
export const NOW = T0 + 20 * 60_000;
const min = (n: number) => T0 + n * 60_000;

const crown: Record<string, string> = { orchestrator: '  ▄▄▄  ', architect: '▄▄▄▄▄▄▄', builder: '▄▄▄▄▄▄▄', 'red-team': '▄     ▄', strategist: '▄▄ ▄ ▄▄', scout: '   ▄   ' };
const mk = (name: string, chassis: string, face: string, role: string) => ({ name, chassis, face, crown: crown[role], crown_rgb: '#facc15' });
export const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: { orchestrator: mk('athena', '#a855f7', '•─•', 'orchestrator'), architect: mk('daedalus', '#38bdf8', '•_•', 'architect'), builder: mk('hephaestus', '#f97316', '•─•', 'builder'), 'red-team': mk('ares', '#ef4444', '-_-', 'red-team'), strategist: mk('odysseus', '#22c55e', '•_•', 'strategist'), scout: mk('hermes', '#eab308', '•••', 'scout') },
};

type N = OState['nodes'][string];
const node = (id: string, role: string, state: string, started: number, ended: number | null, after: string[], task = 't_1'): N =>
  ({ id, role, parent: null, depth: 0, task_id: task, state, tokens: 1000, cost_usd: 0, started_ms: min(started), ended_ms: ended === null ? null : min(ended), after });

export function epicWorld(): OState {
  const s = emptyState();
  s.mission_id = '2026-10-25-rail';
  s.tasks['t_1'] = { id: 't_1', flow: 'build', prompt: 'Add a retry queue to the claim loop', status: 'running' };
  s.nodes = {
    n1: node('n1', 'architect', 'done', 2, 9, []),
    n2: node('n2', 'strategist', 'done', 9, 15, ['n1']),
    n3: node('n3', 'builder', 'running', 15, null, ['n2']),
    n4: node('n4', 'red-team', 'pending', 15, null, ['n3']),
    n5: node('n5', 'orchestrator', 'pending', 15, null, ['n4']),
    n6: node('n6', 'scout', 'pending', 15, null, ['n3']),
  };
  for (const r of ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout']) {
    s.agents[r] = { role: r, state: r === 'builder' ? 'running' : 'idle', task_id: 't_1', endpoint: 'direct', tier: 'frontier', model: 'x', upscaled: false, tokens: { input: 0, cache_read: 0, cache_write: 0, output: 0, thinking: 0 }, cost_usd: 0, calls: 1 };
  }
  s.tokens = { input: 8000, cache_read: 4000, cache_write: 0, output: 2200, thinking: 0 };
  s.pace = [0, 400, 900, 2200, 3100, 4200, 6400, 5200, 3900, 2400, 1500, 900];
  return s;
}
