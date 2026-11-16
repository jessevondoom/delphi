"""A loopback meter for spec 114 M7: forwards a local model server's HTTP API and writes one line per model call with the tokens
the server reported, because the research runner meters no tokens (`usage.tokens` is null in a record).

Usage: token_meter.py <listen port> <upstream base, e.g. http://127.0.0.1:11434> <out.jsonl>
Point a scratch home's local endpoint `base_url` at http://127.0.0.1:<listen port>. It listens on loopback only and forwards
to the one upstream it was given; it keeps no request or response text, only the counts.
"""
import http.client, http.server, json, sys, threading, time, urllib.parse

port, upstream, out_path = int(sys.argv[1]), sys.argv[2], sys.argv[3]
up = urllib.parse.urlparse(upstream)
lock = threading.Lock()


def counts(body: bytes):
    """The last usage the body reports: an OpenAI-style `usage` (a stream sends it once, at the end) or Ollama's native counts."""
    p = c = None
    for line in body.splitlines():
        line = line.strip()
        if line.startswith(b"data:"):
            line = line[5:].strip()
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if not isinstance(v, dict):
            continue
        u = v.get("usage")
        if isinstance(u, dict) and "prompt_tokens" in u:
            p, c = u.get("prompt_tokens"), u.get("completion_tokens")
        elif "prompt_eval_count" in v:
            p, c = v.get("prompt_eval_count"), v.get("eval_count")
    return p, c


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def forward(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n) if n else None
        conn = http.client.HTTPConnection(up.hostname, up.port or 80, timeout=900)
        headers = {k: v for k, v in self.headers.items() if k.lower() not in ("host", "connection", "accept-encoding")}
        t0 = time.time()
        conn.request(self.command, self.path, body=body, headers=headers)
        r = conn.getresponse()
        data = r.read()
        self.send_response(r.status)
        for k, v in r.getheaders():
            if k.lower() not in ("transfer-encoding", "connection", "content-length"):
                self.send_header(k, v)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        if self.command == "POST":
            p, c = counts(data)
            rec = {"at": round(t0, 3), "secs": round(time.time() - t0, 2), "path": self.path, "status": r.status, "prompt_tokens": p, "completion_tokens": c}
            with lock, open(out_path, "a") as f:
                f.write(json.dumps(rec) + "\n")

    do_GET = do_POST = forward


http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
