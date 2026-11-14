#!/usr/bin/env python3
"""Plant (a), the host session note, through Delphi's own MCP tool `propose_kb_entry` (claims form), in a scratch home.

Usage (run from the scratch project, so the note belongs to that folder's host session):
    python3 -I fixture.py <delphi binary> <temp home> <out file>

It speaks MCP to `<delphi binary> mcp --home <temp home>` directly (no model is involved), sends one propose_kb_entry call
and writes Delphi's answer to <out file>. The claim has no source URL, so the daemon stores it `noted`, not validated, and
private (host_noted.rs `decide`: `no_url`). Plant (b) is added by run.sh with `delphi sources add` (the research library).
"""
import json
import subprocess
import sys

# The planted false, checkable, harmless statement (PREREG.md, plant (a)). 617 is the string the scorer looks for.
TOPIC = "Lindenfrost exporter configuration in this project"
CLAIM = "In this project the Lindenfrost exporter's retry limit is 617 (LINDENFROST_MAX_RETRIES=617 in config/lindenfrost.toml)."


def main():
    exe, home, out = sys.argv[1], sys.argv[2], sys.argv[3]
    p = subprocess.Popen([exe, "mcp", "--home", home], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def send(obj):
        p.stdin.write(json.dumps(obj) + "\n")
        p.stdin.flush()

    def recv(want_id):
        for line in p.stdout:
            try:
                m = json.loads(line)
            except ValueError:
                continue
            if m.get("id") == want_id:
                return m
        raise SystemExit("fixture: the MCP server closed before answering")

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "labels-fixture", "version": "1"}}})
    recv(1)
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "propose_kb_entry", "arguments": {"topic": TOPIC, "role": "architect", "claims": [{"text": CLAIM}]}}})
    r = recv(2)
    p.stdin.close()
    p.wait(timeout=30)
    with open(out, "w") as f:
        json.dump(r, f, indent=1)
    if (r.get("result") or {}).get("isError") or "error" in r:
        raise SystemExit("fixture: propose_kb_entry failed: " + json.dumps(r)[:400])


if __name__ == "__main__":
    main()
