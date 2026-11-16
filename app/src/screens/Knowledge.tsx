import { useEffect, useState, type FormEvent } from 'react';
import { DocView } from '../DocView.tsx';
import type { Bridge } from '../bridge.ts';
import { dayOf, docInAddress, headingOf, listTarget, searchTarget, sourceWords, titleOf, type Collection, type DocRow, type Hit, type SearchBody } from '../kb.ts';
import type { Info, OState } from '../types.ts';

type Props = { bridge: Bridge | null; info: Info | null; state: OState | null; light: boolean; /** A search handed over from another screen: it runs at once. */ ask?: string };

/** Ask the bridge for one read and take its body when the daemon said 200; anything else is a sentence for the screen. */
async function read<T>(bridge: Bridge, target: string): Promise<T> {
  const r = await bridge.kb(target);
  if (r.status !== 200) throw new Error(`the knowledge base answered ${r.status}`);
  return r.body as T;
}

/**
 * Screen 3 (spec 111 §5.3): a start page (search, what is on the shelf, what changed last), a page of results, and a document
 * across the full width with a way back. One of the three shows at a time; there is no column beside a document.
 */
export function Knowledge({ bridge, info, state, light, ask }: Props) {
  // A search named in the address (`?q=`) runs at once: a link to a search, for a browser and for looking at the page.
  const asked = ask?.trim() || (typeof location !== 'undefined' ? (new URLSearchParams(location.search).get('q') ?? '').trim() : '');
  const [text, setText] = useState(asked);
  const [query, setQuery] = useState(asked);
  const [kind, setKind] = useState('');
  // Spec 115 §4 item 1: every project's research, or the one project the operator narrowed to.
  const [project, setProject] = useState('');
  const [found, setFound] = useState<SearchBody | null>(null);
  const [open, setOpen] = useState<{ path: string; hit?: Hit } | null>(() => { const p = typeof location !== 'undefined' ? docInAddress(location.search) : ''; return p ? { path: p } : null; });
  const [shelf, setShelf] = useState<Collection[]>([]);
  const [recent, setRecent] = useState<DocRow[]>([]);
  const [problem, setProblem] = useState('');

  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    read<Collection[]>(bridge, '/v1/collections').then((c) => { if (!gone) setShelf(c); }).catch(() => {});
    read<DocRow[]>(bridge, listTarget(8)).then((d) => { if (!gone) setRecent(d); }).catch(() => {});
    return () => { gone = true; };
  }, [bridge]);

  useEffect(() => {
    if (!bridge || !query) return;
    let gone = false;
    setProblem('');
    read<SearchBody>(bridge, searchTarget(query, kind || undefined, 20, project || undefined))
      .then((b) => { if (!gone) setFound(b); })
      .catch((e: unknown) => { if (!gone) { setFound(null); setProblem(e instanceof Error ? e.message : String(e)); } });
    return () => { gone = true; };
  }, [bridge, query, kind, project]);

  const submit = (e: FormEvent) => { e.preventDefault(); const q = text.trim(); if (q) { setOpen(null); setKind(''); setProject(''); setQuery(q); } };
  const home = () => { setQuery(''); setFound(null); setOpen(null); setText(''); setProblem(''); setProject(''); };
  const search = (large: boolean) => (
    <form onSubmit={submit} className="stack" style={{ gap: 6 }} role="search">
      <label htmlFor="kbq" className="muted" style={{ fontSize: 12 }}>Search entries, specs and lessons</label>
      <input id="kbq" className={large ? 'field large' : 'field'} type="search" value={text} placeholder="Ask in your own words, or type a few terms" onChange={(e) => setText(e.target.value)} />
    </form>
  );

  if (open) {
    return (
      <main className="main page">
        <DocView bridge={bridge} info={info} light={light} path={open.path} hit={open.hit} back={{ label: query ? `Back to results for “${query}”` : 'Back to the knowledge base', go: () => setOpen(null) }} />
      </main>
    );
  }

  if (!query) {
    return (
      <main className="main" style={{ alignItems: 'center', paddingTop: 40 }}>
        <div className="stack" style={{ width: '100%', maxWidth: 920, gap: 28 }}>
          <div className="stack" style={{ gap: 8 }}>
            <h1 className="title" style={{ fontSize: 28 }}>Knowledge base</h1>
            <p style={{ fontSize: 15, color: 'var(--text2)' }}>What the team knows, where it came from, and what is waiting on you.</p>
          </div>
          {search(true)}
          {!!state?.pending_proposals && (
            <section className="card gold">
              <strong>{state.pending_proposals === 1 ? '1 proposed entry is waiting for your review' : `${state.pending_proposals} proposed entries are waiting for your review`}</strong>
              <div style={{ fontSize: 12, color: 'var(--text2)' }}>This app is read-only. Keep or drop them in the console.</div>
            </section>
          )}
          <section className="stack">
            <h2 className="caps">On the shelf</h2>
            {shelf.length === 0 ? <div className="empty">The knowledge base has not answered yet.</div> : (
              <div className="tiles">
                {shelf.map((c) => <div key={c.name} className="card"><strong>{c.name}</strong><div className="muted" style={{ fontSize: 13 }}>{c.documents} {c.documents === 1 ? 'document' : 'documents'}</div><div className="time">updated {dayOf(c.updated)}</div></div>)}
              </div>
            )}
          </section>
          <section className="stack">
            <h2 className="caps">Changed most recently</h2>
            {recent.map((d) => (
              <button key={d.rel_path} type="button" className="card link" onClick={() => setOpen({ path: d.rel_path })}>
                <span style={{ fontWeight: 600 }}>{titleOf(d.rel_path)}</span>
                <span className="row" style={{ fontSize: 12, color: 'var(--text2)' }}><span className="chip line">{d.zone}</span><span>{sourceWords(d.source_type)}</span><span className="time push">{dayOf(d.updated_at)}</span></span>
                <span className="time" style={{ overflowWrap: 'anywhere' }}>{d.rel_path}</span>
              </button>
            ))}
          </section>
        </div>
      </main>
    );
  }

  const facets = Object.entries(found?.facets ?? {}).sort((a, b) => b[1] - a[1]);
  // The projects whose research is among the results; while narrowed, the one chosen stays offered so it can be cleared.
  const projects = Object.entries(found?.projects ?? {}).sort((a, b) => a[0].localeCompare(b[0]));
  return (
    <main className="main" style={{ alignItems: 'center' }}>
      <div className="stack" style={{ width: '100%', maxWidth: 920, gap: 16 }}>
        <div className="row"><button type="button" className="btn back" onClick={home}><span aria-hidden="true">←</span> Knowledge base</button></div>
        {search(false)}
        <div className="sub">{found ? `${found.results.length} results${found.capped ? ' (more matched)' : ''} for “${found.query}”` : problem || 'Searching.'}</div>
        {facets.length > 1 && (
          <div className="pills" role="group" aria-label="Kind">
            <button type="button" className="pill" aria-pressed={kind === ''} onClick={() => setKind('')}>All</button>
            {facets.map(([k, n]) => <button key={k} type="button" className="pill" aria-pressed={kind === k} onClick={() => setKind(k)}>{k} {n}</button>)}
          </div>
        )}
        {(projects.length > 0 || project) && (
          <div className="pills" role="group" aria-label="Project">
            <button type="button" className="pill" aria-pressed={project === ''} onClick={() => setProject('')}>Every project</button>
            {projects.map(([p, n]) => <button key={p} type="button" className="pill" aria-pressed={project === p} onClick={() => setProject(p)}>{p} {n}</button>)}
          </div>
        )}
        <div className="stack" style={{ gap: 8 }}>
          {found?.results.length === 0 && <div className="empty">Nothing matched. Agents who come up short ask the scout to research it; nothing here does that for you.</div>}
          {found?.results.map((h) => {
            const { title, above } = headingOf(h.heading);
            return (
              <button key={h.pointer} type="button" className="card link" onClick={() => setOpen({ path: h.path, hit: h })}>
                <span className="row" style={{ fontSize: 11 }}><span className="chip line">{h.collection}</span>{h.project && <span className="chip line" title="the project whose research this is">project {h.project}</span>}<span className={h.label === 'validated' ? 'chip ok' : 'chip warn'}>{h.label}</span>{h.status !== 'active' && <span className="chip">{h.status}</span>}<span className="time push" style={{ overflowWrap: 'anywhere' }}>{h.pointer}</span></span>
                <span style={{ fontWeight: 600, fontSize: 15, lineHeight: 1.35 }}>{title}</span>
                {above && <span className="muted" style={{ fontSize: 12 }}>{above}</span>}
                <span style={{ fontSize: 13, color: 'var(--text2)', lineHeight: 1.5 }}>{h.preview.replace(/[#*`]/g, '').slice(0, 320)}</span>
              </button>
            );
          })}
        </div>
      </div>
    </main>
  );
}
