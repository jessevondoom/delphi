// The approval mode and what it says when it changes (`005e` §10.3). `Shift+Tab` cycles at once, with no confirmation:
// the safeguards are that the mode is always in the footer (`auto` in the warning colour) and that each change leaves
// one line saying what it means. Red actions ask in every mode.

export const MODES = ['default', 'accept-edits', 'mission-auto'] as const;

export const nextMode = (mode: string): string => MODES[(MODES.indexOf(mode as (typeof MODES)[number]) + 1) % MODES.length]!;

export function modeNote(mode: string): string {
  switch (mode) {
    case 'mission-auto': return '· approval mode: auto — Amber actions run without asking this mission; Red actions always ask';
    case 'accept-edits': return '· approval mode: accept edits — file edits run without asking; commands and Red actions still ask';
    default: return '· approval mode: default';
  }
}
