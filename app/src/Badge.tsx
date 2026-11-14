import type { CSSProperties } from 'react';
import { colorOf, faceOf, motionOf, nameOf } from './avatar.ts';
import type { Info } from './types.ts';

type Props = { info: Info | null; role: string; light: boolean; activity?: string; plate?: boolean; size?: number; face?: string; delay?: number };

/** The five-cell badge (spec 111 §4): brackets drawn by CSS, each cell one character, the mouth in the role's colour. */
export function Badge({ info, role, light, activity = 'idle', plate = false, size, face, delay }: Props) {
  const f = faceOf(info, role, activity, face);
  const style: CSSProperties = { color: colorOf(info, role, light), fontSize: size, animationDelay: delay === undefined ? undefined : `-${delay}s` };
  const cls = ['bdg', plate ? 'plate' : '', motionOf(activity)].filter(Boolean).join(' ');
  return (
    <span className={cls} style={style} role="img" aria-label={`${nameOf(info, role)} badge`}>
      {f.cells.map((c, i) => <i key={i} className={!plate && i !== f.mouth ? 'e' : undefined}>{c}</i>)}
    </span>
  );
}

/** A role's name in its colour, with the role in brackets when the name is something else (a theme's, a one-off agent's). */
export function Who({ info, role, light, label }: { info: Info | null; role: string; light: boolean; label?: string }) {
  const name = label ?? nameOf(info, role);
  return (
    <>
      <span className="who" style={{ '--c': colorOf(info, role, light) } as CSSProperties}>{name}</span>
      {name !== role && <span className="role">({role})</span>}
    </>
  );
}
