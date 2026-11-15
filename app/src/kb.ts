// The knowledge base as the daemon's retrieval service answers it (spec 111 §5.3), and what the screens make of it. Pure.

/** One search result (`/v1/search`). */
export type Hit = {
  path: string; pointer: string; heading: string; preview: string; kind: string; collection: string;
  /** `validated` or `not validated`: the daemon's own word for whether someone vouched for it. */
  label: string; status: string; note: string | null; vouched_by: string | null; updated?: string; start_line?: number; end_line?: number;
};
export type SearchBody = { query: string; mode?: string; candidates?: number; capped?: boolean; facets?: Record<string, number>; results: Hit[] };
/** One row of a listing (`/v1/documents`). */
export type DocRow = { rel_path: string; kind: string; zone: string; status: string; source_type: string; updated_at: string };
export type Collection = { name: string; documents: number; updated: string };
/** One document (`/v1/doc`). */
export type Doc = { path: string; collection: string; status: string; stale: boolean; text: string };

export const searchTarget = (q: string, kind?: string, limit = 20): string =>
  `/v1/search?q=${encodeURIComponent(q.trim())}&limit=${limit}&facets=1${kind ? `&kind=${encodeURIComponent(kind)}` : ''}`;
export const docTarget = (path: string): string => `/v1/doc?path=${encodeURIComponent(path)}`;
export const listTarget = (limit = 50, kind?: string): string => `/v1/documents?limit=${limit}${kind ? `&kind=${encodeURIComponent(kind)}` : ''}`;

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
