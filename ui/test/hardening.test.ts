process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import stringWidth from 'string-width';
import * as C from '../src/composer.ts';
import { completions } from '../src/complete.ts';
import { addBlock, initial } from '../src/fold.ts';
import { inboxInit } from '../src/inbox.ts';
import { panelInit, VIEWS } from '../src/panels.ts';
import { frame, transcript, WINDOW, type Model } from '../src/screen.ts';
import { graphemes } from '../src/text.ts';
import { clip, sanitize, spread } from '../src/theme.ts';
import { ON, OFF } from '../src/terminal.ts';
import { info, observerWorld, plain, stateWorld } from './helpers.ts';

// ── the terminal is always given back ───────────────────────────────────────

const fixture = fileURLToPath(new URL('./fixtures/crash.ts', import.meta.url));
const crash = (mode: string) => spawnSync(process.execPath, ['--import', 'tsx', fixture, mode], { encoding: 'utf8', timeout: 20_000, stdio: ['ignore', 'pipe', 'pipe'] });

for (const [mode, code, says] of [['exit', 0, ''], ['throw', 1, 'boom'], ['reject', 1, 'boom'], ['term', 143, ''], ['hup', 129, ''], ['int', 130, '']] as const) {
  test(`terminal restore: ${mode} leaves the alternate screen, mouse and paste modes and shows the cursor`, () => {
    const r = crash(mode);
    assert.equal(r.status, code, `${r.stdout}\n${r.stderr}`);
    assert.ok(r.stdout.startsWith(ON), 'the console took the terminal over first');
    assert.ok(r.stdout.endsWith(OFF), `and the last thing written is the restore sequence: ${JSON.stringify(r.stdout.slice(-60))}`);
    assert.equal(r.stdout.split(OFF).length - 1, 1, 'restored once, not twice');
    if (says) assert.match(r.stderr, new RegExp(says), 'the error is on stderr, after the screen was given back');
  });
}

// ── text that is wider than it looks ────────────────────────────────────────

const family = '👨‍👩‍👧';
const flag = '🇯🇵';
const accent = 'é';

test('an emoji with joiners, a flag and an accented letter are one character to edit', () => {
  assert.equal(graphemes(`a${family}${flag}${accent}`).length, 4);
  let c = C.insert(C.empty(), `x${family}`);
  assert.equal(c.chars.length, 2);
  c = C.backspace(c);
  assert.equal(C.textOf(c), 'x', 'one backspace removes the whole family');
  c = C.insert(c, `${accent}${flag}`);
  assert.equal(c.cur, 3);
  c = C.left(C.left(c));
  assert.equal(C.textOf(C.del(c)), `x${flag}`, 'delete removes the accented letter whole');
});

test('layout counts display columns: wide glyphs wrap by width and the cursor stays on its row', () => {
  const c = C.insert(C.empty(), '日本語日本語日本語');
  const l = C.layout(c, 8);
  assert.deepEqual(l.rows.map((r) => stringWidth(r.before + r.at + r.after)), [8, 8, 8, 2].slice(0, l.rows.length).map((n, i, a) => (i === a.length - 1 ? stringWidth(l.rows[i]!.before + l.rows[i]!.after + l.rows[i]!.at) : n)));
  assert.equal(l.cursorRow, l.rows.length - 1);
  for (const r of l.rows) assert.ok(stringWidth(r.before + r.after) <= 8);
});

test('completion works on characters, not UTF-16 units: an emoji before the @ does not shift the token', () => {
  const c = C.insert(C.empty(), '🙂 @arch');
  const got = completions(c.chars, c.cur, info, () => []);
  assert.ok(got, 'a completion is offered');
  assert.equal(got!.items[0]!.label, '@architect');
  assert.deepEqual([got!.from, got!.to], [2, 7], 'indexes count characters');
  const slash = C.insert(C.empty(), '/pan');
  assert.equal(completions(slash.chars, slash.cur, info, () => [])!.items[0]!.label, '/panel');
});

test('clip never cuts a wide glyph or a joined emoji in half, keeps styling, and fits', () => {
  for (const s of [`${family} ${family} ${family}`, '日本語日本語日本語', `\x1b[31m${flag}${flag}${flag}${flag}\x1b[0m`, 'abc日本語def']) {
    for (let w = 1; w <= stringWidth(s) + 1; w++) {
      const out = clip(s, w);
      assert.ok(stringWidth(out) <= w, `${JSON.stringify(s)} at ${w}: ${stringWidth(out)}`);
      if (stringWidth(s) <= w) assert.equal(out, s);
      assert.ok(!/[\ud800-\udbff]$|‍$/.test(plain(out).replace('…', '')), 'no dangling half of a pair or joiner');
    }
  }
});

test('spread gives way to the left side when the terminal is narrower than the hint', () => {
  for (let w = 1; w < 60; w++) assert.ok(stringWidth(spread('status text here', 'navigate: shift+arrows · ○ ● ○', w)) <= w, `w=${w}`);
  assert.equal(plain(spread('left', 'right', 12)), 'left   right');
});

test('sanitize removes escape sequences, controls and bidi overrides, keeps text and newlines', () => {
  assert.equal(sanitize('a\x1b[31mb\x1b[0m\x1b]0;title\x07c‮dcba⁦x⁩\x07\x00\td\ne'), 'abcdcbax\td\ne');
});

// ── a session that keeps going ──────────────────────────────────────────────

const model = (over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true }, state: initial(0), now: 5000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], help: false, inbox: inboxInit(), mode: 'default', obs: observerWorld(), panel: null, ...over,
});

test('a very long session draws a bounded window and says so; the work per frame does not grow with it', () => {
  let s = stateWorld();
  for (let i = 0; i < 20_000; i++) s = addBlock(s, { kind: i % 2 ? 'reply' : 'user', role: 'orchestrator', text: `message ${i}\n\nwith **markdown** and \`code\``, at: i });
  const m = model({ state: s });
  const t0 = performance.now();
  const lines = transcript(m, 100);
  const cold = performance.now() - t0;
  assert.ok(lines.some((l) => plain(l).includes(`earlier messages are not drawn`)), 'the cut is announced');
  assert.ok(lines.length < WINDOW * 8, `bounded: ${lines.length} lines`);
  assert.ok(plain(lines.join('\n')).includes('message 19999'), 'the newest is there');
  assert.ok(!plain(lines.join('\n')).includes('message 100\n'), 'the oldest is not drawn');
  const t1 = performance.now();
  for (let i = 0; i < 20; i++) frame(m, 100, 40);
  const warm = (performance.now() - t1) / 20;
  assert.ok(cold < 4000, `cold ${cold.toFixed(0)} ms`);
  assert.ok(warm < 60, `a frame took ${warm.toFixed(1)} ms with 20000 messages`);
});

test('every screen is exactly w by h and no wider, down to a terminal that is barely there', () => {
  let s = stateWorld();
  s = addBlock(s, { kind: 'user', text: 'hello 日本語 🙂 ' + family, at: 0 });
  const composer = C.insert(C.empty(), 'draft 日本語🙂 ' + family);
  for (const [w, h] of [[1, 1], [2, 2], [5, 3], [10, 4], [20, 5], [30, 8], [40, 10], [8, 40], [200, 60]] as const) {
    for (const deck of [0, 1, 2] as const) for (const view of [null, ...VIEWS]) {
      const lines = frame(model({ state: s, composer, deck, panel: view ? { ui: panelInit(view), cap: 1, pending: 1, mission: 'm' } : null }), w, h);
      assert.equal(lines.length, h, `${w}x${h} deck ${deck} ${view}`);
      for (const l of lines) assert.ok(stringWidth(l) <= w, `${w}x${h} deck ${deck} ${view}: ${plain(l)}`);
    }
  }
});
