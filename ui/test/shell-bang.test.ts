// The `!` bang (`specs/061` §6): shell.run over the console socket, the usage note, refusals, and the block's states.
process.env.TZ = 'UTC';
import test from 'node:test';
import assert from 'node:assert/strict';
import { frame, SHELL_ROWS, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { initial, type State } from '../src/fold.ts';
import { newInbox } from '../src/inbox.ts';
import { BANG_USAGE, runBang, type ShellResult } from '../src/shell.ts';
import { info, plain } from './helpers.ts';

const model = (state: State, over: Partial<Model> = {}): Model => ({
  info, pal: { dark: true, bg: [15, 23, 42] }, state, now: 1000, deck: 1, composer: C.empty(), back: 0, menu: null, expandTools: false, search: null,
  queue: [], inbox: newInbox(0), mode: 'mission-auto', panel: null, help: false, still: true, ...over,
});

const done = (o: Partial<ShellResult> = {}): ShellResult => ({ ok: true, exit_code: 0, timed_out: false, output: 'a.txt\nb.txt\n', output_capped: false, elapsed_ms: 120, ...o });

function rig(reply: ShellResult | Error | Promise<ShellResult>) {
  const calls: [string, any][] = [], notes: [string, string | undefined][] = [];
  let state = initial(0);
  const io = {
    cmd: { call: async (m: string, p: any) => { calls.push([m, p]); const r = await reply; if (r instanceof Error) throw r; return r; } },
    cwd: '/work/here', update: (f: (s: State) => State) => { state = f(state); }, note: (t: string, tone?: 'info' | 'error' | 'ok') => notes.push([t, tone]), now: () => 5,
  };
  return { io, calls, notes, state: () => state };
}

const rows = (s: State, w: number, h = 30) => frame(model(s), w, h).map(plain);

test('`!ls -l` calls shell.run with the command stripped and the cwd, and shows a block', async () => {
  const r = rig(done());
  await runBang('!  ls -l ', r.io);
  assert.deepEqual(r.calls, [['shell.run', { command: 'ls -l', cwd: '/work/here' }]]);
  assert.deepEqual(r.calls.map((c) => c[0]), ['shell.run'], 'no task.submit');
  const b = r.state().blocks[0]!;
  assert.equal(b.kind, 'shell');
  assert.equal(b.status, 'ok');
  assert.equal(b.text, 'ls -l');
});

test('a bare `!` shows the usage note and calls nothing', async () => {
  for (const line of ['!', '!   ']) {
    const r = rig(done());
    await runBang(line, r.io);
    assert.equal(r.calls.length, 0);
    assert.deepEqual(r.notes, [[BANG_USAGE, undefined]]);
    assert.equal(r.state().blocks.length, 0);
  }
});

test('`!!x` is not special: the command is `!x`', async () => {
  const r = rig(done());
  await runBang('!!x', r.io);
  assert.equal((r.calls[0]![1] as any).command, '!x');
});

test('a refusal is an error note and leaves no block; an RPC error likewise', async () => {
  const a = rig(done({ ok: false, exit_code: null, refused: 'denied by [exec] deny: rm -rf *' }));
  await runBang('!rm -rf x', a.io);
  assert.deepEqual(a.notes, [['refused: denied by [exec] deny: rm -rf *', 'error']]);
  assert.equal(a.state().blocks.length, 0);
  const b = rig(Object.assign(new Error('method not found'), {}));
  await runBang('!ls', b.io);
  assert.deepEqual(b.notes, [['could not run: method not found', 'error']]);
  assert.equal(b.state().blocks.length, 0);
});

test('while in flight the block is running; the spinner turns with time and stands still when motion is reduced', async () => {
  let release!: (r: ShellResult) => void;
  const r = rig(new Promise<ShellResult>((ok) => { release = ok; }));
  const p = runBang('!sleep 5', r.io);
  const running = r.state();
  assert.equal(running.blocks[0]!.status, 'running');
  const at = (now: number, still: boolean) => frame(model(running, { now, still }), 60, 12).map(plain).join('\n');
  assert.match(at(0, true), /! sleep 5\n.*running…/);
  assert.notEqual(at(0, false), at(250, false), 'the spinner moves');
  assert.equal(at(0, true), at(250, true), 'and stands still when motion is reduced');
  release(done({ output: '' }));
  await p;
  assert.equal(r.state().blocks[0]!.status, 'ok');
});

test('the block renders ok, failed, timed out and capped, at two widths', async () => {
  const cases: [Partial<ShellResult>, RegExp[]][] = [
    [{}, [/! ls/, /a\.txt/, /exit 0 · 0\.1s/]],
    [{ ok: false, exit_code: 2, output: 'no such file', elapsed_ms: 2500 }, [/no such file/, /exit 2 · 2\.5s/]],
    [{ ok: false, exit_code: null, timed_out: true, output: 'partial', elapsed_ms: 30000 }, [/partial/, /timed out · 30\.0s/]],
    [{ output_capped: true }, [/exit 0 · output capped · 0\.1s/]],
  ];
  for (const w of [50, 100]) {
    for (const [res, want] of cases) {
      const r = rig(done(res));
      await runBang('!ls', r.io);
      const text = rows(r.state(), w).join('\n');
      for (const re of want) assert.match(text, re, `${w}: ${re}`);
      for (const l of rows(r.state(), w)) assert.ok(l.length <= w, `fits ${w}`);
    }
  }
});

test('a long command is cut to the width, and a long output is clipped with a tail', async () => {
  const many = Array.from({ length: SHELL_ROWS + 37 }, (_, i) => `line ${i}`).join('\n');
  const r = rig(done({ output: many }));
  await runBang('!' + 'x'.repeat(300), r.io);
  const all = frame(model(r.state()), 60, SHELL_ROWS + 40).map(plain);
  assert.ok(all.some((l) => /! x+…\s*$/.test(l)), 'header clipped');
  assert.ok(all.some((l) => l.includes('… 37 more lines')));
  assert.ok(all.some((l) => l.includes(`line ${SHELL_ROWS - 1}`)));
  assert.ok(!all.some((l) => l.includes(`line ${SHELL_ROWS}`)));
});

test('terminal escapes in output are not passed through', async () => {
  const r = rig(done({ output: 'hi\x1b]0;title\x07\x1b[31mred\x1b[0m\n' }));
  await runBang('!echo', r.io);
  const raw = frame(model(r.state()), 60, 12).join('\n');
  assert.ok(!raw.includes('title'));
  assert.ok(!raw.includes('\x1b[31mred'));
});
