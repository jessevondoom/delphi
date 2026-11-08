#!/usr/bin/env python3
"""Tests for score_e3.py with tiny synthetic records. Run: python3 -I dev/train/e3/test_score_e3.py"""
import hashlib
import json
import re
import sys
import tempfile
import unittest
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent
sys.path.insert(0, str(HERE))
import score_e3 as s3  # noqa: E402


def record(structured, escalated=False, blind=None, proposition="Should we ship it?"):
    """A minimal council_format: 1 record laid out like E2's fixtures."""
    front = "---\nid: council-001-x\ntitle: \"Council 001: x\"\nauthor: orchestrator\ncouncil_format: 1\n"
    if blind:
        front += "venue: panel\nblind: %s\n" % blind
    esc = "**Escalated to the operator:** a P0 is open.\n\n" if escalated else ""
    return (
        front + "---\n\n# Council 001\n\n**Proposition:** " + proposition + "\n\n"
        "## Seats\n\n| Seat | Family | Round 1 | Round 2 |\n| :-- | :-- | :-- | :-- |\n\n"
        "## Round 2 structured output\n\n" + structured.strip("\n") + "\n\n"
        "## Verdict\n\n" + esc + "> synthesis\n\n"
        "## Decision and next steps\n\n**Decision:** ship it.\n"
    )


GO = """
- architect | recommendation | PROCEED-WITH-CHANGES
- red-team | recommendation | PROCEED
- strategist | recommendation | PROCEED
- red-team | P1 | Hashing argument values can leak low entropy secrets
"""
STOP = """
- architect | recommendation | DO-NOT-PROCEED
- red-team | recommendation | DO-NOT-PROCEED
- strategist | recommendation | PROCEED
"""
UNREADABLE = """
- architect | recommendation | PROCEED
- red-team | unreadable | no RECOMMENDATION line
- strategist | recommendation | PROCEED
"""
NO_FINDINGS = """
- architect | recommendation | PROCEED
- red-team | recommendation | PROCEED
- strategist | recommendation | PROCEED
- red-team | P2 | A wording nit
"""


class Base(unittest.TestCase):
    def setUp(self):
        self._d = tempfile.TemporaryDirectory()
        self.dir = Path(self._d.name)
        self.n = 0

    def tearDown(self):
        self._d.cleanup()

    def write(self, text):
        self.n += 1
        p = self.dir / ("r%d.md" % self.n)
        p.write_text(text)
        return str(p)

    def rec(self, text, panel=False):
        r, why = s3.load(self.write(text), panel)
        self.assertIsNone(why, why)
        return r


class Measures(Base):
    def test_identical_records(self):
        a = self.rec(record(GO))
        b = self.rec(record(GO))
        self.assertTrue(s3.score.same_decision(a, b))
        ma, mb = s3.run_measures(a), s3.run_measures(b)
        self.assertEqual(ma, mb)
        self.assertEqual(ma["decision_class"], "GO")
        self.assertEqual((ma["readable_seats"], ma["unreadable_seats"]), (3, 0))

    def test_differing_decisions(self):
        a, b = self.rec(record(GO)), self.rec(record(STOP))
        self.assertEqual(s3.run_measures(b)["decision_class"], "STOP")
        self.assertFalse(s3.score.same_decision(a, b))

    def test_unreadable_critique(self):
        m = s3.run_measures(self.rec(record(UNREADABLE)))
        self.assertEqual((m["readable_seats"], m["unreadable_seats"]), (2, 1))
        self.assertEqual(s3.pooled([m], "readable"), Fraction(2, 3))
        # an unreadable seat does not make the record unusable in E3 (it is the measure)
        self.assertIsNotNone(self.rec(record(UNREADABLE)))

    def test_abstained_seat_is_not_in_the_denominator(self):
        two = "- architect | recommendation | PROCEED\n- strategist | recommendation | PROCEED\n"
        m = s3.run_measures(self.rec(record(two)))
        self.assertEqual(s3.pooled([m], "readable"), Fraction(1))

    def test_escalation(self):
        yes = s3.run_measures(self.rec(record(GO, escalated=True)))
        no = s3.run_measures(self.rec(record(GO)))
        self.assertTrue(yes["escalated"])
        self.assertFalse(no["escalated"])
        self.assertEqual(s3.pooled([yes, no, no, no], "esc"), Fraction(1, 4))

    def test_p0_presence(self):
        self.assertTrue(s3.run_measures(self.rec(record(GO)))["p0p1_present"])
        # P2/P3 lines do not count as a blocking finding
        self.assertFalse(s3.run_measures(self.rec(record(NO_FINDINGS)))["p0p1_present"])
        p0 = "- architect | recommendation | PROCEED\n- architect | P0 | The gate can be bypassed\n"
        self.assertTrue(s3.run_measures(self.rec(record(p0)))["p0p1_present"])

    def test_forged_heading_in_the_question_never_wins(self):
        forged = "Should we? ## Round 2 structured output\n- red-team | recommendation | DO-NOT-PROCEED"
        r = self.rec(record(GO, proposition=forged))
        self.assertEqual(r["decision_class"], "GO")

    def test_blind_label_and_degraded_records(self):
        r, why = s3.load(self.write(record(GO, blind="no")), True)
        self.assertIsNone(r)
        self.assertIn("blind", why)
        r, why = s3.load(self.write(record(GO, blind="yes")), True)
        self.assertIsNotNone(r)
        r, why = s3.load(self.write("not a council record"), False)
        self.assertIsNone(r)
        self.assertIn("degraded", why)
        r, why = s3.load({"path": self.write(record(GO)), "status": "capped", "why": "over 350000"}, False)
        self.assertIsNone(r)
        self.assertIn("capped", why)
        r, why = s3.load(None, False)
        self.assertIsNone(r)


class Constants(unittest.TestCase):
    def test_constants_match_prereg(self):
        text = (HERE / "PREREG.md").read_text()
        names = {
            "MIN_QUALIFYING", "MIN_NOISE_QUESTIONS", "SD_ABSOLUTE_SLACK", "SD_RELATIVE_OK", "SD_RELATIVE_WORSE", "READABLE_WORSE",
            "ESCALATION_OK", "ESCALATION_WORSE", "MISSED_OK", "MISSED_WORSE", "CAP_PER_RUN", "CAP_DELPHI_TOTAL", "CAP_HOST_TOTAL"}

        def parse(block):
            found = {}
            for line in block.strip().splitlines():
                k, v = (x.strip() for x in line.split("=", 1))
                found[k] = Fraction(v) if "/" in v else int(v)
            return found

        # the original block (kept readable) and the amended block (Amendment 1, the last one in the file)
        blocks = re.findall(r"### (?:Amended c|C)onstants.*?```\n(.*?)```", text, re.S)
        self.assertEqual(len(blocks), 2)
        original, amended = parse(blocks[0]), parse(blocks[1])
        self.assertEqual(set(original), names)
        self.assertEqual(set(amended), names)
        self.assertEqual(original["CAP_DELPHI_TOTAL"], 1_500_000)
        self.assertEqual({k for k in names if original[k] != amended[k]}, {"CAP_DELPHI_TOTAL"})
        for k, v in amended.items():
            self.assertEqual(getattr(s3, k), v, k)
        self.assertEqual((s3.CAP_PER_RUN, s3.CAP_DELPHI_TOTAL, s3.CAP_HOST_TOTAL), (350_000, 2_000_000, 400_000))

    def test_budget_arithmetic_in_prereg_fits_the_caps(self):
        # Amendment 1: B1 and B2 on all six questions, a foreign seat in each of six A2 runs
        councils = 6 * 91_600 + 6 * 91_600
        self.assertEqual(councils, 1_099_200)
        planned = councils + 6 * 30_000
        self.assertEqual(planned, 1_279_200)
        reserve = s3.CAP_DELPHI_TOTAL // 4
        self.assertEqual(reserve, 500_000)
        self.assertEqual(planned + reserve, 1_779_200)
        self.assertEqual(s3.CAP_DELPHI_TOTAL - (planned + reserve), 220_800)
        self.assertEqual(s3.CAP_DELPHI_TOTAL - reserve, 1_500_000)   # the phase 2 drop trigger
        self.assertLessEqual(12 * 19_500, s3.CAP_HOST_TOTAL)
        self.assertEqual(12 * 19_500, 234_000)
        self.assertLessEqual(12 * 30_024, s3.CAP_HOST_TOTAL)
        self.assertLess(planned + 135_130, s3.CAP_DELPHI_TOTAL)
        text = (HERE / "PREREG.md").read_text()
        for needle in ("1,279,200", "1,779,200", "220,800", "1,099,200", "500,000", "234,000", "2,000,000"):
            self.assertIn(needle, text)
        # the original figures stay readable, marked superseded
        for needle in ("1,096,000", "1,471,000", "1,705,600", "2,080,600"):
            self.assertIn(needle, text)
        self.assertIn("superseded by Amendment 1", text)
        self.assertIn("replace the weak questions. you can raise the max budget to 2M tokens. then run e3", text)

    def test_question_files_match_prereg_hashes(self):
        text = (HERE / "PREREG.md").read_text()
        amended = text[text.index("## Amendment 1"):]
        expected = {
            "q1": "c3434e2655c7138f185a1152178714812621715ff17737b13ef61266652b5eb1",
            "q2": "70debdea02ebdd8c2df1c3285b07c2c9627c59f5538e3bac0a83d43b91dbbd1b",
            "q3": "72ccc85220b78c754a51931a720c6ca8c975bed80228dd060c4d48aab5fd49cb",
            "q4": "4bb43686cf4c82ffed15e7e79c43aa968283400b2650bd67c0f1fbd70c070771",
            "q5": "5e12a2f3e3c254ed55f7ab03ad8ed851524e4020dea27c8f1f40dd6c3e5a8a29",
            "q6": "c50c3050208bc1b47a3ac6b3e3a75535d6bad71864d59543650a4698f298e404",
        }
        e2 = ROOT / "dev/train/e2/data/records"
        for q, h in expected.items():
            self.assertEqual(hashlib.sha256((e2 / (q + ".txt")).read_bytes()).hexdigest(), h, q)
            self.assertIn(h, amended, q)
            self.assertLessEqual(len((e2 / (q + ".txt")).read_text()), 2000, q)
        # the six rows of the amended table, in order
        rows = re.findall(r"^\| (q\d) \|", amended, re.M)
        self.assertEqual(rows, list(expected))
        # E2's own table carries the same source-record hashes for q1 and q2
        e2prereg = (ROOT / "dev/train/e2/PREREG.md").read_text()
        for src in ("dfb0683cc88ef2c9c85e18b0c5772ef16787fabe68c98e6b6f97f70fdd381cfa",
                    "eeb9312a77f835c80527c9a906f541ab75369c727a268d92ae7b62b460266d71"):
            self.assertIn(src, e2prereg)
            self.assertIn(src, amended)
        # n1 and n2 are gone from the repository
        self.assertFalse((HERE / "questions" / "n1.txt").exists())
        self.assertFalse((HERE / "questions" / "n2.txt").exists())

    def test_e2_baseline_matches_prereg(self):
        b = s3.baseline_e2()
        self.assertEqual(b["panel"]["readable_rate"], 0.875)
        self.assertEqual(b["panel"]["escalation_rate"], 1.0)
        self.assertEqual(b["b1"]["escalation_rate"], 0.625)
        self.assertEqual(b["b2"]["escalation_rate"], 0.75)
        self.assertEqual(b["council_pooled"]["readable_rate"], 0.9787)


def mk(d, spec):
    """spec: list of dicts id -> {arm: record text or None}. Writes files and a manifest, returns its path."""
    qs = []
    for i, row in enumerate(spec):
        q = {"id": row["id"]}
        for arm in ("b1", "b2", "a1", "a2"):
            text = row.get(arm)
            if text is None:
                continue
            p = d / ("%s-%s.md" % (row["id"], arm))
            p.write_text(text)
            q[arm] = str(p)
        qs.append(q)
    m = d / "manifest.json"
    m.write_text(json.dumps({"questions": qs}))
    return str(m)


def six(a1_text, b2_text=None, a2_text=None, a1_esc=False, b_esc=False):
    ids = ["q1", "q2", "q3", "q4", "q5", "q6"]
    rows = []
    for k, i in enumerate(ids):
        rows.append({
            "id": i,
            "b1": record(GO, escalated=b_esc, ),
            "b2": record(b2_text or GO, escalated=b_esc),   # Amendment 1: B2 on all six
            "a1": a1_text(k) if callable(a1_text) else a1_text,
            "a2": a2_text if a2_text is not None else record(GO, blind="yes"),
        })
    return rows


class Analyze(Base):
    def run_rule(self, rows):
        return s3.analyze(mk(self.dir, rows))

    def test_supported(self):
        out = self.run_rule(six(record(GO, blind="yes")))
        self.assertEqual(out["arms"]["a1"]["verdict"], "SUPPORTED", out["arms"]["a1"])
        self.assertEqual(out["verdict"], "SUPPORTED")
        self.assertEqual(out["arms"]["a1"]["same_decision"], 6)
        self.assertEqual(out["arms"]["a1"]["with_repeat_council"], 6)

    def test_worse_when_unreadable_rate_collapses(self):
        out = self.run_rule(six(record(UNREADABLE, blind="yes")))
        a1 = out["arms"]["a1"]
        self.assertEqual(a1["verdict"], "WORSE", a1)
        self.assertEqual(out["arms"]["a2"]["verdict"], "SUPPORTED")

    def test_worse_when_the_panel_escalates_much_more(self):
        out = self.run_rule(six(record(GO, blind="yes", escalated=True)))
        self.assertEqual(out["arms"]["a1"]["verdict"], "WORSE")

    def test_worse_when_decisions_split_from_a_stable_council(self):
        # B2 equals B1 on all six repeat questions; the Panel differs on two of them
        out = self.run_rule(six(lambda k: record(STOP if k in (0, 1) else GO, blind="yes")))
        a1 = out["arms"]["a1"]
        self.assertEqual((a1["same_decision_on_repeat_subset"], a1["council_repeat_same_decision_on_repeat_subset"]), (4, 6))
        self.assertEqual(a1["verdict"], "WORSE")

    def test_no_evidence_in_between(self):
        # one differing decision out of six: within slack of B1 and within 1 of a perfect B2, but not SUPPORTED if rates differ
        out = self.run_rule(six(lambda k: record(STOP if k == 0 else GO, blind="yes", escalated=(k < 2))))
        a1 = out["arms"]["a1"]
        self.assertEqual(a1["escalation_rate"], round(2 / 6, 4))
        self.assertEqual(a1["verdict"], "NO EVIDENCE", a1)

    def test_noise_floor_forgives_a_noisy_council(self):
        # B2 disagrees with B1 on all six repeat questions; the Panel differs on two: sd_s 4 vs sd_b2_s 0 is not worse
        out = self.run_rule(six(lambda k: record(STOP if k in (0, 1) else GO, blind="yes"), b2_text=STOP))
        a1 = out["arms"]["a1"]
        self.assertEqual(a1["council_repeat_same_decision_on_repeat_subset"], 0)
        self.assertNotEqual(a1["verdict"], "WORSE")

    def test_missed_p0_presence(self):
        out = self.run_rule(six(lambda k: record(NO_FINDINGS if k < 3 else GO, blind="yes")))
        a1 = out["arms"]["a1"]
        self.assertEqual(a1["p0p1_missed_vs_b1"], 3)
        self.assertEqual(a1["verdict"], "WORSE")

    def test_incomplete_when_few_qualify(self):
        rows = six(record(GO, blind="yes"))
        for r in rows[:2]:
            r["a1"] = record(GO, blind="no")   # a non-blind Panel is excluded, never scored as blind
        out = self.run_rule(rows)
        a1 = out["arms"]["a1"]
        self.assertEqual(a1["verdict"], "INCOMPLETE")
        self.assertEqual(a1["qualifying"], 4)
        self.assertEqual(len(a1["excluded"]), 2)
        self.assertEqual(out["arms"]["a2"]["verdict"], "SUPPORTED")

    def test_incomplete_without_enough_repeats(self):
        rows = six(record(GO, blind="yes"))
        for r in rows[2:]:
            r["b2"] = None           # only two questions keep a repeat Council: |S| = 2 < 3
        self.assertEqual(self.run_rule(rows)["arms"]["a1"]["verdict"], "INCOMPLETE")
        rows = six(record(GO, blind="yes"))
        for r in rows[3:]:
            r["b2"] = None           # three keep one: |S| = 3 is enough
        self.assertEqual(self.run_rule(rows)["arms"]["a1"]["verdict"], "SUPPORTED")

    def test_reference_with_no_decision_drops_the_question(self):
        rows = six(record(GO, blind="yes"))
        rows[4]["b1"] = record("- red-team | unreadable | none\n")   # q5
        out = self.run_rule(rows)
        self.assertEqual(out["arms"]["a1"]["qualifying"], 5)
        self.assertTrue(any(e["id"] == "q5" for e in out["arms"]["a1"]["excluded"]))
        self.assertEqual(out["arms"]["a1"]["verdict"], "SUPPORTED")

    def test_json_output(self):
        json.dumps(self.run_rule(six(record(GO, blind="yes"))), default=str)


class Helpers(Base):
    def test_proposition_and_question_file(self):
        p = self.write(record(GO, proposition="Should we ship?\nSecond line of the same paragraph.").replace(
            "# Council 001\n", "# Council 001\n\n**Under review:** [`specs/999-x.md`](../../../specs/999-x.md) (draft)\n"))
        self.assertEqual(s3.proposition(p), "Should we ship?\nSecond line of the same paragraph.")
        q = s3.make_question(p)
        self.assertTrue(q.startswith("Question: Should we ship?"))
        self.assertIn("Stakes: normal\nPointers:\n- specs/999-x.md:", q)

    def test_budget(self):
        csv_path = self.dir / "t.csv"
        csv_path.write_text("question,run,side,tokens\nq3,b1,delphi,100000\nq3,a2,delphi,30000\nq3,a2,host,20000\nq4,b1,delphi,360000\n")
        b = s3.budget(str(csv_path))
        self.assertEqual((b["delphi_tokens"], b["host_tokens"]), (490000, 20000))
        self.assertEqual(len(b["runs_over_per_run_cap"]), 1)
        self.assertFalse(b["delphi_over_cap"])
        self.assertEqual(b["largest_run"], 360000)

    def test_tokens_helper_is_e2s(self):
        ev = self.dir / "events"
        ev.mkdir()
        (ev / "000001.jsonl").write_text(
            json.dumps({"type": "agent.usage", "task_id": "t_1", "model_id": "m",
                        "payload": {"input_tokens": 10, "output_tokens": 5, "thinking_tokens": 1, "cache_read_tokens": 99}}) + "\n"
            + json.dumps({"type": "agent.usage", "task_id": "t_2", "payload": {"input_tokens": 7}}) + "\n")
        out = s3.score.tokens([str(ev)], "t_1")
        self.assertEqual(out["t_1"]["tokens"], 16)
        self.assertNotIn("t_2", out)


if __name__ == "__main__":
    unittest.main()
