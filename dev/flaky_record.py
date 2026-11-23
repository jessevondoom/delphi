#!/usr/bin/env python3
"""The flaky record across CI runs (spec 125, slice R6; amendment 125-A1 rule 3).

Each CI test job keeps the JUnit file of `cargo nextest run --profile ci` as an artifact (`junit-*`). One run's summary
lists its own flaky tests; this script reads the JUnit files of many runs and says which tests are flaky again and
again. Usage:

    python3 -I dev/flaky_record.py [--runs N] [--dir DIR]   # download the last N `ci` runs' JUnit files, then report
    python3 -I dev/flaky_record.py report DIR               # report on files already downloaded, no network
    python3 -I dev/flaky_record.py self-test                # check the parser and the rule on built-in samples

Downloading uses the GitHub CLI (`gh run list`, `gh run download`) with the caller's own login, read only. Runs on
`main` and on pull requests are both read. A run already downloaded under DIR is not fetched again; a run with no JUnit
artifact (cancelled, or the build failed before the tests) is skipped and counted.

The table names every test that was flaky (failed, then passed on its one retry) or failed (also on the retry) in at
least one run: how many runs it was flaky in, how many it failed in, in how many runs it ran at all, and the first and
last run where it was flaky or failed. A test flaky in 2 or more distinct runs is listed under "needs an owner"
(125-A1 rule 3). This script files nothing: the operator or a builder opens the issue, naming the runs it prints.
"""

import json
import os
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

OWNER_THRESHOLD = 2  # 125-A1 rule 3: flaky in two runs gets an issue and an owner
META = "run.json"


def gh(args):
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def fetch(dest, runs):
    """Download the JUnit artifacts of the last `runs` runs of the `ci` workflow into dest/<run id>/."""
    os.makedirs(dest, exist_ok=True)
    listed = json.loads(gh(["run", "list", "--workflow", "ci", "--limit", str(runs), "--json", "databaseId,headBranch,event,createdAt,conclusion,status"]))
    skipped = 0
    for run in listed:
        if run.get("status") != "completed":
            continue
        rid = str(run["databaseId"])
        where = os.path.join(dest, rid)
        if os.path.exists(os.path.join(where, META)):
            continue
        os.makedirs(where, exist_ok=True)
        got = subprocess.run(["gh", "run", "download", rid, "--pattern", "junit-*", "--dir", where], capture_output=True, text=True)
        if got.returncode != 0:
            skipped += 1
        meta = {"id": rid, "branch": run.get("headBranch", ""), "event": run.get("event", ""), "created": run.get("createdAt", ""), "conclusion": run.get("conclusion", ""), "junit": got.returncode == 0}
        with open(os.path.join(where, META), "w", encoding="utf-8") as f:
            json.dump(meta, f)
    return skipped


def cases(path):
    """(test name, outcome) for every test case of one JUnit file: outcome is 'flaky', 'failed' or 'passed'."""
    root = ET.parse(path).getroot()
    for case in root.iter("testcase"):
        name = f"{case.get('classname', '')} {case.get('name', '')}".strip()
        if case.find("failure") is not None or case.find("error") is not None:
            yield name, "failed"
        elif case.findall("flakyFailure") or case.findall("flakyError"):
            yield name, "flaky"
        elif case.find("skipped") is None:
            yield name, "passed"


def load(dest):
    """Every run under dest: (meta, {test: worst outcome in that run}). A run with several jobs counts each test once."""
    rank = {"passed": 0, "flaky": 1, "failed": 2}
    out = []
    for rid in sorted(os.listdir(dest)):
        where = os.path.join(dest, rid)
        if not os.path.isdir(where):
            continue
        meta = {"id": rid, "branch": "", "created": ""}
        if os.path.exists(os.path.join(where, META)):
            with open(os.path.join(where, META), encoding="utf-8") as f:
                meta.update(json.load(f))
        seen = {}
        for base, _, files in os.walk(where):
            for fn in files:
                if fn.endswith(".xml"):
                    for name, outcome in cases(os.path.join(base, fn)):
                        if rank[outcome] >= rank.get(seen.get(name), -1):
                            seen[name] = outcome
        if seen:
            out.append((meta, seen))
    return out


def record(runs):
    """Per test: flaky runs, failed runs, runs seen, and the ids of the runs where it was flaky or failed (oldest first)."""
    rows = {}
    for meta, seen in sorted(runs, key=lambda r: (r[0].get("created", ""), r[0]["id"])):
        for name, outcome in seen.items():
            row = rows.setdefault(name, {"flaky": 0, "failed": 0, "seen": 0, "runs": []})
            row["seen"] += 1
            if outcome != "passed":
                row[outcome] += 1
                row["runs"].append((meta["id"], meta.get("created", "")[:10], meta.get("branch", ""), outcome))
    return {n: r for n, r in rows.items() if r["runs"]}


def report(dest, out=sys.stdout):
    runs = load(dest)
    rows = record(runs)
    out.write(f"{len(runs)} runs with JUnit files read from {dest}.\n\n")
    if not rows:
        out.write("No test was flaky or failed in these runs.\n")
        return 0
    order = sorted(rows.items(), key=lambda kv: (-kv[1]["flaky"], -kv[1]["failed"], kv[0]))
    out.write("| test | flaky | failed twice | runs seen | first seen | last seen |\n| :-- | --: | --: | --: | :-- | :-- |\n")
    for name, r in order:
        first, last = r["runs"][0], r["runs"][-1]
        out.write(f"| `{name}` | {r['flaky']} | {r['failed']} | {r['seen']} | {first[1]} run {first[0]} | {last[1]} run {last[0]} |\n")
    owners = [(n, r) for n, r in order if r["flaky"] >= OWNER_THRESHOLD]
    out.write(f"\n**Needs an owner** (flaky in {OWNER_THRESHOLD} or more runs, spec 125-A1 rule 3; open an issue naming these runs):\n\n")
    if not owners:
        out.write("None.\n")
    for name, r in owners:
        flaky_runs = ", ".join(f"{rid} ({day}, {branch})" for rid, day, branch, o in r["runs"] if o == "flaky")
        out.write(f"- `{name}`: flaky in {r['flaky']} runs: {flaky_runs}\n")
    return 0


SAMPLE = """<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="4" failures="1" errors="0">
  <testsuite name="delphi-engine" tests="4" disabled="0" errors="0" failures="1">
    <testcase name="a::steady" classname="delphi-engine" timestamp="t" time="0.1"></testcase>
    <testcase name="a::wobbly" classname="delphi-engine" timestamp="t" time="0.1">
      <flakyFailure type="test failure" message="{msg}"><system-err>boom</system-err></flakyFailure>
    </testcase>
    <testcase name="a::broken" classname="delphi-engine" timestamp="t" time="0.1">
      {broken}
    </testcase>
    <testcase name="a::off" classname="delphi-engine" timestamp="t" time="0"><skipped/></testcase>
  </testsuite>
</testsuites>
"""


def self_test():
    with tempfile.TemporaryDirectory() as d:
        def run(rid, created, wobbly, broken, jobs=1):
            where = os.path.join(d, rid)
            for j in range(jobs):
                os.makedirs(os.path.join(where, f"junit-{j}"))
                xml = SAMPLE.format(msg="first" if wobbly else "", broken='<failure type="test failure">x</failure>' if broken and j == 0 else "")
                if not wobbly:
                    xml = xml.replace('<flakyFailure type="test failure" message=""><system-err>boom</system-err></flakyFailure>', "")
                with open(os.path.join(where, f"junit-{j}", "junit.xml"), "w", encoding="utf-8") as f:
                    f.write(xml)
            with open(os.path.join(where, META), "w", encoding="utf-8") as f:
                json.dump({"id": rid, "branch": "main", "created": created}, f)

        run("300", "2026-11-03T00:00:00Z", wobbly=True, broken=False, jobs=2)  # flaky in two jobs of ONE run: counts once
        run("100", "2026-11-01T00:00:00Z", wobbly=True, broken=True)
        run("200", "2026-11-02T00:00:00Z", wobbly=False, broken=False)
        os.makedirs(os.path.join(d, "400"))  # a run with no JUnit artifact is not counted
        rows = record(load(d))
        assert set(rows) == {"delphi-engine a::wobbly", "delphi-engine a::broken"}, rows
        w, b = rows["delphi-engine a::wobbly"], rows["delphi-engine a::broken"]
        assert (w["flaky"], w["failed"], w["seen"]) == (2, 0, 3), w
        assert [r[0] for r in w["runs"]] == ["100", "300"], "oldest first, by date, not by id"
        assert (b["flaky"], b["failed"], b["seen"]) == (0, 1, 3), b
        import io
        buf = io.StringIO()
        report(d, buf)
        text = buf.getvalue()
        assert "3 runs with JUnit files" in text, text
        owners = text.split("**Needs an owner**")[1]
        assert "a::wobbly" in owners and "a::broken" not in owners, owners
        assert "100 (2026-11-01, main), 300 (2026-11-03, main)" in owners, owners
    print("flaky_record self-test: ok")
    return 0


def main(argv):
    args = argv[1:]
    if args[:1] == ["self-test"]:
        return self_test()
    if args[:1] == ["report"]:
        if len(args) != 2:
            print(__doc__, file=sys.stderr)
            return 2
        return report(args[1])
    runs, dest = 30, None
    while args:
        flag = args.pop(0)
        if flag == "--runs" and args:
            runs = int(args.pop(0))
        elif flag == "--dir" and args:
            dest = args.pop(0)
        else:
            print(__doc__, file=sys.stderr)
            return 2
    dest = dest or os.path.join(tempfile.gettempdir(), "delphi-flaky-record")
    skipped = fetch(dest, runs)
    if skipped:
        print(f"{skipped} run(s) had no JUnit artifact (cancelled, or no test ran) and are left out.\n")
    return report(dest)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
