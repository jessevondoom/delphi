// Mermaid diagrams in stored documents, drawn in the window (spec 118 §5 item 4). The renderer is bundled and pinned
// (`@mermaid-js/tiny`, one file with no dependencies, loaded from the app's own files by `mermaid-load.ts`); nothing is
// fetched. A diagram's text is a document's text, so it is treated as hostile:
//
// 1. A diagram whose text asks a style sheet to load anything is not drawn (`asksToLoad`). Mermaid runs in strict mode,
//    and a diagram's own `%%{init}%%` or front matter cannot change that, nor any setting that decides how labels are
//    sanitised or whether they are HTML (`SECURE_KEYS`). Labels are plain SVG text.
// 2. What Mermaid returns is parsed as an inert SVG document (never inserted into the page) and cleaned here: no script,
//    no element that can embed, link or animate, no event attribute, no address other than one inside the drawing, no
//    style that loads anything.
// 3. The cleaned drawing is shown only as an `<img>` of a `data:` address. A picture runs no script, follows no link and
//    loads nothing, whatever it holds.
//
// When anything fails, the window shows the diagram's text instead.

/** The part of Mermaid's interface the window uses. */
export type Mermaid = {
  initialize(config: Record<string, unknown>): void;
  render(id: string, text: string, container?: Element): Promise<{ svg: string }>;
};

/** A diagram longer than this is shown as its text (Mermaid's own default limit). */
export const MAX_DIAGRAM_CHARS = 50_000;

/**
 * Settings a diagram's own directives may not change. Mermaid's own list, and every setting that decides how labels are
 * sanitised (`dompurifyConfig`) or drawn (`htmlLabels`, a flowchart's own), the style sheet and font a drawing carries,
 * and the ids it makes.
 */
export const SECURE_KEYS = [
  'secure', 'securityLevel', 'startOnLoad', 'maxTextSize', 'suppressErrorRendering', 'maxEdges',
  'dompurifyConfig', 'htmlLabels', 'flowchart', 'themeCSS', 'fontFamily', 'arrowMarkerAbsolute',
  'deterministicIds', 'deterministicIDSeed', 'legacyMathML', 'forceLegacyMathML',
] as const;

/** The settings Mermaid runs with. */
export function mermaidConfig(dark: boolean): Record<string, unknown> {
  return {
    startOnLoad: false,
    securityLevel: 'strict',
    secure: [...SECURE_KEYS],
    htmlLabels: false,
    flowchart: { htmlLabels: false },
    maxTextSize: MAX_DIAGRAM_CHARS,
    maxEdges: 500,
    suppressErrorRendering: true,
    deterministicIds: true,
    theme: dark ? 'dark' : 'default',
    fontFamily: 'system-ui, -apple-system, sans-serif',
  };
}

/** Whether a fenced block is a Mermaid diagram. */
export const isMermaid = (lang: string): boolean => lang.trim().toLowerCase() === 'mermaid';

const SVG_NS = 'http://www.w3.org/2000/svg';
const HTML_NS = 'http://www.w3.org/1999/xhtml';
/** The elements a drawing may keep: shapes, text, grouping, gradients, markers, clips and its style sheet. Anything else
 * (script, use, image, iframe, animate, set and the rest) is removed with what it holds; a link (`a`) is replaced by a
 * plain group, so what it held is still drawn but leads nowhere. */
const KEEP = new Set([
  'svg', 'g', 'defs', 'style', 'title', 'desc', 'path', 'rect', 'circle', 'ellipse', 'line', 'polyline', 'polygon',
  'text', 'tspan', 'marker', 'lineargradient', 'radialgradient', 'stop', 'clippath', 'mask', 'pattern', 'symbol',
  'filter', 'fegaussianblur', 'feoffset', 'feflood', 'fecomposite', 'femerge', 'femergenode', 'fedropshadow', 'feblend',
  'fecolormatrix', 'switch', 'foreignobject',
]);
/** Inside a `foreignObject` (a label some diagrams still write as HTML), only text and its emphasis. */
const KEEP_HTML = new Set(['div', 'span', 'p', 'br', 'b', 'i', 'strong', 'em', 'code', 'del', 's', 'u', 'sub', 'sup', 'ul', 'ol', 'li']);
/** Attributes that hold an address. Only an address inside the drawing (`#id`) is kept. */
const ADDRESS = new Set(['href', 'xlink:href', 'src', 'srcset', 'action', 'formaction', 'xml:base', 'background', 'poster', 'data']);
/** A style function that loads something, unless it names a place inside the drawing (`url(#id)`). */
const LOADS = /url\s*\((?!\s*['"]?#)|(?:-webkit-)?image-set\s*\(|@import|@font-face|expression\s*\(/gi;
const loads = (s: string): boolean => new RegExp(LOADS.source, 'i').test(s);

/** An escape that could spell a loading function some other way: CSS (`\75rl(`), JSON or YAML (`u`, `\x75`). */
const ESCAPE = /\\[0-9a-fux]/i;

/** Whether diagram text asks for anything a style sheet could load, or holds an escape that could spell such a request.
 * Mermaid copies `style`, `classDef` and theme text into the drawing it measures in the page, so such a diagram is not
 * drawn at all: its text is shown. */
export const asksToLoad = (text: string): boolean => loads(text) || ESCAPE.test(text);

/** What the drawing's style sheet gets when the operator asked for stillness: nothing in it moves. */
export const STILL_CSS = '*{animation:none!important;transition:none!important}';

/** Style text with every escape and every loading function made inert (an invalid declaration is dropped by CSS). */
const inertCss = (css: string): string => css.replaceAll('\\', '').replace(LOADS, 'invalid(');

/** Whether an element may stay where it is found: inside HTML (a `foreignObject`) or inside SVG. */
function kept(el: Element, html: boolean): boolean {
  const name = el.localName.toLowerCase();
  return html ? el.namespaceURI === HTML_NS && KEEP_HTML.has(name) : el.namespaceURI === SVG_NS && KEEP.has(name);
}

/**
 * Clean a drawing in place, as `KEEP`, `KEEP_HTML`, `ADDRESS` and `LOADS` say. `doc` is an inert document from
 * `DOMParser`. Returns false when the document is not one SVG drawing.
 */
export function cleanSvg(doc: Document, still: boolean): boolean {
  const root = doc.documentElement;
  if (!root || root.localName !== 'svg' || root.namespaceURI !== SVG_NS || doc.getElementsByTagName('parsererror').length) return false;
  const walk = (el: Element, html: boolean) => {
    for (const found of Array.from(el.children)) {
      let child = found;
      if (!html && child.namespaceURI === SVG_NS && child.localName.toLowerCase() === 'a') {
        const g = doc.createElementNS(SVG_NS, 'g');
        while (child.firstChild) g.appendChild(child.firstChild);
        child.replaceWith(g);
        child = g;
      }
      if (!kept(child, html)) { child.remove(); continue; }
      walk(child, html || child.localName.toLowerCase() === 'foreignobject');
    }
    for (const a of Array.from(el.attributes)) {
      const name = a.name.toLowerCase();
      const local = a.localName.toLowerCase();
      const address = ADDRESS.has(name) || ADDRESS.has(local);
      if (name.startsWith('on') || local.startsWith('on') || (address && !a.value.trim().startsWith('#'))) el.removeAttributeNode(a);
      else if (loads(a.value) || a.value.includes('\\')) a.value = inertCss(a.value);
    }
    if (el.localName.toLowerCase() === 'style') el.textContent = inertCss(el.textContent ?? '');
  };
  walk(root, false);
  // Processing instructions (an `xml-stylesheet` among them) and comments outside the drawing are dropped too.
  for (const n of Array.from(doc.childNodes)) if (n !== root && n.nodeType !== 10) doc.removeChild(n);
  if (still) {
    const s = doc.createElementNS(SVG_NS, 'style');
    s.textContent = STILL_CSS;
    root.appendChild(s);
  }
  return true;
}

/** A drawing as a `data:` address for an `<img>`. */
export const svgDataUrl = (svg: string): string => `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;

export type Drawn = { ok: true; src: string; width: number; height: number } | { ok: false; why: string };

let serial = 0;

/**
 * Draw one diagram. `render` is Mermaid's, run with `mermaidConfig`; it draws in a hidden box of `host` (to measure text)
 * and takes it away again. The result is cleaned and made into a picture's address.
 */
export async function drawDiagram(mermaid: Mermaid, text: string, opts: { dark: boolean; still: boolean; host: Element }): Promise<Drawn> {
  if (!text.trim()) return { ok: false, why: 'the diagram is empty' };
  if (text.length > MAX_DIAGRAM_CHARS) return { ok: false, why: 'the diagram is too long to draw' };
  if (asksToLoad(text)) return { ok: false, why: 'the diagram asks a style to load something, or writes in escapes' };
  let svg: string;
  const box = opts.host.ownerDocument.createElement('div');
  box.setAttribute('aria-hidden', 'true');
  box.style.cssText = 'position:absolute;left:-10000px;top:0;width:1200px;visibility:hidden;pointer-events:none';
  opts.host.appendChild(box);
  try {
    mermaid.initialize(mermaidConfig(opts.dark));
    svg = (await mermaid.render(`delphi-diagram-${++serial}`, text, box)).svg;
  } catch (e) {
    return { ok: false, why: firstLine(e) };
  } finally {
    box.remove();
  }
  const doc = new DOMParser().parseFromString(svg, 'image/svg+xml');
  if (!cleanSvg(doc, opts.still)) return { ok: false, why: 'the renderer did not return a drawing' };
  const root = doc.documentElement;
  const [, , vw, vh] = (root.getAttribute('viewBox') ?? '').split(/[\s,]+/).map(Number);
  const width = vw !== undefined && Number.isFinite(vw) && vw > 0 ? Math.ceil(vw) : 0;
  const height = vh !== undefined && Number.isFinite(vh) && vh > 0 ? Math.ceil(vh) : 0;
  // A picture takes its size from its own width and height; Mermaid's percentage width means nothing in an <img>.
  if (width && height) { root.setAttribute('width', String(width)); root.setAttribute('height', String(height)); }
  root.removeAttribute('style');
  return { ok: true, src: svgDataUrl(new XMLSerializer().serializeToString(doc)), width, height };
}

/** The first line of an error, short: Mermaid's parse errors are long and quote the text back. */
function firstLine(e: unknown): string {
  const s = (e instanceof Error ? e.message : String(e)).split('\n')[0]!.trim();
  return s.length > 160 ? `${s.slice(0, 157)}...` : s || 'the renderer refused it';
}
