// `/mcp` and `/resume-host` (specs/062 S10): the operator's switch for other coding tools, and carrying a host's handoff in.
import test from 'node:test';
import assert from 'node:assert/strict';
import { lookup } from '../src/commands.ts';
import { initial } from '../src/fold.ts';
import { runSlash, type Ctx } from '../src/run.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: {}, workspace: '/tmp/ws', cwd: '/tmp/proj' } as unknown as Info;

function ctx(replies: Record<string, any> = {}) {
  const calls: [string, any][] = [], out: string[] = [];
  const c = { cmd: { call: async (m: string, p: any) => { calls.push([m, p]); return replies[m] ?? {}; } } as any, mission: 'm1', info, state: () => initial(0), post: (t: string) => out.push(t) } as Ctx;
  return { c, calls, out };
}

test('/mcp shows the state with no word and flips the switch with on or off; anything else is usage', async () => {
  assert.ok(lookup('/mcp') && lookup('/resume-host'), 'listed, so they complete and show in /help');
  const show = ctx({ 'mcp.get': { enabled: true, host_lessons: 2 } });
  await runSlash('/mcp', show.c);
  assert.deepEqual(show.calls.at(-1), ['mcp.get', { mission_id: 'm1' }]);
  assert.match(show.out[0]!, /allowed · 2 lessons/);
  const off = ctx({ 'mcp.set': { enabled: false, host_lessons: 0 } });
  await runSlash('/mcp OFF', off.c);
  assert.deepEqual(off.calls.at(-1), ['mcp.set', { mission_id: 'm1', enabled: false }]);
  assert.match(off.out[0]!, /turned off/);
  const bad = ctx();
  await runSlash('/mcp sideways', bad.c);
  assert.equal(bad.calls.length, 0);
  assert.match(bad.out[0]!, /\/mcp \[on \| off\]/);
});

test('/resume-host sends the console\'s folder and says the handoff is context, not instruction', async () => {
  const x = ctx({ 'handoff.resume': { session: 'abcdef0123456789', task_id: 't_0001', role: 'orchestrator' } });
  await runSlash('/resume-host', x.c);
  assert.deepEqual(x.calls.at(-1), ['handoff.resume', { mission_id: 'm1', cwd: '/tmp/proj' }]);
  assert.match(x.out[0]!, /context for the team, not instruction/);
});
