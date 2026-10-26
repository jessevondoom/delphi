// `delphi-ui --observe [view]`: the Observer window. Attaches with a narrator token, so the daemon refuses anything
// that would start work or approve anything; killed and reopened, it rebuilds the same screen from the daemon.
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Box, Text, useApp, useStdout } from 'ink';
import type { Ev } from './input.ts';
import { emptyState, epicOrder, observeFrame, parseView, stepView, type OModel, type OState, type View } from './observe-screen.ts';
import * as A from './archives.ts';
import { openExternal } from './external.ts';
import { copyText } from './links.ts';
import type { Rpc } from './rpc.ts';
import { copyToClipboard, useSelection } from './select.ts';
import type { Term } from './terminal.ts';
import type { Info } from './theme.ts';
import { stillMotion } from './motion.ts';

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
  const [connected, setConnected] = useState(true);
  const still = useMemo(() => stillMotion(), []);
  const [now, setNow] = useState(Date.now());
  // One clock for the badges. It only runs while something is working (thinking faces swap every 600 ms), so an
  // idle window does not repaint.
  const busy = Object.values(st.agents).some((a) => a.state === 'running');
  useEffect(() => {
    if (still || !busy) return;
    const t = setInterval(() => setNow(Date.now()), 300);
    return () => clearInterval(t);
  }, [still, busy]);
  const [arch, setArchState] = useState<A.Arch>(A.emptyArch());
  const R = useRef({ view, pick, st, arch, scroll, selNode, size });
  R.current = { view, pick, st, arch, scroll, selNode, size };
  // The ref moves at once, not when React gets to the update: text and Enter can arrive in one read, and the second
  // key must see what the first one did.
  const setArch = (f: (a: A.Arch) => A.Arch) => { const n = f(R.current.arch); R.current.arch = n; setArchState(n); };
  const picked = useRef(first !== null);

  const load = () => cmd.call('observe.state', {}).then((s: OState) => {
    setSt(s);
    setNow(Date.now()); // the rail's waterline is this moment
    if (!picked.current) { picked.current = true; setView((s.default_view as View) ?? 'epic'); }
  }).catch(() => setConnected(false));

  useEffect(() => { void load(); }, []);

  // The knowledge window reads through `kb.get`, the read-only knowledge API on the narrator socket (spec 028 §6a).
  const kb = (target: string): Promise<{ status: number; body: any }> => cmd.call('kb.get', { target });
  const enc = encodeURIComponent;
  const loadShelf = () => Promise.all([kb(`/v1/collections?${A.trustParam(R.current.arch).slice(1)}`), kb(`/v1/documents?limit=6${A.trustParam(R.current.arch)}`), kb('/v1/health')])
    .then(([c, d, h]) => setArch((a) => A.gotShelf(a, c.body ?? [], d.body ?? [], h.body)))
    .catch(() => setArch((a) => A.withNote(a, 'the knowledge base is not available')));
  useEffect(() => { if (view === 'archives' && !R.current.arch.shelf) void loadShelf(); }, [view, connected]);
  const run = (act: A.Action) => {
    const fail = (msg: string) => setArch((a) => A.withNote(a, msg));
    const a0 = R.current.arch, tp = A.trustParam(a0);
    if (act.t === 'reload') { void loadShelf(); if (a0.searched && a0.q.trim() && !a0.browse) run({ t: 'search', q: a0.q.trim() }); else if (a0.browse) run({ t: 'browse', name: a0.browse }); }
    else if (act.t === 'search') kb(`/v1/search?q=${enc(act.q)}&limit=${A.RESULTS}${a0.scope ? `&collection=${enc(a0.scope)}` : ''}${tp}`).then((r) => setArch((a) => (r.status === 200 ? A.gotHits(a, act.q, r.body) : A.withNote(a, r.body?.error ?? 'the search failed')))).catch(() => fail('the search failed'));
    else if (act.t === 'browse') kb(`/v1/documents?collection=${enc(act.name)}&limit=50${tp}`).then((r) => setArch((a) => A.gotBrowse(a, act.name, r.body ?? []))).catch(() => fail('could not list that collection'));
    else if (act.t === 'open') {
      const t = act.target;
      kb(`/v1/doc?path=${enc(t.path)}${tp}`).then((r) => setArch((a) => (r.status === 200 ? A.gotDoc(a, r.body, t, R.current.size.w) : A.withNote(a, r.status === 404 ? `${t.path} is not in the knowledge base` : r.body?.error ?? 'could not open it')))).catch(() => fail('could not open it'));
    } else if (act.t === 'browser') fail(openExternal(act.url) ? `opened ${act.url} in your browser` : 'could not open the browser');
    else if (act.t === 'copy') { copyToClipboard(act.text); fail('copied'); }
  };

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
      } else if (n.method === 'event') {
        timer ??= setTimeout(() => { timer = null; void load(); }, 300);
        if (n.params?.type === 'dag.node.state' && ['done', 'failed', 'escalated'].includes(n.params?.payload?.state)) { setLive((cur) => { const { [n.params.payload.node_id]: _x, ...rest } = cur; return rest; }); }
      } else if (n.method === 'resync_required') void load();
    };
    ev.on('notification', onNote);
    const down = () => setConnected(false);
    const up = () => { cmd.whenReady().then(() => { setConnected(true); return load(); }); };
    ev.on('closed', down);
    ev.on('reconnected', up);
    return () => { ev.off('notification', onNote); ev.off('closed', down); ev.off('reconnected', up); if (timer) clearTimeout(timer); };
  }, [ev]);

  useEffect(() => {
    // Only left and right change the view, and they stop at the ends; up, down, the page keys and the wheel scroll.
    // Nothing else is bound and nothing else is advertised (`005e` §9.2).
    const go = (v: View) => { setView(v); setScroll(0); setPick(Number.MAX_SAFE_INTEGER); };
    const step = (d: -1 | 1) => { const next = stepView(R.current.view, d); if (next !== R.current.view) go(next); };
    const onEv = (e: Ev) => {
      const { view: v } = R.current;
      if (e.t === 'bg') { if (!themeForced) setPal({ dark: e.dark }); return; }
      if (v === 'archives' && !(e.t === 'ctrl' && e.c === 'c')) {
        const r = A.key(R.current.arch, e, R.current.size.w, Math.max(1, R.current.size.h - 7));
        if (r.used) { setArch(() => r.a); run(r.action); return; }
      }
      if (e.t === 'wheel') { setScroll((x) => Math.max(0, x + (e.dir === 'up' ? -3 : 3))); return; }
      if (e.t === 'ctrl' && e.c === 'c') { exit(); return; }
      if (e.t !== 'key' || e.shift || e.alt) return;
      if (e.k === 'right') { step(1); return; }
      if (e.k === 'left') { step(-1); return; }
      if (e.k === 'down') { setScroll((x) => x + 1); return; }
      if (e.k === 'up') { setScroll((x) => Math.max(0, x - 1)); return; }
      if (e.k === 'pgdn') { setScroll((x) => x + 10); return; }
      if (e.k === 'pgup') { setScroll((x) => Math.max(0, x - 10)); return; }
    };
    term.on('ev', onEv);
    return () => { term.off('ev', onEv); };
  }, [term]);

  const model: OModel = { info, pal, st, view, pick, scroll, selNode, arch, live, connected, now, still };
  const lines = observeFrame(model, size.w, size.h);
  const shown = useSelection(term, lines);
  return (
    <Box flexDirection="column" height={size.h} width={size.w}>
      <Text>{shown.join('\n')}</Text>
    </Box>
  );
}

export { parseView };
