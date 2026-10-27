"""Benchmark runner for the evaluation plan (missions/.../evaluation-plan-2026-10-25.md §7).

usage:
  python3 -I dev/bench/bench.py run --tasks <dir or file> --arms <dir or file> --out <results.jsonl>
                                    [--seeds 3] [--pilot] [--prereg F --prereg-pin blake3:H --rubric P] [--order-seed 1] [--only-task ID] [--only-arm NAME] [--keep] [--dry-run]
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
import argparse, hashlib, json, os, pathlib, random, re, shutil, subprocess, sys, tempfile, time

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


def parse_task_md(text, source="task"):
    """T8, an organization task (`specs/027` §7.2), from a bundle's `evals/delphi/tasks/*.md`:

        ---
        id: add-retry-to-billing-client
        repo: /path/or/url
        base: <commit>
        set: pilot            # or confirmatory; the two sets must not share an id
        ---
        ## Prompt
        the issue text, which is all an arm sees
        ## Rubric
        the hidden rubric, which no arm sees and a blind grader from another family applies

    Returns a task dict. The rubric stays in `rubric` and `rubric_sha256`; it is never put in an arm's prompt or command."""
    m = re.match(r"---\n(.*?)\n---\n(.*)\Z", text, re.S)
    if not m:
        raise ValueError("%s: a task needs a frontmatter block" % source)
    meta = {}
    for line in m.group(1).splitlines():
        line = re.sub(r"\s+#.*$", "", line)
        if ":" in line:
            k, v = line.split(":", 1)
            meta[k.strip()] = v.strip()
    sections, cur = {}, None
    for line in m.group(2).splitlines():
        h = re.match(r"##\s+(.*?)\s*$", line)
        if h:
            cur = h.group(1).lower()
            sections[cur] = []
        elif cur:
            sections[cur].append(line)
    body = lambda k: "\n".join(sections.get(k, [])).strip()
    for need in ("id", "repo", "base"):
        if not meta.get(need):
            raise ValueError("%s: frontmatter lacks `%s`" % (source, need))
    if not body("prompt") or not body("rubric"):
        raise ValueError("%s: needs a `## Prompt` and a `## Rubric` section" % source)
    if meta.get("set", "confirmatory") not in ("pilot", "confirmatory"):
        raise ValueError("%s: `set` is pilot or confirmatory" % source)
    rubric = body("rubric")
    return {"id": meta["id"], "repo": meta["repo"], "base": meta["base"], "set": meta.get("set", "confirmatory"),
            "prompt": body("prompt"), "rubric": rubric, "rubric_sha256": hashlib.sha256(rubric.encode()).hexdigest(),
            "timeout": int(meta["timeout"]) if meta.get("timeout") else DEFAULT_TIMEOUT}


def load_all(path):
    """Tasks and arms: `*.json`, and for tasks `*.md` (T8)."""
    p = pathlib.Path(path)
    files = sorted(list(p.glob("*.json")) + list(p.glob("*.md"))) if p.is_dir() else [p]
    out = []
    for f in files:
        d = parse_task_md(f.read_text(), str(f)) if f.suffix == ".md" else json.loads(f.read_text())
        d["_dir"] = str(f.parent)
        out.append(d)
    ids = [d.get("id") or d.get("name") for d in out]
    if len(set(ids)) != len(ids):
        raise ValueError("duplicate id among %s" % path)
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


# ── A3b: Delphi with the organization's bundle added ────────────────────────

# What a bundle copy for an arm must not carry: the evals hold the hidden rubric (`specs/027` §7.2), and the machine-written
# files are regenerated for the stripped tree.
BUNDLE_DROP = ["evals", ".delphi-bundle/files.json", ".delphi-bundle/sig.json", ".delphi-bundle/eval.json"]


def strip_bundle(src, dst):
    """A copy of the bundle at `src` that an arm may be given: no `evals/` (the hidden rubric would be readable as
    `@bundle/...`), no signature, no stale manifest, and `evals = true` removed from `[sections]` so it still validates."""
    shutil.copytree(src, dst, symlinks=True, ignore=shutil.ignore_patterns(".git"))
    dst = pathlib.Path(dst)
    for rel in BUNDLE_DROP:
        t = dst / rel
        if t.is_dir() and not t.is_symlink():
            shutil.rmtree(t)
        elif t.exists() or t.is_symlink():
            t.unlink()
    toml = dst / "bundle.toml"
    if toml.is_file():
        toml.write_text(re.sub(r"(?m)^evals\s*=\s*true[ \t]*\n", "", toml.read_text()))
    return dst


def add_bundle(arm, home, scratch):
    """Add the arm's bundle (stripped) to the run's own copy of the home; returns the bundle hash the install computed."""
    delphi, ws = arm.get("delphi", "delphi"), arm.get("workspace", "bench")
    stripped = strip_bundle(arm["bundle"], pathlib.Path(scratch) / "bundle")
    claude = ["--claude"] if (stripped / ".claude-plugin").is_dir() else []
    code, out, err, _, _ = sh([delphi, "bundle", "manifest", str(stripped), *claude], timeout=120)
    if code != 0:
        raise RuntimeError("bundle manifest failed: " + (err or out).strip()[-300:])
    m = re.search(r"bundle hash (\S+)", out)
    code, out2, err, _, _ = sh([delphi, "bundle", "add", str(stripped), "--yes", "--workspace", ws, "--home", str(home)], timeout=600)
    if code != 0:
        raise RuntimeError("bundle add failed: " + (err or out2).strip()[-300:])
    return m.group(1) if m else None


def count_cited(home, workspace):
    """`bundle.cited` events the run wrote (`specs/027` §7.3): descriptive, from the log, never from the model."""
    n = 0
    for f in pathlib.Path(home, "workspaces", workspace, "local", "missions").glob("*/events/*"):
        try:
            for line in f.read_text().splitlines():
                if '"bundle.cited"' in line and json.loads(line).get("kind") == "bundle.cited":
                    n += 1
        except (OSError, ValueError):
            continue
    return n


def run_one(task, arm, seed, scratch_root, keep=False, stage=None, extra_row=None):
    scratch = pathlib.Path(tempfile.mkdtemp(prefix="bench-", dir=scratch_root))
    workdir = scratch / "work"
    row = {"task": task["id"], "arm": arm["name"], "seed": seed, "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    if stage:
        # T8 columns: the protocol stage and task set, the bundle the arm had, the rubric's digest (the rubric itself stays
        # in the task file), and `graded` for the blind grader's verdict, filled in later by a grader from another family.
        row.update({"stage": stage, "task_set": task.get("set"), "rubric_sha256": task.get("rubric_sha256"), "graded": None, "bundle": None, "bundle_hash": None, "bundle_cited": None})
        row.update(extra_row or {})
    try:
        clone_at(task, workdir)
        timeout = int(arm.get("timeout", task.get("timeout", DEFAULT_TIMEOUT)))
        if arm.get("kind") == "delphi":
            home = prepare_delphi_home(arm, task, workdir, scratch)
            if arm.get("bundle"):
                row["bundle"] = pathlib.Path(arm["bundle"]).name
                row["bundle_hash"] = add_bundle(arm, home, scratch)
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
        if stage and arm.get("kind") == "delphi":
            row["bundle_cited"] = count_cited(home, arm.get("workspace", "bench"))
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


# ── T8: pilot first, then the pre-registered confirmatory run (`specs/027` §7.2) ──

PREREG_HEADINGS = ["Hypotheses", "Arms", "Task set", "Rubric", "Sample size and runs", "Thresholds", "Analysis plan", "Exclusions and stopping", "Deviations"]
MIN_PILOT_TASKS = 8
PILOT_SEEDS = 2


class StageError(Exception):
    """A refusal that protects the protocol; the message says what to do."""


def md_headings(text):
    return [(len(m.group(1)), m.group(2).strip().lower()) for m in re.finditer(r"(?m)^(#+)[ \t]+(.*)$", text)]


def prereg_missing(text):
    have = {h for _, h in md_headings(text)}
    return [h for h in PREREG_HEADINGS if h.lower() not in have]


def md_section(text, name):
    lines = text.splitlines()
    for i, l in enumerate(lines):
        m = re.match(r"(#+)[ \t]+(.*)$", l)
        if m and m.group(2).strip().lower() == name.lower():
            level, body = len(m.group(1)), []
            for l2 in lines[i + 1:]:
                m2 = re.match(r"(#+)[ \t]", l2)
                if m2 and len(m2.group(1)) <= level:
                    break
                body.append(l2)
            return "\n".join(body)
    return None


def delphi_hasher(delphi):
    """blake3 comes from the same build that computes the pin (`delphi bundle prereg …`); Python has none in its library."""
    def hasher(what, path):
        code, out, err, _, _ = sh([delphi, "bundle", "prereg", what, str(path)], timeout=120)
        if code != 0:
            raise StageError("`delphi bundle prereg %s` failed: %s" % (what, (err or out).strip()[-300:]))
        return out.strip()
    return hasher


def check_prereg(path, pin, targets, hasher):
    """The pre-registration is whole, unchanged since it was pinned, and names the digests of what is about to run.
    `targets` maps a section heading to the file or directory whose digest that section must contain."""
    p = pathlib.Path(path)
    if not p.is_file():
        raise StageError("the pre-registration %s does not exist; commit it before any confirmatory run" % path)
    text = p.read_text()
    missing = prereg_missing(text)
    if missing:
        raise StageError("the pre-registration lacks the heading(s): %s" % ", ".join(missing))
    if not re.fullmatch(r"blake3:[0-9a-f]{64}", pin or ""):
        raise StageError("--prereg-pin wants the blake3:<hex> recorded in specs/027 §7.2 when the file was pinned")
    got = hasher("check", p)
    if got != pin:
        raise StageError("the pre-registration changed after it was pinned (pinned %s, now %s); only `## Deviations` may change, and the report must carry the deviation" % (pin, got))
    for heading, target in targets.items():
        digest = hasher("digest", target)
        if digest not in (md_section(text, heading) or ""):
            raise StageError("`## %s` does not contain the digest %s of %s; the thing that will run is not the thing that was registered" % (heading, digest, target))


def select_stage(tasks, arms, args, hasher=None):
    """(tasks to run, stage or None, extra row columns, seeds). Refuses what the protocol forbids; touches nothing."""
    t8 = [t for t in tasks if "set" in t]
    seeds = getattr(args, "seeds", None)
    if not t8:
        if getattr(args, "pilot", False):
            raise StageError("--pilot applies to T8 tasks (`set: pilot` in evals/delphi/tasks/*.md)")
        return tasks, None, {}, seeds or 3
    pilot_t, conf_t = [t for t in t8 if t["set"] == "pilot"], [t for t in t8 if t["set"] != "pilot"]
    clash = {t["id"] for t in pilot_t} & {t["id"] for t in conf_t}
    clash |= {a["id"] for a in pilot_t for b in conf_t if a["prompt"] == b["prompt"]}
    if clash:
        raise StageError("the pilot and confirmatory task sets must be disjoint; shared: %s" % ", ".join(sorted(clash)))
    bundle_arms = [a for a in arms if a.get("bundle")]
    if not bundle_arms:
        raise StageError("T8 compares A3b with A3: no arm names a `bundle`")
    for a in bundle_arms:
        if not any(b["name"] == a.get("paired_with") for b in arms):
            raise StageError("arm %s needs its A3 pair (`paired_with`) among the arms: the comparison is paired" % a["name"])
    if getattr(args, "pilot", False):
        if len(pilot_t) < MIN_PILOT_TASKS:
            raise StageError("a pilot needs at least %d tasks with `set: pilot` (found %d)" % (MIN_PILOT_TASKS, len(pilot_t)))
        if seeds not in (None, PILOT_SEEDS):
            raise StageError("a pilot is %d runs per task and arm" % PILOT_SEEDS)
        if "pilot" not in pathlib.Path(args.out).name:
            raise StageError("name the output file with `pilot` (e.g. results-pilot.jsonl) so pilot rows are never mistaken for results")
        return pilot_t, "pilot", {}, PILOT_SEEDS
    if not conf_t:
        raise StageError("no confirmatory tasks (`set: confirmatory`) to run")
    if not getattr(args, "prereg", None):
        raise StageError("a confirmatory run needs --prereg <file> and --prereg-pin <blake3:…>, committed before the run (specs/027 §7.2); use --pilot to try the runner")
    if not getattr(args, "rubric", None):
        raise StageError("--rubric <file or directory> (the rubric and grader prompt) is needed so the `## Rubric` digest can be checked")
    hasher = hasher or delphi_hasher(getattr(args, "delphi", "delphi"))
    check_prereg(args.prereg, getattr(args, "prereg_pin", None), {"Task set": args.tasks, "Rubric": args.rubric}, hasher)
    return conf_t, "confirmatory", {"prereg": str(args.prereg), "prereg_pin": args.prereg_pin}, seeds or 3


def cmd_run(args, hasher=None):
    tasks, arms = load_all(args.tasks), load_all(args.arms)
    try:
        tasks, stage, extra_row, seeds = select_stage(tasks, arms, args, hasher)
    except StageError as e:
        print("bench: refused: %s" % e, file=sys.stderr)
        return 2
    jobs = plan(tasks, arms, seeds, args.order_seed, args.only_task, args.only_arm)
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
        row = run_one(t, a, s, scratch_root, keep=args.keep, stage=stage, extra_row=extra_row)
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
        self_test_t8(root, repo, base, adir)
        print("bench self-test: ok")
        return 0
    finally:
        shutil.rmtree(root, ignore_errors=True)


def self_test_t8(root, repo, base, adir):
    """A3b / T8: task files, pilot and confirmatory separation, the pre-registration checks, the bundle copy, the columns."""
    canary = "HIDDEN-RUBRIC-CANARY"
    t8 = root / "t8"
    t8.mkdir()

    def task_md(i, set_):
        return "---\nid: org-%s-%d\nrepo: %s\nbase: %s\nset: %s   # comment\n---\n## Prompt\nDo org task %s-%d.\n## Rubric\n- %s %d\n" % (set_, i, repo, base, set_, set_, i, canary, i)

    for i in range(8):
        (t8 / ("p%d.md" % i)).write_text(task_md(i, "pilot"))
    for i in range(3):
        (t8 / ("c%d.md" % i)).write_text(task_md(i, "confirmatory"))
    parsed = parse_task_md(task_md(1, "pilot"))
    assert parsed["set"] == "pilot" and parsed["id"] == "org-pilot-1" and canary in parsed["rubric"] and canary not in parsed["prompt"]
    for bad in ("no frontmatter", "---\nid: x\nrepo: r\nbase: b\n---\n## Prompt\np\n", "---\nid: x\nrepo: r\nbase: b\nset: odd\n---\n## Prompt\np\n## Rubric\nr\n"):
        try:
            parse_task_md(bad)
            raise AssertionError("a malformed task must be refused")
        except ValueError:
            pass

    arms = root / "t8arms"
    arms.mkdir()
    bundle_src = root / "bundle-src"
    (bundle_src / "evals/delphi/tasks").mkdir(parents=True)
    (bundle_src / ".delphi-bundle").mkdir()
    (bundle_src / "knowledge").mkdir()
    (bundle_src / "knowledge/k.md").write_text("# K\n")
    (bundle_src / "evals/delphi/tasks/t.md").write_text(canary)
    for n in ("files.json", "sig.json", "eval.json"):
        (bundle_src / ".delphi-bundle" / n).write_text("{}")
    (bundle_src / "bundle.toml").write_text('[bundle]\nname = "b"\n[sections]\nevals = true\nvectors = false\n')
    ok_cmd = [sys.executable, "-I", "-c", "print('{\"total_cost_usd\": 0.1, \"result\": \"ok\"}')"]
    (arms / "a3.json").write_text(json.dumps({"name": "a3", "kind": "command", "command": ok_cmd, "parse": "claude-json"}))
    (arms / "a3b.json").write_text(json.dumps({"name": "a3b", "kind": "command", "command": ok_cmd, "parse": "claude-json", "bundle": str(bundle_src), "paired_with": "a3"}))

    # The bundle an arm gets has no evals (the rubric would be readable), no signature, and still validates its sections.
    stripped = strip_bundle(bundle_src, root / "bundle-stripped")
    assert not (stripped / "evals").exists() and not any((stripped / ".delphi-bundle" / n).exists() for n in ("files.json", "sig.json", "eval.json"))
    assert (stripped / "knowledge/k.md").is_file() and "evals" not in (stripped / "bundle.toml").read_text() and "vectors = false" in (stripped / "bundle.toml").read_text()
    assert (bundle_src / "evals/delphi/tasks/t.md").is_file(), "the source is untouched"
    assert not any(canary in p.read_text() for p in stripped.rglob("*") if p.is_file())

    # Cited events are counted from the log, not from what an arm says.
    h = root / "citehome"
    (h / "workspaces/bench/local/missions/m_1/events").mkdir(parents=True)
    (h / "workspaces/bench/local/missions/m_1/events/00000001.jsonl").write_text(
        json.dumps({"kind": "bundle.cited", "payload": {}}) + "\n" + json.dumps({"kind": "task.output", "payload": {"text": "bundle.cited"}}) + "\n" + "not json bundle.cited\n")
    assert count_cited(h, "bench") == 1

    class A:  # the command-line namespace
        def __init__(self, **kw):
            self.__dict__.update(dict(tasks=str(t8), arms=str(arms), out=str(root / "o" / "results-pilot.jsonl"), seeds=None, order_seed=1, only_task=None, only_arm=None, keep=False, dry_run=False, pilot=False, prereg=None, prereg_pin=None, rubric=None, delphi="delphi"))
            self.__dict__.update(kw)

    tasks, armlist = load_all(t8), load_all(arms)

    def refused(args, hasher=None, why=""):
        try:
            select_stage(tasks, armlist, args, hasher)
        except StageError as e:
            assert why in str(e), str(e)
            return
        raise AssertionError("expected a refusal containing %r" % why)

    # Pilot: the pilot set only, 2 runs, at least 8 tasks, output named pilot, pairs present.
    chosen, stage, extra, seeds = select_stage(tasks, armlist, A(pilot=True))
    assert (stage, seeds, len(chosen), {t["set"] for t in chosen}) == ("pilot", 2, 8, {"pilot"}) and extra == {}
    refused(A(pilot=True, seeds=3), why="2 runs")
    refused(A(pilot=True, out=str(root / "results.jsonl")), why="pilot")
    few = [t for t in tasks if t["set"] != "pilot" or t["id"] != "org-pilot-0"]
    try:
        select_stage(few, armlist, A(pilot=True))
        raise AssertionError("seven pilot tasks are too few")
    except StageError as e:
        assert "at least 8" in str(e)
    try:
        select_stage(tasks, [a for a in armlist if a["name"] == "a3b"], A(pilot=True))
        raise AssertionError("an unpaired A3b is refused")
    except StageError as e:
        assert "paired" in str(e)
    try:
        select_stage(tasks + [dict(next(t for t in tasks if t["set"] == "pilot"), set="confirmatory")], armlist, A(pilot=True))
        raise AssertionError("sets must be disjoint")
    except StageError as e:
        assert "disjoint" in str(e)

    # Confirmatory: refused without a pre-registration, with an incomplete one, with a changed one, and when a digest is missing.
    refused(A(), why="--prereg")
    prereg = root / "prereg.md"
    rub = root / "rubric.txt"
    rub.write_text("grader prompt")
    fake = lambda what, path: ("blake3:" + hashlib.sha256((what + ("".join(PREREG_HEADINGS) if what == "check" else "") + (pathlib.Path(path).read_text() if what == "check" else str(pathlib.Path(path).name))).encode()).hexdigest()) if what == "digest" else "blake3:" + hashlib.sha256(re.sub(r"(?s)(## Deviations\n).*?(?=\n## |\Z)", r"\1", pathlib.Path(path).read_text()).encode()).hexdigest()
    body = "".join("## %s\n%s\n\n" % (h, {"Task set": fake("digest", t8), "Rubric": fake("digest", rub)}.get(h, "x")) for h in PREREG_HEADINGS if h != "Deviations") + "## Deviations\n"
    prereg.write_text("# Pre-registration\n\n" + body)
    pin = fake("check", prereg)
    args = A(prereg=str(prereg), prereg_pin=pin, rubric=str(rub))
    chosen, stage, extra, seeds = select_stage(tasks, armlist, args, fake)
    assert (stage, seeds, len(chosen), extra["prereg_pin"]) == ("confirmatory", 3, 3, pin) and {t["set"] for t in chosen} == {"confirmatory"}
    prereg.write_text(prereg.read_text() + "- 2026-11-02: a run was excluded because the arm crashed.\n")
    select_stage(tasks, armlist, args, fake)  # a deviation appended under its heading keeps the pin
    prereg.write_text(prereg.read_text().replace("## Arms\nx", "## Arms\nchanged after the pin"))
    refused(args, fake, "changed after it was pinned")
    prereg.write_text("# Pre-registration\n\n## Hypotheses\nx\n")
    refused(args, fake, "lacks the heading")
    prereg.write_text("# Pre-registration\n\n" + body.replace(fake("digest", rub), "blake3:" + "0" * 64))
    refused(A(prereg=str(prereg), prereg_pin=fake("check", prereg), rubric=str(rub)), fake, "does not contain the digest")
    refused(A(prereg=str(prereg), prereg_pin="nonsense", rubric=str(rub)), fake, "--prereg-pin")
    refused(A(prereg=str(prereg), prereg_pin=pin), fake, "--rubric")

    # A pilot through the runner: both arms, 2 runs, the T8 columns, and the rubric text nowhere in the rows.
    results = root / "o" / "results-pilot.jsonl"
    assert cmd_run(A(only_task="org-pilot-0", pilot=True)) == 0
    rows = [json.loads(l) for l in results.read_text().splitlines()]
    assert len(rows) == 4 and {r["arm"] for r in rows} == {"a3", "a3b"} and {r["seed"] for r in rows} == {1, 2}
    assert all(r["stage"] == "pilot" and r["task_set"] == "pilot" and r["graded"] is None and r["rubric_sha256"] for r in rows)
    assert canary not in results.read_text(), "the rubric never reaches the rows"
    assert cmd_run(A(pilot=True, dry_run=True)) == 0
    assert cmd_run(A(out=str(root / "o" / "x.jsonl"))) == 2, "a confirmatory run without a pre-registration is refused before anything runs"
    assert not (root / "o" / "x.jsonl").exists()


def main(argv):
    ap = argparse.ArgumentParser(description="evaluation benchmark runner")
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--tasks", required=True)
    r.add_argument("--arms", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--seeds", type=int, default=None, help="runs per task and arm (default 3; a T8 pilot is 2)")
    r.add_argument("--pilot", action="store_true", help="T8: run the disjoint pilot task set (`set: pilot`), 2 runs each; results are not reported")
    r.add_argument("--prereg", help="T8 confirmatory: the committed pre-registration file")
    r.add_argument("--prereg-pin", help="T8 confirmatory: the blake3:<hex> pin recorded in specs/027 §7.2")
    r.add_argument("--rubric", help="T8 confirmatory: the rubric and grader prompt (file or directory) whose digest `## Rubric` carries")
    r.add_argument("--delphi", default="delphi", help="the delphi binary that computes blake3 digests")
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
