#!/usr/bin/env python3
"""Finding matcher v2 (spec 089 section 7 / slice P5). Deterministic; no model; reads nothing.

Written AFTER E2's data was seen, so it is exploratory and never replaces the pre-registered v1 matcher in
score.py. The thresholds below are FIXED by spec 089 section 7 and are not tuned against any data.

Rule: two findings match if
  (they share an anchor AND content-word Jaccard >= ANCHOR_JACCARD)  OR  content-word Jaccard >= PLAIN_JACCARD.
Pairing is greedy and one-to-one, best score first, as in score.match (v1).
"""
import re

# Fixed by spec 089 section 7. test_matcher_v2.py checks these against the spec text.
ANCHOR_JACCARD = 0.2
PLAIN_JACCARD = 0.4

# A fixed stop-word list, written down here so it cannot drift: function words and generic verbs of advice.
STOPWORDS = frozenset((
    "a an the and or but nor so yet for of to in on at by with from into onto over under about after before "
    "between against through during without within is are was were be been being am do does did done doing "
    "has have had having it its this that these those there their they them he she his her you your we our "
    "not no can could may might must shall should will would than then too very also only just more most less "
    "such each both any all some other another what which who whom whose when where why how as if while "
    "via per vs one two ought need needs needed"
).split())

# Anchors: ids such as D-2, H-008, section marks (5.5), "spec 071", severities P0..P3.
ANCHOR_RES = (
    re.compile(r"\b[A-Z]{1,3}-\d{1,4}\b"),          # D-2, H-008, OD-1, S-3
    re.compile(r"§\s*\d+(?:\.\d+)*"),               # §5.5
    re.compile(r"\bspecs?\s+(\d{2,4})\b", re.I),    # spec 071
    re.compile(r"\bP[0-3]\b"),                      # P0..P3
)


def _norm_anchor(s):
    return re.sub(r"\s+", "", s.lower())


def anchors(text):
    """The set of normalised anchors in a finding title."""
    out = set()
    for rx in ANCHOR_RES:
        for m in rx.finditer(text):
            if rx.pattern.startswith(r"\bspecs?"):
                out.add("spec" + m.group(1))
            else:
                out.add(_norm_anchor(m.group(0)))
    return out


def words(text):
    """Content words: lowercase, punctuation stripped, anchors removed, stop words and tokens under 3 chars dropped."""
    t = text
    for rx in ANCHOR_RES:
        t = rx.sub(" ", t)
    return {w for w in re.sub(r"[^a-z0-9]+", " ", t.lower()).split() if len(w) >= 3 and w not in STOPWORDS}


def jaccard(x, y):
    u = x | y
    return len(x & y) / len(u) if u else 0.0


def score_pair(title_a, title_b):
    """(jaccard, shared_anchor, matches) for two finding titles."""
    j = jaccard(words(title_a), words(title_b))
    shared = bool(anchors(title_a) & anchors(title_b))
    return j, shared, (j >= PLAIN_JACCARD or (shared and j >= ANCHOR_JACCARD))


def match(titles_a, titles_b):
    """Greedy one-to-one matching over two lists of titles; returns [(i, j)], best score first."""
    pairs = []
    for i, x in enumerate(titles_a):
        for j, y in enumerate(titles_b):
            s, _, ok = score_pair(x, y)
            if ok:
                pairs.append((-s, i, j))
    pairs.sort()
    used_a, used_b, out = set(), set(), []
    for _, i, j in pairs:
        if i not in used_a and j not in used_b:
            used_a.add(i)
            used_b.add(j)
            out.append((i, j))
    return out
