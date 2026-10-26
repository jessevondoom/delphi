---
status: approved
---
# A keyless local endpoint (Ollama) for the model pool

Delphi needs a free last-resort model for when paid quota runs out: a local Ollama server. It speaks the OpenAI-compatible chat API on a loopback address and needs no credential. The configuration layer (`crates/delphi-adapter`) does not allow that today: every endpoint must name a credential source, and the gateway request builder always sends a bearer token.

## Required behavior (crate `delphi-adapter`)

1. **A credential reference `none`.** `CredentialRef::parse("none")` yields a new variant `CredentialRef::NoAuth`, and resolving it gives `Resolved::None` (like `harness-login`). The "unknown credential source" error message lists `none` among the accepted sources.
2. **Only for a local server.** Config validation (`Config::parse` and friends in `config.rs`) rejects `credential = "none"` on any endpoint that is not a `gateway-compat` endpoint whose `base_url` is a loopback address (`http://localhost`, `http://127.0.0.1`, or `http://[::1]`, with or without a port). The error message for an endpoint includes the words `only for a local server`. `Config::parse` of a valid loopback compat endpoint with `credential = "none"` succeeds.
3. **Loopback detection handles IPv6.** The loopback check (`is_loopback` in `config.rs`) must treat a bracketed IPv6 literal as one host: `http://[::1]:11434` is loopback, and a colon inside the brackets is not a port separator.
4. **A `local` family.** `[access] role_family` may name the family `local` alongside `claude` and `gemini`; the error for an unknown family lists all three. A role mapped to `local` resolves through `family_for_role` to `"local"`. A `local` endpoint declared after the paid endpoints is last in `pool_entries()` (the pool reaches families in declaration order), so it is the last resort.
5. **No prices needed for a free model.** The configuration problems check that flags a catalog model without prices ("budget reservation is impossible") must not flag a model on an endpoint whose `cost_model` is `local` (as it already does not for `subscription`).
6. **No authorization header.** The OpenAI-compatible request builder (`platform.rs`, the gateway-compat path) sends **no** `authorization` header for an endpoint whose credential is `none`; it must not invent a token. Other endpoints behave exactly as before. The URL, model and streaming fields are unchanged.

## Where

`crates/delphi-adapter/src/config.rs`, `credential.rs`, `platform.rs`. Update the doc comment on `role_family`. Optionally add a commented example endpoint to `crates/delphi/config.example.toml` (declared after the paid endpoints, `credential = "none"`, `cost_model = "local"`).

## Notes

- Tests for this are not visible to you. They parse configs with a keyless Ollama endpoint, check the error text above, check `pool_entries()` order and `family_for_role`, and inspect the built HTTP request's headers.
- Run `cargo test -p delphi-adapter --lib` to check you have not broken existing behavior.
