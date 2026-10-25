import test from 'node:test';
import assert from 'node:assert/strict';
import { parseSpan, parseTimer, renderTimers, runSlash, type Ctx } from '../src/run.ts';
import { initial } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

const roles = { orchestrator: { name: 'athena' }, architect: { name: 'daedalus' } } as unknown as Info['roles'];
const info = { roles } as unknown as Info;

function ctx(reply: (m: string, p: any) => any = () => ({})) {
  const out: string[] = [];
  const calls: any[] = [];
  const c: Ctx = { cmd: { call: async (m: string, p: any) => { calls.push([m, p]); return reply(m, p); } } as any, mission: 'm1', info, state: () => initial(0), post: (t) => out.push(t) };
  return { c, out, calls };
}

test('a time span is seconds, minutes, hours or days; nothing else is one', () => {
  assert.deepEqual(['90', '90s', '20m', '2h', '1d'].map(parseSpan), [90, 90, 1200, 7200, 86400]);
  for (const bad of ['', '0', 'm', '1w', '-5m', '1.5h', 'soon']) assert.equal(parseSpan(bad), null, bad);
});

test('/timer takes a span and a note and wakes the orchestrator; @role, in, at and --every change that', () => {
  assert.deepEqual(parseTimer(['20m', 'check', 'the', 'CI'], roles), { target: 'orchestrator', note: 'check the CI', in_secs: 1200 });
  assert.deepEqual(parseTimer(['@daedalus', 'in', '1h', 'review', 'the', 'draft'], roles), { target: 'architect', note: 'review the draft', in_secs: 3600 });
  assert.deepEqual(parseTimer(['@architect', '5m', 'poll', '--every', '10m'], roles), { target: 'architect', note: 'poll', in_secs: 300, every_secs: 600 });
  assert.deepEqual(parseTimer(['at', '2026-10-26T09:00:00Z', 'standup'], roles), { target: 'orchestrator', note: 'standup', at: '2026-10-26T09:00:00Z' });
});

test('/timer says what is missing instead of guessing', () => {
  assert.match(String(parseTimer(['soon', 'check'], roles)), /`soon` is not a time span/);
  assert.match(String(parseTimer(['20m'], roles)), /needs a note/);
  assert.match(String(parseTimer(['at'], roles)), /at needs a time/);
  assert.match(String(parseTimer(['10m', 'x', '--every'], roles)), /--every needs a time span/);
  assert.match(String(parseTimer([], roles)), /^\/timer <when> <note>/);
});

test('/timer sets, lists and cancels through the daemon and shows its answer', async () => {
  const reply = (m: string, p: any) => m === 'timer.set' ? { id: 'tm_0004', target: p.target, due_at: '2026-10-25T09:00:00Z', every_secs: p.every_secs }
    : m === 'timer.cancel' ? { id: p.id }
    : { now: '2026-10-25T08:00:00Z', timers: [
      { id: 'tm_0004', target: 'orchestrator', state: 'pending', relative: 'in 59m 59s', due_at: '2026-10-25T09:00:00Z', creator: 'operator', note: 'check\nCI', every_secs: 3600 },
      { id: 'tm_0003', target: 'architect', state: 'cancelled', cancelled_by: 'operator', creator: 'operator', note: 'old' }] };
  const set = ctx(reply);
  await runSlash('/timer 1h --every 1h check CI', set.c);
  assert.deepEqual(set.calls, [['timer.set', { mission_id: 'm1', target: 'orchestrator', note: 'check CI', in_secs: 3600, every_secs: 3600 }]]);
  assert.match(set.out[0]!, /^tm_0004 set: wakes orchestrator at 2026-10-25T09:00:00Z, then every 1h\. Timers fire only while the daemon is running/);
  const list = ctx(reply);
  await runSlash('/timer', list.c);
  assert.match(list.out[0]!, /^1 pending timer\(s\) · now 2026-10-25T08:00:00Z\n  tm_0004 .*in 59m 59s .*every 1h  by operator: check CI\nrecent:\n  tm_0003 .*cancelled by operator/);
  const cancel = ctx(reply);
  await runSlash('/timer cancel tm_0004', cancel.c);
  assert.deepEqual(cancel.calls, [['timer.cancel', { mission_id: 'm1', id: 'tm_0004' }]]);
  assert.equal(cancel.out[0], 'tm_0004 cancelled');
  const bare = ctx();
  await runSlash('/timer cancel', bare.c);
  assert.deepEqual(bare.calls, []);
  assert.match(bare.out[0]!, /needs an id/);
});

test('a refusal from the daemon (a time in the past, a cap) is shown as it is', async () => {
  const r = ctx(() => { throw { code: -32602, message: '2020-01-01T00:00:00Z has already passed; it is now 2026-10-25T08:00:00Z' }; });
  await runSlash('/timer at 2020-01-01T00:00:00Z x', r.c);
  assert.match(r.out[0]!, /already passed; it is now/);
  assert.equal(renderTimers({ timers: [] }), '0 pending timer(s) · now ?');
});
