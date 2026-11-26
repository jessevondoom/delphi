import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { HELD_RUN_MAX, copyAllowed, exportAllowed, keptWords } from '../src/ency.ts';

// spec 142 §2.5 item 4 (h) and AC-9: what the app can export, copy or print from its source view and a page.

test('a page the daemon says may not leave is neither exported nor printed; an older reply with no answer may be', () => {
  assert.equal(exportAllowed({ may_export: false }), false);
  assert.equal(exportAllowed({ may_export: true }), true);
  assert.equal(exportAllowed({}), true);
});

test('a held source is copied at most 300 characters at a time, whitespace counted collapsed; another source is copied whole', () => {
  const held = { text_held: true, may_export: false };
  const run = 'x'.repeat(HELD_RUN_MAX);
  assert.equal(copyAllowed(held, run), true);
  assert.equal(copyAllowed(held, `${run}y`), false);
  assert.equal(copyAllowed(held, `  ${'a  '.repeat(150)}`), true, '150 letters and 149 collapsed spaces');
  assert.equal(copyAllowed({ text_held: false, may_export: true }, 'z'.repeat(5000)), true);
  assert.equal(keptWords({ capture_level: 'full', private: false, meta: { text_held: true } }), 'its whole text is kept; kept for reading on this machine only: no licence allows copying it, so it is not exported, printed or copied past 300 characters');
});

test('the window asks both rules where it exports, prints and copies, and keeps a held text out of print', () => {
  const src = readFileSync(new URL('../src/screens/Encyclopedia.tsx', import.meta.url), 'utf8');
  assert.equal((src.match(/exportAllowed\(page\) && <button/g) ?? []).length, 2, 'the export and the print buttons');
  assert.ok(src.includes('onCopy=') && src.includes('copyAllowed(v, selected)') && src.includes('e.preventDefault()'), 'a copy past the bound is refused');
  assert.ok(src.includes("v.text_held ? 'stack no-print'"), 'a held text is not printed');
});
