// Look and feel: the palette, the five-column badge and the hairline, following skills/tui-design and spec 005a.
import chalk from 'chalk';
import stringWidth from 'string-width';

export type RoleInfo = { name: string; chassis: string; face: string; crown?: string; crown_rgb?: string };
export type Info = {
  console_socket: string; narrator_socket: string; workspace: string; mission: string | null;
  version: string; cwd: string; roles: Record<string, RoleInfo>;
  /** Where `delphi` is and which workspace/home this is: for the few things the UI asks the CLI for. */
  delphi_exe?: string; workspace_name?: string; home?: string;
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

function faceFor(role: string, state: AgentState, info: Info): string {
  if (state === 'waiting on you') return 'x?x';
  if (state === 'escalated') return 'x_x';
  if (state === 'running') return role === 'scout' ? '···' : '•─•';
  return info.roles[role]?.face ?? '•_•';
}

/** `⟦•─•⟧`: brackets and mouth in the role's colour, eyes bright (white on dark, slate on light, gold for the red-team's alert). */
export function badge(role: string, state: AgentState, info: Info, p: Pal): string {
  const chassis = chalk.hex(info.roles[role]?.chassis ?? '#94a3b8');
  const alert = role === 'red-team' && state === 'waiting on you';
  const eye = alert ? chalk.hex('#facc15') : p.dark ? chalk.hex('#ffffff') : chalk.hex('#0f172a');
  const body = [...faceFor(role, state, info)].map((c) => (c === '─' || c === '_' || c === '-' ? chassis(c) : eye(c))).join('');
  return chassis('⟦') + body + chassis('⟧');
}

export const roleName = (role: string, info: Info) => info.roles[role]?.name ?? role;
export const roleColor = (role: string, info: Info) => chalk.hex(info.roles[role]?.chassis ?? '#94a3b8');

export const width = (s: string) => stringWidth(s);

/** Left and right text on one line of `w` columns, the gap filled with spaces. The left side gives way first. */
export function spread(left: string, right: string, w: number): string {
  if (width(right) >= w) return clip(right, w);
  const gap = w - width(left) - width(right);
  if (gap >= 1) return left + ' '.repeat(gap) + right;
  if (w - width(right) - 1 < 2) return clip(right, w); // no room for both: the right side (the navigation) wins
  return clip(left, w - width(right) - 1) + ' ' + right;
}

/** Cut to `w` display columns, keeping styling and adding an ellipsis when something was cut. */
export function clip(s: string, w: number): string {
  if (width(s) <= w) return s;
  let out = '', used = 0, i = 0;
  while (i < s.length && used < w - 1) {
    const m = /^\x1b\[[0-9;]*m/.exec(s.slice(i));
    if (m) { out += m[0]; i += m[0].length; continue; }
    const cp = String.fromCodePoint(s.codePointAt(i)!);
    const cw = stringWidth(cp);
    if (used + cw > w - 1) break;
    out += cp; used += cw; i += cp.length;
  }
  return out + '\x1b[0m…';
}

export function tilde(path: string): string {
  const home = process.env.HOME;
  return home && path.startsWith(home) ? '~' + path.slice(home.length) : path;
}

/** Strip anything a terminal would act on from text that came from a model. */
export function sanitize(s: string): string {
  return s.replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g, '').replace(/\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]/g, '').replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '');
}
