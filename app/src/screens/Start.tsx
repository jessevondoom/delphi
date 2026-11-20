import { useState } from 'react';
import { INSTALL_COMMAND, START_COMMAND, cards, switchable, type Card, type DaemonList } from '../daemons.ts';
import type { Conn } from '../types.ts';
import { UpdateBanner } from '../UpdateBanner.tsx';

async function toClipboard(text: string): Promise<boolean> {
  try { await navigator.clipboard.writeText(text); return true; } catch { return false; }
}

/**
 * A command to run in a terminal, shown and copied by a button. This window starts nothing (spec 111 §2, spec 130 §1.1): the
 * only thing the button does is put the text on the clipboard, and it says so.
 */
export function CopyButton({ command, label = 'Copy command' }: { command: string; label?: string }) {
  const [done, setDone] = useState<boolean | null>(null);
  return (
    <span className="copy-line">
      <code className="mono">{command}</code>
      <button type="button" className="btn small" onClick={() => { void toClipboard(command).then(setDone); }}>{label}</button>
      {done !== null && <span className="muted" role="status">{done ? 'Copied. Run it in a terminal; this window does not start it.' : 'The clipboard was not available; copy the command above by hand.'}</span>}
    </span>
  );
}

/** What a card says about the daemon's pairing with this window: known only for the daemon the window is attached to. */
function pairingWords(conn: Conn | null): string | null {
  if (!conn || !conn.connected) return null;
  return conn.paired ? `paired as ${conn.paired}` : 'read-only';
}

function Body({ c, pairing }: { c: Card; pairing: string | null }) {
  return (
    <>
      <span className="daemon-head">
        <b>{c.title}</b>
        <span className="daemon-state" data-state={c.state}>{c.state}</span>
      </span>
      {c.path && <span className="muted mono daemon-path">{c.path}</span>}
      <span className="muted daemon-version">{c.version ? `delphi ${c.version}` : 'version not known'}{pairing ? ` · ${pairing}` : ''}</span>
      {c.versionNote && <span className="daemon-note">{c.versionNote}</span>}
      {c.lines.map((l) => <span key={l} className="daemon-line">{l}</span>)}
      {c.needsYou && <span className="daemon-needs">{c.needsYou}</span>}
    </>
  );
}

/**
 * One card per daemon (spec 130 §4 items 1 to 4), in the order of `daemons.ts`. A daemon that answers is a button that
 * attaches. A stopped one is no button at all but the command that starts it, with a copy button: nothing here starts a
 * process. A card that waits on you says where it is answered, and has no control that answers it.
 */
export function Cards({ list, now, current, conn, onChoose }: { list: DaemonList; now: number; current: string | null; conn: Conn | null; onChoose: (hash: string) => void }) {
  return (
    <div className="daemon-cards">
      {cards(list, now).map((c) => {
        const pairing = c.hash === current ? pairingWords(conn) : null;
        return c.attachable ? (
          <button key={c.hash} type="button" className={c.hash === current ? 'card link daemon on' : 'card link daemon'} data-state={c.state} aria-current={c.hash === current ? 'true' : undefined} onClick={() => onChoose(c.hash)}>
            <Body c={c} pairing={pairing} />
          </button>
        ) : (
          <div key={c.hash} className="card daemon" data-state={c.state}>
            <Body c={c} pairing={pairing} />
            {c.command ? <><span className="daemon-line">To start it, run this in a terminal:</span><CopyButton command={c.command} /></> : c.state === 'stopped' && <span className="daemon-line">Start it in a terminal.</span>}
          </div>
        );
      })}
    </div>
  );
}

/**
 * The start screen (spec 130 §4): shown when several daemons run and none was named, and when none runs. With none, it says
 * what to run, and how to install Delphi if there is nothing to run.
 */
export function Start({ list, now, conn, onChoose, error }: { list: DaemonList | null; now: number; conn: Conn | null; onChoose: (hash: string) => void; error: string }) {
  const none = switchable(list) === 0;
  return (
    <main className="main start" style={{ maxWidth: 900 }}>
      <h1 className="title">{none ? 'Nothing is running' : 'Choose a daemon'}</h1>
      {!list && <div className="empty" role="status">Looking for daemons.</div>}
      {list && none && (
        <div className="empty stack" role="status">
          <span>Nothing is running. Start Delphi in a terminal, and this window attaches by itself:</span>
          <CopyButton command={START_COMMAND} />
          <span>No <span className="mono">delphi</span> on this machine yet? Install it with:</span>
          <CopyButton command={INSTALL_COMMAND} />
        </div>
      )}
      {list && !none && <div className="muted" style={{ marginBottom: 12 }}>More than one daemon is running here. Choose the one to show; this window reads it, and answers nothing.</div>}
      {error && <div className="empty" role="alert">{error}</div>}
      {/* Spec 120 §13.2 item 12: a daemon that names a newer release, or says it was updated, shows it with the command to copy. */}
      {list && list.daemons.filter((d) => d.state === 'running').map((d) => (
        <UpdateBanner key={d.hash} d={d} attached={false} paired={false} />
      ))}
      {list && list.daemons.length > 0 && <Cards list={list} now={now} current={list.current} conn={conn} onChoose={onChoose} />}
    </main>
  );
}
