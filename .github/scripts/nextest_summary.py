#!/usr/bin/env python3
"""Job summary of one nextest run (spec 125 amendment 125-A1, rule 2: a flaky pass is visible, never silent).

Reads the JUnit file of `cargo nextest run --profile ci` and writes, as Markdown, how many tests ran, failed and were
flaky, and every flaky test by name with the first line of its first failure. Each flaky test also gets a `::warning::`
line, so it shows among the run's annotations. Usage:

    nextest_summary.py <junit.xml> <title>

The Markdown goes to the file named by $GITHUB_STEP_SUMMARY (standard output when it is unset). A missing JUnit file
(the build failed before any test ran) is said plainly and is not an error of this script: the test step has already
failed the job.
"""

import os
import sys
import xml.etree.ElementTree as ET


def first_line(text):
    for line in (text or "").splitlines():
        line = line.strip()
        if line:
            return line[:200]
    return ""


def main(argv):
    if len(argv) != 3:
        print("usage: nextest_summary.py <junit.xml> <title>", file=sys.stderr)
        return 2
    path, title = argv[1], argv[2]
    out = []
    flaky = []
    failed = []
    total = 0
    if not os.path.exists(path):
        out.append(f"### {title}\n\nNo JUnit file at `{path}`: no test ran (see the test step's log).\n")
    else:
        root = ET.parse(path).getroot()
        for case in root.iter("testcase"):
            total += 1
            name = f"{case.get('classname', '')} {case.get('name', '')}".strip()
            flakes = case.findall("flakyFailure") + case.findall("flakyError")
            if case.find("failure") is not None or case.find("error") is not None:
                failed.append(name)
            elif flakes:
                msg = flakes[0].get("message") or first_line(flakes[0].findtext("system-err")) or first_line(flakes[0].findtext("system-out"))
                flaky.append((name, msg))
        out.append(f"### {title}\n")
        out.append(f"{total} tests ran; {len(failed)} failed after their retry; {len(flaky)} flaky (failed, then passed on the one retry).\n")
        if flaky:
            out.append("\n**Flaky tests** (spec 125-A1: a test flaky in two runs gets an issue and an owner):\n")
            for name, msg in flaky:
                out.append(f"- `{name}`" + (f": {msg}" if msg else ""))
                print(f"::warning title=flaky test::{name} failed, then passed on its one retry (spec 125-A1)")
            out.append("")
        else:
            out.append("\nNo flaky tests.\n")
        if failed:
            out.append("\n**Failed** (also on the retry):\n")
            out.extend(f"- `{n}`" for n in failed)
            out.append("")
    text = "\n".join(out) + "\n"
    dest = os.environ.get("GITHUB_STEP_SUMMARY")
    if dest:
        with open(dest, "a", encoding="utf-8") as f:
            f.write(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
