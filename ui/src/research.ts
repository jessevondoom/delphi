// What the console does for research: where answers came from, a search of the knowledge base with a preview, and a
// mission notebook. Pure helpers and small file operations; the panel (`panel.ts`) draws and the app wires them.
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import type { Info } from './theme.ts';

export type Source = { path: string; how: 'read' | 'cited'; roles: string[]; count: number; last: number };

/** Logical workspace paths in a piece of text: `specs/…`, `missions/…`, `context/…`, `memory/…`, `@project/…`, `@payload/…`. */
export function citations(text: string): string[] {
  const out = new Set<string>();
  const re = /(?:^|[\s(`'"\[])(@?(?:(?:specs|missions|context|memory|themes|skills|agents|project|payload)\/[A-Za-z0-9_./#-]*[A-Za-z0-9_/-]|(?:REVIEW|GOVERNANCE|SPEC|OVERVIEW)\.md))/g;
  for (const m of text.matchAll(re)) out.add(m[1]!.replace(/[.,;:]+$/, ''));
  return [...out];
}

/** Add what a piece of work touched to the list: an agent read it (live) or a reply cited it. */
export function noteSources(list: Record<string, Source>, paths: string[], how: Source['how'], role: string, at: number): Record<string, Source> {
  if (!paths.length) return list;
  const next = { ...list };
  for (const p of paths) {
    const cur = next[p];
    const merged: Source = cur
      ? { ...cur, how: cur.how === 'read' || how === 'read' ? 'read' : 'cited', roles: cur.roles.includes(role) ? cur.roles : [...cur.roles, role], count: cur.count + 1, last: at }
      : { path: p, how, roles: [role], count: 1, last: at };
    next[p] = merged;
  }
  return next;
}

/** The place on disk for a logical path, if it is a workspace document the console may show. `null` otherwise. */
export function physical(info: Info, logical: string): string | null {
  const rel = logical.replace(/^@/, '').split('#')[0]!;
  if (!rel || rel.includes('..') || rel.startsWith('/') || /^(project|payload)\//.test(rel)) return null;
  for (const root of [join(info.workspace, 'team'), join(info.workspace, 'local'), info.workspace]) {
    const p = join(root, rel);
    if (existsSync(p)) return p;
  }
  return null;
}

/** The first lines of a document, for the preview beside a search hit. */
export function previewOf(info: Info, logical: string, max = 40): string[] {
  const p = physical(info, logical);
  if (!p) return [];
  try { return readFileSync(p, 'utf8').replace(/\x1b/g, '').split('\n').slice(0, max); } catch { return []; }
}

export const notesPath = (info: Info, mission: string) => join(info.workspace, 'local/missions', mission, 'notes.md');

const FRONTMATTER = (mission: string, now: string) => `---\nid: notes-${mission}\ntitle: "Mission notebook"\nauthor: operator\ntimestamp: ${now}\nsource_type: internal_generated\nconfidence: medium\nstatus: active\nsource_mission: ${mission}\ntags: [notes]\n---\n\n# Notebook\n\nPinned by the operator from the console. Each entry says where it came from.\n`;

/** Append one entry to the mission notebook, creating it with its frontmatter the first time. */
export function pin(info: Info, mission: string, from: string, text: string, now = new Date()): string {
  const file = notesPath(info, mission);
  mkdirSync(dirname(file), { recursive: true });
  if (!existsSync(file)) writeFileSync(file, FRONTMATTER(mission, now.toISOString()));
  appendFileSync(file, `\n## ${now.toISOString()} · ${from}\n\n${text.trim().replace(/\x1b/g, '')}\n`);
  return file;
}

export function readNotes(info: Info, mission: string): string {
  try { return readFileSync(notesPath(info, mission), 'utf8').replace(/^---[\s\S]*?---\n/, ''); } catch { return ''; }
}
