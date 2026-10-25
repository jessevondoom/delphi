import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import { extract, isEmpty, paint, wordAt, copyToClipboard } from '../src/select.ts';
import { InputParser } from '../src/input.ts';
import { ON, OFF } from '../src/terminal.ts';
chalk.level = 3;

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const lines = [chalk.bold('delphi v0.2.5') + '   ', 'the quick brown fox', '', chalk.red('日本語 mixed') + ' end', 'last line   '];

test('a selection inside one line is exactly the cells dragged over, without colour codes', () => {
  assert.equal(extract(lines, { a: { x: 4, y: 1 }, b: { x: 8, y: 1 } }), 'quick');
  assert.equal(extract(lines, { a: { x: 8, y: 1 }, b: { x: 4, y: 1 } }), 'quick', 'dragged backwards');
  assert.equal(extract(lines, { a: { x: 0, y: 0 }, b: { x: 5, y: 0 } }), 'delphi');
});

test('across lines it takes the rest of the first, the whole middle and the start of the last, trailing blanks dropped', () => {
  assert.equal(extract(lines, { a: { x: 4, y: 1 }, b: { x: 4, y: 4 } }), 'quick brown fox\n\n日本語 mixed end\nlast');
  assert.equal(extract(lines, { a: { x: 0, y: 0 }, b: { x: 40, y: 1 } }), 'delphi v0.2.5\nthe quick brown fox', 'past the end of a line is the end of it');
});

test('wide characters count two cells and are never split in the copy', () => {
  assert.equal(extract(lines, { a: { x: 0, y: 3 }, b: { x: 1, y: 3 } }), '日');
  assert.equal(extract(lines, { a: { x: 1, y: 3 }, b: { x: 2, y: 3 } }), '日本', 'a cell inside a wide character selects the whole of it');
  assert.equal(extract(lines, { a: { x: 7, y: 3 }, b: { x: 11, y: 3 } }), 'mixed');
});

test('a double-click takes the word, a blank takes nothing', () => {
  assert.deepEqual(wordAt(lines[1]!, 6), [4, 8]);
  assert.deepEqual(wordAt(lines[1]!, 0), [0, 2]);
  assert.equal(wordAt(lines[1]!, 3), null, 'the space between words');
  assert.deepEqual(wordAt(lines[3]!, 3), [0, 5], 'the wide characters are one word: three cells of two');
  assert.deepEqual(wordAt(lines[3]!, 8), [7, 11]);
});

test('painting reverses exactly the selected cells and keeps the line a line', () => {
  const out = paint(lines, { a: { x: 4, y: 1 }, b: { x: 8, y: 1 } });
  assert.equal(out[1], 'the \x1b[7mquick\x1b[27m brown fox');
  assert.deepEqual([out[0], out[2], out[4]], [lines[0], lines[2], lines[4]], 'other lines are untouched');
  assert.equal(plain(out[1]!).replace(/\x1b\[[0-9]+m/g, ''), 'the quick brown fox');
  assert.equal(paint(lines, { a: { x: 3, y: 1 }, b: { x: 3, y: 1 } }), lines, 'a click selects nothing');
  assert.ok(isEmpty({ a: { x: 3, y: 1 }, b: { x: 3, y: 1 } }));
});

test('a colour reset inside the selection cannot switch the highlight off', () => {
  const out = paint([chalk.red('ab') + 'cd'], { a: { x: 0, y: 0 }, b: { x: 3, y: 0 } })[0]!;
  const after = out.slice(out.indexOf('ab'));
  assert.ok(/\x1b\[39m\x1b\[7m/.test(after) || /\x1b\[0m\x1b\[7m/.test(after), JSON.stringify(out));
  assert.ok(out.endsWith('\x1b[27m'));
});

test('painting a multi-line selection reverses the tail, the middle and the head', () => {
  const out = paint(['one two', 'three', 'four five'], { a: { x: 4, y: 0 }, b: { x: 3, y: 2 } });
  assert.deepEqual(out.map((l) => l.replace(/\x1b\[7m/g, '[').replace(/\x1b\[27m/g, ']')), ['one [two]', '[three]', '[four] five']);
});

test('the mouse: press, drag and release become events at 0-based cells; other buttons and the wheel are as before', () => {
  assert.deepEqual(new InputParser().feed('\x1b[<0;5;3M\x1b[<32;9;3M\x1b[<0;9;3m'), [
    { t: 'mouse', k: 'down', x: 4, y: 2 }, { t: 'mouse', k: 'drag', x: 8, y: 2 }, { t: 'mouse', k: 'up', x: 8, y: 2 },
  ]);
  assert.deepEqual(new InputParser().feed('\x1b[<4;5;3M'), [{ t: 'mouse', k: 'down', x: 4, y: 2 }], 'shift held is still the left button');
  assert.deepEqual(new InputParser().feed('\x1b[<2;5;3M\x1b[<2;5;3m\x1b[<35;5;3M'), [], 'right button and bare motion are not used');
  assert.deepEqual(new InputParser().feed('\x1b[<64;1;1M'), [{ t: 'wheel', dir: 'up' }]);
});

test('the terminal asks for drags and gives them back', () => {
  assert.ok(ON.includes('\x1b[?1002h') && ON.includes('\x1b[?1006h'));
  assert.ok(OFF.includes('\x1b[?1002l') && OFF.includes('\x1b[?1000l'));
});

test('copying writes the terminal clipboard sequence with the text in base64', () => {
  const out: string[] = [];
  copyToClipboard('hello 日本', (s) => out.push(s), []); // no machine tool: a test must not touch the real clipboard
  assert.equal(out[0], `\x1b]52;c;${Buffer.from('hello 日本').toString('base64')}\x07`);
  copyToClipboard('', (s) => out.push(s), []);
  assert.equal(out.length, 1, 'nothing selected, nothing written');
});
