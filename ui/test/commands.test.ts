import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { COMMANDS } from '../src/commands.ts';
import { runSlash, type Ctx } from '../src/run.ts';
import { renderList } from '../src/decisions.ts';
import { initial } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

// The two consoles answer the same commands because they read one list: `src/commands.json`. This checks the list
// itself and that this console really handles what the list promises it handles.
import raw from '../src/commands.json' with { type: 'json' };
import { lookup } from '../src/commands.ts';

type Entry = { name: string; desc: string; ink: boolean; rust: boolean; local: boolean; aliases?: string[]; args?: string };
const LIST = raw as Entry[];

test('the shared list is well formed, with only /more left to the Rust consoles', () => {
  const names = LIST.flatMap((c) => [c.name, ...(c.aliases ?? [])]);
  assert.equal(new Set(names).size, names.length, 'no name or alias twice');
  for (const c of LIST) { assert.match(c.name, /^\/[a-z]+$/); assert.ok(c.desc.length > 8, c.name); assert.ok(c.ink || c.rust, `${c.name} is on some console`); }
  assert.deepEqual(LIST.filter((c) => c.rust && !c.ink).map((c) => c.name), ['/more'], 'the only command this console lacks');
  assert.deepEqual(COMMANDS.map((c) => c.name), LIST.filter((c) => c.ink).map((c) => c.name).sort((a, b) => a.localeCompare(b)), 'the menu is the list, alphabetical');
});

test('every command of this console is handled: a daemon call in run.ts, or a local one in app.tsx; aliases resolve', async () => {
  const app = readFileSync(join(import.meta.dirname, '../src/app.tsx'), 'utf8');
  for (const c of LIST.filter((x) => x.ink)) {
    if (c.local) { assert.ok(app.includes(`'${c.name}'`), `${c.name} is local but app.tsx does not handle it`); continue; }
    const r = ctx(() => ({}));
    await runSlash(c.name, r.c);
    assert.ok(!r.out.some((t) => t.startsWith('unknown command')), `${c.name}: ${r.out.join(' | ')}`);
  }
  for (const c of LIST.filter((x) => x.ink)) for (const a of c.aliases ?? []) assert.equal(lookup(a)?.name, c.name, `${a} means ${c.name}`);
  assert.equal(lookup('/nope'), undefined);
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

test('the slash menu lists commands alphabetically', () => {
  const names = COMMANDS.map((c) => c.name);
  assert.deepEqual(names, [...names].sort((a, b) => a.localeCompare(b)));
});

test('/observe opens a new window on every call, with the view', async () => {
  const opened: string[][] = [];
  delete process.env.TMUX;
  const { c, out } = ctx(() => ({}));
  const full = { ...c, info: { roles: {}, delphi_exe: '/bin/delphi', workspace_name: 'w', home: '/h' } as unknown as Info, open: async (a: string[]) => { opened.push(a); return null; } };
  await runSlash('/observe council', full);
  await runSlash('/observe council', full);
  assert.equal(opened.length, 2);
  assert.deepEqual(opened[0], ['sh', '-c', '/bin/delphi observe council --workspace w --home /h']);
  assert.match(out[0]!, /new terminal window/);
  await runSlash('/observe nonsense', full);
  assert.equal(opened.length, 2);
  await runSlash('/observe', { ...full, open: async () => 'no terminal' });
  assert.match(out.at(-1)!, /could not open a terminal window \(no terminal\)/);
});

test('windowCommand: Terminal.app by default, iTerm when that is the terminal', async () => {
  const { windowCommand } = await import('../src/window.ts');
  assert.ok(windowCommand(['a b', "c'd"], 'darwin', {})!.join('\n').includes('tell application "Terminal"'));
  assert.ok(windowCommand(['x'], 'darwin', { TERM_PROGRAM: 'iTerm.app' })!.join('\n').includes('create window with default profile'));
  assert.equal(windowCommand(['x'], 'win32', {}), null);
});

test('/models prints the daemon\'s pool text; /agents lists what is running', async () => {
  const m = ctx((method) => (method === 'models.get' ? { text: 'claude  usable\n' } : {}));
  await runSlash('/models', m.c);
  assert.equal(m.out[0], 'claude  usable');
  assert.deepEqual(m.calls[0], ['models.get', { mission_id: 'm1' }]);
  await runSlash('/agents', m.c);
  assert.equal(m.out[1], 'Nothing is running.');
});
