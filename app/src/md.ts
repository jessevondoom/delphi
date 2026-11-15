// Markdown, as the window draws it (spec 111 §5): headings, paragraphs, emphasis, links, images, lists with tasks, quotes,
// tables, rules and fenced code. This turns text into plain data; the component makes elements from it, so nothing an agent or
// a document wrote is ever read as HTML (§2.5). Raw HTML in the text stays text. Pure.

export type Inline =
  | { t: 'text'; s: string }
  | { t: 'code'; s: string }
  | { t: 'strong' | 'em' | 'del'; c: Inline[] }
  | { t: 'link'; href: string; c: Inline[] }
  | { t: 'image'; src: string; alt: string }
  | { t: 'br' };

export type Block =
  | { t: 'p'; c: Inline[] }
  | { t: 'h'; level: number; c: Inline[] }
  | { t: 'code'; lang: string; s: string }
  | { t: 'quote'; c: Block[] }
  | { t: 'list'; ordered: boolean; start: number; items: { task: boolean | null; c: Block[] }[] }
  | { t: 'table'; head: Inline[][]; align: ('left' | 'right' | 'center' | null)[]; rows: Inline[][][] }
  | { t: 'image'; src: string; alt: string }
  | { t: 'hr' };

// One alternation, tried left to right at each position: an escape, a code span, an image, a link, an autolink, a bare address,
// then strong, strike and emphasis.
const INLINE = new RegExp(
  [
    '\\\\([\\\\`*_{}\\[\\]()#+\\-.!~|>])', // 1 escape
    '(`+)(?!`)([\\s\\S]*?[^`])\\2(?!`)', // 2,3 code
    '!\\[([^\\]]*)\\]\\(\\s*<?([^)\\s>]+)>?(?:\\s+"[^"]*")?\\s*\\)', // 4,5 image
    '\\[((?:[^\\[\\]]|\\[[^\\]]*\\])+)\\]\\(\\s*<?([^)\\s>]+)>?(?:\\s+"[^"]*")?\\s*\\)', // 6,7 link
    '<((?:https?:\\/\\/|mailto:)[^>\\s]+)>', // 8 autolink
    '(https?:\\/\\/[^\\s<>()\\[\\]]*[^\\s<>()\\[\\].,;:!?\'"])', // 9 bare address
    '\\*\\*(?=\\S)([\\s\\S]*?\\S)\\*\\*', // 10 strong
    '__(?=\\S)([\\s\\S]*?\\S)__(?![A-Za-z0-9])', // 11 strong
    '~~(?=\\S)([\\s\\S]*?\\S)~~', // 12 strike
    '\\*(?=\\S)([^*]*?[^\\s*])\\*', // 13 emphasis
    '(?<![A-Za-z0-9_])_(?=\\S)([^_]*?[^\\s_])_(?![A-Za-z0-9])', // 14 emphasis
  ].join('|'),
  'g',
);

/** The inline parts of one run of text. Anything unbalanced stays as written. */
export function inline(text: string): Inline[] {
  const out: Inline[] = [];
  const push = (s: string) => {
    if (!s) return;
    const last = out[out.length - 1];
    if (last?.t === 'text') last.s += s;
    else out.push({ t: 'text', s });
  };
  INLINE.lastIndex = 0;
  let at = 0;
  for (let m = INLINE.exec(text); m; m = INLINE.exec(text)) {
    push(text.slice(at, m.index));
    at = m.index + m[0].length;
    if (m[1] !== undefined) push(m[1]);
    else if (m[3] !== undefined) out.push({ t: 'code', s: m[3].replace(/^ (.*) $/s, '$1') });
    else if (m[5] !== undefined) out.push({ t: 'image', src: m[5], alt: m[4] ?? '' });
    else if (m[7] !== undefined) out.push({ t: 'link', href: m[7], c: inlineNested(m[6]!) });
    else if (m[8] !== undefined) out.push({ t: 'link', href: m[8], c: [{ t: 'text', s: m[8].replace(/^mailto:/, '') }] });
    else if (m[9] !== undefined) out.push({ t: 'link', href: m[9], c: [{ t: 'text', s: m[9] }] });
    else if (m[10] !== undefined || m[11] !== undefined) out.push({ t: 'strong', c: inlineNested((m[10] ?? m[11])!) });
    else if (m[12] !== undefined) out.push({ t: 'del', c: inlineNested(m[12]) });
    else out.push({ t: 'em', c: inlineNested((m[13] ?? m[14])!) });
  }
  push(text.slice(at));
  return out;
}

// The shared regular expression keeps its place between calls, so a nested run saves and restores it.
function inlineNested(text: string): Inline[] {
  const keep = INLINE.lastIndex;
  const r = inline(text);
  INLINE.lastIndex = keep;
  return r;
}

/** A paragraph's lines as inline parts: a line that ends in two spaces or a backslash breaks; the others run on. */
function lines(ls: string[]): Inline[] {
  const out: Inline[] = [];
  ls.forEach((l, i) => {
    const hard = /( {2,}|\\)$/.test(l) && i < ls.length - 1;
    out.push(...inline(l.replace(/( {2,}|\\)$/, '').trim()));
    if (i < ls.length - 1) out.push(hard ? { t: 'br' } : { t: 'text', s: ' ' });
  });
  // Runs of text joined by the line ends become one.
  return out.reduce<Inline[]>((acc, x) => {
    const last = acc[acc.length - 1];
    if (x.t === 'text' && last?.t === 'text') last.s += x.s;
    else acc.push(x);
    return acc;
  }, []);
}

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([\w+#.-]*)[^`]*$/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(?:\|\s*:?-+:?\s*)*\|?\s*$/;
const ONLY_IMAGE = /^\s*!\[([^\]]*)\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)\s*$/;

const cells = (line: string): string[] => {
  const t = line.trim().replace(/^\|/, '').replace(/\|$/, '');
  // A `|` inside a code span or after a backslash does not end a cell.
  const out: string[] = [];
  let cur = '', tick = false;
  for (let i = 0; i < t.length; i++) {
    const ch = t[i]!;
    if (ch === '\\' && t[i + 1] === '|') { cur += '|'; i++; continue; }
    if (ch === '`') tick = !tick;
    if (ch === '|' && !tick) { out.push(cur.trim()); cur = ''; continue; }
    cur += ch;
  }
  out.push(cur.trim());
  return out;
};

/** The blocks of a document. `depth` stops a quote or a list nested without end. */
export function parse(text: string, depth = 0): Block[] {
  const src = text.replace(/\r\n?/g, '\n').replace(/<!--[\s\S]*?-->/g, '').split('\n');
  const out: Block[] = [];
  let i = 0;
  const startsBlock = (l: string, next?: string) => FENCE.test(l) || HEADING.test(l) || RULE.test(l) || /^ {0,3}>/.test(l) || ITEM.test(l) || (l.includes('|') && next !== undefined && TABLE_RULE.test(next) && next.includes('-'));
  while (i < src.length) {
    const line = src[i]!;
    if (!line.trim()) { i++; continue; }
    const fence = FENCE.exec(line);
    if (fence) {
      const body: string[] = [];
      const close = new RegExp(`^ {0,3}${fence[1]![0] === '`' ? '`' : '~'}{${fence[1]!.length},}\\s*$`);
      for (i++; i < src.length && !close.test(src[i]!); i++) body.push(src[i]!);
      i++;
      out.push({ t: 'code', lang: fence[2] ?? '', s: body.join('\n') });
      continue;
    }
    const head = HEADING.exec(line);
    if (head) { out.push({ t: 'h', level: head[1]!.length, c: inline(head[2]!) }); i++; continue; }
    if (RULE.test(line)) { out.push({ t: 'hr' }); i++; continue; }
    if (/^ {0,3}>/.test(line)) {
      const body: string[] = [];
      for (; i < src.length && (/^ {0,3}>/.test(src[i]!) || (src[i]!.trim() !== '' && body.length > 0 && !startsBlock(src[i]!, src[i + 1]))); i++) body.push(src[i]!.replace(/^ {0,3}> ?/, ''));
      out.push({ t: 'quote', c: depth < 6 ? parse(body.join('\n'), depth + 1) : [{ t: 'p', c: inline(body.join(' ')) }] });
      continue;
    }
    if (line.includes('|') && src[i + 1] !== undefined && TABLE_RULE.test(src[i + 1]!) && src[i + 1]!.includes('-')) {
      const head = cells(line).map(inline);
      const align = cells(src[i + 1]!).map((c) => (/^:-+:$/.test(c) ? 'center' : /-+:$/.test(c) ? 'right' : /^:-+/.test(c) ? 'left' : null));
      const rows: Inline[][][] = [];
      for (i += 2; i < src.length && src[i]!.trim() !== '' && src[i]!.includes('|'); i++) {
        const row = cells(src[i]!).map(inline);
        while (row.length < head.length) row.push([]);
        rows.push(row.slice(0, head.length));
      }
      out.push({ t: 'table', head, align: head.map((_, k) => align[k] ?? null), rows });
      continue;
    }
    const first = ITEM.exec(line);
    if (first) {
      const ordered = /\d/.test(first[2]!);
      const indent = first[1]!.length;
      const items: { task: boolean | null; c: Block[] }[] = [];
      while (i < src.length) {
        const m = ITEM.exec(src[i]!);
        if (!m || m[1]!.length > indent + 1 || /\d/.test(m[2]!) !== ordered || m[1]!.length < indent) break;
        const pad = m[1]!.length + m[2]!.length + 1;
        const body = [m[3]!];
        for (i++; i < src.length; i++) {
          const l = src[i]!;
          if (!l.trim()) {
            // A blank line stays in the item only when what follows is indented under it.
            const next = src.slice(i + 1).find((x) => x.trim() !== '');
            if (next === undefined || next.search(/\S/) < Math.min(pad, indent + 2)) break;
            body.push('');
            continue;
          }
          const lead = l.search(/\S/);
          if (lead <= indent && (ITEM.test(l) || startsBlock(l, src[i + 1]))) break;
          body.push(lead >= pad ? l.slice(pad) : lead > indent ? l.slice(Math.min(lead, pad)) : l.trim());
        }
        const task = /^\[( |x|X)\]\s+/.exec(body[0]!);
        if (task) body[0] = body[0]!.slice(task[0].length);
        items.push({ task: task ? task[1] !== ' ' : null, c: depth < 6 ? parse(body.join('\n'), depth + 1) : [{ t: 'p', c: inline(body.join(' ')) }] });
        while (i < src.length && !src[i]!.trim()) i++;
      }
      out.push({ t: 'list', ordered, start: ordered ? parseInt(first[2]!, 10) || 1 : 1, items });
      continue;
    }
    const only = ONLY_IMAGE.exec(line);
    if (only) { out.push({ t: 'image', src: only[2]!, alt: only[1] ?? '' }); i++; continue; }
    const para: string[] = [];
    for (; i < src.length && src[i]!.trim() !== '' && (para.length === 0 || !startsBlock(src[i]!, src[i + 1])); i++) para.push(src[i]!);
    out.push({ t: 'p', c: lines(para) });
  }
  return out;
}

/** Where a link or a picture written in a document points, as a path in the workspace; nothing for an address or an anchor. */
export function resolvePath(docPath: string | undefined, href: string): string | null {
  if (!docPath || !href || /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('#') || href.startsWith('//')) return null;
  const clean = href.split(/[?#]/)[0]!;
  let decoded = clean;
  try { decoded = decodeURIComponent(clean); } catch { /* as written */ }
  const base = decoded.startsWith('/') ? [] : docPath.split('/').slice(0, -1);
  for (const part of decoded.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') { if (base.length === 0) return null; base.pop(); } else base.push(part);
  }
  return base.length ? base.join('/') : null;
}

/** Whether an address is one the window will hand to the system's browser. */
export const isWeb = (href: string): boolean => /^https?:\/\/[^\s]+$/i.test(href);
