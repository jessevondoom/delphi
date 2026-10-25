// `/narrator` (`005e` finding 7): in tmux it opens the read-only window in splits, as the Rust consoles do; elsewhere it
// says what to run in a second terminal. The words match `chat.rs` so the consoles answer alike.
import { execFileSync } from 'node:child_process';
import type { Info } from './theme.ts';

export const NARRATOR_VIEWS = ['council', 'epic', 'forge', 'agents', 'spend', 'archives'] as const;
const TILE = ['council', 'epic', 'forge', 'agents'];

/** A shell word: as it is when it is safe, else in single quotes. */
const sh = (s: string) => (/^[\w@%+=:,./-]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`);

/** The commands that open each view of the read-only window, or what is wrong with the request. */
export function narratorLines(view: string | undefined, info: Info): { lines: string[]; tile: boolean } | string {
  const v = view ?? '';
  const tile = v === 'tile';
  if (v && !tile && !(NARRATOR_VIEWS as readonly string[]).includes(v)) return `/narrator takes council, epic, forge, agents, spend, archives or tile, not ${v}`;
  const exe = info.delphi_exe ?? 'delphi';
  const lines = (tile ? TILE : [v]).map((x) => [sh(exe), 'observe', ...(x ? [x] : []), '--workspace', sh(info.workspace_name ?? ''), '--home', sh(info.home ?? '')].join(' '));
  return { lines, tile };
}

export type Tmux = (args: string[]) => boolean;
const realTmux: Tmux = (args) => { try { execFileSync('tmux', args, { stdio: 'ignore' }); return true; } catch { return false; } };

/** Open the window in tmux splits when inside tmux, else say what to run in a second terminal. Returns the note to post. */
export function openNarrator(view: string | undefined, info: Info, env: Record<string, string | undefined> = process.env, tmux: Tmux = realTmux): string {
  const r = narratorLines(view, info);
  if (typeof r === 'string') return r;
  if (env.TMUX) {
    // One pane per view, laid out by tmux: Delphi never tiles inside a window.
    const opened = r.lines.every((l, i) => tmux(['split-window', i % 2 === 0 ? '-h' : '-v', l]));
    if (opened) { tmux(['select-layout', 'tiled']); return `(${r.lines.length} observer window(s) opened in tmux)`; }
    return `could not open tmux panes; run these in other terminals:\n  ${r.lines.join('\n  ')}`;
  }
  return `Run ${r.tile ? 'each of these' : 'this'} in another terminal (it only watches):\n  ${r.lines.join('\n  ')}`;
}
