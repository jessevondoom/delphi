import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode, type Ref } from 'react';
import { DocView } from '../DocView.tsx';
import { Ask, CopyCommand, Problem, useRead } from '../EncyParts.tsx';
import { Rich, toSection } from '../Rich.tsx';
import type { Bridge } from '../bridge.ts';
import {
  HELD_RUN_MAX, NOTE_MAX, builtAt, byDay, byLetter, byProject, changeRows, contents, continueCommand, dayOfUnix, disputeCommand, findQuote, kindOf, matching, parts, passages,
  copyAllowed, exportAllowed, pieces, rebuildCommand, routeInAddress, sectionNote, shownValue, sourceFacts, statusWhy,
  type Anchor, type Changes, type ClaimView, type Links, type Orphan, type PageParts, type Research, type ResearchRecord, type Route, type SourceView, type Topic, type TopicPage, type Version,
} from '../ency.ts';
import { EMPTY, annotate, arrive, editNote, exportMarkdown, fresh, lastHere, markRead, newSince, parseMarks, passageKey, pinned, placed, togglePin, type Annotation, type Marks } from '../marks.ts';
import { blockText, type Block } from '../md.ts';
import { QUESTION_MAX, againRequest, disputeRequest, followUpRequest, rebuildRequest, sectionNoteRoom, sectionRequest } from '../requests.ts';
import type { Info } from '../types.ts';

type Props = { bridge: Bridge | null; info: Info | null; light: boolean; searchKb: (q: string) => void };

/** "Ask a follow-up" in a window that may ask: a question of its own, where the clipboard command takes a narrowing note. */
const FOLLOW_UP = { prompt: `A follow-up question, one line of at most ${QUESTION_MAX} characters. The daemon researches it in this topic's project; a question it already holds a topic on continues that topic.`, max: QUESTION_MAX };

type Go = (r: Route) => void;
type Mark = (f: (m: Marks) => Marks) => void;

const backWords = (r: Route | undefined): string => {
  switch (r?.v) {
    case 'topic': return 'Back to the topic page';
    case 'source': return 'Back to the source';
    case 'claim': return 'Back to the claim';
    case 'research': return 'Back to the research topic';
    case 'changes': return 'Back to changes';
    case 'doc': return 'Back to the document';
    default: return 'Back to the encyclopedia';
  }
};

const statusChip = (shown: string): string => (shown.includes('validated') && !shown.includes('not validated') ? 'chip ok' : shown.includes('disproved') ? 'chip warn' : 'chip line');

/**
 * The encyclopedia (spec 117 §4 items 1 to 3 and 5): a home page, and from it a topic page, a source, a claim, a
 * research topic and the list of changes, one at a time across the full width. It reads; the four things a page offers
 * to start (§6) are asked of the daemon when this window's key holds `request` (spec 120 §6) and are otherwise commands
 * for the clipboard. A request carries ids and what was typed, never a page's words: "Research this section further"
 * sends the section's anchor, not its heading. The operator's marks (§5) are loaded once and kept through the bridge.
 */
export function Encyclopedia({ bridge, info, light, searchKb }: Props) {
  // A page named in the address opens over the home page, so the way back is there.
  const [trail, setTrail] = useState<Route[]>(() => { const named = typeof location !== 'undefined' ? routeInAddress(location.search) : null; return named ? [{ v: 'home' }, named] : [{ v: 'home' }]; });
  const [marks, setMarks] = useState<Marks>(EMPTY);
  const [marksSaid, setMarksSaid] = useState('');
  const loaded = useRef(false);
  const at = trail[trail.length - 1]!;
  const go: Go = useCallback((r) => setTrail((t) => [...t, r]), []);
  const back = useCallback(() => setTrail((t) => (t.length > 1 ? t.slice(0, -1) : t)), []);

  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    bridge.marks().then((raw) => {
      if (gone) return;
      const m = arrive(parseMarks(raw), new Date().toISOString());
      loaded.current = true;
      setMarks(m);
      void bridge.saveMarks(m).catch((e: unknown) => setMarksSaid(String(e instanceof Error ? e.message : e)));
    }, (e: unknown) => { if (!gone) setMarksSaid(`Your marks could not be read (${e instanceof Error ? e.message : String(e)}); none are shown and none are kept.`); });
    return () => { gone = true; };
  }, [bridge]);

  // Marks change only once they were read: a window that could not read them never writes an empty file over them.
  const mark: Mark = useCallback((f) => {
    if (!loaded.current || !bridge) return;
    setMarks((m) => {
      const n = f(m);
      if (n !== m) void bridge.saveMarks(n).then(() => setMarksSaid(''), (e: unknown) => setMarksSaid(`Your marks could not be kept (${e instanceof Error ? e.message : String(e)}).`));
      return n;
    });
  }, [bridge]);

  const lists = useRead<{ topics: Topic[] }>(bridge, 'pages.list');
  const research = useRead<{ topics: Research[] }>(bridge, 'topics.list');
  const topics = lists.data?.topics ?? [];
  const from = trail[trail.length - 2];
  const way = trail.length > 1 ? <button type="button" className="btn back" onClick={back}><span aria-hidden="true">←</span> {backWords(from)}</button> : null;
  const shared = { bridge, go, way, topics, marksSaid };
  // The place in the trail is the key: a page opened again from itself (a see-also back to where one was) starts afresh.
  const key = `${trail.length}:${JSON.stringify(at)}`;

  return (
    <>
      {at.v === 'home' ? <Home {...shared} problem={lists.problem} loaded={lists.data !== null} research={research.data?.topics ?? []} marks={marks} mark={mark} searchKb={searchKb} />
        : at.v === 'topic' ? <TopicView key={key} {...shared} slug={at.slug} version={at.version} research={research.data?.topics ?? []} marks={marks} mark={mark} />
        : at.v === 'source' ? <SourcePage key={key} {...shared} id={at.id} quote={at.quote} />
        : at.v === 'claim' ? <ClaimPage key={key} {...shared} id={at.id} />
        : at.v === 'research' ? <ResearchPage key={key} {...shared} id={at.id} />
        : at.v === 'changes' ? <ChangesPage key={key} {...shared} marks={marks} />
        : <main className="main page"><DocView bridge={bridge} info={info} light={light} path={at.path} back={{ label: backWords(from), go: back }} /></main>}
    </>
  );
}

type Shared = { bridge: Bridge | null; go: Go; way: ReactNode; topics: Topic[]; /** Why the operator's marks could not be read or kept, when they could not. */ marksSaid: string };

// ── Home (§4 item 1) ─────────────────────────────────────────────────────────────────────────────────────────────────────

function TopicCard({ t, go, marks, note }: { t: Topic; go: Go; marks: Marks; note?: ReactNode }) {
  const state = fresh(marks, t);
  return (
    <button type="button" className="card link" onClick={() => go({ v: 'topic', slug: t.slug })}>
      <span className="row">
        <span style={{ fontWeight: 600, fontSize: 15 }}>{t.title}</span>
        {pinned(marks, t.slug) && <span className="chip line">pinned</span>}
        {state && <span className="chip strong">{state}</span>}
        {!t.built && <span className="chip line">not built yet</span>}
        {t.due && <span className="chip warn">due</span>}
        <span className="time push">{t.built ? `version ${t.version} · ${dayOfUnix(builtAt(t))}` : ''}</span>
      </span>
      {note}
    </button>
  );
}

function Home({ go, topics, marksSaid, problem, loaded, research, marks, mark, searchKb }: Shared & { problem: string; loaded: boolean; research: Research[]; marks: Marks; mark: Mark; searchKb: (q: string) => void }) {
  const [q, setQ] = useState('');
  const [group, setGroup] = useState<'letter' | 'project'>('letter');
  const found = useMemo(() => matching(topics, q), [topics, q]);
  const pins = topics.filter((t) => pinned(marks, t.slug));
  const news = useMemo(() => newSince(marks, topics), [marks, topics]);
  const due = topics.filter((t) => t.due);
  const open = topics.filter((t) => (t.open_questions?.length ?? 0) > 0);
  const groups = useMemo(() => (group === 'letter' ? byLetter(topics) : byProject(topics, research)), [group, topics, research]);
  const loose = research.filter((r) => !r.page);
  const words = q.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const asked = words.length ? research.filter((r) => words.every((w) => r.question.toLowerCase().includes(w))) : [];
  const since = lastHere(marks);

  return (
    <main className="main" style={{ alignItems: 'center', paddingTop: 40 }}>
      <div className="stack" style={{ width: '100%', maxWidth: 920, gap: 28 }}>
        <div className="stack" style={{ gap: 8 }}>
          <h1 className="title" style={{ fontSize: 28 }}>Encyclopedia</h1>
          <p style={{ fontSize: 15, color: 'var(--text2)' }}>What this workspace knows, a page for each topic. Every page is generated from cited records and is not authoritative: each statement is one click from its source.</p>
        </div>
        <form className="stack" style={{ gap: 6 }} role="search" onSubmit={(e) => { e.preventDefault(); if (q.trim()) searchKb(q.trim()); }}>
          <label htmlFor="encq" className="muted" style={{ fontSize: 12 }}>Find a topic by its title, its key terms or its open questions</label>
          <div className="row">
            <input id="encq" className="field large" style={{ flex: '1 1 320px' }} type="search" value={q} placeholder="Type a few words" onChange={(e) => setQ(e.target.value)} />
            {q.trim() && <button type="submit" className="btn">Search the whole knowledge base</button>}
          </div>
        </form>
        {marksSaid && <div className="notice" role="status">{marksSaid}</div>}
        {!loaded ? <Problem problem={problem} waiting="Reading the topics." /> : q.trim() ? (
          <section className="stack">
            <h2 className="caps">Topics · {found.length}</h2>
            {found.length === 0 && <div className="empty">No topic's title, key terms or open questions hold those words. Searching the whole knowledge base looks in every document.</div>}
            {found.map((t) => <TopicCard key={t.slug} t={t} go={go} marks={marks} />)}
            {asked.length > 0 && <h2 className="caps">Research topics · {asked.length}</h2>}
            {asked.map((r) => <ResearchCard key={r.id} r={r} go={go} />)}
          </section>
        ) : (
          <>
            <div className="row">
              <button type="button" className="btn" onClick={() => go({ v: 'changes' })}>Changes</button>
              <span className="muted" style={{ fontSize: 12 }}>{topics.length} {topics.length === 1 ? 'topic' : 'topics'} · {research.length} research {research.length === 1 ? 'topic' : 'topics'}{since ? ` · you were last here on ${dayOfUnix(since)}` : ''}</span>
            </div>
            {topics.length === 0 && <div className="empty">No topic page yet. A research topic gets one by itself, and <span className="mono">delphi pages new &lt;title&gt;</span> in a terminal makes one you name.</div>}
            {pins.length > 0 && (
              <section className="stack">
                <h2 className="caps">Pinned</h2>
                {pins.map((t) => <TopicCard key={t.slug} t={t} go={go} marks={marks} note={<span className="row"><span className="muted" style={{ fontSize: 12 }}>{t.change ?? ''}</span></span>} />)}
              </section>
            )}
            <section className="stack">
              <h2 className="caps">New and changed since you were last here · {news.length}</h2>
              {news.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>Nothing new: you have read every built page to its end.</div>
                : news.map(({ topic, state }) => <TopicCard key={topic.slug} t={topic} go={go} marks={marks} note={<span style={{ fontSize: 13, color: 'var(--text2)' }}>{state === 'new' ? 'You have not read this page.' : `Rebuilt since you read version ${marks.read[topic.slug]?.version}: ${topic.change ?? 'no change line'}.`}</span>} />)}
            </section>
            {due.length > 0 && (
              <section className="stack">
                <h2 className="caps">Due for a rebuild · {due.length}</h2>
                {due.map((t) => (
                  <div key={t.slug} className="stack" style={{ gap: 6 }}>
                    <TopicCard t={t} go={go} marks={marks} note={<span style={{ fontSize: 13, color: 'var(--text2)' }}>{t.due}</span>} />
                    <CopyCommand small label="Rebuild now" command={rebuildCommand(t.slug)} request={rebuildRequest(t.slug)} />
                  </div>
                ))}
              </section>
            )}
            {open.length > 0 && (
              <section className="stack">
                <h2 className="caps">With open questions · {open.length}</h2>
                {open.map((t) => <TopicCard key={t.slug} t={t} go={go} marks={marks} note={<span style={{ fontSize: 13, color: 'var(--text2)' }}>{t.open_questions![0]}{t.open_questions!.length > 1 ? ` (and ${t.open_questions!.length - 1} more)` : ''}</span>} />)}
              </section>
            )}
            {topics.length > 0 && (
              <section className="stack">
                <div className="head-row">
                  <h2 className="caps">All topics</h2>
                  <div className="pills" role="group" aria-label="Group the topics">
                    <button type="button" className="pill" aria-pressed={group === 'letter'} onClick={() => setGroup('letter')}>By letter</button>
                    <button type="button" className="pill" aria-pressed={group === 'project'} onClick={() => setGroup('project')}>By project</button>
                  </div>
                </div>
                {groups.map(([name, list]) => (
                  <div key={name} className="stack" style={{ gap: 6 }}>
                    <h3 className="group-head">{name}</h3>
                    <ul className="topic-list">
                      {list.map((t) => (
                        <li key={t.slug}>
                          <button type="button" className="md-link" onClick={() => go({ v: 'topic', slug: t.slug })}>{t.title}</button>
                          {fresh(marks, t) && <span className="chip strong">{fresh(marks, t)}</span>}
                          {t.due && <span className="chip warn">due</span>}
                          {!t.built && <span className="chip line">not built yet</span>}
                          <button type="button" className="pin" aria-pressed={pinned(marks, t.slug)} onClick={() => mark((m) => togglePin(m, t.slug))}>{pinned(marks, t.slug) ? 'Unpin' : 'Pin'}</button>
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </section>
            )}
            {loose.length > 0 && (
              <section className="stack">
                <h2 className="caps">Research with no page yet · {loose.length}</h2>
                {loose.map((r) => <ResearchCard key={r.id} r={r} go={go} />)}
              </section>
            )}
          </>
        )}
      </div>
    </main>
  );
}

function ResearchCard({ r, go }: { r: Research; go: Go }) {
  return (
    <button type="button" className="card link" onClick={() => go({ v: 'research', id: r.id })}>
      <span className="row" style={{ fontSize: 11 }}>
        <span className="chip line">research {r.state}</span>
        {r.project && <span className="chip line">project {r.project}</span>}
        {r.private && <span className="chip warn">private</span>}
        <span className="time push">{(r.updated ?? '').slice(0, 10)}</span>
      </span>
      <span style={{ fontWeight: 600 }}>{r.question}</span>
      <span className="muted" style={{ fontSize: 12 }}>{r.runs} {r.runs === 1 ? 'run' : 'runs'} · {r.claims} {r.claims === 1 ? 'claim' : 'claims'} · {r.label}</span>
    </button>
  );
}

// ── Topic page (§4 item 2) ───────────────────────────────────────────────────────────────────────────────────────────────

/** The anchors of a claim: each one's source, where in it, and the words quoted. Choosing one opens the source at that passage. */
function Anchors({ anchors, sources, go }: { anchors: Anchor[]; sources: ClaimView['sources']; go: Go }) {
  if (anchors.length === 0) return <div className="muted" style={{ fontSize: 13 }}>This claim has no anchor in a source.</div>;
  return (
    <div className="stack" style={{ gap: 8 }}>
      {anchors.map((a, i) => {
        const s = sources.find((x) => x.id === a.source_id);
        return (
          <button key={i} type="button" className="card link" onClick={() => go({ v: 'source', id: a.source_id, quote: a.quote ?? undefined })}>
            <span className="row" style={{ fontSize: 12 }}>
              <span style={{ fontWeight: 600 }}>{shownValue(s?.title) || a.source_id}</span>
              {s?.private && <span className="chip warn">private</span>}
              {a.locator && <span className="time">{a.locator}</span>}
              {a.state && a.state !== 'active' && <span className="chip line">{a.state}</span>}
            </span>
            {a.quote ? <span className="quoted">{a.quote}</span> : <span className="muted" style={{ fontSize: 12 }}>No passage is quoted; the source opens at its start.</span>}
            <span className="muted" style={{ fontSize: 12 }}>Open the source at this passage</span>
          </button>
        );
      })}
    </div>
  );
}

/** One claim as the citation panel and the claim page show it: its words, its status and why, its anchors. */
function ClaimBody({ view, go, full }: { view: ClaimView; go: Go; full?: boolean }) {
  const c = view.claim;
  return (
    <div className="stack">
      <div className="row"><span className={statusChip(view.shown)}>{view.shown}</span><span className="chip line">{view.status}</span>{c.project && <span className="chip line">project {c.project}</span>}</div>
      <p style={{ fontSize: full ? 18 : 15, lineHeight: 1.5 }}>{c.text}</p>
      <p style={{ fontSize: 13, color: 'var(--text2)' }}>{statusWhy(view.status, c)}</p>
      <h3 className="caps">Anchors</h3>
      <Anchors anchors={c.anchors ?? []} sources={view.sources} go={go} />
      <Ask small label="Dispute this statement" prompt="What is wrong with it? One line; it goes on the claim's record as your dispute." max={500} build={(w) => disputeCommand(c.id, w)} request={(typed) => disputeRequest(c.id, typed)} />
    </div>
  );
}

function CiteItem({ label, page, bib, bridge, go }: { label: string; page: TopicPage; bib: PageParts['bibliography']; bridge: Bridge | null; go: Go }) {
  const kind = kindOf(label);
  const claim = useRead<ClaimView>(bridge, kind === 'claim' ? 'library.claim' : null, { id: label });
  const row = bib.find((r) => r.label === label);
  if (kind === 'claim') {
    return (
      <section className="stack" aria-label={`Claim ${label}`}>
        <div className="row"><h3 className="caps">Claim</h3><span className="time">{label}</span></div>
        {claim.data ? <ClaimBody view={claim.data} go={go} /> : <Problem problem={claim.problem} waiting="Reading the claim." />}
        {claim.data && <button type="button" className="btn small" onClick={() => go({ v: 'claim', id: label })}>Open the claim's page</button>}
      </section>
    );
  }
  if (kind === 'source') {
    return (
      <section className="stack" aria-label={`Source ${label}`}>
        <div className="row"><h3 className="caps">Source</h3><span className="time">{label}</span></div>
        {row ? <><p style={{ fontSize: 15, fontWeight: 600 }}>{row.title}</p><div className="row"><span className="chip line">{row.status}</span><span className="muted" style={{ fontSize: 12 }}>{[row.origin, row.date].filter((x) => x && x !== '—').join(' · ')}</span></div></> : <p className="muted" style={{ fontSize: 13 }}>The page's bibliography has no row for it.</p>}
        <button type="button" className="btn small" onClick={() => go({ v: 'source', id: label.slice(2) })}>Open the source</button>
      </section>
    );
  }
  const rec = (page.records ?? []).find((r) => r.id === label);
  return (
    <section className="stack" aria-label={`${kind} ${label}`}>
      <div className="row"><h3 className="caps">{kind === 'finding' ? 'Finding' : 'Knowledge-base passage'}</h3><span className="time">{label}</span></div>
      {rec ? (
        <>
          <p style={{ fontSize: 15, fontWeight: 600 }}>{rec.title}</p>
          <div className="row"><span className="chip line">{rec.status}</span><span className="muted" style={{ fontSize: 12 }}>{[rec.kind, rec.origin, rec.locator].filter(Boolean).join(' · ')}</span></div>
          <span className="quoted">{rec.text}</span>
          <span className="muted" style={{ fontSize: 12 }}>The passage as it was when the page was built. It is a record of this workspace, not a library source.</span>
          {rec.rel_path && <button type="button" className="btn small" onClick={() => go({ v: 'doc', path: rec.rel_path })}>Open the document</button>}
        </>
      ) : <p className="muted" style={{ fontSize: 13 }}>{row ? `${row.title} (${row.status})` : 'The page holds no record by this label.'}</p>}
    </section>
  );
}

/** The side panel a citation marker opens (§4 item 2): each record the marker names, with its status and its anchors. */
function CitePanel({ labels, page, bib, bridge, go, close }: { labels: string[]; page: TopicPage; bib: PageParts['bibliography']; bridge: Bridge | null; go: Go; close: () => void }) {
  const head = useRef<HTMLHeadingElement>(null);
  useEffect(() => { head.current?.focus(); }, [labels.join(',')]);
  return (
    <aside className="aside cite-panel" aria-label="What this cites">
      <div className="head-row">
        <h2 ref={head} tabIndex={-1} className="caps">Cited here · {labels.length}</h2>
        <button type="button" className="btn small" onClick={close}>Close</button>
      </div>
      {labels.map((l) => <CiteItem key={l} label={l} page={page} bib={bib} bridge={bridge} go={go} />)}
    </aside>
  );
}

/** The notes on one passage, in the margin beside it, and the way to add one (§5 item 2). */
function Margin({ notes, passage, onAdd, onEdit }: { notes: Annotation[]; passage: string; onAdd: (passage: string, text: string) => void; onEdit: (id: string, text: string) => void }) {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const start = (id: string, text: string) => { setEditing(id); setDraft(text); };
  const save = () => { if (editing === 'new') onAdd(passage, draft); else if (editing) onEdit(editing, draft); setEditing(null); setDraft(''); };
  return (
    <div className="passage-notes">
      {notes.map((n) => (editing === n.id ? null : (
        <div key={n.id} className="note">
          <span>{n.text}</span>
          <span className="row"><span className="time">{n.at.slice(0, 10)}{n.version ? ` · written on version ${n.version}` : ''}</span><button type="button" className="pin" onClick={() => start(n.id, n.text)}>Edit</button></span>
        </div>
      )))}
      {editing ? (
        <div className="note editing">
          <label className="muted" style={{ fontSize: 12 }}>Your note on this passage<textarea className="field note-field" value={draft} autoFocus maxLength={4000} onChange={(e) => setDraft(e.target.value)} /></label>
          <span className="row">
            <button type="button" className="btn small" onClick={save} disabled={editing === 'new' && !draft.trim()}>{editing !== 'new' && !draft.trim() ? 'Remove the note' : 'Keep the note'}</button>
            <button type="button" className="btn small" onClick={() => { setEditing(null); setDraft(''); }}>Cancel</button>
          </span>
        </div>
      ) : <button type="button" className="pin note-add" onClick={() => start('new', '')}>Add a note</button>}
    </div>
  );
}

function TopicView({ bridge, go, way, topics, marksSaid, slug, version, research, marks, mark }: Shared & { slug: string; version?: string; research: Research[]; marks: Marks; mark: Mark }) {
  const got = useRead<TopicPage>(bridge, 'pages.get', version ? { slug, version } : { slug });
  const versions = useRead<{ versions: Version[] }>(bridge, 'pages.versions', { slug });
  const links = useRead<Links>(bridge, 'pages.links', { slug });
  const [citing, setCiting] = useState<string[] | null>(null);
  const [said, setSaid] = useState('');
  const end = useRef<HTMLDivElement>(null);
  const page = got.data;
  const p = useMemo(() => (page ? parts(page.markdown) : null), [page]);
  const listed = topics.find((t) => t.slug === slug);
  const number = version ? versions.data?.versions.find((v) => v.version === version)?.number ?? 0 : page?.version ?? 0;
  const notes = useMemo(() => (p ? placed(marks, slug, passages(p)) : { on: new Map<string, Annotation[]>(), orphaned: [] }), [marks, slug, p]);

  // Read state (§5 item 1): the version on screen is marked read when its end comes into view. A page with nothing
  // built has no end to reach.
  useEffect(() => {
    const el = end.current;
    if (!el || !page || !page.markdown || number < 1 || typeof IntersectionObserver === 'undefined') return;
    const seen = new IntersectionObserver((entries) => { if (entries.some((e) => e.isIntersecting)) mark((m) => markRead(m, slug, number, new Date().toISOString())); });
    seen.observe(el);
    return () => seen.disconnect();
  }, [page, number, slug, mark]);

  if (!page || !p) return <main className="main page"><div className="row">{way}</div><Problem problem={got.problem} waiting="Opening the page." /></main>;

  const titleOf = (s: string): string => topics.find((t) => t.slug === s)?.title ?? s;
  const topicId = page.research_topics?.[0] ?? '';
  const built = dayOfUnix(version ? versions.data?.versions.find((v) => v.version === version)?.built : builtAt(listed ?? page));
  const toc = contents(p);
  const cites = (labels: string[]): ReactNode => labels.map((l) => <button key={l} type="button" className="cite" aria-label={`Citation: ${l}`} onClick={() => setCiting([l])}>{l}</button>);
  const wrap = (b: Block, _i: number, drawn: ReactNode): ReactNode => {
    const text = blockText(b);
    const key = text ? passageKey(text) : '';
    if (!key) return drawn;
    return (
      <div className="passage">
        <div className="passage-text">{drawn}</div>
        <Margin notes={notes.on.get(key) ?? []} passage={text}
          onAdd={(passage, body) => mark((m) => annotate(m, { slug, version: number, passage, text: body }, new Date().toISOString(), `n-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`))}
          onEdit={(id, body) => mark((m) => editNote(m, id, body))} />
      </div>
    );
  };
  const ctx = { bridge, cite: setCiting, openPage: (s: string) => go({ v: 'topic', slug: s }), anchors: true, wrap };
  const plainCtx = { ...ctx, wrap: undefined };
  const out = () => {
    const file = exportMarkdown(page, new Date().toISOString().slice(0, 10));
    if (!file || !bridge) { setSaid('This page has no banner to export with; it is not exported.'); return; }
    bridge.exportPage(file.name, file.text).then((where) => setSaid(`Exported as Markdown: ${where}`), (e: unknown) => setSaid(e instanceof Error ? e.message : String(e)));
  };
  const earlier = versions.data?.versions ?? [];

  return (
    <>
      <main className="main page ency-page">
        <div className="row no-print" style={{ fontSize: 12 }}>
          {way}
          <span className="chip line">generated page</span>
          {page.markdown && <span className="chip line">{version ? `an earlier version${number ? ` (${number})` : ''}` : `version ${page.version}`}</span>}
          {built && <span className="time">built {built}</span>}
          {page.outcome === 'claims_only' && <span className="chip warn">claims only: the draft did not pass its checks</span>}
          {page.outcome === 'seed' && <span className="chip warn">seed draft: assembled by code from research notes, not written up and not validated</span>}
          {page.outcome === 'pass_contested' && <span className="chip warn">cites a contested claim</span>}
          {!version && page.due && <span className="chip warn">due: {page.due}</span>}
          {page.valid_until_date && <span className="time">looked at again by {page.valid_until_date}</span>}
        </div>
        {version && (
          <div className="card gold no-print">
            <strong>You are reading an earlier version of this page{built ? `, built ${built}` : ''}.</strong>
            <div className="row"><button type="button" className="btn small" onClick={() => go({ v: 'topic', slug })}>Open the current version</button></div>
          </div>
        )}
        <h1 className="question" style={{ fontSize: 28 }}>{page.title || p.title || slug}</h1>
        {!version && page.change && <p className="changed"><span className="caps">What changed</span> {page.change}</p>}
        <div className="row no-print">
          <button type="button" className="btn small" aria-pressed={pinned(marks, slug)} onClick={() => mark((m) => togglePin(m, slug))}>{pinned(marks, slug) ? 'Unpin from the home page' : 'Pin to the home page'}</button>
          {page.markdown && exportAllowed(page) && <button type="button" className="btn small" onClick={out}>Export as Markdown</button>}
          {page.markdown && exportAllowed(page) && <button type="button" className="btn small" onClick={() => { void bridge?.print().catch((e: unknown) => setSaid(e instanceof Error ? e.message : String(e))); }}>Print or save as PDF</button>}
          {page.markdown && !exportAllowed(page) && <span className="muted" style={{ fontSize: 12 }}>Not exported or printed: {page.export_refused ?? 'the page carries more of a source kept for reading on this machine than may leave it'}.</span>}
          {!version && <CopyCommand small label="Rebuild now" command={rebuildCommand(slug)} request={rebuildRequest(slug)} />}
          {topicId && <Ask small label="Ask a follow-up" prompt={`One line that narrows the next research run on this topic (at most ${NOTE_MAX} characters).`} max={NOTE_MAX} build={(w) => (w.trim() ? continueCommand(topicId, w) : '')} request={(typed) => followUpRequest(topicId, typed)} asking={FOLLOW_UP} />}
        </div>
        {(said || marksSaid) && <div className="muted no-print" style={{ fontSize: 12 }} role="status">{[said, marksSaid].filter(Boolean).join(' ')}</div>}

        {!page.markdown ? (
          <div className="empty">This topic has no page yet. {page.due ?? ''} The daemon builds it while idle; "Rebuild now" asks for it at once.</div>
        ) : (
          <div className="page-text">
            {p.banner && <p className="banner">{p.banner}</p>}
            {p.lead && <div className="said doc"><Rich text={p.lead} ctx={ctx} /></div>}

            {(page.key_terms?.length || p.named['Key terms']) && (
              <section className="stack under" aria-label="Key terms">
                <h2 className="md-h md-h2">Key terms</h2>
                {page.key_terms?.length ? (
                  <dl className="terms">{page.key_terms.map((k) => <div key={k.term}><dt>{k.term}</dt><dd>{k.definition} {cites(k.cites)}</dd></div>)}</dl>
                ) : <div className="said doc"><Rich text={p.named['Key terms']!} ctx={plainCtx} /></div>}
              </section>
            )}

            {toc.length > 0 && (
              <nav className="toc no-print" aria-label="Contents">
                <h2 className="caps">Contents</h2>
                <ol>{toc.map((c, i) => <li key={i} className={c.depth ? 'deep' : undefined}><button type="button" className="md-link" onClick={() => toSection(c.anchor)}>{pieces(c.title).filter((x) => x.t === 'text').map((x) => (x as { s: string }).s).join('').trim()}</button></li>)}</ol>
              </nav>
            )}

            {p.sections.map((s) => (
              <section key={s.anchor} className="stack">
                <div className="said doc"><Rich text={`## ${s.heading}\n\n${s.text}`} ctx={ctx} /></div>
                {topicId && !version && <div className="no-print"><Ask small label="Research this section further" prompt={`What should the next run look for in "${s.heading}"? It may be left empty.`} max={NOTE_MAX - 20} build={(w) => continueCommand(topicId, sectionNote(s.heading, w))} request={(typed) => sectionRequest(topicId, s.anchor, typed)} asking={{ prompt: `What should the next run look for in this section? It may be left empty (at most ${sectionNoteRoom(s.anchor)} characters).`, max: Math.max(1, sectionNoteRoom(s.anchor)) }} /></div>}
              </section>
            ))}

            <section className="stack under" aria-label="Bibliography and citations">
              <h2 className="md-h md-h2" id="sec-bibliography-and-citations" tabIndex={-1}>Bibliography and citations</h2>
              {p.bibliography.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>No sources cited: the page is unsupported, and its banner says so.</div> : (
                <div className="table-wrap md-table">
                  <table className="table">
                    <thead><tr><th scope="col">Id</th><th scope="col">Status</th><th scope="col">Title</th><th scope="col">Author or origin</th><th scope="col">Date</th><th scope="col">Locator</th><th scope="col">Citation</th></tr></thead>
                    <tbody>{p.bibliography.map((r) => <tr key={r.label}><td><button type="button" className="cite" aria-label={`Show ${r.label}`} onClick={() => setCiting([r.label])}>{r.label}</button></td><td><span className={statusChip(r.status)}>{r.status}</span></td><td>{r.title}</td><td>{r.origin}</td><td>{r.date}</td><td>{r.locator}</td><td>{r.citation}</td></tr>)}</tbody>
                  </table>
                </div>
              )}
            </section>

            {(page.start_here?.length || p.named['Start here']) && (
              <section className="stack under" aria-label="Start here">
                <h2 className="md-h md-h2">Start here</h2>
                {page.start_here?.length ? <ol className="plain-list">{page.start_here.map((s, i) => <li key={i}>{s.why} {cites(s.cites)}</li>)}</ol> : <div className="said doc"><Rich text={p.named['Start here']!} ctx={plainCtx} /></div>}
              </section>
            )}
            {(page.see_also?.length ?? 0) > 0 && (
              <section className="stack under no-print" aria-label="See also">
                <h2 className="md-h md-h2">See also</h2>
                <ul className="plain-list">{page.see_also!.map((s) => <li key={s}><button type="button" className="md-link" onClick={() => go({ v: 'topic', slug: s })}>{titleOf(s)}</button></li>)}</ul>
              </section>
            )}
            <section className="stack under no-print" aria-label="What links here">
              <h2 className="md-h md-h2">What links here</h2>
              {!links.data ? <div className="muted" style={{ fontSize: 13 }}>{links.problem || 'Reading.'}</div> : links.data.links_here.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>No other topic page links to this one.</div>
                : <ul className="plain-list">{links.data.links_here.map((s) => <li key={s}><button type="button" className="md-link" onClick={() => go({ v: 'topic', slug: s })}>{titleOf(s)}</button></li>)}</ul>}
            </section>
            {(page.open_questions?.length ?? 0) > 0 && (
              <section className="stack under" aria-label="Open questions">
                <h2 className="md-h md-h2">Open questions</h2>
                <ul className="plain-list">{page.open_questions!.map((q, i) => <li key={i}>{q}</li>)}</ul>
                <span className="muted" style={{ fontSize: 12 }}>The gaps the research records name, in their own words; not checked.</span>
              </section>
            )}
          </div>
        )}

        <section className="stack under no-print" aria-label="Versions">
          <h2 className="md-h md-h2">Versions</h2>
          <ul className="plain-list">
            {page.markdown !== '' && !version && <li><strong>Version {page.version}</strong>, on screen{builtAt(listed ?? page) ? `, built ${dayOfUnix(builtAt(listed ?? page))}` : ''}{page.change ? `: ${page.change}` : ''}</li>}
            {version && <li><button type="button" className="md-link" onClick={() => go({ v: 'topic', slug })}>The current version</button></li>}
            {earlier.map((v) => (
              <li key={v.version}>
                {v.version === version ? <strong>{v.number ? `Version ${v.number}` : v.version}, on screen</strong> : <button type="button" className="md-link" onClick={() => go({ v: 'topic', slug, version: v.version })}>{v.number ? `Version ${v.number}` : v.version}</button>}
                {v.built ? `, built ${dayOfUnix(v.built)}` : ''}{v.change ? `: ${v.change}` : ''}
              </li>
            ))}
            {!versions.data && <li className="muted">{versions.problem || 'Reading the versions.'}</li>}
            {versions.data && earlier.length === 0 && <li className="muted">No earlier version is kept.</li>}
          </ul>
        </section>

        {notes.orphaned.length > 0 && (
          <section className="stack under no-print" aria-label="Orphaned notes">
            <h2 className="md-h md-h2">Notes whose passage is not on this version</h2>
            {notes.orphaned.map((n) => (
              <div key={n.id} className="note orphan">
                <span>{n.text}</span>
                <span className="quoted">{n.passage}</span>
                <span className="row"><span className="time">{n.at.slice(0, 10)}{n.version ? ` · written on version ${n.version}` : ''}</span><button type="button" className="pin" onClick={() => mark((m) => editNote(m, n.id, ''))}>Remove the note</button></span>
              </div>
            ))}
          </section>
        )}
        {page.research_topics?.length ? (
          <section className="stack under no-print" aria-label="Research behind this page">
            <h2 className="md-h md-h2">Research behind this page</h2>
            <ul className="plain-list">{page.research_topics.map((id) => <li key={id}><button type="button" className="md-link" onClick={() => go({ v: 'research', id })}>{research.find((r) => r.id === id)?.question ?? id}</button></li>)}</ul>
          </section>
        ) : null}
        <div ref={end} className="page-end" aria-hidden="true" />
      </main>
      {citing && <CitePanel labels={citing} page={page} bib={p.bibliography} bridge={bridge} go={go} close={() => setCiting(null)} />}
    </>
  );
}

// ── Source and claim pages (§4 item 3) ───────────────────────────────────────────────────────────────────────────────────

const SHOWN_META = new Set(['title', 'author', 'status', 'licence', 'license', 'retrieved', 'last_checked', 'URL', 'url', 'capture_level', 'private', 'kept_privately', 'cited_by', 'rights_holder', 'origin', 'id']);

/** A source's stored text, as text and never as markup, with the quoted passage marked where it is found (AC-4). */
export function SourceText({ text, quote, at }: { text: string; quote?: string; at?: Ref<HTMLElement> }) {
  const range = quote ? findQuote(text, quote) : null;
  return <div className="said source-text">{range ? <>{text.slice(0, range.from)}<mark ref={at} className="quote">{text.slice(range.from, range.to)}</mark>{text.slice(range.to)}</> : text}</div>;
}

/** The anchors into a source that no longer resolve (spec 142 §2.3 item 3), below its text: each quote as text, where it was last found, and why. */
export function Orphans({ orphans, open }: { orphans: Orphan[]; open?: (claim: string) => void }) {
  if (orphans.length === 0) return null;
  return (
    <section className="stack" aria-label="Orphaned anchors">
      <h2 className="caps">Orphaned anchors · {orphans.length}</h2>
      <span className="muted" style={{ fontSize: 12 }}>Quotations a claim rests on that are no longer found in the text the library keeps for this source.</span>
      {orphans.map((o, i) => (
        <div key={`${o.claim}-${i}`} className="card">
          <span className="quoted">{o.quote || 'The quotation went with the capture.'}</span>
          <span className="row" style={{ fontSize: 12 }}>
            <span className="chip warn">{o.why}</span>
            <span className="muted">last found in version {o.version}{o.date ? ` of ${o.date}` : ''}</span>
            {open ? <button type="button" className="cite push" aria-label={`Show ${o.claim}`} onClick={() => open(o.claim)}>{o.claim}</button> : <span className="time push">{o.claim}</span>}
          </span>
        </div>
      ))}
    </section>
  );
}

function SourcePage({ bridge, go, way, id, quote }: Shared & { id: string; quote?: string }) {
  const got = useRead<SourceView>(bridge, 'library.source', quote ? { id, quote } : { id });
  const at = useRef<HTMLElement>(null);
  const [copyNote, setCopyNote] = useState('');
  const v = got.data;
  const range = useMemo(() => (v && quote ? findQuote(v.text, quote) : null), [v, quote]);
  // The quoted passage is brought into view at once: no gliding, so nothing for reduce motion to still.
  useEffect(() => { at.current?.scrollIntoView({ behavior: 'auto', block: 'center' }); }, [range]);
  if (!v) return <main className="main page"><div className="row">{way}</div><Problem problem={got.problem} waiting="Opening the source." /></main>;
  const citing = (Array.isArray(v.meta.cited_by) ? v.meta.cited_by : []).filter((x): x is string => typeof x === 'string').slice(0, 40);
  const others = Object.entries(v.meta).filter(([k, x]) => !SHOWN_META.has(k) && shownValue(x) !== '');
  const address = shownValue(v.meta.URL) || shownValue(v.meta.url);
  return (
    <main className="main page">
      <div className="row" style={{ fontSize: 12 }}>
        {way}
        <span className="chip line">library source</span>
        {v.private && <span className="chip warn">private</span>}
        <span className="time">{v.label}</span>
      </div>
      <h1 className="question" style={{ fontSize: 26 }}>{shownValue(v.meta.title) || v.id}</h1>
      <dl className="facts">
        {sourceFacts(v).map((f) => <div key={f.label}><dt>{f.label}</dt><dd style={{ overflowWrap: 'anywhere' }}>{f.value}</dd></div>)}
        {others.map(([k, x]) => <div key={k}><dt>{k.replace(/_/g, ' ')}</dt><dd style={{ overflowWrap: 'anywhere' }}>{shownValue(x)}</dd></div>)}
      </dl>
      {/^https?:\/\//i.test(address) && bridge?.openUrl && <div className="row"><button type="button" className="btn small" onClick={() => { void bridge.openUrl!(address).catch(() => {}); }}>Open its address in your browser</button></div>}

      <section className="stack" aria-label="Claims citing it">
        <h2 className="caps">Claims citing it · {citing.length}</h2>
        {citing.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>No claim is anchored to this source.</div> : citing.map((c) => <ClaimRow key={c} id={c} bridge={bridge} go={go} />)}
      </section>

      <section className={v.text_held ? 'stack no-print' : 'stack'} aria-label="Text" onCopy={(e) => {
        // A held source (spec 142 §2.5 item 4 (h)): shown here, but not copied out past the bound, and not printed (`no-print`).
        const selected = window.getSelection()?.toString() ?? '';
        if (!copyAllowed(v, selected)) { e.preventDefault(); setCopyNote(`Not copied: this text is kept for reading on this machine only; copy at most ${HELD_RUN_MAX} characters at a time.`); }
      }}>
        <h2 className="caps">{v.passage_only ? 'The cited passage' : 'Its text as the library keeps it'}</h2>
        {copyNote && <div className="notice" role="status">{copyNote}</div>}
        {quote && !range && <div className="notice" role="status">{v.passage_only ? 'The quoted passage is not in the text the library keeps for this source, so nothing of it is shown.' : 'The quoted passage was not found in the stored text as written; the text is shown from its start.'} The quotation: <span className="quoted">{quote}</span></div>}
        {v.passage_only && <span className="muted" style={{ fontSize: 12 }}>This source is kept privately for retrieval (spec 119): this window shows the passage a claim cites and no more of it. <span className="mono">delphi sources show {v.id}</span> at a terminal prints its record.</span>}
        {v.text ? (
          <SourceText text={v.text} quote={quote} at={at} />
        ) : !v.passage_only && <div className="empty">The library keeps no text for this source.</div>}
        {v.truncated && <span className="muted" style={{ fontSize: 12 }}>The text is longer than this window is given; it ends here.</span>}
        {v.assets.length > 0 && <span className="muted" style={{ fontSize: 12 }}>{v.assets.length} {v.assets.length === 1 ? 'picture is' : 'pictures are'} kept with it.</span>}
      </section>

      <Orphans orphans={v.orphans ?? []} open={(claim) => go({ v: 'claim', id: claim })} />
    </main>
  );
}

function ClaimRow({ id, bridge, go }: { id: string; bridge: Bridge | null; go: Go }) {
  const got = useRead<ClaimView>(bridge, 'library.claim', { id });
  return (
    <button type="button" className="card link" onClick={() => go({ v: 'claim', id })}>
      <span className="row" style={{ fontSize: 11 }}>{got.data && <span className={statusChip(got.data.shown)}>{got.data.shown}</span>}<span className="time push">{id}</span></span>
      <span>{got.data ? got.data.claim.text : got.problem || 'Reading the claim.'}</span>
    </button>
  );
}

function ClaimPage({ bridge, go, way, id }: Shared & { id: string }) {
  const got = useRead<ClaimView>(bridge, 'library.claim', { id });
  const v = got.data;
  if (!v) return <main className="main page"><div className="row">{way}</div><Problem problem={got.problem} waiting="Opening the claim." /></main>;
  const c = v.claim;
  const facts: [string, string][] = ([['Kind', c.kind], ['Scope', c.scope], ['Origin', c.origin], ['Recorded', c.created?.slice(0, 10)], ['Last confirmed', c.last_confirmed?.slice(0, 10)], ['Holds until', c.valid_until?.slice(0, 10)]] as [string, string | undefined][]).filter((f): f is [string, string] => !!f[1]);
  return (
    <main className="main page">
      <div className="row" style={{ fontSize: 12 }}>{way}<span className="chip line">library claim</span><span className="time">{c.id}</span></div>
      <ClaimBody view={v} go={go} full />
      {facts.length > 0 && <dl className="facts">{facts.map(([k, x]) => <div key={k}><dt>{k}</dt><dd>{x}</dd></div>)}</dl>}
      {(c.contested_by?.length ?? 0) > 0 && (
        <section className="stack" aria-label="Disputes">
          <h2 className="caps">Disputes on its record · {c.contested_by!.length}</h2>
          {c.contested_by!.map((d, i) => <div key={i} className="card"><span>{typeof d === 'string' ? d : shownValue((d as Record<string, unknown>).note) || JSON.stringify(d)}</span>{typeof d === 'object' && d && <span className="time">{[shownValue((d as Record<string, unknown>).by), shownValue((d as Record<string, unknown>).at).slice(0, 10)].filter(Boolean).join(' · ')}</span>}</div>)}
        </section>
      )}
      {(c.supersedes || c.superseded_by) && (
        <div className="row">
          {c.supersedes && <button type="button" className="btn small" onClick={() => go({ v: 'claim', id: c.supersedes! })}>The claim it supersedes</button>}
          {c.superseded_by && <button type="button" className="btn small" onClick={() => go({ v: 'claim', id: c.superseded_by! })}>The claim that supersedes it</button>}
        </div>
      )}
    </main>
  );
}

// ── A research topic's record ────────────────────────────────────────────────────────────────────────────────────────────

function ResearchPage({ bridge, go, way, id }: Shared & { id: string }) {
  const got = useRead<ResearchRecord>(bridge, 'topics.get', { id });
  const v = got.data;
  if (!v) return <main className="main page"><div className="row">{way}</div><Problem problem={got.problem} waiting="Opening the research topic." /></main>;
  const rec = v.record;
  const claims = (Array.isArray(rec.claims) ? rec.claims : []).filter((x): x is string => typeof x === 'string');
  const gaps = (Array.isArray(rec.gaps) ? rec.gaps : []).map((g) => (typeof g === 'string' ? g : shownValue((g as Record<string, unknown>)?.text) || shownValue((g as Record<string, unknown>)?.what) || JSON.stringify(g)));
  const runs = Array.isArray(rec.runs) ? rec.runs.length : 0;
  return (
    <main className="main page">
      <div className="row" style={{ fontSize: 12 }}>
        {way}
        <span className="chip line">research topic</span>
        {shownValue(rec.state) && <span className="chip line">{shownValue(rec.state)}</span>}
        {shownValue(rec.project) && <span className="chip line">project {shownValue(rec.project)}</span>}
        {rec.private === true && <span className="chip warn">private</span>}
        <span className="time">{v.id}</span>
      </div>
      <h1 className="question" style={{ fontSize: 26 }}>{shownValue(rec.question) || v.id}</h1>
      <p className="banner">{v.label}</p>
      <div className="row">
        {v.page && <button type="button" className="btn small" onClick={() => go({ v: 'topic', slug: v.page! })}>Open its topic page</button>}
        <Ask small label="Ask a follow-up" prompt={`One line that narrows the next research run on this topic (at most ${NOTE_MAX} characters).`} max={NOTE_MAX} build={(w) => (w.trim() ? continueCommand(v.id, w) : '')} request={(typed) => followUpRequest(v.id, typed)} asking={FOLLOW_UP} />
        <CopyCommand small label="Run it again" command={continueCommand(v.id, '')} request={againRequest(v.id)} />
      </div>
      <dl className="facts">
        <div><dt>Runs</dt><dd>{runs}</dd></div>
        <div><dt>Claims</dt><dd>{claims.length}</dd></div>
        {shownValue(rec.created) && <div><dt>First asked</dt><dd>{shownValue(rec.created).slice(0, 10)}</dd></div>}
        {shownValue(rec.updated) && <div><dt>Updated</dt><dd>{shownValue(rec.updated).slice(0, 10)}</dd></div>}
      </dl>
      <section className="stack" aria-label="Answer">
        <h2 className="caps">The written answer of the newest finished run</h2>
        {v.answer ? <div className="said doc"><Rich text={v.answer} ctx={{ bridge, cite: (labels) => { const l = labels[0]; if (l) go(kindOf(l) === 'source' ? { v: 'source', id: l.slice(2) } : { v: 'claim', id: l }); } }} /></div> : <div className="muted" style={{ fontSize: 13 }}>No run of this topic has written an answer.</div>}
      </section>
      {gaps.length > 0 && <section className="stack" aria-label="Gaps"><h2 className="caps">Gaps the record names · {gaps.length}</h2><ul className="plain-list">{gaps.map((g, i) => <li key={i}>{g}</li>)}</ul></section>}
      {claims.length > 0 && <section className="stack" aria-label="Claims"><h2 className="caps">Claims it recorded · {claims.length}</h2>{claims.slice(0, 40).map((c) => <ClaimRow key={c} id={c} bridge={bridge} go={go} />)}</section>}
    </main>
  );
}

// ── Changes (§4 item 5) ──────────────────────────────────────────────────────────────────────────────────────────────────

const KIND_WORDS = { page: 'page rebuilt', research: 'research', finding: 'finding', note: 'note kept' } as const;

function ChangesPage({ bridge, go, way, marks }: Shared & { marks: Marks }) {
  const last = lastHere(marks);
  const [span, setSpan] = useState<'last' | '7' | '30' | 'all'>(last ? 'last' : '30');
  // The start of the span is fixed when it is chosen, so the read is asked once and not on every draw.
  const time = useMemo(() => (span === 'all' ? 0 : span === 'last' ? last : Math.floor(Date.now() / 1000) - Number(span) * 86400), [span, last]);
  const got = useRead<Changes>(bridge, 'changes.since', { time });
  const days = useMemo(() => (got.data ? byDay(changeRows(got.data)) : []), [got.data]);
  return (
    <main className="main" style={{ alignItems: 'center' }}>
      <div className="stack" style={{ width: '100%', maxWidth: 920, gap: 18 }}>
        <div className="row">{way}</div>
        <h1 className="title">Changes</h1>
        <p style={{ fontSize: 13, color: 'var(--text2)' }}>Topic pages built, research topics added or updated, findings recorded and notes kept, by day. A claim whose status changed is not in this list: the daemon's answer does not carry it yet.</p>
        <div className="pills" role="group" aria-label="Since when">
          {last > 0 && <button type="button" className="pill" aria-pressed={span === 'last'} onClick={() => setSpan('last')}>Since you were last here ({dayOfUnix(last)})</button>}
          <button type="button" className="pill" aria-pressed={span === '7'} onClick={() => setSpan('7')}>7 days</button>
          <button type="button" className="pill" aria-pressed={span === '30'} onClick={() => setSpan('30')}>30 days</button>
          <button type="button" className="pill" aria-pressed={span === 'all'} onClick={() => setSpan('all')}>Everything</button>
        </div>
        {!got.data ? <Problem problem={got.problem} waiting="Reading what changed." /> : days.length === 0 ? <div className="empty">Nothing changed in that time.</div> : days.map(([day, rows]) => (
          <section key={day} className="stack" style={{ gap: 6 }}>
            <h2 className="group-head">{day}</h2>
            {rows.map((r, i) => (r.open ? (
              <button key={i} type="button" className="card link" onClick={() => go(r.open!.v === 'topic' ? { v: 'topic', slug: r.open!.slug } : { v: 'research', id: (r.open as { id: string }).id })}>
                <span className="row"><span className="chip line">{KIND_WORDS[r.kind]}</span><span style={{ fontWeight: 600 }}>{r.title}</span></span>
                <span className="muted" style={{ fontSize: 12 }}>{r.detail}</span>
              </button>
            ) : (
              <div key={i} className="card">
                <span className="row"><span className="chip line">{KIND_WORDS[r.kind]}</span><span>{r.title}</span></span>
                <span className="muted" style={{ fontSize: 12 }}>{r.detail}</span>
              </div>
            )))}
          </section>
        ))}
      </div>
    </main>
  );
}
