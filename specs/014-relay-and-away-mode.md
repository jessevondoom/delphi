---
id: spec-014-relay-and-away-mode
title: "014: The Relay and Away Mode (approval cards by email)"
author: architect
timestamp: 2026-10-25T20:00:00Z
status: approved   # by the operator, 2026-10-25
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, relay, approvals, email, away-mode, security]
---

# Spec 014: The Relay and Away Mode

## 1. Scope & Objectives
A build that needs a person's decision should be able to wait for them when they are away from the terminal, and they should be able to answer from wherever they are, without any of the properties of an approval card being weakened. This spec adds three things: a **relay scope** on the daemon, a **relay process** that carries cards out and answers back, and **parking** so an unanswered card does not fail the task.

Origin and reasoning: [`gap-closure-plan-2026-10-25.md`](../missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md) §1. Implemented: the relay scope, the email channel, parking, `delphi relay`, and `delphi run --on-card relay`. TOTP step-up (§7.1) is built (`delphi_core::totp::Verifier`). Not yet implemented: Slack and other chat channels beyond Telegram (§7), and optimistic decisions (spec 015).

### 1.1 Non-Goals
- **Starting work from a message.** A reply is a verb and a code, nothing else. Command ingress from a chat channel is prompt injection waiting to happen and is out of scope.
- **Approving a Red action remotely without a second factor.** A relay can deny anything and approve Amber; it approves Red only with the operator's authenticator code, which the daemon checks (§3, §7).
- Reading the operator's own mailbox. The relay reads a dedicated one.

### 1.2 Simpler Alternative Considered
**Forward the console over the network** (an SSH tunnel to the console socket, or a web console). Rejected: it hands a remote device the console's authority (budget, permissions, ratification, rewind) to answer a yes/no question, and it needs an inbound port or a tunnel. The relay needs only an outbound mail connection and can do exactly one thing.

## 2. The relay scope
A third client scope beside `console` and `narrator`, on its own socket (`<run>/<hash>.relay.sock`, `0600`), like the Narrator's.

- **Tokens.** The daemon issues a relay token only to a peer with a controlling terminal, the same rule as the console (`specs/005` §2.1). So the relay is started by the operator in a terminal, or a `tmux` pane on a server; no token is stored on disk, and a script that merely runs as the same user cannot obtain one by reading a file. A relay token is refused on the other two sockets, and theirs on this one.
- **Methods.** A fixed list, refused by name for anything else: `session.ping`, `events.subscribe`, `approval.list`, `approval.relay_resolve`, `budget.get`. The list is pinned by a test.
- **Events.** A relay client is sent only `approval.requested`, `approval.resolved`, `approval.parked`, `task.completed` and `task.failed`, and never live model text (the 60 Hz delta frames). It carries cards to a person; it has no need to see what agents say.
- **`approval.relay_resolve`.** Takes `approval_id`, `payload_blake3`, `decision` (`allow` or `deny` only), `channel` and `sender`. It is narrower than the console's `approval.resolve`:
  - it needs the relay scope;
  - **it cannot allow a Red card**: that is refused by the daemon, whatever a relay sends, while denying a Red card is accepted;
  - "allow similar" is not a relay decision;
  - the answer is bound to the payload hash, loses to an earlier answer, and cannot revive a timed-out card, exactly as at the console;
  - the log records who answered as `relay:<channel>:<sender>`. (Before this spec the log recorded `console` for every non-timeout answer; it now records the broker's actual answerer, which for a console client is `console-<id>`.)

## 3. Tier policy
| Card | Console | Relay (email) |
| :-- | :-- | :-- |
| Amber | approve, allow similar, deny | approve, deny |
| Red: merge, pull request, constitution or ratification change | approve, deny | **deny only**, even with step-up (014-A1) |
| Red: outbound, destructive, other | approve, deny | deny; **approve only with step-up** (§7.1) |

A Red card is still sent to the operator, so they know it is waiting. Without step-up the text says "approve it at the console, or deny it here" and there is no approve link; with it (§7.1) the card explains the extra code, except on a merge-boundary card (014-A1), which is answered only at the console.

## 4. The relay process (`delphi relay`)
Run in a terminal: `delphi relay` (the relay), `delphi relay check` (log in to the mailbox and report, sending and reading nothing).

**Configuration is ratified.** `[relay]` is one of the pinned `config.toml` blocks (`lock::ACCESS_KEYS`), like `[[endpoints]]`. The relay reads the **ratified** copy; an edit to the file changes nothing until the operator ratifies it, so no local edit, and no agent, can add an address that may answer a card. With no ratified `[relay]` the command refuses to start and says so.

```toml
[relay]
channel = "email"
detail = "summary"            # "full" also sends the card's payload (truncated); the default sends the summary only
poll_secs = 30                # how often to read replies (minimum 5)
max_outstanding = 20          # cards waiting at once
max_per_hour = 60             # messages sent in any hour, notices included
notify_tasks = true           # also say when a task finishes or fails
require_auth = true           # refuse replies the mail provider did not authenticate

[relay.email]
mailbox = "delphi.relay@gmail.com"      # the dedicated Delphi mailbox; cards are sent from it and replies read from it
operator = "you@example.com"            # where cards go, and the sender whose replies count
also_allow = []                         # more addresses whose replies count
imap_host = "imap.gmail.com"            # port 993 with tls = "implicit" (default); 143 with "starttls"
smtp_host = "smtp.gmail.com"            # port 465 with "implicit"; 587 with "starttls"
credential = "keychain:delphi-relay-mail"   # keychain:, env: or command: (specs/009 §4); never in the file
authserv_id = "mx.google.com"           # the authserv-id your mailbox provider writes in Authentication-Results
```

Behaviour:
- It attaches on start, announces cards already waiting, then forwards each new card and polls for replies. It reconnects by itself if the daemon restarts, and on start it reads and discards any unseen mail already in the inbox (replies to cards that no longer exist).
- **Rate limits** hold: at most `max_outstanding` cards at once and `max_per_hour` messages in any hour. Past them, cards are not sent (they still time out normally).
- Expired cards are forgotten. A card resolved elsewhere (the console, a timeout) is closed and the operator is told in one short notice, so a stale email does not look live.

## 5. The email channel
**Transport.** Outbound SMTP and inbound IMAP over TLS (implicit, or STARTTLS; plain text only to a mailbox on this machine, which the tests use). No inbound port is opened. Each poll opens a new IMAP session, reads unseen mail in `INBOX` up to 50 messages, skips and marks read anything over 256 KiB, marks what it read as seen, and logs out. It never deletes, moves or sends through IMAP.

**Outgoing.** Plain text only: no HTML, no images, no tracking links. Subject `[delphi:<approval_id>] <summary>`. The body is the card (the summary, who asked, the tool, the tier, a prefix of the payload hash, how long it waits), then the two lines the operator may send back, and `mailto:` links that open a pre-filled reply. Headers `Auto-Submitted: auto-generated` and `X-Auto-Response-Suppress: All` stop out-of-office replies. Control characters are removed from anything that reaches a header.

**What counts as an answer.** All of these:
1. The message is not auto-generated (`Auto-Submitted`, `Precedence: bulk|junk|list`, `X-Auto-Response-Suppress`, `List-Id`, an empty return path, `mailer-daemon@`, `postmaster@`) and has not been seen before (by `Message-ID`).
2. The sender is on the allowlist (`operator`, `also_allow`).
3. With `require_auth`, the receiving provider vouches for the sender: the **topmost** `Authentication-Results` header from the configured `authserv_id` (or the topmost of any, if none is configured, which is weaker) reports `dmarc=pass`, or `dkim=pass` or `spf=pass` aligned with the `From` domain. A header lower down, or from another server, is ignored.
4. The **first non-blank line of new text** is exactly `APPROVE <code>` or `DENY <code>`, where the code is 32 hex characters. Text below it, quoted history, signatures and attachments are never read; a reply written below a quote is not accepted. An HTML-only reply is accepted by its first visible line.
5. The code matches a card still waiting. **Codes are single-use** (128 random bits per card, held only as a hash), gone when the card is resolved, expires or times out, and a re-raised card has a new one.

The **code is the secret; the address is a filter**. A message that fails any step is ignored and nothing is sent back to its sender, so the relay cannot be used to send mail to a stranger. The one exception is a message from an allowed address that fails authentication, which produces at most one notice per ten minutes to the operator.

**What leaves the machine.** By default only the card's one-line summary, the role, task and tool, and a hash prefix. The provider that carries the mail sees that. `detail = "full"` adds the payload. Everything passes through the daemon's redactor first.

## 5.1 The Telegram channel
`[relay] channel = "telegram"` carries the same cards to a Telegram bot. It polls the Bot API (`getUpdates`), so no port is opened, and uses the same core: single-use codes, rate limits, notices, step-up.

- **Cards** are plain-text messages (no markup is parsed, so nothing in a summary can format itself) with **Approve** and **Deny** buttons. A tap sends the same `APPROVE <code>` / `DENY <code>` the email links do, so it is answered by the same checks. A Red card with step-up has an Approve button too; tapping it asks for the authenticator code, and the operator types `APPROVE <code> <123456>`.
- **The sender** is the numeric user id Telegram attaches to every update (`from.id`), not a username, which can change hands. The allowlist is `chat_id` and `also_allow`, both ids.
- **Only a private chat with that user counts.** A message in a group or a channel, or one whose chat is not the sender's own, is not authenticated and is ignored (and the operator is told once). A bot's message is never acted on.
- **Replies** are the button's data or the first non-blank line of a typed message. Updates are acknowledged by offset so none is delivered twice.
- **The bot token** is a credential reference (`keychain:`, `env:`, `command:`), never in the file. It appears in every request URL, so it is removed from every error message; the HTTP client's errors are scrubbed and tested for it. Plain `http` is accepted only for a server on this machine.

```toml
[relay]
channel = "telegram"

[relay.telegram]
chat_id = 123456789                      # your numeric user id (message @userinfobot to find it)
also_allow = []                          # more numeric ids whose replies count
credential = "keychain:delphi-relay-telegram"   # the token BotFather gave you
```
Set up: make a bot with @BotFather, open a chat with it and press Start (Telegram does not let a bot message someone who has not), store the token, add the table, `/ratify`, then `delphi relay check` (it asks the bot who it is) and `delphi relay`.

## 6. Parking (away mode)
`[away] approval_park_max_sec` (default 0, off) is how long a card may stay outstanding across re-raises.

- A card still times out and is denied at its normal timeout, and its id is dead: a late answer to it cannot revive it. This is the existing rule and it is unchanged.
- With parking on, the task is not told no. A fresh card for the same action is raised with a new id, and the event records `reraised_from`. Between cards nothing is spent: the task is waiting on the operator, not on a model.
- When `approval_park_max_sec` is used up, the last card's denial is delivered to the agent as before.
- `delphi run --on-card relay` answers nothing and waits, which pairs with this: the run's `--timeout` bounds how long. The merge card is still denied by the run so it can end at "ready to merge".

## 7. Not yet built
- **Slack** (Socket Mode). It needs a WebSocket client. The channel interface is two functions (`send`, `poll`) and the rules above live once, in the core, so it is an adapter and nothing more.
- A `doctor` line for the relay.

## 7.1 Step-up: approving a Red action by reply (TOTP)
A stolen mailbox or chat account must not be able to approve a merge. With step-up, a Red approval **other than a merge, pull request or constitution/ratification change (014-A1)** needs something the mailbox does not hold: a six-digit code from the operator's authenticator app.

- **Setup.** `delphi relay totp-enroll` makes a secret and shows it once, as a key and an `otpauth://` link, with the keychain command to store it. In `config.toml`: `[relay] step_up = "totp"` and `totp_credential = "keychain:delphi-relay-totp"` (any `keychain:`, `env:` or `command:` reference; never the secret itself). Both are in the ratified `[relay]` block. `delphi relay totp-check <code>` confirms the phone and the stored key agree.
- **Use.** A Red card's message explains it. The operator replies `APPROVE <card code> <6 digits>`. A code on a `DENY`, or a malformed one, is not a reply. `APPROVE <card code>` alone leaves the card code unspent and tells the operator what to add.
- **The daemon checks, not the relay.** `approval.relay_resolve` takes the code with the answer and verifies it against the secret (RFC 6238, HMAC-SHA1, 30-second steps, one step of drift either side). Each code works **once**. Five wrong codes in five minutes lock step-up out for five minutes, even for the right code. The used-code window and the failure list are kept in `local/totp-state` (atomic write, mode 0600, pruned by the window) and read at start, so a restart resets neither; an unreadable or corrupt file starts the daemon locked for five minutes rather than empty (014-A1). A card can still be answered at the console meanwhile. The answer is logged as `relay:<channel>:<sender>+totp`.
- **What it does not change.** Without `step_up` a relay still cannot approve a Red card. Amber approvals need no code. The card code stays single-use, so a wrong authenticator code spends it: one guess per card.
- **What it does not protect.** Someone who holds both the mailbox and the phone. The secret is as safe as the credential store it sits in.

## 8. Setting it up
1. Make a **dedicated mailbox** for Delphi (a new free Gmail account works; turn on 2-Step Verification and make an app password). Never use your own mailbox.
2. Store the password: `security add-generic-password -s delphi -a delphi-relay-mail -w` (macOS; it prompts).
3. Add `[relay]` and `[relay.email]` to the workspace's `config.toml` as in §4.
4. In the console, `/ratify` the change (it is shown as a Red `config-access` proposal).
5. `delphi relay check`: it logs in to both servers and reports what it will do.
6. In a terminal or tmux pane: `delphi relay`. Leave it running.
7. For unattended work: `delphi run "…" --on-card relay`, optionally with `[away] approval_park_max_sec = 14400` so a card that nobody answers for a while is raised again instead of failing the task.

### 8.1 Running Delphi on another machine
There is no remote execution backend, on purpose (a second sandbox is a second thing to get wrong). The same result comes from running all of Delphi on the machine that should do the work, over `ssh`, with the relay as the way back to you:
1. On the remote machine: install Delphi, `delphi init` and set up model access as usual, and do §8 steps 1 to 5 there.
2. In a `tmux` session: one pane with `delphi relay`, which keeps running when you disconnect (it needs a terminal, and a `tmux` pane is one).
3. In another pane, start work with `delphi run "…" --on-card relay` (optionally with `--timeout` and `[away] approval_park_max_sec`), or `delphi run "/build --optimistic …"` to review everything at the end.
4. Detach. Cards and notices arrive by email; Amber cards are answered by reply; a merge, any Red card and the review of provisional decisions are done in the console when you next attach (`tmux attach`, then `delphi`).

Nothing here opens a port. Whether a true backend is worth building is a question for `delphi needs`, once this has been used for real work.

### 8.2 Setting up Telegram instead of email
1. Talk to @BotFather: `/newbot`, and keep the token.
2. Open a chat with your new bot and press **Start**.
3. Find your numeric user id (@userinfobot will tell you).
4. `security add-generic-password -s delphi -a delphi-relay-telegram -w` (it prompts for the token).
5. Add the `[relay.telegram]` table of §5.1, `/ratify`, `delphi relay check`, then `delphi relay` in a terminal or tmux pane.

## 9. Verification Plan
Unit tests (`relay/`): verdict parsing; a card goes out with a token and, by default, only its summary; the token approves once and only once; strangers get no answer and no reply; the right address without authentication is refused and the operator told once, and the forgery does not use up the real code; a wrong token approves nothing and a duplicate message id is not processed twice; replies that are not a verdict or are auto-generated do nothing; a Red card can be denied but never approved (and has no approve link); what the daemon says back (too late, refused, down) reaches the operator; a card resolved elsewhere is closed and its code is dead; a re-raised card has a new code and the old one stays dead; caps hold; header injection through a summary is neutralised.

Email tests: only the first line counts (quoted history, top-posting, reply-below-quote, blank lines); forged and failed authentication; a header the sender wrote does not vouch for them; DKIM/SPF alignment; out-of-office, bounce and bulk mail; HTML-only, quoted-printable and multipart bodies with attachments; no panic on garbage; plain-text composition with pre-filled links and no way to approve Red. Network tests against fake SMTP and IMAP servers on localhost: delivery with headers, login failures, unseen-only reads, oversize skipped, mark-seen; plain text refused for any host but localhost; the password never appears in `Debug` output.

Step-up: the TOTP implementation matches the RFC 6238 and RFC 4226 test vectors and reads base32 as authenticator apps show it; a code is accepted one step either side and no further; a code works once; five wrong codes lock it out for five minutes, even for the right code; at the daemon, a Red approval is refused without step-up configured, without a code, with a wrong one, with a replayed one and with an unusable secret, is accepted with a valid one and logged `+totp`, and a Red denial never needs a code; at the relay, a code is accepted only on an approval, is forwarded only for a Red one, a missing code does not spend the card code, and a wrong one does.

Telegram: a card goes to the operator's private chat with buttons whose data is the reply, in plain text, cut to Telegram's limit; a tap and a typed reply from a private chat are answers and the tap is acknowledged; updates are acknowledged by offset so none repeats; a group message, a chat that is not the sender's, a bot and an unknown update kind are not answers; the token's shape and the `https` rule are checked; failures are reported; and the real HTTP client, against a local server, posts JSON to the bot path, turns a refusal into an error, and never puts the token in an error message (including a refused connection).

Daemon tests (`service.rs`, `tests/daemon.rs`): each socket accepts only its own token and the relay socket issues only relay tokens; the relay method list is fixed and everything else is refused by name; a relay receives only the listed event kinds and never model text; a relay can approve Amber, deny anything, and never approve Red, with a wrong hash, a second answer and the console/relay method split all refused; parking re-raises under a new id, the old id stays dead, nothing runs while parked, no model call is made, and the window ends.

End-to-end (`tests/e2e.rs`): a headless run raises a card, `delphi relay` under a terminal mails it, a forged reply is ignored, the operator's real reply approves it, the write runs, and the log names `relay:email:<sender>`; an unratified `[relay]` is ignored.

Not covered, and to do before relying on it: a live run against a real mailbox (§8 steps 5 to 7 on a Gmail account), because no provider-specific behaviour (app-password login, the exact `Authentication-Results` text, delivery delay) has been seen yet.

## 10. Amendment 014-A1: The merge boundary and persistent step-up state (council 013 D12, TI14; `[REV-SEC-09]`)
GOVERNANCE §1.1 keeps final merges with the operator at the console; §3 and §7.1 as first written let a TOTP code approve one by reply, which disagreed with the away-mode guide, `specs/016` §3 and §8.1. Resolved in favour of the console:

1. **Merge boundary.** `approval.relay_resolve` refuses `allow` for a card whose tool is `merge`, `git_merge`, `pull_request`, `constitution`, `constitution_change` or `ratify`, with or without a valid code, and says it is approved only at the console. Denying stays open to the relay. One check, in `service.rs`, before step-up is consulted, so a code is not even spent. Other Red cards keep §7.1.
2. **Persistent replay and lockout.** The daemon's TOTP verifier saves its used-code window and failure list to `local/totp-state` (temp file and rename, 0600, at most 16 KiB, pruned by the code window) and loads it at start. A file that exists but cannot be read or parsed fails closed: step-up is locked for five minutes from the first check. If the file cannot be written, the check refuses.
3. **Residual (not changed).** The relay's table of outstanding card codes and tokens (§4, §5) is still in the relay process's memory. A restart of the relay drops them, which only loses pending replies (the card must be re-sent); it cannot approve anything, because the daemon still needs the card's payload hash, and for a Red card a fresh code from the phone. Persisting that table is deferred.
