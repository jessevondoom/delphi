---
state: built
date: 2026-11-11
title: "full DOI/ORCID/ROR normalisation (optional)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 968, under `Spec 101: library practice for research (2026-11-11, approved)`):

| Slice | Content | Hours (planned) | State |
| :-- | :-- | --: | :-- |
| E | full DOI/ORCID/ROR normalisation (optional) | 6 | built (follow-up PR open): `doi.rs` gains `orcid` (sixteen characters, hyphenated or not, behind `https://orcid.org/` or `orcid:`, the ISO 7064 MOD 11-2 check character verified, one canonical spelling `0000-0002-1694-233X`) and `ror` (nine characters: `0`, six Crockford base32, two check digits `98 - ((n * 100) % 97)`; one canonical spelling, the bare id). The origin count gains `Cite::work` (the source's normalised DOI from `retraction::doi_of`): two citations of the same work, however spelled or hosted, are ONE origin (`corroboration::support`), so it can only lower a count; named tests at the pure function and through `cites_of`. A net test pins that only the matching files name an identifier and that nothing calls `orcid` or `ror` yet (the library records no author or publisher identifier, so they have no data to match). Verified: ROR's identifier page (format, regex, example `02mhbdp94`) and the generator code it links (the checksum), by hand against the example; ORCID's FAQ (last character is a checksum, 10 written `X`, example `…233X`). NOT VALIDATED: ORCID's support page (the ISO 7064 algorithm and that `X` must be a capital) answered 403 to a fetch; the code is checked against two real iDs (the FAQ's and `0000-0002-1825-0097`) and accepts a lowercase `x`, which only widens matching |
