// Links on the chat and the change feed (spec 029 slices 3 and 4): found by their text in finished rows, focus drawn
// reversed, a document opened from a link reading in place, and what the console records (sources, pins).
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import * as A from '../src/archives.ts';
import * as C from '../src/composer.ts';
import { newInbox } from '../src/inbox.ts';
import { paintFocus, reverseSpan, scanLinks } from '../src/links.ts';
import { feedLinks, observeFrame, type OModel } from '../src/observe-screen.ts';
import { noteSources, passageOf } from '../src/research.ts';
import { backShowing, chatLinks, frame, type Model } from '../src/screen.ts';
import { epicWorld, info as epicInfo, NOW } from './epic-fixture.ts';
import { info, plain, stateWorld } from './helpers.ts';

const REV = /\x1b\[7m(.*?)\x1b\[27m/g;
const reversed = (lines: string[]) => lines.flatMap((l) => [...l.matchAll(REV)].map((m) => plain(m[1]!)));

test('links are found by their text in styled rows, at the right column, and not inside a longer word or path', () => {
  const rows = ['  \x1b[2mread\x1b[0m \x1b[36mspecs/004-x.md#L10-L20\x1b[0m then https://example.com/a.', 'user@host/docs/a.md and README.md', '一二 context/a.md'];
  const links = scanLinks(rows);
  assert.deepEqual(links.map((l) => l.label), ['specs/004-x.md#L10-L20', 'https://example.com/a', 'context/a.md']);
  assert.equal(links[0]!.spans[0]!.col, 7);
  assert.equal(links[2]!.spans[0]!.col, stringWidth('一二 '), 'wide characters count two columns');
  assert.deepEqual(links.map((l) => l.id), [0, 1, 2]);
});

test('focus is drawn by reversing the link and nothing around it, keeping the styling already there', () => {
  const row = '  \x1b[2mread\x1b[0m \x1b[36mspecs/a.md\x1b[0m now';
  const [l] = scanLinks([row]);
  const out = reverseSpan(row, l!.spans[0]!.col, l!.spans[0]!.width);
  assert.equal(plain(out), plain(row), 'same text');
  assert.deepEqual(reversed([out]), ['specs/a.md']);
  assert.ok(out.includes('\x1b[36m'), 'the colour is kept');
  assert.deepEqual(paintFocus([row], [l!], null), [row], 'no focus, no change');
  assert.deepEqual(reversed(paintFocus([row], [l!], l!.id)), ['specs/a.md']);
});

test('a pin records the pointer, a hash of the passage and the passage; a changed passage hashes differently', () => {
  const text = Array.from({ length: 10 }, (_, i) => `line ${i + 1}`).join('\n');
  const p = passageOf(text, { k: 'pointer', path: 'specs/a.md', from: 3, to: 4 });
  assert.match(p.text, /^specs\/a\.md#L3-L4\nsha256:[0-9a-f]{12}\n\n> line 3\n> line 4$/);
  assert.equal(p.from, 'read specs/a.md#L3-L4');
  const changed = passageOf(text.replace('line 4', 'line four'), { k: 'pointer', path: 'specs/a.md', from: 3, to: 4 });
  assert.notEqual(changed.text.split('\n')[1], p.text.split('\n')[1]);
  const whole = passageOf(text, { k: 'doc', path: 'specs/a.md' });
  assert.match(whole.text, /^specs\/a\.md\nsha256:[0-9a-f]{12}$/, 'a whole document is pointed at, not quoted');
});

test('opening from the chat is a source the console lists as read; the same file cited by an agent stays one entry', () => {
  const s = noteSources({}, ['specs/a.md'], 'cited', 'architect', 1);
  const t = noteSources(s, ['specs/a.md'], 'read', 'operator', 2);
  assert.equal(Object.keys(t).length, 1);
  assert.equal(t['specs/a.md']!.how, 'read');
  assert.deepEqual(t['specs/a.md']!.roles, ['architect', 'operator']);
});

const doc = { path: 'specs/a.md', collection: 'specs', trust: 'internal_generated', stale: false, text: Array.from({ length: 30 }, (_, i) => `line ${i + 1}`).join('\n') };
const k = (key: string) => ({ t: 'key', k: key, alt: false, shift: false, ctrl: false }) as never;

test('p in the reader pins the passage that was cited, but only in the console; the narrator is read-only', () => {
  const target = { k: 'pointer', path: 'specs/a.md', from: 5, to: 6 } as const;
  const console_ = A.gotDoc(A.emptyArch(true), doc, target, 100);
  const r = A.key(console_, { t: 'text', s: 'p' }, 100, 30);
  assert.deepEqual(r.action, { t: 'pin', target });
  assert.match(plain(A.view(console_, 100, 30, { dark: true }).join('\n')), /p pin/);
  const narrator = A.gotDoc(A.emptyArch(), doc, target, 100);
  assert.deepEqual(A.key(narrator, { t: 'text', s: 'p' }, 100, 30).action, { t: 'none' });
  assert.doesNotMatch(plain(A.view(narrator, 100, 30, { dark: true }).join('\n')), /p pin/);
  const whole = A.gotDoc(A.emptyArch(true), doc, { k: 'doc', path: 'specs/a.md' }, 100);
  assert.deepEqual(A.key(whole, { t: 'text', s: 'p' }, 100, 30).action, { t: 'pin', target: { k: 'doc', path: 'specs/a.md' } });
});

// ── the chat ────────────────────────────────────────────────────────────────

const chat: Model = {
  info, pal: { dark: true, bg: [15, 23, 42] }, state: stateWorld(), now: 5000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'default', panel: null, help: false, still: true,
};
const W = 100, H = 40;

test('chat: the pointers an agent wrote are links; link mode says where the focused one goes and draws it reversed', () => {
  const { links } = chatLinks(chat, W);
  const cited = links.find((l) => l.label === 'specs/002-adapter-and-routing.md#4');
  assert.ok(cited, 'the reply cites the ladder');
  assert.deepEqual(cited!.target, { k: 'doc', path: 'specs/002-adapter-and-routing.md', anchor: '4' });
  const off = frame(chat, W, H).map(plain);
  assert.ok(!reversed(frame(chat, W, H)).length, 'nothing is highlighted outside link mode');
  const on = frame({ ...chat, links: { focus: cited!.id, note: '' } }, W, H);
  assert.deepEqual(reversed(on), ['specs/002-adapter-and-routing.md#4']);
  const text = on.map(plain);
  assert.ok(text.some((l) => l.startsWith('link specs/002-adapter-and-routing.md § 4') && l.includes('enter reads it here') && l.includes('esc leaves')));
  assert.ok(text.some((l) => l.startsWith('link mode:')), 'the prompt row names the keys');
  assert.equal(text.length, off.length, 'the screen does not change height');
});

test('chat: link mode with nothing focused says how many links there are; the reader replaces the transcript and closing it returns', () => {
  const none = frame({ ...chat, links: { focus: null, note: '' } }, W, H).map(plain);
  assert.ok(none.some((l) => /\d+ links? · tab chooses/.test(l)));
  const reader = A.gotDoc(A.emptyArch(true), { ...doc, path: 'specs/002-adapter-and-routing.md' }, { k: 'doc', path: doc.path }, W);
  const open = frame({ ...chat, reader }, W, H).map(plain);
  assert.ok(open.some((l) => l.includes('specs/002-adapter-and-routing.md')) && open.some((l) => l.includes('line 1')));
  assert.ok(!open.some((l) => l.includes('for the ladder')), 'the transcript is hidden while a document is open');
  const closed = frame({ ...chat, reader: A.key(reader, k('esc'), W, 30).a }, W, H).map(plain);
  assert.ok(closed.some((l) => l.includes('for the ladder')), 'esc returns to the chat');
});

test('chat: moving focus scrolls just enough to show the link', () => {
  assert.equal(backShowing(100, 50, 20, 0), 30, 'a row above the window scrolls back until it is the first row shown');
  assert.equal(backShowing(100, 90, 20, 30), 9, 'a row below the window comes into view');
  assert.equal(backShowing(100, 85, 20, 10), 10, 'a row already in view changes nothing');
  assert.equal(backShowing(100, 99, 20, 0), 0);
});

// ── the narrator's change feed ──────────────────────────────────────────────

const T0 = Date.UTC(2026, 9, 25, 10, 0);
const feed = (over: Partial<OModel> = {}): OModel => ({
  info: epicInfo, pal: { dark: true }, st: { ...epicWorld(), sessions: [{ id: 's_0001', reason: 'new', started_ms: T0 - 60_000, turns: 1 }], changes: [
    { session: 's_0001', at_ms: T0, role: 'builder', task: 't_1', summary: 'added a timeout, see specs/003-retry.md#L10-L20', files: ['crates/queue/src/claim.rs'] },
    { session: 's_0001', at_ms: T0 + 60_000, role: 'architect', task: 't_2', summary: 'wrote the spec', files: ['specs/032-change-log.md'] },
  ] }, view: 'changes', pick: 0, scroll: 0, selNode: 0, arch: A.emptyArch(), live: {}, connected: true, still: true, now: NOW, ...over,
} as OModel);

test('feed: documents named in recorded changes are links (files that are not documents are not)', () => {
  const links = feedLinks(feed(), 100);
  assert.deepEqual(links.map((l) => l.label), ['specs/032-change-log.md', 'specs/003-retry.md#L10-L20'], 'newest first, as the feed is');
  const text = observeFrame(feed(), 100, 30).map(plain);
  assert.ok(text.some((l) => l.includes('2 links · tab to follow')));
});

test('feed: focus is drawn reversed with its target in the status row; the reader takes the body and esc returns to the feed', () => {
  const [first] = feedLinks(feed(), 100);
  const lit = observeFrame(feed({ feed: { focus: first!.id, note: '' } }), 100, 30);
  assert.deepEqual(reversed(lit), ['specs/032-change-log.md']);
  assert.ok(lit.map(plain).some((l) => l.includes('specs/032-change-log.md  (enter reads it here)')));
  const rdr = A.gotDoc(A.emptyArch(), { ...doc, path: 'specs/032-change-log.md' }, { k: 'doc', path: 'specs/032-change-log.md' }, 100);
  const open = observeFrame(feed({ reader: rdr }), 100, 30).map(plain);
  assert.ok(open.some((l) => l.includes('line 1')) && !open.some((l) => l.includes('added a timeout')), 'the document replaces the feed');
  assert.ok(open.some((l) => l.includes('delphi v')), 'the header and footer stay');
  const back = observeFrame(feed({ reader: A.key(rdr, k('esc'), 100, 23).a }), 100, 30).map(plain);
  assert.ok(back.some((l) => l.includes('added a timeout')));
  assert.ok(!observeFrame(feed({ reader: rdr }), 100, 30).map(plain).join('').includes('p pin'), 'the narrator does not offer pinning');
});
