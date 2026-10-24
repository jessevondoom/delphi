// A JSON-RPC client for the daemon's Unix socket (line-delimited JSON) that survives the daemon going away: calls
// in flight fail at once instead of hanging, and with `keepAlive` the connection is retried and re-attached.
import net from 'node:net';
import { EventEmitter } from 'node:events';

export class Rpc extends EventEmitter {
  private sock: net.Socket | null = null;
  private buf = '';
  private nextId = 1;
  private pending = new Map<number, { ok: (v: any) => void; err: (e: any) => void }>();
  private restore: (() => Promise<void>) | null = null;
  private stopped = false;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private usable = true;

  private constructor(private path: string) { super(); }

  static async connect(path: string): Promise<Rpc> {
    const r = new Rpc(path);
    await r.open();
    return r;
  }

  private open(): Promise<void> {
    return new Promise((ok, err) => {
      const sock = net.createConnection(this.path);
      let up = false;
      sock.setEncoding('utf8');
      sock.on('connect', () => { up = true; this.sock = sock; this.buf = ''; ok(); });
      sock.on('data', (d: string) => {
        this.buf += d;
        let i;
        while ((i = this.buf.indexOf('\n')) >= 0) {
          const line = this.buf.slice(0, i);
          this.buf = this.buf.slice(i + 1);
          if (!line.trim()) continue;
          try { this.onLine(JSON.parse(line)); } catch { /* a malformed line is not worth dying for */ }
        }
      });
      const gone = () => {
        if (!up) { err(new Error('cannot connect to the daemon')); return; }
        if (this.sock === sock) this.lost();
      };
      sock.on('close', gone);
      sock.on('error', gone);
    });
  }

  private lost() {
    this.sock = null;
    this.usable = false;
    for (const p of this.pending.values()) p.err({ code: -32000, message: 'the daemon connection closed' });
    this.pending.clear();
    this.emit('closed');
    if (this.restore && !this.stopped) this.schedule(500);
  }

  private schedule(ms: number) {
    this.retry = setTimeout(async () => {
      this.retry = null;
      if (this.stopped) return;
      try {
        await this.open();
        await this.restore!();
        this.usable = true;
        this.emit('reconnected');
      } catch {
        try { this.sock?.destroy(); } catch { /* already gone */ }
        this.sock = null;
        this.schedule(Math.min(ms * 2, 5000));
      }
    }, ms);
  }

  /** From now on, when the connection drops, retry with a growing pause and run `restore` (attach again, subscribe again). */
  keepAlive(restore: () => Promise<void>) { this.restore = restore; }

  private onLine(m: any) {
    if (m.id !== undefined && m.id !== null && this.pending.has(m.id)) {
      const p = this.pending.get(m.id)!;
      this.pending.delete(m.id);
      m.error ? p.err(m.error) : p.ok(m.result);
    } else if (m.method) {
      this.emit('notification', m);
    }
  }

  call(method: string, params: any = {}): Promise<any> {
    if (!this.sock) return Promise.reject({ code: -32000, message: 'not connected to the daemon (reconnecting…)' });
    const id = this.nextId++;
    return new Promise((ok, err) => {
      this.pending.set(id, { ok, err });
      this.sock!.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n', (e) => { if (e) { this.pending.delete(id); err({ code: -32000, message: 'the daemon connection closed' }); } });
    });
  }

  get connected() { return this.sock !== null; }

  /** Resolves when the connection is attached and usable (immediately if it already is). */
  whenReady(): Promise<void> {
    return this.usable && this.sock ? Promise.resolve() : new Promise((ok) => this.once('reconnected', () => ok()));
  }

  /** Attach as the read-only Narrator (the daemon refuses anything that would change state). */
  async attachAsNarrator() {
    const t = await this.call('auth.issue', { scope: 'narrator' });
    return this.call('hello', { protocol_version: 1, binary_version: 'delphi-ui', token: t.token });
  }

  /** Attach as a console: ask for a token (the daemon requires a terminal), then say hello. */
  async attachAsConsole() {
    const t = await this.call('auth.issue', { scope: 'console' });
    return this.call('hello', { protocol_version: 1, binary_version: 'ink-spike', token: t.token });
  }

  close() {
    this.stopped = true;
    if (this.retry) clearTimeout(this.retry);
    this.sock?.destroy();
  }
}
