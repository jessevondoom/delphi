// What the console does for research: where answers came from, a search of the knowledge base with a preview, and a
// mission notebook. Pure helpers and small file operations; the panel (`panel.ts`) draws and the app wires them.
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { copyText, type Target } from './links.ts';
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

/** The most passage lines a pin quotes; the pointer and hash cover the rest. */
const QUOTE_LINES = 40;

/**
 * What a pin of a document or passage records: the pointer, a hash of the passage (of the whole document when no lines
 * are cited) so a later look can tell whether it changed, and the passage itself for a pointer. `from` is the heading
 * the notebook entry gets.
 */
export function passageOf(text: string, target: Extract<Target, { k: 'doc' | 'pointer' }>): { from: string; text: string } {
  const pointer = copyText(target);
  const all = text.split('\n');
  const lines = target.k === 'pointer' ? all.slice(Math.max(0, target.from - 1), Math.max(target.from, target.to)) : all;
  const hash = createHash('sha256').update(lines.join('\n')).digest('hex').slice(0, 12);
  const quote = target.k === 'pointer' ? '\n\n' + lines.slice(0, QUOTE_LINES).map((l) => '> ' + l).join('\n') + (lines.length > QUOTE_LINES ? `\n> … (${lines.length - QUOTE_LINES} more lines)` : '') : '';
  return { from: `read ${pointer}`, text: `${pointer}\nsha256:${hash}${quote}` };
}
