import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { Bridge } from './bridge.ts';
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

/** A picture beside a workspace document, asked of the daemon as pixels and painted on a canvas. A click opens the file. */
function Picture({ src, alt, ctx }: { src: string; alt: string; ctx: RichContext }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const path = resolvePath(ctx.docPath, src);
  const local = !!ctx.bridge && !!ctx.docPath && !!path && PICTURE.test(src.split(/[?#]/)[0]!);
  // A picture the window has no way to ask for is its words from the first frame, not an empty box.
  const [state, setState] = useState<'wait' | 'shown' | 'none'>(local ? 'wait' : 'none');

  useEffect(() => {
    if (!local || !ctx.bridge || !ctx.docPath) { setState('none'); return; }
    let gone = false;
    setState('wait');
    ctx.bridge.kb(`/v1/image?path=${encodeURIComponent(ctx.docPath)}&src=${encodeURIComponent(src.split(/[?#]/)[0]!)}`)
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
    <button type="button" className="md-picture" onClick={open} disabled={!can} aria-label={alt ? `${alt}: open the picture` : 'Open the picture'}>
      <canvas ref={canvas} role="img" aria-label={alt} style={{ visibility: state === 'shown' ? 'visible' : 'hidden' }} />
      {alt && <span className="md-caption">{alt}</span>}
    </button>
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
          case 'code': return <pre key={i} className="code">{b.lang && <span className="md-lang">{b.lang}</span>}{b.s}</pre>;
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
