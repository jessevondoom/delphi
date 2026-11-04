---
id: spec-076-private-repo-distribution
title: "076: Installing and updating Delphi from a private repository"
author: architect
timestamp: 2026-11-04T22:10:00Z
status: approved   # by the operator, 2026-11-04 ("start writing code. build all slices on one branch")
approved_by: operator (2026-11-04)
source_type: internal_generated
confidence: low
source_mission: 2026-10-23-foundational-design
tags: [spec, installer, updater, release, security]
---

# Spec 076: Installing and updating Delphi from a private repository

> **Status: approved by the operator 2026-11-04 (no Council).** Amends [`specs/006`](006-installer-and-updater.md) and [`specs/036`](036-signed-releases-and-start-time-refresh.md) (section 7, the origin rule), and touches the proof in [`specs/074`](074-stable-launch-path-and-delphi-connect.md) section 8.

## 1. Problem & Intent

`install.sh` and `delphi update` read GitHub anonymously: `curl` against `api.github.com` and `github.com/.../releases/download/`, and `reqwest` in `selfupdate.rs`. A private repository answers every anonymous request with 404, so both stop working. The repository will live in two places: a personal account (any plan) and a GitHub Enterprise Cloud organization. Both must work.

Intent: authorized users (the operator and their organization) install and update exactly as before, using the credentials they already have for `gh`, with no loss in what the installer verifies.

## 1.1 Non-Goals

- No external users, no public releases repository or mirror (the `DELPHI_API` / `DELPHI_DOWNLOAD` / `DELPHI_RELEASES_URL` overrides stay as they are).
- No Homebrew change. `Formula/delphi.rb` and its generator in `release.yml` are untouched; Homebrew keeps working only while the repository is public, and the docs say so.
- No GitHub Enterprise Server, no `*.ghe.com` data-residency hosts, no Enterprise Managed Users. The host stays `github.com`.
- No new credential storage. Delphi never writes, logs or caches a token.

## 2. Credentials

One lookup, used by the installer and the updater:

1. `GH_TOKEN`, then `GITHUB_TOKEN`, if set and non-empty.
2. Otherwise the output of `gh auth token`.
3. Otherwise no token. A public repository still works anonymously; a private one stops with the message in section 5.

A token is sent only to `api.github.com` (and `github.com`), never to a mirror named by an override, and never to the storage host a release asset redirects to (see 3.2). It is never printed.

## 3. Changes

### 3.1 `install.sh`

1. Release lookup: `gh api repos/$REPO/releases/latest` when `gh` is signed in or `GH_TOKEN` / `GITHUB_TOKEN` is set (gh honours both); otherwise anonymous `curl`, as before. The installer has no `curl`-with-token path: gh is the one place a credential is handled.
2. Asset download: `gh release download` (which uses the release-assets API, because a private repository's browser download URLs do not take a Bearer token); otherwise anonymous `curl`, as before.
3. The install one-liner becomes `gh release download --repo $REPO --pattern install.sh --output - | sh` (README, `docs/getting-started.md`). The anonymous `curl` form stays documented for a public repository.
4. Mirror overrides keep their meaning, including the HTTPS-only rule and the loud warning, and receive no token.

### 3.2 `delphi update` (`selfupdate.rs`)

1. Add the section 2 token as `Authorization: Bearer` on requests to the GitHub API, and use each asset's API URL with `Accept: application/octet-stream` (the header already sent).
2. The client follows redirects only over HTTPS, as now, and **removes the `Authorization` header on any redirect to another host**.
3. The origin rule (`ORIGIN_DOWNLOADS`, `asset_allowed`) trusts the project's own API and, for assets, its API asset URLs under `https://api.github.com/repos/jessevondoom/delphi/releases/assets/` as well as its browser URLs. Anything else from the project's own API is still ignored. A mirror the operator named is still trusted, as now.
4. The repository is named in `selfupdate.rs` (three constants) and in `install.sh` (`REPO`); a test fails if they disagree, so a move between the personal account and the organization is a find-and-replace the test checks.

### 3.3 Verification is tiered, and never weaker than today

Today the installer requires a verified build-provenance attestation. Attestations for private repositories need GitHub Enterprise Cloud, so a personal-account repository cannot produce them. The installer therefore accepts, in order:

1. **Attestation**: `gh attestation verify ... --signer-workflow` succeeds (as now).
2. **Maintainers' release signature**: if the repository has no attestation for the download (`gh api repos/$REPO/attestations/sha256:<digest>` returns none), the unpacked build's `delphi release status` (`crates/delphi/src/release.rs`) must report `signed by` a key in `release-keys.json`. It works on any plan and needs no network. **It is weaker than tier 1**: the build reports its own signature, so it catches an unsigned or accidentally altered build, not an attacker who can replace the release files with a binary that lies. The trust root is then who can publish to the repository, and the installer says so when it takes this path. Where attestations are available, tier 1 is used and cannot be bypassed by tier 2.
3. Neither: stop, and install nothing, unless `DELPHI_SKIP_ATTESTATION=1` (unchanged, noisy).

A tier-1 attempt that **fails** (an attestation exists and does not verify) stops the install; it never falls through to tier 2. Only "no attestation exists" falls through. The unpacked build is run only for `--version` and `release status`.

### 3.4 Release workflows

`release.yml` and `dev.yml` keep building and signing. The attestation step must not fail the build when the repository's plan cannot produce attestations (`continue-on-error` or a plan check); the signature step is unchanged. The release signing secret, tag-driven `gh release create` and the `GITHUB_TOKEN` use need no change.

## 4. Plugin marketplaces

Spec 074 section 8 proved the Copilot and Codex marketplace flows against the public GitHub source. Slice P2 repeats them against the private repository, per host, and records the result there. A host that cannot authenticate is documented as unsupported for a private repository; no workaround ships in this spec.

## 5. Failure messages

On 401 or 404 with no token: `this repository is private; run "gh auth login" (or set GH_TOKEN), then try again`. On 403 or a 404 with a token: add `if the repository is in an organization with SAML single sign-on, authorize the token for it`. Neither message includes the token.

## 6. Security considerations

- The token is a credential that now crosses a network client: it is sent only to GitHub's API, dropped on every cross-host redirect, never logged, never written, never passed to a mirror.
- Tiered verification keeps the installer from trusting only the checksum, which comes from the same place as the file. Section 3.3 keeps a signed-by-maintainers check where attestations are unavailable.
- No code path acts outside the sandbox, approves its own action, or raises trust from content it reads. `release-keys.json` changes still need maintainer review.

## 7. Slices

| Slice | Content | Estimate | State |
| :-- | :-- | --: | :-- |
| P0 | this spec, approval, spec tracker entry | 2 h | built (this branch) |
| P1 | token lookup; `install.sh` and `delphi update` changes (3.1 to 3.3), tests (404 hint without a token, token only to GitHub's API, asset-API path, repo-name agreement, tiered verification in `crates/delphi/tests/install_sh.rs`, a failed attestation does not fall through; the redirect strip is reqwest's own behaviour for `Authorization`, with the header marked sensitive) | 1 day | built (this branch) |
| P2 | marketplace proof against the private repository (4), workflow change (3.4), docs, CHANGELOG | 0.5 day | workflow, docs and CHANGELOG built (this branch); the marketplace proof needs the private repository and is not done |

## 8. Open questions

- Which repository path (personal account or organization) is canonical? The constant moves with it; attestation can be required (dropping tier 2) only where the plan supports it.
- Resolved: `delphi release status` exits 0 for both signed and unsigned builds, so the installer matches its `release <v>: signed by` line. A `--json` form would be sturdier and is left for later.
- Tier 2 is circular (see 3.3). A non-circular check needs an already-trusted `delphi` (an update, not a first install) or a public key the installer carries; neither is built.
