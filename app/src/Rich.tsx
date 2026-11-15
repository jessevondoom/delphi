import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { Bridge } from './bridge.ts';
import { drawDiagram, isMermaid, type Drawn } from './diagram.ts';
import { isWeb, parse, resolvePath, type Block, type Inline } from './md.ts';

/** Where a piece of Markdown came from, so its links and pictures can be followed. Without it they are shown, not followed. */
export type RichContext = {
  bridge?: Bridge | null;
  /** The workspace path of the document this text is, when it is one. */
  docPath?: string;
  /** Open another document of the workspace in the reader. */
  openDoc?: (path: string) => void;
};

const PICTURE = /\.(png|jpe?g|gif)$/i;
const isDocument = (path: string): boolean => /\.(md|mdx|markdown|txt)$/i.test(path);

/** The daemon's own size when none is named, and the most the window may ask for (spec 118 §4 item 3). */
export const PICTURE_SIDE_DEFAULT = 160;
export const PICTURE_SIDE_MAX = 1600;

/** Pixels a side to ask a picture for: as wide as the space it is drawn in, in device pixels, at most 1,600. With no width
 * to measure (a test, a hidden pane) it is the daemon's default. The daemon never enlarges a picture, so asking for more
 * than a picture has costs nothing. */
export function pictureSide(cssWidth: number, dpr: number): number {
  if (!Number.isFinite(cssWidth) || cssWidth <= 0) return PICTURE_SIDE_DEFAULT;
  return Math.max(1, Math.min(PICTURE_SIDE_MAX, Math.ceil(cssWidth * pixelRatio(dpr))));
}

/** Device pixels to a CSS pixel, 1 to 4 (a nonsense ratio is 1). */
function pixelRatio(dpr: number): number {
  return Number.isFinite(dpr) && dpr > 0 ? Math.min(dpr, 4) : 1;
}

/** The image read for a picture beside a document, at `side` pixels a side. */
export function pictureTarget(docPath: string, src: string, side: number): string {
  return `/v1/image?path=${encodeURIComponent(docPath)}&src=${encodeURIComponent(src.split(/[?#]/)[0]!)}&max=${side}`;
}

/** A picture beside a workspace document, asked of the daemon as pixels at the size of the space it is drawn in, and
 * painted on a canvas. A click opens the file. A library picture (`lib:`) is not asked for: the app may show one only
 * with its licence record (spec 118 §4 item 4), which is not built yet. */
function Picture({ src, alt, ctx }: { src: string; alt: string; ctx: RichContext }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const frame = useRef<HTMLButtonElement>(null);
  const path = resolvePath(ctx.docPath, src);
  const local = !!ctx.bridge && !!ctx.docPath && !!path && PICTURE.test(src.split(/[?#]/)[0]!);
  // A picture the window has no way to ask for is its words from the first frame, not an empty box.
  const [state, setState] = useState<'wait' | 'shown' | 'none'>(local ? 'wait' : 'none');

  useEffect(() => {
    if (!local || !ctx.bridge || !ctx.docPath) { setState('none'); return; }
    let gone = false;
    setState('wait');
    // The space the picture may take: the text column it sits in (the button is no wider than its picture yet).
    const room = frame.current?.parentElement?.clientWidth ?? 0;
    const dpr = typeof window !== 'undefined' ? window.devicePixelRatio : 1;
    ctx.bridge.kb(pictureTarget(ctx.docPath, src, pictureSide(room, dpr)))
      .then((r) => {
        if (gone) return;
        const b = r.body as { width?: number; height?: number; rgba?: string };
        const el = canvas.current;
        if (r.status !== 200 || !b.width || !b.height || !b.rgba || !el) { setState('none'); return; }
        const raw = atob(b.rgba);
        const px = new Uint8ClampedArray(raw.length);
        for (let i = 0; i < raw.length; i++) px[i] = raw.charCodeAt(i);
        if (px.length !== b.width * b.height * 4) { setState('none'); return; }
        el.width = b.width;
        el.height = b.height;
        // Drawn at its size in CSS pixels, so a picture asked for in device pixels is sharp, not twice as large.
        el.style.width = `${Math.round(b.width / pixelRatio(dpr))}px`;
        el.getContext('2d')?.putImageData(new ImageData(px, b.width, b.height), 0, 0);
        setState('shown');
      })
      .catch(() => { if (!gone) setState('none'); });
    return () => { gone = true; };
  }, [ctx.bridge, ctx.docPath, src, local]);

  const open = () => { if (path) void ctx.bridge?.open?.(path).catch(() => {}); else if (isWeb(src)) void ctx.bridge?.openUrl?.(src).catch(() => {}); };
  const can = (path && ctx.bridge?.open) || (isWeb(src) && ctx.bridge?.openUrl);
  if (state === 'none') {
    // A picture the window cannot draw (an address on the web, a kind the daemon does not decode) is named, and opens outside.
    return can ? <button type="button" className="md-link" onClick={open}>{alt || src} (picture, opens outside)</button> : <span className="md-alt">{alt || src}</span>;
  }
  return (
    <button ref={frame} type="button" className="md-picture" onClick={open} disabled={!can} aria-label={alt ? `${alt}: open the picture` : 'Open the picture'}>
      <canvas ref={canvas} role="img" aria-label={alt} style={{ visibility: state === 'shown' ? 'visible' : 'hidden' }} />
      {alt && <span className="md-caption">{alt}</span>}
    </button>
  );
}

/** A Mermaid diagram of a document (spec 118 §5 item 4): its text until it is drawn, then a picture of it, with the text a
 * click away; when it cannot be drawn, its text and why. The renderer is loaded the first time a diagram is shown, and
 * what it draws is only ever shown as a picture (`diagram.ts` says how the text is kept from running anything). */
function Diagram({ text }: { text: string }) {
  const host = useRef<HTMLElement>(null);
  const [drawn, setDrawn] = useState<Drawn | null>(null);
  useEffect(() => {
    const el = host.current;
    setDrawn(null);
    if (!el || typeof window === 'undefined') return;
    let gone = false;
    const dark = getComputedStyle(document.documentElement).colorScheme === 'dark';
    // Under the system's reduce-motion setting nothing in the drawing moves.
    const still = !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    import('./mermaid-load.ts')
      .then(({ loadMermaid }) => loadMermaid())
      .then((m) => drawDiagram(m, text, { dark, still, host: el }))
      .catch((e: unknown): Drawn => ({ ok: false, why: e instanceof Error ? e.message : 'the renderer did not start' }))
      .then((d) => { if (!gone) setDrawn(d); });
    return () => { gone = true; };
  }, [text]);
  const code = <pre className="code"><span className="md-lang">mermaid</span>{text}</pre>;
  if (!drawn?.ok) {
    return (
      <figure ref={host} className="md-diagram" aria-busy={drawn === null}>
        {code}
        {drawn && <figcaption className="md-caption">Not drawn: {drawn.why}. Its text is shown.</figcaption>}
      </figure>
    );
  }
  return (
    <figure ref={host} className="md-diagram">
      <img src={drawn.src} width={drawn.width || undefined} height={drawn.height || undefined} alt="A diagram, drawn from its text below" draggable={false} />
      <details><summary className="md-caption">Its text</summary>{code}</details>
    </figure>
  );
}

function Link({ href, ctx, children }: { href: string; ctx: RichContext; children: ReactNode }) {
  const path = resolvePath(ctx.docPath, href);
  let act: (() => void) | null = null;
  let hint = '';
  if (isWeb(href) && ctx.bridge?.openUrl) { act = () => void ctx.bridge!.openUrl!(href).catch(() => {}); hint = 'opens in your browser'; }
  else if (path && isDocument(path) && ctx.openDoc) { act = () => ctx.openDoc!(path); hint = path; }
  else if (path && ctx.bridge?.open) { act = () => void ctx.bridge!.open!(path).catch(() => {}); hint = `${path} (opens outside)`; }
  if (!act) return <span className="md-ref" title={href}>{children}</span>;
  return <button type="button" className="md-link" title={hint} onClick={act}>{children}</button>;
}

function Spans({ c, ctx }: { c: Inline[]; ctx: RichContext }) {
  return (
    <>
      {c.map((x, i) => {
        switch (x.t) {
          case 'text': return <span key={i}>{x.s}</span>;
          case 'code': return <code key={i}>{x.s}</code>;
          case 'strong': return <strong key={i}><Spans c={x.c} ctx={ctx} /></strong>;
          case 'em': return <em key={i}><Spans c={x.c} ctx={ctx} /></em>;
          case 'del': return <del key={i}><Spans c={x.c} ctx={ctx} /></del>;
          case 'link': return <Link key={i} href={x.href} ctx={ctx}><Spans c={x.c} ctx={ctx} /></Link>;
          case 'image': return <Picture key={i} src={x.src} alt={x.alt} ctx={ctx} />;
          default: return <br key={i} />;
        }
      })}
    </>
  );
}

function Blocks({ blocks, ctx }: { blocks: Block[]; ctx: RichContext }) {
  return (
    <>
      {blocks.map((b, i) => {
        switch (b.t) {
          case 'h': {
            const Tag = `h${Math.min(6, b.level + 1)}` as 'h2' | 'h3' | 'h4' | 'h5' | 'h6';
            return <Tag key={i} className={`md-h md-h${b.level}`}><Spans c={b.c} ctx={ctx} /></Tag>;
          }
          case 'p': return <p key={i}><Spans c={b.c} ctx={ctx} /></p>;
          case 'code':
            if (isMermaid(b.lang)) return <Diagram key={i} text={b.s} />;
            return <pre key={i} className="code">{b.lang && <span className="md-lang">{b.lang}</span>}{b.s}</pre>;
          case 'quote': return <blockquote key={i}><Blocks blocks={b.c} ctx={ctx} /></blockquote>;
          case 'hr': return <hr key={i} />;
          case 'image': return <Picture key={i} src={b.src} alt={b.alt} ctx={ctx} />;
          case 'list': {
            const items = b.items.map((it, k) => (
              <li key={k} className={it.task === null ? undefined : 'md-task'}>
                {it.task !== null && <input type="checkbox" checked={it.task} readOnly disabled aria-label={it.task ? 'done' : 'not done'} />}
                <div><Blocks blocks={it.c} ctx={ctx} /></div>
              </li>
            ));
            return b.ordered ? <ol key={i} start={b.start}>{items}</ol> : <ul key={i}>{items}</ul>;
          }
          default:
            return (
              <div key={i} className="table-wrap md-table">
                <table className="table">
                  <thead><tr>{b.head.map((h, k) => <th key={k} scope="col" style={{ textAlign: b.align[k] ?? undefined }}><Spans c={h} ctx={ctx} /></th>)}</tr></thead>
                  <tbody>{b.rows.map((r, k) => <tr key={k}>{r.map((cell, j) => <td key={j} style={{ textAlign: b.align[j] ?? undefined }}><Spans c={cell} ctx={ctx} /></td>)}</tr>)}</tbody>
                </table>
              </div>
            );
        }
      })}
    </>
  );
}

/** Markdown drawn as elements made from data. Every piece of the text ends in a text node; none of it is parsed as HTML. */
export function Rich({ text, ctx = {} }: { text: string; ctx?: RichContext }) {
  return <div className="rich"><Blocks blocks={parse(text)} ctx={ctx} /></div>;
}
