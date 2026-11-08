---
id: renumber-090-citation-map
title: "Spec 090 citation map: which spec each citation of 014, 037 and 061 meant"
author: builder
timestamp: 2026-11-08T12:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [spec, numbering, citations, map]
---

# Spec 090 citation map

Written by the build of spec 090 S1 (2026-11-08). It records, for every citation by number of the three shared spec numbers (014, 037, 061) in a live file, which spec of the pair it meant and what was done. The operator samples this table (spec 090 section 5, test 6); the rows decided by reading are marked `reading` and should all be looked at.

**What moved.** The documents-and-dataviz spec was 014 and is now 095. The console-deck spec was 037 and is now 096 (amended from "training": the training truth packs are fingerprinted, see spec 090). The encyclopedia-search-pages spec was 061 and is now 097. The relay, training and shell-tool specs keep 014, 037 and 061.

**Rules, first match wins.** `path`: an exact file name, unambiguous. `location`: the file's place decides it (a crate directory or file-name prefix). `amend-tag`: the amendment tag exists in only one of the pair. `section`: only one of the pair has that section heading. `keyword`: the line, or the two lines above it, names one spec's subject and not the other's. `own-file`: a citation inside one of the six spec files. `reading`: decided by a person reading the context, with the reason in the last column.

**Actions.** `MOVE`: the citation meant a moved spec and was rewritten to its new number or file name. `keep`: it meant a spec that kept its number, so nothing changed. `none`: not a citation of a spec (a Council id, for example), or text that describes the renumbering itself and was edited by hand or left as history.

Historical records (council records, history files, past changelog entries) are not in this table and were not rewritten.

**Line numbers** are those of main at the start of the work (before any edit in this pull request). Four files were also edited by hand, so their rows can point at a line that moved: `docs/specifications.md` (the index table and its closing sentence), `missions/2026-10-23-foundational-design/spec-tracker.md` (the number-collisions paragraph and the 088 and 090 sections), `CONTRIBUTING.md` (the new "Spec numbers" section) and the three renamed specs (a note line under each title).

**The one-time strict check** (spec 090 section 3.4) rescanned the tree after the rewrite. Result: no row decided `MOVE` still cites an old number at its place (one row, `docs/specifications.md:18`, was reported only because the hand edit shifted the lines: that line is now the relay row, which keeps 014); and every remaining citation in a live file is either a row here decided `keep` or `none`, or text written in this pull request (the CONTRIBUTING table, the lint test fixtures, the new index rows, spec 090's own prose, the three note lines), which was read.

**Counts.** 343 decided rows: 105 MOVE, 181 keep, 57 none. By rule: 7 amend-tag, 74 keyword, 148 location, 50 meta, 26 path, 35 reading, 3 section.

| File:line | Number | Meant | Rule | Action | Reason (reading rows) |
| :-- | :-- | :-- | :-- | :-- | :-- |
| `GOVERNANCE.md:2` | 014 | 014-relay-and-away-mode | reading | keep | TOTP step-up status line: relay |
| `GOVERNANCE.md:88` | 014 | 014-relay-and-away-mode | reading | keep | "Remote Answers Are Narrower" rule cites the relay spec |
| `REVIEW.md:27` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `context/invariant-enforcement.md:89` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `context/invariant-enforcement.md:90` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `context/invariant-enforcement.md:91` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `context/invariant-enforcement.md:92` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `context/model-selection-evidence.md:6` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "Delphi's own results (specs 037, 048)": experiments |
| `context/model-selection-evidence.md:103` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "retrospective (spec 037)": experiments |
| `crates/delphi-adapter/src/config.rs:804` | 061 | 061-encyclopedia-search-pages (now 097) | reading | MOVE | section 4.4 exists only in the encyclopedia spec |
| `crates/delphi-core/src/hub.rs:21` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-core/src/lock.rs:34` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-core/src/proto.rs:17` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-core/src/server.rs:225` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-core/src/spawn.rs:30` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-core/src/totp.rs:2` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi-core/src/totp.rs:129` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi-docs/Cargo.toml:6` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/Cargo.toml:11` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/archive.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/cache.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/chart/mod.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/chart/more.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/chart/palette.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/chart/render.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/chart/scale.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/corpus_tests.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/csv.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/data.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/diagram/mod.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/diagram/tests.rs:42` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/docx.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/epub.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/font.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/html.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/htmlkinds.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/htmllint.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/htmlpage.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/htmlpage.rs:15` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/htmlpage.rs:74` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/image.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/lib.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/lib.rs:94` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/lib.rs:118` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/limits.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/mail.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/markdown.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/notebook.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/odf.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/office_tests.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/ooxml.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/pdf.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/pdf_tests.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/pdffont.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/pdftext.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/png.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/pptx.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/r4_tests.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/raster.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/rtf.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/sheetwriter.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/sqlite.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/table.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/xlsx.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/xml.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/zipfile.rs:2` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-docs/src/zipwriter.rs:1` | 014 | 014-documents-and-dataviz (now 095) | location | MOVE |  |
| `crates/delphi-engine/src/approval.rs:11` | 061 | 061-shell-tool | location | keep |  |
| `crates/delphi-engine/src/approval.rs:31` | 061 | 061-shell-tool | location | keep |  |
| `crates/delphi-engine/src/approval.rs:224` | 061 | 061-shell-tool | location | keep |  |
| `crates/delphi-engine/src/artifacts.rs:1` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `crates/delphi-engine/src/artifacts.rs:109` | 014 | 014-documents-and-dataviz (now 095) | reading | MOVE | section 7.2 exists only in the documents spec (Tools) |
| `crates/delphi-engine/src/engine.rs:47` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-engine/src/engine.rs:137` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-engine/src/engine.rs:2185` | 061 | 061-shell-tool | keyword | keep |  |
| `crates/delphi-engine/src/exec.rs:70` | 061 | 061-shell-tool | location | keep |  |
| `crates/delphi-engine/src/exec.rs:599` | 061 | 061-shell-tool | location | keep |  |
| `crates/delphi-engine/src/exec.rs:719` | 061 | 061-shell-tool | location | keep |  |
| `crates/delphi-engine/src/kb_docs.rs:3` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `crates/delphi-engine/src/kbserve.rs:271` | 061 | 061-encyclopedia-search-pages (now 097) | keyword | MOVE |  |
| `crates/delphi-engine/src/library_page_check.rs:344` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_check.rs:653` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_meta.rs:1` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_meta.rs:69` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_search.rs:34` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_search.rs:61` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_search.rs:156` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_search.rs:463` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_page_search.rs:605` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/library_workspace.rs:1` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/merge_class.rs:1` | 014 | 014-relay-and-away-mode | amend-tag | keep |  |
| `crates/delphi-engine/src/page_run.rs:27` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/page_run.rs:144` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/page_run.rs:223` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/page_run.rs:255` | 061 | 061-encyclopedia-search-pages (now 097) | location | MOVE |  |
| `crates/delphi-engine/src/service.rs:1802` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-engine/src/service.rs:1819` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-engine/src/service.rs:1820` | 014 | 014-relay-and-away-mode | amend-tag | keep |  |
| `crates/delphi-engine/src/service.rs:1878` | 061 | 061-shell-tool | keyword | keep |  |
| `crates/delphi-engine/src/service.rs:4583` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-engine/src/service.rs:4984` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi-engine/src/shellsafe.rs:1` | 061 | 061-shell-tool | location | keep |  |
| `crates/delphi-engine/src/tools.rs:41` | 061 | 061-shell-tool | keyword | keep |  |
| `crates/delphi-engine/src/tools.rs:1353` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `crates/delphi-engine/src/tools.rs:1388` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `crates/delphi/config.example.toml:151` | 061 | 061-shell-tool | keyword | keep |  |
| `crates/delphi/config.example.toml:206` | 061 | 061-encyclopedia-search-pages (now 097) | reading | MOVE | the writer proposes how long a page stays fresh |
| `crates/delphi/src/main.rs:998` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `crates/delphi/src/relay/cli.rs:1` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi/src/relay/cli.rs:217` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi/src/relay/email.rs:1` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi/src/relay/mod.rs:1` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi/src/relay/mod.rs:127` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi/src/relay/telegram.rs:1` | 014 | 014-relay-and-away-mode | location | keep |  |
| `crates/delphi/src/train.rs:28` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train.rs:54` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train.rs:88` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_arms.rs:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_arms.rs:397` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_defects.rs:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_hidden.rs:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_hidden.rs:10` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_hidden.rs:377` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_judge.rs:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_judge.rs:11` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_report.rs:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_report.rs:545` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_review.rs:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_review.rs:21` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_run.rs:152` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_run.rs:344` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_run.rs:346` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_run.rs:581` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_run.rs:702` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_truth.rs:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_truth.rs:32` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_truth.rs:168` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_truth.rs:656` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `crates/delphi/src/train_truth.rs:659` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/PILOT.md:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/PILOT.md:4` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/README.md:35` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/README.md:349` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/README.md:367` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/README.md:385` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/arms-defs.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/arms-pilot-example.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/e2/data/records/q7-panel-claude-code.md:61` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/e2/data/records/q7-panel-claude-code.md:70` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/e2/data/records/q7-panel-claude-code.md:107` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/e2/data/records/q7-panel-claude-code.md:177` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/e3/PREREG.md:327` | 014 | na | reading | none | "Council 014" is a Council id, not a spec |
| `dev/train/suites/code/cases/agy-builder-ready/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/code/cases/agy-builder-ready/hidden/hidden.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/code/cases/ollama-keyless/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/code/cases/ollama-keyless/hidden/hidden.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/code/cases/snapshot-loaded/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/code/cases/snapshot-loaded/hidden/hidden.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/lions-mane-claim/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/lions-mane-claim/truth/facts.toml:2` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/lions-mane-claim/truth/insights.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/lions-mane-claim/truth/traps.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/lions-mane-claim/truth/views.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/livewire-strategy/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/livewire-strategy/truth/facts.toml:2` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/livewire-strategy/truth/insights.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/livewire-strategy/truth/traps.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/livewire-strategy/truth/views.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/motogp-2027-regs/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/motogp-2027-regs/truth/facts.toml:2` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/motogp-2027-regs/truth/insights.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/motogp-2027-regs/truth/traps.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/motogp-2027-regs/truth/views.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/mycorrhizal-networks/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/mycorrhizal-networks/truth/facts.toml:2` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/mycorrhizal-networks/truth/insights.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/mycorrhizal-networks/truth/traps.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/mycorrhizal-networks/truth/views.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/phantom-paper/case.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/phantom-paper/truth/facts.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/phantom-paper/truth/insights.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `dev/train/suites/research/cases/phantom-paper/truth/views.toml:1` | 037 | 037-training-experiments-quality-and-efficiency | location | keep |  |
| `docs/features.md:23` | 014 | 014-documents-and-dataviz (now 095) | path | MOVE |  |
| `docs/features.md:28` | 014 | 014-relay-and-away-mode | path | keep |  |
| `docs/features.md:84` | 037 | 037-training-experiments-quality-and-efficiency | path | keep |  |
| `docs/guides/away-mode.md:3` | 014 | 014-relay-and-away-mode | path | keep |  |
| `docs/reference/config.md:82` | 061 | 061-shell-tool | keyword | keep |  |
| `docs/reference/config.md:126` | 061 | 061-encyclopedia-search-pages (now 097) | path | MOVE |  |
| `docs/specifications.md:18` | 014 | 014-documents-and-dataviz (now 095) | path | MOVE |  |
| `docs/specifications.md:19` | 014 | 014-relay-and-away-mode | path | keep |  |
| `docs/specifications.md:29` | 037 | 037-console-deck-change-log (now 096) | path | MOVE |  |
| `docs/specifications.md:30` | 037 | 037-training-experiments-quality-and-efficiency | path | keep |  |
| `docs/specifications.md:50` | 014 | meta | reading | none | hand-edited: the sentence about duplicate numbers |
| `docs/specifications.md:50` | 037 | meta | reading | none | hand-edited: the sentence about duplicate numbers |
| `docs/thesis.md:44` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "evaluation work (spec 037)" |
| `llms.txt:51` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `llms.txt:51` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "training experiments (037)" |
| `missions/2026-10-23-foundational-design/build-plan-2026-11-05.md:89` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `missions/2026-10-23-foundational-design/decisions.md:398` | 014 | 014-relay-and-away-mode | reading | keep | "Specs 014 (relay and away mode), 015, 016 approved" |
| `missions/2026-10-23-foundational-design/decisions.md:450` | 061 | 061-shell-tool | keyword | keep |  |
| `missions/2026-10-23-foundational-design/decisions.md:455` | 061 | 061-shell-tool | path | keep |  |
| `missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md:24` | 014 | 014-relay-and-away-mode | path | keep |  |
| `missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md:27` | 014 | 014-relay-and-away-mode | reading | keep | remote recipe: relay |
| `missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md:28` | 014 | 014-relay-and-away-mode | path | keep |  |
| `missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md:31` | 014 | 014-relay-and-away-mode | reading | keep | "specs 014 to 016" approved with the relay amendments |
| `missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md:155` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md:231` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `missions/2026-10-23-foundational-design/knowledge-and-delight-plan-2026-10-28.md:43` | 014 | meta | reading | none | describes this renumbering task |
| `missions/2026-10-23-foundational-design/knowledge-and-delight-plan-2026-10-28.md:43` | 037 | meta | reading | none | describes this renumbering task |
| `missions/2026-10-23-foundational-design/roadmap.md:75` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:17` | 014 | 014-relay-and-away-mode | path | keep |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:17` | 037 | 037-training-experiments-quality-and-efficiency | path | keep |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:90` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:96` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:189` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "Extends 086, 062, 051, 047, 056, 037, 048": the arms |
| `missions/2026-10-23-foundational-design/spec-tracker.md:241` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "measured on the spec 037 harness" |
| `missions/2026-10-23-foundational-design/spec-tracker.md:443` | 061 | 061-shell-tool | keyword | keep |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:466` | 014 | 014-relay-and-away-mode | amend-tag | keep |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:477` | 061 | 061-encyclopedia-search-pages (now 097) | keyword | MOVE |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:562` | 014 | na | reading | none | "Council 014 record" is a Council id, not a spec |
| `missions/2026-10-23-foundational-design/spec-tracker.md:734` | 014 | meta | meta | none |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:782` | 014 | meta | meta | none |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:782` | 037 | meta | meta | none |  |
| `missions/2026-10-23-foundational-design/spec-tracker.md:782` | 061 | meta | meta | none |  |
| `schemas/config.schema.json:116` | 061 | 061-shell-tool | keyword | keep |  |
| `schemas/config.schema.json:190` | 061 | 061-encyclopedia-search-pages (now 097) | reading | MOVE | page cache lifetime the writer proposes |
| `schemas/config.schema.json:191` | 061 | 061-encyclopedia-search-pages (now 097) | reading | MOVE | page cache lifetime the writer proposes |
| `skills/CATALOG.md:129` | 061 | 061-encyclopedia-search-pages (now 097) | keyword | MOVE |  |
| `specs/008-ambient-watch-and-rewind.md:92` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/014-documents-and-dataviz.md:2` | 014 | 014-documents-and-dataviz (now 095) | path | MOVE |  |
| `specs/014-documents-and-dataviz.md:15` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `specs/014-relay-and-away-mode.md:2` | 014 | 014-relay-and-away-mode | path | keep |  |
| `specs/014-relay-and-away-mode.md:15` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/014-relay-and-away-mode.md:47` | 014 | 014-relay-and-away-mode | amend-tag | keep |  |
| `specs/014-relay-and-away-mode.md:50` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/014-relay-and-away-mode.md:131` | 014 | 014-relay-and-away-mode | amend-tag | keep |  |
| `specs/014-relay-and-away-mode.md:135` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/014-relay-and-away-mode.md:179` | 014 | 014-relay-and-away-mode | amend-tag | keep |  |
| `specs/015-provisional-decisions.md:29` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/016-headless-run.md:37` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/016-headless-run.md:46` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/016-headless-run.md:52` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/017-timers.md:70` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `specs/023-org-bundle-standard.md:97` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `specs/027-bundle-authoring-distribution-and-evaluation.md:70` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `specs/032-change-log.md:46` | 037 | 037-console-deck-change-log (now 096) | keyword | MOVE |  |
| `specs/034-narrator-monitoring-panes-spike.md:17` | 037 | 037-console-deck-change-log (now 096) | keyword | MOVE |  |
| `specs/037-console-deck-change-log.md:2` | 037 | 037-console-deck-change-log (now 096) | path | MOVE |  |
| `specs/037-console-deck-change-log.md:15` | 037 | 037-console-deck-change-log (now 096) | keyword | MOVE |  |
| `specs/037-training-experiments-quality-and-efficiency.md:2` | 037 | 037-training-experiments-quality-and-efficiency | path | keep |  |
| `specs/037-training-experiments-quality-and-efficiency.md:15` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/040-validator-role-and-fact-ratification.md:233` | 014 | 014-relay-and-away-mode | reading | keep | "relayable under specs/014" |
| `specs/041-agent-registry-routing-and-role-extensions.md:182` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "using the specs/037 harness" |
| `specs/041-agent-registry-routing-and-role-extensions.md:215` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "specs/037 harness" |
| `specs/043-knowledge-window-search-page-and-growing-corpus.md:84` | 014 | 014-documents-and-dataviz (now 095) | reading | MOVE | PDF best match: document readers |
| `specs/043-knowledge-window-search-page-and-growing-corpus.md:141` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `specs/043-knowledge-window-search-page-and-growing-corpus.md:217` | 014 | 014-documents-and-dataviz (now 095) | reading | MOVE | "spec 014 readers" |
| `specs/045-harness-loop-techniques.md:17` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/045-harness-loop-techniques.md:101` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/046-harness-and-openai-coverage.md:157` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/047-codex-harness-and-coverage.md:34` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "API-key batch style of spec 037" |
| `specs/047-codex-harness-and-coverage.md:133` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/048-harness-benchmarking-and-targeted-evals.md:21` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/048-harness-benchmarking-and-targeted-evals.md:23` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/048-harness-benchmarking-and-targeted-evals.md:37` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/048-harness-benchmarking-and-targeted-evals.md:60` | 014 | 014-documents-and-dataviz (now 095) | keyword | MOVE |  |
| `specs/051-copilot-cli-harness.md:127` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/056-discovered-model-assignment.md:50` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | retrospectives (037) |
| `specs/056-discovered-model-assignment.md:126` | 037 | 037-training-experiments-quality-and-efficiency | reading | keep | "M2, 037": retrospective results |
| `specs/061-encyclopedia-search-pages.md:2` | 061 | 061-encyclopedia-search-pages (now 097) | path | MOVE |  |
| `specs/061-encyclopedia-search-pages.md:16` | 061 | 061-encyclopedia-search-pages (now 097) | keyword | MOVE |  |
| `specs/061-shell-tool.md:2` | 061 | 061-shell-tool | path | keep |  |
| `specs/061-shell-tool.md:16` | 061 | 061-shell-tool | keyword | keep |  |
| `specs/064-knowledge-retrieval-for-agents-and-mcp.md:18` | 061 | 061-encyclopedia-search-pages (now 097) | path | MOVE |  |
| `specs/064-knowledge-retrieval-for-agents-and-mcp.md:71` | 061 | 061-encyclopedia-search-pages (now 097) | keyword | MOVE |  |
| `specs/068-research-capture-and-summary-pages.md:18` | 061 | 061-encyclopedia-search-pages (now 097) | path | MOVE |  |
| `specs/068-research-capture-and-summary-pages.md:27` | 061 | 061-encyclopedia-search-pages (now 097) | keyword | MOVE |  |
| `specs/068-research-capture-and-summary-pages.md:78` | 061 | 061-encyclopedia-search-pages (now 097) | reading | MOVE | hatnote: encyclopedia pages |
| `specs/068-research-capture-and-summary-pages.md:123` | 061 | 061-encyclopedia-search-pages (now 097) | reading | MOVE | valid_until: encyclopedia pages (section 4) |
| `specs/069-workspace-administration-and-operator-assistance.md:18` | 014 | na | reading | none | "Council 014" is a Council id, not a spec |
| `specs/082-orchestrator-outside-delegation-contracts.md:174` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/082-orchestrator-outside-delegation-contracts.md:179` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/083-learning-signals-and-host-diagnostics.md:22` | 037 | 037-training-experiments-quality-and-efficiency | keyword | keep |  |
| `specs/085-fixed-standing-limits-reminder.md:152` | 037 | 037-training-experiments-quality-and-efficiency | path | keep |  |
| `specs/088-spec-compile-and-current-set.md:64` | 014 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:124` | 014 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:124` | 037 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:124` | 061 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:126` | 014 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:330` | 014 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:330` | 037 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:330` | 061 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:333` | 014 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:333` | 037 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:333` | 061 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:382` | 014 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:382` | 037 | exempt | meta | none |  |
| `specs/088-spec-compile-and-current-set.md:382` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:30` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:31` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:32` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:33` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:34` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:35` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:37` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:41` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:57` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:65` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:85` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:85` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:85` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:89` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:89` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:89` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:93` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:93` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:93` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:101` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:105` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:109` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:119` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:119` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:119` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:123` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:127` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:127` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:127` | 061 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:183` | 014 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:183` | 037 | exempt | meta | none |  |
| `specs/090-renumber-the-shared-spec-numbers.md:183` | 061 | exempt | meta | none |  |
| `specs/091-seats-families-and-outside-launch.md:18` | 037 | 037-training-experiments-quality-and-efficiency | path | keep |  |
| `specs/092-operator-authorized-tests.md:25` | 014 | 014-relay-and-away-mode | path | keep |  |
| `specs/092-operator-authorized-tests.md:25` | 037 | 037-training-experiments-quality-and-efficiency | path | keep |  |
| `specs/092-operator-authorized-tests.md:39` | 014 | 014-relay-and-away-mode | amend-tag | keep |  |
| `specs/092-operator-authorized-tests.md:61` | 014 | 014-relay-and-away-mode | keyword | keep |  |
| `ui/src/archives.ts:62` | 061 | 061-encyclopedia-search-pages (now 097) | section | MOVE |  |
| `ui/src/archives.ts:329` | 061 | 061-encyclopedia-search-pages (now 097) | section | MOVE |  |
| `ui/src/fold.ts:13` | 061 | 061-shell-tool | keyword | keep |  |
| `ui/src/fold.ts:26` | 037 | 037-console-deck-change-log (now 096) | reading | MOVE | "One agent's work in progress": the left deck |
| `ui/src/screen.ts:227` | 061 | 061-shell-tool | reading | keep | the "!" block output rows: shell tool (section 6) |
| `ui/src/shell.ts:1` | 061 | 061-shell-tool | location | keep |  |
| `ui/test/pages.test.ts:97` | 061 | 061-encyclopedia-search-pages (now 097) | section | MOVE |  |
| `ui/test/shell-bang.test.ts:1` | 061 | 061-shell-tool | location | keep |  |
