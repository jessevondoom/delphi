// The knowledge base as the daemon's retrieval service answers it (spec 111 §5.3), and what the screens make of it. Pure.

/** One search result (`/v1/search`). */
export type Hit = {
  path: string; pointer: string; heading: string; preview: string; kind: string; collection: string;
  /** `validated` or `not validated`: the daemon's own word for whether someone vouched for it. */
  label: string; status: string; note: string | null; vouched_by: string | null; updated?: string; start_line?: number; end_line?: number;
  /** The project whose research this is, by its display name (spec 115 §4 item 1); absent or null for anything else. */
  project?: string | null;
};
/** `projects`: how many results each project's research gave, for narrowing to one. */
export type SearchBody = { query: string; mode?: string; candidates?: number; capped?: boolean; facets?: Record<string, number>; projects?: Record<string, number>; results: Hit[] };
/** One row of a listing (`/v1/documents`). `author` is who its front matter names; an older daemon does not send it. */
export type DocRow = { rel_path: string; kind: string; zone: string; status: string; source_type: string; updated_at: string; author?: string | null };
export type Collection = { name: string; documents: number; updated: string };
/** One document (`/v1/doc`). */
export type Doc = { path: string; collection: string; status: string; stale: boolean; text: string };

/** A search, with every project's research (`projects=all`) or only the `project` named (spec 115 §4 item 1). */
export const searchTarget = (q: string, kind?: string, limit = 20, project?: string): string =>
  `/v1/search?q=${encodeURIComponent(q.trim())}&limit=${limit}&facets=1${kind ? `&kind=${encodeURIComponent(kind)}` : ''}${project ? `&project=${encodeURIComponent(project)}` : '&projects=all'}`;
export const docTarget = (path: string): string => `/v1/doc?path=${encodeURIComponent(path)}`;
/** A listing of the newest documents; `where` narrows it to one kind or one collection and `offset` asks for a later page. */
export const listTarget = (limit = 50, kind?: string, where: { collection?: string; offset?: number } = {}): string =>
  `/v1/documents?limit=${limit}${kind ? `&kind=${encodeURIComponent(kind)}` : ''}${where.collection ? `&collection=${encodeURIComponent(where.collection)}` : ''}${where.offset ? `&offset=${where.offset}` : ''}`;

/** The most rows the daemon sends for one listing. */
export const LIST_PAGE = 200;
/**
 * Every row of a listing, read a page at a time until a short page: the daemon sends at most `LIST_PAGE` rows, newest
 * first, so one read of a large knowledge base leaves the older documents out. Stops at `most` rows. A page the daemon
 * refuses ends the read with that status and the rows so far.
 */
export async function listAll(read: (target: string) => Promise<{ status: number; body: unknown }>, where: { kind?: string; collection?: string }, most = 5000): Promise<{ status: number; rows: DocRow[] }> {
  const rows: DocRow[] = [];
  while (rows.length < most) {
    const r = await read(listTarget(LIST_PAGE, where.kind, { collection: where.collection, offset: rows.length }));
    if (r.status !== 200) return { status: r.status, rows };
    const page = Array.isArray(r.body) ? (r.body as DocRow[]) : [];
    rows.push(...page);
    if (page.length < LIST_PAGE) break;
  }
  return { status: 200, rows };
}

/** A document named in the address (`?doc=missions/m/report.md`): a link straight to its page. Only a plain relative path. */
export function docInAddress(search: string): string {
  const p = (new URLSearchParams(search).get('doc') ?? '').trim();
  return /^[\w][\w ./-]*$/.test(p) && !p.split('/').includes('..') ? p : '';
}

/** A heading path (`A > B > C`) as its last part and the parts above it. */
export function headingOf(heading: string): { title: string; above: string } {
  const parts = heading.split(' > ').map((p) => p.trim()).filter(Boolean);
  return { title: parts[parts.length - 1] ?? heading, above: parts.slice(0, -1).join(' › ') };
}

/** A document's front matter (the `key: value` lines between the first two `---` lines) and the text after it. */
export function frontmatter(text: string): { meta: Record<string, string>; body: string } {
  const t = text.replace(/\r\n?/g, '\n');
  if (!t.startsWith('---\n')) return { meta: {}, body: t };
  const end = t.indexOf('\n---', 4);
  if (end < 0) return { meta: {}, body: t };
  const meta: Record<string, string> = {};
  for (const line of t.slice(4, end).split('\n')) {
    const m = /^([A-Za-z_][\w-]*):\s*(.*)$/.exec(line);
    if (!m) continue;
    // A value may be quoted and may carry a trailing comment; the window shows the value.
    let v = m[2]!.trim();
    const quoted = /^"((?:[^"\\]|\\.)*)"/.exec(v) ?? /^'([^']*)'/.exec(v);
    v = quoted ? quoted[1]!.replace(/\\"/g, '"') : v.replace(/\s+#.*$/, '');
    if (v) meta[m[1]!] = v;
  }
  const rest = t.slice(end + 4);
  return { meta, body: rest.replace(/^[^\n]*\n?/, '').replace(/^\n+/, '') };
}

/** What to call a document: its front matter's title or name, else its first heading, else its file name. */
export function titleOf(path: string, text?: string): string {
  if (text !== undefined) {
    const { meta, body } = frontmatter(text);
    const h = /^#\s+(.+)$/m.exec(body);
    if (meta.title || meta.name) return meta.title ?? meta.name!;
    if (h) return h[1]!.replace(/`/g, '');
  }
  const file = path.split('/').pop() ?? path;
  const stem = file.replace(/\.[^.]+$/, '');
  // `SKILL.md`, `status.md`, `AGENT.md` say little: the folder is the name.
  const generic = ['skill', 'agent', 'status', 'readme', 'index', '_index'].includes(stem.toLowerCase());
  return generic ? `${path.split('/').slice(-2, -1)[0] ?? stem} (${stem.toLowerCase()})` : stem.replace(/[-_]+/g, ' ');
}

/** `2026-11-12`, or nothing for a time that does not parse. */
export const dayOf = (iso?: string): string => (iso && !Number.isNaN(Date.parse(iso)) ? iso.slice(0, 10) : '');

/** The daemon's `source_type`, in words. */
export function sourceWords(source: string): string {
  switch (source) {
    case 'user_ratified': return 'ratified by the operator';
    case 'internal_generated': return 'written by an agent';
    case 'internal_empirical': return 'observed in a session';
    case 'external_untrusted': return 'from outside, not checked';
    case 'federated_verified': return 'from the shared library';
    default: return source.replace(/_/g, ' ');
  }
}
