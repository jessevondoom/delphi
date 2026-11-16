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

test('a search shows the encyclopedia page as soon as it arrives, unless the operator chose the raw list or searched again (specs/097 §4.6)', () => {
  const searched = (q: string, a: A.Arch = A.emptyArch(true)) => {
    for (const c of q) a = walk(a, text(c)).a;
    const r = walk(a, k('enter'));
    assert.deepEqual(r.action, { t: 'search', q });
    return A.gotHits(r.a, q, { results: [{ path: 'specs/a.md', collection: 'specs', trust: 'x', heading: '', preview: 'p' }] });
  };
  const a = searched('tides');
  assert.equal(a.mode, 'search', 'the raw list is on screen until the page is ready');
  const got = A.gotPages(a, (p) => P.gotPage(p, 'tides', PAGE));
  assert.equal(got.mode, 'pages', 'the page takes the view');
  assert.equal(walk(got, text('f')).a.mode, 'search', 'f shows the raw results');
  const raw = walk(got, text('f')).a;
  assert.equal(A.gotPages(raw, (p) => P.gotPage(p, 'tides', PAGE)).mode, 'search', 'a page arriving after the operator chose raw does not take the view back');
  const cached = A.gotPages(searched('tides'), (p) => P.gotPage(p, 'moon', PAGE));
  assert.equal(cached.mode, 'search', 'a late page for an older query is dropped');
});

test('a search shows a building state with a cancel, and a cancelled page is dropped if it still arrives (specs/064 §6)', () => {
  let a = A.emptyArch(true);
  for (const c of 'tides') a = walk(a, text(c)).a;
  a = A.gotHits(walk(a, k('enter')).a, 'tides', { results: [{ path: 'specs/a.md', collection: 'specs', trust: 'x', heading: '', preview: 'p' }] });
  const shown = (x: A.Arch, still: boolean) => A.view(x, 120, 30, { dark: true }, undefined, 0, still).map(plain).join('\n');
  assert.match(shown(a, true), /building a page · esc cancels/);
  const cancelled = walk(a, k('esc')).a;
  assert.equal(cancelled.searched, 'tides', 'esc cancels the page, not the search');
  assert.doesNotMatch(shown(cancelled, true), /building a page/);
  assert.equal(A.gotPages(cancelled, (p) => P.gotPage(p, 'tides', PAGE)).mode, 'search', 'the late page does not take the view');
  const again = walk(walk(cancelled, text('x')).a, k('enter')).a;
  assert.equal(again.pg.drop, null, 'a new search clears the drop');
});

// ── Following links on a page (specs/117 §7, closing E-7) ───────────────────────────────────────────────────────────────

const TOPIC_MD = [
  'Generated page. Not authoritative.',
  '',
  '# Tides',
  '',
  'Tides come twice a day [c-1]. See [the plan] and `[c-9]` is not in a sentence here.',
  '',
  '## Contents',
  '',
  '- [Spring tides](#spring-tides)',
  '',
  '## Why twice',
  '',
  ...Array.from({ length: 30 }, (_, i) => `Filler line ${i}.\n`),
  'The moon pulls the sea [s-tides-1, c-1]. A passage says so too [s-kb-0123456789abcdef].',
  '',
  '## Spring tides',
  '',
  'Higher near a full moon. See [Lighthouse lamps](page:lighthouse-lamps) and [not a slug](page:../etc).',
  '',
  '```',
  '[c-7] inside code is not a link',
  '```',
  '',
  '## Bibliography and citations',
  '| Id | Status | Title |',
  '| --- | --- | --- |',
  '| c-1 | ● validated | Tides come twice a day |',
].join('\n');
const TOPIC = { ...PAGE, markdown: TOPIC_MD };
const onTopic = () => walk(A.gotPages(withPage(), (p) => P.gotOpened(p, TOPIC)), k('down')).a;

test('the links of a page are its contents anchors, its page links and its citation markers, in reading order', () => {
  assert.deepEqual(P.navOf(TOPIC_MD), [
    { k: 'cite', ids: ['c-1'], label: '[c-1]' },
    { k: 'cite', ids: ['c-9'], label: '[c-9]' },
    { k: 'anchor', anchor: 'spring-tides', label: 'Spring tides' },
    { k: 'cite', ids: ['s-tides-1', 'c-1'], label: '[s-tides-1, c-1]' },
    { k: 'cite', ids: ['s-kb-0123456789abcdef'], label: '[s-kb-0123456789abcdef]' },
    { k: 'page', slug: 'lighthouse-lamps', label: 'Lighthouse lamps' },
  ], 'prose in brackets, a page link that is not a slug, fenced code and the bibliography hold no link');
  assert.equal(P.anchorOf('Spring & neap tides'), 'spring--neap-tides');
  assert.deepEqual(P.navOf('no links here'), []);
});

test('tab moves through the links of a page, shift+tab goes back, and past either end the focus returns to the results', () => {
  let a = onTopic();
  assert.deepEqual([a.pg.focus, a.pg.nav], ['page', null]);
  a = walk(a, k('tab')).a;
  assert.deepEqual([a.pg.focus, a.pg.nav, a.pg.note], ['page', 0, 'enter opens the claim c-1']);
  a = walk(walk(a, k('tab')).a, k('tab')).a;
  assert.deepEqual([a.pg.nav, a.pg.note], [2, 'enter goes to the section "Spring tides"']);
  a = walk(a, k('shift-tab')).a;
  assert.equal(a.pg.nav, 1);
  // Back past the first link, and forward past the last: the results, as tab always did.
  assert.deepEqual([walk(walk(a, k('shift-tab')).a, k('shift-tab')).a.pg.focus, walk(walk(a, k('shift-tab')).a, k('shift-tab')).a.pg.nav], ['results', null]);
  for (let i = 0; i < 4; i++) a = walk(a, k('tab')).a;
  assert.deepEqual([a.pg.focus, a.pg.nav], ['page', 5]);
  assert.deepEqual([walk(a, k('tab')).a.pg.focus, walk(a, k('tab')).a.pg.nav], ['results', null]);
  // The focused link is drawn reversed, and a page with no link keeps tab as the way back.
  const drawn = A.view(walk(onTopic(), k('tab')).a, 120, 30, { dark: true }, undefined, 0, true).join('\n');
  assert.match(drawn, /\x1b\[7m\[c-1\]\x1b\[27m/);
  const bare = A.gotPages(withPage(), (p) => P.gotOpened(p, { ...PAGE, markdown: '# Plain\n\nNo links.' }));
  assert.equal(walk(bare, k('tab')).a.pg.focus, 'results');
});

test('enter follows the focused link: a section, another page, a source or a claim', () => {
  const at = (n: number) => { let a = onTopic(); for (let i = 0; i <= n; i++) a = walk(a, k('tab')).a; return a; };
  // A contents anchor moves the page to its heading, which is then on screen.
  const jumped = walk(at(2), k('enter'));
  assert.equal(jumped.action.t, 'none');
  assert.ok(jumped.a.pg.at > 30, `the page moved down to the section (row ${jumped.a.pg.at})`);
  const shown = A.view({ ...jumped.a, pg: { ...jumped.a.pg, nav: null } }, 120, 30, { dark: true }, undefined, 0, true).map(plain).join('\n');
  assert.match(shown, /Spring tides\n[\s\S]*Higher near a full moon/);
  // A citation of a claim opens the claim's record in the Claims view once the list is read.
  const claim = walk(at(0), k('enter'));
  assert.deepEqual([claim.a.mode, claim.action], ['claims', { t: 'library', what: 'claims' }]);
  const listed = A.gotLibrary(claim.a, 'claims', [{ id: 'c-0', text: 'Other' }, { id: 'c-1', text: 'Tides come twice a day.', status: 'validated', anchors: [{ source_id: 'tides-1', quote: 'twice a day' }] }]);
  assert.deepEqual([listed.lib.detail?.k, listed.lib.detail?.k === 'claim' && listed.lib.detail.claim.id, listed.lib.want], ['claim', 'c-1', undefined]);
  assert.equal(A.gotLibrary(claim.a, 'claims', []).lib.note, 'that claim is not in the library');
  // A citation whose first record is a source opens that source.
  const source = walk(at(3), k('enter'));
  assert.deepEqual([source.a.mode, source.action], ['sources', { t: 'library', what: 'sources', open: 'tides-1' }]);
  // A knowledge-base passage is not a library source: nothing is opened, and the note says where it is.
  const passage = walk(at(4), k('enter'));
  assert.deepEqual([passage.a.mode, passage.action.t], ['pages', 'none']);
  assert.match(passage.a.pg.note, /passage of this workspace/);
  // A `page:` link asks for that page.
  const page = walk(at(5), k('enter'));
  assert.deepEqual(page.action, { t: 'pages-open-page', slug: 'lighthouse-lamps' });
  // With no link focused, enter does nothing; a new page starts with none focused.
  assert.equal(walk(onTopic(), k('enter')).action.t, 'none');
  assert.equal(A.gotPages(at(3), (p) => P.gotOpened(p, TOPIC)).pg.nav, null);
});
