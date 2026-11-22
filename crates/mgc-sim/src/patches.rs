//! Retail-bug patch switches — the sim half of the `gameplay ·
//! patches` option class (docs/DEVIATIONS.md "Patch options").
//!
//! Each field is one deliberate upstream bugfix with BOTH arms
//! implemented: `true` runs the patched (fixed) behavior, `false`
//! runs retail's shipped bug. The struct is config-like — never part
//! of the state hash or the snapshot stream — and defaults to
//! [`WorldPatches::RETAIL`] at world construction, so every direct
//! `World::new*` consumer (goldens, unit tests, mgc-conform, which
//! never reads app config) evolves under retail law unless the app
//! explicitly opts a patch in. Conformance imports additionally
//! re-force RETAIL as a belt (`World::strict_retail` remains the
//! overriding kill-switch at the gated sites).
//!
//! Reach: `World` methods read `self.patches`; Gen-side ticks get it
//! through [`crate::mc1::mobs::MobCtx::patches`] where a ctx already
//! flows, or as an explicit parameter on the castle/building lanes
//! (the `strict` precedent — a Gen field would drag the wholesale
//! `#[derive(Hash)]` and the snapshot codec along).

/// Per-patch switches; `true` = the patched (bug-fixed) arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorldPatches {
    /// Class-12 jars re-snap to their tile's ground every tick.
    /// Retail's reshape walk skips class 12 (:51729): terrain shaped
    /// over/under a jar leaves it buried (HW ships several) or
    /// hovering.
    pub jar_ground_snap: bool,
    /// A settled (f58 == 0) MC1 mana ball tracks the ground both
    /// directions. Retail freezes it wherever it is — mid-hop balls
    /// hang in the air, terrain edits bury grounded ones.
    pub ball_ground_track: bool,
    /// Mana balls run their roll physics map-wide, BOTH games. Retail
    /// re-arms a settled ball's +58 only within the 24-tile awake
    /// radius of the human (MC1 :64352-61; MC2 `sub_68C70`'s sphere
    /// leg, EF:55526), so approaching a downhill ball wakes it and it
    /// visibly "runs away". Balls only — the creature awake gate is
    /// untouched. (MC1-only until 2026-09-06; the MC2 arm was
    /// player-requested alongside `ball_owner_recolor`, which stays a
    /// separate option: a settled ball that never wakes still wants
    /// its owner's colour.)
    pub map_wide_ball_rolling: bool,
    /// A possess that lands on an MC2 mana worm (5,22) takes effect
    /// at once, map-wide — the SIM half of the app's
    /// `ball_owner_recolor` option (the draw half is
    /// `LivePose::owner_type_index`). Retail's ch1 tag intake is
    /// awake-gated (`byte_0x39_57`) on both doors — the head's own
    /// (`sub_26F10`, EF:17542; `Gen::m22_dmg`) and the segment relay
    /// (`sub_26D20`, EF:17447; `Gen::m22_relay`) — so
    /// a tag that lands outside the 24-tile awake radius PARKS in the
    /// mailbox and the worm only changes owner, runs its recolour
    /// sweep and turns for the castle when the human walks into wake
    /// range. Retail witness `recordings/mc2l18.mgcr` worm 596: tag
    /// 529 parked at t=9333, 34 tiles out, and consumed at t=9466 the
    /// tick the human crosses 24 tiles ("the worm only changes colour
    /// when approached", player-reported 2026-09-30). Patched: the tag
    /// is read asleep or awake. The DAMAGE intake stays awake-gated
    /// (the `mc1_fix_dragon_tail` precedent).
    pub mc2_worm_possess_map_wide: bool,
    /// A possessed dwelling keeps its footprint extents under the
    /// owner-flag sprite. Retail's sprite stamp (:30808) clobbers
    /// +78..+84 with the tiny flag extent, collapsing villager-emit /
    /// defender spawns onto the roof — a walled-in corpse-flame loop
    /// that destroys the possessed house from the inside.
    pub possessed_footprint: bool,
    /// MC2 downgrade's 10% capacity haircut computed in i64. Retail's
    /// i32 `10 * x / 100` overflows at the level-7 rung (10 × 300M)
    /// into a NEGATIVE cut — a maxed castle downgrade *raises* its
    /// cap and scatters nothing.
    pub mc2_downgrade_overflow: bool,
    /// MC1 Create Castle placement validation. Retail (the "latch
    /// bug", certified on mc1l32-castle-bug.mgcr): the castle ball
    /// spawns at the HAND muzzle (sub_55EF0 — ±256 units at yaw∓512,
    /// steerable by aim), the launch-tick scan samples THAT tile, and
    /// the landing condition short-circuits (`ground > z || life < 0
    /// || !scan`, :63588-90) — a terrain touchdown builds the castle
    /// with no placement check at all; the scan only re-runs on
    /// airborne ticks, where a failure stops the ball early
    /// (flip 180° + one step back) and still builds. Combined with
    /// sub_12F70's NW-only 8×8 window this lets a wall-corner cast
    /// raise a castle inside a no-castle maze and carve its protected
    /// walls. Patched: the ball spawns at the carpet (the scan
    /// samples where you are) and the landing always re-scans
    /// (failure displaces the site one step back).
    pub castle_latch_bug: bool,
    /// **THE HUMAN'S SPELLBOOK IS PERMANENT** (player-ruled
    /// 2026-09-07, an unfaithful patch, DEFAULT ON, both games).
    /// Retail can lose a spell the wizard already holds, and the
    /// design never earns it back: MC1 has no legitimate way to lose
    /// a spell, MC2 exactly one — the undead wraith's steal
    /// (`mc2_spell_steal`), which drops the jar for re-collection.
    /// Everything else is the abandoned death-scatter mechanism and
    /// its fallout, witnessed on mc1l49: ⑴ the respawn re-mint is
    /// one-shot and a starved mint zeroes the bank entry (:54917 /
    /// EF:59402); ⑵ WITHOUT a death, a stale entity handle kills the
    /// owned token — mc1l49 t=17809: the volcano's plume register
    /// still named slot 235, which the last respawn had re-minted as
    /// the wall-of-fire token, so the next eruption soft-killed it
    /// (`combat.rs` volcano arm, retail law), the exhausted pool
    /// recycled the slot into one of the wizard's own fires a tick
    /// later, the blind list read registered a second fireball, and
    /// with the 24-entry list FULL of stale slots every wall-of-fire
    /// jar the rivals dropped for the rest of the level was refused
    /// (:64841) — "a map littered with jars I could not pick up".
    ///
    /// Patched: the death scatter throws COSMETIC decaying jars and
    /// leaves the tokens in the book (so nothing is re-minted on
    /// respawn either); a soft-kill of a live human token is refused
    /// at `World::free_slot` (the tick-top reap and the MC2 in-loop
    /// free) and the token is scrubbed off the sacrifice stack; a
    /// stale MC1 list entry that no longer names a class-12 record
    /// is cleared by the per-tick rebuild so a re-dropped jar can be
    /// picked up. The wraith steal is untouched — it is the one
    /// explicit, designed inventory writer.
    pub no_spell_loss: bool,
    /// MC1 body segments (the dragon's 16-piece tail, the sea
    /// serpent's, the two-piece m6) keep their rigid follow while
    /// ASLEEP. Retail's `sub_19550` (:21107) only walks an awake
    /// segment to `+56` behind its leader; an asleep one collapses
    /// onto the leader every 4th tick — a period CPU save behind the
    /// 24-tile awake radius that reads as "the tail lags in strange
    /// ways" once the port's fog shows distant dragons. This is the
    /// only reason anyone raised `awake_range`, which also wakes the
    /// whole ecology (player-ruled 2026-09-07, DEFAULT ON). The
    /// damage intake stays awake-gated.
    pub mc1_fix_dragon_tail: bool,
    /// **THE MC1 PACK-SPEED RATCHET** (player-reported 2026-10-10:
    /// vultures "at mach speed" late in a level, worms shooting off
    /// cliffs on map 48 under `awake_range 0`). The pack catch-up
    /// (`sub_1A390` :21814) writes a follower's `+126` = the LEADER's
    /// `+126` + the leader's `+130`, and retail's leader-lost default
    /// (:21793) drops the follower to WANDER with its link cleared and
    /// its speed UNTOUCHED — so an orphaned follower wanders one accel
    /// step hot, the next pack scan makes it a leader, and its followers
    /// take that plus another step. Every generation adds `+130`; only
    /// the models with a retail chase-exit trailer (m2/m4/m7/m9/m15)
    /// ever restore. Measured headless on map 48 (65 wyverns, parked
    /// ghost human, always-awake): 60 → 640 in 35k ticks, in exact
    /// steps of 60, every write from the pack handler; at the retail
    /// 24-tile gate the same flock still climbed 60 → 120 asleep.
    /// Retail's own listing has no restore, so this is retail's
    /// behaviour taken to the extreme the port's long sessions reach.
    /// Patched: a follower leaving the pack state by ANY path (leader
    /// lost, leader joined a chase, promoted by a hit) restores its own
    /// `+128` first; the catch-up itself is untouched, so a pack still
    /// flies one step faster than its leader while it is a pack.
    pub mc1_pack_speed_restore: bool,
    /// **MC2's PHANTOM CASTLE AT THE MAP ORIGIN** (player-reported
    /// 2026-09-10, DEFAULT ON). Retail's building-completion tail
    /// `sub_377A0` castle-re-paints EVERY class-3 record overlapping
    /// the finished plot, model unchecked, minting a (10,42) painter
    /// at that record's spare axis `@0x9A` with its `@0x10` word as
    /// the BUILD00 row. On a castle those are the site and the level;
    /// on a dead rival wizard they are a never-scouted (0,0,0) and the
    /// respawn countdown — a corpse lying on a village re-raises
    /// castle rows at the origin, at sea level, every time a hut
    /// finishes under it (mc2l22 t=20868; on demand on mc2l1: kill
    /// Nyphur over the mana-magnet basins, destroy a dwelling). Both
    /// games' retail shows some of it (the outline for countdown bytes
    /// 7..76). Patched: only castles (3,2) are re-painted.
    pub mc2_phantom_castle: bool,
    /// **DUAL WIELD — EVERY CAST LEAVES THE HAND IT WAS CAST WITH**
    /// (player-requested 2026-09-15, an unfaithful patch, DEFAULT ON,
    /// both games). Retail keeps ONE firing-hand register per wizard
    /// (flags +16 & 0x300: `byte[1] &= 0xFC; dword |= 0x100/0x200`,
    /// MC1 :55894-95 / MC2 `sub_5F7B0` EF:60973-82 — a clear-then-OR,
    /// last successful cast wins) and the muzzle placer reads it off
    /// the CASTER at EMIT time (`sub_55EF0` :64978- / `sub_68E50`
    /// EF:55595), not at arm. Two hands casting at once therefore
    /// share one muzzle: with both buttons held the right hand's
    /// stamp lands last, and everything — the left hand's stream
    /// included — leaves the right muzzle; a burst can even flip
    /// mid-way when the other hand casts. Patched: every arm also
    /// records its hand on the TOKEN it armed (`World::token_hand`,
    /// a port-only side table keyed by token slot) and the token's
    /// fires read that instead of the shared register. The register
    /// itself is still stamped exactly as retail (the conformance
    /// column and the retail arm read it unchanged). Casting the
    /// SAME spell from both hands is a different mechanism (one
    /// token per spell id, not per hand) and is NOT changed here.
    pub dual_wield_muzzle: bool,
    /// **ONE CASTLE PER WIZARD** (player-reported 2026-09-15, an
    /// unfaithful patch, DEFAULT ON, all three games). Retail's
    /// "do you already own a castle" test is a REGISTER read, and
    /// the register is written ONE TICK LATE — MC2's create guard
    /// (EF:58831) reads the owner's `CastleEntityIndex_0x3A_58`,
    /// which only lands in the castle's own level-up commit
    /// (`sub_60480` EF:61896, beside `level++` / `actionIndex = 5`),
    /// a tick after the ball minted the (3,2) record. Two castle
    /// balls in the air whose landings fall one tick apart — the
    /// rapid-fire-while-turning cast, far enough apart that the
    /// spatial site test does not refuse the second — therefore BOTH
    /// build: the second reads a zero register, passes the guard,
    /// and the register then latches the NEWER castle, orphaning the
    /// older one ALIVE. An orphaned castle is fully functional and
    /// belongs to no player brain: its balloons never resolve a home,
    /// their health cycles, no mana is banked, and when the
    /// registered castle dies the owner is left pointing at nothing
    /// while the orphan stands. Witnessed on `recordings/mc2l12.mgcr`
    /// — seven split births from t=18274, castle 678 orphaned for
    /// hundreds of ticks (`castle_split_probe_mc2`).
    ///
    /// MC1 is looser still and needs no timing race at all: the plain
    /// create-ball arm (:63588-) carries NO owner test whatsoever,
    /// and the homing/delivery arm's test (:63500-04) demands
    /// `f26 > 0`, so a castle that has landed but not yet transformed
    /// does not count as owned. Hidden Worlds shares that arm.
    ///
    /// Patched: every castle-ball landing that would CREATE resolves
    /// the owner's castle by POOL SCAN — any live, un-reaped (3,2)
    /// stamped with that owner, at any level — and despawns the ball
    /// instead of building a second one. The scan sees the record the
    /// tick it is born, so the one-tick window closes; the level test
    /// is dropped, so the not-yet-transformed window closes with it.
    /// Retail's refusal shape is kept exactly (the ball despawns; no
    /// refund, no re-aim), and the create-vs-upgrade decision at CAST
    /// time is untouched.
    ///
    /// ⚠ The retail arm needs the register the port never modelled,
    /// so `false` additionally drives [`crate::engine::world::Gen`]'s
    /// `castle_reg` from the MC2 level-up commit and teardown — the
    /// ONLY MC2 consumer of that array. Every other MC2 "which castle
    /// is mine" question stays on its existing pool scan: retail
    /// resolves ~21 brain sites through the register, and modelling
    /// those would be building the split's whole blast radius
    /// faithfully for a behaviour we ship disabled (player ruling,
    /// 2026-09-15).
    pub one_castle_per_wizard: bool,
    /// An Alliance-charmed wyvern whose attack state loses its lock
    /// drops to idle like every other species (MC2). Retail's wyvern
    /// attack state `sub_24510` (EF:15451) is the ONLY caller of the
    /// lock resolver `sub_1ED30` with no null arm (`NETHERW.EXE`
    /// 0x48e0a `jbe` → epilogue): once the charm clock lapses
    /// mid-attack — or the charm lands mid-attack and zeroes the lock
    /// (`sub_3A650`, 0x5ef8f) — the wyvern flies its last heading
    /// until it dies, never shooting, re-targeting or waking
    /// (`recordings/mc2l17.mgcr`, seven witnesses).
    pub mc2_wyvern_alliance_brain: bool,
    /// A dead MC2 mana balloon whose castle is gone dissolves into
    /// its mana sphere on its own dispatch, instead of haunting the
    /// pool for the rest of the level. Retail's ONLY reaper for a
    /// (3,3) balloon is `sub_5FF50` (EF:61740, `NETHERW.EXE`
    /// 0x84750), the castle's own fleet pass — `if (v3x->life_0x8 >=
    /// 0) break; TransformEntityToManaSphere_36BA0(v3x, false);
    /// DisableEntityDrawing04_57F10(v3x); array_0x3C_60[v1] = 0;` —
    /// so when the owner's castle falls first, nothing is left to run
    /// it. The balloon's own handler `AddBallon_60AB0` (EF:62160, EXE
    /// 0x852B0) carries NO life test and its damage tail `sub_60EA0`
    /// (EF:62330, EXE 0x856A0) opens `if (life < 0) return;`, so the
    /// corpse is immortal, undamageable and still dispatched every
    /// tick: it flies at whatever record now occupies its home
    /// castle's recycled pool slot (`word_0x96_150` is read by index
    /// with no identity test, EF:62180) and the owner's register
    /// `array_0x3C_60` keeps naming it forever
    /// (`recordings/mc2l22-new.mgcr`: Prish's balloon 668, killed by
    /// the human at t=4248, still there at t=59100).
    pub mc2_orphan_balloon_reap: bool,
    /// A claimed MC2 dwelling flies its owner's CASTLE colour band.
    /// The shipped EXE's claim intake (`AddHouse0A_2D_38330`,
    /// `NETHERW.EXE` 0x38484 / 0x384DA `add di,[eax+0x38]`) adds the
    /// owner's RAW player index to the flag row, while the castle's
    /// latch (0x5FAE7) runs `TransformPlayerColorIndex_616D0` — so for
    /// players 2/4/6/7 a rival's houses and its castle fly different
    /// colours (a known MC2 bug; remc2 patches it in its decompile,
    /// EF:28063/28074). Patched: `177 + COLOR_ART[team]`, the castle's
    /// band. Retail (conformance): `177 + team`.
    pub mc2_house_flag_color: bool,
    /// **THE IMMEDIATE FREE (MC2)** — player-ruled 2026-09-18 (round
    /// 149), the explicit opt-out of retail's tick-top reap. Retail's
    /// `UpdateEntities_57730` (banner `00057730`, shipped `NETHERW.EXE`
    /// file 0x7BF30) opens every frame with an ASCENDING sweep that
    /// frees each `byte[1] & 4` record BEFORE a single handler runs:
    /// ```text
    ///   7bf4e  mov    0x1a3e8,%ebx             ; Entities_EA3E4[1]
    ///   7bf56  cmpb   $0x0,0x3f(%ebx)          ; class != 0 ?
    ///   7bf5c  testb  $0x4,0xd(%ebx)           ; byte[1] & 4  (port flags & 0x400)
    ///   7bf63  call   0x7c720                  ; sub_57F20 — FREE
    ///   7bf6b  add    $0xa8,%ebx               ; stride 168 — ASCENDING
    ///   7bf77  jb     0x7bf56
    /// ```
    /// so a record that dies at walk slot `n` stays occupied for the
    /// rest of its own tick and its slot cannot be popped by a spawner
    /// at a later slot — but ANY stale index that still names it
    /// resolves to a live record for one more frame, and the slot is
    /// then re-popped by whatever spawns first at the next top
    /// (retail's own stale-index hazard; the doom-summon husk and the
    /// mc2l22 orphan balloon both ride it). Retail (conformance, every
    /// graded lane, `init-check`'s witnesses on mc2l13/l16/l24): the
    /// tick-top sweep. Patched (native default): the port's original
    /// MC2 arm — a reap-flagged record is freed at the end of its OWN
    /// dispatch iteration, so its slot returns within the tick and no
    /// later handler can dereference a corpse. MC1 runs retail's
    /// sweep in both arms (it never had the in-walk free).
    pub mc2_immediate_reap: bool,
    /// **THE STALE RECYCLE VICTIM (MC1)** — player-ruled 2026-09-18.
    /// When the free stack runs dry, MC1's allocator `NewEvent_372C0`
    /// (:43885-908) sacrifices entries of the recycle stack — a list
    /// of pool SLOT NUMBERS armed at the last death landing
    /// (`rebuild_recycle(0x20400)`: every record then carrying the
    /// sacrificable bit or the reap flag) and never purged when one
    /// of those slots is freed and re-minted. The seizure re-checks
    /// nothing, so it eats WHATEVER lives in the slot now.
    /// `recordings/mc1l26.mgcr` t=27343: the pool hits 0 free mid
    /// spell-storm, 128 victims go in one tick, and slot 990 — a smoke
    /// puff when the list was built, by then Mahmoud's castle
    /// ground-leveler (10,41) one tick from its finish — is handed to
    /// a (9,9) spawner. The leveler's finish is the ONLY write that
    /// returns a castle from TRANSFORM (`+70`=5, `+48`=6) to SETTLED,
    /// so the castle parks in TRANSFORM for the rest of the level:
    /// its damage mail is read by the settled tick alone (it banked
    /// 915k of the player's damage unread), it cannot upgrade, its
    /// dead balloon is never reaped (that is the settled tick's fleet
    /// pass) and it keeps respawning its owner. Retail (conformance):
    /// the bare seizure. Patched (default): a popped victim whose
    /// CURRENT flags no longer carry the mask is skipped, as if it had
    /// been purged when its old record died. MC2 purges on free
    /// (`sub_57F20`), so the arm is a no-op there.
    pub mc1_recycle_victim_revalidate: bool,
    /// **THE ORPHANED CASTLE TRANSFORM (MC1)** — player-ruled
    /// 2026-09-18, the belt to `mc1_recycle_victim_revalidate`'s
    /// braces: the sanity check that no castle sits in TRANSFORM with
    /// nobody working on it. A castle in a pure wait sub-state (`+48`
    /// 1/4: waiting on its (10,42) painter; 6: on its (10,41)
    /// leveler) whose worker no longer exists at its site takes the
    /// same exit a blast-shake already gives the leveler (:30333's
    /// else arm): sub-state 2, the finish, consumed into SETTLED by
    /// the same dispatch. In normal play a worker ALWAYS stands at the site while
    /// the castle waits — the painter/leveler are spawned in the
    /// same dispatch that enters the wait, finish by writing the next
    /// sub-state before they reap-flag themselves, and the reap runs
    /// at the next tick's top — so the arm fires only when a worker
    /// was destroyed from outside (the stale-victim seizure, or any
    /// future hazard of that shape); it never shortens a healthy
    /// transformation. Measured: `Gen::castle_watchdog_fired` counts
    /// the predicate in BOTH arms, and the MC1 corpus sweep shows it
    /// only on mc1l26 from t=27344.
    ///
    /// ⚠ THAT SWEEP IS STALE AS OF ROUND 163 (2026-09-23) — the
    /// predicate now fires on THREE takes, and the two new ones are
    /// Hidden Worlds: `mc1hwl9-lightningbug` (21,972 orphaned-wait
    /// ticks from t=16575, 5,419 recycle victims seized) and
    /// `mc1hwl14` (35,731 ticks from t=10724, 3,089 seized).
    ///
    /// ⭐ AND THE NEW WITNESSES NAME A SECOND HAZARD. mc1l26's worker
    /// was destroyed by a volcano; `mc1hwl9-lightningbug` was
    /// captured by the player specifically to witness a rival castle
    /// that *"got destroyed and rebuilt in a rapid sequence, probably
    /// destroying it mid building because of the damage that lightning
    /// storm does, and it made it stay at level 1 forever,
    /// indestructible"* — i.e. the LIGHTNING STORM is the outside
    /// agent that killed the (10,42) painter / (10,41) leveler. The
    /// doc's own "or any future hazard of that shape" is now
    /// witnessed, on a different hazard, in a different game build.
    /// That is the strongest available evidence that this arm is
    /// scoped to the MECHANISM (a worker destroyed from outside) and
    /// not to the volcano, which is what the player's 2026-09-18
    /// ruling assumed and nothing had yet tested.
    pub mc1_castle_transform_watchdog: bool,
    /// **THE CRUSHED CONSTRUCTION SITE (MC1)** — player-reported
    /// 2026-09-19 (`bug.mgcr`, mc1:34). A castle's founding and every
    /// upgrade run the pre-clear `sub_12C50` (:17616, `CARPET.EXE`
    /// file 0x2B4DF `movl $0xffffffff,0xc(%ebx)`), which stamps life
    /// -1 on EVERY house of the +36470 chain inside the next-level box
    /// — including a (10,45) still under construction (state 51). A
    /// finished house collapses on that; the construction tick
    /// `sub_27D30` (:29993) only tests `--life == 0` (file 0x405BC
    /// `test ecx,ecx; je`) and divides each footprint cell's goal step
    /// by that life, so the site never finishes (no flag, cannot be
    /// possessed) and pushes its footprint AWAY from the goal forever,
    /// byte-wrapping: towers of 255, pits of 0 — the "extremely tall
    /// building with extremely deep holes". Retail witness:
    /// `recordings/mc1l13.mgcr` slot 140 (t=520 → end). Retail
    /// (conformance): the endless reversed flatten. Patched: a site
    /// whose life is already <= 0 when its tick opens collapses like a
    /// crushed finished house (state 53, `sub_28FE0`).
    pub mc1_crushed_site_collapse: bool,
    /// **THE OVERFLOWING BUILDING PAD (MC2)** — round 157 (w157f),
    /// the MC2 half of the player's "similar weirdness" charge. Both
    /// BUILD00 stampers lerp each footprint cell toward an ABSOLUTE
    /// goal `pad + datum` (the row's pad byte plus the site's own
    /// `position.z >> 5`) and store the result as a BYTE with no clamp:
    /// the building's construction tick `ApplyTerrainModification_37240`
    /// (EF:27365; `NETHERW.EXE` VA 0x373E1 `idivl 0x8(%ebx)`, byte store 0x373ED
    /// `mov %al,0x4b4e0(%ecx)`) and the castle painter
    /// `AddTerrainMod0A_2A_37BC0` (EF:27863/27891; VA 0x37F90 `idivl
    /// 0x10(%ebx)`, byte store 0x37FA9). A building sited
    /// high enough that `pad + datum > 255` wraps: its tallest cells
    /// finish at `goal & 0xFF` — pits of 0..30 in the middle of a
    /// 150-250 plateau — and the finished building's own z re-reads
    /// the ground under its centre (`getTerrainAlt`, EF:27324), so the
    /// model sinks into the pit. Retail witness: `recordings/mc2l22.mgcr`
    /// slots 1/7/12 (the authored ridge town, t=0..21) — slot 7 (row
    /// 43, datum 157, pad up to 127) ends with a 28-deep pit ringed by
    /// 241s and four 0-deep gates, z 157*32 → 896. Retail
    /// (conformance): the byte wrap. Patched: the goal saturates at
    /// 0..=255, so the pad tops out flat at the height ceiling.
    pub mc2_building_pad_saturate: bool,
    /// **THE OVERFLOWING BUILDING PAD (MC1)** — round 157 (w157g), the
    /// MC1 twin of `mc2_building_pad_saturate`. MC1's three BUILD
    /// stampers also lerp each cell toward an ABSOLUTE `datum + pad`
    /// (`datum = z >> 5`, pad `0/12/16/4*(lo-1)`, at most 56) and store
    /// a byte with no clamp: the construction tick `sub_27D30` (:29993;
    /// `CARPET.EXE` file 0x40650-0x40673 `movswl 0x20(%esp)` datum −
    /// `movzbl` height, `idivl 0xc(%edi)`, `add`, byte store `mov
    /// %al,0x4c1e0(%ecx)`; twins at 0x4070C/0x40753/0x4079D), and the
    /// castle painter `sub_285C0` (:30445; goal fill file 0x410F9-0x41117
    /// `datum + 4*(lo-1) − height` as i16, apply 0x41290-0x412B1 `idiv
    /// +26`, byte `add`/store). The leveler `sub_28200` (:30284) that
    /// follows EVERY painter translates the whole rect with an 8-bit
    /// `add %cl,%ch` (file 0x40CBF/0x40D7B) — modular — and clamps only
    /// its TARGET to 220 (file 0x40B40 `cmp $0xdc`). Retail witness
    /// `recordings/mc1l32-new.mgcr`: castle painter slot 39 (level 1,
    /// datum 232) wraps (239,218) (pad 40, goal 272) 254 → 0 at t=6331
    /// and ends its paint at 16; the leveler re-minted on slot 39 at
    /// t=6341 (`+48` current 232, `+44` target 173) then walks it 16 →
    /// 11 → 5 → 255 → … → 213 = `272 − 59`, and (238,218) (pad 48) to
    /// 221 = `280 − 59`. Painter slot 9
    /// (datum 241, goal 289) does the same at t=8036. So a castle's wrap
    /// is a TRANSIENT pit that heals when the leveler target is <= 207
    /// and PERSISTS when it is 208..=220; a dwelling (no leveler; the
    /// finish re-reads its z off the ground under its centre) keeps it.
    /// Patched: the dwelling's goal saturates (the shared
    /// [`crate::engine::features::building_pad_goal`]); the castle
    /// painter's datum and the leveler's current AND target are capped
    /// at `255 − (tallest pad of rows 1..=level)` (207 for every shipped
    /// level), so the painted shape never wraps and — whenever retail's
    /// leveler would have healed it — the castle settles on exactly
    /// retail's final heights. A plain goal clamp on the painter would
    /// NOT: the modular leveler then lowers the clamped 255 by the full
    /// step (mc1l32-new (238,218): 255 − 59 = 196 where retail ends at 221).
    pub mc1_building_pad_saturate: bool,
    /// **THE KRAKEN'S PHANTOM MANA BALL (MC1)** — player-reported
    /// 2026-09-19 (`recordings/mc1l42-new.mgcr`: "a mana ball the map
    /// draws with a purple dot, that cannot be possessed and stays
    /// forever"). The death handler `sub_1A6C0` (:21792; `CARPET.EXE`
    /// file 0x32EB8, loop body 0x32EFA-0x32F26: `call sub_424F0` =
    /// `mov %al,0x46(%edx)`, then `mov 0x36(%ebx),%bx`) walks the
    /// head's `+54` segment chain stamping `+70 = base + 5` with no
    /// class, id or life test, and nothing clears a head's `+54` when
    /// a segment's corpse drops its ball and is reaped. Witness: kraken
    /// head 114 dies at t=1026 (segment 115 corpses with it, drops its
    /// ball at t=1028, and slot 115 is re-minted as a (10,0) corpse
    /// fire at t=1029); at t=1031 pack-mate 174's blind `+52` write
    /// revives the head's CORPSE to chase (38), it dies again at
    /// t=1032, and at t=1033 the walk stamps 41 onto the fire. Class-10
    /// state 41 is the mana-ball handler, so slot 115 lives out the
    /// level as a model-0 ball: ball sprites and physics, it merges
    /// other drops' mana (0 → 1500 → 3000 → … 4888 at the end), but
    /// every possession/collection filter wants model 39 and the map
    /// draws a class-10 non-ball in the unowned magenta. Retail
    /// (conformance): the blind walk. Patched: the walk stops at the
    /// first link that is not a class-5 record carrying the head's
    /// `+24` (every segment is a `qmemcpy` of its head).
    ///
    /// **The same class through the PACK `+52` link** (2026-10-05,
    /// the player's "ghost mana on flat ground, with stuck fires"
    /// report; witness `ghostmana.mgcr`, mc1 level 9): a pack member's
    /// death or hit (`sub_1A390` :21746 / :21755-65) stamps `+70 =
    /// base + 2` through `+52` with no test, and nothing clears `+52`
    /// when the leader dies. A (5,10) leader's slot re-minted as the
    /// player's (10,0) fireball fire takes `60 + 2 = 62` — the
    /// class-10 mana-SPHERE handler. Patched: the write lands only on a
    /// class-5 record of the member's own `model65`
    /// (`Gen::pack_partner_retarget`).
    pub mc1_segment_chain_revalidate: bool,
    /// **THE STALE VOLCANO REGISTERS (MC1 AND MC2)** — round 158 (w158b), the
    /// player-reported FREEZES (`recordings/mc1l45-froze.mgcr`, and the
    /// older `mc1l26-froze.mgcr`). A volcano starting its eruption
    /// (`sub_25EC0` :28778-79; `CARPET.EXE` file 0x3E7B4 `76 06` = the
    /// only gate, `slot > pool base`; 0x3E7B6 `66 c7 42 1a fa 00`)
    /// kicks the PREVIOUS erupting volcano dormant by writing `+26 =
    /// 250` into whatever record now holds the global register's slot
    /// — no class, model or life test. When that slot has been
    /// re-minted as a CASTLE, `+26` is its LEVEL: the castle becomes
    /// level 250. Every later downgrade reads the build table (69/78
    /// entries) out of bounds; rows 250..246 happen to read 0 rows, but
    /// the collapse walker `sub_28FE0` (file 0x417D8; its only exit is
    /// the row counter at 0x418E8, decremented solely on a zero byte)
    /// run for row 245 reads 127 rows from a garbage pointer and never
    /// returns. Both witnesses hang on exactly that tick: mc1l45-froze
    /// castle 966 (level 3 → 250 at t=27730, 245 in state 6 at the last
    /// record t=28506) and mc1l26-froze castle 984 (1 → 250 at t=31424,
    /// 245 in state 6 at t=31915). The same eruption start soft-kills
    /// the old plume register's slot just as blindly (:28782-93) —
    /// mc1l45 t=23826 (w158c): a rival's Fireball token, whose loss
    /// ends in four hovering orphan learn tokens — and a stale kick
    /// naming the new driver's own slot self-kicks it (mc1l49
    /// t=29062). Retail (conformance): all three. Patched: the kick
    /// lands only on a `(10,18)` other than the driver itself, the
    /// plume kill only on a `(10,19)`.
    ///
    /// **MC2 twin** (w158e; renamed from `mc1_volcano_register_revalidate`
    /// when it took this on): the ground-vortex controller `sub_32A70`
    /// (`NETHERW.EXE` file 0x57270) has the same two blind writes
    /// through `word_0x31`/`word_0x33`. Kick: file 0x5735A-0x57379,
    /// `cmp Entities[0]; jbe` then `movl $0xfa,0x10(%eax)` — a 32-bit
    /// `@0x10 = 250` into whatever holds the slot (a CASTLE's @0x10 is
    /// its level; a self-kick stops the new vortex, whose @0x10 is
    /// re-read at 0x5743C and 0x574CB). Kill: file 0x573BE-0x573DE,
    /// `word_0x33` read AFTER the new `(10,19)` spawn, `call 0x7c710`
    /// (`flags |= 0x400`) on whatever holds it — a stranger (mc2l22
    /// t=23012: a loose `(10,39)` 1000-mana sphere, slot 986), or the
    /// brand-new column itself when it was minted into the stale slot.
    /// Patched: the kick lands only on a `(10,18)` other than the
    /// driver itself, the kill only on a `(10,19)` other than the new
    /// column. See [`crate::engine::features::Gen::mc2_summit18_tick`].
    pub volcano_register_revalidate: bool,
    /// **THE LEVIATHAN LUNGES TO A ROLLED HEIGHT (MC2)** — player-ruled
    /// 2026-09-30, an UNFAITHFUL balance deviation, the enhanced
    /// lift's counterweight (the app raises it only while the
    /// altitude model is the enhanced lift). Retail's (5,28) bite is
    /// `sub_1CED0`: a 768 sphere (`sub_583F0` 3-D distance) around a
    /// creature that strikes from sea level — all 162 strikes in the
    /// corpus have it at z 0 — so a carpet 768 above the water cannot
    /// be bitten. Retail's carpet CAN get there (band 1024), but only
    /// by flying pitched against a 16/tick sink: of 151 recorded
    /// strikes at the human 20 found him at 768 or higher and all 20
    /// missed (mc2l18 t=7583..7985 holds 878..955 through eight).
    /// The enhanced lift PARKS at 1024, which turns that effort into
    /// standing immunity.
    ///
    /// Patched: each strike rolls its own height off the leviathan's
    /// LCG, uniform in 768..=1200, and carries it in the strike
    /// pose's height extent (`array_0x52_82.fov`, which retail stamps
    /// 768 — `SetEntityShiftRot_49EA0(a1x, 384, 768)`); the bite
    /// tests the same 768 sphere with the target's height above the
    /// creature scaled by 768/height, i.e. an ellipsoid that tall and
    /// 768 wide, and the strike sprite is drawn stretched upward by
    /// the same factor. Parked at 1024 straight overhead, about two
    /// strikes in five reach. Retail (conformance, recordings): no
    /// roll, 768.
    pub mc2_leviathan_high_lunge: bool,
    /// **THE LEVIATHAN LEADS ITS TARGET BY THE TARGET'S SPEED (MC2)**
    /// — player-ruled 2026-09-30, an UNFAITHFUL deviation; the second
    /// half of the app's `mc2_leviathan_high_lunge` option (not gated
    /// on the altitude model — it is right under either).
    ///
    /// Retail's (5,28) never steers at its target. `sub_2B260`'s chase
    /// arm copies the target's position, steps the copy a FIXED 768
    /// along the target's own yaw (`MoveEntity_57FA0(&v23x,
    /// v25x->yaw_0x1C_28, 0, 768)`, EF:21035) and both steers at and
    /// range-tests against that point; the strike then only re-aims at
    /// the real target every 8th tick and only beyond 896. The lead is
    /// what a carpet at the 80/tick speed cap covers in ~10 ticks, so
    /// it suits a target in full flight — and lunges at empty water
    /// three tiles off a target that is standing still, in whatever
    /// direction that target happens to face. Retail's carpet keeps
    /// its throttle, so it seldom stood still; the enhanced hold-to-fly
    /// carpet rests at a hover, which made the creature all but
    /// harmless ("faithful AND wrong", player).
    ///
    /// Patched: the lead is LINEAR in the target's speed — 768 at the
    /// 80/tick cap and beyond (retail's point, for a target flying
    /// flat out along its facing), nothing at a standstill, so a
    /// hovering carpet is lunged at directly. The human's lead follows
    /// his actual displacement over the last tick (the enhanced carpet
    /// can slide sideways of its facing); any other target's follows
    /// its speed word along its yaw. See `Gen::m28_lead`.
    pub mc2_leviathan_true_aim: bool,
    /// **AIRBORNE CREATURES FLY OVER THE TERRAIN FENCE (MC2)** —
    /// player-ruled 2026-10-03, an UNFAITHFUL deviation.
    ///
    /// Retail's creature move core (`sub_1B8C0`, `Gen::mc2_move_core`)
    /// tests each step's DESTINATION TILE — its capability bits against
    /// the behaviour row's mask (`sub_102D0`) and its roughness against
    /// the row's slope fence `v_16` — and never looks at the creature's
    /// altitude. A creature high in the sky is fenced exactly like one
    /// on the ground. Wanderers shrug it off (their next turn finds
    /// another way), but the mana worm (5,22) steers in a straight line
    /// for its owner's castle and parks forever against a steep wall it
    /// is visibly far above — mc2:022's tall rampart blocks trap whole
    /// worms — and the m23 mana siphon (slope fence 20) shows the same
    /// stalls. Crater clearing the way was the only cure.
    ///
    /// Patched: a creature AIRBORNE where it stands (at least
    /// [`crate::mc2::mobs::FLY_OVER_AIRBORNE`] above its own tile's
    /// ground) that would pass at least
    /// [`crate::mc2::mobs::FLY_OVER_CLEARANCE`] above the destination
    /// tile's ground is not fenced by that tile. Walkers never qualify
    /// (they stand at their row's hover height over the ground — a
    /// walker at a cliff top is not airborne, so it cannot step off
    /// the cliff), and caves keep retail's law whole (the ceiling
    /// tests are the real obstacle there).
    pub mc2_flyers_clear_terrain: bool,
    /// **THE TRADER THE SKELETONS FORGOT (MC1 AND MC2)** — player-ruled
    /// 2026-10-05. A skeleton with nothing to chase converts the
    /// nearest civilian within reach into a new (5,9), hunting ONE
    /// model per think window off its own clock: `(+63 / v_26) % 3`
    /// over the rosters of m4 (militia / archer), m12 and m13 (MC1
    /// `sub_1D060` / `sub_1D6D0`, remc1 :23837 / :24031; MC2 `sub_203D0`
    /// / `sub_20940`, EF:12150). The fourth civilian, m14 (MC1's
    /// cross-map migrant — the backpack trader; MC2's builder/trader
    /// sibling of m12), is on no window, so it alone can never be
    /// converted — an oversight. Retail corpus (82 MC1/HW takes): 134
    /// m4, 22 m12, 20 m13, 0 m14 conversions. Castle guards (m15) and
    /// every non-civilian (MC2's goats included) stay off the menu.
    ///
    /// Patched: the rotation runs `% 4` over {m4, m12, m13, m14}, each
    /// model a quarter of the windows. Retail: `% 3`, m14 immune.
    pub skeletons_convert_traders: bool,
    /// THE FOOL'S-MANA BREAK (MC2): a rival's mana-ball pick
    /// (`sub_148E0` EF:6518-6609) walks the class-10 sphere chain, and
    /// at every non-own FOOL'S sphere (model 57) rolls its Perception
    /// — `if (watcomrand() % 255 < Perception) break;` (EF:6544-49).
    /// The roll is the wizard seeing through the decoy; the `break`
    /// abandons the WHOLE search, keeping only the balls before that
    /// sphere in slot order. A perceptive rival facing a few fool's
    /// spheres therefore all but stops collecting mana — the sharper
    /// the wizard, the worse (third-party report via the player,
    /// 2026-11-22). Read as a `break`-for-`continue` slip.
    ///
    /// Patched: a landed roll SKIPS that sphere and the walk goes on
    /// (the same roll, still on the global watcom stream — one draw
    /// per non-own m57 node, all of them now). Retail: the break.
    pub mc2_fools_mana_skip: bool,
}

impl WorldPatches {
    /// Every patch off — retail's shipped behavior, bug for bug. The
    /// world-construction default; what conformance, goldens and
    /// `--record`/`--replay` runs use.
    pub const RETAIL: WorldPatches = WorldPatches {
        jar_ground_snap: false,
        ball_ground_track: false,
        map_wide_ball_rolling: false,
        mc2_worm_possess_map_wide: false,
        possessed_footprint: false,
        mc2_downgrade_overflow: false,
        castle_latch_bug: false,
        no_spell_loss: false,
        mc1_fix_dragon_tail: false,
        mc1_pack_speed_restore: false,
        mc2_phantom_castle: false,
        dual_wield_muzzle: false,
        one_castle_per_wizard: false,
        mc2_wyvern_alliance_brain: false,
        mc2_orphan_balloon_reap: false,
        mc2_house_flag_color: false,
        mc2_immediate_reap: false,
        mc1_recycle_victim_revalidate: false,
        mc1_castle_transform_watchdog: false,
        mc1_crushed_site_collapse: false,
        mc2_building_pad_saturate: false,
        mc1_building_pad_saturate: false,
        mc1_segment_chain_revalidate: false,
        volcano_register_revalidate: false,
        mc2_leviathan_high_lunge: false,
        mc2_leviathan_true_aim: false,
        mc2_flyers_clear_terrain: false,
        skeletons_convert_traders: false,
        mc2_fools_mana_skip: false,
    };

    /// The pre-option behavior set: what native play hard-wired
    /// before the patches became options (2026-08-08). Port
    /// recordings taped before the `--record` force-retail policy
    /// replay under THIS set — it is the sim their inputs were
    /// recorded against. `map_wide_ball_rolling` did not exist then.
    /// (`castle_recast_cost`, live-law castle pricing, was part of
    /// this set until its retirement on 2026-09-07 — player-ruled:
    /// the lockout is retail's and the relief made the early game
    /// too easy; such recordings replay under the lockout now.)
    pub const LEGACY: WorldPatches = WorldPatches {
        jar_ground_snap: true,
        ball_ground_track: true,
        map_wide_ball_rolling: false,
        mc2_worm_possess_map_wide: false,
        possessed_footprint: true,
        mc2_downgrade_overflow: true,
        castle_latch_bug: true,
        no_spell_loss: false,
        mc1_fix_dragon_tail: false,
        mc1_pack_speed_restore: false,
        mc2_phantom_castle: false,
        dual_wield_muzzle: false,
        one_castle_per_wizard: false,
        mc2_wyvern_alliance_brain: false,
        mc2_orphan_balloon_reap: false,
        mc2_house_flag_color: false,
        // Native MC2 freed in-walk from the first port until round 148
        // (2026-09-17) landed retail's sweep; the player's 2026-09-18
        // ruling keeps the in-walk free as the native default.
        mc2_immediate_reap: true,
        // Both 2026-09-18 (round 151); no port recording predates
        // the option class they join, so LEGACY carries them off.
        mc1_recycle_victim_revalidate: false,
        mc1_castle_transform_watchdog: false,
        // 2026-09-19 (round 157); no port take predates it.
        mc1_crushed_site_collapse: false,
        mc2_building_pad_saturate: false,
        mc1_building_pad_saturate: false,
        mc1_segment_chain_revalidate: false,
        volcano_register_revalidate: false,
        mc2_leviathan_high_lunge: false,
        mc2_leviathan_true_aim: false,
        mc2_flyers_clear_terrain: false,
        skeletons_convert_traders: false,
        mc2_fools_mana_skip: false,
    };
}
