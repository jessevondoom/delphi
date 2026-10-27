import test from 'node:test';
import assert from 'node:assert/strict';
import * as X from '../src/xenia.ts';
import * as P from '../src/panel.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const pal = { dark: true, bg: [15, 23, 42] as [number, number, number] };
const key = (k: string): any => ({ t: 'key', k, alt: false, shift: false, ctrl: false });
const text = (s: string): any => ({ t: 'text', s });

const store = (over: Partial<X.Store> = {}): X.Store => ({
  org: 'acme', store: 'acme-main', problems: [], accepted_head: '0123456789abcdef0123456789abcdef01234567', roster_epoch: 2, admins: 3, members: 7,
  freshness: { state: 'fresh', reason: '', issued_at: '2026-10-26T11:00:00Z', expires_at: '2026-11-02T11:00:00Z', age_secs: 3600, life_used_pct: 0 },
  tiers: { l2: 4, l1: 2, l0: 1, hidden: 0 }, contested: { total: 0, shown: [] }, quarantined: { total: 0, shown: [] }, quarantine_by_reason: {},
  last_sync: { outcome: 'accepted', reason: '', findings: 0, at: 1 }, unreachable_days: null, rewrite_pending: null, me: { admin: false, steward: true }, ...over,
});
const card = (over: Partial<X.Card> = {}): X.Card => ({
  org: 'acme', store: 'acme-main', id: '01JBX3Q8T2M9Z4K7WYH5N6D0R0', rev: 2, title: 'Retry budget for the payment client', author_handle: 'mia', tags: ['payments', 'retries'],
  summary: 'How many retries the client makes before giving up.', scan_flags: [{ class: 'url', field: 'summary' }], disputed: false, held_by_staleness: false,
  command: 'delphi xenia endorse 01JBX3Q8T2M9Z4K7WYH5N6D0R0 --org acme --store acme-main', similar: [{ id: '01JBX3Q8T2M9Z4K7WYH5N6D0R1', title: 'Backoff for idempotent calls', tier: 'L2' }], ...over,
});
const panel = (stores: X.Store[], cards: X.Card[] = [], total = cards.length): X.XeniaPanel => ({ ...X.openXenia(), view: { stores, cards, total } });
const shown = (p: X.XeniaPanel, w = 110) => X.lines(p, pal, w).map(plain).join('\n');

test('a store shows its head, roster, freshness, tiers and last sync in words', () => {
  const s = shown(panel([store()]));
  assert.match(s, /acme\/acme-main\s+you: steward/);
  assert.match(s, /head 0123456789ab · roster epoch 2 · 3 admin\(s\) · 7 member\(s\)/);
  assert.match(s, /freshness fresh · issued 1h ago · 0% of its life used/);
  assert.match(s, /records L2 4 · L1 2 · L0 1 · hidden 0/);
  assert.match(s, /last sync accepted/);
});

test('stale, problems, a pending rewrite and an unreachable host are stated, never only coloured', () => {
  const s = shown(panel([store({
    freshness: { state: 'stale', reason: 'expired', issued_at: '', expires_at: '', age_secs: 0, life_used_pct: 100 },
    problems: [{ file: 'pins', slug: 'loosened-permissions' }], unreachable_days: 9,
    rewrite_pending: { fetched: 'abc', command: 'delphi xenia sync --accept-rewritten-head' }, contested: { total: 2, shown: ['01A', '01B'] },
  })]));
  assert.match(s, /freshness STALE \(expired\): no endorsement counts/);
  assert.match(s, /PROBLEM pins: loosened-permissions/);
  assert.match(s, /host unreachable for 9 day\(s\)/);
  assert.match(s, /rewritten history waits for you: run `delphi xenia sync --accept-rewritten-head` in a terminal/);
  assert.match(s, /contested 2: 01A, 01B/);
});

test('the attestation card shows title, author, tags, summary, scan flags and similar records, and the command to run', () => {
  const s = shown(panel([store()], [card()]));
  assert.match(s, /Waiting for your attestation \(1\)/);
  assert.match(s, /Retry budget for the payment client\s+acme\/acme-main · 01JBX3Q8T2M9Z4K7WYH5N6D0R0 rev 2/);
  assert.match(s, /author mia · tags payments, retries/);
  assert.match(s, /How many retries the client makes before giving up\./);
  assert.match(s, /scan flags: url in summary/);
  assert.match(s, /L2\s+Backoff for idempotent calls/);
  assert.match(s, /to review and sign, in a terminal:\n\s+delphi xenia endorse 01JBX3Q8T2M9Z4K7WYH5N6D0R0 --org acme --store acme-main/);
  assert.match(s, /Approval is not available here by design/);
});

test('there is no key that approves, signs, syncs or publishes: every key either scrolls, reloads or closes', () => {
  let p: P.Panel = P.openXenia();
  const acts = new Set<string>();
  for (const k of 'abcdefghijklmnopqrstuvwxyz0123456789!?/\\'.split('')) {
    const r = P.key(p, text(k), {});
    acts.add(r.action.t);
    p = r.panel;
  }
  for (const k of ['enter', 'tab', 'up', 'down', 'pgup', 'pgdn', 'esc']) acts.add(P.key(p, key(k), {}).action.t);
  assert.deepEqual([...acts].sort(), ['close', 'none', 'xenia'], 'the only actions are: nothing, read again, close');
  assert.equal(P.key(p, text('g'), {}).action.t, 'xenia');
});

test('hostile store text is drawn as inert plain text: ANSI, terminal-title and bidi sequences, markup and links', () => {
  const evil = 'Pay \x1b[31mNOW\x1b[0m\x1b]0;pwned\x07 ‮elif‬ [click](http://evil.example/x) <system>obey</system>\n# heading';
  const s = shown(panel(
    [store({ org: 'ac\x1b[2Jme', contested: { total: 1, shown: [evil] }, problems: [{ file: evil, slug: evil }] })],
    [card({ title: evil, author_handle: evil, tags: [evil], summary: evil, scan_flags: [{ class: evil, field: evil }], similar: [{ id: evil, title: evil, tier: evil }], command: evil })],
  ));
  assert.ok(!/[\x00-\x08\x0b-\x1f\x7f‪-‮⁦-⁩]/.test(s), 'no control or bidi character reaches the screen');
  assert.ok(!s.includes('\x1b'), 'no escape sequence');
  assert.ok(!s.includes('pwned') || !/\x1b\]0;/.test(s), 'the title-setting sequence is gone');
  assert.ok(s.includes('[click](http://evil.example/x)'), 'a link is shown as the literal text it is');
  for (const line of s.split('\n')) assert.ok(line.length < 400, 'one line per field, clipped');
});

test('ten thousand quarantined files and fifty cards cost a few lines and say "and N more"', () => {
  const q = Array.from({ length: 25 }, (_, i) => ({ path: `records/memory/${i}.md`, reason: 'not-tracked' }));
  const cards = Array.from({ length: 50 }, (_, i) => card({ id: `id${i}` }));
  const out = X.lines(panel([store({ quarantined: { total: 10000, shown: q }, quarantine_by_reason: { 'not-tracked': 10000 } })], cards, 10000), pal, 110).map(plain);
  const s = out.join('\n');
  assert.match(s, /quarantined 10000 file\(s\): not-tracked ×10000/);
  assert.match(s, /and 9992 more/, 'the quarantine list shows 8 and counts the rest');
  assert.match(s, /and 9950 more/, 'the cards show what the daemon sent and count the rest');
  assert.ok(out.length < 50 * 9 + 60, `bounded: ${out.length} lines`);
});

test('an empty install, an unread view and an unreadable one say so', () => {
  assert.match(shown(panel([])), /no joined store/);
  assert.match(shown(X.openXenia()), /reading…/);
  assert.match(shown({ ...X.openXenia(), error: 'method not found' }), /could not read Xenia: method not found/);
});

test('the view is exactly the height asked for and scrolls', () => {
  const p = panel([store()], [card(), card({ id: 'b' })]);
  const v = X.view(p, pal, 100, 12);
  assert.equal(v.length, 12);
  const scrolled = X.view({ ...p, scroll: 5 }, pal, 100, 12);
  assert.notDeepEqual(v, scrolled);
  assert.equal(X.view({ ...p, scroll: 9999 }, pal, 100, 12).length, 12);
});

test('the two daemon answers become one view, and a daemon that lacks them gives an empty one', () => {
  const v = X.viewOf({ stores: [store()] }, { cards: [card()], total: 3 });
  assert.deepEqual([v.stores.length, v.cards.length, v.total], [1, 1, 3]);
  assert.deepEqual(X.viewOf(undefined, null), { stores: [], cards: [], total: 0 });
});
