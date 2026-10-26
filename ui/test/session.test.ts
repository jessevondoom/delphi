// Console sessions (`specs/030`): a new session empties the screen, a replay shows only the current one, `/clear` is
// `/new`, and the narrator says which session it is looking at.
import test from 'node:test';
import assert from 'node:assert/strict';
import { fold, initial, replay } from '../src/fold.ts';
import { lookup } from '../src/commands.ts';
import { emptyState, sessionNote } from '../src/observe-screen.ts';

const ev = (type: string, payload: any, task_id?: string) => ({ method: 'event', params: { type, payload, task_id } });

test('a new session empties the screen, keeps what is still running, and a replay shows only the current session', () => {
  let s = fold(initial(0), ev('task.created', { prompt: 'first question' }, 't_1'), 1);
  s = fold(s, ev('task.created', { prompt: 'still going' }, 't_2'), 2);
  s = fold(s, ev('task.output', { text: 'first answer', status: 'done' }, 't_1'), 3);
  assert.ok(s.blocks.length >= 2);
  s = fold(s, ev('session.started', { session_id: 's_0001', reason: 'new' }), 4);
  assert.deepEqual(s.blocks, [], 'the screen starts empty');
  assert.ok(s.running['t_2'], 'a task still running carries on');
  assert.equal(s.started, true, 'the welcome text does not come back');

  const events = [
    { type: 'task.created', task_id: 't_1', payload: { prompt: 'old session ask' } },
    { type: 'task.output', task_id: 't_1', payload: { text: 'old answer', status: 'done' } },
    { type: 'session.started', payload: { session_id: 's_0001', reason: 'restart' } },
    { type: 'task.created', task_id: 't_2', payload: { prompt: 'current ask' } },
  ];
  assert.deepEqual(replay(initial(0), events).blocks.map((b) => b.text), ['current ask']);
});

test('/clear is an alias of /new: a clear that leaves the team remembering would surprise', () => {
  assert.equal(lookup('/clear')?.name, '/new');
  assert.equal(lookup('/new')?.name, '/new');
});

test('the narrator names the session and counts earlier work that is still running', () => {
  const st = emptyState();
  assert.equal(sessionNote(st), '', 'before any session began there is nothing to say');
  st.sessions = [{ id: 's_0001', reason: 'new', started_ms: 0, turns: 4 }, { id: 's_0002', reason: 'new', started_ms: 1, turns: 1 }];
  st.tasks = {
    t_1: { id: 't_1', flow: 'orchestrator', prompt: 'a', status: 'running', session: 's_0001' },
    t_2: { id: 't_2', flow: 'orchestrator', prompt: 'b', status: 'running', session: 's_0002' },
  };
  assert.equal(sessionNote(st), 'session 2 · 1 ask · 1 from before');
});
