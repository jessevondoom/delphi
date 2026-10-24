import React, { useEffect, useMemo, useRef, useState } from 'react';
import { render, Box, Text, useInput, useApp, useStdout } from 'ink';
import { execFileSync } from 'node:child_process';
import { Rpc } from './rpc.js';
import { renderMarkdown } from './markdown.js';

type Block = { kind: 'user' | 'agent' | 'sys'; who?: string; text: string };
type Seat = { role: string; family?: string; stance?: string; critique?: string };

const delphi = process.env.DELPHI_BIN ?? 'delphi';
const where = JSON.parse(execFileSync(delphi, ['daemon', 'where'], { encoding: 'utf8' }));

function App({ cmd, ev, mission }: { cmd: Rpc; ev: Rpc; mission: string }) {
  const { exit } = useApp();
  const stdout = useStdout().stdout as unknown as NodeJS.WriteStream;
  const [size, setSize] = useState({ w: stdout.columns, h: stdout.rows });
  const [blocks, setBlocks] = useState<Block[]>([{ kind: 'sys', text: `mission ${mission} · type a message · Alt+Enter newline · PgUp/PgDn or wheel scroll · Ctrl+T council · Ctrl+C quit` }]);
  const [live, setLive] = useState('');
  const [text, setText] = useState('');
  const [back, setBack] = useState(0); // lines scrolled up from the tail
  const [view, setView] = useState<'chat' | 'council'>('chat');
  const [seats, setSeats] = useState<Seat[]>([]);
  const [tokens, setTokens] = useState(0);
  const frames = useRef(0);

  useEffect(() => {
    const onResize = () => setSize({ w: stdout.columns, h: stdout.rows });
    stdout.on('resize', onResize);
    return () => { stdout.off('resize', onResize); };
  }, [stdout]);

  useEffect(() => {
    const onNote = (n: any) => {
      if (n.method === 'delta.frame') {
        frames.current++;
        const spans = n.params?.spans ?? {};
        const t = Object.keys(spans).sort().map((k) => spans[k].text ?? '').join('');
        if (t) { setLive((l) => (l + t).slice(-6000)); setTokens((x) => x + Math.ceil(t.length / 4)); }
      } else if (n.method === 'event') {
        const e = n.params, p = e.payload ?? {};
        if (e.type === 'task.output') {
          setLive('');
          setBlocks((b) => [...b, { kind: 'agent', who: 'agent', text: String(p.text ?? '') }]);
        } else if (e.type === 'council.started') {
          setSeats((p.seats ?? []).map((s: any) => ({ role: s.role, family: s.family })));
        } else if (e.type === 'council.stance') {
          setSeats((s) => s.map((x) => (x.role === p.role ? { ...x, stance: String(p.text ?? '').split('\n')[0] } : x)));
        } else if (e.type === 'council.critique') {
          setSeats((s) => s.map((x) => (x.role === p.role ? { ...x, critique: String(p.recommendation ?? '') } : x)));
        }
      }
    };
    ev.on('notification', onNote);
    return () => { ev.off('notification', onNote); };
  }, [ev]);

  const chatH = Math.max(3, size.h - 6 - Math.max(0, text.split('\n').length - 1));
  const lines = useMemo(() => {
    const out: string[] = [];
    for (const b of blocks) {
      if (b.kind === 'user') out.push('\x1b[1m› ' + b.text.split('\n').join('\n  ') + '\x1b[22m', '');
      else if (b.kind === 'sys') out.push('\x1b[2m' + b.text + '\x1b[22m', '');
      else out.push(...renderMarkdown(b.text, size.w - 2));
    }
    if (live) out.push(...renderMarkdown(live, size.w - 2).map((l) => '\x1b[2m' + l + '\x1b[22m'));
    return out;
  }, [blocks, live, size.w]);
  const maxBack = Math.max(0, lines.length - chatH);
  const off = Math.min(back, maxBack);
  const shown = lines.slice(Math.max(0, lines.length - chatH - off), lines.length - off);

  useEffect(() => {
    // Wheel scrolling: terminals report it as SGR mouse sequences when mouse mode is on.
    process.stdout.write('\x1b[?1000h\x1b[?1006h');
    return () => { process.stdout.write('\x1b[?1000l\x1b[?1006l'); };
  }, []);

  useInput((input, key) => {
    const wheel = /\[<(\d+);\d+;\d+[Mm]/.exec(input);
    if (wheel) {
      const b = Number(wheel[1]);
      if (b === 64) setBack((x) => Math.min(maxBack, x + 3));
      if (b === 65) setBack((x) => Math.max(0, x - 3));
      return;
    }
    if (key.ctrl && input === 'c') { exit(); return; }
    if (key.ctrl && input === 't') { setView((v) => (v === 'chat' ? 'council' : 'chat')); return; }
    if (key.pageUp) { setBack((x) => Math.min(maxBack, x + chatH - 1)); return; }
    if (key.pageDown) { setBack((x) => Math.max(0, x - chatH + 1)); return; }
    if (key.return && (key.meta || key.shift)) { setText((t) => t + '\n'); return; }
    if (key.return) {
      const t = text.trim();
      if (!t) return;
      setText(''); setBack(0);
      setBlocks((b) => [...b, { kind: 'user', text: t }]);
      cmd.call('task.submit', { mission_id: mission, prompt: t }).catch((e) =>
        setBlocks((b) => [...b, { kind: 'sys', text: 'error: ' + JSON.stringify(e) }]));
      return;
    }
    if (key.backspace || key.delete) { setText((t) => t.slice(0, -1)); return; }
    if (key.ctrl && input === 'u') { setText(''); return; }
    if (key.escape) { setText(''); return; }
    if (!key.ctrl && !key.meta && !key.upArrow && !key.downArrow && !key.leftArrow && !key.rightArrow && !key.tab) setText((t) => t + input);
  });

  const hair = '─'.repeat(size.w);
  return (
    <Box flexDirection="column" height={size.h} width={size.w}>
      <Text bold>delphi · ink spike · mission {mission}{view === 'council' ? ' · council' : ''}</Text>
      <Text dimColor>{hair}</Text>
      {view === 'chat' ? (
        <Box flexDirection="column" height={chatH}>
          {shown.map((l, i) => <Text key={i} wrap="truncate-end">{l || ' '}</Text>)}
          {off > 0 ? <Text color="yellow">↓ {off} lines below · PgDn</Text> : null}
        </Box>
      ) : (
        <Box flexDirection="column" height={chatH}>
          {seats.length === 0 ? <Text dimColor>no council yet: /council in the Rust console, or start one here later</Text> : null}
          {seats.map((s) => (
            <Box key={s.role} flexDirection="column" marginBottom={1}>
              <Text bold color="cyan">{s.role} <Text dimColor>({s.family})</Text></Text>
              <Text wrap="truncate-end">{s.stance ?? 'thinking…'}</Text>
              {s.critique ? <Text color="yellow">critique → {s.critique}</Text> : null}
            </Box>
          ))}
        </Box>
      )}
      <Text dimColor>{hair}</Text>
      <Box flexDirection="column">
        {(text + '▏').split('\n').map((l, i) => <Text key={i}>{i === 0 ? '› ' : '  '}{l}</Text>)}
      </Box>
      <Text dimColor>{hair}</Text>
      <Text dimColor>~{tokens} tokens · frames {frames.current} · {size.w}x{size.h}</Text>
    </Box>
  );
}

const cmd = await Rpc.connect(where.console_socket);
await cmd.attachAsConsole();
const mission = (await cmd.call('mission.open', { mission_id: null })).mission_id as string;
const ev = await Rpc.connect(where.console_socket);
await ev.attachAsConsole();
await ev.call('events.subscribe', {});

process.stdout.write('\x1b[?1049h');
const app = render(<App cmd={cmd} ev={ev} mission={mission} />, { exitOnCtrlC: false });
await app.waitUntilExit();
process.stdout.write('\x1b[?1049l');
cmd.close(); ev.close();
