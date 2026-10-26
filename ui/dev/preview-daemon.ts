// A stand-in for the daemon's narrator socket that runs `Preview` (preview-sim.ts): `observe.state` answers with the simulated
// mission, live frames carry the builder's token total and the scout's estimate, and events make the window refetch.
// Run by `ui/dev/preview.sh`; or by hand: `node --import tsx dev/preview-daemon.ts <socket> [--calm] [--seed N]`.
import net from 'node:net';
import fs from 'node:fs';
import { Preview } from './preview-sim.ts';

export function startPreview(path: string, opts: { calm?: boolean; seed?: number } = {}): net.Server {
  try { fs.unlinkSync(path); } catch { /* none there */ }
  const sim = new Preview(Date.now(), opts);
  const subs = new Set<net.Socket>();
  let frame = 0;
  const send = (s: net.Socket, o: unknown) => { if (!s.destroyed) s.write(JSON.stringify(o) + '\n'); };
  const server = net.createServer((s) => {
    let buf = '';
    s.on('data', (d) => {
      buf += d;
      let i: number;
      while ((i = buf.indexOf('\n')) >= 0) {
        const m = JSON.parse(buf.slice(0, i)); buf = buf.slice(i + 1);
        const ok = (result: unknown) => send(s, { jsonrpc: '2.0', id: m.id, result });
        switch (m.method) {
          case 'auth.issue': ok({ token: 'preview' }); break;
          case 'events.subscribe': subs.add(s); ok({}); break;
          case 'observe.state': ok({ ...sim.state(), default_view: 'pulse', mission_id: 'preview-mission', budget: { cap_usd: 0 }, pending_proposals: 0, cache_hit_ratio: 0 }); break;
          case 'kb.get': ok({ status: 404, body: { error: 'the preview has no knowledge base' } }); break;
          default: ok({});
        }
      }
    });
    s.on('close', () => subs.delete(s));
    s.on('error', () => subs.delete(s));
  });
  const timer = setInterval(() => {
    const { frames, events } = sim.tick(Date.now());
    frame++;
    const spans = Object.fromEntries(Object.entries(frames).map(([k, v]) => [k, { text: '', thinking: '', ...v }]));
    for (const c of subs) {
      send(c, { jsonrpc: '2.0', method: 'delta.frame', params: { frame_from: frame, frame_to: frame, spans } });
      for (const e of events) send(c, { jsonrpc: '2.0', method: 'event', params: e });
    }
  }, 1000);
  server.on('close', () => clearInterval(timer));
  server.listen(path);
  return server;
}

if (process.argv[1]?.endsWith('preview-daemon.ts')) {
  const socket = process.argv[2];
  if (!socket) { console.error('usage: preview-daemon.ts <socket> [--calm] [--seed N]'); process.exit(2); }
  const seedAt = process.argv.indexOf('--seed');
  startPreview(socket, { calm: process.argv.includes('--calm'), seed: seedAt >= 0 ? Number(process.argv[seedAt + 1]) : undefined });
}
