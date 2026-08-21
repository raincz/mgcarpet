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
    w.set_dev_spells(true);
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
    w.set_dev_spells(true);
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
    assert_eq!(
        got,
        vec![
            0x30a7039ebee444b7u64,
            0xeb019cfa3c5ee5d8,
            0xb3e17b8aaa6b4d89,
            0x1198b51aca429fcc,
        ],
        "cave goldens moved — re-pin ONLY for an intended fidelity change"
    );

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
    const OBSERVABLE: [u64; 4] = [
        0xca0e5c449cf57b10,
        0x65bac868017c2757,
        0xdbbeee1a0bf108ce,
        0x73367e42e9d447a8,
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
    sim.flyer.x = fx;
    sim.flyer.z = fz;
    sim.flyer.y = g0 + 1.5;
    sim.flyer.yaw = 0.0; // -Z: straight at the wall
    sim.flyer.pitch = 0.0;
    sim.sync_carpet_from_flyer();

    let mut worst_floor = f32::MAX;
    let mut worst_ceil = f32::MAX;
    for _ in 0..140 {
        sim.step(&mgc_sim::FlightInput {
            thrust: 1.0,
            ..Default::default()
        });
        let f = sim.flyer;
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
    sim.thrust_model = mgc_sim::ThrustModel::Enhanced;
    // Hug the pinch CORNER (the height/ceiling bytes live on tile
    // corners): the tile-center line interpolates away from the
    // narrowest point and misses the squeeze.
    let fx = x as f32 + 0.05;
    let fz = zw as f32 + 5.5;
    let g0 = sim.world.as_ref().unwrap().ground_height_tiles(fx, fz);
    sim.flyer.x = fx;
    sim.flyer.z = fz;
    sim.flyer.y = g0 + 1.5;
    sim.flyer.yaw = 0.0; // -Z: straight into the wall
    sim.flyer.pitch = 0.0;
    sim.sync_carpet_from_flyer();

    let mut worst: f32 = f32::MAX; // min (ceiling - eye), engine units
    for _ in 0..250 {
        sim.step(&mgc_sim::FlightInput {
            thrust: 1.0,
            ..Default::default()
        });
        let f = sim.flyer;
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

/// The mc2:23 spawn-embedded-in-rock case: level-023's (3,4) wizard
/// start at (134,47) lies in baked-sealed
/// rock — the entry cavern is carved at LOAD by an authored (10,82)
/// room at (127,47) with par extents (58,42) and depth par3 = 9
/// (PrepareEvents case 0x52, EV:373-379). Pin that the load settle
/// leaves the start tile (and a 3×3 ring around it) open cave with
/// real headroom, so the port never regresses to the ctor-default
/// 6×6 carve that left the player inside the wall.
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
