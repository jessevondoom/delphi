// Links as data (spec 029 §3): what a link points at, where it sits in rendered text, and how a surface moves through
// them. Pure: nothing here draws or reads a file. The Markdown renderer produces `Link`s (`renderMarkdownLinks`), the
// reader and every surface that wants link focus consume them.

/** What a link points at. External targets are never fetched by Delphi: the system browser opens them on request. */
export type Target =
  | { k: 'doc'; path: string; anchor?: string }
  | { k: 'pointer'; path: string; from: number; to: number }
  | { k: 'external'; url: string };

/** Where a link was drawn: a row of the rendered lines and a column range on it. A wrapped link has one span per row. */
export type Span = { line: number; col: number; width: number };

export type Link = { id: number; label: string; target: Target; spans: Span[] };

/**
 * A logical path to a Markdown file, written out in plain text: `specs/004-x.md`, `context/lessons/a.md#L10-L20`,
 * `specs/005e.md#section`. It needs a directory part, so a bare `README.md` in prose is not a link; and it cannot
 * start inside a URL or a longer path.
 */
export const POINTER_RE = /(?<![\w/.:@-])(?:[\w.-]+\/)+[\w.-]+\.md(?:#L\d+(?:-L?\d+)?|#[\w-]+)?/g;
const FULL_POINTER = new RegExp(`^${POINTER_RE.source}$`);

export const isPointerText = (s: string): boolean => FULL_POINTER.test(s);

/** The directory part of a logical path (`specs/a.md` is `specs`, `a.md` is ``). */
const dirOf = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '');

/** `a/b/../c` to `a/c`; `null` when it would leave the tree. */
export function normalize(path: string): string | null {
  const out: string[] = [];
  for (const seg of path.split('/')) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') { if (!out.pop()) return null; } else out.push(seg);
  }
  return out.join('/');
}

/**
 * The target of a link as written. `base` is the logical path of the document it appears in: a relative target is
 * resolved against it, and a bare `#anchor` points into it. Without `base` a relative target is already a logical path
 * (an agent citing `specs/004.md` in the chat). `null` when it is not something Delphi can open.
 */
export function parseTarget(href: string, base?: string): Target | null {
  const raw = href.trim();
  if (/^(https?:\/\/|mailto:)/i.test(raw)) return { k: 'external', url: raw };
  if (/^[a-z][a-z0-9+.-]*:/i.test(raw)) return null; // `file:`, `javascript:`, anything else: never followed
  const [file, frag] = [raw.split('#')[0]!, raw.includes('#') ? raw.slice(raw.indexOf('#') + 1) : undefined];
  let path: string | null;
  if (file === '') {
    if (!base || frag === undefined) return null;
    path = base;
  } else {
    if (!file.endsWith('.md')) return null;
    path = normalize(file.startsWith('/') || base === undefined ? file : `${dirOf(base)}/${file}`);
  }
  if (!path) return null;
  const range = frag === undefined ? null : /^L(\d+)(?:-L?(\d+))?$/.exec(frag);
  if (range) { const from = Number(range[1]); return { k: 'pointer', path, from, to: range[2] ? Number(range[2]) : from }; }
  return frag ? { k: 'doc', path, anchor: frag } : { k: 'doc', path };
}

/** The same target in the words a status line shows: `specs/a.md § intro`, `specs/a.md L10-L20`, the URL. */
export function describe(t: Target): string {
  return t.k === 'external' ? t.url : t.k === 'pointer' ? `${t.path} L${t.from}${t.to !== t.from ? `-L${t.to}` : ''}` : t.anchor ? `${t.path} § ${t.anchor}` : t.path;
}

/** The target as the text a person copies. */
export function copyText(t: Target): string {
  return t.k === 'external' ? t.url : t.k === 'pointer' ? `${t.path}#L${t.from}-L${t.to}` : t.anchor ? `${t.path}#${t.anchor}` : t.path;
}

/** The logical path a target opens in the reader; external targets have none. */
export const pathOf = (t: Target): string | null => (t.k === 'external' ? null : t.path);

/** The next link after `cur` (or the first), wrapping or stopping at the ends; `cur` null means none is focused. */
export function stepFocus(links: Link[], cur: number | null, dir: 1 | -1): number | null {
  if (!links.length) return null;
  if (cur === null) return dir === 1 ? links[0]!.id : links[links.length - 1]!.id;
  const i = links.findIndex((l) => l.id === cur);
  const n = Math.min(links.length - 1, Math.max(0, (i < 0 ? 0 : i) + dir));
  return links[n]!.id;
}

/** The first link that is wholly or partly inside rows `[top, top + height)`: where focus starts after scrolling. */
export function firstVisible(links: Link[], top: number, height: number): number | null {
  return links.find((l) => l.spans.some((s) => s.line >= top && s.line < top + height))?.id ?? null;
}
