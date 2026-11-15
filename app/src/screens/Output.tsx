import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { Agent, hasMascot } from '../Badge.tsx';
import { DocView } from '../DocView.tsx';
import { KindIcon } from '../KindIcon.tsx';
import { fillOf } from '../avatar.ts';
import type { Bridge } from '../bridge.ts';
import { dayOf, listTarget, type DocRow } from '../kb.ts';
import { GROUP_LABEL, canOpen, fileKind, groupCounts, iconOf, items, type Group, type Item } from '../output.ts';
import type { Info, OState } from '../types.ts';
import { clock } from '../view.ts';

type Props = { bridge: Bridge | null; info: Info | null; state: OState | null; light: boolean };
const LISTED = 200;

const when = (ms: number): string => (ms ? `${dayOf(new Date(ms).toISOString())} ${clock(ms)}` : '');

/**
 * Screen 4 (spec 111 §5.4): what the team produced, grouped by what is there, as a list or a grid. Choosing an item opens it
 * across the full width with a way back; nothing is shown in a column beside the list.
 */
export function Output({ bridge, info, state, light }: Props) {
  const [rows, setRows] = useState<DocRow[] | null>(null);
  const [problem, setProblem] = useState('');
  // The grid can be asked for in the address (`?view=grid`), as a search or a document can on the other screens.
  const [view, setView] = useState<'grid' | 'list'>(() => (typeof location !== 'undefined' && new URLSearchParams(location.search).get('view') === 'grid' ? 'grid' : 'list'));
  const [group, setGroup] = useState<Group | 'all'>('all');
  const [picked, setPicked] = useState('');
  const [said, setSaid] = useState('');

  useEffect(() => {
    if (!bridge) return;
    let gone = false;
    bridge.kb(listTarget(LISTED))
      .then((r) => { if (gone) return; if (r.status === 200) setRows(r.body as DocRow[]); else setProblem(`The knowledge base answered ${r.status}.`); })
      .catch((e: unknown) => { if (!gone) setProblem(e instanceof Error ? e.message : String(e)); });
    return () => { gone = true; };
  }, [bridge]);

  const all = useMemo(() => items(rows ?? [], state), [rows, state]);
  const groups = groupCounts(all);
  const shown = all.filter((x) => group === 'all' || x.group === group);
  const sel = picked ? all.find((x) => x.key === picked) : undefined;
  // A role is written as an agent always is; any other author (the operator, an import) is a name with no chip or colour.
  const isRole = (role?: string): role is string => !!role && hasMascot(info, role);
  const tint = (role?: string) => (isRole(role) ? ({ '--c': fillOf(info, role, light) } as CSSProperties) : undefined);
  const by = (x: Item) => (isRole(x.role) ? <Agent info={info} role={x.role} light={light} /> : x.role ? <span className="muted">{x.role}</span> : null);
  const outside = (act: ((p: string) => Promise<void>) | undefined, path: string) => { setSaid(''); void act?.(path).catch((e: unknown) => setSaid(String(e))); };
  const back = { label: 'Back to work output', go: () => { setPicked(''); setSaid(''); } };

  if (sel?.path) return <main className="main page"><DocView bridge={bridge} info={info} light={light} path={sel.path} back={back} /></main>;
  if (sel) {
    return (
      <main className="main page">
        <div className="row"><button type="button" className="btn back" onClick={back.go}><span aria-hidden="true">←</span> {back.label}</button><span className="chip line">{GROUP_LABEL[sel.group]}</span><span className="chip">{sel.status}</span></div>
        <h1 className="question" style={{ fontSize: 26 }}>{sel.title}</h1>
        <dl className="facts">
          {sel.role && <div><dt>By</dt><dd>{by(sel)}</dd></div>}
          {when(sel.at_ms) && <div><dt>When</dt><dd>{when(sel.at_ms)}</dd></div>}
          {sel.mission && <div><dt>Mission</dt><dd>{sel.mission}</dd></div>}
          <div><dt>Files named</dt><dd>{sel.files.length || 'none'}</dd></div>
        </dl>
        {sel.files.length > 0 && (
          <section className="stack" style={{ gap: 10, maxWidth: 860 }}>
            <h2 className="caps">Files named</h2>
            {sel.files.map((f) => (
              <div key={f} className="card" style={{ padding: '10px 14px' }}>
                <div className="row">
                  <span className="chip line">{fileKind(f)}</span><span className="mono" style={{ fontSize: 13, overflowWrap: 'anywhere' }}>{f}</span>
                  <span className="row push">
                    {bridge?.open && canOpen(f) && <button type="button" className="btn small" onClick={() => outside(bridge.open, f)}>Open</button>}
                    {bridge?.reveal && <button type="button" className="btn small" onClick={() => outside(bridge.reveal, f)}>Show in Finder</button>}
                  </span>
                </div>
              </div>
            ))}
            {said && <div className="muted" style={{ fontSize: 12 }} role="status">{said}</div>}
            <div className="muted" style={{ fontSize: 12 }}>A deck, a page, a picture or a recording opens in your browser or viewer. Paths are as the agent named them, looked for in the workspace and in your registered projects.</div>
          </section>
        )}
      </main>
    );
  }

  return (
    <main className="main">
      <div className="head-row" style={{ alignItems: 'flex-start' }}>
        <div className="stack" style={{ gap: 6 }}>
          <h1 className="title">Work output</h1>
          <div style={{ fontSize: 13, color: 'var(--text2)' }}>
            {rows === null && !problem ? 'Asking the knowledge base what the team has written.' : problem || `What the team has made: ${all.length} ${all.length === 1 ? 'item' : 'items'}, newest first.`}
            {rows?.length === LISTED && ` Only the ${LISTED} most recently changed documents are listed.`}
          </div>
        </div>
        <div className="pills" role="group" aria-label="View">
          <button type="button" className="pill" aria-pressed={view === 'list'} onClick={() => setView('list')}><KindIcon icon="list" size={16} />List</button>
          <button type="button" className="pill" aria-pressed={view === 'grid'} onClick={() => setView('grid')}><KindIcon icon="grid" size={16} />Grid</button>
        </div>
      </div>

      {groups.length > 1 && (
        <div className="pills" role="group" aria-label="What to show">
          <button type="button" className="pill" aria-pressed={group === 'all'} onClick={() => setGroup('all')}>Everything {all.length}</button>
          {groups.map((g) => <button key={g.group} type="button" className="pill" aria-pressed={group === g.group} onClick={() => setGroup(g.group)}>{g.label} {g.count}</button>)}
        </div>
      )}

      {shown.length === 0 && rows !== null && <div className="empty">Nothing here yet. Documents the team writes and changes agents record appear as they are made.</div>}

      {view === 'grid' ? (
        <div className="tiles" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))' }}>
          {shown.map((x) => (
            <button key={x.key} type="button" className="card link out-card" style={tint(x.role)} onClick={() => setPicked(x.key)}>
              <span className="out-thumb"><KindIcon icon={iconOf(x)} size={34} /></span>
              <span className="out-body">
                <span className="row" style={{ fontSize: 11 }}><span className="caps">{GROUP_LABEL[x.group]}</span><span className={x.status === 'draft' ? 'chip warn push' : 'chip push'}>{x.status}</span></span>
                <span style={{ fontWeight: 600, fontSize: 15, lineHeight: 1.35 }}>{x.title}</span>
                {x.detail && <span className="muted" style={{ fontSize: 12 }}>{x.detail}</span>}
                <span className="row" style={{ fontSize: 12, marginTop: 'auto' }}>{by(x)}<span className="time push">{when(x.at_ms)}</span></span>
              </span>
            </button>
          ))}
        </div>
      ) : shown.length > 0 && (
        // The list says what a thing is and who made it; where it lives and when it changed are on its own page.
        <div className="table-wrap">
          <table className="table">
            <thead><tr><th scope="col">Name</th><th scope="col">By</th><th scope="col">What</th><th scope="col">Status</th></tr></thead>
            <tbody>
              {shown.map((x) => (
                <tr key={x.key}>
                  <td style={{ width: '48%' }}><button type="button" className="cell-btn with-icon" onClick={() => setPicked(x.key)}><KindIcon icon={iconOf(x)} /><span>{x.title}</span></button></td>
                  <td>{by(x)}</td>
                  <td className="muted">{GROUP_LABEL[x.group]}{x.detail ? ` · ${x.detail}` : ''}</td>
                  <td><span className={x.status === 'draft' ? 'chip warn' : 'chip'}>{x.status}</span></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </main>
  );
}
