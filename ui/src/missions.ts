// /sessions: the workspace's sessions, newest first (`crates/delphi/src/missions.rs` does the same for the CLI and
// the Rust console; keep the two in step). A session is a directory under `local/sessions/`, or under `local/missions/`
// when it was written before that name changed (spec 058-A9); its last activity is the newest event file in it.
import { existsSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

export type MissionRow = { id: string; last: number | null };

const mtime = (p: string): number | null => { try { return statSync(p).mtimeMs; } catch { return null; } };

/** Where session `id` lives: its old `local/missions/` directory if it has one, otherwise `local/sessions/`. */
export function sessionDir(workspace: string, id: string): string {
  const old = join(workspace, 'local/missions', id);
  return existsSync(old) ? old : join(workspace, 'local/sessions', id);
}

export function listMissions(workspace: string | undefined): MissionRow[] {
  if (!workspace) return [];
  const ids = new Set<string>();
  for (const root of ['local/missions', 'local/sessions']) {
    try { for (const e of readdirSync(join(workspace, root), { withFileTypes: true })) if (e.isDirectory()) ids.add(e.name); } catch { /* no such root */ }
  }
  const rows = [...ids].map((id) => {
    const dir = sessionDir(workspace, id);
    let newest: number | null = null;
    try { for (const f of readdirSync(join(dir, 'events'))) newest = Math.max(newest ?? 0, mtime(join(dir, 'events', f)) ?? 0); } catch { /* no events yet */ }
    return { id, last: newest ?? mtime(dir) };
  });
  return rows.sort((a, b) => (b.last ?? 0) - (a.last ?? 0) || (a.id < b.id ? 1 : -1));
}

export function ago(then: number | null, now: number): string {
  if (then === null) return '—';
  const s = Math.max(0, Math.floor((now - then) / 1000));
  return s < 60 ? 'just now' : s < 3600 ? `${Math.floor(s / 60)} min ago` : s < 86400 ? `${Math.floor(s / 3600)} h ago` : `${Math.floor(s / 86400)} d ago`;
}

export function renderMissions(rows: MissionRow[], now: number): string {
  if (!rows.length) return 'no missions yet: the first one starts when you run `delphi` or `delphi run "<task>"`';
  const width = Math.max(...rows.map((r) => [...r.id].length));
  const lines = rows.map((r, i) => `  ${r.id.padEnd(width)}  ${ago(r.last, now)}${i === 0 ? '  (latest)' : ''}`);
  return [...lines, 'reopen one with: delphi chat --mission <id>'].join('\n');
}
