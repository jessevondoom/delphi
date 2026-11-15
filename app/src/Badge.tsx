import type { CSSProperties } from 'react';
import { colorOf, faceOf, fillOf, motionOf, nameOf } from './avatar.ts';
import type { Info } from './types.ts';

type Look = { info: Info | null; role: string; light: boolean; activity?: string; face?: string; size?: number };

/** A role's chip (spec 111 §4): its three face cells on a plate of its colour. The face is dark on the plate in both themes. */
export function Plate({ info, role, light, activity = 'idle', face, size = 12 }: Look) {
  const f = faceOf(info, role, activity, face);
  const cls = ['bdg', motionOf(activity) === 'wait' ? 'wait' : ''].filter(Boolean).join(' ');
  return (
    <span className={cls} style={{ color: fillOf(info, role, light), fontSize: size }} aria-hidden="true">
      {f.cells.map((c, i) => <i key={i}>{c}</i>)}
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
