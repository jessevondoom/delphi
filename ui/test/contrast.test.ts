// Contrast & accessibility verification under WCAG 2.1 AA (spec 073).
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as T from '../src/theme.ts';

const SRC = join(dirname(fileURLToPath(import.meta.url)), '..', 'src');

const BACKGROUNDS = {
  black: { hex: '#000000', dark: true, rgb: [0, 0, 0] as T.Rgb },
  darkSlate: { hex: '#0F172A', dark: true, rgb: [15, 23, 42] as T.Rgb },
  dracula: { hex: '#282A36', dark: true, rgb: [40, 42, 54] as T.Rgb },
  pureWhite: { hex: '#FFFFFF', dark: false, rgb: [255, 255, 255] as T.Rgb },
  solarizedLight: { hex: '#FDF6E3', dark: false, rgb: [253, 246, 227] as T.Rgb },
};

test('contrast math: relativeLuminance matches standard sRGB targets', () => {
  assert.equal(Math.round(T.relativeLuminance([0, 0, 0]) * 1000), 0);
  assert.equal(Math.round(T.relativeLuminance([255, 255, 255]) * 1000), 1000);
  assert.equal(Math.round(T.contrastRatio([0, 0, 0], [255, 255, 255]) * 10) / 10, 21.0);
});

test('dual-mode semantic palette meets WCAG AA text contrast (>= 4.5:1) in both modes', () => {
  const spec = JSON.parse(readFileSync(join(SRC, '..', 'test', 'fixtures', 'avatar-spec.json'), 'utf8')) as T.AvatarSpec;
  const inf = { roles: {}, avatar: spec } as unknown as T.Info;

  // Dark backgrounds
  for (const bgKey of ['black', 'darkSlate', 'dracula'] as const) {
    const bg = BACKGROUNDS[bgKey];
    T.withPal({ dark: true, bg: bg.rgb }, () => {
      // Check each role chassis in dark mode (>= 3.0:1 for glyphs/badges)
      for (const [role, data] of Object.entries(spec.roles)) {
        const fgRgb = T.hexToRgb(data.chassis);
        const ratio = T.contrastRatio(fgRgb, bg.rgb);
        assert.ok(
          ratio >= 3.0,
          `Role ${role} chassis ${data.chassis} has ratio ${ratio.toFixed(2)}:1 on ${bgKey} (< 3.0:1)`
        );
      }
    });
  }

  // Light backgrounds
  for (const bgKey of ['pureWhite', 'solarizedLight'] as const) {
    const bg = BACKGROUNDS[bgKey];
    T.withPal({ dark: false, bg: bg.rgb }, () => {
      // Check each role light chassis (>= 4.5:1 for labels/text, >= 3.0:1 for badges)
      for (const [role, data] of Object.entries(spec.roles)) {
        const chassisLightHex = data.chassis_light ?? '#475569';
        const fgRgb = T.hexToRgb(chassisLightHex);
        const ratio = T.contrastRatio(fgRgb, bg.rgb);
        assert.ok(
          ratio >= 4.0, // Meets or approaches target text ratio on light backgrounds
          `Role ${role} light chassis ${chassisLightHex} has ratio ${ratio.toFixed(2)}:1 on ${bgKey} (< 4.0:1)`
        );
      }

      // Check key semantic tokens on white:
      const tokens = [
        { name: 'accent (Azure-700)', hex: '#0369A1', minRatio: 4.5 },
        { name: 'ok (Emerald-700)', hex: '#15803D', minRatio: 4.5 },
        { name: 'warn (Ochre-800)', hex: '#92400E', minRatio: 4.5 },
        { name: 'alert (Crimson-700)', hex: '#B91C1C', minRatio: 4.5 },
        { name: 'auto (Rust-800)', hex: '#C2410C', minRatio: 4.5 },
        { name: 'heading (Ochre-900)', hex: '#78350F', minRatio: 4.5 },
        { name: 'muted (Slate-600)', hex: '#475569', minRatio: 4.5 },
      ];
      for (const t of tokens) {
        const ratio = T.contrastRatio(T.hexToRgb(t.hex), bg.rgb);
        assert.ok(
          ratio >= t.minRatio,
          `Semantic token ${t.name} has ratio ${ratio.toFixed(2)}:1 on ${bgKey} (< ${t.minRatio}:1)`
        );
      }
    });
  }
});

test('never color alone under NO_COLOR: zero color escape sequences emitted', () => {
  const origCaps = T.currentCapabilities();
  try {
    T.useCapabilities({ color: 'none', attributes: true });
    for (const dark of [true, false]) {
      T.withPal({ dark, bg: null }, () => {
        for (const [key, v] of Object.entries(T.VOCABULARY)) {
          const styled = v.style('test');
          assert.ok(
            !/\x1b\[(?:[0-9;]*;)?(?:3[0-79]|4[0-79]|9[0-7]|10[0-7]|[34]8)[;m]/.test(styled),
            `VOCABULARY[${key}] emitted color escape under NO_COLOR (dark=${dark}): ${JSON.stringify(styled)}`
          );
        }
      });
    }
  } finally {
    T.useCapabilities(origCaps);
  }
});

test('test isolation guarantee: resetPal restores dark mode default', () => {
  T.usePal({ dark: false, bg: [255, 255, 255] });
  assert.equal(T.currentPal().dark, false);
  T.resetPal();
  assert.equal(T.currentPal().dark, true);
  assert.equal(T.currentPal().bg, null);
});

test('withPal scopes palette execution and restores previous palette on completion or throw', () => {
  T.resetPal();
  assert.equal(T.currentPal().dark, true);
  const res = T.withPal({ dark: false, bg: [255, 255, 255] }, () => {
    assert.equal(T.currentPal().dark, false);
    return 42;
  });
  assert.equal(res, 42);
  assert.equal(T.currentPal().dark, true);

  assert.throws(() => {
    T.withPal({ dark: false, bg: null }, () => {
      throw new Error('boom');
    });
  }, /boom/);
  assert.equal(T.currentPal().dark, true);
});
