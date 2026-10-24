# Real exhausted-quota output from `claude` 2.1.310 (captured 2026-10-24)
Redacted: ids, paths, addresses and token-like strings are replaced. Model: `sonnet`. Prompt: "Reply with the single word ok."

## stream-json mode: exit code 1

### stdout

```text
{"type":"system","subtype":"init","cwd":"<home>/delphi-claude-exhaustion/scratch","session_id":"<uuid>","tools":[],"mcp_servers":[],"model":"claude-sonnet-5-5","permissionMode":"default","slash_commands":["design","design-sync","dataviz","update-config","verify","debug","code-review","simplify","batch","fewer-permission-prompts","doctor","loop","schedule","claude-api","run","run-skill-generator","plugin-authoring","advisor","agents","auto-mode-setup","autocompact","clear","color","compact","config","output-style","context","effort","fast","focus","heapdump","init","mcp","import","model","__remote-workflow","workflow-launch-exec","reload-plugins","reload-skills","rename","ultrareview","security-review","usage-credits","extra-usage","usage","insights","recap","skill-doctor","goal","design-consent","design-revoke","list-agents","team-onboarding"],"terminal_slash_commands":["doctor","color","focus","reload-plugins"],"apiKeySource":"none","claude_code_version":"2.1.310","output_style":"default","agents":["claude","Explore","general-purpose","Plan","statusline-setup"],"skills":["design","design-sync","dataviz","update-config","verify","debug","code-review","simplify","batch","fewer-permission-prompts","doctor","loop","schedule","claude-api","run","run-skill-generator","plugin-authoring"],"plugins":[{"name":"cc-plugin-agents-md","path":"builtin","source":"cc-plugin-agents-md@builtin"},{"name":"cc-plugin-telemetry","path":"builtin","source":"cc-plugin-telemetry@builtin"},{"name":"cc-plugin-plugin-authoring","path":"builtin","source":"cc-plugin-plugin-authoring@builtin"}],"capabilities":["interrupt_receipt_v1","interrupt_cancel_queued_v1","interrupt_send_now_v1","interrupt_requeue_taken_v1","msg_lifecycle_v1","request_marker_lists_v1","lifecycle_wakeup_source_v1","sdk_mcp_tools_list_changed","sdk_mcp_manifests","mcp_read_resource_v1","mcp_tool_ui_meta_v1","ui_surface_v1","mcp_call_result_not_for_model_v1"],"analytics_disabled":false,"product_feedback_disabled":false,"uuid":"<uuid>","memory_paths":{"auto":"<home>/.claude/projects/-<token>/memory/"},"original_cwd":"<home>/delphi-claude-exhaustion/scratch","additional_directories":[],"messaging_socket_path":"<socket>","fast_mode_state":"off","fast_mode_disabled_reason":"sdk_opt_in_required","per_turn_effort_active":true,"view_mode":"default"}
{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1792864800,"rateLimitType":"five_hour","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":1,"resetsAt":1792864800,"status":"rejected","surpassedThreshold":1},"seven_day":{"utilization":0.62,"resetsAt":1793365200,"status":"allowed"}}},"uuid":"<uuid>","session_id":"<uuid>"}
{"type":"assistant","message":{"diagnostics":null,"id":"<uuid>","container":null,"model":"<synthetic>","role":"assistant","stop_details":null,"stop_reason":"stop_sequence","stop_sequence":"","type":"message","usage":{"output_tokens_details":null,"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":null,"cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0},"inference_geo":null,"iterations":null,"speed":null,"fallback_credit":null},"content":[{"type":"text","text":"You've hit your session limit · resets 2pm (America/New_York)"}],"context_management":null},"parent_tool_use_id":null,"session_id":"<uuid>","uuid":"<uuid>","timestamp":"2026-10-24T15:11:30.180Z","error":"rate_limit","request_id":"<request-id>","is_api_error_message":true,"api_error_status":429,"api_error":"usage_limit_reached","api_error_params":{"rate_limit_info":{"status":"rejected","resetsAt":1792864800,"rateLimitType":"five_hour","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false}},"api_error_origin":"first_party"}
{"duration_api_ms":0,"stop_reason":"stop_sequence","session_id":"<uuid>","total_cost_usd":0,"usage":{"output_tokens_details":{"thinking_tokens":0},"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0,"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":"standard","cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0},"inference_geo":"","iterations":[],"speed":"standard","fallback_credit":null},"modelUsage":{},"permission_denials":[],"terminal_reason":"api_error","fast_mode_state":"off","fast_mode_disabled_reason":"sdk_opt_in_required","subagent_stats":{"spawned":0,"requested":{"background":0,"foreground":0,"unset":0},"started_in_background":0,"max_depth":0,"spawned_by_subagents":0,"completed":0,"failed":0,"killed":{"parent":0,"user":0,"system":0},"refused":{"depth_limit":0,"concurrency_limit":0,"budget":0},"by_type":{}},"safety_stops":0,"is_error":true,"num_turns":1,"subtype":"success","api_error_status":429,"api_error_origin":"first_party","api_error":"usage_limit_reached","result":"You've hit your session limit · resets 2pm (America/New_York)","type":"result","duration_ms":268,"uuid":"<uuid>","queued_turn_count":0,"result_index":0,"model_usage_since_last_result":{}}
```

### stderr

```text

```

## text mode: exit code 1

### stdout

```text
You've hit your session limit · resets 2pm (America/New_York)
```

### stderr

```text

```

## Notes

- **Exhaustion marker:** exit code 1 in both modes, empty stderr, returned in about 270 ms with zero tokens used. In stream-json mode a `rate_limit_event` with `status: "rejected"` follows `init`, then a synthetic assistant message (`model: "<synthetic>"`, `error: "rate_limit"`, `api_error: "usage_limit_reached"`, `api_error_status: 429`), then a `result` with `is_error: true` and `terminal_reason: "api_error"`.
- **Reset time:** `resetsAt` is **epoch seconds** (1792864800 = 2026-10-24 14:00 EDT), in `rate_limit_info` and per window in `unifiedWindows` (`five_hour` rejected at utilization 1; `seven_day` allowed at 0.62). Text mode gives only a local clock time and zone name ("resets 2pm (America/New_York)"), so prefer the event.
- **Kind:** `rateLimitType: "five_hour"` is a subscription window, not a per-minute throttle. `overageStatus: "rejected"`, `overageDisabledReason: "org_level_disabled"`.
- **Run stops:** yes, there is no further output after `result`.
- Not captured: the `allowed_warning` status, and the `seven_day` window being the one rejected.
