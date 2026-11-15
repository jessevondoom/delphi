import { useEffect, useState, type ReactNode } from 'react';
import { Agent } from './Badge.tsx';
import { Rich } from './Rich.tsx';
import { spec } from './avatar.ts';
import type { Bridge } from './bridge.ts';
import { dayOf, docTarget, frontmatter, sourceWords, titleOf, type Doc, type Hit } from './kb.ts';
import type { Info } from './types.ts';

type Props = { bridge: Bridge | null; info: Info | null; light: boolean; path: string; hit?: Hit; lead?: ReactNode; over?: ReactNode };

// Front matter the facts already show, or that only repeats the title.
const SHOWN = new Set(['title', 'name', 'author', 'source_type', 'confidence', 'status', 'timestamp', 'tags', 'id']);
const tagsOf = (v?: string): string[] => (v ? v.replace(/^\[|\]$/g, '').split(',').map((t) => t.trim().replace(/^["']|["']$/g, '')).filter(Boolean) : []);

/**
 * One document from the workspace (spec 111 §5.3): its front matter as facts, the passage a search matched, and its Markdown
 * drawn in full. A link to another document opens it here, with a way back; a link to a page, a deck, a picture or a recording
 * opens outside the window. Read-only.
 */
export function DocView({ bridge, info, light, path, hit, lead, over }: Props) {
  const [trail, setTrail] = useState<string[]>([]);
  const [doc, setDoc] = useState<Doc | null>(null);
  const [problem, setProblem] = useState('');
  const [said, setSaid] = useState('');
  const at = trail[trail.length - 1] ?? path;

  useEffect(() => { setTrail([]); }, [path]);
  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    setDoc(null);
    setProblem('');
    setSaid('');
    bridge.kb(docTarget(at))
      .then((r) => { if (gone) return; if (r.status === 200) setDoc(r.body as Doc); else setProblem(r.status === 404 ? 'The knowledge base does not have this document.' : `The knowledge base answered ${r.status}.`); })
      .catch((e: unknown) => { if (!gone) setProblem(e instanceof Error ? e.message : String(e)); });
    return () => { gone = true; };
  }, [bridge, at]);

  const back = trail.length > 0 && <button type="button" className="btn small" onClick={() => setTrail(trail.slice(0, -1))}>Back</button>;
  if (!doc) return <>{back}<div className="empty">{problem || 'Opening the document.'}</div></>;
  const { meta, body } = frontmatter(doc.text);
  const author = meta.author && meta.author in spec(info).roles ? meta.author : '';
  const shownHit = trail.length === 0 ? hit : undefined;
  const others = Object.entries(meta).filter(([k]) => !SHOWN.has(k));
  const tags = tagsOf(meta.tags);
  const copy = () => { void navigator.clipboard?.writeText(doc.path).then(() => setSaid('Path copied.'), () => setSaid('The path could not be copied.')); };
  const show = () => { void bridge?.reveal?.(doc.path).then(() => setSaid(''), (e: unknown) => setSaid(String(e))); };
  return (
    <>
      <div className="row" style={{ fontSize: 12 }}>
        {back}
        <span className="chip line">{doc.collection}</span>
        {shownHit && <span className={shownHit.label === 'validated' ? 'chip ok' : 'chip warn'}>{shownHit.label}</span>}
        {doc.stale && <span className="chip warn">changed since it was indexed</span>}
        <span className="time" style={{ overflowWrap: 'anywhere' }}>{shownHit?.pointer ?? doc.path}</span>
      </div>
      {lead && trail.length === 0 ? <div className="lead">{lead}<div className="stack">{over}<h1 className="question" style={{ fontSize: 26 }}>{titleOf(doc.path, doc.text)}</h1></div></div> : <h1 className="question" style={{ fontSize: 26 }}>{titleOf(doc.path, doc.text)}</h1>}
      <dl className="facts">
        {(author || meta.author) && <div><dt>Author</dt><dd>{author ? <Agent info={info} role={author} light={light} /> : meta.author}</dd></div>}
        {meta.source_type && <div><dt>Source</dt><dd>{sourceWords(meta.source_type)}</dd></div>}
        {meta.confidence && <div><dt>Confidence</dt><dd>{meta.confidence}</dd></div>}
        <div><dt>Status</dt><dd>{meta.status ?? doc.status}</dd></div>
        {dayOf(meta.timestamp) && <div><dt>Written</dt><dd>{dayOf(meta.timestamp)}</dd></div>}
        {shownHit?.vouched_by && <div><dt>Vouched for by</dt><dd>{shownHit.vouched_by}</dd></div>}
        {others.map(([k, v]) => <div key={k}><dt>{k.replace(/_/g, ' ')}</dt><dd style={{ overflowWrap: 'anywhere' }}>{v}</dd></div>)}
      </dl>
      {tags.length > 0 && <div className="row" aria-label="Tags">{tags.map((t) => <span key={t} className="chip line">{t}</span>)}</div>}
      <div className="row">
        {bridge?.reveal && <button type="button" className="btn" onClick={show}>Show in Finder</button>}
        <button type="button" className="btn" onClick={copy}>Copy path</button>
        {said && <span className="muted" style={{ fontSize: 12 }} role="status">{said}</span>}
      </div>
      {shownHit && <div className="card" style={{ background: 'var(--sel)', fontSize: 14 }}><div className="caps">Matched passage</div><div className="said"><Rich text={shownHit.preview} /></div>{shownHit.note && <div className="muted" style={{ fontSize: 12 }}>{shownHit.note}</div>}</div>}
      <div className="said doc"><Rich text={body} ctx={{ bridge, docPath: doc.path, openDoc: (p) => setTrail([...trail, p]) }} /></div>
    </>
  );
}
