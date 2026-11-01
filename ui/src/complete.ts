// What Tab completes: slash commands at the start, and after an `@` the roles and files a request can point at.
import { COMMANDS, filter, lookup } from './commands.ts';
import { graphemes } from './text.ts';
import type { Info } from './theme.ts';

export type Item = { label: string; desc: string; insert: string; /** Enter on it runs `/projects <insert>` at once. */ run?: boolean };

/** The word that names a project after `/projects`: its directory name, lowercased, spaces as hyphens. */
export const projectKey = (name: string): string => name.trim().toLowerCase().replace(/\s+/g, '-');
export type Completion = { items: Item[]; from: number; to: number };

/**
 * Enter with the menu open: does it take the highlighted item (true) or run what was typed (false)? What was typed runs
 * when it already is the item, or is a whole command name or alias (`/clear` for `/new`, `/exit` for `/quit`) and the
 * operator has not moved the highlight off the first item; otherwise Enter completes it, as Tab does.
 */
export function enterAccepts(typed: string, item: Item, highlighted: number): boolean {
  if (typed === item.insert.trimEnd()) return false;
  return !(highlighted === 0 && lookup(typed) !== undefined);
}

/** `listDir('specs')` names the entries of a directory under the team root (injected so this stays pure). */
export type ListDir = (rel: string) => { name: string; dir: boolean }[];

/**
 * `chars` are the prompt's characters (graphemes) and `cur` an index into them, which is how the composer counts. A
 * plain string is split the same way. `from` and `to` of the result are indexes into the same array.
 */
export function completions(input: string | string[], cur: number, info: Info, listDir: ListDir): Completion | null {
  const chars = typeof input === 'string' ? graphemes(input) : input;
  let from = cur;
  while (from > 0 && !/\s/.test(chars[from - 1]!)) from--;
  const tok = chars.slice(from, cur).join('');
  const pm = /^\/projects\s+(\S*)$/.exec(chars.slice(0, cur).join(''));
  if (pm && !chars.includes('\n')) {
    const q = pm[1]!.toLowerCase();
    const items: Item[] = (info.projects ?? []).filter((p) => projectKey(p.name).startsWith(q)).map((p) => ({ label: projectKey(p.name), desc: p.home ? 'home workspace' : p.root, insert: projectKey(p.name), run: true }));
    return items.length ? { items, from: cur - pm[1]!.length, to: cur } : null;
  }
  if (tok.startsWith('/') && from === 0 && !chars.includes('\n')) {
    const items = filter(tok).map((c) => ({ label: c.name, desc: c.desc, insert: c.name }));
    return items.length ? { items, from, to: cur } : null;
  }
  if (!tok.startsWith('@')) return null;
  const p = tok.slice(1);
  const items: Item[] = [];
  if (!p.includes('/')) {
    const q = p.toLowerCase();
    for (const [id, r] of Object.entries(info.roles)) {
      if (info.forbidden_agents?.includes(id)) continue;
      if (info.interactive_agents && info.interactive_agents.length > 0 && !info.interactive_agents.includes(id)) continue;
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

/** `bundle:agent` or `org/bundle:agent` (the word after `@`) as the daemon's `bundle:` target, else null. A path mention such as `specs/a.md:12` is not one. */
export function bundleTarget(word: string): string | null {
  const m = /^([A-Za-z0-9_-]+)(?:\/([A-Za-z0-9_-]+))?:([A-Za-z0-9_-]+)$/.exec(word);
  if (!m) return null;
  if (m[2] && ['specs', 'project', 'bundle', 'runtime', 'team', 'missions'].includes(m[1]!)) return null;
  return `bundle:${m[2] ? `${m[1]}/${m[2]}` : m[1]}:${m[3]}`;
}

/** Ctrl+R: the newest history entry containing `q` at or before index `from` (exclusive upper bound), or -1. */
export function searchHistory(hist: string[], q: string, before: number): number {
  if (!q) return -1;
  const needle = q.toLowerCase();
  for (let i = Math.min(before, hist.length) - 1; i >= 0; i--) if (hist[i]!.toLowerCase().includes(needle)) return i;
  return -1;
}

export { COMMANDS };
