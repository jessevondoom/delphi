# Thesis: why Delphi exists

Delphi is a harness, in the family of Codex and Claude Code, for **research, strategy, brainstorming, business analysis and product work**.

AI for code has a solid process: a test either passes or it does not. Research and strategy have no such oracle. The field is non-deterministic, sources disagree, facts go stale, and a confident wrong answer looks the same as a right one. Tools for that work are far less developed. Delphi is built for it.

## The claim

> Teams make better decisions when every person works from the same *trusted* understanding, and that understanding grows every time anyone does research.

## How the pieces serve it

| Idea | What it means | Where it lives |
| :-- | :-- | :-- |
| **One operator, one agent team** | Each person runs their own install and their own team. No server, no shared login. | [README](../README.md) |
| **A shared baseline** | Every install starts from the same org knowledge, skills, goals and standards, so people working alone stay aligned. | [Org bundles](guides/org-bundles.md) |
| **Xenia** | The protocol that moves signed knowledge and lessons between installs, so the baseline stays current. | [Xenia operator guide](guides/xenia-operator.md) |
| **Trusted knowledge** | A claim carries its source, its trust level and its validation. Content read from outside never raises its own trust. | specs 040, 043, 044 |
| **A knowledge base that compounds** | Each research run mines trusted sources and leaves the base better than it found it. | specs 004, 028, 043, 044 |
| **Argument built in** | The Council answers blind, cross-examines, and reports the strongest dissent, because agreement among models is not evidence. | [Builds and Councils](guides/builds-and-councils.md) |
| **Learning with a leash** | Agents can learn and new behaviors can emerge, but a lesson reaches the shared baseline only through review. | spec 031 |

## The team

The team is centered on the roles that do this work: `scout` (finds sources), `investigator` (digs into one question), `strategist` (turns findings into options), `writer` (turns them into something a reader can use), and the Council (argues the hard calls). `orchestrator` runs them.

Code roles stay close: `builder`, `architect` and `red-team`. Code is critical, and a strong builder keeps the engineering perspective in the room when a strategy has to be made real. They are one voice on the team, not its purpose.

## How to judge a change to Delphi

Ask four things. A change that serves none of them is probably not needed.

1. **Trust.** Can every claim be traced to a source, a trust level and a validation record?
2. **Compounding.** Does the shared base get better from this work, and is that measured?
3. **Alignment.** Does an operator working alone stay current with everyone else's baseline?
4. **Honest uncertainty.** Does the output show disagreement, gaps and age, rather than hide them?

## What would prove this wrong

- Operators accept the baseline without reading it, and errors spread faster than they are caught.
- Research quality does not measurably improve with a larger base.
- Lessons diverge between installs until "shared" is only a label.

These are tested by the evaluation work (spec 037) and the live trials the operator runs. Until then, most of the above is designed and built, not proven. [Status and limitations](../README.md#status-and-limitations) lists what is proven.
