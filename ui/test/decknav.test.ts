import test from 'node:test';
import assert from 'node:assert/strict';
import { deckTarget } from '../src/decknav.ts';
import { InputParser } from '../src/input.ts';
import * as I from '../src/inbox.ts';
import { cards, newInbox } from '../src/inbox.ts';
import { fold, initial } from '../src/fold.ts';

const key = (k: string, over: object = {}) => ({ t: 'key', k, alt: false, shift: false, ctrl: false, ...over }) as any;

test('arrows move a deck when the prompt is empty, and move the cursor when it is not', () => {
  assert.equal(deckTarget(1, key('left'), true), 0);
  assert.equal(deckTarget(1, key('right'), true), 2);
  assert.equal(deckTarget(0, key('right'), true), 1);
  assert.equal(deckTarget(1, key('left'), false), null, 'text in the prompt: the arrow edits');
  assert.equal(deckTarget(1, key('right'), false), null);
});

test('the ends stop: there is no deck beyond the left or the right one', () => {
  assert.equal(deckTarget(0, key('left'), true), 0);
  assert.equal(deckTarget(2, key('right'), true), 2);
});

test('shift+arrows move a deck even with a half-written message, which stays in the prompt', () => {
  assert.equal(deckTarget(1, key('right', { shift: true }), false), 2);
  assert.equal(deckTarget(1, key('left', { shift: true }), false), 0);
});

test('alt and ctrl arrows are not deck keys, whatever the prompt holds', () => {
  for (const empty of [true, false]) {
    assert.equal(deckTarget(1, key('left', { alt: true }), empty), null);
    assert.equal(deckTarget(1, key('right', { ctrl: true }), empty), null);
    assert.equal(deckTarget(1, key('right', { alt: true, shift: true }), empty), null);
  }
});

test('other keys, text and mouse events are not navigation', () => {
  for (const e of [key('up'), key('down'), key('enter'), key('tab'), { t: 'text', s: 'a' }, { t: 'wheel', dir: 'up' }]) assert.equal(deckTarget(1, e as any, true), null);
});

test('what a terminal sends for a plain arrow is the plain key (CSI and application mode), with no modifier', () => {
  const p = new InputParser();
  for (const seq of ['\x1b[D', '\x1bOD']) {
    const [e] = p.feed(seq);
    assert.deepEqual(e, { t: 'key', k: 'left', alt: false, shift: false, ctrl: false });
    assert.equal(deckTarget(1, e as any, true), 0);
  }
  assert.equal(deckTarget(1, p.feed('\x1b[1;2C')[0] as any, false), 2, 'shift+right');
  assert.equal(deckTarget(1, p.feed('\x1b[1;3C')[0] as any, true), null, 'alt+right');
});

test('in the inbox, ← from the list goes back to the chat; in focus mode ← and → walk the cards', () => {
  let s = initial(0);
  for (const id of ['a1', 'a2']) s = fold(s, { method: 'event', params: { type: 'approval.requested', task_id: 't', payload: { approval_id: id, role: 'builder', tier: 'amber', payload_blake3: 'h' + id, payload: {}, summary: id, timeout_secs: 600, tool: 'write_file' } } }, 1000);
  const list = cards(s);
  const inb = newInbox(0);
  assert.equal(inb.mode, 'list');
  assert.deepEqual(I.key(inb, list, key('left'), 5000).action, { t: 'chat' });
  assert.deepEqual(I.key(inb, list, key('right'), 5000).action, { t: 'none' }, 'nothing is to the right of the inbox');
  const focus = { ...inb, mode: 'focus' as const };
  const next = I.key(focus, list, key('right'), 5000);
  assert.equal(next.action.t, 'none');
  assert.equal(next.inbox.sel, 1, 'focus mode: → is the next card');
  assert.equal(I.key({ ...focus, sel: 1 }, list, key('left'), 5000).inbox.sel, 0, 'focus mode: ← is the previous card, not the chat');
  assert.deepEqual(I.key(inb, list, key('left', { alt: true }), 5000).action, { t: 'none' }, 'alt+← is not a navigation key');
  assert.deepEqual(I.key(focus, [], key('left'), 5000).action, { t: 'chat' }, 'with nothing to review, ← leaves');
});
