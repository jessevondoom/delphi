import test from 'node:test';
import assert from 'node:assert/strict';
import * as C from '../src/composer.ts';

const typed = (s: string) => C.insert(C.empty(), s);

test('insert in the middle, backspace, delete and wide characters', () => {
  let c = typed('hllo');
  c = C.insert({ ...C.left(C.left(C.left(c))) }, 'e');
  assert.equal(C.textOf(c), 'hello');
  c = C.del(C.backspace(c));
  assert.equal(C.textOf(c), 'hllo'.slice(0, 0) + 'hlo');
  assert.equal(C.textOf(typed('日本🙂')), '日本🙂');
});

test('kill word, to start and to end work on the current line', () => {
  assert.equal(C.textOf(C.killWord(typed('one two  '))), 'one ');
  assert.equal(C.textOf(C.killToStart(typed('a\nbcd'))), 'a\n');
  assert.equal(C.textOf(C.killToEnd(C.home(typed('a\nbcd')))), 'a\n');
});

test('up and down walk lines, then history, and the draft comes back', () => {
  let c = C.empty(['first', 'second']);
  c = C.insert(c, 'draft');
  c = C.up(c); assert.equal(C.textOf(c), 'second');
  c = C.up(c); assert.equal(C.textOf(c), 'first');
  c = C.down(c); c = C.down(c); assert.equal(C.textOf(c), 'draft');
  const m = typed('ab\ncd');
  assert.equal(m.cur, 5);
  assert.equal(C.up(m).cur, 2, 'same column on the line above, clamped');
});

test('submit trims, remembers once, and empties', () => {
  const { text, next } = C.submit(typed('  hi  '));
  assert.equal(text, 'hi');
  assert.deepEqual(next.hist, ['hi']);
  assert.equal(C.submit(C.insert(next, 'hi')).next.hist.length, 1, 'no duplicate in a row');
});

test('layout wraps at the width and finds the cursor row', () => {
  const l = C.layout(typed('abcdefgh'), 5);
  assert.deepEqual(l.rows.map((r) => r.before + r.at + r.after), ['abcde', 'fgh ']);
  assert.equal(l.cursorRow, 1);
  const two = C.layout(typed('ab\ncd'), 10);
  assert.equal(two.rows.length, 2);
});
