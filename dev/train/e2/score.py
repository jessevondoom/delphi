#!/usr/bin/env python3
"""E2 scoring harness (spec 086 S5). Mechanical: no model judges anything. Definitions are in PREREG.md.

  python3 -I score.py score  RECORD_A RECORD_B           the three measures for one pair, JSON
  python3 -I score.py analyze MANIFEST.json              the pre-registered decision rule over all questions, JSON
  python3 -I score.py extract-question RECORD            the question text of a source record (stdout)
  python3 -I score.py question-check RECORD QUESTIONFILE does the record's proposition contain the question verbatim
  python3 -I score.py tokens EVENTS_DIR... [--task-id T] sums agent.usage tokens (input + output + thinking)

Nothing here runs a model, a host or the daemon. It reads files.
"""
import json
import re
import sys
from fractions import Fraction
from pathlib import Path

# Pinned by PREREG.md ("Constants"); test_score.py checks they are the same.
MATCH_JACCARD = 0.5
STOPWORDS = (
    "the and for are but not you all can had her was one our out has his how its may new now see two way who did get let put say she too "
    "use that this with from have been will into than then them they what when where which would should could there their about after "
    "before other these those also only more most must such each both over under while does done doing being were your"
).split()

H_STRUCT = "## Round 2 structured output"
H_VERDICT = "## Verdict"
H_DECISION = "## Decision and next steps"
H_MINORITY = "## Accepted Risks & Minority Report (Red-Team / Dissent)"
SEVS = ("P0", "P1", "P2", "P3")


def words(title):
    """Normal form of a title: a set of words (PREREG.md)."""
    out = set()
    for w in re.sub(r"[^a-z0-9]+", " ", title.lower()).split():
        if len(w) < 3 or w in STOPWORDS:
            continue
        if len(w) > 4 and w.endswith("s") and not w.endswith("ss"):
            w = w[:-1]
        out.add(w)
    return out


def jaccard(x, y):
    u = x | y
    return len(x & y) / len(u) if u else 0.0


def last_heading(lines, heading):
    """Index of the last line that IS the heading, starting at column 0; None if there is none."""
    hit = None
    for i, l in enumerate(lines):
        if l.rstrip() == heading:
            hit = i
    return hit


def section(lines, heading):
    """Lines of a section: after the last real heading to the next column-0 `## ` line. None if absent."""
    i = last_heading(lines, heading)
    if i is None:
        return None
    body = []
    for l in lines[i + 1:]:
        if l.startswith("## "):
            break
        body.append(l)
    return body


def front_matter(lines):
    fm = {}
    if not lines or lines[0].rstrip() != "---":
        return fm
    for l in lines[1:]:
        if l.rstrip() == "---":
            break
        if ":" in l:
            k, v = l.split(":", 1)
            fm[k.strip()] = v.strip().strip('"')
    return fm


def parse_record(path):
    text = Path(path).read_text(encoding="utf-8", errors="replace")
    lines = text.split("\n")
    fm = front_matter(lines)
    errs = []
    if fm.get("council_format") != "1":
        errs.append("not council_format: 1")
    blind = "unknown"
    m = re.search(r"blind:\s*(yes|no)\b", "\n".join(lines[:60]), re.I)
    if "blind" in fm and fm["blind"].lower() in ("yes", "no"):
        blind = fm["blind"].lower()
    elif m:
        blind = m.group(1).lower()
    recs, unreadable, raw = [], [], []
    st = section(lines, H_STRUCT)
    if st is None:
        errs.append("no Round 2 structured output section")
        st = []
    for l in st:
        if not l.startswith("- "):
            continue
        parts = l[2:].split(" | ", 2)
        if len(parts) != 3:
            continue
        seat, kind, rest = (p.strip() for p in parts)
        if kind == "recommendation":
            recs.append((seat, rest))
        elif kind == "unreadable":
            unreadable.append(seat)
        elif kind in SEVS:
            raw.append((kind, seat, rest))
    findings = cluster([(s, seat, t) for s, seat, t in raw if s in ("P0", "P1")])
    cls, alts = decision_class(recs)
    verdict = section(lines, H_VERDICT) or []
    dec = section(lines, H_DECISION) or []
    dtext = next((l[len("**Decision:**"):].strip() for l in dec if l.startswith("**Decision:**")), None)
    return {
        "path": str(path),
        "venue": fm.get("venue"),
        "barrier": fm.get("barrier"),
        "blind": blind,
        "parse_errors": errs,
        "recommendations": recs,
        "unreadable_seats": unreadable,
        "decision_class": cls,
        "alt_texts": alts,
        "escalated": any(l.startswith("**Escalated to the operator:**") for l in verdict),
        "decision_text": dtext,
        "findings": findings,
        "minority_present": last_heading(lines, H_MINORITY) is not None,
    }


def cluster(raw):
    """Merge findings whose title words reach MATCH_JACCARD with an earlier cluster's first member."""
    out = []
    for sev, seat, title in raw:
        w = words(title)
        for c in out:
            if jaccard(w, c["words"]) >= MATCH_JACCARD:
                c["seats"].append(seat)
                if sev == "P0":
                    c["sev"] = "P0"
                break
        else:
            out.append({"sev": sev, "seats": [seat], "title": title, "words": w})
    return out


def decision_class(recs):
    classes, alts = [], []
    for _, label in recs:
        if label.startswith("ALTERNATIVE"):
            classes.append("ALT")
            alts.append(label.split(":", 1)[1].strip() if ":" in label else "")
        elif label.startswith("DO-NOT-PROCEED"):
            classes.append("STOP")
        elif label.startswith("PROCEED"):
            classes.append("GO")
    if not classes:
        return "NONE", alts
    counts = {c: classes.count(c) for c in set(classes)}
    top = max(counts.values())
    best = [c for c, n in counts.items() if n == top]
    return (best[0] if len(best) == 1 else "SPLIT"), alts


def same_decision(a, b):
    ca, cb = a["decision_class"], b["decision_class"]
    if ca in ("SPLIT", "NONE") or cb in ("SPLIT", "NONE"):
        return False
    if ca != cb:
        return False
    if ca == "ALT":
        return any(jaccard(words(x), words(y)) >= MATCH_JACCARD for x in a["alt_texts"] for y in b["alt_texts"])
    return True


def match(fa, fb):
    """Greedy one-to-one matching; returns list of (i, j)."""
    pairs = []
    for i, x in enumerate(fa):
        for j, y in enumerate(fb):
            s = jaccard(x["words"], y["words"])
            if s >= MATCH_JACCARD:
                pairs.append((-s, i, j))
    pairs.sort()
    used_a, used_b, out = set(), set(), []
    for _, i, j in pairs:
        if i not in used_a and j not in used_b:
            used_a.add(i)
            used_b.add(j)
            out.append((i, j))
    return out


def degraded(r, panel=False):
    why = []
    if r["parse_errors"]:
        why.append("parse: " + "; ".join(r["parse_errors"]))
    if r["unreadable_seats"]:
        why.append("unreadable: " + ",".join(r["unreadable_seats"]))
    if panel and r["blind"] != "yes":
        why.append("blind label is %s, not yes" % r["blind"])
    return why


def compare(a, b):
    """The three measures for records a and b (a = the first named, b = the second)."""
    fa, fb = a["findings"], b["findings"]
    pairs = match(fa, fb)
    ma = {i for i, _ in pairs}
    mb = {j for _, j in pairs}
    m = len(pairs)
    union = len(fa) + len(fb) - m
    return {
        "same_decision": same_decision(a, b),
        "decision_class": [a["decision_class"], b["decision_class"]],
        "same_plurality_label_exact": a["decision_class"] == b["decision_class"] and a["alt_texts"] == b["alt_texts"],
        "escalated": [a["escalated"], b["escalated"]],
        "n_a": len(fa),
        "n_b": len(fb),
        "matched": m,
        "union": union,
        "overlap": (m / union) if union else 1.0,
        "both_empty": union == 0,
        "severity_mismatches": sum(1 for i, j in pairs if fa[i]["sev"] != fb[j]["sev"]),
        "b_lacked": len(fa) - m,
        "a_lacked": len(fb) - m,
        "b_lacked_p0": sum(1 for i, f in enumerate(fa) if i not in ma and f["sev"] == "P0"),
        "a_lacked_p0": sum(1 for j, f in enumerate(fb) if j not in mb and f["sev"] == "P0"),
        "a_only": [fa[i]["title"] for i in range(len(fa)) if i not in ma],
        "b_only": [fb[j]["title"] for j in range(len(fb)) if j not in mb],
    }


def score(path_a, path_b, panel_a=False):
    a, b = parse_record(path_a), parse_record(path_b)
    out = compare(a, b)
    out["a"] = {k: a[k] for k in ("path", "venue", "barrier", "blind", "decision_text")}
    out["b"] = {k: b[k] for k in ("path", "venue", "barrier", "blind", "decision_text")}
    out["degraded"] = {"a": degraded(a, panel_a), "b": degraded(b)}
    out["is_degraded"] = bool(out["degraded"]["a"] or out["degraded"]["b"])
    return out


def venue_check(rec, arm):
    """The arm comes from where the harness put the record; the record's own label is only a cross-check."""
    v = rec["venue"]
    if v is None:
        return "venue_missing"
    expect = "panel" if arm == "panel" else "daemon"
    if v != expect:
        return "venue_mismatch"
    return "ok"


def analyze(manifest_path):
    man = json.loads(Path(manifest_path).read_text())
    host = man["headline"]
    rows, excluded = [], []
    for q in man["questions"]:
        try:
            b1, b2 = parse_record(q["b1"]), parse_record(q["b2"])
            p = parse_record(q["panel"][host])
        except (KeyError, OSError) as e:
            excluded.append({"id": q["id"], "why": "missing record: %s" % e})
            continue
        why = degraded(p, True) + ["b1 " + w for w in degraded(b1)] + ["b2 " + w for w in degraded(b2)]
        checks = {"panel": venue_check(p, "panel"), "b1": venue_check(b1, "council"), "b2": venue_check(b2, "council")}
        if why:
            excluded.append({"id": q["id"], "why": "; ".join(why), "venue_checks": checks})
            continue
        rows.append({"id": q["id"], "venue_checks": checks, "p_b1": compare(p, b1), "b2_b1": compare(b2, b1), "p": p, "b1": b1})
    q = len(rows)
    pooled = lambda key, num, den: (Fraction(sum(r[key][num] for r in rows), sum(r[key][den] for r in rows)) if sum(r[key][den] for r in rows) else None)
    sd_p = sum(1 for r in rows if r["p_b1"]["same_decision"])
    sd_b = sum(1 for r in rows if r["b2_b1"]["same_decision"])
    ov_p, ov_b = pooled("p_b1", "matched", "union"), pooled("b2_b1", "matched", "union")
    n_ref = sum(len(r["b1"]["findings"]) for r in rows)
    rc_p = Fraction(sum(r["p_b1"]["matched"] for r in rows), n_ref) if n_ref else None
    rc_b = Fraction(sum(r["b2_b1"]["matched"] for r in rows), n_ref) if n_ref else None
    # a_lacked_p0 in compare(p, b1): P0 findings of b1 that the Panel lacks
    p0_missed = sum(1 for r in rows if r["p_b1"]["a_lacked_p0"] > 0)
    res = {"headline_host": host, "qualifying": q, "excluded": excluded}
    if q < 6 or n_ref == 0:
        verdict = "INCOMPLETE"
        res["why"] = "fewer than 6 qualifying questions" if q < 6 else "the reference has no P0/P1 findings"
    else:
        T = Fraction
        worse = sd_p <= sd_b - 3 or ov_p <= ov_b - T(1, 4) or rc_p <= rc_b - T(1, 4) or p0_missed >= 3
        supported = (sd_p >= q - 1 and sd_p >= sd_b - 1 and ov_p >= T(3, 5) and ov_p >= ov_b - T(1, 10)
                     and rc_p >= T(3, 5) and rc_p >= rc_b - T(1, 10) and p0_missed <= 1)
        verdict = "WORSE" if worse else ("SUPPORTED" if supported else "NO EVIDENCE")
    f = lambda x: None if x is None else round(float(x), 4)
    res.update({
        "verdict": verdict,
        "sd_panel": sd_p, "sd_council_repeat": sd_b,
        "overlap_panel": f(ov_p), "overlap_council_repeat": f(ov_b),
        "recall_panel": f(rc_p), "recall_council_repeat": f(rc_b),
        "p0_missed_by_panel": p0_missed,
        "reference_findings": n_ref,
        "panel_added_total": sum(r["p_b1"]["b_lacked"] for r in rows),
        "council_repeat_added_total": sum(r["b2_b1"]["b_lacked"] for r in rows),
        "per_question": [{"id": r["id"], "venue_checks": r["venue_checks"], "panel_vs_b1": {k: v for k, v in r["p_b1"].items()},
                          "b2_vs_b1": {k: v for k, v in r["b2_b1"].items()}} for r in rows],
    })
    return res


def extract_question(path):
    lines = Path(path).read_text(encoding="utf-8", errors="replace").split("\n")
    begin = next((i for i, l in enumerate(lines) if l.strip() == "--- BEGIN HOST QUESTION ---"), None)
    if begin is not None:
        end = next((i for i in range(begin + 1, len(lines)) if lines[i].strip() == "--- END HOST QUESTION ---"), None)
        if end is not None:
            return "\n".join(lines[begin + 1:end]).strip()
    start = next((i for i, l in enumerate(lines) if l.startswith("**Proposition:**")), None)
    if start is None:
        raise SystemExit("no proposition in %s" % path)
    end = next((i for i in range(start + 1, len(lines)) if lines[i].rstrip() == "## Seats"), len(lines))
    first = lines[start][len("**Proposition:**"):].strip()
    return "\n".join([first] + lines[start + 1:end]).strip()


def question_check(record, qfile):
    lines = Path(record).read_text(encoding="utf-8", errors="replace").split("\n")
    end = last_heading(lines, "## Seats")
    head = " ".join(" ".join(lines[: end if end is not None else len(lines)]).split())
    q = " ".join(Path(qfile).read_text(encoding="utf-8").split())
    return {"record": str(record), "question_file": str(qfile), "contains_question_verbatim": bool(q) and q in head}


def tokens(dirs, task_id=None):
    by = {}
    for d in dirs:
        for f in sorted(Path(d).glob("*.jsonl")):
            for line in f.read_text(encoding="utf-8", errors="replace").splitlines():
                try:
                    e = json.loads(line)
                except ValueError:
                    continue
                if e.get("type") != "agent.usage":
                    continue
                t = e.get("task_id")
                if task_id and t != task_id:
                    continue
                p = e.get("payload", {})
                r = by.setdefault(t, {"tokens": 0, "cache_read": 0, "cache_write": 0, "events": 0, "models": set()})
                r["tokens"] += int(p.get("input_tokens", 0)) + int(p.get("output_tokens", 0)) + int(p.get("thinking_tokens", 0))
                r["cache_read"] += int(p.get("cache_read_tokens", 0))
                r["cache_write"] += int(p.get("cache_write_tokens", 0))
                r["events"] += 1
                if e.get("model_id"):
                    r["models"].add(e["model_id"])
    return {str(t): {**v, "models": sorted(v["models"])} for t, v in by.items()}


def main(argv):
    if len(argv) >= 3 and argv[1] == "score":
        print(json.dumps(score(argv[2], argv[3], panel_a="--panel-a" in argv), indent=2, sort_keys=True))
    elif len(argv) == 3 and argv[1] == "analyze":
        print(json.dumps(analyze(argv[2]), indent=2, sort_keys=True, default=str))
    elif len(argv) == 3 and argv[1] == "extract-question":
        print(extract_question(argv[2]))
    elif len(argv) == 4 and argv[1] == "question-check":
        print(json.dumps(question_check(argv[2], argv[3]), indent=2))
    elif len(argv) >= 3 and argv[1] == "tokens":
        rest, tid = argv[2:], None
        if "--task-id" in rest:
            k = rest.index("--task-id")
            tid = rest[k + 1]
            rest = rest[:k] + rest[k + 2:]
        print(json.dumps(tokens(rest, tid), indent=2, sort_keys=True))
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
