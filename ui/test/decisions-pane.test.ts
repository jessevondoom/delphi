// The decisions pane's list (specs/044 §5): the list from the daemon, tier colours and tags, the kind filter, tick
// boxes only where the daemon allows, and the golden screens at 100x34 and 80x24.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { chips, confirm, detail, hint, isTicked, key, newPane, parse, promptLine, reconcile, replied, ticked, unsent, view, visible, withThread, type Decision, type Pane } from '../src/decisions-pane.ts';
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


const open = (items: Decision[], at: number, now = 1000): Pane => key({ ...newPane(), sel: at }, items, k('enter'), now).pane;

test('enter opens the detail view: a recommended decision starts on the accept button, one with no recommendation on nothing', () => {
  const items = five();
  const rec = open(items, 2);
  assert.equal(rec.open, 'dec-082');
  assert.equal(rec.opt, 0, 'the recommended choice is focused');
  const none = open(items, 0);
  assert.equal(none.open, 'dec-081');
  assert.equal(none.opt, null, 'nothing is preselected for a decision with no recommendation');
  assert.equal(key(none, items, k('enter'), 5000).action.t, 'none', 'enter with nothing focused chooses nothing');
});

test('answering: the guard, one answer only, a number takes that choice, and the note says what was decided', () => {
  const items = five();
  const p = open(items, 2, 1000);
  assert.equal(key(p, items, k('enter'), 1100).action.t, 'none', 'the guard: the decision only just opened');
  const r = key(p, items, k('enter'), 1400);
  assert.equal(r.action.t, 'answer');
  assert.equal(r.action.t === 'answer' && r.action.choice.id, 'ratified');
  assert.equal(r.pane.answered, 'dec-082');
  assert.equal(r.pane.note, '✔ DEC-082 decided: ratify');
  assert.equal(key(r.pane, items, k('enter'), 2000).action.t, 'none', 'a second press does not send it twice');
  const two = key(p, items, ch('2'), 1400);
  assert.equal(two.action.t === 'answer' && two.action.choice.id, 'rejected');
  assert.equal(key(p, items, ch('9'), 1400).action.t, 'none', 'there is no ninth choice');
  assert.equal(unsent(r.pane).answered, null, 'a failed answer can be sent again');
});

test('tab and shift+tab walk the choices; up and down walk the decisions of the filter; esc and left go back; ctrl+d shows the payload', () => {
  const items = five();
  let p = open(items, 0);
  assert.equal(p.opt, null);
  p = key(p, items, k('tab'), 2000).pane;
  assert.equal(p.opt, 0);
  p = key(p, items, k('tab'), 2000).pane;
  assert.equal(p.opt, 1);
  p = key(p, items, k('tab'), 2000).pane;
  assert.equal(p.opt, 0, 'wraps');
  assert.equal(key(open(items, 0), items, k('shift-tab'), 2000).pane.opt, 1, 'backwards from nothing lands on the last');
  const next = key(open(items, 0), items, k('down'), 2000).pane;
  assert.deepEqual([next.open, next.sel, next.openedAt], ['dec-083', 1, 2000]);
  assert.equal(key(open(items, 0), items, k('up'), 2000).pane.open, 'dec-081', 'the first has nothing before it');
  assert.equal(key({ ...open(items, 2), filter: 'build', sel: 0 }, items, k('down'), 2000).pane.open, 'dec-085');
  assert.equal(key(p, items, k('esc'), 2000).pane.open, null);
  assert.equal(key(p, items, k('left'), 2000).pane.open, null);
  assert.equal(key(p, items, { t: 'ctrl', c: 'd' }, 2000).pane.payload, true);
  assert.equal(key(p, items, ch('d'), 2000).pane.payload, false, 'a plain d is the start of a word the operator is typing');
});

test('when the open decision is answered the pane moves to the next in the filter, and leaves when none is left', () => {
  const items = five();
  const p = open(items, 2);
  const rest = items.filter((d) => d.label !== 'DEC-082');
  const r = reconcile({ ...p, answered: 'dec-082' }, rest);
  assert.equal(r.open, 'dec-085', 'the next one');
  assert.equal(r.answered, null);
  assert.equal(r.opt, 0);
  const last = reconcile({ ...open(items, 4), answered: 'dec-084' }, items.filter((d) => d.label !== 'DEC-084'));
  assert.equal(last.open, 'dec-085', 'the last one answered: the previous one, so it stays inside the list');
  assert.equal(reconcile({ ...p }, []).open, null, 'nothing left: back to the empty state');
  assert.equal(reconcile(p, items).sel, 2, 'still open and in place');
});

test('the detail view: a recommendation card with the accept button, or a no-recommendation card with nothing selected', () => {
  const items = five();
  const ctx = { pal: { dark: true }, now: NOW, w: 100, h: 34, at: (d: Decision) => Date.parse(d.raised_at ?? '') || null };
  const rec = detail(items, open(items, 2), ctx).map(plain).join('\n');
  for (const want of ['‹ decisions', '3 of 5 · ↑ previous · ↓ next', 'DEC-082', 'AMBER · build · requested by builder · task t_1 · waiting 9 m', 'WHAT IT IS', 'WHAT IT ADDS OR CHANGES', 'RECOMMENDATION', '[ Accept recommendation ]', 'enter, or 1', 'other choices   2 Reject', 'Ratify. Routine risk, second seat (scout) concurred.']) assert.ok(rec.includes(want), want);
  const red = detail(items, open(items, 1), ctx).map(plain).join('\n');
  assert.ok(red.includes('Red: you decide, nothing is ticked for you'));
  const none = detail(items, open(items, 0), ctx).map(plain).join('\n');
  assert.ok(none.includes('NO RECOMMENDATION') && none.includes('your call'));
  assert.ok(!none.includes('Accept recommendation'), 'no accept button without a recommendation');
  assert.ok(!none.includes('❯'), 'nothing is selected');
  const tabbed = detail(items, key(open(items, 0), items, k('tab'), 2000).pane, ctx).map(plain).join('\n');
  assert.match(tabbed, /❯ 1 {2}Ratify/);
  const diff = detail(items, { ...open(items, 2), payload: true }, { ...ctx, payload: () => ['+ new line', '- old line'] }).map(plain).join('\n');
  assert.ok(diff.includes('CHANGES') && diff.includes('+ new line') && !diff.includes('WHAT IT IS'));
});

test('an answer becomes the call for the decision source: a card resolves, a provisional decision is reviewed, and the list answers nothing', () => {
  let s = initial(0);
  s = fold(s, { method: 'event', params: { type: 'approval.requested', task_id: 't_1', payload: { approval_id: 'c1', role: 'builder', tier: 'amber', tool: 'write_file', summary: 'write deck.rs', payload_blake3: 'h', payload: { preview: '+x' }, timeout_secs: 600 } } } as const, 1000);
  const cardChoices = [{ id: 'allow', label: 'Approve once', effect: 'approval.resolve', reversible: false }, { id: 'allow_similar', label: 'Allow similar actions this task', effect: 'approval.resolve', reversible: false }, { id: 'deny', label: 'Reject', effect: 'approval.resolve', reversible: false }];
  const items = parse({ decisions: [raw('c1', 'action', 'amber', 'write deck.rs', 's', null, false, { source: 'card', source_id: 'c1', choices: cardChoices }), raw('p1', 'build', 'amber', 'b', 's', 'ok', true, { source: 'provisional', source_id: 'p1' })] });
  let inb = newInbox(0, items);
  assert.equal(inboxKey(inb, cards(s), ch('y'), 9000).action.t, 'none', 'the list answers nothing');
  inb = inboxKey(inb, cards(s), k('enter'), 5000).inbox;
  assert.equal(inb.pane.open, 'c1');
  const r = inboxKey(inb, cards(s), ch('3'), 9000).action;
  assert.deepEqual(r.t === 'resolve' ? [r.card.id, r.decision] : r, ['c1', 'deny']);
  assert.equal(inboxKey(inb, cards(s), ch('3'), 5100).action.t, 'none', 'the guard');
  let inb2 = newInbox(0, items);
  inb2 = inboxKey({ ...inb2, pane: { ...inb2.pane, sel: 1 } }, cards(s), k('enter'), 5000).inbox;
  const v = inboxKey(inb2, cards(s), k('enter'), 9000).action;
  assert.deepEqual(v.t === 'review' ? [v.d.source_id, v.resolution] : v, ['p1', 'ratified']);
});

test('nothing owed: the daemon list has arrived and is empty, so the pane says so; before it arrives the inbox is what it was', () => {
  const s = initial(0);
  const inb = newInbox(0, [], newPane(), true);
  const t = inboxView(s, inb, info, { dark: true }, 5000, 100, 12).map(plain).join('\n');
  assert.ok(t.includes('Nothing needs you.'));
  const done = inboxView(s, { ...inb, pane: { ...inb.pane, note: '✔ DEC-081 decided: reject' } }, info, { dark: true }, 5000, 100, 12).map(plain).join('\n');
  assert.ok(done.includes('✔ DEC-081 decided: reject'));
  assert.ok(inboxView(s, newInbox(0), info, { dark: true }, 5000, 100, 12).map(plain).join('\n').includes('nothing needs you'), 'no daemon list yet');
  assert.equal(inboxKey(inb, [], k('esc'), 9000).action.t, 'chat');
});

const tickedPane = (items: Decision[]): Pane => key(newPane(), items, ch('a')).pane;

test('y with ticks opens the confirm page, y with none does nothing, and esc goes back with the ticks kept', () => {
  const items = five();
  assert.equal(key(newPane(), items, ch('y'), 2000).pane.confirm, false, 'nothing is ticked');
  const p = key(tickedPane(items), items, ch('y'), 2000).pane;
  assert.deepEqual([p.confirm, p.confirmedAt, p.ticks.length], [true, 2000, 3]);
  const back = key(p, items, k('esc'), 3000).pane;
  assert.deepEqual([back.confirm, back.ticks.length], [false, 3]);
  assert.equal(key(p, items, k('left'), 3000).pane.confirm, false);
  assert.equal(key(p, items, ch('x'), 3000).action.t, 'none', 'no other key does anything on the confirm page');
});

test('enter on the confirm page accepts exactly what was ticked, after the guard, and clears the ticks', () => {
  const items = five();
  const p = key(tickedPane(items), items, ch('y'), 2000).pane;
  assert.equal(key(p, items, k('enter'), 2100).action.t, 'none', 'the guard: the page only just opened');
  const r = key(p, items, k('enter'), 2400);
  assert.equal(r.action.t, 'accept-all');
  assert.deepEqual(r.action.t === 'accept-all' ? r.action.ds.map((d) => d.label) : [], ['DEC-082', 'DEC-085', 'DEC-084']);
  assert.deepEqual([r.pane.confirm, r.pane.ticks.length, r.pane.note], [false, 0, '✔ accepting 3 recommendations…']);
  assert.notEqual(key(r.pane, items, k('enter'), 3000).action.t, 'accept-all', 'nothing is ticked any more, so a second press accepts nothing');
});

test('the confirm page closes by itself when the ticks go away (a payload changed, or the decisions were answered elsewhere)', () => {
  const items = five();
  const p = key(tickedPane(items), items, ch('y'), 2000).pane;
  assert.equal(reconcile(p, items).confirm, true);
  assert.equal(reconcile(p, items.map((d) => ({ ...d, payload_blake3: d.payload_blake3 + 'x' }))).confirm, false);
  assert.equal(reconcile(p, items.filter((d) => !d.tickable)).confirm, false);
});

test('the confirm page names what is accepted, what each does, and what is not included', () => {
  const items = five();
  const p = key(tickedPane(items), items, ch('y'), 2000).pane;
  const t = confirm(items, p, { pal: { dark: true }, now: NOW, w: 100, h: 34, at: () => null }).map(plain).join('\n');
  for (const want of ['Accept 3 recommendations?', '✔ DEC-082', '✔ DEC-085', '✔ DEC-084', "ratify · builder's build on task t_1", 'not included: these need you one at a time', 'DEC-081', 'DEC-083', 'Each acceptance is signed as you and logged', ' Accept 3 ']) assert.ok(t.includes(want), want);
  assert.equal((t.match(/✔ DEC-/g) ?? []).length, 3, 'only the ticked ones are listed as accepted');
  const list = view(items, tickedPane(items), { pal: { dark: true }, now: NOW, w: 100, h: 34, at: () => null }).map(plain).join('\n');
  assert.match(list, /y {2}review 3 ticked/);
});

test('through the inbox, enter on the confirm page is one accept action for the ticked decisions', () => {
  const items = five();
  const inb = newInbox(0, items, { ...tickedPane(items), confirm: true, confirmedAt: 0 });
  const r = inboxKey(inb, [], k('enter'), 5000);
  assert.equal(r.action.t, 'accept');
  assert.equal(r.action.t === 'accept' && r.action.ds.length, 3);
  assert.equal(r.inbox.pane.ticks.length, 0);
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
    ['decisions-detail-100x34', { ...base, inbox: newInbox(0, five(), open(five(), 2)) }, 100, 34],
    ['decisions-detail-red-100x34', { ...base, inbox: newInbox(0, five(), open(five(), 1)) }, 100, 34],
    ['decisions-detail-norec-100x34', { ...base, inbox: newInbox(0, five(), open(five(), 0)) }, 100, 34],
    ['decisions-detail-80x24', { ...base, inbox: newInbox(0, five(), open(five(), 2)) }, 80, 24],
    ['decisions-confirm-100x34', { ...base, inbox: newInbox(0, five(), { ...tickedPane(five()), confirm: true, confirmedAt: 0 }) }, 100, 34],
    ['decisions-empty-100x34', { ...base, inbox: newInbox(0, [], { ...newPane(), note: '✔ DEC-081 decided: reject' }, true) }, 100, 34],
  ] as [string, Model, number, number][]) {
    const r = snapshot(name, frame(m, w, h));
    assert.ok(r.ok, r.message);
  }
});

// ── typing instead of choosing (specs/044 §6.3) ──
const typeIn = (p: Pane, items: Decision[], text: string, now = 2000): Pane => [...text].reduce((q, c) => key(q, items, ch(c), now).pane, p);

test('opening a decision asks for its saved thread, and the thread shows under EARLIER ON THIS DECISION', () => {
  const items = five();
  const r = key({ ...newPane(), sel: 1 }, items, k('enter'), 1000);
  assert.deepEqual([r.action.t, r.action.t === 'thread' && r.action.d.id], ['thread', 'dec-083']);
  const p = withThread(r.pane, 'dec-083', [{ role: 'you', kind: 'question', text: 'why 64 KB?', tokens: 0 }, { role: 'model', kind: 'reply', text: 'It keeps a frame under the buffer.', tokens: 600 }, { junk: true }]);
  assert.equal(p.thread.length, 2, 'a malformed line is dropped');
  const t = detail(items, p, { pal: { dark: true }, now: NOW, w: 100, h: 34, at: () => null }).map(plain).join('\n');
  assert.match(t, /EARLIER ON THIS DECISION · saved with it, 0 tokens to reopen/);
  assert.match(t, /you\s+why 64 KB\?/);
  assert.match(t, /model It keeps a frame under the buffer\./);
  assert.equal(withThread(p, 'dec-081', [{ role: 'you', kind: 'question', text: 'x' }]), p, 'a thread for a decision no longer open is ignored');
  assert.equal(key(p, items, k('down'), 2000).pane.thread.length, 0, 'moving on clears it until the next one loads');
});

test('typing in the detail view fills the prompt bound to the decision; digits choose only while it is empty', () => {
  const items = five();
  let p = open(items, 2);
  assert.match(plain(promptLine(p, 100)!), /^❯ +or type here to answer in your own words/);
  assert.equal(promptLine(newPane(), 100), null, 'the list has no prompt');
  p = typeIn(p, items, 'do it, but 2 weeks');
  assert.equal(p.draft, 'do it, but 2 weeks', 'd and 2 inside a sentence are letters, not shortcuts');
  assert.equal(p.payload, false);
  assert.match(plain(promptLine(p, 100)!), /^❯ do it, but 2 weeks/);
  p = key(p, items, k('backspace'), 2000).pane;
  assert.equal(p.draft, 'do it, but 2 week');
  assert.equal(key(p, items, { t: 'paste', s: ' s\n  here' }, 2000).pane.draft, 'do it, but 2 week s here');
  assert.equal(key(p, items, { t: 'ctrl', c: 'u' }, 2000).pane.draft, '');
  assert.equal(key(p, items, k('esc'), 2000).pane.draft, '', 'esc takes back the words first');
  assert.equal(key(p, items, k('esc'), 2000).pane.open, p.open);
  assert.equal(key(p, items, k('left'), 2000).pane.open, p.open, 'left does not leave while words are typed');
  assert.equal(typeIn(open(items, 2), items, 'x'.repeat(700)).draft.length, 600);
  assert.equal(key(open(items, 2), items, ch('1'), 2000).action.t, 'answer', 'an empty prompt: 1 picks the first choice');
});

test('enter sends the words once to be read; a bare choice is the choice itself and goes to no model', () => {
  const items = five();
  let p = typeIn(open(items, 2), items, 'what does this change?');
  const sent = key(p, items, k('enter'), 2000);
  assert.deepEqual([sent.action.t, sent.action.t === 'reply' && sent.action.text, sent.pane.draft, sent.pane.busy], ['reply', 'what does this change?', '', true]);
  assert.match(plain(promptLine(sent.pane, 100)!), /reading your words/);
  assert.equal(key({ ...sent.pane, draft: 'again' }, items, k('enter'), 2000).action.t, 'none', 'one reply at a time');
  assert.equal(key(open(items, 2), items, ch('2'), 2000).action.t, 'answer', 'a lone digit picks at once: no model, no draft');
  for (const bare of ['accept', 'Accept recommendation', 'reject', 'Ratify']) {
    const r = key(typeIn(open(items, 2), items, bare), items, k('enter'), 2000);
    assert.equal(r.action.t, 'answer', bare);
  }
  const rej = key(typeIn(open(items, 2), items, 'reject'), items, k('enter'), 2000);
  assert.equal(rej.action.t === 'answer' && rej.action.choice.id, 'rejected');
  assert.equal(rej.action.t === 'answer' && rej.action.conditions, undefined, 'a bare choice carries no conditions');
  assert.equal(key(typeIn(open(items, 0), items, 'accept'), items, k('enter'), 2000).action.t, 'reply', 'no recommendation: "accept" is not a choice, so it is read');
  p = unsent(sent.pane);
  assert.equal(p.busy, false);
});

const withReading = (items: Decision[]) => {
  const sent = key(typeIn(open(items, 2), items, 'reject it but keep t_016 paused'), items, k('enter'), 2000);
  return replied(sent.pane, 'dec-082', { intent: 'answer', reading: { choice_id: 'rejected', label: 'Reject', conditions: ['keep t_016 paused', '\x1b[31mtell the architect'] }, tokens: 612, thread: [{ role: 'you', kind: 'answer', text: 'reject it', tokens: 0 }, { role: 'model', kind: 'reading', text: 'Reject', choice_id: 'rejected', conditions: ['keep t_016 paused'], tokens: 612 }] });
};

test('a reading shows as READ AS, decides nothing, and needs enter on an empty prompt to confirm', () => {
  const items = five();
  const p = withReading(items);
  assert.deepEqual([p.busy, p.reading?.choice_id, p.reading?.conditions], [false, 'rejected', ['keep t_016 paused', 'tell the architect']]);
  assert.equal(p.rnote, 'that reply used 612 tokens and is saved with the decision');
  const t = detail(items, p, { pal: { dark: true }, now: NOW, w: 100, h: 34, at: () => null }).map(plain).join('\n');
  assert.match(t, /READ AS +nothing is decided yet/);
  assert.match(t, /2 +Reject/);
  assert.match(t, /\+ +keep t_016 paused/);
  assert.match(t, /\[ Confirm this \]/);
  assert.equal(hint(items, p), 'enter confirm the reading · keep typing to change it · tab back to the choices · esc discard');
  const c = key(p, items, k('enter'), 5000);
  assert.deepEqual([c.action.t, c.action.t === 'answer' && c.action.choice.id, c.action.t === 'answer' && c.action.conditions], ['answer', 'rejected', ['keep t_016 paused', 'tell the architect']]);
  assert.equal(c.pane.reading, null);
});

test('a reading is dropped by esc or tab, replaced by typing again, and never confirms itself', () => {
  const items = five();
  const p = withReading(items);
  assert.equal(key(p, items, k('esc'), 5000).pane.reading, null);
  assert.equal(key(p, items, k('esc'), 5000).pane.open, p.open, 'the first esc only discards the reading');
  const tab = key(p, items, k('tab'), 5000).pane;
  assert.deepEqual([tab.reading, tab.opt], [null, 1], 'tab goes back to the choices, moving the focus on');
  const again = key(typeIn(p, items, 'actually ratify'), items, k('enter'), 5000);
  assert.deepEqual([again.action.t, again.pane.reading], ['reply', null], 'typing and sending reads again; the old reading is gone');
  assert.equal(replied(p, 'dec-081', { intent: 'answer', reading: { choice_id: 'rejected', label: 'Reject', conditions: [] } }), p, 'a reply for another decision is ignored');
  assert.equal(replied({ ...p, reading: null }, 'dec-082', { intent: 'question', reply: 'x', tokens: 5, thread: [] }).reading, null, 'a question leaves nothing to confirm');
  assert.equal(key(p, items, k('enter'), 1100).action.t, 'none', 'the guard counts from when the decision opened');
  assert.equal(key(p, items, k('enter'), 1400).action.t, 'answer');
});

test('ctrl+r asks for a model brief only on a card that has the plain one', () => {
  const items = parse({ decisions: [raw('c1', 'action', 'amber', 'write deck.rs', 's', null, false, { source: 'card', source_id: 'a1', choices: [{ id: 'allow', label: 'Approve once', effect: 'x', reversible: true }, { id: 'deny', label: 'Reject', effect: 'x', reversible: true }] })] });
  const plainCard = { ...items[0]!, brief: { ...items[0]!.brief, state: 'plain' } };
  const p = open([plainCard], 0);
  assert.equal(key(p, [plainCard], { t: 'ctrl', c: 'r' }, 2000).action.t, 'retry');
  assert.equal(key(p, items, { t: 'ctrl', c: 'r' }, 2000).action.t, 'none', 'an authored brief is not asked for again');
  const gen = { ...plainCard, brief: { ...plainCard.brief, state: 'generated', tokens: 312 } };
  const t = detail([gen], open([gen], 0), { pal: { dark: true }, now: NOW, w: 100, h: 34, at: () => null }).map(plain).join('\n');
  assert.match(t, /summary cached, written once \(312 tokens\)/);
});

test('through the inbox the new actions pass, and a confirmed reading carries its notes to the call that resolves', () => {
  const items = five().map((d) => ({ ...d, source: 'provisional' }));
  const inb = newInbox(0, items);
  const opened = inboxKey(inb, [], k('enter'), 1000);
  assert.equal(opened.action.t, 'thread');
  const typed = [...'hello'].reduce((i, c) => inboxKey(i, [], ch(c), 2000).inbox, opened.inbox);
  assert.equal(typed.pane.draft, 'hello');
  assert.equal(inboxKey(typed, [], k('enter'), 2000).action.t, 'reply');
  assert.equal(inboxKey(typed, [], { t: 'ctrl', c: 'u' }, 2000).inbox.pane.draft, '');
  const conf = inboxKey({ ...inb, pane: { ...open(items, 2), reading: { choice_id: 'rejected', label: 'Reject', conditions: ['hold t_016'] } } }, [], k('enter'), 5000);
  assert.deepEqual([conf.action.t, conf.action.t === 'review' && conf.action.resolution, conf.action.t === 'review' && conf.action.conditions], ['review', 'rejected', ['hold t_016']]);
});

test('a raised decision is answered with decision.resolve, singly and in the batch of recommended defaults', () => {
  const items = five().map((d) => ({ ...d, source: 'raised' }));
  const opened = inboxKey(newInbox(0, items), [], k('enter'), 1000);
  assert.equal(opened.action.t, 'thread');
  const one = inboxKey(opened.inbox, [], ch('1'), 2000);
  assert.deepEqual([one.action.t, one.action.t === 'record' && one.action.d.id, one.action.t === 'record' && one.action.choice_id], ['record', 'dec-081', 'ratified']);
  const cond = inboxKey({ ...opened.inbox, pane: { ...opened.inbox.pane, reading: { choice_id: 'rejected', label: 'Reject', conditions: ['hold t_016'] } } }, [], k('enter'), 5000);
  assert.deepEqual([cond.action.t, cond.action.t === 'record' && cond.action.conditions], ['record', ['hold t_016']]);
  const batch = inboxKey(newInbox(0, items, { ...tickedPane(items), confirm: true, confirmedAt: 0 }), [], k('enter'), 5000);
  assert.equal(batch.action.t, 'accept', 'the batch carries the raised ones too; the console calls decision.resolve for each');
});

test('a digest candidate is a learn entry: labelled "learn", never "untrusted", and keep is its recommendation', () => {
  const items = parse({ decisions: [raw('digest:2026-10-29', 'learn', 'open', 'build the table', 'Captured from the Council’s verdict, not validated.', 'keep', true, { source: 'digest', source_id: 'n2' })] });
  assert.equal(items.length, 1);
  assert.equal(items[0].kind, 'learn');
  const text = view(items, newPane(), { pal: { dark: true }, now: NOW, w: 100, h: 20, at: (d) => Date.parse(d.raised_at ?? '') || null }).map(plain).join('\n');
  assert.ok(text.includes('OPEN · learn'), text);
  assert.ok(!/untrusted/i.test(text));
});
