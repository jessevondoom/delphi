---
id: spec-071-host-plugin-marketplace-and-durable-mcp-enablement
title: "071: Host plugin marketplace and durable MCP enablement for Claude Code, Codex, Antigravity and Copilot"
author: architect
timestamp: 2026-11-03T22:50:00Z
status: approved   # by the operator, 2026-11-04 ("yeah build as much as possible"), after three Council reviews: approves E1a, E1, E1b, E2 (Q-8 answered) and, by the amendment of 2026-11-04 ("build all the rest", "don't pin a version update"), E4a and E4c as documentation and hints only (Q-1 answered: no new host directories). E1s, E3, E4b, E4d and E5 are not approved (§10).
approved_by: operator (2026-11-04)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, mcp, host-agents, plugins, marketplace, claude-code, copilot, codex, agy, distribution]
---

# Spec 071: Host plugin marketplace and durable MCP enablement

> **Status: approved by the operator 2026-11-04 for E1a, E1 and E1b; amended after three Council rounds (2026-11-03).** Round 1 (Delphi Council `t_0001`; no seat could read the file): E1-E2 only, defer the installer, reject the launcher. Round 2 (three Copilot-run seats that read the text): amendments applied. Round 3 (three seats, independent review then deliberation on every suggestion; all APPROVE-WITH-AMENDMENTS; §12): `delphi connect` and the stable path leave E1, E1a goes first, the production update channel follows protected release tags by operator decision D-5(B). **This spec approves only E1a, E1, E1b and E2 (Q-8 answered 2026-11-04)** (as gated in §10); the operator-UX items of §5.4 ride on those slices. The installer, `delphi connect`, the stable path (E1s), the launcher and the other hosts' directories are not approved by it.
>
> **Based on:** Extends [`specs/062`](062-delphi-as-host-agent-mcp-server.md) §10 (`--print-config`, `delphi plugin export`) and [`specs/025`](025-plugin-skill-and-tool-compatibility.md). Amends nothing. 062 §10 ("nothing is installed for you") stands unchanged.

## 1. Problem & Intent

Delphi is offered to four hosts (Claude Code, Codex, Antigravity/`agy`, Copilot CLI) by `delphi mcp`. Enabling it today is a copy-paste:

1. `delphi mcp --print-config <host>` prints a snippet; the operator pastes it into a host file.
2. Every snippet runs a bare `delphi`, so it works only where the host's `PATH` contains the binary. GUI-launched hosts and a moved or upgraded binary break it silently.
3. Claude Code's snippet is a per-project `.mcp.json`; it is not on everywhere.
4. Nothing notices when an entry is missing, stale or wrong. `delphi doctor` only points at the print command.
5. `delphi plugin export` writes a one-off directory (`delphi-plugin/` at the repo root is one such stray) that the operator must place and re-export on each release; plugin and binary versions drift.

**Intent (what E1a, E1 and E2 deliver, and no more).** (a) A cross-host absolute-path snippet with the host's own install command beside it (E1a); (b) a Claude Code plugin installable with the host's own marketplace command, that follows release tags (E1); (c) every generated plugin and marketplace file housed in this repository, generated from the code that defines the snippets so the two cannot disagree; (d) a best-effort settings check of the Claude Code setup with a copy-paste fix for a missing, stale or incompatible plugin (E2); (e) a post-install next-step hint (E1b). **This is not "connect once, then forget it".** That goal needs the stable path (E1s), a Red-tier `delphi connect` and each proven host slice, all separate approvals (§12). It does not make enablement durable against a missing or hijacked `PATH`, and it does nothing for Codex, Copilot, Gemini or Antigravity beyond E1a (§5.2, §13).

## 1.1 Non-Goals

- **Widening authority.** A marketplace entry, plugin or config only makes the existing 062 server reachable. It adds no tool, no write path, no network port, and does not change what a host may do (062 §1.1, §2). Everything a plugin carries (skill text included) is advice to the host, never authority.
- **Hosting a public registry or accepting third-party plugins.** The marketplace lists Delphi's own plugin only.
- **Submitting to a vendor's public directory.** Possible later; out of scope here (Q-2).
- **Changing the MCP server, its tools or its toggle.** `[mcp] enabled` and `/mcp off` still stop everything at once.
- **Bundling the binary in a plugin.** The binary is installed by `install.sh` or Homebrew (spec 006); a plugin never downloads or executes a fetched binary.

## 1.2 Simpler Alternative Considered

*Keep print-and-paste, only fix the snippets to use an absolute path* (`delphi mcp --print-config <host> --absolute`, refusing `target/` and versioned Cellar paths). This is the cheapest change that helps all four hosts, and the strategist seat recommends doing it before or with E1; it is slice **E1a** (§10), small and independent. It leaves manual edits, no updates and no drift detection, which is what E1-E2 add for Claude. *A separate `delphi-marketplace` repository.* Rejected by the operator: one repository keeps plugin, binary, spec and tests in one pull request and one version.

## 2. Principles

1. **One source of truth.** The files under `ecosystem/` are *generated* by the code in `crates/delphi-engine/src/mcp_hosts.rs` (the same code `--print-config` and `plugin export` use), checked in, and guarded by a drift test. They are never hand-edited. The repository rule for `docs/reference/` (AGENTS.md "Generated files") applies unchanged.
2. **Host-native first.** Use each host's own install and marketplace mechanism. This spec never writes a host's config file.
3. **The operator acts, Delphi never self-installs.** Nothing in this spec runs at `delphi` start-up, in the daemon, or in the MCP server. `doctor` and `--check` only read.
4. **Fail closed, say so.** Where a state cannot be verified, the answer is `unknown` or `unverified`, never `ok` (§7).
5. **The version is the release's.** The *release version* is `[package].version` in `crates/delphi/Cargo.toml` (there is no workspace version; engine crates carry their own). The CLI passes it into the generator; `mcp_hosts.rs` must not hard-code a plugin version (it does today: `0.1.0`). Plugin version, marketplace entry and release version must match; a mismatch fails the gate.

## 3. Repository layout

All plugin and marketplace files live in this repository under a single top-level `ecosystem/` folder, plus the one file a marketplace host requires at the repository root.

```
.claude-plugin/marketplace.json        # root marketplace manifest (required location); generated
ecosystem/
  README.md                            # what is here, how to install, the "generated: do not edit" rule
  claude-code/delphi/                  # the Claude Code plugin
    .claude-plugin/plugin.json
    .mcp.json
    skills/delphi/SKILL.md
  copilot/delphi/                      # Copilot CLI plugin, if Q-1 confirms the format
  codex/delphi/                        # Codex plugin or config bundle, per Q-1
  agy/delphi/                          # Antigravity / Gemini CLI extension, per Q-1
  shared/skill.md                      # the single skill/instructions text (today SKILL_MD in mcp_hosts.rs)
```

- `ecosystem/` is the only new content directory; the root `.claude-plugin/` exists solely to hold `marketplace.json`. That file is unavoidable: Claude Code (and, per Q-1, Copilot CLI) reads a marketplace manifest from the repository root of `owner/repo`.
- Each host directory is self-contained: a host that installs from a path or a git subdirectory needs nothing outside it.
- The stray `delphi-plugin/` at the repository root is removed in slice E1; it is superseded by `ecosystem/`.
- `ecosystem/` holds JSON and Markdown only; no executable file is generated or shipped. It is built under this spec (070: unlisted file types are software).
- Only `ecosystem/claude-code/` and the root manifest are in E1. The `copilot/`, `codex/` and `agy/` directories above are placeholders for later slices and are **not created** until their format is verified (§13 Q-1).

## 4. The marketplace

`.claude-plugin/marketplace.json`:

```json
{
  "name": "delphi",
  "description": "Delphi for host agents: roles, project knowledge, Councils and memory over MCP",
  "owner": { "name": "jessevondoom" },
  "plugins": [
    {
      "name": "delphi",
      "source": "./ecosystem/claude-code/delphi",
      "description": "Delphi's roles, skills, project specs and memory, over MCP",
      "version": "<release version>"
    }
  ]
}
```

- Entry `name` equals the plugin's `plugin.json` `name`; `source` is relative to the repository root with no `..` (a validation failure otherwise).
- Operator install, Claude Code:
  ```sh
  claude plugin marketplace add jessevondoom/delphi
  claude plugin install delphi@delphi
  ```
- Updates arrive with the next release because the marketplace lives in the same repository as the binary; the operator runs the host's normal plugin update.
- Update channel (operator decision D-5(B), 2026-11-03, overriding the three-seat recommendation of SHA pinning): production installs track the latest release tag (not the default branch), so users get features and fixes at each release and not on every commit; host-side updates and `--auto-update` are permitted. Accepted weakness, named: whoever can move or re-point a release tag changes the skill text that steers the host model on the next update. Mitigations that remain required: protected release tags (no force-move or deletion), branch protection and CODEOWNERS review on `ecosystem/` and `.claude-plugin/`, required CI checks (§8) before merge, release attestation of those files, and the plugin carrying no binary, downloader or executable. The red-team dissent in §12 stands against this choice.
- A listing is not an endorsement or approval: installing the plugin grants Delphi nothing; the operator's approvals in the console are unchanged. Docs say "installed" never means "approved".
- `CODEOWNERS` requires release-owner review for `.claude-plugin/`, `ecosystem/` and `crates/delphi-engine/src/mcp_hosts.rs`, because the skill text steers the host model.

## 5. The plugin contents and the launch command

### 5.1 What each plugin carries

The 062 §10 contents, unchanged in meaning: the MCP declaration (`delphi mcp`), and the instruction text that tells the host *when* to consult Delphi (the `delphi` skill for Claude Code; the equivalent instructions file for the others). Role IDs only, never theme names outside `themes/` (lint-enforced).

### 5.2 Launch command and the PATH limit

E1 keeps `command: "delphi"` for Claude Code, the same command every 062 snippet runs. Checked-in files are identical for every machine and cannot hold an absolute path. The consequences are stated, not hidden:

1. **A host resolves `delphi` with its own environment**, which `doctor` cannot observe. A writable earlier `PATH` entry could run another binary, and a GUI-launched host may not find `delphi` at all. This is the status quo of every pasted snippet; E1-E2 do not worsen it and do not cure it. `doctor` and `--check` never report launchability from the invoking shell's `PATH` (§7).
2. **The cure is an absolute path.** For hand-placed configs, `--print-config --absolute` (E1a). For the plugin, a stable executable path needs a spec-006 amendment (E1s): `install.sh --dir`, Homebrew and self-update do not today maintain `~/.local/bin/delphi`, and Claude Code expands `${HOME}` in a plugin `command` (verified, §13), so the declaration could then be `${HOME}/.local/bin/delphi`. Until then this spec ships no stable-path claim.
3. **Launcher: rejected.** No executable file is generated or shipped by this spec.

### 5.3 Per-host connect commands (vendor docs, 2026-11-03; research only)

Not implemented or approved by this spec. Each host's own command is recorded to scope the later slices; anything that causes a host to write its configuration is a Red-tier host-config write (§6, §12).

| Host | Delphi artifact | The host's own connect command |
| :-- | :-- | :-- |
| Claude Code | root `.claude-plugin/marketplace.json`, `ecosystem/claude-code/delphi` | `claude plugin marketplace add jessevondoom/delphi` then `claude plugin install delphi@delphi` |
| Copilot CLI | `ecosystem/copilot/delphi` (E4a) | `copilot plugin install OWNER/REPO:ecosystem/copilot/delphi` |
| Gemini CLI | `ecosystem/gemini/delphi` (E4b, Q-7) | `gemini extensions install <url> --ref <release tag>` (`--auto-update` permitted, D-5(B)) |
| Codex | `ecosystem/codex/delphi` (E4c) | the plugin directory, or `codex mcp add` |
| Antigravity | none | manual, unverified |

### 5.4 Operator experience (read-only, no host writes)

Every item prints text the operator copies; none writes a host file, runs a host CLI or touches the network.

1. **Print the host's own command (E1a).** `delphi mcp --print-config <host> [--absolute]` ends with the host's documented install command (Claude Code, Copilot CLI and Gemini CLI once their slice is proven; otherwise the snippet only, labelled as such). Commands come from one table in `mcp_hosts.rs`, so docs, help and output cannot disagree.
2. **Fix hints (E2).** `doctor` and `--check` print the one command that cures `missing` (install), `stale` (the host's update command) or `incompatible` (update the binary or the plugin, whichever is older).
3. **Next-step hint (E1b).** `install.sh` and `delphi init` end with static text: "Use Delphi from another tool: `delphi mcp --print-config <host>`", listing the hosts. It detects nothing, reads no host file and changes no behaviour; spec 006's installer contract is otherwise untouched.
4. **Version visibility, no server change.** MCP `initialize` already returns `serverInfo.version` (the binary). Each generated skill states the release it was generated for, so a host user sees both; the comparison is E2's `stale` and `incompatible` states. The MCP server, its tools and `tools/list` are unchanged.
5. **Uninstall and removal docs (E1a, E1).** The guide gives each host's documented one-line removal (for Claude Code, `claude plugin uninstall delphi@delphi` and `claude plugin marketplace remove delphi`, to be verified on the named version), and says that turning Delphi off without removing it is `[mcp] enabled = false` or `/mcp off`.

## 6. Deferred installer

Host-configuration installation, uninstallation and absolute-path rewriting are **out of scope**. This spec defines no `--install`, `--uninstall`, `--dry-run`, `--yes` or host-config override flag. They need a separate approved spec with Red-Team review, which must at least carry: operator-only enforced in code (TTY required; refused from the MCP surface, the daemon and any agent or taint-reduced span); a ratified allowlist of host config paths; symlink and TOCTOU refusal; single-key merge, backup and atomic write, idempotence and uninstall; JSONC-preserving edits; a running-host-process check; an audit line per write; Windows paths. It is the class GOVERNANCE treats as Red (host-executed files outside the workspace).

## 7. Status check: `delphi doctor` and `delphi mcp --check`

**Scope (E2): Claude Code only; the schema is recorded in Q-8.** `delphi mcp --check claude-code` and one `delphi doctor` line read the documented Claude Code settings file, on a recorded named Claude version, at the exact path and JSON fields Q-8 records, to learn whether plugin `delphi@delphi` is installed, enabled and at which version. Any other path, schema or version yields `unknown`. It never infers marketplace installation from a project `.mcp.json`. Other hosts report `unverified` until their slices exist. Generation and every check are **offline**; only the host's own install and update commands use the network.

States: `configured` (the expected enabled plugin entry and version were parsed; this does **not** assert the binary can be launched), `missing` (prints the command that fixes it), `stale` (version differs from the release), `incompatible` (plugin newer than this binary supports, which would expose tools the binary lacks), `unknown`, `unverified`. **Launchability is reported separately and is `unknown`** until E1s defines a stable path: then it requires a non-empty absolute `HOME`, a regular user-owned non-symlink executable and parents that are user-owned and not group/world-writable, and it is never established by executing the binary or from the invoking shell's `PATH`. `--check` exits non-zero on anything but `configured`; `delphi run` exit codes are unaffected. It prints the command, not the snippet (as 062 does).

**Safe reads.** A fixed per-host path allowlist; open without following symlinks; regular file owned by the user only; at most 1 MiB; bounded JSON/TOML parsing. Only the one expected entry is inspected. Argument, environment, parse-error and unknown-field values are omitted from diagnostics, so a token in a hostile file cannot be echoed. It reads and writes nothing else.

## 8. Generation, versioning and gates

- **Generate and check:** `delphi plugin export --host <h> --out <dir>` keeps its 062 behavior. A new maintainer command `delphi docs ecosystem --repo <repo> --check` compares the generated `ecosystem/` tree and `<repo>/.claude-plugin/marketplace.json` without writing and exits non-zero on drift; `--write` regenerates exactly those paths. The docs integration test runs `--check` against the checkout and `--write` into a fresh fixture root. (Today `docs` has only `cli|man|catalog|coverage`; this is new work in E1.) The CLI passes the release version in (§2.5).
- **Generated header:** every generated file carries a "generated by `delphi docs ecosystem`; do not edit" notice where its format allows comments (otherwise `ecosystem/README.md` states it).
- **Drift test, failing CI:** `cargo test -p delphi --test docs` (existing) runs in CI as a required check and additionally fails when `ecosystem/` or `.claude-plugin/marketplace.json` differ from the generated output, and when the plugin `version`, the marketplace entry version and the release version differ.
- **Schema checks:** `plugin.json` and `marketplace.json` are validated in the test against the vendors' documented required fields (`name`, `owner`, `plugins[].name`, `plugins[].source`), and `claude plugin validate` is run in CI when the `claude` CLI is available (skipped, not faked, when absent).
- **Lint:** `delphi lint --repo .` already rejects theme names outside `themes/`; `ecosystem/` is inside its scope.
- **Release:** the release workflow regenerates `ecosystem/` and fails if the tree is dirty; the release signature or manifest covers `.claude-plugin/marketplace.json`, `ecosystem/**` and the generator, so a tagged release carries matching, attested plugin files (§4 update channel).

## 9. Security and trust

- Same machine, same user; no new port, token or authentication (062 §3.2).
- A plugin, skill or instructions file is data a *host* loads; it cannot raise Delphi's trust, and what Delphi returns to the host stays advice with the 062 caveats.
- The deferred installer (§6) is not part of this spec; nothing here can write a host config, and a test asserts `tools/list` from `delphi mcp` is unchanged.
- Supply chain: the plugin contains no binary, no downloader and no executable. It is an update channel for **skill text that steers the host model** and for the MCP declaration, so §4 requires protected release tags, CODEOWNERS review and release attestation of those files (the operator chose a release-tag channel over SHA pinning, D-5(B)). The repository's release attestations (spec 006) remain the trust root for the binary. A plugin newer than the binary is detected by `incompatible` (§7).
- A malicious or corrupt host config file is never executed; reads follow §7 "Safe reads" and fail closed.

## 10. Slices (one per pull request; none starts before approval)

| # | Slice | Depends on |
| :-- | :-- | :-- |
| E0 | this spec's approval | |
| E1a | `delphi mcp --print-config <host> --absolute`: prints the snippet with an absolute path (refuses `target/` and versioned Cellar paths, relative paths, and an unset `HOME`); guide text for the existing hosts. The cross-host MVP. Also prints the host's own install command (§5.4.1) and adds the guide's per-host uninstall text (§5.4.5) | E0 |
| E1 | generator `delphi docs ecosystem --check/--write`; `ecosystem/claude-code/` only (no placeholder host directories) and the root `.claude-plugin/marketplace.json`; release version passed in (no hard-coded `0.1.0`); production source tracks release tags (D-5(B), protected tags, no SHA pin); drift and version tests; CODEOWNERS entries; remove the stray `delphi-plugin/`; guide update and `llms.txt`. Declaration is bare `delphi` and documented as PATH-dependent (D-4) | E0 |
| E1b | `install.sh` and `delphi init` end with the static next-step hint (§5.4.3); no detection, no reads, no behaviour change; golden-output test | E1a |
| E1s | stable executable path: a spec-006 amendment for installer, updater and Homebrew, then the plugin declaration switches to it (and `doctor` reports `launchable` per §7). Not started by this spec | E1, D-6 |
| E2 | `delphi mcp --check claude-code` and one Claude Code `delphi doctor` line (§7) with the fix hints of §5.4.2, built on Claude Code 2.1.314 (Q-8 answered) | E1 |
| E3 | installer and `delphi connect` (any host CLI that writes host state): deferred to a Red-tier successor spec (§6, §12) | E2, D-7 |
| E4a | Copilot directory, first of the E4 slices (operator priority: Claude and Copilot); needs its recorded proof (Q-1), including `${HOME}` expansion in an MCP `command`; fixture tests; no connect command | E1, Q-1 |
| E4b-c | Gemini CLI and Codex directories, one slice each, only after its recorded proof (Q-1) | E1, Q-1 |
| E4d | Antigravity: only if official documentation appears | Q-1 |
| E5 | launcher: rejected | |

Each slice updates the spec tracker in its own pull request, regenerates `docs/reference/`, and passes the three gates in AGENTS.md. Commits carry `Implements-Spec: specs/071-host-plugin-marketplace-and-durable-mcp-enablement.md`.

## 11. Testing

- Golden-file tests for each generated file; the version-consistency test (§8); `--check` and `--write` as in §8.
- A test that `tools/list` from `delphi mcp` is unchanged by this spec.
- `doctor` and `--check` states (`ok`, `missing`, `stale`, `incompatible`, `unknown`, `unverified`) against fixture settings, including symlink, FIFO, oversized and malformed files, a secret-bearing field that must not appear in output, and newer and older plugin-binary pairs.
- E1a: `--absolute` refuses `target/` and Cellar paths; the printed host command matches the table. E1b: golden output of the `install.sh` and `init` hint.

## 12. Decisions for the operator

**Council, round 3 (2026-11-03; three Copilot-run seats, two rounds: independent review with suggestions, then deliberation on every suggestion).** Final verdicts: architect, red-team and strategist all **APPROVE-WITH-AMENDMENTS** (the red-team moved from REJECT in round 1 after the consolidated amendments). Unanimous: E1a first; Claude-only E1 with no placeholder host directories; the original recommendation was a full commit SHA with no production auto-update, **overridden by the operator** (D-5(B): release tags, protected, auto-update allowed); E2 gated on a named Claude version, exact settings path, schema and fields, `unknown` otherwise; any host CLI that writes host state is a Red-tier host-config write whoever executes it, so `delphi connect` is **not** approved here and moves to the §6 successor spec (Claude first, with the safe-exec contract below); fixture and recorded-proof harness per host; offline behaviour stated.

Majority resolution (red-team and strategist, against the architect): the stable executable path is **not** shipped by E1. E1 keeps `command: "delphi"` and says it is PATH-dependent; the stable path waits on a spec-006 amendment (slice E1s). Architect's alternative, kept as dissent: support `${HOME}/.local/bin/delphi` for the default `install.sh` location only, with safe-path checks, no spec-006 change.

Dissents kept verbatim: *Architect:* "Automated `connect` remains deferred until a Red-tier host-state ownership, disable, uninstall, and safe-exec specification is approved." *Red-team:* "No production plugin install may rely on a mutable tag, branch, or unverified stable executable path." *Strategist:* "No E1 marketplace declaration should ship until the stable-path contract is implemented and verified." The last is retained as a gate the operator may choose (D-4).

Requirements carried to the successor spec for `delphi connect` and any installer: no shell; a fixed argv template per host and version, rejecting operator-, agent-, environment- and repository-supplied arguments; the host CLI resolved before consent and shown by absolute path with escaped argv, working directory, commit SHA and expected writes and network effects; SHA and executable identity revalidated immediately before exec; TTY required and refusal from the MCP surface, the daemon and any agent span enforced in code; uninstall and disable ownership; one canonical host-ID map (`claude-code`, `copilot`, `codex`, `gemini`, `agy`; Gemini CLI and Antigravity are distinct).

- **D-1.** One repository, one `ecosystem/` folder, generated and drift-tested; E1a, E1, E1b, E2, with the §5.4 operator-UX items. *Approve.*
- **D-2.** The installer and `delphi connect` are deferred to a Red-tier successor spec. *Confirm.*
- **D-3.** No launcher script. *Confirm.*
- **D-4.** Release behaviour for E1: (A) ship the marketplace now with a bare, PATH-dependent `delphi` and say so; or (B) hold E1 until E1s delivers the stable-path contract (strategist dissent). *Recommended A, with E1a available to operators who want an absolute path now.*
- **D-5 (decided 2026-11-03: B, release-tag updates).** Update behaviour: (A) commit-SHA pinned, a deliberate update per release; or (B) updates that follow release tags, accepting the branch-update supply-chain risk. *Operator chose B, release tags only (not the default branch); the three seats recommended A and the red-team dissent stands (§4).*
- **D-6.** Write a spec-006 amendment for a stable executable path (E1s) next? *Recommended yes.*
- **D-7.** Write the Red-tier `delphi connect` successor spec next, Claude first? *Recommended yes, after D-6.*

## 13. Open questions

- **Q-1 (gates each E4 slice separately; one recorded proof per host and named CLI version).** Copilot CLI: manifest and `copilot plugin install OWNER/REPO:PATH`. Gemini CLI: a minimal `gemini-extension.json` and `gemini extensions install` (see Q-7). Codex: the plugin manifest or `codex mcp add`. Antigravity: no official source found; distinct from Gemini CLI. Each proof is a recorded transcript plus a fixture config; no host directory is generated before its proof.
- **Q-1 research (2026-11-03, vendor docs; not live-proven).** Copilot CLI: `copilot plugin marketplace add`, `copilot plugin install OWNER/REPO:PATH`, `copilot plugin update` (https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-plugin-reference). Gemini CLI: `gemini extensions install <url> [--ref] [--auto-update]`, `gemini-extension.json` with `mcpServers` and `${extensionPath}`, safe environment only (https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/extensions/reference.md). Codex: `.codex-plugin/plugin.json` plus `.mcp.json`, repo marketplace `.agents/plugins/marketplace.json`, `codex mcp add` (https://developers.openai.com/plugins/build/plugins). Claude Code expands `${VAR}` in a plugin `command` (https://code.claude.com/docs/en/mcp.md); Copilot and Codex expansion is unverified.
- **Q-3 (closed 2026-11-04).** Withdrawn: the operator chose no version or SHA pinning (D-5(B)).
- **Q-6.** Nested defaults in `command` (`${DELPHI_BIN:-${HOME}/.local/bin/delphi}`) and `HOME` in GUI-launched hosts. Belongs to E1s.
- **Q-7.** Gemini installs a repository root as the extension; does it accept a subdirectory, or does the extension need its own root? Decided by the E4b proof.
- **Q-8 (answered 2026-11-04; E2 built).** Recorded on Claude Code 2.1.314 by a real `claude plugin marketplace add <local checkout>`, `plugin install`, `plugin disable` and `plugin uninstall`, then removed. Installed state: `~/.claude/plugins/installed_plugins.json`, `{"version": 2, "plugins": {"delphi@delphi": [{"scope", "version", "installPath", "gitCommitSha", ...}]}}`; the entry is removed on uninstall. Enabled state: `~/.claude/settings.json` `enabledPlugins["delphi@delphi"]` (`true` after install, `false` after `disable`, key removed on uninstall). `claude plugin list --json` shows the same plus the declared `mcpServers`. E2 reads only these two files. A project-scope install is not checked (`unknown`). Proven against the live host: `configured`, disabled and uninstalled states matched. Not proven: a marketplace added from GitHub (the local directory form was used, which resolved the relative `source` correctly), and `claude plugin update` pinning (Q-3).
- **Q-9 (not a slice here).** `request_council` from a host (`t_0001`, 2026-11-03) ran with no seat able to read the project file. Establish whether a host-raised Council can read the pointers it is given before the guide recommends it. Daemon-down behaviour is already clear (`daemon_not_running`, `mcp_daemon.rs`), so no change is proposed.
- **Q-5.** Windows is out of scope here (POSIX paths only); the successor spec decides it.

## 14. Acceptance criteria

- **AC-1.** With `delphi` on the host's `PATH` and a recorded Claude Code version, `claude plugin marketplace add` of the repository (release tag) then `claude plugin install delphi@delphi` makes Delphi's tools appear in Claude Code. (PATH-dependent; durable path behaviour is AC-6.)
- **AC-2.** `delphi docs ecosystem --check` (run by the docs test) fails if any file under `ecosystem/` or the root manifest is edited by hand, or if plugin, marketplace and release versions differ.
- **AC-3.** `delphi mcp --check claude-code` reports `unknown`, not `configured`, when the settings file is absent or unrecognized, and `incompatible` for a plugin newer than the binary; it never executes the binary and reports launchability `unknown` (§7).
- **AC-4.** `delphi doctor` reports a missing, stale or incompatible Claude Code plugin with the command that fixes it, and prints no secret from any file it reads.
- **AC-5.** No new MCP tool exists, `tools/list` is unchanged, and no code in this spec writes a host config file or runs a host CLI.
- **AC-6.** (Deferred with E1s.) With the stable-path contract implemented, Claude Code connects with `PATH` empty, and a test covers unset `HOME`, a symlinked binary or parent, and a group/world-writable parent.
- **AC-7.** `delphi mcp --print-config <host> --absolute` (E1a) refuses `target/` and versioned Cellar paths, and prints the host's own install command for hosts whose slice is proven.
- **AC-8.** `install.sh` and `delphi init` print the next-step hint and nothing else new (E1b); `doctor` prints the fix command for `missing`, `stale` and `incompatible` (E2).

- **Q-1 and E4a/E4c findings (2026-11-04, Copilot CLI 1.0.105, Codex 0.166.0, real hosts, state restored afterwards).** Both hosts read the root `.claude-plugin/marketplace.json` and install the same `ecosystem/claude-code/delphi` plugin unchanged. Copilot: `copilot plugin marketplace add <path>` then `copilot plugin install delphi@delphi`; `copilot mcp list` shows `delphi (local)` under plugin servers; state in `~/.copilot/settings.json` (`enabledPlugins`, `extraKnownMarketplaces`). Codex: `codex plugin marketplace add <path>` then `codex plugin add delphi@delphi`; `codex mcp list` shows `delphi delphi mcp enabled`; state in `~/.codex/config.toml` (`[plugins."delphi@delphi"]`). So `ecosystem/copilot/` and `ecosystem/codex/` are not created: E4a and E4c ship as install hints, guide text and `--print-config` hints. Gemini CLI 0.63.0 does not accept this directory (`validate` needs `gemini-extension.json`): E4b needs its own generated file and proof, not yet built. Antigravity (E4d) stays blocked on official docs. Host-state checks for Copilot and Codex are not built (`unverified`). Not proven: `${HOME}` expansion, a GitHub-sourced marketplace.
