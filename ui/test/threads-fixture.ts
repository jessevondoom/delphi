// A world for the threads pane: five tasks that fall into four threads. A vendor comparison feeds a pricing model, a
// build has a council verdict behind it and a docs task about the same queue, and a finished survey sits settled.
import type { OState } from '../src/observe-screen.ts';
import { epicWorld, info, NOW, T0 } from './epic-fixture.ts';

export { info, NOW, T0 };
const min = (n: number) => T0 + n * 60_000;
type N = OState['nodes'][string];
const node = (id: string, role: string, task: string, state: string, started: number, ended: number | null, after: string[] = []): N =>
  ({ id, role, parent: null, depth: 0, task_id: task, state, tokens: 1000, cost_usd: 0, started_ms: started ? min(started) : 0, ended_ms: ended === null ? null : min(ended), after });

export function threadsWorld(): OState {
  const s = epicWorld();
  s.nodes = {}; s.tasks = {}; s.councils = []; s.builds = [];
  s.tasks['t_1'] = { id: 't_1', flow: 'research', prompt: 'Compare managed vector databases for the knowledge base', status: 'running' };
  s.tasks['t_2'] = { id: 't_2', flow: 'strategy', prompt: 'Draft the knowledge base pricing tiers', status: 'running' };
  s.tasks['t_3'] = { id: 't_3', flow: 'build', prompt: 'Add a retry queue to the claim loop', status: 'running' };
  s.tasks['t_4'] = { id: 't_4', flow: 'docs', prompt: 'Document the retry queue for operators', status: 'running' };
  s.tasks['t_5'] = { id: 't_5', flow: 'research', prompt: 'Survey competitor onboarding flows', status: 'done' };
  Object.assign(s.nodes, {
    a1: node('a1', 'scout', 't_5', 'done', 1, 5), a2: node('a2', 'strategist', 't_5', 'done', 5, 8, ['a1']),
    b1: node('b1', 'scout', 't_1', 'done', 3, 9), b2: node('b2', 'architect', 't_1', 'running', 9, null, ['b1']), b3: node('b3', 'red-team', 't_1', 'pending', 0, null, ['b2']),
    c1: node('c1', 'strategist', 't_2', 'pending', 0, null, ['b3']), c2: node('c2', 'orchestrator', 't_2', 'pending', 0, null, ['c1']),
    d1: node('d1', 'architect', 't_3', 'done', 2, 9), d2: node('d2', 'strategist', 't_3', 'done', 9, 15, ['d1']), d3: node('d3', 'builder', 't_3', 'running', 15, null, ['d2']),
    d4: node('d4', 'red-team', 't_3', 'pending', 0, null, ['d3']), d5: node('d5', 'orchestrator', 't_3', 'pending', 0, null, ['d4']),
    f1: node('f1', 'scout', 't_4', 'pending', 0, null, ['d3']),
  });
  s.councils = [{ task_id: 't_3', proposition: 'Should the retry queue be a table or a file?', seats: [{ role: 'architect', family: 'a' }, { role: 'strategist', family: 'b' }, { role: 'scout', family: 'c' }], homogeneous: false, phase: 'verdict', stances: [], critiques: [{ role: 'architect', recommendation: 'table', concur: true, findings: [] }, { role: 'strategist', recommendation: 'table', concur: true, findings: [] }, { role: 'scout', recommendation: 'file', concur: false, findings: [] }], escalation: null, path: '', unresolved: [], options: [] }];
  s.builds = [{ task_id: 't_3', branch: 'delphi/t_3', attempts: ['attempt 1: tests red, fixed'], sha: 'abc1234', gates: [{ name: 'tests', state: 'running', detail: '12 of 14 passed' }, { name: 'audit', state: 'pending', detail: '' }, { name: 'merge', state: 'pending', detail: 'waits for you' }], findings: [], flaky: false, merge: '' }];
  return s;
}
