# Real exhausted-quota output from `agy` 1.4.7 (captured 2026-10-24)

Captured by running `agy --mode plan --model gemini-4-argon-medium --print-timeout 60s -p "Reply with the single word ok."` in an empty directory, once in text mode and once with `--output-format stream-json`. The model's quota was exhausted. The call returned immediately, with zero tokens used. Conversation and error IDs are replaced with placeholders, the working directory with `<cwd>`, and the 60-entry tool list in `init` is shortened.

Used as the fixture for the Step 3c M1 exhaustion classifier (`missions/2026-10-23-foundational-design/step-3c-m1-m2-exploration.md`).

## Text mode: exit code 3, stdout empty, stderr

```text
error: Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 136h12m57s.
AGY_ERROR: {"short_error":"RESOURCE_EXHAUSTED (code 429): Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 136h12m57s.","status":"RESOURCE_EXHAUSTED","error_code":429,"code_kind":"http","retryable":true,"error_id":"<error-id>"}
```

## Stream-json mode: exit code 3, stdout (one JSON object per line), stderr as above

```json
{"event":"init","conversation_id":"<conversation-id>","init":{"model":"gemini-4-argon-medium","cwd":"<cwd>","tools":["ask_permission","run_command","view_file","..."],"permission_mode":"proceed-in-sandbox","expanded_commands":[{"name":"plan","type":"system"}]}}
{"event":"step_update","step_update":{"conversation_id":"<conversation-id>","step_index":0,"state":"DONE","step_type":"user_input"}}
{"event":"step_update","step_update":{"conversation_id":"<conversation-id>","step_index":1,"state":"DONE","step_type":"error_message"}}
{"event":"result","result":{"conversation_id":"<conversation-id>","status":"ERROR","response":"","error":"Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 136h12m54s.","duration_seconds":0,"num_turns":1,"usage":{"input_tokens":0,"output_tokens":0,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":0}}}
```

## What the sample shows

- The exhaustion is explicit: `status` `RESOURCE_EXHAUSTED`, `error_code` 429, text "Individual quota reached", and a reset duration in Go duration format (`136h12m57s`, about 5.7 days).
- `"retryable": true` is **wrong for our purposes**: a 5.7-day wait is not worth retrying. The classifier must not trust this flag.
- On stdout the reset time and message are only in `result.error`. `result.response` is empty, and the harness today reads `response` and discards stderr, so Delphi currently reports "the CLI reported an error: " with no reason.
- Exit code 3, zero usage.
