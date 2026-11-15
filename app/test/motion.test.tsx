import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Mascot, Plate } from '../src/Badge.tsx';
import { BLINK_MAX_MS, BLINK_MIN_MS, BUILT_IN, blinkFrames, blinks, crownOf, mascotRow, nextBlinkDelay } from '../src/avatar.ts';
import { fixtureInfo } from '../src/bridge.ts';

const info = fixtureInfo();

test('a face starts its next blink 9 to 33 seconds on, at a time of its own', () => {
  assert.deepEqual([BLINK_MIN_MS, BLINK_MAX_MS], [9000, 33000]);
  assert.equal(nextBlinkDelay(0), 9000);
  assert.equal(nextBlinkDelay(0.5), 21000);
  assert.equal(nextBlinkDelay(0.999999), 33000);
  // Out of range or not a number never leaves the window.
  for (const r of [-1, 2, Number.NaN, Number.POSITIVE_INFINITY]) assert.ok(nextBlinkDelay(r) >= 9000 && nextBlinkDelay(r) <= 33000, String(r));
  const drawn = new Set(Array.from({ length: 200 }, () => nextBlinkDelay()));
  assert.ok(drawn.size > 150, 'two faces almost never draw the same time');
  assert.ok([...drawn].every((d) => d >= 9000 && d <= 33000));
});

test("a blink is the table's own frames, a one-off face blinks mechanically, and only idle and working faces blink", () => {
  assert.deepEqual(blinkFrames(info, 'scout'), ['-••', '•-•', '••-'], 'the scout closes one eye at a time');
  assert.deepEqual(blinkFrames(info, 'orchestrator'), ['-─-']);
  assert.deepEqual(blinkFrames(info, 'builder', '^‿^'), ['-‿-']);
  for (const role of Object.keys(BUILT_IN.roles)) {
    const frames = blinkFrames(info, role);
    assert.ok(frames.length >= 1 && frames.every((f) => Array.from(f).length === 3), role);
    assert.notEqual(frames[0], BUILT_IN.roles[role]!.badge.idle!.cells, `${role} blink is not its idle face`);
    // Every expression the table defines has a three-cell chip and a seven-cell mascot row.
    for (const activity of Object.keys(BUILT_IN.roles[role]!.badge)) assert.equal(Array.from(mascotRow(info, role, activity)).length, 7, `${role} ${activity}`);
    assert.equal(crownOf(info, role).cells.length, 7, role);
  }
  assert.deepEqual([blinks('idle'), blinks('working'), blinks('waiting_approval'), blinks('thinking_left'), blinks('success')], [true, true, false, false, false]);
  assert.equal(mascotRow(info, 'someone-new', 'idle'), '  •_•  ');
  assert.deepEqual(crownOf(info, 'orchestrator'), { cells: [false, false, true, true, true, false, false], color: '#facc15' });
});

test('a chip shows the expression for what the agent is doing, and the mascot its crown, face and chin', () => {
  const cells = (html: string) => [...html.matchAll(/<i[^>]*>([^<]*)<\/i>/g)].map((m) => m[1]).join('');
  const chip = (activity: string) => cells(renderToStaticMarkup(<Plate info={info} role="architect" light={false} activity={activity} />));
  assert.deepEqual(['idle', 'working', 'waiting_approval', 'success', 'surprise', 'alarm', 'escalated', 'thinking_left'].map(chip), ['•o•', '•─•', 'x?x', '^─^', '●_●', '●_!', 'x_x', '•• ']);
  const m = renderToStaticMarkup(<Mascot info={info} role="builder" light={false} />);
  assert.ok(m.includes('class="mascot-crown"') && m.includes('class="mascot-chin"') && m.includes('--c:#f59e0b'), m);
  assert.equal((m.match(/class="on"/g) ?? []).length, 7, 'the builder wears a full crown');
  assert.equal(cells(m.slice(m.indexOf('mascot-face'))), ' •===• ');
  assert.match(m, /aria-label="builder, idle"/);
});
