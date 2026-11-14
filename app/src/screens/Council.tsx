import { useState, type CSSProperties } from 'react';
import { Badge, Who } from '../Badge.tsx';
import { Rich } from '../Rich.tsx';
import { colorOf } from '../avatar.ts';
import type { Envelope, Info, OState } from '../types.ts';
import { STAGES, clock, councilEntries, isBlind, isDecided, seatsOf, stageOf } from '../view.ts';

type Props = { info: Info | null; state: OState | null; events: Envelope[]; light: boolean; pick: number };

const SHOWN = 7;
const KIND = { said: 'said', read: 'tool', council: 'council' } as Record<string, string>;

/** Screen 2 (spec 111 §5.2): a council's question, stages and seats; its activity while it runs; its verdict once it is in. */
export function Council({ info, state, events, light, pick }: Props) {
  const [who, setWho] = useState('all');
  const [open, setOpen] = useState(false);
  const [more, setMore] = useState(false);
  const c = state?.councils[pick];
  if (!state || !c) {
    return (
      <main className="main">
        <h1 className="title">Councils</h1>
        <div className="empty">No council has been held in this mission. When agents convene one, its seats, its deliberation and its verdict appear here.</div>
      </main>
    );
  }
  const blind = isBlind(c);
  const decided = isDecided(c);
  const stage = stageOf(c);
  const seats = seatsOf(c);
  const all = councilEntries(events, c);
  const matching = all.filter((e) => who === 'all' || e.role === who);
  const shown = more ? matching : matching.slice(0, SHOWN);
  const tint = (role: string) => ({ '--c': colorOf(info, role, light) }) as CSSProperties;
  const showStream = !decided || open;

  return (
    <main className="main">
      <div className="stack" style={{ gap: 8 }}>
        <div className="sub">council · {c.seats.length} seats{c.homogeneous ? ' · one model family' : ''}</div>
        <h1 className="question">{c.proposition}</h1>
      </div>

      <ol className="stages" aria-label="Council stages">
        {STAGES.map((label, i) => <li key={label} className={i === stage ? 'now' : i < stage ? 'past' : undefined} aria-current={i === stage ? 'step' : undefined}>{label}</li>)}
      </ol>

      <div className="seats">
        {seats.map((s, i) => (
          <section key={s.role} className="seat" style={tint(s.role)}>
            <div className="row">
              <Badge info={info} role={s.role} light={light} size={17} activity={s.position.state === 'writing' ? 'thinking_left' : 'idle'} delay={i * 1.3} />
              <Who info={info} role={s.role} light={light} />
              {s.family && <span className="src push">{s.family}</span>}
            </div>
            {s.lens && <div style={{ fontSize: 12, color: 'var(--text2)' }}>Lens: {s.lens}</div>}
            {s.position.state === 'shown' ? (
              <div className="card tint">
                <div className="caps">Round 1 · position</div>
                <div className="said"><Rich text={s.position.text} /></div>
              </div>
            ) : (
              <div className="sealed-box">
                <div className="caps">Round 1 · sealed</div>
                <div style={{ fontSize: 13, color: 'var(--text2)', marginTop: 6 }}>{s.position.state === 'writing' ? 'Writing. Hidden from the other seats until every seat has answered.' : 'Submitted. Sealed until every seat has answered.'}</div>
              </div>
            )}
            {s.critique && (
              <div className="card">
                <div className="row">
                  <span className="caps">Round 2 · critique</span>
                  {s.critique.recommendation && <span className="chip line push">recommends {s.critique.recommendation}</span>}
                </div>
                <div className="said" style={{ fontSize: 13 }}><Rich text={s.critique.text} /></div>
                {s.critique.findings.map(([severity, title], k) => <div key={k} className="row" style={{ fontSize: 12 }}><span className={severity === 'major' || severity === 'critical' ? 'chip warn' : 'chip'}>{severity}</span><span>{title}</span></div>)}
              </div>
            )}
            {!blind && !s.critique && !decided && <div style={{ fontSize: 12, color: 'var(--muted)' }}>Critique not in yet.</div>}
          </section>
        ))}
      </div>

      {decided && (
        <section className="verdicts">
          <div className="verdict card tint" style={tint('orchestrator')}>
            <div className="row">
              <Badge info={info} role="orchestrator" light={light} size={16} />
              <strong>{c.phase === 'escalated' ? 'The seats did not settle it' : 'Verdict'}</strong>
              <span className={c.phase === 'escalated' ? 'chip warn push' : 'chip ok push'}>{c.phase === 'escalated' ? 'needs your decision' : 'verdict in'}</span>
            </div>
            {c.synthesis && <div className="said" style={{ fontSize: 15 }}><Rich text={c.synthesis} /></div>}
            {c.escalation && <div className="said">{c.escalation}</div>}
            {c.options.length > 0 && (
              <div className="stack" style={{ gap: 6 }}>
                {c.options.map(([label, backers]) => <div key={label} className="row" style={{ fontSize: 13 }}><span className="chip line">{label}</span><span className="muted">{backers.map((r) => info?.roles[r]?.name || r).join(', ')}</span></div>)}
              </div>
            )}
            {c.phase === 'escalated' && <div style={{ fontSize: 12, color: 'var(--text2)' }}>This app is read-only. Choose in the console.</div>}
            {c.path && <div className="time">{c.path}</div>}
          </div>
          <div className="dissent card">
            <div className="caps">Kept on the record</div>
            {c.unresolved.length === 0 && <div style={{ fontSize: 13, color: 'var(--text2)' }}>Nothing was left unresolved.</div>}
            {c.unresolved.map(([severity, seat, title], k) => (
              <div key={k} style={{ fontSize: 13, lineHeight: 1.55 }}>
                <Badge info={info} role={seat} light={light} size={13} activity="idle" /> <Who info={info} role={seat} light={light} /> <span className="chip">{severity}</span> {title}
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="stack" aria-label="Council activity" style={{ gap: 12 }}>
        <div className="row">
          {decided ? (
            <button type="button" className="btn" aria-expanded={open} onClick={() => setOpen(!open)}>
              <span aria-hidden="true">{open ? '▾' : '▸'}</span> Activity during this council <span className="time">{all.length} events</span>
            </button>
          ) : (
            <>
              <h2 className="caps">Live activity</h2>
              <span className="conn"><span className="dot on" />following</span>
            </>
          )}
          {showStream && (
            <div className="pills push" role="group" aria-label="Show activity from">
              <button type="button" className="pill" aria-pressed={who === 'all'} onClick={() => setWho('all')}>Everyone</button>
              {c.seats.map((s) => <button key={s.role} type="button" className="pill" aria-pressed={who === s.role} style={who === s.role ? undefined : { color: colorOf(info, s.role, light) }} onClick={() => setWho(s.role)}>{info?.roles[s.role]?.name || s.role}</button>)}
            </div>
          )}
        </div>
        {blind && <div className="empty" style={{ padding: '10px 12px', fontSize: 12 }}>Round 1 is blind. You can follow that each seat is working and what it reads; its words stay sealed until every seat has answered.</div>}
        {showStream && (
          <div className="stack" style={{ gap: 8 }}>
            {shown.length === 0 && <div className="empty">Nothing from this council has reached the window yet.</div>}
            {shown.map((e, i) => (
              <article key={e.key} className="card tint" style={{ ...tint(e.role), padding: '10px 14px', gap: 6 }}>
                <div className="row">
                  <Badge info={info} role={e.role} light={light} size={14} delay={(i * 1.37) % 5} />
                  <Who info={info} role={e.role} light={light} />
                  <span className={e.kind === 'said' ? 'chip strong' : e.kind === 'read' ? 'chip line' : 'chip'}>{KIND[e.kind] ?? e.kind}</span>
                  <span className="time push">{clock(e.at_ms)}</span>
                </div>
                {e.sealed ? <div className="sealed" style={{ fontSize: 13 }}>Answered. Sealed until every seat has answered.</div> : e.text && <div className="said"><Rich text={e.text} /></div>}
                {e.code && <div className="code">{e.code}</div>}
              </article>
            ))}
            {!more && matching.length > shown.length && <button type="button" className="btn small" onClick={() => setMore(true)}>Show {matching.length - shown.length} earlier</button>}
          </div>
        )}
      </section>
    </main>
  );
}
