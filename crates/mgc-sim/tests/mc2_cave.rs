//! CAVE FIXTURE over real level-014 data — the roster-richest cave (32
//! pillars, 61 brutes, 92 bees, 25 switches) — under the full MC2
//! profile. Positively exercised: the load settle (sculptors, pillar
//! MEASURE and the load-time arms) holds the floor↔ceiling invariant
//! over the whole map, the cave-only roster spawns
//! ((14,2)/(5,24)/(2,6)), the cave-EXCLUDED ctors spawn NOTHING, the
//! (10,86) drip spawner fires on its 8-turn cadence, and a NATIVE
//! Cave-In cast (spell 25, the one cave-only spell) flies, impacts and
//! collapses terrain through the (9,30) → (10,89) chain.
//!
//! Golden hashes pin the trajectory (the MC1 goldens in state_hash.rs
//! and the mc2_slice level-000 goldens are untouched — shared chassis,
//! separate fixtures). Self-skips without baked mc2 data.

use mgc_sim::Hx;
use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::ids::GameId;
use std::path::PathBuf;

#[path = "common/mod.rs"]
mod common;

fn baked_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    (p.join("mc2/level-014.mgcl").exists()
        && p.join("assets/mc2-cave/build.tab.bin").exists()
        && !common::modded_bake(&p))
    .then_some(p)
}

fn build_world(root: &std::path::Path) -> Option<World> {
    build_world_level(root, "mc2/level-014.mgcl").map(|(w, _)| w)
}

fn build_world_level(
    root: &std::path::Path,
    level: &str,
) -> Option<(World, mgc_formats::LevelPackage)> {
    let file = std::fs::File::open(root.join(level)).unwrap();
    let pkg: mgc_formats::LevelPackage = mgc_formats::mgcl::read(file).unwrap();
    let terrain = pkg.terrain.as_ref()?;
    let ceiling = terrain.ceiling.clone().unwrap_or_default();
    if ceiling.is_empty() {
        // A bake without a ceiling plane — nothing to pin.
        return None;
    }
    let planes = Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone().unwrap(),
        angle: terrain.angle.clone().unwrap(),
        ceiling,
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-cave")).unwrap();
    let mut assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap()
    .with_bldgprm(bundle.bldgprm.as_deref().unwrap_or_default());
    if let Some(sp) = bundle.spells.as_deref() {
        assets = assets.with_spells(sp).unwrap();
    }
    // Day-sourced extents (Bundle::mc2_extent_dims — the boot-time
    // TMAPS0-0 law), whichever bank the level renders.
    if let Some(dims) = bundle.mc2_extent_dims(&root.join("assets")) {
        assets = assets.with_mc2_sprite_ext(mgc_sim::mc2::derive_sprite_extents(&dims));
    }
    let seed = pkg.gen_params.as_ref().map_or(0, |g| g.seed);
    let mut w = World::new_for_game(planes, &pkg.things.things, seed, assets, GameId::Mc2);
    w.set_placeholders(true);
    // Caves are non-Day: runtime repaints invert relief shading
    // (sub_462A0's non-day arm) — the app sets the same.
    w.set_mc2_night_shade(true);
    if let Some(st) = pkg.stages.as_ref() {
        let rows: Vec<(i8, i16, i16, i16)> = st
            .checkpoints
            .iter()
            .map(|c| (c.index, c.stage, c.x, c.y))
            .collect();
        w.set_mc2_stages(&rows);
        let vars: Vec<(i8, i8, u8, u8, u32)> = st
            .variables
            .iter()
            .map(|v| (v.index, v.stage, v.x, v.y, v.data))
            .collect();
        w.set_mc2_stagevars(&vars);
    }
    Some((w, pkg))
}

fn hover(w: &mut World, x: f32, z: f32, ticks: usize, cmd: PlayerCommand) {
    for _ in 0..ticks {
        let alt = w.ground_height_tiles(x, z) + 2.0;
        w.tick(PlayerPose::from_tiles(x, alt, z, 0.0, 0.0, 0.0), cmd);
    }
}

/// An OPEN cavern spot: a 3×3 unsealed neighborhood with > 40 height
/// units of headroom (most of a cave map is sealed rock — parking in
/// it squashes the player against the pinned ceiling and detonates
/// any cast on the spot, authentically).
fn open_spot(w: &World) -> (f32, f32) {
    let p = w.planes();
    let c = w.ceiling_plane();
    for y in 8..248usize {
        for x in 8..248usize {
            let ok = (0..3).all(|dy| {
                (0..3).all(|dx| {
                    let t = (y + dy - 1) * 256 + (x + dx - 1);
                    p.angle[t] & 8 == 0 && c[t] as i32 - p.height[t] as i32 > 40
                })
            });
            if ok {
                return (x as f32 + 0.5, y as f32 + 0.5);
            }
        }
    }
    (64.5, 64.5)
}

fn count(w: &World, class: u8, model: u8) -> usize {
    w.debug_pool()
        .1
        .into_iter()
        .filter(|e| e.class == class && e.model == model && e.life >= 0)
        .count()
}

/// THE invariant over the whole map: ceiling > floor ⇔ bit3 clear.
fn invariant_violations(w: &World) -> usize {
    let p = w.planes();
    let c = w.ceiling_plane();
    (0..c.len())
        .filter(|&t| {
            let open = c[t] > p.height[t];
            let sealed_bit = p.angle[t] & 8 != 0;
            open == sealed_bit
        })
        .count()
}

/// The scripted cave run; returns the checkpoint hashes.
fn run(root: &std::path::Path) -> Option<(Vec<u64>, Vec<u64>)> {
    let mut w = build_world(root)?;
    let (sx, sy) = open_spot(&w);
    let idle = PlayerCommand::default();
    let mut hashes = vec![w.state_hash()];
    let mut obs = vec![w.observable_digest()];

    // A: idle in an open cavern — the walker/bee cadences + the drip
    // spawner's 8-turn cadence in front of the parked pose.
    hover(&mut w, sx, sy, 64, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // B: a NATIVE Cave-In (spell 25, LEFT hand, tier 0) fired into
    // the cavern — the (9,30) manifestation detonates on the nearest
    // wall/ceiling and the (10,89) radial collapse runs to
    // completion (wave 227 → 1024 at +22/tick ≈ 37 ticks).
    w.set_dev_spells(Hx::FIRST, true);
    let select = PlayerCommand {
        mc2_select: Some((25, 0, 0)),
        ..Default::default()
    };
    hover(&mut w, sx, sy, 1, select);
    let firing = PlayerCommand {
        fire_left: true,
        ..Default::default()
    };
    hover(&mut w, sx, sy, 2, firing);
    hover(&mut w, sx, sy, 96, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // C: the sweep's disposition storm (matches mc2sweep) — trips
    // the switch column, materializing the dis-gated brutes/bees.
    for dis in 1..=64 {
        w.debug_fire_disposition(dis);
    }
    hover(&mut w, sx, sy, 64, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // StageVar hold-gate: with the row table baked VERBATIM (byte0 is
    // a FLAG byte, not signed — reading it signed drops every flagged
    // row), level-014's full table loads: the flagged rows binding
    // kind-1 walkers, kind-4 guardians and kind-6 timer spawns. The
    // kind-9 model-18 hold (THING 334, &2-clear bound watch on
    // template 6) HOLDS at load — the AUTHORED death-watch law
    // (player-ruled 2026-07-25, data-faithful): thing 6 spawns at
    // load ALIVE, so the bound watch stands until it actually dies.
    // (Retail's in-level checkpoint autosave severs the watch into a
    // per-config coin — see the level-004 ground-truth trace; the
    // port implements the level data.) Pin the census by kind
    // including the standing kind-9 hold.
    // The held model-18 takes a (10,0) fire hit in the storm's wake —
    // a FOREIGN attacker breaks its hold (`mc2_held_hit`, StageVar2 =
    // 10) and retail's re-leash reclaims it the moment it is neither
    // attacking nor fleeing (`sub_12500` case 0xA). How long that
    // aggro cycle runs is the creature's own slot-seeded RNG, and
    // round 112's native human record moved every later slot by one:
    // the old layout had calmed inside the 64-tick hover, the retail
    // layout has not. Wait for the re-leash (bounded) before pinning
    // the census — the law under test is the hold, not the cadence.
    for _ in 0..512 {
        if w.debug_mc2_held().iter().any(|&(_, _, k)| k == 9) {
            break;
        }
        hover(&mut w, sx, sy, 1, idle);
    }
    let held = w.debug_mc2_held();
    assert!(
        held.iter().any(|&(_, _, k)| k == 9),
        "level-014: the kind-9 bound watch HOLDS while the watched thing lives"
    );
    let mut kinds = std::collections::BTreeMap::<u8, usize>::new();
    for &(_, _, k) in &held {
        *kinds.entry(k).or_insert(0) += 1;
    }
    assert_eq!(
        kinds,
        [(1u8, 7usize), (4, 26), (6, 26), (9, 1)]
            .into_iter()
            .collect(),
        "level-014 held census by kind (verbatim StageVar rows)"
    );

    Some((hashes, obs))
}

#[test]
fn mc2_cave_behaviors_and_goldens() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let Some((got, obs)) = run(&root) else {
        common::golden_skip("mc2 level-014 has no baked ceiling (pre-EPOCH-8 bake)");
        return;
    };
    assert_eq!(
        (got.clone(), obs.clone()),
        run(&root).unwrap(),
        "cave run is not deterministic"
    );
    println!("mc2 cave hashes: {got:#018x?}");
    // Print the OBSERVABLE projection too. It is asserted far below the
    // layout pin, so a moved layout hash used to hide it entirely (the
    // "23c trap"): you could not tell a layout-only re-pin from a real
    // behaviour change without editing the test first. Printing both up
    // front makes the A/B a one-run question.
    println!("mc2 cave OBSERVABLE: {obs:#018x?}");

    // Behavior probes on a fresh world.
    let mut w = build_world(&root).unwrap();
    let idle = PlayerCommand::default();

    // The load settle held the invariant everywhere (probe = 0 on
    // all 47 caves; this pins the fixture level forever).
    assert_eq!(invariant_violations(&w), 0, "post-settle invariant");

    // The cave-only roster: pillars are authored load-time (DisId
    // −1) and measured in the settle; the level's brutes/bees are
    // ALL dis-gated (61 + 92 records) — fire the sweep's
    // disposition storm to materialize them, exactly like retail's
    // switch column would.
    let pillars = count(&w, 14, 2);
    assert!(pillars >= 20, "pillars measured + idle ({pillars}/32)");
    for dis in 1..=64 {
        w.debug_fire_disposition(dis);
    }
    let brutes = count(&w, 5, 24);
    assert!(brutes >= 10, "cave brutes spawned ({brutes}/61)");
    let bees = count(&w, 2, 6);
    assert!(bees >= 10, "cave bees spawned ({bees}/92)");

    // Cave-EXCLUDED: the flying-bee siblings and the m27 kraken
    // never spawn here whatever the level authors.
    assert_eq!(count(&w, 2, 7) + count(&w, 2, 8), 0, "no (2,7)/(2,8)");
    assert_eq!(count(&w, 5, 27), 0, "no (5,27) in caves");

    // The (10,86) drip spawner: every 8th turn, one drip lands on an
    // empty passable tile in the 20×20 window 10 tiles ahead of the
    // player (life 9 — sample every tick). Park mid-map, where the
    // window reaches carved type-0 floor (the open pocket's window
    // is all typed rock — the search finds nothing there,
    // authentically).
    let (sx, sy) = open_spot(&w);
    let mut saw_drip = false;
    for _ in 0..32 {
        hover(&mut w, 64.0, 64.0, 1, idle);
        saw_drip |= count(&w, 10, 86) > 0;
    }
    assert!(saw_drip, "the 8-turn drip cadence fired");

    // The native Cave-In: the (9,30) manifestation flies (often
    // sub-tick — 1.5 tiles/tick to the nearest wall detonates inside
    // the launch tick, authentically), the (10,89) collapse appears
    // and the ceiling under the burst moves (terrain is the weapon).
    let ceiling_before: Vec<u8> = w.ceiling_plane().to_vec();
    w.set_dev_spells(Hx::FIRST, true);
    let select = PlayerCommand {
        mc2_select: Some((25, 0, 0)),
        ..Default::default()
    };
    hover(&mut w, sx, sy, 1, select);
    let firing = PlayerCommand {
        fire_left: true,
        ..Default::default()
    };
    hover(&mut w, sx, sy, 2, firing);
    let mut saw_collapse = count(&w, 10, 89) > 0;
    for _ in 0..96 {
        hover(&mut w, sx, sy, 1, idle);
        saw_collapse |= count(&w, 10, 89) > 0;
    }
    assert!(saw_collapse, "the (10,89) collapse ran");
    assert_ne!(
        ceiling_before,
        w.ceiling_plane(),
        "the collapse moved the ceiling"
    );
    // Every terrain writer re-ran the invariant.
    assert_eq!(invariant_violations(&w), 0, "post-collapse invariant");

    // GOLDEN: pin the checkpoint hashes. Re-pin deliberately when a
    // cave system lands a fidelity fix (document the move in git).
    //
    // Re-pinned for the m9 grounded-arm fix (`sub_20940` EF:12357-89):
    // the damage/death head now runs FIRST (a grounded hive was
    // unkillable), the stand-up counts UP and only the tick that READS
    // -1 stands the hive back up, an AWAKE hive arms the 50-tick
    // stand-up instead of scanning, and an ASLEEP hive parks at 0 and
    // feeds in place rather than cycling back to a 400-tick walk. This
    // level authors m9, so its hives move and eat on a different
    // schedule. The first two checkpoints hold; the last two move.
    // ATTRIBUTED by probe: the magic-mine teardown landed in the same
    // batch and moves NOTHING here (identical hashes before and after
    // it), so this re-pin is m9 alone.
    //
    // Re-pinned for the full `ApplyEvents_498A0` load settle (EV:410-
    // 556): authored scorch rings (level 014 has 9) now dig their
    // craters DURING the load instead of the first 40 live ticks, the
    // settle steps the global LCG once per pass (EV:420), and settled
    // one-shots are reaped at load (slot layout shifts). ALL FOUR
    // checkpoints move — the world enters play with different terrain,
    // RNG phase and pool layout, which is the fidelity fix itself.
    // Re-pinned (last checkpoint ONLY) for the mc2:04 battle fixes:
    // the m9 brain's prey scan now seeks CASTLES over the class-3
    // chain (EF:12119-21), its cone scan is class-3-only, and the
    // stagevar kind-3/4/5 dead-watch scrub (sub_12500 EF:5086-89)
    // re-resolves a dead watch to the next live victim. Level 014
    // authors m9 + kind-4/9 holds, so the disposition-storm phase
    // (checkpoint D) plays out differently; A-C hold.
    //
    // Re-pinned (LAST checkpoint only) for the immediate-rescan-on-
    // release nudge (released creatures get f63 = 0 so the acquire
    // scan runs the release tick — the retail-observed mc2:04 worm
    // switch timing): the disposition-storm phase's gate releases
    // now rescan immediately.
    //
    // Re-pinned (B-D; load checkpoint holds) for the AUTHORED
    // death-watch law (player-ruled 2026-07-25, data-faithful,
    // replacing fire-at-bind): level 014's kind-9 model-18 hold now
    // STANDS at load (its watched template-6 entity spawns alive),
    // so the m18 stays held through every later checkpoint. The
    // shipped-retail behavior is a per-config coin created by the
    // in-level checkpoint autosave severing the watch pointer — see
    // docs/traces/mc2-level004-stagevar-ground-truth.md; the port
    // follows the level data.
    // Re-pinned (all four, layout-only) for the MC1 `rival_wanted`
    // per-rival village-wanted timers joining the shared Gen hash: MC2
    // never flags a rival wanted (that mechanism is MC1's), so the delta
    // at every checkpoint is the new all-zero field in the hash input.
    // Re-pinned (all four) for the traced MC2 sphere mover
    // (TransformArcherToMana EF:26015, kinematics round): spheres now
    // SETTLE (the @0x39 countdown freeze — the ctor's f58 = 0x80 was
    // always seeded, the tick previously ignored it under MC2), the
    // ground rebound is the exact −impact/4-zeroed-at-≤16 law
    // (EF:26244-52, replacing the untraced −32 floor), merges are
    // grounded-only (EF:26265-69), and every re-sprite writes the
    // per-size rotation quad (EF:26744-77). Behavior change toward
    // retail by design.
    // Re-pinned (B-D; load checkpoint holds) for the corpus-solved
    // cave rand structure: one unconditional tick-top draw (the
    // post-pass baseline retired), the carpet tail's additive = the
    // POST-increment turn landing after every draw at the carpet's
    // position (post-pass in native play), and the drip cadence
    // keyed on the incremented turn. Behavior change toward retail
    // by design — the mc2l30 corpus fits 9,079 of 9,337 pairs
    // exactly under this stream.
    // Re-pinned (all four) for day-sourced sprite extents
    // (Bundle::mc2_extent_dims): retail derives its particle-param
    // table once at boot against TMAPS0-0, so cave levels run
    // day-art collision boxes/pitches (52 param rows shift vs the
    // cave bank). Behavior change toward retail by design.
    // Re-pinned (B-D; the load checkpoint holds) for the mana-sphere
    // MERGE law: the partner search is retail's map-tile RING walk
    // (`sub_10A50` EF:3876 / MC1 `sub_11D10` :17127 — rounded base
    // tile, `(applied_pitch + 255) >> 8` rings) instead of a
    // whole-pool scan, and MC2's absorbed donor now takes the HARD
    // free every `sub_36D50` arm ends in (`sub_57F20`) instead of a
    // one-tick soft-kill. Both shift which cave drips coalesce and
    // when their slots return to the stack. Behavior change toward
    // retail by design — mc2l24's fountain window loses 112 of its
    // 662 extra spheres and mc2l30 0+2000 loses 36 of 149 extras.
    // Re-pinned (B-D; the load checkpoint holds) for the 180° TURN
    // TIE-BREAK law (Gen::turn_sign, mc1/mobs.rs — retail keeps the
    // raw sign on an EXACT half-turn, sub_582F0 Sound.cpp:6580 /
    // MC1 twin :52664 SYNCHRONIZED): cave creatures at an antipodal
    // wander target now commit the capped turn in retail's
    // direction. Behavior change toward retail by design —
    // mc1l0 0+2000 +5 conforming, mc2l0 0+2000 +25 conforming.
    // Re-pinned (all four) for the MC2 LOAD-TIME SPAWN DATUM + the
    // pit/hill recentre split — the cave stock-bake dig
    // (docs/CONFORMANCE-FINDINGS.md, mc2l3 record-0):
    //   * `PrepareEvents_49540` spawns at the bare tile CORNER
    //     `axis2d.x << 8` (Events.cpp:307/339/353); only the runtime
    //     disposition path `sub_4A310` adds +128 (EF:33014). We added
    //     +128 on both, so every cave sculptor's box rounded a tile
    //     over and its radial profile came out 2x2-symmetric instead
    //     of cell-centred.
    //   * `sub_4A310`'s −128 for models 0x54/0x55 (EF:33129-31)
    //     CANCELS that same function's +128; it belongs to the
    //     disposition path only, never to the load pass.
    //   * the relief-shade inversion keys on the level's MapType
    //     (Terrain.cpp:2030), which retail holds before
    //     GenerateEvents — so the load settle's repaints already
    //     invert on a cave.
    // Behavior change toward retail by design: mc2l3's measured
    // record-0 planes go type 2,244 → 131, height 4,483 → 140,
    // shading 15,432 → 61, ceiling 4,770 → 132 cells.
    // Re-pinned (2nd-4th) for the AREA-BROADCAST TILE ROUNDING
    // fidelity change (`area_write`): the window centers on the
    // NEAREST tile (`(pos + 128) >> 8` — MC1 sub_120B0 and the MC2
    // twin EF:3750/3798 alike) where the old truncation dropped the
    // advancing edge; corpus pin = the mc1l0 t=91 tent claim.
    // (Previous re-pin: the sphere vertical law, ball_tick.)
    // Re-pinned (2nd-4th; the load checkpoint holds) for the MANA
    // MAGNET REGRESSION FIX, both halves. (a) `byte_0x39_57` has
    // exactly ONE writer — `sub_68C70` (EF:55494) off `sub_68BF0`'s
    // sphere-chain loop (EF:55489-90), ported as `mc2_awake_pass`'s
    // second leg; the sphere tick only READS it (EF:26173). The
    // handler's leftover local decrement made that TWO per tick, so
    // every sphere froze in half the ticks retail gives it — the 2nd
    // checkpoint moves on this alone. (b) The (10,54) aura's homing
    // stamp (`word_0x7A_122`) is a PER-TICK handshake — re-stamped by
    // the aura every tick (EF:28364), cleared by the sphere at the
    // head of its own tick (EF:26109) with the `v35` latch that drags
    // a settled sphere — where the port released it only on the
    // moving tail a settled sphere never reaches, latching the claim
    // forever. Both shift which cave drips travel, coalesce and when.
    // Behavior change toward retail by design.
    // Re-pinned (LAST checkpoint only; 1-3 hold) for MC2's BUILDING
    // FOOTPRINT PASS — `sub_10C80`'s missing middle pass over the
    // (10,45) list `dword_38527` (EF:4076-4105), plus the tile scan's
    // matching `(class != 10 || model != 45)` exclusion at EF:4135.
    // A building is linked into the tile chain at its ANCHOR only, so
    // before this an area writer could reach it from a 3x3 window
    // there and nowhere else; retail samples the BUILD00 footprint
    // mask under the writer and takes the hit anywhere in the
    // perimeter. Verified attributable to the pass alone — reverting
    // the dome/scorch-ring writer re-bind in the same patch leaves
    // this hash unchanged. Behavior change toward retail by design.
    // Re-pinned (ALL FOUR) for the BUILDING DEGRADATION LINK moving to
    // its retail home, the PER-ENTITY `fontTypeIndex_0x3D_61` → `f46`
    // (`sub_49A30` EF:32795-98 seeds it; `RemoveCastleStage_385C0`
    // EF:28090 branches on it) — see [`Gen::mc2_spawn_building`]. `f46`
    // is hashed, so every authored building whose `bldgprm` byte_3 is
    // nonzero moves the stream from t=0; the LEVEL's own behavior is
    // unchanged here (no castle level-up and no (10,67) quake grab in
    // this window, and with neither the entity copy equals the table
    // read the collapse used to do).
    // Re-pinned (ALL FOUR) for the BUILDING-LIFE FIELD HOME — the
    // production rate moving from the mana word to its retail home
    // `subSpellIndex_0x2A_42` → `f44`, and the derived mana from
    // `maxMana_0x8C_140` (f136, dead on a building) to
    // `mana_0x90_144` → `f140` (`sub_49A30` EF:32793/32796/32808; the
    // construction finish parks `life = 1000 * subSpellIndex`,
    // EF:27291) — see [`Gen::mc2_spawn_building`]. f44/f136/f140 are
    // all hashed, so every authored building moves the stream from
    // t=0. This level's own BEHAVIOR is unchanged: the parked life
    // comes out identical (the two words are only independent once a
    // conformance import supplies them separately), which is exactly
    // what the layout-independent companion golden below shows by
    // holding.
    // Re-pinned (ALL FOUR) for the MANA-SPHERE CTOR STAMPS (mc1l0
    // (10,39) dig): `spawn_mana_ball` now stamps the source pair
    // 10/39 and the base speed 32 like both retail ctors (MC1
    // sub_3B5A0 :47456-57/:47463; MC2 CreateManaSphere EF:36614-17
    // xtype/xsubtype + actSpeed). All three are hashed fields, so
    // every native sphere — the cave drips included — moves the
    // stream from its spawn tick. The fields are motion-inert for
    // MC2 spheres (nothing in TransformArcherToMana reads them), so
    // this is bookkeeping toward retail's byte image; see the
    // OBSERVABLE verdict below.
    // Re-pinned (all checkpoints) for the DISPOSITION-FIRE stack
    // rebuild: every fire re-ranks the allocator stacks by the
    // descending pool scan and disarms the victim stack after
    // (sub_49F90 at sub_4A1E0's top, EF:32966 + dword_0x11e6=-1 —
    // MC1 twin sub_37220/sub_37440, CARPET.EXE-verified; the mc1l1
    // t=344 slot-allocation wall). Cave dis-fires now allocate
    // ascending-from-lowest. Attribution: the only MC2-visible
    // change of the batch (register + chains are MC1-gated).
    // Re-pinned (LAST checkpoint only; 1-3 hold) for the SCORCH DISC —
    // `dig_scorch` is the ring-0 DISC of sub_40D30 / MC2 sub_572C0
    // (the SEARCH.DAT 2x2 zero block minus the walker's dropped last
    // cell): THREE cells per scorch — center, (+1,0), (0,+1) — each
    // with the full cell update (crater, angle latch, restencil/retile,
    // cave ceiling counter-shift). Verified attributable to the disc
    // alone — reverting dig_scorch to the single-cell form restores
    // this hash with the zero-depth latch change still in. Behavior
    // change toward retail by design.
    // Re-pinned (all checkpoints) for the mc2l0 on-ramp batch
    // (2026-08-21f) — corpus-receipted fidelity laws with hashed
    // lanes: the universal cast-site token-mana copy (mc2_launch,
    // EF:55865 et al — every fired projectile's f140), the impact
    // pitch stamp (f32, EF:63194-95), the fireball terrain-contact
    // move REVERT (sub_65C20's v16x commit), the authored-jar
    // SetSpell tier-0 seeding (f28/f30/f59/f71/f136/f140/max_life),
    // and the sub_377A0 completion painters. The disposition-fire
    // ghost reap was A/B-EXCLUDED (MGC_AB_NO_REAP run reproduced
    // these exact hashes with the reap off).
    // Re-pinned (3rd-4th; the first two hold) for the MC2 HAND
    // MIRROR: `mc2_select_spell` now keeps `Player::left/right` (the
    // hashed import/obs lane) in step with the book — retail's
    // SpellIndexLeft/Right is ONE lane, and the split let an equip
    // pair's obs project the stale hand (mc2l3 t=108, the 12-row
    // player0.hand_left family). The scripted select at the second
    // hover is what moves the stream; checkpoints before it hold.
    // Re-pinned (4th ONLY; 1-3 hold) for the SCATTER SPEED STAMP:
    // `mc2_mana_spheres` now writes the rolled `rand%0x30+16` onto
    // the sphere's actSpeed (f126) like retail's EF:26907 — the
    // ctor's 32 is only the un-scattered default (mc2l3 t=245-260,
    // the crush-scatter 12-row speed family). The checkpoint-4
    // dis-fire sweep's kills scatter spheres inside the window.
    // A/B-attributed: suppressing the single f126 write restores the
    // old hash with every other law of the batch still in.
    // Re-pinned for the 2026-08-23c mc2l0 free-run sprint. THREE of
    // the batch's six laws move this world and each was A/B-isolated
    // by reverting it alone with the rest still in:
    //   - the BUILDING `min_speed` shift (`(w*h) >> 2`, not remc2's
    //     `>> 4` — mc2l0's row-37 village reads 6 on a 5x5, and its
    //     recorded extents invert to w = h = 5): moves ALL FOUR.
    //   - the MC2 STAGE ENGINE moving AFTER the entity walk
    //     (EF:31800-18): moves checkpoints 2-4; the first holds.
    //   - the HAND MIRROR reaching every writer, `mc2_set_hand`
    //     (the load-time `mc2_rebind_hands_canonical` now keeps the
    //     hashed `Player` lane in step): moves ALL FOUR.
    //   - the DRAGGED SPHERE WRITING ITS OWN HEADING at the mana
    //     magnet (`yaw_0x1C_28 = tan2(sphere, aura)`, EF:26101 — the
    //     MC2 half of a write the MC1 ch4 twin already had): moves
    //     checkpoints 2-4; the first holds. A/B-ATTRIBUTED 2026-08-23
    //     — suppressing this one write alone reproduces the PREVIOUS
    //     pin exactly, and it is the ONLY law of that session's six
    //     native-MC2 changes that this golden can see (`f30` on a
    //     (10,39) is a hashed lane; the other five leave it untouched).
    // A/B-EXCLUDED, each reverted alone and reproducing these exact
    // hashes: the ch0 footprint mask probe, the (10,42) painter ctor,
    // the switch mid-walk pose phase, and — from the same 2026-08-23
    // round — the MC2 castle ctor's sprite-table swap, the
    // m17/19/20/28 phase-7 sub-state reset, the pack scan's tick-top
    // roster walk, the drowned-projectile position commit and the
    // unmasked one-shot yaw.
    // Re-pinned (LAST CHECKPOINT ONLY; 1-3 hold) for the AWAKE
    // PRE-PASS DEAD ARM: `sub_68BF0`'s `byte_0x39_57 = 0xFA` stamp
    // (EF:55484-85) is reachable only for a roster member that died
    // BETWEEN the tick-top rebuild and the pre-pass, because the
    // rebuild drops `life < 0` records (EF:39988) — so an
    // already-dead creature's counter FREEZES at its last live value
    // instead of being re-stamped 250 every tick of its death
    // animation. NOT a presentation lane: `mc2/roster.rs` reads
    // `f58 != 0` inside the creature's own handler with no life gate,
    // which is why a cave full of dying creatures diverges only after
    // the long disposition phase — the first three checkpoints are
    // unmoved because the divergence needs deaths to accumulate.
    // A/B-ATTRIBUTED 2026-08-23c: restoring that ONE arm reproduces
    // the previous pin exactly with every other law of the batch
    // still in, and it is the only one of the round this world sees.
    // A/B-EXCLUDED from the same round, each reverted alone and
    // reproducing these hashes: the ejected-sphere `ball_resize`
    // deletion, the +3000 re-cast surcharge and its ladder-drain
    // phase fix (also `MGC_NO_MC2_RECAST_SURCHARGE=1`), the type-0
    // objective's castle-register gate, the balloon sphere-pick reap
    // clause, the `sub_67CB0` tick-top acquisition rosters and the
    // `sub_585D0` post-walk wizard re-floor.
    // ALL FOUR re-pinned for the M3 HEAD's PARTICLE-TABLE READ
    // (`mc2_spawn_m3`, EF:33869-71): the head takes its pitch/roll
    // box from the SAME `particlesParameters_D951C` rows its own
    // child loop reads two lines above — the DERIVED pair, not the
    // shipped static row, whose `speed_6` column is zero almost
    // everywhere (it is filled at load from the sprite bitmap's
    // aspect). A zero-width head could not collide with anything;
    // every checkpoint sees the cave's multipart flyer.
    // A/B-ATTRIBUTED: reverting that ONE read alone reproduces the
    // previous pin exactly with every other law of the batch still
    // in. A/B-EXCLUDED from the same round, each reverted alone and
    // reproducing these hashes: the pack crowd-avoid's un-wrapped
    // i16 gap (`sub_5CFC0` EF:9473 — needs two same-model creatures
    // straddling tile 128), the XP scroll's `sub_106C0` collect, the
    // MC2 balloon-fleet REGISTER, and the strict-only class-14 probe
    // admission (a NATIVE golden cannot see a `strict` gate).
    // Re-pinned (LAST CHECKPOINT ONLY; 1-3 hold) for the MANA
    // SPHERE'S CAVE-CEILING CLAMP (`TransformArcherToMana_35940`
    // EF:26256-63): a sphere thrown up inside a cave stops at the
    // rock, `ceiling − fov`, with its lift forced downward — the
    // ceiling twin of the ground rebound the port already had, and
    // MC2-only (MC1 has no ceiling plane). The first three hold
    // because the divergence needs spheres to be launched high enough
    // to reach the roof, which the disposition storm supplies.
    // A/B-ATTRIBUTED: reverting that ONE clamp alone reproduces the
    // previous pin exactly with every other law of the batch still in
    // (and the observable assert below is reached under that arm).
    // A/B-EXCLUDED from the same round, unmoved with it in: the
    // CAVE-WALL ESCAPE fan in the same function (EF:26192-26239 — no
    // sphere in this world wedges under rock), the class-10 dead-@0x90
    // projection, `my_sign32` in the meteor quad, the launch aim
    // measured from the caster, and the `sub_11900` point-write
    // protocol.
    // Re-pinned (ALL FOUR) 2026-08-25g for the MC2 MANA-SPHERE CTOR'S
    // MAIL-CHANNEL ADMIT MASK. `CreateManaSphere_500C0` writes
    // `byte_0x38_56 = 3` (EF:36617) and the shared `spawn_mana_ball`
    // stamped only MC1's home (+28), leaving MC2's @0x38 — the port's
    // `f56`, where the importer homes it for class 2/10 and the roster
    // publishes it as the graded `b38` lane — at 0 on every free-run
    // sphere. Corroborated ELEVEN TIMES in one tick: mc2l3 t=9816
    // borns eleven spheres and `explain` shows all eleven taking
    // `b38 0 -> 3`. `f56` is hashed `Ent` state, so EVERY checkpoint
    // that has minted a sphere moves — which is why all four go, and
    // why the move is uniform rather than confined to a window.
    // A/B-ATTRIBUTED: reverting that ONE stamp alone reproduces the
    // previous pin exactly with the aura's chain-walk law still in.
    // ⚠ A/B-EXCLUDED, and this is the useful half: the (10,54) aura's
    // move from a live-pool scan to the TICK-TOP `dword_38523` chain
    // (the mc2l3 t=9816 law, worth +240 on that take) moves NOTHING
    // here — reverting it alone still reproduces these hashes. A law
    // that is worth 240 ticks on the corpus can be invisible to a
    // golden, and a stamp worth 0 ticks can move every checkpoint.
    // Re-pinned (ALL FOUR) 2026-08-25i for THE 77TH BLDGPRM RECORD,
    // which is not in `BLDGPRM.DAT` at all: retail's buffer is
    // `std::array<…, 77>` and its load fills `76 * sizeof(record)`
    // (EF:38328), so index 76 reads the four bytes at 0xD93C0 + 76*4
    // = 0xD94F0 — `str_D94F0_bldgprmbuffer`'s initialised
    // `{0xAA,0x00,0x63,0x0D}`. The villager build lottery's
    // `rand % 0x3C + 17` tops out at exactly 76 and 0x63 has the
    // eligibility bit, so one build in sixty raises that phantom
    // template; `MSPRD00.DAT` row 76 is a 0x0 footprint, so it also
    // has the smallest site box in the game. mc2l3 t=14611 pins all
    // four bytes on four separate graded lanes of one newborn
    // (`b46` 76, `f2a` 170, `b38` 35, `b3d` 13) and the take goes
    // 14610 -> 15418. Every checkpoint moves because a caves world
    // runs villagers too, and the changed pick reshapes what they
    // build from the first lottery on.
    // A/B-ATTRIBUTED: the session's other three laws (the m20
    // state-2 pointer test, and `dword_38519`'s life-test entry
    // condition at its three scan sites) move NOTHING here —
    // reverting the table push alone reproduces the previous pin
    // exactly with all three still in.
    // Re-pinned (ALL FOUR; the OBSERVABLE array below HOLDS) 2026-09-17
    // for ROUND 147's NATIVE-INIT laws — the first round to grade the
    // world CONSTRUCTOR (`mgc-conform init-check`), so a fresh-boot
    // golden is exactly what should move. A/B-attributed by the digs,
    // each law alone: the level-start book (`MGC_NO_MC2_PICKUP_ONLY_
    // STEAL_LOCK`, `_SPELL_DEFAULTS_PATCH`, `_NATIVE_HUMAN_START_POSE`,
    // `_TOKEN_CTOR_CLEARS_2C`) and the authored seats (`_PILLAR_NO_
    // SPRITE`, `_AURA_MANA_ZERO`, `_PORTAL_LAUNCH_AXIS`). COMBINED
    // REVERSION PROBE: all 25 round-147 switches set reproduces the
    // previous pins exactly, here and in `mc2_slice.rs`.
    assert_eq!(
        got,
        // Checkpoint D re-pinned 2026-08-29 (A/B/C hold) for the
        // RIVAL SPEED-TOKEN dispatch family: the token body now runs
        // at the token's own walk slot (GetScroll_69DB0's model-1
        // arm — 240/160 sustain writes, the (10,2) puff on the
        // token's phase byte), the approach brake (word_0xe_14,
        // sub_14C90) collapses a live window into the same-tick 1×
        // restore, and the cast projectile no longer inherits the
        // AI's f146 target (retail hands the bolt target96 = 0).
        // A caves world runs rival wizards, so the late checkpoint
        // reshapes from the first rival SPEED window on.
        vec![
            // Checkpoint A re-pinned 2026-09-02 (B/C/D hold byte-for-
            // byte) for THE FIRE CTOR'S @0x2C SEED: `NewEvent_4A050`
            // writes `subSpellIndex_0x2A_42 = 100` (Events.cpp:569) —
            // that is @0x2A — while the memset leaves @0x2C at 0, and
            // for a (10,0) fire the port's `f44` IS retail's @0x2C
            // (`port_ent_lanes_mc2`'s `f2c` arm lists 10/0 among the
            // ramp2c-style homes). The port had been seeding every
            // fire's fall/flicker register with 100. Measured on
            // mc2l22 t=10914..10932: `(10,0) f2c` 61 rows, retail 0 /
            // port 100, one per fire minted in the window.
            // A/B-ATTRIBUTED: reverting `mc2_spawn_fire`'s `f44 = 0`
            // ALONE reproduces the previous pin exactly with all 25
            // other laws of this session still in; and this session's
            // Cruise-roll `watcomrand()` law and the rival mint-tier
            // law were each independently reverted and moved NOTHING
            // here.
            // Re-pinned (ALL FOUR) 2026-09-03 for THE MC2 ALLOCATOR'S
            // BEHAVIOR-ROW SEED. `NewEvent_4A050` writes
            // `dword_0xA0_160x = &str_D7BD6[59]` in BOTH arms
            // (Events.cpp:573 free pop, :599 sacrifice) — MC2's table
            // BASE, `v_12 = 0` / `v_14 = −4`. MC1's twin writes
            // `var_u32_29951_156 = unk_98F38` (sub_main.cpp:43877 /
            // :43902), which IS index 0, so only the MC2 column needed
            // the stamp. Every natively spawned MC2 record had been
            // running the (5,0) creature row (`v_12 = 7`, `v_14 =
            // +244`) — a wrong hover floor and a wrong vertical drift
            // on everything the sim mints, which is why all four
            // checkpoints move.
            // A/B-ATTRIBUTED: reverting the seed ALONE reproduces the
            // previous pin EXACTLY on checkpoints A, B and C, with the
            // session's other four laws still in.
            // Checkpoint D additionally carries the MANA SPHERE'S
            // INVENTED ROLL: `TransformEntityToManaSphere_36BA0`
            // (EF:26903) writes `yaw_0x1C_28` and nothing else, where
            // the port also stamped `roll_0x20_32` (our `f34`).
            // A/B-ATTRIBUTED: reverting that one line alone moves D
            // and leaves A/B/C byte-identical.
            // A/B-EXCLUDED, and worth recording: the two MC2 cave arms
            // dropping their spurious `ROW_BASE +` (`ball_tick`,
            // `mc2_fool_flight`) move NOTHING here even though this IS
            // a caves world — reverting them alone reproduces these
            // same four hashes.
            // Corpus receipt: mc2l6-rival-spells-galore free horizon
            // 10,087 -> 10,109, clean boundaries 10,319 -> 10,662.
            //
            // ⭐⭐⭐ RE-PINNED 2026-09-04 (dig 98-Q30) — THIS TEST HAD
            // BEEN RED AT HEAD SINCE ROUND 97 WITH THE MOVE
            // UNATTRIBUTED. It is now attributed to exactly TWO laws,
            // and to nothing else:
            //
            // (1) CHECKPOINTS B, C AND D moved AT HEAD (A held
            // byte-for-byte) for THE SHADOW BACK-OFF —
            // `MGC_NO_SV_BACKOFF`, `mc2/stagevars.rs::mc2_sv_walk`,
            // landed in the same commit (f2fb6e8) that wrote the
            // previous pin, but AFTER the pin was computed, so it was
            // never folded in. `sub_1D8C0` (held kinds 3/4/5) closes
            // case 0 with a fourth step `sub_1DDA0` (kind 1) does not
            // have: if |dx| and |dy| are BOTH strictly under the SUM
            // of the two records' `0x54` half-extents, the escort
            // stamps `roll_0x20_32 = tan2(&watched, &self)` — the AWAY
            // angle — over both the toward-aim and the packmate
            // override. VERIFIED IN THE SHIPPED `NETHERW.EXE` at file
            // 0x42345-0x4239D (disassembled, not quoted from memory):
            //   42349 movswl 0x54(%ebx),%eax ; 4234d movswl 0x54(%esi),%ecx
            //   42351 movswl 0x4c(%ebx),%edx ; 42355 add %eax,%ecx   <- the SUM
            //   42357 movswl 0x4c(%esi),%eax ; 4235b sub %eax,%edx
            //   4235f cltd/xor/sub (abs)     ; 42364 cmp %ecx,%eax
            //   42366 jge 0x423e5            <- STRICT `<`
            //   4236c..42387 the same block on 0x4e (y), same jge
            //   42389 lea 0x4c(%ebx),%eax / push / add $0x4c,%esi /
            //   42390 push %esi / 42391 call 0x7c9e0 (tan2) /
            //   42399 mov %ax,0x20(%ebx)     <- roll, AWAY direction
            // A caves world runs stagevar-held escorts, so every
            // checkpoint after the first back-off reshapes; the LOAD
            // checkpoint (A) predates any, which is why A held.
            //
            // (2) ALL FOUR then move again for THE MC2 SPRITE STAMP —
            // `MGC_NO_MC2_FRAMES89` (dig 98-Q12, `mc2/mobs.rs`):
            // `byte_0x5D_93 = x_BYTE_D8A2E[particlesParameters[row].byte_12]`
            // is now stamped on the MC2 column, and `frames89` is
            // hashed `Ent` state, so every checkpoint that has minted
            // a sprited record moves. That one is a HASHED-FIELD move,
            // not behaviour — see the OBSERVABLE assert below, which
            // HOLDS unchanged across it.
            //
            // THREE-WAY A/B (ONE binary md5 b4eb5bf6911bc30fd34e1f874274b33e,
            // back to back, plus a pristine-HEAD build 707787cdab22e3b6…):
            //   * `MGC_NO_MC2_FRAMES89=1 MGC_NO_MC2_FRAME_CAP=1` alone
            //     reproduces the PRISTINE HEAD BUILD's four hashes
            //     exactly (9f0bd0d2… 9286ce62… cc2e833a… 1e7748a9…);
            //   * adding `MGC_NO_SV_BACKOFF=1` reproduces the PREVIOUS
            //     PIN exactly (9f0bd0d2… 552a7ea6… 97c82747… 1ef324a8…);
            //   * setting ALL 33 of round 98's other kill switches on
            //     top changes NOTHING further — every other law of this
            //     round is A/B-EXCLUDED here;
            //   * and on the pristine HEAD build, each of round 97's
            //     69 kill switches was set ALONE: `MGC_NO_SV_BACKOFF`
            //     is the ONLY one that moves a hash.
            //
            // ⭐ RE-PINNED ROUND 105 (2026-09-05) — `MGC_NO_MC2_CLASS5_W36`,
            // the class-5 `@0x36` law. It meets THIS TEST'S OWN STANDARD for
            // a justified re-pin, stated in the OBSERVABLE comment below:
            // the four layout hashes move and **OBSERVABLE HOLDS BYTE-FOR-BYTE
            // UNCHANGED** (0xca0e5c44… 0xb60c271e… 0x020feba3… 0xf08daed0…),
            // which is the independent proof of a hashed-field move rather
            // than behaviour. Attribution A/B'd on ONE binary: green under
            // `MGC_NO_MC2_CLASS5_W36=1`, red without it, so the move is this
            // law's alone.
            // THE LAW: `sub_4C8F0` (`NETHERW.EXE` file 0x710F0) never writes
            // `[eax+0x36]` — its complete store set is 0x04 0x14 0x1c 0x1e
            // 0x20 0x22 0x2a 0x2c 0x38 0x39 0x3e 0x3f 0x40 0x41 0x43 0x44
            // 0x45 0x46 0x82 0x84 0x90 0xa0 — so retail's `@0x36` on a fresh
            // class-5 creature is `NewEvent_4A050`'s zero. All 18 ctors wrote
            // the ch0 damage contract TWICE, once correctly as `e.f28 = 1`
            // and once as a duplicate `e.f56 = 1` landing in a DIFFERENT
            // retail word. `Ent::f56` is `@0x36` on class 5 (the importer
            // agrees: `f56: if matches!(class3f, 2|10) { b38 } else { f36 }`).
            // None of the 18 is a multipart model (0/3/22/27) — verified by
            // re-deriving each site's class/model from the ctor — so no port
            // reader consumes it and the stray 1 was inert but recorded.
            // ⭐⭐⭐ ALL FOUR RE-PINNED 2026-09-06 (ROUND 109) FOR TWO
            // GENERATE-AXIS LAWS, EACH A/B-ATTRIBUTED ALONE:
            // (1) the (10,31) RIVER chain arm (`mc2_stamp_river_leg` →
            // a (10,32) head per leg, action 0x22 = a traveling (10,11)
            // scorch-ring emitter — `sub_487D0`'s `4A190(.., 10, 32)`
            // is DECIMAL (10,32), not the hex (10,50) the trace doc
            // read, so the "inert river" was never inert); level-014
            // carries 19 river/ring/trail records, so its floor is
            // carved differently from tick 0.
            // (2) `mc2_apply_events` frees a finished record INSIDE the
            // sweep, like retail's per-arm `sub_57F20` — a later slot's
            // load-time spawn re-pops it, so every settle-band spawn
            // (authored rings, fire trails, river rings) lands in
            // retail's slot instead of a fresh higher one.
            // A/B: reverting (2) alone gives 0xc8ce1ca59242c806 /
            // 0x1bddaeedd5f02a8f / 0xa0531f1b4c0b05f7 / 0x8abf44b8ae49d939;
            // reverting (1) alone gives 0xc2ba382b0d8d3a69 /
            // 0xeffb74b5307a144f / 0x71474279f9392450 / 0xc7dcb70777a8b628
            // — neither reproduces the previous pin (0x9437af5b85e0b34a /
            // 0xea13f2bab820e3b6 / 0x93f294ee5acf9025 / 0x3a2cb5a2bbae8d82),
            // so BOTH move every checkpoint from the load on. Witness for
            // both: mc2:15's terrain, phase-aligned at t=120, matches
            // retail on all five planes, 65,536/65,536
            // (`terrain-diff --settle`, ROUND 109).
            // ⭐ RE-PINNED 2026-10-10 for THE HUMAN'S SLOT TAG (multiplayer stage 1):
            // the human's owner tag is his carpet record's pool slot (retail's
            // convention), no longer the out-of-pool 0xFFFF, and the record's own
            // `+24` carries it. `MGC_NO_HUMAN_SLOT_TAG=1` restores every old pin.
            0x46d3c771d2719de5_u64,
            // ⭐ CHECKPOINTS B, C AND D RE-PINNED (A HOLDS BYTE-FOR-
            // BYTE) FOR THE MANA AURA'S PULL STAMP. `sub_38D80`
            // (EF:28353-83, shipped `NETHERW.EXE` 0x5d5bd-0x5d610)
            // writes the pulled sphere's `@0x76` speed and `@0x7A`
            // aura index — the port's `mail[4]` pair — and NOTHING
            // else; the port had ALSO written the sphere's
            // `axis_0x9A` VELOCITY there, at the AURA's own walk slot,
            // which ran the polar step one pass early, off the
            // sphere's pre-mover position. See
            // [`mgc_sim::mc2::tail::no_mc2_aura_stamp`]; mc2l1-new
            // goes 1117 -> END, devs 13 -> 0, and the take is
            // CERTIFIED.
            // A/B-ATTRIBUTED: `MGC_NO_MC2_AURA_STAMP=1` reproduces the
            // previous pin (0x6dd134b1443f1fd3 / 0x03410c90208a9e8c /
            // 0xe3e7b5f9e3471341) exactly with the kind-1 arrival-box
            // seam law still in — measured on one binary, both arms.
            // A caves world mints and magnets spheres from its first
            // castle on, so every checkpoint that has pulled one moves.
            // Re-pinned (CHECKPOINT B ALONE; A/C/D and the whole
            // OBSERVABLE array below HOLD) 2026-09-18 for ROUND 149's
            // AURA CLASS-GATE law — `sub_38D80` has no class test, so a
            // chain node recycled out of class 10 mid-tick takes the
            // `@0x76`/`@0x7A` stamp and the hashed `mc2_aura_claim`
            // side map gains its entry. A/B-ATTRIBUTED, this binary,
            // both arms: `MGC_NO_MC2_AURA_CLASS_GATE=1` reproduces
            // 0xae0a114269b7ad69 exactly with the round's other two
            // laws (`MGC_NO_SUMMON_HEAD_VICTIM`,
            // `MGC_NO_MC2_PAINTER_OOB_RESIDUE`) still in, and neither
            // of those moves any checkpoint on its own.
            // ⭐ RE-PINNED 2026-10-10 for THE HUMAN'S CARPET RECORD (multiplayer
            // stage 2): the record at his slot is a live (3,0) wizard record mirroring
            // him, a member of the tick-top class-3 roster and linked into his tile
            // chain — the record's own bytes and its neighbours' `+20/+22` are hashed.
            // Behaviour-neutral by measurement: under `MGC_HASH_EXCLUDE_CARPET=1` (the
            // record spliced out and blanked before hashing) every checkpoint is
            // identical with the stage on and off. `MGC_NO_HUMAN_RECORD_MIRROR=1`
            // restores every old pin.
            // Re-pinned 2026-10-13 (the first checkpoint holds) for MP STAGE 3 CH. 2:
            // the human's (3,0) carpet record carries retail's probe-victim bit
            // (`+16 & 8`, the recorded 0x20D) — `MGC_NO_HUMAN_POOL_VICTIM=1`
            // reproduces the previous pins exactly, and under
            // `MGC_HASH_EXCLUDE_CARPET=1` the values are identical switch on/off
            // (the record's own word is the whole move).
            0x3f1f32978d0b4093,
            // ⭐⭐⭐ CHECKPOINTS C AND D RE-PINNED 2026-09-06 (ROUND 108),
            // A/B HOLD — TWO SEPARATE, SEPARATELY ATTRIBUTED CAUSES.
            //
            // **C moved for THE CAVE-IN'S WIZARD SURVIVAL POCKET.**
            // `sub_311E0` (EF:23003-37) digs a spherical cavity around
            // every class-3 MODEL-0 wizard inside the collapse — floor
            // down to `z/32 - r`, ceiling up to `z/32 + r`, walking
            // `dword_38519` (the tick-top class-3 live list) and
            // testing only `!model`. In MC2 model 0 is the HUMAN
            // CARPET ALONE (rivals are (3,1), castles (3,2)) and the
            // port's carpet lives OUT OF POOL — so the port's pool
            // walk matched NOTHING and the pocket had never once been
            // carved. mc2l15 t=35340..46 under the descending human at
            // tile (51,54): retail's floor falls 68 → 65 → 63 → 61 →
            // 59 → 58 → 57 while the port's stayed frozen at 68.
            // Landing it took mc2l15 from 593 segments to **3, devs=0,
            // horizon=END — the take CERTIFIED**. A caves world flies
            // its human through its own collapses, so this checkpoint
            // moving is the law working.
            //
            // **D moved in ROUND 107 AND WAS NEVER RE-PINNED.** Proved,
            // not assumed: this test PASSED on a build of the commit
            // BEFORE round 107's (`a913653`, "mc1l6-sg certified") and
            // FAILED on round 107's own commit, whose diff is the
            // held-seam / phase-7 wrapper-tail family (roster.rs,
            // stagevars.rs, mobs.rs, proj.rs) and which touched no
            // golden. ⚠ That commit was `279f731` when the bisect ran
            // and has since been SQUASHED away — bisect from `a913653`
            // if this ever needs re-deriving. Same mechanism as the 2026-08-29 re-pin below: a
            // caves world runs creatures, and a changed creature seam
            // reshapes the late checkpoints. ⚠ It was carried into this
            // round as a RED failure on purpose rather than papered
            // over, and is re-pinned only now that its cause is named.
            // (C and D carried forward under ROUND 109's two laws above.)
            // Checkpoints C AND D re-pinned 2026-09-15 (A/B hold BYTE-
            // FOR-BYTE) for THE DEV-SPELLS OVERLAY (player ruling; see
            // `World::set_dev_spells`). The old toggle ran MC1's
            // `rebind_hands_canonical` on the MC2 column too, which
            // WIPED the MC2 book's hand mirror (`player.left/right`,
            // hashed) to None — a side effect the ghost-fireball unit
            // test had pinned as if intended. The overlay mints nothing
            // and leaves the mirror alone. A/B-ATTRIBUTED: putting the
            // wipe back alone (`if on { rebind_hands_canonical() }`)
            // reproduces the previous C and D pins exactly, and the
            // OBSERVABLE digests below hold — the world is unchanged,
            // only the mirror is.
            // RE-PINNED 2026-11-22: the dev-spells toggle re-stamps the held book (`World::set_dev_spells` — the seeded spells pick up the instrument's free gates when it goes on; was 0xbcdf82b98d45cc38). `if false` on that loop restores the old pin.
            0x2a3f556332195f9f, // was 0x57237f7c82d36989
            // Checkpoint D re-pinned 2026-09-10 (A/B/C hold BYTE-FOR-
            // BYTE) for THE (10,77) FIRE-ORB SATELLITE'S CLONED @0x2C —
            // the SAME CLASS as checkpoint A's 2026-09-02 fire-ctor
            // re-pin above. `AddFireSpheres_4F2A0`'s satellite is a
            // whole-record `rep movsl` off the hub (`NETHERW.EXE`
            // 0x73B57-0x73B61, 42 dwords = the 0xA8 stride), and the
            // hub's ctor stored ZERO at @0x2C (0x73B1B
            // `66 c7 43 2c 0000`, EF:35989), so all 25 satellites
            // reach the pool with 0 where ours kept `new_event`'s
            // `+44 = 100`. Witness: mc2l24 t=45560, retail slot 17
            // `f2c` 0 / port 100.
            // A/B-ATTRIBUTED: `MGC_NO_MC2_ORB_SATELLITE_F44=1`
            // reproduces the previous pin (0xc11ac6c4af76bf67) exactly
            // with wave 125's other laws still in, and the OBSERVABLE
            // projection below holds all four unchanged.
            // Checkpoint D re-pinned 2026-09-13 (A/B/C hold BYTE-FOR-
            // BYTE) for THE KIND-1 ARRIVAL-BOX SEAM. `sub_12500`'s
            // 2048-unit stage-hold release box sign-extends EACH
            // operand and subtracts in 32 bits — shipped
            // `NETHERW.EXE` VA 0x125DE is `movswl 0x4(%ecx),%edx` /
            // `movswl 0x4c(%ebx),%eax` / `sub %eax,%edx` — where the
            // port had a 16-bit WRAPPING subtract (`abs16`), a
            // different function whenever the authored fly-point and
            // the creature straddle the 0x8000 axis seam. It is the
            // exact sibling of the kind-2 LEASH box law landed earlier
            // this session at VA 0x1DC14 — the same arithmetic, the
            // other call path (⭐⭐⭐ A LAW ON ONE CALL PATH IS NOT
            // LANDED). A caves world runs stage-held creatures, so a
            // release that fires 36 ticks early reshapes the late
            // checkpoint. Witness mc2l8 slot 67, a (5,17) on a kind-1
            // StageVar at point (33024, 13568): retail holds it until
            // t=610, the tick its own x first crosses 0x8000; the port
            // released at t=574. mc2l8 920 -> 618 segments.
            // A/B-ATTRIBUTED: `MGC_NO_MC2_KIND1_SEAM=1` reproduces the
            // previous pin (0x0a717b6bbb39a15f) exactly with this
            // session's other seven laws still in — verified in the
            // main session, not merely reported.
            // RE-PINNED 2026-09-17 (round 148), checkpoint D ONLY, two
            // state-only laws, OBSERVABLE array unmoved:
            // `MGC_NO_MC2_NULL_DISPATCH_PHASE` (a null dispatch row —
            // the (10,75)/(10,77) swarm nodes — does not clock the
            // hashed `f63`) and `MGC_NO_MC2_CLASS14_MARKER_HIDE_BIT`
            // (the ending markers hide on byte[0] bit 0, not 0x20).
            // Setting BOTH switches reproduces 0xb81708d1a4f89892.
            // …and a THIRD law the same day: `MGC_NO_MC2_NATIVE_TICKTOP_REAP`, since 2026-09-18 the retail arm of the `mc2_immediate_reap` patch
            // (dig w148m — the native reap is retail's tick-top sweep); it
            // moves OBSERVABLE D too. With it set the pin is 0x06f940b92d340c8b.
            // …and a FOURTH: `MGC_NO_MC2_ADMIT_MASK_BOTH_HOMES` (dig w148p — a
            // natively spawned whirlwind / fire-orb record writes the admit
            // mask into BOTH of its port homes; state-only, OBSERVABLE holds).
            // With that switch alone the pin is 0x5f504759165dbf93.
            // RE-PINNED 2026-11-22: the dev-spells toggle re-stamps the held book (`World::set_dev_spells` — the seeded spells pick up the instrument's free gates when it goes on; was 0x5aa1d00924cc72ab). `if false` on that loop restores the old pin.
            0xa9ba31fcefa67816, // was 0x5f504759165dbf93 / 0x06f940b92d340c8b / 0xb81708d1a4f89892 (2026-09-17); 0x7387656e3a5ccc62 (2026-09-15, see C)
        ],
        "cave goldens moved — re-pin ONLY for an intended fidelity change"
    );
    // Re-pinned (CHECKPOINTS C AND D; A/B hold) for THE FIRING HAND
    // MOVING TO THE CASTER. Retail keeps it in the caster's own flag
    // dword (`sub_5F7B0`: `byte[1] &= 0xFC; …dword |= 256|512`,
    // EF:60977-78) and `sub_68E50` (EF:55595) reads it back there at
    // every spawn; the port kept the same information on the class-15
    // MANIFESTATION as `f50` = 256/512 — a field retail leaves dead
    // for class 15 (`import_ent_mc2` zeroes it outright), so the port
    // was writing a port-only register into the hashed entity lane.
    // The muzzle now reads the shared `hand_bits`, and the token
    // write is gone. NOTHING ELSE MOVED: the three launches this
    // world makes all resolve to the same side before and after
    // (traced: `f50=0 old_right=false`, `hand_bits=0x100
    // side=Some(false)`), so the delta is exactly the dropped write.
    // A/B-ATTRIBUTED: restoring ONLY `ent[m].f50 = 256/512` at the two
    // arm sites reproduces the previous pin exactly with the muzzle
    // rewire and the three-way `sub_68E50` side still in.
    // Corpus receipt: mc2l3 pair-mode UNEXPLAINED pairs 567 → 90 (the
    // whole (9,1) possession-birth lane, 479 pairs), every free-run
    // horizon in the corpus byte-identical.
    // RE-PINNED (checkpoint D only; A/B/C byte-identical): the
    // (10,77) fire-sphere satellites now inherit the hub's
    // actSpeed/minSpeed/maxSpeed triple, because retail builds each
    // satellite with `qmemcpy(entity2, entity, sizeof(...))` —
    // EF:35967, a full struct clone of the hub — where the port built
    // them field-by-field and left `new_event`'s defaults
    // (+126 = 16, +128 = +130 = 0). Level-014 authors two
    // disposition-5 (10,76) firestorms and this test fires
    // dispositions 1..=64, so 50 satellites × 3 hashed fields move.
    // A/B-ATTRIBUTED: reverting ONLY those three writes reproduces the
    // previous pin exactly, with the mc2l0 death-phase law of the same
    // round still in — so that law is A/B-EXCLUDED here.
    // Corpus receipt: mc2l0-spells-galore free horizon 4397 → 5889.

    // The layout-INDEPENDENT companion golden — see state_hash.rs:
    // survives hashed-layout re-pins; moves ONLY with real behavior.
    //
    // The m9 grounded-arm fix moves the LAST checkpoint only, and that
    // is the correct signal: a hive that no wizard has approached now
    // squats and feeds in place instead of cycling back into a 400-tick
    // walk, so late-run hive positions genuinely differ. The first
    // three hold — the divergence needs ~400 asleep ticks to appear.
    //
    // The ApplyEvents load settle moves ALL FOUR — a REAL behavior
    // change by design: the level's 9 authored scorch rings finish
    // their dig before tick 0 (terrain plane differs at every
    // checkpoint) and the load RNG phase shifts every spawn after it.
    // The mc2:04 battle fixes (m9 castle scan + dead-watch scrub)
    // move the LAST checkpoint only — real behavior in the
    // disposition-storm phase, same signal as the hash pin above.
    //
    // The bound-watch law moves the last checkpoint only, in BOTH
    // directions it has taken: under fire-at-bind the released m18
    // wandered from tick 1; under the AUTHORED death-watch law
    // (player-ruled 2026-07-25, current) the m18 stays HELD — either
    // way the observable divergence needs the long disposition-storm
    // phase to accumulate, and the earlier state-hash moves are
    // RNG-phase and hold-table internals, which this projection is
    // blind to by design.
    // The traced sphere mover (see the golden pin above) moves B-D
    // and HOLDS the load checkpoint: settled spheres freeze at their
    // landing pose (no eternal re-merge/re-roll), the rebound zeroes
    // at ≤16, and the rotation quad stamps on re-sprite — all
    // observable sphere state downstream of ~a settle window.
    // The corpus-solved cave rand structure (see the golden pin
    // above) moves the LAST checkpoint only: the re-phased stream
    // needs the disposition-storm phase for its spawn/motion
    // downstream to reach the projection.
    // The merge law (see the golden pin above) moves B-D and HOLDS the
    // load checkpoint: which drips coalesce is observable sphere
    // state, and the donor's slot returning a tick earlier re-phases
    // every later spawn in the pool — the load checkpoint predates any
    // merge.
    // The 180° turn tie-break law (see the golden pin above) moves
    // B-D and HOLDS the load checkpoint: an antipodal wander turn now
    // commits in retail's direction, and creature poses diverge from
    // the first tie on — real behavior, not layout.
    // The load-time spawn datum (see the golden pin above) moves ALL
    // FOUR, including the load checkpoint — as it must: every cave
    // sculptor now carves a cell-centred cone at retail's tile, so
    // the terrain plane this projection hashes differs before tick 0,
    // and every walker standing on it is placed differently from the
    // first tick. Real behavior, measured against retail's own t=0
    // planes (mc2l3).
    // 4th re-pinned for the sphere vertical-law change (strict
    // below-ground clamp, EF:26244): a sphere landing EXACTLY on the
    // floor keeps its fall lift one more tick, so its free-running z
    // phase shifts one tick toward retail's — the same family the
    // mc1l0 replay corpus pins at t=2. Per-pair conformance
    // observables are unchanged (all 9 suites green across the
    // change); only free-running evolutions move.
    // 2nd-4th re-pinned with the state pins above (the AREA-BROADCAST
    // TILE ROUNDING, `area_write` — MC2 twin EF:3750/3798): edge-tile
    // victims now receive their mail on retail's tick; the mc2l0
    // t=7257 fixture went conforming on the same change.
    // LAST re-pinned with the state pin above (the SCORCH DISC): a
    // real behavior change — each flame scorches the ring-0 THREE-cell
    // disc (center, +x, +y) with the full cell update, so late-run
    // terrain, its ceiling counter-shifts and everything
    // ground-following them genuinely move. First three hold.
    // LAST re-pinned again with the M3 HEAD PARTICLE-TABLE read: a
    // real behavior change and the point of the law — a head whose
    // pitch/roll box was 0 collided with nothing, so the cave's
    // multipart flyer only becomes hittable at the last checkpoint's
    // depth. First three hold, which is itself the attribution (the
    // other four laws of the round move NOTHING here; each was
    // reverted alone against the PREVIOUS layout pin, so the
    // observable assert was actually reached — the 23c trap).
    // The sphere cave-ceiling clamp (see the golden pin above) moves
    // the LAST checkpoint only, on BOTH pins — the correct signal: a
    // sphere's z under the rock is observable sphere state, and the
    // divergence needs the disposition storm to throw one high enough
    // to reach the roof. Same A/B attribution and same exclusions.
    // THE SHADOW BACK-OFF (dig 98-Q30's attribution of the long-red
    // layout pin above — `MGC_NO_SV_BACKOFF`, `sub_1D8C0`'s fourth
    // step, byte-verified at `NETHERW.EXE` 0x42345-0x4239D) moves
    // B, C and D here and HOLDS the load checkpoint — exactly the
    // correct signal, and the reason this projection exists: an escort
    // that closes on its charge now peels away instead of walking
    // through it, so held-creature poses and positions genuinely
    // differ from the first back-off on, while A predates any of them.
    // A/B-ATTRIBUTED (ONE binary md5 ac3a8db27fbcf73be74b1849f4c08b98,
    // back to back): `MGC_NO_SV_BACKOFF=1` ALONE reproduces the
    // previous OBSERVABLE pin EXACTLY on all four.
    // A/B-EXCLUDED, and this is the useful half: round 98's MC2 SPRITE
    // STAMP (`MGC_NO_MC2_FRAMES89` + `MGC_NO_MC2_FRAME_CAP`), which
    // moves ALL FOUR of the layout hashes above, moves NOTHING here —
    // both arms are byte-identical. That is the independent proof that
    // the stamp is a hashed-field move and not behaviour.
    // ⚠ C AND D RE-PINNED HERE TOO 2026-09-06 (ROUND 108) — and unlike
    // most re-pins above, these two are NOT hashed-field moves: both
    // are real behaviour. C is the cave-in's survival pocket actually
    // digging for the first time (the port carved no pocket at all
    // before, because model 0 is the out-of-pool human); D is round
    // 107's creature-seam family, proved by bisecting round 107's
    // commit against its parent `a913653`. A/B are untouched, which is
    // the evidence that neither change reached the pre-collapse world.
    // ⚠ ALL FOUR RE-PINNED 2026-09-06 (ROUND 109) — real behaviour, not a
    // hashed-field move: the (10,31) river chains now CARVE (level-014
    // has them), and the load settle allocates its spawns in retail's
    // slot order (in-sweep free). Each law reverted alone moves all
    // four away from both the previous pin (0xca0e5c449cf57b10 /
    // 0xb60c271e559caa80 / 0x478ffb68996ab8ea / 0x9d0d9120f359f775) and
    // HEAD — see the state-pin note above for the arm values.
    // Re-pinned 2026-09-06 (ROUND 112, THE NATIVE HUMAN RECORD): the
    // world now pops the human's pool slot before the book's tokens,
    // like retail's `sub_5C950`, so every later record sits one slot
    // higher — all four LAYOUT hashes move (slot numbering from load
    // on), checkpoints A/B/C of the OBSERVABLE projection HOLD
    // BYTE-FOR-BYTE, and only D (the disposition storm) moves: the
    // storm's spawns land one slot higher and every per-entity stream
    // (`rand = slot + global`, `f63`) re-phases — real behaviour toward
    // retail's layout, the same class as round 111's mc2_slice re-pin.
    // A/B: `MGC_NO_MC2_NATIVE_HUMAN_RECORD=1` reproduces the previous
    // pin (0x921f2692901c8c49 / 0x548f26fbb5411d18 / 0xaba02729e2083cca /
    // 0x5b1a79824eb6d514; OBSERVABLE D 0x7c5397293f30a730) exactly.
    // OBSERVABLE D re-pinned 2026-09-13 (A/B/C hold BYTE-FOR-BYTE)
    // alongside the state golden's checkpoint D, for THE KIND-1
    // ARRIVAL-BOX SEAM (`sub_12500`, NETHERW.EXE VA 0x125DE — two
    // `movswl` and a 32-bit `sub` where the port wrapped in 16 bits;
    // the sibling of the kind-2 leash box at VA 0x1DC14). A stage-hold
    // release that fires early changes WHEN a held creature starts
    // acting, which is a behaviour change and so is expected to move
    // the observable projection as well as the raw state — this
    // assertion's own standard.
    // A/B-ATTRIBUTED IN THE MAIN SESSION: `MGC_NO_MC2_KIND1_SEAM=1`
    // makes the whole test pass on its previous pins (state D
    // 0x0a717b6bbb39a15f, OBSERVABLE D 0x9e18c00f22c0123c) with this
    // session's other seven laws still in.
    const OBSERVABLE: [u64; 4] = [
        0x536144e2ca06878d,
        0x4cfe444e3d74d9d1,
        0x674d00718d9a0d5b,
        0xc8d9d776de5c8373, // was 0xe733f7c078ba072c — 2026-09-17, `MGC_NO_MC2_NATIVE_TICKTOP_REAP`, since 2026-09-18 the retail arm of the `mc2_immediate_reap` patch (round 148)
    ];
    assert_eq!(
        obs, OBSERVABLE,
        "the OBSERVABLE projection diverged — this is a behavior \
         change, never a layout-only one"
    );
}

/// The wall-hug eye band: drive the faithful MC2 mover straight into a
/// sealed cave wall and pin that the eye NEVER leaves the mover's
/// clamp band — >= floor+256 and <= ceiling-384 against the
/// INTERPOLATED surfaces the renderer draws (mesh == collision: same
/// corner heights, same parity diagonals). This exonerates the
/// vertical clamps for the wall-peek x-ray: the residual vector is the
/// near plane cutting a hugged steep face LATERALLY, which the terrain
/// shader's backface-black arm paints as rock instead of x-raying the
/// far chamber.
#[test]
fn mc2_cave_wall_hug_holds_the_clamp_band() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(w) = build_world(&root) else {
        eprintln!("skipping: no ceiling plane");
        return;
    };
    // A wall approach: sealed tile at (x, zw), 6 open roomy tiles
    // straight south of it.
    let (p, c) = (w.planes(), w.ceiling_plane().to_vec());
    let mut approach = None;
    'scan: for zw in 8..240usize {
        for x in 8..240usize {
            let sealed = p.angle[zw * 256 + x] & 8 != 0;
            if !sealed {
                continue;
            }
            let ok = (1..=6).all(|d| {
                let t = (zw + d) * 256 + x;
                p.angle[t] & 8 == 0 && c[t] as i32 - p.height[t] as i32 > 24
            });
            if ok {
                approach = Some((x, zw));
                break 'scan;
            }
        }
    }
    let Some((x, zw)) = approach else {
        eprintln!("no wall approach found");
        return;
    };
    eprintln!("approach: wall at ({x},{zw}), corridor south");

    let mut sim = mgc_sim::Simulation::with_world(w);
    let fx = x as f32 + 0.5;
    let fz = zw as f32 + 5.5;
    let g0 = sim.world.as_ref().unwrap().ground_height_tiles(fx, fz);
    sim.flyer[Hx::FIRST].x = fx;
    sim.flyer[Hx::FIRST].z = fz;
    sim.flyer[Hx::FIRST].y = g0 + 1.5;
    sim.flyer[Hx::FIRST].yaw = 0.0; // -Z: straight at the wall
    sim.flyer[Hx::FIRST].pitch = 0.0;
    sim.sync_carpet_from_flyer(Hx::FIRST);

    let mut worst_floor = f32::MAX;
    let mut worst_ceil = f32::MAX;
    for _ in 0..140 {
        sim.step(&mgc_sim::FlightInput {
            thrust: 1.0,
            ..Default::default()
        });
        let f = sim.flyer[Hx::FIRST];
        let ex = ((f.x.rem_euclid(256.0)) * 256.0) as u16;
        let ez = ((f.z.rem_euclid(256.0)) * 256.0) as u16;
        let eye = f.y * 256.0;
        let w = sim.world.as_ref().unwrap();
        let floor = w.ground_z_engine(ex, ez) as f32;
        let ceil = w.player_cave_ceiling(ex, ez).unwrap() as f32 + 384.0;
        let (df, dc) = (eye - floor, ceil - eye);
        worst_floor = worst_floor.min(df);
        worst_ceil = worst_ceil.min(dc);
    }
    // The retail clamps: floor+256 (EF:59768) / ceiling-384
    // (EF:59758-63); 1.0 slop for the f32 round-trip.
    assert!(
        worst_floor >= 255.0,
        "eye dipped under floor+256 while wall-hugging (worst {worst_floor:.1})"
    );
    assert!(
        worst_ceil >= 383.0,
        "eye rose over ceiling-384 while wall-hugging (worst {worst_ceil:.1})"
    );
}

/// The ENHANCED-mover funnel squeeze: the deviation mover needs a cave
/// narrow-space law — without one, nothing refuses entry into the seam
/// where floor meets ceiling and a floor-wins pinch clamp hoists the
/// head THROUGH the diving ceiling. With the squeeze gate (the
/// faithful gate's sub_11E20 predicate) the eye must stay under the
/// interpolated ceiling for the whole approach.
#[test]
fn mc2_cave_enhanced_funnel_never_breaches_ceiling() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(w) = build_world(&root) else {
        eprintln!("skipping: no ceiling plane");
        return;
    };
    let (p, c) = (w.planes(), w.ceiling_plane().to_vec());
    // A FUNNEL: an OPEN (unsealed) pinch tile — air band a few height
    // bytes, far under the mover's 0.75-tile floor clearance — with a
    // roomy open corridor leading in. This is the mc2:03 shape: no
    // sealed tile ever stops the approach, the band just narrows.
    let mut approach = None;
    'scan: for zw in 8..240usize {
        for x in 8..240usize {
            let t0 = zw * 256 + x;
            let band0 = c[t0] as i32 - p.height[t0] as i32;
            if p.angle[t0] & 8 != 0 || band0 <= 0 || band0 > 5 {
                continue;
            }
            let ok = (1..=6).all(|d| {
                let t = (zw + d) * 256 + x;
                let band = c[t] as i32 - p.height[t] as i32;
                p.angle[t] & 8 == 0 && band > if d >= 3 { 20 } else { 4 }
            });
            if ok {
                approach = Some((x, zw));
                break 'scan;
            }
        }
    }
    let Some((x, zw)) = approach else {
        eprintln!("no funnel approach found on level-014");
        return;
    };
    eprintln!("funnel: pinch at ({x},{zw}), corridor south");

    let mut sim = mgc_sim::Simulation::with_world(w);
    sim.thrust_model[Hx::FIRST] = mgc_sim::ThrustModel::Enhanced;
    // Hug the pinch CORNER (the height/ceiling bytes live on tile
    // corners): the tile-center line interpolates away from the
    // narrowest point and misses the squeeze.
    let fx = x as f32 + 0.05;
    let fz = zw as f32 + 5.5;
    let g0 = sim.world.as_ref().unwrap().ground_height_tiles(fx, fz);
    sim.flyer[Hx::FIRST].x = fx;
    sim.flyer[Hx::FIRST].z = fz;
    sim.flyer[Hx::FIRST].y = g0 + 1.5;
    sim.flyer[Hx::FIRST].yaw = 0.0; // -Z: straight into the wall
    sim.flyer[Hx::FIRST].pitch = 0.0;
    sim.sync_carpet_from_flyer(Hx::FIRST);

    let mut worst: f32 = f32::MAX; // min (ceiling - eye), engine units
    for _ in 0..250 {
        sim.step(&mgc_sim::FlightInput {
            thrust: 1.0,
            ..Default::default()
        });
        let f = sim.flyer[Hx::FIRST];
        let ex = ((f.x.rem_euclid(256.0)) * 256.0) as u16;
        let ez = ((f.z.rem_euclid(256.0)) * 256.0) as u16;
        let w = sim.world.as_ref().unwrap();
        // player_cave_ceiling = interpolated ceiling − 384.
        let ceil = w.player_cave_ceiling(ex, ez).unwrap() as f32 + 384.0;
        worst = worst.min(ceil - f.y * 256.0);
    }
    assert!(
        worst > 0.0,
        "the enhanced carpet's head breached the cave ceiling \
         (worst ceiling-eye = {worst:.1} engine units)"
    );
}

/// The enhanced LIFT reaches retail's own cave ceiling: ground+3072,
/// cave row 104's band (player ruling 2026-09-25 — retail constants
/// set the lift's range; a same-day 1024 cave cap was reverted), and
/// never through the roof. Both thrust models: they reach the
/// desired-altitude law through the same shared tail.
#[test]
fn mc2_cave_enhanced_lift_reaches_the_retail_cave_ceiling() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    for thrust in [mgc_sim::ThrustModel::Mc1, mgc_sim::ThrustModel::Enhanced] {
        let Some(w) = build_world(&root) else {
            eprintln!("skipping: no ceiling plane");
            return;
        };
        let (p, c) = (w.planes(), w.ceiling_plane().to_vec());
        // A tall open chamber: a 5x5 corner patch of flat-ish floor
        // with more headroom than 3072 + the 384 roof margin, so
        // only the lift band can stop the climb.
        let mut spot = None;
        'scan: for zw in 8..240usize {
            for x in 8..240usize {
                let t0 = zw * 256 + x;
                let ok = (0..5).all(|dz| {
                    (0..5).all(|dx| {
                        let t = (zw + dz) * 256 + x + dx;
                        p.angle[t] & 8 == 0
                            && c[t] as i32 - p.height[t] as i32 > 115
                            && (p.height[t] as i32 - p.height[t0] as i32).abs() <= 2
                    })
                });
                if ok {
                    spot = Some((x, zw));
                    break 'scan;
                }
            }
        }
        let Some((x, zw)) = spot else {
            panic!("no tall chamber found on level-014");
        };
        let mut sim = mgc_sim::Simulation::with_world(w);
        sim.thrust_model[Hx::FIRST] = thrust;
        sim.set_altitude_model(Hx::FIRST, mgc_sim::AltitudeModel::ExtendedLift);
        let (fx, fz) = (x as f32 + 2.0, zw as f32 + 2.0);
        let g0 = sim.world.as_ref().unwrap().ground_height_tiles(fx, fz);
        sim.flyer[Hx::FIRST].x = fx;
        sim.flyer[Hx::FIRST].z = fz;
        sim.flyer[Hx::FIRST].y = g0 + 1.0;
        sim.flyer[Hx::FIRST].pitch = 0.0;
        sim.sync_carpet_from_flyer(Hx::FIRST);
        sim.set_altitude_model(Hx::FIRST, mgc_sim::AltitudeModel::Faithful);
        sim.set_altitude_model(Hx::FIRST, mgc_sim::AltitudeModel::ExtendedLift);
        for _ in 0..200 {
            sim.step(&mgc_sim::FlightInput {
                lift: 1.0,
                ..Default::default()
            });
        }
        let f = sim.flyer[Hx::FIRST];
        let g = sim.world.as_ref().unwrap().ground_height_tiles(f.x, f.z);
        let over = (f.y - g) * 256.0;
        eprintln!("{thrust:?}: chamber ({x},{zw}), held {over:.1} over ground");
        let (ex, ez) = ((f.x * 256.0) as u16, (f.z * 256.0) as u16);
        let roof = sim
            .world
            .as_ref()
            .unwrap()
            .player_cave_ceiling(ex, ez)
            .unwrap() as f32;
        let want = 3072f32.min(roof - g * 256.0);
        assert!(
            want > 2048.0,
            "the chamber is tall enough to tell 3072 from 1024"
        );
        assert!(
            (over - want).abs() <= 2.0,
            "{thrust:?}: the cave lift must reach min(ground+3072, roof) = {want}, held {over:.1}"
        );
    }
}

/// Flying into a cave wall under Speed kills the spell (retail's
/// `moveTest_5D0A0` refusal → `zero_speed` → the token cancel,
/// EF:59602) — under BOTH control modes. The enhanced controls used
/// to keep the boost and shove into the wall until it expired (parity
/// audit 2026-09-25); they now share retail's gate.
#[test]
fn mc2_cave_wall_cancels_speed_under_both_controls() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    for thrust in [mgc_sim::ThrustModel::Mc1, mgc_sim::ThrustModel::Enhanced] {
        let Some(w) = build_world(&root) else {
            eprintln!("skipping: no ceiling plane");
            return;
        };
        let (p, c) = (w.planes(), w.ceiling_plane().to_vec());
        let open = |x: usize, z: usize| {
            let t = z * 256 + x;
            p.angle[t] & 8 == 0 && c[t] as i32 - p.height[t] as i32 > 60
        };
        let sealed = |x: usize, z: usize| p.angle[z * 256 + x] & 8 != 0;
        // A corridor ending in a broad sealed wall to the north (-Z):
        // four open tiles in a column, a 7-wide sealed face beyond.
        let mut spot = None;
        'scan: for zw in 12..240usize {
            for x in 8..240usize {
                let run = (0..4).all(|d| (0..3).all(|dx| open(x + dx - 1, zw + d)));
                let wall =
                    (0..7).all(|dx| sealed(x + dx - 3, zw - 1) && sealed(x + dx - 3, zw - 2));
                if run && wall {
                    spot = Some((x, zw));
                    break 'scan;
                }
            }
        }
        let Some((x, zw)) = spot else {
            panic!("no sealed cave wall found on level-014");
        };
        let mut sim = mgc_sim::Simulation::with_world(w);
        sim.thrust_model[Hx::FIRST] = thrust;
        let (fx, fz) = (x as f32 + 0.5, zw as f32 + 3.5);
        let g0 = sim.world.as_ref().unwrap().ground_height_tiles(fx, fz);
        sim.flyer[Hx::FIRST].x = fx;
        sim.flyer[Hx::FIRST].z = fz;
        sim.flyer[Hx::FIRST].y = g0 + 1.5;
        sim.flyer[Hx::FIRST].yaw = 0.0; // -Z: straight at the wall
        sim.flyer[Hx::FIRST].pitch = 0.0;
        sim.sync_carpet_from_flyer(Hx::FIRST);
        sim.world
            .as_mut()
            .unwrap()
            .debug_arm_mc2_speed(Hx::FIRST, 3);
        let fly = mgc_sim::FlightInput {
            thrust: 1.0,
            ..Default::default()
        };
        sim.step(&fly);
        assert!(
            sim.world
                .as_ref()
                .unwrap()
                .accel_override(Hx::FIRST)
                .is_some(),
            "{thrust:?}: the boost is live before the wall"
        );
        let mut hit = None;
        for t in 0..120 {
            sim.step(&fly);
            if sim
                .world
                .as_ref()
                .unwrap()
                .accel_override(Hx::FIRST)
                .is_none()
            {
                hit = Some(t);
                break;
            }
        }
        eprintln!(
            "{thrust:?}: wall at ({x},{}), boost cancelled at {hit:?}",
            zw - 1
        );
        assert!(hit.is_some(), "{thrust:?}: the cave wall must cancel Speed");
    }
}

/// The mc2:23 spawn-embedded-in-rock case: level-023's (3,4) wizard
/// start at (134,47) lies in baked-sealed
/// rock — the entry cavern is carved at LOAD by an authored (10,82)
/// room at (127,47) with par extents (58,42) and depth par3 = 9
/// (PrepareEvents case 0x52, EV:373-379). Pin that the load settle
/// leaves the start tile (and a 3×3 ring around it) open cave with
/// real headroom, so the port never regresses to the ctor-default
/// 6×6 carve that left the player inside the wall.
/// ROUND 109 — mc2:15's LAVA MOATS. The two (10,31) river chains
/// (THING slots 50-54 and 268-272, closed rectangles inside the two
/// (10,82) mesa rooms at (215,182)) spawn one (10,32) river head per
/// leg (`sub_487D0` EV:5558, `mc2_stamp_river_leg`), and the head
/// (`sub_344A0`, action 0x22) drops a (10,11) scorch ring per tile
/// whose −3/tick disc dig carves the channel to floor 0 — which the
/// cave retile paints as the lava types 36/37/39. Two laws, both
/// A/B-proved against retail's measured planes (`terrain-diff` on a
/// `slice --from 120 --to 126`, port settle 130: ALL FIVE PLANES MATCH
/// on 65,536 cells):
/// 1. the river arm itself — before it the port held 26/22/20 cells of
///    types 36/37/39 (the trace doc's §3.4 called the consumer a stub
///    after reading `4A190(.., 10, 32)` as hex; retail spawns (10,32));
/// 2. `mc2_apply_events` frees a finished record INSIDE the sweep, like
///    retail's per-arm `sub_57F20` — deferring it to the sweep's end
///    let a river head's re-popped ring tick one sweep early and over-
///    dug the rims: floor-0 cells 1,800 against retail's 1,747.
///
/// Retail record 0 counts (whole plane): type 36 = 464, 37 = 125,
/// 39 = 127, floor 0 = 1,747.
#[test]
fn mc2_level_015_lava_moats_are_carved() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let Some((mut w, _)) = build_world_level(&root, "mc2/level-015.mgcl") else {
        common::golden_skip("mc2 level-015 has no baked ceiling");
        return;
    };
    let (sx, sy) = open_spot(&w);
    // The (10,45) building lerps still run over the first ~40 ticks;
    // the lava is a LOAD product (present at tick 0) and unmoved by
    // them. Settle past the buildings so the whole-plane counts are
    // the retail record's.
    hover(&mut w, sx, sy, 60, PlayerCommand::default());
    let p = w.planes_clone();
    let count_type = |t: u8| p.tile_type.iter().filter(|&&v| v == t).count();
    assert_eq!(
        (count_type(36), count_type(37), count_type(39)),
        (464, 125, 127),
        "mc2:15 lava types 36/37/39 (retail 464/125/127; 26/22/20 before the (10,31) river arm)"
    );
    assert_eq!(
        p.height.iter().filter(|&&h| h == 0).count(),
        1747,
        "mc2:15 floor-0 cells (retail 1,747; 887 before the river arm, 1,800 with the \
         settle's free deferred to the sweep end)"
    );
    // The outer moat's top leg: (189,157)→(240,157), a channel at
    // floor 0 (rows 156..158 everywhere along it; row 159 carries a
    // stray 1 at x=220 in retail too) with the 88 mesa plateau below.
    for x in [200u8, 220, 236] {
        for y in 156..=158u8 {
            assert_eq!(
                p.height[y as usize * 256 + x as usize],
                0,
                "channel cell ({x},{y})"
            );
        }
        assert_eq!(
            p.height[162 * 256 + x as usize],
            88,
            "plateau cell ({x},162)"
        );
    }
}

#[test]
fn mc2_level_023_start_chamber_is_carved_open() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked mc2 data");
        return;
    };
    if !root.join("mc2/level-023.mgcl").exists() {
        eprintln!("skipping: no baked level-023");
        return;
    }
    let (w, pkg) = build_world_level(&root, "mc2/level-023.mgcl").unwrap();
    let start = pkg
        .things
        .things
        .iter()
        .find(|t| t.kind == mgc_formats::ThingKind::Entity && t.class == 3 && t.model == 4)
        .expect("level-023 authors the (3,4) start marker");
    let (sx, sy) = (start.x as usize, start.y as usize);
    let p = w.planes();
    let c = w.ceiling_plane();
    for dy in 0..3 {
        for dx in 0..3 {
            let t = (sy + dy - 1) * 256 + (sx + dx - 1);
            assert!(
                p.angle[t] & 8 == 0,
                "start ring tile ({},{}) still sealed",
                sx + dx - 1,
                sy + dy - 1
            );
            assert!(
                c[t] as i32 - p.height[t] as i32 > 20,
                "start ring tile ({},{}) has no headroom (floor {} ceiling {})",
                sx + dx - 1,
                sy + dy - 1,
                p.height[t],
                c[t]
            );
        }
    }
}
