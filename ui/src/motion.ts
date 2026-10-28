// Badge animation (`005a` §7 amendment A1, `005e` §10.2): which frame each role's badge shows at a given moment.
// Pure functions of the agent's state and the time, so the cadence is tested with a fake clock. One timer in the app
// asks `tickKey` every 150 ms and repaints only when it changed.
import type { Agent } from './fold.ts';
import { capabilities, type AgentState } from './theme.ts';

export const BLINK_EVERY_MS = 5000;
export const BLINK_FOR_MS = 150;
export const PAN_MS = 600;
export const THINKING_WINDOW_MS = 1500;
export const SUCCESS_MS = 3000;
export const ESCALATED_MS = 10_000;
/** The timer's period: the finest change any frame has (a blink lasts one tick). */
export const TICK_MS = 150;

/** The six roles of the strip, in their fixed order. */
export const STRIP_ROLES = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout'];

/** What a badge shows: the library's activity name, and the console state it belongs to (eye colour, fallback faces). */
export type Face = { frame: string; state: AgentState };

/**
 * No animation: `DELPHI_REDUCE_MOTION=1`, `NO_COLOR`, `TERM=dumb`, or the operator's reduce-motion setting (`asked`).
 * Every animation, now or later, takes its answer from here (AGENTS.md): faces, spinners, pulses, anything on a timer.
 */
export const stillMotion = (env: Record<string, string | undefined> = process.env, asked = false): boolean =>
  asked || env.DELPHI_REDUCE_MOTION === '1' || capabilities(env).color === 'none';

/** The "working" spinner. Standing still it is one fixed glyph: the words beside it already say the work is going on. */
const SPIN = ['◐', '◓', '◑', '◒'];
export const spinner = (now: number, still: boolean): string => (still ? '●' : SPIN[Math.floor(now / 250) % SPIN.length]!);

/** A two-state flash (a live dot, a shimmer): steady in its first state when motion is reduced. */
export const pulseOn = (now: number, still: boolean, everyMs = 600): boolean => still || Math.floor(now / everyMs) % 2 === 0;

/** Roles blink out of step, so the strip does not wink all at once. */
const phase = (role: string) => [...role].reduce((n, c) => n + c.charCodeAt(0) * 31, 0) % BLINK_EVERY_MS;

export function faceOf(a: Agent | undefined, role: string, now: number, still: boolean): Face {
  const st = a?.state ?? 'idle';
  if (st === 'waiting on you') return { frame: 'waiting_approval', state: st };
  if (st === 'escalated' && !(a?.since !== undefined && now - a.since >= ESCALATED_MS)) return { frame: 'escalated', state: st };
  if (st === 'running') {
    const thinking = a?.thinkingAt !== undefined && now - a.thinkingAt < THINKING_WINDOW_MS;
    if (!thinking) return { frame: 'working', state: 'running' };
    return { frame: still || Math.floor(now / PAN_MS) % 2 === 0 ? 'thinking_left' : 'thinking_right', state: 'running' };
  }
  if (a?.successAt !== undefined && now - a.successAt < SUCCESS_MS) return { frame: 'success', state: 'idle' };
  if (!still && (now + phase(role)) % BLINK_EVERY_MS < BLINK_FOR_MS) return { frame: 'blink', state: 'idle' };
  return { frame: 'idle', state: 'idle' };
}

export const stripFaces = (agents: Record<string, Agent>, roles: string[], now: number, still: boolean): Face[] => roles.map((r) => faceOf(agents[r], r, now, still));

/**
 * Changes exactly when something on screen must be redrawn because time passed: a badge frame, or the coarse clock
 * (a second when idle, a quarter second while something runs, for the spinner). An idle window does not repaint.
 */
export function tickKey(agents: Record<string, Agent>, roles: string[], now: number, still: boolean, running: boolean): string {
  return `${Math.floor(now / (running && !still ? 250 : 1000))}|${stripFaces(agents, roles, now, still).map((f) => f.frame).join(',')}`;
}
