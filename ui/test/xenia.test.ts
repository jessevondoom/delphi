// `/xenia`: the org store's status and the steward's attestation card in the console.
import test from 'node:test';
import assert from 'node:assert/strict';
import { runSlash, type Ctx } from '../src/run.ts';
import { cardText, parseXenia, resultText, statusText, tokenize } from '../src/xenia.ts';
import { initial } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: {} } as unknown as Info;
function ctx(reply: (m: string, p: any) => any) {
  const out: string[] = []; const calls: any[] = [];
  const c: Ctx = { cmd: { call: async (m: string, p: any) => { calls.push([m, p]); return reply(m, p); } } as any, mission: 'm1', info, state: () => initial(0), post: (t) => out.push(t) };
  return { c, out, calls };
}

test('words keep their quotes: a note has spaces', () => {
  assert.deepEqual(tokenize('endorse 01ABC note="the retry window is wrong" scope=a,b'), ['endorse', '01ABC', 'note=the retry window is wrong', 'scope=a,b']);
});

test('/xenia parses a status request, an action with options, and the two answers to a card', () => {
  assert.deepEqual(parseXenia(''), { kind: 'status' });
  assert.deepEqual(parseXenia('status acme/main'), { kind: 'status', store: 'acme/main' });
  assert.deepEqual(parseXenia('dispute 01ABC note="wrong window"'), { kind: 'prepare', params: { action: 'dispute', target: '01ABC', options: { note: 'wrong window' } } });
  assert.deepEqual(parseXenia('withdraw 01ABC reason=sensitive store=acme/main'), { kind: 'prepare', params: { action: 'withdraw', target: '01ABC', options: { reason: 'sensitive' }, store: 'acme/main' } });
  assert.deepEqual(parseXenia('sign 0123456789ab flags'), { kind: 'decide', code: '0123456789ab', decision: 'sign', flags: true });
  assert.deepEqual(parseXenia('cancel 0123456789ab'), { kind: 'decide', code: '0123456789ab', decision: 'cancel', flags: false });
  for (const bad of ['sign', 'sign nope', 'sign 0123456789AB', 'bogus', 'endorse a b']) assert.equal(parseXenia(bad).kind, 'usage', bad);
});

test('/xenia asks the daemon for the card and never signs on its own', async () => {
  const card = { ok: true, code: '0123456789ab', flags: 1, card: 'XENIA  Dispute x\n=== records/endorsement/E.md ===\nbody', expires_secs: 900 };
  const r = ctx((m) => (m === 'xenia.attest.prepare' ? card : {}));
  await runSlash('/xenia dispute 01ABC note="wrong window"', r.c);
  assert.deepEqual(r.calls.map((c) => c[0]), ['xenia.attest.prepare'], 'showing the card is all that happened');
  assert.deepEqual(r.calls[0][1], { mission_id: 'm1', action: 'dispute', target: '01ABC', options: { note: 'wrong window' } });
  assert.match(r.out[0]!, /^XENIA {2}Dispute x/);
  assert.match(r.out[0]!, /1 flag\(s\) above do not block/);
  assert.match(r.out[0]!, /\/xenia sign 0123456789ab flags to sign exactly this, or \/xenia cancel 0123456789ab/);
});

test('/xenia sign and cancel send the typed code, and say what came of it', async () => {
  const r = ctx(() => ({ ok: true, action: 'dispute', commit: 'abcdef0123456789', files: ['a', 'b'] }));
  await runSlash('/xenia sign 0123456789ab flags', r.c);
  assert.deepEqual(r.calls[0], ['xenia.attest.decide', { mission_id: 'm1', code: '0123456789ab', decision: 'sign', flags_acknowledged: true }]);
  assert.match(r.out[0]!, /dispute: 2 file\(s\) in commit abcdef012345/);
  const c = ctx(() => ({ ok: false, reason: 'denied', message: 'cancelled: nothing was signed or sent' }));
  await runSlash('/xenia cancel 0123456789ab', c.c);
  assert.deepEqual(c.calls[0][1], { mission_id: 'm1', code: '0123456789ab', decision: 'cancel' });
  assert.match(c.out[0]!, /^cancelled: cancelled: nothing was signed/);
  const bad = ctx(() => ({}));
  await runSlash('/xenia sign', bad.c);
  assert.equal(bad.calls.length, 0, 'a missing code asks nothing of the daemon');
  assert.match(bad.out[0]!, /needs the 12-character code/);
});

test('/xenia status words each store, or says none is joined; daemon text cannot move the cursor', async () => {
  assert.match(statusText({ stores: [] }), /no Xenia store is joined/);
  assert.match(statusText({}), /no Xenia store is joined/);
  assert.ok(!statusText({ stores: [{ text: 'Xenia store acme/main\x1b[2J ok' }] }).includes('\x1b'));
  assert.ok(!cardText({ ok: true, code: 'c', card: 'a\x1b[31mb' }).includes('\x1b'));
  assert.match(cardText({ ok: false, message: 'outside your areas' }), /^nothing to sign: outside your areas/);
  assert.match(resultText({ ok: false, reason: 'scan-blocked', message: 'a secret' }), /^not published: a secret/);
  // A refusal carries the catalog's what-to-do line, and daemon text cannot move the cursor there either.
  const helped = resultText({ ok: false, reason: 'scan-blocked', message: 'a secret', help: 'what to do: remove it.\x1b[2J (ask: you)' });
  assert.match(helped, /^not published: a secret\n  what to do: remove it\./);
  assert.ok(!helped.includes('\x1b'));
  const r = ctx(() => ({ stores: [{ text: 'Xenia store acme/main' }] }));
  await runSlash('/xenia', r.c);
  assert.equal(r.calls[0][0], 'xenia.status');
  assert.equal(r.out[0], 'Xenia store acme/main');
});
