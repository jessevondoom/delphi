// The knowledge window (specs 028, 029): typing and searching, the shelf, the reader, following links.
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import * as A from '../src/archives.ts';
import { openable, openExternal } from '../src/external.ts';
import type { Ev } from '../src/input.ts';
import { info, snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const k = (key: string, extra: object = {}): Ev => ({ t: 'key', k: key as never, alt: false, shift: false, ctrl: false, ...extra });
const type = (a: A.Arch, s: string) => [...s].reduce((acc, ch) => A.key(acc, { t: 'text', s: ch }, 100, 30).a, a);
const W = 100, H = 27;

const shelf = (a: A.Arch = A.emptyArch()) => A.gotShelf(a,
  [{ kind: 'spec', documents: 31, updated: '2026-10-25T10:00:00Z' }, { kind: 'docs', documents: 212, updated: '2026-10-24T10:00:00Z' }],
  { ok: true, documents: 243, passages: 1800, vectors: 1800 });

const DOC = '# Alpha\n\nSee [beta](beta.md#intro), specs/gamma.md and [the web](https://example.com/x).\n\n' + Array.from({ length: 60 }, (_, i) => `line ${i}`).join('\n\n') + '\n\n## Intro\n\nend\n';
const doc = (path = 'specs/alpha.md', text = DOC, over: object = {}) => ({ path, collection: 'specs', trust: 'internal_generated', stale: false, text, ...over });

test('typing fills the box, backspace and ctrl+u edit it, enter asks for a search', () => {
  let a = type(A.emptyArch(), 'council');
  assert.equal(a.q, 'council');
  assert.equal(A.key(a, k('backspace'), W, H).a.q, 'counci');
  assert.equal(A.key(a, { t: 'ctrl', c: 'u' }, W, H).a.q, '');
  const r = A.key(a, k('enter'), W, H);
  assert.deepEqual(r.action, { t: 'search', q: 'council' });
  assert.equal(A.key(A.emptyArch(), k('enter'), W, H).action.t, 'none', 'an empty box searches nothing');
  assert.equal(A.key(a, { t: 'paste', s: 'a\nb  c' }, W, H).a.q, 'councila b c');
  assert.equal(A.key(a, { t: 'text', s: '\x1b[A' }, W, H).used, false, 'control text is not typed');
  assert.equal(a.q.length < 200, true);
});

test('left and right are not used here, so the window can change view; ctrl+c is never taken', () => {
  for (const key of ['left', 'right']) assert.equal(A.key(A.emptyArch(), k(key), W, H).used, false, key);
  assert.equal(A.key(A.emptyArch(), { t: 'ctrl', c: 'c' }, W, H).used, false);
  const read = A.gotDoc(A.emptyArch(), doc(), null, W);
  assert.equal(A.key(read, { t: 'ctrl', c: 'c' }, W, H).used, false);
  assert.equal(A.key(read, k('left'), W, H).used, false);
});

test('results arrive in order, the list takes the keys, enter opens the pick, esc goes back to rest', () => {
  let a = type(shelf(), 'retry');
  const hits = [{ path: 'specs/003.md', collection: 'specs', trust: 'x', heading: 'Retry', preview: 'p', start_line: 10, end_line: 20 }, { path: 'context/b.md', collection: 'context', trust: 'x', heading: '', preview: 'q' }];
  a = A.gotHits(a, 'retry', { results: hits, took_ms: 300, mode: 'hybrid' });
  assert.deepEqual([a.searched, a.focus, a.sel, a.hits.length], ['retry', 'list', 0, 2]);
  assert.equal(A.gotHits(a, 'other', { results: [] }), a, 'a late answer for an old query is dropped');
  a = A.key(a, k('down'), W, H).a;
  assert.equal(a.sel, 1);
  assert.equal(A.key(a, k('down'), W, H).a.sel, 1, 'stops at the last');
  a = A.key(a, k('up'), W, H).a;
  const open = A.key(a, k('enter'), W, H);
  assert.deepEqual(open.action, { t: 'open', target: { k: 'pointer', path: 'specs/003.md', from: 10, to: 20 } });
  assert.equal(A.key(A.key(a, k('down'), W, H).a, k('enter'), W, H).action.t === 'open' && true, true);
  const back = A.key(a, k('esc'), W, H).a;
  assert.deepEqual([back.searched, back.hits.length, back.focus], [null, 0, 'box']);
  assert.equal(A.key(back, k('esc'), W, H).used, true === (back.q !== '') ? true : false);
  const typed = A.key(a, { t: 'text', s: 'x' }, W, H).a;
  assert.deepEqual([typed.focus, typed.q], ['box', 'retryx'], 'typing returns to the box');
});

test('the resting page counts the store by kind: a kind opens a listing of its documents, newest first', () => {
  let a = shelf();
  assert.equal(A.rows(a).length, 2);
  a = A.key(a, k('down'), W, H).a;
  assert.deepEqual(A.key(a, k('enter'), W, H).action, { t: 'browse', name: 'spec' });
  assert.deepEqual(A.key(A.key(a, k('tab'), W, H).a, k('enter'), W, H).action, { t: 'browse', name: 'docs' }, 'tab picks the next kind at rest');
  const b = A.gotBrowse(a, 'spec', [{ rel_path: 'specs/a.md', zone: 'specs', kind: 'spec', source_type: 'x', updated_at: '2026-10-25T00:00:00Z' }]);
  assert.deepEqual([b.searched, b.browse, b.hits[0]!.path, b.hits[0]!.kind], ['', 'spec', 'specs/a.md', 'spec']);
});

test('the reader: tab walks the links, enter opens a document link, an external link goes to the browser', () => {
  let a = A.gotDoc(A.emptyArch(), doc(), null, W);
  assert.equal(a.at, 0);
  const { links } = A.layout(a.trail[0]!, W);
  assert.deepEqual(links.map((l) => l.target.k), ['doc', 'doc', 'external']);
  a = A.key(a, k('tab'), W, H).a;
  assert.equal(a.trail[0]!.focus, 0);
  const open1 = A.key(a, k('enter'), W, H);
  assert.deepEqual(open1.action, { t: 'open', target: { k: 'doc', path: 'specs/beta.md', anchor: 'intro' } });
  a = A.key(A.key(a, k('tab'), W, H).a, k('tab'), W, H).a;
  assert.deepEqual(A.key(a, k('enter'), W, H).action, { t: 'browser', url: 'https://example.com/x' });
  assert.deepEqual(A.key(a, { t: 'text', s: 'y' }, W, H).action, { t: 'copy', text: 'https://example.com/x' });
  assert.equal(A.key(A.key(a, k('tab'), W, H).a, k('tab'), W, H).a.trail[0]!.focus, 2, 'stops at the last link');
  assert.equal(A.key(a, k('shift-tab'), W, H).a.trail[0]!.focus, 1);
});

test('opening a link pushes history; [ and ] and esc walk it; the last esc closes the reader', () => {
  let a = A.gotDoc(A.emptyArch(), doc(), null, W);
  a = A.gotDoc(a, doc('specs/beta.md', '# Beta\n\ntext\n'), { k: 'doc', path: 'specs/beta.md' }, W);
  assert.deepEqual([a.at, a.trail.length], [1, 2]);
  a = A.key(a, { t: 'text', s: '[' }, W, H).a;
  assert.equal(a.at, 0);
  a = A.key(a, { t: 'text', s: ']' }, W, H).a;
  assert.equal(a.at, 1);
  a = A.key(a, k('esc'), W, H).a;
  assert.equal(a.at, 0);
  a = A.gotDoc(a, doc('specs/b2.md', '# B2\n'), null, W);
  a = A.gotDoc(a, doc('specs/b3.md', '# B3\n'), null, W);
  assert.deepEqual([a.trail.map((e) => e.path), a.at], [['specs/alpha.md', 'specs/b2.md', 'specs/b3.md'], 2], 'opening from the middle drops what was forward');
  const third = A.key(A.key(a, k('esc'), W, H).a, k('esc'), W, H).a;
  assert.equal(third.at, 0);
  const closed = A.key(third, k('esc'), W, H).a;
  assert.deepEqual([closed.at, closed.trail.length], [-1, 0]);
});

test('the reader scrolls and stops at both ends, follows an anchor, and never types', () => {
  let a = A.gotDoc(A.emptyArch(), doc(), { k: 'doc', path: 'specs/alpha.md', anchor: 'intro' }, W);
  assert.ok(a.trail[0]!.scroll > 20, `opened at the Intro section, scroll ${a.trail[0]!.scroll}`);
  a = A.gotDoc(A.emptyArch(), doc(), null, W);
  assert.equal(A.key(a, k('up'), W, H).a.trail[0]!.scroll, 0);
  const down = A.key(a, k('pgdn'), W, H).a;
  assert.ok(down.trail[0]!.scroll > 10);
  const end = A.key(a, k('end'), W, H).a;
  assert.equal(A.key(end, k('down'), W, H).a.trail[0]!.scroll, end.trail[0]!.scroll, 'stops at the end');
  assert.equal(A.key(a, { t: 'text', s: 'z' }, W, H).a, a, 'a stray letter changes nothing');
  assert.equal(A.key(a, { t: 'wheel', dir: 'down' }, W, H).a.trail[0]!.scroll, 3);
  const pointer = A.gotDoc(A.emptyArch(), doc(), { k: 'pointer', path: 'specs/alpha.md', from: 60, to: 70 }, W);
  assert.ok(pointer.trail[0]!.scroll > 0, 'a line range opens partway down');
});

test('drawing: every state fits the window, shows what it should, and has no box outside the search box', () => {
  const states: [string, A.Arch][] = [
    ['at rest, loading', A.emptyArch()],
    ['at rest, with the shelf', shelf()],
    ['results', A.gotHits(type(shelf(), 'retry'), 'retry', { results: [{ path: 'specs/a.md', collection: 'specs', trust: 'x', heading: 'A > B', preview: '## B\nfirst line\nsecond line\nthird' }], took_ms: 400, mode: 'lexical' })],
    ['no results', A.gotHits(type(shelf(), 'zzz'), 'zzz', { results: [] })],
    ['the reader', A.gotDoc(A.emptyArch(), doc('specs/alpha.md', DOC, { stale: true }), null, W)],
  ];
  for (const [name, a] of states) for (const [w, h] of [[100, 27], [60, 15], [40, 12], [28, 9]] as const) {
    const lines = A.view(a, w, h, { dark: true });
    assert.equal(lines.length, h, `${name} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${name} ${w}x${h}: ${plain(l)}`);
  }
  const text = (a: A.Arch) => A.view(a, 100, 27, { dark: true }).map(plain).join('\n');
  assert.match(text(states[0]![1]), /reading the index/);
  assert.match(text(states[1]![1]), /docs 212 {3}specs 31/, 'the store counted by kind, on one line');
  assert.match(text(states[1]![1]), /243 documents · 1800 passages · hybrid/);
  assert.match(text(states[2]![1]), /1 result · 0\.40 s · 0 tokens/);
  assert.match(text(states[2]![1]), /specs\/a\.md › B/);
  assert.match(text(states[2]![1]), /first line second line third/, 'the best match is the passage\'s own words');
  assert.match(text(states[3]![1]), /nothing in the knowledge base matches/);
  assert.match(text(states[4]![1]), /specs\/alpha\.md\s+.*changed since indexed/);
  assert.match(text(states[4]![1]), /3 links · tab to focus/);
});

test('matches are marked in bold, with stems, and nothing else changes in the text', async () => {
  const chalk = (await import('chalk')).default, T = await import('../src/theme.ts');
  const saved = T.currentCapabilities(), level = chalk.level;
  T.useCapabilities({ color: 'truecolor', attributes: true });
  try {
    const out = A.emphasize('The council escalates a deadlock to the operator', 'how does the council escalate a deadlock');
    assert.equal(plain(out), 'The council escalates a deadlock to the operator');
    const bold = [...out.matchAll(/\x1b\[1m([^\x1b]+)\x1b\[22m/g)].map((m) => m[1]);
    assert.deepEqual(bold, ['council', 'escalates', 'deadlock']);
    assert.equal(A.emphasize('nothing to mark here', ''), 'nothing to mark here');
    assert.equal(plain(A.emphasize('a.b (c) [d]', 'a.b (c) [d]')), 'a.b (c) [d]', 'a query with regex characters is safe');
    assert.deepEqual(A.terms('How does the retry-ladder escalate?'), ['retry-ladder', 'escalate']);
  } finally { T.useCapabilities(saved); chalk.level = level; }
});

test('a result says when its provenance is not the usual', () => {
  assert.equal(A.caveat({ trust: 'internal_generated', status: 'active' }), '');
  assert.equal(A.caveat({ trust: 'external_untrusted', status: 'active' }), 'untrusted');
  assert.equal(A.caveat({ trust: 'federated_peer', status: 'superseded' }), 'untrusted · inactive');
  const a = A.gotHits(type(shelf(), 'zebra'), 'zebra', { results: [{ path: 'specs/web.md', collection: 'specs', kind: 'spec', trust: 'external_untrusted', status: 'active', heading: '', preview: 'zebra from the web' }], took_ms: 10, mode: 'lexical' });
  assert.match(A.view(a, 100, 27, { dark: true }).map(plain).join('\n'), /spec · untrusted/);
});

test('tab walks the kind chips, and a search in progress is run again with that kind', () => {
  const hit = (path: string, kind: string) => ({ path, collection: 'specs', kind, updated: '2026-10-25T10:00:00Z', trust: 'internal_generated', heading: '', preview: 'p' });
  let a = A.gotHits(type(shelf(), 'retry'), 'retry', { results: [hit('specs/a.md', 'spec'), hit('context/b.md', 'docs')], facets: { spec: 1, docs: 1, lesson: 2 } });
  assert.deepEqual(A.chipKinds(a), ['docs', 'spec', 'lesson'], 'the spec\'s order, only the kinds that have results');
  assert.equal(a.kind, null);
  const first = A.key(a, k('tab'), W, H);
  assert.deepEqual([first.a.kind, first.action], ['docs', { t: 'search', q: 'retry' }]);
  assert.equal(A.key(first.a, k('tab'), W, H).a.kind, 'spec');
  assert.equal(A.key(A.key(A.key(first.a, k('tab'), W, H).a, k('tab'), W, H).a, k('tab'), W, H).a.kind, null, 'wraps back to all');
  const again = A.gotHits(first.a, 'retry', { results: [hit('context/b.md', 'docs')], facets: { spec: 1, docs: 1, lesson: 2 } });
  assert.deepEqual(again.facets, { spec: 1, docs: 1, lesson: 2 }, 'the chips keep their counts when one is chosen');
  assert.equal(A.key(a, k('shift-tab'), W, H).a.kind, null, 'shift+tab switches the mode (spec 028-A1), not the chip');
});

test('ctrl+t switches between every document and trusted ones only, and asks for a reload', () => {
  const a = shelf();
  const t = A.key(a, { t: 'ctrl', c: 't' }, W, H);
  assert.deepEqual([t.a.trustedOnly, t.action, A.trustParam(t.a)], [true, { t: 'reload' }, '&trusted_only=1']);
  const back = A.key(t.a, { t: 'ctrl', c: 't' }, W, H);
  assert.deepEqual([back.a.trustedOnly, A.trustParam(back.a)], [false, '']);
  assert.match(A.view(t.a, 100, 27, { dark: true }).map(plain).join('\n'), /trusted only/);
});

test('up and down in the box recall earlier queries, newest first, and back to an empty box', () => {
  let a = A.emptyArch();
  for (const q of ['alpha', 'beta', 'beta', 'gamma']) { a = type({ ...a, q: '' }, q); a = A.key(a, k('enter'), W, H).a; }
  assert.deepEqual(a.history, ['alpha', 'beta', 'gamma'], 'a repeat is not recorded twice');
  a = { ...a, q: '', hits: [], searched: null, focus: 'box' };
  a = A.key(a, k('up'), W, H).a;
  assert.equal(a.q, 'gamma');
  a = A.key(a, k('up'), W, H).a;
  assert.equal(a.q, 'beta');
  a = A.key(A.key(a, k('up'), W, H).a, k('up'), W, H).a;
  assert.equal(a.q, 'alpha', 'stops at the oldest');
  a = A.key(a, k('down'), W, H).a;
  assert.equal(a.q, 'beta');
  a = A.key(A.key(a, k('down'), W, H).a, k('down'), W, H).a;
  assert.deepEqual([a.q, a.hist], ['', null], 'past the newest is the empty box');
  const typed = type({ ...A.emptyArch(), history: ['old'] }, 'new');
  assert.equal(A.key(typed, k('up'), W, H).a.q, 'new', 'up does not replace what is being typed');
  assert.equal(A.key({ ...A.emptyArch(), history: ['old'], hist: 0, q: 'old' }, { t: 'text', s: 'x' }, W, H).a.hist, null, 'typing ends recall');
});

test('a search asks for thirty results and says so when it gets them', () => {
  const many = Array.from({ length: A.RESULTS }, (_, i) => ({ path: `specs/${i}.md`, collection: 'specs', trust: 'x', heading: '', preview: 'p' }));
  const a = A.gotHits(type(shelf(), 'x'), 'x', { results: many, took_ms: 5, mode: 'hybrid' });
  assert.equal(a.hits.length, 30);
  assert.match(A.view(a, 100, 27, { dark: true }).map(plain).join('\n'), /top 30 results/);
  const down = Array.from({ length: 29 }, () => k('down')).reduce((acc, e) => A.key(acc, e, W, H).a, a);
  assert.equal(down.sel, 29);
  assert.match(A.view(down, 100, 27, { dark: true }).map(plain).join('\n'), /specs\/29\.md/, 'the list scrolls to the last result');
});

test('the search box is a closed rectangle, empty or full, wherever the cursor is', () => {
  for (const q of ['', 'a', 'a rather long query '.repeat(8)]) for (const focus of ['box', 'list'] as const) {
    const rows = A.view({ ...A.emptyArch(), q, focus }, 100, 27, { dark: true }).map(plain).filter((l) => /[╭╮╰╯│]/.test(l));
    assert.equal(rows.length, 3, JSON.stringify(q));
    assert.equal(new Set(rows.map((l) => stringWidth(l))).size, 1, `the three rows of the box are one width: ${rows.join('|')}`);
    assert.ok(rows[1]!.trimEnd().endsWith('│') && rows[0]!.trimEnd().endsWith('╮') && rows[2]!.trimEnd().endsWith('╯'));
  }
});

test('an external link opens only when it is a web address, with the address as an argument', () => {
  assert.ok(openable('https://example.com/a?b=1&c=2') && openable('http://x.org') && openable('mailto:a@b.co'));
  for (const bad of ['file:///etc/passwd', 'javascript:alert(1)', 'https://x.org/a b', 'https://x.org/`id`', 'ftp://x', 'https://x.org/\n', 'x.org']) assert.equal(openable(bad), false, bad);
  const calls: [string, string[]][] = [];
  assert.equal(openExternal('https://example.com/?a=1&b=2', (c, a) => calls.push([c, a]), 'darwin'), true);
  assert.deepEqual(calls, [['open', ['https://example.com/?a=1&b=2']]]);
  assert.equal(openExternal('https://example.com', (c, a) => calls.push([c, a]), 'linux'), true);
  assert.equal(calls[1]![0], 'xdg-open');
  assert.equal(openExternal('file:///x', (c, a) => calls.push([c, a]), 'darwin'), false);
  assert.equal(openExternal('https://example.com', (c, a) => calls.push([c, a]), 'plan9'), false);
  assert.equal(calls.length, 2);
});
// ── spec 028-A1 §10.1: two modes, no offline/online indicators, a clean expand page ──

const view = (a: A.Arch, w = 100, h = 27) => A.view(a, w, h, { dark: true }, info).map(plain).join('\n');

test('shift+tab switches between search and expand, both ways, and the mode is kept', () => {
  const a = shelf();
  assert.equal(a.mode, 'search', 'the window starts on search');
  const e = A.key(a, k('shift-tab', { shift: true }), W, H);
  assert.deepEqual([e.used, e.a.mode, e.action.t], [true, 'expand', 'none']);
  const kept = A.key(type(e.a, 'raft'), k('esc'), W, H); // esc clears the topic and leaves the mode
  assert.equal(kept.a.mode, 'expand');
  const back = A.key(e.a, k('shift-tab', { shift: true }), W, H);
  assert.deepEqual([back.a.mode, back.action.t], ['search', 'none'], 'the shelf is already loaded');
  const cold = A.key(A.key(A.emptyArch(), k('shift-tab'), W, H).a, k('shift-tab'), W, H);
  assert.deepEqual([cold.a.mode, cold.action], ['search', { t: 'reload' }], 'back on search with no shelf: load it');
});

test('there are no offline or online indicators in either mode, and the title names the page', () => {
  const s = view(shelf());
  assert.ok(!/offline|online|○|●/.test(s), s);
  assert.match(s, /⟦•─•⟧ search the archives/);
  assert.ok(!s.includes('knowledge\n'), 'the word knowledge no longer titles the page');
  const x = view({ ...shelf(), mode: 'expand' });
  assert.ok(!/offline|online/.test(x), x);
  assert.match(x, /search the archives/);
  assert.match(view({ ...shelf(), trustedOnly: true }), /trusted only/);
});

test('the expand page is clean: the title, the scout label and the box, and no shelf', () => {
  const x = view({ ...shelf(), mode: 'expand' });
  assert.match(x, /expand knowledge base: research a topic/);
  assert.match(x, /a topic to research…/);
  for (const gone of ['documents', 'passages', 'specs', 'docs 212', '243', 'reading the index']) assert.ok(!x.includes(gone), gone);
  assert.match(x, /shift\+tab {2}search/, 'the footer names the other mode');
  assert.match(view(shelf()), /shift\+tab {2}expand/);
  const lines = x.split('\n').filter((l) => l.trim());
  assert.ok(lines.length <= 8, `a clean page: ${lines.length} non-empty lines\n${x}`);
});

test('in expand mode typing fills the topic, enter says the research is not built yet, and no list, history or scope is used', () => {
  let a = A.key(shelf(), k('shift-tab'), W, H).a;
  a = type(a, 'raft');
  assert.equal(a.q, 'raft');
  const r = A.key(a, k('enter'), W, H);
  assert.deepEqual([r.used, r.action.t], [true, 'none']);
  assert.match(r.a.note, /not built yet/);
  assert.equal(A.key(a, k('tab'), W, H).a.kind, null, 'tab has no kinds here');
  assert.equal(A.key(a, k('down'), W, H).a.focus, 'box', 'there is no list to move into');
  assert.equal(A.key(a, k('esc'), W, H).a.q, '');
  assert.equal(A.key(A.key(a, k('esc'), W, H).a, k('esc'), W, H).used, false, 'esc with an empty topic leaves the pane to the window');
  for (const key of ['left', 'right']) assert.equal(A.key(a, k(key), W, H).used, false, key);
  assert.equal(A.key(a, { t: 'ctrl', c: 't' }, W, H).a.trustedOnly, false, 'the trust filter belongs to search');
});

test('golden screens: the search page and the expand page', () => {
  const a = shelf();
  for (const [name, arch] of [['archives-search-100x34', a], ['archives-expand-100x34', { ...a, mode: 'expand' as const }], ['archives-expand-typed-80x24', type({ ...a, mode: 'expand' as const }, 'byzantine fault tolerance')]] as [string, A.Arch][]) {
    const w = name.includes('80x24') ? 80 : 100, h = name.includes('80x24') ? 24 : 34;
    const r = snapshot(name, A.view(arch, w, h, { dark: true }, info));
    assert.ok(r.ok, r.message);
  }
});

// ── spec 043 §3 to §5: kinds, the best match and the result list ──

const NOW = Date.parse('2026-10-28T10:00:00Z');
const day = (n: number) => new Date(NOW - n * 86_400_000).toISOString();
const found = () => A.gotHits(type(shelf(), 'byzantine fault tolerance'), 'byzantine fault tolerance', {
  took_ms: 40, mode: 'hybrid', facets: { research: 5, docs: 4, spec: 3, lesson: 1, mission: 1 },
  results: [
    { path: 'knowledge/web/en.wikipedia.org/byzantine-fault-tolerance.md', collection: 'knowledge', kind: 'research', updated: day(2), trust: 'trusted_source', status: 'active', heading: 'Byzantine fault tolerance > Overview', start_line: 12, end_line: 31, preview: '## Overview\nA Byzantine fault is a condition in which components fail in arbitrary ways, including sending conflicting information to different parts of the system. A system is Byzantine fault tolerant when it keeps working correctly despite such failures, usually by reaching agreement among the healthy nodes. Colour themes are unrelated.' },
    { path: 'knowledge/web/arxiv.org/castro-liskov-pbft.md', collection: 'knowledge', kind: 'research', updated: day(2), trust: 'trusted_source', status: 'active', heading: 'Abstract', preview: 'a state machine replication algorithm that tolerates Byzantine faults and works in asynchronous systems' },
    { path: 'context/lessons/council-deadlock.md', collection: 'context', kind: 'lesson', updated: day(5), trust: 'internal_generated', status: 'active', heading: 'Model-family tiebreakers', preview: 'two seats of one model family are one vote: they tend to share the same fault' },
    { path: 'context/reference/council.md', collection: 'context', kind: 'docs', updated: day(9), trust: 'internal_generated', status: 'active', heading: 'Quorum and votes', preview: 'a quorum needs three agreeing votes; two seats of one model family count as one vote' },
    { path: 'specs/003-collaboration-and-council.md', collection: 'specs', kind: 'spec', updated: day(15), trust: 'user_ratified', status: 'active', heading: '4 Quorum', preview: 'the council decides by quorum; a seat that cannot be reached counts as a fault, not a vote' },
  ],
});
const shown = (a: A.Arch, w = 100, h = 34) => A.view(a, w, h, { dark: true }, info, NOW).map(plain).join('\n');

test('the best-match summary is the passage\'s own words: the sentences with the most query words, in order, at most 300 characters', () => {
  const p = '## 9.2\nWhen two rounds end without agreement, the council escalates. The seats keep their stances. The card pauses autonomous work. Colour themes are a different matter entirely.';
  const s = A.bestSummary(p, 'council escalates deadlock');
  assert.equal(s, 'When two rounds end without agreement, the council escalates. The seats keep their stances. The card pauses autonomous work.');
  assert.ok(!s.includes('Colour'), 'three sentences at most');
  assert.equal(A.bestSummary('## only a heading', 'x'), '', 'a heading is not a summary');
  const long = A.bestSummary(`${'word '.repeat(120)}end.`, 'word');
  assert.ok(long.length <= 300 && long.endsWith('…'), `${long.length}`);
  const pick = A.bestSummary('Alpha is first. Beta is about zebra crossings. Gamma follows. Delta is last. Zebra herds travel.', 'zebra');
  assert.equal(pick, 'Alpha is first. Beta is about zebra crossings. Zebra herds travel.', 'the best three (ties go to the earlier sentence), back in document order');
});

test('ago says how long ago in days, weeks and months', () => {
  assert.deepEqual([A.ago(day(0), NOW), A.ago(day(3), NOW), A.ago(day(15), NOW), A.ago(day(100), NOW), A.ago('', NOW)], ['today', '3 d', '2 w', '3 mo', '']);
});

test('the results page: a count with its cost, the kind chips, a best-match card, then a uniform list', () => {
  const t = shown(found());
  assert.match(t, /5 results · 0\.04 s · 0 tokens/);
  assert.match(t, /show {2}● all 14 {3}○ research 5 {3}○ docs 4 {3}○ specs 3 {3}○ lessons 1 {3}○ missions 1/);
  assert.match(t, /BEST MATCH\s+research · listed · edited 2 d/);
  assert.match(t, /knowledge\/web\/en\.wikipedia\.org\/byzantine-fault-tolerance\.md › Overview/);
  assert.match(t, /A Byzantine fault is a condition/);
  assert.ok(!t.includes('Colour themes'), 'the card holds the best sentences only');
  assert.match(t, /lines 12–31/);
  assert.match(t, /enter read/);
  assert.match(t, / 2 {2}knowledge\/web\/arxiv\.org\/castro-liskov-pbft.md › Abstract\s+research\s+listed\s+2 d/);
  assert.match(t, / 3 {2}context\/lessons\/council-deadlock\.md › Model-family tiebreakers\s+lesson\s+trusted\s+5 d/);
  assert.match(t, / 4 {2}context\/reference\/council\.md › Quorum and votes\s+docs\s+trusted\s+9 d/);
  assert.match(t, / 5 {2}specs\/003-collaboration-and-council\.md › 4 Quorum\s+spec\s+trusted\s+2 w/);
  assert.ok(!/ 1 {2}knowledge/.test(t), 'the best match is not repeated in the list');
  assert.match(t, /shift\+tab {2}expand/);
});

test('the card is selected first, the list follows, and the kind is always printed', () => {
  let a = found();
  assert.deepEqual([a.focus, a.sel], ['list', 0]);
  assert.match(shown(a), /❯ knowledge\/web\/en\.wikipedia\.org/);
  a = A.key(a, k('down'), W, H).a;
  assert.match(shown(a), /› +2 +knowledge\/web\/arxiv/);
  const open = A.key(a, k('enter'), W, H).action;
  assert.deepEqual(open, { t: 'open', target: { k: 'doc', path: 'knowledge/web/arxiv.org/castro-liskov-pbft.md' } });
  assert.match(shown({ ...found(), kind: 'lesson' }), /○ research 5.*● lessons 1/, 'the active chip is filled, the others keep their counts');
});

test('under 90 columns the edited column goes, under 80 the trust column goes and an untrusted row is marked', () => {
  const a = A.gotHits(type(shelf(), 'zebra'), 'zebra', { results: [
    { path: 'specs/a.md', kind: 'spec', trust: 'internal_generated', updated: day(2), heading: '', preview: 'zebra one' },
    { path: 'specs/b.md', kind: 'spec', trust: 'internal_generated', updated: day(3), heading: '', preview: 'zebra two' },
    { path: 'specs/web.md', kind: 'spec', trust: 'external_untrusted', updated: day(4), heading: '', preview: 'zebra from the web' }], facets: { spec: 3 } });
  const row = (w: number, p: string) => shown(a, w, 30).split('\n').find((l) => l.includes(p))!;
  assert.match(row(100, 'specs/b.md'), /spec\s+trusted\s+3 d\s*$/);
  assert.match(row(85, 'specs/b.md'), /spec\s+trusted\s*$/);
  assert.ok(!/trusted/.test(row(70, 'specs/b.md')) && /spec/.test(row(70, 'specs/b.md')));
  assert.match(row(70, 'specs/web.md'), /spec +!\s*$/, 'an untrusted row is still marked');
});

test('golden screens: results with the best match, at 100x34 and 80x24', () => {
  for (const [name, w, h] of [['archives-results-100x34', 100, 34], ['archives-results-80x24', 80, 24]] as [string, number, number][]) {
    const r = snapshot(name, A.view(found(), w, h, { dark: true }, info, NOW));
    assert.ok(r.ok, r.message);
  }
});
