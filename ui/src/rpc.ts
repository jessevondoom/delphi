// A minimal JSON-RPC client for the daemon's Unix socket (line-delimited JSON).
import net from 'node:net';
import { EventEmitter } from 'node:events';

export class Rpc extends EventEmitter {
  private sock: net.Socket;
  private buf = '';
  private nextId = 1;
  private pending = new Map<number, { ok: (v: any) => void; err: (e: any) => void }>();

  private constructor(sock: net.Socket) {
    super();
    this.sock = sock;
    sock.setEncoding('utf8');
    sock.on('data', (d: string) => {
      this.buf += d;
      let i;
      while ((i = this.buf.indexOf('\n')) >= 0) {
        const line = this.buf.slice(0, i);
        this.buf = this.buf.slice(i + 1);
        if (line.trim()) this.onLine(JSON.parse(line));
      }
    });
    sock.on('close', () => this.emit('closed'));
    sock.on('error', () => this.emit('closed'));
  }

  static connect(path: string): Promise<Rpc> {
    return new Promise((ok, err) => {
      const s = net.createConnection(path, () => ok(new Rpc(s)));
      s.on('error', err);
    });
  }

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
    const id = this.nextId++;
    return new Promise((ok, err) => {
      this.pending.set(id, { ok, err });
      this.sock.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    });
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

  close() { this.sock.destroy(); }
}
