import { useEffect, useState } from 'react';
import { Badge, Who } from './Badge.tsx';
import { Rich } from './Rich.tsx';
import { spec } from './avatar.ts';
import type { Bridge } from './bridge.ts';
import { dayOf, docTarget, frontmatter, sourceWords, titleOf, type Doc, type Hit } from './kb.ts';
import type { Info } from './types.ts';

type Props = { bridge: Bridge | null; info: Info | null; light: boolean; path: string; hit?: Hit };

/** One document from the workspace: its front matter as facts, the passage a search matched, and its text. Read-only. */
export function DocView({ bridge, info, light, path, hit }: Props) {
  const [doc, setDoc] = useState<Doc | null>(null);
  const [problem, setProblem] = useState('');
  const [said, setSaid] = useState('');

  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    setDoc(null);
    setProblem('');
    setSaid('');
    bridge.kb(docTarget(path))
      .then((r) => { if (gone) return; if (r.status === 200) setDoc(r.body as Doc); else setProblem(r.status === 404 ? 'The knowledge base no longer has this document.' : `The knowledge base answered ${r.status}.`); })
      .catch((e: unknown) => { if (!gone) setProblem(e instanceof Error ? e.message : String(e)); });
    return () => { gone = true; };
  }, [bridge, path]);

  if (!doc) return <div className="empty">{problem || 'Opening the document.'}</div>;
  const { meta, body } = frontmatter(doc.text);
  const author = meta.author && meta.author in spec(info).roles ? meta.author : '';
  const copy = () => { void navigator.clipboard?.writeText(doc.path).then(() => setSaid('Path copied.'), () => setSaid('The path could not be copied.')); };
  const show = () => { void bridge?.reveal?.(doc.path).then(() => setSaid(''), (e: unknown) => setSaid(String(e))); };
  return (
    <>
      <div className="row" style={{ fontSize: 12 }}>
        <span className="chip line">{doc.collection}</span>
        {hit && <span className={hit.label === 'validated' ? 'chip ok' : 'chip warn'}>{hit.label}</span>}
        {doc.stale && <span className="chip warn">changed since it was indexed</span>}
        <span className="time" style={{ overflowWrap: 'anywhere' }}>{hit?.pointer ?? doc.path}</span>
      </div>
      <h1 className="question" style={{ fontSize: 26 }}>{titleOf(doc.path, doc.text)}</h1>
      <dl className="facts">
        {(author || meta.author) && <div><dt>Author</dt><dd>{author ? <><Badge info={info} role={author} light={light} size={13} /> <Who info={info} role={author} light={light} /></> : meta.author}</dd></div>}
        {meta.source_type && <div><dt>Source</dt><dd>{sourceWords(meta.source_type)}</dd></div>}
        {meta.confidence && <div><dt>Confidence</dt><dd>{meta.confidence}</dd></div>}
        <div><dt>Status</dt><dd>{meta.status ?? doc.status}</dd></div>
        {dayOf(meta.timestamp) && <div><dt>Written</dt><dd>{dayOf(meta.timestamp)}</dd></div>}
        {hit?.vouched_by && <div><dt>Vouched for by</dt><dd>{hit.vouched_by}</dd></div>}
      </dl>
      <div className="row">
        {bridge?.reveal && <button type="button" className="btn" onClick={show}>Show in Finder</button>}
        <button type="button" className="btn" onClick={copy}>Copy path</button>
        {said && <span className="muted" style={{ fontSize: 12 }} role="status">{said}</span>}
      </div>
      {hit && <div className="card" style={{ background: 'var(--sel)', fontSize: 14 }}><div className="caps">Matched passage</div><div className="said"><Rich text={hit.preview} /></div>{hit.note && <div className="muted" style={{ fontSize: 12 }}>{hit.note}</div>}</div>}
      <div className="said" style={{ fontSize: 15, lineHeight: 1.65, maxWidth: 760 }}><Rich text={body} /></div>
    </>
  );
}
