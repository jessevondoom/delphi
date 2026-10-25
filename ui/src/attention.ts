// How a stopped task reads (`specs/005e` §6.6): the stop and what to do about it, not the engine's bracketed
// plumbing. The Rust consoles have the same table in `crates/delphi/src/escalation.rs`; keep the two in step.

export type Cause = 'no_model_configured' | 'budget' | 'reauth_required' | 'exhausted' | 'other';
export type Attention = { cause: Cause; headline: string; hints: string[] };

/** The reason inside the engine's `[escalated to the operator: …]` wrapper, or the text itself. */
export function stripWrapper(text: string): string {
  const t = text.trim();
  const m = /^\[escalated to the operator:\s*([\s\S]*)\]$/.exec(t);
  return m ? m[1]!.trim() : t;
}

export function classify(text: string): Attention {
  const reason = stripWrapper(text);
  if (reason.includes('no_model_configured')) {
    return { cause: 'no_model_configured', headline: 'Stopped: no model is configured for this workspace.', hints: ['delphi setup     connects one', '/models          shows the pool and what is usable'] };
  }
  if (reason.startsWith('mission budget')) {
    return { cause: 'budget', headline: "Stopped: this mission's ceiling is reached.", hints: ['/budget <dollars>   raises it', '/usage              shows where it went'] };
  }
  if (reason.startsWith('re-authenticate ')) {
    return { cause: 'reauth_required', headline: `Stopped: ${reason.slice('re-authenticate '.length).trim()} needs you to sign in again.`, hints: ["aws sso login (or the provider's command), then send again"] };
  }
  if (reason.startsWith('every allowed endpoint is throttled or unavailable')) {
    return { cause: 'exhausted', headline: 'Stopped: every usable model is out of quota or down.', hints: ['/models shows when each is back'] };
  }
  return { cause: 'other', headline: `Stopped: ${reason}`, hints: ['/resume to try again · /models · /usage'] };
}
