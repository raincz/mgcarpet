# The fidelity record

This is the porting documentation: for every gameplay subsystem, what
the original engine does, what this engine implements, how that was
verified, and where behavior deliberately deviates. It is the durable
companion to [ROADMAP.md](ROADMAP.md) (the working session log): the
roadmap records *how we got here*, this file records *what is true
now*. When a subsystem changes, update its entry in the same change.

The project stance (see the README and the authenticity matrix below):
**the faithful original behavior is always the default and always
available**; every modern improvement is a named, opt-in alternate.

## How to read an entry

Each subsystem entry has five parts:

- **Original** — the retail behavior, with its evidence anchor
  (decompile function/lines, retail observation, recorded gameplay).
  Anchors of the form `sub_NNNNN :LLLLL` refer to the remc1
  decompilation (`sub_main.cpp` line numbers) unless marked remc2.
- **Port** — what we implemented and where it lives
  (`crate::module`).
- **Verified** — the strongest verification the entry has passed, one
  of the grades below.
- **Options** — the authenticity-matrix columns this subsystem
  exposes, with their class (P/G) and defaults. Absent = no options,
  the faithful port is the only behavior.
- **Deviations & interims** — every known divergence from retail:
  deliberate improvements (with their toggle), approximations pending
  a deeper trace, and honest gaps. "None known" is a claim, not a
  hope: anything the next playtest falsifies moves the entry back.

### Verification grades

Ordered weakest to strongest; an entry states the strongest grade it
has actually earned. The senior-source rule: **recorded original
gameplay outranks the decompile** — remc1 is a machine reconstruction
with known transcription errors (truncated tables, mis-fixed lines),
so when retail play contradicts it, retail wins.

1. **decompile-traced** — ported line-by-line from the remc1/remc2
   reconstruction; not yet exercised against the original.
2. **oracle-diffed** — output compared byte-/stream-exactly against
   original-engine output (reference dumps, instrumented DOSBox,
   memory-image regression fixtures).
3. **player-validated** — a targeted in-game check by the player
   confirmed the specific behavior.
4. **player-certified** — the player has played the subsystem at
   length and judges it faithful to retail ("as I remember" or
   better); residual deviations are treated as future spottings.
5. **retail-verified** — the specific behavior was reproduced in the
   original game side-by-side (DOSBox/MC1PLUS), the strongest grade.

### Option classes (the authenticity matrix)

Options are enums, not booleans — room for named alternates. Columns:
`mc1` (the faithful MC1 port, default), `mc2` (an MC2 behavior offered
as a faithful alternate in MC1 contexts), `improved` (a deliberate
modern deviation). Two engine-level classes:

- **P-class** (presentation): resolves at render/input time, never
  changes simulation outcomes. Freely flippable.
- **G-class** (gameplay): changes simulation state or RNG consumption.
  Recorded into replays; a replay taped under a non-faithful G option
  is not a faithful fixture.
- **Patch-class** (`gameplay · patches`, DEVIATIONS.md "Patch
  options"): a deliberate retail-bug fix with BOTH arms implemented
  (`retail` = the shipped bug, `patched` = the fix). Sim-affecting like
  G, but fixture safety is structural — worlds default to the retail
  arms, and `--record`/`--replay`/conformance force them — so patches
  never flag the run; the startup banner counts them apart.

Current option surface: `mgcarpet.json` + CLI flags (the generated
`mgcarpet.json.defaults` documents every option); an in-game menu is
planned.

---

## Terrain generation (MC1)

**Original.** MC1 levels ship no heightmap — each level stores 12
generator parameters (seed, raise, gnarl, river…) and the engine grows
the world at load: a seeded fractal midpoint field, normalized to
16-bit, classified into terrain types (water/lowland/rock/snow bands),
then shaded. The generator is exactly reproducible from the
parameters; its arithmetic wraps in load-bearing ways (the i16
corner-sum wrap), and one retail level (index 039) hits a degenerate
collapse — an all-negative field normalizing to a flat plateau — which
is plausibly why the campaign's hardcoded skip table exists.

**Port.** `mgc_import::mc1_terrain`, a native Rust port of the remc1
generator (heightmap, type classifier, shading, angle planes). Runs at
bake time; the engine never sees a seed (baked packages carry expanded
grids). Entity-driven terrain modification (craters, walls, building
flattening) is deliberately NOT baked: `mgc_sim::features` applies it
at load, as the original engine does after generation.

**Verified.** retail-verified. Heightmaps reproduce the
previously-oracle-validated reference output near-byte-exactly across
all 143 MC1/HW retail levels (1:1 validation pass, 2026-07-04,
player-checked against DOSBox renders); the level-039 degenerate
collapse reproduces exactly (player-confirmed in DOSBox).

**Options.** None — generation is bake-time and single-truth.

**Deviations & interims.**
- `hmap2` (the original's second heightmap, the water-reflection
  plane) is not derived — needed only by a future reflections render
  pass; rebuilt post-load in the original, so nothing is missing from
  the bake.

---

## Player flight (MC1)

**Original.** remc1 `sub_455D0`: mouse steering is a turn *rate*
(stick-like: offset = rate, not position); aim pitch is absolute;
speed is impulse-based — accelerate/decelerate keys add ±16/tick while
held and the speed *holds* on release (no friction stop); thrust acts
in the level ground plane regardless of aim pitch. Vertical motion is
terrain-follow with a soft ceiling: climb authority is full below
ground+768, fades to zero at ground+1024, inverts above — but level
flight *holds* any altitude reached (the wall-climb move: ride a slope
up, fly off level). The camera pitches at HALF the aim pitch. The
human player is class 3 model 0 with an explicit wall gate
(`sub_45410`) — flying monsters cross walls; the player cannot.

**Port.** `mgc_sim::flight`, verbatim: rate stick, absolute aim
pitch, impulse speed, soft-ceiling climb authority, half-pitch camera,
the wall gate in the flyer. Mouse-forward = dive is the authentic
polarity.

**Verified.** player-certified 2026-07-07 ("as frustrating and
useless as I remember"); the wall-climb altitude acceptance test is a
standing regression test.

**Options.** Three orthogonal enums (`flight.*` in config/CLI),
faithful defaults:
- `thrust` — G-class: `mc1` (default) | `enhanced` (hold-to-fly with
  auto-deceleration; keeps the authentic level-plane thrust rule).
- `altitude` — G-class: `faithful` (default) | `extended-lift`
  (adds explicit float keys E/Q, float-up capped at the level's
  highest terrain; wall blocking intact).
- `bindings` — P-class: `classic` (default; mouse aims, arrows
  accelerate/strafe) | `wasd`.
- `mouse_sensitivity`, `invert_y` — P-class preferences (the
  originals shipped an invert option too).

**Deviations & interims.**
- Camera ROLL is unrendered (the original banks slightly in turns).
- An `mc2` normalize-key thrust tier and the player's torso-aim
  design are banked, not implemented.
- Tick/time rate: the original locked simulation ticks to framerate,
  so game speed varied with resolution — there is deliberately NO
  faithful target here; we run a fixed timestep and will pick
  canonical rate constants (likely MC2's) in a dedicated timing pass.

---

## Spell jars — the owned-spell hide (RETAIL, not an option)

**Original.** Both MC1 and MC2 keep every spell jar in the world
forever: the pickup gate (`sub_68FF0` MC2 / the class-12 pickup MC1)
flies you *through* an already-owned jar without collecting it, and
AUTHORED/PLACED jars carry `life = 0` so they never decay. Only
DEATH-scattered jars carry a ttl (life 200-289). **But retail does not
DRAW the ones you own.** The wizard's init claims one entity per carried
spell as its live manifestation and sets the byte[0] hide bit
(MC1 :54907); the record stays pooled and invisible. Measured: mc2l3
t=0 carries 26 class-15 records, one per carried spell, every one at
`flags 0x5`. Corroborated on the retail build twice — owning a spell
hides its jar, and the all-spells cheat makes floor jars vanish (that
second witness is what proves the hide reaches WORLD pickups and not
just the wizard's own claimed records).

**So this is faithful behaviour, and as of 2026-08-25g it is
unconditional.** It shipped for weeks as `gameplay.enhancement.
prune_owned_jars`, a P-class option defaulting ON with
`--no-prune-owned-jars` as its "faithful" arm — an arm that showed
something retail never shows. The player closed it from a replay: after
a death the port drew the whole scattered spell book at the death site
and kept drawing it across the respawn, which retail does not do. The
option, its CLI flags and its settings row are gone.

**Where it lives.** `World::owned_spell_jar`, a PAINTER test in
`live_things`/`live_poses` — never a sim edit. Retail hides per-entity
because a jar is per-player collectible (one wizard owning a spell must
not remove the pickup for anybody else), so a world edit could not
express it even in principle; keeping it in the painter also makes it
allocation-neutral and `state_hash`-free. Strict-retail worlds read the
REAL bit (`flags & 1`) instead, so imported and replayed retail states
never depend on the port's stand-in. The residual deviation is the
encoding only: the port models owned spells outside the pool and never
mints manifestation records, so it keys the test on the local player's
spell set.

**Verified.** `owned_spell_jars_are_never_drawn` (MC1) +
`mc2_owned_spell_tokens_are_never_drawn` (MC2), both asserting BOTH
halves — absent from the painter, PRESENT in the pool — because
asserting only the first would pass equally well for the pre-2026-08-24f
cull. Corpus-side: `level_005_golden_state_hashes` re-pinned D/E with
the STATE hashes byte-identical (2 jars drawn until the all-spells cheat
arms, 0 after).
---

## Vertical projection (crosshair/pitch feel) — APPROX, player-ruled

**Original.** Retail renders pitch as an affine horizon SHEAR: the
eye-level row shifts by `width·pitch/256` (`:33872`/`:38245`), object
elevation is added separately as `fowDist·tan(α)` (`:36853`,
`fowDist = √(W²+H²)/2`), and the camera pitch is HALF the aim pitch
(`:52434`). The shot fires on the full aim, so the half-pitch shear
near-cancels its elevation: at full aim the crosshair sits at ~1/3
(up) / ~2/3 (down) of screen height (player-measured in retail).

**Port.** A true perspective camera pitched at aim/2 (FOV_Y 60°,
`mgc-render`); the crosshair predictor projects the aim ray through
the same camera (`mgc-app`). No shear cancellation, so the aim/pitch
disparity reads ~2.4× stronger (~0.145/0.855 at full aim). The
crosshair stays exactly over where port-rendered shots fly — the
property certified during the autoaim work.

**Verified.** Formula-level comparison against the decompile, both
projections quantified: docs/traces/mc1-crosshair-pitch-law.md.

**Deviations & interims.** The projection model itself is the
deviation. **Player ruling 2026-07-15: keep the perspective renderer
as-is** — retail's shear is the technically-wrong projection; a
crosshair-only correction was rejected (it would desync the predictor
from visible shot paths). The trace holds the full affine law should
a faithful-shear renderer alternate ever be wanted.

---

## MC2 level-7 castle downgrade haircut — APPROX (overflow not reproduced)

**Original.** `sub_605E0` (remc2 EF:61622) computes the 10% capacity
haircut as i32 `10 * maxMana / 100`. At the level-7 cap
(`MC2_CASTLE_CAP[7] = 300_000_000`) the multiply ALWAYS overflows
into a negative cut — a maxed level-7 castle downgrade *raises* its
cap and scatters no mana. Lower levels compute normally.

**Port.** The multiply widens to i64 (`mgc-sim mc2/castle.rs`,
`mc2_castle_downgrade`), so level 7 takes a genuine 10% haircut like
every other level. (The widening originally fixed a player crash —
the shift+L downgrade overflow panic, 2026-07-13.)

**Verified.** Decompile re-read 2026-07-16 (review item G9n).
Terrain restore is unaffected either way — only the mana-scatter
amount differs.

**Deviations & interims.** Deliberate idealization: we keep the sane
10% rather than reproduce an integer-overflow bug whose only effect
is a broken edge (no scatter at the top rung).

---

## MC2 "under attack" owner flags — APPROX (player-only HUD latch)

**Original.** Castle damage sets the OWNER's `byte_0x195_405 = 4`
and balloon damage `byte_0x197_407 = 4` for ANY owner (remc2
EF:61752 / EF:61947) — rival wizards receive the same flags.

**Port.** A single player-side alert latch (`castle_alert` /
`balloon_alert` in `mgc-sim mc2/castle.rs`); rival owners have no
per-owner record, and the rival brain does not consume an
under-attack signal yet.

**Verified.** Decompile-cited (review 2026-07-15 item G9c).

**Deviations & interims.** Becomes real work when a rival defense-AI
consumer lands; until then the flags are HUD-only and hash-excluded.

---

## MC2 doomsday pyramid — APPROX register (module-doc promoted)

**Original.** The (5,10) doomsday machine's full retail script:
sprite-length-derived state timers (`sub_221F0` EF:13661), the
`sub_5C800` case-7 palette beam flash, self-acquiring projectile
bursts, the case-0xE global wipe's `byte[1] |= 0x20` render bit, and
per-list (dword_38531) bucket scans.

**Port.** `mgc-sim mc2/doomsday.rs` — the machine, phases, devour,
terrain flatten and death script are ported; the module doc's
DELIBERATE APPROXIMATIONS register covers the deltas: seeded 16/32
state timers (no TMAPS frame counts in the sim — cadence-only),
palette flashes skipped (presentation), projectile bursts pre-locked
at the avatar (the proj module's acquisition APPROX), the unmapped
0x20 render bit skipped, pool slot-order scans for the list scans.
Also: the `dword_0x364D2` devour tally is banked for the stats
screen, and the hurl-away pose transport is app-side.

**Verified.** Review sessions C (2026-07-15) items C2/C7 +
extinction-script fixture (`mc2_doomsday_pyramid_extinction_script`).

**Deviations & interims.** All of the above are deliberate; none is
hash-visible beyond the seeded timers, which the goldens pin.

---

## MC2 held creatures (StageVar holds) — APPROX (idle reductions)

**Original.** A held creature runs its per-model phase-7 wrapper
around `sub_1D5D0`: ambient-sound draws, speed refresh, idle FACING
choreography (64-tick LCG roll jitter) and the `sub_1B8C0`/`sub_1EEE0`
physics settle.

**Port.** `mgc-sim mc2/stagevars.rs` (module-doc APPROX register):
the hold gate, killability, aggro-break and the kind-3 ambush law are
faithful; the idle EXTRAS are not run — a held creature keeps its
spawn pose and draws no idle RNG. Gates reading `f63 & 7` see the
static spawn ordinal, which matches retail (never increments while
held; verified on the m0/goat/m16/m18 wrappers).

**Verified.** Session H (2026-07-16) — the hold seam traced against
`sub_1D5D0` + four per-model wrappers.

**Deviations & interims.** Idle presentation/RNG only; the ordinal
law (the part that gates behavior) is faithful.

---

## MC2 rival DEFENSE disguise — APPROX (visual unported)

**Original.** The AI DEFENSE state's metamorph disguise draws the
picked creature IN PLACE of the AI carpet (remc2 sub_15FC0/sub_161A0).

**Port.** `mgc-sim mc2/rivals.rs`: the state machine, tier pick,
shadowing and speed law are faithful (reworked per review 2026-07-15
P1-6); the disguise VISUAL is presentation-side unported — the rival
still renders as a carpet while disguised.

**Verified.** Review session E27 (2026-07-16) re-verified the state
law against the decompile.

**Deviations & interims.** Presentation gap, banked with the rivals
polish track; the sim state is faithful, so no golden is affected.

---

## MC2 rival wizard tags (name + health bar) — LANDED, with named gaps

**Original.** `DrawSorcererNameAndHealthBar_2CB30` (remc2
GameRenderHD.cpp:2797-2879, hooked from the sprite pass at :5010-17):
every drawn class-3 model-0/1 sprite wears a boxed name + 2px health
row, gated ONLY on the "Player Names" toggle (default ON,
PlayerInput.cpp:1503) — never on damage, lock or distance. Box =
8·len+6 × 18 px (640 frame), 2px bevel (map-type chrome
`str_D94F0_bldgprmbuffer[MapType][{0,2,3}]`), name at left+4, bar at
top+14 with fill `floor(life·(w−2)/max)`; name ink and fill share the
team color `playersColors_E88E0x[slot][0]` (identity slot map in
single player — NOT the art order), empty bar = palette[0]. The tag
anchors left edge at the sprite's horizontal center, top 20 px above
the sprite top.

**Port.** `ui::rival_tag_quads` + `entities::rival_tag_chrome` +
the lib.rs overlay block (anchored via `world_to_screen` on
`RivalView.{x,alt,z}`), option `render.preference.rival_tags`
(auto* = per-game faithful / on = MC1 opt-in / off = retail MC2's
toggle off). The debug `render.debug.health_bars` stays a separate
instrument.

**Verified.** Retail law traced with full citations (agent sweep
2026-07-26, all three renderer backends identical); chrome indices
pinned by `rival_tag_chrome_resolution`; MC2_TEAM_* column-0 tables
re-checked against the decompile dump.

**Deviations & interims.**
- FONT1 is proportional where retail reserves monospace 8px cells:
  the box hugs the true text width; ink drawn at y+3 scaled into the
  11px interior band (retail's glyph cells carry their own leading).
- The anchor approximates "sprite top" as entity altitude + 0.6
  tiles before the retail 20px lift (we project the entity datum,
  not the rasterized sprite rect) — tune `WIZ_TOP` on playtest.
- The tag rides the smooth-motion sub-tick lerp (presentation
  enhancement, matches the sprite it floats over).
- MC1's opt-in chrome carries Day-row constants pre-sampled from
  PALD-0.DAT (MC1 levels have no MC2 palette to resolve through).
- Retail's multiplayer-only `GetTrueWizardNumber` slot remap is not
  modeled (single-player identity is).

---

## MC2 barrel roll — LANDED, with named gaps

**Original.** Both strafe keys pressed the same frame from neutral
(edge vs the prev-frame strafe byte, PlayerInput.cpp:2080-97) →
command bit 0x80 → `sub_55C60` (EF:38879-969): a seven-phase
spring-settle on the VIEW roll only — no displacement, no i-frames,
no hitbox change (verified negatives; the phase flag has exactly two
readers engine-wide). The move's one mechanic is `sub_55EB0`
(EF:38972-98), fired at phase 1 AND the finish: every entity whose
homing target is the player drops its lock (bucket skip list
{1, 12..=15, 22..=23}). The driver pins the bank stick centered each
tick (`rollDelta = 0`), aborts to the finish on a >16-count mouse-X
grab past phase 4, and holding both keys ALSO strafes right (bit 8
decodes last, EF:60793-96). MC2 only; the MC1 decompile has no input
module to check (absence is the faithful default).

**Port.** `flight::BarrelRoll` (verbatim phase machine) driven from
`Simulation::step` after the move, retail's order; lock-break =
`World::mc2_break_player_locks` (skip list per entity class over the
flat pool); trigger + raw-dx + stick recenter in the app's
`tick_input`/`device_event`; the tumble publishes through
`flyer.roll` (the renderer's camera basis takes any angle). Gated by
the MC2 flight verb — no config option (always-on, like retail; a VR
comfort gate is the VR fork's call). Hash-quiet at rest (TAG 11);
`SNAPSHOT_VERSION` 5 → 6.

**Verified.** Phase-machine unit tests (full tumble, two lock-break
pulses, direction from bank sign, seed-91-clamps-to-68, mouse abort);
lock-break pool test (player locks drop, third-party and skipped-class
locks survive); end-to-end baked-level tests (MC2 rolls and settles,
MC1 refuses); the snapshot acceptance now plays its MC2 fixture INTO
a mid-roll snapshot and holds hash equality 600 ticks after restore.
Goldens unmoved (hash-quiet at rest; no fixture rolls).

**Deviations & interims.**
- The abort window is one 24 Hz tick where retail's was one render
  frame — the 16-count constant is kept, so abort sensitivity scales
  with the frame/tick ratio. Playtest dial if rolls cut short.
- The app recenters the virtual stick on every roll tick (retail
  zeroes its `rollDelta` stick the same way; the visible difference
  is only that our pre-roll stick deflection cannot be preserved —
  retail's couldn't either).
- Enhanced thrust: the derived float bank feeds the phase targets in
  angle units, and the suppressed steering is the mouse-yaw channel —
  the enhancement's analog of the bank stick.
- MC1 keeps the both-strafes neutral cancel (its retail decode order
  is untranscribed); MC2 now decodes right-wins, as retail.
- The camera roll lerp takes the shortest arc (presentation; the
  masked tumble wraps 2047→0 once per revolution).

---

## MC2 worm chain link length — APPROX floor 96 (provenance OPEN)

**Original.** The multipart worm's link spacing derives from the
particle sprite rows' `speed_6` metric — which is ZERO in the
pristine EXE's rows (verified), the value that collapsed retail's
formula to nothing.

**Port.** `mgc-sim mc2/multipart.rs`: zero-length links fall back to
the head ctor's authored 96 (`f56 = 96` floor) so the chain stretches
instead of re-blobbing onto the head (PLAYTEST-11 round 3, "the worm
is a blob").

**Verified.** Fixture asserts a nonzero link and the 89+i sprite
chain; the pristine-EXE zero was re-checked against the original data.

**Deviations & interims.** The true retail spacing source is an OPEN
question banked with the disassembly-authors questions; 96 is the
plausible-and-playable floor until it resolves.

---

## Full-screen movies (FMV) — LANDED, with named gaps

**Original.** One player serves every full-screen movie in both games
(`PlayInfoFmv_107C0` :16159; remc2 `Animation.cpp:41`). It streams a
12-byte-header Bullfrog FLIC one frame at a time into the visible
320x200 surface, breaking at `frameCount - 1` so the file's last frame
(the ring delta back to frame 0) never shows. Pacing does NOT come
from the file: each movie carries a compiled-in event script of
`(startFrame, key, index)` records, and key `'A'` sets the inter-frame
delay in ticks of the shared 120 Hz timer (default 5 = 24 fps,
`dword_9ADC4` :6457). Abort is any key or either mouse button, and is
PER MOVIE — the flag resets at the top of each call — with several
call sites passing `allowSkip = 0`. Palette COLOR chunks install live,
on the frame they arrive; the fades between movies are 16/32-step DAC
ramps run by the CALLER, not the player.

Sequences: MC1/HW dispatch INTEL → LOGO → INTRO → TITLE → main menu
(`sub_4AB20_4AE60` :57879-907), holding the logo 8 s and the title
6 s; MC2 runs a welcome still, INTRO, then INTRO2 (`Intros_76D10`,
MenusAndIntros.cpp:736-800). MC1 plays a congratulation movie after a
won level, picking LEVELW1 or LEVELW2 by the parity of the free-running
timer (:59905), and LEVELOSE after a lost one; the outro fires when the
worlds-completed counter reaches 50 (25 for HW). MC2 slots CUT1-5 after
level indices 4/8/12/16/23 (`cutScene_E16E0`, MenusAndIntros.cpp:189)
and CUT6 — its ending — after 24.

**Port.** `mgc-import fmv.rs` (`FmvCursor`, the incremental decoder),
`mgc-import bundle.rs::bake_movies` (raw streams + `MovieIndex`),
`mgc-app movie.rs` (the player: cue chain, tick-denominated pacing,
per-movie skip, boundary fades, holds, the audio cue stream and the
subtitle strip), `mgc-audio`'s movie-sample lane, and the seams in
`mgc-app main.rs` (`Screen::Movie`, `intro_movies`, `mc2_cutscene`,
`mc1_win_movie`, the `NextStep::Outro` arm). Option:
`render.preference.movies` (Preference, default ON = faithful).

**Verified.** All 24 retail streams decode to their exact header frame
counts through the cursor (`fmv::tests::full_screen_movies_decode`),
and the cursor agrees frame-for-frame with the eager decoder on the
menu movies. The player's frame budget, per-movie skip, unskippable
cues, authored and default pacing, post-movie holds and the intro's
full soundtrack (bank order, 51 sample cues, the four music cues
including the looped middle section, all 17 subtitle lines) are pinned
by `mgc-app movie::tests`. The transcription has an independent
cross-check: `every_cued_sample_exists` resolves every sample index in
every script against the baked sound banks, and the banks corroborate
the reading — MC1's intro loads exactly the banks holding
`voc1`..`voc12`, one clip per narration cue, and MC2 banks 5-9 are
`viscut1`..`viscut5`, one per cutscene. PLAYTEST OWED — none of this
has been seen or HEARD running by a player yet.

**Deviations & interims.**
- **Three player-ruled corrections** (all in docs/DEVIATIONS.md, all
  first-playtest findings): playback runs 25% slower than the authored
  delays, because at the authored rate a narration clip is cut off by
  the next scene's bank load — retail clips it too, but it reads as a
  defect; the script fires one frame early, so long scene holds park on
  the settled page instead of a frame or two into the next flip; and
  skipping a movie stops its music, which retail leaves running.
- **Subtitles are gated on a setting, not on language + sound.** Retail
  turns the strip on for every non-English build, and for an English one
  only when there is no digital-sound device (remc1 `sub_357C0_35B80`;
  remc2 MenusAndIntros.cpp:756-765) — the narration is recorded in
  English only, so subtitles are its stand-in, never a preference. This
  port ships English audio, so the faithful state here is OFF and that
  is the default; `render.preference.movie_subtitles` forces the strip
  open for anyone who wants the text. MC2's CUT6 suppression is
  unconditional in retail and is not modelled — forcing subtitles on
  subtitles CUT6 too.
- **The subtitle fonts and pens are per-game, both retail-exact.**
  MC1: SFONT1, left pen at x=10, advance `tabRecord[4] - 1` (the
  glyphs kern by a pixel; advancing by the full width ran MC1's
  longest narration line to x=363 on a 320px screen, which is what a
  playtest saw), authored CRLF line breaks. MC2: NOT SFONT1 — retail
  loads a dedicated 7×8 monospace font out of HSCREEN0.DAT before
  every movie (`Intros_76D10` / `PlayInGameFmv_82670`; lowercase
  records repeat the capitals, so captions render all-caps) and lays
  it out in 640-space halved by the low-res blitter: fixed 7-px cell
  for every character, greedy 42-cell word wrap, centring by
  `315 - 7·strlen` (counting each wrapped line's trailing space),
  8-px line stacking from screen row 170 — up to four lines, where
  SFONT1's 14-px glyphs fit two (the shipped bug: long captions lost
  their tails). The wrap walk and its off-by-ones are ported
  index-for-index and pinned against a retail screen capture
  (`movie::tests::mc2_wrap_matches_retail_capture` — same three
  lines, same word splits, same starting columns). The ink is flat
  nearest-white with no outline (`DrawColourizedBitmap` repaints
  every glyph pixel), also retail. The picture lift is exact for both
  (MC1 21 rows, MC2 31). But MC1's strip runs from buffer row 180,
  which is 20 rows ABOVE the band the lift clears — retail draws those
  rows over live picture, and the frame decoder can repaint them
  between subtitle changes. We draw the text last, so ours always
  survives. Whether retail's flickers, and how badly, was not
  determined.
- **Sample volume/fade operands are flattened.** MC2's `'H'` key starts
  a looping sample at volume 0 and a paired `'O'`/`'P'` then raises it
  to 127 or 80; we start the loop at full instead, so those two
  ambiences arrive without their fade-in and one plays louder than
  retail. MC1 has no volume operands at all, so MC1 is unaffected.
- **Two MC1 scripts are TRUNCATED in the decompile** and only their
  leading delay record survives: `LEVELOSE` (up to 4 records lost) and
  `INTEL` (up to 2). `LEVELOSE`'s bank load and cue are RECONSTRUCTED
  from the sample banks rather than transcribed — bank 8 holds exactly
  one sample, `failed` — and its cue frame is a guess. Neither movie is
  reachable in this engine anyway.
- **`LEVELW2` is scored better than retail.** Retail points both win
  movies at one script (`dword_4A5D8_4A918`), so `levelw2` plays with
  bank 6's `win1` at frame 200. An unreferenced table at 0x4A5FC is
  byte-identical but for bank 7 (`win2`) and frame 180 — plainly
  `levelw2`'s own script, orphaned by the shared pointer. We give each
  movie its own table, which is a **deliberate deviation**: it fixes a
  retail bug rather than reproducing it.
- **INTEL.DAT is never played.** It is the Intel Pentium branding
  bumper, gated on CPUID family 5 model 1 (`sub_19470` :19475) — no
  machine running this port qualifies. MC2 ships the file and never
  references it at all. Baked, unused.
- **LEVELOSE.DAT has no seam.** A failed MC1 level does not route back
  through a post-level screen in this engine, so the world-lost movie
  never fires. The stream is baked and the call site is documented at
  `mc1_win_movie`.
- **MC1's title overlay (TITLE-02/04) is unported** — retail composites
  a 4-frame loop over the held title screen via a different stepper
  (`sub_4F120_4F460` :60245); ours holds the title static.
- **MC2's welcome still screen** (HSCREEN0 at 0x178E5F, fade in, hold
  2 s, fade out) ahead of its intro is unported.
- **The MC1 attract mode is unported**: after 40 s idle on the main
  menu retail rotates INTRO, TITLE and a recorded input demo
  (`MOVIE/MVI%05d.DAT` — an input recording, not a movie). The demo
  format is unimplemented.
- **The delay is reset per movie, not carried across.** Retail's
  `dword_9ADC4` is a process-global that only the `'A'` key writes, so
  a movie with no delay record inherits the previous movie's rate.
  Every transcribed table opens with an `'A'` record, so the two are
  equivalent and ours cannot drift.
- **Frame pacing is wall-clock, not a busy-wait on the game timer.**
  Retail spins on the shared 120 Hz counter and zeroes it after each
  frame; we accumulate delta time and drop frames under stall rather
  than catching up. Same rate, no lost seconds on a slow host.
- **Centred letterboxing** on non-4:3 windows is ours; retail had one
  320x200 mode and no such case.

---

*Entries to come (the full subsystem list, in rough dependency
order): terrain features & villages; triggers, events & portals;
monsters (per-model AI); combat & damage channels; projectiles &
autoaim; the 24-spell repertoire (per-spell); mana economy & castles;
player mortality & the castle weapon; rival wizards; map & HUD;
audio; campaign progression & the skip table.*
