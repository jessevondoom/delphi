// Look and feel: the palette, the five-column badge and the hairline, following skills/tui-design and spec 005a.
import chalk from 'chalk';
import stringWidth from 'string-width';
import { graphemes } from './text.ts';

export type RoleInfo = { name: string; chassis: string; face: string; crown?: string; crown_rgb?: string };
/**
 * The look of every role, as data from the Rust avatar library (`avatar::ui_spec`, mirrored in
 * test/fixtures/avatar-spec.json). The console draws only what is in here: a new role or face added to the library
 * reaches it without a change to this file.
 */
export type AvatarRole = {
  chassis: string; crown: string; crown_rgb: string; mascot_face: string;
  /** Per activity: the three cells between the brackets, and which of them is the mouth (the rest are eyes). */
  badge: Record<string, { cells: string; mouth: number | null }>;
  /** Per activity: the seven cells of the mascot's face row. */
  mascot: Record<string, string>;
  /** Eye colour while the role waits on the operator, by terminal background. */
  alert_eye?: { dark: string; light: string };
};
export type AvatarSpec = { version: number; eye: { dark: string; light: string }; roles: Record<string, AvatarRole> };

export type Info = {
  console_socket: string; narrator_socket: string; workspace: string; mission: string | null;
  version: string; cwd: string; roles: Record<string, RoleInfo>;
  /** Where `delphi` is and which workspace/home this is: for the few things the UI asks the CLI for. */
  delphi_exe?: string; workspace_name?: string; home?: string;
  /** Absent when an older `delphi` started the console; the built-in faces below are used then. */
  avatar?: AvatarSpec;
};

export type Pal = { dark: boolean };

export const text = (p: Pal) => (s: string) => (p.dark ? chalk.hex('#e2e8f0')(s) : chalk.hex('#0f172a')(s));
export const dim = (s: string) => chalk.hex('#64748b')(s);
export const rule = (p: Pal) => (s: string) => chalk.hex(p.dark ? '#334155' : '#cbd5e1')(s);
export const link = (s: string) => chalk.hex('#60a5fa')(s);
export const accent = (s: string) => chalk.hex('#38bdf8')(s);
export const warn = (s: string) => chalk.hex('#facc15')(s);
export const crimson = (s: string) => chalk.hex('#ef4444')(s);

export const hairline = (p: Pal, w: number) => rule(p)('─'.repeat(w));

/** The state a badge shows: what an agent is doing, in the words the Observer uses. */
export type AgentState = 'idle' | 'running' | 'waiting on you' | 'escalated';

/** The library's name for what each console state looks like. */
export const ACTIVITY: Record<AgentState, string> = { idle: 'idle', running: 'working', 'waiting on you': 'waiting_approval', escalated: 'escalated' };

/** The three cells and the mouth's index: from the library's data, or the built-in faces for an older `delphi`. */
function badgeCells(role: string, state: AgentState, info: Info): { cells: string[]; mouth: number | null } {
  const spec = info.avatar?.roles[role]?.badge[ACTIVITY[state]];
  if (spec) return { cells: [...spec.cells], mouth: spec.mouth };
  const cells = state === 'waiting on you' ? 'x?x' : state === 'escalated' ? 'x_x' : state === 'running' ? (role === 'scout' ? '···' : '•─•') : (info.roles[role]?.face ?? '•_•');
  return { cells: [...cells], mouth: role === 'scout' || state === 'waiting on you' ? null : 1 };
}

/** The eye colour for a role in a state on a dark or light terminal. */
export function eyeColor(role: string, state: AgentState, info: Info, p: Pal): string {
  const alert = state === 'waiting on you' ? info.avatar?.roles[role]?.alert_eye ?? (role === 'red-team' ? { dark: '#facc15', light: '#a16207' } : undefined) : undefined;
  if (alert) return p.dark ? alert.dark : alert.light;
  const eye = info.avatar?.eye ?? { dark: '#ffffff', light: '#0f172a' };
  return p.dark ? eye.dark : eye.light;
}

/**
 * `⟦•─•⟧`: brackets and mouth in the role's colour, eyes bright on a dark terminal and deep slate on a light one.
 * Which cell is the mouth comes from the library's data, not from the character: the red-team's `-_-` has `-` eyes.
 */
export function badge(role: string, state: AgentState, info: Info, p: Pal): string {
  const chassis = chalk.hex(info.avatar?.roles[role]?.chassis ?? info.roles[role]?.chassis ?? '#94a3b8');
  const eye = chalk.hex(eyeColor(role, state, info, p));
  const { cells, mouth } = badgeCells(role, state, info);
  return chassis('⟦') + cells.map((c, i) => (i === mouth ? chassis(c) : eye(c))).join('') + chassis('⟧');
}

/**
 * Dark or light from the environment, as the Rust console decides it (`observer::detect_dark`): `DELPHI_THEME` wins
 * outright; `COLORFGBG` (`fg;bg`, set by several terminals) is the fallback when the terminal does not answer the
 * background-colour query. `null` when neither says.
 */
export function envTheme(env: Record<string, string | undefined> = process.env): { dark: boolean; forced: boolean } | null {
  if (env.DELPHI_THEME === 'light') return { dark: false, forced: true };
  if (env.DELPHI_THEME === 'dark') return { dark: true, forced: true };
  const bg = Number((env.COLORFGBG ?? '').split(';').pop());
  if (env.COLORFGBG && Number.isInteger(bg)) return { dark: !(bg === 7 || bg >= 9), forced: false };
  return null;
}

export const roleName = (role: string, info: Info) => info.roles[role]?.name ?? role;
export const roleColor = (role: string, info: Info) => chalk.hex(info.roles[role]?.chassis ?? '#94a3b8');

export const width = (s: string) => stringWidth(s);

/** Left and right text on one line of `w` columns, the gap filled with spaces. The left side gives way first. */
export function spread(left: string, right: string, w: number): string {
  const gap = w - width(left) - width(right);
  if (gap >= 1) return left + ' '.repeat(gap) + right;
  const room = w - width(right) - 1;
  if (room < 8) return clip(left, w); // too narrow for both: the status on the left matters more than the hint on the right
  return clip(left, room) + ' ' + right;
}

/** Cut to `w` display columns, keeping styling and adding an ellipsis when something was cut. */
export function clip(s: string, w: number): string {
  if (width(s) <= w) return s;
  let out = '', used = 0;
  for (const m of s.matchAll(/\x1b\[[0-9;]*m|[^\x1b]+|\x1b/g)) {
    const tok = m[0];
    if (tok.length > 2 && tok.startsWith('\x1b[') && tok.endsWith('m')) { out += tok; continue; }
    for (const g of graphemes(tok)) { // whole characters only: never half of an emoji or a wide glyph
      const cw = stringWidth(g);
      if (used + cw > w - 1) return out + '\x1b[0m…';
      out += g; used += cw;
    }
  }
  return out + '\x1b[0m…';
}

export function tilde(path: string): string {
  const home = process.env.HOME;
  return home && path.startsWith(home) ? '~' + path.slice(home.length) : path;
}

/** Strip anything a terminal would act on from text that came from a model. */
export function sanitize(s: string): string {
  return s.replace(/[\u202a-\u202e\u2066-\u2069]/g, '').replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g, '').replace(/\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]/g, '').replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '');
}
