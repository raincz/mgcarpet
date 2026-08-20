//! Refactor guard: golden state-hash fixtures over real baked data.
//! The scripted run exercises triggers, dispositions, crater digging,
//! creature spawns and movement, rival wizards, spell grants,
//! projectile combat and the economy loop; [`World::state_hash`]
//! digests the FULL persistent state (pool internals, LCG streams,
//! mailboxes), so ANY behavioral divergence — however internal — trips
//! the fixture.
//!
//! The goldens pin the CURRENT port's behavior, not retail's: they are
//! a refactoring invariant, not a fidelity oracle. Regenerate (run
//! with `--nocapture` and copy the printed array) only when a
//! DELIBERATE behavior change lands, and say so in the commit.
//!
//! Self-skips when the baked tree is absent (game data is optional).

use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::mc1::rivals::RivalConfig;
use mgc_sim::mc1::spells::SpellId;
use std::path::PathBuf;

#[path = "common/mod.rs"]
mod common;

fn baked_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    (p.join("mc1/level-005.mgcl").exists() && !common::modded_bake(&p)).then_some(p)
}

/// Level 005 with its authored wizards (rival preplants), mirroring
/// the app's WorldInit path.
fn build_world(root: &std::path::Path) -> World {
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
    let mut w = World::new(planes, &pkg.things.things, seed, assets);
    if let Some(f) = pkg.gen_params.as_ref().and_then(|g| g.footer) {
        w.set_win_pct(f[0]);
    }
    let (wizards, player_count) = rival_configs(pkg.wizards.as_ref());
    w.set_wizards(&wizards, player_count);
    w
}

/// wizards.json → per-slot rival configs (the mgc-app resolver,
/// duplicated here because the test crate can't reach it).
fn rival_configs(wizards: Option<&mgc_formats::Wizards>) -> ([Option<RivalConfig>; 8], u16) {
    let mut out: [Option<RivalConfig>; 8] = Default::default();
    let Some(w) = wizards else { return (out, 1) };
    let count = w.player_count.unwrap_or(1).min(8);
    for (slot, cfg) in w.wizards.iter().enumerate().take(8).skip(1) {
        let (Some(acc), Some(tempo), Some(allowed_mask)) =
            (cfg.accuracy, cfg.tempo, cfg.allowed_spells.as_ref())
        else {
            continue;
        };
        let mut book = [false; 24];
        let mut allowed = [false; 24];
        for s in 0..24 {
            let a = allowed_mask.get(s).copied().unwrap_or(0) != 0;
            allowed[s] = a;
            book[s] = a && cfg.starting_spells.get(s).copied().unwrap_or(0) != 0;
        }
        out[slot] = Some(RivalConfig {
            aggression: cfg.aggression.clamp(0, 255) as u8,
            accuracy: acc.clamp(0, 255) as u8,
            tempo: tempo.clamp(0, 255) as u8,
            castle_level: cfg.castle_level.unwrap_or(0),
            book,
            allowed,
        });
    }
    (out, count)
}

/// Hover near the ground at (x, z) for `ticks` turns under `cmd`.
fn fly(w: &mut World, x: f32, z: f32, ticks: usize, cmd: PlayerCommand) {
    for _ in 0..ticks {
        let alt = w.ground_height_tiles(x, z) + 2.0;
        w.tick(PlayerPose::from_tiles(x, alt, z, 0.0, 0.0, 0.0), cmd);
    }
}

/// The scripted run; returns the checkpoint hashes.
fn run(root: &std::path::Path) -> (Vec<u64>, Vec<u64>) {
    let mut w = build_world(root);
    let idle = PlayerCommand::default();
    let mut hashes = vec![w.state_hash()];
    let mut obs = vec![w.observable_digest()]; // post-init, pre-tick

    // A: idle far from everything — ambient economy + rival brains.
    fly(&mut w, 20.0, 20.0, 32, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // B: the (99,115) proximity trigger → disposition 1 (crater +
    // follow-up trigger); back off while the crater digs.
    fly(&mut w, 101.5, 117.5, 16, idle);
    fly(&mut w, 20.0, 20.0, 120, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // C: the follow-up trigger → disposition 2 (8-creature ambush).
    fly(&mut w, 95.5, 109.5, 16, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // D: combat over the ambush — dev spells, fireballs both hands
    // (projectiles, mailboxes, deaths, corpse mana balls).
    w.set_dev_spells(true);
    let equip = PlayerCommand {
        equip_left: Some(SpellId(0)),
        equip_right: Some(SpellId(23)),
        ..Default::default()
    };
    fly(&mut w, 95.5, 109.5, 1, equip);
    let firing = PlayerCommand {
        fire_left: true,
        fire_right: true,
        ..Default::default()
    };
    fly(&mut w, 95.5, 109.5, 64, firing);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    // E: aftermath — regen, decay, wandering survivors.
    fly(&mut w, 20.0, 20.0, 100, idle);
    hashes.push(w.state_hash());
    obs.push(w.observable_digest());

    (hashes, obs)
}

/// The limit-removing property: a bumped pool is bit-identical to
/// pristine MC1 up to the first exhaustion event. Level 005's scripted
/// run never exhausts, so the OBSERVABLE state (terrain, population,
/// poses) must match exactly — the raw state hash legitimately differs
/// (pool length + chassis are hashed).
#[test]
fn bumped_pool_is_transparent_without_exhaustion() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let observe = |chassis: mgc_sim::chassis::ChassisParams| {
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
        let mut w = World::new_with_chassis(planes, &pkg.things.things, seed, assets, chassis);
        fly(&mut w, 101.5, 117.5, 16, PlayerCommand::default());
        fly(&mut w, 95.5, 109.5, 64, PlayerCommand::default());
        let poses: Vec<_> = w
            .live_poses()
            .iter()
            .map(|p| (p.type_index, (p.x * 256.0) as i32, (p.z * 256.0) as i32))
            .collect();
        (w.planes().height.clone(), w.live_things().len(), poses)
    };
    let pristine = observe(mgc_sim::chassis::ChassisParams::MC1);
    let bumped = observe(mgc_sim::chassis::ChassisParams {
        pool_slots: 2000,
        ..mgc_sim::chassis::ChassisParams::MC1
    });
    assert_eq!(pristine, bumped, "bumped pool must be transparent");
}

#[test]
fn level_005_golden_state_hashes() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let (got, obs) = run(&root);
    // Bit-identical across runs before anything else.
    assert_eq!(
        (got.clone(), obs.clone()),
        run(&root),
        "sim is not deterministic"
    );
    println!("state hashes: {got:#018x?}");
    println!("observable:   {obs:#018x?}");

    // These goldens encode retail's house-emit gate: the EXACT
    // equality `f26 == f128` (:30819), not a `>=` (which would let
    // every over-full house emit villagers forever — the runaway-
    // ecology trap: unbounded peasants + loose mana until pool
    // saturation). The house-emit law affects checkpoints D/E (the
    // fixture's first house fills during the combat window); A-C hold.
    // Any behavioral re-pin here moves the OBSERVABLE projection below
    // at the same checkpoints — expected and REQUIRED.
    //
    // Re-pinned for the m4 militia movement-core fix: retail's idle
    // (sub_1B5D0 :22541) and chase (sub_1A120 :21654) handlers both run
    // sub_196E0 (`creature_move`) every alive tick — the altitude-clamp
    // carrier — which our port had dropped, freezing collapse-spawned
    // militia mid-air. Restoring it (plus the idle wander jitter) settles
    // and wanders them; the first evacuee militia appear once the crater
    // dig reaches a house at checkpoint B, so post-init + A hold while
    // B-E move in BOTH the layout hash and the OBSERVABLE projection.
    //
    // Re-pinned for the m12 settler transcription fixes (sub_1EED0
    // :25077-84, sub_1F120 :25165-70) — pre-decrement +26 tests in
    // WANDER and APPROACH, and the `(f63 % v_26) / 2` think gate. This
    // level has settlers, so their ent_rand phase shifts B-E.
    //
    // Re-pinned for the corpse-flame two-pass fix (sub_25130 :28142-58):
    // retail's life test reads the PRE-decrement value, and the `& 2`
    // latch guards ONLY the one-shot sound — so a life-1 corpse puff
    // spawns its fire ring on TWO ticks. Our port tested post-decrement
    // AND returned early on the latch, so it spawned one ring: every
    // creature death delivered HALF its fire damage. Measured on a
    // 17-part worm crushed under a fresh level-1 castle: 10,400 before,
    // 20,400 after, against a 20,000 ladder — i.e. retail's reported
    // "the crush destroys the castle outright, or leaves the bar at 0".
    // The ~50% per-cell spawn gate is FAITHFUL and stays (confirmed in
    // remc2's independent decompile of a different binary,
    // engine/EventsFunctions.cpp:22793) — it was never the halving.
    // B-E move: the crater dig at B is the first thing that kills.
    //
    // Re-pinned for the m4 behavior-ROW fix (row 0 -> row 16): remc1's
    // m4 ctor (sub_386DE) could not resolve its row symbol and wrote
    // unk_98F38[0]; the unresolved declaration survives commented out
    // as `//int unk_99138;//fix` directly above it, and unk_99138
    // self-identifies as row 16. Row 0 is the flyer row (v_14=-4,
    // v_20=0xFFFFFFFF), which is why militia never descended and
    // walked out over water; row 16 is the ground-walker row
    // (v_14=-128, v_20=0xFFF080FE).
    // Blast radius CONFIRMED by probe, not assumed: level 005 holds
    // ZERO live m4 through post-init/A/B/C, gains its first at D and a
    // second at E. Exactly D and E move, in both arrays. (The
    // "militia appear at B" claim in the note below is stale — that
    // was written before the crater/evacuation timing changed.)
    //
    // Re-pinned for the authored-castle footprint fix: retail stamps
    // one build pass per authored level with the row = the pass index
    // (:54983-91), i.e. rows 0..=level, and BUILD row 0 is EMPTY.
    // `spawn_starting_castle` was passing `level + 1`, so every
    // authored rival castle wore one build ring more terrain than it
    // owned — and since the demolish un-stamps the row matching the
    // LEVEL, that surplus ring outlived the castle as a flagless
    // stump. Level 005's rivals hold authored castles, so their
    // load-time footprint shrinks by a ring: EVERY layout hash moves.
    //
    // Re-pinned for the rest of sub_1F120's APPROACH shape (:25164-77):
    // the walk runs before the think gate on every tick, the re-aim and
    // the proximity promotion run only INSIDE it, the patience /
    // dead-anchor bail falls through instead of returning (so it can
    // still promote to BUILD the same tick), +146 is never cleared, and
    // the range test is the three-axis ROOTED distance (sub_42340_42680
    // :52721), not a 2-D squared one. Settlers therefore arrive later:
    // on the isolated settler fixture the build tile is UNCHANGED
    // (123,107) but the build tick moves 154 -> 241. Post-init and A
    // hold; B-E move, as with every settler re-pin above.
    //
    // Re-pinned for the class-10 effect PRE-decrement batch: retail's
    // whole class-10 family reads the PRE-decrement life (sub_24F60
    // :28068, sub_25410 :28285, sub_25760 :28433, sub_25A60 :28592,
    // sub_262D0 :28906, sub_26360 :28933, sub_263C0 :28956, sub_26D20
    // :29311, sub_25CE0 :28685) while the class-9 FLIGHT family is
    // genuinely post-decrement. Our port had it backwards at the
    // class-10 sites and right at the class-9 ones, so every fire,
    // splash, flash, tether and cloud ran one tick short. B-E move;
    // post-init and A hold (nothing has died yet at A).
    //
    // Re-pinned for the militia idle +26 re-zero (sub_1B5D0 :22482):
    // retail's FIRST statement in the m4 idle handler clears the
    // walk-in flag every tick, so the silent-absorb death gate only
    // ever sees +26 != 0 on the one-tick house hop. Our port kept the
    // spawn stagger (+26 = slot % 100) alive into combat, so once
    // mob_death's gate widened to m4 virtually every militia despawned
    // silently — no corpse, no 500-mana ball. Level 005 holds no live
    // m4 until D, so exactly D and E move — and OBSERVABLE holds,
    // because no militia dies inside the window: the moved hashes are
    // the re-zeroed +26 field itself, layout-only by construction.
    //
    // Re-pinned for the m13/m14 feeder-wander transcription fixes
    // (sub_1F640 :25382-25438 / sub_1FAC0 :25558-25614): door radius
    // BEFORE fullness on the rooted 3-axis distance (the village
    // leash — a full home keeps pulling its villager back), act-speed
    // swaps on anchor drop/acquire (+126 = +130 / +128), the m14
    // distant filter INSIDE the acquire loop, and one think gate
    // wrapping both arms. Villager walk/absorb streams shift, so B-E
    // move; post-init and A hold (no feeder has thought yet).
    //
    // Re-pinned for the class-2 static tick port (sub_49AA0/sub_49AD0/
    // sub_49B50): stones, dolmens and bad stones now run their retail
    // per-tick handlers — the terrain snap plus the +18 |= 2 static
    // draw stamp (and the dolmen's wizard shrine sweep). A-E move,
    // post-init holds (the stamp first lands on tick 1). Layout-only
    // by construction: the stamp is the whole delta (disabling it
    // alone restores the old pins — on this run's static terrain the
    // snap is an identity write), and OBSERVABLE holds.
    // Re-pinned for the per-rival village-wanted timers: `rival_wanted`
    // ([i16; 8]) joined the Gen hash so the m4 militia and m8 griffon
    // wanted-gates can turn on hostile RIVAL wizards, not only the human.
    // Layout-only by construction — OBSERVABLE holds byte-for-byte below:
    // level 005's scripted run flags no rival wanted, so the whole delta
    // is the new zeroed field entering the hash input.
    // Re-pinned for the rival castle-site scout fix (sub_13F00): the
    // scout now returns the FIRST grid candidate that clears the 12288
    // Chebyshev spacing (the wizard's home-supercell corner), not the
    // candidate nearest the wizard — matching retail, which planted on
    // the crater rim where our port planted dead-centre. The scouted
    // `.site` stored in rival state therefore differs, so B-E move in the
    // layout hash; post-init and A hold (no rival has scouted yet).
    // Layout-only by construction: OBSERVABLE holds byte-for-byte below —
    // within this 316-tick window the corrected site changes only the
    // internal target, not the observable projection.
    // Re-pinned for the MC1 ball-physics conformance fixes (sub_27030
    // :29518-64): balls now gate their ballistic arm on the +58 settle
    // countdown (ballistic 128 ticks, then frozen at rest), run
    // friction and the downhill terrain roll only while GROUNDED (the
    // old arm ran friction unconditionally and never rolled), merge
    // only on grounded ticks, and merge donors hard-free (sub_41E90)
    // instead of soft-killing to the sweep. A behavior change by
    // design: every loose ball's trajectory and rest position shifts,
    // so A-E move in BOTH hashes; post-init holds (no ball has ticked).
    // Re-pinned B-E for the SETTLED-BALL GROUND SNAP (player-ruled
    // deviation, DEVIATIONS.md): a settled ball tracks the live
    // ground each tick. OBSERVABLE holds below — in this window the
    // snapped z deltas (balls frozen a hair off their resting
    // ground) sit under the pose projection's notice, so the moved
    // legs are the layout z lanes alone.
    // Re-pinned for the MC1 ball bounce floor (:29538-49): retail
    // zeroes any rebound <= 16 (`if (f46 <= 16) f46 = 0`) — rebound
    // only past impact -64; the port kept 8..16-unit hops from
    // -33..-64 impacts. MC1-scoped (the MC2 sphere twin keeps -32).
    // Post-init holds; A-E move with every loose ball's settle
    // trajectory.
    // Re-pinned (ALL legs incl post-init) for the worm-segment id24
    // fix: retail's segment byte-copy KEEPS the head's +24 (the
    // mc1l0 corpus pins it), the port re-stamped each segment with
    // its own slot. Layout-only if OBSERVABLE holds (id24 is not a
    // pose lane); also keeps kill credit head-only.
    // Re-pinned (D and E) for the MC1 tick-top reap law (:52226-31):
    // 0x400-killed records persist through their death tick's
    // snapshot and free at the top of the NEXT tick, so combat
    // kills (D) and aftermath decay (E) carry one-frame death
    // records and same-tick spawns pop the pre-existing stack
    // instead of recycling the dying slot. Post-init..C hold —
    // nothing dies before D in this fixture.
    // Re-pinned (A-E) for the CASTLE-COLLATERAL round (mc1hwl0 slot
    // 522, −833/tick napalm collateral, dead t=9457 — the whole
    // chain corpus-proven): the +78=0xE000 z-center marker (sub_37150
    // :43798) now written with the extents and re-applied in the
    // settled tick's every-other-tick block (sub_46DB0 :52083, level
    // VERBATIM) with the +144 owner echo (:52080); ent_overlap reads
    // +78 SIGNED (the decompile's uint16 typing is a movsx artifact
    // — the corpus overlap only reconciles signed); castles join the
    // homing-acquire candidate list (sub_54520 cases 0/3/4/0x10
    // branch model 2 to the castle scorer sub_54BD0, raw-z aim); the
    // explode child carries the victim slot in +146; the (10,53)
    // cloud joins the class-10 PRE-decrement family (7 burns from a
    // 6-life cloud). Post-init holds — the authored castles load at
    // level 0 (the guarded ctor shell); the marker first lands on
    // each castle's first settled tick inside the window.
    // Re-pinned (B-E; post-init + A hold — no ball sits in the
    // far-afield radius) for the mana-ball WAKE law (sub_54F80
    // :64352-66, corpus-proven on mc1hwl0): settled balls within
    // 24 tiles of the human re-arm +58 = 16 on the awake pass's
    // 17-tick cycle, and the ballistic gate reads the
    // post-maintenance value (retail's handler order). Behavior
    // change toward retail by design.
    // Re-pinned (B-E) for the 180° TURN TIE-BREAK law (sub_582F0
    // Sound.cpp:6580 / MC1 twin :52664 SYNCHRONIZED, corpus-proven
    // both games all takes): retail unwraps the turn delta only when
    // strictly > 1024, so an EXACT half-turn keeps the raw sign; the
    // port's wrapped-delta sign turned the other way (Gen::turn_sign,
    // mc1/mobs.rs). Behavior change toward retail by design.
    // Re-pinned (B-E; post-init + A hold — nothing burns yet) for the
    // (10,13) RISING SMOKE PUFF, ported for the first time: the ctor
    // sub_3AAA0 (str_255D0C[13]), the state-13 tick sub_257B0 (:28443)
    // and BOTH creators — the standing fire's 1-in-7 exhaust puff
    // (sub_252D0 :28224, previously a documented skip that kept only
    // the LCG draw) and the volcano plume's per-tick ring spray
    // (sub_26140 :28874, previously an untraced life countdown that
    // drew nothing at all). Every burning tree and crater now emits
    // smoke entities, so the pool population and the free-list order
    // move from B on. Behavior change toward retail by design;
    // corpus-proven on mc1hwl0, where (10,13) was the single largest
    // unexplained family (1.75M field rows + 10,135 missing).
    // Re-pinned (D/E; post-init through C hold) for the spell-16
    // cost-cache ctor seed that came with the `castle_recast_cost`
    // patch: `grant_spell` now stamps the manifestation's retail ctor
    // values (+136 = 1000, +140 = 1000/101 — sub_3BF70 :47996), so
    // the D-window `set_dev_spells(true)` grant carries them into the
    // hashed pool. LAYOUT-ONLY by the disable experiment (seed off →
    // old pins return) and OBSERVABLE holds: the cache is only ever
    // READ by the castle cast gate, which this fixture never fires.
    // The patch's arm switches themselves cannot move goldens — every
    // `World::new*` world runs WorldPatches::RETAIL.
    // Re-pinned (A on; post-init holds) for the castle-on-water
    // flatten-law split (FlattenLaw::CastleLive, sub_285C0 :30550-62):
    // the live castle painter now skips zero-delta cells outright (a
    // shore castle's water apron stays LIVE WATER instead of being
    // drained to land) and its water→land flip keys on HEIGHT 0 with
    // the `& 0xF8 | 1` write + dig-mode retile (sub_33E10). A rival's
    // window-A shore castle at ~(3,217) rises from height-0 tiles, so
    // the terrain planes move from A on. Behavior change toward
    // retail by design; the level-init stamp (sub_279D0) keeps its
    // unconditional conversion, which is why post-init holds.
    // C..E re-pinned for the m2 bee laws (toward retail, attributed
    // by toggling each piece): the acquisition-lunge arm fires in
    // window C (sound 13 is hash-visible via the sim sounds vec), and
    // the chase z-nudge moves D/E; the m3 spawn-guard change moves
    // nothing here (no pool pressure).
    // A-E re-pinned (post-init holds) for the ball vertical-law
    // conformance fix (ball_tick :29532-49 / EF:26188-26252): gravity
    // integrates every moving tick, clamp+rebound requires STRICTLY
    // below ground, grounded contact is post-clamp z == ground. The
    // awake pass re-arms settled balls near the human every 17-tick
    // cycle, so their hidden +46 lift now runs retail's 0 → −16 → 0
    // rest cycle in every window. See the OBSERVABLE verdict below
    // for the behavior/layout attribution of this re-pin.
    // D/E re-pinned (post-init through C hold) for the AIM-Z BRACKET
    // law (`Ent::aim_z` — sub_524C0/sub_524E0 :62503-14, the MC2 twin
    // sub_65580/sub_655A0 EF:62750-67): homing (sub_52550) and the
    // sub_54A90 acquire scorer measure a target at z + signed +78
    // EXCEPT model 2, measured RAW — the guard reads the MODEL byte
    // alone, so castles (3,2) home at the FLAG (the player's "sharp
    // dive at the castle base" report) and m2 BEES are aimed at their
    // raw z like retail. This fixture's D-window fireballs fight the
    // ambush BEES, which is what moves D/E (attributed by a
    // class-3-only guard experiment: restricting the guard still
    // moves D/E — the impact-landing site already carried the
    // model-2 guard, so homing/acquire were the inconsistent
    // holdouts). Behavior change toward retail by design.
    // A-E re-pinned (post-init holds) for THE CASTLE COMMIT LAWS
    // (mc1l0 t=562/563 dig): the level-up commit's first-commit
    // latch (+16 |= 2, :56057-62), the m41/m42 build workers' ctor
    // life 0 (:47557/:47579) with the castle link left out of f146
    // (retail carries it in +42, unmodeled — workers re-derive by
    // site), the castle z ground-refresh gated to the established/
    // wait cases (:56013 + 1/4/6 — action cases hold the stale z),
    // the live painter's fill goal table (:30637-41, no 3x arm),
    // and the ctor site snap = truncation + raw-point ground z
    // (sub_37920 :44244-55). Level 005's authored rival castles run
    // these machines from the first idle window. OBSERVABLE holds at
    // every leg — the shift is bookkeeping-layout only.
    // A-E re-pinned for the RIVAL TOKEN COST-CACHE SEED (mc1l5 dig:
    // `mint_manifestation` now seeds every rival spell-16 token with
    // the class-12 ctor's 1000/9 — sub_3BF70 :47996, the same seed
    // grant_spell always gave the human book; the mc1l5 take pins
    // Vodor's token at exactly 1000/9 under his standing authored
    // castle). The seed rides in the state from the mint, so every
    // leg's hash shifts; the poverty-gate READ (`rival_castle_price`)
    // resolves to the same 1000 the old static gate used, so no
    // decision moves. See the OBSERVABLE verdict below.
    // A-E re-pinned (post-init INCLUDED) for the MANA-BALL SPAWN
    // STAMPS (mc1l0 (10,39) dig): the ball ctor now stamps +66/+67 =
    // 10/39 and +126 = 32 (sub_3B5A0 :47456-57/:47463) — the level's
    // authored balls carry them from load — and the corpse drop
    // persists its speed draw in +126 (sub_27690 :29689) with the
    // signed +46 launch lift (:29692), so every kill's ball in the
    // combat windows shifts three hashed fields. Motion-inert in
    // these windows (ball physics reads none of them; no magnet,
    // possess claim or castle teardown fires here). See the
    // OBSERVABLE verdict below.
    const GOLDEN: [u64; 6] = [
        // ALL SIX re-pinned for THE mc1l2 RIVAL-WIZARD SESSION (the
        // first live-rival corpus): the wizard ch0 intake gates on the
        // mail SOURCE and consumes source-only (sub_46540 :55694+ —
        // src-0 letters are dead residue), the AI life regen applies
        // the u16_341 register UNCONDITIONALLY (no stall gate,
        // :17990-18018, `Rival::life_rate` joins the hash — the
        // post-init/A movement is that layout half), attack casts
        // gate on 3-D ARRIVAL (sub_15470/sub_42340) with the z-hover
        // on failed attempts only, rival launcher tokens run the live
        // sub_56090 burst machine (fire from the token, debit, pool
        // freeze, decrement), emissions spawn untargeted with the
        // cost/count +140 stamp, the poverty release is >=, the picker
        // walks the class-5 chains from the castle anchor, and the
        // stale-target test is sig-only. Corpus: mc1l2 19,527 → 306
        // unexplained rows; l0/l1/l32-head hold bit-exact; l5 67 + hw
        // 182 fixtures promoted, 0 regressions. Behavior change
        // toward retail by design — OBSERVABLE moves B-E (Vodor
        // plays the level differently from the dig window on) and
        // holds at post-init/A.
        // Re-pinned ALL SIX for THE mc1l2 RESIDUE SESSION. Layout:
        // `Rival::knock_dir`/`knock_mag` join the hash (SNAPSHOT 13).
        // Behavior: the arrival gate now compares retail's truncated
        // isqrt so rivals commit casts on the boundary tick, the
        // balloon fleet re-picks off the tick-top ball chain, the
        // militia's fabricated house rung is gone, m2 bees keep their
        // pre-work on hit/death ticks, and a dying rival drifts on the
        // killing blow's knock. OBSERVABLE below still holds at
        // post-init/A and moves B-E — the correct signal.
        // C/D/E re-pinned for THE mc1l2 FREE-REPLAY SESSION: the
        // pair-up scan walks the TICK-TOP per-model chain instead of
        // the pool (a creature born this tick is not yet a member and
        // cannot pair — sub_1B5D0 :22653-77), the m2 bee ctor stamps
        // its `+67 = 0` human-only filter (sub_38370 :44744, the only
        // class-5 ctor that writes the byte), and a CLAIMED BUILDING
        // takes its own owner's damage (sub_29640 :31070 carries no
        // owner test — the port's immunity clause was invented).
        // ⭐ OBSERVABLE below holds byte-for-byte at ALL SIX
        // checkpoints, post-init through E: on this level the pack
        // links and the building mailbox are the only words that move,
        // and no pose, terrain cell or population count follows them —
        // the layout-only signal. The behavioral half is free-run-only
        // (mc1l2 horizon 392 → 8282, missing/extra 72/64 → 5/5), where
        // pair mode re-imports retail's own state every tick.
        // B/C re-pinned for THE CRATER CTOR'S FLAG EDIT (the mc1l42
        // intake): sub_3A9A0 :46779-80 READS the flag word, masks it
        // `& 0xFFFDFFF7` and ORs `+18 |= 2` — a read-modify-write of
        // whatever NewEvent left standing, not the fresh clear the
        // port wrote. The (10,11) digger therefore keeps its inherited
        // bits, which is the only word that moves here: level 005's
        // authored crater dig runs through B and C and the diggers are
        // gone by D, so post-init/A/D/E hold. OBSERVABLE holds
        // byte-for-byte at ALL SIX — the dig's outcome, its cells and
        // its population are identical and only the flag word differs,
        // the layout-only signal. Corpus: mc1l42 t=20159, retail
        // flags 131072 against the port's 0; the same mask-then-set
        // shape as the (10,23) hit flash (sub_3AE80 :47076-79).
        // D/E previously re-pinned for THE CAST-PHASE LAW (the MC1
        // arm→token-fire restructure): every hand cast now arms its
        // spell token at the wizard pass and the token's own tick
        // fires ONE FRAME later (corpus: 257/257 l0 + 371/371 l32
        // arms spawn at arm+1) — the D-window fireballs all shifted
        // one tick, and their f140 stamp is now cost/period (40,
        // corpus-pinned). Then again for the AREA-BROADCAST TILE
        // ROUNDING (`area_write` nearest-tile center,
        // sub_120B0/EF:3750 — corpus: mc1l0 t=91 tent claim): the
        // D-window explosion mail reaches edge-tile bees on retail's
        // tick.
        // D/E re-pinned AGAIN for THE CHANNEL-0 WINDOW BIAS: that
        // nearest-tile rounding is right for channels 1+ and WRONG for
        // channel 0, which retail centres one tile BACK at
        // `(pos - 128) / 256` — byte-identical in all three MC1
        // variants (sub_120B0 :17339/17352, sub_124F0 :17427/17439,
        // sub_127E0 :17535/17547) and NOT shared by MC2 (sub_10C80's
        // ch0 arm rounds like every other channel, EF:4118-19). Every
        // MC1 area DAMAGE window therefore moves one tile in x and y;
        // the D/E fireball combat is exactly where that shows.
        // Corpus: mc1l0 whole take 5,171 -> 5,174 conforming pairs and
        // 4,163 -> 4,090 unexplained field rows, fixture t=4177
        // (`field:5,3:life`) promoted. Behavior change toward retail
        // by design. A-C hold — no ch0 area write lands in them.
        // D/E re-pinned once more for THE ONE-SHOT ACQUISITION LATCH:
        // retail attempts target acquisition EXACTLY ONCE per
        // projectile, latched on flags bit 2 and set win or lose
        // (`sub_52770` :62640-60, `sub_52B30` :62811-15), snapping the
        // live heading onto the pick — where the port re-scanned every
        // untargeted tick and never committed. D is literally "64 ticks
        // of two-hand FIREBALL combat", so it is the checkpoint this
        // most affects. Corpus: mc1l0 5,174 -> 5,319 conforming pairs
        // and 4,090 -> 3,344 unexplained field rows; three fixtures
        // promoted. Behavior change toward retail by design.
        // D/E re-pinned once more for THE MC1 MAILBOX RESIDUE LAW.
        // MC1 has TWO write protocols, not one: the area writers
        // (`sub_120B0`/`sub_124F0`/`sub_127E0` :17466-70) accumulate
        // while a source is pending and overwrite a stale amount, but
        // `sub_12B50` (:17604-07) — the SINGLE-target writer, with
        // exactly two callers in the binary, the creature melee thunk
        // `sub_1AB10` (:21970) and the death field's class-3 arm
        // (:31296) — does the exact INVERSE. Since every reader clears
        // the SOURCE and never the amount (:55734 / :21337), MC1 point
        // damage SNOWBALLS onto the residue of the previous hit. The
        // port ran the area order everywhere and under-damaged.
        // Corpus (mc1l0): 3,344 -> 3,340 unexplained field rows,
        // conforming pairs unchanged at 5,319, mc1l5 t=403 promoted.
        // Pinned by mc1l0 t=3230, where one 100-damage melee onto a 400
        // residue costs the player exactly 500 life, and t=3235 where
        // the 500 it left behind makes the next one cost 600.
        // ⭐ D/E ONLY, and BOOKKEEPING here, not behavior: OBSERVABLE
        // holds byte-for-byte below. L005's D/E is FIREBALL combat —
        // all area writes, which keep the old order — so nothing in
        // this slice takes different damage; the hash moves only
        // because the consumer now leaves an amount standing in the
        // hashed `player_mail` word where it used to memset the block.
        // The behavioral half of this law is melee, which the corpus
        // above pins and this slice never exercises.
        // D/E re-pinned once more for THE PROJECTILE LEDGER + BLIND
        // TRACKER LAWS: (1) sub_16540 (:19643) ledgers every class-9
        // record ONCE at the tick top — flags |= 0x2000 the first tick
        // it has a class-3 owner and a held victim, feeding the rival
        // hate/war tables at ACQUISITION time (the D-window fireballs
        // wear the mark, which alone moves the hash); (2) sub_52550's
        // tracker NEVER re-validates (:62543-55) — the port used to
        // clear +146 on a dead/empty slot, so bolts now steer at
        // corpses and recycled slots exactly as retail does;
        // (3) the fireball's terrain arm reverts to the pre-step
        // position for its water test (:62899-908) where the generic
        // keeps the stepped one (:62680-701), both exempting model 4.
        // Corpus: mc1l0 5,687 -> 5,887 conforming pairs, unexplained
        // 1,624 -> 798; the (9,0)/(9,1) block 1,135 -> 324; twelve
        // fixtures promoted across five suites, 0 regressions.
        // ⭐ BOOKKEEPING in this slice: OBSERVABLE holds byte-for-byte
        // below — no D-window bolt loses its victim mid-flight or
        // grounds on a shore tile here, so the hash moves on the
        // hashed ledger marks alone. The behavioral halves are
        // corpus-pinned (the chase -> 0 family, mc1l0 t=1818-30).
        // D/E re-pinned for THE PROJECTILE PROBE RING + THE CASTLE
        // BALL'S TWO ARMS + THE THUNK MUZZLE LAWS (one session, three
        // MC1-only laws): (1) `victim_scan`/`claim_victim_scan` walk
        // retail's SEARCH.DAT ring iterator over the rounded centre
        // with no radius floor (sub_11980 :16999-17001) — MC2 keeps
        // the inflated square as the anti-tunnel march's compensating
        // window; (2) sub_53980's +146 arm — a castle ball with a
        // homing slot homes via sub_52610 and never touches the
        // launch latch, and the ctor binds row [1] (:46185);
        // (3) creature thunks aim from the UNLIFTED muzzle and are
        // born with +34/+36 = 0, and the muzzle-acquire 34-step is
        // stored raw (:62824). Corpus: mc1l0 5,983 -> 6,007
        // conforming pairs, unexplained field rows 322 -> 136,
        // (9,10)+(10,43) dead, (5,3) 119 -> 52; mc1l5 -28.9k rows,
        // 10/10 suites 0 regressions.
        // D/E re-pinned for THE CAST-CHARGE METER (Type_160 u8_326,
        // the raw-lane surfacing session): every wizard banks a
        // +1/tick 200-cap meter (human :55377-78, rival :17987-89)
        // and the fireball/earthquake/meteor/volcano spawners move it
        // into the new bolt's +26 and zero it (:65072-73 and
        // siblings; possess :65246 zeroes without stamping). The
        // meter itself is hash-quiet (no in-engine reader); what
        // moves D/E is the HASHED f26 stamp on the D-window combat
        // fireballs, previously born 0. Post-init..C hold — no bolt
        // exists before D. OBSERVABLE holds byte-for-byte: the stamp
        // is bookkeeping on a lane nothing in-engine reads.
        // D/E re-pinned for the CORPSE-DROP f34 MIRROR removal
        // (corpse_drop_mc1 — retail sub_27690 :29663 writes only
        // +30): the D-window kills' mana balls are born target_yaw 0
        // like retail. Post-init..C hold (no drop before D).
        // Motion-inert — the ball tick never reads f34; OBSERVABLE
        // holds byte-for-byte below. Corpus: the mc1l0 pair-564
        // (10,39) family (129 masked rows) at 0, l1 −187, l2 −166,
        // l5 −484 unexplained target_yaw rows; 10/10 suites, 0
        // regressions.
        // D/E re-pinned for THE WIZARD-TICK BODY ORDER + THE LIFE-RATE
        // REGISTER (mc1l0 free-run t=1128/t=1144 digs): the mailbox
        // block now runs INSIDE the carpet dispatch BEFORE the move
        // (sub_45C90: intake :55344-78, move sub_455D0 at the tail —
        // a lower-slot hit's knock shoves the SAME tick's move), the
        // regen tail follows the move, and life regen applies the
        // PERSISTED u16_341 register before re-selecting it (:55388 /
        // :55414-20 — the castle rate lands one tick after var_50
        // latches). ⭐ BOOKKEEPING in this slice: OBSERVABLE holds
        // byte-for-byte below — D/E fireball combat takes identical
        // damage on identical ticks; the moved words are the
        // reordered intake/knock/mail phase inside the tick and the
        // register joining player state. The behavioral half is
        // free-run-only (horizon 605 → 1188, the vulture-knock pose
        // fork and the castle-establish regen staircase), where pair
        // mode imports retail's own wizext lanes each pair.
        // A..E re-pinned for THE END-TO-END SESSION (mc1l0 free run
        // bit-exact 0..7097): (1) the fire scorch gate's water probe
        // is sub_11760's ANGLE nibble (:28098), not the tile-type
        // twin — the ambient fires' draw streams and flicker deltas
        // move from leg A on (shore cells split the two probes);
        // (2) the live castle painter buffers per-cell goal deltas
        // over the LEVEL rect, last row wins, one apply pass
        // (sub_285C0 :30538-70); (3) the projectile impact/rebound
        // Player arms lift by PLAYER_HH like the pool arms; (4) the
        // house wanted arm rides the occupied branch only
        // (:30790-97); (5) villager hit ticks freeze (m12 :25057-67);
        // (6) the militia chase re-bears f34 only + re-arms wanted on
        // its own cadence (:22705-14). OBSERVABLE holds byte-for-byte
        // at post-init..C (the moved words are draw-stream and
        // bookkeeping state under pose quantization) and moves at D/E
        // with the combat-window laws — the CORRECT signal.
        // B..E re-pinned for THE TRIGGER PRE-MOVE POSE LAW (mc1l1
        // t=3082 worm wave, one 8-tick probe window early): the
        // class-11 probe reads the carpet's PREVIOUS-frame pose (the
        // pooled carpet sits ABOVE every authored volume in retail's
        // slot-ordered walk), so the B crater trigger and the C
        // ambush disposition fire one tick later, and every ctor
        // draw downstream shifts with them. OBSERVABLE holds at B
        // (the dig's outcome is identical, only its tick moved) and
        // moves from C on (the ambush spawns' rand/poses shift) —
        // the correct signal for a pure timing law. Corpus: mc1l1
        // missing-(5,3) 85 → 0; mc1l0 free run stays bit-exact
        // 0..7097; 10/10 suites, 0 regressions.
        // D/E re-pinned for THE mc1l42 RESIDUE SESSION. Post-init and
        // A..C hold byte-for-byte in BOTH goldens; D and E move in
        // both, which is the correct signal — every law that landed
        // this session is a COMBAT law, and D/E are the only combat
        // windows. mc1l5 is the militia/mound level, so the m4 idle's
        // +146 ANCHOR ARM bites hardest: a militiaman carrying a stale
        // +146 spends his every-v_26 tick forgetting it instead of
        // drawing twice and jittering (:22543-68), which moves his
        // per-entity draw stream from that tick on. Beside it: m9's
        // hidden head hoisted above the mailbox intake so a mound
        // promoted by damage re-arms +26 = 400 (:23682-98), m8's chase
        // pre-work hoisted the same way (:23546-53), m2's lunge arm as
        // a chase-ENTRY trailer (:22324-32), the genie's break-off
        // firing its parting steal seeker (:24643-50), the m0
        // fireball's vertical bearing taking the i16-TRUNCATED run
        // (sub_42180 :52646-48 — past 32767 the negate hands back a
        // positive run and retail aims the long way round), Heal's own
        // token body (sub_56270 :65091-128) and the cast command's
        // mana gate reading the pool ahead of the mailbox intake
        // (:55354 before :55366/:55385). Corpus: mc1l42 raw 1,542 →
        // 276, free-run horizon 349 → 1,161; mc1l0 3/0/0 → 0/0/0,
        // mc1l1 5/0/2 → 1/0/2, mc1l2 4/0/0 → 3/0/0; 77/77 fixtures,
        // 0 regressions.
        // ALL SIX re-pinned again, same session, for ONE further cause:
        // a THING-placed spell jar now runs the real ctor (`sub_3BF70`
        // :47979-48013 via the `off_987DE[model]` thunks :48020-161)
        // rather than the inert stand-in, so the level-load class-12
        // records carry retail's life/max_life/flags/f136/f140/f50/f44.
        // That is POST-INIT hashed state, hence every checkpoint moves.
        // ⭐ OBSERVABLE HOLDS BYTE-FOR-BYTE AT ALL SIX below — the jars
        // sit where they always sat and nothing reads the corrected
        // fields in these windows, so this half is a pure byte-image
        // correction, layout-only by the companion's construction.
        // A/B-proven with `MGC_NO_MC1_JAR_CTOR=1`, which restores these
        // hashes exactly (and `flight_tier_golden_state_hashes` too —
        // one cause, both goldens). mc1l42 6 field rows -> 0.
        // D/E re-pinned for THE SEGMENTED-REPLAY RESIDUE SESSION.
        // Post-init and A..C hold byte-for-byte and OBSERVABLE holds
        // at ALL SIX, which is the expected shape: every law that
        // landed is a COMBAT-window field law and mc1l5 is the militia
        // level. The m4 chase now takes its target-lost verdict BELOW
        // the shared re-bear (`sub_1BB20` = `sub_1A120(a1x, 24, …)`,
        // :21654-61), so a militiaman whose target dies turns onto the
        // corpse on his exit tick — his `+34` moves, and nothing else
        // does. Beside it: the (10,12) possess flash carrying its
        // ctor's `+44 = -1536` into the ch1 claim it broadcasts (the
        // amount MC1's intake never reads), and the class-12 ctor
        // stamping its whole row (`+50`/`+136`/`+140` and both life
        // words) on every grant, not spell 16 alone.
        // ⭐ ALL SIX re-pinned for THE RIVAL SPEED-TOKEN SESSION —
        // REAL behavior, and mc1l5 is the level that measures it.
        // Vodor enters this take under a 248-tick Accelerate burst,
        // and the port now runs retail's whole token handler at the
        // token's own pool slot (`sub_56380` :65131-99): the v_14
        // kill, the `+16` ACTIVE bit, the SNAPPED 3x/2x speed
        // override and `sub_55E80`'s debit/regen-pin. Beside it the
        // mana census now credits a counted entity's store through
        // `+144` alone (`sub_48340` :56911-19), so an authored castle
        // whose owner sits only in `+24` feeds NOBODY until its
        // established tick echoes the owner across. Post-init moves
        // because the ceiling is a post-init hashed field; every
        // later window moves because the rival genuinely flies,
        // spends and regenerates differently. Corpus: mc1l5's
        // free-run horizon 0 -> 491 boundaries and mc1l4's 1 -> 256,
        // with l0/l1/l2/l42 each still ONE segment end to end.
        // ⭐ ALL SIX re-pinned for THE CASTLE MACRO-STATE session. The
        // castle's job byte `+70` is the DISPATCH KEY (the three rows
        // at :4673-75 — 4 settled / 5 transforming / 6 leveler), not a
        // spare: the port had fused both levels of the machine into
        // `f59` and parked every castle at `tick70 = 5` forever, which
        // made the rival's upgrade predicate (`castle.tick70 == 4`,
        // :18428) unreachable in every free run. POST-INIT moves for a
        // LAYOUT reason alone — the authored castle keeps the ctor's
        // `+70 = 5` now that `spawn_starting_castle`'s (inert, and
        // once harmless) `tick70 = 4` override is gone, matching the
        // mint at :54974-55002 which never writes the job byte.
        // ⭐ OBSERVABLE HOLDS BYTE-FOR-BYTE AT ALL SIX below, so this
        // re-pin is LAYOUT-ONLY by the companion's construction: what
        // moved is the castle's own state byte, and nothing inside
        // these six windows plays differently for it. The evidence
        // that the law does work is the corpus — mc1l5's free-run
        // horizon 932 → 2498 boundaries on this change alone, with
        // l0/l1/l2/l42 each still ONE bit-exact segment.
        // ⭐ ALL SIX re-pinned AGAIN, same session, for THE DWELLING
        // CTOR — and this half is REAL behavior, stated as such.
        // `sub_3B690` (:47501) closes on `sub_36FA0_37360(event, 177)`
        // and `sub_36DF0_371B0` hands the build row to
        // `sub_37150_37510` (:43798), which writes the `+78 = 0xE000`
        // z-center marker along with the extents; the occupancy cap
        // `+128` is the footprint area over FOUR (corpus-measured on
        // build rows 25/26/30/53 — the lift's `>> 4` misses every one
        // by 4x). So every authored house on this level now wears its
        // art, aims from its FOOTING instead of its roof, and holds
        // four times the villagers it did. Corpus: mc1l5's free-run
        // horizon 4226 -> 4441 and mc1l3's 710 -> 1858, with
        // l0/l1/l2/l42 each still ONE bit-exact segment.
        // ⭐ B..E re-pinned for THE SPEED TOKEN'S ARM CHIME — the last
        // deliberately-unported line of the rival speed-token machine
        // (:65158, `sub_55370_558A0(owner, -1, 19)`). Case 19 of the
        // router (:64525-47) is the PLAIN POSITIONAL group: it carries
        // no local-player arm at all, so this chime always sounded for
        // every wizard and the port emitted it for none. OBSERVABLE
        // HOLDS BYTE-FOR-BYTE AT ALL SIX — the sounds vec is hashed
        // into the state digest and not into the observable
        // projection, so this re-pin is a SOUND-ONLY one by
        // construction. Post-init and A hold because the burst arms
        // after them. A/B-measured against the same tree with the two
        // emits stubbed out: the hashes below revert exactly to the
        // previous pin, so nothing else in this change set (the hit
        // flash's anim step, the `+58` import normalization) touches
        // this level's goldens at all.
        // ⭐ D/E re-pinned for THE REPEAT-FIREBALLS LAUNCHER LAW
        // (mc1l4 t=5376): spell 23's command arm is retail's bare
        // LABEL_20→LABEL_32 launcher flow (sub_46B00 :55893) and its
        // fire machine sub_58240 (:66296) is byte-identical to
        // fireball's sub_56090 — so the right hand's held stream in
        // the D window now ARMS on the press tick and fires from the
        // TOKEN one lap later, one ball per held tick at the FULL
        // per-shot cost (the sub_55E80 delta debit), not the old
        // command-site immediate fire at cost/count. Every D ball
        // launches one tick later and the mana ledger drains 3x, so
        // combat and its aftermath genuinely differ. Post-init..C
        // hold byte-for-byte — nothing casts 23 before D. Corpus:
        // mc1l4's segmented deviations 1,358 → 4 on this law alone,
        // l0/l1/l2/l42 each still ONE bit-exact segment.
        // ⭐ ALL SIX re-pinned for THE mc1l4 CERTIFICATION SESSION —
        // LAYOUT-ONLY by the companion's construction: OBSERVABLE
        // holds byte-for-byte at every window. What moved is the
        // rivals hash lane's serialized bytes: `Rival::acq`, the +532
        // ACQUISITION LIST (pickup order — the death scatter and the
        // respawn re-grant iterate IT, not the spell-id book), is
        // state from init on, so every window's digest shifts while
        // nothing inside them plays differently. The session's other
        // laws (the tick-top class-9 roster `var_u32_36462[3]` under
        // the rival defense scan, the tick-top bucket[0] membership
        // under the projectile acquire, the m1 vulture's grave-hunt
        // trailer sub_1B200) never fire inside these windows — no
        // rival is pelted, dies or leaves a grave here. Corpus:
        // mc1l4 devs 4 → 0, CERTIFIED in BOTH instruments; mc1l5
        // devs 2,066 → 2,054; l0/l1/l2/l42 each still ONE bit-exact
        // segment.
        // ⭐ A..E re-pinned for THE CASTLE-TOKEN LADDER STAMP (the
        // mc1l3-certification session): sub_47DD0's every-tick
        // re-price of the owner's Create-Castle token runs for ANY
        // owner whose castle is BOUND (the first-commit latch, flags
        // bit 1 — retail's wizext+50 written only by the level-up
        // commit :56484), not for the human alone. Level 005's
        // authored rival castles commit their authored level on
        // their first tick, so every rival token re-prices from t≈2
        // and the hashed rivals/pool lanes shift at every window
        // after post-init. LAYOUT-ONLY, PROVEN: OBSERVABLE holds
        // byte-for-byte at ALL SIX — no rival casts the castle spell
        // inside these windows. Corpus: mc1l5's rival upgrade at
        // t=5152-5155 pins the machine (token 679 reads 10000/99 at
        // t=3 and 20000/198 after the t=5153 upgrade, both matched);
        // l0/l1/l2/l3/l4/l42 all bit-exact END in both instruments.
        // ⭐ ALL SIX re-pinned for THE mc1l32 CERTIFICATION SESSION's
        // three laws. (1) THE M5 CTOR f58 PHASE SEED: model 5's row-17
        // mint site (:45004) carries the `v26 - (ord % v26) + 4`
        // phase-spread like rows 14/16/21/24, not the flat 64 the port
        // filed it under (mc1l32 t=33135 pins it: the village-trigger
        // dwellers at slots 54/56 read 30/29 for ordinals 4/5) — the
        // seed byte is hashed state from the authored mints on, so
        // post-init..B move in GOLDEN while OBSERVABLE holds there
        // (a wake-phase byte with no behavioral divergence yet).
        // (2) THE CHASE LOST TEST IS THE RECORD'S BYTES (:21658):
        // `+12 < 0 || (+17 & 4)` and no class test — a pack-recruited
        // chaser holding `+146 = 0` keeps hunting the all-zeros
        // scratch record for its whole v_26 window (mc1l32 t=33144)
        // where the port's `class64 == 0` conjunct dropped it to
        // WANDER on the same tick. (3) THE PLAYER KNOCK ARM gates on
        // `src != 0` alone (:55711) — a source freed between the post
        // and the drain still bears the knock off its stale record
        // (mc1l32 t=29923). OBSERVABLE moves at C/D/E — REAL behavior
        // by design: chasers persist through pack handoffs and the
        // house-emit m5s wake on retail's phase. Corpus: mc1l32 free
        // run 16 excess resets -> 2, horizon 29,923 -> 45,231, final
        // segment bit-exact to END; l0/l1/l2/l3 re-verified END
        // bit-exact under all three laws.
        0xac013c377e17a279, // post-init
        0x1425a09285152c87, // A
        0x17b7bac172ad5992, // B
        0xd91a848f4264dd7e, // C
        0x9511b8b62e2b1e05, // D: 64 ticks of two-hand fireball combat
        0x8e5f8f10143c5c65, // E: 100 aftermath ticks
    ];
    assert_eq!(
        got, GOLDEN,
        "state hash diverged from the golden fixture — if this change \
         in behavior is DELIBERATE, re-pin (run with --nocapture) and \
         say so in the commit"
    );

    // The layout-INDEPENDENT companion golden: the observable
    // projection (poses + terrain + population) at the same
    // checkpoints. It must SURVIVE hashed-layout re-pins — when GOLDEN
    // moves but OBSERVABLE holds, the re-pin is layout-only by
    // construction; if OBSERVABLE moves too, behavior moved and the
    // claim must say so.
    // The castle-footprint re-pin above moves post-init ONLY: the
    // authored castles stamp one ring less terrain at load, and
    // nothing downstream diverges — A-E hold byte-for-byte, which is
    // the evidence that the fix changed the castles' footprint and
    // not the way the level plays.
    // The m12 APPROACH re-pin and the class-10 PRE-decrement batch BOTH
    // move OBSERVABLE at B-E, and that is the correct signal: these are
    // behavior changes, not layout changes. Settlers arrive later, and
    // every fire, splash, flash, tether and cloud lives one tick longer,
    // so populations and poses at B-E genuinely differ. Post-init and A
    // hold — nothing has died and no settler has thought yet.
    // The walk-in silent-absorb fix (mob_death now vanishes militia and
    // retired settlers that enter a house, matching retail's per-model
    // death slots, instead of dropping them into the corpse path whose
    // 400-dmg flame destroyed the dwelling and churned the village) moves
    // B-E again: those creatures no longer corpse, so no flame, no house
    // damage, and the populations that survive differ. A still holds.
    //
    // The whole array then re-pins once more — including post-init and A
    // — for a PRESENTATION change, NOT a behavior one: `live_poses` now
    // keeps unclaimed MC1 dwellings in the pose set (as `map_only`, so no
    // billboard and no map dot) purely so the debug health-bar overlay
    // can cover them. `observable_digest` hashes the pose set, so the
    // extra (unclaimed, always-present) house poses shift every
    // checkpoint. The raw GOLDEN state hash above is UNCHANGED — proof
    // the sim itself did not move.
    // The feeder-wander leash fix moves OBSERVABLE at B-E as well —
    // a behavior change by design: villagers steer home instead of
    // diffusing, walk in the door in different ticks, and the act
    // speeds they wear differ. Post-init and A hold.
    // The corpse-flame spreader re-pin (sub_25130: skip law
    // `v5 % 157 >= 79`, jitter drawn only on the spawn branch, f30
    // inherit) moves GOLDEN+OBSERVABLE at D-E only — kills → corpse
    // flames live in the combat/aftermath stages; post-init/A/B/C
    // hold byte-for-byte. Behavior change toward retail by design.
    // The MC1 ball-physics re-pin moves OBSERVABLE at A-E — a behavior
    // change by design (see the GOLDEN note): loose balls settle after
    // their 128-tick ballistic window, roll downhill while grounded,
    // and merge only on grounded ticks, so ball poses and populations
    // at every checkpoint genuinely differ. Post-init holds.
    // The bounce-floor re-pin moves OBSERVABLE at A-E — a behavior
    // change by design (see the GOLDEN note): -33..-64 impacts no
    // longer hop, so ball rest poses at every checkpoint differ.
    // Post-init holds.
    // The tick-top reap re-pin moves OBSERVABLE at D-E — a behavior
    // change by design (the retail reap law, see the GOLDEN note):
    // every 0x400 kill's record is present for one more snapshot
    // (poses/populations include the dying frame, as retail draws
    // it) and same-tick spawns take pre-existing stack slots, so
    // combat (D) and aftermath (E) pose sets genuinely differ.
    // Post-init..C hold — nothing dies before D.
    // The castle-collateral re-pin (marker + acquire + pre-decrement
    // cloud, see the GOLDEN note) holds OBSERVABLE at EVERY leg:
    // nothing in this window attacks a castle or casts a napalm
    // cloud, so the moved layout lanes are the castle +78/+144
    // fields and the acquire-latch bits alone.
    // The mana-ball WAKE law (see the GOLDEN note) moves B-E — REAL
    // behavior: near-wizard settled balls resume rolling on the
    // 17-tick duty cycle, so ball rest poses from the dig window on
    // differ. Post-init + A hold (no ball inside the far-afield
    // radius).
    // The 180° TURN TIE-BREAK law (see the GOLDEN note) moves B-E —
    // REAL behavior: a creature whose wander target sits at the exact
    // antipode now commits its capped turn in retail's direction, so
    // wander poses diverge from the first half-turn tie on.
    // The m2 bee laws move OBSERVABLE at C-E — REAL behavior by
    // design: the acquisition lunge (arm + buzz) fires in the C
    // ambush window, and chasing bees now step z toward their victim
    // every tick, so creature poses from C on genuinely differ.
    // Post-init..B hold — no bee has acquired before C.
    // The ball vertical-law fix moves OBSERVABLE at A-E — REAL
    // behavior by design: a ball landing EXACTLY on the ground keeps
    // its fall lift one more tick (strict below-ground clamp,
    // :29538 / EF:26244), so every mid-settle ball's bounce phase
    // shifts one tick. Retail-correctness is corpus-certified: the
    // mc1l0 pure replay's bit-exact horizon moves 1 → 62 boundaries
    // on this change alone (the t=2 z+32 cohort), while all 9
    // per-pair conformance suites hold green. Post-init holds — the
    // law first acts on tick 1.
    // The aim-z bracket law (see the GOLDEN note) moves D-E — REAL
    // behavior by design: the D-window fireballs acquire and home on
    // the ambush m2 BEES at their RAW z (retail's sub_524C0 model-2
    // guard), so projectile pitches, flight paths and downstream
    // combat genuinely differ. Post-init..C hold — nothing is aimed
    // at before D.
    const OBSERVABLE: [u64; 6] = [
        // ⭐ THE DWELLING CTOR moves OBSERVABLE at ALL SIX, unlike the
        // castle macro-state re-pin above it, and that difference is
        // the whole verdict: `+78` is the sprite half-height AND the
        // aim lift, and `+128` is the occupancy cap, so the houses
        // draw differently, offer a different aim point, and take four
        // times the villagers off the map. Behavior, by design.
        0x2dea118b36808b49, // post-init — + unclaimed-dwelling poses
        0x9ef97ad683928ffc, // A
        // B..E re-pinned with THE mc1l2 RIVAL-WIZARD SESSION (see the
        // GOLDEN note). OBSERVABLE moving from B on is the CORRECT
        // signal — REAL behavior by design: Vodor heals through
        // hits at the register rate, holds his fire until 3-D
        // arrival, chain-boosts on the speed token and hunts along
        // the chain buckets, so his flight path, casts and every
        // population he touches differ from the dig window on.
        // Post-init and A hold — the AI has not diverged in the
        // far-afield idle.
        // B-E re-pinned again with THE mc1l2 RESIDUE SESSION —
        // post-init and A hold byte-for-byte, which is the whole
        // verdict: nothing changed in the far-afield idle, and every
        // moved window is a rival/creature machine genuinely playing
        // differently (boundary-tick casts, tick-top ball picks, no
        // militia house rung, bee pre-work on death ticks, knocked
        // corpses).
        // D/E re-pinned with the state pins above (THE CAST-PHASE
        // LAW + the area-broadcast tile rounding): the D-window
        // fireballs launch at arm+1 with the corpus f140 stamp and
        // their explosion mail lands edge-true — A-C unmoved.
        // D/E re-pinned again with THE CHANNEL-0 WINDOW BIAS (see the
        // GOLDEN note): OBSERVABLE moving here is the CORRECT signal —
        // this is a behavior change, not a layout one, because which
        // bees an explosion's ch0 mail reaches is exactly what the
        // window decides. Post-init..C hold, which is the evidence
        // that the shift is confined to the ch0 damage windows.
        // D/E re-pinned with the ONE-SHOT ACQUISITION LATCH (see the
        // GOLDEN note). OBSERVABLE moving here is again the CORRECT
        // signal: fireballs that used to hunt for their whole flight
        // now commit at the muzzle, so trajectories, impacts and the
        // populations that survive the D window genuinely differ.
        // Post-init..C hold — nothing is fired before D.
        // D/E re-pinned with THE PROJECTILE PROBE RING + THE THUNK
        // MUZZLE LAWS (see the GOLDEN note). OBSERVABLE moving is
        // the CORRECT signal — REAL behavior by design: which victim
        // a D-window fireball strikes mid-flight is exactly what the
        // probe window decides (retail's narrow forward-biased ring
        // vs the old inflated square), and the ambush bees' return
        // bolts now launch at the unlifted-muzzle pitch. Post-init..C
        // hold — nothing is fired before D.
        // C..E re-pinned with THE TRIGGER PRE-MOVE POSE LAW (see the
        // GOLDEN note). OBSERVABLE holding at B and moving from C is
        // the CORRECT signal for a pure timing law: the crater dig's
        // outcome is unchanged (B byte-identical), while the C
        // ambush's one-tick-later fire shifts every spawn's ctor
        // draws, poses and downstream combat.
        // D/E move with THE mc1l42 RESIDUE SESSION's combat laws (see
        // the GOLDEN note) — REAL behavior, stated as such: militia
        // forget their stale targets instead of re-acquiring, damaged
        // mounds re-arm, damaged bees lunge, a genie that breaks off
        // still looses one steal seeker, and a fireball whose tracker
        // is further than 32767 away aims the LONG way round. Combat
        // in D therefore lands differently and the aftermath in E
        // inherits it. Post-init..C hold, so nothing before the
        // combat window moved.
        // ⭐ B..E re-pinned for THE RIVAL SPEED-TOKEN SESSION, and
        // OBSERVABLE moving is the CORRECT signal: this is behavior,
        // not layout. Vodor's Accelerate burst now drives his speed
        // columns the way retail's does (snapped 3x on the arm tick,
        // 2x while it holds, base on expiry) and ends the moment his
        // brain retakes v_12, while his purse no longer regenerates
        // under an active spell and his authored castle no longer
        // inflates his ceiling. His flight path, his casts and every
        // population he touches therefore differ from the dig window
        // on. Post-init and A hold byte-for-byte — the far-afield
        // idle is unmoved, which is the evidence that nothing outside
        // the rival column changed.
        // D/E re-pinned with THE REPEAT-FIREBALLS LAUNCHER LAW (see
        // the GOLDEN note). OBSERVABLE moving is the CORRECT signal —
        // REAL behavior by design: the right hand's stream launches
        // one tick later and pays full cost per shot, so D's
        // projectile poses, impacts and the populations that survive
        // genuinely differ. Post-init..C hold — nothing casts 23
        // before D.
        0x43bf391b4fe16821, // B — settler phase + feeder leash
        // C..E re-pinned with the mc1l32 certification session (see
        // the GOLDEN note): the scratch-chase persistence, the m5
        // wake phase and the freed-source knock arm first bite in the
        // C window and compound through combat/aftermath.
        0x3ab68929d7e226fd, // C
        0xd5f0bb85237a1c61, // D
        0x325841fe2feb9fe0, // E
    ];
    assert_eq!(
        obs, OBSERVABLE,
        "the OBSERVABLE projection diverged — this is a behavior \
         change, never a layout-only one"
    );
}
