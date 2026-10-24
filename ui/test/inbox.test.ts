import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { cardsOf, inboxBody, inboxHint, inboxInit, inboxKey } from '../src/inbox.ts';
import { Observer } from '../src/observer.ts';
import { fold, initial } from '../src/fold.ts';
import { answerCard } from '../src/run.ts';
import { frame, header, footer, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import type { Info } from '../src/theme.ts';

const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: { builder: { name: 'hephaestus', chassis: '#f59e0b', face: '•_•' }, architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' } },
};
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const ev = (type: string, payload: any, task = 't1') => ({ method: 'event', params: { type, task_id: task, payload } });
const card = (id: string, tier: string, role: string, at: number, summary = 'do a thing') =>
  ev('approval.requested', { approval_id: id, role, tier, payload_blake3: 'h' + id, payload: { preview: 'preview ' + id, args: { path: 'x/' + id } }, summary, timeout_secs: 600, tool: 'write_file' }, 't_' + id);
const three = () => [card('a1', 'amber', 'builder', 1000), card('r1', 'red', 'architect', 2000, 'ratify the spec'), card('a2', 'amber', 'builder', 3000)].reduce((s, n) => fold(s, n, n.params.payload.approval_id === 'a1' ? 1000 : n.params.payload.approval_id === 'r1' ? 2000 : 3000), initial(0));
const key = (k: string, extra = {}) => ({ t: 'key', k, alt: false, shift: false, ctrl: false, ...extra }) as any;
const text = (s: string) => ({ t: 'text', s }) as any;
const view = { info, pal: { dark: true }, now: 5000 };

test('red cards come first, then the oldest', () => {
  assert.deepEqual(cardsOf(three()).map((c) => c.id), ['r1', 'a1', 'a2']);
});

test('list mode: arrows select, enter opens focus, tab toggles, esc leaves', () => {
  const cards = cardsOf(three());
  let r = inboxKey(inboxInit(), key('down'), cards);
  assert.equal(r.ui.idx, 1);
  r = inboxKey(r.ui, key('down'), cards); r = inboxKey(r.ui, key('down'), cards);
  assert.equal(r.ui.idx, 2, 'stops at the last card');
  r = inboxKey(r.ui, key('enter'), cards);
  assert.equal(r.ui.mode, 'focus');
  r = inboxKey(r.ui, key('tab'), cards);
  assert.equal(r.ui.mode, 'list');
  assert.deepEqual(inboxKey(r.ui, key('esc'), cards).act, { t: 'leave' });
});

test('y, n and a answer the selected card; a never applies to Red', () => {
  const cards = cardsOf(three());
  const focus = { ...inboxInit(), mode: 'focus' as const, idx: 1 };
  assert.deepEqual(inboxKey(focus, text('y'), cards).act, { t: 'answer', id: 'a1', d: 'allow' });
  assert.deepEqual(inboxKey(focus, text('n'), cards).act, { t: 'answer', id: 'a1', d: 'deny' });
  assert.deepEqual(inboxKey(focus, text('a'), cards).act, { t: 'answer', id: 'a1', d: 'allow_similar' });
  // the reducer asks; answerCard refuses allow-similar on a Red card
  assert.deepEqual(inboxKey({ ...focus, idx: 0 }, text('a'), cards).act, { t: 'answer', id: 'r1', d: 'allow_similar' });
});

test('in list mode a needs a second press to approve every Amber card, and any other key cancels', () => {
  const cards = cardsOf(three());
  let r = inboxKey(inboxInit(), text('a'), cards);
  assert.equal(r.ui.confirm, true);
  assert.equal(r.act, undefined);
  assert.match(plain(inboxHint(r.ui, cards, view)), /approve 2 amber card\(s\) once each\? press a again/);
  const cancelled = inboxKey(r.ui, key('down'), cards);
  assert.equal(cancelled.ui.confirm, false);
  r = inboxKey(r.ui, text('a'), cards);
  assert.deepEqual(r.act, { t: 'amber-all' });
  assert.equal(r.ui.confirm, false);
});

test('only Red cards: a in list mode does nothing', () => {
  const s = fold(initial(0), card('r1', 'red', 'architect', 1000), 1000);
  const r = inboxKey(inboxInit(), text('a'), cardsOf(s));
  assert.equal(r.act, undefined);
  assert.equal(r.ui.confirm, false);
});

test('d shows the full payload, esc closes it before it leaves the inbox', () => {
  const cards = cardsOf(three());
  let r = inboxKey(inboxInit(), text('d'), cards);
  assert.equal(r.ui.mode, 'focus'); assert.equal(r.ui.view, true);
  const body = inboxBody(r.ui, cards, view, 80, 20).map(plain).join('\n');
  assert.match(body, /full payload/); assert.match(body, /x\/r1/);
  r = inboxKey(r.ui, key('esc'), cards);
  assert.equal(r.ui.view, false);
  assert.equal(r.act, undefined);
  assert.deepEqual(inboxKey(r.ui, key('esc'), cards).act, { t: 'leave' });
});

test('an empty inbox renders and does not crash on keys', () => {
  const cards = cardsOf(initial(0));
  assert.match(inboxBody(inboxInit(), cards, view, 80, 10).map(plain).join('\n'), /NOTHING PENDING/);
  assert.equal(inboxKey(inboxInit(), text('y'), cards).act, undefined);
  assert.equal(inboxKey(inboxInit(), key('down'), cards).ui.idx, 0);
});

test('list and focus render at every width without overflow, and show the countdown', () => {
  const cards = cardsOf(three());
  for (const w of [40, 80, 120]) for (const mode of ['list', 'focus'] as const) {
    const lines = inboxBody({ ...inboxInit(), mode }, cards, view, w, 16);
    assert.equal(lines.length, 16);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${mode} ${w}: ${plain(l)}`);
  }
  const list = inboxBody(inboxInit(), cards, view, 100, 12).map(plain).join('\n');
  assert.match(list, /DECISION INBOX · 3 PENDING/);
  assert.match(list, /RED .*ratify the spec/);
  assert.match(list, /AMB .*do a thing/);
  const focus = inboxBody({ ...inboxInit(), mode: 'focus' }, cards, view, 100, 16).map(plain).join('\n');
  assert.match(focus, /APPROVAL REQUIRED \[1 of 3\] · red-tier gate/);
  assert.match(focus, /Task +: t_r1/);
});

test('a card is answered with its own hash, not the newest one\'s', async () => {
  const calls: any[] = [];
  const cmd = { call: async (m: string, p: any) => { calls.push([m, p]); return {}; } } as any;
  const s = three();
  assert.equal(await answerCard(cmd, s.approvals.a1!, 'allow', 5000), null);
  assert.deepEqual(calls, [['approval.resolve', { approval_id: 'a1', payload_blake3: 'ha1', decision: 'allow' }]]);
  assert.match((await answerCard(cmd, s.approvals.r1!, 'allow_similar', 5000))!, /never applies to a Red/);
  assert.match((await answerCard(cmd, s.approvals.r1!, 'allow', 2100))!, /only just appeared/);
  assert.equal(calls.length, 1);
});

test('deck 2 swaps the prompt for the inbox hint and the header counts the decisions', () => {
  const s = three();
  const m: Model = { info, pal: { dark: true }, state: s, now: 5000, deck: 2, composer: C.insert(C.empty(), 'draft text'), back: 0, menu: null, expandTools: false, search: null, queue: [], help: false, inbox: inboxInit(), mode: 'default', obs: new Observer(), panel: null };
  const out = frame(m, 100, 24).map(plain).join('\n');
  assert.doesNotMatch(out, /draft text/, 'the draft is kept but not shown while the inbox has the keys');
  assert.match(out, /decision 1\/3/);
  assert.match(plain(header(m, 100)[0]!), /3 decisions pending/);
});

test('the footer names a non-default approval mode', () => {
  const m = { info, pal: { dark: true }, state: initial(0), now: 0, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null, queue: [], help: false, inbox: inboxInit(), mode: 'accept-edits', obs: new Observer(), panel: null } as Model;
  assert.match(plain(footer(m, 100)), /0 tokens · accept-edits · approvals|0 tokens · accept-edits · shortcuts/);
});

test('shift+tab goes round default, accept-edits, mission-auto and back, whatever it starts from', async () => {
  const { nextMode, modeNote } = await import('../src/run.ts');
  assert.deepEqual(['default', 'accept-edits', 'mission-auto', 'default'].map(nextMode), ['accept-edits', 'mission-auto', 'default', 'accept-edits']);
  assert.equal(nextMode('something-new'), 'default', 'an unknown mode starts the round again');
  assert.match(modeNote('mission-auto'), /lockfile/);
});
