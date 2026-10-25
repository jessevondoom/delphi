// `/observe` in the full-screen console behaves as it does in the Rust consoles (`005e` finding 7, slice 6): one narrator window.
import test from 'node:test';
import assert from 'node:assert/strict';
import { narratorCommand, openNarrator } from '../src/narrator.ts';
import { runSlash, type Ctx } from '../src/run.ts';
import { initial } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: {}, delphi_exe: '/opt/delphi/bin/delphi', workspace_name: 'app', home: '/srv/state' } as unknown as Info;

test('each view is the command that opens it; no view is the window\'s first', () => {
  assert.equal(narratorCommand('epic', info), '/opt/delphi/bin/delphi observe epic --workspace app --home /srv/state');
  assert.equal(narratorCommand(undefined, info), '/opt/delphi/bin/delphi observe --workspace app --home /srv/state');
});

test('a path with a space is quoted, and a view that does not exist is refused with the list', () => {
  const spaced = { ...info, home: "/srv/some one/it's" } as Info;
  assert.ok(narratorCommand('epic', spaced).endsWith(`--home '/srv/some one/it'\\''s'`));
  assert.equal(narratorCommand('nope', info), '/observe takes council, epic, forge, agents or archives, not nope');
  assert.match(narratorCommand('tile', info), /^\/observe takes/, 'the four-pane tile is gone');
});

test('outside tmux it opens one new terminal window, every call', async () => {
  const opened: string[][] = [];
  const open = async (a: string[]) => { opened.push(a); return null; };
  const noTmux = () => { throw new Error('tmux must not be called'); };
  assert.equal(await openNarrator('forge', info, {}, noTmux, open), '(the narrator is open in a new terminal window)');
  await openNarrator('forge', info, {}, noTmux, open);
  assert.equal(opened.length, 2);
  assert.deepEqual(opened[0], ['sh', '-c', '/opt/delphi/bin/delphi observe forge --workspace app --home /srv/state']);
});

test('if no window can be opened, it says what to run in a second terminal', async () => {
  const note = await openNarrator('forge', info, {}, () => false, async () => 'no terminal');
  assert.equal(note, 'could not open a terminal window (no terminal). The narrator only watches; run this in another terminal:\n  /opt/delphi/bin/delphi observe forge --workspace app --home /srv/state');
});

test('inside tmux it opens one tmux window, never a split', async () => {
  const calls: string[][] = [];
  const note = await openNarrator(undefined, info, { TMUX: '/tmp/tmux-1/default,1,0' }, (a) => { calls.push(a); return true; });
  assert.deepEqual(calls, [['new-window', '/opt/delphi/bin/delphi observe --workspace app --home /srv/state']]);
  assert.equal(note, '(the narrator is open in a new tmux window)');
});

test('if tmux refuses, the command is given instead', async () => {
  const note = await openNarrator('epic', info, { TMUX: 'x' }, () => false);
  assert.match(note, /^could not open a tmux window; run this in another terminal:\n  \/opt\/delphi\/bin\/delphi observe epic/);
});

test('the slash command goes through it', async () => {
  const out: string[] = [];
  const c: Ctx = { cmd: { call: async () => ({}) } as any, mission: 'm', info, state: () => initial(0), post: (t) => out.push(t) };
  const saved = process.env.TMUX; delete process.env.TMUX;
  try { await runSlash('/narrator council', { ...c, open: async () => null }); await runSlash('/observe nope', c); } finally { if (saved !== undefined) process.env.TMUX = saved; }
  assert.match(out[0]!, /^\(the narrator is open/);
  assert.equal(out[1], '/observe takes council, epic, forge, agents or archives, not nope');
});
