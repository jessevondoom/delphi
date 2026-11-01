// Markdown to ANSI lines: the part of the UI where a library (marked) does the parsing and we only style.
import { Lexer, walkTokens, type Token, type Tokens } from 'marked';
import chalk from 'chalk';
import * as T from './theme.ts';
import { pictureOk, sanitize } from './theme.ts';
import { highlightLine } from './highlight.ts';
import wrapAnsi from 'wrap-ansi';
import stringWidth from 'string-width';
import { isPointerText, parseTarget, POINTER_RE, type Link, type Target } from './links.ts';
import { paint, type Raster } from './image.ts';

/**
 * While `renderMarkdownLinks` runs, links are wrapped in zero-width OSC 8 markers (`delphi-link:<id>`). They survive
 * the wrap, so that afterwards each link's rows and columns can be read off the finished lines (spec 029 §3); the markers
 * are then removed. Outside that call `ctx` is null and rendering is exactly what it was.
 */
type Ctx = { base?: string; focus?: number | null; links: Link[]; width: number; images?: Record<string, Raster | null> };
let ctx: Ctx | null = null;
const OPEN = (id: number) => `\x1b]8;;delphi-link:${id}\x1b\\`;
const CLOSE = '\x1b]8;;\x1b\\';

/** `label` as a link to `target`: styled, registered with the renderer, and marked so its place can be found. */
function linked(label: string, plain: string, target: Target): string {
  const c = ctx!;
  const id = c.links.length;
  c.links.push({ id, label: plain, target, spans: [] });
  return OPEN(id) + (c.focus === id ? chalk.inverse(label) : label) + CLOSE;
}

/** Text with the logical paths in it turned into links (only while links are being collected). */
function withPointers(text: string): string {
  if (!ctx) return text;
  let out = '', last = 0;
  for (const m of text.matchAll(POINTER_RE)) {
    const target = parseTarget(m[0]);
    if (!target) continue;
    out += text.slice(last, m.index) + linked(T.link(m[0]), m[0], target);
    last = m.index! + m[0].length;
  }
  return out + text.slice(last);
}

/** The file an image points at, as the daemon is asked for it: no fragment or query. Null for the web and absolute paths (never fetched). */
export function imageSrc(href: string): string | null {
  const trimmed = href.trim();
  if (trimmed.startsWith('lib:')) {
    const file = trimmed.split(/[?]/)[0]!;
    return /^lib:[a-zA-Z0-9_-]+\/[a-zA-Z0-9._-]+$/.test(file) ? file : null;
  }
  if (trimmed.startsWith('kb:')) {
    return trimmed;
  }
  const h = trimmed.split(/[#?]/)[0]!;
  return h === '' || h.startsWith('/') || /^[a-z][a-z0-9+.-]*:/i.test(h) ? null : h;
}

/** What stands for a picture that is not drawn: its alt text, and where it points when that is the web. */
const imageNote = (i: Tokens.Image): string => chalk.dim(`[image: ${i.text || 'no description'}${imageSrc(i.href) === null ? ` <${i.href}>` : ''}]`);

/** The picture as rows of half-block cells when the reader has it (and the terminal has colour), else its note. */
function picture(i: Tokens.Image): string[] {
  const src = ctx && imageSrc(i.href), r = src ? ctx!.images?.[src] : null;
  return r && pictureOk() ? paint(r, Math.min(ctx!.width, 72)) : [imageNote(i)];
}

/** The relative image files a document points at, in order, once each: what the reader asks the daemon for. */
export function imageRefs(src: string): string[] {
  const seen = new Set<string>();
  walkTokens(new Lexer().lex(sanitize(src)), (t) => { if (t.type === 'image') { const h = imageSrc((t as Tokens.Image).href); if (h) seen.add(h); } });
  return [...seen];
}

const inline = (toks: Token[] | undefined): string =>
  (toks ?? []).map((t) => {
    switch (t.type) {
      case 'strong': return chalk.bold(inline((t as Tokens.Strong).tokens));
      case 'em': return chalk.italic(inline((t as Tokens.Em).tokens));
      case 'codespan': {
        const text = (t as Tokens.Codespan).text;
        const target = ctx && isPointerText(text) ? parseTarget(text) : null;
        return target ? linked(T.accent(text), text, target) : T.accent(text);
      }
      case 'link': {
        const l = t as Tokens.Link;
        const target = ctx ? parseTarget(l.href, ctx.base) : null;
        if (target) return linked(T.link(l.text), l.text, target) + (target.k === 'external' ? chalk.dim(` <${l.href}>`) : '');
        return T.link(l.text) + chalk.dim(` <${l.href}>`);
      }
      case 'br': return '\n';
      case 'image': return imageNote(t as Tokens.Image);
      case 'text': return (t as Tokens.Text).tokens ? inline((t as Tokens.Text).tokens) : withPointers((t as Tokens.Text).text);
      default: return (t as any).raw ?? '';
    }
  }).join('');

function block(t: Token, indent = ''): string[] {
  switch (t.type) {
    case 'heading': return ['', T.heading(inline((t as Tokens.Heading).tokens)), ''];
    case 'paragraph': {
      const only = (t as Tokens.Paragraph).tokens.filter((x) => !(x.type === 'text' && (x as Tokens.Text).text.trim() === ''));
      if (only.length === 1 && only[0]!.type === 'image') return [...picture(only[0] as Tokens.Image), ''];
      return inline((t as Tokens.Paragraph).tokens).split('\n').concat('');
    }
    case 'code': {
      const c = t as Tokens.Code;
      const lang = (c.lang ?? '').split(/\s/)[0]!;
      return [chalk.dim(`  ${lang}`), ...c.text.split('\n').map((l) => '  ' + highlightLine(l, lang)), ''];
    }
    case 'list': {
      const l = t as Tokens.List;
      return l.items.flatMap((it, i) => {
        const mark = l.ordered ? `${(Number(l.start) || 1) + i}. ` : '• ';
        const body = it.tokens.flatMap((x) => block(x)).filter((x, j, a) => !(x === '' && j === a.length - 1));
        return body.map((ln, j) => (j === 0 ? mark : ' '.repeat(mark.length)) + ln);
      }).concat('');
    }
    case 'blockquote': return (t as Tokens.Blockquote).tokens.flatMap((x) => block(x)).map((l) => chalk.dim('│ ') + chalk.italic(l));
    case 'hr': return [chalk.dim('─'.repeat(40)), ''];
    case 'text': return inline((t as Tokens.Text).tokens ?? [t]).split('\n'); // a tight list item's body
    case 'space': return [];
    case 'table': {
      const tb = t as Tokens.Table;
      const cells = (r: Tokens.TableCell[]) => r.map((c) => inline(c.tokens));
      return [chalk.bold(cells(tb.header).join('  │  ')), ...tb.rows.map((r) => cells(r).join('  │  ')), ''];
    }
    default: return [(t as any).raw ?? ''];
  }
}

const ANSI = '(?:\\x1b\\[[0-9;]*m)*';
const LEAD = new RegExp(`^(${ANSI})( *)((?:• |\\d+\\. )?)`);

/**
 * One line wrapped at `width`, continuation rows hanging under the text after its indent and list mark, and no space
 * left at the start of a continuation row (the break falls on the space, which would otherwise sit before the next word).
 */
export function wrapLine(l: string, width: number): string[] {
  const lead = LEAD.exec(l)!;
  const hang = lead[2]!.length + lead[3]!.length;
  const rows = wrapAnsi(l.slice(lead[0].length), Math.max(10, width - hang), { hard: true, trim: false }).split('\n');
  const pad = ' '.repeat(hang);
  return rows.map((r, i) => (i ? pad + r.replace(new RegExp(`^(${ANSI}) +`), '$1') : lead[1]! + lead[2]! + lead[3]! + r));
}

/** Markdown to lines no wider than `width` (ANSI styling preserved across the wrap). */
export function renderMarkdown(src: string, width: number): string[] {
  const lines = new Lexer().lex(sanitize(src)).flatMap((t) => block(t));
  return lines.flatMap((l) => (l === '' ? [''] : wrapLine(l, width)));
}

const OSC = /\x1b\]8;;([^\x07\x1b]*)(?:\x07|\x1b\\)/g;
const TOKENS = /\x1b\]8;;([^\x07\x1b]*)(?:\x07|\x1b\\)|\x1b\[[0-9;]*m|[^\x1b]+|\x1b/g;

/** Reads each link's rows and columns off wrapped lines carrying the markers, then removes the markers. */
function collect(lines: string[], links: Link[]): string[] {
  lines.forEach((line, row) => {
    let col = 0, open: { id: number; col: number } | null = null;
    const close = () => { if (open && col > open.col) links[open.id]!.spans.push({ line: row, col: open.col, width: col - open.col }); open = null; };
    for (const m of line.matchAll(TOKENS)) {
      if (m[1] !== undefined) {
        close();
        const id = /^delphi-link:(\d+)$/.exec(m[1]);
        if (id && links[Number(id[1])]) open = { id: Number(id[1]), col };
      } else if (!m[0].startsWith('\x1b')) col += stringWidth(m[0]);
    }
    close();
  });
  return lines.map((l) => l.replace(OSC, ''));
}

/**
 * `renderMarkdown`, and the links in it: `[label](target)` links and logical paths written as plain text (in prose or in
 * code spans), each with the rows and columns it occupies in the returned lines. `base` is the logical path of the
 * document, for relative links; `focus` is the link to draw reversed; `images` holds the pictures the reader has fetched,
 * by the file they were written as (an image not in it shows as a note with its alt text). Fenced code is left alone.
 */
export function renderMarkdownLinks(src: string, width: number, opts: { base?: string; focus?: number | null; images?: Record<string, Raster | null> } = {}): { lines: string[]; links: Link[] } {
  const links: Link[] = [];
  ctx = { ...opts, links, width };
  try {
    const lines = new Lexer().lex(sanitize(src)).flatMap((t) => block(t));
    return { lines: collect(lines.flatMap((l) => (l === '' ? [''] : wrapLine(l, width))), links), links };
  } finally {
    ctx = null;
  }
}
