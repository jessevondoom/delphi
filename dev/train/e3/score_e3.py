#!/usr/bin/env python3
"""E3 scoring harness (spec 089 P6). Mechanical: no model judges anything. Definitions are in PREREG.md.

  python3 -I score_e3.py analyze MANIFEST.json        the pre-registered decision rule, JSON (run once, after every run)
  python3 -I score_e3.py budget TOKENS.csv            token totals against the caps, JSON
  python3 -I score_e3.py proposition RECORD           the proposition paragraph of a source record (stdout)
  python3 -I score_e3.py make-question RECORD         the question file built from a source record (stdout)
  python3 -I score_e3.py baseline-e2                  E2's mechanical numbers on the same measures, JSON
  python3 -I score_e3.py tokens EVENTS_DIR... [--task-id T]   sums agent.usage tokens (same code as E2)

Parsing, the decision class and the same-decision test are imported from dev/train/e2/score.py (v1, unchanged).
Finding overlap and recall are NOT measured in E3 (matcher v2 failed validation, spec 089 P5).
Nothing here runs a model, a host or the daemon. It reads files.
"""
import csv
import json
import re
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "e2"))
import score  # noqa: E402  (E2's scorer; its constants are pinned by E2's own test)

# Pinned by PREREG.md ("Constants"); test_score_e3.py checks they are the same.
MIN_QUALIFYING = 5           # questions per arm that must qualify
MIN_NOISE_QUESTIONS = 3      # questions with a usable B2 that must qualify per arm
SD_ABSOLUTE_SLACK = 2        # SUPPORTED: sd_arm >= q - 2
SD_RELATIVE_OK = 1           # SUPPORTED: sd_arm_s >= sd_b2_s - 1
SD_RELATIVE_WORSE = 2        # WORSE: sd_arm_s <= sd_b2_s - 2
READABLE_WORSE = Fraction(1, 4)   # WORSE: readable_arm <= readable_council - 1/4
ESCALATION_OK = Fraction(1, 4)    # SUPPORTED: |esc_arm - esc_council| <= 1/4
ESCALATION_WORSE = Fraction(1, 2)  # WORSE: esc_arm - esc_council >= 1/2
MISSED_OK = 1                # SUPPORTED: questions where B1 raised a P0/P1 and the arm did not <= 1
MISSED_WORSE = 3             # WORSE: >= 3
CAP_PER_RUN = 350_000        # tokens, each run's own side
CAP_DELPHI_TOTAL = 1_500_000  # daemon Councils + the foreign seat
CAP_HOST_TOTAL = 400_000     # Panel host tokens (host account)
ARMS = ("a1", "a2")


# ---------------------------------------------------------------- question extraction
def proposition(path):
    """The paragraph that follows `**Proposition:**` (up to the first blank line)."""
    lines = Path(path).read_text(encoding="utf-8", errors="replace").split("\n")
    start = next((i for i, l in enumerate(lines) if l.startswith("**Proposition:**")), None)
    if start is None:
        raise SystemExit("no proposition in %s" % path)
    out = [lines[start][len("**Proposition:**"):].strip()]
    for l in lines[start + 1:]:
        if not l.strip():
            break
        out.append(l.rstrip())
    return "\n".join(out).strip()


def under_review(path):
    text = Path(path).read_text(encoding="utf-8", errors="replace")
    m = re.search(r"\*\*Under review:\*\* \[`([^`]+)`\]", text)
    return m.group(1) if m else None


def make_question(path):
    """A question file in the layout of the host-raised E2 questions: Question, Stakes, Pointers."""
    ptr = under_review(path)
    body = "Question: %s\nStakes: normal\nPointers:\n" % proposition(path)
    body += ("- %s: the spec the question is about\n" % ptr) if ptr else "(none)\n"
    return body


# ---------------------------------------------------------------- per-record measures
def usable(rec, panel=False):
    """Parsed as a council_format 1 record with a Round 2 section, and (Panel) labelled blind: yes.
    An unreadable seat does NOT make a record unusable in E3: it is the readable-critique measure."""
    if rec["parse_errors"]:
        return False
    return not (panel and rec["blind"] != "yes")


def run_measures(rec):
    readable = {s for s, _ in rec["recommendations"]}
    unreadable = set(rec["unreadable_seats"]) - readable
    return {
        "decision_class": rec["decision_class"],
        "readable_seats": len(readable),
        "unreadable_seats": len(unreadable),
        "escalated": bool(rec["escalated"]),
        "p0p1_present": len(rec["findings"]) > 0,
    }


def rate(num, den):
    return Fraction(num, den) if den else None


def pooled(measures, key_num, key_den=None):
    """Pooled readable rate or escalation rate over a list of run_measures."""
    if key_num == "readable":
        r = sum(m["readable_seats"] for m in measures)
        u = sum(m["unreadable_seats"] for m in measures)
        return rate(r, r + u)
    return rate(sum(1 for m in measures if m["escalated"]), len(measures))


# ---------------------------------------------------------------- analysis
def load(entry, panel):
    """entry: a path string, or {"path":..., "status": "ok"|"capped"|"excluded"|..., "why":...}. Returns (record|None, why)."""
    if entry is None:
        return None, "not run"
    if isinstance(entry, dict):
        if entry.get("status", "ok") != "ok":
            return None, "%s: %s" % (entry["status"], entry.get("why", ""))
        entry = entry.get("path")
    if not entry:
        return None, "not run"
    try:
        rec = score.parse_record(entry)
    except OSError as e:
        return None, "missing record: %s" % e
    if not usable(rec, panel):
        why = "; ".join(rec["parse_errors"]) if rec["parse_errors"] else "blind label is %s, not yes" % rec["blind"]
        return None, "degraded: " + why
    return rec, None


def judge(q, s, sd, sd_s, sd_b2_s, read_x, read_c, esc_x, esc_c, missed):
    """The decision rule. Returns (verdict, reason)."""
    if q < MIN_QUALIFYING:
        return "INCOMPLETE", "fewer than %d qualifying questions" % MIN_QUALIFYING
    if s < MIN_NOISE_QUESTIONS:
        return "INCOMPLETE", "fewer than %d qualifying questions with a usable repeat Council (B2)" % MIN_NOISE_QUESTIONS
    if read_x is None or read_c is None or esc_x is None or esc_c is None:
        return "INCOMPLETE", "a rate has no denominator"
    worse = (
        sd_s <= sd_b2_s - SD_RELATIVE_WORSE
        or read_x <= read_c - READABLE_WORSE
        or esc_x - esc_c >= ESCALATION_WORSE
        or missed >= MISSED_WORSE
    )
    supported = (
        sd >= q - SD_ABSOLUTE_SLACK
        and sd_s >= sd_b2_s - SD_RELATIVE_OK
        and read_x >= read_c
        and abs(esc_x - esc_c) <= ESCALATION_OK
        and missed <= MISSED_OK
    )
    return ("WORSE" if worse else ("SUPPORTED" if supported else "NO EVIDENCE")), "rule applied"


def fl(x):
    return None if x is None else round(float(x), 4)


def analyze(manifest_path):
    man = json.loads(Path(manifest_path).read_text())
    qs = man["questions"]
    excluded, b1s, b2s = [], {}, {}
    for q in qs:
        b1, why = load(q.get("b1"), False)
        if b1 is None:
            excluded.append({"id": q["id"], "run": "b1", "why": why})
        elif b1["decision_class"] == "NONE":
            excluded.append({"id": q["id"], "run": "b1", "why": "reference has no readable recommendation (decision class NONE)"})
        else:
            b1s[q["id"]] = b1
        b2, why = load(q.get("b2"), False)
        if b2 is None:
            if q.get("b2") is not None:
                excluded.append({"id": q["id"], "run": "b2", "why": why})
        else:
            b2s[q["id"]] = b2
    council_measures = [run_measures(r) for r in list(b1s.values()) + list(b2s.values())]
    read_c, esc_c = pooled(council_measures, "readable"), pooled(council_measures, "esc")
    out = {"council_readable_rate": fl(read_c), "council_escalation_rate": fl(esc_c), "council_runs": len(council_measures),
           "excluded_runs": excluded, "arms": {}}
    for arm in ARMS:
        rows, ex = [], []
        for q in qs:
            x, why = load(q.get(arm), True)
            if x is None:
                ex.append({"id": q["id"], "why": why})
            elif q["id"] not in b1s:
                ex.append({"id": q["id"], "why": "no usable reference (B1)"})
            else:
                rows.append((q["id"], x))
        sd = sd_s = sd_b2_s = s = missed = added = 0
        per = []
        for qid, x in rows:
            b1 = b1s[qid]
            same = score.same_decision(x, b1)
            sd += same
            has_b2 = qid in b2s
            same_b2 = score.same_decision(b2s[qid], b1) if has_b2 else None
            if has_b2:
                s += 1
                sd_s += same
                sd_b2_s += same_b2
            mx, mb = run_measures(x), run_measures(b1)
            missed += mb["p0p1_present"] and not mx["p0p1_present"]
            added += mx["p0p1_present"] and not mb["p0p1_present"]
            per.append({"id": qid, "arm_class": x["decision_class"], "b1_class": b1["decision_class"],
                        "b2_class": b2s[qid]["decision_class"] if has_b2 else None, "same_as_b1": same, "b2_same_as_b1": same_b2,
                        "arm": mx, "b1": mb})
        q = len(rows)
        arm_m = [run_measures(x) for _, x in rows]
        read_x, esc_x = pooled(arm_m, "readable"), pooled(arm_m, "esc")
        verdict, why = judge(q, s, sd, sd_s, sd_b2_s, read_x, read_c, esc_x, esc_c, missed)
        out["arms"][arm] = {
            "verdict": verdict, "why": why, "qualifying": q, "with_repeat_council": s, "excluded": ex,
            "same_decision": sd, "same_decision_on_repeat_subset": sd_s, "council_repeat_same_decision_on_repeat_subset": sd_b2_s,
            "readable_rate": fl(read_x), "escalation_rate": fl(esc_x),
            "p0p1_missed_vs_b1": missed, "p0p1_added_vs_b1": added, "per_question": per,
        }
    out["headline_arm"] = "a1"
    out["verdict"] = out["arms"]["a1"]["verdict"]
    return out


# ---------------------------------------------------------------- tokens and budget
def budget(csv_path):
    """CSV columns: question,run,side,tokens. side is `delphi` (daemon Council or the foreign seat) or `host` (Panel host account)."""
    delphi = host = 0
    per_run, over = {}, []
    with open(csv_path, newline="", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            t = int(row["tokens"])
            side = row["side"].strip()
            if side not in ("delphi", "host"):
                raise SystemExit("side must be delphi or host: %r" % side)
            key = (row["question"], row["run"], side)
            per_run[key] = per_run.get(key, 0) + t
            if side == "delphi":
                delphi += t
            else:
                host += t
    for (qid, run, side), t in sorted(per_run.items()):
        if t > CAP_PER_RUN:
            over.append({"question": qid, "run": run, "side": side, "tokens": t})
    return {
        "delphi_tokens": delphi, "delphi_cap": CAP_DELPHI_TOTAL, "delphi_over_cap": delphi > CAP_DELPHI_TOTAL,
        "host_tokens": host, "host_cap": CAP_HOST_TOTAL, "host_over_cap": host > CAP_HOST_TOTAL,
        "per_run_cap": CAP_PER_RUN, "runs_over_per_run_cap": over,
        "largest_run": max(per_run.values()) if per_run else 0,
    }


# ---------------------------------------------------------------- E2 baseline (same measures, E2's final records)
def baseline_e2():
    d = HERE.parent / "e2" / "data" / "records"
    names = {"panel": "-panel-claude-code", "b1": "-b1", "b2": "-b2"}
    out = {}
    for arm, suffix in names.items():
        ms = [run_measures(score.parse_record(d / ("q%d%s.md" % (i, suffix)))) for i in range(1, 9)]
        out[arm] = {"records": len(ms), "readable_rate": fl(pooled(ms, "readable")), "escalation_rate": fl(pooled(ms, "esc")),
                    "p0p1_present": sum(1 for m in ms if m["p0p1_present"])}
    council = [run_measures(score.parse_record(d / ("q%d%s.md" % (i, s)))) for i in range(1, 9) for s in ("-b1", "-b2")]
    out["council_pooled"] = {"records": 16, "readable_rate": fl(pooled(council, "readable")), "escalation_rate": fl(pooled(council, "esc"))}
    return out


def main(argv):
    if len(argv) == 3 and argv[1] == "analyze":
        print(json.dumps(analyze(argv[2]), indent=2, sort_keys=True, default=str))
    elif len(argv) == 3 and argv[1] == "budget":
        print(json.dumps(budget(argv[2]), indent=2, sort_keys=True))
    elif len(argv) == 3 and argv[1] == "proposition":
        print(proposition(argv[2]))
    elif len(argv) == 3 and argv[1] == "make-question":
        sys.stdout.write(make_question(argv[2]))
    elif len(argv) == 2 and argv[1] == "baseline-e2":
        print(json.dumps(baseline_e2(), indent=2, sort_keys=True))
    elif len(argv) >= 3 and argv[1] == "tokens":
        rest, tid = argv[2:], None
        if "--task-id" in rest:
            k = rest.index("--task-id")
            tid = rest[k + 1]
            rest = rest[:k] + rest[k + 2:]
        print(json.dumps(score.tokens(rest, tid), indent=2, sort_keys=True))
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
