// The approval mode and what it says when it changes (`005e` §10.3, amendment 005e-A1). `Shift+Tab` cycles the four
// permission modes; entering `auto` takes one more press to confirm (any other key cancels). The mode is always in the
// footer (`auto` in the warning colour) and each change leaves one line saying what it means. Red actions ask in every
// mode. Optimistic is not a mode: it is chosen per build (`/build --optimistic`, `specs/015` §7).

export const MODES = ['default', 'accept-edits', 'shell-auto', 'mission-auto'] as const;

export const nextMode = (mode: string): string => MODES[(MODES.indexOf(mode as (typeof MODES)[number]) + 1) % MODES.length]!;

export const AUTO_CONFIRM_NOTE = '· auto runs Amber actions without asking this mission — press Shift+Tab again to confirm, any other key cancels';

/** One Shift+Tab: the mode to set now (null while waiting for the confirming press) and whether a confirm is pending. */
export function cycleMode(mode: string, armed: boolean): { set: string | null; armed: boolean } {
  const next = nextMode(mode);
  if (next !== 'mission-auto') return { set: next, armed: false };
  return armed ? { set: next, armed: false } : { set: null, armed: true };
}

export function modeNote(mode: string): string {
  switch (mode) {
    case 'mission-auto': return '· approval mode: auto — Amber actions run without asking this mission; Red actions always ask';
    case 'shell-auto': return '· approval mode: shell auto — safe shell commands and file edits run without asking; other commands and Red actions still ask';
    case 'accept-edits': return '· approval mode: accept edits — file edits run without asking; commands and Red actions still ask';
    default: return '· approval mode: default';
  }
}
