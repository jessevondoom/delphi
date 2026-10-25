"""Benchmark runner for the evaluation plan (missions/.../evaluation-plan-2026-10-25.md §7).

usage:
  python3 -I dev/bench/bench.py run --tasks <dir or file> --arms <dir or file> --out <results.jsonl>
                                    [--seeds 3] [--order-seed 1] [--only-task ID] [--only-arm NAME] [--keep] [--dry-run]
  python3 -I dev/bench/bench.py self-test

One row is written per (task, arm, seed). Arms for one task run in a random order (seeded), each in a fresh
clone of the task's repository at its base commit, so no arm sees another's work. The hidden tests are applied
only after the arm has finished, on the tree it left, and are never visible to an arm.

A task (JSON):
  {"id": "...", "repo": "<path or url>", "base": "<commit>", "prompt": "<the issue text>",
   "hidden_tests": {"patch": "<file, relative to the task file>", "command": ["cargo", "test", "-p", "x"], "timeout": 900},
   "timeout": 1800}

An arm (JSON), either a command or Delphi:
  {"name": "a0-claude-p", "kind": "command", "command": ["claude", "-p", "{prompt}", "--output-format", "json"], "parse": "claude-json"}
  {"name": "a3-delphi", "kind": "delphi", "template_home": "<a home with a ratified workspace>", "workspace": "bench",
   "input": "/build {prompt}", "flags": ["--permissions", "accept-edits", "--budget", "5"], "delphi": "delphi"}
Placeholders in a command: {prompt} {workdir}. `parse` turns the arm's stdout into cost and outcome:
"none", "claude-json" (the `claude -p --output-format json` result) or "delphi-summary" (the last `run.summary` line).

Not done here, and why it matters: the network is not restricted (run it where egress is limited to the model
endpoints), and a Delphi arm that needs a ratified spec for a large change will stop at gate 3 (the spec step has
no operator proxy yet). Both are listed in the evaluation plan as open decisions.
"""
import argparse, json, os, pathlib, random, re, shutil, subprocess, sys, tempfile, time

DEFAULT_TIMEOUT = 1800


def sh(cmd, cwd=None, timeout=None, env=None):
    """Run a command; returns (exit code, stdout, stderr, seconds, timed_out). Never raises on a failing command."""
    t0 = time.time()
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout, env=env)
        return p.returncode, p.stdout, p.stderr, time.time() - t0, False
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
        err = e.stderr.decode() if isinstance(e.stderr, bytes) else (e.stderr or "")
        return 124, out, err, time.time() - t0, True
    except FileNotFoundError as e:
        return 127, "", str(e), time.time() - t0, False


def git(*args, cwd=None):
    code, out, err, _, _ = sh(["git", *args], cwd=cwd, timeout=300)
    if code != 0:
        raise RuntimeError("git %s failed: %s" % (" ".join(args), err.strip()))
    return out.strip()


def load_all(path):
    p = pathlib.Path(path)
    files = sorted(p.glob("*.json")) if p.is_dir() else [p]
    out = []
    for f in files:
        d = json.loads(f.read_text())
        d["_dir"] = str(f.parent)
        out.append(d)
    return out


def expand(cmd, **kw):
    return [re.sub(r"\{(\w+)\}", lambda m: str(kw.get(m.group(1), m.group(0))), part) for part in cmd]


def clone_at(task, dest):
    git("clone", "-q", "--no-hardlinks", task["repo"], str(dest))
    git("-c", "advice.detachedHead=false", "checkout", "-q", task["base"], cwd=dest)
    # An arm's commits must not look like the project's own history.
    git("config", "user.name", "bench", cwd=dest)
    git("config", "user.email", "bench@example.invalid", cwd=dest)


# ── reading what an arm printed ─────────────────────────────────────────────

def parse_output(kind, stdout):
    """(cost_usd, outcome, extra) from an arm's stdout; None where the arm does not say."""
    if kind == "claude-json":
        for line in reversed(stdout.strip().splitlines()):
            try:
                d = json.loads(line)
            except ValueError:
                continue
            if isinstance(d, dict) and ("total_cost_usd" in d or "result" in d):
                return d.get("total_cost_usd"), "error" if d.get("is_error") else "completed", {"turns": d.get("num_turns")}
        return None, None, {}
    if kind == "delphi-summary":
        for line in reversed(stdout.strip().splitlines()):
            if '"run.summary"' not in line:
                continue
            try:
                d = json.loads(line)
            except ValueError:
                continue
            rows = (d.get("usage") or {}).get("rows") or []
            cost = sum(r.get("cost_usd") or 0 for r in rows) if rows else None
            ready = d.get("ready_to_merge") or {}
            extra = {"cards": d.get("cards"), "ready_sha": ready.get("sha"), "ready_branch": ready.get("branch"),
                     "merge_commit": d.get("merge_commit"), "decisions_awaiting_review": (d.get("decisions_awaiting_review") or {}).get("count")}
            return cost, d.get("outcome"), extra
        return None, None, {}
    return None, None, {}


# ── one arm on one task ─────────────────────────────────────────────────────

def delphi_command(arm, task, workdir, home):
    flags = [f.replace("{policy}", str(pathlib.Path(arm["_dir"]) / "policy.toml")) for f in arm.get("flags", [])]
    text = arm.get("input", "/build {prompt}").replace("{prompt}", task["prompt"])
    return [arm.get("delphi", "delphi"), "run", text, "--json", "--workspace", arm.get("workspace", "bench"), "--home", str(home),
            "--timeout", "%ds" % int(arm.get("timeout", task.get("timeout", DEFAULT_TIMEOUT))), *flags]


def prepare_delphi_home(arm, task, workdir, scratch):
    """A copy of the pre-ratified template home, pointed at this task's checkout. Only `[workspace] projects` is
    edited: it is not one of the pinned blocks, so the ratification still holds."""
    home = pathlib.Path(scratch) / "home"
    shutil.copytree(arm["template_home"], home, symlinks=True)
    cfg = home / "workspaces" / arm.get("workspace", "bench") / "config.toml"
    text = cfg.read_text()
    new, n = re.subn(r"(?m)^projects\s*=\s*\[[^\]]*\]", 'projects = ["%s"]' % str(workdir).replace("\\", "\\\\").replace('"', '\\"'), text)
    if n != 1:
        raise RuntimeError("the template workspace config must have exactly one `projects = [...]` line (found %d)" % n)
    cfg.write_text(new)
    return home


def run_one(task, arm, seed, scratch_root, keep=False):
    scratch = pathlib.Path(tempfile.mkdtemp(prefix="bench-", dir=scratch_root))
    workdir = scratch / "work"
    row = {"task": task["id"], "arm": arm["name"], "seed": seed, "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    try:
        clone_at(task, workdir)
        timeout = int(arm.get("timeout", task.get("timeout", DEFAULT_TIMEOUT)))
        if arm.get("kind") == "delphi":
            home = prepare_delphi_home(arm, task, workdir, scratch)
            cmd, cwd = delphi_command(arm, task, workdir, home), workdir
            parse = "delphi-summary"
        else:
            cmd, cwd = expand(arm["command"], prompt=task["prompt"], workdir=str(workdir)), workdir
            parse = arm.get("parse", "none")
        code, out, err, secs, timed_out = sh(cmd, cwd=cwd, timeout=timeout + 60)
        cost, outcome, extra = parse_output(parse, out)
        row.update({"exit_code": code, "wall_secs": round(secs, 1), "timed_out": timed_out, "cost_usd": cost, "arm_outcome": outcome, **extra})
        (scratch / "arm.stdout").write_text(out)
        (scratch / "arm.stderr").write_text(err)
        # What the arm produced: a Delphi build stops on its own branch, so test that commit; otherwise the tree as left.
        sha = extra.get("merge_commit") or extra.get("ready_sha")
        if sha:
            git("-c", "advice.detachedHead=false", "checkout", "-q", sha, cwd=workdir)
        row["hidden_pass"], row["hidden_tail"] = hidden_tests(task, workdir)
    except Exception as e:  # a harness problem is a row, not a crash: the run goes on
        row.update({"error": str(e)[:500], "hidden_pass": None})
    finally:
        row["scratch"] = str(scratch) if keep else None
        if not keep:
            shutil.rmtree(scratch, ignore_errors=True)
    return row


def hidden_tests(task, workdir):
    """Apply the withheld tests and run them: (pass, last lines of output). None when the task has none."""
    h = task.get("hidden_tests")
    if not h:
        return None, ""
    patch = pathlib.Path(task["_dir"]) / h["patch"]
    code, _, err, _, _ = sh(["git", "apply", "--whitespace=nowarn", str(patch)], cwd=workdir, timeout=120)
    if code != 0:
        return False, "the hidden tests did not apply to what the arm left: " + err.strip()[-300:]
    code, out, err, _, timed_out = sh(h["command"], cwd=workdir, timeout=int(h.get("timeout", 900)))
    tail = "\n".join((out + err).strip().splitlines()[-8:])
    return (code == 0 and not timed_out), tail


# ── the whole run ───────────────────────────────────────────────────────────

def plan(tasks, arms, seeds, order_seed, only_task=None, only_arm=None):
    rng = random.Random(order_seed)
    jobs = []
    for t in tasks:
        if only_task and t["id"] != only_task:
            continue
        for s in range(1, seeds + 1):
            chosen = [a for a in arms if not only_arm or a["name"] == only_arm]
            rng.shuffle(chosen)
            jobs.extend((t, a, s) for a in chosen)
    return jobs


def cmd_run(args):
    tasks, arms = load_all(args.tasks), load_all(args.arms)
    jobs = plan(tasks, arms, args.seeds, args.order_seed, args.only_task, args.only_arm)
    if args.dry_run:
        for t, a, s in jobs:
            print("%s  %-24s seed %d" % (t["id"], a["name"], s))
        print("%d run(s)" % len(jobs))
        return 0
    out = pathlib.Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    scratch_root = tempfile.mkdtemp(prefix="bench-root-")
    for i, (t, a, s) in enumerate(jobs, 1):
        print("[%d/%d] %s × %s (seed %d)" % (i, len(jobs), t["id"], a["name"], s), flush=True)
        row = run_one(t, a, s, scratch_root, keep=args.keep)
        with out.open("a") as f:
            f.write(json.dumps(row, sort_keys=True) + "\n")
        print("    hidden tests: %s · exit %s · %ss · cost %s" % (row.get("hidden_pass"), row.get("exit_code"), row.get("wall_secs"), row.get("cost_usd")), flush=True)
    shutil.rmtree(scratch_root, ignore_errors=True)
    return 0


# ── self test: fake arms against a real repository ──────────────────────────

def self_test():
    root = pathlib.Path(tempfile.mkdtemp(prefix="bench-selftest-"))
    try:
        repo = root / "repo"
        repo.mkdir()
        git("init", "-q", cwd=repo)
        git("config", "user.name", "t", cwd=repo)
        git("config", "user.email", "t@example.invalid", cwd=repo)
        (repo / "calc.py").write_text("def add(a, b):\n    return a - b\n")
        git("add", ".", cwd=repo)
        git("commit", "-qm", "base", cwd=repo)
        base = git("rev-parse", "HEAD", cwd=repo)
        # The hidden test lives outside the repository until grading.
        tdir = root / "tasks"
        tdir.mkdir()
        (tdir / "hidden.patch").write_text(
            "diff --git a/test_calc.py b/test_calc.py\nnew file mode 100644\n--- /dev/null\n+++ b/test_calc.py\n@@ -0,0 +1,4 @@\n"
            "+from calc import add\n+\n+def test_add():\n+    assert add(2, 3) == 5\n")
        task = {"id": "fix-add", "repo": str(repo), "base": base, "prompt": "add() subtracts. Fix it.",
                "hidden_tests": {"patch": "hidden.patch", "command": [sys.executable, "-I", "-c", "import sys; sys.path.insert(0, '.'); from calc import add; sys.exit(0 if add(2, 3) == 5 else 1)"]}, "timeout": 60}
        (tdir / "fix-add.json").write_text(json.dumps(task))
        adir = root / "arms"
        adir.mkdir()
        fixer = "import pathlib; p = pathlib.Path('calc.py'); p.write_text(p.read_text().replace('a - b', 'a + b'))"
        (adir / "good.json").write_text(json.dumps({"name": "good", "kind": "command", "command": [sys.executable, "-I", "-c", fixer + "; print('{\"total_cost_usd\": 0.25, \"result\": \"ok\"}')"], "parse": "claude-json"}))
        (adir / "lazy.json").write_text(json.dumps({"name": "lazy", "kind": "command", "command": [sys.executable, "-I", "-c", "print('did nothing')"]}))
        (adir / "crash.json").write_text(json.dumps({"name": "crash", "kind": "command", "command": ["/no/such/arm", "{prompt}"]}))

        tasks, arms = load_all(tdir / "fix-add.json"), load_all(adir)
        # Planning: every arm once per seed, in an order that depends on the seed and not on the arm's name.
        jobs = plan(tasks, arms, 2, 7)
        assert len(jobs) == 6 and sorted(a["name"] for _, a, s in jobs if s == 1) == ["crash", "good", "lazy"]
        assert [a["name"] for _, a, _ in plan(tasks, arms, 2, 7)] == [a["name"] for _, a, _ in jobs], "the order is reproducible"
        assert plan(tasks, arms, 1, 1, only_arm="good")[0][1]["name"] == "good" and len(plan(tasks, arms, 1, 1, only_arm="good")) == 1

        results = root / "out" / "results.jsonl"
        ns = argparse.Namespace(tasks=str(tdir / "fix-add.json"), arms=str(adir), out=str(results), seeds=1, order_seed=3, only_task=None, only_arm=None, keep=False, dry_run=False)
        assert cmd_run(ns) == 0
        rows = {json.loads(l)["arm"]: json.loads(l) for l in results.read_text().splitlines()}
        assert rows["good"]["hidden_pass"] is True and rows["good"]["cost_usd"] == 0.25 and rows["good"]["arm_outcome"] == "completed", rows["good"]
        assert rows["lazy"]["hidden_pass"] is False and rows["lazy"]["cost_usd"] is None, rows["lazy"]
        assert rows["crash"]["exit_code"] == 127 and rows["crash"]["hidden_pass"] is False, "a missing arm is a failed run, not a crash"
        assert all(r["scratch"] is None for r in rows.values()), "scratch is removed"
        # The arms never touched the task's own repository.
        assert (repo / "calc.py").read_text().strip().endswith("a - b")

        # A hidden-test patch that does not apply is reported as a failure with the reason.
        bad = dict(task, hidden_tests=dict(task["hidden_tests"], patch="missing.patch"), _dir=str(tdir))
        ok, why = hidden_tests(bad, repo)
        assert ok is False and "did not apply" in why

        # Reading what the arms print.
        assert parse_output("claude-json", 'noise\n{"total_cost_usd": 1.5, "is_error": false, "result": "x", "num_turns": 4}\n') == (1.5, "completed", {"turns": 4})
        summary = json.dumps({"type": "run.summary", "outcome": "ready_to_merge", "ready_to_merge": {"sha": "abc", "branch": "delphi/t_0001"}, "usage": {"rows": [{"cost_usd": 0.5}, {"cost_usd": 0.25}]}, "cards": {"total": 2}})
        cost, outcome, extra = parse_output("delphi-summary", "progress\n" + summary + "\n")
        assert (cost, outcome, extra["ready_sha"], extra["ready_branch"]) == (0.75, "ready_to_merge", "abc", "delphi/t_0001")
        assert parse_output("delphi-summary", "no summary here")[1] is None and parse_output("none", "x") == (None, None, {})

        # The Delphi command line: a copy of the template home, this task's checkout, no shell, the prompt as one argument.
        t0 = tmp = pathlib.Path(tempfile.mkdtemp(dir=root))
        home = tmp / "tmpl"
        (home / "workspaces" / "bench").mkdir(parents=True)
        (home / "workspaces" / "bench" / "config.toml").write_text('[workspace]\nname = "bench"\nprojects = ["/old/path"]\n[theme]\nname = "greek"\n')
        arm = {"name": "a3", "kind": "delphi", "template_home": str(home), "workspace": "bench", "input": "/build {prompt}", "flags": ["--on-card", "policy:{policy}"], "_dir": str(adir)}
        h = prepare_delphi_home(arm, task, "/work/dir", t0)
        assert 'projects = ["/work/dir"]' in (h / "workspaces" / "bench" / "config.toml").read_text() and '[theme]' in (h / "workspaces" / "bench" / "config.toml").read_text()
        assert "/old/path" in (home / "workspaces" / "bench" / "config.toml").read_text(), "the template is not modified"
        c = delphi_command(arm, task, "/work/dir", h)
        assert c[:3] == ["delphi", "run", "/build add() subtracts. Fix it."] and "--json" in c and c[c.index("--home") + 1] == str(h) and c[c.index("--on-card") + 1].startswith("policy:") and c[c.index("--timeout") + 1] == "60s"
        (home / "workspaces" / "bench" / "config.toml").write_text('projects = ["a"]\nprojects = ["b"]\n')
        try:
            prepare_delphi_home(arm, task, "/w", pathlib.Path(tempfile.mkdtemp(dir=root)))
            raise AssertionError("two projects lines must be refused")
        except RuntimeError as e:
            assert "exactly one" in str(e)
        print("bench self-test: ok")
        return 0
    finally:
        shutil.rmtree(root, ignore_errors=True)


def main(argv):
    ap = argparse.ArgumentParser(description="evaluation benchmark runner")
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--tasks", required=True)
    r.add_argument("--arms", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--seeds", type=int, default=3)
    r.add_argument("--order-seed", type=int, default=1)
    r.add_argument("--only-task")
    r.add_argument("--only-arm")
    r.add_argument("--keep", action="store_true", help="keep each run's scratch directory (arm output, checkout)")
    r.add_argument("--dry-run", action="store_true")
    sub.add_parser("self-test")
    args = ap.parse_args(argv)
    return self_test() if args.cmd == "self-test" else cmd_run(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
