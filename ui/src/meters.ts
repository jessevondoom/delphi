// Small pieces the narrator's panes share: number formatting, bars and sparklines, and the meters that watch a running
// agent (a token-cap bar, a quiet note). Pure; the panes decide where they go.
import * as T from './theme.ts';
import type { OModel } from './observe-screen.ts';

export type Paint = (s: string) => string;
export const colored = (s: string, paint: Paint) => paint(s);
export const kilo = (n: number) => (n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : String(n));
export const bar = (f: number, w: number) => { const n = Math.round(Math.min(1, Math.max(0, f)) * w); return '▓'.repeat(n) + '░'.repeat(Math.max(0, w - n)); };
export const spark = (v: number[], w: number) => {
  const r = v.slice(-w), max = Math.max(0, ...r);
  return max <= 0 ? '▁'.repeat(r.length) : r.map((x) => '▁▂▃▄▅▆▇█'[Math.round((x / max) * 7)]).join('');
};

/** Seconds one `pulse` sample covers, and how many the window keeps. */
export const SAMPLE_SECS = 5, SAMPLES = 36;
/** How long an agent may sit inside one tool call (a build, a test run) before its row says so. */
export const QUIET_TOOL_MS = 600_000;

/** How long a running agent may say nothing before its row says so, how many calls of one tool in a row are worth a flag, and how many failures in a row. */
export const QUIET_MS = 120_000, LOOP_CALLS = 6, LOOP_FAILS = 3;
/** Share of the span token cap at which the meter turns to a warning, and to an alert. */
export const CAP_WARN = 0.8, CAP_ALERT = 0.95;

/** The approval the operator has not answered for this role, if any. */
export const waitingOn = (m: OModel, role: string) => Object.values(m.st.approvals ?? {}).filter((a) => a.role === role).sort((a, b) => a.since_ms - b.since_ms)[0];

/**
 * What a silent agent is silent *in*, because the right patience depends on it. Waiting on the operator is not silence (the
 * waiting-on-you rows say it). Inside a tool call (a build, a test run) it may be quiet for ten minutes before it is flagged,
 * as `in run_command 12m`; thinking, two minutes, as `quiet 3m`. Else ''.
 */
export function quietNote(m: OModel, role: string): string {
  const now = m.now ?? 0;
  const spans = Object.values(m.st.nodes).filter((n) => n.role === role && n.state === 'running');
  if (!now || !spans.length || waitingOn(m, role)) return '';
  const idle = Math.min(...spans.map((n) => now - (m.lastSeen?.[n.id] ?? m.since ?? now)));
  const open = m.st.tools?.[role]?.open;
  if (open) return idle > QUIET_TOOL_MS ? colored(` · in ${open.tool} ${Math.floor(Math.max(0, now - open.since_ms) / 60_000)}m`, T.warn) : '';
  return idle > QUIET_MS ? colored(` · quiet ${Math.floor(idle / 60_000)}m`, T.warn) : '';
}

/** The token-cap meter for the role's busiest running span: `▓▓▓░░░░░ 34%`. Only a span that reports a running total has one. */
export function capMeter(m: OModel, role: string): string {
  const cap = m.st.token_cap ?? 0;
  if (!cap) return '';
  const used = Math.max(0, ...Object.values(m.st.nodes).filter((n) => n.role === role && n.state === 'running').map((n) => m.liveTokens?.[n.id] ?? 0));
  if (!used) return '';
  const f = used / cap;
  const paint = f >= CAP_ALERT ? T.alert : f >= CAP_WARN ? T.warn : T.muted;
  // From the pace of the last minute: when the cap would be reached if it carried on (only when that is within the half hour).
  const recent = (m.pulse?.[role] ?? []).slice(-Math.round(60 / SAMPLE_SECS));
  const perSec = recent.reduce((t, x) => t + x, 0) / Math.max(1, recent.length * SAMPLE_SECS);
  const eta = perSec > 0 && f < 1 ? (cap - used) / perSec : Infinity;
  const forecast = eta <= 30 * 60 ? colored(`  cap in ~${Math.max(1, Math.round(eta / 60))}m`, f >= CAP_WARN ? T.warn : T.muted) : '';
  return '  ' + colored(`${bar(f, 8)} ${Math.min(999, Math.round(f * 100))}%`, paint) + forecast;
}

