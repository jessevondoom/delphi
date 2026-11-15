// A role's look in the window (spec 111 §4), computed from what the daemon's CLI sent. Pure: no DOM, no table of the app's own
// beyond the built-in copy of the avatar library that stands in when the CLI could not be asked.
import builtIn from './avatar-spec.json' with { type: 'json' };
import type { AvatarSpec, Info } from './types.ts';

export const BUILT_IN = builtIn as unknown as AvatarSpec;
/** The six roles whose colours make the mark, in the order the operator chose (2026-11-14). */
export const MARK_ROLES = ['orchestrator', 'strategist', 'architect', 'scout', 'red-team', 'builder'];
const FALLBACK = { dark: '#94a3b8', light: '#475569' };

export const spec = (info: Info | null): AvatarSpec => info?.avatar ?? BUILT_IN;

/**
 * A role's two light colours (spec 111 §4). A `delphi` that predates the fill sends an older, darker light colour and no fill;
 * for a role the built-in table knows, its pair is used then, so a name and a chip are never drawn from two different sets.
 */
function lightPair(info: Info | null, role: string): { text: string; fill: string } | null {
  const sent = spec(info).roles[role];
  const r = sent?.chassis_light_fill ? sent : BUILT_IN.roles[role] ?? sent;
  if (!r) return null;
  const text = r.chassis_light ?? r.chassis;
  return { text, fill: r.chassis_light_fill ?? text };
}

/** The colour a role's name is written in. A role the library does not know (a bundle's own) gets the neutral pair. */
export function colorOf(info: Info | null, role: string, light: boolean): string {
  if (light) return lightPair(info, role)?.text ?? FALLBACK.light;
  return spec(info).roles[role]?.chassis ?? info?.roles[role]?.chassis ?? FALLBACK.dark;
}

/** The colour of a role's chip, bar or tint: its colour on dark, its brighter fill on light. */
export function fillOf(info: Info | null, role: string, light: boolean): string {
  if (light) return lightPair(info, role)?.fill ?? FALLBACK.light;
  return colorOf(info, role, false);
}

/** The name to show: what the CLI sent, else the role's id. */
export const nameOf = (info: Info | null, role: string): string => info?.roles[role]?.name || role;

/** The daemon's word for what an agent or a node is doing, as one of the avatar library's activities. */
export function activityOf(state: string): string {
  switch (state) {
    case 'waiting on you': return 'waiting_approval';
    case 'running': case 'working': return 'working';
    case 'thinking': return 'thinking_left';
    case 'escalated': case 'failed': return 'escalated';
    case 'done': return 'success';
    default: return 'idle';
  }
}

export type Face = { cells: [string, string, string]; mouth: number | null; activity: string };

/** The three cells of a role's badge for an activity, and which is the mouth. `face` overrides the cells (a one-off agent's own). */
export function faceOf(info: Info | null, role: string, activity: string, face?: string): Face {
  const r = spec(info).roles[role];
  const b = r?.badge[activity] ?? r?.badge.idle ?? { cells: '•_•', mouth: 1 };
  const src = face && activity === 'idle' ? face : b.cells;
  const c = Array.from(src);
  return { cells: [c[0] ?? ' ', c[1] ?? ' ', c[2] ?? ' '], mouth: face && activity === 'idle' ? 1 : b.mouth, activity };
}

/** The CSS class that carries an activity's motion. */
export function motionOf(activity: string): string {
  if (activity === 'waiting_approval') return 'wait';
  if (activity === 'thinking_left' || activity === 'thinking_right') return 'think';
  if (activity === 'idle' || activity === 'working') return '';
  return 'still';
}
