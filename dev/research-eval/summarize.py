"""Spec 114 M7: one table from the driver's results and the meter's lines, before and after.

Usage: summarize.py <before dir> <before tokens.jsonl> <after dir> <after tokens.jsonl> [--json]
Per case: the state and why it stopped, who planned, sub-questions covered of planned, claims kept, claims stated by two or more
origins, the answer's label, web searches sent, pages read, model calls and tokens (from the meter, inside the case's time window).
"""
import glob, json, os, sys


def load_tokens(path):
    if not os.path.exists(path):
        return []
    return [json.loads(l) for l in open(path) if l.strip()]


def row(rec, tokens):
    r = rec.get("result") or {}
    claims = r.get("claims") or []
    subs = r.get("sub_questions") or []
    flow = (r.get("search_log") or {}).get("flow") or {}
    t0, t1 = rec.get("started_at", 0), rec.get("ended_at", 0)
    calls = [t for t in tokens if t0 <= t["at"] <= t1]
    answer = r.get("answer") or ""
    label = answer.strip().splitlines()[-1] if answer.strip() else ""
    return {
        "id": rec["case"]["id"],
        "state": r.get("status") or r.get("state") or ("refused" if rec["request_reply"].get("_is_error") else "none"),
        "stopped": r.get("stopped", ""),
        "plan": r.get("plan", "fixed (no plan call)"),
        "subs_planned": len(subs),
        "subs_covered": sum(1 for s in subs if s.get("covered")),
        "claims": len(claims),
        "claims_2_origins": sum(1 for c in claims if ((c.get("corroboration") or {}).get("origins") or 0) >= 2),
        "label": label[-60:],
        "searches": flow.get("searches", 0),
        "pages_read": len(r.get("source_ids") or []),
        "model_calls": len(calls),
        "tokens_in": sum(t.get("prompt_tokens") or 0 for t in calls),
        "tokens_out": sum(t.get("completion_tokens") or 0 for t in calls),
        "secs": rec.get("wall_secs"),
    }


def side(d, tok):
    tokens = load_tokens(tok)
    return [row(json.load(open(p)), tokens) for p in sorted(glob.glob(os.path.join(d, "q*.json")))]


def totals(rows):
    keys = ["subs_planned", "subs_covered", "claims", "claims_2_origins", "searches", "pages_read", "model_calls", "tokens_in", "tokens_out"]
    return {k: sum(r[k] for r in rows) for k in keys}


before, after = side(sys.argv[1], sys.argv[2]), side(sys.argv[3], sys.argv[4])
if "--json" in sys.argv:
    print(json.dumps({"before": before, "after": after, "before_total": totals(before), "after_total": totals(after)}, indent=1))
    sys.exit(0)
cols = ["id", "state", "stopped", "plan", "subs_covered", "subs_planned", "claims", "claims_2_origins", "searches", "pages_read", "model_calls", "tokens_in", "tokens_out", "secs"]
for name, rows in (("before", before), ("after", after)):
    print(f"\n{name}")
    print(" | ".join(cols))
    for r in rows:
        print(" | ".join(str(r[c]) for c in cols))
    print("total", json.dumps(totals(rows)))
