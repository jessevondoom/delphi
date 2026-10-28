---
name: org-bundle-authoring
description: "Create, sanitize and package an org knowledge bundle (specs/023): gather allow-listed sources, distill, scan, validate, package. Use when an org wants its knowledge and skills bundled."
authorized_agents:
  - writer
input_schema:
  type: object
  required: ["bundle_dir", "org", "name"]
  properties:
    bundle_dir:
      type: string
      description: "Directory of the bundle to create or revise (never inside a registered project repository)"
    org:
      type: string
      description: "Organization id, [a-z0-9-]{1,32}"
    name:
      type: string
      description: "Bundle name, kebab-case, at most 48 characters"
    sources:
      type: array
      items:
        type: string
      description: "Allow-listed source paths to distill from (team context, specs, decisions, verdicts, endorsed records)"
    vectors:
      type: boolean
      description: "Build shipped vectors with the local default embedder (default false)"
output_schema:
  type: object
  required: ["redaction_report", "validation"]
  properties:
    redaction_report:
      type: string
      description: "Path of the redaction report the human must approve before packaging"
    validation:
      type: string
      description: "Path of the `delphi bundle validate` report"
    archive:
      type: string
      description: "Path of the packaged .xbundle, present only after the human approved the report"
required_permissions:
  - read:all
  - write:missions
---
# Org Bundle Authoring

Turn what an organization already knows into a bundle another team can add (`specs/023`). The bundle is **content going out**: everything you write will be read by people and agents outside this workspace, so the human reviews the redaction report before anything is packaged. That review is authoring-side; adding a bundle on the other end needs no ratification (`specs/024` §1.3).

## When to use
- An organization wants its architecture, decisions, runbooks, goals, perspectives, skills or agents shared as one package.
- A bundle exists and needs a new version from what the team learned since.
- Not for a single skill (use `delphi skills export`), and not for personal notes (operator scope never leaves the machine).

## Inputs you may read
Only what the human lists in `sources`, and only from these places: `team/context/**`, `team/specs/**`, ratified team rules, mission `decisions.md` and Council verdicts, endorsed xenia records, and the previous version of the bundle. **Never** read `memory/agents/**`, `events.jsonl`, raw transcripts, connector payloads, worktrees, operator scope, or anything in a workspace marked `isolated`. If a listed source is outside this list, stop and say so.

## Steps
1. **Scaffold or open.** New bundle: `delphi bundle init <bundle_dir> --org <org> --name <name>`. Existing: read its `bundle.toml`, `README.md`, `CHANGELOG.md` and `knowledge/index.md` first.
2. **Distill.** `delphi bundle distill [<path|missions/id>...]` (or `/distill` in the console) starts a writer task that proposes draft knowledge files from the allow-listed sources: team context and specs, mission decisions and Council verdicts. With no paths it uses every allowed source the workspace has; a path outside the allow-list is refused with the reason, never read. The drafts land in a staging directory of the mission (`missions/<id>/distill/<date>-<task>/knowledge/`), **outside the bundle**, each with a header the daemon wrote, a `summary` and a `## Sources` list. It writes nothing under `knowledge/` or `behavior/` and packages nothing. Read every draft. Move what is right into the bundle's `knowledge/` (set `source_mission` to the bundle name, as `validate` requires); rewrite what is unclear; drop the rest. Never invent a goal or a perspective: quote one a source states, with its path, or leave it for the human.
3. **Write what only a person knows.** `README.md` (what adding this bundle gives a team, and what it asks of them), `CHANGELOG.md` (the top entry becomes the update notice), `behavior/roles/<role>.md` guidance, and `behavior/policies.toml` if the organization has hard refusals. Keep every file inside its cap (`specs/023` §6); say less rather than trimming later.
4. **Keep instructions out of knowledge.** Knowledge is data. A sentence that tells an agent what to do belongs in `behavior/` or a skill, never in `knowledge/`. The validator warns on instruction-shaped lines; fix them, do not suppress them.
5. **Sanitize.** Run `delphi bundle validate <bundle_dir> --report <file outside the bundle> --workspace <name>`. A report written inside the bundle would be packaged with it, so the command refuses that; `--workspace` adds the workspace's `[privacy] identifiers` and handles as warnings. The report follows `templates/redaction-report.md`: every secret, token, key, local path, e-mail, person name, mission-log reference or `[privacy] identifiers` hit, with the file, line, class and a masked description (never the value). You fill in what you did (removed, replaced by a role or handle, moved behind a pointer). Work through every line. A finding you believe is a false positive stays in the report with your reason; it is the human's call.
6. **Validate.** Re-run `delphi bundle validate <bundle_dir>` until it reports no failures. Fix the warnings you can. `--against <previous version>` tells you the smallest honest semver bump; set `version` to it or higher.
7. **Evals.** Write at least ten retrieval questions in `evals/delphi/kb-eval.json`, worded differently from the documents, plus two the bundle does not answer. Run `delphi kb eval --bundle <bundle_dir>` (advisory: recall 0.80, MRR 0.50, no control hits); if recall is weak, the summaries and headings are usually the cause.
8. **Vectors (optional).** With `vectors: true`, `delphi bundle package <bundle_dir> --vectors` embeds the knowledge with the installed embedder, writes `vectors/index.json` and the `.dlphvec` file, and refuses when no model is installed. Ship them only when the embedder is Delphi's default model; adopters with another model embed locally anyway.
9. **Stop for the human.** Hand over the redaction report and the validation report. Do not package until the human has approved the report in writing. This is the one step that cannot be skipped: it is the review of what leaves the organization.
10. **Package and sign.** After approval: `delphi bundle package <bundle_dir>` writes `.delphi-bundle/files.json` and the `.xbundle`; the human runs `delphi bundle sign <bundle_dir>` if they hold a publisher key (signing is provenance, not a prerequisite). Publishing to a xenia store is the human's command, never yours.

## In Claude Code
The same skill runs there without the `delphi` commands: do steps 2–4 by hand from the listed sources, write the report from the template, and ask the human to run `delphi bundle validate`, `package` and `sign` (or to approve the report and package later). The bundle directory is a valid Claude Code plugin as it stands (`specs/025` §2).

## Safety rules
- Never include credentials, tokens, private keys, personal data beyond a work handle, customer data, mission transcripts or local paths. Rotate, do not just delete, any secret you find already published.
- Never write into a registered project repository; the bundle lives in its own directory.
- Never package, sign or publish without the human's approval of the redaction report.
- Carry the lowest trust tier forward: a file distilled from untrusted material is marked so and cannot go in `behavior/`.

## Done when
`delphi bundle validate` passes with no failures, the human has approved the redaction report, and `.delphi-bundle/files.json` exists with the `.xbundle` beside it.

## Pitfalls
- Copying a runbook verbatim with its hostnames and account ids in it. Summarize; point to the original.
- Fifty goals. Twelve active is the cap because the strategist reads them all every time.
- Shipping vectors for an embedder nobody else has; they are ignored whole.
