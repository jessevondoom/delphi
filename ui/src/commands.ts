// The slash menu: names, one-line descriptions and the fuzzy filter. What each command does is wired in the app.
export type Command = { name: string; desc: string; local: boolean };

export const COMMANDS: Command[] = [
  { name: '/help', desc: 'Show keys and commands', local: true },
  { name: '/clear', desc: 'Clear the screen (the history stays in the mission)', local: true },
  { name: '/more', desc: 'Show the rest of the last long reply', local: true },
  { name: '/quit', desc: 'Leave the console; the daemon keeps running', local: true },
  { name: '/observe', desc: 'Open the Observer in another window (delphi observe)', local: false },
  { name: '/council', desc: 'Convene a council on a proposition', local: false },
  { name: '/permissions', desc: 'Configure approval mode (default vs auto)', local: false },
  { name: '/usage', desc: 'Tokens and spend by role', local: false },
  { name: '/ratify', desc: 'Review and approve pending changes', local: false },
];

/** Subsequence match, best first: a prefix beats a word start beats a scattered match. */
export function filter(query: string, cmds: Command[] = COMMANDS): Command[] {
  const q = query.toLowerCase();
  const score = (c: Command): number | null => {
    const n = c.name.toLowerCase();
    if (n.startsWith(q)) return 0;
    if (n.includes(q)) return 1;
    let i = 0;
    for (const ch of n) if (ch === q[i]) i++;
    return i === q.length ? 2 : null;
  };
  return cmds.map((c) => [c, score(c)] as const).filter((x): x is readonly [Command, number] => x[1] !== null).sort((a, b) => a[1] - b[1]).map((x) => x[0]);
}
