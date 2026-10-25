"""Honing tools for the built-in skills (skills/CATALOG.md).

usage:
  python3 -I dev/skill_trigger_eval.py audit
      Offline checks on the skill descriptions: shape, size, and pairs that read alike.
  python3 -I dev/skill_trigger_eval.py trigger <scratch dir> [--model ID] [--dry-run] [--role ROLE]
      Trigger test. An agent sees only its own role's skills (name and description, the discovery tier of
      specs/001 §3), so for each role one batched call asks a model which skill each task in
      each task in dev/skill-evals/*.json should load, or none (cases.json is direct phrasing; hard.json is indirect
      phrasing, sibling confusions and tasks no skill covers). Prints accuracy and every miss.

The trigger test spends a few small model calls through `agy` (read-only plan mode, run from the scratch dir so it
sees none of the repository). Use a cheap model; the descriptions, not the model, are what is being measured.
"""
import itertools, json, re, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MODEL = "gemini-3.8-flash-low"


def skills():
    out = {}
    for f in sorted((ROOT / "skills").glob("*/SKILL.md")):
        text = f.read_text()
        fm = text.split("\n---\n", 1)[0]
        name = re.search(r"^name: (.+)$", fm, re.M).group(1).strip()
        desc = json.loads(re.search(r"^description: (.+)$", fm, re.M).group(1))
        out[name] = {"description": desc, "bytes": len(text.encode()), "lines": text.count("\n")}
    return out


def role_skills():
    out = {}
    for f in sorted((ROOT / "agents").glob("*/AGENT.md")):
        text = f.read_text()
        m = re.search(r"^authorized_skills:\n((?:  - .+\n)+)", text, re.M)
        out[f.parent.name] = [l.strip()[2:] for l in m.group(1).splitlines()] if m else []
    return out


WORDS = re.compile(r"[a-z]{4,}")
STOP = {"with", "that", "this", "from", "when", "into", "uses", "used", "using", "before", "after", "their", "them", "each", "your"}


def audit():
    sk, bad = skills(), 0
    for n, s in sk.items():
        d = s["description"]
        problems = []
        if len(d) > 200:
            problems.append("over 200 characters")
        if not re.search(r"\b(Use|use)\b", d):
            problems.append("no trigger clause (\"Use when …\")")
        if s["lines"] > 500:
            problems.append("body over 500 lines")
        if problems:
            bad += 1
            print(f"[shape] {n}: {', '.join(problems)}")
    total = sum(len(n) + len(s["description"]) for n, s in sk.items()) // 4
    print(f"{len(sk)} skills; discovery tier about {total} tokens in all; largest bodies: " + ", ".join(f"{n} {s['bytes'] // 4}t" for n, s in sorted(sk.items(), key=lambda kv: -kv[1]["bytes"])[:3]))
    # Descriptions that read alike are where a model will pick the wrong skill. Compare only within a role.
    seen = set()
    for role, names in role_skills().items():
        for a, b in itertools.combinations([n for n in names if n in sk], 2):
            ta = {w for w in WORDS.findall(sk[a]["description"].lower()) if w not in STOP}
            tb = {w for w in WORDS.findall(sk[b]["description"].lower()) if w not in STOP}
            j = len(ta & tb) / max(1, len(ta | tb))
            if j >= 0.25 and (a, b) not in seen:
                seen.add((a, b))
                print(f"[similar] {role}: {a} ~ {b} ({j:.2f}): shared {sorted(ta & tb)}")
    # A skill an agent lists but the skill does not authorize (or the reverse) never loads.
    rs = role_skills()
    for n in sk:
        text = (ROOT / "skills" / n / "SKILL.md").read_text().split("\n---\n", 1)[0]
        agents = set(re.findall(r"^  - ([a-z-]+)$", text.split("authorized_agents:")[1].split("input_schema:")[0], re.M))
        listed = {r for r, names in rs.items() if n in names}
        if n != "tui-design" and agents != listed:
            bad += 1
            print(f"[roles] {n}: skill authorizes {sorted(agents)}, agents list it for {sorted(listed)}")
    print("audit: " + ("clean" if not bad else f"{bad} issue(s)"))
    return 1 if bad else 0


def ask(model, scratch, prompt):
    r = subprocess.run(["agy", "-p", prompt, "--mode", "plan", "--model", model], cwd=scratch, capture_output=True, text=True, timeout=600)
    m = re.search(r"\[.*\]", r.stdout, re.S)
    if not m:
        raise SystemExit(f"no JSON in reply:\n{r.stdout[-800:]}\n{r.stderr[-400:]}")
    return json.loads(m.group(0))


def trigger(scratch, model, dry, only):
    sk, rs = skills(), role_skills()
    cases = [c for f in sorted((ROOT / "dev/skill-evals").glob("*.json")) for c in json.load(open(f))]
    hits = total = 0
    for role, names in rs.items():
        if only and role != only:
            continue
        mine = [(i, c) for i, c in enumerate(cases) if c[0] == role]
        catalog = "\n".join(f"- {n}: {sk[n]['description']}" for n in names)
        tasks = "\n".join(f"{k + 1}. {c[1]}" for k, (_, c) in enumerate(mine))
        prompt = (
            f"You are the {role} agent in a multi-agent engineering team. You may load exactly one of these skills for a task, or none if no skill fits.\n"
            f"Skills:\n{catalog}\n\nFor each task, choose the single best skill by name, or \"none\".\n"
            f"Tasks:\n{tasks}\n\nReply with only a JSON array like [{{\"id\": 1, \"skill\": \"name-or-none\"}}], one entry per task. No other text."
        )
        if dry:
            print(prompt, "\n")
            continue
        got = {r["id"]: r["skill"] for r in ask(model, scratch, prompt)}
        for k, (_, c) in enumerate(mine):
            want, have = c[2] or "none", got.get(k + 1, "?")
            total += 1
            hits += want == have
            if want != have:
                print(f"[miss] {role}: wanted {want}, got {have}: {c[1]}")
    if not dry:
        print(f"trigger accuracy ({model}): {hits}/{total}")
        return 0 if hits == total else 1
    return 0


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[:1] == ["audit"]:
        sys.exit(audit())
    if a[:1] == ["trigger"] and len(a) >= 2:
        model = a[a.index("--model") + 1] if "--model" in a else DEFAULT_MODEL
        role = a[a.index("--role") + 1] if "--role" in a else None
        sys.exit(trigger(a[1], model, "--dry-run" in a, role))
    print(__doc__)
    sys.exit(2)
