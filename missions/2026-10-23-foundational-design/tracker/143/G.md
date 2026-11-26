---
state: built
date: 2026-11-26
title: "delphi bridge setup claude: the token in the secret store, the documentation's lines"
---

Slice G of spec 143. **Built 2026-11-26** on `knowledge-v2` (no pull request yet; one stack per spec at the end of the program). Every test uses a fake keychain, a fake test call or no test; no test reads a keychain or a real token.

What is built:

- **`delphi bridge setup claude`** (`crates/delphi/src/bridge_cmd.rs`, §2.9 item 1). At a terminal it:
  - prints the quota notice of §3 first: the bridge spends the operator's own subscription quota, and heavy or unattended use belongs on an API key;
  - asks the operator to run `claude setup-token` themselves, then takes the token with hidden input;
  - makes **one test call** through the bridge's own launch (`harness_single::launch`: a home built empty for the call, tools off, model alias `haiku`, the token in the child's environment only);
  - only after the test passes, stores the token in the keychain (service `delphi`, account `bridge-claude`) and writes the reference `keychain:bridge-claude`, never the token, to `[bridge.claude] credential`. The new file is checked by the daemon's own parser before it is written;
  - shows the change for the operator to ratify.

  A token whose test fails, or text that is not a token, is not stored and nothing is written. Without a terminal it takes only `--credential` with a reference (`keychain:`, `env:` or `command:`), tests the token that reference names, writes the reference and ratifies nothing. A value that is not a reference is refused without being repeated. Inside an agent seat it is refused whatever the flags. `--no-test` skips the call.
- **The doctor** names `delphi bridge setup claude` in its warning when no harness can serve single calls (AC-21's last clause).
- **The documentation's lines** (§3): `docs/guides/model-access.md` has a new section, "Background jobs on your Claude subscription": the bridge spends your own quota, heavy or unattended use goes on an API key, and how to set it up. `docs/reference/config.md` names the command under `[bridge.claude] credential`. The command's own help says the same. The CLI reference and man page are regenerated.

Verified (tests):

- `bridge_cmd::tests::at_a_terminal_the_token_is_tested_then_kept_in_the_keychain_and_only_its_reference_is_written`: the notice comes before the question; the typed token is the one tested and the one stored, under `bridge-claude`; the rest of config.toml is kept; the token is in no file and on no screen.
- `bridge_cmd::tests::a_token_that_fails_its_test_or_is_no_token_is_neither_stored_nor_written`.
- `bridge_cmd::tests::without_a_terminal_only_a_reference_is_taken_and_nothing_is_asked_or_stored`.
- `tests/cli.rs::bridge_setup_needs_a_terminal_or_a_reference_and_is_refused_in_a_seat`, on the built binary: refused without a terminal, with a token passed as a flag (it is never repeated), inside a seat, and for another CLI. No refusal writes anything. With a reference and `--no-test` only the `[bridge.claude]` block is added.
- AC-21's launch half and its token scan were built and tested in slice B.

Not verified, or left:

- **Not run live.** The test call against the real CLI happens when the operator runs the command, at the start of slice E.
- The spike stored the operator's token under keychain service `delphi-bridge-claude`, which no reference names. Running `delphi bridge setup claude` once stores it where `keychain:bridge-claude` reads it; the old entry can then be deleted by hand (`security delete-generic-password -s delphi-bridge-claude`).
- The **sign-in rung** (`--sign-in`, AC-22, AC-23) is not built: slice B's spike showed that rung 1 holds.
- The CLI process does not register the token with a redactor: it never prints or logs the token, and the daemon registers it when it resolves the reference (slice B).
