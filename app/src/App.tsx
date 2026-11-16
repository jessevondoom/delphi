import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Asking, type Asker } from './EncyParts.tsx';
import { MARK_ROLES, fillOf } from './avatar.ts';
import { openBridge, type Bridge } from './bridge.ts';
import { load, save, type Prefs, type Screen } from './prefs.ts';
import { canAsk, connWords, requestStates } from './requests.ts';
import { Activity } from './screens/Activity.tsx';
import { Council } from './screens/Council.tsx';
import { Encyclopedia } from './screens/Encyclopedia.tsx';
import { Knowledge } from './screens/Knowledge.tsx';
import { Memory } from './screens/Memory.tsx';
import { Output } from './screens/Output.tsx';
import { Settings } from './screens/Settings.tsx';
import { Team } from './screens/Team.tsx';
import type { Conn, Envelope, Info, OState } from './types.ts';
import { defaultCouncil, isDecided, phaseNote, tilde, waiting } from './view.ts';

const MAX_EVENTS = 4000;
const REFETCH_MS = 300;
const ITEMS: { key: Screen; label: string }[] = [
  { key: 'activity', label: 'Activity' },
  { key: 'council', label: 'Councils' },
  { key: 'encyclopedia', label: 'Encyclopedia' },
  { key: 'knowledge', label: 'Knowledge base' },
  { key: 'memory', label: 'Agent memory' },
  { key: 'output', label: 'Work output' },
  { key: 'agents', label: 'Agents and skills' },
  { key: 'settings', label: 'Settings' },
];

function useLight(theme: Prefs['theme']): boolean {
  const query = typeof window !== 'undefined' && window.matchMedia ? window.matchMedia('(prefers-color-scheme: light)') : null;
  const [system, setSystem] = useState(query?.matches ?? false);
  useEffect(() => {
    if (!query) return;
    const on = (e: MediaQueryListEvent) => setSystem(e.matches);
    query.addEventListener('change', on);
    return () => query.removeEventListener('change', on);
  }, [query]);
  useEffect(() => {
    if (theme === 'system') document.documentElement.removeAttribute('data-theme');
    else document.documentElement.setAttribute('data-theme', theme);
  }, [theme]);
  return theme === 'system' ? system : theme === 'light';
}

export function App() {
  const [prefs, setPrefs] = useState<Prefs>(load);
  const [bridge, setBridge] = useState<Bridge | null>(null);
  const [info, setInfo] = useState<Info | null>(null);
  const [state, setState] = useState<OState | null>(null);
  const [events, setEvents] = useState<Envelope[]>([]);
  const [conn, setConn] = useState<Conn>({ connected: false, detail: 'looking for the daemon' });
  const [councilPick, setCouncilPick] = useState<number | null>(null);
  // Counts the times a menu item was chosen: a screen showing a document goes back to its start when its item is chosen again.
  const [visit, setVisit] = useState(0);
  // A search the encyclopedia handed to the knowledge base (spec 117 §4 item 1): the words, until the screen is left.
  const [ask, setAsk] = useState('');
  const light = useLight(prefs.theme);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const update = useCallback((patch: Partial<Prefs>) => setPrefs((p) => { const n = { ...p, ...patch }; save(n); return n; }), []);

  useEffect(() => {
    let off = () => {};
    let gone = false;
    void openBridge().then((b) => {
      if (gone) return;
      setBridge(b);
      // The daemon folds the state; the window asks for it again, at most every 300 ms, when something happened (spec 111 §3.3).
      const refetch = () => { void b.state().then((s) => { if (!gone) setState(s); }).catch(() => {}); };
      const all = () => {
        void b.info().then((i) => { if (!gone) setInfo(i); }).catch(() => {});
        void b.snapshot().then((ev) => { if (!gone) setEvents(ev.slice(-MAX_EVENTS)); }).catch(() => {});
        refetch();
      };
      const soon = () => { if (!timer.current) timer.current = setTimeout(() => { timer.current = null; refetch(); }, REFETCH_MS); };
      off = b.subscribe({
        event: (e) => { setEvents((old) => (old.length >= MAX_EVENTS ? [...old.slice(-MAX_EVENTS + 1), e] : [...old, e])); soon(); },
        conn: (c) => { setConn(c); if (c.connected) all(); },
        resync: all,
      });
    });
    return () => { gone = true; off(); if (timer.current) clearTimeout(timer.current); };
  }, []);

  const waits = state ? waiting(state).length : 0;
  const said = connWords(conn, bridge?.kind === 'fixture');
  // What a page needs in order to ask (spec 120 §6): only a window whose key holds `request` asks; the rest copy commands.
  const asker: Asker = useMemo(() => ({
    can: !!bridge && bridge.kind === 'app' && canAsk(conn),
    send: (r) => (bridge ? bridge.request(r.what, r.arg) : Promise.reject(new Error('not connected to the daemon'))),
    states: requestStates(events),
    waits,
  }), [bridge, conn, events, waits]);
  const live = state?.councils.filter((c) => !isDecided(c)).length ?? 0;
  const notes: Partial<Record<Screen, string>> = {
    activity: waits ? `${waits} waiting on you` : '',
    council: live ? `${live} live` : '',
    knowledge: state?.pending_proposals ? `${state.pending_proposals} pending review` : '',
  };
  const fold = (key: string) => update({ folded: { ...prefs.folded, [key]: !prefs.folded[key] } });
  const council = state && state.councils.length ? Math.min(councilPick ?? defaultCouncil(state), state.councils.length - 1) : 0;
  const clipped = (t: string) => (t.length > 64 ? t.slice(0, 63) + '…' : t);
  const sections: { key: string; title: string; rows: { label: string; note?: string; on?: boolean; pick?: () => void }[] }[] = [
    { key: 'councils', title: 'Councils', rows: (state?.councils ?? []).map((c, i) => ({ label: clipped(c.proposition), note: phaseNote(c), on: prefs.screen === 'council' && i === council, pick: () => { setCouncilPick(i); update({ screen: 'council' }); } })).reverse() },
  ].filter((s) => s.rows.length > 0);

  return (
    <div className="app">
      <header className="titlebar">
        <button type="button" className="icon-btn" aria-label="Show or hide the menu" aria-expanded={prefs.menu} onClick={() => update({ menu: !prefs.menu })}>
          <svg aria-hidden="true" width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><path d="M3 5h14M3 10h14M3 15h14" /></svg>
        </button>
        <span className="brand">
          <span className="mark" role="img" aria-label="Delphi">{MARK_ROLES.map((r) => <span key={r} style={{ background: fillOf(info, r, light) }} />)}</span>
          delphi
        </span>
        {/* Display only: the daemon serves one workspace and no read call takes a project, so there is nothing to switch (spec 111 §8). */}
        <span className="where">
          {info?.workspace_name && <b>{info.workspace_name}</b>}
          {info?.workspace && <span>{tilde(info.workspace)}</span>}
          {info?.version && <span>delphi {info.version}</span>}
        </span>
        <span className="spacer" />
        {/* Spec 120 §6 item 2: read-only or paired, and the grants held; a key that was not accepted says why. */}
        <span className="conn" role="status"><span className={conn.connected ? 'dot on' : 'dot off'} />{said.text}{said.why && <span className="conn-why">{said.why}</span>}</span>
      </header>
      <div className="body">
        {prefs.menu && (
          <nav className="menu" aria-label="Sections">
            <div className="menu-items">
              {ITEMS.map((it) => (
                <button key={it.key} type="button" className="menu-item" aria-current={prefs.screen === it.key ? 'page' : undefined} onClick={() => { setVisit((v) => v + 1); setAsk(''); update({ screen: it.key }); }}>
                  <span>{it.label}</span>
                  {notes[it.key] && <small>{notes[it.key]}</small>}
                </button>
              ))}
            </div>
            {sections.map((s) => (
              <div key={s.key} className="menu-rows">
                <button type="button" className="menu-head caps" aria-expanded={!prefs.folded[s.key]} onClick={() => fold(s.key)}>
                  <span>{s.title}</span><span aria-hidden="true">{prefs.folded[s.key] ? '▸' : '▾'}</span>
                </button>
                {!prefs.folded[s.key] && s.rows.map((r) => (r.pick ? (
                  <button key={r.label} type="button" className={r.on ? 'menu-row on' : 'menu-row'} aria-current={r.on ? 'true' : undefined} onClick={r.pick}><span>{r.label}{r.note && <small>{r.note}</small>}</span></button>
                ) : (
                  <div key={r.label} className={r.on ? 'menu-row on' : 'menu-row'}><span>{r.label}{r.note && <small>{r.note}</small>}</span></div>
                )))}
              </div>
            ))}
          </nav>
        )}
        {!conn.connected && !state ? (
          <main className="main">
            <h1 className="title">No daemon to show</h1>
            <div className="empty">{conn.detail}. Start Delphi in a terminal with <span className="mono">delphi chat</span>, and this window attaches by itself. Without a key paired from a terminal it only reads.</div>
          </main>
        ) : prefs.screen === 'activity' ? (
          <Activity bridge={bridge} info={info} state={state} events={events} light={light} />
        ) : prefs.screen === 'council' ? (
          <Council key={council} info={info} state={state} events={events} light={light} pick={council} />
        ) : prefs.screen === 'knowledge' ? (
          <Knowledge key={visit} bridge={bridge} info={info} state={state} light={light} ask={ask} />
        ) : prefs.screen === 'encyclopedia' ? (
          <Asking.Provider value={asker}>
            <Encyclopedia key={visit} bridge={bridge} info={info} light={light} searchKb={(q) => { setAsk(q); setVisit((v) => v + 1); update({ screen: 'knowledge' }); }} />
          </Asking.Provider>
        ) : prefs.screen === 'memory' ? (
          <Memory key={visit} bridge={bridge} info={info} light={light} />
        ) : prefs.screen === 'output' ? (
          <Output key={visit} bridge={bridge} info={info} state={state} light={light} />
        ) : prefs.screen === 'agents' ? (
          <Team key={visit} bridge={bridge} info={info} light={light} />
        ) : (
          <Settings bridge={bridge} info={info} conn={conn} prefs={prefs} update={update} />
        )}
      </div>
    </div>
  );
}
