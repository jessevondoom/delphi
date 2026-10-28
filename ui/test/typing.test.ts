// The typing bubble: after a prompt, a random 500-850 ms wait, then three dots pulse in an agent's bubble until the reply begins.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { typingDelay, typingFrame, TYPING_MIN_MS, TYPING_SPAN_MS } from '../src/motion.ts';
import { transcript, typingOf, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { fold, initial, type State } from '../src/fold.ts';
import { newInbox } from '../src/inbox.ts';
import { info, plain } from './helpers.ts';

const T0 = Date.parse('2026-10-25T09:00:00Z');
const ev = (type: string, payload: any, ms: number, task = 't_1') => ({ method: 'event', params: { type, task_id: task, payload, timestamp: new Date(T0 + ms).toISOString() } });
const model = (state: State, now: number, over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true }, state, now, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'default', panel: null, help: false, ...over,
});
const sent = () => fold(initial(T0), ev('task.created', { prompt: 'hello' }, 0), T0);
const text = (s: State, now: number, over: Partial<Model> = {}) => transcript(model(s, now, over), 80).map(plain).join('\n');

test('the wait is 500-850 ms, differs between prompts and never changes for one', () => {
  const waits = Array.from({ length: 200 }, (_, i) => typingDelay(i));
  assert.ok(waits.every((w) => w >= TYPING_MIN_MS && w <= TYPING_MIN_MS + TYPING_SPAN_MS));
  assert.ok(new Set(waits).size > 50);
  assert.equal(typingDelay(7), typingDelay(7));
});

test('frames: waiting, then dot 0, 1, 2 and a rest, round and round', () => {
  const d = typingDelay(1);
  assert.equal(typingFrame(0, 1, d - 1), -1);
  assert.deepEqual([0, 300, 600, 900, 1200].map((t) => typingFrame(0, 1, d + t)), [0, 1, 2, 3, 0]);
});

test('no bubble before the wait; then three dots with one lit; the reply starting removes them', () => {
  const s = sent();
  const b = typingOf(s)!;
  assert.deepEqual(b && { since: b.since }, { since: T0 });
  assert.ok(!/typing/.test(text(s, T0 + 400)), 'still waiting');
  const lit = text(s, T0 + typingDelay(b.id) + 5);
  assert.match(lit, /typing/);
  assert.match(lit, /● ○ ○/);
  assert.match(text(s, T0 + typingDelay(b.id) + 305), /○ ● ○/);
  assert.match(text(s, T0 + typingDelay(b.id) + 905), /○ ○ ○/);
  assert.equal(typingOf({ ...s, live: 'Hi' }), null, 'streamed text replaces the dots');
  const done = fold(s, ev('task.output', { status: 'done', text: 'Hi there.' }, 2000), T0 + 2000);
  assert.equal(typingOf(done), null);
  assert.ok(!/typing|[●○] [●○] [●○]/.test(text(done, T0 + 2100)));
  assert.match(text(done, T0 + 2100), /Hi there\./);
});

test('reduced motion: the dots are still, all three lit, but the wait is kept', () => {
  const s = sent();
  const id = typingOf(s)!.id;
  assert.ok(!/typing/.test(text(s, T0 + 400, { still: true })));
  assert.match(text(s, T0 + typingDelay(id) + 305, { still: true }), /● ● ●/);
});
