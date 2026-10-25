---
id: provider-readiness-2026-10-25
title: "Provider readiness matrix and API-readiness criteria"
author: strategist
timestamp: 2026-10-25T16:30:00Z
source_type: internal_generated
confidence: medium
status: draft
source_mission: 2026-10-23-foundational-design
tags: [plan, providers, api-keys, readiness, harness]
---

# Provider readiness (2026-10-25)

P0 of the [gap-closure plan](gap-closure-plan-2026-10-25.md) §2: record, for each way Delphi reaches a model, how far it is proven. Sources are the code, its tests and the earlier notes in this mission; nothing here was run live in this pass.

**Headline: no direct-API path has a live test in the repository.** The only `#[ignore]` live tests cover the two subscription CLIs (`claude`, `agy`), the harness build, and the Council. Everything that spends API money rests on unit tests against scripted servers plus the end-to-end suite's mock Claude server.

## 1. The matrix

"Built" means the driver exists and has unit tests. "Live" means a recorded run against the real provider.

| Family | Path | Built | Tests in the repo | Live |
| :-- | :-- | :-- | :-- | :-- |
| Claude | Direct API (`api`) | yes (`claude.rs`) | 16 unit tests; end-to-end suite against a mock Claude server | **none recorded** |
| Claude | AWS Bedrock (`cloud`, `platform = "bedrock"`) | yes (`InvokeModelWithResponseStream`, SigV4 `sigv4.rs`, event stream `eventstream.rs`) | 5 and 3 unit tests; request-envelope and endpoint config tests | **none recorded** |
| Claude | Google Vertex (`cloud`, `platform = "vertex"`) | yes (`streamRawPredict`, gcloud ADC credential) | credential tests; config validation | **none recorded** |
| Claude | Gateway, native passthrough | yes (config, probing) | config tests | **none recorded** |
| Claude / any | Gateway, compat (`/v1/chat/completions`) | yes (`openai.rs`) | 4 unit tests | **none recorded** |
| Gemini | Direct (Developer) API with an API key (`x-goog-api-key`) | yes (`gemini.rs`, `platform.rs`) | 11 unit tests | **none recorded** |
| Gemini | Direct API with Google sign-in | yes (`oauth2.rs`) | 14 unit tests | **never**: `first-real-run.md` says it needs the operator's OAuth client, and its scope and model-ID assumptions are marked unverified |
| Gemini | Vertex (`cloud`) | yes (`platform.rs`) | shared envelope tests | **none recorded** |
| OpenAI | Direct API | **no** separate driver; only the compat dialect above | | |
| Claude | Harness (`claude` CLI) | yes (`harness.rs`) | scripted tests; an `#[ignore]` live round trip | **yes**: a full build ran end to end on 2026-10-24 (Builder, tests, audit, approval card, merge) |
| Gemini | Harness (`agy`) | yes | scripted tests; an `#[ignore]` live round trip | **yes** for a simple round trip; a Builder on `agy` is refused by design (no approval bridge) |

Quota exhaustion fixtures exist for both CLIs (`tests/fixtures/quota/`), captured from real runs.

## 2. What "ready for API keys" has to mean

An operator who has only API keys (or Bedrock or Vertex) should be able to run everything Delphi does. The proposed definition, checked in order:

1. **Each API row has one recorded live run** of a tool loop, with the date, the model, the cost, and the provider's own usage figures compared to the ledger.
2. **Pricing is mandatory.** Already true in the code, and worth keeping as a stated criterion: setup does not write a free price unless the operator says so (commit 2e09bc9), the engine refuses to call a model with no catalog price (`engine.rs`), routing skips unpriced models (`route.rs`), and `doctor` flags them (`config.rs`). The earlier roadmap review listed this as open; it is not.
3. **A full mission with no harness available.** Builder, tests, cross-family audit, a Council and the merge card run end to end with `claude` and `agy` removed from `PATH`. This is the single test that proves the claim.
4. **Failure behaviour is exercised live, not just mocked:** a 429 with backoff, an expired credential, cancellation mid-stream, an unreachable endpoint.
5. **No secret leaks:** the key does not appear in events, the usage ledger, logs or the summary of a headless run, checked after the live runs.

## 3. Proposed live conformance suite
`#[ignore]` tests gated on environment variables so CI runs them only where keys are provided; a nightly workflow with a hard spend cap runs them where they are.

| Test | Needs | Cost (guess) |
| :-- | :-- | :-- |
| Claude tool loop, with reasoning signatures preserved over two tool turns | `ANTHROPIC_API_KEY` | cents |
| Claude on Bedrock: one tool loop, one streamed reply | AWS credentials, a model enabled in a region | cents |
| Claude on Vertex: same | a Vertex project | cents |
| Gemini tool loop with an API key; then again with the OAuth client, checking the model-ID and scope assumptions from `first-real-run.md` | a Gemini API key; the OAuth client for the second | cents |
| Compat gateway: one tool loop | any OpenAI-compatible endpoint | cents |
| Usage accounting: provider-reported tokens equal the ledger's | the above | none extra |
| 429 handling | needs a way to provoke it cheaply; may stay mocked, flagged as such | |
| Key hygiene sweep over events, ledger and logs after the runs | the above | none extra |

None of these has been written. They need keys, so they are gated on the operator (decision 4 in the plan).

## 4. Harness contract pinning
The CLI paths are the only live-proven ones, and they depend on output formats Google and Anthropic can change. Needed:
- Recorded stream fixtures per CLI version, replayed in CI, so a changed format fails a test.
- `doctor` warns on an unrecognised CLI version.
- Harness findings from the red-team sweep: all but one were fixed in commit 2e09bc9 (a non-UTF-8 line no longer ends the reader, the line cap applies while reading, a CLI that closes its output but stays alive is killed, unfinished `agy` tool steps are not reported as completed, `agy` with no tools is told so). **Still open:** the system prompt travels in the process arguments (`--append-system-prompt`), where other users on the machine can see it with `ps`. The prompt holds governance text and role instructions, not secrets, so this is low severity; moving it to a file or stdin depends on what each CLI accepts.

## 5. Order of work
1. Write the live suite behind environment gates, so it is ready when keys exist.
2. Record the first live runs when the operator supplies keys and a spend cap, and fill the "Live" column.
3. The `PATH`-scrubbed full mission.
4. Harness contract pinning (§4): recorded stream fixtures per CLI version, and the `doctor` version warning.
5. The system prompt in argv, if the CLIs allow another route.

Steps 1 and 4 need no keys and can proceed now.
