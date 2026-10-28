// Console sessions (`specs/030`): a new session empties the screen, a replay shows only the current one, `/clear` is
// `/new`, and the narrator header says nothing of sessions.
import test from 'node:test';
import assert from 'node:assert/strict';
import { addBlock, fold, initial, replay } from '../src/fold.ts';
import { lookup } from '../src/commands.ts';
import { emptyState, observeFrame, VIEWS, type OModel } from '../src/observe-screen.ts';
import { info } from './epic-fixture.ts';

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

test('a session announced again, as on a reconnect after a daemon restart, does not wipe what was added since', () => {
  // The console rebuilds from the log, which already holds the restart's announcement, and says it reconnected...
  const log = [
    { type: 'task.created', task_id: 't_1', payload: { prompt: 'before the restart' } },
    { type: 'task.output', task_id: 't_1', payload: { text: 'old answer', status: 'done' } },
    { type: 'session.started', payload: { session_id: 's_0002', reason: 'restart' } },
  ];
  let s = replay(initial(0), log);
  assert.equal(s.session, 2);
  s = addBlock(s, { kind: 'note', text: 'reconnected to the daemon', at: 5 });
  // ...and then the same announcement arrives on the live stream. Whichever of the two comes first, the note stays.
  const live = ev('session.started', { session_id: 's_0002', reason: 'restart' });
  const after = fold(s, live, 6);
  assert.deepEqual(after.blocks.map((b) => b.text), ['reconnected to the daemon']);
  const live_first = addBlock(fold(replay(initial(0), log.slice(0, 2)), live, 4), { kind: 'note', text: 'reconnected to the daemon', at: 5 });
  assert.deepEqual(live_first.blocks.map((b) => b.text), ['reconnected to the daemon'], 'announced before the history was rebuilt, the screen is cleared once and the note is added after');
});

test('a newer session, or one announced with no id, still empties the screen', () => {
  let s = fold(initial(0), ev('task.created', { prompt: 'a' }, 't_1'), 1);
  s = fold(s, ev('session.started', { session_id: 's_0001' }), 2);
  s = fold(s, ev('task.created', { prompt: 'b' }, 't_2'), 3);
  assert.equal(s.blocks.length, 1);
  assert.deepEqual(fold(s, ev('session.started', { session_id: 's_0002' }), 4).blocks, [], 'the next session clears');
  const local = fold(s, ev('session.started', {}), 4);
  assert.deepEqual([local.blocks, local.session], [[], 2], 'the console clears at once when it asks for /new, before the daemon answers');
  assert.deepEqual(fold(local, ev('session.started', { session_id: 's_0002' }), 5).blocks, [], 'and the daemon\'s own announcement of that session is then a no-op');
});

test('/clear is an alias of /new: a clear that leaves the team remembering would surprise', () => {
  assert.equal(lookup('/clear')?.name, '/new');
  assert.equal(lookup('/new')?.name, '/new');
});

test('the narrator header carries no session number and no ask count, on any page', () => {
  const st = emptyState();
  st.sessions = [{ id: 's_0001', reason: 'new', started_ms: 0, turns: 4 }, { id: 's_0002', reason: 'new', started_ms: 1, turns: 1 }];
  st.tasks = { t_1: { id: 't_1', flow: 'orchestrator', prompt: 'a', status: 'running', session: 's_0001' } };
  for (const view of VIEWS) {
    const head = observeFrame({ info, pal: { dark: true }, st, view, pick: 0, scroll: 0, selNode: 0, live: {}, connected: true, still: true, now: 5 } as OModel, 100, 30).slice(0, 2).map((l) => l.replace(/\x1b\[[0-9;]*m/g, '')).join('\n');
    assert.ok(!/session \d|\d+ asks?|from before/.test(head), `${view}: ${head}`);
    assert.match(head, new RegExp(`delphi v.* · ${view}`));
  }
});
