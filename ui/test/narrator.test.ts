// `/narrator` in the full-screen console behaves as it does in the Rust consoles (`005e` finding 7, slice 6).
import test from 'node:test';
import assert from 'node:assert/strict';
import { narratorLines, openNarrator } from '../src/narrator.ts';
import { runSlash, type Ctx } from '../src/run.ts';
import { initial } from '../src/fold.ts';
import type { Info } from '../src/theme.ts';

const info = { roles: {}, delphi_exe: '/opt/delphi/bin/delphi', workspace_name: 'app', home: '/srv/state' } as unknown as Info;

test('each view is the command that opens it; tile is four', () => {
  assert.deepEqual(narratorLines('epic', info), { lines: ['/opt/delphi/bin/delphi observe epic --workspace app --home /srv/state'], tile: false });
  const all = narratorLines('tile', info) as { lines: string[]; tile: boolean };
  assert.equal(all.tile, true);
  assert.deepEqual(all.lines.map((l) => l.split(' ')[2]), ['council', 'epic', 'forge', 'agents']);
  assert.equal((narratorLines(undefined, info) as { lines: string[] }).lines[0], '/opt/delphi/bin/delphi observe --workspace app --home /srv/state', 'no view: the window\'s first');
});

test('a path with a space is quoted, and a view that does not exist is refused with the list', () => {
  const spaced = { ...info, home: "/srv/some one/it's" } as Info;
  assert.ok((narratorLines('epic', spaced) as { lines: string[] }).lines[0]!.endsWith(`--home '/srv/some one/it'\\''s'`));
  assert.equal(narratorLines('nope', info), '/narrator takes council, epic, forge, agents, archives or tile, not nope');
});

test('outside tmux it says what to run in a second terminal', () => {
  const note = openNarrator('forge', info, {}, () => { throw new Error('tmux must not be called'); });
  assert.equal(note, 'Run this in another terminal (it only watches):\n  /opt/delphi/bin/delphi observe forge --workspace app --home /srv/state');
  assert.match(openNarrator('tile', info, {}, () => false), /^Run each of these in another terminal/);
});

test('inside tmux it opens one pane per view, alternating the split, then tiles them', () => {
  const calls: string[][] = [];
  const note = openNarrator('tile', info, { TMUX: '/tmp/tmux-1/default,1,0' }, (a) => { calls.push(a); return true; });
  assert.deepEqual(calls.map((c) => c.slice(0, 2).join(' ')), ['split-window -h', 'split-window -v', 'split-window -h', 'split-window -v', 'select-layout tiled']);
  assert.match(calls[0]![2]!, /observe council --workspace app/);
  assert.equal(note, '(4 observer window(s) opened in tmux)');
});

test('if tmux refuses, the commands are given instead', () => {
  const note = openNarrator('epic', info, { TMUX: 'x' }, () => false);
  assert.match(note, /^could not open tmux panes; run these in other terminals:\n  \/opt\/delphi\/bin\/delphi observe epic/);
});

test('the slash command goes through it', async () => {
  const out: string[] = [];
  const c: Ctx = { cmd: { call: async () => ({}) } as any, mission: 'm', info, state: () => initial(0), post: (t) => out.push(t) };
  const saved = process.env.TMUX; delete process.env.TMUX;
  try { await runSlash('/narrator council', c); await runSlash('/observe nope', c); } finally { if (saved !== undefined) process.env.TMUX = saved; }
  assert.match(out[0]!, /^Run this in another terminal/);
  assert.equal(out[1], '/narrator takes council, epic, forge, agents, archives or tile, not nope');
});
