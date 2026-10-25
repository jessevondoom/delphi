// The badges and mascots are drawn from data the Rust avatar library produces (test/fixtures/avatar-spec.json, kept
// current by a Rust test). These tests walk every role in that file, so a role or face added to the library is
// checked here without anyone writing a TypeScript test for it.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import chalk from 'chalk';
import stringWidth from 'string-width';
import { ACTIVITY, badge, currentCapabilities, eyeColor, useCapabilities, type AgentState, type AvatarSpec, type Info } from '../src/theme.ts';
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
const rgb = (hex: string) => { const n = parseInt(hex.slice(1), 16); return `\x1b[38;2;${n >> 16};${(n >> 8) & 255};${n & 255}m`; };
const withColour = <T>(f: () => T): T => {
  const l = chalk.level, c = currentCapabilities();
  useCapabilities({ color: 'truecolor', attributes: true });
  try { return f(); } finally { useCapabilities(c); chalk.level = l; }
};

test('the fixture covers the library: every role, and every activity a state maps to', () => {
  assert.ok(roles.length >= 6 && roles.includes('red-team') && roles.includes('scout'), roles.join());
  for (const r of roles) for (const a of Object.values(ACTIVITY)) assert.ok(spec.roles[r]!.badge[a] && spec.roles[r]!.mascot[a], `${r} has ${a}`);
});

test('every badge is five columns, in every state, on dark and light terminals', () => {
  for (const r of roles) for (const s of STATES) for (const dark of [true, false]) {
    const b = badge(r, s, info, { dark });
    assert.equal(stringWidth(b), 5, `${r} ${s}`);
    assert.equal(plain(b), `⟦${spec.roles[r]!.badge[ACTIVITY[s]]!.cells}⟧`);
  }
});

test('eyes are deep slate on a light terminal and white on a dark one, for every role and state', () => {
  withColour(() => {
    for (const r of roles) for (const s of STATES) {
      const lib = spec.roles[r]!.badge[ACTIVITY[s]]!;
      const alert = s === 'waiting on you' ? spec.roles[r]!.alert_eye : undefined;
      for (const dark of [true, false]) {
        const want = rgb(alert ? (dark ? alert.dark : alert.light) : dark ? spec.eye.dark : spec.eye.light);
        const out = badge(r, s, info, { dark });
        [...lib.cells].forEach((c, i) => {
          if (c === ' ' || i === lib.mouth) return;
          assert.ok(out.includes(want + c), `${r} ${s} dark=${dark}: eye ${JSON.stringify(c)} at ${i} should be ${want.slice(1)}`);
        });
        if (!dark && !alert) assert.ok(!out.includes(rgb('#ffffff')), `${r} ${s}: nothing white on a light terminal`);
      }
    }
  });
});

test('the mouth is the cell the library says it is, not whichever character looks like one: the red-team\'s -_- has - eyes', () => {
  withColour(() => {
    const idle = spec.roles['red-team']!.badge.idle!;
    assert.equal(idle.cells, '-_-');
    const out = badge('red-team', 'idle', info, { dark: false });
    assert.ok(out.includes(rgb(spec.eye.light) + '-'), 'the first eye is eye-coloured');
    assert.ok(out.includes(rgb(spec.roles['red-team']!.chassis) + '_'), 'the mouth is chassis-coloured');
    assert.equal((out.match(new RegExp(rgb(spec.eye.light).replace(/\[/g, '\\['), 'g')) ?? []).length, 2, 'two eyes');
  });
});

test('the red-team\'s alert eyes have enough contrast on a light terminal', () => {
  const alert = spec.roles['red-team']!.alert_eye!;
  assert.notEqual(alert.light, alert.dark);
  assert.equal(eyeColor('red-team', 'waiting on you', info, { dark: false }), alert.light);
  assert.equal(eyeColor('red-team', 'idle', info, { dark: false }), spec.eye.light);
  const lum = (h: string) => { const c = [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)); return 0.2126 * c[0]! + 0.7152 * c[1]! + 0.0722 * c[2]!; };
  const contrast = (a: string, b: string) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x! + 0.05) / (y! + 0.05); };
  assert.ok(contrast(alert.light, '#ffffff') >= 4.5, `alert eyes on white: ${contrast(alert.light, '#ffffff').toFixed(1)}:1`);
  assert.ok(contrast(spec.eye.light, '#ffffff') >= 12, 'slate eyes on white');
  assert.ok(contrast(alert.dark, '#0f172a') >= 7, 'gold eyes on the dark background');
});

test('an older delphi that sends no spec still gets correct badges, eyes by position', () => {
  const old: Info = { ...info, avatar: undefined };
  withColour(() => {
    const out = badge('red-team', 'idle', old, { dark: false });
    assert.equal(plain(out), '⟦-_-⟧');
    assert.ok(out.includes(rgb('#0f172a') + '-'), 'eyes are dark on light even when they are dashes');
    assert.equal(plain(badge('scout', 'running', old, { dark: true })), '⟦···⟧');
  });
});

test('the mascot is seven columns in every state and its face row is the library\'s', () => {
  withColour(() => {
    for (const r of roles) for (const st of STATES) {
      const rows = mascot(r, st, info, { dark: false });
      for (const row of rows) assert.equal(stringWidth(row), 7, `${r} ${st}`);
      assert.equal(plain(rows[1]), spec.roles[r]!.mascot[ACTIVITY[st]], `${r} ${st}`);
    }
  });
});

test('light or dark: DELPHI_THEME wins, COLORFGBG is the fallback, nothing set means ask the terminal', async () => {
  const { envTheme } = await import('../src/theme.ts');
  assert.deepEqual(envTheme({ DELPHI_THEME: 'light', COLORFGBG: '15;0' }), { dark: false, forced: true });
  assert.deepEqual(envTheme({ DELPHI_THEME: 'dark' }), { dark: true, forced: true });
  assert.deepEqual(envTheme({ COLORFGBG: '0;15' }), { dark: false, forced: false }, 'a white background');
  assert.deepEqual(envTheme({ COLORFGBG: '15;0' }), { dark: true, forced: false });
  assert.deepEqual(envTheme({ COLORFGBG: '0;7' }), { dark: false, forced: false });
  assert.equal(envTheme({}), null);
  assert.equal(envTheme({ COLORFGBG: 'garbage' }), null);
});
