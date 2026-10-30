// Opening a web link in the system's default browser, on the operator's request (spec 029 §4). Delphi never fetches
// the page itself. Only http(s) and mailto are opened, and the address is an argument, never shell text.
import { spawn } from 'node:child_process';

const OPENERS: Record<string, string[]> = { darwin: ['open'], linux: ['xdg-open'] };

export type Run = (cmd: string, args: string[]) => void;

const detached: Run = (cmd, args) => {
  const p = spawn(cmd, args, { stdio: 'ignore', detached: true });
  p.on('error', () => {});
  p.unref();
};

/** Whether `url` is something this opens: an absolute http(s) or mailto address with nothing a shell could read in it. */
export const openable = (url: string): boolean => /^(https?:\/\/[^\s"'`<>\\]+|mailto:[^\s"'`<>\\]+)$/i.test(url) && !/[\x00-\x1f\x7f]/.test(url);

/** Open `url`; returns false (and does nothing) when it is not openable or the platform has no opener. */
export function openExternal(url: string, run: Run = detached, platform: string = process.platform): boolean {
  const opener = OPENERS[platform];
  if (!opener || !openable(url)) return false;
  run(opener[0]!, [...opener.slice(1), url]);
  return true;
}

/** Open a file the daemon has vouched for (`kb.open`) in the system's viewer or browser; the path is an argument, never shell text. */
export function openFile(file: string, run: Run = detached, platform: string = process.platform): boolean {
  const opener = OPENERS[platform];
  if (!opener || !file.startsWith('/') || /[\x00-\x1f\x7f]/.test(file)) return false;
  run(opener[0]!, [...opener.slice(1), file]);
  return true;
}
