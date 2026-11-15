import { useEffect, useState, type CSSProperties } from 'react';
import { BLINK_FRAME_MS, PAN_MS, blinkFrames, blinks, colorOf, crownOf, faceOf, fillOf, mascotRow, motionOf, nameOf, nextBlinkDelay, spec } from './avatar.ts';
import type { Info } from './types.ts';

type Look = { info: Info | null; role: string; light: boolean; activity?: string; face?: string; size?: number };

const still = (): boolean => typeof window !== 'undefined' && !!window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

/**
 * Which frame of its expression a face is on. While idle or working it blinks: each face starts a blink at a time of its own,
 * 9 to 33 seconds after the last, so no two blink together. While thinking its eyes pan. Under the system's reduce-motion
 * setting it holds still. Returns the blink frame being shown (or -1) and which way a thinking face looks.
 */
function useFrame(role: string, activity: string, frames: number): { blink: number; pan: boolean } {
  const [blink, setBlink] = useState(-1);
  const [pan, setPan] = useState(false);
  useEffect(() => {
    setBlink(-1);
    setPan(false);
    if (still()) return;
    if (motionOf(activity) === 'think') {
      const t = setInterval(() => setPan((p) => !p), PAN_MS);
      return () => clearInterval(t);
    }
    if (!blinks(activity) || frames === 0) return;
    let timer: ReturnType<typeof setTimeout>;
    let gone = false;
    const play = (i: number) => {
      if (gone) return;
      if (i >= frames) { setBlink(-1); timer = setTimeout(() => play(0), nextBlinkDelay()); return; }
      setBlink(i);
      timer = setTimeout(() => play(i + 1), BLINK_FRAME_MS);
    };
    timer = setTimeout(() => play(0), nextBlinkDelay());
    return () => { gone = true; clearTimeout(timer); };
  }, [role, activity, frames]);
  return { blink, pan };
}

/**
 * A role's chip (spec 111 §4): its three face cells on a plate of its colour, in the expression the avatar table gives for
 * what the agent is doing, blinking in the table's own frames. The face is dark on the plate in both themes.
 */
export function Plate({ info, role, light, activity = 'idle', face, size = 12 }: Look) {
  const frames = blinkFrames(info, role, face);
  const { blink, pan } = useFrame(role, activity, frames.length);
  const shown = blink >= 0 && frames[blink] ? frames[blink]! : faceOf(info, role, pan ? 'thinking_right' : activity, face).cells;
  const cls = ['bdg', motionOf(activity) === 'wait' ? 'wait' : ''].filter(Boolean).join(' ');
  return (
    <span className={cls} style={{ color: fillOf(info, role, light), fontSize: size }} aria-hidden="true">
      {Array.from(shown).slice(0, 3).map((c, i) => <i key={i}>{c}</i>)}
    </span>
  );
}

/**
 * The large form of a role's look: its crown, its seven-cell face on the plate, and its chin, as the avatar table draws its
 * mascot. It follows the same expressions and blinks on the same rule as the chip.
 */
export function Mascot({ info, role, light, activity = 'idle', size = 22 }: Look) {
  const { blink, pan } = useFrame(role, activity, 1);
  const row = mascotRow(info, role, blink >= 0 ? 'blink' : pan ? 'thinking_right' : activity);
  const crown = crownOf(info, role);
  const fill = fillOf(info, role, light);
  return (
    <span className="mascot" style={{ fontSize: size, '--c': fill } as CSSProperties} role="img" aria-label={`${nameOf(info, role)}, ${activity.replace(/_/g, ' ')}`}>
      <span className="mascot-crown" aria-hidden="true">{crown.cells.map((on, i) => <i key={i} style={on ? { background: crown.color } : undefined} className={on ? 'on' : undefined} />)}</span>
      <span className="mascot-face" aria-hidden="true">{Array.from(row).slice(0, 7).map((c, i) => <i key={i}>{c}</i>)}</span>
      <span className="mascot-chin" aria-hidden="true" />
    </span>
  );
}

/**
 * How an agent is always written: its chip, its name in bold in its colour, and its role after it in regular grey. `label` is a
 * one-off agent's own name. When the daemon's CLI sent no name of its own for a role, the name is the role and is written once.
 */
export function Agent({ label, size, ...look }: Look & { label?: string }) {
  const { info, role, light } = look;
  const name = label ?? nameOf(info, role);
  return (
    <span className="agent-id">
      <Plate {...look} size={size} />
      <span className="who" style={{ '--c': colorOf(info, role, light) } as CSSProperties}>{name}</span>
      {name !== role && <span className="role">({role})</span>}
    </span>
  );
}

/** Whether a role is one the avatar table knows, so its mascot can be drawn. */
export const hasMascot = (info: Info | null, role: string): boolean => role in spec(info).roles;
