// The decisions pane's list (specs/044 §5): the list from the daemon, tier colours and tags, the kind filter, tick
// boxes only where the daemon allows, and the golden screens at 100x34 and 80x24.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { chips, isTicked, key, newPane, parse, reconcile, ticked, view, visible, type Decision, type Pane } from '../src/decisions-pane.ts';
import { frame, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { fold, initial } from '../src/fold.ts';
import { cards, key as inboxKey, newInbox, view as inboxView } from '../src/inbox.ts';
import type { Ev } from '../src/input.ts';
import { info, snapshot, stateWorld } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const k = (name: string): Ev => ({ t: 'key', k: name as any, alt: false, shift: false, ctrl: false });
const ch = (s: string): Ev => ({ t: 'text', s });
const NOW = Date.parse('2026-10-28T10:00:00Z');

const raw = (id: string, kind: string, tier: string, title: string, summary: string, rec: string | null, tickable: boolean, extra: object = {}) => ({
  id, source: 'raised', source_id: id, label: id.toUpperCase(), kind, tier, title, requester: 'builder', task: 't_1', raised_at: '2026-10-28T09:51:00Z', payload_blake3: 'h-' + id,
  brief: { summary, changes: [], recommendation: rec ? { choice_id: 'ratified', why: rec } : null, state: 'authored' },
  choices: [{ id: 'ratified', label: 'Ratify', effect: 'record', reversible: true }, { id: 'rejected', label: 'Reject', effect: 'record', reversible: true }],
  tickable, tickable_reason: '', ...extra,
});
const five = () => parse({
  decisions: [
    raw('dec-081', 'action', 'red', 'Run a migration on the host staging.db, outside the sandbox', 'builder wants to run scripts/migrate_0.2.7.sql on /var/data/staging.db. It rewrites about 420 KB of host data that git cannot roll back, and it leaves the sandbox.', null, false),
    raw('dec-083', 'spec', 'red', 'Approve spec 042: chunked bundle streaming over IPC', 'Streams bundle manifests over IPC in 64 KB chunks with backpressure, so large repositories stop spiking memory. Approving it lets builder start t_016.', 'architect review done, no new authority', false),
    raw('dec-082', 'build', 'amber', 'Raise the rate-limit burst from 20 to 50 per second', 'Provisional change from optimistic build t_014 (rate.rs, +12 -4) to stop telemetry drops in benchmark loops. 28 tests and the load suite pass.', 'routine risk, second seat (scout) concurred', true),
    raw('dec-085', 'build', 'amber', 'Reuse stored vectors when a document is unchanged', 'Provisional change from optimistic build t_015 (index.rs, +41 -9). Re-indexing 1,200 documents drops from 40 s to 6 s, and a changed file is still re-embedded.', 'routine risk, second seat (red-team) concurred', true),
    raw('dec-084', 'question', 'open', 'OD-2: which search API powers online mode?', 'Online search needs one provider first. Brave Search has a plain JSON API and a free tier; self-hosted SearXNG sends nothing to a vendor but needs setup.', 'the client is provider-neutral, so switching later is cheap', true),
  ],
});

test('parse keeps what the daemon sent, drops what is malformed, and cleans terminal escapes from text', () => {
  const r = parse({ decisions: [raw('a', 'build', 'amber', 'ok\x1b[31m red \x07title', 's', null, false), { id: 'broken' }, null, raw('b', 'build', 'weird', 't', 's', null, false)] });
  assert.equal(r.length, 2);
  assert.equal(r[0]!.title, 'ok red title');
  assert.equal(r[1]!.tier, 'open', 'an unknown tier is the plain one');
  assert.deepEqual(parse(null), []);
  assert.deepEqual(parse({ decisions: 'x' }), []);
});

test('the chips list the kinds present, in the order of the spec, with their counts', () => {
  assert.deepEqual(chips(five()), [{ kind: 'action', n: 1 }, { kind: 'spec', n: 1 }, { kind: 'build', n: 2 }, { kind: 'question', n: 1 }]);
  assert.deepEqual(chips([]), []);
});

test('space ticks only what the daemon says is tickable; a red decision never gets a tick', () => {
  const items = five();
  let p: Pane = newPane();
  assert.deepEqual(key(p, items, ch(' ')).pane.ticks, [], 'the first entry is a Red action');
  p = key(p, items, k('down')).pane;
  assert.deepEqual(key(p, items, ch(' ')).pane.ticks, [], 'a Red spec has a recommendation but is not tickable');
  p = key(key(p, items, k('down')).pane, items, ch(' ')).pane;
  assert.equal(ticked(p, items).map((d) => d.label).join(), 'DEC-082');
  p = key(p, items, ch(' ')).pane;
  assert.deepEqual(p.ticks, [], 'space again clears it');
});

test('a ticks every tickable decision in the filter, never more, and a second press clears them', () => {
  const items = five();
  let p = key(newPane(), items, ch('a')).pane;
  assert.deepEqual(ticked(p, items).map((d) => d.label), ['DEC-082', 'DEC-085', 'DEC-084']);
  p = key(p, items, ch('a')).pane;
  assert.deepEqual(p.ticks, []);
  // Filtered to the builds, `a` reaches only the builds.
  p = { ...newPane(), filter: 'build' };
  p = key(p, items, ch('a')).pane;
  assert.deepEqual(ticked(p, items).map((d) => d.label), ['DEC-082', 'DEC-085']);
  // Filtered to the actions: nothing is tickable, so nothing happens.
  assert.deepEqual(key({ ...newPane(), filter: 'action' }, items, ch('a')).pane.ticks, []);
});

test('f walks the filters and wraps to all; esc clears a filter before it leaves the pane', () => {
  const items = five();
  let p = newPane();
  const seen: (string | null)[] = [];
  for (let i = 0; i < 5; i++) { p = key(p, items, ch('f')).pane; seen.push(p.filter); }
  assert.deepEqual(seen, ['action', 'spec', 'build', 'question', null]);
  p = { ...newPane(), filter: 'build', sel: 1 };
  const r = key(p, items, k('esc'));
  assert.equal(r.pane.filter, null);
  assert.equal(r.action.t, 'none');
  assert.equal(key(r.pane, items, k('esc')).action.t, 'chat');
  assert.equal(key(newPane(), items, k('left')).action.t, 'chat');
});

test('the filter shows one kind, and the selection stays inside it', () => {
  const items = five();
  assert.deepEqual(visible(items, 'build').map((d) => d.label), ['DEC-082', 'DEC-085']);
  let p: Pane = { ...newPane(), filter: 'build' };
  p = key(key(key(p, items, k('down')).pane, items, k('down')).pane, items, k('down')).pane;
  assert.equal(p.sel, 1);
});

test('a tick belongs to the payload it was given for: a changed payload, a gone decision or a lost tick drops it', () => {
  const items = five();
  const p = key(newPane(), items, ch('a')).pane;
  assert.equal(p.ticks.length, 3);
  const changed = items.map((d) => (d.label === 'DEC-082' ? { ...d, payload_blake3: 'new' } : d));
  assert.equal(isTicked(p, changed.find((d) => d.label === 'DEC-082')!), false);
  assert.deepEqual(reconcile(p, changed).ticks.length, 2);
  const lost = items.map((d) => (d.label === 'DEC-085' ? { ...d, tickable: false } : d));
  assert.equal(reconcile(p, lost).ticks.length, 2);
  assert.equal(reconcile(p, items.filter((d) => d.label !== 'DEC-084')).ticks.length, 2);
  assert.equal(reconcile({ ...p, filter: 'question' }, items.filter((d) => d.kind !== 'question')).filter, null, 'a filter nothing matches is dropped');
});

test('the list prints the tier and kind on every entry, a tick box only where tickable, and says when there is no recommendation', () => {
  const lines = view(five(), newPane(), { pal: { dark: true }, now: NOW, w: 100, h: 34, at: (d) => Date.parse(d.raised_at ?? '') || null }).map(plain);
  const text = lines.join('\n');
  for (const tag of ['RED · action', 'RED · spec', 'AMBER · build', 'OPEN · question']) assert.ok(text.includes(tag), tag);
  const titleRow = (id: string) => lines.find((l) => l.includes(id))!;
  assert.match(titleRow('DEC-081'), /^ {2}❯ {5}DEC-081/, 'the Red action has no box, and is selected');
  assert.match(titleRow('DEC-083'), /^ {8}DEC-083/);
  assert.match(titleRow('DEC-082'), /^ {4}\[ \] DEC-082/);
  assert.match(text, /no recommendation: your call\. Red actions are decided one at a time\./);
  assert.match(text, /✔ recommend ratify · routine risk, second seat \(scout\) concurred/);
  assert.match(text, /5 decisions · 3 can take the recommendation/);
  assert.match(text, /● all 5 {3}○ actions 1 {3}○ specs 1 {3}○ builds 2 {3}○ questions 1/);
  assert.match(text, /9 m/, 'how long ago it was raised');
});

test('ticked entries show [x], a filtered list says what it hides, and 80 columns keep one sentence of each summary', () => {
  const items = five();
  const ctx = { pal: { dark: true }, now: NOW, h: 24, at: () => null };
  const p = key({ ...newPane(), filter: 'build' }, items, ch('a')).pane;
  const t = view(items, p, { ...ctx, w: 100, h: 34 }).map(plain).join('\n');
  assert.match(t, /2 builds · hiding 3 other decisions/);
  assert.equal((t.match(/\[x\]/g) ?? []).length, 2);
  assert.ok(!t.includes('DEC-081'), 'the filter hides the rest');
  const narrow = view(items, newPane(), { ...ctx, w: 80 }).map(plain);
  assert.ok(narrow.every((l) => [...l].length <= 80), 'nothing past 80 columns');
  assert.ok(narrow.some((l) => l.includes('DEC-081')) && narrow.length === 24);
});

test('opening a card from the list opens the focus view of that card; a build has no detail view yet', () => {
  let s = initial(0);
  s = fold(s, { method: 'event', params: { type: 'approval.requested', task_id: 't_1', payload: { approval_id: 'c1', role: 'builder', tier: 'amber', tool: 'write_file', summary: 'write deck.rs', payload_blake3: 'h', payload: { preview: '+x' }, timeout_secs: 600 } } } as const, 1000);
  const items = parse({ decisions: [raw('c1', 'action', 'amber', 'write deck.rs', 's', null, false, { source: 'card', source_id: 'c1' }), raw('p1', 'build', 'amber', 'b', 's', 'ok', true, { source: 'provisional', source_id: 'p1' })] });
  const inb = newInbox(0, items);
  const open = inboxKey(inb, cards(s), k('enter'), 5000);
  assert.equal(open.inbox.mode, 'focus');
  assert.equal(open.inbox.sel, 0);
  const second = inboxKey({ ...inb, pane: { ...inb.pane, sel: 1 } }, cards(s), k('enter'), 5000);
  assert.equal(second.inbox.mode, 'list', 'a build stays in the list until D2');
  assert.equal(second.action.t, 'none');
  // y and n still answer the selected card (after the guard); on a build they do nothing.
  const y = inboxKey(inb, cards(s), ch('y'), 9000).action;
  assert.deepEqual(y.t === 'resolve' ? [y.card.id, y.decision] : y, ['c1', 'allow']);
  const n = inboxKey(inb, cards(s), ch('n'), 9000).action;
  assert.deepEqual(n.t === 'resolve' ? [n.card.id, n.decision] : n, ['c1', 'deny']);
  assert.equal(inboxKey(inb, cards(s), ch('y'), 1100).action.t, 'none', 'the guard: the card only just appeared');
  assert.equal(inboxKey({ ...inb, pane: { ...inb.pane, sel: 1 } }, cards(s), ch('y'), 9000).action.t, 'none', 'a build is not answered from the list');
  // With no daemon list the inbox is what it was.
  assert.ok(inboxView(s, newInbox(0), info, { dark: true }, 5000, 100, 24).map(plain).join('\n').includes('Pending decisions'));
});

const state = stateWorld();
const base: Model = {
  info, pal: { dark: true, bg: [15, 23, 42] }, state, now: NOW, deck: 2, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0, five()), mode: 'default', panel: null, help: false, still: true,
};
test('golden screens: the list, the filtered list and the list at 80x24', () => {
  for (const [name, m, w, h] of [
    ['decisions-list-100x34', base, 100, 34],
    ['decisions-list-filtered-100x34', { ...base, inbox: newInbox(0, five(), { ...newPane(), filter: 'build', ticks: ['dec-082@h-dec-082', 'dec-085@h-dec-085'] }) }, 100, 34],
    ['decisions-list-80x24', base, 80, 24],
  ] as [string, Model, number, number][]) {
    const r = snapshot(name, frame(m, w, h));
    assert.ok(r.ok, r.message);
  }
});
