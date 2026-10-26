// The monitoring panes (`specs/034`): pulse, timeline and tools. Goldens at three sizes, then what each one says.
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { emptyState, observeFrame, type OModel } from '../src/observe-screen.ts';
import { snapshot } from './helpers.ts';
import { monitorsModel, monitorsWorld } from './monitors-fixture.ts';
import { info, NOW } from './epic-fixture.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const text = (m: OModel, w = 110, h = 34) => observeFrame(m, w, h).map(plain).join('\n');
const PANES = ['pulse', 'timeline', 'tools'] as const;

test('goldens: pulse, timeline and tools at 100x34, 80x24 and 40x12', () => {
  for (const view of PANES) for (const [w, h] of [[100, 34], [80, 24], [40, 12]] as const) {
    const r = snapshot(`monitors-${view}-${w}x${h}`, observeFrame(monitorsModel(view), w, h));
    assert.ok(r.ok, r.message);
  }
});

test('every pane fits every size exactly, with a full world and an empty one, and shows no money', () => {
  for (const view of PANES) for (const st of [monitorsWorld(), emptyState()]) for (const [w, h] of [[28, 9], [40, 12], [60, 20], [100, 34], [160, 50]] as const) {
    const lines = observeFrame(monitorsModel(view, { st }), w, h);
    assert.equal(lines.length, h, `${view} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${view} ${w}x${h}: ${plain(l)}`);
    assert.ok(!/\$\d|spend|cost/i.test(lines.map(plain).join('\n')), `${view} ${w}x${h}: no money`);
  }
});

test('pulse: a meter per working agent with pace, cap share, quiet note and loop flag; calm when nothing runs', () => {
  const t = text(monitorsModel('pulse'));
  assert.match(t, /hephaestus\s+[▁-█]{10,}\s+[\d.]+k\/min\s+● running/);
  assert.match(t, /▓{7}░ 82%\s+·\s+quiet 3m\s+·\s+⟳ run_command ×7\s+·\s+→ run_command/);
  assert.match(t, /ares[\s\S]*► waiting on you/);
  assert.ok(!/daedalus/.test(t), 'an idle agent has no meter');
  assert.match(t, /all agents\s+[▁-█]+/);
  assert.match(text(monitorsModel('pulse', { st: emptyState() })), /nothing is working right now/);
  // Not silent, not over the cap: no warnings, no meter without a live total.
  const calm = text(monitorsModel('pulse', { liveTokens: {}, lastSeen: { n3: NOW - 1000 }, st: { ...monitorsWorld(), tools: {} } }));
  assert.ok(!/quiet|⟳|%/.test(calm));
});

test('timeline: a lane per role across the window, runs drawn where they happened, a mark for every recorded change', () => {
  const t = text(monitorsModel('timeline'));
  const lane = (name: string) => t.split('\n').find((l) => l.includes(name))!;
  assert.match(lane('daedalus'), /█{5,}✎█+/, 'the architect finished early and recorded one change');
  assert.match(lane('hephaestus'), /▓+✎▓+✎▓+/, 'the builder is still running and recorded two');
  assert.match(t, /\d\d:\d\d\s+\d\d:\d\d\s+\d\d:\d\d\s+\d\d:\d\d\s+\d\d:\d\d/, 'five time marks along the bottom');
  assert.match(t, /3 changes in this window/);
  assert.match(text(monitorsModel('timeline', { st: emptyState() })), /no work has started yet/);
});

test('tools: busiest first with failures, who reports their work, who is going round in circles', () => {
  const t = text(monitorsModel('tools'));
  assert.ok(t.indexOf('read_file') < t.indexOf('run_command') && t.indexOf('run_command') < t.indexOf('edit_file'), 'by calls');
  assert.match(t, /run_command\s+▓+░*\s+12\s+✗ 5/);
  assert.match(t, /claude-sonnet-5-5\s+▓+\s+100%\s+5\/5 runs/);
  assert.match(t, /gemini-flash\s+▓+░+\s+25%\s+1\/4 runs/);
  assert.match(t, /going round in circles: hephaestus run_command ×7/);
  assert.match(text(monitorsModel('tools', { st: emptyState() })), /no tool has been called yet/);
});

test('the panes show names, counts and times only: nothing a model wrote or an argument', () => {
  const st = monitorsWorld();
  st.changes![0]!.summary = 'wrote the retry queue design';
  for (const view of PANES) {
    const t = text(monitorsModel(view, { st }));
    assert.ok(!/retry queue design|specs\/040/.test(t), `${view} does not repeat what an agent wrote`);
  }
  assert.equal(info.roles['builder']?.name, 'hephaestus');
});
