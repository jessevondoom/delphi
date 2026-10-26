// The agents view in two columns on a wide window (`specs/034`): the roster beside the tools board, each clipped to its own column.
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { joinColumns, leftWidth, TWO_COLUMNS_AT } from '../src/columns.ts';
import { observeFrame } from '../src/observe-screen.ts';
import { monitorsModel } from './monitors-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

test('joined columns are exactly as wide as the window, whatever their rows hold', () => {
  const rows = joinColumns(['short', 'x'.repeat(200), '\x1b[31mred\x1b[0m'], ['a', 'b'.repeat(200), 'c', 'only right'], 130, { dark: true });
  assert.equal(rows.length, 4, 'as many rows as the longer column');
  for (const r of rows) assert.ok(stringWidth(plain(r)) <= 130, `${stringWidth(plain(r))}`);
  assert.equal(rows.filter((r) => plain(r).includes(' │ ')).length, 4, 'every row has the bar, so the columns stay aligned');
  assert.equal(plain(rows[0]!).indexOf(' │ '), leftWidth(130));
});

test('only a wide window gets two columns; a narrower one keeps one', () => {
  const wide = observeFrame(monitorsModel('agents'), TWO_COLUMNS_AT, 34).map(plain).join('\n');
  assert.match(wide, /hephaestus[^\n]* │ [^\n]*tools/, 'the roster and the tools board on the same rows');
  const narrow = observeFrame(monitorsModel('agents'), TWO_COLUMNS_AT - 1, 34).map(plain).join('\n');
  assert.doesNotMatch(narrow, / │ /);
  assert.doesNotMatch(narrow, /reports its work/, 'the tools board is its own view there');
});

test('in two columns the tool counters get a row of their own and the loop warning stays on the screen', () => {
  const text = observeFrame(monitorsModel('agents'), 130, 34).map(plain).join('\n');
  assert.match(text, /~740\.0k tokens +▓+░* 82%[^\n]* │/, 'tokens and the cap meter on the third row');
  assert.match(text, /\n +41 calls · ✎ 2 · ✗ 6 · → run_command[^\n]* │/, 'the counters on the fourth');
  assert.match(text, /⟳ going round in circles: hephaestus run_command ×7/, 'and the board repeats the loop in full');
});

test('goldens: the agents and tools side by side at 130x34 and 160x40', () => {
  for (const [w, h] of [[130, 34], [160, 40]] as const) {
    const r = snapshot(`agents-two-columns-${w}x${h}`, observeFrame(monitorsModel('agents'), w, h));
    assert.ok(r.ok, r.message);
  }
});
