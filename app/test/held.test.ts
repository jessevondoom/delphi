import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { HELD_RUN_MAX, copyAllowed, copyDecision, exportAllowed, exportDecision, keptWords, labelledCopy, type SinkDecision } from '../src/ency.ts';

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
  assert.equal((src.match(/exportAllowed\(page\) && leaving !== 'refuse' && <button/g) ?? []).length, 2, 'the export and the print buttons');
  assert.ok(src.includes('onCopy=') && src.includes('copyDecision(v, selected)') && src.includes('e.preventDefault()'), 'a copy past the bound is refused');
  assert.ok(src.includes("v.text_held ? 'stack no-print'"), 'a held text is not printed');
});

// spec 141 §2.4 and AC-6, AC-15: the page-export and clipboard rows as the window follows them.

const decision = (d: SinkDecision['decision'], quotations = false): SinkDecision => ({ decision: d, reason: d === 'refuse' ? 'refused: personal (handling:x)' : d === 'labelled' ? 'internal (handling:y)' : '', quotations, line: d === 'labelled' ? '> Handling: INTERNAL' : null, marks: [] });

test('a refused page offers no export or print, a labelled one asks first, an unlabelled one is as before', () => {
  assert.equal(exportDecision({ export: decision('refuse') }), 'refuse');
  assert.equal(exportDecision({ may_export: false, export: decision('send') }), 'refuse', 'held text past the bound');
  assert.equal(exportDecision({ export: decision('labelled') }), 'labelled');
  assert.equal(exportDecision({ export: decision('send') }), 'send');
  assert.equal(exportDecision({}), 'send', 'an older reply: the Rust side refuses it');
  const src = readFileSync(new URL('../src/screens/Encyclopedia.tsx', import.meta.url), 'utf8');
  assert.ok(src.includes("leaving === 'labelled' ? setConfirming({ what })"), 'a labelled page is confirmed in the window before it is exported or printed');
  assert.ok(src.includes('bridge.exportPage(file.name, file.text, ref, confirmed)') && src.includes('bridge?.print(ref, confirmed)'), 'the app is told which page, and whether the operator confirmed');
});

test('the clipboard row: refuse, labelled with its line, and the quotation bound', () => {
  const long = 'y'.repeat(HELD_RUN_MAX + 1);
  assert.equal(copyDecision({ copy: decision('refuse') }, 'short'), 'refuse');
  assert.equal(copyDecision({ copy: decision('labelled') }, 'short'), 'labelled');
  assert.equal(copyDecision({ copy: decision('send', true) }, long), 'refuse');
  assert.equal(copyDecision({ copy: decision('send', true) }, 'short'), 'send');
  assert.equal(copyDecision({ copy: decision('labelled', true) }, long), 'refuse', 'a stricter decision keeps the bound');
  assert.equal(copyDecision({ copy: decision('send') }, long), 'send');
  assert.equal(labelledCopy('> Handling: INTERNAL', 'text'), '> Handling: INTERNAL\n\ntext');
  const src = readFileSync(new URL('../src/screens/Encyclopedia.tsx', import.meta.url), 'utf8');
  assert.equal((src.match(/copyDecision\((page|v), selected\)/g) ?? []).length, 2, 'the page and the source view both ask');
});
