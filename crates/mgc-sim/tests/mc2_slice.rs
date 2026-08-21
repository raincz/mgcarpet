//! The MC2 vertical slice on real level-000 data under the full MC2
//! profile. Every criterion is POSITIVELY exercised: creatures are
//! found via the pool debug view, the player is parked next to them,
//! and the observable is asserted per model — Goat wakes/flees/dies
//! (mana sphere + kill credit), Archers stand and FIRE (arrow entity +
//! danger music), Villager wanders and never attacks, the type-5
//! fly-to objective latches at its authored point.
//!
//! Golden hashes pin the slice (the MC1 goldens in state_hash.rs are
//! untouched — the columns share the chassis, not the fixtures).
//! Self-skips without baked mc2 data.

use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::ids::GameId;
use std::path::PathBuf;

#[path = "common/mod.rs"]
mod common;

fn baked_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    (p.join("mc2/level-000.mgcl").exists()
        && p.join("assets/mc2-night/build.tab.bin").exists()
        && !common::modded_bake(&p))
    .then_some(p)
}

fn build_world(root: &std::path::Path) -> Option<World> {
    let file = std::fs::File::open(root.join("mc2/level-000.mgcl")).unwrap();
    let pkg: mgc_formats::LevelPackage = mgc_formats::mgcl::read(file).unwrap();
    let terrain = pkg.terrain.as_ref()?;
    let planes = Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone().unwrap(),
        angle: terrain.angle.clone().unwrap(),
        ceiling: terrain.ceiling.clone().unwrap_or_default(),
    };
    // The mc2-night bundle's own feature data (level-000 is a night
    // map): SEARCH + the BUILD0-0 footprint bank + BLDGPRM — the
    // building creator consumes all three (the app arranges the
    // same).
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).unwrap();
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
    // Level-000 is a NIGHT map: runtime repaints invert relief
    // shading (sub_462A0's non-day arm) — the app sets the same.
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
    Some(w)
}

fn hover(w: &mut World, x: f32, z: f32, ticks: usize, cmd: PlayerCommand) {
    for _ in 0..ticks {
        let alt = w.ground_height_tiles(x, z) + 2.0;
        w.tick(PlayerPose::from_tiles(x, alt, z, 0.0, 0.0, 0.0), cmd);
    }
}

/// Nearest live (class, model) entity's tile position from the pool
/// debug view.
fn find_creature(w: &World, class: u8, model: u8) -> Option<(f32, f32)> {
    w.debug_pool()
        .1
        .into_iter()
        .find(|e| e.class == class && e.model == model && e.life >= 0)
        .map(|e| (e.tx as f32 + 0.5, e.ty as f32 + 0.5))
}

/// The scripted slice run; returns the checkpoint hashes.
fn run(root: &std::path::Path) -> Option<(Vec<u64>, Vec<u64>)> {
    let mut w = build_world(root)?;
    let idle = PlayerCommand::default();
    let mut hashes = vec![w.state_hash()];
    let mut obs = vec![w.observable_digest()];

    // A: idle far from everything — awake pass + wander cadences.
    hover(&mut w, 16.0, 16.0, 64, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // B: the type-5 fly-to objective at (115, 212).
    hover(&mut w, 115.5, 212.5, 8, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // C: park next to a goat — awake + flee.
    if let Some((vx, vz)) = find_creature(&w, 5, 1) {
        hover(&mut w, vx + 2.0, vz, 96, idle);
    }
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // D: NATIVE fireballs at the nearest goat (the MC1 equip bridge
    // does NOT cast on the MC2 column — the seeded book's LEFT
    // fireball is the cast path). RIGHT unbinds so possession pulses
    // don't spray claims over the script; tier-0 fireball is CLICK
    // cadence, so the volley pulses the edge.
    w.set_dev_spells(true);
    if let Some((vx, vz)) = find_creature(&w, 5, 1) {
        let unbind_r = PlayerCommand {
            mc2_select: Some((255, 0, 1)),
            ..Default::default()
        };
        hover(&mut w, vx + 1.5, vz, 1, unbind_r);
        let firing = PlayerCommand {
            fire_left: true,
            ..Default::default()
        };
        for _ in 0..48 {
            hover(&mut w, vx + 1.5, vz, 1, firing);
            hover(&mut w, vx + 1.5, vz, 1, idle);
        }
    }
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // E: materialize the rest of the authored population (archers
    // sit behind dispositions the sweep never trips) and provoke
    // them: killing townsfolk arms the wizard's wanted timer.
    for dis in 1..=64 {
        w.debug_fire_disposition(dis);
    }
    if let Some((vx, vz)) = find_creature(&w, 5, 13) {
        let firing = PlayerCommand {
            fire_left: true,
            fire_right: true,
            ..Default::default()
        };
        hover(&mut w, vx + 1.0, vz, 64, firing);
    }
    if let Some((ax, az)) = find_creature(&w, 5, 4) {
        hover(&mut w, ax + 3.0, az, 160, idle);
    }
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    Some((hashes, obs))
}

#[test]
fn mc2_slice_behaviors_and_goldens() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let Some((got, obs)) = run(&root) else {
        common::golden_skip("mc2 level-000 has no baked terrain");
        return;
    };
    assert_eq!(
        (got.clone(), obs.clone()),
        run(&root).unwrap(),
        "slice is not deterministic"
    );
    println!("mc2 slice hashes: {got:#018x?}");

    // Re-run the script with behavior probes at each phase.
    let mut w = build_world(&root).unwrap();
    let idle = PlayerCommand::default();

    // The at-load buildings raise the western village during the
    // first 30 ticks (the (10,45) action) — count them before the
    // idle window, then confirm the finished state below.
    let buildings = |w: &World| {
        w.debug_pool()
            .1
            .into_iter()
            .filter(|e| e.class == 10 && e.model == 45 && e.life >= 0)
            .count()
    };
    let b0 = buildings(&w);
    assert!(b0 >= 10, "at-load buildings spawned ({b0})");

    // Class-11 switches: live INVISIBLE pool entities (never
    // billboarded, never ledgered), each carrying its record's
    // disposition id + word_10 box; the route sequence rides them.
    let switches0 = w.debug_pool().1.iter().filter(|e| e.class == 11).count();
    assert!(
        switches0 >= 20,
        "level-000's switches spawned ({switches0})"
    );
    assert!(
        !w.misfits().iter().any(|&(c, m, _)| c == 11 && m <= 3),
        "proximity switches are known things now"
    );
    assert!(
        !w.live_poses().iter().any(|p| p.class == 11),
        "switches draw nothing (invisible in retail)"
    );
    assert!(
        w.active_volumes()
            .iter()
            .filter(|v| matches!(v.kind, mgc_sim::engine::world::VolumeKind::Proximity))
            .count()
            >= 20,
        "switch boxes feed the map-triggers overlay"
    );
    assert!(
        w.active_volumes()
            .iter()
            .any(|v| matches!(v.kind, mgc_sim::engine::world::VolumeKind::Objective)),
        "stage checkpoints plot on the overlay"
    );

    // Villager wanders (and never attacks). The building creators'
    // walkable village paint FREES the townsfolk — free walkers can
    // also die authentically (die-on-water row flag on a boxed-in
    // wander), so survival is not asserted; kill CREDIT staying zero
    // is (townsfolk/construction deaths never credit).
    let yaw0: Vec<f32> = w
        .live_poses()
        .iter()
        .filter(|p| p.class == 5 && p.model == 13)
        .map(|p| p.yaw)
        .collect();
    assert!(!yaw0.is_empty(), "villagers spawned at init");
    hover(&mut w, 16.0, 16.0, 200, idle);
    let yaw1: Vec<f32> = w
        .live_poses()
        .iter()
        .filter(|p| p.class == 5 && p.model == 13)
        .map(|p| p.yaw)
        .collect();
    assert!(!yaw1.is_empty(), "villagers survive the idle window");
    assert!(
        yaw0.iter().zip(&yaw1).any(|(a, b)| (a - b).abs() > 0.05),
        "the wander cadence turned somebody"
    );
    assert_eq!(
        w.combat_stats().0,
        0,
        "nothing credited while idling afield"
    );

    // The build actions have finished: pads parked as the static
    // building (state 52) and the footprint carrying REAL building
    // ground — either a texture-band paint (sub_45DC0, types 8..=0x22)
    // or a blend-transition tile (sub_462A0 through the generated
    // building_F2CD0x; type 50 = the [3,1,1,1] corner row under this
    // pad).
    let parked = w
        .debug_pool()
        .1
        .into_iter()
        .find(|e| e.class == 10 && e.model == 45 && e.life >= 0 && e.state == 52)
        .expect("a building parked static (state 52)");
    let t = parked.ty as usize * 256 + parked.tx as usize;
    let ground = w.planes().tile_type[t];
    assert!(
        ground >= 8,
        "the building's tile painted real building ground (got {ground})"
    );

    // The herd law: every level-000 goat spawns BOUND to the kind-2
    // graze-leash StageVar (slot 3, anchor tile (53,32)) via
    // `sub_12100`'s subtype pass — state 15 (8·model+7), speed 18,
    // milling the anchor. They never free-wander or form follow-chains.
    assert!(
        w.debug_pool()
            .1
            .iter()
            .filter(|e| e.class == 5 && e.model == 1 && e.life >= 0)
            .all(|e| e.state == 15),
        "every goat is stage-held at the graze leash (state 15)"
    );

    // Goat flee: the kind-2 WIZARD WATCH (`sub_1DBF0` tail) — an
    // AWAKE leashed goat that sees a class-3 (range v_28 = 6 tiles,
    // cone-gated on ITS facing, one roll per v_26 = 32-tick cadence)
    // breaks to kind 10 → the FLEE-flagged raise (state 14, speed
    // 54). Park over the anchor: the mill sweeps every goat's cone
    // across us within a lap or two.
    let (gx, gz) = (53.5f32, 32.5f32);
    let mut fled = false;
    for _ in 0..512 {
        let alt = w.ground_height_tiles(gx, gz) + 1.0;
        w.tick(PlayerPose::from_tiles(gx, alt, gz, 0.0, 0.0, 0.0), idle);
        if w.debug_pool()
            .1
            .iter()
            .any(|e| e.class == 5 && e.model == 1 && e.state == 14)
        {
            fled = true;
            break;
        }
    }
    assert!(fled, "a goat that saw the player entered FLEE (14)");

    // ...and the RE-LEASH (`sub_12500` case 0xA): once the flee
    // drops (target ≥ v_28 away → the machine parks it back at
    // wander), the stage bind reclaims it into state 15 — retail's
    // calm-down-and-walk-home loop. Park far away and let it settle.
    let mut releashed = false;
    for _ in 0..600 {
        let alt = w.ground_height_tiles(16.0, 16.0) + 2.0;
        w.tick(PlayerPose::from_tiles(16.0, alt, 16.0, 0.0, 0.0, 0.0), idle);
        if w.debug_pool()
            .1
            .iter()
            .filter(|e| e.class == 5 && e.model == 1 && e.life >= 0)
            .all(|e| e.state == 15)
        {
            releashed = true;
            break;
        }
    }
    assert!(releashed, "the fled goat re-leashed (kind-10 -> re-hold)");

    // Materialize the kill-target archers via the AUTHORED
    // progression (no debug disposition fire): leave the start box —
    // the (11,1) leave-trigger at (74,212), box 64, releases dis 1 —
    // then complete the two narrated fly-to checkpoints; each
    // completion trips its stage-gated (11,32) switch (par1 0 → dis
    // 2, par1 1 → dis 3 = the four (5,4) archers in the drowned
    // village). Order-insensitive like retail: a gate spawning after
    // its stage completed fires on its first probe.
    w.set_dev_spells(true);
    assert!(find_creature(&w, 5, 4).is_none(), "archers gated at start");
    hover(&mut w, 150.0, 212.5, 12, idle);
    hover(&mut w, 115.5, 212.5, 12, idle);
    hover(&mut w, 194.5, 213.5, 12, idle);
    assert!(
        find_creature(&w, 5, 4).is_some(),
        "the checkpoint chain released the archers (dis 1 → stage gates → dis 3)"
    );

    // Kill an ARCHER with the fireball: model 4 earns kill credit;
    // mana 500 drops a sphere. NATIVE cast (the MC1 equip bridge does
    // NOT cast on the MC2 column): rebind the seeded fireball onto
    // LEFT, keep RIGHT unbound (no possession claims over the kill
    // loop).
    let bind_l = PlayerCommand {
        mc2_select: Some((0, 0, 0)),
        ..Default::default()
    };
    hover(&mut w, 16.0, 16.0, 1, bind_l);
    let unbind_r = PlayerCommand {
        mc2_select: Some((255, 0, 1)),
        ..Default::default()
    };
    hover(&mut w, 16.0, 16.0, 1, unbind_r);
    let firing = PlayerCommand {
        fire_left: true,
        ..Default::default()
    };
    // Fire straight DOWN from directly overhead in short volleys: MC2
    // creatures are faithfully zero-extent (the cross-column damage
    // contract), so projectiles pass through them and the kill path
    // is the explosion FIRE landing ON the cell — whose area write
    // fires ONCE per fire. Volley, let the fire burn out (a live fire
    // captures follow-up fireballs and drifts out of the z band),
    // volley again: 4 connected 250-payload drops beat the
    // archer's 1000 (docs/traces/mc2-fireball-damage.md). The
    // hands spawn ~±1 tile lateral of the carpet — park one tile east
    // so the LEFT hand's drop lands on the cell.
    'kill: for _ in 0..12 {
        let Some((ax, az)) = find_creature(&w, 5, 4) else {
            break;
        };
        let galt = w.ground_height_tiles(ax, az);
        let overhead = PlayerPose::from_tiles(
            ax + 1.0,
            galt + 2.5,
            az,
            0.0,
            -std::f32::consts::FRAC_PI_2,
            0.0,
        );
        for _ in 0..2 {
            w.tick(overhead, firing);
        }
        for _ in 0..30 {
            w.tick(overhead, idle);
            if w.combat_stats().0 > 0 {
                break 'kill;
            }
        }
    }
    let kills = w.combat_stats().0;
    assert!(kills >= 1, "the fireball killed an archer (kills {kills})");
    // The kill state waits for phase & 7 == 0 before transforming
    // (KillEntity_1C930) — give the corpse its settle ticks.
    hover(&mut w, 16.0, 16.0, 16, idle);
    assert!(
        w.live_poses()
            .iter()
            .any(|p| p.class == 10 && p.model == 39),
        "the corpse dropped a mana sphere"
    );

    // Archers: the disposition-3 survivors of the fireball above
    // still stand. Arm the wanted timer by shooting a villager
    // (townsfolk kills are EXCLUDED from kill credit), then stand in
    // range: they fire — an arrow entity exists and the danger music
    // arms.
    let kills_before = w.combat_stats().0;
    if let Some((tx, tz)) = find_creature(&w, 5, 13) {
        hover(&mut w, tx + 1.0, tz, 64, firing);
    }
    assert_eq!(
        w.combat_stats().0,
        kills_before,
        "villager kills never count (model-13 exclusion)"
    );
    // The wanted timer decays (200 ticks) while the archer's acquire
    // cadence samples only every 4 x scanPeriod = 120 ticks — keep
    // the timer armed by continuing to harass villagers from the
    // archer's side (each processed hit re-arms 200; :14561),
    // TRACKING the archer (with buildings live, the archer brain's
    // building/shrine walk states move them between shots). The
    // (9,13) arrow probe honors the class-3 target filter, so arrows
    // don't wipe the pack.
    let mut arrow_seen = false;
    let (mut ax, mut az) = find_creature(&w, 5, 4).expect("archers materialized");
    let mut ayaw = 0.0f32;
    for _ in 0..400 {
        if let Some(p) = w.live_poses().iter().find(|p| p.class == 5 && p.model == 4) {
            (ax, az, ayaw) = (p.x, p.z, p.yaw);
        }
        let vtarget = find_creature(&w, 5, 13);
        // Stand 3 tiles along the archer's FACING — the wizard scan
        // is cone-gated on the archer's yaw (sub_1BF90 :9152-95).
        let (px, pz) = (ax + 3.0 * ayaw.sin(), az - 3.0 * ayaw.cos());
        let alt = w.ground_height_tiles(px, pz) + 0.75;
        let (yaw, pitch) = match vtarget {
            Some((tx, tz)) => {
                let (dx, dz) = (tx - px, tz - pz);
                let dist = (dx * dx + dz * dz).sqrt().max(0.1);
                let galt = w.ground_height_tiles(tx, tz);
                (dx.atan2(-dz), -((alt - galt) / dist).atan())
            }
            None => (0.0, 0.0),
        };
        w.tick(PlayerPose::from_tiles(px, alt, pz, yaw, pitch, 0.0), firing);
        if w.debug_pool()
            .1
            .iter()
            .any(|e| e.class == 9 && e.model == 13)
        {
            arrow_seen = true;
            break;
        }
    }
    assert!(arrow_seen, "an archer fired an arrow at the wanted wizard");
    let alt = w.ground_height_tiles(ax + 3.0, az) + 2.0;
    let frame = w.take_audio(PlayerPose::from_tiles(ax + 3.0, alt, az, 0.0, 0.0, 0.0));
    assert!(frame.danger, "the arrow armed the danger music");

    // The objective board: the archer-unlock flight above already
    // completed both fly-to checkpoints (rows 0 and 1) and advanced
    // the cursor onto the kill objective.
    let (cur, stages) = w.mc2_objective_view();
    assert_eq!(stages.len(), 5, "level-000 registers five stages");
    assert_eq!(stages[0], (5, 2), "checkpoint 1 latched at (115, 212)");
    assert_eq!(stages[1], (5, 2), "checkpoint 2 latched at (194, 213)");
    assert!(cur > 1, "the cursor advanced past the completed fly-tos");
    assert!(!w.completed(), "three stages remain — no premature win");

    // Pinned goldens: regenerate with --nocapture on a DELIBERATE
    // behavior change and say so in the commit.
    //
    // Re-pinned for the full `ApplyEvents_498A0` load settle (EV:410-
    // 556): authored settle-band one-shots now run to completion at
    // LOAD (scorch craters pre-dug, buildings pre-built), the settle
    // steps the global LCG per pass (EV:420), settled slots are
    // reaped before dis 0, and the load mixer is muted (EF:39364-65/
    // :39430). Every checkpoint moves — the world enters play with
    // different terrain, RNG phase and pool layout by design.
    //
    // Re-pinned (E only) for the MC2 class-2 static tick fidelity
    // pass: AddStatue02_01_65040 / sub_65110 statics now stamp the
    // byte[2] |= 2 static draw bit every tick (the first port kept
    // only the snap), and the dolmen runs its AddDolmen02_02 shrine
    // sweep. A-D hold — no model-1/3 static ticks before the E
    // window's spawns. Layout-only: disabling the stamp alone
    // restores the old pin, and OBSERVABLE holds.
    // Re-pinned (all six, layout-only) for the MC1 `rival_wanted` timers
    // joining the shared Gen hash: MC2 never flags a rival wanted, so the
    // delta at every checkpoint is the new all-zero field; OBSERVABLE
    // holds below.
    // Re-pinned (A-E; post-init holds) for the held goat's idle BLEAT
    // draw (`AddGoat05_01_1F5B0` :11452): the phase-7 wrapper rolls the
    // per-entity u16 stream once EVERY held tick — the mc2l0 retail
    // corpus measured the missing draw as 95% of all per-entity rand
    // divergence. Behavior change toward retail by design.
    // Re-pinned (A-E; post-init holds) for the traced MC2 sphere
    // mover (TransformArcherToMana EF:26015, kinematics round):
    // spheres settle at the @0x39 countdown's zero (f58 = 0x80 ctor
    // seed, previously ignored under MC2), rebound zeroes at ≤16
    // (EF:26244-52), merges go grounded-only (EF:26265-69), and each
    // re-sprite stamps the per-size rotation quad (EF:26744-77).
    // Behavior change toward retail by design.
    // Re-pinned (A-E; post-init holds) for the corpus-solved cave
    // rand structure (see mc2_cave.rs): the MC2 baseline draw moved
    // to the tick top (retiring the post-pass draw), so every
    // mid-pass global-stream consumer sees the retail phase —
    // non-cave levels like this slice shift by exactly that one
    // draw. Behavior change toward retail by design (mc2l0
    // conforming pairs 167 → 240 under the same change).
    // Re-pinned (ALL SIX, post-init included) for day-sourced
    // sprite extents (Bundle::mc2_extent_dims): retail's particle
    // params derive once at boot from TMAPS0-0, so this night level
    // runs day-art extents (sprite 96 stamps f80 194 not 184; 52
    // param rows shift). Load-time stamps move post-init too.
    // Behavior change toward retail by design.
    // Re-pinned (A-E; post-init holds) for the 180° TURN TIE-BREAK
    // law (Gen::turn_sign, mc1/mobs.rs — retail keeps the raw sign
    // on an EXACT half-turn, sub_582F0 Sound.cpp:6580 / MC1 twin
    // :52664 SYNCHRONIZED): the slice's goats/villagers commit
    // antipodal wander turns in retail's direction. Behavior change
    // toward retail by design — mc1l0 0+2000 +5 conforming, mc2l0
    // 0+2000 +25 conforming.
    // Re-pinned (B-E; post-init + A hold) for the AWAKE-PASS POSE
    // PHASE: the pre-pass proximity gate reads the local player's
    // POOL entity (:64352-53 / remc2 sub_68C70), which pre-walk
    // still holds the PREV frame's carpet — the port now feeds the
    // `human_pose_prev` echo instead of this tick's pose. A wizard
    // crossing the 24-tile gate mid-tick wakes the bucket one tick
    // LATER (retail's tick). Corpus: mc1l0 replay horizon 413 → 561
    // boundaries (the t=414 worm-chain wall), 5 open exemplars
    // conforming (mc1l0 t=683/1613, mc2l4 t=621, mc2l24
    // t=616/1206). The slice's goat/flyer wake ticks shift by one.
    // Behavior change toward retail by design.
    // Re-pinned (A-E; post-init holds) for MC2's BUILDING FOOTPRINT
    // PASS — the middle pass of `sub_10C80`'s ch0 arm over the (10,45)
    // list `dword_38527` (EF:4076-4105) plus the tile scan's matching
    // exclusion (EF:4135). Level-000 is a village map, and a building
    // is tile-chained at its ANCHOR alone, so every area writer that
    // used to miss a house it stands inside now samples the BUILD00
    // footprint mask and lands. Behavior change toward retail by
    // design — mc2l4 t=2249 (an open `field:3,3:life` exemplar) went
    // conforming on the same patch.
    // Re-pinned (E ONLY; post-init + A-D hold) for the TREE-IGNITION
    // RE-LINK (`sub_57D40` EF:40306, sole call EF:62443 — the tree
    // re-heads its tile chain so its flame paints in FRONT of it).
    // ⭐ PURE BOOKKEEPING, not behavior: it moves `next20`/`prev22`,
    // which are hashed `Ent` fields, and NOTHING else — the ignition
    // branch clears the burning tree's target bit one line earlier, so
    // the re-headed tree satisfies no scan predicate, and a relink
    // preserves relative order for every other member of the tile. The
    // proof is right below: the layout-independent OBSERVABLE
    // projection does NOT move on this patch.
    // Re-pinned (ALL SIX) for the BUILDING-LIFE FIELD HOME — the
    // production rate moving from the mana word to `subSpellIndex_
    // 0x2A_42` → `f44`, and the derived mana from `maxMana_0x8C_140`
    // (f136, dead on a building) to `mana_0x90_144` → `f140`
    // (`sub_49A30` EF:32793/32796/32808; the construction finish parks
    // `life = 1000 * subSpellIndex`, EF:27291). Level-000 is a village
    // map, so every authored house moves the hashed stream from t=0.
    // ⭐ PURE BOOKKEEPING here, not behavior: the parked life comes out
    // identical in fresh play (the two words only diverge once a
    // conformance import supplies them separately) — proof right
    // below, the layout-independent OBSERVABLE projection does NOT
    // move on this patch.
    // A-E re-pinned (post-init holds) for the SCORCH DIG CELL
    // ROUNDING — `dig_scorch` rounds the cell (+128 before the
    // shift) like both retail chassis (MC1 sub_40D30 :51705-06, MC2
    // sub_572C0 EF:39722-23). The MC2 fire already GATED on the
    // rounded cell; its dig landed one cell over for upper-half
    // positions. Level-000's ambient village fires scorch from the
    // first live ticks, so every checkpoint after load moves;
    // post-init holds because the load settle's authored scorch
    // rings ride `dig_disc`, which always rounded. Verified
    // attributable by reverting the one dig_scorch line. Behavior
    // change toward retail by design.
    // A-E re-pinned for the SCORCH DISC LAW (`dig_scorch` = the ring-0
    // disc of sub_40D30 / MC2 sub_572C0: THREE cells — center, +x, +y —
    // each with the full cell update at ANY depth, 0 included, so
    // zero-depth scorches still latch + restencil/retile all three).
    // The ambient village fires scorch from the first live ticks, so
    // every checkpoint after post-init moves. Verified attributable to
    // the pair: reverting dig_scorch to the single-cell zero-skipping
    // form restores all six. Behavior change toward retail by design.
    const GOLDEN: [u64; 6] = [
        0x7633ac8b22e56968, // post-init (GenerateEvents + dis 0)
        0xe98f413166ba5e72, // A: 64 idle ticks afield
        0xffc80a25dbeb6580, // B: the type-5 fly-to latched
        // C-E re-pinned for the mc2l0 on-ramp batch (2026-08-21f;
        // attribution in mc2_cave.rs): the fireball's terrain-contact
        // move REVERT (sub_65C20 v16x) + the universal token-mana
        // copy + the impact pitch stamp move the combat checkpoints;
        // the D fireball window is the first consumer.
        0x71d2291ade3f15e3, // C: goat awake/flee window
        0xf60ca6ccde0e6313, // D: fireball combat over the goat
        // E re-pinned for the AREA-BROADCAST TILE ROUNDING
        // (`area_write` centers on the nearest tile — sub_120B0 /
        // EF:3750; corpus pins: mc1l0 t=91 tent claim, mc2l0 t=7257
        // fixture conforming): an edge-tile victim in the provocation
        // window now gets its mail on retail's tick.
        // E re-pinned again for the MAILBOX RESIDUE LAW: the player
        // damage consumer now clears the SOURCE only and leaves the
        // amount standing, exactly like the pool inbox already did
        // (MC1 :55734, MC2 `sub_5EFA0` EF:60725 — and BOTH leave it
        // armed on a fatal hit), plus the shield writes its quartered
        // value back (:55704 / EF:60684). The old code memset the
        // whole 6-channel block every tick, which no retail path does.
        // ⭐ E ONLY, and the residue is INERT for MC2 behaviour — MC2's
        // writers are all area-order, so the next write overwrites the
        // stale amount regardless; only the hashed `player_mail` word
        // moves. (It is NOT inert in MC1, which is the whole point:
        // `mail_write_single` accumulates onto it.)
        0x6ba8da4c55c391c2, // E: census + villager/archer provocation
    ];
    // Checkpoints 4-6 re-pinned for the DISPOSITION-FIRE stack
    // rebuild (see mc2_cave.rs — sub_49F90 at sub_4A1E0's top,
    // EF:32966; checkpoints 1-3 hold, the first mid-run fire is
    // between 3 and 4).
    assert_eq!(
        got, GOLDEN,
        "the MC2 slice diverged from its goldens — if DELIBERATE, \
         re-pin (--nocapture) and say so in the commit"
    );

    // The layout-INDEPENDENT companion golden — see state_hash.rs:
    // survives hashed-layout re-pins; moves ONLY with real behavior.
    // The ApplyEvents load settle moves all six — REAL behavior by
    // design (pre-dug craters, shifted load RNG phase).
    // Re-pinned (A-E) with the held goat's idle BLEAT draw — a REAL
    // behavior change toward retail (the per-entity stream feeds
    // every later flee/combat roll), certified by the mc2l0 retail
    // corpus (rand family 86947 → 7522 hits, first conforming pairs).
    // Re-pinned (A-E; post-init holds) with the traced sphere mover
    // (see the GOLDEN note above) — settled spheres freeze, rebound
    // and merge law changed, rotation quad observable.
    // Re-pinned (A-E; post-init holds) with the corpus-solved cave
    // rand structure (see the GOLDEN note above) — the tick-top
    // baseline draw re-phases every mid-pass stream consumer, a
    // REAL behavior change toward retail.
    // Re-pinned (A-E; post-init holds) for the 180° turn tie-break
    // law (see the GOLDEN note): antipodal wander turns now commit
    // in retail's direction — creature poses diverge from the first
    // tie on, real behavior, not layout.
    // Re-pinned (A-E; post-init holds) for the SCORCH DIG CELL
    // ROUNDING (see the GOLDEN note): ambient fires scorch their
    // retail cells — terrain, and everything ground-following it,
    // genuinely moves. Real behavior toward retail, not layout.
    // Re-pinned (A-E; post-init holds) for the SCORCH DISC LAW (see
    // the GOLDEN note): every scorch is the ring-0 three-cell disc
    // with full cell updates at any depth — terrain latches, retiles
    // and craters land on retail's cells from the first ambient fire.
    // Re-pinned (E ONLY; post-init..D hold) for the FIREBALL
    // TERRAIN-CONTACT REVERT (sub_65C20's v16x commit — the burst
    // parks at the PRE-move x/y with the contact z, mc2l0 t=2817):
    // the provocation window's fireball lands one step short of the
    // old endpoint, so its fire/scorch — and everything downstream —
    // genuinely moves. Real behavior toward retail, not layout.
    const OBSERVABLE: [u64; 6] = [
        0x5951c95adf7436f9,
        0x3eaed2073972a99e,
        0x832f419cb3f9716b,
        0xad0f895abf178c2b,
        0x55dc4df57cc26a90,
        0x2914d2e5dab5b8d3,
    ];
    assert_eq!(
        obs, OBSERVABLE,
        "the OBSERVABLE projection diverged — this is a behavior \
         change, never a layout-only one"
    );
}

/// Level-000's authored mission chain, end to end: fly-to rows 0/1 →
/// archers (dis 3) → kill them → row 2 latches (only while CURRENT —
/// the type-7 cursor gate) and the m17 kill switch drops the (15,3)
/// spell jar → row 3 (type 0: castle + 15% banked share; forced here
/// — the banked economy is pending) → the m32 row-3 watcher fires
/// dis 6 = FIVE (5,19)
/// fireflies while row 4 arms → killing the wave completes the
/// level. The m32 ObjectiveDone_2 pause keeps rows 2/4 from
/// latching vacuously in the one-tick gap before their targets
/// spawn.
#[test]
fn mc2_level000_mission_chain() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        common::golden_skip("mc2 level-000 has no baked terrain");
        return;
    };
    let idle = PlayerCommand::default();
    let count = |w: &World, class: u8, model: u8| {
        w.debug_pool()
            .1
            .into_iter()
            .filter(|e| e.class == class && e.model == model && e.life >= 0)
            .count()
    };

    // Fly the route with hops so the switch cascade keeps pace.
    for (x, z) in [
        (77.5, 222.5),
        (90.0, 214.0),
        (105.0, 212.5),
        (115.5, 212.5), // row 0
        (140.0, 212.5),
        (165.0, 212.5),
        (185.0, 213.0),
    ] {
        hover(&mut w, x, z, 16, idle);
    }
    hover(&mut w, 194.5, 213.5, 32, idle); // row 1 (the spire)
    let (_, stages) = w.mc2_objective_view();
    assert_eq!(stages[0].1, 2, "row 0 fly-to latched");
    assert_eq!(stages[1].1, 2, "row 1 fly-to latched");
    assert_eq!(
        stages[2],
        (7, 1),
        "row 2 (kill archers) armed, NOT vacuously latched"
    );
    assert_eq!(count(&w, 5, 4), 4, "dis 3 released the archer wave");

    // Extinguish the archer wave with the smite instrument — this
    // test's subject is the OBJECTIVE CHAIN reacting to model-4
    // extinction, not marksmanship (a stray-fireball fight floods
    // model-4 MILITIA into the type-7 extinction predicate; authentic,
    // but separately owned by the combat fixtures). The native MC2
    // hands are unbound and the runner is invincible.
    assert!(w.debug_smite(5, 4) >= 4, "the wave was live to smite");
    hover(&mut w, 194.5, 213.5, 48, idle);
    assert_eq!(count(&w, 5, 4), 0, "the archer wave died");
    let (_, stages) = w.mc2_objective_view();
    assert_eq!(stages[2].1, 2, "row 2 latched on the real kills");
    assert_eq!(
        count(&w, 15, 3),
        1,
        "the m17 kill switch dropped the spell jar"
    );
    assert!(!w.completed(), "row 4 held — no premature completion");

    // Row 3 = castle + banked share (type 0). Force it (the banked
    // economy is pending) and expect the m32 watcher's dis 6.
    w.debug_complete_mc2_stage(3);
    hover(&mut w, 170.0, 200.0, 32, idle);
    assert_eq!(count(&w, 5, 19), 5, "dis 6 released the FIREFLY wave");
    let (cur, stages) = w.mc2_objective_view();
    assert_eq!(stages[4], (7, 1), "row 4 (kill fireflies) armed and held");
    assert_eq!(cur, 4, "the cursor advanced to the firefly hunt");
    assert!(!w.completed(), "the wave must die first");

    // Extinguish the wave → all rows complete → the level ends
    // (the smite instrument again — the chain is the subject).
    assert!(w.debug_smite(5, 19) >= 1, "fireflies live to smite");
    hover(&mut w, 170.0, 200.0, 64, idle);
    assert_eq!(count(&w, 5, 19), 0, "the firefly wave died");
    assert!(w.completed(), "all stages done — the level completed");
    assert!(
        w.misfits().is_empty(),
        "no misfits on the full run (start markers + castle guards known): {:?}",
        w.misfits()
    );
}

/// The par1-authored SPELLS.DAT overrides (PrepareEvents EV:387-390):
/// a synthetic (10,11) tier-1 and (10,15) tier-2 THING must spawn with
/// the RETAIL CD table's life values — row 16 {6,12,24} / row 17
/// {16,32,64} — not the ctor defaults (240 / 128). Uses the real
/// baked spells.bin, so this also guards the import end to end
/// (the CD values differ from the decompile's baked-in fallback).
#[test]
fn mc2_par1_spells_overrides() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).unwrap();
    let Some(sp) = bundle.spells.as_deref() else {
        common::golden_skip("bundle predates spells.bin (rebake)");
        return;
    };
    let assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap()
    .with_spells(sp)
    .unwrap();
    let planes = Planes {
        height: vec![50; 65536],
        tile_type: vec![1; 65536],
        shading: vec![32; 65536],
        angle: vec![0; 65536],
        ceiling: Vec::new(),
    };
    let thing = |slot, model, x, par1| mgc_formats::Thing {
        slot,
        kind: mgc_formats::ThingKind::Entity,
        class: 10,
        model,
        x,
        y: 100,
        dis_id: 0,
        swi_sz: 0,
        swi_id: 0,
        parent: par1,
        child: 0,
        par3: None,
    };
    let things = [thing(1, 11, 100, 1), thing(2, 15, 120, 2)];
    let w = World::new_for_game(planes, &things, 1, assets, GameId::Mc2);
    let (_, pool) = w.debug_pool();
    // (10,11) = the SCORCH RING (NewAdd0A0B_4E840) — NOT a remap to
    // model 19.
    let ring = pool
        .iter()
        .find(|e| e.class == 10 && e.model == 11)
        .expect("the (10,11) scorch ring spawned");
    assert_eq!(ring.life, 12, "row 16 tier 1 life (CD SPELLS.DAT)");
    let trail = pool
        .iter()
        .find(|e| e.class == 10 && e.model == 15)
        .expect("the (10,15) trail spawned");
    assert_eq!(trail.life, 64, "row 17 tier 2 life (CD SPELLS.DAT)");
}

/// The (10,9) raise-land dome (mc2::morph): a synthetic tier-0 dome
/// on flat ground must ease a raised-cosine hill up over its life,
/// finalize to the `summit - 24` plateau with the 2x2 cap at
/// `plateau - 16`, and despawn — geometry per
/// docs/traces/mc2-class10-m9-dome-geometry.md (par1=0 → CD SPELLS
/// row 18 tier 0: maxLife 7, subSpell 400; radius = 7|1 = 7 tiles,
/// height = 2*7 + 100 = 114 over the base 50).
#[test]
fn mc2_dome_raises_and_finalizes() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).unwrap();
    let Some(sp) = bundle.spells.as_deref() else {
        common::golden_skip("bundle predates spells.bin (rebake)");
        return;
    };
    let assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap()
    .with_spells(sp)
    .unwrap();
    let planes = Planes {
        height: vec![50; 65536],
        tile_type: vec![1; 65536],
        shading: vec![32; 65536],
        angle: vec![0; 65536],
        ceiling: Vec::new(),
    };
    let things = [mgc_formats::Thing {
        slot: 1,
        kind: mgc_formats::ThingKind::Entity,
        class: 10,
        model: 9,
        x: 100,
        y: 100,
        dis_id: 0,
        swi_sz: 0,
        swi_id: 0,
        parent: 0, // par1 = tier 0
        child: 0,
        par3: None,
    }];
    let mut w = World::new_for_game(planes, &things, 1, assets, GameId::Mc2);
    {
        let (_, pool) = w.debug_pool();
        let dome = pool
            .iter()
            .find(|e| e.class == 10 && e.model == 9)
            .expect("the dome spawned");
        assert_eq!(dome.life, 17, "ctor life stands (override hits maxLife)");
    }
    // Park the player far away and run the dome to completion:
    // 16 grow ticks + the phase-2 flip + finalize.
    let idle = PlayerCommand::default();
    hover(&mut w, 30.0, 30.0, 24, idle);
    let (_, pool) = w.debug_pool();
    assert!(
        !pool.iter().any(|e| e.class == 10 && e.model == 9),
        "the dome despawned after finalize"
    );
    let h = |tx: usize, ty: usize| w.planes().height[ty << 8 | tx] as i32;
    // Base 50 + height 114 - 24 = the 140 plateau. The center tile
    // is (101,101) — retail's `(pos + 128) >> 8` on the authored
    // tile-center position (EF:23241) — so the 2x2 summit cap
    // presses (100..=101, 100..=101) to 124.
    for (tx, ty) in [(100, 100), (101, 100), (100, 101), (101, 101)] {
        assert_eq!(h(tx, ty), 124, "summit cap at ({tx},{ty})");
    }
    // Inside the disc but off the cap: clamped to the plateau.
    assert_eq!(h(99, 99), 140, "plateau northwest of the cap");
    assert_eq!(h(102, 101), 140, "plateau east of the cap");
    // Far outside the 7-tile disc: untouched flat ground.
    assert_eq!(h(120, 100), 50, "ground beyond the footprint");
    // The (10,18) summit child is REAL (mc2::morph summit vortex):
    // the ledger is clean and the eruption family (the vortex or what
    // it emitted before its ground-shift teardown) actually ran — the
    // finalize pass moves the terrain under the vortex, so by now it
    // may have despawned; the (10,19) fire column it raised on tick 0
    // persists.
    assert!(
        !w.misfits().iter().any(|&(c, m, _)| (c, m) == (10, 18)),
        "no (10,18) misfit anymore: {:?}",
        w.misfits()
    );
    let (_, pool) = w.debug_pool();
    assert!(
        pool.iter().any(|e| e.class == 10 && e.model == 19),
        "the summit fire-spray column exists"
    );
}

/// The (5,10) doomsday pyramid (mc2::doomsday): on a doom-flagged
/// level it activates (footprint wipe + terrain-flatten crater,
/// sound 10), is unkillable by damage (the life-8 clamp), and its
/// scripted death (tripped by player proximity) mass-kills the
/// creatures and hands off to the (10,9) APOCALYPSE dome with the
/// extinction latch set (docs/traces/mc2-class5-m10-doomsday.md).
/// On an unflagged level the first tick applies retail's ctor gate
/// and the pyramid never exists.
#[test]
fn mc2_doomsday_pyramid_extinction_script() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).unwrap();
    let assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap();
    let planes = || Planes {
        height: vec![50; 65536],
        tile_type: vec![2; 65536],
        shading: vec![32; 65536],
        angle: vec![0; 65536],
        ceiling: Vec::new(),
    };
    let thing = |slot: u32, class: u16, model: u16, x: u16, y: u16| mgc_formats::Thing {
        slot,
        kind: mgc_formats::ThingKind::Entity,
        class,
        model,
        x,
        y,
        dis_id: 0,
        swi_sz: 0,
        swi_id: 0,
        parent: 0,
        child: 0,
        par3: None,
    };
    let idle = PlayerCommand::default();

    // Unflagged level: the gate despawns it on the first tick.
    let things = [thing(1, 5, 10, 100, 100)];
    let mut w = World::new_for_game(planes(), &things, 1, assets.clone(), GameId::Mc2);
    hover(&mut w, 30.0, 30.0, 2, idle);
    assert!(
        w.debug_pool().1.iter().all(|e| e.class != 5),
        "no pyramid on an unflagged level"
    );

    // Doom level: activate far from the player and run the active
    // cycle — the crater flatten + the falling-rock ring; the
    // pyramid holds (death is damage-scripted — the life-8 clamp
    // route is covered by the in-crate test).
    let things = [thing(1, 5, 10, 100, 100), thing(2, 5, 1, 130, 130)];
    let mut w = World::new_for_game(planes(), &things, 1, assets, GameId::Mc2);
    w.set_mc2_doom_level(true);
    hover(&mut w, 220.0, 220.0, 40, idle);
    {
        let (_, pool) = w.debug_pool();
        let p = pool
            .iter()
            .find(|e| e.class == 5 && e.model == 10)
            .expect("the pyramid stands");
        assert!(p.life >= 8, "unkillable clamp holds");
        assert!(
            pool.iter().any(|e| e.class == 10 && e.model == 14),
            "the falling-rock summon ring spins"
        );
    }
    // The flatten crater: the center region sinks below the flat 50.
    let h = |tx: usize, ty: usize| w.planes().height[ty << 8 | tx] as i32;
    assert!(
        h(100, 100) < 50 || h(101, 101) < 50,
        "the crater is sinking ({} / {})",
        h(100, 100),
        h(101, 101)
    );
}

/// THE VISSULUTH SIZE LAW (player retail footage, 2026-08-04). The
/// (5,10) doomsday boss rewrites its own sprite-parameter row's height
/// field: `D951C[341].rotSpeed_8 := 60` at the ritual start (EF:12700)
/// — a 20x linear shrink off the authored 1200, "a tiny handful of
/// pixels" — then `:= the doom meter` every tick of the ramp the 0xA00
/// proximity wake arms (EF:13041), stepping +30/tick from 30 up to
/// exactly the authored 1200. The boss therefore sits invisibly small
/// through the wait, grows in discrete tick steps for 40 ticks, and
/// stays full size for the rest of the fight. It ALSO carries raster
/// bit 23 (ctor `|= 0x48800001`, EF:33980 → mode 2, 33% blend,
/// GRO:3798-3805) which the same wake clears (EF:13024). All three
/// legs are asserted, so neutering any of them fails the test.
#[test]
fn mc2_doomsday_is_tiny_until_the_proximity_wake_then_grows_to_full_size() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).unwrap();
    let assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap();
    let planes = Planes {
        height: vec![50; 65536],
        tile_type: vec![2; 65536],
        shading: vec![32; 65536],
        angle: vec![0; 65536],
        ceiling: Vec::new(),
    };
    let things = [mgc_formats::Thing {
        slot: 1,
        kind: mgc_formats::ThingKind::Entity,
        class: 5,
        model: 10,
        x: 100,
        y: 100,
        dis_id: 0,
        swi_sz: 0,
        swi_id: 0,
        parent: 0,
        child: 0,
        par3: None,
    }];
    let idle = PlayerCommand::default();
    let mut w = World::new_for_game(planes, &things, 1, assets, GameId::Mc2);
    w.set_mc2_doom_level(true);

    // The boss's exported (blend, patched sprite height) — None while
    // it is hidden (the ctor's byte[0] bit 0 holds the whole opening
    // ritual out of the billboard pass).
    let boss = |w: &World| {
        w.live_poses()
            .into_iter()
            .find(|p| p.class == 5 && p.model == 10)
            .map(|p| (p.blend, p.sprite_h_units))
    };

    // Wait phase: hover far away (well outside 0xA00 = 10 tiles) until
    // the ritual's hide bit clears and the body becomes drawable.
    let mut revealed = None;
    for _ in 0..400 {
        hover(&mut w, 220.0, 220.0, 1, idle);
        if let Some(b) = boss(&w) {
            revealed = Some(b);
            break;
        }
    }
    assert_eq!(
        revealed,
        Some((2, Some(60.0))),
        "the revealed boss is 33%-ghosted AND shrunk to 60/1200 of its \
         authored sprite height through the wait phase"
    );
    // And it holds that state while the player keeps his distance.
    hover(&mut w, 220.0, 220.0, 60, idle);
    assert_eq!(boss(&w), Some((2, Some(60.0))), "still tiny at range");

    // Close to ~4 tiles: the wake drops bit 23 (opaque) and arms the
    // meter ramp. Sample the exported height every tick.
    // The export stops once the state machine swaps the sprite row
    // (341 → 343 for the attack): rows 342-345 are never patched, so
    // they always draw the authored full size — that IS retail.
    let mut sizes = Vec::new();
    let mut blend_after_wake = None;
    for _ in 0..200 {
        hover(&mut w, 103.5, 103.5, 1, idle);
        let Some((bl, h)) = boss(&w) else { continue };
        if bl == 0 {
            blend_after_wake = Some(bl);
            match h {
                Some(h) => sizes.push(h),
                None if !sizes.is_empty() => break, // attack row: unpatched
                None => {}
            }
        }
    }
    assert_eq!(
        blend_after_wake,
        Some(0),
        "closing inside 10 tiles turns the demon opaque"
    );
    // Retail's ramp: +30 per tick, monotonic, ending exactly on the
    // authored 1200 and then holding there (the meter's later reuse as
    // a state timer must NOT shrink the boss again).
    assert!(
        sizes.windows(2).all(|w| w[1] >= w[0]),
        "the growth ramp never goes backwards: {sizes:?}"
    );
    assert!(
        sizes.iter().any(|&h| h > 60.0 && h < 1200.0),
        "the ramp is STEPPED — mid-growth samples exist: {sizes:?}"
    );
    assert_eq!(
        sizes.last().copied(),
        Some(1200.0),
        "the ramp settles at the authored full size and stays: {sizes:?}"
    );
}

/// The growth ramp's render smoothing (player-requested presentation
/// deviation): consecutive tick snapshots straddle a `sprite_h_units`
/// step, which is what lets the app's `lerp_poses` interpolate the
/// scale on the frame alpha instead of popping once per tick.
#[test]
fn mc2_doomsday_growth_ramp_exports_lerpable_size_steps() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked mc2 data not present");
        return;
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).unwrap();
    let assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap();
    let planes = Planes {
        height: vec![50; 65536],
        tile_type: vec![2; 65536],
        shading: vec![32; 65536],
        angle: vec![0; 65536],
        ceiling: Vec::new(),
    };
    let things = [mgc_formats::Thing {
        slot: 1,
        kind: mgc_formats::ThingKind::Entity,
        class: 5,
        model: 10,
        x: 100,
        y: 100,
        dis_id: 0,
        swi_sz: 0,
        swi_id: 0,
        parent: 0,
        child: 0,
        par3: None,
    }];
    let idle = PlayerCommand::default();
    let mut w = World::new_for_game(planes, &things, 1, assets, GameId::Mc2);
    w.set_mc2_doom_level(true);
    let boss_h = |w: &World| {
        w.live_poses()
            .into_iter()
            .find(|p| p.class == 5 && p.model == 10)
            .and_then(|p| p.sprite_h_units)
    };
    // Run the ritual out at range, then close and capture the first
    // strictly-growing tick pair.
    for _ in 0..400 {
        hover(&mut w, 220.0, 220.0, 1, idle);
        if boss_h(&w).is_some() {
            break;
        }
    }
    let mut step = None;
    let mut prev = boss_h(&w);
    for _ in 0..200 {
        hover(&mut w, 103.5, 103.5, 1, idle);
        let cur = boss_h(&w);
        if let (Some(a), Some(b)) = (prev, cur) {
            if b > a {
                step = Some((a, b));
                break;
            }
        }
        prev = cur;
    }
    let (a, b) = step.expect("the ramp produces a growing tick pair to lerp across");
    assert_eq!(b - a, 30.0, "retail's step is exactly +30 units/tick");
    // The half-frame the renderer would draw sits strictly between the
    // two tick values — that IS the smoothing.
    let mid = a + (b - a) * 0.5;
    assert!(a < mid && mid < b, "a frame alpha of 0.5 lands mid-step");
}
