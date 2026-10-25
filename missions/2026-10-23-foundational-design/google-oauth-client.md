---
id: google-oauth-client
title: "Google OAuth client: what to create, what to configure, what to run"
author: builder
timestamp: 2026-10-25T00:00:00Z
source_type: internal_generated
confidence: medium                 # the Delphi side is tested against a mock identity provider; Google's console wording and the MCP server URLs are from memory and the spec's "reported" notes
status: active
source_mission: 2026-10-23-foundational-design
tags: [howto, oauth, google, connectors, gemini]
---

# Google OAuth client

One OAuth client serves everything Google in Delphi: the Gemini API with your own sign-in ([`first-real-run.md`](first-real-run.md) §2) and the Drive, Calendar and Gmail connectors ([`specs/010`](../../specs/010-connectors-and-tool-access.md) §7). Delphi ships no client id, so this is the one step only you can do: it needs your Google account and your own Google Cloud project. Nothing here is billed by creating the client.

## 1. In the Google Cloud console (about 10 minutes)

1. Create or pick a project at `console.cloud.google.com`. Note its **project id**.
2. **APIs & Services > Library**: enable the APIs you will use: **Generative Language API** (Gemini), and for connectors **Google Drive API**, **Google Calendar API**, **Gmail API**. The remote MCP servers (`specs/010` §7, URLs still "reported") may also need their own service switched on; the first live `delphi connectors test` says so if they do.
3. **APIs & Services > OAuth consent screen** (called Google Auth Platform > Branding/Audience in the newer console): user type **External**, app name anything ("Delphi"), your address as support and developer contact. Add yourself under **Test users**.
4. Under **Data access** (older console: *Scopes*), add the scopes below. Without them the consent page does not offer them.
5. **Credentials > Create credentials > OAuth client ID**, application type **Desktop app**. Copy the **client id** (ends `.apps.googleusercontent.com`) and the **client secret**.

**Why Desktop app and not "TV and limited input" (device flow):** Google allows the device flow only a short list of scopes (`openid`, `email`, `profile`, `drive.file`, `drive.appdata`, `youtube`). Drive read, Gmail, Calendar and `cloud-platform` are refused there. So Google uses `loopback_pkce`: Delphi listens on `127.0.0.1` and your browser is redirected to it. A Desktop client accepts any `127.0.0.1` port, so no redirect URI is registered and `redirect_port` stays unset. Google still requires the client secret at the token endpoint for Desktop clients; it is not confidential in the usual sense, but Delphi keeps it in the keychain like any secret.

**Scopes**

| For | Scope |
| :-- | :-- |
| Sign-in identity for the account pin (connectors) | `openid`, `email` |
| Gemini API | `https://www.googleapis.com/auth/cloud-platform` (broad; narrower `generative-language` scopes may also work: unverified) |
| Drive (read only, v1) | `https://www.googleapis.com/auth/drive.readonly` |
| Calendar | `https://www.googleapis.com/auth/calendar.readonly` (read tools) |
| Gmail | `https://www.googleapis.com/auth/gmail.readonly` (read tools); `gmail.compose` for drafts |

Request only what you will use: adding a scope later means a new consent and a new login.

**Test mode lasts a week.** While the consent screen is in *Testing*, Google expires refresh tokens after 7 days for these scopes, so you will sign in again weekly. Setting the app to *In production* removes that; Google then shows an "unverified app" warning that you click through for your own account (verification is only needed to go beyond 100 users).

## 2. In the keychain (never in a file)

macOS (it prompts for the value):

```
security add-generic-password -s delphi -a google-client-secret -w
```

Linux: `secret-tool store --label=Delphi service delphi name google-client-secret`.

## 3. In `config.toml`

In `$DELPHI_HOME/workspaces/<name>/config.toml`:

```toml
[auth.google]
flow = "loopback_pkce"
issuer = "https://accounts.google.com"
client_id = "<your client id>.apps.googleusercontent.com"
client_secret = "keychain:google-client-secret"
scopes = ["openid", "email", "https://www.googleapis.com/auth/cloud-platform", "https://www.googleapis.com/auth/drive.readonly"]
authorization_params = { access_type = "offline", prompt = "consent" }   # without these Google issues no refresh token
```

For Gemini, add the endpoint from [`first-real-run.md`](first-real-run.md) §2 (`credential = "oauth2:google"`, `project = "<project id>"`). For Drive, add a `[connectors.<name>]` block with `login = "google"`, `identity = "google"` and `account = "<you>@gmail.com"` (`specs/010` §3); `hosts` must include the MCP server's host and `openidconnect.googleapis.com`.

Then run `/ratify` in the console (`delphi chat`): `[auth.*]`, `[[endpoints]]` and `[connectors.*]` are trusted only once ratified.

## 4. Commands, in order

Use `--home <dir>` if your workspace is not the default.

| Command | What it proves |
| :-- | :-- |
| `delphi auth login google` | Browser opens; approve; the terminal says "signed in". Add `--manual` on a headless machine or if the browser is on another computer: paste the address the browser fails to load. |
| `delphi auth status` | `google` shows `signed in`. |
| `delphi endpoints test gemini-oauth --probe` | A real Gemini call with your user token. The first failure prints Google's HTTP error verbatim and tells us which unverified point (scope, model ids, free-tier quota) was wrong. |
| `delphi connectors test <name>` | The MCP server answers `tools/list` with your token. `delphi connectors pin <name>` then proposes the tool definitions for `/ratify`. |

`delphi auth login google` stops before opening a browser, with the fix named, when: no `[auth.google]` block is ratified; `client_id` is a placeholder or not a Google id; `client_secret` is missing or its keychain entry does not exist; or `authorization_params` lacks `access_type = "offline"`.

## 5. What to send back if something fails

The exact line Delphi printed. Do not paste tokens or the client secret: Delphi's errors never contain them. Likely first findings: a scope Google refuses for your app type (`invalid_scope`), an API that is not enabled on the project (HTTP 403 naming it), or an MCP server URL in `specs/010` that is not live.
