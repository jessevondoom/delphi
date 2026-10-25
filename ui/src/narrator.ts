// `/observe [view]` (`005e` finding 7): opens the narrator, the read-only full-screen window, as one window of its own:
// a new tmux window when inside tmux, else a new terminal window. Never a split pane; the views switch inside it.
// The words match `chat.rs` so the consoles answer alike.
import { execFileSync } from 'node:child_process';
import type { Info } from './theme.ts';
import { openWindow } from './window.ts';

export const NARRATOR_VIEWS = ['council', 'epic', 'forge', 'agents', 'archives'] as const;

/** A shell word: as it is when it is safe, else in single quotes. */
const sh = (s: string) => (/^[\w@%+=:,./-]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`);

/** The command that opens the narrator (at `view`, when one is named), or what is wrong with the request. */
export function narratorCommand(view: string | undefined, info: Info): string {
  const v = view ?? '';
  if (v && !(NARRATOR_VIEWS as readonly string[]).includes(v)) return `/observe takes council, epic, forge, agents or archives, not ${v}`;
  const exe = info.delphi_exe ?? 'delphi';
  return [sh(exe), 'observe', ...(v ? [v] : []), '--workspace', sh(info.workspace_name ?? ''), '--home', sh(info.home ?? '')].join(' ');
}

export type Tmux = (args: string[]) => boolean;
const realTmux: Tmux = (args) => { try { execFileSync('tmux', args, { stdio: 'ignore' }); return true; } catch { return false; } };

export type Opener = (argv: string[]) => Promise<string | null>;

/** Open the narrator: a tmux window when inside tmux, else a new terminal window, every call. Returns the note to post. */
export async function openNarrator(view: string | undefined, info: Info, env: Record<string, string | undefined> = process.env, tmux: Tmux = realTmux, open: Opener = openWindow): Promise<string> {
  const line = narratorCommand(view, info);
  if (line.startsWith('/observe ')) return line;
  if (env.TMUX) {
    if (tmux(['new-window', line])) return '(the narrator is open in a new tmux window)';
    return `could not open a tmux window; run this in another terminal:\n  ${line}`;
  }
  const why = await open(['sh', '-c', line]);
  if (why !== null) return `could not open a terminal window (${why}). The narrator only watches; run this in another terminal:\n  ${line}`;
  return '(the narrator is open in a new terminal window)';
}
