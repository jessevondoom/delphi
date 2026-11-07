#!/usr/bin/env python3
"""Tests for matcher_v2.py. Run: python3 -I dev/train/e2/test_matcher_v2.py"""
import re
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import matcher_v2 as m  # noqa: E402

# The three pairs quoted in the E2 report (v1 Jaccard 0.45, 0.35, 0.58; all below v1's 0.5 except the last, which v1
# also missed because of its own normalisation). Scores below are what v2 actually returns; they were not tuned for.
PAIR_045 = ("TOCTOU and corruption risks in external host config mutation (D-2)",
            "Host config mutation creates external state corruption and privilege escalation (D-2)")
PAIR_035 = ("D-3 launcher script is executable content run unsandboxed by the host; no justification why a plain command entry in MCP config is insufficient; defer",
            "D-3 launcher script ships executable content run unsandboxed by the host (§5.5); a bare `command: delphi` needs no script, so cost exceeds return")
PAIR_058 = ("Critic must be a different model family than the drafter (H-008); label the verdict homogeneous if only one family is configured",
            "Reviewer must be a different model family than the drafter (H-008); label homogeneous if only one family exists")


class T(unittest.TestCase):
    def test_constants_equal_the_spec(self):
        spec = (HERE.parent.parent.parent / "specs" / "089-panel-hardening-from-e2.md").read_text(encoding="utf-8")
        self.assertIn("share an anchor and their content-word Jaccard is at least 0.2", spec)
        self.assertIn("content-word Jaccard is at least 0.4", spec)
        self.assertEqual(m.ANCHOR_JACCARD, 0.2)
        self.assertEqual(m.PLAIN_JACCARD, 0.4)

    def test_anchors(self):
        self.assertEqual(m.anchors("see D-2 and H-008, §5.5, spec 071 and a P0"), {"d-2", "h-008", "§5.5", "spec071", "p0"})
        self.assertEqual(m.anchors("nothing here"), set())

    def test_words_drop_stopwords_punctuation_and_anchors(self):
        self.assertEqual(m.words("The Host (D-2) must NOT mutate config!"), {"host", "mutate", "config"})

    def test_pair_045(self):
        # Shares anchor D-2 and Jaccard 0.4545: above both thresholds, so it matches.
        j, shared, ok = m.score_pair(*PAIR_045)
        self.assertTrue(shared)
        self.assertAlmostEqual(j, 0.4545, places=3)
        self.assertTrue(ok)

    def test_pair_035(self):
        # Shares anchor D-3; Jaccard 0.381 is below 0.4 but above 0.2: it matches only through the anchor rule.
        j, shared, ok = m.score_pair(*PAIR_035)
        self.assertTrue(shared)
        self.assertAlmostEqual(j, 0.381, places=3)
        self.assertLess(j, m.PLAIN_JACCARD)
        self.assertTrue(ok)

    def test_pair_058(self):
        # Shares anchor H-008; Jaccard 0.545: above both thresholds, matches.
        j, shared, ok = m.score_pair(*PAIR_058)
        self.assertTrue(shared)
        self.assertAlmostEqual(j, 0.5455, places=3)
        self.assertTrue(ok)

    def test_unrelated_pair_does_not_match(self):
        _, _, ok = m.score_pair("Token budget cap is missing for the nightly sync (D-2)",
                                "Colour palette fails contrast on the dark theme")
        self.assertFalse(ok)

    def test_anchor_alone_is_not_enough(self):
        j, shared, ok = m.score_pair("D-2 rollback path untested", "D-2 pricing page wording unclear")
        self.assertTrue(shared)
        self.assertLess(j, m.ANCHOR_JACCARD)
        self.assertFalse(ok)

    def test_cross_question_control_fixtures(self):
        q1 = ["Host config mutation is a TOCTOU risk (D-2)", "Launcher script is unsandboxed executable content (D-3)",
              "No rollback if the install is interrupted"]
        q2 = ["Pricing tier names confuse new customers", "Retention cohort data lacks a baseline (D-2)",
              "Quarterly forecast ignores seasonality"]
        self.assertEqual(m.match(q1, q2), [])

    def test_same_question_fixture_pairs_one_to_one(self):
        a = ["Host config mutation is a TOCTOU risk (D-2)", "No rollback if the install is interrupted"]
        b = ["Host config mutation allows TOCTOU races (D-2)", "Host config mutation corrupts external state (D-2)",
             "Install interruption leaves no rollback"]
        pairs = m.match(a, b)
        self.assertEqual(len({i for i, _ in pairs}), len(pairs))
        self.assertEqual(len({j for _, j in pairs}), len(pairs))
        self.assertIn(0, {i for i, _ in pairs})

    def test_no_stray_unicode_in_stopwords(self):
        self.assertTrue(all(re.fullmatch(r"[a-z]+", w) for w in m.STOPWORDS))


if __name__ == "__main__":
    unittest.main()
