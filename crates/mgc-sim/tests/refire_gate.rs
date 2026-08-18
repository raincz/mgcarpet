//! Regression coverage for the cast refire gate. The trigger classes,
//! per the decompile latch (+60 field, :20601/:20621):
//! - EDGE spells (fireball 0 and the 1/4/5/12/14 channels): one cast
//!   per press; a fresh press refires even mid-burst (no `armed`
//!   cadence gate), but HOLDING never re-casts.
//! - 15 Lightning: streams while held, per-shot debit; a dry stream
//!   (pool empty) dies SILENTLY and does NOT auto-resume when mana
//!   returns — only a fresh click restarts it.
//! - 23 Rapid Fireball: the firehose — one emission per held tick.
//! - 2/21 Accelerate: hold-to-channel (the +60==0 set minus 15/23).
//! - 16 Castle recast-fizzle is pinned separately in spell_castle.rs.
//!
//! Drives the public `World::tick(pose, PlayerCommand)` surface on a
//! synthetic flat world (no baked data), like accelerate_hold.rs.
//! NOTE: the per-tick mana census re-derives `mana_max` from world
//! mana, pinning the pool ceiling at the intrinsic 1000 here. The
//! pure gate-semantics tests therefore run under dev-spells (gate +
//! debit bypassed; the refire logic is identical), and the lightning
//! mana-law test runs with REAL mana on BLUE-blessed spells
//! (`debug_bless_owned_spells` zeroes the castle ladder) with the
//! pool refilled to its 1000 ceiling between ticks.

use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::mc1::spells::SpellId;

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

/// A flat world with every spell granted under dev-spells. The pose
/// flies well above the ×32-scaled terrain (height 100 → 3200
/// engine) so projectiles don't spawn underground.
fn armed_world() -> (World, PlayerPose) {
    let planes = Planes {
        height: vec![100; 0x10000],
        tile_type: vec![5; 0x10000],
        shading: vec![32; 0x10000],
        angle: vec![5; 0x10000],
        ceiling: Vec::new(),
    };
    let mut w = World::new(planes, &[], 1, synthetic_assets());
    w.set_dev_spells(true);
    let pose = PlayerPose::from_tiles(16.0, 40.0, 16.0, 0.0, 0.0, 0.0);
    (w, pose)
}

fn equip(w: &mut World, pose: PlayerPose, id: u8) {
    w.tick(
        pose,
        PlayerCommand {
            equip_left: Some(SpellId(id)),
            ..Default::default()
        },
    );
}

/// One tick with the pool topped to the 1000 ceiling first (real-mana
/// tests; a no-op refill under dev-spells).
fn tick_full(w: &mut World, pose: PlayerPose, fire: bool) {
    w.set_player_mana(1000);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: fire,
            ..Default::default()
        },
    );
}

fn projectiles(w: &World) -> usize {
    w.live_poses().iter().filter(|p| p.class == 9).count()
}

/// A fresh press refires immediately, even while the prior burst
/// counter is still live — no `armed` cadence gate.
#[test]
fn edge_spell_refires_on_every_fresh_press() {
    let (mut w, pose) = armed_world();
    equip(&mut w, pose, 0);
    tick_full(&mut w, pose, true); // press arms the token
    assert!(
        w.loadout().cooldown[0] > 0.0,
        "the burst counter is live after the arm"
    );
    tick_full(&mut w, pose, false); // the token fires at arm+1
    assert_eq!(projectiles(&w), 1, "first press casts");
    tick_full(&mut w, pose, true); // fresh edge mid-burst re-arms
    tick_full(&mut w, pose, false); // and fires again at arm+1
    assert_eq!(
        projectiles(&w),
        2,
        "the re-click refires with no cadence gate"
    );
}

/// The other half of the gate: HOLDING an edge spell casts once.
#[test]
fn edge_spell_does_not_refire_while_held() {
    let (mut w, pose) = armed_world();
    equip(&mut w, pose, 0);
    for _ in 0..10 {
        tick_full(&mut w, pose, true);
    }
    assert_eq!(projectiles(&w), 1, "10 held ticks = one cast");
}

/// The 1/4/5/12/14 channels are EDGE-only (+60==1) — holding must not
/// keep the channel armed.
#[test]
fn shield_channel_is_edge_only() {
    let (mut w, pose) = armed_world();
    equip(&mut w, pose, 4);
    tick_full(&mut w, pose, true);
    assert!(w.loadout().cooldown[4] > 0.0, "the click arms the shield");
    let mut expired_while_held = false;
    for _ in 0..600 {
        tick_full(&mut w, pose, true);
        if w.loadout().cooldown[4] == 0.0 {
            expired_while_held = true;
            break;
        }
    }
    assert!(
        expired_while_held,
        "holding must NOT renew the shield channel (edge-only law)"
    );
    tick_full(&mut w, pose, false);
    tick_full(&mut w, pose, true);
    assert!(w.loadout().cooldown[4] > 0.0, "a fresh click re-arms it");
}

/// 2 Accelerate stays a hold-to-channel toggle (+60==0).
#[test]
fn accelerate_channel_holds() {
    let (mut w, pose) = armed_world();
    equip(&mut w, pose, 2);
    for _ in 0..120 {
        tick_full(&mut w, pose, true);
    }
    assert!(
        w.loadout().cooldown[2] > 0.0,
        "holding keeps the accelerate channel armed past its burst"
    );
}

/// 23 Repeat Fireballs is a LAUNCHER (mc1l4 t=5376): the press tick
/// only ARMS the token (sub_46B00's bare LABEL_32 flow), the token
/// fires one ball per lap from its own tick (sub_58240 = fireball's
/// sub_56090), and the held re-issue re-arms every tick — so N held
/// ticks yield N−1 emissions, first ball one tick after the press.
#[test]
fn firehose_fires_every_held_tick() {
    let (mut w, pose) = armed_world();
    equip(&mut w, pose, 23);
    tick_full(&mut w, pose, true);
    assert_eq!(
        projectiles(&w),
        0,
        "the press tick ARMS only — the token fires next lap (the cast-phase law)"
    );
    for _ in 0..5 {
        tick_full(&mut w, pose, true);
    }
    assert!(
        projectiles(&w) >= 5,
        "6 held ticks yield 5 emissions (got {})",
        projectiles(&w)
    );
}

/// Held Lightning, stream half: while held with the pool covering the
/// re-arm, every tick re-emits (dev-spells bypasses the
/// mana check; the re-arm/emit logic is the same code path). The
/// zigzag is a one-tick multi-segment transient, so seeing segments
/// alive AFTER four held ticks proves a fresh per-tick emission.
#[test]
fn lightning_streams_while_held() {
    let (mut w, pose) = armed_world();
    equip(&mut w, pose, 15);
    for _ in 0..4 {
        tick_full(&mut w, pose, true);
        assert!(projectiles(&w) > 0, "the held stream re-emits every tick");
    }
}

/// Held Lightning, mana half — REAL mana: the re-arm check is
/// SILENT on an empty pool, the dry stream does NOT auto-resume when
/// mana returns while held, and only a fresh click restarts it. With
/// the synthetic 1000 ceiling the edge debit (-1000) empties the pool
/// by the next cast point, so the stream dies right after the first
/// emission — the faithful pool-1000 behavior.
#[test]
fn lightning_stream_dies_dry_and_needs_a_reclick() {
    let (mut w, pose) = armed_world();
    // Blue-bless the granted spells (zeroes lightning's 25000 castle
    // ladder) and drop the dev bypass.
    w.debug_bless_owned_spells();
    w.set_dev_spells(false);
    equip(&mut w, pose, 15);
    tick_full(&mut w, pose, true);
    assert!(projectiles(&w) > 0, "the edge cast fires");

    // Held ticks: the pool can never recover to 1000 by the cast
    // point (the -1000 delta applies first), so the re-arm silently
    // fails and the burst (count=2) decays to dead.
    for _ in 0..6 {
        tick_full(&mut w, pose, true);
    }
    assert_eq!(projectiles(&w), 0, "the dry stream emits nothing");
    assert_eq!(
        w.loadout().cooldown[15],
        0.0,
        "the burst is dead after the dry hold"
    );

    // The pool is back at its ceiling now (regen + refill), but the
    // button never came up: NO auto-resume.
    for _ in 0..6 {
        tick_full(&mut w, pose, true);
    }
    assert_eq!(
        projectiles(&w),
        0,
        "a dry stream must not auto-resume while held"
    );
    // A fresh click restarts it.
    tick_full(&mut w, pose, false);
    tick_full(&mut w, pose, true);
    assert!(projectiles(&w) > 0, "the re-click restarts the stream");
}
