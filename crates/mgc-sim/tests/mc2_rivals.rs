//! MC2 rival column regression (docs/traces/mc2-rivals-brain.md +
//! mc2-rivals-spawn-mortality.md + mc2-rivals-open-closure.md):
//! rivals spawn from the level record under the NumberOfPlayers
//! bound, carry their authored books and castles, run the brain
//! deterministically, and elimination feeds the staged objective
//! engine's kill-player cases.
//!
//! Runs against the real bakes (`baked/mc2`); skips silently when the
//! player's gamedata bake is absent (CI without game assets).

use mgc_formats::LevelPackage;
use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::ids::GameId;
use mgc_sim::mc2::rivals::Mc2RivalConfig;
use std::path::Path;

fn load(level: &str) -> Option<(World, LevelPackage)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    let root = root.as_path();
    let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-night")).ok()?;
    let assets = FeatureAssets::parse(
        bundle.search.as_ref()?,
        bundle.build_tab.as_ref()?,
        bundle.build_dat.as_ref()?,
    )
    .ok()?
    .with_bldgprm(bundle.bldgprm.as_deref().unwrap_or_default());
    // Day-sourced extents (Bundle::mc2_extent_dims — the boot-time
    // TMAPS0-0 law), whichever bank the level renders.
    let assets = match bundle.mc2_extent_dims(&root.join("assets")) {
        Some(dims) => assets.with_mc2_sprite_ext(mgc_sim::mc2::derive_sprite_extents(&dims)),
        None => assets,
    };
    let assets = match bundle.spells.as_deref() {
        Some(sp) => assets.with_spells(sp).ok()?,
        None => assets,
    };
    let file = std::fs::File::open(root.join("mc2").join(format!("{level}.mgcl"))).ok()?;
    let pkg: LevelPackage = mgc_formats::mgcl::read(file).ok()?;
    let terrain = pkg.terrain.as_ref()?;
    let planes = Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone()?,
        angle: terrain.angle.clone()?,
        ceiling: terrain.ceiling.clone().unwrap_or_default(),
    };
    let seed = pkg.gen_params.as_ref().map_or(0, |g| g.seed);
    let mut w = World::new_for_game(planes, &pkg.things.things, seed, assets, GameId::Mc2);
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
    let (cfgs, count) = rival_configs(&pkg);
    w.set_mc2_wizards(&cfgs, count);
    Some((w, pkg))
}

/// The app's `mc2_rival_configs` resolution, replicated for the
/// sim-side fixture (wizards.json MC2 shape + the header's authored
/// castle levels + the unk09 NumberOfPlayers bound).
fn rival_configs(pkg: &LevelPackage) -> ([Option<Mc2RivalConfig>; 8], u16) {
    let mut out: [Option<Mc2RivalConfig>; 8] = Default::default();
    let (Some(w), Some(h)) = (pkg.wizards.as_ref(), pkg.header.as_ref()) else {
        return (out, 1);
    };
    let count = h.number_of_players.clamp(1, 8) as u16;
    for (slot, cfg) in w.wizards.iter().enumerate().take(8).skip(1) {
        let (Some(reflexes), Some(perception)) = (cfg.reflexes, cfg.perception) else {
            continue;
        };
        let mut start = [false; 26];
        let mut start_level = [0u8; 26];
        let mut blocked = [false; 26];
        for s in 0..26 {
            start[s] = cfg.starting_spells.get(s).copied().unwrap_or(0) != 0;
            start_level[s] = cfg
                .starting_spell_levels
                .get(s)
                .copied()
                .unwrap_or(0)
                .min(2);
            blocked[s] = cfg.blocked_spells.get(s).copied().unwrap_or(0) != 0;
        }
        out[slot] = Some(Mc2RivalConfig {
            aggression: cfg.aggression.clamp(0, 255) as u8,
            perception: perception.clamp(0, 255) as u8,
            reflexes: reflexes.clamp(0, 255) as u8,
            life: cfg.life.unwrap_or(0).max(0) as u16,
            castle_level: h.players[slot].max(0) as u8,
            start,
            start_level,
            blocked,
        });
    }
    (out, count)
}

fn count(w: &World, class: u8, model: u8) -> usize {
    w.debug_pool()
        .1
        .into_iter()
        .filter(|e| e.class == class && e.model == model && e.life >= 0)
        .count()
}

/// Herd spread metric: max pairwise TILE distance (torus-aware) among
/// live creatures of `(class, model)`, plus the centroid. For the
/// flocking measurement.
fn herd_spread(w: &World, class: u8, model: u8) -> (f32, (f32, f32), usize) {
    let pts: Vec<(f32, f32)> = w
        .debug_pool()
        .1
        .into_iter()
        .filter(|e| e.class == class && e.model == model && e.life >= 0)
        .map(|e| (e.tx as f32, e.ty as f32))
        .collect();
    let n = pts.len();
    if n == 0 {
        return (0.0, (0.0, 0.0), 0);
    }
    let wrap = |d: f32| {
        let d = d.abs();
        d.min(256.0 - d)
    };
    // Number of independent CLUSTERS (connected components where any
    // two members within LINK tiles are joined) — the "how many
    // separate herds" metric the player cares about. Retail = ~1-2.
    const LINK: f32 = 6.0;
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let nxt = p[c];
            p[c] = r;
            c = nxt;
        }
        r
    }
    for a in 0..n {
        for b in (a + 1)..n {
            let dx = wrap(pts[a].0 - pts[b].0);
            let dy = wrap(pts[a].1 - pts[b].1);
            if dx.hypot(dy) <= LINK {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                parent[ra] = rb;
            }
        }
    }
    let clusters = (0..n).filter(|&i| find(&mut parent, i) == i).count();
    let cx = pts.iter().map(|p| p.0).sum::<f32>() / n as f32;
    let cy = pts.iter().map(|p| p.1).sum::<f32>() / n as f32;
    (clusters as f32, (cx, cy), n)
}

/// DIAGNOSTIC (not a regression assertion — `#[ignore]`, run explicitly
/// with `--ignored --nocapture`): quantify goat-herd fragmentation on
/// level-000. The `(5,1)` goats start as ~4 clusters (LINK=6 tiles).
///
/// From-binary verification (NETHERW.EXE, the real DOS/4GW LE
/// disassembled at remc2's addresses): the three cohesion routines —
/// `sub_1BF90` wander/scan (awake-gates BOTH aggro AND the pack scan),
/// `sub_68C70` awake pass (wake within 24 tiles of the player, propagate
/// to the follow chain), `sub_1C560` follow (aim-at-leader + a 256-unit
/// separation scan) — ALL match remc2 and the port BYTE-FOR-BYTE. So
/// remc2 is CORRECT and the port is faithful; the leader-follow law is
/// simply loose. Measured here (herd kept always-awake): still ~5-8
/// clusters, so the awake gate is not the primary cause. The player has
/// retail footage showing tight ~1-2 flocks, so the divergence is
/// OUTSIDE these (verified) routines — leading hypothesis: terrain
/// fencing (the `v_16` slope-refusal / `v_20` passability that pens the
/// two authored herds into their basins). Kept as the harness to
/// validate any future flocking fix.
#[test]
#[ignore]
fn mc2_flock_dispersal_measurement() {
    let Some((mut w, _pkg)) = load("level-000") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let idle = PlayerCommand::default();
    // Glue the wizard to the herd centroid so the whole herd stays inside
    // the 24-tile wake radius (always AWAKE) — this isolates the
    // follow/cohesion steering from the awake gate. RESULT: still ~5-8
    // clusters, so the awake gate is NOT the primary cause; the faithful
    // leader-follow law is inherently loose. (Park the wizard far away
    // instead to see the gate compound it to ~10-12.)
    let (d0, c0, n) = herd_spread(&w, 5, 1);
    eprintln!(
        "goats n={n} start: clusters={d0:.0} centroid=({:.0},{:.0})",
        c0.0, c0.1
    );
    // GOAT_BASE=8: role = tick70-8. 0=patrol 1=wander 2=chase→flee
    // 3=FOLLOW(pack/torus) 4=prekill 5=kill 6=flee.
    let state_hist = |w: &World| -> [usize; 8] {
        let mut h = [0usize; 8];
        for e in w.debug_pool().1 {
            if e.class == 5 && e.model == 1 && e.life >= 0 {
                let r = e.state.wrapping_sub(8);
                if (r as usize) < 8 {
                    h[r as usize] += 1;
                }
            }
        }
        h
    };
    let pose = PlayerPose::from_tiles(2.0, 2.0, 40.0, 0.0, 0.0, 0.0); // FAR: goats roam free
    for t in 1..=8000 {
        w.tick(pose, idle);
        if t % 500 == 0 {
            let (d, c, alive) = herd_spread(&w, 5, 1);
            let h = state_hist(&w);
            eprintln!(
                "  t={t}: ALIVE={alive}/{n} clusters={d:.0} centroid=({:.0},{:.0}) states[wander={} FOLLOW={} flee={}]",
                c.0, c.1, h[1], h[3], h[6]
            );
        }
    }
}

/// Level 004 (n=3): colors 1..2 spawn as rivals, the brain runs
/// deterministically, and the two kill-player stages (authored
/// 1-based payloads 3/2 -> colors 2/1) complete on elimination —
/// ending the level.
#[test]
fn mc2_rivals_spawn_brain_objective() {
    let Some((mut w, _pkg)) = load("level-004") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    // Two AI carpets ((3,1)) spawned; the human is out-of-pool.
    assert_eq!(count(&w, 3, 1), 2, "colors 1..2 spawn as rivals");
    let views = w.rival_views();
    assert_eq!(views.len(), 2);
    assert!(views.iter().all(|v| v.alive));
    assert_eq!(views[0].name, "Nyphur");

    // Determinism: the same run twice = the same state hash.
    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);
    let Some((mut w2, _)) = load("level-004") else {
        return;
    };
    for _ in 0..600 {
        w.tick(pose, idle);
        w2.tick(pose, idle);
    }
    assert_eq!(
        w.state_hash(),
        w2.state_hash(),
        "the rival brain is deterministic"
    );
    assert!(!w.completed(), "kill-player stages still open");
    // The view exports a live altitude (the in-view rival tag's
    // anchor): a flying carpet sits well above z = 0.
    assert!(
        w.rival_views().iter().all(|v| v.alive && v.alt > 0.0),
        "rival_views carries a live altitude"
    );

    // Eliminate both rivals. They may have BUILT castles during the
    // run (castle rung 0 costs exactly the starting 1000 mana) — a
    // dead rival with a castle RESPAWNS, so keep the castles smitten
    // too; a castle-less dead rival is BANISHED, which the two
    // kill-player stages read.
    let mut saw_banish = false;
    for t in 0..8000 {
        if w.completed() {
            break;
        }
        if t % 16 == 0 {
            w.debug_kill_mc2_rival(1);
            w.debug_kill_mc2_rival(2);
            w.debug_smite(3, 2);
        }
        w.tick(pose, idle);
        // The FINAL-death broadcast (retail lang 283, fired once on
        // the elimination edge — distinct from non-final deaths).
        saw_banish |= w
            .notification()
            .is_some_and(|(t, _)| t.contains("has been banished from the realm"));
    }
    let views = w.rival_views();
    assert!(
        views.iter().all(|v| v.eliminated),
        "castle-less dead rivals are banished"
    );
    assert!(
        saw_banish,
        "an elimination broadcasts the banished-from-the-realm line"
    );
    assert!(
        w.completed(),
        "both kill-player stages completed -> level end"
    );
}

/// A dead MC2 wizard leaves a POSSESSABLE grave: it mirrors MC1
/// `spawn_grave` (action 42 `grave_tick`, `f28 = 2`, targetable bit 8
/// kept) — a wizard's possession claim inherits everything the grave
/// owns, then it despawns. Without bit 8 and the ch1 claim channel the
/// corpse can never be hit or claimed and its re-pointed mana is lost.
#[test]
fn mc2_dead_wizard_grave_is_possessable() {
    let Some((mut w, _pkg)) = load("level-004") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);

    // Kill a rival and let the death fall run to the grave spawn.
    // A respawn-capable rival can revive, so keep re-killing until the
    // grave materializes (mirrors mc2_rivals_spawn_brain_objective).
    let mut grave = false;
    for t in 0..2000 {
        if t % 16 == 0 {
            w.debug_kill_mc2_rival(1);
        }
        w.tick(pose, idle);
        if count(&w, 10, 40) > 0 {
            grave = true;
            break;
        }
    }
    assert!(grave, "the dead wizard leaves a (10,40) grave");

    // The human (PLAYER_TARGET) possesses the grave: it must respond to
    // the ch1 claim and despawn, transferring every entity it owned.
    // The hook's debug_asserts also pin bit 8 + f28 == 2.
    let (before, after, freed) = w
        .debug_mc2_possess_grave(0xFFFF)
        .expect("a live grave to possess");
    assert!(freed, "possessing the grave despawns it (no longer inert)");
    assert_eq!(
        before, after,
        "every sphere the grave owned transfers to the possessor"
    );
}

/// Level-001's FIFTH objective (`index=9 stage=153`) is a type-9
/// "destroy building" — razing the two vaults by Pyahandra's tower.
/// This drives the real level: force-complete rows 0-3 (which fires the
/// m32 stage-gated switch → disposition 8 → the two `par1=21` vaults),
/// confirm the level does NOT complete vacuously while the vaults live,
/// then raze them and confirm the type-9 row completes the level.
#[test]
fn mc2_level001_destroy_building_objective_completes() {
    let Some((mut w, _pkg)) = load("level-001") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    // Five objective rows; row 4 is the type-9 destroy-building.
    let (_, board) = w.mc2_objective_view();
    assert_eq!(board.len(), 5, "level-001 has five objective rows");
    assert_eq!(board[4].0, 9, "row 4 is the destroy-building objective");

    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);

    // Force the first four objectives; the row-3 completion fires the
    // m32 switch (par1=3 → disposition 8) that spawns the two vaults.
    for row in 0..4 {
        w.debug_complete_mc2_stage(row);
    }
    // Let the switch fire and the vaults build out and park.
    for _ in 0..120 {
        w.tick(pose, idle);
        if w.debug_mc2_count_buildings(21) >= 2 {
            break;
        }
    }
    for _ in 0..50 {
        w.tick(pose, idle);
    }
    assert_eq!(
        w.debug_mc2_count_buildings(21),
        2,
        "the two par1=21 vaults spawned by the tower"
    );
    let (cursor, board) = w.mc2_objective_view();
    assert_eq!(cursor, 4, "the destroy-building row is current");
    assert_eq!(board[4].1, 1, "row 4 still active");
    assert!(
        !w.completed(),
        "level must NOT complete vacuously while the vaults stand"
    );

    // Raze the tag-21 stage once. Each vault DEGRADES into its byte_3
    // successor (bldgprm[21].chain = 54) — a fresh tag-54 building — so
    // the objective must NOT complete yet: the chain still has a live
    // stage. (A par1-21-only test would wrongly finish here.)
    w.debug_smite(10, 45);
    for _ in 0..60 {
        w.tick(pose, idle);
    }
    assert_eq!(w.debug_mc2_count_buildings(21), 0, "tag-21 stage collapsed");
    assert_eq!(
        w.debug_mc2_count_buildings(54),
        2,
        "each vault degraded into its tag-54 successor stage"
    );
    assert!(
        !w.completed(),
        "the chain still stands (tag 54) — objective must wait"
    );

    // Raze the tag-54 stage (bldgprm[54].chain = 0 → collapses fully).
    // Now the whole chain is gone → the destroy-building row completes.
    w.debug_smite(10, 45);
    for _ in 0..60 {
        w.tick(pose, idle);
        if w.completed() {
            break;
        }
    }
    assert_eq!(
        w.debug_mc2_count_buildings(54),
        0,
        "the vaults are fully razed"
    );
    assert!(
        w.completed(),
        "razing the whole chain completes the type-9 row → level end"
    );
}

/// `mc2_objective_targets` must resolve the CURRENT objective's live
/// world targets so the app can highlight them + point
/// the arrow. Reuse level-001's type-9 vault fixture: once the two
/// `par1=21` vaults exist and the destroy-building row is current, the
/// getter must yield exactly those two buildings, flag one nearest, and
/// then shrink to one as a vault is razed — proving it re-enumerates
/// live state each call (the dwelling-straggler requirement).
#[test]
fn mc2_objective_targets_tracks_current_stage() {
    let Some((mut w, _pkg)) = load("level-001") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);
    for row in 0..4 {
        w.debug_complete_mc2_stage(row);
    }
    for _ in 0..120 {
        w.tick(pose, idle);
        if w.debug_mc2_count_buildings(21) >= 2 {
            break;
        }
    }
    for _ in 0..50 {
        w.tick(pose, idle);
    }

    let targets = w.mc2_objective_targets();
    assert_eq!(
        targets.len(),
        2,
        "the two live vaults are the current type-9 targets"
    );
    assert_eq!(
        targets.iter().filter(|t| t.nearest).count(),
        1,
        "exactly one target is flagged as the arrow anchor"
    );
    // Positions are real tile coords on the map, not the origin.
    for t in &targets {
        assert!(
            t.x > 0.0 && t.z > 0.0 && t.x < 256.0 && t.z < 256.0,
            "target inside the map"
        );
    }

    // Raze one stage of the chain; the getter must drop the razed pieces
    // and keep the survivors (tag-21 → tag-54, still in the chain).
    w.debug_smite(10, 45);
    for _ in 0..60 {
        w.tick(pose, idle);
    }
    let after = w.mc2_objective_targets();
    assert_eq!(
        after.len(),
        2,
        "tag-21 razed but degraded to tag-54 — both still targets in the chain"
    );

    // Finish razing → objective complete → nothing left to point at.
    w.debug_smite(10, 45);
    for _ in 0..60 {
        w.tick(pose, idle);
        if w.completed() {
            break;
        }
    }
    assert!(
        w.mc2_objective_targets().is_empty(),
        "completed objective yields no targets"
    );
}

/// `mc2_grant_plausible` learns each listed spell (a hidden
/// manifestation like the dev grant) and installs
/// its banked XP, deriving the tier from the per-spell `xpos1` ladder.
/// A big XP install must push owned spells to their max tier; a zero-XP
/// install must leave them learned at tier 0. Off-MC2 it is a no-op.
#[test]
fn mc2_grant_plausible_learns_spells_and_levels_them() {
    let Some((mut w, _pkg)) = load("level-000") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    // Fireball (0) is multi-tier (tier 1 = repeat, tier 2 = lightning),
    // so a huge XP install must promote it above tier 0. Spell 9 gets a
    // huge install too; spell 13 gets zero XP → must stay tier 0.
    w.mc2_grant_plausible(&[(0u8, 100_000i32), (9, 100_000), (13, 0)]);

    let book = w.mc2_book_view();
    assert!(
        book.owned[0] && book.owned[9] && book.owned[13],
        "all learned"
    );
    assert!(
        book.levels[0] > 0,
        "a huge XP install promotes fireball above tier 0"
    );
    assert!(book.levels[0] <= 2, "tier never exceeds the 0..2 range");
    assert_eq!(book.levels[13], 0, "spell 13 with 0 XP stays tier 0");
    // The effective XP is the banked install (fresh level, no volatile).
    assert_eq!(book.xp[0], 100_000, "banked XP is the installed value");
}

/// Objective type 1 (kill a NAMED creature): the port binds the row to
/// the live entity its authored THING index spawns (`sub_58DA0`,
/// EF:40650-90) and completes when that bound creature is gone.
/// Level-008 row 1 names THING 111 = a class-5 model-17
/// diver spawned at dis 0 (i.e. at load, BEFORE the app registers the
/// stages — so this also exercises the retroactive bind in
/// `set_mc2_stages`). Type 1 is a background row (not current-gated): it
/// must stay active while the diver lives and latch the moment it dies —
/// never vacuously at load.
#[test]
fn mc2_level008_kill_named_creature_objective_completes() {
    let Some((mut w, _pkg)) = load("level-008") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let (_, board) = w.mc2_objective_view();
    assert_eq!(board[1].0, 1, "row 1 is a kill-named-creature (type 1)");
    assert_eq!(board[1].1, 1, "row 1 active at load (not vacuously done)");
    // The named diver (THING 111 -> class-5 model-17) spawned at load and
    // bound. It is persistent (max_life 10000), so it will not self-expire.
    assert!(count(&w, 5, 17) >= 1, "the named diver spawned");

    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);

    // Idle: the diver lives, so the background row must NOT complete.
    for _ in 0..40 {
        w.tick(pose, idle);
    }
    let (_, board) = w.mc2_objective_view();
    assert_eq!(
        board[1].1, 1,
        "row 1 stays open while the bound creature lives"
    );

    // Kill the diver: the bound row latches on the next objective pass.
    assert!(w.debug_smite(5, 17) >= 1, "smote the diver");
    for _ in 0..8 {
        w.tick(pose, idle);
    }
    let (_, board) = w.mc2_objective_view();
    assert_eq!(
        board[1].1, 2,
        "killing the named creature completes the type-1 row"
    );
}

/// Objective type 2 (kill NAMED target "for real") shares type 1's
/// bind seam PLUS the degradation-chain succession: razing an
/// intermediate building spawns its `bldgprm.chain` successor and the
/// bound row FOLLOWS it
/// (`sub_59760`, EF:40921-54), so the row completes only when the
/// FINAL stage of the chain dies (retail's `!fontTypeIndex` term,
/// EF:40771-79). EVERY shipped type-2 target is a NAMED BUILDING
/// (class-10 model-45), not a plain creature. Level-008 row 3 names
/// THING slot 63 = a building released by disposition 1 — so this
/// exercises the SPAWN-TIME bind hook in `spawn_from_thing` (vs the
/// type-1 test's retroactive load-time bind). The row must bind the
/// named instance specifically, not any model-45, survive the
/// intermediate collapses, and latch when the chain is exhausted.
#[test]
fn mc2_level008_kill_named_building_type2_completes() {
    let Some((mut w, _pkg)) = load("level-008") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let (_, board) = w.mc2_objective_view();
    assert_eq!(board[3].0, 2, "row 3 is a kill-for-real (type 2)");
    assert_eq!(board[3].1, 1, "row 3 active at load");

    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);

    // The named target (slot 63) is dis-1-gated — not yet live, so
    // row 3 is unbound. Smiting any load-time buildings must NOT complete
    // it (the bind is entity-specific, not by-model).
    w.debug_smite(10, 45);
    for _ in 0..8 {
        w.tick(pose, idle);
    }
    let (_, board) = w.mc2_objective_view();
    assert_eq!(
        board[3].1, 1,
        "row 3 unbound — its named target is not among any load buildings"
    );

    // Release it (disposition 1): the building spawns and binds through
    // the spawn seam.
    w.debug_fire_disposition(1);
    for _ in 0..30 {
        w.tick(pose, idle);
    }
    assert!(count(&w, 10, 45) >= 1, "dis 1 released the named building");
    let (_, board) = w.mc2_objective_view();
    assert_eq!(board[3].1, 1, "row 3 still open — the bound building lives");

    // Raze it. The building degrades down its bldgprm chain — each
    // intermediate collapse spawns a successor the bound row follows —
    // so the row must stay OPEN after the first raze and latch only
    // when the whole chain is exhausted.
    assert!(w.debug_smite(10, 45) >= 1, "razed the named building");
    for _ in 0..8 {
        w.tick(pose, idle);
    }
    let (_, board) = w.mc2_objective_view();
    assert_eq!(
        board[3].1, 1,
        "row 3 still open — the razed building degraded to its chain successor"
    );
    // Keep razing until the chain dies out (≤8 links by construction).
    let mut done = false;
    for _ in 0..8 {
        if w.debug_smite(10, 45) == 0 {
            break;
        }
        for _ in 0..8 {
            w.tick(pose, idle);
        }
        let (_, board) = w.mc2_objective_view();
        if board[3].1 == 2 {
            done = true;
            break;
        }
    }
    assert!(
        done,
        "razing the full degradation chain completes the type-2 row"
    );
}

/// The StageVar hold-gate layer (`crate::mc2::stagevars`). A gated
/// creature spawns HELD (frozen at its phase-7 wait) until its trigger
/// fires; then it drops to its active
/// action. Level-019 holds four model-16 creatures on a KIND-3 gate
/// (release when a bound entity dies): they must stay dormant while it
/// lives and all release when it dies. This exercises the load-time
/// retroactive attach + the per-tick reaction + the death-watch scan.
#[test]
fn mc2_level019_stagevar_holds_until_bound_death() {
    let Some((mut w, _pkg)) = load("level-019") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    // Four model-16 creatures are held at load (kind 3).
    let held0 = w.debug_mc2_held();
    assert_eq!(held0.len(), 4, "level-019 holds four creatures at load");
    assert!(
        held0
            .iter()
            .all(|&(_, model, kind)| model == 16 && kind == 3),
        "all four are model-16 on a kind-3 (bound-death) gate: {held0:?}"
    );

    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);

    // A kind-3 gate does not self-release: the creatures stay dormant.
    for _ in 0..60 {
        w.tick(pose, idle);
    }
    assert_eq!(
        w.debug_mc2_held().len(),
        4,
        "kind-3 holds do not release on their own"
    );

    // Kill the watched entity (smite every creature model): the gate
    // fires and all four release to their active action.
    for m in 0..30 {
        w.debug_smite(5, m);
    }
    for _ in 0..20 {
        w.tick(pose, idle);
    }
    assert!(
        w.debug_mc2_held().is_empty(),
        "the bound entity's death released every held creature: {:?}",
        w.debug_mc2_held()
    );
}

/// The KIND-6 (timer) gate — a held creature releases after a fixed
/// countdown, with no external trigger. Level-104
/// holds two model-16 creatures whose timers are 2020/2040 ticks; both
/// must still be held well before then and both released after.
#[test]
fn mc2_level104_stagevar_timer_releases() {
    let Some((mut w, _pkg)) = load("level-104") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let held0 = w.debug_mc2_held();
    assert_eq!(held0.len(), 2, "level-104 holds two creatures at load");
    assert!(
        held0
            .iter()
            .all(|&(_, model, kind)| model == 16 && kind == 6),
        "both are model-16 on a kind-6 (timer) gate: {held0:?}"
    );

    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);

    // Still held at 1000 ticks (both timers are > 2000).
    for _ in 0..1000 {
        w.tick(pose, idle);
    }
    assert_eq!(w.debug_mc2_held().len(), 2, "held while the timer runs");

    // Past both countdowns → both released.
    for _ in 0..1100 {
        w.tick(pose, idle);
    }
    assert!(
        w.debug_mc2_held().is_empty(),
        "the timer expired and released both: {:?}",
        w.debug_mc2_held()
    );
}

/// Level 022 (n=8, seven authored rival castles): every configured
/// color gets its castle at load, Life-scaled, full of mana.
#[test]
fn mc2_rivals_authored_castles() {
    let Some((mut w, pkg)) = load("level-022") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let players = pkg.header.as_ref().unwrap().players;
    let expected = players[1..].iter().filter(|&&p| p > 0).count();
    assert_eq!(count(&w, 3, 1), 7, "colors 1..7 spawn");
    assert_eq!(
        count(&w, 3, 2),
        expected,
        "one authored castle per configured color"
    );
    // The authored BOOK law (InitialiseSpells_54A50: grant = start &&
    // !blocked, level = authored tier clamped <= 2) and the authored
    // castle bank (spawns FULL, clamped 320000 — EF:43812-17), pinned
    // at load before the brain spends anything.
    let (configs, _) = rival_configs(&pkg);
    for slot in 1..8u8 {
        let Some(cfg) = &configs[slot as usize] else {
            continue;
        };
        let (book, bank) = w
            .debug_mc2_rival_economy(slot)
            .expect("a live rival record per configured color");
        for s in 0..26 {
            let want = cfg.start[s] && !cfg.blocked[s];
            assert_eq!(
                book[s].0, want,
                "slot {slot} spell {s}: authored grant = start && !blocked"
            );
            if want {
                assert_eq!(
                    book[s].1,
                    cfg.start_level[s].min(2),
                    "slot {slot} spell {s}: authored starting tier"
                );
            }
        }
        if cfg.castle_level > 0 && book[2].0 {
            let (stored, cap) = bank.expect("an authored castle for the configured color");
            assert_eq!(
                stored,
                cap.clamp(0, 320_000),
                "slot {slot}: the authored castle spawns FULL (clamped 320000)"
            );
            assert!(stored > 0, "slot {slot}: a non-empty castle bank");
        }
    }
    // The castles stand (action 4) and survive the brain running.
    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);
    for _ in 0..300 {
        w.tick(pose, idle);
    }
    assert_eq!(count(&w, 3, 2), expected, "castles stand through play");
    assert_eq!(w.rival_views().len(), 7);
}

#[test]
fn mc2_steal_mana_casts_a_projectile_not_a_stub() {
    // Steal Mana (13) is a class-9 subtype-8 homing bolt whose (10,25)
    // impact stamps the struck wizard's ch3 "steal" inbox (the
    // rival/human ch3 consumers already drain + credit). Deterministic
    // lock: casting it spawns a real (9,8) bolt. (The full drain is
    // exercised by the pre-existing ch3 consumers + manual playtest;
    // the economy is not cleanly observable headless — dev_spells masks
    // the caster pool and rivals self-spend their mana.)
    let Some((mut w, _pkg)) = load("level-004") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    w.set_dev_spells(true);
    let pose = PlayerPose::from_tiles(64.0, 40.0, 64.0, 0.0, 0.0, 0.0);
    for _ in 0..6 {
        w.tick(pose, PlayerCommand::default());
    }
    w.mc2_select_spell(13, 0, 0);
    w.tick(
        pose,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    assert_eq!(
        count(&w, 9, 8),
        1,
        "casting Steal Mana launches the (9,8) homing bolt"
    );
}

/// The m27 kraken's 0xDF stage-command state (`sub_29930`). Level-058
/// authors a kind-3 StageVar (byte0 0x43,
/// watch-by-model) holding its m27 (THING 165) — the AMBUSH kraken:
/// the guardian arm aggros on the WATCHED subtype's nearest instance
/// when it comes within the row's v_28 (4608 units = 18 tiles). On
/// this level the watch is already in reach at load, so the ambush
/// fires within the body's every-tick guardian cadence (spawn ordinal
/// 0) — the body self-raises to the 0xDA chase and the MASS-ATTACK
/// broadcast throws every idle branch (f71 == 1) into begin-whip (2)
/// at the ambushed target.
#[test]
fn mc2_level058_kind3_ambush_kraken_mass_attacks() {
    let Some((mut w, _pkg)) = load("level-058") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let held = w.debug_mc2_held();
    let Some(&(body, _, kind)) = held.iter().find(|&&(_, model, _)| model == 27) else {
        panic!("level-058 holds its m27 at load: {held:?}");
    };
    assert_eq!(kind, 3, "the kraken hold is the kind-3 ambush gate");

    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);
    for _ in 0..12 {
        w.tick(pose, idle);
    }
    let (state, branches) = w.debug_mc2_m27_branches(body as usize);
    assert_eq!(
        state, 218,
        "the ambush fired: the held kraken self-raised to the 0xDA chase"
    );
    assert!(
        branches.iter().all(|&(f71, _)| f71 != 1),
        "mass-attack broadcast: no branch left in idle scan: {branches:?}"
    );
    assert!(
        branches.iter().any(|&(_, t)| t > 0),
        "the tentacle machine ran (branch ticks advanced): {branches:?}"
    );
}

/// A SYNTHETIC kind-6 (timer) hold on the same level-058 kraken. Kind
/// 6 routes through the generic handler
/// (`sub_1E1C0`) — no guardian arm, physics gated OFF by the m27
/// type-row `&2` flag — so the body must STAY at its 0xDF wait while
/// the tentacle machine keeps animating (retail never hard-freezes a
/// held creature), and a hit from a foreign attacker must break the
/// hold (`sub_1E040`: m27 FLEE clear → `216+2 = 0xDA`) with the
/// broadcast aimed at the attacker.
#[test]
fn mc2_held_kraken_animates_and_breaks_hold_on_hit() {
    let Some((mut w, _pkg)) = load("level-058") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    // Re-author the StageVar table: one kind-6 slot, huge timer,
    // holding THING 165 (the kraken). The retroactive attach re-holds
    // the live body on the new slot.
    w.set_mc2_stagevars(&[(0, 0, 0, 0, 0), (6, 0, 165, 0, 5000)]);
    let held = w.debug_mc2_held();
    let Some(&(body, _, kind)) = held.iter().find(|&&(_, model, _)| model == 27) else {
        panic!("the synthetic kind-6 var holds the m27: {held:?}");
    };
    assert_eq!(kind, 6, "kind-6 timer hold");

    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);
    for _ in 0..12 {
        w.tick(pose, idle);
    }
    // Still held (timer far from zero), NOT frozen: branches animate.
    assert!(
        w.debug_mc2_held()
            .iter()
            .any(|&(e, m, _)| e == body && m == 27),
        "kraken still held while the timer runs"
    );
    let (state, branch_ticks) = w.debug_mc2_m27_branches(body as usize);
    assert_eq!(state, 223, "body waits at 0xDF (phase-7 stage state)");
    assert!(
        branch_ticks.iter().any(|&(_, t)| t > 0),
        "held kraken branches animate (not frozen): {branch_ticks:?}"
    );

    // A non-lethal hit from a foreign attacker (the human pseudo-slot)
    // breaks the hold into 0xDA with the mass-attack broadcast.
    w.debug_mail_hit(body as usize, 100, 1);
    w.tick(pose, idle);
    let (state, branches) = w.debug_mc2_m27_branches(body as usize);
    assert_eq!(state, 218, "hit broke the held kraken into 0xDA chase");
    assert!(
        branches.iter().all(|&(f71, _)| f71 != 1),
        "mass-attack broadcast: no branch left in idle scan: {branches:?}"
    );
}

/// The coverage storms fire dispositions 1..=64, but shipped levels
/// author REAL creature/scroll dispositions up to 110
/// (level-020's staggered m24 waves; class-0 rows additionally carry
/// garbage ids up to 30720 — excluded, they spawn nothing).
/// Pin the true bound so a future re-bake that moves it fails loudly,
/// and so the storms' partial coverage stays an HONEST, documented
/// choice rather than a silent one.
#[test]
fn mc2_disposition_id_census_bound() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../baked/mc2");
    let Ok(dir) = std::fs::read_dir(&root) else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let mut max_dis = 0u16;
    let mut levels = 0;
    for entry in dir.filter_map(Result::ok) {
        let p = entry.path();
        if p.extension().is_none_or(|x| x != "mgcl") {
            continue;
        }
        let Ok(file) = std::fs::File::open(&p) else {
            continue;
        };
        let Ok(pkg) = mgc_formats::mgcl::read(file) else {
            continue;
        };
        levels += 1;
        for t in &pkg.things.things {
            if t.class != 0 && t.dis_id != 0xFFFF {
                max_dis = max_dis.max(t.dis_id);
            }
        }
    }
    assert_eq!(levels, 165, "the full MC2 level census");
    assert_eq!(
        max_dis, 110,
        "authored disposition ids top out at 110 — update the storm \
         bounds (and this pin) if a re-bake moves it"
    );
}

/// A NON-GOLDEN wide storm on level-020 (the deepest disposition
/// ladder, m24 waves to dis 110+). Fires every authored
/// id and ticks — asserts the misfit census stays empty (every spawn
/// materialized into a ported machine) without touching the pinned
/// 1..=64 golden fixtures.
#[test]
fn mc2_level020_wide_disposition_storm_no_misfits() {
    let Some((mut w, _pkg)) = load("level-020") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    for dis in 1..=120 {
        w.debug_fire_disposition(dis);
    }
    let idle = PlayerCommand::default();
    let pose = PlayerPose::from_tiles(8.0, 20.0, 8.0, 0.0, 0.0, 0.0);
    for _ in 0..32 {
        w.tick(pose, idle);
    }
    assert_eq!(
        w.misfits(),
        &[],
        "every wide-storm spawn runs a ported machine"
    );
}

/// Retail's InitStages "drop typed rows with stage==0" guard is dead
/// code — it reads the zeroed DESTINATION row, so every `index != -1`
/// row registers, active (taking the guard literally severs
/// level-198's m32 chain: its par1=1 switch gates on a stage0 type-7
/// row that retail completes vacuously). Pin the un-drop: level-198
/// registers all four type-7 rows (rows 1/2 are the stage0 pair) and
/// level-038 registers its full 7-row board including the stage0
/// type-1 row 6 (faithfully un-completable — binds the empty record).
#[test]
fn mc2_stage0_typed_rows_register() {
    let Some((w, _pkg)) = load("level-198") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    let (_, board) = w.mc2_objective_view();
    assert_eq!(
        board.iter().map(|&(k, _)| k).collect::<Vec<_>>(),
        vec![7, 7, 7, 7],
        "level-198: all four type-7 rows registered (stage0 pair kept)"
    );
    let Some((w, _pkg)) = load("level-038") else {
        return;
    };
    let (_, board) = w.mc2_objective_view();
    assert_eq!(
        board.iter().map(|&(k, _)| k).collect::<Vec<_>>(),
        vec![7, 5, 2, 0, 1, 5, 1],
        "level-038: the full 7-row board registered (stage0 type-1 row 6 kept)"
    );
}

/// The DUEL spell effect (docs/spell-audit/duel.md). Cast next to a
/// rival: the (10,26) tether grips the rival wizard → the LOCK forms
/// {opponent, held dist ∈ [1024,3072], tier}, +duel XP; tier 1's drain
/// mode 1 bleeds the rival's mana (regen + 8 per tick — net-negative
/// against regen); flying out of the tier's range (7720 ≈ 30 tiles)
/// breaks the lock (EF:59916).
#[test]
fn mc2_duel_locks_drains_and_breaks() {
    let Some((mut w, _pkg)) = load("level-004") else {
        eprintln!("skipping: no baked mc2 gamedata");
        return;
    };
    w.set_dev_spells(true);
    let idle = PlayerCommand::default();
    let views = w.rival_views();
    assert!(!views.is_empty(), "level-004 spawns rivals");
    let (rx, rz) = (views[0].x, views[0].z);
    // Park 3 tiles from the rival (inside every tier range).
    let near = PlayerPose::from_tiles(rx + 3.0, 20.0, rz, 0.0, 0.0, 0.0);
    // Select duel tier 1 (drain mode 1), then fire.
    w.tick(
        near,
        PlayerCommand {
            mc2_select: Some((14, 1, 0)),
            ..Default::default()
        },
    );
    w.tick(
        near,
        PlayerCommand {
            fire_left: true,
            ..Default::default()
        },
    );
    // The 8-tick tether grips within its life.
    for _ in 0..6 {
        w.tick(near, idle);
    }
    let lock = w.debug_mc2_duel();
    assert!(lock.is_some(), "the tether gripped the rival wizard");
    let (_, hold, tier) = lock.unwrap();
    assert_eq!(tier, 1);
    assert!((1024..=3072).contains(&hold), "held dist clamped: {hold}");
    // Tier-1 drain: the rival's mana goes NET NEGATIVE against its
    // own regen while the lock holds.
    let m0 = w.rival_views()[0].mana;
    for _ in 0..30 {
        w.tick(near, idle);
    }
    let m1 = w.rival_views()[0].mana;
    assert!(m1 < m0, "duel drains through the rival regen: {m0} -> {m1}");
    // Fly far beyond the tier range → the lock breaks.
    let far = PlayerPose::from_tiles(rx + 60.0, 20.0, rz, 0.0, 0.0, 0.0);
    for _ in 0..3 {
        w.tick(far, idle);
    }
    assert!(
        w.debug_mc2_duel().is_none(),
        "out of range ends the duel (EF:59947)"
    );
}

/// The carpet art family is a SWITCH, not a base+offset
/// (`AddPlayer_4A920`, EF:43732-59): color-art 0 — the human's slot,
/// and the one the replay ghost draws — takes sprite-param row 44,
/// and only 1..7 run 273..279. Row 272 is the `(10,38)` storm cloud
/// (sprite 202), so "272 + k" extrapolated onto k=0 put a fat
/// translucent ball where the wizard belongs.
#[test]
fn the_human_carpet_takes_row_44_not_the_storm_cloud() {
    use mgc_sim::mc2::{carpet_sprite_row, sprite_params::SPRITE_PARAMS};

    assert_eq!(carpet_sprite_row(0), 44, "human = the case-0 arm");
    for slot in 1..8u8 {
        let row = carpet_sprite_row(slot);
        assert!(
            (273..=279).contains(&row),
            "rival slot {slot} stays in the 273..279 band, got {row}"
        );
    }
    // Row 272 is the storm cloud, and no carpet may resolve to it.
    assert_eq!(SPRITE_PARAMS[272].word_0, 202);
    // The human's row heads the 8-view carpet family at sprite 0.
    assert_eq!(SPRITE_PARAMS[carpet_sprite_row(0) as usize].word_0, 0);
    // All eight wizards wear a DISTINCT pre-colored carpet band.
    let heads: Vec<u16> = (0..8u8)
        .map(|s| SPRITE_PARAMS[carpet_sprite_row(s) as usize].word_0)
        .collect();
    let mut sorted = heads.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 8, "eight distinct carpet families: {heads:?}");
    assert!(!heads.contains(&202), "no wizard wears the storm cloud");
}
