// Plain-ASCII badges (spec 087 §6.2): `DELPHI_ASCII_BADGES=1` keeps every badge at five columns and draws only ASCII, from the
// table the Rust library sends (`ascii_glyphs` in test/fixtures/avatar-spec.json); the console keeps no table of its own.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import chalk from 'chalk';
import stringWidth from 'string-width';
import { ACTIVITY, asciiBadgesRequested, asciiCells, badge, currentAsciiBadges, currentCapabilities, poolBadge, useAsciiBadges, useCapabilities, type AgentState, type AvatarSpec, type Info } from '../src/theme.ts';
import { mascot } from '../src/observe-screen.ts';

const spec: AvatarSpec = JSON.parse(readFileSync(new URL('./fixtures/avatar-spec.json', import.meta.url), 'utf8'));
const roles = Object.keys(spec.roles);
const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0', cwd: '/',
  roles: Object.fromEntries(roles.map((r) => [r, { name: r, chassis: spec.roles[r]!.chassis, face: spec.roles[r]!.badge.idle!.cells }])),
  avatar: spec,
};
const STATES: AgentState[] = ['idle', 'running', 'waiting on you', 'escalated'];
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const isAscii = (s: string) => [...s].every((c) => (c.codePointAt(0) ?? 0) < 128);
/** Run `f` with the ASCII switch on or off and truecolour, then put both back. */
const withAscii = <T>(on: boolean, f: () => T): T => {
  const was = useAsciiBadges(on), l = chalk.level, c = currentCapabilities();
  useCapabilities({ color: 'truecolor', attributes: true });
  try { return f(); } finally { useAsciiBadges(was); useCapabilities(c); chalk.level = l; }
};

test('the switch is DELPHI_ASCII_BADGES=1, the way DELPHI_REDUCE_MOTION is', () => {
  assert.equal(asciiBadgesRequested({ DELPHI_ASCII_BADGES: '1' }), true);
  for (const v of [undefined, '', '0', 'true', 'yes']) assert.equal(asciiBadgesRequested({ DELPHI_ASCII_BADGES: v }), false, String(v));
  assert.equal(currentAsciiBadges(), asciiBadgesRequested(), 'the console starts with what the environment says');
});

test('the library sends the table, and it covers every glyph a badge or a mascot row can draw', () => {
  const table = spec.ascii_glyphs;
  assert.ok(table && Object.keys(table).length >= 18, 'ascii_glyphs is in the fixture');
  const used = new Set<string>(['⟦', '⟧']);
  for (const r of roles) {
    const lib = spec.roles[r]!;
    for (const b of Object.values(lib.badge)) for (const c of b.cells) used.add(c);
    for (const row of Object.values(lib.mascot)) for (const c of row) used.add(c);
    for (const s of lib.blink_steps ?? []) for (const c of s) used.add(c);
    for (const c of lib.mascot_face ?? '') used.add(c);
  }
  for (const c of used) if (!isAscii(c)) assert.ok(table![c], `\`${c}\` is drawn but the table has no ASCII stand-in`);
  for (const [g, a] of Object.entries(table!)) assert.ok(!isAscii(g) && a.length === 1 && isAscii(a), `${g} -> ${a}`);
});

test('with the switch on, every role in every state is a five-column ASCII badge; off, nothing changes', () => {
  for (const r of roles) for (const s of STATES) {
    const off = withAscii(false, () => plain(badge(r, s, info, { dark: true })));
    const on = withAscii(true, () => plain(badge(r, s, info, { dark: true })));
    assert.equal(stringWidth(on), 5, `${r} ${s}: ${on}`);
    assert.ok(isAscii(on) && on.startsWith('[') && on.endsWith(']'), `${r} ${s}: ${on}`);
    assert.equal(off, `⟦${spec.roles[r]!.badge[ACTIVITY[s]]!.cells}⟧`, `${r} ${s}: the Unicode badge is as before`);
    assert.equal(on, asciiOf(off), `${r} ${s}: the ASCII badge is the Unicode one through the library's table`);
  }
});

const asciiOf = (s: string) => [...s].map((c) => (isAscii(c) ? c : spec.ascii_glyphs![c] ?? '?')).join('');

test('the eyes keep their colour and the mouth keeps the role colour in ASCII', () => {
  const rgb = (hex: string) => { const n = parseInt(hex.slice(1), 16); return `\x1b[38;2;${n >> 16};${(n >> 8) & 255};${n & 255}m`; };
  const uni = withAscii(false, () => badge('red-team', 'idle', info, { dark: true }));
  const asc = withAscii(true, () => badge('red-team', 'idle', info, { dark: true }));
  assert.equal([...uni.matchAll(/\x1b\[[0-9;]*m/g)].map((m) => m[0]).join(''), [...asc.matchAll(/\x1b\[[0-9;]*m/g)].map((m) => m[0]).join(''), 'the same colour codes in the same order');
  assert.ok(asc.includes(rgb(spec.roles['red-team']!.chassis) + '_'), 'the mouth is chassis-coloured');
  assert.ok(plain(asc) === '[~_~]');
});

test('the mascot face row is ASCII too, still seven columns', () => {
  for (const r of roles) for (const s of STATES) {
    const rows = withAscii(true, () => mascot(r, s, info, { dark: false }));
    const face = plain(rows[1]);
    assert.ok(isAscii(face) && stringWidth(face) === 7, `${r} ${s}: '${face}'`);
  }
});

test('in ASCII the core roles stay apart (except the twins) and a blink still differs from its idle face', () => {
  const idle = new Map<string, string>();
  for (const r of roles.filter((x) => x !== 'operator')) {
    const lib = spec.roles[r]!;
    const i = asciiOf(lib.badge.idle!.cells), b = asciiOf(lib.badge.blink!.cells);
    assert.notEqual(i, b, `${r}: the blink looks like the idle face in ASCII (${i})`);
    for (const step of lib.blink_steps ?? []) assert.notEqual(asciiOf(step), i, `${r}: a blink frame equals idle (${i})`);
    const other = idle.get(i);
    if (other) assert.deepEqual([other, r].sort(), ['investigator', 'writer'], `${other} and ${r} are both ${i} in ASCII`);
    idle.set(i, r);
  }
});

test('with no table (an older delphi) or the switch off, text is left exactly as it is', () => {
  assert.equal(withAscii(true, () => asciiCells('⟦•─•⟧', { ...info, avatar: { ...spec, ascii_glyphs: undefined } })), '⟦•─•⟧');
  assert.equal(withAscii(true, () => asciiCells('⟦•─•⟧', { ...info, avatar: undefined })), '⟦•─•⟧');
  assert.equal(withAscii(false, () => asciiCells('⟦•─•⟧', info)), '⟦•─•⟧');
  assert.equal(withAscii(true, () => asciiCells('⟦•─•⟧', info)), '[0-0]');
  assert.equal(withAscii(true, () => asciiCells('♥_♥', info)), '?_?', 'a glyph the library does not know is a question mark, never a wide character');
});

// A one-off agent's badge (spec 087 §4) goes through the same switch as every other badge.
test('a one-off agent\'s badge is five ASCII columns under the switch, in every state, blink included', () => {
  const pool: Info = { ...info, pool: { 'pool-36': { name: 'Sisyphus', face: '×_×' }, 'pool-04': { name: 'Theseus', face: '◔_◔' } } };
  for (const face of ['×_×', '◔_◔', '•─•']) {
    for (const st of STATES) for (const frame of [undefined, 'blink']) {
      const on = plain(withAscii(true, () => poolBadge('builder', face, st, pool, { dark: true }, frame)));
      assert.equal(stringWidth(on), 5, `${face} ${st} ${frame}`);
      assert.ok(isAscii(on), `${face} ${st} ${frame} -> ${on}`);
      const off = plain(withAscii(false, () => poolBadge('builder', face, st, pool, { dark: true }, frame)));
      assert.ok(off.startsWith('⟦') && off.endsWith('⟧'), `off keeps the Unicode brackets: ${off}`);
    }
  }
});
