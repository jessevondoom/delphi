// The `!` bang (`specs/061` §6): a console line starting with `!` is a shell command for the operator's own machine.
// It is sent as `shell.run` on the console socket, shown as a block, and never sent to a model.
import { addBlock, type State } from './fold.ts';

export const BANG_USAGE = 'usage: !<command> runs it in a shell on your machine (output is not sent to the model)';

export type ShellResult = { ok: boolean; exit_code: number | null; timed_out: boolean; output: string; output_capped: boolean; elapsed_ms: number; refused?: string };

export type BangIo = {
  cmd: { call: (method: string, params?: any) => Promise<any> };
  cwd: string;
  update: (f: (s: State) => State) => void;
  note: (text: string, tone?: 'info' | 'error' | 'ok') => void;
  now?: () => number;
};

let counter = 0;

const withShell = (s: State, ref: string, f: (b: State['blocks'][number]) => State['blocks'][number]): State => ({ ...s, blocks: s.blocks.map((b) => (b.kind === 'shell' && b.ref === ref ? f(b) : b)) });

/** Run a `!` line: the usage note when it is bare, else a running block that the result (or a refusal or an error, as a note) replaces. */
export async function runBang(line: string, io: BangIo): Promise<void> {
  const command = line.replace(/^!\s*/, '').trim();
  if (!command) { io.note(BANG_USAGE); return; }
  const ref = `shell-${++counter}`;
  io.update((s) => addBlock(s, { kind: 'shell', text: command, at: (io.now ?? Date.now)(), status: 'running', ref, detail: '' }));
  const drop = (s: State): State => ({ ...s, blocks: s.blocks.filter((b) => b.ref !== ref) });
  let r: ShellResult;
  try { r = await io.cmd.call('shell.run', { command, cwd: io.cwd }); }
  catch (e: any) { io.update(drop); io.note('could not run: ' + (e?.message ?? JSON.stringify(e)), 'error'); return; }
  if (r?.refused) { io.update(drop); io.note('refused: ' + r.refused, 'error'); return; }
  const ok = !!r?.ok && !r.timed_out;
  io.update((s) => withShell(s, ref, (b) => ({ ...b, status: ok ? 'ok' : 'error', detail: String(r?.output ?? ''), shell: { exit: r?.exit_code ?? null, timedOut: !!r?.timed_out, capped: !!r?.output_capped, ms: Number(r?.elapsed_ms ?? 0) } })));
}

export const anyShellRunning = (s: State): boolean => s.blocks.some((b) => b.kind === 'shell' && b.status === 'running');
