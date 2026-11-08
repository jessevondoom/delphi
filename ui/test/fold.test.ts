import test from 'node:test';
import assert from 'node:assert/strict';
import { fold, initial } from '../src/fold.ts';

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

test('a reasoning delta marks the agent as thinking, and a finished task marks it for success', () => {
  let s = fold(initial(0), ev('dag.node.added', { node_id: 'n_1', role: 'builder', depth: 0 }), 1000);
  assert.equal(s.agents['builder']!.thinkingAt, undefined);
  s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { thinking: 'hmm' } } } }, 2000);
  assert.equal(s.agents['builder']!.thinkingAt, 2000);
  assert.equal(s.live, '', 'reasoning is never shown, only noted');
  s = fold(s, ev('task.output', { status: 'done', text: 'ok' }), 5000);
  assert.deepEqual([s.agents['builder']!.state, s.agents['builder']!.successAt], ['idle', 5000]);
});

test('the live text is bounded and keeps the newest words (spec 093 L1)', () => {
  let s = fold(initial(0), ev('dag.node.added', { node_id: 'n_1', role: 'architect', depth: 0 }), 1);
  // A harness seat streams for a long time; the console keeps only the tail, in memory.
  for (let i = 0; i < 30; i++) s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { text: 'x'.repeat(1000) } } } }, 2 + i);
  assert.equal(s.live.length, 8000, 'the live text never grows past its bound');
  s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { text: 'THE-NEWEST-WORDS' } } } }, 100);
  assert.equal(s.live.length, 8000);
  assert.ok(s.live.endsWith('THE-NEWEST-WORDS'), 'the newest words are kept, the oldest dropped');
});

test('a frame that arrives after its reply was spoken does not bring the reply back as live text (spec 093 L1)', () => {
  let s = fold(initial(0), ev('dag.node.added', { node_id: 'n_1', role: 'architect', depth: 0 }), 1);
  s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { text: 'the answer' } } } }, 2);
  s = fold(s, ev('task.output', { text: 'the answer', status: 'done' }), 3);
  assert.equal(s.live, '');
  // Frames and events travel on different sockets, so a last frame can land after the task's last event.
  s = fold(s, { method: 'delta.frame', params: { spans: { n_1: { text: 'the answer' } } } }, 4);
  assert.equal(s.live, '', 'the finished reply is in a block already; it is not shown twice');
});

test('the weekly research digest is one reply from the orchestrator, with nothing to answer', () => {
  const s = fold(initial(0), { method: 'event', params: { type: 'research.digest', agent: 'orchestrator', payload: { text: '5 research pages are waiting outside the knowledge base: 5 held in the default scope. /validate lists them; nothing is added until you ratify it.' } } }, 0);
  assert.equal(s.blocks.length, 1);
  assert.deepEqual([s.blocks[0]!.kind, s.blocks[0]!.role], ['reply', 'orchestrator']);
  assert.match(s.blocks[0]!.text, /^5 research pages are waiting/);
});
