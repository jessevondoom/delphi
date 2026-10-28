// agents, archives and changes in the narrator's style (`005f` slice 10): lowercase section rules, a tinted
// band for the running row, no money, no columns the operator cannot act on. Goldens at 80x24, 100x34 and 40x12.
import test from 'node:test';
import { emptyArch } from '../src/archives.ts';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { observeFrame, VIEWS, type OModel, type OState } from '../src/observe-screen.ts';
import { epicWorld, info, NOW } from './epic-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

function world(): OState {
  const s = epicWorld();
  s.agents['builder']!.tokens = { input: 6200, cache_read: 3100, cache_write: 0, output: 2400, thinking: 900 };
  s.agents['builder']!.model = 'claude-sonnet-x';
  s.agents['builder']!.tier = 'workhorse';
  s.agents['red-team']!.state = 'waiting on you';
  s.builds = [{ task_id: 't_1', branch: 'delphi/t_1', attempts: ['attempt 1: tests red, fixed'], sha: 'abc1234', gates: [{ name: 'tests', state: 'green', detail: '12 passed' }, { name: 'audit', state: 'running', detail: 'red-team is reading the diff' }, { name: 'merge', state: 'pending', detail: 'waits for you' }], findings: [['P1', 'unchecked unwrap in claim.rs']], flaky: true, merge: '' }];
  return s;
}
const model = (view: (typeof VIEWS)[number], over: Partial<OModel> = {}): OModel => ({
  info, pal: { dark: true }, st: world(), view, pick: 0, scroll: 0, selNode: 0, arch: emptyArch(), live: {}, connected: true, still: true, now: NOW, ...over,
} as OModel);
const SIZES = [[80, 24], [100, 34], [40, 12]] as const;
const hits = [{ path: 'specs/003.md', collection: 'specs', trust: 'internal_generated', heading: 'Retry ladder', preview: 'three attempts\nthen escalate' }];
const T0 = Date.UTC(2026, 9, 25, 10, 0);
const changed = [
  { session: 's_0001', at_ms: T0, role: 'builder', task: 't_1', summary: 'added a visibility timeout to the queue claim', files: ['crates/queue/src/claim.rs'] },
  { session: 's_0002', at_ms: T0 + 21 * 60_000, role: 'builder', task: 't_2', summary: 'reworked the permission model to be less intrusive; removed ratify requirements for explicit updates', files: [] },
  { session: 's_0002', at_ms: T0 + 42 * 60_000, role: 'architect', task: 't_2', summary: 'wrote the spec for the change log', files: ['specs/032-change-log.md'] },
];
const withChanges = (): Partial<OModel> => ({ st: { ...world(), changes: changed, sessions: [{ id: 's_0001', reason: 'new', started_ms: T0 - 60_000, turns: 1 }, { id: 's_0002', reason: 'new', started_ms: T0 + 10 * 60_000, turns: 2 }] } });

const cases: [string, Partial<OModel>][] = [
  ['agents', {}],
  ['archives', { arch: { ...emptyArch(), q: 'retry', searched: 'retry', hits, took: 400, engine: 'hybrid', focus: 'list' } }],
  ['changes', withChanges()],
];

test('goldens: agents, archives and changes at 80x24, 100x34 and 40x12', () => {
  for (const [view, over] of cases) for (const [w, h] of SIZES) {
    const r = snapshot(`narrator-${view}-${w}x${h}`, observeFrame(model(view as (typeof VIEWS)[number], over), w, h));
    assert.ok(r.ok, r.message);
  }
});

test('the four views fit every size exactly and carry no money', () => {
  for (const [view, over] of cases) for (const [w, h] of [[28, 9], [40, 12], [60, 20], [80, 24], [100, 34], [160, 50]] as const) {
    const lines = observeFrame(model(view as (typeof VIEWS)[number], over), w, h);
    assert.equal(lines.length, h, `${view} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${view} ${w}x${h}: ${plain(l)}`);
    assert.ok(!/\$\d|spend|cost/i.test(lines.map(plain).join('\n')), `${view} ${w}x${h}: no money`);
  }
});

test('agents leads with the roster: name, role, state in words, model, task and one token figure', () => {
  const t = observeFrame(model('agents'), 100, 40).map(plain).join('\n');
  assert.match(t, /hephaestus {2}builder {2}● running/);
  assert.match(t, /workhorse · claude-sonnet-x · t_1/);
  assert.match(t, /12\.6k tokens|12600 tokens/, 'one figure: input + cache + output + thinking');
  assert.match(t, /ares {2}red-team {2}► waiting on you/);
  assert.ok(!/cache \d|think \d|call\(s\)/.test(t), 'the per-kind token split is gone');
});

test('archives and changes keep their calm empty states', () => {
  assert.match(observeFrame(model('archives'), 100, 24).map(plain).join('\n'), /search the archives[\s\S]*search what Delphi knows/);
  const t = observeFrame(model('changes'), 100, 24).map(plain).join('\n');
  assert.match(t, /no changes recorded yet/);
  assert.match(t, /chat summary and a pull request description/);
});

test('changes: grouped by session, newest first, results only; the session in progress is marked', () => {
  const t = observeFrame(model('changes', withChanges()), 100, 34).map(plain).join('\n');
  assert.ok(t.indexOf('session s_0002') < t.indexOf('session s_0001'), 'the newest session is on top');
  assert.match(t, /session s_0002 · \d\d:\d\d · now · 2 changes/);
  assert.match(t, /session s_0001 · \d\d:\d\d · 1 change\b/);
  assert.ok(t.indexOf('wrote the spec for the change log') < t.indexOf('reworked the permission model'), 'newest first within a session');
  assert.match(t, /reworked the permission model to be less intrusive; removed ratify requirements for\s+explicit updates/);
  assert.match(t, /specs\/032-change-log\.md/);
  assert.ok(!/reasoning|thinking/i.test(t), 'no reasoning anywhere');
});

test('agents: the running builder shows its live token total (estimated) and its tool counts on the row under it', () => {
  const tools = { builder: { calls: 15, errors: 2, last: 'run_command', by_tool: { read_file: { calls: 12, errors: 0 }, record_change: { calls: 3, errors: 0 }, run_command: { calls: 0, errors: 2 } } } };
  const t = observeFrame(model('agents', { st: { ...world(), tools }, liveTokens: { n3: 4_000 } }), 100, 34).map(plain).join('\n');
  // 6,200 + 3,100 + 2,400 + 900 logged, plus 4,000 so far in the span that has not ended.
  assert.match(t, /~16.6k tokens[^\n]*\n +15 calls · ✎ 3 · ✗ 2 · → run_command/);
  // Once the span's usage is logged the estimate goes away: only the logged figure.
  const done = observeFrame(model('agents', { st: { ...world(), tools } }), 100, 34).map(plain).join('\n');
  assert.match(done, /12\.6k tokens[^\n]*\n +15 calls/);
  assert.ok(!done.includes('~'), 'no estimate mark without a live total');
  // Changed something, recorded nothing yet: the running agent's count is a warning; a reader (no work tool) is not nagged.
  const worked = { builder: { calls: 4, errors: 0, last: 'write_file', by_tool: { read_file: { calls: 3, errors: 0 }, write_file: { calls: 1, errors: 0 } } } };
  assert.match(observeFrame(model('agents', { st: { ...world(), tools: worked } }), 100, 34).map(plain).join('\n'), /4 calls · ✎ 0/);
  const reader = { builder: { calls: 3, errors: 0, last: 'read_file', by_tool: { read_file: { calls: 3, errors: 0 } } } };
  assert.ok(!/✎/.test(observeFrame(model('agents', { st: { ...world(), tools: reader } }), 100, 34).map(plain).join('\n')));
  // An agent that has called nothing shows no tool line.
  assert.ok(!/ calls/.test(observeFrame(model('agents'), 100, 34).map(plain).join('\n')));
});

test('agents: a token-cap meter on the busiest running span, a quiet note when it goes silent, a flag when it loops', () => {
  const at = (extra: Partial<OModel>, tools?: unknown) => observeFrame(model('agents', { st: { ...world(), token_cap: 20_000, tools: tools as never }, now: NOW, since: NOW - 600_000, ...extra }), 110, 34).map(plain).join('\n');
  // 4,000 of a 20,000 cap, then 17,000 (warning colour, same text), and no meter at all without a live total.
  assert.match(at({ liveTokens: { n3: 4_000 } }), /~16\.6k tokens {2}▓{2}░{6} 20%/);
  assert.match(at({ liveTokens: { n3: 17_000 } }), /▓{7}░ 85%/);
  assert.ok(!/▓/.test(at({})), 'no live total, no meter');
  // Heard from a moment ago: nothing; silent for three minutes: `quiet 3m`; never heard from since the window opened: counted from then.
  assert.ok(!/quiet/.test(at({ lastSeen: { n3: NOW - 5_000 } })));
  assert.match(at({ lastSeen: { n3: NOW - 190_000 } }), /running · quiet 3m/);
  assert.match(at({ lastSeen: {}, since: NOW - 300_000 }), /quiet 5m/);
  // The same tool six times, or failing three times in a row.
  const loop = (n: number, failed: number) => ({ builder: { calls: n, errors: failed, last: 'run_command', streak: { tool: 'run_command', n, failed }, by_tool: { run_command: { calls: n, errors: failed } } } });
  assert.match(at({}, loop(6, 0)), /⟳ run_command ×6/);
  assert.match(at({}, loop(3, 3)), /⟳ run_command ×3 \(3 failed\)/);
  assert.ok(!/⟳/.test(at({}, loop(5, 2))), 'under both thresholds');
});

