// The activity deck as a ticker with finished history (`specs/005e` §8, slice 5 of 005f).
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import stringWidth from 'string-width';
import * as C from '../src/composer.ts';
import { fold, HISTORY_MAX, initial, replay, type State } from '../src/fold.ts';
import { newInbox } from '../src/inbox.ts';
import { frame, type Model } from '../src/screen.ts';
import * as T from '../src/theme.ts';
import { info, plain, snapshot } from './helpers.ts';

const T0 = Date.parse('2026-10-25T09:00:00Z');
const ev = (type: string, payload: any, task = 't_1', extra: any = {}) => ({ method: 'event', params: { type, task_id: task, payload, ...extra } });
const delta = (span: string, d: { thinking?: string; text?: string }) => ({ method: 'delta.frame', params: { spans: { [span]: d } } });
const step = (s: State, n: any, at: number) => fold(s, n, T0 + at);

const model = (state: State, over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true, bg: [15, 23, 42] }, state, now: T0 + 60_000, deck: 0, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'mission-auto', panel: null, help: false, still: true, ...over,
});
const lines = (m: Model, w: number, h: number) => frame(m, w, h).map(plain);
const check = (name: string, m: Model, w: number, h: number) => { const r = snapshot(name, frame(m, w, h)); assert.ok(r.ok, r.message); };

/** Three spans have finished and three are at work: the conversation of the §8 mockups. */
function world(): State {
  let s = initial(0);
  s = step(s, ev('task.created', { prompt: 'table or file?' }, 't_1'), 0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_architect_1', role: 'architect', depth: 0 }, 't_1'), 0);
  s = step(s, ev('dag.node.state', { node_id: 'sp_architect_1', state: 'done' }, 't_1'), 4000);
  s = step(s, ev('task.created', { prompt: 'implement it' }, 't_2'), 10_000);
  s = step(s, ev('dag.node.added', { node_id: 'sp_builder_2', role: 'builder', depth: 0 }, 't_2'), 10_000);
  s = step(s, ev('agent.block.complete', { block: { type: 'tool_call', id: 'w1', name: 'write_file', arguments: { path: 'crates/queue.rs' } } }, 't_2', { span_id: 'sp_builder_2', agent: 'builder' }), 15_000);
  s = step(s, ev('tool.result', { tool_call_id: 'w1', content: 'ok', is_error: false }, 't_2', { span_id: 'sp_builder_2' }), 16_000);
  s = step(s, ev('dag.node.state', { node_id: 'sp_builder_2', state: 'done' }, 't_2'), 22_000);
  s = step(s, ev('dag.node.added', { node_id: 'sp_red-team_3', role: 'red-team', depth: 1 }, 't_2'), 23_000);
  s = step(s, ev('dag.node.state', { node_id: 'sp_red-team_3', state: 'done' }, 't_2'), 44_000);
  // at work now
  s = step(s, ev('task.created', { prompt: 'check the design' }, 't_3'), 48_000);
  s = step(s, ev('task.created', { prompt: 'read the spec' }, 't_4'), 49_000);
  s = step(s, ev('task.created', { prompt: 'write the claim loop' }, 't_5'), 50_000);
  s = step(s, ev('dag.node.added', { node_id: 'sp_orchestrator_4', role: 'orchestrator', depth: 0 }, 't_3'), 48_000);
  s = step(s, ev('dag.node.added', { node_id: 'sp_architect_5', role: 'architect', depth: 1 }, 't_4'), 49_000);
  s = step(s, ev('dag.node.added', { node_id: 'sp_builder_6', role: 'builder', depth: 1 }, 't_5'), 56_000);
  s = step(s, delta('sp_orchestrator_4', { thinking: 'The question is whether claims need transactional guarantees. A table gives row locks for free; a file needs a lock file and careful fsync ordering. Leaning table, but check what 003 already decided about the audit trail before committing to it.' }), 52_000);
  s = step(s, ev('agent.block.complete', { block: { type: 'tool_call', id: 'r1', name: 'read_file', arguments: { path: 'specs/003-collaboration-and-council.md' } } }, 't_4', { span_id: 'sp_architect_5', agent: 'architect' }), 57_000);
  s = step(s, delta('sp_builder_6', { text: 'fn claim(id: u64) -> Result<(), Error> {\n    let row = tx.select_for_update(id)?;' }), 58_000);
  return { ...s, tokens: 12_400 };
}

// ── the fold ───────────────────────────────────────────────────────────────────────────

test('a span that ends leaves the running zone and joins the history in the same step', () => {
  let s = initial(0);
  s = step(s, ev('task.created', { prompt: 'table or file?' }), 0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_architect_1', role: 'architect', depth: 0 }), 0);
  assert.deepEqual([s.activity.running.length, s.activity.history.length], [1, 0]);
  s = step(s, ev('dag.node.state', { node_id: 'sp_architect_1', state: 'done' }), 4000);
  assert.deepEqual([s.activity.running.length, s.activity.history.length], [0, 1], 'one step: gone from running, in the history');
  assert.deepEqual(s.activity.history[0], { id: 1, role: 'architect', what: 'answered "table or file?"', elapsed: 4000, at: T0 + 4000, ok: true });
});

test('what a span did: the file it wrote, what it answered, the file it read', () => {
  const w = world().activity.history;
  assert.deepEqual(w.map((d) => [d.role, d.what, d.elapsed]), [['architect', 'answered "table or file?"', 4000], ['builder', 'wrote crates/queue.rs', 12_000], ['red-team', 'finished', 21_000]]);
  let s = initial(0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_scout_1', role: 'scout', depth: 1 }), 0);
  s = step(s, ev('agent.block.complete', { block: { type: 'tool_call', id: 'x', name: 'read_file', arguments: { path: 'specs/002.md' } } }, 't_1', { span_id: 'sp_scout_1' }), 100);
  s = step(s, ev('dag.node.state', { node_id: 'sp_scout_1', state: 'done' }), 200);
  assert.equal(s.activity.history[0]!.what, 'read specs/002.md');
  s = step(s, ev('task.created', { prompt: 'a very long question that goes on and on and on' }, 't_9'), 300);
  s = step(s, ev('dag.node.added', { node_id: 'sp_architect_2', role: 'architect', depth: 0 }, 't_9'), 300);
  s = step(s, ev('dag.node.state', { node_id: 'sp_architect_2', state: 'done' }, 't_9'), 400);
  assert.equal(s.activity.history[1]!.what, 'answered "a very long question that go…"');
});

test('a span that stopped needs the operator: it joins the history as stopped, not finished', () => {
  let s = initial(0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_builder_1', role: 'builder', depth: 0 }), 0);
  s = step(s, ev('dag.node.state', { node_id: 'sp_builder_1', state: 'escalated', reason: 'x' }), 1000);
  assert.deepEqual([s.activity.history[0]!.ok, s.activity.history[0]!.what], [false, 'needs you']);
});

test('a task that ends or is stopped takes its running spans with it, without a chip', () => {
  let s = initial(0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_a_1', role: 'architect', depth: 0 }, 't_1'), 0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_b_2', role: 'builder', depth: 0 }, 't_2'), 0);
  s = step(s, ev('task.output', { status: 'done', text: 'x' }, 't_1'), 10);
  assert.deepEqual(s.activity.running.map((r) => r.span), ['sp_b_2']);
  s = step(s, ev('task.cancelled', {}, 't_2'), 20);
  assert.deepEqual([s.activity.running.length, s.activity.history.length], [0, 0]);
});

test('the history keeps the newest 500 and the running text keeps its tail', () => {
  let s = initial(0);
  for (let i = 1; i <= HISTORY_MAX + 7; i++) {
    s = step(s, ev('dag.node.added', { node_id: `sp_a_${i}`, role: 'architect', depth: 1 }), i * 10);
    s = step(s, ev('dag.node.state', { node_id: `sp_a_${i}`, state: 'done' }), i * 10 + 5);
  }
  assert.equal(s.activity.history.length, HISTORY_MAX);
  assert.equal(s.activity.history[0]!.id, 8, 'the oldest seven are gone');
  assert.equal(s.activity.history[HISTORY_MAX - 1]!.id, HISTORY_MAX + 7);
  s = step(s, ev('dag.node.added', { node_id: 'sp_b_1', role: 'builder', depth: 1 }), 99_999);
  for (let i = 0; i < 100; i++) s = fold(s, delta('sp_b_1', { text: 'x'.repeat(100) }), 100_000 + i);
  assert.ok(s.activity.running[0]!.text.length <= 4000);
});

test('a council\'s blind round keeps no text, only the fact that it is thinking', () => {
  let s = initial(0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_a_1', role: 'architect', depth: 1 }), 0);
  s = step(s, ev('council.started', {}), 1);
  s = step(s, delta('sp_a_1', { thinking: 'secret stance', text: 'OPTION: table' }), 2);
  const r = s.activity.running[0]!;
  assert.deepEqual([r.text, r.thinking, r.blind], ['', '', true]);
  assert.ok(!lines(model(s), 100, 24).join('\n').includes('table'));
  assert.match(lines(model(s), 100, 24).join('\n'), /daedalus · thinking · \d\d:\d\d/);
});

test('a failed tool call shows its first line under the span until the next one', () => {
  let s = initial(0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_b_1', role: 'builder', depth: 1 }), 0);
  s = step(s, ev('agent.block.complete', { block: { type: 'tool_call', id: 'c', name: 'run_command', arguments: { path: 'x' } } }, 't_1', { span_id: 'sp_b_1' }), 1);
  s = step(s, ev('tool.result', { tool_call_id: 'c', content: 'exit 1: no such file\nmore', is_error: true }, 't_1', { span_id: 'sp_b_1' }), 2);
  assert.match(lines(model(s), 100, 24).join('\n'), /✖ exit 1: no such file/);
  assert.ok(!lines(model(s), 100, 24).join('\n').includes('\nmore'));
});

// ── the screen ───────────────────────────────────────────────────────────────────────────

test('golden: history above, running spans below (005e 8.2)', () => {
  check('activity-100x34', model(world()), 100, 34);
  check('activity-80x24', model(world()), 80, 24);
});

test('golden: scrolled, with earlier entries above; empty; and without colour', () => {
  let s = world();
  for (let i = 0; i < 12; i++) {
    s = step(s, ev('dag.node.added', { node_id: `sp_extra_${i}`, role: 'strategist', depth: 1 }, 't_x'), 100_000 + i * 100);
    s = step(s, ev('dag.node.state', { node_id: `sp_extra_${i}`, state: 'done' }, 't_x'), 100_050 + i * 100);
  }
  check('activity-earlier-80x24', model(s, { now: T0 + 130_000 }), 80, 24);
  check('activity-scrolled-80x24', model(s, { now: T0 + 130_000, activityBack: 6 }), 80, 24);
  check('activity-empty-80x24', model(initial(0)), 80, 24);
  const prev = T.currentCapabilities();
  T.useCapabilities(T.capabilities({ NO_COLOR: '1' }));
  try { check('activity-nocolor-80x24', model(world()), 80, 24); } finally { T.useCapabilities(prev); }
});

test('the chip: green with the terminal\'s own background as its text; [✔ finished] without colour; never in the chat', () => {
  const prev = T.currentCapabilities(), lvl = chalk.level;
  T.useCapabilities(T.capabilities({ TERM: 'xterm' }));
  try {
    const row = frame(model(world()), 100, 34).find((l) => plain(l).includes('finished'))!;
    assert.match(row, /\x1b\[32m\x1b\[7m|\x1b\[7m\x1b\[32m|\x1b\[32;7m/, 'green, reversed');
    assert.ok(plain(row).includes(' ✔ finished '));
  } finally { T.useCapabilities(prev); chalk.level = lvl; }
  T.useCapabilities(T.capabilities({ NO_COLOR: '1' }));
  try { assert.ok(lines(model(world()), 100, 34).some((l) => l.includes('[✔ finished]'))); } finally { T.useCapabilities(prev); }
  assert.ok(!lines(model(world(), { deck: 1 }), 100, 34).join('\n').includes('finished'), 'not in the chat deck');
});

test('a running span shows at most six lines, the last six, wrapped to the window width less six', () => {
  let s = initial(0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_o_1', role: 'orchestrator', depth: 0 }), 0);
  const long = Array.from({ length: 30 }, (_, i) => `line ${i + 1} ` + 'word '.repeat(12)).join('\n');
  s = step(s, delta('sp_o_1', { thinking: long }), 1);
  for (const w of [100, 80, 48, 28]) {
    const rows = lines(model(s), w, 60);
    const title = rows.findIndex((l) => l.includes('thinking'));
    assert.ok(title > 0, `${w}`);
    let n = 0;
    for (let i = title + 1; i < rows.length && rows[i]!.startsWith('    ') && rows[i]!.trim() !== ''; i++) { n++; assert.ok(stringWidth(rows[i]!) <= w - 2, `${w}: ${rows[i]}`); }
    assert.equal(n, 6, `${w}: six lines`);
    assert.ok(rows.join('\n').includes('line 30'), `${w}: the newest text`);
    assert.ok(!rows.join('\n').includes('line 1 '), `${w}: the oldest is gone`);
  }
});

test('the running zone stays in reach: following shows the end, with ↑ N earlier above; scrolling shows ↓ N lines below', () => {
  let s = world();
  for (let i = 0; i < 12; i++) {
    s = step(s, ev('dag.node.added', { node_id: `sp_extra_${i}`, role: 'strategist', depth: 1 }, 't_x'), 100_000 + i * 100);
    s = step(s, ev('dag.node.state', { node_id: `sp_extra_${i}`, state: 'done' }, 't_x'), 100_050 + i * 100);
  }
  const following = lines(model(s), 80, 24);
  assert.match(following.join('\n'), /↑ \d+ earlier · pgup/);
  assert.ok(following.join('\n').includes('hephaestus · writing'), 'the running zone is on screen');
  assert.ok(!following.join('\n').includes('lines below'));
  const scrolled = lines(model(s, { activityBack: 8 }), 80, 24);
  assert.match(scrolled.join('\n'), /↓ 8 lines below · pgdn/);
  assert.ok(!scrolled.join('\n').includes('hephaestus · writing'), 'scrolled up, the end is out of view');
  const huge = lines(model(s, { activityBack: 9999 }), 80, 24);
  assert.equal(huge.length, 24, 'an offset past the start is clamped');
});

test('the deck and the chat keep separate offsets; history survives a resize', () => {
  const s = world();
  const a = lines(model(s, { back: 50, activityBack: 0 }), 100, 34).join('\n');
  assert.ok(a.includes('wrote crates/queue.rs'), 'the chat\'s offset does not move the deck');
  for (const [w, h] of [[100, 34], [80, 24], [60, 20], [40, 16]] as const) {
    const out = lines(model(s), w, h);
    assert.equal(out.length, h);
    for (const l of out) assert.ok(stringWidth(l) <= w, `${w}x${h}: ${l}`);
  }
});

test('a streaming frame in flight while a resync reloads the history does not bring a finished reply back as a live one', () => {
  const past = [
    { type: 'task.created', task_id: 't_1', payload: { prompt: 'hi' } },
    { type: 'dag.node.added', task_id: 't_1', payload: { node_id: 'sp_architect_1', role: 'architect', depth: 0 } },
    { type: 'task.output', task_id: 't_1', payload: { status: 'done', text: 'Understood.' } },
  ];
  const reloaded = replay(initial(0), past);
  const late = step(reloaded, delta('sp_architect_1', { text: 'Understood.' }), 5);
  assert.equal(late.live, '', 'no live block');
  assert.equal(late.tokens, reloaded.tokens, 'and no tokens counted twice');
  assert.ok(!lines(model(late, { deck: 1 }), 100, 24).join('\n').includes('writing'));
  // A failed or stopped task is over too.
  for (const end of ['task.failed', 'task.cancelled']) {
    const s = replay(initial(0), [past[0]!, past[1]!, { type: end, task_id: 't_1', payload: { error: 'x' } }]);
    assert.equal(step(s, delta('sp_architect_1', { text: 'x' }), 5).live, '', end);
  }
  // A task that had not finished when the history was reloaded is still streaming, and keeps doing so.
  const open = replay(initial(0), [past[0]!, past[1]!]);
  assert.equal(step(open, delta('sp_architect_1', { text: 'more' }), 5).live, 'more');
  // Another task's spans are unaffected by the first one ending.
  const two = replay(initial(0), [...past, { type: 'dag.node.added', task_id: 't_2', payload: { node_id: 'sp_b_2', role: 'builder', depth: 0 } }]);
  assert.equal(step(two, delta('sp_b_2', { text: 'still going' }), 5).live, 'still going');
});

test('replay does not bring the deck back, and the empty deck says what it is for', () => {
  assert.deepEqual(replay(world(), []).activity, { running: [], history: [] });
  assert.match(lines(model(initial(0)), 100, 24).join('\n'), /nothing running/);
});

test('a streaming frame that arrives after its task is over does not bring the reply back as a live one', () => {
  let s = initial(0);
  s = step(s, ev('task.created', { prompt: 'hi' }), 0);
  s = step(s, ev('dag.node.added', { node_id: 'sp_architect_1', role: 'architect', depth: 0 }), 0);
  s = step(s, delta('sp_architect_1', { text: 'Understood.' }), 1);
  assert.equal(s.live, 'Understood.');
  s = step(s, ev('task.output', { status: 'done', text: 'Understood.' }), 2);
  assert.equal(s.live, '');
  const late = step(s, delta('sp_architect_1', { text: 'Understood.' }), 3); // the frame crossed the event on the way
  assert.equal(late.live, '', 'no live block');
  assert.equal(late.tokens, s.tokens, 'and no tokens counted twice');
  assert.ok(!lines(model(late, { deck: 1 }), 100, 24).join('\n').includes('writing'));
  // the same when the span's own end event comes first
  let t = step(initial(0), ev('dag.node.added', { node_id: 'sp_b_1', role: 'builder', depth: 1 }), 0);
  t = step(t, ev('dag.node.state', { node_id: 'sp_b_1', state: 'done' }), 1);
  assert.equal(step(t, delta('sp_b_1', { text: 'late' }), 2).live, '');
  // another span of the same task is unaffected
  let u = step(initial(0), ev('dag.node.added', { node_id: 'sp_a_1', role: 'architect', depth: 0 }, 't_1'), 0);
  u = step(u, ev('dag.node.added', { node_id: 'sp_a_2', role: 'architect', depth: 0 }, 't_2'), 0);
  u = step(u, ev('task.output', { status: 'done', text: 'x' }, 't_1'), 1);
  assert.equal(step(u, delta('sp_a_2', { text: 'still going' }), 2).live, 'still going');
});
