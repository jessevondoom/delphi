// The encyclopedia as the daemon's eleven reads answer it (spec 117 §3), and what the screens make of it (§4, §6). Pure:
// nothing here draws, reads a file or asks the daemon. Everything in these shapes is data a program or a model wrote; a
// screen shows it and never acts on it.

import { blockText, parse } from './md.ts';

/** The reads the window may name, as `app/src-tauri/src/ency.rs` lists them. */
export type EncyRead = 'pages.list' | 'pages.get' | 'pages.versions' | 'pages.links' | 'topics.list' | 'topics.get' | 'library.source' | 'library.claim' | 'memory.roles' | 'memory.role' | 'changes.since';

export type KeyTerm = { term: string; definition: string; cites: string[] };
/** A knowledge-base passage or a finding a topic page cites (`[s-kb-…]`, `[f-…]`). */
export type KbRecord = { id: string; kind: string; title: string; locator: string; origin: string; status: string; text: string; rel_path: string; start_line?: number };
/** One topic as `pages.list` has it. */
export type Topic = {
  slug: string; title: string; origin?: { kind?: string; topic_id?: string }; research_topics?: string[]; created?: number;
  /** When the current version was built (Unix seconds); null before the first build. `pages.get` sends a yes or no here instead. */
  built: number | boolean | null; version: number; change: string | null; outcome?: string | null; valid_until?: number | null; valid_until_date?: string | null;
  cited?: string[]; due: string | null;
  key_terms?: KeyTerm[]; start_here?: { cites: string[]; why: string }[]; see_also?: string[]; open_questions?: string[]; links?: string[]; records?: KbRecord[];
};
export type TopicPage = Topic & { markdown: string; version_shown: string | null };
export type Version = { version: string; built: number | null; number: number | null; change: string | null; outcome: string | null };
export type Links = { slug: string; links_here: string[]; links_out: string[]; exists: boolean };
/** One research topic as `topics.list` has it. */
export type Research = { id: string; question: string; state: string; project: string | null; private: boolean; updated?: string; created?: string; runs: number; claims: number; gaps?: unknown; page: string | null; label: string };
export type ResearchRecord = { id: string; record: Record<string, unknown>; answer: string | null; page: string | null; label: string };
export type Anchor = { source_id: string; locator?: string | null; quote?: string | null; state?: string; via?: string };
export type Claim = { id: string; text: string; kind?: string; scope?: string; created?: string; last_confirmed?: string; contested_by?: unknown[]; anchors: Anchor[]; origin?: string; project?: string; valid_until?: string; supersedes?: string; superseded_by?: string | null;
  /** The daemon's count behind the status: independent origins, how many of them trusted sources, and whether something contradicts it. */
  support?: { origins?: number; trusted_sources?: number; contradicted?: boolean } };
export type ClaimView = { claim: Claim; status: string; shown: string; sources: { id: string; title: string | null; private: boolean }[] };
/** An anchor into a source that no longer resolves (spec 142 §2.3 item 3): its own quote, the version and date it last resolved in, and why. */
export type Orphan = { claim: string; quote: string; version: string; date: string; why: string };
export type SourceView = { id: string; label: string; meta: Record<string, unknown>; private: boolean; capture_level?: string | null; status?: string | null; text: string; truncated: boolean; assets: string[]; passage_only?: boolean; orphans?: Orphan[] };
export type Changes = { since: number; pages: { slug: string; title: string; built: number | null; version: number; change: string | null }[]; research: { id: string; question: string; updated: string; state: string }[]; findings: FindingRow[]; notes: Note[] };
export type RoleCount = { role: string; lesson_files: number; notes: number; findings: number };
export type Note = { id: string; role: string; mission?: string; project?: string | null; kind: string; created: string; valid_until?: string | null; uses: number; tier: string; source?: string; text: string; label: string };
export type FindingRow = { id: string; rel_path: string; title: string | null; role: string | null; author?: string | null; status?: string | null; finding_status?: string | null; timestamp: string | null; source_type?: string | null; label: string; text?: string };
export type RoleMemory = { role: string; lessons: { from: string; text: string }[]; notes: Note[]; findings: FindingRow[] };

// ── Citation markers ─────────────────────────────────────────────────────────────────────────────────────────────────────

/** The status words a marker may carry, as the daemon writes them (`library_page_check::SHOWN`). */
export const SHOWN = ['● validated', '○ not validated', '◆ not validated', '◐ not validated', '✕ disproved', '○ vetted'];

/** One record a marker names: its label (`c-…` a claim, `s-…` a source, `s-kb-…` a passage, `f-…` a finding) and the status it printed. */
export type Cite = { label: string; shown?: string };

/** What kind of record a label names. */
export function kindOf(label: string): 'claim' | 'source' | 'passage' | 'finding' {
  if (/^s-kb-[0-9a-f]{16}$/.test(label)) return 'passage';
  if (/^f-[0-9a-f]{16}$/.test(label)) return 'finding';
  return label.startsWith('s-') ? 'source' : 'claim';
}

/** The records one bracketed token names, or null when it is not a marker. The daemon's rule (`library_page_check::marker`). */
export function marker(tok: string): Cite[] | null {
  if (!tok.startsWith('[') || !tok.endsWith(']')) return null;
  const out: Cite[] = [];
  for (const raw of tok.slice(1, -1).split(/[,;]/)) {
    const seg = raw.trim();
    const sp = seg.indexOf(' ');
    const [id, rest] = sp < 0 ? [seg, ''] : [seg.slice(0, sp), seg.slice(sp + 1).trim()];
    const bare = id.startsWith('s-') ? id.slice(2) : id.startsWith('c-') || id.startsWith('f-') ? id : null;
    if (bare === null || !/^[A-Za-z0-9-]+$/.test(bare) || !(rest === '' || SHOWN.includes(rest))) return null;
    out.push(rest ? { label: id, shown: rest } : { label: id });
  }
  return out;
}

export type Piece = { t: 'text'; s: string } | { t: 'cite'; cites: Cite[]; raw: string };

/** A run of text as its words and its citation markers, in order. A bracket followed by `(` is a link, not a marker. */
export function pieces(text: string): Piece[] {
  const out: Piece[] = [];
  let at = 0, i = 0;
  const push = (s: string) => { if (s) out.push({ t: 'text', s }); };
  while (i < text.length) {
    const start = text.indexOf('[', i);
    if (start < 0) break;
    const end = text.indexOf(']', start);
    if (end < 0) break;
    const tok = text.slice(start, end + 1);
    const cites = !tok.slice(1, -1).includes('[') && text[end + 1] !== '(' ? marker(tok) : null;
    if (cites) { push(text.slice(at, start)); out.push({ t: 'cite', cites, raw: tok }); at = i = end + 1; } else i = start + 1;
  }
  push(text.slice(at));
  return out;
}

/** Every record the text cites, once each, in order. */
export function cited(text: string): string[] {
  const seen: string[] = [];
  for (const p of pieces(text)) if (p.t === 'cite') for (const c of p.cites) if (!seen.includes(c.label)) seen.push(c.label);
  return seen;
}

/** The text without its markers, its spaces folded: what a passage says, whatever it cites. */
export function unmarked(text: string): string {
  return pieces(text).filter((p) => p.t === 'text').map((p) => (p as { s: string }).s).join('').replace(/\s+/g, ' ').replace(/\s+([.,;:!?])/g, '$1').trim();
}

// ── A page's parts ───────────────────────────────────────────────────────────────────────────────────────────────────────

/** The anchor a heading makes: the daemon's rule (`library_page_check::slug_of`), which the page's own `#section` links follow. */
export function anchorOf(heading: string): string {
  let out = '';
  for (const ch of heading.toLowerCase()) out += /[\p{L}\p{N}]/u.test(ch) ? ch : ch === ' ' || ch === '-' ? '-' : '';
  return out;
}

export const BIBLIOGRAPHY = 'Bibliography and citations';
const UNDER = ['Key terms', 'Start here', 'See also', 'Open questions', 'Contents', BIBLIOGRAPHY];

export type BibRow = { label: string; status: string; title: string; origin: string; date: string; locator: string; citation: string };
export type PageParts = {
  /** The daemon's "Generated page. Not authoritative. …" line. */
  banner: string;
  /** The page's own `# title`, when it wrote one. */
  title: string;
  /** What comes before the first section. */
  lead: string;
  /** The page's sections, in order, each with its heading line; the named sections below are taken out. */
  sections: { heading: string; anchor: string; text: string }[];
  /** `## Key terms`, `## Start here`, `## See also`, `## Open questions`: each one's Markdown, when the page has it. */
  named: Record<string, string>;
  bibliography: BibRow[];
};

const cells = (line: string): string[] => line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim());

/** A page as `pages.get` sends it, taken apart for the topic screen (spec 117 §4 item 2). Nothing is rewritten: every part is the page's own text. */
export function parts(markdown: string): PageParts {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const segs: { heading: string | null; lines: string[] }[] = [{ heading: null, lines: [] }];
  let fence = false;
  for (const l of lines) {
    if (/^ {0,3}(```|~~~)/.test(l)) fence = !fence;
    const h = fence ? null : /^##\s+(.+?)\s*#*\s*$/.exec(l);
    if (h) segs.push({ heading: h[1]!, lines: [] });
    else segs[segs.length - 1]!.lines.push(l);
  }
  const head = segs[0]!.lines;
  const b = head.findIndex((l) => l.replace(/^>\s*/, '').startsWith('Generated page.'));
  const banner = b < 0 ? '' : head[b]!.replace(/^>\s*/, '').trim();
  const t = head.findIndex((l) => /^#\s+\S/.test(l));
  const title = t < 0 ? '' : head[t]!.replace(/^#\s+/, '').replace(/\s*#*\s*$/, '');
  const lead = head.filter((_, i) => i !== b && i !== t).join('\n').trim();
  const named: Record<string, string> = {};
  const sections: PageParts['sections'] = [];
  let bibliography: BibRow[] = [];
  for (const s of segs.slice(1)) {
    const text = s.lines.join('\n').trim();
    if (s.heading === BIBLIOGRAPHY) {
      bibliography = s.lines.filter((l) => l.trim().startsWith('|')).map(cells).filter((c) => !c.every((x) => /^:?-+:?$/.test(x)) && c[0] !== 'Id' && c[0] !== 'no sources cited')
        .map((c) => ({ label: c[0] ?? '', status: c[1] ?? '', title: c[2] ?? '', origin: c[3] ?? '', date: c[4] ?? '', locator: c[5] ?? '', citation: c[6] ?? '' }));
    } else if (UNDER.includes(s.heading!)) named[s.heading!] = text;
    else sections.push({ heading: s.heading!, anchor: anchorOf(s.heading!), text });
  }
  return { banner, title, lead, sections, named, bibliography };
}

/** The contents of a page: its sections and the headings one level under them, each with the anchor a link to it carries. */
export function contents(p: PageParts): { title: string; anchor: string; depth: 0 | 1 }[] {
  const out: { title: string; anchor: string; depth: 0 | 1 }[] = [];
  for (const s of p.sections) {
    out.push({ title: s.heading, anchor: s.anchor, depth: 0 });
    let fence = false;
    for (const l of s.text.split('\n')) {
      if (/^ {0,3}(```|~~~)/.test(l)) fence = !fence;
      const h = fence ? null : /^###\s+(.+?)\s*#*\s*$/.exec(l);
      if (h) out.push({ title: h[1]!, anchor: anchorOf(h[1]!), depth: 1 });
    }
  }
  return out;
}

/** The prose passages of a page, in order (its lead, then each section's paragraphs, lists and quotes): what a note can be attached to. */
export function passages(p: PageParts): string[] {
  return [p.lead, ...p.sections.map((s) => s.text)].flatMap((text) => parse(text).map(blockText)).filter((t) => unmarked(t) !== '');
}

/**
 * What a claim's status means, in a sentence: the daemon's derived status (`knowledge_status::Status`) put in words. The
 * status is the daemon's; this only says what each one is, and never works one out.
 */
export function statusWhy(status: string, c: Pick<Claim, 'contested_by' | 'anchors' | 'superseded_by' | 'support'>): string {
  const anchors = c.anchors.length === 1 ? '1 anchor in a source' : `${c.anchors.length} anchors in sources`;
  const said = ((): string => {
    switch (status) {
      case 'noted': return `Noted: recorded with ${anchors}, and not yet confirmed by independent origins or the operator.`;
      case 'corroborated': return 'Corroborated: three or more independent origins state it. A count made by code; it still reads not validated.';
      case 'substantiated': return 'Substantiated: three or more independent origins state it, at least one of them a trusted source. It still reads not validated.';
      case 'vetted': return 'Vetted: five or more independent origins, two of them trusted, and the validator would ratify it. It waits for the operator.';
      case 'checking': return 'Checking: its sources are being checked again.';
      case 'validated': return 'Validated: it was vouched for, and its record says by whom.';
      case 'contested': return 'Contested: someone said it is wrong. It reads not validated until the contest is resolved.';
      case 'disproved': return 'Disproved: it was checked and found wrong. Pages do not cite it.';
      case 'retired': return 'Retired: it was withdrawn and is kept for the record.';
      default: return status ? `${status.charAt(0).toUpperCase()}${status.slice(1)}.` : '';
    }
  })();
  const contests = c.contested_by?.length ?? 0;
  const o = c.support?.origins;
  const counted = typeof o === 'number' ? `The daemon counts ${o} independent ${o === 1 ? 'origin' : 'origins'}${c.support?.trusted_sources ? `, ${c.support.trusted_sources} of them trusted` : ''}${c.support?.contradicted ? ', and something contradicts it' : ''}.` : '';
  return [said, counted, contests && status !== 'contested' ? `${contests} ${contests === 1 ? 'dispute is' : 'disputes are'} on its record.` : '', c.superseded_by ? 'A later claim supersedes it.' : ''].filter(Boolean).join(' ');
}

/** One of a record's values as words to show: text as it is, a number, a list joined; nothing for anything else. */
export function shownValue(v: unknown): string {
  if (typeof v === 'string') return v.trim();
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) return v.map(shownValue).filter(Boolean).join(', ');
  return '';
}

/** A source's authors as its record names them (CSL names), or who holds its rights, or where it came from. */
export function authorOf(meta: Record<string, unknown>): string {
  const names = (Array.isArray(meta.author) ? meta.author : []).map((a) => {
    const n = a as { literal?: string; family?: string; given?: string };
    return n?.literal ?? (n?.family ? `${n.given ? `${n.given} ` : ''}${n.family}` : '');
  }).filter(Boolean);
  return names.length ? names.join('; ') : shownValue(meta.rights_holder) || shownValue(meta.origin);
}

/** How the library keeps a source (spec 117 §4 item 3), from its record's `capture_level` and whether it is kept privately. */
export function keptWords(v: Pick<SourceView, 'capture_level' | 'private' | 'meta' | 'passage_only'>): string {
  const level = v.capture_level ?? '';
  const how = level === 'full' ? 'its whole text is kept' : level === 'excerpts' ? 'the passages quoted from it are kept, not the whole text' : level === 'pointer' || level === 'metadata' ? 'only a pointer to it is kept, no text' : level ? `kept as ${level}` : 'not said';
  const purged = v.meta.purged ? '; its text was purged' : '';
  const privately = typeof v.meta.kept_privately === 'string' ? '; kept privately for retrieval, so only the cited passage is shown here' : v.private ? '; private: shown to you, never given to another session or agent' : '';
  return `${how}${purged}${privately}`;
}

/** The facts of a source's record a page shows first; whatever else the record holds is listed after them by its own names. */
export function sourceFacts(v: SourceView): { label: string; value: string }[] {
  const m = v.meta;
  const day = (x: unknown): string => shownValue(x).slice(0, 10);
  return [
    { label: 'Author or origin', value: authorOf(m) },
    { label: 'Status', value: shownValue(v.status) || shownValue(m.status) },
    { label: 'How it is kept', value: keptWords(v) },
    { label: 'Licence', value: shownValue(m.licence) || shownValue(m.license) },
    { label: 'Retrieved', value: day(m.retrieved) },
    { label: 'Last checked', value: day(m.last_checked) },
    { label: 'Address', value: shownValue(m.URL) || shownValue(m.url) },
  ].filter((f) => f.value);
}

/** When a topic's current version was built, in Unix seconds, or null: `pages.get` sends a yes or no in that field. */
export const builtAt = (t: Pick<Topic, 'built'>): number | null => (typeof t.built === 'number' ? t.built : null);

/** `2026-11-15` for Unix seconds; nothing for no time. */
export const dayOfUnix = (secs: number | null | undefined): string => (typeof secs === 'number' && Number.isFinite(secs) && secs > 0 ? new Date(secs * 1000).toISOString().slice(0, 10) : '');

// ── The home page's lists ────────────────────────────────────────────────────────────────────────────────────────────────

/** Topics under the letter their title starts with (`#` for anything else), each group and the groups in order. */
export function byLetter(topics: Topic[]): [string, Topic[]][] {
  const groups = new Map<string, Topic[]>();
  for (const t of [...topics].sort((a, b) => a.title.localeCompare(b.title))) {
    const first = t.title.trim().charAt(0).toUpperCase();
    const key = /^\p{L}$/u.test(first) ? first : '#';
    groups.set(key, [...(groups.get(key) ?? []), t]);
  }
  return [...groups.entries()].sort((a, b) => (a[0] === '#' ? 1 : b[0] === '#' ? -1 : a[0].localeCompare(b[0])));
}

/** What a topic with no project is filed under. */
export const NO_PROJECT = 'This workspace';

/** Topics under the project whose research they follow (a topic the operator named, or whose research has no project, is the workspace's). */
export function byProject(topics: Topic[], research: Research[]): [string, Topic[]][] {
  const project = new Map(research.map((r) => [r.id, r.project]));
  const groups = new Map<string, Topic[]>();
  for (const t of [...topics].sort((a, b) => a.title.localeCompare(b.title))) {
    const names = [...new Set((t.research_topics ?? []).map((id) => project.get(id)).filter((p): p is string => !!p))];
    for (const key of names.length ? names : [NO_PROJECT]) groups.set(key, [...(groups.get(key) ?? []), t]);
  }
  return [...groups.entries()].sort((a, b) => (a[0] === NO_PROJECT ? 1 : b[0] === NO_PROJECT ? -1 : a[0].localeCompare(b[0])));
}

/** Topics whose title, key terms or open questions hold every word of `q`. */
export function matching(topics: Topic[], q: string): Topic[] {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return topics;
  return topics.filter((t) => {
    const hay = [t.title, t.slug, ...(t.key_terms ?? []).map((k) => k.term), ...(t.open_questions ?? [])].join(' ').toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

// ── A quoted passage in a source ─────────────────────────────────────────────────────────────────────────────────────────

/**
 * Where `quote` is in `text`, as a range of `text`'s own characters, or null. Spaces and line ends are compared folded,
 * as the daemon compares a quotation with a stored text, so a quote that ran across a line end is still found.
 */
export function findQuote(text: string, quote: string): { from: number; to: number } | null {
  const want = quote.replace(/\s+/g, ' ').trim();
  if (!want) return null;
  // The text folded, with each folded character's place in the original.
  let folded = '';
  const place: number[] = [];
  let gap = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i]!;
    if (/\s/.test(ch)) { gap = folded.length > 0; continue; }
    if (gap) { folded += ' '; place.push(i - 1); gap = false; }
    folded += ch;
    place.push(i);
  }
  const at = folded.indexOf(want);
  if (at < 0) return null;
  return { from: place[at]!, to: place[at + want.length - 1]! + 1 };
}

// ── Asking from a page (spec 117 §6): commands for the clipboard ─────────────────────────────────────────────────────────

/** One argument for a POSIX shell, in single quotes: nothing in it is expanded, whatever the page or the operator wrote. */
export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** One line of at most `max` characters: control characters and line ends become spaces. */
export function oneLine(s: string, max: number): string {
  return Array.from(s.replace(/[\p{Cc}\p{Zl}\p{Zp}]+/gu, ' ').replace(/\s+/g, ' ').trim()).slice(0, max).join('');
}

const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const ID = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
/** A page of the encyclopedia. Each is read on a page of its own (spec 117 §4). */
export type Route =
  | { v: 'home' }
  | { v: 'topic'; slug: string; version?: string }
  | { v: 'source'; id: string; quote?: string }
  | { v: 'claim'; id: string }
  | { v: 'research'; id: string }
  | { v: 'changes' }
  | { v: 'doc'; path: string };

/**
 * A page named in the address (`?topic=tides`, `?topic=tides&version=…`, `?source=<id>&quote=…`, `?claim=<id>`,
 * `?research=<id>`, `?changes`): a link straight to it, for a browser and for looking at one page. Only a slug or an id
 * of the shapes the daemon gives; anything else opens the home page.
 */
export function routeInAddress(search: string): Route | null {
  const q = new URLSearchParams(search);
  // A source may be named by its label (`s-<id>`) or by its id.
  const id = (k: string): string => { const raw = (q.get(k) ?? '').trim(); const v = k === 'source' ? raw.replace(/^s-(?=.)/, '') : raw; return ID.test(v) && !v.includes('..') && v.length <= 120 ? v : ''; };
  const slug = (q.get('topic') ?? '').trim();
  if (SLUG.test(slug) && slug.length <= 60) { const version = id('version'); return version ? { v: 'topic', slug, version } : { v: 'topic', slug }; }
  if (id('source')) { const quote = (q.get('quote') ?? '').slice(0, 4000); return quote ? { v: 'source', id: id('source'), quote } : { v: 'source', id: id('source') }; }
  if (id('claim')) return { v: 'claim', id: id('claim') };
  if (id('research')) return { v: 'research', id: id('research') };
  return q.has('changes') ? { v: 'changes' } : null;
}

/** A role named in the address (`?role=scout`), for the agent memory screen. */
export function roleInAddress(search: string): string {
  const r = (new URLSearchParams(search).get('role') ?? '').trim();
  return SLUG.test(r) && r.length <= 40 ? r : '';
}

/** The most a `--note` for `delphi research continue` may hold (its help says so). */
export const NOTE_MAX = 120;

/** `delphi pages rebuild <slug>`, or nothing for a slug that is not one. */
export const rebuildCommand = (slug: string): string => (SLUG.test(slug) ? `delphi pages rebuild ${slug}` : '');

/** `delphi research continue <topic> --note '…'`: a follow-up question, or "research this section further". */
export function continueCommand(topicId: string, note: string): string {
  if (!ID.test(topicId)) return '';
  const n = oneLine(note, NOTE_MAX);
  return `delphi research continue ${topicId}${n ? ` --note ${shellQuote(n)}` : ''}`;
}

/** `delphi pages dispute <claim> --note '…'`. A dispute says what is wrong, so it needs a note. */
export function disputeCommand(claimId: string, note: string): string {
  const n = oneLine(note, 500);
  return ID.test(claimId) && n ? `delphi pages dispute ${claimId} --note ${shellQuote(n)}` : '';
}

/** The note that asks for one section to be researched further. */
export const sectionNote = (heading: string, more = ''): string => oneLine(`${more ? `${more} ` : ''}(section: ${heading})`, NOTE_MAX);

// ── Agent memory (spec 117 §4 item 4) ────────────────────────────────────────────────────────────────────────────────────

/** One thing a role learned, with the day it is dated. */
export type Learned = { day: string; role: string; kind: 'note' | 'finding' | 'episode' | 'lesson'; title: string; detail: string; path?: string };

const DAY = /\b(\d{4}-\d{2}-\d{2})\b/;

/** A role's lessons as lines: each bullet of each file, with the day it carries when it carries one. Lesson files hold no date of their own. */
export function lessonLines(lessons: RoleMemory['lessons']): { from: string; text: string; day: string }[] {
  const out: { from: string; text: string; day: string }[] = [];
  for (const f of lessons) for (const l of f.text.split('\n')) {
    const m = /^\s*[-*+]\s+(.*\S)\s*$/.exec(l);
    if (m) out.push({ from: f.from, text: m[1]!, day: DAY.exec(m[1]!)?.[1] ?? '' });
  }
  return out;
}

/** "Learned lately": a role's notes, findings, episodes and dated lessons in one list, newest first. Undated things are left out. */
export function learned(m: RoleMemory, episodes: { rel_path: string; updated_at: string }[] = []): Learned[] {
  const out: Learned[] = [];
  for (const n of m.notes) out.push({ day: n.created.slice(0, 10), role: m.role, kind: 'note', title: n.kind || 'note', detail: n.text });
  for (const f of m.findings) if (f.timestamp) out.push({ day: f.timestamp.slice(0, 10), role: m.role, kind: 'finding', title: f.title ?? f.id, detail: f.finding_status ?? f.status ?? '', path: f.rel_path });
  for (const e of episodes) if (/^\d{4}-\d{2}-\d{2}/.test(e.updated_at)) out.push({ day: e.updated_at.slice(0, 10), role: m.role, kind: 'episode', title: e.rel_path.split('/').pop()?.replace(/\.md$/, '').replace(/[-_]+/g, ' ') ?? e.rel_path, detail: '', path: e.rel_path });
  for (const l of lessonLines(m.lessons)) if (l.day) out.push({ day: l.day, role: m.role, kind: 'lesson', title: l.from, detail: l.text });
  return out.filter((x) => /^\d{4}-\d{2}-\d{2}$/.test(x.day)).sort((a, b) => b.day.localeCompare(a.day) || a.role.localeCompare(b.role));
}

/** A dated list as its days, newest first, each with what happened on it. */
export function byDay<T extends { day: string }>(rows: T[]): [string, T[]][] {
  const days = new Map<string, T[]>();
  for (const r of rows) days.set(r.day, [...(days.get(r.day) ?? []), r]);
  return [...days.entries()].sort((a, b) => b[0].localeCompare(a[0]));
}

// ── Changes (spec 117 §4 item 5) ─────────────────────────────────────────────────────────────────────────────────────────

export type ChangeRow = { day: string; kind: 'page' | 'research' | 'finding' | 'note'; title: string; detail: string; open?: { v: 'topic'; slug: string } | { v: 'research'; id: string } };

/** What `changes.since` answered, as one dated list, newest first. */
export function changeRows(c: Changes): ChangeRow[] {
  const out: ChangeRow[] = [];
  for (const p of c.pages) out.push({ day: dayOfUnix(p.built), kind: 'page', title: p.title, detail: p.version <= 1 ? 'first version' : `version ${p.version}: ${p.change ?? 'rebuilt'}`, open: { v: 'topic', slug: p.slug } });
  for (const r of c.research) out.push({ day: (r.updated ?? '').slice(0, 10), kind: 'research', title: r.question, detail: `research ${r.state}`, open: { v: 'research', id: r.id } });
  for (const f of c.findings) out.push({ day: (f.timestamp ?? '').slice(0, 10), kind: 'finding', title: f.title ?? f.id, detail: [f.role, f.finding_status ?? f.status].filter(Boolean).join(' · ') });
  for (const n of c.notes) out.push({ day: n.created.slice(0, 10), kind: 'note', title: n.text.length > 140 ? `${n.text.slice(0, 139)}…` : n.text, detail: n.label });
  return out.filter((r) => r.day).sort((a, b) => b.day.localeCompare(a.day));
}
