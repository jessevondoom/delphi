import { useState, type CSSProperties } from 'react';
import { Badge, Who } from '../Badge.tsx';
import { Rich } from '../Rich.tsx';
import { colorOf, spec } from '../avatar.ts';
import type { Envelope, Info, OState } from '../types.ts';
import { clock, roster, stream, waiting, type Entry } from '../view.ts';

type Props = { info: Info | null; state: OState | null; events: Envelope[]; light: boolean };

const FILTERS: { key: string; label: string; kinds: Entry['kind'][] | null }[] = [
  { key: 'all', label: 'Everything', kinds: null },
  { key: 'said', label: 'Replies', kinds: ['said', 'asked'] },
  { key: 'read', label: 'Tool calls', kinds: ['read'] },
  { key: 'recorded', label: 'Recorded changes', kinds: ['recorded'] },
];
const KIND_LABEL: Record<Entry['kind'], string> = { said: 'said', asked: 'asked', read: 'tool', recorded: 'recorded', waiting: 'waiting on you', answered: 'answered', council: 'council' };

const FOLD_AT = 600;

/** An agent's words as text. A long reply shows its start, with a button for the rest. */
function Said({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  if (text.length <= FOLD_AT) return <div className="said"><Rich text={text} /></div>;
  return (
    <>
      <div className="said"><Rich text={open ? text : text.slice(0, FOLD_AT).trimEnd() + '…'} /></div>
      <button type="button" className="btn small" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? 'Show less' : `Show all (${text.length.toLocaleString('en')} characters)`}</button>
    </>
  );
}

/** Screen 1 (spec 111 §5.1): what agents said, read and recorded, who is working, and what waits on the operator. */
export function Activity({ info, state, events, light }: Props) {
  const [filter, setFilter] = useState('all');
  if (!state) return <main className="main"><h1 className="title">Activity</h1><div className="empty">Waiting for the daemon's state.</div></main>;
  const kinds = FILTERS.find((f) => f.key === filter)?.kinds ?? null;
  const known = spec(info).roles;
  const entries = stream(events, state, 200, (r) => r in known).filter((e) => !kinds || kinds.includes(e.kind));
  const agents = roster(state);
  const waits = waiting(state);
  const pool = (slot?: string) => (slot ? info?.pool?.[slot] : undefined);
  const tint = (role: string) => ({ '--c': colorOf(info, role, light) }) as CSSProperties;

  return (
    <>
      <main className="main">
        <div className="head-row">
          <div className="stack" style={{ gap: 6 }}>
            <h1 className="title">Activity</h1>
            <div className="sub">{state.mission_id ? `mission ${state.mission_id}` : 'no mission open'} · {agents.length} agents</div>
          </div>
          <div className="pills" role="group" aria-label="Show">
            {FILTERS.map((f) => <button key={f.key} type="button" className="pill" aria-pressed={filter === f.key} onClick={() => setFilter(f.key)}>{f.label}</button>)}
          </div>
        </div>
        <div className="stack">
          {entries.length === 0 && <div className="empty">Nothing to show yet. Activity appears here as agents work.</div>}
          {entries.map((e, i) => {
            const one = pool(e.slot);
            return (
              <article key={e.key} className="card tint" style={tint(e.role)}>
                <div className="row">
                  <Badge info={info} role={e.role} light={light} size={15} face={one?.face} activity={e.kind === 'waiting' ? 'waiting_approval' : 'idle'} delay={(i * 1.37) % 5} />
                  <Who info={info} role={e.role} light={light} label={one?.name} />
                  <span className={e.kind === 'said' ? 'chip strong' : e.kind === 'read' ? 'chip line' : 'chip'}>{e.kind === 'asked' && e.to ? `asked ${info?.roles[e.to]?.name || e.to}` : KIND_LABEL[e.kind]}</span>
                  <span className="time push">{clock(e.at_ms)}</span>
                </div>
                {e.text && <Said text={e.text} />}
                {e.code && <div className="code">{e.code}</div>}
              </article>
            );
          })}
        </div>
      </main>
      <aside className="aside" aria-label="Agents and attention">
        {waits.length > 0 && (
          <section className="card gold">
            <div className="row">
              <Badge info={info} role={waits[0]!.role} light={light} size={16} activity="waiting_approval" />
              <strong>{waits.length === 1 ? '1 decision is waiting on you' : `${waits.length} decisions are waiting on you`}</strong>
            </div>
            {waits.map((w) => <div key={w.id} style={{ fontSize: 13 }}><Who info={info} role={w.role} light={light} /> {w.summary}</div>)}
            <div style={{ fontSize: 12, color: 'var(--text2)' }}>This app is read-only. Answer in the console:</div>
            <div className="code">delphi chat</div>
          </section>
        )}
        <section className="stack">
          <h2 className="caps">Agents</h2>
          {agents.map((a) => (
            <div key={a.role} className="agent">
              <Badge info={info} role={a.role} light={light} size={15} plate activity={a.activity} />
              <div className="grow stack" style={{ gap: 4 }}>
                <div className="name">{info?.roles[a.role]?.name || a.role}</div>
                <div className="doing">{a.doing}</div>
              </div>
              {a.family && <span className="time">{a.family}</span>}
            </div>
          ))}
        </section>
      </aside>
    </>
  );
}
