---
state: not-built
date: 2026-11-21
title: "Stage 0 review (2026-11-20) of C1, branch `fix-122-self-install-update`"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The state cell did not map clearly to a state; `not-built` is the closest, and the cell is kept below as it was. The table named this slice `C1 fixes` of spec `122`.

The row (line 1279, under `The Council and the Panel from a host: spec 123 (2026-11-16)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 122 | C1 fixes | Stage 0 review (2026-11-20) of C1, branch `fix-122-self-install-update` | V3-4: `delphi update` from the copy inside `*.app/Contents/` refuses and names `self-install` (§10.1 item 5); test `update_from_inside_the_app_bundle_is_refused_and_names_self_install`. V3-1: `delphi update --home X` reads `X/app/opt-out` (the home is threaded into `selfupdate`); test `update_reads_the_opt_out_of_the_home_it_was_given`. V3-2, V3-3: `self-install` reads the destination's version text and keeps a newer `delphi` unless `--allow-downgrade` (§10.1 item 6); the same program at a non-0755 mode is set to 0755; tests `self_install_keeps_a_newer_delphi_unless_allow_downgrade`, `a_newer_delphi_is_not_replaced_without_allow_downgrade`, `the_same_program_without_the_execute_bit_is_made_executable`. P3 nits: the app and the doctor look in `$DELPHI_INSTALL_DIR` (V3-5); the app counts an installed `delphi` only with an execute bit (V3-6); a link at the opt-out path counts, in `delphi` and `install.sh` (V3-7); the opt-out folder comment says an existing folder keeps its mode (V3-8); the PATH note compares canonical folders (V3-9); the folder is synced after the rename and the module note explains the check-then-rename window (V3-11, V3-12); the app's self-install command refuses DEL and U+2028/2029 (V3-14); a misplaced doc comment moved (V3-15). V3-10 left: the marker and version scan need the destination's bytes, and `==` already compares lengths first |
