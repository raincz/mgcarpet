//! Runtime trigger/disposition integration over real baked data:
//! level 005's authored cascade — a proximity trigger at (99,115)
//! fires disposition 1 (the chain-terminating crater at (95,108) + a
//! follow-up trigger), whose trigger fires disposition 2 (an 8-creature
//! ambush around the crater).
//!
//! Self-skips when the baked tree is absent (game data is optional).

use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use std::path::PathBuf;

#[path = "common/mod.rs"]
mod common;

fn baked_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    (p.join("mc1/level-005.mgcl").exists() && !common::modded_bake(&p)).then_some(p)
}

fn build_world(root: &std::path::Path) -> (World, usize) {
    let file = std::fs::File::open(root.join("mc1/level-005.mgcl")).unwrap();
    let pkg: mgc_formats::LevelPackage = mgc_formats::mgcl::read(file).unwrap();
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc1-temperate")).unwrap();
    let terrain = pkg.terrain.as_ref().unwrap();
    let planes = Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone().unwrap(),
        angle: terrain.angle.clone().unwrap(),
        ceiling: Vec::new(),
    };
    let assets = FeatureAssets::parse(
        bundle.search.as_ref().unwrap(),
        bundle.build_tab.as_ref().unwrap(),
        bundle.build_dat.as_ref().unwrap(),
    )
    .unwrap();
    let seed = pkg.gen_params.as_ref().map_or(0, |g| g.seed);
    let world = World::new(planes, &pkg.things.things, seed, assets);
    let drawable = pkg
        .things
        .things
        .iter()
        .filter(|t| t.kind == mgc_formats::ThingKind::Entity && matches!(t.class, 2 | 3 | 5 | 12))
        .count();
    (world, drawable)
}

/// Hover the player near the ground at (x, z) for `ticks` turns.
fn fly(w: &mut World, x: f32, z: f32, ticks: usize) {
    for _ in 0..ticks {
        let alt = w.ground_height_tiles(x, z) + 2.0;
        w.tick(
            PlayerPose::from_tiles(x, alt, z, 0.0, 0.0, 0.0),
            PlayerCommand::default(),
        );
    }
}

/// Sum of heights over a tile rectangle.
fn region_height(w: &World, x0: usize, y0: usize, x1: usize, y1: usize) -> u32 {
    let mut sum = 0u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            sum += w.planes().height[y * 256 + x] as u32;
        }
    }
    sum
}

#[test]
fn level_005_trigger_cascade() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let (mut w, drawable_records) = build_world(&root);

    // Disposition gating: only dis_id == 0 things exist at level init.
    let init = w.live_things().len();
    assert!(
        init < drawable_records,
        "expected latent things: {init} live of {drawable_records} drawable records"
    );
    assert!(w.live_things().iter().all(|t| t.dis_id == 0));

    // Idle far away: nothing fires, terrain static.
    let before = region_height(&w, 90, 103, 101, 114);
    fly(&mut w, 20.0, 20.0, 32);
    assert_eq!(w.live_things().len(), init);
    assert_eq!(region_height(&w, 90, 103, 101, 114), before);

    // Fly into the trigger at (99,115) (extent 4 tiles) → disposition
    // 1: the model-11 crater near (95,108) spawns and digs from the
    // next ticks on. The visit point sits inside trigger 1 but clear
    // of the follow-up trigger's volume at (95,109) (extent 6 tiles +
    // the player carpet's sprite-44 half-width — the AABB test sums
    // both entities' extents, sub_118C0).
    fly(&mut w, 101.5, 117.5, 16);
    fly(&mut w, 20.0, 20.0, 120);
    let after_crater = region_height(&w, 90, 103, 101, 114);
    assert!(
        after_crater < before,
        "crater must dig: region height {after_crater} vs {before}"
    );
    // ⚠ Counted WITHOUT the village families, for the same reason the
    // one-shot assertion below excludes them: full dwellings emit and
    // absorb villagers on their own clock, so a plain class-5 count
    // measures the ambush plus whatever the village did in the same 16
    // ticks. It went unnoticed while the port's houses carried a
    // quarter of retail's occupancy cap and turned nearly every feeder
    // away; with `+128 = area/4` landed, two more walk in the door
    // during this window and the raw delta reads 6.
    let live_after_crater = non_village_count(&w);

    // The follow-up trigger at (95,109) → disposition 2: the ambush
    // (8 class-5 model-2 creatures around the crater).
    fly(&mut w, 95.5, 109.5, 16);
    let live_final = non_village_count(&w);
    assert_eq!(
        live_final - live_after_crater,
        8,
        "disposition 2 spawns the 8-creature ambush"
    );

    // Both triggers were one-shot: no NEW spawns on re-entry (the
    // ambush creatures now move and may die in the fresh crater, so
    // the count can only shrink). The VILLAGE families (m4/12/13/14)
    // are excluded: full dwellings emit and absorb villagers on their
    // own clock (sub_28DC0/sub_1F640), so their count breathes either
    // way — only the trigger-spawned models prove one-shot-ness.
    let nv_final = non_village_count(&w);
    fly(&mut w, 101.5, 117.5, 32);
    fly(&mut w, 95.5, 109.5, 32);
    assert!(non_village_count(&w) <= nv_final);
}

/// Live class-5 creature count EXCLUDING the village families
/// (m4/12/13/14): full dwellings emit and absorb villagers on their
/// own clock (sub_28DC0/sub_1F640), so their count breathes either
/// way — only the trigger-spawned models prove anything about a
/// trigger.
fn non_village_count(w: &World) -> usize {
    w.live_things()
        .iter()
        .filter(|t| t.class == 5 && !matches!(t.model, 4 | 12 | 13 | 14))
        .count()
}

#[test]
fn level_005_deterministic() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let run = || {
        let (mut w, _) = build_world(&root);
        fly(&mut w, 101.5, 117.5, 16);
        fly(&mut w, 20.0, 20.0, 100);
        fly(&mut w, 95.5, 109.5, 16);
        let poses: Vec<_> = w
            .live_poses()
            .iter()
            .map(|p| (p.type_index, (p.x * 256.0) as i32, (p.z * 256.0) as i32))
            .collect();
        (
            w.planes().height.clone(),
            w.planes().tile_type.clone(),
            w.live_things().len(),
            poses,
        )
    };
    assert_eq!(run(), run());
}
