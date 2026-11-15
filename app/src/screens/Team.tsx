import { useEffect, useMemo, useState } from 'react';
import { Agent, Mascot } from '../Badge.tsx';
import { DocView } from '../DocView.tsx';
import { spec } from '../avatar.ts';
import type { Bridge } from '../bridge.ts';
import { docInAddress, listAll, type DocRow } from '../kb.ts';
import { skillRows, teamRows } from '../team.ts';
import type { Info } from '../types.ts';

type Props = { bridge: Bridge | null; info: Info | null; light: boolean };

/**
 * Screen 5a (spec 111 §5.5): the roles and the skills the workspace holds. Choosing one opens its manifest across the full
 * width, a role's headed by its mascot, with a way back. Read-only.
 */
export function Team({ bridge, info, light }: Props) {
  const [docs, setDocs] = useState<DocRow[]>([]);
  const [skills, setSkills] = useState<DocRow[]>([]);
  const [session, setSession] = useState<Record<string, unknown>>({});
  const [filter, setFilter] = useState('');
  const [open, setOpen] = useState(() => (typeof location !== 'undefined' ? docInAddress(location.search) : ''));
  const [problem, setProblem] = useState('');

  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    // Asked for by collection and by kind, and read to the end: the newest documents of the whole knowledge base hold
    // only the manifests that changed lately.
    const list = (where: { kind?: string; collection?: string }, set: (d: DocRow[]) => void) =>
      listAll((t) => bridge.kb(t), where).then((r) => { if (gone) return; if (r.rows.length) set(r.rows); if (r.status !== 200) setProblem(`The knowledge base answered ${r.status}.`); }).catch((e: unknown) => { if (!gone) setProblem(String(e)); });
    void list({ collection: 'agents' }, setDocs);
    void list({ kind: 'skill' }, setSkills);
    bridge.session().then((s) => { if (!gone) setSession(s); }).catch(() => {});
    return () => { gone = true; };
  }, [bridge]);

  const known = spec(info).roles;
  const agents = useMemo(() => teamRows(docs, session), [docs, session]);
  const skillList = useMemo(() => skillRows(skills).filter((s) => !filter.trim() || s.name.toLowerCase().includes(filter.trim().toLowerCase())), [skills, docs, filter]);

  if (open) {
    const role = agents.find((a) => a.path === open)?.role;
    return (
      <main className="main page">
        <DocView bridge={bridge} info={info} light={light} path={open} back={{ label: 'Back to agents and skills', go: () => setOpen('') }}
          lead={role && role in known ? <Mascot info={info} role={role} light={light} size={12} /> : undefined} />
      </main>
    );
  }

  return (
    <main className="main">
      <div className="stack" style={{ gap: 6 }}>
        <h1 className="title">Agents and skills</h1>
        <div style={{ fontSize: 13, color: 'var(--text2)' }}>What each role is told and what it can call on. This app only reads them; change one in its file or in the console.</div>
      </div>
      {problem && <div className="empty">{problem}</div>}

      <section className="stack">
        <h2 className="caps">Agents · {agents.length}</h2>
        {agents.length === 0 && !problem && <div className="muted" style={{ fontSize: 13 }}>No agent manifest is listed in this workspace's knowledge base.</div>}
        <div className="tiles" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))' }}>
          {agents.map((a) => (
            <button key={a.path} type="button" className="card link" onClick={() => setOpen(a.path)}>
              <span className="row">
                {a.role in known ? <Agent info={info} role={a.role} light={light} /> : <strong>{a.role}</strong>}
                {a.flag && <span className="chip warn push">{a.flag}</span>}
              </span>
              <span className="time" style={{ overflowWrap: 'anywhere' }}>{a.path}</span>
            </button>
          ))}
        </div>
      </section>

      <section className="stack">
        <div className="head-row">
          <h2 className="caps">Skills · {skillList.length}</h2>
          <div className="stack" style={{ gap: 4, width: 280, maxWidth: '100%' }}>
            <label htmlFor="skillq" className="muted" style={{ fontSize: 12 }}>Find a skill by name</label>
            <input id="skillq" className="field" type="search" value={filter} onChange={(e) => setFilter(e.target.value)} />
          </div>
        </div>
        <div className="tiles" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: 8 }}>
          {skillList.map((s) => (
            <button key={s.path} type="button" className="card link" style={{ padding: '10px 14px', gap: 4 }} onClick={() => setOpen(s.path)}>
              <span style={{ fontWeight: 600 }}>{s.name}</span>
              <span className="muted" style={{ fontSize: 12 }}>{s.source}</span>
            </button>
          ))}
        </div>
      </section>
    </main>
  );
}
