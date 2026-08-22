//! MC2 creature machinery: the class-5 dispatch, shared state
//! primitives, the slice creatures (Goat m1, Archers m4, Villager
//! m13) and the (9,13) archer arrow, ported from remc2
//! EventsFunctions.cpp (`:N` cites; trace bank docs/archive/PHASE3-RESEARCH.md).
//! Runs on the SHARED chassis ([`crate::engine::features::Gen`]) — same
//! pool, mailboxes, LCG, terrain samplers. MC2's NewEvent defaults
//! match MC1's field-for-field (life 300, flags dword 8, speed 16,
//! strength 100, id = slot, filter bytes -1; Events.cpp:582-599).
//!
//! Entity-field mapping (MC2 name → our [`Ent`] field):
//! `actionIndex_0x45_69`→tick70 · `byte_0x3E_62` phase→f63 (both
//! engines increment AFTER the handler) · `yaw_0x1C_28`→f30 ·
//! `pitch_0x1E_30`→f32 · `roll_0x20_32` target-yaw→f34 ·
//! `word_0x24_36` killer→f38 · `word_0x26_38` hit-source→f40 ·
//! `word_0x32_50` pack-leader→f52 · `word_0x34_52` subentity
//! chain→f54 · `byte_0x39_57` awake→f58 (0xFA dead sentinel) ·
//! `byte_0x3A_58` wake delay→f59 · `word_0x96_150` target→f146 ·
//! `actSpeed_0x82_130`→f126 · `minSpeed_0x84_132`→f128 ·
//! `maxSpeed_0x86_134`→f130 (NB: MC1's f128/f130 mean max/accel —
//! per-column semantics, handlers never cross) ·
//! `subSpellIndex_0x2A_42`→f44 · `mana_0x90_144`→f140 ·
//! `playerEntityIndex_0x94_148` sphere owner→f144 ·
//! `dword_0x10_16` scratch/invis→f26 · `word_0x5A_90` sprite-param
//! index→type86 · `array_0x52_82` {yaw,pitch,roll,fov}→
//! {f78,f80,f82,f84} · `xtype_0x41_65`→f66 · `xsubtype_0x42_66`→f67
//! (their -1 default = MC1's 0xFF filter default — aligned) ·
//! `rand_0x14_20` (u16, global_types.h:331)→rand under the U16
//! chassis · melee inbox `str_0x5E_94` {damage, attacker}→mail[0]
//! (same clear-source-keep-amount quirk as MC1, :8966) ·
//! `struct_byte_0xc` byte[0]&0x20 invisible→flags 0x20 · byte[0]&2
//! arrow-whoosh-played→flags bit 25 · byte[1]&4 disabled→flags
//! 0x400 (our reap) · byte[1]&8 forced-stop→flags bit 26 · byte[2]&4
//! blocked-status→flags bit 27 · byte[2]&0x10 no-corpse→flags
//! bit 28 · byte[2]&0x20 forced-claim lock→flags bit 29.
//!
//! Per-wizard fields shared with the MC1 column (same gameplay
//! semantics, human = out-of-pool):
//! - `word_0x248_584` (the wizard "wanted" timer, armed to 200 by
//!   offenses against the village; archers only engage wizards with
//!   it live, :11799) → [`Gen::player_aggro`] for the human — the
//!   MC1 militia gate's exact analog.
//! - `word_0x36_54` = 100 on arrow fire (:60598 sub_5EF70) → the
//!   danger-music countdown, [`Gen::player_danger`].
//!
//! DELIBERATE APPROXIMATIONS (cited, revisit as the port widens):
//! - remc2 rebuilds per-tick entity LISTS in slot order (:39930:
//!   wizards → dword_38519, per-model class-5 → bytearray_38403x
//!   skipping 0xB4/0xE8/0xEA, buildings → dword_38527). We scan the
//!   pool in slot order — identical order and tie behavior.
//! - The human wizard lives OUTSIDE the pool: wizard scans visit
//!   the human via [`MobCtx`] first (retail's list is slot-ordered
//!   with the human in slot 1), then pool class-3 wizards.
//! - The arrow's impact effect `sub_10C80(arrow, 0, subSpell)` is
//!   not yet transcribed; the port writes channel-0 area damage of
//!   `f44` through the shared mailbox writer at the impact point —
//!   the same observable (creature inboxes + the player probe).
//!   The `sub_68740` shielded-target ricochet (word[0] & 0x8010)
//!   has no shielded targets in the slice and lands with the MC2
//!   damage arm.
//! - The arrow's hit probe `sub_10780` → our tile-chain victim scan
//!   ([`Gen::victim_scan_at`]'s MC2 twin pending the class-9 pass).
//! - `TransformEntityToManaSphere` spawns spheres through the MC1
//!   (10,39) ball ctor and writes the MC2 launch fields into the
//!   MC1 ball's field homes so the shared ball tick flies them —
//!   until MC2's own (10,39) handler is diffed.
//! - `sub_20130` (archer base+6) is MISSING from the decompile
//!   (gap between //2010f0 and //201140); unreachable for archers
//!   (row flags bit 8 clear) — stubbed as hold-state.
//! - The global creature counter (`dword_0x364D2--` on the boxed-in
//!   suicide, :8860) has no reader in the slice; not tracked.

use super::behavior::{BEHAVIOR, Mc2BehaviorRow};
use super::sprite_params::SPRITE_PARAMS;
use crate::engine::features::Gen;
use crate::mc1::mobs::{MobCtx, PLAYER_TARGET};

/// MC2-only flag bits on [`Ent::flags`] (high bits; MC1 owns the low
/// ones — see the module doc mapping).
pub(crate) const F_WHOOSH: u32 = 1 << 25; // byte[0] & 2 (arrow sound played)
pub(crate) const F_STOP: u32 = 1 << 26; // byte[1] & 8 (forced stop)
pub(crate) const F_BLOCKED: u32 = 1 << 27; // byte[2] & 4 (move blocked)
pub(crate) const F_NO_CORPSE: u32 = 1 << 28; // byte[2] & 0x10
/// byte[2] & 0x20 — the Mana Lock claim lock (EF:28026/26084): set by
/// a FORCED claim ((10,70) pulse, possession tier 2); a locked target
/// ignores weak claims — only another forced claim steals it.
pub(crate) const F_CLAIM_LOCK: u32 = 1 << 29;

const GOAT_BASE: u8 = 8;
const ARCHER_BASE: u8 = 32;
const VILLAGER_BASE: u8 = 104;
/// The arrow's action/state (= its model; :35031).
const ARROW_STATE: u8 = 13;

impl Gen {
    // ---- shared MC2 helpers ------------------------------------------------

    /// One u16 LCG draw (`rand_0x14_20 = 9377*x + 9439`); the
    /// chassis-selected [`Gen::ent_rand`] does exactly this under
    /// RandWidth::U16.
    pub(crate) fn mc2_rand(&mut self, i: usize) -> u32 {
        self.ent_rand(i)
    }

    /// The `rand_0x14 += setting_30` per-entity stream perturb —
    /// retail's ONLY three sites: the pyramid's two pick rolls
    /// (EF:13140/13220) and the m27 branch bolt (EF:20521). Applied
    /// AFTER the modulo draw, so the current roll reads the clean
    /// LCG value and the NEXT roll starts from the shifted seed.
    /// `turn` = `MobCtx::mc2_turn` (the post-increment counter the
    /// cave carpet tail's corpus solve anchored, EF:59803).
    pub(crate) fn mc2_rand_perturb(&mut self, i: usize, turn: u32) {
        self.ent[i].rand = self.ent[i].rand.wrapping_add(turn) & 0xFFFF;
    }

    /// `SetEntityIndexAndRot_49CD0` (:32837): store the sprite-param
    /// row and derive the rot/extent quad from it (/2). No RNG.
    pub(crate) fn mc2_set_sprite(&mut self, i: usize, idx: u16) {
        let (s6, r8) = self.mc2_params_ext(idx as usize);
        let e = &mut self.ent[i];
        e.type86 = idx;
        e.frame88 = 0;
        e.f78 = r8 / 2; // array.yaw
        e.f80 = s6 / 2; // array.pitch
        e.f82 = s6 / 2; // array.roll
        e.f84 = r8 / 2; // array.fov
    }

    /// The (speed_6, rotSpeed_8) pair for a particle-param row —
    /// the DERIVED table when the dims-fed assets carry it
    /// ([`crate::mc2::derive_sprite_extents`]), else the raw static
    /// row (pre-dims callers keep the old behavior).
    pub(crate) fn mc2_params_ext(&self, idx: usize) -> (u16, u16) {
        self.assets
            .mc2_sprite_ext
            .get(idx)
            .copied()
            .unwrap_or_else(|| {
                let p = &SPRITE_PARAMS[idx];
                (p.speed_6, p.rot_speed_8)
            })
    }

    /// `sub_49E10` (:32865): sprite + the quad doubled (the arrow's
    /// call with 195).
    pub(crate) fn mc2_set_sprite_x2(&mut self, i: usize, idx: u16) {
        self.mc2_set_sprite(i, idx);
        let e = &mut self.ent[i];
        e.f80 *= 2;
        e.f82 *= 2;
        e.f84 *= 2;
    }

    /// `SetEntityShiftRot_49EA0` (:32874): pitch = roll = shift,
    /// fov = fov.
    pub(crate) fn mc2_shift_rot(&mut self, i: usize, shift: u16, fov: u16) {
        let e = &mut self.ent[i];
        e.f80 = shift;
        e.f82 = shift;
        e.f84 = fov;
    }

    /// `SetEvent144_49C70` (:32826): mana = maxLife >> 1.
    pub(crate) fn mc2_set_mana_half(&mut self, i: usize) {
        self.ent[i].f140 = (self.ent[i].max_life >> 1) as i32;
    }

    /// `sub_580E0` (:40372): sink by the row's zStep while above
    /// ground, clamp to ground + hover.
    fn mc2_alt_core(z: &mut i16, ground: i16, hover: i16, z_step: i16) {
        if *z > ground {
            *z = z.wrapping_add(z_step);
        }
        if *z <= ground.wrapping_add(hover) {
            *z = ground.wrapping_add(hover);
        }
    }

    /// `sub_1EEE0` (:11172): altitude commit at the current position.
    pub(crate) fn mc2_alt_commit(&mut self, i: usize) {
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let (hover, z_step) = (row.v_12, row.v_14);
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let ground = self.ground_z(x, y) as i16;
        let mut z = self.ent[i].z;
        Self::mc2_alt_core(&mut z, ground, hover, z_step);
        self.move_relink(i, x, y, z);
    }

    /// `sub_102D0` with a3 = 1 (:3632): walk up to max(array.pitch,
    /// array.roll) units along yaw in 256 steps; blocked when a
    /// tile's capability bit falls outside the row's permission
    /// mask, and on caves also when the probe tile is bit3-SEALED or
    /// the ceiling poke test fires (:3674-83).
    pub(crate) fn mc2_path_blocked(&self, i: usize, from: (u16, u16, i16)) -> bool {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let reach = (e.f80).max(e.f82) as i32;
        let mut pos = from;
        let mut walked = 0i32;
        // Retail loop shape `while (walked <= reach) { probe; step }`
        // (:3659-3686): for walker extents <= 255 that is exactly ONE
        // probe at the predicted point. Order matters — testing after
        // the step probes an extra 256-step point and false-blocks a
        // tile early (1-tile causeways).
        loop {
            if walked > reach {
                return false;
            }
            if !row.v_20 & self.cap_bit(pos.0, pos.1) != 0 {
                return true;
            }
            if self.is_cave() {
                let t = crate::engine::features::tile((pos.0 >> 8) as u8, (pos.1 >> 8) as u8);
                if self.t.angle[t] & 8 != 0
                    || self.cave_poke(e.f84 as i32, row.v_12 as i32, pos.0, pos.1)
                {
                    return true;
                }
            }
            walked += 256;
            Self::polar_step(&mut pos, self.ent[i].f30, 0, 256);
        }
    }

    /// Diagnostic (the flocking terrain-fence check): the whole-map
    /// walkability of creature `i`'s behavior row, one byte per tile —
    /// bit 0 = roughness >= v_16 (the slope fence), bit 1 = tile-type
    /// blocked (`!v_20 & cap_bit`). Probes tile centers.
    pub(crate) fn mc2_block_map(&self, i: usize) -> Vec<u8> {
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let mut out = vec![0u8; 256 * 256];
        for ty in 0..256u16 {
            for tx in 0..256u16 {
                let (x, y) = (tx * 256 + 128, ty * 256 + 128);
                let mut b = 0u8;
                if self.roughness(x, y) >= row.v_16 as i32 {
                    b |= 1;
                }
                if !row.v_20 & self.cap_bit(x, y) != 0 {
                    b |= 2;
                }
                out[(ty as usize) << 8 | tx as usize] = b;
            }
        }
        out
    }

    /// One predicted candidate of the MC2 move core: altitude core +
    /// polar step at the CURRENT yaw, then the block test (crossing
    /// into a new tile only).
    /// `always_test`: the retry predictions run the block/roughness
    /// test UNCONDITIONALLY (EF:8826/8840/8852) — only the FIRST
    /// prediction gates it on the tile change (EF:8806). A rotated
    /// retry that stays in-tile must still be terrain-tested.
    fn mc2_move_candidate(&self, i: usize, always_test: bool) -> ((u16, u16, i16), bool) {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let mut pos = (e.x, e.y, e.z);
        let ground = self.ground_z(pos.0, pos.1) as i16;
        Self::mc2_alt_core(&mut pos.2, ground, row.v_12, row.v_14);
        Self::polar_step(&mut pos, e.f30, 0, e.f126);
        let crossed = e.x >> 8 != pos.0 >> 8 || e.y >> 8 != pos.1 >> 8;
        let blocked = (always_test || crossed)
            && (self.mc2_path_blocked(i, pos) || self.roughness(pos.0, pos.1) >= row.v_16 as i32);
        (pos, blocked)
    }

    /// `sub_1B8C0` (:8741): the MC2 creature move core. Result codes
    /// 1 same-tile / 2 moved / 3 moved-after-retry / 4 blocked. The
    /// retry yaws replicate the decompile's byte arithmetic verbatim
    /// — including the third retry's C precedence quirk.
    pub(crate) fn mc2_move_core(&mut self, i: usize) -> u8 {
        if self.ent[i].flags & F_STOP != 0 {
            self.ent[i].flags &= !F_STOP;
            return 4;
        }
        // The commit turn is clamped by row v_2 (goat 45, villager 22
        // per tick): sub_58350's v_4 arg is DEAD in retail, the real
        // clamp is subtype_160_0x2_2 (EF:8868-75 + 40391-405; MC1's
        // creature_move already uses its v_2 twin). NOT v_4 (=5),
        // which under-turns 4-9x and can't catch the wander heading.
        let turn_cap = BEHAVIOR[self.ent[i].row156 as usize].v_2;
        fn commit(g: &mut Gen, i: usize, pos: (u16, u16, i16), cap: i16) {
            g.move_relink(i, pos.0, pos.1, pos.2);
            let e = &g.ent[i];
            let turned = (e.f30 as i32 + Gen::turn_step(e.f30, e.f34, cap) as i32) as u16;
            g.ent[i].f30 = turned & 0x7FF;
        }

        let (pos, blocked) = self.mc2_move_candidate(i, false);
        let same_tile = self.ent[i].x >> 8 == pos.0 >> 8 && self.ent[i].y >> 8 == pos.1 >> 8;
        if same_tile {
            commit(self, i, pos, turn_cap);
            self.ent[i].flags &= !F_BLOCKED;
            return 1;
        }
        if !blocked {
            commit(self, i, pos, turn_cap);
            self.ent[i].flags &= !F_BLOCKED;
            return 2;
        }
        self.ent[i].flags |= F_BLOCKED;
        let yaw0 = self.ent[i].f30;
        // Retry 1: +341 (:8815).
        self.ent[i].f30 = yaw0.wrapping_add(341) & 0x7FF;
        let (pos, blocked) = self.mc2_move_candidate(i, true);
        if !blocked {
            commit(self, i, pos, turn_cap);
            return 3;
        }
        // Retry 2: LOBYTE = yaw0-85, HIBYTE = ((yaw0-341)>>8)&7 —
        // verbatim byte split (:8890-92).
        let lo = yaw0.wrapping_sub(85) as u8;
        let hi = ((yaw0.wrapping_sub(341) >> 8) & 7) as u8;
        self.ent[i].f30 = u16::from_le_bytes([lo, hi]);
        let (pos, blocked) = self.mc2_move_candidate(i, true);
        if !blocked {
            commit(self, i, pos, turn_cap);
            return 3;
        }
        // Retry 3: (yaw0 + 0x400) & (0x700 + LOBYTE(yaw0)) — the
        // decompile's precedence quirk kept verbatim (:8846).
        self.ent[i].f30 = yaw0.wrapping_add(0x400) & (0x700 + (yaw0 & 0xFF));
        let (pos, blocked) = self.mc2_move_candidate(i, true);
        if !blocked {
            commit(self, i, pos, turn_cap);
            return 3;
        }
        // All four blocked (:8855-62): die-on-water/boxed-in suicide.
        let row_flags = BEHAVIOR[self.ent[i].row156 as usize].flags;
        let on_water = self.cap_bit(self.ent[i].x, self.ent[i].y) == 1;
        if row_flags & Mc2BehaviorRow::DIE_ON_WATER != 0 || on_water {
            self.ent[i].act_life = -1;
        }
        4
    }

    /// The shared inbox/life head opening every MC2 state handler
    /// (:8960-8998 pattern): apply the melee mailbox (clear source,
    /// KEEP amount — the MC1 quirk, :8966), inherit the weakest
    /// linked-subentity life, latch killer on death. Returns
    /// 0 quiet / 1 hit / 2 dead.
    pub(crate) fn mc2_state_head(&mut self, i: usize) -> u8 {
        let mut v = 0u8;
        if self.ent[i].mail[0].1 != 0 {
            let (amt, src) = self.ent[i].mail[0];
            self.ent[i].act_life -= amt as i32;
            self.ent[i].mail[0].1 = 0;
            self.ent[i].f40 = src;
            v = 1;
        } else {
            self.ent[i].f40 = 0;
        }
        let mut j = self.ent[i].f54 as usize;
        while j != 0 {
            if self.ent[j].act_life < self.ent[i].act_life {
                self.ent[i].act_life = self.ent[j].act_life;
                self.ent[i].f40 = self.ent[j].f40;
                v = 1;
                break;
            }
            j = self.ent[j].f54 as usize;
        }
        if self.ent[i].act_life < 0 {
            self.ent[i].f38 = self.ent[i].f40;
            v = 2;
        }
        v
    }

    /// The two-draw wander-turn idiom (:9136-38 and twins): `v =
    /// rand; rand; f34 += ((rand & 0xFF) + 85) * (2*((v % 0x9D)/79)
    /// - 1); f34 &= 0x7FF`.
    pub(crate) fn mc2_wander_turn(&mut self, i: usize) {
        let v = self.mc2_rand(i);
        let r = self.mc2_rand(i);
        let sign = 2 * ((v % 0x9D) / 79) as i32 - 1;
        let step = ((r & 0xFF) + 85) as i32 * sign;
        self.ent[i].f34 = (self.ent[i].f34 as i32 + step) as u16 & 0x7FF;
    }

    /// Arm the wizard "wanted" timer (`word_0x248_584 = 200`) on a
    /// hit/kill source when it is a wizard — the human maps to the
    /// shared aggro register, pool wizards to the hash-quiet
    /// `mc2_wanted` side channel.
    pub(crate) fn mc2_arm_wanted(&mut self, src: u16) {
        if src == PLAYER_TARGET {
            self.player_aggro = 200;
        } else {
            let j = src as usize;
            if j > 0 && j < self.ent.len() && self.ent[j].class64 == 3 && self.ent[j].model65 <= 1 {
                self.mc2_wanted.0.insert(src, 200);
            }
        }
    }

    /// Is `slot`'s wanted timer live? (the archer Scan-A post-reject
    /// gate, :11799-802.)
    pub(crate) fn mc2_wanted_live(&self, slot: u16) -> bool {
        if slot == PLAYER_TARGET {
            self.player_aggro > 0
        } else {
            self.mc2_wanted.0.get(&slot).is_some_and(|&t| t > 0)
        }
    }

    /// The full class-3 pool walk shared by the archer's Scan A
    /// (:11768-95) and m24 acquire (sub_28690 :18744-64): nearest
    /// class-3 ANYTHING (wizards, castles, balloons) with `d2 <=
    /// v_28²`, cone `< v_30`, skipping only invisibles (byte[0] &
    /// 0x20). The human wizard sits in retail's dword_38519 like any
    /// pool entity, so the out-of-pool pseudo-target joins the walk.
    pub(crate) fn mc2_class3_scan(&self, i: usize, ctx: &MobCtx) -> Option<u16> {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let range = (row.v_28 as i32) * (row.v_28 as i32);
        let cone = row.v_30 as u16;
        let (ex, ey, eyaw) = (e.x, e.y, e.f30);
        let mut best: Option<(u16, i32)> = None;
        let mut consider = |tx: u16, ty: u16, slot: u16| {
            let d2 = Self::dist2_sq(ex, ey, tx, ty);
            if d2 > range {
                return;
            }
            let bearing = Self::angle_between(ex, ey, tx, ty);
            if Self::angdist(eyaw, bearing) >= cone {
                return;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((slot, d2));
            }
        };
        if !self.player_invisible {
            consider(ctx.px, ctx.py, PLAYER_TARGET);
        }
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if c.class64 == 3 && c.act_life >= 0 && c.flags & 0x400 == 0 && c.flags & 0x20 == 0 {
                consider(c.x, c.y, j as u16);
            }
        }
        best.map(|(s, _)| s)
    }

    /// The wizard-target scan of `sub_1BF90` (:9152-95): nearest
    /// live wizard within range and FOV cone, skipping invisibles
    /// (byte[0] & 0x20). `wanted_only` = the archer brain's extra
    /// gate (target's word_0x248_584 must be live, :11799).
    pub(crate) fn mc2_wizard_scan(&self, i: usize, ctx: &MobCtx, wanted_only: bool) -> Option<u16> {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let range = (row.v_28 as i32) * (row.v_28 as i32);
        let cone = row.v_30 as u16;
        let (ex, ey, eyaw) = (e.x, e.y, e.f30);
        let mut best: Option<(u16, i32)> = None;
        let consider = |tx: u16, ty: u16, slot: u16, skip: bool, best: &mut Option<(u16, i32)>| {
            if skip {
                return;
            }
            let d2 = Self::dist2_sq(ex, ey, tx, ty);
            if d2 > range {
                return;
            }
            let ty_yaw = Self::angle_between(ex, ey, tx, ty);
            if Self::angdist(eyaw, ty_yaw) >= cone {
                return;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                *best = Some((slot, d2));
            }
        };
        let human_skip = self.player_invisible || (wanted_only && self.player_aggro <= 0);
        consider(ctx.px, ctx.py, PLAYER_TARGET, human_skip, &mut best);
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if c.class64 == 3 && c.model65 <= 1 && c.act_life >= 0 && c.flags & 0x400 == 0 {
                // Pool wizards carry no wanted timer yet (see
                // mc2_arm_wanted) — under wanted_only they never
                // qualify, faithful to an unarmed timer.
                consider(
                    c.x,
                    c.y,
                    j as u16,
                    c.flags & 0x20 != 0 || wanted_only,
                    &mut best,
                );
            }
        }
        best.map(|(s, _)| s)
    }

    /// The same-model pack scan (:9197-9231): nearest leaderless
    /// same-model creature in range + cone. `reversed_cone` = the +0
    /// patrol quirk (:9038): its cone test uses the REVERSED bearing
    /// `tan2(candidate → self)`, unlike wander's `tan2(self →
    /// candidate)` (:9194) — vestigial for goats/townies (they never
    /// occupy +0) but kept verbatim.
    pub(crate) fn mc2_pack_scan(&self, i: usize, reversed_cone: bool) -> Option<u16> {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let range = (row.v_28 as i32) * (row.v_28 as i32);
        let cone = row.v_30 as u16;
        let mut best: Option<(u16, i32)> = None;
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if j == i
                || c.class64 != 5
                || c.model65 != e.model65
                || c.f52 != 0
                || c.act_life < 0
                || matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
                || c.flags & 0x400 != 0
            {
                continue;
            }
            let d2 = Self::dist2_sq(e.x, e.y, c.x, c.y);
            if d2 > range {
                continue;
            }
            let ty_yaw = if reversed_cone {
                Self::angle_between(c.x, c.y, e.x, e.y)
            } else {
                Self::angle_between(e.x, e.y, c.x, c.y)
            };
            if Self::angdist(e.f30, ty_yaw) >= cone {
                continue;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j as u16, d2));
            }
        }
        best.map(|(s, _)| s)
    }

    /// The same-model AVOIDANCE override in chase/flee re-aims
    /// (:9643-56): first packmate closer than array.pitch on both
    /// axes steers us away from it.
    pub(crate) fn mc2_avoid_packmate(&mut self, i: usize) {
        let (ex, ey, pitch, model, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.f80 as i32, e.model65, e.id24)
        };
        if pitch == 0 {
            return;
        }
        for c in self.ent.iter().skip(1) {
            if c.class64 == 5
                && c.model65 == model
                && c.id24 != id
                // Retail iterates the LIVE per-model bucket — the
                // dying never appear (EF:9641-50); the full-array
                // walk needs the explicit life gate.
                && c.act_life >= 0
                && !matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
                && c.flags & 0x400 == 0
                && ((ex.wrapping_sub(c.x)) as i16 as i32).abs() < pitch
                && ((ey.wrapping_sub(c.y)) as i16 as i32).abs() < pitch
            {
                let away = Self::angle_between(c.x, c.y, ex, ey);
                self.ent[i].f34 = away;
                break;
            }
        }
    }

    /// Resolve a target slot to (x, y, z) — `sub_1ED30`'s validation
    /// core for StageVar2 == 0 spawns (:11060: non-14 stage vars
    /// return the candidate; the caller then rejects dead/reaped).
    pub(crate) fn mc2_target(&self, slot: u16, ctx: &MobCtx) -> Option<(u16, u16, i16)> {
        if slot == PLAYER_TARGET {
            return Some((ctx.px, ctx.py, ctx.pz));
        }
        let j = slot as usize;
        if j == 0 || j >= self.ent.len() {
            return None;
        }
        let t = &self.ent[j];
        if t.class64 == 0 || t.act_life < 0 || t.flags & 0x400 != 0 {
            return None;
        }
        Some((t.x, t.y, t.z))
    }

    /// 3D distance (`sub_583F0`, 16-bit deltas).
    pub(crate) fn mc2_dist3(a: (u16, u16, i16), b: (u16, u16, i16)) -> u32 {
        let dx = (b.0.wrapping_sub(a.0)) as i16 as i32;
        let dy = (b.1.wrapping_sub(a.1)) as i16 as i32;
        let dz = (b.2 as i32) - (a.2 as i32);
        Self::isqrt((dx * dx + dy * dy + dz * dz) as u32)
    }

    /// `sub_1BD90` (:8945) — PATROL: inbox/life head, transitions,
    /// pack detection on the row cadence. No movement; altitude
    /// commit on the quiet and hit paths.
    pub(crate) fn mc2_patrol(&mut self, i: usize, base: u8) {
        match self.mc2_state_head(i) {
            1 => {
                self.ent[i].f146 = self.ent[i].f40;
                let flee = BEHAVIOR[self.ent[i].row156 as usize].flags & Mc2BehaviorRow::FLEE != 0;
                self.ent[i].tick70 = base + if flee { 6 } else { 2 };
                self.mc2_alt_commit(i);
            }
            2 => {
                self.ent[i].tick70 = base + 4;
                self.mc2_alt_commit(i);
            }
            _ => {
                let row = &BEHAVIOR[self.ent[i].row156 as usize];
                let pack_ok = row.flags & Mc2BehaviorRow::PACK_DISABLE == 0;
                let period = row.v_26.max(1) as u8;
                if pack_ok && self.ent[i].f63 % period == 0 {
                    if let Some(l) = self.mc2_pack_scan(i, true) {
                        self.ent[i].f52 = l;
                        self.ent[i].tick70 = base + 3;
                    }
                }
                self.mc2_alt_commit(i);
            }
        }
    }

    /// `sub_1BF90` (:9064) — IDLE/WANDER (the spawn state): inbox
    /// head, move, wander turn + wizard scan on the row cadence
    /// (scan gated on the awake byte), pack fallback.
    pub(crate) fn mc2_idle(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        match self.mc2_state_head(i) {
            1 => {
                self.ent[i].f146 = self.ent[i].f40;
                let flee = BEHAVIOR[self.ent[i].row156 as usize].flags & Mc2BehaviorRow::FLEE != 0;
                self.ent[i].tick70 = base + if flee { 6 } else { 2 };
                self.mc2_alt_commit(i);
            }
            2 => self.ent[i].tick70 = base + 4,
            _ => {
                self.mc2_move_core(i);
                let row = &BEHAVIOR[self.ent[i].row156 as usize];
                let period = row.v_26.max(1) as u8;
                if self.ent[i].f63 % period == 0 {
                    self.mc2_wander_turn(i);
                    if self.ent[i].f58 != 0 {
                        if let Some(t) = self.mc2_wizard_scan(i, ctx, false) {
                            self.ent[i].f146 = t;
                            let flee = BEHAVIOR[self.ent[i].row156 as usize].flags
                                & Mc2BehaviorRow::FLEE
                                != 0;
                            self.ent[i].tick70 = base + if flee { 6 } else { 2 };
                        } else if BEHAVIOR[self.ent[i].row156 as usize].flags
                            & Mc2BehaviorRow::PACK_DISABLE
                            == 0
                            && let Some(l) = self.mc2_pack_scan(i, false)
                        {
                            self.ent[i].f52 = l;
                            self.ent[i].tick70 = base + 3;
                        }
                    }
                }
            }
        }
    }

    /// `sub_1C560` (:9345) — PACK-FOLLOW: validate the leader,
    /// inbox head (transitions also RETARGET the leader), then on
    /// the cadence copy the leader's state/target and match its
    /// speed (leader max + act, :9482).
    pub(crate) fn mc2_pack(&mut self, i: usize, base: u8) {
        if self.ent[i].f52 == 0 {
            self.ent[i].tick70 = base + 1;
            return;
        }
        let l = self.ent[i].f52 as usize;
        let leader_ok = l != 0
            && l < self.ent.len()
            && self.ent[l].act_life >= 0
            && self.ent[l].flags & 0x400 == 0
            && self.ent[l].class64 == self.ent[i].class64
            && self.ent[l].model65 == self.ent[i].model65;
        let v = self.mc2_state_head(i);
        match v {
            1 | 2 => {
                // The leader inherits our attacker as its target
                // (:9500-9516) before we transition.
                if leader_ok {
                    let flee =
                        BEHAVIOR[self.ent[l].row156 as usize].flags & Mc2BehaviorRow::FLEE != 0;
                    self.ent[l].f146 = self.ent[i].f40;
                    self.ent[l].f52 = 0;
                    self.ent[l].tick70 = base + if flee { 6 } else { 2 };
                }
                if v == 2 {
                    self.ent[i].f52 = 0;
                    self.ent[i].tick70 = base + 4;
                } else {
                    let flee =
                        BEHAVIOR[self.ent[i].row156 as usize].flags & Mc2BehaviorRow::FLEE != 0;
                    self.ent[i].f146 = self.ent[i].f40;
                    self.ent[i].f52 = 0;
                    self.ent[i].tick70 = base + if flee { 6 } else { 2 };
                    self.mc2_alt_commit(i);
                }
            }
            _ => {
                self.mc2_move_core(i);
                if !leader_ok {
                    self.ent[i].f52 = 0;
                    self.ent[i].tick70 = base + 1;
                    return;
                }
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                if self.ent[i].f63 % period == 0 {
                    let lrole = self.ent[l].tick70.wrapping_sub(base);
                    match lrole {
                        0 | 1 | 3 => {
                            if lrole == 3 {
                                self.ent[i].f52 = self.ent[l].f52;
                            }
                            // Aim at the (possibly re-linked) leader
                            // and sidestep a crowding packmate
                            // (:9455-77, threshold 256).
                            let ll = self.ent[i].f52 as usize;
                            if ll != 0 && ll < self.ent.len() {
                                let e = &self.ent[i];
                                self.ent[i].f34 =
                                    Self::angle_between(e.x, e.y, self.ent[ll].x, self.ent[ll].y);
                                let (ex, ey, model, id) = {
                                    let e = &self.ent[i];
                                    (e.x, e.y, e.model65, e.id24)
                                };
                                for c in self.ent.iter().skip(1) {
                                    if c.class64 == 5
                                        && c.model65 == model
                                        && c.id24 != id
                                        && !matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
                                        && c.flags & 0x400 == 0
                                        && ((ex.wrapping_sub(c.x)) as i16 as i32).abs() < 256
                                        && ((ey.wrapping_sub(c.y)) as i16 as i32).abs() < 256
                                    {
                                        self.ent[i].f34 = Self::angle_between(c.x, c.y, ex, ey);
                                        break;
                                    }
                                }
                                // Catch-up: leader max + act (:9482) —
                                // both operands from the LEADER.
                                self.ent[i].f126 = self.ent[l].f130 + self.ent[l].f126;
                            }
                        }
                        2 => {
                            self.ent[i].f146 = self.ent[l].f146;
                            self.ent[i].f52 = 0;
                            self.ent[i].tick70 = base + 2;
                        }
                        6 => {
                            self.ent[i].f146 = self.ent[l].f146;
                            self.ent[i].f52 = 0;
                            self.ent[i].tick70 = base + 6;
                        }
                        _ => {
                            self.ent[i].f52 = 0;
                            self.ent[i].tick70 = base + 1;
                        }
                    }
                }
            }
        }
    }

    /// `sub_1C980` (:9572) — FLEE: inbox head, move, re-aim AWAY
    /// every 4th phase (`HIBYTE += 4` = the 180° flip) with the
    /// packmate avoidance; drop to patrol when the threat dies or
    /// leaves range on the cadence tick.
    pub(crate) fn mc2_flee(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        match self.mc2_state_head(i) {
            1 => {
                self.ent[i].f146 = self.ent[i].f40;
                self.mc2_alt_commit(i);
            }
            2 => self.ent[i].tick70 = base + 4,
            _ => {
                self.mc2_move_core(i);
                let Some((tx, ty, tz)) = self.mc2_target(self.ent[i].f146, ctx) else {
                    self.ent[i].tick70 = base + 1;
                    return;
                };
                if self.ent[i].f63 & 3 == 0 {
                    let e = &self.ent[i];
                    let away = Self::angle_between(e.x, e.y, tx, ty).wrapping_add(0x400) & 0x7FF;
                    self.ent[i].f34 = away;
                    self.mc2_avoid_packmate(i);
                }
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                if self.ent[i].f63 % period == 0 {
                    let e = &self.ent[i];
                    let d3 = Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz));
                    if d3 >= BEHAVIOR[self.ent[i].row156 as usize].v_28 as u32 {
                        self.ent[i].tick70 = base + 1;
                    }
                }
            }
        }
    }

    /// `sub_1C310` (:9240) — CHASE-AND-ATTACK: inbox head, move,
    /// re-aim at the target every 4th phase (packmate avoidance),
    /// and on the cadence drop the chase (out of range → base+1) or
    /// fire the thunk. Returns true when the thunk fired.
    pub(crate) fn mc2_chase_attack(
        &mut self,
        i: usize,
        base: u8,
        ctx: &MobCtx,
        attack: fn(&mut Self, usize, u16, &MobCtx) -> bool,
    ) -> bool {
        match self.mc2_state_head(i) {
            1 => {
                self.ent[i].f146 = self.ent[i].f40;
                self.mc2_alt_commit(i);
                false
            }
            2 => {
                self.ent[i].tick70 = base + 4;
                false
            }
            _ => {
                self.mc2_move_core(i);
                let slot = self.ent[i].f146;
                let Some((tx, ty, tz)) = self.mc2_target(slot, ctx) else {
                    self.ent[i].tick70 = base + 1;
                    return false;
                };
                if self.ent[i].f63 & 3 == 0 {
                    let e = &self.ent[i];
                    self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                    self.mc2_avoid_packmate(i);
                }
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                if self.ent[i].f63 % period == 0 {
                    let e = &self.ent[i];
                    let d3 = Self::mc2_dist3((e.x, e.y, e.z), (tx, ty, tz));
                    if d3 >= BEHAVIOR[self.ent[i].row156 as usize].v_28 as u32 {
                        self.ent[i].tick70 = base + 1;
                        return false;
                    }
                    return attack(self, i, slot, ctx);
                }
                false
            }
        }
    }

    /// `PreKillEntity_1C890` (:9533): chain subentities to state+5,
    /// inherit their killer latch, kill credit (player killer,
    /// victim model NOT in {9, 12, 13, 14, 15}), then state+5.
    pub(crate) fn mc2_prekill(&mut self, i: usize, base: u8) {
        let mut j = self.ent[i].f54 as usize;
        while j != 0 {
            self.ent[j].tick70 = base + 5;
            if self.ent[j].f38 != 0 {
                self.ent[i].f38 = self.ent[j].f38;
            }
            j = self.ent[j].f54 as usize;
        }
        let killer = self.ent[i].f38;
        let model = self.ent[i].model65;
        // PreKillEntity_1C890 (EF:9543-51): credit gates on killer
        // class-3 MODEL-0 (the human avatar only — rivals are (3,1)
        // and never score creature kills) AND the SELF-ID check:
        // killing your own creature earns nothing.
        if killer == PLAYER_TARGET
            && self.ent[i].id24 != PLAYER_TARGET
            && !matches!(model, 9 | 12 | 13 | 14 | 15)
        {
            self.kills += 1;
        }
        self.ent[i].tick70 = base + 5;
    }

    /// `KillEntity_1C930` (:9556): every 8th phase — mana spheres +
    /// the (10,1) corpse burst + reap.
    pub(crate) fn mc2_kill(&mut self, i: usize) {
        if self.ent[i].f63 & 7 != 0 {
            return;
        }
        self.mc2_mana_spheres(i, false);
        if self.ent[i].flags & F_NO_CORPSE == 0 {
            // The (10,1) corpse burst.
            self.mc2_corpse_burst(i);
        }
        self.ent[i].flags |= 0x400;
    }

    /// `TransformEntityToManaSphere_36BA0` (:26867), verbatim
    /// draws/order: one corpse draw before the loop; per sphere —
    /// draw #1 → yaw = (rand % 0x71 + heading − 56) & 0x7FF, draw
    /// #2 → speed = rand % 0x30 + 16; fall = signed (1024 − zdiff)/8.
    /// Spheres allocate through the shared (10,39) ball ctor and
    /// write the launch into the MC1 ball's field homes so the
    /// shared ball tick flies them (module-doc APPROX).
    pub(crate) fn mc2_mana_spheres(&mut self, i: usize, use_fraction: bool) {
        if self.ent[i].f140 <= 0 {
            return;
        }
        let total = self.ent[i].f140;
        let (fraction, loc) = if use_fraction {
            let f = (total / 1000).clamp(1, 16);
            (f, total / f)
        } else {
            (1, total)
        };
        let (x, y, z, heading, owner) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f144)
        };
        let _ = self.mc2_rand(i); // the pre-loop corpse draw (:26884)
        let ground = self.ground_z(x, y) as i16;
        for n in 0..fraction {
            let Some(b) = self.spawn_mana_ball(x, y, z) else {
                continue;
            };
            self.ent[b].f140 = if n == fraction - 1 {
                total - (fraction - 1) * loc
            } else {
                loc
            };
            self.ent[b].f144 = owner;
            let d1 = self.mc2_rand(b);
            let yaw = ((d1 % 0x71) as i32 + heading as i32 - 56) as u16 & 0x7FF;
            self.ent[b].f30 = yaw;
            self.ent[b].f34 = yaw;
            let d2 = self.mc2_rand(b);
            let speed = (d2 % 0x30 + 16) as i16;
            // Retail WRITES the roll onto the sphere's actSpeed@0x82
            // (EF:26907), not just into the velocity step — the
            // scattered spheres carry 16..63, the ctor's 32 is only
            // the un-scattered default (mc2l3 t=245-260: every
            // crush-scatter sphere read 32).
            self.ent[b].f126 = speed;
            // Velocity into the MC1 ball's dest fields (the shared
            // ball tick consumes them), fall arc into f46 — signed
            // TRUNCATING /8 like the C idiom at EF:26909 (NOT
            // div_euclid, which floors: off by one for deaths > 1024
            // above terrain), NO clamp (MC1 clamps ≥ 0; MC2 does not).
            let mut v = (0u16, 0u16, 0i16);
            Self::polar_step(&mut v, yaw, 0, speed);
            self.ent[b].dest_x = v.0;
            self.ent[b].dest_y = v.1;
            let zdiff = (z as i32) - (ground as i32);
            self.ent[b].f46 = ((1024 - zdiff) / 8) as i16;
        }
        // `TransformEntityToManaSphere_36BA0`'s tail zeroes ONLY
        // `playerEntityIndex_0x94` — the corpse KEEPS its mana
        // (mc2l3 t=245: crushed firebug corpses read 300 until the
        // reap; the invented f140 wipe dirtied every corpse pair).
        // No double-scatter: every caller raises 0x400 right after.
        self.ent[i].f144 = 0;
    }

    // ---- spawn ctors -------------------------------------------------------

    /// `AddCreature_4B490` (:33720) — the Goat (5,1). NO ctor RNG.
    pub(crate) fn mc2_spawn_goat(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 1;
            e.tick70 = GOAT_BASE + 1; // actionIndex 9
            // MC2 carries NO per-channel vulnerability mask — its
            // single damage gate is byte[0] & 8 (mapped to flags & 8,
            // the shared NewEvent default). MC1's writers additionally
            // check the +28 channel mask; admit their physical channel
            // at the seam (cross-column damage contract).
            e.f28 = 1;
            e.f128 = 54; // minSpeed
            e.f130 = 18; // maxSpeed
            e.f126 = 18; // actSpeed = maxSpeed
            e.max_life = 600;
        }
        self.mc2_set_mana_half(i); // 300
        {
            let e = &mut self.ent[i];
            e.f34 = 0;
            e.f30 = 0;
            e.f32 = 0;
            e.f26 = (i % 100) as i16;
            e.row156 = 98; // ABSOLUTE row index (:33739)
        }
        self.ent[i].f58 = BEHAVIOR[98].v_26 + 1;
        // Per-model spawn ordinal → f63 (:33740) — de-syncs the herd
        // cadence (else every `f63 & N` gate runs in lockstep at 0).
        self.ent[i].f63 = self.mc2_ord(1);
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 238);
        Some(i)
    }

    /// `AddArchers_4BA10` (:33878) — the Archers (5,4). ONE ctor RNG
    /// draw → facing.
    pub(crate) fn mc2_spawn_archers(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 4;
            e.tick70 = ARCHER_BASE + 1; // actionIndex 33
            // MC2 carries NO per-channel vulnerability mask — its
            // single damage gate is byte[0] & 8 (mapped to flags & 8,
            // the shared NewEvent default). MC1's writers additionally
            // check the +28 channel mask; admit their physical channel
            // at the seam (cross-column damage contract).
            e.f28 = 1;
            e.f128 = 30; // minSpeed
            e.f130 = 0; // maxSpeed — STATIONARY
            e.f126 = 30;
            e.max_life = 1000;
        }
        self.mc2_set_mana_half(i); // 500
        let d = self.mc2_rand(i);
        {
            let e = &mut self.ent[i];
            let f = ((d & 0x7FF) as i32 - 1) as u16;
            e.f34 = f;
            e.f30 = f;
            e.f32 = f;
            e.f44 = 500;
            e.row156 = 75; // ABSOLUTE row index (:33899)
        }
        // Ordinal FIRST (:33900) — it feeds the wake stagger on the
        // very next line; unset f63 collapses f58 to the constant
        // period+4 (no stagger, degenerate archer wake).
        self.ent[i].f63 = self.mc2_ord(4);
        let period = BEHAVIOR[75].v_26.max(1);
        self.ent[i].f58 = (period - (self.ent[i].f63 as i16 % period)) + 4; // :33902
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 0);
        self.mc2_shift_rot(i, 128, 256);
        Some(i)
    }

    /// `AddVilliger_4BF40` (:34037) — the Villager (5,13). TWO ctor
    /// RNG draws (facing, then the % 9 sprite pick) — the order is
    /// stream-visible.
    pub(crate) fn mc2_spawn_villager(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = 13;
            e.tick70 = VILLAGER_BASE + 1; // actionIndex 105
            // MC2 carries NO per-channel vulnerability mask — its
            // single damage gate is byte[0] & 8 (mapped to flags & 8,
            // the shared NewEvent default). MC1's writers additionally
            // check the +28 channel mask; admit their physical channel
            // at the seam (cross-column damage contract).
            e.f28 = 1;
            e.f128 = 54;
            e.f130 = 18;
            e.f126 = 18;
        }
        let d = self.mc2_rand(i); // draw #1 (:34048)
        {
            let e = &mut self.ent[i];
            let f = ((d & 0x7FF) as i32 - 1) as u16;
            e.f34 = f;
            e.f30 = f;
            e.f32 = f;
            e.max_life = 1000;
            e.f140 = 0; // mana 0: drops nothing
            e.f44 = 500;
            e.row156 = 100; // ABSOLUTE row index (:34058)
            e.f58 = 64;
            e.f26 = 2;
        }
        // Per-model spawn ordinal → f63 (:34062) — herd cadence.
        self.ent[i].f63 = self.mc2_ord(13);
        self.link(i, x, y, z);
        self.refill_life(i);
        let d2 = self.mc2_rand(i); // draw #2 (:34065)
        let sprite = match d2 % 9 {
            0..=2 => 242,
            3..=5 => 271,
            6 | 7 => 241,
            _ => 239,
        };
        self.mc2_set_sprite(i, sprite);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// `AddEvent09_0D_4DAB0` (:35031) — the (9,13) archer arrow:
    /// speed 384, life 5120/384 = 13, sprite 195 with the doubled
    /// quad.
    pub(crate) fn mc2_spawn_arrow(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            e.model65 = 13;
            e.tick70 = ARROW_STATE;
            e.f126 = 384; // actSpeed
            e.f128 = 384; // minSpeed
            e.max_life = (5120 / 384) as u32; // 13
            e.flags &= !8; // byte[0] &= 0xF7 (:35038) — arrows are not targets
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite_x2(i, 195);
        Some(i)
    }

    // ---- the Goat block (8..=15, :11386-11462) --------------------------

    fn goat_tick(&mut self, i: usize, ctx: &MobCtx) {
        let role = self.ent[i].tick70 - GOAT_BASE;
        match role {
            0 => {
                self.mc2_patrol(i, GOAT_BASE);
                self.goat_snd(i, 0x4D);
                self.goat_speed_fixup(i);
            }
            1 => {
                self.mc2_idle(i, GOAT_BASE, ctx);
                self.goat_snd(i, 0x4D);
                self.goat_speed_fixup(i);
            }
            2 => {
                // sub_1F440 (:11410): the chase slot redirects into
                // FLEE.
                self.ent[i].tick70 = GOAT_BASE + 6;
                self.ent[i].f126 = self.ent[i].f128;
                self.goat_hit(i, ctx);
            }
            3 => {
                self.mc2_pack(i, GOAT_BASE);
                self.goat_snd(i, 0x4D);
                self.goat_speed_fixup(i);
            }
            4 => self.mc2_prekill(i, GOAT_BASE),
            5 => self.mc2_kill(i),
            6 => self.goat_hit(i, ctx),
            _ => {
                // AddGoat05_01 (:11452): sub_1D5D0 is a no-op for
                // StageVar2 == 0 — sound roll + speed by action.
                self.goat_snd(i, 0x4D);
                if self.ent[i].tick70 == GOAT_BASE + 6 {
                    self.ent[i].f126 = self.ent[i].f128;
                } else {
                    self.ent[i].f126 = self.ent[i].f130;
                }
            }
        }
    }

    /// `HitGoat_1F530` (:11441): flee + exit speed + the 0x2B roll.
    fn goat_hit(&mut self, i: usize, ctx: &MobCtx) {
        self.mc2_flee(i, GOAT_BASE, ctx);
        if self.ent[i].tick70 != GOAT_BASE + 6 {
            self.ent[i].f126 = self.ent[i].f130;
        }
        self.goat_snd(i, 0x2B);
    }

    /// The post-primitive `action == 14 → actSpeed = minSpeed` fixup
    /// shared by states 8/9/11/15 (:11393 etc.).
    fn goat_speed_fixup(&mut self, i: usize) {
        if self.ent[i].tick70 == GOAT_BASE + 6 {
            self.ent[i].f126 = self.ent[i].f128;
        }
    }

    /// The screech roll: one LCG, sound 46 on `% modulus == 0`.
    pub(crate) fn goat_snd(&mut self, i: usize, modulus: u32) {
        if self.mc2_rand(i) % modulus == 0 {
            self.snd(46, i);
        }
    }

    // ---- the Archer block (32..=39, :11624-11970) --------------------------

    fn archer_tick(&mut self, i: usize, ctx: &MobCtx) {
        let role = self.ent[i].tick70 - ARCHER_BASE;
        match role {
            0 => {
                self.mc2_patrol(i, ARCHER_BASE);
                if self.ent[i].tick70 == ARCHER_BASE + 2 {
                    self.archer_aim(i);
                }
            }
            1 => self.archer_brain(i, ctx),
            2 => {
                // AddArcher0504_1FF40 (:11884).
                let _ = self.mc2_chase_attack(i, ARCHER_BASE, ctx, Self::archer_fire);
                if self.ent[i].tick70 != ARCHER_BASE + 2 {
                    self.archer_unaim(i);
                    return;
                }
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                if self.ent[i].f63 % period == 0 {
                    // Re-arm the target wizard's wanted timer per
                    // volley (:11900).
                    let t = self.ent[i].f146;
                    if t == PLAYER_TARGET {
                        self.mc2_arm_wanted(PLAYER_TARGET);
                    } else if (t as usize) < self.ent.len()
                        && self.ent[t as usize].class64 == 3
                        && self.ent[t as usize].model65 <= 1
                    {
                        self.mc2_arm_wanted(t);
                    }
                }
            }
            3 => {
                // sub_1FFE0 (:11907).
                self.mc2_pack(i, ARCHER_BASE);
                if self.ent[i].tick70 == ARCHER_BASE + 2 {
                    self.archer_aim(i);
                }
            }
            4 => {
                // HitArcher_20010 (:11918): the shrine-consumed
                // archer (f26 set) vanishes without a corpse.
                if self.ent[i].f26 != 0 {
                    self.ent[i].flags |= 0x400;
                } else {
                    self.mc2_prekill(i, ARCHER_BASE);
                }
            }
            5 => self.mc2_kill(i),
            6 => {
                // sub_20130: MISSING from the decompile (module
                // doc); unreachable for archers (flags bit 8
                // clear) — hold.
            }
            _ => {
                // AddScroll05_04_20140 (:11960): clear the shrine
                // flag; sub_1D5D0 no-op for StageVar2 == 0.
                self.ent[i].f26 = 0;
                if self.ent[i].tick70 == ARCHER_BASE + 2 {
                    self.archer_aim(i);
                }
            }
        }
    }

    /// `sub_1FAA0` (:11636) — the Archer idle/acquire brain.
    fn archer_brain(&mut self, i: usize, ctx: &MobCtx) {
        self.ent[i].f26 = 0; // dword_0x10_16 = 0 every tick
        match self.mc2_state_head(i) {
            1 => {
                self.ent[i].f146 = self.ent[i].f40;
                self.ent[i].tick70 = ARCHER_BASE + 2; // 34 — hardwired
                self.mc2_alt_commit(i);
                self.archer_aim(i);
            }
            2 => self.ent[i].tick70 = ARCHER_BASE + 4,
            _ => {
                self.mc2_move_core(i);
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
                if self.ent[i].f63 as i16 % period != 0 {
                    return;
                }
                if self.ent[i].f146 != 0 {
                    // Shrine handling (:11700-24): only a (10,45)
                    // stays a destination; walk to it and be
                    // consumed at 0x1000.
                    let t = self.ent[i].f146 as usize;
                    let shrine = t < self.ent.len()
                        && self.ent[t].class64 == 10
                        && self.ent[t].model65 == 45
                        && self.ent[t].flags & 0x400 == 0;
                    if !shrine {
                        self.ent[i].f146 = 0;
                    } else {
                        let (sp, tp) = {
                            let e = &self.ent[i];
                            let s = &self.ent[t];
                            ((e.x, e.y, e.z), (s.x, s.y, s.z))
                        };
                        if Self::mc2_dist3(sp, tp) > 0x1000 {
                            self.ent[i].f34 = Self::angle_between(sp.0, sp.1, tp.0, tp.1);
                        } else {
                            self.ent[i].f26 = 1;
                            self.ent[i].tick70 = ARCHER_BASE + 4;
                            self.ent[t].f26 += 1;
                        }
                    }
                    return;
                }
                self.mc2_wander_turn(i);
                let period4 = 4 * period;
                if self.ent[i].f63 as i16 % period4 == 0 {
                    // Scan A (:11768-11804): nearest class-3 ANYTHING,
                    // then POST-REJECT the single winner unless it is
                    // a wizard (model ≤ 1) with a live wanted timer —
                    // a nearer castle/balloon/non-wanted wizard voids
                    // the whole scan (falls to Scan B).
                    let mut target = self.mc2_class3_scan(i, ctx).filter(|&s| {
                        let wizard = s == PLAYER_TARGET || self.ent[s as usize].model65 <= 1;
                        wizard && self.mc2_wanted_live(s)
                    });
                    if target.is_none() {
                        // Scan B: nearest model-9 creature, no cone
                        // (:11811). RETAIL-OBSERVED EXTENSION
                        // (player-replayed mc2:04, 2026-07-24): the
                        // retail archers shoot until every skeleton
                        // is dead, THEN start shooting the worms —
                        // the monster-hunter design ("archers target
                        // unnatural creatures"). The decompile's
                        // Scan B walks only chain[9] and the retail
                        // mechanism is unrecovered, so the port falls
                        // back to the next UNNATURAL model when the
                        // watched one is EXTINCT — never before, so
                        // the battle order is preserved. Set = worms
                        // (m3); extend if other levels surface more.
                        let e = &self.ent[i];
                        let row = &BEHAVIOR[e.row156 as usize];
                        let range = (row.v_28 as i32) * (row.v_28 as i32);
                        let (ex, ey) = (e.x, e.y);
                        for model in [9u8, 3] {
                            let mut best: Option<(u16, i32)> = None;
                            let mut extinct = true;
                            for (j, c) in self.ent.iter().enumerate().skip(1) {
                                if c.class64 == 5
                                    && c.model65 == model
                                    && c.act_life >= 0
                                    && !matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
                                    && c.flags & 0x400 == 0
                                {
                                    extinct = false;
                                    let d2 = Self::dist2_sq(ex, ey, c.x, c.y);
                                    if d2 <= range && best.is_none_or(|(_, bd)| d2 < bd) {
                                        best = Some((j as u16, d2));
                                    }
                                }
                            }
                            target = best.map(|(s, _)| s);
                            if !extinct {
                                break;
                            }
                        }
                    }
                    if let Some(t) = target {
                        // Shrines never become targets (:11824).
                        let is_shrine = (t as usize) < self.ent.len()
                            && self.ent[t as usize].class64 == 10
                            && self.ent[t as usize].model65 == 45;
                        if !is_shrine {
                            self.ent[i].f146 = t;
                            self.ent[i].tick70 = ARCHER_BASE + 2;
                            self.archer_aim(i);
                            return;
                        }
                    }
                    // Scan C: pack (:11840-69).
                    if let Some(l) = self.mc2_pack_scan(i, false) {
                        self.ent[i].f52 = l;
                        self.ent[i].tick70 = ARCHER_BASE + 3;
                    }
                }
            }
        }
    }

    /// `sub_20060` (:11936): one LCG, stop, firing sprite 206 or 1
    /// by `% 0x14 <= 10`, shift-rot, record target class/model into
    /// the filter bytes.
    fn archer_aim(&mut self, i: usize) {
        let d = self.mc2_rand(i);
        self.ent[i].f126 = 0;
        let sprite = if d % 0x14 <= 10 { 206 } else { 1 };
        self.mc2_set_sprite(i, sprite);
        self.mc2_shift_rot(i, 128, 256);
        let t = self.ent[i].f146;
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

    /// `sub_200F0` (:11950): back to the patrol sprite/speed.
    fn archer_unaim(&mut self, i: usize) {
        self.ent[i].f126 = self.ent[i].f128;
        self.mc2_set_sprite(i, 0);
        self.mc2_shift_rot(i, 128, 256);
        self.ent[i].f66 = 3;
        self.ent[i].f67 = 0xFF;
    }

    /// `sub_1CCE0` (:9713) — the arrow-fire thunk: spawn the (9,13)
    /// arrow aimed at the target (yaw + pitch), lift by fov/2, arm
    /// f44 = 250, and poke the target wizard's danger timer
    /// (sub_5EF70 → 100).
    fn archer_fire(&mut self, i: usize, target: u16, ctx: &MobCtx) -> bool {
        let (x, y, z, own, fov) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f84)
        };
        let Some((tx, ty, tz)) = self.mc2_target(target, ctx) else {
            return false;
        };
        let Some(a) = self.mc2_spawn_arrow(x, y, z) else {
            return false;
        };
        self.ent[a].id24 = own;
        let yaw = Self::angle_between(x, y, tx, ty);
        self.ent[a].f30 = yaw;
        let dh = Self::isqrt(Self::dist2_sq(x, y, tx, ty) as u32) as i32;
        self.ent[a].f32 = Self::pitch_toward(z, tz, dh);
        let (ax, ay) = (self.ent[a].x, self.ent[a].y);
        let az = self.ent[a].z.wrapping_add((fov / 2) as i16);
        self.move_relink(a, ax, ay, az);
        self.ent[a].f146 = self.ent[i].f146;
        self.ent[a].f44 = 250;
        let (tc, tm) = if target == PLAYER_TARGET {
            (3, 0)
        } else {
            (
                self.ent[target as usize].class64,
                self.ent[target as usize].model65,
            )
        };
        self.ent[a].f66 = tc;
        self.ent[a].f67 = tm;
        if target == PLAYER_TARGET {
            self.player_danger = 100; // sub_5EF70 (:60598)
        }
        // No shots++: a creature volley never bumps the player's
        // accuracy stat in retail.
        true
    }

    /// `AddArcherArrow_672E0` (:58852) — the (9,13) flight tick:
    /// first-tick whoosh (global stage LCG picks sound 33/34), polar
    /// step, victim probe, terrain/expiry impact. Returns true when
    /// terrain changed (never — arrows don't dig).
    pub(crate) fn mc2_arrow_tick(&mut self, i: usize, ctx: &MobCtx) {
        if self.ent[i].flags & F_WHOOSH == 0 {
            self.rand = self.rand.wrapping_mul(9377).wrapping_add(9439);
            let snd = ((self.rand & 1) + 33) as u8;
            self.snd(snd, i);
            self.ent[i].flags |= F_WHOOSH;
        }
        let e = &self.ent[i];
        let mut pos = (e.x, e.y, e.z);
        Self::polar_step(&mut pos, e.f30, e.f32, e.f126);
        // Victim probe (sub_10780 → the shared tile-chain scan;
        // module-doc APPROX). Owner-immunity via id24 like MC1, PLUS
        // the projectile's own xtype/xsubtype filter (:3766-69) via
        // the shared `mc2_proj_filter`. The fire seams stamp the
        // TARGET's class+model onto the arrow (creature thunks
        // sub_1CCE0/sub_1CDA0; the archer combat state's own bytes
        // come from sub_20060 — sub_200F0's 3/-1 is the IDLE reset,
        // not the combat filter) — so a skeleton volley strikes the
        // FIRST archer along its path, not just the locked target:
        // stray arrows through a packed flock are what spread the
        // mc2:04 war. (APPROX: the original keeps scanning the ring
        // past a non-matching body in the same tick; we let the
        // arrow fly on and re-probe next tick.)
        let scanned = self.victim_scan_at(i, pos, ctx);
        let hit = self.mc2_proj_filter(i, scanned);
        let above_ground = self.ground_z(pos.0, pos.1) as i16 <= pos.2;
        if above_ground {
            let life = self.ent[i].act_life;
            self.ent[i].act_life = life - 1;
            if life != 0 && hit.is_none() {
                self.move_relink(i, pos.0, pos.1, pos.2);
                return;
            }
        }
        // The Rebound gate (`sub_68740` at EF:58892): a shielded
        // victim throws the arrow back (model 13 passes the engine's
        // whitelist unconditionally).
        if let Some(h) = hit
            && self.mc2_rebound_deflect(i, h, ctx)
        {
            return;
        }
        // Impact (LABEL_10 / the entity branch): move to the victim,
        // area-write ch0 with f44, despawn.
        match hit {
            Some(crate::mc1::combat::MailTarget::Pool(v)) => {
                let (vx, vy, vz) = (self.ent[v].x, self.ent[v].y, self.ent[v].z);
                self.move_relink(i, vx, vy, vz);
            }
            Some(crate::mc1::combat::MailTarget::Player) => {
                let (px, py, pz) = (ctx.px, ctx.py, ctx.pz);
                self.move_relink(i, px, py, pz);
            }
            None => self.move_relink(i, pos.0, pos.1, pos.2),
        }
        let amt = self.ent[i].f44 as u32;
        self.area_write(i, 0, amt, ctx, false, false);
        self.ent[i].flags |= 0x400;
    }

    // ---- the Villager block (104..=111, :14498-14718) ----------------------

    fn villager_tick(&mut self, i: usize, ctx: &MobCtx) {
        let role = self.ent[i].tick70 - VILLAGER_BASE;
        match role {
            0 | 2 | 3 => {
                // sub_23320/23640/23660: re-enter the brain.
                self.ent[i].tick70 = VILLAGER_BASE + 1;
                self.villager_brain(i, ctx);
            }
            1 => self.villager_brain(i, ctx),
            4 => {
                // KillTownie_23680 (:14668).
                if self.ent[i].f26 != 0 {
                    self.ent[i].flags |= 0x400;
                    return;
                }
                let killer = self.ent[i].f38;
                if killer == PLAYER_TARGET {
                    self.mc2_arm_wanted(PLAYER_TARGET);
                }
                self.mc2_prekill(i, VILLAGER_BASE);
            }
            5 => self.mc2_kill(i),
            6 => {
                // HitTownie_23710 (:14691).
                self.mc2_flee(i, VILLAGER_BASE, ctx);
                if self.ent[i].tick70 != VILLAGER_BASE + 6 {
                    self.ent[i].f146 = 0;
                    self.ent[i].f126 = self.ent[i].f130;
                }
            }
            _ => {
                // AddTownie05_0D_23750 (:14707): 1D5D0 no-op; speed
                // by action.
                if self.ent[i].tick70 == VILLAGER_BASE + 6 {
                    self.ent[i].f126 = self.ent[i].f128;
                } else {
                    self.ent[i].f126 = self.ent[i].f130;
                }
            }
        }
    }

    /// `sub_23340` (:14506) — the townie wander brain.
    fn villager_brain(&mut self, i: usize, ctx: &MobCtx) {
        match self.mc2_state_head(i) {
            1 => {
                // A wizard hit arms its wanted timer (:14561-63).
                let src = self.ent[i].f40;
                if src == PLAYER_TARGET {
                    self.mc2_arm_wanted(PLAYER_TARGET);
                }
                self.ent[i].f146 = src;
                self.ent[i].tick70 = VILLAGER_BASE + 6; // 110
            }
            2 => self.ent[i].tick70 = VILLAGER_BASE + 4, // 108
            _ => {
                self.mc2_move_core(i);
                let period = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1) as u8;
                if self.ent[i].f63 % period == 0 {
                    if self.ent[i].f146 != 0 {
                        // Rally to a (10,45) building flag within
                        // 0x800; consumed if it has capacity
                        // (:14584-99: shrine.minSpeed > shrine
                        // counter).
                        let t = self.ent[i].f146 as usize;
                        let shrine = t < self.ent.len()
                            && self.ent[t].class64 == 10
                            && self.ent[t].model65 == 45
                            && self.ent[t].flags & 0x400 == 0;
                        if shrine {
                            let (sp, tp) = {
                                let e = &self.ent[i];
                                let s = &self.ent[t];
                                ((e.x, e.y, e.z), (s.x, s.y, s.z))
                            };
                            if Self::mc2_dist3(sp, tp) > 0x800 {
                                self.ent[i].f34 = Self::angle_between(sp.0, sp.1, tp.0, tp.1);
                            } else if (self.ent[t].f128 as i32) > self.ent[t].f26 as i32 {
                                self.ent[i].f26 = 1;
                                self.ent[i].tick70 = VILLAGER_BASE + 4;
                                self.ent[t].f26 += 1;
                            } else {
                                self.ent[i].f146 = 0;
                                self.ent[i].f126 = self.ent[i].f130;
                            }
                        } else {
                            self.ent[i].f146 = 0;
                            self.ent[i].f126 = self.ent[i].f130;
                        }
                    } else {
                        self.mc2_wander_turn(i);
                        // Nearest ENTERABLE building — a (10,45)
                        // whose bldgprm row has byte_2 & 1 (:14619),
                        // no range limit: townies are NEVER in free
                        // wander, they permanently march at the
                        // nearest dwelling.
                        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                        let mut best: Option<(u16, i32)> = None;
                        for (j, c) in self.ent.iter().enumerate().skip(1) {
                            if c.class64 == 10
                                && c.model65 == 45
                                && c.flags & 0x400 == 0
                                && self.assets.build_tab.get(c.f71 as usize).is_some()
                                // bldgprm byte_2 & 1 ENTERABLE gate
                                // (:14619): dwellings attract townies;
                                // stone/route templates (the dis-13
                                // causeway obelisks, flags 0x08/0x18)
                                // must not capture them.
                                && self
                                    .assets
                                    .bldgprm
                                    .get(c.f71 as usize)
                                    .is_some_and(|p| p.flags & 1 != 0)
                            {
                                let d2 = Self::dist2_sq(ex, ey, c.x, c.y);
                                if best.is_none_or(|(_, bd)| d2 < bd) {
                                    best = Some((j as u16, d2));
                                }
                            }
                        }
                        if let Some((b, _)) = best {
                            self.ent[i].f146 = b;
                            self.ent[i].f126 = self.ent[i].f130 + 12;
                        }
                    }
                }
                let _ = ctx;
            }
        }
        // LABEL_43 tail: flee state walks at minSpeed.
        if self.ent[i].tick70 == VILLAGER_BASE + 6 {
            self.ent[i].f126 = self.ent[i].f128;
        }
    }

    // ---- class 2: scenery (tree / stone / dolmen) ---------------------------

    /// `AddTree_4AC40` (:33433) — the MC2 tree (2,0). FOUR per-entity
    /// LCG draws (lifespan, x/y jitter, sprite pick), byte-faithful.
    /// The class-2 tick column (the tree burn ladder + static decay)
    /// lives in `scenery.rs`.
    pub(crate) fn mc2_spawn_tree(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = 0;
            e.tick70 = 0;
            e.f26 = (i % 11) as i16; // dword_0x10_16: phase stagger
            e.f56 = 1; // byte_0x38_56: burnable (ch0 intake)
            // Cross-column damage contract: MC2's burnable gate IS
            // `(1 << ch) & byte_0x38_56` — admit ch0 through MC1's
            // +28 mask so the shared area writer reaches the tree.
            e.f28 = 1;
        }
        // The 2500..7500 life roll is DEAD VALUE in retail: AddTree
        // (:33443-50) rolls it, then `CopyMaxLifeToLife_49A20` right
        // after the map link resets life = maxLife (the pool-default
        // 300 — mc2l0 t=3169's disposition wave records every tree at
        // 300/300). The draw itself must still burn (the record's
        // rand stream feeds the jitters + sprite pick).
        let d = self.mc2_rand(i);
        self.ent[i].act_life = (d % 0x1388 + 2500) as i32;
        let jx = ((self.mc2_rand(i) & 0x3F) as i32 - 32) as i16;
        let jy = ((self.mc2_rand(i) & 0x3F) as i32 - 32) as i16;
        let (nx, ny) = (x.wrapping_add(jx as u16), y.wrapping_add(jy as u16));
        self.link(i, nx, ny, z);
        self.refill_life(i);
        let d = self.mc2_rand(i);
        self.mc2_set_sprite(i, if d & 1 != 0 { 84 } else { 83 });
        Some(i)
    }

    /// `AddStone_4AD70` (:33466) — the standing stone (2,1):
    /// non-collidable (byte[0] &= 0xF7), state 3, sprite row 79.
    pub(crate) fn mc2_spawn_stone(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = 1;
            e.tick70 = 3;
            e.f26 = (i % 11) as i16;
            e.flags &= !8;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 79);
        Some(i)
    }

    /// `AddDolmen_4ADF0` (:33484) — the dolmen (2,2), "similar as
    /// Obelisk": non-collidable, state 6, sprite row 39, quad
    /// ShiftRot(1024, 1024).
    pub(crate) fn mc2_spawn_dolmen(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = 2;
            e.tick70 = 6;
            e.f26 = (i % 11) as i16;
            e.flags &= !8;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 39);
        self.mc2_shift_rot(i, 1024, 1024);
        Some(i)
    }

    // ---- class 10 models 0/1: ground fire + the big explosion --------------

    /// `NewAdd0A00_4E320` (:35332) — the MC2 ground fire/eruption
    /// element (every explosion chain resolves into these): life 8,
    /// area-damage amount 400 (`subSpellIndex`), sprite row 7, quad
    /// (128, 128). Flag ops: `dword &= 0xFFFDFFF7` (clears collidable
    /// and byte[2] bit 1) then `byte[2] |= 2` — byte[2] doubles as
    /// the paint `inType` seed, its bit 0 as the no-damage gate.
    pub(crate) fn mc2_spawn_fire(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 0;
            e.tick70 = 0;
            e.max_life = 8;
            e.f140 = 400; // subSpellIndex = sub_10C80's ch0 amount
            e.f56 = 0;
            e.flags = (e.flags & !0x2_0008) | 0x2_0000;
        }
        self.link(i, x, y, z);
        self.ent[i].act_life = 8;
        self.mc2_set_sprite(i, 7);
        self.mc2_shift_rot(i, 128, 128);
        Some(i)
    }

    /// `NewAdd0A01_4E3B0` (:35354) — the "Big explosion" (10,1), the
    /// route marker: a 1-life seeder whose whole job is the (10,0)
    /// cluster. Sprite row 41. (The dynamic light AddEvent2_847D0 is
    /// presentation, unported.)
    pub(crate) fn mc2_spawn_big_explosion(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 1;
            e.tick70 = 1;
            e.max_life = 1;
            e.f140 = 400;
            e.f26 = 0; // dword_0x10_16 = the seeding ring span
            e.flags = (e.flags & !0x2_0008) | 0x2_0000;
        }
        self.link(i, x, y, z);
        self.ent[i].act_life = 1;
        self.mc2_set_sprite(i, 41);
        Some(i)
    }

    /// `NewAdd0A02_4E430` (EF:35375) — the (10,2) AMBIENT PUFF, the
    /// Speed spell's slipstream marker (`GetScroll_69DB0` EF:56253 is
    /// its only caller). Four writes and no more: maxLife/life 8,
    /// action 2, `dword_0x10_16` = 0, and the flag word masked to
    /// `byte[0] |= 1` / `byte[0] &= ~8` (untargetable) /
    /// `byte[2] |= 2` (sacrificable) — recorded flags 0x20001.
    ///
    /// It is deliberately UNLINKED (the ctor assigns `position_0x4C_76`
    /// instead of calling `AddEventToMap_57D70`) and SPRITELESS — the
    /// MC1 twin of the same puff behaves identically
    /// (docs/traces/mc1-class12-spell-tokens.md).
    pub(crate) fn mc2_spawn_speed_puff(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        let e = &mut self.ent[i];
        e.class64 = 10;
        e.model65 = 2;
        e.tick70 = 2;
        e.max_life = 8;
        e.act_life = 8;
        e.f26 = 0;
        e.flags = (e.flags & !0x2_0009) | 0x2_0001;
        e.x = x;
        e.y = y;
        e.z = z;
        Some(i)
    }

    /// `sub_30D50` (:22692) — the (10,0) fire tick: optional fuse
    /// (`dword_0x10_16 & 3`), then per active tick: one-shot
    /// activation (area damage 400 via sub_10C80 ≡ our `area_write`
    /// under the cross-column mask contract, gated on byte[2] bit 0;
    /// terrain burn — worn-path repaints 26/10/11 through the
    /// texture-band painter, else the scorch dig; flicker draw; sound
    /// 3), the z rule (drift by flicker above ground, clamp up, cave
    /// ceiling clamp), anim advance.
    pub(crate) fn mc2_fire_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].f26 & 3 != 0 {
            self.ent[i].f26 -= 1;
            return false;
        }
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < -1 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        self.ent[i].flags &= !1;
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let ground = self.ground_z(x, y) as i16;
        let mut dirty = false;
        if self.ent[i].flags & 2 == 0 {
            let in_type = ((self.ent[i].flags >> 16) & 0xFF) as u8;
            if self.ent[i].flags & 0x1_0000 == 0 {
                let amt = self.ent[i].f140 as u32;
                self.area_write(i, 0, amt, ctx, false, false);
            }
            let (cx, cy) = (
                ((x.wrapping_add(128)) >> 8) as u8,
                ((y.wrapping_add(128)) >> 8) as u8,
            );
            let t = crate::engine::features::tile(cx, cy);
            let ty = self.t.tile_type[t];
            if ty != 0 {
                match ty {
                    26 => {
                        self.mc2_paint_cell(in_type, cx, cy, 0x14);
                        dirty = true;
                    }
                    10 => {
                        self.mc2_paint_cell(in_type, cx, cy, 0x15);
                        dirty = true;
                    }
                    11 => {
                        self.mc2_paint_cell(in_type, cx, cy, 0x16);
                        dirty = true;
                    }
                    _ => {
                        // sub_104A0 (:2052) reads the UNROUNDED cell.
                        let raw = crate::engine::features::tile((x >> 8) as u8, (y >> 8) as u8);
                        if !(6..=0x22).contains(&ty)
                            && self.t.angle[t] & 7 != 1
                            && (z as i32 - ground as i32) <= 128
                            && (1u32 << (self.t.angle[raw] & 0xF)) & 1 == 0
                        {
                            let d = self.ent_rand(i);
                            self.dig_scorch(i, -((d % 7) as i16));
                            dirty = true;
                        }
                    }
                }
            }
            self.ent[i].flags |= 2;
            let d = self.ent_rand(i);
            self.ent[i].f44 = ((d % 0x41) as i32 - 32) as u16;
            self.snd(3, i);
        }
        // sub_580E0(pos, ground, 0, 0, flicker).
        let mut nz = self.ent[i].z;
        Self::mc2_alt_core(&mut nz, ground, 0, self.ent[i].f44 as i16);
        self.ent[i].z = nz;
        // Cave ceiling clamp (EF:22752-58).
        if self.is_cave() {
            let c = (self.ceiling_z(x, y) - self.ent[i].f84 as i32) as i16;
            if self.ent[i].z > c {
                self.ent[i].z = c;
            }
        }
        // sub_585A0: frame advance (the renderer's 22..=36 band caps
        // by the sprite's span; retail caps by x_BYTE_D8A2E).
        self.ent[i].frame88 = self.ent[i].frame88.saturating_add(1);
        dirty
    }

    /// `AddQuickfair0A_01_30F60` (:22768) — the (10,1) tick: two
    /// acting ticks (post-decrement `life-- < 0`), sound 3 once, and
    /// per tick a sweep of SEARCH rings 0..=`dword_0x10_16` seeding
    /// (10,0) children at `pos - 96 + 192*cell ± rand%129-64` with a
    /// ~50% per-cell draw; children inherit id + yaw and raise
    /// byte[0] bit 7.
    pub(crate) fn mc2_big_explosion_tick(&mut self, i: usize) {
        let life = self.ent[i].act_life;
        self.ent[i].act_life -= 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.snd(3, i);
        }
        let ring = self.ent[i].f26 as i32;
        let cells = self.ring_cells(ring, ring);
        let (px, py, pz, id, yaw) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30)
        };
        for (dx, dy) in cells {
            let d = self.ent_rand(i);
            if 2 * ((d % 0x9D) as i32 / 79) - 1 > 0 {
                let d = self.ent_rand(i);
                let nx = (px as i32 - 96 + 192 * dx as i32 + (d % 0x81) as i32 - 64) as u16;
                let d = self.ent_rand(i);
                let ny = (py as i32 - 96 + 192 * dy as i32 + (d % 0x81) as i32 - 64) as u16;
                if let Some(c) = self.mc2_spawn_fire(nx, ny, pz) {
                    self.ent[c].id24 = id;
                    self.ent[c].f30 = yaw;
                    self.ent[c].flags |= 0x80;
                }
            }
        }
    }

    // ---- class 10 model 45: buildings --------------------------------------

    /// `AddTerrainModification_50250` (:36677) + the `sub_49A30`
    /// building setup (:32753) that both spawn paths run right after
    /// the creator (PrepareEvents Events.cpp:348 / disposition
    /// :33089). `bldg` = the THING's par1 = the BUILD00/BLDGPRM
    /// building id. Draws NO entity RNG (SetEntityIndexAndRot is
    /// RNG-free).
    ///
    /// APPROX register (like the module doc): the VGA half-resolution
    /// footprint shrink (:32771) is the low-res render mode, skipped;
    /// `dword_0x10_16 = 2` has no ported consumer; the id-68 player
    /// castle global (:32812) lands with MC2 castles.
    pub(crate) fn mc2_spawn_building(
        &mut self,
        x: u16,
        y: u16,
        z: i16,
        bldg: u16,
    ) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 45;
            e.max_life = 30;
            e.tick70 = 51; // actionIndex 0x33
            // byte_0x38_56 = 33 (:36688): ch0 damage intake + bit 5 —
            // buildings are DESTRUCTIBLE by area writers; the
            // productive kind adds bit 1 (claim channel) below.
            // f28 mirrors the intake bits for the SHARED writer gate
            // (the cross-column damage contract — area_write tests
            // f28, not f56; docs/traces/mc2-possession-delivery.md:
            // without it the possess pulse's ch1 claim mail and ch0
            // area damage are both dropped at the gate).
            e.f56 = 33;
            e.f28 = 1;
            // byte[0] = 9 (:36687): bit 3 targetable + bit 0 (the
            // unclaimed/no-flag marker; the claim clears it).
            e.flags |= 1;
            // dword_0x10_16: ctor 4 → sub_49A30 overwrites 2
            // (:32757) — the occupant count the house tick pops.
            e.f26 = 2;
        }
        self.mc2_set_sprite(i, 177);
        // sub_49A30: footprint metadata + snapped placement.
        let def = self.assets.build_tab.get(bldg as usize).copied();
        let (w, h) = def.map_or((0u8, 0u8), |d| (d.w, d.h));
        // Snap to the tile corner (:32777-79), then the parity
        // alignment: an odd top-left corner sum shifts one tile +x
        // (:32782-88).
        let mut sx = x & 0xFF00;
        let sy = y & 0xFF00;
        let mut tlx = ((sx >> 8) as u8).wrapping_sub(w / 2);
        let tly = ((sy >> 8) as u8).wrapping_sub(h / 2);
        if (tlx.wrapping_add(tly)) & 1 != 0 {
            sx = sx.wrapping_add(256);
            tlx = tlx.wrapping_add(1);
        }
        // z = 32 * the 4-corner average over the footprint (:32790,
        // GetTerrainHeightFromSquare_48DF0 ≡ our avg4 — chassis).
        let site = (32 * self.avg4(tlx, tly, h, w)) as i16;
        let _ = z;
        self.link(i, sx, sy, site);
        let prm = self
            .assets
            .bldgprm
            .get(bldg as usize)
            .copied()
            .unwrap_or_default();
        {
            let e = &mut self.ent[i];
            e.f128 = ((w as u16 * h as u16) >> 4) as i16; // minSpeed_132
            // SetShiftByCastle_49EC0 (:32882): the footprint quad.
            e.f78 = 0;
            e.f80 = ((w as u16) << 8).wrapping_add(1280) >> 1;
            e.f82 = ((h as u16) << 8).wrapping_add(1280) >> 1;
            e.f84 = 256;
            e.f71 = bldg as u8;
            e.act_life = 30;
            // ⭐ THE PRODUCTION RATE IS `subSpellIndex_0x2A_42` (f44),
            // NOT the mana word: `subSpellIndex = bldgprm[a2].word_0`
            // (EF:32793), and the construction finish parks the
            // building's LIFE at `1000 * subSpellIndex` (EF:27291).
            // f140 is retail's `mana_0x90_144`, which the same ctor
            // zeroes and then re-derives FROM the rate below. Parking
            // the rate in f140 gave the right life in fresh play by
            // coincidence and 1000x the MANA under import, where the
            // importer faithfully restores @0x2A → f44 and @0x90 →
            // f140 (mc2l1 t=888 slot 161: retail life 190,000,
            // port 0 — the imported building's mana was 0).
            e.f44 = prm.rate;
            // ⭐ THE DEGRADATION LINK IS PER-ENTITY, NOT A TABLE READ:
            // `fontTypeIndex_0x3D_61 = bldgprm[a2].byte_3` (:32795-98).
            // Two crush paths ZERO it on the live entity — the castle
            // level-up pre-clear `sub_11960` (EF:4410-11, called from
            // EF:61128) and the (10,67) quake grab `sub_3A090`
            // (EF:29335-36) — and `RemoveCastleStage_385C0` branches on
            // the ENTITY's copy (EF:28090), so a crushed building
            // demolishes for good instead of rebuilding as its
            // successor forever. Reading the static table here is what
            // let the 16 self-chaining ids resurrect under a levelling
            // castle. Second consumer: the type-2 objective latch
            // (EF:40771-79) — a castle-crushed building COMPLETES it.
            e.f46 = prm.chain as i16;
            // `mana_0x90_144 = 0` (EF:32796), then the productive kind
            // (`byte_2 & 8 == 0`) re-derives it off the rate at
            // EF:32808. Retail leaves `maxMana_0x8C_140` (f136)
            // untouched on a building — the uniform import restores it
            // as the dead 0 it is.
            e.f140 = 0;
            if prm.flags & 8 == 0 {
                e.f56 |= 2;
                e.f28 |= 2; // claim channel, writer-gate mirror
                e.f140 = (1000 * prm.rate as i32) >> 7;
            }
        }
        Some(i)
    }

    /// `sub_57390` (:39746): building placement clears its footprint
    /// tile — scenery entities removed, creatures killed EXCEPT the
    /// protected models {6, 8, 10, 16, 22, 23, 27} (+ 25 while in
    /// action 200, retail's `actionIndex != -56`).
    ///
    /// `owner` is the caller's `id_0x1A_26` and the skip test is
    /// `victim.id24 != owner` — an OWNER compare, not a slot compare:
    /// a wizard's own creatures walk through their own construction
    /// unharmed. It degenerates to "skip the builder itself" for an
    /// unowned building, whose `id24` defaults to its own slot, which
    /// is why the slot-compare this used to do was indistinguishable
    /// on the village path — but NOT on the castle path, where the
    /// castle carries its wizard's id.
    ///
    /// The victim's killer/attacker pair (`word_0x24_36` /
    /// `word_0x26_38`) is stamped with the owner, so the kill credits
    /// the builder.
    pub(crate) fn mc2_building_clear_tile(&mut self, t: usize, owner: u16) {
        let mut j = self.map_entity[t] as usize;
        while j != 0 {
            let next = self.ent[j].next20 as usize;
            if self.ent[j].id24 != owner {
                match self.ent[j].class64 {
                    2 => self.free_entity(j),
                    5 => {
                        let m = self.ent[j].model65;
                        let protected = matches!(m, 6 | 8 | 10 | 16 | 22 | 23 | 27)
                            || (m == 25 && self.ent[j].tick70 == 200);
                        if !protected {
                            self.ent[j].act_life = -1;
                            self.ent[j].f36 = owner;
                            self.ent[j].f38 = owner;
                        }
                    }
                    _ => {}
                }
            }
            j = next;
        }
    }

    /// `ApplyTerrainModification_37240` (:27181), the 30-tick build
    /// action (state 51): first countdown tick clears the footprint
    /// (sub_57390), every tick lerps the height plane toward the
    /// building data's pad heights, every 5th tick (and the last)
    /// paints the walkable village tiles, and the final tick parks
    /// the entity as the static building (state 52) with its
    /// production timer. Footprint cells = BUILD00 data, TWO bytes
    /// per cell: [0] = paint code (0xff = none), [1] = pad height
    /// (0xff = none). Returns true (terrain changed).
    ///
    /// APPROX register: the one-at-a-time build carousel
    /// (IsNextEvent0A_2A_37740/sub_377A0) is skipped — all authored
    /// buildings raise concurrently at load. The sub_462A0 retile,
    /// the sub_45DC0 texture-band paint and the sub_48A20 pad-edge
    /// rings are the real ports ([`crate::mc2::terrain_paint`]) at
    /// the retail cadence. On caves, unless the bldgprm row carries
    /// flag 4 (no-cave-raise), EVERY footprint cell (pad or not)
    /// lerps the ceiling toward `min(max(floor, base) + 80, 255)`
    /// and re-asserts the invariant per tick (:27349-27373) — the
    /// headroom bubble that makes rock-embedded buildings enterable.
    /// The instant-placement sibling (`sub_36FC0`, same arm at
    /// :27114-27137) has no ported caller yet (`sub_5C950` stage
    /// machinery — unported).
    /// `human` = (previous-tick settled pose, carpet slot) for the
    /// `sub_377A0` completion pass — None on the load-time carousel
    /// (an APPROX like the concurrent raise: retail would mint for a
    /// wizard overlapping at load; the recording's seed state
    /// already carries those).
    pub(crate) fn mc2_building_tick(
        &mut self,
        i: usize,
        human: Option<((u16, u16, i16), u16)>,
    ) -> bool {
        let bldg = self.ent[i].f71 as usize;
        let Some(def) = self.assets.build_tab.get(bldg).copied() else {
            self.ent[i].tick70 = 52;
            return false;
        };
        let (w, h) = (def.w as usize, def.h as usize);
        // Copy the footprint cells (2 bytes each) out of the bank —
        // the loops below write the terrain planes.
        let start = def.offset as usize;
        let Some(cells) = self
            .assets
            .build_dat
            .get(start..start + 2 * w * h)
            .map(<[u8]>::to_vec)
        else {
            self.ent[i].tick70 = 52;
            return false;
        };
        let cx = ((self.ent[i].x.wrapping_add(128)) >> 8) as u8;
        let cy = ((self.ent[i].y.wrapping_add(128)) >> 8) as u8;
        let tlx = cx.wrapping_sub((w / 2) as u8);
        let tly = cy.wrapping_sub((h / 2) as u8);
        let base = self.ent[i].z >> 5; // v35
        // v50 (:27251): raise the cave ceiling over the footprint
        // unless the bldgprm row says no-cave-raise (flags & 4).
        let cave_raise = self.is_cave()
            && self
                .assets
                .bldgprm
                .get(bldg)
                .is_none_or(|b| b.flags & 4 == 0);
        self.ent[i].act_life -= 1;
        let life = self.ent[i].act_life;

        if life <= 0 {
            // Final frame (:27256-79): the per-cell sub_462A0 sweep
            // over every footprint cell with a paint code, then park
            // as the static building with the pad-edge rings
            // (:27289-304, thickness 2 then 5).
            for dy in 0..h {
                for dx in 0..w {
                    if cells[2 * (dy * w + dx)] == 0xff {
                        continue;
                    }
                    let (cx2, cy2) = (tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                    self.mc2_retile_region(cx2, cy2, cx2, cy2);
                }
            }
            let e = &mut self.ent[i];
            e.tick70 = 52;
            // `life_0x8 = 1000 * subSpellIndex_0x2A_42` (EF:27291) —
            // the production rate, f44 (see `mc2_spawn_building`).
            e.act_life = 1000 * e.f44 as i32;
            // The flag protocol (:27292-97): owned → bit 0 cleared
            // (the flag flies), unowned → set (no flag).
            if e.f144 != 0 {
                e.flags &= !1;
            } else {
                e.flags |= 1;
            }
            e.site_z = e.z;
            let (x, y) = (e.x, e.y);
            self.ent[i].z = self.ground_z(x, y) as i16;
            self.mc2_pad_edge_ring(tlx, tly, (h / 2) as u8, (w / 2) as u8, 2);
            self.mc2_pad_edge_ring(tlx, tly, (h / 2) as u8, (w / 2) as u8, 5);
            // `sub_377A0` (:27304, the action-51 completion tail):
            // every class-3 wizard whose box overlaps the finished
            // building gets a (10,42) painter minted ON THE WIZARD
            // (see `mc2_spawn_wizard_painter`). The overlap is the
            // 2-D `CompareAxisWithShift_10750` — extents sum, no z.
            // The chain read of the wizard's record is PRE-move for
            // every building below the carpet slot (all of mc2l0's),
            // so the human tests at the previous settled pose; pool
            // class-3 records test in ascending-slot order (the
            // chain-order approximation of `dword_38519`).
            let (bx, by, bw, bh) = {
                let e = &self.ent[i];
                (e.x, e.y, e.f80 as i32, e.f82 as i32)
            };
            let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
            for w in 1..self.ent.len() {
                let e = &self.ent[w];
                if e.class64 != 3 || e.flags & 0x400 != 0 {
                    continue;
                }
                if wd(e.x, bx) < bw + e.f80 as i32 && wd(e.y, by) < bh + e.f82 as i32 {
                    let (dest, row, own) = (
                        (e.dest_x, e.dest_y, e.site_z),
                        e.f26.clamp(0, 7) as u8,
                        e.id24,
                    );
                    if self
                        .mc2_spawn_wizard_painter(dest, row, own, w as u16)
                        .is_some()
                    {
                        self.ent[w].f46 = 4;
                    }
                }
            }
            if let Some((pose, slot)) = human {
                let pw = (self.mc2_params_ext(44).0 / 2) as i32;
                if wd(pose.0, bx) < bw + pw && wd(pose.1, by) < bh + pw {
                    // The human's spare axis @0x9A is unwritten on
                    // the wizard body — (0,0,0), row 0 (@0x10).
                    self.mc2_spawn_wizard_painter(
                        (0, 0, 0),
                        0,
                        crate::mc1::mobs::PLAYER_TARGET,
                        slot,
                    );
                }
            }
            return true;
        }

        // First countdown tick: the footprint kill (:27310-28).
        if self.ent[i].max_life as i32 - 1 == life {
            for dy in 0..h {
                for dx in 0..w {
                    let t = crate::engine::features::tile(
                        tlx.wrapping_add(dx as u8),
                        tly.wrapping_add(dy as u8),
                    );
                    self.mc2_building_clear_tile(t, self.ent[i].id24);
                }
            }
        }

        // Height lerp toward pad height + base (:27341-44), marking
        // touched flat tiles as village ground (angle low bits 1);
        // then the cave headroom-bubble ceiling lerp on EVERY
        // footprint cell — pad or not (:27349-73).
        for dy in 0..h {
            for dx in 0..w {
                let cell = dy * w + dx;
                let pad = cells[2 * cell + 1];
                let t = crate::engine::features::tile(
                    tlx.wrapping_add(dx as u8),
                    tly.wrapping_add(dy as u8),
                );
                if pad != 0xff {
                    let target = pad as i32 + base as i32;
                    let cur = self.t.height[t] as i32;
                    self.t.height[t] = (cur + (target - cur) / life as i32) as u8;
                    if self.t.angle[t] & 7 == 0 {
                        self.t.angle[t] = (self.t.angle[t] & 0xF0) | 1;
                        let (cx2, cy2) = (tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                        self.mc2_retile_region(cx2, cy2, cx2, cy2);
                    }
                }
                if cave_raise {
                    let bubble = (self.t.height[t] as i32).max(base as i32) + 80;
                    let bubble = bubble.min(255);
                    let cur = self.t.ceiling[t] as i32;
                    if bubble > cur {
                        self.t.ceiling[t] = (cur + (bubble - cur) / life as i32) as u8;
                    }
                    self.cave_seal_fixup(t);
                }
            }
        }

        // Every 5th tick + the last (:27381-27427): the walkable
        // village pre-paint for cells with a paint code, then the
        // sub_45DC0 texture-band overpaint (the code interpreter;
        // painted cells self-lock via angle bit 7 so the next village
        // pass can't clobber them).
        if life % 5 == 0 || life == 1 {
            for dy in 0..h {
                for dx in 0..w {
                    if cells[2 * (dy * w + dx)] == 0xff {
                        continue;
                    }
                    let t = crate::engine::features::tile(
                        tlx.wrapping_add(dx as u8),
                        tly.wrapping_add(dy as u8),
                    );
                    self.t.angle[t] = (self.t.angle[t] & 0xF0) | 1;
                    self.t.tile_type[t] = 1;
                }
            }
            for dy in 0..h {
                for dx in 0..w {
                    let code = cells[2 * (dy * w + dx)];
                    if code == 0xff {
                        continue;
                    }
                    self.mc2_paint_cell(
                        dx as u8,
                        tlx.wrapping_add(dx as u8),
                        tly.wrapping_add(dy as u8),
                        code,
                    );
                }
            }
        }
        true
    }

    /// `GetRandManaSphere_38270` (:27917) — one occupant out of a
    /// dying/besieged building: ONE entity-RNG draw %12 → 0-1 archers
    /// (dock 33), 2-3 trader (113), 4-8 villager (105), 9-11 settler
    /// (97).
    pub(crate) fn mc2_rand_occupant(&mut self, i: usize, x: u16, y: u16, z: i16) -> Option<usize> {
        let d = self.mc2_rand(i) % 12;
        let (s, dock) = match d {
            0 | 1 => (self.mc2_spawn_archers(x, y, z), 33),
            2 | 3 => (self.mc2_spawn_m14(x, y, z), 113),
            4..=8 => (self.mc2_spawn_villager(x, y, z), 105),
            _ => (self.mc2_spawn_m12(x, y, z), 97),
        };
        let s = s?;
        self.ent[s].tick70 = dock;
        Some(s)
    }

    /// `AddHouse0A_2D_38330` (:27959), the parked building (state
    /// 52): the CompareEvent08_38B00 damage core (death → state 53),
    /// the militia pop on a non-lethal hit, the possess-claim intake
    /// (claimed buildings fly the flag), and the per-tick terrain
    /// z-snap.
    ///
    /// APPROX register: the mana-sphere production roll (:28040-58,
    /// full enterable houses) and SetMaxDistance_5C8D0 are OPEN
    /// (economy track). The claimed sprite-row colorize
    /// (`word_0x5A_90 += color`, :28039) is ported: flag row 177 +
    /// owner color, index shifted AFTER the extent derivation off the
    /// base row (there is no team-tint stage in the billboard pass —
    /// the earlier note claiming one was wrong, and bare 177 flew the
    /// human's flag on rival-claimed houses).
    pub(crate) fn mc2_house_tick(&mut self, i: usize) {
        // CompareEvent08_38B00 (:28255): 0 idle / 1 hit / 2 dead.
        self.ent[i].f40 = 0;
        let status = if self.ent[i].act_life < 0 {
            2
        } else if self.ent[i].mail[0].1 != 0 {
            let (amt, src) = self.ent[i].mail[0];
            self.ent[i].act_life -= amt as i32;
            self.ent[i].f40 = src;
            if self.ent[i].act_life < 0 {
                self.ent[i].f38 = src;
                2
            } else {
                self.ent[i].mail[0] = (0, 0);
                1
            }
        } else {
            0
        };
        if status == 2 {
            // Lethal: the RemoveCastleStage_385C0 teardown (state 53).
            self.ent[i].tick70 = 53;
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            self.ent[i].z = self.ground_z(x, y) as i16;
            return;
        }
        if status == 1 && self.ent[i].f26 > 2 {
            // Militia pop (:27994-28015): one occupant out to defend
            // (enterable kind only), and the attacker goes wanted.
            self.ent[i].f26 -= 1;
            let bldg = self.ent[i].f71 as usize;
            let enterable = self
                .assets
                .bldgprm
                .get(bldg)
                .is_some_and(|b| b.flags & 1 != 0);
            if enterable {
                let (x, y, z, off, atk) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z, e.f80, e.f40)
                };
                if let Some(a) = self.mc2_spawn_archers(x.wrapping_add(off), y, z) {
                    self.ent[a].tick70 = 33;
                    self.ent[a].mail[0] = (1, atk);
                }
            }
            let atk = self.ent[i].f40;
            self.mc2_arm_wanted(atk);
        }
        // The claim intake (:28016-42): possess ch1 → new owner,
        // chime 4 at the claimer, flag bit 0 cleared (the flag
        // FLIES), sprite re-set. Claimability is the DELIVERY's
        // f56-bit-1 gate — stone templates (bldgprm flags & 8) never
        // set it, so they can never receive this mail. The mail
        // AMOUNT is retail's `dword_0x64_100` force flag: a FORCED
        // claim (the tier-2 (10,70) pulse) steals unconditionally and
        // sets the claim lock (`byte[2] |= 0x20`, EF:28026); a weak
        // claim bounces off a locked building (EF:28031).
        if self.ent[i].mail[1].1 != 0 {
            let (force, src) = self.ent[i].mail[1];
            self.ent[i].mail[1] = (0, 0);
            if src != self.ent[i].f144 && (force != 0 || self.ent[i].flags & F_CLAIM_LOCK == 0) {
                self.ent[i].f144 = src;
                self.ent[i].flags &= !1;
                if force != 0 {
                    self.ent[i].flags |= F_CLAIM_LOCK;
                }
                if src == crate::mc1::mobs::PLAYER_TARGET {
                    self.snd_player(4);
                }
                self.mc2_set_sprite(i, 177);
                // Owner recolor (EF:28035-40; castle-builder trace
                // `+90 += TransformPlayerColorIndex`): the flag INDEX
                // shifts to the owner's ART row AFTER the extent
                // derivation off the base row — flag family 177 +
                // COLOR_ART[slot], same as the rival castle flag. Bare
                // 177 flew the HUMAN's flag on rival-claimed houses.
                let team = self.owner_team(src).unwrap_or(0);
                self.ent[i].type86 = 177 + crate::mc2::color_art(team) as u16;
            }
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
    }

    // ---- dispatch + awake --------------------------------------------------

    /// The MC2 class-5 per-state dispatch (`sub_57730`'s class-5
    /// table, :40116/:1242) — the MovementVerb::Mc2 arm. Unknown
    /// actions disable the entity like retail's invalid-row path
    /// (:40177) and count a misfit.
    pub(crate) fn mc2_creature_tick(&mut self, i: usize, ctx: &MobCtx) {
        // The ALLIANCE clock (`sub_1E9C0` head EF:10873 + expiry
        // EF:11003-10): the charm counts down in EVERY state (the
        // tier's lever IS the duration) and reverts through the
        // kind-10 resume shim on expiry or parent death; it also
        // re-enters the controlled slot after a combat resolves.
        if self.ent[i].site_z == 14 {
            self.mc2_alliance_clock(i);
        }
        let action = self.ent[i].tick70;
        // The shared class-5 `8*M+7` slot (`sub_1D5D0`, EF:9977) — a
        // CONTROLLED creature. StageVar2 (port field: site_z, free on
        // creatures) selects the body: 12 = Metamorph pose-puppet, 13 =
        // Summon-Army allied AI. Stage-HELD kinds (1..=10, 15) never
        // reach here — the world dispatch seam routes them through
        // `World::mc2_held_tick` (stagevars.rs).
        // StageVar2 == 0 (every ordinary spawn) is a no-op, so those
        // fall through to the per-model dispatch
        // (docs/spell-audit/summon-creatures.md).
        if action & 7 == 7 && self.ent[i].site_z != 0 {
            match self.ent[i].site_z {
                12 => self.mc2_metamorph_creature_tick(i, ctx),
                13 => self.mc2_summon_creature_tick(i, ctx),
                14 => self.mc2_alliance_creature_tick(i, ctx),
                // 16/17 = the pyramid-summon release chain.
                16 => self.mc2_doom_summon_home_tick(i, ctx),
                17 => self.mc2_doom_summon_spinup_tick(i, ctx),
                _ => {}
            }
            return;
        }
        match action {
            0..=7 => self.m0_tick(i, ctx),
            8..=15 => self.goat_tick(i, ctx),
            16..=23 => self.m2_tick(i, ctx),
            24..=31 => self.m3_tick(i, ctx),
            32..=39 => self.archer_tick(i, ctx),
            72..=79 => self.m9_tick(i, ctx),
            96..=103 => self.m12_tick(i, ctx),
            104..=111 => self.villager_tick(i, ctx),
            112..=119 => self.m14_tick(i, ctx),
            120..=127 => self.m15_tick(i, ctx),
            128..=135 => self.m16_tick(i, ctx),
            136..=143 => self.m17_tick(i, ctx),
            144..=151 => self.m18_tick(i, ctx),
            152..=159 => self.m19_tick(i, ctx),
            160..=167 => self.m20_tick(i, ctx),
            168..=175 => self.m21_tick(i, ctx),
            176..=183 => self.m22_tick(i, ctx),
            184..=191 => self.m23_tick(i, ctx),
            192..=199 => self.m24_tick(i, ctx),
            200..=207 => self.m25_tick(i, ctx),
            208..=215 => self.m26_tick(i, ctx),
            216..=223 => self.m27_tick(i, ctx),
            224..=231 => self.m28_tick(i, ctx),
            // The m0/m3 child follow (sub_1B6B0, table 0xE8).
            232 => self.mc2_child_tick(i),
            // m27 branches / tier-2 segments: NULL table entries —
            // body-driven via sub_29A90, never self-dispatched.
            233 | 234 => {}
            _ => {
                self.note_misfit(5, self.ent[i].model65 as u16);
                self.ent[i].flags |= 0x400;
            }
        }
    }

    /// `sub_1E4D0` (EF:10650), StageVar2 == 12 — the METAMORPH creature:
    /// a cosmetic pose-PUPPET slaved to the caster every tick (position +
    /// facing copied). The engine never rebinds control — the wizard
    /// stays under normal control and keeps casting; the carpet is just
    /// hidden (player.metamorph) and this creature draws in its place.
    /// The human is out of the pool, so the parent pose comes from `ctx`
    /// (the live player pose), not a pooled parent. The per-model z
    /// offset (m16 −896, m25 −512, EF:10664-74) drops the creature's
    /// origin so its sprite aligns where the carpet was. Teardown rides
    /// the cast window (mc2_cast_expire). No autonomous combat.
    fn mc2_metamorph_creature_tick(&mut self, i: usize, ctx: &MobCtx) {
        let off: i16 = match self.ent[i].model65 {
            16 => 896,
            25 => 512,
            _ => 0,
        };
        let z = ctx.pz.saturating_sub(off);
        self.move_relink(i, ctx.px, ctx.py, z);
        self.ent[i].f30 = ctx.pyaw;
        self.ent[i].f34 = ctx.pyaw;
        // The creature's cry LOOPS while morphed — the FP effect: no
        // visible sprite from first person, just the monster's scream
        // on a loop (plus the distinct Morph cast sound 60). Play the
        // model's characteristic cry on a ~24-tick loop, anchored at
        // the creature (= the player pose).
        if self.ent[i].f26 <= 0 {
            let cry = match self.ent[i].model65 {
                16 => 39, // Wyvern
                25 => 37, // Cymmerian
                2 => 12,  // Day creature
                _ => 43,  // FireFly (19)
            };
            self.snd(cry, i);
            self.ent[i].f26 = 24;
        } else {
            self.ent[i].f26 -= 1;
        }
    }

    /// `sub_1E580` (EF:10689), StageVar2 == 13 — the SUMMON-ARMY allied
    /// creature: free-roam AI that hunts enemy wizards for the caster
    /// (no player input). Acquire the nearest enemy wizard (class 3,
    /// model ≤ 1, not our team); with none, follow the caster; face and
    /// move toward it via the creature move core; once in engage range,
    /// hand off to the model's normal `+2` attack state (the landed
    /// class-5 combat). Self-expires after its 250-tick life (`f26`) with
    /// a fire puff. The idle-follow + acquire resolve the caster to the
    /// out-of-pool human via `ctx` (docs/spell-audit/summon-creatures.md).
    fn mc2_summon_creature_tick(&mut self, i: usize, ctx: &MobCtx) {
        // Life countdown (word_0x2E_46 → f26): expire with a puff.
        self.ent[i].f26 -= 1;
        if self.ent[i].f26 <= 0 {
            let (x, y, z) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            self.mc2_spawn_fire(x, y, z);
            self.ent[i].flags |= 0x400;
            return;
        }
        let own = self.ent[i].id24;
        let (mx, my) = (self.ent[i].x, self.ent[i].y);
        // Re-acquire on the throttle (byte_0x3E_62 & 7) or when the lock
        // is stale — nearest ENEMY wizard by 2-D distance.
        let mut target = self.ent[i].f146;
        let valid = target != 0
            && target != crate::mc1::mobs::PLAYER_TARGET
            && (target as usize) < self.ent.len()
            && self.ent[target as usize].class64 == 3
            && self.ent[target as usize].model65 <= 1
            && self.ent[target as usize].flags & 0x400 == 0
            && self.ent[target as usize].act_life >= 0;
        if !valid && self.ent[i].f63 & 7 == 0 {
            target = 0;
            let mut best = i32::MAX;
            for j in 1..self.ent.len() {
                let e = &self.ent[j];
                if e.class64 != 3
                    || e.model65 > 1
                    || e.id24 == own
                    || e.flags & 0x400 != 0
                    || e.act_life < 0
                {
                    continue;
                }
                let d = Self::dist2_sq(mx, my, e.x, e.y);
                if d < best {
                    best = d;
                    target = j as u16;
                }
            }
            self.ent[i].f146 = target;
        }
        // Face + move toward the target, or follow the caster (the human,
        // resolved via ctx) when there is none.
        let (tx, ty) = if target != 0 && (target as usize) < self.ent.len() {
            (self.ent[target as usize].x, self.ent[target as usize].y)
        } else {
            (ctx.px, ctx.py)
        };
        let yaw = Self::angle_between(mx, my, tx, ty);
        self.ent[i].f34 = yaw;
        self.mc2_move_core(i);
        // In engage range → hand off to the model's `+2` attack state
        // (leaving the controlled slot: StageVar2 → 0).
        if target != 0 {
            let d = Self::isqrt(Self::dist2_sq(mx, my, tx, ty) as u32);
            if d < 1536 {
                self.ent[i].tick70 = self.ent[i].model65.wrapping_mul(8).wrapping_add(2);
                self.ent[i].site_z = 0;
            }
        }
    }

    /// A pyramid-summon target's position: PLAYER_TARGET is the
    /// out-of-pool human (never invalid — its death restarts the
    /// level), pool slots validate like retail's `Entities[t] >
    /// Entities[0] && life >= 0 && !(byte[1] & 4)` probe.
    fn mc2_doom_target_pos(&self, t: u16, ctx: &MobCtx) -> Option<(u16, u16, i16)> {
        if t == PLAYER_TARGET {
            return Some((ctx.px, ctx.py, ctx.pz));
        }
        let j = t as usize;
        if j == 0 || j >= self.ent.len() {
            return None;
        }
        let e = &self.ent[j];
        if e.flags & 0x400 != 0 || e.act_life < 0 {
            return None;
        }
        Some((e.x, e.y, e.z))
    }

    /// `sub_1E320` (EF:10566), StageVar2 == 17 — the pyramid-summon
    /// SPIN-UP: the hurled creature keeps flying at the summon's 320
    /// while decelerating `f126 -= 8`/tick and turning onto its
    /// target; at ≤ 16 it takes the per-model cruise (m0 → 30,
    /// m19 → 76, m21 → 96, m25 unchanged, EF:10588-601) and drops to
    /// the StageVar2-16 homing slot. An invalid target skips straight
    /// to slot 16 with the speed untouched (the `goto LABEL_14`).
    ///
    /// The head is the MOVE CORE ONLY, then a BARE life test
    /// (EF:10572-76) — `sub_1B8C0` is `mc2_move_core`, not the damage
    /// intake, and MC2 damage reaches an entity solely through the
    /// accumulate-mailbox that a state handler's head drains
    /// (EF:4023-25 and twins; `Gen::mail_write`). So retail applies
    /// NOTHING during the ~37-tick launch flight: a hit taken in
    /// flight stays QUEUED and is consumed on the first slot-16 tick,
    /// where it becomes either the `v2==1` retarget (the creature
    /// leaves the summon lane at once) or the `v2==2` husk. Draining
    /// the mailbox here instead swallowed that first hit — a
    /// non-fatal one lost retail's tick-1 retarget-out and left the
    /// creature in the husk-prone lane longer, and a fatal one made
    /// it vanish outright with no death animation and no puff. The
    /// life test is therefore unreachable in practice, exactly as in
    /// retail; it is kept because retail keeps it.
    fn mc2_doom_summon_spinup_tick(&mut self, i: usize, ctx: &MobCtx) {
        self.mc2_move_core(i);
        if self.ent[i].act_life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let target = self.ent[i].f146;
        if let Some((tx, ty, _)) = self.mc2_doom_target_pos(target, ctx) {
            let (mx, my) = (self.ent[i].x, self.ent[i].y);
            self.ent[i].f34 = Self::angle_between(mx, my, tx, ty);
            self.ent[i].f126 -= 8;
            if self.ent[i].f126 > 16 {
                return;
            }
            match self.ent[i].model65 {
                0 => self.ent[i].f126 = 30,
                19 => self.ent[i].f126 = 76,
                21 => self.ent[i].f126 = 96,
                _ => {}
            }
        }
        self.ent[i].site_z = 16;
    }

    /// `sub_1E580` (EF:10689), StageVar2 == 16 — the pyramid-summon
    /// HOME slot: case 13's Summon-Army twin WITHOUT the per-tick
    /// life decrement (EF:10703-06 — pyramid summons persist while
    /// the pyramid lives; the life home is the spawn block's `f26`).
    /// Parent death zeroes the life → expire with a fire puff. The
    /// latch's home is **f26** — the class-5 @0x2E lane (the same home
    /// the StageVar2-13 Summon-Army twin counts down and the same one
    /// the conformance importer restores); it used to ride f46, which
    /// on a creature is `fontTypeIndex_0x3D_61` (the m0 dodge window)
    /// and has no @0x2E import, so replayed summons puffed on sight.
    /// Otherwise the `sub_1E700` core runs: mailbox intake (a KILL
    /// stamps @0x2E = 1 and freezes the body — no move, no state
    /// change, EF:10864-66), a hit re-targets the
    /// attacker — never the parent or a same-species peer; flee rows
    /// hand to +6, others +2 (the retail parent-XP `sub_6D8B0` award
    /// is a wizard-only no-op for the pyramid) — and the quiet path
    /// aims at the target on the 8-tick throttle with the 64-tick
    /// wander jink and the same-model crowd steer-away (EF:10814-40).
    /// ALL THREE arms then end in the engage check — the dead one
    /// included, because `sub_1E700` never returns early: 3-D reach
    /// inside the row's `v_28` hands to the model's +2 attack
    /// (site_z stays 16, as retail leaves StageVar2). That is how a
    /// husk leaves the lane: the frozen corpse keeps testing the
    /// reach on the `f63 & 7` throttle, converts to `8m+2`, and the
    /// model handler's own head turns `life < 0` into `8m+4` — the
    /// ordinary death animation. Corpus (mc2l24): NO doom summon
    /// anywhere in the take dies in `8m+7` — slot 573 (5,0) leaves
    /// via the `v2==1` retarget one tick into the lane (t=60142),
    /// slots 772/820 (5,19) leave via this engage check at FULL life
    /// (t=60153/60161) and are one-shot later in `8m+2`, each with a
    /// full 8-tick `8m+4`→`8m+5` death. Parent link: the level authors
    /// exactly ONE (5,10), scan-resolved (`parentId_0x28_40` has no
    /// entity home — the spawn-block APPROX).
    fn mc2_doom_summon_home_tick(&mut self, i: usize, ctx: &MobCtx) {
        let parent = (1..self.ent.len()).find(|&j| {
            let e = &self.ent[j];
            e.class64 == 5 && e.model65 == 10 && e.flags & 0x400 == 0 && e.act_life >= 0
        });
        if parent.is_none() {
            self.ent[i].f26 = 0;
        }
        if self.ent[i].f26 <= 0 {
            let (x, y, z) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            };
            self.mc2_spawn_fire(x, y, z);
            self.ent[i].flags |= 0x400;
            return;
        }
        // Stale pool locks clear; the 8-tick re-acquire resolves the
        // pyramid's standing enemy (`sub_16FC0(parent)`) — the
        // out-of-pool player.
        let mut target = self.ent[i].f146;
        if target != 0 && self.mc2_doom_target_pos(target, ctx).is_none() {
            self.ent[i].f146 = 0;
            target = 0;
        }
        if target == 0 && self.ent[i].f63 & 7 == 0 {
            target = PLAYER_TARGET;
            self.ent[i].f146 = target;
        }
        if target == 0 {
            // Parentward drift + fast decay while unlocked
            // (EF:10725-31: aim the move at the parent, @0x2E -= 4).
            if let Some(p) = parent {
                let (mx, my) = (self.ent[i].x, self.ent[i].y);
                let (px, py) = (self.ent[p].x, self.ent[p].y);
                self.ent[i].f34 = Self::angle_between(mx, my, px, py);
            }
            // Retail runs the FULL `sub_1E700` core here (with
            // `word_0x96_150 = parentId`), THEN reads the latch back
            // and subtracts 4 (EF:10727-30) — so a summon that died
            // on this very tick reads the core's `= 1` and lands on
            // −3, expiring next tick, instead of draining its live
            // latch by 4s for ~62 ticks. APPROX: the port keeps the
            // manual parent-aim above and skips the core's wander
            // jink / crowd steer / `v2==1` retarget (whose
            // `word_0x96_150` lock retail clobbers back to 0 two
            // lines later anyway, EF:10729).
            if self.mc2_state_head(i) == 2 {
                self.ent[i].f26 = 1;
            } else {
                self.mc2_move_core(i);
            }
            self.ent[i].f26 -= 4;
            return;
        }
        // The `sub_1E700` core. NOTE the dead arm does NOT return:
        // retail's `else if (v2 == 2)` stamps the latch and falls off
        // the end of `sub_1E700` (EF:10864-66), so control lands back
        // on the caller's engage check at EF:10735 — a DEAD husk
        // keeps testing the reach and converts to the model's `+2`
        // the moment it is inside `v_28`, whereupon that handler's
        // own head sees `life < 0` and hands to `+4`, the ordinary
        // death animation. Returning here stranded the husk in
        // `8m+7` until the pyramid itself died (player-reported
        // "frozen forever", 2026-08-05).
        match self.mc2_state_head(i) {
            2 => {
                self.ent[i].f26 = 1;
            }
            1 => {
                self.mc2_move_core(i);
                let atk = self.ent[i].f40;
                let same_species = atk != 0
                    && atk != PLAYER_TARGET
                    && (atk as usize) < self.ent.len()
                    && self.ent[atk as usize].class64 == self.ent[i].class64
                    && self.ent[atk as usize].model65 == self.ent[i].model65;
                let is_parent = atk != PLAYER_TARGET && parent.is_some_and(|p| p == atk as usize);
                if atk != 0 && !same_species && !is_parent {
                    self.ent[i].f146 = atk;
                    target = atk;
                    let flee =
                        BEHAVIOR[self.ent[i].row156 as usize].flags & Mc2BehaviorRow::FLEE != 0;
                    self.ent[i].tick70 = self.ent[i]
                        .model65
                        .wrapping_mul(8)
                        .wrapping_add(if flee { 6 } else { 2 });
                }
            }
            _ => {
                self.mc2_move_core(i);
                if self.ent[i].f63 & 7 == 0 {
                    if let Some((tx, ty, _)) = self.mc2_doom_target_pos(target, ctx) {
                        let (mx, my) = (self.ent[i].x, self.ent[i].y);
                        if self.ent[i].flags & (1 << 18) == 0 {
                            self.ent[i].f34 = Self::angle_between(mx, my, tx, ty);
                            if self.ent[i].f63 & 0x3F == 0 {
                                self.mc2_wander_turn(i);
                            }
                        }
                        // Same-model crowd steer-away (EF:10829-38):
                        // the first DIFFERENT-owner same-species
                        // neighbour inside the f80 footprint box
                        // turns the heading straight away from it.
                        let pitch = self.ent[i].f80 as i32;
                        for j in 1..self.ent.len() {
                            if j == i {
                                continue;
                            }
                            let e = &self.ent[j];
                            if e.class64 != 5
                                || e.model65 != self.ent[i].model65
                                || e.flags & 0x400 != 0
                                || e.id24 == self.ent[i].id24
                            {
                                continue;
                            }
                            if (mx as i32 - e.x as i32).abs() < pitch
                                && (my as i32 - e.y as i32).abs() < pitch
                            {
                                self.ent[i].f34 = Self::angle_between(e.x, e.y, mx, my);
                                break;
                            }
                        }
                    }
                }
            }
        }
        // The engage handoff (EF:10735-40): 8-tick throttle, 3-D
        // reach inside the row's `v_28` → the model's +2 attack.
        if self.ent[i].f63 & 7 == 0
            && let Some(tp) = self.mc2_doom_target_pos(target, ctx)
        {
            let me = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            let reach = BEHAVIOR[self.ent[i].row156 as usize].v_28.max(0) as u32;
            if Self::mc2_dist3(me, tp) < reach {
                self.ent[i].tick70 = self.ent[i].model65.wrapping_mul(8).wrapping_add(2);
            }
        }
    }

    /// `sub_3A650` (EF:29637; the (10,74) executor's class-10 action
    /// 0x51) — the ALLIANCE conversion: a SAME-SPECIES area charm.
    /// Sweep a square of tile-radius `radius` (the tier's 16/26/32)
    /// around the struck creature; every living creature of the
    /// victim's MODEL passing the `sub_3A7F0` eligibility filter
    /// (EF:29701) converts: sound 6, StageVar2 = 14, owner → the
    /// caster (the `mc2_allied` side table — `id24` stays the
    /// authored disposition), duration → f26, and either its target
    /// clears (mid-attack, `action & 7 == 2`) or it enters the
    /// model's controlled slot `8m+7` (EF:29660-90). Zero damage.
    /// APPROX: retail also converts stage-HELD creatures (StageVar1
    /// saved to `word_0x4A_74`, restored on expiry) — the port skips
    /// creatures under a live hold or another charm.
    pub(crate) fn mc2_alliance_convert(
        &mut self,
        victim: u16,
        parent: u16,
        radius: i32,
        duration: i32,
    ) {
        let v = victim as usize;
        if victim == 0 || victim == PLAYER_TARGET || v >= self.ent.len() || self.ent[v].class64 != 5
        {
            return;
        }
        let model = self.ent[v].model65;
        // `sub_3A7F0`'s model bar: 12-15, 22, 23, 26, 27 are never
        // charmable — a victim of a barred species converts nothing.
        if matches!(model, 12..=15 | 22 | 23 | 26 | 27) {
            return;
        }
        let (vx, vy) = (self.ent[v].x as i32 >> 8, self.ent[v].y as i32 >> 8);
        let dur = duration.clamp(1, i16::MAX as i32) as i16;
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if e.class64 != 5
                || e.model65 != model
                || e.flags & 0x400 != 0
                || e.act_life < 0
                || ((e.x as i32 >> 8) - vx).abs() > radius
                || ((e.y as i32 >> 8) - vy).abs() > radius
                // Charmed (13/14/16/17) or stage-held (port APPROX,
                // doc above) — only free-roaming creatures convert.
                || !matches!(e.site_z, 0 | 10)
                // The child-follow state (232) and the flagged m25
                // are ineligible (EF:29701-29726).
                || e.tick70 == 232
                || (model == 25 && e.f71 != 0)
            {
                continue;
            }
            self.snd(6, j);
            let e = &mut self.ent[j];
            e.site_z = 14;
            e.f26 = dur;
            if e.tick70 & 7 == 2 {
                e.f146 = 0;
            } else {
                e.tick70 = model.wrapping_mul(8).wrapping_add(7);
            }
            self.mc2_allied.0.insert(j as u16, parent);
        }
    }

    /// The per-tick half of the alliance law (`sub_1E9C0` head
    /// EF:10873 + expiry EF:11003-10), run from the class-5 dispatch
    /// head in EVERY state: count the charm down, revert on expiry /
    /// parent death through the kind-10 resume shim (`id24` was never
    /// touched, so the authored disposition simply resumes), and
    /// re-enter the controlled slot once a combat resolves (retail
    /// returns controlled creatures to `8m+7`; our model machines
    /// drop to their wander phases 0/1 instead).
    fn mc2_alliance_clock(&mut self, i: usize) {
        if self.ent[i].flags & 0x400 != 0 || self.ent[i].act_life < 0 {
            self.mc2_allied.0.remove(&(i as u16));
            return;
        }
        let parent = self.mc2_allied.0.get(&(i as u16)).copied().unwrap_or(0);
        // Parent-death probe on the 8-tick cadence (pool wizards by
        // owner id; the human parent's death restarts the level, so
        // it counts as alive here).
        let mut parent_dead = parent == 0;
        if parent != 0 && parent != PLAYER_TARGET && self.ent[i].f63 & 7 == 0 {
            parent_dead = !(1..self.ent.len()).any(|j| {
                let e = &self.ent[j];
                e.class64 == 3
                    && e.model65 <= 1
                    && e.id24 == parent
                    && e.flags & 0x400 == 0
                    && e.act_life >= 0
            });
        }
        self.ent[i].f26 -= 1;
        if self.ent[i].f26 <= 0 || parent_dead {
            self.ent[i].site_z = 10;
            self.ent[i].f146 = 0;
            self.mc2_allied.0.remove(&(i as u16));
            return;
        }
        if self.ent[i].tick70 & 7 < 2 {
            self.ent[i].tick70 = self.ent[i].model65.wrapping_mul(8).wrapping_add(7);
        }
    }

    /// `sub_1E9C0` (EF:10873), StageVar2 == 14 — the ALLIANCE-charmed
    /// creature's controlled slot: fight the caster's fight. Retail
    /// adopts the parent wizard's target/attacker words; the port's
    /// out-of-pool human keeps neither, so the observable equivalent
    /// serves: the nearest pool entity currently TARGETING the parent
    /// (its attacker), else the nearest enemy wizard. Never a fellow
    /// ally of the same parent (EF:10984). Engage hands to the
    /// model's `8m+2` attack KEEPING StageVar2 = 14 (the clock keeps
    /// counting and re-arms the slot after combat) and awards the
    /// caster Alliance XP (`sub_6D8B0(parentId, 0x18, 1)`, EF:10998).
    fn mc2_alliance_creature_tick(&mut self, i: usize, _ctx: &MobCtx) {
        let parent = self.mc2_allied.0.get(&(i as u16)).copied().unwrap_or(0);
        let (mx, my) = (self.ent[i].x, self.ent[i].y);
        let mut target = self.ent[i].f146;
        let stale = target == 0
            || target == PLAYER_TARGET
            || (target as usize) >= self.ent.len()
            || self.ent[target as usize].flags & 0x400 != 0
            || self.ent[target as usize].act_life < 0;
        if stale {
            target = 0;
            self.ent[i].f146 = 0;
        }
        if target == 0 && self.ent[i].f63 & 7 == 0 {
            let mut best = i32::MAX;
            for j in 1..self.ent.len() {
                let e = &self.ent[j];
                if j == i || e.flags & 0x400 != 0 || e.act_life < 0 {
                    continue;
                }
                if self.mc2_allied.0.get(&(j as u16)) == Some(&parent) {
                    continue;
                }
                let attacks_parent = parent == PLAYER_TARGET
                    && matches!(e.class64, 3 | 5)
                    && e.f146 == PLAYER_TARGET;
                let enemy_wizard = e.class64 == 3 && e.model65 <= 1 && e.id24 != parent;
                if !(attacks_parent || enemy_wizard) {
                    continue;
                }
                let d = Self::dist2_sq(mx, my, e.x, e.y);
                if d < best {
                    best = d;
                    target = j as u16;
                }
            }
            self.ent[i].f146 = target;
        }
        if target == 0 {
            return; // no fight to join — stand by (retail idles too)
        }
        let (tx, ty) = (self.ent[target as usize].x, self.ent[target as usize].y);
        let yaw = Self::angle_between(mx, my, tx, ty);
        self.ent[i].f34 = yaw;
        self.mc2_move_core(i);
        let d = Self::isqrt(Self::dist2_sq(mx, my, tx, ty) as u32);
        if d < 1536 {
            self.ent[i].tick70 = self.ent[i].model65.wrapping_mul(8).wrapping_add(2);
            self.mc2_cast_xp.0.push((parent, 24, 1));
        }
    }

    /// The MC2 class-9 dispatch — the TargetingVerb::Mc2 arm's
    /// projectile side. Only the (9,13) arrow is MC2-ported; every
    /// other flight state falls back to the MC1 projectile handler
    /// with a fallback note — the player's spells stay MC1 until the
    /// MC2 spell column lands (deliberate cross-column play, the
    /// seam's graceful-degradation contract).
    pub(crate) fn mc2_proj_tick(&mut self, i: usize, ctx: &MobCtx) {
        // MC2-native projectiles carry the F_MC2PROJ marker (their
        // ctors set it); MC1-fallback spawns never do, so state
        // numbers can't collide across the columns.
        if self.ent[i].flags & super::proj::F_MC2PROJ != 0 {
            // The creature-launched family all rides the shared
            // flyer core (sub_65820 ≡ states 2..8, 0x0B, 0x0E-0x1C;
            // state 0's CastPlayerFire delta is initial-aim only —
            // creature launches pre-aim, so the core serves). The
            // (9,3) meteor shot's action-3 wrapper adds the trailing
            // spark (sub_66180, mc2::proj).
            if self.ent[i].model65 == 10 && self.ent[i].tick70 == 10 {
                // The castle ball rides its own dedicated flight
                // (CastCastleProjectile_66B30 / sub_66D00) — the
                // generic flyer's water arm was splashing the build
                // away (mc2l3 t=244's (10,5) where retail builds).
                self.mc2_castle_ball_tick(i);
            } else if self.ent[i].model65 == 3 && self.ent[i].tick70 == 3 {
                self.mc2_meteor_shot_tick(i, ctx);
            } else if self.ent[i].model65 == 9 && self.ent[i].tick70 == 9 {
                // Lightning L0 (subtype 9) = the `sub_66750` one-tick
                // hitscan BEAM, not a traveling ball. Resolve it whole
                // this tick (docs/spell-audit/lightning.md §5.A) so it
                // flashes to its impact and is gone — under RAPID
                // re-fire that reads as the authentic crackle.
                self.mc2_lightning_beam_tick(i, ctx);
            } else if self.ent[i].model65 == 9 && self.ent[i].tick70 == 14 {
                // The beam's cosmetic sprite-216 trail billboards
                // (`sub_67410`, action 14): inert, self-despawning.
                self.mc2_lightning_node_tick(i);
            } else {
                self.mc2_flyer_tick(i, ctx);
            }
            return;
        }
        match self.ent[i].tick70 {
            // Keyed on model AND state: MC1 flight states (the
            // fallback below) may also use the value 13.
            ARROW_STATE if self.ent[i].model65 == 13 => self.mc2_arrow_tick(i, ctx),
            0xFE => {} // authored inert parking (shared convention)
            _ => {
                self.note_verb_fallback(crate::verbs::VerbKind::Targeting);
                if self.proj_tick(i, ctx) {
                    self.terrain_dirty = true;
                }
            }
        }
    }

    /// The MC2 awake pre-pass (`sub_68BF0`/`sub_68C70`,
    /// :55469/:55494) — the AwakeVerb::Mc2 arm. Order per the
    /// transcript: an armed counter propagates to followers THEN
    /// decrements; a zero counter waits out the wake delay (f59),
    /// then the 2D proximity probe (same 0x2400000 as MC1) arms 16
    /// (followers 18). Dead entities reset to the 0xFA sentinel.
    pub(crate) fn mc2_awake_pass(&mut self, ctx: &MobCtx) {
        for i in 1..self.ent.len() {
            let e = &self.ent[i];
            if e.class64 != 5 || matches!(e.tick70, 0xB4 | 0xE8 | 0xEA) || e.flags & 0x400 != 0 {
                continue;
            }
            if e.act_life < 0 {
                self.ent[i].f58 = 0xFA;
                self.ent[i].f59 = 0;
                continue;
            }
            self.mc2_awake_one(i, ctx);
        }
        // sub_68BF0's SECOND loop (EF:55489-90): dword_38523 = the
        // mana-sphere family awake-ticks too — spheres near the
        // player arm their f58 like creatures do. No dead reset here
        // (retail's sphere loop is unconditional), and NO model test:
        // the chain itself is the filter, and it is built from models
        // 39, 40 AND 57 (EF:40023-40062), so a fool's sphere wakes
        // exactly like a real one.
        for i in 1..self.ent.len() {
            let e = &self.ent[i];
            if e.class64 == 10 && matches!(e.model65, 39 | 40 | 57) && e.flags & 0x400 == 0 {
                self.mc2_awake_one(i, ctx);
            }
        }
    }

    /// One entity's `sub_68C70` body (EF:55494): f58 propagate +
    /// decrement, the HIDDEN-skip, the f59 hold, proximity-wake.
    fn mc2_awake_one(&mut self, i: usize, ctx: &MobCtx) {
        if self.ent[i].f58 != 0 {
            let v = self.ent[i].f58;
            let mut j = self.ent[i].f54 as usize;
            while j != 0 {
                self.ent[j].f58 = v;
                j = self.ent[j].f54 as usize;
            }
            self.ent[i].f58 = v - 1;
            return;
        }
        // The hidden-skip (`byte[0] & 1`, EF:55515): a hidden entity
        // (burrowed m27 etc.) never proximity-wakes. Registry: flags
        // bit 0 = hidden, bit 5 (0x20) = scan-invisible — both are
        // verbatim byte[0] mappings, distinct from the synthesized
        // high bits (F_STOP &c).
        if self.ent[i].flags & 1 != 0 {
            return;
        }
        if self.ent[i].f59 != 0 {
            self.ent[i].f59 -= 1;
            return;
        }
        let e = &self.ent[i];
        if Self::dist2_sq(e.x, e.y, ctx.px, ctx.py) < 0x240_0000 {
            self.ent[i].f58 = 16;
            let mut j = self.ent[i].f54 as usize;
            while j != 0 {
                self.ent[j].f58 = 18;
                j = self.ent[j].f54 as usize;
            }
        }
        self.ent[i].f59 = 0;
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::features::Gen;

    /// **THE 180° TURN TIE-BREAK IS SIGNED, NOT WRAPPED.** Retail's
    /// `sub_582F0` (Sound.cpp:6580; MC1's `sub_42240_42580` :52664 is
    /// the same body, marked SYNCHRONIZED) takes the plain integer
    /// difference of the two masked angles and unwraps it only when
    /// `abs(v3) > 1024` — STRICTLY greater. An exact half-turn keeps
    /// the raw sign, so a target numerically BELOW the current heading
    /// turns negative and one above turns positive; every other delta
    /// agrees with the wrapped form.
    ///
    /// The two rows this pins are the mc2l24 (5,23) dweller's, both on
    /// slot 363, the residue the riser-replay dig left behind. The move
    /// core's THIRD retry is the antipode (`yaw0 + 0x400`, EF:8846),
    /// and the dweller's wander target (`roll_0x20_32`) still equals
    /// its pre-retry heading, so the commit turn lands exactly on the
    /// tie every time that leg fires:
    ///
    /// | pair | yaw0 = target | retry-3 leg | retail | wrapped form |
    /// |---|---|---|---|---|
    /// | t=15044 | 437 | 1461 | **1205** | 1717 |
    /// | t=15129 | 519 | 1543 | **1287** | 1799 |
    ///
    /// Non-vacuous: `MGC_NO_TURN_TIE=1` restores the wrapped sign and
    /// this test fails on the first assert.
    #[test]
    fn turn_step_breaks_the_exact_half_turn_toward_the_lower_angle() {
        // The recorded (5,23) pairs, row 91's turn cap = 256.
        for (yaw0, leg, want) in [(437u16, 1461u16, 1205i32), (519, 1543, 1287)] {
            assert_eq!(
                yaw0.wrapping_add(0x400) & (0x700 + (yaw0 & 0xFF)),
                leg,
                "retry 3 is the antipode for this yaw"
            );
            let turned = leg as i32 + Gen::turn_step(leg, yaw0, 256) as i32;
            assert_eq!(turned & 0x7FF, want, "half-turn back onto the target");
        }
        // The sign is the RAW difference's, both ways round the tie.
        assert_eq!(Gen::turn_sign(1461, 437), -1, "target below → negative");
        assert_eq!(Gen::turn_sign(437, 1461), 1, "target above → positive");
        // ...and every non-tie delta is unchanged by the law.
        for (cur, tgt) in [(0u16, 1u16), (0, 2047), (0, 1023), (0, 1025), (100, 1500)] {
            let wrapped = if tgt.wrapping_sub(cur) & 0x7FF <= 1024 {
                1
            } else {
                -1
            };
            assert_eq!(
                Gen::turn_sign(cur, tgt),
                wrapped,
                "only the exact half-turn moves ({cur} → {tgt})"
            );
        }
    }
}
