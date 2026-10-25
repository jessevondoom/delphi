// `delphi-ui --observe [view]`: the Observer window. Attaches with a narrator token, so the daemon refuses anything
// that would start work or approve anything; killed and reopened, it rebuilds the same screen from the daemon.
import React, { useEffect, useRef, useState } from 'react';
import { Box, Text, useApp, useStdout } from 'ink';
import type { Ev } from './input.ts';
import { emptyState, epicOrder, observeFrame, parseView, VIEWS, type Hit, type OModel, type OState, type View } from './observe-screen.ts';
import type { Rpc } from './rpc.ts';
import { useSelection } from './select.ts';
import type { Term } from './terminal.ts';
import type { Info } from './theme.ts';

type Props = { cmd: Rpc; ev: Rpc; term: Term; info: Info; dark: boolean; themeForced?: boolean; view: View | null };

export function Observer({ cmd, ev, term, info, dark, themeForced, view: first }: Props) {
  const { exit } = useApp();
  const stdout = useStdout().stdout as unknown as NodeJS.WriteStream;
  const [size, setSize] = useState({ w: stdout.columns || 100, h: stdout.rows || 30 });
  const [pal, setPal] = useState({ dark });
  const [st, setSt] = useState<OState>(emptyState());
  const [view, setView] = useState<View>(first ?? 'epic');
  const [pick, setPick] = useState(Number.MAX_SAFE_INTEGER);
  const [scroll, setScroll] = useState(0);
  const [selNode, setSelNode] = useState(0);
  const [live, setLive] = useState<Record<string, string>>({});
  const [think, setThink] = useState<Record<string, string>>({});
  const [connected, setConnected] = useState(true);
  const [diff, setDiff] = useState<OModel['diff']>(null);
  const [search, setSearch] = useState<OModel['search']>({ editing: false, query: '', hits: [], note: '' });
  const R = useRef({ view, pick, st, search, scroll, selNode, size });
  R.current = { view, pick, st, search, scroll, selNode, size };
  const picked = useRef(first !== null);

  const load = () => cmd.call('observe.state', {}).then((s: OState) => {
    setSt(s);
    if (!picked.current) { picked.current = true; setView((s.default_view as View) ?? 'epic'); }
  }).catch(() => setConnected(false));

  useEffect(() => { void load(); }, []);

  useEffect(() => {
    const onResize = () => setSize({ w: stdout.columns, h: stdout.rows });
    stdout.on('resize', onResize);
    return () => { stdout.off('resize', onResize); };
  }, [stdout]);

  useEffect(() => {
    // Events change the picture: refetch it, at most about three times a second. Frames only feed the live text.
    let timer: ReturnType<typeof setTimeout> | null = null;
    const onNote = (n: any) => {
      if (n.method === 'delta.frame') {
        const spans = n.params?.spans ?? {};
        setLive((cur) => {
          const next = { ...cur };
          for (const [id, v] of Object.entries<any>(spans)) if (v?.text) next[id] = ((next[id] ?? '') + v.text).slice(-6000);
          return next;
        });
        setThink((cur) => {
          const next = { ...cur };
          for (const [id, v] of Object.entries<any>(spans)) if (v?.thinking) next[id] = ((next[id] ?? '') + v.thinking).slice(-4000);
          return next;
        });
      } else if (n.method === 'event') {
        timer ??= setTimeout(() => { timer = null; void load(); }, 300);
        if (n.params?.type === 'dag.node.state' && ['done', 'failed', 'escalated'].includes(n.params?.payload?.state)) { setLive((cur) => { const { [n.params.payload.node_id]: _x, ...rest } = cur; return rest; }); setThink((cur) => { const { [n.params.payload.node_id]: _y, ...rest } = cur; return rest; }); }
      } else if (n.method === 'resync_required') void load();
    };
    ev.on('notification', onNote);
    const down = () => setConnected(false);
    const up = () => { cmd.whenReady().then(() => { setConnected(true); return load(); }); };
    ev.on('closed', down);
    ev.on('reconnected', up);
    return () => { ev.off('notification', onNote); ev.off('closed', down); ev.off('reconnected', up); if (timer) clearTimeout(timer); };
  }, [ev]);

  const refreshDiff = () => {
    const b = R.current.st.builds[Math.min(R.current.pick, R.current.st.builds.length - 1)];
    if (!b) { setDiff(null); return; }
    cmd.call('observe.diff', { task_id: b.task_id }).then(setDiff).catch(() => setDiff(null));
  };
  useEffect(() => { // the live diff follows the build on screen
    if (view !== 'forge') return;
    refreshDiff();
    const t = setInterval(refreshDiff, 3000);
    return () => clearInterval(t);
  }, [view, pick, st.builds.length]);

  useEffect(() => {
    const go = (v: View) => { setView(v); setScroll(0); setPick(Number.MAX_SAFE_INTEGER); };
    const step = (d: number) => go(VIEWS[(VIEWS.indexOf(R.current.view) + d + VIEWS.length) % VIEWS.length]!);
    const onEv = (e: Ev) => {
      const { view: v, search: s } = R.current;
      if (e.t === 'bg') { if (!themeForced) setPal({ dark: e.dark }); return; }
      if (e.t === 'wheel') { setScroll((x) => Math.max(0, x + (e.dir === 'up' ? -3 : 3))); return; }
      if (s.editing) {
        if (e.t === 'text' && [...s.query].length < 200) setSearch({ ...s, query: s.query + e.s });
        else if (e.t === 'key' && e.k === 'backspace') setSearch({ ...s, query: [...s.query].slice(0, -1).join('') });
        else if (e.t === 'key' && e.k === 'esc') setSearch({ ...s, editing: false });
        else if (e.t === 'key' && e.k === 'enter') {
          setSearch({ ...s, editing: false });
          if (s.query.trim()) cmd.call('kb.search', { query: s.query }).then((r: any) => setSearch((cur) => ({ ...cur, hits: (r?.hits ?? r ?? []) as Hit[], note: (r?.hits ?? r ?? []).length ? '' : 'no matches' }))).catch((er) => setSearch((cur) => ({ ...cur, note: String(er?.message ?? er) })));
        }
        return;
      }
      if (e.t === 'ctrl' && e.c === 'c') { exit(); return; }
      if (e.t === 'text') {
        if (e.s === 'q') { exit(); return; }
        if (/^[1-7]$/.test(e.s)) { go(VIEWS[Number(e.s) - 1]!); return; }
        if (e.s === '/' && v === 'archives') { setSearch({ ...s, editing: true }); return; }
        if (e.s === '[') { setPick((p) => Math.max(0, Math.min(p, 1e6) - 1)); return; }
        if (e.s === ']') { setPick((p) => p + 1); return; }
        if (e.s === 'j') { v === 'epic' ? setSelNode((x) => x + 1) : setScroll((x) => x + 1); return; }
        if (e.s === 'k') { v === 'epic' ? setSelNode((x) => Math.max(0, x - 1)) : setScroll((x) => Math.max(0, x - 1)); return; }
        if (e.s === ' ') { setScroll((x) => x + 10); return; }
        if (e.s === 'r' && v === 'forge') { refreshDiff(); return; }
        return;
      }
      if (e.t === 'key') {
        if (e.k === 'esc') { exit(); return; }
        if (e.shift && e.k === 'right') { step(1); return; }
        if (e.shift && e.k === 'left') { step(-1); return; }
        if (e.k === 'tab') { step(1); return; }
        if (e.k === 'shift-tab') { step(-1); return; }
        if (e.k === 'down') { v === 'epic' ? setSelNode((x) => Math.min(x + 1, Math.max(0, epicOrder(R.current.st).length - 1))) : setScroll((x) => x + 1); return; }
        if (e.k === 'up') { v === 'epic' ? setSelNode((x) => Math.max(0, x - 1)) : setScroll((x) => Math.max(0, x - 1)); return; }
        if (e.k === 'pgdn') { setScroll((x) => x + 10); return; }
        if (e.k === 'pgup') { setScroll((x) => Math.max(0, x - 10)); return; }
        if (e.k === 'home') { setScroll(0); return; }
      }
    };
    term.on('ev', onEv);
    return () => { term.off('ev', onEv); };
  }, [term]);

  const model: OModel = { info, pal, st, view, pick, scroll, selNode, search, diff, live, connected, think };
  const lines = observeFrame(model, size.w, size.h);
  const shown = useSelection(term, lines);
  return (
    <Box flexDirection="column" height={size.h} width={size.w}>
      <Text>{shown.join('\n')}</Text>
    </Box>
  );
}

export { parseView };
