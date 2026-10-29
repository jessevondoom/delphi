// The knowledge window's Sources and Claims views (specs/055 SK2, SK3; vocabulary of specs/053 VL4).
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import * as A from '../src/archives.ts';
import type { Ev } from '../src/input.ts';
import { info, snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const k = (key: string): Ev => ({ t: 'key', k: key as never, alt: false, shift: false, ctrl: false });
const ctrlL: Ev = { t: 'ctrl', c: 'l' };
const NOW = Date.parse('2026-10-28T10:00:00Z');

const SOURCES = [
  { id: 'wikipedia-sky-1', meta: { title: 'Rayleigh scattering', capture_level: 'full', licence: 'CC-BY-SA-4.0', version: '2', last_checked: '2026-10-26T10:00:00Z', cited_by: ['c-1', 'c-2'], status: 'not validated' } },
  { id: 'private-notes', meta: { title: 'My notes', capture_level: 'full', licence: 'private', version: '1', last_checked: '2026-09-01T10:00:00Z', cited_by: [], private: true } },
];
const CLAIMS = [
  { id: 'c-1', text: 'The sky is blue because of Rayleigh scattering', kind: 'fact', scope: 'team', status: 'validated', created: '2026-10-20T10:00:00Z', last_confirmed: '2026-10-26T10:00:00Z', cited_by: ['m1'], contested_by: [], anchors: [{ source_id: 'wikipedia-sky-1', locator: 'p1', quote: 'Rayleigh scattering', state: 'active' }] },
  { id: 'c-2', text: 'Water boils at 90C', kind: 'fact', scope: 'team', status: 'disproved', created: '2026-09-01T10:00:00Z', last_confirmed: '2026-09-01T10:00:00Z', cited_by: [], contested_by: ['c-9'], anchors: [{ source_id: 'wikipedia-sky-1', locator: 'p2', quote: 'boils at 100C', state: 'lost' }] },
  { id: 'c-3', text: 'A new note', kind: 'episode', scope: 'personal', status: 'mystery', created: '2026-10-28T09:00:00Z', anchors: [] },
];

const inConsole = () => A.emptyArch(true);
const walk = (a: A.Arch, e: Ev) => A.key(a, e, 100, 30);
const onClaims = () => A.gotLibrary(walk(walk(inConsole(), ctrlL).a, ctrlL).a, 'claims', CLAIMS);

test('ctrl+l walks search, sources, claims and back, asking for each list', () => {
  const s = walk(inConsole(), ctrlL);
  assert.equal(s.a.mode, 'sources');
  assert.deepEqual(s.action, { t: 'library', what: 'sources' });
  const c = walk(s.a, ctrlL);
  assert.equal(c.a.mode, 'claims');
  assert.deepEqual(c.action, { t: 'library', what: 'claims' });
  assert.equal(walk(c.a, ctrlL).a.mode, 'search');
  assert.equal(walk(s.a, k('esc')).a.mode, 'search');
  assert.equal(walk(s.a, k('left')).used, false, '← → still change the view of the window');
});

test('the narrator window only reads: ctrl+l says so and changes nothing', () => {
  const r = walk(A.emptyArch(false), ctrlL);
  assert.equal(r.a.mode, 'search');
  assert.deepEqual(r.action, { t: 'none' });
  assert.match(r.a.note, /only reads/);
});

test('sources list, pick, open one: its excerpts arrive for the id that was opened', () => {
  let a = A.gotLibrary(walk(inConsole(), ctrlL).a, 'sources', SOURCES);
  a = walk(a, k('down')).a;
  assert.equal(a.lib.sel, 1);
  assert.equal(walk(a, k('down')).a.lib.sel, 1, 'the list stops at its end');
  const o = walk(a, k('enter'));
  assert.deepEqual(o.action, { t: 'excerpts', id: 'private-notes' });
  const late = A.gotExcerpts(o.a, 'wikipedia-sky-1', [{ id: 'e1', locator: 'p1', purpose: 'x', bytes: 5 }]);
  assert.equal(late.lib.detail?.k === 'source' && late.lib.detail.excerpts, null, 'an answer for another source is dropped');
  const got = A.gotExcerpts(o.a, 'private-notes', [{ id: 'e1', locator: 'p1', purpose: 'because', bytes: 5, contested: true }]);
  const text = plain(A.view(got, 100, 24, { dark: true }, info, NOW).join('\n'));
  assert.match(text, /My notes/);
  assert.match(text, /e1 +p1 .*5 bytes.*contested.*because/);
  assert.equal(walk(got, k('esc')).a.lib.detail, null);
});

test('claims carry the 052 words and glyphs with their age; an unknown word reads noted', () => {
  const a = onClaims();
  const lines = A.view(a, 100, 24, { dark: true }, info, NOW).map(plain);
  const row = (id: RegExp) => lines.find((l) => id.test(l)) ?? '';
  assert.match(row(/Rayleigh/), /● .*validated +2 d/);
  assert.match(row(/Water boils/), /✕ .*disproved +8 w/);
  assert.match(row(/A new note/), /○ .*noted +today/);
  for (const l of lines) assert.ok(stringWidth(l) <= 100);
  assert.ok(!lines.some((l) => /untrusted/i.test(l)));
});

test('a claim opens to its text, anchors, quotes and who it was handed to', () => {
  let a = onClaims();
  a = walk(a, k('down')).a;
  a = walk(a, k('enter')).a;
  const text = plain(A.view(a, 100, 24, { dark: true }, info, NOW).join('\n'));
  assert.match(text, /disproved/);
  assert.match(text, /Water boils at 90C/);
  assert.match(text, /wikipedia-sky-1 · p2 · lost/);
  assert.match(text, /“boils at 100C”/);
  assert.match(text, /contested by c-9/);
});

test('an empty library says how to start, and a failed read says so', () => {
  let a = A.gotLibrary(walk(inConsole(), ctrlL).a, 'sources', []);
  assert.match(plain(A.view(a, 90, 20, { dark: true }, info, NOW).join('\n')), /no sources yet/);
  a = A.libraryFailed(a, 'could not read the library');
  assert.match(plain(A.view(a, 90, 20, { dark: true }, info, NOW).join('\n')), /could not read the library/);
  assert.deepEqual(walk(a, { t: 'text', s: 'r' }).action, { t: 'library', what: 'sources' });
});

test('golden screens: the Sources and Claims views, at 100x24 and 80x20', () => {
  const base = A.gotLibrary(A.gotLibrary(walk(inConsole(), ctrlL).a, 'sources', SOURCES), 'claims', CLAIMS);
  for (const [name, what, w, h] of [['library-sources-100x24', 'sources', 100, 24], ['library-claims-100x24', 'claims', 100, 24], ['library-claims-80x20', 'claims', 80, 20]] as const) {
    const r = snapshot(name, A.view({ ...base, mode: what }, w, h, { dark: true }, info, NOW));
    assert.ok(r.ok, r.message);
  }
});
