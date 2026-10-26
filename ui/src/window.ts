// Open a command in a new terminal window of the user's own terminal app: `/observe` does this on every call.
import { spawn } from 'node:child_process';

const shq = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`;
const asq = (s: string) => s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');

/** The program and arguments that open `argv` in a new window, or null when this platform has no known way. */
export function windowCommand(argv: string[], platform: string = process.platform, env: NodeJS.ProcessEnv = process.env): string[] | null {
  const line = argv.map(shq).join(' ');
  if (platform === 'darwin') {
    // The new window takes the console window's profile and size, so it looks like the console and not like the terminal's default.
    if (env.TERM_PROGRAM === 'iTerm.app') {
      return ['osascript', '-e', 'tell application "iTerm"', '-e', 'set p to profile name of current session of current window', '-e', 'activate', '-e', `create window with profile p command "${asq(line)}"`, '-e', 'end tell'];
    }
    // `do script` with no target always opens a new Terminal window; the front window is the console that asked for it.
    return [
      'osascript',
      '-e', 'tell application "Terminal"',
      '-e', 'set src to front window',
      '-e', 'set prof to current settings of selected tab of src',
      '-e', 'set cols to number of columns of src',
      '-e', 'set rws to number of rows of src',
      '-e', `set t to do script "${asq(line)}"`,
      '-e', 'set current settings of t to prof',
      '-e', 'try',
      '-e', 'set number of columns of (first window whose selected tab is t) to cols',
      '-e', 'set number of rows of (first window whose selected tab is t) to rws',
      '-e', 'end try',
      '-e', 'activate',
      '-e', 'end tell',
    ];
  }
  if (platform === 'linux') {
    const t = env.TERMINAL;
    return [t || 'x-terminal-emulator', '-e', ...argv];
  }
  return null;
}

/** Resolves to an error message, or null once the launcher has started and exited cleanly. */
export function openWindow(argv: string[], env: NodeJS.ProcessEnv = process.env, interactive: boolean = Boolean(process.stdin.isTTY)): Promise<string | null> {
  // A window is only ever opened for a person at a console who asked for one: never from a test run or a script.
  if (env.NODE_ENV === 'test' || env.NODE_TEST_CONTEXT || env.DELPHI_NO_WINDOWS || !interactive) return Promise.resolve('windows are only opened from an interactive console');
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
