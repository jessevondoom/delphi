**Panel ruling** · host seats · barrier: held by Delphi · isolation: host-reported · blind: yes (host-reported) · 1 family reported · 3 model(s): claude-haiku-5-5, claude-opus-5-5, claude-sonnet-5-5

**Decision:** 

**Who sat:** `architect` (answered, claude-opus-5-5), `red-team` (answered, claude-sonnet-5-5), `strategist` (answered, claude-haiku-5-5)

**Where they stood in Round 2**
- `architect`: ALTERNATIVE: one spec-068 l0 inbox, fed by daemon auto-stage of delphi-run research plus the , did not concur
- `red-team`: PROCEED-WITH-CHANGES
- `strategist`: PROCEED-WITH-CHANGES, did not concur

**Dissent (verbatim from the record)**
> - [P0] red-team: Host-supplied sources, confidence and "internal" labels are untrusted claims. The daemon must derive taint and origin from the actual retrieval span, and must sign records daemon-side.
> - [P0] red-team: Pending or staged items must be excluded from retrieval, validated answers and bundle or Xenia export until ratified, and a test must enforce this.
> - [P1] architect: Capture that depends only on the agent choosing to call a tool cannot deliver "always"; Delphi-run research must be staged by the daemon
> - [P1] architect: Options 1 and 2 re-decide approved spec 068; a new tool or second store duplicates propose_kb_entry and splits provenance
> - [P1] architect: Secret scrubbing and a local-only flag until ratified must run on the staging path, not only on import; check the sharing scope (Xenia, bundles) on keep
> - [P1] architect: Staged entries must stay out of search_kb (against the red-team's labelled-inbox retrieval); inclusion must never read as validity
> - [P1] red-team: Auto-staging with batch keep invites rubber-stamping. It needs hash dedupe, per-source grouping, caps, expiry of untouched items, and a ratify-time sharing-scope and confidentiality check.
> - [P1] red-team: Secret scrubbing must run on the staging path (MCP and daemon), not only on import.
> - [P1] red-team: Store the source excerpt and hash alongside the paraphrase, so a later validator can recheck it.
> - [P1] strategist: Pending inbox items must never feed validated answers or raise their own trust in search, and search must label them unvalidated.
> - [P1] strategist: Secret scrubbing for internal sources must run on the staging write path, not only on import.
> - [P1] strategist: Taint comes from the source span the daemon retrieved, never from host-supplied sources or confidence, so a host cannot launder web content as internal.
> - [P1] strategist: Verify spec 068 and the tracker before any build, so the Council does not re-decide an approved spec or create a second store or schema.
> 
> Homogeneous (low diversity): every seat is in one model family, so the author's family reviewed its own work. You may prefer to wait for a second family before approving.

**You must answer:** 2 unresolved P0 finding(s); the Round 2 recommendations conflict.
- ALTERNATIVE: one spec-068 l0 inbox, fed by daemon auto-stage of delphi-run research plus the
- PROCEED-WITH-CHANGES

**Cost:** seats ran on your host's tokens (not measured here); Delphi spent none. **Record:** `missions/host-53d5758984b831a9/councils/001-operators-do-research-via-delphi-tui-or.md`.

⟦•─•⟧ **Athena** (orchestrator) via delphi

---

My own notes, which are not part of the card:
- **Decision line is empty.** My verdict left out the closing `DECISION:` and `IF … NEXT` lines the brief asked for, so `next_steps` is empty. Delphi accepted it anyway.
- **Option labels are garbled.** The first option's label is cut off ("…plus the"). It comes from the brief itself, not from the seats.
- **Seats needed a prompt to submit.** The strategist didn't call `council_submit` in either round, and I had to ask it to. The architect and red-team submitted on their own.
- **Model names are host-reported.** The architect reported running on claude-opus-5-5 and the strategist on claude-haiku-5-5, though I told every seat to report claude-sonnet-5-5. I can't verify which model each seat actually ran on.
- **The operator still has to choose.** I haven't called `council_choose`. If you pick an option, tell me which and I'll report it to Delphi.