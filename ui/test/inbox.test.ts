import test from 'node:test';
import assert from 'node:assert/strict';
import { cards, GUARD_MS, key, newInbox, options, view, hint } from '../src/inbox.ts';
import { fold, initial } from '../src/fold.ts';
import { frame, header, footer, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import type { Info } from '../src/theme.ts';
import type { Ev } from '../src/input.ts';

const info: Info = {
  console_socket: '', narrator_socket: '', workspace: '', mission: null, version: '0.5', cwd: '/proj',
  roles: { orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' }, architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' }, builder: { name: 'hephaestus', chassis: '#f97316', face: '•─•' }, 'red-team': { name: 'ares', chassis: '#ef4444', face: '-_-' } },
};
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const pal = { dark: true };
const req = (id: string, role: string, tier: string, summary: string, at: number) => ({ method: 'event', params: { type: 'approval.requested', task_id: 't_1', payload: { approval_id: id, role, tier, tool: 'write_file', summary, payload_blake3: 'h-' + id, payload: { preview: '+ new line\n- old line' }, timeout_secs: 600 } } }) as const;
const k = (name: string): Ev => ({ t: 'key', k: name as any, alt: false, shift: false, ctrl: false });
const ch = (s: string): Ev => ({ t: 'text', s });

function three() {
  let s = initial(0);
  s = fold(s, req('a2', 'builder', 'amber', 'write deck.rs', 2000), 2000);
  s = fold(s, req('r1', 'architect', 'red', 'ratify spec 005b', 3000), 3000);
  s = fold(s, req('a3', 'builder', 'amber', 'cargo test', 4000), 4000);
  return s;
}

test('red cards come first, then oldest first; red offers no "allow similar"', () => {
  const list = cards(three());
  assert.deepEqual(list.map((c) => c.id), ['r1', 'a2', 'a3']);
  assert.deepEqual(options(list[0]!).map((o) => o.d), ['allow', 'deny']);
  assert.deepEqual(options(list[1]!).map((o) => o.d), ['allow', 'allow_similar', 'deny']);
});

test('keys that answer are ignored for 300 ms after the deck opens, and after a card appears', () => {
  const list = cards(three());
  const open = newInbox(10_000);
  assert.equal(key(open, list, ch('y'), 10_000 + GUARD_MS - 1).action.t, 'none', 'the deck only just opened');
  const fresh = fold(initial(0), req('x', 'builder', 'amber', 'w', 20_000), 20_000);
  assert.equal(key(newInbox(0), cards(fresh), ch('y'), 20_100).action.t, 'none', 'the card only just appeared');
  const a = key(newInbox(0), cards(fresh), ch('y'), 20_000 + GUARD_MS).action;
  assert.deepEqual(a.t === 'resolve' && [a.card.id, a.decision], ['x', 'allow']);
});

test('list: arrows move, enter opens focus, y and n answer the selected one, esc goes back to chat', () => {
  const list = cards(three());
  let r = key(newInbox(0), list, k('down'), 9000);
  assert.equal(r.inbox.sel, 1);
  r = key(r.inbox, list, k('enter'), 9000);
  assert.equal(r.inbox.mode, 'focus');
  r = key({ ...r.inbox, mode: 'list' }, list, ch('n'), 9000);
  assert.deepEqual(r.action.t === 'resolve' && [r.action.card.id, r.action.decision], ['a2', 'deny']);
  assert.equal(key(newInbox(0), list, k('esc'), 9000).action.t, 'chat');
});

test('focus: numbers and enter choose an option; a red card has no option 3', () => {
  const list = cards(three());
  const f = { ...newInbox(0), mode: 'focus' as const };
  const two = key(f, list, ch('2'), 9000).action;
  assert.deepEqual(two.t === 'resolve' && two.decision, 'deny', 'for a red card, 2 is Reject');
  assert.equal(key(f, list, ch('3'), 9000).action.t, 'none');
  const amber = key({ ...f, sel: 1 }, list, ch('2'), 9000).action;
  assert.deepEqual(amber.t === 'resolve' && amber.decision, 'allow_similar');
  const viaEnter = key({ ...f, sel: 1, opt: 2 }, list, k('enter'), 9000).action;
  assert.deepEqual(viaEnter.t === 'resolve' && viaEnter.decision, 'deny');
});

test('approve-all-amber needs a second press and never includes a red card', () => {
  const list = cards(three());
  const first = key(newInbox(0), list, ch('a'), 9000);
  assert.equal(first.action.t, 'none');
  assert.equal(first.inbox.confirmAll, true);
  const second = key(first.inbox, list, ch('a'), 9000).action;
  assert.deepEqual(second.t === 'resolve-all' && second.cards.map((c) => c.id), ['a2', 'a3']);
  assert.equal(key(first.inbox, list, k('down'), 9000).inbox.confirmAll, false, 'any other key cancels the confirmation');
});

test('the views follow the design: list rows with tier, role badge and time; focus card with options and the diff', () => {
  const s = three();
  const list = view(s, newInbox(0), info, pal, 5000, 100, 24).map(plain).join('\n');
  assert.match(list, /Pending decisions \(3\)/);
  assert.match(list, /❯ 1\. RED\s+ratify spec 005b\s+⟦•_•⟧ daedalus\s+09:58/);
  assert.match(list, /2\. AMB\s+write deck\.rs/);
  const focus = view(s, { ...newInbox(0), mode: 'focus' }, info, pal, 5000, 100, 24).map(plain).join('\n');
  assert.match(focus, /Approval required · 1 of 3 pending · Red-tier gate/);
  assert.match(focus, /Requester: ⟦•_•⟧ daedalus \(architect\) · Task: t_1/);
  assert.match(focus, /❯ 1\. Approve once\n\s+2\. Reject/);
  assert.match(focus, /\+ new line\n\s*- old line/);
  assert.match(view(initial(0), newInbox(0), info, pal, 0, 100, 10).map(plain).join('\n'), /nothing needs you/);
  assert.match(hint(newInbox(0), 3), /a approve all amber/);
});

test('the header alert uses the red-team badge and counts decisions; the footer shows the approval mode', () => {
  const s = three();
  const m = (over: Partial<Model> = {}): Model => ({ info, pal, state: s, now: 5000, deck: 2, composer: C.empty(), back: 0, help: false, menu: null, expandTools: false, search: null, queue: [], inbox: newInbox(0), mode: 'default', panel: null, ...over });
  assert.match(plain(header(m(), 100)[0]!), /^⟦x\?x⟧ ares: 3 decisions pending · \d\d:\d\d remaining\s+navigate: shift\+arrows · ○ ○ ●$/);
  assert.match(plain(footer(m(), 100)), /^0 tokens · approvals · 3 waiting · shortcuts: \?/);
  assert.match(plain(footer(m({ mode: 'mission-auto' }), 100)), /· auto ·/);
  const lines = frame(m(), 100, 30).map(plain);
  assert.ok(lines.some((l) => l.startsWith('enter open card')), 'the prompt row is replaced by the key hint on this deck');
  assert.ok(!lines.some((l) => l.startsWith('❯ ') && l.includes('draft')));
});
