// Reduce motion, an accessibility setting: `/reduce-motion`, `/settings set reduce_motion`, or asking, and every animation obeying it.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { lookup } from '../src/commands.ts';
import { initial } from '../src/fold.ts';
import { faceOf, pulseOn, spinner, stillMotion, tickKey } from '../src/motion.ts';
import { reduceMotionFrom } from '../src/reduce-motion.ts';
import { renderSettings, runSlash, type Ctx } from '../src/run.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: {}, workspace: '/tmp/ws' } as unknown as Info;

function ctx(replies: Record<string, any> = {}) {
  const calls: [string, any][] = [], out: string[] = [];
  const c = { cmd: { call: async (m: string, p: any) => { calls.push([m, p]); return replies[m] ?? {}; } } as any, mission: 'm1', info, state: () => initial(0), post: (t: string) => out.push(t) } as Ctx;
  return { c, calls, out };
}

test('/reduce-motion is a command; with no word it flips the saved setting, with on or off it sets it', async () => {
  assert.ok(lookup('/reduce-motion'), 'listed, so it completes and shows in /help');
  for (const [line, saved, want] of [['/reduce-motion', false, 'on'], ['/reduce-motion', true, 'off'], ['/reduce-motion on', true, 'on'], ['/reduce-motion OFF', false, 'off']] as const) {
    const x = ctx({ 'settings.get': { reduce_motion: saved } });
    await runSlash(line, x.c);
    assert.deepEqual(x.calls.at(-1), ['settings.set', { mission_id: 'm1', key: 'reduce_motion', value: want }], line);
  }
  const bad = ctx();
  await runSlash('/reduce-motion sideways', bad.c);
  assert.equal(bad.calls.length, 0);
  assert.match(bad.out[0]!, /\/reduce-motion \[on \| off\]/);
  assert.match(renderSettings({ roles: [], pending: [], reduce_motion: true }), /reduce motion on/);
});

test('the windows follow only the daemon\'s own setting.changed event for reduce_motion', () => {
  const ev = (key: string, nu: string) => ({ method: 'event', params: { type: 'setting.changed', payload: { key, old: 'x', new: nu } } });
  assert.equal(reduceMotionFrom(ev('reduce_motion', 'on')), true);
  assert.equal(reduceMotionFrom(ev('reduce_motion', 'off')), false);
  assert.equal(reduceMotionFrom(ev('role_tier.builder', 'fast')), null);
  assert.equal(reduceMotionFrom({ method: 'event', params: { type: 'task.created', payload: { key: 'reduce_motion', new: 'on' } } }), null);
});

test('asking for reduced motion stills everything, whatever the terminal says', () => {
  assert.equal(stillMotion({ TERM: 'xterm-256color' }, false), false);
  assert.equal(stillMotion({ TERM: 'xterm-256color' }, true), true);
  assert.equal(stillMotion({ DELPHI_REDUCE_MOTION: '1' }, false), true);
  assert.equal(stillMotion({ NO_COLOR: '1' }, false), true);
});

test('when still: no blink, no thinking pan, a fixed spinner, a steady pulse, and no quarter-second repaints', () => {
  const idle = { state: 'idle' } as any, thinking = { state: 'running', doing: '', thinkingAt: 0 } as any;
  const frames = new Set<string>();
  for (let t = 0; t < 60_000; t += 50) { frames.add(faceOf(idle, 'scout', t, true).frame); frames.add(faceOf(thinking, 'builder', t, true).frame); }
  assert.deepEqual([...frames].sort(), ['idle', 'thinking_left', 'working']);
  assert.equal(new Set([0, 250, 500, 750, 1000].map((t) => spinner(t, true))).size, 1);
  assert.ok(new Set([0, 250, 500, 750].map((t) => spinner(t, false))).size > 1, 'it does turn when motion is allowed');
  assert.ok([0, 600, 1200, 1800].every((t) => pulseOn(t, true)));
  assert.notEqual(pulseOn(0, false), pulseOn(600, false));
  const keys = new Set([0, 250, 500, 750].map((t) => tickKey({}, [], t, true, true)));
  assert.equal(keys.size, 1, 'a running window repaints once a second, not four times');
});

// A rule for the future: an animation must ask `motion.ts` (stillMotion / spinner / pulseOn / faceOf), not run its own clock.
test('no source file keeps its own animation clock outside motion.ts', () => {
  const dir = new URL('../src/', import.meta.url);
  const TIMERS_OK = new Set(['app.tsx', 'observer.tsx', 'rpc.ts', 'elog.ts', 'select.ts', 'main.tsx']); // repaint tick (gated by `still`), samples, retries, locks
  for (const f of readdirSync(dir).filter((n) => /\.(ts|tsx)$/.test(n) && n !== 'motion.ts')) {
    const src = readFileSync(new URL(f, dir), 'utf8');
    assert.ok(!/Math\.floor\([^)]*\bnow\b[^)]*\/[^)]*\)\s*%/.test(src), `${f}: a frame picked from the clock (Math.floor(now / n) % k) belongs in motion.ts, behind \`still\``);
    if (/setInterval/.test(src)) assert.ok(TIMERS_OK.has(f), `${f}: a new interval timer; if it animates anything, gate it on stillMotion and list the file here`);
  }
});
