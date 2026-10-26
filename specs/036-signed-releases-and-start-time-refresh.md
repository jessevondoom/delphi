---
id: spec-036-signed-releases-and-start-time-refresh
title: "036: Signed releases and the start-time refresh (amends 006 section 3.1 and A6)"
author: architect
timestamp: 2026-10-26T14:00:00Z
status: draft   # core logic: for the maintainers to review before it is approved; it changes nothing for anyone until the maintainers make a release key (section 4)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # built and tested end to end with a throwaway key (sign, rebuild, verify, tamper); never run with a real key or in CI
source_mission: 2026-10-23-foundational-design
tags: [spec, release, signing, ratification, update, security, draft]
---

# Spec 036: Signed releases and the start-time refresh

## 1. What this is
Operators should not have to ratify what the maintainers ship. `specs/006` A6 already says running `delphi update` ratifies the release's own files, but it can only say so because the operator typed the command, and it could not be more automatic without a way to tell a real release from a tampered or hostile build. That way was always planned ("Step 5": a release key, a signed manifest) and never built: every `manifest.lock` says `unsigned-dev-build`, and `delphi doctor` warns about it.

This spec builds it. The maintainers sign, in CI, the manifest of the files a release ships. Every build carries that signature and the list of keys it trusts, and checks them against its own files. A build that verifies can refresh an older workspace and ratify the release's own files **the first time it is started**, with nobody asked. Security stays on the maintainers' side: who may sign, and what the signature covers, change only by a reviewed change to files that CODEOWNERS names.

## 2. The rules (who ratifies what)
| # | Rule |
| :-- | :-- |
| R1 | **Release files.** A file held byte for byte as the release has it is ratified when the operator runs `delphi update` (A6, unchanged), and, in a **signed** build, also at the first `delphi chat` or `delphi run` after the binary changed. |
| R2 | **Everything else stays the operator's.** A customized file (`core/`), a file in `team/` or `local/`, endpoint, access and credential config, a spec approval, an org bundle, anything whose bytes differ from the release: never ratified by an update or a refresh. It stays a pending proposal (`/ratify`). |
| R3 | **Unsigned builds** (a local or development build with no signature) behave exactly as before: no start-time refresh; `delphi update` ratifies when the operator runs it. |
| R4 | **An invalid signature ratifies nothing.** If a signature is present and does not hold (a file, the version, the key or the signature is not what was signed, a key not in the trust list, a revoked key), `delphi update` refuses before it reads the workspace, `delphi release` exits 1, and `delphi doctor` fails check 2. It never falls back to "unsigned". |
| R5 | **Require signed.** `--require-signed` on `delphi update`, or `DELPHI_REQUIRE_SIGNED=1`, refuses an unsigned build too. |
| R6 | **Only the operator starts it.** The refresh runs from `delphi chat` and `delphi run`, and `delphi update` is as before. No daemon, agent, hook, relay, timer, narrator or `status` can start it (A6's rule, kept). `DELPHI_NO_AUTO_UPDATE=1` turns the refresh off. |
| R7 | **Forward only.** The downgrade guard is unchanged; a workspace newer than the binary is left alone. |

## 3. What is signed
- **The manifest.** `{"logical/path":"<blake3 hex>", ...}` for every file the binary embeds as its seed (`GOVERNANCE.md`, `SPEC.md`, `REVIEW.md` and the `agents`, `skills`, `themes`, `context` and `memory` trees), keys sorted, no spaces. `delphi_xenia::release_sig::manifest_json` builds it; the signing tool builds it from the checkout and the binary from what it embeds, and a test holds the two equal (`release::tests::the_signer_and_the_binary_see_the_same_files`).
- **The signature.** Strict Ed25519 (RFC 8032, as for org bundles) over a new domain `xenia-release/1 \n version \n kid \n manifest hash \n signed_at`. The `version` is the verifier's own (`CARGO_PKG_VERSION`), so a signature for one release does not verify under another, and a development build is a different version from the release it grew from. The leading label makes a bundle, record or fresh-record signature unusable as a release signature, and the reverse (tested).
- **The trust list.** `release-keys.json`, compiled in: public ids (`did:key`) with `active` or `revoked`. An empty list trusts nothing.
- **What the signature does not vouch for.** The Rust code. It says "these seed files are what the maintainers' CI built from `main`". The code is protected as before: review, branch protection, the build attestations (`install.sh` checks them), and the SHA-256 the self-updater checks. A signature does not make a hostile change to `ratify.rs` safe; CODEOWNERS (section 5) is what stands in front of that.

## 4. The maintainers' procedure (once)
1. Make the key on a machine you trust: `cargo run -p delphi-release -- keygen --out <file>`. It prints the public id and refuses to overwrite a file. The file is `0600` and is never printed.
2. Add the public id to `release-keys.json` in a pull request (CODEOWNERS makes that a reviewed change).
3. Store the key as the repository secret: `base64 < <file> | gh secret set RELEASE_SIGNING_KEY`. Keep an offline copy of the key file; delete it from the machine.
4. Merge. From then on the `sign` job in `dev.yml` and `release.yml` signs every build and runs `delphi-release verify` against `release-keys.json`; with the secret set and the id missing from the list, the workflow fails, so a mistake is loud and nothing unsigned-looking ships by accident.
5. **Rotation:** add the new id, ship a release signed with both available, then mark the old id `revoked`. **Compromise:** mark it `revoked` and ship; every build that carries the new list refuses what the old key signed. A build already installed with the old list keeps trusting the old key until it is updated (a limit of any pinned key; section 7).

Until step 3 nothing changes for anyone: every build is unsigned and behaves as before.

## 5. Admin-side controls
- `.github/CODEOWNERS` names the files that decide what an installation trusts: the trust list and the signing tool and verifier, the update and seed code, the workflows, the constitution, rubric, roles, skills, themes and heuristics, and the code that enforces ratification, provenance, paths, approvals, gates and execution. It only *requests* the review; to bind it, turn on **Require review from Code Owners** in the branch protection rule for `main`. With one maintainer GitHub will not let an author approve their own pull request, so the realistic setting is a protection rule with an admin bypass, or a second reviewer account.
- The committed `crates/delphi/release.sig.json` must stay `{}`: a test (`release::tests::a_checkout_is_unsigned_and_is_not_refused`) fails if a signature is committed, because a signature belongs to one build and would make every later build invalid. CI writes it into the build it signs and nowhere else.
- The secret is read by one step of one job, written to a `0700` directory with `umask 077`, and removed before the next command.

## 6. Built
`delphi-xenia`: `Domain::release`, `release_sig` (sign, verify, `manifest_json`, `TrustList`, `check`). New crate `delphi-release` (the maintainers' tool: `keygen`, `manifest`, `sign`, `verify`; not shipped). `delphi`: `release.rs` (the build's own status), `delphi release`, the lock's `signature` (`ed25519:<kid>` or `unsigned-dev-build`), the refusal in `delphi update` and `--require-signed`, `update::auto_refresh` from `chat` and `run`, doctor check 2. Workflows: the `sign` job and the hand-off to the build jobs. `release-keys.json` (empty), CODEOWNERS.

Tested: the crypto and the trust decision (`delphi-xenia`), the signer and the binary agreeing on the files, the refresh with a signed status injected (ratifies the release files, leaves customized and operator-written files pending, does it once, does nothing for unsigned, invalid or opted-out), and end to end with a throwaway key: sign this checkout, rebuild, `delphi release` says signed; change one shipped file, rebuild, it says invalid and `delphi update` refuses.

## 7. Not built, and open
- **Refusing to install an unsigned binary.** `--require-signed` guards the workspace step; the self-updater installs the binary before the new program can be asked about its signature. Checking it first needs the downloaded program run once (`delphi release`) in `selfupdate.rs` before the swap.
- **Team and local seed files** (`REVIEW.md`, `context/*`, `memory/*`) are seeded once and not refreshed by an update; a signed release could refresh the ones the operator has not touched.
- **A second signer** (two of N) and an offline-held root that signs the list of release keys: the answer to a key that is stolen before it is revoked.
- **Trust-list freshness.** A build trusts the list it was built with; a revocation reaches an install with its next update.
- **Decision for the maintainers:** whether the rolling `dev` channel is signed with the same key (this spec does it, so the maintainer's own installs refresh too) or with a separate, lower-trust key; and whether, once signing is in use, `--require-signed` becomes the default.
