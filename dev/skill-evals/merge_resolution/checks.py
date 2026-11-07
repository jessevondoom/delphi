"""Checks run on a repository AFTER a resolution is committed. Pure inspection: each `check_*(repo) -> (ok, reason)`.

They read `.git/eval-meta.json` written by cases.py. The self-test (run.py selftest) proves each one passes a
known-good resolution and fails known-bad ones before any model is involved.
"""
import json
import pathlib
import subprocess

from cases import git, gen_out


def _meta(repo):
    return json.loads((pathlib.Path(repo) / ".git" / "eval-meta.json").read_text())


def _read(repo, name):
    p = pathlib.Path(repo) / name
    return p.read_text() if p.exists() else None


def _ancestor(repo, a, b):
    return git(repo, "merge-base", "--is-ancestor", a, b, check=False).returncode == 0


def common(repo, marker_ok=False):
    """Checks that hold in every case. Returns a reason string on failure, else None."""
    m = _meta(repo)
    if git(repo, "remote").stdout.strip():
        return "the repo has a remote; evals are local only"
    if git(repo, "rev-parse", "main").stdout.strip() != m["main_tip"]:
        return "the base branch main was moved"
    st = [l for l in git(repo, "status", "--porcelain").stdout.splitlines() if not (marker_ok and l.endswith("NEEDS-HUMAN"))]
    if st:
        return "working tree not clean: " + "; ".join(st[:3])
    if git(repo, "ls-files", "-u").stdout.strip():
        return "unmerged paths remain"
    if git(repo, "grep", "-nE", "^(<<<<<<<|>>>>>>>) |^=======$", "HEAD", check=False).stdout.strip():
        return "conflict markers committed"
    if git(repo, "symbolic-ref", "--short", "HEAD").stdout.strip() != "feature":
        return "not on branch feature"
    return None


def merged_not_rebased(repo):
    """The base tip is in the branch via a merge commit, and the original branch commits are untouched ancestors."""
    m = _meta(repo)
    if not _ancestor(repo, m["main_tip"], "HEAD"):
        return "main's tip is not an ancestor of the branch (not brought up to date)"
    if not _ancestor(repo, m["branch_tip"], "HEAD"):
        return "the original branch commits are no longer ancestors (history rewritten, e.g. a rebase)"
    if int(git(repo, "rev-list", "--merges", "--count", m["main_tip"] + "..HEAD").stdout.strip() or 0) < 1:
        return "no merge commit (a merge commit is required here)"
    return None


def _fail(why):
    return (False, why)


def _finish(repo, *steps):
    r = common(repo)
    if r:
        return _fail(r)
    r = merged_not_rebased(repo)
    if r:
        return _fail(r)
    for s in steps:
        r = s()
        if r:
            return _fail(r)
    return (True, "ok")


def check_tracker(repo):
    def content():
        t = _read(repo, "tracker.md") or ""
        lines = t.split("\n")
        need = ["| 001 | done |", "| 002 | approved |", "| 003 | draft |"]
        if any(n not in lines for n in need):
            return "a table row is missing"
        idx = [lines.index(n) for n in need]
        if idx != [idx[0], idx[0] + 1, idx[0] + 2]:
            return "table rows are not contiguous and in order directly under the table"
        if lines[idx[0] - 1] != "| :-- | :-- |":
            return "the rows are not under their table"
        for s in ("## Main notes", "main section", "## Branch notes", "branch section"):
            if t.count(s) != 1:
                return "section text %r should appear exactly once" % s
        if not (idx[2] < lines.index("## Main notes") < lines.index("## Branch notes")):
            return "sections missing, out of order, or above the table rows"
        return None
    return _finish(repo, content)


def check_modlist(repo):
    def content():
        t = _read(repo, "lib.rs") or ""
        for l in ("mod a;", "mod b;", "mod x;", "mod y;"):
            if t.split("\n").count(l) != 1:
                return "%r should appear exactly once in lib.rs" % l
        p = subprocess.run(["sh", "check.sh"], cwd=str(repo), capture_output=True, text=True)
        if p.returncode != 0:
            return "check.sh fails: " + (p.stdout + p.stderr).strip()
        return None
    return _finish(repo, content)


def check_generated(repo):
    def content():
        items = (_read(repo, "items.txt") or "").split("\n")
        for i in ("gamma", "alpha", "beta", "delta", "epsilon"):
            if i not in items:
                return "items.txt lost %s" % i
        if _read(repo, "out.md") != gen_out(repo):
            return "out.md is not the generator's output (hand-merged or stale)"
        return None
    return _finish(repo, content)


def check_lockfile(repo):
    def content():
        want = git(repo, "show", "main:Cargo.lock").stdout
        if _read(repo, "Cargo.lock") != want:
            return "Cargo.lock is not main's version"
        if _read(repo, "notes.txt") != "feature work\n":
            return "the branch's own work (notes.txt) was lost"
        return None
    return _finish(repo, content)


def check_behind(repo):
    def content():
        for f in ("one.txt", "main.txt", "feature.txt"):
            if _read(repo, f) is None:
                return "%s missing" % f
        if _read(repo, "feature.txt") != "feature\nmore\n":
            return "feature.txt changed"
        return None
    return _finish(repo, content)


def check_semantic(repo):
    """Either a committed merge where the gate (check.sh) is green, or an unmerged repo with a NEEDS-HUMAN file."""
    m = _meta(repo)
    marker = pathlib.Path(repo) / "NEEDS-HUMAN"
    if marker.exists() and git(repo, "rev-parse", "HEAD").stdout.strip() == m["branch_tip"]:
        r = common(repo, marker_ok=True)
        if r:
            return _fail(r)
        if not marker.read_text().strip():
            return _fail("NEEDS-HUMAN is empty: it must say why")
        return (True, "stopped with NEEDS-HUMAN, nothing merged")
    r = common(repo)
    if r:
        return _fail(r)
    r = merged_not_rebased(repo)
    if r:
        return _fail(r)
    p = subprocess.run(["sh", "check.sh"], cwd=str(repo), capture_output=True, text=True)
    if p.returncode != 0:
        return _fail("merge committed with the gate red: " + (p.stdout + p.stderr).strip())
    if "$c\" -gt 10000" not in (_read(repo, "rule.sh") or "") and "-gt 10000" not in (_read(repo, "rule.sh") or ""):
        return _fail("branch intent (free over 10000 cents) is gone from rule.sh")
    if "* 100" not in (_read(repo, "rule.sh") or ""):
        return _fail("main intent (dollars in) is gone from rule.sh")
    return (True, "merged with both intents and a green gate")


CHECKS = {
    "tracker": check_tracker, "modlist": check_modlist, "generated": check_generated,
    "lockfile": check_lockfile, "behind": check_behind, "semantic": check_semantic,
}
