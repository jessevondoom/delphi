# The smoke test of the built app (spec 111 §6, "Together"): start the real app against a stand-in daemon and check that the
# window itself asked for its data. It passes only if the app found the daemon through `delphi daemon where`, attached on the
# narrator scope twice, subscribed to events, and the page inside the window called `observe.state` and `observe.snapshot`.
# Nothing of the operator's is touched: the stand-in `delphi`, its socket and the Delphi home are in a temporary folder.
# Run from app/ after `npm run app`: `python3 -I dev/smoke.py [path to the app's executable]`. It opens a window for a few seconds.
import json
import os
import socketserver
import subprocess
import sys
import tempfile
import threading
import time

WANTED = {"auth.issue": 2, "hello": 2, "events.subscribe": 1, "observe.state": 1, "observe.snapshot": 1}
seen = []
lock = threading.Lock()


class Daemon(socketserver.StreamRequestHandler):
    def handle(self):
        for raw in self.rfile:
            try:
                req = json.loads(raw)
            except ValueError:
                return
            with lock:
                seen.append(req)
            method, params = req.get("method"), req.get("params") or {}
            if method == "auth.issue":
                result = {"token": "smoke", "scope": params.get("scope")}
            elif method == "hello":
                result = {"protocol_version": 1, "binary_version": "smoke", "scope": "narrator", "pid": os.getpid()}
            elif method == "events.subscribe":
                result = {"subscribed": True}
            elif method == "observe.state":
                result = {"mission_id": "smoke", "agents": {}, "nodes": {}, "tasks": {}, "councils": [], "changes": []}
            elif method == "observe.snapshot":
                result = {"mission_id": "smoke", "events": []}
            else:
                result = {}
            self.wfile.write((json.dumps({"jsonrpc": "2.0", "id": req.get("id"), "result": result}) + "\n").encode())
            self.wfile.flush()


class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


def counts():
    with lock:
        return {m: sum(1 for r in seen if r.get("method") == m) for m in WANTED}


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    exe = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "..", "target", "app", "release", "delphi-app")
    if not os.path.isfile(exe):
        print(f"smoke: no built app at {exe}; run `npm run app` first")
        return 2
    tmp = tempfile.mkdtemp(prefix="dsmoke", dir="/tmp")
    sock = os.path.join(tmp, "n.sock")
    server = Server(sock, Daemon)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    where = {"narrator_socket": sock, "workspace": tmp, "workspace_name": "smoke", "version": "smoke", "home": tmp, "roles": {}}
    fake = os.path.join(tmp, "delphi")
    with open(os.path.join(tmp, "where.json"), "w") as f:
        json.dump(where, f)
    with open(fake, "w") as f:
        f.write(f'#!/bin/sh\ncat "{tmp}/where.json"\n')
    os.chmod(fake, 0o755)
    env = dict(os.environ, DELPHI_EXE=fake, DELPHI_HOME=tmp)
    app = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ok = False
    try:
        deadline = time.time() + 30
        while time.time() < deadline and app.poll() is None:
            got = counts()
            if all(got[m] >= n for m, n in WANTED.items()):
                ok = True
                break
            time.sleep(0.2)
    finally:
        app.terminate()
        try:
            app.wait(timeout=5)
        except subprocess.TimeoutExpired:
            app.kill()
        server.shutdown()
    got = counts()
    with lock:
        scopes = sorted({(r.get("params") or {}).get("scope") for r in seen if r.get("method") == "auth.issue"})
        others = sorted({r.get("method") for r in seen} - set(WANTED) - {"session.snapshot", "kb.get", "session.ping"})
    print("smoke: the stand-in daemon saw " + ", ".join(f"{m} x{got[m]}" for m in WANTED))
    if scopes != ["narrator"]:
        print(f"smoke: FAILED, a scope other than narrator was asked for: {scopes}")
        return 1
    if others:
        print(f"smoke: FAILED, the app sent a method it is not allowed: {others}")
        return 1
    if not ok:
        print("smoke: FAILED, the app did not attach and ask for its data within 30 seconds")
        return 1
    print("smoke: ok, the built app attached read-only and its window asked for the state")
    return 0


if __name__ == "__main__":
    sys.exit(main())
