//! The MC2-NATIVE CASTLE COLUMN over real baked level-000 data
//! (mc2::castle — retail actions 4/5/6): the NATIVE Create-Castle cast
//! (spell 2) must raise a class-3 m2 castle that runs the MC2
//! machinery — the 19-tick (10,42) painter stamps the tower, the MC2
//! capacity ladder rungs
//! (8500/18000 — NOT MC1's 10000/20000) prove the game-keyed swap,
//! the (10,43) token recast upgrades one level, the (3,3) balloon
//! fleet spawns to quota, and demolish walks the level back down to
//! a barren, unprotected square.
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

/// First tile whose neighborhood — including the cast site 16 tiles
/// south — is dry and free of the building-protection bit (the same
/// scan as the MC1 castle test).
fn clear_spot(w: &World) -> (u16, u16) {
    let p = w.planes();
    for cy in (24..222u16).step_by(3) {
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
            return (cx, cy);
        }
    }
    panic!("no clear 19x19 spot on the level");
}

fn count(w: &World, class: u8, model: u8) -> usize {
    w.debug_pool()
        .1
        .iter()
        .filter(|e| e.class == class && e.model == model && e.life >= 0)
        .count()
}

/// A tier-1 castle cast must grow FIRE turrets — the (10,79) ring with
/// part-type 1 — via the cast-time research stamp; the dev-granted
/// spell must behave exactly like a legitimately leveled one.
#[test]
fn mc2_castle_tier1_cast_grows_fire_turrets() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = clear_spot(&w);
    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);

    // Bind the castle spell at TIER 1 (fire) and build.
    w.mc2_select_spell(2, 1, 0);
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
    let (_, _, lvl) = w.loadout().castle.expect("castle stands");
    assert_eq!(lvl, 1, "level-1 castle built");
    assert_eq!(
        count(&w, 10, 79),
        1,
        "the tier-1 build grows the stage-1 turret"
    );

    // Recast = upgrade to level 2: the 4-corner ring.
    w.tick(pose, PlayerCommand::default());
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    for _ in 0..220 {
        w.tick(pose, PlayerCommand::default());
    }
    let (_, _, lvl2) = w.loadout().castle.expect("castle survives");
    assert_eq!(lvl2, 2, "upgraded to level 2");
    assert_eq!(count(&w, 10, 79), 4, "level 2 grows the 4-turret ring");
}

/// The level's ending cluster is the CHECKPOINT variant — dis 4 spawns
/// the (11,12) X-marker trigger at (75,218) and the (14,3) fly-to
/// "X"/portal at (97,221) (there is NO (11,31)/(14,4) on level-000;
/// retail routes this through the same endGameSeq under actionIndex
/// 12). The marker must spawn HIDDEN, the trip must REVEAL it and
/// seize the flyer, and the fly-in must end in WON.
#[test]
fn mc2_level000_ending_end_to_end() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.debug_fire_disposition(4);
    w.tick(
        PlayerPose::from_tiles(10.0, 5.0, 10.0, 0.0, 0.0, 0.0),
        PlayerCommand::default(),
    );
    let pool = w.debug_pool().1;
    let trig = pool
        .iter()
        .find(|e| e.class == 11 && e.model == 12 && e.life >= 0)
        .expect("dis 4 spawns the (11,12) ending trigger");
    let marker = pool
        .iter()
        .find(|e| e.class == 14 && e.model == 3 && e.life >= 0)
        .expect("dis 4 spawns the (14,3) fly-to marker");
    assert!(
        !w.live_poses().iter().any(|p| p.class == 14 && p.model == 3),
        "the ending marker spawns HIDDEN (not drawable) until the trip"
    );
    eprintln!(
        "trigger at ({},{}), marker at ({},{})",
        trig.tx, trig.ty, marker.tx, marker.ty
    );
    // Park on the trigger; the 8-tick phase gate opens quickly.
    let (tx, ty) = (trig.tx as f32 + 0.5, trig.ty as f32 + 0.5);
    let alt = w.ground_height_tiles(tx, ty) + 1.0;
    let pose = PlayerPose::from_tiles(tx, alt, ty, 0.0, 0.0, 0.0);
    let mut seized_at = None;
    for t in 0..40 {
        w.tick(pose, PlayerCommand::default());
        if w.mc2_end_pose().is_some() {
            seized_at = Some(t);
            break;
        }
    }
    assert!(
        seized_at.is_some(),
        "the trigger trip seizes the flyer (endGameSeq installs)"
    );
    assert!(
        w.live_poses().iter().any(|p| p.class == 14 && p.model == 3),
        "the trip REVEALS the fly-to marker"
    );
    assert!(!w.won(), "the trip alone must not end the level");
    let mut won_at = None;
    for t in 0..2000 {
        w.tick(pose, PlayerCommand::default());
        if w.won() {
            won_at = Some(t);
            break;
        }
    }
    assert!(won_at.is_some(), "the fly-in ends the level (won)");
    let (ex, _, ez, _) = w.mc2_end_pose().expect("pose holds through the end");
    let d = ((ex - (marker.tx as f32)).powi(2) + (ez - (marker.ty as f32)).powi(2)).sqrt();
    assert!(
        d < 4.0,
        "the scripted carpet stopped at the marker (dist {d:.1} tiles)"
    );
}

#[test]
fn mc2_castle_builds_upgrades_and_demolishes() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);

    let (cx, cy) = clear_spot(&w);
    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);

    // Snapshot the target region's planes.
    let region: Vec<usize> = (-8i32..=8)
        .flat_map(|dy| {
            (-8i32..=8).map(move |dx| {
                ((cy as i32 + dy) as usize % 256) * 256 + ((cx as i32 + dx) as usize % 256)
            })
        })
        .collect();
    let snap: Vec<(u8, u8, u8)> = region
        .iter()
        .map(|&t| {
            (
                w.planes().height[t],
                w.planes().tile_type[t],
                w.planes().angle[t],
            )
        })
        .collect();

    // The NATIVE castle cast: bind the MC2 castle spell (index 2,
    // dev-granted above) — the MC1 equip bridge does NOT cast on the
    // MC2 column (else ghost fireballs).
    w.mc2_select_spell(2, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    w.tick(pose, PlayerCommand::default());
    assert_eq!(count(&w, 9, 10), 1, "the cast launched the castle ball");

    // Ball flight + level-up + the 19-tick MC2 painter + settle.
    let mut saw_castle = false;
    for _ in 0..110 {
        w.tick(pose, PlayerCommand::default());
        saw_castle |= count(&w, 3, 2) > 0;
    }
    assert!(saw_castle, "the ball landing raised the class-3 m2 castle");
    let changed = region.iter().zip(&snap).any(|(&t, &(h, ty, a))| {
        w.planes().height[t] != h || w.planes().tile_type[t] != ty || w.planes().angle[t] != a
    });
    assert!(
        changed,
        "the MC2 (10,42) painter stamped the castle footprint"
    );

    // THE LADDER DISCRIMINATOR: MC2 level-1 capacity = 8500
    // (sub_60810 EF:61710) — MC1's rung is 10000. A bridge castle
    // still running the MC1 column would report 10000 here.
    let (_, cap1, lvl1) = w.loadout().castle.expect("castle panel data");
    assert_eq!(
        (lvl1, cap1),
        (1, 8_500),
        "level 1 with the MC2 capacity rung"
    );

    // The balloon fleet: level-1 quota = 1 (sub_60400 EF:61529) —
    // the roster pass spawns it from the standing tick.
    assert_eq!(count(&w, 3, 3), 1, "one (3,3) balloon at level 1");

    // RECAST = the upgrade: the ball morphs into the (10,43) token
    // at the castle, the token mails the upgrade-request channel
    // (retail word_0x80_128/word_0x7C_124 — sub_389F0 EF:28240),
    // the castle re-runs the level-up arm.
    w.tick(pose, PlayerCommand::default()); // release the button
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(count(&w, 9, 10), 1, "the recast launches the upgrade ball");
    for _ in 0..200 {
        w.tick(pose, PlayerCommand::default());
    }
    let (_, cap2, lvl2) = w.loadout().castle.expect("castle survives the upgrade");
    assert_eq!(
        (lvl2, cap2),
        (2, 18_000),
        "the token upgrade raised level 2 on the MC2 ladder"
    );

    // Climb to level 6 (the 48x48 stage) — four more recasts. The
    // even-frame/odd-row origin math: retail origin = D/2 - d/2
    // (EF:27798), NOT (D-d)/2 — the wrong read shifts every interior
    // ring one tile toward -x/-y (offset walkways, a squashed center
    // tower, and castle guards spawning inside wall cells where the
    // all-four-blocked walker law kills them in a respawn loop).
    for expect in 3..=6i16 {
        w.tick(pose, PlayerCommand::default());
        w.tick(
            pose,
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        for _ in 0..220 {
            w.tick(pose, PlayerCommand::default());
        }
        let (_, _, lvl) = w.loadout().castle.expect("castle survives the upgrade");
        assert_eq!(lvl, expect as u8, "recast raised level {expect}");
    }
    let (_, cap6, _) = w.loadout().castle.expect("castle at level 6");
    assert_eq!(cap6, 317_400, "the MC2 level-6 capacity rung");
    // Each upgrade awards +1 castle XP (`sub_6D8B0(owner,2,1)`
    // EF:61596) — five upgrades landed above (2..=6), so the ladder
    // that unlocks Fire/Lightning Tower tiers has climbed, and the XP
    // drain's spell-2 branch keeps the pane cost synced.
    let book = w.mc2_book_view();
    assert!(
        book.xp[2] >= 5,
        "castle upgrades awarded spell-2 XP (got {})",
        book.xp[2]
    );
    // The guard roster survives on the (correctly aligned) walkways —
    // a one-tile ring offset would block them on all four sides and
    // kill them as fast as they spawn.
    for _ in 0..200 {
        w.tick(pose, PlayerCommand::default());
    }
    assert!(
        count(&w, 5, 15) >= 3,
        "castle guards survive on the level-6 walkways (got {})",
        count(&w, 5, 15)
    );

    // Demolish walks ONE level down per press (life = -1 → intake 2
    // → action 6 → sub_605E0), all the way to a barren unprotected
    // square (RemoveCastleStage model-0 arm).
    let castle = w
        .debug_pool()
        .1
        .into_iter()
        .find(|e| e.class == 3 && e.model == 2)
        .expect("castle entity lives");
    let (tx, ty) = (castle.tx as i32, castle.ty as i32);
    w.tick(
        pose,
        PlayerCommand {
            demolish: true,
            ..Default::default()
        },
    );
    for _ in 0..60 {
        w.tick(pose, PlayerCommand::default());
    }
    let (_, cap_d, lvl_d) = w.loadout().castle.expect("castle survives one demolish");
    assert_eq!(
        (lvl_d, cap_d),
        (5, 158_200),
        "one demolish = one level down (the MC2 downgrade)"
    );
    for _ in 0..5 {
        w.tick(
            pose,
            PlayerCommand {
                demolish: true,
                ..Default::default()
            },
        );
        for _ in 0..60 {
            w.tick(pose, PlayerCommand::default());
        }
    }
    assert_eq!(count(&w, 3, 2), 0, "the level-1 demolish killed the castle");
    for dy in -4i32..=4 {
        for dx in -4i32..=4 {
            let t = ((ty + dy) as usize % 256) * 256 + ((tx + dx) as usize % 256);
            assert_eq!(
                w.planes().angle[t] & 0x80,
                0,
                "no protection bit lingers at ({dx},{dy})"
            );
        }
    }
}

/// The mana gate reads the manifestation's CACHED cost (`max_life`,
/// written by SetSpell_6D5E0), and retail refreshes that cache on
/// EVERY castle stat stamp — `sub_60780` (EF:61670) re-runs SetSpell
/// on the manifestation's own tier from both transform directions.
/// The upgrade path stays fresh via the +1 XP award's spell-2 branch,
/// but a DOWNGRADE (demolish / enemy razing) awards nothing, so the
/// cost cache must re-sync at the upgrade-lock release edge — else an
/// affordable rebuild dings (sound 29) against the stale higher rung
/// until the spell is re-selected.
///
/// The assert surface is the CACHE (`debug_spell_gate_cost`) against
/// the live law (`mc2_book_view().cost`) — the bug is exactly their
/// divergence. (A full end-to-end cast can't be driven here: the
/// per-tick mana census re-derives the pool ceiling from claimed
/// world mana, and this harness world has none to claim.)
#[test]
fn mc2_castle_cost_refreshes_on_downgrade() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);

    let (cx, cy) = clear_spot(&w);
    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);

    // Build to level 2 under the dev instrument (gate bypassed).
    w.mc2_select_spell(2, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    for _ in 0..110 {
        w.tick(pose, PlayerCommand::default());
    }
    w.tick(pose, PlayerCommand::default());
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    for _ in 0..220 {
        w.tick(pose, PlayerCommand::default());
    }
    let (_, _, lvl) = w.loadout().castle.expect("castle stands");
    assert_eq!(lvl, 2, "harness built to level 2");

    // Real-mana mode; the re-select recomputes the honest level-2
    // cache: the NEXT build (level 3) = ladder rung 20000.
    w.set_dev_spells(false);
    w.mc2_select_spell(2, 0, 0);
    assert_eq!(
        w.debug_spell_gate_cost(2),
        Some(20_000),
        "at level 2 the cached gate cost is the level-3 rung"
    );

    // A failed cast attempt (the ding) — it must not perturb the cache
    // or wedge any state.
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(count(&w, 9, 10), 0, "unaffordable: no castle ball");
    w.tick(pose, PlayerCommand::default()); // release the button

    // Demolish one level; the downgrade transform raises and then
    // releases the upgrade lock — the sub_60780 cost re-sync rides
    // the release edge.
    w.tick(
        pose,
        PlayerCommand {
            demolish: true,
            ..Default::default()
        },
    );
    for _ in 0..90 {
        w.tick(pose, PlayerCommand::default());
    }
    let (_, _, lvl) = w.loadout().castle.expect("castle survives the demolish");
    assert_eq!(lvl, 1, "one demolish = one level down");

    // The cache must track the live law back DOWN to the level-2 rung
    // with NO re-select in between (a stale higher rung would ding an
    // affordable rebuild).
    assert_eq!(
        w.mc2_book_view().cost[2],
        10_000,
        "the live law prices the level-2 rebuild at the level-1 rung"
    );
    assert_eq!(
        w.debug_spell_gate_cost(2),
        Some(10_000),
        "the cached gate cost re-synced on the downgrade (no re-select)"
    );
}

/// The MC2 face of the `castle_recast_cost` patch (DEVIATIONS.md
/// first-castle lockout). CORRECTED 2026-08-22 by the mc2l3 corpus
/// (t=265): total castle DEATH walks the destroy's DOWNGRADE, and
/// `sub_605E0` → `sub_60810` runs the level-0 ladder rung THROUGH
/// `sub_60780` — the gate-suppressed SetSpell re-prices the token at
/// the level-0 rung (base 1000) BEFORE the record frees. "Death never
/// re-stamps" was the same mis-reading the MC1 half of the entry shed
/// on 2026-08-10 (its teardown stamps CAP[0]). Both arms therefore
/// converge on a ladder-stamped death; the patch's remaining scope is
/// the castle-less RELEASE re-sync (a release edge with no castle and
/// no ladder stamp in between). One world per arm.
fn mc2_castle_death_cost_arm(patched: bool) {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    if patched {
        w.set_patches(mgc_sim::WorldPatches {
            castle_recast_cost: true,
            ..mgc_sim::WorldPatches::RETAIL
        });
    }
    w.set_dev_spells(true);

    let (cx, cy) = clear_spot(&w);
    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);

    // Build to level 1 under the dev instrument.
    w.mc2_select_spell(2, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    for _ in 0..160 {
        w.tick(pose, PlayerCommand::default());
    }
    let (_, _, lvl) = w.loadout().castle.expect("castle stands");
    assert_eq!(lvl, 1, "harness built to level 1");

    // Real-mana mode; the re-select recomputes the honest level-1
    // cache: the NEXT build (level 2) = ladder rung 10000.
    w.set_dev_spells(false);
    w.mc2_select_spell(2, 0, 0);
    assert_eq!(w.debug_spell_gate_cost(2), Some(10_000));

    // Demolish level 1 -> total destruction, castle-less.
    w.tick(
        pose,
        PlayerCommand {
            demolish: true,
            ..Default::default()
        },
    );
    for _ in 0..90 {
        w.tick(pose, PlayerCommand::default());
    }
    assert!(w.loadout().castle.is_none(), "the demolish razed it");

    // THE +3000 RE-CAST SURCHARGE APPLIES ON BOTH ARMS. This harness
    // demolishes at LEVEL 1 EXACTLY, the one gate that latches
    // retail's `byte_0x1BE_446` (EF:37993-95), and the caster is now
    // castle-less — so `GetSpellManaCost_6D710` (L:1723-26) adds 3000
    // to the tier's base 1000.
    // ⚠ The arms deliberately AGREE here (player-ruled 2026-08-23c).
    // `castle_recast_cost` relieves MC1's first-castle LOCKOUT, which
    // is an unpatched retail BUG; the MC2 surcharge is DESIGNED
    // behaviour — MC2 re-prices an enemy-destroyed castle at 1000 and
    // taxes only the demolish you chose. Putting design behind a
    // bug-relief switch would make the patched arm less faithful for
    // no gameplay reason, so the surcharge does not fork.
    let want = 4_000;
    assert_eq!(
        w.mc2_book_view().cost[2],
        want,
        "the castle-less rebuild price ({} arm)",
        if patched { "patched" } else { "retail" }
    );
    // Both arms: the death's own downgrade ladder stamped the level-0
    // rung through the suppressed re-sync (mc2l3 t=265: retail token
    // 10000/99 -> 1000/9 the tick the castle fell) — now priced
    // against the MAILING castle, so the deferral to the post-walk
    // drain no longer changes the rung.
    assert_eq!(
        w.debug_spell_gate_cost(2),
        Some(want),
        "the death's downgrade ladder re-priced the token at the level-0 rung"
    );
}

#[test]
fn mc2_castle_death_reprices_at_the_level0_rung_on_the_retail_arm() {
    mc2_castle_death_cost_arm(false);
}

#[test]
fn mc2_castle_death_reprices_at_the_level0_rung_on_the_patched_arm() {
    mc2_castle_death_cost_arm(true);
}

/// The pane grey-out law (`canSummon`/`canSubSummon`, EF:22503-08 /
/// EF:22602-08): a tier whose `maxManaLimit_A` castle-pool
/// prerequisite is nonzero must read NOT castable while no own castle
/// exists; requirement-free tiers (fireball) always read castable.
#[test]
fn mc2_pane_castable_reflects_castle_gate() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    let bv = w.mc2_book_view();
    // The CD table's own shape: the BASE fireball is requirement-free,
    // but its tier 2 carries a nonzero `maxManaLimit_A` — even
    // fireball's top tier greys castle-less.
    assert_eq!(
        bv.castable[0],
        [true, true, false],
        "fireball: base/repeat lit, tier 2 castle-gated (SPELLS.DAT)"
    );
    assert!(
        bv.castable.iter().flatten().any(|c| !c),
        "castle-less world: at least one castle-gated tier reads grey"
    );
    // The flyout's SECOND grey term (EF:22609/:22618, player
    // retail-verified 2026-08-21): the broke test compares hand mana
    // against a PER-TIER recomputed cost, so the view must surface
    // every tier's cost, not just the selected one's — and the
    // owned base fireball's cost must be live in it.
    assert_eq!(
        bv.cost_tier[0][0], bv.cost[0],
        "tier 0 of the selected-tier-0 fireball matches the selected cost"
    );
    assert!(
        bv.cost_tier[0][0] > 0,
        "fireball's base tier carries a nonzero per-cast cost"
    );
}

/// A castle's terrain must not outlive it — on SLOPED ground.
///
/// The stamp writes `datum + cell` absolutely; the demolish only
/// subtracts `cell` back off, and nothing anywhere saves the original
/// ground. Retail keeps that asymmetry harmless by taking the datum as
/// the PERIMETER MINIMUM of the row-1 footprint (`sub_4AA40` EF:33399
/// → `sub_48E60`/`sub_48F20`, init 250), so the leftover pad lands at
/// or below the lowest surrounding ground. The port used the corner
/// MEAN, which on any slope sits above the low side — and the demolish
/// left `mean - ground` of stone-textured mesa standing where the
/// castle had been, the flagless "tower" the player reported. Flat
/// ground (mean == min) hid it, hence the site-dependence.
/// `clear_spot`'s scan plus a relief requirement over the row-1
/// footprint: the site must slope by at least 12 height units.
fn sloped_spot(w: &World) -> Option<(u16, u16)> {
    let p = w.planes();
    let mut best: Option<(u16, u16, u8)> = None;
    for cy in (24..222u16).step_by(2) {
        'cand: for cx in (24..232u16).step_by(2) {
            for dy in -9i32..=25 {
                for dx in -9i32..=9 {
                    let t =
                        ((cy as i32 + dy) as usize % 256) * 256 + ((cx as i32 + dx) as usize % 256);
                    if p.angle[t] & 0x80 != 0 || p.angle[t] & 0xF == 0 {
                        continue 'cand;
                    }
                }
            }
            let (mut lo, mut hi) = (255u8, 0u8);
            for dy in -4i32..=4 {
                for dx in -4i32..=4 {
                    let t =
                        ((cy as i32 + dy) as usize % 256) * 256 + ((cx as i32 + dx) as usize % 256);
                    lo = lo.min(p.height[t]);
                    hi = hi.max(p.height[t]);
                }
            }
            if best.is_none_or(|(_, _, r)| hi - lo > r) {
                best = Some((cx, cy, hi - lo));
            }
        }
    }
    best.filter(|&(_, _, relief)| relief >= 12)
        .map(|(cx, cy, _)| (cx, cy))
}

#[test]
fn a_castle_on_a_slope_leaves_no_mesa_behind() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    // A clear site whose row-1 footprint genuinely SLOPES — the whole
    // point is that mean != min there. `clear_spot` happily returns
    // dead-flat ground, where the old mean datum equalled the min and
    // the bug could not show.
    let (cx, cy) = sloped_spot(&w).expect("a sloped clear site");
    let foot = |w: &World, cx: u16, cy: u16| {
        let (mut lo, mut hi) = (255u8, 0u8);
        for dy in -4i32..=4 {
            for dx in -4i32..=4 {
                let t = ((cy as i32 + dy) as usize % 256) * 256 + ((cx as i32 + dx) as usize % 256);
                lo = lo.min(w.planes().height[t]);
                hi = hi.max(w.planes().height[t]);
            }
        }
        (lo, hi)
    };
    let (lo, hi) = foot(&w, cx, cy);
    eprintln!("site ({cx},{cy}) relief {lo}..{hi}");

    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);
    let before: Vec<u8> = w.planes().height.to_vec();

    w.mc2_select_spell(2, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    for _ in 0..140 {
        w.tick(pose, PlayerCommand::default());
    }
    let castle = w
        .debug_pool()
        .1
        .into_iter()
        .find(|e| e.class == 3 && e.model == 2)
        .expect("the castle stands");
    let (tx, ty) = (castle.tx as i32, castle.ty as i32);

    // Demolish it out of existence (level 1 → dead).
    for _ in 0..8 {
        w.tick(
            pose,
            PlayerCommand {
                demolish: true,
                ..Default::default()
            },
        );
        for _ in 0..60 {
            w.tick(pose, PlayerCommand::default());
        }
        if count(&w, 3, 2) == 0 {
            break;
        }
    }
    assert_eq!(count(&w, 3, 2), 0, "the castle is gone");

    // With the datum at the perimeter MIN this site comes back
    // EXACTLY (worst residue 0); with the old corner MEAN it kept an
    // 18-unit mesa. The bound sits between the two so the test
    // actually discriminates — the un-stamp's own LCG jitter (up to
    // +19, retail-faithful) never fires here because the apron cells
    // carry height 0 and land on the `cell >= current` zeroing arm.
    for dy in -6i32..=6 {
        for dx in -6i32..=6 {
            let t = ((ty + dy) as usize % 256) * 256 + ((tx + dx) as usize % 256);
            let (now, was) = (w.planes().height[t] as i32, before[t] as i32);
            assert!(
                now - was <= 8,
                "mesa left at ({dx},{dy}): {was} → {now} (site relief {lo}..{hi})"
            );
        }
    }
}

/// THE CASTLE AS A WEAPON: a rising castle EXECUTES what stands on
/// its footprint.
///
/// Retail's (10,42) castle painter runs `sub_57390` over every cell of
/// the cumulative footprint on EVERY tick of the 19-tick rise
/// (EF:27826-27), gated on the painter's kill bit — which only the
/// level-UP spawn sets (`sub_60480` EF:61602), never the damage
/// repaint (`sub_5FBD0`). Class-5 creatures die unless their model is
/// protected {6, 8, 10, 16, 22, 23, 27} (+ 25 in action 200), and the
/// skip test is an OWNER compare, so the builder's own creatures are
/// spared. Our painter did none of this, which is why castles read as
/// far less lethal than retail.
#[test]
fn a_rising_castle_executes_what_stands_under_it() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = clear_spot(&w);
    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);

    // Model 19 is unprotected; model 16 is on the protected list.
    // One of each, on the footprint centre, NOT owned by the caster.
    let victim = w.debug_mc2_spawn_creature(19, cx, cy, 900);
    let immune = w.debug_mc2_spawn_creature(16, cx, cy, 900);
    assert!(victim != 0 && immune != 0, "both creatures spawned");
    // And one unprotected creature that the CASTER owns: retail's
    // owner compare spares it.
    let own_id = w
        .debug_pool()
        .1
        .iter()
        .find(|e| e.slot == victim)
        .unwrap()
        .id24;
    assert_eq!(own_id, 900, "the victim is owned by the stranger");
    let friend = w.debug_mc2_spawn_creature(19, cx, cy, 0xFFFF /* PLAYER_TARGET */);
    assert!(friend != 0, "the friendly creature spawned");

    let alive = |w: &World, slot: usize| {
        w.debug_pool()
            .1
            .iter()
            .any(|e| e.slot == slot && e.class == 5 && e.life >= 0)
    };
    assert!(alive(&w, victim) && alive(&w, immune) && alive(&w, friend));

    w.mc2_select_spell(2, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    for _ in 0..140 {
        w.tick(pose, PlayerCommand::default());
    }
    assert_eq!(count(&w, 3, 2), 1, "the castle rose");
    assert!(
        !alive(&w, victim),
        "the rising castle killed the unprotected creature under it"
    );
    assert!(
        alive(&w, immune),
        "model 16 is on retail's protected list and survives"
    );
    assert!(
        alive(&w, friend),
        "the builder's OWN creature is spared (the owner compare)"
    );
}

/// ⭐⭐ THE MC2 CASTLE BALL MINTS THROUGH MC2's OWN CREATOR, UNARMED
/// (`sub_69AB0`'s `_4A190(9,10)` tail EF:56127-58 → `sub_4D900`
/// EF:34965). PAIR-BLIND by construction — `retail_import_mc2`
/// stamps `F_MC2PROJ` on every class-9 and restores the behavior row
/// off `ptr_a0`, so no fixture can hold any of this; the free run is
/// the only witness (mc2l3 t=241-243) and this is its guard.
///
/// Three lanes, one mint:
///   * behavior ROW 60 — MC1's `spawn_castle_ball` builds on row 1,
///     whose turn authority swung the ball ~20 units of yaw per tick
///     where retail eases 789 → 810;
///   * the `F_MC2PROJ` routing marker — without it a NATIVE ball took
///     the MC1 fallback flight while every IMPORTED one took MC2's;
///   * UNARMED at birth (retail's flags byte0 = link alone), so the
///     arm bit + the launch site test land on the ball's own FIRST
///     dispatch a tick later and the first flight step the tick after
///     that. Folding the head into the mint flew the ball a tick
///     early and moved the landing tile.
///
/// Plus the cast-charge bank (`byte_0x154` → `@0x10`, EF:56153-54).
#[test]
fn the_mc2_castle_ball_mints_on_mc2s_own_creator_unarmed_and_banks_the_charge() {
    let Some(root) = baked_root() else {
        eprintln!("skipping: no baked data");
        return;
    };
    let Some(mut w) = build_world(&root) else {
        eprintln!("skipping: level-000 has no terrain");
        return;
    };
    w.set_dev_spells(true);
    let (cx, cy) = clear_spot(&w);
    let px = cx as f32 + 0.5;
    let pz = cy as f32 + 16.5;
    let alt = w.ground_height_tiles(px, pz) + 2.0;
    let pose = PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);
    w.mc2_select_spell(2, 0, 0);

    // Idle well past the meter's 200 cap so the bank is unambiguous.
    for _ in 0..240 {
        w.tick(pose, PlayerCommand::default());
    }
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );

    let ball = w
        .debug_pool()
        .1
        .into_iter()
        .find(|e| e.class == 9 && e.model == 10)
        .expect("the cast minted a (9,10) castle ball");
    assert_eq!(ball.row, 60, "row 60 — sub_4D900's, not MC1 row 1");
    assert_ne!(
        ball.flags & (1 << 29),
        0,
        "F_MC2PROJ — the ball takes the MC2 flight, not the MC1 fallback"
    );
    assert_eq!(
        ball.f26, 200,
        "the cast banks the wizext charge meter at its 200 cap"
    );

    // ⚠ THE ARM BIT IS NOT ASSERTABLE HERE. The mint leaves it clear,
    // but sub_66D00's head runs on the ball's own first DISPATCH —
    // and whether that falls on the birth tick is walk-position
    // dependent: a ball minted into a slot ABOVE the caster's takes
    // its head the same tick (arm + site test, no move, all inside
    // the birth boundary and so invisible from outside), while the
    // mc2l3 recording's ball at slot 53 sits below the token at 170
    // and takes it at the NEXT boundary. Both are the same law. Its
    // witness is the corpus free run (mc2l3 t=241 unarmed at birth,
    // 242 arms unmoved, 243 first flight step); folding the head into
    // the mint flew the ball a tick early and moved the landing tile.
}
