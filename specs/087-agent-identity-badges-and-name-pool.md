---
id: spec-087-agent-identity-badges-and-name-pool
title: "087: Agent identity: a distinct badge and light/dark color for every role, a name pool for dynamic agents, and a visible signal outside the console"
author: architect
timestamp: 2026-11-06T12:00:00Z
status: approved   # by the operator, 2026-11-06 (answers to OD-1 and OD-5 given; OD-2 to OD-4 take the recommendations, §10)
approved_by: operator (2026-11-06)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code claims checked against main (45a445b) on 2026-11-06; the effect on the operator's understanding is not measured (§7)
source_mission: 2026-10-23-foundational-design
tags: [spec, identity, badge, avatar, theme, dynamic-agents, mcp, outside]
---

# Spec 087: Agent identity

> **Status: approved by the operator, 2026-11-06.** Written from an operator conversation on 2026-11-06. The operator chose the 28 badge faces (30 offered, #2 and #14 dropped as too wide), kept all 36 names, and approved the twin and core-role proposals in §3. Decisions OD-1 and OD-5 were answered (§10); OD-2 to OD-4 take the recommendations. Slices follow §9, one per pull request. Nothing here is built.
> **Extends:** [`specs/005a`](005a-terminal-avatar-system.md) (badges and mascots), [`specs/073`](073-console-adaptive-contrast-and-dual-mode-palette.md) (light and dark palettes), [`specs/066`](066-mcp-research-endpoint-and-role-identity.md) §7 and [`specs/072`](072-host-cognitive-parity-and-external-orchestration.md) §3.1 (the signature line outside).
> **Number:** 086 is a draft in PR #498, so this is 087.

---

## 1. Scope and objectives

Several agents answering one question improves quality and puts a specialist on each issue. It also does something else: the operator reads *who* is speaking to learn that the framing has changed, which concern is being raised, and how the agents are interacting. When names and badges stay visible, that context carries through a long exchange. When they do not, the operator sees one voice.

Today that signal is weak, and weakest outside the console (verified on main, §2). This spec does five things:

1. **Every core role gets its own badge face and light/dark color** (§3.1), including three roles that have none and an investigator that borrows the scout's. The writer and the investigator are twins and look it.
2. **Per-role blink frames** (§3.2), so a blink is a gesture of the role and not one shared dash.
3. **A pool of 36 Greek names, each with keywords and a badge face** (§4), for the one-off agents Delphi spawns by role. The format stays the same: `⟦face⟧ Name (role)`.
4. **Light and dark colors on the outside identity** (§5.1), reused from the console's role colors.
5. **The same identity outside the console** (§5.2 to §5.4): the name, badge and role an agent has inside are what a host shows outside, with only `, via delphi` added after the role, and a turn-boundary line when the speaker changes.

### 1.1 Non-goals

- **No authority.** A badge, a name or a color is a label. A pool name grants nothing; a dynamic agent's scope, tools and counterweights come from its **role** (`agents/<role>/AGENT.md`), exactly as now. Nothing here lets text raise its own trust.
- **No behaviour from the theme.** Pool names live in the theme pack and change display only (AGENTS.md). Code refers to role IDs and to pool *slots*, never to a name.
- **No "host is acting as" label.** The operator decided (OD-5) that the `via delphi` after the role is the only reminder needed while a host runs an agent. Role consistency for the operator's perception comes first (§5.3).
- **No mascot redesign.** The 3-line mascot (spec 005a, 028) is not redrawn. §10 OD-1 says how its face follows the badge.
- **No guarantee a host shows anything.** A host may strip or summarize tool output. §5 raises the odds and says so; it cannot force display (§8).
- **Color is never the only carrier.** Face plus name plus role must identify an agent in plain text (§6).

### 1.2 Simpler alternative considered

Only the core-role table (§3) and the three missing colors, no pool and no outside work. That is slices S1 alone. It fixes the console but leaves dynamic agents unnamed and the outside experience as it is, which is the part the operator asked about, so S2 to S4 follow. S1 stands alone if the rest is not wanted.

---

## 2. What is true today (verified on main, 2026-11-06)

1. **The avatar library has 7 roles.** `crates/delphi/src/avatar.rs` `ROLES` holds orchestrator, builder, architect, red-team, strategist, scout and operator. Each has `chassis_rgb` and `chassis_rgb_light`, a `badge_face`, a `blink_badge` and a mascot face. The investigator, administrator, assistant, advocate, writer, validator and messenger are not in it.
2. **The outside identity has one color per role and nine looks.** `delphi_core::research::role_look` (`research.rs:117`) returns a badge, one hex and one 256-color index for nine roles. The investigator borrows the scout's `⟦•••⟧` and `#ffff00`. **Writer, validator and messenger fall through to the grey default** `#94a3b8`. `Identity::styled` has no light variant. A test in `avatar.rs` checks only the roles in `ROLES` against this table.
3. **Seven of thirteen roles share two faces.** Idle badges today are `•─•` (orchestrator, builder, administrator) and `•_•` (architect, strategist, assistant, advocate and every unlisted role). Only red-team (`-_-`) and scout (`•••`) differ.
4. **Blinks are per-role in the library, shared in the fallback.** The library gives each role a `blink_badge` (red-team's is the open-eyed `•_•`). An older `delphi` that sends no avatar data gets the console's built-in frames in `ui/src/theme.ts:497-498`: `-_-` for everyone, `---` for the scout.
5. **A blink is one frame.** `ui/src/motion.ts` `faceOf` returns `{ frame: 'blink' }` for one 150 ms tick (`BLINK_FOR_MS`), at a hashed offset every 7 to 30 s. With reduce-motion (`stillMotion`) there is no blink.
6. **Outside, identity is a single advisory line.** `Identity::signature` is `⟦badge⟧ Name (role) via delphi`; `[mcp] badge_style` picks `plain`, `ansi` or `markdown`. The host shows it only if its model followed the `delphi-attribution` skill. A role adopted with `get_agent` is explicitly unsigned (that skill, step 5), so the operator sees no mark that the host is acting as red-team.
7. **The contrast gate exists.** `ui/test/contrast.test.ts` requires every role chassis to reach 3.0:1 on dark backgrounds and 4.0:1 on light ones (`chassis_light`), over several background colors. New colors use it; they do not add a second rule.

---

## 3. The core roles

### 3.1 Faces

The table replaces `badge_face` and `blink_badge` for the thirteen roles. Rows are canonical role IDs; the theme pack supplies each role's display name and does not change a face. Faces are three cells wide, drawn from characters that render at width 1 in common terminals (§6.2). The mouth is the cell at index 1 except where noted.

| Role ID | Idle | Blink | Note |
| :-- | :-: | :-: | :-- |
| `orchestrator` | `⟦•─•⟧` | `-─-` | unchanged; the anchor face |
| `builder` | `⟦•=•⟧` | `>=<` | anvil mouth; squints at the forge |
| `administrator` | `⟦•.•⟧` | `-.-` | flat, still |
| `architect` | `⟦•o•⟧` | `-o-` | mid-idea |
| `strategist` | `⟦•u•⟧` | `-u-` | wry half-smile |
| `assistant` | `⟦•‿•⟧` | `-‿-` | polite, attentive |
| `advocate` | `⟦•^•⟧` | `*^*` | hopeful; a spark |
| `messenger` | `⟦•w•⟧` | `·w·` | trickster; a quick flick |
| `validator` | `⟦¬_•⟧` | `-_-` | one eye narrowed, checking the proof |
| `red-team` | `⟦¬_¬⟧` | `-_-` | glare; was `-_-` idle with an open-eyed blink |
| `writer` | `⟦•_•⟧` | `•_-` | twin of investigator; wink |
| `investigator` | `⟦•_•⟧` | `-_•` | twin of writer; mirrored wink |
| `scout` | `⟦•••⟧` | `-••`, `•-•`, `••-` | no mouth; one eye at a time, never all asleep |

**The twins** share a face. They are told apart by color (writer gold, investigator silver, §3.3), by the mirrored winks, and by name. **The validator and the pool name Eris** are a deliberate mirror (§4): the validator `¬_•`, Eris `•_¬`, because the validator's figure is Eris's child in Hesiod (see the theme pack). The investigator stops borrowing the scout's look.

The writer's wink `•_-` reads as cheerful, like the shared success frame `^─^`, but the cells differ and nothing needs them apart. If the operator wants the wink clear of the success frame, `‾_‾` is the alternative (OD-4).

### 3.2 Blink frames

- A blink is the role's own sequence. Single-frame blinks need no new code: the library already stores a `blink_badge` per role.
- **The scout's blink has three frames** (`-••`, `•-•`, `••-`), one per tick. `faceOf` returns `{ frame: 'blink', step }` and the library holds an array of cells for the frame. Total length stays under half a second.
- **Pool agents** (§4) use the mechanical blink: the face with each eye cell replaced by `-`. A face whose eyes already are `-` (none in the pool) would blink to itself and is refused by the lint (§6.1).
- **Reduce motion needs no new rule.** `faceOf` already returns the still frame when `still` is set, and `ui/test/reduce-motion.test.ts` fails on a private clock. The new frames take their timing from `motion.ts` and add no clock of their own.

### 3.3 Colors

- Each role keeps its current `chassis_rgb` and `chassis_rgb_light`. The new library entries (administrator, assistant, advocate, investigator, writer, validator, messenger) take the dark values already in `role_look` where there is one, plus a **new light value** from the same hue, darkened until it clears the contrast gate (§2, item 7).
- **New hues** for the roles that have none: writer **gold**, investigator **silver**, validator **emerald**, messenger **orange**. Exact values are fixed in S1 by the contrast test, not here. Two constraints: gold must stay clearly apart from the scout's yellow (S1 may use a deeper ochre if it does not), and silver must stay apart from administrator's slate.
- **No two core chassis within a minimum distance** in CIELAB, both modes. S1 measures the spread of the nine existing colors and sets the threshold from it; it is stated in the PR and pinned by a test. This spec does not invent a number.
- The scout's chassis stays `#ffff00` on dark; nothing here changes it.

---

## 4. The name pool

A **pool** is a list in the theme pack (`themes/greek/theme.md`, the machine-readable block) of names that Delphi may give to a one-off agent it spawns for a role. A pool entry has a `name`, 3 to 5 `keywords`, and a badge `face`. It has no role, no scope and no authority.

### 4.1 The 36 names

Generic faces (`•_•`, `•─•`) are used by eleven names; the name and the role color tell them apart. The faces are the operator's picks from the 30 offered, minus #2 and #14.

| # | Name | Keywords | Face |
| :-: | :-- | :-- | :-: |
| 1 | Heracles | strong, blunt, labor, finish-the-job | `>_<` |
| 2 | Atlas | load-bearing, endurance, scale, holds | `•─•` |
| 3 | Achilles | decisive, fast, single-fight, weak-point | `◉_◉` |
| 4 | Theseus | pathfinder, thread-follower, maze-solver | `◔_◔` |
| 5 | Ariadne | thread, navigation, guide, unraveling | `^‿^` |
| 6 | Perseus | indirect-approach, reflection, borrowed-tools | `■_■` |
| 7 | Cassandra | warnings, forecast, true-but-unwelcome, risk | `O_O` |
| 8 | Penelope | patience, long-game, unpicking, stalling | `-‿-` |
| 9 | Nestor | veteran, precedent, long-counsel, history | `•_•` |
| 10 | Mentor | coaching, handoff, teaching, guise | `•ᴥ•` |
| 11 | Orpheus | persuasion, presentation, descent, don't-look-back | `☆_☆` |
| 12 | Circe | transformation, conversion, rewrite, potion | `¬‿¬` |
| 13 | Hestia | hearth, steady, maintenance, home | `•_•` |
| 14 | Demeter | harvest, cycles, yield, sustenance | `^_^` |
| 15 | Dionysus | improvisation, divergence, play, ferment | `•ω•` |
| 16 | Hecate | crossroads, thresholds, keys, triage-routing | `°‿°` |
| 17 | Themis | law, fairness, policy, compliance | `•─•` |
| 18 | Nemesis | proportion, consequence, postmortem, balance | `•_•` |
| 19 | Tyche | chance, variance, sampling, luck | `>‿<` |
| 20 | Metis | cunning, craft-intelligence, planning, counsel | `•_•` |
| 21 | Mnemosyne | memory, recall, archive, records | `·_·` |
| 22 | Iris | signal, bridge, color, notification | `✦_✦` |
| 23 | Eos | dawn, first-light, brief, summary | `▪‿▪` |
| 24 | Helios | sun, observation, daily-course, monitoring | `o_o` |
| 25 | Selene | night, quiet, overnight, batch | `=_=` |
| 26 | Pan | wild, unscripted, exploratory, noise | `•~•` |
| 27 | Asclepius | healing, diagnosis, repair, debugging | `•─•` |
| 28 | Chiron | tutor, wounded-healer, patient, teaching | `•_•` |
| 29 | Pythia | oracle, ambiguity, forecast, trance | `≈_≈` |
| 30 | Kairos | timing, opportunity, scheduling, the-moment | `°_°` |
| 31 | Eris | discord, dissent, contention, stir | `•_¬` |
| 32 | Nike | victory, ship, release, finish-line | `•─•` |
| 33 | Atalanta | speed, sprint, quick-task, race | `•─•` |
| 34 | Cadmus | letters, schema, founding, format | `•_•` |
| 35 | Pandora | edge-cases, unknowns, opened-jar, hope | `×‿×` |
| 36 | Sisyphus | repetition, regression, flaky, endless-maintenance | `×_×` |

Four faces changed from the first list to avoid near-duplicates with the core set (§3.1): Eris from `¬_¬` (the red-team's), Hestia from `•‿•` (the assistant's), Asclepius from `•·•` (the administrator's `•.•` is almost the same in some fonts), and Nemesis from `•═•` (the builder's `•=•`).

The names are the display layer; the **pool slots** the code uses are the ordinal ids `pool-01` to `pool-36`, so renaming a name never touches code or stored events.

### 4.2 Color of a pool agent

A pool agent wears **its role's chassis color** (dark and light), so a spawned builder is amber like the core builder and the reader still knows the kind of work at a glance. The *face* and *name* are what mark it as a different agent. Two live agents of the same role in one session therefore share a color and differ in face and name; the picker prefers faces that differ (§4.3). This avoids inventing 12 more hues on a wheel the core set already crowds (§3.3).

### 4.3 Choosing and keeping a name

1. **The orchestrator picks** when it defines a dynamic agent (inside Delphi through the roster, outside through `spawn_agent_pack`): it matches the task's words against the keywords and takes the best-scoring free entry.
2. **A deterministic fallback.** With no clear match, or if a host asks for no choice, the name is the free entry at `hash(agent_id) mod 36`, scanning forward. The same agent keeps the same identity for its life and across a replay.
3. **No two live agents in one session share a name.** Prefer a face different from every live agent of the same role. If all 36 are live, the next takes a numeral (`Heracles II`); this is rare enough that it is not designed further (OD-3).
4. **The role stays visible.** The signature outside is `⟦•_•⟧ Sisyphus (builder, via delphi)`; inside it is the same without the notice. A pool name never replaces the role ID in the signature, in events or in the handoff.
5. **Stored with the agent.** The chosen slot is recorded in the agent's start event, so a replay and the transcript show the same name. The event gains a `pool_slot` field; its schema is updated in the slice that adds it.

### 4.4 Safety

All names, keywords and faces are static data in the theme pack, not model output. A host's claim of a name is never trusted: the identity a host shows for a role it *adopts* stays sanitized and labelled `via`, as `Identity::new` does now. The lint (§6.1) rejects a pool entry whose face is not exactly three cells, uses a character outside the allowed set, or collides with a core face on the same idle and blink.

---

## 5. Outside the console

Outside, Delphi serves text over MCP; the host decides what to print. This section changes what Delphi serves so that compliance costs the host nothing, and does not claim more than that.

### 5.1 Light and dark on the identity

- `Identity` gains `hex_light` and `ansi256_light`, filled from the same library data as the console, and `role_look` grows to the thirteen roles.
- `[mcp] badge_mode = "dark" | "light"` (default `dark`) selects which pair the ANSI form uses. Delphi cannot see the host's terminal background; the operator sets the value once. The plain and markdown forms carry no color and are unaffected. Config key, schema (`schemas/config.schema.json`), `config.example.toml` and `docs/reference/config.md` change together in S3.
- The pool name and face reach outside through the same `identity` object, with a `pool` field when one applies.

### 5.2 A pre-rendered `display` line

Every signed result carries a ready-made `display` string in the form of §5.3 (`⟦face⟧ Name (role, via delphi)`), already rendered as chosen by `badge_style` and `badge_mode`, and the tool description tells the host to print it verbatim first. Today the host must load a skill, build the line and choose a form; the skill remains for hosts that want the detail (`knowledge_status` and the rest). A result with no signature has no `display`.

### 5.3 One identity, inside and outside

Decided by the operator on 2026-11-06 (OD-5): an agent keeps **the same name, badge and role everywhere**, so the operator's picture of who is who does not change with the venue. The only difference outside is a short `via delphi` notice inside the role's parentheses.

- **The form.** Inside the console an agent is shown as `⟦face⟧ Name (role)`. Outside it is the same text with `, via delphi` added after the role, inside its parentheses: `⟦¬_¬⟧ <display name> (red-team, via delphi)`. The notice is shorter than today's trailing `via delphi` and sits inside the role's parentheses, so the line stays one unit.
- **A host that runs an agent** (it adopted the role with `get_agent`, so it is the host's model doing the work) shows that same identity with the same `via delphi` notice. That notice is the only reminder that a host, not Delphi's own seat, is running it. There is **no separate "host is acting as" label** and no `frame_line`; the operator decided one is not wanted, neither required nor recommended.
- **The host's own voice** (words that are not a Delphi agent's) carries no mark, as now.

Consequences, so they are not a surprise: the `delphi-attribution` skill's step 5 ("never sign as Delphi yourself; say you are acting as that role") is replaced in S3 by this rule, and `Identity.via` (set today only when a host adopts a role) is kept but no longer changes how the line reads. The `via delphi` notice does not tell a reader whether Delphi's own seat or a host's model wrote the text. That ambiguity is accepted by the decision and is listed in §8.

### 5.4 Hand-offs and the roster

- **A turn-boundary line** is part of a result when the speaker changes: `── <display name> (red-team) takes over: threat-model the relay path ──`, and a return line when control comes back. It states the *reason* in the same words as the handoff's task field, so the framing shift is readable. It is advisory.
- **A roster line** (`Now speaking: <name> (orchestrator) · <name> (red-team) · Sisyphus (builder)`) is available from `delphi_roster` and may lead a response; hosts may drop it.

### 5.5 What a host can still defeat

A host can summarize, reorder or omit tool output. §5.2 to §5.4 raise the rate at which identity survives; they do not make it certain, and Delphi cannot see whether it did. This is the same rule as spec 082: what a host reports is a claim, not a result.

---

## 6. Rules the lint and tests hold

### 6.1 Lint

1. Every badge face is exactly three cells and uses only the allowed character set (§6.2).
2. No two core roles share an idle face unless both are in the documented twin pair; no pool face equals a core face except the generic `•_•` and `•─•`.
3. Every blink differs from its idle face (a role cannot blink to itself).
4. Every chassis, dark and light, clears the contrast gate (§2, item 7), and no two core chassis are inside the distance threshold (§3.3).
5. Pool: 36 entries, unique names, 3 to 5 keywords each, no theme name appears in code outside `themes/` (the existing role-ID rule).

### 6.2 Allowed characters

Width-1 in the common terminal fonts: `• · ° ^ _ - ─ = . o O u w ~ > < * ¬ × ‿ ≈ ω ᴥ ☆ ✦ ◉ ◔ ■ ▪`. Several of these (`◉ ◔ ■ ▪ ☆ ✦`, and `°` and `×` in some East Asian locales) are ambiguous-width. They are allowed only on pool entries, which are secondary, and never on a core role. A proposed `DELPHI_ASCII_BADGES=1` maps every face to an ASCII fallback listed in the theme pack; S2 defines the table and the switch, and the name is not final.

### 6.3 Tests

- `avatar.rs` test: all thirteen core roles are in `ROLES` (today seven are); `role_look` matches `ROLES` for all of them (extends the existing check).
- `ui/test/avatar.test.ts` and `contrast.test.ts` run the new roles in every console state on dark and light terminals, with no new TypeScript, through `DELPHI_WRITE_UI_SPEC=1` (`ui/AVATARS.md`).
- `ui/test/reduce-motion.test.ts` stays green; add a case for the three-frame blink.
- Golden outputs for `display` and the turn-boundary line in the plain, ANSI and markdown forms.
- Docs and schema drift: `cargo test -p delphi --test docs` after the config key.

---

## 7. What is not measured

Nothing in this spec has been tried with an operator or a host. Whether distinct faces and a visible handoff line make the operator *understand* a long multi-agent exchange better is a claim the design assumes and the work does not test. A measure, deferred to after S3: take a recorded multi-agent session, show it to the operator once with identity marks and once with them removed, and ask who spoke and why the framing changed. The result is written up without adjusting the design to it afterwards. Whether any given host prints `display` is observable per host through the conformance probe (spec 086 S0b) and should be recorded there, not guessed.

---

## 8. Known weaknesses

1. **The wheel is crowded.** Thirteen core hues plus the scout's yellow already strain a color-blind reader; that is why face, name and role carry identity too (§1.1).
2. **Eleven pool names share a generic face.** Name and role color distinguish them, so two same-role pool agents on generic faces differ only by name.
3. **Ambiguous-width glyphs** may misalign rows in some terminals. The ASCII fallback (§6.2) is the answer, and it loses the variety.
4. **Hosts decide.** §5.5.
5. **Twin faces are identical.** In a plain, colorless transcript the writer and the investigator differ by name and wink only; the name is always present.
6. **No mascot work.** The 3-line mascots for the new roles follow the badge (OD-1) and may look thinner than the badges; the operator will revisit this later.
7. **`via delphi` does not say who wrote the text.** A host's model running an adopted role and Delphi's own seat show the same line (§5.3). This is the accepted cost of keeping the identity consistent; the conformance probe (spec 086 S0b) is where any difference in what hosts do should be recorded, not the line.

---

## 9. Slices

One slice per pull request, in this order. Each updates the spec tracker.

| Slice | Content | Hours |
| :-- | :-- | --: |
| S0 | operator approves this spec and answers OD-1 and OD-5; tracker entry | 1 |
| S1 | the thirteen core roles in `ROLES`: faces, blink frames, light/dark colors, `role_look` for all; library fixture and tests; the lint rules of §6.1 that concern core roles | 9 |
| S2 | the pool in the theme pack; lint; picker (`hash(agent_id)` fallback and keyword match); `pool_slot` on the start event, with its schema; roster and `spawn_agent_pack` use it | 9 |
| S3 | outside: `hex_light`, `badge_mode`, `display` in the §5.3 form, the `delphi-attribution` skill step 5 rewritten, turn-boundary and roster lines; goldens; config schema and docs | 9 |
| S4 | console: three-frame scout blink in `faceOf`; pool agents shown in the agents pane, epic threads and Observe; reduce-motion case | 6 |
| S5 | the recognition check of §7, once S1 to S3 are merged and an operator is available | 3 |

---

## 10. Decisions

OD-1 and OD-5 were answered by the operator on 2026-11-06. OD-2 to OD-4 were not contested and take the recommendations.

| # | Question | Decision |
| :-- | :-- | :-- |
| OD-1 | The 7-column mascot face for the new roles: derive from the badge, or draw each? | **Decided: follow the badge for now** (the badge cells, centered; the builder keeps `•───•`). The operator will revisit this later, with spec 028. |
| OD-2 | `badge_mode` as a config key, or always dark in the ANSI form? | The key; a light-terminal operator otherwise gets unreadable gold and silver. |
| OD-3 | When all 36 pool names are live: a numeral (`Heracles II`), or refuse to spawn? | Numeral. It is cosmetic; refusing would block work for a label. |
| OD-4 | The writer's wink `•_-`: keep, or `‾_‾` to keep it clear of the success frame? | Keep; the cells already differ. |
| OD-5 | Is a host-acting label (`frame_line`) required of a host, or recommended? | **Decided: neither; there is no such label.** Keeping the role's identity consistent matters more for the operator's perception, and while a host runs an agent the `via delphi` after the role is a sufficient reminder. The name stays the same inside and outside, except for that shorter notice inside the role's parentheses (§5.3). |
---

## 11. Review checklist

- A pool name, badge or color grants nothing and changes no scope; scope comes from the role.
- No code outside `themes/` names a theme character; code uses role IDs and `pool-NN` slots.
- An agent's name, badge and role read the same inside and outside; the only outside difference is `, via delphi` after the role. A Delphi answer is never unsigned when a signature exists.
- Every new color passes the existing contrast gate in both modes; every new animation takes its timing from `motion.ts` and honours reduce motion.
- The core table is the only place a core face is defined; the fixture is regenerated from `avatar.rs`.
