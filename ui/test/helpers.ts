// Shared fixtures for the golden-screen tests, and the golden-file helper.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fold, initial, type State } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

const crown: Record<string, string> = { orchestrator: '  ▄▄▄  ', architect: '▄▄▄▄▄▄▄', builder: '▄▄▄▄▄▄▄', 'red-team': '▄     ▄', strategist: '▄▄ ▄ ▄▄', scout: '   ▄   ' };
const mk = (name: string, chassis: string, face: string, role: string) => ({ name, chassis, face, crown: crown[role], crown_rgb: '#facc15' });
export const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: {
    orchestrator: mk('athena', '#a855f7', '•─•', 'orchestrator'), architect: mk('daedalus', '#38bdf8', '•_•', 'architect'), builder: mk('hephaestus', '#f97316', '•─•', 'builder'),
    'red-team': mk('ares', '#ef4444', '-_-', 'red-team'), strategist: mk('odysseus', '#22c55e', '•_•', 'strategist'), scout: mk('hermes', '#eab308', '•••', 'scout'),
  },
};
export const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

const note = (type: string, payload: any, task = 't1', extra: any = {}) => ({ method: 'event', params: { type, task_id: task, payload, ...extra } });

/** A conversation: a question, a tool call, a long markdown answer, two cards waiting and a task still running. */
export function stateWorld(): State {
  let s = initial(0);
  s = fold(s, note('task.created', { prompt: 'How should the retry queue persist its jobs?' }), 1000);
  s = fold(s, note('dag.node.added', { node_id: 'n1', role: 'architect', depth: 0 }), 1000);
  s = fold(s, note('agent.block.complete', { block: { type: 'tool_call', id: 'c1', name: 'read_file', arguments: { path: 'specs/002-adapter-and-routing.md' } } }, 't1', { agent: 'architect' }), 1100);
  s = fold(s, note('tool.result', { tool_call_id: 'c1', content: '# Spec 002\nthe retry ladder', is_error: false }), 1200);
  s = fold(s, note('task.output', { text: '## Recommendation\n\nUse a **table**. It gives you:\n\n- transactions\n- one place to query\n\n```rust\nfn enqueue(job: Job) -> Result<()> { Ok(()) }\n```\n\nSee specs/002-adapter-and-routing.md#4 for the ladder.', status: 'done' }), 2000);
  s = fold(s, note('task.created', { prompt: 'Now write it' }, 't2'), 3000);
  s = fold(s, note('approval.requested', { approval_id: 'a1', role: 'builder', tier: 'amber', payload_blake3: 'h1', payload: { preview: '+ fn enqueue()\n+ fn dequeue()', args: { path: 'crates/queue.rs' } }, summary: 'Write crates/queue.rs (+84)', timeout_secs: 600, tool: 'write_file' }, 't2'), 3100);
  s = fold(s, note('approval.requested', { approval_id: 'r1', role: 'architect', tier: 'red', payload_blake3: 'h2', payload: { preview: 'ratify specs/011' }, summary: 'Ratify spec 011 into the canonical repository', timeout_secs: 600, tool: 'ratify' }, 't3'), 3200);
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
