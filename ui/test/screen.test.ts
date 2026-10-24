import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { frame, header, footer, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { addBlock, fold, initial } from '../src/fold.ts';
import { badge, sanitize, type Info } from '../src/theme.ts';
import { completions } from '../src/complete.ts';

const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: {
    orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' },
    architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' },
    'red-team': { name: 'ares', chassis: '#ef4444', face: '-_-' },
  },
};
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const model = (over: Partial<Model> = {}): Model => ({ info, pal: { dark: true }, state: initial(0), now: 1000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null, queue: [], help: false, ...over });

test('every badge is five columns, in every state, dark and light', () => {
  for (const role of Object.keys(info.roles)) for (const st of ['idle', 'running', 'waiting on you', 'escalated'] as const) for (const dark of [true, false]) {
    assert.equal(stringWidth(badge(role, st, info, { dark })), 5, `${role} ${st}`);
  }
  assert.equal(plain(badge('orchestrator', 'running', info, { dark: true })), '⟦•─•⟧');
  assert.equal(plain(badge('red-team', 'waiting on you', info, { dark: true })), '⟦x?x⟧');
});

test('the screen is exactly w by h, with a hairline above and below the prompt and nothing boxed', () => {
  let s = initial(0);
  s = addBlock(s, { kind: 'user', text: 'hello', at: 0 });
  s = addBlock(s, { kind: 'reply', role: 'orchestrator', text: '# Plan\n\n- one\n- two\n\n```rust\nfn main() {}\n```', at: 0 });
  for (const [w, h] of [[100, 30], [60, 12], [40, 8]] as const) {
    const lines = frame(model({ state: s, composer: C.insert(C.empty(), 'draft') }), w, h);
    assert.equal(lines.length, h);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `line wider than ${w}: ${plain(l)}`);
    const text = lines.map(plain).join('\n');
    assert.ok(!/[┌┐└┘│]/.test(text), 'no box characters');
    assert.ok(text.includes('❯ draft'), 'the prompt is shown');
    assert.equal(lines.filter((l) => /^─+$/.test(plain(l))).length, 2, 'two hairlines');
  }
});

test('line one: active agent, then the alert with a countdown, then the idle strip', () => {
  let s = fold(initial(0), { method: 'event', params: { type: 'dag.node.added', payload: { node_id: 'n_1', role: 'orchestrator', depth: 0 } } }, 500);
  const l1 = (m: Model) => plain(header(m, 100)[0]!);
  assert.match(l1(model({ state: s, now: 1000 })), /^⟦•─•⟧ athena: working\s+navigate: shift\+arrows · ○ ● ○$/);
  assert.match(l1(model({ state: s, now: 60_000 })), /all agents idle/);
  s = fold(s, { method: 'event', params: { type: 'approval.requested', payload: { approval_id: 'a', role: 'red-team', timeout_s: 600 } } }, 1000);
  assert.match(l1(model({ state: s, now: 1000 })), /^⟦x\?x⟧ ares: decision needed · 10:00 remaining/);
  assert.match(plain(header(model(), 100)[1]!), /^delphi v0\.5 · \/proj$/);
});

test('footer has tokens, mode and shortcuts on the left, deck dots on the right, and no money', () => {
  const f = plain(footer(model({ state: { ...initial(0), tokens: 42100 } }), 100));
  assert.match(f, /^42\.1k tokens · approvals · shortcuts: \?\s+navigate: shift\+arrows · ○ ● ○$/);
  assert.ok(!f.includes('$'));
});

test('typing / opens the command menu above the prompt, filtered', () => {
  const lines = frame(model({ composer: C.insert(C.empty(), '/obs'), menu: { items: completions('/obs', 4, info, () => [])!.items, sel: 0 } }), 100, 30).map(plain);
  const hit = lines.findIndex((l) => l.includes('/observe · '));
  assert.ok(hit > 0 && hit < lines.findIndex((l) => l.startsWith('❯ /obs')), 'menu is above the prompt');
  assert.ok(!lines.join('\n').includes('/council · '), 'filtered out');
});

test('scrolled up, a marker says how many lines are below', () => {
  let s = initial(0);
  for (let i = 0; i < 30; i++) s = addBlock(s, { kind: 'note', text: `note ${i}`, at: 0 });
  const lines = frame(model({ state: s, back: 10 }), 80, 20).map(plain);
  assert.ok(lines.some((l) => /↓ 10 lines below/.test(l)));
});

test('model text cannot move the cursor or set the title', () => {
  assert.equal(sanitize('a\x1b[2Jb\x1b]0;evil\x07c\x1b[31md'), 'abcd');
});

test('while a task runs the status row shows a spinner, the time, the queue and how to stop', () => {
  const s = fold(initial(0), { method: 'event', params: { type: 'task.created', task_id: 't1', payload: { prompt: 'q' } } }, 1000);
  const lines = frame(model({ state: s, now: 13_000, queue: ['next thing'] }), 100, 30).map(plain);
  const row = lines.find((l) => l.includes('working')) ?? '';
  assert.match(row, /^[◐◓◑◒] working · 12s · 1 queued\s+esc to stop$/);
  assert.ok(lines.some((l) => l.includes('❯ next thing  (queued)')));
  assert.ok(!frame(model(), 100, 30).map(plain).some((l) => l.includes('working')), 'idle: no status row text');
});

test('tool calls are one dim line, and ctrl+o opens their output', () => {
  let s = initial(0);
  s = fold(s, { method: 'event', params: { type: 'agent.block.complete', agent: 'architect', payload: { block: { type: 'tool_call', id: 'c1', name: 'read_file', arguments: { path: 'specs/005.md' } } } } }, 1);
  s = fold(s, { method: 'event', params: { type: 'tool.result', payload: { tool_call_id: 'c1', is_error: false, content: 'line a\nline b' } } }, 2);
  const shut = frame(model({ state: s }), 100, 30).map(plain).join('\n');
  assert.match(shut, /→ read_file\(specs\/005\.md\) ✔/);
  assert.ok(!shut.includes('line a'));
  assert.ok(frame(model({ state: s, expandTools: true }), 100, 30).map(plain).join('\n').includes('line b'));
});

test('stopping ends the task, clears the spinner and says so', () => {
  let s = fold(initial(0), { method: 'event', params: { type: 'task.created', task_id: 't1', payload: { prompt: 'q' } } }, 1);
  assert.deepEqual(Object.keys(s.running), ['t1']);
  s = fold(s, { method: 'event', params: { type: 'task.cancelled', task_id: 't1', payload: {} } }, 2);
  assert.deepEqual([Object.keys(s.running), s.blocks.at(-1)?.text], [[], '■ stopped']);
});
