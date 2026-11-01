// Links as data (spec 029 §3): what a link points at, where it sits in rendered text, and how a surface moves through
// them. Pure: nothing here draws or reads a file. The Markdown renderer produces `Link`s (`renderMarkdownLinks`), the
// reader and every surface that wants link focus consume them.
import stringWidth from 'string-width';

/** What a link points at. External targets are never fetched by Delphi: the system browser opens them on request. */
export type Target =
  | { k: 'doc'; path: string; anchor?: string }
  | { k: 'pointer'; path: string; from: number; to: number }
  | { k: 'external'; url: string }
  | { k: 'page'; slug: string };

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
  if (raw.startsWith('page:')) {
    const slug = raw.slice(5).trim();
    return slug ? { k: 'page', slug } : null;
  }
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
  return t.k === 'external' ? t.url : t.k === 'pointer' ? `${t.path} L${t.from}${t.to !== t.from ? `-L${t.to}` : ''}` : t.k === 'page' ? `page: ${t.slug}` : t.anchor ? `${t.path ? t.path + ' ' : ''}§ ${t.anchor}` : t.path;
}

/** The target as the text a person copies. */
export function copyText(t: Target): string {
  return t.k === 'external' ? t.url : t.k === 'pointer' ? `${t.path}#L${t.from}-L${t.to}` : t.k === 'page' ? `page:${t.slug}` : t.anchor ? `${t.path}#${t.anchor}` : t.path;
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

// ── links found in finished rows ────────────────────────────────────────────
// The chat and the change feed are built from many pieces (rails, tinted blocks, tool rows). Rather than thread the
// renderer through each, links are read off the finished rows by their text: a logical path or an `http(s)` address,
// at the row and column it was drawn. A link a wrap split across two rows is not found.

const URL_RE = /https?:\/\/[^\s<>"'`)\]]+/g;
const MARKUP = /\x1b\[[0-9;]*m|\x1b\]8;;[^\x07\x1b]*(?:\x07|\x1b\\)/g;
const TOKENS = /\x1b\[[0-9;]*m|\x1b\]8;;[^\x07\x1b]*(?:\x07|\x1b\\)|[^\x1b]+|\x1b/g;

/** The links in `lines`, in reading order, ids from 0. Pointers and addresses written as text; the rows may carry styling. */
export function scanLinks(lines: string[]): Link[] {
  const links: Link[] = [];
  lines.forEach((line, row) => {
    const plain = line.replace(MARKUP, '');
    const found: { at: number; text: string; target: Target }[] = [];
    for (const m of plain.matchAll(POINTER_RE)) { const target = parseTarget(m[0]); if (target) found.push({ at: m.index!, text: m[0], target }); }
    for (const m of plain.matchAll(URL_RE)) {
      const text = m[0].replace(/[.,;:!?]+$/, '');
      const target = parseTarget(text);
      if (target && !found.some((f) => m.index! < f.at + f.text.length && f.at < m.index! + text.length)) found.push({ at: m.index!, text, target });
    }
    for (const f of found.sort((a, b) => a.at - b.at)) {
      links.push({ id: links.length, label: f.text, target: f.target, spans: [{ line: row, col: stringWidth(plain.slice(0, f.at)), width: stringWidth(f.text) }] });
    }
  });
  return links;
}

/** `line` with the columns `[col, col + width)` drawn reversed, whatever styling is already there. */
export function reverseSpan(line: string, col: number, width: number): string {
  let out = '', at = 0, on = false;
  const end = col + width;
  for (const m of line.matchAll(TOKENS)) {
    const tok = m[0];
    if (tok.startsWith('\x1b')) { out += tok; continue; }
    for (const ch of tok) {
      if (!on && at >= col && at < end) { out += '\x1b[7m'; on = true; }
      if (on && at >= end) { out += '\x1b[27m'; on = false; }
      out += ch;
      at += stringWidth(ch);
    }
  }
  return on ? out + '\x1b[27m' : out;
}

/** `lines` with link `focus` drawn reversed. */
export function paintFocus(lines: string[], links: Link[], focus: number | null): string[] {
  const l = focus === null ? undefined : links.find((x) => x.id === focus);
  if (!l) return lines;
  const out = lines.slice();
  for (const s of l.spans) if (out[s.line] !== undefined) out[s.line] = reverseSpan(out[s.line]!, s.col, s.width);
  return out;
}
