// The screen as data: a pure function from the console's model to exactly `h` lines of `w` columns. Ink only paints
// what this returns, so the whole look is testable without a terminal (see test/screen.test.ts).
import { renderMarkdown } from './markdown.ts';
import { layout, type Composer } from './composer.ts';
import { filter, type Command } from './commands.ts';
import { textOf } from './composer.ts';
import { status, type Block, type State } from './fold.ts';
import * as T from './theme.ts';
import chalk from 'chalk';

export type Model = {
  info: T.Info; pal: T.Pal; state: State; now: number; deck: 0 | 1 | 2;
  composer: Composer; back: number; menuSel: number; help: boolean;
};

const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout'];
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8);
const dots = (deck: number) => [0, 1, 2].map((i) => (i === deck ? T.accent('●') : T.dim('○'))).join(' ');
const nav = (deck: number) => T.dim('navigate: shift+arrows · ') + dots(deck);

export function header(m: Model, w: number): string[] {
  const st = status(m.state, m.now);
  let left: string;
  if (st.kind === 'alert') {
    left = T.badge(st.role, 'waiting on you', m.info, m.pal) + ' ' + T.crimson(chalk.bold(T.roleName(st.role, m.info) + ': decision needed')) + T.dim(` · ${String(Math.floor(st.secs / 60)).padStart(2, '0')}:${String(st.secs % 60).padStart(2, '0')} remaining`);
  } else if (st.kind === 'active') {
    left = T.badge(st.role, 'running', m.info, m.pal) + ' ' + T.roleColor(st.role, m.info).bold(T.roleName(st.role, m.info) + ':') + ' ' + T.text(m.pal)(st.doing || 'working');
  } else {
    const roles = ORDER.filter((r) => m.info.roles[r]);
    left = roles.map((r) => T.badge(r, m.state.agents[r]?.state ?? 'idle', m.info, m.pal)).join(' ') + T.dim('  all agents idle');
  }
  const l2 = chalk.bold(T.text(m.pal)('delphi')) + T.dim(` v${m.info.version} · ${T.tilde(m.info.cwd)}`);
  return [T.spread(left, nav(m.deck), w), T.clip(l2, w)];
}

export function footer(m: Model, w: number): string {
  const k = m.state.tokens >= 1000 ? (m.state.tokens / 1000).toFixed(1) + 'k' : String(m.state.tokens);
  const pending = Object.keys(m.state.approvals).length;
  const left = T.dim(`${k} tokens · approvals${pending ? ` · ${pending} waiting` : ''} · shortcuts: ?`);
  return T.spread(left, nav(m.deck), w);
}

function block(b: Block, m: Model, w: number): string[] {
  if (b.kind === 'user') {
    const lines = b.text.split('\n');
    return ['', ...lines.map((l, i) => (i === 0 ? T.dim('❯ ') : '  ') + T.text(m.pal)(l))];
  }
  if (b.kind === 'note') return ['', '  ' + T.dim(b.text)];
  const role = b.role ?? 'orchestrator';
  const head = T.badge(role, b.status === 'escalated' ? 'escalated' : 'idle', m.info, m.pal) + ' ' + T.roleColor(role, m.info).bold(T.roleName(role, m.info)) + T.dim(` · ${role} · ${clock(b.at)}`);
  return ['', head, ...renderMarkdown(b.text, w - 2).map((l) => '  ' + l)];
}

/** Every transcript line, oldest first. The unfinished reply is dim at the bottom. */
export function transcript(m: Model, w: number): string[] {
  const out = m.state.blocks.flatMap((b) => block(b, m, w));
  if (m.state.live) out.push('', ...renderMarkdown(m.state.live.slice(-3000), w - 2).map((l) => '  ' + chalk.dim(l)));
  return out;
}

const HELP = [
  'keys',
  '  enter send · alt+enter or ctrl+j newline · esc clear · ↑↓ history',
  '  pgup pgdn or the mouse wheel scroll · shift+← shift+→ move between decks',
  '  ctrl+a ctrl+e line start/end · ctrl+u ctrl+k ctrl+w delete · ctrl+c leave',
  '  / opens the command menu · tab or enter completes',
];

function deckBody(m: Model, w: number, h: number): string[] {
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  if (m.deck === 0) return pad(['', T.dim('  Activity and thinking stream'), '', ...(m.state.live ? renderMarkdown(m.state.live.slice(-2000), w - 2).map((l) => '  ' + chalk.dim(l)) : [T.dim('  nothing running')]), '', T.dim('  Reasoning and tool parameters belong here, never in the chat. Lands in a later phase.')]);
  const pend = Object.entries(m.state.approvals);
  return pad(['', T.dim('  Plan and decisions'), '', ...(pend.length ? pend.map(([id, a]) => `  ${T.badge(a.role, 'waiting on you', m.info, m.pal)} ${T.roleName(a.role, m.info)} · ${a.tool || 'approval'} ${T.dim('(' + id + ')')}`) : [T.dim('  nothing needs you')]), '', T.dim('  The decision inbox and plan progress land in a later phase.')]);
}

/** The whole screen: exactly `h` lines. */
export function frame(m: Model, w: number, h: number): string[] {
  const head = header(m, w);
  const text = textOf(m.composer);
  const menu: Command[] = text.startsWith('/') && !text.includes(' ') && !text.includes('\n') ? filter(text) : [];
  const lay = layout(m.composer, Math.max(8, w - 2));
  const promptRows = Math.min(lay.rows.length, 6);
  const first = Math.max(0, Math.min(lay.cursorRow - promptRows + 1, lay.rows.length - promptRows));
  const prompt = lay.rows.slice(first, first + promptRows).map((r, i) =>
    (first + i === 0 ? T.dim('❯ ') : '  ') + T.text(m.pal)(r.before) + (r.at ? chalk.inverse(r.at) : '') + T.text(m.pal)(r.after));
  const bodyH = Math.max(1, h - head.length - 1 /*blank*/ - 1 - prompt.length - 1 - 1);
  let body: string[];
  if (m.deck !== 1) body = deckBody(m, w, bodyH);
  else {
    const lines = m.help ? [...transcript(m, w), '', ...HELP.map(T.dim)] : transcript(m, w);
    const maxBack = Math.max(0, lines.length - bodyH);
    const back = Math.min(m.back, maxBack);
    body = lines.slice(Math.max(0, lines.length - bodyH - back), lines.length - back);
    while (body.length < bodyH) body.unshift('');
    if (back > 0) body[bodyH - 1] = T.spread('', chalk.hex('#facc15')(`↓ ${back} lines below · pgdn`), w);
    if (menu.length) {
      const shown = menu.slice(0, 4), sel = Math.min(m.menuSel, shown.length - 1);
      shown.forEach((c, i) => {
        const row = (i === sel ? T.accent('❯ ') : '  ') + (i === sel ? chalk.bold(T.link(c.name)) : T.link(c.name)) + T.dim(` command · ${c.desc}`);
        body[bodyH - shown.length + i] = T.clip(' ' + row, w);
      });
    }
  }
  const out = [...head, '', ...body.map((l) => T.clip(l, w)), T.hairline(m.pal, w), ...prompt.map((l) => T.clip(l, w)), T.hairline(m.pal, w), footer(m, w)];
  while (out.length < h) out.push('');
  return out.slice(0, h);
}
