import test from 'node:test';
import assert from 'node:assert/strict';
import { InputParser } from '../src/input.ts';

const keys = (s: string) => new InputParser().feed(s);

test('text then return in one read is text and a keypress, not a paste', () => {
  assert.deepEqual(keys('hello\r'), [{ t: 'text', s: 'hello' }, { t: 'key', k: 'enter', alt: false, shift: false, ctrl: false }]);
});

test('bracketed paste keeps its newlines and can span reads', () => {
  const p = new InputParser();
  assert.deepEqual(p.feed('\x1b[200~line one\r'), []);
  assert.deepEqual(p.feed('line two\x1b[201~x'), [{ t: 'paste', s: 'line one\nline two' }, { t: 'text', s: 'x' }]);
});

test('mouse wheel, several in one read; clicks are ignored', () => {
  assert.deepEqual(keys('\x1b[<64;10;10M\x1b[<64;10;10M\x1b[<65;1;1M\x1b[<0;5;5M'), [
    { t: 'wheel', dir: 'up' }, { t: 'wheel', dir: 'up' }, { t: 'wheel', dir: 'down' },
  ]);
});

test('alt+enter, shift+arrows and page keys', () => {
  assert.deepEqual(keys('\x1b\r'), [{ t: 'key', k: 'enter', alt: true, shift: false, ctrl: false }]);
  assert.deepEqual(keys('\x1b[1;2C'), [{ t: 'key', k: 'right', alt: false, shift: true, ctrl: false }]);
  assert.deepEqual(keys('\x1b[5~\x1b[6~'), [
    { t: 'key', k: 'pgup', alt: false, shift: false, ctrl: false }, { t: 'key', k: 'pgdn', alt: false, shift: false, ctrl: false },
  ]);
  assert.deepEqual(keys('\x1b[Z'), [{ t: 'key', k: 'shift-tab', alt: false, shift: true, ctrl: false }]);
});

test('control letters, wide characters and a lone escape', () => {
  assert.deepEqual(keys('\x03\x12'), [{ t: 'ctrl', c: 'c' }, { t: 'ctrl', c: 'r' }]);
  assert.deepEqual(keys('é日🙂'), [{ t: 'text', s: 'é日🙂' }]);
  assert.deepEqual(keys('\x1b'), [{ t: 'key', k: 'esc', alt: false, shift: false, ctrl: false }]);
});

test('an escape sequence split across reads is completed, not dropped', () => {
  const p = new InputParser();
  assert.deepEqual(p.feed('\x1b[<64;1'), []);
  assert.deepEqual(p.feed('0;10M'), [{ t: 'wheel', dir: 'up' }]);
});

test('the background colour reply says dark or light', () => {
  assert.deepEqual(keys('\x1b]11;rgb:1a1a/1f1f/2929\x07'), [{ t: 'bg', dark: true }]);
  assert.deepEqual(keys('\x1b]11;rgb:ffff/ffff/ffff\x1b\\'), [{ t: 'bg', dark: false }]);
});

test('escape then a control key in one read is two keypresses', () => {
  assert.deepEqual(keys('\x1b\x03'), [{ t: 'key', k: 'esc', alt: false, shift: false, ctrl: false }, { t: 'ctrl', c: 'c' }]);
});

test('Shift+Enter arrives as modifyOtherKeys or kitty sequences and is a newline request', () => {
  const nl = { t: 'key', k: 'enter', alt: false, shift: true, ctrl: false };
  assert.deepEqual(keys('\x1b[27;2;13~'), [nl]);
  assert.deepEqual(keys('\x1b[13;2u'), [nl]);
  assert.deepEqual(keys('\x1b[99;5u'), [{ t: 'ctrl', c: 'c' }]);
  assert.deepEqual(keys('\x1b[97;1u'), [{ t: 'text', s: 'a' }]);
});
