import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { PAIR_WAITING, PairCode, pairCodeOf } from '../src/PairCode.tsx';
import type { Conn } from '../src/types.ts';

const waiting: Conn = { connected: true, detail: 'Connected to the daemon, read-only', pair_code: 'K7Q-4M2' };

test('a window that asked to be approved shows the code and the words that say where to type it', () => {
  assert.equal(pairCodeOf(waiting), 'K7Q-4M2');
  const html = renderToStaticMarkup(<PairCode code="K7Q-4M2" />);
  assert.match(html, /K7Q-4M2/);
  assert.ok(html.includes(PAIR_WAITING));
  assert.ok(PAIR_WAITING.startsWith('Waiting for you to approve this at your terminal'));
  assert.match(html, /delphi app approve/);
  // The screen offers nothing to press: it cannot approve, widen or skip, and nothing moves.
  assert.doesNotMatch(html, /<button|<input|<a |animation|spin/i);
});

test('only an unpaired, attached window with a well-formed code shows one', () => {
  assert.equal(pairCodeOf({ ...waiting, paired: 'app-0a1b2c' }), null, 'a paired window has no code to show');
  assert.equal(pairCodeOf({ ...waiting, connected: false }), null);
  assert.equal(pairCodeOf({ connected: true, detail: '' }), null);
  // What the daemon sent is not trusted text: a code outside the alphabet or the shape is not printed.
  for (const bad of ['', 'K7Q4M2', 'K7Q-4M20', 'k7q-4m2', 'K70-4M2', 'KIQ-4L2', '<b>x</b>', 'K7Q-4M2\n<script>']) assert.equal(pairCodeOf({ ...waiting, pair_code: bad }), null, JSON.stringify(bad));
});
