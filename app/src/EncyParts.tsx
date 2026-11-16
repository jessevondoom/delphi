// Pieces the encyclopedia's screens share (spec 117 §4, §6): one read with its problem, the sentence for a window that
// is not paired, and the clipboard actions that stand in for acting until the app holds a `request` grant (spec 120 K5).
import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import type { Bridge } from './bridge.ts';
import type { EncyRead } from './ency.ts';

const words = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/**
 * One encyclopedia read: what came back, or why nothing did. Asked again when the read, its argument or `again` changes.
 * `arg` is compared by its JSON, so a caller may build it in place.
 */
export function useRead<T>(bridge: Bridge | null, read: EncyRead | null, arg: Record<string, unknown> = {}, again = 0): { data: T | null; problem: string } {
  const [got, setGot] = useState<{ key: string; data: T | null; problem: string }>({ key: '', data: null, problem: '' });
  const key = `${read ?? ''} ${JSON.stringify(arg)} ${again}`;
  useEffect(() => {
    if (!bridge || !read) return;
    let gone = false;
    bridge.ency(read, arg).then((d) => { if (!gone) setGot({ key, data: d as T, problem: '' }); }, (e: unknown) => { if (!gone) setGot({ key, data: null, problem: words(e) }); });
    return () => { gone = true; };
    // `key` stands for `read`, `arg` and `again`.
  }, [bridge, key]);
  return got.key === key ? { data: got.data, problem: got.problem } : { data: null, problem: '' };
}

/** Whether a refusal is the bridge saying this window proved no paired key. */
export const needsPairing = (problem: string): boolean => problem.includes('paired key');

/** Why a read gave nothing. A window that is not paired is told how to pair it (spec 120 §6 item 1); anything else is the daemon's sentence. */
export function Problem({ problem, waiting = 'Reading.' }: { problem: string; waiting?: string }) {
  if (!problem) return <div className="empty">{waiting}</div>;
  if (needsPairing(problem)) {
    return (
      <div className="empty" role="status">
        <strong>The encyclopedia is read with a paired key.</strong> In a terminal, run <span className="mono">delphi app</span> once: it starts this app with a ticket, the app keeps a key, and from then on this window is given the encyclopedia and agent memory. The other screens read without one. Nothing here acts either way.
      </div>
    );
  }
  return <div className="empty" role="status">{problem}</div>;
}

async function toClipboard(text: string): Promise<boolean> {
  try { await navigator.clipboard.writeText(text); return true; } catch { return false; }
}

/** What the window says after a command went to the clipboard, or could not. The command is shown either way, so it can be copied by hand. */
function Copied({ command, ok }: { command: string; ok: boolean }) {
  return (
    <div className="copied" role="status">
      <span>{ok ? 'Copied to the clipboard. Run it in a terminal or a host; this app does not start it.' : 'The clipboard was not available. This is the command to run in a terminal:'}</span>
      <code className="mono">{command}</code>
    </div>
  );
}

/** A request that needs no words from the operator ("Rebuild now"): one click puts its command on the clipboard and says so. */
export function CopyCommand({ label, command, small = false }: { label: string; command: string; small?: boolean }) {
  const [done, setDone] = useState<boolean | null>(null);
  if (!command) return null;
  return (
    <span className="ask">
      <button type="button" className={small ? 'btn small' : 'btn'} title="Copies a command; nothing is started from this window" onClick={() => { void toClipboard(command).then(setDone); }}>{label}</button>
      {done !== null && <Copied command={command} ok={done} />}
    </span>
  );
}

/**
 * A request that carries the operator's words ("Ask a follow-up", "Research this section further", "Dispute this
 * statement"): the button opens one line to write in, and sending it puts the composed command on the clipboard.
 * `build` quotes the words for a shell; an empty answer from it means the words are needed first.
 */
export function Ask({ label, prompt, build, max, small = false, lead }: { label: string; prompt: string; build: (words: string) => string; max: number; small?: boolean; lead?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [done, setDone] = useState<{ command: string; ok: boolean } | null>(null);
  const [id] = useState(() => `ask-${Math.random().toString(36).slice(2, 9)}`);
  const send = (e: FormEvent) => {
    e.preventDefault();
    const command = build(text);
    if (command) void toClipboard(command).then((ok) => setDone({ command, ok }));
  };
  if (!open) return <button type="button" className={small ? 'btn small' : 'btn'} title="Composes a command for the clipboard; nothing is started from this window" onClick={() => setOpen(true)}>{label}</button>;
  const ready = build(text) !== '';
  return (
    <form className="ask open" onSubmit={send}>
      {lead}
      <label htmlFor={id} className="muted" style={{ fontSize: 12 }}>{prompt}</label>
      <div className="row">
        <input id={id} className="field" style={{ flex: '1 1 260px' }} type="text" value={text} maxLength={max} autoFocus onChange={(e) => { setText(e.target.value); setDone(null); }} />
        <button type="submit" className="btn" disabled={!ready}>Copy the command</button>
        <button type="button" className="btn" onClick={() => { setOpen(false); setDone(null); setText(''); }}>Close</button>
      </div>
      {done && <Copied command={done.command} ok={done.ok} />}
    </form>
  );
}
