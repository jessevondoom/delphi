#!/usr/bin/env python3
"""Exploratory, post hoc re-score of E2 with matcher v2 (spec 089 section 7, slice P5). Runs no model.

  python3 -I dev/train/e2/rescore.py            writes dev/train/e2/data/rescore.json and dev/train/e2/RESCORE.md

Findings are parsed with v1's parser (score.parse_record); only the matching differs. The pre-registered verdict
(INCOMPLETE) comes from score.py and is not touched.
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import matcher_v2 as m2  # noqa: E402
import score  # noqa: E402

RECORDS = HERE / "data" / "records"
FAIL_AGREEMENT_BELOW = 0.3   # spec 089 task: v2 must match at least this share of B1-B2 findings
FAIL_FALSE_MATCH_ABOVE = 0.1  # ... and no more than this share across different questions


def local(path):
    """The manifest holds the original /tmp paths; the records were saved under data/records."""
    return RECORDS / Path(path).name


def titles(rec):
    return [f["title"] for f in rec["findings"]]


def pair(a, b):
    ta, tb = titles(a), titles(b)
    pairs = m2.match(ta, tb)
    return {"n_a": len(ta), "n_b": len(tb), "matched": len(pairs),
            "pairs": [[ta[i], tb[j]] for i, j in pairs]}


def share(rows):
    tot = sum(r["n_a"] + r["n_b"] for r in rows)
    return (2 * sum(r["matched"] for r in rows) / tot) if tot else None


def rnd(x):
    return None if x is None else round(x, 4)


def main():
    man = json.loads((HERE / "data" / "manifest.json").read_text())
    host = man["headline"]
    recs = {}
    for q in man["questions"]:
        qid = q["id"]
        recs[qid] = {"b1": score.parse_record(local(q["b1"])), "b2": score.parse_record(local(q["b2"])),
                     "panel": score.parse_record(local(q["panel"][host]))}

    # (a) agreement: B1 vs B2, same question; both records parse
    agree = []
    for qid, r in recs.items():
        if r["b1"]["parse_errors"] or r["b2"]["parse_errors"]:
            continue
        agree.append({"id": qid, **pair(r["b1"], r["b2"])})
    # (b) negative control: B1 of i vs B1 of j, i < j (match is symmetric up to ties)
    ids = [i for i, r in recs.items() if not r["b1"]["parse_errors"]]
    neg = []
    for x, a in enumerate(ids):
        for b in ids[x + 1:]:
            neg.append({"a": a, "b": b, **pair(recs[a]["b1"], recs[b]["b1"])})
    ag, fm = share(agree), share(neg)
    failed = []
    if ag is None or ag < FAIL_AGREEMENT_BELOW:
        failed.append("agreement %s is under %s" % (rnd(ag), FAIL_AGREEMENT_BELOW))
    if fm is None or fm > FAIL_FALSE_MATCH_ABOVE:
        failed.append("false-match rate %s is above %s" % (rnd(fm), FAIL_FALSE_MATCH_ABOVE))
    passed = not failed

    # (c) Panel vs B1 for the questions that qualified under the pre-registered rule (same exclusions as score.analyze)
    qual, per_q = [], []
    for qid, r in recs.items():
        why = score.degraded(r["panel"], True) + score.degraded(r["b1"]) + score.degraded(r["b2"])
        if why:
            continue
        qual.append(qid)
        pb, bb = pair(r["panel"], r["b1"]), pair(r["b2"], r["b1"])
        per_q.append({"id": qid, "panel_vs_b1": pb, "b2_vs_b1": bb})
    n_ref = sum(p["panel_vs_b1"]["n_b"] for p in per_q)

    def agg(key):
        mt = sum(p[key]["matched"] for p in per_q)
        na = sum(p[key]["n_a"] for p in per_q)
        union = na + n_ref - mt
        return {"matched": mt, "n_ref": n_ref, "n_other": na, "overlap": rnd(mt / union if union else None),
                "recall": rnd(mt / n_ref if n_ref else None), "added": na - mt}

    out = {
        "status": "exploratory, post hoc; does not alter the pre-registered verdict",
        "matcher": {"anchor_jaccard": m2.ANCHOR_JACCARD, "plain_jaccard": m2.PLAIN_JACCARD},
        "validation": {"passed": passed, "failures": failed,
                       "agreement_share": rnd(ag), "false_match_share": rnd(fm),
                       "thresholds": {"agreement_min": FAIL_AGREEMENT_BELOW, "false_match_max": FAIL_FALSE_MATCH_ABOVE},
                       "definition": "share = 2 * matched / (findings in A + findings in B), pooled over the pairs"},
        "agreement_b1_vs_b2": agree,
        "negative_control_b1_vs_b1": neg,
        "qualifying_questions": qual,
        "panel_vs_b1": agg("panel_vs_b1"),
        "b2_vs_b1": agg("b2_vs_b1"),
        "per_question": per_q,
    }
    (HERE / "data" / "rescore.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    (HERE / "RESCORE.md").write_text(render(out))
    print("validation %s: agreement=%s false_match=%s" % ("PASSED" if passed else "FAILED", rnd(ag), rnd(fm)))
    return 0


def render(o):
    v = o["validation"]
    L = []
    L.append("# E2 re-score with matcher v2 (generated; do not edit)\n")
    L.append("> **EXPLORATORY, POST HOC.** Matcher v2 was written after E2's data was seen. Its thresholds (anchor + Jaccard >= %s, or Jaccard >= %s) were fixed by spec 089 before use and were not tuned. "
             "This file does **not** replace or alter the pre-registered verdict, which stays **INCOMPLETE** (4 of 8 questions qualified, 6 needed). "
             "It only asks whether a future experiment could use this matcher.\n" % (o["matcher"]["anchor_jaccard"], o["matcher"]["plain_jaccard"]))
    if v["passed"]:
        L.append("> **Validation: PASSED.** Agreement (B1 vs B2, same question) is %s (needed at least %s); the false-match rate across different questions is %s (allowed at most %s).\n"
                 % (v["agreement_share"], v["thresholds"]["agreement_min"], v["false_match_share"], v["thresholds"]["false_match_max"]))
    else:
        L.append("> **Validation: FAILED. Matcher v2 is not usable and section (c) below is NOT informative.** " + "; ".join(v["failures"]) + ".\n")
    L.append("Share = 2 x matched / (findings in A + findings in B), pooled. Findings are the P0/P1 clusters parsed by v1 (`score.parse_record`).\n")
    L.append("## (a) Agreement: B1 vs B2 on the same question\n")
    L.append("| Question | B1 findings | B2 findings | Matched |\n| :-- | --: | --: | --: |")
    for r in o["agreement_b1_vs_b2"]:
        L.append("| %s | %d | %d | %d |" % (r["id"], r["n_a"], r["n_b"], r["matched"]))
    L.append("\nPooled share matched: **%s**\n" % v["agreement_share"])
    L.append("## (b) Negative control: B1 of question i vs B1 of question j (i != j)\n")
    hits = [r for r in o["negative_control_b1_vs_b1"] if r["matched"]]
    if hits:
        L.append("| Pair | Findings | Matched |\n| :-- | --: | --: |")
        for r in hits:
            L.append("| %s / %s | %d + %d | %d |" % (r["a"], r["b"], r["n_a"], r["n_b"], r["matched"]))
    else:
        L.append("No pair of different questions had a single match.")
    L.append("\n%d pairs of different questions; pairs with no match are omitted. Pooled false-match rate: **%s**\n" % (len(o["negative_control_b1_vs_b1"]), v["false_match_share"]))
    fp = [(r["a"], r["b"], p) for r in o["negative_control_b1_vs_b1"] for p in r["pairs"]]
    if fp:
        L.append("False matches, for inspection:\n")
        for a, b, p in fp:
            L.append("- %s / %s: \"%s\" ~ \"%s\"" % (a, b, p[0], p[1]))
        L.append("")
    L.append("## (c) Panel vs B1 under v2 (questions that qualified under the pre-registered rule: %s)\n" % ", ".join(o["qualifying_questions"]))
    if not v["passed"]:
        L.append("**Not informative: v2 failed validation.** The numbers are listed only so the failure can be audited.\n")
    L.append("| Arm vs B1 | Overlap | Recall of B1 findings | Added (not in B1) | Matched / B1 findings |\n| :-- | --: | --: | --: | --: |")
    for name, k in (("Panel", "panel_vs_b1"), ("B2 (Council repeat)", "b2_vs_b1")):
        a = o[k]
        L.append("| %s | %s | %s | %s | %d / %d |" % (name, a["overlap"], a["recall"], a["added"], a["matched"], a["n_ref"]))
    L.append("\nThe same-decision measure is unchanged: it comes from v1 (`data/analysis.json`). v1 matched 0 of 40 reference findings for both arms.\n")
    L.append("Per question matched (Panel vs B1 / B2 vs B1): " + ", ".join("%s %d/%d" % (p["id"], p["panel_vs_b1"]["matched"], p["b2_vs_b1"]["matched"]) for p in o["per_question"]) + "\n")
    L.append("Generated by `dev/train/e2/rescore.py` from `data/manifest.json` and `data/records/`; raw numbers in `data/rescore.json`.")
    return "\n".join(L) + "\n"


if __name__ == "__main__":
    sys.exit(main())
