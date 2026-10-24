// The prompt editor: text, a cursor, history and the editing keys. Pure functions over an immutable value.
import stringWidth from 'string-width';

export type Composer = { chars: string[]; cur: number; hist: string[]; histAt: number | null; draft: string };

export const empty = (hist: string[] = []): Composer => ({ chars: [], cur: 0, hist, histAt: null, draft: '' });
export const textOf = (c: Composer) => c.chars.join('');

const set = (c: Composer, chars: string[], cur: number): Composer => ({ ...c, chars, cur, histAt: null });

export function insert(c: Composer, s: string): Composer {
  const add = [...s];
  return set(c, [...c.chars.slice(0, c.cur), ...add, ...c.chars.slice(c.cur)], c.cur + add.length);
}
export const backspace = (c: Composer) => (c.cur === 0 ? c : set(c, [...c.chars.slice(0, c.cur - 1), ...c.chars.slice(c.cur)], c.cur - 1));
export const del = (c: Composer) => (c.cur >= c.chars.length ? c : set(c, [...c.chars.slice(0, c.cur), ...c.chars.slice(c.cur + 1)], c.cur));
export const left = (c: Composer) => ({ ...c, cur: Math.max(0, c.cur - 1) });
export const right = (c: Composer) => ({ ...c, cur: Math.min(c.chars.length, c.cur + 1) });

const lineStart = (c: Composer) => { let i = c.cur; while (i > 0 && c.chars[i - 1] !== '\n') i--; return i; };
const lineEnd = (c: Composer) => { let i = c.cur; while (i < c.chars.length && c.chars[i] !== '\n') i++; return i; };
export const home = (c: Composer) => ({ ...c, cur: lineStart(c) });
export const end = (c: Composer) => ({ ...c, cur: lineEnd(c) });
export const killToStart = (c: Composer) => { const a = lineStart(c); return set(c, [...c.chars.slice(0, a), ...c.chars.slice(c.cur)], a); };
export const killToEnd = (c: Composer) => set(c, [...c.chars.slice(0, c.cur), ...c.chars.slice(lineEnd(c))], c.cur);
export function killWord(c: Composer): Composer {
  let i = c.cur;
  while (i > 0 && /\s/.test(c.chars[i - 1]!)) i--;
  while (i > 0 && !/\s/.test(c.chars[i - 1]!)) i--;
  return set(c, [...c.chars.slice(0, i), ...c.chars.slice(c.cur)], i);
}

/** Up and down: through the lines of a multi-line prompt, then through history. */
export function up(c: Composer): Composer {
  const a = lineStart(c);
  if (a > 0) { // a line above
    const col = c.cur - a;
    let ps = a - 1; while (ps > 0 && c.chars[ps - 1] !== '\n') ps--;
    return { ...c, cur: Math.min(ps + col, a - 1) };
  }
  if (!c.hist.length) return c;
  const at = c.histAt === null ? c.hist.length - 1 : Math.max(0, c.histAt - 1);
  const chars = [...c.hist[at]!];
  return { ...c, chars, cur: chars.length, histAt: at, draft: c.histAt === null ? textOf(c) : c.draft };
}
export function down(c: Composer): Composer {
  const e = lineEnd(c);
  if (e < c.chars.length) {
    const col = c.cur - lineStart(c);
    let ne = e + 1; while (ne < c.chars.length && c.chars[ne] !== '\n') ne++;
    return { ...c, cur: Math.min(e + 1 + col, ne) };
  }
  if (c.histAt === null) return c;
  const at = c.histAt + 1;
  const chars = [...(at >= c.hist.length ? c.draft : c.hist[at]!)];
  return { ...c, chars, cur: chars.length, histAt: at >= c.hist.length ? null : at };
}

/** After Enter: the text to send and a fresh composer that remembers it. */
export function submit(c: Composer): { text: string; next: Composer } {
  const text = textOf(c).trim();
  return { text, next: empty(text && c.hist[c.hist.length - 1] !== text ? [...c.hist, text].slice(-200) : c.hist) };
}

export type Layout = { rows: { before: string; at: string; after: string }[]; cursorRow: number };

/** Wrap to `w` columns (prefix excluded) and find the cursor's row. `at` is the cell under the cursor (a space at the end). */
export function layout(c: Composer, w: number): Layout {
  const rows: { before: string; at: string; after: string }[] = [];
  let cur = 0, cursorRow = 0;
  const lines: { cells: string[]; start: number }[] = [];
  let cells: string[] = [], used = 0, start = 0;
  c.chars.forEach((ch, i) => {
    if (ch === '\n') { lines.push({ cells, start }); cells = []; used = 0; start = i + 1; return; }
    const cw = stringWidth(ch) || 1;
    if (used + cw > w) { lines.push({ cells, start }); cells = []; used = 0; start = i; }
    cells.push(ch); used += cw;
  });
  lines.push({ cells, start });
  for (const [li, ln] of lines.entries()) {
    const nextStart = li + 1 < lines.length ? lines[li + 1]!.start : c.chars.length + 1;
    const hasCursor = c.cur >= ln.start && (c.cur < nextStart || li === lines.length - 1) && c.cur - ln.start <= ln.cells.length;
    if (hasCursor) {
      const k = c.cur - ln.start; cursorRow = li; cur = k;
      rows.push({ before: ln.cells.slice(0, k).join(''), at: ln.cells[k] ?? ' ', after: ln.cells.slice(k + 1).join('') });
    } else rows.push({ before: ln.cells.join(''), at: '', after: '' });
  }
  void cur;
  return { rows, cursorRow };
}
