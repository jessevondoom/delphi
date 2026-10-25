// Terminal setup and teardown, and the one place stdin is read. Whatever happens, the terminal is given back: the
// alternate screen left, mouse and paste modes off, the cursor shown, raw mode cleared.
import { EventEmitter } from 'node:events';
import { InputParser, type Ev } from './input.ts';

const SCREEN = '\x1b[?1049h';
const PASTE = '\x1b[?2004h';
const MOUSE = '\x1b[?1000h\x1b[?1002h\x1b[?1006h'; // presses and releases, and motion while a button is held (a drag)
const KEYS = '\x1b[>4;1m'; // modifyOtherKeys, level 1: Shift+Enter becomes distinguishable where the terminal knows it
export const OFF = '\x1b[>4;0m\x1b[?2004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?1049l\x1b[?25h';
export const ON = SCREEN + PASTE + KEYS + MOUSE;

export class Term extends EventEmitter {
  private parser = new InputParser();
  private active = false;
  private mouseOn: boolean;
  private titled = false;
  private titleNow = '';

  constructor(opts: { mouse?: boolean } = {}) {
    super();
    this.mouseOn = opts.mouse ?? !process.env.DELPHI_NO_MOUSE;
  }

  get mouse() { return this.mouseOn; }

  /** Mouse reporting makes the wheel scroll the transcript and lets the console select text itself (`select.ts`); it stops the terminal's own selection, which `/mouse` switches back. */
  setMouse(on: boolean) {
    this.mouseOn = on;
    if (this.active) process.stdout.write(on ? MOUSE : '\x1b[?1006l\x1b[?1002l\x1b[?1000l');
  }

  /** Name the terminal window or tab. The title it had is saved the first time and put back by `stop()`. Off with `DELPHI_NO_TITLE`. */
  setTitle(text: string) {
    if (process.env.DELPHI_NO_TITLE || !process.stdout.isTTY || text === this.titleNow) return;
    this.titleNow = text;
    if (!this.active) return;
    if (!this.titled) { this.titled = true; process.stdout.write('\x1b[22;0t'); }
    process.stdout.write(`\x1b]2;${text}\x07`);
  }

  start() {
    if (this.active) return;
    this.active = true;
    process.stdout.write(SCREEN + PASTE + KEYS + (this.mouseOn ? MOUSE : ''));
    if (process.stdin.isTTY) process.stdin.setRawMode(true);
    process.stdin.setEncoding('utf8');
    process.stdin.resume();
    if (this.titleNow) { this.titled = true; process.stdout.write(`\x1b[22;0t\x1b]2;${this.titleNow}\x07`); } // continued after a suspend
    process.stdout.write('\x1b]11;?\x07'); // ask for the background colour: the reply picks dark or light
    if (this.wired) return;
    this.wired = true;
    process.stdin.on('data', (d: string) => { for (const e of this.parser.feed(d)) this.input(e); });
    if (process.stdin.isTTY) process.stdin.on('end', () => { this.stop(); process.exit(0); }); // the terminal went away
    process.on('exit', () => this.stop());
    // Say what went wrong only after the screen is given back: anything printed on the alternate screen is gone
    // the moment it is left.
    for (const sig of ['SIGTERM', 'SIGHUP', 'SIGINT', 'SIGQUIT'] as const) process.on(sig, () => { this.stop(); process.exit(128 + (sig === 'SIGHUP' ? 1 : sig === 'SIGINT' ? 2 : sig === 'SIGQUIT' ? 3 : 15)); });
    const die = (e: unknown) => { this.stop(); console.error(e); process.exit(1); };
    process.on('uncaughtException', die);
    process.on('unhandledRejection', die);
    // Suspended (Ctrl+Z, a shell's job control) and continued: hand the terminal back, take it again, redraw.
    process.on('SIGCONT', () => { if (!this.active) { this.start(); this.emit('resume'); } });
  }

  private wired = false;

  private input(e: Ev) {
    if (e.t === 'ctrl' && e.c === 'z') { this.suspend(); return; }
    if (e.t === 'mouse') { this.emit('mouse', e); return; } // selection (`select.ts`); the keyboard handlers never see it
    this.emit('ev', e);
  }

  /** Ctrl+Z: leave the terminal as we found it, then stop like any foreground program. */
  suspend() {
    this.stop();
    process.kill(process.pid, 'SIGSTOP');
  }

  stop() {
    if (!this.active) return;
    this.active = false;
    const pop = this.titled ? '\x1b[23;0t' : '';
    this.titled = false;
    try { process.stdout.write(pop + OFF); if (process.stdin.isTTY) process.stdin.setRawMode(false); } catch { /* the terminal is gone */ }
  }
}
