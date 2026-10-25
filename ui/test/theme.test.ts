// The palette and the terminal's capabilities (`specs/005e` §5, slice 2 of 005f).
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import chalk from 'chalk';
import * as C from '../src/composer.ts';
import { initial } from '../src/fold.ts';
import { newInbox } from '../src/inbox.ts';
import { emptyState, observeFrame, VIEWS, type OModel } from '../src/observe-screen.ts';
import { frame, type Model } from '../src/screen.ts';
import * as T from '../src/theme.ts';
import { info, observerWorld, stateWorld } from './helpers.ts';

const SRC = join(dirname(fileURLToPath(import.meta.url)), '..', 'src');

test('no colour is written as a hex literal outside theme.ts', () => {
  const offenders: string[] = [];
  for (const f of readdirSync(SRC).filter((n) => /\.(ts|tsx)$/.test(n) && n !== 'theme.ts')) {
    readFileSync(join(SRC, f), 'utf8').split('\n').forEach((l, i) => {
      if (/['"`]#[0-9a-fA-F]{3,8}['"`]/.test(l) || /\b(?:hex|bgHex|rgb|bgRgb)\(/.test(l)) offenders.push(`${f}:${i + 1}: ${l.trim()}`);
    });
  }
  assert.deepEqual(offenders, [], 'colours come from theme.ts');
});

test('capabilities: NO_COLOR (any value) and TERM=dumb mean no colour; dumb also means no attributes', () => {
  assert.deepEqual(T.capabilities({ NO_COLOR: '1' }), { color: 'none', attributes: true });
  assert.deepEqual(T.capabilities({ NO_COLOR: '' }), { color: 'none', attributes: true }, 'set to anything, even nothing');
  assert.deepEqual(T.capabilities({ TERM: 'dumb' }), { color: 'none', attributes: false });
  assert.deepEqual(T.capabilities({ TERM: 'dumb', COLORTERM: 'truecolor' }), { color: 'none', attributes: false });
  assert.deepEqual(T.capabilities({ NO_COLOR: '1', COLORTERM: 'truecolor', TERM: 'xterm-256color' }), { color: 'none', attributes: true });
});

test('capabilities: the depth comes from COLORTERM, then TERM', () => {
  assert.equal(T.capabilities({ COLORTERM: 'truecolor', TERM: 'xterm' }).color, 'truecolor');
  assert.equal(T.capabilities({ COLORTERM: '24bit' }).color, 'truecolor');
  assert.equal(T.capabilities({ TERM: 'xterm-256color' }).color, '256');
  assert.equal(T.capabilities({ TERM: 'screen-256color', COLORTERM: '' }).color, '256');
  assert.equal(T.capabilities({ TERM: 'xterm' }).color, '16');
  assert.equal(T.capabilities({}).color, '16', 'nothing known: the safe answer');
  assert.equal(T.capabilities({ COLORTERM: 'truecolor' }, 'dumb').color, 'none', 'the second argument is the terminal type');
});

const BG = (s: string) => /\x1b\[(?:[0-9;]*;)?(?:4[0-7]|48|10[0-7])[;m]/.test(s);

test('tint: truecolor moves the background 8 % toward white on dark and 6 % toward black on light', () => {
  const l = chalk.level; chalk.level = 3;
  try {
    assert.ok(T.tint([15, 23, 42], true, 'truecolor')!.fill('x').includes('48;2;34;42;59'), 'dark slate, a step lighter');
    assert.ok(T.tint([255, 255, 255], false, 'truecolor')!.fill('x').includes('48;2;240;240;240'), 'white, a step darker');
    assert.ok(T.tint([0, 0, 0], true, 'truecolor')!.ink('▄').includes('38;2;20;20;20'), 'the cap is the same colour as the fill');
  } finally { chalk.level = l; }
});

test('tint: 256 colours use the grey ramp two steps from the background', () => {
  const l = chalk.level; chalk.level = 2;
  try {
    assert.ok(T.tint([0, 0, 0], true, '256')!.fill('x').includes('48;5;234'));
    assert.ok(T.tint([255, 255, 255], false, '256')!.fill('x').includes('48;5;253'));
    const mid = T.tint([128, 128, 128], true, '256')!.fill('x'); // ramp step 12 (value 128) + 2 = entry 246
    assert.ok(mid.includes('48;5;246'), mid);
  } finally { chalk.level = l; }
});

test('tint: 16 colours use ANSI 8 on dark and 7 on light, unless that is the background itself', () => {
  const l = chalk.level; chalk.level = 1;
  try {
    assert.ok(T.tint([0, 0, 0], true, '16')!.fill('x').includes('[100m'), 'bright black on a black terminal');
    assert.equal(T.tint([127, 127, 127], true, '16'), null, 'the terminal background is already ANSI 8');
    assert.ok(T.tint([255, 255, 255], false, '16')!.fill('x').includes('[47m'), 'white-ish on a white terminal');
    assert.equal(T.tint([229, 229, 229], false, '16'), null, 'the terminal background is already ANSI 7');
  } finally { chalk.level = l; }
});

test('tint: an unknown background, or no colour, means no tint: the rail fallback', () => {
  for (const color of ['truecolor', '256', '16'] as const) for (const dark of [true, false]) assert.equal(T.tint(null, dark, color), null, `${color} ${dark}`);
  assert.equal(T.tint([0, 0, 0], true, 'none'), null);
});

const model = (over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true }, state: stateWorld(), now: 5000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'mission-auto', panel: null, help: false, ...over,
});
const om = (view: (typeof VIEWS)[number]): OModel => ({ info, pal: { dark: true }, st: observerWorld(), view, pick: 0, scroll: 0, selNode: 1, search: { editing: false, query: '', hits: [], note: '' }, diff: null, live: {}, connected: true });

/** True when any SGR sequence in `s` sets a colour (foreground, background, bright or extended). */
function hasColour(s: string): boolean {
  for (const m of s.matchAll(/\x1b\[([0-9;]*)m/g)) {
    for (const p of m[1]!.split(';').map(Number)) {
      if ((p >= 30 && p <= 38) || (p >= 40 && p <= 48) || (p >= 90 && p <= 97) || (p >= 100 && p <= 107)) return true;
    }
  }
  return false;
}

test('with no colour, every screen writes no colour code, and still carries emphasis', () => {
  const prev = T.currentCapabilities();
  T.useCapabilities(T.capabilities({ NO_COLOR: '1' }));
  try {
    const screens = [
      ...frame(model(), 100, 34), ...frame(model({ deck: 0 }), 100, 24), ...frame(model({ deck: 2 }), 100, 24), ...frame(model({ state: initial(0), noModel: true }), 80, 24),
      ...VIEWS.flatMap((v) => observeFrame(om(v), 100, 30)), ...observeFrame({ ...om('epic'), st: emptyState() }, 80, 24),
    ].join('\n');
    assert.ok(!hasColour(screens), 'no colour SGR');
    assert.match(screens, /\x1b\[1m/, 'bold survives');
    assert.match(screens, /\x1b\[2m/, 'dim survives');
    assert.match(screens, /⟦/, 'badges keep their glyphs');
  } finally { T.useCapabilities(prev); }
});

test('with TERM=dumb nothing at all is written: no colour, no attributes', () => {
  const prev = T.currentCapabilities();
  T.useCapabilities(T.capabilities({ TERM: 'dumb' }));
  try {
    const screens = [...frame(model(), 100, 34), ...observeFrame(om('council'), 100, 30)].join('\n');
    assert.ok(!/\x1b\[[0-9;]*m/.test(screens), 'no SGR sequence at all');
  } finally { T.useCapabilities(prev); }
});

test('with 16 colours the semantic palette is the terminal\'s own ANSI colours, and role colours fall back to the nearest', () => {
  const prev = T.currentCapabilities();
  T.useCapabilities(T.capabilities({ TERM: 'xterm' }));
  try {
    assert.equal(T.accent('x'), '\x1b[36mx\x1b[39m', 'cyan, ANSI 6');
    assert.equal(T.ok('x'), '\x1b[32mx\x1b[39m');
    assert.equal(T.warn('x'), '\x1b[33mx\x1b[39m');
    assert.match(T.alert('x'), /^\x1b\[31m\x1b\[1mx/, 'red and bold');
    assert.equal(T.body('x'), 'x', 'body text is the terminal\'s default foreground');
    assert.ok(!/38;2/.test(frame(model(), 100, 30).join('\n')), 'no 24-bit colour at 16 colours');
    assert.ok(hasColour(T.roleColor('architect', info)('x')), 'a role still has a colour');
  } finally { T.useCapabilities(prev); }
});

test('the approval mode auto is warning-red and bold, never reverse video', () => {
  const prev = T.currentCapabilities();
  try {
    T.useCapabilities(T.capabilities({ COLORTERM: 'truecolor' }));
    assert.ok(T.auto('auto').includes('1m') && T.auto('auto').includes('38;2;'), 'orange under truecolor');
    T.useCapabilities(T.capabilities({ TERM: 'xterm' }));
    assert.ok(T.auto('auto').includes('[91m'), 'ANSI 9 at 16 colours');
    T.useCapabilities(T.capabilities({ NO_COLOR: '1' }));
    assert.equal(T.auto('auto'), '\x1b[1mauto\x1b[22m', 'bold alone under NO_COLOR');
    assert.ok(!BG(T.auto('auto')) && !T.auto('auto').includes('[7m'));
  } finally { T.useCapabilities(prev); }
});
