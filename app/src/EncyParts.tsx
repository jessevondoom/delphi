// Pieces the encyclopedia's screens share (spec 117 §4, §6): one read with its problem, the sentence for a window that
// is not paired, and the four things a page offers to start. With a paired key that holds `request` each is a button
// that asks the daemon (spec 120 §6 item 1); without one each composes a command for the clipboard, as before, and
// says how to pair. Nothing here moves: a request's state is a sentence, replaced when the state changes.
import { createContext, useContext, useEffect, useState, type FormEvent, type ReactNode } from 'react';
import type { Bridge } from './bridge.ts';
import type { EncyRead } from './ency.ts';
import { ended, refusalWords, stateWords, type Asked, type Request, type RequestState } from './requests.ts';

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
        <strong>The encyclopedia is read with a paired key.</strong> In a terminal, run <span className="mono">delphi app</span> once: it starts this app with a ticket, the app keeps a key, and from then on this window is given the encyclopedia and agent memory, and may ask for a page to be rebuilt or a topic researched further. The other screens read without one.
      </div>
    );
  }
  return <div className="empty" role="status">{problem}</div>;
}

async function toClipboard(text: string): Promise<boolean> {
  try { await navigator.clipboard.writeText(text); return true; } catch { return false; }
}

/**
 * What a page needs in order to ask (spec 120 §6): whether this window may, the one way to send a request, each
 * request's latest state from the daemon's events, and how many cards wait on the operator in the console.
 */
export type Asker = { can: boolean; send: (r: Request) => Promise<Asked>; states: Map<string, RequestState>; waits: number };
const NO_ASKER: Asker = { can: false, send: () => Promise.reject(new Error('this window cannot ask')), states: new Map(), waits: 0 };
/** The window's asker. Without a provider, and in a window that is not paired, nothing asks and every action is a clipboard command. */
export const Asking = createContext<Asker>(NO_ASKER);

/** What became of one request this control sent: it is being sent, the daemon took it (and gave it an id), or it was not taken. */
export type Sent = { state: 'sending' } | { state: 'taken'; request: string; first: string } | { state: 'not-taken'; why: string };

/** Send `r` and follow what the daemon answers. */
function sendWith(asker: Asker, r: Request, set: (s: Sent) => void) {
  set({ state: 'sending' });
  asker.send(r).then(
    (a) => set({ state: 'taken', request: typeof a.request === 'string' ? a.request : '', first: typeof a.state === 'string' ? a.state : '' }),
    (e: unknown) => set({ state: 'not-taken', why: refusalWords(e) }),
  );
}

/**
 * Where a request stands (spec 120 §6 item 3), as a sentence: being asked, queued, running, done, refused and why, or
 * waiting on a card elsewhere. The state is the daemon's latest `app.request` event for it, or its first answer until
 * one arrives. A status line, announced politely; nothing spins or pulses.
 */
export function RequestLine({ sent }: { sent: Sent }) {
  const asker = useContext(Asking);
  let words: string;
  let over = false;
  if (sent.state === 'sending') words = stateWords({ state: 'sending' });
  else if (sent.state === 'not-taken') { words = `The daemon did not take it: ${sent.why}`; over = true; }
  else {
    const now = asker.states.get(sent.request) ?? { state: sent.first, why: '' };
    words = stateWords(now, ended(now.state) ? 0 : asker.waits);
    over = ended(now.state);
  }
  return <div className="copied" role="status" aria-live="polite" data-over={over ? 'yes' : 'no'}>{words}</div>;
}

/** What the window says after a command went to the clipboard, or could not. The command is shown either way, so it can be copied by hand. */
function Copied({ command, ok }: { command: string; ok: boolean }) {
  return (
    <div className="copied" role="status">
      <span>{ok ? 'Copied to the clipboard. Run it in a terminal or a host; this window does not start it.' : 'The clipboard was not available. This is the command to run in a terminal:'}</span>
      <code className="mono">{command}</code>
      <span>To ask from this window instead, pair it: run <span className="mono">delphi app</span> in a terminal once.</span>
    </div>
  );
}

/**
 * A request that needs no words from the operator ("Rebuild now", "Run it again"). In a window that may ask, one click
 * asks the daemon and the request's state is shown; otherwise one click puts `command` on the clipboard and says so.
 */
export function CopyCommand({ label, command, request = null, small = false }: { label: string; command: string; request?: Request | null; small?: boolean }) {
  const asker = useContext(Asking);
  const [done, setDone] = useState<boolean | null>(null);
  const [sent, setSent] = useState<Sent | null>(null);
  if (asker.can && request) {
    return (
      <span className="ask">
        <button type="button" className={small ? 'btn small' : 'btn'} title="Asks the daemon to start it, as your key's `request` grant allows" disabled={sent?.state === 'sending'} onClick={() => sendWith(asker, request, setSent)}>{label}</button>
        {sent && <RequestLine sent={sent} />}
      </span>
    );
  }
  if (!command) return null;
  return (
    <span className="ask">
      <button type="button" className={small ? 'btn small' : 'btn'} title="Copies a command; nothing is started from this window" onClick={() => { void toClipboard(command).then(setDone); }}>{label}: copy the command</button>
      {done !== null && <Copied command={command} ok={done} />}
    </span>
  );
}

/**
 * A request that carries the operator's words ("Ask a follow-up", "Research this section further", "Dispute this
 * statement"): the button opens one line to write in. In a window that may ask, sending it asks the daemon: `request`
 * is given **the words in the field and nothing else**, and makes the request from them and from ids it already holds.
 * Otherwise sending puts the command `build` composes on the clipboard (`build` quotes the words for a shell). An empty
 * answer from either means the words are needed first.
 *
 * `asking` replaces the prompt and the bound when the request takes other words than the command does (a follow-up is a
 * question to the daemon and a narrowing note to the command).
 */
export function Ask({ label, prompt, build, max, request, asking, small = false, lead }: {
  label: string; prompt: string; build: (words: string) => string; max: number;
  request?: (typed: string) => Request | null; asking?: { prompt: string; max: number };
  small?: boolean; lead?: ReactNode;
}) {
  const asker = useContext(Asking);
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [done, setDone] = useState<{ command: string; ok: boolean } | null>(null);
  const [sent, setSent] = useState<Sent | null>(null);
  const [id] = useState(() => `ask-${Math.random().toString(36).slice(2, 9)}`);
  const asks = asker.can && !!request;
  const send = (e: FormEvent) => {
    e.preventDefault();
    if (asks) {
      // The request is made from the field's own value: what the operator typed, and nothing a page holds.
      const r = request(text);
      if (r) sendWith(asker, r, setSent);
      return;
    }
    const command = build(text);
    if (command) void toClipboard(command).then((ok) => setDone({ command, ok }));
  };
  if (!open) return <button type="button" className={small ? 'btn small' : 'btn'} title={asks ? 'Opens one line to write in; sending it asks the daemon' : 'Composes a command for the clipboard; nothing is started from this window'} onClick={() => setOpen(true)}>{label}</button>;
  const ready = asks ? request(text) !== null : build(text) !== '';
  return (
    <form className="ask open" onSubmit={send}>
      {lead}
      <label htmlFor={id} className="muted" style={{ fontSize: 12 }}>{asks && asking ? asking.prompt : prompt}</label>
      <div className="row">
        <input id={id} className="field" style={{ flex: '1 1 260px' }} type="text" value={text} maxLength={asks && asking ? asking.max : max} autoFocus onChange={(e) => { setText(e.target.value); setDone(null); }} />
        <button type="submit" className="btn" disabled={!ready || sent?.state === 'sending'}>{asks ? 'Ask' : 'Copy the command'}</button>
        <button type="button" className="btn" onClick={() => { setOpen(false); setDone(null); setSent(null); setText(''); }}>Close</button>
      </div>
      {asks ? sent && <RequestLine sent={sent} /> : done && <Copied command={done.command} ok={done.ok} />}
    </form>
  );
}
