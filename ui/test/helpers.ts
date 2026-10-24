// Shared fixtures for the screen tests: a small world with a council, a build, spend, approvals and a conversation,
// and the golden-file helper used by test/snapshots.test.ts.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Observer } from '../src/observer.ts';
import { addBlock, fold, initial, type State } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

export const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: {
    orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' },
    architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' },
    builder: { name: 'hephaestus', chassis: '#f59e0b', face: '•_•' },
    'red-team': { name: 'ares', chassis: '#ef4444', face: '-_-' },
  },
};
export const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

let seq = 0;
export const ev = (type: string, task: string, payload: any, extra: any = {}) => ({ seq: ++seq, type, task_id: task, payload, ...extra });

export function observerWorld(): Observer {
  seq = 0;
  const o = new Observer(['orchestrator', 'architect', 'builder', 'red-team']);
  const events = [
    ev('task.created', 't1', { flow: 'council', prompt: 'Table or file for the retry queue?' }),
    ev('council.started', 't1', { proposition: 'Table or file for the retry queue?', seats: [{ role: 'architect', family: 'claude' }, { role: 'red-team', family: 'gemini' }], homogeneous: false }),
    ev('dag.node.added', 't1', { node_id: 'n1', role: 'architect', depth: 0 }),
    ev('dag.node.added', 't1', { node_id: 'n2', role: 'builder', depth: 1, parent: 'n1' }),
    ev('agent.usage', 't1', { input_tokens: 1000, cache_read_tokens: 3000, output_tokens: 200, thinking_tokens: 50, cost_usd: 0.02 }, { agent: 'architect', span_id: 'n1', model_tier: 'fast', model_id: 'haiku', endpoint_id: 'direct' }),
    ev('agent.usage', 't1', { input_tokens: 500, output_tokens: 100, cost_usd: 0.08 }, { agent: 'architect', span_id: 'n1', model_tier: 'frontier', model_id: 'opus', endpoint_id: 'direct' }),
    ev('council.stance', 't1', { role: 'architect', status: 'answered', text: 'A table, because of transactions.' }),
    ev('council.stance', 't1', { role: 'red-team', status: 'answered', text: 'A file: no lock contention.' }),
    ev('council.barrier.released', 't1', {}),
    ev('council.critique', 't1', { role: 'red-team', recommendation: 'file', concur: false, findings: [{ severity: 'P0', title: 'lock contention' }] }),
    ev('council.verdict', 't1', { escalated: 'seats split', options: [{ label: 'table', seats: ['architect'] }, { label: 'file', seats: ['red-team'] }], unresolved: [{ severity: 'P1', seat: 'red-team', title: 'contention unresolved' }], path: 'missions/m1/council.md' }),
    ev('task.created', 't2', { flow: 'build', prompt: 'build the queue' }),
    ev('build.attempt', 't2', { attempt: 1, tier: 'workhorse', family: 'claude' }),
    ev('worktree.created', 't2', { branch: 'delphi/t2' }),
    ev('gates.evaluated', 't2', { sha: 'abcdef0123456789', gates: [{ name: 'tests', state: 'green', detail: '41 passed' }, { name: 'clippy', state: 'red', detail: '2 warnings' }] }),
    ev('audit.recorded', 't2', { findings: [{ severity: 'P1', title: 'unbounded retry' }] }),
  ];
  for (const e of events) o.apply(e);
  o.setConnectors([{ name: 'drive', url: 'https://mcp.example/drive', account: 'a@b.c', login: 'google', roles: ['scout'], tools: [{ name: 'search', class: 'read', pinned: true }, { name: 'upload', class: 'write', pinned: false }] }]);
  o.apply(ev('connector.call', 't2', { connector: 'drive', tool: 'search' }));
  o.apply(ev('connector.result', 't2', { connector: 'drive', tool: 'search', is_error: false }));
  o.applyFrame({ spans: { n1: { text: 'Weighing a table against a file:\ntransactions favour the table.' } } });
  return o;
}

const note = (type: string, payload: any, task = 't1') => ({ method: 'event', params: { type, task_id: task, payload } });

/** A conversation: a question, tool calls, a long markdown answer, a card waiting, and a task still running. */
export function stateWorld(): State {
  let s = initial(0);
  s = fold(s, note('task.created', { prompt: 'How should the retry queue persist its jobs?' }), 1000);
  s = fold(s, note('dag.node.added', { node_id: 'n1', role: 'architect', depth: 0 }), 1000);
  s = fold(s, { method: 'event', params: { type: 'agent.block.complete', agent: 'architect', task_id: 't1', payload: { block: { type: 'tool_call', id: 'c1', name: 'read_file', arguments: { path: 'specs/002-adapter-and-routing.md' } } } } }, 1100);
  s = fold(s, note('tool.result', { tool_call_id: 'c1', content: '# Spec 002\nthe retry ladder', is_error: false }), 1200);
  s = fold(s, note('task.output', { text: '## Recommendation\n\nUse a **table**. It gives you:\n\n- transactions\n- one place to query\n\n```rust\nfn enqueue(job: Job) -> Result<()> { Ok(()) }\n```\n\nSee specs/002-adapter-and-routing.md#4 for the ladder.', status: 'done' }), 2000);
  s = fold(s, note('task.created', { prompt: 'Now write it' }, 't2'), 3000);
  s = fold(s, note('approval.requested', { approval_id: 'a1', role: 'builder', tier: 'amber', payload_blake3: 'h1', payload: { preview: '+ fn enqueue()\n+ fn dequeue()', args: { path: 'crates/queue.rs' } }, summary: 'Write crates/queue.rs (+84)', timeout_secs: 600, tool: 'write_file' }, 't2'), 3100);
  s = fold(s, note('approval.requested', { approval_id: 'r1', role: 'architect', tier: 'red', payload_blake3: 'h2', payload: { preview: 'ratify specs/011' }, summary: 'Ratify spec 011 into the canonical repository', timeout_secs: 600, tool: 'ratify' }, 't3'), 3200);
  s = addBlock(s, { kind: 'note', text: 'approval mode: default', at: 3300 });
  return s;
}

/** Compare against `test/snapshots/<name>.txt`. `UPDATE_SNAPSHOTS=1` writes it; a missing file otherwise fails. */
export function snapshot(name: string, lines: string[]): { ok: boolean; message: string } {
  const file = join(dirname(fileURLToPath(import.meta.url)), 'snapshots', `${name}.txt`);
  const text = lines.map((l) => plain(l).replace(/\s+$/, '')).join('\n') + '\n';
  if (process.env.UPDATE_SNAPSHOTS) { mkdirSync(dirname(file), { recursive: true }); writeFileSync(file, text); return { ok: true, message: '' }; }
  if (!existsSync(file)) return { ok: false, message: `no snapshot ${name}; run UPDATE_SNAPSHOTS=1 npm test and review the file` };
  const want = readFileSync(file, 'utf8');
  return want === text ? { ok: true, message: '' } : { ok: false, message: `snapshot ${name} differs (UPDATE_SNAPSHOTS=1 npm test to accept):\n--- want\n${want}\n--- got\n${text}` };
}
