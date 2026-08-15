# Conformance findings ledger

Divergences surfaced by `mgc-conform verify-deltas` (docs/RECORDING.md)
against the retail recordings. This file records LEADS, not verdicts:
each entry needs decompile corroboration before any port change, and
several may resolve into capture caveats or DEVIATIONS.md entries.
Add new findings here as runs are triaged; move resolved ones to a
`RESOLVED` section with the outcome.

Enforcement lives in the FIXTURE SUITE (docs/CONFORMANCE.md,
`conformance/*.json`): triaged pairs replay on every `cargo test` with
expected statuses (`conforming`/`open`/`capture`); fixture notes cite
the entries below. Fixing an entry flips its fixtures — promote them
(`mgc-conform fixtures … --promote`) in the same change that moves the
entry to Resolved.

Baseline corpus (2026-07-31 re-records on the MONOTONIC-frame-counter
`*_REC.EXE` recorder — the tickpatch mailbox latches the per-frame
clock on both games, so the MC2 Turn++-park tear is GONE; all pairs
at `--input-delay 2`; suites refreshed 2026-08-01 via fresh extract +
`carry_curation.py` + `classify_fixtures.py`):
- **mc1l0**: gapless full level-0 playthrough, 5,874 ticks, 5,873
  pairs, 0 torn, all fixture-grade, **450 conforming** (440 before
  the wake-law round); RNG (1,1) on every pair. Roster-aware
  (post corpse-flame spreader fix, 2026-08-02): **4,152
  conforming-or-explained**, UNEXPLAINED 12,129 field /
  98 missing / 211 extra rows. (The `mc1l0-village-regrade` rule
  hit 0 rows — its t/rect scope was the OLD take's regrade event;
  retire or re-scope on the next roster pass.)
- **mc1hwl0**: full HW take under meteor weather, ticks 0..39,800
  with 15 gaps (69 frames — heavy-animation skips; a skip-free HW
  run is not achievable) + 517 torn, 39,199 of 39,716 pairs
  fixture-grade, **49 conforming** (46 → 48 wake-law round → 49
  corpse-flame spreader 2026-08-02);
  RNG (1,1) on 39,171 pairs, retail >16-draw bursts on 28. Terrain
  closure still owns ~every pair (`mc1hwl0-terrain-z` explains
  2.12M rows / 39,133 pairs; 2.28M field rows unexplained — HW
  progress keeps reading from per-family totals + the story suite,
  not the pair headline).
- **mc2l0**: gapless 8,627 ticks, 8,626 pairs, **0 torn** (take-2
  on the rate-limited recorder tore 1,105 of 3,640), all
  fixture-grade, **479 conforming** (167 → 240 cave-rand round 2 →
  452 same-tick reap → 466 day-bank extents → 479 possession
  tier-0 gate + shared spreader, 2026-08-02); rng
  mismatch on **2 pairs only** (was 3 — reap-aligned seeds).
  Roster-aware: **6,066 conforming-or-explained**, UNEXPLAINED
  6,829 field / 123 missing / 21 extra (the reap converted most
  ghost-alias extras: gross extras 3,761 → 1,389, unexplained
  extras 198 → 22; gross missing 431 → 1,095 — dominated by
  re-labeled slot-alias rows, see the reap Resolved entry).
- **mc2l4 + mc2l30** (CUT 2026-08-01 from the single conjoined
  `mc2l4,30.mgcr` take at t=17713; the take's SINGLE frame skip
  17711→17713 is exactly the level transition — the tick fn never
  ran during the load — so both cuts are internally gapless, and
  the embedded level record flips at the cut as before): mc2l4 =
  17,711 pairs, 0 torn, all fixture-grade, 0 conforming raw but
  **13,698 of 17,711 pairs roster-explained (77%)**,
  rng mismatch **13** (163 before the fire-spray ring loop,
  2026-08-02); mc2l30 = 9,337 pairs, 0 torn, all
  fixture-grade, rng mismatch **19 of 9,337 pairs** (9,328 →
  cave-rand structure round 2 → 202 → 19 fire-spray ring loop +
  summit latch/frozen-z, 2026-08-02; session 4 REFUTED
  the per-entity `rand_0x14` hypothesis — the residual WAS the
  VOLCANO-CASCADE, §l30-churn (b) as re-written; of the last 19,
  one is the t=274 dome-import eruption-timing pair, 18 ride the
  slot-desync fire cascade), **6,686 pairs roster-explained**
  (was 1 → 6,320 → 6,658; reap collapsed the (10,0)/(10,14)
  extra side 5,590→346 / 917→36; UNEXPLAINED now 14,007 field /
  188 missing / 87 extra). Suite note: one mc2l4 exemplar's signature differed
  between the full extract pass and the sparse suite pass (the
  shared world instance leaks a trace of which pairs ran before —
  select-dependence, warning-grade); re-promoted to the
  suite-stable signature.
(Triage tooling on the runner: `--csv` per-diff TSV for offline
clustering, the POSE-PHASE classifier (2026-08-01, docs/CONFORMANCE.md
§pose-phase: every dirty pair re-runs under the other `--pin-pose`
sample; rows clean in either run tag `pose-phase` = within-tick pose
capture, leave the UNEXPLAINED headline, CSV rule column literal;
`--no-pose-alt` disables — mc1l0 claims 987 field rows/288 pairs,
mostly (5,x)/(9,0) aim+step; the (9,1)/(9,0) aim families that match
NEITHER pose stay open), `--dump <t> [--dump-port]`, `dump-state
<file> <t> <slot…|all>` — now also prints both free/recycle stack tails,
next-pop last — `trace <file> <slot> <t0> <t1>`, `--start <t>`
windowed triage on the MC1 arm too (announces pairs + the
free-stack fallback, wired through the MC1 import report), and
`ground-audit <file> [--dump t]` — retail rest-z vs the port's
generated plane per (class,model) + 16-tile site, the instrument
that refuted the HW generator-shortfall hypothesis.)
(History: the first takes ran 627 pairs / 73 conforming and 417 /
32; the fix rounds moved the like-for-like take to 34, and the full
recipe + fixes reached 117.) (History, 2026-07-30 corpus on the
rate-limited recorder, retired by the monotonic re-records: mc1l0
5,329 pairs / 385 conforming after the tick-top-reap round; mc1hwl0
40,586 fixture-grade / 1 conforming, entity-set misses 717,798 →
~33k after reap + rival re-anchor; mc2l0 take-2 7,762 fixture-grade
of 11,523 with 3,761 torn / 7→11 conforming, 5,242
conforming-or-explained; mc2l4 12,786 grade of 19,154 with 6,368
torn, mc2l30 10,021 grade of 15,428 with 5,407 torn — the per-take
triage sections below cite THAT corpus's tick numbers and counts.)
(History: the
first flat-tolerance gate starved HW — ambient spawn churn rewrites
+63 with spawn ordinals every tick and was read as tearing, 85-tick
rejection streaks; the gate now counts only `dv±1` steps as tear
suspects.) Every open entry below reproduced across all takes,
including the 75%-torn pre-gate corpus.

## Confirmed conforming (worth naming)

- **Global LCG draw law**: 627/627 MC1 pairs and 48/48 HW pairs draw
  exactly one `9377x+9439` step per tick, matching the port's
  tick-top draw. (The previously-banked "12.5% draw-driven stall"
  was a capture artifact — see RECORDING.md "Capture tearing".)
- **The +63 phase clock**: 12 stray entity-ticks over 627 MC1 pairs,
  all spawn-edge (ordinal overwrites, projectile birth/death). The
  port's "step every dispatched entity" matches retail's static
  state table (`data10` is 1 on every live row —
  docs/traces/mc1-state-table.md, sub_main :52356/:52406).
- **Free-list discipline**: with the LIVE free-stack imported, port
  spawns land on retail's slots (verified: the fireball at slot 627
  both sides).

## Open leads (port vs retail, unfixed by ruling 2026-07-29)

0. **ENTITY-SET MISSING SIDE (post-reap map, 2026-08-01).** The
   MC2 same-tick reap LANDED (see Resolved — the extra side
   collapsed: l30 (10,0) 5,590→346, (10,14) 917→36; mc2l0
   unexplained extras 198→22) and the missing side is now the
   dominant entity-set lead. Post-reap gross missing (mostly
   roster-explained cast-timing, but the big non-(9,x) families
   are real): **mc2l30 (10,0) 1,693 + (10,14) 984** and **mc2l4
   (10,0) 2,005 + (10,14) 890 + (10,12) 717** — retail fire/riser
   spawns the port never makes (churn spawn cadence — the
   rand_0x14 suspicion is REFUTED, see §l30-churn (b): on l30 the
   family is the VOLCANO-CASCADE fire spread + summit re-erupt
   cadence; the l30 202 rng-mismatch pairs cluster exactly on the
   eruption windows); mc2l0 (10,13) 45 missing / 81
   extra (newborn churn into recycled slots — fixture t=737
   re-statused capture `mc2-fire-churn-m13`). Genuinely
   independent missing families still queued: **mc1l0 (10,0)
   fires 57 missing / 210 extra — MC1 has no reap excuse, fire
   spawn/expiry CADENCE, a real family**; mc1l0 (10,39)
   ball-merge edges (50/31); mc2l0 (2,0) trees 18 missing;
   (10,45) houses 7 missing (= §castle follow-up (c)
   build-window). Slot-mismatch stays MINOR (15 rows mc1l0 / 0
   mc2l0).

0b. **MC2L24 SCRIPTED CREATURE WAVES — SPAWN, BUT SLOT-DESYNCED
   (2026-08-02; dig B's "unported trigger" claim CORRECTED by the
   fixture signatures).** Two level-scripted spawn waves fire at
   t≈3569 ((5,3) worms + (14,1)×3 + (10,63) + (5,9) + (5,26)) and
   t≈13330 ((11,x) triggers + (5,17)/(5,20)/(5,26) + (10,71) +
   more). The t=3569/13330 fixture sigs show EXTRA and MISSING of
   the SAME models in the same pair, and whole-take totals are
   balanced ((5,3) 63/60, (14,1) 4/4, (5,9) 6/8) — the port DOES
   spawn the waves, at desynced slots: the ruled free-list
   slot-order infrastructure limit at mass-spawn ticks, not a
   missing trigger. **SESSION-6 UPDATE (2026-08-03): the ruling is
   now the computed `slot-desync` roster rule (dig F, see
   Resolved), and the re-census is DONE — (10,25) 37/0 and
   (10,75) 110/13 post-absorption are REAL unported-spawn leads
   (doomsday-pyramid effect + tail-drag chain); the (5,0) owner
   rows and the class-15 detach machine are RESOLVED (digs E + D —
   note (5,0) = pyramid-summoned worms, NOT hydra segments).**
   **SESSION-7 UPDATE (2026-08-03): (10,25) + (10,75) are now
   RESOLVED too — and the "doomsday" attribution was WRONG. Both
   are the (11,2) STORM-SWITCH disposition (whirlwind heads +
   funnel nodes + area blasts); the port's switch box dropped the
   human carpet's own 121-unit half-extent. See Resolved, "MC2
   SWITCH VOLUMES LOST THE HUMAN'S OWN HALF-EXTENT" —
   (10,25) 37/0→7/0, (10,75) 110/13→13/14, (10,22) 10/0→2/1,
   l24 missing 1,209→1,074. The residue is the same free-list
   slot-order limit this entry describes.**
   **SESSION-8 UPDATE (2026-08-03): the "free-list slot-order limit"
   was NOT an infrastructure limit — it was a PORT BUG. The MC2
   import double-pushed every ghost slot onto the free stack (once
   itself, once through `tick()`'s reap), so a spawn burst deeper
   than the ghost count re-allocated slots it had just filled. See
   Resolved, "THE MC2 CONFORMANCE IMPORT DOUBLE-PUSHED EVERY GHOST
   SLOT". A SECOND slot-order source survives and is now ROOT-CAUSED
   too (fix NOT landed — `mgc-formats`, own dig):
   **`mgcr::mc2_stack` recovers the pool base by assuming the stack's
   HIGHEST cell is slot 999.** It scans `cells[0] − s·168` from
   s=999 down and takes the first candidate under which every cell
   decodes in-range — every candidate keeps the cells stride-aligned
   (they are all pool pointers), so the only binding constraint is
   "max index < 1000", i.e. the lowest legal base = max cell ↦ 999.
   The moment the top pool slots are IN USE and therefore absent
   from the stack, every decoded slot is inflated by that many.
   Measured on mc2l24: t=53808 shift 0 (0 of 716 cells land on an
   occupied slot, census passes), **t=60101 shift 2** (197 of 576
   cells land on live (10,39)/(5,25)/(5,15)/(10,79) records),
   **t=62929 shift 4** (129 of 226) — a brute force over a constant
   k proves a unique k makes EVERY cell land class-0. The import's
   census catches the corruption (`live.len() != scan_free`) and
   falls back to the descending slot scan, which pops lowest-first
   and re-orders every spawn in the pair (t=60101: the pyramid's
   worm chain lands in slots 5/8/9/37… against retail's
   576/584/585…, 48 balanced missing/extra). FIX SHAPE: choose the
   base by VALIDATING against the pool image (the unique shift under
   which every cell lands on a `class3f == 0` slot) instead of the
   max-index guess; the runner's `free-stack fallback: live X !=
   scan Y` stderr line (under `--start`) is the ready-made
   instrument for counting how many pairs it costs per take.**
   Knock-on: mass-tick slot skew feeds the 52-63k epoch churn
   asymmetry and the lone rng residual at t=51556.
   **SESSION-9 UPDATE (2026-08-04): the `mc2_stack` half is FIXED —
   the base is now validated against the pool image (and the recycle
   stack shares it). See Resolved, "THE `.mgcr` MC2 DECODE GUESSED
   'THE HIGHEST STACKED CELL IS SLOT 999'": `free-stack fallback` is
   gone from all 69,207 mc2l24 pairs, the four shifted windows drop
   gross missing 9,402→469 / extra 10,134→1,241, and the computed
   `slot-desync` rule stops firing there entirely. WHAT SURVIVES: the
   EARLY-take desync (t=3569 / 13330 and the whole-take 208/208
   slot-desync residue) is a DIFFERENT cause — l24's first shifted
   snapshot is t=54932 and the 3500+300 window is byte-identical
   across the A/B, so this entry's early-wave rows need their own
   dig.**
1. **HW ambient-family population loss — ROOT-CAUSED, mostly fixed**
   (see Resolved; residuals on the 289-pair mc1hwl0-test take): the
   port lacked generic MC1 handlers that HW's content exercises. The
   engine is byte-identical MC1↔HW (no data-table delta); the
   "weather" was (a) (10,2)/(10,3) puffs the port reaped via the
   terrain-dispatch self-kill catch-all — ctors+ticks now ported —
   and (b) rivals' class-12 owned-spell TOKENS the port decayed as
   scatter jars — docs/traces/mc1-class12-spell-tokens.md, fixed via
   strict_retail. REMAINING: 57 (10,0) + 3 (10,1) corpse-cascade
   under-spawn (retail `sub_1A800`: corpse slot → (10,1) puff, ball
   via `sub_27690` only on carried mana — port's mob_corpse differs
   at the boundary; ~33 (10,39) ball diffs ride the same chain);
   39 (10,2) from the UNPORTED active speed-token emitter
   (`sub_56380`, puff every 4th token-tick); a few (9,1) from the
   spell-3 bolt token (`sub_56510`).
2. **TERRAIN CLOSURE — the dominant residual family. ⚑ PLAN
   FINALIZED 2026-08-05 (player-directed), EXECUTE NEXT SESSION,
   ALL THREE GAMES — `docs/RECORDING-TERRAIN-V2.md` is the plan.**
   Decided shape: the recorder captures the height+type planes
   every record and stores **deltas relative to the PREVIOUS
   RECORDED TICK** (record 0 = full planes) — self-healing across
   recording gaps, near-zero size on quiet ticks. **Importer
   CARRY-FORWARD was evaluated and is a DEAD BRANCH (player-ruled
   same day): the recordings contain gaps (graphic-overload
   stalls), a carried terrain loses every edit inside a gap and
   silently poisons all downstream grading. Do not revisit.**
   Free instruments: record-0 planes = the stock-bake validator;
   per-pair terraform grading (port terrain writes vs retail's
   delta, cell-by-cell). The import-time reconstructions (pad
   replays, riser endcaps, prop-z inversion) demote to sensors
   once measurement lands. Rides the tickpatch + PP_CASTLE
   re-record round. Player's underlying theory, refined: every
   terraforming CAUSE is already in the corpus, so a continuous
   1:1 replay would need no channel — but the harness grades PAIR
   closures, and accumulated terrain state is not in the snapshot;
   the channel turns the capture bucket into measurement.** (proven on
   the 2026-07-30 mc1l0 corpus): the recording has no terrain
   channel and every pair replays on pristine planes, so retail's
   runtime terrain edits are invisible. The z diffs cluster at
   event sites with CONSTANT per-entity offsets from an onset tick
   to end-of-run: fireball craters (t=109+, −1..−4), a large edit
   field around (112,88) from t=472, the rival castle raise at
   (44,40) +256 from t≈1028, and the village regrade at
   (152-164,44-56) from t≈992-1139 (heights AND tile TYPES — the
   construction paint pass, sub_27D30 :30184-248). After the fix
   round this family is z 134k hits / 4801 pairs plus the walker
   x/y/heading knock-on — roughly all remaining bulk noise. Fix
   direction: a terrain channel in recording format v2 (height +
   type planes, delta-coded or hashed-with-keyframes), or replay
   the edit events in the importer.
3. **Castle build WINDOW + economy** (narrowed from the old "castle
   column" entry — the settled-castle half is RESOLVED below): the
   t≈469-513 initial-build window still diverges (transform ball
   slots, (10,39)/(10,0)/(10,1) around the build, castle binding
   retail 627 vs port 475, upgrade mana lumps −5000/−10000). Known
   unported pieces: the wizard's castle slot binding (retail
   player+50, sub_47960 :56484), the mound-mana write at level-up
   (sub_47BD0's mound arm :56561-66 — the slot-28 mana_max
   10000-vs-1000 rows, now only 129 hits), and the ball-economy
   cap-fill (sub_47130 :56160). ~100 pairs.
   **FIX ROUND 2026-07-30 (opus dig + port)**: the "mound" = the
   player's CREATE-CASTLE MANIFESTATION (class 12 m16, ctor
   sub_3C060 → sub_3BF70 :48026: +136=1000, +140=1000/101, +50=101
   the divisor). sub_47DD0 (:56617-73) refreshes it EVERY tick from
   the wizard handler while a castle is bound: +136=cap[level],
   +140=cap/101 — now ported at the castle dispatch site
   (world.rs, strict-retail scoped, f144-owner join; the imported
   manifestation encodes +70<MANIFEST_BASE and never reaches
   manifestation_tick, so a manifestation-side fix is dead under
   conformance). castle_eject also gained retail's pool-headroom
   count cap `min(free+1, clamp(spill/1000,1,32))` (:56194-205) and
   continue-on-failed-alloc (:56213). Slot-28 mana_max 129→21 hits;
   350→384 conforming with the entry-5 clamp. wizext+50/+416: +416
   is WRITE-ONLY (no reader — no port action); the 627-vs-475
   binding is an allocation-order SYMPTOM of the remaining window
   divergence, not a missing write.
   **WORM-WINDOW DIG RESOLVED 2026-07-30 (opus agent + corpus
   A/B)**: the "(5,3) per-tick segment emission" hypothesis was
   WRONG — no emission law exists. The window is a WORM MASS-DEATH:
   heads 61/97 die at t≈472 (state 22), the death handler corpses
   the whole +54 chain in ONE tick (sub_1A6C0 :21828-39), and the
   corpses free one-per-phase-lane (`f63&7==0`, two lanes per worm
   8 slots apart = the four descending lanes). The port's death/
   corpse/segment handlers are faithful; the divergence is
   POOL-ORDER: the port frees 0x400 slots at once and its corpse
   ball-drops recycle the just-freed low slots, where retail
   allocates high (633+) and keeps low-ghost records. ⚠ EXTENDING
   MC2's strict-retail ghost/free deferral to MC1 was TRIED and
   MEASURED WORSE (384→377 conforming, both halves independently)
   — MC1's reaper evidently returns slots within the frame, unlike
   MC2's next-frame remove pass; the free-guard comment records the
   refutation. ALSO from the dig: the port's
   worm-segment ctor re-stamped id24 with the segment's own slot —
   retail's byte-copy KEEPS the head's +24 (corpus-pinned). Fixed
   (goldens re-pinned layout-only, OBSERVABLE holds); this also
   keeps kill credit head-only in native play.
   **CASCADE RESOLVED 2026-07-31 — it was the TICK-TOP REAP LAW,
   not pool order** (see Resolved). The trace showed the pop order
   was correct all along: retail's castle pop (627) was the
   stack top BECAUSE the flag-deferred frees land at the top of
   the next tick, before dispatch. With the reap law landed the
   worm-window substitutions, the phantom second castle at 475
   (a delivered (9,10) re-triggering), and the missing same-tick
   (10,42) painter all cleared. REMAINING (small): the same-tick
   painter's ctor fields differ (port max_life 30 vs retail 0,
   chase 627 vs 0, x off-by-one-tile at spawn) — a (10,42) ctor
   transcription pass vs sub_47020's spawn site is owed.
4. **Impact cluster around casts** — (9,1) 468/610 missing/extra,
   (10,12) hit-flash, (9,0), and the t=67-style substitution
   clusters: input-latency + unreconstructed aim (control aim_yaw/
   aim_pitch). Same ruling as before: partially mitigated by
   `--input-delay 2`; consider recording the control slot mid-tick.
   This is now the FIRST divergence family (t=58).
5. **Human mana regen cadence — RESOLVED 2026-07-30: THERE IS NO
   REGEN CLOCK.** Retail applies `mana += +132` EVERY frame behind
   only the pause gate, then recomputes +132 to its +100/+1000
   floor (:55385, :55407-21); the "drifting 3-4-tick cadence" is
   the NET of that regen against firing-driven suppression + costs:
   every live MID-burst spell event zeroes the caster's +132 before
   the next apply (sub_55E80 :64956 — the first burst tick,
   +48==+50, does not; remc2's twin sub_68DE0 runs the same shape
   with the cost stamp live), and the +90 mailbox debits land the
   same frame. Manifestation slots fall before/after the carpet's
   slot 630 as they churn, so suppression lands same-frame or
   next-frame — a one-frame stamp-then-apply jitter beating against
   the ~4-tick fire rhythm (the MC2 @0x88 "survives one frame" twin
   is the same phenomenon). Timer-domain and f63%4 hypotheses
   REFUTED (five consecutive +100 frames at t=121-125). **The
   port's every-tick regen is CORRECT — the long-standing "port
   regens 3-4× retail" concern is DISSOLVED; do not throttle it.**
   The conformance residual was import-side: the recorder samples
   +132 AFTER the recompute, so the importer seeded +100 on frames
   retail had suppressed — retail_import_mc1 now clamps the seed to
   0 when a live mid-burst manifestation exists (f48≠0, ≠f50,
   human-owned). player.mana 1276→580 pairs; the remainder is
   cast-latency compounds (capture) + the entry-3 window.
6. **wizard0 hand residuals** — 4 pairs: t=310 retail Some(3) port
   Some(16), t=409 hand_right Some(3) vs None. Pickup RESOLUTION
   differences (which jar/hand a mid-level acquisition lands in),
   not the old flicker; revisit with the quickselect-assign law
   (docs/traces/mc1-quickselect-assign-law.md).
9. **Small new families** (post-fix corpus): (9,0)/(9,1) flags
   0x2006-vs-0x6 (bit13, ~176 rows); (2,0) tree missing residue 53
   rows at exactly t=1056(×6)/t=1100(×47) — the hut-completion
   retile edge ticks; (10,39) flags 12→4 (port ball loses the 0x8
   default bit on some spawn path, 177 rows — likely the entry-3
   substitution family, see the worm-segment note there).
   **CORRECTED 2026-07-30: sub_1E810 is NOT a wizard path** — it is
   the GENIE's (m11) ball eat, called only from genie states
   0x43/0x44 (:24512/:24643), and it was ALREADY PORTED
   (`genie_eat_ball`). No retail wizard-flyover ball absorb exists;
   the wizard economy is the castle path (entry 3). mc1l0 has no
   genie, so nothing here rode on it. The dig did surface one real
   gap, now FIXED: the genie's (10,0) puff and the sparkle ring
   were missing retail's `+18 |= 1` stamp (:24793-94/:24377-78 —
   our `flags |= 0x10000`).
7. **HW terrain shortfall — GENERATOR CANDIDATE (a) REFUTED
   2026-07-31 by the `ground-audit` instrument** (mgc-conform mode:
   retail entities' rest-z vs the port's generated plane at their
   coordinates). At t=0 — before any runtime edit exists — every
   class-2 static and every (10,45) hut on the full HW take sits at
   **dz = 0 exactly** (99 samples map-wide; the mc1l0 control is
   identical, its (5,1)+512 rows being balloons at tether
   altitude). Late-tick audits localize ALL large dz to one
   contiguous castle-mound region (80-112, 160-208, ~+1000, the
   (3,2) at +2272) plus battle sites — runtime edits. So the HW z
   bulk is candidate (b) — TERRAIN CLOSURE (entry 2), capture
   domain per the standing deferral ruling; no generator fix
   exists to make, and the one-off live-DOSBox height-plane dump
   is no longer needed for this question (optional someday as a
   full-plane certifier — statics only sample where they stand).
   The old "~256 z around (56,246)" measurement came from the
   superseded partial take's walker families. Still open from the
   same triage: retail manifestations import with `+70 < 200`,
   below the port's `MANIFEST_BASE = 200` encoding, so imported
   manifestations take the resting-jar path instead of
   `manifestation_tick`.
8. **HW stat doubling — DISSOLVED (entity-substitution artifacts)**:
   "life 30 + flags 65536(0x10000)" = the port's (10,42) castle
   build painter occupying a slot it reaped (painter max_life=30,
   build bit 0x10000) — not a scaled (10,2); "mana_max 20000 vs
   10000" = the castle (3,2), whose stats are identical MC1↔HW —
   that is finding 3's castle column. retail flags 131073 = 0x20001
   (effect bit17 + active), port 65536 = 0x10000 (painter build
   bit): two different entities, no bit-shift exists. No stat-scale
   fix anywhere; closed into findings 1 and 3.

## MC1HW take-1 (2026-07-31 triage; suite conformance/mc1hwl0.json, 12 story fixtures)

41,488 pairs / 902 torn / 40,586 fixture-grade / 1 conforming (t=0)
after the reap law + the rival re-anchor. Terrain closure (entry 2's
HW face — the castle-mound region (80-112,160-208) raised ~+1000 by
late-run) z-poisons ~every pair, so progress reads from families +
the story suite. Post-fix field totals: z 1.72M (capture-dominated) ·
life 613k · flags 428k · x/y ~300k · rand 139k · heading 111k ·
mana_max 8.7k pairs · player.mana 7.6k · player.life 1.2k.

- **Rival carpet FREEZE — RESOLVED 2026-07-31 (importer defect, not
  a motion bug)**: `rival_entity_tick` keys on `self.rivals[i].ent`,
  which `retail_import_mc1` never re-anchored to the imported slots —
  every imported rival carpet was a frozen husk (obs@1 = state@0
  verbatim; the first divergence family, every pair). The port's
  motion law itself is verbatim sub_14EB0 (:18781; hand-computed one
  tick from state@0 → retail obs@1 EXACTLY: z band-settle quarter-rate
  −1, polar step sin/cos>>16, ±16/tick speed slew toward Type_160
  v_12, turn rate angdist/(8+(255−tempo)/16) clamped + overshoot
  snap). Fix: re-anchor `self.rivals` per pair (ent = play_index) +
  reseed vdes/jink/grace/mana lanes from the closure. First HW
  conforming pair.
- **Rival AI-STATE reconstruction — RESOLVED 2026-07-31** (the
  freeze entry's REMAINING): the AI record imported as state=Fresh
  with no target, so the decision cascade re-aimed f34 (target_yaw
  1477-vs-1825 on ~25k pairs) and cast choices diverged. Retail runs
  the state HANDLER before the selector (sub_13170 :17847), so state
  and a `target_alive`-surviving target must import TOGETHER or the
  tick falls back to Fresh. Decoded (all Type_160-relative, opus dig
  cited): +415 state byte (dispatch :17847; value map in
  `AiState::from_retail` — cut states 2/4/5/10 → Fresh), +404 burst
  (i16, negative lockout :17936-38), +406 poverty latch (:19468-91),
  +460/+462 hate/war per player slot (str_456, neutral 0x601F),
  +628 learn countdowns (:19409-12), +724 cooldown[24] (u16;
  triangulated: [16] = var_756 castle-build stagger). Target + site
  need NO decode: they ride the already-imported carpet entity
  (f146 tr-translated by import_ent, dest_x/dest_y); target_sig
  recomputed = retail's stored +148 exactly (sub_15420 :19041).
  Implemented as `reanchor_rival_ai` (rivals.rs) called from the
  importer's re-anchor loop. mc1hwl0: rival (3,1) target_yaw
  ~25k pairs → 320 rows (top slot 473); target_yaw total 25k→20.6k
  (rest = creature (5,x) share); rand 139k→128k (cast knock-ons);
  conforming 1→46; 8/12 story fixtures drifted shrinking (all lost
  their 3,1:target_yaw atom), promoted; mc1l0 47/47 + mc2l0 24/24
  unmoved; native goldens untouched.
- **§weather churn cadence** — the port under/over-spawns the ambient
  fire/meteor systems: (10,0) 11.6k missing / 1.4k extra (from
  t=355), (10,13) 9.1k missing (meteor showers, from t=9949), (9,9)
  3.8k/5.5k, (10,6) 2.7k/4.6k, (9,1) 1.9k/5.0k. Field-row bulk
  (life/flags/x/y) is the SAME churn as one-tick-offset lifecycle
  overlaps. Untraced; measure per model before patching.
- **(10,2) speed-token contrail — sub_56380 UNPORTED** (entry 1
  residual; 1,304 missing from t=1): the class-12 ACTIVE Accelerate
  token (state 6). Decoded 2026-07-31 (:65131-99): while +48>0 and
  `sub_55DD0` admits — owner cmd-speed v_12 = 3×(+128) on the first
  burst tick (+48==+50, also flags|=0x80 + notify 19) else 2×(+128),
  +126 = v_12, a (10,2) puff at the owner every 4th TOKEN f63 tick
  (id24 = owner's id24, act_life ×4), then sub_55E80 (the burst
  cost). At +48==0: restore v_12 = +128, clear 0x80. Port into the
  strict class-12 arm (world.rs class12_tick phase 0); sub_55DD0 and
  the owner Type_160 v_14 clamp lane still need transcription. The
  heal (sub_56270, state 3) and bolt (sub_56510, state 9 — its (9,1)
  share) arms ride the same dispatch.
- **§census 10000-vs-1000** (mana_max 8.7k pairs, from t=72): the
  claim census under a live rival castle; also rival.castle blink
  (the (3,2)@522 goes missing on 5-8 pairs — the castle state
  machine kills it) and one player.mana_max 58938 blowup (t=10705,
  a census overcount). Entangled with the rival AI-state gap.
- **§player-vitals**: player.mana 7.6k pairs (e.g. t=435 retail 0
  port 1000 — the regen floor applies while retail is suppressed
  mid-drain; the entry-5 clamp misfires on HW's token layout?);
  player.life 1.2k pairs (ambient damage share).
- **§token-blink** (t=3001-3013): the port drops the player's whole
  (12,x) owned-token roster for 13 pairs over the death window —
  the death path scatters/reaps what retail keeps banked.
- **Hands**: 61+18 pairs (quickselect law, mc1 entry 6's twin).
- **PLAYTEST (2026-07-31)**: (1) **HW SNOW GROUND — FIXED same day**
  (player report "reverted to mc1 plains"): the bundle chain was
  correct end-to-end (atlas/palette/shade-LUT all arctic; hiding
  the bundle errors; features switch with --tileset) — the defect
  was the TYPE-PAINT: the baked HW type plane was 94% type 3
  (temperate grass). Decompile-corroborated (opus dig): HIDDEN.EXE
  inserts a SNOW pass `sub_31C10(snlin, snflt)` between rock and
  majority (remc1hw :35792; height > snlin AND 4-neighbor relief
  < snflt AND land → class 6 = snow, then the shared basalt edge
  rule → class 1) AND its rock pass writes steep→class 1 not 6
  (sub_33570 :37269). CARPET.EXE never reads snlin (the old
  mc1_terrain.rs:48 claim was temperate-only truth). Ported
  arctic-gated into mc1_terrain::generate (rock steep param +
  snow pass; bake threads `Game::HiddenWorlds`); BAKE_EPOCH 22→23;
  HW:0 histogram flipped 61,452×type-3 → 63,284×type-6 (+80-83
  snowy-rock transitions), mc1:0 byte-identical, water untouched
  (snow never visits class 0 — water semantics = type 0 safe).
  Screenshot-verified snowfield; full workspace tests green;
  DDLEVELS snlin is a real per-level knob (5 on lvl0 = full snow,
  135 on lvl20 = peaks only). (2) The HOMING METEOR spell has
  always been wrong: wrong sprite (renders like a plain meteor)
  and far-too-weak combat law (retail: 3 guaranteed hits wreck
  Vodor, only rebound defends; the port's rival outheals it) —
  **RESOLVED 2026-07-31 (both defects, opus dig cited; PLAYTEST
  OWED)**. §3c dissolved: the sprite is a CODE LITERAL, not a
  descriptor-table lookup — HW swaps the m16 ctor sub_3A270
  (sprite 42, remc1:46353) for sub_3A5F0 (sprite 76 = the big
  meteor, hw:42451/:42474); SPRITE_STATS row 76's 420x350 extents
  also size the hitbox, so the port's hard-coded 42 was wrong
  look AND collision. Damage: the m16 bolt does NO direct damage —
  the state-17 handler sub_52770_52AB0 copies the bolt's +44 into
  the (10,53) cloud at delivery (hw:58859), so the cloud burns the
  ROW damage 5000 over its 6 ticks (833/tick) instead of the ctor
  3000; the port never copied → ~3000/hit vs Vodor's 10000 with
  ~20/tick regen between casts = outhealed; 3×5000 with the
  regen stall (+383=16/hit, hw:51748) = the retail 3-hit wreck.
  Both fixes HW-gated (spawn_firewall_bolt sprite,
  proj_firewall_tick copy_f44); test
  hidden_worlds_firewall_bolt_is_the_meteor_and_copies_damage
  pins both games; MC1 goldens + all 3 suites unmoved. Rebound
  uniquely defends because HW adds 53 to the model-53 reflect set
  {1,17} (hw:58806) — reflect itself still dormant in the port.
  CORRECTION: the earlier "(9,9) state-14 = meteor" guess was a
  MISID — that family is the Lightning beam segment swarm
  (spawn_zigzag one-frame segments, own=472); the meteor lane is
  (9,16) st17 + (10,53/58) + (10,0). SECOND CORRECTION (player
  push-back, same day): the meteor is RICHLY PRESENT in the take
  (3,005 (9,16) + 3,711 (10,53) diff rows from t≈727; first full
  engagement: cast t=798 slot 546 → homes on creature 183 →
  delivery t=801, cloud slot 512) — the "absent" reading came
  from the RUNNER replaying HW takes under base-MC1 law (next
  entry). CORPUS-CONFIRMED both fixes: bolt type86=76 and
  f44=5000 at birth; delivered cloud f44=5000 (the hw:58859
  copy, byte-for-byte). BANKED LEADS from the dig (INFERRED,
  verify before acting): ① base MC1's cloud should ALSO inherit
  +44 (=24464→191/tick) via the same sub_52770 copy — remc1's
  truncated class-9 table hid it; changing it moves MC1 combat
  balance + goldens, needs its own corpus/playtest pass. ②
  ~~rival at-castle grace mail-wipe "no retail basis"~~ —
  **REFUTED 2026-07-31 (the dig read the intake fn and missed
  the CALLER's gate)**: retail :17971-79 is verbatim the port's
  law (own-castle overlap sub_11950 → grace +331=2; while grace:
  memset the 36-byte mailbox, skip the intake). At-castle rival
  invincibility IS retail. The human's explicit ch0 redirect
  into the castle (:55353-62) is ported; retail has NO rival
  analog — a camping rival's castle takes damage as ordinary
  AREA-blast collateral (player testimony: "the damage is dealt
  to the castle instead"). See the playtest-round entry below
  for the REAL lead this resolves into.

## HW-LAW RUNNER FIX 2026-07-31 (the fall-through trap, new shape)

`verify::build_world` built EVERY MC1-family conformance world
with bare `World::new` = base-MC1 law — the game string selected
only the ASSET variant. **The whole mc1hwl0 triage to date ran
without SPELLS_HW, the m16 homing acquire, or the HW napalm
fork.** Fixed: `new_for_game(GameId::Mc1Hw)` for "mc1hw"
(verify.rs; serves verify, fixtures, and dump — MC2 has its own
builder). This is the mc1hw-survey durable-lesson trap in a new
shape: not an equality gate this time but a DEFAULT CONSTRUCTOR —
sweep `World::new(` call sites, not just `== Game::` tests, when
a per-game seam lands. New HW-law family baseline (full re-run):
z 1.641M · life 596k · flags 412k · x/y ~280k · rand 127k ·
max_life 112k (−16k) · heading 96.7k (−14k) · model 17.0k
(−48%: napalm life-6 fork + meteor lifecycles had been graded
against base law) · target_yaw 20.6k · player.mana 7,567
(unchanged).

Meteor engagement triage under real HW law (pairs 796-810):
- **Birth pair 797 now conforms on identity**: the acquire fires
  (chase 183, latch set, heading/pitch snap — retail 882/134 vs
  port 890/147; residue = pose-latency muzzle offset, capture
  domain). At the doctrine input-delay 2 the cast lands 2 ticks
  late (jitter — cast pairs are inherently capture).
- **Bolt f140 FIXED (both games)**: retail's emit copies the
  MANIFESTATION's +140 (hw:62371/:66151) = the ctor's
  cost-per-shot `a4/count` (:48005) — 5000/26=192 HW, 5000/51=98
  base; the port stamped the row total 5000. cast_firewall now
  computes the quotient (manifestations stay f140-unstamped =
  hash-quiet; nothing ever rewrites class-12 +140 — the castle
  ladder rewrites +136). Corpus row (mana 192-vs-5000) gone;
  the wall_of_fire test pins it.
- **NEW LEAD — cast DEBIT lands one tick late (suspected
  §player-vitals root)**: retail applies the −possess_mana
  regen-delta WITHIN the cast tick (obs: player.mana 10000→5000
  on the cast pair); the port's mana_debit writes mana_delta but
  the vitals application ran earlier in the tick, so the debit
  surfaces one pair late. MC1-wide ordering question (every
  spell, both games) — needs its own round with mc1l0 re-verify;
  candidate root for a chunk of player.mana 7.6k.
- **Fixtures**: t=797 (capture, cast story) + t=800 (open,
  delivery story: cloud delivered same-pair, f44 copy conforms;
  residue = free-stack allocation order 534-vs-512 + the
  jittered second cast) added; suite now 14 fixtures, all green;
  mc1l0 47/47 and mc2l0 24/24 untouched.

## METEOR PLAYTEST ROUND 2 (2026-07-31): mostly certified; the
## residual "Vodor tougher than retail" TRIAGED to ONE chain

Player: meteor sprite + 3-hit damage feel right; Vodor still
harder to kill than the retail playthrough ("starts healing
fast"), possess homing "feels broken" on unclaimed balls, and
respawn "way faster than retail". Adjudications:
- **Possess homing — RULED FAITHFUL, don't re-open**: retail's
  acquire case 1 gates BOTH candidate lists on `+58 != 0`
  (hw:60176/:60194) — identical to the port filter — and balls
  SETTLE to +58==0 forever after their 128-tick ballistic
  window. A settled unclaimed ball is never a homing target in
  retail either; the lob homes only on fresh still-bouncing
  balls, and old balls are claimed by aim + the possession
  flash's area-claim at the blast. Mid-flight steering is fully
  corpus-graded (every tick of every imported lob).
- **Respawn law — timer + cadence VERIFIED faithful** (formula
  32·((255−tempo)>>3)+32 at :55555-57 byte-identical; per-tick
  countdown + castle check + castle-less elimination :55601-30).
  The port's "fast respawn" is NOT a timer bug — see the chain.
- **THE CHAIN — RESOLVED 2026-07-31 (the CASTLE-COLLATERAL round,
  see Resolved for the fix inventory)**: the corpus lever paid off
  exactly as banked. Slot 522's record: castle born t=73 at life
  20000, UNTOUCHED until t=9330, then damage in runs of −833/tick
  (with −1666 overlap ticks), dead at t=9457 — 833 = 5000/6 = the
  meteor's (10,53) napalm cloud burn, and each burst is 7×833 per
  cloud (14 for two overlapped). The retail meteor bolt and cloud
  both carry chase=522: **the homing acquire locks the CASTLE
  itself** — that is how player fire aimed at a castle-camping
  Vodor fell his castle. Four port defects found and fixed (each
  corpus-validated on the 9325-9345 window; castle life diffs
  12→1): ① ent_overlap widened +78 UNSIGNED — the castle's 0xE000
  z-center marker read as +57344 instead of −8192, z-orphaning
  every castle out of the area-write pre-pass; ② the castle never
  carried the marker natively (the port skipped sub_37150's
  +78=0xE000 write *because* of ①); ③ the acquire candidate set
  lacked castles (retail's list-1 walk branches model 2 to a
  dedicated castle scorer in cases 0/3/4 AND 0x10); ④ the (10,53)
  cloud ran post-decrement — retail is class-10 PRE-decrement (7
  burns from a 6-life cloud, 5831 delivered not 5000). Remaining
  window rows = capture domain: the pair-9329 birth edge + both
  chase rows are cast-timing skew (the port's replayed cast fires
  a pair off, allocating its own bolt), and the 849 acquire miss
  is the terrain-closure z (port castle at pristine 5600 vs the
  raised mound's 7168 pushes the pitch bearing ~7 units outside
  the 0x71 cone). Exemplar fixture t=9331 added (castle intake
  CONFORMING inside it). Second-order Home/camp cadence: NOT
  re-checked this round — revisit only if a future take shows a
  camping-cadence divergence.

## MC2 take-2 (2026-07-30 re-record; FIX ROUND 1 LANDED 2026-07-30)

The re-recorded mc2l0: **11,524 ticks gapless, check-decode exact,
`channels.input: "raw"`** (the MC2 input frame validated live — mode 7,
arrow keybinds), spell upgrades + end-to-end level completion. 11,523
pairs → 7,762 fixture-grade (33% torn). Suite re-extracted per doctrine
(`conformance/mc2l0.json`, 24 exemplars, 23 open / 1 capture; sigs
re-promoted after the fix round). Post-triage it sat at 0 conforming
by construction: the §terraform capture family (village growth regrades
the hill at ~t=751; house z re-snaps both sides, ours to the pristine
plane) puts (10,45) z rows on every later pair — 186k of the 249k
then-remaining z hits. **The 2026-07-31 kinematics round moved it to
11 conforming + 8 rng-only** (total diff rows 329.9k → 300.2k).
Port-side conformance now lives in the t<751 window
and in the per-family totals, which the fix round moved hard:
player.mana 5,894→232 pairs · player.mana_max 5,939→458 · entity
mana_max 6,296→599 · player_ent_idx 6,759→out of top-20 · owner
2,724→~250 · rand 21,655→10,983 · player.castle (a fix-round
regression, then fixed) 6,083→0. Pair 0→1 = ONE row (the regen-cadence
lead below). Fix round (all in Resolved below): §class15 + spellbook
import, the @0x1A id-fusion/claim-census/economy block, the fire
activation bit + (10,0)/(10,6) field map, and the strict-retail MC2
sweep laws (newborn skip, disabled skip + ghost records, ghost slot
reallocation) — plus the tile-chain-cycle OOM guard (pair 9074's
100 GB allocation: a linked ghost's slot reallocated → chain cycle →
unbounded `area_write` victim walk).

Open leads, take-2 (verify with `--start <t> --limit <n>` windows; run
the full file under `ulimit -v` — see the pair-9074 note):

- **§effects per-model field-map grind — the dominant port residual**:
  the class-10 effect models keep per-model homes the uniform alias
  table misses, exactly like class-15 did. Landed: (10,0)/(10,6)
  (@0x2A amount → f140, @0x2C flicker/lift → f44, @0x90 dead-0).
  Remaining: small fire z residues (the sub_580E0 alt-core arg
  order?), smoke ±1-step tails, per-model rand rolls ((10,13)
  emitters, (10,12) hit-flash), (10,1) explosion cluster fields.
  Measure per model — the two-wrongs trap is real (the activation-bit
  fix EXPOSED the f44 aliasing; totals briefly rose).
  **SWEEP SLOT-ORDER LAW LANDED 2026-07-30 — the smoke families
  collapsed.** The universal "newborns never tick" gate was the t=0
  special case: retail's frame pass (EF:40116) is a bare ascending
  pointer walk — a mid-pass spawn ticks the SAME pass iff its slot
  lies ahead of the cursor. The chimney corpus pins it (9 births/
  tick, lives 31..−1, NO life-32 record ever). Gate removed; the
  natural loop serves both native and strict (DEVIATIONS.md entry
  updated — the dome guard is faithful, not a deviation). t<751:
  total rows 47.8k→34.1k, (10,14) life 6,060→308, y 6,029→1,061,
  (10,13) life/y gone. REMAINING smoke rows are capture-domain:
  newborn rand/actSpeed derive from the reused slot's STALE seed
  (SetSmoke4 steps the slot's leftover rand once; the slot's last
  ghost obs is 1-2 frames before the pair) and newborn drift reads
  stale yaw — not reachable from a single-pair closure. The extras
  (~9/pair) are the newborn capture tear (born after the recorder's
  window passes the slot; present in port's end-of-tick obs,
  absent from retail's mid-frame one).
- **§casts misfire — FIXED 2026-07-30 (the pane theory was WRONG)**:
  the recorded cursor sits dead-center (320,199) — no pane click.
  The real cause: the RIGHT BUTTON is already HELD on the
  recording's first frame (a hold crossing the level boundary), and
  the harness ring's default pre-fill read "released" →
  manufactured a press edge → the t≈3 phantom (9,17). verify_mc2
  now extends the first input frame's held state backward (retail
  latched the press before t=0; its first real edge is the t=5
  re-press). The substitution rows cleared. The --input-delay
  re-sweep 0..3 ran FLAT (<0.2% — this window barely casts);
  delay 2 stands.
- **Cross-pair StageVar leak — FIXED 2026-07-30**: the live
  StageVars2 rows @0x365F4 now decode (`RetailMc2::stagevars`, raw
  8-byte rows [kind, flags, chain, cadence, payload]) and overlay
  the port table's RUNTIME lanes per pair (kind/flags/chain/
  cadence + kind-6/7 param; loader-derived hold/watch fields stay
  from the build — the &2-clear payload can be a bound-entity
  guest POINTER (EF:4740), which the sv1 lanes already rebuild).
  The t=726 sv1/sv2 self-drift pair is now FULLY conforming. Note:
  mc2l0's recorded rows are byte-identical t=0..751+, so this
  overlay = a per-pair reset; a take with live trigger churn will
  exercise the lanes for real.
- **player.mana regen cadence — narrowed**: the pending delta @0x88
  applies mana@N+1 = mana@N + d88@N on almost every pair, EXCEPT a
  freshly-stamped −100 survives ONE extra frame before applying
  (measured pairs 0→1 and 16→17; the port applies immediately →
  ±100 on ~232 pairs). The MC1 entry-5 resolution EXPLAINS the
  mechanism (slot-order jitter between the stamping spell event and
  the carpet's apply — remc2's sub_68DE0 cost stamp is LIVE, so
  MC2 stamps −cost then applies next frame when the event's slot
  follows the carpet's). RE-MEASURED 2026-07-30: the stamp pends
  TWO recorded frames (d88=−100 at obs 0 AND 1, manifestation
  timer FROZEN between them = the recorder's mid-frame window
  catching pre-apply state), so a single-pair import cannot
  distinguish the hold from the apply — an f2e-first-tick clamp
  bought exactly one pair and was reverted. Reclassify toward
  capture unless a cleaner discriminator appears.
- **mana_max residual** (599 rows): the claim census within the tick
  — retail's t=64→65 jump (+187) lands mid-frame (a ball absorb the
  port's census sees one tick late?). Same family: owner retail-152
  rows (t=620 slot 49 — a just-learned manifestation's adopt path).
- **Completion arc** (t≈11,000+): still untriaged.
- Familiar carryovers: §terraform (capture), §wander turn law,
  §balloon (3,3) extra, §rng under load.

## MC2 open leads (mc2l0 take-1 triage 2026-07-30; fixtures retired with the take-1 recording — family shapes carry over)

Post-fix family table (2535 fixture-grade pairs, per-entity-torn
slots excluded from field comparison — see the MC2 capture caveat):
z 43128 (of which ~38k = the §terraform capture family) · rand 3874 ·
speed 2955 · x 2571 · y 2409 · mana_max 1114 · player.mana 945 ·
heading 764 · life 712.

- **§effects — the (10,13)/(10,14)/(10,0)/(10,60) fire-smoke band**
  (fixtures t=0/6/21/24): the dominant PORT residual. Lifecycle
  churn (5.7k missing / 3.1k extra (10,14); 2.3k/1.2k (10,13)) plus
  motion (speed ±4 = one decel step families beyond the torn
  residue) and draw cadence (retail (10,14) draws 0/tick with rare
  9-draw bursts, (10,60) draws 3/tick — measure per model before
  patching effects.rs). Entry point: `mc2_smoke_particle_tick` /
  `mc2_smoke_emitter_tick` vs EF:35618-35700.
- **§wander — (5,1)/(5,13) turn law: RE-RULED 2026-07-31** (see
  Resolved: KINEMATICS ROUND rulings): the law is byte-exact; the
  isolated ±22/±45 blips self-heal (capture). The REAL residual is
  the **held-state split**: on the sustained ±341 runs retail parks
  the goat in action 15 (+7 controlled, sv2=2) while the port
  wanders at 9 — the StageVar hold-gate isn't latching that
  creature (slot 81 exemplar, t≈2380-3096). Port lead, own dig.
- **§balloon — (3,3) extra-in-port** (t=1913): 549 extra balloons
  from t=1807 — the port's castle dispatches balloons retail does
  not send here (likely gated on economy state the importer seeds
  differently, or a cadence lead in `mc2_balloon`).
- **§rng — global-LCG divergence** (t=1520): 62/3640 pairs, all
  under load (draw counts high); likely a draw inside one of the
  §effects laws or spawn paths, will collapse with them.
- **§houses — (10,45) life deltas** (t=2181): +250 family (militia
  pop refund? repair?) on top of the terraform z (capture).
- **§castle — (3,2) player_ent_idx + z** (t=2204): the castle's
  sphere-owner field and z datum drift late in the run.
- **§player-vitals — player.life** (t=2347): 66 pairs of human life
  drift; partially entangled with the cast closure (capture) — the
  ambient-damage share is the port lead.
- **MC2 importer approximations (accepted, watch for families)**:
  the live StageVar table imports from LEVEL data, not the runtime
  rows @0x365F4 (FIRED/cadence bits stale across trigger ticks);
  `mc2_allied`/`mc2_aura_claim` clear at import; rival spell/XP
  columns (str_611) not imported (level 0 has no rivals); the
  scratch quartet f26/f36/f46/f50/f56 uses best-single-home
  mappings (conformance.rs `import_ent_mc2` doc) — f56 ← @0x36 was
  A/B-tested (the b38 mapping poisoned kinematics +14%).

## MC2 mc2l4 + mc2l30 triage (2026-07-31)

The first-cut triage of the two takes cut from the 2026-07-30 mc2:4
session. Four fixes landed during the round (Resolved below: worm-bob
import lane, lightning trail nodes, castle phantom-upgrade lane, the
cave ambient rand tail + turn anchor); the families here are what
remains, each with its dive verdict. Suites:
`conformance/mc2l4.json` + `conformance/mc2l30.json` (re-extracted
post-fix).

- **§l4-guard-terrain — the (5,15) castle-guard family (BOTH takes'
  #1 residual, ~170k rows l4 / ~130k l30): CAPTURE (terrain
  closure)**. (5,15) = the wizard-manager-spawned defensive archer
  guard (`sub_5FF50` EF:61488-502 stamps yaw=roll=512 + terrain-alt
  spawn z; behavior row 83 grounds it to `getTerrainAlt` every tick
  via the ported `mc2_alt_core`). Retail's guards walk up a
  runtime-terraformed castle-mound ramp (+15 z per +30 x, 512/544
  plateaus); the `.mgcr` has no terrain channel, so the port replays
  on pristine planes — z tracks the missing mound, the pristine tile
  TYPE trips the wander die-gate → action 121→124 (prekill), and the
  die-gate's early return freezes the guard's rand. One root, three
  fields; port laws verified faithful line-by-line. Rides the
  standing §terraform/terrain-channel remedy. The sv1/sv2 rows
  nearby are the SEPARATE mc2:04 death-watch/hold choreography;
  (9,13) arrow churn is part guard-downstream, part cast-timing.
- **§sphere — RESOLVED 2026-07-31** (see Resolved: KINEMATICS ROUND
  fix 4): the settle law (`byte@0x39 || kick`) + @0x2C z-vel import
  + latch imports + exact bounce/merge/rotation landed in the
  shared `ball_tick` MC2 arm. l4 (10,39) 37.9k → ~3.8k rows;
  residual = terrain-closure z + birth edges. The l30 sphere z bulk
  (−1169/−542 constants) was always the cave mound/plateau terrain
  closure — capture.
- **§l30-churn residual — the coupled fire/smoke draw+lifecycle
  family**: after the cave-tail fix ~22% of l30 pairs still
  mismatch rng, all count-mismatches on churn-heavy ticks. Two
  mechanisms: (a) ~~the MC2 per-tick reap lag~~ **RESOLVED
  2026-08-01 — the same-tick reap landed (see Resolved); the
  extra side collapsed but the 202 rng pairs survived UNCHANGED,
  so the rng residual is entirely (b)**; (b) ~~the per-ENTITY
  `rand_0x14 += counter` sites~~ **REFUTED as the l30/l4 driver
  2026-08-01 session 4** (the three sites EF:13140/13220/20521
  belong to the (5,10) doomsday pyramid and the (5,27) hydra
  branch bolt — NEITHER model exists on l30 or l4, censused
  across the takes; the perturb law itself LANDED anyway, see
  Resolved). The REAL (b) = the **VOLCANO CASCADE**: the human
  map-casts Volcano (spell 18) at t≈258-262 at (67.5,110.5) and
  (111.5,10.5) → (10,9) domes (both sides spawn them, ±2 ticks
  cast latency, slot-skewed) → dome life==3 beat → (10,18)
  summit controller (retail slot 134 @274) → (10,19) column +
  (10,16) + (9,0) + 4×(10,14) smoke ring → (10,0) fire cascade
  spreading tick-by-tick. The 202 rng pairs cluster EXACTLY on
  the eruption windows (274-468, 478-518, 2536-2776 — the SAME
  site re-erupting — plus singles 4359/4834/4866/6314-22/6490/
  7642-50/7762-78/7810/7934/7954/7970/8114-22/8330; a third site
  (201-206,0-11) at 2530-2537 emits (10,0)+(9,3)). The port
  erupts ONCE (under-sized cascade — gross missing 1,693 (10,0)
  vs 346 extras) and NEVER re-erupts. Two port bugs indicted:
  the (10,0) fire-entity spread law (also feeds l4's 2,005
  missing — no volcano there, combat ignitions) and the summit
  column re-erupt cadence. Dig round launched same session.
- **§l30-terrain — the (14,5) flat-512 plateau (CAPTURE, with a
  port-side check owed)**: 12 of the 14 (14,5) markers sit exactly
  −1664 (retail 2176 plateau at tiles (160-171,194-205), port flat
  512); nearby slots track terrain within ±32. Both sides ground-
  snap faithfully — the port's mc2:30 heightfield simply lacks the
  plateau. ~~OWED: load-time vs runtime~~ **ANSWERED 2026-08-03
  (session 6, dig C): RUNTIME-terraformed — `mc2_dome_tick`/
  `sub_31940` (EF:23193) direct heightmap writes; pure capture,
  nothing portable.** The l4 face of the same question: the (5,4) ARCHER
  family walks at a CONSTANT −192 z from t=0 (slot 210, byte-
  identical dynamics) — a pristine-plane datum gap at its site,
  present before any runtime edit can exist. (5,4) XP-scroll z, (14,3) −16, (15,19) token-fall
  (slot 92: port clamps up to its pristine 1296 floor while retail
  falls to 288) are the same terrain-closure story.
- **§castle follow-ups** (split from the resolved phantom-upgrade
  lane): (a) ~~painter @0x28 owner projection~~ **RESOLVED
  2026-08-03 (session 6, dig E — parent castle lane landed; see
  Resolved owner entry)**; (b) the (3,3) stage-piece −128 z residual post-rise —
  re-measure now that the phantom upgrade is gone; (c) the (5,1)
  at slot 92 killed at t=0 by `mc2_building_clear_tile` (build
  footprint clear) while retail's construction hasn't cleared that
  tile this tick — build-window timing; (d) player.mana_max
  claim-census within-tick (the standing mc2l0 lead, NOT a castle
  ripple — the mc2l4 castles are rival-owned).
- **§wander-drift residual — (5,0)/(5,3): RE-RULED 2026-07-31**
  (see Resolved: KINEMATICS ROUND rulings): the walker turn law is
  byte-exact — the smooth heading drift is capture (chaotic
  amplification, rand-matched). **SESSION-6 NOTE: the (3,3)
  BALLOON altitude half is RESOLVED (dig A — row-base import +
  sub_580E0 servo; see Resolved).** **SESSION-7 SCOUT (2026-08-03):
  the "multipart flyer z-bob" lead is CLOSED — capture (see
  Resolved "MULTIPART FLYER Z-BOB RULED CAPTURE"); no M0/M3
  altitude source exists to trace; roster mc2-flyer-drift-m0/m3
  flipped open→capture.** What survives as PORT-side work from
  that scout is the l4 **terrain datum gap** sizing (437 tiles at
  −23..+8 height bytes across five windows — a recording-format-v2
  terrain-channel / import question, not an entity law); plus the
  l4 t=17954 mass spawn-wave divergence (dozens of slots at once,
  unexamined).
- **§drip placement — (10,86)/(10,87) residual**: at the best
  cadence anchor (turn0&7==0, phase-scanned) the drip still lands
  9 missing/56 extra per 2000 pairs — the target-tile walk consumes
  the global stream, so any upstream rng divergence relocates the
  drip; expected to shrink with §l30-churn.
- **§lightning residual — (9,9)/(10,23) extras+missing**: the
  input-delay-2 cast-timing skew + retail's parked ghost husks vs
  the port's free-list reuse — capture-domain (the field families
  resolved, see Resolved).

## Resolved

- **THE AIM-Z BRACKET / CASTLE-FLAG HOMING (2026-08-09, player
  report).** Homing, acquisition and impact placement measure a
  target at z + signed +78 EXCEPT model 2, measured RAW — MC1
  sub_524C0/sub_524E0 (:62503-14, bracketing sub_52550 homing and the
  sub_54A90 scorer), MC2 twin sub_65580/sub_655A0 (EF:62750-67,
  bracketing sub_65610/sub_655C0/sub_68490); the MC2 class-3 acquire
  walk additionally routes model 2 through the raw-position castle
  scorer sub_685D0 (EF:54790/54899/54945 — same cones/score). The
  guard reads the MODEL byte alone in both games (MC1 +65, MC2 +64).
  Ported as `Ent::aim_z`, now used at every aim site. What was wrong:
  MC1 homing/acquire lifted castles by their +78 = 0xE000 (−8192)
  collision marker — every homing projectile (fireball, meteor,
  volcano…) steered at a point 8192 UNDER the mound and dove sharply
  at the castle base, with only the (already-guarded) landing
  teleport putting the burst at the flag; MC2 guarded on the CLASS
  byte (a remc2 field-naming trap — `model_0x40_64` IS the model,
  value key "2 - castle"), so castles took the same wrong lift and
  class-2 statics missed their correct one. Side law, same guard:
  model-2 CREATURES (the MC1 m2 bee) are aimed at their RAW z —
  L005 goldens D/E re-pinned for exactly this (the D-window fireball
  combat fights bees). Pinned by
  `homing_steers_at_the_castle_flag_not_under_the_mound` +
  `mc2_flyer_homing_steers_at_the_castle_flag` (both proven
  non-vacuous against both bug variants). EXPECT MOVEMENT in the
  z-offset conformance families on the next takes — castle-homing
  flights (mc1hwl0 slot-522 meteor churn is castle-homing) should
  grade toward retail. NOT this fix: the castle spawn z +64 residual
  (retail 7872 vs port 7808, mc1l32 t=5502+ exemplars) — that's the
  entity-z datum lead, still open. FLAGGED, not yet fixed: MC2
  castle-piece turret fire aims at the target's RAW z where retail's
  sub_655C0 lifts non-model-2 targets (mc2/castle.rs::mc2_piece_fire
  EF:30292 — turret shots aim a half-box low at creatures/wizards);
  separate change, needs its own verification pass.

- **⭐ SESSION-10 CLOSE (2026-08-05, THE LANDING ROUND) — authoritative
  full-take numbers on the final tree, all six takes, suites promoted
  203/203 as-expected, 0 regressions, `MGC_REQUIRE_GOLDENS=1` 0
  failures.** Every slate-A item landed or closed-by-refutation; three
  player-report digs landed on top (demon size law PLAYTEST CERTIFIED;
  camera EYE_LIFT; terrain keyframes decided). Numbers (conforming /
  unexplained field·missing·extra / rng pairs):
  - **mc2l24: 7,166 conf** (was 1,163 post-cast-phase) / 453,195·407·
    5,913 / **rng 4** (was 8). Conf-or-explained 19,579.
  - **mc2l0: 5,520 conf** (was 2,232) / 3,372·107·28 / rng 2.
  - **mc1l0: 506 conf** (was 501) / 10,585·87·101 / **rng 0**.
  - **mc2l4:** 14,709 of 17,711 fully explained (83.1%) / 10,523·116·
    15 / rng 5 (was 6). Rival split live: terrain-z 7,934 · ai-residual
    4,190 · mana-mirror 1,288 (purse-mirror decision still owed).
  - **mc2l30: 13 conf** / 6,428·70·40 / rng 18 (was 19).
  - **mc1hwl0: 49 conf** / 2.17M·27,418·9,844 / rng 28 — still
    terrain-dominated = the keyframe channel's first customer.
  - Landmark rule collapses: `mc2l24-static-terrain-z` **375,572 → 37
    rows** (prop-z inversion); `mc2-guard-terrain` residue 1,197 (l24
    doomsday ground, narrowed rule); `mc2-castle-pad-z` 17 l24 (sensor)
    + 166 l4 rival authored pads (real closure, keyframe territory);
    `mc2-terraform-houses` 1. `mc2-walker-ground-z` 233,822 = the next
    terrain lever, awaits keyframes.
  - The conforming jumps come from the compound of: 180° turn tie-break
    (both games, all takes), manifestation-slot cast order (every MC2
    cast was one tick early), prop-z terrain inversion, merge ring-walk
    + hard-free, muzzle endpoint admission, pool-base decode (session
    9), and the pad replays (session 8) grading together for the first
    time.
  - Instruments live corpus-wide: press-position fold (off by default),
    cycle-ring detector (0 hits all takes, as predicted), per-pair
    recycle/drop telemetry.

- **THE PORT FIRED EVERY MC2 CAST ONE TICK EARLY — the ARM and the
  LAUNCH are two different entities' ticks, and the port ran both at
  the caster's pool slot. The l24 "19 phantom possession bolts" are
  that one tick. LANDED 2026-08-04 (session 10, possession
  re-attribution dig).** Re-owns the divergence the session-9 entry
  parked on `mc2-rival-ai-lanes` and then had to give back when the
  corpus proved l24 is single-player ("(a) the l24 late-window
  divergence is NOT rival-attributable — look at the human/class-9
  path"). It is neither rivals nor input skew: it is retail's
  entity-walk order.
  - **THE CENSUS.** `verify-deltas --csv`, mc2l24 t=40000+4000 (the
    window the "19" was measured in): **24 extra (9,1), 0 missing**,
    all roster-swallowed by `mc2-cast-timing-extra`, at t=40123/29/34,
    40156/61/65/71/86, 40405/10, 41288/93/98, 41308/12/18/22/27/32/
    37/41/46/50/59 — a ~5-tick click cadence, and `MGC_CAST_TRACE`
    puts an aligned press edge on every one of them. So retail took
    the SAME presses. Probing the raw states says what it did with
    them: at t=40124 the right-hand manifestation (slot 9, class 15
    model 1, action 3) goes `word_0x2E_46` 0 → 3 with the pool still
    holding **zero** (9,1); at t=40125 the timer reads 2 and the bolt
    is there. **Retail armed on the press frame and launched on the
    next one.**
  - **THE LAW, AND ITS SPAWN-FREE ORACLE.** `sub_5F660` (EF:60874) and
    its arm `sub_5F7B0` (EF:60973) only stamp `word_0x2E_46 =
    duration`; they are called from the tail of `sub_5F380`
    (EF:60850-62), which is the HUMAN entity's own action
    (`AddPlayer03_00_5E010` EF:59954). The LAUNCH lives in the
    manifestation's own **class-15 action** (`sub_69640` EF:55915 for
    possess, dispatch EV:3491-92), run at ITS pool slot in the same
    ascending `UpdateEntities` walk (EF:40116). So the arm reaches the
    effect state in the same frame **iff the manifestation sits ABOVE
    the carpet**. That is measurable without looking at a single
    spawn: on the record where a timer leaves 0, `word_0x30_48 −
    word_0x2E_46` is 0 if the effect state has not run yet and 1 if it
    has. Over the whole corpus — **mc2l0 713/713 arms lag 1**
    (manifestations 153/154, carpet 152), **mc2l4 1,333/1,333 lag 1**
    (266..275 vs 265), **mc2l30 666/666 lag 1** (85/86/87/93 vs 83),
    **mc2l24 464 above → lag 1 and 3,184 below → lag 0**, with the
    only exceptions being the CASTLE spell, whose timer is an upgrade
    lock and never a countdown (castle-cost entry). Zero
    counter-examples in 6,360 arms. l24 is the odd take because its
    hands re-home into low slots after the opening (spell 1 at slot
    7/9/10, spell 0 at 6/78/79, spell 9 at 84) while carpet stays 116.
  - **THE FIX.** `World::mc2_player_cast_pass` (world.rs) now ARMS
    only; the per-manifestation effect state moved into
    `World::mc2_manifestation_pass`, called from the class-15 walk arm
    (`mc2_spell_token_tick`'s state-3M case, which used to `return`
    early because 3M "is not a jar"). `mc2_cast_tick`'s loop body split
    out as `World::mc2_manifestation_tick` (cast.rs) so both callers
    share one implementation. **Scoped to a POOLED human**
    (`mc2_carpet_slot != 0` — the conformance import): native MC2 keeps
    the human out of pool at slot 0, i.e. BELOW every manifestation,
    which is already the law's `above` arm, so the pre-walk combined
    pass stays exactly as it was and no golden moves.
    `MGC_NO_MANIFESTATION_ORDER=1` restores the pre-dig placement.
  - **A/B (one frozen binary, env-toggled, back-to-back).** mc2l24
    40000+4000: **(9,1) extra 24 → 0** with missing still 0, entity
    extras 691 → **632** — (10,12) 6 → 3, (9,28) 6 → 0, (10,75) 11 → 0,
    (10,0) 109 → 100, (9,9) 21 → 20, (9,3)/(9,26)/(10,22) 1 → 0 each —
    missing 957 → 959 ((10,23) +2), unexplained field 10,520 →
    **10,472**. mc2l30 0+2000: gross rows 39,184 → **39,038** (−146:
    134 `mc2-cast-timing-fields` `rand` rows on (9,1)/(9,10), 18
    fire-churn, 1 pose-phase; +2 unexplained (5,15) `rand`) — the
    launch moved two slots up the walk and the bolt's LCG seed now
    lands where retail's does, a second independent confirmation.
    mc2l0 0+2000: conforming 1,677 → **1,678**. **mc2l4 0+2000 and
    mc2l24 0+2000 are BYTE-IDENTICAL** — every manifestation is above
    the carpet there, which is exactly what the law predicts.
  - **THIS IS THE ±1 PHASE the session-9 entry measured**
    ("`retail_arm − port_fire` is +1 on 227 of 408 casts, +2 on 96, 0
    on 39") — that +1 was never input latency, it was this, and the
    39 zeros are the arms whose manifestation sat above the carpet.
    The proposed corpus-wide `--input-delay 3` knob is therefore NOT
    needed and must not be taken: it would model an entity-order law
    with a capture knob and would break every take where the
    manifestation is above the carpet (l0, l4, l30 entirely).
  - **IT IS NOT the mc2l0 "(10,12)-pulse vs (9,1)-bolt SUBSTITUTION"
    lead — CHECKED AND STILL OPEN.** l0's manifestations are all above
    the carpet, so this fix is a near-no-op there and the residue is
    untouched: mc2l0 0+2000 keeps **17 missing (10,12)** with zero
    (9,1) extras, byte-identical across the A/B but for one (9,10)
    extra row. That lead needs its own dig.
  - **ROSTER PROPOSAL (described, not applied).**
    `mc2-cast-timing-extra` / `mc2-cast-timing-missing` no longer carry
    a launch-phase family on any MC2 take; re-measure their hit counts
    and re-scope the notes to whatever residue survives (l24 40k keeps
    24 `mc2-cast-timing-missing` (9,0) rows, which are the RAPID
    fireball stream, a different story).
  - Test: `mc2_human_cast_arms_at_the_carpet_and_launches_at_the_manifestation`
    (world.rs — the old `..._pops_the_free_stack_after_lower_slots`
    rewritten to the two-tick geometry its own law now implies: tick 1
    arms and launches nothing, tick 2 launches from the LOW
    manifestation slot and therefore pops the free stack BEFORE the
    higher emitter's puff). Non-vacuous: under
    `MGC_NO_MANIFESTATION_ORDER=1` the first assert fails (the cast has
    already fired and expired inside tick 1). **No golden moved.**

- **THE (5,23) RETRY-LEG PAIR IS THE 180° TURN TIE-BREAK, NOT A RETRY
  ORDER BUG — retail's turn helper unwraps the angle delta only when
  it is STRICTLY past a half-turn, so an exact half-turn keeps the RAW
  sign. LANDED 2026-08-04 (session 10).** Closes NEW LEAD ② of the
  riser-endcap entry ("the 2 surviving (5,23) heading rows are exactly
  512 apart — a retry-3 / retry-2 leg disagreement worth one narrow
  dig"). The leg was never in doubt: both sides take retry 3. The
  512 is `2 × 256` = the commit turn applied in opposite directions.
  - **THE PAIR, RECONSTRUCTED TO THE UNIT.** mc2l24 t=15044 and
    t=15129, both slot 363, `field:5,23:heading` retail **1205/1287**
    vs port **1717/1799**. The dweller's stored yaw and its wander
    target (`roll_0x20_32`, the port's `f34`) are EQUAL at both ticks —
    437 and 519 — and the move core's third retry
    `(yaw0 + 0x400) & (0x700 + LOBYTE(yaw0))` (EF:8846) is the exact
    ANTIPODE for both (the mask clears nothing): 1461 and 1543. The
    commit then turns back toward the target, capped at row 91's
    `subtype_160_0x2_2 = 256` — from exactly 1024 away.
  - **RETAIL'S SIGN.** `sub_582F0` (Sound.cpp:6580; MC1's twin
    `sub_42240_42580` :52664 is the same body, and the decompile marks
    it SYNCHRONIZED): `v3 = (tgt & 0x7FF) − (cur & 0x7FF)`, unwrapped
    by ±2048 **only when `abs(v3) > 1024`** — strictly greater — then
    `v3 / abs(v3)`. At the tie `v3 = 437 − 1461 = −1024`, no unwrap,
    sign −1 → 1461 − 256 = **1205**. The magnitude helper `sub_582B0`
    (Sound.cpp:6569, MC1 `sub_42210_42550` :52652) folds on the same
    strict `> 0x400`, so it returns 1024 and the cap takes 256.
  - **PORT DEFECT.** `Gen::turn_step` derived the sign from the
    WRAPPED delta — `(tgt − cur) & 0x7FF <= 1024 → +1`. That agrees
    with retail on every delta except the one case `cur − tgt == 1024`
    exactly, where it turns +256 instead of −256: a full `2 × cap`
    error, and the antipodal retry lands on it every time it fires
    against a creature already facing its wander target. Fixed by
    porting the retail body as `Gen::turn_sign` (mc1/mobs.rs);
    `MGC_NO_TURN_TIE=1` restores the wrapped form.
  - **THE MOVE CORE IS STILL CLEAN** — the riser entry's ruling stands
    verbatim; nothing about the retry ORDER or the blocked test moved.
  - **A/B (one frozen binary, env-toggled).** mc2l24 t=14680+500:
    **(5,23) rows 2 → 0**, window gross 27,754 → **27,632**,
    unexplained field 13,429 → **13,307**, collateral ALL downward and
    all class-5 — (5,20) −74, (5,17) −39, (5,26) −7, nothing up.
    **mc1l0 0+2000: conforming 525 → 530**, conforming-or-explained
    1,428 → 1,441, unexplained field 3,391 → 3,371. mc2l0 0+2000:
    conforming 1,678 → **1,703**, gross 3,586 → 3,554. mc2l30 0+2000:
    explained 1,527 → 1,536, gross 39,038 → 39,024. mc2l4 0+2000:
    explained 1,313 → 1,318, unexplained field 4,059 → 4,049. Both
    games, every take, one direction.
  - **⚠ GOLDENS MOVE — THE RE-PIN RITUAL IS OWED AND NOT PERFORMED.**
    `turn_step` is shared creature-turn code, so fixing it is a
    deliberate behaviour change: `level_005_golden_state_hashes`,
    `flight_tier_golden_state_hashes`, `mc2_cave_behaviors_and_goldens`
    and `mc2_slice_behaviors_and_goldens` all diverge (level_005 from
    hash index 2 on). `MGC_NO_TURN_TIE=1` makes all four green again,
    and nothing else in the tree fails either way. The four re-pins are
    a player decision, exactly like the banked rival purse mirror; the
    fix is landed ON so the decision is visible rather than rotting in
    the backlog.
  - Test: `turn_step_breaks_the_exact_half_turn_toward_the_lower_angle`
    (mc2/mobs.rs) — replays both recorded pairs through the antipodal
    retry and the capped commit, pins the sign both ways round the tie,
    and asserts every non-tie delta is unchanged. Non-vacuous: under
    `MGC_NO_TURN_TIE=1` it fails reproducing the port's exact old 1717.

- **THE l24 FOUNTAIN "OVER-SPAWN" IS NOT AN OVER-SPAWN: the fountain
  is byte-exact and the extras are the MANA-SPHERE MERGE, which the
  port ran with the wrong SEARCH SET and the wrong TEARDOWN. LANDED
  2026-08-04 (session 10, fountain-over-spawn dig).** Closes the
  session-9 pool-base entry's parting call ("the (10,39) extras in the
  fountain window (673 after the fix) are a real over-spawn") — the
  premise is falsified and the real law is two decompile routines the
  port had never read.
  - **THE FOUNTAIN IS EXONERATED, BY LCG COUNT.** `sub_32CF0`
    (EF:24007, action 98) launches `for (i = 0; i < 3; i++)` spheres
    and spends **five** `9377·r + 9439` draws on each one that
    allocates (speed, apex, colour, mana, yaw — all inside `if (v1x)`).
    Probe over the corpus's own (10,91) spawner (mc2l24 slot 662,
    t=64490..64559): the spawner's `rand_0x14_20` advances by
    **exactly 15 steps on every one of the 70 ticks** — 3 successful
    creations per tick, never 2, never 0. An identity-keyed census
    ((slot, rand) over the whole pool) agrees: **3 (10,39) births per
    tick, every tick**. The port spawns 3 too. The window's 662 extra
    spheres are 2.8 retail DEATHS/tick of which the port reproduced
    only about half.
  - **DEFECT 1 — THE PARTNER SEARCH IS A MAP-TILE RING WALK, NOT A
    POOL SCAN.** `sub_10A50` (EF:3876) and its MC1 twin `sub_11D10`
    (:17127) are the same routine: base tile = `((pos + 128) >> 8)`
    — **ROUNDED**, not floored; ring count = `(applied_pitch + 255)
    >> 8` (the searcher's own +80 extent in tiles, and with **no**
    `.max(1)` — the area writers' `.max(1)` is a different routine);
    `sub_11410`/`sub_10080` seed a walker at ring 0 and
    `sub_114B0`/`sub_10130` yield each ring's tile offsets outwards;
    each tile's `mapEntityIndex` chain is walked and the FIRST hit
    admitted by (+66/+67 filter, `id != id`, AABB) wins. The port
    scanned all 999 slots by AABB alone, so it merged partners retail
    cannot see. Corpus proof, mc2l24 t=64509-11: the settled shore
    sphere slot 845 (55.973/228.99, +80 = 112 ⇒ ONE ring around tile
    56/229) does **not** absorb slot 795 when it steps to 54.98/227.98
    — the AABBs already overlap (Δ 255/260 < 112+153 = 265) but tile
    54/227 is two rings out — and absorbs it one tick later at
    55.23/228.23 (tile 55/228). Retail's mana ledger confirms the
    merge to the unit: 845 goes 141,653 → 143,966 (= +795's 2,313) at
    t=64511, then +78 as slot 828 vanishes at t=64512.
  - **DEFECT 2 — MC2's ABSORBED DONOR TAKES THE HARD FREE.**
    `sub_36D50` (EF:26919-96) is a ladder of owner-resolution arms and
    **every arm ends `return sub_57F20(a2x)`** — the hard free
    (Events.cpp:5209-39: tile unlink, recycle-stack swap-removal,
    `class = 0`, free-stack push). Nothing defers it to the disable
    sweep. The port kept MC1's hard free but soft-killed (0x400) on
    MC2 ("MC2's twin free is untraced"), so every MC2 merge left the
    donor in the pool for one more snapshot AND withheld its slot —
    which is both an extra-in-port AND a deeper free-stack pop that
    lands the tick's later spawns in slots retail never used.
  - **WHAT LANDED** (native + strict, one law, both games —
    `mc1::combat`): new `Gen::ball_merge_candidates` (combat.rs) is
    `sub_10A50`/`sub_11D10`'s ring walk, and the merge tail now calls
    `free_entity` (= `sub_57F20`) for MC2 as well. The explicit
    `class 10 / model 39` family test IS retail's +66/+67 filter —
    every ball ctor stamps `xtype/xsubtype` = (10,39) — so native
    balls, which carry no +66/+67, keep working. `MGC_NO_BALL_MERGE_FIX=1`
    restores both pre-dig halves for A/B.
  - **CONFORMANCE (windowed A/B, one frozen binary, env-toggled arms).**
    UNEXPLAINED field·missing·extra / gross missing / gross extra:
    l24 **64500+400 (the fountain)** 812·0·17 / 4 / **687** →
    809·0·17 / 15 / **574** ((10,39) extras **662 → 550**);
    l24 **61000+450 (the boss fight)** 7035·1·26 / 119 / **477** →
    6945·1·23 / 124 / **370** — the merge donors were feeding that
    window's slot pressure too, (9,9) extras **152 → 83**, (10,0)
    268 → 247; l24 30000+300 (control) 2250·0·0 / 3 / 18 → 17;
    **mc2l30 0+2000** 1765·13·41 / 65 / **149** → 1742·13·29 / 66 /
    **113** (explained pairs 1539 → 1549; (10,14) extras 37 → 21,
    (10,39) 26 → 15). Byte-identical in both arms: **mc2l0 0+2000**
    (523·17·17, 97/106) and **mc2l4 4300+300** (947·1·1, 6/20).
    **mc1l0 0+2000** is entity-set identical (75/52) and moves ONE
    unexplained field row (3390 → 3391) — the MC1 ring restriction is
    all but inert on that take. Fixture suites identical in both arms
    — mc1l0 68, mc1hwl0 29, mc2l0 41, mc2l4 24, mc2l30 24 all
    as-expected, and the mc2l24 17 (10 as-expected / 2 fixed / 5
    drifted) is the concurrent static-terrain-z dig's `field:2,2:z` /
    `field:2,3:z` drift, unchanged by this one.
  - **GOLDENS MOVED — ONE TEST, DELIBERATELY.**
    `mc2_cave_behaviors_and_goldens` checkpoints B-D (state hash and
    observable projection; the load checkpoint holds) — cave drips are
    mana spheres, so which ones coalesce and when their slots come
    back is exactly what this law changes. Re-pinned with the reason
    in place (mc2_cave.rs). Everything else green: 340 mgc-sim lib
    tests + every integration suite under `MGC_REQUIRE_GOLDENS=1`.
  - Pinned by
    `mc2_mana_merge_walks_the_tile_ring_and_hard_frees_the_donor`
    (world.rs — the corpus's own 845/795 geometry: an out-of-ring
    partner with overlapping AABBs is NOT absorbed, the same pair one
    tile closer IS, the donor ends `class == 0` and its slot is back
    on `free`). It FAILS under `MGC_NO_BALL_MERGE_FIX=1`, and the
    neutered arm reproduces the old port's exact wrong number
    (141,653 + 2,313 = 143,966 a tick early). Two existing merge tests
    (`mc2_mana_merge_takes_bigger_owner`,
    `mc2_mana_lock_survives_the_unclaimed_merge_arm_only`) now assert
    the donor by `class64`, not by the 0x400 soft-kill.
  - **WHAT IT IS NOT.** The 61k full-pool cluster is NOT one family
    with the fountain: its extras are **(10,0) 268 + (9,9) 152 of 477**
    (the boss's fire/blast churn, already captured by
    `mc2-fire-churn-m0` / `mc2-lightning-blast-churn`) against only 15
    (10,39). The merge fix helps it only through slot pressure.
  - **RESIDUALS / LEADS.** (a) The fountain window's remaining 550
    (10,39) extras are dominated by **summit merges the pristine
    terrain cannot host** — retail's early deaths cluster at
    (38-40, 213-216) z 2400..3300 on the doomsday mound, where the
    port's ball is 60 units off the ground and never grounds, so the
    merge branch is never entered. That is the `mc2l24-ball-terrain-roll`
    capture family and it belongs to the terrain-replay track; a
    60-pair join attributes 47 of 61 residual extras to slots retail
    never allocated (the downstream of those missed frees) and 11 to
    the one-snapshot decay-expiry linger. (b) 12 (10,39) missing
    remain: the port still merges a handful of SHORE-pile spheres a
    tick early — the order WITHIN one ring comes from retail's
    `bitmaps_E9980x` offset table, which the decompile does not
    carry, and raster order stands in. (c) Retail's decay expiry
    (`DisableEntityDrawing04` at life 0) leaves the record in the pool
    for exactly one snapshot with `byte[1] = 0x24` before the
    tick-top sweep frees it — the import carries that as 0x400 and the
    port's sweep matches, so the 11 `expiry(L0)` extras are pure
    slot-order downstream, not a law gap.

- **THE PRESS POSITION IS NOT THE CAST'S AIM, AND THE 0x40 BIT IS NOT
  A HAND — both landing-round cast-input items CLOSED ON THE
  DECOMPILE, 2026-08-04 (session 10, cast-input dig).** Two premises
  from the session-9 close (`mouse_press_pos recorded but UNUSED (= the
  aim the cast actually used)` and `cycle-ring hand bit 0x40`) were
  wrong about what retail does with either datum. Both lanes are now
  first-class in the format and measured; neither warrants the wiring
  that was proposed.
  - **③ `mouse_press_pos` = `x_WORD_E375C/E375E`, the ISR's
    cursor-at-press snapshot (EF:51478-97, written on the left, right
    AND middle press edges; nothing ever clears it). The poll copies it
    to `unk_18058C.x_DWORD_1805B8/1805BC` (EF:49664-65, and the three
    sibling control-mode arms 49703/49750/50423) and its ONLY consumer
    is `sub_1A7A0_fly_asistant` (PI:1988-2013) — the fly-assistant
    idle-recentre watchdog: 0x30 frames with the in-struct mouse
    unmoved and no pending action → `HandleButtonClick_191B0(39, 0)`.**
    The aim/attitude command is a different register: the input frame's
    `roll`/`pitch` (bytes 3/4) come from the LIVE cursor
    `x_DWORD_1805B0_mouse` ← `x_WORD_E3760` via
    `ComputeMousePlayerMovement_17060` (PI:643/1007/925 → PI:2100-41).
    And the cast itself takes NO aim at all: `sub_5F660` (EF:60874)
    is called `(caster, manifestation, hand-flag)` at all three sites,
    with the launch direction read off the caster entity's own pose —
    which `verify_mc2` already pins byte-exact from the recording
    (`carpet_pose_mc2`). **There is no aim to wire in.**
  - **③b The one place the datum could still bear on a cast — a
    sub-poll press oracle — was A/B'd and is STRICTLY WORSE.** The
    snapshot changes only on a press edge, so a change between records
    proves a press the latch lane may have missed. Measured against
    retail's own arm oracle (the equipped hand manifestation's
    `word_0x2E_46` 0 → nonzero) over the FULL mc2l0 take, 8,626 pairs,
    731 retail arms: **the landed latch law catches 728/731** (3 misses,
    t=3867/4144/4479; 201 armless edges = mana refusals, possess
    re-presses that raise `byte_0x3C_60` instead of the timer,
    cave-only refusals), **the press-position edge catches 480/731 with
    354 changes that arm nothing** — and it catches none of the latch
    law's 3 misses. End-to-end windows agree: mc2l0 0+2000 conforming
    **1,677 → 1,563** under the fold (unexplained field 523 → 513,
    conforming-or-explained 1,886 both ways — it trades 114 clean pairs
    for 10 roster-absorbed rows); mc2l4 3300+600 unexplained field
    **130 → 143** (extra 1 → 0, and two brand-new capture rules appear
    — `mc2-cast-timing-extra` 11 rows, `mc2-cast-timing-missing` 10 —
    i.e. manufactured casts); mc2l24 51500+600 headline unchanged
    (2,239 field / 1 missing / 140 extra both ways) but the fold adds
    270 rows of `mc2-fire-churn-m0` (3,481 → 3,751), the same
    manufactured-cast signature absorbed by a capture rule. LANDED OFF
    behind `MGC_PRESS_EDGE=1`
    (`verify_mc2::press_edge_mc2`, mirrored in `fixtures.rs` so a suite
    run under the toggle matches the triage run) purely to keep the
    result reproducible and to stand as the fallback if a recorder
    change ever costs us the latch.
  - **④ THE 0x40 BIT IS A THIRD CAST LANE, NOT A HAND SELECTOR.** The
    carpet's dispatch tail fires three lanes off `str_164->
    entityIndex_0x0` (EF:60851-62): `& 0x10` →
    `sub_5F660(carpet, SpellEnabled[SpellIndexLeft], 256)`, `& 0x20` →
    `…[SpellIndexRight], 512)`, and `& 0x40` →
    `…[spellIndex_D94FF[spellIndex_0x458_1112]], 256)`. So 0x40 casts
    the RING PANE's CATEGORY CURSOR through the LEFT hand-slot flag,
    consulting neither equipped hand — the shortcut that fires a ring
    spell without equipping it. `spellIndex_D94FF` (GameUI.cpp:59) is
    the identity over 0..25 (the three tail cells 0/3/0 are pane
    padding). It is raised at exactly one site, PI:880-84: the ring
    pane open (`MenuState_0x3DF` 5 or 8, the PI:806 branch), no equip
    pending (`byte_0x457_1111 == 0`, PI:836/842), no SHIFT (PI:856),
    and **BOTH press latches up** (`MouseButtonState & 1 && & 2` — bits
    0/1 are the ISR latches `x_WORD_180746`/`180744`, EF:49676-79). The
    dispatcher writes `spellIndex_0x458_1112 = byte1` first
    (EF:37626-27), so the cast reads the cursor the click just picked.
  - **④b UNREACHABLE ON THE WHOLE CORPUS — measured, not assumed.**
    Both press latches are never up in the same record in ANY MC2 take:
    mc2l0 0/8,626, mc2l4 0/17,711, mc2l24 0/69,220, mc2l30 0/9,337
    (both buttons are not even HELD together except 10 mc2l24 records,
    all outside the pane). The pane IS visited with the cursor moving
    (mc2l0 201 records, mc2l4 277, mc2l30 93; `hand_pending` reaching 1
    on all three), so the gate is live code this corpus simply never
    trips. LANDED as a DETECTOR, `verify_mc2::ring_cast_mc2`, default
    on / `MGC_NO_HAND_BIT=1` off: it reproduces retail's gate from the
    recorded state and prints the tick + spell if a take ever raises
    it, so the lane can never be silently dropped again. Verified a
    pure no-op — mc2l0 0+2000 is byte-identical with the toggle either
    way.
  - **PORT GAP, CONSCIOUSLY DOCUMENTED (not landed).** `mc2_cast_input`
    (cast.rs:1028) still models only the two hand bits, so a player who
    both-clicks inside the ring pane gets no cast. Wiring it needs a
    third `PlayerCommand` lane plus the ring CURSOR (the port carries
    `array_0x3B5` ring MEMBERSHIP, not `spellIndex_0x458_1112`), i.e. a
    real sim change with no corpus to verify it against — deferred
    until a take reaches the gate, at which point the detector above
    will say so. Related and separately open: mc2l0 0+2000 carries 2
    `player0.hand_left` rows (t=1750 retail `Some(2)` port `Some(0)`) —
    the ring pane's EQUIP path (`byte_0x457_1111` → PlayerAction 40 /
    31 / 32, PI:806-91) is unmodelled too.
  - **FORMAT.** `mgcr::Ext` gained `latch` and `press` (the `latch_b64`
    / `press_b64` raw registers the recorder has written since
    2026-07-30 and the decoder was dropping on the floor), and
    `RetailPlayerMc2` gained `menu_state` (+0x3DF), `hand_pending`
    (+998+1111) and `ring_cursor` (+998+1112) — the three fields the
    0x40 gate needs. Additive; `Ext` has no other consumer in the tree,
    so MC1 is untouched (mc1l0 0+2000 re-run unchanged: 525 conforming
    / 3,391 unexplained field / 48 missing / 27 extra, and the MC1 arm
    never reaches any of this dig's code).
  - **⚠ CORRECT THE FIELD MAP.** The recorder note and
    `tools/mc_dosbox_recorder.py:199-201/321-22/1452-54` both gloss
    `0xE375C` as "cursor-AT-PRESS (the cast's aim, game-snapped)". The
    provenance is right, the purpose is wrong — it is the
    fly-assistant watchdog's datum. `0xE3760` (live cursor) is the aim
    source.
  - Tests: `mc2_press_position_decodes_from_the_recorded_frame`,
    `mc2_press_move_can_carry_a_cast_the_latch_missed` (non-vacuous —
    the neutered `moved = false` arm must NOT manufacture a cast),
    `mc2_ring_cast_bit_needs_the_pane_and_both_latches` (nine
    coordinates, every neutered one refuses). Suites: the four
    non-mc2l24 MC2/MC1 manifests + mc1hwl0 ran as expected;
    conformance/mc2l24.json's 2 fixed + 5 drifted are the concurrent
    static-terrain-z / recycle-allocator sim work in the shared tree
    (the drifted atoms are `field:2,2:z` / `field:2,3:z` /
    `field:player.mana`), NOT this dig — nothing promoted.

- **THE STATIC GROUND PROBES — `.mgcr` has a terrain channel after
  all, hidden in the pool: every class-2 prop's `z` IS retail's own
  ground sample, and inverting the sampler ENDS
  `mc2l24-static-terrain-z`. LANDED 2026-08-04 (session 10,
  static-terrain-z dig).** Cashes session-9's NEW LEAD ①, and
  **refutes its pyramid hypothesis**: the l24 onset is not the (5,10)
  flatten, and the pyramid is not even in the pool at the onset tick.
  - **THE ONSET, TRIAGED FIRST (as instructed) AND IT IS FIRE.** The
    whole 375,572-row family is **8 entities**: the (2,2) dolmen at
    (128,112) and 7 of the 12 (2,3) props of the stone ring at
    (223-231, 233-241) — × the tail of a 69,207-pair take. The ring's
    props peel off one at a time, t=23411/25/26/27/27/35/37, as the
    player's METEOR barrage walks over them: `sub_32880` (EF:23834)
    seeds a ring of (10,0) fire children per tick out to ring 10
    (~6 tiles), and each fire's FIRST acting tick digs the ground once
    — `sub_30D50` EF:22741, `sub_572C0(fire, 0, 0, -(rand % 7), 1)`,
    the protected single-cell scorch (`sub_56F10` EF:39499). The
    dolmen's own −96 landed before t=3573 the same way. Every one of
    those fires despawned within ~30 ticks, so the dig history is
    **LOST BY CONSTRUCTION** — the pads entry's own "any crater whose
    caster despawned" class. There is nothing to replay.
  - **THE LAW THAT SAVES IT.** Three MC2 class-2 handlers end their
    tick with a bare ground read on an entity that NEVER MOVES:
    `AddStatue02_01_65040` (EF:62519, the (2,1) statue),
    `AddDolmen02_02_65080` (EF:62534, the (2,2) dolmen) and
    `sub_65110` (EF:62545, the (2,3) prop) all finish
    `position.z = getTerrainAlt_10C40(&position)` — our
    `Gen::ground_z` ≡ `sub_724C0` :81516. So each prop's recorded `z`
    is retail's interpolated ground height at a known position,
    sampled by retail's own sampler on the tick before the import: a
    **one-sample terrain channel per prop, which the recorder captured
    without knowing it**. `crates/mgc-sim/src/mc2/probes.rs` inverts
    that sampler (`mc2_static_ground_reconstruct`, :178) over the ≤4
    height cells the sample reads — one-cell solves by two binary
    searches on the branch-monotone corner, the uniform `32k` shift
    (`comp` is a form in height DIFFERENCES, so shifting every
    influential cell moves the sample exactly `32k`), then a bounded
    tilt search (:279). Hook: `world/conformance.rs:971-980`, LAST
    after the castle/building pads and the risers, so a prop standing
    on a replayed pad solves to a no-op. Toggle
    `MGC_NO_STATIC_TERRAIN_REPLAY=1`.
  - **THE BLAST-RADIUS RULE is the whole difference between this arm
    helping and hurting** — the point-sample descendant of the pad
    replay's off-footprint fence. A sample UNDER-determines its tile
    (2 corners on even parity, 3 on odd), so most exact assignments
    are wrong, and what decides is who else stands on those cells:
    `mc2_ground_reader_cost` (:120) scores every height cell by its
    live readers, an entity already at EXACTLY its own ground counting
    1,000 (a WITNESS that the cell is already right) and any other
    reader 1; the solve takes the smallest `(blast, edit)`. Measured
    on the dolmen, whose −96 can go −3/−3 across its corners or −6 on
    the far one alone: the naive split cost **38 new unexplained rows**
    at t=23300+300 (it broke the byte-exact (11,2) switch at (127,111),
    which shares the near corner) and **+981** at t=3560+440 (it moved
    a (5,3) body chain's head, and the chain amplified it across 12
    followers). The switch also PROVES the right answer independently:
    retail's digs only lower ground, so a witness reading the pristine
    `h(127,111)+h(128,112)` sum means neither moved, which forces the
    dolmen's whole −6 onto (129,113) — exactly what the reader cost
    picks with only the pool in hand.
  - **CONFORMANCE — windowed A/B, same binary, replay off/on.**
    mc2l24 t=60000+300: `mc2l24-static-terrain-z` **2400 → 0**, z rows
    3642 → 1242, every other rule and the entity sets byte-identical,
    UNEXPLAINED 2700 → 2700. t=40000+300: static-terrain-z **2400 →
    0**, z 4365 → 1965, nothing else moves. t=23300+300 (the onset):
    static-terrain-z **1512 → 12** (7 pairs — the ticks where the
    ground genuinely moves DURING the pair, which is now what the pair
    tests), `mc2-fire-churn-m0` 4103 → 4042, z 8086 → 6524,
    UNEXPLAINED 4257 → 4257. **THE ONE COST, reported honestly:**
    t=0+4000 conforming **1175 → 1212 (+37)**, conf+explained 3220 →
    3219, static-terrain-z **430 → 1**, fire-churn 3374 → 3315, z rows
    12992 → 12549 (pairs 2439 → 2224) — but UNEXPLAINED **5410 → 5538
    (+128)** and `mc2-walker-ground-z` 1225 → 1270. Row-level on
    t=3560+440: **519 rows fixed** (427 `(2,2) z`, 59 `(10,0) z`, 33
    (5,3)) against **206 new**, all of them the same (5,3) body chain
    reacting to the now-correct ground under the dolmen (48 z, 47 y, 37
    pitch, 35 x, 32 heading + 7 (5,9) z). Net −313 rows and +37
    conforming pairs. Regression probes **byte-identical** in both
    arms: mc2l0/mc2l4/mc2l30 t=0+2000, plus mc2l4 t=17000+300 and
    mc2l30 t=9000+300 (their class-2 props stand on unmodified ground
    for the whole take, so the arm is a no-op there — it is l24's dig).
    Runtime cost unmeasurable (t=60000+300: 60.7 s off, 62.6 s on).
  - **WHAT THIS IS NOT, stated plainly.** It is not a replay of the
    digs, and it recovers the RESULT at the sample points only — from
    the very field `mc2l24-static-terrain-z` grades. **That rule's row
    count is therefore not independent evidence for this arm, and it
    stops being a sensor for anything but the arm itself.** The
    independent evidence is the neighbours: 59 `(10,0) z` rows and 33
    (5,3) rows that the corrected plane FIXES, and the (11,2) witness
    that pins the dolmen's split. What the arm buys is not a number:
    without it every pair compares a STALE baseline — the port's prop
    stands on authored ground from the tick the dig landed to the end
    of the take, wrong by the same constant, testing nothing. With it
    the pair tests what a pair is for: whether the imported TICK moves
    the ground under the prop the way retail's did.
  - **SUITES.** mc2l0 41/41, mc2l4 24/24, mc2l30 24/24, mc1l0 68/68,
    mc1hwl0 29/29 all as expected, **0 regressions anywhere**. mc2l24
    17 ran, **0 regressions, 2 FIXED** (t=3588 Capture → conforming,
    t=10062 Open → conforming), 5 drifted — and **every drift is pure
    signature SHRINK**: `field:2,2:z` and `field:2,3:z` dropping out at
    t=13330/15288/51556/51751/64000, nothing added. Promotion owed,
    NOT applied. `cargo test -p mgc-sim` with `MGC_REQUIRE_GOLDENS=1`
    green, 339 lib tests, **no golden moved** (import-only).
  - Tests ×2, non-vacuous by construction:
    `static_ground_reconstruct_restores_the_sample_under_a_dug_prop`
    digs a 5×5 scorch bowl under a live prop, imports the prop's
    sample onto a pristine plane and demands the sample back, with the
    FENCE leg (every cell outside the sampled corners identical, plus a
    witness count so it cannot pass vacuously), a non-pristine
    assertion and an idempotence leg;
    `ground_probe_blast_radius_spares_a_witness_cell` runs the same
    deficit with and without a witness on the shared corner and demands
    the correction move to the OTHER corner — the no-witness leg is the
    non-vacuity.
  - **ROSTER RE-SCOPE PROPOSED (described, not applied — central
    re-scope owns it).** `mc2l24-static-terrain-z` should be re-noted
    as a post-reconstruct residual: whole-take it should fall from
    375,572 to the handful of pairs where the ground moves within the
    tick. `mc2-walker-ground-z` is **untouched** by this arm (its
    234k rows are creatures on scorched ground all over l24, not on
    the 8 probes) and gains ~45 rows in the early window.
  - **NEW LEADS.** ① `dig_scorch` (mc1/combat.rs:4214) digs the
    UNROUNDED cell `(x >> 8, y >> 8)`, but MC2's `sub_572C0`
    (EF:39712-39723) walks rings 0..0 around the ROUNDED cell
    `((x + 128) >> 8)` — the MC2 fire scorches a cell up to one off in
    each axis. It is shared MC1 code, so the fix needs an MC2 seam and
    MC1 evidence of its own; conformance-invisible per-pair (the plane
    is pristine every pair) but it deforms a NATIVE 69k-tick run's map.
    ② The same probe-inversion technique is the only handle on
    `mc2-walker-ground-z` and would need MOVING probes (the (5,3)
    heads are `z == ground_z` exactly, per `mc2-flyer-drift-m3`); a
    least-squares plane fit over a whole pair's ground-clamped
    population is the shape, and it is a much bigger and much more
    speculative arm than this one. ③ The l24 dolmen's ground OSCILLATES
    1184/1152 every tick from t≈13333 — something re-digs and re-raises
    (128,112)/(129,113) on alternate ticks for thousands of ticks;
    untriaged, and the reconstruct tracks it correctly either way.

- **THE MC2 ALLOCATOR'S FULL-POOL ARM: the port had NO recycle-victim
  path at all, and now has retail's — but the measurement that came
  with it FALSIFIES the lead that asked for it. LANDED 2026-08-04
  (session 10, recycle-victim dig).** Closes session-9's NEW LEAD (a).
  Law gap closed; **zero conformance movement, and that is the
  finding**: on the whole corpus retail never once takes this arm.
  - **THE RETAIL LAW (three routines, one stack).** `NewEvent_4A050`
    (Events.cpp:561-608) pops the FREE stack first (:563-79) and only
    with `dword_0x35 < 0` falls through to the recycle stack
    (:581-605) — the opposite priority of MC1. The victim arm is a
    **bare seizure, not a death**: `SetMapEntity_57E50` (tile unlink),
    `class = 0`, then the same 168-byte memset + `NewEvent` defaults
    the free arm runs (:589-604). No damage, no kill credit, no
    corpse, no parent notify, and the slot **never visits the free
    stack** — it goes straight to the new occupant. The ranking is
    `sub_49F90` (Level.cpp:1271-1302): reap every `byte[1] & 4`
    record through `sub_57F20` (:1276-80), then reset BOTH tops
    (:1281-83) and rebuild them in ONE descending 999→1 scan (:1284-
    1301) — live + `byte[2] & 2` (our `flags & 0x2_0000`) pushes to
    the recycle stack, class-0 pushes to the free stack. Descending
    pushes ⇒ **the stack top is the LOWEST-numbered victim**. The
    third routine is the removal: `sub_57F20` (Events.cpp:5209-39)
    pulls a dying sacrificable entity OUT of the stack by linear
    search + **swap-with-top** (:5220-34, order-destroying below the
    hole) before pushing its slot free — without it the allocator
    would hand one slot out twice. Refresh cadence: EF:39396 (level
    generate), EF:60101, EF:61275-79 (literally "free stack empty ⇒
    `sub_49F90` ⇒ retry"), and every save/load path *empties* it
    (`sub_49F90(); dword_0x11e6 = -1;` — Level.cpp:304-305/:423-424,
    EF:38829/:38874/:39467).
  - **THE MEASUREMENT THAT KILLS THE LEAD** (probe over all 104,898
    MC2 corpus snapshots). The 74 mc2l24 full-pool snapshots are real
    — and **every one of them has an EMPTY recycle stack**, so retail
    drops those spawns exactly like the pre-dig port did. The two
    conditions never meet anywhere: mc2l24 has 371 snapshots with a
    victim list and its **free stack never drops below 85** while one
    exists; mc2l4 has 2,851 and never below **696**; mc2l0/l30 have
    no victim list at all and never fill. Adjacent-tick recycle
    transitions on l24: 237 unchanged, 120 reordered (the `sub_57F20`
    swap-removal signature — e.g. mc2l4's stable tail
    `[146, 343, 368, 332, 249, 346]`, NOT ascending, so retail's own
    list is post-rebuild mutated), 14 clean shrinks (all to zero =
    the load/save `dword_0x11e6 = -1` reset), 14 grows. **The port's
    new fallback fires 0 times in every window measured.**
  - **WHAT LANDED** (native + strict, one law): `Mc2Recycle`
    (features.rs:940 — `stack` bottom-up, LAST pops first, hash-quiet
    while empty so no golden can move) on `Gen::mc2_recycle`
    (:604); `new_event` (:1228) is now
    `free.pop().or_else(mc2_recycle_pop)` — the shared body's
    existing `unlink` + `Ent::default()` already IS retail's victim
    teardown, so the fallback only chooses the slot;
    `mc2_recycle_pop` (:1294) skips cells that are no longer live
    victims and, natively, re-ranks once via `mc2_rebuild_recycle`
    (:1331 = `sub_49F90`'s victim half, verbatim); `free_entity`
    (:1414) gained `sub_57F20`'s swap-with-top removal. Native MC2
    arms the list in `World::new_full` (world.rs:1228); the strict
    import carries the RECORDED stack with `refill` clear
    (conformance.rs:685-700) so replay sacrifices retail's victims in
    retail's order and starves exactly where retail's list ran out.
    The refresh CADENCE is the one native gap — logged in
    docs/DEVIATIONS.md (`Gen::mc2_rebuild_recycle`). The stack is
    deliberately NOT saved (features.rs `snap_write`): retail's own
    load empties it.
  - **CONFORMANCE (windowed A/B, one frozen binary, env-toggled arms
    via `MGC_NO_RECYCLE_VICTIM=1`).** Every window is **byte-identical
    between arms**, as the measurement predicts — l24 61000+450 (the
    36-snapshot full-pool cluster; UNEXPLAINED 7035 field·1
    missing·26 extra, 119 missing-in-port / 477 extra-in-port), l24
    62950+300 (the 27-snapshot cluster; 4852·2·45, 40/293), l4
    4300+300 (the recycle window, and the import DOES carry 6 victims
    there from t≈4400; 947·1·1, 6/20). Cross-take regression probes
    mc2l0 and mc2l30 at 0+2000: byte-identical, and MC1/HW never
    touch the arm. Fixture suites identical in both arms — mc1l0 68,
    mc1hwl0 29, mc2l0 41, mc2l4 24, mc2l30 24 all as-expected; the
    mc2l24 17 (10 as-expected / 2 fixed / 5 drifted) belongs to the
    concurrent static-terrain-z dig (its rows are exactly the
    `field:2,2:z` / `field:2,3:z` lanes), not to this one. 337
    mgc-sim lib tests + every integration suite green under
    `MGC_REQUIRE_GOLDENS=1`; **no golden moved**.
  - **WHAT THE FULL-POOL PAIRS ACTUALLY SHOW.** The new telemetry
    (`World::take_recycle_seized` world.rs:3216, printed per pair by
    verify_mc2.rs:182 beside `take_pool_exhausted`) reports up to
    **238 dropped spawns in a single pair** around t=61117 — with
    **0 victims available**, i.e. retail was equally starved. And the
    window's entity sets run 477 extra-in-port against 119 missing:
    at a full pool the port is **over**-spawning relative to retail,
    never under. Pool starvation is not a missing-entity source on
    this corpus.
  - **WHAT IT IS NOT.** Not the l24 slot-desync residue (that lives
    in the early take, t=3569/13330). Not the (10,39) fountain
    extras. And **not** "the import filters live victims out so the
    port fails to spawn where retail sacrifices" — the import filter
    is correct and the premise's second half never happens.
  - Pinned by `mc2_full_pool_sacrifices_the_recorded_recycle_victims_in_order`
    (conformance.rs — a full imported pool sacrifices 300, 500, 700
    in the recorded order, the seized slot skips the free stack, the
    4th alloc returns None, and a normally-dying victim leaves the
    stack by swap-with-top; FAILS under `MGC_NO_RECYCLE_VICTIM=1`,
    which is the pre-dig port) and
    `mc2_full_pool_sacrifices_the_lowest_ranked_victim_natively`
    (world.rs — victims 640/210/480 are seized 210→480→640, and the
    in-line neutered arm returns None).
  - **INSTRUMENTS LEFT BEHIND.** `dump-state` finally prints the MC2
    free/recycle stack tails (main.rs:275 — the MC1 arm has printed
    them since session 4; the ledger's tooling note at the top of
    this file claimed both), and every MC2 verify pair now reports
    `N recycle victim(s), M spawn(s) dropped` when either is nonzero.
  - **NEW LEADS.** (a) The full-pool clusters (l24 t≈58707, 60188,
    61004-61434, 62963-63231) are a **479-entity over-spawn** window
    — the port fills the pool with entities retail does not have.
    That, not starvation, is what to dig there. (b) If a future take
    ever records a full pool WITH a live victim list, this arm
    becomes measurable for the first time; the seizure telemetry is
    already wired to say so.

- **THE MC2 RIVAL AI-LANE DECODE: the wizard-extension brain half is
  mapped and imported — and the (3,1) remainder turns out NOT to be
  decisions (2026-08-04).**
  - **The lane map.** The rival's brain lives in `type_str_164`,
    EMBEDDED in the per-player block at **+998** (`m2::PP_FLIGHT`;
    remc2 `dword_0x3E6_2BE4_12228`, and `str_611` is that struct's
    +611 — which is why the already-decoded book lanes are at
    block +0x649…+0x81D). Every remc2 field name carries its own
    offset twice (`word_0x159_345` = 0x159 = 345), all block-relative
    to +998. Decoded (`crates/mgc-formats/src/mgcr.rs`
    `RetailPlayerMc2`, verified against mc2l4):
    | +998+ | lane | writer/reader |
    |---|---|---|
    | 449 | `byte_0x1C1_449` **AI state** | dispatch `sub_12910` EF:5252; selector writes it EF:5517-70 |
    | 418 | `word_0x1A2_418` **burst** (counts shots up, goes NEGATIVE for the lockout) | gate EF:5947, walk-back EF:5358 |
    | 420 | `word_0x1A4_420` poverty latch | EF:7191-7205 |
    | 516+8c / 518+8c | **hate / war** per colour — 8-BYTE records based at 516, i.e. retail's `array_0x1FC_508[4c+4]`/`[4c+5]`, NOT a flat array at 508 | decay EF:5377-92, readers EF:6201/6257/7363, respawn truce EF:43839/43850 |
    | 871 (26×u16) | **AI recast cooldowns** `str_611.array_0x367_871x` — a SECOND per-spell array right after the manifestation table at 819 | EF:5364-70 |
    | 578/580/582/586 | aggression / perception / reflexes / Life scalar | think cadence EF:5460 |
    | 1116/1117 | combat-weave dir / phase | EF:7469 etc. |
    | 1118/1119 | water-steer FSM / exit | |
    | 58 | `CastleEntityIndex_0x3A_58` — see the DECODE BUG below | 21 brain sites |
    Target and site do NOT live here: they ride the wizard ENTITY
    (`word_0x96_150` + signature `word_0x98_152`, `axis_0x9A_154x`;
    EF:6114-15), already imported by `import_ent_mc2`.
  - **Landed** (import-only, no native law touched): `mgcr.rs`
    decodes the lanes; `World::reanchor_mc2_rival_ai`
    (`mc2/rivals.rs`) + the call in `retail_import_mc2`
    (`engine/world/conformance.rs`) seat state (via the new
    `Mc2AiState::from_retail`), target, RAW signature, site, burst,
    poverty, cooldowns, hate/war, weave, avoid, personality, Life —
    plus the rival SPELLBOOK (`spell_ent`/XP/levels/sel/ring), which
    was the frozen-husk bug's twin: `book.ent[s]` kept the fresh
    world's manifestation slots and after import pointed at whatever
    entity the closure had put there. The signature is imported RAW,
    not recomputed (MC1's re-anchor recomputes): it IS retail's
    staleness detector, so recomputing silently revives a dropped
    target. Death rewrites `SpellsEnabled[s]` to the boolean marker
    **1** (EF:60147) — imported verbatim, quirk included, because
    retail's own dead-window reads index the pool with that 1.
  - **Measured (mc2l4, the ONLY take in the corpus with rivals —
    see below).** A/B on identical code via a temporary import gate:
    window 8000-9000 `mc2-rival-ai-lanes` 1190 → 1057 rows / 733 →
    692 pairs, and inside it speed 85 → 44, heading 52 → 29, rand
    61 → 39; window 0-1500 3914 → 3896; window 5400-6600 1282 →
    1269 (x/y/heading were already clean there). Full take (with the
    round's other landings): 14,823 → **14,842 explained pairs**,
    unexplained field **11,184 → 10,247**, extra 72 → 64, missing
    87 → 90, rng 6 (flat). mc1l0 full take **501 conforming, rng 0 —
    unregressed**; mc2l0 full take 2,232 conf / 3,032 field / rng 2.
  - **THE HEADLINE NEGATIVE RESULT: the (3,1) bucket is not made of
    decisions.** Per-field census of the surviving (3,1) rows:
    window 5400-6600 → z 1019, mana 179, mana_max 26, pitch 24,
    everything else ≤ 4; window 8000-9000 → z 659, mana 90, life 50,
    speed 44, x/y 86, rand 39, heading 29, action 25. So **~80% is
    terrain-closure z** (the rival hovers over its own castle and the
    altitude clamp `EF:5482-86` reads the port's pristine+replayed
    heights: the l4 t=0 rival sits 2304 units off — the castle-pad
    stamp — while the 5400 window is a flat ±3 offset) and **~14% is
    a missing ENTITY MIRROR** (below). The decision lanes were worth
    ~11% of the bucket, and only in the combat window.
  - **TOP PROPOSAL (native, NOT applied — this dig was import-only):
    the MC2 rival's mana/regen never reaches its entity.** Retail
    keeps the wizard's purse ON THE ENTITY (`a1x->mana_0x90_144 +=
    a1x->manaRegen_0x88_136`, EF:5423, and every cast debits it
    there); the port keeps it in `Mc2Rival::{mana,mana_max}` and
    NOTHING writes `ent.f140`/`f136` back. The obs projects the
    ENTITY, so every rival's `mana` reads as "the imported value,
    frozen" — retail +1000/tick at its castle vs port +0, and a
    cast that drops retail 16840 → 1000 leaves the port at 16840.
    One mirror at the tail of `mc2_rival_alive` closes it; it WILL
    move goldens (an `Ent` field write), so it needs the re-pin
    ritual. Same shape, smaller: the `life` family (retail's rival
    takes damage at t≈8229 that the port's at-castle grace discards
    — check `rival_castle` resolution first, see the decode bug).
  - **DECODE BUG FOUND (documented, deliberately NOT moved):
    `m2::PP_CASTLE = 1080` is the WRONG LANE.** The real
    `CastleEntityIndex_0x3A_58` is at block **+1056** (= `PP_FLIGHT`
    + 58): on mc2l4 +1056 tracks the live (3,2) slots (297/304, and
    it follows raze/rebuild — p2 goes 304 → 318 → 0), while +1080 is
    dead 0 for every player of every sampled tick of every take. The
    RECORDER captures the same wrong offset, so the stored
    `obs.players[].castle` is a constant 0 AND `verify_mc2` PINS the
    port's projection from it — the compare is vacuous, a blind spot
    rather than a false positive. Moving `PP_CASTLE` would break
    `check-decode` against the whole corpus (RECORDING.md lockstep),
    so the truth is exposed as `PP_CASTLE_TRUE` /
    `RetailPlayerMc2::castle_ent` and the fix is owed to the recorder
    + a re-record. Retail's AI READS that stored index at 21 sites;
    the port re-derives it by owner scan (`rival_castle`).
  - **CORPUS FACT that re-attributes two banked leads: mc2l0, mc2l24
    and mc2l30 have NO rival wizards at all.** `player_count` is 1
    for every tick of all three takes (l0 8,627 / l24 69,221 / l30
    full scans) and none has a class-3 model-1 entity at t=0; only
    mc2l4 has AI players (colours 1-2, `is_ai` set). The importer
    eliminates every port-side rival record when the slot is absent
    (`reanchor_mc2_rival(ri, 0, …)`), and the pool import overwrites
    all 1000 slots, so a port-only rival cannot act. Therefore:
    (a) the "l24 late window: retail casts ZERO possession bolts,
    port casts 19" divergence is **NOT rival-attributable** — look
    at the human/class-9 path; (b) the "(3,1) frozen husk on mc2l0"
    label is a mis-file (the family is mc2l4, as the kinematics-round
    entry says); (c) the roster rule's `takes: [mc2l4, mc2l30]`
    deserves a re-check — l30 has no (3,1) at t=0.
  - **ROSTER PROPOSAL (described, not applied):**
    `mc2-rival-ai-lanes` is class/model-scoped, so it absorbs every
    (3,1) row and is now badly named for what it holds. Split it:
    keep an `mc2-rival-terrain-z` (the z majority, same family as
    `mc2-castle-pad-z`/`mc2-archer-ground-z`), add
    `mc2-rival-entity-mana-mirror` for the mana/mana_max/life rows,
    and let `mc2-rival-ai-lanes` shrink to the residual
    heading/speed/rand/action rows — or retire it if the mirror
    lands and the residue proves to be terrain only.
  - **Tests:** `mgcr::tests::mc2_wizard_ext_ai_lanes_decode_off_str_164`
    (synthetic image; the non-vacuity leg proves the hate array is
    NOT flat at +508 and that the per-player stride is respected) and
    `engine::world::tests::mc2_rival_ai_reanchor_replays_the_recorded_target`
    (two mana balls on opposite bearings; the re-anchored rival faces
    the RECORDED one and its imported recast cooldown ticks 10 → 9,
    while the un-anchored twin re-runs the cascade and aims
    elsewhere). `MGC_REQUIRE_GOLDENS=1 cargo test -p mgc-sim` green,
    no goldens moved.
  - **Open after this dig:** the ±3 z offsets (terrain closure) and
    the pitch family (`got(t) == want(t−1)` on the aim-pitch lane,
    ~25 rows/window — the rival skips retail's aim update on those
    ticks, EF:6803). Both are small and neither is a decision lane.
  - **Decompile lead, NOT resolved:** the hate decay reads
    `array_0x1FC_508[4*i]` (= 508+8i) as the addend while writing
    `[4*i+4]` (= 516+8i) — i.e. `hate[c] = agg + 1 + hate[c-1]` as
    literally transcribed. The port implements the sane
    `hate[c] += agg + 1`. Either remc2 mis-indexed one operand or
    retail has an off-by-one-record bug; the import now dominates it
    per pair, so it is conformance-quiet — but it must be settled
    against the raw binary before anyone trusts native hate pacing.

- **THE HUMAN'S MC2 CAST IS AN ENTITY-WALK EVENT, NOT A PRE-PASS —
  and that, not any "(10,14) re-arm", was the slot-order corruptor
  behind the whole mc2l0 `(10,14)` extra family (2026-08-04).**
  - **THE PRIOR HYPOTHESIS IS REFUTED, ON THE DATA.** The cast-phase
    entry below read mc2l0 t=28's specimen as "retail's dying (10,14)
    at slot 206 (`life -2`) RE-ARMS IN PLACE". `dump-state` says
    otherwise: retail's slot 206 at t=28 is at (78.56, 220.74, 7222)
    and at t=29 it is at (77.63, 218.38, 4866) with a fresh
    `life 31/32` — a DIFFERENT particle in a RECYCLED slot, not a
    re-arm. The retail handlers agree: `sub_32160`/`sub_322A0`
    (EF:23572/:23613, the (10,13)/(10,14) particle ticks) have ONE
    death path, `if (life-- < 0) { DisableEntityDrawing04_57F10;
    return; }` — byte[1] |= 4 and nothing else. **There is no re-arm
    arm anywhere in the class-10 smoke family**; the tick-top reap
    (`UpdateEntities_57730` EF:39948-56 → `sub_57F20`) frees it, and
    the port already matched that exactly.
  - **THE REAL LAW (EF-cited).** `sub_57F20` (Events.cpp:5209) pushes
    the freed slot onto the LIFO free stack (`pointers_0x246
    [++dword_0x35]`) and `NewEvent_4A050` (Events.cpp:561) pops it, so
    **who allocates FIRST inside the tick decides who gets the
    recycled slot**. Retail arms and fires the human's spells from the
    human's OWN class-3 dispatch, mid-walk:
    `AddPlayer03_00_5E010` (EF:59954) → **`sub_5F380` (EF:60748),
    whose tail IS the cast gate** — the three `sub_5F660` calls at
    **EF:60850/:60855/:60859** (left hand / right hand / the cycle-ring
    hand) — then `sub_5EFA0` (EF:59989), then the mover `sub_5D530`
    (EF:59994). The port ran `mc2_cast_input` + `mc2_cast_tick` as a
    PRE-PASS, ahead of the whole ascending walk, i.e. as if the human
    sat at slot 0.
  - **THE SPECIMEN, SOLVED TO THE SLOT.** mc2l0 t=28→29. Disabled at
    t=28: slots **122-129 and 206** (all `flags 0x20404`, `life -2`).
    Tick-top reap pushes them ASCENDING → stack top 206, then
    129…122. The nine chimney emitters (10,60) sit at slots
    **113-121** and each spawns one (10,14): emitter 113→**206**,
    114→129, 115→128, 116→127, 117→126, 118→125, 119→124, 120→123,
    121→**122** — every one confirmed by matching the particle's x/y
    to its emitter's tile. The human is slot **152**, so his bolt
    allocates AFTER all nine and lands on **453**, off the deep
    stack. The port's pre-pass cast took 206 for the bolt and shoved
    the ninth puff out to 453 — one slot of rotation across the whole
    ring, which is exactly the `(10,14) 0 / 125` shape.
  - **CHANGES.** `crates/mgc-sim/src/engine/world.rs`: new
    `World::mc2_player_cast_pass` (:1685) = `mc2_cast_input` +
    `mc2_cast_tick`, the human's own body; `tick()`'s MC2 spell block
    (:1902) now calls it only when `mc2_carpet_slot == 0`, and the
    ascending walk's carpet-slot hook (:2016) calls it at the human's
    slot, BEFORE `mc2_cave_carpet_tail` (retail's own order:
    sub_5F380 tail → sub_5EFA0 → sub_5D530). Pane selection
    (`mc2_select_spell`) stays pre-walk — it is PlayerInput's, not the
    entity's.
  - **NATIVE IMPACT: NONE, BY CONSTRUCTION.** `mc2_carpet_slot` is
    written only by `retail_import_mc2` (conformance.rs:556); native
    MC2 has no pooled human and leaves it 0, so native takes the
    unchanged pre-walk path (a human at slot 0). No `DEVIATIONS.md`
    entry covered the pre-pass placement — it was an unrecorded
    accident of harness ordering, not a ruled deviation.
    **Goldens: NONE moved** (`MGC_REQUIRE_GOLDENS=1 cargo test
    -p mgc-sim` = 333 lib + all integration green, 0 re-pins).
  - **A/B (one binary, arm neutered/restored in place, same tree,
    windowed 0+4000).** **mc2l0**: raw conforming **2,032 → 2,232**
    (+200); unexpl field 2,026 → **1,889**; rng 3 → **2**;
    `mc2-cast-timing-fields` **5,215 rows / 704 pairs → 1,301 / 457**;
    entity sets 101/140 → 101/**138**. **mc2l4**: unexpl field
    4,568 → **4,354**; extra 45 → **43**; entity sets 69/110 →
    70/**90**; cast-timing-fields 4,553/1,007 → **3,483/935**; rng 6
    (unmoved). **mc2l24**: conforming 1,163 → **1,175**; unexpl field
    5,454 → **5,410**; entity sets 538/813 → 538/**802**;
    cast-timing-fields 2,565/470 → **1,985/448**; `cast-timing-extra`
    99 → 111 (the one counter-move). **mc1l0 0+4000 UNCHANGED** — 501
    conforming / 6,524 unexpl field / rng 0 (MC1 cannot reach the
    changed code).
  - **FULL TAKE mc2l0** (8,626 pairs, vs the SESSION-8 post-cast-phase
    baseline): **2,232 conforming** (was 2,032) / **3,032** unexpl
    field (was 3,236) / 33 missing (was 32) / **42** extra (was 43) /
    **rng 2** (was 3).
  - **SUITES: 0 REGRESSIONS ANYWHERE.** mc2l0 41 fixtures — **1 FIXED
    (t=28, the specimen itself)**, 0 drifted. mc2l4 24 — 2 drifted,
    both SHRINKING: t=3449 loses its entire 14-row `field:9,1:*` slot
    substitution (→ `field:15,19:z field:3,3:z`), t=4233 loses
    `field:3,1:z`. mc2l24 17 — 1 drifted (t=3559 `extra:10,0` →
    `extra:9,0`). mc2l30 / mc1l0 / mc1hwl0 clean. **`--promote` NOT
    run** (conformance/*.json was re-frozen today; the promotion is
    owed to a central pass).
  - Pinned by `mc2_human_cast_pops_the_free_stack_after_lower_slots`
    (world.rs tests): a (10,60) chimney one slot BELOW the carpet slot
    must take the free stack's top and the bolt the next. Neutering
    the placement (cast back to pre-walk) flips both and the test
    fails — verified.
  - **NEW LEADS.** (a) Retail ticks each spell MANIFESTATION at its
    own pool slot (`sub_693F0` EF:55831, the class-15 3M action), not
    in book order from the wizard; the port still runs all 26 from
    `mc2_cast_tick`. Immaterial on today's corpus (manifestations are
    contiguous right above the human: mc2l0 153-154, mc2l4 266-291,
    mc2l24 117-135), but a jar picked up mid-level adopts the TOKEN's
    slot, which can be anywhere — worth a per-slot dispatch when a
    take shows a low-slot manifestation. (b) The surviving
    `mc2-cast-timing-fields` residue on mc2l0 is now dominated by
    slot substitutions where retail holds a (10,12) claim pulse and
    the port a (9,1) bolt (t=348 slot 165) — a possession-lane
    question, adjacent to the OPEN `mc2-claim-census-manifest`.
    (c) `sub_5F380`'s tail also fires the CYCLE-RING hand
    (EF:60859, `entityIndex_0x0 & 0x40` → `spellIndex_D94FF
    [spellIndex_0x458_1112]`); the port's `mc2_cast_input` models only
    the two hand bits (0x10/0x20).

- **SESSION-8 CLOSE (2026-08-04, authoritative full takes on the
  final tree; suites 6/6 green, 203/203 fixtures as-expected after
  a reviewed `--promote` pass — 10 promoted incl. mc2l0 t=737, the
  3 attributed mc2l0 regressions re-statused open with notes;
  NOTHING COMMITTED — player handles git).** Seven digs landed this
  session: (10,12) claim pulse + claim-probe gate, riser-endcap
  terrain replay, importer ghost double-push + (5,10) summon
  stride, tier-0 (9,1) possession bolt + fov launch lift (fool
  OPEN-5 closed), conformance cast-EDGE harness fix, `.mgcr`
  pool-base validated recovery (free+recycle stacks), BUILD00 pad
  replays (castle mound + village terrace); plus the mc1:49 O
  ruling (retail-confirmed). Corpus close:
  - **mc2l24** 67,391 grade: **1,030 conforming** (was 4) /
    19,266 conf-or-explained / 459,142 unexpl field (was 733,635)
    / 409 missing (was 980) / 6,034 extra / rng 10 (was 12).
  - **mc2l0** 8,626: **1,771 conforming** (was 479) / 7,414
    conf-or-explained / 3,206 unexpl field / 34 missing / 40
    extra / **rng 3**.
  - **mc2l4** 17,711: 0 raw-conf / **14,811 fully explained
    (83.6%)** / 11,283 unexpl field / 87 missing / 81 extra /
    rng 10. **mc2l30** 9,337: 13 conf / 7,224 conf-or-explained /
    7,322 unexpl field / 78 missing / 97 extra / rng 19.
  - **mc1l0** 5,873: **501 conforming** (was 450) / 4,214
    conf-or-explained / 10,632 unexpl field / **rng 0**.
    **mc1hwl0** 39,199 grade: 49 conf / 2.17M unexpl field
    (terrain-channel domination unchanged).
  **SAME-DAY ADDENDUM — CAST-PHASE LAW LANDED (player-ruled "build
  in the one correct mapping"; see its own Resolved entry): the
  pair takes its END record's command; the ISR press LATCH bit
  resolves frame ownership (`aligned = (held && !latch) ||
  latch(r−1)`; delta 0 on 4,814/4,815 casts corpus-wide; the old
  "+1 early" reading was nearest-arm ALIASING — the port was 3-4
  pairs LATE). Harness-only (verify_mc2 + fixtures; MC1 has no
  latch register, verify.rs untouched). Post-law close: mc2l0
  **2,032 conf** / rng 3 · mc2l24 **1,163 conf** / 405 missing /
  rng 8 · mc2l4 rng **6**, extra 72 · mc1l0 UNCHANGED to the
  digit. Suites re-promoted 203/203, 0 regressions (mc2l0 t=32/
  291 — this morning's re-statused pair — now genuinely
  conforming).**

- **THE BUILD00 PAD REPLAYS — the castle mound and the village
  terrace are pure functions of imported state, and replaying them at
  import ENDS the `mc2-guard-terrain` family. LANDED 2026-08-04
  (session 9, terrain-replay dig).** The riser entry's NEW LEAD ①
  ("point the same technique at the other terraform roots") cashes:
  MC2's two BUILD00 stampers both end their progressive lerp on a tick
  that divides by 1, so their terminal map is ABSOLUTE and depends only
  on the stamper's cell, its BUILD00 row and its build datum — all
  three of which the `.mgcr` already carries.
  - **THE CASTLE LAW.** `sub_5FBD0` (EF:61188) spawns the (10,42)
    painter AT the castle's `axis_0x9A_154` and copies the castle's
    `dword_0x10_16` (the LEVEL) into its `byte_0x46_70` (EF:61189).
    The ctor `sub_4AA40` fills `axis_0x9A_154.z = 32 *
    sub_48E60(...)` — the perimeter MINIMUM ground over the row-1
    footprint (EF:33399) — and it is the ONLY (3,2) ctor, so every
    castle carries its datum there. The painter reads it back off its
    own position (`v40 = position.z >> 5`, EF:27775) and writes
    `height += (pad + datum − height) / countdown` per cell of BUILD00
    rows `1..=level` (EF:27846-56); `countdown == 1` on the last rise
    tick makes the terminal height exactly `pad + datum`. Every
    level-up spawns a fresh painter over the same cumulative
    footprint, so the LAST one reproduces the whole history. Import
    homes: anchor `x`/`y`, datum `site_z` (@0x9A.z → `dest_z`), level
    `f26` (@0x10). **The tell**: mc2l4 slot 330 is the human's
    water-sited castle at (154,34) — `dest_z` 0 (perimeter min 0, it
    was built on a shore), level 5, retail z 4160; the port's castle
    ground-snapped to **0** because the pristine plane reads height 0
    there and nothing had ever stamped the mound.
  - **THE VILLAGE LAW.** `ApplyTerrainModification_37240` (EF:27181)
    is the same shape with `life` as divisor (EF:27341-44): 30 frames,
    the last dividing by 1. On the final frame it parks the building
    (action 51 → 52), stamps `axis_0x9A_154 = position` and only THEN
    overwrites `position.z` with the ground — so a parked building's
    build datum survives in `site_z` and its BUILD00 row in
    `byte_0x46_70` (`f71`). Both imported.
  - **THE FIX.** `crates/mgc-sim/src/mc2/pads.rs` — conformance-import
    only, native untouched: `Gen::mc2_castle_pad_reconstruct` (the
    terminal form of the painter, rows `1..=level` overlaid later-row-
    wins, first-tick flat-nibble promotion, `countdown 2`/`countdown
    1`/settle bit3↔bit7 dance, last-tick texture pass) and
    `Gen::mc2_building_pad_reconstruct` (drives the REAL
    `mc2_building_tick` for the frames already run — `max_life` when
    parked, `max_life − act_life` mid-construction — with the
    footprint kill suppressed and the entity row restored afterwards).
    `retail_import_mc2` runs castles, then buildings, then the risers
    (`world/conformance.rs`; a castle build purges the buildings inside
    its footprint, so a surviving building never overlaps a castle pad,
    and l24's risers sit in compounds no pad reaches). `MGC_NO_PAD_
    REPLAY=1|all|castle|building` is the A/B toggle.
  - **THE OFF-FOOTPRINT FENCE is the whole difference between this
    helping and hurting.** The building's final frame runs two
    pad-edge smoothing rings (`sub_48A20` EF:32348) anchored on the
    top-left corner MINUS the half extents, so they reach a full
    footprint-width PAST the pad; over ground the baseline plane had
    already settled that second 3×3 average is pure damage. Measured:
    without the fence, ONE 1-unit re-smooth at (71,166) — the top band
    of the 23×11 building at (82,180) — cost **all 291 conforming
    pairs** of mc2l0 t=700+400. Snapshotting the heights outside the
    footprint and putting them back turns the same window into **291 →
    377 conforming**. Inside the footprint the replay is idempotent by
    construction (the lerp lands the absolute target before the rings
    re-smooth). A majority-vote "is the terrace already there?" gate
    was tried and REJECTED — it gives back the 377 (291) without
    recovering the early-window cost.
  - **CONFORMANCE — windowed A/B, same binary, replay off/on.**
    mc2l4 t=4000+300: `mc2-guard-terrain` **2220 → 0**,
    `mc2-castle-pad-z` **900 → 0**, `mc2-terraform-houses` **300 → 0**,
    `mc2-balloon-z` **300 → 0**; raw z **3965 → 870**, rand 257 → 32,
    action 227 → 2, x 129 → 42, y 106 → 47, heading 39 → 10;
    UNEXPLAINED 112 → 112, rng unchanged, entity sets unchanged,
    **nothing up**. mc2l30 t=2400+300: guard-terrain **5095 → 0**,
    terraform-houses **900 → 0**, castle-pad-z **300 → 0**,
    archer-ground-z 4292 → 4231; UNEXPLAINED 369 → 369. mc2l24
    t=5000+300: castle-pad-z 300 → 0, balloon-z 66 → 0, UNEXPLAINED
    7 → 7. mc2l24 t=25000+300: guard-terrain **6605 → 0**,
    `mc2l24-castle-piece-terrain-z` **2217 → 5**, terraform-houses
    300 → 0, castle-pad-z 300 → 0, walker-ground-z 2532 → 2335,
    balloon-z 183 → 165, splash-churn 38 → 21; UNEXPLAINED field
    **4394 → 4019**, extra 2 → 1. mc2l0 t=700+400: conforming
    **291 → 377**, `mc2-terraform-houses` **1024 → 0**, fire-churn-m0
    319 → 70, UNEXPLAINED 103 → 103. **THE ONE COST**, reported
    honestly: mc2l0 t=0+400 conforming **241 → 219** (`conforming +
    explained` 398 both ways — the 22 pairs move into
    `mc2-cast-timing-fields`, 43 → 74 pairs). It is the BUILDING arm
    alone (castle-only measures 241) and it is the authored village:
    the baked `.mgcl` heightfield already carries those terraces, and
    the port's own construction law lands them a unit or two
    differently. Net on mc2l0 across both windows: **+64 conforming**.
  - **FULL TAKE mc2l24** vs the session-9 baseline (15 conf / 19,135
    conf-or-explained / 494,747 unexplained field / 464 missing /
    6,038 extra / rng 10): **1,030 conforming**, 19,266
    conf-or-explained, UNEXPLAINED field **459,142 (−35,605)**,
    missing **409**, extra **6,034**, rng **10**. Whole-take rule
    counts after: `mc2-guard-terrain` **1,199** (corpus 1.81M before),
    `mc2l24-castle-piece-terrain-z` **804** (367k before),
    `mc2-castle-pad-z` **17**, `mc2-terraform-houses` **1** (37k
    before), `mc2-walker-ground-z` 234,015, `mc2l24-static-terrain-z`
    375,572 (untouched — see the lost list). A concurrent
    possession dig shares the tree, so part of the full-take delta is
    not this fix; the windowed A/Bs above are the isolated
    measurement.
  - **FIXTURE DRIFT (promotion owed, NOT applied).** All of it is
    signature SHRINK — `field:3,2:z`, `field:10,45:z`, `field:5,15:*`
    dropping out. mc2l4 24/24 drifted (was 19 as-expected), mc2l30
    23/24 drifted + 1 fixed, mc2l24 12/17 drifted + 2 fixed, mc2l0
    7 fixed. ONE new regression: **mc2l0 t=138 `field:9,1:z`** (a
    projectile spawn-z on the authored village terrace — the same
    early-window cost above; mc2l0 t=32/t=291 were already regressed
    by the concurrent dig). MC1/MC1HW suites unmoved. `cargo test -p
    mgc-sim` with `MGC_REQUIRE_GOLDENS=1` green, 333 lib tests, **no
    golden moved** (import-only).
  - **PROPOSED ROSTER RE-SCOPES (described, not applied).** Retire or
    re-note `mc2-castle-pad-z` (17 rows left) and `mc2-terraform-
    houses` (1 row left); narrow `mc2-guard-terrain` to `mc2l24` and
    re-triage its 1,199-row residue (it clusters where
    `mc2l24-static-terrain-z` does — the doomsday family, not the
    castle mound); re-note `mc2l24-castle-piece-terrain-z` and
    `mc2-balloon-z` as post-replay residuals. Every one of these
    rules is now a REGRESSION SENSOR for the pad replay — a hit-count
    jump means the reconstruct stopped firing.
  - Tests ×2, both non-vacuous by construction (each asserts the
    replayed map is NOT pristine, so a neutered arm fails):
    `castle_pad_reconstruct_rebuilds_the_mound_two_painters_left`
    lives a castle through two level-up painters and demands the
    replay rebuild that map from the terminal row alone (plus an
    idempotence leg); `building_pad_reconstruct_rebuilds_the_hut_
    terrace` does the same for a parked hut and adds the FENCE leg
    (every off-footprint cell the live rings wrote must come back
    unchanged, with a witness count so the assertion cannot pass
    vacuously).
  - **LOST BY CONSTRUCTION** (triaged, not attempted — the source
    entity is gone, so the pool holds no evidence): a DEMOLISHED
    castle's un-stamp residue (`RemoveCastleStage_385C0` EF:28071
    subtracts the pad back with a per-cell entity-LCG jitter, and
    nothing anywhere saves the original ground); a FINALIZED (10,18)
    volcano dome (`mc2_dome_tick`/`sub_31940` EF:23193 — the l30
    summit plateau, already closed by session-6 dig C: at t=0 the only
    live dome is mid-grow at a DIFFERENT site while the summit already
    reads 2624); any crater whose caster despawned.
  - **NEW LEADS.** ① `mc2l24-static-terrain-z` (375k, untouched) is
    the DOOMSDAY family and it may yet be replayable: the pyramid's
    flatten (`mc2_pyramid_attack` → `sub_56F10` EF:39499) is a
    deterministic ring expansion driven to a FIXED POINT (repeat
    `radius = 15 − f26` passes until the radius-7 disc is all
    type 0), and the (5,10) pyramid persists in the pool with its
    phase bits (`f44 & 8` expanding, `& 4` done) — a finished crater
    is reproducible by iterating the same loop from the pristine
    plane. But the measured l24 (2,3) signature is a CONSTANT per-slot
    delta from t=23411, which is a one-shot edit, not the progressive
    flatten — triage the onset event first. ② The mc2l0 t=0 cost says
    the baked `.mgcl` village terraces and the port's own
    `mc2_building_tick` disagree by a unit or two; a direct
    plane-vs-replay diff at t=0 would say which is retail's, and
    would also settle whether the authored terraces belong in the
    generator or in the first 30 ticks. ③ `mc2-walker-ground-z`
    (234k on l24) survives the pad replay — its remaining root is the
    same doomsday/volcano ground as ①.

- **THE RECORDER'S SNAPSHOT STRADDLES RETAIL'S INPUT POLL, AND THE
  PRESS LATCH SAYS WHICH SIDE — so the ±1 cast phase is not a delay
  knob, it is a PER-PRESS bit the recording already carries. The MC2
  arm now derives the cast phase from it and ignores `--input-delay`
  entirely. LANDED 2026-08-04 (session 9, ±1 cast-phase dig).**
  Successor to the cast-EDGE entry below; **retires its "`--input-delay
  3` absorbs the dominant +1" reading, which was an ALIASING artifact**
  (nearest-arm matching against a 4-6-tick press cadence while the port
  actually fired 3-4 pairs LATE — traced live with `MGC_CAST_TRACE=1`).
  - **THE FRAME ORDER, CITED.** `DrawAndEventsInGame_47560`
    (EF:31724): `PaletteChanges` → `sub_715B0` →
    **`ReadGameUserInputs_89D10` (EF:31734)** → **`MouseAndKeysEvents_
    17A00` (EF:31763)** → **`PlayerEvents_51BB0` (EF:31796, `Turn++`)**
    → `UpdateEntities_57730` ×`speedIndex` → draw → the native limiter
    spin (`InGameLoop_47320`). **Both the poll and the button consume
    run at the TOP of the frame, before `Turn++` and before the entity
    pass.** The poll rebuilds `MouseButtonState_18059C` from the ISR
    registers (EF:49675-83: bit0/1 = the press LATCHES @0x180746/
    0x180744, bit2/3 = held @0x18074C/0x18074A);
    `HandleMouseButtons_18F80` consumes bit 0 and clears it
    (PI:2043-49, family `byte_0x3B_59 == 1`; PI:2050 = the
    `bit0 || (bit2 && armed)` repeat arm); the tail of
    `MouseAndKeysEvents_17A00` (**PI:1049-52**, LABEL_306) then drops
    the LATCH REGISTER itself whenever the matching MouseButtonState
    bit is down — i.e. **the latch dies in the very frame that
    consumes it**.
  - **THE RECORDER'S SAMPLING POINT.** `build_record`
    (tools/mc_dosbox_recorder.py) reads the input registers from the
    same parked window as the struct, and MC2 records are parked in the
    settled tail (after the entity pass, inside MC2's own limiter
    spin). So record `r`'s registers are read AFTER frame `r`'s poll
    and BEFORE frame `r+1`'s. A press visible at record `r` may
    therefore belong to EITHER frame — and the latch resolves it:
    **latch still up ⇒ frame `r` did not poll it ⇒ frame `r+1` will.**
  - **THE LAW.** The input frame `r` actually consumed is
    `aligned(r) = (held(r) && !latch(r)) || latch(r-1)`, and the pair
    `(r-1 → r)` — which IS frame `r`'s transition — must carry it, with
    `aligned(r-1)` as its edge predecessor. In harness terms the pair
    takes its **END** record's command, not a delayed copy of its start
    record's.
  - **THE MEASUREMENT (port-independent: recorded registers vs retail's
    OWN arm ticks, the hand manifestation's `word_0x2E_46` 0 →
    nonzero).** Raw held edges split **308 / 95** on mc2l4 0+4200
    between "arm on the same record" and "arm one record later" — and
    the split is EXACTLY the latch bit: **latch=0 ⇒ delta 0 (308/314),
    latch=1 ⇒ delta +1 (95/95, no exceptions)**. Under `aligned`, arm
    records land on a rising edge with **delta 0 on 4,814 of 4,815
    right-hand casts corpus-wide** (mc2l24 2,778/2,778, mc2l4
    1,074/1,075, mc2l0 556/556, mc2l30 406/407) and 1,558/1,607
    left-hand (the residue is retail's own repeat arm — a HELD button
    re-arming without a new press, which the aligned LEVEL still
    serves). Latch runs are **always exactly one record long** (584 +
    594 runs on l24), which is the PI:1049-52 clear seen from outside.
  - **CHANGES.** `verify_mc2.rs`: new `raw_input_mc2` (held/latch,
    unmerged) + `align_cmd_mc2` (the law, with the derivation in its
    doc-comment); the run loop computes `aligned` per record and hands
    each pair `(cmd_now, pcmd)` instead of `(pcmd, prev_cmd)`;
    `MGC_CAST_RING=1` restores the legacy `--input-delay` ring for A/B
    and `MGC_CAST_TRACE=1` prints the port's cast pairs.
    `fixtures.rs`: the MC2 loop reconstructs identically (no ring) —
    the suite MUST match `verify-deltas`. **No sim change; MC1
    (`verify.rs`) untouched — it has no latch register.** Fixture
    bundles need no re-extract: the `t-(input_delay+2)..t+1` window
    already carries the two leading records `aligned` needs.
  - **A/B (windowed, one binary, env-toggled arms, back-to-back).**
    mc2l4 0+4000: entity sets **559/532 → 179/169**, (9,1)
    **261/327 → 37/47**, (10,12) 141/59 → 37/18, unexpl field 4,620 →
    **4,591**, rng 10 → **6**, `cast-timing-missing`+`-extra` 663 →
    **125**. mc2l0 0+4000: **raw conforming 1,771 → 2,032**, entity
    sets 504/540 → **101/140** ((9,1) 241/131 and (10,14) 0/125 both
    off the board). mc2l24 0+4000: **conforming 1,030 → 1,163**,
    unexpl field 5,641 → **5,454**, extra 19 → 16. mc2l30 0+4000:
    unexpl field 3,636 → **3,321**, missing 30 → 22, extra 73 → 65,
    entity sets 504/771 → **200/487**.
  - **FULL TAKES (aligned, vs the session-8 close above).** **mc2l0**
    8,626: **2,032 conforming** (was 1,771) / 3,236 unexpl field
    (+30) / 32 missing (−2) / 43 extra (+3) / rng 3. **mc2l4** 17,711:
    **14,823 explained** (was 14,811) / **11,184** unexpl field (−99) /
    87 missing / **72 extra** (−9) / **rng 6** (was 10). **mc2l24**
    67,391 grade: **1,163 conforming** (was 1,030) / 19,263
    conf-or-explained / 459,635 unexpl field (+493) / **405 missing**
    (−4) / 6,103 extra (+69) / **rng 8** (was 10). **mc1l0
    UNCHANGED** — 501 conforming / 10,632 unexpl field / rng 0, the
    baseline to the digit.
  - **THE FIELD-ROW RISE IS AN ACCOUNTING ARTIFACT, and it exposed the
    next lead.** A cast that is now phase-correct but lands in a
    DIFFERENT POOL SLOT stops being 1 missing + 1 extra row and becomes
    ~15 field rows. Specimen mc2l0 t=28 (the take's first cast): the
    port's bolt is byte-identical to retail's — (9,1) life 9/10,
    pos (78.19, 221.04, 5160), mana 33, action 1 — but sits at slot
    **206** while retail's sits at **453**. Cause: retail's dying
    (10,14) at slot 206 (`life -2` at t=28) **re-arms IN PLACE** at
    t=29, while the port frees the slot and re-allocates, so the free
    stack hands the bolt 206 and the respawned (10,14) 453. That is a
    (10,14) respawn law, not a cast law — and plausibly the root of
    mc2l0's whole `(10,14) 0 / 125` extra family.
  - **RESIDUE.** The remaining (9,1) rows are RIVAL casts, not the
    human's: on mc2l4 the survivors cluster at map positions
    (118,164), (54,237), (132,188) — nowhere near the human carpet —
    and belong to `mc2-rival-ai-lanes` (the un-imported AI decision
    lanes), exactly as the l24 late-window finding predicted.
  - Pinned by `mc2_pending_latch_defers_the_cast_one_record`,
    `mc2_sub_poll_click_casts_once_on_the_consuming_record`,
    `mc2_consumed_press_casts_on_its_own_record` and
    `mc2_long_hold_is_one_aligned_edge` (verify_mc2.rs) — the first two
    assert the LEGACY merge's edge index alongside the aligned one, so
    they fail if the old mapping is restored.
  - **SUITE DRIFT owed to a central `--promote` (fixture JSON out of
    remit):** mc2l0 **4 FIXED** (t=32, 79, 156, 291 now conforming) +
    2 drifted (t=28 the slot-swap specimen above, t=60 loses its
    `extra:10,14`); mc2l4 4 drifted (t=491, 520, 3407 lose their
    `extra:9,1`/`missing:9,1`; t=3449 becomes the slot-swap shape);
    mc2l24 1 drifted (t=2868); mc2l30 / mc1l0 / mc1hwl0 **clean**;
    **0 regressions anywhere**.

- **THE POSSESSION OVER-FIRE WAS NOT A MISSING SUPPRESSION LAW — THE
  PORT'S CAST *EDGE* WAS DEAD IN THE HARNESS. `prev_cmd` was read from
  `prev` AFTER `prev.take()` had already emptied it, in BOTH verify
  loops and BOTH fixture loops, so it never left its seed and
  `edge = cmd.fire && !prev_fire` degenerated to the raw HELD level for
  every run ever measured. LANDED 2026-08-04 (session 9,
  possession-over-fire dig).** Closes the bolt-launch-lanes dig's
  parting lead ("THE RESIDUAL IS AN OVER-FIRE, NOT AN IDENTITY";
  full-take l24 (9,1) 355 missing / 3,794 extra) and **REFUTES the
  session-4 ruling's scope** — the residue it waved off as decode skew
  was mostly this, and the decode was never touched (nor does it need
  to be).
  - **THE RETAIL LAW, END TO END.** The two registers behind a cast are
    the two halves of `MouseButtonState_18059C`, rebuilt from scratch
    every input poll (EF:49675-83): **bit 0/1 = the ISR press LATCH**
    (`x_WORD_180746` / `x_WORD_180744`), **bit 2/3 = the HELD state**
    (`x_WORD_18074C` / `x_WORD_18074A`). `HandleMouseButtons_18F80`
    (PI:2027-76) then splits the spell families on `byte_0x3B_59`:
    **`== 1` fires off bit 0 ALONE** and clears it (PI:2043-49);
    everything else takes `bit0 || (bit2 && spell->word_0x2E_46 > 0)`
    (PI:2050) — the repeat arm, live only while the cast window is.
    The frame tail then drops the GLOBAL latch whenever bit 0 is down
    (PI:1049-52), so the latch is one-shot: **one cast per physical
    click, however long the button is held.** Possession's
    `byte_0x3B_59` is 1.
  - **THE CAST CHAIN ITSELF IS ALREADY VERBATIM — do not go looking
    for a lockout.** `sub_5F660` case 1 (EF:60900-07): armed
    (`word_0x2E_46 > 0`) → set `byte_0x3C_60 = 1`, stamp the hand, run
    `sub_5F7E0`, `goto LABEL_23`, **no re-arm, no mana gate, no buzz**;
    not armed → the mana gate then `sub_5F7B0` (EF:60973:
    `word_0x2E_46 = word_0x30_48`). `sub_69640` (EF:55915) fires only
    at `word_0x2E_46 == word_0x30_48` and counts down one per tick,
    expiring into `sub_6D880` (EF:58215 — a pending-tier apply, NOT a
    cooldown). `word_0x36_54` is decremented at the tail and read by
    **nothing** on this path. `byte_0x154_340` is the charge
    accumulator (incremented per frame to a 200 cap, EF:5423-25, spent
    as `dword_0x10_16` by the leveled arms and simply reset by
    `sub_69900` at EF:56058) — **not a suppression latch**. There is no
    cooldown, no in-flight cap, and no token gate. The only thing
    standing between two bolts is `word_0x2E_46` and the press latch.
  - **THE MEASUREMENT THAT PINNED IT.** Instrumented `mc2l4 0+4000`
    (the manifestation's `word_0x2E_46`/`word_0x30_48` are IMPORTED per
    pair, so retail's own arm cadence is readable straight off the
    trace): possession's `word_0x30_48` is **3**; retail arms
    (`f26` 0→2) **404** times; the recording's held-right register has
    **409 rising edges**; the port launched **883**. Every port
    `mc2_cast_input` sample in the window read `edge == held`, 2,058
    for 2,058 — the edge detector was structurally incapable of
    reporting anything else. After the fix the port launches **408**.
    (`mouse_clicks` — the recorded latch — is never set without
    `mouse_buttons` on this corpus, so the harness's `held || latch` OR
    is a no-op and the held register's own rising edge IS the press.)
  - **CHANGES.** `verify.rs` + `verify_mc2.rs`: `prev_cmd = pcmd` moved
    INSIDE the `prev.take()` arm (env `MGC_NO_FIRE_EDGE=1` restores the
    old behaviour for A/B); `fixtures.rs`: the same, both loops — the
    suite MUST reconstruct input exactly like `verify-deltas` or its
    signatures drift from the triage run. Port-side, two verbatim
    corrections on the same press path: `mc2_cast_input`'s repeat test
    is `f59 != 1`, not `f59 == 0` (PI:2043 tests `== 1`), and
    `mc2_cast_gate`'s armed possession arm raises `f56` FIRST and
    unconditionally — retail never reaches the sound-29 `v6` flag
    there, so a broke wizard re-pressing possession no longer buzzes.
  - **NUMBERS (A/B, one binary, env-toggled, back-to-back).** **mc2l0
    0+4000: raw conforming 682 → 705**, unexplained field 3,660 →
    **3,553**, entity sets 112/917 → **497/642**, (9,1) 12/168 →
    241/131, (10,14) 149 → **125**. **mc1l0 0+4000: raw conforming
    450 → 501**, unexplained field 7,440 → **6,524**, unexplained extra
    135 → **79**, entity sets 400/1,029 → **429/401**. mc2l4 0+4000:
    unexplained field 5,348 → **5,092**, extra 55 → **49**, (9,1)
    80/443 → 261/327. mc2l30 0+4000: explained 2,867 → **2,876**,
    unexplained field 4,826 → **4,519**, extra 89 → **78**, (9,1)
    23/213 → 103/143. mc2l24 0+4000: unexplained field 5,729 →
    **5,641**, extra 23 → **19**, (9,1) 11/138 → 79/100.
  - **FULL-TAKE mc2l24 (final tree, shared with the concurrent
    doomsday dig — absolutes, not attribution):** 69,207 pairs, 1,816
    torn, 67,391 fixture-grade, 15 conforming, **19,135**
    conforming-or-explained, UNEXPLAINED **494,747 field / 464 missing
    / 6,038 extra** (from 495,023 / 482 / 6,984 — the extra side is
    **−946**), rng **10** (was 12); (9,1) **140 / 1,733** (was
    355 / 3,794).
  - **THE RULING, RE-TESTED.** On every take where the HUMAN casts, the
    (9,1) family is now two-sided (l4 261/327, l30 103/143, l24-early
    79/100, l0 241/131) — the one-sided extra family the dig was
    chartered on is GONE. What is left is genuine ±phase: measured
    against retail's arm ticks, `retail_arm − port_fire` is **+1 on 227
    of 408 casts, +2 on 96, 0 on 39** at `--input-delay 2`, i.e. the
    port fires one tick EARLY. So the session-4 skew ruling **still
    holds for the residue, and only for the residue** — but it never
    covered the bulk, and the decode still needs no change.
    **`--input-delay 3` absorbs the dominant +1**: same window, mc2l4
    (9,1) 261/327 → **173/135**, (10,12) 141/60 → **68/60**, entity
    sets 571/542 → **407/318**, unexplained field 5,092 → **5,033**,
    explained pairs 3,064 → **3,069**. That is a corpus-wide knob and a
    roster decision, so it is measured here, NOT landed.
  - **THE l24 (9,1) RESIDUE IS NOT INPUT AT ALL.** Late window
    (40000+4000): retail casts **zero** possession bolts while the port
    casts 19 — these are RIVAL casts, and rivals' `cooldown[]`/`burst`
    AI lanes are the ones `retail_import_mc2` explicitly does not
    import ("the AI decision-lane decode is still owed"). They belong
    to `mc2-rival-ai-lanes` / open-leads, not to possession. The fix
    still halved them (56 → 19) by keeping the shared world instance
    closer to retail.
  - **SUITE ACTION OWED (not applied — fixture JSON is out of this
    dig's remit).** `mgc-conform fixtures conformance/*.json --promote`
    is needed: mc2l0 **5 fixed / 2 regressions / 2 drifted** (t=32
    `extra:10,14` and t=291 `missing:9,1` are both the ±1 phase),
    mc2l4 5 drifted, mc2l30 2 drifted, mc2l24 6 drifted, mc1l0 2
    drifted, mc1hwl0 clean, 0 regressions anywhere outside mc2l0.
    Also propose re-scoping `mc2-cast-timing-extra`'s note: it no
    longer carries "the possession fresh-arm input-reconstruction
    extras" as a bulk family.
  - **NEW LEAD (tier 1/2 possession re-fire, traced but NOT landed —
    no corpus coverage).** `sub_69640`'s else-branch (EF:55995-56013)
    is fully readable now: the `byte_0x3C_60` signal drives a 3-tick
    decay counter (1→2→3→4, reset at >3 with the trailing `sub_68DE0`
    SKIPPED via LABEL_26), the re-fire happens ONLY at counter == 1,
    and it calls **`sub_69900`** — i.e. tiers 1/2 re-fire the BASIC
    (9,1), not their own (9,17) — and it is **NOT mana-debited**
    (`sub_68DE0`'s `word_0x2E_46 != word_0x30_48` arm only pins regen,
    EF:55569-93). The port instead re-runs `mc2_spell_fire` (leveled
    entity + full debit) with no decay counter. Three wrong lanes,
    zero observables; needs a tier-1/2 take before it is touched.
  - Pinned by `mc2_possession_held_button_casts_exactly_once` (a
    24-tick hold on a 3-tick marker casts ONCE; the neutered
    level-trigger casts 8 — the over-fire's exact shape),
    `mc2_repeat_family_is_every_byte_3b_except_one` (fails under the
    old `f59 == 0`) and `mc2_possession_repress_while_armed_never_buzzes`
    (fails with the mana gate restored). All three verified failing
    against their neutered arms. 331 lib tests + every integration
    suite green under `MGC_REQUIRE_GOLDENS=1`; no golden moved.

- **THE `.mgcr` MC2 DECODE GUESSED "THE HIGHEST STACKED CELL IS SLOT
  999", so every snapshot with the TOP of the pool in use handed the
  import a free stack shifted by a constant — and the census then threw
  it away and replayed the pair on a descending slot scan, re-ordering
  every spawn. LANDED 2026-08-04 (session 9, mgcr pool-base dig).**
  Closes the SECOND slot-order source root-caused in open-leads 0b's
  session-8 update. Decode-side only: no gameplay law moved.
  - **WHY THE OLD RECOVERY WAS UNPINNED.** `D41A0_0` is a static, but
    DOS/4GW's load delta makes its guest address run-specific, so
    `mgcr::mc2_stack` recovered the pool base from the cells: scan
    `cells[0] − s·168` from s=999 down, take the first candidate under
    which every cell decodes in range. Every cell is a pool pointer, so
    **alignment is s-independent** — the in-range candidates form one
    contiguous interval and the only binding constraint is "max index
    < 1000", i.e. *the highest stacked cell is slot 999*. True only
    while the top of the pool is free. Occupy the top N slots (they
    then never appear on the free stack) and EVERY decoded slot
    inflates by N.
  - **THE MEASUREMENT (probe over every MC2 snapshot in the corpus).**
    mc2l24: **14,219 of 69,221 snapshots shifted** (20.5%), shifts
    1..993, e.g. t=56539 shift 3 (205 of 493 cells landing on LIVE
    records), t=60101 shift 2 (197/576), t=62929 shift 4 (129/226),
    t=64566 shift 5 (223/526). First shifted tick is **t=54932** — the
    take runs clean before it. mc2l0/l4/l30: shift 0 on all 35,677
    snapshots (their pools never fill), which is why the bug hid.
  - **THE FIX — VALIDATE THE BASE AGAINST THE POOL IMAGE**
    (`mgc-formats/src/mgcr.rs`: new `mc2_pool_base` /
    `mc2_base_from_cells` / `mc2_stack_cells`, `mc2_stack` now takes
    the recovered base). Retail's frame-top reap zeroes `class` before
    pushing (`sub_57F20`), so every free-stack cell must land on a
    `class3f == 0` record, and slot 0 is the reserved null that is
    never stacked. That base is **unique on all 104,824 corpus
    snapshots** with a non-empty free stack, and the decoded set is
    then EXACTLY the class-0 slots minus slot 0 on every one of them
    (= the import's own `scan_free` census, so the fallback can no
    longer fire). Ambiguity or no candidate ⇒ empty stack, i.e. the
    import's descending-scan fallback still rides.
  - **INDEPENDENT CORROBORATION.** The recovered base equals
    `base160 + 736_026` in **all 104,824** snapshots across four
    separate process runs (both are statics in the same image, so
    their distance is a build constant). Not used as the recovery —
    it is per-build — but it pins the validator's answer without
    reference to the class-0 criterion.
  - **THE RECYCLE STACK NEEDED IT TOO — the opposite validator.**
    Its cells are LIVE victims (`sub_49F90`'s sacrificable list), so
    recovering ITS base from "max index == 999" put mc2l4's victims on
    bogus FREE slots: 23,700 cells over 2,851 snapshots, every one of
    them decoding onto a class-0 record that the import then chained
    into the port's free list (and every one of those pairs took the
    fallback). Both stacks now share the one recovered base; under it
    **0 of 48,049 corpus recycle cells** land on a free record, so the
    import's `class64 == 0` filter drops them all — correct: a recycle
    victim is not a free slot.
  - **CONFORMANCE (windowed A/B, one frozen binary, env-toggled arms).**
    `free-stack fallback` stderr lines / gross missing / gross extra /
    UNEXPLAINED field·missing·extra / computed `slot-desync` rows:
    l24 60000+300 **296**/3271/3403/2503·2·49 →
    **0**/8/159/2635·1·28; l24 62800+300 **288**/3433/3288/6590·8·76 →
    **0**/411/286/6563·1·47; l24 56400+300 **300**/2395/2471/1169·4·61
    → **0**/46/123/1170·0·47; l24 64500+400 (the fountain window)
    **400**/303/972/759·1·52 → **0**/4/673/820·0·9; l4 4300+300 (the
    recycle window) **292**/142/138/946·2·2 → **0**/41/43/952·1·2.
    Over the four l24 windows: fallback **1,284 → 0**, gross missing
    **9,402 → 469 (−95%)**, gross extra **10,134 → 1,241 (−88%)**,
    unexplained extra 238 → 131, unexplained missing 15 → 2, and the
    computed `slot-desync` rule goes **124/124 rows across 47 pairs →
    ZERO** (those rows are gone, not re-labelled). UNEXPLAINED field
    rises 11,021 → 11,188 (+1.5%): entities that used to be absent are
    now present in retail's slot and get compared lane by lane.
    Cross-take regression probes mc2l0 / mc2l4 / mc2l30 at 0+2000 are
    **byte-identical** between arms (shift 0, empty recycle there), and
    the MC1/HW arm never touches this decode.
  - **FIXTURE SUITES (A/B, one binary, per manifest).** mc1l0 68 (2
    drift), mc1hwl0 29, mc2l0 41 (2 regressions / 5 fixed / 2 drift),
    mc2l4 24 (5 drift), mc2l30 24 (2 drift) — **identical in both
    arms** (the mc2l0 regressions/fixes belong to the concurrent
    possession dig, not this one). mc2l24 12 as-expected/5 drift →
    **11/6**: t=64000 LOSES `missing:10,39` and gains that ball's
    field lanes. Wants a `--promote` pass — NOT applied here.
  - **FULL-TAKE mc2l24** (whole tree, so it also carries the
    concurrent possession dig): 69,207 pairs, 1,816 torn, 67,391
    fixture-grade, **15 conforming** (=), 19,135 conforming-or-
    explained (was 19,131), **494,747 unexplained field** (was
    495,023), **464 missing** (was 482), **6,038 extra** (was 6,984,
    −14%), **rng 10** (was 12), and **zero `free-stack fallback`
    lines** in 69,207 pairs (was ~14,219 pairs' worth).
  - **WHAT IT IS NOT.** The t=3569 / t=13330 scripted-wave slot desync
    is NOT this bug — l24's first shifted snapshot is t=54932 and the
    t=3500+300 window is byte-identical in both arms. The whole-take
    `slot-desync` residue (208/208 rows across 21 pairs) lives in that
    early region and still wants its own cause. Likewise the (10,39)
    extras in the fountain window (673 after the fix) are a real
    over-spawn, no longer masked by a slot skew.
  - Pinned by `mc2_pool_base_is_pinned_by_the_free_records_not_the_max_index`,
    `mc2_recycle_stack_rides_the_free_stack_pool_base` and
    `mc2_ambiguous_pool_base_yields_no_stack` (mgcr.rs unit tests on
    synthetic snapshots; all three FAIL against the old recovery — the
    first prints the literal `[999, 400, 399, 304]` vs `[700, 101,
    100, 5]` shift).
  - **NEW LEADS.** (a) The port has NO recycle-victim allocation path:
    retail pops the recycle stack when the free stack is exhausted
    (74 mc2l24 snapshots have a FULL pool), the import filters those
    live slots out, so the port simply fails to spawn there.
    (b) `base160 + 736_026` could become a decode cross-check (or the
    recovery, if the recorder ever stamps the build).
    (c) The mc2l24 suite drift wants `--promote`, and the roster's
    computed `slot-desync` rule now absorbs far fewer rows — re-scope
    it to the early-take wave family.

- **THE TIER-0 POSSESSION BOLT IS (9,1), NOT (9,17) — the port had no
  subtype-1 creator AT ALL and launched the leveled entity for every
  tier — and FOOL'S-MANA OPEN-5's missing launch lift turned out to be
  the SAME retail law seen from the other end. BOTH LANDED 2026-08-03
  (session 8, bolt-launch-lanes dig).** Closes the claim-pulse dig's
  parting lead (full-take l24 "(9,1) 362 missing / 0 extra") and
  fools-mana.md OPEN-5.
  - **THE TIER GATE PICKS AN ENTITY, NOT A PAYLOAD.** `sub_69640`
    (EF:55946-49) branches on `SPELLS[model].subspell[tier].life_0x1A`:
    **0** → `sub_69900` (EF:56039) → `SummonManaPosession_4D3B0`
    (EF:34764) = class 9 model **1**, **action 1**, speed/minSpeed 384,
    `maxLife = 4096/384 = 10`, mana 50, row `str_D7BD6[61]`,
    **`xtype_0x41_65 = 10`**, sprite 209 +
    `SetEntityShiftRot_49EA0(2*pitch, **5*fov/2**)`; **1..3** → the
    inline (9,17) arm (EF:55950, `sub_4DDD0` EF:35132 — same row/sprite
    but action 18 and ShiftRot `2*fov`), `byte_0x44_68` 54 / 69 / the
    NewEvent 0; **>3** → the `<= 3` gate fails and NOTHING is cast.
    Row 1's baked `life` column is (0,1,2), so tier index ≡ life.
  - **`sub_69900`'s TAIL, pinned field by field against the recording**
    (mc2l4 t=13 slot 303, `dump-state`; full table in
    docs/traces/mc2-possession-delivery.md, Addendum 2026-08-03):
    `dword_0x10_16` = **200** (@0x10 → f26); `word_0x26_38` = the spell
    TOKEN's slot **267** (@0x26 → f40 — DELIBERATELY not ported: the
    port spends f40 on the spell INDEX, which is `mc2_proj_impact`'s XP
    back-ref, while retail hard-codes `sub_6D8B0(id, 1, 1)` per handler
    at EF:63314/59052; the lane is not compared); `mana_0x90_144` = the
    TOKEN's mana **33**, not the ctor's 50 (@0x90 → f140, a COMPARED
    lane); box `apitch/aroll` **180**, **afov 187** = 5·75/2 off sprite
    209's (speed_6 0, rotSpeed_8 150) — the leveled twin's is 150;
    impact (10,12); and `actSpeed` **336**.
  - **336 IS THE FIND INSIDE THE FIND.** Both possession arms add the
    carpet boost RAW — `v2x->actSpeed += a2x->actSpeed` (EF:56048 /
    EF:55953). The `[384, 0x2000]` clamp the port applied to every cast
    belongs to `sub_6DCA0` ALONE (EF:44226-31); on a REVERSING carpet
    retail genuinely launches a sub-384 bolt, and `mc2_launch` was both
    flooring it at 384 and dropping the negative term (`p.speed.max(0)`).
  - **A AND B ARE ONE LAW.** `position.z += <launcher>->array_0x52_82.fov`
    appears at EF:56054 / EF:55969 with the WIZARD as launcher and at
    EF:26688 (`sub_36770`) / EF:26718 (`sub_36850`) with the fool's-mana
    SPHERE as launcher: "leave from the top of the launcher's own box".
    The cast half was already carried — `World::muzzle` returns
    `p.z + PLAYER_HH`, and PLAYER_HH is exactly the MC2 wizard's fov
    (`AddPlayer_4A920` EF:33334 sets sprite row 44; MC2 row 44
    rotSpeed_8 = 200 → fov 100, MC1 row 44 height 200 → PLAYER_HH 100).
    The trap half was OPEN-5 and is now `e.z + e.f84` in
    `mc2_fools_bolt`.
  - **OPEN-5's "self-detonation" WAS NEVER A PROBE FILTER.**
    `sub_10780` (EF:3739-71) has no launcher exclusion — flags,
    xtype narrowing, `a1x->id_0x1A_26 != v5x->id_0x1A_26`, box. What
    keeps retail's bolt off its own sphere is (a) the sprung tier-0
    sphere is UNMAPPED and class-zeroed INSIDE ITS OWN TICK — the walk
    runs `sub_57F20` (Events.cpp:551; body :5209 = `SetMapEntity_57E50`
    + `class = 0` + free-stack push) the instant
    `DisableEntityDrawing04_57F10` latches `byte[1]&4` — and (b) retail
    probes ONCE, at the END of a full 384-unit step (`sub_65C20`
    EF:63126-29). Our soft kill leaves the sphere linked to the tick-top
    reap and our anti-tunnel chord march probes 128-unit sub-steps
    retail never visits, so the exclusion rides the shared owner gate
    instead: `mc2_fools_bolt` stamps `id24 = sphere.id24` and
    `victim_scan`'s `c.id24 != id` drops it, for an authored sphere
    (id24 = own slot on both sides) and a cast decoy (id24 = caster)
    alike. **NEW LEAD (fools-mana OPEN-7):** the chord march probes from
    the MUZZLE OUT where retail probes only the endpoint — any future
    launcher that does not inherit its host's id will detonate on tick 1.
  - **CHANGES.** `CREATORS` gained `(1, 1, 384, 10, 61, 209)`;
    `mc2_spawn_cast_proj` gained the possession pair's ShiftRot and the
    (9,1)'s `xtype = 10`; `mc2_flyer_tick` now SKIPS `mc2_proj_filter`
    on the claim arm (retail's narrowing lives inside `sub_10780`,
    EF:3765-68 — `sub_108B0` has none, so filtering a claim hit by
    xtype 10 would have swallowed worm (5,22) and building (10,45)
    claims); `mc2_spell_fire` spell 1 and `mc2_rival_emit` (which
    hardcoded 17 for every rival cast) both pick the entity off
    `life_0x1A`; `mc2_fools_bolt` lifts by the sphere's f84.
  - **NUMBERS (A/B, one binary, env-toggled, back-to-back).** mc2l4
    0+4000: explained 3065 → **3067**, unexplained field 5351 →
    **5348**, **(9,17) 443 extra → 0** with (9,1) 80/0 → 80/443, gross
    `action` 3706 → **3399**, `model` 628 → **322**, `mana` 1813 →
    **1551**, `speed` 1295 → **1030**, `applied_pitch` 1063 → **756**.
    mc2l30 0+4000: (9,17) 213 extra → 0 (all now (9,1)), gross `action`
    6142 → **5957**, `applied_pitch` 908 → **723**, `speed` 834 →
    **658**, `mana` 775 → **590**, `model` 642 → **459**; unexplained
    field 4824 → 4826. mc2l24 0+2000: gross `model`/`action` 786/783 →
    **617**, `speed` 1497 → **1329**, `mana` 409 → **245**,
    `applied_pitch` 878 → **709**, `z` 9600 → **9520** (the lift alone
    is −12 of those); unexplained field 1007 → 1010. Net: ~1,400 gross
    field rows per take stop being wrong, ±3 unexplained (the rows were
    already inside the class-9 capture rules, which are class-scoped and
    survive the relabel).
  - **FULL-TAKE mc2l24 (final tree, shared with the concurrent
    dweller/doomsday dig — absolutes, not attribution):** 69,207 pairs,
    1,816 torn, 67,391 fixture-grade, **15 conforming**, 19,131
    conforming-or-explained, UNEXPLAINED **495,023 field / 482 missing /
    6,984 extra**, rng **12**; (9,1) 355 missing / 3,794 extra, (9,17)
    2/24, (10,12) 540/1,114.
  - **THE RESIDUAL IS AN OVER-FIRE, NOT AN IDENTITY.** With the entity
    right, the (9,1) family reads ~5× (l4) to ~10× (l24) more port rows
    than retail rows — the port launches far more possession bolts than
    the recording does at the same input. That is the next possession
    lead, and it is orthogonal to everything above (it was hiding under
    the (9,17) extras before).
  - **RECORDER LEAD (concrete, two fields).** Retail aims BOTH bolts at
    `wizext.nextEntity_0x18_24 + yaw` / `entityIndex2_0x1A_26 + pitch`
    (EF:56060-66 / EF:55970-71) — the per-frame FREE-LOOK input deltas
    (`playerInputs_0x6E3E` word6/word8 → EF:38065-66; the camera adds the
    same pair at EF:40273-74). They live in `type_str_164` at **+24 /
    +26**, i.e. the very block `RetailPlayerMc2` already reads for
    `cmd_speed` (+12) and `strafe` (+16), and the `.mgcr` does NOT carry
    them — so a free-looking player's launch heading is unreproducible
    from the recording. Proposed: add `look_yaw`/`look_pitch` to
    `RetailPlayerMc2` on the next recorder + format pass.
  - Pinned by `mc2_tier0_possession_launches_the_basic_bolt_with_sub_69900s_tail`
    and `fools_trap_bolt_leaves_from_the_sphere_box_top_and_clears_its_own_muzzle`
    (world.rs lib tests; the first fails on the neutered arm with
    "tier 0 must NOT launch the leveled (9,17)", the second with the
    lift removed — and its CONTRAST arm shows a same-muzzle bolt with a
    foreign owner DOES detonate on the sphere, so the owner gate is
    load-bearing rather than lucky). The pre-existing
    `mc2_possession_tier0_does_not_refire_while_the_marker_runs` had to
    be flipped from counting model 17 to model 1 — the port's old
    behavior in one line. No golden moved; 328 lib tests + all
    integration suites green under `MGC_REQUIRE_GOLDENS=1`.

- **THE MC2 CONFORMANCE IMPORT DOUBLE-PUSHED EVERY GHOST SLOT ONTO
  THE FREE STACK — so any spawn burst deeper than the ghost count
  re-`NewEvent`ed a slot it had just filled. The (5,0) pyramid-summon
  "misses" were the doomsday worm chain RE-ALLOCATING ITS OWN HEAD.
  LANDED 2026-08-03 (session 8, (5,0) summon-cadence dig).** The
  session-7 hypothesis (a state-9 `count(m0) < 4` cap divergence) is
  **DISPROVEN** — the cap law is already verbatim (see below); the
  bug was in the import's free-list reconstruction and it was
  **global to every MC2 pair**, not a pyramid law at all.
  - **THE MEASUREMENT.** mc2l24 pair 53808→53809: retail spawns a
    17-record m0 chain (head slot **905** + 16 children 837, 813,
    796, 727, 690, 72, 65, 63, 62, 61, 625, 620, 423, 422, 420, 407
    — the head's `word_0x34_52` chain read straight off the corpus),
    the port spawned **nothing visible**. Instrumented pop order:
    905, 837, 813, 796, 727, 690, **905 again**, 837, 813, 796, 727,
    690, 72, 65, 63, 62, 61. The 7th pop re-entered `NewEvent_4A050`
    on the live head and `*e = Ent::default()` zeroed its class; the
    child loop then byte-copied that zeroed head into every
    subsequent child, so the whole chain projected as class 0 =
    16 `missing` rows in one pair.
  - **THE LAW.** Retail's frame top (`UpdateEntities` EF:39948-56)
    reaps every disabled record with `sub_57F20` (Events.cpp:5209-38:
    tile-unlink, `class = 0`, `dword_0x35++; pointers_0x246[top] =
    entity`) — ASCENDING slot order — and only then rebuilds the
    per-model buckets. So a `.mgcr` capture's recorded free stack is
    the **pre-reap** image and the ghosts are exactly the slots the
    next frame's reap will push. The port already runs that reap
    (`World::tick`'s strict-MC2 top pass, DEVIATIONS.md "World::tick
    (MC2 sweep: disabled dispatch)"), and `retail_import_mc2` was
    ALSO appending `ghost_slots` — the double push its own comment
    warns about ("appending them here too would double-push the
    slots"). The corpus confirms the reconstruction: at t=53808 the
    recorded free stack is 716 entries ending
    `…, 406, 407, 420, 422, 423, 620, 625, 61, 62, 63, 65, 72` and
    the 6 ghosts {690, 727, 796, 813, 837, 905} are absent — reap
    them once, ascending, and the top becomes 905, i.e. retail's
    exact allocation order for the chain. FIX: drop the
    `self.g.free.extend(ghost_slots)` (`conformance.rs`; the binding
    still feeds the census `scan_free`). Pinned by
    `mc2_import_leaves_the_ghost_free_push_to_the_tick_reap`
    (non-vacuous: restoring the extend fails both asserts).
  - **SECOND FIX — the pyramid's SUMMON STRIDE had no import home.**
    `sub_21850` stamps the ring stride into `word_0x4A_74` (@0x4A,
    682 for every creature pick / 256 for the m19 swarm,
    EF:13160/13173/13186/13199) and `sub_21AB0` fans the ring at
    `stride * repeat + yaw` (EF:13364). The uniform class-5 import
    read f50 ← @0x30, which is DEAD for the pyramid, so every
    replayed summon spawned stacked on the pyramid's own bearing
    (t=53808: retail x 7616 vs port 7936). f50 now imports @0x4A for
    (5,10) — the third (5,10) exception next to f26 ← @0x10 and
    f36 ← @0x28. Pinned by
    `mc2_pyramid_import_keeps_the_summon_stride`.
  - **THE CAP LAW IS ALREADY RIGHT (hypothesis retired).** `sub_223E0`
    (EF:13780-13808) recomputes four counts from the per-MODEL bucket
    lists rebuilt at frame top (class 5, `life >= 0`, action not
    0xB4/0xE8/0xEA — EF:39987-40007), and `mc2_pyramid_pick_summon`'s
    predicate matches that membership exactly. At the l24 summon the
    port and retail agree on the pick (both selector 3, both at
    state 8 t=53806). NOTE for a future dig: the decompile's three
    `bytearray_38403x[0]` loops (picks 3/4/6) vs the fourth
    `[100/4]` = bucket 25 (pick 5, m25) are internally inconsistent
    with the "cap counts the summoned model" reading — either the
    caps for picks 4 (m21, <12) and 6 (m19, <28) really are MODEL-0
    counts (kept verbatim) or the decompiler lost two bucket indices.
    Nothing at l24 discriminates; leave verbatim.
  - **CONFORMANCE (windowed A/B, same binary, env-toggled arms, 300
    pairs each).** UNEXPLAINED field / missing / extra:
    t=53700 3698/29/21 → **3098/1/31**; t=54700 3074/16/42 →
    **2532/0/45**; t=56400 1333/13/55 → **1169/4/61**; t=60000
    3069/41/42 → **2577/2/51**; t=62800 7212/40/56 →
    **6605/8/80**. Totals **18,386 → 15,981 field (−13%)**,
    **139 → 15 missing (−89%)**, 216 → 268 extra. Every named (5,0)
    miss tick is answered: 53808 (12/0 → gone), 54825 (14/0 → gone),
    56539 (9/0 → 2/3), 60101-60103 (50/16 → 48/49, now BALANCED =
    the computed slot-desync class), 62929 (12/0 → 0/3). Pair 53808
    alone: 77 field + 16 missing → **4 field, 0 missing, 1 extra**.
    The `extra` rise is the mirror of the same law — spawns that used
    to be clobbered into a shared slot now all survive.
  - **FIXTURE SUITES (A/B, same binary).** MC1/HW untouched (68/68,
    29/29). MC2 baseline → fixed: mc2l0 4 fixed/0 regressions/0 drift
    → **6 fixed / 1 regression / 3 drift**; mc2l4 3 → 4 drift;
    mc2l30 0 → 1 drift; mc2l24 2 → 5 drift. The drifts are
    IMPROVEMENTS (l0 t=449 loses `missing:10,0`, t=3449 loses
    `missing:10,12`, l4 t=39 loses `field:10,0:rand/x/y` +
    `missing:10,0`) and want a `--promote` pass. **NEW LEAD**: the
    one regression is an `extra:10,14` on mc2l0 (t=32, and the same
    row drifts into t=33/60/79) — a (10,14) the port over-spawns that
    the duplicate slot used to swallow.
  - **FULL-TAKE mc2l24 (end of session 8, whole tree — this dig plus
    the concurrent claim-pulse dig).** 69,207 pairs (13 gaps), 1,816
    TORN, 67,391 fixture-grade: **15 conforming** (was 10), 19,131
    conforming-or-explained, **495,023 unexplained field** (was
    500,845), **482 missing** (was 975, −51%), 6,984 extra (was
    6,227), **rng 12** (unchanged). (5,0) is no longer a headline
    family: 50 missing / 55 extra whole-take, near-balanced, first at
    t=56539 — all of it the `mc2_stack` shift lead above.

- **THE (10,12) POSSESSION CLAIM PULSE WAS NEVER AN ENTITY IN THE
  PORT — and the BASIC (9,1) bolt was missing from the claim-probe
  gate, so it detonated on everything it grazed. BOTH LANDED
  2026-08-03 (session 8, claim-pulse dig).** Open-leads entry 0
  banked "(10,12) missing 313 (l30) / 779 (l4) = possession
  WEAK-PULSE family"; the l24 `want=12 got=54` rows were the same
  machine seen as a slot skew.
  - **THE RETAIL LAW (already traced, now ported).** Possession is
    delivered by MAIL FROM A SEPARATE ENTITY, not by the hit
    (docs/traces/mc2-possession-delivery.md §1-§3). Both bolt
    handlers spawn it: `CastPosses_65F60` (EF:63306-19, class-9
    action **1**, the basic (9,1)) spawns `_4A190(&pos, byte_0x43,
    byte_0x44)` = (10,12) and copies id/yaw/pitch; `sub_674C0`
    (EF:59032-59058, action 18, the leveled (9,17)) spawns TWO
    children on a victim — the pulse FIRST ((10,12), or **(10,70)**
    when the payload is (10,69), EF:59036-39), then the (10,54)/
    (10,69) aura, with `sub_6D8B0(id, 1, 1)` = the possession-XP
    mail (EF:58228, `class==3 && model==0` guard = the human only)
    and `sub_65780` (EF:62836) = an ACCURACY-STATS counter on the
    caster's wizext, not the claim. Ctors `NewAdd0A0C_4E8C0` /
    `NewAdd0A46_4E950` (EF:35573/:35595) are byte-identical bodies:
    life 8, `subSpellIndex_0x2A_42` 64000, sprite 41, `byte[0] =
    (b & 0xF6) | 1`, box 512³, no RNG. Ticks
    `PossesHitMana_320E0` / `sub_32120` (EF:23546/:23559): bump
    @0x10, class-10 PRE-decrement, anim, then `sub_112D0(0|1)` —
    the ch1 broadcast, every tick of the 9-tick window.
  - **WHAT THE PORT DID.** `mc2_proj_impact`'s (10,12)/(10,54)/
    (10,69) arms ran ONE `area_write(i, 1, …)` from the BOLT and
    returned `None` — no entity, and the claim reach was the bolt's
    own box for one tick instead of the pulse's 512³ box for nine
    (retail's near-miss claims are exactly that reach). Fixed:
    `Gen::mc2_spawn_claim_pulse` (mc2/effects.rs) + the forced
    twin's tick `Gen::mc2_steal_pulse_tick` (action 0x4D; the weak
    action 12 was ALREADY equivalent to MC1's `possess_flash_tick`
    and keeps riding the shared class-10 band, world.rs).
  - **CTOR PINNED AGAINST THE RECORDING.** mc2l4 pair 22→23, slot
    309: the port now matches retail on EVERY projected lane —
    class/model/action 10/12/12, life 7 (it spawns ahead of the
    walk cursor and ticks the same pass), max_life 8, x/y/z,
    heading, pitch, applied_yaw **125**, applied_pitch **512**,
    speed 16, mana 0, rand. The applied_yaw/applied_pitch split is
    the ORDER fingerprint: `SetEntityIndexAndRot_49CD0(41)` writes
    all four lanes from the sprite row, then `SetEntityShiftRot_
    49EA0(512, 512)` overwrites pitch/roll/fov only — so row 41's
    half rot-speed survives at @0x52.
  - **THE SECOND BUG THE PULSE EXPOSED.** With the pulse invisible,
    the port's over-detonation was invisible too. `mc2_flyer_tick`
    gated the claim probe on `tick70 == 18`, but `sub_108B0` has
    exactly TWO callers and action **1** is the other one — its own
    comment said so. Every basic (9,1) bolt (including the ones the
    importer replays) therefore ran the generic any-solid
    `sub_10780` probe AND skipped possession's ground-skim clamp
    (EF:63262-64, likewise `is_possess`-gated). l30 went 258 retail
    pulses vs **714** port ones; fixed to `matches!(tick70, 1 | 18)`.
  - **NUMBERS (A/B, same binary, back-to-back, env-toggled).**
    mc2l4 0+4000: explained pairs 3,046 → **3,061**, unexplained
    field 6,734 → **6,652**, entity sets 819/1,183 →
    **627/694**, (10,12) 279 missing/0 extra → **89/79**.
    mc2l30 0+6000: explained 4,422 → **4,433**, unexplained field
    7,826 → 7,830, unexplained missing 65 → **58**, entity sets
    1,688/961 → **1,445**/1,027, (10,12) 258/0 → **55/72**.
    mc2l24 63900+5000: explained 994 → 994, unexplained field
    10,519 → 10,557, unexplained missing 26 → **23**, (10,12)
    71/0 → 71/**49**.
  - **WHY l24 KEEPS ITS 71.** On l24 the port's pulses are pure
    SLOT desync, not timing: same ticks (64566, 65040, 65140,
    65272, 65495, 65542, 65623, 65682, 65707 …), retail at high
    slots (423/485/397/247) and the port at low ones (7/9/18/24/27)
    — the free-list slot-order lead (open-leads 0b), which the
    runner already flags per run (`free-stack fallback: live 342 !=
    scan 564`). Nothing in the pulse's own law is left open.
  - Pinned by `mc2_possession_impact_spawns_the_claim_pulse_entity`
    and `mc2_basic_possession_bolt_rides_the_claim_probe` (world.rs
    lib tests; both verified to FAIL against the neutered arms).
    No golden moved.
  - **NEW LEAD OUT OF THIS DIG: the tier-0 bolt is the wrong
    ENTITY.** — **CLOSED 2026-08-03 by the bolt-launch-lanes dig; see
    the entry at the head of Resolved.** Retail's basic possession is
    `sub_69900` (EF:56039)
    spawning **(9,1)** `SummonManaPosession_4D3B0` (EF:34764 —
    action 1, speed 384/384, `maxLife = 4096/384 = 10`, row 61,
    sprite 209, `xtype = 10`, box ×2 / ×2.5 off the sprite row) and
    only tier `life_0x1A` 1..3 takes (9,17) (EF:55950). The port's
    cast arm (mc2/cast.rs spell 1) always launches (9,17), and
    `CREATORS` has no subtype-1 row at all: full-take l24 shows
    **(9,1) 362 missing / 0 extra**, mc2l4 0+4000 shows (9,1) 96
    missing vs (9,17) 393 extra. sub_69900's own tail is also
    unported — `dword_0x10_16 = 200`, `word_0x26_38` = the token
    slot, `mana_0x90_144` = the TOKEN's mana (recorded 33; the port
    ships the ctor default 50, and `mana` IS a projected lane), z +=
    caster fov, and the head-offset aim
    (`wizext.nextEntity_0x18_24 + yaw`, `entityIndex2_0x1A_26 +
    pitch`) at a 10240-unit designated point.

- **THE l24 m23 DWELLER STEERING RESIDUAL IS A MISSING TERRAIN
  REPLAY, NOT A MOVE-CORE BUG — the (14,1) riser's ENDCAP WALL
  outlives its own lowering, and the conformance import was landing
  retail's whole pool onto PRISTINE heights, so the fence the
  dwellers bounce off did not exist on our side. LANDED 2026-08-03
  (session 8, dweller-steering dig).** Session 7 closed the siphon
  dig with "the entire residual is x/y/heading on two dwellers
  cruising with retail's blocked-status byte[2]&4 toggling, i.e. the
  shared move-core fence reroute" — the right SITE, the wrong cause.
  - **THE RESIDUAL'S SHAPE.** `verify-deltas --csv`, mc2l24
    t=14680..15180: 665 (5,23) rows on three slots (230/363/364),
    211 heading + 208 y + 206 x + 39 z, one `action` row in 500
    pairs. Retail's heading advances in **exact ±85 steps** — and
    85 is not a law constant, it is `341 − 256`: the move core's
    retry-1 yaw offset (EF:8815) minus behavior row 91's turn cap
    `subtype_160_0x2_2 = 256`. Reconstructing the pairs by hand
    (slot 363, import (30962,26728) yaw 566 speed 24 → retail
    (30970,26751) yaw 651) shows retail stepping at yaw **566+341 =
    907** and then turning −256, i.e. **blocked on the first
    prediction and committing on retry 1**, every tick. The port
    stepped at the un-rerouted 566. Both sides carried the same
    `f34` aim, which is why `action` agreed everywhere.
  - **THE MOVE CORE IS CLEAN — do not re-open it.** `sub_1B8C0`
    (EF:8741-8938) sets `byte[2] |= 4` only on the first-prediction
    block (EF:8812) and clears it only on results 1 and 2
    (EF:8917/8933) — never on a successful retry (result 3) and
    never on the boxed-in result 4. `mobs.rs::mc2_move_core`
    matches that latch/clear pattern exactly, and the import already
    homes byte[2]&4 at `F_BLOCKED` (mobs.rs:81). The retry yaws
    match too (retry 2's byte-split `LOBYTE(v37−85) /
    HIBYTE(((v37−341)>>8)&7)` is provably `(v37−341) & 0x7FF` for
    ALL v37, since the two low bytes differ by exactly 256).
    `sub_102D0(_,_,1)` (EF:3632) cannot block an m23 at all outside
    a cave: row 91's `dword_160_0x14_20 = 0xFFFFFFFF` makes
    `~v_20 & water` identically 0. So the ONLY fence is
    `sub_1B7A0_tile_compare(pred) >= v_16 (=20)` — the heightmap
    second-difference, transcribed byte-exact in `Gen::roughness`.
  - **THE FENCE IS A RISER ENDCAP.** Instrumenting the candidate
    predicate: the port's pristine l24 map reads roughness **13** at
    the tile slot 363 tries to enter (121,104) and **9** at slot
    364's (123,104) — under the 20 gate, so the port walks through.
    Take-wide the pristine map has only **42 of 65536 tiles** at
    roughness >= 20: the walker fence essentially does not exist on
    l24's pristine planes. The imported state explains why — l24
    carries **20 (14,1) risers, ALL at `life = 4`** (idle-REMOVED),
    four of them boxing the compound the dwellers patrol (riser 3
    @(122,104) orient 1 len 15, riser 1 @(121,102) orient 0 len 15,
    riser 2 @(133,104), riser 4 @(121,119)). **A lowered riser is
    not a restored map**: the life-0 INSTANT build raises all `L`
    rows of its 2-wide strip by +48 (EF:41492-41513), while both
    animated phases only ever touch rows `3..L-3` (raise
    EF:41938-41955, lower EF:42159-42203) — so **the strip's 3-row
    ENDCAPS stand at +48 for the rest of the level**, no matter how
    often the (10,63)/(10,64) triggers cycle it. Riser 3's endcap is
    cells (122..123, 104..106) — exactly the fence. With the wall
    replayed, roughness at (121,104) reads **95** and at (123,104)
    **103**, and the port's retry chain reproduces retail's
    positions to the unit (slot 364: blocked at 1180 → retry 1 at
    1521 blocked → retry 2 at 839 → (31765,26686) = retail's want).
  - **THE FIX.** `mc2::riser::mc2_riser_reconstruct` (riser.rs) —
    conformance-import only — rebuilds a riser's cumulative terrain
    write from its own imported state (cell, `f71` orientation,
    `f26` length POST-increment, `act_life` phase, `f44` progress):
    life 3 = the instant build; life 4 = build + the full 48-tick
    lower (type restore included); life 2 = build + `48−f44` lower
    ticks; life 1 = build + full lower + `f44` raise ticks (forced,
    since `f44 < 48` is only reachable through a completed lower —
    the build and the raise both park at 48 and a raise trigger on a
    `f44 >= 0x30` riser is a no-op, EF:41934/42133). Junk
    orientations write nothing (EF:41487); the replayed loop-sound
    47 requests are truncated back off. `world/conformance.rs`
    `retail_import_mc2` runs it over every imported (14,1) after the
    pool lands. This is the ledger's own standing remedy for entry 2
    ("or replay the edit events in the importer") in its cheapest
    form: no recording-format change, the terraform is a pure
    function of state the `.mgcr` already carries.
  - **CONFORMANCE.** Windowed A/B (same binary, replay off/on),
    mc2l24 UNEXPLAINED field rows: t=1000+300 **1057 → 148**;
    t=5000+300 **4856 → 7**; t=14680+500 **16625 → 15536**;
    t=25000+300 **5540 → 4407**; t=45000+300 **227 → 189**. In the
    t=14680 window the (5,23) diff rows go **665 → 2** (both
    heading-only, both exactly 512 apart — a residual retry-leg
    disagreement, new lead) and the collateral is all downward:
    (5,21) −975, (5,17) −838, (5,20) −447, (5,26) −245, (11,2) −63,
    (5,18) −50, (5,25) −22, (9,0) −5, (10,0) −1, nothing up.
    FULL TAKE l24 vs the session-7 close: conforming pairs **4 →
    10**, conforming-or-explained **11,635 → 19,136**, UNEXPLAINED
    field **733,635 → 500,845 (−232,790, −31.7%)**, missing 980 →
    975, extra 6,167 → 6,227, rng 12 → 12. (The tree also carried a
    concurrent session-8 dig, so part of the full-take delta is not
    this fix; the windowed A/B pairs above are the isolated
    measurement.) Suites: **0 regressions, 6/6** — mc2l24 2 fixtures
    DRIFTED (both signature shrinks: t=15288 loses
    `field:5,23:{heading,x,y}`, t=3569 loses `field:10,39:z`),
    promotion owed.
  - **SCOPE.** l24 is the only take with (14,1) THINGs (l0/l4/l30
    have zero), and windowed probes on all three are byte-identical
    with the replay on — no cross-take risk. Gameplay is untouched:
    the reconstruction is reachable only from `retail_import_mc2`.
  - Test: `riser_reconstruct_rebuilds_the_endcap_wall_a_lowered_riser_leaves`
    (riser.rs) — lives one riser through build + trigger + 49 lower
    ticks, then demands the reconstruction rebuild that exact map
    from the terminal state alone, asserts the endcaps at +48 and
    the interior back at flank level, and asserts the map is NOT
    pristine. Proved non-vacuous (neutered arm fails on the
    height-plane equality). No golden moved.
  - **NEW LEADS.** ① The same technique should be pointed at the
    other terraform roots the roster currently rules as capture —
    `mc2-guard-terrain` (1.81M rows), `mc2l24-static-terrain-z`
    (376k), `mc2l24-castle-piece-terrain-z` (367k),
    `mc2-walker-ground-z` (250k), `mc2-terraform-houses` (37k): each
    is a deterministic edit whose source entity is in the imported
    pool (castle stamps, the (14,5) plateau, house regrades).
    ② The 2 surviving (5,23) heading rows are exactly 512 apart —
    a retry-3 / retry-2 leg disagreement worth one narrow dig.

- **MC2L24 HYDRA HEAVY-BOLT BARRAGE UNDER-FIRE — the (5,27) BOLT
  POWER @0x88 was never imported, so 4 of every 5 shots no-opped.
  LANDED 2026-08-03 (session 7, hydra dig).** The session-5 dig-D
  NEXT-LEAD ("a portable head-state bug under the capture noise")
  is REAL and is an IMPORT field-home bug, not a state-machine bug.
  - **RETAIL LAW.** The whip is a FIVE-SHOT burst, not one shot.
    `sub_29A90` case 3 (EF:19889-19928): at `actSpeed == 192` the
    branch sets `byte_0x44_68 = 3`, `dword_0x10_16 = 4` and raises
    `v37 = 1`; on each of the next four ticks sub-state 3 raises
    `v37 = 2` and steps `dword_0x10_16` 4→0 (0 → `byte_0x46_70 = 0`,
    counter re-armed to 1). LABEL_94 (EF:20197-20201) calls
    `sub_2A7F0(ix, v35x, v37 == 1)` on all five. `sub_2A7F0`
    (EF:20507-40): the a3=1 shot ROLLS the power
    `manaRegen_0x88_136 = (rand%12 > 7) + 1` (4/12 = power 2) and
    perturbs the branch LCG by `setting_30`; the four a3=0 re-fires
    only READ it back — power 2 spawns `(9,9)` on every one of the
    five, power 1 spawns one `(9,0)` and four no-ops
    (`if (!a3) goto LABEL_13`, EF:20524).
  - **PORT DEFECT.** `m27_branch_bolt` (multipart.rs) keeps the power
    in `f136` — but `import_ent_mc2`'s UNIFORM MC2 map spends f136 on
    `@0x8C` (`f136: r.mana_max`), and `@0x8C` is DEAD 0 on the whole
    (5,27) family. Under per-pair replay every pair therefore re-read
    the power as 0, the four re-fires fell into the `_ => return`
    arm, and each whip laid ONE arc instead of five. The obs
    `mana_max` lane took the same collision from the other side: the
    native low-roll wrote f136 = 1|2 mid-tick and the projection
    reported it where retail reads @0x8C = 0 (30 rows in window 1).
  - **CORPUS PROOF** (mc2l24 t=10650-11150, retail heads 16/26/36/
    46/56 + body 15). 30 whips in the window, 9 of them power-2.
    Per-tick `(9,9)` census, pre-fix: at every mid-burst tick the
    port reproduced the AGED generation (life −2) exactly and missed
    precisely the NEWBORN one (life −1) — e.g. t=10704/05/06/07
    missing 82/82/81/81 with the aged 78×(0,−2)+3×(−1,−2) matching
    row-for-row, and 0 missing on the FIRE tick (t=10703) and on the
    post-burst tick (t=10708). Retail's own churn confirms the
    5-shot burst: arcs born at 10703 (82 nodes) and 10704 (81), then
    new=1/gone=1 per tick through 10707 = a fresh 81-node arc every
    tick landing in the LIFO-freed slots. Family census over the
    whole take (every 37th tick, 87,210 (5,27) rows): `@0x8C` = 0 ×
    87,210; `@0x88` = 0/1/2 (6,876 non-zero) — the lane is free.
  - **FIX** (conformance-only, (5,27)-scoped; native untouched):
    `import_ent_mc2` `f136: if m27 { r.d88 } else { r.mana_max }`
    (conformance.rs:1367) + the (5,27) reverse-map `row.mana_max = 0`
    in `obs_project_mc2` (conformance.rs:977), matching the class-15
    and class-10 precedents. multipart.rs's field-map doc updated.
  - **NUMBERS**, `(9,9)` missing/extra. Window 1 (t=10650-11150):
    **1,530 / 94 → 89 / 175**; whole-window entity sets 1,727/122 →
    258/204; `mana_max` field rows 30 → **0**; rng 0/502 both sides.
    Window 2 (t=43100-43600): **1,686 / 0 → 166 / 0**. Full take on
    the current tree: `(9,9)` **46,241 / 1,967 → 13,632 / 3,052**
    (−71% missing); unexplained field 771,218 → 733,089, unexplained
    missing 1,209 → 1,006, pairs fully explained 11,127 → **11,634**.
    Suites 6/6 green, 0 regressions (the one l24 drift at t=15288 is
    the (5,23) dweller dig's, and improves). Goldens UNMOVED — the
    sim law is untouched, so no state hash changes.
  - **RESIDUAL, ruled capture.** What is left on `(9,9)` is BEAM
    GEOMETRY, not cadence: the arc node positions are a per-node
    `ent_rand` walk off a freshly-spawned beam (5,583 rows now
    classified `mc2-cast-timing-fields`), and the arc LENGTH is
    `steps·8` where `steps` is the beam's terrain walk — on l24 the
    port's terrain z runs high across the level (`mc2l24-static-
    terrain-z` 502/502 pairs in this window), so a beam that walks
    until it meets ground overshoots. The four extra-heavy fire
    ticks (t=10757/10797/10822/10940: retail 34/18/34/18 nodes vs
    port 63/40/57/33) are exactly the whips fired from the highest
    heads. Terrain-closure = the standing capture class; NO new
    roster rule needed (the existing `mc2-cast-timing-*` and
    `mc2-lightning-blast-churn` rules absorb it — window-1
    UNEXPLAINED went 6,807 → 6,810 field / 1 → 1 missing / 0 → 0
    extra).
  - Non-vacuous tests: `mc2_m27_import_field_homes`
    (conformance.rs — the five (5,27) homes with distinct
    sentinels, f136 = 2 not 999) and
    `mc2_m27_refire_needs_the_imported_bolt_power` (world.rs — a
    sub-state-3 branch lays a `(9,9)` at power 2, NOTHING at power 1
    (EF:20524) and nothing at power 0, which IS the pre-fix import).
  - NEXT: the m0/m3/m22 siblings share `sub_2A7F0`'s caller shape
    only through m27; no extension owed. The (10,23) impact family
    (826/5) shares the beam-walk root, not the cadence one.

- **THE (5,23) DWELLER'S MANA SIPHON IS A BALL-SIDE MECHANIC — the
  sphere flies to the collector, and that arm was unported. LANDED
  2026-08-03 (session 7, dig ④).** Player report: "the dwellers hover
  down to a mana ball but the ball is never attracted to them for
  pickup, so they never collect".
  - **RETAIL LAW, DWELLER SIDE** (`sub_27C10` EF:18211-93). The
    siphon does not move anything: it MARKS. Each tick it re-asserts
    `node.byte[0] |= 0x40` and `node.word_0x96_150 = self`
    (:18268-69), bumps its OWN `word_0x2C_44` by 10 (:18270, seeded
    to **18** on the arrival tick :18238 alongside the 64-tick
    `dword_0x10_16`), and swallows when `sub_106C0(self, node)` — the
    3-axis EXTENT overlap, not a radius — or `node.z > self.z`
    (:18271-76). Control flow is a **fall-through**: sub 0 seeds and
    then runs the body in the same tick (only sub ≥ 2 jumps to
    LABEL_24), so the grab, the first +10 and the first swallow test
    all land on the arrival tick.
  - **RETAIL LAW, BALL SIDE** (`TransformArcherToMana_35940`
    EF:26111-72; the (10,57) twin `sub_35FB0` EF:26385-447 is the
    same code). A sphere carrying `byte[0] & 0x40` runs NO physics
    and instead flies to `Entities[word_0x96_150]`, admitting exactly
    two collectors and dropping the grab for anything else
    (:26115-27): the **(3,3) balloon**, z step the constant 32, and
    the **(5,23) dweller**, z step = *the collector's own
    `word_0x2C_44`* (:26120) — i.e. the ramp above, so a siphoned
    sphere accelerates upward 28, 38, 48, … Then: own `word_0x2C_44
    = 128` (the release pop, :26135), yaw at the collector, and on
    the 2-D gap (`EuclideanDistXYZ_58490` sums X and Y only —
    utilities/Maths.cpp:738) either a 16/tick horizontal step
    (≥ 16), or an x/y SNAP plus the z servo into the band
    `[collector.z, +512]` (< 16), ground-clamped; past 1024 the ball
    releases itself (:26169).
  - **CORPUS PROOF** (mc2l24, dweller slot 363 / sphere slot 360).
    t=14512 act 187/0, `f2c` 8192, timer 0, sphere unmarked → t=14513
    act 187/1, `f2c` **28**, timer **63**, sphere `byte[0]` 0x0C →
    **0x4C** with `+150 = 363`: one tick, seed AND body — the
    fall-through. The sphere then walks 16/tick (28330,31292) →
    (28228,31242), snaps to the dweller's x/y at t=14521 and rises
    444 → 542 → 650 → 768 → 896, i.e. **+98, +108, +118, +128** —
    the dweller's `f2c` at each tick, exactly. Swallow at t=14524
    (mana 100 → 2300, sphere `byte[1] |= 4`): the extent test
    `|(1120+280) − (896+70)| = 434 < 384+70` fires that tick and not
    at t=14523 (562). All 14 siphon entries between t=14512 and
    t=15648 enter with `dz ∈ [588, 701]` and a 2-D gap ≤ 121.
  - **FOUR PORT DEFECTS.** ① the ball-side arm existed but admitted
    the balloon ONLY (mc1/combat.rs `ball_tick`) — a grounded sphere
    under a hovering dweller never rose, the swallow could never
    fire, and every siphon burned its 64 ticks into an eternal
    re-hunt: the reported symptom. ② the siphon arm returned on the
    arrival tick instead of falling through, and used
    `mc2_dist3 < 256` for `sub_106C0`. ③ the descend arm
    (`sub_27B20` :18250 → `sub_28390` :18580) handed over on a bare
    2-D 256 with NO altitude condition; retail station-keeps **640
    above the node within ±64** and inside a 2-D reach of **128**,
    runs the mover ONLY outside that reach, and aborts on
    `sub_28060` (:18415, the anti-stack lift) — all three unported.
    ④ the hunt arm re-aimed and range-tested every tick; retail rides
    the `byte_0x3E_62 & 3` cadence (:18140).
  - **THE IMPORTER COLLISION.** m23 is the second model (after the
    m27 hydra) whose machine runs on `word_0x2C_44` while our column
    homes `subSpellIndex_0x2A_42` at f44; the uniform map therefore
    handed every imported dweller a flat **500** where the ramp
    belongs, lifting its sphere 500/tick. `world/conformance.rs` now
    imports @0x2C for (5,23) too; @0x2A has no reader on our side
    (the (9,9) bolt launcher stamps its own payload).
  - **FOOL'S MANA.** `sub_28000` (:18384) and `sub_28420` (:18603)
    filter `model == 39` out of a list that DOES carry the (10,57)
    trap spheres (:40018-63 files models 39/40/57 into
    `dword_38523`), and the balloon fleet's `sub_5F810` (:61005)
    filters the same way — neither ever takes fool's mana. Our native
    m57 keeps the (10,39) family model and carries retail's action
    0x3E, so all three scans now exclude action 62.
  - **CONFORMANCE** (windowed verify-deltas, mc2l24). Pairs
    t=14500..14680 (180): **(5,23) rows 899 → 0, (10,39) rows
    90 → 0**, total diff rows 13880 → 12741, UNEXPLAINED 6251 →
    5648 (baseline = the session-5 CSV, same window). The importer
    fix alone took that window's (5,23)/(10,39) from 191/13 to 0/0.
    Pairs t=14680..15180 (500, ~12 siphon entries): (5,23) 2542 →
    665, (10,39) 309 → 29 (all `mc2l24-ball-terrain-roll`), and the
    siphon-dense half t=14680..15000 is **completely clean** — the
    entire residual is x/y/heading on two dwellers cruising in
    185/0 with retail's blocked-status byte[2]&4 toggling, i.e. the
    shared move-core fence reroute, not the siphon. OPEN LEAD.
  - Tests: `leviathan_siphon_lifts_a_grounded_sphere_and_swallows_it`
    (proved non-vacuous — without the collector admission the sphere
    never leaves the ground), `leviathan_stations_640_above_its_node_before_siphoning`
    (end to end from the ctor cruise altitude),
    `leviathan_never_siphons_a_fools_mana_sphere` (also non-vacuous).
    No golden moved.

- **THE l24 VISSULUTH ENDGAME — the demon is the (5,10) DOOMSDAY
  PYRAMID, and the port was drawing it through both invisible phases
  and running its death animation on the wrong clock. LANDED
  2026-08-03 (session 7, dig ③).** Player report: the demon sprite
  pops in the instant the pyramid is destroyed and then stands idle
  and indestructible until you fly into it; its death animation
  loops (keel over, get up, keel over) until the growing mana
  fountain buries it.
  **PLAYTEST 2026-08-04 (partial): keel-over/death CERTIFIED —
  "now OK, doesn't loop" (the one-shot anim-timer fix confirmed).**
  **RETAIL RE-VERIFY DONE (player, 2026-08-04) — the "second
  visibility gate" hypothesis is DEAD.** Retail DOES draw the demon
  body through the wait phase, exactly as the port does — but
  **scaled down TINY**, easily overlooked through the smoke; on slow
  approach it scales UP, becomes big and starts attacking BEFORE the
  narration stage trigger (attack trigger = the 0xA00 proximity
  activation, separate from the StageVar — both confirmed live).
  **⚠ THE "BLEND-ONLY, FULL SIZE" READING BELOW WAS REFUTED BY PLAYER
  FOOTAGE (2026-08-04). THE SIZE LAW IS REAL — see "THE VISSULUTH SIZE
  LAW" further down. Both effects are retail and simultaneous; the
  blend paragraphs stand, only the "there is NO size term" claim
  fell.**
  **⭐ PLAYTEST CERTIFIED (player, 2026-08-04, same day): "Confirmed
  through playthrough. The shrinking is now faithful." — the size law
  (20× wait-phase shrink + meter-driven growth ramp, with the
  render-side smooth-lerp deviation) is CLOSED. Remaining owed on this
  story: the 3-D wake-gate distance nit (doomsday.rs, blocked on the
  fountain dig's file ownership).**
  **WAIT-PHASE RASTER MODE TRACED + LANDED 2026-08-04 (session 8).**
  The suspect bit does mean translucency:
  - `DrawSprites_3E360` GRO:3779-3806: `byte[2] & 0x80` (flags bit
    23) takes the flag-override arm and, absent the player-colour
    bits, forces `str_F2C20ar.dword0x01_rotIdx = 2` (GRO:3805).
    Mode 2's inner loop (GRO:4525-4562, index at 4546/4559) is
    `if (src) *dst = T[0x4000 + (src<<8) | *dst]` — the TABLES
    256×256 blend matrix, empirically `nearest_palette(⅓·row +
    ⅔·col)`, so the sprite contributes ⅓ and the background ⅔:
    **33%-opaque, full size** (docs/traces/mc2-transparency-
    drawlist.md §4-§5).
  - There is no size term on the FLAG (this part still holds, and it
    is what sent the first pass wrong). Sprite height is
    `dword0x18 * particlesParameters_D951C[word_0x5A_90].rotSpeed_8
    / depth` (GRO:3770-3772) and nothing in `DrawSprite_41BD3` is
    keyed on `rotIdx` except the per-pixel writer — the mirroring/edge
    setup that owns `realWidth`/`realHeight` (GRO:4038-4400) is shared
    by all nine modes, and modes 0 and 2 step the source identically
    (`v53`/`v70` +2 dwords per pixel, GRO:4432-4490 vs 4526-4562), so
    mode 2 neither decimates nor blits raw. **The size instead moves
    because retail REWRITES THE TABLE — see below.**
  - Corroboration that mode 2 = the faint end of a fade RAMP: the
    generic effect tail at EF:26290-303 walks a dying entity
    opaque → `byte[3] |= 1` (mode 3, 67%) at life 12 → `byte[2] |=
    0x80` (mode 2, 33%) at life ≤ 6 → `DisableEntityDrawing` at 0.
  - **THE PORT BUG was a bad carve-out, not a missing law.**
    `World::live_poses_mc2` already mapped bit 23 → blend 2, but
    excluded `(5,10)` on the theory that the boss draws through
    `sub_3FD60` (GRO:2205-12), whose rotIdx comes from the static
    descriptor alone. That theory is WRONG: `sub_3FD60`'s only two
    call sites (GRO:1260, GRO:1327) sit inside the
    `m_Graphics.m_wReflections` block opened at GRO:1104 — it is the
    WATER-MIRROR pass. Every main-world per-tile billboard call is
    `DrawSprites_3E360` (GRO:900/1026/1775/1841), which reads bit 23.
    FIX: carve-out deleted (`engine/world.rs` `live_poses_mc2`), so
    the boss exports `blend = 2` from hide-clear (t=51645) until the
    0xA00 wake (t=51732) and `blend = 0` after. Scope is the plain
    bit-23 rule with no exception — nothing else changed lane.
  - Corpus confirmation of the lane the pose export reads (slot 7,
    `dump-state`): t=51650 and t=51731 flags `0x4880000C` (bit 23
    SET, hide bit clear, f2a `0x50` = the proximity-watch arm) →
    t=51733 flags `0x4800000C`, f2a `0x60` (doom-meter ramp). The
    native ctor stamps the same bit (`mc2::doomsday` `flags |=
    0x4880_0001`) and the same clear (`flags &= !(1 << 23)`), so
    replay and native runs read one storage.
  - Render side needed NO change: `LivePose.blend` → `Billboard.blend`
    → alpha ⅓ on `billboard_blend_pipeline` (mgc-render) was already
    wired for smoke. Presentation-only: `observable_digest` hashes
    only pose `type_index/x/z`, never `blend`, and `state_hash` never
    sees the export — `MGC_REQUIRE_GOLDENS=1 cargo test -p mgc-sim`
    green, no golden moved.
  - **★ THE VISSULUTH SIZE LAW — A SELF-MODIFYING SPRITE-PARAM ROW.
    TRACED + LANDED 2026-08-04 (session 8), after player retail
    FOOTAGE refuted the blend-only reading.** Player's two frames:
    (1) wait — the boss is a tiny handful of pixels at the base of the
    smoke column, gone entirely in lowres; (2) closing — it enlarges
    GRADUALLY and TICK-STEPPED ("several mid-step images getting
    bigger and bigger but nonetheless separate images") into the full
    demon bust, and the smoke stops. No entity lane carries it (corpus
    slot 7: box extents identical 1024/1024/1280 at t=51700 and
    t=51780; f5a 341→343; rows 341-345 all authored `rotSpeed_8`
    0x4B0). **The boss rewrites its own row in the STATIC TABLE.**
    Both writes decompile as stores into `x_BYTE_D9F50`, which is a
    MIS-SPLIT ALIAS of `particlesParameters_D951C`:
    `0xD9F50 − 0xD951C = 0xA34`, and remc2 itself flagged the symbol
    ("`x_BYTE_D9F50 - ? used only byte 0x87A,0x5b6,0x126 (error?)`",
    EventsFunctions.cpp:92) while its own data dump at EF:2422-24
    carries the D951C row constants `0x212C` / `0x002A` verbatim.
    Address arithmetic (14-byte rows, height at row offset 8):
    `&x_BYTE_D9F50[0x87a]` = **0xDA7CA** = row 341 (starts 0xDA7C2)
    `+8` = **`D951C[341].rotSpeed_8`** — the demon's own draw height.
    The decompile even leaves the breadcrumb `// DA7CA: using guessed
    type __int16 x_WORD_DA7CA;` at EF:12883/13097. The other two
    aliased offsets resolve the same way (0x5b6 → row 291 `word_0`,
    0x126 → row 207 `rotSpeed_8`), so the pattern is general.
    - **EF:12700** (state 0, the ritual start): row 341 height := 60.
      Against the authored 1200 that is a **20× linear shrink** —
      0.23 tiles, a few pixels, invisible at 320×200. This is the
      wait-phase floor, and it holds for the whole dormancy.
    - **EF:13041** (the `f44 & 0x20` doom-meter arm, armed the tick
      the 0xA00 proximity clears bit 23): row 341 height := the meter,
      which steps **+30/tick from 30 to 1200 over 40 ticks** — the
      filmed stepped growth. Corpus: scratch10 = 60 @ t=51733, 420 @
      t=51745, meter capped 1200 @ t=51772.
    - The ramp ENDS on the authored 1200 and never writes again (the
      meter's later reuse as a state timer — scratch10 = 3 @ t=51780 —
      lives in a different arm), so the attack rows 342-345 (never
      patched) and the post-attack idle back on row 341 all draw full
      size. That is why the demon does not re-shrink between attacks.
    - **PORT.** `LivePose.sprite_h_units: Option<f32>` (new,
      presentation-only) carries the patched row height; `live_poses`
      exports it for MC2 poses with `type_index == 341` from
      `World::mc2_doom_meter`, which is ALREADY the port's mirror of
      that field (`mc2::doomsday` writes 60 at state 0 and the meter
      each ramp tick — the module header already named it
      `x_BYTE_D9F50[0x87a]`, it was just never wired to the draw).
      `mgc-app::entities::billboards_from_poses` uses it in place of
      the baked `rot_speed_8`; the renderer re-derives width from the
      frame aspect exactly as retail does, so one field is the whole
      law. `mc2_doom_meter == 0` = never patched ⇒ the authored row
      stands. Hash-quiet: nothing new is hashed, `observable_digest`
      hashes only pose `type_index/x/z`, `mc2_doom_meter` was already
      sim state. `MGC_REQUIRE_GOLDENS=1 cargo test -p mgc-sim` green
      (340/340 lib + every suite), no golden moved.
    - **RENDER SMOOTHING — deliberate presentation deviation
      (player-requested).** Retail steps the size once per SIM TICK
      (the player can see the discrete images). The port lerps
      `sprite_h_units` on the same frame alpha as the transforms in
      `mgc-app::entities::lerp_poses`, so the growth is continuous at
      display rate. The sim law itself is untouched: +30/tick,
      exactly retail. Render-path only, no DEVIATIONS.md sim entry.
  - Tests (crates/mgc-sim/tests/mc2_slice.rs):
    `mc2_doomsday_is_tiny_until_the_proximity_wake_then_grows_to_full_size`
    — asserts (blend 2, height 60) through the wait, then a monotone
    ramp with mid-growth samples settling exactly on 1200; and
    `mc2_doomsday_growth_ramp_exports_lerpable_size_steps` — the tick
    pair differs by exactly +30 so a 0.5 alpha lands mid-step. Both
    proved non-vacuous (neutering the export fails them:
    `left: Some((2, None)), right: Some((2, Some(60.0)))`); the blend
    leg is separately non-vacuous (restoring the `(5,10)` carve-out
    fails it with `left: Some(0), right: Some(2)`).
  - **SMOKE-STOP: ALREADY FAITHFUL, nothing owed.** The wait-phase
    smoke is the (10,14) falling-rock ring the machine spawns every
    tick. Retail gates it on `v27`, cleared by
    `if (dword_0x10_16 >= 600) v27 = 0` inside the ramp arm
    (EF:13029) — i.e. the ring stops HALFWAY up the growth
    (meter 600 ≈ t=51752), not at ramp start. The port matches
    verbatim: `suppress_ring` at doomsday.rs:677 under the same
    `f26 >= 600` test, consumed at doomsday.rs:689. READ-ONLY check,
    nothing landed in doomsday.rs.
  - **ATTACK GATE RE-CHECKED, FAITHFUL.** `mc2_pyramid_attack`
    (doomsday.rs:650-673) arms only on `f44 & 0x10` + `f44 & 0x40`
    and the 0xA00 squared-distance test, then the caller flips state
    1 → 4 with `f44 |= 0x80` (doomsday.rs:337-340). No StageVar /
    narration row is consulted anywhere in the escalation. One nit
    left for the doomsday.rs owner: retail's gate is
    `EuclideanDistXYZ_58490` (3-D, EF:13010) while the port compares
    `dx²+dy²` only — a plan-distance approximation that ignores the
    player's altitude over the crater.
  - **WHAT THE DEMON IS.** l24's THING table: slot 373 = a (10,45)
    BUILDING (BLDGPRM id 68) at (40,212), dis-gated 28, `child` 29 =
    its ON-DEATH disposition; slot 379 = the (5,10) doomsday machine
    at (40,213), dis 29. So destroying the pyramid *building* fires
    dis 29 and spawns the boss — corpus t=51557 (slot 7, act 80,
    life 300000/300000). The "reach the centre" goal is a separate
    STAGE row (stages.json checkpoint `index 5, stage 0` at
    (40,212), objective kind 5 fly-to-point, `World::mc2_objectives`
    :40803-14) and the kill goal is `index 1, stage 379` (kind 1,
    bound to the boss THING) — both already ported, neither gates
    the spawn. The mana "fountain" is the state-0xF (10,9)
    APOCALYPSE dome → its life-3 (10,91) mana rain (t=63308), which
    RAISES the land over the corpse — hence "buries it".
  - **RETAIL DORMANCY LAW.** The ctor's `|= 0x48800001` (EF:33980)
    carries **byte[0] bit 0 = the billboard hide bit**: the MC2
    sprite pass skips `byte[0] & 0x21` (GameRenderOriginal.cpp:3157
    `DrawSprites_3E360`, mirrored NG:2838/HD:3235, plus the
    sub_3FD60 gather at GRO:1936). The boss is therefore INVISIBLE
    through its opening ritual (crater flatten + the 70-tick
    kill-all) and the kill-all exit clears the bit (EF:12983) —
    corpus slot 7: flags `0x4880000d` at t=51557 → `0x4880000c` at
    t=51645 (88 ticks). It then waits, still un-damageable (state 1
    never calls `sub_22190`), until the player closes inside 0xA00 =
    10 tiles, drops the ctor's raster-mode bit (`byte[2] &= 0x7F`,
    EF:13024 — corpus `0x4880000c` → `0x4800000c` at t=51732) and
    ramps the doom meter into the attack cycle at t=51772. **PORT
    BUG**: `live_poses` only honoured bit 5 (0x20), so the boss was
    billboarded from spawn. FIX: `World::live_poses_mc2` now skips
    `flags & 1` for the **class-5** column — the doomsday machine is
    the only MC2 class-5 ctor that writes bit 0, so the widening is
    provably scoped (see DEVIATIONS.md's multipart entry for why it
    is NOT global). `mc2::doomsday` also lands the missing
    `byte[2] &= 0x7F`.
  - **DEATH ANIMATION = ONE CYCLE.** `sub_221F0` (EF:13667-72): for
    sprites 343/344/345 (0x157..0x159) the state timer is re-seeded
    from the TMAPS animation's `CountOfFrames_16`, so those states
    last exactly one animation cycle; the cases' own seeds
    (16/16/32) are pre-override values. The sim carries no frame
    table, so the counts are PINNED FROM THE CORPUS (slot 7 b46
    dwell): **343 → 5** (states 6+7, t=51778..51782 and 3 more
    cycles), **344 → 15** (0xA+0xB, 51793..51807), **345 → 20**
    (state 0xE, 63201..63220). State 0xD is 32 and 0xF is 60 —
    both already right (63169..63200, 63221..63280, despawn 63281).
    With 0xE at 32 the death animation over-ran its cycle AND the
    port kept drawing the corpse through 0xF (retail re-sets the
    hide bit at the end of 0xE, EF:12846) — 92 visible ticks of a
    globally-looping FLC instead of 20. Both halves fixed.
  - **CONFORMANCE.** Windowed verify, pairs t=51500..52699 (1200):
    explained pairs **65 → 137**, UNEXPLAINED rows **8494 field →
    4536** (missing 12 → 14, extra 228 → 232, rng 1/1200 both).
    Gross families all down (x 7875→6021, y 7602→5744, heading
    2761→2388, action 3157→3129, model 584→566, class 478→464, life
    946→899). The timer pin is what moved it: with the wrong dwell
    the port's summon/ring cadence and per-tick LCG draws sat one
    phase off retail for the whole fight.
  - **SUMMON LIFE-LATCH FIELD HOME (report C, partial).** The
    pyramid's summon block stamps `word_0x2E_46 = 250` (EF:13419);
    the port wrote it to **f46**, which on a creature is
    `fontTypeIndex_0x3D_61` — for the selector-3 (5,0) worm that is
    the projectile-DODGE alert window (`m0_dodge`), so every worm
    summon was born with 250 ticks of phantom dodging armed, and the
    latch had no import home at all (the class-5 @0x2E lane is
    **f26**), so a replayed summon read ≈0 and puffed itself on its
    first ticked pair. Moved to f26 in `mc2::doomsday` +
    `mc2::mobs::mc2_doom_summon_{home,spinup}_tick`, with a
    conformance import arm so a StageVar2 16/17 (5,0)/(5,27) summon
    keeps @0x2E instead of the m0 bob velocity.
  - **~~STILL OPEN~~ → SUPERSEDED 2026-08-05. The earlier "the
    standing husk is RETAIL LAW" reading was HALF the law and the
    half it missed is the whole bug — see the next entry.** The
    latch write is real (`sub_1E700` v2==2 → `word_0x2E_46 = 1`,
    EF:10864-66; `sub_1E580`'s head only zeroes the latch when the
    PARENT pyramid is gone, EF:10699-10701), but retail NEVER
    STRANDS on it, because the caller runs an escape hatch after
    the core returns that the port `return`ed past — **FIXED and
    landed the same day** (see "VISSULUTH'S SUMMONS — THE 'FROZEN
    HUSK FOREVER' IS A PORT BUG"). Do not cite the old paragraph.
  - **NOT A BUG: the l24 "fountain sphere model 54".** The (10,54)
    rows at (41,212) z 2845..3600 over t=63971..68788 are NOT a
    wrong fountain model — they are `AddAuxiliary_50500` (EF:36812)
    MANA-MAGNET AURAS from the player's own possession casts on the
    raining spheres (life 128/128, act 59, speed 256, applied_yaw 0
    / applied_pitch 1024 = the ctor's ShiftRot(1024, 0x4000), mana
    0 — an exact ctor fingerprint), spawned at each possess impact
    point above the fountain. The fountain itself rains (10,39):
    `sub_32CF0` (EF:24030) calls `_4A190(&pos, 10, 39)` verbatim and
    the port matches. The slot skew is the port's missing (10,12)
    claim-pulse ENTITY — retail spawns BOTH the pulse and the aura
    per magnet impact (EF:59036-59054) while the port only runs the
    `area_write` — which shifts every subsequent slot by one (the
    4 `want=12 got=54` rows). Ported pulse entity = a separate
    lead.

- **THE l24 START SPHERES ARE ALL FOOL'S MANA AND THE PORT HANDED
  THEM OVER — the (10,57) claim intake was gated on a CAST-DECOY
  marker retail does not have — LANDED 2026-08-03 (session 7, dig
  ②).** Player report: on mc2l24 every mana ball on the ground at
  level start is fool's mana in retail — possess one and it fires
  back; the port let you collect them.
  **PLAYTEST FOLLOW-UP (same day): "fires the trap fireball in the
  wrong direction — not at the player" — FIXED.** `mc2_fools_bolt`
  only aimed at POOL-entity claimers; the human sentinel fell back
  to the sphere's stale launch heading (junk for an authored ground
  sphere) on the assumption the flyer autoaim would re-acquire.
  Retail `sub_36770` aims `sub_655C0` at the CLAIMER entity — the
  human included (retail humans are in-pool). Fix: ctx threaded
  through `mc2_fools_retaliate`/`mc2_fools_bolt` (cast.rs) and the
  human claimer resolved via the ctx pose exactly as every creature
  attack aim does (`Gen::mc2_target` convention). Non-vacuous lib
  test `fools_trap_fireball_aims_at_the_human_claimer` (world.rs;
  neutered arm leaves at yaw 5 vs expected 295 → fails). NOTE
  found while testing: a GROUND-level muzzle detonates the bolt on
  its first step — that is OPEN-5's deferred `+fov` launch lift
  (the l24 bait hangs at z 1280..3840, so live traps fire fine). **LAW.** (10,57) is retail's
  RANDOM-VALUE sphere: ctor `sub_50130` (EF:36631) stamps action
  **0x3E**, whose handler `sub_35FB0` (EF:26318, strA0 row 62) is
  the (10,39) ball's twin EXCEPT in the claim intake. The ball
  transfers ownership + chimes sound 4
  (`TransformArcherToMana_35940` EF:26069-94); the m57 instead runs
  `else if (word_0x68_104 && sub_36680(a1x)) { _4A190(&pos,10,0);
  DisableEntityDrawing04(a1x); }` (EF:26362-66) — the FOOL'S-MANA
  trap. `sub_36680` (EF:26615) has **no owner precondition**: its
  only skip is `parentId == claimer` (EF:26623), so a sphere with
  the NewEvent defaults (`parentId` 0, `byte_0x46_70` 0) is a live
  TIER-0 trap for everyone → one homing `(9,0)` fireball
  (`sub_36770` EF:26672, `word_0x96_150` = claimer, sound 9),
  `sub_6D8B0(parentId,22,1)`, consume. **CORPUS PROOF (this is what
  resolved the audit doc's OPEN-2).** t=0 census: 21 authored
  (10,57), slots 67-87, all `own=0 pe=0 act=62 flags=0x2000c`, raw
  **b46=0, owner28=0, f2a=100**. All 21 die in t=0..1836 and **every
  one dies by the trap, none by damage or collection**: the tick
  before each death the ch1 mail SOURCE (= `word_0x68_104`) flips to
  116 = the human, written by a co-located (10,12) possess pulse
  (`PossesHitMana_320E0` → `sub_112D0` EF:4199); the next state has
  the sphere at `flags |= 0x400` with life still 300/300, a **(10,0)
  poof at its exact position** and a **(9,0) fireball with tgt=116**.
  Slot→last-m57-tick→poof/fireball: 67→1322→569/589 ·
  68→1355→489/589 · 69→1358→622/402 · 70→1422→589/75 ·
  71→1402→75/622 · 72→406→539/627 · 73→854→524,618 · 74→294→524/599
  · 75→786→430/524 · 76→1132→432/620 · 77→998→145/144 ·
  78→1452→75,179 · 79→1531→228/271 · 80→1649→280,340 ·
  81→1718→326/342 · 82→1700→345/363 · 83→1693→161/285 ·
  84→1573→155,322 · 85→1515→310/363 · 86→1835→469/478 ·
  87→959→609/73. **PORT BUG.** `ball_tick`'s fool arm gated on
  `is_fool = Mc2 && f52 != 0` (mc1/combat.rs) — a marker only the
  spell-22 cast wrote — so authored spheres (f52 = 0) fell through
  to the ownership-transfer arm: `f144 = 116`, sound 4, sphere kept.
  Worse, the whole round-1 trap used PORT-PRIVATE lanes the importer
  never feeds (f50 tier, f136 payload, f146 claimer, f56 counter, f52
  owner) — `f136` is the observed `mana_max` lane and `f146` is the
  balloon-tether target. **FIX.** (a) gate = the (10,57) identity
  `model65 == 57 || tick70 == 62`; (b) every trap lane re-homed onto
  the RETAIL field the importer already carries — parentId `id24`
  (@0x28), tier `f71` (@0x46), payload `f44` (@0x2A), counter `f26`
  (@0x10), and the claim latch IS `mail[1].1` (@0x68), never cleared
  except on the owner arm; (c) the missing **(10,0) consume poof**
  (`mc2_spawn_fire`) + `flags |= 0x400` soft kill, and a claimed
  sphere runs no physics that tick (retail's `else if`); (d) tier > 3
  = no trap, no transfer, latched forever (EF:26665); (e) NATIVE arm:
  `mc2_spawn_mana_sphere(57, …)` now stamps `tick70 = 62`
  (`sub_50130`'s action) so the l24 authored spheres are trap-armed
  in real play — this is a gameplay fix, not only a conformance one.
  World-mana census now excludes CAST decoys only (action-62 with a
  real caster in `id24`); authored spheres count exactly as before.
  **NUMBERS (windowed t=0..2200, the whole life of the 21 spheres;
  before/after measured 20 min apart with only this change between).**
  UNEXPLAINED rows **8,007 → 7,963 (−44)**, and the entire delta is
  the (10,57) family: **227 → 183** (`player_ent_idx` 23 → **0**, x
  80→71, y 67→60, z 57→52); every other family byte-identical (5,19
  6935, 10,16 463, 10,17 168, 10,18 113, 10,19 65, 3,3 13, 10,42 13).
  Entity sets in-window: missing **2,086 → 2,046**, extra 487 → 491;
  **(9,0) missing 12 → 3**, (10,0) missing 284 → 270, (10,12) missing
  119 → 117, (9,1) missing 17 → 14, `mc2-cast-timing-missing` at the
  21 spring pairs **10 → 1**. Every newly-spawned (9,0)/(10,0) row is
  absorbed by the existing `mc2-cast-timing-fields` /
  `mc2-fire-churn-m0` rules — **zero new unexplained rows**. Whole
  take (l24, after): 69,207 pairs, 11,135 explained, unexplained
  773,459 field / 1,074 missing / 6,085 extra, **(10,57) 0 missing /
  0 extra** — identical to the figures dig ① banked, which were
  measured with this change already in the shared tree, so the
  whole-take attribution lives in the windowed run above. All six
  fixture suites 0 regressions / 0 drifted; mgc-sim 313 + 28 green
  (one new test), MC1 goldens unmoved (`level_005_golden_state_hashes`,
  `flight_tier_golden_state_hashes` under `MGC_REQUIRE_GOLDENS=1`).
  Test `mc2_authored_ground_sphere_is_a_tier0_trap` is two-sided and
  non-vacuity-proven (restore the `f52 != 0` gate → 0 fireballs).
  RESIDUE: the 183 remaining (10,57) rows are the sphere SETTLE
  PHYSICS (x/y/z drift on the authored spheres' first ~300 ticks),
  a different machine; and two DEFERRED items recorded in
  docs/spell-audit/fools-mana.md §6 — the bolt's `+= array_0x52_82.fov`
  launch lift (OPEN-5, ~42 units; our victim probe admits the
  launcher sphere) and the port's `model65 = 39` on natively-spawned
  spheres (OPEN-6; the action lane carries the law, the model residual
  would need a sweep of every `model65 == 39` sphere gate).

- **MC2 SWITCH VOLUMES LOST THE HUMAN'S OWN HALF-EXTENT — the
  (10,25)+(10,75) "unported doomsday spawns" were a SWITCH-BOX
  MISS — LANDED 2026-08-03 (session 7, dig ①).** The paired lead
  was misattributed: neither family belongs to the doomsday
  pyramid. On l24 every one of the seven bursts is a **(11,2)
  repeating enter-switch** releasing its disposition — a storm of
  `AddWind_4F040` whirlwinds (each 1 head (10,22) + 11 (10,75)
  funnel nodes) plus a scatter of `sub_4F6A0` (10,25) area blasts
  at authored tile centres (corpus proof: at t=34041 slots
  8/122/135 are three fresh heads maxLife 40 = 8×tier-1 charge,
  `word_0x2A_42`=100, plus 8 blasts maxLife 8 / subSpell 2000 /
  action 25 — and switch slot 93 at (144.5,109.5) steps
  `dword_0x10_16` 0→10, the 10-count rearm, on exactly that tick).
  **LAW.** `sub_6F0B0` (:54408) → `InitSwitchChainZaxisAndSound_
  6F850` (:44523) walks the wizard list `dword_38519` for a
  class-3 **model-0** entity (AI wizards are model 1,
  `sub_4A9C0` — the port's human-only probe IS faithful) and tests
  `CompareAxisWithShift_10750` → `_106F0` (:3726), which SUMS BOTH
  boxes: `|dx| < a.pitch + b.pitch`. The human carpet's own
  half-extent is `particlesParameters_D951C[44].speed_6 / 2`
  (`AddPlayer_4A920` :33317 → `SetEntityIndexAndRot_49CD0`
  :32841). **PORT BUG.** Row 44 AUTHORS `speed_6 = 0` — retail
  fills it in at BOOT from the TMAPS geometry
  (`speed_6 = width * rotSpeed_8 / height`, the table pass at
  EF:44898-903), giving 242 → half-extent **121** (verified in the
  corpus: the l24 and l30 human entities both carry
  `apitch=aroll=121, afov=ayaw=100` from t=0). `mc2_switch_overlap`
  read the RAW static row (0) and shrank every MC2 switch volume
  by 121 units, so l24's marginal trips never happened: at t=34041
  the human sits 1588 from switch 93, inside 1536+121 but outside
  1536. FIX = `world.rs:7632` `pw = self.g.mc2_params_ext(44).0/2`
  (the derived table the port already builds,
  `mc2::derive_sprite_extents`). Native + conformance; no
  deviation involved. **HARNESS TWIN (needed, else the fix
  regresses other takes):** a one-shot disposition ZEROES the
  records it releases (`sub_4A1E0(id,1)`) and that consumption is
  NOT in the captured `D41A0_0` closure, so it could not be
  re-imported per pair — one mis-timed trip disarmed the
  disposition for the whole rest of the run (l30: the port tripped
  the (11,0) at (201.5,204.5) at t=3234, one phase period early
  under the `--pin-pose n1` sample, and the real t=3242 release
  then spawned NOTHING). Added `World::thing_table_clone` /
  `restore_thing_table` (conformance.rs, opaque `ThingTable`) and
  re-imprint it per pair in `exec_pair_mc2` next to
  `restore_planes` — the same modelling choice already made for
  terrain. **NUMBERS (whole take, unexplained).** l24 family:
  (10,25) **37 missing/0 extra → 7/0**, (10,75) **110/13 → 13/14**,
  (10,22) **10/0 → 2/1** — 109 of the newly-spawned rows now pair
  off as `slot-desync`. l24 totals: missing **1,209 → 1,074**,
  pairs fully explained **11,127 → 11,135**, extra 6,067→6,085,
  field 771,218→773,459 (the port now CREATES 133 entities it
  never did — they land on desynced free-list slots, which is what
  the field/extra ticks buy). l30 missing 126→124, explained
  7,018→7,023; mc2l0 missing 97→80, explained 6,700→6,692; mc2l4
  missing 199 (unchanged). All six fixture suites: **0
  regressions, 0 drifted** (no re-promote needed); mgc-sim 313+
  green, goldens unmoved. Test
  `mc2_switch_box_sums_the_human_carpet_half_extent` (world.rs) is
  two-sided and non-vacuity-proven (pw=0 → fails). RESIDUE (22
  rows): t=13338/13378/13379 (5 (10,25) missing + 9 (10,75) extra)
  = free-list ordering inside one already-firing burst; 1-2 rows
  per burst at t=34041/34324/48007 = the same; t=57223 is a
  DIFFERENT machine — a projectile-impact whirlwind (`sub_678E0`,
  maxLife 24, subSpell 20, `id_0x1A`=7 stamped by the impact tail
  EF:63183) from a non-human caster, i.e. the `mc2-cast-timing-*`
  family, not a switch.

- **MULTIPART FLYER Z-BOB RULED CAPTURE — the "untraced M0/M3
  altitude source" DOES NOT EXIST — 2026-08-03 (session 7, scout;
  read-only, no code changes; roster mc2-flyer-drift-m0/m3 flipped
  open→capture).** Six windowed l4 re-measures post-session-6
  prove the family unchanged (the dig-A servo commits touch
  castle.rs only; the multipart servo `mc2_alt_core` mobs.rs:169
  was already the retail 2-branch shape). Mechanism, decompile-
  pinned: MC2 creature move core `sub_1B8C0` (EF:8741) calls the
  `sub_580E0` servo at :8804 with row args and `MoveEntity` at
  :8805 with **pitch literal 0** — multiparts never fly along
  pitch; m3 row 74 has v_12=0 so the head z ≡ ground_z every tick
  (measured: retail free-descent branch 0% over 6,728 rows —
  BOTH sides clamp); m3 has NO bob state (multipart.rs:548-565
  bare wrappers); m0's bob `sub_1F040` (EF:11233-55) is ported
  byte-equivalent incl. the `ground+256` bounce gate (its 1-4-tick
  z bursts = the bounce firing a tick apart across the terrain
  gap). Family decomposition (t=8000 window, (5,3) 7,077 rows):
  heads med|off| 259 vs segments 14 (rigid `sub_1B6B0` follow —
  all signal in the head); 92-94% of z rows carry a same-tick x/y
  diff with |dz| monotone in |dpos| = wander capture drift
  SAMPLING terrain; the byte-identical-position residue (444
  rows, 423 = the four heads) is the pristine-plane terrain datum
  gap (retail z pinned at 0/2624 for 40+ ticks while the port
  tracks its own heightmap; deltas −23..+8 height bytes; probe
  validated by re-deriving the (5,15) +256 castle-pad raise,
  16,365/17,500 rows). REMAINING from the scout: the l4 terrain
  datum sizing (437 tiles) = recording-format-v2 terrain channel /
  import work, and the untouched t=17954 mass spawn-wave lead. The
  near-universal (3,3) z family (66,845 of 67,391 pairs, the reason
  l24 had zero raw-conforming pairs) clustered position-independent
  → not terrain. ① IMPORT: `mc2_balloon_tick` is the ONE MC2 tick
  that indexes its servo row RELATIVE to `ROW_BASE`
  (`BEHAVIOR[ROW_BASE + row156]`; native spawn sets row156=9 → abs
  68). Retail's ctor `sub_4ABA0` pins `&str_D7BD6[68]` (EF:33422),
  so the generic import produced row156=68 and the tick read
  `BEHAVIOR[127]` — v14=−128 — sinking every imported balloon
  128/tick (= the whole original histogram: floor +128, climb
  +258/+353, descent +112). Fixed conformance-import-scoped,
  (3,3)-only row rebase (conformance.rs:558-579). ② NATIVE LAW: the
  port reused MC1's 3-branch `alt_clamp` (25%·v14 through the band);
  retail MC2 uses `sub_580E0` (EF:40372) — 2-branch, `z>ground →
  z+=v14; floor at ground+v12`, ceiling arg DEAD → open-sky descent
  −4 vs retail −16 = the −12 residual. Fixed both branches
  (castle.rs:910-924), decompile-proven; ZERO goldens moved.
  Numbers: balloon-z rows 163,717→52,517 (−68%), afflicted pairs
  66,845→31,430; windows mid(t20k) −84%, late(t58k) −95%; **l24
  raw-conforming 0→4 (first ever)**; rng untouched. Residual 52.5k
  = balloon DOCKED over the terraformed castle pad (retail floor
  pad≈1536+512=2048; pristine replay descends one servo step) —
  capture, roster `mc2-balloon-z` flipped open→capture with the fix
  provenance. Non-vacuous test
  `mc2_balloon_servo_descends_full_v14_in_band`. EF: sub_4ABA0
  :33422, sub_580E0 :40372, AddBallon_60AB0 :61857-61, sub_60D50
  :61933-35. **PLAYTEST OWED (native hover rate now retail).**
- **MC2 (10,79) "ENDGAME MOVER" = the CASTLE DEFENDER PIECE, eleven
  import mis-homes — LANDED 2026-08-03 (session 6, dig B).** Ctor
  `sub_508E0` (EF:36987), tick `sub_3AF00` (EF:30106): max_life
  100000, action 0x56, sprite 66; 4 pieces per castle upgrade in a
  2×2 grid, three cohorts on l24 (t=15288..69273, ~8-12 live). The
  port already had the FULL machine (mc2/castle.rs) — the ~730k-row
  family was pure import: the piece is minted with a fresh layout
  and the uniform alias table mis-read ELEVEN homes. Killer: recoil
  `f68 ← @0x43` (part-type, nonzero) instead of @0x44 → every
  imported piece re-applied a 115-unit launch displacement per pair
  (slot 619 t=30000: y retail 173.0 vs port 173.449 = 115/256
  exact) = the entire y family. Fixed homes: f44←@0x10, f30←@0x2C,
  f69←@0x3D, f68←@0x44, f54←@0x36, f28←@0x96, f34←@0x1C, f36←@0x1E,
  f26←@0x4A, f67←@0x43; + obs override: (10,79) heading projects
  from f34 (f30 now holds the fire-mode selector). All
  conformance-import scoped, no native change, no golden. Full-take:
  y 335,570→534, heading 593→22, x 1,263→541, **total 732,243→
  368,779 (−49.6%)**, zero collateral. Residual z ~367k = terrain
  closure (pieces on terraformed mounds; want/got bob in lockstep,
  constant per-slot ~16) → rule `mc2l24-castle-piece-terrain-z`.
  Test `mc2_castle_piece_import_field_homes`.
- **MC2 (10,57) FROZEN FALLING SPHERE + VOLCANO-LANE z + the
  l30-terrain RULING — LANDED 2026-08-03 (session 6, dig C).**
  ① The fixture's "z+16 constant" was a pair-0 artifact; real diff =
  −f2c(retail@t): the port FROZE a falling sphere. (10,57) = the
  random-value mana sphere (`sub_50130` EF:36631, action 0x3E=62);
  its tick `sub_35FB0` (EF:26318, settle EF:26526-46, bounce
  EF:26567-77) is byte-identical to the (10,39) ball law ball_tick
  already serves. Importer was ALREADY right — the class-10
  effect-tick whitelist just never listed action 62, so imported
  spheres fell to the terrain catch-all. Fix: `| 62` in the effect
  gate (world.rs:2270) + `62 => ball_tick` (mc1/combat.rs:2999);
  native-inert (native spawns m57 as model-39/action-41). t=0..500:
  (10,57) z 1438→22, all rows 5233→72 (−98.6%); whole-window
  unexplained field 10,175→1,753. ② (10,16) boulder vz home:
  `sub_32600` EF:23765 reads vz from @0x2C; uniform import homed
  f44←@0x2A (=200 always) → +200 relaunch per pair; scoped
  f44←f2c block (conformance.rs:1373). ③ (10,19) column z-snap
  strict-gated frozen-z (tail.rs:1578), mirroring summit18.
  ④ **RULING — the §l30-terrain OWED check is ANSWERED: the summit
  plateau is RUNTIME-terraformed, pure capture.** `mc2_dome_tick`/
  `sub_31940` (EF:23193) writes the heightmap directly
  (`mc2_dome_cap` EF:23300-18); decisive: at t=0 exactly one dome
  exists mid-grow at a DIFFERENT site while the summit already
  reads 2624 — an earlier finalized dome the recording's
  entity channel cannot carry. Nothing further portable; rules
  `mc2-summit-fire6-z-capture` + boulder/column re-eruption landed.
  BANKED: extending frozen-z-under-strict to the shared
  `standing_fire_tick` would recover summit-slope (10,6) fires but
  touches MC1 terrain goldens — own dig. Tests ×2, non-vacuous.
- **MC2 (5,19) FIREBUG LUNGE + CLASS-15 DETACHED-JAR ARC — LANDED
  2026-08-03 (session 6, dig D).** ① (5,19) = the FIREBUG; retail
  oscillates actSpeed 76↔8 with the `byte_0x46` sub-state and rolls
  the entity LCG only on the fast leg. `HitFirebug_25610` case 1
  (EF:16386-16407) sets b46=2 and RETURNS; case 2's drop to
  maxSpeed + its own roll (EF:16409-16416) run the NEXT tick. The
  port's `continue` fell through into case 2 SAME-tick → speed
  dropped a tick early AND the LCG double-advanced. One-word fix
  `continue`→`break` (mc2/roster.rs:2119-28); native change, NO
  golden moved. t=1960+1500: speed 109→8, rand 129→28; fixture
  sigs dropped 5,19:rand+speed in TWO corpora (mc2l24 t=2157 AND
  mc2l4 t=76). Residual heading/x/y = ruled wander-turn capture.
  ② Class-15 detach: the lead's premise CORRECTED — slot 73's
  20k-tick idle is FAITHFUL (0 rows, not torn); the family is a
  ~15-tick FLING at t=15080-95: the m26-wraith spell-steal jar is
  a moving projectile (z 251→344→0, action 78/pitch 5) the port
  dropped on frame 1. Retail arc `sub_59DC0` (EF:41198-41243) runs
  off homes the class-15 import never mapped: arc counter
  dword@0x10 (`sub_69300` EF:55807 zeroes it at the steal) + wraith
  slot word@0x26. Fix: `action45 == 78` arm in the class-15 import
  (conformance.rs:1359) + 1-line native pitch-copy each rising tick
  (EF:41216-18; world.rs:6911). t=14990+130: 64→12 rows,
  unexplained 64→0 (residual = pre-existing pose-phase). Tests ×2,
  non-vacuous, no re-pin.
- **MC2 `owner` OBS-SCHEMA GAP + the (5,0) identity CORRECTION —
  LANDED 2026-08-03 (session 6, dig E).** Per-class @0x28 truth
  table (EF cites): (10,42) build painter @0x28 = parent CASTLE
  (repaint `sub_5FBD0` EF:61192-93; level-up `sub_60480`
  EF:61596-97); (5,{0,19,21,25}) pyramid-SUMMONED creatures @0x28 =
  the PYRAMID (summon EF:13420/13413); (5,10) = ring-spin angle
  (f36 arm intact); class-15 = wizard (unchanged). ⚠ PREMISE
  CORRECTIONS: the session-5 "(5,0) = hydra segments" handoff was
  WRONG — on l24 the hydra is (5,27); the (5,0) owner=7 family is
  the pyramid's summoned WORMS in the apocalypse window t≈52-68k.
  And the pyramid was POISONING its own children: its repurposed
  @0x28 (spin angle) was fused into id24, which the summon copies —
  excluding (5,10) from the fusion makes pyramid id24 = @0x1A = its
  own index, the identity that makes retail parentId ≡ port own_id.
  ⚠ TRAP (the transient mid-session `field:5,0:owner` atom): model
  0 is ALSO the generic worm body whose id24 is its body slot — a
  naive `id24 != slot` guard over-projected 261,555 wild rows; the
  final discriminator is "referenced entity IS a live (5,10)
  pyramid". Numbers: whole-file owner mismatches **47,083→143
  (−99.7%)**; painter window t=10060-70 →0; apocalypse t=52000-500
  →0 with 2,509 summoned-creature rows present; non-owner rows
  byte-identical before/after (no ripple); exactly ONE sig atom
  changed anywhere (mc2l24 t=10062 lost 10,42:owner). Residue: 29
  class-15 want=116 rows (one per spellbook model — adjacent lead).
  Tests ×2 (gate non-vacuity proven).
- **SLOT-DESYNC CLASSIFIER + village-regrade RETIREMENT + WAVE
  RE-CENSUS — LANDED 2026-08-03 (session 6, dig F).** The
  session-4 free-list ruling + open-leads 0b are now a COMPUTED
  roster rule (literal id `slot-desync`, pose-phase mechanism;
  `--no-slot-desync` opt-out): within one pair, still-unexplained
  missing/extra of the same (class,model) pair up by nearest x/y,
  tagging only min(missing,extra) per side — one-sided residue
  stays open. Ordering is LOAD-BEARING: runs after the roster,
  BEFORE pose-phase (at a wave the port extras are pose-phase but
  the retail missing are not; pose-first orphans the balanced
  family). Field rows untouched — proven byte-identical OFF/ON on
  all four takes. Fires on l24 at 236/67,391 pairs (0.35%),
  exactly the two scripted waves + the apocalypse epoch. Impact
  (missing unexplained): l24 1,688→1,209, l30 188→126, l4 249→199,
  mc1l0 98→83. `mc1l0-village-regrade` RETIRED (0 hits post
  re-record; region absorbed by mc1l0-terrain-z). **RE-CENSUS
  VERDICTS — both REAL unported-spawn leads:** (10,25) 37 missing
  / 0 extra, 100% one-sided — a short-lived doomsday-pyramid
  effect (action 25, life 7/8) the port never spawns; (10,75) 128/31
  → post-absorption **110 missing / 13 extra** — the doomsday
  TAIL-DRAG segment chain (tail.rs:448 model65=75) under-produced.
  Small one-sided residues: (5,3)+3, (5,26)+2, (14,1)+2 missing.
  ⚠ SESSION-7 CORRECTION: the census counts stand but the
  ATTRIBUTION was wrong — neither family is doomsday. Both come
  from the (11,2) storm-switch disposition (whirlwind heads +
  their 11 funnel nodes + `sub_4F6A0` area blasts); see the
  session-7 switch-box entry above.
- **SESSION-6 CLOSE-OUT (2026-08-03).** Post-six-digs full l24:
  unexplained field 1.58M→**771,218** (−51%), missing 1,721→1,209,
  extra 6,432→6,067; pairs fully explained 4,517→**11,127**;
  conforming 0→**4**; rng 12 singles unchanged. All six suite
  manifests promoted green, 0 regressions (l24: 1 fixed + 12
  drifted-improved; fixes propagated to l4 balloon atoms + l30
  firebug atoms). Workspace sweep 42 bins green --no-fail-fast,
  fmt clean. Roster 48 rules. **Hydra bolt-cadence RE-MEASURE
  (the dig-D-session-5 NEXT-LEAD precondition): t=10650-11150
  reads 1,529/94 — BYTE-IDENTICAL to the mid-fix session-5
  measurement ⇒ the barrage under-fire is independent of the
  import homes; the portable head-state slice (multipart.rs:1732)
  is a LIVE dig.**

- **MC2 HYDRA (5,27) — FOUR IMPORT FIELD-HOME BUGS froze the whole
  machine — LANDED 2026-08-02 (session 5 mc2l24 intake, dig A).**
  The m27 hydra branch machine homes four struct words where the
  uniform MC2 importer spent other lanes. remc2 `sub_2AD40`
  (EF:20770-800) writes `fov_0x22_34`; the branch integrator
  `sub_2A340` (EF:20233) switches on `word_0x2C_44` and reads
  `dword_0x10_16`; the branch index / body live-branch gauge is
  `byte_0x3B_59`. `import_ent_mc2` had the uniform homes: `f36:0`
  (dropped @0x22), `f44:@0x2A`, `f50:@0x30`, `f26:@0x2E`. Corpus
  proof (dump-state slot 16, t=0→1): `m27_integrate` mode 0 =
  roll+73 / fov+62 / speed+16 verbatim (2461→2534, 1433→1495,
  160→176). f36←@0x22 = per-branch spline PITCH
  (1433/2595/1709/1905/1985 — imported 0 collapsed all 5 branch
  heads to one z=2951); f44←@0x2C = integrate MODE (@0x2A=100 hit
  the no-op arm → roll/fov/speed frozen; branch 46 mode-1 −64+−16
  =−80 = the "|speed|=port+16" symptom); f50←@0x3B = branch index
  0..4 + body gauge 5 (@0x30=0 collapsed every branch onto
  `D404C[0]`); f26←@0x10 = whip counter (steps 1→2→3→4 in lockstep
  with crack speeds −192/−130/−23/192 @ t=180 slot 46; the m0
  `(5,0)` arm extended to `(5,0|27)`). Fix conformance-only in
  `import_ent_mc2`, (5,27)-scoped; native spawn untouched.
  Numbers ((5,27) rows): t=0..2000 **34,923→1,180 (−96.6%)**
  (speed 7357→0); t=40000..41500 **67,269→19,012 (−71.7%)**
  (speed 13356→3); non-(5,27) rows +0.06% (noise). RULED not-bugs:
  death-window z residual 11,110 = terrain-crater non-closure
  (bodies are ground-walkers, `m27_move` z=`ground_z`; x/y match,
  z differs ~370); t=40565 missing 78/extra 58 = free-stack
  slot-order desync on a mass spawn/death tick; window-1 residual
  ~1,180 = body-brain wander phase drift (shared move-core).
  NEXT: m0/m3/m22 siblings likely need the same f44/f26 homes if
  their families ever surface — extend the arms, don't re-derive.
- **MC2 DOOMSDAY PYRAMID — RNG LEAK + owner FIELD-MAP — LANDED
  2026-08-02 (session 5, dig B).** The `got[t]==want[t+4]` rng
  window t=51751-70 SOLVED, and the "blind-landed perturb arm
  draws global" hypothesis REFUTED: retail's (10,14) ring-rock
  ctor DOES draw the global LCG; the window was the port failing
  to SUPPRESS the ring. The importer restored the pyramid's `f26`
  from @0x2E (charm lane ≈0) instead of `dword_0x10_16`/@0x10, so
  the 0..1200 doom-meter reset to 0 every pair, re-ramped to only
  30, and never crossed the 600 gate (`sub_21490` EF:13031) that
  stops the `for k in 0..4` (10,14) spawn ring (EF:13070-90) — 4
  spurious global draws/tick. Fix: `f26` match `+ (5,10) =>
  r.scratch10`. rng 51500-52100: 21→1 (window 20→0); whole-file
  32→~12; death window 0/100. ALSO: the pyramid repurposes
  `parentId_0x28` as its ring-spin angle (+96 & 0x7FF per
  un-suppressed tick, EF:13072) — imported f36=0 mis-angled the
  ring and pinned the `owner` obs at 0 (11,721 rows); fix =
  import `f36 (5,10)←owner28`, project `owner (5,10)←f36`
  (owner diffs 51500-52100: 874→334, all in-window pyramid rows
  gone). `sub_21030` case 0xF verified ALREADY PORTED
  (doomsday.rs:432-453, session 4) and faithful to EF:12857-80 —
  retail reaches state 0xF at t≈63289 and the apocalypse window
  grades clean. Pyramid heading (6,969 rows) = pose-phase noise.
- **MC2 PLAYER DEATH/RESPAWN — RULED FAITHFUL (first player-death
  corpus) + class-15 heading gate — 2026-08-02 (session 5, dig
  C).** mc2l24 holds 14 human deaths (respawns t=2609/4462/6093/
  8977/11243/34931/39200/39895/41232/43087/46127/54046/60451/
  61490). Every respawn re-inits in ONE tick (trace slot 116
  t=2608→2609): life→maxLife via `CopyMaxLifeToLife_49A20`
  (template `AddPlayer_4A920` EF:33317-38), mana→full refill,
  z→respawn pad, scratch d88→1000, action 3→0, flags clear
  0x1020, class-15 spellbook (slots 161-191) re-granted — and
  both sides AGREE at t+1 at every death. Residual death-window
  rows (1-tick mana/spellbook/life/hand blips + 7 slot-79 swaps
  at t=2608 = transient slot-alloc of the 22 re-granted book
  records) are the input-delay-2 boundary, NOT a port bug; no
  native change needed. FIXED: 25,334 class-15 `heading`
  false-divergences → 0 — the port repurposes the class-15
  world-yaw lane @0x1C for the subSpellIndex payload and projects
  heading 0 (conformance.rs:890-94); the "@0x1C dead on
  manifestations" premise is REFUTED (a detached spell jar, model
  0 action 78, slot 73, holds its fling yaw ~1634 for 20k ticks);
  facing is cosmetic (cast reads f30/f34) → skip class-15 heading
  in `compare_mc2_gated` (verify_mc2.rs), twin of the human
  applied_yaw skip. RULED capture: player.mana 24,465 +
  player.life 4,667 = regen-cadence drift (the stored
  `lifeRegen_0x163_355`/+132 deltas live in the un-recorded
  wizext; life onset t=299: retail holds post-damage ~16 ticks
  then +5/tick, port regens one quantum early, heals at cap);
  mana_max 5,053 + player_ent_idx 1,462 = class-10 effects/
  slot-desync lanes, not vitals. OPEN: class-15 detach state
  machine (slot 73 pitch 5→0, action 78→1 — real unmodeled state
  diff, still compared).
- **MC2 FOUNTAIN + TEMP MANA + BALLOON-REFUSAL — 2026-08-02
  (session 5, dig F).** ① BALLOON-REFUSAL LAW PORTED (the
  player-observed law): retail's balloon sphere-acquisition scan
  `sub_5F810` (EF:60994-61023) skips any (10,39) carrying the
  decay channel `byte[1]&0x20` (port flag 0x2000) — fountain/
  mana-rain spheres are 140-tick TTL and carry it, so retail
  balloons never take off for temporary mana. Port scan omitted
  the gate; fixed in `mc2_castle_roster` (castle.rs:737,
  `|| e.flags & 0x2000 != 0`), MC2-only by construction, pinned
  by non-vacuous test `mc2_balloon_refuses_a_decaying_fountain_
  sphere`. Faithful port, NOT a deviation. ② NATIVE FOUNTAIN ARC
  fixed: `mc2_summit91_tick` discarded the apex
  (`word_0x2C_44=(rand&0x7F)+128`, EF:24052) → balls sprayed
  flat; now `e.f46 = apex` (morph.rs:563); conformance-neutral.
  ③ TEMP-BALL TTL LAW PROVEN already byte-exact: source
  `AddManaRain` sub_32CF0 (EF:24007, 3 spheres/tick, 5-draw
  arming [speed/apex/color r%9−1/mana/yaw], maxLife=140,
  byte[1]|0x20, z=ground+96); mover `TransformArcherToMana`
  (EF:26173-307: z+=v, v−=16 clamp≥−128, bounce −v/4 zero≤16,
  roll+friction 250/256, decay tail fade@12/ghost@6/expire@0);
  corpus slots 133/168 match to the unit. ④ (10,39) FOUNTAIN BULK
  (2.5M rows) RULED terrain-closure capture + slot desync: x
  diffs 99.8% ≤1 tile, z 88% ≤16 — balls rest on the
  doomsday-terraformed mound the pristine replay lacks; early/mid
  rows = terrain-roll (worm-death mana + wake-law downhill).
  Ball laws byte-exact throughout. NOTE: the retail minimap draws
  ALL fountain balls ORANGE (player-observed retail one-off hack)
  — port colors by logic; standing map-presentation deviation
  ruling applies, never "fix" toward retail. HANDOFF: (5,0) on
  this take = HYDRA SEGMENTS (not balloons); their owner rows
  (16,333, constant want=7 got=0) = obs-schema gap (@0x28
  projected only for class-15 and (5,10)) — open lead.
- **MC2L24 LIGHTNING (9,9) 46k-MISSING — RULED CAPTURE 2026-08-02
  (session 5, dig D; mechanism fully proven, no code landed).**
  The take's #1 missing family — (9,9) 46,241 missing / 1,967
  extra (42% of all missing), max_life 64,987 rows (63,865 =
  want −1 got 0). Caster identified: the HYDRA (dump-state 10703
  id=15, (5,27), 1e6 life) — the trail is its (9,9) heavy bolt
  (`mc2_spawn_bolt9` = `sub_4D860`/`sub_1D260`, EF:9883/34942,
  impact (10,23) id=15); the seven (9,0) id=116 are the PLAYER's
  bolts, a separate no-trail family — don't conflate. Born-dead
  law CONFIRMED byte-correct (EF:58341 ≡ proj.rs:883; ahead node
  lives 2 recorded frames, behind 1); "reaped a tick early"
  REFUTED (import census t=10703→05: nodes 81→162→162, disabled
  0→81→81 = retail exactly); node-cap REFUTED (clamp 96/beam,
  beams ~80; retail's 326-node frames are multi-volley stacking).
  Residual = two capture mechanisms: ① UNDER-FIRING — port lays
  18 trails vs retail ~38 birth-ticks per 500; the hydra's
  multi-head barrage cadence comes up ~half (the attack GATE is
  faithful: `sub_27E00` EF:18297 ≡ roster.rs:2877-87; divergence
  is upstream head-state/rand); ② maxLife −1/0 — both engines
  pop a LIFO free stack; a sustained barrage drifts the beam
  slot upward (measured 160→183→254 across t=10703/04/05) so
  retail's steady nodes fall below the beam; per-pair replay
  cannot reproduce multi-frame free-stack drift. Both = the
  cast-timing-skew + free-list-reuse class already ruled
  capture; l24 amplifies the mc2l4 residual ~30×. (10,23) 826/5
  shares the root. Windows: 10650-11150 = 1529/94; 43100-43600
  = 1686/0. NEXT-LEAD: trace the hydra multipart bolt path
  (multipart.rs:1732) at t=10704/05 for any PORTABLE
  head-state bug under the capture noise — note dig A's
  field-home fixes landed mid-measurement; the post-fix cadence
  may already differ, re-measure before digging.
  ⚠ SESSION-7 CORRECTION: mechanism ① (UNDER-FIRING) was NOT
  capture — the NEXT-LEAD paid out. The whip is a five-shot
  burst and the port dropped four of the five because
  `manaRegen_0x88_136` (the bolt power) was never imported; see
  the session-7 Resolved entry "MC2L24 HYDRA HEAVY-BOLT BARRAGE
  UNDER-FIRE". Mechanism ② (maxLife −1/0 free-stack drift) and
  the beam GEOMETRY stand as capture. Windows now 89/175 and
  166/0; full take (9,9) 46,241/1,967 → 13,632/3,052.
- **MC2 SUMMIT RE-ERUPTION TRIO — LANDED 2026-08-02 (session 4,
  opus dig; complements the fire-spray ring loop).** Retail law
  (EF cites): the (10,18) summit vortex controller (`sub_32A70`
  EF:23906) is a PERSISTENT invisible singleton latched by the
  GLOBAL `D41A0.word_0x31` (the (10,19) column latches
  `word_0x33`); tick-0 eruption spawns column + one (9,0) bolt
  (impact (10,17)) + one (10,16) boulder, controller yaw +=1280
  UNMASKED (only the bolt copy gets the &7FF mask, EF:23976-87);
  pulse rolls (`dword<128 && dword&0xF && rand%5==0`) each spawn
  a (10,16); despawn ONLY on ground-move (`z != getTerrainAlt`)
  or dword>=127, releasing the latch. RE-ERUPTION CADENCE
  (EF:23921-35): at dword>2500, a 1-in-100 per-tick roll resets
  dword=0 — ONLY while `word_0x31==0` (latch free). mc2l30
  corroborated tick-exact: site-118's controller (slot 134)
  erupts t=274, site-114's (slot 195) steals the latch t=279 and
  despawns t=281 (its still-growing dome moved the ground under
  it), slot 134 idles to dword=2507 and re-erupts EXACTLY at
  t=2536 (roll r1=28800 %100==0), then holds the latch forever —
  no further re-eruptions (recorded word_0x31 stream matches:
  0@2535, 134@2537+). THREE port bugs fixed: ① PHANTOM
  GROUND-MOVE DESPAWN — the port compared the imported plateau z
  (3296) against pristine ground_z (1232) and killed the
  controller; strict-gated FROZEN-Z law (no re-snap/despawn
  under replay; native keeps the exact check) + regression test
  `mc2_summit_vortex_frozen_z_under_strict`; ② controller yaw
  wrongly &0x7FF-masked (heading 512-vs-2560); ③ the eruption
  LATCH was not imported → over-eruption (~13 phantom eruptions
  5037-8330 once frozen-z landed) — `word_0x31`/`word_0x33` ARE
  in the recorded D41A0 header: decoded as `RetailMc2.vortex`/
  `fire_col` (mgcr.rs, additive) and imported into erupting/
  plume (conformance.rs). Both halves load-bearing (frozen-z
  without the latch import regresses to 37 rng / 1,152 extra).
  Net on current tree: l30 rng 20→19, missing −28 (the recovered
  t=2536 re-eruption records), extras flat, mc2l0/l4 provably
  inert (no (10,18) there). Goldens unmoved (dome-trap
  mc2_slice passes); check-decode clean ×3. Residual 19 rng: 1 =
  t=274 dome-import eruption-timing (open, dome life decrement
  on import), 18 = the slot-desync fire cascade (ruled, fire
  entry).

- **MC2 FIRE-SPRAY RING LOOP — LANDED 2026-08-01/02 (session 4,
  opus dig; closes the l30/l4 RNG residual).** Retail's (10,19)
  ground-fire-spray column tick `sub_32F40` (EF:24095) wraps its
  (10,14) smoke emission in a walk of the RING-0 SPLAT TEMPLATE
  (`while (sub_10130(AddE7EE0x_10080(0,0)) == 1)`, EF:24112-40)
  — ring 0 has 4 cells (baked search.bin value-0 count), last
  dropped as the stop code ⇒ THREE emission cells per tick, each
  with the ~50% gate roll (`2*((r%0x9D)/79)-1 > 0`), 2 jitter
  draws offset `192*(dx,dy)`, and the odd-life 4-puff (10,14)
  ring. The port's `mc2_fire_spray_tick` (mc2/tail.rs) emitted
  ONCE (no ring loop) → ~1/3 the smoke AND under-drew the GLOBAL
  stream (each smoke ctor draws lcg32 — retail-matching), which
  WAS the l30 rng residual. Fix: ring_cells(0,0) loop, native+
  strict (unconditional retail law). Numbers: mc2l30 rng
  **202→19** pairs, (10,14) missing 990→390; mc2l4 rng
  **163→13**, (10,14) missing 873→326; mc2l0 untouched (479
  conforming — no volcano, fix inert); extras FLAT everywhere
  (no over-production); no golden moved. Same dig VERIFIED
  BYTE-FAITHFUL (no fix): (10,0) fire tick sub_30D50 (fire does
  NOT spread — one damage pulse + burn + flicker + z + anim),
  (10,6) sub_31760 incl. 1/7 smoke-on-shrink, (10,17) meteor
  ring-seeding sub_32880, (10,1) big-explosion sub_30F60,
  meteor-shot spark sub_66180, (10,16) boulder→(10,6) sub_32600,
  and the ring template mapping. Port re-eruptions CONFIRMED
  PRESENT and matched at t>2400 (the "never re-erupts" reading
  of the missing rows was wrong). RULED on the residual: the (10,0)
  missing/extra bulk (l30 1,659, l4 1,975) = FREE-LIST
  SLOT-ORDER DESYNC, not law — proven at l4 t=9082: missing and
  extra fires have IDENTICAL x/y, differing only in slot (and
  hence flicker/z, since rand_0x14 seeds from slot+global_rand)
  — single-snapshot import can't recover retail's within-tick
  free-then-reuse LIFO order; matcher pairs by slot. NEW LEAD
  banked: (10,12) missing 313 (l30) / 779 (l4) = possession
  WEAK-PULSE family (cast lane, not fire).

- **MC1 CORPSE-FLAME SPREADER CADENCE — LANDED 2026-08-01
  (session 4, opus dig; the "mc1l0 (10,0) fires 57/210" family).**
  The (10,0) ground-fire family is rung out by the (10,1)
  fire-spreader (`sub_25130`, sub_main:28161-70): per ring cell
  ONE draw is the skip test — spawn iff `v5 % 157 >= 79` (~50%) —
  and the x/y jitter PAIR is drawn only on the SPAWN branch (a
  skipped cell costs a single draw). Spawned fire inherits id24
  (:28175), f30 (:28176), `flags |= 0x80 | (spreader & 0x10000)`
  (:28177-79). Port bugs (mc1/combat.rs `spreader_tick`): skip
  test was `rand & 1`, and both jitter draws ran on EVERY cell —
  the 3-draws-vs-1 skew desynced the spreader's per-entity stream
  so the whole ring's fire SET diverged (the free-stack census
  passes on every pair, so this was a genuine tick-law bug, not
  drift). Fixed + f30 inherit. mc1l0 (10,0) 57/211 → 32/166 (the
  t=564-583 worm-death burst ~130 rows → 1); mc1hwl0 48→49
  conforming, (10,0) 2754/1571 → 2455/1222, (10,1) 385/691 →
  261/586. Residual = within-tick slot substitution (fires
  faithful in pose+tick but landing in different slots at
  free/reuse boundaries) = capture, matching the MC2 ruling.
  L005 GOLDEN+OBSERVABLE re-pinned D-E ONLY (post-init/A/B/C
  hold byte-for-byte — behavior change localized to the combat/
  aftermath stages, by design). ⚠ SHARED TICK: `spreader_tick`
  dispatches for BOTH games (engine/world.rs effect_tick) — the
  MC1 law also collapsed MC2 corpse flames: mc2l0 fixtures t=58
  and t=77 flipped open→conforming (promoted same session). If
  MC2's retail spreader ever proves a different skip law, split
  per-game then — empirically the MC1 law fits MC2.

- **MC2 (9,17) POSSESSION RE-FIRE — LANDED 2026-08-01 (session
  4, opus dig; the biggest EXTRA family, mis-swept under
  `mc2-cast-timing-extra`).** The port's `mc2_cast_gate`
  re-pressed an already-armed possess manifestation into a FULL
  new (9,17) delivery bolt + mana debit every press, all tiers.
  Retail law: the armed-possess press only sets `byte_0x3C_60`
  (sub_5F660 case 1, EF:60902) and the consumer `sub_68DE0`
  (EF:55987-56013) is TIER-gated — tier 0 just CLEARS the signal
  (no bolt, no debit); only tiers 1/2 spawn (a different class-9
  subtype-1 via sub_69900, 3-tick cadence, untraced/unexercised).
  Corpus proof: MISSING (9,17)=0 vs EXTRA=452 — symmetric timing
  skew would balance; retail emits exactly ONE bolt per arm.
  The old cast.rs "//player retail-verified, all tiers" comment
  was a misreading (likely a higher-tier Mana-Magnet
  observation). Port: cast.rs re-press gated on `f71 > 0`;
  test renamed → `mc2_possession_tier0_does_not_refire_...`.
  Numbers: mc2l0 466→479 conforming ((9,17) extras 445→312);
  mc2l30 extras 452→355, rng 202 UNCHANGED (the residual rides
  the volcano windows, not casts); mc2l4 (9,17) 1393→1208. The
  remaining fresh-arm extras are genuine input-reconstruction
  skew (the recorded held-register toggles per frame; retail
  arms ~1 tick before the recorded button) — correctly
  roster-swept, NOT a sim bug; don't chase them into the input
  decode, changing it regresses the other takes.

- **PER-ENTITY `rand_0x14 += setting_30` PERTURB — LANDED
  2026-08-01 (session 4), corpus-invisible by census.** Retail
  has exactly THREE per-entity perturb sites (whole-tree grep):
  the pyramid pick rolls (sub_21850, EF:13140/13220) and the m27
  branch bolt (sub_2A7F0, EF:20521); pattern = LCG → modulo draw
  → `rand_0x14 += setting_30` (next roll starts shifted). The
  counter: `setting_30` increments beside `Turn++` in
  `PlayerEvents_51BB0` (EF:37557) and zeroes at level init
  (EF:31290/38455/39339/43327) → during the entity pass it
  EQUALS the post-increment turn — the same value the cave
  carpet tail's corpus solve anchored (EF:59803 is the one
  GLOBAL-stream perturb site, already ported). remc2's
  `uint8_t setting_30` typing and Level.cpp:340's "0x3D after
  load" are both remc2 artifacts (the latter is their own debug
  reseed `//fix`), not retail law. Port: `Gen::mc2_rand_perturb`
  (mc2/mobs.rs) + `MobCtx::mc2_turn` (the sanctioned no-Gen-field
  channel, same rationale as `strict`); applied at the three
  sites. BONUS FIX found in the same read: the port short-
  circuited pyramid roll 1 under the bit7 escalation — retail
  draws UNCONDITIONALLY (EF:13137-39) and only overrides the
  ROLL to 0 (:13141-45); the draw+perturb now always land.
  Zero corpus effect (no (5,10)/(5,27) on any graded take — this
  is why the old "top lever" claim was wrong); prepares future
  doomsday/hydra takes. Suites green, goldens unmoved.

- **MC2 SAME-TICK REAP — LANDED 2026-08-01 (session 3; the
  player-chosen top lever, opus dig corroborated).** Retail MC2's
  death path only SETS the disable bit (`DisableEntityDrawing04`
  EF:40332-35 = `byte[1] |= 4`, nothing else); the reap is
  `sub_57F20` (Events.cpp:5209-39: tile-unlink → recycle-list
  scrub if byte[2]&2 → class-zero → free-stack push, atomic) and
  the per-tick site is the TOP of `UpdateEntities_57730`
  (EF:39948-56): after the single global LCG draw (EF:39947), one
  unconditional ascending pass frees every record already
  disabled at tick entry, BEFORE bucketing and dispatch. (The
  ledger's old "Events.cpp:548" cite is `ApplyEvents_498A0` —
  LOAD-TIME only, sole caller GenerateEvents; it shows the same
  disable→sub_57F20 idiom but is not the per-tick mechanism.) So
  a record disabled during tick T's dispatch survives EXACTLY ONE
  end-of-tick snapshot and is gone before T+1's dispatch — the
  measured ghost law and the MC1 tick-top reap are the SAME LAW
  seen from the two ends. Slot reuse: earliest at T+1 dispatch
  (NewEvent pops free-FIRST, LIFO; reap pushes ascending →
  reused slots pop highest-first). Disabled-but-unreaped records
  draw NO rand (already dispatched their death tick, skipped
  after). PORT (strict-scoped; native MC2 keeps its in-loop free
  pending the native sweep-law port — DEVIATIONS.md updated): ①
  world.rs `tick()` tick-top reap gate extended to
  `Mc2 && strict_retail` (runs after the tick-top draw, before
  bucket counts — ghosts stop inflating class buckets); ② the
  importer's ghost free-stack pre-push DELETED
  (conformance.rs — the reap owns the push now; keeping both
  would double-push; `ghost_slots` stays for the census, ghosts
  still import class≠0/unlinked = retail's end-of-T state).
  NUMBERS (A/B on identical build): mc2l0 conforming 240→452,
  gross extras 3,761→1,389, unexplained extras 198→22, rng
  mismatch 3→2 pairs; mc2l30 (10,0) extras 5,590→346, (10,14)
  917→36, roster-explained 6,320→6,658; mc2l4 explained
  13,330→13,698. The l30 202 rng pairs survived UNCHANGED ⇒ the
  rng residual is entirely §l30-churn (b) per-entity rand sites.
  Two-wrongs exposure measured and accepted: mc2l0 unexplained
  field +232 rows ((10,39)/(10,1) slot-occupancy collateral,
  model co-diverges — different entity in the slot) and gross
  missing 431→1,095 (mostly re-labeling: slots that used to hold
  a port ghost aliasing retail's record now sit empty = cleaner
  missing atoms). Fixture t=737 re-statused conforming→capture
  (`mc2-fire-churn-m13`: a newborn (10,13) churn spawn into
  recorded-free slot 464 — pre-reap it "conformed" only because
  the ghost extra masked the family). 7 mc2l0 fixtures FIXED
  (several were mis-bucketed capture — they rode the reap), all
  suites promoted green, sim tests green, native goldens
  UNMOVED (strict-scoped).

- **MC2 +0x54 `applied_pitch` STATICS FAMILY — SPLIT 2026-08-01
  (opus dig): one real ASSET-BAKE lead + two downstream slices;
  no f80-law bug anywhere.** +0x54 = `array_0x52_82.pitch` (port
  f80) and means different things per family:
  ① **(10,45) dwellings, 41 rows, retail 194 → port 184 —
  RESOLVED 2026-08-01 (opus dig + fix landed): NOT a bake trim.
  The night/cave art is GENUINELY 36 wide** (RNC-decompressed
  payload headers read straight from the retail files: TMAPS0-0
  day = (38,39), TMAPS1-0 night/night-fog = (36,39), TMAPS2-0
  cave = (36,39); 240 of 504 entries differ across banks, in
  both directions — no uniform trim exists, and the port's
  decode/bake reads the header dims verbatim). The REAL law:
  retail derives `particlesParameters` ONCE AT BOOT from the DAY
  bank — `sub_71410_process_tmaps`' sole caller is Initialize
  (EF:42885), the boot-active tmaps file is TMAPS0-0
  (TextureMaps.cpp:595), and the per-level TAB swap
  (ReadAndDecompress.cpp:55/110/137) never recomputes the table —
  so night/cave levels run day-art extents session-wide. The port
  re-derived per level from the level's own bundle. FIX:
  `Bundle::mc2_extent_dims` (mgc-formats bundle.rs) day-sources
  the dims for ALL MC2 extents derivations (app loader,
  conformance runner, every test/example world recipe — 11
  sites); rendering stays on the level's variant bank; no rebake,
  BAKE_EPOCH unchanged. 52 SPRITE_PARAMS rows shift on
  night/cave/fog levels; mc2_cave + mc2_slice STATE goldens
  re-pinned as behavior (OBSERVABLE goldens HELD). mc2l0 452→466
  conforming, the 41-row family gone, fixtures t=223/t=291
  promoted.
  ② **(10,39) spheres, 123 rows = DOWNSTREAM of the open sphere
  mana economy; rotation law CONFIRMED byte-exact** (thresholds
  BALL_SIZES + ROT quads match retail's own values 140→13,
  300→28, 1028→56, 2250→70; re-sprite gate EF:26742 ==
  combat.rs:2904). 28 rows are slot-occupancy mismatches, 92
  co-diverge on f140 mana (death-burst fractionation: port slots
  carry 280=2·140, 420=3·140 where retail dropped 140-mana
  balls). No sphere fix here — rides the open l4/l30 sphere
  economy + AI-lane work. Size-threshold-off-by-one REJECTED.
  ③ **(10,1)/(10,42), ~38 rows = pure slot-occupancy collateral**
  (model co-diverges on every row — different entities in the
  slot; comparing +0x54 is meaningless). Rides the population/
  timeline divergence; no per-family fix.

- **MC2 (5,13) VILLAGER FAMILY — TERRAIN-CLOSURE CAPTURE, RULED
  2026-08-01 (opus dig).** Model 13 = the Townie/Villager
  (`AddVilliger_4BF40` EF:34037, behavior row 100, flags 0x9 =
  die-on-water + flee-on-hit). The dominant mc2l0 unexplained
  creature family (575 heading + 530 x + 503 y + 255 life + 55
  rand + 26 speed + 22 extra) splits in two, both capture:
  ① **the DROWN family** — every life row is retail-alive →
  port-dead(−1); the dying slots cluster on the EASTERN approach
  (154-173, 206-227), exactly the region MC2's village-growth
  construction paint terraforms to land at runtime (ledger
  §terraform); the pristine replay reads deep water there, so the
  port's FAITHFUL all-four-blocked die law (`mc2_move_core`
  mobs.rs:318-24 = EF:8855-62, row flag bit 0) drowns them —
  confirmed live at pair 1473 (slot 76: retail life 1000, port −1,
  port heading 1918 = the blocked-retry-yaw signature). Per-pair
  reseeding re-imports the live villager and re-drowns it every
  tick (slot 76 alone = 171 rows). Zero deaths on the stamped
  y=212 causeway strip — the load-time stamp is fine; the missing
  edits are the RUNTIME house-cluster regrades. Chaotic heading/
  x/y/rand/speed/extra = downstream of the deaths. ② **the +44
  family** (39 heading rows, got = want+44 ≈ 2× the v_2=22 turn
  cap, alive west-side villagers) — rides the RULED-byte-exact
  wander law's ±22/±45 blip capture (hypothesis-grade within an
  established ruling). Port walker/move/drown/retile/stamp laws
  verified faithful line-by-line — do NOT touch. Disposition:
  roster rule `mc2-walker-drown-terrain` (capture, mirrors
  mc2-guard-terrain); real remedy = the deferred terrain channel.

- **MC1 CLASS-9 AIM SKEW — SPLIT 2026-08-01 (opus dig); the (9,1)
  slice was a LAW BUG — FIX LANDED 2026-08-01 (session 3).**
  Refines the SPAWN-ARM entry's "aim skew stays open": (9,0) =
  fireball, (9,1) = the POSSESS LOB (`spawn_spell_lob(1)`,
  combat.rs:388). Split of the post-pose class-9 residue:
  (c) **LAW BUG, FIXED — (9,1) target_yaw, 525 rows → 0**:
  retail's possess handler `sub_52ED0` (:62970) homes through the
  SHARED `sub_52550_52890` (:62534) which writes +34 =
  angle_between and +36 = pitch_toward EVERY tick
  (:62543/:62546); the port's separate `home_possess`
  (combat.rs:1251) updated only f30/f32 and never wrote f34/f36.
  Proven by the one-tick-lag signature under the re-seed runner:
  361/363 rows-with-predecessor were exact got[t]==want[t-1].
  LANDED: `e.f34 = yaw; e.f36 = pitch;` in home_possess per
  :62543-46 — write-only for the lob (proj_m1_tick reads only
  f30/f32/f126), zero gameplay change, no golden moved (goldens
  never fire the lob); mc1l0 fixtures t=620/t=1158 signatures
  promoted (the target_yaw atom vanished); mc1l0
  conforming-or-explained 4,026 → 4,150. LATENT
  (decompile-corroborated, invisible on this corpus, do NOT
  bundle): home_possess hardcodes yaw cap 34 vs retail row-0
  v_2=56, and snaps f32=pitch instantly vs retail's v_6=22
  turn-step — the pitch one WOULD move trajectory if pitch ever
  steps >22/tick; separate behavior change with its own re-verify.
  (a) **(9,0) fireball = TARGET-DRIVEN, law faithful**: 229/250
  target_yaw rows co-flag `chase` (port acquires a different
  target — 152 port-acquires/94 reverse; exemplar slot 555 retail
  flew straight f146=0 while port acquired the pose-phase creature
  556); same-target cases show ±1-3 noise only. Stays
  open/capture-leaning; optional roster rule "target_yaw co-flags
  chase+heading ⇒ homing a divergent target".
  (a2) (9,1) pitch 144 rows = target BALL z via terrain-z —
  re-triage after the f34 fix. (b) ~56 (slot,t) slot-alias records
  (port holds a different class/model — lob born a tick off) ride
  mc1l0-cast-impacts. The (9,x) flags bit13 lead stays its own
  item.

- **MC1 (5,15) CASTLE-GUARD FAMILY — TERRAIN-CLOSURE CAPTURE,
  RULED 2026-08-01 (opus dig; the MC1 twin of §l4-guard-terrain).**
  Model 15 = the castle guard (behavior row 24, `v_20 = 0x20000` —
  terrain-locked to castle-pad tile types 21/22/24 + 13/14). The
  whole post-pose unexplained family (1,288 rand + 154 heading +
  547 x + 374 y rows on mc1l0) is ONE root: `grid_walk`'s vote-tick
  die-gate (`sub_20480` :25934-40, port mobs.rs:2600-04) reads
  `cap_bit & !v_20` — on retail's recording the castle build had
  regraded those tiles to pad type, so retail runs the 4-draw
  quadrant vote + 1-draw move coin (5 per-entity LCG steps,
  measured); the pristine-plane replay reads the original tile
  type, trips the die-return, and draws ZERO (proven: port rand
  `got` == seed on vote ticks, incl. the unaligned t=3415 case
  that disproves any phase/order theory). Heading = pure ±512
  knock-on (the vote's candidates are f30 + 512k, :25945-71 — a
  frozen +30 vs a re-vote); x/y = the movement knock-on; 154/154
  heading rows co-locate with a rand row (zero heading-alone → the
  uniform vote-weight stand-in contributes nothing here). All six
  diverging slots cluster in one tile box x17-26/y19-28 = one
  castle site, co-tiled with `mc1l0-terrain-z` want-512/got-256
  rows. Port laws verified byte-identical line-by-line — do NOT
  touch grid_walk. Disposition: roster rule `mc1-guard-terrain`
  (capture, mirrors `mc2-guard-terrain`); real remedy = the
  deferred terrain channel, which retires both games' guard
  families at once.

- **SPAWN-ARM f34 MIRROR — RULED DEVIATION (player, 2026-08-01).**
  The post-pose-filter mc1l0 target_yaw residue splits two ways: the
  (9,0)/(9,1) rows are both-sides-nonzero aim skew (targets that
  themselves diverged + cast latency — capture-flavored, stays
  open), but (10,0)+170 / (10,39)+73 / (10,1)+22 rows have retail
  +52 == 0 on EVERY row with the port nonzero — all BIRTH pairs.
  Mechanism: the port's spawn arms (`arm_projectile`, `corpse_drop`,
  payload/eruption/storm) mirror `f34/f36 = f30/f32` on every spawn;
  retail writes +52 only on homing paths, so corpse balls and
  Wall-of-Fire bolts (and the standing fires they convert into
  in-place) are born 0. The lane is WRITE-ONLY for those families in
  the port (readers: class-9 homing `proj_tick`, class-5 multipart)
  — no gameplay bearing; un-stamping would mean splitting the shared
  arm per-spell and risking the faithful homing paths. Ruled a
  deliberate deviation: DEVIATIONS.md "spawn arms (universal f34/f36
  target mirror)" + roster rules `mc1-spawn-arm-f34-{fire,ball,
  flame}` (status deviation). Guard: the rules' hit counts are
  birth-pair-bounded — a jump means a NEW divergence hiding behind
  the lane, re-triage.

- **MC2 CAVE RAND STRUCTURE, ROUND 2 (2026-08-01) — the mc2l30
  headline closed: rng mismatches 9,328 → 202 of 9,337 pairs.**
  The clean corpus offline-solve (recordings→`--csv` rng rows,
  scratch solver) fits `R' = LCG^k(R) + (turn+1)` — the additive
  lands AFTER every draw of the carpet's position, and the counter
  is the POST-increment Turn (solved s = recorded-turn@t + 1 on
  every fitting pair): k=2 on quiet ticks (6,806), k=4 exactly on
  drip ticks t≡5 mod 8 (1,001), +1 activity draw variants (846),
  367 "sandwich" pairs with draws after the additive (activity in
  slots ABOVE the carpet — proving the tail runs AT the carpet's
  pool slot inside the frame walk, not pre/post-pass), and 258
  pure-LCG pairs with NO tail at all: t=3257-3267 (possession
  holds the byte[1]&8 stall every tick, carpet flags 0x1000_0A0D)
  and t=9090..end (carpet action45 = 12, the level-end arm — the
  mover `sub_5D530` is only called from the flying arm EF:59994
  and the death-test arm EF:60074). Source-corroborated: exactly
  ONE unconditional global draw/tick at the frame-function top
  (EF:39947; the parked-carpet window measures precisely one
  draw), the drip reads the post-Turn++ player Turn (&7,
  EF:40501), the tail is `sub_5D530`'s late body (EF:59800-08).
  Port restructure (world.rs): tick-top draw unconditional for MC2
  (post-pass baseline deleted), drip gate → incremented mc2_turn,
  tail moved INTO the frame walk at the imported carpet slot (new
  `mc2_carpet_slot`; native = post-pass fallback), additive =
  post-increment counter, importer folds the mover-less action
  arms into `mc2_carpet_stall`. SUPERSEDES round 1's
  [drip→tail→pass→baseline] order and the tick-entry drip anchor
  (both fit on the TORN corpus under the wrong additive position).
  Numbers: mc2l30 roster-explained pairs 1 → 6,320, rng residual
  202 pairs (all churn-tick draw-count skew — rides §l30-churn);
  mc2l0 167 → **240 conforming** (the tick-top draw re-phases
  every mid-pass stream consumer on non-cave levels too); mc2l4
  roster-explained 13,325 → 13,330, rng 162 → 160; mc1/HW
  untouched. Goldens re-pinned as behavior (mc2_cave B-D + obs D,
  mc2_slice A-E + obs A-E); suites 0 regressions (mc2l0 t=737
  fixed, all mc2l30 sigs re-promoted).

- **MANA-BALL WAKE LAW (2026-08-01) — the ⓪b banked lead, ported
  verbatim; "mana rolls away downhill when approached" is now port
  behavior. ⭐ PLAYTEST CERTIFIED 2026-08-05: "Mana roll now
  faithful. It stops moving outside of the awake range, starts
  moving when moving in range."** Decompile dig closed every open
  question: the writer of 16 into +58 is `sub_54F80` :64361 — the
  SAME per-tick maintenance pass that decrements (:64321), called
  from the bucket walk `sub_54F00_55430` :64266. Law: +58 nonzero
  → decrement, mirror down the +54 chain; else if +59 nonzero →
  decrement it (DEAD branch — nothing ever writes +59 > 0); else
  2D squared distance (sub_42410 :52748, x/y only) to the LOCAL
  HUMAN's wizard entity (:64352 — single scalar index, rivals
  never wake balls) `< 37748736` (= 6144² = 24.0 tiles, strict) →
  +58 = 16, chain members 18 (:64364), +48 stamped with an
  isqrt-of-index artifact (not ported — flagged low-load-bearing).
  The corpus 17-tick period is emergent: 16 decrements + 1
  observe-zero re-arm tick (duty 16/17). Ctor 128 = `sub_3B5A0`
  :47465; HW twin `sub_554B0` byte-identical (hw:60542/:60576/
  :60582). Retail has NO class gate in the pass (bucket membership
  beyond balls/creatures = open); the port scopes to the
  corpus-proven rows: balls (10, state 41) now ride
  `mob_awake_pass` alongside class-5 (mobs.rs — counter handled as
  a raw BYTE, the i8-import −128 trap), and ball_tick's private
  decrement fold is REMOVED: the ballistic gate reads the
  post-maintenance value (retail handler order — this also fixes
  the old 1→0-edge quirk: a fresh ball's window ends at the
  counted zero, and each wake cycle moves 16 / freezes 1).
  Native + strict both (retail law); the settled-ball ground-track
  deviation still applies to out-of-radius balls only. Acceptance:
  `settled_ball_wakes_within_24_tiles_on_a_17_tick_cycle`
  (features.rs) pins the strict boundary + exact period. Corpus:
  mc1l0 440 → **450 conforming** ((10,39) fixture x/y atoms gone,
  t=882 fixture now conforming), mc1hwl0 46 → 48. Goldens
  re-pinned as behavior (flight A-C both modes, L005 B-E + obs
  B-E); suites 0 regressions, drifts promoted.

- **KINEMATICS ROUND 2026-07-31 (the banked coordinate+speed
  deep-dive) — four port/import fixes + three capture rulings.**
  Fixes (all decompile-corroborated, corpus-A/B'd; mc1l0 385
  conforming UNCHANGED, all five suites 0 regressions / 9 drifted
  sigs promoted; mc2l0 7 → 11 conforming + 8 rng-only):
  1. **Class-9 spurious speed ramp (the retail-+2 family, ~14k rows
     across takes)**: retail states 0 (`sub_65C20` EF:63126), 1
     (`CastPosses_65F60` EF:63261) and 29 (`sub_65B50` EF:63023 — a
     charged-impact wrapper over the state-0 body) fly at CONSTANT
     actSpeed; only the shared `sub_65820` core ramps ±2 toward
     minSpeed (EF:62923-31). The port's `mc2_flyer_tick` ramped
     every state toward 384 — the corpus proof was the delta sign
     flipping exactly at 384 and (9,0) (whose launcher floors speed
     at 384, EF:44224) being 100% one-signed. Gated out for
     tick70 0|1|29 (proj.rs). mc2l0 class-9 speed 6,426 → 342 rows,
     l4 ±2 rows 10,055 → 289 — residue = birth-pair cast-timing
     (the slots are free in retail at state-N). The original
     "ramp one step out of phase" hypothesis was WRONG — order was
     always right; the ramp itself was spurious.
  2. **(3,3) balloon ceiling-walk latch (the l30 retail-+48
     family)**: `sub_60D50` walks the cave ceiling at actSpeed 96
     with `byte[0]|=1` (EF:61896/61903) vs 48 flying (EF:61905),
     ceiling clamp flying-only (EF:61921) — the port law was
     verbatim (castle.rs) but the importer dropped bit 0, so every
     imported walker re-took the flying branch each pair. (3,3)-
     scoped bit-0 import (port bit 0 is per-class overloaded).
     l30 954 speed rows → 0 + the z/x/y cascade.
  3. **(3,1) = the MC2 RIVAL WIZARD, replayed as a frozen husk**
     (NOT a balloon — the banked label was a misID): every (3,1)
     field satisfied got(t)=want(t−1) for the wizard's whole life —
     the ±16 "dither" was a one-tick lag on retail's ±16/tick speed
     slew (EF:6484, port-verbatim rivals.rs). `retail_import_mc2`
     never re-anchored `self.mc2_rivals` (the MC1 rival-freeze
     twin); `reanchor_mc2_rival` now points the brain at the
     imported slot + reseeds vdes/strafe/grace/mana lanes from the
     closure. l4 (3,1) rows 46.6k → 24.3k; the REMAINDER is the
     AI decision-lane reconstruction (state/target/hate/burst —
     needs the MC2 wizard-ext decode; the same split as MC1's fix).
  4. **(10,39) sphere mover — the ledger's §sphere spec, ported**
     (TransformArcherToMana EF:26015; behavior change toward
     retail, cave + slice GOLDEN&OBSERVABLE re-pinned, post-init/A
     hold): the MC2 settle law is MC1's shape at different homes —
     moving only while `byte@0x39 || fresh-kick` (EF:26173; ctor
     seeds 128, EF:36617; corpus: b39 counts ~1/tick to 0, f2c
     parks at −16, frozen forever) with z-velocity @0x2C. The port
     had opted the MC2 arm out of the settle gate entirely →
     always-on physics dropped every authored sphere to bare
     ground (the l4 z family) and re-rolled/merged settled spheres
     forever. Landed: settle gate (f58 was already imported from
     b39), f46 ← @0x2C import, absorb-chase (b0&0x40) + decay
     (b1&0x20 → bit-13 tail) + stall-skip (b1&8 → bit 26) latch
     imports, unconditional moving-mode gravity, the EXACT bounce
     `−impact/4 zeroed at ≤16` (EF:26244-52, replacing the
     untraced −32 floor), grounded-ONLY merge (EF:26265-69 —
     the always-scan was port invention), and the per-size
     ROTATION quad on re-sprite (EF:26744-77: 14·(size+1), 13 at
     size 0 — the port stamped MC1 art extents into the applied
     lanes instead: the 16k applied_yaw/pitch family). l4 (10,39)
     37.9k rows → ~3.8k (residual = terrain-closure z + birth
     edges); applied 16,004 → 270. MC1 arm byte-untouched.
     OPEN note: retail l4 renders a RIVAL-claimed sphere in the
     NEUTRAL family (sprite 56 with live class-3 owner 298) while
     mc2l0's human spheres color 105+size — the wizard spawn
     stamps ext color = slot for both (EF:43710), so the neutral
     mechanism is unresolved; conformance-invisible (sprite lane
     uncompared, rotation is size-only), port keeps team colors
     natively pending a colored-rival-sphere take.
  Rulings from the same round:
  - **§wander turn law = BYTE-EXACT, capture** (opus dig): turn
    clamp `sub_58350`, alt core `sub_580E0`, polar step, wander
    nudge, block-retry chain incl. the precedence quirk, move-
    then-nudge order, goat per-tick sound draw — all verified
    verbatim. The ±v_2/±341 heading blips are self-healing chaotic
    amplification through position-fed branches (rand streams
    match); the hypothesized 24-31-unit "binary branch divergence"
    DOES NOT EXIST (torus wraps + one t=17954 spawn wave). Two
    real leads split out: the HELD-STATE SPLIT (retail parks a
    goat in action 15 = +7 controlled, port wanders at 9 — a
    StageVar hold-gate miss, drives the sustained ±341 runs) and
    the (5,0)/(5,3) FLYER Z-BOB (±8..56 airborne offset — the
    multipart altitude source, untraced; NOT the walker path).
  - **§effects (10,0)/(10,6)/(10,14) = CAPTURE** (opus dig +
    substitution-split measurement): every fire/smoke motion law
    verified byte-exact (smoke `actSpeed−4 clamp[64,128]`, fire
    flicker `rand%0x41−32`, emitter `rand%0x4D` bonus). The
    "64-quantum" was the SHARED clamp; the one-sided spikes were
    100% slot-substitution rows. The standing "sub_580E0 alt-core
    arg order?" lead is CLOSED — exact, dead a4 correctly dropped.
  - **The lightning-trail TESTS were stale, not the law**: the two
    mc2_spell_channels lightning tests asserted live end-of-tick
    (9,9) nodes — under the certified born-dead law the trail
    decays within the cast tick (which node survives is pool-
    layout noise; they failed at the PRIOR commit too, hidden by
    cargo's per-bin fail-fast — run `--no-fail-fast` for truth).
    Re-asserted on laid RECORDS. ⚠ presentation question for the
    playtest: retail's crackle renders from the mid-frame draw;
    the port draws end-of-tick — verify lightning still reads
    visually.
  - NEW LEAD (out of family): the l4 (5,4) ARCHER walks at a
    CONSTANT −192 z from t=0 with byte-identical dynamics — a
    pristine-plane datum gap at its site, i.e. the LOAD-TIME
    terrain-edit question (the (14,5) plateau entry's l4 face),
    not an entity law.

- **MC2 CAVE AMBIENT RAND TAIL + the turn anchor (the mc2l30
  ">16 draws/tick" banked lead)** — RESOLVED 2026-07-31.
  ⚠ PARTIALLY SUPERSEDED by "MC2 CAVE RAND STRUCTURE, ROUND 2"
  above: the tail's EXISTENCE, LCG constants and full-counter
  addend stand, but this round's tick ORDER
  ([drip→tail→pass→baseline]), the pre-increment additive/drip
  anchor, and the "s = t" solve were artifacts of the TORN corpus
  — the clean re-record pins additive-last at the carpet's slot
  with the POST-increment counter. Level 30
  is a CAVE (`map_type: "cave"` + ceiling plane), and retail's
  carpet handler `sub_5D530` runs a cave-only tail (EF:59800-08):
  `rand_0x8 = 9377·rand + 9439 + counter` — a NON-LCG perturbation
  of the GLOBAL stream, once per carpet, which the port omitted
  (the runner bucketed the unreachable values as ">16 draws";
  they are a small step count + an additive). Three
  corpus-solved laws beyond the decompile: (1) the addend is the
  FULL per-tick counter (= the local player's Turn, reset at level
  load) — solved s=304/305/309/310 at ticks 304/305/309/310;
  remc2's `uint8_t setting_30` typing is refuted (the counter
  passes 255); (2) intra-tick op order solved from the recorded
  stream: [cave-drip draws (8th ticks)] → [carpet tail] → [frame
  pass draws] → [baseline ApplyEvents draw] — the human carpet
  updates in a PRE-pass phase (tail-first fits r=k−1 on ~all
  solved ticks; drip ticks fit (k=4,r=1)), so the MC2 baseline
  draw moved post-pass (count-preserving on non-cave takes —
  mc2l0/mc2l4 parity untouched, suites green); (3) the drip
  cadence anchors on the TICK-ENTRY counter (turn0&7==0; phase
  scan 442-vs-535+ rng mismatches per 2000). Wiring: the importer
  anchors `mc2_turn` from the recorded player Turn (also fixing
  the drip cadence which previously re-anchored to 0 every pair)
  and arms the carpet's byte[1]&8 one-shot stall skip (EF:59616 —
  the handler early-return that also skips the tail; the retail
  (1,1) stall pairs pinned it). mc2l30 rng mismatches 63% → ~22%
  (all residuals = churn-tick count mismatches, §l30-churn), first
  fully-conforming mc2l30 pairs appeared (t=6 promoted FIXED).
  Runner tooling: `--csv` now emits a per-pair `rng` row (retail,
  port) — the offline solver's input. ⚠ the ambient-loop SOUND
  gate (%0x83<5) reads the perturbed value without stepping it —
  presentation, owed to the audio layer, NOT the sim `sounds` vec.
- **MC2 m0 worm/hydra DEAD-BOB import lane** — RESOLVED 2026-07-31
  (the mc2l4 triage round's dominant family: 2.6M diff rows —
  (5,0) z/pitch/x/y/heading on 140 slots, every pair). The class-5
  arm of `import_ent_mc2` mapped port f26 ← retail @0x2E (the
  charm/armed lane, the mc2l0-era A/B choice), but the m0
  worm/hydra keeps its BOB VELOCITY in @0x10 (`dword_0x10_16`:
  the multipart ctor seeds it, `sub_1F040` integrates z += f26,
  f26 −= 5/tick, bounce +150 at terrain+256) — so every imported
  worm head had a dead bob and sank while retail undulated
  (corpus: slot 2 climbs +136/tick to ~2400, ~60-tick arcs, rand
  FROZEN — pure deterministic ballistics; the port's bob law
  already reproduced the arc exactly once seeded). Fix: the f26
  import is model-aware — (5,0) takes scratch10 (@0x10), other
  class-5 keep @0x2E (conformance.rs). mc2l4 z 899k→340k, x
  596k→100k, y 594k→98k, pitch 556k→51k, heading 516k→75k rows;
  all three prior suites green. RESIDUAL (5,0)/(5,3): smooth
  ±1..6 heading / ~±25 z accumulated drift (wander/bob phase
  detail, own open entry below). ⚠ the f26 dual-homing is
  per-MODEL, not per-class — m2's attack countdown and the
  doomsday timers are ALSO @0x10-homed in the port; if their
  families surface in a future corpus, extend the match arm, do
  not re-litigate the class-wide A/B.
- **MC2 castle PHANTOM-UPGRADE import lane (the mc2l4 build-out
  block)** — RESOLVED 2026-07-31, the MC2 twin of MC1's
  phantom-upgrade family. `import_ent_mc2` filled f59 from @0x3A —
  DEAD for (3,2) castles, whose build sub-state lives in @0x2E
  (`word_0x2E_46` → f59, docs/traces/mc2-castle-builder.md §2) —
  so every imported castle sat in f59=0 (level-up commit) and the
  port re-ran `mc2_castle_upgrade` each pair: level 1→2 → the
  HP/CAP ladder one level high (max_life 9375-vs-4687 = exactly
  `40000·Life60>>8` vs `20000·Life60>>8`; mana_max 18000-vs-8500 =
  CAP[2] vs CAP[1]), z frozen for the tick (the upgrade path never
  writes z → the rigid one-step rise lag on (3,2)), and one
  phantom (10,42) painter spawned per pair (the slot-304 squat
  where retail spawns a second rival castle head at t=5). One
  model-aware import remap (conformance.rs f59 ← @0x2E for (3,2))
  cleared max_life/mana_max/model/class/player.mana_max and the
  painter extras from the pairs-0..300 window; suites green.
  FOLLOW-UPS split out as their own entries (below): the (10,42)
  painter's parent @0x28 is NOT projected by obs_project_mc2
  (owner retail-297-vs-0 — the "@0x28 nonzero only on class-15"
  comment is false for painters), and the (3,3) stage-piece
  −128 z residual + the player.mana_max claim census are separate
  families to re-measure post-fix.
- **MC2 lightning trail-node born-dead law + phantom yaw stamp**
  — RESOLVED 2026-07-31 (the mc2l4 (9,9) window t=2517..8494:
  34k extra-in-port + 16k max_life + 14k life + 14k heading
  rows). The (9,9) swarm is the tier-0 Lightning beam's cosmetic
  trail (sub_66750 lays steps·8 sprite-216 billboards per cast,
  action 14 = sub_67410 pure pre-decrement decay). Retail births
  each node DEAD: `maxLife = (node_slot >= beam_slot) - 1`
  (EF:58341, so 0 ahead of the beam / −1 behind; life copied
  from maxLife; the ascending frame pass drives both to the
  disabled bit within a frame) and never writes the node's yaw.
  The port hardcoded max_life=1 (born-alive → 3 enabled frames →
  slot-recycle skew accumulating extras) and stamped the beam
  yaw into f30 (the heading family, retail 0). Both fixed in
  proj.rs (max_life encodes −1 as wrapped u32 — refill_life and
  the obs projection both cast through i32). Window t=2517+200:
  max_life 972→154, life 1087→273, yaw-stamp heading family
  gone; suites green. RESIDUAL (9,9)/(10,23) extras+missing =
  the input-delay-2 cast-timing skew + retail's parked ghost
  husks vs the port's free-list reuse — capture-domain, rides
  the standing input-latency + free-stack rulings.
  (docs/spell-audit/lightning.md §trail updated — the old "life
  1, self-despawning" note was the refuted reading.)

- **CASTLE COLLATERAL DAMAGE (the mc1hw playtest-round-2 chain:
  "Vodor tougher than retail" + "fast respawn")** — RESOLVED
  2026-07-31, opus decompile dig + corpus (mc1hwl0 slot 522, life
  20000→dead t=9457 at −833/tick; window 9325-9345 castle-life
  diffs 12→1 after the round; mc1l0 385 conforming UNCHANGED; all
  three suites green, L005 GOLDEN re-pinned A-E with OBSERVABLE
  holding — layout-only in that window). One chain, five laws:
  1. **+78 is SIGNED** in sub_118C0's z test (`ent_overlap`,
     `player_overlap`, the app-side `overlap`): the decompile
     types it `uint16_t` with a 32-bit `abs32` — a movsx artifact;
     the 0xE000 literal and the corpus overlap only reconcile as
     −8192. Port previously widened unsigned, so any entity with
     a negative z-center was orphaned from every AABB test.
  2. **Castle extents quad** (sub_37150 :43798, HW 40191-203):
     `+78=0xE000, +80/+82=((dim<<8)+1280)>>1, +84=0x4000` — now
     written at the level-up commit, the downgrade, castle_extents,
     AND re-applied in the settled tick's every-other-tick block
     (sub_46DB0 :52083, level VERBATIM) with the every-settled-tick
     `+144 = +24` owner echo (:52080). The port had deliberately
     skipped the marker ("would z-orphan our AABB overlaps" — true
     only because of defect 1).
  3. **Castles are homing-acquire candidates**: sub_54520's list-1
     walk (the significant-entity list: wizard models 0/1 + castle
     model 2) branches `+65==2` to the dedicated castle scorer
     sub_54BD0 in the base cases 0/3/4 (cone 0x71) and HW's meteor
     case 0x10 (cone 0x100) alike. The castle scorer is the
     generic scorer minus the sub_524C0 z-lift bracket (which
     itself skips model 2): castles are aimed at the RAW flag
     position. Ported into aim_assist_mc1_cone + the crosshair
     preview (Creatures set) + the victim-teleport lift skip +
     the AimLock alt. NOTE the sub_524C0 guard is MODEL-only (any
     class's model 2 skips the lift) — ported verbatim.
  4. **(10,53) cloud joins the class-10 PRE-decrement family**: 7
     burns from a 6-life cloud (pre-values 6..0), 5831 delivered
     per cloud — the corpus bursts are 7×833 (14 for two
     overlapped clouds), and the burst arithmetic is the proof
     (the decompile's C shows post-decrement; the batch law +
     corpus overrule it). Terminal act_life = −2, matching every
     class-10 ghost record in the corpus.
  5. **The sub_52770 explode stamps the child's +146 with the
     struck victim's SLOT** (:58859-64 `v20[73]`) — states 3/17
     ONLY; the m0/m1 explode blocks (:59015/:59092) write
     owner/yaw/pitch alone. First landed unconditionally, which
     put a foreign chase lane on (10,0) children of m0 explodes
     (suite drift caught it at t=355/366) — re-scoped via a
     stamp_victim parameter. Mechanically inert (no handler reads
     it; the cloud's damage is pure position overlap) but it is
     an observable lane.
  Death chain verified retail-equivalent: demolition clears the
  owner wizext `var_50` (:52598) where the port's `rival_castle`
  id24-scan needs no stored binding; the castle-less elimination
  (:55601-30) was already verified byte-identical. Intake law
  confirmed: the ch0 castle pre-pass gates ONLY model==2 + owner
  +24 differs + sub_11950 overlap — NO damageable-flag or
  +28-mask check for castles — and the general ch0 pass excludes
  (3,2) (both already ported). Gameplay effect: meteors aimed at
  a castle-camping rival now lock and fell the castle → castle-
  less death → ELIMINATION, replacing infinite camp-heal-respawn.
  PLAYTEST OWED. Banked adjacent leads: retail's list-1 walk also
  gates candidates on the OWNER's row v_28 rooted range BEFORE the
  scorer (port keeps only the scorer's 5120 — unexercised by
  corpus so far); base-MC1 napalm_tick never decrements act_life
  (retail does — inert under the 15-wave cap, but the obs lane
  drifts; fold into the banked base-17 +44-copy pass).
- **MC1 TICK-TOP REAP LAW (the castle-window "pool-order cascade" +
  the HW linger families)** — RESOLVED 2026-07-31, decompile-
  corroborated remc1:52226-31 / remc1hw:48276-81: retail has ONE
  unconditional reap pass at the TOP of every sub-step (after the
  LCG draw, before the awake build and dispatch) freeing every
  `class≠0 && flags&0x400` record via sub_41E90. Death paths only
  SET the flag (single setter sub_41E80 :52508, ~100 callers) or
  hard-free inline. Consequences that all fall out of the one pass:
  a record flagged mid-tick persists through that tick's snapshot
  (the delivered create-castle projectile's 0x406 one-frame linger;
  there is NO separate delivery latch — reap-before-dispatch IS the
  latch), corpse records persist MULTIPLE frames because the corpse
  HANDLER (sub_1A800 :21855-71) gates its own flagging on
  `f63 & 7 == 0` (the worm lanes), and same-tick spawns pop the
  PRE-EXISTING stack rather than the dying slots (the castle → 627
  = stack top, then the same-tick (10,42) painter → 481). The
  port's same-iteration free lost the linger AND recycled dying
  slots; the MC2-style next-frame deferral (the refuted 384→377
  experiment) re-ticked flagged records — the correct move was the
  FRONT of the tick, not the back. Landed MC1-scoped (native +
  strict; MC2 keeps its measured next-frame ghost law pending the
  owed sweep-law port). mc1l0 367→385 conforming (18 fixed, 0
  regressed; missing (10,0) 735→58, (10,12) 288→57, (9,1) 468→192);
  mc1hwl0 missing rows 717,798→33,379, phase-clock rows 67,378→
  2,257 (the (1,9) pattern was linger records vs respawned slots).
  Native goldens re-pinned as a BEHAVIOR change (flight-tier leg B
  both models; L005 GOLDEN+OBSERVABLE D-E — death records live one
  more snapshot and slot reuse shifts; post-init..C hold). The t=136
  mc1l0 capture fixture flipped conforming; t=470's phantom-castle
  atoms cleared.
- **§class15 manifestation aliasing + spellbook import** — RESOLVED
  2026-07-30 (take-2 fix round): `import_ent_mc2` now applies the
  cast.rs class-15 map (EIGHT fields — the ledger's seven plus the
  cadence flag `@0x3B → f59`, which gates rapid-fire): @0x2E→f26 ·
  @0x30→f28 · @0x2A→f30 · @0x2C→f44 · @0x36→f54 · @0x88→f136 ·
  @0x8C→max_life · @0x3B→f59; the projection reverse-maps heading=0,
  max_life=0, mana_max←max_life (measured constants; applied/speed/z
  ride through untouched). `action = 3·model` CONSTANT even across
  tier upgrades (measured — the "state" term never moves in this
  take), so the uniform tick70 lane round-trips. ALSO: the human's
  str_611 spellbook (banked/volatile XP @+0x649/+0x6B1, manifestation
  slots @+0x719, ring @+0x79B, levels @+0x803, sel @+0x81D — offsets
  validated against the pool roster) now imports per pair; before, the
  cast machinery ticked the WORLD-BUILD slots and the book's XP was a
  cross-pair leak of its own.
- **MC2 economy block: @0x1A id fusion + claim census + regen seed +
  castle echo** — RESOLVED 2026-07-30: retail's `id_0x1A` is the LIVE
  owner-or-self lane (census over the take: caster on projectiles,
  owner on castles/balloons/charmed (5,15), watch target on class-11,
  self elsewhere) while `parentId_0x28` is nonzero ONLY on class-15 —
  the fusion now imports `tr(owner28 ∥ f1a ∥ slot)` and the obs owner
  lane projects class-15-only (detached manifestations excepted).
  This fixed the claim census (`recompute_mana`: mana_max = 1000 +
  Σ f140 of claimed via f144/id24) → player.mana_max, entity
  mana_max, player_ent_idx, and the ball-claim stamps (bolt id24 =
  caster, not its own slot). `player.mana_delta` seeds from the
  carpet's @0x88 (the MC1 f132 law's twin). `player.castle` is now
  ECHOED from the recorded per-player word (+1080 = the AUTHORED
  castle binding; a runtime-BUILT castle never fills it — 0 across
  this take with the castle live; deriving it from the pool was a
  6,083-pair regression, briefly).
- **(10,0)/(10,6) fire aliasing + the activation bit** — RESOLVED
  2026-07-30: retail's `byte0&2` (the one-shot-done latch) imported
  only to the port's bit-25 mirror while the fire/explosion ticks
  latch on POSITIONAL bit 1 — every imported active fire re-ran its
  activation (area damage + flicker draw + scorch + sound) each pair
  (the fire-band rand churn, 19k→11k on the fix). The fire field
  map: @0x2A subSpellIndex = the area AMOUNT → f140, @0x2C = the z
  flicker/lift → f44, @0x90 mana lane dead-0 (projection override) —
  the uniform @0x2A→f44 alias fed the flicker a 400-unit constant
  (masked until the activation fix — the two-wrongs trap).
- **MC2 sweep laws (strict-retail scoped) + the ghost-record law** —
  RESOLVED 2026-07-30, measured on the take: (a) NEWBORNS never tick
  in their birth pass (the phase byte stamps at spawn; a fresh
  emitter particle surfaces at t+1 with life 32 and spawn z/speed
  untouched — the port's same-pass tick skewed all nine opening
  smoke columns every pair); (b) DISABLED entities (byte[1]&4) never
  run again, but their pool records PERSIST until slot reuse (the
  (10,1) death record sits a frame at life −2; the recorded obs
  carries ghosts, so the projection must too); (c) ghost slots are
  NOT in the recorded free stack — the next frame's remove pass
  pushes them (ascending scan; measured via the reused-slot ↔
  emitter mapping: 129←113 … 122←120, LIFO) — the importer appends
  ghost slots ascending atop the recorded stack; (d) ghosts NEVER
  tile-link (their link bit is stale bytes), and `new_event`
  defensively unlinks any still-linked record it reallocates —
  without (d), a reallocated linked ghost leaves a dangling chain
  pointer and the tile-chain WALK CYCLES: pair 9074 grew a 100 GB
  `area_write` victim list (use `ulimit -v` on full runs; `--start`
  + the per-pair announce found it in seconds). Laws (a)/(b) are
  STRICT-RETAIL ONLY for now: the native MC2 dome/eruption chain
  relies on the same-pass tick (the (10,19) summit column dies
  unspawned under the gate — mc2_slice caught it), so the native
  port of the sweep laws is OWED together with that timing fix;
  native goldens unmoved, DEVIATIONS.md entry added.

- **MC2 held-goat idle BLEAT draw (the mc2l0 rand family)** —
  RESOLVED 2026-07-30: retail's phase-7 goat wrapper
  (`AddGoat05_01_1F5B0` EF:11452) rolls the per-entity u16 stream
  once EVERY held tick (bleat on `% 0x4D == 0`); the port's held
  seam deliberately skipped the sound rolls (stagevars APPROX
  register), silently freezing every held goat's rand stream. The
  mc2l0 corpus measured it: 82,353 of 86,947 rand hits (95%) were
  held goats. `mc2_held_tick` now runs `goat_snd(i, 0x4D)` for
  model 1 between the 1D5D0 legs and the speed tail (retail order);
  rand family 86,947 → 3,874, first conforming MC2 pairs (0 → 7).
  MC2 slice goldens re-pinned (GOLDEN A-E + OBSERVABLE — a real
  behavior change toward retail; post-init holds). Other models'
  wrapper rolls remain skipped (APPROX, per-model transcription
  owed as §effects narrows).
- **MC2 importer wiring findings (landed with the importer)**:
  (a) class-9 projectiles must carry the port's `F_MC2PROJ` marker
  (bit 29, collidable bit cleared — the ctor convention) or they
  fall into the MC1 fallback arm and index MC1's 31-row BEHAVIOR
  table with an MC2 row (a panic, not a family); (b) the port fuses
  retail's own-id (`id_0x1A` = slot) and `parentId_0x28` into
  `id24` — import owner-if-nonzero-else-slot, project owner as 0
  when `id24 == slot`; (c) behavior rows derive from `ptr_a0` via
  retail's own load fixup `(ptr − base160@0x36DF6)/34 + 59`
  (validated: every live mc2l0 entity converts, creatures land on
  their model rows); (d) the free stack lives at top@0x35 (the
  0x242 dword is DEAD in remc2) + pointer cells @0x246, recycle
  @0x11E6/@0x11EA, allocation pops free-first (opposite of MC1's
  recycle-first) — g.free = recycle ++ free so the Vec pop matches.

- **Castle phantom upgrade (the settled-castle half of old entry
  3)** — RESOLVED 2026-07-30: retail castles keep their macro-state
  in the JOB byte +70 (4 settled / 5 transforming / 6 building,
  sub_46DB0/sub_46F10) with the transform sub-state in +48; the
  importer wrote retail's dead +59 byte into the port's fused `f59`
  machine, parking every settled castle in f59=0 = the level-up
  commit — one phantom upgrade per pair (stats one level ahead,
  1612 extra (10,42) painters, castle life reset). Importer now
  maps (3,2) f59 from (+70,+48); `castle_tick` case 4 additionally
  honors the retail upgrade-request bit (+16 & 0x40, :56007-11,
  cleared at commit :56475) and `castle_absorb` takes ONE ball per
  absorb tick (:56030-42). max_life hits 5736 → 695, mana_max
  3627 → 129, (10,42) extra 1612 → 11. The retail ladder
  (CASTLE_HP/CASTLE_CAP) was already correct. Retail castles have
  NO life regen (only the ladder snap + damage) — confirmed, and
  the port's case 4 has none either.
- **Mana-ball laws (old entry 2 + the slot-103 insta-kill)** —
  RESOLVED 2026-07-30 from sub_27030 (:29416-571) + sub_54F80
  (:64318-20): a ball is ballistic — gravity, grounded downhill
  roll (sub_41F50 = the 2×2 forward difference), 250/256 friction,
  and the grounded-only MERGE scan — only while its +58 settle
  countdown (ctor 0x80, −1/tick via the global anim pass) is
  nonzero; at 0 it freezes at rest FOREVER (no TTL — max_life 300
  is inert). Retail's merge donor is HARD-freed (sub_41E90), gone
  from the same snapshot. The port merged/rolled resting balls
  forever (a settled ball beside the castle was re-merged on every
  pair for 3000 ticks, timeline-matching spawn+128), ran MC1
  friction unconditionally with no roll (contradicted by its own
  cite), and soft-killed donors into extra-in-port rows. All
  MC1-scoped; MC2's sphere twin untraced and untouched. Ball x/y
  hits 56k/62k → 9.7k/9.7k, (10,39) missing 1621 → 315. MC1
  goldens re-pinned (behavior change by design; OBSERVABLE moved
  A-E, post-init holds).
- **Jar pickup under strict_retail (the t=11 first divergence)** —
  RESOLVED 2026-07-30: the strict arm was fully inert, so retail's
  jar-pickup poll (sub_55A40 :64729-872 — every-4th-tick, AABB,
  grant = in-place convert to the owned token + LEFT auto-equip +
  the jar's own bit0 stamp; already-owned = pure no-op, NO
  jar→mana path exists) never ran. Ported into the strict arm with
  retail's encoding (tick70 = spell*3). The old "port converts
  pickup to a mana ball" reading was wrong — the extra (10,39) at
  t=11 was retail's grounded ball-merge hard-free (see above).
- **Village-tree reap ("(2,0) hut" family)** — RESOLVED 2026-07-30:
  the reaped entities are TREES (class-2 model-0; retail huts are
  class-10 model-45, ctor sub_3B690 :47501-18). Retail's village
  construction PAINTS tile types under them (sub_27D30 :30184-248);
  on pristine replay planes those tiles still read water and the
  tree's own splash-die (:57703-11) fired in one tick. Strict-retail
  now suppresses the tree water arm (capture-domain, same pattern
  as the class-12 frozen-z law). 1960 rows → 53 (the completion
  retile edges, entry 9). Gameplay unchanged.
- **player.mana regen seed** — RESOLVED-as-import 2026-07-30: the
  importer now seeds `player.mana_delta` from the carpet's +132
  (the applied-then-recomputed pipeline both engines share). The
  remaining divergence is entry 5's cadence gap (port every-tick vs
  retail ~every-4th) — the family flipped sign from +100 (missing
  regen) to −100 (over-regen).

- **(10,2)/(10,3) puff reaping (the bulk of old entry 1)** — RESOLVED
  2026-07-29: ctors (`str_255D0C[2/3]` = `sub_3A570`/`sub_3A5D0`) and
  tick handlers (`str_255998[2/3]` = `sub_252B0`/`sub_253F0`, bare
  pre-decrement) ported un-gated (generic MC1 code, HW just exercises
  it). Missing (10,2) entity-ticks 1090-scale → 39 (only the unported
  speed-token emitter remains).
- **(12,1) loss (162-scale → 0)** — RESOLVED 2026-07-29: those were
  rivals' class-12 owned-spell TOKENS (retail encodes tick70 =
  spell*3+phase; state 3 = the idle HEAL token), which the port's
  DROPPED_JAR=3 decay reaped. Under `strict_retail`, imported
  class-12 entities follow retail's law (inert; active handlers
  still open) — docs/traces/mc1-class12-spell-tokens.md. Phase-clock
  disagreements 224 → 44 per 289 pairs; MC1 conforming pairs rose
  32 → 34.

- **HW systematic z −64 (the class-12 half of old entry 7)** —
  RESOLVED 2026-07-29: the port re-snapped resting class-12
  jars/manifestations to ground every tick (`class12_tick`), an
  unregistered cosmetic workaround; retail's terrain-reshape walk
  re-snaps class 2 and kills class 5 but default-skips class 12
  (remc1/remc1hw `sub_40E20_41160` :51745-65), leaving jars
  hovering/buried at their spawn z — confirmed by the recording
  (slot 161 holds z=3408 for hundreds of ticks over lowered
  ground). True sign was port-LOWER, magnitude the local terrain
  gap (64/80/256), not a uniform datum. Resolution (player-ruled):
  the snap STAYS for gameplay — it is what keeps HW's authored
  jars pickable and earthquake aftermath grounded — but is now a
  registered deviation (DEVIATIONS.md "World::class12_tick (jar
  ground-snap)") disabled in strict-retail mode: `retail_import_mc1`
  sets `World::strict_retail`, under which imported retail worlds
  evolve by retail's frozen-z law. Tests pin both behaviors;
  goldens unmoved; HW z hits 5189 → 1349 (all class-12 diffs gone;
  the rest is entry 7's terrain shortfall).

## The known-deviation roster (2026-07-31)

`conformance/known-deviations.json` + `verify-deltas` classification
(docs/CONFORMANCE.md §roster): every diff row is tagged against
scoped, ledger-cited rules (capture / deviation / open) and the
report's headline is the UNEXPLAINED residue + per-rule hit counts;
`--csv` carries the rule id per row (`--no-roster` = raw). Seeded
from this ledger's ruled families (33 rules). The player-stated
goal: a fully triaged take runs to unexplained = 0 — everything
conforming or known. Baseline at seeding (2026-07-31, post-
kinematics-round): mc2l0 **5,242 of 7,762 pairs conforming-or-
explained**, 7,512 unexplained rows (gross was ~300k); mc2l4
8,398/12,786 + 14,136; mc2l30 3,434/10,021 + 13,878; mc1l0
1,196/5,329 + 44,523 (the walker x/y/heading terrain knock-on is
DELIBERATELY unexplained until a terrain channel exists — only the
direct z family carries the ledger's whole-take ruling); mc1hwl0
800/40,586 + 2.17M (only the z closure seeded — the §weather/
token/census families await their own triage rounds). Notable: the
roster instantly SIZED the undug (3,3) balloon-z lead at 40k rows
on l4 / 19k on l30 (tagged open, not hidden).

**Capture-window clarification (player question, same day)**: the
read-consensus scheme (N byte-identical neighboring reads ⇒ the
guest is between ticks) IS the recorder's mechanism — but identical
reads prove only that the guest was FROZEN, and DOSBox regularly
parks MID-entity-loop, so a perfectly stable consensus image can be
a mid-tick state (RECORDING.md "Capture tearing" — the original 75%-
torn corpus). Higher snapshot frequency cannot fix this (it is an
alignment problem, not a sampling-rate problem); the by-construction
fix is the tickpatch MAILBOX window (`in_window` raised during the
pacing spin = a guaranteed quiescent window), which is why mc1l0
runs 0 torn. The MC2 takes run the pacer but not the windowed
mailbox — they fall back to the phase-byte tear gate (~33% torn,
plus the per-entity torn-slot exclusion). **The owed MC2 tickpatch
mailbox/emit gate would reclaim those pairs the same way —
PLAYER-APPROVED 2026-07-31 as the next session's headline ("the
final piece for proper recording"): a NETHERW_REC.EXE arm hooking
MC2's OWN frame limiter (no pacer needed) at the true frame
boundary — after the post-pass ApplyEvents baseline draw, before
the next PlayerEvents Turn++ — so the Turn++-park tear mode
becomes unobservable by construction. New mailbox magic + its own
window-open counter (Turn advances mid-frame, unusable as a
continuity token); recorder grows the MC2 windowed path. Pays on
re-recorded takes only.**

## Capture caveats (not port bugs)

- Pre-gate recordings: mid-pass tearing (75% of mc1l0 pairs) — see
  RECORDING.md. The runner's `capture_clean` re-classifier is
  authoritative for old files.
- The human carpet's +63/rand/flags have no port counterpart (the
  human lives outside the pool); the comparator restricts the pinned
  slot to life/mana fields.
- `owner_ptr` (guest pointer) is never compared; behavior rows are
  compared via the derived index (`(ptr − base)/32`, base anchored on
  the carpet's canonical row 7).
- **MC2 tear law (measured, supersedes the old "Turn + LCG parity"
  guess)**: Turn advances on EVERY adjacent pair (it increments in
  `PlayerEvents` BEFORE the entity pass) and the global LCG draw
  count is activity-dependent, so neither discriminates. The gate is
  phase-byte step-1 DOMINANCE (`byte_0x3E_62`, RECORDING.md); 1105 of
  mc2l0's 3640 pairs (30%) are torn. WITHIN accepted pairs,
  minority entities can still be individually torn (0- or 2-pass) —
  the runner excludes them from field comparison per slot
  (`verify_mc2::torn_slots`); their signature was the perfectly
  balanced ± families (life ±1, z ±64, speed ±4, y ±30) that no sim
  law produces. A recorder-side MC2 emit gate is still owed.
- **MC2 input closure (mc2l0 §casts)**: the 2026-07-29 take carries
  `channels.input: "none"` — the human's casts are invisible
  (control commands consumed+zeroed mid-tick). Every human cast
  surfaces as missing (9,x) projectiles + player.mana spend families
  (fixtures t=425/1410, `capture`). **CLOSURE FIX LANDED 2026-07-30**
  (recorder-side, no exe patch): the MC2 raw-input register frame
  (held buttons + press latches + cursor + cursor-at-press +
  pressedKeys) is now mapped and validated — RECORDING.md "input" —
  and `verify_mc2`/the fixture loop consume it (`fire = held ∥
  latch` through the `--input-delay` ring). A RE-RECORDED take gets
  the channel automatically; these fixtures stay `capture` for the
  old take and retire with it.
- **MC2 terrain closure (mc2l0 §terraform)**: village growth
  terraforms the hill under the (157..173, 205..209) house cluster
  at ~t=751; house ticks re-snap z to terrain both sides, so every
  later pair shows the (10,45) z family against the pristine plane
  (fixture t=1447, `capture`) — the MC2 face of the mc1 ledger's
  dominant TERRAIN CLOSURE residual. Same fix direction: a terrain
  channel in .mgcr v2.

## mc1:49 — the map "O" that triggers nothing: RULED FAITHFUL
## ⭐ RETAIL-CONFIRMED BY PLAYER 2026-08-04 (ruling closed, top tier)

Player report (2026-08-03, MC1 level 049, the last campaign level):
after the final genie wave dies an "O" map marker appears, attached
to nothing — flying over it does nothing. Investigated without a
recording (decompile + level data only). **Verdict: the port is
byte-faithful; retail does the same. No fix, no deviation.**
**Player replayed the level in retail (2026-08-04, cheat-assisted)
and confirmed: the O appears there too, inert — a known community
oddity, apparently a Bullfrog placeholder for an ending sequence
that was never built. Ruling stands at the highest evidence tier
(disasm + retail replay agree); do not re-open.**

**The level's trigger graph** (`baked/mc1/level-049.mgcl`, 114
class-11 THINGs — the densest in either game):

- 104 x (11,0) proximity one-shots, whose dispositions hold 103
  (10,9) growing-hill/volcano creators + 27 (5,6) creatures. This is
  the "most triggers spawn volcanoes" the player saw.
- (11,6) @ (123,152), box 64 — the leave-polarity one-shot that
  opens the level: fires dis 1 = the main wave (19x(5,2), 4x(5,3),
  14x(5,5) crabs, 17x(5,8), 10x(5,16), 1x(5,6), 1x(5,9)) plus four
  kill triggers.
- Kill chains (state = 13 + watched class-5 bucket; state 30 = the
  -1 "all buckets" variant):
  (11,15)@(45,159) bucket 2 -> dis 101 -> (11,15)@(57,74) -> dis 102
  -> (11,21)@(90,31) -> dis 103 = EMPTY;
  (11,18)@(134,251) bucket 5 -> dis 46 = 8x(10,52) crab eggs;
  (11,21)@(32,222) bucket 8 -> dis 6 = 9x(5,8);
  (11,30)@(230,86) ALL -> dis 87 = 5x(5,11) GENIES + (11,24)@(118,85)
  -> dis 106 = 6 more genies + (11,24)@(153,128)
  -> dis 107 = **the (11,31) at tile (0,0)** -> its own dis 108 has
  ZERO member THINGs. Terminal.

So the O is authored, placement is faithful (LEVELS.DAT entry 49,
sha `b6d6c6ff…`, slot 1633, x=y=0 -> the map's corner), it appears
exactly when the last genie dies, and even a hypothetical trip could
spawn nothing because disposition 108 is empty.

**Retail law for class-11 state 31** (remc1 + CARPET.EXE, both):
`str_256038[31]` (remc1 sub_main.cpp:4953) is a LIVE entry
(`data4 = 0x1F`, `data10 = 1`) pointing at `sub_5A080`. In
CARPET.EXE that data row sits at VA 0x981EA (`F4 68 00 00 1F 00 80
A0 04 00 01 00 00 00`) and `sub_5A080` is **one byte: `C3`** — the
state-30 thunk at 0x5A070 falls through a `90` pad into that shared
`ret`. The dispatch site (VA 0x41A0A-0x41A7C: `movsx ecx,[ebx+0x46]`
/ `imul edi,ecx,0x0E` / `call [eax+0x6]`) has **no state bound
compare** — index 31 is genuinely dispatched, and does nothing.
Whole-image scan: the 12 callers of the proximity probe 0x5A090 are
states 0-3/5-12, the 18 callers of the kill helper 0x59E40 are
states 13-30, and 0x5A080 has **zero** callers besides the table
slot. WAV 41 (inside the probe, at 0x5A0E9) is therefore unreachable
from state 31. Model 31's only other consumer in the entire binary
is the map draw `sub_48710` (model jump table VA 0x4868C: models
9-12 -> sprite 83 "X", model 31 -> sprite 84 "O", all else nothing).
MC1 has no exit-marker win path at all — the level ends through the
mana-share latch `sub_415C0` (bit 1 of +13325) — so MC1's O is NOT
MC2's ending switch despite sharing the sprite and the model number.

**Port**: `World::trigger_tick`'s `_ => {}` arm == retail's `ret`
(the dispatcher's `f63++` at :52406 is applied by the caller for
every entity, so even the phase clock matches);
`World::advertised_marker_poses` plots models 9..=12|31 exactly like
case 0xB. Comments at both sites now carry the proof.

**Retail-replay checklist** (to confirm on the player's next run):
the O should appear at the moment the last genie of the second
(11,24) wave dies; it should sit in the map's (0,0) corner, i.e.
diagonally opposite/wrapped from wherever the player is, never
moving; flying through that corner should produce NO chime (sound
41), NO spawn and NO level end; and it should persist unchanged
until the level is won on mana share. If retail instead chimes,
spawns, or ends the level there, `str_256038[31]` is being reached
by some path this dig did not find — reopen.

## SESSION-9 LANDING ROUND, BUNDLE 4 (2026-08-05): fool's-mana OPEN-6/OPEN-7, the hate-decay from-binary check, the Vissuluth wake metric

Four backlog items, one dig. Two closed as NO-BUG with citations, two
landed; the only corpus mover is OPEN-7, and it moved the corpus the
right way.

### 1. OPEN-7 — the chord march probed sub-steps retail never visits, and it WAS costing us pairs

Retail's flight states run the victim probe ONCE, at the END of a full
step (`sub_65C20` EF:63126-29: MoveEntity → CopyEntityPosition →
`sub_10780`). Our anti-tunnel march walks the chord in ≤128-unit
sub-steps and probed **every one from the muzzle out**, so a projectile
born co-located with a targetable entity it does not own detonated on
its first sub-step.

**Landed law** (`crates/mgc-sim/src/mc2/proj.rs`, `mc2_flyer_tick` +
the new `mc2_hit_covers`): a victim whose box already contains the
step's START is admitted only at `k == n`, retail's own probe point.
Mid-chord ENTRIES still detonate at the sub-step, so anti-tunnelling is
intact. Chosen over "skip `k == 1`" because a PARKED projectile's only
probe IS the endpoint and retail detonates that one — which is why the
existing pin
`fools_trap_bolt_leaves_from_the_sphere_box_top_and_clears_its_own_muzzle`
keeps its contrast arm unchanged (it pins the endpoint LAW, not the
residual). New pin:
`a_projectile_born_inside_a_foreign_box_flies_clear_of_its_muzzle`
(engine/world.rs). A/B toggle `MGC_NO_MUZZLE_ADMISSION=1`.

**The audit called this latent. It was not.**

- **mc2l0 0+2000: 1703 → 1704 conforming.** The whole delta is t=618
  slot 165, a (9,1) possession bolt the port blew up in its own muzzle:
  `life 2` vs retail 3, position frozen at 82/180/3616 instead of
  retail's 84.57/181.94/3335, plus a phantom (10,12) possess flash at
  slot 123. `--csv` row diff: **5 rows removed, 0 added.**
- **mc2l24 51500+600: pair verdicts unchanged** (51 conforming / 549
  field-diff / 27 explained, both arms). At t=51500 a (9,3) stops
  self-detonating (slot 720 life/x/y/z all become retail's) and three
  phantom (10,0) impact puffs disappear; entity-set extras 284 → 281.
  Downstream the freed slots reshuffle the free list and the row detail
  wobbles (+8 field, +2 missing) in an epoch whose FIRST pair was
  already deeply divergent. Net entity mismatches 387 → 386.

### 2. OPEN-6 — the native fool's sphere wore the wrong model, and four gates were on the wrong side of it

`spawn_mana_ball` stamps `model65 = 39` for the whole MC2 sphere line,
so a natively-spawned (10,57) read model 39 where retail's `sub_50130`
builds a real model-57 entity. Now stamped
(`crates/mgc-sim/src/mc2/effects.rs`); the action-62 discriminator
stays as belt-and-braces. Full gate audit —
`docs/spell-audit/fools-mana.md` §7, table of eleven laws with cites.

The organising fact: retail's class-10 chain `dword_38523` is built
from models **39, 40 AND 57** (EF:40023-40062). Laws that walk it with
no model test include m57; laws that test `model == 39` exclude it; the
census is a third thing.

Port changes the stamp forced:

- **awake pass** (`mc2_awake_pass`, mc2/mobs.rs) — retail's sphere loop
  has no model test (EF:55489); 57 ADDED, else native fools stop waking.
- **mana-magnet aura** (`mc2_aura_tick`, mc2/tail.rs) — no model test
  either (EF:28362); 57 ADDED. (Model 40 rides retail's chain too and
  the port has never pulled it — pre-existing residual, noted in place,
  deliberately not changed here.)
- **world-mana census** (`recompute_mana`, engine/world.rs) — retail's
  MC2 census `sub_61F50` is a MODEL SWITCH: 39 and 58 count, 45 banks,
  **everything else falls through** (EF:62012-35). So (10,57) never
  enters the type-0 castle-share denominator — cast decoys AND authored
  ground spheres alike. The port's decoy-only special case is deleted;
  the match list is the filter. (§3 of the audit said authored spheres
  "keep counting exactly as they did" — that was wrong against retail.)
- **possess whitelist** (`claim_admits`, mc1/combat.rs) — the `(10,57)`
  arm read `f40`. Retail reads `parentId_0x28` (EF:3846), whose port
  home is `id24` (the importer's `owner28` fuse). Invisible while only
  IMPORTED spheres reached that arm (both lanes read 0); with the model
  native it would have let a caster's own possess bolt detonate on his
  own trap. Lane corrected.
- **castle absorb** (mc2/castle.rs) needed no edit but had a live bug
  the stamp fixes: retail filters `model != 39` (EF:61105), so a native
  fool's sphere touching a castle used to be eaten as real mana.
- **rival mana hunt** already walked 39/40 then 57 under the Perception
  break (EF:6544-49) — the native m57 was simply in the wrong pass.
- **map dot** (mgc-app/src/entities.rs) keeps (10,57) on the (10,39)
  arm: a decoy that looks different is not a decoy.

**Corpus: byte-identical** on both windows (proved by running the whole
change set with `MGC_NO_MUZZLE_ADMISSION=1` — output matched the
pre-change baseline exactly). Expected: `verify-deltas` rebuilds
entities from the recording, where an m57 already carried model 57, so
only NATIVE play is affected.

### 3. Rival hate decay — FROM-BINARY VERDICT: **SANE. remc2's shifted index is a decompiler artifact.**

remc2 EF:5377-93 writes `array_0x1FC_508[4·i+4]` from
`array_0x1FC_508[4·i]` — eight bytes lower, i.e.
`hate[p] = agg + 1 + hate[p−1]`, an accumulator that would leak hate
across pairs. Disassembled the shipped NETHERW.EXE (`sub_12A70`, linear
0x12A70 → file 0x37270 by the banked LE recipe `0x34800 + (linear −
0x10000)`; pristine copy at
`/home/rain/games/dosgames/carpet2/patched/netherw.exe.orig`):

```
12AE6  lea  ecx,[ecx*8+0x0]        ; 8·i
12AF6  lea  esi,[ecx+eax]          ; playerRec + 8·i
12AF9  mov  cx,[esi+0x204]         ; READ hate[i]
12B00  cmp  cx,0x601f
12B05  jnc  .above                 ; unsigned >= neutral
12B07  mov  ax,[eax+0x242]         ; aggression — per-PLAYER, NOT indexed
12B0E  inc  eax
12B0F  add  ecx,eax
12B11  mov  [esi+0x204],cx         ; WRITE hate[i]  ← SAME element
12B23  cmp  word [eax+0x204],0x601f / jna / mov 0x601F   ; clamp DOWN
.above:
12B54  cmp  word [esi+0x206],0x0   ; war flag → pin
12B5E  mov  ecx,0x100 / sub cx,[eax+0x242] / sub [esi+0x204],ax
12B85  cmp  word [eax+0x204],0x601f / jnc / mov 0x601F   ; clamp UP
```

Read and write are the same element; both compares are unsigned and
strict. remc2's `[4·i+4]` for hate and `[4·i+5]` for the war flag are
BOTH right (0x204+8i and 0x206+8i) — only the right-hand-side operand
is mistyped. `mc2_rival_hate_decay` (mc2/rivals.rs) already implements
exactly this. **No port change; annotated with the disassembly.**
`cargo test -p mgc-sim --test mc2_rivals`: 17 passed.

### 4. Vissuluth wake gate — NO-BUG, metric pinned

`Maths::EuclideanDistXYZ_58490` (Maths.cpp:738) is
`radix = (int16)(dx)² + (int16)(dy)²` and nothing else — **Z is never
read**, confirming the banked "the name lies" trap; and it is a true
2-D EUCLIDEAN, not Manhattan: the return is
`sub_7277A_radix_3d(radix)` (Maths.cpp:744), a Heron integer sqrt
seeded from `x_WORD_727B0[bsr]` terminating on `radix / i >= i` — an
exact FLOOR sqrt. So retail's `>= 0xA00` and the port's `>= 0xA00²` are
the same predicate, boundary included. `doomsday.rs` already had the
squared 2-D form: **the session-7 "already faithful" ruling stands.**
Comment now carries the derivation; arithmetic widened to i64 for the
same reason retail accumulates into a `uint32_t` (two i16 legs reach
2³¹ and the i32 form wrapped negative there). Last Vissuluth crumb —
closed.

### Suites

`MGC_REQUIRE_GOLDENS=1 cargo test -p mgc-sim --no-fail-fast`: **0
failures** (342 lib + all integration; the three fool's-mana channel
tests updated to count (10,57), which is the OPEN-6 pin). Workspace
minus mgc-conform: 0 failures. `cargo fmt --all --check` clean; clippy
warnings all pre-existing (probes.rs / roster.rs doc lists).

`mgc-conform fixtures conformance/*.json`: **0 regressions** on all six
manifests. The 9 FIXED + 9 drifted fixtures it reports are
**pre-existing, not from this dig**, on three independent grounds:
(a) re-running the identical command under `MGC_NO_MUZZLE_ADMISSION=1`
gives byte-identical output, so OPEN-7 moves no fixture; (b) five of
the nine fixes are MC1 (mc1l0 t=112/178, and mc1hwl0's drift), and
every remaining change in the bundle is MC2-only — `model65 = 57` and
`tick70 = 62` are stamped in exactly one place, `mc2_spawn_mana_sphere`,
so the `(10,57)` claim arm and the deleted census `tick70 == 62` test
are unreachable on the MC1 column; (c) the two mandated windows are
byte-identical to the pre-change baseline under that same toggle. The
manifests are stale relative to the uncommitted session-8 work.
**NOT promoted** — the promote decision is the orchestrator's.

## THE EYE LIFT — "docked at my castle I sit lower than retail" was a MISSING +128 IN THE CAMERA (player report 2026-08-05, FIXED)

**⭐ PLAYTEST CERTIFIED (player, 2026-08-05, same day): "Eye-height
playtest confirmed, it looks much better in all situations."**

**Symptom (player, native MC2 side-by-side vs retail):** parked on their
own castle the port puts the view CONSISTENTLY LOWER than retail, while
ambient creature/guard placement reads 1:1 exact. Three hypotheses were
offered; the corpus + both decompiles convict the third, in its
player-specific variant.

**RETAIL LAW, measured then read.** The corpus pins the sim half:
`mc2l0` parks the human at z **256** over sea-level ground for
t=5683..5758, and its spawn pose is z **5024** over a 149-byte cell
(149·32 + 256) — clearance **256**, exactly `sub_5D530`'s floor
`z = getTerrainAlt + word_160_0xc_12` (EF:59768). `mc1l0`'s spawn pose
is z **2080** over a 61-byte cell (61·32 + 128) — MC1 clearance
**128** (:55151). The castle mound is ordinary terrain under that law:
`mc2l0`'s human castle at tile (48,34) walks its z 1644 → **2336**
across t=2564..2583 as its (10,42) painter stamps the BUILD00 pad, and
holds there — 2336 = 73 height bytes × 32 IS the pad top (the castle
entity re-pins to live ground every tick, both games). `mc1l0`'s own
castle at (117,101) does the same over t=562..607: z 1022 → **2656** =
83 bytes × 32, then flat.

The half the port never had is the RENDER half: **retail hands its
world draw `axis.z + 128`, never the raw carpet z** — MC2
`DrawWorld_411A0` (remc2 EventsFunctions.cpp:21575, mirrored :21606 /
:21868 / :21899), MC1 `DrawWorld_30D90_30DD0` (remc1
sub_main.cpp:26406, :26589). Same literal, both games. The per-frame
view record is otherwise a verbatim copy of the entity position
(EF:40250-54 — it even calls `getTerrainAlt_10C40` and throws the
result away), and `array_0x52_82.fov` (the head clearance 100) never
touches the camera. So retail's docked eye over that mound is
`2336 + 256 + 128` = **2720**; the port rendered from **2592**, a flat
half-tile low — everywhere, but only judgeable where a structure of
known height stands next to you, which is why the castle dock is where
the player saw it.

**The other two hypotheses, refuted.**
- *Castle taller than its bounds* — NO. The visible castle is painted
  terrain in both engine and port; the only drawn (3,2) art is the
  owner's flag billboard, anchored at the entity z, which
  `castle_tick` re-pins to `ground_z` every tick
  (features.rs:3639, mc2/castle.rs:105-172). The terrain MESH samples
  the same plane at the same scale (`HEIGHT_SCALE` 1/8 =
  32/256, terrain.wgsl:121) with matching triangulation parity, so art
  and collision share one datum. Guards standing right is not a
  coincidence — there is no second datum for them to stand on.
- *Per-resolution perspective* — NO. `GameRenderOriginal` / `NG` / `HD`
  are constant-identical in every projection field (camera z, screen
  centre, focal `7·isqrt(W²+H²)·fov >> 11`, horizon `pitch·W >> 8`);
  NG/HD only parameterize fog and draw distance, and reproduce Original
  exactly at scale 1. There IS a real resolution effect, but it is
  ASPECT, not a constant: focal scales with the diagonal while the
  screen centre scales with H, so with the default `fov` 128
  (EF:38163) the vertical FOV is ≈62.3° at 320×200 (16:10) and ≈68.7°
  at any 4:3 hires mode. It rescales the whole picture, creatures
  included, so it cannot produce a player-only offset — and the port's
  fixed 60° sits within ~4% of retail lores. BANKED, not landed:
  deriving `FOV_Y` from retail's aspect formula would close the last
  few degrees.

**Landed (presentation layer, hash-quiet, zero sim law touched):**
`mgc_sim::EYE_LIFT = 128.0 / 256.0` (crates/mgc-sim/src/lib.rs:47-68,
carrying the citations), applied at the one live-gameplay camera —
`crates/mgc-app/src/lib.rs:5464-5480` (`y: carpet_y + EYE_LIFT`,
:5474). The
`Flyer` pose stays the CARPET plane deliberately: it round-trips
through `sync_carpet_from_flyer` and feeds the world its pose, so the
lift belongs to whoever builds the camera. The debug coordinate
overlay (lib.rs:6142) backs the lift out again so its floor/band
readout stays carpet-relative.

Test `docked_on_a_castle_pad_floors_and_lifts_the_eye`
(flight.rs) pins the whole chain on the measured mound: MC2 floors at
2336+256 and renders from 2720, MC1 at 2336+128 and renders from 2592,
with a non-vacuity clause on the 128 itself and on the two games
docking at different heights. No golden moved and no fixture is
reachable — the conformance harness pins the human pose from the
recording and compares sim fields only, so a camera constant is
invisible to it (no probe run, none applicable).
**PLAYTEST OWED:** docked on your MC2 castle the view should now sit
half a tile higher — same carpet, same mound, eye raised 128 engine
units; MC1 gets the identical lift (its carpet floors half as high, so
the change is proportionally more visible there).

## VISSULUTH'S SUMMONS — THE "FROZEN HUSK FOREVER" IS A PORT BUG: ONE `return` SKIPS RETAIL'S ESCAPE HATCH — **DIAGNOSED AND LANDED 2026-08-05, PLAYTEST OWED** (l24 windowed unexplained −208 / −245, `sv2` mismatches → 0, no golden moved)

Player report (sharpened 2026-08-05): on mc2l24, Vissuluth's summoned
creatures killed at a specific very early moment after flying out
remain behind as frozen husks — forever. Cyclone shoves them (physics
still applies) but they never move or die. Incidence dropped after the
session-8 latch-home fix; in the last playthrough it happened ONLY to
"fireflies". Player adds: **they have never seen this in retail**,
across substantial play of the level.

**VERDICT: (b) THE PORT DIVERGES.** The ledger's earlier reading —
"the standing husk is retail law" — was half the law. The latch write
is real, but retail has an escape hatch on the very next statement and
the port `return`s past it. The corpus contains **no standing-husk
specimen at all**: every doom summon that dies in the recorded take
dies in an ordinary model death state, never in the summon slot. The
player's "never seen it in retail" is exactly right.

### The latch lifecycle, both sides

Retail `sub_1E580` (EF:10689-10746) is the StageVar2 13/16 body; a2 =
`8 * model` (`sub_1D5D0` call sites, EF:11356/11454/11567/…). Its
target-valid branch is:

```c
sub_1E700(a1x, a2);                                    // EF:10734
if (!(a1x->byte_0x3E_62 & 7)) {                        // EF:10735
    v4 = a1x->dword_0xA0_160x->word_160_0x1c_28;       // EF:10737
    if (sub_583F0_distance_3d(&a1x->position, &v3x->position) < v4)
        a1x->actionIndex_0x45_69 = a2 + 2;             // EF:10739
}
```

`sub_1E700` (EF:10753-10871) is the shared damage head plus three
arms:

- **v2 == 0** (quiet) — move core `sub_1B8C0`, face target, crowd
  steer-away (EF:10806-40).
- **v2 == 1** (damaged, survived) — move core, then RETARGET: lock
  the attacker and hand to the model's `+2` (or `+6` for flee rows),
  plus the parent-XP mail (EF:10844-61). **This leaves the doom lane
  permanently** — `8m+2 & 7 == 2`, so the next tick dispatches to the
  model's own handler, whose head sends `life < 0` to `a2 + 4`, the
  ordinary death animation (EF:9008-9013 and its 30-odd twins).
- **v2 == 2** (dead) — `word_0x2E_46 = 1` (EF:10864-66). No state
  change, no move, **no early return.** Control falls back to
  EF:10735, so a DEAD husk still runs the engage check and still
  converts to `a2 + 2` the moment it is inside `v_28` of its target.

So retail's husk is a ≤ 8-tick freeze (the `byte_0x3E_62 & 7`
throttle) whenever the player is within the row's engage reach, and
the reach is enormous: `v_28` = 5120 (20 tiles) for m0 and m19, 4608
for m21, 3072 for m25 (behavior.rs rows 71/88/96/92). In a fight
fought inside that radius the husk is invisible; the creature simply
freezes for a beat and then plays its death animation.

The port (`crates/mgc-sim/src/mc2/mobs.rs`,
`mc2_doom_summon_home_tick`) mirrors all three arms — and then:

```rust
match self.mc2_state_head(i) {
    2 => {
        self.ent[i].f26 = 1;
        return;                      // mobs.rs:2598  ← THE BUG
    }
    ...
}
// The engage handoff (EF:10735-40) — mobs.rs:2659-2669, unreachable
// from the dead arm.
```

`f26 = 1` and "do not move" are both correct. The `return` is not in
retail. With it, a port husk can never leave `8m+7`, so it stands
until the parent scan (mobs.rs:2552-58) fails — i.e. until Vissuluth
himself dies (t=63221 in the corpus). That is the player's "forever".

### Corpus specimens (recordings/mc2l24.mgcr) — retail never husks

`f2e` in the trace output is `word_0x2E_46`, the latch; `flags` bit 10
(0x400) is retail's `byte[1] & 4` teardown bit.

- **slot 573, (5,0) worm.** Spin-up t=60104-60140 (`act=7`, speed
  312 → 24 at −8/tick), enters the home lane t=60141 at the m0 cruise
  30. **t=60142: first hit, 1600 damage, life 4000 → 2400, `act 7 →
  2` in the SAME tick** — the v2==1 retarget (EF:10855-60), one tick
  into the lane. t=60143 → 800, t=60144 → −800 and `act 4` (prekill),
  t=60145-49 `act 5` (kill), t=60150 flags 0x40c. Latch `f2e = 250`
  constant the whole time — never decremented, because StageVar2 16
  skips the `--` (EF:10703-06), exactly as the port models it.
- **slot 772, (5,19) firefly.** Home lane from ≈t=60138. **t=60153:
  `act 159 → 154` at FULL life (600/600)** — the engage handoff,
  15 ticks of exposure. Killed t=60193 by a single 1600 hit
  (600 → −1000) while in `act 154` → `act 156` (prekill) →
  `act 157` t=60194-60200 → torn down t=60201. A ONE-SHOT KILL, and
  still a full 8-tick death animation.
- **slot 820, (5,19) firefly.** Born t=60068, home lane ≈t=60105,
  **`act 159 → 154` at t=60161 at full life** (56 ticks of exposure),
  one-shot t=60192 → 156 → 157 → slot recycled t=60202.

Every one of them left `8m+7` alive-or-instantly, by either the
v2==1 retarget or the engage handoff. **The port's exposure window is
the home-lane dwell before the engage handoff fires: 15 and 56 ticks
in these two specimens (0.6-2.2 s).** A one-shot kill inside that
window strands the husk permanently in the port and costs retail
≤ 8 ticks.

### Why only fireflies — HP, not a lane home

Not a per-model field home; the four summon models share one lane and
the session-8 f26 fix covers all of them. It is max_life against the
player's 1600-damage fireball:

| pick | model | max_life | per burst | roll weight |
|------|-------|----------|-----------|-------------|
| 3 | (5,0) worm | 4000 | 3 | 10/70 |
| 4 | (5,21) | 1000 | 3 | 1/70 (roll 69 only) |
| 5 | (5,25) | 7500 | 3 | 10/70 |
| 6 | (5,19) firefly | **600** | **8** | 9/70 |

(`mc2_spawn_m*` in roster.rs/multipart.rs; repeats + weights from
`mc2_pyramid_pick_summon`, doomsday.rs:786-813.) m0 and m25 can never
be one-shot, so their first hit is always a v2==1 retarget that pulls
them out of the lane — specimen 573 is that, one tick in. m21 is
one-shottable but is the rarest pick. Fireflies are ≈96% of the
one-shottable summon population, and they arrive eight at a time.

### NOT a divergence: visibility. There is no hide lane to honour

Ruled out explicitly, because the reconciliation hypothesis was that
retail hides its husks. It does not. Retail's sprite pass gates only
on `byte[0] & 0x21` (`DrawSprites_3E360` GameRenderOriginal.cpp:3157,
mirrored NG:2838/HD:3235; gather `sub_3FD60` GRO:1936) — no life gate
anywhere. The corpus agrees: specimens 573/772/820 hold `flags=0xc`
(bit 0 CLEAR = drawn) through prekill and kill, and only gain 0x400
(`byte[1] & 4`) at teardown. The port draws exactly what retail draws
and `live_poses_mc2`'s class-5 `flags & 1` gate (world.rs:1662) is
already right. The floating red bar over the husk is the port's opt-in
`render.debug.health_bars` overlay, not a retail element. **The
divergence is DURATION, not visibility.**

### Second divergence, same family: the spin-up lane eats the first hit

`sub_1E320` (EF:10566-10604), StageVar2 17, calls **only** the move
core and then tests life directly:

```c
sub_1B8C0(a1x);                       // EF:10572 — move core, no
if (a1x->life_0x8 < 0) {              // EF:10573   damage intake
    DisableEntityDrawing04_57F10(a1x); return; }
```

Damage in MC2 reaches an entity solely through the accumulate-mailbox
(`dword_0x5E_94` += / `word_0x62_98` = src, EF:4023-25 and ~30 twins;
the port's `Gen::mail_write`, mc1/combat.rs:85-95, matches) and is
applied only by a state handler's head. So retail applies NOTHING
during the ~37-tick spin-up flight: a hit taken in flight is carried
into the home lane and consumed there, where it becomes either the
v2==1 retarget (the escape) or the husk.

The port's `mc2_doom_summon_spinup_tick` opens with
`if self.mc2_state_head(i) == 2 { flags |= 0x400; return; }`
(mobs.rs:2505-2508), which DRAINS the mailbox and applies the damage
in the spin-up lane. Two consequences: (a) a non-fatal in-flight hit
is swallowed, so the creature enters the home lane pristine and loses
the retarget that would have taken it out of the husk-prone lane on
tick 1; (b) a fatal in-flight hit makes it vanish outright — no death
animation, no puff — where retail would have flown it into the home
lane and given it the full `+4`/`+5` death.

### ~~Proposed fix shape — DEFERRED~~ → **LANDED 2026-08-05** (player go-ahead: "Let's fix this")

All three parts landed in `crates/mgc-sim/src/mc2/mobs.rs`, one
function each side of the summon chain:

1. **The `return` in the dead arm is gone**
   (`mc2_doom_summon_home_tick`, mobs.rs:2617-2619). `f26 = 1` stays,
   `mc2_move_core` still is NOT called (retail's v2==2 arm does
   neither), and the arm now falls through to the engage handoff at
   mobs.rs:2680-2688 exactly as EF:10864-66 → EF:10735-39 does. This
   is the whole player-visible fix.
2. **The spin-up no longer drains the mailbox**
   (`mc2_doom_summon_spinup_tick`, mobs.rs:2517-2521): move core, then
   a BARE `act_life < 0` test, mirroring EF:10572-76. The queued hit
   now survives the launch flight and is consumed by the home lane —
   non-fatal → the tick-1 `v2==1` retarget, fatal → the husk arm and
   thence the normal death animation.
3. **The drift latch read-back** (mobs.rs:2599-2611): the dead head
   stamps `f26 = 1` before the `-= 4`, so an unlocked dead summon
   lands on −3 and expires next tick (EF:10727-30) instead of draining
   its live 250 by 4s.

The `mc2_doom_summon_home_tick` doc comment carries the corrected law
(the dead arm's fall-through and the three corpus specimens); the
stale "a KILL leaves the corpse standing until the pyramid's death"
line in `mc2_pyramid_summons_release_fight_and_expire`'s header
(world.rs) is corrected to note that test sits its summon ~90 tiles
out, far beyond m21's `v_28` 4608, which is why its husk legitimately
stands.

**Tests** (world.rs, all four green, each proven non-vacuous by
neutering its own arm and watching only that test fail):

- `mc2_doom_husk_converts_to_the_death_handoff_in_reach` — a firefly
  one-shot (1600 vs 600) in `8m+7` six tiles from the player leaves
  the lane inside the 8-tick throttle and finishes `154 → 156 → 157 →
  reaped`. Neutered (`return` restored): *"the husk converts out of
  the summon lane inside the 8-tick throttle (it used to stand in
  8m+7 forever)"*.
- `mc2_doom_husk_out_of_reach_still_stands_at_latch_one` — the same
  kill 60 tiles out still stands at `tick70 = 159`, `f26 = 1`, in the
  pool. The non-vacuity PARTNER: it pins that the conversion is gated
  on EF:10738's `v_28` test and that the standing corpse itself is
  retail law, so the engage check cannot be made unconditional.
- `mc2_doom_spinup_keeps_the_hit_queued_for_the_home_lane` — a fatal
  in-flight hit leaves `act_life` untouched, the mailbox still
  charged and the summon still in the pool, then lands once the home
  lane owns it; the non-fatal twin (a worm) becomes the tick-1
  retarget to `8m+2`. Neutered: `left: -1000, right: 600` on *"the
  spin-up applies NO damage"*.
- `mc2_doom_unlocked_dead_summon_drains_to_minus_three` — `f26 == -3`.
  Neutered: `left: 246, right: -3` (246 = the old 250 − 4).

**Goldens: NOTHING MOVED — no re-pin.** `MGC_REQUIRE_GOLDENS=1 cargo
test -p mgc-sim --no-fail-fast` = 347 lib + every integration binary
green, 0 failed. As predicted: no golden world authors a (5,10), so
the doom-summon lanes are simply not reachable from them.

**Windowed A/B** (`verify-deltas recordings/mc2l24.mgcr`, pre-fix vs
post-fix release binaries, `ulimit -v 2000000`, one at a time). The
baseline was rebuilt from the SAME tree with only these three arms
reverted, so the comparison isolates this law and nothing else — it
reproduced the first baseline's CSV byte-identically in both windows:

| window | UNEXPLAINED field | rows fixed | rows NEW | rng | conforming |
|--------|-------------------|-----------|----------|-----|------------|
| 51500 +600 | 2247 → **2039** (−208) | 224 | **0** | 1/600 → 1/600 | 51 → 51 |
| 60000 +300 | 2625 → **2380** (−245) | 281 | **0** | 0/300 → 0/300 | 0 → 0 |

Missing/extra rows unchanged in both (1/137 and 1/27). **Every moved
row is a pyramid summon and the change introduces no new diff
anywhere:** the only (class,model) touched are **(5,25)** in the
first window and **(5,0) / (5,19) / (5,25)** in the second — the
fireflies the player named are the largest block there (slots 974,
959, 960, 908). Per-family, every count fell and none rose: w1 x
2042→2005, y 2035→1998, z 925→909, heading 824→787, speed 418→381,
life 182→126, action 66→64; w2 y 1632→1587, x 1631→1585, z
1209→1173, speed 612→566, heading 540→494, life 270→216, action
100→96. **`sv2` mismatches go to ZERO in both windows (2→0, 4→0)** —
the spin-up's early mailbox drain had been shifting the
StageVar2 17→16 handover a tick off retail, and dropping it puts the
handover tick back on retail's. The `life` family is the direct hit:
retail holds an in-flight hit QUEUED, so the port's early application
had every damaged summon reading a wrong life for the rest of its
spin-up and a wrong track thereafter.

**Fixture suite: no drift, nothing promoted.** `cargo test -p
mgc-conform` green — all 6 manifests, 203 fixtures, `0 regressions,
0 fixed, 0 drifted, 0 not reached` (mc2l24: 17/17 as expected). The
l24 manifest does not sample these summon ticks, so no fixture status
moved; **`--promote` was NOT run.** `cargo fmt --all --check` clean.

**PLAYTEST OWED — what the player should see:** a summon killed
instantly right after it flies out now freezes for **at most a third
of a second** (the 8-tick engage throttle, and only while you are
within its engage reach — 20 tiles for a firefly) and then plays its
normal death animation and disappears, instead of standing there for
the rest of the fight. Summons shot during their launch flight now
fly on and die properly instead of blinking out of existence. A
corpse left standing far from you is still retail-correct — fly
within ~20 tiles and it will drop.

---

## MC1/HW RIVAL REBOUND — the arm existed, the BIT was never published (2026-08-06)

**Player report (retail-observed, mc1hw:0):** the sole rival wizard, once
taken down by meteor, puts Rebound (spell 14) up and keeps re-upping it
for the rest of the level. The port's MC1/HW rivals had never been seen
casting it.

**Retail law (corroborated line-for-line in BOTH trees).**

1. *Trigger* — `sub_132B0` (remc1 :18024-34 / remc1hw :16156-66), inside
   the per-tick brain `sub_13170` (remc1 :17842 / hw :15974). Every
   decision tick (`ent+63 % (64 - tempo/4) == 0`): `sub_16800` picks the
   NEAREST class-9 whose `+146` (chase target) is my id, 3-D distance²
   under `0x1900000` (= 5120²), walking bucket[3] — the class-9 list
   rebuilt at :52277-84; a hit sets strafe 80 (`sub_16870`) and runs
   `sub_16890`. (The same block also self-heals: `if (+12 < +8)
   sub_155F0(a1, 1)`.)
2. *Reactive pick* — `sub_16890` (remc1 :19808-54 / remc1hw :17940-90),
   byte-identical between the trees. Inside `0x100000` (= 1024²), a
   three-way switch on the THREAT's `+65`:
   models {0, 3, 16} → `if (sub_15A00(a1,0xE)) sub_155F0(a1,0xE); else if
   (sub_15A00(a1,4)) sub_155F0(a1,4);` — a LADDER, Rebound first and
   Shield as the fallback; models {4, 9} → Shield only; **every other
   model casts NOTHING** (1/2 fall out of the `< 4` branch, 5..8 out of
   the `>= 9` branch, `!= 16` returns outright).
3. *Readiness* — `sub_15A00` case 4/0xC/0xE (remc1 :19289-99 / hw
   :17422-34): the manifestation must exist, its burst `+48` must be
   **ZERO** (no re-cast while the buff runs), the AI cooldown
   `+724[2*s]` must be zero, and wizard mana `+140 >= +136`.
4. *Commit* — `sub_155F0` case 1/4/5/0xE (remc1 :19140-48 / hw
   :17271-81): `+48 = +50` (= `count` = 101 for Rebound) and
   `+724[2*s] = word_90034[s]` (= 1). No projectile, and **no castle
   check at cast time**.
5. *Token tick + THE BIT* — class-12 handler `0x2A` (= 3 × spell 14) of
   `str_2563D8` (:4996) is `sub_573F0_57920` (remc1 :65774 / remc1hw
   :61996): while `+48 > 0` run `sub_55DD0_56300` (remc1 :64910 — the
   stored-mana ladder: owner mana/life >= 0 and, since the ctor
   `sub_3C210` :48080 = `sub_3BF70(a1, 14, 42, 1000, 101, 1, 0, 8000,
   100)` sets `+132` = 8000, the owner's castle entity `+140` must hold
   >= 8000), then **`owner->+17 |= 0x80`** (our `flags & 0x8000`) and
   `sub_55E80` (regen pin); on failure `+48 = 1` (dies next decrement,
   buzz 29). `+48 <= 0` → `owner->+17 &= ~0x80`. Then `--+48`.
6. *The effect* — that bit is what the projectile-vs-victim step reads
   to deflect (`proj_move_and_hit`, remc1 :62848-90) — already ported,
   already reading `ent[j].flags & 0x8000` for pool victims.

**Corpus proof (`recordings/mc1hwl0.mgcr`, 50,150 ticks).** The retail
rival is slot 473 (class 3 model 1). Its Rebound bit is directly
observable in the obs `flags` lane, and the take contains **seven**
windows, each exactly `count` = 101 ticks, ON→OFF at
5591→5692, 7578→7679, 8506→8607, 12247→**12550**, 13442→13543,
17858→17959, 21372→21473. The 12247 window is 303 ticks = 3 × 101: the
rival re-upped twice, so "permanent" is a re-up loop, not a long token.
The port missed the ON edge of all seven and the OFF edge of six — it
never wrote the bit at all.

**Root cause: the arm was there; the PUBLISH was not.** `rival_defense`
already ported `sub_16800/70/90` and, driven against the corpus, fires
correctly (instrumented windowed replay at 20800..21500 caught the port
casting spell 14 at t=21371 with `owned=484, mana=24906, castle_stored=
13768` — one tick off retail's t≈21372, inside the take's own class-9
divergence). But `rival_refresh_buffs` mirrored ONLY the invisibility
cloak (0x20) onto the wizard entity; the Rebound token's `f26` never
reached `flags & 0x8000`, so `class12_tick` (which deliberately skips
rival-owned manifestations) plus the missing mirror meant NOTHING could
ever bounce off an AI wizard. `docs/DEVIATIONS.md` already flagged the
symptom for MC2 ("rival Rebound windows are not yet mirrored onto their
entities"); MC1/HW had the same hole.

**Fixed (all in `crates/mgc-sim/src/mc1/rivals.rs`).**
- `rival_refresh_buffs` publishes the token: `flags |= 0x8000` while the
  burst is live, `&= !0x8000` when it lapses (`sub_573F0_57920`). The
  wizard's death transition drops the bit too — retail's token keeps
  ticking through death states 2/3, which the port's rival lanes never
  reach, so without it a corpse would deflect forever.
- `rival_cast_ready` gains retail's ALREADY-ACTIVE gate (`+48 != 0` →
  not ready) for the self-buff group whose burst the port actually
  decrements for rivals: 2/4/12/14. Before it, `AI_RECAST[14] = 1` let
  the port re-cast Rebound every other tick for as long as the trigger
  held, paying 1000 mana each time — the instrumented window shows the
  port firing 3× in 12 ticks where retail fired once. Retail applies the
  same gate to the aimed group (3/7/8/17/20, :19265-68) and to Castle
  (:19305); **BANKED**, because the port never runs a per-tick countdown
  on a rival's OFFENSIVE manifestations, so gating them would freeze the
  picker after one shot.
- `rival_defense` is now verbatim `sub_16890`: the default arm casts
  nothing (was `_ => 4`, burning Shield + 2000 mana on threats retail
  ignores) and the fire-spell arm is the Rebound→Shield LADDER (which
  matters precisely because the new readiness gate makes Rebound unready
  for the 100 ticks after it arms).
- New `rival_token(ri, spell)` validates the `owned[]` binding
  (class 12 / matching model / `tick70 >= MANIFEST_BASE` / `f144` = the
  rival / not removed) before any burst lane touches it.

**⚠ NEW FINDING — the rival spell book is NOT re-anchored at import, so
no MC1 rival cast/buff lane is conformance-measurable.**
`import_state` (conformance.rs :324-350) rebinds a rival's entity, mana,
flight and brain lanes but NOT `owned[]`/`known[]`: the port's book still
points at the slots the FRESH world minted, which the import has since
overwritten with unrelated entities. Retail's token carries its owner in
`+42`, a field the port's `Ent` does not model, so the importer *cannot*
re-anchor today. Consequences: (a) the pre-existing code was decrementing
`f26` on whatever entity happened to occupy a stale slot — `rival_token`
stops that; (b) publishing the Rebound bit from a stale token actively
DESTROYED retail's own imported bit (measured: +100 `flags` rows on slot
473 per Rebound window, mc1hwl0 window 5500/+400 went 828 → 926
unexplained field rows), which is why the mirror is gated on the port
actually driving the token. Fixing this properly needs `Ent` to carry the
manifestation owner (+42) — a hashed-field change. **Banked.**

**Numbers.** Windowed A/B on `mc1hwl0` (`--input-delay 2`), before vs
after, identical in every window: 5500/+400 → 828 field / 25 missing / 73
extra both ways; 7500/+400 → 5533 / 117 / 55 both ways; 21300/+400 →
16310 / 148 / 201 both ways. **Conformance-neutral by construction** —
the rival token lanes are inert under a delta-verify import (see above),
so the corpus validates the RETAIL law and the free-run arm, not the
port's own casts. Full take after: 7,464 conforming / 49,765
fixture-grade, 800,222 unexplained field rows (the drop from the 2.03 M
baseline is the concurrent (10,13) smoke-puff landing, not this change).

**Goldens.** `level_005_golden_state_hashes` moves — attributed by
per-sub-change toggle to the ALREADY-ACTIVE gate alone (the mirror and
the reactive-arm corrections leave it pinned). Level 005's rival casts
Shield/Accelerate, and retail refuses those re-casts while the burst
runs. Behavior change toward retail by design. NOTE: the pin now on disk
was refreshed by the concurrent (10,13) smoke-puff work from a tree that
already contained this change, so the recorded value covers both; with
this change alone reverted the test fails.

**Tests.** `rival_rebound_arms_publishes_expires_and_re_ups` (arm →
publish → 101-tick countdown with no re-arm → expire → clear → re-up on
a fresh threat) and `unlisted_threat_models_provoke_no_reactive_cast`
(model 2 provokes nothing; model 9 controls that the Shield arm is live).
Both NON-VACUOUS — verified by temporarily disabling each sub-fix behind
an env toggle: the mirror off fails "did not publish the 0x8000
deflection bit", the gate off fails "the live Rebound token was re-armed
(100 -> 101)", the `_ => 4` fallback restored fails "a model-2 threat
provoked the Shield cast retail ignores".

**PLAYTEST OWED — what the player should see:** fireballs and meteors
now BOUNCE off a rival wizard (with the deflection twang, sound 28) for
about four seconds after he takes fire at close range, and the deflected
bolt turns around and homes on whoever fired it. A rival needs a castle
storing 8000+ for it — a poor or homeless rival still eats everything.
And the rival no longer stands there re-casting Rebound into itself:
once it is up, a second incoming bolt makes him raise Shield instead.

## LIGHTNING STORM + THE (10,13) SMOKE PUFF — the two biggest mc1hw families, both LANDED 2026-08-06

Player report (retail-observed): *"Lightning Storm is the safest way to
kill griffin flocks in mc1"*, but in the port *"it barely kills one and
the rest kill you"*, with the bolts visibly sitting **just above** the
creatures. Dig target also covered the take's `(9,9)` / `(10,13)`
families and the 33 rng over-draw pairs.

### 1. FAMILY SPLIT — what those rows actually are

- **`(10,13)` is NOT storm and NOT weather.** It is the class-10
  **RISING SMOKE PUFF**: ctor `sub_3AAA0` (`str_255D0C[13]`, remc1
  :46817 / remc1hw :44xxx — identical), tick `sub_257B0` (:28443,
  remc1hw :26987 — identical). Life `rand%23+17`, rise speed
  `rand%53+51`, sprite 67, filter pair `(10,13)`, `+18` bit1. The old
  ledger note "(10,13) 9.1k missing (meteor showers)" was a mis-ID —
  **retire it**. Exactly TWO creators in the whole binary, both
  unported:
  - the STANDING FIRE's exhaust (`sub_252D0` :28224, remc1hw :26774):
    inside the shrink window (`life < 12 && +26 > 0`) and with `+16`
    bit7 clear, one entity-LCG draw and on `d % 7 == 0` a puff at the
    fire with `+26 = 100` (which parks it past the tick's 16-tick
    drift window → fire smoke rises dead straight), `life = 15`
    (`max_life` keeps the ctor roll), the fire's owner, sprite `+2`;
  - the VOLCANO PLUME's ring spray (`sub_26140` :28834 = class-10
    state 19, remc1hw :27419) — see §3.
- **`(9,9)` is the lightning family and nothing else** — the m9 BEAM
  (state 9) plus its born-dead state-14 SEGMENTS. In mc1hwl0 it is
  driven by the storm windows (the `(9,12)` carrier fires at
  t≈21976 / 26669 / 34573 / 41015 / 41338 / 42212) and by kraken/creature
  beams elsewhere. The balanced missing/extra is segment churn, as the
  standing law says — count RECORDS.

### 2. THE STORM LAW, line-cited

`sub_26D20` (:29279 / remc1hw :27823 — **byte-identical**), class-10
state 40, reached from the `(9,12)` carrier's `sub_53DC0` (:63628),
which stamps owner / `+30` / `+32` / `+44` / `+146` / `+68=9` / `+69=9`
onto the `(10,38)` cloud (:63767-83):

1. `v2 = ground_z(pos)`. `z < v2+1024` → `z += 64`, **skip the tick**.
   `z > v2+1024` → `z = v2+1024`, **skip the tick too** (`HIBYTE(v2)+=4`
   is `+1024`). The port fired on the clamp-DOWN tick — fixed.
2. Pre-decrement life (33 firing ticks from the ctor's 32).
3. ONE entity-LCG draw → `+30 = d & 0x7FF`; `+32 = 56` **fixed pitch**.
4. Twice: `+30 ^= 0x400` (180° flip) then create `(+68, +69)` =
   `(9, 9)` **AT THE CLOUD'S OWN POSITION**. Retail builds a
   `z + <+78>` point in the shared temp `word_AE454_AE444` and then
   passes `(axis_3d*)(a1+72)` — **the `+78` lift is a DEAD STORE**. The
   port was adding it, laying every bolt a sprite half-height high.
   Child gets owner, `life /= 3`, `+30`/`+32` (NOT `+34`/`+36` — retail
   leaves those at NewEvent 0 for the beam's own acquire to fill),
   `+68=10`, `+69=23`, `+44` = the storm damage.
5. Thunder 23, positioned at the LAST bolt (the port plays it at the
   cloud — presentation, left alone).

Dropping the launcher's `+34`/`+36` pre-seed exposed a second missing
store: `sub_534C0`'s acquire gate (:63232-40) copies `+30`/`+32` into
`+34`/`+36` on a MISSED acquire, and the port only ever wrote them on a
HIT. The storm's pre-seed had been standing in for that copy. The miss
branch now lives in `proj_m9_tick` where the original puts it — worth
−93 / −155 unexplained field rows in the two storm windows on its own,
and no golden moves.

**Reach arithmetic: the storm CANNOT hit anything on its own.** The
`(9,9)` ctor (`sub_39EC0` :46135) is speed 384 / life 9, so a storm bolt
gets `9/3 = 3` → **4 steps of 384 = 1536 units** of travel, fired at
pitch 56 (≈9.8°) from **1024 above the terrain**. It descends 262 units
over its whole flight. Every single storm kill therefore comes from the
BEAM'S ACQUIRE snapping it down onto a victim.

**⭐ THE BUG IS ONE CONSTANT.** `sub_54520` (:63943 / remc1hw :60053)
switches on `+65`, and **case 9 is its own lane** (:64125 / remc1hw
:60256, identical):

- the wizard/castle list: cones `(0x71, 0x71)`, range gate
  `+128 * +8` (speed × max_life = the beam's own reach), 2-D distance;
- the **CREATURE buckets: `sub_54A90(a1, ii, 0x71u, 0x200u)`** — yaw
  ±0x71 as usual, **pitch ±0x200 = ±90°**, no range gate beyond the
  scorer's 3-D 5120.

No other subtype does this (cases 0/3/4 and 1 are `0x71/0x71`; HW's
meteor case 0x10 is `0x100/0x71` on BOTH lists). The port ran one shared
`0x71/0x71` cone for every subtype, so a flock 1024 below the cloud sat
outside the pitch wedge: nothing locked, the bolt held pitch 56, and it
sailed out level — **"the bolts hover just above the monsters"**,
exactly as reported. Restoring `0x200` on the creature lane makes the
storm the flock-killer it is remembered as.

### 3. THE PLUME (class-10 state 19) WAS UNTRACED — now transcribed

`sub_26140` (:28834): pre-decrement life; `+26 = 0`; walk the radius-0
ring (`sub_11410(0, +26)` → the 2×2 recentre block, 3 cells after the
iterator's dropped-last quirk); per cell one draw for the spreader's
~50% skip test (`d % 0x9D >= 79`) and, on a pass, the ±64 jitter pair;
then on ODD post-decrement life ticks each passing cell emits **four**
`(10,13)` puffs at yaws `{v, v+0x200, v+0x400, v+0x600}` with
`v = ((life/2) & 1) << 8` — the column corkscrews. Re-seat on ground. NO
animation step (sprite 228 is static). The port's stub drew **zero**
entity-LCG values a tick where retail draws **3 / 5 / 7 / 9** — that is
the entire `(10,19)` `rand` column in mc1hwl0 (3,822 rows on slot 767).

**OPEN (banked, deliberately NOT ported):** `sub_26140` closes with an
UNCONDITIONAL `sub_120B0(a1, 0, +44)` — 200 ch0 per tick over the
ctor's 512 extents for the plume's whole 240-tick life, i.e. the volcano
plume is a persistent damage field. No corpus take erupts a volcano, so
this is left for a measured eruption window rather than guessed in.

### 4. THE 33 rng OVER-DRAW PAIRS — DIAGNOSED, fix is OUTSIDE this lane

Located by windowed bisection: they are **contiguous runs of ticks
around the human's death**, e.g. t=21468..21481 (14 pairs) and
t≈6xxx / 18xxx. On MC1 the port has exactly one site that can step the
GLOBAL LCG more than once a tick: `player_land` (world.rs :3122-24),
three draws per owned spell for the jar scatter.

**Root cause is the conformance IMPORT, not a sim arm.**
`import_state` (`crates/mgc-sim/src/engine/world/conformance.rs` :392)
reads the human's life state from **`carpet.f66`**:
```
state: match carpet.f66 { 2 => Falling, 3 => Dead, _ => Alive }
```
Retail keeps the wizard's life state in **`+70`** (`sub_46B00`'s death
tail writes `*(_BYTE*)(a1+70) = 3` at :55550). Measured on mc1hwl0 slot
472: `f66 = 255` at t=100, 21400, 21470 and 21600 — *always* — while
`f70` is 0 alive and 3 during the dead-waiting window. So a dead retail
player imports as **Alive with a stale negative life**; the next damage
mail re-runs the whole death (`life < 0` → `Falling` → the same tick's
landing test → `player_land`) on EVERY imported tick of the window.

**Measured probe** (applied, measured, reverted — the file is the
harness-side import and outside this dig's territory): switching that
match to `carpet.f70` on window 21440/+80 takes rng **14 → 1**
mismatched pairs, unexplained field rows **5,385 → 4,775** and extra
**36 → 11**, with all 203 fixtures still `as expected`. **Handed to the
lead to land deliberately.**

### 5. NUMBERS

Full take `mc1hwl0.mgcr --input-delay 2`, before → after:

| | before | after |
|---|---|---|
| conforming | 7,355 | **7,464** |
| UNEXPLAINED field rows | 2,030,967 | **799,629** (−60.6 %) |
| UNEXPLAINED missing | 32,097 | **17,142** (−46.6 %) |
| UNEXPLAINED extra | 14,210 | 18,117 |
| `(10,13)` missing / extra | 10,135 / 0 | **422 / 1,782** |
| `(10,6)` missing / extra | 3,582 / 875 | **501 / 2,882** |
| `(9,9)` missing / extra | 12,017 / 11,344 | 11,785 / 11,106 |
| phase-clock disagreements | 1,844 | **898** ((10,13,13) 1,277 → 370) |
| `mc1hwl0-terrain-z` rule rows | 609,374 | **159,071** |
| rng mismatched pairs | 33 | 33 (see §4) |

`(10,13)` alone carried **1,752,673** of the take's diff rows — 86 % of
the whole field-row mass — because the state had no tick arm at all:
every imported puff fell through world.rs's class-10 catch-all (the
terrain-feature dispatch) and self-killed one tick after import
(`flags` `0x20004` → `0x20404`, `life` +1, `z` −64 on every row).

Windowed A/B, before → after (unexplained field / missing / extra):
- storm window **41000/+120**: 40,450 / 183 / 82 → **19,018 / 6 / 161**
- storm window **26690/+120**: 22,189 / 156 / 311 → **20,170 / 39 / 52**
- storm window **41340/+120**: 5,428 / 4 / 5 → **5,079 / 4 / 5**

`mc1l0` (regression guard): 8,834 / 126 / 98 → **8,520 / 119 / 99**,
rng 0/7097, no new families. All six suites `as expected`
(29 + 68 + 41 + 24 + 17 + 24 = 203, 0 regressions, 0 drifted).

### 6. GOLDENS + TESTS

`level_005_golden_state_hashes` B–E re-pinned (post-init and A hold —
nothing burns before B): every burning tree and crater now emits smoke
entities, so pool population and free-list order move. **Behavior change
toward retail by design.** The layout-independent OBSERVABLE companion
golden **held**. NOTE: the pin on disk was taken from a tree that also
contained the concurrent MC1/HW rival-Rebound work, so the recorded
value covers both changes.

New NON-VACUOUS tests (`engine::world::tests`):
- `storm_bolt_locks_the_flock_below_and_strikes_at_creature_z` — asserts
  in-fixture that the target's pitch delta is `> 0x71` (so the shared
  cone provably cannot see it) and `<= 0x200`, then that the bolt locks,
  that the acquire SNAPS `+30/+32` onto the pick, that the beam ends at
  flock altitude instead of ~760 above the deck, and that the `(10,23)`
  endpoint flash carries and delivers the storm's 2000. Verified by
  temporarily restoring the `0x71` creature cone: fails with
  "the storm bolt must lock the creature under it — left 0, right 2".
- `burning_fire_emits_rising_smoke_that_survives_its_own_tick` — the
  fire's 1-in-7 exhaust, `+26 = 100` / `life = 15` / owner inheritance,
  the `[64,128]` speed decay, the rise, the absence of lateral drift,
  pre-decrement life, and expiry on its own clock (it used to be flagged
  dead on the spot).

### 7. PLAYTEST OWED — what the player should see

Lightning Storm should once again be **the safest way to clear a griffin
flock**: the cloud parks 1024 up and its bolts now *snap down* onto the
birds under it instead of drifting overhead, two strikes a tick at
2000 apiece. The same constant governs every m9 beam, so kraken beams
also stop skimming past low targets. Separately, burning trees, fire
walls and craters now trail rising smoke, and volcano plumes churn out
a corkscrewing smoke column instead of standing there inert.

## THE MC2 HUMAN DEATH LAW — the corpse, the token scatter and the SPACE reset, LANDED 2026-08-06 (mc2l3 both reset pairs now conform outright)

The mc2l3 dig. The take carries **two human deaths** (t≈15280 and
t≈20560) and the port failed both: it kept the corpse alive (regening
life AND applying the frozen `manaRegen`, and clamping the corpse's
mana to `mana_max`), never respawned, and never re-minted the
spellbook — the 26 class-15 tokens retail creates at the reset were
the take's whole `(15,*)` missing census. Everything below is
`EventsFunctions.cpp` (`EF:`) unless marked.

### 1. THE STATE MACHINE IS THE RIVALS' — THE FORK IS INSIDE ACTION 3

The human wizard is a class-3 pool entity running the SAME action
machine as the AI wizards. There is no separate human death path; the
AI/human fork sits *inside* the action-3 body.

| action | body | what it does |
|---|---|---|
| 0 | `AddPlayer03_00_5E010` (EF:60040-44) | `life < 0` → action = 2, `word_0x2C_44` = 0, sound 16, DEAD scene |
| 2 | `sub_5E310` (EF:60074-99) | mover, then z-only gravity; floor = ground + row clearance; one `(10,1)` puff/tick; EXACT floor contact = the payout |
| — | the payout (EF:60101-77) | `sub_49F90`, kill credit, mailbox wipe, "has died.", the 26-token SCATTER, the `(10,40)` grave + sphere re-point, action = 3, `dword_0x10_16` = 1200, hide |
| 3 | `sub_5E7C0` (EF:60254) | **AI**: count 1200 down → castle respawn / banish. **HUMAN** (EF:60303-05): `sub_5C800(7)` + `sub_5E6C0` — face the killer at ≤22/tick, pin z to the ground, wait for SPACE forever |
| reset | `sub_5C950` (EF:43630) | PlayerAction 0xF, accepted by PI:1102 only while `life < 0 && action == 3` |

Two things the corpse does NOT do, both because the whole regen block
in the action-0 body is inside `if (life >= 0)` (EF:59996-60033):
it never heals and it never touches `mana`/`manaRegen`. mc2l3 holds
life at **−3060** and mana at **16,957** for the entire 15-tick corpse
window, and `mana` stays 95,951 at death 2 even though `mana_max` is
2,559 — the corpse never runs the clamp either.

Measured constants: floor − ground = **256** = the carpet tuning row's
`word_160_0xc_12` (`Mc2Row::CAVE`/`OPEN` clearance, both 256) — mc2l3
t=15300 lands at z 2125 over ground 1869. The corpse yaw walks
585 → 563 → 541 → 519 → 497 → 482 in exact 22s, then wobbles ±3 as
the killer moves: `sub_58350(yaw, bearing, 5, 0x16)`, cap 22.

### 2. THE SCATTER KEEPS THE BOOK — `SpellEnabled[i] = 1`, NOT 0

`sub_5E310` EF:60137-62 walks all 26 book slots. A live manifestation
is detached in place — `byte[0] &= ~1`, `actionIndex++` (3M → 3M+1,
the loose-pickup state), position ±256 around the corpse, life
`rand%90 + 200` — and **the book entry becomes a boolean 1**. That
marker is the wizard's whole memory of what he knew; the reset
re-mints exactly the non-zero entries. mc2l3 t=15300 shows all 26
tokens (slots 168-204) flipping to state 3M+1 with lives in [200,289]
scattered over the death tile, and both hands (`SpellIndexLeft/Right`
= 0/1) unchanged across both deaths — the scatter never touches them.

The three draws per token come off the DYING WIZARD's private LCG
(`a1x->rand_0x14_20`), never the world stream and never the token's
own `rand` (mc2l3 keeps all 26 token seeds at their allocation values
through the scatter).

### 3. THE RESET — WHAT `sub_5C950` WRITES, AND WHY THE MANA RESIDUE IS 750

For a human (`IsAiPlayer != 1`, so `actionIndex = 0` and the
fresh-join block EF:43744-822 is skipped), in order:

1. `sub_49F90` — **both** stacks rebuilt (descending 999→1 scan).
2. position ← the own castle's FULL position (x, y **and z**);
   castle-less single player = the level's authored start point at
   ground + 0x100, plus `byte[2] |= 0xC` (lost + level-over, PI's
   own flags) — retail respawns you anyway.
3. grace (`word_0x159_345`) = 100, wanted = 0, `dword_0x16D_365` =
   2000, commanded speed/yaw/fov/boost/strafe = 0, life-scale = 256.
4. **`maxMana` = 1000, `maxLife` = 10000.**
5. `sub_5CF40` (EF:59374) — the token RE-MINT (see §4).
6. **`life = maxLife`; `mana = maxMana`; and the mana-census BASE
   `byte_0x150_336` = maxMana.**
7. every other wizard's hate toward this colour → −24609; the AI
   target lanes cleared; the recycle stack emptied
   (`dword_0x11e6 = −1`, EF:43857).

`maxMana = 1000` is invisible one tick later: `sub_60F00` (EF:61976)
recomputes every wizard's `maxMana` from `byte_0x150_336` plus the
`mana` of every entity they own (castles, balloons, creatures,
`(10,39)` spheres) **before the entity pass**, which is why mc2l3's
`mana_max` reads 82,157 straight across the reset while `mana` lands
on 750. (It is also why `mana_max` collapsed 82,157 → 2,559 between
the deaths: the castle's stored mana went with the castle.)

**THE 750/2000 RESIDUE IS THE FRAME ORDER, NOT A PENALTY.** The reset
runs in `PlayerEvents` — ahead of the census, ahead of the entity pass
and (measured) ahead of even the frame's one unconditional LCG draw —
so the same frame's wizard body applies the `manaRegen` the corpse had
frozen:

```
mana(reset) = 1000 + manaRegen_held
  mc2l3 death 1: 1000 + (−250) =  750   (a cast debit was pending)
  mc2l3 death 2: 1000 + 1000   = 2000   (at-castle regen floor)
```

mc2l24's 14 human deaths confirm it on a second take: resets land on
1000 (delta 0, regen suppressed by a live cast), 1100 (the afield
floor 100), **1295** = 1000 + 590060/2000, **1310** = 1000 +
621060/2000 and 0 (a −1000 debit pending). The `maxMana/2000` afield
rate reproduces the odd residues exactly.

The RNG dating: retail's 26 fresh tokens are seeded `slot + rand` off
**29,590**, which is the recorded global LCG at t=15314 *unadvanced* —
so the reset allocates before the frame's own draw. That single fact
places the whole reset in the input phase.

### 4. `sub_5CF40` — the re-mint, and why the slots are 99, 100, 102…

For every non-zero book entry, spawn a fresh class-15 token at the
wizard's (already moved) position, `parentId` = the wizard,
`byte[0] |= 1`, then re-apply `SetSpell_6D5E0` at the stored tier. An
allocation failure zeroes that book entry — retail's own silent loss.

Because `sub_49F90` rebuilt the free stack first, the mint takes the
LOWEST free slots in ascending spell order: mc2l3 t=15315 puts spell 0
at slot **99**, spell 1 at 100, spell 2 at 102 (101 is live), … spell
25 at 133, all at the castle's exact x/y/z, with the same
cost/upkeep payloads as the jars they replace. The same rebuild is why
the graves land on slots **3** and **1**.

The OLD scattered jars stay out in the world, still collectible — a
death therefore doubles the class-15 census for a few hundred ticks.

### 5. WHAT LANDED (port)

- `retail_import_mc2`: `action45` 2/3 → `LifeState::Falling`/`Dead`
  (it pinned `Alive` unconditionally, which is what let the corpse
  regen and clamp).
- `World::tick`: the MC2 reset is processed FIRST, ahead of the frame
  draw, the census and the wizard body — the ordering IS the residue
  law. `MGC_NO_MC2_DEATH=1` restores the pre-dig behaviour for A/B.
- the mana step + delta recompute are gated off for an MC2 corpse.
- `mc2_player_fall` / `mc2_player_land` / `mc2_player_dead_wait` /
  `mc2_player_respawn` / `mc2_remint_book` — the human column;
  `Gen::mc2_rebuild_free` is `sub_49F90`'s free half.
- `mc2_scatter_spells` was written but never wired (and zeroed the
  book, which would have made every death a permanent wipe); it now
  leaves the boolean marker, keeps the hands, drops the jars at the
  corpse's z and does NOT mutate the token's `rand`.
- the book-driven manifestation loops now test the OWNED action state
  (`tick70 == 3M`) — retail's own dispatch condition, which also keeps
  the boolean marker from aliasing a real slot-1 manifestation.
- harness: the recorded SPACE key (scancode 57) is decoded into
  `PlayerCommand::respawn`. Dating it needs a witness — the key
  registers carry no press latch and mc2l3 shows both sides of
  retail's poll (SPACE first at record 15314 with the reset in frame
  15315; at record 20612 with the reset in frame 20612) — so the rule
  is `space(end) && (space(start) || recentred(end))`, where
  "recentred" is the cursor jumping to a point that equals the
  press-position snapshot: retail's 0xF handler runs
  `SetCenterScreenForFlyAssistant_6EDB0` (EF:37653). Over the whole
  take 348 records carry the recentre shape and exactly two of them
  also have SPACE down — the two reset frames.

### 6. THE `(10,2)` FAMILY — the Speed spell's slipstream, never ported

175 missing rows, first at t=15499, right after respawn 1: an
owner-stamped `(10,2)` appearing every 4th tick along the boosted
flight path, life counting 31 → −2. It is `GetScroll_69DB0`'s
contrail (EF:56251-59): on every live tick of the Speed window with
`byte_0x3E_62 & 3 == 0` — the TOKEN's phase byte, not the caster's —
spawn `NewAdd0A02_4E430` at the caster and **quadruple** the ctor's
life (8 → 32). The ctor is four writes: maxLife/life 8, action 2,
`dword_0x10_16` = 0, flags masked to `byte[0]|1`, `byte[0]&~8`,
`byte[2]|2` (= the recorded 0x20001) — no sprite, and deliberately NO
map link (it assigns `position_0x4C_76` instead of calling
`AddEventToMap_57D70`), which is why the trail hangs where the carpet
was. The MC1 twin of the same puff is documented in
docs/traces/mc1-class12-spell-tokens.md.

### 7. NUMBERS (windowed A/B, `MGC_NO_MC2_DEATH=1` = before)

unexplained field / missing / extra:

| window | before | after |
|---|---|---|
| mc2l3 **15255 +70** (death 1: fall, corpse, reset) | 375 / 28 / 7 | **215 / 2 / 7** |
| mc2l3 **20595 +30** (death 2 reset) | 98 / 26 / 0 | **30 / 0 / 0** |
| mc2l24 **2580 +40** (death 1) | 226 / 18 / 2 | 233 / **2** / 2 |
| mc2l24 **11205 +45** (death 5) | 610 / 22 / 4 | 610 / **0** / 4 |

- mc2l3 death-1 reset pair **15314→15315: 0 unexplained rows and the
  rng matched** — life 10000, mana 750, all 26 tokens at retail's own
  slots with retail's own seeds. (The rng needed one more thing: the
  importer arms `mc2_carpet_stall` off the corpse's action 3, and the
  reset must clear it — the resurrected carpet runs the mover, and
  with it the cave tail's draw, that same frame.)
- mc2l3 death-2 reset pair **20611→20612**: life/mana/tokens all
  conform; the window's residue is the pre-existing grave-census
  family (`slot 1 mana_max`, 12/12 pairs, present before the reset).
Full take (`verify-deltas recordings/mc2l3.mgcr`, 27,489 pairs):

| | before | after |
|---|---|---|
| conforming | 21,534 | **21,597** |
| conforming + explained | 21,865 | **21,947** |
| UNEXPLAINED field | 28,450 | **27,650** |
| UNEXPLAINED missing | 440 | **209** |
| UNEXPLAINED extra | 144 | 145 |
| entity sets missing / extra | 559 / 304 | **328** / 305 |
| rng mismatched pairs | 2 | **0** |
| `(15,*)` reset rows | 52 / 0 | **0 / 0** |
| `(10,2)` | 175 / 0 | **0 / 1** |
| `player.life` / `player.mana` pairs | 454 / 1,447 | **357** / **1,351** |

The two `(>16, 1)` rng pairs in the census were the two resets; the
take now has none.

### 8. RESIDUE + LEADS (all measured, none blocking)

1. **The landing pair keeps ~104 rows** (26 tokens × life/x/y/z): the
   scatter's three draws belong to the dying wizard's private LCG,
   which this port has no home for (the human owns no pool record). A
   copy of the token's own seed stands in. The recorded carpet DOES
   carry `rand` — importing it would close this exactly.
2. **Loose class-15 jars do not snap z.** Retail's scattered tokens
   read 1997/2063/3194/3887 one tick after the scatter (cave terrain);
   the port leaves them at the corpse's z. That is the class-15
   pickup-state arm, not the death law — a clean follow-up, and the
   take's `z` family is the biggest one left.
3. **The death puff ticks one frame early.** The port's player column
   runs before the pool walk, so a `(10,1)` allocated below the carpet
   slot seeds its `(10,0)` fires in the same frame; retail's puff is
   spawned at slot 167 inside the walk and waits. 6 extra rows on the
   landing pair (`mc2_manifestation_pass` is the existing cure shape).
4. **The mid-burst regen suppression is worth a dig.** `sub_68DE0`
   zeroes the caster's `manaRegen` on every non-first burst tick, and
   the manifestation slot decides whether the wizard body already
   applied it: mc2l3's spell-3 token sits at slot 103 < carpet 167, so
   retail's mana stays FLAT while the recorded `d88` reads the
   post-recompute 100. The port applies the imported delta and
   over-regens by exactly one quantum. This is the take's largest
   single family (`player.mana` 1,447 pairs) and the MC1 import lane
   already has the twin clamp (`conformance.rs`, the `f48 != f50`
   probe) — the MC2 version wants `any live human class-15 token with
   `f26 != 0 && f26 != f28` below the carpet slot`.
5. `mc2_rival_death_impact` spawns the rival grave at `ground_z`, not
   at the corpse's floor — the same one-line law this dig fixed on the
   human side, left alone because no rival death was measured here.
6. The port's respawn queues `pending_respawn` but the human pose
   belongs to the app, so the same frame's scans still run from the
   corpse (retail moves the entity mid-frame). Harmless — you collect
   your own jars a tick early — but it is a real one-tick divergence
   in native play.

### 9. TESTS + GOLDENS

New, non-vacuous, `engine::world::tests`:
- `mc2_death_scatters_the_book_and_the_reset_re_mints_it` — the
  landing scatters every manifestation into a loose jar on the
  `rand%90+200` clock, leaves the **boolean 1** marker (not 0), raises
  the grave, then a corpse tick that changes NOTHING (no heal, no
  regen, no delta recompute), then SPACE: alive, life 10000, mana
  **750** = 1000 + the held −250, a castle teleport, fresh tokens in
  the owned state at NEW slots, the old jars still out in the world,
  hands unchanged.
- `mc2_speed_window_trails_a_puff_every_fourth_tick` — one puff per
  four ticks, `maxLife` 8 with `life` 4×8, action 2, the caster stamp
  and the exact flag word; off-cadence shifts WHICH tick drops, never
  how many.

No golden was re-pinned by this work: the MC2-native-visible changes
(the Speed puff, the owned-state guard) were A/B'd against
`mc2_cave_behaviors_and_goldens` and left its hashes bit-identical.
That test IS red in the shared tree — its divergence is in `hashes[0]`,
the freshly built world before any tick, which no death path can
reach; it belongs to the concurrent `(10,13)` smoke-puff work
(§"LIGHTNING STORM + THE (10,13) SMOKE PUFF"), whose own re-pin note
says pool population and free-list order move from the build settle on.

## MC1/HW SPEED-TOKEN CONTRAIL + TOKEN-OWNER IMPORT + PLAYER
## LIFE-STATE LANE (session lead, dig/re-triage round, 2026-08-06)

**(10,2) = the direction tokens' contrail, BOTH directions.** The
ledger's old attribution (Accelerate `sub_56380` alone) was
incomplete: the reverse token's arm `sub_57F00_58410` (remc1hw
:62390-451; v_12 written −3×/−2× base, otherwise the same body)
emits the same `(10,2)` puff every 4th token `+63` tick, and the
hw:0 rival brakes/boosts near-permanently — every one of the take's
1,847 missing rows was RIVAL contrail (`id24` 473; the human never
cast a direction spell all take). Landed:

1. **Strict class-12 phase-0 arm** (world.rs `class12_tick`): for
   spells 2|21 with the imported burst counter live (f26 ← retail
   +48) and `f63 & 3 == 0`, spawn the `(10,2)` at the OWNER's pose,
   `id24` = owner, `act_life ×4` (ctor 8 → 32 — the t=3 corpus puff
   reads 31). Admission per `sub_55DD0_56300` (:61132-55): both
   direction ctors author no castle-store req; only first-burst-tick
   mana ≥ 1000 gates. Cadence corroborated tick-exact (rival token
   f63 244 & 3 == 0 at the t=2 spawn).
2. **Token-owner import** (conformance.rs `import_ent`): class-12
   `f144 ← tr(f42)` — retail keeps the token's owner wizard carpet
   slot in +42 and +144 is authored 0 on every token (corpus-proven),
   so the lane is free. This is the re-anchor lane the Rebound dig
   banked as lead 1; MC1 obs never projects f144, and
   `rival_token()` requires native encoding, so both stay unaffected.
3. **Native contrail** (`manifestation_tick` arm 2): same law off the
   native f26 burst; test
   `accelerate_burst_emits_the_contrail_on_the_4_tick_cadence`
   (non-vacuous: off-phase and lapsed-burst ticks spawn nothing).
4. **Player life-state import fix** (the storm dig's diagnosed lead,
   landed verbatim): `state:` read `carpet.f66` = sCLASS (255
   always) — every dead player imported Alive-with-negative-life and
   re-ran the death cascade per pair. Real lane = the tick-handler
   byte `+70` (`*(_BYTE*)(a1+70) = 3`, :55550).

**Full-take mc1hwl0 (`--input-delay 2`), post-storm baseline →
after:** conforming 7,464 → **7,810**; rng mismatched pairs 33 →
**4**; missing 17,142 → **15,035**; unexplained field 799,629 →
796,115. `(10,2)` 1,847/0 → **150/220** — the residue is (a)
burst-START pairs (the rival casts mid-tick: token 0 → 251 inside
the pair, e.g. t=70 — cast-timing capture) and (b) free-stack
slot-allocation capture from t≈278 (both sides spawn the same puff
at the same tick, different slots; the retail slot is port-occupied
so the balance degrades to field rows + one-sided extras — the
extras run 278-326 at exactly the retail cadence). Suites: 1 FIXED
(hw t=4 → conforming), 3 hw drifts + 1 mc2l4 drift all
signature-shrinking, promoted; 0 regressions.

**Leads:** ① the extras/missing (10,2) residue wants a slot-capture
roster rule scoped to burst windows; ② the strict arm skips retail's
`Type_160 +14` force-end clamp (`+48 = 1` when the owner's v_14 lane
is pinned) — unmodeled, invisible on this corpus; ③ the regen seed
clamp (conformance.rs :385) counts ANY mid-burst class-12 with
`f144 == 0` as the human's — with rival tokens now stamped via f42
that heuristic could be tightened to `f144 == PLAYER_TARGET` (it
currently keys retail rows, where +144 = 0 for rivals too — worth a
re-measure against the player.mana family, 9,983 rows).

## MC2 CAVE STOCK-BAKE DIG — THE GENERATOR IS BYTE-PERFECT; THE
## LOAD-TIME SPAWN DATUM WAS THE BUG (dig agent, 2026-08-06)

**Premise falsified.** The dig opened on "the MC2 cave terrain
GENERATOR chain diverges from retail's t=0 bake". It does not.
`mgc-import/src/mc2_terrain.rs` reproduces retail's
`GenerateLevelMap_43830` **byte for byte on every plane**: over
mc2l3's 65,536 cells, the count of cells that neither retail's nor
our load pass touched and that still disagree is **0 on all five
planes** (type/height/shading/angle/ceiling). Every divergence the
record-0 validator saw lived in MC2's LOAD-TIME cave-sculptor pass
(`GenerateEvents_49290` + the `ApplyEvents_498A0` settle), which
runs after the bake and before tick 0.

The three-way split that proved it: `mgc-conform terrain-diff --out`
gives retail's measured record-0 planes and our POST-LOAD planes;
the new `mgc-import --example tmp_mc2gen` dumps the generator's
output BEFORE the load pass. Generator-vs-`.mgcl` matched exactly;
`.mgcl`-vs-post-load differed by 12,465 height cells — i.e. the
carve, not the bake.

### The three laws (all decompile-line-cited)

1. **LOAD-TIME SPAWNS SIT ON THE TILE CORNER, NOT ITS CENTRE.**
   `PrepareEvents_49540` builds every one of its three spawn
   positions as the bare `entity->axis2d_4.x << 8`
   (Events.cpp:307 class 2/0x0E, :339 the 0x2D building arm, :353 the
   generic class-10 arm). Only the RUNTIME disposition spawn
   `sub_4A310` adds the half tile (`(axis2d_4.x << 8) + 128`,
   EF:33014). `World::spawn_from_thing` added +128 unconditionally,
   so the MC2 at-load pass placed every cave sculptor half a tile
   SE of retail's. That half tile is load-bearing twice over: each
   sculptor derives its box origin from `(position + 128) >> 8`
   (EF:25599-25602) — which rounds a whole tile the other way — and
   its radial profile from `EuclideanDistXYZ` to each tile CORNER
   `(i << 8, j << 8)` (EF:25666-68), so retail's cell-centred cone
   (`d == 0` at the centre cell) became a 2x2-symmetric one with no
   `d == 0` sample at all. Fix: `spawn_from_thing_at(ti, corner)`,
   with `mc2_generate_events` passing `corner = true` and
   `fire_disposition` keeping the centre. MC1 is untouched (its
   load pass runs `load_time_pass`, not this seam) and stayed
   byte-perfect on mc1l0/mc1hwl0.

2. **THE PIT/HILL −128 RECENTRE BELONGS TO THE DISPOSITION PATH
   ONLY.** `sub_4A310` subtracts 128 from x and y for models
   0x54/0x55 (EF:33129-31) — which exactly CANCELS the +128 that
   same function applied at EF:33014, landing the sculptor on the
   corner. `PrepareEvents_49540`'s 0x54/0x55 case (Events.cpp:384-88)
   consumes `word_10` and `par3_18` and does NO position fixup,
   because it never added the half tile. We ran the −128 on both
   paths, so after fix 1 the load-time pits/hills sat half a tile NW.
   Fix: gate the recentre on `r.dis_id != 0xFFFF`.

3. **THE RELIEF-SHADE INVERSION IS LIVE DURING THE LOAD SETTLE.**
   `sub_462A0` / `AddBuildingToTerrain_46570` write `32 - s + 32`
   whenever `MapType != Day` (Terrain.cpp:2030-33, EF:31185-88) — a
   LEVEL property retail holds long before `GenerateEvents`. Our
   `mc2_night_shade` is a post-construction setter, so every repaint
   the load carve fired baked DAY shading into a cave level (the
   plane came out `64 - correct` on ~15k cells). Fix: derive the
   cave half of the flag inside `World::new_for_game` from the
   ceiling plane (`Gen::is_cave`) before `mc2_generate_events`.
   REMAINING GAP: a NIGHT non-cave level has no ceiling plane, so
   its load-time painters (roads/rivers/beams) still repaint under
   the Day law until the flag reaches construction properly.

### Numbers (mc2l3, record-0 stock-bake validator)

| plane | before | after | capture-domain | real |
|---|---|---|---|---|
| type | 2,244 (3.42%) | **131 (0.20%)** | 0 | 131 |
| height | 4,483 (6.84%) | **140 (0.21%)** | 85 | 55 |
| shading | 15,432 (23.55%) | **61 (0.09%)** | 0 | 61 |
| angle | 9,640 (14.71%) | **5,373 (8.20%)** | 0 | 5,373 |
| ceiling | 4,770 (7.28%) | **132 (0.20%)** | 80 | 52 |

The "one-cell X-shift at map edges" in the original report was law 1
seen edge-on, not a wrap/clamp bug: nothing is shifted globally
(rolling the port ±1 in x or y makes every plane WORSE by 3-4x).

### The capture-domain share is now MEASURED, not guessed

Level 3 was recorded twice (the take was re-recorded mid-dig). The
two takes' record-0 bases are **byte-identical on type, shading and
angle, and differ on exactly 85 height + 80 ceiling cells** — all six
`(14,2)` CAVE PILLAR footprints, and monotonically (old take floor
46 / ceiling 91, new take 48 / 89). That is a pillar mid-ANIMATION:
each pillar carries a co-located `(10,64)` riser trigger at
disposition 0 (slots 646-651), which `fire_disposition(0)` arms at
load and which drives the pillar's RETRACT arm on the first live
ticks — after the load settle, before record 0. Our stock bake is a
zero-tick world, so it cannot show it. **Those cells are capture
domain, not a generator or carve bug** — the two-take A/B is the
free discriminator, and it should become standard practice: record
any stock-bake level twice and the cells that MOVE between takes are
exactly the pre-record runtime edits.

### Remaining (all attributed, none unknown)

- **One cave-tube leg, x 158..170 / y 17..28** (55 height, 51
  ceiling, 61 shading, 72 type cells): chain 21→22→23 around node
  (165,22). Both sides carve it; ours runs 2-3 units LOW in both
  floor AND ceiling, i.e. the `sub_34540` rolling midline baseline
  (`x_BYTE_F01FEx[2+0]`, the 32-sample `(floor+ceiling)/2` buffer) is
  2 low for those steps. Ruled out by reading: the chain walk and
  FROM/TO order (Events.cpp:5352-59), the packed-radii nibbles
  (EV:5348), `MoveEntity_57FA0` ≡ our `polar_step` (Player.cpp:6-19,
  pitch 0), the box/abs-wrap arithmetic, the `sub_34B00` wall ring,
  and the buffer shift order. Only 1 of ~55 legs diverges.
- **A 59-cell TYPE-only cluster at x 145..152 / y 79..88**, adjacent
  to the `(152,85)` pillar — downstream of the same capture-domain
  pillar difference (the classes under a retracted pillar differ, so
  the blend retile picks other textures).
- **angle 5,373 = the ORIENTATION NIBBLE ONLY** (bits 4-6; class,
  seal bit 3 and lock bit 7 all agree to within 225/0/0 cells).
  `sub_462A0`/`AddBuildingToTerrain_46570` draw it from the shared
  terrain LCG `rand2_17B4E0` (Terrain.cpp:1995-99, EF:31142-45),
  whose post-generation state we reconstruct with
  `post_generation_pseudo_rand` — and that reconstruction is CORRECT
  (a brute-force solve over all 65,536 seeds against retail's own
  nibbles picks our value, 777). The stream is a per-draw one: an
  offline alignment of our 314,196 load-time draws against retail's
  captured nibbles agrees **1.000 through draw ~154,500** (all
  mesas, domes, pits, hills and most tube legs) and then goes random
  — one missing/extra draw in a tube carve near the y-wrap at
  x 196..201 / y 253..1 desyncs everything after it. Presentation
  only (texture rotation), so it costs no sim fidelity, but it is
  the single remaining lead with a known first-divergence index.

### Instruments added

- `mgc-conform terrain-diff --out <dir>` — dumps both sides of every
  plane as raw 256x256 byte images (`<plane>.retail`/`.port`) for
  offline clustering.
- `mgc-conform terrain-diff --baseline <dir>` — reads the MEASURED
  planes from an earlier `--out` dump instead of the take's own
  record-0 base, so an attribution stays reproducible after a
  re-record (and so two takes can be A/B'd against each other).
- `mgc-import --example tmp_mc2gen <index> <out>` — the generator's
  planes before the load carve; the third side of the triangle.

**Goldens re-pinned (behavior change toward retail):**
`mc2_cave_behaviors_and_goldens` GOLDEN and OBSERVABLE, all four
checkpoints including the load checkpoint — as it must be, since the
terrain plane the projection hashes differs before tick 0.
Provenance comments carry the citations above. `cargo test -p
mgc-sim` 18/18 suites green, `cargo test -p mgc-conform` suite green
(no fixture regressions), `mgc-import` generator golden unmoved.

## THE MC2 BURST MANA LANE — THE RECORDED `manaRegen` IS ONLY THE APPLIED ONE WHEN THE MANIFESTATION SITS ABOVE THE CARPET (dig agent, 2026-08-06; mc2l3 take-2's #1 family, 3,738 pairs)

`player.mana` + the carpet's `mana` were 3,710 pairs each on the fresh
mc2l3 — the take's largest family — with the dominant shape
`port = want + 100`: one un-suppressed regen quantum per casting tick.
The same lane also swallowed a 40,000-mana Create Castle whole
(t=8445, want 1,359 got 41,359).

### 1. THE LAW

Both engines run the wizard's mana as applied-then-recomputed:
`AddPlayer03_00_5E010` does `mana += manaRegen`, then recomputes
`manaRegen` to the regen floor (EF:59996-60033). The CAST machinery
writes the same word from somewhere else entirely — `sub_68DE0`
(EF:55569), called from the manifestation's OWN class-15 action:

```c
if (a1x->word_0x2E_46 == a1x->word_0x30_48) {          // FIRST tick
    v3 = a2x->manaRegen_0x88_136;                      // a2x = the CASTER
    a2x->manaRegen_0x88_136 = v3 >= 0 ? -a1x->maxMana_0x8C_140
                                      : v3 - a1x->maxMana_0x8C_140;
} else if (a1x->word_0x2E_46 && a2x->manaRegen_0x88_136 > 0)
    a2x->manaRegen_0x88_136 = 0;                       // the mid-burst PIN
```

Both live in the SAME ascending entity walk, so **the manifestation's
pool slot against the carpet's decides which of the two writes the
recorder's frame-tail snapshot catches**:

- **token ABOVE the carpet** — wizard applies, recomputes, then the
  token overwrites. The record holds the token's stamp and applying it
  next frame is exactly right. (mc2l24 slot 118 vs carpet 116:
  `d88` −100 with mana flat, then mana −100 with `d88` 0, then flat.)
- **token BELOW the carpet** — the token stamps first, the wizard
  applies it and then recomputes. The record holds the RECOMPUTED
  FLOOR; what the next frame applies is whatever the token stamps
  then. (mc2l3 t=9033-36: `d88` pinned at 100 while mana goes
  41359 → −100 → flat → flat → +100.)

Two exceptions, both retail's own dispatch:

- **The CASTLE (spell 2)** never re-enters `sub_68DE0` while its timer
  is parked: that timer is an upgrade LOCK, not a countdown. mc2l3
  t=8446+ holds `word_0x2E_46` at 100 while mana climbs +1000/tick.
- **Any action but 0** skips the regen block outright, because the
  block lives in the action-0 body. Actions 2/3 are the corpse (see
  the death-law entry — their HELD delta is the reset's 750/2000
  residue). Action **12**, the level-end sequence
  (`sub_5E8C0_endGameSeq`), freezes mana for its whole run: 176 of
  mc2l3's 177 action-12 ticks apply 0 against a recorded 100.

### 2. THE FAMILY DECOMPOSES EXACTLY

Census over all 22,798 pairs (recorded `d88` vs what retail's mana
actually did, clamped pairs excluded):

| shape | pairs |
|---|---|
| a live burst BELOW the carpet | **3,407** (2,186 first-tick, 1,221 mid-burst) |
| the carpet in a non-action-0 body (death fall / corpse / end seq) | **330** |
| a live burst ABOVE the carpet | 1 |
| **total mismatched** | **3,738** |

which is the reported `player.mana` 3,710 plus the handful the clamp
filter dropped. Nothing else is in there.

### 3. WHAT LANDED

- `conformance::mc2_applied_mana_delta` — the importer now seeds the
  word the wizard body will APPLY, not the one the recorder caught:
  replay `sub_68DE0` over the human's book manifestations that sit
  below the carpet, in slot order. Non-action-0 carpets seed 0.
- `World::mc2_same_frame_debit` — the FIRST-tick debit is deliberately
  NOT pre-applied by the importer. The port stamps it in the
  manifestation pass exactly like retail and lands it there, which
  keeps retail's ORDERING: `mc2_afford` reads the purse BEFORE the
  debit. Pre-applying it instead (tried first, measured) made the gate
  see 1,359 against a 40,000 cost at t=8445 and the entire Create
  Castle cast vanished — the build ball and its (10,43) painter both
  went missing. Native play is untouched (the human owns no pool slot,
  so the stamp pends a tick like retail's).
- `MGC_NO_MC2_BURST_DELTA=1` restores the pre-dig import (both halves)
  for A/B.

The book entry must still BE that manifestation in its owned action
state `3M`: the death scatter parks a boolean 1 marker in the book and
a wraith-stolen jar runs action 78 — neither reaches `sub_68DE0`.

### 4. NUMBERS (windowed A/B, `MGC_NO_MC2_BURST_DELTA=1` = before)

conforming | unexplained field / missing / extra:

| window (mc2l3 take-2) | before | after |
|---|---|---|
| **13500 +500** (burst-dense) | 109 conf, 1,700 / 0 / 31 | **237 conf, 946 / 0 / 31** |
| **14500 +500** | 169 conf, 1,481 / 0 / 10 | **290 conf, 851 / 0 / 10** |
| **10000 +500** | 148 conf, 4,954 / 5 / 32 | **208 conf, 4,344 / 5 / 32** |
| **22620 +178** (the level-end sequence) | 354 / 0 / 0 | **2 / 0 / 0** |
| **9028 +20** (possess re-arms) | 44 / 0 / 0 | **26 / 0 / 0** |
| **8443 +6** (Create Castle, 40k) | `player.mana` want 1,359 got 41,359 | **conforms** |

Full take (`verify-deltas recordings/mc2l3.mgcr`, 22,798 pairs):

| | before | after |
|---|---|---|
| conforming | 14,276 | **15,706** (+1,430) |
| conforming + explained | 14,546 | **15,999** |
| UNEXPLAINED field | 42,414 | **34,986** (−7,428) |
| UNEXPLAINED missing / extra | 335 / 340 | 335 / **337** |
| **`player.mana` pairs** | **3,710** | **10** |
| carpet+entity `mana` hits / pairs | 5,606 / 4,632 | **1,904 / 1,626** |
| rng mismatched pairs | 4 | 4 (see §5) |

Cross-take regression guard: `mc2l24` @45000 +400 conforming
140 → **155**, unexplained unchanged; `mc2l24` @20000 +400 and
`mc2l0` @3000 +400 both unchanged. All six fixture suites `as
expected`, including the newly frozen `conformance/mc2l3.json`
(1,452 fixtures, 0 regressions, 0 drifted).

### 5. THE FOUR RNG PAIRS (mc2l3 take-2)

- **22621** was ours: `player.mana` +100 in the level-end sequence,
  now clean. Its rng lane stays mismatched `(1, >16)` — retail's
  action-12 frame draws ONLY the frame-top LCG step (the mover, and
  with it the cave tail, is parked), while the port still ticks
  something through the end sequence. That is the end-sequence lane,
  not the mana one.
- **14275 / 14395 / 14491** are one shape and NOT this dig's: slot 96
  reads `action 97` in retail against the port's `105`, with a stray
  (10,0)/(10,45) spawned in the port each time. A creature state
  machine one step ahead — worth its own probe.

### 6. RESIDUE + LEADS

1. **The 10 `player.mana` pairs left are the FATAL-HIT frame** (first
   at t=7884, the frame that ends with `action45` 2). Retail takes the
   killing damage in `sub_5EFA0`, which runs BEFORE the `life >= 0`
   gate, so the whole regen block is skipped on the way into the death
   fall; the port applies its imported delta at the top of `tick()`,
   ahead of its own damage intake, and lands a −250 retail never
   applied. Same root as lead 3.
2. `mc2_afford`'s castle-store gate (`f136` against the castle's
   stored mana) is evaluated against the port's own castle bank, which
   carries its own residual — a starved bank there would now suppress
   a cast the importer already zeroed the regen for. Not observed on
   any take in the corpus.
3. The port still applies the wizard's mana at the top of `tick()`
   rather than at the carpet's slot in the walk. `mc2_same_frame_debit`
   is the targeted repair for the one observable that ordering breaks;
   a general fix (running the human's mana body from the walk at
   `mc2_carpet_slot`, where `mc2_player_cast_pass` and the cave tail
   already run) would retire this whole class — including the death
   column's own one-frame puff artifact.

## LIGHTNING/FIRE FIELD-ROW TRIAGE + VISUAL-ONLY DOCTRINE
## (session lead, 2026-08-06, player-ruled)

Post-dig triage of mc1hwl0's 796k unexplained field rows; player
ruling generalized into docs/CONFORMANCE.md §roster "Visual-only
families". All claims verified against consumers before
classification:

1. **(9,9) lightning trail nodes (~331k rows) = capture.** Born-dead
   by law (maxLife=(node>=beam)-1), no victim scan — storm kills
   ride the beam's acquire exclusively (storm dig). Row shapes:
   life −1 vs −2 corpse stamps; max_life 0-vs-300 = different record
   in slot; x/y bimodal — >60-tile (different record) or <0.12-tile
   (one-node index shift along the beam). Rules
   mc1hw-lightning-node-{life,maxlife,x,y}. REAL signal = record
   counts (harness metric still owed) + missing/extra atoms, which
   stay unexplained.
2. **(10,0)/(10,6) fire churn** — fire_tick moves only the z flicker
   and never reads f30: heading = write-only spawn stamp → capture
   (mc1hw-fire-churn-heading, 67k). Same-slot x/y/rand = a DIFFERENT
   fire in the slot, knock-on of the ambient spawn-cadence
   divergence (the undug weather-churn lead) → classified **open**
   citing the parent (mc1hw-fire-churn-{x,y,rand} 75k,
   mc1hw-standing-fire-churn-{x,y} 11.7k) — explained, still on the
   books.
3. **(10,13) smoke puffs = fully visual entity** (smoke_puff_tick:
   rise + 16-tick drift + sprite step, no damage, nothing scans it)
   → blanket field rule mc1-smoke-puff-fields (50k hw + 15 mc1l0).
4. **Boundary case proving the bar: (10,39) ball heading is NOT
   visual** — re-derived per tick and fed to the merge-walk's
   polar_step (:4119) → rows KEPT (ball-merge lead symptoms).

**Close: mc1hwl0 unexplained field 796,115 → 260,818 (−67%);
missing/extra untouched by design.** Remaining top families: (5,15)
guards (~42k — now live leads on measured terrain), mana cadence
(~26k incl. the :385 clamp over-fire lead), (10,40)/(10,39) known
leads, long tail. Doctrine: visual-only = capture with consumer-read
proof cited; knock-ons of real leads = open citing the parent; field
rows only; hit-count jump = re-verify.

**CROSS-GAME SWEEP ADDENDUM (same day):** the visual-only pass was
run over base-MC1 and MC2 as well. MC2 fire tick verified
(mobs.rs mc2_fire_tick): positions never move, z = the f44 flicker
roll, PITCH never read = write-only spawn stamp — but MC2 needed no
new rules: the existing mc2-fire-churn-m0/m6/m13 blanket rules
(cadence dug-to-completion and RULED capture in the session-4/8
rounds) merely lacked the mc2l3 take scope — extended. MC1 fire
rules extended to mc1l0 (same stationary-fire law; its open cadence
lead stands). **Post-sweep: mc2l3 unexplained 34,986 → 29,386
field / 212 missing / 144 extra · mc1l0 8,505 → 8,054.** Everything
left in both games' top families is gameplay-bearing: MC2 (10,40)
aura-pull, (10,39) ball physics, (9,0)/(9,1) bolt aim, (5,19)
firebug stagevars; MC1 (10,39) merge-walk heading, mana cadence,
(9,x) aim. No further visual-only candidates above the noise floor.

## HW:0 LIVE REBOUND — playtest "still no rebound" TRIAGED: mechanism WORKS live, the gate is the 8000-stored era (2026-08-06)

**Player report (post-fix playtest):** hw:0 Vodor still never shows
Rebound in the port, "even though he has it pretty much the entire
time in retail."

**Method.** Temporary live-game probe (deleted; recipe: build hw:0
from `baked/mc1hw/level-000.mgcl` + `mc1-arctic` via
`World::new_for_game(Mc1Hw)` + `set_wizards`, drive scripted
`PlayerPose`/`PlayerCommand` ticks, watch `debug_rival_ai()` +
`debug_pool()` — the pool view now carries `f26/f140/f136/f144/f146`
lanes for exactly this kind of dig). Retail side read straight off
the corpus with `mgc-conform dump-state recordings/mc1hwl0.mgcr <t>
<slots>`.

**Findings.**

1. **Config is right.** hw:0 wizards.json slot 1 (Vodor): Rebound
   pregranted+allowed (book[14] set, manifestation minted at spawn),
   `castle_level` 0 — and retail t=10 confirms NO rival castle
   exists (slot 473 wizard only), so the authored-castle question is
   closed: retail Vodor bootstraps homeless too.
2. **The whole arm works end-to-end in a live game.** With his
   castle storing >= 8000, EVERY fireball volley opened a window
   within ~5 ticks: trigger scan (class-9 f146 = him, <= 1024),
   ladder pick, `rival_cast_ready`, commit past the castle gate,
   f26 = 101 countdown, `flags|0x8000` published, deflection
   returned the volley (and eventually killed the probe player).
   A realistic 25k-tick session (64-tick hit-and-run volleys every
   1500 ticks) produced 19 windows / 1,900 bounce-ticks, including
   the re-up chatter retail shows, and even reproduced the
   rebuild arc: stray volley fire collapsed his castle (lvl 4 ->
   gone), he re-planted at a new site, re-banked 10,000 in ~1k
   ticks, and went back to bouncing.
3. **The gate that hides it is the Rebound castle_req 8000 — an
   ERA, not a bug.** Both engines bank the same curve: first haul
   4,490 by t~1-2k, stall while the map's ball supply is dry, then
   cross 8,000 on the next wave. Retail crossed at ~5.5k (first
   corpus window 5,591); the port crossed at 6.8k/9.6k/15.8k with a
   PASSIVE player (variance = ball scarcity) and at **t=750** with
   an ACTIVE one (the player's own fighting feeds the world's
   balls). Corpus cross-check: the identical 4,490 first-haul
   appears in retail slot 522 f140 at t=1000 — the economy port is
   faithful; wealth split during ferry (balloon cargo vs banked)
   lags retail ~1.2k ticks at worst.
4. **Retail "permanent" decoded.** The take has just SEVEN windows
   (~1.4% of ticks): Rebound is REACTIVE — the player only observes
   it when attacking, and after t~5.5k every attack provokes it,
   hence "permanently up." Retail's era ended at ~21-26k when the
   player wrecked castle 522, Vodor re-planted, died castle-less,
   and was ELIMINATED (slot 473 corpse states from ~31k on).

**Verdict: no port defect found.** What the playtest needs in order
to SEE it: engage Vodor a few minutes in (after his castle banks
8,000 — faster the more the level is actually being played), don't
eliminate him first, and expect bounces only WHILE provoking him
(4-second windows, re-upped per threat). If a session with those
conditions still shows nothing, the next suspect is the app binary
being stale (the fix tree must be rebuilt), not the sim.

**Tree delta:** `DebugEvent` (debug_pool) gained the f26/cargo/cap/
owner/chase lanes (diagnostics-only, hash-silent). Suites: rivals
unit tests, level-005 goldens, fmt — all green.

## HW REBOUND ROUND 2 — the deflection itself was broken: sound-only "rebounds" (2026-08-06)

**Player report (second playtest, rich Vodor confirmed):** the rebound
SOUND plays, but the meteor explodes on him, hurts no one, and no
projectile ever comes back. Reproduced the triage conclusion from
round 1 (the arm/bit/economy are fine) and found the real remaining
defect in the deflection reader.

**Retail law (sub_52B30 :62858-90, re-read from source).** On a bolt
striking a victim whose `+17` bit 7 is set: quarter = the bolt's
`+140 / 4` (signed); **afford gate** `quarter <= victim->+140` — the
victim's +140 is his MANA (retail keeps a wizard's mana ON the
entity). On success: sound 28 positional at the victim (**inside**
the branch, :62861), `victim->+140 -= quarter`, heading reversed +
rand%0x5B−45 scatter, pitch negated, chase = the original shooter,
owner = the deflector, life refreshed, bolt relinked at the victim
LIFTED by `victim->+84`, and NO explosion. On afford-fail: **nothing
at all** — no sound, no debit, no hit; the bolt flies straight
through (the :62859 false arm leaves the explode flag clear).

**Port defects found.**
1. **The rival wizard entity's `f140` was never written** — rival
   mana lives in `Rival::mana` and the entity field stayed 0 for
   life, so the afford gate compared against 0 and EVERY real bolt
   failed it. (`RivalAiDebug`'s doc comment even claimed the mirror
   existed.)
2. The port played sound 28 BEFORE the afford gate — the reported
   sound-but-no-rebound.
3. On afford-fail the port fell through to the normal
   teleport-onto-victim EXPLODE — retail flies through.
4. The deflected bolt relinked at the victim's z without the `+84`
   lift.

**Fixed.**
- `crates/mgc-sim/src/mc1/rivals.rs`: the +140 mana mirror —
  seeded at `spawn_rival`/`rival_respawn` (1000), published at the
  end of every `rival_alive_tick` (= `Rival::mana`), and reconciled
  DOWNWARD at the tick top so combat-side quarter debits land in the
  pool. Downward-only: every port-side credit writes `Rival::mana`
  first and re-publishes the same tick.
- `crates/mgc-sim/src/mc1/combat.rs` (`proj_move_and_hit`): sound 28
  moved inside the afford branch (positional at the deflector);
  afford-fail = silent fly-through (advance + return, no explode);
  deflect relink lifted by the victim's `f84`.

**Verification.**
- New unit test `rebound_deflection_bounces_debits_and_is_silent_when_poor`
  (rivals.rs): world maintains the mirror; a direct `proj_tick` on a
  parked bolt against the rebounding wizard deflects (no explode,
  owner swap, re-homed on shooter), debits exactly quarter (1000 →
  900 for a 400-mana bolt), twangs; the poor arm (50 mana vs quarter
  100) flies through silently with no debit. NON-VACUOUS: verified by
  toggling each sub-fix off — the mirror off fails the mirror assert,
  the old explode/sound restored fails the fly-through + silence
  asserts. Test trap for posterity: park the encounter AWAY from the
  starting castle — its 0x2000-tall envelope scan-resolves the CASTLE
  instead of the wizard hovering on it.
- Live hw:0 probe (temporary, deleted): rich era at t=6,839; volley
  → bit ON at +5; **returned bolt (class-9 owned by Vodor, chase =
  PLAYER_TARGET) at +10**. The player-visible loop is closed.
- Goldens: `level_005_golden_state_hashes` re-pinned — attributed by
  toggle to the f140 mirror ALONE (a deliberate hashed-field change;
  the deflection restructure is golden-silent). Observable goldens
  unmoved. All 18 mgc-sim test bins green; fmt clean.
- Conformance: mc1l0 492 / mc1hwl0 852 / mc2l3 1,452 fixtures — all
  as expected, 0 regressions, 0 drift (the rival lanes are inert
  under import; the mirror writes retail's own value back).

**BANKED — MC2 twin:** `mc2/proj.rs` has the same quarter-debit
pattern against entity f140. MC2 rival Rebound windows are not yet
mirrored onto their entities (existing DEVIATIONS note), so the gate
cannot mis-fire there today — but when the MC2 rival rebound lands,
it needs the same mana-mirror treatment. Also banked: the PLAYER
deflection arm keeps its INTERIM no-debit (sound placement now
matches retail; the debit needs the player pool reachable from Gen).

### ROUND-2 ADDENDUM — playtest PASSED; retail "stuck flag" lead OPEN (2026-08-06)

Player confirms deflection now works live. New lead from the same
report: retail Vodor remembered as rebounding AT ALL TIMES, even
castle-less — suspicion that the flag gets stuck. Corpus second look
(dump-state slot 473 `flags` at 5600/7000/21400/24000/26000/31000/
42000/49000): **this take is clean** — bit 15 ON only inside the
seven windows, OFF between them, OFF through the final death and the
entire corpse era; and window 7 (21,372→21,473) is law-consistent —
he had REBUILT (slot 233, level-3 castle, stored 13,768 — the number
the round-1 instrumentation saw). The take never TESTS the stuck
hypothesis though: he never died mid-window. Mechanism is credible:
retail clears `+17` bit 7 only from the token's burst tick
(sub_573F0_57920), and rival death scatters the manifestations into
decaying ground jars (:55519-49) whose handlers never touch the bit
— a death with the window live should orphan the bit ON permanently
(castle-less, across respawns). The port deliberately deviates (the
death transition drops the bit — see the ROUND-1 fix notes).
**Discriminator for a future retail take: kill the rival within ~4 s
of a deflection, then check whether everything bounces off him for
the rest of the level (flags lane stuck 0x8000 through state 3).**
If confirmed: player ruling — keep the sane deviation (DEVIATIONS.md
entry) or port the stuck flag faithfully.

## V2 CORPUS INTAKE — mc2l0 / mc2l4 / mc2l30 / mc2l24 (2026-08-07)

The 2026-08-06 evening re-records intaken and FROZEN per the
freeze-at-intake law; these four sections supersede the "Baseline
corpus" block's numbers for their levels. Pipeline per level:
check-decode → terrain-diff → full verify-deltas (`--csv` at repo
root) → extract → carry_curation → classify_fixtures → freeze →
suite gate. ALL SEVEN suites green at close.

**check-decode (all 100% clean, terrain base present):** mc2l0
22,696 ticks (659 deltas / 37,913 cells) · mc2l4 17,819 (626 /
128,685) · mc2l30 12,555 (803 / 118,614) · mc2l24 67,268 (3,821 /
722,177). The mc2l4/mc2l30 cut (t=17,820) with the materialized
part-B base decodes clean on both halves.

**Stock-bake terrain-diff (first ever on these four levels; cells
of 65,536):**

| take | type | height | shading | angle | note |
|---|---|---|---|---|---|
| mc2l0 | 34 | 15 | **3,676 (5.61%)** | 33 | shading = SUM-64 signature |
| mc2l4 | 28 | 1,357 (2.07%) | 583 | 940 (1.43%) | unattributed, first look |
| mc2l30 | 8 | 288 | 88 | 345 | local clusters (x≈182-206) |
| mc2l24 | 1,086 (1.66%) | 729 | 429 | 3,088 (4.71%) | ONE region x≈120-134,y≈101-105 |

- **mc2l0 shading: every example sums to 64 (retail = 64 − port)** —
  this is the banked night-shading gap made measurable (mc2:0 is
  MapType Night; port bakes Day relief shading — the flag must reach
  construction, the known ~60 `World::new` sites item). 3,676 cells =
  the relief-shaded population. The corpus now grades the fix.
- **mc2l24 (Night+doom): NOT the inversion** — diffs cluster in one
  region (the citadel footprint): type retail 8 vs port 77/79,
  shading a constant −8, angle 4.71%. Reads as an authored
  citadel-stamp bake difference, one story.
- **mc2l30 = MapType CAVE (level.json: cave, basic_height 173) and
  the take has NO ceiling plane** — the conjoined take started on
  non-cave l4, so the recorder never declared ceiling (the cut
  caveat, as predicted). Ceiling runs PRISTINE-GENERATED in verify;
  in-level cave carves are unshielded there. The floor planes'
  small residues cluster locally (eruption/spawn-stamp shaped, e.g.
  height ±2..4 at x=197-203,y=16-17). A future native-l30 take
  would close the ceiling.

**verify-deltas headlines (terrain MEASURED everywhere):**

| take | pairs | torn | conforming raw | conf+explained | UNEXPL field/miss/extra | rng |
|---|---|---|---|---|---|---|
| mc2l0 | 22,695 | 0 | **17,153** | **21,872 (96.4%)** | 2,288 / 33 / 26 | 1 |
| mc2l4 | 17,818 | 0 | 9,716 (was 0!) | 14,898 (83.6%) | 9,406 / 57 / 43 | 4 |
| mc2l30 | 12,553 | 0 | 2,657 | 10,986 (87.5%) | 4,221 / 37 / 42 | 1 |
| mc2l24 | 67,233 | 329 | 13,251 | 27,740 (41.3%) | 367,145 / 445 / 569 | **3** |

RNG stays essentially locked corpus-wide (9 mismatched pairs across
120k) — even through the l24 endgame frenzy (old take: 1,816 torn;
new: 329).

**mc2l24 freakshow scoping REFUTES the convenient story:** the
unexplained bulk is MID-GAME — t=5k-25k holds ~254k of the 367k
field rows; the post-victory stress window (t=50k-70k) only ~78k.
Biggest single family in the GROSS entity-set table: 22,093 missing
(10,39) mana spheres from t=2,857 on — **CORRECTED at triage: all
22,093 are already claimed by the `mc2l24-ball-terrain-roll` capture
rule** (fountain/merge/summit-grounding closure, 2026-08-04 dig) and
none reach the unexplained headline (445 total unexplained missing).
A spot dump (slot 357, t=2,857→2,858: alive 300/300 both ticks in
retail, port kills it within ONE tick of import) is consistent with
that rule's merge/grounding story rather than a spawn miss; the
port-side "N spawn(s) dropped" bursts live in the LATE stress
window only (pairs 36k+) and remain the entity-cap story. Second
family: (9,9) lightning 9,527 missing / 2,539 extra from t=8,222.

**Cross-take unexplained families (the joint-triage roster; all
takes agree, exemplars in the repo-root TSVs mc2l0/l4/l30[-v2].tsv):**

1. **player.life +5 skew** — (3,0).life + player.life mirrored, all
   four takes (111/426/443 rows + l24): after a damage event the
   port sits EXACTLY +5 above retail persistently (retail 9500 /
   port 9505 for hundreds of flat ticks), damage tick itself one
   frame apart (t=265 l30: retail 10000 port 9220). Reads as one
   extra +5 life-regen application around the damage frame — the
   frame-ORDER class again (cf. the burst-mana debit law).
2. **(9,9) lightning missing** — l4 1,355 missing from t=928 (the
   3-player rival level), l24 9,527 from t=8,222. Port draws far
   less lightning than retail in rival fights.
3. **(10,17) spawn pitch −2 + seeded mana** — CORRECTED by the
   want/got distribution: retail applied_pitch walks multiples of
   192 (192/384/576/…) and the port is EXACTLY −2 on every rung
   (not compounding) → a spawn-time applied_pitch offset of −2 with
   an identical 192/tick rate. Same family also shows port seeding
   mana 8000/16000 where retail keeps 0 (l30: 103 of 106 mana
   rows). (l30 965 + l4 417 pitch rows.)
4. **(10,39) mana-sphere speed** — port pins 16, retail varies
   (17/24/28/63); all takes (472/122/108 rows + l24's missing mass).
   Sphere speed law, not a constant.
5. **(5,9) action 72→73 at import** — l4 t=0 (354 rows over the
   whole starting population), l30 a t=2,298 burst: retail parks at
   action 72 (39 rows also 79) where the port writes 73 — the port
   collapses a 72..79 state band onto 73. Import/state-mapping
   off-by-one class (the f66→f70 lesson).
6. **(10,79) spawn z** — port spawns at flat z=1760 where retail has
   terrain-varied 800-1152 (l30 t=1,114 burst; l4 437 z + 253 x
   rows). Spawn-z law.
7. Lesser recurring: (10,1).mana, (10,45).life, (5,x).sv1 families,
   (3,3) rival pose (x/y/heading), (10,40).mana_max l0 t≈11.2k
   (the aura-pull lead's family).

**Suites (frozen at intake, gates green):** mc2l0 1,740 fixtures
(1 carried + 23 stories; classify 10 capture/13 open; one
select-dependence FIXED at t=11,322 promoted) · mc2l4 996 (15 open /
8 capture; t=2,278 (15,9) select-dependence demoted open) · mc2l30
290 (t=3,586 (15,21) ditto) · mc2l24 1,349 (0 carried — the
freakshow shares no stories with the old take; 16 capture / 8 open;
green first run). The (15,x) select-dependence pair = the known
warning-grade shared-world leak (2026-08-01 suite note), now with
two more exemplars, both class 15.

### mc1hwl0 gate: 5 rival-mana fixtures parked on the REBOUND-PENDING pile (2026-08-07)

The all-suite pass found mc1hwl0 at 846/852: five fixtures
(t=21,039 / 24,145 / 24,163 / 24,264 / 25,922), ALL `field:(3,1)
mana`, frozen as conforming at session-11 close, now +100
port-over-retail. Full-take windowed re-verify REPRODUCES it (not
suite-context): retail's rival drains ~100-150/tick in these
stretches while the port, re-importing retail state each pair,
re-creates a +100 skew within ONE tick — i.e. retail is PAYING for
something per-tick that the imported port rival doesn't resume
(rival cast/channel state not reconstructed at import — the f66→f70
lesson's rival sibling). This is rebound/re-up territory and the
player owes the retail triple-check (ROUND-2 addendum above), so:
demoted open with a note, t=140 FIXED promoted, suite green
852/852. Revisit WITH the rebound ruling. (Why it was green at the
session-11 freeze and not now is unexplained — the tree is the
committed db356a7 state; suspect the freeze predated the last
rival-arm edits of that session.)

### JOINT-TRIAGE ANSWERS (player, 2026-08-07) — three redirects

1. **l24 mana spheres: "only after victory" did mana lie around** —
   the player collected as they went mid-game. This REFUTES the
   lying-mana/entity-cap framing for the 22,093 missing (10,39)
   rows from t=2,857: the early/mid-game missing spheres need a
   different story (sphere spawn/lifetime law, not cap pressure).
   The port-side "spawns dropped" bursts (pairs 36k+) remain the
   late-game cap story only.
2. **Life regen (player concept, TO VALIDATE FROM DECOMPILE):**
   "life regenerates extremely slowly; speeds up at dolmen/shrine/
   castle; outside of that it's so slow it feels disabled." The +5
   skew = port applying a regen quantum around the damage frame
   that retail gates. Dig authorized.
3. **(9,9) lightning is NOT wizard-cast: "primarily castle
   defenses; I do not recall players using it. In l24 it's also the
   hydra and to some extent the final boss (Vissuluth)."** So the
   under-firing arm is the CASTLE-DEFENSE lightning (and hydra m27 /
   Vissuluth breath arms on l24) — not the rival reactive-cast
   ladder. l4's t≈928 onset = when rival castles first stand.

**Dig order (player selected ALL four lanes):** ① life +5 frame
order (decompile validation of the regen concept) → ② l24
mana-sphere drop (now a spawn/lifetime story) → ③ castle-defense
lightning arm → ④ the constants batch ((10,17) 384-vs-382 rate,
(10,39) sphere speed, (5,9) import 72→73, (10,79) spawn z 1760).

### ⭐ LANE ① LANDED SAME DAY — the life+5 family was the UNSEEDED REGEN-STALL COUNTER (2026-08-07, FIXED)

Opus decompile dig, spot-verified, then landed. **The port's live sim
law was already correct** (world.rs `regen_delay`: armed 16 on
hit/grip/steal, damage-before-regen order, 10000/2000 = +5 afield vs
10000/250 = +40 at castle/dolmen — validating the player's concept
exactly: slow ambient, 8× at castle/dolmen, and the 16-tick stall
re-armed under sustained fire is the "feels disabled"). The
divergence was VERIFICATION-DOMAIN: retail's stall counter
(MC2 `dword_0x18D_397` = wizext +397, EF:60000-60003, armed
EF:60662/60710/62222; MC1 `u32_383` = +383, sub_main.cpp:55387-90 —
the agent's report said MC1 +397, corrected against remc1 before
landing) was never decoded by the recorder structs, so BOTH
importers seeded `regen_delay = 0` every pair → each pair inside
retail's stall window applied exactly one heal quantum retail
withheld → the dead-flat retail+0/port+5 (or +40) runs, ~15 pairs
per hit, on every MC2 take.

**Fix (pure decoder+importer, no sim change, no re-record — the
bytes were always in the recorded struct images):**
`mgc_formats::mgcr` decodes `regen_stall` (MC1 t+383 u32, MC2 t+397
i32); both conformance imports seed `regen_delay` from it.
PROOF: the l0 t=4103-4160 and l30 t=265-271 flat runs are GONE;
survivors are only the one-frame damage-tick pairs themselves
(capture-window artifact — the attacker's mail is not in the
imported closure; roster-able as capture). All 7 suites green, 0
regressions 0 fixed; state_hash goldens pass (import-side change —
goldens can't see it). **Post-fix full mc2l0 re-verify: 17,494
conforming (was 17,153), conf+explained 22,249/22,695 = 98.0% (was
96.4%), UNEXPLAINED field 1,482 (was 2,288)** — ~800 rows retired on
the cleanest take; l4/l30/l24 carry proportional retirements
(re-verify at next full pass; intake headline table above still
shows PRE-fix numbers).

Banked from the same dig (second-order, masked until now): ① retail
consumes the STORED stale rate `lifeRegen_0x163_355` (+355; port
computes inline — only differs ±35 on castle/dolmen entry/exit
ticks; +355 is in the recorded bytes, seed when it shows up); ② the
l24 roster rules mc2l24-player-life-regen / mc2l24-wizard-life-regen
(and the life half of the mana twins) = this same root cause —
expect their hits to collapse on the next full l24 verify; retire
then; ③ human maxLife = 10000 × header scalar/256 — port hardcodes
10000 (all four takes are scalar 256; a non-256 level would bite);
④ AI wizards have NO stall in retail (EF:5426-5433) and heal
maxLife/200 home / /500 afield — unverified against the port's AI
arm, separate lane.

### LANE ③ MAPPED — the (9,9) lightning deficit IS the castle-defense turret burst arm (2026-08-07, Opus dig; player-redirected)

The player's triage answer ("primarily castle defenses; I do not
recall players using it; in l24 also the hydra and Vissuluth")
aimed this dig straight past the rival-cast ladder — and the
decompile confirms every element.

**The retail arm:** castle pieces (10,79) (ctor sub_508E0 EF:36987,
brain sub_3AF00 EF:30106-472) rebuilt on every castle level change
by sub_613D0 (EF:62234); tower TYPE byte_0x43_67 = the castle spell
tier's life_0x1A stamped by the research child sub_69AB0 (EF:56121)
— type 1 = fireball tower, any other non-zero = LIGHTNING tower.
Scan once per 64 ticks (byte_0x3E_62 & 0x3F), ring band 3..12, no
LOS/invisibility test; weapon roll: 94% → 6-shot burst, one shot
per tick, via the SHARED Lightning path sub_6DCA0 a3==7 → (9,9).
**Row multiplier:** the (9,9) beam (sub_4D860, speed 384, life 9) is
a one-tick hitscan laying steps*8+1 trail nodes (sub_66750
EF:58268-400) → one bolt ≈ 1+81 records, one burst ≈ 490 (9,9)
records. Secondary (9,9) sources CONFIRMED as the player said:
hydra (5,27) whip bolt ≈33% power roll (EF:20543), Vissuluth
doomsday pyramid case-2 volley, selector-gated ~10/29 (EF:13345);
plus one UNRESOLVED site EF:26708 (owner not in any action table).

**Corpus evidence both onsets = this arm:** mc2l4 missing (10,23)
impacts arrive in ~6-wide consecutive-tick runs (925-930, 1156-63,
…) — 48 retail burst windows, ~19 with NO port burst at all; (9,0)
balanced so the port is not rolling fireball instead. mc2l24: 392
missing-impact windows, ALL t≥8,223, starts 63-65 ticks apart —
the 64-tick re-scan clock verbatim.

**Port state: the machine is PORTED and largely faithful**
(castle.rs mc2_castle_piece_tick/scan/fire; proj.rs beam+trail) —
divergence list, ranked: **D7** one-tick aim-phase lag (every
(10,79) pitch row = port carrying retail's previous-tick value;
fits every burst; root walk-order vs fire-delay UNDUG — highest
value next step); **D-19-bursts** ~19/48 l4 bursts absent entirely
(candidates: D5 target-choice deviation, or the f67 tower-type
stamp path — port stamps SPELLS[2].tiers[tier].life at cast time
vs retail's research-child stamp at castleLevel+1 — compare f67
against the recording); **D1** aim order (retail aims from unlifted
muzzle at box-raised target, lifts z AFTER; port aims from lifted
muzzle at raw target); **D2** trail one node short + one spacing
downrange (drops retail's muzzle node — the mc1hw-lightning-node-x
shift); **D3** lateral jag accumulator (possible IDA artifact);
**D4** charged lightning doesn't chain (documented deviation —
also kills turret mode 2, 5% of bursts); **D5/D6** documented
deviations (target order, friendly exemption).

**⚠ ROSTER MASKING WARNING:** mc2-cast-timing-missing/extra swallow
ALL class-9 set rows — their own notes warn "ONE-SIDED counts = a
real port bug hiding here", and 1,355/21 (l4), 9,527/2,539 (l24)
is exactly that. mc2-lightning-blast-churn likewise masks the
strongest signal (217/0, 1,660/14 (10,23) missing). When the
turret fixes land, re-scope those rules or the win is invisible.

Also corrected: docs/traces/mc2-castle-runtime.md:422 conflates
(10,79) with the (5,15) guard — different brains (sub_3AF00 vs
sub_23C40); trace doc NOT edited this session, note carried here.

### REBOUND ROUND 4 — CLOSED BY PLAYER RETAIL PLAYTEST (2026-08-07)

Player ran the retail check: **reduced the rival's castle to rubble
→ the rebound went away.** That is the ported law exactly (reactive
re-up gated on castle stores ≥8000; castle-less = no re-up, window
lapses) — the remembered "rebounding at all times, castle-less" did
NOT reproduce. Player ruling: "everything is the way it should be
now; if I ever spot a deviation, I'll reopen." Topic CLOSED. (The
narrow die-mid-window jar-orphan discriminator from the ROUND-2
addendum was not the path tested; it stays moot under this ruling —
the port's death-drops-the-bit guard stands as the sane behavior,
no DEVIATIONS.md entry needed since no retail divergence was ever
observed.)

The five mc1hwl0 (3,1)-mana fixtures parked on this pending check
are hereby RE-SCOPED, not un-parked: with the rebound arm vindicated,
their +100-per-pair skew is a pure IMPORT-DOMAIN lead — retail's
rival is paying ~100-150/tick for something in those stretches and
the fixture import doesn't reconstruct the paying state (the exact
class the regen-stall fix just closed for the human). Banked as
"rival per-tick payment state unseeded"; fixture notes updated; dig
when convenient, no urgency.

### mc2l30 NATIVE RE-RECORD INTAKEN — THE CEILING CHANNEL IS CLOSED (2026-08-07)

Player recorded l30 natively — CORRECTED MECHANISM (player,
same day): NOT a savestate load. MC2 makes a reached hidden level
revisitable from the campaign map (red portal; flag once
completed). The player unlocked it via l4, exited, saved the
CAMPAIGN, restarted, and launched l30 directly — a NORMAL level
start, so the recorder pinned MapType=Cave and declared the
ceiling. (Mid-level save-load recording remains UNTESTED; player
rates it low priority.) Take REPLACES the cut one at
recordings/mc2l30.mgcr per player instruction (cut preserved as
mc2l30-cut.mgcr; regenerable from mc2l4,30.mgcr anyway).

- check-decode: 12,562 ticks 100% clean, 763 deltas / 119,257
  cells.
- **terrain-diff: FIRST MEASURED l30 CEILING — 0.13% (84 cells)**
  vs the port generator; floor planes 0.23-1.03% — MORE than the
  cut take's (8/288/88/345 cells), which a fresh map-launch would
  not obviously explain. OPEN QUESTION: does MC2 PERSIST hidden-
  level terrain across revisits (campaign state), or does the
  revisit re-bake with different spawn stamps? Until answered,
  treat the native base as ceiling evidence only; the cut take
  remains the t0-bake instrument for the floor planes.
- verify: 12,561 pairs, 0 gaps 0 TORN, 2,024 raw conforming,
  10,541 conf+explained (83.9%), UNEXPL 5,199 field / 98 / 22 —
  ceiling now INSTALLS measured per pair. ⚠ **rng 39/12,561** (cut
  take: 1) — cause unknown (save-load theory RETIRED with the
  mechanism correction; different gameplay or revisit state are the
  remaining candidates), long-tail item.
- Suite re-frozen: 227 fixtures (14 carried via sig-bridge from the
  cut-take suite, 10 new stories), gate green; ALL 7 suites green.
  Select-dependence claimed a THIRD exemplar (t=3,362, again
  (15,21) action/owner) — demoted+sig-refreshed like the others.

## PORTAL WARP-OUT ALTITUDE — mc2l24 enclosure deviation, the WHOLE
## teleport family re-pinned to retail z + speed laws (player-spotted,
## LANDED 2026-08-07, PLAYTEST OWED)

Player report: the mc2l24 start enclosure's pad warps the port flyer
at its PRE-portal altitude (x/y only, snap-up-if-buried), where retail
emerges ON the ground — and the too-high flyer drags the enclosure
monsters' aggro/aim upward (live-play cascade; conformance replays pin
the recorded pose, so the suites never saw it).

**Retail law (both games, resolved from the decompiles):** the warp
tick recomputes `dest.z = row->word_0xc + terrainAlt(dest)` on every
warp — MC2 `sub_35390` EF:25785-86, MC1 vortex `sub_26A60` :29212-13 —
and the pad keeps the NewEvent-default behavior row (`str_D7BD6[59]` /
`unk_98F38[0]`, byte-identical), whose word12 = **0**: the wizard
arrives exactly on the destination ground. This closed trace OPEN-2 in
docs/traces/mc2-class10-m50-chains-and-tail.md.

**The teleport SPELLS carry their own z laws** (read while in there —
MC1 `sub_56E50_57380` :65554, MC2 `sub_6AD60` EF:56860):
- castle hop → the castle entity's FULL axis (`CopyEntityPosition`;
  MC2 offsets −448 along yaw−204 pitch-0, MC1 has no stand-off);
- T1/return toggle → the SAVED axis restored verbatim (x, y AND z);
  the toggle is castle-gated, and the no-castle random hop CLEARS an
  armed return (:65585);
- random hop → pitch-0 `MoveEntity`, altitude rides along untouched;
- EVERY resolve arm zeroes the caster's flight TARGET speed
  (`Type_160 v_12` :65583/:65601; MC2 `speed_0xc_12` EF:57029), and
  the burst EXPIRY repeats the zero (:65614 / EF:57046) — the
  formerly-banked "flight-speed zero on resolve" follow-up landed
  with this round.

**Port shape:** `pending_teleport` widened to carry the arrival
altitude (`None` = keep, the pitch-0 arms); new `pending_speed_zero`
channel (`take_speed_zero`) → `carpet.tgt_speed = 0` (target only —
the actual chases down 16/tick, retail's glide-out); the consumer
re-seeds `lift_desired` at the arrival pose so a ground emergence
doesn't auto-climb back; `teleport_return` widened to the full saved
axis and cleared at the MC1 death scatter (retail's +154 dies with
the recycled manifestation). SNAPSHOT_VERSION 7→8. Tests: level-032
vortex + (10,34) pad asserts extended with the ground-z law;
`mc1_teleport_spell_z_and_speed_laws` covers the three spell arms.
Full workspace + ALL 7 conformance suites green (bit-unchanged, as
expected for a live-play-only fix).

Still owed on the pad: the rival-warp arm (retail warps EVERY player
in the list) and the `sub_5C800(player, 6)` palette flash — both
pre-registered in the (10,34) APPROX register, unchanged this round.

## TRANSIT-CLUSTERING PROBE (2026-08-07): the pin does NOT alias
## portal warps — and one dated whirlwind lead at t=31121

Cheap pre-test for the proposed player-pose channel: do mc2l24's
unexplained rows cluster after recorded player-position jumps (the
mid-tick-warp aliasing hypothesis — retail moves the player DURING the
entity walk, our pin holds one pose per tick)? Instrument:
`mgc-conform --example pose_dump` (new) + scratch clustering script
over `mc2l24-v2.tsv`.

**Verdict: NO broad aliasing.** 9 true pad transits found (>30-tile
one-tick jumps, flying both sides). All-warp windows 0.92x baseline;
transit-only windows 1.77x but the excess is ONE event — 7 of 9
transits sit at zero-to-baseline rows (the three early enclosure
entries are completely clean: 0 rows before AND after). Mechanism
retired as a deviation source at current corpus scale; the pose
channel's case rests on coverage, not on explaining existing rows.

**The one event — t=31121 (enclosure re-entry, dz=−3184): a 410-row
burst in 8 ticks, ~90% (10,75) WHIRLWIND FUNNEL rows** — full-motion
mismatches (x/y/z/pitch/yaw/rand/speed/applied_*) on ~4 funnel
entities plus ~11 wrong-record rows (model/action/life/max_life/mana
— mis-slotted spawns?). A whirlwind engages right at the re-entry and
the port's funnel column diverges wholesale. (10,75) is the (10,22)
head's funnel child; the tail-band creators around it are the trace's
under-transcribed models. DATED, POSITIONED LEAD — parked for player
triage (mc2l24 carries the late-game freakshow ruling; this is
mid-take t=31121).

**Bonus corpus corroboration of the portal-z law:** every enclosure
transit in the recording lands with dz ≈ −3200 (−12.4 tiles) — retail
measurably DROPS the player to the destination ground on warp. The
row-59 word12=0 law now has recording-side proof independent of the
decompile.

Incidental: pose data alone cleanly classifies the player-motion
event families — pad warps (>30-tile jumps), respawns (act3→0,
+~1700dz, back to castle), knock throws (~80u/tick decaying
displacement runs, the buffet law visible passively). Exactly the
signal set a tier-1 pose channel would formalize.

## METEOR TRAIL SOUND MACHINE-GUN — the dispatchers' SILENT-EMITTER
## gate was unported (player report 2026-08-07, FIXED, PLAYTEST OWED)

Player report: the port repeats the meteor's shoot/trail sound at
every step of the flight path until impact, clipping the audio engine;
retail (BOTH games) sounds once at fire + once at impact. Not a
regression — born with the sound wiring (git bisect found nothing,
correctly).

**Cause:** both retail sound dispatchers refuse any request whose
EMITTER wears flags byte[0] bit 0x80 — the no-damage/decorative bit —
before the request-slot table: remc1 `sub_55370` :64473
(`byte[+16] < 0 → return`), remc2 `PrepareEventSound_6E450`
Sound.cpp:6291-92. The meteor trail is built ENTIRELY of 0x80-stamped
entities (MC1: the per-tick (10,1) seeder + its ring children, all
`|= 0x80`; MC2: the per-tick (10,0) sparks, `|= 0x10080`), and every
fire's activation tick requests sound 3 (:28118/:28152 / EF-side
sub_30D50) — faithful emissions the port then PLAYED because
`Gen::snd()` had no gate: one sound-3 restart per flight tick =
the machine-gun + mode-1 restart clipping.

**Fix:** the gate applied at DRAIN time in `World::take_audio`
(retain: player-sourced OR emitter `flags & 0x80 == 0`), beside the
existing drain-time owner resolution — the sim-side `sounds` vec is
hashed and stays byte-stable (the standing "audio fixes go in
mgc-audio/drain, not snd()" law). Regression test
`silent_emitter_gate_drops_decorative_trail_sounds`. Workspace + all
7 suites green (no hashed state touched).

**Noted while in there (small open lead, not dug):** MC2's dispatcher
keys the channel on the emitter's OWN id (`id_0x1A_26`,
Sound.cpp:6299), not the owner — retail MC2 gives concurrent
same-owner emitters separate channels where our drain-time id24
resolution folds them onto one (restart/keep-running granularity).
MC1 keys on +24, which effect ctors owner-stamp — our resolution is
exact there. Audible-difference cases look rare (the 0x80 gate now
silences the bulk emitters); parked.

## THE POSE CHANNEL LANDED (2026-08-07): the player-motion column
## verified over the whole corpus — ~196k pairs, 99.3% bit-exact,
## two sim fixes and one positioned MC2 gate lead out of round one

The tier-1 ticks-replay design from the transit-clustering session
(§TRANSIT-CLUSTERING PROBE) is implemented and graded. `verify-deltas`
pins the human pose, so the player's own motion column was the one
lane the diff never verified. The POSE CHANNEL
(`crates/mgc-conform/src/pose_lane.rs`, docs/CONFORMANCE.md §"The
pose channel") shadow-steps the faithful mover beside every
fixture-grade pair: flight state seeded from the recorded closure at
N, input recovered from the recorded flight column, one
`flight::mc1_move`/`mc2_move` step against the imported world,
stepped pose diffed bit-exact against N+1. World lanes untouched;
fixture signatures cannot drift (`exec_pair` not involved).

**The decode key (both games, decompile-verified):** the whole flight
column lives in the recorded wizard/player block and most of it was
simply undecoded. New `RetailWizardMc1` lanes (Type_160): `dw_0`
move/fire byte @+0, filter deltas @+4/+6, `v_28` eff_pitch @+28.
New `RetailPlayerMc2` lanes (Type_str_164): move byte @+0, deltas
@+4/+6, knock `moveBoost` @+30 + direction @+32 (remc2's `yaw_0x1E_30`
name is stale), eff_pitch @+36, stick accumulators @+341/+343,
web-slow ladder @+332/+333, paralyze @+334/+336, nudge latch @+609,
water counter @+610.

**Input reconstruction is exact, not modeled:**
- The move byte is stamped by the consume loop and SURVIVES to the
  settled snapshot (unlike the memset 10-byte command). Phase is
  per-game and was corpus-measured: MC1 stamps post-pass (:49018-22
  runs after the entity pass — pair N→N+1 reads record N; mb@56's
  strafe bit moves the recorded strafe across 56→57), MC2 stamps in
  PlayerEvents (EF:38064) — read record N+1.
- The stick reaches the mover only through the IIR filter, and the
  accumulators are recorded at both ends: `recover_stick` inverts
  `(2·stick − acc)/4` per pair (truncation-aware; any solution is
  downstream-equivalent). Map-screen ticks self-classify (retail
  zeroes the command; the accumulators decay; recovery returns a
  centered stick) — no gate needed.
- Mid-pass knock arms reconstruct by un-decaying the N+1 channel
  (mc1hwl0 t=371: kmag 0→76 = 80 armed − 4).
- Terrain probes run on MEASURED terrain@N+1 (terraform writers sit
  below the carpet's slot; on mc1l0 every eff_pitch/z residue row sat
  on a live terraform window; the port-evolved planes are no
  substitute where port terraform itself diverges, e.g. the t=562-570
  castle build).

**Round-one grades (% of stepped pairs bit-exact):**

| take | offered | stepped | bit-exact | % |
|---|---|---|---|---|
| mc1l0 | 7,097 | 7,097 | 7,092 | 99.93 |
| mc1hwl0 | 49,765 | 49,121 | 49,059 | 99.87 |
| mc2l0 | 22,695 | 22,402 | 22,376 | 99.88 |
| mc2l4 | 17,818 | 17,523 | 17,517 | 99.97 |
| mc2l30 | 12,561 | 12,172 | 12,141 | 99.75 |
| mc2l3 | 22,798 | 22,371 | 21,706 | 97.04 |
| mc2l24 | 66,904 | 65,342 | 64,775 | 99.13 |

Gates (death/respawn, warp, accel/Speed-domain — importer lacks a
`speed_boost` seed, MC2 debuffs, stick-unrecoverable) hold 2-3% of
offered pairs; every gated class is counted in the report.

**FIXED toward retail (sim, corpus-proven, goldens re-pinned):**
1. **Flutter draw phase** — retail tests the +63 clock BEFORE the
   tick's bump (:55294 tests the settled value, no increment in the
   handler); the port tested post-increment, so EVERY draw landed one
   pair late (the whole 220-row rand lane on mc1l0 was adjacent-pair
   swaps; f63 191/192 at the t=57/58 exemplar). `flight.rs` now tests
   then increments; rand lane 220→0.
2. **Both-strafes-held resolves RIGHT** — retail's strafe bit tests
   are SEQUENTIAL (:55783-86; MC2 EF:60793-96 identical), so 0xC in
   the move byte steps +16; the port's match treated it as release
   and decayed. Fixed in BOTH movers + regression test
   `both_strafes_held_resolve_right_not_release`. mc1l0 exemplars
   t=3189/3305 under move byte 0xE/0x2E.

**Emulated in the harness, small sim lead parked:** `sub_46840`
short-circuits WHOLE when the move byte is exactly 48 (both fires, no
move — :55759), freezing a held strafe's decay while double-firing.
`flight::mc1_move` cannot see fire state (Mc1Input has no fire bits),
so the harness pre-feeds one decay quantum on dw==48 pairs (18 on
mc1l0). Wire the fire bits into Mc1Input when convenient; MC2's
sub_5F380 has NO such short-circuit.

**OPEN LEAD (positioned, the one real family): the MC2 commit gate
refuses water-skim moves retail allowed.** mc2l3's 3% residue is one
story: 566 `tgt_speed want 80 got 0` rows + ~650 x/y/z rows, first
exemplar t=1156-1172 — the player skims z≈1276 over deep water toward
a rising shore, our `moveTest_5D0A0` port (`mc2_flight_gate`,
mc2/cave.rs) refuses where retail passed, the deep-water wedge then
legitimately fires `sub_5DD50`'s 128-unit nudge + zero_speed. The
stuck law itself is verified cave-gated correctly (water-only
off-cave, :59854-81). Likely live-play visible as phantom shoves when
grazing terrain near water on Day levels. Dig target: the gate's
water-slide arm vs retail moveTest_5D0A0 (EF:59429).

**Corroborations and small families:**
- l24's dirty windows cluster at t≈7686-7757 and t≈30-31k —
  whirlwind-funnel grabs and the enclosure portal transits: the
  funnel full-motion override corroborates the parked t=31121 lead
  (§TRANSIT-CLUSTERING), now also visible in the player's OWN pose.
  mc2l30's t≈2972-2990 yaw burst is the same signature.
- Teleport-resolve `tgt_speed` zeroes (HW t=20088, 3 rows; l24 8
  rows) — the spell-arm speed-zero law, spell-domain, not gated yet.
- Residual z/eff_pitch rows (≤ a few dozen per take, |d| ≤ ~100) sit
  on mid-pass terraform fractions (castle carve, meteor craters) —
  neither terrain@N nor @N+1 captures intra-pass edits; irreducible
  capture noise at current closure.
- mc1hwl0 stick-unrecoverable 0 / l24 155 — heavy-combat stretches
  where something else wrote the accumulators mid-pair (suspect the
  paralyze/possession writers); un-dug.

**Instruments kept:** `--example flight_dump_mc1` / `flight_dump_mc2`
(the recorded flight column per tick — the microscope every finding
above came from), CSV `kind=pose` rows. Golden re-pin:
`flight_tier_golden_state_hashes` FAITHFUL A-C (flutter phase +
strafe law; ENHANCED holds). Workspace + all 7 suites green.

**Tier 2 (banked, design ready):** chained gap-free segments — seed
once, recover input per pair from the recorded accumulators, step
continuously, report first divergence per scalar; the same loop as
tier 1 minus the reseed. Becomes the stepping stone to the full
`--replay` verifier.

## THE REPLAY VERIFIER LANDED (2026-08-07): pure input replay —
## seed once, free-run on the recovered input stream, report (never
## correct) divergence; the MC1 ±1-tick cast caveat is RETIRED

Player-directed scope cut: no resync machinery — gaps are recording
artifacts (re-anchor a segment), divergence is the finding. New mode
`mgc-conform replay <take>` (crates/mgc-conform/src/replay.rs;
docs/CONFORMANCE.md §"The replay verifier"): world imported ONCE from
the first closure, the human's flight state chained and stepped by
the same integer laws as the app (`Simulation::step`'s faithful path:
dead/falling override, Accelerate expiry edge, knock drain, mover,
death fall + dead-camera turn, `World::tick(pose, cmd)`, then the
respawn/teleport/speed-zero/end-pose mailboxes). `--pose-only` =
tier 2 (flight chained, world re-imported per pair). Headline metric:
the BIT-EXACT HORIZON.

**dw_0 IS the cast ground truth (decompile + corpus, both games).**
Writer: MC1's poller sends press-edge OR held-while-reloading/charging
via `MakeControlCommand_188A0(6, 16|32)` (:20590-20632), the consume
loop stamps `T160->dw_0` post-pass (:49021) — record N's byte is the
byte tick N+1 acts on. Readers: bit 0x10 = LEFT hand (+940), 0x20 =
RIGHT (+944), dispatched EVERY set tick (:55825/:55830) — retail cast
dispatch is LEVEL-triggered through the manifestation reload ladder
(`f48 := f50` arm at :55893; launch at `f48==f50`), autorepeat = the
held level itself. No edge detection exists in retail; replaying the
byte as a per-tick level is exact by construction. Corpus: mc1l0
312/312 and mc1hwl0 2,056/2,056 single-shot casts at exactly
byte-record +2 (zero off-phase; the +2 is pool pass order); MC2
560/560 arms carry the byte on the same record, and the press-latch
law's extra edges are UI clicks the byte correctly omits — the byte
outranks the latch. RECORDING.md updated; `--input-delay` stays only
for the legacy raw-externals path.

**Fixed in this round (all three caught BY the chain):**
1. `RetailPlayerMc2.water_ctr` decoded u16 at +610 — retail's
   `n_0x262_610` is int8_t (remc2 global_types.h); the read polluted
   the value with the 0xE0 neighbor byte on every take. mgc-formats
   now reads the byte.
2. Measured-terrain install order: the importer's terrain-replay pass
   DOUBLE-APPLIES state-derived edits on already-measured planes.
   The replay driver installs measured planes AFTER the import (the
   pose channel's proven order); mc2l0 pose horizon 0 → 597 on the
   fix. (`exec_pair`'s install-before-import is unchanged — its world
   lanes absorb the same double-apply; candidate cleanup.)
3. MC2 importer gap: `g.player_knock` now seeds from the recorded
   `moveBoost` channel (the MC1 arm's :295 twin).

**Grades (2026-08-07 corpus). Pose-only horizon = boundaries
bit-exact from the anchor; world horizon = whole-world raw compare
(NO roster — known-open families are chain-breakers by design):**

| take | pose-only horizon | first pose break | world horizon | first world break |
|---|---|---|---|---|
| mc1l0 | 567 | t=568 z−3 (castle-build terraform window) | 1 | t=2 (10,39,41) statics z +32..35 re-snap |
| mc1hwl0 | 339; seg1 **14,053 ZERO-DIVERGENCE**; seg7 1,021 | t=340 z+21 | 1 | t=2 rival col: mana 0→100, target_yaw, speed |
| mc2l0 | 597 | t=598 spell-arm tgt_speed-zero (known, un-gated) | 64 | t=65 (10,12) missing + slot 418 life/xyz |
| mc2l3 | **1,156** | t=1157 x/y/z/yaw — THE WATER-GATE exemplar (t=1156-1172), exact hit | 4 | t=5 slot 124 z −128 |
| mc2l4 | 1,656 | t=1657 z−1/eff_pitch | 0 | t=1 slots 99/100/135 action 72→73 cohort |
| mc2l30 | 246 | t=247 z−10 | 14 | t=15 slot 30 life 3→1 |
| mc2l24 | **7,686** (~5.3 min) | t=7687 y−2 | 1 | t=2 slot 16 heading |

MC1's global LCG holds LOCKSTEP across entire takes even after
entity-set divergence (rng 0 mismatches on all MC1 runs — one
unconditional draw + per-entity private LCGs); MC2's
activity-dependent draw count breaks within ~35-2,600 ticks of the
first entity divergence, as expected.

**Positioned world-chain leads (NOT dug — categorize-and-ask):**
mc1l0 (10,39,41) z re-snap on the SECOND free tick (+32..35, port
lands on ×16 grid; survives the terrain-order fix — per-pair
verification structurally cannot see it, only tick 2+ of a chain
does); mc1hwl0 rival-column mana/aim at t=2 (corroborates the banked
hw-mana import-domain lead); mc2l4's t=1 action-72→73 cohort. The
pose chain's breaks all land on ALREADY-KNOWN families — the
recovery+mover chain itself is sound.

**Notes for the next round:** replay compares RAW; a roster-aware
replay tier (skip known families when counting the horizon) is the
obvious lever on MC2 world horizons. The app/driver step the mover
BEFORE `World::tick` (player probes last tick's terrain) where
retail's carpet moves mid-pass after the terraform writers — the
structural skew is now measurable here if it ever matters. MC1
respawn stays keyboard-±1 (SPACE, no latch). Runtime: the whole
7-take × 2-mode sweep ≈ 3 min wall.

**Follow-ups agreed with the player:** (a) the in-app PORT recorder
(`source:"port"`, `input:"exact"`, optional hash channel) + `--replay`
playback — one consumer plays both port demos and transcoded retail
takes; the hash channel dates any post-fix drift for free re-record
decisions. (b) For watchable retail replays the app should derive the
faithful-tier pose from the integer carpet, not the accumulated float
yaw (a quantization risk over long sessions the integer driver
sidesteps).

# THE IN-APP REPLAY + RECORDER LANDED (2026-08-07, session 13)

Both follow-ups above are LANDED, plus the lift that makes them one
implementation (RECORDING.md "Consumers" is the normative entry):

- **The lift:** input recovery moved to `mgc_formats::recover`
  (stick inversion, consumed knock, move/fire byte, equips/rebinds,
  the respawn SPACE lane + MC2 recentre witness as
  `Mc2RespawnWitness`, per-pair `recover_pair_mc1/mc2`, the
  capture-grade laws); chain seeding + the pose-lane compare moved
  to mgc-sim's conformance module (`mc1_state_from_retail`,
  `mc2_state_from_retail`, `integer_pose`, `pose_lanes_*`).
  `mgc-conform replay` now consumes the shared home — regraded
  IDENTICAL post-lift (mc1l0 pose-only horizon 567; mc2l3 pose-only
  1,156 breaking at the water-gate t=1157).
- **(b) integer pose:** `Simulation::step`'s faithful tier hands
  `World::tick` the INTEGER carpet pose verbatim (heading/pitch were
  re-quantized from the accumulated float yaw — a ±1-unit cumulative
  drift); the death fall integrates in integer space at the carpet
  (the replay driver's exact form) and respawn preserves the integer
  yaw. Enhanced keeps the float flyer. Goldens unmoved (the drift
  needs long sessions to bite — exactly the risk retired).
- **(a) `--replay` (source-agnostic) + `--record`:**
  crates/mgc-app/src/replay.rs. Retail arm = the verifier's chain
  through the app's own `Simulation::step` (`FlightInput` gained the
  replay-only `mc1_move_byte` exact-byte lane — the float axes
  cannot express both-bits-held states; MC2 dw_0 bit 0x80 arms the
  barrel roll). Port arm = header sim-closure pins (tier tags
  applied, foreign `snapshot_version` refused), `start_mgcs_b64`
  restore, hash channel asserted live. HUD: bit-exact/diverged-since
  counter + translucent recorded-pose GHOST billboard.
  `--replay-check` = the headless twin (exit 0 = zero divergence).
  `--record` writes `source:"port"` + `input:"exact"`
  (`mgcr::PortInput`) + hash via the new `mgcr::RecordingWriter`,
  start state ALWAYS embedded as `start_mgcs_b64` (pristine boot =
  the t=0 special case).
- **Certification:** app `--replay-check` vs `mgc-conform replay`
  world mode — IDENTICAL pose timelines on mc1l0 (first divergence
  t=563, 7,097 ticks, one segment) and mc2l3 (t=244, 22,798 ticks);
  the port loop pinned by
  `mgc-app replay::tests::port_record_replay_roundtrip` (record →
  reopen → replay: 200/200 hash-verified, end states bit-equal).

## The first playtest round (same session): jars + demolish

The player's first watched replays surfaced two failures; both dug
to root and fixed, plus what the digs exposed on the way.

**"Picked-up spells leave their jars behind" = a DRAW-FILTER gap.**
The grant itself worked: strict-retail pickup converts the jar IN
PLACE into the owned-spell TOKEN (`tick70 = spell*3`, phase 0), but
`live_poses`/`live_things` only hid the NATIVE encoding
(`>= MANIFEST_BASE`), so the token kept rendering as a ground jar.
Both draw layers now hide phase-0 class-12 under `strict_retail`
(retail never draws tokens; phases 1/2 = the visible world jars).
Sibling fix: the MC1 rival-death jar scatter now stamps the RETAIL
encoding (`spell*3 + 1`, a phase-1 world jar) in strict worlds — the
native `DROPPED_JAR = 3` aliases the heal TOKEN there.

**The dig exposed the strict pickup poll granting 5-8 ticks EARLY**
(mc1hwl0: port t=10 vs retail t=13 on jar 17 — hands then diverge
from t=9 and 15k hand-lane rows follow). Root: retail's poll
(sub_55A40 :64798-842, the plain summed-extents sub_11950 AABB over
the bucket[0] wizard chain) sees the carpet at its PREVIOUS-frame
position — jars sit low in the pool, the carpet's slot runs after
them. The port tested against the CURRENT tick's pose. Fix: the
world keeps `human_pose_prev` (hash-quiet, derived) and the strict
jar poll reads it; both importers seed `human_pose`(+prev) from the
recorded carpet so the first post-import tick is phase-correct.
Measured: mc1hwl0's first hand divergence moves t=9 → t=1268 (the
first three pickups land on retail's exact ticks; the 1268 residue
is downstream of the pose break at t=294). mc1l0 world-replay field
traffic drops 3.03M → 2.22M rows.

**"No input for castle destruction (Shift+L)" — demolish recovery
LANDED, both games** (decompile + corpus verified):
- MC1/HW: `dw_0 == 48` IS the command (`MakeControlCommand(6, 48)`,
  the only writer producing exactly 16|32; consumed by the :55760
  short-circuit that skips the whole mover INCLUDING both casts).
  Measured 18/18 on mc1l0's demolish presses, zero false positives
  over 7,098 records; `act_life = -1` lands on the next record each
  time. `recover_pair_mc1` now emits `demolish` on the byte and
  fires NEITHER hand on those ticks (the old recovery double-cast —
  bits 0x10|0x20 read as fire levels).
- MC2: the command rides `PlayerAction` 0x2A (EF:37991-96), never
  the move byte. Witness: own castle (`players[local].castle_ent`)
  at the END record with `life == -1 && action45 == 6`, corroborated
  by the held Shift+L scancodes (38 + 42|54) — verified at mc2l24
  t=41798. Idempotent re-fire while the castle parks at −1.
- Live-play sibling fix: the app now binds EITHER shift for
  Shift+L (retail :20467 accepts both; the corpus takes used RIGHT
  shift, which the app ignored).

**Recorder lead (banked):** `lastPressedKey` — retail's own keyboard
press latch, the byte the demolish/respawn dispatchers actually read
— sits at `pressed_keys_guest + 128` in ALL THREE builds; recording
`pressed_keys_len = 129` would retire the keyboard ±1 caveat for
both games (needs a re-record).

All 7 suites stayed green through every change (7,108 fixtures, 0
regressions); pose-only horizons unmoved (mc1l0 567, mc2l3 1,156);
`--replay-check` re-certified identical on mc1l0/mc2l3/mc1hwl0.

## ⭐ THE BEE ALTITUDE LAW — mc1l32-bee-height intake + FIX LANDED 2026-08-08 (player-queued: "a very significant deviation from retail")

**Take:** `recordings/mc1l32-bee-height.mgcr` (2026-08-08, window-gated
`CARPET_REC.EXE`, format 1 — no terrain channel): 3,958 pairs, 0 torn,
all fixture-grade, RNG (1,1) on every pair. Suite FROZEN AT INTAKE
(pre-fix): `conformance/mc1l32-bee-height.json`, 168 fixtures
(144 sampled conforming + 24 story exemplars), bundle
`conformance/fixtures/mc1l32-bee-height-fixtures.mgcr`.

**THE FINDING — port bees never climbed.** Pre-fix headline: (5,2) z
= 3,133 rows / 2,396 pairs, single-tick signature retail +24/tick
climbing vs port 0/−8 (diff 32, sign-flipped when retail bobs at its
hover). Retail's law is NOT a pitch and NOT in the shared chase: the
m2 CHASE wrapper `sub_1B3C0` (:22349-54) steps z DIRECTLY by
`sign(z − target.z) · row.v_14` (row 14 v_14 = −32) BEFORE `sub_1A120`
— unvalidated target read (f146==0 reads slot 0, dead targets still
steer), and the mover's alt clamp then fights it: net +24/tick
climbing in-band, net 0 above ground+v_10 (hard ceiling at
ground+1792), and an asymmetric 4-tick bob (+24/−40 legs) about the
victim's altitude. Cross-verified byte-identical in remc1hw:20906-12.
The mover itself is pitch-0 for creatures (all four :21224-88 legs) —
the port's `move_probe` pitch-0 is CORRECT; only the wrapper nudge was
missing. FIXED in `bee_chase` (mc1/mobs.rs). The same session landed
retail's ACQUISITION LUNGE ARM: every non-chase m2 handler
(sub_1B350 :22319 / sub_1B370 :22327-31 incl. sound 13 / sub_1B4C0
:22374-75) arms +26 = 1 on promotion to CHASE, so the FIRST chase tick
fires the 3x lunge (the port had only the ctor's slot%100 seed + the
post-sting re-arm). And the m3 SPAWN GUARD: retail guards multipart
spawns on 16 free slots for m0 (:44586) and m6 (:45028) ONLY — m3's
ctor sub_384B0 has no guard (straight to NewEvent; HW sibling agrees)
— the port's blanket guard is now m0/m6-only.

**Receipts.** Bee family 3,186 → 110 rows (z 3,133 → 57, −98%);
conforming pairs 1,433 → 1,647. Suite: t=951 exemplar FIXED+promoted
(regression guard), t=981/991/1054 drifted down to their (9,0)
co-atoms, 0 regressions; all 7 prior suites green (7,108 fixtures,
0 regressions). Pins: `bee_climbs_to_meet_an_elevated_victim` (climb +
band-top ceiling) and `bee_arms_the_lunge_the_tick_it_acquires_a_target`
(stale-77 overwrite + first-tick 3x), both revert-checked. L005
GOLDEN C..E + OBSERVABLE C..E re-pinned, attributed by toggling each
piece: the arm fires in the C ambush window (sound 13 is hash-visible
via the sounds vec), the z-nudge moves D/E, the m3 guard moves nothing
there. fmt clean, workspace green.

**Residuals on the take (open):**
- (5,2) z long tail: 57 rows, ±32 on scattered ticks — suspect the
  PLAYER-z read phase at altitude-crossing ticks (retail reads the
  pool wizard entity; the pin feeds ctx.pz at n1) + a few
  floor-snap/tile-edge rows (51/8/24 diffs). Not worth a lane until
  the (9,0) family is read.
- (5,3) SECOND-WORM family: 10,555 rows t=3028-3939 across 68 slots
  (4 heads + 64 segments, the end-of-take wall scene) — full-motion
  per-tick divergence, NOT the bee law (m3 chases via the shared
  handlers with no wrapper) and NOT the spawn guard (single-tick
  pairs init from retail state). Needs its own dig: suspect the
  segment-follow path (sub_19550 — the one creature mover that DOES
  feed pitch/+32 into sub_41EC0).
- (9,0) 3,595 rows (flags 4-vs-1028 = port sets 0x400, full motion) +
  (10,0) z 1,355 rows: same ±32 shape — likely knock-on of creature
  launch z (e.z + f84), re-measure after the (5,3) dig.
- (5,9) m9 life 1000-vs-600 + z lanes; (5,4) militia heading
  (160 rows, 3 slots); class-12 ecology churn (retail respawns
  statics, port doesn't — known l32 lane); (10,39/41) balloon
  z-resnap + flags 0x20000000 (known family); wizard hand equips
  (capture, input-recon ±1).

**Boulder re-bind LANDED same session** (two-bugs banked lead (a)):
state 0xF → `proj_generic_tick(false)` per the relocated table; the
throw ctor's pre-target CORRECTION (sub_1AE30 :22122-23 copies the
thrower's +146 + binds row [6] — the old "no pre-targeting ctor"
reading was wrong) makes retail boulders PITCH-home (row 6 v_2 = 0:
yaw never turns, :5247); m14 = acquire default-refuse (:64185); no
fire trail (the trail wrapper is state 3's sub_53070). DEVIATIONS
boulder entry rewritten; residual deliberate: the (3,0xFF)-vs-
thrower's-+66/+67 filter copy. Pin:
`troll_boulder_rides_the_generic_flight_pretargeted` (homing + refuse
legs, mutually non-vacuous geometry). No goldens moved.

**Two-bugs lead (c) SCOPED, banked deeper than memoed**: the rival
mid-game first castle (rivals.rs ~1950-70) not only stamps CastleInit
instantly — it parks the (3,2) at state 4 and pre-sets extents +
f136, bypassing the state-5/f59=0 build machine the player's
touchdown runs (leveler 43 → painter 44 → protection promote).
Retail rivals cast a real castle BALL through the same touchdown
chain. The minimal faithful form is letting the spawned castle run
its own state-5 build (delete the tick70=4 override + instant
stamp/extents/cap), but rival-side assumptions (instant usability,
banking cap, recast timers) need reading first; the full form is
rival ball emission. Water-yard drain is the only player-visible
consequence known.

**(5,3) SECOND-WORM family DUG same session — two causes, segment-follow EXACT.**
Agent-measured (hand-derived both sides from the port's own helpers,
bit-exact), zero segment-only divergent ticks across all 4 chains —
segments are 100% downstream of their heads:
- **CAUSE A (~92%, ~9.7k rows, CROSS-FAMILY): the terrain datum.**
  The port's excavation floor pins at height 196 (z 6272) in the
  x∈[3.7,14.1], y∈[195,230] corner while retail keeps digging to 189
  (6049 stable); every grounded/floored z lane rides it — (10,39)
  mana balls 7.7k rows (the "balloon z-resnap" bulk!), (10,0) fires
  974, (5,3) 225 via the alt-clamp floor, (5,9) 205, (9,1) 60. First
  at t=2036, ~1000 ticks before any m3 involvement. Candidate: the
  protection abort in dig_cell (features.rs ~1712, angle bit 0x80)
  believing cells building-protected where retail does not — SEPARATE
  dig, and THE TAKE IS FORMAT-1: attribution needs a **v2 re-record
  of MC1 l32 with the terrain channel** (top ask). No m3/creature
  change can move these rows.
- **CAUSE B (~8%): a hit costs the WHOLE handler tick.** All four
  shared retail class-5 handlers (idle :21368-75 / wander :21488-503
  / chase :21634-54 / pack :21741-69, villager variant :25048-63)
  abort BEFORE the mover on ANY hit — non-wizard attacker = bare
  return, wizard = retarget/promote then return. The port's
  centralized intake (mobs.rs ~2945-99) returns only for WIZARD
  attackers, so a creature damaged by another creature still moves —
  proven exactly at t=3284 (head slot 4: retail frozen, port a full
  turn+step) and t=3839 (the SEGMENT-CHAIN life-inheritance path
  latching a militia attacker). Fix audit in flight: every non-shared
  retail handler's prologue shape (m5/m9/m11/m15/m16/feeders) must be
  read before the unconditional return lands.
- **segment_follow faithfulness nits** (not load-bearing here; feed
  cause B's +40): missing `else { f40 = 0 }` (:21134-37) leaves a
  dead attacker latched; port-invented `f38 = src` (retail writes
  only +40, :21132); intake ordered before the move (retail after);
  orphan arm returns where retail falls through (:21112-13);
  combat.rs:275 sets f38 = f40 unconditionally where all four retail
  prologues set +38 only on the lethal branch.

**HIT-ABORT RESTRUCTURE — AUDITED, BANKED (implementation spec).**
The full per-state prologue audit (agent, 2026-08-08, every claim
cited + HW-corroborated) ruled the blanket "any hit returns from
creature_tick" WRONG. Retail's invariant: a hit skips the shared/
custom CORE (prologue-tail through sub_196E0) — wrapper PRE-WORK and
TRAILERS still run. The correct shape:
1. NO intake at all for states 30 (5,0) / 54 (9,0) / 66 (11,0) /
   72 (12,0) — retail handlers carry no damage prologue (:22775-78,
   :23591-623, :24317-84, :24835-992); the port currently runs its
   wizard-return intake for them, an infidelity in the OTHER
   direction. Keep (13/14/15,0) + (13/14,2|3) out too — untranscribed
   stubs (:4636-40).
2. m15 guard_chase (state 92, sub_201D0 :25826) is the exception:
   ONLY the lethal branch acts — any non-lethal hit (wizard or not)
   falls through into the FULL chase body (aim/range/bolt), no
   retarget, no return. HW byte-identical (:24383-86). Its lethal
   write is raw `+70 = 94` (:25828), bypassing the state helper.
3. m9 hidden (state 55) promotes on ANY attacker class — no class-3
   test (:23735-38; buried arm :24004-07): drop attacker_is_wizard
   for (9,1).
4. Everything else roles 0-3: return from the CORE, keeping alive —
   pre-prologue work (m2 chase +26/z-step :22343-54 ✓ already so;
   m6 chase speed pin :23146; m7 wander regen :23303-11 + chase
   countdown/sprite :23325-32; m8 chase speed + 0x8000 :23549-52;
   m9 bury countdown/hide-timer :23682-98) and post-handler trailers
   (m0 flyer_bob; m2 lunge-arm ✓ + exit speed ✓; m4 sub_1BC50/1BCE0
   + wanted flag :22689-717; m5 regen :22977-83; m6 speed pin
   :23116/:23276; m8 screech; m9 sub_1DCD0/1DD50 :23920-22/:24209-10
   /:24219-20; m15 enter/exit trailers :25766-67/:25862-63; m16
   house hunt :26032-58 — retail RUNS it on a non-wizard-hit tick).
   The port's hoisted m6 `f126 = 30` (mobs.rs ~3002) must move into
   the m6 arms to survive the abort.
5. Missing secondary arms to fold in: m6 chase wizard-hit `+26 = -10`
   (:23193); m15 wander same-owner retaliation veto (attacker +24 ==
   own +24, :25727); and neither (5,3) :23052 nor (12,3) :25264 does
   the pack-leader retarget the port's Inbox::Dead role-3 arm applies
   to every model.
Cause-B measured size on this take: ~0.8k rows (t=3284/t=3839
exemplars in the suite). This is a central-intake restructure across
every model — its own session.

## ⭐ mc1l32 FULL-LEVEL TAKE INTAKE 2026-08-08 (the player's "as brutal as levels get" end-to-end re-record)

**Take:** `recordings/mc1l32.mgcr` — 50,762 ticks, 50,673 pairs, 15 gap
events (=74 skipped ticks, player-confirmed unavoidable, mostly meteor
showers), 264 torn, 50,409 fixture-grade; window-gated build A;
**format 1 — NO terrain channel** (see the recorder fix below).
Suite: `conformance/mc1l32.json`, 384 fixtures (356 sampled conforming
+ curated exemplars; 12 stories carried from the bee-height suite by
exact signature). Raw: 3,555 conforming, 46,854 dirty, 1.9M
unexplained rows across 15,509 signatures. Supersedes nothing — the
bee-height suite stays (self-contained bundle).

**RECORDER FIX (tools/mc_dosbox_recorder.py):** pin_terrain ran once
at attach, BEFORE wait_until_live; attaching while a level GENERATES
(shading all-zero until final bake — the validator doubles as the
readiness gate) silently degraded the whole take to format 1. Both
2026-08-08 l32 takes lost their terrain channel this way. Now retried
after go-live. Existing takes are unaffected (nothing can add terrain
retroactively); the terrain-datum attribution still wants ANY l32
recording with terrain — need not be a full/better run.

**STORIES FOUND AT INTAKE:**
- **The (11,9) SPAWN TRIGGER never fires (exemplar t=39412).** Retail:
  a class-11 model-9 trigger (17 tiles from site; player just arrived
  at 21 tiles) fires TWO full m3 worm chains in one tick — 25+ slots,
  free stack 395 (NOT pool pressure). Port: nothing, and the pair's
  sole diff is the missing (5,3) set (no rand rows — the refusal costs
  no draws). The whole late-take (5,3) missing block (510 rows,
  t=39412+) is this one unfired trigger. Dig lane: the port's class-11
  model-9 handler (the mc1:37 trigger session certified sounds +
  x-markers; model 9 = creature spawn is evidently not among them).
- **THE STUCK EXPLOSION (player-reported; exemplars t=39509/39511).**
  Retail spawns a (10,0) into slot 2 at t=39508 that WEDGES: flags
  0x30086, life frozen 6/8, chase 182, floating at z=6139 — never
  ticks down, never dies, alive 11,000+ ticks to take end. The PORT
  KILLS IT within one tick (stamps 0x400) — every pair, 11,215 flags
  rows. The port does not reproduce this retail bug; it silently
  fixes it. **RULED same day, re-confirmed 2026-08-08: do NOT
  reproduce** — the port's one-tick kill is the kept behavior
  (DEVIATIONS.md "the stuck explosion", roster rule
  `mc1l32-stuck-explosion-wedge`, exemplars expected-fail by design;
  wedge-mechanism dig demoted to curiosity — NB the meteor-trail
  session's "dispatchers refuse 0x80 emitters" rhyme, and the entity
  carries 0x80). Birth pair also shows the terrain-datum floor (port
  z 6272).
- **METEOR-STORM CHURN (the take's dominant entity-set family).**
  (10,0) 1,120 missing / 2,179 extra + (9,0) 1,341/1,405 from t≈414
  on, retail >16-LCG-draw bursts on 26 pairs — the ambient meteor
  weather diverging, same class as mc1hwl0's storms. Global rng still
  (1,1) on 50,382/50,673 pairs.
- **CASTLE (3,2)/(3,3) z (exemplars t=5502-5981)** — the castle-latch
  session's banked castle-z+64 lead, now with suite exemplars.
- **(9,9) missing block** (881 rows, t=23132+, exemplar) — m9-bolt
  family, undug.
- **The terrain-datum class dominates z** (1.3M rows / 46,670 pairs)
  level-wide across the full take — unattributable at format 1 (see
  the bee-session cause-A entry).

Suites: mc1l32 384 fixtures baseline all-as-expected; all other
suites untouched.

## 🏆 mc1l32 SETTLE SESSION 2026-08-09 — the (11,9) spawn trigger and the (9,9) block, both to root cause

**Player directive:** stuck explosion stays ruled closed (re-confirmed);
settle the other two stories for good.

**1. The (11,9) SPAWN TRIGGER (exemplar t=39412) — IMPORTER BUG, FIXED.**
The port's trigger probed and MATCHED (overlap true, phase f63=104
aligned, pose inside the 768+119 AABB with margin) and fired — but
`import_ent` (conformance.rs) translated `id24` through the human-slot
map for EVERY class, and for class-11 triggers id24 is the DISPOSITION
id, not a slot reference. l32's breadcrumb disposition is 14 == this
take's human slot 14, so the armed trigger imported with dis
`PLAYER_TARGET` (65535) and the fire resolved 134 load-sentinel
(dis −1) rows, all consumed at load: a silent no-op. Self-concealing:
the obs projection `untr()` mapped 65535 back to 14, so no id diff ever
surfaced, and the differ's flags row was absent because BOTH arms kill
the trigger (0x400) on fire. Fix: class-11 id24 imports untranslated.
Post-fix the pair spawns the full dis-14 set — 30×(5,3) worm chains
(17 slots each) + 21×(5,4) + the next (11,9) breadcrumb (dis 15,
slot 622) = 532 slots, exactly retail's free-stack delta (893→361).
The old "(5,3) 510-row missing block t=39412+" is GONE; residue at the
fire pair = free-list slot-order desync (361+361 rows, roster-tagged)
+ pose-phase, plus the pre-existing 44 terrain-z field rows.
- **The intake's story was wrong twice**: not "2 worm chains" (30),
  and not "the trigger never fires" (it fired into a mangled dis).
- **NATIVE GAMEPLAY WAS NEVER AFFECTED** — spawn_postinit sets id24
  from the THING record; `tr()` exists only in the conformance import.
  No patches toggle needed, nothing player-visible.
- **LEAD (unverified): MC2 import** keys id24 off `owner28` — check
  whether an MC2 class-11 switch whose dis == the recorded carpet slot
  can take the same collision. All MC2 suites green today, no exemplar.
- **Trap for future digs:** in per-pair verify, a one-shot's
  fire_disposition CONSUMES `table` rows in the shared World (and the
  pose-alt second pass re-fires into the already-consumed table), so
  "fires but spawns nothing" can also mean "someone consumed the dis
  earlier in THIS RUN" — rule that out with an env-gated fire log
  before blaming the bake.

**2. The (9,9) BLOCK (exemplar t=23132; 881 missing + 13k field rows,
t=21041..34571) — DECOMPOSED: two segment micro-laws FIXED, geometry
residue RULED terrain-family.**
(9,9) = the zigzag-lightning one-frame beam SEGMENTS (sprite 216), born
8·steps+1 per beam by the m5 multishot volleys (owner = (5,5) creatures
sieging the player near the dug castle; bursts of 282/142/102 rows in
single pairs). No rng rows, no wrong-branch spawns — the volleys fire
in both arms. Three sub-families:
- **max_life ctor gap (2,940 rows, FIXED):** retail segments carry
  max_life 0/−1 in lockstep with the slot-order act_life; the port
  left the NewEvent default 300. Ctor now mirrors the value.
- **pre/post-decrement kill (3,764 rows, FIXED):** retail's state-14
  arm decrements THEN tests (dying segments read −2); the port killed
  on the pre-decrement −1. Death frames identical either way — the
  recording pins the residual value. (The pre/post-decrement error
  class strikes again; remc1's class-9 table is truncated at state 14,
  so the corpus is the only witness.)
  A/B to t=24000: life rows 1662→43, max_life 1171→63, every other
  row class byte-identical.
- **Geometry displacement (RULED, stays open-frozen):** the
  missing/extra bursts and x/y/z rows all sit in tiles x16-31/y176-223
  where retail terrain is dug (retail z 6107-6270 vs pristine 6272,
  mean |dz| 175). Shooter z, launch z, pitch, and the beam's
  terrain-stop all shift on pristine planes → sprays land elsewhere.
  This is the terrain-datum family (bee-session cause A), frozen
  pending any terrain-channel l32 take. NOT a port bug; no roster rule
  (deliberately visible, like the rest of the z class).

**Verification:** 369 mgc-sim tests + goldens green; all 9 frozen
suites green (only the t=39412 exemplar drifted, consciously
re-pinned with the settled note; t=23132 note updated, no drift).
Full-take headline moved: the (5,3)/(5,4) unexplained missing block
0'd; (9,9) life/max_life field families −97%/−95%.

## 🚀 mc1l0 1:1-REPLAY CAMPAIGN OPENED 2026-08-09 — ball vertical law + possess-lob life; horizon 1 → 62, clean boundaries 1 → 196

**Player directive (standing):** drive ONE lightweight level's pure
replay to 1:1, family by family — families generalize; l1 gets
recorded when l0 is perfect. Method settled: fix families in the
PER-PAIR lane, certify horizons in `replay --pose-only` / full
`replay`, never debug through compounding replays.

**Baseline (l0, terrain-measured take, 7,097 pairs):** per-pair 99.93%
pose-bit-exact (6 imperfect ticks); pose-only chained flight bit-exact
567 boundaries, first break = the pose.z t=567 exemplar; FULL replay
horizon was 1 boundary — first divergence t=2.

**1. BALL VERTICAL LAW (the t=2 family) — FIXED, both games.** The
replay's first wall was the authored (10,39) mana balls: the port ran
gravity only airborne-or-launched, clamped at `z <= ground`, and
grounded only inside the clamp. Retail (sub_27030 :29532-64 and the
MC2 twin EF:26188-26265, VERBATIM the same): gravity integrates EVERY
moving tick; clamp+rebound (−impact/4, zeroed ≤16) fires only
STRICTLY below ground; grounded contact (merge/roll/friction) is
post-clamp `z == ground`. A ball landing EXACTLY on ground keeps its
fall lift one more tick — l0's cohort all fell 128-multiples onto
flat ground, so the port flipped them one tick early IN LOCKSTEP
(the t=2 z+32 cohort). PER-PAIR VERIFY CAN NEVER SEE THIS CLASS: the
import restores retail's +46 each pair and the flip tick's
observables coincide — only free-running evolution (replay, goldens)
exposes it. Horizon 1 → 62 on this change alone; all 9 suites green
(observables untouched); goldens re-pinned WITH attribution
(mc2_cave 2, flight-tier 6, L005 full+OBSERVABLE — the observable
moves are REAL one-tick bounce-phase shifts, corpus-certified by the
horizon jump).
- **Bonus fidelity:** the resting awake ball's hidden +46 now cycles
  0 → −16 → 0 like retail's (the old at-rest gate froze it), and the
  climb-into-terrain rebound uses retail's formula.

**2. POSSESS-LOB LIFE (pair-63 family) — FIXED.** spawn_spell_lob
gave every payload the fireball's life 21; retail's possess ctor
sub_39A90 (:45900-16) is the family's ONE short fuse: 4096/speed =
10 (corpus: retail lob 9/10 vs port 20/21, mc1l0 pair 63). The
doubled range overshot close-in possess targets — the likeliest root
of the player's "possessed tent missed in replay". l32's t=23132
exemplar dropped its (9,1) life atoms (promoted+noted); clean replay
boundaries 62 → 196.

**NEXT WALLS (in order, exemplars pinned):**
- **CAST PHASE (t=63, the current horizon wall):** in free replay the
  port casts ONE TICK EARLY (port lob in obs@63, retail's first at
  obs@64) while per-pair the same pair casts in phase — the cast
  gate's hidden charge state runs a tick fast in evolution. Related:
  per-pair t=49 `wizard0.hand_right` retail Some(3) vs port None (the
  ARM side lags; 5 hand rows across the take). This family is the
  player's "spells fired in the wrong direction" original sin lane.
- Pose z t=567 (want 1056 got 1053, 5 rows max |d| 25) + eff_pitch
  wrap t=3879 (2047 vs 0) — the 6 imperfect mover ticks.
- Entity-set churn after the cast wall (sets/fields traffic) —
  re-triage AFTER the cast phase lands; most of it is downstream.
- Per-pair unexplained residue for reference: 8,171 field rows, top
  families (10,39):heading 1,280 (ball aim/roll lane), player.mana +
  (3,0):mana 737 each, (9,0)/(9,1) projectile fields (cast-adjacent).

**Verification state:** 19 test binaries green (goldens re-pinned:
mc2_cave.rs, sim_state_hash.rs, state_hash.rs — every re-pin
annotated with its window attribution), all 9 suites green, fmt
clean.

## 🏆 THE CAST-PHASE LAW 2026-08-09 — MC1 casts ARM the token, the token FIRES at arm+1; horizon 62 → 413

**The l0 campaign's cast wall, dug to the root and landed.** Retail
MC1 has NO spawn at the cast command site — for all 24 spells:

**1. THE LAW (decompile + corpus, both readers' reports verbatim):**
- **Command site** = `sub_46B00_46E40` (:55851-919), called per hand
  from the carpet mover's tail (:55825-34) INSIDE the class-3 carpet
  dispatch (`sub_45C90` = `str_254ADC[0]`) at the carpet's pool slot.
  It only: gates (SILENTLY on mana short — :55873/:55890/:55908; the
  ONE audible refusal is castle-16's armed-token buzz :55903-06),
  reloads the burst (`+48 = +50`, :55893, unconditional — re-click =
  re-arm, no cadence gate), restamps the wizard's HAND BITS (+16 &
  0x300: both cleared, 0x100 left / 0x200 right, :55886-95), breaks
  the cloak (~0x20, :55896). NO debit, NO spawn.
- **Token tick** = one function per spell (class-12 dispatch
  `str_2563D8`, model = 3×spell), shared skeleton (:65203-63 =
  possess `sub_56510`): dry (+48 ≤ 0) = NOTHING (no decrement);
  gate `sub_55DD0` (:64910-32: alive, castle-store `+132`, first-tick
  affordability; failure = buzz 29 + burst abort); FULL (+48 == +50)
  = FIRE the emission + the `sub_55E80` debit (wizard `+132` =
  −(token `+136`) overwrite / deepen-if-negative — remc1 ships it
  `//fix`-commented, the .bak and remc2's live twin prove it);
  MID = zero a positive `+132`; decrement LAST (:65260).
- **Walk order IS the phase**: tokens sit below the carpet in every
  recorded pool (l0: 28/139/305 vs carpet 630; l32: 9 vs 14; HW: 472
  with 57 actives above) → an arm can never fire the same frame, and
  a token's mana write applies the SAME frame (the wizard's
  apply-then-recompute `:55385/:55409-17` runs after the token
  slots). Corpus: **257/257 l0 + 371/371 l32 arms spawn at arm+1**;
  the l0 t=63/64 trace pins arm (f48 0→3, mana untouched) then
  fire+debit together (spawn record + 1000→950 + f132 already
  re-armed +100).
- **Muzzle** (`sub_55EF0` :64963-65026): from the WIZARD's OWN pose
  at token time = the PREVIOUS frame's settled carpet (the
  pose-phase-tagged ctor diffs, now exact), offset 256 units at
  yaw∓512 by the hand bits, terrain-guarded, z += +84. **Rivals
  never set hand bits** (:19111 clears) — rival shots fire centered.
- **Projectile stamps**: `+140` = token `+140` = **cost/period**
  (:48005 — fireball 40, possess 16, castle ball = LADDER/101:
  corpus 9/49/99/198; feeds the Rebound deflection economics >> 2);
  `+30/+32` only — **`+34/+36` stay ctor-default** (corpus
  target_yaw 0 on every fresh m0/m1/m10).

**2. THE PORT RESTRUCTURE (engine/world.rs):**
- `mc1_wizard_pass` (mana step + cast commands + demolish word) runs
  at the walk hook `mc1_carpet_slot` (conformance import; the MC2
  precedent pattern) / post-walk natively. `mc1_cast_command` =
  arm-only for the launcher set {0,3,6,7,8,9,10,11,13,16,17,18,19,
  20,22}; `manifestation_tick` fires at full + `mc1_token_gate`
  (buzz 29) + debit + suppression + post-fire decrement; the strict
  class12 phase-0 arm now runs the LIVE machine for human launcher
  tokens (the old "tokens rest inert" harness emulations retired —
  the importer's mana_delta clamp narrowed to the still-inert set).
- New World fields: `mc1_carpet_slot`, `mc1_hand_bits`,
  `mc1_cast_pose` (prev-frame pose echo) — hash/snapshot-quiet like
  the mc2 carpet pair (a native save inside a live burst reloads
  with a center muzzle; ledgered).
- strict fire semantics: recorded dw_0 bits are the CONSUMED command
  word (input layer pre-edged, +60 law :20601-34) → each bit = one
  command; native keeps level+edge (the hold spells' legacy arms ARE
  the input-layer emulation).
- verify.rs now feeds recovered equips + demolish (the 5 hand rows
  were a harness gap — all gone, t=49 + 4 hand_left rebinds).

**3. TWO SCAN-GEOMETRY LAWS (the impact walls behind the cast wall):**
- `sub_11AC0` (possess victim scan): center = NEAREST tile
  (`(pos+128)>>8`) and the neighborhood = the SEARCH.DAT RING
  iterator (`sub_11410` rings 0..(f80+255)>>8 — 2×2-anchored shells,
  ring 1 spans −1..2) — `possess_victim_at` now uses `ring_cells` +
  rounded center (l0 t=69/t=78 impacts: big-extent tents overlap
  from outside a square window).
- `sub_120B0` / MC2 twin EF:3750 (area mail broadcast): SQUARE
  window but ROUNDED center — `area_write` fixed (l0 t=91 tent
  claim; mc2l0 t=7257 fixture went conforming on the same change;
  MC2 cave/slice goldens re-pinned with attribution).
- The detonating lob parks AT the victim's aim point (x/y + z+f78
  bracket → the −7296 record) and the HIT tick skips the life
  decrement; fresh (10,12) flashes carry flags bit 1 (corpus 0x5).
- ⚠ AUDIT LEAD: other truncated scan centers may lurk (e.g. the m17
  reconstruction-bridge house scan still truncates) — the class is
  now named; fix on corpus evidence.

**RESULTS (mc1l0):** full replay horizon **62 → 413 boundaries**
(t=1..414 bit-exact; clean total 196 → 413; entity-set first 63 →
569); per-pair first divergent pair **49 → 413**; unexplained field
rows **8,171 → 4,385**; the (10,39):heading 1,280-row and player.mana
737-row families are GONE; per-pair fixture promotions: mc1l32 ×9,
bee-height ×4, mc1l0, mc1hwl0, mc2l24 (castle-ball f140), mc2l0
(area rounding). Pose-only 567 unchanged (its wall is the
terraform-window family). **NEXT WALL t=414: (5,3) creatures in
state 120 — the multipart segment family** (slots 62/63/64, x/y/z/
heading/pitch drift).

**DEFERRED (positioned, with the full decompile map above):**
- Token-phase for the hold/channel/toggle set {2,15,21,23} + heal +
  {4,5,12,14}: their certified command-site machines kept verbatim
  (l0 never casts them). Retail truth when dug: full +136 debit per
  token fire (firehose 600/burst-tick, stream 1000), silent
  command gates, effects from the token.
- The charge machinery (+61/+62, release-cast, HUD charge bar) is
  DEAD CODE in retail MC1 — a7 = 0 in all 24 ctors.
- Invisibility arm nuance: :55896 clears the cloak bit on EVERY arm
  including spell 12's own; the live invis token re-sets it — the
  port's break-cloak (kills the invis burst) may over-break; dig
  with a corpus exemplar.
- Rival cast phase (rivals arm their own tokens in retail; the port
  mints + emits directly) — own lane.
- The wizard-pass hook runs before the post-walk damage intake
  (retail: intake INSIDE sub_45C90 before the mover) — invisible on
  l0 (nothing above slot 630); revisit if an HW window pins it.

**Verification: 697 workspace tests green** (L005 D/E state +
observable, mc2_cave 2nd-4th, mc2_slice E re-pinned, all with
attribution; windows A-C byte-identical throughout), all fixture
suites green post-promotion, clippy + fmt clean.

## 🏆 THE AWAKE-PASS POSE PHASE — mc1l0 wall 1 (t=414 worm chain) FIXED; horizon 413 → 561

**The (5,3) state-120 "multipart segment family" at t=414 was never
a segment law at all.** Retail slots 62-73 (worm head 61 + chain)
sat ASLEEP through t=414 (f58=0, f63&3≠0 → sub_19550's collapse
branch idles); the port ran the AWAKE branch (fresh yaw/pitch +
polar re-place — pitch sign flipped vs the stale 1932-1995 values
because it was freshly computed, not because the formula differs;
`segment_follow` itself is FAITHFUL). The port woke the chain one
tick early.

**THE LAW.** Retail's awake maintenance (MC1 sub_54F00 :64266 →
sub_54F80; MC2 twin sub_68BF0/sub_68C70 remc2 :55469) is a PRE-pass
whose proximity gate reads the local player's POOL entity (:64352-53
index → +72). Pre-walk, that entity still holds the PREV frame's
carpet — the pooled walk hook (sub_45C90 / AddPlayer03) hasn't run.
The port fed the pass THIS tick's integrated pose, so a wizard
crossing the 24-tile gate (dist² < 0x2400000) mid-tick woke the
bucket one tick early. mc1l0 t=414: pose@413 dist² 38,125,256 (24.06
tiles, OUT) vs pose@414 37,187,713 (IN) — retail arms f58=16 at
t=415 with pose@414; the port armed at 414. Corpus-pinned by the
dump-state ladder (head f58 0→0→16 over t=413..415, chain 18 at 415).

**Fix (world.rs, the AwakeVerb seam):** the wake pass gets a ctx
whose px/py = the `human_pose_prev` echo — which IS the pool-entity
value in every lane: the conformance import seeds it to the carpet's
recorded pose@N (the carpet-slot record itself is OUT-OF-POOL in
import lanes — zeroed, unreadable; a first attempt reading
`ent[carpet_slot]` collapsed the replay horizon to 66 on the stale
record). Unseeded first tick falls back to the incoming pose (the
pool wizard would hold the placement).

**Receipts:**
- mc1l0 FULL replay horizon 413 → **561** boundaries (t=1..562);
  per-pair first divergent pair 413 → **561**; both lanes agree on
  the new wall. (5,3) rows 1,263 → 350 (−72%); total field rows
  8,848 → 7,865; unexplained-row count EXACTLY unchanged — the fix
  killed precisely the pose-phase-tagged family, zero collateral.
- **5 open exemplars → conforming across 3 suites** (promoted):
  mc1l0 t=683 + t=1613, mc2l4 t=621, mc2l24 t=616 + t=1206 — the
  MC2 twin generalized for free.
- All fixture suites green, 0 regressions, 0 drift. 697 workspace
  tests green (MGC_REQUIRE_GOLDENS=1), clippy + fmt clean.
- Goldens re-pinned WITH toggle attribution (old sample restored →
  old hashes byte-exact): mc2_slice GOLDEN B-E (goat/flyer wake
  bookkeeping phase; **OBSERVABLE untouched — visible behavior
  identical**), flight-tier FAITHFUL C only (coast crosses a ball's
  gate; enhanced track never does).

**Leads opened:**
- The cave-drip probe (EF:40468) reads `player.*` = this tick's pose
  where retail's pre-walk pass would see the pool entity — same
  class, MC2 cave lane, weak signal (only matters when the 20×20
  window shifts a tile). Fix on corpus evidence.
- The mid-walk ctx consumers still ride this tick's pose; retail
  walkers below the carpet slot read the pool entity = prev pose.
  This is the bee-session "(5,2) player-z read phase" suspicion —
  now with a named mechanism. Own lane, blast radius large.
- Retail sub_54F80 stamps +48 = Distance(player) on every arm; the
  port never writes f48. Not in the diff projection; port f48
  readers unknown — audit before caring.

**NEXT WALL t=562 (both lanes agree): slot 486 castle site** —
retail builds at (114,96) z=797, port at (115,97) z=736, and
wizard0.castle/player.castle bind 486 where retail still has 0 (a
site/timing pair). Pose channel unchanged: z t=567 ×5 rows
(terraform-window family), eff_pitch t=3879 (2047 vs 0).

## 🏆 THE CASTLE COMMIT LAWS — mc1l0 wall 2 (t=562 slot-486 site) FIXED; per-pair 561 → 564

**The castle-site pair dug to the root: five laws, all corpus- and
decompile-pinned.**

**1. THE SITE LAW** (ctor sub_37920 :44244-55; MC2 twin sub_4AA40
EF:33383-88 agrees): the snap is **TRUNCATION** (`HIBYTE(x)` /
`>>= 8`) + the odd-parity x+1 — NOT rounding — and the MC1 link/site
z = `sub_11F50` at the **RAW landing point BEFORE the snap** (MC2's
perimeter-MIN site z unchanged). The port rounded (+128) and sampled
the snapped corner → (115,97) z=736 where retail builds (114,96)
z=797. The ctor also writes the +150 site echo (dest) — now ported.

**2. THE BIND LAW**: wizard +50 is written ONLY by the level-up arm
(sub_47960 :56484, with the +416 level echo) and cleared by the
level-down-to-0/removal path (:56534); the rival direct mint
(:19206) binds at spawn; the landing ctor copies id24 alone. The
obs `castle_of` scan now requires **f26 > 0** — the established
level ⇔ the bound field on the human lane (retail t=562: flag live,
+50 still 0; the bind arrives WITH the t=563 commit). The rival
mint-tick nuance is the rival-cast-phase lane's.

**3. THE COMMIT TICK** (sub_46F10 case 0): the first-commit latch —
flags |= 2 + the one-time type86 team stamp (+= wizard +48; port
keeps the ctor row, team art is the renderer's pose.team lane) —
and **NO ground z-refresh in the action cases**: the refresh
belongs to the established tick + pure waits (:56013 + 1/4/6), so
the ctor's raw-point z survives the commit tick (797 held while
the corner reads 864).

**4. THE LADDER ORDER** (sub_47DD0): the every-tick token stamp
runs from the WIZARD's walk slot — above the castle — so the
commit tick's stamp reads the POST-level f26 (t=563: 10000/99, not
the pre-commit 1000/9). Port stamp moved AFTER `castle_tick`,
gated established (+!0x400), keyed: human token = `owned[16]` (the
mint registry natively, the recorded wizext+724 slot under import —
the old f144-keyed scan silently missed the importer's
PLAYER_TARGET tag on book tokens); rival tokens = f144 == castle
id24 under strict (the importer's f42 join).

**5. THE BUILD WORKERS**: the m41/m42 ctors write **life 0**
(:47557/:47579 — the machines run on the +26 counter, never life);
the castle link is carried in **+42** at all three spawn sites
(:56484-91, sub_47020/sub_47080 :56100-133) — an unmodeled field,
so the port workers now re-derive their castle BY SITE (unique per
the 8-tile spacing law) and leave f146 = 0 like the recordings.
The painter body gained retail's castle-+50 shake-suspend
(:30520-21), and the live painter's fill goal table (:30637-41)
has **NO 3x arm** — every 0xF.. cell steps to 4*(lo-1)+target (the
+12/+16 fork belongs to the INIT stamp :29877-95; sharing it
mis-heighted tower-wall cells one sub-step per tick).

**RECEIPTS:**
- Per-pair first divergent pair **561 → 564**; the landing, commit
  and first-work pairs (561-563) fully conforming; unexplained
  field rows 4,385 → 4,282.
- Full replay horizon 561 → **562 boundaries**; channel firsts:
  fields 562 → 565, entity-set 569 → 568 (post-fork cascade noise —
  the world forks at the pose wall below), pose 563 unchanged.
- **2 open fixtures FIXED + promoted**: mc1l0 t=1208 AND mc1hwl0
  t=89, both `missing:(10,41)` — the ground leveler now survives
  to its tick (ctor life + link laws).
- Terrain probe: the port's paint sequence is **BIT-EXACT** against
  the measured terrain channel at every boundary t=562..565.
- All fixture suites green, 0 regressions; workspace tests green;
  clippy + fmt clean. L005 GOLDEN A-E re-pinned with attribution —
  **OBSERVABLE holds at every leg** (bookkeeping-layout only; the
  authored rival castles run these machines from window A). The
  castle-latch corridor-park synthetic pins re-derived under the
  faithful snap: patched arm (15,233) (still carpet-side), retail
  arm (16,232) ON the wall column — the recorded cheese's own
  character; the recorded-cast pins were already truncation-exact
  and did not move.

**NEXT WALLS:**
1. **✅ LANDED — see §THE MID-WALK RESTRUCTURE LANDS (end of file).**
   **REPLAY wall = pose.z t=563: THE MOVER GROUND-SAMPLE PHASE,
   root-caused.** The driver steps the human flight BEFORE
   `World::tick` (sampling the PREV tick's terrain) where retail's
   carpet mover, at the walk slot, samples AFTER the same-tick
   painter step — the carpet lags each paint step under it by one
   tick (retail 904 vs port 898 over the rising tower). This IS the
   terraform-window family (the pose-only z t=567 ×5 rows). The fix
   is the named mid-walk restructure (step the flight at the carpet
   slot; prev-pose ctx for the walkers below) — own lane, blast
   radius large: app + replay + pose lanes share `Simulation::step`.
2. **Pair 564→565: the castle established-tick ball collection** —
   retail stamps nearby (10,39) mana balls sclass/smodel = 10/39,
   resets target_yaw, speeds 42..48 (port: 255/255, stale yaw, 16),
   plus (10,0) z/rand rows — sub_46DB0's every-other-tick block
   (sub_47130 ejector / sub_47400) + the absorption loop. Fresh
   family, undug.
3. Leads: the painter's counter-2/-1 angle-bit rituals
   (0x80 ↔ 8 over the rectangle, :30556-85) unported —
   protection-channel only, obs-invisible today; the (10,43)
   upgrade token still rides f146 (retail resolves via wizard +50)
   — revisit on an upgrade-window exemplar; wizard +48 team sprite
   stamp stays presentation-side.

## 🏆 THE MC1L5 THREE-DIG SESSION 2026-08-10 — poverty gate, undead conversion, wall-of-fire damage law

**Take intake:** `recordings/mc1l5.mgcr` (23,680 ticks, 100% clean
decode, terrain channel 898 deltas / 145,078 cell edits). Suite
extracted + auto-triaged + frozen AT intake: `conformance/mc1l5.json`,
167 fixtures (143 conforming sample, 22 open, 2 no-rows), bundle
`conformance/fixtures/mc1l5-fixtures.mgcr`. Full verify TSV at repo
root `mc1l5.tsv` (235,904 rows raw). Pose channel 99.9% bit-exact
(23,655/23,675). ⚠ manifest+bundle+TSV uncommitted — player git.

**1. RIVAL CASTLE-REBUILD POVERTY GATE = the live cost stamp.**
Retail want gate (sub_13F00 :18359, every tick, castle-less):
`manifest16 +136 <= wizard mana_max` (sub_15E90 :19375); commit adds
`cooldown[16]==0 && manifest.f48==0 && CURRENT mana >= stamp`
(sub_15A00 case 0x10 :19332). The stamp is a LIVE cache: ctor 1000/9
(sub_3BF70 :47996), CAP[lvl] at castle build/level-up (sub_47960
:56481 → sub_47C60), **CAP[0] = 5000 at total teardown** (sub_47A70
:56527-28 stamps AFTER the final decrement — the folklore "<5000 no
rebuild"). Corroboration: cast-phase corpus token +140 = 49 =
5000/101; **mc1l5 take: Vodor castle-less at mana_max 1,768-3,796 for
t≈15,700-17,600 (no rebuild), rebuilds at t=17643 the moment
mana_max crosses 5,322**. The port had NO poverty gate (static ctor
1000 in both want + ready). LANDED: death stamp world-side in the
castle dispatch arm (castle died inside castle_tick → stamp CAP[0]
via `castle_owner_token`: human registry / native rival registry /
import f144 join, model-16-gated), `rivals.rs::rival_castle_price`
read in the want gate + `rival_cast_ready` s==16,
`mint_manifestation` now seeds the ctor 1000/9 (matches recorded
rival tokens). ⚠ LAW REFINED ON TAKE EVIDENCE: the every-tick
standing re-stamp is HUMAN-ONLY — mc1l5 t=0 pins Vodor's token at
ctor 1000/9 under his STANDING authored castle (rival init order:
wizext+708 empty when the authored-castle stamp runs; a first
attempt stamping rival tokens every tick regressed 70 mc1l5
fixtures + threw (12,19) mis-stamps on mc1hwl0 until the model-16
gate landed). DEVIATIONS `spell_cast_cost` entry CORRECTED (old
claim "no teardown re-stamp / stale 10000" was wrong; lockout price
= 5000; NOT rival-immune). Human-lane collateral: post-death recast
now 5000 (was stale CAP[lvl]) under the retail arm. Pinned by
`rival_rebuild_waits_out_poverty_at_the_death_stamp` +
`first_castle_lockout_stale_stamp_vs_live_law` (updated to CAP[0]).
DEFERRED: rival token CAP stamp at the rival's own build/level-up
commit (retail sub_47960 site; decision-inert in the port — the
upgrade arm reads CASTLE_CAP directly — but obs-relevant if a take
ever samples a rival token mid-life between rebuild and death).

**2. UNDEAD CONVERT TAIL LANDED (the open villager→skeleton lead,
player-reports 2026-07-19 #8).** Full law in `mobs.rs::m9_convert`
doc + ROADMAP entry (updated). mc1l5 corpus: village battle from
t≈4,441 (the 68-riser army spawn is pose-phase-explained; the
trickle of single missing:(5,9) at ~4-25-tick gaps from t=4,477 =
the conversions the port lacked). No caps; victim deleted raw
(no corpse/ball/credit); newborn state-54 emergence; owner-stamp
wizard-gate surfaced vs unconditional buried (retail quirk kept).
Pinned by `m9_mound_converts_civilians_into_skeletons`.

**3. WALL-OF-FIRE DAMAGE LAW (griffon-instakill report).** Retail:
the (10,53) cloud is the wall's ONLY damage source — bolt +44
(24464) copies into the cloud at impact (sub_52770 tail :62770; the
truncated class-9 state table had banked this as HW-only), cloud
writes f44/maxLife = 24464/128 = **191/tick — the recorded victim
life slope EXACTLY (mc1l5 t=23,415-23,420: two (5,9) at −191/tick)**;
all 225 flames carry `flags |= 0x10080` (:31169: +18 bit0 = no ch0
broadcast, +16 bit7 = no smoke LCG draw) — pure light show. The port
spawned the flames UNSTAMPED: 15 live cells × 100 accumulated into
one mailbox read ≈ 6,000/tick (griffon 10,000 dead in 2 ticks; the
2026-07-29 ambient-fire extents fix unmasked it — before that the
wall flames had zero extents and hit nobody). Retail griffon kill
time = 53 ticks; instakill impossible. LANDED: the 0x10080 stamp +
f44 inherit on napalm children, the +44 bolt→cloud copy now BOTH
games (`proj_move_and_hit(.., true, ..)`), cloud actLife decrement
(:31150-52 — the recorded (10,53) life ramp). The 0x80 half also
kills the port's extra smoke rand draws — the take's post-cast
(5,15) wanderer rand/heading cascade (t=23,404+) was this. Test
`hidden_worlds_firewall_bolt_...` updated: base MC1 copies too.
COLLATERAL TO SPOT-CHECK (banked, unverified): pre-fix port walls
also tree-burned (amt/10 ch0 to class-2 m0) and castle-pre-passed
with no radius gate — both now suppressed by the stamp; visual
smoke density on walls drops to retail's zero.

**Receipts:** 699 workspace tests green (MGC_REQUIRE_GOLDENS=1), all
10 fixture suites green (incl. fresh mc1l5 167/167), clippy + fmt
clean. L005 GOLDEN A-E re-pinned for the rival token mint seed —
OBSERVABLE holds every leg (layout-only; no golden-run behavior
moved). PLAYTESTS OWED: all three fixes.

# THE MC2L1 FOUR-DEVIATION SESSION (2026-08-11)

The player's mc2l1 ("Payahandra's tower") take produced four
implementation-ready specs; all four LANDED here, in the order the dig
session proposed. Receipts for every one: workspace tests green under
`MGC_REQUIRE_GOLDENS=1`, all 10 fixture suites 0 regressions, clippy +
fmt clean. **PLAYTESTS OWED: all four.**

**1. THE MANA MAGNET REGRESSION — two structural defects, both fixed.**
The player: the aura pulls a sphere a little, it stops far short, and
it twitches back to life when you walk toward it.
(a) `byte_0x39_57`, the sphere settle counter, has exactly ONE writer —
`sub_68C70` (EF:55494) off `sub_68BF0`'s second loop over the sphere
chain `dword_38523` (EF:55489-90), ported as `mc2_awake_pass`'s sphere
leg by commit 3844924. The sphere tick `TransformArcherToMana_35940`
only READS it (EF:26173). `ball_tick`'s MC2 arm still carried the local
decrement fold it used before that pass existed, so the counter stepped
TWICE per tick and every sphere froze in half the ticks retail gives
it. Removed; the handler now only reads.
(b) The aura's homing stamp `word_0x7A_122` is a PER-TICK HANDSHAKE:
`sub_38D80` re-stamps every unclaimed sphere in range every tick
(EF:28364, `if (!w7A)`) and the SPHERE clears it at the head of its own
tick (EF:26109), latching `v35` — the flag that opens the moving branch
`if (byte_0x39_57 || v35)` and therefore drags a sphere whose settle
counter has already run out. The port homed +122 in the aura claim map
but released it on the ball's MOVING TAIL, which a settled sphere never
reaches: one pull, then the claim latched forever and the aura's scan
skipped that sphere for the rest of the level. The release moved to
retail's position and now latches the kick. The "starts moving again
when the player walks over" was the awake pass's 24-tile re-arm — the
only thing still able to move it. Pinned by
`mc2_aura_drags_a_settled_sphere_home` +
`mc2_sphere_settle_counter_steps_once_per_tick` (both verified
non-vacuous; the counter one reads 80 vs 90 against the old code).
mc2_cave goldens 2-4 re-pinned (leg 2 moves on the decrement alone).

**2. THE JAGGED COLLAPSE GROUND — a missing finalizer, one call.**
Retail's demolish arm (`RemoveCastleStage_385C0`, the
`fontTypeIndex == 0` branch) ends with
`SetHeightmapByBuildingArea_48B50` (EF:28171 → EF:32446): a gated
in-place 3x3 height average over exactly the footprint, in RASTER
order, so smoothed cells feed later windows — a one-pass IIR blur, not
an independent average. `World::mc2_house_collapse` ended with
`mc2_retile_region`, which writes tile_type/angle/shading and never
touches `t.height`, so the rubble carve's per-cell LCG jitter WAS the
final ground: on sloped authored terrain the `pad >= height` fast path
almost never fires, leaving ~58% of cells 0..19 units above the datum
with no correlation to their neighbours. The jitter itself is faithful
(ROADMAP: the pad + "up to +19 byte-wrapping LCG rubble jitter") — only
the finalizer was missing. The port already owned a verbatim port,
`Gen::mc2_smooth_heights_region` (the castle un-stamp twin); made
`pub(crate)` and called at retail's position, BEFORE the port's
approximate retile so the smoother's gate still reads the angle blend
nibble the carve just wrote and the texture pass sees final heights.
Pinned by `mc2_building_demolish_smooths_the_rubble_floor` — an
arithmetic separation, not a tuned threshold: the carve can only write
60..=79, and the finalizer must leave the top-left corner cell at
`(5*100 + 240..=316)/9` = 82..=90. No golden or fixture moved.

**3. THE BUILDING FOOTPRINT PASS — the missing middle pass.**
MC2's `sub_10C80` runs THREE passes on channel 0 and the port had two.
Between the castle list and the tile scan sits a walk of `dword_38527`,
the (10,45) BUILDING list (EF:4076-4105): the 2-D box
(`CompareAxisWithShift_10750`, NO z) then a BUILD00 footprint-mask
sample under the WRITER's tile — **no owner immunity, no damageable
flag, no vulnerability mask, no +66/+67 filter**. The tile scan carries
the matching `(class != 10 || model != 45)` exclusion at EF:4135
because pass 2 owns buildings; `sub_116A0` (the shake variant) has
neither, and MC1's `sub_120B0` has neither. Both landed, gated on the
`sub_10C80` variant. A building is linked into the tile chain at its
ANCHOR alone (`AddEventToMap_57D70` single-links — the multi-link
theory was REFUTED), so a ground fire's 3x3 window reached 4 of the
main tower's 2,024 footprint cells and retail lands all 2,024; the
"damage snaps to the flag" report was the anchor hit being the port's
only hit, the snap itself being faithful.
- ⚠ The mask row is **BUILD00**, not the sprite table remc2 guessed:
  the raw expression is `**filearray[24] + 6*idx + 4`, a 6-byte TAB
  record with w at +4 / h at +5, and the ctor `sub_49A30` reads the
  same row through `filearrayindex_BUILD00DATTAB`. remc2 marks the
  block `//fix it` x3 and transcribes the top-left from the WRITER,
  which would pin the index to the mask's centre cell forever and make
  the parity bump meaningless; the corner is the BUILDING's, computed
  by its own ctor with this exact expression and bump (EF:32780-88) —
  and because the ctor SHIFTS the building a tile to make the sum even,
  re-deriving it is idempotent, which is the corroboration. remc2's
  resolution halving belongs to the same misreading; the port's
  un-halved extents already reproduce the recorded retail ones.
- The two MC2 sites bound to the WRONG writer variant were re-bound in
  the same patch, as the dig required: the dome (`sub_116A0` at
  EF:23393, mc2/morph.rs) and the scorch ring (EF:23513, mc2/tail.rs).
  Both are `sub_116A0` in retail; left on `sub_10C80` they would have
  wrongly ACQUIRED the footprint pass.
- ⛔ HELD BACK deliberately, per the dig's landing caution: the ring
  radius (`(f80+255)>>8` vs retail `f80>>8`) and the ch0 window centre
  (`+128` vs `-128`/`-127`). The over-wide ring is the port's only
  partial compensation for the missing pass; removing both at once
  makes the per-pair TSV unreadable. Re-measure mc2l1 first, then the
  radius alone.
- Pinned by `mc2_area_damage_lands_across_a_building_footprint` (an A/B
  on one 0xFF cell punched in an otherwise solid template, both cells
  outside the anchor's old 3x3 reach; non-vacuous). mc2_slice goldens
  A-E and mc2_cave leg 4 re-pinned — verified attributable to the pass
  alone, reverting the writer re-bind leaves the cave hash unchanged.

**3b. THE IMPORTER DROPPED THE NO-BROADCAST STAMP (found by 3).**
The footprint pass surfaced 4 mc2l24 regressions: 41 (10,0) ground
fires x 400 landing on one village house in a single tick where retail
delivered nothing. Cause: retail's fire tick gates its damage
broadcast on `if (!(byte[2] & 1))` (EF:22719) and the port reads that
bit POSITIONALLY at `flags & 0x1_0000` (three sites) — but
`import_ent_mc2` mapped byte[2] bits 2/4/5 and never bit 0, so every
imported DECORATIVE fire (the 0x10080-stamped light-show family the
mc1l5 wall-of-fire dig pinned) broadcast full damage under conformance.
Carried now. All 4 regressions cleared and mc2l4 t=2249 — an open
`field:3,3:life` exemplar — went conforming (PROMOTED).
⭐ OPEN LEAD: byte[2] bit 1 is the port's `0x2_0000` recycle-stack
marker, also positional, also unimported. Left alone deliberately —
importing it moves free-list behaviour, which is the slot-order desync
lane, and it is not needed here.

**4. THE TORNADO NEVER TURNED YOU.** Retail's whirlwind victim block
`sub_33340` writes the victim's `yaw_0x1C_28` on EVERY arm it takes:
`v38` per tick for the far-grab, near-grab and inner-lift arms — and
`v38` is **56 for the wizard**, `v40 = (class == 3 && !model)` picking
it over the 204 creatures get (EF:24294-99) — while the MID RING
(`d2 >= 0x40000`, not yet grabbed) sets the ABSOLUTE tangent
`bearing + 591` (EF:24350-56), which is what makes the approach a
spiral instead of a fall straight in. The port's player arm carried
only the positional shove on `player_knock`, so the funnel threw the
flyer around while it kept facing exactly where it came in. Added
`Gen::player_spin` — same transport shape as the knock (world writes,
mover drains) but with NO decay, because retail rewrites it from
scratch every tick the funnel holds you and the tick it stops is the
tick you stop turning. Applied in both flyer paths BEFORE the move.
`PlayerSpin` is hash-TRANSPARENT at zero (the NightShade pattern) so no
pre-tornado golden moved, and it is deliberately NOT snapshotted — a
per-tick transient, like the carpet echoes, so SNAPSHOT_VERSION stands
at 8. The grab / lift / camera-roll takeover remains the deferred
FlightVerb seam. Pinned by `mc2_whirlwind_turns_the_flyer_it_sweeps`
(eye = 56, mid ring = the tangent, and the channel drains on read).

**Still open from the mc2l1 intake, in the order they are worth
taking:** the building-life FIELD HOME bug (`mc2_spawn_building` parks
bldgprm word_0 in f140 where retail's home is `subSpellIndex_0x2A_42`
= f44, and the mana `(1000*word_0)>>7` in f136 — right number by
coincidence in fresh play, WRONG under import, mc2l1 t=888 slot 161
retail 190,000 / port 0); the CHAIN SOURCE defect (retail branches
demolish-vs-rebuild on the ENTITY's `fontTypeIndex_0x3D_61`, seeded
`bldgprm[a2].byte_3` by the ctor at EF:32797 and ZEROED by the two
crush paths to force a demolish, where the port re-reads the static
table, so 16 self-chaining ids rebuild forever); the held-back radius
and window-centre fixes above; and the mc2l1 pure-replay blocker (the
t=1 slot-138 rival-wizard z).

# THE mc2l1 ROUND-2 REPORTS (2026-08-11) — ✅ ALL FOUR LANDED

Four more player reports off the same mc2l1 session. **D landed 2026-08-11;
A, B and C (both halves) landed 2026-08-12 from the banked specs — see
§THE BANK, CASHED at the end of this section for what actually shipped, what
the specs got wrong, and the one banked lead the CORPUS REFUTED.** Every dig ran
two independent verifiers (an adversarial refuter and a completeness critic);
all three cores came back CONFIRMED, all three fix specs came back
PARTLY_WRONG. **The verifier corrections below are load-bearing — read them
before implementing, they are not editorial.**

## D — MC1 SPELL SELECTOR: THE RIGHT-HAND CHORD IS CTRL, NOT SHIFT (LANDED)
MC1 has **two** digit paths, and they are the two hands:
- bare digit → `MakeControlCommand_188A0(24, key-2)` (:20568) → slot **+940** = LEFT
- **CTRL**+digit → `MakeControlCommand_188A0(25, key-2)` (:20356) → slot **+944** = RIGHT

Both index the same per-player bind table `var_15198_1875_772[digit]`
(−1 = unbound) and commit through the pending-command mailbox (:48747 / :48766).
⚠ ROOT CAUSE OF OUR ERROR, worth remembering: remc1 annotates the gate
`pressedKeys_12EEF0_12EEE0[29]` as `//clrl + ]`, so the chord looked like it
needed a bracket and the whole feature got treated as a port enhancement free to
pick its own binding. **Scancode 29 is 0x1D = LEFT CTRL; `]` is 0x1B.** There is
no bracket, it is not the only digit path, and retail owns the quick binds (the
`+772` table) — only the bind-from-book UI is ours.
LANDED: `ctrl_mod`, a plain modifier latch separate from `ctrl_held`. They must
stay separate: `ctrl_held` is the selector PANE's hold latch, carries a pointer-
grab release, and is only tracked when `pane.is_some()` — which is FALSE in
default MC1 (`SpellSelector::Auto` → `ctrl_pane: false`), i.e. exactly the game
this chord belongs to. Tracked before the pane's early `return`, cleared on
focus loss. App-layer input only: no sim state, no goldens. **PLAYTEST OWED.**

## A — A CASTLE PERMANENTLY KILLS A SELF-CHAINING BUILDING (BANKED)
This is the CHAIN SOURCE defect banked last session, now complete in both
directions. The degradation link is a **PER-ENTITY** field, not a table read:
- SEED: `sub_49A30` EF:32795/32798 — `fontTypeIndex_0x3D_61 = bldgprm[a2].byte_3`
  (int8 @0x3D; byte_3 IS the port's `BldgParam.chain`).
- BRANCH: `RemoveCastleStage_385C0` EF:28090 `if (!event->fontTypeIndex_0x3D_61)`
  → demolish, else → rebuild with `sub_49A30(successor, fontTypeIndex)` (EF:28190).
- ZEROED BY TWO CRUSH PATHS: `sub_11960` EF:4410-11 (the CASTLE level-up
  pre-clear, called EF:61128) and `sub_3A090` EF:29335-36 (the (10,67) QUAKE grab
  — **not** a castle path; 3 call sites, incl. Events.cpp:2753-55).
- The player's "flat basalt sea-level area" is the demolish branch's own
  `pad >= height → height = 0` (EF:28147-49), already ported at world.rs:8324-27.
- NEITHER STRUCTURE CAN DAMAGE THE OTHER: the castle painter's purge `sub_57390`
  (EF:39746) handles only class 2 (free) and class 5 (kill, minus protected
  models {6,8,10,16,22,23,27} + 25-in-action-200). Class 10 is untouched, both
  ways. So the fight is purely between their TERRAIN passes.
- SECOND CONSUMER: objective type 2 (EF:40771-79) latches on
  `life <= -1 && !fontTypeIndex` — a castle-crushed building COMPLETES a type-2
  objective in retail; a damage-killed chain building hands off via `sub_59760`.

**PORT:** `mc2_spawn_building` never seeds it, and `mc2_house_collapse` re-reads
the static table (world.rs:8199-8200), so the 16 self-chaining ids resurrect
forever. The "levels the castle but does not damage it" half is the SUCCESSOR's
construction pass: it inherits the OLD completion datum (world.rs:8216
`z = site_z` ≡ EF:28191) and lerps the plane toward its own pad every tick
(mobs.rs:2145-47), dragging the castle mound back down — then the castle's next
level-up pre-clears again, and round it goes.

**FIELD HOME RULING: `Ent::f46`.** Already the MC2 alias for @0x3D on class 5,
unread for (10,45) anywhere in the workspace, and the value the importer
currently puts there (@0x2E) is dead on both sides. NOT free, though — three
sites must ship in the SAME commit or a replayed building imports chain 0:
1. `conformance.rs` `import_ent_mc2` needs the (10,45) arm (~:1695);
2. ⚠ `mc2_building_pad_reconstruct`'s field-restore WHITELIST (mc2/pads.rs:195-203,
   called from conformance.rs:1076) is {act_life, tick70, z, site_z, max_life,
   flags} — f46 must join it, or a replayed construction pass drops the link;
3. `mc2_spawn_building` has THREE callers, not two: world.rs:7489 (authored),
   world.rs:8215 (chain) and **mc2/roster.rs:926** (the model-12 builder villager).
Rejected: `f69` — splits the @0x3D alias family and collides with `new_event`'s
explosion defaults.
Blast: `f46` is hashed, so MC2 levels with authored buildings whose bldgprm
byte_3 ≠ 0 move from t=0. MC1 goldens unmoved. The MC2 behavioural suites build
with `bldgprm: Vec::new()` → chain 0 → unmoved. Save format unchanged (f46 is
already serialised).
Verifier corrections to fold in: `dword_38527` is class-10 model **45 ONLY**
(EF:40019-51 — independently re-confirmed, so the port's existing `model65 == 45`
filters are already exact); the port's footprint dirty-bit clear
(world.rs:8236-44) is unconditional where retail gates per cell on
`locData2[1] != 0xff || locData2[0] != 0xff` (EF:28210); several dig EF cites are
off by one or two (4399/4405/4415, 28147/28149, 28191, 28115).

## B — FIRESTORM: THE DISTINCTION IS THE LOCK, NOT THE HIT (BANKED)
⭐ The player's "there has to be a structural distinction between the two types
of structure" is exactly right, and it is one level UPSTREAM of the firestorm:

**A charged fireball can LOCK a castle. It can never lock a (10,45) building.**
`sub_67CB0` case 0x1C (the (9,28) charged fireball, action 29) walks the class-3
list `dword_38519` (EF:54783, castles scored by the model-2-specific
`sub_685D0` EF:54790) and the class-5 buckets — it **never** walks the building
list `dword_38527`, which only the model 1/0x11 possession arm reaches
(EF:54853-58 → EF:55047).

The hub reads its leader `word_0x96_150` (port `f146`) in exactly two places:
phase-0 SIZING (`sub_339B0` EF:24581-90 — bounds from `leader.f80`) and the
per-tick HARD SNAP (`sub_33C70` EF:24722-45 — `leader.pos.z + leader.f78`).
- **Castle leader** → `maxSpeed 3392 / minSpeed 640`, re-centred every tick =
  the engulf the player recognises as real.
- **Leader 0** → the AUTHORED `192/480` compact ring (EF:35950-51), floating
  where the ball died, riding the building's own stamped heightmap (EF:27341) =
  **"spins and runs above the flag", verbatim.**

And the delivery: `sub_65C20` (EF:63057) is the ONE MC2 impact worker that never
struck-stamps its spawned effect — it only ZEROES the projectile's own lock when
nothing was struck (EF:63195-96); `sub_65B50` (action 29) then copies that lock
onto the hub (EF:63027-29). Its two siblings DO struck-stamp (`sub_65820`
EF:62992, `CastPosses_65F60` EF:63557). The port folded all three into one seam
and took the struck-write as universal: **crates/mgc-sim/src/mc2/proj.rs:768
`e.f146 = victim;`**, unconditional. No victim-type branch exists anywhere in
the port's chain.
FIX = gate that stamp on the spawning action. ⚠ **VERIFIER CORRECTION: the gate
is action 29 ONLY, not `0 | 29`.** `CastPlayerFire_65B30` (action 0, EF:63005-09)
leaves the (10,0) splat's lock at the memset 0 UNCONDITIONALLY; including action
0 writes a lock retail never writes, and `f146` is both hashed AND a COMPARED
conformance column (`chase`, conformance.rs:517 → mgc-conform/src/verify.rs:883).
⚠ Second acquisition source the spec must respect: `sub_68940` runs FIRST
(EF:63093) and can lock a class-10 **model-78 MAGIC MINE** from `dword_38535`;
on success `sub_67CB0` never runs.
Corpus: **zero `chase` deviation rows across all six banked MC2 takes** — no
evidence against the change, and no cover for it either; it must introduce none.
Damage is untouched (`sub_33C00` EF:24700-14, 70 per satellite from the
satellite's own quad), matching the player's "damage seems identical".
⚠ Trace-bank note: `docs/traces/mc2-class10-m76-fire-spheres.md` §7 concluded
"remc2 under-transcribes a struck-write". That is **superseded as the
explanation but NOT disproven** — remc1's equivalent worker
`sub_52ED0_53210` (:63188-63210) really does carry a struck-write.
⭐ Latent divergence found in passing: `mc2_castle_extents_ent`
(mc2/castle.rs:416-424) never writes f78, where retail's `SetShiftByCastle_49EC0`
writes `yaw = 0` explicitly.

## C — CLASSIC FIRE PAINTS UNDER THE TREE (BANKED — HALF 2 NEEDS A RULING)
Two independent halves; the first is small and safe, the second is not.

**HALF 1 (safe, retail-literal): the missing IGNITION RE-LINK.** At tree
ignition retail re-links the TREE to the head of its tile chain —
remc1 `sub_41CC0_42000` (:52460, sole call :57698) / remc2 `sub_57D40`
(EF:40306, sole call EF:62443); MC1HW identical (remc1hw :48510 / :53754). Both
are unlink+link with the tree's OWN position, so nothing moves. The sprite pass
walks head→tail and is a pure painter with **no z-buffer at all**, so the OLDEST
member paints LAST = on top. The flame was head-linked one instruction earlier;
re-heading the tree puts the flame behind it in the walk, so the flame paints
after = in front. The port never relinks (mc1/combat.rs:3807-3820,
mc2/scenery.rs:173-191 both end at `tick70 = 1`; `move_relink` no-ops within a
tile and `link` early-returns on `flags & 4`).
Half 1 moves hashed state (`next20`/`prev22` are `Ent` fields) but changes NO
behaviour — verified: the ignition branch clears the tree's hittable bit
(MC1 :57694 / MC2 EF:62439), so the re-headed tree satisfies no scan predicate,
and a relink preserves relative order for every other member. The golden re-pin
is pure bookkeeping. Suites to expect: state_hash.rs, sim_state_hash.rs, plus
snapshot/frankenstein/mc2_slice/mc2_cave/mc2_rivals if their window contains an
ignition.

**HALF 2 — ⚖ PLAYER-RULED 2026-08-11: LAND THE PERFECTLY FAITHFUL RETAIL
VERSION.** The player was shown that reproducing the painter order re-rules
co-tile ordering for EVERY sprite pair in both games (a creature that walks into
a tree's tile becomes the chain head and the tree then covers it) and ruled for
retail fidelity anyway. So BOTH halves land, there is NO opt-out toggle and NO
DEVIATIONS entry — this is the faithful default. The verifier corrections below
still bind; they are about the proposed IMPLEMENTATION, not the ruling.
**The mechanism:**
The port keys billboard depth to the anchor TILE (billboard.wgsl:106-111
`floor(inst.pos.xz)+0.5`, force-written :176), so two sprites on one tile get
bit-identical depth; the opaque pipeline (`depth_write_enabled: true`,
`depth_compare: Less`, mgc-render/src/lib.rs:2554-55) resolves the tie by
submission order — one instanced draw in buffer order (lib.rs:5571-74) = pool
order. So even with Half 1 the outcome is pool-allocation luck unless the
painter order is reproduced.
⚠ VERIFIER CORRECTIONS — the dig's proposal does not survive as written:
- `LivePose` has no `Default` and THREE construction sites (world.rs:1570,
  mgc-app/src/lib.rs:9170, mgc-app/src/entities.rs:2426) — the spec names one.
- The BLEND pipeline does **not** write depth (lib.rs:2601), so translucent-vs-
  translucent order is decided by the back-to-front sort on RAW plan distance
  (lib.rs:4506-12); a depth epsilon does nothing there.
- An ABSOLUTE chain-hop rank is wrong: the burning tree's tile continuously
  gains/loses (10,13) smoke puffs for the whole 130..189-tick burn, so the
  proposed 64 cap is actually reached and clamping restores the exact tie. It
  must be a RELATIVE rank over the co-tile DRAWABLE set.
- Retail's chain order is LINK RECENCY, not allocation age, so Half 2 re-rules
  co-tile ordering for EVERY sprite pair in both games — a creature that walks
  into a tree's tile becomes the head and the tree then covers it. That IS what
  retail did at 320x200 with no z-buffer, but it is a broad, immediately visible
  presentation change well beyond the reported bug. **Ask the player first, and
  it probably wants a DEVIATIONS entry either way.**
- The comparison/replay billboard paths (`push_billboard` entities.rs:1841,
  `ghost_billboard` entities.rs:1790-1800) keep the bug under the proposal.
- Presentation-only, so there is NO guard: per the visual-only triage doctrine,
  name every consumer (opaque pass, blend pass, the mirror pass at
  billboard.wgsl:91-94, and the `conceal` alpha demotion at lib.rs:4483-88 which
  can push an opaque MC2 sprite into the no-depth-write bucket) before landing.
  Under the player's fidelity ruling this is a "name them and playtest" item,
  not a reason to hold the change.
⭐ THE FLAME'S PLACEMENT IS A TWO-PART LAW AND THE PORT ALREADY HAS ALL OF IT —
⛔ do not "fix" any of it. **Player-explained 2026-08-11, code-confirmed:**
- BOTH games SIZE the flame from the tree: MC1 `flame.+46 = (3 * tree.+84) >> 2`
  (:57685-86, ported mc1/combat.rs:3812 — note MC1's home is +46 where MC2's is
  `word_0x2C_44`, an alias split); MC2 `flame.word_0x2C_44 = (3 * tree.fov) >> 2`
  (EF:62428-30, ported mc2/scenery.rs:180).
- MC2 ADDITIONALLY lowers the flame 128 (EF:62429-33, ported
  mc2/scenery.rs:172) — **because the MC2 tree visibly SHRINKS as it burns**:
  `sub_64F60` swaps it to the charred sprite at `life < 60` (83→226, 84→227 via
  `SetHalfSpeedEntity_49DA0`, EF:62471-84, ported mc2/scenery.rs:204-207, and
  the set-sprite call re-derives the extents). The flame therefore rides the
  VISIBLE trunk rather than the tree's anchor. MC1 has no such drop and no such
  shrink — correctly absent from the port's MC1 arm.
⭐ Consequence, and it usefully narrows the whole lane: the ONLY divergence in
either game's ignition block is the missing RELINK. Everything else in
mc1/combat.rs:3795-3820 and mc2/scenery.rs:150-196 reproduces retail line for
line.
⭐ OPEN QUESTION FOR THE PLAYER: is this one case or a family? Retail has NO
relink for fire sharing a tile with a dwelling, a building anchor, a castle stage
piece or a second tree — each relink function has exactly one call site — so
retail genuinely draws fire UNDER those, and the port may already match.

## ✅ THE BANK, CASHED (2026-08-12) — A, B AND C (BOTH HALVES) LANDED

Implemented straight from the specs above, corrections included. All 10 fixture
suites green (0 regressions, 0 drifts, 0 unpromoted fixes), 394 unit tests, fmt
+ clippy clean, every WGSL shader naga-validated.

### A — the degradation link moved to its retail home, `Ent::f46`
`mc2_spawn_building` seeds it from `bldgprm[type].byte_3` (:32795-98) and
`mc2_house_collapse` now branches on the ENTITY's copy (:28090) instead of
re-reading the static table. The two ZEROING paths were already correct and
untouched (`mc2_castle_preclear` / the flood's quake grab), so the port had the
clear and not the read — the exact shape that let the 16 self-chaining ids
resurrect forever. Shipped in the same commit, as the spec demanded: the
importer's `(10,45)` arm (`@0x3D`, was importing the dead `@0x2E`),
`mc2_building_pad_reconstruct`'s field-restore whitelist, and — a consumer the
spec named but did not schedule — **the type-2 objective latch, which was
reading `bldgprm[f71].chain`**. That last one is a real behaviour change in
retail's direction: a castle-CRUSHED chain building now COMPLETES a type-2
objective, where the table read left it pending forever.
One retail detail the spec missed and the decompile carries: on pool
starvation the successor spawn fails and retail clears the DEAD building's own
link before going dark (:28187-88) — ported.

### B — the firestorm leader is the LOCK, gated on action 29 only
`mc2_proj_impact`'s `e.f146 = victim` became
`match act { 29 => lock, 0 => 0, _ => victim }`. The port already carries the
retail action in `tick70` and already routes 0/1/29 apart, so the gate is
exact. `mc2_aim_lists(0x1C)` was ALREADY building-free, so the upstream half of
the law needed no change — only a fixture to pin it.
⭐ ACCEPTANCE MET: the spec required the change introduce no `chase` rows.
Measured on mc2l1 (18,632 pairs): `chase` = **0 before, 0 after**.

### C — both halves
Half 1: `Gen::relink_head` (`sub_41CC0_42000` / `sub_57D40`), called from the
one ignition site in each game. Pure bookkeeping, and the corpus proves it:
mc2_slice's RAW state hash moved on checkpoint E while its layout-independent
OBSERVABLE projection did **not**.
Half 2 (player-ruled fidelity): `LivePose::chain_depth` carries each pose's
place in its tile chain — RELATIVE over the co-tile POSE set, per the verifier's
correction, so no cap can restore the tie. It reaches both consumers the
verifier named: the opaque pass via a `CHAIN_BIAS = 0.25/DEPTH_RANGE` depth
nudge (a quarter of one tile's depth quantum — no co-tile nudge can cross a tile
boundary), and the blend pass via its sort, which now keys back-to-front on the
**anchor tile's** plan distance — the metric its own comment always claimed and
the one the depth channel actually uses — so co-tile translucent pairs tie
exactly and the chain rank breaks them. The `conceal` alpha demotion rides the
blend path and is covered by the same sort; the mirror arm computes depth from
the same tile center, so the nudge rides through unchanged. The comparison /
replay paths (`push_billboard`, `ghost_billboard`) and the fire-preview poses
have no chain and take the neutral `0.5`. No toggle, no DEVIATIONS entry — the
player ruled this the faithful default.

### ⛔ REFUTED BY THE CORPUS — the `mc2_castle_extents_ent` f78 lead
§B above banked "a latent divergence: `mc2_castle_extents_ent` never writes f78
where retail's `SetShiftByCastle_49EC0` writes `yaw = 0` explicitly". The
decompile is right (EF:32891) and the field mapping is right
(`f78` ← `ayaw` ← `array_0x52_82.yaw`) — **and the fix is still wrong.** Adding
the write costs **469 mc2l0 fixtures** on the compared `field:3,2:applied_yaw`
column: retail castles carry a NONZERO yaw across the ticks the port refreshes
extents on, because the port's callers are not retail's callers (retail reaches
the helper only at the level-up seams and around the pre-clear's temporary
next-level box, EF:4399/4415) and the castle path's follow-up yaw/fov writes own
the lane. Reverted, with a ⛔ comment at the site. ⭐ LESSON, and it is the same
one the castle-pool "2x" reading taught in §THE MOB-SPEED RUNAWAY: a
decompile-literal write is a HYPOTHESIS until the recording agrees. Recorded
gameplay outranks the decompile.

### Fixtures (5 new, 4 of them non-vacuous — checked by reverting the patch)
- `a_firestorm_leaders_its_lock_never_the_thing_it_struck` (three ways: a
  building struck but never lockable → leader 0; a castle LOCKED but never
  touched, detonating on a creature → leader = the castle; action 0's splat →
  leader 0 even with a live lock and a struck victim)
- `a_building_collapse_branches_on_its_own_link_not_the_table` (both halves:
  seeded link rebuilds — which pins the seed — cleared link demolishes)
- `tree_ignition_re_heads_the_tree_mc1` / `_mc2`
- `the_burning_tree_poses_behind_its_own_flame` (half 2 reaching the renderer)
- `a_castle_preclear_cuts_the_building_degradation_link` — ⚠ passes BEFORE and
  AFTER by design (that half was already correct): a regression guard, not a fix
  witness, and labelled as such at the site.

### Goldens re-pinned, both attributed by bisecting the patch
- `mc2_cave` ALL FOUR — A's `f46` seed, hashed, moves the stream from t=0 for
  every authored building with a nonzero byte_3. Behaviour in that window is
  unchanged (no level-up, no quake grab ⇒ entity copy ≡ old table read).
- `mc2_slice` checkpoint E only — C half 1's `next20`/`prev22`. OBSERVABLE held.

### ⚠ WHAT THE CORPUS DOES NOT SHOW
mc2l1 whole-take: 261,182 → **261,179** rows (−3), conforming pairs 370 → 370.
These are correctness fixes for events the take never captures — no castle
levels onto a chain building in the recorded window, and no charged fireball
meets a house. **Do not read the flat number as "no effect"; read it as "no
regression, and the corpus cannot see the fix."** The fixture suites and the
five new pins are the evidence here, not the row count.

### ▶ PLAYTESTS OWED (four, one pass)
1. A castle levelling into a village building DEMOLISHES it — flat basalt, no
   endless rebuild, and the castle's own mound stops being dragged back down.
2. A charged fireball on a HOUSE leaves a compact ring floating where the ball
   died (not an engulf); on a CASTLE it still engulfs and tracks.
3. A burning tree's flame draws IN FRONT of the trunk.
4. ⚠ THE BROAD ONE: half 2 re-rules co-tile ordering for EVERY sprite pair in
   both games — a creature that walks onto a tree's tile becomes the chain head
   and the tree covers it. That IS what retail did at 320x200 with no z-buffer,
   and the player ruled for it knowingly, but it wants a real look.

# THE MOB-SPEED RUNAWAY (2026-08-11) — ✅ RESOLVED: THE MISSING CHASE-EXIT RESTORES

⭐ **JUMP TO THE END OF THIS SECTION** — everything between here and `RESOLVED
2026-08-11` is the investigation as it ran, kept for its rulings. The answer is
that MC1 bounds creature speed with per-model chase ENTRY/EXIT trailers, not
with a cap, and the port was missing four of them.

⚠⚠ **READ THIS FIRST — THE HEADING BELOW IS SUPERSEDED.** This section was
written while the report was believed to be VR-only. **The player then observed
the same runaway on the DESKTOP build.** The mechanism below is UNCHANGED and
still correct — it is pure integer code, byte-identical between `master` and
`vr/master`, which is exactly why three of the five verifiers refused to accept
"VR-only" as explicable and said so. **The corrected reading: the ratchet is the
bug, and the Android `awake_range = 80` override is only a RATE AMPLIFIER
(~11x the awake area) that surfaces it in hours instead of sessions.** The fork
config is therefore a real finding but NOT the cause, and the "PRIMARY fix =
fork-side config" line below is WRONG — the fix is port-side. The open question
became: *what bounds this in retail?*, which is being measured against the
recorded corpus (retail's own creature speed over an 18k-tick take). Everything
in the KILLED list at the bottom still stands.

## (superseded heading) THE VR MOB-SPEED RUNAWAY — THE ANCIENT BUG, RE-ARMED BY CONFIG

Player report from the VR fork (`vr/master`, Android/Quest): *"some monsters keep
speeding up throughout the game until they are super fast, well faster than a
flyer with Accelerate on — never observed outside the VR port."* Dug across five
independent modalities (accumulator audit / platform math / fork diff /
archaeology / retail ramp law), each adversarially verified. **SOLVED. Not
landed — the fix is a decision, see below.**

## THE ANSWER IN ONE LINE
The port's pack catch-up has no cap of its own; retail's cap **is the awake
gate**; and the VR fork force-sets `awake_range = 80` on Android, which
effectively switches that gate off for the whole map.

## THE MECHANISM (survived every attack)
`crates/mgc-sim/src/mc1/mobs.rs:2419`
`self.ent[i].f126 = self.ent[l].f126.wrapping_add(self.ent[l].f130);` — a pack
follower takes the LEADER's speed plus the leader's accel. `f128`, the
creature's own max speed, is **never consulted as a cap on this path**.
⭐ **THE CARRIER IS THE PACK *EXIT*, NOT THE JOIN.** Nothing re-baselines `f126`
when a creature LEAVES a pack: all three break sites (:2369-72 leader chased,
:2384-87 leader elsewhere, :3059-71 the damage-inbox arm) drop the link and
return the creature to WANDER with the inflated speed intact, and
`mob_idle`/`mob_wander`/`mob_chase` never write `f126`. `pack_scan` admits any
leaderless same-model creature (`f52 == 0`) with **no speed filter**
(mc1/mobs.rs:826-846; retail's own filter at remc1 :21551 is the same), so an
inflated ex-follower is a fully legal future LEADER. Each generation adds one
`f130` permanently, so the POPULATION maximum is monotone non-decreasing and
unbounded. Cadence is once per `v_26` = 30-40 ticks (mc1/mobs.rs:2354), which is
why it reads as a slow climb over a session rather than a step change.
**SCOPE — this is why the player said "SOME monsters":** the ratchet only bites
families with `f130 != 0`, a PACK slot, and no per-model `f126` restore — worms
m0/m3, m1, m10, m16. IMMUNE: m2 bees (restore `f126 = f128` on every chase exit,
:1164-66), m6 kraken (forced 30/tick), m4/m15 guards, m9, m13/m14; m5 and m12
never reach `mob_pack` at all (dispatch overrides at :3130/:3191).

## WHY VR ONLY — THE ONLY SUCH LEVER IN THE TREE
`vr/master:crates/mgc-app/src/lib.rs:8653-8666`, inside
`#[cfg(target_os = "android")] fn parse_args()`:
```
args.fog_distance = Option::from(80);
args.awake_range  = Option::from(80);   // faithful = 24 tiles
args.pool_slots   = Option::from(5000); // faithful = 1000
args.thrust       = Some(ThrustModel::Enhanced);
```
WANDER's `pack_scan` is AWAKE-GATED (mc1/mobs.rs:1025-30). The faithful wake
radius is 24 tiles (`awake_gate_sq = 0x240_0000`, chassis.rs:70/81), so on
desktop a creature more than 24 tiles from the player never enters the
join/break churn at all — the ratchet barely advances. **80 tiles is 11.1x the
awake AREA, and on a 128x128 torus that is effectively the whole map awake,
always**, so every eligible creature churns continuously for the entire level.
`pool_slots = 5000` compounds it through DENSITY (5x the simultaneous
same-model population feeding the churn).
Both are G-class knobs the desktop build refuses silently to treat as faithful
(it prints "G-class — not a faithful run", lib.rs:989-1014); on Android there is
no opt-out, because `Config::load` is stubbed to `Self::default()` and the
forced args are applied after it. Neither commit is in master (verified:
`git merge-base --is-ancestor <sha> master` → exit 1 for both).
⭐ **AUTHORING STORY (near-certain):** `fog_distance = 80` sits on the line
directly above `awake_range = 80` under a `// TODO: Configure this via a menu
option...`. The intent was plainly *"draw distance 80, so wake what you can
see"* — entirely reasonable for VR, and the coupling is the accident.
⭐ The same block forces `thrust = Enhanced`, so the player's own yardstick
("faster than Accelerate") is already the enhanced mover's boosted ceiling.

## THE ANCIENT BUG THE PLAYER HALF-REMEMBERED IS REAL, AND THIS IS IT
`docs/archive/ROADMAP-2026-07-19-full.md:7974-88`: *"Runaway worm/bee speed
(packs gradually accelerating without bound). Two compounding causes, both
fixed: (1) WANDER's scans are entirely awake-gated in the original (:21514) —
the agent trace read it as 'awake→wizard ELSE pack', so every distant asleep
crowd packed up. (2) The pack catch-up at :21814 … the `+=` is a remc1
maintainer MIS-FIX and porting it verbatim is the runaway."*
Leg (2) was fixed and STAYED fixed (the port uses MC2's SET form, EF:9482, for
both games — the `+=` is gone). **Leg (1) is a chassis PARAMETER, not code —
and the fork turns it off.** So the report is the ancient bug's mechanism,
re-armed by configuration. The fork author's belief that "the VR port does not
really touch the sim" is CORRECT and is exactly what made this invisible: the
whole `mgc-sim` delta over the merge base is **14 lines in
crates/mgc-sim/src/lib.rs** (the Android `pitch = 0` pose override), and
`mc1/mobs.rs` is byte-identical. The fork changes the sim's CHASSIS from the
launcher, which no diff of `crates/mgc-sim` can show.

## FIX — A DECISION, NOT A PATCH
⚠ REORDERED after the desktop sighting: (2) is now the PRIMARY lane and (1) is a
worthwhile fork hygiene item that reduces the rate but does not fix the bug.
1. **(fork hygiene, not the fix):** stop forcing `awake_range = 80` on
   Android — decouple wake radius from fog/draw distance, which is what the TODO
   wanted anyway. `pool_slots = 5000` should be reviewed on the same pass (it is
   a G-class knob). This lowers the rate; it does not remove the ratchet.
2. **PRIMARY (port-side, NEEDS A PLAYER RULING):** should the catch-up clamp
   `f126` to the creature's own `f128`? ⚠ **Retail does NOT clamp** — neither
   remc1 :21814 nor remc2 EF:9482 — so a clamp is a DEVIATION and would want a
   `docs/DEVIATIONS.md` entry. It is defensible as robustness (it makes the
   ratchet structurally impossible at any awake_range) but it is not fidelity.
   The faithful alternative is to re-baseline `f126` at the pack-BREAK sites,
   which is also not in retail. Ask before landing either.

## ⛔ KILLED BY THE VERIFIERS — DO NOT RE-OPEN
- **Build-profile / overflow-checks asymmetry.** Dead three ways: MC1's line is
  `wrapping_add` and cannot panic on any profile; desktop playtesters run
  `--release` too (.github/workflows/release.yml:42); and the symptom appears at
  `f126` ~150-400 while i16 overflow is 32767 = 128 tiles/tick, ~400x later.
- **Self-leader (`f52 == i`) or a cyclic leader graph.** Provably unreachable:
  every scan admits only `f52 == 0` ROOTS and skips self, so the leader graph is
  a FOREST and relink is path compression.
- **Chain DEPTH as the carrier.** Bounded — a merge adds exactly one level and
  the flatten removes one per follower per `v_26`.
- **m27 hydra branch retract.** Wrong entry cited, sign backwards (it is a
  12/tick DECREMENT), and time-bounded to ~11 ticks.
- **"pool_slots 5000 means slots are never recycled, so inflated f126
  persists".** Wrong reasoning — recycling only ever touches DEAD entities.
  `pool_slots` contributes via density, not persistence.
- **"mob_idle's pack_scan is not awake-gated".** True but irrelevant: `mob_idle`
  is dead code in MC1 normal play (every ctor spawns in WANDER).
- **"Bees have no self-heal".** They do — `bee_chase` restores `f126 = f128` on
  every chase exit.
- Platform math is NOT implicated: the whole path is integer.
⭐ Partial natural brake worth knowing: `creature_move` sets `act_life = -1`
when all four probed headings fail the capability/roughness gates
(mc1/mobs.rs:717-22, :752-54), so a very fast creature is likelier to cull
itself on terrain. A brake, not a bound.

## ADDENDUM (2026-08-11, same day): THE CORPUS MEASUREMENT + THE RETAIL READ

**⚖ ANSWER TO "does retail do this too, and is the awake gate what saves it?"
— retail has the SAME UNBOUNDED MECHANISM, and the corpus shows it does not
actually run away in a full level.** Both halves matter.

### The retail read (full writer census of remc1, verdict SOUND)
NOTHING bounds it in retail that the port lacks, for the five named families:
- `+126` has exactly ONE inflation site in the whole engine — remc1 :21814, the
  pack catch-up — and retail's transcribed form is `+=`, i.e. it compounds
  every `v_26` ticks *within a single pack episode*, where the port's SET form
  advances at most one step per leadership generation. **The port is the more
  conservative of the two.**
- `+128` (max speed) and `+130` (accel) are **WRITE-ONCE, ctors only**. No live
  handler ever writes them, so "max speed" can never cap anything except
  through an explicit `f126 = f128` restore.
- ⭐ THE STRUCTURAL QUESTION IS ANSWERED "NO": retail's class-5 mover
  `sub_196E0` (:21182) passes `actSpeed_126` VERBATIM as the step distance into
  `sub_41EC0_42200` (:52523-44), which applies it with no clamp and no
  re-derivation, and the behaviour row carries turn rate / altitude / roughness
  / terrain mask / cadence / range / cone and **no speed field at all**. An
  inflated actSpeed is exactly as load-bearing in retail as in the port.
- The state setter (`sub_424F0_42830` :52757-60 is one assignment), the wake
  pass (:64266-64371 writes only +58/+59/+48), the damage-inbox prologue and the
  ctors contain no re-baseline the port lacks. MC1 has no respawner for these
  families, so "they die before it matters" is dead too.
- The behaviour table (:5240-71) is byte-identical to the port's
  `mc1/behavior.rs:81-112`, so cadence/range/cone are NOT the divergence.

### The corpus measurement (mc1l5, 18,633 ticks, `want` = RETAIL / `got` = PORT)
722 class-5 `speed` rows; **128 of them are slot-desync artifacts and were
excluded** (e.g. t=4484 slot 813: retail holds a class-5 m9 with life 1000, the
port a class-9 m1 with life 9 — every field differs and its "speed 464" is a
projectile's). The clean 594:

| model | n | retail max | port max |
|---|---|---|---|
| 2 (bee) | 12 | 70 | **210** (= 3x70) and **−30** |
| 4 | 22 | 30 | 30 |
| 7 | 22 | 20 | **23** |
| 9 | 538 | 20 | 20 |

- **RETAIL'S CREATURE SPEEDS DO NOT CLIMB.** Across a full level they sit at
  their spawn values (20/30/70). Whatever holds retail together, it holds.
- **m2's 210 / −30 are the bee's own lunge and recoil** (`sub_1B3C0` :22347
  `f126 = 3*f128`, :22359 `f126 = -f130`) — legitimate values at the WRONG
  TICK. That is a chase-phase divergence, not a ratchet.
- ⭐ **m7 at 23 vs retail 20 is the missing restore, MEASURED** — exactly one
  `f130` step (m7's accel = 3) above the max it should have been restored to.

### ⚠ WHAT THE CORPUS STRUCTURALLY CANNOT SHOW
`verify-deltas` re-imports retail state at EVERY pair and ticks ONE step, so it
can prove the single-tick math and can prove retail does not ramp — but it
**cannot observe a cumulative free-play ratchet by construction**. The same
limit bit the mc2l1 tower-damage lane. So the ratchet is neither confirmed nor
refuted by this measurement; what IS confirmed is that retail does not ramp and
that the port has a concrete, measured speed-restore gap.

### THE ONE REAL PORT DEFECT FOUND (faithful fix, no ruling needed)
**m7 has no per-model CHASE handler in the port.** `mc1/mobs.rs:3182`
(`(_, 2) => self.mob_chase(...)`) routes model 7 through the shared chase, so
the port has none of retail's `sub_1C960` (:23319-55, dispatch slot 0x2C, twin
confirmed at remc1hw :21876-912), which carries THREE speed writes:
`:23330-31` restore `f126 = f128` when the 30-tick dug-in timer expires;
`:23342-43` set `f126 = f130` (=3) on entering the dug-in form; **`:23352-53`
restore `f126 = f128` on ANY chase exit.** m7 therefore satisfies every ratchet
precondition in the port while retail bounds it. **Add m7 to the affected list
and port `sub_1C960` — this is a straight faithfulness fix and the corpus
already scores it (23 vs 20).**

### A SECOND, OPPOSITE-DIRECTION FIDELITY BREAK (banked)
`mc1/mobs.rs:3062-3070`, the PACK arm of the damage inbox, clears only the
FOLLOWER's `f52`; retail's `sub_1A390` (:21758 + :21762) clears **both** the
leader's and the follower's. The port's stale leader `f52` makes that leader
invisible to `pack_scan` (which rejects `f52 != 0`), so the port forms FEWER
packs on this path. It cannot be the amplifier — but it is a real break, and it
means port-vs-retail pack churn is not comparable until it is fixed.

### CORRECTION TO THE SECTION ABOVE
There is **no** `docs/DEVIATIONS.md` entry for the pack catch-up. DEVIATIONS.md
:135 only name-drops :21814 ("like :21814") inside the `combat.rs` mail-write
entry. Any clamp/re-baseline fix still needs a NEW entry — and note the corpus
now shows a clamp WOULD be visible: retail carries m2 `f126` = 95 against
`f128` = 70 for 62 creature-ticks in this very take, so `.min(f128)` at the
catch-up would break conforming rows.

## ⚖ RESOLUTION (2026-08-11): THE `+=` IS THE ARTIFACT — OUR SET FORM IS FAITHFUL
Player ruled "always be perfectly faithful", and proposed adopting remc1's `+=`
on the reasoning that chase-exit restores bound it anyway. **The reasoning is
sound; the premise was wrong, and the action inverts.**

BOTH remc1 :21813-14 AND remc1hw :20370-71 carry the SAME pair of lines:
```c
//v10 = v3x->acceleration_29925_130 + v3x->actSpeed_29921_126;
a1x->actSpeed_29921_126 += v3x->acceleration_29925_130;
```
The COMMENTED line is the decompiler's own output and takes BOTH operands from
`v3x` — the LEADER. That is the SET form, and it is byte-for-byte what the port
does (`mc1/mobs.rs:2419` `f126 = leader.f126 + leader.f130`). The LIVE line
reads the FOLLOWER's own speed and is not equivalent.
⭐ remc1 and remc1hw share a maintainer, so their agreement is ONE hand edit
applied twice — **not** independent corroboration. The independent corroboration
runs the other way: **remc2 EF:9482, a different transcription lineage, carries
the SET form** and matches the commented-out MC1 output exactly. Three
machine-derived sources agree; only the hand edit dissents.
⇒ The archived "remc1 maintainer MIS-FIX" ruling
(docs/archive/ROADMAP-2026-07-19-full.md:7974-88) is **CONFIRMED**. The port's
line is already the faithful one. ⛔ **DO NOT adopt the `+=`** — it would import
a decompiler artifact and make us LESS faithful. This is
[[decompile-corroboration-across-binaries]] working exactly as designed: a
commented-out original beside a live rewrite proves the rewrite is human.

### NEXT SESSION, FIRST THING (player-directed) — revised order
1. ✅ **NO CHANGE to `mc1/mobs.rs:2419`** — already faithful. Add a doc note
   citing the commented-out originals so nobody "fixes" it toward the `+=`.
2. ⭐ **PORT m7's `sub_1C960`** (remc1 :23319-55 / remc1hw :21876-912): its
   three speed writes, above all the chase-exit restore `f126 = f128` at
   :23352-53. `mc1/mobs.rs:3182` currently routes model 7 through the shared
   `mob_chase`. Corpus already scores it: mc1l5 shows port 23 vs retail 20.
3. **Clear the LEADER's f52 too** in the damage-inbox PACK arm
   (`mc1/mobs.rs:3062-3070`) — retail clears both (:21758 + :21762). Real
   fidelity break; currently SUPPRESSES joins, so port-vs-retail pack churn is
   not comparable until it lands.
4. THEN re-assess the ratchet. With the chase-exit restores in place the
   player's boundedness argument may simply hold — every chase ends, and retail
   re-baselines on exit. Re-measure before proposing any clamp (a clamp remains
   a DEVIATION and would break conforming rows — retail carries m2 f126 = 95
   against f128 = 70 in this very take).

## ⚖⚖ THE RECORDING RULES (2026-08-11): NO RATCHET IN RETAIL, AND `+=` IS OUT
Player: *"the deviation could just be remc1, not retail, and we have the
recording to make that ruling."* Correct, and it does.

**UNBIASED SAMPLES** (`dump-state recordings/mc1l5.mgcr <t> $(seq 1 400)`, every
LIVE class-5 entity, comparing its own `f126` against its own `f128`):

| tick | live class-5 | above own f128 |
|---|---|---|
| 5,000 | 146 | **0** |
| 12,000 | 72 | **1** — slot 332, m2, f126 = 210 = exactly 3 x f128 (the lunge) |
| 17,000 | 40 | **0** |

Across 18,633 ticks the ONLY creature ever above its own max speed is a bee
mid-lunge at exactly 3x (`sub_1B3C0` :22347), a deliberate bounded state that is
restored. **There is no speed ratchet in retail at any point in the take.**

⇒ **remc1's `+=` is EMPIRICALLY RULED OUT.** Under it a follower accumulates
`+f130` every `v_26` (30-40) ticks for as long as it follows — hundreds of steps
over this take — and the recording would be full of inflated creatures. It
contains none. Recorded gameplay outranks the decompile, so the `+=` is not the
original's behaviour whatever its provenance. ⛔ DO NOT PORT IT. This is now
settled on EVIDENCE, not on the artifact inference.

### ⚠ BUT THE SET FORM IS NOT VINDICATED EITHER — THE QUESTION HAS MOVED
At t=5000 a 15-deep `f52` chain of m0 worms sits at `f126` = 30 for EVERY
member, with `f128` = 80 and `f130` = 16. The SET form predicts a follower at
30 + 16 = 46. They are all at 30. **So in retail the catch-up essentially never
fires, in either form.** The question is no longer "which arithmetic?" but
**"why does ours fire when retail's does not?"** — i.e. the GATING/CHURN lane,
not the formula.

### ⚠⚠ CONFOUND TO RESOLVE FIRST — `f52` IS OVERLOADED
For worms, `+52`/`+54` are the MULTIPART BODY-SEGMENT links (features.rs `Ent`
doc: "+52 = toward the head, +54 = toward the tail"), NOT pack links. The deep
chains sampled above are very likely worm BODIES. **Whether the pack arm and the
segment chain share `f52`, and how retail tells them apart, must be settled
before any pack-churn analysis means what it appears to mean.** Everything in
the churn lane is provisional until then.

### CORRECTION TO THE ADDENDUM ABOVE
The "retail's creature speeds do not climb — they sit at spawn values" table was
**BIASED**: `verify-deltas` TSV rows exist only where retail and port DIFFER,
which is exactly where retail restores and the port does not. Retail DOES reach
210. Tracked directly: slot 281 (m2) runs `f126` = 210 at t=730 and t=731, then
retail restores to 70 at t=732 while the port holds 210. The correct reading of
those rows is **"the port misses retail's restores"**, not "the port inflates".
Column semantics confirmed at crates/mgc-conform/src/verify.rs:817
(`"{}: retail {} port {}", d.field, d.want, d.got`) — `want` = RETAIL.

### REVISED PLAN (supersedes the previous "next session" list)
1. ⛔ NO CHANGE to `mc1/mobs.rs:2419` — neither to the `+=` (ruled out by the
   recording) nor otherwise. Add the doc note.
2. **Settle the `f52` overload** (pack link vs multipart segment link) — this
   gates everything else.
3. **Port m7's `sub_1C960`** — still the clearest single defect: the corpus
   scores it (port 23 vs retail 20) and it is a missing RESTORE, which is the
   same family of defect as the m2 rows.
4. **Audit the port's RESTORES generally.** The corrected reading says our real
   divergence is failing to restore where retail restores — m2 at t=732 and m7
   both. That is a different and more tractable bug class than the ratchet.
5. Only then re-open the ratchet, with the churn/gating question, not the
   formula.

---

## ✅ RESOLVED 2026-08-11 — IT WAS THE RESTORES, AND THEY ARE LANDED

The revised plan's step 4 ("audit the port's RESTORES generally") turned out to
be the whole answer. **MC1 does not bound creature speed with a clamp. It bounds
it with per-model ENTRY and EXIT trailers hung off the individual state
handlers.** `+128`/`+130` are write-once in the ctors, the mover passes `+126`
verbatim (`sub_196E0` :21182 → `sub_41EC0` :52523), and the pack catch-up
(`sub_1A390` :21814) is the only writer that can push `+126` past a creature's
own `+128`. What ends that inflation is the exit trailer of whatever state the
creature leaves next. Miss a trailer and that one creature keeps the inflated
speed for the rest of the level — which is exactly the player's *"some
monsters"*.

### THE AUDIT — every `+126` writer in the MC1 engine, against the port

| retail site | what it does | port before | now |
|---|---|---|---|
| `sub_1A390` :21814 | pack catch-up (the only inflater) | ✓ SET form | unchanged, ⛔ do not clamp |
| `sub_1B3C0` :22347/:22359/:22366 | m2 lunge / recoil / chase-exit | ✓ | ✓ + death tick |
| `sub_1BC50` :22753 | m4 arm, speed 0 | ✗ one tick late, inside the chase | `militia_arm`, promotion tick |
| `sub_1BCE0` :22768 | **m4 chase-exit disarm, `+126 = +128`** | ⛔ **MISSING ENTIRELY** | `militia_disarm` |
| `sub_1C4A0`/`sub_1C4F0`/`sub_1C880` :23116/:23146/:23276 | m6 pin 30 | ✓ value, ✗ ORDER (all pre-handler) | chase pre-, wander/pack post- |
| `sub_1C960` :23331/:23343/:23353 | **m7 plant / un-plant / chase-exit** | ⛔ **MISSING ENTIRELY** (`(_, 2) => mob_chase`) | `m7_chase` |
| `sub_1CE30` :23551 | m8 griffon cooldown restore | ✓ | ✓ |
| `sub_1DCD0` :24247 | **m9 chase-entry, speed 0 (rooted)** | ⛔ MISSING | `m9_enter_chase` |
| `sub_1DD50` :24257 | m9 chase-exit restore | ✗ one tick late, in state 55, with the wrong `+26` (400 vs 50) | on the chase-exit tick |
| `sub_1F640`/`sub_1FAC0` :25401/:25438 etc. | m13/m14 feeder home speeds | ✓ | ✓ |
| `sub_20410`/`sub_20450` :25891/:25901 | m15 guard enter/exit | ✓ (the model the port got right — the template) | ✓ + death tick |

### THE THREE LAWS THAT FELL OUT

1. **ENTRY trailers run on the PROMOTION tick**, from the *non-chase* handler
   (`sub_1B5A0` :22432 / `sub_1B5D0` :22690 / `sub_1BBE0` :22725 for m4; the
   `sub_1C900`/`sub_1CA00` `+26 = 1` pair for m7; :23922/:24220 for m9), never
   from the chase's own first tick. Coverage is NOT uniform — m7's idle slot
   `sub_1C8F0` and m15's pack slot `sub_203E0` are bare shared handlers with no
   trailer at all, which is why `chase_entry_trailer` is keyed on (model, role).
2. **EXIT trailers run on the tick the chase breaks**, for ANY reason —
   including **the creature's own DEATH**. Easy to miss: retail's damage
   prologue lives *inside* each handler and `goto`s the trailer instead of
   returning (`sub_1DA60` :24184 `goto LABEL_31`; the others reach it through
   `sub_1A120`'s plain `return v15`). The port's shared inbox returned before
   dispatch, so no dying creature ever restored. The recording shows it plainly:
   **mc1l5 slot 348 goes `act_life = -1` at t=6241 and is STILL restored to
   `+126 = 20`, type 201, filter 255 at t=6242.**
3. **The shared chase's target-lost test is `+12 < 0 || (+17 & 4)`** (:21656) —
   dead OR **destroy-flagged**. The port tested `class64 == 0` and missed the
   0x400 half, so a chaser whose target was blown up kept chasing the corpse
   forever and never reached its trailer.

Two riders the audit surfaced, both landed because the trailers are useless
without them: m4's chase breaks to `base+1` (25), not `base` — `sub_1A120`'s own
`a2 + 1` (:21657/:21661), so state 24 is the shared idle + arm (`sub_1B5A0`),
not a synthetic "disarm slot"; and m9's chase keeps the CASTLE-extent widening
on its drop-out radius (`sub_1DA60` :24201-02, `+80 + v_28` — the same radius
the mound's castle hunt acquires on at :23770), without which a mound acquired a
castle its own drop-out test then rejected and flapped chase/hidden every `v_26`.

### MEASURED — mc1l5, 23,679 pairs, `want` = RETAIL

Creature (`class 5`) `speed` deviation rows: **722 → 148, −79%.**

| model | before | after | what closed |
|---|---|---|---|
| m2 bee | 12 | 2 | the lunge/recoil rows were the DEATH-tick restore, not a chase phase bug |
| m4 militia | 83 | 7 | the missing disarm + the one-tick-late arm |
| m7 | 23 | 1 | `sub_1C960`, including the 3 rows of `port 23 vs retail 20` = one `+130` step of pack catch-up left standing |
| m9 mound | 600 | 136 | entry/exit trailers + the castle-extent drop-out |
| m15 guard | 1 | 0 | death tick |

Whole-take: conforming pairs **1427 → 1436**, missing-in-port entities **3828 →
3376**. The other three MC1 takes all improved or held (mc1l0 −6 field rows,
mc1l32 −1190, mc1hwl0 +2 conforming / −225 field rows); all 10 fixture suites
stay green with 0 regressions and 0 drifts.

⚠ **ONE HONEST REGRESSION, and it is a pre-existing bug now UNMASKED.** mc1l5's
class-9 model-13 (bolt) field rows rise ~1600 because the mound now *stays* in
its chase and fires the bolts retail fires, at slots that desync.

### ⚖ CORRECTED 2026-08-11 — THE RESIDUAL IS THE CASTLE, NOT THE MOUND

The first reading of the 94 residual m9 `speed` rows blamed m9's state-55 wizard
scan, off ONE sampled row (t=4804, `chase: retail 0 port 650`). **That sample
was unrepresentative and the reading was wrong** — it is one of only 3 rows in
the group carrying `rule = pose-phase`, i.e. row-clean under the other
`--pin-pose` sample. Checking the `rule` column before generalising would have
caught it; a residual set must be characterised across its clusters, never off
its first row.

The real story: the 94 rows sit in **7 tick clusters, and 5 of the 6 largest are
one castle dying** (slot 312). Retail kills it; the port does not, so every mound
besieging it keeps a live target, never breaks the chase, and never runs the exit
trailer. The SAME cause produces the extra bolts — the mounds keep shooting a
castle retail already destroyed, and the extras land in exactly the t≈4000-8000
window the castle is contested in.

| t | unrestored m9 rows | castle 312 `life` retail / port |
|---|---|---|
| 6335 | 28 | −400 / 39600 |
| 6518 | 16 | −400 / 39600 |
| 5966 | 12 | −750 / 19250 |
| 5757 | 12 | −350 / 39650 |
| 6874 | 11 | −400 / 39600 |
| 7321 | 8 | −800 / 59200 |

Two crisp constants sit under it, both with exact witnesses in the take:
- **A steady 400-damage delta.** Across the siege the two sides track each other
  and the port is consistently 400 LOWER — 38 of the 46 in-siege `life` rows are
  exactly +400 (retail − port), 3 more are +800 (two of them). The port applies
  one 400-damage event retail does not. `mob_death`'s own corpse path is a
  400-damage fire, and *"village-corpse damage guard"* is already an open item
  in the village-churn lane — the obvious first suspect.
- **The castle life POOL is 2x off**: `mana_max` retail 20000 / port 10000 for
  slot 312 (and the same 20000/10000 on a class-12 model-16). That is what makes
  the death/rebuild boundary diverge by ~20000 instead of by 400, and it is why
  the port's castle refills where retail's dies. No DEVIATIONS entry rules this
  (the castle entries there are about COST and the latch bug), so it is fair
  game.

⭐ **NEXT DIG in this lane = castle 312's life pool and the 400-damage delta,
NOT m9's wizard scan.** The m9 acquisition question is unresolved but accounts
for at most 3 pose-phase rows here and should not be prioritised off this take.

---

## ✅ FOLLOW-UP 2026-08-11 — THE CASTLE DIES A TICK LATE, AND THE MOUND'S UNGATED PROLOGUE

Both fell out of the table above, and neither was what the two constants looked
like from a distance.

### 1. THE CASTLE DEATH IS A **TWO-TICK** SEQUENCE (`engine/features.rs`)
Lethal damage does NOT downgrade the castle on the tick it lands.
`sub_47EC0` returning 2 only parks it: :56003 does `+70 = 6` and **nothing
else**, so the castle sits at its NEGATIVE life for that whole tick, and the
leveler `sub_470E0` (:56138) runs at the top of the next dispatch — downgrade,
`+70 = 4`, eject, repaint, `+50 = 5`. Retail is directly observable doing it —
mc1l5 slot 312: `act_life` 450 at t=5757, **−350 at t=5758**, 39650 (level 3 →
2) at t=5759. The port called `castle_downgrade` inline from the ch0 intake, so
its castle skipped the negative tick entirely and was always a step ahead on the
ladder. MC2's castle already modelled this on the same field (`mc2::castle`,
actions 4/5/6); MC1's `f59` sub-state machine is the action-4 body, and MC1's
own ctor was already writing `tick70 = 5`, so the field was there and unread.

⭐ **The "castle life pool is 2x off" reading was WRONG, and fixing the
deferral proved it.** The `mana_max` retail 20000 / port 10000 rows were the
port's ladder standing one level ahead, not a bad table — `CASTLE_HP` and
`CASTLE_CAP` are correct as written (they match retail's `sub_47BD0(a1, _, max,
cap)` pairs at :56586-601). Every one of those rows vanished with the deferral,
and no table was touched. **Measure the mechanism before "fixing" the
constant.**

### 2. THE LURKING MOUND HAS NO CLASS GATE ON ITS ATTACKER (`mc1/mobs.rs`)
`sub_1D060` :23732-38 and its buried twin `sub_1D6D0` :24004-07 both do a bare
`+146 = +40; state 0x38` — no `class == 3` test, where everything sharing
`sub_19B10`/`sub_1A120` has one, **and m9's own CHASE prologue (:24177-79) keeps
it.** So a mound that is still hiding turns on ANY attacker, a militiaman
included; a mound already chasing does not. mc1l5 t=4655 slot 819 is the
witness: 250 damage from the class-5 model-4 in slot 776, and retail retaliates
straight onto it. The port's shared inbox gated every model on
`attacker_is_wizard`, so our mounds simply absorbed it.

### MEASURED (mc1l5, cumulative with the trailer work above)

| | HEAD | trailers | + castle | + mound |
|---|---|---|---|---|
| creature `speed` rows | 722 | 148 | 86 | **70** |
| conforming pairs | 1427 | 1436 | 1436 | **1437** |
| castle-312 `life` rows | 55 | 55 | 43 | **43** |
| `mana_max` rows | 410 | — | 368 | **368** |
| castle-ownership rows (`rival/player/wizard0.castle`) | 12 | 12 | 2 | **2** |

Other takes vs HEAD: mc1l0 conforming 5168 → 5171, field 4292 → 4163; mc1l32
field 1887744 → 1886446; mc1hwl0 conforming 10494 → 10496, field 199637 →
198892. 484 unit tests and all 10 fixture suites green, 0 regressions/drifts.

### THE 400-DELTA IS A **PHASE**, NOT A MISSING GUARD
All 43 surviving castle-`life` rows are the same shape — 40 at exactly +400, 3
at +800. Retail takes 400-damage hits on its own clock (slot 312: 30850 at
t=5525, 30450 at t=5527, 30050 at t=5530 — one every ~3 ticks), and 400 is the
**m9 bolt's damage** (`sub_1AA40` :21935, 400 without body segments). So this is
not an extra damage source and not the village-corpse guard: it is the mound
bolts landing on neighbouring ticks, i.e. the same (9,13) slot/cadence desync as
the bolt lane below. Roughly a quarter of the siege's ~76 hits are one tick out.
⚠ **The "one 400-damage event the port applies and retail does not" reading in
the table above is therefore superseded — it is a timing spread, not a missing
guard.**

## ✅ THE BOLT LANE, CLOSED 2026-08-11 — TWO MORE m9 LAWS

Chasing "why do the mound's bolts land on the wrong tick" found nothing about
timing and two things about the bolts themselves. Both were found by dumping
retail's own (9,13) entities out of the take rather than by reading more
decompile: at t=5727 ten mound bolts spawn in one volley, every one of them
carrying `f30` = a real bearing on the castle, `f126 = f128 = 384`, and
**`f66 = 3, f67 = 2`**.

### 3. EVERY THUNK STAMPS THE **SHOOTER'S** FILTER PAIR (`mc1/mobs.rs`)
`sub_1A8E0` :21895-98, `sub_1A990` :21952-55, `sub_1AB70` :22005-06, `sub_1AE30`
:22122-25, `sub_1AA40` :21951-52 and m15's :25857-58 all write `+66/+67 =
a1x->+66/+67`. Only m8's `sub_1AEE0` :22155-60 takes the TARGET's, and m11's
`sub_1E380` writes none at all. The port hardcoded `(3, 0xFF)` in every arm.
For most creatures that IS the shooter's pair — the ctor sets `+66 = 3`, NewEvent
defaults `+67 = 0xFF` — but **m4 and m9 NARROW the pair to their target's
class/model on the chase-entry trailer** (`sub_1BC50` / `sub_1DCD0`), and the
narrowed filter rides their shots. `filter_admits` tests the human as (3, 0), so
the hardcoded wild card let a castle-aimed mound bolt collide with the wizard
flying past it, with a rival carpet, and with a mana balloon — none of which
retail's (3, 2) bolt can touch. ⭐ This also closes the OPEN boulder-filter note
that sat on the m7 thunk.

### 4. THE MOUND RE-BEARS ON A **DECIMAL** PERIOD (`mc1/mobs.rs`)
`sub_1DA60` :24197 is `+63 % 10`, not the shared chase's `(+63 & 3) == 0`
(:21654). m9 drives its own chase in retail, so routing it through the shared one
gave our rooted mounds a 4-tick swing where retail's take 10.

### MEASURED — the bolt lane went from a COST to a NET WIN

| mc1l5 | HEAD | after trailers+castle+mound | + filter | + re-bear |
|---|---|---|---|---|
| conforming pairs | 1427 | 1437 | 1437 | **1449** |
| total diff rows | 235335 | 236579 | 234603 | **233833** |
| (9,13) `field` rows | 33354 | 34967 | 32688 | **32688** |
| m9 `heading`+`target_yaw` | 855 | 855 | 855 | **86** |
| creature `speed` rows | 722 | 70 | 70 | **70** |

Every column the bolt lane had pushed up is now paid for: against HEAD, `smodel`
−1239, `sclass` −921, `speed` −597, `target_yaw` −273, `mana_max` −42, and the
whole take is **1502 rows below baseline**. Other takes: mc1l0 conforming
5168 → 5171, mc1l32 field 1887744 → 1885682, mc1hwl0 conforming 10494 → 10500.
486 unit tests, all 10 fixture suites green, 0 regressions/drifts.

### WHAT IS LEFT, HONESTLY
The 43 castle-`life` rows (40 at +400, 3 at +800) and the 70 creature-`speed`
rows did NOT move on either fix, so the ~25% bolt-arrival spread is neither the
filter nor the re-bear cadence. (9,13) `extra` is still 848 against HEAD's 672 —
the port holds bolts retail has already retired — which sits inside the take's
pre-existing 10.4k extra-entity population, a whole-pool lifetime question rather
than an m9 one. ⭐ **NEXT, if this lane reopens: projectile LIFETIME/retirement,
not m9.** Fixtures: `a_mounds_castle_bolt_carries_the_castles_filter_not_the_wild_card`,
`a_rooted_mound_re_bears_every_tenth_tick_not_every_fourth` (both non-vacuous).

### ⛔ STILL RULED — DO NOT RE-OPEN
The `+=` (empirically ruled out by the recording, see above) and any `.min(+128)`
clamp at the catch-up: retail carries m2 `+126` = 95 against `+128` = 70 for 62
creature-ticks in this take alone, so a clamp breaks conforming rows. Both are
pinned by `pack_catch_up_is_the_set_form_and_stays_uncapped`, which passes both
BEFORE and AFTER this work by design — it is the regression guard, not a fix
witness.

### FIXTURES
`crates/mgc-sim/src/engine/features.rs` (test module), seven tests; six fail
against the pre-fix `mobs.rs` and pass after (non-vacuity checked by swapping
the file), the seventh is the ⛔ guard above:
`m7_plants_on_the_hit_and_restores_on_the_timer`,
`m7_chase_exit_restores_a_pack_inflated_speed`,
`pack_catch_up_is_the_set_form_and_stays_uncapped`,
`militia_arms_on_promotion_and_restores_its_walk_speed_on_exit`,
`mound_enters_the_chase_rooted_and_restores_on_exit`,
`chase_exit_trailers_run_on_the_death_tick`,
`kraken_pack_tick_ends_at_its_pinned_speed`.

### STILL OPEN FROM THE ORIGINAL PLAN
- The `f52` overload (pack link vs multipart segment link) is **not** settled;
  it still gates any pack-CHURN analysis. It did not gate this work, which is
  about restores rather than churn.
- `mc1/mobs.rs`'s damage-inbox PACK arm still clears only the FOLLOWER's `+52`;
  retail clears BOTH (:21758 + :21762), so the port forms fewer packs there.
- The VR fork's `awake_range = 80` / `pool_slots = 5000` overrides
  (`vr/master:crates/mgc-app/src/lib.rs:8653-66`) remain a real rate amplifier
  and worth fixing as hygiene, but they are not the cause.
- m7's wander slot `sub_1C900` (:23300-10) carries a per-`v_26` LIFE restore
  the port does not have; the decompile of it is mangled (`v1 = maxLife >> 6 >
  maxLife` is always 0, leaving a literal FULL heal) and the HW twin is
  identical, so it needs a ruling before porting. Speed lane only here.
- m8's `sub_1CE30` fires sound 38 and refreshes the victim's `+528` on the
  connecting attack (:23555-60); the port has the cadence screech but not the
  attack-gated pair. Now trivially reachable — `mob_chase` returns the thunk
  verdict.

# THE MC2 BUILDING-LIFE FIELD HOME (2026-08-12) — ✅ LANDED

The top open item banked by the mc2l1 intake, and the other half of the
`sub_49A30` field-home audit that the round-2 report **A** (the degradation
link → `Ent::f46`) started. Same ctor, two lines apart.

## THE LAW (decompile, verified line by line)
`sub_49A30` EF:32793-32808 writes the building's two production words off the
SAME table row:

```
a1x->subSpellIndex_0x2A_42 = str_D93C0_bldgprmbuffer[a2].word_0;   // EF:32793
a1x->mana_0x90_144         = 0;                                    // EF:32796
if (!(str_D93C0_bldgprmbuffer[a2].byte_2 & 8)) {
    a1x->byte_0x38_56 |= 2u;
    a1x->mana_0x90_144 = 1000 * a1x->subSpellIndex_0x2A_42 >> 7;   // EF:32808
}
```

and the construction finish (action 51 → 52) parks the LIFE off the first of
them: `event->life_0x8 = 1000 * event->subSpellIndex_0x2A_42;` (EF:27291).
`maxMana_0x8C_140` is never written on a building at all.

## THE DEFECT
`Gen::mc2_spawn_building` parked the RATE in `f140` (the port's
`mana_0x90_144` home — see the mc2/mobs.rs module header alias table) and the
derived mana in `f136` (`maxMana_0x8C_140`, dead on a building), and the finish
read `1000 * f140`. In FRESH PLAY that is the right number by pure coincidence,
because the port itself had written the rate into the field it then read back.

The two words are **independent on the wire**, and a conformance import
restores them independently — the uniform alias map already carries @0x2A →
`f44` and @0x90 → `f140` faithfully (`import_ent_mc2`, no (10,45) arm needed).
So a replayed building's finish read a mana word it had never authored:
**mc2l1 t=888 slot 161, retail life 190,000 vs port 0** (that building's retail
mana was 0). The MC1 column had it right all along — `spawn_creator`'s model-45
arm seeds `f44 = 100` and `tick_building`'s finish parks `act_life = f44`; MC2
was the outlier.

## THE FIX (4 sites, one lane)
- `mc2_spawn_building`: `f44 = bldgprm.rate`, `f140 = 0` then
  `f140 = (1000 * rate) >> 7` on the productive kind; the `f136` writes are
  gone (retail leaves that word alone).
- `mc2_building_tick`'s finish: `act_life = 1000 * f44`.
- `World::live_poses_mc2`: the parked-dwelling health-bar denominator reads
  `f44` (it now matches `live_poses_mc1`, which already denominated on `f44`).

No importer change and no pad-reconstruct whitelist change: `f44` is not
touched by the replayed construction, and `act_life` is restored from the
import afterward either way.

## MEASURED
- **10/10 fixture suites, 0 regressions, 0 drifts — and 2 FIXED**: mc2l0
  t=3318 and mc2l30 t=21, whose open atom was in both cases exactly
  `field:10,45:life`. Promoted in this change.
- mc2l1 whole take: rows 261,255 → 261,169 (−86); the **(10,45) `life`**
  family 204 → 147 rows and **(10,45) `mana`** 4 → 1. The t=888 slot-161
  exemplar is gone. Residual on that family is the ±160 one-tick damage
  phase on slot 161 (107 of the 147) — the known one-frame capture class,
  not this lane.
- 721 workspace tests green under `MGC_REQUIRE_GOLDENS=1`, fmt + clippy clean.

## GOLDENS
`mc2_cave` (all four) and `mc2_slice` (all six, post-init included) re-pinned:
`f44`/`f136`/`f140` are hashed and every authored building moves the stream
from t=0. ⭐ **Both layout-INDEPENDENT companion goldens HELD** — which is the
proof that this is pure bookkeeping in fresh play, exactly as the law predicts:
the parked life is unchanged there, and only an import can make the two words
disagree.

## FIXTURE
`a_buildings_life_is_a_thousand_times_its_rate_not_its_mana` (world.rs), both
halves — the ctor's three field homes, and a finish whose two words DISAGREE
(the import shape). Non-vacuity checked by restoring the old two lines in
place: it fails on the first assert (`f44` reads `new_event`'s default 100,
which is precisely why nothing noticed).

## ⭐ CONFIRMED IN PASSING (no defect)
MC2's parked-house tick is `AddHouse0A_2D_38330` (EF:27959) and is correctly
split from MC1's `sub_28DC0` (:30767) in the port — `world.rs` dispatches
`mc2_house_tick` for MC2 before the shared `tick_building_live` arm. The MC1
function's `%40` gate carries `+140 = occupants << 8` (:30819); MC2's `& 0x1F`
gate carries **no mana write at all**, so the banked worry that
`tick_building_live` would clobber the relocated mana word every 40 ticks does
not apply to MC2. `mc2_house_tick`'s own APPROX register (the mana-sphere
production roll EF:28040-58 and `SetMaxDistance_5C8D0`) is unchanged and still
open on the economy track.

# THE HELD-BACK AREA FIXES (2026-08-12) — ⚖ ONE LANDED, TWO HELD BACK WITH RECEIPTS

The mc2l1 intake banked three "held-back area fixes, land AFTER re-measuring
mc2l1". Re-measured, and the bank was **partly wrong about what retail does**.
Reading all six broadcast/probe variants across both decompiles settled it.

## THE FULL GEOMETRY, MEASURED OFF THE DECOMPILE
| | window centre | radius | shape |
|---|---|---|---|
| MC1 `sub_120B0` ch≥1 (:17260-72) | `(pos + 128) >> 8` | `(+80 + 255) >> 8` | square ±r |
| MC1 `sub_120B0` ch0 (:17339-52) | **`(pos − 128) / 256`** | same | square ±r |
| MC1 `sub_124F0` ch0 (:17427-39) | **`(pos − 128) / 256`** | same | square ±r |
| MC1 `sub_127E0` ch0 (:17535-47) | **`(pos − 128) / 256`** | same | square ±r |
| MC2 `sub_10C80` ch0 (EF:4118-20) | `(pos + 128) >> 8` | same | square ±r |
| MC1 `sub_11980` / MC2 `sub_10780`, `sub_108B0` | `(pos + 128) >> 8` | same | **SEARCH.DAT ring walk** |

Two things the bank had backwards. The radius is **not** `f80 >> 8` — it is
`(f80 + 255) >> 8` everywhere, which the port already had. And the `−128` ch0
bias is **MC1-only**: MC2's ch0 arm rounds like every other channel. Every
`__CFSHL__` / `my_sign32` fixup wrapped around these expressions is DEAD code —
`axis_3d::x` and the extent are both `uint16`, so the sums never go negative.
(`my_sign32` is a −1/0 indicator, not a signum; the idiom is signed division by
256, i.e. truncation, which only bites in the first half-tile of the map.)

## ✅ LANDED — MC1's CHANNEL-0 WINDOW BIAS
`area_write`'s centre is now per-channel and per-game: `(pos − 128) / 256` on
MC1 channel 0, `(pos + 128) >> 8` everywhere else. Every MC1 area DAMAGE window
had been sitting exactly one tile +x and +y of retail's (`(x+128)>>8` and
`(x−128)>>8` differ by exactly 1 for every x).

⭐ This is the other half of the AREA-BROADCAST TILE ROUNDING fix: that one
generalised the ch1 rounding — corpus-pinned on the mc1l0 t=91 tent CLAIM,
a **channel-1** write — to channel 0, where it does not belong.

**MEASURED:** mc1l0 whole take **5,171 → 5,174 conforming pairs**, unexplained
field rows **4,163 → 4,090** (−73), missing 72 → 71; fixture mc1l0 **t=4177
(`field:5,3:life`) promoted**; 10/10 suites, 0 regressions. Goldens:
`state_hash` level-005 D/E only, and **OBSERVABLE moved with them, correctly** —
which bees an explosion's ch0 mail reaches is precisely what the window decides.
Post-init/A-C hold, so the shift is confined to the ch0 damage windows.
Fixture: `the_mc1_channel_zero_area_window_sits_one_tile_back` (three ways —
MC1 reaches back and not forward, MC2 the mirror image, MC1 ch1 still rounds).

## ⏸ HELD BACK #1 — THE `.max(1)` RADIUS FLOOR
Retail has no floor: a zero-extent writer runs `for i = -0; i <= 0` and scans
its OWN TILE, where the port's `.max(1)` hands it a 3x3. **Removing it buys
NOTHING** — mc1l0 whole take is identical to the pair with it in or out — and it
BREAKS `mc2_arrow_direct_hits_the_first_filter_matching_creature`. Comment left
at the site.

## ⏸ HELD BACK #2 — THE PROBE RING GEOMETRY (and it is the bigger prize)
`victim_scan` (`sub_11980` :16999-17001) and `claim_victim_scan` (`sub_108B0`
EF:3798-3802) both walk `sub_11410(0, r)` / `AddE7EE0x_10080(0, r)` — the
SEARCH.DAT ring iterator, over a rounded centre — exactly like the already-ported
`sub_11AC0` sibling beside them. The port walks a truncated-centre square with
the `.max(1)` floor. Ported both, then held them back:

- ⭐ **the corpus WANTS them**: on top of the ch0 fix, mc1l0 goes a further
  **5,174 → 5,180 conforming and 4,090 → 3,976 unexplained rows** (−114);
  mc2l4 +1 and mc2l30 +2 conforming with one fewer spurious (10,12) claim pulse.
- ⛔ but they cost **five pinned unit fixtures** (the fools-trap muzzle, the
  meteor homing lock, the arrow's collateral direct hit, two muzzle-admission
  guards) and mc2l4 t=621.

**WHY, and it is not a decompile error.** Measured the real ring table
(`baked/assets/*/search.bin`): ring 0 is the 2x2 block {(0,0),(1,0),(0,1),(1,1)}
and the iterator DROPS the last cell of the last ring, so `r == 0` probes three
forward-biased cells — while `AddEventToMap_57D70` (EF:40315-20) chains every
entity at plain `x >> 8`. Retail's probe window is therefore genuinely
forward-biased and narrow, and retail gets away with it because it probes ONCE,
at the end of the move. **The port does not**: it ray-marches the chord in
≤128-unit sub-steps (a documented anti-tunnel deviation invented because several
projectile sprites carry a zero-width box). Instrumented the arrow: f80 = 0,
384 units per tick, ONE probe per tick — that path never engages the march at
all, so the tight window simply drops the hit.

⇒ The port's window inflation and its chord march are ONE compensating family
and have to come out together. The dig this wants is the **projectile probe
cadence** — which paths march, at what sub-step, and what each carries for f80 —
and only then the three geometry arms in one patch. Filed as the next area-lane
dig; the two held-back arms are the payoff (~+120 rows on mc1l0 alone).

## METHOD NOTE
Every arm was attributed by BISECTING the patch against the whole take, not the
sparse suites — and it mattered twice. The suites credited the ch0 fix with the
one mc1l0 fixture but hid its whole-take +3; and they reported the probe lane as
"1 regression" when the whole take says +9. Conversely the whole take is what
proved the `.max(1)` removal inert. **Sparse fixtures rank; whole takes decide.**

# THE ONE-SHOT ACQUISITION LATCH (2026-08-12) — ✅ LANDED, AND IT IS THE WHOLE PROJECTILE COLUMN

Opened as a narrow player-report verification — "MC1's meteor curves oddly; is
that retail, and does it depend on awake range?" — and the verification found a
defect four flight functions wide.

## THE VERDICT ON THE REPORT (both halves)
**The long curve is FAITHFUL.** `sub_52550_52890` (:62534) is the post-lock
tracker and it runs every tick the bolt has a target: recompute the bearing to
the target's LIVE position, turn yaw and pitch toward it at the behaviour row's
rates. **No range cap, no lifetime cutoff, no line-of-sight test, no
re-validation of anything.** MC1's meteor is speed 384 / life 21 ≈ 31 tiles of
flight, so most of that arc historically happened past the fog veil — which is
exactly why it reads as wrong now.

**It is NOT independent of awake — but awake is sampled ONCE.** The acquire
switch `sub_54520` case 0/3/4 (:63979) scans two lists: the creature buckets,
where a candidate must have the awake counter `+58` nonzero (:63996), and the
wizard/castle list, gated instead on a distance from the caster's own row value
(`+156 → +28`) plus the not-cloaked bit. Awake decides WHICH CREATURE MAY BE
PICKED, at one instant. It does not gate the homing: once locked, the bolt
tracks its victim for the rest of the flight however far it goes, asleep or not.

## THE DEFECT — RETAIL COMMITS AT THE MUZZLE, WE HUNTED FOR THE WHOLE FLIGHT
Both flight prologues wrap the acquire in a ONE-SHOT LATCH on flags bit 2, set
win or lose:

```
if (no target) {
    if ((+16 & 2) == 0) {
        +16 |= 2;
        if (sub_54520(self)) { commit the heading to the pick }   // HIT
        else                 { +34 = +30; +36 = +32; }            // MISS
    }
} else  sub_52550(self, target);                                  // the tracker
```

`sub_52770` :62640-60 (generic) and `sub_52B30` :62811-15 (fireball) — identical
shape. The commit differs: the generic path SNAPS (`+30 = +34; +32 = +36`), the
fireball turns yaw by AT MOST 34 toward the pick and takes pitch outright
(:62817-24). Note the tracker is the **else** arm: the tick that acquires
commits and stops, easing starts the tick after.

The port re-ran the scan EVERY untargeted tick and never committed. Consequences,
all matching the report: a bolt that missed its cone at launch kept hunting for
its whole life and bent onto anything that wandered in; a creature that merely
WOKE UP mid-flight became targetable mid-flight; and without the commit the first
turn was a long lazy ease from the launch heading instead of a re-point. (Also:
`home()` clears `f146` on target death, so with no latch a bereaved bolt
immediately re-acquired — retail's tracker has no liveness check and cannot.)

## ⚠ ROOT CAUSE — THE SAME LESSON, THIRD TIME
Retail's `sub_52770` is ONE function; the port split it across
`proj_generic_tick`, `proj_firewall_tick` and `proj_payload_tick`. The latch got
ported into the `proj_firewall_tick` half ALONE, where a comment asserted it was
Hidden Worlds' own — **it is not, it is in base remc1 at :62640**. Only the SCAN
is HW's (base MC1's `sub_54520` has no case 16, so the bolt takes the miss arm,
which still sets the bit and still mirrors). **The port's function boundaries are
not retail's** — cf. the `mc2_castle_extents_ent` refutation and the building
degradation link.

## FIXED IN FOUR PLACES (one law each)
- `proj_generic_tick` — m3 meteor, m14 boulder (restructured to retail's literal
  if/else so the acquire tick cannot also ease).
- `proj_m0_tick` — **the fireball**, and this is where the corpus payoff is.
- `proj_payload_tick` — m4 volcano, m7 duel, m11; m2/m5/m17 take the miss arm.
- `proj_firewall_tick` — latch un-gated from HW, scan left HW-only.
Already correct and untouched: the possess lob (:62952-60), the m9 beam, the
castle ball.

## MEASURED — THE LARGEST CORPUS MOVE OF THE SESSION
| take | conforming pairs | unexplained field rows |
|---|---|---|
| mc1l0 | 5,174 → **5,319** (+145) | 4,090 → **3,344** (−746) |
| mc1l32 | 3,635 → **4,122** (+487) | 1,885,831 → **1,879,802** (−6,029) |
| mc1l5 | 1,449 → **1,455** (+6) | 215,043 → **210,226** (−4,817) |
| mc1hwl0 | 10,500 → **10,589** (+89) | 197,763 → **196,011** (−1,752) |

**+727 conforming pairs, −13,344 unexplained rows.** 10/10 suites, 0
regressions; three fixtures FIXED and promoted (mc1l0 t=2696, mc1l32-bee-height
t=981 and t=1392). 723 tests, fmt + clippy clean. `state_hash` level-005 D/E
re-pinned with OBSERVABLE — correct, D is literally "64 ticks of two-hand
fireball combat". MC2 untouched (its projectile column is its own).
Fixture: `a_projectile_acquires_once_at_the_muzzle_and_a_miss_never_re_acquires`
(both halves; non-vacuity checked by disabling the fireball latch in place).

⭐ Attribution note: the meteor arm ALONE was worth −30 rows on mc1l32 and
nothing anywhere else — measured, before the fireball arm was written. Almost
the entire +727 is the fireball. Had the dig stopped at the reported symptom it
would have banked a rounding error and missed the column.

## 🏆 THE MC1 MAILBOX RESIDUE LAW — two write protocols, not one (LANDED 2026-08-12)

**MC1 has TWO damage-mail write protocols and the port ran one
everywhere.** The area writers (`sub_120B0` / `sub_124F0` / `sub_127E0`,
:17466-70) accumulate while a source is pending and overwrite a stale
amount. `sub_12B50` (:17604-07) — the SINGLE-target writer — is the exact
INVERSE: it OVERWRITES while a source is pending and ACCUMULATES onto the
stale amount once a reader has cleared the source. MC2's `EF:4022-25` is
area-order, which is what the port implemented for both games.

`sub_12B50` has **exactly two callers in the binary**: the creature melee
thunk `sub_1AB10` (:21970, damage = the attacker's `+44`, 3D range < 1024)
and the death field's class-3 arm (:31296).

Every reader clears the **SOURCE and never the amount** — MC1 player
:55734, MC1 pool inbox :21337, MC2 player `sub_5EFA0` EF:60725 — and
**both games leave the source armed on a FATAL hit** (the clear sits past
the early return; the dead wizard's next pass bails at :55643). So a
consumed mailbox keeps its last amount as residue, and under `sub_12B50`
MC1 point damage SNOWBALLS onto the previous hit.

Also landed: the shield writes its QUARTERED value back into +90
(:55704 / EF:60684), and the port's unconditional 6-channel wipe at the
end of the player consumer is DELETED — no retail path does it. The only
full clears are the spawn-grace memset (:55367-71) and the death landing
(:55485-569).
⚠ The at-castle ch0 redirect (:55357-60) open-codes the AREA order — it
must NOT route through the single writer.

**Pinned by mc1l0**: t=3230, one 100-damage vulture melee onto a 400
residue costs the player exactly **500**; t=3235, the 500 left behind
makes the next cost **600**. The opposite branch is pinned by t=565-570
(the castle tanking worm fire via the redirect), where the source stays
pending across four writes and the amounts record 1200/800/1200/400 with
**no compounding at all**.

**Receipts**: mc1l0 unexplained field rows 3,344 → **3,340** (life 26→24,
player.life 3→1 — exactly the four targeted); conforming pairs unchanged
at 5,319; **mc1l5 t=403 promoted**; 0 regressions across all 9 suites
(7,829 fixtures). Re-pinned: `state_hash` L005 D/E (⭐ OBSERVABLE HOLDS —
bookkeeping; L005's D/E is fireball/AREA combat, so nothing there takes
different damage) and `mc2_slice` E (the residue is INERT for MC2
behaviour — area order overwrites it — only the hashed `player_mail` word
moves).

⚠ **A first attempt keyed this per GAME and regressed pool `life` 26 → 331
rows.** The split is per WRITER, not per game. Read the area writer's
open-coded branches before touching the shared one.

⭐ **Attribution note**: the corpus win is FOUR rows. The law is right and
decompile-pinned, but mc1l0 barely exercises melee — mc1l5 gave up a
fixture, so look for the payoff in takes with real creature contact.

## 🏆 THE (10,39) MANA-BALL DIG — ONE FAMILY, FIVE LAWS, −51% OF THE WHOLE TAKE (2026-08-12)

**The banked "castle established-tick ball collection" family (pair
564, 57% of mc1l0's unexplained rows) resolved into FIVE independent
port bugs sharing one entity family — none of them the absorption
machinery the ledger note guessed.** The heading lane and the stamp
lane are SEPARATE laws (the memory's ⚠ CONFIRM FIRST was right to
doubt one dig), and the three big spawn events (t=1363/1830/2217) are
neither corpse drops nor overflow ejections but CASTLE TEARDOWNS.

**THE FIVE LAWS (all decompile-pinned + corpus-proven):**

**1. The ball ctor stamps (sclass/smodel + base speed).** BOTH games'
ctors stamp the source pair and a base speed the port skipped: MC1
`sub_3B5A0` +66/+67 = 10/39, +126 = 32 (:47456-57, :47463); MC2
`CreateManaSphere` xtype/xsubtype + actSpeed = 32 (EF:36614-17).
`spawn_mana_ball` now stamps all three. → the 256-row sclass/smodel
family at 0.

**2. The corpse drop persists its speed draw.** `sub_27690` writes the
launch speed INTO +126 (:29689 `v2[63] = draw % 0x30 + 16`) — the port
computed the same draw, fed it to the velocity, and dropped it; every
corpse/house-preclear ball then read the NewEvent default 16 where
retail reads 16..63. Also the +46 launch lift is a SIGNED /8 (:29692)
— the port's `.max(0)` flattened deaths >4 tiles above ground.
→ the 127-row speed family at 0. (The t=564 lane = the castle
UPGRADE PRE-CLEAR demolishing houses; each drop is sub_27690.)

**3. The ch4 magnet-attract intake writes the HEADING.** Retail's ball
tick stamps `+30 = sub_42150(ball, magnet)` (:29453; the MC2 twin
writes yaw_0x1C at EF:26101) every tick a (10,54) magnet's ch4 mail
lands, so a pulled ball's heading tracks the bearing for the magnet's
128-tick life. The port applied the impulse and never wrote f30.
Stored RAW — retail's atan2 returns 0..2048 INCLUSIVE and +30 keeps
the full-turn 2048 (corpus t=1385/2336). → the 1,279-row heading
family at 8.

**4. MC1 ball claims are SOURCE-only — the force/lock protocol is
MC2's.** Retail MC1's ch1 intake (:29439-48) reads the source, claims
unconditionally on owner change, and never reads the amount; the port
ran MC2's force/lock arm (EF:26069-94) for both games, so retail's
parked nonzero ch1 amounts (imported per pair) set the port-only
claim-lock bit 29 on MC1 balls. → the 48-row flags family
(want 12 got 0x2000000C) at 0. Also the fresh-magnet flags: the
magnet ctor sets +16 |= 1 (:47697) — the port didn't (want 5 got 4).

**5. ⭐ THE CASTLE TEARDOWN WRAPPER — two "retail leak" patches were
WRONG and are RETIRED.** `castle_death_mana` / `castle_death_balloons`
were premised on sub_47A70's `!level` arm never scattering the bank or
touching the fleet. The premise missed the state-6 WRAPPER: `sub_470E0`
(:56147-50) calls `sub_47130` + `sub_47400` AFTER the teardown returns,
so retail ALWAYS re-runs the ejector at the post-downgrade level
(death: f26==0 → the whole bank, :56189-90; survivor: the spill above
the new cap) and re-quotas the fleet (death: level-0 quota culls every
balloon with the cargo drop, :56399-411). CORPUS PROOF, arithmetic-
exact: t=2217 castle slot 107 dies holding 8302 → 8 balls of
8302/8 = 1037, residual 6 = 8302 % 8, 4 magnets, balloon flags 1036;
t=1363 = 3000 → 3×1000 residual 0. Also pinned: the 10% haircut is
RESTORED after ejector #1 (:56513, port kept it — the observed stale
9000 cap), and the death runs the LADDER RESET (sub_47C60 → sub_47BD0
row 0: **cap unconditional = 5000, HP row-gated at 0** — mana_max
9000 → 5000 with NO life rows). `castle_downgrade` now follows the
wrapper flow unconditionally; the two patches are deleted from
patches.rs/config.rs/settings.rs/defaults, DEVIATIONS.md entries
retired-with-refutation. A spilling DOWNGRADE now runs TWO ejector
passes (haircut pass + wrapper pass at the new cap) — the corpus's own
"leftover bank parks exactly at the new cap" shape.

**RECEIPTS (mc1l0 verify-deltas):**
- Unexplained field rows **3,340 → 1,624 (−51%)**; missing 72 → 48
  (exactly the 13 scatter balls + 11 magnets); (10,39) family
  **1,738 → 37**; (10,54) at 0.
- Conforming pairs **5,319 → 5,687 (+368)**; first unexplained pair
  564 → **581**.
- Free-replay horizon HOLDS at 562 — the wall is the pose.z t=563
  mover ground-sample phase (the mid-walk restructure lane), not this
  family.
- **6 fixtures FIXED + promoted** across mc1l0 (t=1364 + 1), mc1l5
  (t=1390 + 1), mc1hwl0 (t=1698), mc1l32; 2 drift re-annotations
  (t=156/230 LOST their (10,39) flags diff — the claim-lock fix; l32
  t=39509 gained knock-on stamp rows inside a pre-existing
  model-mismatched slot). All 9 suites green, 0 regressions;
  workspace 724 tests green; clippy + fmt clean.
- Goldens re-pinned with attribution: mc2_cave (4), sim_state_hash
  FAITHFUL+ENHANCED (stamps ride from load in level 001), state_hash
  L005 A-F (authored balls + combat-window corpse drops). ⭐ EVERY
  OBSERVABLE COMPANION HELD — the stamp lanes are motion-inert
  bookkeeping in those windows. New pin:
  `castle_death_scatters_the_whole_bank_arithmetic_exact` (the 8302
  law end to end); `castle_downgrade_ejects_mana_and_demolish_razes`
  re-pinned for the two-pass eject (8 magnets, bank parks at cap).

**REMAINING (10,39) RESIDUE (37 rows, small laws):** x/y 28 rows
(sub-tile deltas at pull/teleport ticks — polar/rounding lane, undug);
heading 8 (same-tick ch4 phase on freshly-scattered balls at the
spawn boundary + double-magnet last-writer at t=1869); mana 1
(t=2371, rides the one slot-swap pair). The (9,0)/(9,1) projectile
families (1,135 rows) are now the take's top block.

# THE PROJECTILE LEDGER + BLIND TRACKER (2026-08-12 evening) — ✅ FIVE LAWS LANDED, THE (9,0)/(9,1) BLOCK −71%

**The banked (9,0)/(9,1) block (1,135 rows, mc1l0's top family) resolved
into five independent laws. mc1l0: conforming pairs 5,687 → 5,887,
unexplained 1,624 → 798, the block itself 1,135 → 324 ((9,1) 30, (9,0)
294). Twelve fixtures promoted across five suites, 0 regressions;
mc1l32 +22 pairs / −10.7k rows, mc1l5 +76 / −23.6k, mc1hwl0 +584 /
−15.3k. First unexplained pair 581 → 586. Player-reported the free
replay converged further on this change (with one new symptom, banked
below).**

## 1. THE LEDGER SWEEP `sub_16540` (:19643) — flags 0x2000, hate at acquisition time

Runs once per tick between the per-class list rebuild and the mana
census (:52326). Every class-9 record with a class-3 owner (+24) and a
held victim (+146) is ledgered ONCE — `flags |= 0x2000` (:19678), win
or lose on the tables below — and re-examined every tick until it has
both (a rebound can hand a miss a victim mid-flight). The mark alone
was mc1l0's 202-row `flags want 8198 got 6` family. The tables:
victim's wizard gains hate vs the shooter keyed on the PROJECTILE
model ({3,4,11,16} → +3000 carpet/balloon, +5000 castle; model 10 →
nothing; else +500/+1000); the CASTLE arm alone runs the war check at
`50000 − shooter_wealth/10 × victim_agg/255`; a possess lob (m1)
locked onto a claimed (10,39) ball bumps the claimant by ball_mana/4.
The port's intake-time hate feed (the flagged interim in rivals.rs) is
DELETED — it was this scan's approximation.

⚠ TWO remc1 TRANSCRIPTION SLIPS, arbitrated by the MC2 twin
`sub_159E0` (EF:7320, called EF:786): the carpet-arm base-read is the
VICTIM-owner's table (remc1's text reads `v2->id24` — the shooter's),
and BOTH arms key on the projectile MODEL (the text reads +63 in the
carpet arm). The twin also pins the war formula's wealth as the
SHOOTER's max mana (the port had folded to the victim's). MC2 is NOT
wired to the sweep — zero class-9 flags signal in all four MC2 takes;
its own frame calls the twin, so a future MC2 lane may want it.

## 2. THE TRACKER IS BLIND — `sub_52550` (:62543-55) never re-validates

Retail steers at whatever `pool[164 * +146]` holds: corpses awaiting
the reaper, even slots recycled into different entities (mc1l0
t=1818-30: two lobs track slots that are live PROJECTILES by then).
The port cleared +146 on a dead/empty slot in BOTH `home()` and the
possess homer — the 133-pair `chase → 0` family, 56% of the block's
rows. Both clears deleted; only an out-of-range guard stays.

## 3. THE LOB ROWS — `home_possess` deleted, ctor rows off the decompile

The possess lob (m1) and magnet bolt (m17) ctors sit on row [2] — yaw
AND pitch caps 113/113 (:45908/:46375-76) — and BOTH take the short
fuse `4096/speed = 10` (the port had m17 at 21). The payload lobs
m2/m4/m5/m7/m11 sit on row [1] (:45941..:46220). The port handed every
spell lob row 0 AND tracked possession through a hardcoded homer (34
yaw cap, pitch snapped outright) — the (9,1) ±79 = 113−34 heading
staircase. `home_possess` is deleted; the lob tracks through the
shared row-capped `home()`. (The per-pair lane always seeded the RIGHT
row from the recorded +156 pointer — the ctor rows only bit in native
play and free replay; the homer bit everywhere.)

## 4. THE TERRAIN ARM POSITION LAW — fireball reverts, generic doesn't

On terrain impact the FIREBALL (:62899-908) reverts to its PRE-STEP
position (`sub_41C70(a1, &v21)`) — its water test, (10,5) splash and
detonation all happen at the point it flew FROM; the GENERIC
(:62680-701) keeps the STEPPED position for all three. Both exempt
model 4 (the volcano lob) from the splash — over water it detonates
like on land. The port had folded both to "pre-move" with the water
test at the stepped point.

## 5. THE REBOUND HEADING IS STORED RAW (:62740/:62877)

`+30 = draw % mod + reversed_yaw − half` with NO mask — a draw below
`half` off a near-zero reversed yaw parks a NEGATIVE u16 in +30 for a
tick (corpus t=2739: want 65512 = −24; the port's masked 2024 is the
same angle). Every consumer masks on read (`& 0x7FF` ≡ retail's
`HIBYTE &= 7`); the next homing write canonicalizes.

## GOLDENS + FIXTURES

`state_hash` L005 D/E re-pinned — ⭐ OBSERVABLE HELD BYTE-FOR-BYTE:
in that slice the hash moves on the hashed ledger marks alone (no
D-window bolt loses its victim or grounds on a shore there). New pin:
`the_tracker_is_blind_and_the_ledger_marks_each_bolt_once`. Promoted:
mc1l0 t=581/t=5038, mc1l32 t=148/152/156, mc1l32-bee-height
t=198/219/230, mc1l5 t=2038/2477, mc1hwl0 t=929/1314. 725 workspace
tests green, fmt + clippy clean.

## REMAINING (9,0) RESIDUE (294 rows) — the NEXT dig, with a player symptom

Two lanes, both already mapped:

1. **THE ACQUISITION-PICK DIVERGENCE** (chase want/got both nonzero
   and different, target_yaw `0 vs picked`, t=3200-3500/4200-4300/
   4800-5000 barrages): from identical seeded state, the port's
   `aim_assist` picks a DIFFERENT victim than retail's `sub_54520`
   case 0 — or picks where retail misses. Gates/score/order need a
   line-by-line audit (:63979-64220 vs combat.rs).
2. **THE PROBE GEOMETRY** (full-step x/y diffs + the (10,0) 4-missing/
   1-extra explosion set): the HELD-BACK ring lane (§THE HELD-BACK
   AREA FIXES) — the port's inflated square window + chord march vs
   retail's narrow forward-biased SEARCH.DAT ring, ONE compensating
   family, needs the probe-cadence dig first.

⭐ **PLAYER RULING (2026-08-12): the mid-replay VULTURE KILL is
CORRECT.** First read as a deviation symptom; the player ruled the
vulture is not supposed to survive — the kill matches the real play,
i.e. that stretch of the free replay CONVERGED on this session's laws.
It is a convergence receipt, NOT a certification exemplar — do not
hunt for a way to keep that vulture alive. The two lanes above remain
open on their corpus rows alone.

Other residue: (3,3) balloon 211 rows (now the top block), (5,3) 119,
(9,10) 56, (10,39) 37, (10,43) 25.

# THE ACQUIRE SCAN EXACT + THE FREED-SLOT STALE BYTES (2026-08-12 night) — ✅ LANE 1 CLOSED, mc1l0 −37% AGAIN

**The acquisition-PICK divergence lane (§ above) resolved into TWO
independent laws — the exact sub_54520/sub_54A90 scan and the
freed-slot import law. mc1l0: conforming 5,887 → 5,917, unexplained
field rows 796 → 505; the (9,0) block 294 → 74 (every chase row dead),
(3,3) 211 → 163, (9,1) 30 → 8. mc1l5 +42 pairs / −5.6k rows, mc1hwl0
+29 / −2.5k, mc1l32 +0 / −1.0k. 728 tests (3 new pins), 10 suites 0
regressions, MC2 held. DEVIATIONS "aim assist metric" RETIRED.**

## 1. THE ACQUIRE SCAN IS NOW THE EXACT RETAIL SCAN (`aim_assist_mc1_cone2`)

The port's Δyaw²+Δpitch² metric was a documented deliberate
approximation; the line-by-line audit of sub_54520 case 0 (:63979)
found FIVE divergences, all landed:

1. **The score is distance-weighted** (sub_54A90 :64212-17, castle
   twin sub_54BD0 :64261 identical): the 2-D ground distance
   decomposed onto the angular-error axes — cos terms `>>16`, sin
   terms `>>14` through an i16 truncation (~16x angular weight,
   squared). DISTANCE multiplies everything: the closer candidate
   wins unless the farther is far straighter. This alone was the
   barrage `chase` column (t=3200-3500/4200/4900): every port re-pick
   chose a straighter-farther victim than retail's closer one.
   Shared helper `Gen::acquire_score`; the possess scan already ran
   this exact math — the fix is its generalization (⭐ the method
   lesson AGAIN: the law was already IN THE TREE, one subtype wide).
2. **The range gate is 2-D** (sub_423D0 :52739 has no z term):
   `isqrt(dx²+dy²) ≤ 5120`, and the score uses that SAME truncated
   distance. The port gated 3-D.
3. **The class-3 list is pre-gated at the OWNER row's v_28**
   (:64018-19): 3-D distance (sub_42340, wrapping i16 deltas) from
   the bolt to the node's RAW +72 position vs
   `pool[164*id24] → +156 → +28`. Wizard rows 7/8 carry 8192 (near
   vacuous next to the 5120+pitch-cone bound); a CREATURE-cast bolt
   reads the creature's own row reach (row 21 = 2048 bites hard).
   The beam alone (case 9 :64137) gates on its own `f128 × max_life`.
4. **No model filter anywhere**: bucket[0] holds every live class-3
   body (the Scan-A membership ruling, `nearest_wizard_target`) —
   fireballs acquire mana BALLOONS (+78-lifted) and castles (RAW
   flag z, the sub_54BD0 twin = sub_524C0's model-2 exemption). The
   "wizard-only" blocks 7/8/B/C (duel m7, steal m8, undead m11) have
   NO model filter either (:64100-118) — the port scanned models 0/1;
   `aim_assist_wizards_mc1` is now a thin `creature_pitch: None`
   wrapper over the shared core.
5. **Scan order**: significant list FIRST (human out-of-pool first,
   pool ascending — the Scan-A tie-break ruling), then the 20
   creature buckets model-major ascending (:52267 rebuild keys the
   bucket on the MODEL byte). Ties break to the earlier candidate
   under strictly-less.

The crosshair preview (`aim_preview_scan`) mirrors all of it.
⛔ RULED OUT by measurement: the tick-start list-membership snapshot
(retail rebuilds its lists at :52326 BEFORE handlers, so a mid-tick
death leaves a candidate visible for the rest of the tick) — every
barrage chase row died without it; not modeled, revisit only if a
corpus row demands it.

## 2. A FREED SLOT IS NOT AN EMPTY SLOT (`retail_import_mc1`)

Retail's free path clears +64 and pushes the stack — EVERY OTHER BYTE
STAYS. The blind tracker (§THE PROJECTILE LEDGER + BLIND TRACKER law
2) steers at whatever the record still holds, and that includes slots
already REAPED: mc1l0 t=3464-70, bolt 557 tracks slot 534 three ticks
after its corpse was freed — the stale position keeps the bearing at
~1445 and the corpse's raw-2048 pitch bearing pins the bolt level
(pitch 0, seven ticks). The importer turned class-0 slots into
`Ent::default()`, so the port's tracker aimed at the ORIGIN (bearing
142). Freed slots now import their stale bytes verbatim — class 0,
row 0, unlinked, uncounted, still free-stack members. Worth −174 rows
across three blocks ((9,0), (3,3) balloon, (9,1) possess-lob — blind
readers are everywhere). NOT touched: the port's own NATIVE free path
still zeroes on realloc only (new_event fully re-stamps), but a
mid-tick free inside one pair could still expose zeroed-vs-stale —
no corpus row demands it yet.

## PINS + RECEIPTS

`the_acquire_score_prefers_the_closer_candidate` (near-off-axis beats
far-dead-center, ~1.5M vs ~8.4M), 
`balloons_are_acquire_candidates_and_v28_pre_gates_the_list` (balloon
lock + row-21 vs row-12 owner gate flip),
`mc1_import_keeps_a_freed_slots_stale_bytes_for_the_blind_tracker`
(stale position/f78 survive, slot stays free + unlinked).
728 workspace tests, fmt + clippy clean, 10/10 suites 0 regressions
(0 fixed — the sparse-fixture under-report again; the takes decided).
RetailWizardMc1 gained `Default` (test ergonomics only).
✅ PLAYTESTED + PLAYER-CONFIRMED (2026-08-12): the closer-target
picks and balloon/castle locks play correctly.

## REMAINING mc1l0 (505 field rows) — the NEXT digs

Top blocks: **(3,3) balloon 163** (now decisively the top), (5,3) 119,
(9,10) 56, (10,39) 37, (10,43) 25, (9,0) 74. The (9,0) remainder is
lane 2 as mapped (§ above): position-drift x/y (t=2978-3127 slots
714/663/752 + flyby-amplified target_yaw/pitch echoes at close pass),
the t=4829-4945 death-tick/parked-(253,251) family (= the (10,0)
4-missing/1-extra explosion set), and two small decompile-pinned
loose ends: (a) 4 rows of RAW heading storage (want 65512/65520/
65506/2068 — law 5's storage half; the port stores masked, retail
stores the unmasked u16 and every consumer masks on read), (b) 4
"want 0" pairs (t=3051/3081/4194/4254: retail +34 stays 0 = never
armed while the port acquired — latch-timing attribution open; check
the bolts' tick70/state before believing an acquire story).

# THE BALLOON FLEET STAGGER + THE BLIND BALLOON MOVER (2026-08-12 night 2) — ✅ (3,3) BLOCK DEAD, mc1l0 −32%

**The (3,3) balloon block (163 rows, the top block) resolved into ONE
retail law pair — the dispatcher's stagger and the mover's blindness —
plus the 3-D pick metric. mc1l0: conforming 5,917 → 5,980, unexplained
field rows 505 → 345; (3,3) 163 → 3 (one pair of (10,39)-lane
collateral + one missing spawn). mc1l5 −20.8k rows, mc1l1 −119,
mc2l0 improved, 0 regressions, 1 mc1hwl0 fixture PROMOTED. 403
mgc-sim tests (2 pins rewritten/added).**

## 1. THE DISPATCHER STAGGERS; A FRESH BALLOON PARKS UNTARGETED (`castle_balloons`)

sub_47400's per-fleet-index walk (:56330-97), previously approximated
as "re-pick every pass" (DEVIATIONS entry RETIRED):

1. **A spawned index takes the place of its targeting arm**
   (:56340-49): the newborn parks at the flag with chase 0 — mc1l0
   t=2379, balloon 492 spawns at castle 107 (169,77) and retail holds
   it parked while the old port flew it at ball 115 the same tick.
2. **The stagger** (:56338): the retarget block runs only on passes
   where `castle+63 % quota == 0`; between turns every live balloon
   keeps its stale +146 — even one whose ball has been FREED. The
   modulus is the QUOTA (the MC2 twin sub_60400 EF:61405 agrees).
3. **The census-full arm bypasses the stagger** (:56333-35): houses +
   stored ≥ capacity homes every live balloon every pass; the
   balloon-cargo-full → castle default sits INSIDE the stagger.
4. **The pick is 3-D nearest** (sub_46CA0 :55922 via sub_42390 —
   wrapping i16 deltas INCLUDING z, compared unsigned; the port
   compared 2-D): the t=2407 "wrong ball" rows (want 90 got 67) were
   this metric.
5. **A dead register balloon reaps at dispatch** (:56345-47): cargo
   dropped as an owned ball, record freed, before the quota count.
6. tick70 must be 9 to retarget (:56339).

## 2. THE MOVER IS BLIND (`balloon_move`, sub_47F90)

The mover dereferences the claim ticket by the target's CLASS BYTE
alone (:56735-36) — no liveness check, no model check (the projectile
blind-tracker law, third appearance):

- class 10 → the ball arm (owner gate, tether, absorb, snap) — a slot
  recycled into ANY class-10 record gets absorbed (retail's latent
  LIFO-reuse bug :56742-73 — now the port's law too);
- class 3 → the castle arm (level·speed ring, z-gate, delivery);
- anything else — **including a freed slot's class-0 stale bytes** —
  a plain polar step at the corpse position. mc1l0 t=2472-2516:
  balloon 492 bounces ±48 across freed ball 88's tile forever,
  heading flipping 1024/0 (angle(0,+48)=1024, angle(0,0)=0). The old
  port dead-target guard IDLED there (every pair a y+heading row);
  the old model-39 guard cleared claims retail keeps. Both removed.
- The dispatcher un-sticks a stuck balloon only on its stagger turn —
  and ONLY if it is in the register; mc1l0's 492 was never retargeted
  again (castle 107 established at 2470 with the request bit pending
  — see the open (3,2) lane below).

## ATTRIBUTION METHOD (the row-staring trap again)

The (3,3) "one-tick-late arrival" story survived three readings of the
rows and was WRONG three ways: slot 492 at t=2378 was a freed corpse
(the "arrival" was a SPAWN), the state channel is PRE-tick (dump-state
@N = before tick N — the obs channel is post-tick; align before
comparing), and the oscillation was anti-PHASE only because the port
idled on odd ticks while retail stepped. The kill shot was the
15-minute eprintln probe on mover/dispatcher order, not the row table.

## PINS + RECEIPTS

`the_balloon_mover_is_blind` (recycled-record absorb + the freed-slot
bounce, heading 1024/0), 
`the_dispatcher_staggers_retargeting_and_a_fresh_balloon_parks_untargeted`
(spawn-pass park, off-turn stale claim, 3-D metric + sibling
exclusion). The old `balloon_ignores_recycled_claim_slots` pin (the
guard) was corpus-refuted and REWRITTEN. mc1l0 7,097 pairs: 5,980
conforming, 345 unexplained field rows (first unexplained block now
(5,3) 119 = lane-2 collateral at t=2978/4276 — the worm rows are
bolt-position fallout, NOT a worm bug). mc1l5 200,306 → 179,549,
mc1l1 15,974 → 15,855, mc2l0 2,288 → 1,409 (older baseline), mc1hwl0
t=113 fixture promoted. fmt + clippy clean.

## ⭐ OPEN: THE (3,2) CASTLE REQUEST LANE (the self-destruct cadence)

Player report: a castle self-destructed L3 → nothing, then recast to
crush a worm — the replay's degradation steps run LONG and the kill
misses. Corpus anchors: t=1187 slot 663 flags want 78 got 14 + life
want 19100 got 19200 (−100 in retail), t=2472 slot 107 flags want 78
got 14 — the OBS carries the 0x40 request bit set MID-TICK while the
pre-tick state channel shows 14 both sides: the port's cast/request
writer does not fire where retail's does (and retail's life drops 100
at the same tick). Castle 107's traced rebuild: established action 4
at 2380 (f63 151), upgrade machine action 5 sub-states 0/4/6 through
2422-2469 (~47 ticks/level — the dispatcher and the fleet freeze for
the whole window), established again 2470. NEXT: find the retail
request writer (the m16 manifestation pass? the −100 life is its
signature), diff the port's, then re-time the action-5 wait states
against the painter round trip.

## ADDENDUM (same session): THE (3,2) REQUEST LANE ~CLOSED — token delivery + the death-notice fall-through

The "OPEN (3,2) lane" above resolved the same night into three more
laws, mc1l0 345 → 322 field rows / 47 → 46 missing:

1. **The upgrade token delivers through the OWNER'S BOUND CASTLE**
   (sub_293D0 :31025-31 resolves wizext+50 — retail never writes the
   token's +146; the port's imported tokens carried no link and
   silently missed, which is exactly the flags-78-got-14 family).
   The token is strictly ONE-tick: f26++, PRE-decrement life, arm,
   hit → ch5 {10, owner} + free; miss → release the owner's m16
   manifestation charge pin (sub_46D20(_,0) → port class-12 f26 = 0)
   + free. The ball's lander stamps the owner ONLY, and the token
   ctor LINKS at spawn (:47537 — the fresh token's flags 4 is the
   tile-link bit; the port's ctor never linked).
2. **The death-notice tick falls through** (sub_46DB0 :56003): a
   lethal sub_47EC0 parks `+70 = 6` and the established tick KEEPS
   GOING — owner echo + the whole f63-even block (ejector, extents,
   fleet dispatch, absorb) run while the castle sits at its negative
   life. mc1l0 t=2310: the Shift+L self-destruct (life −1, the
   :55846-50 stamp) SPAWNS balloon 484 mid-death; the level-0 cull
   demolishes it the next tick. The port's early return dropped the
   spawn (the last (3,3) missing row). The lethal arms skip only
   sub_47EC0's own tail (ch5 stays boxed) and the 0x40 else-if; the
   ch0-lethal arm stamps the killer into +38 (:56695-97).
3. The castle-ball's upgrade landing calls sub_524C0 = the +78
   z-lift helper (model-2 EXEMPT → castle no-op) — NOT a damage
   call; the lone remaining (3,2) row (t=1187 life −100) has no
   writer in the token/castle chain and the seed's mail box is
   empty: suspected obs/state capture-point artifact (state@N is
   PRE-tick; a mid-tick mail written after the capture point is
   invisible to the seed). Left open, one row.

Pins: `the_death_notice_tick_still_runs_the_dispatcher`;
`castle_downgrade_ejects_mana_and_demolish_razes` re-tuned (the
notice tick's ejector clamps an over-banked castle BEFORE the
downgrade's two passes — three spilling passes = 12 magnets on the
synthetic 30k bank). mc2l30 t=609 fixture FIXED + promoted by the
stagger law (castle_balloons is reachable from the MC2-column path).
404 mgc-sim tests, workspace green, fmt + clippy clean. mc1l5
179,549 → 179,526, mc1l1 15,855 → 15,789, mc1l2 31,746 → 31,735.

⭐ REPLAY NOTE (the player's worm-kill report): the pair lane is now
clean through the whole self-destruct window (t=2160-2516 holds five
sub-pixel/collateral rows) — every degradation step evolves
tick-exact from a retail seed, so the free-run cadence through a
self-destruct chain is now retail's. The free-run wall remains
pose.z t=563 (the mid-walk restructure) — the worm-kill segment sits
far behind it and needs that wall down before the replay shows the
kill.

# THE PROBE RING + THE CASTLE BALL'S TWO ARMS + THE THUNK MUZZLE (2026-08-13) — ✅ mc1l0 −58%, FOUR LAWS

**mc1l0: conforming pairs 5,983 → 6,007, unexplained field rows 322 →
136 (−58%), missing 46 → 42, extra 2 → 1. Cross-level: mc1l5 200,306
→ 171,099 (−29.2k), mc1l1 15,974 → 15,629, mc1l2 31,897 → 31,447.
The (9,10)+(10,43) castle-ball block is DEAD (60 → 0), (5,3) 119 →
52, (9,0) 62 → 18. 406 mgc-sim tests (2 new pins), 10/10 suites 0
regressions, L005 goldens D/E re-pinned (OBSERVABLE moved D/E only —
correct signal, behavior by design).**

## 1. THE MUZZLE-ACQUIRE 34-STEP IS STORED RAW (:62824)

The fireball's one-shot acquire, on a HIT, turns yaw ≤34 toward the
pick and stores the sum UNMASKED — a step across 0 parks an
out-of-range u16 in +30 for a tick (t=2739: 65512 = −24; port masked
2024 = same angle). Every consumer masks on read; the next homing
write canonicalizes (sub_52550 masks, `HIBYTE &= 7`). Worth 4 rows.
Pin: `the_muzzle_acquire_34_step_stores_the_heading_raw`.

## 2. A THUNK BOLT AIMS FROM THE UNLIFTED MUZZLE AND IS BORN WITH +34/+36 = 0

Every creature thunk (sub_1A8E0 :21874 and its whole family,
:21893/:21922/:21949/:22120/:22153/:23257/:25855, ×4-lift :26171,
seeker :24693) computes BOTH bearings from the SHOOTER's +72 struct —
the muzzle lift `+76 += +84` never enters the aim — and none writes
+34/+36 (NewEvent zeroes them; the first homing/arm tick fills them).
The port aimed from the lifted z and mirrored f34/f36 at the arm.
`arm_projectile` now takes the lift and applies it AFTER the aim.
ALSO settled here: the four "want 0" pairs (t=3051/3081/4194/4254)
are the WALK-CURSOR law, not a latch — a creature-cast bolt born
BEHIND the ascending walk cursor (caster slot > bolt slot) never
ticks on its spawn tick, so retail surfaces it with target_yaw 0 and
the ctor pitch; player casts spawn from the carpet's walk position
and tick the same pass when they land ahead. The port's natural
ascending walk already models this once the ctor stops pre-arming.
Pin: `a_thunk_bolt_aims_from_the_unlifted_muzzle_and_is_born_with_zero_target_yaw`.

## 3. THE MC1 PROBE WINDOW IS RETAIL'S RING (held-back #2 LANDED, per-game seam)

`victim_scan` (sub_11980 :16999-17001) and `claim_victim_scan`
(sub_108B0 EF:3798-3802) now walk the SEARCH.DAT ring iterator over
the `(pos + 128) >> 8` rounded centre with NO radius floor on
MC1/HW — retail's exact forward-biased narrow window, matching
retail's ONE-probe-per-move cadence which the MC1 movers already
had. The probe-cadence dig resolved to: **MC1 movers never march**
(endpoint-only `victim_scan_at`), so the ring is retail-exact there
with no compensation needed; the chord march is MC2's alone
(mc2/proj.rs ≤128-unit sub-steps, the anti-tunnel deviation), so
MC2 KEEPS the truncated-centre square + `.max(1)` floor as the
march's compensating window — the five 2026-08-12 pin casualties
(fools-trap muzzle, meteor homing lock, arrow collateral, two
muzzle-admission guards) all survive because MC2 is untouched. The
seam is `Gen::probe_window` (game-keyed on the movement verb). The
forfeited mc2l4/mc2l30 ring pairs (+1/+2) stay forfeited with the
march, documented at the seam. Worth −114 rows / +8 pairs on mc1l0
alone, zero new rows.

## 4. THE CASTLE BALL'S TWO ARMS SPLIT ON +146, AND THE CTOR BINDS ROW [1]

sub_53980 (:63459) dispatches on the TARGET, not the model: a castle
ball with a homing slot in +146 (the upgrade cast stamps the bound
castle, :65906-08) runs the HOMING arm — sub_52610 every tick (the
twin WITHOUT the aim-lift wrap: bearing to the target's RAW z), ±2
speed ease, arrival on plain OVERLAP (blind — no class/dead guard)
teleports onto the target, terrain touch delivers in place, life
decrements only AIRBORNE (:63494-96 short-circuit), and the launch
latch (+16 bit 1) is NEVER touched — the corpus balls fly with flags
4 throughout (t=2174-77). Delivery morphs into (+68, +69) at the
current position, owner-stamped; a class-3 morph is REFUSED when the
owner already holds a bound castle (:63500-04, wizext+50); a FULL
POOL releases the owner's m16 charge pin instead of killing the ball
(:63513-15 — it retries next tick). +146 = 0 falls to the sub_53B50
create arm (dest +150 steering, the launch latch, the site scans —
the port already had this arm right). The port had routed BOTH
variants through the create arm, so the per-pair upgrade ball
steered at IMPORTED ZEROS (+150 is never written by the upgrade
cast — dest_x 0 in every record) and stamped the latch bit: the
whole (9,10) 56-row + (10,43) 4-row block. ALSO: sub_39F40 binds
`unk_98F38[1]` (:46185) — the port's ctor bound row 0; the recorded
ball's model_ptr resolves to row [1] (anchor: the player fireball's
row [5]).

## REMAINING mc1l0 (136 field rows) — catalogued for the NEXT digs

1. **(5,3) 52 = the t=2978 TERRAIN-SHADOW burst** (55 rows one tick,
   whole worm chain, sub-pixel x/y + heading/pitch ±1-40): every
   segment also carries EXPLAINED `mc1l0-terrain-z` rows — the
   knock-on that rule's own note predicts ("walker x/y/heading
   knock-on stays UNexplained"). A mid-tick terraform writer is the
   terrain channel's documented blind spot. Classification decision
   (categorize+ask): extend the rule vs leave catalogued — ASK.
2. **(10,39) 37** (ball flight residue) + **(9,0) 18** (2978
   collateral, 3005-3127 sub-pixel drift band, t=4285, t=5026).
3. **THE t=3855 ECOLOGY BURST: 39 missing (2,0) TREES in one tick**
   (+ 3 missing (2,1) at t=1883). Retail mass-plants; the port has
   NO tree-reproduction/ecology spawner for MC1 (tree_tick handles
   damage/burn only; retail's sub_49890 likewise — the spawner is
   elsewhere, dig wanted: find sub_37BC0's periodic caller).
4. **t=5026 (5,12) rand divergence** (slot 640: rand + heading +
   target_yaw + x/y, one draw off) with (9,0) 785 collateral.
5. The t=1187 (3,2) life−100 capture-point row (held open, 1 row).

## RECEIPTS

Pins: `the_muzzle_acquire_34_step_stores_the_heading_raw`,
`a_thunk_bolt_aims_from_the_unlifted_muzzle_and_is_born_with_zero_target_yaw`.
The worm-chain test now aims PITCH at the head too (the level-only
aim left the kill to the dive phase; the deterministic chase settles
into orbits that never present one — second occurrence of this
phase-sensitivity, after the class-10 fire fix). L005 GOLDEN +
OBSERVABLE D/E re-pinned with attribution (post-init..C hold both).
406 mgc-sim, workspace green, fmt + clippy clean, 10/10 suites 0
regressions. Repo-root mc1l0/l1/l2/l5.tsv baselines refreshed.

⭐ REPLAY NOTE: free-run wall holds at pose.z t=563 (unchanged — the
mid-walk restructure lane). Post-wall traffic benchmark for the
fraction-of-a-second castle-destruction lateness: 18,351 pose rows /
96,212+356,706 set atoms / 1,469,581 field rows (record here, compare
after each block; no prior benchmark existed).

## ADDENDUM (same session): THE CASTLE TIMELINE GROUND TRUTH + THE BOUND-CASTLE CAST DISCRIMINATOR

**The player's either/or on the replay's castle lateness ("demolition
too slow at ~2100" vs "the 1750 castle was never meant to reach L3")
is settled by the state channel — NEITHER, exactly: the retail castle
NEVER LEVELS AT ALL in this era.** Dump-state ground truth (slots
533/107, t=1765-2420):

- t=1765 castle A raised at tile (132,90), establishes to f26=1 /
  cap 10000 by t=1780 (kills the worm on the footprint).
- t=1800-1830 **castle A is KILLED**: life 20000 → 0 inside ~15
  ticks (monster-fight class — the snowballing single-target melee
  scale), −100 death stamp at t=1830 (= the (10,39) dig's t=1830
  teardown), slot freed t=1832. The player's recollection has no
  castle death here — the record does.
- t=1880 cast: with NO castle standing this is a CREATE — castle B
  rises at tile (148,104), 16 tiles away, level 1. The t=1910
  "makes no sense" cast produced NO ball at all (fizzled in
  retail).
- t=2159-2177: two UPGRADE balls home at B (f146=107, today's
  homing-arm corpus), tokens deliver, and B STAYS level 1 / cap
  10000 — bank 8302 < the 10000 ladder; delivery without a full
  bank does not level (the port matches per-pair: 0 rows).
- t=2310 Shift+L kills B (life −1, the balloon-session date);
  t=2350 castle C rises at tile (169,77) — the worm hill — legal
  because B is GONE.

⇒ The REPLAY's L3-castle-blocks-everything story hinges on ONE
load-bearing event: **castle A must die at t≈1830.** In the drifted
free run the fight goes elsewhere, A survives, and the t=1880/1910
recasts become UPGRADES on a standing bound castle — L2/L3 at site
A, no move to B, the 2350 site-C cast blocked by the stub, and the
rng stream diverges from there (the player's shore-worm hunch).
Demolition cadence is EXONERATED (per-pair clean through the whole
window; there is only one Shift+L kill of an L1 castle to
reproduce). This family converges only as pre-1880 drift falls —
same lane as the pose.z t=563 wall, not a castle law.

**One genuine law landed from the dig:** the cast's create-vs-
upgrade split reads **wizext+50 — the BOUND castle** (:65893-94),
not "any owned (3,2)": +50 is written only by the level-up commit
(:56484) and cleared by the removal path (:56534), so a fresh
level-0 flag is UNBOUND and a recast over it re-SITES. The port's
`cast_castle` used any-live-castle; it now filters on the
established stand-in (f26 > 0, the same stand-in the upgrade
token's delivery resolves). Corpus-NEUTRAL by construction (at
every recorded cast both discriminators agree — verified: mc1l0
byte-identical, 10/10 suites, all tests green); the arm it fixes is
native/free-run recasts over an unestablished flag and the
death-notice window.

## ADDENDUM 2 (same session): CASTLE A'S DEATH IS A TWO-TICK CREATURE KILL + THE DEMOLISH READS wizext+50 + THE FIRE-RAND MASK RE-ATTRIBUTED

**Correction to Addendum 1's "monster-fight class" guess AND the
player's "I killed the castle" recollection — the tick-exact trace
settles the mechanism:** castle A holds 20000 life through t=1807,
drops to EXACTLY 0 by t=1810 (one hit ≥ 20000, clamped — the
TWO-TICK castle death law's first tick), parks at 0 for ~20 ticks
while balloons keep banking (280 → 1260), and dies to a second
100-damage bite at t=1830 (−100 stamp, f26 → 0, bank scattered).
That is the SNOWBALLING single-target melee protocol delivering an
accumulated ~20k residue as bite 1, and a plain 100 as bite 2 — a
creature kill, NOT a demolition. Retail's Shift+L (:55838,
`if (wizext->var_50) pool[var_50].actLife = -1`) is an INSTANT −1
and never produces this shape. ⇒ the free replay's castle-A
survival is snowball-residue drift sensitivity (the residue history
rides the whole worm fight), not a demolish defect; converges with
the drift lanes. Demolition remains exonerated.

**Landed:** the MC1 demolish arm now resolves the BOUND castle
(wizext+50, established stand-in f26 > 0) like retail — the same
discriminator the cast split got in Addendum 1; the port had
any-owned-castle. Corpus-NEUTRAL (recorded demolish t=2310 targets
the established castle B under both rules — take byte-identical,
10/10 suites, tests green). An unestablished flag can no longer be
demolished natively.

**The `mc1hw-fire-churn-rand` mask is RE-ATTRIBUTED (player
challenge, measured):** its "a DIFFERENT fire occupies the slot"
story fits ZERO of the 148 currently-masked mc1l0 pairs — none has
x/y rows; 130 ride with an mc1l0-terrain-z sibling, 18 stand alone.
Measured at t=1768 slot 555 (seed-stepped): SAME fire, SAME
position, retail draws 2 per-entity LCG steps on the latch tick,
the port 1 — the SCORCH GATE (:28095-98, `z − ground ≤ 128` term)
flips on the terrain closure's divergent ground sample. Draw parity
is per-entity (no global cascade), but the flipped scorch also
skips/adds a dig_scorch TERRAIN write — the flip feeds the terrain
closure forward in free run. Rule note rewritten; stays OPEN with
the terrain-phase dig as parent (the port's fire draw structure
matches :28085-28118 line-for-line).

**BANKED (player ask, next session): add the castle-parameter lanes
to the verify CSV.** The wizard channel already diffs the raw
wizext+50 against the port's established stand-in (`wizard0.castle`
/ `player.castle` — a live cross-check of today's discriminator),
but **entity +26 (f26) is NOT in the per-entity diff set**: a castle
LEVEL / establishment-timing divergence only surfaces indirectly
through mana_max (the CASTLE_CAP ladder), and f26 also carries the
token charge, the fire's spread gate and the class-2 slot stamp —
cheap to add (export + one cmp_field), high signal for the whole
castle pipeline. The wizext +326 charge byte (the upgrade ball's
+26 stamp source) is not decoded at all — add it to the wizard
decode + diff while there. Expect a new row family on first run
(f26 was never compared); triage before trusting.

# THE f26 + CHARGE RAW LANES LANDED (2026-08-13, the surfacing session)

**The banked ask above is DELIVERED — two new comparison lanes ride
the RAW state channel (the obs schema is check-decode-locked, so
they take the same lane the hands ride, `append_charge_diffs`):**

- **Per-entity `f26`** — the castle level / establishment lane, the
  token burst counter, the charge stamp. Class-12 rows compare
  retail **+48** (the port keeps a manifestation's burst counter in
  f26; retail's +26 is the spell level there — `import_ent`'s own
  mapping). Port side exported by `World::charge_lane_mc1`.
- **wizext +326 (`charge`)** — decoded into `RetailWizardMc1` and
  MODELED in the port: `World::wiz_charge[8]`, +1 per live carpet
  tick to a 200 cap (human :55377-78, rival :17987-89, each in its
  own handler right before the mana-regen step), seeded at import
  like the regen stall, **consumed by exactly the arms the
  decompile shows touching +326**: fireball :65072-73 (spells 0/23),
  earthquake :65356, meteor :65414, volcano :65472 bank the meter in
  the new bolt's +26 and zero it; possess :65246 zeroes WITHOUT
  stamping (its bolt carries the forced 200). Crater/duel/steal/
  magnet/bolt never touch it. Same stamps on the rival emit path
  (same class-12 spawners). Hash-quiet + not in the savestate (no
  in-engine reader); the L005 golden D/E re-pin is the HASHED bolt
  f26 stamp alone — post-init..C hold, OBSERVABLE holds
  byte-for-byte.

**mc1l0 (7,097 pairs): the new lanes are PURELY ADDITIVE — every old
family byte-identical, unexplained 136 → 2,081 field rows, all new:**

| family | rows / pairs | reading |
|---|---|---|
| (12,3) f26, slot 139 | 854 | HUMAN possess manifestation (f42=630): burst-counter CYCLE PHASE skew — retail counts 3→2→1→0, port cycles offset 1-2 ticks |
| (12,0) f26, slot 305 | 782 | HUMAN fireball manifestation: same cycle-phase class |
| (9,1) f26 | 234 (all `16→200`) | retail's possess lob/possessed ball carries +26=16 (the drain-period constant); the port carries the spawner's forced 200 — post-claim +26 law unmodeled |
| (12,16) f26, slot 28 | 44 | ⭐ the CASTLE manifestation's cast/lockout state machine ({0,100,101}) — rows sit EXACTLY on the castle timeline (559-606, 802-805, 1138-1294, 1761-1925, 2158-2515, 3845-3897, incl. t=1807/1830 = castle A's death window and t=1870 the recast): the port arms/releases its 100-lockout on different TICKS than retail's 101, plus an armed-value off-by-one (100 vs 101) |
| wizard0.charge | 12 | cast-tick consume skew (retail 1 vs port 200 = port missed the cast that pair; small values = port cast n ticks early) — same phase story as (12,x) |
| (9,10) 8, (9,0) 6, (5,12) 1 | 15 | minor; (9,0)'s `16→…` repeats the in-flight-16 signature |

**⭐ THE CASTLE HEADLINE: castle (3,2) f26 has ZERO rows — the
castle LEVEL conforms on all 7,097 pairs.** Within-pair castle
leveling/establishment is exact; the replay's castle-A drift stays a
free-run phenomenon, and the (12,16) lockout skew is the only
castle-pipeline residue the pair harness can see.

**mc1l1/l2/l5 refreshed (baselines mc1l1/l2/l5.tsv):** l1 +5,386
f26 / +2,986 charge, l2 +9,063 / +4,103 + 1,206 rival.charge, l5
+44,113 / +9,104 + 3,061 rival.charge. Reading: honest new dials on
the KNOWN cast-cadence/phase skew of the messy takes — l1's retail
meter cycles 0/2/3 (firehose take, zeroed every emission) vs the
port's offset cycle; first-cast rows (e.g. l1 t=46 `0→65`) are the
meter's level-start climb consumed one tick apart. NOT harness
noise.

**Fixture fallout (NOT re-frozen — player call):** mc1l0 65
regressions + 5 drifted, mc1l32 119+7, mc1l32-bee-height 26+6,
mc1l5 56+16, mc1hwl0 383+18 — every one a new-lane row on a
previously-conforming pair (the pre-authorized noise: "the new rows
are the deliverable"). ⚠ mc1hwl0's bulk is a NEW family: **(10,18)
f26** — an HW effect-counter lane, uncharacterized. MC2 suites
untouched by construction: 5/5 green, 0 regressions.

**OPEN (categorize+ask, in ask order):** ① the (12,0)/(12,3) human
burst-counter cycle phase — one cast-arm phase story or a counter
law? ② the (12,16) castle lockout arm ticks + 100-vs-101; ③
suite doctrine for the 649 new-lane reds (re-freeze with attribution
vs hold red until the phase digs land); ④ (9,1) post-claim +26=16;
⑤ mc1hwl0 (10,18) f26; ⑥ possess-family +50=3 note: slot 139
carries +50=3 (emit at counter==3) — the port fires at count, worth
a look during ①.

## ADDENDUM (same session): SUITE DOCTRINE RULED + THE DEMOTE FLAG

**⚖ PLAYER RULING on open question ③:** the 649 new-lane reds were
never truly conforming — the comparison simply didn't cover the
lanes at freeze time. Mark them as what they are. **Landed:**
`fixtures --demote <note>` — the deliberate twin of `--promote`:
acknowledges regressions as OPEN with the signature recorded and
the note appended (existing curation notes are kept, the
attribution is appended). All five MC1 suites re-frozen with note
"f26/charge raw-lane surfacing — lanes not compared at freeze":
mc1l0 65 demoted + 5 drift-refreshed, mc1l32 119+7, bee-height
26+6, mc1l5 56+16, mc1hwl0 383+18. Confirmation pass: **10/10
suites fully as-expected, 0 regressions / 0 drifted.**

**Ordering ①/② left to the port (player): ① first** — the
(12,0)/(12,3) cycle-phase story is the bulk and the castle
lockout's arm-tick skew (②) most likely shares the cast-arm phase
root; ② gets the 100-vs-101 armed-value check during the same dig.
④/⑤ deferred until ①/② settle (player ruling).

# THE dw_0 CAST LANE + CASTLE LOCKOUT LATCH (2026-08-13, digs ①+②)

**Both banked digs closed in one root-cause session — the
(12,0)/(12,3) burst cycle-phase family and the (12,16) castle
lockout machine are BOTH DEAD, plus the wizard0.charge and (9,10)
families, minus 1,704 mc1l0 rows, 290 fixtures promoted, 0
regressions, 10/10 suites fully as-expected.**

## ① THE CAST LANE WAS FED THE WRONG INPUT (harness fix)

The (12,x) "cycle phase skew" was never a counter law at all — the
port's burst machine is exact. The harness's cast lane was wrong
three ways at once:

1. **Wrong source.** verify/fixtures fed the RAW sampled mouse
   levels (`input.mouse_buttons`). A physical click spans 2-3
   consensus samples (each re-armed the token mid-burst: the
   `got=3,3,3` stair), fast clicks vanish between samples entirely
   (mc1l0 t=75/77 casts had NO raw bit), and spellbook/UI clicks
   leak in as casts (t=2653: the raw left click is a hand equip —
   retail's input layer never emits a command for it).
2. **Wrong phase.** The `--input-delay` ring is pre-seeded with
   `delay+1` defaults, so even at delay 0 pair N consumed the
   sample at N−1 — every arm landed one pair late.
3. **Wrong semantics.** Under strict the port treated every held
   bit as a command edge; retail's input layer emits ONE command
   per click for the `+60==1` launcher set (:20601-34, the
   press-edge latch) and only re-emits per held tick for the
   hold/channel set gated on burst-live/queue-pending.

**The fix: the cast lane now rides dw_0** — the CONSUMED move/fire
word in the record's own state channel (`verify::fire_bits_mc1`,
bits 16/32; dw_0@N drives pair N→N+1, the move-byte phase law).
It is already edge-filtered, UI-filtered, phase-correct, and
carries the clicks the sampler missed. dw_0 == 48 exactly is the
DEMOLISH word (both fire bits, casts neither hand — the recovery's
own rule, re-used; missing that exclusion was the one intermediate
regression, mc1l5 t=711). The raw-mouse lane and the delay ring
are GONE from the MC1 pair paths.

**Retail command law pinned while proving it (sub_46B00 :55851):**
the human cast handler is TWO-ARM — `+62 != 0` spells don't arm
+48 at the command at all: they bump the +61 QUEUE (clamped to
min(mana/cost, +62); the HUD meter :27735 draws 55·(+61)/(+62)),
and the no-command branch (:55862) arms +48=+50 while the queue
pends. `+62 == 0` spells (every current corpus family) arm
directly at LABEL_32 (:55893). The token tick's `while (+61 >= 0)`
loop (:65054) spawns one bolt per queued click on the full tick.
Queue machinery is NOT yet modeled in the port — no corpus row
needs it (f61/f62 read 0 throughout).

**Also pinned: the silent command mana gate reads the PRE-step
pool** (:55890 — retail consumes commands before its wizard mana
step). mc1l32 t=671: fireball refused at 148 < 200 with +100 regen
landing the same tick. The port's launcher gate now compares
`pre_mana` captured above the delta apply (`mc1_wizard_pass`); the
token-side gates were already pre-step by pass order.

## ② THE CASTLE LOCKOUT IS A LATCH DRIVEN BY THE CASTLE MACHINE

`sub_46D20_47060` (:55949) — the "charge pin" — resolves the
owner's Create-Castle TOKEN through wizard+708 and either PINS
`+48 = +50−1 = 100` (a2=1) or RELEASES `+48 = 0` (a2=0). The pin
census (all call sites):

- every blast-shake countdown tick (:55995-96, pre +50 >= 2 — the
  ==1 tick transitions without pinning): **castle damage re-arms
  the lockout with NO cast** — the whole attack-window residue;
- the repaint/leveler actions (+48 cases 3/5, :56085/:56089);
- the downgrade teardown (:56529).

Releases: the machine settling to established (case 2, :56081),
total destruction (:56533 — which also unbinds wizext+50), and the
ball's failure paths (pool-full :63513, water-fail :63620). The
cast itself latches 100 at the token (:65885, only inside the
spawn's `if (v3)`); gate failure releases (:65920). NOTHING
decrements it — the old port recomputed a per-tick predicate
(`castle_lock_active`: ball-alive ∨ f59 ≠ 4) that lagged retail's
transitions by 1-2 ticks and read established (=released) during
blast-shake countdowns retail PINS through.

**Port now models the latch**: `manifestation_tick` 16 fires and
latches count−1 / releases on gate-fail and otherwise leaves f26
alone; the World castle dispatch applies the pin/release census at
the castle's own pass position (pre-tick f59/f50/tick70 → stamp
via `castle_owner_token`, the +708 registry twin); the command
site reads the token latch directly (:55903). The native
`cast_spell` legacy arm keeps the old predicate (non-launcher
callers only). Accepted residuals (commented at the arm): retail
re-runs the token gate every latched tick (mid-latch caster-death
release), the ball failure releases, and the pool-full-ball retry
at 101.

**Bonus consumer the +326 sweep missed:** the castle ball spawner
stamps the caster's charge meter into the ball's +26 and zeroes it
(:65910-11) — `cast_castle` now does both. That was the ENTIRE
wizard0.charge residue (12 rows, all on castle windows) and the
(9,10) f26 family (12 rows).

## CENSUS (mc1l0, 7,097 pairs)

| family | before | after |
|---|---|---|
| (12,3) f26 slot 139 | 854 | **0** |
| (12,0) f26 slot 305 | 782 | **0** |
| (12,16) f26 slot 28 (castle lockout) | 44 | **0** |
| wizard0.charge | 12 | **0** |
| (9,10) f26 (castle ball charge) | 12 | **0** |
| (10,39) z | 43 | 41 |
| every other family | — | byte-identical |

Total rows 5,097 → 3,391; unexplained field rows 2,081 → 377.
The 100-vs-101 armed value needed no separate law — it was the
input phase. (9,1) 234 `16→200` (post-claim +26, item ④) and
mc1hwl0 (10,18) (item ⑤) remain, per the deferral ruling.

## SUITES + BASELINES

All five MC1 suites promoted (`fixtures --promote`): mc1l0 62
fixed + 8 drift-refreshed, mc1l32 119+6 (its 1 intermediate
regression was the demolish-word exclusion, fixed), bee-height
25+6, mc1l5 0+4 (drifts all row-shrink improvements; its core
residue is (12,2) accelerate-token f26 — a hold-spell lane — and
the ④ family), mc1hwl0 84+34 (residue = (10,18), item ⑤).
Confirmation: **10/10 suites fully as-expected, 0 regressions**
(MC2 untouched by construction). Baselines mc1l0/l1/l2/l5.tsv
refreshed post-fix. 504 mgc-sim tests green; workspace green.

f26+charge rows per refreshed baseline: mc1l0 1,945 → 241, mc1l1
8,372 → 2,806, mc1l2 14,372 → 6,998, mc1l5 56,278 → 42,189. The
remainders: ④ (9,x) post-claim +26, mc1hwl0's (10,18) ⑤, the
messy takes' wizard0/rival.charge cast-cadence dials, rival-owned
tokens (inert by design under strict — l5 (12,3) slot 677), and
⭐ A NEW CHARACTERIZED LEAD on l5's HUMAN tokens: (12,3) slot 652
reads `0→251` × 6,114 (the port PINS 251 — a 251-count value —
with NO command in the pair; NOT owned[]-routed, the owned table
is clean) and (12,2) reads an off-by-one live countdown
(`247→248…`) plus `0→3` × 463 — the accelerate/hold token machine
under strict. Writer unidentified — next-session microscope.

# THE mc1l0 CASTLE STORY + THE l0-MVP PLAN (2026-08-13, scoping session)

**⚖ PLAYER RE-SCOPE: mc1l0 fully explained = the MVP of the entire
conformance approach. Castle build/destruction first; other takes
are regression guards only; non-l0 fixes banked.**

## THE CASTLE STORY — RECONSTRUCTED FROM THE RECORDING (six castles)

Microscope: `examples/castle_timeline_mc1.rs` (event log over (3,2),
(9,10), (10,41/42/43), wizext+50 binds). The retail states read:
f70 5=action/4=established/6=deferred-death, f48 = the case machine
(4 painter, 5→6 leveler, 2 settle), f50 = blast-shake countdown.

| # | slot | site | built | estab. | fate |
|---|---|---|---|---|---|
| 1 | 486 | 114,96 | 562 | 605 | ⭐ MAIL GULP −68,800 @606 → dead 607 |
| 2 | 663 | 37,93 | 1142 | 1185 | lvl2 @1189; DEMOLISH (−1) @1289 → downgrade lvl1; demolish AGAIN @1297 mid-repaint → dead 1364 |
| 3 | 533 | 132,90 | 1765 | 1808 | gulp EXACTLY 20,000 @1809 → life 0; creature −100 @1830 → dead 1831 |
| 4 | 107 | 148,104 | 1883 | 1926 | lives 290t; demolish (−1) @2216 → dead 2218 |
| 5 | 107 | 170,86 | 2267 | 2310 | demolished MID-BUILD @2289 (−1 while f48=6) → dead 2312 |
| 6 | 107 | 169,77 | 2336 | 2379 | gulp EXACTLY 20,000 @2380 → life 0, SURVIVES; lvl2 @2428 (life re-caps 40,000), lvl3 @2474, ⭐ lvl4 @3856 — THE KEEPER |

- **The gulps are the mailbox law working as retail built it**: ch0
  mail accrues UNTOUCHED during the build (established f70=4 is the
  only damage processor) and the first settled tick swallows it
  whole. Castle 1 measured: 44,000 by t=570 (source slot 119),
  68,800 by t=580 (source 97) — both **(10,0) FIRES, f44=400**, at
  the site: the player built INSIDE A BURNING VILLAGE (~14 fires ×
  400/tick × 43 build ticks). 20,000 − 68,800 = −48,800 exact.
- Castles 3 and 6 both gulped EXACTLY 20,000 (sources 80/489) —
  the exact-lethal-to-zero constant is unexplained; check whether
  a single attacker class mails a 20,000 quantum or the gulp
  CLAMPS. (Castle 6 survived at 0 and became the keeper.)
- The −1 deaths are the Shift+L DEMOLISH write (act_life = −1,
  :55846-50; dw_0 == 48 words) — the player RELOCATED castles by
  demolishing: 5 demolish events across castles 2/4/5.
- ⭐ **t=3855 is NOT an ecology burst**: the 39 missing (2,0) trees
  sit ON castle 6's LEVEL-4 UPGRADE COMMIT (ball @3854, commit
  @3856). Lead: the port's `castle_upgrade_preclear` (house
  pre-clear, :56461) may sweep TREES the retail pre-clear keeps —
  or the commit's terrain paint spawns/keeps trees the port kills.

## THE UNEXPLAINED MAP (426 rows, post-dw_0/latch session)

- **234 = (9,1) f26 `16→200`** — the ~5-tick-cadence singles across
  the whole take (the possess spam). Retail re-stamps a CLAIMED
  ball's +26 to 16 (= possess cost/period 50/3) at the claim; the
  port keeps the spawner's forced 200. Likely a one-site stamp in
  the claim/detonation handler (spawner :65246's claim path).
- **Castle-window clusters** (castle-coupled, dig against the
  timeline): 7@604 + singles 567-733 (castle 1 establish/death +
  aftermath; pose.z@567 = build-paint under the carpet), 2@1143 +
  1143-1233 (castle 2 build, incl player.life@1143), 1765-2014
  (castle 3: 2@1808, 2@1810, 4@1830 death, **9@1869** recast prep),
  1@2216 + 6@2217 (castle 4 demolish), 1@2345-2@2383 (castle 6
  gulp window), 39@3855 (the trees), 2@3879 (pose.eff_pitch/z).
- **Non-castle**: 55@2978 = (5,3) terrain-shadow lead; 11@5026 +
  3@5030 = (5,12) rand; 4@2811 (9,0); (10,39) x/y/heading spread.

## THE l0-MVP PLAN (next session, in order)

① (9,1) post-claim +26 stamp — 234 rows, likely one line after the
   claim law is read out of the decompile (start :65246 → the
   (10,12) claim flash handler).
② The castle-window clusters — walk each against the timeline
   events above (death-notice ticks, ejector draws, balloon fleet,
   recast prep at 1869). The story table gives each cluster its
   named retail event.
③ The 39@3855 trees = castle lvl-4 upgrade pre-clear vs trees.
④ 55@2978 (5,3) terrain-shadow; ⑤ 11@5026 (5,12) rand;
⑥ (10,39) + (9,0) + pose.z/eff_pitch tails.
Guard: all 10 suites green after each landing; non-l0 findings
BANKED, not fixed.

## ⚖ PLAYER CORRECTIONS TO THE STORY (same session — ground truth)

The gulp attribution above was misread from the (10,0) mail
sources. The player's account (outranks inference):

- **Castle 1: built onto THREE WORMS** — the footprint crush kills
  all three simultaneously, and the crush recoil is guaranteed
  lethal (68,800 accrued). NOT a burning village; the (10,0) mail
  sources are the worm-kill effects, not ambient fires.
- **Castle 2: manual demolish** ✓ (as read).
- **Castle 3: built ON TOP OF one worm** — the crush recoil is the
  EXACTLY-20,000 gulp (⭐ answers the quantum question: ONE worm's
  crush-back = 20,000, the worm's own life/damage constant — castle
  6's identical 20,000 = the same law, another worm). Then a
  **VULTURE attacking while the carpet sits on the castle premises
  lands the −100 that kills it in retail (t=1830) — THE PORT DOES
  NOT PROCESS THAT DEATH: the castle survives at −100 and the
  subsequent castle cast REBUILDS/UPGRADES it instead of creating
  castle 4 at the new site.** This is the 4@1830 + 9@1869 cluster.

**⚖ RULED: the vulture-hit castle death at t=1830 is THE FIRST FIX
of the l0-MVP session** — ahead of the (9,1) stamp. Question to
answer in the dig: how does a vulture attack aimed at the FLYER
route its damage into the castle (premises splash? building-under-
carpet targeting?), and why does the port's castle miss or not
process that ch0 mail on the death tick.

# THE l0-MVP SESSION: BALL CHAIN + ACQUIRE CLAMP + SOFT-KILL SWEEP + TERRAIN SHADOW (2026-08-13)

**mc1l0 unexplained 426 → 48 rows (+6 pose singles). 6011/7097 pairs
conforming, 7077 conforming+explained (99.7%). All 10 suites
as-expected after promote (8 fixed, 5 drifted refreshed, 0
regressions, MC2 untouched). 504 mgc-sim tests green.**

## 0. THE VULTURE-DEATH DIG RESOLVED THE STORY — AND FOUND A DIFFERENT LAW

The ⓪ question dissolved on measurement: **pair-mode already
conforms end-to-end on the castle-3 death.** The vulture's victim is
the CASTLE ITSELF (f146 = 533 — the engine targeted the castle; the
"attack on the flyer" is the visual), the −100 ch0 mail is delivered
in pair 1828→1829 identically by both engines (`castle_mail_probe`
microscope — mailboxes are outside the locked obs schema, so this
needed the new probe), and the death processes cleanly (life −100,
f70 4→6→dead, no (3,2) rows). The replay-mode "castle survives"
observation is compounded free-run drift, not a pair law. The actual
4@1830 residue was the castle-death EJECTOR's mana ball — which
exposed:

## 1. THE TICK-START BALL CHAIN (retail var_u32_36462[1]) — LANDED

Retail rebuilds four entity chains ONCE per tick, before the walk
(:52246-312): chain[0] buildings, chain[1] = class-10 models 39/40
(mana balls + jars), chain[2] houses, chain[3] projectiles, plus the
20 per-model creature chains (str_36382x). **Chain WALKERS see the
tick-start roster, not the live pool** — an entity spawned mid-walk
is invisible to every chain consumer until the next rebuild. The
(10,54) magnet stamp (sub_29920 :31247) walks chain[1]: the port's
live-pool scan saw the freshly-ejected castle-death ball and pulled
it ONE TICK EARLY (measured: retail@1832 ≡ port@1831 field-for-field
— f30 918, the (1,4) impulse, the gravity step). Port: `TickChain`
(`Gen::ball_chain`, hash-silent like SlotGens), rebuilt in the
bucket-count sweep at the tick top; `mana_magnet_tick` iterates it.
Killed 4@1830 AND 6@2217 (castle-4 demolish, same law). ⚠ Other
chain[1] readers (:17325, :55931, :56024, :64043, :64288, :22268,
:22929, :24769, :18509) still scan the live pool in the port —
migrate as corpus signals appear.

Also measured, NOT residue: the ejector ball's f46 differs (−44
retail vs −60 port = ground probe 1152 vs 1024 at the just-unstamped
castle footprint) and z by 16 — neither field reaches the obs diff.
The eject's f144 = castle id24 → the importer's PLAYER_TARGET
translation round-trips correctly (looked alarming in the probe, is
not a bug). Port-only 11 house-drop ball spawns at (143-147, 96-99)
during the death pair remain UNINVESTIGATED (invisible to obs —
mana-economy drift lane; the free-stack shows retail spawned only
bolt+ball+4 magnets).

## 2. THE ACQUIRE ENTRY CLAMP (+26 ≤ 16, :63975-76) — LANDED

`sub_54520` (the one-shot acquisition scan) clamps the caller's +26
to 16 AT ENTRY, before the model switch — every ctor stamp above 16
(the possess lob's 200 from :65244, a high fireball charge) is cut
on the acquire tick. THE (9,1) f26 16→200 family = 234 rows + the
counter drift → 1 residual (t=5026, the (5,12) cluster's). Port:
clamp at `aim_assist_mc1_cone2` + `aim_assist_possess_mc1` entries
(all sub_54520 twins route through these). The ledger's earlier
"post-claim +26 stamp at the claim flash" hypothesis was wrong —
it's the acquire, spawn-tick, one comparison.

## 3. THE FOOTPRINT SWEEP SOFT-KILLS SCENERY (:51747) — LANDED

`sub_40E20`'s class-2 arm kills swept scenery via sub_41E80 =
flags |= 0x400 (one-snapshot corpse, tick-top reap), NOT an
immediate free. The port freed eagerly → the 39 lvl-4-upgrade-commit
trees at t=3855 vanished from the obs a snapshot early (retail
corpus shows them dead-flagged 0x2040C at 3856). One-line fix in the
footprint walk.

## 4. TERRAIN-SHADOW — COMPUTED CLASSIFICATION (223 rows / 81 pairs)

The mc1l0-terrain-z rule note predicted "the walker x/y/heading
knock-on stays UNexplained": measured at t=2978 ((5,3) flock, 55
rows) — every shadowed row rides a terrain-z-TAGGED z sibling on the
same slot in the same pair (the mover's ground diverged → its
ground-following motion diverged). New computed tag
`Tag::TerrainShadow` (id `terrain-shadow`, twin of pose-phase/
slot-desync): claims still-unexplained x/y/heading/pitch rows whose
slot carries a terrain-rule-tagged z row in the same pair. Take-wide
it claimed 223 rows across 81 pairs.

## ⚠ OPEN: THE t=1869→1870 MAGNET-556 ONE-TICK SKIP (9 rows)

At exactly pair 1869→1870, retail magnet 556 stamped NO balls (642's
f30 froze at 798, 732's at 1537) while magnet 88 stamped its
in-range set (670/695 turned toward 88 — bearing math exact); every
neighboring tick 556 stamps normally (balls track its bearing).
Eliminated: chain membership (static that tick), walk order (f63
clocks consistent, both magnets dispatched — 556's life ticked
89→88), range (6.4M < 12.8M), allocations (free stack IDENTICAL
across the pair — the recast spawn is later), chain severing (no
next-pointer writers outside the build; the :61540 hit is a font
struct), capture tear (pair passed capture_clean and is in the TSV).
The one dying entity (bolt 484 expiry, slot between 88 and 556) soft
-kills only. NO structural explanation found — possibly a retail
mid-walk chain-head interaction not visible in the decompile lines
read so far. 9 rows, banked.

## REMAINING UNEXPLAINED (48 field/extra + 6 pose)

9@1869 (above); 7@5026+3@5030 (5,12) rand family; 4@2811 ((2,0)
flags/life/rand + (10,6) extra); 4@2978 (5,3) residue (no z sibling
that pair); singles: 604 (9,1) chase/target_yaw, 1143 player.life,
1187 (3,2) life, (10,39) x/y pairs at 1233/1810/1824/1928/2345/2371,
2216 (5,1), 3005/3035 (9,0), 3×(5,4) heading @5051-59; pose.z
singles 567/1765-67/3879 + eff_pitch 3879.

Microscope NEW: `examples/castle_mail_probe_mc1.rs` — run ONE
verify-grade pair, print port mailboxes/launch-lane fields vs retail
(`--pair <t> <slot>…`, `--near x,y` scans pair-start entities);
`World::debug_mail` + `World::debug_launch` accessors back it.

# THE REPLAY-MODE CASTLE-3 RESCUE — RESOLVED: HYPOTHESIS A, CASTLE LAWS EXONERATED (2026-08-13)

**⚖ The player-ruled test ran and ruled A (gulp under-accrued), with
the mechanism fully measured. No castle/fire/mail law is at fault —
the free-run rescue is compounded pre-1700 state drift. Port
UNCHANGED except debug instruments.**

## THE TEST (replay-mode castle timeline)

New env probes in the `replay` MC1 driver (`MGC_KNOCK_TRACE` twins):
`MGC_CASTLE_TRACE=<t0>:<t1>` prints every retail (3,2) row beside the
port's live castles at each boundary — f70 / case machine (retail f48,
port f59) / f50 shake / level / life / ch0 mail; `MGC_SITE_TRACE=
<x>,<y>:<t0>:<t1>` prints both engines' full entity roster within 8
tiles of a site. Backed by `World::debug_castle_machine` (doc-hidden
twin of `debug_mail`).

## THE VERDICT — A, BY EXACTLY ONE 400-QUANTUM

Full-take replay (anchor t=0): the build-window mailbox accrues
**(19600, 80)** in the port vs retail's **(20000, 80)**. The establish
gulp leaves the port castle at life **400** (retail: exactly 0). The
vulture's −100 (src 562) is then not lethal; the ~1869 recast finds
the binding held and UPGRADES (lvl 2, life re-caps 40000) — the
player's exact account. The vulture keeps mailing the transforming
castle (mail (400, 562) pending by ~1893) and the settle swallows it
harmlessly. Hypothesis B disproven: the port castle sat in f59=4
(established) throughout 1809-1830, mail processing live.

## THE IGNITION-MAIL LAW (measured, both engines)

Each (10,0) corpse-flame mails its f44=400 into the castle's ch0
ONCE, on its FIRST dispatch. Verified: mail-delta@T = (#fires first
seen L8 @T−1) + (#fires first seen L7 @T) matches all nine accrual
boundaries on both engines exactly. So gulp total = fire census ×
400. **The ledger's "exactly 20,000" question is ANSWERED: 50 fires
× 400** (and castle 6's survival AT 0 is consistent — life 0 is not
< 0; exact-lethality is census luck, not a constant).

## THE CENSUS — 50 RETAIL vs 49 PORT (free-run only)

The 17 crushed worm segments (slots 80-96, identical crush @1766 both
sides) reap on the corpse 8-beat (`mob_corpse`) → each spawns a
(10,1) spreader (`corpse_puff`) → the spreader's per-cell ~50% skip
test (`rand % 157 >= 79`, combat.rs fire-ring) spawns the (10,0)
set. The skip stream is the SPREADER's per-entity LCG, seeded
`slot + global_rand` at alloc — so the census is a function of free-
stack order and LCG state. In the drifted full-run pool the puffs
land on different slots → the census reshuffles → 49. The dispatch
skew (retail 33 spawn-then-wait / 17 same-tick vs port 15/34) is the
walk-cursor law × allocation drift, not a law difference — pair mode
grades spawn slots and same-tick dispatch and is clean here.

## THE PROOF — 1700-ANCHOR FREE RUN IS TICK-EXACT

`replay --start 1700`: from a drift-free import the port free-runs
the ENTIRE story identically — census 50, gulp (20000, 80), life 0
@1809, vulture (100, 562) @1829, f70=6 @1830, downgrade + dead
0x40e @1831, castle 4 built at (148,104) slot 107. (First divergence
from the clean anchor: slot 562 — the vulture itself — target_yaw
422→430 @1701, a replay pose-feed nuance that did not perturb the
story; free-run set drift resumes @1795, also harmless here.)

## RULING + NEXT

The replay-mode castle-3 rescue is CLOSED as compounded free-run
drift seeded at the take's first horizon (pose.z t=563, sets t=568).
The only path to a conforming full replay is killing the earliest
horizon rows — the remaining-48 map + pose singles above — not
castle work. Castle-window pair rows stay as mapped (2@1808, 2@1810,
4@1830 landed earlier; 9@1869 banked).

## ADDENDUM (same session): THE SCORCH-DIG CHAIN MEASURED — mc1hw-fire-churn-rand's PARENT IS A LAW, NOT DRIFT

**Player re-escalation (correct): the 148 fire-churn-rand rows sit on
the worm-crush windows of castles 1/3/6 (47@564-574, 14@1768-1775,
10@2338-2346) + the 2978-3132 flock era — the same windows as the
terrain-z peaks, and the same family as the replay wall (pose.z
t=563 = castle-1 build-paint under the carpet). New microscope
`examples/scorch_gate_probe_mc1.rs` (one verify pair; recorded
terrain truth at BOTH boundaries vs the port's live planes at a
watched fire's cell; `World::debug_rand`) reproduces the masked row
live at pair 1768→1769 (retail rand 0x338ce391 = the TSV want, port
0x9af70f72 = the TSV got) and shows the payload:**

| cell | @N (both) | retail @N+1 (truth) | port @N+1 |
|---|---|---|---|
| (131,88) fire 555 | h33 ty3 | **h30 ty1** | h32 ty51 |
| (135,90) fire 667 | h30 ty3 | **h30 ty50** | h29 ty48 |

From an identical, truth-matching pair start, the port's scorch-dig
chain (gate → dig depth → burnt-type stamp → angle recompute) writes
DIFFERENT terrain than retail within one tick. The type bytes 48/50/
51-vs-1 look like scorch-variant stamps; the ANGLE plane is a gate
term with NO recorded truth (terrain channel = height/type/ceiling
only — measured blind spot) and is the first-flip suspect since
h/ty match at pair start (port angle 35@(131,88), 51@(135,90) pre).
The rand skew is downstream smoke: a flipped gate skips a draw AND a
dig; the skipped dig feeds the next fire's gate — cascading within
the pair and compounding in free run (this is the census driver that
rescued replay castle 3).

**NEXT (the terrain-phase dig, now concrete): read retail's fire
scorch body (:28085-28118 tail — dig_scorch sub_40D30 depth/count
per latch, the burnt-type stamp law, and the angle-nibble update on
dig) against the port's `dig_scorch`/`dig_cell` + type/angle bake;
fix at the exemplar (pair 1768→1769 cells above), then expect the
148 rand rows + big chunks of mc1l0-terrain-z (465) / terrain-shadow
(223) / the t=563 replay wall to move together.**

# THE SCORCH DIG CELL ROUNDING + TYPE-0 GATE LANDED (2026-08-13, same session)

**The scorch-dig chain dig above resolved to TWO retail-law fixes,
landed: mc1l0 3,094 → 2,961 rows (unexplained 54 → 50 — the banked
4@2978 (5,3) residue DIED), mc1l1 −627, mc1l2 −1,303, mc1l5 −3,008
(unexplained down on every take). 10/10 suites as-expected, 0
regressions (2 open drifts promoted: mc1l32 t=23132 LOST a rand row;
mc2l3 t=1309 residue reshaped). 504 mgc-sim green. Baselines
mc1l0/l1/l2/l5.tsv refreshed.**

1. **`dig_scorch` cell ROUNDING** (combat.rs): both retail chassis
   round the dig cell — `(x+128)>>8` (MC1 sub_40D30 :51705-06, MC2
   sub_572C0 EF:39722-23); the port floored. A fire in the upper
   half of a cell scorches the NEIGHBOR. Shared MC1+MC2 (the MC2
   fire already GATED on the rounded cell — gate and dig now agree).
   mc2_cave golden #4 + mc2_slice A-E re-pinned (state + OBSERVABLE,
   post-init/1-3 hold), each verified attributable by reverting the
   one line.
2. **MC1 fire gate: rounded cell + the `if (v5)` type-0 wrapper**
   (fire_tick :28075-98): the reaction cell lookup (type/angle/
   conversions) now rounds like retail, and a type-0 cell never
   scorches (the MC2 twin already had both — ⭐ "remc1 slips → diff
   the MC2 twin", textbook).

**Verification at the exemplar** (scorch_gate_probe, pair 1768→1769):
both watched fires' rand now retail-EXACT (0x338ce391 / 0xd735b721),
heights on truth (30/30). The 1768-era rand mask fell 14 → 3.

**REMAINING (the mask stays OPEN, 148 → 139 rows):** the burnt-TYPE
bytes still differ (rebake reads the ANGLE plane) and the 564-era
rows went 51 → 57: castle-1's window burns the same cells across
many consecutive pairs, and each pair-import instals a PRISTINE
angle plane — retail's accumulated scorch latches (angle&7==1) are
UNRECORDED (terrain channel = height/type/ceiling only), so the port
re-scorches cells retail latched in EARLIER ticks. That is an
INSTRUMENT gap (`install_measured_terrain` keeps level angle), not a
sim law: free run evolves angle correctly. Fix directions if ever
needed: record the angle plane in the recorder (new takes only), or
reconstruct latches from measured-vs-pristine height diffs (impure:
zero-delta scorches latch without a height change; painted cells
change height without a latch). The t=563 replay wall does NOT move
with this landing — its first single is the castle-1 PAINTER's
mid-tick phase under the carpet, the next family member to dig.

## ADDENDUM: THE t≈1300 UNCLAIMED-BALL REPORT (player) — DRIFT, ANCHOR-VERIFIED

Post-landing, the player's port replay leaves ONE vulture-drop mana
ball unclaimed in the castle-2 era (~t=1300) where it used to claim.
Measured (SITE trace + 1150-anchor bisector): pair mode in
1140-1380 is byte-identical to pre-fix (same 5 unexplained rows);
from a drift-free t=1150 anchor the port ball takes retail's SLOT
754, launches/settles at retail's exact positions and is claimed at
t=1230 tick-exact. In the full run the drifted pool allocates it at
slot 629 → different per-entity launch seed → rest point 2.3 tiles
SW of retail's → the recorded possess (aimed where retail's ball
sits) misses it. Same class as the castle-3 census: the scorch
landing reshuffled the free-run dice (terrain evolution from
castle-1's fires cascades allocation) — this window lost the roll
the old code won by luck, and vice versa elsewhere. No action; dies
with the earliest-horizon work (t=563 painter phase).

Sharpened en route: the (10,39) x/y SUB-PIXEL pair residue (1233 +
1810/1824/1928/2345/2371 — all castle windows, balls rolling on
just-terraformed ground) is the only ball-motion law residue pair
mode still shows — a real, tiny, terrain-coupled settle/roll lead;
unrelated to the unclaimed ball (wrong scale by 3 orders).

## 🏆 THE MID-WALK RESTRUCTURE LANDS — THE t=563 REPLAY WALL FALLS (the mover joins the entity walk)

**The pose.z t=563 replay wall is DEAD.** The named restructure (the
"NEXT WALLS" #1 lane of the castle-machine session) landed exactly as
prescribed: the human carpet's mover now steps INSIDE the entity walk
at the carpet's slot — retail's class-3 carpet-dispatch position — so
its ground probe reads terrain the lower-slot painters stamped THIS
tick. Over castle-1's rising tower the free-run carpet no longer lags
each paint step by one tick (the 904-vs-898 single and its t=567 ×5
terraform-window family, all gone).

**THE API:** `World::tick_flight(&mut FlightDrive, cmd)` beside the
untouched pinned-pose `World::tick(pose, cmd)`; both wrap the shared
`tick_inner`. `FlightDrive` = the driver-sampled tick-head channels
(input after the dead/falling override, the Accelerate override +
restore edge, the consumed knock, falling/dead) around `&mut
Mc1State`. At the carpet's walk slot `step_player_flight` runs the
mover (`flight::mc1_move` on the LIVE planes), the death-fall ride
(sub_45FC0) and the dead-camera turn (sub_463B0) — all carpet-
dispatch siblings in retail — then the walk's `player` pose, the
`MobCtx` and `human_pose` ADVANCE mid-walk:

- **Walkers below the slot read the record's PRE-move pose** (the
  rival brains, the token slots, every low-slot handler — retail's
  record law; the pose-phase tag's "other sample" made structural).
- **The wizard pass, walkers above, and the tick tail read the
  settled pose** — pair mode's proven `--pin-pose n1` shape.
- Native (slot-0) worlds step the flight at the post-walk wizard
  pass — retail allocates the carpet ABOVE the level entities.

**THE LANES:** conform replay `step_mc1` hands its chain through the
drive (mover/fall/camera code moved world-side); the app's faithful
MC1/HW path defers the same way (`Simulation::step` → `tick_flight`,
flyer derived after; extended-lift deviation now applies post-turn).
Enhanced flyer, world-less sims and MC2 keep the pre-tick move —
`step_mc2` and the MC2 walk are UNTOUCHED (never split MC2 along).
Pair mode is untouched BY CONSTRUCTION (drive = None is the old
single-pose body).

**RECEIPTS:**
- mc1l0 free-run horizon 562 → **564 boundaries**; channel firsts:
  pose t=563 → **t=566**, fields t=565, entity-set t=568. The
  castle-1 commit + first-work window now free-runs bit-exact.
- Post-fork traffic vs the recorded pre-fix benchmark: field rows
  1,469,581 → **972,072 (−34%)**, set atoms 96,212+356,706 →
  181,436+194,191 (452.9k → 375.6k, −17%), pose 18,351 → 19,934
  (reshuffled fork dice). The whole downstream story tracks retail
  substantially closer.
- mc2l3 free-run pose wall **t=244 UNCHANGED** (fields t=5, horizon
  4 — MC2 proven untouched). 10/10 fixture suites, 0 regressions,
  0 drifted. Workspace tests green incl. the state-hash goldens;
  clippy + fmt clean. Attribution: the pre-fix 562/benchmark numbers
  are last session's recorded measurements on an otherwise unchanged
  tree.
- First-recorded free-run baselines for the untracked takes (no
  prior numbers existed): mc1l1 horizon 343 (pose t=630), mc1l32
  seg-0 horizon 28 (pose t=29), mc1l2 horizon 0 (fields t=1: slot
  300 ctor mana 400-vs-1000 — an import-era family, no pose rows in
  the window), mc1l5 horizon 1 (fields t=2), mc1hwl0 seg-0 horizon 1
  (fields t=2). All pre-existing take-specific families, none
  pose-shaped.

**THE NEW HORIZON = the predicted family #2, verbatim:** pair
564→565 — the castle established-tick BALL COLLECTION (sub_46DB0's
every-other-tick block: sub_47130 ejector / sub_47400 + the
absorption loop). First rows: (10,39) balls' `target_yaw` retail 0
(the collection RESET) vs port stale 660/1021/1055/716 (slots
481/484/627/631), plus (10,0) z/rand rows (slots 633/634, z
876-vs-841 / 897-vs-870). Fresh family, undug — now first in line
for the full-replay campaign.

**PHASE LAWS DELIBERATELY KEPT AT THE TICK HEAD** (driver-side,
corpus-fit as-is; revisit only on corpus signal): the knock
consumption (armed knock always lands NEXT tick — MGC_KNOCK_TRACE's
measured shape), the Accelerate override sample + restore edge, and
the falling/dead flags (a death registered mid-walk moves the carpet
starting the FOLLOWING tick).

## 🏆 THE CORPSE-DROP f34 MIRROR FALLS — THE "BALL COLLECTION" FAMILY WAS A SPAWN LAW (2026-08-13)

**The predicted pair-564→565 "castle established-tick ball collection"
family is DEAD, and it was never the collection machinery.** The
horizon note guessed sub_46DB0's every-other-tick block (sub_47130 /
sub_47400 / the absorption loop) with a "target_yaw RESET"; the
record probe (`examples/ball_collect_probe_mc1.rs`) refuted that in
one dump: slots 481/484/627/631 do not exist at t=564 and appear at
t=565 as fresh (10,39) balls whose HEADINGS are exactly the port's
"stale" f34 values (660/1021/1055/716) — they are castle-1-era
house-demolition corpse drops, born mid-pair. Retail births them
target_yaw 0; the port's `corpse_drop_mc1` still carried the
spawn-arm mirror `f34 = yaw`. sub_27690 (:29663-92) verified line by
line: it writes +30 (heading draw), +126 (speed draw), +46 (signed
lift), zeroes +150/+152 — and never touches +34. The collection
machinery itself was already fully ported (owner echo +144, ejector,
sub_37150 re-apply, balloons, absorb — features.rs state-4 block).

**THE FIX:** one line — `corpse_drop_mc1` no longer writes f34
(motion-inert: the ball tick never reads it; MC2 never routes through
this drop). Pinned in `fireball_kills_and_the_corpse_drops_a_mana_ball`
(f30 nonzero, f34 == 0).

**THE 2026-08-01 "SPAWN-ARM f34 MIRROR" RULING IS RETIRED** (all
three `mc1-spawn-arm-f34-*` roster rules deleted; DEVIATIONS.md entry
rewritten as retired):
- Its rationale ("splitting the shared arm risks the homing paths")
  stopped applying when the thunk-muzzle law removed the mirror from
  `arm_projectile` — corpse_drop was the LAST survivor and is not a
  shared arm.
- The fire/flame rules' remaining hits (201 + 20 rows, mc1hwl0) are
  ALL misattributed SLOT-COLLISION rows — class 10-vs-9 pool
  mismatches (a port meteor/bolt wearing retail's fresh-fire slot)
  whose target_yaw member the rule swallowed; ditto the 5 surviving
  (10,39) rows in l2/l5. No fire/flame spawn site writes f34 anymore
  (whole-crate sweep). Those rows now surface honestly with their
  class/model siblings.
- Cast emissions (player + rival class-9) KEEP the mirror — retail
  writes the aim at emission (live (9,x) records carry nonzero
  target_yaw from birth; the pose-phase rows prove the want side).

**RECEIPTS:**
- mc1l0 verify-deltas 2,961 → 2,832 rows: the 129-row masked family
  at 0. mc1l1 −187 (all unmasked = unexplained there), mc1l2 −166,
  mc1l5 −484 unexplained target_yaw rows.
- Free-run mc1l0: post-fork field traffic 972,072 → **947,671
  (−24.4k)**; horizon HOLDS at 564 boundaries — the same pair also
  carries the (10,0) fire z/rand sub-family (separate root, below).
  mc2l3 free-run byte-identical (horizon 4, pose t=244, fields t=5).
- 10/10 fixture suites green, 0 regressions; mc1l32 2 drifts = pure
  improvements (open fixtures t=39509/39511 LOST their
  (10,39)/(10,1) target_yaw rows, gained nothing) — promoted.
- Workspace 732 tests green; goldens re-pinned WITH the fix's
  signature: flight-tier FAITHFUL B/C (ambient-death drop in
  window; ENHANCED holds), L005 D/E (combat-window kills;
  post-init..C hold, observable companions byte-exact). clippy+fmt
  clean.

**⭐ THE NEW HORIZON LEAD, MEASURED — the (10,0) fire z/rand pair at
564→565 is a DRAW-PARITY / EARLY-TICK shape, not seed law and not
(only) angle churn:**
- Port `new_event` seeds children `rand = slot + global_rand` RAW
  (no step) — retail-shaped.
- Retail's fire at the 565 snapshot reads rand = **lcg¹**(slot+G)
  (slots 633/634: values differ by exactly 9377 = consecutive
  integer seeds, seed−slot identical 3716259035). The port's reads
  **lcg²**(slot+G) — `port_rand = lcg(retail_rand)` EXACTLY, both
  slots. One extra draw on the CHILD's stream between birth and
  snapshot.
- The SAME fires carry z 876→841 / 897→870 (retail keeps spawn z,
  port sits lower ≈ ground) — one cause covers both if the port
  runs the fresh fire's first handler tick a tick early (ground
  clamp + first-tick draw), or its spawn path draws once on the
  child. Fresh corpse-drop BALLS in the same windows show NO rand/z
  skew — whatever it is, it is fire-lane-specific, not the walk.
- Next probe: find who advances a fresh (10,0)'s stream/z in the
  spawn tick — the fire's first-tick sound/flicker draw, the
  spreader's child handling, or an eager z clamp in the spawn path.

**ADDENDUM — the horizon pair probed (scorch_gate_probe, pair 564,
slots 633/634): the fire z/rand rows split into TWO measured leads,
and the "instrument, not law" attribution is HALF-REFUTED for free
run:**
- **Slot 633, cell (116,95)**: terrain truth MATCHES port at both
  boundaries (pre 27/3, post 27/51 — the type-51 stamp conforms).
  Retail drew once (flicker only — its scorch gate REFUSED: the cell
  is angle-latched in retail); the port drew twice (pristine angle 3
  → re-scorch; the `d%7` depth drew 0 so no height moved — rand-only
  residue). ⚠ FREE RUN shows the identical lcg² — the port's OWN
  accumulated angle plane also lacks the latch at 564, so retail has
  a LATCH WRITER (or an earlier scorch the port's gate refused) that
  the port does not reproduce from a clean t=0. The 139-row
  fire-churn family is a REAL dig again, not just the pair-mode
  angle-import gap. First probe: enumerate retail's `angle |= 1`
  writers (the dig chain writes it identically in both engines —
  sub_40BC0 family vs features.rs dig_cell) and find the earlier
  tick where the two engines' scorch decisions at (116,95) parted.
- **Slot 634, cell (115,96)**: a live WITHIN-TICK TYPE-STAMP law
  divergence from a truth-matching start — heights agree (28→30,
  the crush-window raise conforms) but retail paints type 60→75
  where the port paints 60→93. One cell, one tick, both engines
  from retail's own planes: the burnt-type/footprint-repaint law
  (crush-window painter stage row or corner-orient pick) is wrong
  in the port. This is the sharpest reproduction yet of the
  "burnt-TYPE rebake residue".
- The fire z rows ride the same machinery (spawner z phase +
  neighbor-corner digs), and fresh corpse-drop balls in the same
  windows show NO skew — the whole remaining 564-era block is the
  scorch/paint lane, not the ball lane and not the walk.

## 🏆 THE SCORCH DISC LAW — BOTH 564-ERA LEADS DIE IN ONE LAW (2026-08-14)

**The fire's scorch was never a single-cell dig. `sub_40D30(expl, 0,
0, -depth, 1)` walks RING 0 of the SEARCH.DAT table — the 2x2 zero
block minus the ring walker's dropped last cell — i.e. THREE cells per
scorch: the rounded center, (+1,0) and (0,+1), each getting the FULL
cell update at ANY depth, zero included.** The port's `dig_scorch` dug
one cell and skipped `delta == 0` outright; retail (MC1 sub_40A10
:51621-89, MC2 sub_56F10 EF:39499-609 — neither has a zero-delta
early-out) writes the height back unchanged, sets the `angle |= 1`
LATCH and runs the flag-mode restencil/retile on every disc cell. Two
port bugs, one retail law:

1. `dig_scorch` now routes through `dig_disc(i, 0, 0, delta, true)`
   (the already-faithful sub_40D30 port — `dig_disc_pub` wrapper).
   The fire's f80=128 clamps hi to ring 0 exactly like retail.
2. The `delta == 0` early-return is gone: a zero-depth scorch still
   latches + restencils/retiles all three cells, so the fire gate
   (`angle & 7 != 1`) refuses later re-scorches there — retail's
   "invisible" latch writer that the (116,95) lead was hunting.

**HOW IT FELL (pair 564→565, the horizon pair):** the record probe
grew a whole-plane pre/post cell diff (port-vs-truth h/ty per changed
cell). Retail's dig set at the pair decoded via the birth-rand law
(child rand = slot + G raw; G = 3716259035 here): fires 632-637 born
mid-pair from corpse-flame spreaders, walk order = slot order.
- 632 (cell (116,96)) drew depth **0** → zero-depth disc latched
  (116,96)+(117,96)+(116,97) and restenciled — types 3→50/48/48. That
  latch is why retail REFUSED 633's re-scorch at (117,96) and 634's at
  (116,96) (both drew flicker only, rand = lcg¹(birth)) while the port
  scorched: the lcg²-vs-lcg¹ churn-rand shape, closed.
- 635 (cell (114,94), depth 5) dug (114,94)+(115,94)+(114,95) — the
  three −5 truth digs, two previously TRUTH-ONLY. 636 (cell (113,95),
  depth 2) dug (113,95)+(114,95)+(113,96): with the painter's +2
  flatten the arithmetic lands exactly ((114,95): 32+2−5−2 = 27 ✓).
  637 (cell (116,90), depth 6) dug the remaining two truth-only −6s.
- The (115,96) "type-stamp law" lead (60→75 retail vs 60→93 port) was
  the SAME law: 75 is the retile product when 632's zero-depth disc
  has restenciled the neighborhood first — the corner codes feeding
  the retile table differ without it. Post-fix the whole pair's cell
  diff is BYTE-PERFECT (every dig, every retile product incl. 75 and
  the 61→94/90 stamps), and 633's rand AND z conform exactly.
- Also decoded en route: the 565-era type stamps (48/50/51/75/94) are
  the FIRES' dig-recompute retiles, NOT the castle painter's paint —
  painter 628 sat at f26=16 during the pair (paint fires at 14/7/1;
  the mass ty-26 paint is truth-567). Its flatten ran every work tick:
  ramp (goal−h)/f26 with goal = 4·(lo−1)+target fits the truth ramp
  exactly. The painter had been SHAKING (f50, :30520) through 554-562,
  which is why work started at f26=9 — the counter decrements through
  suspension.

**RECEIPTS:**
- mc1l0 pair mode: TSV 2,832 → 2,786; `mc1hw-fire-churn-rand`
  139 → 112. The remaining 112 are the ANGLE-IMPORT instrument gap
  (pair mode re-imports PRISTINE angle each pair — retail's
  accumulated latches are unrecorded), proven by free run:
- mc1l0 FREE RUN: clean boundaries 564 → **1091**; **rng NEVER forks
  (0/7097 boundaries — was t=6295)**; pose first 566 → 1128;
  entity-set first 568 → 1295; post-fork field traffic 947,671 →
  **594,966 (−37%)**, sets −73%. First fork still pair 564→565 but
  down to TWO rows: slots 635/636 z (retail 967/1056 vs port
  915/1008) — the corpse-flame SPREADER SPAWN-Z lane (floating fires
  expose the parent spreader's z where ground-clamped ones mask it;
  Δ≈50 ≈ the crush window's flatten step) — the new lead.
- mc2l3 free run BYTE-IDENTICAL to baseline (horizon 4, fields t=5,
  pose t=244). mc2l30 fixture t=33 went CONFORMING (promoted);
  mc2l3 t=1309 open fixture lost its z row (drift promoted).
- 10/10 suites green, 0 regressions. mc2_cave #4 + observable and
  mc2_slice A-E + observable re-pinned, each REVERT-attributed
  (cave: disc alone moves it; slice: needs the zero-depth latch too —
  both halves are one law). Workspace green, clippy+fmt clean.
- l1 −43 rows; l2/l5 flat (their blocks are other families).
  Repo-root l0/l1/l2/l5 TSVs refreshed 2026-08-14.

**NEW PROBES:** `examples/terrain_cell_history_mc1.rs` (truth-channel
cell history — when did retail's h/ty at a cell change),
`examples/slot_dump_mc1.rs` (raw record slot-range dump per tick),
and `scorch_gate_probe_mc1` grew the whole-plane pre/post cell diff.
⚠ Truth-channel phase in scanners: the image holds terrain@(t−1) at
record t — label shift of +1 vs pair-probe "post@" lines.

## 🏆 THE FIRE'S PRE-DIG GROUND — THE SPREADER SPAWN-Z LANE DIES (2026-08-14)

**The fire's z rule never sees its own crater. Retail sub_24F60 takes
ONE ground sample (`v3 = sub_11F50`, :28073) right after the life
test — BEFORE the first-active terrain reaction — and passes that
stale value into both the scorch gate (`z − v3 <= 128`) and the z
rule (sub_42000_42340, :28116). A fire that digs its own cell still
gates and clamps against the PRE-dig ground.** The port sampled
ground after the first-active block, i.e. post-dig. Corroborated
across binaries: remc1 :28073, remc1hw :26616, and the MC2 twin
sub_30D50 (EF:22711 `v3 = getTerrainAlt_10C40` → EF:22752
`sub_580E0(pos, v3, ...)`) — a different decompile lineage. The
port's `mc2_fire_tick` already had it right (mobs.rs samples before
the block); MC1's `fire_tick` was the outlier. One-hoist fix; the
sample also feeds the gate (one call, used twice, retail-exact).

**HOW IT FELL (the spreader spawn-z lane, pair 564→565):** the
predicted "corpse z sampled pre-vs-post the painter's raise" was
wrong upstream — the whole corpse chain CONFORMS. Slot archaeology:
three worm chains (61-77, 99-113, 119-135) crushed mid-pair 562→563
by painter 628's footprint kill; corpses puff one per phase tick;
spreader 476 spawned at corpse 74's exact (113.42,94.21) z=946 ✓.
Fires 635/636 spawn AT spreader z 946 in both engines. The fork is
the fires' own FIRST tick: 635 dug −5 at (114,94), 636 dug −2 at
(113,95) — then the port re-sampled the lowered ground while retail
kept v3. Bilinear reconstruction is EXACT on both sides: 636 retail
1056 = pre-dig corners (113,95)=34 → 1055.7; port 1008 = post-dig
(113,95)=32 → 1007.7; 635 retail 967 = pre-dig 967.04 (clamp-up),
port 915 = 946 + f46(−31) after the flicker branch flipped (post-dig
ground 869 < spawn z). Same f46 = −31 confirmed by retail's own
t=566 step (967−31=936). Fire 637 conformed because it floated above
even the pre-dig ground — both engines took the flicker branch; the
clamp-direction flip is what exposed 635/636. Δ≈50 was one painter
ramp step ONLY in the sense that the painter had just raised the
neighborhood the fires then dug back down — the sample-order bug is
the law.

**RECEIPTS:**
- mc1l0 pair mode: TSV 2,786 → 2,758 (−28: 36 rows gone, 8 converged
  toward want, ZERO new — wins across the whole corpus: t=1768,
  2338-2346, 3121-3125, 3215, 4288, not just the 564 window). First
  divergent pair 564 → 565 (the remaining 565+ block is the pair-mode
  angle-import instrument; churn-rand floor stays 112 exactly).
- mc1l0 FREE RUN: bit-exact horizon 564 → **604** (+40), clean
  boundaries 1091 → 1106, fields first 565 → 605, rng still 0/7097.
  The crush-window fire cascade (564-567) conforms end to end.
- l1 13,769 → 13,734 (−35); l2 31,564 → 31,551 (−13); l5 162,621 →
  162,492 (−129); every replaced row is a (10,0) z convergence.
  Free-run forks unchanged (l1 344, l2 1, l5 2, l32 29, hw 2 — the
  import-era families). mc2l3 free run BYTE-IDENTICAL (MC2 fire
  untouched). Repo-root l0/l1/l2/l5 TSVs refreshed.
- 10/10 suites, 0 regressions. mc1l32 t=465 open → CONFORMING
  (promoted); mc1l5 t=2872 open fixture lost its (10,0) z row
  (drift promoted, f26 row remains). Workspace tests green incl.
  state-hash goldens; clippy+fmt clean.

**⭐ NEXT DIG — the (9,1) MUZZLE-ACQUISITION PICK (measured, undug):**
new free-run first fork = pair 604→605, slot 642: the (10,39) ball
despawns mid-pair, a fresh (9,1) rival bolt (life 9, f26=16) spawns
into the freed slot at (126.07,93.23) — and the PORT'S bolt acquired
a DIFFERENT target: chase retail 104 (a worm-2 corpse slot) vs port
714, driving heading 1351-vs-1244, pitch 85-vs-120, target_yaw, and
the x/y/z muzzle offsets. rng never forks (0/7097) — the acquire
scan's candidate pick differs deterministically. Suspects: the
sub_54A90 distance-weighted scan over a field full of corpses/fresh
fires ("freed slot ≠ empty slot — free clears +64 ONLY"), or the
scan's walk order vs the port's. The one-shot-acquisition and
Acquire-scan ledgers are the prior art.

## 🏆 THE SEVERED BALL CHAIN — A MID-TICK SLOT REUSE CUTS THE TICK-HEAD LIST (2026-08-14)

**Retail's per-tick model lists are singly linked THROUGH the entity
records (the tick-head build at :52246-52296 chains `var_u32_36462[]`
node-to-node in ascending slot order; the possess acquire walks
list[1] at :64043 via `k = *(_DWORD *)k`). A record freed and REUSED
mid-tick gets its link wiped by NewEvent's ctor — the chain is
severed at that node: every list walk later in the same tick sees
the prefix, the reused node itself (with its NEW bytes), and nothing
beyond. A plain free never severs — "free clears +64 ONLY", the
freed-slot stale-bytes law's other half.**

**HOW IT FELL (the (9,1) muzzle-acquisition pick, pair 604→605):**
the "rival bolt" was the HUMAN's — wizext wiz0 charge 4→1 at t=601
and t=605 = possess casts at 600→601 and 604→605 (probe
`rival_ai_dump_mc1`). Slot archaeology: bolt 61 (cast 600→601 into
the OLD bolt's projectile slot, scanned on its first tick 601→602)
chased ball **714**; bolt 642 (cast 604→605 into collected ball
642's slot, scanned same tick) chased ball **104**. Same muzzle
family, same aim, near-identical ball field — the exact ported
scorer (trace `MGC_ACQUIRE_TRACE`) ranks 714 at 16.5M vs 104 at
36.6M from EITHER aim, and the port's would-be snap for 104
(heading 1351, pitch 85) matches retail's recorded bolt EXACTLY. So
retail's scan at 604→605 never SAW 714/643/690 (every candidate
that outscores 104 — all slots > 642), yet bolt 61 three ticks
earlier saw and took 714 (slot > 61). The one split that fits both:
the candidate roster ends at the bolt's own REUSED slot. Chased-ball
and falling-ball exclusion theories both died on measurement (714
was falling at scan A and still taken; nothing chases 643/690).

**THE PORT:** `TickChain` grew a `cut` (visible-prefix length, reset
at the tick-top rebuild); `Gen::new_event` lowers it when a popped
slot binary-searches into the chain (sever at reuse — free alone
does not). `aim_assist_possess_mc1` now walks the severed chain for
MC1/HW (retail's list gates verbatim: +144/+58 ALONE — no class,
life, or reap-mark test, so a chain member that died mid-tick stays
a stale-byte candidate and a mid-tick-spawned ball is invisible),
then the m45 dwelling list (live walk, conservative gates — m45
records never reuse mid-tick in the corpus). The magnet stamp's
chain walk inherits the cut (same retail list). MC2 keeps the
live-pool fallback — its list law is unmeasured.

**RECEIPTS:**
- mc1l0 pair mode: TSV 2,758 → 2,741 (−17, ZERO new): the whole
  604 family (chase/heading/pitch/target_yaw/x/y/z), PLUS two
  unhunted ball families — t=1869 (castle-3 teardown: ball
  heading/x/y at slots 642/670/695/732) and t=2345 — other
  consumers of the same severed chain.
- mc1l0 FREE RUN: bit-exact horizon 604 → **605**, clean boundaries
  1106 → 1125, fields first 605 → 606, rng 0/7097 (never forks),
  pose first t=1128 (unchanged — the vulture-attack lane, see NEXT).
- l1 13,734 → 13,727 (−7, 0 new); l2 31,551 flat (0 churn); l5
  162,492 → 162,338 (−154; +6 value-churn rows inside two
  already-divergent pairs: 5773 is a known slot-desync pair, 7078
  collapsed 5 rows → 1 target_yaw residue — retail's bolt there has
  chase set but +34=0, the rebound-handed-victim signature, a
  different lane).
- 10/10 suites, 0 regressions; mc2l3 free run BYTE-IDENTICAL
  (horizon 4 / fields 5 / pose 244). New conforming fixture t=604
  added to mc1l0.json (493 total) + bundle refrozen. Unit pin
  `a_lob_reusing_a_ball_slot_scans_only_the_severed_chain_prefix`
  (non-vacuous: removing the sever picks NEAR above the cut).
- New probes: `examples/rival_ai_dump_mc1.rs` (wizext AI lanes +
  carpet aim per tick), `examples/targeter_scan_mc1.rs` (who
  chases/claims whom).

**⚠ OPEN EDGES OF THE LAW:** (1) the sibling lists — class-3
(:36462[0], the fireball/wizard scans), class-9 (:36462[3]), the
per-model creature buckets, and the m45 dwelling list — share the
same linked-through-records build and MUST sever identically in
retail; unported (no measured row demands it yet — the earlier
"tick-start list snapshot RULED OUT" barrage measurement only
covered the no-reuse case). Sweep when a row asks. (2) A lob whose
heading sits within the ±0x71 cone of bearing 0 scores its OWN
record at dist 0 when it reuses a listed ball slot (retail
arithmetic — angle_between(self,self)=0); faithfully reproduced,
never corpus-visible. (3) The recorder's "rand" u32 at +0 vs the
decompile's link-at-+0 walk idiom — the record layouts disagree by
one field somewhere; the observable law doesn't depend on it, but a
layout audit would tidy the map.

**⭐ NEXT DIG — the t=1128 POSE fork = A VULTURE ATTACK (player-
identified 2026-08-14):** the free-run pose lane first forks at
t=1128, pose.x retail 10463 vs port 10473 (|d|=10) — the player
reports this moment is a VULTURE attacking the human carpet. A
knock/buffet delta on the human is the natural suspect (knock
channel v_22/v_24, the buffet law, or the vulture's dive contact
timing). The fields channel forks earlier (606) — reconcile which
lane to dig first at session start.

## ⭐ THE t=606 GULP QUANTUM — RECONNAISSANCE (2026-08-14, same session, undug)

**The new free-run first fork (t=606, castle 486 life −48800 vs
−49600) is the establish-gulp consuming a Δ=800 mis-accrued ch0
mailbox — and the delta is NOT the fires' census: it is the PLAYER'S
AT-CASTLE DAMAGE REDIRECT.** Pair mode conforms throughout (mail is
obs-silent; each pair imports retail's accumulated mailbox) — this
lane is free-run-only, the castle-3 story's shape at a now-clean
anchor.

- `MGC_CASTLE_TRACE 560:610`: both engines accrue castle 486's ch0
  from t=565, final retail (68800, 97) vs port (69600, 97) pending
  by t=575, consumed 605→606 (life 20000 − gulp). Per-tick deltas
  wobble (the walk-cursor dispatch skew, harmless); the TOTAL is
  the bug.
- New sink trace `MGC_MAIL_TRACE` (mail_write/mail_write_single →
  Pool(486) ch0): the port's 69600 = **158 × 400** (every (10,0)
  first-active post — same fires both engines, obs-bit-exact
  through 605) **+ 5 lumps 1200/1200/800/2000/1200 = 6400** with
  worm-lineage srcs (61/119) — the at-castle redirect forwarding
  the human's pending fire damage (world.rs :55353-62 arm).
- Retail's lump total must be 5600 — and `mail_write_single`'s own
  doc pin for this exact window records retail's sequence
  **1200/800/1200/400 (+2000) = 5600**. One lump differs: retail
  400 where the port forwards 1200 — two 400-fire posts on the
  player that retail's pipeline drops.
- **Suspect law:** retail's redirect forwards and LEAVES the player
  mail armed (:55357-60); the grace memset (:55367-71, armed by
  grace=2) clears it later — posts landing between forward and
  memset die. The port forwards-and-clears immediately, so those
  posts survive into the next lump. Also audit: the redirect's
  protocol vs sub_12B50 (the single-write pin), at_castle geometry
  per tick, and which wizard-tick arm (grace gate vs intake) runs
  first. Traces are committed env-gated (`MGC_MAIL_TRACE`,
  `MGC_AREA_TRACE` on the area pre-pass).

## 🏆 THE PLAYER TILE-WINDOW + THE WIZARD-TICK BODY ORDER (2026-08-14)

**Three laws in one session; mc1l0 free-run horizon 605 → 1188
(fields 606 → 1188, pose 1128 → 2985, rng still 0/7097).**

### Law 1 — the player probe carries the tile-scan window

The t=606 gulp-quantum recon was WRONG about the eater: retail eats
nothing at the redirect. The tick-stamped two-sided mail ledger
(MGC_MAIL_TRACE tagged AREA→castle/AREA→player/REDIRECT, interleaved
with MGC_CASTLE_TRACE via line-buffered merge) shows the castle
ignition stream identical every tick (157 posts — the recon's "158"
had miscounted a 400-lump) and the player-post stream identical every
tick EXCEPT t=568: port 5 posts, retail 3. The recorded carpet-record
residues (the boundary mailbox, decoded straight off the wizard's
pool slot 630) pin retail's per-tick truth: 1200/1200/800/1200/1200/
400 armed at boundaries 565-570, each forwarded by the NEXT tick-head
redirect — the port's post-walk block forwards same-tick, the
walk-cursor skew, total-preserving and harmless.

The discriminator at t=568: retail has NO special player arm at all.
The carpet is a pool record linked at its plain `(x>>8,y>>8)` tile,
reachable only through the windowed tile scan — and fires 692/694
overlap the carpet's AABB while their ch0 one-tile-back 3×3 windows
stop at x 114..116, one short of carpet tile (117,96). Hand-checking
every window in 565-570 reproduces retail's residue sequence exactly.
The port's out-of-pool player probe posted on pure AABB; it now
requires the carpet's map tile inside the writer's window (`area_write`,
MC1/HW only — MC2's probe is unmeasured and keeps pure AABB). Unit pin
`the_mc1_player_probe_carries_the_tile_scan_window` (the measured
t=568 pair verbatim). Free-run only: pair mode imports the mailbox
and mail is obs-silent — all four TSVs byte-identical under this law
alone. Horizon 605 → 1128.

### Law 2 — the knock is consumed at the move, IN-dispatch

t=1128 pose fork = the player-identified vulture attack: slot-533
(5,1) dives, melee 100 → carpet mail; retail's boundary shows the
intake processed it SAME tick (residue (100,0), stall 15, danger 99)
with knock (6,1659) — armed at 10 = amt/10 (sub_46540 :55719-24, dir
= attacker bearing; +26 pitch lane exists, unmodeled) and ALREADY
DECAYED BY THE SAME TICK'S MOVE (10 − 4 = 6; next boundary 0 — the
port's `take_knock_step` maths were exact, only one tick late).
Function-boundary math places the knock consumption (:55204-18) and
stick-filter apply (:55143-44) inside sub_455D0 = THE MOVE, called at
the TAIL of sub_45C90 — the mailbox block precedes the move.

Port restructure (`tick_inner`): the whole wizard-tick body now runs
at the MC1 carpet's walk slot in the order mailbox block → move →
cast pass → regen block, with the knock sampled inside
`step_player_flight` (FlightDrive lost its pre-sampled `knock`
field). ⚠ THE CAST PASS STAYS POST-MOVE: the raw listing order
(46840 before 455D0) was tried first and collapsed the horizon to 63
— the t=64 bolt (slot 627) stamps heading/pitch/speed 415/40/400 =
the SETTLED pose, so the arm → next-frame token fire double-buffer is
what makes the effective cast phase post-move. Corpus beats listing.
Horizon 1128 → 1143, pose lane clean to 2985.

### Law 3 — the life-regen rate is a one-tick-stale REGISTER

t=1144: castle 663 establishes (var_50 latches during 1143 — the
castle's own slot is ABOVE the carpet, so the wizard tick first sees
it at 1144), retail regens +5 at 1144 and +40 only from 1145. Retail's
regen tail (:55381-421) applies u16_341 FIRST and re-selects it after
(:55388 vs :55414-20) — the rate applied at tick N was chosen at N−1,
exactly like the mana delta (+132 apply-then-select, which the port
already had). Port: `Player::life_rate` register (hashed only once
the castle rate has latched — the death_owned_blue transparency
shape; snapshot v8 → v9), seeded per pair from the recorded wizext
+341 (new decode). MC2 keeps fresh-select (its register lane is
undecoded; unmeasured). Horizon 1143 → 1188.

### Verification

- 506 tests green; state_hash D/E re-pinned LAYOUT-ONLY (OBSERVABLE
  byte-for-byte identical — the moved words are the reordered
  intake/knock phase + the register joining player state).
- 10/10 suites, 0 regressions; 5 open fixtures drifted, ALL strict
  improvements, promoted: mc1l5 t=86/1199/2477/5742 lost their
  `wizard0.charge` (+`9,0:f26`) rows — the charge meter's in-dispatch
  phase — and mc1l32 t=23132 lost `player.life` + `3,0:life` (the
  staircase seeding).
- TSVs (baselines refreshed at root): mc1l0 2741 → 2738, mc1l1
  13727 → 11984 (−1743), mc1l2 31551 → 29187 (−2364, +2 pose-phase
  f26 rows), mc1l5 162338 → 155701 (−6637; the charge lane 4496 → 29
  rows). l5's 27 charge newcomers cluster at t=11372-16151 — a
  residual sub-lane, likely the same window as the (9,1) possess
  block. mc2l3 replay byte-profile = baseline (4 / t=5 / t=244).

### ⭐ NEXT: t=1188 — the castle ch5 work-mail lane

First fork now: castle 663 life retail 19100 vs port 19000 at t=1188
(the pre-upgrade tick; both level to 2 at 1189). Retail's boundary
shows ch5 mail residue `(10, 0)` (amount 10, src consumed) standing
from 1188 on — the port posts NOTHING on ch5. Retail −100 vs port
−200 out of the same 19200: either the port double-lands a 100-hit or
retail's ch5 protocol (the m41/m42 build-worker lane? slot 754
(10,42) is on site) offsets 100. Undug; the castle trace covers it.

## 🏆 THE GRACE RE-ARM + THE SOFT-DISABLED BALL + THE SHAKE OFF-BY-ONE + THE OVERLAP BAND (2026-08-14, evening)

**Five laws; mc1l0 free-run BIT-EXACT horizon 1188 → 2175 (fields
1188 → 2175, entity-set 1295 → 2175, pose 2985 and rng 0/7097
unchanged).** Every law is free-run-only or better: the four TSVs
lost 291 rows net and no golden moved except two strict promotions.

### Law 1 — the at-castle tick re-arms grace and wipes the box (t=1188)

The banked "castle ch5 work-mail" fork was neither a ch5 offset nor
a double-landed hit. Retail's ch5 intake (:56707-11) reads and
clears ONLY the source word — the amount is never read and never
cleared, so the upgrade token's `{10, owner}` receipt stands as the
permanent `(10, 0)` residue (now port-matched byte-for-byte, and the
ch0 LETHAL arm likewise keeps its amount, :56695-97 vs :56703). The
real −100/−200 fork was the SNOWBALL: `grace = 2` sits in the
at-castle branch OUTSIDE the pending-ch0 check (:55363; MC2's twin
`AddPlayer03_00_5E010` is identical, engine-shared, no gate), so
every tick at home memsets the whole 36-byte box (:55367-71). The
recorded boundary residues prove it: the t=1128 melee residue
`(100,0)` stands to t=1143, castle50 latches at 1143, and from 1144
the box is clean with grace pinned at 1 forever. The port re-armed
grace only inside the redirect, kept the stale 100, and the t=1188
melee accumulated (`mail_write_single`'s law) into a 200 forward.
Pin `the_at_castle_tick_rearms_grace_and_wipes_stale_mail`.
Horizon 1188 → 1234.

### Law 2 — a soft-disabled ball still runs its final dispatch (t=1234)

Ball 754 (claimed to the player at 1227 by the collector bolts)
matches to 1233, then retail slides (−14,−11) at 1234 and the port
froze. The castle absorb (:56022-36, walking the tick-head BALL
LIST, gates model+owner+overlap only) banks the ball from slot 663
and calls sub_41E80 = `flags |= 0x400` ALONE; MC1's dispatch loop
gates on class alone (:52351-53, f63++ post-dispatch — retail's
ball f63 24→25 at 1234 proves the dispatch ran). The port's
whole-tick 0x400 early-out in `ball_tick` was a fossil of the
soft-kill merge era (the merge hard-frees since the sub_277D0 law);
it is now MC2-scoped (where it models EF's unlink-at-disable).
Receipts: mc1l0 t=2812 fixture FIXED+promoted, mc1l5 t=1083 drift
promoted (its 10,39:z row died), TSVs mc1l1 −26, mc1l2 −46, mc1l5
−219 net. Pin `a_soft_disabled_ball_still_runs_its_final_dispatch`.
⚠ Residual sub-lane: 3 new l5 (10,39):z rows (t=7708/13194/13972),
one 32-unit z-step each — the flagged ball's final-tick z arm
differs somewhere (tether? balloon collect order). ⚠ Kept port-only:
`ball_merge_candidates`' 0x400 exclusion — retail MC1's sub_11D10
has NO disable gate, so an absorb-then-merge same tick would DUPE
the mana (castle + survivor both take f140); undug, no corpus row.
Horizon 1234 → 1295.

### Law 3 — the blast shake counts down to ONE before the repaint (t=1295)

Retail (:55983-99) is CHECK-then-decrement: the f50==1 tick
transitions to the repaint (f70=5/f48=3, f50 zeroed, NO decrement),
so the boundary shows 1 for a full tick; >1 ticks only count down
(that arm is what the wrapper's pin census had ALREADY decoded as
"pre50 >= 2"). The port decremented first and spawned the (10,42)
repaint painter one boundary early — the entity-set fork after
self-destruct #1's downgrade (life −1 at 1289, lvl 2→1 at 1290,
shake armed 5). Pin
`the_blast_shake_counts_to_one_before_the_repaint`.
Horizon 1295 → 1828 (+533: the off-by-one poisoned every repaint).

### Law 4 — the at-castle test is sub_11950 = the FULL overlap (t=1828)

The Δ=900 mana fork (retail +1000, port +100 for one tick) at the
recast castle 533: retail's `bool1` is sub_11950 → sub_118C0, the
3-axis overlap with SUMMED extents and strict `<` — the carpet's own
~half-tile extent band counts. The port's `regen_boost` used a bare
2-axis `<=` against castle f80/f82 alone; at t=1827 the carpet
hovered exactly in the missing band. Now `player_overlap` (the same
predicate the creature scans run, and the dolmen leg already used).
Feeds mana rate, life rate, grace re-arm and the redirect in one
place. Horizon 1828 → 2175.

### Verification

737 tests green (3 new pins), 10/10 suites, 0 regressions, 2
promotions (both strict), fmt+clippy clean. TSV baselines refreshed
at root: mc1l0 2698 (was 2737), mc1l1 11957 (−26), mc1l2 29140
(−46), mc1l5 155481 (−219 net, +3 stragglers above). mc2l3 replay
byte-profile = baseline (4 clean / fields t=5 / pose t=244).
Free-run traffic after: sets 43098/46537, fields 365742, pose 11208
rows — all downstream of 2175.

### ⭐ NEXT: t=2175 — the FAILED-upgrade accounting lane (⚖ PLAYER-RULED 2026-08-14)

First fork now: retail casts the castle spell at 2175 (mana
28848→13088 ≈ −10,100 debit + the (9,10) upgrade ball at slot 713);
the port refuses (no ball, no debit — mana byte-equal until the
cast). **Player ruling: the cast was USER ERROR in the take** — cast
from a DIFFERENT LOCATION, not realizing a castle stood, intending
to PLANT a new one; the very next recorded command is the DEMOLISH
(clear the site, rebuild). The player expects retail did NOT level
the castle either, so the deviation to hunt is ACCOUNTING: retail
lets the cast FIRE and eats the mana on failure — which is exactly
the already-decoded token protocol (:31040-44: every armed path
frees the token the same tick; MISS → release the owner's m16
charge pin, no level, mana gone) — while the port's command-site /
`mc1_token_gate` refuses up front and keeps the mana. Find the
too-strict gate, then verify the miss arm end-to-end: debit timing,
the ball's flight to the wrong site, the token's overlap miss
against the bound castle, the charge-pin release. Castle 533 = the
recast at (132,90), lvl 1, **life 0** (retail-true, matched both
sides). The castle trace + a mana-lane dump cover it. ⚠ The pose fork t=2985 stays
PLAYER-RULED DOWNSTREAM NOISE — do not dig the pose lane.
⚠ Side find for an MC2 session: EF's `AddPlayer03_00_5E010` shows
apply-then-select for the life register too (`life +=
lifeRegen_355` BEFORE the tail re-select) — the port keeps MC2
fresh-select only because the recorder can't seed the register.

## 🏆 THE UNFINDABLE CHARGE PIN (2026-08-14, night)

**One law; mc1l0 free-run BIT-EXACT horizon 2175 → 2217 (+42:
fields 2175 → 2217, entity-set 2175 → 2441, pose 2985 and rng 0/7097
unchanged; post-divergence traffic sets 43098/46537 → 36688/44518,
fields 365742 → 348741).** ⚖ PLAYER-RULED up front and confirmed to
the letter: "as long as you have enough mana, you can cast as many
balls as possible" — the phantom "previous castle cast still active"
lockout was port-only.

### The law — every castle-ball failure arm releases the charge pin

The t=2175 "refused upgrade cast" was never a too-strict gate — it
was a RELEASE the port could not deliver. Decoded timeline (retail
dump + `MGC_CASTLE_PIN_TRACE`, tick-for-tick identical up to the
fork):

- t=2159 cast A arms (+48 = 101, both sides), t=2160 fires: ball
  485, debit 10000, latch 100 — the player is ~25 tiles from home,
  wanting a NEW castle, not realizing one stands; wizext+50 is
  bound, so the muzzle (sub_57610 :65893-908) stamps the HOMING
  upgrade ball (+68/69 = 10/43, +146 = the bound castle).
- t=2161 the ball grounds after ONE flight tick, morphs into the
  (10,43) receipt at slot 713; the receipt's one armed tick rules
  MISS against the bound castle and calls sub_46D20(_, 0) — retail's
  f48 reads 0 from 2162, TWELVE ticks of free hand.
- t=2174 cast B arms, t=2175 fires ball 713. The port's 2174 command
  hit the :55903-06 buzz instead: its pin sat at 100 since 2160.

sub_46D20 (:55949-71) resolves through the GIVEN entity's id24 → the
owner's wizext+708 → the +48 write — so retail passes the token, the
ball, the wizard, anything owner-stamped. The port's Gen-side
stand-in joins on the f144 owner tag… which only the PAIR IMPORTER
ever stamped (`f144: tr(f42)`). A natively-acquired token — the
strict-arm jar grant (:64843-67, the path this very replay took at
the level-start castle jar), `grant_spell`, `try_pickup` — carried
f144 = 0, so `release-find=None` and the pin stuck forever. The pair
lane never saw any of it: pairs import a normalized f144 every pair,
and neither f26 nor f144 is a graded lane.

**Fix: f144 = the owner tag at every native mint/acquire** (grant,
pickup, strict-arm jar grant; the death scatter re-zeroes it — a
stale tag would let a post-death release zero the DROPPED jar's
decay timer), plus `Gen::release_castle_charge_pin(own)` now serving
all three retail release arms: the receipt MISS (:31037), the homing
morph's pool-full retry (:63513-15), and the create ball's
launch-scan failure (:63614-16) — that last one was MISSING outright
(the port despawned silently), the other half of the player's
lockout report. The homing arm's bound-castle refusal keeps NO
release (:63500-04 confirmed — kill only).

⚠ REGRESSION CAUGHT MID-SESSION (test battery + the player live,
within minutes): the stamp initially broke `class12_tick`'s native
human-vs-rival discriminator (`f144 == 0`) — every owned token read
as rival-owned and NO spell fired. The discriminator now rides the
ownership registry (`player.owned[spell] == i`, the strict arm's own
predicate). Moral: before stamping a lane, census its READERS —
grep every consumer of the field, not just the one you're feeding.

### Verification

739 tests green (2 new pins:
`a_missed_upgrade_delivery_releases_the_charge_pin_for_the_recast`,
`a_refused_launch_site_releases_the_charge_pin`; non-vacuity proven
by reverting the stamp — the miss pin fails exactly there), 10/10
fixture suites 0 regressions, mc1l0 pair TSV BYTE-IDENTICAL
(pair-lane blindness confirmed empirically), mc2l3 replay
byte-profile = baseline (4 clean / fields t=5 / pose t=244),
fmt+clippy clean. State-hash goldens D/E re-pinned (deliberate: the
owner tag is persistent state; the OBSERVABLE hashes did not move).
New instruments: `MGC_CASTLE_PIN_TRACE` (cast command / muzzle /
ball morph / receipt verdict / release-find, tick-stamped) and
`mgc_sim::DEBUG_TICK` (harness stamps the recording tick so any
sim-side env-gated probe can label itself).

### Open residuals

- The muzzle re-runs the token gate EVERY latched tick and releases
  on failure (:65869/:65918-21) — still unmodeled (mid-latch caster
  death / negative pool); no corpus row asks yet.
- Port pre_mana at the 2174 command read 22988 vs the recorded
  28848 — both clear the 10000 gate, no graded lane moved; the
  wizard-pool bookkeeping lane is worth a microscope next time a
  mana row surfaces.

### ⭐ NEXT: t=2217 — slot 562 (5,1) target_yaw (retail 107, port 163)

First fork now: the vulture's AIM lane, one tick after the 2175-2216
clean run — and the pair TSV's own t=2216 row
(`562 5 1 target_yaw 107 163`) is the same family, so THIS one the
pair lane can rank. Entity-set first is downstream at 2441, pose
2985 stays player-ruled noise.

## 🏆 THE CORPSE-AIMED EXIT, THE PRE-MOVE PROBE, AND THE RECORDED CHAIN (2026-08-15)

**Three laws; mc1l0 free-run BIT-EXACT horizon 2217 → 2542 (+325:
fields 2217 → 2542, entity-set 2441 → 2812, pose 2985 → 3143 — the
"player-ruled noise" pose row was downstream of the mana fork after
all; post-divergence fields 348741 → 252137), and the pair lanes drop
114 rows across four takes with ZERO new unexplained.**

### Law 1 — the shared chase re-bears BEFORE the target-lost test

t=2217: vulture 562 leaves its chase of the player's DEMOLISHED
castle (slot 107, act_life −1) with target_yaw 107 — the bearing to
the ruin — where the port kept the stale 163. sub_1A120's shared
chase body re-aims +34 on the `(+63 & 3) == 0` cadence at :21656,
BEFORE the `+12 < 0 || (+17 & 4)` lost test at :21658, and the
position read is a raw `+146` dereference with no validity check — a
dead target's coordinates still steer, so the exit tick aims at the
corpse. The port ran the lost test first, so its exit never re-aimed.

⚠ m9 is the OTHER way around: sub_1DA60 (:24190-97) tests first and
re-bears on a DECIMAL `% 10` cadence — a mound's exit tick keeps its
stale aim. The port's shared `mob_chase` now branches the ORDER on
model 9 exactly as it already branched the cadence (bee/m7/griffon
wrappers route through the shared body and inherit the fix; the
militia/guard/genie/wyvern machines are separate and untouched).
Pin: `a_chase_exit_tick_still_aims_at_the_corpse_except_for_the_mound`
(both arms; non-vacuity by revert — fails at the m1 arm).

### Law 2 — the wizard regen probe reads the PRE-move pose

t=2428, the fork the charge-pin session predicted ("wizard pool lane
ungraded... worth a microscope when a mana row surfaces"): pool
+140 27172 → retail 28172, port 27272. The player cast the castle
UPGRADE at 2426 (−10000 debit visible only because the pool finally
left its 36172 cap) and flew away; on tick 2427 the carpet crossed
the level-1 castle's summed overlap extent (dy 1814 > 1783) at the
very tick the upgrade ball was landing. Retail still selected the
castle rate: sub_45C90's bool1 (:55345-52) is read ONCE, before the
move, and :55407-16 feeds BOTH the mana rate (min 1000 vs 100) and
the life-regen u16_341 from that one read. The port passed the
pre-move pair to `player_regen_block` but let `mc1_wizard_pass`
RE-PROBE with the post-move ctx — one floor-rate tick, −900. Fix:
the pass takes the caller's pre-move `(at_castle, at_dolmen)`. At
2428 the castle's own tick (slot 107 < carpet 630) doubles the
extents to 3328 before the carpet ticks, so both engines re-enter
overlap — the fork was exactly one tick wide.
Pin: `the_regen_select_honors_the_premove_probe_not_the_settled_pose`
(both directions; non-vacuity by revert). New instrument:
`MGC_REGEN_TRACE` (castle resolve + overlap + pool/delta, DEBUG_TICK
stamped).

⚠ Decoded but NOT modeled (no corpus row): retail's select is
`bool1 || (flags & 0x1000)` — a one-shot castle-rate bit on the
carpet, CONSUMED when taken (:55416 `&= ~0x10` on byte[1]). Its only
writer is sub_49AD0 (:57794), the class-2 MODEL 6 static's tick,
which stamps every overlapping living wizard. mc1l0 authors no
(2,6) — the port's (2,2)-scanning `at_dolmen` leg may be aimed at
the wrong model; census the class-2 model table before the next
dolmen row surfaces.

### Law 3 — the pair import rebuilds tile chains in RECORDED order

Pair 2371 (mc1l0): ball 90 grounds and merges a neighbor; two
overlap post-move (94 at 280 mana, 500 at 140). Retail's probe
sub_11D10 walks the SEARCH.DAT ring from the ROUNDED (+128) cell
over the TRUNCATION-keyed (`x >> 8`, sub_41CF0) per-cell next20
chains and returns the FIRST full overlap — chain order [90, 76, …,
86, 94, 500] hands it 94. The importer rebuilt chains by ascending
slot-order `link()`, whose head-insertion REVERSED them — the port's
walk met 500 first: a phantom (10,39) slot-desync pair plus a mana
fork (retail 90+94=560, port 90+500=420). The free run never forked
here — its own live history reproduces retail's order; only the
import lane lied. Fix: rebuild each chain by walking the RECORDED
next20/prev22 from each head (prev22 == 0), linking in reverse; the
human slot is spliced out (out-of-pool), torn links fall back to
ascending. MC2's import (its ghost/stale-bit lore) untouched.

Pair-lane sweep, all baselines refreshed: mc1l0 −15 (the 2371
family, plus t=586 (10,12)/(9,1) and t=4285 (9,0)/(10,0) rows that
had been half-credited to terrain/hit-flash rules), mc1l1 −43,
mc1l2 −59, mc1l5 −102/+3 (99 of the removed were UNGRADED; the 3
new: one ruled slot-desync + t=20964 slot 913 (10,0) z+rand, the
fire-churn signature surfacing at a shifted site).

### Verification

mgc-sim battery 513 green (2 new pins above, both revert-proven),
10/10 fixture suites 0 regressions 0 drifted, fmt+clippy clean,
mc1l0 free-run horizon stable across the import change, all four MC1
pair TSVs refreshed with zero new unexplained rows.

### ⭐ NEXT (re-ranked by the player's post-session review)

⚖ PLAYER RULINGS: the run now plays as recorded up to the FINAL
FIGHT, whose whole point is killing the two runaway vultures flying
around the water — the port's vultures are misplaced (possibly
should be ONE by then) and the destruction lands on a dwelling,
raising the militia. So the t=5026-30 settler and t=5051-59 militia
families are SYMPTOMS, off the dig list: those entities shouldn't be
in the fight at all. And the [capture] rules must not be read as
"inconsequential" — the terrain closure demonstrably steers the
endgame (this session's chain fix likewise killed rule-graded rows
at t=586/4285 that were never really terrain/hit-flash).

- **Pair-importer pose gap**: the TSV's pose.z t=567 row (and
  likely 1765-67) sits INSIDE the free-run's bit-exact window — the
  sim steps that same state correctly live, so the IMPORTER (or the
  pose lane's phase pin) loses something the live run keeps. Same
  defect class as this session's chain-order fix. Tool census:
  in-app replay (~3150 eff_pitch) == conform free-run pose first
  (3143), downstream of 2542; the pair lane's t=3879 eff_pitch
  (2047 vs 0) is the lane's only single-tick law candidate.
- **The terrain closure is the campaign wall**: t=2542 slot 500
  (5,15), the guard walking measured-vs-generated mound cells — now
  proven consequential. Re-open the base terrain gap
  (`terrain-diff` clustering) rather than routing around it.
- Remaining rankable pair rows: t=2811 slot 465 (2,0) burning tree
  (flags/life/rand + extra (10,6)); t=3005/3035 slot 714 (9,0)
  sub-tile x/y nudges; t=3879 eff_pitch; slot 785 (9,0) target_yaw
  at 5026 only if it survives the vulture correction.
- The ball-merge candidate scan still walks Chebyshev square rings
  where retail's sub_11D10 walks the SEARCH.DAT ring — same center
  cell, different multi-cell order; no row has forced it yet.
- l5 charge stragglers t=11372-16151 still banked; l5's new t=20964
  churn site noted above.

## 🏆 THE MID-WALK GROUND (2026-08-15, second session)

**The pose channel's ground probe now reads a per-cell reconstruction
of retail's MID-WALK terrain, and the MC1 pose lanes collapse: mc1l0
7097/7097, mc1l1 10541/10541, mc1l2 10588/10588 stepped pairs
BIT-EXACT (100%); mc1l5 23670/23675 (the banked t=16843 charge
tgt_speed + 4 z rows in a twice-written terraform window); mc1hwl0
78 → 20 rows. The whole prior pose dig list — t=567/1765-67 pose.z
AND the t=3879 eff_pitch 2047-vs-0, the lane's only law candidate —
was ONE phase defect in the harness, not the mover: the mover is
bit-exact everywhere the ground image is honest.**

### The defect

Retail's carpet mover probes ground TWICE inside its own walk slot
(:55151 pre-move climb authority, :55103 post-move floor) — after
every lower-slot terraform of the tick, before every higher-slot
one. Neither record endpoint holds that image, and both single-image
rules failed a measured family:

- **measured@N+1** (the old rule, measured on low-slot terraform
  windows) reads the POST-dig ground where diggers sit ABOVE the
  carpet. mc1l0 t=567: fires at slots 692-705 dig the carpet-630
  landing cells; the floor clamp read 925 for retail's 928 and the
  shadow sat 3 low — inside the free run's bit-exact window, because
  the free run's in-walk mover (`tick_flight`) never had the phase
  problem. The t=3879 eff_pitch flip was the same defect through the
  OTHER probe: the pre-move ground fed v5 (climb authority), and a
  one-unit ground error flipped the climb/dive product's sign into
  the 2047-vs-0 wrap.
- **the raw mid-walk snapshot** (first fix attempt) inherits the
  port's own divergent below-carpet terraform: mc1l0 t=1210/1219,
  the graded slot-663 castle-leveler mound window, put the shadow
  +582 off.
- **the one-witness oracle** (snapshot-vs-start only) kept @N
  wherever the port writer was MISSING: mc1l2 t=1112-14 (retail
  wrote below the carpet, no port twin ran).

### The law

`World::arm_midtick_ground_snapshot()`: the pair tick copies its
height plane as the entity walk crosses the carpet anchor (the
sub_45C90 dispatch site, world.rs). `verify.rs::midwalk_ground` then
phases the two MEASURED endpoints per cell with the port tick as a
two-witness ORACLE: a cell keeps measured@N exactly when the port
DEMONSTRABLY wrote it after the carpet (untouched at the snapshot,
changed by tick end); every other cell — written early, or never
written by the port — takes measured@N+1. Every output cell is a
RETAIL value; the port only picks the phase, so a divergent or
missing port writer degrades a cell to an endpoint, never to a port
height. Unit pin: `midwalk_ground_phases_each_cell_by_the_ports_own_
terraform` (one cell per family). The measured@N+1 install stays for
the wall gate.

Remaining shape the rule cannot thread: a cell written BOTH below
and above the carpet in one tick (retail's mid-tick value is neither
endpoint) — mc1l5 t=7426-29, |d| ≤ 51, 4 rows, down from 16.

### Verification

mgc-sim 513 green, mgc-conform 12 green (new pin), 10/10 fixture
suites (7830 fixtures) 0 regressions 0 drifted, fmt+clippy clean.
All four MC1 pair TSVs refreshed: mc1l0 −6 rows (unexplained 26 →
20, zero new — the remaining set is exactly the dig list: t=2811
burning tree, t=3005/3035 bolt nudges, and the player-ruled t=5026+
vulture symptoms), mc1l1 −52, mc1l2 net −0 (3 transient one-witness
rows died with the second witness), mc1l5 −16. mc1l32 untouched
(67.1%, closure-dominated; no terrain channel effect). World gains
two hash-quiet, snapshot-quiet instrument fields (both exhaustive
destructures annotated).

### ⭐ NEXT

- **The terrain closure stays the campaign wall** (t=2542 slot 500
  (5,15) guard on measured-vs-generated mound cells; steers the
  final fight). `terrain-diff` clustering next.
- t=2811 slot 465 (2,0) burning tree (flags/life/rand + extra
  (10,6) slot 67); t=3005/3035 slot 714 (9,0) sub-tile nudges.
- MC2's pose lane still runs the settled-planes rule — port the
  mid-walk reconstruction when an MC2 pose row asks.
- mc1l5's twice-written window (t=7426-29): only a finer terrain
  channel cadence or a sub-tick writer model could thread it; parked.

## 🏆 THE LEVELER'S FIELD HOME + THE WEIGHT TABLE AT 1FF40 (2026-08-15, second session)

**The t=2542 campaign wall was never terrain. Two laws: the pair
importer mis-homed the castle leveler's "current" rung, and the
guard vote's weight table — a code/data alias the decompile could
not express — was extracted from the retail binary. mc1l0 free-run
horizon 2542 → 2812 (+270); the free run and the pair lane now
share one frontier: t=2811, the burning tree. The mc1-guard-terrain
family (1,613 rows / 641 pairs on mc1l0) is ZERO. Take-wide
unexplained: mc1l1 −4,806, mc1l2 −2,106, mc1l5 −14,059,
mc1hwl0 −41,764.**

### Prologue: the "base terrain gap" was neither

`terrain-diff` on mc1l0's record-0 base: all four planes MATCH the
port's generated level — the bake is byte-perfect, and the wall's
"measured-vs-generated mound cells" framing died in one command.
What remained was LAW.

### Law 1 — the leveler's "current" lives at retail +48

The graded castle-663 windows (t=1164-1350, three 10-pair transform
phases each ending in a runaway blowup) were the microscope: retail
sinks the mound −64/tick while the pair-lane port RAISES it +160
runaway. Slot 629, the (10,41) ground leveler, mid-run at pair 1210:
counter +26=10, target +44=52, current +48=73, +28=0. The port keeps
"current" in f28 (the sub_28200 port, features.rs) but the GENERIC
importer copied retail +28=0 there — step = (52−0)/10 = +5 (+160
engine) where retail stepped (52−73)/10 = −2 (−64). The truncating
div with the shrinking counter produced the window-end blowups
(52/2, 52/1). One-line re-home in the MC1 import (the class-12
token's +48→f26 twin): (10,41) f28 ← retail +48. The mc2l24 hydra
taught this class name — "import field-home bug" — the MC1 pool
just paid it. mc1l0-terrain-z: 375 → 266 rows.

### Law 2 — the guard vote weights are [7000, 7000, 10, 7000]

Pair 2541, guard 500 (5,15) at castle 107's pad edge: retail keeps
heading 512 and moves; the port reverses to 1536 and stalls. The
per-entity LCG pinned the shape exactly — same seed, retail draws 4
(vote, aligned, no coin), port draws 5 (vote + failed coin,
3454098813 % 20 = 13). The candidate caps MATCH (measured type
plane: own cell pad 22, ±y cells 26 = blocked, ±x cells 22 = free);
only the SCORES differed. sub_20480 reads its 4-entry weight table
through `*(_DWORD*)sub_1FF40` — bytes at a CODE label, which the
port had stubbed as uniform [16,16,16,16] ("extract from the retail
binary someday"). Extracted today from CARPET.EXE (LE obj1 vbase
0x10000, VA 0x1FF40 → file 0x38738): `58 1b 58 1b 0a 00 58 1b` =
**[7000, 7000, 10, 7000]** — straight/right/left roll a huge range,
the REVERSAL (k=2) caps at 11: a guard almost never turns around on
a free pad. At 2541: retail 291/0/3/0 keeps 512; uniform 3/0/17/0
reversed. The stand-in kept the draw COUNT (why every stream
stayed aligned and the family looked like terrain) but flipped the
CHOICE. `mc1-guard-terrain` roster note rewritten: RESOLVED, any
future hit = grid_walk/weight regression signal.

⚠ Moral, twice in one session: the port's function boundaries are
not retail's, and neither are its DATA boundaries — an 8-byte table
can hide at a function label. When a graded family survives the
closure that justified it, re-run the grading's own experiment.

### Verification

mgc-sim 513 green, mgc-conform 12 green, fmt+clippy clean, 10/10
fixture suites 0 regressions (mc1l0: 2 open exemplars FIXED →
promoted; mc1l5: 1 open exemplar's diff narrowed, signature
refreshed). All four MC1 pair TSVs refreshed; mc1l0 unexplained
holds at exactly the dig list (20 rows). Pose channel still 100%
on l0/l1/l2 after both laws. mc1l32 untouched (its (12,x) f26
freakshow, not guards).

### ⭐ NEXT

- **t=2811 slot 465 (2,0) burning tree** (flags/life/rand + extra
  (10,6) slot 67) — now BOTH the pair lane's first unexplained AND
  the free run's first divergence: one dig, two horizons.
- t=3005/3035 slot 714 (9,0) sub-tile x/y nudges.
- The (12,2)/(12,3) f26 families own mc1l32's 1.8M rows — separate
  campaign.
- Guard weights: the retail bytes cover MC1; HW's CARPET.EXE twin
  (HIDDEN.EXE) unchecked — verify the table at its 1FF40 twin
  before trusting HW guard streams.

## 🏆 THE SIGNED WINDOW — THE BURNING TREE WAS NEVER A TREE LAW (2026-08-15, third session)

**t=2811, the shared frontier of the pair lane and the free run,
falls. The tree code was faithful all along — the divergence was the
MC1 ch0 AREA WINDOW CENTRE, which CARPET.EXE computes on the
coordinate loaded SIGN-EXTENDED (`movsx`): truncation toward zero
makes the famous one-tile-back bias hold only on the WEST/NORTH half
of the map, flipping to a nearest-up centre at pos ≥ 0x8000. One
expression fixed. mc1l0 unexplained 20 → 16 (the whole tree family
dead), free-run horizon 2812 → 2984 (+172), and the free run's pose
channel now first diverges at t=3857 — the castle level-4 upgrade.**

### The dig

Retail's slot-465 tree at (144.55,25.55): inert at life 300, flags
0x2000C, rand untouched through t=2812. A fresh player-fireball
(10,0) fire lands in reused slot 76 at (145.62,26.66) during tick
2810; its (10,6) flame child (slot 96, both sides, id24=630 = the
human) starts the −5/tick grind at t=2813 — the sub_124F0 tree
DISCOUNT (f44=50 → /10). Life hits exactly 0 at t=2872 and the
ignition transition runs at 2873: ONE per-entity draw 177366285 →
1011320332, burn = rand%60+130 = 182, flag 0x8 cleared, (10,6)
spawned at the trunk. The port ran the IDENTICAL transition (same
draw, same 182) at t=2811 — 62 ticks early — because the fire@76's
first-active ch0 broadcast (sub_120B0, amt=400 raw, no tree
discount) reached the tree in the port and not in retail. The
`MGC_MAIL_TRACE`/`MGC_AREA_TRACE` probes printed the poster twice —
that was the pose-alt pass re-running the dirty pair, not a double
write (count under `--no-pose-alt`).

### The law

Every listing-derived gate said HIT: the unsigned back-bias window
(x−128)/256 centres tile 145 and covers the tree's (144,25);
sub_11950 = sub_118C0 = `ent_overlap` passes (dx 271 < 305, dy 284
< 305, z 78 < 278); wildcard filter; flags&8; f28&1. The record
says MISS. The binary settles it — sub_120B0's ch0 arm (VA
0x1215B/0x12172), sub_124F0 (0x1259B/0x125B2) and sub_127E0
(0x1288D/0x128A4) all load the coordinate `movsx` and divide with
the `sar 31/shl 8/sbb/sar 8` TRUNCATING-SIGNED idiom, so the
truncation is toward zero from both sides: below 0x8000 the
one-tile-back bias (every prior pin — t=565-570, t=91, t=568 — is
west-half and stands), at/above 0x8000 effectively `(pos+127)>>8`.
The fire at x=37278 (tile 145.62, as i16 −28258) centres 146 —
window 145..147 misses the tree; the flame at x=37234 centres 145
and hits it. Both measured facts reproduced by one rule. The ch1+
arm (VA 0x12329) is `movsx` + `add 0x80` + `sar 8` — a FLOOR
divide, which commutes with the u8 tile wrap: nearest-rounding is
sign-agnostic and untouched. The listing types the coordinate
unsigned, which erases the movsx — after the function-boundary and
data-boundary lessons, the third way a reconstruction lies: its
TYPES.

Fix: `area_write`'s centre closure, `(p as i16 as i32 - 128) / 256`
(mc1/combat.rs). HIDDEN.EXE byte-scan: all three variant sites
carry the identical idiom — the HW twin is CHECKED for this law
(the 1FF40 weight-table twin remains unchecked).

### Verification

Pair 2811 CONFORMING (rng aligned, pose bit-exact); the true
ignition pairs 2872/2873 conforming. mgc-sim 513 green, mgc-conform
12 green, fmt+clippy clean. Fixtures 10/10, 0 regressions; mc1hwl0
3 drifted open exemplars all IMPROVED (t=91 lost (3,2) z, t=1906
lost (10,39) z, t=3561 lost (5,15) heading) → signatures refreshed
(--promote). TSVs: mc1l0 unexplained 20 → 16 — the remaining set is
exactly t=3005/3035 bolt 714 (9,0) sub-tile nudges + the
player-ruled t=5026+/5051+ endgame symptoms; mc1l1 −10 rows, mc1l2
−13, mc1l5 −661/+38 (net −623; the t=8881 balloon-train/volley
family mostly died, residuals reshaped — ⚖ PLAYER-RULED mid-session:
l5's deltas are rival-wizard lanes, not worth digging). Free run:
BIT-EXACT HORIZON 2812 → 2984.

### ⭐ NEXT

- **Free-run first divergence t=2984**: slots 67/562 z (retail 2093
  port 1993) — the fire-churn z family at the burning village.
  Pair-lane first unexplained: t=3005/3035 bolt 714 (9,0) sub-tile
  x/y nudges.
- **Pose channel first divergence t=3857** — ⚖ PLAYER-INSPECTED:
  the proximate event is a WORM's FIREBALL that was supposed to hit
  but didn't; the suspected ROOT is the castle LEVEL-4 UPGRADE
  upstream (geometry change → rng drift → the miss). Dig the chain
  root-first: castle upgrade stamp, then the rng stream between it
  and the fireball, then the hit lane itself. If the site is
  east-half, also sanity-check the hit-probe/blast-window
  interaction with this session's signed centre.
- Player-ruled t=5026+ vulture/settler/militia symptoms stand (the
  final-fight misplacement, not local laws).
- HW: 1FF40 weight-table twin still unchecked in HIDDEN.EXE.

## 🏆 THE END-TO-END RUN — mc1l0 BIT-EXACT 0..7097, ZERO DIVERGENCE (2026-08-15, fourth session)

**The whole take. 7097 boundaries, every entity field, every rand
draw, every pose sample — the free-running port, seeded once and fed
only the recovered input stream, reproduces the retail recording
bit-for-bit from the first tick to the last. Six laws in one session,
each found by chasing the free run's next wall; the player's "final
1-2 deviations and everything snaps into place" was the correct
forecast, though the chain ran castle-paint geometry → terrain →
aggro, not rng (the rng channel never drifted: 0/7097 all day).**

### The six laws

1. **The projectile impact lands at aim height on the player too**
   (mc1/combat.rs, t=2984): `proj_move_and_hit`'s teleport-onto-victim
   snaps a pool victim to `aim_z()` (+78) but relinked the out-of-pool
   player at raw `ctx.pz`. The carpet's +78/+84 are PLAYER_HH = 100
   (sprite 44 height/2): the worm's fireball must land at pz+100
   (:62852-55), and the rebound relink at pz+f84 (:62885-88). The
   pair lane never sees this arm — the imported human is pool.
   Free-run horizon 2984 → 3857.

2. **The live castle painter is BUFFERED, last row wins** (engine/
   features.rs, t=3857 pose): sub_285C0 memsets one goal-delta word
   per cell of the LEVEL row's rect (:30538-45), walks rows
   1..=level writing `goal − height` at each row's centered offset —
   a cell under several rows keeps the LAST row's delta — then ONE
   apply pass steps height by delta/counter (:30550-70), with the
   counter-1 protection downgrade and the counter-2 bit-3 sweep. The
   port applied each row straight to the map: the L3 courtyard byte
   (goal = datum 73) fought the L3 ring byte (goal = 105) over the
   same apron cell every tick — the t=3856-73 dip the truth channel
   never shows, 32 z-units under the carpet's climb ceiling at the
   exact tick it flattened out. The castle-L4-upgrade root the player
   suspected — geometry, through terrain, not rng. Horizon 3857 →
   4290, and the pose channel's last divergence died here.

3. **Two water probes, not one** (t=4290): retail has sub_11760
   (angle nibble == 0) AND sub_11810 (tile type == 0). The port had
   collapsed both into the type test. Shore/wave cells (type 45,
   angle nibble 0) split them: the fire scorch gate is sub_11760
   (:28098) — a fire over a wave cell draws NO scorch. One call-site
   fix (`on_water` vs `on_water_pub`), both probes now documented
   against their listing anchors; every other port site audited
   against its retail caller (all sub_11810, correct). Horizon
   4290 → 4948.

4. **The village wanted arm rides the occupied-house branch**
   (t=4948): sub_28DC0 arms the attacker's +528 = 200 ONLY inside
   `+26 > 2` — the same branch that pops the militia — and only for
   a carpet-borne attacker (+40's model ≤ 1, the out-of-pool player
   included). The port flagged on every surviving house hit, so
   torching emptied houses kept player_aggro alive: the t=4948
   collapse-evac militia (slot 96, minted mid-walk by the burning
   village, NOT ticked on its birth tick — the m4 ctor's one facing
   draw and f63 = the per-model spawn counter decode the whole
   boundary state) acquired the human on its first idle scan where
   retail's, wanted 0, found nobody. Horizon 4948 → 5027.

5. **A villager's hit tick freezes the walker** (t=5026): the
   m12/13/14 damage prologue puts the movement core and the wander
   draws in the ELSE of the damage test (m12 :25057-67) — the tick
   the 400 lands, the settler marks the attacker and does nothing
   else. The port fell through and walked. The player-ruled
   "endgame symptom" rows at t=5026/5030 were exactly this.

6. **The militiaman turns, never snaps** (t=5051): m4's chase is the
   shared sub_1A120 — the re-bear writes f34 ONLY; the movement core
   turns f30 ±v_2 a tick even at stand-ground speed 0 (retail steps
   toward the STALE bearing on the very tick the re-bear posts the
   new one). The port's custom body snapped f30 = f34. Plus
   sub_1BB20's own tail: the wanted re-arm fires on the v_26 cadence
   in chase, outside the range gate (:22705-14). Horizon 5051 →
   END OF TAKE.

### The pair lane and the measured ANGLE plane

With the fire gate reading angle live, the pair lane's terrain
reconstruction (measured height/type over pristine angle) broke at
paint windows — the mid-paint walkability/protection dance is not
derivable from the pool closure. The recording's format-2 terrain
channel measures the angle plane; `install_measured_terrain` now
installs it when present (all consumers: verify, replay anchors,
pose lane, fixtures, the app's --replay). This also fixed two OPEN
MC2 exemplars (mc2l3 t=1095/1309, mc2l30 t=13) and shrank an mc2l3
missing-entity drift to a field diff — the reconstruction gap was
never MC1-only.

### What remains in mc1l0

**3 unexplained pair rows** (t=3005/3035, bolt 714 sub-tile x/y) —
and the free run is bit-exact through them, so the laws are proven
exact and the rows are the pair harness's own limit: the worm (slot
507, BELOW the carpet slot) chases the human off the PRE-move pose
in retail, and pair mode pins one recorded sample (n1) throughout.
The deflect/impact snap inherits the worm's ~10-unit step delta one
hop later. Structural to the one-sample pin, not a sim defect;
player agrees they don't matter.

### Verification

mc1l0 free run: BIT-EXACT 0..7097, zero divergence, all channels
(pose included — 7097/7097). Pair lane: unexplained 16 → 3, pose
channel 100%. mc1l1/l2/l5 TSVs refreshed (l5 pose 23674/23679, one
residual). Fixture suites 10/10, 0 regressions, 5 exemplars FIXED →
promoted (mc1l0 t=1024 capture + t=5068 endgame; mc2l3 ×2; mc2l30
×1), 1 mc2l3 drift refreshed. mgc-sim green (level-5 state-hash
golden re-pinned A..E, annotated — post-init unchanged, OBSERVABLE
holds byte-for-byte at post-init..C). mgc-conform green, fmt+clippy
clean. New microscope: MGC_CELL_TRACE (replay mode) — port
height/type/angle beside the truth channel per watched cell, the
tool that found both terrain plants.

### ⭐ NEXT

- mc1l0 is CLOSED end-to-end (3 structural pair rows stand,
  player-acknowledged). The campaign moves to the next lane:
  mc1hwl0 free run (horizon t=2 — the HW column: 1FF40 weight twin
  still unchecked, HW-specific laws undug), or the l1/l2 rival-
  wizard campaigns, or MC2's free run (mc2l0 horizon 65).
- The known-deviations roster could absorb the 3 pose-pin rows as a
  classed artifact if the player wants the count at 0 — ask first.

## THE mc1l1 INTAKE (2026-08-15e): four laws, 4374 → 387

The player-chosen lane after mc1l0's closure. The take: 10709
fixture-grade pairs, no rivals (the campaign memory's "first rival
lane" claim was wrong — corrected), a possession-driven dwelling
grind, two authored trigger events, and an endgame castle era.
Intake classification collapsed 4374 unexplained rows into five
families; four laws closed 91% of them in one session.

1. **The f132 SIGN/WIDTH decode** (the whole mana lane, 2238 rows):
   carpet +132 is retail's SIGNED 32-BIT pending mana delta — the
   one-word mailbox the token slots overwrite (cast debits negative,
   castle casts past 16 bits; mc1l1 records a −40000). The decoder
   read it as u16, so every pair seeded with a recorded debit
   applied +65486 instead of −50 and clamped the player onto the
   census ceiling — the "wrong mana per dwelling" the player
   reported, and the idle 950-vs-1000 breathing before it. There is
   NO collection metering in retail (decompile agent, both
   binaries): every collector lumps the whole cargo; +136 is the
   per-tick census total (1000 + Σ claimed cargo — houses re-derive
   theirs as population×256 every 40 ticks), and +140 chases it at
   the +132 rate (max(+136/2000, 100); castle/dolmen
   max(+136/200, 1000)). The measured "+150/5-tick metering" was
   the chase riding the recast cadence: −50, 0, 0, +100, +100.
   Residue: 9 pairs (t=8745+, the castle-pump/mid-burst regen
   suppression for the castle era) — open.

2. **The TRIGGER PRE-MOVE POSE law** (the t=3082 worm ambush, 85
   missing rows): the class-11 fire probe (sub_5A090 :67632, every
   8th f63 tick, human carpets only) reads the carpet's
   PREVIOUS-frame pose — retail's pooled carpet (slot 280) sits
   above every authored volume in the slot-ordered walk. The port
   probed the post-move pose and fired the (11,0) token at
   (198.5,224.5) one whole probe window early, consuming the
   disposition before retail's fire tick: 85 extra rows at t=3074,
   85 missing at t=3082 (dis 3 = five authored (5,3) records × 17 —
   the m3 worm ctor sub_384B0 mints 1 head + 16 segments, no pool
   guard). Same law family as the awake pre-pass. Level-5 golden
   re-pinned B..E for it (B "crater trigger" is exactly a scripted
   trigger trip; OBSERVABLE holds at B — pure timing).

3. **The (10,14) MANA-SCATTER PUFF column** (~1650 rows, the take's
   biggest family): the port's "NO MC1 creator" note was half-true —
   no code-side caller, but level 001 AUTHORS (10,14) THING records
   behind trigger dispositions (dis 1 = 8 puffs + 15 mana balls +
   the chained (11,1)). Ported: the ctor (sub_3AB40 :46860 — life
   rand%33+28, filter pair (10,14), sprite 9, rise rand%53+51) via
   spawn_creator→spawn_effect, and the tick (sub_258A0 :28489 — the
   state-13 riser with an UNCONDITIONAL last-6-ticks sprite
   walk-down) + the world.rs effect_tick admission. Player-corrected
   mid-session: t=343 is "puffs of smoke where mana appears" — both
   readings were right, the trigger mints both.

4. **The JAR z-SERVO + THE HIDE BIT** (the (12,2) z family + the
   player's spell-jar report): (a) retail's placed-jar tick falls
   −128/tick toward ground and clamps up instantly (sub_55A40
   :64765-70) — frozen-z only matched corpora while no terrain
   moved under a jar; mc1l1's scorch/worm episodes move it and
   retail tracks. Ported into the strict arm. (b) The start-point
   "fireball jar": retail's wizard init claims one class-12 entity
   per carried spell as the live manifestation AT the wizard and
   hides it with byte[0] |= 1 (:54907) — the l1 start pile
   (Fireball/Possess/Create Castle, flags 5) IS the player's
   spellbook; the authored pity jar nearby is claimed+hidden the
   same way. The strict projections now key the class-12 skip on
   the BIT (replacing the phase-0 heuristic, which drew the claimed
   authored jar); prune_owned_jars stays the free-run stand-in
   (DEVIATIONS.md entry corrected: retail HIDES, the port REMOVES —
   the slot economy is the residual deviation).

### What remains in mc1l1 (387 rows)

- (12,2) f26 ×165: a 251→0 countdown on the placed Accelerate jar
  from t=8747, port one tick behind — the +26 writer is unidentified
  (blue-jar recharge? post-grant timer?). NEXT.
- (5,0) m0-worm pose dribble ×137, (9,0) fireball sub-tile ×~20.
- player.mana ×9 pairs (castle-era mid-burst suppression).
- FREE RUN: bit-exact 0..344, wall = DISPOSITION SLOT ALLOCATION —
  retail rebuilds its free stacks lowest-slot-first on EVERY
  disposition fire (sub_37220 :43825, + a one-fire eviction window,
  + var_4593=-1 after) so payload slots allocate ascending; the
  port's allocator orders differently and slot 41 gets a puff where
  retail parks the chained (11,1). THE law for the l1 free run.
- Secondary retail laws banked from the trigger agent: the REARM
  probe (sub_5A120 :67654) walks the whole wizard roster (rivals
  included — matters from l2 on); class 7/8/9 THING creators are
  allocate-and-abandon (spawn_inert materializes them — a real
  deviation, no l1 records); spell_cast_cost reads the live
  manifestation +136 only for spell 16 where retail reads it for
  every spell (:64948).

### Verification

mc1l0 free run BIT-EXACT 0..7097 (zero divergence) after all four
laws; l0 pair lane 3 unexplained (the standing pose-pin rows).
mc1l1 pair lane 4374 → 387 unexplained; first divergent pair t=47 →
t=343. Fixture suites 10/10, 0 regressions (5 drifts = rows VANISHING
under the f132 fix; promoted). known-deviations.json predates the
`recording` field and errors standalone — untouched. Level-5 golden
re-pinned B..E (state) / C..E (observable) for the trigger law,
annotated. Full workspace tests green, fmt clean.

## THE mc1l1 FREE-RUN SPRINT (2026-08-15f): five laws, wall 344 → 8746

The player-corrected frontier ("first divergence = the castle at
t=630 that should have killed a worm") decoded as five stacked laws;
mc1l0 stays BIT-EXACT 0..7097 (zero divergence) under all of them.
The two lured vulture packs and the castle-kill endgame supplied
every corpus ruling. CARPET.EXE itself (extracted from the GOG ISO,
LE-parsed, disassembled at need) settled two of them — the lift lied
three separate ways in this region.

1. **DISPOSITION-FIRE STACK REBUILD** (the t=344 wall): EVERY
   disposition fire re-ranks both allocator stacks by the descending
   999→1 pool scan (sub_37220 at sub_37440's top :43960; MC2 twin
   sub_49F90 at sub_4A1E0's top EF:32966), so fire payloads allocate
   ASCENDING from the lowest free slot; the victim stack disarms
   after the fire (var_4593=-1 / dword_0x11e6=-1 — the one-fire
   eviction window). `World::fire_disposition` now does all three
   (MC1 victim mask 0x20400, MC2 0x2_0000). Effect: 344 → 1925, and
   the RNG lane went 8726 divergent boundaries → ZERO for the whole
   run (the puffs had been landing in wrong slots, scrambling every
   per-entity LCG). MC2 goldens re-pinned for it (cave ×2, slice
   checkpoints 4-6, flight-tier, l5 state+obs — annotated in the
   tests; all 5 MC2 fixture suites 0 regressions).

2. **THE MOVSX-SIGNED SEPARATION BOX** (t=1925/1933 + three fired
   twins): the pack separation (:21796) and grid-walk repulsion
   (:25984) box tests sign-extend EACH coordinate before a 32-BIT
   subtract (binary at obj1 0x1d5eb: movsx+sub, never a wrapped i16
   difference) — a pair straddling the signed midline 0x8000 reads
   ~65k apart and never separates. Five straddle skips vs 436
   same-side fires pinned it. The id24 self-skip, full-chain walk,
   first-hit break and angle(member→own) are all byte-verified; the
   catch-up tail reads speed+accel BOTH from the leader (confirms
   the earlier mis-fix ruling). Killed en route, by corpus: the
   leaderless-only skip, the entry-pack skip, a Euclidean-256 disc,
   and a stop-at-leader walk bound — each fit a subset, only the
   movsx box fits all eight rulings. 1925 → 2572.

3. **THE wizext+84 GUARD REGISTER** (t=2571): the castle-guard lane
   is a per-OWNER positional register on the wizard extension, not a
   live census (sub_47400 :56412-47). Stale entry (freed slot or
   state-95 corpse) → clear + RE-ARM f46=16 with NO spawn; empty
   entry + f46==0 → spawn ONE at the castle pos, relink to the
   courtyard (+128,+640), facing 512. The register SURVIVES castle
   death — guard 313 died ~t=2000, its stale entry re-armed at the
   next state-4 dispatch (2571), the fresh guard landed 16 passes
   late (t≈2605, slot 348) where the census spawned instantly. Also
   corpus-proven here: the fleet dispatch runs from the STATE-4 arm
   (the castle's long-lived working state), not state 5 — 30 settled
   level-3 ticks with gq=4 and f46 untouched. New Gen field
   `mc1_guard_reg` (hash-transparent all-zero, SNAPSHOT_VERSION 10);
   conformance import rebuilds the live half from owner-stamped
   guards (stale entries are unknowable from a snapshot — one
   boundary of pair residue per stale event). MC2 keeps the census
   stand-in (twin unverified; corpora identical under both).
   2572 → 3810.

4. **THE m0 DEATH-TICK BOB** (t=3810): the m0 wrappers
   (sub_1B070/1B090/1B0E0) run the z-bob sub_1B120 as an
   UNCONDITIONAL tail — the tick the damage prologue demotes the
   worm to the death state still rises (recorded +130 while state
   2→4). The port's Inbox::Dead early-return now bobs m0 first.
   3810 → 4130.

5. **TICK-TOP CHAIN MEMBERSHIP for the acquire** (t=4130) + **THE
   UNIVERSAL HIT FREEZE** (t=5450): (a) the projectile acquire's
   creature sweep walks the per-model chains whose MEMBERSHIP is the
   tick-top rebuild (heads at wizext-file 36382+4·model —
   binary-verified; the lift wrote them to 36462, a transcription
   bug) — a segment the castle crush promoted mid-tick stays
   invisible to the muzzle acquire (fireball 427 flies straight,
   f146=0). New derived `Gen::mob_chains` snapshot (hash-silent,
   never saved, severed-chain cut) consumed by aim_assist; the
   pack/grid/recruit scans keep their live-scan stand-ins until a
   row demands otherwise. (b) The shared prologues freeze EVERY
   creature's hit tick (idle/chase/pack all return out of the v4
   arm) — not just the villager families: vulture 200 takes a
   400 fire hit mid-pack and holds position, heading and aim. The
   wizard-attacker retaliate arms were already freezes; l0 never
   discriminated because its creatures only took wizard fire.
   4130 → 8746 in one pair of laws.

### State at close

- mc1l1 FREE RUN: bit-exact 0..8746 (from 0..344 at intake close);
  RNG lane clean across all 10709 ticks; post-wall traffic is a few
  thousand rows (was ~900k).
- mc1l1 pair lane: 387 → 191 unexplained (189 field + 2 extra);
  first divergent pair 343 → 507 (slot 15 target_yaw ±2, likely
  pose-adjacent).
- mc1l0 free run BIT-EXACT 0..7097 under all five laws. Fixture
  suites 10/10, 0 regressions (4 Open drifts all SHED rows — l32's
  t=3284 loses its spurious (9,0) acquisition — promoted).
- ⭐ NEXT (the t=8746 wall): the player casts Accelerate; retail
  sets flags|=0x80 on the HIDDEN spellbook jar (slot 26, class 12,
  flags 5→133) — the spell-ACTIVE bit — and the placed jar's f26
  251→0 countdown follows from t=8747 (the known ×165 pair family,
  port one tick behind, +26 writer unidentified). Find the 0x80
  writer in the cast path (sub_55E80 family) and the countdown law.
- Tooling: CARPET.EXE lives in the GOG ISO (`game.gog` LBA 30, LE at
  0x5998, obj1 vbase 0x10000, datapages 0x24600; globals appear
  flat-0x90000; a second LE at 0xf5998 is a −0x200-shifted sibling).
  `MGC_PACK_TRACE=1` also makes verify-deltas announce every pair on
  stderr.

## 🏆🏆 mc1l1 CLOSED (2026-08-16): BIT-EXACT 0..10709, two laws

The t=8746 wall and everything behind it fell to two laws; the free
run is now bit-exact for the ENTIRE 10709-tick recording — zero
divergence on every channel (pose, fields, entity sets, RNG). mc1l0
stays bit-exact 0..7097, the mc1l32 head (0..60, its teleporter
included) grades bit-exact, all 10 fixture suites 0 regressions /
0 drifted (no goldens moved), workspace tests green.

1. **THE SPEED-TOKEN JAR-SIDE LAW** (sub_56380 :65131 / backwards
   twin sub_57F00 :66172, the t=8746 wall): the Accelerate tokens run
   a full jar-side machine the port had left inert under strict:
   - flags |= 0x80 (the spell-ACTIVE bit) on the full tick
     (+48 == +50, :65154-57), cleared at total−2 (:65160-65) and at
     burst end. Recorded 5→133→5 exactly (t=8746..8750).
   - The +48 countdown pins at full on every held re-arm (the cast
     dispatcher's :55893 reload) and drains 1/tick after release.
   - **sub_55E80's full-arm debit is TOKEN-side and LIVE in retail**
     (the remc1 `//fix` comment-out is the maintainer's): the arm
     tick, each held re-arm, and the first release tick each stamp
     the full one-shot cost — mc1l1's held ladder measures three
     −1000 landings (t=8746-48). The command-site debit moved to the
     token's full arm; the silent re-arm gate reads the PRE-step pool
     (:55890, the mc1l32 t=671 law — `pre_mana` threaded into
     cast_spell).
   - Mid-burst suppression runs from the token every sustain tick —
     the pool is FROZEN for the whole 251-tick glide (recorded 87818
     for 165 ticks).
   - **THE v_14 TWO-PHASE KILL** (:55766-80 + :65146-50): a speed
     press that MOVES v_12 arms the carpet dispatch's v_14 latch —
     during a boost only the RESISTING press can (the boosted ±160/
     ±240 target sits outside the ±80 bounds test; the press also
     clamps it back into the band while act-speed chases one 16-step)
     — and the token reads the latch on its NEXT pass: counter = 1,
     one final decrement, burst over. Port: `Mc1Moved::speed_touched`
     → `World::mc1_v14`; the old instant thrust_cancel kill retired
     to `accel_brake_immediate` (enhanced-mover alternates only).
   - **THE BURST-END BASE SNAP is SIGNED**: target AND actual speed
     restore to +80 from the forward burst (:65194-95) but **−80
     from the backwards twin** (:66226-27) — the port's "+80 max
     forward even out of backwards flight" note was over-generalized
     from the forward twin. Mailed to the carpet
     (`pending_speed_base`), consumed at its walk slot BEFORE the
     command integration (retail's token-below-carpet order); the
     override write moved ABOVE the input integration in `mc1_move`
     for the same reason. The mover re-reads the override at its own
     walk moment (a same-tick kill already dropped it).
   - The contrail puff is PRE-decrement inside the sustain arm (the
     counter==1 expiry tick still puffs; a v_14 kill tick does not),
     and the backwards twin puffs too (:66211-18).
   - Corpus effect: free-run wall 8746 → 9899 in one stroke (flags +
     mana + kill-seam pose cascade all one family); the pair lane's
     ×165 "f26 countdown" family collapsed to the ONE structurally
     unknowable v_14-kill pair (t=8911 — v_14 is not recoverable from
     a snapshot import). SNAPSHOT_VERSION 10 → 11
     (`pending_speed_base`, `mc1_v14`; both hash-quiet one-tick
     mail).

2. **THE PORTAL FACING CONE WAS COMPUTED ON MANGLED DELTAS** (the
   t=9899 wall, sub_26A60 :29208-09): the (10,34) vortex warps a
   player who overlaps (sub_11950 summed extents) AND faces it —
   bearing = sub_42150(wizard → portal) on the full 16-bit wrapping
   axes, diff vs heading < 0xAA. The port computed the bearing with
   `Gen::wrap_delta` — a TILE-BYTE (±128-wrap) helper — on
   engine-unit deltas, and in the reversed direction: t=9899's true
   bearing 1273 (heading 1163, diff 110 → warp) read as a mangled
   932 → never fired. Fixed in portal_tick AND the mc2_portal_tick
   twin (same defect; EF cone cited identical — MC2 corpus
   unverified). Also: the warp mail from a BELOW-carpet portal now
   lands BEFORE the carpet's move (`pending_teleport` consumed at
   the walk slot too — retail sub_41C70 writes the wizard axis at
   the portal's own tick, so the recorded boundary is dest + one
   move step; spell teleports still consume post-tick, their stamp
   site is the carpet's own pass). This closed the level: 9899 →
   10709 BIT-EXACT.

### Pair lane at close

mc1l1 verify-deltas: 191 → **5 unexplained field rows + 2 extras**
(from 4374 at intake). The fireball-debit six-pack (t=8889-8910,
−200 pairs during the glide) fell to an importer fix: the
`mana_delta` seed clamp no longer wipes a pending ABOVE-carpet debit
riding f132 while a SPEED token is mid-burst (2/21 joined the
launcher raw-seed set — they run the live machine under strict now).
Remaining: (9,0) t=639 x / t=3231 y+heading, (10,39) t=3278 z ±1,
the t=8911 v_14 pair, and the two (5,15) guard-register stale-entry
extras (t=2571/3453 — the documented law-3 import limitation). POSE
CHANNEL: 10541/10541 stepped pairs bit-exact (100%), 167
accel-domain + 1 warp gated.

### Notes

- The `held_accelerate_drains_mana_every_tick` /
  `accelerate_directions_are_mutually_exclusive` /
  `accelerate_hold.rs` tests re-pinned to the two-phase law (the
  brake tick still boosts; the dry hold survives the emptying tick
  at 3.0 because the re-arm gate reads pre-step).
- Un-modeled residual (corpus-unseen): a REFUSED full tick (dry pool
  mid-hold) leaves retail's v_12 at its stale boosted value for one
  tick; the port's recompute model reads 2.0 there.
- `Gen::rebuild_mob_chains` scoped `#[cfg(test)]` (its only caller;
  the live tick builds chains in its top sweep) — the dead-code
  warning is gone.
- mc1l32 beyond t=60 deliberately unexplored (player scope ruling).
