// What the daemon and its CLI send (spec 111 §3). Every field the app does not use is left out; nothing here is the app's own table.

/** One role's look, from the avatar library (`avatar::ui_spec`), by way of `delphi daemon where`. */
export type AvatarRole = {
  chassis: string;
  chassis_light?: string;
  /** The fill on a light surface: chips, bars. Absent from a `delphi` that predates it. */
  chassis_light_fill?: string;
  /** Per activity: the three cells between the brackets, and which of them is the mouth (the rest are eyes). */
  badge: Record<string, { cells: string; mouth: number | null }>;
  blink_steps?: string[];
  /** The mascot: its crown (seven cells, `▄` where there is a point), the crown's colour, and per activity its seven-cell face row. */
  crown?: string;
  crown_rgb?: string;
  mascot?: Record<string, string>;
  alert_eye?: { dark: string; light: string };
};
export type AvatarSpec = { version: number; eye: { dark: string; light: string }; roles: Record<string, AvatarRole> };

/** `delphi daemon where`: where the sockets are and how roles look. No token is in it. */
export type Info = {
  narrator_socket?: string;
  workspace?: string;
  workspace_name?: string;
  version?: string;
  home?: string;
  roles: Record<string, { name: string; chassis: string; face: string }>;
  avatar?: AvatarSpec;
  /** The active theme's name pool: slot to the name and face a one-off agent wears. */
  pool?: Record<string, { name: string; face: string }>;
  projects?: { name: string; root: string; home: boolean }[];
};

export type Tokens = { input: number; cache_read: number; cache_write: number; output: number; thinking: number };
export type AgentRow = { role: string; state: string; task_id: string; endpoint: string; tier: string; model: string; tokens: Tokens; cost_usd: number; calls: number };
export type Node = {
  id: string; role: string; parent: string | null; depth: number; task_id: string; state: string; waiting?: string;
  endpoint?: string; model?: string; pool_slot?: string; label?: string; started_ms: number; ended_ms: number | null; after: string[];
};
export type Task = { id: string; flow: string; prompt: string; status: string; output?: string; session?: string };
export type Change = { session: string; at_ms: number; role: string; task: string; summary: string; files: string[] };
export type Council = {
  task_id: string; proposition: string; seats: { role: string; family: string }[]; homogeneous: boolean; phase: string;
  stances: { role: string; answered: boolean; text: string }[];
  critiques: { role: string; recommendation: string; concur: boolean | null; findings: [string, string][]; text: string }[];
  escalation: string | null; path: string; unresolved: [string, string, string][]; options: [string, string[]][];
  synthesis: string; lenses: [string, string][]; lens_note: string;
};

/** `observe.state`: the daemon's folded picture of a mission. */
export type OState = {
  mission_id?: string;
  agents: Record<string, AgentRow>;
  nodes: Record<string, Node>;
  tasks: Record<string, Task>;
  councils: Council[];
  builds?: { task_id: string; branch: string; sha: string; merge: string }[];
  changes?: Change[];
  approvals?: Record<string, { role: string; summary: string; since_ms: number }>;
  since_you?: { since_ms: number; changes: number; done: number; failed: number; approvals: number };
  sessions?: { id: string; reason: string; started_ms: number; turns: number }[];
  providers?: { endpoint: string; family: string; models: number; cooling: number; out: number; back_ms: number | null }[];
  pending_proposals?: number;
};

/** A persisted event, as `observe.snapshot` lists them and as an `event` notification carries one. */
export type Envelope = {
  type: string; timestamp: string; seq?: number; mission_id?: string; task_id?: string; span_id?: string; agent?: string;
  pool_slot?: string; round?: number; payload: Record<string, unknown>;
};

export type Conn = { connected: boolean; detail: string };
