// The daemons on this machine, as the start screen and the switcher show them (spec 130 §3, §4). Pure: what the bridge's
// `daemons` command returned in, cards (words, order, commands to copy) out. Everything a daemon's presence file said is text
// here: a workspace name or a path is shown, and a command is composed only from a name that is a plain one; nothing in a card is
// run, opened or sent anywhere by this window.
import { tilde } from './view.ts';

/** What a running daemon says about itself through `observe.state` (spec 130 §3.3). Tokens are counted; money never is. */
export type Summary = {
  running: number;
  waiting: number;
  escalated: number;
  approvals: number;
  /** When the oldest waiting approval was raised, in ms since the epoch. */
  oldest_approval_ms: number | null;
  mission: string | null;
  session: string | null;
  tokens: number;
};

export type Daemon = {
  /** The workspace hash that names the daemon's files: the only thing `attach` is given. */
  hash: string;
  /** `running`: its socket answered as the daemon that wrote the file. `stopped`: a file, and no such daemon. `older`: a socket with no file. */
  state: 'running' | 'stopped' | 'older';
  answers: boolean;
  workspace_name: string;
  workspace: string;
  home: string;
  version: string;
  pid: number;
  started: string;
  summary: Summary | null;
  /** The newest stable release the daemon found (`1.2.3`), from its presence file; spec 120 §13.2 item 12. Absent from a daemon that does not say. */
  update_available?: string | null;
  /** Set after an update was applied: the daemon still runs the old code until it is restarted. */
  restart_pending?: boolean;
};

export type DaemonList = {
  /** Nothing is attached until a daemon is chosen: the start screen is showing. */
  held: boolean;
  /** The hash of the daemon the window is attached to, or looking for. */
  current: string | null;
  /** The app's own version; `0.0.0` in a build that was not made for a release. */
  app_version: string;
  daemons: Daemon[];
  /** Whether an installed `delphi` was found, and the path of the one inside this app's bundle, when there is one (spec 122 §10.1). */
  program?: { installed: boolean; bundled: string | null };
};

/** What a card says a daemon is doing. `running` is a daemon that answered but gave no summary in time. */
export type CardState = 'needs you' | 'working' | 'idle' | 'running' | 'stopped' | 'older daemon';

/** `needs you` first, then working, idle, stopped, older daemon (spec 130 §4 items 1 and 2). */
export const CARD_ORDER: CardState[] = ['needs you', 'working', 'idle', 'running', 'stopped', 'older daemon'];

export function cardState(d: Daemon): CardState {
  if (d.state === 'older') return 'older daemon';
  if (d.state === 'stopped') return 'stopped';
  const s = d.summary;
  if (!s) return 'running';
  if (s.approvals > 0 || s.waiting > 0 || s.escalated > 0) return 'needs you';
  return s.running > 0 ? 'working' : 'idle';
}

/** The cards in the order they are shown: by state, then by workspace name, then by hash, so a refresh never shuffles equals. */
export function orderDaemons(list: Daemon[]): Daemon[] {
  const rank = (d: Daemon) => CARD_ORDER.indexOf(cardState(d));
  return [...list].sort((a, b) => rank(a) - rank(b) || a.workspace_name.localeCompare(b.workspace_name) || a.hash.localeCompare(b.hash));
}

/** A workspace name that may stand in a command: plain, and never an option. */
export const plainName = (n: string): boolean => /^[A-Za-z0-9_][A-Za-z0-9._-]{0,63}$/.test(n);

/** The command that starts a stopped workspace, to be copied and run in a terminal; `null` when its name is not a plain one. */
export function startCommand(d: Daemon): string | null {
  return plainName(d.workspace_name) ? `delphi --workspace ${d.workspace_name}` : null;
}

/** What to run when nothing is running (spec 130 §4 item 5) and how to get Delphi when there is none (spec 122 §3). */
export const START_COMMAND = 'delphi chat';
export const INSTALL_COMMAND = 'curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh';

/**
 * The command that installs the program inside the app (spec 122 §10.1 item 3): the bundled path in quotes, then `self-install`.
 * A path with a character a shell reads inside double quotes is single-quoted instead. Null when there is nothing to install
 * from, or when an installed `delphi` was found. This window never runs it.
 */
export function selfInstallCommand(program: DaemonList['program']): string | null {
  if (!program || program.installed || !program.bundled || !program.bundled.startsWith('/') || /[\u0000-\u001f]/.test(program.bundled)) return null;
  const p = program.bundled;
  const quoted = /["$`\\!]/.test(p) ? `'${p.replace(/'/g, `'\\''`)}'` : `"${p}"`;
  return `${quoted} self-install`;
}

/** `1`, `12.3k`, `1.2M`: tokens, never dollars. */
export function tokenWords(n: number): string {
  const unit = (v: number, u: string) => `${v >= 100 ? Math.round(v) : Math.round(v * 10) / 10}${u}`;
  const t = n >= 1_000_000 ? unit(n / 1_000_000, 'M') : n >= 1000 ? unit(n / 1000, 'k') : String(n);
  return `${t} ${n === 1 ? 'token' : 'tokens'}`;
}

/** How long ago, as a card says it: seconds, minutes, hours, days. */
export function agoWords(ms: number, now: number): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86400) return `${Math.floor(s / 3600)} h`;
  return `${Math.floor(s / 86400)} d`;
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** The counts a running daemon's card shows, one short line each. */
export function summaryLines(s: Summary | null, now: number): string[] {
  if (!s) return [];
  const out: string[] = [];
  if (s.approvals > 0) out.push(`${plural(s.approvals, 'approval')} waiting${s.oldest_approval_ms ? `, the oldest for ${agoWords(s.oldest_approval_ms, now)}` : ''}`);
  if (s.waiting > 0) out.push(`${plural(s.waiting, 'agent')} waiting on you`);
  if (s.escalated > 0) out.push(`${plural(s.escalated, 'agent')} escalated`);
  out.push(s.running > 0 ? `${plural(s.running, 'agent')} running` : 'no agent running');
  if (s.session) out.push(`session ${s.session}${s.mission ? ` in mission ${s.mission}` : ''}`);
  out.push(tokenWords(s.tokens));
  return out;
}

/**
 * When the daemon's version is not the app's, both are said, with what brings them together (spec 130 §4 item 4). A build of
 * the app that has no release version (`0.0.0`), or a daemon that names none, says nothing.
 */
export function versionNote(daemonVersion: string, appVersion: string): string | null {
  if (!daemonVersion || !appVersion || appVersion === '0.0.0' || daemonVersion === appVersion) return null;
  return `daemon ${daemonVersion}, app ${appVersion}. \`delphi update\` restarts idle daemons.`;
}

export type Card = {
  hash: string;
  state: CardState;
  /** The workspace's name, or for an older daemon the hash that names its files. */
  title: string;
  path: string;
  version: string;
  versionNote: string | null;
  lines: string[];
  /** Said on a card that waits on the operator: where it is answered. The window never answers. */
  needsYou: string | null;
  /** For a stopped workspace: what to run in a terminal. Copied by a button; nothing in this window starts it. */
  command: string | null;
  /** Whether the card is a button that attaches: a daemon that answers. */
  attachable: boolean;
};

export const NEEDS_YOU_WORDS = 'Answer it in the console.';

export function cardOf(d: Daemon, now: number, appVersion: string): Card {
  const state = cardState(d);
  const older = d.state === 'older';
  return {
    hash: d.hash,
    state,
    title: older ? `daemon ${d.hash}` : d.workspace_name || d.hash,
    path: older ? '' : tilde(d.workspace),
    version: d.version,
    versionNote: d.state === 'running' || older ? versionNote(d.version, appVersion) : null,
    lines: d.state === 'running' ? summaryLines(d.summary, now) : older ? [d.answers ? 'It predates the presence files, so only its hash is known.' : 'Its socket does not answer.'] : ['Its daemon is not running; a crash may have left this record.'],
    needsYou: state === 'needs you' ? NEEDS_YOU_WORDS : null,
    command: d.state === 'stopped' ? startCommand(d) : null,
    attachable: d.answers,
  };
}

/** Cards for the daemons, in order. */
export function cards(list: DaemonList, now: number): Card[] {
  return orderDaemons(list.daemons).map((d) => cardOf(d, now, list.app_version));
}

/** How many daemons a window may be pointed at. With one, the title row's name is plain text (spec 130 §4 item 6). */
export const switchable = (list: DaemonList | null): number => (list ? list.daemons.filter((d) => d.answers).length : 0);

/**
 * Whether the window shows the start screen (spec 130 §3.2, §4): when it holds nothing until one daemon is chosen, and when no
 * daemon runs and nothing is attached (the empty state). One daemon, attached as before, never shows it.
 */
export function showStart(list: DaemonList | null, connected: boolean, hasState: boolean): boolean {
  return !!list && (list.held || (!connected && !hasState && switchable(list) === 0));
}

/** What the update banner says (spec 120 §13.2 item 12): all of it is made from a version number and a plain workspace name. */
export type UpdateBannerWords = {
  /** `Update to v1.2.3 available`, or null when the daemon names no release. */
  text: string | null;
  /** The command to copy and run in a terminal. */
  command: string;
  /** Whether this window may offer "Update now": attached to this daemon with a key that holds `update`, and nothing waiting to restart. */
  canApply: boolean;
  /** `restart when idle`, with the command that restarts this daemon; shown after an update was applied. Never a button that restarts. */
  restart: { text: string; command: string } | null;
};

/** A release the daemon names is a number and never text: three dot-separated numbers, nothing more. */
export const releaseVersion = (v: unknown): string | null => (typeof v === 'string' && /^\d{1,4}\.\d{1,4}\.\d{1,4}$/.test(v) ? v : null);

/**
 * The banner for one daemon, from its presence file's fields. `attached` is whether this window is attached to it now: only
 * then, and only with a key that holds `update`, is "Update now" offered. Every other window copies the command.
 */
export function updateBannerWords(d: Daemon, attached: boolean, grants: string[] | undefined, paired: boolean): UpdateBannerWords | null {
  const version = releaseVersion(d.update_available);
  const restart = d.restart_pending === true && plainName(d.workspace_name) ? { text: 'Updated: restart when idle', command: `delphi daemon restart --workspace ${d.workspace_name}` } : null;
  if (!version && !restart) return null;
  return { text: version && !restart ? `Update to v${version} available` : null, command: 'delphi update', canApply: !!version && !restart && attached && paired && (grants ?? []).includes('update'), restart };
}
