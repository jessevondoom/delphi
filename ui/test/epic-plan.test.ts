import test from 'node:test';
import assert from 'node:assert/strict';
import { plan } from '../src/epic-plan.ts';
import { epicWorld, NOW, T0 } from './epic-fixture.ts';

const min = (n: number) => T0 + n * 60_000;

test('estimates: the median duration of finished work of the same role, else three minutes', () => {
  const st = epicWorld();
  const node = (id: string, role: string, state: string, a: number, b: number | null, after: string[]) => ({ id, role, parent: null, depth: 0, task_id: 't_1', state, tokens: 0, cost_usd: 0, started_ms: min(a), ended_ms: b === null ? null : min(b), after });
  st.nodes = {
    a: node('a', 'strategist', 'done', 0, 6, []),
    b: node('b', 'strategist', 'done', 6, 10, ['a']),
    c: node('c', 'strategist', 'pending', 10, null, ['b']),
    d: node('d', 'scout', 'pending', 10, null, ['b']),
  };
  const items = Object.fromEntries(plan(st, min(10)).map((i) => [i.node.id, i]));
  assert.equal(items.c!.end - items.c!.start, 5 * 60_000, 'median of 6 and 4 minutes');
  assert.equal(items.d!.end - items.d!.start, 3 * 60_000, 'no finished scout: three minutes');
  assert.equal(items.c!.start, min(10), 'a queued task starts when its predecessor ends, never in the past');
});

test('the critical path is the longest chain to the end, from running or queued work only', () => {
  const items = plan(epicWorld(), NOW);
  const crit = items.filter((i) => i.critical).map((i) => i.node.id);
  assert.deepEqual(crit.sort(), ['n3', 'n4', 'n5']);
  assert.ok(!items.find((i) => i.node.id === 'n1')!.critical, 'finished work is not on it');
});
