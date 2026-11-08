"""Behavior evals for the builder's git-merge-and-conflict-resolution skill (specs/094 section 3.7, AC-7b).

usage:
  python3 -I dev/skill-evals/merge_resolution/run.py selftest
  python3 -I dev/skill-evals/merge_resolution/run.py live --dry-run [--case NAME]...
  python3 -I dev/skill-evals/merge_resolution/run.py live --model M --template-home DIR --budget-tokens N --yes [--case NAME]...

selftest: no model, no network, under a minute. For every case a known-good canned resolution must pass its check and
at least one known-bad one must fail it. Exits non-zero on any miss. This proves the checks discriminate.

live: SPENDS THE OPERATOR'S TOKENS. Builds each case's repo in a temp dir, hands it to the builder through `delphi run`
(the headless path dev/bench/bench.py uses), then applies the check. Refuses to run without --budget-tokens and --yes.
Stdlib only; repos live in temp dirs; the Delphi repository's own git state and any real remote are never touched.
"""
import argparse
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import cases  # noqa: E402
import checks  # noqa: E402
from cases import git, write  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
POLICY = HERE.parent.parent / "bench" / "arms-example" / "policy.toml"


# ── canned resolutions: (case, label, expect_ok, function(repo)) ─────────────────────────────────────────────────

def merge(repo):
    return git(repo, "merge", "--no-edit", "main", check=False)


def finish(repo):
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "--no-edit")


def strip_markers_keep_both(repo, name):
    p = pathlib.Path(repo) / name
    p.write_text("".join(l for l in p.read_text().splitlines(True) if not re.match(r"^(<<<<<<<|=======|>>>>>>>)", l)))


def good_tracker(r):
    merge(r)
    write(r, "tracker.md", cases.TRACKER_BASE + "| 002 | approved |\n| 003 | draft |\n\n## Main notes\nmain section\n\n## Branch notes\nbranch section\n")
    finish(r)


def bad_tracker_keep_one(r):
    merge(r)
    git(r, "checkout", "--ours", "tracker.md")
    finish(r)


def bad_tracker_row_stranded(r):
    merge(r)
    write(r, "tracker.md", cases.TRACKER_BASE + "| 002 | approved |\n\n## Main notes\nmain section\n\n## Branch notes\nbranch section\n| 003 | draft |\n")
    finish(r)


def good_modlist(r):
    merge(r)
    write(r, "lib.rs", "mod a;\nmod b;\nmod x;\nmod y;\n\npub fn f() {}\n")
    finish(r)


def bad_modlist_keep_one(r):
    merge(r)
    git(r, "checkout", "--ours", "lib.rs")
    finish(r)


def good_generated(r):
    merge(r)
    git(r, "checkout", "--theirs", "out.md")
    pathlib.Path(r, "out.md").write_text(cases.gen_out(r))
    finish(r)


def bad_generated_hand_merge(r):
    merge(r)
    strip_markers_keep_both(r, "out.md")
    finish(r)


def good_lockfile(r):
    merge(r)
    git(r, "checkout", "--theirs", "Cargo.lock")
    finish(r)


def bad_lockfile_mix(r):
    merge(r)
    write(r, "Cargo.lock", cases.LOCK % ("1.0.3", "b" * 64))
    finish(r)


def bad_lockfile_ours(r):
    merge(r)
    git(r, "checkout", "--ours", "Cargo.lock")
    finish(r)


def good_behind(r):
    merge(r)


def bad_behind_rebase(r):
    git(r, "rebase", "main")


def bad_behind_untouched(r):
    pass


def good_semantic_fix(r):
    merge(r)
    p = pathlib.Path(r, "rule.sh")
    p.write_text(p.read_text().replace('[ "$1" -gt 10000 ]', '[ "$c" -gt 10000 ]'))
    git(r, "commit", "-q", "-am", "fix: the free-shipping test reads cents")


def good_semantic_needs_human(r):
    write(r, "NEEDS-HUMAN", "main changed fee() to take dollars, the branch tests cents; the intents need an owner's call.\n")


def bad_semantic_red_gate(r):
    merge(r)  # a clean textual merge, committed with check.sh failing
    pass


def bad_semantic_pick_by_file(r):
    merge(r)
    git(r, "checkout", "main", "--", "rule.sh")
    git(r, "commit", "-q", "-am", "take main's rule.sh")


CANNED = [
    ("tracker", "good: both rows and sections in order", True, good_tracker),
    ("tracker", "bad: keep the branch side only", False, bad_tracker_keep_one),
    ("tracker", "bad: row stranded below prose", False, bad_tracker_row_stranded),
    ("modlist", "good: both mod lines, gate green", True, good_modlist),
    ("modlist", "bad: keep the branch side only", False, bad_modlist_keep_one),
    ("generated", "good: take a side, regenerate", True, good_generated),
    ("generated", "bad: hand-merged, both sides kept", False, bad_generated_hand_merge),
    ("lockfile", "good: main's lockfile", True, good_lockfile),
    ("lockfile", "bad: hand-edited mix", False, bad_lockfile_mix),
    ("lockfile", "bad: kept the branch's lockfile", False, bad_lockfile_ours),
    ("behind", "good: merge main in", True, good_behind),
    ("behind", "bad: rebase onto main", False, bad_behind_rebase),
    ("behind", "bad: left behind", False, bad_behind_untouched),
    ("semantic", "good: combined behaviour, gate green", True, good_semantic_fix),
    ("semantic", "good: stopped with NEEDS-HUMAN", True, good_semantic_needs_human),
    ("semantic", "bad: textual merge committed, gate red", False, bad_semantic_red_gate),
    ("semantic", "bad: main's file taken by file, branch intent lost", False, bad_semantic_pick_by_file),
]


def selftest():
    failures = []
    tally = {}
    with tempfile.TemporaryDirectory(prefix="mr-selftest-") as root:
        for i, (name, label, expect, fn) in enumerate(CANNED):
            repo = pathlib.Path(root) / ("r%d" % i)
            cases.build(name, repo)
            ok0, why0 = checks.CHECKS[name](repo)
            if ok0:
                failures.append("%s: the fresh, unresolved repo must FAIL its check but passed" % name)
            fn(repo)
            ok, why = checks.CHECKS[name](repo)
            t = tally.setdefault(name, [0, 0])
            t[0 if expect else 1] += 1
            verdict = "pass" if ok == expect else "MISS"
            print("  %-9s %-52s check %s (%s)" % (name, label, "ok" if ok else "FAIL", why.replace("\n", "; ")[:70]), "" if ok == expect else "<-- " + verdict)
            if ok != expect:
                failures.append("%s / %s: expected %s, got %s (%s)" % (name, label, "ok" if expect else "fail", "ok" if ok else "fail", why))
        for name in cases.CASES:
            g, b = tally.get(name, [0, 0])
            if g < 1 or b < 1:
                failures.append("%s needs at least one good and one bad canned resolution (has %d good, %d bad)" % (name, g, b))
    print("cases x (good, bad): " + ", ".join("%s %d/%d" % (n, *tally.get(n, [0, 0])) for n in cases.CASES))
    if failures:
        print("merge-resolution self-test: FAILED")
        for f in failures:
            print("  - " + f)
        return 1
    print("merge-resolution self-test: ok")
    return 0


# ── live mode ────────────────────────────────────────────────────────────────────────────────────────────────────

def prepare_home(template_home, workspace, project, scratch):
    """A copy of the pre-ratified template home pointed at this case's repo (only `projects = [...]` is edited)."""
    home = pathlib.Path(scratch) / "home"
    shutil.copytree(template_home, home, symlinks=True)
    cfg = home / "workspaces" / workspace / "config.toml"
    new, n = re.subn(r"(?m)^projects\s*=\s*\[[^\]]*\]", 'projects = ["%s"]' % str(project).replace("\\", "\\\\").replace('"', '\\"'), cfg.read_text())
    if n != 1:
        raise RuntimeError("the template workspace config must have exactly one `projects = [...]` line (found %d)" % n)
    cfg.write_text(new)
    return home


def delphi_command(args, task, home):
    return [args.delphi, "run", "/build " + task, "--json", "--workspace", args.workspace, "--home", str(home),
            "--timeout", "%ds" % args.timeout, "--on-card", "policy:%s" % POLICY]


def live(args):
    names = args.case or list(cases.CASES)
    for n in names:
        if n not in cases.CASES:
            print("unknown case " + n)
            return 2
    if args.dry_run:
        with tempfile.TemporaryDirectory(prefix="mr-dry-") as root:
            for n in names:
                repo = cases.build(n, pathlib.Path(root) / n)
                print("%s: built %s (branch feature, main tip %s)" % (n, repo, cases.head(repo, "main")[:10]))
                print("  would run: " + " ".join(delphi_command(args, cases.CASES[n][1], "<home>")))
                print("  then check: checks.check_%s(repo)" % n)
        print("dry run: nothing was sent to a model, no tokens spent.")
        return 0
    print("WARNING: live mode SPENDS THE OPERATOR'S TOKENS: %d case(s), each a full builder run on model %s." % (len(names), args.model))
    if not args.budget_tokens or args.budget_tokens <= 0:
        print("refusing: --budget-tokens N is required (the most you accept to spend across all cases)")
        return 2
    if not args.yes:
        print("refusing: pass --yes to confirm that you accept spending up to %d tokens" % args.budget_tokens)
        return 2
    if not args.model or not args.template_home:
        print("refusing: --model and --template-home (a pre-ratified Delphi home, as dev/bench uses) are required")
        return 2
    results = []
    spent = 0
    for n in names:
        if spent >= args.budget_tokens:
            results.append((n, None, "skipped: token budget reached"))
            continue
        with tempfile.TemporaryDirectory(prefix="mr-live-") as root:
            repo = cases.build(n, pathlib.Path(root) / "repo")
            home = prepare_home(args.template_home, args.workspace, repo, root)
            cmd = delphi_command(args, cases.CASES[n][1], home)
            p = subprocess.run(cmd, capture_output=True, text=True, stdin=subprocess.DEVNULL, env=dict(cases.env(), HOME=str(pathlib.Path.home())))
            spent += tokens_in(p.stdout)
            ok, why = checks.CHECKS[n](repo)
            results.append((n, ok, why + " (delphi exit %s)" % p.returncode))
    for n, ok, why in results:
        print("%-9s %s  %s" % (n, "skip" if ok is None else ("PASS" if ok else "FAIL"), why))
    print("tokens counted from run summaries: %d of %d" % (spent, args.budget_tokens))
    return 0 if all(ok for _, ok, _ in results if ok is not None) else 1


def tokens_in(stdout):
    """Tokens in the run.summary line's usage rows, if it reports any; else 0 (the budget then only limits case count)."""
    for line in reversed(stdout.strip().splitlines()):
        if '"run.summary"' in line:
            try:
                rows = (json.loads(line).get("usage") or {}).get("rows") or []
            except ValueError:
                return 0
            return sum(int(r.get("input_tokens") or 0) + int(r.get("output_tokens") or 0) for r in rows)
    return 0


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("selftest")
    lv = sub.add_parser("live")
    lv.add_argument("--model", help="a label for the report; `delphi run` takes its model from the template home's workspace config")
    lv.add_argument("--template-home")
    lv.add_argument("--workspace", default="bench")
    lv.add_argument("--delphi", default="delphi")
    lv.add_argument("--timeout", type=int, default=900)
    lv.add_argument("--budget-tokens", type=int)
    lv.add_argument("--yes", action="store_true")
    lv.add_argument("--dry-run", action="store_true")
    lv.add_argument("--case", action="append")
    a = ap.parse_args(argv)
    return selftest() if a.cmd == "selftest" else live(a)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
