//! MC2 armed-window channel behaviors:
//!
//! - **Invisibility (11) break-on-self-cast law** (`sub_5F7E0`
//!   EF:60987): T0 (any cast) breaks the cloak, T2 (nothing) survives.
//!   On break the invis window's burst `f26` is zeroed too, so the
//!   mana-regen block lifts with the cloak — observable here as
//!   `mc2_book_view().armed[11]` flipping false.
//! - **Speed (3) interruptible window** (`GetScroll_69DB0`,
//!   docs/spell-audit/speed.md): a BRAKE input cancels the window
//!   early. The interrupt zeroes the burst timer `f26` too, so the
//!   mana-regen suppression lifts with the boost — a forward press
//!   does not cancel.
//!
//! Self-skips without baked mc2 data (game data is optional).

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
    w.set_mc2_night_shade(true);
    Some(w)
}

/// A dry, unprotected tile to fly over (same scan as the castle test,
/// minus the 19×19 footprint — these spells need only a valid pose).
fn open_spot(w: &World) -> (u16, u16) {
    let p = w.planes();
    for cy in (24..222u16).step_by(3) {
        for cx in (24..232u16).step_by(3) {
            let t = (cy as usize % 256) * 256 + (cx as usize % 256);
            if p.angle[t] & 0x80 == 0 && p.angle[t] & 0xF != 0 {
                return (cx, cy);
            }
        }
    }
    panic!("no open spot on the level");
}

fn pose_at(w: &World, cx: u16, cy: u16) -> PlayerPose {
    let (px, pz) = (cx as f32 + 0.5, cy as f32 + 0.5);
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0)
}

fn count(w: &World, class: u8, model: u8) -> usize {
    w.debug_pool()
        .1
        .iter()
        .filter(|e| e.class == class && e.model == model && e.life >= 0)
        .count()
}

/// Pool RECORDS regardless of life — the lightning trail's (9,9)
/// billboards are born on the DEAD side of the born-dead law
/// (`maxLife = (node_slot >= beam_slot) - 1`, EF:58341) and are
/// already decaying by end-of-tick; retail's crackle renders from the
/// mid-frame draw, not from a surviving record.
fn records(w: &World, class: u8, model: u8) -> usize {
    w.debug_pool()
        .1
        .iter()
        .filter(|e| e.class == class && e.model == model)
        .count()
}

#[test]
fn mc2_possession_magnet_needs_a_mana_claim() {
    // Mana Magnet (Possession T1) must NOT drop a free-floating magnet
    // where the bolt happens to detonate in empty space — the magnet
    // rides a CLAIMED mana sphere, and a bolt that misses mana
    // "evaporates without trace". Cast over open terrain with no mana
    // in the flight path → zero (10,54) magnet auras exist after the
    // bolt resolves.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    assert_eq!(count(&w, 10, 54), 0, "no magnet auras at level start");

    w.mc2_select_spell(1, 1, 0); // Possession tier 1 = Mana Magnet
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    // Let the bolt fly and detonate on terrain.
    for _ in 0..40 {
        w.tick(pose, PlayerCommand::default());
    }
    assert_eq!(
        count(&w, 10, 54),
        0,
        "an empty-space possession bolt spawns NO magnet aura"
    );
}

#[test]
fn mc2_invisibility_break_law_per_tier() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);

    // --- Tier 0: any offensive cast BREAKS the cloak. -----------------
    // Bind invis (11) tier 0 to the left hand, fireball (0) to the right
    // (dev_spells self-grants both on select).
    w.mc2_select_spell(11, 0, 0);
    w.mc2_select_spell(0, 0, 1);
    // Cast invis: arms + runs the first effect tick (sets the flag and
    // the break strength). The window is now live.
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert!(
        w.mc2_book_view().armed[11],
        "T0 invisibility window is live right after casting"
    );
    // Cast fireball while cloaked at T0 → the arm-path break law fires
    // and zeroes the invis window (armed → false), lifting the regen
    // block with the cloak.
    w.tick(
        pose,
        PlayerCommand {
            fire_right: true,
            ..Default::default()
        },
    );
    assert!(
        !w.mc2_book_view().armed[11],
        "T0: casting fireball breaks invisibility (window cleared)"
    );

    // --- Tier 2: NOTHING breaks the cloak. ----------------------------
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    w.mc2_select_spell(11, 2, 0); // invis tier 2 (strength 3)
    w.mc2_select_spell(0, 0, 1);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert!(
        w.mc2_book_view().armed[11],
        "T2 invisibility window is live"
    );
    w.tick(
        pose,
        PlayerCommand {
            fire_right: true,
            ..Default::default()
        },
    );
    assert!(
        w.mc2_book_view().armed[11],
        "T2: casting fireball does NOT break invisibility (window survives)"
    );
}

#[test]
fn mc2_castle_cost_gate_tracks_live_level() {
    // The castle cast GATE must charge the OWN castle level's live
    // tier-scaled cost — not the stale SetSpell-time `max_life`. The
    // castle level rises via build with no re-select, so the gate must
    // track it (else you could recast below the shown cost). Retail
    // re-syncs via the +1 castle XP on each upgrade; we re-sync at the
    // gate.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);

    // A 19×19 clear footprint (the castle needs room to stamp).
    let p = w.planes();
    let mut spot = None;
    'outer: for cy in (24..222u16).step_by(3) {
        'cand: for cx in (24..232u16).step_by(3) {
            for dy in -9i32..=25 {
                for dx in -9i32..=9 {
                    let t =
                        ((cy as i32 + dy) as usize % 256) * 256 + ((cx as i32 + dx) as usize % 256);
                    if p.angle[t] & 0x80 != 0 || p.angle[t] & 0xF == 0 {
                        continue 'cand;
                    }
                }
            }
            spot = Some((cx, cy));
            break 'outer;
        }
    }
    let (cx, cy) = spot.expect("a clear 19x19 spot");
    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);

    // Build a level-1 castle.
    w.mc2_select_spell(2, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    for _ in 0..120 {
        w.tick(pose, PlayerCommand::default());
    }
    let (_, _, lvl1) = w.loadout().castle.expect("castle raised");
    assert_eq!(lvl1, 1, "castle at level 1");

    // The live upgrade cost at level 1 = LADDER[1] = 10000 (tier 0, no
    // multiply). The pane already shows this; the gate must match it.
    assert_eq!(
        w.mc2_book_view().cost[2],
        10_000,
        "the live castle cost is the level-1 ladder rung"
    );

    // Now play for real (dev off). With mana ABOVE the stale base (1000)
    // but BELOW the live cost (10000), the recast must be REFUSED — no
    // new castle ball launches, the castle stays level 1.
    w.set_dev_spells(false);
    w.set_player_mana(5_000);
    let count_balls = |w: &World| {
        w.debug_pool()
            .1
            .iter()
            .filter(|e| e.class == 9 && e.model == 10 && e.life >= 0)
            .count()
    };
    w.tick(pose, PlayerCommand::default()); // release (fresh edge)
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(
        count_balls(&w),
        0,
        "5000 mana < the live 10000 cost → the recast is refused"
    );
    let (_, _, lvl_after) = w.loadout().castle.expect("castle survives");
    assert_eq!(
        lvl_after, 1,
        "the refused recast left the castle at level 1"
    );

    // Sanity: with dev spells back on (the gate is bypassed) the same
    // recast DOES launch — proving the refusal above was the mana gate,
    // not a broken binding.
    w.set_dev_spells(true);
    w.tick(pose, PlayerCommand::default());
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(
        count_balls(&w),
        1,
        "dev-spells bypasses the gate → the recast launches"
    );
}

/// The CASTLE spell (2) "active" window is an UPGRADE LOCK that tracks
/// the tower build, NOT a fixed 101-tick timer. Retail
/// (`sub_69AB0`/`sub_5F890`) never counts the timer down; the castle
/// build/upgrade entity holds it and clears it on completion. Here:
/// casting must raise the lock, and it must drop the moment the build
/// settles — well before 101 ticks.
#[test]
fn mc2_castle_spell_lock_tracks_the_build_not_a_fixed_timer() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);

    // A 19×19 clear footprint (same scan as the cost-gate test).
    let p = w.planes();
    let mut spot = None;
    'outer: for cy in (24..222u16).step_by(3) {
        'cand: for cx in (24..232u16).step_by(3) {
            for dy in -9i32..=25 {
                for dx in -9i32..=9 {
                    let t =
                        ((cy as i32 + dy) as usize % 256) * 256 + ((cx as i32 + dx) as usize % 256);
                    if p.angle[t] & 0x80 != 0 || p.angle[t] & 0xF == 0 {
                        continue 'cand;
                    }
                }
            }
            spot = Some((cx, cy));
            break 'outer;
        }
    }
    let (cx, cy) = spot.expect("a clear 19x19 spot");
    let pose = PlayerPose::from_tiles(
        cx as f32 + 0.5,
        w.ground_height_tiles(cx as f32 + 0.5, cy as f32 + 16.5) + 2.0,
        cy as f32 + 16.5,
        0.0,
        0.0,
        0.0,
    );

    // Idle before casting: the lock is clear.
    assert_eq!(w.debug_mc2_spell_active(2), 0, "lock clear before casting");

    // Cast.
    w.mc2_select_spell(2, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    // The lock engages while the ball flies + the castle builds.
    let mut active_ticks = 0usize;
    let mut cleared_at = None;
    for t in 0..101 {
        w.tick(pose, PlayerCommand::default());
        if w.debug_mc2_spell_active(2) > 0 {
            active_ticks += 1;
        } else if active_ticks > 0 && cleared_at.is_none() {
            cleared_at = Some(t);
            break;
        }
    }
    let (_, _, lvl) = w.loadout().castle.expect("castle raised");
    assert_eq!(lvl, 1, "the build finished");
    // The lock was engaged during the build...
    assert!(active_ticks > 0, "the castle spell locked during the build");
    // ...and CLEARED when the build settled, strictly before the
    // 101-tick bound.
    let cleared = cleared_at.expect("the lock cleared after the build");
    assert!(
        cleared < 101,
        "the lock cleared at tick {cleared} — with the build, not a 101-tick timer"
    );
    // NB the castle lock does NOT suppress mana regen: retail's
    // `sub_69AB0` touches the caster's mana only once (the cost debit on
    // the cast tick, `sub_68DE0` while `word_0x2E_46 == word_0x30_48`),
    // never on the held ticks — unlike a generic channelled spell whose
    // `sub_693F0` suppresses every tick. The port matches: the castle
    // spell is handled outside the generic effect loop and never calls
    // `suppress_regen`.
    // Once cleared, the lock stays clear (no phantom re-arm) while idle.
    for _ in 0..30 {
        w.tick(pose, PlayerCommand::default());
    }
    assert_eq!(
        w.debug_mc2_spell_active(2),
        0,
        "the lock stays clear once the castle is standing idle"
    );
}

#[test]
fn mc2_lightning_l0_is_a_one_tick_beam() {
    // Lightning L0 (subtype 9) is a one-tick hitscan beam, not a
    // traveling ball — it must flash to its (10,23) blast and be gone,
    // NOT persist as a slow class-9 bolt. docs/spell-audit/lightning.md
    // §5.A.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);

    w.mc2_select_spell(7, 0, 0); // Lightning tier 0
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    // The VISIBLE flash: `sub_66750` lays a line of sprite-216 (9,9)
    // billboards from the muzzle to the impact THIS frame (the
    // crackle). Without them the one-tick beam despawns before it can
    // render.
    // Counted as RECORDS: the born-dead law (EF:58341) has every
    // node at/behind the beam decaying within the cast tick — which
    // node survives to end-of-tick depends on pool slot order, so
    // the live count is layout-noise. The LAID line is the law.
    let flash = records(&w, 9, 9);
    assert!(
        flash > 1,
        "the beam lays a sprite-216 trail flash ({flash} nodes)"
    );

    let mut saw_blast = false;
    let mut later_bolt = 0;
    for _ in 0..6 {
        w.tick(pose, PlayerCommand::default());
        saw_blast |= count(&w, 10, 23) > 0;
        later_bolt = later_bolt.max(count(&w, 9, 9));
    }
    assert!(saw_blast, "L0 detonates a (10,23) blast");
    assert_eq!(
        later_bolt, 0,
        "the flash is a 1-frame crackle — no (9,9) persists as a slow traveler"
    );
}

#[test]
fn mc2_lightning_storm_rains_beams() {
    // Lightning L1/L2 (subtype 12) detonates into the (10,38) STORM
    // cloud (`sub_4FFB0`), which hovers then RAINS (9,9) beams that
    // strike the ground as (10,23) impacts. It must stay pool-bounded.
    // docs/spell-audit/lightning.md §5.C.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let live = |w: &World| w.debug_pool().1.iter().filter(|e| e.life >= 0).count();

    // Ambient baseline (level-000 has its own churn).
    let Some(mut w0) = build_world(&root) else {
        return;
    };
    w0.set_dev_spells(true);
    let (cx, cy) = open_spot(&w0);
    let pose = pose_at(&w0, cx, cy);
    let mut ambient_peak = live(&w0);
    for _ in 0..120 {
        w0.tick(pose, PlayerCommand::default());
        ambient_peak = ambient_peak.max(live(&w0));
    }

    for tier in 1u8..=2 {
        let mut w = build_world(&root).unwrap();
        w.set_dev_spells(true);
        w.mc2_select_spell(7, tier, 0);
        w.tick(
            pose,
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        let (mut saw_cloud, mut saw_rain, mut peak) = (false, false, live(&w));
        for _ in 0..120 {
            w.tick(pose, PlayerCommand::default());
            saw_cloud |= count(&w, 10, 38) > 0;
            saw_rain |= count(&w, 10, 23) > 0; // a rained beam struck ground
            peak = peak.max(live(&w));
        }
        assert!(saw_cloud, "L{tier} spawns the (10,38) storm cloud");
        assert!(
            saw_rain,
            "L{tier} storm rains (9,9) beams → (10,23) strikes"
        );
        // T3 (`life == 2`) fires TWO bolts → two storm clouds
        // (sub_6A5C0's ±113 fan), so its budget doubles.
        let slack = if tier == 2 { 500 } else { 250 };
        assert!(
            peak <= ambient_peak + slack,
            "L{tier} storm stays pool-bounded (ambient {ambient_peak}, peak {peak})"
        );
    }
}

#[test]
fn mc2_lightning_t3_fans_two_bolts() {
    // Lightning T3 (`life_0x1A == 2`): the cast site `sub_6A5C0` spawns
    // TWO (9,12) charged bolts fanned yaw ±113 off the aim heading and
    // cross-links the pair via f52 (EF:56599-56656) — "two L2 bolts
    // side by side". T2 (`life == 1`) stays a single bolt.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);

    w.mc2_select_spell(7, 1, 0); // T2 first: a single bolt
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(count(&w, 9, 12), 1, "tier 2 launches a single bolt");

    let mut w = build_world(&root).unwrap();
    w.set_dev_spells(true);
    w.mc2_select_spell(7, 2, 0); // T3: the ±113 pair
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    let bolts = w.debug_flock_probe(9, 12);
    assert_eq!(bolts.len(), 2, "tier 3 launches two bolts");
    let mut aims: Vec<u16> = bolts.iter().map(|b| b.aim).collect();
    aims.sort_unstable();
    let mut expect = vec![
        pose.heading.wrapping_add(113) & 0x7FF,
        pose.heading.wrapping_sub(113) & 0x7FF,
    ];
    expect.sort_unstable();
    assert_eq!(aims, expect, "the pair fans yaw ±113 off the aim heading");
    assert_eq!(
        (bolts[0].leader as usize, bolts[1].leader as usize),
        (bolts[1].slot, bolts[0].slot),
        "the twins cross-link via f52 (word_0x34_52)"
    );
}

#[test]
fn mc2_alliance_charms_instead_of_burning() {
    // Alliance (spell 24): the (9,25) bolt's impact is the (10,74)
    // SAME-SPECIES AREA CHARM (`sub_50800` → `sub_3A650`,
    // EF:36945/29637) — creatures convert to the caster's side (the
    // controlled slot `8m+7`, StageVar2 = 14) and take ZERO damage.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        return;
    };
    w.set_dev_spells(true);

    // A grounded, charm-eligible species (the sub_3A7F0 bar excludes
    // 12-15/22/23/25/26/27; skip the known flyers — the (9,25) bolt
    // acquires grounded creatures only).
    let victim = w
        .debug_pool()
        .1
        .iter()
        .find(|e| {
            e.class == 5
                && e.life > 0
                && !matches!(e.model, 2 | 3 | 12..=16 | 19 | 22 | 23 | 25..=27)
        })
        .map(|e| e.model)
        .expect("level-000 has an eligible grounded creature");
    let model = victim;

    let life_sum = |w: &World| -> i64 {
        w.debug_pool()
            .1
            .iter()
            .filter(|e| e.class == 5 && e.model == model)
            .map(|e| e.life.max(0) as i64)
            .sum()
    };
    let before = life_sum(&w);

    // Point-blank pursuit: every volley re-reads the (wandering)
    // creature's live position and fires from 2 tiles south of it
    // (heading 0 flies -y) at its own height — the bolt's ≤128-unit
    // sub-step victim probe crosses the creature's map cell on the
    // first flight tick, no autoaim geometry to get lucky with.
    w.mc2_select_spell(24, 2, 0);
    let mut charmed = false;
    'volley: for _ in 0..10 {
        let Some(c) = w
            .debug_flock_probe(5, model)
            .into_iter()
            .find(|r| r.life > 0)
        else {
            break;
        };
        let (cx, cy) = (c.x as f32 / 256.0, c.y as f32 / 256.0);
        let alt = w.ground_height_tiles(cx, cy) + 0.5;
        let pose = PlayerPose::from_tiles(cx, alt, cy + 2.0, 0.0, 0.0, 0.0);
        w.tick(
            pose,
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        for _ in 0..12 {
            w.tick(pose, PlayerCommand::default());
            charmed = w.debug_flock_probe(5, model).iter().any(|r| r.hold == 14);
            if charmed {
                break 'volley;
            }
        }
    }
    assert!(charmed, "an alliance hit charms the species into 8m+7");
    assert!(
        life_sum(&w) >= before,
        "the charm deals no damage (the old (10,0) fire tag burned 400)"
    );
}

#[test]
fn mc2_volcano_boulders_are_not_cyclones() {
    // (10,16) volcano eruption boulders run `sub_32600` — a ballistic
    // rolling rock that bounces and lights (10,6) standing fires —
    // NOT the whirlwind driver `sub_33110` (trap: the boulder is
    // action 16 DECIMAL, not 0x16). The whirlwind path would add
    // cyclone sound 49 + sway + Whirlwind XP.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    let xp21 = w.mc2_book_view().xp[21];

    w.mc2_select_spell(18, 2, 0); // Volcano T3
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    let (mut boulders_seen, mut boulder_fire) = (false, false);
    for _ in 0..3000 {
        w.tick(pose, PlayerCommand::default());
        if count(&w, 10, 16) > 0 {
            boulders_seen = true;
            // A boulder-lit (10,6) fire runs on the short 30-tick
            // life (the ctor's own is 240).
            boulder_fire |= w
                .debug_pool()
                .1
                .iter()
                .any(|e| e.class == 10 && e.model == 6 && e.life > 0 && e.life <= 30);
        }
        assert_eq!(count(&w, 10, 22), 0, "no whirlwind head exists here");
        let frame = w.take_audio(pose);
        assert!(
            !frame.events.iter().any(|e| e.id == 49),
            "a boulder must not run the cyclone loop (sound 49)"
        );
        if boulders_seen && boulder_fire {
            break;
        }
    }
    assert!(boulders_seen, "the volcano actually erupted boulders");
    assert!(
        boulder_fire,
        "a landing boulder lights a 30-tick (10,6) fire"
    );
    assert_eq!(
        w.mc2_book_view().xp[21] - xp21,
        0,
        "volcano rocks award no Whirlwind XP"
    );
}

#[test]
fn mc2_meteor_flight_does_not_drag_the_fire_loop() {
    // The (9,3) meteor shot lays a decorative (10,0) fire spark EVERY
    // flight tick. Retail's fire-ambient loop latches only from the
    // persistent (10,6) BIG fire (`sub_31760` → `sub_5C870`,
    // EF:43602-14) — the small-fire trail must NOT arm `fire_near` and
    // drag the fire-crackle loop along the flight.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    w.mc2_select_spell(9, 0, 0); // Meteor T1
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    let mut saw_spark = false;
    for _ in 0..30 {
        w.tick(pose, PlayerCommand::default());
        saw_spark |= count(&w, 10, 0) > 0;
        let frame = w.take_audio(pose);
        assert!(
            !frame.fire_near,
            "the meteor's (10,0) spark trail must not arm the fire-ambient loop"
        );
    }
    assert!(saw_spark, "the flight actually laid (10,0) sparks");
}

#[test]
fn mc2_speed_window_interrupts_on_brake() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);

    // Cast Speed (3) tier 0 → arms the fixed-duration window and drives
    // the travel-speed override.
    w.mc2_select_spell(3, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert!(w.mc2_book_view().armed[3], "the Speed window is live");
    assert!(
        w.accel_override().is_some(),
        "the Speed boost overrides travel speed"
    );

    // A FORWARD press does not cancel the boost (only a brake does).
    //
    // ⚠ These worlds run the PINNED-pose `tick`, so no mover runs and
    // nothing arms `word_0xe_14`. The faithful path's cancel is now
    // retail's own two-step (the mover raises the flag, the token
    // collapses its window and restores the base) and is unit-tested
    // where it lives, in `flight::mc2_move`. What this exercises is the
    // ALTERNATE-mover entry point, which keeps the immediate form for
    // exactly the movers that never run a carpet dispatch.
    w.accel_brake_immediate(1.0);
    w.tick(pose, PlayerCommand::default());
    assert!(
        w.mc2_book_view().armed[3],
        "a forward thrust leaves the Speed window running"
    );

    // Braking INTERRUPTS the window. The window clears, the boost
    // drops, and because the burst timer `f26` is zeroed the mana-regen
    // suppression lifts with it (armed==false ⇒ f26==0).
    w.accel_brake_immediate(-1.0);
    w.tick(pose, PlayerCommand::default());
    assert!(
        !w.mc2_book_view().armed[3],
        "braking cancels the MC2 Speed window"
    );
    assert!(
        w.accel_override().is_none(),
        "braking stops the MC2 Speed boost"
    );
}

#[test]
fn mc2_speed_direction_follows_current_velocity() {
    // MC2's one Speed spell doubles as MC1's Accelerate AND Accelerate
    // Backwards: the boost direction is the caster's velocity sign at
    // the cast (`GetScroll_69DB0` EF:56212-15 — `speed_0xc_12 >= 0`
    // is forward, standstill included). The brake is the RESISTING
    // input for that direction.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let fire = PlayerCommand {
        fire_left: true,
        ..Default::default()
    };

    // Cast while flying BACKWARD → a backward boost.
    let back = PlayerPose {
        speed: -80,
        ..pose_at(&w, cx, cy)
    };
    w.mc2_select_spell(3, 0, 0);
    w.tick(back, fire);
    let over = w.accel_override().expect("the Speed boost is live");
    assert!(
        over < 0.0,
        "backward flight casts a backward boost ({over})"
    );

    // A further BACKWARD press does not cancel it...
    w.accel_brake_immediate(-1.0);
    w.tick(back, PlayerCommand::default());
    assert!(
        w.mc2_book_view().armed[3],
        "backward thrust rides along with a backward boost"
    );
    // ...the resisting (forward) input does.
    w.accel_brake_immediate(1.0);
    w.tick(back, PlayerCommand::default());
    assert!(
        !w.mc2_book_view().armed[3],
        "forward thrust brakes a backward boost"
    );
    assert!(w.accel_override().is_none());

    // Standstill counts as FORWARD (retail `>= 0`).
    let still = pose_at(&w, cx, cy);
    assert_eq!(still.speed, 0, "fixture pose is at standstill");
    w.mc2_select_spell(3, 0, 0);
    w.tick(still, fire);
    let over = w.accel_override().expect("the Speed boost is live");
    assert!(over > 0.0, "standstill casts a forward boost ({over})");
}

#[test]
fn mc2_enhanced_backward_flight_casts_a_backward_speed_boost() {
    // The enhanced pose must report SIGNED forward speed at the world
    // seam: flying backward reads negative, so the Speed direction law
    // (velocity sign, `GetScroll_69DB0` EF:56212-15) casts a BACKWARD
    // boost. The pose formerly carried the velocity MAGNITUDE, which
    // can never go negative — under enhanced flight the spell
    // propelled forward out of backward flight at every tier
    // (player-reported). The boost then drives with no key held (the
    // permanent-throttle law), until the RESISTING forward press
    // brakes it.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    w.mc2_select_spell(3, 0, 0);

    let mut sim = mgc_sim::Simulation::with_world(w);
    sim.thrust_model = mgc_sim::ThrustModel::Enhanced;
    sim.altitude_model = mgc_sim::AltitudeModel::ExtendedLift;
    sim.flyer.x = cx as f32 + 0.5;
    sim.flyer.z = cy as f32 + 0.5;
    let ground = sim
        .world
        .as_ref()
        .unwrap()
        .ground_height_tiles(sim.flyer.x, sim.flyer.z);
    sim.flyer.y = ground + 3.0;
    sim.sync_carpet_from_flyer();

    // Reverse thrust until the flyer genuinely drifts backward.
    let back = mgc_sim::FlightInput {
        thrust: -1.0,
        ..Default::default()
    };
    for _ in 0..30 {
        sim.step(&back);
    }
    let (sy, cyaw) = sim.flyer.yaw.sin_cos();
    let fwd_v = sim.flyer.vx * sy - sim.flyer.vz * cyaw;
    assert!(
        fwd_v < -0.5,
        "reverse thrust moves the flyer backward (forward speed {fwd_v})"
    );

    // Cast Speed while drifting backward (idle thrust on the cast
    // tick, so the resisting-input brake cannot eat the window).
    sim.step(&mgc_sim::FlightInput {
        fire_left: true,
        ..Default::default()
    });
    let over = sim
        .world
        .as_ref()
        .unwrap()
        .accel_override()
        .expect("the Speed boost is live");
    assert!(
        over < 0.0,
        "backward enhanced flight casts a backward boost ({over})"
    );

    // The boost keeps driving backward with NO key held...
    for _ in 0..10 {
        sim.step(&mgc_sim::FlightInput::default());
    }
    let (sy, cyaw) = sim.flyer.yaw.sin_cos();
    let fwd_v = sim.flyer.vx * sy - sim.flyer.vz * cyaw;
    assert!(fwd_v < -0.5, "the backward boost self-propels ({fwd_v})");
    // ...until the RESISTING (forward) press brakes it.
    sim.step(&mgc_sim::FlightInput {
        thrust: 1.0,
        ..Default::default()
    });
    sim.step(&mgc_sim::FlightInput::default());
    assert!(
        sim.world.as_ref().unwrap().accel_override().is_none(),
        "forward thrust brakes the backward boost"
    );
}

#[test]
fn mc2_earthquake_carves_without_flooding_the_pool() {
    // Earthquake (17) lays a travelling trail of (10,11) SCORCH RINGS
    // (the earth-carve, like a moving Crater) — NOT (10,19) ground-fire
    // sprays. The spray is a fire effect that spews (10,14) smoke every
    // odd tick, so a trail dropping one per tick over its 128-life
    // would FLOOD the entity pool and render as explosions. This pins
    // that the spell's entity footprint stays near the ambient
    // baseline, it lays scorch rings, and it spawns NO fire spray.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let live = |w: &World| w.debug_pool().1.iter().filter(|e| e.life >= 0).count();

    // Ambient baseline: same pose, no cast (level-000 has its own smoke
    // emitters, so absolute counts include ambient churn).
    let Some(mut w0) = build_world(&root) else {
        return;
    };
    w0.set_dev_spells(true);
    let (cx, cy) = open_spot(&w0);
    let pose = pose_at(&w0, cx, cy);
    let mut ambient_peak = live(&w0);
    for _ in 0..141 {
        w0.tick(pose, PlayerCommand::default());
        ambient_peak = ambient_peak.max(live(&w0));
    }

    // Cast Earthquake tier 2 (the longest trail) and track the peak.
    let mut w = build_world(&root).unwrap();
    w.set_dev_spells(true);
    w.mc2_select_spell(17, 2, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    let mut peak = live(&w);
    let mut saw_scorch = false;
    let mut saw_spray = false;
    for _ in 0..140 {
        w.tick(pose, PlayerCommand::default());
        peak = peak.max(live(&w));
        saw_scorch |= count(&w, 10, 11) > 0;
        saw_spray |= count(&w, 10, 19) > 0;
    }
    assert!(saw_scorch, "the trail lays (10,11) scorch-ring carves");
    assert!(!saw_spray, "the trail must NOT spawn (10,19) fire sprays");
    assert!(
        peak <= ambient_peak + 60,
        "Earthquake stays near ambient ({ambient_peak}); no entity flood (peak {peak})"
    );
}

#[test]
fn mc2_fools_mana_throws_six_decoys_that_trap_the_possessor() {
    // Fool's Mana (22) is a SHOTGUN of six neutral fake-mana decoys,
    // not one real collectible sphere. A non-owner possession claim
    // springs the tier retaliation: tier 0 fires ONE fireball at the
    // possessor and the decoy vanishes (docs/spell-audit/fools-mana.md).
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);

    // A fool's sphere is its OWN model — `sub_50130` builds a (10,57),
    // not a (10,39) (fools-mana.md OPEN-6). Real mana is counted apart
    // so the cast can be shown not to make any.
    let base = count(&w, 10, 57);
    let real_mana = count(&w, 10, 39);
    w.mc2_select_spell(22, 0, 0); // Fool's Mana tier 0, left hand
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(
        count(&w, 10, 57),
        base + 6,
        "the cast throws six fake-mana decoys, not one real sphere"
    );
    assert_eq!(
        count(&w, 10, 39),
        real_mana,
        "…and not one of them is a collectible (10,39) ball"
    );

    // A rival (a non-owner id) possession-claims one decoy → it springs.
    let slot = w.debug_mc2_claim_fool_sphere(12345);
    assert!(slot != 0, "a decoy is present to be claimed");
    let fb0 = count(&w, 9, 0); // class-9 subtype-0 = fireball
    w.tick(pose, PlayerCommand::default());
    assert_eq!(
        count(&w, 9, 0),
        fb0 + 1,
        "the claimed decoy fires exactly one fireball at the possessor"
    );
    assert_eq!(
        count(&w, 10, 57),
        base + 5,
        "the sprung (tier-0) decoy despawns after its single fireball"
    );
}

#[test]
fn mc2_authored_ground_sphere_is_a_tier0_trap() {
    // The AUTHORED (10,57) ground spheres are fool's mana too — retail's
    // `sub_36680` (EF:26615) has NO "was this cast" gate: its only
    // no-trap arm is `parentId == claimer`, and a level-load sphere
    // carries the NewEvent defaults parentId 0 / `byte_0x46_70` 0, i.e.
    // a live TIER-0 trap for every possessor. mc2l24 pins it: all 21
    // authored start spheres die the tick after the human's possess
    // pulse stamps the ch1 latch, each leaving a co-located (10,0) poof
    // and a (9,0) fireball homing the player (word_0x96_150 = 116).
    // Player-reported: the port handed them over as legit mana.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    let slot = w.debug_mc2_spawn_ground_sphere((cx << 8) | 128, (cy << 8) | 128);
    assert!(slot != 0, "the authored ground sphere spawned");
    let spheres = count(&w, 10, 57);
    // OPEN-6: the sphere wears retail's own model, and that model is
    // what keeps it off every collection law — the m23 siphon
    // (EF:18396), the balloon fleet (EF:61011), the castle absorb
    // (EF:61105) and the ball merge all test `model == 39`.
    assert_eq!(
        w.debug_pool().1.iter().filter(|e| e.slot == slot).count(),
        1,
        "the authored sphere is in the pool"
    );
    assert!(
        w.debug_pool()
            .1
            .iter()
            .any(|e| e.slot == slot && e.class == 10 && e.model == 57),
        "a NATIVE fool's sphere reads (10,57), not the (10,39) family"
    );

    // The level's own ambient fires make a global (10,0) census noisy —
    // count the poof on the SPHERE'S TILE only.
    let poofs = |w: &World| {
        w.debug_pool()
            .1
            .iter()
            .filter(|e| e.class == 10 && e.model == 0 && e.tx == cx as u8 && e.ty == cy as u8)
            .count()
    };

    // (a) an OWNER reclaim is a no-op: an authored sphere's parentId is
    //     its own slot, so claiming as that id takes retail's skip arm.
    let (fb0, poof0) = (count(&w, 9, 0), poofs(&w));
    w.debug_mc2_claim_sphere_at(slot, slot as u16);
    w.tick(pose, PlayerCommand::default());
    assert_eq!(count(&w, 9, 0), fb0, "the owner cannot spring its own trap");
    assert_eq!(poofs(&w), poof0, "an owner reclaim poofs nothing");
    assert_eq!(count(&w, 10, 57), spheres, "the sphere survives its owner");

    // (b) a NON-owner claim springs the tier-0 trap: exactly one
    //     fireball at the possessor, one (10,0) consume poof, and the
    //     sphere is gone — it is never handed over.
    let (fb0, poof0) = (count(&w, 9, 0), poofs(&w));
    w.debug_mc2_claim_sphere_at(slot, 12345);
    w.tick(pose, PlayerCommand::default());
    assert_eq!(
        count(&w, 9, 0),
        fb0 + 1,
        "the claimed ground sphere fires ONE fireball back at the claimer"
    );
    assert_eq!(
        poofs(&w),
        poof0 + 1,
        "the consumed sphere leaves retail's (10,0) poof (EF:26363)"
    );
    assert_eq!(
        count(&w, 10, 57),
        spheres - 1,
        "the sprung ground sphere is CONSUMED, not handed to the claimer"
    );
}

#[test]
fn mc2_fools_mana_tier2_retaliates_with_lightning() {
    // Tier 2/3 Fool's Mana answers a possession claim with a LIGHTNING
    // bolt (class-9 subtype 9), not a fireball (docs/spell-audit/
    // fools-mana.md §2b, `sub_36850`).
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);

    w.mc2_select_spell(22, 2, 0); // Fool's Mana tier 2
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    let slot = w.debug_mc2_claim_fool_sphere(12345);
    assert!(slot != 0, "a decoy is present to be claimed");
    let (fb0, lb0) = (count(&w, 9, 0), records(&w, 9, 9));
    w.tick(pose, PlayerCommand::default());
    // The thunder bolt (subtype 9) fires the L0 beam, which flashes a
    // trail of (9,9) billboards — a large jump uniquely marks lightning.
    // Records, not live: the trail is born-dead (see `records`).
    assert!(
        records(&w, 9, 9) > lb0,
        "the tier-2 decoy answers with a lightning bolt (flash), not silence"
    );
    assert_eq!(count(&w, 9, 0), fb0, "tier 2 does NOT fire a fireball");
}

#[test]
fn mc2_magic_mine_blast_reaches_a_neighbouring_wizard() {
    // The mine's detonation must actually REACH someone standing next
    // to it. Player-reported: it did not. `ent_overlap` sums BOTH
    // parties' extents and the mine ctor never set f80/f82/f84, so the
    // blast was a POINT — and once the mine started hovering 1024 above
    // ground (EF:29862-72) even standing on the spot could not overlap
    // it. The detonation now opens a real blast box and spits a bolt at
    // whatever tripped it.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    // The working mine is the `mc2_magic_mine` patched arm (retail
    // ships the trigger dead — DEVIATIONS.md).
    w.set_patches(mgc_sim::WorldPatches::LEGACY);
    let (cx, cy) = open_spot(&w);
    // Burn off SPAWN GRACE far from the mine site first — grace absorbs
    // the blast and makes a working mine look broken.
    let (fx, fz) = (cx as f32 + 60.5, cy as f32 + 60.5);
    for _ in 0..400 {
        let alt = w.ground_height_tiles(fx, fz) + 4.0;
        w.tick(
            PlayerPose::from_tiles(fx, alt, fz, 0.0, 0.0, 0.0),
            PlayerCommand::default(),
        );
    }
    assert_eq!(
        w.vitals().grace,
        0,
        "grace must be gone or this proves nothing"
    );

    // A RIVAL-owned mine, so the human is a valid victim.
    assert!(w.debug_mc2_place_mine(cx, cy, 0, 7) != 0, "mine placed");
    let before = w.player_damage_taken();
    let (px, pz) = (cx as f32 + 2.5, cy as f32 + 0.5);
    for _ in 0..300 {
        let alt = w.ground_height_tiles(px, pz) + 4.0;
        w.tick(
            PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0),
            PlayerCommand::default(),
        );
        if w.player_damage_taken() > before {
            break;
        }
    }
    assert!(
        w.player_damage_taken() > before,
        "a wizard two tiles from a tripped mine takes damage"
    );
}

#[test]
fn mc2_magic_mine_places_a_persistent_mine_not_a_fireball() {
    // Magic Mine (23) lands a persistent (10,78) proximity mine ahead
    // of the caster — not a fireball that bursts on first contact. With
    // no enemy in range it arms and just sits there
    // (docs/spell-audit/magic-mine.md).
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    assert_eq!(count(&w, 10, 78), 0, "no mines at level start");

    w.mc2_select_spell(23, 0, 0); // Magic Mine tier 0
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    // Let the carrier fly forward and land (~15-tile maxLife fuse).
    for _ in 0..30 {
        w.tick(pose, PlayerCommand::default());
    }
    assert_eq!(
        count(&w, 10, 78),
        1,
        "the carrier placed exactly one persistent mine"
    );
    // It persists through the arm delay with no target in range.
    for _ in 0..120 {
        w.tick(pose, PlayerCommand::default());
    }
    assert_eq!(
        count(&w, 10, 78),
        1,
        "the mine persists with no enemy nearby (no contact-detonate)"
    );
    // ...AND IS DRAWN. Player-reported: the mine ticked, armed and
    // detonated correctly but (10,78) was missing from the MC2 class-10
    // draw allowlist, so a cast looked like a carrier that flew off and
    // dissolved. Counting entities cannot see that — only a pose can.
    assert!(
        w.live_poses().iter().any(|p| p.type_index == 66),
        "the placed mine exports a sprite pose (ctor sprite 66)"
    );
}

#[test]
fn mc2_magic_mine_detonates_when_a_target_approaches() {
    // The proximity trigger: a mine detonates (despawns + bursts) when a
    // wizard comes within 14 tiles after the arm delay. Placed as a
    // RIVAL-owned mine right where the human sits → it triggers on the
    // out-of-pool human (docs/spell-audit/magic-mine.md §2).
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    // The proximity trigger only exists under the `mc2_magic_mine`
    // patched arm (retail ships it dead — DEVIATIONS.md).
    w.set_patches(mgc_sim::WorldPatches::LEGACY);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    let slot = w.debug_mc2_place_mine(cx, cy, 0, 7); // owner = rival id 7
    assert!(slot != 0, "the mine was placed");
    assert_eq!(count(&w, 10, 78), 1);

    let mut detonated = false;
    for _ in 0..90 {
        w.tick(pose, PlayerCommand::default());
        if count(&w, 10, 78) == 0 {
            detonated = true;
            break;
        }
    }
    assert!(
        detonated,
        "the mine detonates while the human sits inside its 14-tile trigger"
    );
}

#[test]
fn mc2_fools_mana_decoys_do_not_count_toward_world_mana() {
    // The fake decoys carry a random mana value for the disguise, but you
    // can never trip your OWN trap to reclaim them — so they must NOT
    // inflate the world-mana denominator, or their uncollectable share
    // would dilute the castle-share goal below reachability
    // (docs/spell-audit/fools-mana.md).
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    w.tick(pose, PlayerCommand::default()); // settle the mana census
    let before = w.loadout().world_mana;

    w.mc2_select_spell(22, 0, 0); // Fool's Mana tier 0
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    w.tick(pose, PlayerCommand::default()); // recompute the census
    let decoys = count(&w, 10, 57);
    assert!(decoys >= 6, "six decoys exist, got {decoys}");
    let after = w.loadout().world_mana;
    // Six decoys carry up to 6×1999 ≈ 12000 fake mana; excluded, the
    // denominator barely moves (a decoy would add thousands each).
    assert!(
        after <= before + 1999,
        "decoys must not inflate world-mana (before {before}, after {after})"
    );
}

#[test]
fn mc2_metamorph_transforms_and_reverts() {
    // Metamorph (4): the caster becomes a pooled class-5 creature (model
    // 19 on non-Day) slaved to the player pose, carpet hidden; the
    // transform reverts (creature despawns, carpet returns) at the cast
    // window expiry (docs/spell-audit/summon-creatures.md Part A).
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    let base = count(&w, 5, 19);
    assert_eq!(w.mc2_metamorph_model(), 0, "not transformed at start");

    w.mc2_select_spell(4, 0, 0); // Metamorph tier 0 → model 19 (non-Day)
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(w.mc2_metamorph_model(), 19, "transformed into model 19");
    assert_eq!(
        count(&w, 5, 19),
        base + 1,
        "one metamorph creature spawned (the pose-puppet)"
    );

    // Ride out the cast window (tier-0 duration 201 ticks) → revert.
    for _ in 0..260 {
        w.tick(pose, PlayerCommand::default());
    }
    assert_eq!(w.mc2_metamorph_model(), 0, "reverted after the window");
    assert_eq!(count(&w, 5, 19), base, "the pose-puppet despawned");
}

#[test]
fn mc2_summon_army_spawns_an_allied_ring() {
    // Summon Army (19): the carrier lands and spawns a ring of allied
    // class-5 creatures (8 fireflies at tier 0 on non-Day), owned by the
    // caster (docs/spell-audit/summon-creatures.md Part B).
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = open_spot(&w);
    let pose = pose_at(&w, cx, cy);
    let base = count(&w, 5, 19);

    w.mc2_select_spell(19, 0, 0); // Summon Army tier 0 → firefly (19) ×8
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    // Let the carrier fly and land, then the ring appears.
    let mut peak = base;
    for _ in 0..40 {
        w.tick(pose, PlayerCommand::default());
        peak = peak.max(count(&w, 5, 19));
    }
    assert!(
        peak >= base + 2,
        "the carrier spawned an allied creature ring (peak {peak}, base {base})"
    );
}

#[test]
fn mc2_earthquake_travel_scales_with_tier() {
    // The earthquake trail's travel distance scales with the spell
    // level (~2× per tier): life_0x1A {16,32,64} = the trail life 1×
    // (sub_66160 EF:63333-35 — the 8× law is whirlwind's alone). The
    // (10,15) trail persists for its life as it travels, so its total
    // presence is a proxy for reach.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    // Read the trail's remaining LIFE the tick it first appears — the
    // travel is life × step, and reading life avoids the terrain
    // water-gate (`f26 > 8`) that can cut travel short in a wet spot.
    let trail_life = |tier: u8| -> i32 {
        let mut w = build_world(&root).unwrap();
        w.set_dev_spells(true);
        let (cx, cy) = open_spot(&w);
        let pose = pose_at(&w, cx, cy);
        w.mc2_select_spell(17, tier, 0); // Earthquake
        w.tick(
            pose,
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        for _ in 0..60 {
            w.tick(pose, PlayerCommand::default());
            if let Some(e) = w
                .debug_pool()
                .1
                .iter()
                .find(|e| e.class == 10 && e.model == 15 && e.life >= 0)
            {
                return e.life;
            }
        }
        0
    };
    let (l0, l2) = (trail_life(0), trail_life(2));
    // life_0x1A {16,64}: tier 2 lives ~4× longer.
    assert!(
        l0 > 0 && l2 >= l0 * 2,
        "tier-2 earthquake trail lives much longer than tier 0 (l0={l0}, l2={l2})"
    );
}

#[test]
fn mc2_quake_family_lifetimes_scale_with_tier() {
    // The action wrappers stamp per-tier LIVES onto the ground
    // effects — Crater `sub_66280` life = charge {6,12,24}; Gravity
    // Well `sub_677A0` life = charge {16,26,40}; Tremor `sub_677D0`
    // BOTH lives = charge & 0xF0 {48,80,112}.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    // A cast spot with a DRY LANE ahead (north): the tremor carrier
    // (model 23) has no water exemption — a wet tile under the
    // descent is a faithful splash-fizzle, not an impact.
    let dry_lane_spot = |w: &World| -> (u16, u16) {
        let p = w.planes();
        let dry = |cx: u16, cy: u16| {
            let t = (cy as usize % 256) * 256 + (cx as usize % 256);
            p.angle[t] & 0x80 == 0 && p.angle[t] & 0xF != 0
        };
        for cy in (24..222u16).step_by(3) {
            for cx in (24..232u16).step_by(3) {
                if (0..8).all(|d| dry(cx, cy.wrapping_sub(d))) {
                    return (cx, cy);
                }
            }
        }
        panic!("no dry lane on the level");
    };
    let effect_life = |spell: u8, tier: u8, model: u8, pitch: f32| -> i32 {
        let mut w = build_world(&root).unwrap();
        w.set_dev_spells(true);
        let (cx, cy) = dry_lane_spot(&w);
        let (px, pz) = (cx as f32 + 0.5, cy as f32 + 0.5);
        let alt = w.ground_height_tiles(px, pz) + 2.0;
        // Quake/crater carriers hug the ground at any pitch; the
        // gravity-well/tremor carriers FLY — pitch them into the
        // terrain so the impact spawns inside the scan window.
        let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, pitch, 0.0);
        w.mc2_select_spell(spell, tier, 0);
        w.tick(
            pose,
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        for _ in 0..60 {
            w.tick(pose, PlayerCommand::default());
            if let Some(e) = w
                .debug_pool()
                .1
                .iter()
                .find(|e| e.class == 10 && e.model == model && e.life >= 0)
            {
                return e.life;
            }
        }
        0
    };
    // Crater (16) → (10,11): 6 vs 24 (read on the first visible tick,
    // so allow the one-tick decay slack).
    let (c0, c2) = (effect_life(16, 0, 11, 0.0), effect_life(16, 2, 11, 0.0));
    assert!(
        c0 > 0 && c0 <= 6 && c2 > 3 * c0,
        "crater life is the tier charge 6/24 (c0={c0}, c2={c2})"
    );
    // Gravity Well (20) → (10,67): 16 vs 40.
    let (g0, g2) = (effect_life(20, 0, 67, -0.6), effect_life(20, 2, 67, -0.6));
    assert!(
        g0 > 0 && g0 <= 16 && g2 >= 2 * g0,
        "gravity well life is the tier charge 16/40 (g0={g0}, g2={g2})"
    );
    // Tremor (15) → (10,71): 48 vs 112 (charge & 0xF0).
    let (t0, t2) = (effect_life(15, 0, 71, -0.6), effect_life(15, 2, 71, -0.6));
    assert!(
        t0 > 0 && t0 <= 48 && t2 > 2 * t0,
        "tremor life is charge & 0xF0 = 48/112 (t0={t0}, t2={t2})"
    );
}

#[test]
fn mc2_spell_select_raises_notification_toast() {
    // The change-spell path (EF:37925) raises the top-of-screen
    // notification with the chosen TIER's own name, on a 20-frame life
    // (the presentation surface — hash-excluded, so the goldens never
    // see it). Selecting Possession tier 1 must toast "Mana Magnet"
    // (its distinct per-tier hint name), then decay to nothing.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);

    assert!(w.notification().is_none(), "no toast at level start");

    w.mc2_select_spell(1, 1, 0); // Possession tier 1
    let want = w.mc2_spell_name(1, 1).to_string();
    assert!(!want.is_empty(), "the tier-1 name resolves from L1.TXT");
    let (text, color) = w.notification().expect("select raises a toast");
    assert_eq!(text, want, "the toast is the chosen tier's spell name");
    assert_eq!(color, [255, 0, 0], "plain toasts are red");

    // The 20-frame select life decays on the app-driven wall-clock
    // frame cadence — never the sim tick, so game speed cannot
    // stretch or blink it — and clears.
    w.age_notification(19);
    assert!(w.notification().is_some(), "toast still live before expiry");
    w.age_notification(1);
    assert!(
        w.notification().is_none(),
        "toast cleared after its 20-frame life"
    );
}

#[test]
fn mc2_rebound_deflects_and_reowns() {
    // Rebound (spell 8, `sub_68740` EF:55221-310): a hostile bolt
    // striking the shielded player is thrown BACK — re-owned to the
    // player, heading reversed, life refilled — instead of hitting.
    // T1/T2 scatter ±22 around the reverse ray; T3 (PRECISE) returns
    // it EXACTLY reversed.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    for (tier, precise) in [(0u8, false), (2u8, true)] {
        let Some(mut w) = build_world(&root) else {
            return;
        };
        w.set_dev_spells(true);
        let (cx, cy) = open_spot(&w);
        let pose = pose_at(&w, cx, cy);
        let xp8 = w.mc2_book_view().xp[8];

        w.mc2_select_spell(8, tier, 0);
        w.tick(
            pose,
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        assert!(w.mc2_book_view().armed[8], "the Rebound window is live");

        // A hostile bolt from 3 tiles north, flying south (0x400)
        // straight at the player.
        let (bx, by) = (pose.x, pose.y.wrapping_sub(3 * 256));
        let slot = w.debug_mc2_hostile_bolt(bx, by, pose.z, 0x400, 900);
        assert!(slot != 0, "the hostile bolt spawned");

        let mut row = None;
        for _ in 0..12 {
            w.tick(pose, PlayerCommand::default());
            let Some(r) = w
                .debug_flock_probe(9, 0)
                .into_iter()
                .find(|r| r.slot == slot)
            else {
                break; // detonated/expired — no deflection happened
            };
            if r.id24 == 0xFFFF {
                row = Some(r); // re-owned to the player = deflected
                break;
            }
        }
        let row = row.unwrap_or_else(|| panic!("T{} bolt re-owns to the player", tier + 1));
        // Reverse ray = 0x400 + 0x400 = 0 (north). f34 carries it.
        assert_eq!(
            row.aim, 0,
            "the reverse ray points back at the shooter side"
        );
        if precise {
            assert_eq!(
                row.yaw, 0,
                "T3 PRECISE returns exactly down the reverse ray"
            );
        } else {
            let dev = (row.yaw as i32 + 22) & 0x7FF;
            assert!(dev <= 44, "T1 scatter stays within ±22 of the reverse ray");
        }
        assert_eq!(
            w.mc2_book_view().xp[8] - xp8,
            2, // +1 the cast itself, +1 the deflection (EF:55283)
            "the deflection awards Rebound XP"
        );
    }
}

#[test]
fn mc2_whirlwind_duration_law_8x_tier_life() {
    // Whirlwind/Tornado (spell 21) head lifetime = 8 × the tier's
    // `life_0x1A` (`sub_678E0` EF:59202-16), with SPELLS.DAT row 21
    // lives {5, 10, 10} — so retail T3 lasts exactly as long as T2 BY
    // DESIGN (the T3 lever is per-tick damage 240/10, not duration).
    // The law: 40 / 80 / 80 ticks.
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    for (tier, expect) in [(0u8, 40i32), (1, 80), (2, 80)] {
        let Some(mut w) = build_world(&root) else {
            return;
        };
        w.set_dev_spells(true);
        let (cx, cy) = open_spot(&w);
        let pose = pose_at(&w, cx, cy);
        w.mc2_select_spell(21, tier, 0);
        w.tick(
            pose,
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        // Let the (9,26) seed land and hatch the (10,22) head.
        let mut head_life = 0;
        for _ in 0..60 {
            w.tick(pose, PlayerCommand::default());
            if let Some(h) = w
                .debug_pool()
                .1
                .iter()
                .find(|e| e.class == 10 && e.model == 22 && e.life >= 0)
            {
                head_life = head_life.max(h.life);
            }
        }
        assert_eq!(
            head_life,
            expect - 1, // observed after its first countdown tick
            "T{} whirlwind head runs 8×life = {expect} ticks",
            tier + 1
        );
    }
}
