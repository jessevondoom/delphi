"""Capture what the `claude` CLI prints when its quota is (nearly) exhausted (Step 3c M1 fixture).

usage:
  python3 -I dev/capture_quota_sample.py <output dir> [--model sonnet] [--dry-run]
  python3 -I dev/capture_quota_sample.py --self-test

Run it when the Claude limit is close or reached, ideally before topping up. It makes two tiny calls
with the same flags the harness uses (stream-json in and out, no tools), once as the harness does and
once in plain text mode, in an empty scratch directory under <output dir>. A call that is rejected costs
nothing; one that succeeds costs a few cents of subscription quota.

Writes under <output dir>:
  raw/        stdout, stderr and exit code exactly as printed (may hold ids and paths: do not commit)
  claude-quota-sample.md   the same, redacted, as a fixture for crates/delphi-engine/tests/fixtures/quota/

Prints the `rate_limit_event` lines it saw (status, utilization, reset), which is the data M1 needs.
Findings go in missions/2026-10-23-foundational-design/step-3c-m1-m2-exploration.md (section 1.2).
"""
import json, os, pathlib, re, subprocess, sys

PROMPT = "Reply with the single word ok."
FLAGS = ["--verbose", "--no-session-persistence", "--setting-sources", "", "--strict-mcp-config", "--tools", ""]
TIMEOUT = 90


def redact(text):
    """Remove ids, paths, addresses and anything that looks like a credential."""
    home = str(pathlib.Path.home())
    rules = [
        (re.escape(home), "<home>"),
        (r"/Users/[^\s\"']+", "<path>"),
        (r"/home/[^\s\"']+", "<path>"),
        (r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", "<uuid>"),
        (r"\breq_[A-Za-z0-9]+", "<request-id>"),
        (r"\bsk-[A-Za-z0-9_\-]{10,}", "<secret>"),
        (r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}", "<email>"),
        (r"\b[A-Za-z0-9_\-]{40,}\b", "<token>"),
    ]
    for pat, rep in rules:
        text = re.sub(pat, rep, text)
    return text


def run(args, stdin_text, cwd):
    try:
        p = subprocess.run(["claude"] + args, input=stdin_text, capture_output=True, text=True, cwd=cwd, timeout=TIMEOUT)
        return p.stdout, p.stderr, p.returncode
    except subprocess.TimeoutExpired as e:
        return (e.stdout or "", (e.stderr or "") + "\n[timed out after %ds]" % TIMEOUT, -1)


def rate_limit_events(stdout):
    out = []
    for line in stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("type") == "rate_limit_event":
            out.append(m.get("rate_limit_info", m))
    return out


def self_test():
    sample = '{"session_id":"123e4567-e89b-12d3-a456-426614174000","cwd":"%s/x","msg":"a@b.com req_011CSHoEeqs5C35K2UUqR7Fy sk-ant-api03-abcdefghijkl"}' % pathlib.Path.home()
    r = redact(sample)
    assert str(pathlib.Path.home()) not in r and "a@b.com" not in r and "123e4567" not in r, r
    assert "req_011" not in r and "sk-ant" not in r, r
    assert rate_limit_events('{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","utilization":1.0}}\nnot json') == [{"status": "rejected", "utilization": 1.0}]
    print("self-test ok")


def main(argv):
    if "--self-test" in argv:
        return self_test()
    rest = [a for a in argv if a != "--dry-run"]
    dry = "--dry-run" in argv
    model = "sonnet"
    if "--model" in rest:
        i = rest.index("--model")
        model = rest[i + 1]
        del rest[i:i + 2]
    if len(rest) != 1:
        sys.exit(__doc__)
    out = pathlib.Path(rest[0]).resolve()
    work = out / "scratch"
    raw = out / "raw"
    stream_args = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--model", model] + FLAGS
    text_args = ["-p", PROMPT, "--output-format", "text", "--model", model] + FLAGS[1:]  # no --verbose in text mode
    stream_in = json.dumps({"type": "user", "message": {"role": "user", "content": PROMPT}}) + "\n"
    plan = [("stream-json", stream_args, stream_in), ("text", text_args, None)]
    if dry:
        for name, a, _ in plan:
            print(name, "claude " + " ".join(repr(x) if x == "" else x for x in a))
        return
    work.mkdir(parents=True, exist_ok=True)
    raw.mkdir(parents=True, exist_ok=True)
    md = ["# Real `claude` CLI output near or at quota exhaustion (captured with dev/capture_quota_sample.py)\n",
          "Redacted: ids, paths, addresses and token-like strings are replaced. Model: `%s`. Prompt: \"%s\"\n" % (model, PROMPT)]
    for name, a, stdin in plan:
        so, se, rc = run(a, stdin, str(work))
        (raw / ("%s.stdout" % name)).write_text(so)
        (raw / ("%s.stderr" % name)).write_text(se)
        (raw / ("%s.exit" % name)).write_text(str(rc))
        events = rate_limit_events(so)
        print("== %s: exit %s, %d bytes stdout, %d bytes stderr, %d rate_limit_event(s)" % (name, rc, len(so), len(se), len(events)))
        for e in events:
            print("   rate_limit_event:", json.dumps(e))
        md.append("\n## %s mode: exit code %s\n\n### stdout\n\n```text\n%s\n```\n\n### stderr\n\n```text\n%s\n```\n" % (name, rc, redact(so).rstrip(), redact(se).rstrip()))
    md.append("\n## Notes\n\n(fill in: what marks exhaustion, where the reset time is, its unit, and whether the run stopped)\n")
    (out / "claude-quota-sample.md").write_text("".join(md))
    print("\nwrote %s and raw/ (do not commit raw/). Review the redacted file by eye before committing it." % (out / "claude-quota-sample.md"))


if __name__ == "__main__":
    main(sys.argv[1:])
