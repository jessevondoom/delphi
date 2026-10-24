// What Tab completes: slash commands at the start, and after an `@` the roles and files a request can point at.
import { COMMANDS, filter } from './commands.ts';
import type { Info } from './theme.ts';

export type Item = { label: string; desc: string; insert: string };
export type Completion = { items: Item[]; from: number; to: number };

/** `listDir('specs')` names the entries of a directory under the team root (injected so this stays pure). */
export type ListDir = (rel: string) => { name: string; dir: boolean }[];

export function completions(text: string, cur: number, info: Info, listDir: ListDir): Completion | null {
  let from = cur;
  while (from > 0 && !/\s/.test(text[from - 1]!)) from--;
  const tok = text.slice(from, cur);
  if (tok.startsWith('/') && from === 0 && !text.includes('\n')) {
    const items = filter(tok).map((c) => ({ label: c.name, desc: c.desc, insert: c.name }));
    return items.length ? { items, from, to: cur } : null;
  }
  if (!tok.startsWith('@')) return null;
  const p = tok.slice(1);
  const items: Item[] = [];
  if (!p.includes('/')) {
    const q = p.toLowerCase();
    for (const [id, r] of Object.entries(info.roles)) {
      if (id.startsWith(q) || r.name.toLowerCase().startsWith(q)) items.push({ label: '@' + id, desc: `role · ${r.name}`, insert: '@' + id + ' ' });
    }
    if ('specs'.startsWith(q)) items.push({ label: '@specs/', desc: 'a spec in this workspace', insert: '@specs/' });
  } else {
    const slash = p.lastIndexOf('/');
    const dir = p.slice(0, slash), prefix = p.slice(slash + 1);
    if (dir === 'specs' || dir.startsWith('specs/')) {
      for (const e of listDir(dir)) {
        if (e.name.startsWith(prefix) && !e.name.startsWith('.')) items.push({ label: `@${dir}/${e.name}${e.dir ? '/' : ''}`, desc: e.dir ? 'folder' : 'file', insert: `@${dir}/${e.name}${e.dir ? '/' : ' '}` });
      }
    }
  }
  return items.length ? { items: items.slice(0, 40), from, to: cur } : null;
}

/** Ctrl+R: the newest history entry containing `q` at or before index `from` (exclusive upper bound), or -1. */
export function searchHistory(hist: string[], q: string, before: number): number {
  if (!q) return -1;
  const needle = q.toLowerCase();
  for (let i = Math.min(before, hist.length) - 1; i >= 0; i--) if (hist[i]!.toLowerCase().includes(needle)) return i;
  return -1;
}

export { COMMANDS };
