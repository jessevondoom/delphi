# Delphi documentation

Delphi is a team of AI agents in your terminal that plans, builds and reviews software with you and asks before anything leaves the sandbox. The [README](../README.md) is the tour; these pages are for doing things.

## Start here

- [Thesis](thesis.md): why Delphi exists, the claim it makes, and how to judge a change to it.
- [Getting started](getting-started.md): install, the first session, your first build.

## Guides

| I want to | Read |
| :-- | :-- |
| connect my models and subscriptions, or add a local fallback | [Model access](guides/model-access.md) |
| have the team build a change, or settle a hard decision | [Builds and Councils](guides/builds-and-councils.md) |
| run a task from a script or CI | [Headless runs and CI](guides/headless-and-ci.md) |
| answer approval cards from my phone, or wake an agent later | [Away mode](guides/away-mode.md) |
| use Delphi's roles, specs and memory from Claude Code, Copilot, Codex or agy | [Using Delphi from other tools](guides/using-delphi-from-other-tools.md) |
| share knowledge, skills and goals across a team | [Org bundles](guides/org-bundles.md) |
| build a bundle with an agent and an operator | [Building a bundle with an operator](guides/building-a-bundle.md) |
| join, run or troubleshoot the org's shared memory store (Xenia) | [Xenia operator guide](guides/xenia-operator.md), or `delphi xenia help` |
| learn what the console's colours and glyphs mean | [The console's visual language](console-visual-language.md) |
| find out why something does not work | [Troubleshooting](troubleshooting.md) |
| read the error log, or share a session for debugging | [Debugging with the error log](guides/debugging.md) |

## Reference

| What | Where |
| :-- | :-- |
| Every command and option | [reference/cli.md](reference/cli.md), the man page [reference/delphi.1](reference/delphi.1), `delphi help <command>` |
| Every `config.toml` section | [reference/config.md](reference/config.md) and the annotated template [`crates/delphi/config.example.toml`](../crates/delphi/config.example.toml) |
| Machine-readable formats | [reference/schemas.md](reference/schemas.md) and [`schemas/`](../schemas/) |
| Shell completions | `delphi completions bash`, `zsh` or `fish` |

`reference/cli.md` and `reference/delphi.1` are generated (`delphi docs cli`, `delphi docs man`); a test fails when they drift from the program.

## Design

The specifications in [`specs/`](../specs/) say what each subsystem must do and why; the [charter](../GOVERNANCE.md) and the [review rubric](../REVIEW.md) govern the agents. Release history is in the [changelog](../CHANGELOG.md); how to contribute is in [CONTRIBUTING.md](../CONTRIBUTING.md).
