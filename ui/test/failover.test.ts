// Failover on the dashboard and in the chat (`specs/035` D-7): which models are out and until when, which tasks run on a stand-in and who
// agreed, nothing when all is well, and one line in the chat when a task moves.
import test from 'node:test';
import assert from 'node:assert/strict';
import { fold, initial } from '../src/fold.ts';


test('a save-point is announced in the chat, so an operator who is not asked still knows where the work is', () => {
  const s = fold(initial(0), { method: 'event', params: { type: 'checkpoint.saved', payload: { path: 'local/missions/m/checkpoints/001-t_0003.md\x1b[2J' } } }, 1);
  assert.equal(s.blocks[0]!.text, 'the work so far is saved for resumption: local/missions/m/checkpoints/001-t_0003.md');
});

test('a task that moves to another model is said once in the chat', () => {
  const s = fold(initial(0), { method: 'event', params: { type: 'model.promoted', payload: { from: { endpoint_id: 'direct', model_id: 'opus-x' }, to: { endpoint_id: 'ollama', model_id: 'gemma4' }, reason: 'x' } } }, 1);
  assert.equal(s.blocks.length, 1);
  assert.equal(s.blocks[0]!.text, 'continuing on ollama/gemma4 (direct/opus-x is not available)');
});
