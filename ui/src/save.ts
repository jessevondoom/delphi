// `/save`: keep a reply, or a markdown file, in the knowledge base (`specs/068` §3). It runs `delphi kb import`, the one
// door the CLI uses, so what the console saves and what `delphi kb import` saves are the same thing.
import { existsSync, statSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Info } from './theme.ts';

export type Save = { args: string[]; stdin?: string } | { error: string };

/** A dragged-in file arrives as a path, often quoted or with escaped spaces. */
export function pathOf(arg: string): string {
  const t = arg.trim();
  const unq = (t.length > 1 && /^(['"]).*\1$/.test(t)) ? t.slice(1, -1) : t;
  return unq.replace(/\\ /g, ' ');
}

export function saveCommand(info: Info, arg: string, lastReply: string | undefined, cwd = process.cwd()): Save {
  if (!info.delphi_exe) return { error: 'this console was not told where delphi is; run `delphi kb import <file>`' };
  const tail = ['--workspace', info.workspace_name ?? '', '--home', info.home ?? ''];
  if (arg.trim()) {
    const p = resolve(cwd, pathOf(arg));
    if (!existsSync(p)) return { error: `no such file or folder: ${p}` };
    statSync(p);
    return { args: ['kb', 'import', p, ...tail] };
  }
  if (!lastReply?.trim()) return { error: 'nothing to save yet: /save keeps the last reply, or /save <file.md> imports a file (drag one onto the terminal)' };
  return { args: ['kb', 'import', '-', ...tail], stdin: lastReply };
}

/** What the console says for the label the CLI and MCP print exactly (`specs/068` §6.3): plain words, with the machine value beside it. */
export function plainLabel(label: string): { plain: string; machine: string } | null {
  if (!/\bexternal_unvalidated\b/.test(label)) return null;
  return { plain: (/operator-imported/.test(label) ? 'imported summary' : 'summary') + ' · not yet checked', machine: label };
}
