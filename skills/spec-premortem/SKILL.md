---
name: spec-premortem
description: "Assume the design has shipped and failed, then list the most likely causes, their early signs and what the spec changes now. Use on a draft spec before it goes to review or ratification."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["spec"]
  properties:
    spec:
      type: string
      description: "Path of the draft spec"
    horizon:
      type: string
      description: "How far ahead to imagine the failure, for example three months after release"
output_schema:
  type: object
  required: ["premortem"]
  properties:
    premortem:
      type: string
      description: "The ranked failure causes, each with an early sign and a response, and the changes made to the spec"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Spec Premortem

Asking "what could go wrong?" invites reassurance. A premortem starts from the other end: the design was built as specified, and it failed. The work is to explain why, while the spec can still change.

The premortem is a known practice from decision research. What is written here was taken from secondary summaries, not from the primary work. Until the operator ratifies primary sources (spec 124, OD-3), cite no author as an authority for it, and make no claim about how much it improves outcomes.

This is the author's own check. It does not replace Red-Team's review and does not count as an independent one.

## When to use
- A draft spec is complete and about to be handed to review or to the operator.
- A design carries a one-way decision (see `adr-authoring`).
- Not on a half-written spec: there is nothing definite to fail yet. Not as a reason to delay a small, reversible change.

## Steps
1. Fix the frame in one sentence: "It is <horizon> after this shipped as written, and it has failed." Say what failure means here: unused, wrong, unsafe, too slow, too costly in tokens, or abandoned half-built.
2. Write at least five distinct causes, as past-tense stories ("the cache served stale records after a restart, and nobody noticed for a week"). Cover more than one kind:
   - the requirement was wrong or was not the operator's real need;
   - an assumption in the spec was false (list the spec's assumptions and fail each in turn);
   - an interaction with an existing part (read the specs this one links);
   - the build drifted from the spec, or a slice was never finished;
   - an authority or trust boundary was crossed;
   - it worked and nobody could tell (no measure).
3. Rate each cause for likelihood and for damage, high, medium or low, and give one phrase of reason. Do not rate from mood: point at the line of the spec or the fact that makes it likely.
4. For the top three, write the **early sign**: what would be observed first, and where (a test, a log line, a count, an operator's remark).
5. For the top three, choose a response and make it in the spec now: change the design, add an acceptance criterion or a check (`fitness-functions`), narrow the scope, or accept the risk in writing with its early sign and who watches for it.
6. Record the premortem as a short section of the spec or a note beside it, including the causes not acted on.

## Output format
```markdown
### Premortem (<horizon>): how this most likely fails
| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |

Not acted on: <cause numbers and why>
```

## Done when
At least five causes of more than one kind are written as stories, each rated with a reason; the top three each have an early sign and a response that is visible in the spec's text; and the causes left alone are listed.

## Pitfalls
- Generic risks ("performance may be poor"). A cause names the part and the event.
- Only technical causes. Many designs fail because the requirement was wrong.
- A list with no change to the spec. If nothing changed, say why each top cause is accepted.
- Using the premortem to argue the design is safe. It finds causes; it proves nothing.
- Presenting it as a review. The author cannot be the independent reviewer of the author's own spec.
