# Building a bundle with an operator

For a coding agent that has been pointed at these docs and asked to build an [org bundle](org-bundles.md) *with* a person. It says what to read, what to ask, where each answer goes, and where you must stop. The format is [`specs/023`](../../specs/023-org-bundle-standard.md); the commands are in the [command reference](../reference/cli.md); the roles and skills a bundle can attach to are in the [catalog](../reference/catalog.md).

A bundle is **content going out**: people and agents outside this workspace will read every file. You draft; the operator decides. A bundle cannot claim trust for itself and grants no permission (the operator's `add` is what makes its contents trusted, shown `validated · vouched by <org>`), so nothing you write can widen what an agent may do.

## Read first (in this order)

1. [Org bundles](org-bundles.md): the commands and the layout.
2. [`dev/bundles/acme-platform`](../../dev/bundles/acme-platform): a golden bundle that passes `validate`. Copy its shapes.
3. [`skills/org-bundle-authoring/SKILL.md`](../../skills/org-bundle-authoring/SKILL.md): the ten steps and the safety rules. This guide is the interview around them.
4. [Catalog of roles and skills](../reference/catalog.md): the role ids a bundle may name. Roles are named by id (`builder`, `red-team`), never by a theme's display name.
5. Only when needed: `specs/023` (format and caps), `specs/024` (trust and precedence), `specs/025` (Claude Code plugin interchange), `specs/027` (authoring, vectors, evaluation).

## Ground rules

- Work in a **new directory outside any project repository** (`delphi bundle init <dir> --org <org> --name <name>`). Scratch runs use `--home <tmpdir>`; bare `delphi` creates a real workspace.
- Read only what the operator lists. Never read agent memory, event logs, transcripts, connector payloads, worktrees or operator scope.
- Knowledge is **data**. A sentence that tells an agent what to do belongs in `behavior/` or a skill.
- Never invent a goal or a perspective. Quote a source with its path, or leave the slot for the operator.
- Never package, sign or publish without the operator's written approval of the redaction report.

## The interview

Ask these in order, one group at a time. Write the answers into the file named on the right as you go, then run `delphi bundle validate <dir>` after each group so errors stay small.

| Ask | Goes in |
| :-- | :-- |
| Which organization (id: `[a-z0-9-]{1,32}`), what name (kebab-case, at most 48 characters), who publishes it, who is the contact? | `bundle.toml` `[bundle]`, `[publisher]` |
| Who is it for: the whole org, one team, one project? Which repos, technologies and paths does it apply to? | `audience`, `[applies_to]` (routing, not secrecy) |
| What should an agent be able to *look up*? Architecture, decisions and why, runbooks, glossary, gotchas. Which existing documents are safe to distill from? | `knowledge/`, plus the allow-list for `distill` |
| What do you do the same way every time, as a procedure? | `skills/<name>/SKILL.md` |
| Is there a reviewer or specialist persona the org already uses? Which host role should it run as? | `agents/<name>.md` and `[agents.<name>] role = "<role id>"` |
| What does each planning role need to remember about *this* org? (a few lines each) | `behavior/roles/<role>.md` |
| What does the org want achieved this year? What does it believe about how to build? | `behavior/goals.md`, `behavior/perspectives.md` (quoted from sources) |
| Is there anything an agent must never do here? (commands, paths, connector tools) | `behavior/policies.toml` (it can only refuse) |
| How often is it reviewed, and what is the licence? | `review_by`, `license` |
| Is this for inside the org only, or will it be published? | `distribution` (`internal` default, `public`); licence on each source is information and only produces warnings; never ship a source the operator flagged `private` |
| What questions should it answer? Write ten in the operator's own words, and two it should *not* be able to answer. | `evals/delphi/kb-eval.json` |

### Where does this content go?

| It is... | Put it in | Because |
| :-- | :-- | :-- |
| a fact, a decision, a runbook, a definition | `knowledge/` | searchable data; cannot instruct |
| a repeatable procedure with inputs and outputs | `skills/` | loads for the roles it names |
| a persona that runs as an existing role | `agents/` | a span of the host role; its tools are intersected with the host's |
| a nudge for one role (2 KiB at most) | `behavior/roles/<role>.md` | instruction text, scanned for "skip approval" and the like |
| a state of the world the org wants | `behavior/goals.md` | at most 12 active, read by the strategist |
| a refusal | `behavior/policies.toml` | tighten-only; `allow` keys are refused |

If a thing needs a **new tool, a wider scope or a new role**, it does not belong in a bundle. Tell the operator; that is a spec change.

## Gather before you write

1. `delphi bundle list` and `delphi bundle store <org>/<store>` show what the operator already has or could add. Reuse, or `bundle update`, before writing something new.
2. `delphi bundle distill [<path|missions/id>...]` drafts knowledge from allow-listed sources into a **staging directory outside the bundle**. Read every draft; move what is right; rewrite or drop the rest.
3. `delphi bundle import <plugin dir>` turns a Claude Code plugin the org already has into a bundle (hooks, commands and `bin/` are never imported).
4. `delphi kb search "<question>"` shows what the workspace already knows.

## Checkpoints where you must stop

1. **After the interview**: show the operator the file list you plan to create.
2. **After `distill`**: the operator reads the drafts you propose to keep.
3. **After `validate --report <file outside the bundle> --workspace <name>`**: hand over the redaction report and the validation report. The operator approves in writing. **Nothing is packaged before this.**
4. **After `package`**: the operator signs (`delphi bundle sign` needs a terminal) and publishes. Signing and publishing are theirs.

## Prove it works

```sh
delphi bundle validate ./acme-platform --against ./acme-platform-previous   # smallest honest version bump
delphi kb eval --bundle ./acme-platform                                      # advisory: recall >= 0.80, MRR >= 0.50, 0 control hits
delphi bundle add ./acme-platform --dry-run --home "$(mktemp -d)"            # what an adopter would see
delphi bundle export acme-platform --claude-plugin ./plugin                  # optional: the Claude Code form
```

If recall is weak, the cause is nearly always the `summary` (160 characters) and the headings. Fix those before adding documents.

## Not built yet

Agent registry, routing and `[extends]` for role extensions ([`specs/041`](../../specs/041-agent-registry-routing-and-role-extensions.md)) are approved but unbuilt: a bundle agent today is a span of one host role. There is no interactive `bundle wizard`; this interview is the procedure. `delphi bundle search` does not exist; use `bundle list`, `bundle store` and `kb search`.
