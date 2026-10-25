import test from 'node:test';
import assert from 'node:assert/strict';
import { Activity, status, title } from '../src/title.ts';

const ev = (type: string, mission: string, task: string, payload: any = {}) => ({ method: 'event', params: { type, mission_id: mission, task_id: task, payload } });

test('the status line: idle, agents and missions, or what needs the operator', () => {
  assert.equal(status(0, 0, 0), 'idle');
  assert.equal(status(6, 3, 0), '6 agents working, 3 missions');
  assert.equal(status(1, 1, 0), '1 agent working, 1 mission');
  assert.equal(status(6, 3, 2), '6 agents working, 2 need your attention');
  assert.equal(status(2, 1, 1), '2 agents working, 1 needs your attention');
  assert.equal(status(0, 0, 1), '1 needs your attention');
});

test('the title carries the mark and the version', () => {
  assert.equal(title('0.2.6', new Activity()), '⟦•─•⟧ delphi v0.2.6: idle');
  assert.equal(title('dev', new Activity()), '⟦•─•⟧ delphi dev: idle');
});

test('counts follow the events of every mission', () => {
  const a = new Activity();
  assert.equal(a.feed(ev('task.created', 'm1', 't1')), true);
  a.feed(ev('dag.node.added', 'm1', 't1', { node_id: 'n1', role: 'architect' }));
  a.feed(ev('dag.node.added', 'm1', 't1', { node_id: 'n2', role: 'builder' }));
  a.feed(ev('task.created', 'm2', 't2'));
  a.feed(ev('dag.node.added', 'm2', 't2', { node_id: 'n3', role: 'builder' }));
  assert.equal(title('0.2.6', a), '⟦•─•⟧ delphi v0.2.6: 3 agents working, 2 missions');
  a.feed(ev('approval.requested', 'm2', 't2', { approval_id: 'a1' }));
  assert.equal(title('0.2.6', a), '⟦•─•⟧ delphi v0.2.6: 3 agents working, 1 needs your attention');
  a.feed(ev('approval.resolved', 'm2', 't2', { approval_id: 'a1' }));
  a.feed(ev('dag.node.state', 'm1', 't1', { node_id: 'n1', state: 'done' }));
  assert.equal(a.agents, 2);
  a.feed(ev('task.output', 'm1', 't1', { status: 'done' }));
  assert.deepEqual([a.agents, a.missions], [1, 1]);
  a.feed(ev('task.cancelled', 'm2', 't2'));
  assert.equal(title('0.2.6', a), '⟦•─•⟧ delphi v0.2.6: idle');
});

test('events that change nothing, and a reset, are handled', () => {
  const a = new Activity();
  assert.equal(a.feed({ method: 'frame', params: {} }), false);
  assert.equal(a.feed(ev('agent.block.complete', 'm1', 't1')), false);
  a.feed(ev('approval.requested', 'm1', 't1', { approval_id: 'a1' }));
  a.reset();
  assert.equal(a.waiting, 0);
});
