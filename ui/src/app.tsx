import { deckTarget, type Deck } from './decknav.ts';
import { graphemes } from './text.ts';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Box, Text, useApp, useStdout } from 'ink';
import { mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as C from './composer.ts';
import { lookup } from './commands.ts';
import { bundleTarget, completions, enterAccepts, searchHistory, type Completion, type ListDir } from './complete.ts';
import { addBlock, fold, type State } from './fold.ts';
import { contextReport, exportAll, exportHead } from './session.ts';
import { loadHistory } from './attach.ts';
import { resolveCard, runSlash } from './run.ts';
import * as I from './inbox.ts';
import * as P from './panel.ts';
import * as A from './archives.ts';
import { openExternal } from './external.ts';
import * as O from './org.ts';
import { noteSources, passageOf, pin, previewOf, readNotes } from './research.ts';
import { copyText, firstVisible, stepFocus, type Target } from './links.ts';
import { execFile } from 'node:child_process';
import { anchorBack, backShowing, bodyHeight, chatLinks, frame, replyAnchor, transcriptCount, typingOf, type Model } from './screen.ts';
import { helpKey } from './help.ts';
import { modeNote, nextMode } from './mode.ts';
import { STRIP_ROLES, stillMotion, tickKey, TICK_MS, typingFrame } from './motion.ts';
import { useReduceMotion } from './reduce-motion.ts';
import type { Ev } from './input.ts';
import { copyToClipboard, useSelection } from './select.ts';
import type { Rpc } from './rpc.ts';
import type { Term } from './terminal.ts';
import type { Info, Pal, Rgb } from './theme.ts';

type Props = { cmd: Rpc; ev: Rpc; term: Term; info: Info; mission: string; dark: boolean; bg?: Rgb | null; themeForced?: boolean; history: State };

/** Entries of a directory under the workspace's team root, for `@specs/…` completion. Read-only, names only. */
const listTeamDir = (info: Info): ListDir => (rel) => {
  if (rel.split('/').includes('..')) return [];
  try { return readdirSync(join(info.workspace, 'team', rel), { withFileTypes: true }).map((d) => ({ name: d.name, dir: d.isDirectory() })); }
  catch { return []; }
};

export function exportMarkdown(s: State, info: Info, mission: string): string {
  const out = [exportHead(mission)];
  for (const b of s.blocks) {
    if (b.kind === 'user') out.push(`**You** · ${new Date(b.at).toISOString()}\n\n${b.text}\n`);
    else if (b.kind === 'reply') out.push(`**${info.roles[b.role ?? '']?.name ?? b.role}** (${b.role}) · ${new Date(b.at).toISOString()}\n\n${b.text}\n`);
    else if (b.kind === 'tool') out.push(`- tool: \`${b.text}\` ${b.status ?? ''}`);
    else out.push(`> ${b.text.replace(/\n/g, '\n> ')}\n`);
  }
  return out.join('\n');
}

export function App({ cmd, ev, term, info, mission, dark, bg, themeForced, history }: Props) {
  const { exit } = useApp();
  const stdout = useStdout().stdout as unknown as NodeJS.WriteStream;
  const [size, setSize] = useState({ w: stdout.columns || 100, h: stdout.rows || 30 });
  const [now, setNow] = useState(Date.now());
  const [pal, setPal] = useState<Pal>({ dark, bg: bg ?? null });
  const [state, setState] = useState<State>(history);
  const [composer, setComposer] = useState(() => C.empty());
  const [deck, setDeck] = useState<0 | 1 | 2>(1);
  const [back, setBack] = useState(0);
  const [arch, setArchState] = useState<A.Arch>(() => A.emptyArch(true));
  // A document opened from a link in the chat reads in its own window state (the knowledge deck keeps its search and history), and link mode
  // (`specs/029`): the pointers and addresses in the transcript can be focused and followed. Both are refs first, like the rest of the key state.
  const [rdr, setRdrState] = useState<A.Arch>(() => A.emptyArch(true));
  const [lm, setLmState] = useState<{ focus: number | null; note: string } | null>(null);
  const [help, setHelp] = useState(false);
  const [sel, setSel] = useState(0);
  const [expandTools, setExpandTools] = useState(false);
  const [queue, setQueue] = useState<string[]>([]);
  const [inbox, setInbox] = useState(() => I.newInbox(Date.now()));
  const [mode, setMode] = useState('default');
  const [panel, setPanel] = useState<P.Panel | null>(null);
  const [noModel, setNoModel] = useState(false);
  const [search, setSearch] = useState<{ q: string; at: number; match: string | null } | null>(null);

  // Several keys can arrive in one read, so the key handler works on refs it updates itself, not on the last render.
  const comp = useRef(composer);
  const selRef = useRef(0);
  const backRef = useRef(0);
  backRef.current = back;
  /** The transcript line of the label row a long reply is pinned to, while it is (`005e` §7.2), and which reply that is. */
  const anchorRef = useRef<number | null>(null);
  const lastReply = useRef<'live' | number | null>(null);
  const sizeRef = useRef(size);
  sizeRef.current = size;
  const stateRef = useRef(state);
  stateRef.current = state;
  const queueRef = useRef<string[]>([]);
  const quitAskedAt = useRef(0); // when Ctrl+C last asked "again to quit"; a second press within 2 s leaves
  const searchRef = useRef(search);
  const inboxRef = useRef(inbox);
  const deckRef = useRef<0 | 1 | 2>(1);
  deckRef.current = deck;
  const panelRef = useRef<P.Panel | null>(null);
  const rdrRef = useRef(rdr);
  const lmRef = useRef(lm);
  const expandRef = useRef(false);
  const palRef = useRef(pal);
  palRef.current = pal;
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const listDir = useMemo(() => listTeamDir(info), [info]);

  useEffect(() => {
    const onResize = () => setSize({ w: stdout.columns, h: stdout.rows });
    stdout.on('resize', onResize);
    return () => { stdout.off('resize', onResize); };
  }, [stdout]);

  const running = Object.keys(state.running).length;
  const asked = useReduceMotion(cmd, ev);
  const still = useMemo(() => stillMotion(process.env, asked), [asked]);
  // One timer for everything that moves with time (badge frames, the clock, the spinner). It looks every 150 ms and
  // asks for a repaint only when something on screen would differ, so an idle window does not redraw.
  useEffect(() => {
    let last = '';
    const t = setInterval(() => {
      const n = Date.now(), s = stateRef.current;
      const ty = typingOf(s);
      const key = tickKey(s.agents, STRIP_ROLES.filter((r) => info.roles[r]), n, still, Object.keys(s.running).length > 0, ty ? String(typingFrame(ty.since, ty.id, n, still)) : '');
      if (key !== last) { last = key; setNow(n); }
    }, TICK_MS);
    return () => clearInterval(t);
  }, [info, still]);

  useEffect(() => {
    // Frames arrive many times a second; fold them in batches so a stream costs one repaint per 33 ms, not one per frame.
    let queued: any[] = [], timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => { timer = null; const q = queued; queued = []; setState((s) => q.reduce((a, n) => fold(a, n, Date.now()), s)); };
    const onNote = (n: any) => {
      queued.push(n); timer ??= setTimeout(flush, 33);
      if (n?.method === 'event' && n.params?.type === 'model.recovered') checkModels();
    };
    ev.on('notification', onNote);
    const onClosed = () => setState((s) => addBlock(s, { kind: 'note', text: 'the daemon connection closed; trying to reconnect…', at: Date.now() }));
    const onBack = () => { // what happened meanwhile is in the log: rebuild from it, keep what is only on screen
      cmd.whenReady().then(() => loadHistory(cmd, mission)).then((h) => setState((s) => addBlock({ ...h, activity: s.activity, ended: s.ended, sources: { ...h.sources, ...s.sources } }, { kind: 'note', text: 'reconnected to the daemon', at: Date.now() })));
    };
    ev.on('closed', onClosed);
    ev.on('reconnected', onBack);
    return () => { ev.off('notification', onNote); ev.off('closed', onClosed); ev.off('reconnected', onBack); if (timer) clearTimeout(timer); };
  }, [ev]);

  useEffect(() => { cmd.call('permissions.get', {}).then((v: any) => v?.mode && setMode(String(v.mode))).catch(() => {}); }, []);

  // Does the workspace have any model at all? Asked on attach, when something is sent and when a model comes back
  // (`005e` §6.2). A daemon that does not say (older) is taken to have one.
  function checkModels() { cmd.call('models.get', {}).then((v: any) => setNoModel(v?.configured === false)).catch(() => {}); }
  useEffect(checkModels, []);

  const note = (text: string, tone?: 'info' | 'error' | 'ok') => setState((s) => addBlock(s, { kind: 'note', text, at: Date.now(), tone }));

  const send = (text: string) => {
    const m = /^@(\S+)\s+([\s\S]+)$/.exec(text);
    let target: string | undefined, prompt = text;
    if (m) {
      const want = m[1]!.toLowerCase();
      target = Object.entries(info.roles).find(([id, r]) => id === want || r.name.toLowerCase() === want)?.[0];
      if (target) prompt = m[2]!;
      else {
        // `@bundle:agent` / `@org/bundle:agent`: a bundle's agent; the daemon checks the name and says if it is wrong.
        const b = bundleTarget(m[1]!);
        if (b) { target = b; prompt = m[2]!; }
      }
    }
    setBack(0);
    anchorRef.current = null;
    checkModels();
    cmd.call('task.submit', { mission_id: mission, prompt, ...(target ? { target } : {}) }).catch((e) => note('could not send: ' + (e?.message ?? JSON.stringify(e)), 'error'));
  };

  // A queued message goes out when nothing is running.
  useEffect(() => {
    if (running === 0 && queueRef.current.length) {
      const [next, ...rest] = queueRef.current;
      queueRef.current = rest; setQueue(rest);
      send(next!);
    }
  }, [running]);

  /** Scroll the chat. Reaching the end resumes following. */
  const scroll = (by: number) => {
    const next = Math.max(0, backRef.current + by);
    if (next === 0) anchorRef.current = null;
    backRef.current = next;
    setBack(next);
  };
  /** Back to the end of the chat, following new output. */
  const follow = () => { anchorRef.current = null; backRef.current = 0; setBack(0); };

  const stop = () => {
    const had = queueRef.current.length;
    queueRef.current = []; setQueue([]);
    cmd.call('task.cancel', { mission_id: mission }).then((r: any) => {
      const n = (r?.cancelled ?? []).length;
      if (!n) note('nothing was running');
      else if (had) note(`${had} queued message(s) were dropped`);
    }).catch((e) => note('could not stop: ' + (e?.message ?? JSON.stringify(e)), 'error'));
  };

  const run = (line: string) => {
    const typed = line.split(/\s/)[0]!;
    const name = lookup(typed)?.name ?? typed; // `/exit` is `/quit`, `/narrator` is `/observe`
    if (name === '/quit') { exit(); return; }
    if (name === '/help') { setHelp((h) => !h); return; }
    if (name === '/new') {
      // The daemon starts the session and announces it; every console clears on that event. Clear here too, so this one does not wait.
      cmd.call('session.new', { mission_id: mission })
        .then(() => setState((s) => fold(s, { method: 'event', params: { type: 'session.started', payload: {} } }, Date.now())))
        .catch((e) => note('could not start a new session: ' + (e?.message ?? JSON.stringify(e)), 'error'));
      return;
    }
    if (name === '/context') {
      cmd.call('session.context', { mission_id: mission })
        .then((r: any) => note(contextReport(r, line.slice(name.length).trim() === 'full')))
        .catch((e) => note('could not read the context: ' + (e?.message ?? JSON.stringify(e)), 'error'));
      return;
    }
    if (name === '/export') {
      // The screen is the current session; `/export all` is the whole mission, a section per session, from the daemon's log.
      const all = line.slice(name.length).trim() === 'all';
      const write = (text: string, stem: string) => {
        try {
          const dir = join(info.workspace, 'local/missions', mission, 'exports');
          mkdirSync(dir, { recursive: true });
          const file = join(dir, `${stem}-${Date.now()}.md`);
          writeFileSync(file, text);
          note(`exported to ${file.replace(info.workspace + '/', '')}`, 'ok');
        } catch (e: any) { note('could not export: ' + e.message, 'error'); }
      };
      if (!all) { write(exportMarkdown(stateRef.current, info, mission), 'conversation'); return; }
      cmd.call('observe.snapshot', { mission_id: mission })
        .then((r: any) => write(exportAll(r?.events ?? [], info, mission), 'mission'))
        .catch((e) => note('could not export: ' + (e?.message ?? JSON.stringify(e)), 'error'));
      return;
    }
    if (name === '/search') { const q = line.slice(name.length).trim(); openPanel(P.openSearch(q)); if (q) doSearch(q); return; }
    if (name === '/mouse') { term.setMouse(!term.mouse); note(term.mouse ? 'mouse on: the wheel scrolls; drag to select and copy (double-click a word, triple-click a line)' : 'mouse off: select and copy with the terminal itself; scroll with PgUp/PgDn'); return; }
    if (name === '/sources') { openPanel(P.openSources()); return; }
    if (name === '/notes') { openPanel(P.openNotes(readNotes(info, mission))); return; }
    if (name === '/org') { openPanel(P.openOrg()); loadOrg(); return; }
    if (name === '/pin') {
      const arg = line.slice(name.length).trim();
      const last = [...stateRef.current.blocks].reverse().find((b) => b.kind === 'reply');
      const text = arg || last?.text;
      if (!text) { note('nothing to pin yet: /pin keeps the last reply, or /pin <text>'); return; }
      try { pin(info, mission, arg ? 'typed by the operator' : `${info.roles[last!.role ?? '']?.name ?? last!.role} (${last!.role})`, text); note('pinned to the mission notebook (/notes)', 'ok'); }
      catch (e: any) { note('could not pin: ' + e.message, 'error'); }
      return;
    }
    if (name === '/connectors') {
      if (!info.delphi_exe) { note('this console was not told where delphi is; run `delphi connectors status`'); return; }
      execFile(info.delphi_exe, ['connectors', 'status', '--workspace', info.workspace_name ?? '', '--home', info.home ?? ''], { timeout: 15000 }, (err, out, errOut) => note((out || errOut || String(err?.message ?? 'no output')).trimEnd()));
      return;
    }
    // A command may have changed what the workspace can run on (`/ratify apply`), so ask again afterwards.
    void runSlash(line, { cmd, mission, info, state: () => stateRef.current, post: note }).then(checkModels);
  };

  // The left deck is the knowledge window (`archives.ts`), reading through `kb.get` as the narrator's does. The ref moves
  // at once, not when React gets to the update: text and Enter can arrive in one read.
  const archRef = useRef(arch);
  archRef.current = arch;
  const setArchOf = (f: (a: A.Arch) => A.Arch) => { const n = f(archRef.current); archRef.current = n; setArchState(n); };
  const setArch = setArchOf;
  const kb = (target: string): Promise<{ status: number; body: any }> => cmd.call('kb.get', { target });
  const enc = encodeURIComponent;
  const loadShelf = () => Promise.all([kb(`/v1/collections?${A.trustParam(archRef.current).slice(1)}`), kb(`/v1/documents?limit=6${A.trustParam(archRef.current)}`), kb('/v1/health')])
    .then(([c, d, h]) => setArch((a) => A.gotShelf(a, c.body ?? [], d.body ?? [], h.body)))
    .catch(() => setArch((a) => A.withNote(a, 'the knowledge base is not available')));
  const setRdr = (f: (a: A.Arch) => A.Arch) => { const n = f(rdrRef.current); rdrRef.current = n; setRdrState(n); };
  const setLm = (f: (l: { focus: number | null; note: string } | null) => { focus: number | null; note: string } | null) => { const n = f(lmRef.current); lmRef.current = n; setLmState(n); };
  /** Pin a document or passage to the mission notebook with its pointer and a hash of what it said (`specs/029` §6): the one write the reader makes. */
  const pinTarget = (target: Extract<Target, { k: 'doc' | 'pointer' }>) => {
    const say = (msg: string, tone?: 'ok' | 'error') => { setRdr((a) => A.withNote(a, msg)); setLm((l) => (l ? { ...l, note: msg } : l)); if (tone) note(msg, tone); };
    kb(`/v1/doc?path=${enc(target.path)}`).then((r) => {
      if (r.status !== 200) { say(`could not pin: ${target.path} could not be read`, 'error'); return; }
      const p = passageOf(String(r.body?.text ?? ''), target);
      pin(info, mission, p.from, p.text);
      say('pinned to the mission notebook (/notes)', 'ok');
    }).catch((er) => say('could not pin: ' + (er?.message ?? 'the knowledge base is not available'), 'error'));
  };
  const runKb = (act: A.Action, on: 'arch' | 'rdr' = 'arch') => {
    const setArch = on === 'arch' ? setArchOf : setRdr;
    const fail = (msg: string) => { setArch((a) => A.withNote(a, msg)); if (on === 'rdr') setLm((l) => (l ? { ...l, note: msg } : l)); };
    const a0 = on === 'arch' ? archRef.current : rdrRef.current, tp = A.trustParam(a0);
    if (act.t === 'reload') { void loadShelf(); if (a0.searched && a0.q.trim() && !a0.browse) runKb({ t: 'search', q: a0.q.trim() }, on); else if (a0.browse) runKb({ t: 'browse', name: a0.browse }, on); }
    else if (act.t === 'search') kb(`/v1/search?q=${enc(act.q)}&limit=${A.RESULTS}${a0.scope ? `&collection=${enc(a0.scope)}` : ''}${tp}`).then((r) => setArch((a) => (r.status === 200 ? A.gotHits(a, act.q, r.body) : A.withNote(a, r.body?.error ?? 'the search failed')))).catch(() => fail('the search failed'));
    else if (act.t === 'browse') kb(`/v1/documents?collection=${enc(act.name)}&limit=50${tp}`).then((r) => setArch((a) => A.gotBrowse(a, act.name, r.body ?? []))).catch(() => fail('could not list that collection'));
    else if (act.t === 'open') {
      const t = act.target;
      kb(`/v1/doc?path=${enc(t.path)}${tp}`).then((r) => {
        if (r.status === 200 && on === 'rdr') { // a document the operator read from the chat is a source of this research (§6)
          setLm((l) => (l ? { ...l, note: '' } : l));
          setState((st) => ({ ...st, sources: noteSources(st.sources, [t.path], 'read', 'operator', Date.now()) }));
        }
        setArch((a) => (r.status === 200 ? A.gotDoc(a, r.body, t, sizeRef.current.w) : A.withNote(a, r.status === 404 ? `${t.path} is not in the knowledge base` : r.body?.error ?? 'could not open it')));
        if (r.status !== 200 && on === 'rdr') fail(r.status === 404 ? `${t.path} is not in the knowledge base` : r.body?.error ?? 'could not open it');
      }).catch(() => fail('could not open it'));
    } else if (act.t === 'pin') pinTarget(act.target);
    else if (act.t === 'browser') fail(openExternal(act.url) ? `opened ${act.url} in your browser` : 'could not open the browser');
    else if (act.t === 'copy') { copyToClipboard(act.text); fail('copied'); }
  };
  useEffect(() => { if (deck === 0 && !archRef.current.shelf) void loadShelf(); }, [deck]);

  const openPanel = (p: P.Panel | null) => { panelRef.current = p; setPanel(p); if (p) { deckRef.current = 1; setDeck(1); } };
  // The organization deck: `org.view` is a read; `org.action` is the operator's own add, update or remove, which runs
  // the `delphi bundle` command and returns what it printed. The deck is re-read afterwards. No card is involved.
  const onOrg = (f: (cur: O.OrgPanel) => O.OrgPanel) => { const cur = panelRef.current; if (cur?.kind === 'org') openPanel(f(cur)); };
  const loadOrg = () => {
    cmd.call('org.view', {}).then((view: any) => onOrg((cur) => ({ ...cur, view, error: '' })))
      .catch((e) => onOrg((cur) => ({ ...cur, error: String(e?.message ?? JSON.stringify(e)) })));
  };
  const runOrg = (body: { action: O.Verb; path?: string; name?: string }) => {
    const target = body.action === 'add' ? O.resolveDir(body.path ?? '', info.cwd, process.env.HOME) : body.name ?? '';
    cmd.call('org.action', { mission_id: mission, ...body, ...(body.action === 'add' ? { path: target } : {}) })
      .then((r: any) => onOrg((cur) => ({ ...cur, busy: false, result: O.resultOf(body.action, target, r) })))
      .catch((e) => onOrg((cur) => ({ ...cur, busy: false, result: O.resultOf(body.action, target, { ok: false, output: String(e?.message ?? JSON.stringify(e)) }) })))
      .finally(loadOrg);
  };
  // `org.trust` is the console's own act and asks twice: the first call returns the card (what the act does, and the word to
  // type); the typed word goes in the second. An agent or the narrator has no way to this call.
  const runTrust = (body: O.TrustBody) => {
    cmd.call('org.trust', { mission_id: mission, ...body })
      .then((r: any) => {
        if (r?.needs_confirm) onOrg((cur) => ({ ...cur, busy: false, asking: 'confirm', input: '', card: { body, text: String(r.text ?? ''), expect: String(r.expect ?? ''), what: String(r.what ?? 'the word shown') } }));
        else onOrg((cur) => ({ ...cur, busy: false, card: null, result: { title: `${r?.ok ? '' : 'did not '}${body.action} ${body.name}${r?.ok ? ': done' : ''}`, ok: !!r?.ok, lines: [String(r?.message ?? '(no output)')] } }));
      })
      .catch((e) => onOrg((cur) => ({ ...cur, busy: false, card: null, result: { title: `did not ${body.action} ${body.name}`, ok: false, lines: [String(e?.message ?? JSON.stringify(e))] } })))
      .finally(loadOrg);
  };
  const doSearch = (q: string) => {
    cmd.call('kb.search', { query: q }).then((r: any) => {
      const hits: P.Hit[] = (Array.isArray(r) ? r : r?.hits ?? []).map((h: any) => ({ pointer: String(h.pointer), heading: String(h.heading ?? ''), preview: String(h.preview ?? '') }));
      const cur = panelRef.current;
      if (cur?.kind !== 'search') return;
      openPanel({ ...cur, editing: false, hits, sel: 0, note: hits.length ? '' : 'no matches', lines: hits[0] ? previewOf(info, hits[0].pointer.split('#')[0]!) : [] });
    }).catch((e) => { const cur = panelRef.current; if (cur?.kind === 'search') openPanel({ ...cur, editing: false, note: String(e?.message ?? JSON.stringify(e)) }); });
  };

  useEffect(() => {
    const menuNow = (): Completion | null => {
      if (searchRef.current) return null;
      const c = comp.current;
      return completions(c.chars, c.cur, info, listDir);
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

      // The help overlay (chat deck): `?`, Esc close it; paging does nothing; any other key closes it and is then handled.
      const hk = helpKey(help, e, comp.current.chars.length === 0, deckRef.current === 1);
      if (hk === 'open') { setHelp(true); return; }
      if (hk === 'close') { setHelp(false); return; }
      if (hk === 'ignore') return;
      if (hk === 'close-and-pass') setHelp(false);

      // Shift+Tab cycles the approval mode from anywhere.
      if (e.t === 'key' && e.k === 'shift-tab') {
        const next = nextMode(modeRef.current);
        const set = (m: string) => cmd.call('permissions.set', { mode: m }).then((v: any) => { const got = String(v?.mode ?? m); setMode(got); note(modeNote(got)); });
        // Optimistic needs the OS sandbox; without one the cycle skips it and wraps to default.
        set(next).catch((er) => (next === 'optimistic' ? set('default') : Promise.reject(er))).catch((er) => note('could not change the approval mode: ' + (er?.message ?? JSON.stringify(er)), 'error'));
        return;
      }

      // Ctrl+K and Ctrl+S open the research panels from the chat deck.
      if (!panelRef.current && deckRef.current === 1 && e.t === 'ctrl' && (e.c === 'k' || e.c === 's')) { openPanel(e.c === 'k' ? P.openSearch() : P.openSources()); return; }
      // An open panel owns the keyboard until it is closed.
      if (panelRef.current && deckRef.current === 1 && !(e.t === 'key' && e.shift && (e.k === 'left' || e.k === 'right')) && e.t !== 'wheel' && e.t !== 'bg') {
        const { panel: next0, action } = P.key({ ...panelRef.current, flash: undefined } as P.Panel, e, stateRef.current.sources);
        const next = next0;
        panelRef.current = next; setPanel(next);
        if (action.t === 'close') openPanel(null);
        else if (action.t === 'search') doSearch(action.q);
        else if (action.t === 'org') { if (action.action.t === 'run') runOrg(action.action.body); else if (action.action.t === 'trust') runTrust(action.action.body); else loadOrg(); }
        else if (action.t === 'load') { const cur = panelRef.current; if (cur && (cur.kind === 'search' || cur.kind === 'sources')) openPanel({ ...cur, lines: previewOf(info, action.path) }); }
        else if (action.t === 'insert') { openPanel(null); upd((c) => C.insert(c, action.text)); }
        else if (action.t === 'pin') { try { pin(info, mission, action.from, action.text); note('pinned to the mission notebook (/notes)', 'ok'); openPanel({ ...(panelRef.current as P.Panel), flash: 'pinned to the mission notebook (/notes)' }); } catch (er: any) { openPanel({ ...(panelRef.current as P.Panel), flash: 'could not pin: ' + er.message }); } }
        return;
      }

      // Following links in the chat (`specs/029`): a document opened from a link takes the keys until it is closed; link mode
      // takes them until Esc, so a stray Enter or letter opens and types nothing. Ctrl+C, the wheel, the page keys and the deck keys still work.
      if (deckRef.current === 1 && !panelRef.current && e.t !== 'bg' && !(e.t === 'ctrl' && e.c === 'c') && !(e.t === 'key' && e.shift && (e.k === 'left' || e.k === 'right'))) {
        const { w, h } = sizeRef.current, dense = h < 16;
        if (rdrRef.current.at >= 0) {
          const r = A.key(rdrRef.current, e, w, Math.max(1, h - 7));
          if (r.used) { setRdr(() => r.a); runKb(r.action, 'rdr'); }
          return;
        }
        if (!lmRef.current && e.t === 'ctrl' && e.c === 'o') {
          const { lines, links } = chatLinks(chatModel(), w, dense);
          setLm(() => ({ focus: null, note: '' })); // nothing is focused yet, so a stray Enter opens nothing
          return;
        }
        if (lmRef.current) {
          const lmNow = lmRef.current, key = e.t === 'key' ? e.k : e.t === 'text' ? e.s : '';
          const { lines, links } = chatLinks(chatModel(), w, dense);
          const cur = lmNow.focus === null ? undefined : links.find((l) => l.id === lmNow.focus);
          const move = (dir: 1 | -1) => {
            const bh = bodyHeight({ ...chatModel(), composer: C.empty() }, w, h);
            // From nothing, the first link on screen; after that, the next or previous.
            const f = lmNow.focus === null ? firstVisible(links, Math.max(0, lines.length - bh - backRef.current), bh) ?? stepFocus(links, null, dir) : stepFocus(links, lmNow.focus, dir), l = links.find((x) => x.id === f);
            if (l) { const b = backShowing(lines.length, l.spans[0]!.line, bh, backRef.current); backRef.current = b; setBack(b); }
            setLm(() => ({ focus: f, note: '' }));
          };
          if (e.t === 'wheel') { scroll(e.dir === 'up' ? 3 : -3); return; }
          if (key === 'pgup' || key === 'pgdn') { scroll(key === 'pgup' ? Math.max(3, h - 10) : -Math.max(3, h - 10)); return; }
          if (key === 'esc') { setLm(() => null); return; }
          if (key === 'tab') { move(1); return; }
          if (key === 'shift-tab') { move(-1); return; }
          if (cur && key === 'enter') {
            if (cur.target.k === 'external') runKb({ t: 'browser', url: cur.target.url }, 'rdr');
            else { setLm((l) => (l ? { ...l, note: 'opening…' } : l)); runKb({ t: 'open', target: cur.target }, 'rdr'); }
          } else if (cur && key === 'y') runKb({ t: 'copy', text: copyText(cur.target) }, 'rdr');
          else if (cur && key === 'p' && cur.target.k !== 'external') pinTarget(cur.target);
          return;
        }
      }

      // On the left deck the keyboard belongs to the knowledge window; ← → (and shift+← →) it leaves to the deck keys.
      if (deckRef.current === 0 && !(e.t === 'ctrl' && e.c === 'c') && e.t !== 'bg') {
        const r = A.key(archRef.current, e, sizeRef.current.w, Math.max(1, sizeRef.current.h - 7));
        if (r.used) { setArch(() => r.a); runKb(r.action); return; }
      }

      // On the right deck the keyboard belongs to the inbox (an explicit focus change), never the prompt.
      if (deckRef.current === 2 && !(e.t === 'key' && e.shift && (e.k === 'left' || e.k === 'right')) && !(e.t === 'ctrl' && e.c === 'c')) {
        const now = Date.now();
        const { inbox: next, action } = I.key(inboxRef.current, I.cards(stateRef.current), e, now);
        inboxRef.current = next; setInbox(next);
        if (action.t === 'chat') { setDeck(1); deckRef.current = 1; }
        else if (action.t === 'resolve') void resolveCard(cmd, action.card, action.decision).then((bad) => bad && note(bad, 'error'));
        else if (action.t === 'resolve-all') for (const c of action.cards) void resolveCard(cmd, c, 'allow').then((bad) => bad && note(bad, 'error'));
        return;
      }

      const m = menuNow();
      const items = m?.items ?? [];
      switch (e.t) {
        case 'bg': setPal((p) => ({ dark: themeForced ? p.dark : e.dark, bg: e.rgb })); return;
        case 'wheel': scroll(e.dir === 'up' ? 3 : -3); return;
        case 'paste': follow(); upd((c) => C.insert(c, e.s)); return;
        case 'text':
          // Space pages down while lines wait below and the prompt is empty (`005e` §7.3); typing resumes following.
          if (e.s === ' ' && comp.current.chars.length === 0 && deckRef.current === 1 && backRef.current > 0) { scroll(-(bodyH - 1)); return; }
          follow(); upd((c) => C.insert(c, e.s)); return;
        case 'ctrl':
          if (e.c === 'c') {
            // Leave at once when nothing would be lost; otherwise ask once (`005e` §10.5).
            const busy = comp.current.chars.length > 0 || queueRef.current.length > 0 || Object.keys(stateRef.current.running).length > 0;
            const now = Date.now();
            if (!busy || now - quitAskedAt.current < 2000) { exit(); return; }
            quitAskedAt.current = now;
            note('press Ctrl+C again to quit' + (Object.keys(stateRef.current.running).length ? ' (the running task is left to the daemon)' : ''));
            return;
          }
          if (e.c === 'd') { if (comp.current.chars.length === 0) exit(); return; }
          if (e.c === 'j') { upd((c) => C.insert(c, '\n')); return; }
          if (e.c === 'a') { upd(C.home); return; }
          if (e.c === 'e') { upd(C.end); return; }
          if (e.c === 'u') { upd(C.killToStart); return; }
          if (e.c === 'k') { upd(C.killToEnd); return; }
          if (e.c === 'w') { upd(C.killWord); return; }
          if (e.c === 'l') { follow(); return; }
          if (e.c === 't') { expandRef.current = !expandRef.current; setExpandTools(expandRef.current); return; }
          if (e.c === 'r') { setS({ q: '', at: hist.length, match: null }); return; }
          return;
        case 'key': {
          const d = deckTarget(deckRef.current as Deck, e, comp.current.chars.length === 0);
          if (d !== null) {
            deckRef.current = d; setDeck(d);
            if (d === 2) { const fresh = I.newInbox(Date.now()); inboxRef.current = fresh; setInbox(fresh); }
            return;
          }
          const pick = (n: number) => { selRef.current = n; setSel(n); };
          const accept = (it: { insert: string }) => upd((c) => {
            const ins = graphemes(it.insert);
            const next = [...c.chars.slice(0, m!.from), ...ins, ...c.chars.slice(m!.to)];
            return { ...c, chars: next, cur: m!.from + ins.length, histAt: null };
          });
          switch (e.k) {
            case 'pgup': scroll(bodyH - 1); return;
            case 'pgdn': scroll(-(bodyH - 1)); return;
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
              if (e.alt || e.shift) { upd((c) => C.insert(c, '\n')); return; }
              const text = C.textOf(comp.current);
              if (items.length) {
                const it = items[Math.min(selRef.current, items.length - 1)]!;
                const tok = comp.current.chars.slice(m!.from, m!.to).join('');
                if (enterAccepts(tok, it, selRef.current)) { accept(it); return; }
              }
              if (!text && deckRef.current === 1 && backRef.current > 0) { follow(); return; } // Enter on an empty prompt jumps to the end
              const { text: t, next } = C.submit(comp.current);
              if (!t) return;
              comp.current = next; setComposer(next);
              if (t.startsWith('/') && lookup(t.split(/\s/)[0]!)) run(t);
              else if (t.startsWith('/')) note(`unknown command ${t.split(/\s/)[0]}; / shows the list`, 'error');
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

  // Scrolled up, new output must not move what is being read: grow the offset by what was added below. A reply that
  // begins while the operator is at the end is pinned by its label row once it outgrows the body (`005e` §7.2).
  const total = useRef(0);
  const base = { info, pal, state, now: 0, deck, composer, back: 0, help: false, menu: null, expandTools, search: null, queue, inbox, mode, panel, noModel };
  /** The chat model as the key handler sees it (it runs from refs, not from the last render). */
  function chatModel(): Model {
    return { info, pal: palRef.current, state: stateRef.current, now: 0, deck: 1, composer: comp.current, back: 0, help: false, menu: null, expandTools: expandRef.current, search: null, queue: queueRef.current, inbox: inboxRef.current, mode: modeRef.current, panel: null };
  }
  const dense = size.h < 16;
  const lineCount = useMemo(() => transcriptCount(base, size.w, dense), [state.blocks, state.live, size.w, dense, pal, expandTools, queue]);
  useEffect(() => {
    const added = lineCount - total.current;
    total.current = lineCount;
    const a = replyAnchor(base, size.w, dense);
    const id = a?.id ?? null, prev = lastReply.current;
    if (id !== prev) {
      lastReply.current = id;
      const sameReply = prev === 'live' && typeof id === 'number'; // the streamed text became its block
      if (a && !sameReply) anchorRef.current = backRef.current === 0 ? a.row : null;
    }
    if (added <= 0) return;
    const bodyH = bodyHeight({ ...base, composer: C.empty() }, size.w, size.h);
    setBack((b) => {
      const next = b > 0 ? b + added : anchorRef.current != null ? anchorBack(lineCount, anchorRef.current, bodyH) : 0;
      backRef.current = next;
      return next;
    });
  }, [lineCount]);

  const comps = search ? null : completions(composer.chars, composer.cur, info, listDir);
  const model: Model = { ...base, still, now, back, arch, links: lm ?? undefined, reader: rdr, help, menu: comps ? { items: comps.items, sel } : null, search: search ? { q: search.q, match: search.match } : null };
  const lines = useMemo(() => frame(model, size.w, size.h), [state, now, deck, composer, back, arch, lm, rdr, sel, help, size, pal, expandTools, queue, search, inbox, mode, panel, noModel, still]);
  const shown = useSelection(term, lines);
  return (
    <Box flexDirection="column" height={size.h} width={size.w}>
      <Text>{shown.join('\n')}</Text>
    </Box>
  );
}
