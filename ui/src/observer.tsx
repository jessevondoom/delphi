// `delphi-ui --observe [view]`: the Observer window. Attaches with a narrator token, so the daemon refuses anything
// that would start work or approve anything; killed and reopened, it rebuilds the same screen from the daemon.
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Box, Text, useApp, useStdout } from 'ink';
import type { Ev } from './input.ts';
import { SAMPLE_SECS } from './meters.ts';
import { emptyState, epicOrder, feedLinks, observeFrame, parseView, stepView, type OModel, type OState, type View } from './observe-screen.ts';
import * as A from './archives.ts';
import { copyText, stepFocus } from './links.ts';
import type { Rpc } from './rpc.ts';
import { copyToClipboard, useSelection } from './select.ts';
import type { Term } from './terminal.ts';
import type { Info } from './theme.ts';
import { stillMotion } from './motion.ts';
import { useReduceMotion } from './reduce-motion.ts';

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
  const seen = useRef<Record<string, number>>({});
  const [pulse, setPulse] = useState<Record<string, number[]>>({});
  const total = useRef<Record<string, number>>({});
  const since = useMemo(() => Date.now(), []);
  const [liveTokens, setLiveTokens] = useState<Record<string, number>>({});
  const [liveEst, setLiveEst] = useState<Record<string, number>>({});
  const [connected, setConnected] = useState(true);
  const asked = useReduceMotion(cmd, ev);
  const still = useMemo(() => stillMotion(process.env, asked), [asked]);
  const [now, setNow] = useState(Date.now());
  // One clock for the badges. It only runs while something is working (thinking faces swap every 600 ms), so an
  // idle window does not repaint.
  const busy = Object.values(st.agents).some((a) => a.state === 'running');
  useEffect(() => {
    if (still || !busy) return;
    const t = setInterval(() => setNow(Date.now()), 300);
    return () => clearInterval(t);
  }, [still, busy]);
  // The cap meter's forecast samples: every few seconds, the tokens each running role used since the last look (logged plus live).
  useEffect(() => {
    if (!busy) return;
    const t = setInterval(() => {
      const { st: cur } = R.current;
      const next: Record<string, number> = {};
      for (const a of Object.values(cur.agents)) {
        const logged = a.tokens.input + a.tokens.cache_read + a.tokens.cache_write + a.tokens.output + a.tokens.thinking;
        const live = Object.values(cur.nodes).filter((n) => n.role === a.role && n.state === 'running').reduce((x, n) => x + (R.current.liveTokens[n.id] ?? R.current.liveEst[n.id] ?? 0), 0);
        next[a.role] = logged + live;
      }
      setPulse((p) => {
        const out: Record<string, number[]> = {};
        for (const [r, now] of Object.entries(next)) {
          const before = total.current[r];
          // A drop (a span ended and its live estimate gave way to the logged figure) is no negative pace.
          const d = before === undefined ? 0 : Math.max(0, now - before);
          out[r] = [...(p[r] ?? []), d].slice(-60);
        }
        total.current = next;
        return out;
      });
    }, SAMPLE_SECS * 1000);
    return () => clearInterval(t);
  }, [busy]);
  // A document opened from a link in another view (the change feed, `specs/029`) reads in its own window state;
  // closing it returns to the view it came from.
  const [rdr, setRdrState] = useState<A.Arch>(A.emptyArch());
  const [feed, setFeedState] = useState<{ focus: number | null; note: string }>({ focus: null, note: '' });
  const R = useRef({ view, pick, st, rdr, feed, scroll, selNode, size, liveTokens, liveEst });
  R.current = { view, pick, st, rdr, feed, scroll, selNode, size, liveTokens, liveEst };
  const setRdr = (f: (a: A.Arch) => A.Arch) => { const n = f(R.current.rdr); R.current.rdr = n; setRdrState(n); };
  const setFeed = (f: (a: { focus: number | null; note: string }) => { focus: number | null; note: string }) => { const n = f(R.current.feed); R.current.feed = n; setFeedState(n); };
  const picked = useRef(first !== null);

  const load = () => cmd.call('observe.state', {}).then((s: OState) => {
    setSt(s);
    setNow(Date.now()); // the rail's waterline is this moment
    if (!picked.current) { picked.current = true; setView((s.default_view as View) ?? 'epic'); }
  }).catch(() => setConnected(false));

  useEffect(() => { void load(); }, []);

  // A document opened from a link reads through `kb.get`, the read-only knowledge API on the narrator socket (spec 028 §6a).
  const kb = (target: string): Promise<{ status: number; body: any }> => cmd.call('kb.get', { target });
  const enc = encodeURIComponent;
  const run = (act: A.Action) => {
    const setArch = setRdr;
    const fail = (msg: string) => { setArch((a) => A.withNote(a, msg)); setFeed((f) => ({ ...f, note: msg })); };
    const a0 = R.current.rdr, tp = A.trustParam(a0);
    if (act.t === 'reload') { if (a0.searched && a0.q.trim() && !a0.browse) run({ t: 'search', q: a0.q.trim() }); else if (a0.browse) run({ t: 'browse', name: a0.browse }); }
    else if (act.t === 'search') kb(`/v1/search?q=${enc(act.q)}&limit=${A.RESULTS}&facets=1${a0.kind ? `&kind=${enc(a0.kind)}` : ''}${tp}`).then((r) => setArch((a) => (r.status === 200 ? A.gotHits(a, act.q, r.body) : A.withNote(a, r.body?.error ?? 'the search failed')))).catch(() => fail('the search failed'));
    else if (act.t === 'browse') kb(`/v1/documents?kind=${enc(act.name)}&limit=50${tp}`).then((r) => setArch((a) => A.gotBrowse(a, act.name, r.body ?? []))).catch(() => fail('could not list that collection'));
    else if (act.t === 'open') {
      const t = act.target;
      kb(`/v1/doc?path=${enc(t.path)}${tp}`).then((r) => { if (r.status === 200) setFeed((f) => ({ ...f, note: '' })); setArch((a) => (r.status === 200 ? A.gotDoc(a, r.body, t, R.current.size.w) : A.withNote(a, r.status === 404 ? `${t.path} is not in the knowledge base` : r.body?.error ?? 'could not open it'))); }).catch(() => fail('could not open it'));
    } else if (act.t === 'browser') fail('this window only reads: open links from the console, which checks your lists first');
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
        for (const id of Object.keys(spans)) seen.current[id] = Date.now();
        setLive((cur) => {
          const next = { ...cur };
          for (const [id, v] of Object.entries<any>(spans)) if (v?.text) next[id] = ((next[id] ?? '') + v.text).slice(-6000);
          return next;
        });
        setLiveTokens((cur) => {
          const next = { ...cur };
          let changed = false;
          for (const [id, v] of Object.entries<any>(spans)) if (typeof v?.tokens === 'number' && next[id] !== v.tokens) { next[id] = v.tokens; changed = true; }
          return changed ? next : cur;
        });
        setLiveEst((cur) => {
          const next = { ...cur };
          let changed = false;
          for (const [id, v] of Object.entries<any>(spans)) if (typeof v?.est === 'number' && next[id] !== v.est) { next[id] = v.est; changed = true; }
          return changed ? next : cur;
        });
      } else if (n.method === 'event') {
        if (typeof n.params?.span_id === 'string') seen.current[n.params.span_id] = Date.now();
        timer ??= setTimeout(() => { timer = null; void load(); }, 300);
        // The span's real usage is in the log now (or it has ended): the running total no longer stands for it.
        const gone = n.params?.type === 'agent.usage' ? n.params?.span_id : ['done', 'failed', 'escalated'].includes(n.params?.payload?.state) && n.params?.type === 'dag.node.state' ? n.params?.payload?.node_id : null;
        if (gone) setLiveTokens((cur) => { if (!(gone in cur)) return cur; const { [gone]: _t, ...rest } = cur; return rest; });
        if (n.params?.type === 'agent.usage' || gone) setLiveEst((cur) => { const id = gone ?? n.params?.span_id; if (!id || !(id in cur)) return cur; const { [id]: _e, ...rest } = cur; return rest; });
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
      // A document opened from a link takes the keys until it is closed.
      if (R.current.rdr.at >= 0 && !(e.t === 'ctrl' && e.c === 'c')) {
        const r = A.key(R.current.rdr, e, R.current.size.w, Math.max(1, R.current.size.h - 7));
        if (r.used) { setRdr(() => r.a); run(r.action); }
        return; // the view does not change under an open document
      }
      // The change feed's links: tab and shift+tab focus one, enter reads it here (or opens the address in the browser), esc lets go.
      if (v === 'changes' && (e.t === 'text' || e.t === 'key') && !(e.t === 'key' && (e.alt || e.ctrl))) {
        const { w, h } = R.current.size;
        const links = feedLinks({ info, pal: { dark }, st: R.current.st, view: 'changes' } as OModel, w);
        const foc = R.current.feed.focus, cur = links.find((l) => l.id === foc);
        const move = (dir: 1 | -1) => {
          const f = stepFocus(links, foc, dir), l = links.find((x) => x.id === f);
          const room = Math.max(1, h - 7), row = l?.spans[0]?.line ?? 0, top = R.current.scroll;
          if (l && (row < top || row >= top + room)) setScroll(Math.max(0, row - Math.floor(room / 2)));
          setFeed(() => ({ focus: f, note: '' }));
        };
        if (e.t === 'key' && e.k === 'tab' && links.length) { move(1); return; }
        if (e.t === 'key' && e.k === 'shift-tab' && links.length) { move(-1); return; }
        if (e.t === 'key' && e.k === 'enter' && cur) {
          if (cur.target.k === 'external') run({ t: 'browser', url: cur.target.url });
          else { setFeed((f) => ({ ...f, note: 'opening…' })); run({ t: 'open', target: cur.target }); }
          return;
        }
        if (e.t === 'text' && e.s === 'y' && cur) { run({ t: 'copy', text: copyText(cur.target) }); return; }
        if (e.t === 'key' && e.k === 'esc' && (foc !== null || R.current.feed.note)) { setFeed(() => ({ focus: null, note: '' })); return; }
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

  const model: OModel = { info, pal, st, view, pick, scroll, selNode, reader: rdr, feed, live, liveTokens, liveEst, lastSeen: seen.current, since, pulse, connected, now, still };
  const lines = observeFrame(model, size.w, size.h);
  const shown = useSelection(term, lines);
  return (
    <Box flexDirection="column" height={size.h} width={size.w}>
      <Text>{shown.join('\n')}</Text>
    </Box>
  );
}

export { parseView };
