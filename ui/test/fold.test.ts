import test from 'node:test';
import assert from 'node:assert/strict';
import { fold, initial, status } from '../src/fold.ts';

const ev = (type: string, payload: any) => ({ method: 'event', params: { type, payload } });

test('a reply is spoken by the newest top-level agent and clears the live text', () => {
  let s = initial(0);
  s = fold(s, ev('dag.node.added', { node_id: 'n_1', role: 'orchestrator', depth: 0 }), 1);
  s = fold(s, ev('dag.node.added', { node_id: 'n_2', role: 'architect', depth: 1 }), 2);
  s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { text: 'hi \x1b]0;x\x07there' } } } }, 3);
  assert.equal(s.live, 'hi there', 'control sequences from a model are stripped');
  s = fold(s, ev('task.output', { text: '# Done', status: 'done' }), 4);
  assert.deepEqual([s.live, s.blocks[0]?.role, s.blocks[0]?.text], ['', 'orchestrator', '# Done']);
  assert.equal(s.agents.orchestrator?.state, 'idle');
});

test('a council keeps round one blind', () => {
  let s = fold(initial(0), ev('dag.node.added', { node_id: 'n_1', role: 'architect', depth: 1 }), 1);
  s = fold(s, ev('council.started', {}), 2);
  s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { text: 'secret stance' } } } }, 3);
  assert.equal(s.live, '');
  s = fold(s, ev('council.barrier.released', {}), 4);
  s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { text: 'now visible' } } } }, 5);
  assert.equal(s.live, 'now visible');
});

test('header status: alert beats activity; activity gives way to the strip after 30 seconds', () => {
  let s = fold(initial(0), ev('dag.node.added', { node_id: 'n_1', role: 'builder', depth: 0 }), 1000);
  assert.equal(status(s, 2000).kind, 'active');
  assert.equal(status(s, 40_000).kind, 'strip');
  s = fold(s, ev('approval.requested', { approval_id: 'a1', role: 'red-team', timeout_s: 600 }), 2000);
  assert.deepEqual(status(s, 62_000), { kind: 'alert', role: 'red-team', secs: 540 });
  s = fold(s, ev('approval.resolved', { approval_id: 'a1' }), 3000);
  assert.equal(status(s, 3500).kind, 'active');
});
