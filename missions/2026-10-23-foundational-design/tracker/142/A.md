---
state: built
date: 2026-11-25
title: "the capture record: raw, capture.json, both digests, the extractor, revisits, sources verify"
---

Built 2026-11-25 (local branch `build/142-a`, not pushed). `library_capture.rs` is new. A capture that reads something now writes, beside `source.md`, the bytes as they came (`raw`, only where the text is kept) and `capture.json`: `raw_sha256`, `text_sha256`, `bytes`, `media_type`, `retrieved`, `captured_by` (`daemon`, `operator`, `ingest`), the final address and status of a fetch, `extractor: {name, version}`, `rights` (`open`, `held`, `private`) and `revisits`. The stored text is in one normal form (no byte-order mark, `\n`, NFC) and `text_sha256` is the hash of exactly that; it is taken before a level below full drops the text, so a pointer records both digests and keeps neither body.

Revisits (`Library::store`): the same bytes add `{at, by}` to the current version's record; other bytes with the same `text_sha256` add `{at, by, raw_changed: true}`, make no version and touch no anchor; other text is a new version, and the old version's three files move under `versions/<n>/` unchanged. A version lists at most 100 revisits and then counts (`revisits_more`). A takedown (`sources purge`) keeps `capture.json` and marks it `purged`. `delphi sources verify [<id>] [--json]` hashes every kept `raw` and `source.md` again, reports each that differs, changes nothing and exits 1 when one differs.

**Migration: none to run.** A source from before this slice has no `capture.json` and reads as before. The next time the same bytes are read its record is written for the version that is there (`adopted`, with the date; a full source also gains `raw`), once; different bytes make a new version with its record. `verify` counts such versions apart (`unrecorded`). Tested (`a_source_from_before_the_record_keeps_working_and_gains_its_record_on_the_next_read`), and run on a copy of the operator's library (94 pointer sources): see the built entry of slice D1.

Tests: AC-1, AC-2, AC-3 (library and CLI), the revisit bound, the pointer case, the three normal-form cases for one `text_sha256`, the purge mark, and the fitness function `only_store_writes_a_versions_files`.

Not done here, and for Architect: the library's stored text is made by `fact_fetch::extract_text`, not by the HTML conversion at `html.rs:160` that §2.1 item 4 names. Both now have a constant name and version; a capture names the one that made its text (`fact-fetch-text` 1). An archived copy and a workspace document are stored as before, with no capture record.
