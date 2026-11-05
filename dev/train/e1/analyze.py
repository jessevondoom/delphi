#!/usr/bin/env python3
"""Apply the pre-registered decision rule of experiment E1 (dev/train/e1/PREREG.md) to a `rows.jsonl`.

    python3 -I dev/train/e1/analyze.py <work-dir>/rows.jsonl [--arm solo-gemini-flash]

The rule is mechanical on purpose. Cases come in pairs `<id>-t` (terse, the prompt as shipped) and `<id>-c` (the same prompt plus the
fixed contract block). The primary outcome is the deterministic pass. Tokens and fact recall are reported and never decide.
"""
import argparse
import json
import re
import statistics
import sys
from collections import defaultdict

MIN_DIFF = 4          # total passes, contract minus terse, of 15 per variant
MIN_AGREE = 3         # of 5 cases must point the same way as the total
CEILING = 12          # at or above this many terse passes of 15, an improvement of MIN_DIFF cannot be shown
TOKEN_PCT = 25        # secondary: median tokens differing by this much, in at least 4 of 5 cases
RECALL = re.compile(r"fact recall (\d+)%")


def load(path, arm):
    rows = []
    for line in open(path, encoding="utf-8"):
        line = line.strip()
        if line:
            r = json.loads(line)
            if arm is None or r["arm"] == arm:
                rows.append(r)
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("rows")
    ap.add_argument("--arm", default=None)
    a = ap.parse_args()
    rows = load(a.rows, a.arm)
    by = defaultdict(lambda: {"t": [], "c": []})
    excluded = []
    for r in rows:
        base, _, tag = r["case"].rpartition("-")
        if tag not in ("t", "c"):
            continue
        # A run is graded when it has a verdict and the arm stayed as declared (no staffing note, no infrastructure error).
        if r["passed"] is None or r.get("error") or any("staff" in n.lower() or "declared" in n.lower() for n in r["notes"]):
            excluded.append(r)
            continue
        by[base][tag].append(r)

    cases = sorted(by)
    print(f"{'case':26} {'terse':>8} {'contract':>9} {'d':>4}   tokens(median) terse -> contract   recall% terse -> contract")
    tot = {"t": 0, "c": 0}
    n = {"t": 0, "c": 0}
    diffs, tokdirs = [], []
    recall = {"t": [], "c": []}
    for case in cases:
        p = {k: sum(1 for r in by[case][k] if r["passed"]) for k in "tc"}
        m = {k: len(by[case][k]) for k in "tc"}
        for k in "tc":
            tot[k] += p[k]
            n[k] += m[k]
        diffs.append(p["c"] - p["t"])
        med = {k: (statistics.median(r["tokens_in"] + r["tokens_out"] for r in by[case][k]) if by[case][k] else 0) for k in "tc"}
        pct = 100.0 * (med["c"] - med["t"]) / med["t"] if med["t"] else 0.0
        tokdirs.append(0 if abs(pct) < TOKEN_PCT else (1 if pct > 0 else -1))
        rc = {}
        for k in "tc":
            vals = [int(RECALL.search(x).group(1)) for r in by[case][k] for x in r["notes"] if RECALL.search(x)]
            recall[k] += vals
            rc[k] = statistics.mean(vals) if vals else float("nan")
        print(f"{case:26} {p['t']:>3}/{m['t']:<4} {p['c']:>4}/{m['c']:<4} {p['c'] - p['t']:>+4}   {med['t']:>10.0f} -> {med['c']:<10.0f}   {rc['t']:>5.0f} -> {rc['c']:<5.0f}")
    print(f"{'total':26} {tot['t']:>3}/{n['t']:<4} {tot['c']:>4}/{n['c']:<4} {tot['c'] - tot['t']:>+4}")
    if recall["t"] and recall["c"]:
        print(f"mean fact recall: terse {statistics.mean(recall['t']):.0f}%, contract {statistics.mean(recall['c']):.0f}%")
    if excluded:
        print(f"\nexcluded from grading: {len(excluded)} run(s)")
        for r in excluded:
            print(f"  {r['case']} seed {r['seed']}: passed={r['passed']} error={r.get('error')!r} notes={r['notes'][:1]}")

    print("\nPRE-REGISTERED RULE")
    if len(cases) != 5 or any(len(by[c][k]) != 3 for c in cases for k in "tc"):
        print("INCOMPLETE: not every one of the 5 cases has 3 graded runs per variant. Re-run the excluded runs (--resume) before deciding.")
        return 1
    d = tot["c"] - tot["t"]
    same = sum(1 for x in diffs if x != 0 and (x > 0) == (d > 0))
    print(f"total difference (contract - terse) = {d:+d} passes of 15; cases pointing the same way as the total: {same} of 5")
    effect = abs(d) >= MIN_DIFF and same >= MIN_AGREE
    if tot["t"] >= CEILING and d > 0:
        print(f"CEILING: terse already passes {tot['t']} of 15, so an improvement of {MIN_DIFF} cannot be shown; only a decline is detectable.")
    if effect and d > 0:
        verdict = "EFFECT FAVOURS THE CONTRACT: the follow-ups in spec 082 section 8 may be drafted."
    elif effect and d < 0:
        verdict = "EFFECT AGAINST THE CONTRACT: reconsider S1c and S2 before anything further; build nothing from section 8."
    else:
        verdict = "NO EVIDENCE of an effect on passes (not evidence of no effect): build nothing from spec 082 section 8."
    print(verdict)
    ntok = sum(1 for x in tokdirs if x != 0)
    print(f"secondary, tokens (never decides): {sum(1 for x in tokdirs if x > 0)} cases up, {sum(1 for x in tokdirs if x < 0)} down, {5 - ntok} within {TOKEN_PCT}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
