"""Unit tests; run: python3 -I dev/host-probe/test_host_probe.py. No model tokens, fake host binaries only."""
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import host_probe as hp  # noqa: E402

SCRIPT = os.path.join(HERE, "host_probe.py")


class T(unittest.TestCase):
    def test_canary_paths_per_spec(self):
        want = {
            "claude-code": ".claude/agents/delphi-canary-a.md",
            "copilot": ".github/agents/delphi-canary-a.agent.md",
            "codex": ".codex/agents/delphi-canary-a.toml",
            "agy": ".agents/agents/delphi-canary-a.md",
        }
        for h, p in want.items():
            self.assertIn(p, hp.canary_files(h, "x"))

    def test_analyse_parallel_direct(self):
        out = ""
        for n, s, e in (("delphi-canary-a", 100, 110), ("delphi-canary-b", 101, 111)):
            out += f"SEAT {n} START {s}\nSEAT {n} MCP direct 13\nSEAT {n} END {e}\nSEAT {n} CANARY-OK\n"
        c = hp.analyse(out, 0, "haiku")
        self.assertEqual(c["parallel"]["status"], "verified")
        self.assertIn("direct", c["mcp"]["note"])
        self.assertEqual(c["model_pin"]["status"], "verified")

    def test_analyse_serial_and_relayed(self):
        out = ""
        for n, s, e in (("delphi-canary-a", 100, 110), ("delphi-canary-b", 111, 120)):
            out += f"SEAT {n} START {s}\nSEAT {n} MCP none 0\nSEAT {n} END {e}\nSEAT {n} CANARY-OK\n"
        c = hp.analyse(out, 0, None)
        self.assertEqual(c["parallel"]["status"], "failed")
        self.assertIn("relayed", c["mcp"]["note"])
        self.assertEqual(c["model_pin"]["status"], "not run")

    def test_analyse_nothing(self):
        c = hp.analyse("boom", 1, "flash")
        self.assertEqual(c["discovered"]["status"], "failed")

    def test_blank_render_is_all_not_run(self):
        text = hp.render([hp.blank_entry(h) for h in hp.HOSTS])
        self.assertNotIn("| verified", text)
        self.assertNotIn("| failed", text)
        self.assertEqual(text.count("| not run |"), 4 * (len(hp.CELLS) + 1))

    def test_dry_run_with_fake_binaries_spends_nothing(self):
        with tempfile.TemporaryDirectory() as d:
            bindir = os.path.join(d, "bin")
            os.makedirs(bindir)
            marker = os.path.join(d, "ran-with-args")
            for b in hp.BIN.values():
                path = os.path.join(bindir, b)
                with open(path, "w") as f:
                    f.write(f'#!/bin/sh\necho "$@" >> {marker}\n[ "$1" = "--version" ] && echo "{b} 9.9.9" && exit 0\nexit 9\n')
                os.chmod(path, os.stat(path).st_mode | stat.S_IEXEC)
            res = os.path.join(d, "r.json")
            env = dict(os.environ, PATH=bindir + os.pathsep + "/usr/bin:/bin", TMPDIR=d)
            p = subprocess.run([sys.executable, "-I", SCRIPT, "probe", "--write", "--results", res],
                               capture_output=True, text=True, env=env)
            self.assertEqual(p.returncode, 0, p.stderr)
            self.assertIn("NO model tokens", p.stdout)
            with open(marker) as f:
                self.assertEqual(set(f.read().split()), {"--version"})
            data = json.load(open(res))
            self.assertTrue(all(e["cli_version"].endswith("9.9.9") for e in data))
            self.assertTrue(all(c["status"] == "not run" for e in data for c in e["cells"].values()))


if __name__ == "__main__":
    unittest.main()
