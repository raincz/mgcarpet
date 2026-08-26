# mgcarpet ROADMAP

A faithful Rust remake of Magic Carpet 1, Hidden Worlds, and Magic Carpet 2
on one superset engine, running against pristine GOG game data. Both
campaigns are playable end-to-end (MC2 through its finale; MC1/HW hub +
levels); the full MC2 roster, spell set, rivals, stage engine, castle, and
campaign menus are ported; MC1 retail bit-identity is pinned by state-hash
goldens throughout.

This file is the brief successor to the 9,160-line development ledger, which
is preserved at `archive/ROADMAP-2026-07-19-full.md`. It states what stands
and lists what remains; history lives in the archive and git.

## Document map

- `ROADMAP.md` (this) — status + remaining work.
- `RETROSPECTIVE-2026-07-19.md` — the project wrap: architecture verdict,
  seam-deviation history, error classes, refactor shortlist.
- `DEVIATIONS.md` — the canonical register of deliberate departures from
  retail (code sites marked `(deliberate)`). Check BEFORE "fixing" toward
  retail.
- `FIDELITY.md` — fail-open fidelity gaps and approximations still owed.
- `FORMAT.md` — the baked bundle/level format spec (lockstep with code).
- `archive/DESIGN-SAVES.md` — save/load and the in-game menu: retail
  findings and the settled rulings. IMPLEMENTED; kept for the rulings and
  the reasoning behind them, which the code comments cite.
- `traces/`, `spell-audit/` — the decompile research bank; cited by code
  comments; consult before re-porting anything.
- `archive/` — completed working documents (surveys, reviews, audits,
  playtest banks, the full ledger). Open items were extracted into this
  file; the archive is history only.

## Done (the arc, in one screen)

- MC1 core port: sim, terrain, spells, mobs, rivals, HUD/map, GM music —
  player-certified across many playtest rounds.
- Multi-game architecture (2026-07-09): ONE superset sim, five-tier
  divergence taxonomy, VerbSet wiring, ChassisParams, state-hash goldens.
  Kill criterion passed decisively; see the retrospective.
- MC2 full port (07-09 → 07-18): roster + multipart + class-9 flyers, spell
  column with XP, castle column, cave column, stage/objective engine (all
  shipped types), rivals, doomsday endgame, campaign stitching, menus + map
  overlay, audio column (music, narration, SFX policy). 165 levels load
  with a 100.0% THING census; campaign playtested through the finale.
- Hidden Worlds: core delta landed (chassis shared wholesale, one verb arm,
  spells-table data delta).
- Presentation: enhanced renderer (fog/sky/reflections/dynamic lights),
  smooth motion, shore/fire/lightning effect tracks, options registry.
- 2026-07-19: comment sweep (histology out, `DEVIATIONS.md` born), docs
  restructure, retrospective.

## Remaining work

### MC1 crab eggs — CLOSED 2026-08-26 (the mine bug's MC1 twin)

Player-reported: crabs multiply, but nothing is ever visible to multiply
*from*. The suspicion was a mislabelled entity wearing the wrong sprite.
It was neither — **`(10,52)` was missing from the MC1 class-10 draw
allowlist** (`world.rs::drawable`), the same defect as the Magic Mine's
`(10,78)` below, in its second costume.

Everything else was already right, and measured so:
- Retail's ctor `sub_3B860` (:47613; HW `sub_3BBE0` :43740, identical)
  stamps class 10 / model 52 / sprite-stats **row 205**, and the port's
  arm is verbatim. `dump-state --port` on mc1l32 t=21700 slot 62 shows
  every lane equal, `type86 = 205` included.
- The egg incubates and hatches bit-exactly. mc1l32 is certified across
  its one egg (t=21644..22246, ~600 ticks — retail leaves the ctor's
  `f26 = 600` standing rather than the layer's 100..190 timer).
- The import label is `"Crab egg"` and row 205 is the **only**
  `SPRITE_STATS` row pointing at TMAPS sprite 228 (the grey cracked
  shell, 49x49, single frame) — so that art was unreachable art, which
  is why nobody noticed it sitting in the bake.

**Why certification could not see it:** the graded observable projection
carries no `type86`/`frame88` lane. A take can be bit-exact for 52,140
ticks with the entity drawing nothing. Same lesson as the mine: entity
counts and "the player can see it" are different assertions.

**Swept the family, not just the egg.** Cross-referencing every MC1
creator-table ctor's `sub_36FA0_37360` sprite argument against the
allowlist, and against a `(class, model)` census of all 14 MC1
recordings: `(10,52)` was the *only* sprite-carrying MC1 class-10 model
that occurs in the corpus and was not drawn. `(10,12)` (possess flash,
sprite row 41) also carries a sprite and stays excluded deliberately —
retail's ctor clears its draw bit. Models 9/11/15/17/18/41/42/53/55 do
occur and correctly carry no ctor sprite.

Retail also plots the egg on the minimap (the class-0xA arm falls
through to `LABEL_32` for any model that is not 18/34/39), which the
restored pose brings back for free, in wild magenta.

MC1-gated: MC2's `(10,52)` is the invisible castle anchor (`mc2::tail`).
Pinned by `crab_egg_is_drawable_with_its_shell_sprite`, A/B-proven to
fail with the arm reverted.

⚠ Corpus note: `mc1l32-quick` carries **6,088** egg-ticks against
`mc1l32`'s 603 — by far the better egg witness. It was swept in the
same session and paid immediately: its FIRST divergence (t=6719) was a
real MC1 law, the jar pickup poll's walk phase, worth +3,209 ticks. See
CONFORMANCE-FINDINGS.md § "SESSION 2026-08-26b"; the take is now a
guard line in `conformance/brief-baseline.txt` and its next head is
t=9928 `(9,0) slot 2`.

### MC1 castle datum — CLOSED as FAITHFUL 2026-07-22 (opt-in candidate banked)

The reported "peak castle sinks as it upgrades" is retail MC1 behavior,
confirmed both by decompile trace (the leveler re-averages the grown
footprint's outside corners on every transform; `+154` has exactly one
writer, the leveler finish :30424) and by the player's own retail
replay (peak build in the ocean — every upgrade stepped lower). Port is
line-faithful; full law in `traces/mc1-castle-datum.md`; regression pin
`crates/mgc-sim/tests/mc1_castle_datum.rs`. MC2 is clean in both
retail and port (datum computed once at the ctor, frozen).

BANKED (player: "keep it in mind, maybe get back to it"): an OPT-IN
MC2-style frozen-datum alternate for MC1 (lock `site_z` after the L1
leveler pass; sim-divergent, so faithful stays the default and goldens
stay untouched; the regression test must be consciously updated with
it).

### Two player-reported bugs — BOTH FIXED 2026-07-21, playtest owed

**1. MC1 Accelerate: the sustained cast was free.** Fixed at
`world.rs:3043-3072` — the affordability gate and the debit now run on
EVERY re-arm, not only the first.

Retail debits from the manifestation tick, `sub_55E80` (:64936): on
every tick the burst sits at full (`+48 == +50`, which the hold's
re-arm re-pins each tick) it overwrites the caster's regen accumulator
with `-(+136)` — the FULL one-shot cost, 1000 for both 2 and 21. There
is no separate sustain column (`+140` is cost/count and is never
spent). **remc1 ships that debit commented out behind a `//fix`
marker**, which is where the free hold came from; remc2's independent
tree runs the identical block live (`sub_68DE0`,
EventsFunctions.cpp:55569), so the gap is the remc1 maintainer's, not
retail's.

Exhaustion drops the flyer back to 2x for the rest of the burst rather
than cancelling — that falls out of `accel_held` being recomputed each
tick. Known small gap: retail sounds one buzz on the tick a hold runs
dry (from the manifestation gate `sub_55DD0` :64930); the cast path's
sustained refusal is silent (:55873/:55890) and ours has no buzz at
all. Pinned by `held_accelerate_drains_mana_every_tick` (verified
non-vacuous).

**Playtest question, not a code question:** the cost is the whole
intrinsic base pool (both are 1000), so before any mana is claimed the
hold buys exactly ONE tick of 3x and then glides at 2x. Mid-game pools
fund a real hold. If that reads as too expensive, suspect the tick
RATE, not the amount — retail ticks once per rendered frame, we run
24 Hz.

**2. MC2 save recorded the level the carpet was parked on.** Fixed in
`main.rs` (committed): the stored `levels_completed` was right; the
slot row renders the `.mgcs` header's level column, which is
`run.current`, and `campaign_complete` never advanced it — so the run
still called itself "in level 6" while sitting on the map, until
clicking the next portal ran `campaign_switch`. The park-one-back rule
was innocent (`open_map_screen` doesn't use it). The pending-level rule
`start` applies on load is now the named `mc2_pending_level`, and map
entry re-applies it, so a slot names the same level before and after a
reload. Also fixes the map's asset-failure fallbacks, which were
relaunching the level just finished.

Also banked, lower priority: **make MSAA the default** once it has more
playtime. Player-verified this session as fast and good-looking on
integrated graphics; the reason to wait is that it is startup-only and
nobody has yet run it across a full campaign.

### Intro/outro FMVs — LANDED 2026-07-21, playtest round 1 done

The full-screen movies play, with their soundtrack and subtitles: the
launch intro chain, MC1's per-level congratulation movie, MC2's six
cutscenes and both endings.

**Playtest round 1 raised six MC1 findings, all fixed.** Four are
player-ruled deviations (docs/DEVIATIONS.md): playback ran too fast and
clipped a narration line, so every delay is stretched 25%; scene holds
landed a frame or two into the next page-flip, so the script fires one
frame early; a movie's music played on under the NEXT movie, so the
score now ends with the movie that started it (reported twice — the
skip path first, then the natural transition it had masked); and the
menu track no longer starts at boot when an intro is about to play,
which was audible as a blip of menu MIDI under the opening. Two were
plain transcription bugs: the subtitle pen advanced by the glyph's full
width where retail advances by `width - 1` (running the longest line to
x=363 on a 320px screen), and the pen-box clip edge was briefly passed
where the canvas row stride belonged. MC2's intro was reported good.
Round 2 owed — the fixes are unconfirmed and the endings are unplayed.

**MC2 subtitle layout fixed 2026-07-24 (playtest owed).** A player
report (long captions lost their tails — the port stacked two lines
where retail shows three) traced to the wrong font: MC2 movies never
use SFONT1. Retail loads a dedicated 7×8 monospace font from
HSCREEN0.DAT before every FMV and lays captions out monospace —
42-cell greedy wrap, 8-px line stacking (up to four lines), all-caps,
flat nearest-white ink. The wrap walk is ported index-for-index and
pinned against a retail screen capture. Details in docs/FIDELITY.md
("The subtitle fonts and pens are per-game"); BAKE_EPOCH 20 rebakes
the `mc2-movies` font.

**What landed.**
- `mgc-import fmv.rs`: `FmvCursor`, an incremental one-frame-at-a-time
  decoder over the raw stream. The eager `decode` stays for the 3-30
  frame menu loops; the cursor is what the full-screen movies need
  (MC1's intro is 3165 frames = ~200 MB of canvases eagerly).
- `bake_movies` → new `assets/mc1-movies` (107 MB) and
  `assets/mc2-movies` (139 MB) bundles: the streams copied RAW plus a
  `MovieIndex`. The only bundle that does not translate its input, for
  the reason above. Local disk only — bundles are baked from the
  player's own install. **BAKE_EPOCH 19.**
- `assets/mc2-audio` gains `mc2-intro` + `mc2-cuts` (MUSIC.DAT GM
  sub-songs 4 and 5): the movies' score. The container carries no
  audio, so MIDI is all they have.
- `mgc-app movie.rs`: the player — cue chains, per-movie skip, the
  transcribed event scripts (pacing, music, samples, subtitles),
  boundary fades, post-movie holds.
- `mgc-audio`: a movie-sample lane (`play_movie_sample` /
  `set_movie_bank` / stop), bypassing the 3-D gameplay mixer.
- Seams in `main.rs`: `Screen::Movie`, `intro_movies` (boot),
  `mc2_cutscene`, `mc1_win_movie`, and the `NextStep::Outro` arm that
  used to just print "campaign complete!".
- Option `render.preference.movies` (default ON = faithful),
  `--movies`/`--no-movies`.

**Four corrections to the recon, all found in the decompile.**

0. **The movies are NOT silent.** The container has no audio STREAM,
   but that is not the same as having no soundtrack — the same event
   script assembles one at playback time out of the ordinary sample
   banks: narration clips, effects, ambient loops, over the MIDI
   score, with subtitles against the narration. MC1's intro alone
   cues 51 samples, twelve voice clips and seventeen subtitle lines.
   The sample banks corroborate the whole transcription: the intro
   loads exactly the banks holding `voc1`..`voc12`, and MC2 banks 5-9
   are `viscut1`..`viscut5`, one per cutscene. All of it is ported —
   samples, music and subtitles — with the text coming from the games'
   own string tables (MC1 `ETEXT.DAT` 0..=16, MC2 `L2.TXT` 0x10..0x118,
   both verified to be the narration verbatim) and the strip drawn in
   SFONT1, which both games ship.

   Note retail only SHOWS the subtitles when the narration cannot be
   heard — non-English builds, or no sound card. Ours defaults to that
   (off) with `render.preference.movie_subtitles` to force them on.

1. **Retail DOES pace these movies.** The banked "no frame pacing"
   claim was wrong. Each movie carries a compiled-in event script of
   `(startFrame, key, index)` records; `'A'` sets the inter-frame delay
   in 120 Hz ticks (default 5 = 24 fps). MC1's intro changes rate 39
   times and opens on a 2.5 s hold. A flat 20 fps would have been
   wrong everywhere. The `'A'` and music records are transcribed in
   `movie::script`; samples and subtitles are not (docs/FIDELITY.md).
2. **Skip is per MOVIE, not per chain** — the abort flag resets at the
   top of every `PlayInfoFmv`. Several call sites pass `allowSkip = 0`:
   both endings and all six MC2 cutscenes are unskippable.
3. **The last frame never shows.** Both games break at
   `frameCount - 1`; the final FLIC frame is the ring delta back to
   frame 0.

**Also newly traced.** `LEVELW1`/`LEVELW2` are two interchangeable
world-won movies picked by timer parity — no level index involved;
`LEVELOSE` is their world-lost sibling. `INTEL.DAT` is an Intel Pentium
bumper gated on CPUID family 5 model 1, so it never plays (MC2 ships it
and never references it). `MOVIE/MVI00000.DAT` is not a movie at all —
it is the attract-mode input recording. `DATA/SMATITLE.DAT` is a static
screen, not an FMV.

**Still owed** (all registered in docs/FIDELITY.md): MC1's TITLE-02/04
animated title overlay;
MC2's welcome still screen; MC1's 40-second attract mode; and a seam
for `LEVELOSE` (a failed MC1 level does not route through a post-level
screen in this engine).

### Player reports 2026-07-21 (round 2) — ALL FIXED + PLAYER-CONFIRMED

All three closed and confirmed by the player in-game the same session.
Every one turned out to be a TRANSCRIPTION defect, not a design gap.

**1. MC1 castle-as-weapon took HALVED damage — FIXED.**
- Root cause: `spreader_tick` (the `(10,1)` corpse flame, retail
  `sub_25130` :28142-58) ran its fire-ring spawn ONCE. Retail runs it on
  EVERY tick of the puff's life, and the puff's life is 1, so retail
  spawns the ring TWICE. Two independent off-by-ones caused this:
  (a) retail's life test reads the PRE-decrement value, ours read
  post-decrement; (b) retail's `& 2` latch guards ONLY the one-shot
  sound (`sub_55370_558A0(.., -1, 3)`), while our port had hoisted the
  whole body under it and returned early.
- Every creature death in MC1 was therefore delivering half its fire
  damage — the castle crush was just where the player could see it.
- MEASURED on a 17-part worm crushed under a fresh level-1 castle:
  **10,400 before → 20,400 after**, against a 20,000 ladder. That is
  retail's reported "destroys the castle outright, or leaves the bar at
  0 so any scratch finishes it", to the unit.
- RULED OUT and documented so it is not re-opened: the ~50% per-cell
  spawn gate `2 * (rand % 0x9D / 79) - 1 > 0` is FAITHFUL. `rand` is a
  self-contained LCG (`9377*s + 9439`), the idiom appears 16× in remc1
  as the engine's RandomSign, and remc2's INDEPENDENT decompile of a
  DIFFERENT binary has the identical gate in the identical loop
  (`engine/EventsFunctions.cpp:22793`). The numeric fit that made it
  look guilty (51 × 400 = 20,400) is explained by the two-pass law with
  the gate intact. Intake, ring size, part count, one-shot latch and
  mail accumulation were all verified faithful and lossless.

**2. MC1 militia never descended — FIXED (one wrong byte).**
- Root cause: the m4 constructor pointed at BEHAVIOR row 0 instead of
  row 16. remc1's `sub_386DE` could not resolve the row symbol and
  substituted `unk_98F38[0]`; the unresolved declaration survives in the
  file, commented out as `//int unk_99138;//fix` (:44891) directly above
  that constructor, and `unk_99138` (:5328) self-identifies as `0x0010`
  = row 16. Every other single-body ctor maps model n → row 12+n
  (12,13,14,15,**0**,17,…), and row 16 is referenced by NO ctor anywhere.
- Row 0 is the FLYER row (`v_14=-4`, `v_20=0xFFFFFFFF`); row 16 is the
  ground-walker row (`v_14=-128`, `v_20=0xFFF080FE`).
- This ALSO closed the separate "archers walk out over the sea like a
  flyer" report: row 16's terrain mask excludes water, and its descent
  gives ground-glue WITH the flying leeway the player described.
- The previous "arithmetically faithful GIVEN the roam" analysis was
  right about the function BODY and wrong about the question.

**3. mc1:000 "extraneous tower" — NOT a spawn bug; m12 settler fixed.**
- The building is settler-built ~44 ticks in (reads as "at init" from
  the cockpit), not a THING row. The player re-checked and confirmed
  retail builds there too — the ORIGINAL premise was withdrawn. No
  load-path admission test differs; the whole THING chain is faithful.
- But three genuine transcription defects were found in the m12 chain
  and fixed, and the player then confirmed the settlers now travel to
  and build at the shore location retail uses:
  - `m12_wander` (`sub_1EED0` :25077-84): pre-decrement `+26` test —
    retail spends THREE wander think-ticks from the ctor's 2, we spent
    two, leaving our `ent_rand` phase 2 draws ahead at BUILD.
  - `m12_approach` (`sub_1F120` :25165): C precedence makes the think
    gate `(f63 % v_26) / 2`, not `f63 % (v_26 / 2)`.
  - `m12_approach` (:25168-70): the same pre-decrement `+26` test.
- The rest of `m12_approach`'s shape (the early return, the
  top-of-function validity check, the walk/gate order and the 2-D range
  test) was banked and has since LANDED — see the audit item below.

Three of the five defects above share one error class, promoted to its
own item below.

### Pre/post-decrement audit — DONE 2026-07-21 (all 70 sites), ACCEPTED

Status: all 11 fixes landed, suite + goldens green, and the player
ACCEPTED the batch. The **m12 settlers were re-checked in-game and
confirmed correct** — the one change that could have contradicted an
earlier player confirmation, since settlers now arrive later (154 -> 241
ticks) even though the build tile is unchanged. The remaining fixes were
accepted on the analysis below rather than individually playtested;
Meteor, Duel, Possess, Lightning and the MC2 lightning trail are the
ones where a future playtest would still be informative.

**The law, corrected.** Retail sometimes writes
`v = field; field = v - 1; if (v <op>)` — testing the **PRE**-decrement
value — and sometimes a genuine post-decrement. The original "retail
overwhelmingly writes PRE" framing was **wrong**, and a blanket rewrite
would have introduced bugs. The real rule is per-FAMILY:

- **MC1 class-10 effect handlers are PRE** (`sub_24F60`, `25410`,
  `25760`, `25A60`, `262D0`, `26360`, `263C0`, `26D20`, `25CE0`, and the
  already-fixed `25130`). Our port had every one of them backwards.
- **MC1 class-9 flight/projectile handlers are genuinely POST**
  (`sub_53DC0`, `53980`, `530C0`, `534C0`, `542B0`, `52B30`). Our port
  had every one of them RIGHT. Exceptions that are also genuinely post
  and must not be touched: `sub_29780` (both branches, the wall-of-fire
  cloud) and `sub_499C0` (class-2 trees).
- **MC2 `EventsFunctions.cpp` is post-DOMINANT** in the effect/mob
  region — 19 post forms to 1 pre in the audited span. Only two MC2
  sites were wrong.

The error therefore correlated with the source file's idiom *shape*,
not with any individual transcription slip.

**Audited: all 70 decrement-then-test sites.** 11 wrong, 55 correct,
4 unclear/no retail counterpart. **All 11 fixed:**

| site | entity | effect of the fix |
| --- | --- | --- |
| `blast_ring_tick` (`:28685`) | meteor blast, life 9 | 9 -> 10 ring passes, 376 -> 417 fires; the ring was landing 90% of its authored ch0 damage (measured) |
| `duel_tether_tick` (`:28956`) | life 8 | 9 grip ticks, not 8 — ~11% of the Duel spell's only output |
| `possess_flash_tick` (`:28433`) | life 8 | 9 ch1 claim ticks — how many balls/houses a Possess converts |
| `hit_flash_tick` (`:28906`) | effective life 2 | 3 flash ticks, not 2 (33%) |
| `fire_tick` (`:28068`) | every fire, life 8 | 9 burn ticks |
| `effect_tick` state 5 (`:28285`) | water splash, life 8 | 9 anim ticks |
| `steal_flash_tick` (`:28933`) | life 8 | 9 anim ticks |
| `storm_cloud_tick` (`:29311`) | life 32 | 66 bolts, not 64 |
| `lava_bomb_tick` (`:28592`) | life 100-163 | one tick of a long flight |
| `mc2_lightning_node_tick` (EF:58910) | life **1** | 2 ticks, not 1 — every lightning trail node was HALVED |
| `mc2_roster` m12 roam (EF:14195) | counter 2 or 5 | one more roam period-hit per cycle, every villager |
| `mana_magnet_tick` (`:31241`) | life 128 | 129 passes |

**`m12_approach` (`sub_1F120` :25164-77) also landed** — the banked
non-mechanical item. Retail has no top-of-function validity check and
no early return: the walk runs BEFORE the think gate on every tick, the
re-aim and the proximity promotion run only INSIDE it, the
patience/dead-anchor bail FALLS THROUGH (so it can still promote to
BUILD the same tick), `+146` is never cleared, and the range test is
the three-axis ROOTED distance (`sub_42340_42680` :52721), not a 2-D
squared one. Verified on the isolated settler fixture: the build TILE
is unchanged at (123,107) — the spot the player confirmed — while the
build tick moves 154 -> 241.

**Goldens re-pinned**, each attributed by probe rather than assumed:
`flight_tier` (C leg only) and `level_005` GOLDEN + OBSERVABLE (B-E;
post-init and A hold). OBSERVABLE moving is correct here — these are
behavior changes, not layout changes.

**Test-fixture lesson.** `worm_chain_dies_from_the_head_...` broke on
the `fire_tick` fix and it was NOT a regression: the worm is a chaser
that parks beside the player, and the one-tick phase shift left it
permanently outside a fixed forward beam (head at full 4600 life after
30,000 ticks). The fixture now aims at the worm, which is what its own
assertion was ever about. A positional fixture can fail for reasons
that have nothing to do with the claim it makes.

### Audit follow-up batch — LANDED 2026-07-21 (castle playtest owed)

The same-transcription-pass items banked by the pre/post audit, worked
through with the player's ruling on the two that were decisions rather
than fixes.

**Class-10 flash omissions — FIXED.** Retail bumps `+26` EVERY tick,
before the life test, at all four flashes (`sub_25760` :28432,
`sub_262D0` :28905, `sub_26360` :28932, `sub_263C0` :28955); the anim
step `sub_42510` was also missing from `possess_flash_tick` (:28437) and
`duel_tether_tick` (:28959), and the class-10 state-5 splash was missing
its one-shot sound 27 (:28288-91) and retail's early return on the death
tick (:28294). Verified live by probe: the possess flash's `+26` now
counts 1..9 across its 9 ticks. `sub_25760` and `sub_263C0` both carry
remc1's `//SYNCHRONIZED WITH REMC1` marker.

**Castle painter `sub_285C0` — FULL faithful restructure (player ruling).**
Retail decrements `+26` at the TOP (:30510) and the whole body reads the
POST value. Four corrections:
- the flatten divisor IS the counter (:30563); ours read the pre value
  and added 1, so every ramp step was divided by post + 2;
- the paint gate is `f26 % 7 == 0 || f26 == 1` (:30646) — post 14/7/1;
  ours fired at pre 14/7/0, i.e. post 13/6/-1, entirely different ticks;
- the tick that reads a PRE value of 1 returns WITHOUT working
  (:30512-16), so the body runs 18 ticks, not 20;
- the finish is deferred behind a negative idle phase that counts UP,
  and only the tick reading -1 promotes protection, sets the castle to
  sub-state 5 and despawns (:30682-84, :30697-709).

The idle length is retail's byte +60, which we do NOT model as a field
because both writers are known and complementary: :47583 spawns the
plain painter with +60 = 1 (25-tick idle) and :56490 spawns the
upgrade-commit painter with +60 = 0 AND the +18 kill bit (:56492). Our
`flags & 0x10000` therefore selects the branch exactly.

MEASURED (the goldens do not reach the painter, so this was probed
directly): raising a castle over ground 40 units below its target, the
ramp was `62,64,...,96,98,100` — flat +2 over 20 ticks — and is now
`62,64,...,88,91,94,97,100`, 18 ticks with the tail accelerating as the
divisor shrinks. **The footprint crush stays lethal** (a 17-part worm
still goes 153,000 -> 0); it simply executes on 18 ticks instead of 20.
**A castle playtest is owed** — this is the one change that alters
behavior the player previously certified.

**MC2 roster operator nits — FIXED.** `EF:16517-19` and `EF:16699` are
exact `== 0` tests and `EF:15890-92` an exact `!= 0`; we had `<= 0` and
`> 0`. No behavior change today (every counter is seeded positive), but
the code now matches. Also `EF:16050-65`: retail's m18 case 2 has an
explicit `case 3:` and a `default: return;` — our `_ =>` catch-all would
have run the spin-down body for any future sub-state >= 4, so the arm is
now literal.

**`mc2_mine_tick` — PARTIALLY fixed; the expiry TEARDOWN is now traced.**
Retail's expiry is post `<= 0` (EF:29842-45); ours was `< 0`, giving the
mine an extra tick. FIXED.

CORRECTION to an earlier reading in this file's history: retail's
`byte_0x46_70 = 6` at expiry is NOT an engine action/handler switch.
In MC2 the dispatch index is `actionIndex_0x45_69` (offset 69, our
`tick70`); `byte_0x46_70` is offset 70 — our **`f71`** — and for the
mine it is a PRIVATE sub-state machine switched on inside `sub_3A8B0`
itself (EF:29881). The MC1 field names are one offset off from MC2's,
which is what made this look like a class-10 table collision. There is
no table collision and nothing blocks the port.

The expiry teardown is now PORTED (EF:30043-86):
sub-state **6** clears the draw flags and (when `byte_0x44_68 == 0`)
advances to **7** with a 10-tick timer; **7** counts down to **9**;
**9** SINKS the mine (`z -= 32 * counter`, accelerating) until it meets
the ground, then spawns a class-10 puff — model **5** over water, model
**0** over land — and despawns. Sub-states 7 and 9 skip the lifespan
countdown (EF:29840). The draw-bit clear `&= 0xFF7FFFFE` uses the port's
established `flags &= !1` idiom (as at `mc2/mobs.rs`, `mc2/tail.rs`);
retail's bit 23 has no modeled meaning here.

VERIFIED by probe on a 40-tick mine: `f71` walks 6 -> 7 (counting 10..1)
-> 9, then one puff spawns and the mine despawns, 51 ticks total. The
sink resolves in a single tick because the mine is linked AT ground
level (retail links it the same way), so the visible teardown is the
draw-bit clear, the 10-tick pause, then the puff.

The DETONATION family remains separately OPEN in
`spell-audit/magic-mine.md` §6 — this changes only what an UNFIRED mine
does when its lifespan runs out.

The port's proximity scan is our own construction, so its `act_life & 0xF`
cadence is not comparable to retail's frame counter.

### m9 grounded arm — FIXED + PLAYER-VERIFIED 2026-07-21

Investigated before ruling, as asked. `byte_0x39_57` is not an
m9-specific flag: it is the standard awake/proximity counter (our `f58`),
written by the m9 ctor (EF:33948-49, always non-zero) and by the
per-frame awake pass `sub_68C70`. So retail's grounded phase is fully
reachable, and the entry condition already matched ours. The bodies did
not. Restored `sub_20940`'s shape (EF:12357-89):

- **The damage/death head now runs FIRST.** Ours returned before
  `mc2_state_head`, so **a grounded hive could not take damage or die**.
  That was a real bug, not a stylistic deviation.
- The stand-up counts UP toward 0 and only the tick that READS -1 fires
  `sub_20F80` (EF:12638 — f71 = 0, f26 = 400, sprite 201; our exit was
  already faithful). Same pre/post family as the audit above.
- An AWAKE hive arms the 50-tick stand-up and scans NOTHING that tick.
- An ASLEEP hive parks `f26` at 0 and feeds in place indefinitely —
  retail never walks a hive no wizard has approached. Ours cycled 400
  walking ticks + 50 grounded, so hives wandered ~89% of the time and
  their offspring landed in different places over a level's lifetime.

Net consume rate is roughly preserved either way (~18 per 450 ticks), so
split rates barely move; what changes is WHERE hives and their offspring
end up, and that a grounded hive is now killable. This is live content:
**80 of 165 levels author m9, 4,577 records** (level 065 alone has 515).

`DEVIATIONS.md:140` stays accurate as written — it only ever covered
WHICH scan the grounded sweep reuses, not whether one runs.

Golden: the MC2 cave fixture re-pinned, GOLDEN (last two checkpoints)
and OBSERVABLE (last one). OBSERVABLE moving is correct — behavior
moved. ATTRIBUTED by probe: the magic-mine teardown landed in the same
batch and moves nothing there.

PLAYER-VERIFIED in-game: hives behave as intended.

REGRESSION TEST LANDED: `mc2_grounded_hive_still_takes_damage`
(`engine/world.rs` tests) — spawns a real hive via `mc2_spawn_m9` in the
existing `mc2_flat_world()` harness, parks the player far so it settles
into the squat, then mails it lethal damage. Proven non-vacuous: delete
the `mc2_state_head` call from the grounded arm and it fails with "it
was immune".

The earlier "cannot construct an m9" note was WRONG and cost time — the
MC2 roster has direct constructors (`mc2_spawn_m9`) and a ready
`mc2_flat_world()` harness sitting in the Phase-4.3 roster probes. The
mistake was reaching for MC1's `spawn_creature` (which builds the MC1
burrower) and `flat_world()` (an MC1 world whose BEHAVIOR table is 31
rows against MC2's 157). **For MC2 fixtures: use `mc2_flat_world()` +
`mc2_spawn_*`, never the MC1 helpers.**

### Magic Mine — mostly CLOSED 2026-07-21 (low priority remainder)

Set spell 23 straight end to end. Two fixes have landed; the pacing
question is unresolved and is the reason this needs its own session.

**LANDED 2026-07-21**
- **The mine was INVISIBLE** — `(10,78)` was missing from the MC2
  class-10 draw allowlist (`world.rs::drawable`). It ticked, armed and
  detonated correctly but exported no pose, so a cast looked like a
  carrier that flew off and dissolved. Player-reported. NOT a regression:
  no commit ever had 78 in that list, so the mine has been invisible for
  as long as it has placed a persistent mine — which is exactly why it
  "used to work" (the OLD broken behavior, a fireball bursting on
  contact, was the visible one). Regression assert added to
  `mc2_magic_mine_places_a_persistent_mine_not_a_fireball` and proven to
  fail without the fix. **LESSON: entity-counting tests cannot see a
  missing pose. `count(class, model)` and "the player can see it" are
  different assertions.**
- **Expiry**: post `<= 0` (EF:29842-45), and the full teardown chain
  6 -> 7 -> 9 (draw-bit clear, 10-tick pause, accelerating sink, puff
  model 5 over water / 0 over land, despawn). Probe-verified.

**LANDED 2026-07-21 (round 2, player retail observations)**
- **The mine HOVERS.** EF:29862-72: it clamps up out of the ground then
  floats toward ground + 1024 in +/-48 steps with a 96-unit deadband
  (gated on `f69 == 0`). Ours sat on the ground because the whole block
  was missing. Player-observed in retail ("rises to about castle-tower
  height, same sprite"); probe-verified 3200 -> 4160 and holding.
  Sub-states 7 and 9 are excluded from this block in retail precisely so
  the sink is not fought by the float.
- **A TRIGGERED mine now tears down instead of vanishing** — it hangs,
  sinks to the ground and goes out in a puff, reusing retail's own
  expiry chain. Probe-verified end to end: hover 4016 -> trigger ->
  10-tick hang -> accelerating sink -> puff -> despawn.
- **THE BLAST NOW REACHES ANYONE.** Player-reported: a wizard right
  beside a tripped mine took nothing. Cause: `ent_overlap` sums BOTH
  parties' extents and the mine ctor never set `f80/f82/f84`, so the
  blast was a POINT — pre-existing, and the 1024 hover made it
  unmissable. The detonation now opens a 1024 (4-tile) box for the
  write and restores it afterwards, and SPITS a (9,0) bolt at whatever
  tripped it (§5 step 4 calls the detonation a *relaunch*, not an area
  write; the player expected a projectile). Regression test
  `mc2_magic_mine_blast_reaches_a_neighbouring_wizard`, proven to fail
  without the fix. Measured 750 damage at 2 tiles = 250 area + 500 bolt.
  All three DELIBERATE, registered in `DEVIATIONS.md`.

  **FIXTURE TRAP worth remembering: SPAWN GRACE ate the damage.** The
  first four probe runs read zero and looked like the fix had failed.
  A freshly built world gives the player grace, and the mine trips ~28
  ticks in — inside it. The test now burns grace off far from the mine
  and asserts `vitals().grace == 0` before proving anything.

**CAST PACING — RESOLVED, NOT A BUG.** The player tested the
delay-after-cast progression against retail and it is FAITHFUL. The
LABEL_16 duration block is correct as ported; no change needed. (Keep
the analysis below only as the record of why.)

**RETAIL OBSERVATIONS — the spell is largely broken in retail.** The
player could not get a retail mine to trigger at all, despite a rival
wizard flying over it repeatedly, and saw no projectile from mine to
victim. This CORROBORATES §6 open question 1: `sub_50840` leaves
`word_0x36_54 = -1` and no writer that sets it was ever pinned — if
nothing arms it, a retail mine never fires. Ours is therefore MORE
useful than retail's. Ruling: implement faithfully where cheap, but do
not sink time into matching a spell nobody can use.

**THE (former) OPEN QUESTION — cast pacing, kept for the record.** The player observes tier 0 casts
fast, tier 1 slower, tier 2 very slow, and expects that with plenty of
mana several mines should be placeable in sequence (each merely blocking
mana REGENERATION). Established so far:
- The mana gate is NOT the throttle and is faithful: retail EF:60953
  compares the WIZARD's pool `mana_0x90_144` against the spell's
  `maxMana_0x8C_140`, and our `mc2_cast_gate` compares `player.mana`
  against the spell cost. Same test.
- The throttle is one level up: magic mine (0x17) sits in retail's
  LABEL_16 band (EF:60946-48), which SKIPS the cast entirely while the
  spell object's timer `word_0x2E_46` is non-zero, and `sub_5F7B0`
  (EF:60973) arms that timer from the tier's duration `word_0x30_48`.
  Longer tier duration ⇒ longer re-cast block. Our port mirrors the
  shape (`f26 = f28`; blocked while `f26 > 0`).
- **UNVERIFIED**: whether `sub.word_0x18` is the right duration column
  per tier, and whether the mine belongs in the LABEL_16 band at all.
  If retail really allows several simultaneous mines, one of those two
  is wrong. Get the per-tier numbers (duration, sub_spell, maxManaLimit,
  cost) via a THROWAWAY internal test — do NOT add a public debug
  accessor to the shipping crate.
- This ties directly to `spell-audit/magic-mine.md` §6 open question 3:
  "Carrier count per cast — `sub_6CAC0` fires on the
  `word_0x2E_46 == word_0x30_48` tick; believed exactly one mine per
  cast; verify it does not re-lay while the spell-holder lives." That
  IS the player's question. Settle it against retail first.

**Still open from `spell-audit/magic-mine.md` §6**
- `word_0x36_54` / `word_0x34_52` provenance (the armed gate).
- The exact `sub_6DCA0` detonation blast for spell index 23 — our
  detonation is an approximation, and the port's proximity SCAN is our
  own construction (its `act_life & 0xF` cadence is not comparable to
  retail's frame counter).
- Retail sets ACTION-adjacent state via `byte_0x46_70`; note this is
  our `f71`, NOT `tick70` — see the naming trap below.
- Model-78 vs -78 homing (`sub_67960`), low priority.

**THE NAMING TRAP — cost this session real time, read before starting.**
Our `Ent` field names come from MC1 offsets, and MC2's equivalents sit
ONE OFFSET LOWER in the name. MC2 `actionIndex_0x45_69` (offset 69) is
our `tick70`; MC2 `byte_0x46_70` (offset 70) is our `f71`; the spell
object's (timer, duration) pair `word_0x2E_46`/`word_0x30_48` is our
`f26`/`f28`. Reading the NAME instead of the offset produced a confident
wrong claim (that MC2 action 6 collided with MC1's standing fire, and
that the teardown was therefore unportable). ALWAYS resolve MC2 fields
by offset, never by our field name.

### Saves + in-game menu — LANDED 2026-07-21 (playtest owed)
- Mid-level save/load and the pause mini-menu, per `archive/DESIGN-SAVES.md`
  (which now records status, deviations and the remaining open item).
- Sim payload codec `mgc_sim::snapshot` — dependency-free, hand-written,
  exhaustive destructure out / exhaustive struct literal back, so a new
  field is a compile error in both directions. Restore APPLIES onto an
  already-built world (the level package supplies `Gen::assets`/`retile`
  and the `&'static` chassis slice); an identity fingerprint refuses a
  foreign world before writing anything.
- `.mgcs` container (`mgc_formats::mgcs`): ZIP + `save.json` header +
  `campaign.bin` + `snapshot.bin`, DEFLATEd (unlike `.mgcl`, which stays
  Stored for its committed hashes). Header alone drives the slot list.
- Slot model: `<stem>.mgcs` native + `<stem>.gam` retail export beside it;
  native always wins, the `.gam` is read only when no native file exists.
- **OPEN — mid-level option gating.** `entity_pool_size` (and anything else
  that resizes the pool) still has no "mid-level changeable" axis in the
  settings registry, so it can be changed from the in-level Options layer.
  The snapshot identity check turns the result into a REFUSED load rather
  than a corrupt one, but the option should grey out instead.
- UI round 1 (player review) landed: the panel's own "PAUSED" title
  dropped (the retail banner stays — banner = state, panel = menu),
  results moved to the toast line (they overflowed a
  narrow panel), cursor stays free for the whole pause (`set_grab` refuses
  to re-grab while paused — closing the big map was re-capturing it), Esc
  from Options returns to the mini-menu instead of unpausing, the two
  panels are mutually exclusive on screen, panel background darkened for
  contrast over sky and desert.
- UI round 2 (player review): loading is now decided by the SLOT, not by
  where you loaded from — a mid-level slot resumes into its level from the
  main menu / world map too (it used to adopt the record and leave you on
  the menu, so entering replayed the level from the start), and a
  campaign-only slot loaded in-level exits to the hub. Frontend slot lists
  route through `saves::scan_slot` and show `L<n>` on a resuming slot.
  Slot-row text is letters/digits/spaces/`%` ONLY: the messaging font is
  the game's FONT1 bank at `glyph = byte + 1`, so `*` drew as a lightning
  flash and an em dash drew as three junk glyphs.
- UI round 3: EVERY slot names its level (`L3`), and a resuming slot adds
  the mana percentage the run had reached (`L3 15%`) — one shape, the
  suffix says which, and the number doubles as a how-far-in marker.
  `level` was promoted onto the save header (both kinds of save carry one,
  and one copy cannot disagree with itself); `InLevel` gained `mana_pct`
  and lost its duplicate `index`. `SAVE_VERSION` 1 -> 2.
- Rebase hazard, seen once already: the exhaustive destructure makes any
  commit that adds a `Gen`/`World`/`Player`/rival field FAIL THE BUILD
  until the field is added to the codec (`Gen::pal_flash` from the
  purple-flash commit did exactly this). That is the design working. Judge
  separately whether the addition also needs a `SNAPSHOT_VERSION` bump: it
  does whenever the byte layout shifts, which is essentially always,
  because the identity fingerprint is written AHEAD of the payload and so
  cannot catch the misalignment.
- The mini-menu is TEXT rows, not the icon set `archive/DESIGN-SAVES.md` ruling 7
  anticipated: text carries the label, level and progress that icons
  cannot, in a panel narrow enough to leave the HUD and the map's live
  view usable. No `assets/static/` art is owed unless the panel grows an
  icon row.
- Version gates now read the version through a minimal PROBE struct before
  deserializing the rest. A bump is precisely when the schema changed
  shape, so a full parse fails on an unrelated field and buries the
  explanation (v1 saves reported "invalid type: map, expected u32"). Same
  law on the payload side. Regression test carries a verbatim v1 header.
- Cross-version SALVAGE: a `.mgcs` this build cannot apply still gives up
  its campaign record (`mgcs::recover`) — that record is RETAIL's byte
  layout, so it survives any version of ours; only the resume, whose field
  order is `SNAPSHOT_VERSION`'s, is lost. Such a slot lists amber + `old`
  (`SlotInfo::stale`) so the loss is visible before it bites; re-saving
  heals it. Verified against real v1 saves.
- Per-slot save NAMING removed (all three frontends). Every editor seeded
  itself from the RENDERED slot row and wrote it back as the name, so the
  `L<n> <pct>%` suffix accumulated on each save. Slot names are now derived
  — stored label = player name, level/progress composed at draw time — and
  `SaveTo` carries no label. The `SetName` dialogs (player name) stay and
  are the only writers of a stored label.
- Playtest round 1 fixes: MC2 save rows read 0% because the figure came
  from `Player::banked` — the CASTLE panel's numerator (`(10,45)` houses +
  `(3,2)` castle stored), which stays 0 under MC2 until a castle stands.
  Now `World::player_mana_share_pct`: what the player POSSESSES, minus the
  intrinsic 1000 every wizard is born with (so a fresh level reads 0%) and
  clamped, because MC2 seeds its world total at 1 rather than at that base.
- Also fixed (PRE-EXISTING, surfaced by loading): `install_level` cut sfx
  and speech only when an outgoing session existed, so a launch FROM a
  frontend never cut the world map's narration of the upcoming level and it
  played on over the level. Now unconditional, matching retail
  (remc1 :59992-94) and the observed behaviour that entering early cuts the
  map line and the level plays its own, different narration.
- **PLAYTEST OWED**: mini-menu placement against both HUDs and both map
  screens (`minimenu::{MARGIN, TOP, WIDTH}` are the dial); the load
  round trip in all four menu/level x resume/hub combinations; the MC2
  tier-name fix from prereq 3.

### Player reports 2026-07-21 — FIXED 2026-07-21 (playtest owed)
- **FIXED 2026-07-21 (round 2) — MC1 militia/"archers" roam unbounded
  and hover over water.** Same single root cause as the "never descend"
  report: the m4 ctor used BEHAVIOR row 0 (the FLYER row, terrain mask
  `0xFFFFFFFF`) instead of row 16 (ground-walker, mask `0xFFF080FE`,
  which excludes water). See the round-2 entry above. The analysis
  below is kept because its REPRODUCTION is still the right probe, but
  its "faithful given the roam" conclusion is superseded.
  Player report (mc1:02, level-independent): an archer walking
  perfectly horizontally out over the sea, "like a flyer". Player is
  certain retail militia never do this — they get flying LEEWAY (so a
  collapsing building can pop them into the air without killing them)
  but stay near their building. Most creatures are visibly glued to
  the ground.
  - **REPRODUCED headlessly** (probe, not kept): an m4 spawned on
    height-22 coastal land walks off the coast and is left 713 engine
    units above the seabed, shedding 1 unit/tick — ~700 ticks of
    horizontal flight. It then crossed 100+ tiles and the map seam.
  - The hover is arithmetically faithful GIVEN the roam: in-band
    descent is 25% of `v_14` = 1/tick, and the militiaman crosses a
    tile in 8.5 ticks, so it can shed only ~0.27 height units per
    tile. On ordinary slopes that lag is sub-height-unit (invisible =
    "glued"); at a coastline it is 20+ height units at once.
  - **RULED OUT — all verified byte-exact vs the decompile**: the
    altitude clamp (`sub_42000`, and `sub_196E0` calls it, NOT the
    water-aware `sub_42090`); the ground reference (`sub_11F50` →
    `sub_724C0`, bilinear ×32); the move permit (`sub_11640` mode 1)
    and behavior row 0 (`v_20 = 0xFFFFFFFF` really does permit water);
    the roughness probe (`sub_19650`); the position commit
    (`sub_41C70`, pure list maintenance, no ground snap); the m4 ctor
    (`sub_386DE`, speed 30); the wander draws; the acquisition ladder;
    and the runtime dispatch loop (`sub_41780_41AC0` walks all 1000
    slots every step — NO per-entity stagger, hypothesis refuted).
  - **RULED OUT — the house leash.** `sub_1B5D0`'s steer-home branch
    keys on `+146` holding a (10,45) house, but every writer of `+146`
    on the m4 path is gated on `class == 3`, and the ladder explicitly
    EXCLUDES (10,45). Dead code for model 4. The two lists it scans
    are `+36462` = wizards and `+36418` = `str_36382x[9]` = the m9
    burrowers — neither can yield a house.
  - **So nothing in the transcribed decompile bounds the roam.** Every
    piece verified faithful while the aggregate is visibly wrong —
    which in remc1 has twice meant incomplete transcription (the
    truncated class-9 state table; `sub_41C70` "SYNCHRONIZED" with a
    missing body) rather than a mis-port.
  - NEXT (measurements, not theories): (1) compare our militia
    POPULATION over time against retail's emit law — if villages
    over-produce, the rare wanderer becomes a common sight, which fits
    "archers are generally not constrained anymore" better than any
    single-creature explanation; (2) check for an m4 lifetime/despawn
    we have dropped. Player is gathering more data.
- **OPEN — MC1 tick rate is an unverified ESTIMATE.** Retail advances
  the sim once per RENDERED FRAME (`DrawAndEventsInGame` :41672; the
  F3 speeds are 4×/16× of it, which our `game_speed` models
  correctly). MC1 ran uncapped and hardware-bound, so `TICK_RATE_HZ =
  24` is borrowed from MC2's 24 FPS limiter (documented as such in
  mgc-sim/src/lib.rs). This scales every MC1 motion in wall-clock
  terms. Cannot by itself explain the militia roam (a wrong constant
  makes the ocean crossing sooner or later, never impossible).
- **OPEN — two PROVEN MC1 militia deviations** (independent of the
  roam, both from the `sub_1B5D0` trace): (1) our idle ladder has a
  port-invented "nearest building within 0x1000 → walk in" step and
  routes it through `mob_death` — retail's idle has NO house step at
  all (its walk-in lives in the dead leash branch and is a SILENT
  absorb), so ours leaves militia corpses at houses; (2) militia aggro
  reads a single global `player_aggro` flag instead of retail's
  per-wizard `+528` wanted timer, so MC1 RIVAL wizards never draw
  militia fire. Also minor: retail's idle zeroes `+26` every tick
  (:22482), ours does not.
- **The castle-transformation kill was too weak in BOTH columns**
  (player report: "castle building works as a destruction spell, but a
  lot less than it should"). The mechanism is NOT a movement lockup —
  it is an explicit model-keyed execution over the footprint, and
  immunity is by MODEL, not by flight (hence wyvern/griffon immune,
  dragon/worm/bee/vulture not).
  - **MC1: the lethal area was under 40% of retail's.** `sub_40E20`
    fires for EVERY cell of every positive RLE run, over rows
    1..=level, BEFORE the cell byte is read (:30634 precedes :30635) —
    an EMPTY footprint cell kills exactly like a masonry one. Our
    `build_footprint_kill` gated on `byte != 0`, shrinking a level-7
    castle's sweep from 2304 tiles to 899, and swept only the top row.
    Both fixed; test `castle_kill_sweeps_empty_footprint_cells_too`.
    MC1's exemption list {6, 8, 16} = Kraken/Griffon/Wyvern and the
    owner-spare were already faithful.
  - **MC2: the castle path never purged at all** (below).

### Player reports 2026-07-21 — FIXED 2026-07-21 (playtest owed)
- **MC2 castles were not lethal to what they rise over.** Retail's
  (10,42) castle painter runs `sub_57390` over EVERY cell of the
  cumulative footprint on EVERY tick of the 19-tick rise (EF:27826-27),
  gated on the painter's kill bit (`byte[2] & 1`) which only the
  level-UP spawn sets (`sub_60480` EF:61602) — never the damage repaint
  (`sub_5FBD0`). Our MC2 painter never purged at all, so castles only
  killed incidentally (creatures the terrain lift happened to strand);
  the player's report was MC2 fireflies (model 19, unprotected)
  surviving builds they should not. MC1's column already had the arm
  (`build_footprint_kill` under `+18 & 1`, :56492). Now ported as
  `F_BUILD_KILL`. Also fixed in `mc2_building_clear_tile`: the skip
  test is retail's OWNER compare (`victim.id24 != owner`), not a slot
  compare — a wizard's own creatures walk through their own
  construction — and the victim's killer/attacker pair
  (`word_0x24_36`/`+38`) now credits the builder. The slot compare was
  indistinguishable on the village path (an unowned building's `id24`
  defaults to its own slot) but wrong for an owned castle. Test
  `a_rising_castle_executes_what_stands_under_it`.
- **Destroying a castle left a flagless "tower" standing** (both games,
  site-dependent; player-confirmed fixed on the mc2:06 ocean site,
  rival Belix). The remnant was never an entity — the (3,2) castle
  entity CARRIES the flag and despawned correctly; what stood was
  painted TERRAIN. Three independent causes, all now closed:
  1. **MC2 datum was the corner MEAN, retail's is the perimeter MIN**
     (`sub_4AA40` EF:33399 → `sub_48E60`/`sub_48F20`, init 250). The
     stamp writes `datum + cell` absolutely, the demolish only
     subtracts `cell` back, and nothing saves the original ground —
     the min datum is exactly what makes that asymmetry land flush.
     The mean sat above the low side of any slope and left
     `mean - ground` of stone mesa. Flat sites hid it → the
     site-dependence. `mc2_castle_site_z` now uses the existing
     verbatim `mc2_perimeter_min`. Test
     `a_castle_on_a_slope_leaves_no_mesa_behind` (18-unit mesa before,
     0 after).
  2. **Level-0 castles stamped BUILD row 1.** Retail's build row IS
     the level, unclamped, and row 0 is EMPTY (w = h = 0) — a level-0
     castle is a bare flag owning no terrain, which is why the destroy
     path never un-stamps it. Both columns clamped the row up to 1
     (MC2 `mc2_spawn_castle_painter`; MC1 `spawn_starting_castle`
     passing `lvl + 1`, plus the painter/repaint clamps), raising a
     tower nothing would ever remove. MC1's demolish also lacked
     retail's `if (level > 0)` guard (:56506), so a level-0 death
     demolished a row-1 footprint that was never built. Test
     `an_authored_castle_owns_only_its_own_levels_terrain`.
  3. **MC1's un-stamp could silently not run at all.** Retail builds
     the fake collapse event in the SCRATCH slot (entity 0, :56517-24)
     and never allocates; ours took a pool slot with no else-arm,
     right after `castle_eject` can spend up to 36 — on a
     pool-pressured level the whole demolish was skipped and the full
     tower stayed with its flag gone. Now uses `SCRATCH`.
  - Level 005's goldens re-pinned (authored rival castles stamp one
    ring less at load): all six layout hashes move, the OBSERVABLE
    projection moves at post-init ONLY and holds A-E — the evidence
    that footprints changed and play did not.
  - NOT a bug, deliberate: the leftover flatten pad itself. Retail has
    ONE heightmap, no backup, and the demolish is a relative
    subtraction — a destroyed castle genuinely leaves its levelled pad
    (plus up to +19 of byte-wrapping LCG rubble jitter, faithful in
    both columns). Only the EXCESS above the datum was ours.
  - The jitter is faithful, but it is meant to be SMOOTHED: both
    un-stamp sites end in `SetHeightmapByBuildingArea_48B50`
    (`Gen::mc2_smooth_heights_region`), a gated raster-order 3x3 height
    average over the footprint. The castle's had it; the MC2 BUILDING
    demolish (`World::mc2_house_collapse`) did not, which is why a
    collapsed tower left extremely jagged ground where retail leaves
    almost none. Landed 2026-08-11 — ledger §THE MC2L1
    FOUR-DEVIATION SESSION.
- **MC1 Global Death had no player-visible effect.** Retail's only
  sighting of the spell is a full-screen palette flash at the
  detonation — `sub_44BE0(owner, 3)` → `Type_160+152`, painted by the
  frame tail (:41813 case 3: red +48, blue saturated, green untouched
  = a violet wash, then the case-1 `FadeInOut(pal, 4, 1)` ramp home).
  The field handler had it commented as OPEN/unported. Now ported as
  `PalFlash` (Gen, hash-silent presentation channel) → `PlayerVitals
  .pal_flash` → the ui.rs overlay, armed only when the field's owner is
  the local player (retail gates on the slot compare). Test asserts the
  row-3 arm inside `global_death_fuses_at_the_caster_into_the_flat_plane_field`.
- OPEN, same channel: **row 6** (`sub_44BE0(v4, 6)` at :29215 — the
  warm R+48/G+32/B+32 wash when a creature lands a charge on the
  player) is still unported; the `PalFlash` channel is now there to
  carry it. Rows 2 and 7 are already drawn (hit flash, death grey-out).

### Player reports 2026-07-20 — FIXED 2026-07-20 (playtest owed)
- **Collapse-evacuee militia FLOAT** (level 04 "floating archers"):
  fixed in the dormant-arm BONUS below (restored the militia movement
  core + wander).
- **Player-death camera slid along the terrain** instead of pinning at
  the corpse. The dead-state handler (mgc-sim/lib.rs) zeroed only the
  MC1/MC2 carpet speeds; under the Enhanced mover the camera rides the
  float velocity (`flyer.v*`), which kept drifting. Fix = zero BOTH the
  carpet speeds AND the enhanced float velocity BEFORE the move whenever
  dead (a true pin); FALLING keeps its faithful glide; the existing
  turn-toward-killer (killer_pos → yaw) completes the retail behavior.
  Test `dead_wizard_pins_at_the_grave_under_enhanced`.

### Player reports 2026-07-19 — FIXED 2026-07-19
- Rival castle sink PLAYER-CERTIFIED; firefly damage accepted as
  FAITHFUL (byte-verified; the opt-in "stronger firefly" lever would
  be the `f63 % 35` shooter throttle). Playtest still owed on: worm
  possession color, thrust-model switch hand-off, and the
  enhanced-thrust×Faithful-altitude vertical (decline-crosstalk
  ruling: the altitude AXIS owns vertical on both thrust models —
  see DEVIATIONS "move_enhanced (level-plane thrust)").
- MC2 big map coverage FIXED 2026-07-19 (playtest owed): map-screen
  pane now spans the faithful 318.75 tiles vertically (retail
  DrawMinimap scaling 204, EF:21840-49) instead of the bare 256-tile
  world; retail's 4.6% terrain-vs-entity horizontal misalignment
  deliberately not reproduced (DEVIATIONS mgc-render).

### Player reports 2026-07-19 round 2 (traced 2026-07-19)
- **MC1 level 04 trigger altitude-gated — TRACED FAITHFUL, no fix.**
  The retail probe is the same 3-axis AABB (sub_118C0 :16963 has a z
  arm; the 2-D suspicion is refuted — :58490 is unrelated UI code):
  class-11 volumes get authored horizontal extents but a FIXED 4096
  vertical half-extent (:44038 → sub_37130 :43790), probe the
  sprite-44 wizard at flight altitude, and resnap z to the CURRENT
  (dug) ground on quiet probes (:67632). Retail's speed-0 sink is
  the same 8 units/tick (:55171). RESIDUAL suspect if the player
  still finds it worse than retail: our hole dug DEEPER than retail
  at the trigger cell (terrain bake / crater depth — the same
  memimage-compare class as the MC2 foundation angle-nibble check).
- **MC1 level 04 trigger-spawned skeletons passive — FIXED
  2026-07-19, TWO stacked gaps** (playtest owed). Skeleton = the
  (5,9) burrower mound. Gap 1: the state-55 handler `m9_hidden` was
  missing retail sub_1D060's awake-gated WIZARD scan (:23796-23833)
  — the only path that ever targets the player from state 55. Gap 2
  (the player-visible one — reported again post-fix-1 as "attack my
  castle but never me"): mounds bury 400 ticks after emerging with
  no player near, and the buried arm was a stub with NO way back up
  — retail sub_1D6D0 (:24016-28) arms a −50 countdown when the
  wizard enters the 24-tile wake gate and the mound RISES again
  (sub_1DDB0 :24273); the level-04 army had buried itself before
  the player ever arrived (the castle worked because building it
  nearby kept nearby mounds awake → never buried → unbounded-radius
  castle hunt). Both ported; tests
  `m9_mound_scans_the_wizard_when_awake` +
  `m9_buried_mound_rises_near_the_wizard`; live-level verify:
  dis-2 army buries player-far, then 16 risers chasing within 400
  ticks of hovering it. Goldens unmoved. The roam/convert
  self-spawn LANDED 2026-08-10 (`mobs.rs::m9_convert`, mc1l5 dig):
  the undead-army growth — a mound with nothing to chase eats the
  nearest m4/m12/m13 within reach 0x600 every v_26 ticks (victim
  menu cycles on `f63/v_26 % 3`) and mints a fresh (5,9); no
  corpse, no ball, no kill credit; buried mounds convert only
  asleep (player-far). Pinned by
  `m9_mound_converts_civilians_into_skeletons` (both arms + the
  wild-mound owner-stamp quirk).

### MC2 fidelity debts
- `mc2_seed_default_spells` unconditionally seeds `{0,1}` at EVERY level
  init — a floor retail does not have. Spells are HOARDED across levels
  in both games; retail's `InitialiseSpells_54A50` (EF:38721-62) grants
  the carried book alone on campaign levels > 0, and falls back to the
  level's authored `starting_spells` row only when there is no carry
  (level 0, or a direct `--level N` = `LEVEL_LOADED_FROM_ARG`). Two
  consequences: (a) a spell permanently lost to the wraith steal would be
  handed back by us and not by retail; (b) direct `--level N` playtest
  launches should get the authored row (8 spells on mc2:003) instead of
  2 — the row is imported and in the bundle but consumed only by rivals.
  The campaign CARRY itself is already correct (`apply_campaign_book`),
  as is the HAND binding (`mc2_rebind_hands_canonical`).
- Jar re-collect / double-manifestation side bug — mask desync lets a
  carried spell's jar re-collect; root fix = set the SpellEnabled mask bit
  in `mc2_adopt_manifestation` (hashed path — needs golden verification).
- Human MC2 death does not scatter its spellbook — `mc2_scatter_spells`
  (cast.rs) is uncalled; wire into the human-death path + re-mint from a
  `known` mask on respawn.
- Rivals: DEFENSE disguise VISUAL unported (the state machine, tier
  pick, shadowing and speed law ARE faithful — sub_15FC0/sub_161A0);
  heal rate = MC1 stand-in; per-projectile hate-feed timing; creatures
  aggro only the human. (Scroll-grab cast IS wired at rivals.rs:1968;
  steal-mana is absent from retail's rival rotation — neither a gap.)
  Rival-spell tail: Duel tether, Beyond-Sight T2,
  rival rebound-window mirror.
- Model helpers: doomsday (5,10) x41/6 helpers; m22 worm head + link-length
  provenance (floor 96 APPROX); (10,76) fire-sphere own creator; (10,9)
  dome geometry helpers; (10,54) magnet ball-pull pending (9,17) chain;
  misfit class-11 models 5..=11 switch handlers (ids.rs).
- WATCH: (10,83) dome LOAD anchor corner-vs-center (load path may sit a
  tile off; cave levels certified, so subtle if wrong).
- Stage engine: objective type 4 (escort) unported — 0 shipped levels,
  completeness only; type 6 stays `_ => false` (dead in retail);
  per-model phase-7 held-wrapper ambient tails (sound rolls, ground
  re-snap) partly swept; kind-3/4/5 `&2` handle-tracking branches dormant.
- Class-5 stale-slot deaths — banked root fix = hash-excluded per-slot
  generation counter (REVIEW PR-3 class).
- Global Death 0x12/0x13 m18+m19 homing reconstruction banked.
  (Type-68 `sub_21F60` line removed 2026-07-20 — audit found it ported
  as the doomsday devour pass, doomsday.rs:466.)
- Metamorph carpet-hide + spell-name level-up banner (presentation).

### MC1 fidelity debts (the INTERIM inventory)
- Per-spell emission approximations: earthquake m11 digger (vs c10 m15
  crevice walker), undead army 3 fixed skeletons, lightning-storm 8-bolt
  fan, wall-of-fire 5 standing fires, mana-magnet 30-tick puller,
  steal-mana wizard-only.
- Castle housekeeping cluster: balloons/levels/respawn, overflow ejector,
  castle HP/damage/downgrade, per-level win threshold `byte_38C93`, m42
  painter delta-array. (The deferred mana-collection/castle economy home.)
- Spell-audit gaps: Possession tier→(10,54) magnet-child link MISSING
  (tier-1 attracts no mana); Lightning `sub_66FD0` L1/L2 burst unported;
  spell 19 blocked on (10,72); placed Magic-Mine variant (spell 23);
  meteor charge-tiered fuse (proj.rs TODO); quake subtype-23 wrapper
  unverified; mana-regen mid-burst suppression branch; steal-mana
  wizard-gate decision; fools-mana OPEN-1 (OPEN-2 RESOLVED 2026-08-03:
  authored default b46=0, authored spheres DO retaliate — audit doc §2b);
  base-MC1 spell-20 multi-bolt
  spray.
- Starting-spell level-file source undecoded (1042-byte reserved block
  @0x30); jar spell-id-from-model65 unverified; blue-seed cross-level
  carry (var_916) untraced.
- MC1 song-command source untraced (runtime song = level%3 interim); FM
  renderer ignores velocity/pitch-bend (accepted interim).
- Genies MANA STEAL unverified (the mobs.rs mana-track concern).
- Shift+K wizard suicide parked; GEN_MAP pre-header semantics open.

### Hidden Worlds
- PLAYTEST OWED — spell-20 homing/rebalance + napalm fork uncertified.
- TMAPS-156 arctic tree: blank vs neighbor-155 pixels — trace before
  touching the frame-less skip.
- Spell-20 visuals trace + `mc1-arctic` bitmap verify; napalm↔spell-20
  relationship trace; world-map grid 20→10 (optional UI).
- Mana-shield model-53 reflect gate (latent until wizard shields ship).

### App / frontend
- MC1 menu click samples (snds13 bake member + per-mode SFX bank switch).
- Inert menu screens: Multiplayer / SetKeys / Language / Joystick (both
  games' equivalents).
- Post-finale MC2 map refuses resume without `--new-game`; trail-stamp
  gate + editor (14,3)→(11,12) marker remap untraced.
- MC2 map 4-button edge overlay (save/load/next/exit); retail right-click
  replay nuance.
- WATCH: temple hover sprite polarity may be inverted; langindexbuffer[2]
  byte-verify; OK2/CANCEL2 pressed-state sprites.
- Non-4:3 aspect handling: DONE for the in-game HUD, the 3D view
  (`mgc_render::HudFrame` + `flight_fov_y`), the FMV player, and BOTH
  main menus — the temple and the MC1 globe now centre, via
  `ui::letterbox`/`unletterbox`/`offset_quads` (player-reported: they
  sat in the top-left corner). Borderless fullscreen is now the
  DEFAULT (`render.preference.fullscreen`), which is also the faithful
  reading — DOS ran one exclusive full-screen mode and offered no
  window.
  The WORLD MAP is centred too, which took three more pieces than the
  menus. The edge-scroll now reads "at or BEYOND the picture edge", so
  the WHOLE letterbox bar scrolls instead of one boundary pixel (player
  ruling) — on the barred axis, whichever it is: left/right on a wide
  window, top/bottom on a squashed one. The confined pointer is clamped
  to the WINDOW, not the picture, or the map's right edge is
  unreachable. And map content is now CROPPED to the 640x480 screen
  (`ui::clip_quads`): the map scrolls, so portals and dressing hang off
  the viewport constantly, and what the window edge used to clip for
  free was landing in the visible bars.

### Render
- Anti-aliasing (`render.preference.anti_aliasing`, default off):
  off / msaa / 1.5x / 2x. The supersample modes are PLAYER-VERIFIED
  (1.5x "does most of the job"; measured 175/130/95 fps for
  off/1.5x/2x on integrated graphics). 3x was tried and dropped — the
  gain was marginal and it visibly cost frames.
  **MSAA is UNVERIFIED** — nobody has run it. It is startup-only
  (baked into all nine pipelines) and the mirror pass had to become
  multisampled with a resolve, since it shares those pipelines. Watch
  for: the reflection resolving correctly, and MSAA being a no-op on
  sprite silhouettes, which is expected (the discard-based cutout is
  immune to it) rather than a bug.
- OPEN, low priority: at 3x supersampling the radar's player cross
  vanished entirely. 3x is gone, but the mechanism was never found and
  a thin HUD mark may simply be dimming at 2x. Suspects: intensity
  dilution of a 1-scene-pixel mark under the box downsample, or the
  ui.wgsl pixel snap collapsing a sub-pixel quad to zero width.
- TMAPS textured fullscreen map (the "green look" for MC1's book map).
- SKY bitmap cloud-plane bake + the 50-slot night/cave dynamic-light
  system; `hmap2` water-reflection plane.
- Camera ROLL render term (faithful bank), ending blur/fov-dolly, palette
  flashes; MC2 map entity billboards (needs MC2 sprite bake).
- Tremor SHAKING presentation (trace what retail drives it with).

### Audio
- REVIEW audio column: D1 per-id request modes vs remc2 dispatch (ids
  <47); D2 (owner,id) channel key — VERIFY FIRST, likely already landed
  (stale checkbox); D3 missing cue sheet must not drop sounds/music; D4
  stale bank-1 docs; D5 war-stem cc11 expression curve; D6 remaining hard
  cuts; D7 misc cleanups.
- Type-31 beacon speech variant (secret rows unreachable until wired);
  speech-onset palette flash; INTRO/CUTS sub-songs unbaked; F-section FM
  render (faithful alternate); unwired MC2 sound sites (m4/m10 …).
- Clean-CD narration reconstruction if an uncorrupted pressing surfaces
  (bake override hook exists; GOG track heads corrupt — see memory/tools).

### Playtests owed
- HW delta; MC2 stage-hold levels (esp. level-014's dormant model-18);
  Nyphur rival engagement (hate-feed timing feel); MC1 24Hz feel
  re-certification; `castle_lock_active` window feel; duel tether with a
  creature nearby.

### Retail checks owed
- Castle Shift+L self-destruct depth; worm-vs-castle fire-cell magnitude;
  meteor/castle blast-site tracking; retail level-039 fail-open look; MC2
  spell-carry start-row hypothesis; the AI-asymmetry register.

### Banked opt-ins (enhancement/alternate features)
- Torso-aim enhanced aiming (intended eventual enhanced default);
  predictive autoaim closure (no-mutation aim_assist variant).
- Enhanced-flight PHASE 2: the off-desired pitch assist (ruling 3 —
  evaluate after playtesting phase 1). Phase 1 LANDED 2026-07-23:
  turn-rate damper + proportional bank + ground-relative
  desired-altitude law (see DEVIATIONS "flight & controls"); feel
  constants (TURN_GAIN/DECAY/BANK_SCALE) tuned by playtest.
- Exclude creatures from pyramid damage; slot-16 summon-corpse death
  animation; legible map markers; MC2 XMI/AIL no-CD faithful-alternate
  arrangement.

### Refactors (see retrospective §4)
- LANDED 2026-07-19 (goldens unmoved): S1 tick()/spawn_from_thing() arm
  extraction, S2 shared engine → `mgc_sim::engine::{world, features}`,
  S3 live_poses() per-game split, A1 `CampaignSave` enum, A2
  GM-normalize dedupe.
- Deferred: declared per-model dispatch table (only if provably
  order-equivalent); `WizardConfig` enum (with next FORMAT_VERSION
  bump). LATE: game-manual naming reconciliation sweep.

### Later tracks
- FMV/cutscenes — PROMOTED to "next session", see the top of Remaining
  work (recon done: one format, 320×200, decoder already written, the
  outro seam already named). Remainder here: attract mode, PPERF score
  screen, ScrollDialog unroll.
- Feature-family plugin promotion (authenticity-matrix columns → whole
  swappable families) — design agreed, mostly folded into existing seams.
- Flight feel-tuning pass vs remc2; custom level designs (wyvern-kite,
  portal-maze ideas in the archive ledger).
- FIDELITY.md subsystem write-ups ("entries to come" backlog).
