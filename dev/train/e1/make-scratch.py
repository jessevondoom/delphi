#!/usr/bin/env python3
"""Build the scratch training repository for experiment E1 (specs/082 section 7).

    python3 -I dev/train/e1/make-scratch.py --repo <delphi checkout> --out <new directory>

It copies the five research cases twice into `<out>/dev/train/suites/e1/cases/`: `<id>-t` (the prompt exactly as shipped) and
`<id>-c` (the same prompt followed by the fixed contract block below). Truth packs are copied byte for byte, so their
verification stamps still hold. Nothing under the real repository is changed. The block is the same text for every case; it
is part of the pre-registration (dev/train/e1/PREREG.md), and changing it after a run starts voids the experiment.
"""
import argparse
import json
import re
import shutil
import sys
from pathlib import Path

BLOCK = (
    "Deliverable: one written answer to the request above.\n"
    "Done when: every part of the request is answered, or marked as something you could not answer.\n"
    "Stop when: the request depends on something you cannot find or verify. Say what you could not verify and stop there, "
    "instead of filling the gap.\n"
    "Scope: the request above only.\n"
    "Tools: use only the tools you have in this session, and say so if one you needed was missing."
)


def prompt_of(case_toml: str) -> str:
    """The `prompt = "..."` line of a case. A TOML basic string and a JSON string share the escapes these prompts use."""
    m = re.search(r"^prompt = (\".*\")$", case_toml, flags=re.M)
    if not m:
        raise SystemExit("a case has no single-line prompt")
    return json.loads(m.group(1))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    a = ap.parse_args()
    src = a.repo / "dev/train/suites/research/cases"
    if not src.is_dir():
        print(f"no research cases at {src}", file=sys.stderr)
        return 2
    if a.out.exists() and any(a.out.iterdir()):
        print(f"{a.out} is not empty; give a new directory", file=sys.stderr)
        return 2
    dst = a.out / "dev/train/suites/e1/cases"
    n = 0
    for case in sorted(p for p in src.iterdir() if (p / "case.toml").is_file()):
        text = (case / "case.toml").read_text(encoding="utf-8")
        prompt = prompt_of(text)
        for tag in ("t", "c"):
            out = dst / f"{case.name}-{tag}"
            shutil.copytree(case, out)
            t = re.sub(r'^id = ".*"$', f'id = "{case.name}-{tag}"', text, count=1, flags=re.M)
            t = re.sub(r'^suite = ".*"$', 'suite = "e1"', t, count=1, flags=re.M)
            if tag == "c":
                new = prompt.rstrip() + "\n\n" + BLOCK
                t, k = re.subn(r"^prompt = .*$", lambda _: "prompt = " + json.dumps(new, ensure_ascii=False), t, count=1, flags=re.M)
                if k != 1:
                    print(f"{case.name}: no single-line prompt to extend", file=sys.stderr)
                    return 2
            if prompt_of(t) != (prompt if tag == "t" else new):
                print(f"{case.name}-{tag}: prompt did not survive the rewrite", file=sys.stderr)
                return 2
            (out / "case.toml").write_text(t, encoding="utf-8")
            n += 1
    print(f"wrote {n} cases under {dst}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
