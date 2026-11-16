"""Spec 114 M7 driver: ask Delphi's research runner each fixed question through `delphi mcp` (stdio), wait, save the result.

Usage: eval_drive.py <delphi binary> <home> <cases.json> <out dir> [case ids...]
Only the keys the binary's request_research schema lists are sent (the code before M2 has no `new` and no `runner`).
"""
import json, os, subprocess, sys, time

binary, home, cases_path, out_dir = sys.argv[1:5]
only = set(sys.argv[5:])
os.makedirs(out_dir, exist_ok=True)
cases = json.load(open(cases_path))["cases"]

proc = subprocess.Popen([binary, "mcp", "--home", home], cwd=os.path.dirname(os.path.abspath(out_dir)), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(os.path.join(out_dir, "mcp.stderr"), "a"), text=True, bufsize=1)
next_id = [0]


def rpc(method, params=None, notify=False):
    msg = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        msg["params"] = params
    if not notify:
        next_id[0] += 1
        msg["id"] = next_id[0]
    proc.stdin.write(json.dumps(msg) + "\n")
    proc.stdin.flush()
    if notify:
        return None
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit("mcp server closed")
        r = json.loads(line)
        if r.get("id") == msg["id"]:
            return r


def call(name, args):
    r = rpc("tools/call", {"name": name, "arguments": args})
    if "error" in r:
        return {"_rpc_error": r["error"]}
    res = r["result"]
    text = "".join(c.get("text", "") for c in res.get("content", []) if c.get("type") == "text")
    try:
        body = json.loads(text)
    except ValueError:
        body = {"_text": text}
    if res.get("isError"):
        body["_is_error"] = True
    return body


rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "m7-eval", "version": "0"}})
rpc("notifications/initialized", notify=True)
tools = rpc("tools/list", {})["result"]["tools"]
props = next(t for t in tools if t["name"] == "request_research")["inputSchema"]["properties"]

for case in cases:
    if only and case["id"] not in only:
        continue
    args = {"question": case["question"], "depth": case.get("depth", "survey")}
    if "new" in props:
        args["new"] = True
    if "runner" in props:
        args["runner"] = "daemon"
    t0 = time.time()
    asked = call("request_research", args)
    rid = asked.get("research_id")
    rec = {"case": case, "asked_with": args, "request_reply": asked, "polls": 0, "started_at": round(t0, 3)}
    result = None
    while rid and time.time() - t0 < 3700:
        result = call("research_result", {"research_id": rid, "wait": True})
        rec["polls"] += 1
        state = result.get("state") or result.get("status")
        if state in ("done", "partial", "failed", "cancelled"):
            break
    rec["result"] = result
    rec["ended_at"] = round(time.time(), 3)
    rec["wall_secs"] = round(time.time() - t0, 1)
    with open(os.path.join(out_dir, case["id"] + ".json"), "w") as f:
        json.dump(rec, f, indent=1)
    state = (result or {}).get("state") or (result or {}).get("status")
    print(case["id"], rid, state, rec["wall_secs"], flush=True)

proc.stdin.close()
proc.wait(timeout=30)
