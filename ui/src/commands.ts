// The slash menu: names, one-line descriptions and the fuzzy filter. What each command does is wired in the app.
//
// The commands themselves are one list, `commands.json`, read here and by the Rust consoles (`chat::help`, which
// includes the same file), so the two can no longer drift (`005e` finding 7). A command is on the console that has
// `ink` or `rust` set; `local` ones are handled in `app.tsx`, the rest are one call to the daemon in `run.ts`.
import raw from './commands.json' with { type: 'json' };

export type Command = { name: string; desc: string; local: boolean; args?: string; aliases?: string[] };
type Entry = Command & { ink: boolean; rust: boolean };

const ALL = raw as Entry[];
// Alphabetical: the slash menu lists them in this order until you type, whatever the order in `commands.json`.
export const COMMANDS: Command[] = ALL.filter((c) => c.ink).map(({ name, desc, local, args, aliases }) => ({ name, desc, local, args, aliases })).sort((a, b) => a.name.localeCompare(b.name));

/** The command a typed name means (`/exit` is `/quit`), or `undefined` when this console has no such command. */
export const lookup = (typed: string): Command | undefined => COMMANDS.find((c) => c.name === typed || c.aliases?.includes(typed));

/** Subsequence match, best first: a prefix beats a word start beats a scattered match. */
export function filter(query: string, cmds: Command[] = COMMANDS): Command[] {
  const q = query.toLowerCase();
  const rank = (n: string): number | null => {
    n = n.toLowerCase();
    if (n === q) return -1;
    if (n.startsWith(q)) return 0;
    if (n.includes(q)) return 1;
    let i = 0;
    for (const ch of n) if (ch === q[i]) i++;
    return i === q.length ? 2 : null;
  };
  // A command is found by its name or by an alias (`/narr` finds `/observe`), under its own name.
  const score = (c: Command): number | null => {
    const all = [c.name, ...(c.aliases ?? [])].map(rank).filter((x): x is number => x !== null);
    return all.length ? Math.min(...all) : null;
  };
  return cmds.map((c) => [c, score(c)] as const).filter((x): x is readonly [Command, number] => x[1] !== null).sort((a, b) => a[1] - b[1]).map((x) => x[0]);
}
