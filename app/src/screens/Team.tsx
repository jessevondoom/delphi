import { useEffect, useMemo, useState } from 'react';
import { Agent } from '../Badge.tsx';
import { DocView } from '../DocView.tsx';
import { spec } from '../avatar.ts';
import type { Bridge } from '../bridge.ts';
import { listTarget, type DocRow } from '../kb.ts';
import { skillRows, teamRows } from '../team.ts';
import type { Info } from '../types.ts';

type Props = { bridge: Bridge | null; info: Info | null; light: boolean };

/** Screen 5a (spec 111 §5.5): the roles and the skills the workspace holds, each with its manifest. Read-only. */
export function Team({ bridge, info, light }: Props) {
  const [docs, setDocs] = useState<DocRow[]>([]);
  const [skills, setSkills] = useState<DocRow[]>([]);
  const [session, setSession] = useState<Record<string, unknown>>({});
  const [filter, setFilter] = useState('');
  const [open, setOpen] = useState('');
  const [problem, setProblem] = useState('');

  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    const list = (target: string, set: (d: DocRow[]) => void) =>
      bridge.kb(target).then((r) => { if (gone) return; if (r.status === 200) set(r.body as DocRow[]); else setProblem(`The knowledge base answered ${r.status}.`); }).catch((e: unknown) => { if (!gone) setProblem(String(e)); });
    void list(listTarget(200), setDocs);
    void list(listTarget(200, 'skill'), setSkills);
    bridge.session().then((s) => { if (!gone) setSession(s); }).catch(() => {});
    return () => { gone = true; };
  }, [bridge]);

  const known = spec(info).roles;
  const agents = useMemo(() => teamRows(docs, session), [docs, session]);
  const skillList = useMemo(() => skillRows(skills.length ? skills : docs).filter((s) => !filter.trim() || s.name.toLowerCase().includes(filter.trim().toLowerCase())), [skills, docs, filter]);
  const first = agents[0]?.path ?? skillList[0]?.path ?? '';
  const path = open || first;

  return (
    <>
      <section className="results" aria-label="Agents and skills">
        <h1 className="title">Agents and skills</h1>
        <div style={{ fontSize: 13, color: 'var(--text2)' }}>What each role is told and what it can call on. This app only reads them; change one in its file or in the console.</div>
        {problem && <div className="empty">{problem}</div>}
        <h2 className="caps">Agents · {agents.length}</h2>
        <div className="stack" style={{ gap: 6 }}>
          {agents.length === 0 && !problem && <div className="muted" style={{ fontSize: 13 }}>No agent manifest is listed in this workspace's knowledge base.</div>}
          {agents.map((a) => (
            <button key={a.path} type="button" className={path === a.path ? 'card link on' : 'card link'} aria-pressed={path === a.path} onClick={() => setOpen(a.path)} style={{ padding: '10px 12px' }}>
              <span className="row">
                {a.role in known ? <Agent info={info} role={a.role} light={light} /> : <strong>{a.role}</strong>}
                {a.flag && <span className={a.flag === 'not allowed' ? 'chip warn push' : 'chip push'}>{a.flag}</span>}
              </span>
              <span className="time" style={{ overflowWrap: 'anywhere' }}>{a.path}</span>
            </button>
          ))}
        </div>
        <h2 className="caps">Skills · {skillList.length}</h2>
        <label htmlFor="skillq" className="muted" style={{ fontSize: 12 }}>Find a skill by name</label>
        <input id="skillq" className="field" type="search" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <div className="stack" style={{ gap: 4 }}>
          {skillList.map((s) => (
            <button key={s.path} type="button" className={path === s.path ? 'menu-row on' : 'menu-row'} aria-pressed={path === s.path} onClick={() => setOpen(s.path)}>
              <span>{s.name}<small>{s.source}</small></span>
            </button>
          ))}
        </div>
      </section>
      <article className="main" aria-label="Manifest">
        {path ? <DocView bridge={bridge} info={info} light={light} path={path} /> : <div className="empty">Choose an agent or a skill to read its manifest.</div>}
      </article>
    </>
  );
}
