# Away mode

Three tools for when you are not at the terminal: a **relay** that sends approval cards to your email or Telegram and takes your replies, **timers** that wake a role later, and **hooks** that run your own commands on events. The design is [`specs/014-relay-and-away-mode`](../../specs/014-relay-and-away-mode.md), [`017`](../../specs/017-timers.md) and [`018`](../../specs/018-operator-hooks.md).

None of these lowers a safeguard: a Red card is denied if nobody answers, a merge is never approved by reply, and everything you answer from outside the console is checked for who sent it.

## The relay

Pick a channel in `config.toml`. Email needs a mailbox the relay owns and your own address:

```toml
[relay]
channel = "email"
detail = "summary"            # summary | full
step_up = "totp"              # require a one-time code to approve a Red card by reply
totp_credential = "keychain:delphi-relay-totp"   # keychain:…, env:… or command:[…]

[relay.email]
mailbox = "delphi-relay@example.com"   # the relay's own address
operator = "you@example.com"           # the only address whose replies count (plus optional also_allow = [...])
imap_host = "imap.example.com"
smtp_host = "smtp.example.com"
credential = "keychain:delphi-relay-mail"
# tls = "implicit"   # implicit | starttls | none
```

or Telegram:

```toml
[relay]
channel = "telegram"

[relay.telegram]
chat_id = 123456789           # your numeric Telegram user id, not a username
credential = "keychain:delphi-relay-telegram"
```

Other `[relay]` keys: `require_auth` (default true: refuse answers the channel could not authenticate), `notify_tasks` (default true), `max_outstanding` (default 20), `max_per_hour` (default 60), `poll_secs` (default 30, minimum 5). `[relay]` is ratified: an edit takes effect only after you `/ratify` it.

Then:

```sh
delphi relay check            # validate the settings and reach the channel
delphi relay totp-enroll      # make the authenticator secret (shown once), then ratify the settings
delphi relay totp-check <code>
delphi relay                  # run it
```

`delphi relay` is long-running and must run in a terminal or a tmux pane: the daemon issues the relay's token only to a process with a controlling terminal. It keeps running until you stop it.

Combine it with a headless run so a script can ask you:

```sh
delphi run "/build …" --on-card relay --timeout 2h
```

Cards wait for your reply (the merge card is still denied by the run, which ends at "ready to merge"), and the summary records how each card ended and who answered. `[away] approval_park_max_sec` bounds how long a parked approval waits.

## Timers

Wake a role later, once or on a schedule:

```
› /timer 1h review the failing tests
› /timer @architect 1h revisit the retry spec
› /timer list
› /timer cancel <id>
```

From a shell: `delphi timers [list | set <role> (--in <5m> | --at <time>) [--every <5m>] <note…> | cancel <id>]`. Timers live in the daemon, and a wake is a task like any other, so it asks for the same cards.

## Hooks

Run your own commands on events (a desktop notification, a chat message, a log line):

```toml
[[hooks.observe]]                 # runs out of process on events; its output is ignored
name = "notify-done"
events = ["task.completed"]       # exact kinds, a prefix such as "approval.*", or "*"
command = ["/usr/local/bin/notify", "Delphi finished a task"]   # an argv array, never a shell string

[[hooks.deny]]                    # runs in the daemon before a card is raised; it can only refuse
name = "no-force-push"
tools = ["run_command"]           # tool names, or "*"
command = ["/usr/local/bin/check-command"]
```

Exit 0 from a deny hook means "no objection"; anything else, a timeout, or a command that will not start means no, so a broken deny hook blocks the action rather than letting it through. A hook never approves anything, and runs with a cleared environment. Run `delphi hooks` in a terminal or tmux pane to dispatch the observe hooks. `[hooks]` is ratified, like `[relay]`: an edit changes nothing until you `/ratify` it. Details are in [`specs/018`](../../specs/018-operator-hooks.md).
