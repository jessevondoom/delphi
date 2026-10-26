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
  assert.match(t, /▓{7}░ 82%\s+cap in ~2m\s+·\s+in run_command 12m\s+·\s+⟳ run_command ×7\s+·\s+→ run_command/);
  assert.match(t, /ares[\s\S]*► waiting on you/);
  assert.ok(!/daedalus/.test(t), 'an idle agent has no meter');
  assert.match(t, /all agents\s+[▁-█]+/);
  assert.match(text(monitorsModel('pulse', { st: emptyState() })), /nothing is working right now/);
  // Not silent, not over the cap: no warnings, no meter without a live total.
  const calm = text(monitorsModel('pulse', { liveTokens: {}, lastSeen: { n3: NOW - 1000 }, st: { ...monitorsWorld(), tools: {} } }));
  assert.ok(!/quiet|⟳|%|in run_command/.test(calm));
});

test('pulse: the digest for coming back, and how long each approval has waited', () => {
  const t = text(monitorsModel('pulse'));
  assert.match(t, /since you last acted, 18m ago:\s+3 changes · 2 tasks done · 1 approval raised/);
  assert.match(t, /► waiting on you\s+6m\s+ares · run cargo test --workspace/);
  // Nothing happened since, or it happened a moment ago: no digest. A long wait is an alert, not a warning.
  assert.ok(!/since you last acted/.test(text(monitorsModel('pulse', { st: { ...monitorsWorld(), since_you: { since_ms: NOW - 20_000, changes: 2, done: 0, failed: 0, approvals: 0 } } }))));
  assert.ok(!/since you last acted/.test(text(monitorsModel('pulse', { st: { ...monitorsWorld(), since_you: { since_ms: NOW - 600_000, changes: 0, done: 0, failed: 0, approvals: 0 } } }))));
  const old = text(monitorsModel('pulse', { st: { ...monitorsWorld(), approvals: { ap_1: { role: 'red-team', summary: 'x', since_ms: NOW - 25 * 60_000 } } } }));
  assert.match(old, /waiting on you\s+25m/);
});

test('state decides what silence means: inside a tool it takes ten minutes, thinking two, waiting on you never', () => {
  const at = (over: Partial<OModel>, tools?: unknown, approvals?: unknown) => text(monitorsModel('pulse', { st: { ...monitorsWorld(), tools: tools as never, approvals: approvals as never }, ...over }));
  const open = { builder: { calls: 4, errors: 0, last: 'run_command', streak: { tool: 'run_command', n: 1, failed: 0 }, open: { tool: 'run_command', since_ms: NOW - 5 * 60_000 }, by_tool: {} } };
  assert.ok(!/quiet|in run_command \d+m/.test(at({ lastSeen: { n3: NOW - 5 * 60_000 } }, open)), 'five silent minutes inside a build is fine');
  assert.match(at({ lastSeen: { n3: NOW - 11 * 60_000 } }, open), /in run_command 5m/);
  const thinking = { builder: { ...open.builder, open: null } };
  assert.match(at({ lastSeen: { n3: NOW - 190_000 } }, thinking), /quiet 3m/);
  const waiting = { 'builder-ap': { role: 'builder', summary: 'run it', since_ms: NOW - 9 * 60_000 } };
  assert.ok(!/quiet/.test(at({ lastSeen: { n3: NOW - 20 * 60_000 } }, thinking, waiting)), 'waiting on the operator is not silence');
});

test('the cap meter forecasts when the cap is reached at the last minute\'s pace, only when that is near', () => {
  const t = text(monitorsModel('pulse'));
  assert.match(t, /82%\s+cap in ~2m/);
  const slow = { builder: Array<number>(36).fill(10) };
  assert.ok(!/cap in/.test(text(monitorsModel('pulse', { pulse: slow }))), 'at 10 tokens per 5 s the cap is hours away');
  assert.ok(!/cap in/.test(text(monitorsModel('pulse', { pulse: undefined }))), 'no samples, no forecast');
});

test('an API seat shows a live estimate with the logged tokens, and no cap meter', () => {
  const t = text(monitorsModel('agents', { liveTokens: undefined, liveEst: { n3: 3_000 }, lastSeen: { n3: NOW - 1000 } }));
  assert.match(t, /~\d+(\.\d)?k tokens/);
  assert.ok(!/▓/.test(t.split('\n').filter((l) => /tokens/.test(l) && !/^1 running/.test(l)).join('\n')), 'the cap applies to subscription spans only');
});

test('timeline: a lane per role across the window, runs drawn where they happened, a mark for every recorded change', () => {
  const t = text(monitorsModel('timeline'));
  const lane = (name: string) => t.split('\n').find((l) => l.includes(name))!;
  assert.match(lane('daedalus'), /█{5,}✎█+/, 'the architect finished early and recorded one change');
  assert.match(lane('hephaestus'), /▓+✎▓+✎▓+/, 'the builder is still running and recorded two');
  assert.match(t, /\d\d:\d\d\s+\d\d:\d\d\s+\d\d:\d\d\s+\d\d:\d\d\s+\d\d:\d\d/, 'five time marks along the bottom');
  assert.match(t, /3 changes in this window/);
  assert.match(t, /momentum\s+[▁-█]{12}\s+3 in the last hour · last 2m ago/);
  assert.match(t, /── files being changed ─+\n\s+crates\/queue\/src\/claim\.rs\s+hephaestus×4 daedalus×1\s+⚠ two agents\s+\d\d:\d\d\n\s+specs\/040-retry\.md\s+daedalus×2\s+\d\d:\d\d/);
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
    assert.ok(!/retry queue design/.test(t), `${view} does not repeat what an agent wrote`);
    if (view !== 'timeline') assert.ok(!/specs\/040/.test(t), `${view} shows no paths; only the files section of the timeline does`);
  }
  assert.equal(info.roles['builder']?.name, 'hephaestus');
});
