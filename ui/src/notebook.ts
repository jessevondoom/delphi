// The mission notebook: what the operator pinned (`/pin`). One markdown document per mission with the same
// frontmatter as `/export`, so it is indexed and searchable like any mission document. Appending only; nothing is
// ever rewritten, and the daemon is not involved.
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { sanitize } from './theme.ts';

export type Pin = { at: string; source: string; text: string };

const MAX = 20_000;
export const notebookPath = (workspace: string, mission: string) => join(workspace, 'local/missions', mission, 'notebook.md');

const header = (mission: string, now: Date) =>
  `---\nid: notebook-${mission}\ntitle: "Mission notebook"\nauthor: operator\ntimestamp: ${now.toISOString()}\nsource_type: internal_generated\nconfidence: medium\nstatus: active\nsource_mission: ${mission}\ntags: [notebook]\n---\n\n`;

/** A heading inside a pin would split it in two when read back, so it is escaped on the way in. */
const escape = (t: string) => t.replace(/^(#{1,6} )/gm, '\\$1');
const unescape = (t: string) => t.replace(/^\\(#{1,6} )/gm, '$1');

export function renderPin(p: Pin): string {
  return `## ${p.at} · ${p.source.replace(/\s+/g, ' ')}\n\n${escape(sanitize(p.text).trim().slice(0, MAX))}\n\n`;
}

export function parseNotebook(text: string): Pin[] {
  const body = text.startsWith('---\n') ? text.slice(text.indexOf('\n---', 4) + 4) : text;
  return body.split(/^## /m).slice(1).map((chunk) => {
    const nl = chunk.indexOf('\n');
    const head = (nl < 0 ? chunk : chunk.slice(0, nl)).trim();
    const [at = '', ...rest] = head.split(' · ');
    return { at, source: rest.join(' · '), text: unescape(nl < 0 ? '' : chunk.slice(nl + 1).trim()) };
  });
}

export function readNotebook(workspace: string, mission: string): Pin[] {
  try { return parseNotebook(readFileSync(notebookPath(workspace, mission), 'utf8')); } catch { return []; }
}

/** Append one pin, creating the document on first use. Returns the path. */
export function addPin(workspace: string, mission: string, pin: Omit<Pin, 'at'>, now = new Date()): string {
  const file = notebookPath(workspace, mission);
  mkdirSync(dirname(file), { recursive: true });
  if (!existsSync(file)) writeFileSync(file, header(mission, now));
  appendFileSync(file, renderPin({ ...pin, at: now.toISOString() }));
  return file;
}
