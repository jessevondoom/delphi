#!/usr/bin/env python3
"""No-model check of what one arm shows: speaks MCP to the proxy exactly as a host would, lists the tools and sends one
search_kb call, and writes both answers to <out file>. No model is called.

    python3 -I probe.py <arm> <proxy log dir> <out file> <query> -- <delphi binary> mcp --home <temp home>
"""
import json
import os
import subprocess
import sys


def main():
    arm, log_dir, out, query = sys.argv[1:5]
    assert sys.argv[5] == "--"
    proxy = os.path.join(os.path.dirname(os.path.abspath(__file__)), "proxy.py")
    p = subprocess.Popen([sys.executable, "-I", proxy, arm, log_dir, "--"] + sys.argv[6:], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def call(i, method, params):
        p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": i, "method": method, "params": params}) + "\n")
        p.stdin.flush()
        for line in p.stdout:
            m = json.loads(line)
            if m.get("id") == i:
                return m
        raise SystemExit("probe: no answer")

    call(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "labels-probe", "version": "1"}})
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    tools = call(2, "tools/list", {})
    found = call(3, "tools/call", {"name": "search_kb", "arguments": {"query": query}})
    p.stdin.close()
    p.wait(timeout=30)
    text = "".join(c.get("text", "") for c in found["result"]["content"])
    with open(out, "w") as f:
        f.write("## tools/list\n\n")
        for t in tools["result"]["tools"]:
            f.write(json.dumps({"name": t["name"], "description": t["description"], "properties": sorted(t["inputSchema"].get("properties", {}))}) + "\n")
        f.write("\n## search_kb %s\n\n%s\n" % (json.dumps(query), text))


if __name__ == "__main__":
    main()
