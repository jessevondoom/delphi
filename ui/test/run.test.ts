import test from 'node:test';
import assert from 'node:assert/strict';
import { parseCouncil, renderUsage, runSlash, type Ctx } from '../src/run.ts';
import { fold, initial, replay } from '../src/fold.ts';
import { highlightLine } from '../src/highlight.ts';
import type { Info } from '../src/theme.ts';
import chalk from 'chalk';
chalk.level = 3; // colour even when the tests run without a terminal

const info = { roles: {} } as unknown as Info;
const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

function ctx(state = initial(0), calls: any[] = [], reply: (m: string, p: any) => any = () => ({})) {
  const out: string[] = [];
  const c: Ctx = { cmd: { call: async (m: string, p: any) => { calls.push([m, p]); return reply(m, p); } } as any, mission: 'm1', info, state: () => state, post: (t) => out.push(t) };
  return { c, out, calls };
}
const ev = (type: string, payload: any, timestamp?: string) => ({ method: 'event', params: { type, payload, timestamp } });

test('/council parses seats, author and the proposition', () => {
  assert.deepEqual(parseCouncil('--seats architect,red-team --author claude Should we?'), { proposition: 'Should we?', seats: ['architect', 'red-team'], author: 'claude' });
  assert.match(String(parseCouncil('')), /needs a proposition/);
});

test('a card is answered with its payload hash, once it has been on screen for 300 ms', async () => {
  const s = fold(initial(0), ev('approval.requested', { approval_id: 'a1', role: 'builder', tier: 'amber', payload_blake3: 'h1', payload: { preview: 'x' }, summary: 'write a file', timeout_secs: 60 }), 1000);
  const early = ctx(s);
  await runSlash('/y', early.c, 1100);
  assert.deepEqual(early.calls, [], 'too soon: nothing is sent');
  assert.match(early.out[0]!, /only just appeared/);
  const ok = ctx(s);
  await runSlash('/y', ok.c, 1400);
  assert.deepEqual(ok.calls, [['approval.resolve', { approval_id: 'a1', payload_blake3: 'h1', decision: 'allow' }]]);
});

test('allow-similar is refused for a red card and nothing is sent', async () => {
  const s = fold(initial(0), ev('approval.requested', { approval_id: 'a2', role: 'builder', tier: 'red', payload_blake3: 'h', payload: {}, timeout_secs: 60 }), 0);
  const r = ctx(s);
  await runSlash('/a', r.c, 5000);
  assert.deepEqual(r.calls, []);
  assert.match(r.out[0]!, /never applies to a Red action/);
});

test('/ratify lists, then applies exactly the version that was listed', async () => {
  const r = ctx(initial(0), [], (m) => (m === 'ratify.list' ? [{ id: 'cfg', kind: 'config-access', summary: 'allow x', live_blake3: 'L1' }] : {}));
  await runSlash('/ratify', r.c);
  assert.match(r.out[0]!, /1 pending proposal.*\n.*\[config-access\] \[RED\] allow x/);
  await runSlash('/ratify apply 1', r.c);
  assert.deepEqual(r.calls.at(-1), ['ratify.apply', { id: 'cfg', expected_blake3: 'L1' }]);
  assert.equal(r.out.at(-1), 'ratified cfg');
});

test('usage renders a table with a total', () => {
  const t = renderUsage({ mission_id: 'm1', rows: [{ key: 'builder', calls: 2, input_tokens: 10, output_tokens: 5, cost_usd: 0.5 }], budget: { cap_usd: 5, reserved_usd: 0, available_usd: 4.5 } });
  assert.match(t, /builder\s+2\s+10\s+0\s+5\s+\$0\.5000/);
  assert.match(t, /total \$0\.5000 of \$5\.00 ceiling/);
});

test('history replays prompts and replies with their own times, and skips what was only pending', () => {
  const s = replay(initial(0), [
    { type: 'task.created', timestamp: '2026-10-24T09:00:00Z', payload: { prompt: 'q' } },
    { type: 'dag.node.added', payload: { node_id: 'n_1', role: 'architect', depth: 0 } },
    { type: 'approval.requested', payload: { approval_id: 'old', role: 'builder' } },
    { type: 'task.output', timestamp: '2026-10-24T09:00:05Z', payload: { text: 'a', status: 'done' } },
  ]);
  assert.deepEqual(s.blocks.map((b) => [b.kind, b.role ?? '', b.at]), [['user', '', Date.parse('2026-10-24T09:00:00Z')], ['reply', 'architect', Date.parse('2026-10-24T09:00:05Z')]]);
  assert.deepEqual(s.approvals, {});
});

test('the highlighter colours strings, comments, keywords and diffs, and never changes the text', () => {
  for (const [line, lang] of [['fn main() { let x = "hi"; // note', 'rust'], ['+added line', 'diff'], ['def f(): return 1', 'py'], ['{"a": [1, true]}', 'json']] as const) {
    const out = highlightLine(line, lang);
    assert.equal(plain(out), line);
    assert.notEqual(out, line, `${lang} gets colour`);
  }
});
