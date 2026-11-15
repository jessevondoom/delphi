import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { Agent } from '../Badge.tsx';
import { DocView } from '../DocView.tsx';
import { fillOf } from '../avatar.ts';
import type { Bridge } from '../bridge.ts';
import { dayOf, listTarget, type DocRow } from '../kb.ts';
import { GROUP_LABEL, canOpen, fileKind, groupCounts, items, type Group, type Item } from '../output.ts';
import type { Info, OState } from '../types.ts';
import { clock } from '../view.ts';

type Props = { bridge: Bridge | null; info: Info | null; state: OState | null; light: boolean };
const LISTED = 200;

const when = (ms: number): string => (ms ? `${dayOf(new Date(ms).toISOString())} ${clock(ms)}` : '');

/** Screen 4 (spec 111 §5.4): what the team produced, grouped by what is there, as a list or a grid, with where each lives. */
export function Output({ bridge, info, state, light }: Props) {
  const [rows, setRows] = useState<DocRow[] | null>(null);
  const [problem, setProblem] = useState('');
  const [view, setView] = useState<'grid' | 'list'>('list');
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
  const sel = all.find((x) => x.key === picked) ?? shown[0];
  const tint = (role?: string) => (role ? ({ '--c': fillOf(info, role, light) } as CSSProperties) : undefined);
  const by = (x: Item) => (x.role ? <Agent info={info} role={x.role} light={light} /> : null);
  const outside = (act: ((p: string) => Promise<void>) | undefined, path: string) => { setSaid(''); void act?.(path).catch((e: unknown) => setSaid(String(e))); };

  return (
    <>
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
            <button type="button" className="pill" aria-pressed={view === 'list'} onClick={() => setView('list')}>List</button>
            <button type="button" className="pill" aria-pressed={view === 'grid'} onClick={() => setView('grid')}>Grid</button>
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
          <div className="tiles" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))' }}>
            {shown.map((x) => (
              <button key={x.key} type="button" className={sel?.key === x.key ? 'card link on' : 'card link'} aria-pressed={sel?.key === x.key} style={tint(x.role)} onClick={() => setPicked(x.key)}>
                <span className="row" style={{ fontSize: 11 }}><span className="caps">{GROUP_LABEL[x.group]}</span><span className={x.status === 'draft' ? 'chip warn push' : 'chip push'}>{x.status}</span></span>
                <span style={{ fontWeight: 600, fontSize: 15, lineHeight: 1.35 }}>{x.title}</span>
                {x.detail && <span className="muted" style={{ fontSize: 12 }}>{x.detail}</span>}
                <span className="row" style={{ fontSize: 12, marginTop: 'auto' }}>{by(x)}<span className="time push">{when(x.at_ms)}</span></span>
              </button>
            ))}
          </div>
        ) : shown.length > 0 && (
          // The list says what a thing is and who made it; where it lives and when it changed are in the pane beside it.
          <div className="table-wrap">
            <table className="table">
              <thead><tr><th scope="col">Name</th><th scope="col">By</th><th scope="col">What</th><th scope="col">Status</th></tr></thead>
              <tbody>
                {shown.map((x) => (
                  <tr key={x.key} className={sel?.key === x.key ? 'on' : undefined}>
                    <td style={{ width: '46%' }}><button type="button" className="cell-btn" aria-pressed={sel?.key === x.key} onClick={() => setPicked(x.key)}>{x.title}</button></td>
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

      <aside className="aside wide" aria-label="Selected item">
        {!sel ? <div className="empty">Choose an item to see it.</div> : sel.path ? (
          <DocView bridge={bridge} info={info} light={light} path={sel.path} />
        ) : (
          <>
            <div className="row">{sel.role && <Agent info={info} role={sel.role} light={light} />}<span className="chip push">{sel.status}</span></div>
            <h2 className="question" style={{ fontSize: 20 }}>{sel.title}</h2>
            <dl className="facts">
              <div><dt>What</dt><dd>{GROUP_LABEL[sel.group]}</dd></div>
              {when(sel.at_ms) && <div><dt>When</dt><dd>{when(sel.at_ms)}</dd></div>}
              {sel.mission && <div><dt>Mission</dt><dd>{sel.mission}</dd></div>}
            </dl>
            {sel.files.length > 0 ? (
              <div className="stack" style={{ gap: 8 }}>
                <h3 className="caps">Files named</h3>
                {sel.files.map((f) => (
                  <div key={f} className="stack" style={{ gap: 6 }}>
                    <div className="row" style={{ fontSize: 12 }}><span className="chip line">{fileKind(f)}</span><span className="mono" style={{ overflowWrap: 'anywhere' }}>{f}</span></div>
                    {(bridge?.open || bridge?.reveal) && (
                      <div className="row">
                        {bridge.open && canOpen(f) && <button type="button" className="btn small" onClick={() => outside(bridge.open, f)}>Open</button>}
                        {bridge.reveal && <button type="button" className="btn small" onClick={() => outside(bridge.reveal, f)}>Show in Finder</button>}
                      </div>
                    )}
                  </div>
                ))}
                {said && <div className="muted" style={{ fontSize: 12 }} role="status">{said}</div>}
                <div className="muted" style={{ fontSize: 12 }}>A deck, a page, a picture or a recording opens in your browser or viewer. Paths are as the agent named them, looked for in the workspace and in your registered projects.</div>
              </div>
            ) : <div className="muted" style={{ fontSize: 13 }}>The agent named no file for this.</div>}
          </>
        )}
      </aside>
    </>
  );
}
