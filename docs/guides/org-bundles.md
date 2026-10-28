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

## Share through your organization's store

If your organization runs a xenia store (a signed, git-hosted store you have joined), a bundle can be published to it as a `bundle` record that points at a git tree. The record carries no files; it names the tree, its hash and what the bundle touches.

```sh
delphi bundle publish ./acme-platform --to acme/acme-main --url https://git.acme.example/platform/bundle.git --pin git-tree:<tree id>
delphi bundle store acme/acme-main                    # what the store offers, at what level, and what you have
delphi bundle add <record id> --from-xenia acme/acme-main
delphi bundle update                                  # follows the record's newer revisions, forward only
```

`delphi xenia publish --bundle <dir> --store acme/acme-main ...` is the same command, and `delphi xenia sync --auto-update-bundles` is the same as `bundle store ... --auto-update` after a sync. `publish` needs a bundle you have signed with the key this install uses, shows you the approval card with the exact record bytes and asks once (type the code on the card). `add` checks the store first, fetches the tree under the same limits as a marketplace add (no links, no submodules, size caps), and refuses it unless it hashes to the record, is signed by the member who published the record, and belongs to your organization. The level it records is the store's: **endorsed** (a steward of another person endorsed these exact bytes, under a current freshness statement) or **member-signed**. It is information, never a gate. If the store's statement goes stale, `delphi bundle store acme/acme-main --auto-update` brings the lock's level down with it, and applies only the updates that keep the same publisher and do not lower the level.

## Move between Delphi and Claude Code plugins

```sh
delphi bundle import ./some-plugin -o ./some-bundle --agent-role reviewer=red-team --agents reviewer
delphi bundle export acme-platform --claude-plugin ./plugin [--strip-delphi]
delphi bundle export acme-platform --agents-md [file]   # a knowledge-only AGENTS.md fragment
```

`import` prints a table of what was imported, what was ignored and why. `hooks/`, `commands/` and `bin/` never run or load; `.mcp.json` servers become inert declarations with every value dropped; agents need a host role from you (`--agent-role name=role`) and skills need `--agents`. It never adds: you validate and add the new directory yourself.

## What is not built yet

Archive tree pointers for store records, `i8` vectors, vectors in `bundle serve`, the evaluation's blind grader and any live paid evaluation run, and the agent registry and role extensions of [`specs/041`](../../specs/041-agent-registry-routing-and-role-extensions.md). Building a bundle with an agent: [Building a bundle with an operator](building-a-bundle.md).
