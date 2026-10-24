---
id: council-007-narrator-process-or-pane
title: "Council 007: Narrator window, separate process or embedded pane"
author: orchestrator
timestamp: 2026-10-24T05:40:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-007, narrator]
council_format: 1
---

# Council 007: Narrator window, separate process or embedded pane

The first Council run by the runtime itself (`/council`), on a real design question for Step 5. Seats: Architect and Strategist on Claude (Sonnet, subscription harness), Red-Team on Gemini (Flash, via `agy`). **Operator decision (2026-10-24): approved, a separate process (D-25).** The seats unanimously recommended it; the escalation was the deterministic P0 rule, and the P0 items are enforcement requirements recorded as open follow-ups in D-25.

**Proposition:** Should Delphi's Narrator window be a separate `delphi narrator` process that attaches to the daemon over its Unix socket, or a pane embedded in the console REPL process? The Narrator shows an agents pane and four views and must never be able to start work or approve anything.

## Seats

| Seat | Family | Round 1 | Round 2 |
| :-- | :-- | :-- | :-- |
| `architect` | claude | answered | critique |
| `strategist` | claude | answered | critique |
| `red-team` | gemini | answered | critique |

Heterogeneous: the seats span more than one model family (`[H-001]`).

- red-team moved from claude to gemini (never the author's family)

## Round 1 stances (blind)

### architect

> OPTION: Separate process, read-only socket attach
> 
> **Reasoning**
> 
> The requirement "must never be able to start work or approve anything" is a capability constraint, and the strongest way to enforce a capability constraint is to make the violation structurally impossible rather than merely unimplemented. A separate `delphi narrator` process gives that.
> 
> 1. **Capability boundary at the socket.** The daemon can serve the Narrator on a distinct read-only endpoint (a second socket path, or a handshake that declares `role=narrator`). The daemon then rejects every mutating verb (start, approve, dispatch) for that connection. The guarantee lives in the daemon, which is the trust root, and not in Narrator UI code that someone could later wire a keybinding into.
> 
> 2. **Embedding inherits authority.** The console REPL is the operator's command surface. A Narrator pane in that process shares its memory, its socket client and its credentials. One refactor or shortcut away, the pane can call the same functions the REPL does. The "never" would rest on code discipline, which is the weakest form of enforcement.
> 
> 3. **Independent lifecycle.** The Narrator can be opened, closed, crashed or restarted without touching the REPL session or the daemon. It can run in a second terminal or tmux pane, or be handed to an observer who should watch but not drive.
> 
> 4. **Simpler contract surface.** The Narrator needs only a subscribe-and-snapshot protocol: an event stream plus state queries for the agents pane and the four views. That is a small, testable contract, and I can specify it with a verb allowlist table.
> 
> **Simpler Alternative Considered:** an embedded pane is less code, with one process, no second client and no extra endpoint. I reject it because the saving is small (the socket client already exists) and the cost is losing the structural guarantee. If the daemon's read-only role enforcement proves too costly, the fallback is embedding with a compile-time-separate read-only client type, which is weaker.
> 
> **Risks**
> 
> - **Role self-declaration.** If the client merely announces `role=narrator`, any process can claim otherwise. Mitigation: use a separate socket file with its own filesystem permissions, with the daemon binding the verb allowlist to the socket, not to the claim.
> - **Protocol drift.** A second client means a second consumer of the event schema. It needs versioning and a shared types crate or module.
> - **Snapshot/stream races.** Attaching mid-run can miss events. It needs a sequence number and a snapshot-then-tail handshake.
> - **Information leakage.** Read-only does not mean harmless. The Narrator may show sensitive content, so the stream should be filtered at the daemon.
> - **Same-user trust.** On a Unix socket, anything running as the same user can reach both sockets. This design stops accidental or UI-level escalation, not a hostile local process, and the spec should say so plainly.
> - **Operational friction.** Operators must launch another process, and the daemon-down case needs a clear message.

### red-team

> OPTION: Separate process over Unix socket
> 
> Enforcing the hard invariant that the Narrator "must never be able to start work or approve anything" requires a strict operating-system privilege boundary. 
> 
> Embedding the Narrator within the console REPL process creates an unacceptable shared-memory attack surface. The Narrator renders untrusted, dynamic output from multiple agents across five distinct views. If embedded, any memory corruption, event-loop confusion, or terminal-injection vulnerability (such as malicious ANSI escape sequences or bracketed paste exploits sourced from adversarial agent text) executes in the same address space as the REPL's approval and execution logic. A compromised view can synthesize keystrokes, alter approval state variables, or directly invoke internal dispatch functions.
> 
> A separate `delphi narrator` process provides defense-in-depth:
> 1. **Enforceable Capability Separation:** The daemon can expose a dedicated, strictly read-only IPC endpoint (or drop write/exec capabilities upon connection). Even complete remote code execution inside the Narrator process cannot issue an approval frame or initiate work because the daemon's wire protocol rejects state-mutating requests on that socket.
> 2. **Blast-Radius Containment:** Parsing complex multi-pane streaming data is error-prone. Isolating the Narrator ensures rendering crashes, memory exhaustion, or panic loops do not terminate the operator's active REPL session.
> 3. **Falsifiable Auditing:** IPC over a Unix domain socket provides a clear boundary where message schemas and access policies can be statically verified and logged.
> 
> Risks:
> - **Socket Permission & Multi-Tenant Exposure:** Insecure filesystem permissions (e.g., overly permissive umask or world-readable `/tmp` paths) could allow unauthorized local processes to snoop agent telemetry or launch local denial-of-service attacks by starving connection pools.
> - **Protocol Drift:** If the socket interface reuses general daemon RPC schemas instead of a dedicated, structurally read-only publish/subscribe channel, future feature additions might inadvertently introduce mutation commands.
> - **Socket Backpressure / Stale State:** Slow UI consumption over the socket could cause telemetry buffering issues, leading to desynchronization where the operator views stale agent state while making live decisions.

### strategist

> OPTION: Separate `delphi narrator` process, read-only
> 
> **Reasoning**
> 
> From a scope and ROI view, the deciding factor is the requirement that the Narrator must never start work or approve anything. A separate process turns that from a code-review promise into a structural property. The console REPL is the operator's command surface, so it holds the capability to start work and approve. Embedding the Narrator there means every Narrator code path shares an address space with those capabilities, and the guarantee depends on every future PR keeping them apart. That is exactly the kind of quiet scope creep I audit for.
> 
> A separate process also gives the Narrator its own narrow contract. It attaches to the daemon's Unix socket and speaks a read-only subset of the protocol: subscribe, list agents, fetch the four views. If the daemon enforces that at the socket (a read-only role or a separate read-only socket, with mutating verbs rejected server-side), the Narrator cannot misbehave even if its client code is buggy. Enforcement belongs in the daemon, not the client.
> 
> Cost is modest. The daemon already owns the state and the socket, so the Narrator is a thin client. Embedding saves little: it would still need to read daemon state, and it would couple the Narrator's rendering and crash behavior to the REPL. A separate process is also independently restartable and can sit in a second terminal, which is the natural use for a "window".
> 
> Sequencing for the roadmap:
> 1. Define the read-only protocol subset and server-side rejection of mutating verbs, with tests.
> 2. Ship `delphi narrator` with the agents pane and one view, standalone and verifiable.
> 3. Add the remaining three views incrementally.
> 
> I would not build a plugin system, a shared UI abstraction for both hosts, or an embedded mode "for later". Those are premature.
> 
> **Risks**
> 
> - Client-only read-only enforcement is theater. If the daemon does not reject mutating verbs on the Narrator's connection, the separation buys nothing. This must be a P0 acceptance criterion, and I cannot waive it for speed.
> - Socket authentication is unspecified. Any local process that can reach the socket could claim the Narrator role or the full role. Socket permissions and role binding need to be defined (for example, file mode 0600 plus separate sockets per role).
> - Role selection by client self-declaration is weak. Prefer distinct sockets, or role fixed at accept time.
> - Added operational surface: a second binary or subcommand, version skew with the daemon, and reconnect handling.
> - Narrator's streamed content could leak sensitive agent output to anyone who can read the socket. Access needs the same scrutiny as the console.
> - I have not seen the actual daemon protocol, so how cleanly verbs split into read and mutate is unverified.

## Round 2 critiques

### architect

> All three seats chose a separate process, and I concur. The remaining disagreement is about enforcement details, which Round 1 left as risks instead of requirements.
> 
> - **Role binding:** the Strategist and Architect both flag self-declared roles. The listener should fix the role at accept time. The Narrator gets its own socket in a 0700 directory with mode 0600, and the handshake carries no role claim.
> - **Allowlist, not denylist:** a list of "mutating verbs to reject" fails open when a new verb is added. The Narrator socket should speak a dedicated, structurally read-only pub/sub schema (subscribe, snapshot, list agents, fetch views). The daemon's dispatcher for that listener should not contain handlers for start or approve at all. That addresses the Red-Team's protocol-drift concern.
> - **Terminal injection:** the Red-Team is right that agent text is adversarial, and a separate process does not remove that risk. The Narrator must strip ANSI and control sequences when rendering. Without this, a hostile agent can still mislead the operator through the display, even though it cannot approve anything.
> - **Stream integrity:** each event carries a sequence number, attach is snapshot-then-tail, and each subscriber has a bounded queue. On overflow the daemon drops the subscriber and forces a resync instead of showing stale state silently.
> - **Leakage:** redaction happens in the daemon, before events reach the socket.
> - **Trust boundary:** this stops UI-level and accidental escalation, not a hostile process running as the same user. The spec must say so.
> - **Unverified:** nobody has seen the real daemon protocol, so the read/mutate split needs checking against it.
> - **Simpler alternative:** the embedded pane is rejected because the structural guarantee is worth more than the small code saving. The spec must record this, and a compile-time read-only client type is the weaker fallback.
> - **Acceptance test:** a test connects to the Narrator socket and attempts every mutating verb. The daemon must reject each one.
> 
> RECOMMENDATION: PROCEED-WITH-CHANGES
> FINDING P1 Role must be fixed by listener at accept time (separate 0600 socket in 0700 dir), never by client handshake claim
> FINDING P1 Narrator socket uses a deny-by-default read-only schema with no mutating handlers in its dispatcher, plus an acceptance test that attempts every mutating verb
> FINDING P1 Narrator must sanitize ANSI and control sequences in agent text, since process separation does not stop display-level deception
> FINDING P2 Stream needs sequence numbers, snapshot-then-tail attach, and bounded per-subscriber queues with forced resync on overflow
> FINDING P2 Daemon-side redaction, a versioned shared event-types module, and an explicit same-user trust-boundary statement belong in the spec
> FINDING P2 The real daemon protocol was not inspected, so the read/mutate verb split is unverified
> CONCUR: yes
> ROUND 2 COMPLETE

### red-team

> A separate process is structurally necessary, but the Round 1 proposals leave critical attack surfaces unmitigated. Architect's suggestion of an in-band handshake (`role=narrator`) on a shared socket is a major vulnerability: an attacker or subverted client reaching the socket can omit the handshake to issue mutating verbs (`approve`, `start`). Enforcement must strictly require physically separate socket files (mode 0600) bound to an immutable read-only dispatch table. Furthermore, a slow, hung, or halted (`SIGSTOP`) Narrator process can induce socket backpressure that blocks the daemon's event loop or exhausts daemon memory unless the daemon enforces non-blocking bounded queues with consumer disconnects. Finally, streaming raw agent telemetry into Narrator panes allows adversarial agent output to execute ANSI escape injections (e.g., OSC 52 clipboard hijacking or display spoofing), requiring ingestion-boundary sanitization, while snapshot attach without monotonic sequence fencing risks silent UI state desynchronization.
> 
> RECOMMENDATION: PROCEED-WITH-CHANGES
> FINDING P0 In-band role handshake on shared socket permits mutating verb escalation
> FINDING P1 Daemon event-loop starvation via unbuffered Narrator socket backpressure
> FINDING P1 Unsanitized agent telemetry enables terminal escape sequence injection
> FINDING P2 Snapshot attach race condition causes silent state desynchronization
> CONCUR: no
> ROUND 2 COMPLETE

### strategist

> All three seats converge on a separate process, and I agree. The deciding factor is the structural "never approve or start work" requirement, which a separate process enforces at the daemon and an embedded pane leaves to code discipline. On cost, the Narrator is a thin client over a socket the daemon already owns, so embedding saves little. The main scope risks are in the Round 1 proposals themselves: Architect's verb allowlist table, shared types crate, versioning, and snapshot/stream sequencing, plus Red-Team's audit logging and static schema verification, could balloon the first milestone. I would sequence it: first the daemon-side read-only enforcement with tests, then the agents pane and one view, then the remaining three views. Plugin systems, a shared UI abstraction, and an embedded mode "for later" stay out of scope. None of the seats has seen the real daemon protocol, so the split between read and mutate verbs is unverified. That is a precondition to check, not a reason to block.
> 
> RECOMMENDATION: PROCEED-WITH-CHANGES
> FINDING P0 Read-only must be enforced by the daemon, bound to a separate socket at accept time (mode 0600), with mutating verbs rejected server-side and tested; client self-declared role is not acceptable
> FINDING P1 Daemon protocol is unreviewed, so the read/mutate verb split is unverified; confirm it before committing to the milestone plan
> FINDING P1 Scope must be phased: enforcement and tests, then agents pane plus one view, then the other three views; no plugin system, shared UI abstraction, or embedded fallback
> FINDING P1 Daemon-side stream filtering of sensitive output is unspecified; define the minimum policy or state that the Narrator sees what the console sees
> FINDING P2 Snapshot-then-tail with sequence numbers and protocol versioning are needed, but keep them minimal in v1 and skip the shared types crate until a second consumer exists
> FINDING P2 Spec should state that same-user local processes can reach both sockets, so this stops accidental escalation, not a hostile local process
> CONCUR: yes
> ROUND 2 COMPLETE

## Round 2 structured output

- architect | recommendation | PROCEED-WITH-CHANGES
- architect | concur | true
- architect | P1 | Role must be fixed by listener at accept time (separate 0600 socket in 0700 dir), never by client handshake claim
- architect | P1 | Narrator socket uses a deny-by-default read-only schema with no mutating handlers in its dispatcher, plus an acceptance test that attempts every mutating verb
- architect | P1 | Narrator must sanitize ANSI and control sequences in agent text, since process separation does not stop display-level deception
- architect | P2 | Stream needs sequence numbers, snapshot-then-tail attach, and bounded per-subscriber queues with forced resync on overflow
- architect | P2 | Daemon-side redaction, a versioned shared event-types module, and an explicit same-user trust-boundary statement belong in the spec
- architect | P2 | The real daemon protocol was not inspected, so the read/mutate verb split is unverified
- red-team | recommendation | PROCEED-WITH-CHANGES
- red-team | concur | false
- red-team | P0 | In-band role handshake on shared socket permits mutating verb escalation
- red-team | P1 | Daemon event-loop starvation via unbuffered Narrator socket backpressure
- red-team | P1 | Unsanitized agent telemetry enables terminal escape sequence injection
- red-team | P2 | Snapshot attach race condition causes silent state desynchronization
- strategist | recommendation | PROCEED-WITH-CHANGES
- strategist | concur | true
- strategist | P0 | Read-only must be enforced by the daemon, bound to a separate socket at accept time (mode 0600), with mutating verbs rejected server-side and tested; client self-declared role is not acceptable
- strategist | P1 | Daemon protocol is unreviewed, so the read/mutate verb split is unverified; confirm it before committing to the milestone plan
- strategist | P1 | Scope must be phased: enforcement and tests, then agents pane plus one view, then the other three views; no plugin system, shared UI abstraction, or embedded fallback
- strategist | P1 | Daemon-side stream filtering of sensitive output is unspecified; define the minimum policy or state that the Narrator sees what the console sees
- strategist | P2 | Snapshot-then-tail with sequence numbers and protocol versioning are needed, but keep them minimal in v1 and skip the shared types crate until a second consumer exists
- strategist | P2 | Spec should state that same-user local processes can reach both sockets, so this stops accidental escalation, not a hostile local process

## Verdict

**Escalated to the operator:** 2 unresolved P0 finding(s).

- [Option A] PROCEED-WITH-CHANGES — architect, red-team, strategist

> **Verdict: escalated to the operator.**
> 
> All three seats chose a separate `delphi narrator` process, and no seat defended the embedded pane. The deciding reason is that "never start work or approve" holds best as a structural property enforced by the daemon, not by discipline inside a shared REPL address space. The Council therefore leans toward the separate process, but two unresolved P0 findings send the decision to you.
> 
> **Where the seats agreed:** the daemon must enforce read-only access, not the client. Role should be fixed when the listener accepts the connection, using a dedicated 0600 socket, with no client-declared role. Agent text must be sanitized before display, since a separate process does not stop terminal-escape spoofing. Snapshot-then-tail attach with sequence numbers is needed. The spec should state that same-user local processes can still reach the sockets. Nobody inspected the real daemon protocol, so the read/mutate verb split is unverified.
> 
> **Where they differed:** Red-Team did not concur. It treats a shared socket with an in-band handshake as an escalation path, and it wants bounded queues so a stalled Narrator cannot starve the daemon. Architect favors a deny-by-default read-only schema with no mutating handlers at all. Strategist agrees on enforcement but wants a phased scope: enforcement and tests first, then the agents pane and one view, then the other three. It would defer the shared types crate and any embedded fallback. Architect would keep a compile-time read-only client as a weaker fallback.
> 
> **Your choice:** adopt the separate process with the accept-time, socket-bound enforcement above, accepting the added operational surface. Or choose the embedded pane, accepting a smaller code footprint but a guarantee that rests on code discipline. Before committing, confirm the daemon's verb split.

## Accepted Risks & Minority Report (Red-Team / Dissent)

- [P0] red-team: In-band role handshake on shared socket permits mutating verb escalation
- [P0] strategist: Read-only must be enforced by the daemon, bound to a separate socket at accept time (mode 0600), with mutating verbs rejected server-side and tested; client self-declared role is not acceptable
- [P1] architect: Narrator must sanitize ANSI and control sequences in agent text, since process separation does not stop display-level deception
- [P1] architect: Narrator socket uses a deny-by-default read-only schema with no mutating handlers in its dispatcher, plus an acceptance test that attempts every mutating verb
- [P1] architect: Role must be fixed by listener at accept time (separate 0600 socket in 0700 dir), never by client handshake claim
- [P1] red-team: Daemon event-loop starvation via unbuffered Narrator socket backpressure
- [P1] red-team: Unsanitized agent telemetry enables terminal escape sequence injection
- [P1] strategist: Daemon protocol is unreviewed, so the read/mutate verb split is unverified; confirm it before committing to the milestone plan
- [P1] strategist: Daemon-side stream filtering of sensitive output is unspecified; define the minimum policy or state that the Narrator sees what the console sees
- [P1] strategist: Scope must be phased: enforcement and tests, then agents pane plus one view, then the other three views; no plugin system, shared UI abstraction, or embedded fallback
