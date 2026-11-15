// The window's own preferences (spec 111 §2.4): which theme, whether the menu shows, which sections are folded, which screen.
// Local storage only; it can be empty or refuse, and the window draws the same without it.
export type Screen = 'activity' | 'council' | 'knowledge' | 'output' | 'agents' | 'settings';
export type Prefs = { theme: 'system' | 'dark' | 'light'; menu: boolean; folded: Record<string, boolean>; screen: Screen };

export const DEFAULTS: Prefs = { theme: 'system', menu: true, folded: {}, screen: 'activity' };
const KEY = 'delphi.prefs.v1';
const SCREENS: Screen[] = ['activity', 'council', 'knowledge', 'output', 'agents', 'settings'];

/** Whatever was stored, made into valid preferences: an unknown or damaged value falls back to its default. */
export function parse(raw: string | null): Prefs {
  let v: Partial<Prefs> = {};
  try {
    const j: unknown = raw ? JSON.parse(raw) : {};
    if (j && typeof j === 'object') v = j as Partial<Prefs>;
  } catch { /* damaged: defaults */ }
  const folded: Record<string, boolean> = {};
  if (v.folded && typeof v.folded === 'object') for (const [k, x] of Object.entries(v.folded)) if (x === true) folded[k] = true;
  return {
    theme: v.theme === 'dark' || v.theme === 'light' ? v.theme : 'system',
    menu: v.menu !== false,
    folded,
    screen: SCREENS.includes(v.screen as Screen) ? (v.screen as Screen) : 'activity',
  };
}

/** A screen named in the address (`#council`) opens first: a link to one screen, for a browser and for tests of the page. */
export function fromHash(p: Prefs, hash: string): Prefs {
  const named = hash.replace(/^#/, '') as Screen;
  return SCREENS.includes(named) ? { ...p, screen: named } : p;
}

export function load(): Prefs {
  let p: Prefs;
  try { p = parse(globalThis.localStorage?.getItem(KEY) ?? null); } catch { p = { ...DEFAULTS }; }
  return fromHash(p, globalThis.location?.hash ?? '');
}

export function save(p: Prefs): void {
  try { globalThis.localStorage?.setItem(KEY, JSON.stringify(p)); } catch { /* nowhere to keep it */ }
}
