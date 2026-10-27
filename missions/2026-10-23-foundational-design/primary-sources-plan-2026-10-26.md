---
id: primary-sources-plan
title: "Primary sources for truth packs, and a source registry the validator and every agent can use"
author: architect
timestamp: 2026-10-26T23:00:00Z
status: draft
source_type: internal_generated
confidence: medium   # the candidate primary sources below were found by search snippets only; no page was read in full
source_mission: 2026-10-23-foundational-design
tags: [training, truth-packs, sources, validation]
---

# Primary sources: a plan

Asked for by the operator on 2026-10-26: find a primary source for each truth pack (for example motogp.com for the MotoGP
rules), use primaries as the first sources for fact-checking articles, build up an understanding of reputable sources over
time, and make "identify the primary source" a skill for the validator and for every agent when it is needed. The operator
will re-evaluate the packs when there is a good plan.

## 1. What the five packs stand on today

| Pack | Sources now | Tier today | Primary source that exists (found, not yet read in full) |
| :--- | :--- | :--- | :--- |
| `motogp-2027-regs` | crash.net, cycleonline.com.au, tyrepress.com | all press (secondary) | motogp.com announcement of 2024-05-06 and its FAQ; Grand Prix Commission decisions on motogp.com and fim-moto.com |
| `livewire-strategy` | motorcycle.com (one article) | press (secondary) | LiveWire investor site release of 2026-07-23; the same release as Exhibit 99.1 to the Form 8-K on SEC EDGAR (CIK 1898795); the 10-Q for the figures |
| `lions-mane-claim` | six journal DOIs, one a medRxiv preprint | primary (the papers themselves) | add a trial-registry entry per trial and a retraction/correction check (Crossref, PubMed) |
| `mycorrhizal-networks` | four journal DOIs | primary | same check for corrections and retractions |
| `phantom-paper` | one PMC record of a search | primary for "not found", but the procedure is not recorded | Crossref and PubMed queries with the query text and date, plus the journal's own table of contents |

Two packs are built on press coverage of an official record, and that already shows the cost. The MotoGP pack has no fact
about wildcards, yet motogp.com reports that the Grand Prix Commission decided in spring 2026 to ban wildcards in the MotoGP
class from 2027. The press articles the pack was built from did not carry it. A primary is not only more reliable; it is where
amendments appear first.

## 2. A tier is a property of a source for a kind of claim

"motogp.com is a primary source" is wrong as a bare statement. It is the rule-maker's own announcement for what the rules are,
and press for anything else. So the registry records, per source, **what it is primary for**.

| Tier | Meaning | Examples |
| :--- | :--- | :--- |
| P1 primary | the originator of the claim, speaking about its own act | series and federation (rules), the filer (SEC filings, investor releases), the authors (the paper), a regulator (the register) |
| P2 authoritative secondary | independent, accountable, and cites its primaries | systematic reviews, wire services with a corrections policy, a standards body's summary |
| P3 secondary | reports and interprets | trade and general press |
| P4 unvetted | promotional, anonymous, aggregated, or unknown | marketing pages, forums, content farms |

A P1 source can still be wrong or out of date (rules are amended, filings are restated). The registry carries the date a
claim was last checked, and a check at use time notes whether a later amendment exists.

## 3. The registry

`context/sources/registry.toml`, a release file like the other context files, so a signed release ratifies it
(`specs/006` §3.7) and `ratified.lock` pins it. Entries:

```toml
[[source]]
host = "motogp.com"                     # or a path prefix, or a DOI prefix / API such as api.crossref.org
publisher = "Dorna Sports / MotoGP"
primary_for = ["MotoGP technical and sporting regulations as announced", "Grand Prix Commission decisions"]
tier = "P1"
not_primary_for = ["what the rules will do to competitiveness", "rider and team commentary"]
decided_by = "release"                  # release | operator | validator
decided_on = "2026-10-26"
basis = "the rule-maker's own site, linked from the FIM's regulations page"
```

- **Seed:** the hosts above, plus `fim-moto.com`, `sec.gov`, `investor.livewire.com`, `doi.org`, `crossref.org`,
  `pubmed.ncbi.nlm.nih.gov`, `ncbi.nlm.nih.gov/pmc`, `clinicaltrials.gov`, `medrxiv.org` (P1 for "this preprint says", flagged
  not peer reviewed).
- **Growth:** an agent that meets an unknown host classifies it by the skill's rules and, if it is useful, writes a
  `propose` entry (the existing channel, `specs/001` §2.2). Under the operator's rule of 2026-10-26 a source the validation
  skill deems reputable counts as ratified, so a `validator` entry takes effect without a `/retro` review. Its `basis` is
  recorded, it is listed in `delphi sources list`, and the operator can strike it. An entry never raises a source's tier
  above what its `basis` supports, and nothing in the page being fetched can write an entry (fetched text is data).
- **Scopes:** the release seed plus operator, team and workspace layers, merged like the other scoped files
  (`specs/004` §5); a lower layer may add and may tighten, never widen a release entry.

## 4. The skill

Extend `skills/primary-source-verification` (today `authorized_agents: [investigator]`):

1. New first step, **identify the primary**: restate the claim, name its kind (a rule, a number a company reported, a
   published finding, an existence claim), look the claim kind and host up in the registry, and if there is no entry,
   work out who originated the claim and find their own publication of it.
2. **Check an article against its primary:** extract each checkable claim, match it to the primary's passage, report
   *supported / contradicted / not in the primary*, and flag where the article goes beyond its source (interpretation stated as
   fact). That is the fact-checking mode the operator described.
3. **Existence claims** (a paper, a filing, a rule) record the procedure: the database, the exact query, the date, so a
   "not found" can be reproduced.
4. **Record the tier** of every source used, and write an unknown-host `propose` entry when it helps.
5. Output gains `primary_source`, `tier`, and `secondary_only: true` when no primary could be found.

Who may load it: today only the investigator, which is the nearest thing to the "validator" the operator named; no role is
called validator. Recommendation: **authorize it for all nine roles** (an agent that cites an external fact is a
consumer, and a rule that only one role may verify makes the others skip it), keep `network:gated`, and have the
investigator remain the role that runs it as its main job. This changes a ratified skill, which `specs/001` §2.3 lets the
operator do.

## 5. A deterministic check

`delphi check sources <path>` (and the matching read-only tool for roles with no shell), in the same family as
`check dag` or `check spec`, so the skill names it:

- every source in a truth pack's `sources.toml` carries `publisher` and `tier`, and its host is in the registry or the
  entry is a warning with the proposal to make;
- every fact's `source` field resolves, and a fact whose only source is below P1 is listed as **secondary-only**;
- retrieved dates are present and not after `as_of`.

`delphi train verify` then reports, beside its present message, how many facts rest on a primary. A pack with secondary-only
facts verifies only with `--allow-secondary` (the same shape as `--allow-unverified`), so the choice is explicit.

## 6. Plan for the packs

Order by gain. Each step rebuilds an extract under `truth/sources/` and **makes the pack's stamp stale**, so the operator
re-verifies once per pack, which is the re-evaluation they offered.

1. **`motogp-2027-regs`:** replace the three press sources with the 2024 announcement, the FAQ and the GPC decisions
   (motogp.com, fim-moto.com); keep one press article only as an *article to check*, and add the wildcard-ban fact.
2. **`livewire-strategy`:** primary figures from the 2026-07-23 release (investor site, EDGAR copy) and the 10-Q; keep the
   motorcycle.com article as the article to check. Every number in `facts.toml` re-sourced.
3. **`phantom-paper`:** record the Crossref and PubMed queries as the procedure and add the journal's contents page.
4. **`lions-mane-claim`:** add registry entries per trial, a corrections check for each DOI, and say in `sources.toml` that
   the medRxiv item is a preprint.
5. **`mycorrhizal-networks`:** corrections check for each DOI; Klein 2016 and Karst 2023 are critiques, so mark them as
   "primary for their own argument".

## 7. Build order

| Slice | What | Size |
| :--- | :--- | :--- |
| S1 | registry file, parser, seed, `delphi sources list|show`, lint | small |
| S2 | `delphi check sources`, `truth-sheet` shows tiers, `verify --allow-secondary` | small to medium |
| S3 | skill rewrite, widen `authorized_agents`, `propose` entry for new hosts, skill-trigger eval cases | small |
| S4 | rebuild the MotoGP and LiveWire packs from primaries (needs the operator's re-verification) | medium, mostly reading |
| S5 | the other three packs' small changes | small |

S1–S3 do not touch a pack, so the existing stamps stay valid until S4.

## 8. Decisions for the operator

1. **Who is the validator?** The plan uses the investigator and widens the skill to every role. A separate `validator` role
   would be a ninth-to-tenth role and a governance change; the plan does not recommend it.
2. **May an agent's registry entry take effect without review?** The plan says yes under the 2026-10-26 rule, with the entry
   visible and strikable. The stricter alternative queues entries for `/retro`.
3. **Store the fetched primaries?** The packs already keep extracts under `truth/sources/`; the plan keeps that, quoting
   only what a fact needs, to stay clear of copying whole pages.

## 9. Risks

- A fetched page is untrusted text. The skill's existing rule (delimit it, never follow instructions inside it) stands, and
  an entry is written from the skill's judgement, never from page text.
- Sites that render by script or sit behind a paywall cannot always be read; the skill records "unverifiable" rather
  than falling back to press quietly.
- Reputable is not true: the registry says who to ask first, and a primary that contradicts another primary is reported,
  not resolved by tier.
- The candidates in section 1 come from search snippets. S4 begins by reading each page in full.
