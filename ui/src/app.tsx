import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Box, Text, useApp, useStdout } from 'ink';
import { mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as C from './composer.ts';
import { COMMANDS } from './commands.ts';
import { completions, searchHistory, type Completion, type ListDir } from './complete.ts';
import { addBlock, fold, type State } from './fold.ts';
import { resolveCard, runSlash } from './run.ts';
import * as I from './inbox.ts';
import { frame, transcript, type Model } from './screen.ts';
import type { Ev } from './input.ts';
import type { Rpc } from './rpc.ts';
import type { Term } from './terminal.ts';
import type { Info } from './theme.ts';

type Props = { cmd: Rpc; ev: Rpc; term: Term; info: Info; mission: string; dark: boolean; history: State };

/** Entries of a directory under the workspace's team root, for `@specs/…` completion. Read-only, names only. */
const listTeamDir = (info: Info): ListDir => (rel) => {
  if (rel.split('/').includes('..')) return [];
  try { return readdirSync(join(info.workspace, 'team', rel), { withFileTypes: true }).map((d) => ({ name: d.name, dir: d.isDirectory() })); }
  catch { return []; }
};

export function exportMarkdown(s: State, info: Info, mission: string): string {
  const out = [`---\nid: export-${mission}\ntitle: "Conversation export"\nauthor: operator\ntimestamp: ${new Date().toISOString()}\nsource_type: internal_generated\nconfidence: medium\nstatus: active\nsource_mission: ${mission}\ntags: [export]\n---\n`];
  for (const b of s.blocks) {
    if (b.kind === 'user') out.push(`**You** · ${new Date(b.at).toISOString()}\n\n${b.text}\n`);
    else if (b.kind === 'reply') out.push(`**${info.roles[b.role ?? '']?.name ?? b.role}** (${b.role}) · ${new Date(b.at).toISOString()}\n\n${b.text}\n`);
    else if (b.kind === 'tool') out.push(`- tool: \`${b.text}\` ${b.status ?? ''}`);
    else out.push(`> ${b.text.replace(/\n/g, '\n> ')}\n`);
  }
  return out.join('\n');
}

export function App({ cmd, ev, term, info, mission, dark, history }: Props) {
  const { exit } = useApp();
  const stdout = useStdout().stdout as unknown as NodeJS.WriteStream;
  const [size, setSize] = useState({ w: stdout.columns || 100, h: stdout.rows || 30 });
  const [now, setNow] = useState(Date.now());
  const [pal, setPal] = useState({ dark });
  const [state, setState] = useState<State>(history);
  const [composer, setComposer] = useState(() => C.empty());
  const [deck, setDeck] = useState<0 | 1 | 2>(1);
  const [back, setBack] = useState(0);
  const [help, setHelp] = useState(false);
  const [sel, setSel] = useState(0);
  const [expandTools, setExpandTools] = useState(false);
  const [queue, setQueue] = useState<string[]>([]);
  const [inbox, setInbox] = useState(() => I.newInbox(Date.now()));
  const [mode, setMode] = useState('default');
  const [search, setSearch] = useState<{ q: string; at: number; match: string | null } | null>(null);

  // Several keys can arrive in one read, so the key handler works on refs it updates itself, not on the last render.
  const comp = useRef(composer);
  const selRef = useRef(0);
  const sizeRef = useRef(size);
  sizeRef.current = size;
  const stateRef = useRef(state);
  stateRef.current = state;
  const queueRef = useRef<string[]>([]);
  const searchRef = useRef(search);
  const inboxRef = useRef(inbox);
  const deckRef = useRef<0 | 1 | 2>(1);
  deckRef.current = deck;
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const listDir = useMemo(() => listTeamDir(info), [info]);

  useEffect(() => {
    const onResize = () => setSize({ w: stdout.columns, h: stdout.rows });
    stdout.on('resize', onResize);
    return () => { stdout.off('resize', onResize); };
  }, [stdout]);

  const running = Object.keys(state.running).length;
  useEffect(() => { // a slow tick when idle (clock, countdown), a quick one while the spinner turns
    const t = setInterval(() => setNow(Date.now()), running ? 250 : 1000);
    return () => clearInterval(t);
  }, [running]);

  useEffect(() => {
    // Frames arrive many times a second; fold them in batches so a stream costs one repaint per 33 ms, not one per frame.
    let queued: any[] = [], timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => { timer = null; const q = queued; queued = []; setState((s) => q.reduce((a, n) => fold(a, n, Date.now()), s)); };
    const onNote = (n: any) => { queued.push(n); timer ??= setTimeout(flush, 33); };
    ev.on('notification', onNote);
    ev.on('closed', () => setState((s) => addBlock(s, { kind: 'note', text: 'the daemon connection closed', at: Date.now() })));
    return () => { ev.off('notification', onNote); if (timer) clearTimeout(timer); };
  }, [ev]);

  useEffect(() => { cmd.call('permissions.get', {}).then((v: any) => v?.mode && setMode(String(v.mode))).catch(() => {}); }, []);

  const note = (text: string) => setState((s) => addBlock(s, { kind: 'note', text, at: Date.now() }));

  const send = (text: string) => {
    const m = /^@(\S+)\s+([\s\S]+)$/.exec(text);
    let target: string | undefined, prompt = text;
    if (m) {
      const want = m[1]!.toLowerCase();
      target = Object.entries(info.roles).find(([id, r]) => id === want || r.name.toLowerCase() === want)?.[0];
      if (target) prompt = m[2]!;
    }
    setBack(0);
    cmd.call('task.submit', { mission_id: mission, prompt, ...(target ? { target } : {}) }).catch((e) => note('could not send: ' + (e?.message ?? JSON.stringify(e))));
  };

  // A queued message goes out when nothing is running.
  useEffect(() => {
    if (running === 0 && queueRef.current.length) {
      const [next, ...rest] = queueRef.current;
      queueRef.current = rest; setQueue(rest);
      send(next!);
    }
  }, [running]);

  const stop = () => {
    const had = queueRef.current.length;
    queueRef.current = []; setQueue([]);
    cmd.call('task.cancel', { mission_id: mission }).then((r: any) => {
      const n = (r?.cancelled ?? []).length;
      if (!n) note('nothing was running');
      else if (had) note(`${had} queued message(s) were dropped`);
    }).catch((e) => note('could not stop: ' + (e?.message ?? JSON.stringify(e))));
  };

  const run = (line: string) => {
    const name = line.split(/\s/)[0]!;
    if (name === '/quit') { exit(); return; }
    if (name === '/help') { setHelp((h) => !h); return; }
    if (name === '/clear') { setState((s) => ({ ...s, blocks: [] })); return; }
    if (name === '/export') {
      try {
        const dir = join(info.workspace, 'local/missions', mission, 'exports');
        mkdirSync(dir, { recursive: true });
        const file = join(dir, `conversation-${Date.now()}.md`);
        writeFileSync(file, exportMarkdown(stateRef.current, info, mission));
        note(`exported to ${file.replace(info.workspace + '/', '')}`);
      } catch (e: any) { note('could not export: ' + e.message); }
      return;
    }
    void runSlash(line, { cmd, mission, info, state: () => stateRef.current, post: note });
  };

  useEffect(() => {
    const menuNow = (): Completion | null => {
      if (searchRef.current) return null;
      const c = comp.current;
      return completions(C.textOf(c), c.cur, info, listDir);
    };
    const onEv = (e: Ev) => {
      const bodyH = Math.max(3, sizeRef.current.h - 9);
      const upd = (f: (c: C.Composer) => C.Composer) => { comp.current = f(comp.current); selRef.current = 0; setComposer(comp.current); setSel(0); };
      const setS = (s: typeof search) => { searchRef.current = s; setSearch(s); };
      const hist = comp.current.hist;

      // History search owns the keyboard while it is open.
      const sr = searchRef.current;
      if (sr && (e.t === 'text' || e.t === 'paste' || e.t === 'key' || e.t === 'ctrl')) {
        const find = (q: string, before: number) => { const i = searchHistory(hist, q, before); return i < 0 ? { at: before, match: null } : { at: i, match: hist[i]! }; };
        if (e.t === 'text' || e.t === 'paste') { const q = sr.q + (e.t === 'text' ? e.s : e.s.replace(/\n/g, ' ')); setS({ q, ...find(q, hist.length) }); return; }
        if (e.t === 'ctrl' && e.c === 'r') { setS({ ...sr, ...find(sr.q, sr.at) }); return; }
        if (e.t === 'ctrl' && e.c === 'c') { setS(null); return; }
        if (e.t === 'key' && e.k === 'backspace') { const q = sr.q.slice(0, -1); setS({ q, ...find(q, hist.length) }); return; }
        if (e.t === 'key' && e.k === 'esc') { setS(null); return; }
        if (e.t === 'key' && e.k === 'enter') { if (sr.match) upd((c) => C.insert(C.empty(c.hist), sr.match!)); setS(null); return; }
        return;
      }

      // Shift+Tab cycles the approval mode from anywhere.
      if (e.t === 'key' && e.k === 'shift-tab') {
        const order = ['default', 'accept-edits', 'mission-auto'];
        const next = order[(order.indexOf(modeRef.current) + 1) % order.length]!;
        cmd.call('permissions.set', { mode: next }).then((v: any) => { setMode(String(v?.mode ?? next)); }).catch((er) => note('could not change the approval mode: ' + (er?.message ?? JSON.stringify(er))));
        return;
      }

      // On the right deck the keyboard belongs to the inbox (an explicit focus change), never the prompt.
      if (deckRef.current === 2 && !(e.t === 'key' && e.shift && (e.k === 'left' || e.k === 'right')) && !(e.t === 'ctrl' && e.c === 'c')) {
        const now = Date.now();
        const { inbox: next, action } = I.key(inboxRef.current, I.cards(stateRef.current), e, now);
        inboxRef.current = next; setInbox(next);
        if (action.t === 'chat') { setDeck(1); deckRef.current = 1; }
        else if (action.t === 'resolve') void resolveCard(cmd, action.card, action.decision).then((bad) => bad && note(bad));
        else if (action.t === 'resolve-all') for (const c of action.cards) void resolveCard(cmd, c, 'allow').then((bad) => bad && note(bad));
        return;
      }

      const m = menuNow();
      const items = m?.items ?? [];
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
          if (e.c === 'o') { setExpandTools((x) => !x); return; }
          if (e.c === 'r') { setS({ q: '', at: hist.length, match: null }); return; }
          return;
        case 'key': {
          if (e.shift && (e.k === 'left' || e.k === 'right')) {
            const d = Math.max(0, Math.min(2, deckRef.current + (e.k === 'left' ? -1 : 1))) as 0 | 1 | 2;
            deckRef.current = d; setDeck(d);
            if (d === 2) { const fresh = I.newInbox(Date.now()); inboxRef.current = fresh; setInbox(fresh); }
            return;
          }
          const pick = (n: number) => { selRef.current = n; setSel(n); };
          const accept = (it: { insert: string }) => upd((c) => {
            const t = C.textOf(c), cs = [...t];
            const next = [...cs.slice(0, m!.from), ...[...it.insert], ...cs.slice(m!.to)];
            return { ...c, chars: next, cur: m!.from + [...it.insert].length, histAt: null };
          });
          switch (e.k) {
            case 'pgup': setBack((b) => b + bodyH - 1); return;
            case 'pgdn': setBack((b) => Math.max(0, b - bodyH + 1)); return;
            case 'left': upd(C.left); return;
            case 'right': upd(C.right); return;
            case 'home': upd(C.home); return;
            case 'end': upd(C.end); return;
            case 'backspace': upd(C.backspace); return;
            case 'delete': upd(C.del); return;
            case 'esc': {
              if (help) { setHelp(false); return; }
              if (C.textOf(comp.current)) { upd((c) => C.empty(c.hist)); return; }
              if (Object.keys(stateRef.current.running).length) stop();
              return;
            }
            case 'up': if (items.length) pick(Math.max(0, selRef.current - 1)); else upd(C.up); return;
            case 'down': if (items.length) pick(Math.min(items.length - 1, selRef.current + 1)); else upd(C.down); return;
            case 'tab': if (items.length) accept(items[Math.min(selRef.current, items.length - 1)]!); return;
            case 'enter': {
              if (e.alt) { upd((c) => C.insert(c, '\n')); return; }
              const text = C.textOf(comp.current);
              if (items.length) {
                const it = items[Math.min(selRef.current, items.length - 1)]!;
                const tok = text.slice(m!.from, m!.to);
                if (tok !== it.insert.trimEnd()) { accept(it); return; }
              }
              const { text: t, next } = C.submit(comp.current);
              if (!t) return;
              comp.current = next; setComposer(next);
              if (t.startsWith('/') && COMMANDS.some((c) => c.name === t.split(/\s/)[0])) run(t);
              else if (t.startsWith('/')) note(`unknown command ${t.split(/\s/)[0]}; / shows the list`);
              else if (Object.keys(stateRef.current.running).length) { queueRef.current = [...queueRef.current, t]; setQueue(queueRef.current); }
              else send(t);
              return;
            }
          }
        }
      }
    };
    term.on('ev', onEv);
    return () => { term.off('ev', onEv); };
  }, [term, help, info, listDir]);

  // Scrolled up, new output must not move what is being read: grow the offset by what was added below.
  const total = useRef(0);
  const base = { info, pal, state, now: 0, deck, composer, back: 0, help: false, menu: null, expandTools, search: null, queue, inbox, mode };
  const lineCount = useMemo(() => transcript(base, size.w).length, [state.blocks, state.live, size.w, pal, expandTools, queue]);
  useEffect(() => {
    const added = lineCount - total.current;
    total.current = lineCount;
    if (added > 0) setBack((b) => (b > 0 ? b + added : 0));
  }, [lineCount]);

  const comps = search ? null : completions(C.textOf(composer), composer.cur, info, listDir);
  const model: Model = { ...base, now, back, help, menu: comps ? { items: comps.items, sel } : null, search: search ? { q: search.q, match: search.match } : null };
  const lines = useMemo(() => frame(model, size.w, size.h), [state, now, deck, composer, back, sel, help, size, pal, expandTools, queue, search, inbox, mode]);
  return (
    <Box flexDirection="column" height={size.h} width={size.w}>
      <Text>{lines.join('\n')}</Text>
    </Box>
  );
}
