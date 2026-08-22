//! Retail-bug patch switches — the sim half of the `gameplay ·
//! patches` option class (docs/DEVIATIONS.md "Patch options").
//!
//! Each field is one deliberate upstream bugfix with BOTH arms
//! implemented: `true` runs the patched (fixed) behavior, `false`
//! runs retail's shipped bug. The struct is config-like — never part
//! of the state hash or the snapshot stream — and defaults to
//! [`WorldPatches::RETAIL`] at world construction, so every direct
//! `World::new*` consumer (goldens, unit tests, mgc-conform, which
//! never reads app config) evolves under retail law unless the app
//! explicitly opts a patch in. Conformance imports additionally
//! re-force RETAIL as a belt (`World::strict_retail` remains the
//! overriding kill-switch at the gated sites).
//!
//! Reach: `World` methods read `self.patches`; Gen-side ticks get it
//! through [`crate::mc1::mobs::MobCtx::patches`] where a ctx already
//! flows, or as an explicit parameter on the castle/building lanes
//! (the `strict` precedent — a Gen field would drag the wholesale
//! `#[derive(Hash)]` and the snapshot codec along).

/// Per-patch switches; `true` = the patched (bug-fixed) arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorldPatches {
    /// **MC1/HW's FIRST-CASTLE LOCKOUT — an unpatched retail BUG.**
    /// Patched = live-law: the cost re-derives from the OWN castle
    /// every query (homeless → ctor 1000). Retail = the stale stamp:
    /// the manifestation's cached cost is rewritten at castle
    /// init/level-up and NEVER on castle death (sub_47C60/sub_47DD0),
    /// so after ANY castle loss the rebuild is priced at
    /// `CASTLE_CAP[0]` = **5,000** against a **1,000** starting purse
    /// — unaffordable until collection pushes the census ceiling past
    /// it. Player-certified on retail; the lockout may be a deliberate
    /// period challenge, so the FIX is the opt-in (player-ruled
    /// 2026-08-08, DEFAULT RETAIL).
    ///
    /// ⚠⚠ **THIS IS AN MC1/HW CONCERN.** MC2 does NOT have the
    /// lockout: its destroy path re-stamps the token at the level-0
    /// rung (**1,000**) before the record frees, so an MC2 castle
    /// destroyed by an enemy is immediately rebuildable. MC2 charges
    /// for teardown through the DESIGNED +3,000 surcharge instead,
    /// which only a VOLUNTARY demolish latches — that is deliberate
    /// behaviour, it does NOT ride this switch, and it is faithful
    /// under BOTH arms (mc2/cast.rs). The one MC2 thing left here is a
    /// narrow corner: the castle-less RELEASE re-sync in mc2/cast.rs,
    /// a release edge with no castle and no intervening ladder stamp.
    /// Do not extend this toggle to cover MC2 design.
    pub castle_recast_cost: bool,
    /// Class-12 jars re-snap to their tile's ground every tick.
    /// Retail's reshape walk skips class 12 (:51729): terrain shaped
    /// over/under a jar leaves it buried (HW ships several) or
    /// hovering.
    pub jar_ground_snap: bool,
    /// A settled (f58 == 0) MC1 mana ball tracks the ground both
    /// directions. Retail freezes it wherever it is — mid-hop balls
    /// hang in the air, terrain edits bury grounded ones.
    pub ball_ground_track: bool,
    /// MC1 mana balls run their roll physics map-wide. Retail
    /// re-arms a settled ball's +58 only within the 24-tile awake
    /// radius of the human (:64352-61), so approaching a downhill
    /// ball wakes it and it visibly "runs away". Balls only — the
    /// creature awake gate is untouched.
    pub map_wide_ball_rolling: bool,
    /// A possessed dwelling keeps its footprint extents under the
    /// owner-flag sprite. Retail's sprite stamp (:30808) clobbers
    /// +78..+84 with the tiny flag extent, collapsing villager-emit /
    /// defender spawns onto the roof — a walled-in corpse-flame loop
    /// that destroys the possessed house from the inside.
    pub possessed_footprint: bool,
    /// MC2 downgrade's 10% capacity haircut computed in i64. Retail's
    /// i32 `10 * x / 100` overflows at the level-7 rung (10 × 300M)
    /// into a NEGATIVE cut — a maxed castle downgrade *raises* its
    /// cap and scatters nothing.
    pub mc2_downgrade_overflow: bool,
    /// MC2 Magic Mine proximity trigger. Retail never writes the
    /// `word_0x36_54` armed gate (magic-mine.md §6) — a shipped mine
    /// floats, expires and sinks without ever detonating on anyone.
    pub mc2_magic_mine: bool,
    /// MC1 Create Castle placement validation. Retail (the "latch
    /// bug", certified on mc1l32-castle-bug.mgcr): the castle ball
    /// spawns at the HAND muzzle (sub_55EF0 — ±256 units at yaw∓512,
    /// steerable by aim), the launch-tick scan samples THAT tile, and
    /// the landing condition short-circuits (`ground > z || life < 0
    /// || !scan`, :63588-90) — a terrain touchdown builds the castle
    /// with no placement check at all; the scan only re-runs on
    /// airborne ticks, where a failure stops the ball early
    /// (flip 180° + one step back) and still builds. Combined with
    /// sub_12F70's NW-only 8×8 window this lets a wall-corner cast
    /// raise a castle inside a no-castle maze and carve its protected
    /// walls. Patched: the ball spawns at the carpet (the scan
    /// samples where you are) and the landing always re-scans
    /// (failure displaces the site one step back).
    pub castle_latch_bug: bool,
}

impl WorldPatches {
    /// Every patch off — retail's shipped behavior, bug for bug. The
    /// world-construction default; what conformance, goldens and
    /// `--record`/`--replay` runs use.
    pub const RETAIL: WorldPatches = WorldPatches {
        castle_recast_cost: false,
        jar_ground_snap: false,
        ball_ground_track: false,
        map_wide_ball_rolling: false,
        possessed_footprint: false,
        mc2_downgrade_overflow: false,
        mc2_magic_mine: false,
        castle_latch_bug: false,
    };

    /// The pre-option behavior set: what native play hard-wired
    /// before the patches became options (2026-08-08). Port
    /// recordings taped before the `--record` force-retail policy
    /// replay under THIS set — it is the sim their inputs were
    /// recorded against. `map_wide_ball_rolling` did not exist then.
    pub const LEGACY: WorldPatches = WorldPatches {
        castle_recast_cost: true,
        jar_ground_snap: true,
        ball_ground_track: true,
        map_wide_ball_rolling: false,
        possessed_footprint: true,
        mc2_downgrade_overflow: true,
        mc2_magic_mine: true,
        castle_latch_bug: true,
    };
}
