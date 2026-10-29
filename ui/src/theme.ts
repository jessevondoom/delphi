// Look and feel: the palette, the five-column badge and the hairline, following skills/tui-design and spec 005a.
import chalk, { type ChalkInstance } from 'chalk';
import stringWidth from 'string-width';
import { graphemes } from './text.ts';

export type RoleInfo = { name: string; chassis: string; face: string; crown?: string; crown_rgb?: string };
/**
 * The look of every role, as data from the Rust avatar library (`avatar::ui_spec`, mirrored in
 * test/fixtures/avatar-spec.json). The console draws only what is in here: a new role or face added to the library
 * reaches it without a change to this file.
 */
export type AvatarRole = {
  chassis: string;
  /** The ANSI colour (0-15) for a 16-colour terminal, chosen in the library. */
  ansi?: number;
  crown: string; crown_rgb: string; mascot_face: string;
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

/** Dark or light, and the terminal's background colour when it answered the query (`null`/absent: unknown, so no tint). */
export type Pal = { dark: boolean; bg?: Rgb | null };

/**
 * What the terminal can show (`005e` §5). `NO_COLOR` (set to anything) and `TERM=dumb` mean no colour at all;
 * `TERM=dumb` also means no attributes (bold, dim, underline). Otherwise the colour depth comes from `COLORTERM`
 * and `TERM`. Read once at start (`useCapabilities`); the Rust consoles make the same decision.
 */
export type Capabilities = { color: 'none' | '16' | '256' | 'truecolor'; attributes: boolean };

export function capabilities(env: Record<string, string | undefined> = process.env, term: string | undefined = env.TERM): Capabilities {
  const dumb = term === 'dumb';
  if (dumb || env.NO_COLOR !== undefined) return { color: 'none', attributes: !dumb };
  const ct = (env.COLORTERM ?? '').toLowerCase();
  if (ct === 'truecolor' || ct === '24bit') return { color: 'truecolor', attributes: true };
  return { color: /256color/.test(term ?? '') ? '256' : '16', attributes: true };
}

let caps: Capabilities = capabilities();
export const currentCapabilities = (): Capabilities => caps;
/** Adopt `c` and set how many colours chalk may write. Called once at start-up (and by tests). */
export function useCapabilities(c: Capabilities): void {
  caps = c;
  chalk.level = !c.attributes ? 0 : c.color === 'truecolor' ? 3 : c.color === '256' ? 2 : 1;
}

type Style = (s: string) => string;
const plainStyle = chalk as ChalkInstance;
/** `colour` normally; `none` when there is no colour, so structure and emphasis survive. */
const by = (colour: Style, none: Style = (s) => s): Style => (s) => (caps.color === 'none' ? none : colour)(s);

// The semantic vocabulary (`specs/053` §3, docs/console-visual-language.md). Each style answers one
// question and has three looks: `rich` (256 and true colour), `basic` (16 colours) and `none` (no colour: weight
// or a glyph carries the meaning). Delphi draws with the terminal's own foreground and background, the ANSI colours
// and attributes (`005e` §4.1), so every theme the person has chosen is the palette; role colours are the one exception.
type Look = { rich: Style; basic?: Style; none?: Style };
const look = (l: Look): Style => (s) => (caps.color === 'none' ? l.none ?? ((x: string) => x) : caps.color === '16' ? l.basic ?? l.rich : l.rich)(s);

export const body: Style = (s) => s;
export const muted: Style = (s) => chalk.dim(s);
/** The operator's own words step back. */
export const operator: Style = muted;
/** Working, in progress; also the pointer and the keys: the one calm colour. */
export const accent: Style = look({ rich: (s) => chalk.cyan(s) });
/** A link or pointer: underlined, so it reads without colour. */
export const link: Style = look({ rich: (s) => chalk.cyan.underline(s), none: (s) => chalk.underline(s) });
/** Done, validated. */
export const ok: Style = look({ rich: (s) => chalk.green(s) });
/** Contested, caution. */
export const warn: Style = look({ rich: (s) => chalk.yellow(s), none: (s) => chalk.bold(s) });
/** Needs you, disproved, failed: the loudest style, reserved for what is waiting. */
export const alert: Style = look({ rich: (s) => chalk.red.bold(s), none: (s) => chalk.bold(s) });
/** Approval mode `auto`: the warning-red colour, bold, never reverse video. */
export const auto: Style = look({ rich: (s) => chalk.bold.hex('#f97316')(s), basic: (s) => chalk.bold.redBright(s), none: (s) => chalk.bold(s) });
export const heading: Style = look({ rich: (s) => chalk.bold.yellow(s), none: (s) => chalk.bold(s) });
/** Retired, superseded: dim, struck through where the terminal supports it. */
export const retired: Style = (s) => chalk.dim.strikethrough(s);
/** The kinds of archive entry that have a colour of their own (`043`). */
export const lesson: Style = look({ rich: (s) => chalk.magenta(s) });
export const mission: Style = look({ rich: (s) => chalk.blue(s) });
const bold: Style = (s) => chalk.bold(s);

export type Meaning = 'role' | 'needs-you' | 'working' | 'done' | 'noted' | 'contested' | 'disproved' | 'retired' | 'link' | 'mode';
/** The table of spec 053 §3: what each meaning looks like with colour and without. `glyph` is `null` where there is none. */
export const VOCABULARY: Record<Meaning, { style: Style; glyph: string | null; plain: string | null }> = {
  'role': { style: bold, glyph: '●', plain: '●' },
  'needs-you': { style: alert, glyph: '▲', plain: '!' },
  'working': { style: accent, glyph: '◐', plain: '…' },
  'done': { style: ok, glyph: '●', plain: '✔' },
  'noted': { style: muted, glyph: '○', plain: '○' },
  'contested': { style: warn, glyph: '◆', plain: '◆' },
  'disproved': { style: alert, glyph: '✕', plain: '✕' },
  'retired': { style: retired, glyph: '·', plain: '·' },
  'link': { style: link, glyph: null, plain: null },
  'mode': { style: auto, glyph: null, plain: null },
};
/** The glyph of a meaning in its style, or its no-colour form (`!`, `…`, `✔`) under no colour. */
export const mark = (m: Meaning): string => {
  const v = VOCABULARY[m];
  const g = (caps.color === 'none' ? v.plain : v.glyph) ?? '';
  return g ? v.style(g) : '';
};

/** Code and diffs in replies: colour only, never text. */
export const syntax = {
  kw: by((s) => chalk.magenta(s)), str: by((s) => chalk.green(s)), num: by((s) => chalk.yellow(s)), com: muted, ty: by((s) => chalk.cyan(s)),
  add: ok, del: by((s) => chalk.red(s)), hunk: by((s) => chalk.cyan(s)),
};

/**
 * The chip of a finished span (`005e` §8.2): green, its text in the terminal's own background colour (reverse video of
 * the green), padded by a space each side. Without colour it is `[✔ finished]`, in bold.
 */
export const chipOk = (label: string): string => (caps.color === 'none' ? chalk.bold(`[${label}]`) : chalk.green.inverse.bold(` ${label} `));
/** The same chip for a span that stopped and needs the operator: the alert colour. */
export const chipStopped = (label: string): string => (caps.color === 'none' ? chalk.bold(`[${label}]`) : chalk.red.inverse.bold(` ${label} `));

// The older names the screens use; same functions.
export const text = (_p: Pal): Style => body;
export const dim: Style = muted;
export const crimson: Style = alert;
export const rule = (_p: Pal): Style => muted;
export const hairline = (p: Pal, w: number) => rule(p)('─'.repeat(w));

/** A colour from data (a role's chassis, a crown): as itself where there is colour, plain where there is none. */
export const tone = (hex: string): ChalkInstance => (caps.color === 'none' ? plainStyle : chalk.hex(hex));
/** Colours drawn by the mascot that are not a role's own: the crown's default and highlight. */
export const CROWN_DEFAULT = '#facc15';
export const CROWN_LIGHT = '#ffffff';
/** The sprite of a role the library has no look for (a generic sub-agent): slate. */
export const GENERIC_CHASSIS = '#94a3b8';
export const GENERIC_CROWN = '#cbd5e1';
/**
 * One cell of a mascot's crown. Painted as a background with the terminal's own colour in its upper half (a `▀`), so
 * neighbouring cells have no seam and the row meets the face with no gap however the terminal spaces its lines; the
 * glyph `▄` is the fallback when the terminal's background is not known.
 */
export const crownCell = (hex: string, bg: Rgb | null | undefined): string => (caps.color !== 'none' && bg ? chalk.bgHex(hex).rgb(...bg)('▀') : tone(hex)('▄'));
/** A pixel: red, green, blue, alpha (0 to 255). */
export type Px = readonly [number, number, number, number];
/**
 * One cell of a picture (`specs/029` §9): two pixels, the upper as the foreground of a `▀` and the lower as its
 * background. A pixel at least half transparent is left to the terminal's own background. Only called where there
 * is colour: a picture is not drawn without it (`pictureOk`).
 */
export function pixelCell(top: Px, bottom: Px): string {
  const [t, b] = [top[3] >= 128, bottom[3] >= 128];
  if (t && b) return chalk.rgb(top[0], top[1], top[2]).bgRgb(bottom[0], bottom[1], bottom[2])('▀');
  if (t) return chalk.rgb(top[0], top[1], top[2])('▀');
  return b ? chalk.rgb(bottom[0], bottom[1], bottom[2])('▄') : ' ';
}
/**
 * One cell of a quadrant picture (`specs/029` §11): `glyph` in `fg` over `bg`, either of which may be left to the
 * terminal's own colour (null). A cell of one colour is a space on that background, so it has no glyph to leave a seam.
 */
export function paintedCell(glyph: string, fg: readonly [number, number, number] | null, bg: readonly [number, number, number] | null): string {
  let s = chalk;
  if (fg) s = s.rgb(fg[0], fg[1], fg[2]);
  if (bg) s = s.bgRgb(bg[0], bg[1], bg[2]);
  return fg || bg ? s(glyph) : glyph;
}
/** Whether the terminal can show a picture: it needs colour, and `NO_COLOR` and a dumb terminal have none. */
export const pictureOk = (): boolean => caps.color !== 'none';
/** A mascot's chin: seven cells of a darker shade of the body, painted, not drawn with `▀`. Glyphs where there is no colour. */
export const chin = (chassis: string): string => {
  if (caps.color === 'none') return '▀▀▀▀▀▀▀';
  const dark = '#' + [1, 3, 5].map((i) => Math.round(parseInt(chassis.slice(i, i + 2), 16) * 0.6).toString(16).padStart(2, '0')).join('');
  return chalk.bgHex(dark)(' '.repeat(7));
};
/** A mascot's face row: dark ink on the role-coloured body. */
export const onChassis = (chassis: string, row: string): string => (caps.color === 'none' ? row : chalk.bgHex(chassis).hex('#0f172a')(row));

export type Rgb = [number, number, number];
/** A background for a block, a half-step away from the terminal's own (`005e` §5, §7.2). */
export type Tint = { /** background styler */ fill: Style; /** foreground in the same colour, for the half-block caps */ ink: Style; /** the bottom cap: the lower half in the terminal's colour on the tint, so it does not lean on the `▀` glyph */ flip: Style };

const ANSI16: Rgb[] = [[0, 0, 0], [205, 0, 0], [0, 205, 0], [205, 205, 0], [0, 0, 238], [205, 0, 205], [0, 205, 205], [229, 229, 229], [127, 127, 127], [255, 0, 0], [0, 255, 0], [255, 255, 0], [92, 92, 255], [255, 0, 255], [0, 255, 255], [255, 255, 255]];
const dist = (a: Rgb, b: Rgb) => (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2;
const luma = (c: Rgb) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];

/**
 * The tint for a block on terminal background `bg`: truecolor, the background moved 8 % toward white (dark) or 6 %
 * toward black (light); 256 colours, the grey-ramp entry (232-255) two steps from it; 16 colours, ANSI 8 (dark) or 7
 * (light) when that differs from the background. `null` means no tint: the rail fallback. An unknown background
 * (`bg` null) is `null` at every depth, which is the safe answer to open question 3 of `005e` §16.
 */
/**
 * The attention band of the narrator (`005e` §9.2): the row in the warning colour over a warning-tinted background
 * (18 % of the warning colour over the terminal background). Without true colour or a known background it is bold
 * reverse video, which also serves `NO_COLOR`.
 */
export function band(row: string, bg: Rgb | null | undefined): string {
  if (caps.color !== 'truecolor' || !bg) return chalk.bold(chalk.inverse(row));
  const warnRgb: Rgb = [234, 179, 8];
  const mixed = warnRgb.map((v, i) => Math.round(v * 0.18 + bg[i]! * 0.82)) as Rgb;
  return chalk.bgRgb(...mixed).rgb(...warnRgb)(row);
}

/** `boost` > 1 is a brighter tint of the same kind: the council view lifts the seat that is speaking (`005e` §9.2). */
export function tint(bg: Rgb | null, isDark: boolean, color: Capabilities['color'] = caps.color, boost = 1): Tint | null {
  if (!bg || color === 'none') return null;
  if (color === 'truecolor') {
    const k = (isDark ? 0.08 : -0.06) * boost;
    const c = bg.map((v) => Math.round(k > 0 ? v + (255 - v) * k : v * (1 + k))) as Rgb;
    return { fill: (s) => chalk.bgRgb(...c)(s), ink: (s) => chalk.rgb(...c)(s), flip: (s) => chalk.bgRgb(...c).rgb(...bg)(s) };
  }
  if (color === '256') {
    const step = Math.max(0, Math.min(23, Math.round((luma(bg) - 8) / 10)));
    const idx = 232 + Math.max(0, Math.min(23, step + Math.round((isDark ? 2 : -2) * boost)));
    return { fill: (s) => chalk.bgAnsi256(idx)(s), ink: (s) => chalk.ansi256(idx)(s), flip: (s) => chalk.bgAnsi256(idx).rgb(...bg)(s) };
  }
  const near = ANSI16.reduce((best, c, i) => (dist(c, bg) < dist(ANSI16[best]!, bg) ? i : best), 0);
  const idx = isDark ? 8 : 7;
  if (near === idx) return null;
  return isDark ? { fill: (s) => chalk.bgBlackBright(s), ink: (s) => chalk.blackBright(s), flip: (s) => chalk.bgBlackBright.rgb(...bg)(s) } : { fill: (s) => chalk.bgWhite(s), ink: (s) => chalk.white(s), flip: (s) => chalk.bgWhite.rgb(...bg)(s) };
}

/** The state a badge shows: what an agent is doing, in the words the Observer uses. */
export type AgentState = 'idle' | 'running' | 'waiting on you' | 'escalated';

/** The library's name for what each console state looks like. */
export const ACTIVITY: Record<AgentState, string> = { idle: 'idle', running: 'working', 'waiting on you': 'waiting_approval', escalated: 'escalated' };

/** The three cells and the mouth's index: from the library's data, or the built-in faces for an older `delphi`. */
function badgeCells(role: string, state: AgentState, info: Info, frame?: string): { cells: string[]; mouth: number | null } {
  const spec = info.avatar?.roles[role]?.badge[frame ?? ACTIVITY[state]];
  if (spec) return { cells: [...spec.cells], mouth: spec.mouth };
  // Older `delphi`: the animation frames of `005a` §4, built in.
  const scout = role === 'scout';
  const animated: Record<string, string> = scout
    ? { blink: '---', thinking_left: '…  ', thinking_right: '  …', success: '^^^' }
    : { blink: '-_-', thinking_left: '•• ', thinking_right: ' ••', success: '^─^' };
  if (frame && animated[frame]) return { cells: [...animated[frame]!], mouth: scout || frame.startsWith('thinking') ? null : 1 };
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
 * `frame` is the library's activity name when the badge animates (`motion.ts`); without it, the state's own frame.
 * `crossed` swaps the eyes for dim `x`s (the "press Ctrl+C again" window).
 */
export function badge(role: string, state: AgentState, info: Info, p: Pal, frame?: string, crossed = false): string {
  const chassis = roleColor(role, info);
  // The library's eye colours need true colour; below it the eyes are the terminal's own foreground, bold (`005e` §12).
  const eye = caps.color === 'truecolor' ? chalk.hex(eyeColor(role, state, info, p))
    : state === 'waiting on you' && info.avatar?.roles[role]?.alert_eye && caps.color !== 'none' ? chalk.bold.yellow : chalk.bold;
  const { cells, mouth } = badgeCells(role, state, info, frame);
  // `crossed` (Ctrl+C pressed once): every eye becomes a lightly drawn `x`; the mouth and any blank cell stay.
  return chassis('⟦') + cells.map((c, i) => (i === mouth ? chassis(c) : crossed && c !== ' ' ? chalk.dim(eye('x')) : eye(c))).join('') + chassis('⟧');
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
export const roleColor = (role: string, info: Info): ChalkInstance => {
  const ansi = info.avatar?.roles[role]?.ansi;
  if (caps.color === '16' && ansi !== undefined) return chalk.ansi256(ansi); // the library's own choice, not the nearest guess
  return tone(info.avatar?.roles[role]?.chassis ?? info.roles[role]?.chassis ?? '#94a3b8');
};
/** A role's colour (`005e` §5): its chassis from the theme under colour; plain, with the glyphs carrying it, under none. */
export const role = roleColor;

export const width = (s: string) => stringWidth(s);

/** Left and right text on one line of `w` columns, the gap filled with spaces. The left side gives way first. */
export function spread(left: string, right: string, w: number): string {
  if (width(right) >= w) return clip(right, w);
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
      if (used + cw > w - 1) return out + (out.includes('\x1b[') ? '\x1b[0m' : '') + '…';
      out += g; used += cw;
    }
  }
  return out + (out.includes('\x1b[') ? '\x1b[0m' : '') + '…';
}

export function tilde(path: string): string {
  const home = process.env.HOME;
  return home && path.startsWith(home) ? '~' + path.slice(home.length) : path;
}

/** Strip anything a terminal would act on from text that came from a model. */
export function sanitize(s: string): string {
  return s.replace(/[\u202a-\u202e\u2066-\u2069]/g, '').replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g, '').replace(/\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]/g, '').replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '');
}
