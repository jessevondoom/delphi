// Multi-frame blinks (spec 087 §3.2): the scout closes one eye at a time and never all three; every frame lasts one timer
// tick; a role the library does not know closes its eyes and keeps its mouth; and with reduce motion there is no blink at all.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import chalk from 'chalk';
import { badge, blinkSteps, currentCapabilities, mechanicalBlink as themeMechanicalBlink, useCapabilities, type AvatarSpec, type Info } from '../src/theme.ts';
import { BLINK_FOR_MS, TICK_MS, blinkOffset, faceOf, mechanicalBlink, tickKey } from '../src/motion.ts';

const spec: AvatarSpec = JSON.parse(readFileSync(new URL('./fixtures/avatar-spec.json', import.meta.url), 'utf8'));
const roles = Object.keys(spec.roles);
const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0', cwd: '/',
  roles: Object.fromEntries(roles.map((r) => [r, { name: r, chassis: spec.roles[r]!.chassis, face: spec.roles[r]!.badge.idle!.cells }])),
  avatar: spec,
};
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const withColour = <T>(f: () => T): T => {
  const l = chalk.level, c = currentCapabilities();
  useCapabilities({ color: 'truecolor', attributes: true });
  try { return f(); } finally { useCapabilities(c); chalk.level = l; }
};
const idle = { state: 'idle' } as never;
/** The moment slot 0's blink of `role` starts. */
const startOf = (role: string) => blinkOffset(role, 0);

test('the timer period is one blink frame, so a three-frame blink is three ticks', () => {
  assert.equal(TICK_MS, BLINK_FOR_MS);
});

test('the library gives every role a blink whose first frame is the badge\'s blink, and only the scout has more than one', () => {
  for (const r of roles) {
    const steps = spec.roles[r]!.blink_steps;
    assert.ok(steps && steps.length >= 1, `${r} has blink_steps`);
    assert.equal(steps![0], spec.roles[r]!.badge.blink!.cells, `${r}: the first frame is badge.blink`);
    assert.equal(blinkSteps(r, info), steps!.length, r);
  }
  assert.deepEqual(spec.roles.scout!.blink_steps, ['-••', '•-•', '••-']);
  assert.deepEqual(roles.filter((r) => spec.roles[r]!.blink_steps!.length > 1), ['scout']);
  assert.equal(blinkSteps('scout', { ...info, avatar: undefined }), 1, 'an older delphi sends no steps: one frame');
});

test('a three-frame blink shows frame 0, 1, 2 on consecutive ticks and then the idle face', () => {
  const b = startOf('scout');
  const at = (t: number) => faceOf(idle, 'scout', t, false, 3);
  assert.deepEqual([0, 1, 2].map((i) => at(b + i * TICK_MS)), [0, 1, 2].map((i) => ({ frame: 'blink', state: 'idle', step: i })));
  assert.deepEqual([b - 1, b + 3 * BLINK_FOR_MS].map((t) => at(t)), [{ frame: 'idle', state: 'idle' }, { frame: 'idle', state: 'idle' }]);
  assert.equal(at(b + 3 * BLINK_FOR_MS - 1).step, 2, 'the last frame lasts to the end of its tick');
});

test('a one-frame blink is exactly the 150 ms it always was', () => {
  const b = startOf('orchestrator');
  assert.equal(faceOf(idle, 'orchestrator', b, false).step, 0);
  assert.equal(faceOf(idle, 'orchestrator', b + BLINK_FOR_MS - 1, false).frame, 'blink');
  assert.equal(faceOf(idle, 'orchestrator', b + BLINK_FOR_MS, false).frame, 'idle');
  // Asking for fewer than one frame still gives one.
  assert.equal(faceOf(idle, 'orchestrator', b, false, 0).frame, 'blink');
});

test('the scout\'s blink is drawn one eye at a time and never with all three closed', () => {
  withColour(() => {
    const frames = [0, 1, 2].map((step) => plain(badge('scout', 'idle', info, { dark: true } as never, 'blink', false, step)));
    assert.deepEqual(frames, ['⟦-••⟧', '⟦•-•⟧', '⟦••-⟧']);
    assert.ok(!frames.includes('⟦---⟧'));
    // Frame 0 is what a console that knows only one blink frame draws.
    assert.equal(plain(badge('scout', 'idle', info, { dark: true } as never, 'blink')), frames[0]);
    // Another role ignores a step it has no frame for.
    assert.equal(plain(badge('orchestrator', 'idle', info, { dark: true } as never, 'blink', false, 2)), '⟦-─-⟧');
  });
});

test('reduce motion: a multi-frame blink never appears, in any frame, and the repaint key does not move', () => {
  const frames = new Set<string>();
  for (let t = 0; t < 60_000; t += 50) {
    const f = faceOf(idle, 'scout', t, true, 3);
    frames.add(`${f.frame}:${f.step ?? '-'}`);
  }
  assert.deepEqual([...frames], ['idle:-']);
  const keys = new Set<string>();
  for (let t = 0; t < 60_000; t += 50) keys.add(tickKey({}, ['scout'], t, true, false, '', () => 3).split('|')[2]!);
  assert.deepEqual([...keys], ['idle']);
});

test('the repaint key tells the frames of a blink apart, so every frame is drawn', () => {
  const b = startOf('scout');
  const key = (t: number) => tickKey({}, ['scout'], t, false, false, '', () => 3).split('|')[2];
  assert.deepEqual([0, 1, 2].map((i) => key(b + i * TICK_MS)), ['blink', 'blink:1', 'blink:2']);
  assert.equal(key(b + 3 * TICK_MS), 'idle');
  // With one frame the key is the same as before this existed.
  assert.equal(tickKey({}, ['scout'], b, false, false).split('|')[2], 'blink');
});

test('a face with no blink of its own closes its eyes and keeps its mouth', () => {
  assert.equal(mechanicalBlink('•_•'), '-_-');
  assert.equal(mechanicalBlink('•─•'), '-─-');
  assert.equal(mechanicalBlink('¬_•'), '-_-');
  assert.equal(mechanicalBlink('☆ω☆'), '-ω-');
  assert.equal(mechanicalBlink('•••', null), '---', 'with no mouth every cell is an eye');
  assert.equal(mechanicalBlink('•••'), '-•-', 'the default mouth is the middle cell');
  assert.equal(mechanicalBlink('-_-'), null, 'a face whose eyes are already closed has no blink to show');
  assert.equal(mechanicalBlink('••'), null);
  assert.equal(mechanicalBlink('•_•_'), null);
  assert.equal(mechanicalBlink(''), null);
  assert.equal(mechanicalBlink, themeMechanicalBlink, 'one function, re-exported by the animation module');
});

test('a role the library does not know blinks mechanically: eyes closed, mouth kept', () => {
  withColour(() => {
    const pool = { ...info, roles: { ...info.roles, pooled: { name: 'pooled', chassis: '#84cc16', face: '•w•' } } };
    assert.equal(plain(badge('pooled', 'idle', pool, { dark: true } as never)), '⟦•w•⟧');
    assert.equal(plain(badge('pooled', 'idle', pool, { dark: true } as never, 'blink')), '⟦-w-⟧');
    // A face that is already closed falls back to the built-in blink rather than to nothing.
    const closed = { ...info, roles: { ...info.roles, pooled: { name: 'pooled', chassis: '#84cc16', face: '-_-' } } };
    assert.equal(plain(badge('pooled', 'idle', closed, { dark: true } as never, 'blink')), '⟦-_-⟧');
  });
});
