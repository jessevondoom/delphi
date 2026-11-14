// Work output (spec 111 §5.4): what the team produced, grouped by what is there. Many projects are research and reports, not
// specifications and code, so nothing here assumes a kind: groups come from where the daemon filed a document and from what
// agents recorded, and a group with nothing in it is not shown. Pure.
import { titleOf, type DocRow } from './kb.ts';
import type { OState } from './types.ts';

export type Group = 'mission' | 'research' | 'context' | 'memory' | 'change' | 'build';
export const GROUP_LABEL: Record<Group, string> = { mission: 'Mission documents', research: 'Research', context: 'Context and reference', memory: 'Lessons and memory', change: 'Recorded changes', build: 'Builds' };

export type Item = {
  key: string; group: Group; title: string; detail: string;
  /** When it was last written (Unix ms), 0 when the daemon did not say. */
  at_ms: number;
  /** The role that made it, when the daemon said. */
  role?: string;
  /** A document in the workspace: its path there, which the reader and the Finder can open. */
  path?: string;
  /** Files an agent named, relative to the project it worked in. */
  files: string[];
  status: string; mission?: string;
};

/** Which group a listed document belongs to, or none for what configures the team (skills, agents, the charter). */
export function groupOf(row: DocRow): Group | null {
  switch (row.zone) {
    case 'missions': return 'mission';
    case 'summaries': case 'research': return 'research';
    case 'context': return 'context';
    case 'memory': return 'memory';
    default: return null;
  }
}

/** What a file is, from its name: enough to say "deck" or "image" beside it. */
export function fileKind(path: string): string {
  const ext = (/\.([A-Za-z0-9]+)$/.exec(path)?.[1] ?? '').toLowerCase();
  if (['md', 'mdx', 'txt', 'rst', 'pdf', 'doc', 'docx', 'odt', 'tex'].includes(ext)) return 'document';
  if (['ppt', 'pptx', 'key', 'odp'].includes(ext)) return 'deck';
  if (['xls', 'xlsx', 'csv', 'tsv', 'ods', 'parquet'].includes(ext)) return 'data';
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'heic', 'tif', 'tiff'].includes(ext)) return 'image';
  if (['mp4', 'mov', 'webm', 'mkv', 'avi'].includes(ext)) return 'video';
  if (['mp3', 'wav', 'm4a', 'flac', 'ogg'].includes(ext)) return 'audio';
  if (['html', 'htm'].includes(ext)) return 'page';
  if (['json', 'toml', 'yaml', 'yml', 'xml', 'ini', 'lock'].includes(ext)) return 'config';
  if (ext) return 'code';
  return 'file';
}

const missionOf = (path: string): string | undefined => /^missions\/([^/]+)\//.exec(path)?.[1];

/**
 * Everything to show, newest first. A research summary and its sources file (`x.md`, `x.bib.md`) are one item. A mission's
 * generated status page is left out when the mission has anything else: it restates the event log.
 */
export function items(rows: DocRow[], state: OState | null): Item[] {
  const out: Item[] = [];
  const paths = new Set(rows.map((r) => r.rel_path));
  const withMore = new Set(rows.filter((r) => r.zone === 'missions' && !r.rel_path.endsWith('/status.md')).map((r) => missionOf(r.rel_path)));
  for (const r of rows) {
    const group = groupOf(r);
    if (!group) continue;
    if (r.rel_path.endsWith('.bib.md') && paths.has(r.rel_path.replace(/\.bib\.md$/, '.md'))) continue;
    if (group === 'mission' && r.rel_path.endsWith('/status.md') && withMore.has(missionOf(r.rel_path))) continue;
    const sources = paths.has(r.rel_path.replace(/\.md$/, '.bib.md')) && !r.rel_path.endsWith('.bib.md');
    out.push({
      key: `doc:${r.rel_path}`, group, title: titleOf(r.rel_path), detail: [r.kind !== 'other' ? r.kind : '', sources ? 'with its sources' : ''].filter(Boolean).join(' · '),
      at_ms: Date.parse(r.updated_at) || 0, path: r.rel_path, files: sources ? [r.rel_path.replace(/\.md$/, '.bib.md')] : [], status: r.status, mission: missionOf(r.rel_path),
    });
  }
  for (const [i, c] of (state?.changes ?? []).entries()) {
    out.push({ key: `change:${c.at_ms}:${i}`, group: 'change', title: c.summary, detail: c.files.length ? `${c.files.length} ${c.files.length === 1 ? 'file' : 'files'}` : 'no file named', at_ms: c.at_ms, role: c.role, files: c.files, status: 'recorded', mission: state?.mission_id });
  }
  for (const [i, b] of (state?.builds ?? []).entries()) {
    if (!b.branch) continue;
    out.push({ key: `build:${b.task_id}:${i}`, group: 'build', title: b.branch, detail: [b.sha ? b.sha.slice(0, 8) : '', b.merge].filter(Boolean).join(' · '), at_ms: 0, files: [], status: b.merge || 'built', mission: state?.mission_id });
  }
  return out.sort((a, b) => b.at_ms - a.at_ms || a.title.localeCompare(b.title));
}

/** How many items each group holds, for the groups that hold any, in a fixed order. */
export function groupCounts(all: Item[]): { group: Group; label: string; count: number }[] {
  const order: Group[] = ['change', 'mission', 'research', 'context', 'memory', 'build'];
  return order.map((group) => ({ group, label: GROUP_LABEL[group], count: all.filter((x) => x.group === group).length })).filter((g) => g.count > 0);
}
