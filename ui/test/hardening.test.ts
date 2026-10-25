process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import stringWidth from 'string-width';
import * as C from '../src/composer.ts';
import { completions } from '../src/complete.ts';
import { addBlock, fold, initial } from '../src/fold.ts';
import { newInbox } from '../src/inbox.ts';
import { emptyState, observeFrame, VIEWS, type OModel } from '../src/observe-screen.ts';
import { frame, transcript, WINDOW, type Model } from '../src/screen.ts';
import { ON, OFF } from '../src/terminal.ts';
import { graphemes } from '../src/text.ts';
import { clip, sanitize, spread, type Info } from '../src/theme.ts';

const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: {
    orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' },
    architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' },
    'red-team': { name: 'ares', chassis: '#ef4444', face: '-_-' },
  },
};
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

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

test('layout counts display columns: wide glyphs wrap by width and the cursor stays on the last row', () => {
  const c = C.insert(C.empty(), '日本語日本語日本語');
  const l = C.layout(c, 8);
  assert.equal(l.cursorRow, l.rows.length - 1);
  for (const r of l.rows) assert.ok(stringWidth(r.before + r.after) <= 8, `${r.before}${r.after}`);
  assert.ok(l.rows.length >= 3);
});

test('completion works on characters, not UTF-16 units: an emoji before the @ does not shift the token', () => {
  const c = C.insert(C.empty(), '🙂 @arch');
  const got = completions(c.chars, c.cur, info, () => []);
  assert.ok(got, 'a completion is offered');
  assert.equal(got!.items[0]!.label, '@architect');
  assert.deepEqual([got!.from, got!.to], [2, 7], 'indexes count characters');
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

// ── a session that keeps going, and a window that shrinks ───────────────────

const model = (over: Partial<Model> = {}): Model => ({ info, pal: { dark: true, bg: [15, 23, 42] }, state: initial(0), now: 5000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null, queue: [], inbox: newInbox(0), mode: 'default', panel: null, help: false, ...over });

test('a very long session draws a bounded window and says so; the work per frame does not grow with it', () => {
  let s = initial(0);
  for (let i = 0; i < 20_000; i++) s = addBlock(s, { kind: i % 2 ? 'reply' : 'user', role: 'orchestrator', text: `message ${i}\n\nwith **markdown** and \`code\``, at: i });
  const m = model({ state: s });
  const t0 = performance.now();
  const lines = transcript(m, 100);
  const cold = performance.now() - t0;
  assert.ok(lines.some((l) => plain(l).includes('earlier messages are not drawn')), 'the cut is announced');
  assert.ok(lines.length < WINDOW * 8, `bounded: ${lines.length} lines`);
  assert.ok(plain(lines.join('\n')).includes('message 19999'), 'the newest is there');
  assert.ok(!plain(lines.join('\n')).includes('message 100\n'), 'the oldest is not drawn');
  const t1 = performance.now();
  for (let i = 0; i < 20; i++) frame(m, 100, 40);
  const warm = (performance.now() - t1) / 20;
  assert.ok(cold < 4000, `cold ${cold.toFixed(0)} ms`);
  assert.ok(warm < 60, `a frame took ${warm.toFixed(1)} ms with 20000 messages`);
});

const SIZES = [[1, 1], [2, 2], [5, 3], [10, 4], [20, 5], [30, 8], [40, 10], [8, 40], [200, 60]] as const;

test('the console is exactly w by h and no wider, in every deck, down to a terminal that is barely there', () => {
  let s = initial(0);
  s = addBlock(s, { kind: 'user', text: `hello 日本語 🙂 ${family}`, at: 0 });
  s = fold(s, { method: 'event', params: { type: 'approval.requested', task_id: 't1', payload: { approval_id: 'a1', role: 'architect', tier: 'red', payload_blake3: 'h', payload: { preview: 'ratify' }, summary: 'Ratify 日本語', timeout_secs: 60, tool: 'ratify' } } }, 1000);
  const composer = C.insert(C.empty(), `draft 日本語🙂 ${family}`);
  for (const [w, h] of SIZES) for (const deck of [0, 1, 2] as const) {
    const lines = frame(model({ state: s, composer, deck }), w, h);
    assert.equal(lines.length, h, `${w}x${h} deck ${deck}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${w}x${h} deck ${deck}: ${plain(l)}`);
  }
});

test('the Observer window is exactly w by h and no wider, in every view', () => {
  const st = emptyState();
  st.agents.architect = { role: 'architect', state: 'running', task_id: 't1', endpoint: 'direct', tier: 'fast', model: 'haiku', upscaled: false, tokens: { input: 1, cache_read: 0, cache_write: 0, output: 1, thinking: 0 }, cost_usd: 0.01, calls: 1 };
  for (const [w, h] of SIZES) for (const view of VIEWS) {
    const m: OModel = { info, pal: { dark: true }, st, view, pick: 0, scroll: 0, selNode: 0, search: { editing: false, query: '', hits: [], note: '' }, diff: null, live: {}, connected: true };
    const lines = observeFrame(m, w, h);
    assert.equal(lines.length, h, `${view} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${view} ${w}x${h}: ${plain(l)}`);
  }
});
