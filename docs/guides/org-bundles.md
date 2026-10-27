# Org bundles

An **org bundle** is a directory an organization publishes so every install starts from the same knowledge, skills, agents and stated goals. It is a superset of a Claude Code plugin. A bundle carries content, never authority: it states no trust and grants no permissions. The design is [`specs/022`](../../specs/022-xenia-sharing-overview.md) to [`027`](../../specs/027-bundle-authoring-distribution-and-evaluation.md); the format is [`schemas/bundle.schema.json`](../../schemas/bundle.schema.json).

## Use a bundle

```sh
delphi bundle add ./acme-platform                      # a directory, or a .xbundle archive
delphi bundle add --from-marketplace acme/plugins incident-kit
delphi bundle list                                     # what is added, and how far its vectors are
delphi kb search "rolling deploy" --bundle acme/acme-platform
delphi bundle update [name] | rollback <name> [--to <hash>] | remove <name> | gc
```

`add` verifies any signature and the publisher's continuity, prints what the bundle brings, copies it into `~/.delphi/bundles` and makes its knowledge searchable. **No model is called.** A new publisher key needs `--replace-publisher`; `--dry-run` shows the result without adding, and `--yes` skips the confirmation. A vector cache the bundle ships is used only if it matches your embedder and passes a local spot check (`--no-vectors` skips it); the rest are embedded in the background, and until then results say `(lexical only: vectors N% ready)`.

Once added, a bundle's guidance, rules and skills reach the planning roles' prompts; its deny policies apply; `/org` in the console lists the bundles, their goals, perspectives and open items; and `/council --lenses` gives each seat one of the bundle's perspectives. A bundle can add guidance and policies; it cannot widen what an agent may do (see [`specs/024`](../../specs/024-bundle-trust-adoption-and-precedence.md) for precedence).

## Write a bundle

```sh
delphi bundle init ./acme-platform --org acme --name acme-platform   # a skeleton that validates at once
delphi bundle validate ./acme-platform [--claude] [--against <previous dir>] [--report r.txt]
delphi bundle manifest ./acme-platform [--claude]
delphi bundle package ./acme-platform -o acme-platform-1.4.0.xbundle [--vectors]
```

A bundle is a `bundle.toml` plus directories for the sections you turn on:

```
acme-platform/
├── bundle.toml          identity, publisher, what it applies to, which sections exist
├── knowledge/           Markdown the team's agents can search (architecture, decisions, runbooks, glossary)
├── skills/              agentskills.io skills
├── agents/              bundle agents, each mapped to a host role in bundle.toml
├── behavior/            goals, heuristics, perspectives, role guidance, policies.toml
├── evals/               checks that the bundle improves answers (a knowledge eval, tasks)
└── vectors/             optional precomputed embeddings (package --vectors)
```

`bundle.toml` in brief (the golden example is [`dev/bundles/acme-platform`](../../dev/bundles/acme-platform)):

```toml
[bundle]
schema = 1
name = "acme-platform"        # ^[a-z0-9][a-z0-9-]{0,47}$
org = "acme"
version = "1.4.0"             # semver
title = "Acme Platform Engineering"
description = "How Acme builds and runs its platform."   # plain text, 200 characters at most
license = "Proprietary"
audience = "org"              # org | team | project | person
review_by = "2027-01-31"      # past this date every surface shows the bundle as stale

[publisher]
name = "Acme Platform Engineering"

[sections]
knowledge = true
skills = true
```

What `validate` enforces, so you know what to expect: unknown keys are errors (an ignored key is a way to smuggle content); keys that state trust or grant authority (`tier`, `trust`, `permissions`, `allowed_tools`, `hooks`, `env`, `secrets`, `mcp_servers`, `credentials` and a few more) are refused anywhere in the file; a section set to `true` needs its directory; and the redaction scan looks for secrets, paths, addresses and names. `--report` writes the masked findings to a file *outside* the bundle. `--against <previous dir>` checks that your version bump matches the changes. Exit codes: 0 pass, 1 failed, 2 usage.

Have the writer draft knowledge for you with `/distill` in the console (or `delphi bundle distill`). It reads only allow-listed sources (team context and specs, mission decisions, Council verdicts) into a staging directory of the mission and writes nothing into a bundle; you review and move what you keep.

## Sign and distribute

```sh
delphi bundle sign ./acme-platform                     # at a terminal; signs with this install's key
delphi bundle package ./acme-platform -o acme.xbundle  # a reproducible archive
delphi xenia whoami                                    # which key this install signs with
```

Share the `.xbundle`, or the directory in a git repository. To publish through a marketplace file that pins every bundle to a commit:

```sh
delphi bundle export acme-platform --marketplace ./out --repo acme/plugins --sha <commit>
```

## Move between Delphi and Claude Code plugins

```sh
delphi bundle import ./some-plugin -o ./some-bundle --agent-role reviewer=red-team --agents reviewer
delphi bundle export acme-platform --claude-plugin ./plugin [--strip-delphi]
delphi bundle export acme-platform --agents-md [file]   # a knowledge-only AGENTS.md fragment
```

`import` prints a table of what was imported, what was ignored and why. `hooks/`, `commands/` and `bin/` never run or load; `.mcp.json` servers become inert declarations with every value dropped; agents need a host role from you (`--agent-role name=role`) and skills need `--agents`. It never adds: you validate and add the new directory yourself.

## What is not built yet

`bundle serve`, the evaluation runner (B8, B11) and distribution slice B12 are specified, not built; see the README's [Status and limitations](../../README.md#status-and-limitations).
