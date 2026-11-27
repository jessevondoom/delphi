---
state: built
date: 2026-11-27
eval_waived: this slice changes no retrieval path; the retrieval files the finisher names differ from origin/main because the branch is based on the knowledge-v2 integration branch
title: "host-first descriptions: `request_help`, its `role` parameter, `get_agent` and the host instructions say a role runs in the host (§2 item 1); the `request_help` reply carries `tokens_used`"
---

Drift fix against §2 item 1 ("Host first"). The `request_help` description told a host to pass a role the operator named as `role` so that role answered itself; a host followed it and ran the architect inside the daemon for design consultations, a separate session on the operator's tokens that re-read whole specs. Now the `request_help` description and its `role` parameter lead with the rule (adopt a role in the host with `get_agent`; `request_help` is for work Delphi itself must run: an inside exception of §3, another model family, a view not anchored on the host's session, or the operator asking for Delphi's own run), `get_agent` says the same, and the plugin skill and the instruction file (`DELPHI.md` and its kin) carry one line with the reason. An answered `request_help` reply now carries `tokens_used` (the task's `agent.usage` records summed: total, input, cache read, cache write, output, thinking). A test fails when the descriptions and the instruction text disagree. `request_help` still accepts a named role; routing and authority are unchanged. NOT run live with a host.
