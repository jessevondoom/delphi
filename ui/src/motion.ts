// Badge animation (`005a` §7 amendment A1, `005e` §10.2): which frame each role's badge shows at a given moment.
// Pure functions of the agent's state and the time, so the cadence is tested with a fake clock. One timer in the app
// asks `tickKey` every 150 ms and repaints only when it changed.
import type { Agent } from './fold.ts';
import { capabilities, type AgentState } from './theme.ts';
// The blink for a face the library has no blink for lives beside the badge code (theme.ts); the animation's home re-exports it.
export { mechanicalBlink } from './theme.ts';

/** Blinks are random per badge: one per slot, at a hashed offset, so the gap between two blinks is 7-30 s (SLOT - SPREAD .. SLOT + SPREAD). */
export const BLINK_SLOT_MS = 18_500;
export const BLINK_SPREAD_MS = 11_500;
export const BLINK_FOR_MS = 150;
export const PAN_MS = 600;
export const THINKING_WINDOW_MS = 1500;
export const SUCCESS_MS = 3000;
export const ESCALATED_MS = 10_000;
/** The timer's period: the finest change any frame has (each frame of a blink lasts one tick, `BLINK_FOR_MS`). */
export const TICK_MS = 150;

/** The six roles of the strip, in their fixed order. */
export const STRIP_ROLES = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout'];

/**
 * What a badge shows: the library's activity name, and the console state it belongs to (eye colour, fallback faces).
 * `step` is which frame of a multi-frame blink to draw (the library's `blink_steps`); absent means the first.
 */
export type Face = { frame: string; state: AgentState; step?: number };

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

/** Where in slot `slot` the role blinks: 0..BLINK_SPREAD_MS, a pure function of both so a repaint never moves it and a test can pin it. */
export function blinkOffset(role: string, slot: number): number {
  const h = [...role].reduce((n, c) => Math.imul(n ^ c.charCodeAt(0), 16777619), 2166136261 ^ slot);
  return (Math.imul(h ^ (h >>> 15), 2246822519) >>> 0) % (BLINK_SPREAD_MS + 1);
}

/**
 * Which frame of the blink to show now, or null when the role is not blinking. Each frame lasts `BLINK_FOR_MS`, one tick
 * of the timer, so a blink of `steps` frames lasts `steps` ticks; a one-frame blink is the same 150 ms as always.
 */
const blinkStep = (role: string, now: number, steps: number): number | null => {
  const slot = Math.floor(now / BLINK_SLOT_MS);
  const t = now - slot * BLINK_SLOT_MS - blinkOffset(role, slot);
  return t >= 0 && t < BLINK_FOR_MS * steps ? Math.floor(t / BLINK_FOR_MS) : null;
};

/**
 * `steps` is how many frames this role's blink has (`blinkSteps` in theme.ts; 1 unless the library says more). It is a
 * parameter, not looked up here, so this stays a pure function of the agent, the time and the library.
 */
export function faceOf(a: Agent | undefined, role: string, now: number, still: boolean, steps = 1): Face {
  const st = a?.state ?? 'idle';
  if (st === 'waiting on you') return { frame: 'waiting_approval', state: st };
  if (st === 'escalated' && !(a?.since !== undefined && now - a.since >= ESCALATED_MS)) return { frame: 'escalated', state: st };
  if (st === 'running') {
    const thinking = a?.thinkingAt !== undefined && now - a.thinkingAt < THINKING_WINDOW_MS;
    if (!thinking) return { frame: 'working', state: 'running' };
    return { frame: still || Math.floor(now / PAN_MS) % 2 === 0 ? 'thinking_left' : 'thinking_right', state: 'running' };
  }
  if (a?.successAt !== undefined && now - a.successAt < SUCCESS_MS) return { frame: 'success', state: 'idle' };
  if (!still) {
    const step = blinkStep(role, now, Math.max(1, steps));
    if (step !== null) return { frame: 'blink', state: 'idle', step };
  }
  return { frame: 'idle', state: 'idle' };
}

export const stripFaces = (agents: Record<string, Agent>, roles: string[], now: number, still: boolean, steps?: (role: string) => number): Face[] =>
  roles.map((r) => faceOf(agents[r], r, now, still, steps?.(r) ?? 1));

/**
 * Changes exactly when something on screen must be redrawn because time passed: a badge frame, or the coarse clock
 * (a second when idle, a quarter second while something runs, for the spinner, and the typing bubble's own frame). An idle window does not repaint.
 */
export function tickKey(agents: Record<string, Agent>, roles: string[], now: number, still: boolean, running: boolean, typing = '', steps?: (role: string) => number): string {
  // A later frame of a multi-frame blink is a different picture, so it is part of the key (`blink:1`); the first stays `blink`.
  const frames = stripFaces(agents, roles, now, still, steps).map((f) => (f.step ? `${f.frame}:${f.step}` : f.frame));
  return `${Math.floor(now / (running && !still ? 250 : 1000))}|${typing}|${frames.join(',')}`;
}

/** The typing bubble (the operator sent a prompt, no reply has started): it waits a random 500-850 ms, then three dots pulse in turn. */
export const TYPING_MIN_MS = 500;
export const TYPING_SPAN_MS = 350;
export const TYPING_STEP_MS = 300;

/** The wait for one prompt. Random per prompt but a pure function of its id, so a repaint never changes it and a test can pin it. */
export const typingDelay = (id: number): number => TYPING_MIN_MS + (Math.imul(id + 1, 2654435761) >>> 0) % (TYPING_SPAN_MS + 1);

/** Which frame the typing bubble shows `now` (one fixed frame once it appears when `still`): -1 while it is still waiting, then 0..3 (dot 0, 1, 2 lit, then a rest). */
export function typingFrame(since: number, id: number, now: number, still = false): number {
  const t = now - since - typingDelay(id);
  return t < 0 ? -1 : still ? 0 : Math.floor(t / TYPING_STEP_MS) % 4;
}
