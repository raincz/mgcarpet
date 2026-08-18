//! Refactor guard for the `Simulation` tier — the flight state that
//! lives OUTSIDE [`World`] and which `World::state_hash` cannot see:
//! the tick counter, the float `Flyer` pose, both carpet structs and
//! the two G-class model selectors.
//!
//! This gap matters most for save/load (`docs/archive/DESIGN-SAVES.md`): a
//! restored world can be byte-perfect while the carpet resumes at the
//! wrong speed or aim, and no world-level golden would notice. The
//! `hash_sees_*` tests below are the coverage proof; the goldens are
//! the ordinary refactor pin.
//!
//! Same re-pin protocol as `state_hash.rs`: run with `--nocapture`,
//! copy the printed array, and say in the commit that the behavior
//! change was deliberate. Self-skips when the baked tree is absent.

use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::World;
use mgc_sim::{AltitudeModel, FlightInput, Simulation, ThrustModel};
use std::path::PathBuf;

#[path = "common/mod.rs"]
mod common;

fn baked_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
    (p.join("mc1/level-005.mgcl").exists() && !common::modded_bake(&p)).then_some(p)
}

/// Level 005 without the authored wizards — the rival column is
/// already pinned by `state_hash.rs`; this fixture is about the
/// flight tier riding on top of a live world.
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
    World::new(planes, &pkg.things.things, seed, assets)
}

fn sim(root: &std::path::Path, thrust: ThrustModel) -> Simulation {
    let mut s = Simulation::with_world(build_world(root));
    s.thrust_model = thrust;
    s.altitude_model = AltitudeModel::Faithful;
    s.sync_carpet_from_flyer();
    s
}

fn steps(s: &mut Simulation, n: usize, input: &FlightInput) {
    for _ in 0..n {
        s.step(input);
    }
}

/// Cruise, bank into a turn, then coast — the three regimes where the
/// carpet carries state a world snapshot cannot reconstruct.
fn run(root: &std::path::Path, thrust: ThrustModel) -> Vec<u64> {
    let mut s = sim(root, thrust);
    let mut out = vec![s.state_hash()];

    // A: sustained forward thrust from rest (speed ramp + terrain follow).
    steps(
        &mut s,
        40,
        &FlightInput {
            thrust: 1.0,
            stick_y: -40,
            ..Default::default()
        },
    );
    out.push(s.state_hash());

    // B: hard right turn with strafe — exercises the roll/pitch stick
    // filters and the second polar step.
    steps(
        &mut s,
        30,
        &FlightInput {
            thrust: 1.0,
            strafe: 1.0,
            stick_x: 96,
            stick_y: -20,
            yaw_delta: 0.05,
            ..Default::default()
        },
    );
    out.push(s.state_hash());

    // C: hands off — momentum decays, the filters settle.
    steps(&mut s, 40, &FlightInput::default());
    out.push(s.state_hash());

    out
}

#[test]
fn flight_tier_golden_state_hashes() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let faithful = run(&root, ThrustModel::Mc1);
    let enhanced = run(&root, ThrustModel::Enhanced);
    assert_eq!(
        (faithful.clone(), enhanced.clone()),
        (
            run(&root, ThrustModel::Mc1),
            run(&root, ThrustModel::Enhanced)
        ),
        "sim is not deterministic"
    );
    println!("faithful: {faithful:#018x?}\nenhanced: {enhanced:#018x?}");

    // Re-pinned for the m12 settler transcription fixes (sub_1EED0
    // :25077-84, sub_1F120 :25165-70): retail tests the PRE-decrement
    // +26 in both WANDER and APPROACH, and C precedence makes its
    // think gate `(f63 % v_26) / 2`, not `f63 % (v_26 / 2)`. The
    // settler's ent_rand phase at BUILD therefore shifts, moving where
    // and when it plants. Post-init and A hold in both thrust models;
    // the settler's think only diverges past ~40 ticks.
    //
    // Re-pinned again for the rest of sub_1F120's APPROACH shape
    // (:25164-77): the walk runs before the think gate on every tick,
    // the re-aim and the proximity promotion run only INSIDE it, the
    // patience/dead-anchor bail falls through instead of returning,
    // and the range test is the three-axis rooted distance. Only the
    // Re-pinned for the enhanced-flight enhancements (2026-07-22/23
    // rulings, culminating in chase-the-pointer steering + the
    // desired-altitude law). BOTH arrays move on every steering-state
    // reshape, for different reasons: FAITHFUL only because the
    // steering/altitude fields (aim_lead, turn_rate, lift_desired,
    // lift_unclamped) feed the hash — the faithful trajectory itself
    // is untouched (this fixture runs AltitudeModel::Faithful, where
    // the new law never executes, and the integer movers were not
    // modified). ENHANCED moves because chase steering genuinely
    // changes the yaw path — the deliberate deviation itself.
    //
    // Re-pinned for the m13/m14 feeder-wander transcription fixes
    // (sub_1F640 :25382-25438 / sub_1FAC0 :25558-25614): the door
    // radius is tested BEFORE fullness on the rooted 3-axis distance
    // (a full home still pulls its villager — the village leash), the
    // anchor drop/acquire swap the act speed (+126 = +130 / +128), and
    // the m14 distant filter runs INSIDE the acquire loop. Post-init
    // holds; A-C move because village feeders think within the window.
    //
    // Re-pinned for the class-2 static tick port (sub_49AA0/sub_49AD0/
    // sub_49B50): stones, dolmens and bad stones now terrain-snap and
    // stamp the +18 |= 2 static draw bit every tick. Post-init holds
    // (the stamp first lands on tick 1); A-C move. Layout-only: the
    // stamp is the whole delta (see the state_hash.rs pin note — the
    // snap is an identity write on static terrain, OBSERVABLE holds).
    // Re-pinned (layout-only) for the `rival_wanted` per-rival village-
    // wanted timers joining the Gen hash: both arrays move because the
    // world state hash grew a field, though the faithful trajectory and
    // the enhanced steering are untouched (no rival is flagged wanted in
    // this flight fixture — the delta is the zeroed field alone).
    // Re-pinned for the MC1 ball-physics conformance fixes (sub_27030
    // :29518-64): the ballistic arm now gates on the +58 settle
    // countdown (128 ticks, then frozen at rest), applies friction and
    // the downhill terrain roll only while GROUNDED (the old MC1 arm
    // ran friction unconditionally and never rolled — contradicted by
    // its own source cite and the retail corpus), and merge donors
    // hard-free (sub_41E90) instead of soft-killing to the sweep.
    // Post-init holds; A-C move because the level's authored balls
    // follow the new rest law inside the window.
    // Re-pinned for the MC1 ball bounce floor (:29538-49): retail
    // zeroes any rebound <= 16 (`if (f46 <= 16) f46 = 0`), so a ball
    // rebounds only past impact -64; the port kept 8..16-unit hops
    // from -33..-64 impacts. MC1-scoped (the MC2 sphere twin is
    // untraced and keeps -32). Post-init holds; A-C move with the
    // authored balls' settle trajectories.
    // Re-pinned (all legs) for the worm-segment id24 fix: segments
    // keep the head's +24 through the byte-copy (corpus-pinned);
    // layout-only — the L005 OBSERVABLE companion holds.
    // Re-pinned (leg B only, both models) for the MC1 tick-top reap
    // law (:52226-31): a record killed via the 0x400 flag now
    // persists through its death tick's snapshot and frees at the
    // top of the next tick. A transient dies inside leg B's window,
    // so B's pool bytes carry the one-frame death record; C holds
    // because the linger never re-ticks and no allocation raced the
    // freed slot — the trajectory is untouched.
    // Re-pinned (A-C, both modes; post-init holds) for the mana-ball
    // WAKE law (sub_54F80 :64352-66, corpus-proven on mc1hwl0):
    // settled balls near the human re-arm +58 = 16 on the awake
    // pass's exact 17-tick cycle, and the ballistic gate now reads
    // the post-maintenance value (retail's handler order — a ball
    // freezes for the 1 observe-zero tick per cycle and its fresh
    // window ends at the counted zero, not one tick later).
    // Behavior change toward retail by design.
    // Re-pinned (B-C both banks) for the 180° TURN TIE-BREAK law
    // (Gen::turn_sign, mc1/mobs.rs — see the L005 GOLDEN note):
    // ambient creatures whose wander target sits at the exact antipode
    // now turn in retail's direction. Behavior change toward retail by
    // design; post-init + A hold (no tie occurs that early).
    // Re-pinned (A-C, FAITHFUL only) for the two pose-channel corpus
    // fixes (flight.rs): the flutter roll tests the +63 clock BEFORE
    // the tick's bump (retail :55294 tests the settled value — every
    // draw landed one pair late under the post-increment order), and
    // both-strafes-held resolves RIGHT, never to release (:55783-86 /
    // EF:60793-96 are sequential bit tests). Behavior change toward
    // retail by design, corpus-proven on mc1l0 (99.9% pose-channel
    // bit-exactness with the fixes, rand/strafe lanes at zero).
    // ENHANCED holds — neither law runs outside the classic mover.
    // Re-pinned (A-C, both modes; post-init holds) for the ball
    // vertical-law conformance fix (ball_tick, :29532-49 / EF:26188-
    // 26252, the twins verbatim): gravity integrates every moving
    // tick, the ground clamp+rebound requires STRICTLY below (an
    // exact landing keeps its fall lift one more tick — the mc1l0
    // replay t=2 cohort), and grounded contact (roll/friction/merge)
    // is post-clamp z == ground. The authored balls' settle
    // trajectories shift one tick; per-pair conformance observables
    // hold (all 9 suites green across the change).
    // FAITHFUL C re-pinned for the AWAKE-PASS POSE PHASE (the
    // pre-pass proximity gate samples the PREV frame's carpet — the
    // pool-entity read :64352-53; see the mc2_slice GOLDEN note):
    // the coast track crosses a background ball's 24-tile gate
    // mid-window, so its 16-of-17 duty-cycle re-arm now lands on
    // retail's tick — one later. ONLY faithful C moves: the
    // enhanced mover flies a different track (no crossing), and
    // A/B/post-init precede any gate edge. Behavior change toward
    // retail by design (mc1l0 replay horizon 413 → 561).
    // Re-pinned (ALL FOUR, both modes — post-init included) for the
    // MANA-BALL CTOR STAMPS (mc1l0 (10,39) dig): `spawn_mana_ball`
    // now stamps +66/+67 = 10/39 and +126 = 32 like retail's ctor
    // (sub_3B5A0 :47456-57/:47463), and the level's authored balls
    // load through that ctor — three hashed fields shift on every
    // ball from tick 0. Motion-inert (ball physics reads none of
    // them; no magnet/claim/corpse event fires in these windows), so
    // the flight tracks are untouched — bookkeeping toward retail's
    // byte image.
    // FAITHFUL B-C re-pinned for the CORPSE-DROP f34 MIRROR removal
    // (corpse_drop_mc1 — retail sub_27690 :29663 writes only +30): a
    // drop landing in those windows is born target_yaw 0 like
    // retail. Motion-inert (the ball tick never reads f34); ENHANCED
    // holds (its track sees no drop). Bookkeeping toward retail's
    // byte image — the mc1l0 pair-564 (10,39) family at 0.
    // BOTH arrays re-pinned for THE mc1l42 RESIDUE SESSION, and for a
    // SINGLE cause: a THING-placed spell jar now runs the real ctor
    // (`sub_3BF70` :47979-48013, driven by the per-spell thunks off
    // `off_987DE[model]` :48020-161) instead of the inert stand-in, so
    // the level-load class-12 records carry retail's own
    // life/max_life/flags/f136/f140/f50/f44 instead of zeros. That is
    // hashed state at POST-INIT, which is why every checkpoint moves in
    // both tiers. A/B-proven: `MGC_NO_MC1_JAR_CTOR=1` restores the old
    // hashes exactly, in this test and in level_005. The jars do not
    // move and nothing reads the corrected fields in these windows —
    // the flight tracks are untouched, and level_005's OBSERVABLE
    // companion holds byte-for-byte at ALL SIX checkpoints, which is
    // the evidence that this is a byte-image correction and not a
    // behavior change. Corpus: mc1l42 6 field rows -> 0 across
    // t=3267/15575/16153/16782/25401/26579.
    // ⭐ BOTH arrays re-pinned for THE DWELLING CTOR session, and the
    // move is LAYOUT: `sub_3B690`'s closing `sub_36FA0_37360(event,
    // 177)` sprite stamp and `sub_37150_37510`'s `+78 = 0xE000`
    // z-center marker now land on every (10,45), and the occupancy cap
    // `+128` is the build-row area over FOUR (corpus-measured on rows
    // 25/26/30/53; the lift's `>> 4` misses each by 4x). Post-init
    // moves because all three are hashed load-time fields on this
    // level's authored houses; the later checkpoints inherit that
    // image. The flight tracks themselves are untouched — no house is
    // in either carpet's path and the ball physics reads none of these
    // fields.
    const FAITHFUL: [u64; 4] = [
        0x1cf548af8b0245a8, // post-init
        0xef4f1d5818878469, // A: 40 ticks of forward thrust
        0x2f9c1198de59746b, // B: 30 ticks of banked turn + strafe
        0x4e70d1d4dc0201d6, // C: 40 ticks of coast
    ];
    // Re-pinned for the enhanced-bank strafe fix (2026-07-27): the
    // proportional camera bank no longer gates off while strafing — it
    // follows the forward-velocity projection, which is nonzero in
    // scenario B (strafe + turn under thrust). ONLY the ENHANCED B hash
    // moves: `flyer.roll` is the sole differing field, it is camera-only
    // (never feeds position/velocity), and B is the one snapshot taken
    // mid-strafe. A/post-init carry no strafe; C recomputes roll with no
    // strafe held, so its roll — and thus its hash — is unchanged. The
    // FAITHFUL array is untouched (the Mc1 mover never runs this bank).
    // ENHANCED A-C re-pinned with the same ball vertical-law fix —
    // the background balls evolve identically under both movers.
    // ENHANCED re-pinned with the same ball-ctor stamps — the
    // background balls carry them under either mover.
    const ENHANCED: [u64; 4] = [
        0x18794b6f3eb902f5, // post-init
        0xb7d214212f4ac164, // A
        0x57fd578078fae766, // B: strafe+turn now banks on forward speed
        0xea0902a198a99189, // C
    ];
    assert_eq!(
        (faithful, enhanced),
        (FAITHFUL.to_vec(), ENHANCED.to_vec()),
        "flight-tier state hash diverged — if this behavior change is \
         DELIBERATE, re-pin (run with --nocapture) and say so in the commit"
    );
}

/// The coverage proof, and the reason this file exists: two sims whose
/// WORLDS are identical but whose carpets differ must not hash alike.
/// If this ever passes trivially, `Simulation::state_hash` has stopped
/// covering the flight tier and every save/load fixture built on it is
/// blind.
#[test]
fn hash_sees_the_carpet_when_the_world_cannot() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let base = sim(&root, ThrustModel::Mc1);

    let mut moving = sim(&root, ThrustModel::Mc1);
    moving.carpet.act_speed = 80;
    assert_eq!(
        base.world.as_ref().unwrap().state_hash(),
        moving.world.as_ref().unwrap().state_hash(),
        "fixture is wrong: the worlds must be identical for this to prove anything"
    );
    assert_ne!(
        base.state_hash(),
        moving.state_hash(),
        "carpet speed must reach the sim hash"
    );

    let mut aimed = sim(&root, ThrustModel::Mc1);
    aimed.carpet.aim_pitch = 200;
    assert_ne!(
        base.state_hash(),
        aimed.state_hash(),
        "aim must reach the hash"
    );

    let mut drifting = sim(&root, ThrustModel::Enhanced);
    drifting.flyer.vx = 1.0;
    let mut still = sim(&root, ThrustModel::Enhanced);
    still.flyer.vx = 0.0;
    assert_ne!(
        still.state_hash(),
        drifting.state_hash(),
        "float velocity must reach the hash"
    );

    // Sign-of-zero must not alias: the float fields hash by bit
    // pattern, not by value.
    let mut neg_zero = sim(&root, ThrustModel::Enhanced);
    neg_zero.flyer.vx = -0.0;
    assert_ne!(
        still.state_hash(),
        neg_zero.state_hash(),
        "floats must hash by bit pattern, not by value"
    );
}

/// The tick counter is real state: MC2's cave-drip cadence keys off
/// it, and a restore that resets it to zero silently re-phases the
/// world (`World::mc2_turn` is hash-excluded precisely BECAUSE it is
/// a function of this counter).
#[test]
fn hash_sees_the_tick_counter() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let a = sim(&root, ThrustModel::Mc1);
    let mut b = sim(&root, ThrustModel::Mc1);
    b.tick += 1;
    assert_ne!(a.state_hash(), b.state_hash(), "tick must reach the hash");
}

/// The two G-class selectors change the simulation, so they belong in
/// the digest (and, in due course, in every save header).
#[test]
fn hash_sees_the_model_selectors() {
    let Some(root) = baked_root() else {
        common::golden_skip("baked data not present");
        return;
    };
    let faithful = sim(&root, ThrustModel::Mc1);
    let enhanced = sim(&root, ThrustModel::Enhanced);
    assert_ne!(
        faithful.state_hash(),
        enhanced.state_hash(),
        "thrust model must reach the hash"
    );

    let mut lifted = sim(&root, ThrustModel::Mc1);
    lifted.altitude_model = AltitudeModel::ExtendedLift;
    assert_ne!(
        faithful.state_hash(),
        lifted.state_hash(),
        "altitude model must reach the hash"
    );
}
