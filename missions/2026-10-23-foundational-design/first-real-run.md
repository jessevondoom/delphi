---
id: first-real-run
title: "First real run: sign-in, Gemini through OAuth, and both harnesses"
author: strategist
timestamp: 2026-10-24T20:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-3, howto, oauth, harness]
---

# First real run

What works today with a subscriber account and no API keys, what needs one setup step from you, and what cannot be done (and why).

## What each subscription can do

| You have | Path | Status |
| :--- | :--- | :--- |
| Claude subscription | **Harness** (`claude` CLI as your own process, Delphi answers its permission requests) | Works. A full build ran end to end on 2026-10-24: Builder, tests, Red-Team audit, your approval card, merge (`cargo test -p delphi-engine live_build -- --ignored --nocapture`). |
| Claude subscription | **Anthropic API through the subscription's OAuth token** | **Not possible and not something Delphi will do.** Subscription sign-in tokens are issued to Anthropic's own apps; using them from another program is outside the terms, and `specs/009` §4.2 forbids borrowing another tool's login. The ways to reach Claude without the CLI are an API key, Bedrock or Vertex. |
| Gemini subscription (Antigravity) | **Harness** (`agy`) | Works for read-only roles and Council seats; no approval bridge, so no Builder; output is untrusted (L0). |
| Any Google account | **Gemini API with your own OAuth client** | Built and unit-tested; needs the setup below, then a live check. This is the Gemini *API* (free-tier quota of your own Google Cloud project), not the Antigravity plan. |

## 1. Claude harness (works now)

In `$DELPHI_HOME/workspaces/<name>/config.toml`:

```toml
[[endpoints]]
id = "claude-sub"
family = "claude"
path = "harness"
harness = "claude"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "haiku", workhorse = "sonnet", frontier = "opus" }

[access]
order = { claude = ["claude-sub"] }
allowed_endpoints = ["claude-sub"]

[exec]
sandbox = "off"                          # approvals are the control; "prefer" turns the sandbox on
allow = ["cargo test*", "cargo build*", "cargo clippy*"]
test_command = ["cargo", "test"]         # merge gate 1
check_command = ["cargo", "clippy"]      # merge gate 2
```

Then: `delphi doctor` (check 10 shows `claude` signed in), `delphi endpoints test --probe` (one tiny call), `/ratify` in the console to accept the new endpoint, and `/build implement @specs/NNN-your-spec.md in @project/<name>`. Specs must be ratified first (a build starts only against a spec whose current hash is in `ratified.lock`, or after you approve the small-change exemption).

## 2. Gemini API through Google sign-in (one-time setup)

1. In the Google Cloud console, create or pick a project, and enable the **Generative Language API**.
2. Configure the OAuth consent screen (user type External, add yourself as a test user) and create an **OAuth client ID** of type **Desktop app**. Copy the client ID and secret.
3. Store the secret in your keychain under Delphi's service (it never goes in a file): `security add-generic-password -s delphi -a google-client-secret -w` (it prompts).
4. Config:

```toml
[auth.google]
flow = "loopback_pkce"
issuer = "https://accounts.google.com"
client_id = "<your client id>.apps.googleusercontent.com"
client_secret = "keychain:google-client-secret"
scopes = ["https://www.googleapis.com/auth/cloud-platform"]   # unverified: see below
authorization_params = { access_type = "offline", prompt = "consent" }

[[endpoints]]
id = "gemini-oauth"
family = "gemini"
path = "api"
project = "<your google cloud project id>"     # sent as x-goog-user-project; required for user tokens
credential = "oauth2:google"
models = { fast = "gemini-3.8-flash-low", workhorse = "gemini-3.8-flash-medium", frontier = "gemini-4-argon-medium" }
```

5. `/ratify`, then `delphi auth login google` (a browser opens; Delphi listens on 127.0.0.1 for the redirect, or use `--manual`). `delphi auth status` should say "signed in".
6. `delphi endpoints test gemini-oauth --probe`.

**Unverified, and what to watch for:** which OAuth scope the `generateContent` endpoint accepts for a user token (`cloud-platform` is the broad one that should work; the narrower `generative-language` scopes may too), whether model IDs on this API match the `agy` names above (use the IDs shown in AI Studio), and whether Google applies the free tier to user-token calls. The probe reports the HTTP error verbatim, so the first failure tells us which of these it is.

## 3. Gemini harness (`agy`)

```toml
[[endpoints]]
id = "agy-sub"
family = "gemini"
path = "harness"
harness = "agy"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "gemini-3.8-flash-low", workhorse = "gemini-3.8-flash-medium", frontier = "gemini-4-argon-medium" }

[access]
role_family = { red-team = "gemini" }    # for example: have Gemini audit what Claude built
```

Model names come from `agy models`.

## What to expect in the console

Approval cards for each command the Builder's CLI wants to run (unless an `allow` rule covers it), one card for the gate commands, and a Red card for the merge showing host-executed files first. `/y`, `/a` (allow similar), `/n`, `/d` (view).
