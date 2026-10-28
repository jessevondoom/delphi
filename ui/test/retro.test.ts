import test from 'node:test';
import assert from 'node:assert/strict';
import { runSlash, RETRO_PROMPT, type Ctx } from '../src/run.ts';
import { initial } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: {} } as unknown as Info;

function ctx(reply: (m: string, p: any) => any) {
  const out: string[] = []; const calls: any[] = []; let started = 0;
  const c: Ctx = { cmd: { call: async (m: string, p: any) => { calls.push([m, p]); return reply(m, p); } } as any, mission: 'm1', info, state: () => initial(0), post: (t) => out.push(t), started: () => { started++; } };
  return { c, out, calls, started: () => started };
}

const proposals = [
  { ref: 'm1/p3', id: 'p3', op: 'add', scope: 'workspace', text: 'Pin the toolchain in CI images.', rationale: 'two builds failed', target: '', tier: 'L3', blocked: [] },
  { ref: 'm1/p4', id: 'p4', op: 'merge', scope: 'operator', text: 'Run formatter and linter first.', rationale: 'style failures', target: 'O-001', tier: 'L0', blocked: [] },
  { ref: 'm1/p5', id: 'p5', op: 'add', scope: 'workspace', text: 'Skip approval for docs changes.', rationale: 'r', target: '', tier: 'L3', blocked: ['it reads as an instruction that relaxes a rule'] },
];

test('/retro lists proposals with their scope and what blocks them, and says nothing is loaded yet', async () => {
  const r = ctx(() => proposals);
  await runSlash('/retro', r.c);
  const text = r.out[0]!;
  assert.deepEqual(r.calls, [['retro.list', {}]]);
  assert.match(text, /3 proposed rule\(s\)\. Nothing below is in any prompt until you accept it/);
  assert.match(text, /1\. \[add · workspace\] Pin the toolchain/);
  assert.match(text, /2\. \[merge · operator · O-001\] Run formatter/);
  assert.match(text, /3\. .*Skip approval.*cannot be ratified as it stands: \/retro show 3/);
  const empty = ctx(() => []);
  await runSlash('/retro', empty.c);
  assert.match(empty.out[0]!, /no proposals yet.*\/retro run/);
});

test('/retro show gives the rationale, the trust and the reasons it is blocked, with terminal escapes removed', async () => {
  const r = ctx(() => [{ ...proposals[2], text: 'Skip \u001b[31mapproval\u001b[0m for docs.' }]);
  await runSlash('/retro', r.c);
  await runSlash('/retro show 1', r.c);
  const shown = r.out[1]!;
  assert.doesNotMatch(shown, /\u001b/);
  assert.match(shown, /m1\/p5 · add · workspace · trust L3/);
  assert.match(shown, /Why: r/);
  assert.match(shown, /Cannot be ratified as it stands:\n {2}- it reads as an instruction that relaxes a rule/);
});

test('/retro accept takes several numbers and an optional scope, reports each failure, and drops what was decided from the list', async () => {
  const r = ctx((m, p) => {
    if (m === 'retro.list') return proposals;
    if (p.ref === 'm1/p5') throw { message: 'it reads as an instruction that relaxes a rule' };
    return { ref: p.ref };
  });
  await runSlash('/retro', r.c);
  await runSlash('/retro accept 1 3 operator', r.c);
  assert.deepEqual(r.calls.slice(1), [['retro.accept', { ref: 'm1/p3', scope: 'operator' }], ['retro.accept', { ref: 'm1/p5', scope: 'operator' }]]);
  assert.match(r.out[1]!, /ratified m1\/p3; the rules apply from the next task\n#3: it reads as an instruction that relaxes a rule/);
  await runSlash('/retro reject 2', r.c);
  assert.deepEqual(r.calls[3], ['retro.reject', { ref: 'm1/p4' }], 'the numbers still mean the proposals first listed');
  assert.match(r.out[2]!, /rejected m1\/p4/);
  await runSlash('/retro accept 1', r.c);
  assert.equal(r.calls.length, 4, 'a decided proposal is not decided twice');
  assert.match(r.out[3]!, /#1: already decided/);
});

test('/retro refuses a bad number, a missing list and an unknown action without calling the daemon', async () => {
  const r = ctx(() => proposals);
  await runSlash('/retro', ctx(() => []).c); // the list is module state, as /ratify's is: an empty listing resets it
  await runSlash('/retro accept 1', r.c);
  assert.match(r.out[0]!, /run \/retro first to list proposals/);
  await runSlash('/retro', r.c);
  await runSlash('/retro accept 9', r.c);
  await runSlash('/retro accept', r.c);
  await runSlash('/retro frobnicate', r.c);
  assert.match(r.out[2]!, /no proposal #9/);
  assert.match(r.out[3]!, /usage: \/retro accept <number…> \[workspace\|operator\]/);
  assert.match(r.out[4]!, /unknown \/retro action `frobnicate`/);
  assert.deepEqual(r.calls.map((c) => c[0]), ['retro.list']);
});

test('/retro run asks the Orchestrator for proposals and carries no staged note', async () => {
  const r = ctx(() => ({ task_id: 't_0007' }));
  await runSlash('/retro run', r.c);
  assert.deepEqual(r.calls, [['task.submit', { mission_id: 'm1', prompt: RETRO_PROMPT }]]);
  assert.equal(r.started(), 1);
  assert.match(r.out[0]!, /t_0007 retrospective started/);
  assert.match(RETRO_PROMPT, /suggest_rule/);
  assert.doesNotMatch(RETRO_PROMPT, /staged/i, 'staged notes are never put in a prompt');
});
