// Spec 104 §13 G4: `/mcp` shows the standing consent, and the footer's `learn:` line and `/retro` show the host notes waiting.
// Every number comes from the daemon's answer (`mcp.get`, `learn.health`); nothing here computes or moves.
import test from 'node:test';
import assert from 'node:assert/strict';
import { initial } from '../src/fold.ts';
import { runSlash, type Ctx } from '../src/run.ts';
import { footer, standingLines } from '../src/screen.ts';
import type { Info } from '../src/theme.ts';
import { info as worldInfo, snapshot } from './helpers.ts';
import * as C from '../src/composer.ts';
import { newInbox } from '../src/inbox.ts';
import type { Model } from '../src/screen.ts';

const info = { roles: {}, workspace: '/tmp/ws', cwd: '/tmp/proj' } as unknown as Info;

function ctx(reply: (m: string, p: any) => any) {
  const out: string[] = [];
  const c = { cmd: { call: async (m: string, p: any) => reply(m, p) } as any, mission: 'm1', info, state: () => initial(0), post: (t: string) => out.push(t) } as Ctx;
  return { c, out };
}
const mcp = async (standing: any) => { const x = ctx(() => ({ enabled: true, host_lessons: 1, standing_consent: standing })); await runSlash('/mcp', x.c); return x.out[0]!.split('\n'); };
const retro = async (health: any, proposals: any[] = []) => { const x = ctx((m) => (m === 'learn.health' ? health : proposals)); await runSlash('/retro', x.c); return x.out[0]!.split('\n'); };
const golden = (name: string, lines: string[]) => { const g = snapshot(name, lines); assert.ok(g.ok, g.message); };

test('/mcp with the standing amount at zero says plainly that every foreign seat asks first', async () => {
  const lines = await mcp({ standing_foreign_tokens: 0, used_24h: 0, settled_24h: 0, reserved: 0 });
  assert.match(lines[1]!, /standing consent: 0 tokens, so every foreign seat asks you first/);
  golden('mcp-standing-zero', lines);
});

test('/mcp with an amount shows what seats used in 24 hours and what running seats have reserved', async () => {
  const lines = await mcp({ standing_foreign_tokens: 200000, used_24h: 54000, settled_24h: 24000, reserved: 30000 });
  assert.match(lines[1]!, /standing consent: 200000 tokens in 24 hours .* used in the last 24 hours: 24000 · reserved by seats still running: 30000/);
  golden('mcp-standing-used', lines);
  // An older daemon without the split falls back to its one sum; one without the block says nothing about it.
  assert.match((await mcp({ standing_foreign_tokens: 100, used_24h: 7 }))[1]!, /used in the last 24 hours: 7 · reserved by seats still running: 0/);
  assert.deepEqual(standingLines(undefined), []);
  assert.equal((await mcp(undefined)).length, 1);
});

const health = (n: number) => ({ staged: n + 1, review: 0, host_notes: n, keep: 'delphi learn keep <mission>/<id> --as lesson --role <role> | --as rule' });
const proposal = { ref: 'm1/p3', id: 'p3', op: 'add', scope: 'workspace', text: 'Pin the toolchain in CI images.', rationale: 'r', target: '', tier: 'L3', blocked: [] };

test('/retro with no host notes is the line it was; with some it ends with the count and the keep command', async () => {
  const none = await retro(health(0), [proposal]);
  assert.equal(none.length, 3, 'unchanged: heading, proposal, hint');
  assert.doesNotMatch(none.join('\n'), /host note/);
  golden('retro-host-notes-zero', none);
  assert.deepEqual(await retro(health(0)), ['no proposals yet. The Orchestrator proposes rules after a mission; /retro run asks it to look at this one now.']);
  const some = await retro(health(2), [proposal]);
  assert.equal(some.at(-1), '2 host notes waiting: delphi learn keep <mission>/<id> --as lesson --role <role> | --as rule');
  golden('retro-host-notes-some', some);
  assert.match((await retro(health(1)))[1]!, /^1 host note waiting: delphi learn keep/);
  // A daemon that cannot answer learn.health leaves the list as it was.
  const x = ctx((m) => { if (m === 'learn.health') throw new Error('no'); return [proposal]; });
  await runSlash('/retro', x.c);
  assert.equal(x.out[0]!.split('\n').length, 3);
});

test('the footer learn line: unchanged at zero host notes, and the count with the command when some wait', () => {
  const scoped = { ...worldInfo, workspace_name: 'acme', mission: 'm-7' };
  const base: Model = { info: scoped, pal: { dark: true, bg: [15, 23, 42] }, state: initial(0), now: Date.parse('2026-11-05T12:00:00Z'), deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null, queue: [], inbox: newInbox(0), mode: 'default', panel: null, help: false, still: true };
  const at = (hostNotes: number | undefined) => footer({ ...base, learn: { staged: 3, review: 1, ...(hostNotes === undefined ? {} : { hostNotes }) } }, 130);
  assert.equal(at(0), at(undefined), 'zero host notes: the line is what it was');
  assert.doesNotMatch(at(0), /host note/);
  const some = at(2);
  assert.match(some.replace(/\x1b\[[0-9;]*m/g, ''), /learn: 3 staged · 1 to review · 2 host notes waiting \(delphi learn keep\)/);
  golden('footer-learn-host-notes-130', [some]);
  golden('footer-learn-host-notes-none-130', [at(0)]);
});
