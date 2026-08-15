//! MC1 Accelerate must survive ALIGNED thrust under the faithful
//! thrust model. Retail cancels on the v_14 speed-TOUCHED flag
//! (:65144-51), and v_14 arms only when the press actually moves v_12
//! (:55766-80) — while boosted, v_12 (±160/240) sits outside the ±80
//! input clamp, so the aligned press is inert and only the RESISTING
//! press cancels. Trap: do NOT fire both cancel directions on ANY
//! thrust — that kills hold + re-cast the moment the player flies
//! forward.
//!
//! This is the `Simulation`-level companion to the World-level law
//! test (`accelerate_directions_are_mutually_exclusive`): the input
//! plumbing under test lives in lib.rs, which only these steps drive.

use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::World;
use mgc_sim::mc1::spells::SpellId;
use mgc_sim::{FlightInput, Simulation, ThrustModel};

/// Synthetic diamond-ring SEARCH.DAT + a 4x4 building row (the same
/// shape as the sim's unit-test assets — no baked data needed).
fn synthetic_assets() -> FeatureAssets {
    let mut grid = vec![31u8; 1024];
    for y in 0..32i32 {
        for x in 0..32i32 {
            let (dx, dy) = (x - 15, y - 15);
            let r = dx.max(dy).max(-dx + 1).max(-dy + 1) - 1;
            grid[(y * 32 + x) as usize] = r.clamp(0, 31) as u8;
        }
    }
    let tab: Vec<u8> = (0..24u32)
        .flat_map(|_| {
            let mut e = 0u32.to_le_bytes().to_vec();
            e.extend_from_slice(&[4, 4]);
            e
        })
        .collect();
    let mut dat = Vec::new();
    for row in 0..4 {
        dat.push(4u8);
        if row == 1 || row == 2 {
            dat.extend_from_slice(&[0x10, 7, 7, 0x10]);
        } else {
            dat.extend_from_slice(&[0x10, 0x10, 0x10, 0x10]);
        }
        dat.push(0);
    }
    FeatureAssets::parse(&grid, &tab, &dat).unwrap()
}

fn flat_world() -> World {
    let planes = Planes {
        height: vec![100; 0x10000],
        tile_type: vec![5; 0x10000],
        shading: vec![32; 0x10000],
        angle: vec![5; 0x10000],
        ceiling: Vec::new(),
    };
    World::new(planes, &[], 1, synthetic_assets())
}

#[test]
fn mc1_thrust_model_keeps_accelerate_through_forward_hold() {
    let mut sim = Simulation::with_world(flat_world());
    sim.thrust_model = ThrustModel::Mc1;
    sim.world.as_mut().unwrap().set_dev_spells(true);
    sim.step(&FlightInput {
        equip_left: Some(SpellId(2)),
        ..Default::default()
    });
    let boost = |sim: &Simulation| sim.world.as_ref().unwrap().accel_override();

    // CAST-AND-HOLD while flying forward: full boost every tick.
    let hold = FlightInput {
        fire_left: true,
        thrust: 1.0,
        ..Default::default()
    };
    for n in 0..5 {
        sim.step(&hold);
        assert_eq!(
            boost(&sim),
            Some(3.0),
            "held cast + forward thrust keeps max boost (tick {n})"
        );
    }

    // Button released, still pushing forward: the decay channel runs.
    sim.step(&FlightInput {
        thrust: 1.0,
        ..Default::default()
    });
    assert_eq!(
        boost(&sim),
        Some(2.0),
        "forward thrust alone must not cancel the decay channel"
    );

    // RE-CAST during the decay: allowed, back to full.
    sim.step(&hold);
    assert_eq!(boost(&sim), Some(3.0), "re-cast re-arms the full boost");

    // The resisting input is the one cancel (manual: the down cursor)
    // — and it is TWO-PHASE like retail's: the brake press moves the
    // boosted target and arms the mover's v_14 latch (:55766-80)
    // while the boost still runs this tick; the token reads the latch
    // on its next pass and ends the burst (counter = 1 → 0,
    // :65146-50).
    sim.step(&FlightInput {
        thrust: -1.0,
        ..Default::default()
    });
    assert_eq!(
        boost(&sim),
        Some(2.0),
        "the brake tick itself still boosts — v_14 only arms"
    );
    sim.step(&FlightInput::default());
    assert_eq!(boost(&sim), None, "the token ends the burst one pass later");

    // And the refire gate clears: a fresh cast works next tick.
    sim.step(&hold);
    assert_eq!(boost(&sim), Some(3.0), "fresh cast after the cancel");
}
