// The slash menu: names, one-line descriptions and the fuzzy filter. What each command does is wired in the app.
export type Command = { name: string; desc: string; local: boolean };

export const COMMANDS: Command[] = [
  { name: '/help', desc: 'Show keys and commands', local: true },
  { name: '/clear', desc: 'Clear the screen (the history stays in the mission)', local: true },
  { name: '/quit', desc: 'Leave the console; the daemon keeps running', local: true },
  { name: '/build', desc: 'A Builder task: worktree, tests, audit, then your approval to merge', local: false },
  { name: '/council', desc: 'Convene a blind three-round council on a proposition', local: false },
  { name: '/usage', desc: 'Tokens and spend by role, task, model or mission', local: false },
  { name: '/budget', desc: 'Show or change this mission\'s cost ceiling', local: false },
  { name: '/permissions', desc: 'Approval mode: default, accept-edits, mission-auto', local: false },
  { name: '/ratify', desc: 'Review and approve pending changes', local: false },
  { name: '/resume', desc: 'Continue tasks a restart interrupted', local: false },
  { name: '/pause', desc: 'Leave interrupted tasks as they are', local: false },
  { name: '/mission', desc: 'The current mission', local: false },
  { name: '/observe', desc: 'How to open the Observer in another window', local: false },
  { name: '/y', desc: 'Approve the waiting card once', local: false },
  { name: '/a', desc: 'Approve similar actions this task (Amber only)', local: false },
  { name: '/n', desc: 'Reject the waiting card', local: false },
  { name: '/d', desc: 'View the waiting card\'s full payload', local: false },
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
