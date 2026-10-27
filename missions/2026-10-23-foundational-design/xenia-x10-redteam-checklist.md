---
id: xenia-x10-redteam-checklist
title: "Xenia X10 red-team sweep checklist (per REVIEW.md)"
author: builder
timestamp: 2026-10-27T12:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [xenia, verification, x10]
---

# Xenia X10: red-team sweep checklist

**Status of the sweep itself: NOT DONE.** This document is the checklist an independent Red-Team reviewer works through. It was written by the Builder of X10, who is not independent of the code, so nothing here is a finding by a critic. It lists every attack class that `specs/011` §9 (threat table) and the X-series notes raise, the tests that already cover it, and an explicit gap where a test is missing or thin. The reviewer loads `REVIEW.md` from `origin/main` (`[REV-SEC-04]`), not from the PR branch, and reports in the contract of REVIEW.md §6.

How to use it: for each class, run the named tests (they all run in `cargo test --workspace`), try to break the *claim in the first column* by a route the tests do not take, and record P0/P1/P2. A row marked **GAP** is a place where the Builder found no test; the reviewer should treat it as unproven, not as safe. Test names are `<path under crates/>::<function>` as in the conformance map, shortened to the function where the file is unambiguous.

Rules from `REVIEW.md` this sweep leans on: `[REV-SEC-01]` taint tracking, `[REV-SEC-05]` credential hygiene, `[REV-SEC-06]` computed taint and tool gating (no agent sets or raises `source_type`), `[REV-SEC-07]` execution containment, `[REV-TRAP-02]` atomic writes.

## 1. Tier spoofing (a record or a person claims more trust than it has)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| A record states its own tier, trust class, `endorsed`, `verified_by_org`, `source_type` | `record_pipeline::each_trust_key_is_rejected_at_the_top_level_and_in_every_nested_map`; `bundle_tests::a_bundle_cannot_state_its_own_standing`; `bundle_xenia_tests::nothing_in_a_record_or_a_bundle_can_state_its_own_level`; fuzz `record_fuzz::mutated_records_never_panic_and_nothing_forbidden_is_ever_accepted` (oracle rejects any trust key) | |
| Self-endorsement, also from a second key, a second install, or after rotation | `tier_tests::self_endorsement_is_ignored_even_from_another_install_or_after_a_rotation`; `xenia_member_tests::nobody_endorses_their_own_record_from_any_of_their_keys` | |
| Endorsement by a non-steward, a steward outside the area, an admin as such | `tier_tests::a_member_an_admin_or_a_steward_outside_the_area_cannot_endorse` | |
| Endorsement stays on after the bytes change | `tier_tests::an_endorsement_binds_to_the_exact_revision_and_bytes` | |
| A claimed `source_ceiling` or `confidence: verified` lifts the record | `tier_tests::the_source_ceiling_clamps_even_an_endorsed_record`; `xenia_index_tests::a_record_claiming_verified_confidence_imports_as_medium` | |
| A pointer (`evidence`) lends credibility | `tier_tests::evidence_pointers_alone_change_no_tier` (new in X10) | |
| An agent or tool writes the zone or the tier | `xenia_scope_tests::no_agent_tool_reaches_the_xenia_surfaces_or_the_signing_actions`; `xenia_read_tests::a_row_with_a_tier_a_record_cannot_have_or_outside_its_store_is_refused_and_changes_nothing`; `xenia_read_tests::a_file_sync_and_a_reindex_leave_the_zone_alone` | |
| A stale freshness statement keeps L2 alive | `xenia_read_tests::an_l2_whose_statement_lapsed_is_not_shown_to_an_agent_even_if_nothing_re_derived_the_zone`; `xenia_index_tests::a_lapsed_statement_demotes_l2_to_l1_with_nothing_but_the_clock` | |
| Look-alike names (mixed-script confusables in a title or handle) pass for a trusted person | `record_attacks::homoglyphs_and_normalization_tricks_do_not_pass_for_the_real_thing` (NFC, invisible and bidi characters, trust-key spellings) | **GAP (known, X6 weakness 6):** mixed-script confusables in titles and handles are not flagged. Identity is the DID, not the handle; the console display is the exposure. |
| Collusion: a steward endorses a friend's record (area shopping) | Documented as unmitigated by design (`specs/011` §9 T10). No test is possible for a social attack. | **GAP by design**; dogfood (X11) is where it is measured. |

## 2. Replay (a valid old artefact is presented again)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| An older authentic freshness statement of the same epoch | `xenia_sync_tests::replaying_an_older_authentic_statement_of_the_same_epoch_counts_no_endorsement_and_cannot_move_the_mark`; `freshness_tests::a_replay_cannot_move_the_mark_back_and_assess_marks_it_stale`; `freshness_tests::issued_at_only_goes_forward` | |
| A statement for another epoch, store or org | `freshness_tests::another_store_org_epoch_or_epoch_hash_is_ignored` | |
| A record copied to another store, renamed, moved to another kind | `record_pipeline::a_record_copied_renamed_moved_or_presented_under_another_key_fails_signature_verification` | |
| An old roster signature or epoch file replayed | `roster_tests::a_gap_a_reorder_and_a_replay_are_refused`; `roster_tests::a_signature_over_different_bytes_epoch_or_store_counts_for_nothing` | |
| An old admin proposal or signed publish delivered after the head moved | `xenia_admin_tests::an_old_proposal_replayed_after_the_head_moved_is_stale`; `a_signed_publish_is_not_delivered_after_a_different_epoch_took_the_number`; `a_signed_refresh_is_not_delivered_once_the_roster_has_moved_on` | |
| A bundle at an older rev or version offered as an update | `bundle_xenia_tests::an_older_rev_a_lower_version_and_a_rewound_store_are_replays_not_updates` | |
| A release signature reused for a bundle (cross-domain) | `release_sig::a_signature_made_for_anything_else_never_passes_as_a_release_signature`; `bundle_sig::a_changed_name_org_version_manifest_or_key_each_fail` | |

## 3. Rewind (the store goes back in time, or an old state is served)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| The host serves an older head than the install accepted | `xenia_store_tests::a_host_that_goes_back_in_time_is_reported_as_behind`; `xenia_sync_tests::a_host_going_back_in_time_is_refused_and_cannot_be_recovered_without_a_statement` | |
| A chain shorter than the pins | `roster_tests::a_chain_shorter_than_the_pins_is_a_stale_view_and_is_refused` | |
| A hidden newest epoch behind a still-valid old statement | `xenia_sync_tests::a_hidden_newest_epoch_leaves_an_old_statement_stale_and_a_pinned_install_refuses_the_old_view` | An install that has *never* seen the newer epoch cannot know; the statement's expiry (cap 7 days default) bounds the window. This is the design, stated in `specs/011` §4. |
| A record reverted to its older valid rev | `xenia_sync_tests::a_revert_to_an_older_valid_rev_is_tampering_on_both_kinds_of_install`; `a_lower_rev_smuggled_into_a_later_commit_is_caught_by_the_walk_though_the_tip_looks_higher`; `a_delete_then_restore_in_two_commits_is_tampering_though_the_tip_matches_what_was_stored` | |
| A rollback hidden in a merge commit | `xenia_sync_tests::a_deletion_hidden_in_a_merge_commit_is_seen_against_each_parent` | |

## 4. Key rotation and removal abuse

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| A retired or removed key keeps publishing, endorsing, signing epochs or statements | `xenia_admin_tests::a_retired_key_can_neither_publish_nor_refresh`; `after_a_compromise_rotation_the_old_key_cannot_sign_the_next_epoch`; `tier_tests::a_retired_removed_or_stranger_signer_endorses_nothing_but_a_retired_dispute_still_flags` | |
| A rotation that changes the person (account takeover through the roster) | `roster_tests::a_successor_with_another_person_is_refused`; `a_person_cannot_be_swapped_by_moving_a_key_to_a_different_person`; `changing_a_persons_identifier_is_refused` | |
| A compromised key's records stay trusted | `xenia_admin_tests::a_compromise_rotation_voids_tombstones_and_says_what_to_do_about_the_old_keys_records`; `xenia_member_tests::a_compromise_rotation_is_never_re_signed_automatically` | **GAP (inherent):** between the theft and the admin quorum's compromise epoch, the stolen key is a valid key. No test can close that window; §8.2 of the spec describes the response. |
| A new admin approves their own appointment; one admin takes over by changing the quorum base | `roster_tests::a_new_admin_cannot_approve_their_own_appointment`; `a_quorum_computed_from_the_new_epoch_would_let_one_admin_take_over` | |
| The last admin removed, or the admin floor lowered to deadlock | `xenia_admin_tests::the_last_admin_cannot_be_removed_and_the_floor_holds_for_the_second`; `roster_tests::the_roster_always_keeps_an_active_admin` | A two-admin org with a lost key deadlocks by design (§8.4); the drill covers the re-pin path. |
| `rotate` without saying which kind | `xenia_admin_cmd_tests::rotate_needs_exactly_one_kind_and_changes_nothing_without_it` | |

## 5. Tombstone abuse

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| A stolen key withdraws records | `tier_tests::a_stolen_keys_tombstone_hides_until_voids_tombstones_is_set_and_never_restores` | |
| A far-future ULID wins ordering; several tombstones by one signer | `tier_tests::stewards_in_disagreement_make_it_contested_and_a_far_future_ulid_gains_nothing`; `one_signer_has_one_tombstone_per_target_and_the_smallest_ulid_survives` | |
| A steward hides records outside their areas, or a member hides another's | `tier_tests::only_the_author_or_a_covering_steward_may_tombstone`; `xenia_member_tests::who_may_withdraw_a_covering_steward_the_author_and_nobody_else` | |
| An author retags after a steward's tombstone to escape it | `tier_tests::the_area_check_for_a_tombstone_uses_the_union_of_tags_over_all_revisions`; `xenia_index_tests::an_author_who_retags_after_a_sync_cannot_escape_the_steward_who_covered_the_old_tag` | |
| Deleting a tombstone from the store | `xenia_sync_tests::deleting_a_tombstone_an_endorsement_withdraw_a_dispute_or_a_roster_file_is_tampering` | |
| Resurfacing a hidden record by a rotation or `republish --resign` | `xenia_member_tests::a_memory_a_steward_withdrew_is_not_resurfaced_by_its_authors_rotation`; `tier_tests::nothing_hidden_by_its_author_returns_to_active` | |
| A steward with an area hides everything in it (denial of service by an authorised actor) | Documented residual (`specs/011` §9 T10); remedy is removal by admins. | **GAP by design.** |

## 6. Fork (two histories claim the same position)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| Two files for one roster epoch number | `roster_tests::a_fork_two_files_for_one_epoch_number_is_refused`; `xenia_store_tests::an_immutable_roster_epoch_replaced_under_the_same_number_is_equivocation`; `xenia_sync_tests::the_pins_learn_new_epochs_and_a_forked_epoch_is_refused` | |
| Two competing, validly signed proposals for one epoch: only one wins, the other never reaches main | `xenia_admin_cmd_tests::a_three_admin_two_of_three_chain_survives_a_fork_and_an_install_that_slept_through_four_epochs` (CLI: second proposal refused, second publish stale, rival history on a second host refused by an install that accepted the first, rival absent from main); `xenia_admin_tests::a_second_proposal_for_the_same_epoch_is_refused_not_overwritten`; `a_signed_publish_is_not_delivered_after_a_different_epoch_took_the_number` | An install that has *not yet* seen either epoch and is first served the rival accepts the rival (first seen wins per install). Two installs can therefore disagree until a human compares epoch hashes. The pin handed out at join (`epoch:hash`) is the control. **GAP: no test of two never-synced installs diverging; it is the documented design (§3.3).** |
| A fork of a record (same id, same rev, different bytes) | `xenia_sync_tests::the_same_rev_with_a_different_hash_is_tampering_and_the_same_hash_is_idempotent`; `xenia_store_tests::the_same_rev_with_different_bytes_is_a_swap` | |
| Two files for one path/id across kinds | `tier_tests::an_id_under_two_kinds_is_refused_in_every_kind` | |

## 7. Store rewrite (force push, squash, history surgery)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| A force-pushed store | `xenia_sync_tests::a_force_pushed_store_is_refused_by_a_normal_sync`; `xenia_store_tests::a_rewritten_history_is_detected_refused_and_never_applied` | |
| A rewrite recovered with one admin, two keys of one person, a partial approval, an expired statement, or one naming another head | `xenia_sync_tests::a_single_admin_statement_is_refused_and_so_are_two_keys_of_one_person`; `a_partial_approval_applies_nothing`; `an_expired_statement_or_one_naming_another_head_is_refused`; `an_approval_is_bound_to_the_head_the_operator_saw` | |
| A rewrite that changes a pinned epoch or the creator of an id | `xenia_sync_tests::a_rewrite_cannot_change_a_pinned_epoch`; `a_rewrite_that_changes_the_creator_of_an_id_lists_that_path` | |
| The §8.4 procedure end to end on a scratch store | `xenia_sync_tests::xenia_recovery_drill` (ignored; run by hand; transcript in `xenia-x10-verification.md`) | The CLI flag `sync --accept-rewritten-head` is not wired (X9 note); the drill and tests use the library. **GAP: no CLI-level test of recovery.** |
| An admin squashes history *before* signing the statement, so the old revisions never existed for a fresh install | None. Documented as known weakness 3 of X6. | **GAP.** A fresh install trusts the ancestry the signed `store_head` contains. |
| A tampered fast-forward (history walk fails) after a refresh | `xenia_admin_tests::refresh_over_a_tampered_history_is_refused_and_nothing_is_signed`; `the_explicit_walk_does_not_trust_the_installs_own_accepted_state` | |

## 8. Hostile trees and paths (the store is untrusted bytes)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| Symlinks, submodules, executables, `.git*` names, huge blobs (also hidden in history), too many files or commits, fetch past the byte cap | `xenia_store_tests::a_symlink_in_the_tree_is_refused`, `a_submodule_and_a_gitmodules_file_are_refused`, `an_executable_file_is_refused`, `a_huge_blob_is_refused_and_removed`, `a_huge_blob_hidden_in_history_and_deleted_at_the_tip_is_still_refused`, `a_fetch_that_outgrows_the_byte_cap_is_cut_off`, `too_many_files_and_too_many_commits_are_refused`; `store::trees_refuse_symlinks_submodules_executables_and_git_names` | |
| Hooks, filters, `fsmonitor`, `ext::` transport, config in the clone | `xenia_store_tests::a_hostile_store_with_a_hook_a_filter_a_submodule_a_symlink_and_a_huge_blob_runs_nothing`; `hooks_filters_and_fsmonitor_in_the_clone_configuration_do_not_run`; `the_ext_transport_never_runs_even_if_a_url_got_through`; `the_git_command_reads_no_configuration_but_its_own`; `only_the_transport_the_store_url_needs_is_open` | |
| Branch or URL names that smuggle git options | `xenia_store_tests::branch_names_cannot_smuggle_options_or_refs`; `pins_tests::urls_that_could_become_git_options_credentials_or_markup_are_refused` | |
| A symlinked store or key directory; a planted symlink where a state file goes | `xenia_store_tests::a_symlinked_store_directory_is_refused`, `a_planted_symlink_in_place_of_the_clone_is_refused`; `identity::a_symlinked_key_path_is_refused`; `pins_tests::write_atomic_never_follows_a_planted_symlink` | |
| Path traversal in a record path or a store-name | `store::org_and_store_cannot_escape_the_directory`; `store::paths_classify_to_exactly_the_layout`; `record_pipeline::a_file_at_b_whose_frontmatter_id_is_a_is_rejected_even_with_a_valid_signature_for_b` | |
| A hostile proposal branch with a fully signed but hostile tree never reaches main | `xenia_admin_tests::a_hostile_tree_on_a_fully_signed_proposal_branch_never_reaches_main`; `an_epoch_file_that_is_a_symlink_is_not_a_proposal`; `an_oversize_object_on_a_proposal_branch_is_refused_before_anything_is_read` | |
| A hostile bundle tree (links, odd paths, oversize, wrong hash) | `bundle_xenia_tests::hostile_trees_symlinks_submodules_oversize_files_and_bad_paths_are_refused_and_leave_nothing`; the archive tests in `delphi-bundle/src/archive.rs` (`links_directories_devices_and_extensions_abort_at_that_entry`, `dot_dot_absolute_and_odd_names_are_refused_before_anything_is_created`) | |
| CRLF and filter tricks changing the bytes that are signed | `xenia_store_tests::records_with_crlf_bytes_read_back_exactly_even_when_the_clone_says_autocrlf`; `a_gitattributes_that_converts_line_endings_is_refused` | |
| Terminal/console injection through a record's text on the approval card or status | `xenia_action_tests::control_characters_are_shown_as_escapes_so_a_card_cannot_redraw_itself`; `xenia_admin_card` hostile-text tests; `xenia_status_hardening_tests::ten_thousand_hostile_entries_print_and_serialize_a_few_clean_lines` | |
| Operating system without POSIX permissions or symlink semantics (Windows) | None. | **GAP:** the Xenia code is exercised on macOS and Linux only; no Windows run. |

## 9. Parser differential (two parsers disagree about the same bytes)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| YAML features (anchors, aliases, merge keys, tags, multi-document, flow styles) | `frontmatter::yaml_features_are_refused`; `record_attacks::a_yaml_alias_bomb_is_refused_at_the_first_anchor_and_costs_nothing`, `a_second_document_in_the_body_adds_no_keys_and_a_second_frontmatter_block_inside_is_refused` | |
| Duplicate keys (a lenient parser takes the last, a strict one refuses) | `record_pipeline::unknown_duplicate_and_yaml_feature_keys_are_rejected_with_specific_reasons`; `roster_tests::duplicate_keys_are_refused_where_a_lenient_parser_takes_the_last`; `sidecar_fuzz::the_sidecar_parser_is_total_sound_and_round_trips_over_forty_thousand_mutants` (duplicate keys counted by an independent visitor) | |
| Line endings, BOM, overlong or invalid UTF-8, NFC vs NFD, bidi, tag characters | `record_attacks::line_ending_tricks_are_refused_and_lf_is_the_one_spelling`, `overlong_and_otherwise_invalid_utf8_is_refused_even_when_validly_signed`, `invisible_and_reordering_characters_are_refused_in_every_position`; `frontmatter::non_nfc_text_is_refused_and_the_nfd_form_really_differs` | |
| The verifier and the signer disagree about the signed bytes | `serialize_fuzz::everything_the_serializer_emits_validates_round_trips_and_has_one_spelling`; `the_verifier_agrees_with_every_refusal_that_can_be_written_by_hand`; `redaction_hash::the_recipe_is_stable_and_domain_separated` | |
| The two content scanners disagree (bundle scan vs record scan) | `delphi-bundle/tests/xenia_scan_agreement.rs::both_scanners_find_every_canary_and_neither_finds_prose` | |
| A second, independent implementation of the verifier agrees on every input | None: there is one verifier. | **GAP:** differential testing against an independent implementation (another language) is not possible until one exists. The Ed25519 vectors are the only cross-implementation check (`sig::rfc8032_vectors_pass`, `non_canonical_s_is_refused`, `small_order_keys_are_refused`). The same-results-on-every-platform clause of 012 test 20 relies on CI's platform matrix, not on a test that compares outputs. |
| YAML parsed before signature | `record_pipeline::the_yaml_parser_never_runs_for_an_oversized_file_a_bad_signature_or_a_non_active_signer`; `sidecar_fuzz::a_mutated_record_or_sidecar_never_reaches_the_yaml_parser_unless_steps_one_to_three_pass` (12,000 mutated inputs, instrumented parser count) | |

## 10. Bundle record abuse (B8, `specs/012` §11, `specs/024`)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| A bundle states its own level; memory fields on a bundle and the reverse | `bundle_tests::a_bundle_cannot_state_its_own_standing`; `memory_fields_on_a_bundle_and_bundle_fields_on_a_memory_are_unknown_keys` | |
| The tree pointer is swapped after the scan | `bundle_tests::the_redaction_hash_covers_the_tree_pointer_so_a_swapped_locator_cannot_keep_a_passed_scan` | |
| The tree is a floating ref, not an object id, or not https git | `bundle_tests::the_tree_pointer_is_an_https_git_pointer_pinned_by_a_full_object_id` | |
| A changed byte in the pinned tree, a lying hash, a moved head | `bundle_xenia_tests::a_changed_byte_in_the_pinned_tree_a_lying_hash_or_a_moved_head_adds_nothing` | |
| A tree signed by someone else, by no one, or for another org or name | `bundle_xenia_tests::a_tree_signed_by_someone_else_or_by_no_one_or_for_another_org_or_name_is_not_the_published_bundle` | |
| An update that silently raises or lowers the level, changes the publisher, or follows a withdrawn record | `bundle_xenia_tests::automatic_updates_never_lower_the_level_change_the_publisher_or_follow_a_withdrawn_record`; `another_member_publishing_the_same_bundle_under_their_own_key_is_a_publisher_change` | |
| Endorsement carries to a new rev; self-endorsement; stale store | `bundle_xenia_tests::a_new_rev_is_an_update_the_old_endorsement_does_not_carry_and_the_old_version_stays_in_history`; `a_stale_store_endorses_nothing_and_sync_brings_the_lock_down_with_it`; `tier_tests::nobody_endorses_their_own_bundle_and_a_stale_store_endorses_nothing` | |
| A secret in the bundle's text on publish | `xenia_bundle_tests::a_secret_in_a_bundles_text_blocks_the_publish_before_any_preview_and_is_never_echoed` | |
| A bundle and a memory share an id; a non-member publishes | `tier_tests::a_bundle_and_a_memory_cannot_share_an_id_and_a_non_member_publishes_nothing`; `xenia_bundle_tests::only_the_owner_publishes_a_new_rev_and_an_id_cannot_change_kind` | |
| Withdrawn bundle still offered | `xenia_bundle_tests::the_author_and_a_covering_steward_withdraw_a_bundle_and_a_withdrawn_one_is_not_offered` | |
| An endorsed bundle's *content* is hostile (prompt injection in a skill) | Not a Xenia-layer property: the install gate (`specs/024`) and the taint lattice are the controls; the scanner is defence in depth. | **GAP by design:** a steward who endorses a hostile bundle has endorsed it. |

## 11. Trusted-source anchors (#251, `specs/024` §3.7-3.8)

| Attack | Covered by | Gap |
| :-- | :-- | :-- |
| A bundle or a bundle tree grants itself trusted-source status | `bundle_xenia_tests::nothing_in_a_record_or_a_bundle_can_state_its_own_level`; `bundle_trust` tests in `delphi-core/src/bundle_trust.rs` (a grant needs a publisher key, and the grant text states what it does) | |
| An unsigned bundle anchors a trusted source | `delphi/src/bundle_trust.rs::an_unsigned_bundle_cannot_anchor_a_trusted_source` | |
| A grant or a raise without the operator at a terminal (flags, environment, pipe) | `delphi/src/bundle_trust.rs::the_real_confirmation_refuses_without_a_terminal_and_so_do_the_flags`, `the_wrong_word_cancels_and_the_right_one_raises_and_default_lowers_without_asking` | |
| A grant leaks to other origins, other keys, or other bundles; a revoke lowers more than it should | `delphi-core/src/bundle_trust.rs::granting_raises_the_current_bundles_of_that_origin_and_key_only_and_revoking_lowers_just_those`; `an_unknown_bundle_or_origin_is_an_error_and_changes_nothing` | |
| A new version of an anchored bundle under a different publisher key keeps the anchor | `bundle_xenia_tests::another_member_publishing_the_same_bundle_under_their_own_key_is_a_publisher_change` | **Reviewer: confirm** that the publisher-change test also drops a *trusted-source* raise, not only the store-endorsed level. The Builder did not read this path line by line. |
| An agent writes `local/bundles.lock` | `xenia_scope_tests::no_agent_tool_reaches_the_xenia_surfaces_or_the_signing_actions` covers the Xenia surfaces; the lock path is covered by the ordinary write-glob rules (`[REV-SEC-02]`). | **GAP:** no test names `local/bundles.lock` as unwritable by an agent tool. |

## 12. Other classes the sweep should still try

| Class | Covered by | Gap |
| :-- | :-- | :-- |
| Publish-side exfiltration (a secret or operator-scope text sent to the store) | `xenia_publish_tests` (canaries in summary, body and pointer labels; operator scope never previewed; a changed byte aborts) | The scanner is a denylist ("known weakness 5"); the control is the approval card and the taint gate. |
| An approval given for other bytes, or by a script | `xenia_publish_tests::an_approval_for_other_bytes_signs_nothing`; `xenia_admin_cmd_tests::nothing_but_a_terminal_answers_for_the_operator`; `a_hostile_proposal_is_shown_with_its_warnings_and_cannot_be_signed_unread` | |
| The gate starts a model turn (an agent approving its own publish) | `xenia_publish_tests::the_gate_cannot_start_a_model_turn_by_construction`; `xenia_admin_tests::the_gate_cannot_start_a_model_turn_by_construction` | |
| Pointer fetch / SSRF | `no_fetch::no_xenia_source_names_a_network_api`; `xenia_sync_tests::a_pointer_is_never_fetched_a_listener_on_its_target_sees_no_connection` (X10) | **GAP:** no OS-level network-denied run. |
| Spotlighting escape: a record body closes the envelope | `xenia_ingress::an_envelope_cannot_be_closed_or_reopened_by_its_text`; `xenia_read_tests::hostile_text_stays_inside_the_envelope_and_a_body_never_reaches_a_preview` | |
| Tainted span writes or calls out | `xenia_read_tests::a_span_that_quarantine_reads_an_l1_record_is_gated_and_one_that_reads_an_l2_summary_is_not` (X10) | The gate returns Deny for a tainted worktree write and for outbound; the spec's "without a Red approval" is the weaker wording. |
| Signing key at rest | `identity::a_key_others_can_read_is_refused`, `damaged_and_foreign_files_are_refused_and_never_replaced`, `debug_never_shows_the_seed` | No test of a key on a network filesystem or in a backup. |
| Clock manipulation (install clock set back to keep a statement alive) | `freshness_tests::expiry_is_judged_by_the_install_clock`; `issued_at_only_goes_forward` (the mark never moves back) | **GAP:** an attacker who controls the install's clock controls expiry; out of scope for Phase 1 (the install is trusted). |
| Throughput: a store with thousands of commits makes sync slow | `xenia_store_tests::too_many_files_and_too_many_commits_are_refused` (caps) | **GAP:** R3 (cache the last accepted head, walk only the delta) is unbuilt, and the plan asked X10 to measure it; not measured. The caps bound the cost. |
