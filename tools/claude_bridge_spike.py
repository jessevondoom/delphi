"""Spike (2026-10-24): drive `claude` through its stream-json approval bridge and log every message.

usage: python3 tools/claude_bridge_spike.py <empty scratch dir OUTSIDE ~/.claude> allow|deny

Spends a few cents of subscription quota per run. Findings: specs/009 §7B.
"""
import json, subprocess, sys, threading, time
work = sys.argv[1]
decision = sys.argv[2]  # allow | deny
cmd = ["claude","-p","--input-format","stream-json","--output-format","stream-json","--verbose",
       "--permission-prompts","host","--permission-mode","manual","--no-session-persistence",
       "--setting-sources","","--strict-mcp-config","--model","sonnet","--max-budget-usd","0.25","--permission-prompt-tool","stdio"]
p = subprocess.Popen(cmd, cwd=work, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
log = open("spike-%s.jsonl" % decision, "w")
def send(o):
    p.stdin.write(json.dumps(o)+"\n"); p.stdin.flush()
send({"type":"user","message":{"role":"user","content":"Use the Bash tool to run exactly: touch created.txt  Then reply with the single word done."}})
def watchdog():
    time.sleep(100)
    if p.poll() is None: p.kill()
threading.Thread(target=watchdog, daemon=True).start()
for line in p.stdout:
    log.write(line); log.flush()
    try: m = json.loads(line)
    except Exception: continue
    t = m.get("type")
    if t == "control_request":
        rid = m.get("request_id"); req = m.get("request", {})
        print("CONTROL_REQUEST", json.dumps(m)[:600])
        if req.get("subtype") == "can_use_tool":
            if decision == "allow":
                resp = {"behavior":"allow","updatedInput":req.get("input",{})}
            else:
                resp = {"behavior":"deny","message":"The operator denied this in the spike."}
            send({"type":"control_response","response":{"subtype":"success","request_id":rid,"response":resp}})
    elif t == "result":
        print("RESULT", json.dumps(m)[:500])
        p.stdin.close()
        break
    else:
        print(t, json.dumps(m)[:200])
try: p.wait(timeout=20)
except Exception: p.kill()
print("STDERR", p.stderr.read()[:500])
