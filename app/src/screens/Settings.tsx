import { useEffect, useState } from 'react';
import type { Bridge } from '../bridge.ts';
import type { Prefs } from '../prefs.ts';
import { daemonFacts } from '../team.ts';
import type { Conn, Info } from '../types.ts';
import { tilde } from '../view.ts';

type Props = { bridge: Bridge | null; info: Info | null; conn: Conn; prefs: Prefs; update: (patch: Partial<Prefs>) => void };
const THEMES: { key: Prefs['theme']; label: string }[] = [{ key: 'system', label: 'Follow the system' }, { key: 'dark', label: 'Dark' }, { key: 'light', label: 'Light' }];

/** Screen 5b (spec 111 §5.5): this window's own preferences, which it keeps, and the daemon's settings, which it only reads. */
export function Settings({ bridge, info, conn, prefs, update }: Props) {
  const [session, setSession] = useState<Record<string, unknown>>({});
  useEffect(() => {
    if (!bridge || !conn.connected) return;
    let gone = false;
    bridge.session().then((s) => { if (!gone) setSession(s); }).catch(() => {});
    return () => { gone = true; };
  }, [bridge, conn.connected]);
  const facts = daemonFacts(session);

  return (
    <main className="main" style={{ maxWidth: 860 }}>
      <h1 className="title">Settings</h1>

      <section className="stack">
        <h2 className="caps">This window</h2>
        <div className="card">
          <div className="row">
            <span style={{ fontWeight: 600 }}>Theme</span>
            <div className="pills push" role="group" aria-label="Theme">
              {THEMES.map((t) => <button key={t.key} type="button" className="pill" aria-pressed={prefs.theme === t.key} onClick={() => update({ theme: t.key })}>{t.label}</button>)}
            </div>
          </div>
          <div className="row">
            <span style={{ fontWeight: 600 }}>Menu</span>
            <div className="pills push" role="group" aria-label="Menu">
              <button type="button" className="pill" aria-pressed={prefs.menu} onClick={() => update({ menu: true })}>Shown</button>
              <button type="button" className="pill" aria-pressed={!prefs.menu} onClick={() => update({ menu: false })}>Hidden</button>
            </div>
          </div>
          <div className="muted" style={{ fontSize: 12 }}>Kept by this window only. Nothing here is written to Delphi's files.</div>
        </div>
      </section>

      <section className="stack">
        <h2 className="caps">Connection</h2>
        <dl className="facts">
          <div><dt>Daemon</dt><dd>{conn.connected ? 'connected' : 'not connected'}</dd></div>
          <div><dt>Access</dt><dd>read-only (narrator scope)</dd></div>
          {info?.version && <div><dt>Delphi</dt><dd>{info.version}</dd></div>}
          {info?.workspace_name && <div><dt>Workspace</dt><dd>{info.workspace_name}</dd></div>}
        </dl>
        {info?.narrator_socket && <div className="time" style={{ overflowWrap: 'anywhere' }}>{tilde(info.narrator_socket)}</div>}
        <div style={{ fontSize: 13, color: 'var(--text2)', lineHeight: 1.55 }}>This window can read what the daemon knows. It cannot approve anything, change a setting, edit an agent or a skill, or start or stop work, and it never starts a daemon.</div>
        {!conn.connected && <div className="empty">{conn.detail}</div>}
      </section>

      <section className="stack">
        <h2 className="caps">The daemon's settings</h2>
        {facts.length === 0 ? <div className="empty">{conn.connected ? 'The daemon has not answered yet.' : 'Shown when a daemon is connected.'}</div> : (
          <dl className="facts">{facts.map((f) => <div key={f.label}><dt>{f.label}</dt><dd>{f.value}</dd></div>)}</dl>
        )}
        <div style={{ fontSize: 13, color: 'var(--text2)' }}>To change these, use the console: <span className="mono">/permissions</span>, <span className="mono">/budget</span>.</div>
      </section>
    </main>
  );
}
