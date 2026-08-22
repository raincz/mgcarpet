//! MC2 barrel roll, end to end over the public API (retail
//! `sub_55C60`/`sub_55EB0`; the phase-machine unit tests live beside
//! the driver in `flight.rs`, the lock-break beside the pool in
//! `engine/world.rs`): the both-strafes command tumbles the view a
//! full turn and settles, and an MC1 world refuses the command
//! outright (the flight-verb gate — retail MC1 has no such move).
//!
//! Runs against the real bakes (`baked/`); skips silently when the
//! player's gamedata bake is absent (CI without game assets).

use mgc_formats::LevelPackage;
use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::World;
use mgc_sim::ids::GameId;
use mgc_sim::{FlightInput, Simulation, ThrustModel};
use std::path::{Path, PathBuf};

#[path = "common/mod.rs"]
mod common;

fn baked_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    (p.join("mc1/level-005.mgcl").exists() && !common::modded_bake(&p)).then_some(p)
}

fn planes_of(pkg: &LevelPackage) -> Option<Planes> {
    let terrain = pkg.terrain.as_ref()?;
    Some(Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone()?,
        angle: terrain.angle.clone()?,
        ceiling: terrain.ceiling.clone().unwrap_or_default(),
    })
}

fn read_pkg(path: &Path) -> Option<LevelPackage> {
    mgc_formats::mgcl::read(std::fs::File::open(path).ok()?).ok()
}

fn world(root: &Path, game: GameId) -> Option<World> {
    let (bundle, level) = match game {
        GameId::Mc2 => ("assets/mc2-night", "mc2/level-001.mgcl"),
        _ => ("assets/mc1-temperate", "mc1/level-005.mgcl"),
    };
    let bundle = mgc_formats::bundle::Bundle::load(&root.join(bundle)).ok()?;
    let assets = FeatureAssets::parse(
        bundle.search.as_ref()?,
        bundle.build_tab.as_ref()?,
        bundle.build_dat.as_ref()?,
    )
    .ok()?
    .with_bldgprm(bundle.bldgprm.as_deref().unwrap_or_default());
    let assets = match bundle.spells.as_deref() {
        Some(sp) => assets.with_spells(sp).ok()?,
        None => assets,
    };
    let pkg = read_pkg(&root.join(level))?;
    let seed = pkg.gen_params.as_ref().map_or(0, |g| g.seed);
    Some(World::new_for_game(
        planes_of(&pkg)?,
        &pkg.things.things,
        seed,
        assets,
        game,
    ))
}

fn sim_of(w: World) -> Simulation {
    let mut s = Simulation::with_world(w);
    s.thrust_model = ThrustModel::Mc1;
    s.sync_carpet_from_flyer();
    s
}

const ROLL: FlightInput = FlightInput {
    thrust: 0.0,
    strafe: 0.0,
    lift: 0.0,
    yaw_delta: 0.0,
    pitch_delta: 0.0,
    stick_x: 0,
    stick_y: 0,
    fire_left: false,
    fire_right: false,
    equip_left: None,
    equip_right: None,
    mc2_select: None,
    spell_ring: None,
    full_stop: false,
    respawn: false,
    demolish: false,
    suicide: false,
    barrel_roll: true,
    raw_dx: 0,
    mc1_move_byte: None,
    mc2_cmd_speed: None,
    mc2_park: false,
    cheat: None,
};

/// The command tumbles the MC2 view through inverted and settles back
/// to level flight, with the roll state reading active for the whole
/// arc and idle after.
#[test]
fn mc2_command_rolls_the_view_and_settles() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked gamedata");
        return;
    };
    let Some(w) = world(&root, GameId::Mc2) else {
        return;
    };
    let mut s = sim_of(w);
    for _ in 0..30 {
        s.step(&FlightInput::default());
    }
    assert!(!s.barrel_rolling());
    let level_roll = s.flyer.roll;
    assert!(level_roll.abs() < 0.1, "level flight before the roll");

    s.step(&ROLL);
    assert!(s.barrel_rolling(), "the command arms the roll");
    let (mut ticks, mut past_half) = (0u32, false);
    while s.barrel_rolling() {
        s.step(&FlightInput::default());
        // Masked to [0, 2π): inverted is the middle of the circle.
        past_half |= (2.0..4.5).contains(&s.flyer.roll);
        ticks += 1;
        assert!(ticks < 400, "the roll must settle");
    }
    assert!(past_half, "the view passed through inverted");
    assert!((20..200).contains(&ticks), "duration sane: {ticks}");
    for _ in 0..30 {
        s.step(&FlightInput::default());
    }
    assert!(
        s.flyer.roll.abs() < 0.1,
        "level flight resumes: {}",
        s.flyer.roll
    );
}

/// MC1 worlds refuse the command — the flight-verb gate. Retail MC1
/// has no barrel roll (and its decompile has no input module to prove
/// a hidden one — absence is the faithful default).
#[test]
fn mc1_world_refuses_the_command() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked gamedata");
        return;
    };
    let Some(w) = world(&root, GameId::Mc1) else {
        return;
    };
    let mut s = sim_of(w);
    for _ in 0..5 {
        s.step(&FlightInput::default());
    }
    s.step(&ROLL);
    assert!(!s.barrel_rolling(), "MC1 never rolls");
}
