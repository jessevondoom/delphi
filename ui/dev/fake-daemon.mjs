// A stand-in for the daemon's console socket: same line-delimited JSON-RPC, streams markdown at model speed.
import net from 'node:net';
import fs from 'node:fs';
const path = process.argv[2];
try { fs.unlinkSync(path); } catch {}
const subs = new Set();
const doc = (n) => `# Findings ${n}\n\nHere is **bold**, *italic*, \`code\` and a [link](https://example.com).\n\n- first point\n- second point with more words to wrap around the line when the terminal is narrow enough\n\n\`\`\`rust\nfn main() { println!("hi"); }\n\`\`\`\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n` + Array.from({ length: 40 }, (_, i) => `Line ${i} of a long answer.`).join('\n\n');
net.createServer((s) => {
  let buf = '';
  s.on('data', (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const m = JSON.parse(buf.slice(0, i)); buf = buf.slice(i + 1);
      const ok = (result) => s.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }) + '\n');
      if (m.method === 'events.subscribe') { subs.add(s); ok({}); }
      else if (m.method === 'observe.snapshot') ok({ events: [{ type: 'task.created', timestamp: '2026-10-24T09:00:00Z', payload: { prompt: 'an earlier question' } }, { type: 'task.output', timestamp: '2026-10-24T09:00:05Z', payload: { text: 'An earlier **answer**.', status: 'done' } }] });
      else if (m.method === 'approval.list') ok([]);
      else if (m.method === 'session.snapshot') ok({ running_tasks: [] });
      else if (m.method === 'task.submit') {
        ok({ task_id: 't_1' });
        const send = (type, payload) => { for (const c of subs) c.write(JSON.stringify({ jsonrpc: '2.0', method: 'event', params: { type, payload } }) + '\n'); };
        send('task.created', { prompt: m.params.prompt, target: 'orchestrator' });
        send('dag.node.added', { node_id: 'n_1', role: 'orchestrator', depth: 0 });
        setTimeout(() => send('dag.node.added', { node_id: 'n_2', role: 'architect', depth: 1 }), 300);
        const text = doc(1); let k = 0;
        const t = setInterval(() => {
          const chunk = text.slice(k, k += 12);
          for (const c of subs) c.write(JSON.stringify({ jsonrpc: '2.0', method: 'delta.frame', params: { spans: { n_1: { text: chunk } } } }) + '\n');
          if (k >= text.length) {
            clearInterval(t);
            for (const c of subs) {
              c.write(JSON.stringify({ jsonrpc: '2.0', method: 'event', params: { type: 'task.output', payload: { text, status: 'done' } } }) + '\n');
            }
          }
        }, 8);
      } else ok({ token: 'x', mission_id: 'demo-mission' });
    }
  });
  s.on('close', () => subs.delete(s));
}).listen(path);
