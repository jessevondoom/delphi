// Terminal setup and teardown, and the one place stdin is read.
import { EventEmitter } from 'node:events';
import { InputParser, type Ev } from './input.ts';

export const ON = '\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[?2004h';
export const OFF = '\x1b[?2004l\x1b[?1006l\x1b[?1000l\x1b[?1049l\x1b[?25h';

export class Term extends EventEmitter {
  private parser = new InputParser();
  private active = false;

  start() {
    if (this.active) return;
    this.active = true;
    process.stdout.write(ON);
    if (process.stdin.isTTY) process.stdin.setRawMode(true);
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (d: string) => { for (const e of this.parser.feed(d)) this.emit('ev', e as Ev); });
    process.stdin.resume();
    process.stdout.write('\x1b]11;?\x07'); // ask for the background colour: the reply picks dark or light
    // Whatever happens, give the terminal back.
    process.on('exit', () => this.stop());
    // Say what went wrong only after the screen is given back: anything printed on the alternate screen is gone
    // the moment it is left.
    for (const sig of ['SIGTERM', 'SIGHUP', 'SIGINT', 'SIGQUIT'] as const) process.on(sig, () => { this.stop(); process.exit(128 + (sig === 'SIGHUP' ? 1 : sig === 'SIGINT' ? 2 : sig === 'SIGQUIT' ? 3 : 15)); });
    process.on('uncaughtException', (e) => { this.stop(); console.error(e); process.exit(1); });
    process.on('unhandledRejection', (e) => { this.stop(); console.error(e); process.exit(1); });
  }

  stop() {
    if (!this.active) return;
    this.active = false;
    try { process.stdout.write(OFF); if (process.stdin.isTTY) process.stdin.setRawMode(false); } catch { /* the terminal is gone */ }
  }
}
