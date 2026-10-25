// The slash menu: names, one-line descriptions and the fuzzy filter. What each command does is wired in the app.
export type Command = { name: string; desc: string; local: boolean };

// Alphabetical: the slash menu lists them in this order until you type.
export const COMMANDS: Command[] = [
  { name: '/help', desc: 'Show keys and commands', local: true },
  { name: '/clear', desc: 'Clear the screen (the history stays in the mission)', local: true },
  { name: '/export', desc: 'Save this conversation as a markdown document in the mission', local: true },
  { name: '/search', desc: 'Search the knowledge base (also ctrl+k); enter puts @path in the prompt', local: true },
  { name: '/sources', desc: 'Where the answers came from: files read and documents cited (also ctrl+s)', local: true },
  { name: '/pin', desc: 'Keep the last reply, or the text you give, in the mission notebook', local: true },
  { name: '/notes', desc: 'Read the mission notebook', local: true },
  { name: '/connectors', desc: 'Which connectors are configured, logged in and pinned', local: true },
  { name: '/mouse', desc: 'Turn mouse reporting on or off (off hands text selection back to the terminal; on, drag to select and copy)', local: true },
  { name: '/quit', desc: 'Leave the console, also /exit; the daemon keeps running', local: true },
  { name: '/build', desc: 'A Builder task: worktree, tests, audit, then your approval to merge', local: false },
  { name: '/council', desc: 'Convene a blind three-round council on a proposition', local: false },
  { name: '/timer', desc: 'Wake a role later: /timer 20m check CI · /timer @architect 1h review · /timer list · /timer cancel <id>', local: false },
  { name: '/models', desc: 'The model pool: which models are usable, out of quota, or cooling down', local: false },
  { name: '/agents', desc: 'Every agent running now and what it is doing', local: false },
  { name: '/narrator', desc: 'The read-only live window: the command to run in a second terminal', local: false },
  { name: '/usage', desc: 'Tokens and spend by role, task, model or mission', local: false },
  { name: '/budget', desc: 'Show or change this mission\'s cost ceiling', local: false },
  { name: '/permissions', desc: 'Approval mode: default, accept-edits, mission-auto, optimistic', local: false },
  { name: '/decisions', desc: 'Review an optimistic build\'s provisional decisions: show, ratify, reject', local: false },
  { name: '/merge', desc: 'Run the gates and the merge card again for a kept branch, after review: /merge <task>', local: false },
  { name: '/ratify', desc: 'Review and approve pending changes', local: false },
  { name: '/resume', desc: 'Continue tasks a restart interrupted', local: false },
  { name: '/pause', desc: 'Leave interrupted tasks as they are', local: false },
  { name: '/mission', desc: 'The current mission', local: false },
  { name: '/observe', desc: 'Open the Observer in a new terminal window, every time: /observe council · epic · forge · agents · spend · archives', local: false },
  { name: '/y', desc: 'Approve the waiting card once', local: false },
  { name: '/a', desc: 'Approve similar actions this task (Amber only)', local: false },
  { name: '/n', desc: 'Reject the waiting card', local: false },
  { name: '/d', desc: 'View the waiting card\'s full payload', local: false },
].sort((a, b) => a.name.localeCompare(b.name));

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
