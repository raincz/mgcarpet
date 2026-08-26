//! The MC2-native castle column — class-3 model-2 and its court: the
//! three castle actionIndices (4 = standing tick, 5 = build state
//! machine, 6 = destroy-one-level), the MC2 HP/CAP ladder, the
//! straight-subtract intake, the sphere-absorb + overflow-eject mana
//! economy, the (3,3) balloon fleet, the (5,15) guard slots, the
//! (10,42) build painter and the (10,79) defender stage pieces.
//! Citations are `EF:line` into vendored remc2
//! `engine/EventsFunctions.cpp`; traces in docs/traces/mc2-castle-*.md.
//!
//! Structural key: MC1 keeps ONE castle action (5) with `f59`
//! sub-states; MC2 moves the phase to the actionIndex itself —
//! `tick70` 4/5/6 — and `f59` (retail `word_0x2E_46`) is the
//! within-action-5 build sub-state. The quake/whirlwind grab's
//! `f50 = 30` write (mc2::flood) is consumed here as the settle timer
//! (EF:61057-61078): intake pauses while it runs.
//!
//! Retail-law anchors: `word_0x80_128` is the UPGRADE-request channel
//! (written by the delivered castle cast `sub_389F0` EF:28240 with
//! `word_0x7C_124 = 10` — the MC1 ch5 (10, owner) token protocol, so
//! the MC1 (10,43) token serves both columns); `dword_38519` is the
//! class-3 live list (the flood grab targets castles); `sub_60400`
//! returns (balloons, guards), the same quota table as MC1's
//! sub_47400 :56264.
//!
//! Deliberate approximations: `sub_5F890` (Create-Castle HUD widget sync,
//! EF:61029) is a no-op (no ported widget); the balloon/guard slot
//! arrays (`array_0x3C_60`/`array_0x5C_92`) are scan-collected (same
//! membership, no per-slot indices).

use crate::engine::features::{Gen, tile};

/// `sub_60810` (EF:61695): capacity by level. Differs from MC1 at
/// every level >= 1; the level-7 sentinel is 300M (MC1: 30M).
///
/// ⚠ MC2 keeps CAPACITY and PRICE apart — this is capacity only, and
/// the price is [`MC2_CASTLE_COST`] below. MC1 fuses the two into the
/// single `Gen::CASTLE_CAP` ladder, which is why that array must never
/// be reused on an MC2 path.
pub(crate) const MC2_CASTLE_CAP: [i32; 8] =
    [5000, 8500, 18000, 38800, 78600, 158200, 317400, 300_000_000];

/// **MC2 ONLY** — the Create-Castle PRICE by the OWN castle's level,
/// `GetSpellManaCost_6D710`'s switch (Level.cpp:1729-55). Distinct
/// from [`MC2_CASTLE_CAP`] (capacity) and from MC1's fused
/// `Gen::CASTLE_CAP`.
///
/// Versus MC1's ladder, rungs 1..=6 are IDENTICAL — that is shared
/// design, not a shared table — and the two ends differ:
/// * **rung 0: 1,000 here vs MC1's 5,000.** This is the load-bearing
///   divergence. MC1 stamps its 5,000 on EVERY castle teardown against
///   a 1,000 starting purse, which is the accidental FIRST-CASTLE
///   LOCKOUT (an unpatched retail bug — see `castle_recast_cost` in
///   docs/DEVIATIONS.md). MC2 re-prices a destroyed castle at this
///   1,000 and is immediately rebuildable; it charges instead through
///   the DELIBERATE +3,000 surcharge, which only a VOLUNTARY demolish
///   can latch. Same felt penalty, opposite provenance: MC2 taxes the
///   destruction you chose, MC1 taxes the destruction that happened
///   to you.
/// * **rung 7: 300,000,000 vs MC1's 30,000,000.** MC2 did not FIX
///   anything here — it only raised the number. In MC1 that value is
///   simultaneously the castle's enforced capacity ceiling and a
///   deliberately unaffordable price (player-certified on retail
///   2026-08-24: a cheated pool above 30M is EXPELLED, so the cap
///   really works). MC2 splits capacity from price but sets BOTH
///   rung-7 values to 300M. The degenerate part is the MISSING LEVEL
///   CAP behind the number, which both engines share: reach the rung
///   and the level-8 cast FIRES but strands the castle in its
///   transform sub-state forever — no castle, and no out-of-range
///   geometry, because `sub_11A10` restores the extents on every
///   failure path. Reaching it also overflows the downgrade haircut
///   (i32, 10 x 300M wraps negative). See docs/DEVIATIONS.md.
///
/// The per-tier multiplier (x320>>8, x384>>8) applies to this ladder
/// and NOT to MC1's — MC1 has no tier multiply at all.
pub(crate) const MC2_CASTLE_COST: [i64; 8] = [
    1000,
    10000,
    20000,
    40000,
    80000,
    160000,
    320000,
    300_000_000,
];

/// `sub_60810` (EF:61707-61728): max life by level, PRE-scale.
/// Level 0 = 0 (the ladder skips the life write — the footprint
/// keeps whatever it had). Scaled by the owner's Life personality
/// (`mc2_castle_life_factor`).
pub(crate) const MC2_CASTLE_HP: [u32; 8] = [0, 20000, 40000, 40000, 60000, 60000, 80000, 80000];

/// `byte[0] |= 0x40` (EF:61756): the "upgrade armed" latch the
/// standing tick converts into action 5 state 0. Bit 6 is unused on
/// class-3 entities in the MC1 column (the 0x40 ball-tether bit is
/// a class-10 home).
pub(crate) const F_UPGRADE_ARMED: u32 = 0x40;

/// The castle painter's KILL BIT (`struct_byte_0xc_12_15.byte[2] & 1`,
/// EF:27826): while it is set, every footprint cell the painter
/// touches is run through [`Gen::mc2_building_clear_tile`] on EVERY
/// tick of the 19-tick rise — the rising castle EXECUTES what stands
/// on it. Only the level-UP painter carries it (`sub_60480` EF:61602
/// `byte[2] |= 1`); the damage REPAINT painter (`sub_5FBD0` EF:60336)
/// does not, so a castle re-stamping itself after a hit kills nothing.
/// The MC1 column has the identical arm on `+18 & 1` (:56492).
pub(crate) const F_BUILD_KILL: u32 = 1 << 21;

/// `sub_60400` (EF:61523): (balloons, guards) by castle level —
/// byte-identical to MC1's fleet quota (sub_47400 :56264).
const fn mc2_castle_quota(lvl: i16) -> (usize, usize) {
    match lvl {
        1 | 2 => (1, 0),
        3 => (1, 4),
        4 => (2, 6),
        5 => (2, 14),
        6 => (3, 18),
        7 => (3, 34),
        _ => (0, 0),
    }
}

impl Gen {
    /// The class-3 model-2 dispatch under the MC2 column: retail
    /// runs `tick70` through the class-3 action table (EF:1206-08).
    /// Anything else on a (3,2) is a load-time husk — stand still.
    pub(crate) fn mc2_castle_tick(&mut self, i: usize, patches: crate::patches::WorldPatches) {
        match self.ent[i].tick70 {
            4 => self.mc2_castle_standing(i),
            5 => self.mc2_castle_build(i),
            6 => self.mc2_castle_destroy(i, patches),
            _ => {}
        }
    }

    /// `EndOfCastleProjectile_5F8F0` (EF:61055) — action 4, the
    /// STANDING castle tick.
    fn mc2_castle_standing(&mut self, i: usize) {
        // (A) settle/projectile animation running (f50 = retail
        // word_0x30_48): armed 30 by the flood/quake grab
        // (mc2::flood), 5 by the destroy handler. Holds at 1 while
        // the grab bit is still set — the flood releases it.
        //
        // ⚠⚠⚠ THE SHAKE COUNTDOWN **IS** AN EARLY RETURN, AND THIS
        // COMMENT USED TO SAY THE OPPOSITE. It read "the shake
        // countdown is an `else if`, NOT an early return … followed by
        // a COMMON TAIL that always runs", and re-read
        // `EndOfCastleProjectile_5F8F0` (EF:61055-61100) says
        // otherwise, plainly: the ground refresh at :61091, the
        // `playerEntityIndex = id` echo at :61093 and the ENTIRE
        // even-phase block (eject, extents, aim lift, `sub_5FF50`,
        // absorb) all sit INSIDE the `else` of `if (word_0x30_48)`.
        // The countdown arm is exactly three statements —
        // `word_0x30_48--`, `sub_5F890(a1x, 1)`, and its OWN ground
        // refresh — and the `== 1` release arm is three more with no
        // ground refresh at all.
        //
        // The old note admitted its own exposure: "No corpus pair
        // covers it today … so this lands on the decompile alone."
        // mc2l3 covers it now. t=15904 the player self-destructs the
        // castle: the destroy handler arms `word_0x30_48 = 5` and
        // `maxMana_0x8C_140` reprices 78600 → 38800, which leaves
        // `mana_0x90_144` = 38816 SIXTEEN OVER CAP for the whole
        // countdown. Retail ejects nothing on any of those ticks —
        // 38816 stands unchanged from 15904 to 15910 across two even
        // phases (200 and 202) — because `sub_5FD00` is not reached.
        // The port ran the tail and spilled a 16-mana (10,39) into
        // slot 180 at t=15906, a slot retail leaves on the free
        // stack (`extra(10,39)slot180x1`).
        //
        // ⭐ Session 36's law, on the port's own prose: the *because*
        // clause is the part nobody re-reads.
        if self.ent[i].f50 != 0 {
            if self.ent[i].f50 == 1 {
                if self.ent[i].flags & super::flood::F_QUAKE_GRAB == 0 {
                    self.ent[i].tick70 = 5;
                    self.ent[i].f59 = 3; // → the repaint-painter arm
                    self.ent[i].f50 = 0;
                }
                // ⚠ NO ground refresh on the release arm (:61067-71).
            } else {
                self.ent[i].f50 -= 1;
                // sub_5F890(a1x, 1): the CREATE-CASTLE UPGRADE-LOCK
                // PIN (EF:61076) — see docs/DEVIATIONS.md; the port
                // supplies it as a derived predicate.
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                self.ent[i].z = self.ground_z(x, y) as i16;
            }
            return;
        }
        // (B) normal standing tick — everything below is retail's
        // `else`, so the countdown suppresses ALL of it.
        match self.mc2_castle_intake(i) {
            2 => {
                self.ent[i].tick70 = 6;
            }
            _ => {
                if self.ent[i].flags & F_UPGRADE_ARMED != 0 {
                    self.ent[i].flags &= !F_UPGRADE_ARMED;
                    self.ent[i].f59 = 0;
                    self.ent[i].tick70 = 5;
                }
            }
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        // playerEntityIndex = self.id every tick (EF:61092) — the
        // census claim key.
        self.ent[i].f144 = self.ent[i].id24;
        // Heavy work on even ticks only (EF:61094).
        if self.ent[i].f63 & 1 == 0 {
            self.mc2_castle_eject(i);
            let lvl = self.ent[i].f26;
            self.mc2_castle_extents(i, lvl.clamp(0, 7) as u8);
            // EF:61098-99: the AIM LIFT + fov re-assert right after
            // the extents refresh (the yaw seat carries -8192 on a
            // standing castle every even tick).
            self.ent[i].f78 = 0xE000;
            self.ent[i].f84 = 0x4000;
            self.mc2_castle_roster(i);
            self.mc2_castle_absorb(i);
        }
    }

    /// `BeginOfCastleCreation_5FA70` (EF:61123) — action 5, the
    /// build/repaint state machine on `f59` (retail word_0x2E_46).
    fn mc2_castle_build(&mut self, i: usize) {
        match self.ent[i].f59 {
            // ── pre-clear + level-up ──
            0 => {
                self.mc2_castle_preclear(i);
                if self.ent[i].f26 == 0 || self.mc2_castle_space_ok(i) {
                    // Owner palette shift (EF:61137-41): renderer
                    // team tint (deliberate).
                    self.mc2_castle_upgrade(i);
                } else {
                    self.ent[i].f59 = 2;
                    self.ent[i].flags &= !F_UPGRADE_ARMED;
                    // sub_88D00: "no room" hint toast (UI only).
                }
            }
            // ── ground settle waits ──
            1 | 6 => {
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                self.ent[i].z = self.ground_z(x, y) as i16;
            }
            // ── abort/pass-done → steady ──
            2 => {
                self.ent[i].tick70 = 4;
                // ⭐⭐ `sub_5F890(a1x, 0)` IS NOT A NO-OP ON THE CASTLE.
                // Its `a2 == 0` arm releases the owner's castle-spell
                // manifestation AND then calls `sub_6D880(a1x)`
                // (EF:61047) — and `a1x` there is THE CASTLE, not the
                // manifestation the same function just wrote
                // `word_0x2E_46 = 0` on. `sub_6D880` (EF:58215-24) is
                // `if (@0x2C) { SetSpell_6D5E0(a1x, @0x2C - 1); @0x2C
                // = 0; }`.
                //
                // ⚠⚠ THE SAME WORD MEANS TWO THINGS BY CLASS. On a
                // class-15 manifestation @0x2C is a DEFERRED SPELL
                // TIER (port home `f44`, see `mc2_set_spell_at`); on a
                // (3,2) castle it is the GUARD-RESPAWN COOLDOWN (port
                // home `f46`, read/written only by
                // `mc2_castle_roster`). So a spell-selection helper
                // clears the guard cooldown as a side effect, and
                // EVERY build or upgrade that settles back to standing
                // re-arms the guard spawn instead of resuming the
                // 16-pass ladder it froze at.
                //
                // `SetSpell_6D5E0` cannot touch anything else here:
                // case 2 is only entered with `word_0x2E_46 == 2`, so
                // it takes its `if (entity->word_0x2E_46)` branch
                // (Level.cpp:1510-12) and merely re-writes @0x2C,
                // which `sub_6D880` clears on the next line. Net
                // effect on the record is exactly `@0x2C := 0` — and
                // identical when it was already 0, so the write is
                // unconditional here.
                //
                // ⚠ NOT owner-scoped. `SpellsEnabled_0x333[2]` is
                // materialized by `sub_5CF40` (EF:59374) from the
                // wizard spawn `sub_5C950`, which runs for EVERY
                // player — including AI, at EF:60276 under an explicit
                // `IsAiPlayer` guard. A RIVAL castle clears its
                // cooldown on settle too.
                //
                // Measured, galore castle 266 (retail's own bytes):
                // `f2c` holds 16→13 across the level-3 standing pass,
                // FREEZES at 13 through all 19 transform ticks (that
                // half was already faithful — `BeginOfCastleCreation`
                // never calls `sub_5FF50`), then drops 13 → 0 in the
                // SINGLE boundary that completes the upgrade
                // (t=1128→1129, `f2e` 2 → 0, action 5 → 4), and the
                // next even standing pass mints guard #2 and latches
                // 16. Four for four across the 3→4, 4→5, 5→6 and 6→7
                // rungs (t=1129, 1156, 1183, 1210).
                //
                // Keep this BEFORE `f59 = 0`: retail's order is
                // `actionIndex = 4; sub_5F890(locEvent, 0);
                // word_0x2E_46 = 0;` (EF:61149-52), and that order is
                // what keeps `SetSpell_6D5E0` off its ELSE branch.
                self.ent[i].f46 = 0;
                self.ent[i].f59 = 0;
            }
            // ── spawn a repaint painter ──
            3 => {
                self.mc2_spawn_castle_painter(i, true);
            }
            // ── wait for the painter ──
            4 => {
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                self.ent[i].z = self.ground_z(x, y) as i16;
                if self.ent[i].f63 & 0x1F == 0 {
                    // Any (10,42) still alive? (EF:61149-61158 —
                    // the painter signals f59=2 itself when it
                    // finishes; this poll only catches a painter
                    // that died without finalizing.)
                    let alive = (1..self.ent.len()).any(|j| {
                        self.ent[j].class64 == 10
                            && self.ent[j].model65 == 42
                            && self.ent[j].flags & 0x400 == 0
                    });
                    if !alive {
                        self.ent[i].f59 = 3;
                    }
                }
            }
            // ── the (10,41) leveler arm (EF:61162-67): dead code
            // at runtime — nothing in MC2 writes state 5 ──
            _ => {}
        }
    }

    /// `sub_5FCA0_destroy_castle_level` (EF:61222) — action 6:
    /// gated on free pool slots (retail sub_4A810: "spheres can
    /// spawn"), one level off + ejector + roster, then a 5-tick
    /// settle into the repaint. No slots → retry from action 4.
    /// The ejector runs UNCONDITIONALLY (EF:61228) — a level-0
    /// death inside the downgrade still spills the whole bank as
    /// owned (10,39) spheres (the eject's f26==0 arm); roster at
    /// level 0 is NOT a no-op — its `v1 < 3` tail (EF:61436-45) is
    /// where the FLEET dissolves — and the state writes are inert on
    /// a dead entity, matching retail's straight-line body.
    /// ⭐⭐ THE ORDER IS LOAD-BEARING: `sub_5FD00` mints the bank
    /// spheres and `sub_5FF50` mints the balloon spheres, so a
    /// balloon's sphere is the LAST pop of the burst. mc2l3 t=585
    /// (castle 252 dies 1 → 0 holding 6300 = 6 × 1050, fleet cargo
    /// 1200): retail pops 228,248,129,215,135,126 for the bank and
    /// 127 for the balloon. Front-loading the conversion into
    /// `mc2_castle_downgrade` rotated all seven by one.
    fn mc2_castle_destroy(&mut self, i: usize, patches: crate::patches::WorldPatches) {
        if !self.free.is_empty() {
            self.mc2_castle_downgrade(i, patches);
            self.ent[i].tick70 = 4;
            self.mc2_castle_eject(i);
            self.mc2_castle_roster(i);
            self.ent[i].f59 = 0;
            self.ent[i].f50 = 5;
        } else {
            self.ent[i].tick70 = 4;
        }
    }

    /// `sub_609E0` (EF:61733) — the damage intake: STRAIGHT subtract
    /// (no /10, no shield), single mail channel; the self-keyed
    /// upgrade-request channel arms bit6. Returns 0 idle / 1 hit /
    /// 2 lethal (already dead counts).
    fn mc2_castle_intake(&mut self, i: usize) -> u8 {
        if self.ent[i].act_life < 0 {
            return 2;
        }
        let mut result = 0;
        if self.ent[i].mail[0].1 != 0 {
            let (amt, src) = self.ent[i].mail[0];
            self.ent[i].act_life -= amt as i32;
            if self.ent[i].act_life < 0 {
                self.ent[i].f36 = src; // killer memory (word_0x24_36)
                self.ent[i].mail[0].1 = 0;
                return 2;
            }
            self.ent[i].mail[0] = (0, 0);
            result = 1;
            // Owner "castle under attack" HUD flag (byte_0x195_405
            // = 4): retail latches it for ANY owner (EF:61752); ours
            // is a single player-side HUD latch (deliberate; per-owner
            // records await a rival defense-AI consumer).
            if self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
                self.castle_alert = 4;
            }
        }
        // word_0x80_128 == own id (EF:61753): the UPGRADE request —
        // the delivered (10,43) token writes our mail[5] = (10,
        // owner), the same protocol both columns share (sub_389F0
        // EF:28240 writes word_0x7C_124 = 10 + word_0x80_128 = id).
        // Retail clears the channel only INSIDE the id match
        // (EF:61754-58) — a non-matching value sticks forever
        // (faithful quirk; never authored in practice).
        // ⚠ THE AMOUNT IS NOT CLEARED. `sub_609E0` (EF:61753-58)
        // writes `str_0x5E_94.word_0x80_128 = 0` and nothing else —
        // channel 5's AMOUNT word `dword_0x7C_124` (mail stride 6: ch0
        // = @0x5E/@0x62, ch5 = @0x7C/@0x80) is never read and never
        // cleared, so the (10,43) token's `10` stands as PERMANENT
        // residue on the castle. Do NOT confuse it with
        // `dword_0x5E_94`, which is CH0's amount and which this same
        // function does read (EF:61740) and clear (EF:61748). This is
        // the law the certified MC1 arm already carries
        // (features.rs:5042-51). Receipts: mc2l0 free run t=7610 slot
        // 235 retail mail5.amt 10 / port 0; galore t=1054/1079/1107;
        // mc2l4 t=238..272 across 2 slots.
        if self.ent[i].mail[5].1 == self.ent[i].id24 && self.ent[i].mail[5].1 != 0 {
            self.ent[i].mail[5].1 = 0;
            if self.ent[i].f26 < 7 {
                self.ent[i].flags |= F_UPGRADE_ARMED;
            }
        }
        result
    }

    /// `sub_60480` (EF:61563) — the LEVEL-UP: painter spawn, sound
    /// 10, level++, back to wait-for-painter, extents, ladder,
    /// stage-piece rebuild, +1 castle XP (`sub_6D8B0(owner,2,1)`
    /// EF:61596 — the ladder that makes Fire/Lightning Tower tiers
    /// selectable; the XP drain's spell-2 branch also re-syncs the
    /// manifestation tier).
    fn mc2_castle_upgrade(&mut self, i: usize) {
        let lvl = (self.ent[i].f26 + 1).clamp(1, 7);
        // The painter first — retail aborts the whole level-up if
        // the pool is full (EF:61568).
        let Some(p) = self.mc2_spawn_castle_painter_at(i, lvl as u8, false) else {
            return;
        };
        // The level-up painter is ARMED (EF:61602 `byte[2] |= 1`):
        // the rise executes whatever stands on the footprint. The
        // repaint painter is not — see [`F_BUILD_KILL`].
        self.ent[p].flags |= F_BUILD_KILL;
        self.snd(10, i);
        self.ent[i].f26 = lvl;
        self.ent[i].tick70 = 5;
        self.ent[i].f59 = 4; // wait-for-painter
        self.mc2_castle_extents(i, lvl as u8);
        // sub_60480's own follow-up (EF:61590-91): the AIM LIFT into
        // the yaw seat of the extents block + the fov re-assert —
        // the writes the extents helper's ⛔ note defers to (mc2l3
        // t=244: applied_yaw -8192 from the birth's same-frame
        // upgrade).
        self.ent[i].f78 = 0xE000;
        self.ent[i].f84 = 0x4000;
        self.mc2_castle_extents_ent(p, lvl as u8);
        self.mc2_castle_ladder(i);
        self.mc2_castle_stages(i);
        let own = self.ent[i].id24;
        if own == crate::mc1::mobs::PLAYER_TARGET {
            self.mc2_cast_xp.0.push((own, 2, 1));
        }
    }

    /// `sub_605E0` (EF:61612) — ONE LEVEL DOWN: 10% capacity mana
    /// haircut (scattered), terrain restore for the removed level,
    /// ladder + stage rebuild; at level 0 the castle dies (owner
    /// unbind = the id24 link simply despawns with the entity).
    fn mc2_castle_downgrade(&mut self, i: usize, patches: crate::patches::WorldPatches) {
        if self.ent[i].f26 > 0 {
            // 10% capacity haircut. Patched arm (`mc2_downgrade_
            // overflow`): computed in i64 — a castle over-filled past
            // the normal cap ladder can carry an f136 large enough
            // that `10 * f136` overflows i32. Retail's i32
            // `10 * x / 100` overflows at the always-overflowing
            // level-7 rung (10 × 300M) into a NEGATIVE cut — a maxed
            // level-7 castle downgrade *raises* its cap and scatters
            // nothing. The retail arm reproduces the wrap exactly.
            let cut = if patches.mc2_downgrade_overflow {
                (10i64 * self.ent[i].f136 as i64 / 100) as i32
            } else {
                10i32.wrapping_mul(self.ent[i].f136 as i32) / 100
            };
            self.ent[i].f136 -= cut;
            self.mc2_castle_eject(i);
            self.ent[i].f136 += cut;
            self.snd(30, i);
            // The scratch entity carries the row (EF:61628-31); its
            // model-0 branch only sets the mana-sphere drop z
            // (EF:28092-28108) — it does NOT restore heights, and
            // nothing else does either. Level 0 → no re-scatter.
            let lvl = self.ent[i].f26;
            self.mc2_castle_unstamp(i, lvl.clamp(1, 7) as u8);
            self.ent[i].f26 = lvl - 1;
            self.mc2_castle_extents(i, (lvl - 1).clamp(0, 7) as u8);
            self.mc2_castle_ladder(i);
            self.mc2_castle_stages(i);
        }
        if self.ent[i].f26 <= 0 {
            // Castle death (`sub_605E0`'s level-0 arm,
            // EF:61645-61666): free the pieces, then despawn.
            // ⛔ NO FLEET CONVERSION HERE. That arm re-syncs the
            // owner's castle-spell manifestation (`sub_5F890`), clears
            // `CastleEntityIndex_0x3A_58` and calls
            // `DisableEntityDrawing04_57F10` — it never touches a
            // balloon. The fleet dissolves in `sub_5FF50`'s `v1 < 3`
            // TAIL (EF:61436-45), which `sub_5FCA0` runs AFTER
            // `sub_5FD00`, so retail mints the bank spheres FIRST and
            // the balloon sphere LAST. `mc2_castle_roster` runs that
            // identical dissolve at quota 0 (`mc2_castle_quota(0) ==
            // (0, 0)` ⇒ every collected balloon is over-quota).
            //
            // Front-loading it here rotated the whole burst by one
            // free-stack pop: mc2l3 t=585, pop order
            // 228,248,129,215,135,126,127 — the balloon's sphere
            // landed on 228 (retail's FIRST bank sphere) and all six
            // bank scatters slid one slot down the order, port[p_k] ==
            // retail[p_{k-1}] exactly.
            //
            // ⭐ MC1 ALREADY CARRIES THIS LAW: the identical MC1
            // deviation was retired 2026-08-12 (DEVIATIONS.md:121) and
            // `features.rs::castle_downgrade` leaves the fleet to its
            // wrapper tail. MC2 was the laggard column.
            self.mc2_castle_free_stages(i);
            self.ent[i].flags |= 0x400;
        }
    }

    /// `sub_60810` + `sub_60780` (EF:61695/61670) — the HP/CAP
    /// ladder. HP = base[lvl] * factor >> 8 where factor =
    /// (Life * ((research[lvl] << 8) + 256)) >> 8. CONFIRMED
    /// sources (mc2-castle-data-tables.md §2): Life = 256 default
    /// (the human ALWAYS — EF:43720; an AI wizard's comes from the
    /// map header's `WizardMapSettings.Life_0x3612F` via the rival
    /// spawn, EF:43768 — resolved per owner color below);
    /// research[lvl] = `array_0x24E_590[lvl]`, the HP-FACTOR channel
    /// of the castle research table — SPELLS row 2's
    /// `subSpellIndex_2` for the tier the castle spell was CAST at,
    /// `{0, 0, 1}` for tiers I/II/III. Level 0 skips the life write.
    /// A negative (overkill) life carries as debt capped at half the
    /// new max.
    ///
    /// ⭐⭐ A TIER-III CASTLE HAS DOUBLE HP AT EVERY RUNG. research 1
    /// makes the factor `(256 * ((1<<8) + 256)) >> 8 = 512` = 2.0×,
    /// so level 1 lands on 40000 instead of 20000 — and 40000 is also
    /// the ctor's flat birth value (`spawn_castle`, mc1/combat.rs), so
    /// on a tier-III cast the birth value merely SURVIVES the first
    /// dispatch instead of being halved. The two 40000s are equal by
    /// coincidence, from unrelated sources; do not collapse them.
    /// ⚠ The `.1` HP-FACTOR channel was already being STAMPED by
    /// `mc2_research_stamp` and had NO reader anywhere — a half-landed
    /// law. Measured mc2l0-spells-galore t=1029→1030 (ball mana
    /// 49 = 5000/101 = row-2 tier-III cost) against mc2l0 t=7225 /
    /// mc2l1 t=2070 / mc2l30 t=235 / mc2l3 t=245, all tier-I casts
    /// (ball mana 9 = 1000/101), all 20000.
    pub(crate) fn mc2_castle_ladder(&mut self, i: usize) {
        let lvl = self.ent[i].f26.clamp(0, 7) as usize;
        // The owner's Life scalar, then the research rung on top of
        // it — retail's `number1` (EF:61704) in one expression:
        // `(Life * ((research[lvl] << 8) + 256)) >> 8`. Life is 256
        // (1.0× in 16.8) for the human ALWAYS (EF:43720); a rival's is
        // the map header's authored `Life_0x3612F` (EF:43768).
        let own = self.ent[i].id24;
        let slot = self
            .rival_ents
            .iter()
            .position(|&e| e != 0 && e == own)
            .unwrap_or(0);
        let life = self.mc2_life_scale.0[slot] as i64;
        let research = self.mc2_castle_hp_factor(own, lvl as u8) as i64;
        // Two-stage, with the intermediate truncation retail has: the
        // `>> 8` lands BEFORE the base multiply (EF:61710
        // `20000 * number1 >> 8`).
        let factor = (life * ((research << 8) + 256)) >> 8;
        let hp = ((MC2_CASTLE_HP[lvl] as i64 * factor) >> 8) as u32;
        if hp != 0 {
            let debt = if self.ent[i].act_life < 0 {
                (-self.ent[i].act_life).min(hp as i32 / 2)
            } else {
                0
            };
            self.ent[i].max_life = hp;
            self.ent[i].act_life = hp as i32 - debt;
        }
        self.ent[i].f136 = MC2_CASTLE_CAP[lvl];
        // `sub_60780`'s second half: every HP/CAP stamp re-syncs the
        // owner's book token, gate-suppressed (see [`Mc2LadderMail`])
        // — the book is World-side, drained same tick.
        self.mc2_ladder_sync.0.push(i as u16);
    }

    /// The MC2 castle build datum (ctor `sub_4AA40` EF:33399):
    /// `32 * sub_48E60(tlx, tly, w, h)` — the PERIMETER MINIMUM
    /// ground over the BUILD00 row-1 footprint centered on the
    /// (even-parity-snapped) anchor tile. Feeds `site_z` — the
    /// painter/leveler datum — for EVERY (3,2) spawn path: the human
    /// cast, the rival direct build, the authored starting castle.
    ///
    /// MIN, not mean, and the distinction is load-bearing: the stamp
    /// writes `datum + cell` ABSOLUTELY while the demolish only
    /// SUBTRACTS `cell` back off, and nothing anywhere saves the
    /// original ground. Taking the perimeter minimum is what makes
    /// that asymmetry harmless — the leftover pad lands at or below
    /// the lowest surrounding ground, so it reads as flush or sunken.
    /// Our old corner-MEAN datum sat above the low side of any
    /// sloped site, and the un-stamp left exactly `mean - ground` of
    /// stone-textured mesa standing where the castle had been: the
    /// flagless "tower". Flat sites (mean == min) never showed it,
    /// which is precisely the site-dependence the player reported.
    pub(crate) fn mc2_castle_site_z(&self, cx: u8, cy: u8) -> i16 {
        let def = self.assets.build_tab[1 % self.assets.build_tab.len()];
        let tlx = cx.wrapping_sub(def.w / 2);
        let tly = cy.wrapping_sub(def.h / 2);
        (32 * self.mc2_perimeter_min(tlx, tly, def.w as u16, def.h as u16)) as i16
    }

    /// `sub_11CB0` (EF:4557) — the castle-CAST site test the (9,10)
    /// ball runs at launch and as its per-tick tripwire: REFUSED when
    /// (a) any castle (class-3 model-2) sits within its x/y
    /// extents + 2560 of the point, (b) any building (the dword_38527
    /// chain = the live (10,45) roster) within the same margin, or
    /// (c) any cell of retail's 8×8 window anchored at (tile − 8,
    /// tile − 8) (the left/up-biased quirk, kept verbatim) is built
    /// (angle bit7) or, on caves, sealed (bit3 — `sub_11C80`).
    pub(crate) fn mc2_castle_cast_site_ok(&self, x: u16, y: u16) -> bool {
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if e.flags & 0x400 != 0 || e.act_life < 0 {
                continue;
            }
            let castle = e.class64 == 3 && e.model65 == 2;
            let building = e.class64 == 10 && e.model65 == 45;
            if (castle || building)
                && wd(e.x, x) <= e.f80 as i32 + 2560
                && wd(e.y, y) <= e.f82 as i32 + 2560
            {
                return false;
            }
        }
        let tx = (x >> 8) as u8;
        let ty = (y >> 8) as u8;
        for ry in 0..8u8 {
            for rx in 0..8u8 {
                let gx = tx.wrapping_sub(8).wrapping_add(rx);
                let gy = ty.wrapping_sub(8).wrapping_add(ry);
                let a = self.t.angle[tile(gx, gy)];
                if a & 0x80 != 0 || (self.is_cave() && a & 8 != 0) {
                    return false;
                }
            }
        }
        true
    }

    /// `SetShiftByCastle_49EC0` (EF:32882): AABB half-extents from
    /// the BUILD00 row for the level — `((dim<<8)+1280)>>1`. The
    /// tick's follow-up yaw/fov writes land as the sprite fov home.
    pub(crate) fn mc2_castle_extents(&mut self, i: usize, lvl: u8) {
        self.mc2_castle_extents_ent(i, lvl);
    }

    fn mc2_castle_extents_ent(&mut self, i: usize, row: u8) {
        let Some(def) = self.assets.build_tab.get(row as usize).copied() else {
            return;
        };
        let e = &mut self.ent[i];
        // ⛔ DO NOT add `e.f78 = 0` here. The retail helper really does
        // write `array_0x52_82.yaw = 0` (EF:32891), and it was banked
        // as a latent port divergence — but the CORPUS REFUTES IT:
        // adding the write costs 469 mc2l0 fixtures on the compared
        // `field:3,2:applied_yaw` column, i.e. retail castles carry a
        // NONZERO yaw across the ticks the port refreshes extents on.
        // The port's callers are not retail's callers (retail reaches
        // SetShiftByCastle only at the level-up seams and around the
        // pre-clear's temporary next-level box, EF:4399/4415), so the
        // follow-up yaw/fov writes named below are what actually own
        // this lane. Recorded gameplay outranks the decompile.
        e.f80 = (((def.w as u16) << 8).wrapping_add(1280)) >> 1;
        e.f82 = (((def.h as u16) << 8).wrapping_add(1280)) >> 1;
        e.f84 = 0x4000;
    }

    /// `sub_11960` (EF:4391) — the pre-clear: kill every EFFECT
    /// entity whose AABB overlaps the NEXT level's footprint
    /// (life = -1). Effects only — objects/terrain untouched. The
    /// retail effect list (`dword_38527`) is class-10 models
    /// 0x2D..=0x2D (buildings) — the flood's erase pass shares it;
    /// here the practical membership is the class-10 model-45
    /// building band (the MC1 column's pre-clear kills the same
    /// kind via its own list).
    fn mc2_castle_preclear(&mut self, i: usize) {
        let next = (self.ent[i].f26 + 1).clamp(1, 7) as usize;
        let Some(def) = self.assets.build_tab.get(next).copied() else {
            return;
        };
        let half_w = ((((def.w as u16) << 8).wrapping_add(1280)) >> 1) as i32;
        let half_h = ((((def.h as u16) << 8).wrapping_add(1280)) >> 1) as i32;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if j != i
                && e.class64 == 10
                && e.model65 == 45
                && e.flags & 0x400 == 0
                // Inclusive compare — sub_11960's `<=` (the flood's
                // strict `<` is the other helper).
                && wd(e.x, x) <= e.f80 as i32 + half_w
                && wd(e.y, y) <= e.f82 as i32 + half_h
            {
                self.ent[j].act_life = -1;
                self.ent[j].f46 = 0; // fontTypeIndex = 0
            }
        }
    }

    /// `sub_11A10` (EF:4421) — the space check: (a) any OTHER CASTLE
    /// (dword_38519 = the class-3 list, model-2 filter — EF:4449-51)
    /// overlapping the next-level box → no room (z-term omitted:
    /// castle fov is pinned 0x4000 both sides, so retail's
    /// |Δz+Δyaw| < 0x8000 is tautological);
    /// (b) walk retail's QUIRKY partial ring of border cells between
    /// the current and next footprints — a cell with `mapAngle` bit7
    /// (built/blocked), or on caves bit3 (SEALED), fails
    /// (`sub_11C80` EF:4543).
    pub(crate) fn mc2_castle_space_ok(&self, i: usize) -> bool {
        let cur = self.ent[i].f26.clamp(0, 7) as usize;
        let next = (self.ent[i].f26 + 1).clamp(1, 7) as usize;
        let (Some(dc), Some(dn)) = (
            self.assets.build_tab.get(cur).copied(),
            self.assets.build_tab.get(next).copied(),
        ) else {
            return true;
        };
        let half_w = ((((dn.w as u16) << 8).wrapping_add(1280)) >> 1) as i32;
        let half_h = ((((dn.h as u16) << 8).wrapping_add(1280)) >> 1) as i32;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if j != i
                && e.class64 == 3
                && e.model65 == 2
                && e.act_life >= 0
                && e.flags & 0x400 == 0
                && wd(e.x, x) < e.f80 as i32 + half_w
                && wd(e.y, y) < e.f82 as i32 + half_h
            {
                return false;
            }
        }
        // The ring scan: outer minus inner half-extents in tiles.
        let (iw, ih) = (
            ((((dc.w as u16) << 8).wrapping_add(1280)) >> 1) >> 8,
            ((((dc.h as u16) << 8).wrapping_add(1280)) >> 1) >> 8,
        );
        let (ow, oh) = ((half_w >> 8) as u16, (half_h >> 8) as u16);
        let ox = (x.wrapping_add(128) >> 8).wrapping_sub(ow) as u8;
        let oy = (y.wrapping_add(128) >> 8).wrapping_sub(oh) as u8;
        let (mx, my) = (ow.saturating_sub(iw) as u8, oh.saturating_sub(ih) as u8);
        let blocked = |gx: u8, gy: u8| {
            let a = self.t.angle[tile(gx, gy)];
            a & 0x80 != 0 || (self.is_cave() && a & 8 != 0)
        };
        // Retail's QUIRKY partial band walk, kept verbatim
        // (EF:4464-4535; faithful quirk): EVERY band —
        // the two side slivers included — iterates `my` rows, so
        // the side rows below oy+my and the whole EAST border
        // column are never tested (my==0 ⇒ no ring cells at all).
        // Band 4's first row starts at ox+ow−mx (near the CENTER,
        // inside the inner footprint) and its x-cursor then resets
        // to ox, duplicating band 3 one row down.
        for row in 0..my {
            for col in 0..2 * ow as u8 {
                if blocked(ox.wrapping_add(col), oy.wrapping_add(row))
                    || blocked(
                        ox.wrapping_add(col),
                        oy.wrapping_add((2 * oh) as u8)
                            .wrapping_sub(my)
                            .wrapping_add(row),
                    )
                {
                    return false;
                }
            }
        }
        for row in 0..my {
            for col in 0..mx {
                // Band 3 (left sliver at oy+my).
                if blocked(ox.wrapping_add(col), oy.wrapping_add(my).wrapping_add(row)) {
                    return false;
                }
                // Band 4 (the center-collapse walk).
                let sx = if row == 0 {
                    ox.wrapping_add(ow as u8).wrapping_sub(mx)
                } else {
                    ox
                };
                if blocked(sx.wrapping_add(col), oy.wrapping_add(my).wrapping_add(row)) {
                    return false;
                }
            }
        }
        true
    }

    /// `sub_5FD00` (EF:61240) — the overflow EJECTOR: spill = stored
    /// − capacity when (owner bank + stored) exceeds capacity (the
    /// "13C law" — the trigger reads the bank, the amount doesn't);
    /// a level-0 castle spills EVERYTHING. 1..=32 owner-tagged
    /// (10,39) spheres of spill/count each, teleported out at
    /// random yaws (dist rand%0x1400 + 3840, speed rand%0x30 + 16,
    /// the upward pop from the flag height).
    fn mc2_castle_eject(&mut self, i: usize) {
        let stored = self.ent[i].f140;
        let cap = self.ent[i].f136;
        let own = self.ent[i].id24;
        let bank = self.mc2_owner_bank(own);
        let mut spill = if bank.saturating_add(stored) > cap {
            stored - cap
        } else {
            0
        };
        if self.ent[i].f26 == 0 {
            spill = stored;
        }
        if spill <= 0 {
            return;
        }
        // Retail caps the count by the free-pool HEADROOM and splits
        // the FULL spill across the clamped count (EF:61272-96) —
        // fewer-but-bigger spheres on a short pool, never an
        // under-eject. (Retail's zero-headroom arm spawns nothing
        // after a failed GC pass; our free list is exact.)
        let headroom = self.free.len() as i32;
        if headroom == 0 {
            return;
        }
        let count = (spill / 1000).clamp(1, 32).min(headroom);
        let mut share = spill / count;
        let (cx, cy, cz) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let ground = self.ground_z(cx, cy) as i16;
        for _ in 0..count {
            let Some(b) = self.spawn_mana_ball(cx, cy, cz) else {
                break;
            };
            self.ent[b].f140 = share;
            self.ent[b].f144 = own;
            let d = self.ent_rand(b);
            self.ent[b].f126 = (d % 0x30 + 16) as i16;
            self.ent[b].dest_x = 0;
            self.ent[b].dest_y = 0;
            // word_0x2C_44 vertical arc (EF:61286) — our ball pop
            // home is f46 (the MC1 column's shared machinery).
            self.ent[b].f46 = ((1024 - (cz.wrapping_sub(ground)) as i32) / 8) as i16;
            // The castle's rand_0x14_20 is a u16 (EF:61312-16) — the
            // chassis u16 draw, not the raw 32-bit LCG.
            let dist = (self.ent_rand(i) % 0x1400 + 3840) as i16;
            let yaw = (self.ent_rand(i) & 0x7FF) as u16;
            let mut pos = (cx, cy, cz);
            Self::polar_step(&mut pos, yaw, 0, dist);
            self.move_relink(b, pos.0, pos.1, pos.2);
            // ⛔ NO RE-RESIZE HERE — this call was INVENTED in the MC2
            // copy. `sub_5FD00` (EF:61296-61320) writes mana@0x90,
            // playerEntityIndex@0x94 and the scatter position, then
            // STOPS; the only sprite/rotation stamp on an ejected
            // sphere is the ctor's own (`CreateManaSphere512_50080`
            // EF:36595), which runs while mana is still 512 and
            // `NewEvent_4A050`'s memset has zeroed the owner — size
            // class 1, family 52 ⇒ sprite 53, quad 28. The certified
            // MC1 twin `Gen::castle_eject` (features.rs:5219) and the
            // MC2 sibling `mc2_mana_spheres` (mc2/mobs.rs:936) both
            // omit it. A sphere only grows to 105+7 ⇒ quad 112 when
            // its OWN tick re-derives, which this frame reaches only
            // for slots ABOVE the castle's: galore t=1053, the castle
            // at slot 266 ejects 32 (the `clamp(1,32)` ceiling) — 21
            // at slots ≤265 end at quad 28, 11 at ≥274 at 112 (305
            // and 314 were recycled records already holding 112, and
            // retail's `!=` guard skips them entirely — the cleanest
            // in-corpus proof of that guard). ⚠ NOT cosmetic:
            // `ball_resize` also writes f80/f82, the collision
            // half-extents `ent_overlap` reads, so re-adding it both
            // inflates the merge box and MASKS `ball_tick`'s own
            // resize through its `type86 != ty` guard.
            let taken = self.ent[b].f140;
            spill -= taken;
            self.ent[i].f140 -= taken;
            if spill < share {
                share = spill;
            }
            if spill <= 0 {
                break;
            }
        }
    }

    /// The owner's possessed-building bank — retail's per-tick
    /// census credit `dword_0x13C_316` (`sub_60F00` EF:62028): the
    /// summed mana of owned class-10 model-45 buildings.
    pub(crate) fn mc2_owner_bank(&self, own: u16) -> i32 {
        let mut bank = 0i64;
        for e in &self.ent[1..] {
            if e.class64 == 10 && e.model65 == 45 && e.flags & 0x400 == 0 && e.f144 == own {
                bank += e.f140.max(0) as i64;
            }
        }
        bank.min(i32::MAX as i64) as i32
    }

    /// The standing tick's sphere absorption (EF:61101-61116): ONE
    /// owned (10,39) sphere overlapping the castle per (even) tick,
    /// iff below capacity — the whole sphere lands.
    ///
    /// ⭐⭐ AND THE GATE IS EXACTLY THREE TESTS — `model == 39`,
    /// `playerEntityIndex == id` and `CompareAxisWithShift_10750`
    /// (EF:61108-10). **THERE IS NO REAP TEST.** A sphere a balloon
    /// swallowed EARLIER in the same tick still carries its class,
    /// model, owner and position, so the castle — whose standing tick
    /// runs `sub_5FF50`'s fleet pass FIRST — re-absorbs the very
    /// sphere its own balloon just disabled, and the mana is counted
    /// TWICE. Soft kill is not a free (the same law the ball chain and
    /// the collector tether already carry).
    ///
    /// This was a REGISTERED open item for eight days — "retail would
    /// RE-ABSORB a ball flagged earlier in the tick, double-counting
    /// its mana; no corpus row demands it yet and it is the riskier of
    /// the two, so it was deliberately left out." galore t=1304 is the
    /// row: the human's castle 266 takes retail's full 2601 from
    /// sphere 367 — the sphere balloon 237 ate that same tick, cargo
    /// 0 → 2601 on BOTH sides — where the port's `0x400` gate skipped
    /// it and left the castle 1522 short. Worth +2,587 ticks.
    ///
    /// ⭐ The MC1 twin `Gen::castle_absorb` (features.rs) had already
    /// reached the same predicate ("no class test and no `0x400`
    /// test") — MC2 was the laggard column AGAIN. It also walks the
    /// TICK-TOP BALL CHAIN rather than the pool, which is what decides
    /// WHICH ball a castle drinks when two overlap (mc1l5 t=2499).
    /// ⚠ BANKED: retail's MC2 walk is the `dword_38523` linked list
    /// (EF:61104-15), not this ascending pool scan — same shape, and
    /// no corpus row separates them yet.
    fn mc2_castle_absorb(&mut self, i: usize) {
        if self.ent[i].f140 >= self.ent[i].f136 {
            return;
        }
        let own = self.ent[i].id24;
        for j in 1..self.ent.len() {
            if self.ent[j].class64 == 10
                && self.ent[j].model65 == 39
                && self.ent[j].f144 == own
                && self.mc2_overlap_xy(i, j)
            {
                self.ent[i].f140 += self.ent[j].f140;
                self.ent[j].flags |= 0x400;
                return; // one per tick (retail breaks after the first)
            }
        }
    }

    // ---- the court: balloons + guards (sub_5FF50, EF:61342) -----------------

    /// `sub_5FF50` (EF:61342): the balloon fleet + guard slots.
    /// Slot arrays scan-collected (deliberate); dead members
    /// dissolve into mana spheres carrying their cargo
    /// (`TransformEntityToManaSphere`), over-quota members too (a
    /// downgraded castle sheds fleet). Guard respawn: one per pass,
    /// 16-PASS (= 32-tick) cooldown in `f46` — retail
    /// `word_0x2C_44` @0x2C, NOT `f44`, which on a (3,2) carries
    /// `subSpellIndex_0x2A` and which `Gen::new_event` seeds to 100 on
    /// every record. Level 3 is the first rung of `mc2_castle_quota`
    /// with a nonzero guard count — the first BUILD00 row that is a
    /// crenellated curtain WALL with an access ramp instead of a tower
    /// pad — so this block is the whole reason a take's FIRST (5,15)
    /// exists. Placed in the
    /// courtyard at (x+128, y+640) facing 512.
    pub(crate) fn mc2_castle_roster(&mut self, i: usize) {
        let own = self.ent[i].id24;
        let lvl = self.ent[i].f26;
        let (bq, gq) = mc2_castle_quota(lvl);
        // ⭐⭐ THE FLEET HALF WALKS A REGISTER, NOT A CENSUS. The MC1
        // twin ([`Gen::castle_balloons`]) has carried this since the
        // mc1l42 t=17150 row; the MC2 arm kept the ascending-slot
        // stand-in because `sub_60400`'s caller had not been read.
        // It has now: `sub_5FF50` (EF:61377-61431) drives EVERY step
        // from the owner's `array_0x3C_60[v1]`, v1 = 0..quota — an
        // empty slot spawns and `goto LABEL_17`s past the retarget, a
        // dead one converts, CLEARS the slot and likewise walks on
        // (so its replacement is a pass late), and the two exclusions
        // handed to `sub_5F810` are indices `(v1+1)%3` and `(v1+2)%3`
        // (EF:61417-19). INDEX IS SPAWN ORDER, NEVER SLOT ORDER.
        // galore t=1212 is the law in one tick: register
        // [296, 384, 237], so 296 picks first and takes sphere 341,
        // 384 takes 318 and 237 is left 312 — the ascending walk got
        // the same SET and handed it out rotated by one
        // (`port[k] == retail[k-1]`, all three).
        let mut reg = [0u16; 3];
        if !crate::engine::features::no_balloon_reg()
            && let Some(v) = self.mc1_balloon_reg.0.get(&own)
        {
            for (k, s) in v.iter().take(3).enumerate() {
                reg[k] = *s;
            }
        }
        // A register entry can only go stale through a NON-retail
        // path (a forged test entity, a pool import, a balloon freed
        // outside the dispatcher): retail's own writers clear the slot
        // as they free it. Clear those, then ADOPT any live owned
        // balloon the register does not name into the first empty
        // index in slot order — the fill order retail's own spawns
        // produce, and the recovery that keeps an orphaned fleet
        // steerable. With the register empty (the `MGC_NO_BALLOON_REG`
        // A/B arm) the adoption pass alone reproduces the old
        // ascending-slot walk exactly.
        for k in 0..3 {
            let s = reg[k] as usize;
            if s == 0 {
                continue;
            }
            let e = &self.ent[s];
            if e.class64 != 3 || e.model65 != 3 || e.id24 != own || e.flags & 0x400 != 0 {
                reg[k] = 0;
            }
        }
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if e.flags & 0x400 != 0
                || (e.class64, e.model65) != (3, 3)
                || e.id24 != own
                || reg.contains(&(j as u16))
            {
                continue;
            }
            let Some(k) = reg.iter().position(|&s| s == 0) else {
                break;
            };
            reg[k] = j as u16;
        }
        let (cx, cy, cz) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        // Retarget (EF:61403-31): default = come home; a sphere
        // override only on the fleet-staggered tick, with cargo
        // room, skipping the siblings' claims.
        let bank = self.mc2_owner_bank(own);
        let full = bank.saturating_add(self.ent[i].f140.max(0)) >= self.ent[i].f136;
        // Retail's stagger modulus is the QUOTA (sub_60400,
        // EF:61405), not the live-fleet size — they differ only on
        // a pool-starved shortfall.
        let stagger = bq != 0 && self.ent[i].f63 as usize % bq == 0;
        // The 0..quota walk (EF:61370-61431). Spawn, convert and
        // retarget INTERLEAVE per index — index 0 retargets before
        // index 1 is even resolved — which is why the exclusions
        // below read the register as it stands at that moment.
        for k in 0..bq.min(3) {
            let b = reg[k] as usize;
            if b == 0 {
                // Shortfall spawn (EF:61382-90), then `goto LABEL_17`
                // — a newborn keeps target 0 until the NEXT pass, so
                // its first frames run no flight body at all (mc2l3
                // t=264: the port's same-pass retarget sent the fresh
                // balloon through its servo to ground+512 while
                // retail's sat at the castle z).
                if let Some(nb) = self.mc2_spawn_balloon(cx, cy, cz, own) {
                    reg[k] = nb as u16;
                }
                continue;
            }
            if self.ent[b].act_life < 0 {
                // EF:61397-402 — convert, clear, walk on. NO
                // same-pass replacement: the slot stays empty until
                // the next pass spawns into it.
                self.mc2_balloon_to_sphere(b);
                reg[k] = 0;
                continue;
            }
            if full {
                self.ent[b].f146 = i as u16;
                continue;
            }
            if !stagger || self.ent[b].tick70 != 9 {
                continue;
            }
            self.ent[b].f146 = i as u16; // the castle default
            if self.ent[b].f140 >= self.ent[b].f136 {
                continue; // cargo full → home
            }
            // sub_5F810 (EF:60994): nearest own unclaimed sphere no
            // sibling is on. Retail SKIPS spheres carrying the decay
            // channel `byte[1] & 0x20` (EF:61009 — port flag bit 13,
            // 0x2000): the doomsday mana-rain / corpse-fountain spheres
            // are TEMPORARY (140-tick TTL) and a balloon refuses them —
            // it will not even take off for fountain mana (player
            // retail-observed on mc2l24). A fountain sphere only reaches
            // this scan once claimed (f144 set to the fleet owner); the
            // decay gate is what keeps the fleet grounded. The scanned
            // list (`dword_38523`) also carries the (10,57) FOOL'S-MANA
            // spheres (:40018-63 files models 39, 40 and 57 into it)
            // and the `model == 39` filter is what keeps a balloon off
            // them. Since OPEN-6 a NATIVE m57 carries model 57 too, so
            // the model test is the filter on both paths; the action
            // test is kept as belt-and-braces.
            let (bx, by, bz) = (self.ent[b].x, self.ent[b].y, self.ent[b].z);
            let excl = {
                let t = |s: u16| if s == 0 { 0 } else { self.ent[s as usize].f146 };
                [t(reg[(k + 1) % 3]), t(reg[(k + 2) % 3])]
            };
            let mut best = 0usize;
            let mut best_d = u32::MAX;
            // ⭐⭐ THE SCAN IS THE TICK-TOP `dword_38523` CHAIN, NOT THE
            // POOL. `sub_5F810` seeds from the chain HEAD (EF:61003)
            // and steps `v5x = v5x->next_0` (EF:61021-22), so it sees
            // the ascending tick-top membership the case-10 arm of the
            // rebuild files — and, crucially, NOTHING PAST A SEVERED
            // LINK. A member REALLOCATED mid-tick has its +0 wiped by
            // the NewEvent ctor, so every later walk in that frame
            // stops one node past it (the `TickChain::cut` law,
            // lowered in `Gen::new_event`). A SOFT KILL does NOT sever
            // — that is the re-pick law documented immediately below,
            // and the two are opposite halves of the same chain.
            //
            // mc2l0 t=9953 is the row: sphere 145 is drunk and REBORN
            // as a (10,12) puff earlier in the same tick, cutting the
            // walk after node 145, so balloon 108's own spheres past
            // the cut (156, 164) are invisible and retail takes 101
            // though 164 is nearer in the 3-D metric. The pool scan
            // took 164 and at t=9954 aimed 1320 at it where retail
            // bears 1319 at 101 — the free-run signature
            // `(3,3)slot108:heading`.
            //
            // ⭐ MC1's twin `Gen::castle_balloons` has walked
            // `ball_chain.visible_len()` for this exact pick since
            // mc1l5. ⚠ And the BANKED note on `mc2_castle_absorb`
            // named the wrong call site: the row that pays is the
            // RETARGET, not the absorb.
            for c in 0..self.ball_chain.visible_len() {
                let j = self.ball_chain.list[c] as usize;
                let e = &self.ent[j];
                // ⭐⭐ NO REAP TEST, AND NO LIFE TEST. `sub_5F810`'s
                // gate is EXACTLY four tests (EF:61006-10): model 39,
                // not decay-channelled, owner matches, and not one of
                // the other two fleet members' claims. A sphere
                // swallowed EARLIER IN THIS SAME TICK carries only
                // `byte[1] |= 4` — `DisableEntityDrawing04_57F10`
                // (EF:40332) is that one line and nothing else, so the
                // record keeps its class, model, position and its place
                // on the `dword_38523` chain. The actual free is the
                // TICK-TOP reaper (EF:39954), which runs BEFORE the
                // roster rebuild — so the sphere is still a full chain
                // member for the rest of the frame and only vanishes
                // next tick. mc2l3 t=403: the balloon at slot 162 eats
                // sphere 129 at its own feet (cargo 0 -> 300), then the
                // castle's roster pass at slot 252 still sees 129, and
                // 129 is nearest by 52x in the 3-D metric — so retail
                // RE-PICKS THE SPHERE IT JUST ATE and `target96` stays
                // 129. The port's reap skip dropped it and took slot
                // 215 instead (3-D 8,019,381 vs 152,881), bearing 205
                // where retail bears 0: the t=404 free-run signature
                // `(3,3)slot162:x,y,heading`.
                if e.class64 != 10
                    || e.model65 != 39
                    || e.tick70 == 62
                    || e.flags & 0x2000 != 0
                    || e.f144 != own
                {
                    continue;
                }
                // The two exclusions are REGISTER-INDEXED, not a
                // census of the live fleet: retail passes
                // `Entities[Entities[array[(v1+1)%3]]->word_0x96_150]`
                // and the `+2` twin (EF:61417-19), having first
                // scrubbed `Entities[0]->word_0x96_150 = 0` so an
                // EMPTY register slot resolves to entity 0 and
                // excludes nothing.
                if excl.contains(&(j as u16)) {
                    continue;
                }
                // THE PICK METRIC IS 3-D. `sub_58440` (EF:40430-36) sums
                // wrapping-i16 x, y AND z deltas; it is NOT
                // `Maths::EuclideanDistXYZ_58490`, the 2-D-despite-the-name
                // routine `dist2_sq` ports (Maths.cpp:738-42, x²+y² through
                // a sqrt) — EF:61014 is the ONE call site of 58440 in the
                // whole decompile, so every other `dist2_sq` caller stays
                // 2-D. The compare is UNSIGNED: `v3` seeds at -1 (EF:61000)
                // and EF:61015 tests `v7 < v3` as `unsigned int`, which a
                // 3-D sum can exceed i32::MAX to reach.
                //
                // Same law as MC1's certified ball pick (DEVIATIONS §120,
                // "the ball pick metric is 3-D (sub_42390 includes z)").
                // mc2l3 t=331: balloon 162 docked on the castle pad at
                // z 1536 has ground sphere 247 (25979,49844,0) nearest in
                // XY (496,553 vs 721,225) but sphere 231 (25404,48683,829)
                // nearest in 3-D (1,221,074 vs 2,855,849) — retail takes
                // 231, and the bearing it flies at t=332 is 1726, exactly
                // `angle_of` toward 231 (the port's 1085 is toward 247).
                let dx = (e.x as i16).wrapping_sub(bx as i16) as i32;
                let dy = (e.y as i16).wrapping_sub(by as i16) as i32;
                let dz = e.z.wrapping_sub(bz) as i32;
                let d = dx
                    .wrapping_mul(dx)
                    .wrapping_add(dy.wrapping_mul(dy))
                    .wrapping_add(dz.wrapping_mul(dz)) as u32;
                if d < best_d {
                    best_d = d;
                    best = j;
                }
            }
            if best != 0 {
                self.ent[b].f146 = best as u16;
            }
        }
        // LABEL_22 (EF:61437-45): the quota..3 tail — a downgraded
        // castle sheds exactly the fleet its register still names,
        // and this is the ONLY over-quota cull (the walk above never
        // looks past the quota).
        for k in bq.min(3)..3 {
            let b = reg[k] as usize;
            if b != 0 {
                self.mc2_balloon_to_sphere(b);
                reg[k] = 0;
            }
        }
        self.mc1_balloon_reg.0.insert(own, reg.to_vec());
        // Guard slots (EF:61446-61510): cooldown, then one (5,15)
        // per pass into the courtyard.
        //
        // ⭐⭐ THE COOLDOWN IS `word_0x2C_44` (@0x2C) AND ITS PORT HOME
        // IS `f46`, NOT `f44`. Retail decrements it at EF:61446-48
        // (`v11 = a1x->word_0x2C_44; if (v11 > 0) ... = v11 - 1`),
        // gates on it at EF:61486 and latches 16 at EF:61491. The
        // port's `f44` on a (3,2) carries `subSpellIndex_0x2A`
        // instead, and BOTH runners feed it 100: `import_ent_mc2`
        // maps `f44 <- r.f2a` (a castle is not in `ramp2c`), and the
        // shared allocator seeds `e.f44 = 100` because
        // `NewEvent_4A050` (Events.cpp:569/595) stamps
        // `subSpellIndex_0x2A_42 = 100` on every fresh record. The
        // castle ctor `sub_4AA40` (EF:33362) never overwrites it, so
        // retail's castles carry @0x2A = 100 for life, dead and
        // unread. Reading it as the cooldown gave every port castle a
        // phantom 100-PASS (200-tick) guard lockout — and because
        // `sub_60400` (EF:61522-61559) returns quota 0 at levels 1-2
        // the word is never read until a castle first reaches LEVEL 3,
        // which is precisely where both free runs die: mc2l0
        // t=7610->7611 slot 235 and galore t=1101->1102 slot 266,
        // retail latching @0x2C = 16 and minting the take's FIRST EVER
        // (5,15) while the port spawned nothing. Measured free-run
        // divergence at mc2l0 t=7610: retail f2a 100, port 12 — the
        // phantom counter, decremented once per even STANDING pass
        // since the castle's birth (suspended through the action-5
        // transform), which made the port mint its first guard at
        // t=7633, 22 ticks late, and corrupted the ungraded @0x2A lane
        // the whole way down.
        //
        // ⭐ MC1's CERTIFIED TWIN IS THE PROOF THAT ONLY THE SOURCE
        // OFFSET MOVED: remc1 :56414-51 runs the identical walk on
        // `*(_WORD *)(a1 + 46)` = @0x2E = this same `f46`, and the
        // (unreachable) MC2 arm of `features.rs::castle_balloons`
        // already uses `f46` too. MC2 relocated the word from @0x2E to
        // @0x2C; the algorithm and the port field are unchanged. `f46`
        // is free on a (3,2): the importer's `r.f2e` copy is redundant
        // with `f59` and has no reader anywhere on this column.
        if self.ent[i].f46 > 0 {
            self.ent[i].f46 -= 1;
        }
        // ⭐⭐⭐ AND THE GUARD HALF WALKS A REGISTER TOO — `array_0x5C_92`
        // (EF:61483-61508), the fleet register's twin one field along on
        // the same owner block. The port kept a live CENSUS here long
        // after the fleet half was converted, and `features.rs`'s MC1
        // twin said so in its own words: *"MC2 keeps the live-census
        // stand-in: its dispatcher twin has not been register-verified
        // against the binary, its corpora measure identical under both
        // forms."* mc2l3 t=18148 is the corpus that separates them.
        //
        // Retail's walk is `for v20 in 0..quota` over the OWNER's
        // register, and the two arms are NOT symmetric:
        // * slot EMPTY (index 0) → spawn one (5,15) if the cooldown is
        //   down, latch 16, and record the new index;
        // * slot STALE (the record is no longer a live (5,15), or it is
        //   the state-125 corpse) → CLEAR THE SLOT AND LATCH 16 WITH NO
        //   SPAWN. The replacement costs a full cooldown, and the pass
        //   that pays it looks, from the pool alone, exactly like a pass
        //   that should have spawned.
        //
        // ⭐ A census cannot see the difference, because the stale entry
        // is retail-only memory of a guard that is already gone. mc2l3
        // t=18148, the re-sited castle at slot 254 (level 3, quota 4)
        // with ZERO live (5,15) anywhere in the pool: retail latches
        // `word_0x2C_44` 0 → 16 and mints nothing, because the register
        // it inherited from the DEMOLISHED castle still named four dead
        // guards. The port's census read 0 < 4, spawned into slot 245 —
        // the record retail frees in that very tick — and the take's
        // signature was `extra(5,15)slot245x1`.
        //
        // ⭐⭐ THE REGISTER LIVES ON THE OWNER, SO IT OUTLIVES THE CASTLE.
        // That is the whole point here: nothing in the self-destruct
        // ladder clears `array_0x5C_92`, so the castle the player
        // re-sites starts its guard ladder holding the previous
        // castle's ghosts, and pays 16 passes per rung to shed them.
        if gq > 0 {
            let mut greg = self
                .mc1_guard_reg
                .0
                .get(&own)
                .cloned()
                .unwrap_or_else(|| vec![0u16; 34]);
            for k in 0..gq.min(greg.len()) {
                let s = greg[k] as usize;
                if s == 0 {
                    if self.ent[i].f46 != 0 {
                        continue;
                    }
                    let gx = cx.wrapping_add(128);
                    let gy = cy.wrapping_add(640);
                    let gz = self.ground_z(gx, gy) as i16;
                    if let Some(g) = self.mc2_spawn_m15(gx, gy, gz) {
                        self.ent[i].f46 = 16;
                        self.ent[g].id24 = own;
                        self.ent[g].f144 = own;
                        self.ent[g].f30 = 512;
                        self.ent[g].f34 = 512;
                        greg[k] = g as u16;
                    }
                } else {
                    // ⚠ NO REAP TEST — retail asks class, model and the
                    // corpse action and nothing else (EF:61506). A guard
                    // killed earlier in THIS tick still reads (5,15)
                    // until the tick-top reaper clears its class next
                    // frame, so it holds its slot for one more pass.
                    let g = &self.ent[s];
                    if g.class64 != 5 || g.model65 != 15 || g.tick70 == 125 {
                        greg[k] = 0;
                        self.ent[i].f46 = 16;
                    }
                }
            }
            self.mc1_guard_reg.0.insert(own, greg);
        }
    }

    /// EF:61396-97 / EF:61440-41, both literally
    /// `TransformEntityToManaSphere_36BA0(x, false)` followed by
    /// `DisableEntityDrawing04_57F10(x)`.
    ///
    /// ⚠⚠ THIS WAS A SECOND, IMPOVERISHED COPY OF A FUNCTION THE PORT
    /// ALREADY HAD EXACT. [`Gen::mc2_mana_spheres`] (mc2/mobs.rs) is
    /// `TransformEntityToManaSphere_36BA0` verbatim — the SOURCE's
    /// pre-loop draw (EF:26889), the sphere's two draws feeding
    /// `yaw = (r1 % 0x71 + src.yaw - 56) & 0x7FF` and
    /// `actSpeed = r2 % 0x30 + 16`, the `word_0x2C_44` fall arc from
    /// the SOURCE's height over ground, the `MoveEntity_57FA0`
    /// velocity seed into `dest_x`/`dest_y`, and the tail
    /// `playerEntityIndex = 0`. This copy had NONE of them, and
    /// carried an INVENTED `ball_resize` on top: retail's ctor
    /// `CreateManaSphere_500C0` runs `SetManaSphereColorAndRot_36920`
    /// (EF:36607) while mana is still 512 and the owner still 0, and
    /// the transform never re-runs it.
    ///
    /// Measured, mc2l3 t=585 slot 127 (the dying castle's balloon):
    /// retail `yaw 1306` = `(25497 % 0x71 + 1290 - 56) & 0x7FF` off
    /// the balloon's own 1290, `speed 56` = `r % 0x30 + 16`, and
    /// `f2c 64` = `(1024 - 512) / 8` from the balloon's 512 over
    /// ground — none of which this copy produced.
    fn mc2_balloon_to_sphere(&mut self, b: usize) {
        self.mc2_mana_spheres(b, false);
        self.ent[b].flags |= 0x400;
    }

    /// `sub_4ABA0` (EF:33409) — the MC2 (3,3) balloon ctor: life
    /// 10000, speed 48, cargo cap 10000, ch0 intake, behavior row
    /// 68 (= ROW_BASE + 9, the same servo family as MC1's row 9),
    /// sprite 169 (+ team). The ctor's action 7 is overwritten to
    /// the working 9 by the roster (EF:61391) — spawned here as 9
    /// directly.
    fn mc2_spawn_balloon(&mut self, x: u16, y: u16, z: i16, own: u16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 3;
            e.model65 = 3;
            e.tick70 = 9;
            e.max_life = 10000;
            e.act_life = 10000;
            e.f126 = 48;
            e.f136 = 10000;
            e.f140 = 0;
            e.f28 = 1; // byte_0x38_56 = 1: ch0 vulnerable
            e.row156 = 9; // behavior row (MC2 abs 68 = base + 9)
            e.id24 = own;
            e.f144 = own;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        // Balloon family 169+k is authored in Transform order like
        // every MC2 team art band (crate::mc2::COLOR_ART).
        let team = self.owner_team(own).unwrap_or(0);
        self.mc2_set_sprite(i, 169 + crate::mc2::color_art(team) as u16);
        if self.is_cave() {
            // The cave placement box override (EF:33426-27,
            // SetEntityShiftRot(256, 768)).
            self.mc2_shift_rot(i, 256, 768);
        }
        Some(i)
    }

    /// `AddBallon_60AB0` (EF:61763) — the MC2 balloon tick: fly at
    /// the target (f146); a class-10 sphere target is tethered
    /// within 1024 (2048 on caves, EF:61793-96 — cave castles
    /// vacuum spheres from twice as far), absorbed on overlap (cargo +
    /// owner claim + full heal); a class-3 castle target delivers
    /// the whole cargo inside the level×speed ring below the servo
    /// altitude. `sub_60EA0` intake at the tail: straight subtract,
    /// owner balloon-alert, killer memory — the corpse is the
    /// roster pass's business (no despawn here).
    pub(crate) fn mc2_balloon_tick(&mut self, i: usize) {
        use super::behavior::{BEHAVIOR, ROW_BASE};
        let t = self.ent[i].f146 as usize;
        let row = &BEHAVIOR[ROW_BASE + self.ent[i].row156 as usize];
        // The target read is identity-blind (EF:61779): the only
        // retail gate is the slot itself — no liveness, reap-mark or
        // model test — so a reaped-and-reminted target slot is chased
        // as-is (re-bear + servo; the "ours" test then filters the
        // move/absorb). The MC1 balloon_move guards fell to the same
        // law 2026-08-12; the recycled-record absorb is retail's own
        // latent bug and the port's law.
        if t != 0 && t < self.ent.len() {
            let mut pos = {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            };
            let (tx, ty) = (self.ent[t].x, self.ent[t].y);
            let yaw = Self::angle_between(pos.0, pos.1, tx, ty);
            self.ent[i].f30 = yaw;
            let speed = self.ent[i].f126;
            let mut step = true;
            if self.ent[t].class64 == 10 {
                if self.ent[t].f144 != self.ent[i].id24 {
                    step = false; // not ours (EF:61791)
                } else {
                    let d = Self::isqrt(Self::dist2_sq(pos.0, pos.1, tx, ty) as u32) as i32;
                    let tether = if self.is_cave() { 2048 } else { 1024 };
                    if d > tether {
                        self.ent[t].flags &= !0x40; // release tether
                    } else {
                        self.ent[t].flags |= 0x40;
                        self.ent[t].f146 = i as u16;
                        if self.ent_overlap(i, t) {
                            let cargo = self.ent[t].f140;
                            let claim = self.ent[t].f144;
                            self.ent[i].f140 += cargo;
                            self.ent[i].f144 = claim;
                            self.ent[i].f146 = 0;
                            self.ent[i].act_life = self.ent[i].max_life as i32;
                            self.ent[t].flags |= 0x400;
                        }
                    }
                    if d <= speed as i32 {
                        pos.0 = tx;
                        pos.1 = ty;
                        step = false;
                    }
                }
            } else if self.ent[t].class64 == 3 {
                // Castle delivery ring = level * speed (EF:61828).
                let d = Self::isqrt(Self::dist2_sq(pos.0, pos.1, tx, ty) as u32) as i32;
                if d <= self.ent[t].f26 as i32 * speed as i32 {
                    let ground = self.ground_z(pos.0, pos.1) as i16;
                    if pos.2 <= ground.wrapping_add(row.v_12) && self.ent[t].f26 > 0 {
                        pos.0 = tx;
                        pos.1 = ty;
                        let cargo = self.ent[i].f140;
                        self.ent[t].f140 += cargo;
                        self.ent[i].f140 = 0;
                        self.ent[i].f144 = self.ent[i].id24;
                        self.ent[i].act_life = self.ent[i].max_life as i32;
                    }
                    step = false;
                }
            }
            if step {
                Self::polar_step(&mut pos, yaw, self.ent[i].f32, speed);
            }
            // The MC2 altitude servo `sub_580E0` (EF:40372): descend by
            // the row's v_14 whenever ABOVE ground, then floor at
            // ground+v_12. The call site passes v_12/v_10/v_14
            // (EF:61857-61 surface / EF:61933-35 cave sub_60D50) but
            // sub_580E0's a4 (= v_10) is DEAD — MC2 has NO ceiling
            // band and NO 25% intermediate step. MC1's `alt_clamp`
            // is a DIFFERENT 3-branch function; reusing it here sank
            // the balloon only 25%·v_14 through the v_12..v_10 band,
            // leaving the port 12 units high per tick over open sky
            // (the mc2-balloon-z −12 field residual).
            let ground = self.ground_z(pos.0, pos.1) as i16;
            let mut z = pos.2;
            let servo = |z: &mut i16| {
                if *z > ground {
                    *z = z.wrapping_add(row.v_14);
                }
                if *z <= ground.wrapping_add(row.v_12) {
                    *z = ground.wrapping_add(row.v_12);
                }
            };
            if self.is_cave() {
                // The CEILING WALK (`sub_60D50` EF:61872, called from
                // the cave branch EF:61848-50): flags bit0 = "walking
                // on the ceiling" — attach when the tile is sealed or
                // the poke test fires, detach when open sky returns;
                // actSpeed 96 walking / 48 flying; sound 22 on each
                // transition behind a 32-tick cooldown (byte_0x46_70
                // → f71); then the same row servo, and a ceiling−fov
                // clamp while FLYING only.
                let t = crate::engine::features::tile((pos.0 >> 8) as u8, (pos.1 >> 8) as u8);
                let roof = self.t.angle[t] & 8 != 0
                    || self.cave_poke(self.ent[i].f84 as i32, row.v_12 as i32, pos.0, pos.1);
                let walking = self.ent[i].flags & 1 != 0;
                let mut transition = false;
                if walking {
                    if !roof {
                        self.ent[i].flags &= !1;
                        transition = true;
                    }
                    self.ent[i].f126 = 96;
                } else {
                    if roof {
                        self.ent[i].flags |= 1;
                        transition = true;
                    }
                    self.ent[i].f126 = 48;
                }
                if self.ent[i].f71 != 0 {
                    self.ent[i].f71 -= 1;
                }
                if transition && self.ent[i].f71 == 0 {
                    self.snd(22, i);
                    self.ent[i].f71 = 32;
                }
                servo(&mut z);
                if self.ent[i].flags & 1 == 0 {
                    let c = (self.ceiling_z(pos.0, pos.1) as i16 as i32 - self.ent[i].f84 as i32)
                        as i16;
                    if z > c {
                        z = c;
                    }
                }
            } else {
                servo(&mut z);
            }
            self.move_relink(i, pos.0, pos.1, z);
        }
        // sub_60EA0 (EF:61939): the tail intake.
        if self.ent[i].act_life >= 0 && self.ent[i].mail[0].1 != 0 {
            let (amt, src) = self.ent[i].mail[0];
            self.ent[i].act_life -= amt as i32;
            // Retail sets byte_0x197_407 for ANY owner (EF:61947);
            // ours is the player-side HUD latch (deliberate).
            if self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
                self.balloon_alert = 4;
            }
            if self.ent[i].act_life < 0 {
                self.ent[i].f36 = src;
            } else {
                self.ent[i].mail[0].1 = 0;
            }
        }
    }

    // ---- the (10,42) build painter -------------------------------------------

    /// `sub_5FBD0`/`sub_50370` (EF:61182/36733): spawn a (10,42)
    /// painter at the castle's build site. `repaint` = the state-3
    /// arm (generic ctor → f59 = 1 → long settle); the upgrade
    /// spawns with f59 = 0 (short settle).
    pub(crate) fn mc2_spawn_castle_painter(&mut self, castle: usize, repaint: bool) {
        // The BUILD row IS the castle level, verbatim and UNCLAMPED
        // (`sub_5FBD0` EF:60336 `indexx->byte_0x46_70 =
        // a1x->dword_0x10_16`, and the authored spawn's per-level
        // pass EF:43797). Level 0 selects BUILD00 row 0, which is
        // EMPTY (w = h = 0) — a level-0 castle is a bare flag with no
        // structure, which is exactly why the destroy path never
        // un-stamps it. Clamping the row UP to 1 here stamped a
        // level-1 tower that nothing would ever remove.
        let row = self.ent[castle].f26.clamp(0, 7) as u8;
        if self
            .mc2_spawn_castle_painter_at(castle, row, repaint)
            .is_some()
        {
            self.ent[castle].f59 = 4;
        }
    }

    /// `sub_377A0` (EF:27751)'s per-wizard mint, the `sub_5FBD0`
    /// body with a WIZARD as `a1x`: the painter spawns at the
    /// wizard's SPARE AXIS `axis_0x9A_154` (the human's reads
    /// (0,0,0) on this corpus), row = the wizard's `dword_0x10_16`
    /// (0 for a castle-less human — BUILD00 row 0 is EMPTY, the
    /// painter runs its 19+settle window painting nothing and dies),
    /// id = the wizard's own id, parent = the wizard's slot, and the
    /// WIZARD's `word_0x2E_46 = 4` (the pass-live latch — for the
    /// out-of-pool human that write lands on the zeroed husk, dead
    /// state like retail's own row-0 pass). mc2l0 t=3221: the
    /// dis-13 village wave's four completing buildings each overlap
    /// the human → painters at slots 49/255/256/257, all (0,0,0)
    /// row 0 parent 152.
    pub(crate) fn mc2_spawn_wizard_painter(
        &mut self,
        pos: (u16, u16, i16),
        row: u8,
        _own: u16,
        parent: u16,
    ) -> Option<usize> {
        let i = self.mc2_new_painter_record(pos, row)?;
        // Same fused-parent id24 convention as the castle painter
        // (retail @0x28 = the wizard's own slot here). ⚠ `f40` is the
        // port's home for retail `@0x26`, which `sub_5FBD0` never
        // writes — stamping the parent there too put 152 on a lane
        // retail leaves at 0.
        self.ent[i].id24 = parent;
        Some(i)
    }

    /// ⭐⭐ **`sub_50370` (EF:36735) — THE (10,42) CTOR DOES NOT LINK,
    /// AND IT ARMS THE SETTLE WINDOW.** Every painter in the game is
    /// minted through `_4A190(pos, 10, 42)`, and that ctor
    /// ASSIGNS `position_0x4C_76` directly instead of calling
    /// `AddEventToMap_57D70` — so a painter is never a member of any
    /// tile chain — clears flag bits 0 and 3 and sets bit 0
    /// (`&= 0xF6; |= 1`, i.e. walk, NOT collidable, NOT linked), and
    /// stamps `byte_0x3B_59 = 1` UNCONDITIONALLY. Extents come from
    /// the plain `SetShiftByCastle_49EC0` (EF:32882), whose `fov` is
    /// a flat **256** — the `0x4000` in [`Self::mc2_castle_extents_ent`]
    /// belongs to the castle aim stamp that follows a CASTLE's extent
    /// refresh, and no such stamp follows a painter.
    ///
    /// The `+0x3B` stamp is the load-bearing one: the painter tick's
    /// countdown end (EF:27760) parks at `-25` when it is set and
    /// `-1` when it is not, and `-1` is a DEATH. mc2l0's four village
    /// painters minted at t=3221 with `f59 = 0` therefore expired at
    /// t=3240, set their own reap mark and were gone by 3241, where
    /// retail's sit out the settle window as inert records — the
    /// take's `missing (10,42)` entity-set break. Landing the ctor
    /// verbatim moves the free horizon 3240 → **3470**, which is
    /// exactly the take's first divergent PAIR — the free run has
    /// caught up with the graded census.
    fn mc2_new_painter_record(&mut self, pos: (u16, u16, i16), row: u8) -> Option<usize> {
        let i = self.new_event()?;
        let e = &mut self.ent[i];
        e.class64 = 10;
        e.model65 = 42;
        e.tick70 = 0x2C; // action 44 → AddTerrainMod0A_2A_37BC0
        e.max_life = 0;
        e.act_life = 0; // CopyMaxLifeToLife_49A20
        e.flags = (e.flags & !0x9) | 1;
        e.f59 = 1;
        e.f71 = row;
        e.x = pos.0;
        e.y = pos.1;
        e.z = pos.2;
        let (w, h) = self
            .assets
            .build_tab
            .get(row as usize)
            .map_or((0u16, 0u16), |d| (d.w as u16, d.h as u16));
        let e = &mut self.ent[i];
        e.f78 = 0;
        e.f80 = ((w << 8).wrapping_add(1280)) >> 1;
        e.f82 = ((h << 8).wrapping_add(1280)) >> 1;
        e.f84 = 256;
        Some(i)
    }

    fn mc2_spawn_castle_painter_at(
        &mut self,
        castle: usize,
        row: u8,
        repaint: bool,
    ) -> Option<usize> {
        let (x, y, site_z) = {
            let e = &self.ent[castle];
            (e.x, e.y, e.site_z)
        };
        let i = self.mc2_new_painter_record((x, y, site_z), row)?;
        let e = &mut self.ent[i];
        // ⚠ `byte_0x3B_59` is the ctor's flat 1 (see
        // [`Self::mc2_new_painter_record`]); the repaint distinction
        // the port carries here is a port-side pin with no decompile
        // site behind it, and it is now the ONLY thing this mint adds
        // over the ctor. Left in place because the castle column's
        // corpus rows sit on it — the wizard painter is what the
        // corpus arbitrated, and it takes the flat 1.
        e.f59 = u8::from(repaint);
        // The port's id24 convention for painters is the FUSED
        // parentId @0x28 (the import's `id24 = tr(@0x28)`, and
        // `obs_project_mc2` recovers the recorded `owner` lane
        // from it) — the parent CASTLE slot, not the owning
        // wizard (mc2l3 t=243 slot 131: retail owner obs 127 =
        // the castle). The wizard identity rides the parent.
        e.id24 = castle as u16;
        Some(i)
    }

    /// `AddTerrainMod0A_2A_37BC0` (EF:27648) — the painter tick:
    /// 19-tick progressive rise of the CUMULATIVE footprint (BUILD00
    /// rows 1..=level, each cell toward authored height + datum),
    /// sprite/texture paint on the 1st, every 7th and last tick,
    /// then the settle window (f59: 1 tick, or 25 on a repaint)
    /// which flips built cells' angle bit3 → bit7 (feeding the
    /// space check), signals the parent castle (f59 = 2) and
    /// despawns. Returns true when terrain changed.
    pub(crate) fn mc2_castle_painter_tick(&mut self, i: usize) -> bool {
        // First tick: seed the countdown (byte[0] bit1 latch).
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.ent[i].f26 = 19;
        }
        // Parent = the fused id24 (@0x28), NOT f40: both ctors stamp
        // both, but an IMPORTED painter only carries the parent in
        // id24 (the import fuses @0x28 there; f40 mirrors @0x26 = 0),
        // and the settle-end stamp below is how the castle learns its
        // build finished (retail EF:27753 writes through parentId).
        let parent = match self.ent[i].id24 as usize {
            p if p != 0 && p < self.ent.len() => p,
            _ => 0,
        };
        let row = (self.ent[i].f71 as usize).min(7);
        let Some(def) = self.assets.build_tab.get(row).copied() else {
            self.ent[i].flags |= 0x400;
            return false;
        };
        // The working frame = the level row's footprint, widened to
        // the largest accumulated row. BUILD00 rows 1-7 are
        // 8/21/21/35/35/48/48 — monotone non-decreasing, row 7 =
        // 48×48 like row 6; the 1×1 rows are 8-16 and are never a
        // castle level. So this widening loop is a no-op for every
        // reachable level (kept as belt-and-braces for modded tabs).
        let (mut w, mut h) = (def.w as usize, def.h as usize);
        for r in 1..=row {
            if let Some(rd) = self.assets.build_tab.get(r) {
                w = w.max(rd.w as usize);
                h = h.max(rd.h as usize);
            }
        }
        let cx = (self.ent[i].x.wrapping_add(128) >> 8) as u8;
        let cy = (self.ent[i].y.wrapping_add(128) >> 8) as u8;
        let tlx = cx.wrapping_sub((w / 2) as u8);
        let tly = cy.wrapping_sub((h / 2) as u8);

        if self.ent[i].f26 <= 0 {
            // ── phase B: settle, then finalize ──
            self.ent[i].f26 += 1;
            if self.ent[i].f26 == 0 {
                // bit3 → bit7 over the footprint (EF:27737-45) —
                // NON-CAVE only (EF:27729): on caves bit3 is the
                // seal, owned by the ceiling-rise arm.
                if !self.is_cave() {
                    for dy in 0..h {
                        for dx in 0..w {
                            let t = tile(tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                            if self.t.angle[t] & 8 != 0 {
                                self.t.angle[t] = (self.t.angle[t] & 0xF7) | 0x80;
                            }
                        }
                    }
                }
                if parent != 0 && self.ent[parent].flags & 0x400 == 0 {
                    self.ent[parent].f59 = 2; // pass done
                }
                self.ent[i].flags |= 0x400;
            }
            return false;
        }
        // ── phase A: the progressive rise ──
        self.ent[i].f26 -= 1;
        if self.ent[i].f26 == 0 {
            self.ent[i].f26 = if self.ent[i].f59 != 0 { -25 } else { -1 };
            return false;
        }
        // Painting pauses while the castle runs its settle
        // animation (EF:27767).
        if parent != 0 && self.ent[parent].f50 != 0 {
            return false;
        }
        let countdown = self.ent[i].f26 as i32;
        let datum = (self.ent[i].z >> 5) as i32;
        // (1) accumulate per-cell targets over rows 1..=row, mapped
        // into the frame (retail writes a shared scratch keyed by
        // map cell — same cells).
        let mut delta = vec![0i32; w * h];
        let mut paint: Vec<(u8, u8, u8)> = Vec::new();
        let do_paint = countdown % 7 == 0 || countdown == 1;
        let kill = self.ent[i].flags & F_BUILD_KILL != 0;
        // The crush spares the OWNER WIZARD's entities — retail
        // passes the painter's id26 (the wizard). The port's painter
        // id24 is the FUSED parent (the castle slot), so the wizard
        // identity resolves through the parent castle's id24.
        let owner = if parent != 0 && parent < self.ent.len() {
            self.ent[parent].id24
        } else {
            self.ent[i].id24
        };
        for r in 1..=row {
            let Some(rd) = self.assets.build_tab.get(r).copied() else {
                continue;
            };
            let (rw, rh) = (rd.w as usize, rd.h as usize);
            let start = rd.offset as usize;
            let Some(cells) = self.assets.build_dat.get(start..start + 2 * rw * rh) else {
                continue;
            };
            let cells = cells.to_vec();
            // Retail's per-row origin is center - (dim >> 1), i.e.
            // the frame offset is D/2 - d/2 — NOT (D - d)/2, which
            // loses a tile whenever D is even and d odd (EF:27798:
            // v33 = (v36>>1) - v8), sitting every interior ring one
            // tile toward -x/-y of the outer ring.
            let offx = w / 2 - rw / 2;
            let offy = h / 2 - rh / 2;
            for dy in 0..rh {
                for dx in 0..rw {
                    let c = &cells[2 * (dy * rw + dx)..2 * (dy * rw + dx) + 2];
                    let gx = tlx.wrapping_add((offx + dx) as u8);
                    let gy = tly.wrapping_add((offy + dy) as u8);
                    // THE CASTLE AS A WEAPON (EF:27826-27): while the
                    // kill bit is set, EVERY cell of the cumulative
                    // footprint is purged on EVERY tick of the rise —
                    // ahead of the height write, and regardless of
                    // whether this cell carries a pad height or a
                    // paint code. That relentless cadence is what
                    // makes a rising castle lethal to anything that
                    // wanders onto it, not just to what stood there
                    // when the build began.
                    if kill {
                        self.mc2_building_clear_tile(tile(gx, gy), owner);
                    }
                    if c[1] != 0xff {
                        let t = tile(gx, gy);
                        delta[(offy + dy) * w + offx + dx] =
                            c[1] as i32 + datum - self.t.height[t] as i32;
                    }
                    if do_paint && c[0] != 0xff {
                        paint.push((gx, gy, c[0]));
                    }
                }
            }
        }
        // (2) apply 1/countdown of each delta (EF:27846-70). The
        // cave ceiling-rise arm (EF:27871-94) and the non-cave
        // countdown==2 bit3 sweep (EF:27895) sit OUTSIDE the
        // active-delta gate — they run for every frame cell.
        for dy in 0..h {
            for dx in 0..w {
                let d = delta[dy * w + dx];
                let (gx, gy) = (tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                let t = tile(gx, gy);
                if d != 0 {
                    // EF:27852's auto-flat predicate is sub_57450
                    // (morph::auto_flat), NOT the damage pass's
                    // burnable set (flood::burn_flags).
                    if self.t.height[t] == 0 || super::morph::auto_flat(self.t.tile_type[t]) {
                        self.t.angle[t] = (self.t.angle[t] & 0xF8) | 1;
                        self.mc2_add_building_region(gx, gy, gx, gy);
                    }
                    self.t.height[t] = (self.t.height[t] as i32 + d / countdown) as u8;
                    if countdown == 1 && self.t.angle[t] & 0x80 != 0 {
                        // Last rise tick: clear bit7; on NON-cave
                        // also set bit3 for phase B to re-promote
                        // (EF:27859-69 — the cave seal arm below is
                        // the only bit3 authority on caves).
                        self.t.angle[t] &= 0x7F;
                        if !self.is_cave() {
                            self.t.angle[t] |= 8;
                        }
                    }
                }
                if self.is_cave() {
                    // Carve the headroom bubble: ceiling eases to
                    // max(floor, datum)+100, then the seal invariant
                    // re-asserts (EF:27871-94).
                    let floor = self.t.height[t] as i32;
                    let tgt = (floor.max(datum) + 100).min(255);
                    let c = self.t.ceiling[t] as i32;
                    if tgt > c {
                        self.t.ceiling[t] = (c + (tgt - c) / countdown) as u8;
                    }
                    self.cave_seal_fixup(t);
                } else if countdown == 2 {
                    self.t.angle[t] &= !8;
                }
            }
        }
        for (gx, gy, code) in paint {
            // sub_45DC0(7, ...) — the groove-castle path's fixed
            // column counter (EF:27832).
            self.mc2_paint_cell(7, gx, gy, code);
        }
        true
    }

    // ---- the downgrade terrain restore ---------------------------------------

    /// `RemoveCastleStage_385C0` (EF:28071), the scratch-entity
    /// (model 0) arm the downgrade drives: un-stamp one BUILD00
    /// footprint — per active cell reset the angle nibble, the 2x2
    /// rubble stamp, drop the pad height back with the verbatim
    /// jitter RNG (datum-based zKoef, every 8th cell 10 lower is
    /// the sphere-drop height only — no spheres here: the scratch
    /// runs with level 0, the 10% haircut already scattered), then
    /// one retile over the footprint.
    ///
    /// ⭐⭐⭐ THE JITTER RNG IS THE SCRATCH ENTITY'S, NOT THE CASTLE'S.
    /// `sub_605E0` (EF:61629-36) does not hand `RemoveCastleStage_385C0`
    /// the castle — it stages `Entities_EA3E4[0]` (position, level as
    /// `byte_0x46_70`, owner, `model_0x40_64 = 0`, `dword_0x10_16 = 0`,
    /// `parentId_0x28_40` = the castle) and passes THAT. Every
    /// `event->rand_0x14_20 = 9377 * … + 9439` in the height arm
    /// (EF:28149-28160) therefore lands on SLOT 0, and the castle's own
    /// `rand` does not move across the whole un-stamp.
    ///
    /// Two things ride on it, and only the first is visible as a lane:
    /// the castle's `rand` (drawing it here desynchronises every later
    /// draw the castle makes), and — the one that matters — the HEIGHTS
    /// themselves, which come out of slot 0's chain and not the
    /// castle's. mc2l3 t=16248, the last rung of the self-destruct
    /// ladder (castle 162, level 1 → 0): retail's slot 0 goes
    /// `rand` 0 → 46293 (107 draws) with `owner28` 162 and `b46` 1 —
    /// the scratch staging, read straight off the capture — while the
    /// castle holds 29231 on both sides of the tick. The port drew 103
    /// times on the castle instead.
    ///
    /// ⚠ Slot 0's `rand` is a PERSISTENT field, not a per-call scratch:
    /// it reads 0 through every earlier downgrade in the take (15904,
    /// 15956) because those un-stamps found `template >= height` on
    /// every cell and took the no-draw arm, and it holds 46293 from
    /// 16248 to the end of the level. So it must be a real pool lane
    /// the port advances, not a local.
    ///
    /// ⚠ The OTHER caller of `RemoveCastleStage_385C0` — the building
    /// collapse ([`World::mc2_house_collapse`]) — passes a REAL entity,
    /// so its jitter is that building's own `rand`. The scratch is
    /// specific to the castle downgrade's staging.
    fn mc2_castle_unstamp(&mut self, i: usize, row: u8) {
        self.terrain_dirty = true;
        let Some(def) = self.assets.build_tab.get(row as usize).copied() else {
            return;
        };
        let (w, h) = (def.w as usize, def.h as usize);
        let start = def.offset as usize;
        let Some(cells) = self
            .assets
            .build_dat
            .get(start..start + 2 * w * h)
            .map(<[u8]>::to_vec)
        else {
            return;
        };
        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
        let tlx = ((ex.wrapping_add(128) >> 8) as u8).wrapping_sub((w / 2) as u8);
        let tly = ((ey.wrapping_add(128) >> 8) as u8).wrapping_sub((h / 2) as u8);
        for dy in 0..h {
            for dx in 0..w {
                let c = &cells[2 * (dy * w + dx)..2 * (dy * w + dx) + 2];
                if c[0] == 0xff && c[1] == 0xff {
                    continue;
                }
                let (gx, gy) = (tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                let t = tile(gx, gy);
                self.t.angle[t] = (self.t.angle[t] & 0x70) | 1;
                self.mc2_add_building_region(gx, gy, gx, gy);
                if c[1] != 0xff {
                    let cur = self.t.height[t];
                    if c[1] >= cur {
                        self.t.height[t] = 0;
                    } else {
                        // ⭐ SLOT 0 — the staged scratch entity, not
                        // the castle (see this fn's doc comment).
                        let d = self.ent_rand(0);
                        if d % 0x32 <= 20 {
                            self.t.height[t] = cur.wrapping_sub(c[1]);
                        } else {
                            let d2 = self.ent_rand(0);
                            self.t.height[t] =
                                cur.wrapping_sub(c[1].wrapping_sub((d2 % 0x14) as u8));
                        }
                    }
                }
            }
        }
        // Retail's finalizer is the gated 3×3 height smoother over
        // exactly the footprint (SetHeightmapByBuildingArea_48B50,
        // EF:28171) — NOT a retile.
        self.mc2_smooth_heights_region(tlx, tly, h as u8, w as u8);
    }

    /// `SetHeightmapByBuildingArea_48B50` (EF:32446) — the unstamp
    /// finalizer: row-major over rows×cols from the origin, the
    /// gated 3×3 floor smoother (`SetHeightmapByBuilding_48B90` =
    /// [`Gen::mc2_smooth_pad_edge`]) on every cell (no 0xff skip,
    /// no border).
    ///
    /// Shared by BOTH un-stamp sites: the castle's (above) and the
    /// BUILDING demolish's (`RemoveCastleStage_385C0` EF:28171, the
    /// `fontTypeIndex == 0` branch — [`World::mc2_house_collapse`]).
    /// Raster order matters: a smoothed cell feeds its right/lower
    /// neighbours' windows, so this is a one-pass IIR blur, not an
    /// independent 3×3 average.
    pub(crate) fn mc2_smooth_heights_region(&mut self, x: u8, y: u8, rows: u8, cols: u8) {
        for r in 0..rows {
            for c in 0..cols {
                self.mc2_smooth_pad_edge(x.wrapping_add(c), y.wrapping_add(r));
            }
        }
    }

    // ---- the (10,79) stage pieces --------------------------------------------

    /// Free the castle's (10,79) piece set. Retail (EF:62256-62264)
    /// WALKS THE CHAIN the rebuild itself built —
    /// `castle.word_0x34_52` → piece → piece → 0 — and hands every
    /// node to `sub_57F20` (Events.cpp:5209-39), which is an
    /// **IMMEDIATE** free: tile-unlink, recycle-stack removal,
    /// `class = 0`, and the slot PUSHED BACK ON THE FREE STACK right
    /// now. (Retail's *other* primitive, the deferred `0x400` flag,
    /// is `DisableEntityDrawing04_57F10` — EF:61665 uses it for the
    /// castle BODY and deliberately not for the pieces.) The very
    /// next `new_event` therefore pops the piece that was just freed
    /// (LIFO), which is why a retail rebuild RE-USES the old records
    /// instead of minting a fresh set.
    ///
    /// Measured, mc2l0-spells-galore t=1055 (castle 266, level 1→2,
    /// free-stack tail `[…,467,474,475,220,458]`): retail frees the
    /// lone old piece 230, pushing it on top, and allocates
    /// 230,220,475,474 — while the deferred flag this used to set
    /// left 230 off the stack and shifted every pop by one
    /// (220,475,474,467): one record too many AND the four footprint
    /// corners landing on the wrong four slots. The offsets
    /// themselves were never wrong.
    ///
    /// A piece's `word_0x34_52` has no port home — `f54` carries the
    /// piece's `@0x36` windup boost there (conformance.rs:1568) — so
    /// the FORWARD link is derived from the back-links the recording
    /// does carry: `p(k+1)` is the (10,79) whose `@0x32` (`f52`) is
    /// `p(k)`.
    fn mc2_castle_free_stages(&mut self, i: usize) {
        let mut node = self.ent[i].f54 as usize;
        for _ in 0..64 {
            // EF:62258-59 stops at `Entities_EA3E4[0]`; the extra
            // already-free guard only fires on states retail cannot
            // reach — a double free would double-push the slot.
            if node == 0 || node >= self.ent.len() || self.ent[node].class64 == 0 {
                break;
            }
            let mut next = 0usize;
            for j in 1..self.ent.len() {
                if j != node
                    && self.ent[j].class64 == 10
                    && self.ent[j].model65 == 79
                    && self.ent[j].f52 as usize == node
                {
                    next = j;
                    break;
                }
            }
            self.free_entity(node); // sub_57F20 — IMMEDIATE
            node = next;
        }
        self.ent[i].f54 = 0; // EF:62265 `a1x->word_0x34_52 = 0`
    }

    /// `sub_613D0` (EF:62233): rebuild the visible (10,79) piece
    /// set for the current level — free the old chain, then walk
    /// DOWN from the castle level to the highest RESEARCHED stage
    /// (`array_0x24E_590[9+lvl]` nonzero, EF:62271-77) and spawn
    /// one piece per [`MC2_STAGE_PARTS`] offset at that stage's
    /// footprint, z = ground + 384 (level <= 1) / 224 (EF:62315).
    /// Research is empty pre-4.2 (`mc2_castle_part_type`), so
    /// castles stand piece-less exactly like a retail castle whose
    /// research entities haven't completed — the painted terrain
    /// carries the shape.
    pub(crate) fn mc2_castle_stages(&mut self, i: usize) {
        self.mc2_castle_free_stages(i);
        let lvl = self.ent[i].f26;
        let own = self.ent[i].id24;
        if own == 0 || lvl <= 0 {
            return;
        }
        // The walk-down: the highest stage <= level with a
        // researched part-type (EF:62271-77).
        let mut stage = lvl.clamp(1, 7) as u8;
        let mut part = 0u8;
        while stage > 0 {
            part = self.mc2_castle_part_type(own, stage);
            if part != 0 {
                break;
            }
            stage -= 1;
        }
        if stage == 0 {
            return;
        }
        let cx = (self.ent[i].x.wrapping_add(128) >> 8) as u8;
        let cy = (self.ent[i].y.wrapping_add(128) >> 8) as u8;
        let Some(def) = self.assets.build_tab.get(stage as usize).copied() else {
            return;
        };
        let tlx = cx.wrapping_sub(def.w / 2);
        let tly = cy.wrapping_sub(def.h / 2);
        // EF:62305-07 — `v18x` is the chain CURSOR: the castle for
        // the first piece, then the piece just made.
        //     i2x->word_0x32_50 = v18x;   // back-link → f52
        //     v18x->word_0x34_52 = i2x;   // forward   → f54
        //     i2x->word_0x34_52 = 0;
        //     v18x = i2x;
        // Only the CASTLE end of the forward write has a port home
        // (`f54`); a piece's `f54` is its `@0x36` windup boost, so
        // the piece→piece direction is rebuilt from `f52` by
        // `mc2_castle_free_stages`. The old `f146` back-link was an
        // invention — `f146` is retail's `target96`, which is 0 on
        // every (10,79) (dump-state t=1055 slot 230: `target96 0 0`)
        // and which the piece brain never reads (its target is f28).
        let mut prev = i;
        for &(ox, oy) in mc2_stage_parts(stage) {
            let px = (
                (tlx.wrapping_add(ox) as u16) << 8,
                (tly.wrapping_add(oy) as u16) << 8,
            );
            let Some(p) = self.mc2_spawn_castle_piece(px.0, px.1, own, stage, part) else {
                break;
            };
            self.ent[p].f52 = prev as u16; // @0x32 back-link
            if prev == i {
                self.ent[i].f54 = p as u16; // @0x34 chain root
            }
            prev = p;
        }
    }

    /// `array_0x24E_590[9 + stage]` — the researched PART-TYPE for
    /// a stage (EF:62274). Retail fills it one stage at a time via
    /// the castle research/production child (`sub_69AB0` EF:56120-21,
    /// sourcing `SPELLS[model].subspell[row].life_0x1A`); the port
    /// stamps at cast/upgrade time from the castle-spell tier
    /// (deliberate). Unstamped stages read 0: no pieces, HP factor
    /// identity — a fresh retail castle's exact state.
    fn mc2_castle_part_type(&self, own: u16, stage: u8) -> u8 {
        if !(1..=7).contains(&stage) {
            return 0;
        }
        self.mc2_castle_research
            .0
            .iter()
            .find(|(o, _, _)| *o == own)
            .map_or(0, |(_, _, part)| part[stage as usize - 1])
    }

    /// `array_0x24E_590[level]` — the researched HP FACTOR for a
    /// level, the other half of [`Self::mc2_castle_part_type`]'s
    /// table. Retail fills it from
    /// `SPELLS[model].subspell[row].subSpellIndex_2` when the research
    /// for `castleLevel + 1` completes (EF:56116-20); the port stamps
    /// it at cast/upgrade time from the castle-spell tier
    /// (`mc2_research_stamp`). Unstamped levels read 0 = identity,
    /// a fresh retail castle's exact state. Level 0 has no rung.
    fn mc2_castle_hp_factor(&self, own: u16, level: u8) -> u8 {
        if !(1..=7).contains(&level) {
            return 0;
        }
        self.mc2_castle_research
            .0
            .iter()
            .find(|(o, _, _)| *o == own)
            .map_or(0, |(_, hp, _)| hp[level as usize - 1])
    }

    /// The `sub_69AB0` research write (EF:56120-21), stamped by the
    /// port at castle cast/upgrade time (deliberate): for `stage`
    /// (retail `v4 = castleLevel+1`), record the castle spell tier's
    /// `subSpellIndex_2` (HP factor — the ladder still runs identity)
    /// and `life_0x1A` (part-type → fire/lightning tower).
    pub(crate) fn mc2_research_stamp(&mut self, own: u16, stage: u8, tier: u8) {
        if own == 0 || !(1..=7).contains(&stage) {
            return;
        }
        let Some(sub) = self
            .assets
            .spells
            .get(2)
            .map(|r| r.tiers[(tier as usize).min(2)])
        else {
            return;
        };
        let hp = sub.sub_spell.clamp(0, 255) as u8;
        let part = sub.life.max(0) as u8;
        let entry = match self
            .mc2_castle_research
            .0
            .iter_mut()
            .find(|(o, _, _)| *o == own)
        {
            Some(e) => e,
            None => {
                self.mc2_castle_research.0.push((own, [0; 7], [0; 7]));
                self.mc2_castle_research.0.last_mut().unwrap()
            }
        };
        entry.1[stage as usize - 1] = hp;
        entry.2[stage as usize - 1] = part;
    }

    /// `sub_508E0_castle_defend_create` (EF:36987): the (10,79)
    /// piece ctor — action 0x56, maxLife 100000, sprite 66,
    /// fontType 1. The level tag (word_0x4A_74) rides f26; the
    /// researched part-type (byte_0x43_67, EF:62310) rides f67.
    fn mc2_spawn_castle_piece(
        &mut self,
        x: u16,
        y: u16,
        own: u16,
        lvl: u8,
        part: u8,
    ) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 79;
            e.tick70 = 0x56;
            e.max_life = 100_000;
            e.id24 = own;
            e.f26 = lvl as i16; // level tag → the height offset
            e.f67 = part; // byte_0x43_67: the defender kind roll's key
            e.f71 = 0; // byte_0x46_70: the defender state machine
            // The brain repurposes two new_event-defaulted fields:
            // f68 (recoil step, retail byte_0x44_68 — default 10
            // would start mid-recoil out of table range) and f44
            // (dwell counter, retail dword_0x10_16 — default 100).
            e.f68 = 0;
            e.f44 = 0;
        }
        let z = self.ground_z(x, y) as i16 + if lvl <= 1 { 384 } else { 224 };
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 66);
        Some(i)
    }

    /// `sub_3AF00_castle_defend_event` (EF:30106) — the (10,79)
    /// piece tick, the full defend brain. Field homes (the piece has
    /// no prior field layout, so these homes are fresh): state
    /// `byte_0x46_70` → f71 · dwell/windup `dword_0x10_16` → f44 ·
    /// fire mode `word_0x2C_44` → f30 · burst `fontTypeIndex_0x3D_61`
    /// → f69 · recoil `byte_0x44_68` (signed −5..6) → f68 · windup
    /// z-boost `word_0x36_54` → f54 · target `word_0x96_150` → f28 ·
    /// firing yaw/pitch `0x1C/0x1E` → f34/f36 · home anchor
    /// `axis_0x9A_154` → dest_x/dest_y/site_z · tick counter
    /// `byte_0x3E_62` → f63. `player` = the human pose (retail scans
    /// the pooled wizard; ours lives outside — None while dead).
    /// Dead or ownerless → despawn (retail's first two gates).
    pub(crate) fn mc2_castle_piece_tick(&mut self, i: usize, player: Option<(u16, u16, i16)>) {
        if self.ent[i].act_life < 0 || self.ent[i].id24 == 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let mut fire = false;
        match self.ent[i].f71 {
            0 => {
                // Latch the axis-home (retail axis_0x9A_154,
                // EF:30182) — the launch arms return here.
                let e = &mut self.ent[i];
                e.dest_x = e.x;
                e.dest_y = e.y;
                e.site_z = e.z;
                e.f71 = 1;
            }
            1 => {
                let d = self.mc2_rand(i);
                self.ent[i].f44 = (d % 0x30 + 16) as u16;
                self.ent[i].f71 = 2;
                // Retail falls through into the first decrement the
                // same tick (LABEL_9, EF:30190-96) — the dwell is
                // seed−1 ticks long, not seed.
                self.ent[i].f44 -= 1;
            }
            2 => {
                self.ent[i].f44 = self.ent[i].f44.saturating_sub(1);
                if self.ent[i].f44 == 0 {
                    self.ent[i].f71 = 3;
                }
            }
            // SCAN (EF:30194-30203 + the LABEL_33 ring walk): every
            // 64 ticks (`byte_0x3E_62 & 0x3F`), sweep rings 3..=12
            // around the piece's tile (`AddE7EE0x_10080(3, 12)` —
            // the sub-3 hole is the castle's own footprint) for the
            // first hostile; latch it and wind up.
            3 => {
                if self.ent[i].f63 & 0x3F == 0
                    && let Some(t) = self.mc2_piece_scan(i, player)
                {
                    self.ent[i].f71 = 4;
                    self.ent[i].f28 = t;
                }
            }
            // WINDUP (EF:30204-21): 4-tick rise, +160 z-boost per
            // counted tick (case 4 falls into the first decrement).
            4 | 5 => {
                if self.ent[i].f71 == 4 {
                    self.ent[i].f71 = 5;
                    self.ent[i].f44 = 4;
                }
                self.ent[i].f44 = self.ent[i].f44.wrapping_sub(1);
                if self.ent[i].f44 != 0 {
                    self.ent[i].f54 = self.ent[i].f54.wrapping_add(160);
                } else {
                    self.ent[i].f71 = 6;
                    self.ent[i].f54 = 0;
                }
            }
            // PICK FIRE MODE (EF:30222-48), then fall through into
            // the first shot the same tick (goto LABEL_48).
            6 => {
                self.ent[i].site_z = self.ent[i].z; // EF:30224
                let r = self.mc2_rand(i) % 100;
                let part = self.ent[i].f67;
                let mode: u16 = if r == 0 {
                    4
                } else if r <= 5 {
                    if part == 1 { 3 } else { 2 }
                } else {
                    u16::from(part != 1)
                };
                self.ent[i].f30 = mode;
                // The burst count: 6 shots for the common modes,
                // 1 for the rare high-tier shot (EF:30244-47).
                self.ent[i].f69 = if mode <= 1 { 6 } else { 1 };
                self.ent[i].f71 = 7;
                fire = true;
            }
            7 | 8 => fire = true,
            // Death arms (set by a future damage router; retail
            // states 9/0xA, EF:30329-37): 0xA drops an owned mana
            // ball, both free the piece.
            9 => {
                self.ent[i].flags |= 0x400;
                return;
            }
            0xA => {
                let (x, y, z, own) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z, e.id24)
                };
                if let Some(s) = self.spawn_mana_ball(x, y, z) {
                    self.ent[s].id24 = own;
                }
                self.ent[i].flags |= 0x400;
                return;
            }
            _ => {}
        }
        if fire {
            self.mc2_piece_fire(i, player);
        }
        // The LABEL_74 tail (EF:30396-30472). Recoil kick first: the
        // piece is displaced back from its home anchor along the
        // latched firing direction, stepping the offset arc
        // 0/115/230/334/368/384 out (1..6) and back (−5..−1).
        let rec = self.ent[i].f68 as i8;
        if rec != 0 {
            let off: i16 = match rec.unsigned_abs() {
                1 => 0,
                2 => 115,
                3 => 230,
                4 => 334,
                5 => 368,
                _ => 384,
            };
            let (mut pos, yaw, pitch) = {
                let e = &self.ent[i];
                ((e.dest_x, e.dest_y, e.site_z), e.f34, e.f36)
            };
            Gen::polar_step(&mut pos, yaw, pitch, -off);
            self.move_relink(i, pos.0, pos.1, pos.2);
            let next = rec.wrapping_add(1);
            self.ent[i].f68 = if next > 6 { (-5i8) as u8 } else { next as u8 };
        }
        // Then the z law: clamp up to the level height, ride the
        // windup boost, or bob idly (±16 correction beyond 32, ±6
        // flicker off the tick counter's bit 3).
        let (x, y, lvl) = {
            let e = &self.ent[i];
            (e.x, e.y, e.f26)
        };
        let want = self.ground_z(x, y) as i16 + if lvl <= 1 { 384 } else { 224 };
        let z = self.ent[i].z;
        if z < want {
            self.ent[i].z = want;
        } else if self.ent[i].f54 != 0 {
            self.ent[i].z = want.wrapping_add(self.ent[i].f54 as i16);
        } else if self.ent[i].f68 == 0 {
            let d = z - want;
            if d.abs() > 32 {
                self.ent[i].z += if d <= 0 { 16 } else { -16 };
            }
            self.ent[i].z += if self.ent[i].f63 & 8 != 0 { 6 } else { -6 };
        }
    }

    /// The state-3 ring scan (EF:30194-30394): first hostile at tile
    /// ring distance 3..=12 from the piece. Hostile predicate
    /// (EF:30359-84): class 3 model {0,1,3} or class 5 model ≠22,
    /// owner ≠ ours. No invisibility test — retail turrets see
    /// through Invisibility (unlike the m15 guards' scan). The
    /// class-5 `StageVar2==14` own-parent exemption (EF:30378-81) is
    /// skipped (deliberate: the stage binding lives in side-vecs;
    /// only shields own summons at own walls).
    ///
    /// ⭐⭐⭐ THE RING IS SEARCH.DAT'S, NOT A CHEBYSHEV BOX, AND THE
    /// WALK IS THE TILE MAP, NOT THE POOL. Retail's `AddE7EE0x_10080(3,
    /// 12)` + `sub_10130` pair is the same ring iterator every disc dig
    /// uses — [`Gen::ring_cells`], whose membership comes from the
    /// 32×32 ring-index image `DATA/SEARCH.DAT` — and each yielded cell
    /// is resolved through `mapEntityIndex_15B4E0` and walked down
    /// `oldMapEntity_0x16_22` (EF:30341-30393). So "ring 12" is a
    /// rasterised DISC ring, and the corners a Chebyshev box would
    /// include are simply not in the table.
    ///
    /// The old stand-in read `max(|dx|, |dy|)` off each pool record's
    /// position and called the admission sets equal. They are not, and
    /// the corner is where they part: mc2l3 t=18551, piece 235 on tile
    /// (82,180) and the wild (5,19) at slot 226 on tile (70,175) —
    /// dx 12, dy 5, Chebyshev 12 (admitted) but Euclidean 13 (NOT in
    /// SEARCH.DAT's ring 12). Retail's turret never leaves state 3; the
    /// port latched a target, wound up, and the piece's graded `z` rose
    /// on the windup boost while retail's kept bobbing.
    ///
    /// ⚠ The walk is FIRST-IN-ORDER, not nearest: ring by ring, cell by
    /// cell within the ring, chain link by chain link within the cell,
    /// stopping at the first hostile (`v39` gates both loop heads).
    /// [`Gen::ring_cells`] already carries retail's off-by-one — the
    /// last entry of the outermost ring is fetched with the stop code
    /// and dropped.
    ///
    /// ⚠ The OUT-OF-POOL HUMAN JOINS NO TILE CHAIN. Retail's carpet is
    /// a pool record linked like any other, so it is found at its own
    /// cell in walk order; ours is checked as that cell comes up,
    /// ahead of whatever the chain there holds. (Only a RIVAL castle's
    /// piece can reach it at all — a piece never targets its own
    /// owner.)
    fn mc2_piece_scan(&self, i: usize, player: Option<(u16, u16, i16)>) -> Option<u16> {
        let (px, py, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24)
        };
        let tx = (px.wrapping_add(128) >> 8) as u8;
        let ty = (py.wrapping_add(128) >> 8) as u8;
        let human = if own == crate::mc1::mobs::PLAYER_TARGET {
            None
        } else {
            player.map(|(hx, hy, _)| {
                (
                    (hx.wrapping_add(128) >> 8) as u8,
                    (hy.wrapping_add(128) >> 8) as u8,
                )
            })
        };
        for (dx, dy) in self.ring_cells(3, 12) {
            let (gx, gy) = (tx.wrapping_add(dx), ty.wrapping_add(dy));
            if human == Some((gx, gy)) {
                return Some(crate::mc1::mobs::PLAYER_TARGET);
            }
            let mut j = self.map_entity[tile(gx, gy)] as usize;
            while j != 0 {
                let e = &self.ent[j];
                let next = e.next20 as usize;
                // ⚠ NO REAP TEST — EF:30355-84 asks class, model and
                // the owner word and nothing else, and a soft-killed
                // record keeps its tile link until the tick-top reaper.
                if j != i && e.id24 != own {
                    let hostile = match e.class64 {
                        3 => e.model65 <= 1 || e.model65 == 3,
                        5 => e.model65 != 22,
                        _ => false,
                    };
                    if hostile {
                        return Some(j as u16);
                    }
                }
                j = next;
            }
        }
        None
    }

    /// The state-7/8 FIRE arm (LABEL_48, EF:30249-30328): validate
    /// the latched target, map the mode to (spell, tier), launch via
    /// the shared `sub_6DCA0` dispatch aimed dead-on (no lead), and
    /// step the burst/recoil bookkeeping.
    fn mc2_piece_fire(&mut self, i: usize, player: Option<(u16, u16, i16)>) {
        let tgt = self.ent[i].f28;
        // Target gone/dead (EF:30253-57) → re-dwell.
        let tpos: Option<(u16, u16, i16)> = if tgt == 0 {
            None
        } else if tgt == crate::mc1::mobs::PLAYER_TARGET {
            player
        } else {
            let e = &self.ent[tgt as usize];
            // ⭐ THE AIM MEASURES THE TARGET AT ITS Z-BOX CENTRE.
            // `sub_655C0` brackets the two angle calls in
            // `sub_65580`/`sub_655A0` (EF:62750-67) — the target's z is
            // temporarily raised by its own `array_0x52_82.yaw` unless
            // it is model 2 — which is exactly [`Ent::aim_z`], already
            // the port's home for that bracket everywhere else.
            // ⚠ RESIDUAL: the human branch above cannot apply it — the
            // out-of-pool carpet arrives as a bare pose with no `f78`.
            (e.act_life >= 0 && e.flags & 0x400 == 0).then_some((e.x, e.y, e.aim_z()))
        };
        let Some((tx, ty, tz)) = tpos else {
            self.ent[i].f28 = 0;
            self.ent[i].f71 = 1;
            return;
        };
        // Mode → (spell, tier) (EF:30258-82).
        let (spell, tier): (usize, usize) = match self.ent[i].f30 {
            0 => (0, 1),
            1 => (7, 0),
            2 => (7, 1),
            3 => (0, 2),
            _ => (9, 0),
        };
        let first = self.ent[i].f71 == 7;
        let mut done = false;
        let sub = self
            .assets
            .spells
            .get(spell)
            .map(|r| r.tiers[tier])
            .unwrap_or_default();
        let spawned =
            crate::engine::world::World::mc2_dispatch_arm(spell, sub.life).and_then(|arm| {
                // Muzzle: the piece's position + its sprite half-height
                // ⭐⭐ THE MUZZLE LIFT HAPPENS AFTER THE AIM, NOT BEFORE.
                // `sub_6DCA0` is handed `&a1x->position_0x4C_76` — the
                // PIECE's own z — and `sub_655C0(proj, target)` runs on
                // the shot as spawned; only on the NEXT line does
                // retail write `proj->position.z += a1x->
                // array_0x52_82.yaw` (EF:30285/30292/30296, in that
                // order). So the turret aims from its own centre and
                // then teleports the shot up to the muzzle; the elevation
                // it fires at is one sprite-half-height too shallow, and
                // that is retail behaviour, not an accident to smooth
                // over.
                //
                // mc2l3 t=18620, turret 235 at z 2218 with `f78` 100
                // shooting the (5,19) at slot 226: retail's shot carries
                // `pitch` 270 (the angle from 2218), the port's 285 (the
                // angle from 2318). One lane, and it is the aim of every
                // castle turret shot in the game.
                let (mx, my, mz) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                let p = self.mc2_spawn_cast_proj(arm.subtype, mx, my, mz)?;
                let own = self.ent[i].id24;
                // `sub_655C0` aim (EF:30292-99): absolute yaw/pitch at
                // the target's current position — no lead.
                let yaw = Gen::angle_between(mx, my, tx, ty);
                let dh = Gen::isqrt(Gen::dist2_sq(mx, my, tx, ty) as u32) as i32;
                let pitch = Gen::pitch_toward(mz, tz, dh);
                {
                    let e = &mut self.ent[p];
                    e.id24 = own;
                    e.f68 = arm.impact.0;
                    e.f69 = arm.impact.1;
                    e.f44 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
                    if arm.charge {
                        e.f71 = sub.life.max(0) as u8;
                    }
                    e.f30 = yaw;
                    e.f32 = pitch;
                    e.f34 = yaw;
                    e.f36 = pitch;
                    // a5 = 0: no caster speed boost, clamp only
                    // (EF:44226-31). NOTE: f40 stays 0 — retail sets no
                    // XP back-ref on turret shots (EF:30288-89 writes
                    // only id + target); turret kills award nothing.
                    e.f126 = e.f126.clamp(384, 0x2000);
                    e.f146 = tgt; // homing target (word_0x96_150)
                }
                // NOW the muzzle lift (EF:30296) — a bare `z +=`, no
                // relink; the tile link is x/y only.
                let lift = self.ent[i].f78 as i16;
                self.ent[p].z = self.ent[p].z.wrapping_add(lift);
                // The local player's castle FIREBALL swaps to the
                // star muzzle sprite 42 (EF:30290-91).
                if own == crate::mc1::mobs::PLAYER_TARGET && spell == 0 {
                    let e = &mut self.ent[p];
                    e.type86 = 42;
                    e.frame88 = 0;
                }
                Some((yaw, pitch))
            });
        if let Some((yaw, pitch)) = spawned {
            // First shot of the burst: the `sub_6DCA0` cast sound
            // (a6 = state==7, EF:44232-33) — fireball 9, lightning
            // t0 23 / t1 9, meteor 15; positioned at the piece
            // (retail keys it to the castle entity).
            if first {
                let v6 = match (spell, tier) {
                    (0, _) => 9u8,
                    (7, 0) => 23,
                    (7, _) => 9,
                    _ => 15,
                };
                self.snd(v6, i);
            }
            // Latch the firing direction for the recoil kick
            // (EF:30295-30302) and step the recoil counter
            // (EF:30301-12; the tail steps it AGAIN — faithful).
            let e = &mut self.ent[i];
            e.f34 = yaw & 0x7FF;
            e.f36 = pitch & 0x7FF;
            let rec = e.f68 as i8;
            e.f68 = if rec == 0 {
                1
            } else {
                rec.wrapping_add(1).min(5)
            } as u8;
            e.f69 = e.f69.wrapping_sub(1);
            if e.f69 == 0 {
                done = true;
            }
        }
        if done {
            self.ent[i].f28 = 0;
            self.ent[i].f71 = 1;
        } else {
            self.ent[i].f71 = 8;
        }
    }
}

/// `x_BYTE_DB038` (EF:2594) — the (10,79) piece offsets per level,
/// decoded (mc2-castle-data-tables.md §1.3): count at `[2*lvl]`,
/// pair-slot index at `[1+2*lvl]`, pairs at `[18..]`. Tile offsets
/// from the footprint's NW corner. L2/3, L4/5 and L6/7 share lists —
/// BUILD00 row 7 is 48×48 like row 6 (the 1×1 rows are 8-16, never a
/// castle level).
const MC2_STAGE_PARTS: [&[(u8, u8)]; 8] = [
    &[],
    &[(4, 4)],
    &[(3, 3), (17, 3), (3, 17), (17, 17)],
    &[(3, 3), (17, 3), (3, 17), (17, 17)],
    &[(3, 3), (31, 3), (3, 31), (31, 31)],
    &[(3, 3), (31, 3), (3, 31), (31, 31)],
    &[
        (3, 3),
        (24, 3),
        (45, 3),
        (3, 24),
        (45, 24),
        (3, 45),
        (24, 45),
        (45, 45),
    ],
    &[
        (3, 3),
        (24, 3),
        (45, 3),
        (3, 24),
        (45, 24),
        (3, 45),
        (24, 45),
        (45, 45),
    ],
];

fn mc2_stage_parts(lvl: u8) -> &'static [(u8, u8)] {
    MC2_STAGE_PARTS[(lvl as usize).min(7)]
}

#[cfg(test)]
mod tests {
    use crate::chassis::ChassisParams;
    use crate::engine::features::{BuildDef, FeatureAssets, Gen, Planes, tile};
    use crate::verbs::VerbSet;

    fn flat_gen() -> Gen {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells: Vec::new(),
            mc2_sprite_ext: Vec::new(),
        };
        Gen::new(planes, assets, 1, ChassisParams::MC2, VerbSet::MC2)
    }

    /// Downgrading a castle whose capacity `f136` was pumped past the
    /// normal ladder (the level-0 over-level bug) must not overflow the
    /// 10% haircut `10 * f136` ("attempt to multiply with overflow").
    #[test]
    fn mc2_castle_downgrade_survives_oversized_capacity() {
        let mut g = flat_gen();
        let i = g.new_event().expect("castle slot");
        {
            let e = &mut g.ent[i];
            e.class64 = 3;
            e.model65 = 2;
            e.f26 = 7; // level 7
            e.f136 = i32::MAX; // capacity pumped past the ladder
            e.f140 = 1_000; // little stored mana → eject is a no-op
            e.id24 = 1;
            e.x = 100 << 8;
            e.y = 100 << 8;
            e.act_life = 1;
        }
        g.link(i, 100 << 8, 100 << 8, g.ground_z(100 << 8, 100 << 8) as i16);
        // Must not panic on `10 * i32::MAX`.
        g.mc2_castle_downgrade(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f26, 6, "one level off, no overflow");
    }

    /// The MC2 balloon altitude servo is `sub_580E0` (EF:40372), a
    /// 2-branch servo: descend by the row's v_14 whenever ABOVE
    /// ground, floor at ground+v_12. Its a4 (= v_10) is DEAD — there
    /// is NO ceiling band and NO 25% intermediate step. Reusing MC1's
    /// 3-branch `alt_clamp` sank the balloon only 25%·v_14 through the
    /// v_12..v_10 band (−4 vs −16 for row 68), the mc2-balloon-z −12
    /// open-sky field residual. Balloon row 68 = v_10 1536 / v_12 512
    /// / v_14 −16.
    #[test]
    fn mc2_balloon_servo_descends_full_v14_in_band() {
        let mut g = flat_gen(); // flat height 100 → ground 3200, surface (no ceiling)
        let ground = g.ground_z(100 << 8, 100 << 8) as i16;
        assert_eq!(ground, 3200, "flat_gen ground datum");
        // A live (10,39) ball NOT ours: the tick takes step=false
        // (skips the move), so the servo alone touches z.
        let ball = g.new_event().expect("ball slot");
        {
            let e = &mut g.ent[ball];
            e.class64 = 10;
            e.model65 = 39;
            e.act_life = 100;
            e.f144 = 999; // not the balloon's owner
        }
        g.link(ball, 120 << 8, 100 << 8, ground);
        // Balloon parked mid-band (ground+512 < z < ground+1536), no
        // pitch so the (skipped) move would not touch z anyway.
        let bal = g.new_event().expect("balloon slot");
        {
            let e = &mut g.ent[bal];
            e.class64 = 3;
            e.model65 = 3;
            e.tick70 = 9;
            e.row156 = 9; // native abs 68 = ROW_BASE + 9
            e.f126 = 48;
            e.f32 = 0; // pitch 0
            e.id24 = 1;
            e.f144 = 1;
            e.act_life = 10000;
            e.max_life = 10000;
            e.f146 = ball as u16;
        }
        let z0 = ground + 1024; // 4224: mid v_12..v_10 band
        g.link(bal, 100 << 8, 100 << 8, z0);
        g.mc2_balloon_tick(bal);
        // 2-branch: z > ground → z += v_14(−16). MC1's alt_clamp
        // would take the 25% band step (−4 → 4220) here.
        assert_eq!(
            g.ent[bal].z,
            z0 - 16,
            "descends the FULL v_14 above ground (2-branch sub_580E0), not the 3-branch 25% step"
        );
    }

    /// flat_gen + a synthetic BUILD00 (rows 0/1 = 3×3, row 2 = 5×5,
    /// all cells inert 0xff/0xff) so the space check and the
    /// destroy path run their full bodies.
    fn castle_gen() -> Gen {
        let mut g = flat_gen();
        g.assets.build_tab = vec![
            BuildDef {
                offset: 0,
                w: 3,
                h: 3,
            },
            BuildDef {
                offset: 0,
                w: 3,
                h: 3,
            },
            BuildDef {
                offset: 0,
                w: 5,
                h: 5,
            },
        ];
        g.assets.build_dat = vec![0xff; 2 * 25];
        g
    }

    fn place_castle(g: &mut Gen, x: u16, y: u16, lvl: i16, own: u16) -> usize {
        let i = g.new_event().expect("castle slot");
        {
            let e = &mut g.ent[i];
            e.class64 = 3;
            e.model65 = 2;
            e.f26 = lvl;
            e.id24 = own;
            e.act_life = 1;
        }
        let z = g.ground_z(x, y) as i16;
        g.link(i, x, y, z);
        g.mc2_castle_extents(i, lvl.clamp(0, 7) as u8);
        i
    }

    /// The "no room" scan reads the class-3 castle list (retail
    /// dword_38519 + model-2 filter, EF:4449-51) — another CASTLE in
    /// the next-level box blocks; a (10,2) prop at the same spot must
    /// NOT.
    #[test]
    fn space_check_blocks_on_castles_not_props() {
        let mut g = castle_gen();
        let a = place_castle(&mut g, 100 << 8, 100 << 8, 1, 1);
        assert!(g.mc2_castle_space_ok(a), "clear ground upgrades fine");
        let b = place_castle(&mut g, 101 << 8, 100 << 8, 1, 2);
        assert!(!g.mc2_castle_space_ok(a), "a rival castle blocks");
        // Swap the blocker to a (10,2) prop: retail never scans it.
        g.ent[b].class64 = 10;
        assert!(g.mc2_castle_space_ok(a), "a (10,2) prop does not block");
    }

    /// Completing a build/upgrade ZEROES the guard-respawn cooldown.
    ///
    /// `BeginOfCastleCreation_5FA70` case 2 (EF:61149-52) runs
    /// `sub_5F890(locEvent, 0)`, whose `a2 == 0` arm ends in
    /// `sub_6D880(a1x)` (EF:61047) — and `a1x` is THE CASTLE. That
    /// helper is `if (@0x2C) { SetSpell_6D5E0(a1x, @0x2C - 1); @0x2C
    /// = 0; }` (EF:58215-24), and on a (3,2) the word it clears is the
    /// guard cooldown (`f46`), not a deferred spell tier. So a castle
    /// that froze its ladder mid-transform re-arms the spawn instead
    /// of resuming the count.
    ///
    /// ⚠ PAIR-BLIND, SO IT GETS A UNIT TEST: `exec_pair` imports
    /// retail's state at the pair start, which already carries the
    /// zeroed `f2c`, so the port spawns the guard correctly with or
    /// without the fix and NO fixture can ever hold this law
    /// (measured: galore t=1129 conforms at the pre-fix rig). The
    /// free run is the only grader — galore 1129 → 1211.
    ///
    /// Non-vacuity: delete the `f46 = 0` in `mc2_castle_build`'s
    /// arm 2 and the second assert fails with `f46 == 13`.
    #[test]
    fn a_completed_build_zeroes_the_guard_cooldown() {
        let mut g = castle_gen();
        let i = place_castle(&mut g, 100 << 8, 100 << 8, 4, 7);
        // Mid-transform: the build machine on arm 2 (the painter has
        // signalled "pass done"), with a cooldown frozen part-way
        // down its 16-pass ladder — galore castle 266 held exactly 13
        // across all 19 ticks of its level-3 → 4 transform.
        g.ent[i].tick70 = 5;
        g.ent[i].f59 = 2;
        g.ent[i].f46 = 13;
        g.mc2_castle_build(i);
        assert_eq!(g.ent[i].tick70, 4, "arm 2 settles back to standing");
        assert_eq!(g.ent[i].f59, 0, "and clears the build sub-state");
        assert_eq!(
            g.ent[i].f46, 0,
            "sub_6D880 clears the castle's own @0x2C on the way out"
        );
        // The freeze half stays faithful: the build machine must NOT
        // decrement it, so a castle that is still transforming keeps
        // whatever it froze at.
        let mut h = castle_gen();
        let j = place_castle(&mut h, 100 << 8, 100 << 8, 4, 7);
        h.ent[j].tick70 = 5;
        h.ent[j].f59 = 4; // wait-for-painter
        h.ent[j].f46 = 13;
        h.ent[j].f63 = 1; // odd → the 32-tick poll gate stays shut
        h.mc2_castle_build(j);
        assert_eq!(h.ent[j].f46, 13, "a transform in progress FREEZES it");
    }

    /// A castle driven to death spills its ENTIRE stored bank as
    /// owner-tagged (10,39) spheres — the eject runs unconditionally
    /// after the downgrade (EF:61228), even when the level-0 death
    /// happened inside it.
    #[test]
    fn castle_death_spills_the_whole_bank() {
        let mut g = castle_gen();
        let i = place_castle(&mut g, 100 << 8, 100 << 8, 1, 7);
        g.ent[i].f136 = 50_000; // capacity
        g.ent[i].f140 = 20_000; // stored bank
        g.mc2_castle_destroy(i, crate::patches::WorldPatches::RETAIL);
        assert_ne!(g.ent[i].flags & 0x400, 0, "level-1 destroy kills");
        let spilled: i32 = (1..g.ent.len())
            .filter(|&j| {
                j != i
                    && g.ent[j].class64 == 10
                    && g.ent[j].model65 == 39
                    && g.ent[j].f144 == 7
                    && g.ent[j].flags & 0x400 == 0
            })
            .map(|j| g.ent[j].f140)
            .sum();
        assert_eq!(spilled, 20_000, "the whole bank rides out as spheres");
    }

    /// A castle-gen with a spells table wired for the turret column:
    /// row 2 (castle) carries the retail part-type law `life_0x1A =
    /// {0,1,2}` by tier; rows 0/7 give the fire/lightning arms a
    /// payload.
    /// A stand-in for SEARCH.DAT's 32×32 ring-index grid: cell (dx, dy)
    /// belongs to ring `round(hypot(dx, dy))`, enumerated row-major
    /// from the same (15, 15) centre [`FeatureAssets::parse`] finds.
    /// `flat_gen`'s one-dummy-cell stub cannot serve the turret scan —
    /// that scan IS a ring walk (see [`Gen::mc2_piece_scan`]), so with
    /// the stub it can never see anything.
    fn ring_grid() -> Vec<Vec<(u8, u8)>> {
        let mut rings = vec![Vec::new(); 32];
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (dx, dy) = (x - 15, y - 15);
                let r = ((dx * dx + dy * dy) as f64).sqrt().round() as usize;
                if r < 32 {
                    rings[r].push(((dx as i8) as u8, (dy as i8) as u8));
                }
            }
        }
        rings
    }

    fn turret_gen() -> Gen {
        let mut g = castle_gen();
        g.assets.rings = ring_grid();
        let mut spells = vec![crate::mc2::spells::Mc2SpellRow::default(); 10];
        spells[0].tiers[1].sub_spell = 555;
        spells[0].tiers[1].life = 1;
        spells[7].tiers[0].sub_spell = 777;
        for t in 0..3 {
            spells[2].tiers[t].life = t as i8; // the part-type source
            spells[2].tiers[t].sub_spell = i32::from(t == 2); // HP factor
        }
        g.assets.spells = spells;
        g
    }

    /// The turret column: a stamped stage-1 research makes the stage
    /// builder spawn the (10,79) ring (1 piece at
    /// stage 1, part-type from the castle spell tier), and the piece
    /// brain scans rings 3..=12, winds up, and fires the part's
    /// projectile — fire tower (part 1) → spell 0 tier 1 fireball,
    /// owner = the castle's wizard, homing the scanned hostile, NO
    /// XP back-ref (sub_3AF00 EF:30284-89).
    #[test]
    fn fire_turret_spawns_and_shoots_the_hostile() {
        let mut g = turret_gen();
        // Unstamped: the walk-down finds nothing, no pieces.
        let c0 = place_castle(&mut g, 50 << 8, 50 << 8, 1, 9);
        g.mc2_castle_stages(c0);
        assert!(
            !(1..g.ent.len()).any(|j| g.ent[j].class64 == 10 && g.ent[j].model65 == 79),
            "no research → no towers (a fresh retail castle)"
        );
        // Stamp tier 1 (fire) for stage 1 and rebuild.
        g.mc2_research_stamp(9, 1, 1);
        g.mc2_castle_stages(c0);
        let p = (1..g.ent.len())
            .find(|&j| {
                g.ent[j].class64 == 10 && g.ent[j].model65 == 79 && g.ent[j].flags & 0x400 == 0
            })
            .expect("stage-1 spawns its one turret");
        assert_eq!(g.ent[p].f67, 1, "part-type = the cast tier's life_0x1A");
        assert_eq!(g.ent[p].f26, 1, "stage tag");
        // A hostile 5 tiles out (ring 3..=12 admits it).
        let h = g.new_event().expect("hostile slot");
        {
            let e = &mut g.ent[h];
            e.class64 = 5;
            e.model65 = 0;
            e.act_life = 100;
        }
        let (hx, hy) = (((50 + 5) << 8) as u16, (50 << 8) as u16);
        let hz = g.ground_z(hx, hy) as i16;
        g.link(h, hx, hy, hz);
        // Drive the piece brain (the world loop owns f63; pin the
        // scan gate open here).
        let mut shot = None;
        for _ in 0..300 {
            g.ent[p].f63 = 0;
            g.mc2_castle_piece_tick(p, None);
            shot = (1..g.ent.len()).find(|&j| g.ent[j].class64 == 9);
            if shot.is_some() {
                break;
            }
        }
        let s = shot.expect("the turret fires within the dwell+windup budget");
        let e = &g.ent[s];
        assert_eq!(e.model65, 0, "fire tower common mode = fireball tier 1");
        assert_eq!(e.id24, 9, "kills attribute to the castle's wizard");
        assert_eq!(e.f146, h as u16, "homing the scanned hostile");
        assert_eq!(e.f44, 555, "the tier payload rides f44");
        assert_eq!(e.f40, 0, "no XP back-ref on turret shots");
        assert_eq!(g.ent[p].f71, 8, "burst continues (6-shot common mode)");
    }

    /// Tier-2 research → part-type 2 → LIGHTNING tower: the common
    /// mode fires spell 7 tier 0 (subtype 9). Also the walk-down law:
    /// a level-3 castle with only stage-1 research shows the STAGE-1
    /// ring (1 piece), not the level's.
    #[test]
    fn lightning_tower_and_the_walkdown_law() {
        let mut g = turret_gen();
        let c = place_castle(&mut g, 50 << 8, 50 << 8, 2, 9);
        g.mc2_research_stamp(9, 1, 2);
        g.mc2_castle_stages(c);
        let pieces: Vec<usize> = (1..g.ent.len())
            .filter(|&j| {
                g.ent[j].class64 == 10 && g.ent[j].model65 == 79 && g.ent[j].flags & 0x400 == 0
            })
            .collect();
        assert_eq!(pieces.len(), 1, "level 2, stage-1 research → stage-1 ring");
        let p = pieces[0];
        assert_eq!(g.ent[p].f67, 2, "lightning part-type");
        let h = g.new_event().expect("hostile");
        {
            let e = &mut g.ent[h];
            e.class64 = 5;
            e.model65 = 0;
            e.act_life = 100;
        }
        let (hx, hy) = (((50 + 4) << 8) as u16, (50 << 8) as u16);
        let hz = g.ground_z(hx, hy) as i16;
        g.link(h, hx, hy, hz);
        let mut shot = None;
        for _ in 0..300 {
            g.ent[p].f63 = 0;
            g.mc2_castle_piece_tick(p, None);
            shot = (1..g.ent.len()).find(|&j| g.ent[j].class64 == 9);
            if shot.is_some() {
                break;
            }
        }
        let s = shot.expect("the lightning tower fires");
        // Common mode (94%) = spell 7 tier 0 → subtype 9; the rare
        // modes are 12 (lightning t1) and 3 (meteor).
        assert!(
            matches!(g.ent[s].model65, 9 | 12 | 3),
            "a lightning-tower arm, got subtype {}",
            g.ent[s].model65
        );
    }

    /// The unstamp finalizer is the gated 3×3 floor smoother
    /// (`SetHeightmapByBuilding_48B90`, EF:32475), not a retile:
    /// floor = integer average of the
    /// natural 3×3 neighbours; any building-material corner sample
    /// (terrain type 6..=0x22) vetoes the cell.
    #[test]
    fn unstamp_smoother_averages_natural_neighbours() {
        let mut g = flat_gen();
        let t = tile(50, 50);
        g.t.height[t] = 200;
        g.mc2_smooth_pad_edge(50, 50);
        assert_eq!(g.t.height[t] as u32, (200 + 8 * 100) / 9, "3×3 average");
        let t2 = tile(60, 60);
        g.t.height[t2] = 200;
        g.t.tile_type[tile(59, 59)] = 0x10; // building material
        g.mc2_smooth_pad_edge(60, 60);
        assert_eq!(g.t.height[t2], 200, "corner sample gate vetoes");
    }

    /// A castle painted on a CAVE level carves the headroom bubble —
    /// ceiling eases to
    /// max(floor, datum)+100 (EF:27871-94) — and the cave seal
    /// invariant `(ceiling > floor) ⟺ bit3 clear` holds over the
    /// footprint; the non-cave bit3 blind-set / bit3→bit7 phase-B
    /// promote must NOT run (EF:27729, 27866-67).
    #[test]
    fn cave_painter_carves_headroom_and_keeps_the_seal() {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: vec![120; 0x10000],
        };
        // Synthetic BUILD00: rows 0/1 share a 3×3 footprint, every
        // cell height-target 40, no paint codes.
        let mut build_dat = Vec::new();
        for _ in 0..9 {
            build_dat.extend_from_slice(&[0xff, 40]);
        }
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: vec![
                BuildDef {
                    offset: 0,
                    w: 3,
                    h: 3,
                },
                BuildDef {
                    offset: 0,
                    w: 3,
                    h: 3,
                },
            ],
            build_dat,
            bldgprm: Vec::new(),
            spells: Vec::new(),
            mc2_sprite_ext: Vec::new(),
        };
        let mut g = Gen::new(planes, assets, 1, ChassisParams::MC2, VerbSet::MC2);
        let i = g.new_event().expect("painter slot");
        {
            let e = &mut g.ent[i];
            e.class64 = 10;
            e.model65 = 42;
            e.f40 = 0; // no parent castle
            e.f71 = 1; // BUILD00 row 1
            e.f59 = 0; // repaint settle (-1)
            e.x = 50 << 8;
            e.y = 50 << 8;
            e.z = 100 << 5; // datum 100
            e.flags &= !2;
        }
        for _ in 0..2000 {
            if g.ent[i].flags & 0x400 != 0 {
                break;
            }
            g.mc2_castle_painter_tick(i);
        }
        assert_ne!(g.ent[i].flags & 0x400, 0, "painter finished");
        for gy in 49..=51u8 {
            for gx in 49..=51u8 {
                let t = tile(gx, gy);
                let (floor, ceil, angle) = (g.t.height[t], g.t.ceiling[t], g.t.angle[t]);
                assert_eq!(floor, 140, "target = c[1] 40 + datum 100");
                assert_eq!(ceil, 240, "headroom = max(floor,datum)+100");
                assert_eq!(angle & 8, 0, "open cell: bit3 clear");
                assert_eq!(angle & 0x80, 0, "no phase-B bit7 promote on caves");
            }
        }
    }

    /// Decode the verbatim `x_BYTE_DB038` bytes (EF:2594) and prove
    /// [`super::MC2_STAGE_PARTS`] matches: count at [2L], pair-slot
    /// index at [1+2L], pairs base at byte 18.
    #[test]
    fn stage_parts_match_the_db038_decode() {
        const DB038: [u8; 52] = [
            0x00, 0x00, 0x01, 0x00, 0x04, 0x01, 0x04, 0x01, 0x04, 0x05, 0x04, 0x05, 0x08, 0x09,
            0x08, 0x09, 0x00, 0x00, 0x04, 0x04, 0x03, 0x03, 0x11, 0x03, 0x03, 0x11, 0x11, 0x11,
            0x03, 0x03, 0x1F, 0x03, 0x03, 0x1F, 0x1F, 0x1F, 0x03, 0x03, 0x18, 0x03, 0x2D, 0x03,
            0x03, 0x18, 0x2D, 0x18, 0x03, 0x2D, 0x18, 0x2D, 0x2D, 0x2D,
        ];
        for lvl in 0..8usize {
            let count = DB038[2 * lvl] as usize;
            let slot = DB038[1 + 2 * lvl] as usize;
            let decoded: Vec<(u8, u8)> = (0..count)
                .map(|p| (DB038[18 + 2 * (slot + p)], DB038[18 + 2 * (slot + p) + 1]))
                .collect();
            assert_eq!(
                super::MC2_STAGE_PARTS[lvl],
                decoded.as_slice(),
                "level {lvl} piece list"
            );
        }
    }

    /// **THE CASTLE LEVEL-UP PRE-CLEAR DOES NOT JUST KILL THE
    /// BUILDINGS IN ITS WAY — IT CUTS THEIR DEGRADATION LINK.**
    /// `sub_11960` writes BOTH `life_0x8 = -1` and
    /// `fontTypeIndex_0x3D_61 = 0` (EF:4410-11) on every class-10
    /// model-45 entity overlapping the NEXT level's footprint. The
    /// second write is what makes the kill permanent: the collapse
    /// that follows branches on the entity's own link
    /// (`RemoveCastleStage_385C0` EF:28090), so a cleared link
    /// demolishes where a live one would rebuild the successor —
    /// forever, under a castle that levels again next pass. Pinned
    /// together with `a_building_collapse_branches_on_its_own_link_
    /// not_the_table` in `engine::world`, which owns the other half.
    /// ⚠ This half was ALREADY correct, so this fixture passes both
    /// before and after that patch — a regression guard, not a fix
    /// witness. The world-side twin is the non-vacuous one.
    #[test]
    fn a_castle_preclear_cuts_the_building_degradation_link() {
        let mut g = flat_gen();
        // One BUILD00 row for the castle's next level; the pre-clear
        // sizes its box from it.
        g.assets.build_tab = vec![
            BuildDef {
                offset: 0,
                w: 3,
                h: 3,
            },
            BuildDef {
                offset: 0,
                w: 3,
                h: 3,
            },
        ];
        let (cx, cy) = (100u16 << 8, 100u16 << 8);
        let c = g.new_event().expect("castle slot");
        {
            let e = &mut g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.f26 = 0; // level 0 → the pre-clear sizes level 1
            e.id24 = 1;
            e.act_life = 1;
            e.f80 = 512;
            e.f82 = 512;
        }
        g.link(c, cx, cy, g.ground_z(cx, cy) as i16);
        // A self-chaining building standing on the castle's next
        // footprint, and a second one well outside it.
        let mk = |g: &mut Gen, x: u16, y: u16| -> usize {
            let b = g.new_event().expect("building slot");
            {
                let e = &mut g.ent[b];
                e.class64 = 10;
                e.model65 = 45;
                e.act_life = 30;
                e.f46 = 7; // a live degradation link
                e.f80 = 256;
                e.f82 = 256;
            }
            g.link(b, x, y, g.ground_z(x, y) as i16);
            b
        };
        let inside = mk(&mut g, cx, cy);
        let outside = mk(&mut g, 140u16 << 8, 140u16 << 8);

        g.mc2_castle_preclear(c);

        assert_eq!(g.ent[inside].act_life, -1, "the overlapping building dies");
        assert_eq!(
            g.ent[inside].f46, 0,
            "and its degradation link is CUT — this is what makes the \
             castle's kill permanent instead of an endless rebuild"
        );
        assert_eq!(
            g.ent[outside].act_life, 30,
            "a building outside the next footprint is untouched"
        );
        assert_eq!(g.ent[outside].f46, 7, "so is its link");
    }
}
