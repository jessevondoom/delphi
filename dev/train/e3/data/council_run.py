import json, re, subprocess, sys, time

D, H, QFILE, OUT = sys.argv[1:5]   # delphi binary, scratch home, question file, output json
CAP = int(sys.argv[5]) if len(sys.argv) > 5 else 350000
text = open(QFILE, encoding="utf-8").read().rstrip("\n")

def parse(t):
    if not t.startswith("Question: "):
        return {"question": t}
    m = re.search(r"\nStakes: (\w+)\nOptions:\n(.*?)\nPointers:\n(.*)\Z", t, re.S)
    q = t[len("Question: "):m.start()]
    opts = [l[2:] for l in m.group(2).split("\n") if l.startswith("- ")]
    ptrs = []
    for l in m.group(3).split("\n"):
        if l.startswith("- "):
            uri, _, reason = l[2:].partition(": ")
            ptrs.append({"uri": uri, "reason": reason})
    a = {"question": q, "stakes": m.group(1)}
    if opts: a["options"] = opts
    if ptrs: a["context_pointers"] = ptrs
    return a

args = parse(text)
args["model"] = "claude-sonnet-5-5"
print("args", json.dumps(args)[:400], flush=True)
P = subprocess.Popen([D, "mcp", "--workspace", "delphi", "--home", H], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
_id = 0

def rpc(method, params=None, notify=False):
    global _id
    msg = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        msg["params"] = params
    if not notify:
        _id += 1
        msg["id"] = _id
    P.stdin.write(json.dumps(msg) + "\n")
    P.stdin.flush()
    if notify:
        return None
    while True:
        line = P.stdout.readline()
        if not line:
            raise SystemExit("mcp closed: " + P.stderr.read()[:500])
        r = json.loads(line)
        if r.get("id") == _id:
            return r

def tool(name, a):
    r = rpc("tools/call", {"name": name, "arguments": a})
    res = r.get("result", {})
    text = res.get("content", [{}])[0].get("text", "")
    if res.get("isError"):
        raise SystemExit(f"{name} failed: {text[:800]}")
    try:
        return json.loads(text)
    except Exception:
        return {"text": text}

rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "e2"}})
rpc("notifications/initialized", notify=True)
started = tool("request_council", args)
print("started", json.dumps(started)[:300], flush=True)
cid = started["council_id"]
t0 = time.time()
last = None
final = None
while time.time() - t0 < 4500:
    st = tool("council_status", {"council_id": cid})
    line = (st.get("state"), st.get("phase"))
    if line != last:
        print(int(time.time() - t0), "s", line, flush=True)
        last = line
    if st.get("state") not in ("deliberating", "convening"):
        final = tool("council_result", {"council_id": cid})
        break
    time.sleep(10)
open(OUT, "w").write(json.dumps({"started": started, "final": final, "elapsed_s": int(time.time() - t0)}, indent=2))
print("done", (final or {}).get("state"), flush=True)
P.terminate()
