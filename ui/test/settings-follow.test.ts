// `/council --tier`, `/follow` and `/settings` in the full-screen console (`specs/003` §4.2, `specs/035` D-17..D-19), and the chat saying once
// what a setting changed to.
import test from 'node:test';
import assert from 'node:assert/strict';
import { lookup } from '../src/commands.ts';
import { fold, initial } from '../src/fold.ts';
import { parseCouncil, renderSettings, runSlash, type Ctx } from '../src/run.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: {}, workspace: '/tmp/ws' } as unknown as Info;

function ctx(replies: Record<string, any> = {}) {
  const calls: [string, any][] = [], out: string[] = [];
  let started = 0;
  const c: Ctx = {
    cmd: { call: async (m: string, p: any) => { calls.push([m, p]); const r = replies[m]; if (r instanceof Error) throw r; return r ?? {}; } } as any,
    mission: 'm1', info, state: () => initial(0), post: (t) => out.push(t), started: () => { started++; },
  } as Ctx;
  return { c, calls, out, started: () => started };
}

test('/council takes --tier for the top tiers only, and the flag goes to the daemon', async () => {
  assert.deepEqual(parseCouncil('--tier frontier Queue or cron?'), { proposition: 'Queue or cron?', seats: [], author: null, lenses: false, tier: 'frontier' });
  assert.equal((parseCouncil('Queue or cron?') as any).tier, null);
  for (const bad of ['--tier huge Queue or cron?', '--tier', '--tier fast']) {
    const r = parseCouncil(bad);
    if (bad === '--tier fast') assert.equal((r as any).tier, undefined, 'a proposition is still needed'); else assert.match(String(r), /^\/council needs a proposition/);
  }
  const t = ctx({ 'council.start': { task_id: 't_1' } });
  await runSlash('/council --tier frontier --lenses Queue or cron?', t.c);
  assert.deepEqual(t.calls[0], ['council.start', { mission_id: 'm1', proposition: 'Queue or cron?', seats: [], author_family: null, lenses: true, tier: 'frontier' }]);
});

test('/follow starts the steps, takes an option and `again`, and shows what the daemon refuses', async () => {
  const t = ctx({ 'council.follow': { started: 2, option: null } });
  await runSlash('/follow', t.c);
  assert.deepEqual(t.calls[0], ['council.follow', { mission_id: 'm1' }]);
  assert.equal(t.out[0], '(started 2 steps; each starts when the one before it is done)');
  assert.equal(t.started(), 1);
  const o = ctx({ 'council.follow': { started: 1, option: 'DO-NOT-PROCEED' } });
  await runSlash('/follow 2 again', o.c);
  assert.deepEqual(o.calls[0], ['council.follow', { mission_id: 'm1', option: '2', again: true }]);
  assert.match(o.out[0]!, /started 1 step for DO-NOT-PROCEED/);
  const bad = ctx({ 'council.follow': new Error('this Council needs your choice: 1 = A, 2 = B (for example /follow 1)') });
  await runSlash('/follow', bad.c);
  assert.match(bad.out[0]!, /needs your choice: 1 = A, 2 = B/);
});

test('/settings shows what each role starts on and what waits, and its subcommands call the daemon', async () => {
  const get = { roles: [{ role: 'architect', shipped: 'workhorse', override: 'frontier', effective: 'frontier' }, { role: 'scout', shipped: 'fast', override: null, effective: 'fast' }], pending: [{ id: 'sg_2', key: 'role_tier.builder', value: 'frontier', reason: 'raised for 3 tasks', by: 'usage' }], budget: { cap_usd: 5, spent_usd: 0.25 } };
  const t = ctx({ 'settings.get': get });
  await runSlash('/settings', t.c);
  const text = t.out[0]!;
  assert.match(text, /architect +workhorse → frontier +\(yours\)/);
  assert.match(text, /scout +fast\n/);
  assert.match(text, /budget ceiling \$5\.00/);
  assert.match(text, /waiting for your yes:\n +2 +role_tier\.builder → frontier +raised for 3 tasks +\(Delphi noticed\)/);
  assert.match(text, /\/settings accept <n>/);
  assert.match(text, /just ask/);
  for (const [line, want] of [
    ['/settings accept 2', ['settings.accept', { mission_id: 'm1', id: '2' }]],
    ['/settings dismiss 2', ['settings.dismiss', { mission_id: 'm1', id: '2' }]],
    ['/settings reset architect', ['settings.set', { mission_id: 'm1', key: 'role_tier.architect', value: 'default' }]],
    ['/settings set mission_budget_usd 20', ['settings.set', { mission_id: 'm1', key: 'mission_budget_usd', value: '20' }]],
    ['/settings set role_tier.builder fast', ['settings.set', { mission_id: 'm1', key: 'role_tier.builder', value: 'fast' }]],
  ] as const) {
    const x = ctx({ 'settings.dismiss': { dismissed: 'sg_2' } });
    await runSlash(line, x.c);
    assert.deepEqual(x.calls[0], want, line);
  }
  const u = ctx();
  for (const l of ['/settings accept', '/settings reset', '/settings set mission_budget_usd', '/settings nonsense']) await runSlash(l, u.c);
  assert.equal(u.out.length, 4, 'each incomplete command says how to use it');
  assert.ok(u.calls.length === 0 && u.out.every((o) => o.includes('/settings accept <n>')));
  assert.match(renderSettings({ roles: [], pending: [] }), /change one:/);
});

test('the chat says once what a setting changed, in the daemon\'s words, and shows a suggestion with how to answer it', () => {
  let s = fold(initial(0), { method: 'event', params: { type: 'setting.changed', payload: { key: 'role_tier.architect', old: 'workhorse', new: 'frontier', text: 'architect now starts on frontier (it started on workhorse).\x1b[2J' } } }, 1);
  assert.equal(s.blocks[0]!.text, 'architect now starts on frontier (it started on workhorse).');
  s = fold(s, { method: 'event', params: { type: 'setting.suggested', payload: { id: 'sg_3', key: 'role_tier.builder', value: 'frontier', reason: 'the orchestrator has raised builder to frontier for 3 tasks in this mission', by: 'usage' } } }, 2);
  assert.match(s.blocks[1]!.text, /^Delphi noticed a pattern: role_tier\.builder → frontier \(.*3 tasks.*\)\. \/settings accept 3 to make it, \/settings dismiss 3 to decline\.$/);
});

test('the new commands are in the list the console completes from', () => {
  assert.equal(lookup('/follow')?.name, '/follow');
  assert.equal(lookup('/settings')?.name, '/settings');
  assert.match(lookup('/council')!.desc, /--tier/);
});
