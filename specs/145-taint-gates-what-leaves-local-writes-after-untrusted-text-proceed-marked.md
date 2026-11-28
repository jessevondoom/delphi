---
id: spec-145-taint-gates-what-leaves-local-writes-after-untrusted-text-proceed-marked
title: "145: Taint gates what leaves: local writes after untrusted text proceed, marked; outbound actions ask"
author: architect
timestamp: 2026-11-28T04:00:00Z
status: approved   # by the operator on 2026-11-27: "145 is approved. keep going", approved before the red-team review of §6, which still runs before slice G is ratified
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the gate is one function (provenance::gate) and one write check; the risk trade is argued, not measured
source_mission: 2026-11-25-knowledge-v2
tags: [spec, governance, taint, authority, prompt-injection, held-text]
---

# Spec 145: Taint gates what leaves

> **Status: approved** by the operator on 2026-11-27 ("145 is approved. keep going"). Amendment 145-A1 (§2.4), prioritized from the reviews of §6 (OpenAI asks `t_0014` and `t_0016`), was accepted by the operator on 2026-11-27 ("yes").
> **Amends:**
> - `GOVERNANCE.md` §4.3, its fourth bullet ("Taint gates actions, not only memory"). This is a constitutional change: Red-tier, applied only when the operator ratifies the diff (§5.7), in its own gate-changing pull request.
> - [`specs/001`](001-governance-and-manifests.md) §5.2 (the taint-gated tools table) and its amendment refining §5.1 and §5.2 ("a tainted span cannot launder through a broader glob").
>
> **Changes the build of:** [`specs/142`](142-the-source-text-cache-immutable-captures-host-text-handover-and-anchors-that-show-their-orphans.md) B, as merged on `knowledge-v2` on 2026-11-27 (held text by the span's tools), and [`specs/144`](144-pages-built-from-evidence-citable-text-evidence-packs-and-a-trial.md) §2.6.

## 1. Why

### 1.1 The problem

Under GOVERNANCE §4.3, a span whose computed taint is `federated_peer` or lower loses every mutating and outbound tool. Once the knowledge base holds the text of its sources (142), almost every useful search returns some untrusted text. The rule then disarms the roles that use the knowledge base most:

- The architect cannot write a spec after reading a source.
- The strategist cannot write a roadmap.
- The builder cannot build after looking something up.
- Every dispatch from a tainted span stops for an approval.

The first build of 142 B worked around this by giving held text by default only to roles without such tools. That left the knowledge base thin for the roles that act, and it still disarmed the architect and the strategist.

### 1.2 What the rule guards against

1. **False knowledge:** untrusted text puts a wrong statement into a page or a claim. Other checks now cover this:
   - citable text only (144);
   - a validator, with pinpoint passages;
   - labels;
   - `pages dispute`;
   - claim retraction;
   - page rebuilds.

   A tool ban adds little here.
2. **Prompt injection driving actions:** a fetched page tells the agent to send a secret somewhere, or to add a line to a file the host runs. No knowledge check catches this, because it is not a claim. This is the risk the rule exists for.

### 1.3 Where an action can do lasting harm

- **A local write** (a spec, a roadmap, research, docs, code in a builder's worktree) is reversible and already reviewed:
  - a spec needs the operator's approval (§1.3 of the charter);
  - code passes the gates, a pull request and the operator's merge (`specs/094`);
  - a change to a file the host runs outside the sandbox is flagged Red at merge (§5.5);
  - no agent writes a project's `.git`;
  - promotion into ratified files is barred below `internal_empirical` (§4.3).
- **An outbound action** (send, push, post, a network write, an agent-to-agent call) cannot be undone. Once data has left, no review recovers it.

The charter already treats connector writes this way (`specs/001` amendment 001-A3): a Red gate at every taint level, never a refusal, "the control is the operator's synchronous approval of the exact payload".

### 1.4 Non-goals

- **Raising taint.** Taint is still computed from the lowest-trust input and raised only by the operator.
- **Promotion.** The hard invariant of §4.3 (nothing below `internal_empirical` enters heuristics, REVIEW, lessons or skills without verification or ratification) is unchanged.
- **The never-readable and never-writable lists, `.git`, the sandbox and §5.4's destructive-action gates.** All are unchanged.
- **What leaves through exits** (141's sinks, `body_may_leave`). Unchanged.

### 1.5 Options considered

| Option | Method | What it is | Why not chosen, or chosen |
| :-- | :-- | :-- | :-- |
| A | conventional | keep §4.3; give held text by role (142 B as built) | disarms the architect and the strategist; thins the knowledge base for acting roles |
| B | constraint removed: "reading untrusted text removes tools" | drop the gate entirely, a warning only | lets an injected instruction send data off the machine with no one asked |
| C | analogy: connector writes (001-A3) | gate by reversibility: local writes proceed, marked; outbound actions ask | **chosen** |

**Simpler alternative considered:** option A with the architect and the strategist made opt-in (about 1 hour). Rejected: it keeps the knowledge base out of the roles that design and build, and it leaves every tainted dispatch stopping for an approval.

## 2. Design

### 2.1 The table (replaces `specs/001` §5.2's last row)

| Span taint | Read tools | Worktree writes / code execution | Outbound (`gh`, network, A2A) | Dispatch other agents |
| :--- | :--- | :--- | :--- | :--- |
| $L_4$–$L_5$ | ✅ | Per HITL tier | Red gate | ✅ |
| $L_2$–$L_3$ | ✅ | Per HITL tier | Red gate | ✅ (child inherits the parent's tier) |
| $L_0$–$L_1$ | ✅ | **Per HITL tier, with a notice; the written file carries the span's taint** | **Red gate, never waived** | **✅ (child inherits taint)** |

Connector writes stay a Red gate at every taint level.

### 2.2 What each change means

1. **Local writes.**
   - **What may happen:** a tainted span may write where its manifest allows and execute code inside the OS sandbox, under the ordinary HITL tier.
   - **The notice:** each such write emits one non-interactive notice in the event log and the console: `wrote after untrusted text: <path> (taint <tier>, from <source ids>)`. Nothing waits on it.
   - **No laundering.** The written file's provenance takes the span's computed taint, never the path's default. Where a path keeps no per-file provenance, the write records the taint in the file's sidecar, or the daemon's write log, which review and the merge flow show. The 001 amendment's rule becomes: a tainted span may write such a path, and the file is marked at the span's taint. It replaces "refused to any path whose origin default is above $L_0$".
   - **Unchanged:** governed paths needing $L_4$ (`provenance::gate_write`: heuristics, REVIEW, lessons, skills) stay refused; specs and other ratified paths still need the operator's approval to take effect.
2. **Outbound actions.**
   - **The gate:** a Red gate, which asks the operator synchronously, shows the exact payload and is waived by no permission mode.
   - **Approvals:** a headless run has no console, so the action is denied (§5.4). A §5.12 authorization never covers an outbound action.
3. **Dispatch.** Allowed. The child starts at the parent's taint (already so in `provenance::gate`), so the same table follows it. A dispatch is no longer an approval stop.
4. **Held text** is in every role's default search, labelled. The opt-in `include_held` stays as a no-op for compatibility, and `retrieval::held_by_default` returns true for every role.

### 2.3 Charter text (proposed; applied only by the operator's ratification)

GOVERNANCE §4.3, fourth bullet, becomes:

> - **Taint gates what leaves, and marks what stays:** a span whose computed taint is `federated_peer` or lower keeps its local writes and sandboxed execution, under the ordinary approval tier; every file it writes carries its computed taint, and each such write is noticed in the log; it may not write a file the host runs outside the sandbox, and code it runs has no network. Every outbound action it requests (network, `gh`, agent-to-agent, connector) requires a Red-tier approval of the exact payload, shown in full on the approving channel, which no permission mode waives; a dispatch it requests starts the child at its taint. Writes to the promotion paths remain barred as above.

### 2.4 Amendment 145-A1: what the reviews closed, prioritized

The reviews of §6 (OpenAI `gpt-6.2-sol`, asks `t_0014` and `t_0016`) raised a dozen items. The operator asked for only the prioritized ones (2026-11-27: "the 145 amendments seem overly limiting. suggest only prioritized changes"; then "yes"). Kept are the five that stop something that cannot be undone, data leaving the machine or code running outside the sandbox:

1. **No network for tainted commands.** Code that a tainted span runs in the OS sandbox runs with the network off. Without it, a command could send what the outbound gate guards.
2. **No tainted writes to host-run files.** A tainted span is refused writes to the files the host runs outside the sandbox: the merge flow's Red list of host-run paths (`build.rs`, `.cargo/config*`, `.envrc`, `.gitattributes`, `.gitmodules`, `.github/**`, `.vscode/**` and the like), kept as one table in code. The file tools check the normalized path, so `..` and symlinks resolve first. Such a file could run on the next build or checkout, before any review.
3. **Outbound cards show the full payload on the approving channel.** Every outbound card follows `specs/005` §3.1, as connector cards do: the resolved destination, the account, the full body and every attachment, bound to the hash of what is sent. A channel that cannot show all of it (a remote relay, §5.10) cannot approve that card.
4. **Taint follows the work.** A span's lowest taint persists through retries and resumed tasks. A dispatched child starts at its parent's taint, and its result returned to the parent is an input that taints the parent at the child's level. A span that reads a file written at a taint is taken at that taint.
5. **The order.**
   - Slice A merges with the new gate built but switched off, so the current, stricter gate stays in force.
   - The operator then ratifies slice G.
   - Then the gate is switched on.

**Considered and not adopted** (accepted risks, §7):
- commands tainted from launch because they could read a marked file;
- marks carried through copies, renames and generated files, with an unmarked file treated as untrusted;
- resolved-path checks on every write route beyond the file tools;
- every criterion tested at both $L_0$ and $L_1$ against a clean control;
- a dispatch to another provider's model treated as an exit (model calls are not exits, operator 2026-11-27).

## 3. Authority

This spec **loosens** a charter invariant, and says so.

- **Before:** an injected instruction in untrusted text could not cause any write.
- **After:** it can shape a local write, and review in front of every write is the control:
  - the operator's approval of specs;
  - the gates, pull request and merge for code;
  - the Red flag on host-run files at merge;
  - the taint mark on the file.
- **What still never happens without the operator's synchronous yes:** data leaving the machine through an action.
- **Unchanged:** promotion, the never-writable lists, `.git`, the sandbox and destructive-action gates.

## 4. Slices

| Slice | Content | Criteria | Hours |
| :-- | :-- | :-- | --: |
| G | the `GOVERNANCE.md` §4.3 diff and the `specs/001` §5.2 table, as their own gate-changing pull request, merged by the operator after ratification | AC-1 | 1 |
| A | `provenance::gate`: tainted `WorktreeWrite` → Allow; tainted `Outbound` → Red; tainted `DispatchAgent` → Allow (child inherits); the laundering check becomes a mark; the notice; 145-A1 (no network in tainted execution, host-run paths refused, full payload on the approving channel, taint that follows the work); built behind a switch that stays off until G | AC-2 to AC-5, AC-7 to AC-10 | 6-8 |
| B | held text for every role: `held_by_default` → true; `include_held` kept, documented as a no-op; 142 B's tracker and 144 §2.6 updated | AC-6 | 1-2 |
| | **Total** | | **8-11** |

Order (145-A1 item 5): A merges with the new gate switched off; the operator ratifies G; the gate is switched on.

## 5. Acceptance criteria

1. **AC-1:** `GOVERNANCE.md` §4.3 carries §2.3's text, ratified (`ratified.lock` updated), and `specs/001` §5.2 shows §2.1's table.
2. **AC-2:** a span tainted to $L_0$:
   - may write a file its manifest allows (a spec for the architect, a worktree file for the builder);
   - the written file's recorded taint is $L_0$, whatever the path's default;
   - one notice is logged naming the path and the sources.
3. **AC-3:** a span tainted to $L_0$ requesting each outbound action (`gh`, a network write, an agent-to-agent call, a connector write) gets a Red card with the exact payload:
   - a permission mode that waives other cards does not waive it;
   - in a headless run it is denied;
   - a §5.12 authorization does not cover it.
4. **AC-4:** a span tainted to $L_0$ still cannot write `context/heuristics.md`, `REVIEW.md`, any `lessons.md` or `skills/**`, nor any never-writable path or `.git`.
5. **AC-5:** a dispatch from a span tainted to $L_0$ is not a card, and the child span's ceiling starts at $L_0$.
6. **AC-6:** every shipped role's default `kb_search` returns a held source's passage, labelled; no role gets a private source; every exit test passes unchanged.

7. **AC-7:** code run in the sandbox by a span tainted to $L_0$ cannot reach the network; the same command from a clean span is unchanged.
8. **AC-8:** a span tainted to $L_0$ writing `build.rs`, `.envrc`, `.github/workflows/x.yml` or `.cargo/config.toml`, directly or through `..` or a symlink, is refused; the same write from a clean span is not.
9. **AC-9:**
   - an outbound card shows the resolved destination, account, full body and every attachment, and its approval sends only the operation whose hash it bound, once;
   - a relay that cannot show an attachment cannot approve that card.
10. **AC-10:**
    - a retry and a resumed task keep the span's lowest taint;
    - a child's result returned to its parent lowers the parent to the child's taint;
    - a span that reads a file written at $L_0$ is at $L_0$.
    - **One end-to-end test:** a tainted write; a later read by a clean span; that span's next command has no network.

## 6. Verification plan

Scripted tests in `delphi-core` (`provenance`) and `delphi-engine` (`tools`, the write path, dispatch). Before the operator ratifies slice G, a red-team review on a family other than the author's, aimed at:

- injection through a local write that later reaches an outbound path;
- a host-run file written by a tainted builder;
- taint lost across a dispatch or a retry.

## 7. What is not verified

- **Whether a tainted local write is caught by review in practice.** The taint mark and the notice make it visible; nothing measures how often a reviewer reads them.
- **Whether every outbound path goes through `provenance::gate`.** AC-3 lists the kinds; an outbound path that skips the gate would be a defect today as well. For commands, the sandbox's network-off is the boundary (145-A1 item 1).
- **Whether the host-run list is complete.** It cannot be proven complete (145-A1 item 2); a file the host loads that is not on it is a residual risk, and adding one is a one-line change.
- **The risks accepted in 145-A1** ("considered and not adopted"):
  - **Marks through copies.** A file copied or generated from a marked file by a command, not a file tool, loses the mark. Only a model reading it can act on what it says, and that read marks the reader.
  - **Clean commands over marked files.** A command a clean span runs over marked files keeps its network.
  - **Other write routes.** Write routes other than the file tools are not checked for host-run targets.

## 8. Decisions for the operator

1. The direction (decided 2026-11-27: "yes please, this direction is better").
2. Amendment 145-A1, prioritized (2026-11-27: "yes").
3. The charter text of §2.3, at ratification.

## 9. Review

- The operator approved the text on 2026-11-27, before review.
- **Red-team, §6.**
  - **Attempts:** in Delphi on Gemini, help `t_0013`, which ended with an empty answer; then OpenAI (`gpt-6.2-sol`, ask `t_0014`), which answered "do not ratify" with five P0s and one P1.
  - Its six findings were first applied in full, then prioritized (below).
- **Recheck:** OpenAI (`gpt-6.2-sol`, ask `t_0016`): "do not ratify §2.3 yet".
  - It found lasting taint closed and five items open.
- **Prioritized** at the operator's request: the five items of §2.4 are kept; the rest are accepted risks (§7). The operator accepted this on 2026-11-27.
- **Still needed:** the operator's ratification of slice G's charter diff.
