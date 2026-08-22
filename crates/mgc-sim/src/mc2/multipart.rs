//! MC2 multipart/segment-chain subsystem — class-5 models 0 (worm/
//! hydra), 3 (multipart flyer), 22 (segmented worm / castle-mana
//! thief) and 27 (3-tier tree kraken), ported from the trace bank:
//! - docs/traces/mc2-multipart-chains.md (ctors, segment tick,
//!   topology, collision/list skips)
//! - docs/traces/mc2-m0-m3-gaps.md (dispatch table, tether, bob,
//!   PreKillEntity cascade, unrecovered states)
//! - docs/traces/mc2-m22-worm-helpers.md (the m22 helper suite,
//!   drain chain, colorize walkers)
//! - docs/traces/mc2-m27-branch-machine.md (sub_29A90, positioning
//!   spline, data tables)
//!
//! `EF:` cites = remc2 EventsFunctions.cpp.
//!
//! Chain topology (shared): entities link via f52 (toward the head,
//! `word_0x32_50`) and f54 (toward the tail, `word_0x34_52`) — the
//! same homes MC1's worms use. Segment states are collision- and
//! scan-transparent (0xE8 m0/m3 children, 0xB4 m22 tail, 0xEA m27
//! tier-2; 0xE9 m27 branches ARE scannable — EF:39987-40009) — the
//! mc2 scan helpers already carry the exclusions.
//!
//! Field homes beyond the [`super::mobs`]/[`super::roster`] docs
//! (all remc2 names verbatim):
//! - `word_0x36_54` (link length; m22 head reuses it as the 2-bit
//!   writhe phase) → f56 — the MC1 worm's own home. The MC2 burn
//!   mask `byte_0x38_56` (never read at runtime; f28=1 is the
//!   cross-column admit) is NOT stored for these models.
//! - `byte_0x46_70` → f71 (m0 dodge-hook flag · m22 tail length on
//!   the head / SIGNED ring offset on segments, stored as the u8
//!   cast · m27 branch sub-state).
//! - `word_0x2C_44` → f44 (m0 dodge step timer · m22 spin rate ·
//!   m27 speed-mode selector).
//! - `fontTypeIndex_0x3D_61` (m0 dodge-alert window) → f46 — the
//!   effect columns' fontTypeIndex home (flood/tail), free on
//!   these heads.
//! - `subSpellIndex_0x2A_42` (m22 serpentine spiral angle) → f46 —
//!   deviation from the projectile column's f44 home, which m22
//!   already occupies with `word_0x2C_44`.
//! - `word_0x24_36` → f38 (m22 rise budget · m27 body exposure
//!   attacker · m0 hooked-projectile ref). m22/m27 heads never run
//!   the shared inbox; the m0 head DOES, and its death write
//!   (`mc2_state_head` kill-credit, retail's own field reuse)
//!   aliases the hook — [`Gen::m0_dodge`] bounds-guards the read
//!   (the out-of-pool human sentinel must read as "gone").
//! - `word_0x96_150` → f146 (m22 head grow timer / segment head-ref
//!   · m27 branch target). `playerEntityIndex_0x94_148` (the m22
//!   target player) → dest_x — creatures never carry a portal
//!   destination.
//! - `byte_0x3B_59` (m27 branch index 0..4 / the BODY's live-branch
//!   gauge) → f50 — free on this family (MC1's damage-response
//!   countdown never runs on MC2 creatures).
//! - `byte_0x43_67`/`byte_0x44_68` (m27 whip counters) → f68/f69 —
//!   the projectile impact pair is meaningless on creatures.
//! - `manaRegen_0x88_136` (m27 bolt power 1|2) → f136. The uniform
//!   MC2 import spends f136 on @0x8C, so `import_ent_mc2` carries a
//!   (5,27) home for it (@0x8C is dead 0 on the family) — without it
//!   every replayed pair re-read the power as 0 and the four a3=0
//!   RE-FIRES of each whip no-opped (one (9,9) arc per whip, not five).
//! - `word_0x5A_90` (particle/sprite row) → type86;
//!   `animationFrame_0x5C_92` → frame88.
//!
//! DELIBERATE APPROXIMATIONS (flagged in place too):
//! - m0 state 0x06 (`sub_1F2B0`) and its m3 twin state 0x1E
//!   (`sub_1FA40`) are compiled EMPTY STUBS in the shipped binary
//!   (files 0x43AB0/0x44240: push ebp/mov ebp,esp/pop ebp/ret —
//!   docs/AUDIT-STUBBED-ARMS-2026-07-26.md) — the no-op arms are
//!   FAITHFUL, not guesses. The trace §1/§8 "tether is dormant"
//!   claim was FALSE — see [`Gen::m0_dodge`]; m3's recovered
//!   states never call the tether and `sub_68BD0` arms model 0
//!   only, so m3 keeps no dodge.
//! - `struct_byte_0xc` group markers (m27 byte[2]/byte[3] bits, the
//!   m22 byte[2]|=0x20 sound split) are not modeled; the m27
//!   show/hide of segments (byte[0] bit 0) writes flags bit 0
//!   VERBATIM (the awake pass's hidden-skip and retail's 0x21 draw
//!   law read it) PLUS the port's 0x20 draw alias (the renderer's
//!   billboard skip — widening it to 0x21 globally would break the
//!   MC2 map-only house pose and the cave balloon), and the burrow
//!   ops carry the bit-3 targetable toggle (flags 0x08) — the
//!   billboard-suppress bit live_poses already honors.
//! - `byte_0x5D_93` (palette-shade byte of `sub_49D50`) is
//!   renderer-side and unmodeled; the particle-row recolor lands in
//!   type86 + the f78 spin only.
//! - m27 `sub_2A7F0`'s low-power path perturbs the branch LCG by
//!   the global `setting_30` counter — the post-increment turn
//!   (incremented beside `Turn++` in PlayerEvents, EF:37557;
//!   `MobCtx::mc2_turn` carries it). Modeled via
//!   [`Gen::mc2_rand_perturb`], like the pyramid's two pick rolls.
//!   (Level.cpp:340's "0x3D after load" is remc2's own debug
//!   reseed `//fix`, not retail law.)
//! - m27 `sub_2A940`'s `x_DWORD_E9BA8` freeze gate (writer
//!   untraced, likely pause/debug) reads as 0 — the normal path.
//! - The m27 emerge/teleport probe folds `sub_102D0(_, _, 4)` (the
//!   second capability mask) into the shared `mc2_path_blocked`
//!   (a3=1 arm) + roughness test, like the shared move core does.
//! - m22's castle-drain (0xB2/0xB3) reaches through the target
//!   player's `CastleEntityIndex_0x3A_58`; no MC2 level spawns a
//!   castle today, so the lookup returns None and the machine takes
//!   retail's own castle-less arm (LABEL_17 revert). The seam
//!   closes when MC2 castles land.

use super::behavior::BEHAVIOR;
use super::sprite_params::SPRITE_PARAMS;
use crate::engine::features::Gen;
use crate::mc1::mobs::{MobCtx, PLAYER_TARGET};

pub(crate) const M0_BASE: u8 = 0;
pub(crate) const M3_BASE: u8 = 24;
pub(crate) const M22_BASE: u8 = 176;
pub(crate) const M27_BASE: u8 = 216;
/// m0/m3 child follow state (`sub_1B6B0`).
pub(crate) const CHILD_STATE: u8 = 232; // 0xE8
/// m27 branch / tier-2 segment (body-driven, no self-dispatch).
pub(crate) const BRANCH_STATE: u8 = 233; // 0xE9
pub(crate) const TIER2_STATE: u8 = 234; // 0xEA

/// `str_D404C[5]` — the m27 per-branch spline parameters
/// (engine/Type_D404C.cpp, a static array compiled into the binary;
/// only the sim-read fields are carried — w8/w16/w18/w20 are
/// renderer-only). Order: w0 anchor reach, w2 anchor yaw, w4 anchor
/// z, w6 trailing reach, w10 trailing z, w12 splay yaw, w14 splay
/// pitch.
const D404C: [[i16; 7]; 5] = [
    [390, 20, 610, 30, -80, 9, 1771],
    [440, 110, 600, 0, -100, 407, 1685],
    [430, -100, 600, 0, -100, 1641, 1707],
    [420, 50, 450, 0, -70, 284, 1905],
    [420, -10, 450, 40, -70, 770, 1157],
];
const D404C_W0: usize = 0;
const D404C_W2: usize = 1;
const D404C_W4: usize = 2;
const D404C_W6: usize = 3;
const D404C_W10: usize = 4;
const D404C_W12: usize = 5;
const D404C_W14: usize = 6;

/// `xx_DWORD_D40BC[17][3]` (EF:1092) — the m27 spline arc profile;
/// only columns 0/1 are read (outer/inner pitch-bend magnitudes).
const D40BC: [[i16; 2]; 17] = [
    [0, 0],
    [106, 36],
    [151, 51],
    [191, 65],
    [220, 75],
    [246, 84],
    [275, 94],
    [297, 102],
    [318, 109],
    [338, 116],
    [361, 124],
    [380, 130],
    [398, 136],
    [416, 143],
    [437, 150],
    [454, 156],
    [0, 0],
];

/// `x_BYTE_D400C[8][8]` (EF:1080) — the m22 colorize ramp, indexed
/// `[tailLen>>1][|ring offset|]`. Row 5 is non-monotone in retail
/// (…4,3,3,1…) — reproduced verbatim.
const D400C: [[u8; 8]; 8] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 0, 0, 0, 0, 0, 0],
    [3, 2, 1, 0, 0, 0, 0, 0],
    [4, 3, 2, 1, 0, 0, 0, 0],
    [5, 4, 3, 3, 1, 0, 0, 0],
    [6, 5, 4, 3, 2, 1, 0, 0],
    [7, 6, 5, 4, 3, 2, 1, 0],
];

impl Gen {
    // ---- shared ------------------------------------------------------------

    /// `sub_58210_radix_tan` — the vertical bearing FROM a TO b
    /// (the MC1 segment-follow idiom, mc1/mobs.rs :21107 port).
    pub(crate) fn mc2_radix_tan(a: (u16, u16, i16), b: (u16, u16, i16)) -> u16 {
        let dh = Self::isqrt(Self::dist2_sq(a.0, a.1, b.0, b.1) as u32) as i16;
        Self::angle_of(a.2.wrapping_sub(b.2), dh.wrapping_neg())
    }

    /// A position read that tolerates corpses (retail dereferences
    /// the slot regardless); the human resolves through the ctx.
    fn mc2_raw_pos(&self, slot: u16, ctx: &MobCtx) -> Option<(u16, u16, i16)> {
        if slot == PLAYER_TARGET {
            return Some((ctx.px, ctx.py, ctx.pz));
        }
        let j = slot as usize;
        if j == 0 || j >= self.ent.len() {
            return None;
        }
        let e = &self.ent[j];
        Some((e.x, e.y, e.z))
    }

    /// `GetManaSphereColorIndexFromEntityId_369F0` (EF:26782): the
    /// owner's mana-sphere particle-row base — 52 wild, and
    /// `105 + 8·TransformPlayerColorIndex(team)` for ANY wizard
    /// (EF:26800; the human = team 0, rivals by slot; the sphere art
    /// families are authored in Transform order,
    /// crate::mc2::COLOR_ART).
    fn mc2_ball_color(&self, target: u16) -> u16 {
        if target == PLAYER_TARGET {
            return 105;
        }
        match self.rival_ents.iter().position(|&e| e != 0 && e == target) {
            Some(slot) => 105 + 8 * crate::mc2::color_art(slot as u8) as u16,
            None => 52,
        }
    }

    /// `sub_49D50` (EF:32847): the "color index" is a
    /// `particlesParameters_D951C` row — sprite row + spin (the
    /// palette-shade byte_0x5D_93 is renderer-side, unmodeled).
    fn mc2_particle_row(&mut self, i: usize, row: u16) {
        let r = row as usize % SPRITE_PARAMS.len();
        let e = &mut self.ent[i];
        e.type86 = r as u16;
        e.f78 = SPRITE_PARAMS[r].rot_speed_8 / 2;
    }

    // =========================================================================
    // MODELS 0 + 3 — worm/hydra + multipart flyer
    // (ctors sub_4B240 EF:33642 / sub_4B6F0 EF:33797; child tick
    // sub_1B6B0 EF:8696; head states = thin primitive wrappers,
    // docs/traces/mc2-m0-m3-gaps.md §5)
    // =========================================================================

    /// `sub_4B240` — model 0. ONE ctor RNG draw (facing); the child
    /// loop draws nothing (children byte-copy the head, inheriting
    /// its LCG). Bug-compatible: the per-child mana>>5 write lands
    /// on the HEAD (EF:33703) — children keep the copied 2250 each.
    pub(crate) fn mc2_spawn_m0(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        if self.free.len() < 16 {
            return None; // sub_4A810 free-slot gate (EF:33655)
        }
        let head = self.new_event()?;
        {
            let e = &mut self.ent[head];
            e.class64 = 5;
            e.model65 = 0;
            e.tick70 = M0_BASE + 1;
            e.f28 = 1; // byte_0x38_56 = 1 — cross-column damage contract
            e.f128 = 80;
            e.f130 = 16;
            e.f126 = 30;
            e.max_life = 4000;
            e.f136 = 4500;
            e.f140 = 2250; // mana = 4500, maxMana = mana, mana /= 2
        }
        self.mc2_ctor_facing(head);
        let ord = self.mc2_ord(0);
        {
            let e = &mut self.ent[head];
            e.f36 = 0;
            e.f56 = 96; // word_0x36_54 — the link length
            e.f26 = (head % 100) as i16; // dword_0x10_16 — the bob seed
            e.f63 = ord;
            e.f66 = 3; // xtype
            e.f44 = 0;
            e.f71 = 0;
            e.row156 = 71;
        }
        self.ent[head].f58 = Self::mc2_wake_stagger(71, ord);
        self.mc2_spawn_chain_children(head, x, y, z, false);
        self.link(head, x, y, z);
        self.refill_life(head);
        self.mc2_set_sprite(head, 40);
        Some(head)
    }

    /// `sub_4B6F0` — model 3. ONE ctor RNG draw. NO free-slot gate
    /// (relies on NewEvent null-checks). Children carry their own
    /// mana (maxMana/32) and particle-driven link metrics.
    pub(crate) fn mc2_spawn_m3(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let head = self.new_event()?;
        {
            let e = &mut self.ent[head];
            e.class64 = 5;
            e.model65 = 3;
            e.tick70 = M3_BASE + 1;
            e.f28 = 1;
            e.f128 = 64;
            e.f130 = 16;
            e.f126 = 30;
            e.max_life = 9000;
        }
        self.mc2_set_mana_half(head); // SetEvent144: 4500
        {
            let e = &mut self.ent[head];
            e.f136 = e.f140; // maxMana = mana
            e.f140 /= 2; // 2250
        }
        self.mc2_ctor_facing(head);
        let ord = self.mc2_ord(3);
        {
            let e = &mut self.ent[head];
            e.f36 = 0;
            e.f56 = 96;
            e.f26 = (head % 100) as i16;
            e.f63 = ord;
            e.f66 = 3;
            e.row156 = 74;
        }
        self.ent[head].f58 = Self::mc2_wake_stagger(74, ord);
        self.mc2_spawn_chain_children(head, x, y, z, true);
        self.link(head, x, y, z);
        self.refill_life(head);
        self.mc2_set_sprite(head, 88);
        // Head segment metrics from the particle table (EF:33869-71) —
        // the SAME `particlesParameters_D951C` rows the child loop
        // reads two lines above, so the DERIVED pair, not the shipped
        // static row. The static `speed_6` column is zero almost
        // everywhere (it is filled at load from the sprite bitmap's
        // aspect), and reading it here collapsed the head's pitch/roll
        // box to 0.
        let (ps6, pr8) = self.mc2_params_ext(88);
        let shift = (60 * ps6 / 100, 60 * pr8 / 100);
        self.mc2_shift_rot(head, shift.0, shift.1);
        Some(head)
    }

    /// The shared 16-child spawn loop (EF:33691-33712 m0 /
    /// EF:33836-33865 m3): byte-copy of the head, chain links,
    /// state 0xE8, per-model sprite rows and link lengths. The
    /// free-slot gate (m0) makes the NewEvent null arm unreachable;
    /// m3 without a gate stops early like retail's skip.
    fn mc2_spawn_chain_children(&mut self, head: usize, x: u16, y: u16, z: i16, m3: bool) {
        let mut prev = head;
        for ci in 0..16u16 {
            let Some(seg) = self.new_event() else { break };
            // qmemcpy(child, head, 0xA8) — the child KEEPS the
            // head's id_0x1A_26 (owner immunity spans the chain);
            // only the chain links and identity below are rewritten.
            self.ent[seg] = self.ent[head];
            self.ent[seg].flags &= !4; // not yet map-linked
            self.ent[seg].thing_slot = 0;
            self.ent[seg].f52 = prev as u16;
            self.ent[prev].f54 = seg as u16;
            self.ent[seg].f54 = 0;
            self.ent[seg].tick70 = CHILD_STATE;
            if m3 {
                self.ent[seg].f140 = self.ent[head].f136 / 32;
            } else {
                // The m0 decompile-literal quirk: the write lands on
                // the HEAD (EF:33703).
                self.ent[head].f140 = self.ent[head].f136 / 32;
            }
            self.ent[seg].f63 = ci as u8;
            if m3 {
                self.mc2_set_sprite(seg, 89 + ci);
                // Per-child particle metrics override the /2 quad
                // (EF:33846-51): 65% of the row's raw values (the
                // DERIVED pair — retail computes speed_6 from the
                // sprite bitmap at load, EF:44870-44910).
                let (ps6, pr8) = self.mc2_params_ext((89 + ci) as usize);
                let (sh, fov) = (65 * ps6 / 100, 65 * pr8 / 100);
                self.mc2_shift_rot(seg, sh, fov);
                self.ent[seg].f56 = if ci == 0 { 125 * sh / 100 } else { sh };
            } else {
                self.mc2_set_sprite(seg, 19 + ci);
                self.ent[seg].f56 = self.ent[seg].f80; // word_0x36_54 = array.pitch
            }
            // The table's zero speed_6 is never the runtime value:
            // retail DERIVES it at load from the sprite bitmap's
            // aspect (EF:44870-44910, speed_6 = w·rotSpeed/h), which
            // the dims-fed assets reproduce. The 96 floor stays only
            // for dims-less callers (unit fixtures) where the
            // derivation can't run.
            if self.ent[seg].f56 == 0 {
                self.ent[seg].f56 = 96;
            }
            self.link(seg, x, y, z);
            self.refill_life(seg);
            prev = seg;
        }
    }

    /// `sub_1B6B0` (EF:8696) — the m0/m3 child tick (state 0xE8):
    /// awake = rigid follow at -f56 behind the parent along the
    /// exact 3D bearing + own damage intake; asleep = every 4th
    /// phase snap onto the parent. Parent gone/not-a-creature →
    /// orphan reap (the MC1 worm port's precedent for
    /// DisableEntityDrawing04).
    pub(crate) fn mc2_child_tick(&mut self, i: usize) {
        let l = self.ent[i].f52 as usize;
        if l == 0 || self.ent[l].class64 != 5 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let (lx, ly, lz) = (self.ent[l].x, self.ent[l].y, self.ent[l].z);
        if self.ent[i].f58 != 0 {
            let e = &self.ent[i];
            let yaw = Self::angle_between(e.x, e.y, lx, ly);
            let pitch = Self::mc2_radix_tan((e.x, e.y, e.z), (lx, ly, lz));
            self.ent[i].f30 = yaw;
            self.ent[i].f32 = pitch;
            let mut pred = (lx, ly, lz);
            let d = self.ent[i].f56 as i16;
            Self::polar_step(&mut pred, yaw, pitch, -d);
            self.move_relink(i, pred.0, pred.1, pred.2);
            // Damage intake AFTER the follow (EF:8710-19); the
            // attacker latch is word_0x26_38 (f40), else zero.
            if self.ent[i].mail[0].1 != 0 {
                let (amt, src) = self.ent[i].mail[0];
                self.ent[i].mail[0].1 = 0;
                self.ent[i].f40 = src;
                self.ent[i].act_life -= amt as i32;
            } else {
                self.ent[i].f40 = 0;
            }
        } else if self.ent[i].f63 & 3 == 0 {
            self.move_relink(i, lx, ly, lz);
            self.ent[i].f30 = self.ent[l].f30;
        }
    }

    /// `sub_1F040` (EF:11233) — the m0 vertical bob: velocity in
    /// f26 (`dword_0x10_16`), gravity −5/tick, floor bounce +150 at
    /// terrain+256; on caves, ceiling BOUNCE −150 above ceiling−256
    /// (EF:11244-48) — open levels have no upper clamp, exactly
    /// retail. Also the stage-HELD dragon's ambient physics
    /// (`sub_1F300` phase-7 wrapper, kinds 1-10 — the stagevars
    /// held seam).
    pub(crate) fn m0_bob(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let z = self.ent[i].z.wrapping_add(self.ent[i].f26);
        self.move_relink(i, x, y, z);
        let ground = self.ground_z(x, y) as i16;
        self.ent[i].f26 -= 5;
        if z < ground.wrapping_add(256) {
            self.ent[i].f26 = 150;
        } else if self.is_cave() && z as i32 > self.ceiling_z(x, y) - 256 {
            self.ent[i].f26 = -150;
        }
    }

    /// `sub_1F0C0` (EF:11259) — the m0 incoming-projectile DODGE.
    /// Armed on the PROJECTILE side: the class-9 one-shot
    /// acquisition (`sub_67CB0`) calls `sub_68BD0` (EF:55453),
    /// which sets the head's alert window `fontTypeIndex_0x3D_61 =
    /// 32` whenever the lock lands on a class-5 model-0 victim
    /// (EF:54848 — the only live call site; the trace-bank
    /// "gate never armed → dormant" claim was wrong). While the
    /// window runs (decrementing every call, EF:11277-80): with no
    /// hook, spiral the radius-4 tile disc for a class-9 whose
    /// homing target is this head and hook it, timer 5
    /// (EF:11310-42); with a hook live, strafe the HEAD
    /// perpendicular to the projectile's CURRENT heading — side by
    /// hooked-index parity, step `48·timer` (240..48, ≈720 units
    /// total), pitch 0, tile-relinked (EF:11293-11300) — and
    /// release when the timer expires or the projectile dies
    /// (EF:11286-89/11304-06). If the window closes mid-dodge the
    /// hook freezes in place until a fresh arm — retail's own
    /// residue law.
    pub(crate) fn m0_dodge(&mut self, i: usize) {
        let gate = self.ent[i].f46;
        if gate == 0 {
            return;
        }
        self.ent[i].f46 = gate - 1;
        if self.ent[i].f71 != 0 {
            if self.ent[i].f44 == 0 {
                self.ent[i].f71 = 0;
                self.ent[i].f38 = 0;
                return;
            }
            // Validity = retail's `v7x <= Entities[0]` gone-check
            // (EF:11286-89) PLUS an out-of-pool guard: the hook word
            // word_0x24_36 doubles as the kill-credit latch
            // (`mc2_state_head` death write, :350 — retail's own
            // field reuse), and a death tick still reaches this
            // branch. Retail then reads the KILLER's pool entity
            // (benign — its player is in-pool); our out-of-pool
            // human is the PLAYER_TARGET sentinel, which must read
            // as "gone" → release, not an index.
            let p = self.ent[i].f38 as usize;
            if p == 0
                || p >= self.ent.len()
                || self.ent[p].act_life < 0
                || self.ent[p].flags & 0x400 != 0
            {
                self.ent[i].f71 = 0;
                self.ent[i].f38 = 0;
                return;
            }
            let yaw = if self.ent[i].f38 & 1 != 0 {
                self.ent[p].f30.wrapping_add(512)
            } else {
                self.ent[p].f30.wrapping_sub(512)
            } & 0x7FF;
            let mut pos = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            Self::polar_step(&mut pos, yaw, 0, (48 * self.ent[i].f44) as i16);
            self.move_relink(i, pos.0, pos.1, pos.2);
            self.ent[i].f44 -= 1;
        } else {
            let cx = (self.ent[i].x.wrapping_add(128) >> 8) as u8;
            let cy = (self.ent[i].y.wrapping_add(128) >> 8) as u8;
            let my_id = self.ent[i].id24;
            let mut hooked = 0usize;
            'scan: for (dx, dy) in self.ring_cells(0, 4) {
                let t = crate::engine::features::tile(cx.wrapping_add(dx), cy.wrapping_add(dy));
                let mut j = self.map_entity[t] as usize;
                while j != 0 {
                    if self.ent[j].class64 == 9 && self.ent[j].f146 == my_id {
                        hooked = j;
                        break 'scan;
                    }
                    j = self.ent[j].next20 as usize;
                }
            }
            if hooked != 0 {
                self.ent[i].f44 = 5;
                self.ent[i].f71 = self.ent[i].f71.wrapping_add(1);
                self.ent[i].f38 = hooked as u16;
            }
        }
    }

    /// m0 states 0x00-0x07 (docs/traces/mc2-m0-m3-gaps.md §5):
    /// primitive → dodge (`sub_1F0C0`) → bob (`sub_1F040`) in
    /// 0x01/0x02/0x03, per sub_1EF40/1EF70/1EFD0.
    pub(crate) fn m0_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M0_BASE {
            0 => self.mc2_patrol(i, M0_BASE),
            1 => {
                self.mc2_idle(i, M0_BASE, ctx);
                self.m0_dodge(i);
                self.m0_bob(i);
            }
            2 => {
                if self.mc2_chase_attack(i, M0_BASE, ctx, Self::mc2_atk_bolt) {
                    self.snd(8, i);
                }
                self.m0_dodge(i);
                self.m0_bob(i);
            }
            3 => {
                self.mc2_pack(i, M0_BASE);
                self.m0_dodge(i);
                self.m0_bob(i);
            }
            4 => self.mc2_prekill(i, M0_BASE),
            5 => self.mc2_kill(i),
            // 0x06 = sub_1F2B0: a compiled EMPTY STUB in the binary
            // (module doc) — the no-op is faithful.
            6 => {}
            _ => {
                // 0x07 sub_1F300: 1D5D0 no-op for StageVar2==0, and
                // 0 is outside the tether/bob case list → nothing.
            }
        }
    }

    /// m3 states 0x18-0x1F — pure primitive wrappers, aggro base 24
    /// (no tether/bob among the recovered states; trace §5).
    pub(crate) fn m3_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M3_BASE {
            0 => self.mc2_patrol(i, M3_BASE),
            1 => self.mc2_idle(i, M3_BASE, ctx),
            2 => {
                if self.mc2_chase_attack(i, M3_BASE, ctx, Self::mc2_atk_bolt) {
                    self.snd(8, i);
                }
            }
            3 => self.mc2_pack(i, M3_BASE),
            4 => self.mc2_prekill(i, M3_BASE),
            5 => self.mc2_kill(i),
            // 0x1E = sub_1FA40: a compiled EMPTY STUB in the binary
            // (module doc) — the no-op is faithful.
            6 => {}
            _ => {} // 0x1F sub_1FA50: 1D5D0 no-op for StageVar2==0
        }
    }

    // =========================================================================
    // MODEL 22 — segmented worm / castle-mana thief
    // (ctor sub_4CA00 EF:34377 + map tail sub_4CB60 EF:34420;
    // helpers docs/traces/mc2-m22-worm-helpers.md)
    // =========================================================================

    /// `sub_4CA00` + the map-placement arm (`sub_4A310` EF:33025-28:
    /// tail length = par1 & 0xFF, then `sub_4CB60` spawns the tail).
    /// ONE ctor RNG draw. Head z = terrain + 384.
    pub(crate) fn mc2_spawn_m22(&mut self, x: u16, y: u16, _z: i16, par1: u16) -> Option<usize> {
        if self.free.len() < 15 {
            return None; // free-slot gate (EF:34380)
        }
        let head = self.new_event()?;
        {
            let e = &mut self.ent[head];
            e.class64 = 5;
            e.model65 = 22;
            e.tick70 = M22_BASE; // 176
            // byte_0x38_56 = 3 (EF:34400) — bit 1 ADMITS the ch1
            // designation mail (the mc1/combat.rs:180 gate is
            // faithful). Needs f28=3 not 1: f28=1 drops every tag,
            // deadening the whole retarget→colorize machine.
            e.f28 = 3;
            e.f128 = 128;
            e.f130 = 16;
            e.f126 = 16;
        }
        self.mc2_ctor_facing(head);
        let ord = self.mc2_ord(22);
        {
            let e = &mut self.ent[head];
            e.max_life = 2000;
            e.f36 = 0;
            e.row156 = 90;
            e.f63 = ord;
            e.f66 = 3;
            e.dest_x = 0; // playerEntityIndex_0x94_148
            e.f78 = 0; // array.yaw
            e.frame88 = 0;
            e.f44 = 11; // word_0x2C_44 — spin rate
            e.f46 = 0; // subSpellIndex — spiral angle
            e.f56 = 0; // word_0x36_54 — writhe phase bits
            e.f146 = 1024; // word_0x96_150 — the grow timer
            e.f38 = 0; // word_0x24_36 — rise budget
            e.f71 = 15; // byte_0x46_70 — default tail length
        }
        self.ent[head].f58 = Self::mc2_wake_stagger(90, ord);
        let z = (self.ground_z(x, y) as i16).wrapping_add(384);
        self.link(head, x, y, z);
        self.mc2_set_mana_half(head); // SetEvent144: 1000
        self.refill_life(head);
        // Map placement overrides the tail length then grows it.
        self.ent[head].f71 = (par1 & 0xFF) as u8;
        self.mc2_m22_spawn_tail(head);
        Some(head)
    }

    /// `sub_4CB60` (EF:34420): (tailLen/2) rings x 2 segments with
    /// signed offsets +1,-1,+2,-2,…; then colorize + shift-rot +
    /// one follow pass to seat everyone.
    fn mc2_m22_spawn_tail(&mut self, head: usize) {
        let rings = (self.ent[head].f71 / 2) as i16;
        let mut prev = head;
        for ring in 1..=rings {
            for side in 0..2 {
                let off = if side == 1 { -ring } else { ring };
                if let Some(seg) = self.mc2_m22_add_segment(head, prev, off as i8) {
                    prev = seg;
                }
            }
        }
        self.mc2_m22_colorize(head);
        self.mc2_m22_shift_rot(head);
        // sub_276E0: one spiral-follow pass over the chain.
        let mut j = self.ent[head].f54 as usize;
        while j != 0 {
            self.m22_tail_follow(j);
            j = self.ent[j].f54 as usize;
        }
    }

    /// `sub_274C0` (EF:17845) — the m22 segment-spawn primitive:
    /// full struct copy from the PREVIOUS link, chain re-link,
    /// state 0xB4, signed ring offset in f71, head ref in f146.
    fn mc2_m22_add_segment(&mut self, head: usize, prev: usize, off: i8) -> Option<usize> {
        let seg = self.new_event()?;
        self.ent[seg] = self.ent[prev];
        self.ent[seg].thing_slot = 0;
        self.ent[seg].f52 = prev as u16;
        self.ent[prev].f54 = seg as u16;
        self.ent[seg].f54 = 0;
        self.ent[seg].f63 = (off.unsigned_abs()) & 1; // parity seed
        self.ent[seg].flags &= !4;
        self.ent[seg].f71 = off as u8; // SIGNED ring offset
        self.ent[seg].tick70 = M22_BASE + 4; // 0xB4
        self.ent[seg].f44 = 0;
        self.ent[seg].dest_x = 0;
        self.ent[seg].f140 = 0;
        self.ent[seg].f146 = head as u16; // word_0x96_150 = the worm head
        let (hx, hy, hz) = {
            let h = &self.ent[head];
            (h.x, h.y, h.z)
        };
        self.link(seg, hx, hy, hz);
        self.refill_life(seg);
        Some(seg)
    }

    /// `sub_278F0` (EF:18036): particle row = base + the D400C ramp.
    fn m22_color_idx(base: u16, tail_len: u8, off: i8) -> u16 {
        let row = (tail_len >> 1).min(7) as usize;
        let col = (off.unsigned_abs()).min(7) as usize;
        base + D400C[row][col] as u16
    }

    /// `sub_27590` (EF:17867): recolor head + chain to the owner's
    /// mana-sphere palette.
    fn mc2_m22_colorize(&mut self, head: usize) {
        let base = self.mc2_ball_color(self.ent[head].dest_x);
        let len = self.ent[head].f71;
        let hr = Self::m22_color_idx(base, len, 0);
        self.mc2_particle_row(head, hr);
        let mut j = self.ent[head].f54 as usize;
        while j != 0 {
            let r = Self::m22_color_idx(base, len, self.ent[j].f71 as i8);
            self.mc2_particle_row(j, r);
            j = self.ent[j].f54 as usize;
        }
    }

    /// `sub_27610` (EF:17893): per-link spacing/coil radius =
    /// 550 * the colorize row's rotSpeed / 1000.
    fn mc2_m22_shift_rot(&mut self, head: usize) {
        let base = self.mc2_ball_color(self.ent[head].dest_x);
        let len = self.ent[head].f71;
        let hrow = Self::m22_color_idx(base, len, 0) as usize % SPRITE_PARAMS.len();
        let v = 550 * SPRITE_PARAMS[hrow].rot_speed_8 as u32;
        self.mc2_shift_rot(head, (v / 1000) as u16, (v / 1000) as u16);
        let mut j = self.ent[head].f54 as usize;
        while j != 0 {
            let row = Self::m22_color_idx(base, len, self.ent[j].f71 as i8) as usize
                % SPRITE_PARAMS.len();
            let v = 550 * SPRITE_PARAMS[row].rot_speed_8 as u32;
            self.mc2_shift_rot(j, (v / 1000) as u16, (v / 1000) as u16);
            j = self.ent[j].f54 as usize;
        }
    }

    /// `sub_273C0` (EF:17780): the spiral angle a segment orbits the
    /// head at — magnitude grows with |offset| and the writhe frame,
    /// side/chirality from the offset sign and phase bit 1.
    fn m22_spiral(frame: i16, phase: u8, off: i16, tail_len: i16) -> u16 {
        let v4 = off.unsigned_abs() as i32;
        let result = (((15 - tail_len) as i32 * v4 + v4 * frame as i32) & 0x7FF) as u16;
        let v6 = if off >= 0 {
            if phase & 2 != 0 {
                return result;
            }
            2048u16.wrapping_sub(result)
        } else if phase & 2 != 0 {
            1024u16.wrapping_sub(result)
        } else {
            result.wrapping_add(1024)
        };
        v6 & 0x7FF
    }

    /// `sub_271D0` (EF:17685): the m22 spiral follow — positioned
    /// two links up, at the computed orbit angle, with pitch-based
    /// z offset.
    fn m22_tail_follow(&mut self, i: usize) {
        let head = self.ent[i].f146 as usize;
        if head == 0 || head >= self.ent.len() {
            return;
        }
        let v4 = {
            let h = &self.ent[head];
            let spiral = Self::m22_spiral(
                h.frame88 as i16,
                h.f56 as u8,
                self.ent[i].f71 as i8 as i16,
                h.f71 as i16,
            );
            (h.f46 as u16).wrapping_add(spiral) & 0x7FF
        };
        self.ent[i].f44 = v4 as i16 as u16;
        let mut anchor = self.ent[i].f52 as usize;
        if anchor != 0 && self.ent[anchor].f52 != 0 {
            anchor = self.ent[anchor].f52 as usize;
        }
        if anchor == 0 {
            return;
        }
        let (ax, ay, az, ap) = {
            let a = &self.ent[anchor];
            (a.x, a.y, a.z, a.f80 as i16)
        };
        let sp = self.ent[i].f80 as i16;
        let mut pred = (ax, ay, az);
        Self::polar_step(&mut pred, v4, 0, sp.wrapping_add(ap));
        pred.2 = ap.wrapping_sub(sp).wrapping_add(az);
        self.move_relink(i, pred.0, pred.1, pred.2);
    }

    /// `sub_26D20` (EF:17447): segment→head hit/aggro relay. Damage
    /// amounts are consumed for STEER only (the m22 head is
    /// damage-immune through its own suite — trace §12, retail
    /// check banked); the ch1 tag retargets the head.
    fn m22_relay(&mut self, i: usize, ctx: &MobCtx) {
        if self.ent[i].f58 == 0 {
            return;
        }
        let head = self.ent[i].f146 as usize;
        if head == 0 || head >= self.ent.len() {
            return;
        }
        let ha = self.ent[head].tick70;
        if !(ha == M22_BASE || ha == M22_BASE + 2) {
            return; // relay only acts in 0xB0 / 0xB2 (EF:17460)
        }
        if self.ent[i].mail[0].1 != 0 {
            let src = self.ent[i].mail[0].1;
            let (mn, mx) = (self.ent[head].f128, self.ent[head].f130);
            self.ent[head].f126 = ((mn - mx) >> 2) + mx;
            if let Some((ax, ay, _)) = self.mc2_raw_pos(src, ctx) {
                // Surge AWAY: yaw = tan2(attacker → hit SEGMENT)
                // (EF:17472-74) — anchored at the SEGMENT, not the
                // head, and away from the attacker.
                let (sx, sy) = (self.ent[i].x, self.ent[i].y);
                let yaw = Self::angle_between(ax, ay, sx, sy);
                self.ent[head].f30 = yaw;
                self.ent[head].f34 = yaw;
            }
            // Spin law (EF:17475-94): ADDITIVE and orbit-signed —
            // v4 = 56·|seg pos|/(len/2) unclamped, negated when the
            // segment sits on the far half of the ring (head yaw vs
            // the segment's orbit angle f44), ADDED to the head's
            // spin, and the SUM clamps min ±11 / max ±227.
            let so = (self.ent[i].f71 as i8).unsigned_abs() as i32;
            let half = (self.ent[head].f71 >> 1).max(1) as i32;
            let mut v4 = (56 * so / half) as i16;
            let orbit = self.ent[i].f44;
            if self.ent[head].f30.wrapping_sub(orbit) & 0x7FF >= 1024 {
                v4 = -v4;
            }
            let mut v5 = v4 + self.ent[head].f44 as i16;
            if v5.abs() < 11 {
                v5 = if v5 <= 0 { -11 } else { 11 };
            }
            if v5.abs() > 227 {
                v5 = if v5 <= 0 { -227 } else { 227 };
            }
            self.ent[head].f44 = v5 as u16;
            // Clear the hit source on EVERY segment (EF:17520).
            let mut j = self.ent[head].f54 as usize;
            while j != 0 {
                self.ent[j].mail[0].1 = 0;
                j = self.ent[j].f54 as usize;
            }
        }
        let tag = self.ent[i].mail[1].1;
        if tag != 0 {
            if tag != self.ent[head].dest_x {
                self.ent[head].dest_x = tag;
                self.ent[head].tick70 = M22_BASE + 1; // 177
                self.ent[head].f26 = ((self.ent[i].f71 as i8 as i16) << 8) as i16;
                if tag == PLAYER_TARGET {
                    self.snd_player(4);
                } else {
                    self.snd(4, tag as usize);
                }
            }
            let mut j = self.ent[head].f54 as usize;
            while j != 0 {
                self.ent[j].mail[1].1 = 0;
                j = self.ent[j].f54 as usize;
            }
        }
    }

    /// `sub_26CC0` (EF:17427): the chain-kill — one pass, every
    /// downstream segment then the head converts to mana spheres.
    fn m22_chain_kill(&mut self, i: usize) {
        let mut j = self.ent[i].f54 as usize;
        while j != 0 {
            let next = self.ent[j].f54 as usize;
            self.mc2_mana_spheres(j, false);
            self.ent[j].flags |= 0x400;
            j = next;
        }
        self.mc2_mana_spheres(i, false);
        self.ent[i].flags |= 0x400;
    }

    /// `sub_27120` (EF:17655): anti-stack z-push vs OTHER worms
    /// (id-keyed; own segments share the head's id and are skipped;
    /// the bucket holds heads only — 0xB4 is list-excluded).
    fn m22_antistack(&mut self, i: usize) {
        let (ex, ey, ez, id, vwin, hwin) = {
            let e = &self.ent[i];
            (
                e.x,
                e.y,
                e.z,
                e.id24,
                2 * e.f84 as i32 + 32,
                2 * e.f80 as i32,
            )
        };
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 5
                || c.model65 != 22
                || c.id24 == id
                || matches!(c.tick70, 0xB4 | 0xE8 | 0xEA)
                || c.flags & 0x400 != 0
            {
                continue;
            }
            let dx = ((ex.wrapping_sub(c.x)) as i16 as i32).abs();
            let dy = ((ey.wrapping_sub(c.y)) as i16 as i32).abs();
            let dz = ((ez as i32) - (c.z as i32)).abs();
            if dx < hwin && dy < hwin && dz < vwin && ez >= c.z {
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                let z = self.ent[i].z.wrapping_add(64);
                self.move_relink(i, x, y, z);
            }
        }
    }

    /// `sub_26FF0` (EF:17589): head move + altitude — actSpeed
    /// decay, the move core bracketed by a tail-length shift, the
    /// every-16th anti-stack, and the whole-chain ceiling clamp
    /// with the f38 rise budget.
    fn m22_move(&mut self, i: usize) {
        if self.ent[i].f126 > self.ent[i].f130 {
            self.ent[i].f126 -= 2;
        }
        let (save_p, save_f) = (self.ent[i].f80, self.ent[i].f84);
        let shift = (self.ent[i].f71 as u16) << 8;
        self.mc2_shift_rot(i, shift, save_f);
        self.mc2_move_core(i);
        if self.ent[i].f63 & 0xF == 0 {
            self.m22_antistack(i);
        }
        self.mc2_shift_rot(i, save_p, save_f);
        // Whole-chain highest terrain altitude (+ the position it
        // occurred at, for the rise-rate roughness test).
        let mut best: i16 = i16::MIN;
        let mut best_pos = (self.ent[i].x, self.ent[i].y);
        let mut j = i;
        loop {
            let (cx, cy) = (self.ent[j].x, self.ent[j].y);
            let g = self.ground_z(cx, cy) as i16;
            if g > best {
                best = g;
                best_pos = (cx, cy);
            }
            j = self.ent[j].f54 as usize;
            if j == 0 {
                break;
            }
        }
        let ceiling = best.wrapping_add(384);
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        if z >= ceiling {
            if self.ent[i].f38 != 0 {
                self.ent[i].f38 -= 1; // burn the rise budget, hold z
            } else {
                self.move_relink(i, x, y, z.wrapping_sub(2));
            }
        } else {
            let steep = self.roughness(best_pos.0, best_pos.1)
                > BEHAVIOR[self.ent[i].row156 as usize].v_16 as i32;
            let dz = if steep { 0x100 } else { 0x40 };
            self.move_relink(i, x, y, z.wrapping_add(dz));
            self.ent[i].f38 = 0x40;
        }
    }

    /// `sub_27430` (EF:17806): the writhe frame-step band.
    fn m22_anim_step(frame: u8) -> u8 {
        if frame >= 96 {
            2
        } else if frame >= 87 {
            3
        } else if frame >= 60 {
            4
        } else if frame < 30 {
            6
        } else {
            5
        }
    }

    /// `sub_272C0` (EF:17720): writhe animation (tailLen >= 11
    /// only; SOUND 48 in frames (0,16)), serpentine spin advance,
    /// spin-rate decay toward ±11 every 4th phase.
    fn m22_anim(&mut self, i: usize) {
        if self.ent[i].f71 >= 11 {
            let step = Self::m22_anim_step(self.ent[i].frame88);
            let frame = self.ent[i].frame88;
            if frame != 0 && frame < 0x10 {
                self.snd(48, i);
            }
            if self.ent[i].f56 & 1 != 0 {
                let v3 = frame as u16 + step as u16;
                if v3 > 0x64 {
                    self.ent[i].frame88 = 100;
                    self.ent[i].f56 &= 0xFE; // start counting down
                } else {
                    self.ent[i].frame88 = v3 as u8;
                }
            } else if frame > step {
                self.ent[i].frame88 = frame - step;
            } else {
                let v5 = self.ent[i].f56 | 1;
                self.ent[i].frame88 = 0;
                self.ent[i].f56 = v5 ^ 2; // chirality flip each bounce
            }
        }
        let spin = self.ent[i].f44 as i16;
        self.ent[i].f46 = (self.ent[i].f46.wrapping_add(spin)) & 0x7FF;
        if self.ent[i].f63 & 3 == 0 {
            let spin = self.ent[i].f44 as i16;
            let mag = (spin.abs() - 5).max(11);
            self.ent[i].f44 = (if spin <= 0 { -mag } else { mag }) as u16;
        }
    }

    /// `sub_26F10` (EF:17542): head damage-turn (accelerate by
    /// dmg/4, turn AWAY from the attacker) + ch1 retarget + the
    /// life<0 → chain-kill transition. The head's own life NEVER
    /// drops here — melee only enrages.
    fn m22_dmg(&mut self, i: usize, ctx: &MobCtx) {
        if self.ent[i].f58 != 0 {
            if self.ent[i].mail[0].1 != 0 {
                let (amt, src) = self.ent[i].mail[0];
                let mut v = ((amt >> 2) as u16 as i16).wrapping_add(self.ent[i].f126);
                if v < self.ent[i].f130 {
                    v = self.ent[i].f130;
                }
                if v > self.ent[i].f128 {
                    v = self.ent[i].f128;
                }
                self.ent[i].f126 = v;
                self.ent[i].mail[0].1 = 0;
                if let Some((ax, ay, _)) = self.mc2_raw_pos(src, ctx) {
                    // tan2(attacker → self) = turn AWAY (EF:17568).
                    let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                    let yaw = Self::angle_between(ax, ay, ex, ey);
                    self.ent[i].f30 = yaw;
                    self.ent[i].f34 = yaw;
                }
            }
            let tag = self.ent[i].mail[1].1;
            if tag != 0 {
                if tag != self.ent[i].dest_x {
                    self.ent[i].dest_x = tag;
                    self.ent[i].tick70 = M22_BASE + 1;
                    self.ent[i].f26 = 0;
                    if tag == PLAYER_TARGET {
                        self.snd_player(4);
                    } else {
                        self.snd(4, tag as usize);
                    }
                }
                self.ent[i].mail[1].1 = 0;
            }
        }
        if self.ent[i].act_life < 0 {
            self.ent[i].tick70 = M22_BASE + 5; // 0xB5 chain-kill
        }
    }

    /// `sub_27880` (EF:18012): the 1024-tick grow cycle — tail +2
    /// (to <=15) and mana +1000 (cap 50000).
    fn m22_grow(&mut self, i: usize) {
        if self.ent[i].f146 != 0 {
            self.ent[i].f146 -= 1;
            return;
        }
        self.ent[i].f146 = 1024;
        let len = self.ent[i].f71;
        if len <= 13 {
            self.m22_resize(i, len + 2);
        }
        if self.ent[i].f140 < 50000 {
            self.ent[i].f140 += 1000;
        }
    }

    /// `sub_27720` (EF:17938): grow/shrink the tail to an odd
    /// target length — segment PAIRS (+n,−n) appended at the tail
    /// or hidden from it, then recolor + re-spacing.
    fn m22_resize(&mut self, head: usize, target: u8) {
        let target = target | 1;
        let cur = self.ent[head].f71;
        if !(1..=15).contains(&target) || cur == target {
            return;
        }
        // Walk to the tail end.
        let mut last = head;
        while self.ent[last].f54 != 0 {
            last = self.ent[last].f54 as usize;
        }
        let mut failed = false;
        if cur >= target {
            // SHRINK: remove ((cur-target)/2) ring pairs from the end.
            let mut removed = 0i16;
            while removed < ((cur - target) / 2) as i16 {
                let minus = self.ent[last].f52 as usize; // -offset twin
                if minus == 0 {
                    break;
                }
                let anchor = self.ent[minus].f52 as usize;
                self.ent[anchor].f54 = 0;
                self.ent[minus].flags |= 0x400;
                self.ent[last].flags |= 0x400;
                removed += 1;
                last = anchor;
            }
        } else {
            // GROW: one (+n, −n) ring; both slots allocated before
            // the copies, like retail's two NewEvents (alloc order
            // is stream-visible).
            let n = (self.ent[last].f71 as i8).unsigned_abs() as i8 + 1;
            if let Some(plus) = self.new_event() {
                if let Some(minus) = self.new_event() {
                    self.mc2_m22_seed_segment(head, last, plus, n);
                    self.mc2_m22_seed_segment(head, plus, minus, -n);
                } else {
                    self.ent[plus].flags |= 0x400; // rollback (EF:17982)
                    failed = true;
                }
            } else {
                failed = true;
            }
        }
        if !failed {
            self.ent[head].f71 = target;
            self.mc2_m22_colorize(head);
            self.mc2_m22_shift_rot(head);
        }
    }

    /// `sub_274C0` with a pre-allocated slot (the resize path).
    fn mc2_m22_seed_segment(&mut self, head: usize, prev: usize, seg: usize, off: i8) {
        self.ent[seg] = self.ent[prev];
        self.ent[seg].thing_slot = 0;
        self.ent[seg].f52 = prev as u16;
        self.ent[prev].f54 = seg as u16;
        self.ent[seg].f54 = 0;
        self.ent[seg].f63 = off.unsigned_abs() & 1;
        self.ent[seg].flags &= !4;
        self.ent[seg].f71 = off as u8;
        self.ent[seg].tick70 = M22_BASE + 4;
        self.ent[seg].f44 = 0;
        self.ent[seg].dest_x = 0;
        self.ent[seg].f140 = 0;
        self.ent[seg].f146 = head as u16;
        let (hx, hy, hz) = {
            let h = &self.ent[head];
            (h.x, h.y, h.z)
        };
        self.link(seg, hx, hy, hz);
        self.refill_life(seg);
    }

    /// `sub_27470` (EF:17822): the segment at a signed ring offset
    /// (0 = the head itself).
    fn m22_find_segment(&self, head: usize, off: i16) -> Option<usize> {
        if off == 0 {
            return Some(head);
        }
        let mut j = self.ent[head].f54 as usize;
        while j != 0 {
            if self.ent[j].f71 as i8 as i16 == off {
                return Some(j);
            }
            j = self.ent[j].f54 as usize;
        }
        None
    }

    /// The m22 castle resolver: the target player's
    /// `CastleEntityIndex_0x3A_58`.
    fn m22_target_castle(&self, target: u16) -> Option<usize> {
        self.mc2_castle_of(target)
    }

    /// m22 states 0xB0-0xB7 (docs/traces/mc2-m22-worm-helpers.md).
    pub(crate) fn m22_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M22_BASE {
            // 0xB0 idle: move/anim/damage/grow.
            0 => {
                self.m22_move(i);
                self.m22_anim(i);
                self.m22_dmg(i, ctx);
                self.m22_grow(i);
            }
            // 0xB1 chase + the colorize-inward sweep (sub_26990).
            1 => {
                self.m22_move(i);
                self.m22_anim(i);
                let center = (self.ent[i].f26 >> 8) as i16;
                let radius = (self.ent[i].f26 & 0xFF) as i16;
                let base = self.mc2_ball_color(self.ent[i].dest_x);
                let len = self.ent[i].f71;
                let mut found = false;
                let passes = if radius != 0 { 2 } else { 1 };
                for p in 0..passes {
                    let off = center + if p == 1 { -radius } else { radius };
                    if off.unsigned_abs() as u8 <= len / 2
                        && let Some(seg) = self.m22_find_segment(i, off)
                    {
                        let row = Self::m22_color_idx(base, len, off as i8);
                        self.mc2_particle_row(seg, row);
                        found = true;
                    }
                }
                if found {
                    self.ent[i].f26 = (radius + 1) | (center << 8);
                } else if self.ent[i].dest_x != 0 {
                    self.ent[i].tick70 = M22_BASE + 2; // → castle acquire
                } else {
                    self.ent[i].tick70 = M22_BASE; // → idle
                }
            }
            // 0xB2 castle acquire (sub_26AA0) — every 32nd phase.
            2 => {
                self.m22_move(i);
                self.m22_anim(i);
                self.m22_dmg(i, ctx);
                self.m22_grow(i);
                if self.ent[i].f63 & 0x1F != 0 {
                    return;
                }
                let target = self.ent[i].dest_x;
                let mut revert = false;
                if target == 0 {
                    revert = true;
                } else if self.ent[i].f126 <= self.ent[i].f130 {
                    // (still accelerated → hold in 0xB2 without any check)
                    match self.m22_target_castle(target) {
                        None => revert = true,
                        Some(c) => {
                            let (cx, cy) = {
                                let e = &self.ent[c];
                                (e.x, e.y)
                            };
                            let (ex, ey) = {
                                let e = &self.ent[i];
                                (e.x, e.y)
                            };
                            // Retail SNAPS the live heading
                            // (`roll_0x20_32 = tan2`, EF:17337) —
                            // f34 rides along so the move core's
                            // commit turn doesn't pull it back off
                            // the castle between aligned frames.
                            let aim = Self::angle_between(ex, ey, cx, cy);
                            self.ent[i].f34 = aim;
                            self.ent[i].f30 = aim;
                            // `EuclideanDistXYZ_58490` is 2-D despite
                            // the name (Maths:738-42 never reads z —
                            // the morph::dist2d law). A 3-D check here
                            // is unsatisfiable: the head cruises at
                            // chain-ground +384, the castle entity
                            // sits at ground, so the worm would hover
                            // at the flag forever, never absorbed.
                            let d2 = crate::mc2::morph::dist2d(ex, ey, cx as i32, cy as i32);
                            if self.ent[i].f63 & 3 == 0 && d2 <= 0x100 {
                                let room = (self.ent[i].f140 + self.ent[c].f140) < self.ent[c].f136;
                                if room {
                                    self.ent[i].f26 = 128;
                                    self.ent[i].tick70 = M22_BASE + 3; // → deposit
                                } else {
                                    revert = true;
                                }
                            }
                        }
                    }
                }
                if revert {
                    self.ent[i].f26 = 0;
                    self.ent[i].dest_x = 0;
                    self.ent[i].tick70 = M22_BASE + 1; // → chase/sweep
                }
            }
            // 0xB3 deposit / self-consume (sub_26BD0) — anim only.
            3 => {
                self.m22_anim(i);
                if self.ent[i].f26 != 0 {
                    self.ent[i].f26 -= 1;
                } else if self.ent[i].f63 & 1 == 0 {
                    // Timer expired: shrink every even phase (the
                    // countdown is NOT reloaded — EF:17381).
                    let len = self.ent[i].f71;
                    if len > 1 {
                        self.m22_resize(i, len - 2);
                    } else {
                        if let Some(c) = self.m22_target_castle(self.ent[i].dest_x) {
                            let total = self.ent[i].f140 + self.ent[c].f140;
                            self.ent[c].f140 = total.min(self.ent[c].f136);
                        }
                        self.ent[i].flags |= 0x400; // consumed
                    }
                }
            }
            // 0xB4 tail segment: spiral follow + hit relay.
            4 => {
                self.m22_tail_follow(i);
                self.m22_relay(i, ctx);
            }
            // 0xB5 chain-kill.
            5 => self.m22_chain_kill(i),
            // 0xB6: no unique body in retail (interior of sub_27720).
            6 => {}
            // 0xB7 spawn: sub_1D5D0 no-op for StageVar2==0.
            _ => {}
        }
    }

    // =========================================================================
    // MODEL 27 — the HYDRA: 5 bolt-spitting HEADS (branches) that
    // retract and re-grow when killed; the body is attackable only
    // while the f50 head gauge is 0.
    // (ctor sub_4D000 EF:34591 + finalizers; body brains EF:19443-
    // 19736; the branch machine docs/traces/mc2-m27-branch-machine.md)
    // =========================================================================

    /// `sub_4D000`: 1 body + 5 branches + 45 segments = 51 slots,
    /// one linear f54 chain; every member's f52/id24 point at the
    /// BODY. CAVE-EXCLUDED — on caves `v16 = 1` skips the whole
    /// construction and returns 0 (EF:34608-34690).
    pub(crate) fn mc2_spawn_m27(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        if self.is_cave() {
            return None;
        }
        if self.free.len() < 51 {
            return None;
        }
        let body = self.new_event()?;
        {
            let e = &mut self.ent[body];
            e.class64 = 5;
            e.model65 = 27;
            e.tick70 = M27_BASE + 1; // 0xD9
        }
        self.link(body, x, y, z);
        let mut prev = body;
        for b in 0..5u8 {
            let Some(br) = self.new_event() else { break };
            {
                let e = &mut self.ent[br];
                e.class64 = 5;
                e.model65 = 27;
                e.tick70 = BRANCH_STATE;
                e.f50 = b as i16; // byte_0x3B_59 — branch index
                e.id24 = body as u16;
                e.f52 = body as u16;
                e.f54 = 0;
            }
            self.ent[prev].f54 = br as u16;
            self.link(br, x, y, z);
            prev = br;
            for _ in 0..9 {
                let Some(seg) = self.new_event() else { break };
                {
                    let e = &mut self.ent[seg];
                    e.class64 = 5;
                    e.model65 = 27;
                    e.tick70 = TIER2_STATE;
                    e.f50 = b as i16;
                    e.id24 = body as u16;
                    e.f52 = body as u16;
                    e.f54 = 0;
                }
                self.ent[prev].f54 = seg as u16;
                self.link(seg, x, y, z);
                prev = seg;
            }
        }
        self.m27_body_init(body);
        self.m27_branch_init(body);
        self.m27_segment_init(body);
        Some(body)
    }

    /// `sub_2AC50` (EF:20730) — body finalize. Life = 1000000
    /// DIRECT (no CopyMaxLifeToLife); the f50 gauge starts at 5.
    fn m27_body_init(&mut self, body: usize) {
        let ord = self.mc2_ord(27);
        {
            let e = &mut self.ent[body];
            e.f30 = 0;
            e.f32 = 0;
            e.f34 = 0;
            e.f128 = 64;
            e.f130 = 0;
            e.f126 = 30;
            e.f50 = 5; // the live-branch gauge
            e.act_life = 1_000_000;
            e.max_life = 36000;
            e.f140 = 20000;
            e.f26 = (body % 100) as i16;
            e.f36 = 0;
            e.f28 = 1; // byte_0x38_56 = 1
            e.row156 = 97;
            e.f63 = ord;
            e.f66 = 3;
        }
        self.ent[body].f58 = BEHAVIOR[97].v_26 + 1; // byte_0x39_57 = v26+1
        self.mc2_set_sprite(body, 315);
        self.mc2_shift_rot(body, 1024, 1536);
    }

    /// `sub_2AD40` (EF:20770-800) — branch finalize: sprite 316, TWO
    /// RNG draws each (roll then fov), life ladder 460*v2+920 where
    /// v2 counts every chain NODE (the increment sits OUTSIDE the
    /// branch guard, EF:20798-99): branches sit at positions
    /// 1/11/21/31/41 → 1380/5980/10580/15180/19780. NOT
    /// branch-only counting, which makes 2-5 up to 6× too weak.
    fn m27_branch_init(&mut self, body: usize) {
        let mut v2 = 1i32;
        let mut j = self.ent[body].f54 as usize;
        while j != 0 {
            if self.ent[j].tick70 == BRANCH_STATE {
                self.mc2_set_sprite(j, 316);
                let d1 = self.mc2_rand(j);
                self.ent[j].f34 = (d1 & 0x7FF) as u16; // roll
                let d2 = self.mc2_rand(j);
                self.ent[j].f36 = (d2 & 0x7FF) as u16; // fov
                {
                    let e = &mut self.ent[j];
                    e.f128 = 16;
                    e.f126 = 16;
                    e.row156 = 103;
                    e.f28 = 1;
                    let v5 = (460 * v2 + 920) as u32;
                    e.max_life = v5;
                    e.act_life = v5 as i32;
                }
                // sub_2A940 places the fresh branch (EF:20798).
                self.m27_swing_branch(body, j);
            }
            v2 += 1;
            j = self.ent[j].f54 as usize;
        }
    }

    /// `sub_2A6F0` (EF:20452-83) — the m27 branch's OWN wizard scan:
    /// walks the wizard list with STRICT `<` on both dist² and the
    /// nearest compare and NO invisibility/hidden filter — unlike
    /// the shared `mc2_wizard_scan` (a different retail sub), which
    /// must keep its filters for its other callers.
    fn m27_wizard_scan(&self, i: usize, ctx: &MobCtx) -> Option<u16> {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let range = (row.v_28 as i32) * (row.v_28 as i32);
        let cone = row.v_30 as u16;
        let (ex, ey, eyaw) = (e.x, e.y, e.f30);
        let mut best: Option<(u16, i32)> = None;
        let mut consider = |tx: u16, ty: u16, slot: u16| {
            let d2 = Self::dist2_sq(ex, ey, tx, ty);
            if d2 >= range {
                return; // strict < (EF:20461)
            }
            let bearing = Self::angle_between(ex, ey, tx, ty);
            if Self::angdist(eyaw, bearing) >= cone {
                return;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((slot, d2));
            }
        };
        consider(ctx.px, ctx.py, PLAYER_TARGET);
        for (j, c) in self.ent.iter().enumerate().skip(1) {
            if c.class64 == 3 && c.model65 <= 1 && c.act_life >= 0 && c.flags & 0x400 == 0 {
                consider(c.x, c.y, j as u16);
            }
        }
        best.map(|(s, _)| s)
    }

    /// `sub_2AE30` (EF:20808) — segment finalize: sprite 317 only.
    fn m27_segment_init(&mut self, body: usize) {
        let mut j = self.ent[body].f54 as usize;
        while j != 0 {
            if self.ent[j].tick70 == TIER2_STATE {
                self.mc2_set_sprite(j, 317);
            }
            j = self.ent[j].f54 as usize;
        }
    }

    /// `sub_2A5B0` (EF:20374): branch-head anchor placement.
    fn m27_anchor_branch(&mut self, body: usize, br: usize, reach: i16) {
        let row = &D404C[(self.ent[br].f50 as usize).min(4)];
        let (bx, by, bz, byaw, bpitch) = {
            let b = &self.ent[body];
            (b.x, b.y, b.z, b.f30, b.f32)
        };
        let mut pred = (bx, by, bz);
        Self::polar_step(
            &mut pred,
            (row[D404C_W2] as u16).wrapping_add(byaw) & 0x7FF,
            0,
            row[D404C_W0],
        );
        pred.2 = pred.2.wrapping_add(row[D404C_W4]);
        Self::polar_step(
            &mut pred,
            (row[D404C_W12] as u16).wrapping_add(byaw) & 0x7FF,
            (row[D404C_W14] as u16).wrapping_add(bpitch) & 0x7FF,
            reach,
        );
        self.move_relink(br, pred.0, pred.1, pred.2);
    }

    /// `sub_2A940` (EF:20570): move the branch head by its swing
    /// speed along the splay direction (the `x_DWORD_E9BA8` freeze
    /// gate reads 0 — the normal arm; module doc).
    fn m27_swing_branch(&mut self, body: usize, br: usize) {
        if self.ent[br].f126 == 0 {
            return;
        }
        let (byaw, bpitch) = (self.ent[body].f30, self.ent[body].f32);
        let (x, y, z, roll, fov, spd) = {
            let e = &self.ent[br];
            (e.x, e.y, e.z, e.f34, e.f36, e.f126)
        };
        let mut pred = (x, y, z);
        Self::polar_step(
            &mut pred,
            roll.wrapping_add(byaw) & 0x7FF,
            fov.wrapping_add(bpitch) & 0x7FF,
            spd,
        );
        self.move_relink(br, pred.0, pred.1, pred.2);
    }

    /// `sub_2A9F0` (EF:20608): the trailing settle after the spline.
    fn m27_settle_branch(&mut self, body: usize, br: usize) {
        let row = &D404C[(self.ent[br].f50 as usize).min(4)];
        let (byaw, bpitch) = (self.ent[body].f30, self.ent[body].f32);
        let (x, y, z) = {
            let e = &self.ent[br];
            (e.x, e.y, e.z)
        };
        let mut pred = (x, y, z.wrapping_add(row[D404C_W10]));
        Self::polar_step(
            &mut pred,
            (row[D404C_W12] as u16).wrapping_add(byaw) & 0x7FF,
            (row[D404C_W14] as u16).wrapping_add(bpitch) & 0x7FF,
            row[D404C_W6],
        );
        self.move_relink(br, pred.0, pred.1, pred.2);
    }

    /// `sub_2AA90` (EF:20632): the 9-segment drooping-arc spline
    /// from the body anchor to the branch head; fixed 96-unit
    /// steps, symmetric pitch-bend pattern from the D40BC row.
    fn m27_spline_segments(&mut self, body: usize, br: usize) {
        let row = &D404C[(self.ent[br].f50 as usize).min(4)];
        let (bx, by, bz, byaw) = {
            let b = &self.ent[body];
            (b.x, b.y, b.z, b.f30)
        };
        let mut anchor = (bx, by, bz);
        Self::polar_step(
            &mut anchor,
            (row[D404C_W2] as u16).wrapping_add(byaw) & 0x7FF,
            0,
            row[D404C_W0],
        );
        anchor.2 = anchor.2.wrapping_add(row[D404C_W4]);
        let head_pos = {
            let e = &self.ent[br];
            (e.x, e.y, e.z)
        };
        let v5 = (Self::mc2_dist3(anchor, head_pos) as i32 - 468) / 24;
        let v18 = (16 - v5).clamp(0, 15) as usize;
        let yaw = Self::angle_between(anchor.0, anchor.1, head_pos.0, head_pos.1);
        let pitch = Self::mc2_radix_tan(anchor, head_pos);
        let extend = self.ent[br].f71 == 7 && self.ent[br].f69 == 8;
        let mut cursor = anchor;
        let mut seg = self.ent[br].f54 as usize;
        for v6 in 0..9 {
            if seg == 0 || self.ent[seg].tick70 != TIER2_STATE {
                break;
            }
            let bend: i16 = match v6 {
                0 => 0,
                1 | 8 => -D40BC[v18][0],
                2 | 7 => -D40BC[v18][1],
                3 | 6 => D40BC[v18][1],
                _ => D40BC[v18][0], // 4 | 5
            };
            if v6 != 0 {
                Self::polar_step(
                    &mut cursor,
                    yaw,
                    (bend as u16).wrapping_add(pitch) & 0x7FF,
                    96,
                );
            }
            let mut place = cursor;
            if extend {
                let g = self.ground_z(place.0, place.1) as i16;
                if place.2 <= g {
                    place.2 = g;
                }
            }
            self.move_relink(seg, place.0, place.1, place.2);
            seg = self.ent[seg].f54 as usize;
        }
    }

    /// `sub_2A340` (EF:20233): the branch speed/rotation integrator
    /// (mode in f44). Mode 0's default arm advances the branch LCG.
    fn m27_integrate(&mut self, br: usize) {
        match self.ent[br].f44 as i16 {
            0 => {
                {
                    let e = &mut self.ent[br];
                    let (mx, w36) = (e.f130, e.f56 as i16);
                    e.f34 = e.f34.wrapping_add((w36 + mx + 73) as u16);
                    e.f36 = e.f36.wrapping_add((w36 + mx + 62) as u16);
                    if e.f126 != 192 {
                        let v10 = e.f128 + e.f126;
                        e.f126 = v10;
                        if v10.abs() > 192 {
                            e.f126 = if e.f128 <= 0 { -192 } else { 192 };
                            e.f128 = -e.f128;
                        }
                    }
                    if e.f63 & 1 == 0 && e.f56 != 0 {
                        e.f56 -= 1;
                    }
                }
                if self.ent[br].f68 != 0 {
                    if self.ent[br].f68 == 3 && self.ent[br].f126 == 192 {
                        self.ent[br].f128 = -16;
                        self.ent[br].f126 += self.ent[br].f128;
                    }
                } else {
                    let d = self.mc2_rand(br);
                    self.ent[br].f130 = (d % 0x1C) as i16;
                }
            }
            1 => {
                let e = &mut self.ent[br];
                if e.f126.abs() < 192 {
                    e.f126 += e.f128;
                }
                if e.f128 <= 0 {
                    if e.f126 < -192 {
                        e.f126 = -192;
                    }
                } else if e.f126 > 192 {
                    e.f126 = 192;
                }
            }
            2 => {
                let e = &mut self.ent[br];
                if e.f126.abs() < e.f128 {
                    e.f126 = 0;
                } else if e.f126 <= 0 {
                    e.f126 += e.f128;
                } else {
                    e.f126 -= e.f128;
                }
            }
            3 | 4 => {
                let e = &mut self.ent[br];
                match e.f26 {
                    1 => e.f126 = -192,
                    2 => e.f126 = -130,
                    3 => e.f126 = -23,
                    4 => e.f126 = 192,
                    _ => {}
                }
            }
            6 => {
                let (x, y) = (self.ent[br].x, self.ent[br].y);
                let z = self.ent[br].z.wrapping_sub(self.ent[br].f56 as i16);
                self.ent[br].f126 -= self.ent[br].f128;
                let g = self.ground_z(x, y) as i16;
                self.move_relink(br, x, y, z.max(g));
            }
            _ => {}
        }
    }

    /// `sub_2A660` (EF:20395): branch hit intake — forward to the
    /// body's inbox, apply capped-76 damage to the BRANCH, death →
    /// sub-state 6 (retract; branches are regenerating limbs).
    fn m27_branch_intake(&mut self, body: usize, br: usize) {
        if self.ent[br].mail[0].1 == 0 {
            return;
        }
        let (amt, src) = self.ent[br].mail[0];
        self.ent[body].mail[0] = (amt, src);
        let v4 = amt.min(76);
        self.ent[br].act_life -= v4 as i32;
        self.ent[br].mail[0].1 = 0;
        self.ent[br].f40 = src;
        if self.ent[br].act_life < 0 {
            self.ent[br].f71 = 6;
        }
    }

    /// `sub_2A6B0` (EF:20423): body hit consume — 0 none / 1 hit
    /// while branches live / 2 exposed (gauge 0 → state 0xDC armed
    /// in place, attacker in f38).
    fn m27_body_intake(&mut self, body: usize) -> u8 {
        let src = self.ent[body].mail[0].1;
        if src == 0 {
            return 0;
        }
        self.ent[body].f40 = src;
        self.ent[body].mail[0].1 = 0;
        if self.ent[body].f50 != 0 {
            1
        } else {
            self.ent[body].tick70 = M27_BASE + 4; // 220
            self.ent[body].f38 = src;
            2
        }
    }

    /// `sub_2A7F0` (EF:20507): the branch bolt — (9,0) low / (9,9)
    /// high keyed on manaRegen (f136), subSpell 850, sounds 15/23
    /// at the BODY. The a3=0 re-fire path spawns only at regen 2.
    fn m27_branch_bolt(&mut self, br: usize, target: u16, low: bool, ctx: &MobCtx) {
        if low {
            let d = self.mc2_rand(br);
            // The `+= setting_30` perturb after the roll (EF:20521).
            self.mc2_rand_perturb(br, ctx.mc2_turn);
            self.ent[br].f136 = ((d % 12 > 7) as i32) + 1;
        }
        let regen = self.ent[br].f136;
        let (x, y, z, lift, body_id) = {
            let e = &self.ent[br];
            (e.x, e.y, e.z, (e.f84 / 2) as i16, e.id24)
        };
        let p = match regen {
            1 => {
                if !low {
                    return; // re-fire noop at regen 1
                }
                let Some(p) = self.mc2_spawn_bolt(x, y, z) else {
                    return;
                };
                self.ent[p].f68 = 10;
                self.ent[p].f69 = 0;
                self.snd(15, body_id as usize);
                p
            }
            2 => {
                let Some(p) = self.mc2_spawn_bolt9(x, y, z) else {
                    return;
                };
                self.ent[p].f68 = 10;
                self.ent[p].f69 = 23;
                self.snd(23, body_id as usize);
                p
            }
            _ => return,
        };
        self.ent[p].f44 = 850; // subSpellIndex
        self.ent[p].row156 = 106;
        let tpos =
            self.mc2_raw_pos(target, ctx)
                .unwrap_or((self.ent[p].x, self.ent[p].y, self.ent[p].z));
        self.mc2_arm_proj(p, br, target, tpos);
        self.ent[p].id24 = body_id;
        self.ent[p].f146 = self.ent[br].f146;
        let z2 = self.ent[p].z.wrapping_add(lift);
        let (px, py) = (self.ent[p].x, self.ent[p].y);
        self.move_relink(p, px, py, z2);
    }

    /// `sub_29A90` (EF:19737) — the body-driven branch machine.
    /// Walks the f54 chain, processes ONLY branches (0xE9): the
    /// pre-roll draws, the 16-way f71 switch, then the positioning
    /// dispatch (LABEL_94). The manual f63 increment here IS the
    /// branch's phase clock (branches have no dispatch of their
    /// own; the world loop skips their f63).
    pub(crate) fn m27_drive(&mut self, body: usize, ctx: &MobCtx) {
        let mut br = self.ent[body].f54 as usize;
        while br != 0 {
            let next = self.ent[br].f54 as usize;
            if self.ent[br].tick70 == BRANCH_STATE {
                self.m27_drive_branch(body, br, ctx);
            }
            br = next;
        }
    }

    fn m27_drive_branch(&mut self, body: usize, br: usize, ctx: &MobCtx) {
        let mut v37 = 0u8; // projectile-this-tick flag
        let mut v34: u32 = 0x1000_002B; // the leftover-nonzero seed (EF:19783)
        let v5 = self.ent[br].f71;
        self.ent[br].f63 = self.ent[br].f63.wrapping_add(1);

        // Pre-roll (EF:19807-19838).
        if v5 <= 5 {
            let d = self.mc2_rand(br); // DRAW #A
            self.ent[br].f68 = (d % 0x14) as u8;
            self.m27_anchor_branch(body, br, 672);
            self.m27_branch_intake(body, br);
            if self.ent[br].f71 == 1 {
                let d = self.mc2_rand(br); // DRAW #B
                let v6 = d & 7;
                v34 = v6;
                let v7 = self.ent[br].f40;
                if v7 != 0 {
                    if v6 < 4 {
                        self.ent[br].f40 = 0;
                        self.ent[br].f71 = 2;
                        self.ent[br].f146 = v7;
                        let w = self.ent[br].f56.wrapping_add(22);
                        self.ent[br].f56 = w.min(68);
                    }
                } else if v6 < 4 && self.ent[body].f146 != 0 && self.ent[br].f63 & 7 == 0 {
                    self.ent[br].f71 = 2;
                    self.ent[br].f146 = self.ent[body].f146;
                }
            }
        }

        // The 16-way switch (EF:19839-20185), fall-throughs modeled
        // by consecutive ifs on the CURRENT f71.
        let mut state = self.ent[br].f71;
        if state == 0 {
            // case 0 → arm, fall into case 1.
            let e = &mut self.ent[br];
            e.f146 = 0;
            e.f71 = 1;
            e.f44 = 0;
            e.f56 = 0;
            e.f128 = 16;
            state = 1;
        }
        match state {
            1 => {
                if self.ent[body].f58 != 0 {
                    if self.ent[br].f63 & 7 == 0 {
                        if v34 != 0 {
                            if v34 > 4 {
                                self.ent[br].f71 = 4;
                            }
                        } else if let Some(t) = self.m27_wizard_scan(br, ctx) {
                            self.ent[br].f71 = 2;
                            self.ent[br].f146 = t;
                        }
                    }
                    if self.ent[br].f63 & 7 == 0 && v34 & 1 == 0 {
                        let d = self.mc2_rand(br); // DRAW #C — wander yaw
                        let fov = BEHAVIOR[103].v_30.max(1) as u32;
                        let w12 = D404C[(self.ent[br].f50 as usize).min(4)][D404C_W12];
                        let base = (self.ent[body].f30 as i32 + w12 as i32 - fov as i32) as u16;
                        self.ent[br].f30 = base.wrapping_add((d % fov) as u16) & 0x7FF;
                    }
                }
            }
            2 | 3 => {
                if state == 2 {
                    // case 2 → begin forward whip, fall into 3.
                    let e = &mut self.ent[br];
                    e.f71 = 3;
                    e.f69 = 0;
                    e.f44 = 2;
                    e.f128 = 16;
                }
                // case 3: forward whip, target-tracked (sub_2A7B0
                // validity ≡ mc2_target).
                if let Some(tpos) = self.mc2_target(self.ent[br].f146, ctx) {
                    let sub = self.ent[br].f69;
                    if sub == 0 {
                        if self.ent[br].f126 == 0 {
                            let (bx, by, bz) = {
                                let e = &self.ent[br];
                                (e.x, e.y, e.z)
                            };
                            let yaw = Self::angle_between(bx, by, tpos.0, tpos.1);
                            let pitch = Self::mc2_radix_tan((bx, by, bz), tpos);
                            let (byaw, bpitch) = (self.ent[body].f30, self.ent[body].f32);
                            let e = &mut self.ent[br];
                            e.f69 = 1;
                            e.f44 = 1;
                            e.f128 = 16;
                            e.f30 = yaw;
                            e.f32 = pitch;
                            e.f34 = yaw.wrapping_sub(byaw);
                            e.f36 = pitch.wrapping_sub(bpitch);
                        }
                    } else if sub == 1 {
                        if self.ent[br].f126 == 192 {
                            let e = &mut self.ent[br];
                            e.f44 = 3;
                            e.f69 = 3;
                            e.f26 = 4;
                            v37 = 1;
                        }
                    } else if sub == 3 {
                        v37 = 2;
                        self.ent[br].f26 -= 1;
                        if self.ent[br].f26 == 0 {
                            self.ent[br].f71 = 0;
                            self.ent[br].f26 = 1;
                        }
                    }
                } else {
                    self.ent[br].f71 = 0; // target lost
                }
            }
            4 | 5 => {
                if state == 4 {
                    let e = &mut self.ent[br];
                    e.f71 = 5;
                    e.f69 = 0;
                    e.f44 = 2;
                    e.f128 = 16;
                }
                // case 5: back-swing (no target).
                match self.ent[br].f69 {
                    0 => {
                        if self.ent[br].f126 == 0 {
                            let row = &D404C[(self.ent[br].f50 as usize).min(4)];
                            let (byaw, bpitch) = (self.ent[body].f30, self.ent[body].f32);
                            let e = &mut self.ent[br];
                            e.f69 = 1;
                            e.f44 = 1;
                            e.f128 = -16;
                            e.f34 = row[D404C_W12] as u16;
                            e.f36 = row[D404C_W14] as u16;
                            e.f30 = e.f34.wrapping_add(byaw) & 0x7FF;
                            e.f32 = e.f36.wrapping_add(bpitch) & 0x7FF;
                        }
                    }
                    1 => {
                        if self.ent[br].f126 == -192 {
                            self.ent[br].f69 = 2;
                            self.ent[br].f26 = 2;
                        }
                    }
                    2 => {
                        self.ent[br].f26 -= 1;
                        if self.ent[br].f26 == 0 {
                            let e = &mut self.ent[br];
                            e.f44 = 4;
                            e.f69 = 6;
                            e.f26 = 1;
                        }
                    }
                    5 => {
                        self.ent[br].f26 -= 1;
                        if self.ent[br].f26 <= 4 {
                            self.ent[br].f71 = 0;
                            self.ent[br].f26 = 4;
                        }
                    }
                    6 => {
                        self.snd(17, body); // whip crack (EF:19987)
                        self.ent[br].f26 += 1;
                        if self.ent[br].f26 >= 4 {
                            self.ent[br].f69 = 5;
                        }
                    }
                    _ => {} // → LABEL_94 with the current state
                }
            }
            6 | 7 => {
                if state == 6 {
                    let e = &mut self.ent[br];
                    e.f26 = 0;
                    e.f44 = 2;
                    e.f71 = 7;
                    e.f69 = 0;
                    e.f128 = 80;
                }
                // case 7: segment-extend animation.
                let mut v36 = false;
                self.m27_anchor_branch(body, br, 672);
                match self.ent[br].f69 {
                    0 => {
                        if self.ent[br].f126 == 0 {
                            let row = &D404C[(self.ent[br].f50 as usize).min(4)];
                            let e = &mut self.ent[br];
                            e.f69 = 1;
                            e.f44 = 1;
                            e.f34 = row[D404C_W12] as u16;
                            e.f36 = row[D404C_W14] as u16;
                        }
                    }
                    1 => {
                        v36 = true;
                        if self.ent[br].f126 == 192 {
                            let e = &mut self.ent[br];
                            e.f69 = 7;
                            e.f44 = 5;
                            e.f26 = 8;
                        }
                    }
                    7 => {
                        v36 = true;
                        self.ent[br].f26 -= 1;
                        if self.ent[br].f26 == 0 {
                            let e = &mut self.ent[br];
                            e.f69 = 8;
                            e.f44 = 6;
                            e.f36 = 0;
                            e.f68 = 0;
                            e.f56 = 0;
                            e.f128 = 12;
                            e.f26 = 0;
                        }
                    }
                    8 => {
                        let v18 = self.ent[br].f26;
                        if v18 > 10 {
                            self.ent[br].f71 = 8;
                        } else {
                            // Progressively HIDE the chain from the
                            // far end — retail `(byte[0]|1) & 0xF7`
                            // per node (EF:20055/20069-70): hidden +
                            // UNTARGETABLE while burrowing.
                            let hide: usize = if v18 != 0 {
                                let mut s = self.ent[br].f54 as usize;
                                let mut k = 0;
                                while k < 9 - v18 && s != 0 {
                                    s = self.ent[s].f54 as usize;
                                    k += 1;
                                }
                                s
                            } else {
                                br
                            };
                            if hide != 0 {
                                let f = &mut self.ent[hide].flags;
                                *f = (*f | 0x21) & !0x08;
                            }
                            let v22 = self.ent[br].f68 + 1;
                            self.ent[br].f26 += 1;
                            self.ent[br].f68 = v22;
                            self.ent[br].f56 = self.ent[br].f56.wrapping_add(28 * v22 as u16);
                        }
                    }
                    _ => {}
                }
                if v36 {
                    let v23: i16 = if self.ent[br].f63 & 1 != 0 { -204 } else { 204 };
                    let w12 = D404C[(self.ent[br].f50 as usize).min(4)][D404C_W12];
                    self.ent[br].f30 =
                        (self.ent[body].f30 as i32 + w12 as i32 + v23 as i32) as u16 & 0x7FF;
                }
                self.m27_integrate(br);
                self.m27_swing_branch(body, br);
            }
            8 | 9 => {
                if state == 8 {
                    self.ent[br].f71 = 9;
                    self.ent[br].f26 = 100;
                    self.ent[body].f50 -= 1; // gauge --
                }
                self.ent[br].f26 -= 1;
                if self.ent[br].f26 == 0 {
                    self.ent[br].f71 = 10;
                }
            }
            10 | 11 => {
                if state == 10 {
                    self.ent[body].f50 += 1; // gauge ++
                    let first_seg = self.ent[br].f54;
                    let row = &D404C[(self.ent[br].f50 as usize).min(4)];
                    let e = &mut self.ent[br];
                    e.f71 = 11;
                    e.f44 = 5;
                    e.f26 = 7;
                    e.f146 = first_seg;
                    // Case 0xA re-show: `(byte[0] & 0xF6) | 8` —
                    // shown AND re-targetable (EF:20113-17).
                    e.flags = (e.flags & !0x21) | 0x08;
                    e.f34 = row[D404C_W12] as u16;
                    e.f126 = 156;
                    e.f36 = row[D404C_W14] as u16;
                }
                self.ent[br].f26 -= 1;
                if self.ent[br].f26 <= 0 {
                    self.ent[br].f71 = 12;
                    self.ent[br].f26 = 0;
                }
            }
            12 => {
                // case 0xC: sequential segment re-show + branch
                // regrow-life roll.
                if self.ent[br].f26 < 9 {
                    let mut s = self.ent[br].f54 as usize;
                    let mut k = 0;
                    while k < self.ent[br].f26 && s != 0 {
                        s = self.ent[s].f54 as usize;
                        k += 1;
                    }
                    if s != 0 {
                        // Case 0xC: `byte[0] &= 0xFE` — show only,
                        // bit 3 untouched (EF:20144).
                        self.ent[s].flags &= !0x21;
                        self.ent[br].f146 = self.ent[s].f54;
                    }
                    self.ent[br].f26 += 1;
                    if self.ent[br].f26 >= 9 {
                        let d = self.mc2_rand(br); // DRAW #D — regrow life
                        self.ent[br].mail[0].1 = 0;
                        self.ent[br].f71 = 0;
                        self.ent[br].act_life = (d % 0x398) as i32 + 920;
                    }
                }
            }
            13 | 14 => {
                if state == 13 {
                    let e = &mut self.ent[br];
                    e.f71 = 14;
                    e.f68 = 10;
                    e.f26 = 10;
                }
                self.ent[br].f26 -= 1;
                if self.ent[br].f26 == 0 {
                    self.ent[br].f71 = 15;
                }
            }
            15 => {
                // case 0xF: hide branch + its 9 segments, → detach.
                self.ent[br].f71 = 8;
                let mut m = br;
                for _ in 0..10 {
                    // `(byte[0]|1) & 0xF7` on all 10 (EF:20177).
                    self.ent[m].flags = (self.ent[m].flags | 0x21) & !0x08;
                    m = self.ent[m].f54 as usize;
                    if m == 0 {
                        break;
                    }
                }
            }
            _ => {}
        }

        // LABEL_94 (EF:20186): the positioning dispatch on the
        // (possibly updated) sub-state.
        match self.ent[br].f71 {
            0..=5 => {
                self.m27_integrate(br);
                self.m27_swing_branch(body, br);
                self.m27_spline_segments(body, br);
                if v37 != 0 {
                    let target = self.ent[br].f146;
                    self.m27_branch_bolt(br, target, v37 == 1, ctx);
                    self.snd(17, body);
                }
                self.m27_settle_branch(body, br);
            }
            6 | 7 => self.m27_spline_segments(body, br),
            11 | 12 => {
                self.m27_anchor_branch(body, br, 672);
                self.m27_swing_branch(body, br);
                self.m27_spline_segments(body, br);
                let t = self.ent[br].f146 as usize;
                if t != 0 && t < self.ent.len() {
                    let (tx, ty, tz) = {
                        let e = &self.ent[t];
                        (e.x, e.y, e.z)
                    };
                    self.move_relink(br, tx, ty, tz);
                }
                self.m27_settle_branch(body, br);
            }
            _ => {} // 8, 9, 10, 13, 14, 15 → no positioning
        }
    }

    /// The m27 ground mover (`sub_2AF10` EF:20869) — returns
    /// 1 same-tile / 2 moved / 3 turned / 4 fully blocked (which
    /// arms the 0xD8 teleport in place). `commit` = the a2 flag.
    pub(crate) fn m27_move(&mut self, body: usize, commit: bool) -> u8 {
        let (x, y, z, yaw, roll, spd) = {
            let e = &self.ent[body];
            (e.x, e.y, e.z, e.f30, e.f34, e.f126)
        };
        let mut pred = (x, y, z);
        if commit {
            Self::polar_step(&mut pred, yaw, 0, spd);
        }
        pred.2 = self.ground_z(pred.0, pred.1) as i16;
        let mut moved = false;
        let mut turned = false;
        let code: u8;
        if commit && x >> 8 == pred.0 >> 8 && y >> 8 == pred.1 >> 8 {
            moved = true;
            turned = true;
            code = 1;
        } else if self.mc2_path_blocked(body, pred) || self.roughness(pred.0, pred.1) >= 32 {
            if yaw == roll {
                // Scan ±91-step yaws for a free heading.
                let mut v7: i32 = 91;
                let mut v9: i32 = 1;
                let mut found = None;
                while v7 <= 1024 {
                    let cand = ((yaw as i32 + v9 * v7) & 0x7FF) as u16;
                    let mut p2 = (x, y, z);
                    Self::polar_step(&mut p2, cand, 0, spd);
                    p2.2 = self.ground_z(p2.0, p2.1) as i16;
                    if !self.mc2_path_blocked(body, p2) && self.roughness(p2.0, p2.1) < 32 {
                        found = Some(cand);
                        break;
                    }
                    v9 = -v9;
                    if v9 == 1 {
                        v7 += 91;
                    }
                }
                if let Some(cand) = found {
                    self.ent[body].f34 = cand;
                    turned = true;
                    code = 3;
                } else {
                    code = 4;
                }
            } else {
                turned = true;
                code = 3;
            }
        } else {
            moved = true;
            turned = true;
            code = 2;
        }
        if commit && moved {
            self.move_relink(body, pred.0, pred.1, pred.2);
        }
        if turned {
            // The live clamp is sub_58350's LAST arg = row v_2 = 22
            // (EF:20967-72); v_4 (=5) is the dead third arg — the
            // same trap the mc2 move core documents.
            let cap = BEHAVIOR[self.ent[body].row156 as usize].v_2;
            let e = &self.ent[body];
            let step = Self::turn_step(e.f30, e.f34, cap);
            self.ent[body].f30 = (self.ent[body].f30 as i32 + step as i32) as u16 & 0x7FF;
        }
        let (bx, by) = (self.ent[body].x, self.ent[body].y);
        let g = self.ground_z(bx, by) as i16;
        self.move_relink(body, bx, by, g);
        if code == 4 {
            self.ent[body].tick70 = M27_BASE; // 216
            self.ent[body].f26 = 0;
        }
        code
    }

    /// `sub_2AE80` (EF:20830): hide/reap the entire chain.
    fn m27_hide_chain(&mut self, i: usize) {
        let mut j = self.ent[i].f54 as usize;
        while j != 0 {
            let next = self.ent[j].f54 as usize;
            self.ent[j].flags |= 0x400;
            j = next;
        }
        self.ent[i].flags |= 0x400;
    }

    /// `sub_2AED0` (EF:20852): pose set (on change only).
    pub(crate) fn m27_pose(&mut self, i: usize, row: u16) {
        if self.ent[i].type86 != row {
            self.ent[i].type86 = row;
            self.ent[i].frame88 = 0;
        }
    }

    /// m27 states 0xD8-0xDF (EF:19443-19736 verbatim; branches and
    /// tier-2 segments never reach here — the world loop leaves
    /// 0xE9/0xEA undispatched like retail's null table entries).
    pub(crate) fn m27_tick(&mut self, i: usize, ctx: &MobCtx) {
        match self.ent[i].tick70 - M27_BASE {
            // 0xD8 — emerge/teleport sequencer on the f26 phase.
            0 => {
                let v1 = self.ent[i].f26;
                self.ent[i].f26 += 1;
                match v1 {
                    0 => {
                        self.m27_pose(i, 337);
                        self.ent[i].act_life = 1_000_000;
                        self.ent[i].f146 = 0;
                    }
                    9 => {
                        // The teleport probe: TWO draws, then up to
                        // 128 steps of 768 along the body yaw.
                        let d1 = self.mc2_rand(i);
                        let d2 = self.mc2_rand(i);
                        let (x, y, yaw) = {
                            let e = &self.ent[i];
                            (e.x, e.y, e.f30)
                        };
                        let mut pred = (
                            x.wrapping_add((((d1 & 7) + 8) << 8) as u16),
                            y.wrapping_add((((d2 & 7) + 8) << 8) as u16),
                            0i16,
                        );
                        for _ in 0..128 {
                            pred.2 = self.ground_z(pred.0, pred.1) as i16;
                            if !self.mc2_path_blocked(i, pred)
                                && self.roughness(pred.0, pred.1) < 32
                            {
                                break;
                            }
                            Self::polar_step(&mut pred, yaw, 0, 768);
                        }
                        self.move_relink(i, pred.0, pred.1, pred.2);
                        self.snd(22, i);
                    }
                    18 => {
                        self.ent[i].tick70 = M27_BASE + 2; // 218
                        self.m27_pose(i, 337);
                        self.ent[i].f146 = 0;
                        self.ent[i].f71 = 1;
                    }
                    // Phases 3/6/12/15: chain draw-group staging —
                    // renderer markers, unmodeled (module doc).
                    _ => {}
                }
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                let g = self.ground_z(x, y) as i16;
                self.move_relink(i, x, y, g);
                self.m27_drive(i, ctx);
            }
            // 0xD9 — the main brain.
            1 => {
                match self.m27_body_intake(i) {
                    1 => {
                        self.ent[i].tick70 = M27_BASE + 2; // 218
                        let att = self.ent[i].f40;
                        self.ent[i].act_life = 1_000_000;
                        self.ent[i].f146 = att;
                    }
                    2 => {
                        self.m27_drive(i, ctx);
                        return;
                    }
                    _ => {}
                }
                let v3 = self.m27_move(i, true);
                if v3 >= 3 {
                    self.m27_drive(i, ctx);
                    return;
                }
                if self.ent[i].f63 & 0x3F == 0 {
                    let d = self.mc2_rand(i);
                    self.ent[i].f34 =
                        ((d % 0x1C7) as i32 + self.ent[i].f34 as i32 - 227) as u16 & 0x7FF;
                }
                self.m27_drive(i, ctx);
            }
            // 0xDA — chase-target brain.
            2 => {
                let mut drop_target = false;
                match self.m27_body_intake(i) {
                    1 => {
                        self.ent[i].act_life = 1_000_000;
                        let att = self.ent[i].f40;
                        self.ent[i].f146 = att;
                    }
                    2 => {
                        self.m27_drive(i, ctx);
                        return;
                    }
                    _ => {}
                }
                let commit = self.ent[i].f71 == 0;
                let v2 = self.m27_move(i, commit);
                if v2 == 4 {
                    // (m27_move already armed 216.)
                } else if let Some(tpos) = self.mc2_target(self.ent[i].f146, ctx) {
                    let (x, y, z) = {
                        let e = &self.ent[i];
                        (e.x, e.y, e.z)
                    };
                    if self.ent[i].f63 & 3 == 0 && v2 != 3 && self.ent[i].f71 == 0 {
                        self.ent[i].f34 = Self::angle_between(x, y, tpos.0, tpos.1);
                    }
                    if self.ent[i].f63 & 0x1F == 0 {
                        let row = &BEHAVIOR[self.ent[i].row156 as usize];
                        let yaw = Self::angle_between(x, y, tpos.0, tpos.1);
                        if Self::angdist(self.ent[i].f30, yaw) <= row.v_30 as u16 {
                            self.ent[i].f71 = 1;
                            self.m27_pose(i, 337);
                        } else {
                            self.ent[i].f71 = 0;
                            self.m27_pose(i, 315);
                        }
                        if Self::mc2_dist3((x, y, z), tpos) >= row.v_28 as u32 {
                            drop_target = true;
                        }
                    }
                } else {
                    drop_target = true;
                }
                if drop_target {
                    self.ent[i].tick70 = M27_BASE + 1; // 217
                    self.m27_pose(i, 315);
                    self.ent[i].f146 = 0;
                    self.ent[i].f71 = 0;
                }
                self.m27_drive(i, ctx);
            }
            // 0xDB — return-then-idle.
            3 => {
                self.ent[i].tick70 = M27_BASE + 1;
                // ... and run the 0xD9 body this tick (sub_29890).
                self.m27_tick(i, ctx);
            }
            // 0xDC — life = -1, PreKill cascades 0xDD over the chain.
            4 => {
                self.ent[i].act_life = -1;
                self.mc2_prekill(i, M27_BASE);
            }
            // 0xDD — death: mana spheres (fraction mode), the (10,1)
            // burst, then hide the whole chain. Branches/segments
            // cascaded here each run this too (their own spheres are
            // empty — mana 0 — but the burst pops).
            5 => {
                self.ent[i].act_life = -1;
                self.mc2_mana_spheres(i, true);
                if self.ent[i].flags & super::mobs::F_NO_CORPSE == 0 {
                    self.mc2_corpse_burst(i);
                }
                self.m27_hide_chain(i);
            }
            // 0xDE — no unique body (tail of sub_298D0).
            6 => {}
            // 0xDF with StageVar2==0 (an ordinary spawn/appear —
            // never stage-held): retail's sub_29930 head `sub_1D5D0`
            // is a no-op at kind 0, the pose select reads v1=0 → 315,
            // and neither command arm can fire (tick70 stays 223), so
            // pose + life + drive IS the verbatim reduction. A
            // stage-HELD body (site_z 1..=9/10/15) never reaches this
            // dispatch — the world loop routes it through
            // `World::mc2_m27_held_tick` (stagevars.rs), the full
            // sub_29930 port with the 0xDA mass-attack broadcast and
            // the 0xD8→StageVar2=15 arm.
            _ => {
                self.m27_pose(i, 315);
                self.ent[i].act_life = 1_000_000;
                self.m27_drive(i, ctx);
            }
        }
    }
}
