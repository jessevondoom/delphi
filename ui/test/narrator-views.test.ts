// agents, archives and thinking in the narrator's style (`005f` slice 10): lowercase section rules, a tinted
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
const thinkText = { n3: 'The queue needs a visibility timeout.\nA claim without one can be lost for good.' };

const cases: [string, Partial<OModel>][] = [
  ['agents', {}],
  ['archives', { arch: { ...emptyArch(), q: 'retry', searched: 'retry', hits, took: 400, engine: 'hybrid', focus: 'list' } }],
  ['thinking', { think: thinkText }],
];

test('goldens: agents, archives and thinking at 80x24, 100x34 and 40x12', () => {
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

test('archives and thinking keep their calm empty states', () => {
  assert.match(observeFrame(model('archives'), 100, 24).map(plain).join('\n'), /search what Delphi knows[\s\S]*● offline/);
  const t = observeFrame(model('thinking'), 100, 24).map(plain).join('\n');
  assert.match(t, /nothing is thinking right now/);
  assert.match(t, /never saved/);
});
