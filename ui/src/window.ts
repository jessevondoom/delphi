// Open a command in a new terminal window of the user's own terminal app: `/observe` does this on every call.
import { spawn } from 'node:child_process';

const shq = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`;
const asq = (s: string) => s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');

/** The program and arguments that open `argv` in a new window, or null when this platform has no known way. */
export function windowCommand(argv: string[], platform: string = process.platform, env: NodeJS.ProcessEnv = process.env): string[] | null {
  const line = argv.map(shq).join(' ');
  if (platform === 'darwin') {
    if (env.TERM_PROGRAM === 'iTerm.app') {
      return ['osascript', '-e', 'tell application "iTerm"', '-e', 'activate', '-e', `create window with default profile command "${asq(line)}"`, '-e', 'end tell'];
    }
    // `do script` with no target always opens a new Terminal window.
    return ['osascript', '-e', 'tell application "Terminal"', '-e', 'activate', '-e', `do script "${asq(line)}"`, '-e', 'end tell'];
  }
  if (platform === 'linux') {
    const t = env.TERMINAL;
    return [t || 'x-terminal-emulator', '-e', ...argv];
  }
  return null;
}

/** Resolves to an error message, or null once the launcher has started and exited cleanly. */
export function openWindow(argv: string[]): Promise<string | null> {
  const c = windowCommand(argv);
  if (!c) return Promise.resolve(`no way to open a window on ${process.platform}`);
  return new Promise((done) => {
    const p = spawn(c[0]!, c.slice(1), { stdio: ['ignore', 'ignore', 'pipe'], detached: true });
    let err = '';
    p.stderr?.on('data', (d) => { err += d; });
    p.on('error', (e) => done(e.message));
    p.on('close', (code) => done(code === 0 ? null : err.trim() || `${c[0]} exited with ${code}`));
  });
}
