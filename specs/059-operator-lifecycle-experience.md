---
id: spec-059-operator-lifecycle-experience
title: "059: A smooth operator lifecycle from install through everyday use"
author: architect
timestamp: 2026-10-30T00:00:00Z
status: approved   # approved for implementation by the operator's request on 2026-10-30: "craft the specs, commands, and user flows ... start coding the rest"
approved_by: operator (2026-10-30)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, onboarding, installer, startup, update, operator-experience]
---

# Spec 059: A smooth operator lifecycle from install through everyday use

> Approved by the operator on 2026-10-30 for the lifecycle work described below, excluding upgrade rollback and recovery. Builds on [`specs/006`](006-installer-and-updater.md) (install, preflight and update), [`specs/054`](054-first-minute.md) (first minute), and [`specs/058`](058-project-scopes-scaffolding-and-promotion.md) (always-available execution and layered project scope). This spec aligns their user-facing flow; it does not relax verification, ratification, taint or approval gates.

## 1. Goal and operator promise

The operator installs Delphi once, runs it from any directory, and reaches useful work without having to understand workspace internals. Project registration adds local project features; Git is optional. A returning operator gets a quiet launch. An update clearly states what it changes and never silently installs a new executable during ordinary startup.

The one-line model is:

> Install once. Run anywhere. Register a directory when you want its own project context. Delphi shares home memory everywhere, adds the active project's memory, and asks before consequential changes.

## 2. Lifecycle states

| State | Entry | Result |
| :-- | :-- | :-- |
| Not installed | Installer or package manager | Verified CLI and console installed; clear next action printed. |
| Installed, no home | First `delphi` | Home/default workspace initialized; first-run setup offered; console remains reachable if setup is deferred. |
| Home ready, no model | `delphi` or `delphi setup` | Guided model setup can resume; operator can use non-model local features where available. |
| Home ready, no registered project | `delphi` in any directory | Uses home plus the persistent default-project layer; does not register or write to the current directory. |
| Registered project | `delphi project register/init` | Uses home plus project context and memory from the root or any descendant; creates only explicitly authorized scaffold. |
| Returning healthy install | `delphi` | Lightweight preflight and console, without repeating setup or full doctor output. |
| Update available | `delphi update --check` or an informational notice | Shows version and concise release summary; no binary changes. |
| Update requested | `delphi update` | Verifies and stages the selected release, refreshes eligible built-ins according to 006, and reports completion and restart state. Recovery/rollback behavior remains governed by existing 006 and is not expanded by this spec. |

## 3. Install experience

### 3.1 Supported paths

The installer script and Homebrew remain supported. Both install the CLI and full-screen console when the release contains them, verify the artifacts according to the release trust policy, and report the installed version and next command. The documented next command is `delphi`; the operator may run it from any directory.

The installer must not require a project, Git repository, model credential, or write into the current directory. If run interactively, it may offer to launch `delphi` after installation. Declining only ends the installer; it does not start another partial setup flow. Noninteractive output gives the same next action without prompting.

### 3.2 Verification and authentication

- A valid checksum remains mandatory and is never described as proof of publisher identity by itself.
- The installer verifies release provenance using the strongest supported, non-interactive verification path for the published release. It must not require the operator to sign into a general-purpose GitHub CLI solely to install a public release when the attestation is publicly verifiable.
- If provenance cannot be verified, installation stops with a specific reason and changes no installed executable. Any weaker verification escape remains explicit, opt-in, and visibly labelled as weaker; it is never selected automatically.
- Mirror/source overrides retain their HTTPS and warning rules. No release key or credential is embedded in a public installer.

### 3.3 Completion message

Success reports the installed version and path, whether the install directory is on `PATH`, whether the full-screen console is present, and `Next: run delphi in any directory.` It must not instruct the operator to `cd` into a repository. If setup was deferred, the first-run command remains `delphi`; `delphi setup` is available to resume it explicitly.

## 4. First-run onboarding

### 4.1 One front door

Bare `delphi` owns first-run onboarding. It resolves or creates home/default-project state, determines whether model access is already usable, and offers only the missing setup. `delphi init`, `delphi setup`, and `delphi doctor --fix` remain available for explicit administration, but README and installer guidance must not imply that all three commands are required before first use.

### 4.2 Project choice

On an interactive first launch, if the current directory is not already within a registered project, the operator may:

1. **Use the default project** and continue without writing to the current directory; or
2. **Make this directory a project**, with or without Git, preview the exact scaffold from 058, then explicitly register/init it.

The choice is asked at most once per directory identity/session state. A declined choice does not block use and is not repeatedly prompted on every launch. The operator can later run `delphi project register` or `delphi project init`. Bare invocation never creates project files or assumes that a Git root is the intended project boundary.

When inside an already registered project, no project-choice question is shown. Subdirectories resolve to that project per 058. For an existing Git-root workspace created under 006, migration preserves its mapping and explains any one-time identity/scaffold action before writing project files.

### 4.3 Model setup and first useful interaction

- Detect configured and usable model paths using the existing credential-safe probes. Never read provider CLI credential files or display secret values.
- If a model is available, proceed to the console. If none is available, offer guided setup once. Declining opens the console when a useful local path exists; otherwise show a concise explanation and the exact resume command.
- Setup can be resumed and is idempotent. A partial failure reports what completed and how to continue; it does not erase the home or require restarting from the beginning.
- Do not require a tour, project summary, research answer, or optional preference before opening the console. Follow 054's first-use principle: get to the operator's actual task quickly. Any proposed first question or automatic research capture remains optional and governed by 052.
- Project setup and model setup are separate choices. Either may be deferred without blocking the other.

### 4.4 Noninteractive behavior

Piped, CI, `--no-onboard`, and `DELPHI_NO_ONBOARD` paths do not prompt or scaffold project files. They may create the central home/default workspace as already specified in 006 and print concise next-step guidance. Existing scripting exit codes and command semantics remain stable. Explicit `delphi project init --yes` is the only noninteractive path that creates project scaffold files.

## 5. Routine startup

- Resolve workspace, home layer and project/default layer before opening the console, using 058 for project scope.
- When healthy for the current release and workspace, perform only the cheap checks needed to attach safely; do not run a full doctor report or repeat onboarding.
- Run the deterministic doctor on first use for a release and when the cached health state is invalidated by a relevant change. Keep doctor read-only by default. `doctor --fix` asks before each fix, retains the Red-card rules, and never bulk-ratifies protected content.
- A failure that blocks safe operation is reported before the console with a concise explanation and a remediation command. Warnings that do not block operation appear as a compact notice in the console and remain available in `doctor`.
- A declined optional remedy is remembered for that preflight and is not asked twice in one launch. Declining setup never counts as a successful model configuration.
- Startup notices and progress obey reduced-motion and `NO_COLOR` rules (AGENTS.md and 053). A healthy launch has no decorative delay or animation requirement.

## 6. Update experience (excluding rollback/recovery redesign)

- `delphi update --check` is read-only and states the current and available versions, release channel, concise change summary, verification state, and affected workspace refreshes.
- `delphi update` is the operator's explicit request to install the selected update. It verifies before replacing files and clearly reports binary, built-in refresh and daemon-restart outcomes. The signed-release refresh and ratification behavior in 006/036 remains unchanged.
- Ordinary `delphi` startup may show a non-blocking update-available notice if the update metadata is already available locally. Startup must not fetch or install a binary solely to display the notice. No update is applied without an explicit update command or a separately ratified future policy.
- `--no-restart`, `--review`, channel selection, and existing update options retain their current meanings. This spec does not change rollback, recovery, backups, partial-failure repair, or the transactional guarantees of the updater. Those require a separate spec and operator review.
- Workspace/project data, credentials, local memory and active work are not update targets. Update output identifies proposals and any operator action still owed.

## 7. Commands and discoverability

| Command | Operator intent |
| :-- | :-- |
| `delphi` | Start or resume onboarding, then open the console from any directory. |
| `delphi setup [--check]` | Configure or inspect model access; safe to resume. |
| `delphi project register [path]` | Give a directory a distinct project scope without requiring Git. |
| `delphi project init [path] [--yes]` | Create the approved project scaffold after showing what will be written. |
| `delphi project scope` | Show home plus active project/default scope and why it resolved. |
| `delphi doctor [--fix]` | Inspect health; with `--fix`, offer separately approved repairs. |
| `delphi update --check` | Preview an available update without changes. |
| `delphi update` | Apply the selected, verified update and report its effects. |

Help, command reference, installer output, README and Getting Started use this same vocabulary and sequence. Do not tell new operators to run `init → setup → doctor → chat` as mandatory separate steps.

## 8. Safety and privacy invariants

- Project choice and scaffold never occur silently. The project directory remains untouched on bare launch, noninteractive launch, and declined registration.
- Credentials are queried only through approved credential sources and existing harness status interfaces. Setup does not inspect third-party login stores.
- Project/local content is untrusted input; onboarding cannot ratify it or widen tool, network or sandbox authority.
- A model call, network probe, installer verification, update check or release install is disclosed at the step where it occurs. No model call is made solely to show onboarding UI.
- No health cache can suppress a new hard failure, a changed protected file, a revoked credential, or a required release verification.

## 9. Slices

| Slice | Deliverable | Depends on |
| :-- | :-- | :-- |
| OE1 | Align user-facing lifecycle docs and installer completion language; make the default-workspace setup path resumable from any directory | 006, 054, 058 |
| OE2 | First-run choice for default vs explicit directory project; resumable project/model setup; preserve no-write behavior unless explicitly chosen | 058 PS1, PS3; 006 |
| OE3 | Startup health-state rules and concise blocking/warning notices; avoid duplicate setup/doctor prompts | 006-A4; 058 PS1 |
| OE4 | Installer provenance verification without a GitHub CLI login prerequisite, preserving fail-closed verification | release attestation format; installer |
| OE5 | Update preview/notice alignment and clear effects; no rollback/recovery redesign | 006, 036 |
| OE6 | Integration, generated references, getting-started and terminal walkthrough | OE1–OE5; 053, 054 |

Each slice is a separate PR per repository rules. Work on OE5 excludes rollback and recovery. Those topics remain explicitly deferred.

## 10. Verification

- Install completion is correct in interactive and noninteractive cases and says Delphi can run in any directory.
- A public release installs with artifact and provenance verification without requiring `gh auth login`; tampered artifact, missing checksum, invalid/absent required attestation, and unsupported platform leave installed files untouched.
- First launch with model access, without model access, deferred setup, a Git project, an unregistered Git directory and a non-Git directory follows §4. A project choice is not repeated after decline; no project files are created without explicit registration/init.
- Explicit project init works without Git, previews paths, preserves existing files, and produces the 058 layout.
- Repeated healthy startup does not repeat onboarding or print the full doctor report; a blocking finding is still surfaced; warnings remain inspectable.
- Piped/headless launch does not prompt or write project files; exit codes remain stable.
- Update check changes nothing. Update output distinguishes binary replacement, workspace refresh and daemon restart. No rollback/recovery behavior is claimed by this spec.
- Output respects `NO_COLOR` and reduced-motion settings.

## 11. Decisions (operator, 2026-10-30)

- **D-1 — One front door:** bare `delphi` is the path from first launch to everyday console use; setup is resumable.
- **D-2 — Project choice is explicit:** a directory can be registered with or without Git; bare launch never writes project files.
- **D-3 — Defer safely:** model and project setup may be deferred independently without preventing use of available functionality.
- **D-4 — Quiet returning launch:** full doctor is cached for healthy state; concise notices surface only actionable changes or blockers.
- **D-5 — Update is deliberate:** show an available update without installing it; `delphi update` applies it. No hidden startup download/install.
- **D-6 — Strong install verification:** do not require GitHub CLI login for public artifact attestation; fail closed if provenance cannot be checked. A weaker opt-in is clearly marked.
- **D-7 — Recovery excluded:** no new updater rollback, backup, recovery or partial-failure guarantees are authorized here.
