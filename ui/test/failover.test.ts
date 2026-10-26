// Failover on the dashboard and in the chat (`specs/035` D-7): which models are out and until when, which tasks run on a stand-in and who
// agreed, nothing when all is well, and one line in the chat when a task moves.
import test from 'node:test';
import assert from 'node:assert/strict';
import { emptyArch } from '../src/archives.ts';
import { failover } from '../src/dashboard.ts';
import { fold, initial } from '../src/fold.ts';
import { observeFrame, emptyState, type OModel, type OState } from '../src/observe-screen.ts';
import { epicWorld, info, NOW } from './epic-fixture.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const model = (st: OState): OModel => ({ info, pal: { dark: true }, st, view: 'dashboard', pick: 0, scroll: 0, selNode: 0, arch: emptyArch(), live: {}, connected: true, still: true, now: NOW } as OModel);

function world(): OState {
  const s = epicWorld();
  s.models_out = [{ endpoint_id: 'direct', model_id: 'opus-x', reason: 'quota reached', back_ms: NOW + 2 * 3600_000 + 12 * 60_000 }, { endpoint_id: 'old', model_id: 'sonnet-x', reason: 'quota reached', back_ms: NOW - 1000 }];
  s.standins = [
    { task: 't_3', role: 'architect', from: 'direct/opus-x', to: 'ollama/gemma4', why: '', by: 'policy' },
    { task: 't_4', role: 'builder', from: 'direct/sonnet-x', to: 'gem/gem-flash', why: '', by: 'operator' },
    { task: 't_5', role: 'scout', from: 'direct/opus-x', to: 'gem/gem-pro', why: '', by: 'automatic' },
  ];
  return s;
}

test('the dashboard names the models that are out, when they return, and who agreed to each stand-in', () => {
  const rows = failover(model(world()), 120).map(plain);
  assert.equal(rows.length, 4, 'one model out (the one whose reset time has passed is no longer listed) and three stand-ins');
  assert.match(rows[0]!, /✖ direct\/opus-x +quota reached · back in 2h 12m/);
  assert.match(rows[1]!, /⇄ .*\(t_3\) on ollama\/gemma4 instead of direct\/opus-x · to finish the task, as your policy allows/);
  assert.match(rows[2]!, /\(t_4\) on gem\/gem-flash instead of direct\/sonnet-x · you agreed/);
  assert.match(rows[3]!, /\(t_5\) on gem\/gem-pro .* · an equal model, no question asked/);
  const frame = observeFrame(model(world()), 100, 40).map(plain).join('\n');
  assert.match(frame, /── models ─+\n.*✖ direct\/opus-x/, 'it is a section of the dashboard');
});

test('nothing is shown when every model is up, and a hostile reason cannot widen the screen', () => {
  assert.deepEqual(failover(model(epicWorld()), 100), []);
  assert.doesNotMatch(observeFrame(model(emptyState()), 100, 30).map(plain).join('\n'), /── models/);
  const st = world();
  st.models_out![0]!.reason = 'x'.repeat(400);
  for (const l of failover(model(st), 60)) assert.ok(plain(l).length <= 62, String(plain(l).length));
});

test('a task that moves to another model is said once in the chat', () => {
  const s = fold(initial(0), { method: 'event', params: { type: 'model.promoted', payload: { from: { endpoint_id: 'direct', model_id: 'opus-x' }, to: { endpoint_id: 'ollama', model_id: 'gemma4' }, reason: 'x' } } }, 1);
  assert.equal(s.blocks.length, 1);
  assert.equal(s.blocks[0]!.text, 'continuing on ollama/gemma4 (direct/opus-x is not available)');
});
