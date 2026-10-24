// Sources: the documents a conversation leaned on. They are read out of what is already on screen (a reply that names
// `specs/002.md#4`, a tool call that read a file, your own `@specs/…` mention, a knowledge-base hit), never asked of a
// model, so the list is a record of what was actually referenced, not a claim about what was understood.
import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { Block } from './fold.ts';
import { sanitize } from './theme.ts';

export type How = 'said' | 'tool' | 'asked' | 'search';
export type Source = { path: string; anchors: string[]; count: number; by: string[]; how: How[] };

const POINTER = /(?<![\w/.-])((?:specs|context|memory|missions|skills|agents|base|team|local)\/[\w./-]*?\.md|[A-Z][A-Z0-9_-]*\.md)(?:#([\w.-]+))?/g;

/** `path` or `path#anchor` mentions in a piece of text, in order. */
export function extractPointers(text: string): { path: string; anchor: string }[] {
  const out: { path: string; anchor: string }[] = [];
  for (const m of text.matchAll(POINTER)) if (!m[1]!.split('/').includes('..')) out.push({ path: m[1]!, anchor: m[2] ?? '' });
  return out;
}

/** Everything referenced so far: most cited first, then by path. `hits` are knowledge-base search results. */
export function collectSources(blocks: Block[], hits: { pointer: string }[] = [], roleName: (r: string) => string = (r) => r): Source[] {
  const by = new Map<string, Source>();
  const add = (path: string, anchor: string, who: string, how: How) => {
    const s = by.get(path) ?? { path, anchors: [], count: 0, by: [], how: [] };
    by.set(path, s);
    s.count++;
    if (anchor && !s.anchors.includes(anchor)) s.anchors.push(anchor);
    if (!s.by.includes(who)) s.by.push(who);
    if (!s.how.includes(how)) s.how.push(how);
  };
  for (const b of blocks) {
    const who = b.kind === 'user' ? 'you' : roleName(b.role ?? 'orchestrator');
    const how: How | null = b.kind === 'user' ? 'asked' : b.kind === 'tool' ? 'tool' : b.kind === 'reply' ? 'said' : null;
    if (!how) continue;
    for (const p of extractPointers(b.text)) add(p.path, p.anchor, who, how);
  }
  for (const h of hits) { const [path = '', anchor = ''] = sanitize(h.pointer).split('#'); if (path) add(path, anchor, 'search', 'search'); }
  return [...by.values()].sort((a, b) => b.count - a.count || a.path.localeCompare(b.path));
}

/** Where a pointer lives on disk, if it is a file inside the workspace. */
export function resolveSource(workspace: string, rel: string): string | null {
  if (!workspace || rel.split('/').includes('..') || rel.startsWith('/')) return null;
  for (const root of ['team', 'base', 'local', '']) {
    const p = join(workspace, root, rel);
    try { if (statSync(p).isFile()) return p; } catch { /* try the next root */ }
  }
  return null;
}

/** The first lines of a source for the preview pane, or one line saying why there is none. */
export function previewOf(workspace: string, rel: string, maxLines = 60): string[] {
  const p = resolveSource(workspace, rel);
  if (!p) return ['(not found in this workspace)'];
  try {
    const text = readFileSync(p, 'utf8');
    const body = text.startsWith('---\n') ? text.slice(text.indexOf('\n---', 4) + 4).replace(/^\n+/, '') : text;
    return sanitize(body).split('\n').slice(0, maxLines);
  } catch { return ['(could not be read)']; }
}
