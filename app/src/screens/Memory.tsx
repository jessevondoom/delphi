import { useEffect, useMemo, useState } from 'react';
import { Agent, Mascot } from '../Badge.tsx';
import { DocView } from '../DocView.tsx';
import { Problem, useRead } from '../EncyParts.tsx';
import { Rich } from '../Rich.tsx';
import { spec } from '../avatar.ts';
import type { Bridge } from '../bridge.ts';
import { byDay, learned, lessonLines, roleInAddress, type Learned, type RoleCount, type RoleMemory } from '../ency.ts';
import { listAll, type DocRow } from '../kb.ts';
import type { Info } from '../types.ts';

type Props = { bridge: Bridge | null; info: Info | null; light: boolean };
type At = { v: 'roles' } | { v: 'dates' } | { v: 'role'; role: string } | { v: 'doc'; path: string; from: At };

const KIND = { note: 'provisional note', finding: 'finding', episode: 'episode', lesson: 'lesson' } as const;

/** The episodes the knowledge base lists for a role: the documents of the `episodes` collection its front matter gives to that role. */
export function episodesOf(rows: DocRow[], role: string): DocRow[] {
  return rows.filter((d) => d.author === role || d.rel_path.split('/').includes(role)).sort((a, b) => b.updated_at.localeCompare(a.updated_at));
}

function Dated({ rows, info, light, showRole, open }: { rows: Learned[]; info: Info | null; light: boolean; showRole: boolean; open: (path: string) => void }) {
  const days = byDay(rows);
  if (days.length === 0) return <div className="muted" style={{ fontSize: 13 }}>Nothing dated yet. Ratified lessons carry no date unless a line states one.</div>;
  return (
    <>
      {days.slice(0, 60).map(([day, list]) => (
        <div key={day} className="stack" style={{ gap: 6 }}>
          <h3 className="group-head">{day}</h3>
          {list.map((l, i) => (
            <div key={i} className="card">
              <span className="row" style={{ fontSize: 12 }}>
                <span className="chip line">{KIND[l.kind]}</span>
                {showRole && (l.role in spec(info).roles ? <Agent info={info} role={l.role} light={light} /> : <strong>{l.role}</strong>)}
                <span style={{ fontWeight: 600 }}>{l.title}</span>
                {l.path && l.kind === 'episode' && <button type="button" className="pin push" onClick={() => open(l.path!)}>Open</button>}
              </span>
              {l.detail && <span style={{ fontSize: 13, color: 'var(--text2)' }}>{l.detail}</span>}
            </div>
          ))}
        </div>
      ))}
    </>
  );
}

/**
 * Agent memory (spec 117 §4 item 4): a page for each role with its ratified lessons, its provisional notes (with their
 * uses and expiry), its findings, its recent episodes and "learned lately", a dated list across all four; and a second
 * view of the same across every role, by date. The operator is shown all of it (AC-7); everything here is what an
 * agent or a session wrote, shown with the daemon's label for it, never an instruction. Read-only.
 */
export function Memory({ bridge, info, light }: Props) {
  const [at, setAt] = useState<At>(() => { const role = typeof location !== 'undefined' ? roleInAddress(location.search) : ''; return role ? { v: 'role', role } : { v: 'roles' }; });
  const roles = useRead<{ roles: RoleCount[] }>(bridge, 'memory.roles');
  const [episodes, setEpisodes] = useState<DocRow[]>([]);
  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    // Episodes are documents of the knowledge base (`collection=episodes`), read through the read the app already has.
    listAll((t) => bridge.kb(t), { collection: 'episodes' }, 1000).then((r) => { if (!gone) setEpisodes(r.rows); }).catch(() => {});
    return () => { gone = true; };
  }, [bridge]);

  if (at.v === 'doc') return <main className="main page"><DocView bridge={bridge} info={info} light={light} path={at.path} back={{ label: 'Back to agent memory', go: () => setAt(at.from) }} /></main>;
  if (at.v === 'role') return <RolePage bridge={bridge} info={info} light={light} role={at.role} episodes={episodesOf(episodes, at.role)} back={() => setAt({ v: 'roles' })} open={(path) => setAt({ v: 'doc', path, from: at })} />;

  const list = roles.data?.roles ?? [];
  return (
    <main className="main">
      <div className="stack" style={{ gap: 6 }}>
        <h1 className="title">Agent memory</h1>
        <div style={{ fontSize: 13, color: 'var(--text2)' }}>What each role has learned: ratified lessons, notes not yet reviewed, findings and episodes. You are shown all of it; an agent is given only its own. This window reads it and changes nothing.</div>
      </div>
      <div className="pills" role="group" aria-label="View">
        <button type="button" className="pill" aria-pressed={at.v === 'roles'} onClick={() => setAt({ v: 'roles' })}>By role</button>
        <button type="button" className="pill" aria-pressed={at.v === 'dates'} onClick={() => setAt({ v: 'dates' })}>All roles by date</button>
      </div>
      {!roles.data ? <Problem problem={roles.problem} waiting="Reading the roles." /> : at.v === 'dates' ? (
        <AllByDate bridge={bridge} info={info} light={light} roles={list.map((r) => r.role)} episodes={episodes} open={(path) => setAt({ v: 'doc', path, from: at })} />
      ) : list.length === 0 ? <div className="empty">No role has a lesson, a note or a finding yet.</div> : (
        <div className="tiles" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))' }}>
          {list.map((r) => (
            <button key={r.role} type="button" className="card link" onClick={() => setAt({ v: 'role', role: r.role })}>
              <span className="row">{r.role in spec(info).roles ? <Agent info={info} role={r.role} light={light} /> : <strong>{r.role}</strong>}</span>
              <span className="muted" style={{ fontSize: 12 }}>{r.lesson_files} {r.lesson_files === 1 ? 'lesson file' : 'lesson files'} · {r.notes} provisional {r.notes === 1 ? 'note' : 'notes'} · {r.findings} {r.findings === 1 ? 'finding' : 'findings'} · {episodesOf(episodes, r.role).length} {episodesOf(episodes, r.role).length === 1 ? 'episode' : 'episodes'}</span>
            </button>
          ))}
        </div>
      )}
    </main>
  );
}

/** Every role's dated memory in one list. One read a role, at most thirty roles. */
function AllByDate({ bridge, info, light, roles, episodes, open }: Props & { roles: string[]; episodes: DocRow[]; open: (path: string) => void }) {
  const [got, setGot] = useState<RoleMemory[] | null>(null);
  const [problem, setProblem] = useState('');
  const key = roles.join(',');
  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    Promise.all(roles.slice(0, 30).map((role) => bridge.ency('memory.role', { role }) as Promise<RoleMemory>))
      .then((all) => { if (!gone) setGot(all); }, (e: unknown) => { if (!gone) setProblem(e instanceof Error ? e.message : String(e)); });
    return () => { gone = true; };
  }, [bridge, key]);
  const rows = useMemo(() => (got ?? []).flatMap((m) => learned(m, episodesOf(episodes, m.role))).sort((a, b) => b.day.localeCompare(a.day) || a.role.localeCompare(b.role)), [got, episodes]);
  if (!got) return <Problem problem={problem} waiting="Reading every role's memory." />;
  return <section className="stack"><h2 className="caps">Learned lately, every role · {rows.length}</h2><Dated rows={rows} info={info} light={light} showRole open={open} /></section>;
}

function RolePage({ bridge, info, light, role, episodes, back, open }: Props & { role: string; episodes: DocRow[]; back: () => void; open: (path: string) => void }) {
  const got = useRead<RoleMemory>(bridge, 'memory.role', { role });
  const m = got.data;
  const lately = useMemo(() => (m ? learned(m, episodes) : []), [m, episodes]);
  const way = <button type="button" className="btn back" onClick={back}><span aria-hidden="true">←</span> Back to agent memory</button>;
  if (!m) return <main className="main page"><div className="row">{way}</div><Problem problem={got.problem} waiting="Reading this role's memory." /></main>;
  const lines = lessonLines(m.lessons);
  return (
    <main className="main page">
      <div className="row">{way}</div>
      <div className="lead">
        {role in spec(info).roles && <Mascot info={info} role={role} light={light} size={12} />}
        <div className="stack">
          <h1 className="question" style={{ fontSize: 26 }}>What the {info?.roles[role]?.name ?? role} has learned</h1>
          <span className="muted" style={{ fontSize: 13 }}>{lines.length} ratified {lines.length === 1 ? 'lesson' : 'lessons'} · {m.notes.length} provisional {m.notes.length === 1 ? 'note' : 'notes'} · {m.findings.length} {m.findings.length === 1 ? 'finding' : 'findings'} · {episodes.length} {episodes.length === 1 ? 'episode' : 'episodes'}</span>
        </div>
      </div>

      <section className="stack" aria-label="Learned lately">
        <h2 className="caps">Learned lately</h2>
        <Dated rows={lately} info={info} light={light} showRole={false} open={open} />
      </section>

      <section className="stack" aria-label="Ratified lessons">
        <h2 className="caps">Ratified lessons</h2>
        {m.lessons.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>This role has no lessons file.</div> : m.lessons.map((f) => (
          <div key={f.from} className="card">
            <span className="chip line" style={{ alignSelf: 'flex-start' }}>{f.from}</span>
            <div className="said"><Rich text={f.text} /></div>
          </div>
        ))}
      </section>

      <section className="stack" aria-label="Provisional notes">
        <h2 className="caps">Provisional notes · {m.notes.length}</h2>
        {m.notes.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>No note is waiting for review.</div> : m.notes.map((n) => (
          <div key={n.id} className="card">
            <span className="row" style={{ fontSize: 12 }}>
              <span className="chip warn">{n.label}</span>
              {n.kind && <span className="chip line">{n.kind}</span>}
              <span className="chip line">{n.tier}</span>
              {n.project && <span className="chip line">project {n.project}</span>}
            </span>
            <span style={{ fontSize: 14, lineHeight: 1.5 }}>{n.text}</span>
            <span className="muted" style={{ fontSize: 12 }}>Used {n.uses} {n.uses === 1 ? 'time' : 'times'} · written {n.created}{n.valid_until ? ` · expires ${n.valid_until}` : ' · no expiry'}{n.source ? ` · ${n.source}` : ''}</span>
          </div>
        ))}
      </section>

      <section className="stack" aria-label="Findings">
        <h2 className="caps">Findings · {m.findings.length}</h2>
        {m.findings.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>This role has recorded no finding.</div> : m.findings.map((f) => (
          <details key={f.id} className="card">
            <summary><span style={{ fontWeight: 600 }}>{f.title ?? f.id}</span> <span className="chip line">{f.finding_status ?? f.status ?? 'recorded'}</span> <span className="time">{(f.timestamp ?? '').slice(0, 10)}</span></summary>
            <span className="muted" style={{ fontSize: 12 }}>{f.label} · {f.rel_path}</span>
            {f.text && <div className="said"><Rich text={f.text.replace(/^---\n[\s\S]*?\n---\n?/, '')} /></div>}
          </details>
        ))}
      </section>

      <section className="stack" aria-label="Recent episodes">
        <h2 className="caps">Recent episodes · {episodes.length}</h2>
        {episodes.length === 0 ? <div className="muted" style={{ fontSize: 13 }}>The knowledge base lists no episode for this role.</div> : episodes.slice(0, 30).map((d) => (
          <button key={d.rel_path} type="button" className="card link" onClick={() => open(d.rel_path)}>
            <span className="row"><span style={{ fontWeight: 600 }}>{d.rel_path.split('/').pop()?.replace(/\.md$/, '')}</span><span className="chip line">{d.status}</span><span className="time push">{d.updated_at.slice(0, 10)}</span></span>
            <span className="time" style={{ overflowWrap: 'anywhere' }}>{d.rel_path}</span>
          </button>
        ))}
      </section>
    </main>
  );
}
