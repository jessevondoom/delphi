// The console's search pages (specs/057 SK8d): results at once, the page beside them, the keys on a page.
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import * as A from '../src/archives.ts';
import * as P from '../src/pages.ts';
import type { Ev } from '../src/input.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const k = (key: string): Ev => ({ t: 'key', k: key as never, alt: false, shift: false, ctrl: false });
const text = (s: string): Ev => ({ t: 'text', s });
const MD = [
  '> Generated page: written from the library; not validated.',
  '',
  'Tides follow the moon [c-1].',
  'The sun adds a smaller pull [s-tides-1].',
  '',
  '## Bibliography and citations',
  '| Id | Status | Title |',
  '| --- | --- | --- |',
  '| c-1 | ● validated | Tides follow the moon |',
  '| s-tides-1 | ○ noted | Tides |',
].join('\n');
const PAGE = { key: 'a'.repeat(24), query: 'tides', pinned: false, outcome: 'passed', stale: '', external: false, created: 1, markdown: MD };

const onPages = () => {
  let a = A.emptyArch(true);
  a = { ...a, mode: 'pages' };
  return a;
};
const walk = (a: A.Arch, e: Ev) => A.key(a, e, 120, 30);
const withPage = () => {
  let a = onPages();
  for (const c of 'tides') a = walk(a, text(c)).a;
  const r = walk(a, k('enter'));
  assert.deepEqual(r.action, { t: 'pages-search', q: 'tides', external: false });
  a = A.gotPages(r.a, (p) => P.gotResults(p, 'tides', { hits: [{ kind: 'claim', id: 'c-1', title: 'Tides follow the moon', status: 'validated' }] }));
  return A.gotPages(a, (p) => P.gotPage(p, 'tides', PAGE));
};

test('the page parses into banner, cited statements and the bibliography', () => {
  const o = P.parsePage(MD);
  assert.match(o.banner, /Generated page/);
  assert.deepEqual(o.body.map((s) => s.ids), [['c-1'], ['s-tides-1']]);
  assert.equal(o.table.length, 3);
});

test('results show at once and the page opens beside them with the banner and the table', () => {
  const lines = A.view(withPage(), 140, 30, { dark: true }, undefined, 0, true).map(plain);
  const all = lines.join('\n');
  assert.match(all, /1 record found/);
  assert.match(all, /Generated page/);
  assert.match(all, /Bibliography and citations/);
  assert.match(all, /● validated/);
  assert.ok(lines.every((l) => stringWidth(l) <= 140));
});

test('keys on a page: pin, dispute a cited claim, promote, open a cited source', () => {
  let a = walk(withPage(), k('tab')).a;
  assert.equal(a.pg.focus, 'page');
  assert.deepEqual(walk(a, text('s')).action, { t: 'pages-pin', key: PAGE.key, pinned: true });
  assert.deepEqual(walk(a, text('p')).action, { t: 'pages-promote', page: PAGE.key, statement: 'Tides follow the moon.' });
  a = walk(a, text('d')).a;
  assert.equal(a.pg.focus, 'note');
  assert.equal(walk(a, k('enter')).action.t, 'none', 'a dispute needs its reason');
  for (const c of 'wrong') a = walk(a, text(c)).a;
  assert.deepEqual(walk(a, k('enter')).action, { t: 'pages-dispute', claim: 'c-1', note: 'wrong' });
  let b = walk(walk(withPage(), k('tab')).a, k('down')).a;
  const o = walk(b, text('o'));
  assert.equal(o.a.mode, 'sources');
  assert.deepEqual(o.action, { t: 'library', what: 'sources', open: 'tides-1' });
});

test('the wait for the writer is a still glyph under reduce motion', () => {
  const a = A.gotPages(onPages(), (p) => ({ ...p, busy: 'page' as const }));
  const frames = [0, 400, 800, 1200].map((n) => A.view(a, 100, 20, { dark: true }, undefined, n, true).map(plain).join('\n'));
  assert.equal(new Set(frames).size, 1);
});

test('keys on a page: expand with reputable sources and refresh', () => {
  const a = walk(withPage(), k('tab')).a;
  assert.equal(a.pg.focus, 'page');
  assert.deepEqual(walk(a, text('e')).action, { t: 'pages-expand', q: PAGE.query });
  assert.deepEqual(walk(a, text('r')).action, { t: 'pages-refresh', q: PAGE.query, external: PAGE.external });
});

test('toggle between encyclopedia page and raw file search with f', () => {
  const p = withPage();
  assert.equal(p.mode, 'pages');
  const toRaw = walk(p, text('f'));
  assert.equal(toRaw.a.mode, 'search');
  assert.equal(toRaw.a.focus, 'list');
  const backToPage = walk(toRaw.a, text('f'));
  assert.equal(backToPage.a.mode, 'pages');
});
