import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { COMMANDS } from '../src/commands.ts';
import { runSlash, type Ctx } from '../src/run.ts';
import { renderList } from '../src/decisions.ts';
import { initial } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

// The two consoles are meant to answer the same commands. Rust-only commands the full-screen console has not
// gained yet (005f slice 6) are named here; anything else in the Rust HELP missing from COMMANDS fails the test.
const NOT_YET = new Set(['/models', '/agents', '/more', '/narrator']);

test('every command in the Rust console help is in the full-screen console, or is listed as not yet', () => {
  const src = readFileSync(join(import.meta.dirname, '../../crates/delphi/src/chat.rs'), 'utf8');
  const help = src.slice(src.indexOf('pub const HELP'), src.indexOf('/quit";') + 7);
  const names = new Set([...help.matchAll(/^ {2}(\/[a-z]+)/gm)].map((m) => m[1]!));
  assert.ok(names.size > 10, 'found the help block');
  const have = new Set(COMMANDS.map((c) => c.name));
  const missing = [...names].filter((n) => !have.has(n) && !NOT_YET.has(n));
  assert.deepEqual(missing, []);
  for (const n of NOT_YET) assert.ok(!have.has(n), `${n} now exists: remove it from NOT_YET`);
});

const info = { roles: {} } as unknown as Info;
function ctx(reply: (m: string, p: any) => any) {
  const out: string[] = []; const calls: any[] = [];
  const c: Ctx = { cmd: { call: async (m: string, p: any) => { calls.push([m, p]); return reply(m, p); } } as any, mission: 'm1', info, state: () => initial(0), post: (t) => out.push(t) };
  return { c, out, calls };
}
const ledger = {
  pending: 2,
  decisions: [
    { decision_id: 'dec_a', task_id: 't_1', outcome: 'allow', risk: 'routine', summary: 'edit a.rs', rationale: 'small', payload_blake3: 'ha', review: null },
    { decision_id: 'dec_b', task_id: 't_1', outcome: 'allow', risk: 'elevated', summary: 'edit Cargo.toml', rationale: 'dep', payload_blake3: 'hb', second: 'unseconded', review: null },
  ],
};

test('/decisions lists what nobody second-checked first and says what to do', async () => {
  const r = ctx(() => ledger);
  await runSlash('/decisions', r.c);
  const lines = r.out[0]!.split('\n');
  assert.match(lines[0]!, /2 provisional decision\(s\), 2 awaiting/);
  assert.match(lines[1]!, /dec_b/, 'unseconded elevated first');
  assert.match(r.out[0]!, /review with: \/decisions show/);
  assert.match(renderList({ decisions: [] }), /no provisional decisions/);
});

test('/decisions ratify sends the hash that was shown; all-routine leaves elevated ones', async () => {
  const r = ctx((m) => (m === 'decisions.list' ? ledger : {}));
  await runSlash('/decisions ratify all-routine', r.c);
  const reviews = r.calls.filter((c) => c[0] === 'decisions.review');
  assert.deepEqual(reviews.map((c) => c[1].decision_id), ['dec_a']);
  assert.equal(reviews[0][1].payload_blake3, 'ha');
  assert.equal(reviews[0][1].resolution, 'ratified');
  assert.match(r.out[0]!, /dec_a: ratified/);
});

test('/decisions reject carries the reason; /merge needs a task id and asks the daemon', async () => {
  const r = ctx((m) => (m === 'decisions.list' ? ledger : { task_id: 't_1' }));
  await runSlash('/decisions reject dec_b too broad', r.c);
  assert.equal(r.calls.at(-1)[1].note, 'too broad');
  const m = ctx(() => ({ task_id: 't_1' }));
  await runSlash('/merge', m.c);
  assert.match(m.out[0]!, /needs the build's task id/);
  await runSlash('/merge t_1', m.c);
  assert.deepEqual(m.calls[0], ['build.merge', { mission_id: 'm1', task_id: 't_1' }]);
});
