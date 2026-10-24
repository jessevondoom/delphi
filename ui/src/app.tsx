import React, { useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { Box, Text, useApp, useStdout } from 'ink';
import * as C from './composer.ts';
import { COMMANDS, filter } from './commands.ts';
import { addBlock, fold, initial, type State } from './fold.ts';
import { frame, type Model } from './screen.ts';
import type { Ev } from './input.ts';
import type { Rpc } from './rpc.ts';
import type { Term } from './terminal.ts';
import type { Info } from './theme.ts';

type Props = { cmd: Rpc; ev: Rpc; term: Term; info: Info; mission: string; dark: boolean };

export function App({ cmd, ev, term, info, mission, dark }: Props) {
  const { exit } = useApp();
  const stdout = useStdout().stdout as unknown as NodeJS.WriteStream;
  const [size, setSize] = useState({ w: stdout.columns || 100, h: stdout.rows || 30 });
  const [now, setNow] = useState(Date.now());
  const [pal, setPal] = useState({ dark });
  const [state, setState] = useState<State>(() => initial(Date.now()));
  const [composer, setComposer] = useState(() => C.empty());
  const [deck, setDeck] = useState<0 | 1 | 2>(1);
  const [back, setBack] = useState(0);
  const [menuSel, setMenuSel] = useState(0);
  const [help, setHelp] = useState(false);
  // Several keys can arrive in one read, so the handler works on refs that it updates itself, not on the last render.
  const comp = useRef(composer);
  const sel = useRef(0);
  const sizeRef = useRef(size);
  sizeRef.current = size;

  useEffect(() => {
    const onResize = () => setSize({ w: stdout.columns, h: stdout.rows });
    stdout.on('resize', onResize);
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => { stdout.off('resize', onResize); clearInterval(tick); };
  }, [stdout]);

  useEffect(() => {
    // Frames arrive many times a second; fold them in batches so a stream costs one repaint per 33 ms, not one per frame.
    let queue: any[] = [], timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => { timer = null; const q = queue; queue = []; setState((s) => q.reduce((a, n) => fold(a, n, Date.now()), s)); };
    const onNote = (n: any) => { queue.push(n); timer ??= setTimeout(flush, 33); };
    ev.on('notification', onNote);
    ev.on('closed', () => setState((s) => addBlock(s, { kind: 'note', text: 'the daemon connection closed', at: Date.now() })));
    return () => { ev.off('notification', onNote); if (timer) clearTimeout(timer); };
  }, [ev]);

  const note = (text: string) => setState((s) => addBlock(s, { kind: 'note', text, at: Date.now() }));

  const send = (text: string) => {
    const m = /^@(\S+)\s+([\s\S]+)$/.exec(text);
    let target: string | undefined, prompt = text;
    if (m) {
      const want = m[1]!.toLowerCase();
      target = Object.entries(info.roles).find(([id, r]) => id === want || r.name.toLowerCase() === want)?.[0];
      if (target) prompt = m[2]!;
    }
    setState((s) => addBlock(s, { kind: 'user', text, at: Date.now() }));
    setBack(0);
    cmd.call('task.submit', { mission_id: mission, prompt, ...(target ? { target } : {}) }).catch((e) => note('could not send: ' + (e?.message ?? JSON.stringify(e))));
  };

  const run = (name: string) => {
    if (name === '/quit') { exit(); return; }
    if (name === '/help') { setHelp((h) => !h); return; }
    if (name === '/clear') { setState((s) => ({ ...s, blocks: [] })); return; }
    note(`${name} is not in the full-screen console yet; it is in \`delphi chat --inline\``);
  };

  useEffect(() => {
    const onEv = (e: Ev) => {
      const text = C.textOf(comp.current);
      const menu = text.startsWith('/') && !/[\s]/.test(text) ? filter(text) : [];
      const bodyH = Math.max(3, sizeRef.current.h - 8);
      const upd = (f: (c: C.Composer) => C.Composer) => { comp.current = f(comp.current); sel.current = 0; setComposer(comp.current); setMenuSel(0); };
      const pick = (n: number) => { sel.current = n; setMenuSel(n); };
      switch (e.t) {
        case 'bg': setPal({ dark: e.dark }); return;
        case 'wheel': setBack((b) => Math.max(0, b + (e.dir === 'up' ? 3 : -3))); return;
        case 'paste': upd((c) => C.insert(c, e.s)); return;
        case 'text': setHelp(false); upd((c) => C.insert(c, e.s)); return;
        case 'ctrl':
          if (e.c === 'c') { exit(); return; }
          if (e.c === 'j') { upd((c) => C.insert(c, '\n')); return; }
          if (e.c === 'a') { upd(C.home); return; }
          if (e.c === 'e') { upd(C.end); return; }
          if (e.c === 'u') { upd(C.killToStart); return; }
          if (e.c === 'k') { upd(C.killToEnd); return; }
          if (e.c === 'w') { upd(C.killWord); return; }
          if (e.c === 'l') { setBack(0); return; }
          return;
        case 'key': {
          if (e.shift && (e.k === 'left' || e.k === 'right')) { setDeck((d) => Math.max(0, Math.min(2, d + (e.k === 'left' ? -1 : 1))) as 0 | 1 | 2); return; }
          switch (e.k) {
            case 'pgup': setBack((b) => b + bodyH - 1); return;
            case 'pgdn': setBack((b) => Math.max(0, b - bodyH + 1)); return;
            case 'left': upd(C.left); return;
            case 'right': upd(C.right); return;
            case 'home': upd(C.home); return;
            case 'end': upd(C.end); return;
            case 'backspace': upd(C.backspace); return;
            case 'delete': upd(C.del); return;
            case 'esc': if (help) setHelp(false); else upd((c) => C.empty(c.hist)); return;
            case 'up': if (menu.length) pick(Math.max(0, sel.current - 1)); else upd(C.up); return;
            case 'down': if (menu.length) pick(Math.min(Math.min(menu.length, 4) - 1, sel.current + 1)); else upd(C.down); return;
            case 'tab': { if (menu.length) { const c = menu[Math.min(sel.current, menu.length - 1)]!; upd((x) => C.insert(C.empty(x.hist), c.name + ' ')); } return; }
            case 'enter': {
              if (e.alt) { upd((c) => C.insert(c, '\n')); return; }
              if (menu.length) {
                const c = menu[Math.min(sel.current, menu.length - 1)]!;
                if (text !== c.name) { upd((x) => C.insert(C.empty(x.hist), c.name)); return; }
              }
              const { text: t, next } = C.submit(comp.current);
              if (!t) return;
              comp.current = next; setComposer(next);
              if (t.startsWith('/') && COMMANDS.some((c) => c.name === t.split(/\s/)[0])) run(t.split(/\s/)[0]!);
              else if (t.startsWith('/')) note(`unknown command ${t.split(/\s/)[0]}; / shows the list`);
              else send(t);
              return;
            }
          }
        }
      }
    };
    term.on('ev', onEv);
    return () => { term.off('ev', onEv); };
  }, [term, help]);

  const model: Model = { info, pal, state, now, deck, composer, back, menuSel, help };
  const lines = useMemo(() => frame(model, size.w, size.h), [model.state, now, deck, composer, back, menuSel, help, size, pal]);
  return (
    <Box flexDirection="column" height={size.h} width={size.w}>
      <Text>{lines.join('\n')}</Text>
    </Box>
  );
}
