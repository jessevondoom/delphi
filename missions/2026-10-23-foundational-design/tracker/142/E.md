---
state: built
date: 2026-11-25
title: "anchors: text_sha256 over normalised text, pinned excerpts, unverifiable when evicted, orphans shown"
---

Built 2026-11-25 (local branch `build/142-a`, not pushed).

**The anchor.** It gains `text_sha256`, the hash of the stored (normalised) text its `start` and `end` count characters in. `Anchor::resolve_in(text, text_sha256)` resolves in order: the position, when the hash is the anchor's, with no search; else the exact quote; else the quote's context. A stored position is never used against another text: `resolve` no longer prefers the occurrence nearest the old position (among equal surroundings the first in the text wins). A new version re-resolves every live anchor by its quote and writes the new hash with the new place.

**Pinned excerpts.** When an anchor is made its quote (at most 300 characters) is pinned in the source's `pinned.json`, beside its record and outside every version, with the `text_sha256` and position it was found at. For a source whose text is not kept (`pointer`, `excerpts`) a research run passes the capture's text as it read it (`AnchorSpec::read`, from `Library::capture_text`); it is used only when its hash is the one the capture record states. A new version, eviction and a purge of the cache leave `pinned.json`; a takedown (`sources purge`) removes it.

**Orphans.** An anchor not found in a newer text is `lost` as before, with `orphan: {why: "not in the newer text", at}`, and contests its claim. `Library::remove_bodies` (new, in `library_private.rs`) is how text goes by eviction or by a purge of the cache: `raw` and `source.md` of every version are removed, each `capture.json` says `evicted: {raw, text}` or `purged`, the record reads as a pointer with `text_evicted` or `text_purged`, and each live anchor gains `orphan: {why: "text evicted" | "text purged"}` and is not contested. Such an anchor counts (`Library::counts`: corroboration, and the page check reads the kept passage in place of the text) only while its verified excerpt is kept; with none the claim reads `unverifiable, text evicted` (`Library::unverifiable`, shown by `claim_slice`), leaves the corroboration count, and a page statement the validator calls supported by it is recorded `unverifiable` and listed with the unchecked ones. Reading the source again resolves the anchor and it counts again.

**Shown** (`Library::orphans_of`): `delphi sources show` (text and `--json`), the app's source view (`library.source` carries `orphans`; a section below the text), the page bibliography (the citation cell says `N orphaned anchor(s), listed under s-<id>`), and `delphi kb health` (a line and `orphaned_anchors` in the JSON, only when there are some).

**Changed on purpose, two existing tests.** `delphi sources private purge` and the private-text cap used the takedown, which blanked the quote and made anchors `pointer_only`. Under §2.3 they now remove the bodies only: the anchor keeps its quote and reads `text purged` or `text evicted`, the record is not `purged`. The two assertions that named the old behaviour say the new one.

Tests: AC-13 (`a_position_is_used_with_no_search_when_the_text_hash_matches`, `a_position_is_ignored_when_the_text_hash_differs`, the quote-then-context order, and the same on the library across four versions), AC-14 (`an_evicted_anchor_without_an_excerpt_is_barred`, the source view, the bibliography, health, the CLI's `sources show`, the app's `Orphans`), AC-22 (`every_new_anchor_pins_an_excerpt` under held, `pointer` and `excerpts`; survival of eviction, purge and a new version; the three normal-form cases giving one hash and one position).

For Architect: (1) "a page statement resting on it does not pass validation" is built as *not counted as supported and listed as unchecked*, not as a failed page; the validator must name the claim for the bar to apply. (2) "A count that links to them" in the bibliography is the count beside the row whose id the app already links to the source view. (3) An anchor made into an operator's kept excerpt (a source not held whole) has no position to pin; the kept excerpt itself is what backs it.
