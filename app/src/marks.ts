// The operator's own marks on the encyclopedia (spec 117 §5): which page versions were read to their end, notes on
// passages, pinned topics, and when the operator was last here. The app keeps them in its own folder
// (`<home>/app/marks.json`, through the bridge); they are never sent to the daemon and no agent is given them. Pure:
// every function takes marks and returns new marks.
import { unmarked, type Topic, type TopicPage } from './ency.ts';

/** A page version the operator scrolled to the end of. */
export type ReadMark = { version: number; at: string };
/** A note on a passage of a page. `passage` is the passage's own words (markers left out), which is how the note finds it again in a later version. */
export type Annotation = { id: string; slug: string; version: number; passage: string; text: string; at: string };
export type Marks = {
  v: 1;
  read: Record<string, ReadMark>;
  notes: Annotation[];
  pins: string[];
  /** When this sitting at the encyclopedia began, and when the one before it did: "since you were last here" is since `before`. */
  here?: string;
  before?: string;
};

export const EMPTY: Marks = { v: 1, read: {}, notes: [], pins: [] };
const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const NOTES_MAX = 2000;
export const NOTE_TEXT_MAX = 4000;
const PASSAGE_MAX = 2000;
/** A visit after this long away is a new sitting. */
export const SITTING_MS = 30 * 60 * 1000;

const text = (v: unknown, max: number): string => (typeof v === 'string' ? v.slice(0, max) : '');
const time = (v: unknown): string => (typeof v === 'string' && !Number.isNaN(Date.parse(v)) ? v : '');

/** Whatever was kept, made into valid marks: anything damaged or of another shape is dropped, never guessed at. */
export function parseMarks(raw: unknown): Marks {
  const o = raw && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  const read: Record<string, ReadMark> = {};
  if (o.read && typeof o.read === 'object') for (const [slug, r] of Object.entries(o.read as Record<string, unknown>)) {
    const version = (r as ReadMark | null)?.version;
    if (SLUG.test(slug) && Number.isInteger(version) && version! > 0) read[slug] = { version: version!, at: time((r as ReadMark).at) };
  }
  const notes: Annotation[] = [];
  if (Array.isArray(o.notes)) for (const n of o.notes.slice(0, NOTES_MAX) as Partial<Annotation>[]) {
    if (!n || typeof n !== 'object' || !SLUG.test(String(n.slug)) || !text(n.id, 80) || !text(n.passage, PASSAGE_MAX) || !text(n.text, NOTE_TEXT_MAX).trim()) continue;
    notes.push({ id: text(n.id, 80), slug: n.slug!, version: Number.isInteger(n.version) ? n.version! : 0, passage: text(n.passage, PASSAGE_MAX), text: text(n.text, NOTE_TEXT_MAX), at: time(n.at) });
  }
  const pins = Array.isArray(o.pins) ? [...new Set(o.pins.filter((p): p is string => typeof p === 'string' && SLUG.test(p)))].slice(0, 200) : [];
  const out: Marks = { v: 1, read, notes, pins };
  if (time(o.here)) out.here = time(o.here);
  if (time(o.before)) out.before = time(o.before);
  return out;
}

// ── Read state (§5 item 1) ───────────────────────────────────────────────────────────────────────────────────────────────

/** The operator reached the end of `version` of a page. An earlier version read later does not undo a later one. */
export function markRead(m: Marks, slug: string, version: number, now: string): Marks {
  if (!SLUG.test(slug) || !Number.isInteger(version) || version < 1 || (m.read[slug]?.version ?? 0) >= version) return m;
  return { ...m, read: { ...m.read, [slug]: { version, at: now } } };
}

/** `new` for a built page never read to its end, `changed` for one rebuilt since it was, nothing for one that is read or not built yet. */
export function fresh(m: Marks, t: Pick<Topic, 'slug' | 'version' | 'built'>): 'new' | 'changed' | null {
  if (!t.built || t.version < 1) return null;
  const read = m.read[t.slug]?.version ?? 0;
  return read === 0 ? 'new' : read < t.version ? 'changed' : null;
}

/** The topics that are new or changed since the operator read them, newest build first. */
export function newSince<T extends Pick<Topic, 'slug' | 'version' | 'built'>>(m: Marks, topics: T[]): { topic: T; state: 'new' | 'changed' }[] {
  return topics.map((topic) => ({ topic, state: fresh(m, topic) })).filter((x): x is { topic: T; state: 'new' | 'changed' } => x.state !== null)
    .sort((a, b) => (typeof b.topic.built === 'number' ? b.topic.built : 0) - (typeof a.topic.built === 'number' ? a.topic.built : 0));
}

/** The operator opened the encyclopedia at `now`. After a while away it is a new sitting, and the sitting before becomes "last here". */
export function arrive(m: Marks, now: string): Marks {
  if (m.here && Date.parse(now) - Date.parse(m.here) < SITTING_MS) return m;
  return { ...m, here: now, ...(m.here ? { before: m.here } : {}) };
}

/** When the operator was last here before this sitting, in Unix seconds; 0 when this is the first. */
export const lastHere = (m: Marks): number => (m.before ? Math.max(0, Math.floor(Date.parse(m.before) / 1000)) : 0);

// ── Annotations (§5 item 2) ──────────────────────────────────────────────────────────────────────────────────────────────

/** A passage as a note remembers it: its words without citation markers, spaces folded. */
export const passageKey = (blockText: string): string => unmarked(blockText).slice(0, PASSAGE_MAX);

/** Attach a note to a passage of a page version. An empty note, or a passage with no words, is not kept. */
export function annotate(m: Marks, n: { slug: string; version: number; passage: string; text: string }, now: string, id: string): Marks {
  const passage = passageKey(n.passage);
  const body = n.text.trim().slice(0, NOTE_TEXT_MAX);
  if (!SLUG.test(n.slug) || !passage || !body || m.notes.length >= NOTES_MAX) return m;
  return { ...m, notes: [...m.notes, { id, slug: n.slug, version: n.version, passage, text: body, at: now }] };
}

/** Change a note's words; emptied, it is removed. */
export function editNote(m: Marks, id: string, body: string): Marks {
  const t = body.trim().slice(0, NOTE_TEXT_MAX);
  return { ...m, notes: t ? m.notes.map((n) => (n.id === id ? { ...n, text: t } : n)) : m.notes.filter((n) => n.id !== id) };
}

export const removeNote = (m: Marks, id: string): Marks => ({ ...m, notes: m.notes.filter((n) => n.id !== id) });

/**
 * A page's notes against the passages of the version on screen: each note on the passage whose words it was attached
 * to, whichever version that was (so a note is carried to the next version when its passage survives), and the notes
 * whose passage is no longer on the page, listed as orphaned.
 */
export function placed(m: Marks, slug: string, passages: string[]): { on: Map<string, Annotation[]>; orphaned: Annotation[] } {
  const keys = new Set(passages.map(passageKey));
  const on = new Map<string, Annotation[]>();
  const orphaned: Annotation[] = [];
  for (const n of m.notes) {
    if (n.slug !== slug) continue;
    if (keys.has(n.passage)) on.set(n.passage, [...(on.get(n.passage) ?? []), n]);
    else orphaned.push(n);
  }
  return { on, orphaned };
}

// ── Pins (§5 item 3) ─────────────────────────────────────────────────────────────────────────────────────────────────────

export const pinned = (m: Marks, slug: string): boolean => m.pins.includes(slug);
export function togglePin(m: Marks, slug: string): Marks {
  if (!SLUG.test(slug)) return m;
  return { ...m, pins: pinned(m, slug) ? m.pins.filter((p) => p !== slug) : [...m.pins, slug] };
}

// ── Export (§5 item 4) ───────────────────────────────────────────────────────────────────────────────────────────────────

const BANNER = 'Generated page. Not authoritative.';

/**
 * A page as a Markdown file: the stored page as the daemon wrote it (its banner first, its bibliography and citations
 * table last, which is where its attributions are), and one line saying what was exported. Nothing for a page with no
 * banner: a page is exported with it or not at all.
 */
export function exportMarkdown(page: Pick<TopicPage, 'slug' | 'title' | 'version' | 'version_shown' | 'markdown'>, day: string): { name: string; text: string } | null {
  const md = page.markdown.replace(/\r\n?/g, '\n').trim();
  if (!md.includes(BANNER) || !SLUG.test(page.slug)) return null;
  const which = page.version_shown ? `the version of ${page.version_shown}` : `version ${page.version}`;
  const name = page.version_shown ? `${page.slug}-${page.version_shown.replace(/[^A-Za-z0-9-]/g, '')}` : `${page.slug}-v${page.version}`;
  return { name, text: `${md}\n\n---\n\nExported from Delphi on ${day}: the topic page "${page.title.replace(/\s+/g, ' ')}" (${page.slug}), ${which}. Generated, not authoritative: read the cited sources before relying on it.\n` };
}
