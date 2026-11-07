import json, sys
# usage: iso.py <stream.jsonl>
f = sys.argv[1]
agents = 0
agent_ids = []
tools = {}
results = []
init = None
sessions = set()
asked = False
texts = []
for line in open(f):
    try:
        e = json.loads(line)
    except Exception:
        continue
    if e.get("session_id"):
        sessions.add(e["session_id"])
    if e.get("type") == "system" and e.get("subtype") == "init" and init is None:
        init = e
    if e.get("type") == "assistant":
        for c in e.get("message", {}).get("content", []):
            if c.get("type") == "tool_use":
                n = c["name"]
                tools[n] = tools.get(n, 0) + 1
                if n in ("Agent", "Task"):
                    agents += 1
                    agent_ids.append((c["id"], e.get("parent_tool_use_id"), c["input"].get("subagent_type"), c["input"].get("run_in_background"), e.get("message", {}).get("id")))
            if c.get("type") == "text" and not e.get("parent_tool_use_id"):
                texts.append(c["text"])
    if e.get("type") == "result":
        results.append(e)
if init:
    print("init: plugins", init.get("plugins"), "mcp", init.get("mcp_servers"), "model", init.get("model"))
    print("tools mcp council:", [t for t in init.get("tools", []) if "council" in t])
print("sessions", sessions)
print("tool counts", tools)
print("agent calls", agents)
for a in agent_ids:
    print("  ", a)
tot_in = tot_out = 0
for r in results:
    mu = r.get("modelUsage", {})
    i = sum(v.get("inputTokens", 0) for v in mu.values())
    o = sum(v.get("outputTokens", 0) for v in mu.values())
    print("result: is_error", r.get("is_error"), "subtype", r.get("subtype"), "turns", r.get("num_turns"), "dur_ms", r.get("duration_ms"), "in", i, "out", o, "models", {k: (v.get("inputTokens"), v.get("outputTokens")) for k, v in mu.items()})
    tot_in += i
    tot_out += o
print("HOST TOKENS (last result, cumulative modelUsage input+output)", (lambda r: sum(v.get("inputTokens",0)+v.get("outputTokens",0) for v in r.get("modelUsage",{}).values()))(results[-1]) if results else 0)
if results:
    print("final result text:\n", (results[-1].get("result") or "")[:6000])
