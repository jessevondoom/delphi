// The agents view is one full-width column (`specs/034`): the tool counters sit on a row under each agent's summary, not beside it.
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { observeFrame } from '../src/observe-screen.ts';
import { monitorsModel } from './monitors-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

test('the agents view stays one column however wide the window, and the tools board is its own view', () => {
  for (const w of [100, 130, 160]) {
    const text = observeFrame(monitorsModel('agents'), w, 34).map(plain).join('\n');
    assert.doesNotMatch(text, / │ /, `${w}`);
    assert.doesNotMatch(text, /reports its work/);
  }
});

test('the tool counters take a row under the tokens and the cap meter, at any width', () => {
  for (const w of [100, 130]) {
    const text = observeFrame(monitorsModel('agents'), w, 34).map(plain).join('\n');
    assert.match(text, /~740\.0k tokens +▓+░* 82%[^\n]*\n +41 calls · ✎ 2 · ✗ 6 · → run_command/, `${w}`);
  }
});

test('every row fits the window', () => {
  for (const r of observeFrame(monitorsModel('agents'), 130, 34)) assert.ok(stringWidth(plain(r)) <= 130);
});

test('goldens: the agents view at 130x34 and 160x40', () => {
  for (const [w, h] of [[130, 34], [160, 40]] as const) {
    const r = snapshot(`agents-rows-${w}x${h}`, observeFrame(monitorsModel('agents'), w, h));
    assert.ok(r.ok, r.message);
  }
});
