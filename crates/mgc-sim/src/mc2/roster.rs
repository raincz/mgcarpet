//! MC2 class-5 roster, wave A — every non-multipart creature on the
//! shared primitives ([`super::mobs`]). Traces: docs/traces/
//! mc2-class5-*.md; `EF:` cites = remc2 EventsFunctions.cpp. Models
//! here: 2, 9, 12, 14, 15, 16, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28.
//!
//! NOT here: 10 (the doomsday mana pyramid — its scripted sequence
//! leans on untraced helpers) and the multipart family 0, 3, 22, 27
//! (its own subsystem, docs/traces/mc2-multipart-chains.md). Model 15
//! (the castle guard archer) is never authored by any level — its one
//! launch site is the castle's guard respawn (EF:61488).
//!
//! Field-mapping additions over the [`super::mobs`] module doc:
//! `word_0x2C_44`→f44 (when a model reuses the strength slot as a
//! counter the trace says so in place) · `word_0x30_48`→f50 ·
//! `byte_0x46_70` sub-state→f71 · `byte_0x43_67`→f68 ·
//! `byte_0x44_68`→f69 · `manaRegen_0x88_136`→f136 ·
//! `fov_0x22_34`→f36 · byte[2] of struct_byte_0xc→flags bits 16..24.
//!
//! The per-model spawn ordinal (`byte_0x3E_62 = array_0x10[m]++`)
//! comes from [`Gen::mc2_spawn_ord`] and lands in f63. The slice
//! creatures (goat/archer/villager) still use the alloc-slot f63;
//! aligning them is a banked fidelity pass (goldens pinned).
//!
//! DELIBERATE APPROXIMATIONS (all flagged in place too):
//! - Every `+6` state whose body is MISSING from the decompile (m2,
//!   m9, m16, m17, m18, m19, m20 nominal, m21, m23, m25, m26, m28 —
//!   the dispatch would crash in remc2) holds inert; retail can
//!   never reach them (their rows' flee bit is clear).
//! - m18's `sub_253B0` duration table is partially pinned (the trace
//!   lists the formulas, not the (state,sub)→formula map).
//! - m26's human drain uses +14 flat (the human's manaRegen isn't
//!   modeled yet). The %63 spell-hijack is live: the roll mails
//!   [`crate::engine::world::World::mc2_spell_steal`].
//! - m12's footprint-clear/overlap scans (EF:14036-14093) are shaped,
//!   not verbatim. ⚠ NOT because the helpers are untraced — `sub_22640`
//!   is at EF:13906 and `sub_48990` at EF:32301, both fully decompiled
//!   in-tree; the site-jitter half (EF:13991-14024) went verbatim in
//!   2026-08-24e and the ANCHOR PICK (`sub_23020`, EF:14395-99) in
//!   2026-08-24f.

use super::behavior::BEHAVIOR;
use crate::engine::features::Gen;
use crate::mc1::mobs::{MobCtx, PLAYER_TARGET};

const M2_BASE: u8 = 16;
const M9_BASE: u8 = 72;
const M12_BASE: u8 = 96;
const M14_BASE: u8 = 112;
const M15_BASE: u8 = 120;
const M16_BASE: u8 = 128;
const M17_BASE: u8 = 136;
const M18_BASE: u8 = 144;
const M19_BASE: u8 = 152;
const M20_BASE: u8 = 160;
const M21_BASE: u8 = 168;
const M23_BASE: u8 = 184;
const M24_BASE: u8 = 192;
const M25_BASE: u8 = 200;
const M26_BASE: u8 = 208;
const M28_BASE: u8 = 224;

impl Gen {
    // ---- shared bits ---------------------------------------------------------

    /// `D41A0_0.array_0x10[model]++` — the per-model instance
    /// counter every ctor stores into byte_0x3E_62 (f63).
    pub(crate) fn mc2_ord(&mut self, model: usize) -> u8 {
        let o = self.mc2_spawn_ord.0[model];
        self.mc2_spawn_ord.0[model] = o.wrapping_add(1);
        o
    }

    /// The common wake stagger `word_0x1a - ord % word_0x1a + 4`.
    pub(crate) fn mc2_wake_stagger(row: usize, ord: u8) -> i16 {
        let v26 = BEHAVIOR[row].v_26.max(1);
        v26 - (ord as i16 % v26) + 4
    }

    /// The one-draw facing idiom shared by most ctors:
    /// `roll = yaw = (rand & 0x7FF) - 1; pitch = roll`.
    pub(crate) fn mc2_ctor_facing(&mut self, i: usize) {
        let d = self.mc2_rand(i);
        let f = ((d & 0x7FF) as i32 - 1) as u16;
        let e = &mut self.ent[i];
        e.f34 = f;
        e.f30 = f;
        e.f32 = f;
    }

    /// Face a point and sidestep a crowding packmate (the every-4th
    /// tick idiom of the custom attack states).
    fn mc2_aim_avoid(&mut self, i: usize, tx: u16, ty: u16) {
        let e = &self.ent[i];
        self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
        self.mc2_avoid_packmate(i);
    }

    /// Target-is-a-wizard check (class 3 model 0|1; the human counts).
    fn mc2_is_wizard(&self, t: u16) -> bool {
        t == PLAYER_TARGET
            || ((t as usize) < self.ent.len()
                && t != 0
                && self.ent[t as usize].class64 == 3
                && self.ent[t as usize].model65 <= 1)
    }

    /// The bare POINTER test on `word_0x96_150` —
    /// `Entities_EA3E4[target] > Entities_EA3E4[0]`, i.e. "the slot
    /// resolves to something other than the null record". Entity
    /// records are contiguous, so retail's `<=` is exactly `target ==
    /// 0`; the human's own record is a pool record like any other, so
    /// the port's out-of-pool [`PLAYER_TARGET`] passes it.
    ///
    /// ⚠ THIS IS NOT [`Gen::mc2_target`]. That one carries the
    /// life/reap/class guard the CHASE applies (`sub_1C310`
    /// EF:9297-9302), and a state body that re-asks it is re-asking
    /// what its own chase would ask one level down — with a whole
    /// tick of the state machine in between. Several state heads take
    /// only this pointer test and then run arms that WRITE before the
    /// chase ever resolves the target.
    fn mc2_target_ptr(&self, t: u16) -> bool {
        t == PLAYER_TARGET || (t != 0 && (t as usize) < self.ent.len())
    }

    /// `KillEntity_1C930`'s corpse effect is the (10,1) explosion
    /// (id inherited).
    pub(crate) fn mc2_corpse_burst(&mut self, i: usize) {
        let (x, y, z, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        if let Some(b) = self.mc2_spawn_big_explosion(x, y, z) {
            self.ent[b].id24 = id;
        }
    }

    // =========================================================================
    // MODEL 2 — day-only pack hunter (ctor sub_4B590 EF:33751,
    // states 0x10-17, trace: mc2-class5-m2-9-12-14-15.md)
    // =========================================================================

    pub(crate) fn mc2_spawn_m2(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        if self.mc2_night_shade.0 {
            return None; // DAY-ONLY (:33758)
        }
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 2;
            e.tick70 = M2_BASE + 1; // 17
            e.f28 = 1; // cross-column damage contract
            e.f128 = 64;
            e.f130 = 30;
            e.max_life = 3000;
            e.f126 = 32; // minSpeed / 2 (:33771)
        }
        self.mc2_set_mana_half(i); // 1500
        self.mc2_ctor_facing(i);
        let ord = self.mc2_ord(2);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            e.f44 = 200; // melee damage
            e.f66 = 3;
            e.f67 = 0;
            e.f26 = (i % 100) as i16;
            e.f56 = 1; // burnable
            e.row156 = 73;
            e.f63 = ord;
        }
        self.ent[i].f58 = Self::mc2_wake_stagger(73, ord);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 3);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// The wake yelp `(rand & 1) + 12` (:11483 / :11524).
    fn m2_yelp(&mut self, i: usize) {
        let d = self.mc2_rand(i);
        self.snd(((d & 1) + 12) as u8, i);
    }

    pub(crate) fn m2_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M2_BASE {
            0 => {
                self.mc2_patrol(i, M2_BASE);
                if self.ent[i].tick70 == M2_BASE + 2 {
                    self.ent[i].f26 = 1;
                }
            }
            1 => {
                self.mc2_idle(i, M2_BASE, ctx);
                if self.ent[i].tick70 == M2_BASE + 2 {
                    self.m2_yelp(i);
                    self.ent[i].f26 = 1;
                }
            }
            2 => {
                // sub_1F6D0 (:11490): lunge speed on the countdown's
                // last tick, vertical homing, chase w/ 1024-melee.
                if self.ent[i].f26 != 0 {
                    let v2 = self.ent[i].f26;
                    self.ent[i].f26 = v2 - 1;
                    if v2 == 1 {
                        self.ent[i].f126 = 5 * self.ent[i].f128 / 2; // 160
                    }
                }
                if self.ent[i].f146 != 0 {
                    // Vertical homing toward the target's top
                    // (:11509-20); the human's half-height isn't
                    // modeled — its carpet z serves (deliberate).
                    if let Some((_, _, tz)) = self.mc2_target(self.ent[i].f146, ctx) {
                        let top = if self.ent[i].f146 == PLAYER_TARGET {
                            tz
                        } else {
                            let t = &self.ent[self.ent[i].f146 as usize];
                            t.z.wrapping_add(t.f78 as i16)
                        };
                        let v4 = (self.ent[i].z - top).signum();
                        let step = BEHAVIOR[self.ent[i].row156 as usize].v_14;
                        self.ent[i].z = self.ent[i].z.wrapping_add(v4 * step);
                    }
                    if self.mc2_chase_attack(i, M2_BASE, ctx, Self::mc2_atk_melee_1024) {
                        self.m2_yelp(i);
                        self.ent[i].f126 = -self.ent[i].f130; // recoil (:11525)
                        self.ent[i].f26 = 3 * BEHAVIOR[self.ent[i].row156 as usize].v_26;
                    }
                } else {
                    self.ent[i].tick70 = M2_BASE + 1;
                }
                if self.ent[i].tick70 != M2_BASE + 2 {
                    self.ent[i].f126 = self.ent[i].f128;
                }
            }
            3 => {
                self.mc2_pack(i, M2_BASE);
                if self.ent[i].tick70 == M2_BASE + 2 {
                    self.ent[i].f26 = 1;
                }
            }
            4 => self.mc2_prekill(i, M2_BASE),
            5 => self.mc2_kill(i),
            6 => {} // no body in the decompile; unreachable (row 73 flee bit clear)
            _ => {
                // +7 (:11563): the StageVar2 1..=9 wander jiggle never
                // fires for our StageVar2==0 spawns.
                if self.ent[i].tick70 == M2_BASE + 2 {
                    self.ent[i].f26 = 1;
                }
            }
        }
    }

    // =========================================================================
    // MODEL 9 — the hive imp (ctor sub_4BBB0 EF:33912, states
    // 0x48-4F; the most-authored creature in the campaign)
    // =========================================================================

    pub(crate) fn mc2_spawn_m9(&mut self, x: u16, y: u16, _z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 9;
            e.tick70 = M9_BASE; // 72 — spawns into the materialize countdown
            e.f28 = 1;
            e.f128 = 20;
            e.f130 = 0;
            e.max_life = 1000;
            e.f126 = 20;
        }
        self.mc2_set_mana_half(i); // 500
        // ONE draw, modulus 0x832 (NOT the 0x7FF mask — verbatim,
        // :33937).
        let d = self.mc2_rand(i);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            let v5 = ((d % 0x832) as i32 - 1) as u16;
            e.f34 = v5;
            e.f30 = v5;
            e.f32 = v5;
            e.f44 = 500;
            e.f56 = 1;
            e.row156 = 80;
            e.f66 = 3; // xtype_0x41_65 = 3 (EF:33947)
        }
        let ord = self.mc2_ord(9);
        self.ent[i].f63 = ord;
        self.ent[i].f26 = 16; // the materialize countdown (:33948)
        self.ent[i].f58 = Self::mc2_wake_stagger(80, ord);
        let gz = self.ground_z(x, y) as i16;
        self.link(i, x, y, gz); // :33951 ground snap
        self.refill_life(i);
        self.mc2_set_sprite(i, 220);
        self.mc2_shift_rot(i, 128, 128);
        // Blocked-placement despawn (:33955-59).
        if self.mc2_path_blocked(i, (x, y, gz)) {
            self.free_entity(i);
            return None;
        }
        Some(i)
    }

    /// `sub_20EC0` (:12283) — the engage pose: stop, sprite 202,
    /// filter = target's class/model; targeting self resets to idle.
    fn m9_engage_pose(&mut self, i: usize) {
        self.ent[i].f126 = 0;
        self.mc2_set_sprite(i, 202);
        let t = self.ent[i].f146;
        if t == i as u16 {
            self.ent[i].tick70 = M9_BASE + 1;
            return;
        }
        let (c, m) = if t == PLAYER_TARGET {
            (3, 0)
        } else if (t as usize) < self.ent.len() {
            (self.ent[t as usize].class64, self.ent[t as usize].model65)
        } else {
            (3, 0)
        };
        self.ent[i].f66 = c;
        self.ent[i].f67 = m;
    }

    /// `sub_20F20` (:11988) — the walk pose.
    fn m9_walk_pose(&mut self, i: usize) {
        self.ent[i].f126 = self.ent[i].f128;
        self.mc2_set_sprite(i, 201);
        self.ent[i].f66 = 3;
        self.ent[i].f67 = 0xFF;
        self.ent[i].f26 = 50;
        self.ent[i].f71 = 0;
    }

    /// The hive's prey-consumption sweep (:12196-12218 / :12399-415):
    /// bucket = model {4, 12, 13} by `(f63 / v26) % 3`; a victim
    /// within 0x600 is consumed and a NEW (5,9) materializes there.
    fn m9_consume_scan(&mut self, i: usize) {
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let range = (row.v_28 as i32) * (row.v_28 as i32);
        let sel = [4u8, 12, 13][((self.ent[i].f63 as i16 / row.v_26.max(1)) % 3) as usize];
        let (ex, ey, ez) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let mut best: Option<(usize, i32)> = None;
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if c.class64 == 5
                && c.model65 == sel
                && c.act_life >= 0
                && c.flags & 0x400 == 0
                && !matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
            {
                let d2 = Self::dist2_sq(ex, ey, c.x, c.y);
                if d2 <= range && best.is_none_or(|(_, bd)| d2 < bd) {
                    best = Some((j, d2));
                }
            }
        }
        if let Some((j, _)) = best {
            let (vx, vy, vz) = (self.ent[j].x, self.ent[j].y, self.ent[j].z);
            if Self::mc2_dist3((ex, ey, ez), (vx, vy, vz)) <= 0x600 {
                self.ent[j].flags |= 0x400; // consumed
                let _ = self.mc2_spawn_m9(vx, vy, vz); // the hive splits
            }
        }
    }

    /// The awake cone scan of the m9 brain (:12159-93): the walk is
    /// over `dword_38519` — the CLASS-3 chain (wizards, castles,
    /// balloons; no model filter, :12164-68) — NOT the creature
    /// pool. Nearest in range + FOV, invisibility (byte[0] & 0x20)
    /// skipped; the id gate excuses the summoner's own things.
    fn m9_cone_scan(&self, i: usize, ctx: &MobCtx) -> Option<u16> {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let range = (row.v_28 as i32) * (row.v_28 as i32);
        let cone = row.v_30 as u16;
        let (ex, ey, eyaw, my_id) = (e.x, e.y, e.f30, e.id24);
        let mut best: Option<(u16, i32)> = None;
        let mut consider = |tx: u16, ty: u16, slot: u16| {
            let d2 = Self::dist2_sq(ex, ey, tx, ty);
            if d2 > range {
                return;
            }
            if Self::angdist(eyaw, Self::angle_between(ex, ey, tx, ty)) >= cone {
                return;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((slot, d2));
            }
        };
        // ⚠ `pdead` is `dword_38519`'s ENTRY test (EF:39975), which the
        // pool arm below applies itself and the out-of-pool human
        // cannot — see [`Gen::mc2_wizard_scan`].
        if !self.player_invisible && !ctx.pdead {
            consider(ctx.px, ctx.py, PLAYER_TARGET);
        }
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if c.class64 == 3 && c.id24 != my_id && c.act_life >= 0 && c.flags & (0x400 | 0x20) == 0
            {
                consider(c.x, c.y, j as u16);
            }
        }
        best.map(|(s, _)| s)
    }

    pub(crate) fn m9_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M9_BASE {
            0 => {
                // sub_20370 (:11969): the materialize countdown.
                let v = self.ent[i].f26;
                self.ent[i].f26 = v - 1;
                if v != 0 {
                    if v - 1 < 16 && (v - 1) % 2 == 0 {
                        self.ent[i].frame88 = self.ent[i].frame88.saturating_add(1);
                    }
                } else {
                    self.m9_walk_pose(i);
                    self.ent[i].tick70 = M9_BASE + 1;
                    self.ent[i].f26 = 400;
                    self.ent[i].f71 = 0;
                }
            }
            1 => {
                // sub_203D0 (:11998) — the hive brain.
                if self.ent[i].f26 > 0 {
                    self.ent[i].f26 -= 1;
                    if self.ent[i].f26 == 0 {
                        // sub_20F60: grounded/summon posture.
                        self.mc2_set_sprite(i, 201);
                        self.ent[i].f71 = 1;
                    }
                }
                if self.ent[i].f71 != 0 {
                    // sub_20940 (EF:12291) — the GROUNDED variant.
                    //
                    // The damage/death head runs FIRST and
                    // short-circuits, exactly as in the walking arm
                    // (EF:12357-75). Omitting it made a grounded hive
                    // unkillable.
                    match self.mc2_state_head(i) {
                        1 => {
                            self.ent[i].f146 = self.ent[i].f40;
                            self.ent[i].tick70 = M9_BASE + 2; // action 74
                            return;
                        }
                        2 => {
                            self.ent[i].tick70 = M9_BASE + 4; // action 76
                            return;
                        }
                        _ => {}
                    }
                    // EF:12377-84 — the stand-up counts UP toward 0 and
                    // only the tick that READS -1 fires sub_20F80
                    // (EF:12638: f71 = 0, f26 = 400, sprite 201). No
                    // consume sweep runs during it.
                    let v7 = self.ent[i].f26;
                    if v7 < 0 {
                        self.ent[i].f26 = v7 + 1;
                        if v7 == -1 {
                            self.mc2_set_sprite(i, 201);
                            self.ent[i].f71 = 0;
                            self.ent[i].f26 = 400;
                        }
                        return;
                    }
                    // EF:12385-89 — an AWAKE hive arms the 50-tick
                    // stand-up and scans nothing this tick. The player
                    // being near is what stands the hive back up.
                    if self.ent[i].f58 != 0 {
                        self.ent[i].f26 = -50;
                        return;
                    }
                    // Asleep: f26 stays parked at 0, so the hive squats
                    // and feeds in place indefinitely — retail never
                    // walks a hive that no wizard has approached.
                    let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
                    if self.ent[i].f63 as i16 % period == 0 {
                        self.m9_consume_scan(i);
                    }
                    if self.ent[i].tick70 == M9_BASE + 2 {
                        self.m9_engage_pose(i);
                    }
                    return;
                }
                if self.ent[i].f58 != 0 {
                    self.ent[i].f26 = 400;
                }
                match self.mc2_state_head(i) {
                    1 => {
                        self.ent[i].f146 = self.ent[i].f40;
                        self.ent[i].tick70 = M9_BASE + 2;
                    }
                    2 => self.ent[i].tick70 = M9_BASE + 4,
                    _ => {
                        self.mc2_move_core(i);
                        let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
                        if self.ent[i].f63 as i16 % period == 0 {
                            // Prey seek (:12117-48): the nearest
                            // model-2 on the `dword_38519` chain —
                            // the CLASS-3 chain, so the prey is a
                            // CASTLE, not the (5,2) creature. The
                            // skeleton FACES it unconditionally at
                            // ANY distance (:12137 — the map-wide
                            // castle march; on mc2:04 the channel
                            // bank + the move-core retries funnel
                            // the column onto the authored ford
                            // straight past the archer island) and
                            // only CHASES within pitch + v_28 (3D,
                            // :12138-39). The id skip (:12121) also
                            // excuses a wizard-summoned skeleton
                            // from besieging its owner's castle.
                            let (ex, ey, ez) = {
                                let e = &self.ent[i];
                                (e.x, e.y, e.z)
                            };
                            let my_id = self.ent[i].id24;
                            let row = &BEHAVIOR[self.ent[i].row156 as usize];
                            let mut prey: Option<(usize, i32)> = None;
                            for (j, c) in self.ent.iter().enumerate().skip(1) {
                                if c.class64 == 3
                                    && c.model65 == 2
                                    && c.id24 != my_id
                                    && c.act_life >= 0
                                    && c.flags & 0x400 == 0
                                {
                                    let d2 = Self::dist2_sq(ex, ey, c.x, c.y);
                                    if best_d2(&prey, d2) {
                                        prey = Some((j, d2));
                                    }
                                }
                            }
                            // `v41x` — set only when a chase target
                            // was ACQUIRED this tick. A castle found
                            // but out of reach is DISCARDED (:12141)
                            // yet still steers the march, and the
                            // cone/convert scans run in its shadow;
                            // the random turn runs only with NO
                            // castle on the map at all (:12149-56).
                            let mut acquired = false;
                            if let Some((j, _)) = prey {
                                let (tx2, ty2, tz2, pitch) = {
                                    let c = &self.ent[j];
                                    (c.x, c.y, c.z, c.f82)
                                };
                                self.ent[i].f34 = Self::angle_between(ex, ey, tx2, ty2);
                                let reach = (pitch as i32 + row.v_28 as i32).max(0) as u32;
                                if Self::mc2_dist3((ex, ey, ez), (tx2, ty2, tz2)) <= reach {
                                    self.ent[i].f146 = j as u16;
                                    self.ent[i].tick70 = M9_BASE + 2;
                                    acquired = true;
                                }
                            } else {
                                self.mc2_wander_turn(i);
                            }
                            if !acquired {
                                if self.ent[i].f58 != 0 {
                                    if let Some(t) = self.m9_cone_scan(i, ctx) {
                                        self.ent[i].f146 = t;
                                        self.ent[i].tick70 = M9_BASE + 2;
                                        acquired = true;
                                    }
                                }
                                if !acquired {
                                    self.m9_consume_scan(i);
                                }
                            }
                        }
                    }
                }
                if self.ent[i].tick70 == M9_BASE + 2 {
                    self.m9_engage_pose(i);
                }
            }
            2 => {
                // sub_20C50 (:12476) — chase + arrow volley.
                match self.mc2_state_head(i) {
                    1 => self.ent[i].f146 = self.ent[i].f40,
                    2 => self.ent[i].tick70 = M9_BASE + 4,
                    _ => {
                        self.mc2_move_core(i);
                        let slot = self.ent[i].f146;
                        let Some((tx, ty, tz)) = self.mc2_target(slot, ctx) else {
                            self.ent[i].tick70 = M9_BASE + 1;
                            self.m9_walk_pose(i);
                            return;
                        };
                        if self.ent[i].f63 % 10 == 0 {
                            let e = &self.ent[i];
                            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                        }
                        let row = &BEHAVIOR[self.ent[i].row156 as usize];
                        let period = row.v_26.max(1);
                        // A castle target extends the ring by its
                        // PITCH extent (:12551 — array_0x52_82.pitch,
                        // the second of the extent trio).
                        let range = row.v_28 as u32
                            + if slot != PLAYER_TARGET
                                && (slot as usize) < self.ent.len()
                                && self.ent[slot as usize].class64 == 3
                                && self.ent[slot as usize].model65 == 2
                            {
                                self.ent[slot as usize].f82 as u32
                            } else {
                                0
                            };
                        if self.ent[i].f63 as i16 % period == 0 {
                            let e = &self.ent[i];
                            if Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz)) < range {
                                self.mc2_atk_arrow(i, slot, ctx);
                            } else {
                                self.ent[i].tick70 = M9_BASE + 1;
                            }
                        }
                    }
                }
                if self.ent[i].tick70 != M9_BASE + 2 {
                    self.m9_walk_pose(i);
                }
            }
            3 => {
                self.mc2_pack(i, M9_BASE);
                if self.ent[i].tick70 == M9_BASE + 2 {
                    self.m9_engage_pose(i);
                }
            }
            4 => self.mc2_prekill(i, M9_BASE),
            5 => self.mc2_kill(i),
            6 => {} // missing body — unreachable
            _ => {
                if self.ent[i].tick70 == M9_BASE + 2 {
                    self.m9_engage_pose(i);
                }
            }
        }
    }

    // =========================================================================
    // MODEL 12 — the builder (ctor sub_4BDF0 EF:33999, states
    // 0x60-67; completing a building RETIRES it into the villager
    // brain, actionIndex 105)
    // =========================================================================

    pub(crate) fn mc2_spawn_m12(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 12;
            e.tick70 = M12_BASE + 1; // 97
            e.f28 = 1;
            e.f130 = 24;
            e.f126 = 24;
            e.f128 = 54;
            e.max_life = 1000;
        }
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f140 = 0;
            e.f36 = 0;
            e.f44 = 500;
            e.f56 = 1;
            e.row156 = 101;
            e.f58 = 64;
            e.f66 = 3; // xtype_0x41_65 = 3 (EF:34026)
            e.f26 = 2;
        }
        self.ent[i].f63 = self.mc2_ord(12);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 221);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// State head with the townie wanted-timer stamp on wizard
    /// offenses (shared by m12/m14, :14186-94 pattern). Returns the
    /// head code.
    fn mc2_head_wanted(&mut self, i: usize) -> u8 {
        let v = self.mc2_state_head(i);
        if v != 0 {
            let src = if v == 2 {
                self.ent[i].f38
            } else {
                self.ent[i].f40
            };
            if self.mc2_is_wizard(src) {
                self.mc2_arm_wanted(src);
            }
        }
        v
    }

    /// `sub_232C0` (:14474): the GLOBAL-LCG building-template pick —
    /// `rand % 0x3C + 17`, then walk up to 0x4D slots for a
    /// townie-flagged bldgprm row (byte_2 & 2). The walk wraps the
    /// LOW BYTE at 0x4C back to 17 (EF:14489-91) and exhaustion
    /// returns 17, never a failure (EF:14493). Retail accepts on
    /// byte_2 & 2 alone (no extra build_tab gate).
    pub(crate) fn m12_pick_template(&mut self) -> u16 {
        self.rand = self.rand.wrapping_mul(9377).wrapping_add(9439);
        let mut pick = (self.rand % 0x3C + 17) as usize;
        for _ in 0..0x4D {
            if self
                .assets
                .bldgprm
                .get(pick)
                .is_some_and(|p| p.flags & 2 != 0)
            {
                return pick as u16;
            }
            pick = (pick + 1) & 0xFF;
            if pick >= 0x4C {
                pick = 17;
            }
        }
        17
    }

    pub(crate) fn m12_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M12_BASE {
            0 => self.mc2_m12_build(i),
            1 => {
                // sub_22C80 (:14118) — roam.
                self.ent[i].f71 = 0;
                match self.mc2_head_wanted(i) {
                    1 => {
                        self.ent[i].f146 = self.ent[i].f40;
                        self.ent[i].tick70 = M12_BASE + 6;
                    }
                    2 => self.ent[i].tick70 = M12_BASE + 4,
                    _ => {
                        self.mc2_move_core(i);
                        let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                        if self.ent[i].f63 % period == 0 {
                            self.mc2_wander_turn(i);
                            // :14195-99 — the roam counter test reads
                            // the PRE-decrement value and compares
                            // `== 0`, so the villager spends one MORE
                            // period-hit roaming than the post-form
                            // allows (the ctor's 2 buys three hits,
                            // the state re-entries' 5 buys six).
                            let pre = self.ent[i].f26;
                            self.ent[i].f26 = pre - 1;
                            if pre == 0 {
                                self.ent[i].tick70 = M12_BASE + 3;
                                self.ent[i].f26 = 1;
                            }
                        }
                    }
                }
                if self.ent[i].tick70 == M12_BASE + 6 {
                    self.ent[i].f126 = self.ent[i].f128;
                }
            }
            2 => {
                // sub_22E60 (:14216) — walk to the chosen site.
                match self.mc2_head_wanted(i) {
                    1 => {
                        self.ent[i].f146 = self.ent[i].f40;
                        self.ent[i].tick70 = M12_BASE + 6;
                    }
                    2 => self.ent[i].tick70 = M12_BASE + 4,
                    _ => {
                        self.mc2_move_core(i);
                        let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
                        if (self.ent[i].f63 as i16 % period) / 2 == 0 {
                            self.ent[i].f26 -= 1;
                            // :14294 latches the site pointer BEFORE
                            // the decrement, and an out-of-pool handle
                            // lands on `Entities_EA3E4[0]`, the class-0
                            // sentinel — so resolve the slot once, up
                            // front, and let both the liveness test and
                            // the aim below read the same record.
                            let t = self.ent[i].f146 as usize;
                            let t = if t < self.ent.len() { t } else { 0 };
                            let live = t != 0
                                && self.ent[t].class64 != 0
                                && self.ent[t].flags & 0x400 == 0;
                            if self.ent[i].f26 < 0 || !live {
                                self.ent[i].f26 = 5;
                                self.ent[i].tick70 = M12_BASE + 1;
                            }
                            // ⚠ THE RE-AIM AND THE ARRIVAL TEST ARE NOT
                            // IN AN `else`. Retail's give-up arm
                            // (:14296-300) is a BARE `if`: it resets the
                            // counter and flips to 97, then FALLS
                            // THROUGH (:14301-09) to
                            // `roll_0x20_32 = tan2(self, site)` and the
                            // `< 0xA00` arrival test regardless — so the
                            // walk's LAST tick still re-aims, and can
                            // still arrive (96 overrides the 97 flip).
                            // Gating them behind the live arm left a
                            // STALE @0x20 for the roam brain to carry:
                            // mc2l0-spells-galore t=3848 retail re-aimed
                            // 625 -> 617 while the port held 625, the
                            // t=3864 wander turn added the same +219 to
                            // both (836 vs 844), and at t=3891 the
                            // move-core yaw servo (row 101 `v_2` = 22)
                            // clamped onto 844 where retail parked on
                            // 836 — 43 ticks of an UNGRADED lane before
                            // the graded `heading` could see it.
                            let (tx, ty, tz) = {
                                let s = &self.ent[t];
                                (s.x, s.y, s.z)
                            };
                            let e = &self.ent[i];
                            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                            let e = &self.ent[i];
                            if Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz)) < 0xA00 {
                                self.ent[i].tick70 = M12_BASE;
                                self.ent[i].f26 = 0;
                            }
                        }
                    }
                }
                if self.ent[i].tick70 == M12_BASE + 6 {
                    self.ent[i].f126 = self.ent[i].f128;
                }
            }
            3 => {
                // sub_23020 (:14323) — pick the nearest building as
                // the anchor to build near.
                match self.mc2_head_wanted(i) {
                    1 => {
                        self.ent[i].f146 = self.ent[i].f40;
                        self.ent[i].tick70 = M12_BASE + 6;
                    }
                    2 => self.ent[i].tick70 = M12_BASE + 4,
                    _ => {
                        // ⭐⭐ :14395-99 — THE ANCHOR PICK IS A **3-D**
                        // NEAREST OVER THE **BUILDING CHAIN**, NOT A
                        // 2-D POOL SCAN.
                        //
                        // `v9 = sub_583F0_distance_3d(&a1x->position_0x4C_76,
                        //  &jx->position_0x4C_76); if (v9 && v9 < v11)`
                        // — `sub_583F0` (:40421) is
                        // `radix_3d(dy² + dx² + **dz²**)`, a TRUNCATED
                        // isqrt; `v11` starts at unsigned −1 (:14337);
                        // the `v9 &&` drops a coincident record and the
                        // strict `<` resolves equal roundings to the
                        // EARLIER chain entry. The walk is
                        // `jx = dword_38527; jx > Entities_EA3E4[0];
                        // jx = jx->next_0` (:14395) — the TICK-TOP
                        // building roster ([`Gen::bldg_chain`]), which
                        // carries no class/model/flags test of its own
                        // and severs at a slot freed earlier in the
                        // tick.
                        //
                        // ⚠ Retail uses BOTH forms deliberately: the
                        // m14 townie's walk (:14617-25) runs the same
                        // chain with a genuinely 2-D `dx² + dy²`. Only
                        // `sub_23020` is the 3-D one.
                        //
                        // mc2l0-spells-galore t=4360, builder slot 430
                        // at (15254, 50184, 3567), 42 live (10,45)
                        // houses: slot 35 at (16128, 48640, 5152) is
                        // the 2-D nearest (1774 v 1900) and slot 25 at
                        // (16384, 51712, 4992) is the 3-D nearest
                        // (2375 v 2379) — a FOUR-unit margin the
                        // dropped z axis inverts. The port anchored on
                        // 35, so the t=4376 arrival re-aim wrote `roll`
                        // 105 (the bearing to 35) where retail wrote
                        // 862 (the bearing to 25); six ticks later the
                        // roam brain's heading servo (row 101 `v_2` =
                        // 22) stepped −22 against retail's +22 and the
                        // graded `heading` finally showed it at t=4382
                        // — 21 ticks and two ungraded lanes downstream.
                        //
                        // ⭐ MC1's m12 SEEK has carried this exact
                        // helper all along
                        // (`Gen::nearest_building_3d`, mc1/mobs.rs:2431
                        // — 3-axis, truncated isqrt, strict `<`,
                        // `d != 0` skip, its doc comment already
                        // quoting `if (v10 && v10 < v1)`). The TENTH
                        // MC2-is-the-laggard case of the campaign.
                        let (ex, ey, ez) = {
                            let e = &self.ent[i];
                            (e.x, e.y, e.z)
                        };
                        let mut best: Option<(usize, u32)> = None;
                        for c in 0..self.bldg_chain.visible_len() {
                            let j = self.bldg_chain.list[c] as usize;
                            let b = &self.ent[j];
                            let d = Self::mc2_dist3((ex, ey, ez), (b.x, b.y, b.z));
                            if d != 0 && best.is_none_or(|(_, bd)| d < bd) {
                                best = Some((j, d));
                            }
                        }
                        if let Some((j, _)) = best {
                            self.ent[i].f146 = j as u16;
                            self.ent[i].f26 = 10;
                            self.ent[i].tick70 = M12_BASE + 2;
                        } else {
                            self.ent[i].f26 = 5;
                            self.ent[i].tick70 = M12_BASE + 1;
                        }
                    }
                }
                if self.ent[i].tick70 == M12_BASE + 6 {
                    self.ent[i].f126 = self.ent[i].f128;
                }
            }
            4 => self.mc2_prekill(i, M12_BASE),
            5 => {
                if self.ent[i].f38 == PLAYER_TARGET {
                    self.mc2_arm_wanted(PLAYER_TARGET);
                }
                self.mc2_kill(i);
            }
            6 => {
                self.mc2_flee(i, M12_BASE, ctx);
                if self.ent[i].tick70 != M12_BASE + 6 {
                    self.ent[i].f26 = 5;
                    self.ent[i].f146 = 0;
                    self.ent[i].tick70 = M12_BASE + 1;
                    self.ent[i].f126 = self.ent[i].f130;
                }
            }
            _ => {
                // +7 (:14466): respawn straight into the roam brain.
                self.ent[i].f26 = 5;
                self.ent[i].tick70 = M12_BASE + 1;
                self.m12_tick(i, ctx);
            }
        }
    }

    /// `sub_22760` (:13942) — the build-execute state: jitter a
    /// candidate around the anchor building, clear-check, place a
    /// (10,45) and retire into the villager brain. The jitter /
    /// footprint-clear scans are shaped from the trace, not verbatim
    /// (deliberate).
    fn mc2_m12_build(&mut self, i: usize) {
        let t = self.ent[i].f146 as usize;
        let anchor_ok = t != 0
            && t < self.ent.len()
            && self.ent[t].class64 == 10
            && self.ent[t].model65 == 45
            && self.ent[t].flags & 0x400 == 0;
        if !anchor_ok {
            self.ent[i].f26 = 5;
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = M12_BASE + 1;
            return;
        }
        let v2 = self.ent[i].f26;
        self.ent[i].f26 = v2 + 1;
        if v2 >= 4 {
            self.ent[i].f26 = 1;
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = M12_BASE + 1;
            return;
        }
        let pick = self.m12_pick_template();
        let (w, h) = self
            .assets
            .build_tab
            .get(pick as usize)
            .map_or((2u16, 2u16), |d| (d.w as u16, d.h as u16));
        // Candidate: one of four sides of the anchor by pass number,
        // jittered by two draws (cases 1-4, :13991-14024).
        let (ax, ay, az) = {
            let s = &self.ent[t];
            (s.x, s.y, s.z)
        };
        let d1 = ((self.mc2_rand(i) % 3) << 8) as i32;
        let d2 = ((self.mc2_rand(i) % 3) << 8) as i32;
        // ⭐⭐ :13991 SWITCHES ON THE **POST**-INCREMENT PASS COUNTER.
        // `switch (a1x->dword_0x10_16)` runs AFTER
        // `a1x->dword_0x10_16 = v2 + 1` (:13982), so the four passes
        // the `v2 >= 4` give-up budgets (:13983) are sides 1,2,3,4 in
        // that order. Matching the PRE-increment value rotated them to
        // 4,1,2,3 — the second attempt took the EAST face where retail
        // takes the WEST one, and side 4 was never reached at all.
        // (`switch(v2)` cannot be the original: an arrival enters this
        // state with @0x10 = 0, which would fall to `default:` and site
        // the house on the anchor's own centre.)
        //
        // Sides 1/2 straddle X off the anchor's PITCH extent
        // (:13996/:14003); sides 3/4 straddle Y off its ROLL extent
        // (:14013/:14020). And in EVERY case the FIRST draw jitters X
        // and the SECOND jitters Y — the port had d1/d2 swapped on 3/4
        // and read the pitch/width pair there as well.
        //
        // mc2l0-spells-galore t=4282: builder slot 430, @0x10 1 -> 2,
        // anchor (10,45) slot 25 — the port minted a (10,45) at slot
        // 478 off the east candidate while retail's blocked west one
        // left the pool alone (`extra(10,45)slot478x1`).
        let apitch = self.ent[t].f80 as i32;
        let aroll = self.ent[t].f82 as i32;
        // `sub_226D0` (:13933/:13935): the pass extent is the template
        // HALF-size plus a flat 768 (3-tile) clearance —
        // `*exwidth = (width_4 << 8) / 2 + 768`. `(w << 7)` was only
        // the first term, so every candidate sat 3 tiles too close to
        // the anchor. ⚠ Not modelled: :13928 halves both extents when
        // `x_WORD_180660_VGA_type_resolution == 1`.
        let exw = ((w as i32) << 7) + 768;
        let exh = ((h as i32) << 7) + 768;
        let (cx, cy) = match v2 + 1 {
            1 => (
                (ax as i32 + d1 + apitch + exw + 256) as u16,
                (ay as i32 + d2 - 1280) as u16,
            ),
            2 => (
                (ax as i32 - (d1 + apitch + exw + 256)) as u16,
                (ay as i32 + d2 - 1280) as u16,
            ),
            3 => (
                (ax as i32 + d1 - 1280) as u16,
                (ay as i32 + d2 + aroll + exh + 256) as u16,
            ),
            _ => (
                (ax as i32 + d1 - 1280) as u16,
                (ay as i32 - (aroll + exh + d2 + 256)) as u16,
            ),
        };
        // Water veto (:14031-35).
        if self.cap_bit(cx, cy) == 1 {
            self.ent[i].f26 = 2;
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = M12_BASE + 1;
            return;
        }
        // ⭐⭐ THE SITE TEST IS FLATNESS + AN **EXTENT BOX**, NOT A
        // FOOTPRINT-TILE WALK.
        //
        // `sub_22640` (:13906-16) first: the 4-corner max−min of the
        // inflated footprint under `((exw >> 7) + (exh >> 7) > 4) + 15`
        // (:14036-40). Then :14044-53 walks the BUILDING CHAIN
        // (`dword_38527` = `Gen::bldg_chain`) and rejects on
        // `|dx| <= ix.pitch + exwidth && |dy| <= exheight + ix.roll`
        // — each scanned building's OWN apitch/aroll widening the box.
        // Village dwellings carry apitch/aroll of 1536..2560, so the
        // real exclusion reaches ~15 tiles.
        //
        // The port walked only the candidate's own w×h tiles, a ±768
        // box that fires only when a building's single `link` cell
        // lands inside the footprint. mc2l0-spells-galore t=4282: the
        // west candidate (13886, 50944) is NINE TILES from live
        // (10,45) slot 35 at (16128, 48640) (apitch 2304, aroll 2560)
        // — dx 2242 ≤ 3840 and dy 2304 ≤ 3840, a wide margin — so
        // retail vetoes and the port built, minting the extra (10,45)
        // at slot 478 (`extra(10,45)slot478x1`).
        //
        // ⭐ MC1's `m12_build` HAS CARRIED BOTH GATES ALL ALONG
        // (mc1/mobs.rs: the 15/16 threshold and the
        // `dx <= c.f80 + half_x && dy <= c.f82 + half_y` box) — the
        // NINTH "grep MC1 before building for MC2" case.
        //
        // ⚠ Retail's :14056+ adds two more chain scans (class-2
        // model 2, class-2 model 67) and the model-12 `@0x3D` gate at
        // :14086-93; those stay unported and are still registered in
        // docs/DEVIATIONS.md.
        let thr = if (exh >> 7) + (exw >> 7) > 4 { 16 } else { 15 };
        if self.site_roughness(cx, cy, (exw >> 8) as u8, (exh >> 8) as u8) >= thr {
            return;
        }
        for c in 0..self.bldg_chain.visible_len() {
            let b = self.bldg_chain.list[c] as usize;
            let e = &self.ent[b];
            let dx = (e.x.wrapping_sub(cx) as i16 as i32).abs();
            if dx <= e.f80 as i32 + exw {
                let dy = (e.y.wrapping_sub(cy) as i16 as i32).abs();
                if dy <= exh + e.f82 as i32 {
                    return; // occupied — try again next tick
                }
            }
        }
        // Place it (:14096-106).
        if let Some(b) = self.mc2_spawn_building(cx, cy, az, pick) {
            self.snd(10, i);
            self.ent[b].tick70 = 51;
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = 105; // retire into the villager brain
        }
    }

    // =========================================================================
    // MODEL 14 — the trader (ctor AddTrader_4C0B0 EF:34094, states
    // 0x70-77; passive, docks into far-away buildings)
    // =========================================================================

    pub(crate) fn mc2_spawn_m14(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 14;
            e.tick70 = M14_BASE + 1; // 113
            e.f28 = 1;
            e.f130 = 18;
            e.f126 = 18;
            e.f128 = 54;
            e.max_life = 1000;
        }
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f140 = 0;
            e.f36 = 0;
            e.f44 = 500;
            e.f56 = 1;
            e.row156 = 100;
            e.f58 = 64;
            e.f66 = 3; // xtype_0x41_65 = 3 (EF:34117)
            e.f26 = 2;
        }
        self.ent[i].f63 = self.mc2_ord(14);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 219);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    pub(crate) fn m14_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M14_BASE {
            0 | 2 | 3 => {
                self.ent[i].tick70 = M14_BASE + 1;
                self.m14_brain(i, ctx);
            }
            1 => self.m14_brain(i, ctx),
            4 => {
                // :14898 — docked traders vanish instead of dying.
                if self.ent[i].f26 != 0 {
                    self.ent[i].flags |= 0x400;
                } else {
                    self.mc2_prekill(i, M14_BASE);
                }
            }
            5 => {
                if self.ent[i].f38 == PLAYER_TARGET {
                    self.mc2_arm_wanted(PLAYER_TARGET);
                }
                self.mc2_kill(i);
            }
            6 => {
                self.mc2_flee(i, M14_BASE, ctx);
                if self.ent[i].tick70 != M14_BASE + 6 {
                    self.ent[i].f146 = 0;
                    self.ent[i].f126 = self.ent[i].f130;
                }
            }
            _ => {
                if self.ent[i].tick70 == M14_BASE + 6 {
                    self.ent[i].f126 = self.ent[i].f128;
                } else {
                    self.ent[i].f126 = self.ent[i].f130;
                }
            }
        }
    }

    /// `sub_237B0` (:14728) — the trader brain.
    fn m14_brain(&mut self, i: usize, ctx: &MobCtx) {
        match self.mc2_head_wanted(i) {
            1 => {
                self.ent[i].f146 = self.ent[i].f40;
                self.ent[i].tick70 = M14_BASE + 6;
            }
            2 => self.ent[i].tick70 = M14_BASE + 4,
            _ => {
                self.mc2_move_core(i);
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                if self.ent[i].f63 % period == 0 {
                    let t = self.ent[i].f146 as usize;
                    let building = t != 0
                        && t < self.ent.len()
                        && self.ent[t].class64 == 10
                        && self.ent[t].model65 == 45
                        && self.ent[t].flags & 0x400 == 0;
                    if building {
                        let (sp, tp) = {
                            let e = &self.ent[i];
                            let s = &self.ent[t];
                            ((e.x, e.y, e.z), (s.x, s.y, s.z))
                        };
                        if Self::mc2_dist3(sp, tp) > 0x800 {
                            self.ent[i].f34 = Self::angle_between(sp.0, sp.1, tp.0, tp.1);
                        } else if (self.ent[t].f128 as i32) > self.ent[t].f26 as i32 {
                            // Dock (:14820-27).
                            self.ent[i].f26 = 1;
                            self.ent[i].tick70 = M14_BASE + 4;
                            self.ent[t].f26 += 1;
                        } else {
                            self.ent[i].f146 = 0;
                            self.ent[i].f126 = self.ent[i].f130;
                        }
                    } else {
                        self.ent[i].f146 = 0;
                        self.mc2_wander_turn(i);
                        // Seek a FAR trade building (bldgprm byte_2
                        // & 1, dist² > 0xE100000 — :14841-68).
                        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                        let mut best: Option<(usize, i32)> = None;
                        for (j, c) in self.ent.iter().enumerate().skip(1) {
                            if c.class64 == 10
                                && c.model65 == 45
                                && c.flags & 0x400 == 0
                                && self
                                    .assets
                                    .bldgprm
                                    .get(c.f71 as usize)
                                    .is_some_and(|p| p.flags & 1 != 0)
                            {
                                let d2 = Self::dist2_sq(ex, ey, c.x, c.y);
                                // 0xE100000 (~60 tiles, EF:14854).
                                if d2 > 0xE100000 && best_d2(&best, d2) {
                                    best = Some((j, d2));
                                }
                            }
                        }
                        if let Some((j, _)) = best {
                            self.ent[i].f146 = j as u16;
                            self.ent[i].f126 = self.ent[i].f130 + 12;
                        }
                    }
                }
                let _ = ctx;
            }
        }
        if self.ent[i].tick70 == M14_BASE + 6 {
            self.ent[i].f126 = self.ent[i].f128;
        }
    }

    // =========================================================================
    // MODEL 15 — the CASTLE GUARD archer (ctor sub_4C1E0 EF:34129,
    // states 0x78-7F; trace mc2-class5-m2-9-12-14-15.md §MODEL 15).
    // Never authored by any level: its one launch site is the castle
    // guard respawn (EF:61488).
    // =========================================================================

    /// `sub_4C1E0` (:34129) — ZERO ctor RNG draws; rotations
    /// hardcoded 0; mana 0 (no SetEvent144). The retail
    /// `struct_byte[2] |= 2` tracked-entity registration (:34153) has
    /// no reader in our port (sub_57F20's list isn't modeled).
    pub(crate) fn mc2_spawn_m15(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 15;
            e.tick70 = M15_BASE + 1; // actionIndex 121 (:34134)
            // MC2 carries NO per-channel vulnerability mask; admit
            // the physical channel at the seam (cross-column damage
            // contract).
            e.f28 = 1;
            e.f128 = 30; // minSpeed (:34137)
            e.f130 = 0; // maxSpeed (:34138)
            e.max_life = 1000;
            e.f126 = 30; // actSpeed = minSpeed (:34141)
            e.f34 = 0; // yaw = roll = pitch = 0 (:34140-43)
            e.f30 = 0;
            e.f32 = 0;
            e.f140 = 0; // mana (:34144)
            e.f36 = 0; // fov (:34145)
            e.f26 = (i % 100) as i16; // (:34146)
            e.f44 = 500; // subSpellIndex (:34147)
            e.f56 = 1; // byte_0x38_56 (:34148)
            e.row156 = 83; // (:34149)
            e.f66 = 3; // xtype (:34151)
        }
        let ord = self.mc2_ord(15);
        self.ent[i].f63 = ord;
        self.ent[i].f58 = Self::mc2_wake_stagger(83, ord); // (:34152)
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 0);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// `sub_24100` (:15198) — the engage pose: ONE RNG draw picks
    /// stand sprite 206 (draw ≤ 10) or 1; stop.
    fn m15_engage_pose(&mut self, i: usize) {
        let d = self.mc2_rand(i) % 0x14;
        self.ent[i].f126 = 0;
        self.mc2_set_sprite(i, if d <= 10 { 206 } else { 1 });
    }

    /// `sub_24150` (:15214) — the walk pose: full speed, sprite 0.
    fn m15_walk_pose(&mut self, i: usize) {
        self.ent[i].f126 = self.ent[i].f128;
        self.mc2_set_sprite(i, 0);
    }

    /// `sub_24190` (:15221) — the guard's own wander. Every 8th
    /// phase: die where standing is disallowed (`cap & ~v_20`), else
    /// probe the 4 quadrant headings with RNG-weighted scores
    /// (`(rand % w + 2) * unblocked`, weights {0x1B58, 0x1B58, 0xA,
    /// 0x1B58} — the reverse heading is biased against). Every 16th
    /// phase the move candidate snaps to the tile axis. Packmate
    /// separation writes the COMMITTED heading (roll) directly; the
    /// step happens when roll caught up with yaw or on the 55% roll.
    fn m15_wander(&mut self, i: usize) {
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        if self.ent[i].f63 % 8 == 0 {
            let (ex, ey) = (self.ent[i].x, self.ent[i].y);
            if self.cap_bit(ex, ey) & !row.v_20 != 0 {
                self.ent[i].tick70 = M15_BASE + 4; // (:15248-56)
                return;
            }
            const W: [u32; 4] = [0x1B58, 0x1B58, 0x000A, 0x1B58];
            let mut heading = self.ent[i].f30;
            let mut best = 1u16; // v12 init (:15247)
            for w in W {
                let mut pos = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
                Self::polar_step(&mut pos, heading, 0, 256);
                let d = self.mc2_rand(i) % w;
                let score = (d + 2) as u16 * u16::from(!self.mc2_path_blocked(i, pos));
                if score > best {
                    best = score;
                    self.ent[i].f30 = heading;
                }
                heading = (heading + 0x200) & 0x7FF;
            }
        }
        // The move candidate re-seeds from the CURRENT position
        // (:15284): the %16 tile snap keys on the heading quadrant.
        let mut pos = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        if self.ent[i].f63 % 16 == 0 {
            match (self.ent[i].f30.wrapping_sub(256) >> 9) & 3 {
                0 | 2 => pos.1 = (pos.1 >> 8 << 8) + 128,
                _ => pos.0 = (pos.0 >> 8 << 8) + 128,
            }
        }
        // Packmate separation (:15301-11): first same-model neighbor
        // within 256 on both axes — the away bearing lands in ROLL
        // (f34), the wander heading stays in YAW (f30): retail
        // scores/steps yaw_0x1C_28 and writes roll_0x20_32 here
        // (EF:15257-316).
        let (ex, ey, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24)
        };
        for c in self.ent.iter().skip(1) {
            if c.class64 == 5
                && c.model65 == 15
                && c.id24 != id
                && c.act_life >= 0
                && c.flags & 0x400 == 0
                && !matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
                && ((ex.wrapping_sub(c.x)) as i16 as i32).abs() < 256
                && ((ey.wrapping_sub(c.y)) as i16 as i32).abs() < 256
            {
                let away = Self::angle_between(c.x, c.y, ex, ey);
                self.ent[i].f34 = away;
                break;
            }
        }
        if self.ent[i].f30 == self.ent[i].f34 || self.mc2_rand(i) % 0x14 <= 10 {
            let speed = self.ent[i].f126;
            Self::polar_step(&mut pos, self.ent[i].f30, 0, speed);
            self.move_relink(i, pos.0, pos.1, pos.2);
        }
        self.mc2_alt_commit(i);
    }

    /// The brain's acquire scan (:15020-56): nearest CLASS-3 entity
    /// (any model — wizards, castles, balloons; the human counts) in
    /// range + cone off the WANDER heading (yaw, not roll), skipping
    /// invisibles AND same-owner entities — retail gates the walk on
    /// `id_0x1A_26 != own` (:15031), so a CASTLE GUARD never turns
    /// on its own castle/balloons/wizard. A wild archer's id24 is its
    /// own slot, so the gate only drops self-aggro there.
    fn m15_scan(&self, i: usize, ctx: &MobCtx) -> Option<u16> {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let range = (row.v_28 as i32) * (row.v_28 as i32);
        let cone = row.v_30 as u16;
        let (ex, ey, eyaw, own) = (e.x, e.y, e.f30, e.id24);
        let mut best: Option<(u16, i32)> = None;
        let consider = |tx: u16, ty: u16, slot: u16, best: &mut Option<(u16, i32)>| {
            let d2 = Self::dist2_sq(ex, ey, tx, ty);
            if d2 > range {
                return;
            }
            if Self::angdist(eyaw, Self::angle_between(ex, ey, tx, ty)) >= cone {
                return;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                *best = Some((slot, d2));
            }
        };
        if !self.player_invisible && own != PLAYER_TARGET {
            consider(ctx.px, ctx.py, PLAYER_TARGET, &mut best);
        }
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if j != i
                && c.class64 == 3
                && c.id24 != own
                && c.act_life >= 0
                && c.flags & (0x400 | 0x20) == 0
            {
                consider(c.x, c.y, j as u16, &mut best);
            }
        }
        best.map(|(s, _)| s)
    }

    /// `sub_23C40` (:14958) — the idle/scan brain. Clean tick:
    /// wander, then on the row cadence (while the ctor stagger
    /// lives) the class-3 acquire scan. Non-lethal hit: chase a
    /// class-3 source. The engage pose fires whenever the tick ends
    /// in the chase state.
    fn m15_brain(&mut self, i: usize, ctx: &MobCtx) {
        match self.mc2_state_head(i) {
            1 => {
                // (:15060-70) — retarget ONLY on a class-3 source.
                let src = self.ent[i].f40;
                let is_c3 = src == PLAYER_TARGET
                    || ((src as usize) < self.ent.len()
                        && src != 0
                        && self.ent[src as usize].class64 == 3);
                if is_c3 && src != self.ent[i].id24 {
                    self.ent[i].tick70 = M15_BASE + 2;
                    self.ent[i].f146 = src;
                }
                self.mc2_alt_commit(i);
            }
            2 => self.ent[i].tick70 = M15_BASE + 4,
            _ => {
                self.m15_wander(i);
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
                if self.ent[i].f63 as i16 % period == 0
                    && self.ent[i].f58 != 0
                    && let Some(t) = self.m15_scan(i, ctx)
                {
                    self.ent[i].tick70 = M15_BASE + 2;
                    self.ent[i].f146 = t;
                }
            }
        }
        if self.ent[i].tick70 == M15_BASE + 2 {
            self.m15_engage_pose(i);
        }
    }

    /// The (9,13) volley (:15154-66) — the archer's launch minus the
    /// `sub_200F0` overrides: the projectile keeps its template
    /// subSpell (NO f44 write) and inherits xtype/xsubtype from the
    /// GUARD, not the target.
    fn m15_fire(&mut self, i: usize, target: u16, tpos: (u16, u16, i16)) {
        let (x, y, z, own, fov) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f84)
        };
        let Some(a) = self.mc2_spawn_arrow(x, y, z) else {
            return;
        };
        self.ent[a].id24 = own;
        self.ent[a].f30 = Self::angle_between(x, y, tpos.0, tpos.1);
        let dh = Self::isqrt(Self::dist2_sq(x, y, tpos.0, tpos.1) as u32) as i32;
        self.ent[a].f32 = Self::pitch_toward(z, tpos.2, dh);
        let (ax, ay) = (self.ent[a].x, self.ent[a].y);
        let az = self.ent[a].z.wrapping_add((fov / 2) as i16);
        self.move_relink(a, ax, ay, az);
        self.ent[a].f146 = self.ent[i].f146;
        self.ent[a].f66 = self.ent[i].f66;
        self.ent[a].f67 = self.ent[i].f67;
        if target == PLAYER_TARGET {
            self.player_danger = 100; // sub_5EF70
        }
        // No shots++: retail's volley (EF:15154-66) never touches
        // the player stat — the counter is the PLAYER's own.
    }

    /// `sub_23E60` (:15083) — chase/volley. STATIONARY: no move
    /// core; faces the target every 4th tick (the committed heading
    /// directly); NO retarget on a non-lethal hit (unlike the
    /// generic chase core). Out-of-range / dead target → back to the
    /// brain; the walk pose restores on any exit from the state.
    fn m15_chase(&mut self, i: usize, ctx: &MobCtx) {
        match self.mc2_state_head(i) {
            2 => self.ent[i].tick70 = M15_BASE + 4,
            _ => {
                let slot = self.ent[i].f146;
                match self.mc2_target(slot, ctx) {
                    None => self.ent[i].tick70 = M15_BASE + 1, // dead/draw-off (:15138-42)
                    Some((tx, ty, tz)) => {
                        if self.ent[i].f63 & 3 == 0 {
                            let e = &self.ent[i];
                            // ⚠ EF:15135-36 writes `roll_0x20_32` —
                            // the COMMITTED heading (`f34`), NOT yaw.
                            // Aiming `f30` dragged the GRADED `heading`
                            // obs lane with it: 477 unexplained (5,15)
                            // heading rows (mc2l4 466, galore 8,
                            // mc2l30 3), none carried by any ledger
                            // rule. mc2l4 slot 370 is the proof from
                            // the CAPTURE, not the decompile — a
                            // stationary guard in state 122 whose
                            // retail yaw is PINNED at 512 while its
                            // retail roll steps 512 (t=2380) -> 668
                            // (t=2381), against the grader's
                            // `heading: retail 512 port 668`. Same
                            // idiom as `mc2_aim_avoid` (roster.rs:88),
                            // and the docstring above this fn already
                            // says "the committed heading directly".
                            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                        }
                        let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                        let mut left = false;
                        if self.ent[i].f63 % period == 0 {
                            let e = &self.ent[i];
                            let d3 = Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz));
                            if d3 >= BEHAVIOR[self.ent[i].row156 as usize].v_28 as u32 {
                                self.ent[i].tick70 = M15_BASE + 1; // (:15149-53)
                                left = true;
                            } else {
                                self.m15_fire(i, slot, (tx, ty, tz));
                            }
                        }
                        if !left {
                            self.mc2_alt_commit(i); // sub_1EEE0 (:15168)
                        }
                    }
                }
            }
        }
        if self.ent[i].tick70 != M15_BASE + 2 {
            self.m15_walk_pose(i); // LABEL_26 (:15175-76)
        }
    }

    pub(crate) fn m15_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M15_BASE {
            0 => self.mc2_patrol(i, M15_BASE),
            1 => self.m15_brain(i, ctx),
            2 => self.m15_chase(i, ctx),
            3 => self.mc2_pack(i, M15_BASE),
            4 => self.mc2_prekill(i, M15_BASE),
            5 => self.mc2_kill(i),
            // +6 (0x243F0): MISSING from the decompile; unreachable
            // (the row's flee bit is clear). +7 spawn hook sub_1D5D0:
            // a no-op for StageVar2 == 0.
            _ => {}
        }
    }

    // =========================================================================
    // MODEL 16 — the boss (ctor sub_4C310 EF:34163, states 0x80-87;
    // 60000 life, 15-bolt homing bursts, trace mc2-class5-m16-20.md)
    // =========================================================================

    /// `sub_67800`/`sub_51800`→`sub_3A5B0` (EF:59138) — the Summon Army
    /// creature ring. The army SIZE keys off the model: firefly/bee
    /// (19/2) → 8, Cymmerian (25) → 4, wyvern (16) → 2 (weak swarm vs
    /// strong pack). Each node spawns a class-5 creature marked as the
    /// allied controlled-creature (StageVar2/site_z = 13, action `8*M+7`,
    /// owner = caster, 250-tick `f26` life). Radius 512, angle
    /// `k·2048/N` (docs/spell-audit/summon-creatures.md Part B).
    pub(crate) fn mc2_spawn_summon_ring(&mut self, x: u16, y: u16, model: u8, own: u16) {
        let n: u32 = match model {
            25 => 4,
            16 => 2,
            _ => 8, // firefly (19) / bee (2)
        };
        for k in 0..n {
            let ang = ((k * 2048 / n) as u16) & 0x7FF;
            let mut p = (x, y, self.ground_z(x, y) as i16);
            Gen::polar_step(&mut p, ang, 0, 512);
            let gz = self.ground_z(p.0, p.1) as i16;
            let Some(s) = self.mc2_spawn_creature_model(model, p.0, p.1, gz) else {
                continue;
            };
            let e = &mut self.ent[s];
            e.site_z = 13; // StageVar2 = 13 (summon-army allied AI)
            e.tick70 = model.wrapping_mul(8).wrapping_add(7); // action 8*M+7
            e.id24 = own; // caster's team → allied
            e.f26 = 250; // 250-tick lifespan
            e.f146 = 0; // no target yet
        }
    }

    /// Spawn a controlled-creature roster model (the Metamorph / Summon
    /// Army `{2,16,19,25}` ladder) through its normal class-5 ctor. The
    /// caller then overrides the action to `8*M+7` and the StageVar2
    /// marker (site_z) — docs/spell-audit/summon-creatures.md.
    pub(crate) fn mc2_spawn_creature_model(
        &mut self,
        model: u8,
        x: u16,
        y: u16,
        z: i16,
    ) -> Option<usize> {
        match model {
            2 => self.mc2_spawn_m2(x, y, z),
            16 => self.mc2_spawn_m16(x, y, z),
            19 => self.mc2_spawn_m19(x, y, z),
            25 => self.mc2_spawn_m25(x, y, z),
            _ => None,
        }
    }

    pub(crate) fn mc2_spawn_m16(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 16;
            e.tick70 = M16_BASE + 1; // 129
            e.f28 = 1;
            e.f128 = 60;
            e.f130 = 20;
            e.max_life = 60000;
            e.f126 = 60;
        }
        self.mc2_set_mana_half(i);
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            e.f44 = 500;
            e.f56 = 1;
            e.f26 = 0; // :34187 re-zero
            e.row156 = 84;
            e.f58 = 64;
            e.f66 = 3;
        }
        self.ent[i].f63 = self.mc2_ord(16);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 207);
        // :34192-94: array.yaw = 5·word(D9F50+294)/8. D9F50 row 21's
        // word_0 = 0x5DC (1500), and offset 294 is never written at
        // runtime (the table's only writers hit 0x87A/0x5B6/0x126) —
        // so the wyvern's z-box center is the CONSTANT 937.
        self.ent[i].f78 = 937;
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// m16's homing bolt (:15474): a (9,0) with row 61, damage 1600,
    /// mana 50000, z-lift 6·fov.
    fn m16_bolt(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let Some(tpos) = self.mc2_target(target, ctx) else {
            return false;
        };
        let (x, y, z, lift) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, (6 * e.f84) as i16)
        };
        let Some(p) = self.mc2_spawn_bolt(x, y, z.wrapping_add(lift)) else {
            return false;
        };
        self.ent[p].f68 = 10;
        self.ent[p].f69 = 0;
        self.ent[p].row156 = 61;
        self.ent[p].f44 = 1600;
        self.ent[p].f140 = 50000;
        self.mc2_arm_proj(p, i, target, tpos);
        true
    }

    pub(crate) fn m16_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M16_BASE {
            0 => self.mc2_patrol(i, M16_BASE),
            1 => {
                // sub_24440 (:15339): the shared idle PLUS the wide
                // building sweep on the cadence.
                self.mc2_idle(i, M16_BASE, ctx);
                let row = &BEHAVIOR[self.ent[i].row156 as usize];
                let period = (row.v_26 + 1).max(1);
                if self.ent[i].tick70 == M16_BASE + 1 && self.ent[i].f63 as i16 % period == 0 {
                    let range = (row.v_28 as i32) * (row.v_28 as i32);
                    let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                    let mut best: Option<(usize, i32)> = None;
                    for (j, c) in self.ent.iter().enumerate().skip(1) {
                        if c.class64 == 10 && c.model65 == 45 && c.flags & 0x400 == 0 {
                            let d2 = Self::dist2_sq(ex, ey, c.x, c.y);
                            if d2 <= range && best_d2(&best, d2) {
                                best = Some((j, d2));
                            }
                        }
                    }
                    if let Some((j, _)) = best {
                        self.ent[i].tick70 = M16_BASE + 2;
                        self.ent[i].f146 = j as u16;
                    }
                }
            }
            2 => {
                // sub_24510 (:15389) — the burst-attack brain.
                match self.mc2_state_head(i) {
                    1 => self.ent[i].f146 = self.ent[i].f40,
                    2 => self.ent[i].tick70 = M16_BASE + 4,
                    _ => {
                        self.mc2_move_core(i);
                        let slot = self.ent[i].f146;
                        let Some((tx, ty, tz)) = self.mc2_target(slot, ctx) else {
                            self.ent[i].tick70 = M16_BASE + 1;
                            return;
                        };
                        if self.ent[i].f63 & 7 == 0 {
                            let e = &self.ent[i];
                            let far = Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz)) >= 0x200;
                            let is_c3 =
                                slot == PLAYER_TARGET || self.ent[slot as usize].class64 == 3;
                            if is_c3 || far {
                                let e = &self.ent[i];
                                self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                            }
                        }
                        if self.ent[i].f26 > 0 {
                            self.ent[i].f26 -= 1;
                            self.m16_bolt(i, slot, ctx);
                        }
                        let row = &BEHAVIOR[self.ent[i].row156 as usize];
                        let period = row.v_26.max(1);
                        if self.ent[i].f63 as i16 % period == 0 {
                            let e = &self.ent[i];
                            let d2 = Self::dist2_sq(e.x, e.y, tx, ty);
                            let range = (row.v_28 as i32) * (row.v_28 as i32);
                            if d2 < range {
                                if self.ent[i].f63 as i16 % (2 * period) == 0 {
                                    self.snd(39, i);
                                }
                                let e = &self.ent[i];
                                let aim = Self::angle_between(e.x, e.y, tx, ty);
                                if Self::angdist(e.f30, aim) < 0xE3 {
                                    self.ent[i].f26 = 15; // arm the burst
                                    self.mc2_danger_poke(slot);
                                }
                            } else {
                                self.ent[i].tick70 = M16_BASE + 1;
                            }
                        }
                    }
                }
            }
            3 => self.mc2_pack(i, M16_BASE),
            4 => self.mc2_prekill(i, M16_BASE),
            5 => self.mc2_kill(i),
            6 => {} // no handler in the dispatch — unreachable (row 84 flee clear)
            _ => {}
        }
    }

    // =========================================================================
    // MODEL 17 — the dive-bomber (ctor sub_4C460 EF:34201, states
    // 0x88-8F; long-range (9,20) lobs, then a 3x-speed dive on row 87)
    // =========================================================================

    pub(crate) fn mc2_spawn_m17(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 17;
            e.tick70 = M17_BASE + 1; // 137
            e.f28 = 1;
            e.f128 = 68;
            e.f130 = 20;
            e.max_life = 10000;
            e.f126 = 68;
        }
        self.mc2_set_mana_half(i);
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            e.f26 = 0; // :34218/:34226 — set %100 then re-zeroed
            e.f44 = 350;
            e.f56 = 1;
            e.row156 = 85;
            e.f58 = 64;
            e.f66 = 3;
        }
        self.ent[i].f63 = self.mc2_ord(17);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 285);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// The wizard-target validation deviation shared by m17's
    /// wrapper states (:15560+): a non-wizard acquire is dropped.
    fn m17_validate(&mut self, i: usize) {
        if self.ent[i].tick70 == M17_BASE + 2 {
            let t = self.ent[i].f146;
            if !self.mc2_is_wizard(t) {
                self.ent[i].f146 = 0;
            }
            self.ent[i].f71 = 0;
        }
    }

    /// The dive z-curve (:15726-44, VERBATIM): 5 rising ticks
    /// (+192,+96,+48,+24,+12) then a sharp fall (−24,−48,−96,−192,
    /// held at −192).
    fn m17_dive_step(n: i16) -> i16 {
        if n <= 4 {
            192 >> n
        } else {
            let s = (4 - (n - 4)).max(0);
            (-(192 >> s)).max(-192)
        }
    }

    pub(crate) fn m17_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M17_BASE {
            0 => {
                self.mc2_patrol(i, M17_BASE);
                self.m17_validate(i);
            }
            1 => {
                self.mc2_idle(i, M17_BASE, ctx);
                self.m17_validate(i);
            }
            2 => {
                // sub_24930 (:15596) — the dive machine.
                self.snd(58, i); // idle-loop, every tick in state
                match self.mc2_state_head(i) {
                    1 => self.ent[i].f146 = self.ent[i].f40,
                    2 => {
                        self.ent[i].tick70 = M17_BASE + 4;
                        return;
                    }
                    _ => {}
                }
                let v13 = self.mc2_move_core(i);
                let slot = self.ent[i].f146;
                let Some((tx, ty, tz)) = self.mc2_target(slot, ctx) else {
                    self.ent[i].row156 = 85;
                    self.ent[i].f146 = 0;
                    self.ent[i].tick70 = M17_BASE + 1;
                    self.ent[i].f126 = self.ent[i].f128;
                    return;
                };
                if self.ent[i].f63 & 3 == 0 && matches!(self.ent[i].f71, 0 | 4) {
                    self.mc2_aim_avoid(i, tx, ty);
                }
                let row_period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
                match self.ent[i].f71 {
                    0 => {
                        if self.ent[i].f63 as i16 % row_period == 0 {
                            let e = &self.ent[i];
                            let d = Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz));
                            if d >= BEHAVIOR[e.row156 as usize].v_28 as u32 {
                                self.ent[i].tick70 = M17_BASE + 1;
                            } else if d >= 0x700 {
                                self.mc2_atk_lob20(i, slot, ctx);
                            } else {
                                self.ent[i].f71 = 1;
                            }
                        }
                    }
                    1 => {
                        let e = &self.ent[i];
                        let aim = Self::angle_between(e.x, e.y, tx, ty);
                        self.ent[i].f34 = aim;
                        self.ent[i].f30 = aim;
                        self.ent[i].f126 = 3 * self.ent[i].f128; // 204
                        self.ent[i].row156 = 87; // the dive row
                        self.ent[i].f26 = 0;
                        self.ent[i].f71 = 2;
                    }
                    2 | 3 => {
                        if v13 != 3 {
                            self.ent[i].f30 = self.ent[i].f34;
                        }
                        let n = self.ent[i].f26;
                        self.ent[i].f26 = n + 1;
                        let v14 = Self::m17_dive_step(n);
                        self.ent[i].f126 = (self.ent[i].f126 - 8).max(self.ent[i].f130);
                        let (x, y, z) = {
                            let e = &self.ent[i];
                            (e.x, e.y, e.z)
                        };
                        let nz = z.wrapping_add(v14);
                        if nz <= self.ground_z(x, y) as i16 {
                            self.ent[i].f71 = 4;
                            self.ent[i].f26 = 18;
                        } else {
                            self.ent[i].z = nz;
                            if self.ent[i].f71 == 2 && self.mc2_atk_melee_768(i, slot, ctx) {
                                self.ent[i].f71 = 3;
                            }
                        }
                        let _ = tz;
                    }
                    // Leap recovery (EF:15771-88): retail reads the
                    // OLD counter, decrements, then compares the OLD
                    // value — the ground row 85 (v_14=-128) restores
                    // on the FIRST recover tick. Decrement AFTER the
                    // `== 18` compare, or it never fires and the leaper
                    // stays on dive row 87 (v_14=0) running on air.
                    4 => {
                        let v = self.ent[i].f26;
                        self.ent[i].f26 = v - 1;
                        if v != 0 {
                            if v == 18 {
                                self.ent[i].row156 = 85;
                                self.ent[i].f126 = self.ent[i].f130;
                            }
                        } else {
                            self.ent[i].f71 = 0;
                            self.ent[i].f126 = self.ent[i].f128;
                        }
                    }
                    _ => self.ent[i].f71 = 0,
                }
            }
            3 => {
                self.mc2_pack(i, M17_BASE);
                self.m17_validate(i);
            }
            4 => self.mc2_prekill(i, M17_BASE),
            5 => self.mc2_kill(i),
            6 => {} // unreachable
            _ => self.m17_validate(i),
        }
    }

    // =========================================================================
    // MODEL 18 — the slow tank (ctor sub_4C590 EF:34236, states
    // 0x90-97; ground-locked, 5-shot (9,0) fans)
    // =========================================================================

    pub(crate) fn mc2_spawn_m18(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 18;
            e.tick70 = M18_BASE + 3; // 147 — spawns into the pack slot (:34240)
            e.f28 = 1;
            e.f128 = 10;
            e.f130 = 6;
            e.max_life = 36000;
            e.f126 = 10;
        }
        self.mc2_set_mana_half(i);
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            e.f44 = 500;
            e.f56 = 1;
            e.row156 = 86;
            e.f58 = 64;
            e.f66 = 3;
            e.f26 = 100; // :34262
        }
        self.ent[i].f63 = self.mc2_ord(18);
        // The ctor keeps the loader's z (EF:34262 passes the record
        // straight through); the state head snaps to ground on the
        // first TICK.
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 286);
        self.mc2_shift_rot(i, 512, 512);
        Some(i)
    }

    /// `sub_252E0` (:16092): ground pin + state head; death routes to
    /// prekill.
    fn m18_head(&mut self, i: usize) -> u8 {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        let v = self.mc2_state_head(i);
        if v == 2 {
            self.ent[i].tick70 = M18_BASE + 4;
        }
        v
    }

    /// `sub_253B0` (:16155-229): enter (state base+`role`, sub-state
    /// `sub`) with the pinned duration table. ONLY the %-forms draw
    /// the per-entity LCG — the flat forms (0,≥2)/(2,1..3) draw
    /// NOTHING (an unconditional pre-draw would desync every tank's
    /// rand stream).
    pub(crate) fn m18_timer(&mut self, i: usize, role: u8, sub: u8) {
        self.ent[i].tick70 = M18_BASE + role;
        self.ent[i].f71 = sub;
        self.ent[i].f26 = match (role, sub) {
            (0, 0) => {
                let d = self.mc2_rand(i);
                (d % 400 + 400) as i16
            }
            (0, 1) => {
                let d = self.mc2_rand(i);
                (d % 60 + 60) as i16 // :16172-86, v4 = 60
            }
            (0, _) => return, // ≥2: no draw, f26 unchanged (:16187)
            (1, _) => {
                let d = self.mc2_rand(i);
                (d % 0x190 + 400) as i16
            }
            (2, 0) => {
                let d = self.mc2_rand(i);
                (d % 200 + 200) as i16
            }
            (2, 1) => 10, // flat, no draw (:16215)
            (2, 2) => 12, // flat, no draw (:16220)
            (2, 3) => 14, // flat, no draw (:16225)
            _ => return,  // (2,≥4): no draw, f26 unchanged
        };
    }

    /// `sub_254E0` (:16232): turn toward the target by `cap`.
    fn m18_face(&mut self, i: usize, ctx: &MobCtx, cap: i16) {
        if let Some((tx, ty, _)) = self.mc2_target(self.ent[i].f146, ctx) {
            let e = &self.ent[i];
            let aim = Self::angle_between(e.x, e.y, tx, ty);
            self.ent[i].f34 = aim;
            let yaw = self.ent[i].f30;
            self.ent[i].f30 = (yaw as i32 + Self::turn_step(yaw, aim, cap) as i32) as u16 & 0x7FF;
        }
    }

    pub(crate) fn m18_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M18_BASE {
            0 => {
                // sub_24E20 (:15841) — the watch/roam split on f71.
                let r = self.m18_head(i);
                if r == 1 {
                    self.m18_timer(i, 2, 0);
                    return;
                }
                if r != 0 {
                    return;
                }
                if self.ent[i].f71 != 0 {
                    if let Some((tx, ty, _tz)) = self.mc2_target(self.ent[i].f146, ctx) {
                        let e = &self.ent[i];
                        // 2-D: retail's `EuclideanDistXYZ_58490`
                        // (EF:15872) never reads z (2-D despite the
                        // name).
                        let d = crate::mc2::morph::dist2d(e.x, e.y, tx as i32, ty as i32) as u32;
                        if d < BEHAVIOR[e.row156 as usize].v_28 as u32 {
                            self.m18_face(i, ctx, 22); // (4<<11)/360 (EF:15875)
                            let d2 = self.mc2_rand(i);
                            if d2 % 0x31 == 0 {
                                self.m18_timer(i, 2, 0);
                            }
                            return;
                        }
                    }
                    self.ent[i].f146 = 0;
                    self.m18_timer(i, 0, 0);
                } else {
                    self.ent[i].f26 -= 1;
                    // EF:15890-92 — retail tests the post value for `!= 0`.
                    if self.ent[i].f26 != 0 {
                        if self.ent[i].f58 != 0 {
                            let d = self.mc2_rand(i);
                            if d & 1 == 0
                                && let Some(t) = self.mc2_wizard_scan(i, ctx, false)
                            {
                                self.ent[i].f146 = t;
                                self.m18_timer(i, 0, 1);
                            }
                        }
                    } else {
                        self.m18_timer(i, 1, 0);
                    }
                }
            }
            1 => {
                // sub_25050 (:15952) — the walk.
                let r = self.m18_head(i);
                if r == 1 {
                    self.m18_timer(i, 0, 1);
                } else if r == 0 {
                    self.ent[i].f146 = 0;
                    self.mc2_move_core(i);
                    self.ent[i].f26 -= 1;
                    if self.ent[i].f26 <= 0 {
                        self.m18_timer(i, 0, 0);
                    }
                }
            }
            2 => {
                // sub_250B0 (:15976) — the barrage machine.
                let v2 = self.m18_head(i);
                if v2 > 1 {
                    return;
                }
                match self.ent[i].f71 {
                    0 => {
                        self.m18_face(i, ctx, 22); // (4<<11)/360 (EF:15995)
                        if v2 == 1 {
                            self.ent[i].f26 -= 47;
                            if self.ent[i].f26 < 0 {
                                self.m18_timer(i, 2, 1);
                            }
                        } else {
                            let d = self.mc2_rand(i);
                            if d % 0x29 != 0 {
                                self.ent[i].f26 -= 1;
                                if self.ent[i].f26 < 0 {
                                    self.m18_timer(i, 2, 2);
                                }
                            } else {
                                self.m18_timer(i, 2, 1);
                            }
                        }
                    }
                    1 => {
                        self.ent[i].f26 -= 1;
                        if self.ent[i].f26 <= 0 {
                            self.m18_timer(i, 2, 2);
                            return;
                        }
                        let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
                        if self.ent[i].f63 as i16 % period == 0 {
                            let slot = self.ent[i].f146;
                            if self.mc2_target(slot, ctx).is_none() {
                                self.m18_timer(i, 2, 2);
                                return;
                            }
                            self.m18_face(i, ctx, 0x400); // barrage-1 inlines the 0x400 snap (EF:16038)
                            self.mc2_atk_fan(i, slot, ctx);
                        }
                    }
                    2 => {
                        self.ent[i].f26 -= 1;
                        if self.ent[i].f26 <= 0 {
                            self.m18_timer(i, 2, 3);
                        }
                    }
                    // EF:16050-63 — retail's case 3 is EXPLICIT and its
                    // `default:` RETURNS. Nothing seeds a sub-state past
                    // 3 today, so a catch-all was equivalent; keep the
                    // arm literal so a future sub-state cannot silently
                    // inherit the spin-down body.
                    3 => {
                        self.ent[i].f26 -= 1;
                        if self.ent[i].f26 < 0 {
                            self.m18_timer(i, 1, 0);
                        } else if self.ent[i].f26 >= 8 {
                            let yaw = (self.ent[i].f30 + 170) & 0x7FF;
                            self.ent[i].f30 = yaw;
                            self.ent[i].f34 = yaw;
                        }
                    }
                    _ => {} // EF:16065 — `default: return;` (nothing follows either match)
                }
            }
            3 => self.m18_timer(i, 0, 0), // :16074 — re-enter roam
            4 => self.mc2_prekill(i, M18_BASE),
            5 => self.mc2_kill(i),
            6 => {} // unreachable
            _ => {
                // +7 (:16247): ground-lock; a chase entry re-arms.
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                self.ent[i].z = self.ground_z(x, y) as i16;
                if self.ent[i].tick70 == M18_BASE + 2 {
                    self.m18_timer(i, 2, 0);
                }
            }
        }
    }

    // =========================================================================
    // MODEL 19 — the firebug flyer (ctor sub_4C6B0 EF:34271, states
    // 0x98-9F; level-000's final wave — flank, hover, strafe-bolt,
    // dive-melee; flight = handler-driven z writes)
    // =========================================================================

    pub(crate) fn mc2_spawn_m19(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 19;
            e.tick70 = M19_BASE + 1; // 153
            e.f28 = 1;
            e.f128 = 76;
            e.f130 = 8;
            e.max_life = 600;
            e.f126 = 76;
        }
        self.mc2_set_mana_half(i); // 300
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            e.f44 = 300;
            e.f66 = 3;
            e.f67 = 0;
            e.f26 = (i % 100) as i16; // kept (:34290)
            e.f56 = 1;
            e.row156 = 88;
        }
        let ord = self.mc2_ord(19);
        self.ent[i].f63 = ord;
        self.ent[i].f58 = Self::mc2_wake_stagger(88, ord); // :34297 staggered wake
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 287);
        self.mc2_shift_rot(i, 85, 51);
        Some(i)
    }

    fn m19_reset(&mut self, i: usize) {
        if self.ent[i].tick70 == M19_BASE + 2 {
            self.ent[i].f71 = 0;
        }
    }

    /// The flank point 2048 ahead of the target's facing (:16379).
    fn m19_flank(&mut self, i: usize, ctx: &MobCtx, jitter: bool) -> Option<(u16, u16, i16)> {
        let slot = self.ent[i].f146;
        let (tx, ty, tz) = self.mc2_target(slot, ctx)?;
        let tyaw = if slot == PLAYER_TARGET {
            ctx.pyaw
        } else {
            self.ent[slot as usize].f30
        };
        let ang = if jitter {
            let d = self.mc2_rand(i);
            (tyaw as i32 - 256 + (((d % 0x5A) << 11) / 360) as i32) as u16 & 0x7FF
        } else {
            tyaw
        };
        let mut p = (tx, ty, tz);
        Self::polar_step(&mut p, ang, 0, 2048);
        Some(p)
    }

    pub(crate) fn m19_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M19_BASE {
            0 => {
                self.mc2_patrol(i, M19_BASE);
                self.m19_reset(i);
            }
            1 => {
                self.mc2_idle(i, M19_BASE, ctx);
                self.m19_reset(i);
            }
            2 => self.m19_attack(i, ctx),
            3 => {
                self.mc2_pack(i, M19_BASE);
                self.m19_reset(i);
            }
            4 => self.mc2_prekill(i, M19_BASE),
            5 => self.mc2_kill(i),
            6 => {} // unreachable; the hit response is +2 case 7
            _ => self.m19_reset(i),
        }
    }

    /// `HitFirebug_25610` (:16281) — the attack-run machine.
    pub(crate) fn m19_attack(&mut self, i: usize, ctx: &MobCtx) {
        match self.mc2_state_head(i) {
            1 => {
                // The hit arm is a TAIL arm (`else if (v1 <= 1)`,
                // EF:16565-69): latch the dive + retarget the
                // attacker and END the dispatch — no move core, no
                // rand, no dive step until next tick (mc2l3 t=252:
                // the ground-fire hit tick, retail's record freezes
                // everything but life/f71 while the fallen-through
                // port dove same-tick).
                self.ent[i].f71 = 7; // damage → straight into a dive
                self.ent[i].f146 = self.ent[i].f40;
                return;
            }
            2 => {
                self.ent[i].tick70 = M19_BASE + 4;
                return;
            }
            _ => {}
        }
        self.mc2_move_core(i);
        let slot = self.ent[i].f146;
        let Some((tx, ty, tz)) = self.mc2_target(slot, ctx) else {
            self.ent[i].tick70 = M19_BASE + 1;
            self.ent[i].f126 = self.ent[i].f128;
            return;
        };
        let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
        loop {
            match self.ent[i].f71 {
                0 => {
                    self.ent[i].f126 = self.ent[i].f128;
                    self.ent[i].f71 = 1;
                }
                1 => {
                    let Some(p) = self.m19_flank(i, ctx, true) else {
                        break;
                    };
                    let e = &self.ent[i];
                    if Self::mc2_dist3((e.x, e.y, e.z), p) <= 0x500 {
                        // Retail case 1 (EF:16386-88) sets byte_0x46=2 and
                        // RETURNS (:16407) — the flank roll has already fired,
                        // actSpeed is left at minSpeed, and case 2 (which drops
                        // to maxSpeed + rolls again) runs only NEXT tick. A
                        // `continue` here would collapse that two-tick
                        // transition into one, dropping actSpeed to maxSpeed a
                        // tick early and double-rolling rand.
                        self.ent[i].f71 = 2;
                        break;
                    }
                    let e = &self.ent[i];
                    self.ent[i].f34 = Self::angle_between(e.x, e.y, p.0, p.1);
                    if self.ent[i].f63 & 3 == 0 {
                        self.mc2_avoid_packmate(i);
                    }
                    break;
                }
                2 => {
                    self.ent[i].f126 = self.ent[i].f130;
                    let d = self.mc2_rand(i);
                    self.ent[i].f26 = ((d & 0x3FF) as i32 + tz as i32) as i16; // hover altitude
                    self.ent[i].f71 = 3;
                }
                3 => {
                    // Aim runs EVERY tick, BEFORE the gate
                    // (EF:16419); the `f63 & 3` gate covers all of
                    // the avoidance/flank/roll below (EF:16420).
                    {
                        let e = &self.ent[i];
                        self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                    }
                    if self.ent[i].f63 & 3 != 0 {
                        break;
                    }
                    self.mc2_avoid_packmate(i);
                    let Some(p) = self.m19_flank(i, ctx, false) else {
                        break;
                    };
                    let e = &self.ent[i];
                    if Self::mc2_dist3((e.x, e.y, e.z), p) > 0x500 {
                        self.ent[i].f71 = 0;
                        break;
                    }
                    let d = self.mc2_rand(i);
                    let v16 = d % 0x11F;
                    // CASCADING independent ifs (EF:16449-55): a
                    // 0x3F-multiple arms 6, a 0x1F-multiple then
                    // OVERRIDES to 7, zero lands 4; the bob fires on
                    // every multiple of 4 including the above (the
                    // else-if chain made 4 unreachable and 6 final).
                    if v16 & 0x3F == 0 {
                        self.ent[i].f71 = 6;
                    }
                    if v16 & 0x1F == 0 {
                        self.ent[i].f71 = 7;
                    }
                    if v16 == 0 {
                        self.ent[i].f71 = 4;
                    }
                    if v16 & 3 == 0 {
                        // The vertical bob toward the hover altitude —
                        // the flying evidence (:16455-61).
                        let hover = self.ent[i].f26;
                        self.ent[i].z += if self.ent[i].z <= hover { 64 } else { -64 };
                    }
                    break;
                }
                4 => {
                    self.ent[i].f126 = self.ent[i].f128;
                    self.ent[i].f71 = 5;
                }
                5 => {
                    // LABEL_81 (EF:16472-93): the `phase & 3` gate
                    // covers the TARGET AIM and the pack-avoid both —
                    // aim first, pack override second. (Case 3's aim
                    // is the opposite: pre-gate, every tick.)
                    if self.ent[i].f63 & 3 == 0 {
                        {
                            let e = &self.ent[i];
                            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                        }
                        self.mc2_avoid_packmate(i);
                    }
                    if self.ent[i].f63 as i16 % period == 0 {
                        let e = &self.ent[i];
                        if Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz))
                            < BEHAVIOR[e.row156 as usize].v_28 as u32
                        {
                            self.mc2_atk_bolt(i, slot, ctx);
                        } else {
                            self.ent[i].f71 = 6;
                        }
                    }
                    break;
                }
                6 => {
                    self.ent[i].tick70 = M19_BASE + 1;
                    self.ent[i].f126 = self.ent[i].f128;
                    break;
                }
                7 => {
                    let d = self.mc2_rand(i);
                    self.ent[i].f126 = 3 * self.ent[i].f128; // 228
                    self.snd(((d & 1) + 43) as u8, i);
                    self.ent[i].f26 = 24;
                    self.ent[i].f71 = 8;
                }
                _ => {
                    // 8/9 — the dive-melee (:16542-63).
                    self.ent[i].f26 -= 1;
                    // EF:16517-19 — retail tests the post value for `== 0`.
                    if self.ent[i].f26 == 0 {
                        self.ent[i].f71 = 0;
                        break;
                    }
                    // LABEL_59's phase gate (EF:16521) jumps past
                    // BOTH the `v21 > 16` re-aim and the pack-avoid
                    // to LABEL_70 — the dive aims only on phase-0
                    // ticks (mc2l3 t=242: slot 148's roll froze at
                    // 1932 through its 7→8 entry tick, phase 10&3=2,
                    // while the ungated port aim wrote 1936 — the
                    // head standing right behind the castle ball).
                    if self.ent[i].f63 & 3 == 0 {
                        if self.ent[i].f26 > 16 {
                            let e = &self.ent[i];
                            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                        }
                        self.mc2_avoid_packmate(i);
                    }
                    let dz = (tz as i32 - self.ent[i].z as i32).clamp(-64, 64);
                    self.ent[i].z = self.ent[i].z.wrapping_add(dz as i16);
                    if self.ent[i].f71 == 8 && self.mc2_atk_melee_768(i, slot, ctx) {
                        self.ent[i].f71 = 9;
                    }
                    if self.ent[i].f63 as i16 % period == 0 {
                        let e = &self.ent[i];
                        if Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz))
                            >= BEHAVIOR[e.row156 as usize].v_28 as u32
                        {
                            self.ent[i].f71 = 6;
                        }
                    }
                    break;
                }
            }
        }
    }

    // =========================================================================
    // MODEL 20 — dual-mode skirmisher (ctor sub_4C7F0 EF:34307,
    // states 0xA0-A7; (9,21) arcs at range, 1024-melee rushes close)
    // =========================================================================

    pub(crate) fn mc2_spawn_m20(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 20;
            e.tick70 = M20_BASE + 1; // 161
            e.f28 = 1;
            e.f128 = 32;
            e.f130 = 20;
            e.max_life = 5500;
            e.f126 = 32;
        }
        self.mc2_set_mana_half(i);
        {
            // :34320 — fov zeroed BEFORE the draw's facing writes.
            self.ent[i].f36 = 0;
        }
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f44 = 100;
            e.f56 = 1;
            e.row156 = 89;
            e.f58 = 64;
            e.f66 = 3;
        }
        self.ent[i].f63 = self.mc2_ord(20);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 288);
        self.mc2_shift_rot(i, 384, 512);
        Some(i)
    }

    /// The state-2 lock validator — retail's `sub_25D80` (patrol,
    /// EF:16619) and `sub_25DE0` (idle, EF:16637) tails, VERBATIM: if
    /// the wrapped state body left us at action 162, re-read
    /// `word_0x96_150` and clear it unless the record is class 3 and
    /// model 0 or 1, then zero `byte_0x46_70`.
    ///
    /// ⚠ THIS WAS LABELLED "the wizard-validation wrapper DEVIATION"
    /// for months and it is nothing of the kind — it is the decompile
    /// line for line. The mislabel survived because the scan it
    /// guards had an invented `model65 <= 1` filter, which made the
    /// clear unreachable and left the wrapper looking like an
    /// unmotivated port-side extra. Both halves of that were wrong at
    /// once: retail's `sub_1BF90` sweep has NO model test precisely
    /// because this wrapper does it afterwards, and the difference is
    /// live — the scan is nearest-in-cone, so a castle it must be
    /// allowed to WIN is a castle that stops a farther wizard from
    /// winning (mc2l3 t=10222, [`Gen::mc2_wizard_scan`]).
    ///
    /// *When a guard looks unmotivated, check what its caller was
    /// prevented from producing before calling the guard the deviation.*
    fn m20_validate(&mut self, i: usize) {
        if self.ent[i].tick70 == M20_BASE + 2 {
            let t = self.ent[i].f146;
            if !self.mc2_is_wizard(t) {
                self.ent[i].f146 = 0;
            }
            self.ent[i].f71 = 0;
        }
    }

    pub(crate) fn m20_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M20_BASE {
            0 => {
                self.mc2_patrol(i, M20_BASE);
                self.m20_validate(i);
            }
            1 => {
                self.mc2_idle(i, M20_BASE, ctx);
                self.m20_validate(i);
            }
            2 => {
                // sub_25E40 (:16649).
                //
                // ⭐⭐⭐ THE HEAD TEST IS A BARE POINTER TEST (:16661-66)
                // — `Entities_EA3E4[word_0x96_150] <= Entities_EA3E4[0]`
                // and nothing else. NO life test, NO reap test, NO
                // class test. The port asked `mc2_target`, which
                // carries all three, and so dropped out of state 162
                // on a target the chase would still have honoured.
                //
                // The life/reap guard is REAL but it lives one level
                // down, in `sub_1C310`'s QUIET arm (EF:9297-9302,
                // `mc2_chase_attack`) — reached only when the
                // creature took no damage this tick. So a m20 that is
                // itself being hit never re-tests its target at all,
                // and the arms below have already committed by then.
                //
                // mc2l3 t=11731: the player is dead (life −720) and
                // this m20 at slot 161 takes 160 damage on the same
                // tick. Retail's `byte_0x46_70` 1 → 2 arm fires
                // regardless — `dword_0x10_16` = 32, `actSpeed` =
                // 2*minSpeed = 64 — then `sub_1C310` takes its
                // damaged path and never looks at the target. The
                // port bailed to 161 with speed 32, and left the 160
                // damage sitting unconsumed in the inbox.
                if !self.mc2_target_ptr(self.ent[i].f146) {
                    self.ent[i].tick70 = M20_BASE + 1;
                    self.ent[i].f126 = self.ent[i].f128;
                    return;
                }
                self.snd(32, i);
                match self.ent[i].f71 {
                    0 => {
                        // Approach + (9,21) arcs. ⭐⭐ THE COMMIT TEST
                        // IS TARGET-KEYED AND AGAINST A WIZARD IT
                        // DOES NOT READ THE ATTACK AT ALL (EF:16673-
                        // 79): the chase runs either way, but
                        //   v4 = (target is class-3 model-0)
                        //        ? (target.mobilizeCounter == 0)
                        //        : (attack == 0);
                        //   if (!v4) byte_0x46_70 = 1;
                        // — so a m20 only commits its melee rush on a
                        // PARALYZED human, never on a landed lob. The
                        // old note here called the counter "MC2 flight
                        // state, not modeled"; `Mc2Ext::mobilize` has
                        // modeled it all along and `Gen::mc2_mobilize`
                        // is the pool-side mirror. mc2l3 t=1294: the
                        // port's lob landed and set f71 = 1, retail
                        // held 0 through the whole window, and the
                        // rush doubled slot 145's speed to 64 at 1295.
                        let hit = self.mc2_chase_attack(i, M20_BASE, ctx, Self::mc2_atk_lob21);
                        let t = self.ent[i].f146;
                        let wizard = t == PLAYER_TARGET
                            || (t != 0
                                && (t as usize) < self.ent.len()
                                && self.ent[t as usize].class64 == 3
                                && self.ent[t as usize].model65 == 0);
                        let commit = if wizard {
                            self.mc2_mobilize.0 != 0
                        } else {
                            hit
                        };
                        if commit {
                            self.ent[i].f71 = 1;
                        }
                    }
                    1 => {
                        self.ent[i].f71 = 2;
                        self.ent[i].f26 = 32;
                        self.ent[i].f126 = 2 * self.ent[i].f128; // 64
                        let hit = self.mc2_chase_attack(i, M20_BASE, ctx, Self::mc2_atk_melee_1024);
                        self.ent[i].f26 -= 1;
                        // EF:16699 — retail is `if (!(--f26))`, an exact `== 0`.
                        if hit || self.ent[i].f26 == 0 {
                            self.ent[i].f71 = 0;
                            self.ent[i].f126 = self.ent[i].f128;
                        }
                    }
                    _ => {
                        let hit = self.mc2_chase_attack(i, M20_BASE, ctx, Self::mc2_atk_melee_1024);
                        self.ent[i].f26 -= 1;
                        // EF:16699 — retail is `if (!(--f26))`, an exact `== 0`.
                        if hit || self.ent[i].f26 == 0 {
                            self.ent[i].f71 = 0;
                            self.ent[i].f126 = self.ent[i].f128;
                        }
                    }
                }
                if self.ent[i].tick70 != M20_BASE + 2 {
                    self.ent[i].f126 = self.ent[i].f128;
                }
            }
            3 => {
                self.mc2_pack(i, M20_BASE);
                self.m20_validate(i);
            }
            4 => self.mc2_prekill(i, M20_BASE),
            5 => self.mc2_kill(i),
            6 => {} // unreachable
            _ => self.m20_validate(i),
        }
    }

    // =========================================================================
    // MODEL 21 — the DEVIL, a frog-jumping caster (ctor sub_4C8F0
    // EF:34340, states 0xA8-AF; the sub_265A0 jump cycle + (9,0)
    // bolts; the third most-authored creature; trace
    // docs/traces/mc2-m21-jump-m26-steal.md §A)
    // =========================================================================

    pub(crate) fn mc2_spawn_m21(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 21;
            // actSpeed = maxSpeed READ BEFORE ANY WRITE — NewEvent's
            // zero, verbatim bug-compatible (:34345).
            e.f126 = e.f130;
            e.tick70 = M21_BASE + 1; // 169
            e.f28 = 1;
            e.f128 = 96;
            e.max_life = 1000;
            e.f140 = 1000;
            e.f36 = 0;
        }
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            // Retail's `subSpellIndex_0x2A_42 = 400` has no port home
            // (the bolt thunk hard-sets 500); f44 is the jump impulse
            // `word_0x2C_44`, ctor'd 0 (:34367).
            e.f44 = 0;
            e.f56 = 1;
            e.row156 = 96;
            e.f58 = 64;
            e.f66 = 3;
            e.f71 = 0; // byte_0x46_70 — jump state: landed rest
            e.f26 = 0; // byte_0x44_68 — rest countdown (:34368)
            e.f68 = 64; // byte_0x43_67 — rest base (sub_268F0(1) post-ctor)
        }
        self.ent[i].f63 = self.mc2_ord(21);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.m21_pose(i);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// `sub_26500` (:16970): sprite by jump-cycle state.
    fn m21_pose(&mut self, i: usize) {
        let sprite = match self.ent[i].f71 {
            0 => 311,
            1..=3 | 9 => 308,
            4 => 309,
            5 => 310,
            6 => 305,
            7 => 306,
            8 => 307,
            _ => 312,
        };
        if self.ent[i].type86 != sprite {
            self.mc2_set_sprite(i, sprite);
            self.mc2_shift_rot(i, 128, 128);
        }
    }

    /// `sub_268F0` (:17212): mode switch — 1 idle (can-turn 64,
    /// target cleared), 2 attack.
    fn m21_mode(&mut self, i: usize, mode: u8) {
        if mode == 1 {
            self.ent[i].f68 = 64;
            self.ent[i].f146 = 0;
        } else {
            self.ent[i].f68 = 0;
        }
        self.ent[i].tick70 = M21_BASE + mode;
    }

    /// `sub_26930` (:17234-44): yaw may commit only at the landing
    /// tick (state 9), or — for a wading devil — on its aligned
    /// ticks (`!(f63 & 7)`). Direction commits at landing.
    fn m21_can_turn(&self, i: usize) -> bool {
        let s = self.ent[i].f71;
        s == 9 || (s == 10 && self.ent[i].f63 & 7 == 0)
    }

    /// `sub_265A0` (:17010-151) — the frog-jump cycle, VERBATIM.
    /// Field homes: f71 = `byte_0x46_70` jump state,
    /// f44 = `word_0x2C_44` SIGNED impulse, f26 = `byte_0x44_68`
    /// rest countdown, f68 = `byte_0x43_67` rest base (64 idle / 0
    /// attack via [`Self::m21_mode`]). All draws on the ENTITY LCG;
    /// state 9 draws the cackle roll always, the rest roll only when
    /// the base is nonzero (the div-by-zero special case — attack
    /// rests 1 tick on a single draw). The XY veto (`v13` clear →
    /// retail `byte[1] |= 8`) is F_STOP, consumed by the NEXT tick's
    /// walker — both handlers call the walker first; the one-tick
    /// lag is authentic. Also the stage-HELD devil's ambient physics
    /// (`sub_26470` EF:16938-61 runs this after the 1D5D0 legs —
    /// the stagevars held seam).
    pub(crate) fn m21_jump(&mut self, i: usize) {
        let mut v12 = false; // settle: z -= 42 this tick
        let mut v13 = true; // moved: XY allowed this tick
        match self.ent[i].f71 {
            // Landed rest: countdown, then crouch.
            0 | 1 => {
                let n = self.ent[i].f26;
                if n != 0 {
                    self.ent[i].f26 = n - 1;
                } else {
                    self.ent[i].f71 = 2;
                }
                v12 = true;
                v13 = false;
            }
            2 => {
                v12 = true;
                self.ent[i].f71 = 3;
                v13 = false;
            }
            // Launch: airborne — XY moves, z rides the (spent)
            // impulse through the integrator, floored at terrain.
            3 => self.ent[i].f71 = 4,
            // Impulse seed: rand%100 + 140.
            4 => {
                let d = self.mc2_rand(i);
                self.ent[i].f44 = (d % 0x64 + 140) as u16;
                self.ent[i].f71 = 5;
            }
            // Rise → apex (the integrator decays the impulse).
            5 => {
                if (self.ent[i].f44 as i16) < 0 {
                    self.ent[i].f71 = 6;
                }
            }
            // Fall until 230 above the terrain.
            6 => {
                let e = &self.ent[i];
                let ground = self.ground_z(e.x, e.y) as i16;
                if (self.ent[i].z as i32) - (ground as i32) < 230 {
                    self.ent[i].f71 = 7;
                }
            }
            // Pre-land: STILL FALLING (v12 stays 0 — the re-extract's
            // correction to the trace table), XY frozen.
            7 => {
                self.ent[i].f71 = 8;
                v13 = false;
            }
            8 => {
                v12 = true;
                self.ent[i].f71 = 9;
                v13 = false;
            }
            // Landing: cackle roll (always), rest roll (base != 0),
            // land state by rest parity (even → 1, odd → 0).
            9 => {
                let d = self.mc2_rand(i);
                if d % 0xB == 0 {
                    self.snd(42, i);
                }
                let base = self.ent[i].f68;
                if base != 0 {
                    let d = self.mc2_rand(i);
                    let r = (d % base as u32) as i16;
                    self.ent[i].f26 = r;
                    self.ent[i].f71 = (r & 1 == 0) as u8;
                } else {
                    self.ent[i].f26 = 1;
                    self.ent[i].f71 = 0;
                }
                v12 = true;
                v13 = false;
            }
            // 0xA WATER WADE: settle z, XY keeps walking.
            _ => v12 = true,
        }
        // Shared tail (:17098-151): integrator → floor clamp → cave
        // ceiling clamp → water enter/exit → speed → sprite → veto.
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let ground = self.ground_z(x, y) as i16;
        if v12 {
            self.ent[i].z = self.ent[i].z.wrapping_sub(42);
        } else {
            let imp = self.ent[i].f44 as i16;
            self.ent[i].z = self.ent[i].z.wrapping_add(imp);
            self.ent[i].f44 = imp.wrapping_sub(42) as u16;
        }
        if self.ent[i].z < ground {
            self.ent[i].z = ground;
        }
        if self.is_cave() {
            // Ceiling − the params fov (EF:17111-20); impulse zeroed.
            let c = (self.ceiling_z(x, y) as i16 as i32 - self.ent[i].f84 as i32) as i16;
            if self.ent[i].z > c {
                self.ent[i].f44 = 0;
                self.ent[i].z = c;
            }
        }
        let attack = self.ent[i].tick70 == M21_BASE + 2;
        let speed = if self.cap_bit(x, y) == 1 {
            if self.ent[i].f71 == 10 {
                if self.ent[i].z > ground {
                    self.ent[i].f71 = 0; // lifted off the surface
                }
            } else if self.ent[i].z == ground {
                // Grounded on a water tile → wade + (10,5) splash
                // (retail spawns it at the walker's predicted axis —
                // the committed position here, one step apart at most).
                self.ent[i].f71 = 10;
                let z = self.ent[i].z;
                self.mc2_spawn_splash(x, y, z);
            }
            if attack { 66 } else { 40 }
        } else {
            if self.ent[i].f71 == 10 {
                self.ent[i].f71 = 0;
            }
            if attack { 96 } else { 60 }
        };
        self.ent[i].f126 = speed;
        self.m21_pose(i);
        if !v13 {
            self.ent[i].flags |= super::mobs::F_STOP;
        }
    }

    pub(crate) fn m21_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M21_BASE {
            0 | 3 => self.m21_mode(i, 1),
            1 => {
                // sub_26070 (:16760).
                match self.mc2_state_head(i) {
                    1 => {
                        self.ent[i].f146 = self.ent[i].f40;
                        self.m21_mode(i, 2);
                    }
                    2 => self.ent[i].tick70 = M21_BASE + 4,
                    _ => {
                        self.mc2_move_core(i);
                        self.m21_jump(i);
                        // Wander (:16781-91): both draws gated on the
                        // can-turn primitive — heading commits only
                        // at landing (or aligned wade ticks).
                        if self.m21_can_turn(i) {
                            self.mc2_wander_turn(i);
                            self.ent[i].f30 = self.ent[i].f34;
                        }
                        if self.ent[i].f63 & 0x3F == 0
                            && self.ent[i].f58 != 0
                            && let Some(t) = self.mc2_wizard_scan(i, ctx, false)
                        {
                            self.ent[i].f146 = t;
                            self.m21_mode(i, 2);
                        }
                    }
                }
            }
            2 => {
                // sub_26220 (:16838).
                match self.mc2_state_head(i) {
                    1 => self.ent[i].f146 = self.ent[i].f40,
                    2 => {
                        self.ent[i].tick70 = M21_BASE + 4;
                        return;
                    }
                    _ => {}
                }
                let slot = self.ent[i].f146;
                let Some((tx, ty, _)) = self.mc2_target(slot, ctx) else {
                    self.mc2_move_core(i);
                    self.m21_jump(i);
                    self.m21_mode(i, 1);
                    return;
                };
                // Target facing (:16869-85): the OUTER gate is the
                // can-turn primitive; the packmate override runs on
                // the inner 1-in-4 partition. f34 kept in lockstep
                // with the snapped f30 so the walker's commit turn
                // doesn't fight the facing between landings.
                if self.m21_can_turn(i) {
                    let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                    self.ent[i].f34 = Self::angle_between(ex, ey, tx, ty);
                    if self.ent[i].f63 & 3 == 0 {
                        self.mc2_avoid_packmate(i);
                    }
                    self.ent[i].f30 = self.ent[i].f34;
                }
                let mut out_of_range = false;
                if self.ent[i].f63 & 0x1F == 0 {
                    let e = &self.ent[i];
                    if Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, self.ent[i].z))
                        < BEHAVIOR[e.row156 as usize].v_28 as u32
                    {
                        self.mc2_atk_bolt(i, slot, ctx);
                    } else {
                        out_of_range = true;
                    }
                }
                self.mc2_move_core(i);
                self.m21_jump(i);
                if out_of_range {
                    self.m21_mode(i, 1);
                }
            }
            4 => self.mc2_prekill(i, M21_BASE),
            5 => self.mc2_kill(i),
            6 => {} // sub_26420 MISSING from the decompile — unreachable
            _ => {
                // +7 (:16925): 1D5D0 is a no-op for our StageVar2==0
                // spawns; the jump cycle keeps the devil alive.
                self.m21_jump(i);
            }
        }
    }

    // =========================================================================
    // MODEL 23 — the mana leviathan (ctor sub_4CBF0 EF:34454, states
    // 0xB8-BF; the only ctor-flying creature: z = 0x2000, siphons
    // (10,39) mana spheres, (9,9) heavy bolts at wizards)
    // =========================================================================

    pub(crate) fn mc2_spawn_m23(&mut self, x: u16, y: u16, _z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 23;
            e.tick70 = M23_BASE; // 184
            e.f28 = 1;
            e.f128 = 24;
            e.f130 = 14;
            e.f126 = 24;
            e.max_life = 10000;
            e.f140 = 100;
        }
        let d = self.mc2_rand(i);
        {
            let e = &mut self.ent[i];
            let f = ((d & 0x7FF) as i32 - 1) as u16;
            e.f34 = f;
            e.f30 = f;
            // pitch NOT set — verbatim (:34469 note).
            e.f44 = 0x2000; // the flying altitude target
            e.f56 = 1;
            e.row156 = 91;
            e.f58 = 64;
            e.f66 = 3;
        }
        self.ent[i].f63 = self.mc2_ord(23);
        self.link(i, x, y, 0x2000);
        self.refill_life(i);
        self.mc2_set_sprite(i, 289);
        self.mc2_shift_rot(i, 384, 384);
        Some(i)
    }

    /// `sub_27FE0`: state + sub-state + timer in one.
    fn m23_mode(&mut self, i: usize, action: u8, sub: u8, timer: i16) {
        self.ent[i].tick70 = action;
        self.ent[i].f71 = sub;
        self.ent[i].f26 = timer;
    }

    /// `sub_28000` (:18384): nearest live (10,39) mana sphere.
    ///
    /// The scanned list (`dword_38523`) carries class-10 models 39,
    /// 40 AND 57 (:40018-63 builds it), and the scan filters
    /// `model == 39` — the (10,57) FOOL'S-MANA sphere is deliberately
    /// NOT siphonable. Since OPEN-6 a NATIVE m57 carries model 57 too
    /// (mc2/effects.rs), so the model test alone is the filter on both
    /// paths; the action test is kept as belt-and-braces.
    fn m23_find_node(&self, i: usize) -> Option<u16> {
        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
        let mut best: Option<(usize, i32)> = None;
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if c.class64 == 10 && c.model65 == 39 && c.tick70 != 62 && c.flags & 0x400 == 0 {
                let d2 = Self::dist2_sq(ex, ey, c.x, c.y);
                if best_d2(&best, d2) {
                    best = Some((j, d2));
                }
            }
        }
        best.map(|(j, _)| j as u16)
    }

    /// `sub_28420` (:18603): the locked node must still be a (10,39)
    /// (same fool's-mana exclusion as [`Self::m23_find_node`]).
    fn m23_node_ok(&self, i: usize) -> bool {
        let t = self.ent[i].f146 as usize;
        t != 0
            && t < self.ent.len()
            && self.ent[t].class64 == 10
            && self.ent[t].model65 == 39
            && self.ent[t].tick70 != 62
            && self.ent[t].flags & 0x400 == 0
    }

    /// `sub_28110` (:18445): damage intake + wizard retaliation.
    fn m23_post(&mut self, i: usize) {
        match self.mc2_state_head(i) {
            2 => self.m23_mode(i, M23_BASE + 4, 0, 0),
            1 => {
                let src = self.ent[i].f40;
                if self.mc2_is_wizard(src) {
                    self.ent[i].f146 = src;
                    self.m23_mode(i, M23_BASE + 2, 0, 0);
                }
            }
            _ => {}
        }
    }

    /// `sub_28390` (:18580) — the landing servo, and the gate that
    /// starts the siphon. Two independent axes, each with its own
    /// tolerance, and "settled" means BOTH are in band:
    ///   - 2-D reach 128 (`EuclideanDistXYZ_58490` is XY-only,
    ///     utilities/Maths.cpp:738): outside it, turn to the node and
    ///     walk (`sub_1B8C0`); inside it, hold — retail does NOT run
    ///     the mover once aligned.
    ///   - station-keeping 640 ABOVE the node within ±64, stepping
    ///     32/tick.
    ///
    /// Corpus (mc2l24, 14 siphon entries between t=14512 and t=15648):
    /// every dweller enters the siphon with `dz` in [588, 701] and
    /// 2-D gap ≤ 121 — the 640±64 band and the 128 reach exactly.
    fn m23_station_keep(&mut self, i: usize, t: usize) -> bool {
        let mut settled = true;
        let (sp, tp) = {
            let e = &self.ent[i];
            let s = &self.ent[t];
            ((e.x, e.y), (s.x, s.y, s.z))
        };
        if Self::isqrt(Self::dist2_sq(sp.0, sp.1, tp.0, tp.1) as u32) as i32 > 128 {
            settled = false;
            self.ent[i].f34 = Self::angle_between(sp.0, sp.1, tp.0, tp.1);
            self.mc2_move_core(i);
        }
        // Read z AFTER the move commit — retail's servo reads the
        // post-`sub_1B8C0` position.
        let gap = self.ent[i].z as i32 - (tp.2 as i32 + 640);
        if gap.abs() > 64 {
            settled = false;
            let step = if gap <= 0 { 32 } else { -32 };
            self.ent[i].z = self.ent[i].z.wrapping_add(step);
        }
        settled
    }

    /// `sub_28060` (:18415): a descending dweller stacked on a
    /// packmate LIFTS 16 and aborts the approach — only the HIGHER of
    /// the pair moves, and the box is 2·pitch in x/y, 2·fov in z (its
    /// own extents both times). Retail walks the live per-model
    /// bucket, same gates as [`Gen::mc2_avoid_packmate`].
    fn m23_lift_off_packmate(&mut self, i: usize) -> bool {
        let (ex, ey, ez, span, zspan, model, id) = {
            let e = &self.ent[i];
            (
                e.x,
                e.y,
                e.z,
                2 * e.f80 as i32,
                2 * e.f84 as i32,
                e.model65,
                e.id24,
            )
        };
        for c in self.ent.iter().skip(1) {
            if c.class64 == 5
                && c.model65 == model
                && c.id24 != id
                && c.act_life >= 0
                && !matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
                && c.flags & 0x400 == 0
                && ((ex.wrapping_sub(c.x)) as i16 as i32).abs() < span
                && ((ey.wrapping_sub(c.y)) as i16 as i32).abs() < span
                && (ez as i32 - c.z as i32).abs() < zspan
                && ez >= c.z
            {
                self.ent[i].z = ez.wrapping_add(16);
                return true;
            }
        }
        false
    }

    /// The altitude-keeping z step of `sub_27950` (:18052).
    fn m23_altitude(&mut self, i: usize) {
        let v2 = self.ent[i].z as i32 - self.ent[i].f44 as i32;
        if v2.abs() >= 256 {
            self.ent[i].z = self.ent[i].z.wrapping_add(if v2 <= 0 { 32 } else { -32 });
        }
    }

    pub(crate) fn m23_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M23_BASE {
            0 => {
                // sub_27950 — the patrol/hunt loop.
                self.mc2_move_core(i);
                self.mc2_avoid_packmate(i);
                self.m23_altitude(i);
                match self.ent[i].f71 {
                    0 => {
                        self.ent[i].f126 = self.ent[i].f130;
                        // PRE-decrement test (EF:18100-02: `if (v5y)
                        // return` on the OLD value; a post-test fires
                        // one tick early).
                        let old = self.ent[i].f26;
                        self.ent[i].f26 = old - 1;
                        if old <= 0 {
                            self.m23_mode(i, M23_BASE, 1, 0);
                        }
                    }
                    1 => {
                        if let Some(n) = self.m23_find_node(i) {
                            self.ent[i].f44 = 0x2000;
                            self.ent[i].f146 = n;
                            self.m23_mode(i, M23_BASE, 2, 0);
                        } else {
                            let (x, y) = (self.ent[i].x, self.ent[i].y);
                            let hover = (self.ground_z(x, y) as i16).wrapping_add(0x700);
                            self.ent[i].f44 = hover as u16;
                            self.m23_mode(i, M23_BASE, 0, 80);
                        }
                    }
                    _ => {
                        if self.m23_node_ok(i) {
                            // :18140 — the re-aim AND the range test
                            // ride the 4-tick cadence byte; testing
                            // every tick hands the descend over up to
                            // 3 ticks early (mc2l24 slot 230, the
                            // residual `action 184 vs 185` rows).
                            if self.ent[i].f63 & 3 == 0 {
                                let t = self.ent[i].f146 as usize;
                                let (sp, tp) = {
                                    let e = &self.ent[i];
                                    let s = &self.ent[t];
                                    ((e.x, e.y, e.z), (s.x, s.y, s.z))
                                };
                                self.ent[i].f34 = Self::angle_between(sp.0, sp.1, tp.0, tp.1);
                                // 2-D (EF:18144 — `EuclideanDistXYZ`
                                // never reads z): the leviathan flies
                                // far above its node, so a 3-D read
                                // would stall the descend transition.
                                if crate::mc2::morph::dist2d(sp.0, sp.1, tp.0 as i32, tp.1 as i32)
                                    < 768
                                {
                                    self.m23_mode(i, M23_BASE + 1, 0, 500);
                                }
                            }
                        } else {
                            self.m23_mode(i, M23_BASE, 1, 0);
                        }
                    }
                }
                self.m23_post(i);
            }
            1 => {
                // sub_27B20 (:18250) — descend/land onto the node.
                match self.ent[i].f71 {
                    0 => {
                        self.ent[i].f126 = self.ent[i].f128;
                        // PRE-decrement: retail stores `--v2` and
                        // tests the NEW value (:18186-88).
                        self.ent[i].f26 = self.ent[i].f26.wrapping_sub(1);
                        // The abort trio is evaluated BEFORE the
                        // approach servo and short-circuits in this
                        // order: timer, node still a (10,39), and the
                        // anti-stack lift.
                        let approach = self.ent[i].f26 != 0
                            && self.m23_node_ok(i)
                            && !self.m23_lift_off_packmate(i);
                        if approach {
                            let t = self.ent[i].f146 as usize;
                            if self.m23_station_keep(i, t) {
                                self.m23_mode(i, M23_BASE + 3, 0, 0);
                            }
                        } else {
                            self.m23_mode(i, M23_BASE + 1, 1, 0);
                        }
                    }
                    1 => {
                        if self.ent[i].z >= 0x2000 {
                            // No f44 write (EF:18174-84 leaves the
                            // stale value — it governs the NEXT
                            // descent's target).
                            self.m23_mode(i, M23_BASE, 0, 80);
                        } else {
                            self.ent[i].z = self.ent[i].z.wrapping_add(32);
                        }
                    }
                    // :18173 acts on sub 1 alone; anything higher is
                    // a bare post pass.
                    _ => {}
                }
                self.m23_post(i);
            }
            2 => {
                // sub_27E00 — the (9,9) ranged retaliation.
                self.snd(59, i);
                self.ent[i].f126 = self.ent[i].f128;
                self.mc2_move_core(i);
                let slot = self.ent[i].f146;
                let mut broke = self.mc2_target(slot, ctx).is_none();
                if !broke {
                    let (tx, ty, tz) = self.mc2_target(slot, ctx).unwrap();
                    if self.ent[i].f63 & 3 == 0 {
                        self.mc2_aim_avoid(i, tx, ty);
                    }
                    let row = &BEHAVIOR[self.ent[i].row156 as usize];
                    if self.ent[i].f63 as i16 & row.v_26 == 0 {
                        let e = &self.ent[i];
                        if Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz)) < row.v_28 as u32 {
                            self.mc2_atk_heavy9(i, slot, ctx);
                        } else {
                            broke = true;
                        }
                    }
                }
                self.m23_post(i);
                if broke {
                    self.ent[i].f146 = 0;
                    self.m23_mode(i, M23_BASE + 3, 3, 0);
                }
            }
            3 => {
                // sub_27C10 — the siphon. Retail's control flow is a
                // FALL-THROUGH, not a switch: sub 0 seeds the rise
                // step and the 64-tick timer and then runs the siphon
                // body in that same tick (:18226-40 has no return —
                // only sub >= 2 jumps past the body to LABEL_24). So
                // the grab, the +10 ramp and the swallow test all
                // start on the ARRIVAL tick, and an arrival onto a
                // ball another dweller already holds still steals the
                // grab on its way out (v9 is set, the body runs).
                self.snd(59, i);
                let mut abort = false; // v9  → re-hunt   (base+3 sub 3)
                let mut lost = false; // v10 → climb-out (base+1 sub 1)
                let mut body = true;
                match self.ent[i].f71 {
                    0 => {
                        let free = self.m23_node_ok(i) && {
                            let t = self.ent[i].f146 as usize;
                            self.ent[t].flags & 0x40 == 0
                        };
                        if free {
                            // `word_0x2C_44 = 18` (:18238) — the rise
                            // step the GRABBED BALL reads off its
                            // collector every tick (mc1/combat.rs
                            // `ball_tick`, EF:26120), ramped +10 per
                            // siphon tick below.
                            self.ent[i].f44 = 18;
                            self.m23_mode(i, M23_BASE + 3, 1, 64);
                        } else {
                            abort = true;
                        }
                    }
                    1 => {}
                    _ => {
                        // :18242-59 — sub 2 is a bare no-op; only sub
                        // 3 re-hunts. Both skip the siphon body.
                        body = false;
                        if self.ent[i].f71 == 3 {
                            self.ent[i].f146 = 0;
                            if let Some(n) = self.m23_find_node(i) {
                                let t = n as usize;
                                // :18249 assigns the target INSIDE the
                                // condition, BEFORE the range test —
                                // an out-of-reach node still latches
                                // (it is what the next descend reads).
                                self.ent[i].f146 = n;
                                let (sp, tp) = {
                                    let e = &self.ent[i];
                                    let s = &self.ent[t];
                                    ((e.x, e.y, e.z), (s.x, s.y, s.z))
                                };
                                // 2-D (EF:18250 — `EuclideanDistXYZ`
                                // never reads z).
                                if crate::mc2::morph::dist2d(sp.0, sp.1, tp.0 as i32, tp.1 as i32)
                                    <= 3584
                                {
                                    self.m23_mode(i, M23_BASE + 1, 0, 500);
                                } else {
                                    lost = true;
                                }
                            } else {
                                lost = true;
                            }
                        }
                    }
                }
                if body {
                    // :18261-86. The 64-tick f26 timeout decrements
                    // INSIDE the node-ok arm (retail's `v3x &&
                    // (--f26)` short-circuit) — an unreachable ball
                    // aborts to re-hunt instead of siphoning forever.
                    if self.ent[i].f146 != 0 {
                        let held = self.m23_node_ok(i) && {
                            self.ent[i].f26 = self.ent[i].f26.wrapping_sub(1);
                            self.ent[i].f26 != 0
                        };
                        if held {
                            let t = self.ent[i].f146 as usize;
                            self.ent[t].flags |= 0x40; // grabbed
                            self.ent[t].f146 = i as u16;
                            self.ent[i].f44 = self.ent[i].f44.wrapping_add(10);
                            // :18271 is the 3-axis extent overlap
                            // `sub_106C0` (NOT a radius) — with the
                            // leviathan's 384 half-extents the ball is
                            // swallowed well before it reaches the
                            // body, and the `ball.z > self.z` half
                            // catches the ball that overshoots.
                            if self.ent_overlap(i, t) || self.ent[t].z > self.ent[i].z {
                                // Swallow: steal the mana, consume it.
                                self.ent[i].f140 += self.ent[t].f140;
                                self.ent[t].flags |= 0x400;
                                abort = true;
                            }
                        } else {
                            abort = true;
                        }
                    } else {
                        lost = true;
                    }
                    if abort {
                        self.m23_mode(i, M23_BASE + 3, 3, 0);
                    }
                }
                if lost {
                    self.m23_mode(i, M23_BASE + 1, 1, 0);
                }
                self.m23_post(i);
            }
            4 => self.mc2_prekill(i, M23_BASE),
            5 => self.mc2_kill(i),
            6 => {} // sub_28460 MISSING — unreachable
            _ => {}
        }
    }

    // =========================================================================
    // MODEL 24 — cave brute (ctor sub_4CCF0 EF:34487; CAVE-ONLY —
    // aggros the class-3 building list via the shared idle scan,
    // not the player; melee 1500 @ 1536; snd 7 on chase)
    // =========================================================================

    pub(crate) fn mc2_spawn_m24(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        if !self.is_cave() {
            return None; // `if MapType != Cave return 0` (:34490)
        }
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 24;
            e.tick70 = M24_BASE + 1; // 193 idle (:34495)
            e.f28 = 1;
            e.f71 = 0;
            e.f128 = 80;
            e.f130 = 24;
            e.max_life = 16000;
            e.f126 = 24; // actSpeed = maxSpeed (:34502)
        }
        self.mc2_set_mana_half(i);
        self.ent[i].f36 = 0;
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f44 = 1500; // melee damage (sub_1CF20 @ 1536)
            e.f56 = 1;
            e.row156 = 102;
            e.f58 = 64;
            e.f66 = 3;
        }
        self.ent[i].f63 = self.mc2_ord(24);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 335);
        self.mc2_shift_rot(i, 256, 640);
        Some(i)
    }

    /// `sub_28690` (:18723): the shared m24 target acquisition.
    fn m24_acquire(&mut self, i: usize, ctx: &MobCtx) {
        if self.ent[i].f58 == 0 || self.ent[i].f63 & 0xF != 0 {
            return;
        }
        // sub_28690 (:18744-71) walks the WHOLE class-3 list — the
        // brute aggros castles and balloons too, nearest-wins, not
        // just wizards. Winner validity: alive + not reaped
        // (the byte[1]&4 check = our 0x400, already in the scan).
        if let Some(t) = self.mc2_class3_scan(i, ctx) {
            self.ent[i].tick70 = M24_BASE + 2;
            self.ent[i].f146 = t;
        }
    }

    /// `sub_287B0` (:18778): sprite/speed by state.
    fn m24_pose(&mut self, i: usize) {
        match self.ent[i].tick70 - M24_BASE {
            0 => {
                self.mc2_set_sprite(i, 336);
                self.ent[i].f126 = 0;
            }
            2 => self.ent[i].f126 = self.ent[i].f128, // 80
            6 => self.ent[i].f126 = 2 * self.ent[i].f130, // 48
            _ => {
                self.ent[i].f126 = self.ent[i].f130;
                if self.ent[i].type86 != 335 {
                    self.mc2_set_sprite(i, 335);
                }
            }
        }
    }

    pub(crate) fn m24_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M24_BASE {
            0 => {
                self.mc2_patrol(i, M24_BASE);
                if self.ent[i].tick70 == M24_BASE {
                    if self.ent[i].f63 & 7 == 0 {
                        let d = self.mc2_rand(i);
                        if d % 3 == 0 {
                            self.ent[i].tick70 = M24_BASE + 1;
                        }
                    }
                    if self.ent[i].tick70 == M24_BASE {
                        self.m24_acquire(i, ctx);
                    }
                } else {
                    self.ent[i].tick70 = M24_BASE + 6;
                }
                self.m24_pose(i);
            }
            1 => {
                self.mc2_idle(i, M24_BASE, ctx);
                if self.ent[i].tick70 == M24_BASE + 1 {
                    if self.ent[i].f63 & 7 == 0 {
                        let d = self.mc2_rand(i);
                        if d % 3 == 0 {
                            self.ent[i].tick70 = M24_BASE;
                        }
                    }
                    if self.ent[i].tick70 == M24_BASE + 1 {
                        self.m24_acquire(i, ctx);
                    }
                } else if self.ent[i].tick70 == M24_BASE + 2 {
                    // primitive acquire keeps +2
                } else {
                    self.ent[i].tick70 = M24_BASE + 6;
                }
                self.m24_pose(i);
            }
            2 => {
                self.snd(7, i);
                if self.mc2_chase_attack(i, M24_BASE, ctx, Self::mc2_atk_melee_1536) {
                    self.ent[i].tick70 = M24_BASE + 6;
                }
                self.m24_pose(i);
            }
            3 => {
                self.ent[i].tick70 = M24_BASE + 1;
                self.mc2_patrol(i, M24_BASE);
                self.m24_pose(i);
            }
            4 => self.mc2_prekill(i, M24_BASE),
            5 => self.mc2_kill(i),
            6 => {
                self.mc2_flee(i, M24_BASE, ctx);
                self.m24_acquire(i, ctx);
                self.m24_pose(i);
            }
            _ => self.m24_pose(i),
        }
    }

    // =========================================================================
    // MODEL 25 — the swarm splitter (ctor sub_4CE00 EF:34523, states
    // 0xC8-CF; castle-drain minis, splits into 3 on death,
    // trace mc2-class5-m25-26-28-class2-treeburn.md)
    // =========================================================================

    pub(crate) fn mc2_spawn_m25(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 25;
            e.tick70 = M25_BASE + 1; // 201
            e.f28 = 1;
            e.f71 = 0;
            e.f128 = 60;
            e.f130 = 20;
            e.max_life = 7500;
            e.f126 = 60;
        }
        self.mc2_set_mana_half(i);
        {
            self.ent[i].f36 = 0;
        }
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f44 = 300; // damage AND the brain's lifetime countdown
            e.f56 = 1;
            e.row156 = 92;
            e.f58 = 64;
            e.f66 = 3;
        }
        self.ent[i].f63 = self.mc2_ord(25);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 290);
        self.mc2_shift_rot(i, 384, 384);
        Some(i)
    }

    /// The castle of a wizard slot (class 3 model 2 keyed on id24;
    /// the human's is id24 == PLAYER_TARGET).
    pub(crate) fn mc2_castle_of(&self, wiz: u16) -> Option<usize> {
        let want = if wiz == PLAYER_TARGET {
            PLAYER_TARGET
        } else if (wiz as usize) < self.ent.len() {
            self.ent[wiz as usize].id24
        } else {
            return None;
        };
        self.ent
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, c)| {
                c.class64 == 3 && c.model65 == 2 && c.id24 == want && c.flags & 0x400 == 0
            })
            .map(|(j, _)| j)
    }

    pub(crate) fn m25_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M25_BASE {
            0 => self.m25_brain(i, ctx),
            1 => {
                self.mc2_idle(i, M25_BASE, ctx);
                self.ent[i].act_life = self.ent[i].act_life.max(0); // :19062 clamp
            }
            2 => {
                self.snd(37, i);
                let _ = self.mc2_chase_attack(i, M25_BASE, ctx, Self::mc2_atk_bolt);
                self.ent[i].act_life = self.ent[i].act_life.max(0);
            }
            3 => {
                self.ent[i].tick70 = M25_BASE + 1;
                self.ent[i].act_life = self.ent[i].act_life.max(0);
            }
            4 => self.m25_split(i),
            5 => {
                // :19187 — kill/score (the shared 1C890 gate:
                // human killer + self-id exclusion).
                if self.ent[i].f38 == PLAYER_TARGET && self.ent[i].id24 != PLAYER_TARGET {
                    self.kills += 1;
                }
                if self.ent[i].f71 != 0 {
                    self.mc2_kill(i);
                } else {
                    self.ent[i].act_life = -1;
                    self.ent[i].flags |= 0x400;
                }
            }
            6 => {} // sub_28F40 MISSING — unreachable
            _ => {
                // :19205 — respawn hook.
                if self.ent[i].f71 != 0 {
                    self.ent[i].tick70 = M25_BASE;
                    self.ent[i].f71 = 3;
                } // else sub_1D5D0 no-op
            }
        }
    }

    /// `sub_28860` (:18828) — the mini/adult brain: lifetime
    /// countdown, castle hunt, water sprite swap.
    fn m25_brain(&mut self, i: usize, ctx: &MobCtx) {
        let sub = self.ent[i].f71;
        let mut v2 = 0u8;
        if !matches!(sub, 1 | 2) {
            v2 = self.mc2_state_head(i);
        }
        // subSpellIndex-- lifetime (:18896).
        self.ent[i].f44 = self.ent[i].f44.wrapping_sub(1);
        if self.ent[i].f44 == 0 {
            v2 = 2;
        }
        if v2 == 2 {
            self.ent[i].tick70 = M25_BASE + 4;
            return;
        }
        let mut speed_reset = false;
        match self.ent[i].f71 {
            1 => {
                self.ent[i].f26 = 52;
                self.ent[i].f71 = 2;
                // fall into 2's regen next tick (retail falls through;
                // one-tick skew accepted)
            }
            2 => {
                self.ent[i].act_life = self.ent[i].max_life as i32;
                self.ent[i].mail[0].1 = 0;
                self.ent[i].f26 -= 1;
                if self.ent[i].f26 < 0 {
                    self.ent[i].f71 = 3;
                    speed_reset = true;
                } else if self.ent[i].f26 > 13 {
                    // The hatch spin (:19926-28).
                    let f34 = self.ent[i].f34;
                    self.ent[i].f34 = (f34 & 0xFF) | ((((f34 >> 8) + 1) & 7) << 8);
                }
            }
            3 => {
                let t = self.ent[i].f38;
                if !self.mc2_is_wizard(t) {
                    self.ent[i].f71 = 8;
                    self.ent[i].f26 = 100;
                } else if self.mc2_castle_of(t).is_some() {
                    self.ent[i].f71 = 5;
                    self.ent[i].f146 = t;
                } else {
                    let d = self.mc2_rand(i);
                    self.ent[i].f71 = 4;
                    self.ent[i].f26 = (d % 100 + 100) as i16;
                }
            }
            4 => {
                self.ent[i].f26 -= 1;
                if self.ent[i].f26 < 0 {
                    self.ent[i].f71 = 3;
                }
            }
            5 => {
                if let Some(c) = self.mc2_castle_of(self.ent[i].f146) {
                    if self.ent[i].f63 & 7 == 0 {
                        let (cx, cy) = (self.ent[c].x, self.ent[c].y);
                        let e = &self.ent[i];
                        self.ent[i].f34 = Self::angle_between(e.x, e.y, cx, cy);
                        // In-range = the box overlap (deliberate: for
                        // CompareAxisWithShift_10750).
                        let e = &self.ent[i];
                        let near = ((e.x.wrapping_sub(cx)) as i16 as i32).abs()
                            < (e.f80 + self.ent[c].f80) as i32
                            && ((e.y.wrapping_sub(cy)) as i16 as i32).abs()
                                < (e.f82 + self.ent[c].f82) as i32;
                        if near {
                            self.ent[i].f71 = 6;
                        }
                    }
                } else {
                    self.ent[i].f71 = 3;
                }
            }
            6 | 7 => {
                if self.ent[i].f71 == 6 {
                    // The 6→7 transition sets v26 unconditionally
                    // (EF:18980-83) before falling into LABEL_41.
                    speed_reset = true;
                }
                self.ent[i].f71 = 7;
                if let Some(c) = self.mc2_castle_of(self.ent[i].f146) {
                    let (cx, cy) = (self.ent[c].x, self.ent[c].y);
                    let e = &self.ent[i];
                    let near = ((e.x.wrapping_sub(cx)) as i16 as i32).abs()
                        < (e.f80 + self.ent[c].f80) as i32
                        && ((e.y.wrapping_sub(cy)) as i16 as i32).abs()
                            < (e.f82 + self.ent[c].f82) as i32;
                    if near {
                        // The castle gnaw: 60 into its inbox (:18992).
                        let src = self.ent[i].id24;
                        self.mc2_melee_write(c as u16, 0x3C, src);
                    } else {
                        self.ent[i].f71 = 5;
                        speed_reset = true;
                    }
                } else {
                    self.ent[i].f71 = 3;
                    speed_reset = true;
                }
            }
            8 => {
                self.ent[i].f26 -= 1;
                if self.ent[i].f26 < 0 {
                    self.ent[i].tick70 = M25_BASE + 4;
                    return;
                }
            }
            _ => {}
        }
        if v2 == 1 {
            // Damage retarget: the brain hunts the ATTACKER's castle.
            self.ent[i].f38 = self.ent[i].f40;
        }
        // Wander + move (:19018-25).
        if self.ent[i].f63 & 7 == 0 {
            let d1 = self.mc2_rand(i);
            let d2 = self.mc2_rand(i);
            let sign = 2 * ((d1 % 157) / 79) as i32 - 1;
            let f34 = self.ent[i].f34;
            self.ent[i].f34 = (f34 as i32 + sign * (d2 % 381) as i32) as u16 & 0x7FF;
        }
        self.mc2_move_core(i);
        // Water sprite swap (:19026-47).
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let on_water = self.cap_bit(x, y) == 1;
        if on_water {
            if self.ent[i].type86 == 314 {
                // Already swimming: ABOVE ground swaps back to 313
                // with no minSpeed and no v26; otherwise a total
                // no-op (EF:19029-35).
                if z > self.ground_z(x, y) as i16 {
                    self.mc2_set_sprite(i, 313);
                }
            } else {
                self.mc2_set_sprite(i, 314);
                self.ent[i].f128 = 35;
                speed_reset = true;
            }
        } else if self.ent[i].type86 != 313 {
            self.mc2_set_sprite(i, 313);
            self.ent[i].f128 = 60;
            speed_reset = true;
        }
        if speed_reset {
            self.ent[i].f126 = self.ent[i].f128;
            if self.ent[i].f71 == 2 {
                self.ent[i].f126 = self.ent[i].f128 + 50;
            }
        }
        let _ = ctx;
    }

    /// `sub_28CE0` (:19103) — the death split: 3 minis + the (10,1)
    /// burst.
    fn m25_split(&mut self, i: usize) {
        if self.ent[i].f71 != 0 {
            self.mc2_prekill(i, M25_BASE);
            return;
        }
        let (x, y, z, mana, killer) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f140, e.f40)
        };
        // Pool exhaustion trades the 3 minis for a sphere dump, but
        // FALLS THROUGH to the shared burst + state advance — the
        // (10,1) spawn sits outside the if/else (EF:19176-81); the
        // old early return skipped the burst.
        if self.free.len() <= 1 {
            self.mc2_mana_spheres(i, false);
        } else {
            self.m25_split_minis(x, y, z, mana, killer);
        }
        self.mc2_corpse_burst(i);
        self.ent[i].tick70 = M25_BASE + 5;
        self.ent[i].f71 = 0;
    }

    /// The 3-mini spawn loop of `sub_28CE0` (:19110-70).
    fn m25_split_minis(&mut self, x: u16, y: u16, z: i16, mana: i32, killer: u16) {
        let share = mana / 3;
        for n in 0..3 {
            let Some(c) = self.new_event() else { continue };
            {
                let e = &mut self.ent[c];
                e.class64 = 5;
                e.model65 = 25;
                e.tick70 = M25_BASE;
                e.f28 = 1;
                e.f71 = 1;
                e.f128 = 35;
                e.f130 = 60;
                e.f126 = 85;
                e.f140 = if n == 2 { mana - 2 * share } else { share };
                e.max_life = 80;
            }
            let d = self.mc2_rand(c);
            {
                let e = &mut self.ent[c];
                let f = ((d & 0x7FF) as i32 - 1) as u16;
                e.f34 = f;
                e.f30 = f;
                e.f44 = 15000; // the mini's lifetime seed (:19161)
                e.f56 = 1;
                e.row156 = 95;
                e.f58 = 64;
                e.f66 = 3;
                e.f38 = killer;
            }
            self.ent[c].f63 = self.mc2_ord(25);
            self.link(c, x, y, z);
            self.refill_life(c);
            self.mc2_set_sprite(c, 314);
            self.mc2_shift_rot(c, 32, 32);
        }
    }

    // =========================================================================
    // MODEL 26 — the mana leech (ctor sub_4CF00 EF:34557, states
    // 0xD0-D7; drains wizard mana, forces spell discharges)
    // =========================================================================

    pub(crate) fn mc2_spawn_m26(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 26;
            e.tick70 = M26_BASE + 1; // 209
            e.f28 = 1;
            e.f128 = 25;
            e.f130 = 25;
            e.max_life = 4400;
            e.f126 = 25;
        }
        self.mc2_set_mana_half(i);
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            e.f44 = 300;
            e.f56 = 1;
            e.row156 = 99;
            e.f58 = 64;
            e.f66 = 3;
        }
        self.ent[i].f63 = self.mc2_ord(26);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 318);
        self.mc2_shift_rot(i, 256, 384);
        // sub_293D0 post-init (:34585) — the wake primitive.
        self.m26_wake(i);
        Some(i)
    }

    /// `sub_293D0` (:19425): outside the attack state, clear the
    /// target and go full-speed (byte[2] bit 7 = flags bit 23).
    /// DUAL-PURPOSE bit: the renderer's per-entity override reads it
    /// as translucency mode 2 (GRO:3779-3805) — retail's wraith is
    /// deliberately 33%-opaque while hunting, solid while draining
    /// (docs/traces/mc2-transparency-drawlist.md §6.2).
    fn m26_wake(&mut self, i: usize) {
        if self.ent[i].tick70 != M26_BASE + 2 {
            self.ent[i].f146 = 0;
            self.ent[i].flags |= 1 << 23;
            self.ent[i].f126 = self.ent[i].f130;
        }
    }

    /// `sub_293B0` (:19411): in the attack state, slow down.
    fn m26_calm(&mut self, i: usize) {
        if self.ent[i].tick70 == M26_BASE + 2 {
            self.ent[i].flags &= !(1 << 23);
            self.ent[i].f126 = self.ent[i].f128;
        }
    }

    pub(crate) fn m26_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M26_BASE {
            0 => {
                self.mc2_patrol(i, M26_BASE);
                self.m26_calm(i);
            }
            1 => {
                self.mc2_idle(i, M26_BASE, ctx);
                self.m26_calm(i);
            }
            2 => {
                // sub_28FF0 (:19233) — the leech.
                if self.ent[i].f63 & 0x1F == 0 {
                    self.snd(62, i);
                }
                match self.mc2_state_head(i) {
                    1 => self.ent[i].f146 = self.ent[i].f40,
                    2 => {
                        self.ent[i].tick70 = M26_BASE + 4;
                        self.m26_wake(i);
                        return;
                    }
                    _ => {}
                }
                self.mc2_move_core(i);
                let slot = self.ent[i].f146;
                if self.mc2_is_wizard(slot) && self.mc2_target(slot, ctx).is_some() {
                    let (tx, ty, tz) = self.mc2_target(slot, ctx).unwrap();
                    if self.ent[i].f63 & 3 == 0 {
                        self.mc2_aim_avoid(i, tx, ty);
                    }
                    // The drain (:19331-34): −(manaRegen + 14).
                    if slot == PLAYER_TARGET {
                        self.mc2_player_drain.0 += 14; // deliberate: human regen not modeled
                    } else {
                        let t = slot as usize;
                        let amt = self.ent[t].f136 + 14;
                        self.ent[t].f140 = (self.ent[t].f140 - amt).max(0);
                    }
                    if self.ent[i].f63 & 3 == 0 {
                        let e = &self.ent[i];
                        let v10 = Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz));
                        let row = &BEHAVIOR[e.row156 as usize];
                        if v10 <= row.v_28 as u32 {
                            let target_is_avatar =
                                slot == PLAYER_TARGET || self.ent[slot as usize].model65 == 0;
                            // ALL in-range paths STAY DRAINING: every
                            // `return sub_293D0` is a state no-op at
                            // 210 (EF:19338-76 + 19426-40) — the only
                            // exit to 209 is v10 > v_28 below.
                            if !(v10 >= 2048 || !target_is_avatar) {
                                // The %63 spell-hijack roll
                                // (EF:19346-47, ONE global-LCG draw):
                                // 4 = steal the RIGHT hand, 5 = the
                                // LEFT, all else nothing. The
                                // empty-hand/slot-0/re-steal-lock
                                // aborts run AFTER the draw
                                // (world-side, sub_69300) — the roll
                                // is spent either way. Only the
                                // human's book exists port-side, so
                                // the mail is PLAYER_TARGET-gated
                                // (retail model-0 targets only).
                                self.rand = self.rand.wrapping_mul(9377).wrapping_add(9439);
                                let roll = self.rand % 63;
                                if slot == PLAYER_TARGET && (roll == 4 || roll == 5) {
                                    self.mc2_steal_mail.0.push((i as u16, (roll - 3) as u8));
                                }
                            }
                        } else {
                            self.ent[i].tick70 = M26_BASE + 1;
                        }
                    }
                } else {
                    self.ent[i].tick70 = M26_BASE + 1;
                }
                self.m26_wake(i);
            }
            3 => {
                self.mc2_pack(i, M26_BASE);
                self.m26_calm(i);
            }
            4 => self.mc2_prekill(i, M26_BASE),
            5 => self.mc2_kill(i),
            6 => {} // sub_29370 MISSING — unreachable
            _ => {
                self.m26_calm(i);
            }
        }
    }

    // =========================================================================
    // MODEL 28 — the melee brute (ctor sub_4D1D0 EF:34695, states
    // 0xE0-E7; the fastest creature, 2000-damage swing arcs)
    // =========================================================================

    pub(crate) fn mc2_spawn_m28(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 28;
            e.tick70 = M28_BASE + 1; // 225
            e.f28 = 1;
            e.f128 = 120;
            e.f130 = 64;
            e.max_life = 8000;
        }
        self.mc2_set_mana_half(i);
        // byte[3] |= 8 (:34707) — no ported reader; bit 30 is its
        // home (27 belongs to the blocked-status mapping).
        self.ent[i].flags |= 1 << 30;
        self.mc2_ctor_facing(i);
        {
            let e = &mut self.ent[i];
            e.f36 = 0;
            e.f44 = 2000;
            e.f56 = 1;
            e.row156 = 93;
            e.f58 = 64;
            e.f66 = 3;
            e.f126 = e.f130 + (e.f128 - e.f130) / 2; // 92 (:34719)
        }
        self.ent[i].f63 = self.mc2_ord(28);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 292);
        self.mc2_shift_rot(i, 85, 42);
        Some(i)
    }

    /// `sub_2B860` (:21308): sprite/row config.
    fn m28_pose(&mut self, i: usize, mode: u8) {
        match mode {
            1 => {
                self.ent[i].row156 = 93;
                self.mc2_set_sprite(i, 292);
                self.mc2_shift_rot(i, 85, 42);
                self.ent[i].f126 = self.ent[i].f130;
            }
            2 => {
                self.ent[i].row156 = 93;
                self.ent[i].f44 = 0;
                self.ent[i].f126 = self.ent[i].f128;
                self.mc2_set_sprite(i, 291);
                self.mc2_shift_rot(i, 384, 768);
                // dword_0x10_16 = the strike animation length; the
                // retail count comes from the anim bank (deliberate 16).
                self.ent[i].f26 = 16;
            }
            _ => {
                self.ent[i].f58 = 0;
                self.ent[i].row156 = 94;
                self.ent[i].f126 = self.ent[i].f128 - 28; // 92
            }
        }
    }

    /// `sub_2BA50` (:21416).
    fn m28_sub(&mut self, i: usize, n: u8) {
        self.ent[i].f71 = n;
        self.ent[i].f26 = match n {
            2 => 32,
            8 => 16,
            _ => 0,
        };
    }

    /// `sub_2B7E0` (:21273): only one m28 strikes at a time.
    fn m28_strike_taken(&self, i: usize) -> bool {
        self.ent.iter().enumerate().skip(1).any(|(j, c)| {
            j != i
                && c.class64 == 5
                && c.model65 == 28
                && c.flags & 0x400 == 0
                && matches!(c.f71, 3 | 4 | 5)
                && c.tick70 == M28_BASE + 2
        })
    }

    pub(crate) fn m28_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M28_BASE {
            0 => {
                self.mc2_patrol(i, M28_BASE);
                if self.ent[i].tick70 == M28_BASE + 2 {
                    self.ent[i].f71 = 0;
                }
            }
            1 => {
                self.mc2_idle(i, M28_BASE, ctx);
                if self.ent[i].tick70 == M28_BASE + 2 {
                    self.ent[i].f71 = 0;
                }
            }
            2 => self.m28_attack(i, ctx),
            3 => self.ent[i].tick70 = M28_BASE + 1,
            4 => self.mc2_prekill(i, M28_BASE),
            5 => {
                self.mc2_kill(i);
            }
            6 => {} // sub_2B7A0 MISSING — unreachable
            _ => {
                if self.ent[i].tick70 == M28_BASE + 2 {
                    self.ent[i].f71 = 0;
                }
            }
        }
    }

    /// `sub_2B260` (:21010) — the swing machine.
    fn m28_attack(&mut self, i: usize, ctx: &MobCtx) {
        let v1 = {
            let v = self.mc2_state_head(i);
            if v == 2 {
                self.ent[i].tick70 = M28_BASE + 4;
                return;
            }
            v
        };
        if v1 == 1 {
            self.ent[i].f146 = self.ent[i].f40;
        }
        let slot = self.ent[i].f146;
        match self.ent[i].f71 {
            0 => {
                self.m28_pose(i, 3);
                self.m28_sub(i, 1);
            }
            1 => {
                let (x, y, z) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                self.mc2_spawn_splash(x, y, z);
                self.m28_sub(i, 2);
            }
            2 => {
                let Some((tx, ty, _)) = self.mc2_target(slot, ctx) else {
                    if !self.m28_strike_taken(i) {
                        self.m28_sub(i, 3);
                    }
                    return;
                };
                self.ent[i].f26 -= 1;
                if self.ent[i].f26 <= 0 {
                    if !self.m28_strike_taken(i) {
                        self.m28_sub(i, 3);
                    }
                    return;
                }
                // Chase the point 768 ahead of the target's facing.
                let tyaw = if slot == PLAYER_TARGET {
                    ctx.pyaw
                } else {
                    self.ent[slot as usize].f30
                };
                let mut pred = (tx, ty, 0i16);
                Self::polar_step(&mut pred, tyaw, 0, 768);
                if self.ent[i].f63 & 3 == 0 {
                    self.mc2_aim_avoid(i, pred.0, pred.1);
                }
                let mv = self.mc2_move_core(i);
                if mv == 3 {
                    self.m28_sub(i, 7);
                } else if self.ent[i].f63 & 3 == 0 && self.ent[i].f26 < 14 {
                    let e = &self.ent[i];
                    let d2 = Self::dist2_sq(e.x, e.y, tx, ty);
                    if d2 < 2_768_896 && !self.m28_strike_taken(i) {
                        self.m28_sub(i, 3);
                    }
                }
            }
            3 => {
                self.m28_sub(i, 4);
                self.m28_pose(i, 2);
                self.ent[i].f50 = self.ent[i].f30 as i16;
                self.snd(38, i);
            }
            4 | 5 => {
                if self.ent[i].f26 <= 0 {
                    self.m28_sub(i, 6);
                    return;
                }
                self.ent[i].f30 = self.ent[i].f50 as u16;
                self.ent[i].f34 = self.ent[i].f30;
                if self.ent[i].f71 == 4 {
                    if let Some((tx, ty, _)) = self.mc2_target(slot, ctx) {
                        if self.ent[i].f63 & 7 == 0 {
                            let e = &self.ent[i];
                            if Self::dist2_sq(e.x, e.y, tx, ty) > 802_816 {
                                let e = &self.ent[i];
                                self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                            }
                        }
                        if (4..=12).contains(&self.ent[i].f26)
                            && self.mc2_atk_melee_768(i, slot, ctx)
                        {
                            self.ent[i].f71 = 5;
                        }
                    }
                }
                self.ent[i].f26 -= 1;
                if self.ent[i].f63 & 3 == 0 {
                    self.mc2_avoid_packmate(i);
                }
                self.mc2_move_core(i);
                self.ent[i].f50 = self.ent[i].f30 as i16;
                let swing = if self.ent[i].f26 & 4 != 0 { 56 } else { -56 };
                self.ent[i].f30 = (self.ent[i].f30 as i32 + swing) as u16 & 0x7FF;
            }
            6 => {
                self.m28_pose(i, 3);
                {
                    let (x, y, z) = {
                        let e = &self.ent[i];
                        (e.x, e.y, e.z)
                    };
                    self.mc2_spawn_splash(x, y, z);
                }
                let ok = self.mc2_target(slot, ctx).is_some_and(|(tx, ty, tz)| {
                    let e = &self.ent[i];
                    Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz))
                        < BEHAVIOR[e.row156 as usize].v_28 as u32
                });
                if ok {
                    self.m28_sub(i, 2);
                } else {
                    self.m28_sub(i, 7);
                }
            }
            7 => {
                let d = self.mc2_rand(i);
                self.ent[i].f34 = (d & 0x7FF) as u16;
                self.m28_sub(i, 8);
            }
            8 => {
                self.mc2_move_core(i);
                self.ent[i].f26 -= 1;
                if self.ent[i].f26 <= 0 {
                    self.m28_sub(i, 9);
                }
            }
            _ => {
                self.m28_pose(i, 1);
                self.ent[i].tick70 = M28_BASE + 1;
                self.ent[i].f146 = 0;
            }
        }
    }
}

/// Nearest-candidate accumulator test.
fn best_d2(best: &Option<(usize, i32)>, d2: i32) -> bool {
    best.is_none_or(|(_, bd)| d2 < bd)
}
