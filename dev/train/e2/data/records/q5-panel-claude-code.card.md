**Panel ruling** · host seats · barrier: held by Delphi · isolation: host-reported · blind: yes (host-reported) · 1 family reported · 2 model(s): claude-opus-5-5, claude-sonnet-5-5

**Decision:** needs the operator: Approve the amendments (including the two bypass fixes) inside PL1/PL2 before any writer ships, or hold the current text as is?

**Next steps**
1. `architect` (if PROCEED-WITH-CHANGES): Amend specs/070 with a class order (governed path, then loaded-by-consumer, then executable/config/archive, then the Artifact allow-list, then Markdown/scratch) and reclassification on rename, move, mode change and symlink.
2. `builder` (if PROCEED-WITH-CHANGES): Remove (system) entries from PL2, make the artifact hash mandatory, anchor each chain head in the audit log, and add a minimal chain verifier plus scrubbing and a default retention before any writer ships.
3. `strategist` (if PROCEED-WITH-CHANGES): Update section 8 to record this re-review, give each section 4 change its own tracker row, and record the pending GOVERNANCE /ratify.

**Who sat:** `architect` (answered, claude-opus-5-5), `red-team` (answered, claude-sonnet-5-5), `strategist` (answered, claude-sonnet-5-5)

**Where they stood in Round 2**
- `architect`: PROCEED-WITH-CHANGES, did not concur
- `red-team`: PROCEED-WITH-CHANGES, did not concur
- `strategist`: PROCEED-WITH-CHANGES, did not concur

**Dissent (verbatim from the record)**
> - [P0] red-team: Class is fixed at write time; require reclassification on rename, move, chmod and symlink, and classify archives and containers (zip, PDF with JS or embedded files) as Software
> - [P0] red-team: Markdown exemption must run after the loaded-as-instructions check, defined by path and consumer, not extension; the architect's "accept the gap" option is rejected
> - [P1] architect: AC-6 chain detection contradicts OD-1 deferral of the verifier; no anchor against tail truncation
> - [P1] architect: Class order undefined: Markdown or other files loaded as instructions escape the gate; Markdown vs plain-document Artifact overlap
> - [P1] architect: Classification only at write time: rename, move, chmod, symlink and archives bypass it
> - [P1] architect: Line 34 "operator-approved prompt" leaves System operation open-ended; enumerate it
> - [P1] architect: PL2 still lists (system) entries, against §3.5 and D-3
> - [P1] red-team: "Operator-approved prompt" in the System class is open-ended; replace it with an enumerated list of write types
> - [P1] red-team: "Verbatim" contradicts pre-write scrubbing; define the redaction marker and record scrubbed spans as a count or hash; ship scrubbing before any writer, with a default retention
> - [P1] red-team: AC-6 claims a broken chain is detected while OD-1 defers the verifier; move a minimal verifier into PL2 or drop the claim, and anchor the chain head per entry in the audit log to catch tail truncation
> - [P1] red-team: PL2 still lists (system) entries, contradicting §3.5 and D-3; the content hash is "optional" in §3.2 but mandatory in §3.4; fix both so a builder does not create (system) entries
> - [P1] red-team: Unattributed writes (scheduled, sub-agent) are unspecified; state allowed with an audit-log entry only, and add the acceptance test (the architect's AC for D-3)
> - [P1] strategist: "Loaded" versus Markdown overlap: Markdown is always System operation, but Markdown loaded as instructions (prompts, skills, AGENTS.md) is Software under section 2.1.3. Run the loaded/consumer check b
> - [P1] strategist: "Verbatim" (section 3.1, AC-3) conflicts with scrubbing before write (OD-1). Define the redaction marker and say that entries are verbatim except for scrubbed spans, recorded as a count or hash.
> - [P1] strategist: AC-6 requires detecting a broken hash chain, but the verifier is deferred to a follow-up; tail truncation is undetectable without an anchor. Move a minimal verifier into PL2 and anchor the chain head 
> - [P1] strategist: Interim secret retention: until PL2 scrubbing and a retention rule exist, pasted secrets sit on disk in the log. Make scrubbing a gate before any writer ships, or set a short interim default retention
> - [P1] strategist: PL2 writes "(system)" entries, contradicting section 3.5 and D-3; fix the slice text before any builder reads it.
> - [P1] strategist: Post-write reclassification: rename, mode change (chmod +x), symlink into a governed path, and archives or containers can change class after the check. Require reclassification on rename, move and mod
> 
> Homogeneous (low diversity): every seat is in one model family, so the author's family reviewed its own work. You may prefer to wait for a second family before approving.

**You must answer:** 2 unresolved P0 finding(s).
- PROCEED-WITH-CHANGES

**Cost:** seats ran on your host's tokens (not measured here); Delphi spent none. **Record:** `missions/host-4462edd4de2e97aa/councils/001-should-draft-spec-070-be-approved-it-kee.md`.

---

Notes from me, outside the card:
- **Answers to D-1 to D-3:** the seats agreed on all three.
  - **D-1:** a daemon-only log under `local/artifact-log/<mission>/`, with each chain head anchored in the audit log.
  - **D-2:** `specs/external/<project>/`.
  - **D-3:** no `(system)` entries; unattributed writes go to the audit log only.
- **Spec status:** the seats found that `specs/070` already says `status: approved` (2026-11-03) and GOVERNANCE §1.3 already carries the amended text. They treated this as the re-review that §8 asks for.
- **Seat submissions:** the red-team and strategist seats returned their Round 1 text without calling `council_submit`. I submitted those two stances for them, word for word. The Round 2 answers and the architect's Round 1 were submitted by the seats themselves.
- **Decision pending:** the card has one option, PROCEED-WITH-CHANGES. I haven't called `council_choose` because that's your decision. Tell me if you want it recorded.