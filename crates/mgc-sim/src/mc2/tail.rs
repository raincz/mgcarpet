//! MC2 class-10 TAIL EFFECTS — the small-count effect
//! band: (10,52) castle anchor, (10,25)/(10,23) one-shot blasts,
//! (10,17) meteor, (10,15) fire trail + its (10,11→19) ground-fire
//! spray, (10,54) proximity aura. Trace bank:
//! docs/traces/mc2-class10-m50-chains-and-tail.md (§3-§7) +
//! mc2-class10-m6-m9-m11-m28-m31.md (§3, the 11→19 remap)
//! (`EF:` = remc2 EventsFunctions.cpp).
//!
//! Entity-field homes follow the class-10 effect column: subSpell
//! (the area amount) → f140, `dword_0x10_16` scratch → f26,
//! `byte_0x46_70` → f71, `word_0x26_38` → f40.
//!
//! DELIBERATE APPROXIMATIONS (cited):
//! - `sub_6D8B0(id, kind, hits)` spellbook reports ((10,17) kind 9,
//!   (10,23) kind 7, (10,15)'s spray kind — the spell-XP intake):
//!   the hit counts are computed and dropped.
//! - The (10,19) spray's `word_0x33` singleton latch IS ported: the
//!   summit-18 eruption registers each new column and kills the
//!   previous (`plume`, morph.rs `mc2_summit18_tick` — EF:23962-64),
//!   and the spray's death releases it (`mc2_fire_spray_tick`,
//!   EF:24148). The old "no ported writer" note here was stale.
//! - `AddEvent2_847D0` attached lights/children ((10,23)'s
//!   (128,9,0)) are presentation, unported (the (10,1) note).
//! - The (10,54) aura scans retail's `dword_38523` creature list —
//!   our pool slot-order scan over the mobs.rs list stands in.

use super::sprite_params::SPRITE_PARAMS;
use crate::engine::features::Gen;
use crate::mc1::combat::MailTarget;
use crate::mc1::mobs::{MobCtx, PLAYER_TARGET};

/// The whirlwind's victim GRAB latch (retail byte[3] & 0x10, dword
/// 0x1000_0000) — a free high bit next to the mobs.rs MC2 band.
// NB: NOT 1 << 29 — that is [`super::proj::F_MC2PROJ`]'s bit, and the
// whirlwind teardown clears this flag over a radius-12 disc on EVERY
// entity class (tail of `sub_338D0`); reusing 1 << 29 would strip the
// MC2-column marker off any projectile caught in the sweep, dropping
// it to the MC1 handler with an MC2 behavior row.
pub(crate) const F_GRABBED: u32 = 1 << 22;

impl Gen {
    // ---- ctors ---------------------------------------------------------------

    /// `sub_50430` (EF:36772) — the (10,52) permanent CASTLE/BUILDING
    /// ANCHOR: sprite 205, maxLife 100000 (effectively immortal),
    /// subSpell 500, a 500/2000 mana pool, untargetable. Its action
    /// 0x38 is an EMPTY EV case (EV:2693) — the entity ticks nothing,
    /// which the class-10 dispatch's fall-through arm already is.
    /// maxMana (2000) has no ported home or reader until the MC2
    /// building economy pass — the mana pool rides f140's mana home.
    pub(crate) fn mc2_spawn_castle_anchor(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 52;
            e.tick70 = 0x38;
            e.max_life = 100000;
            e.f140 = 500; // mana_0x90_144 (subSpell 500 shares the value)
            e.f26 = 600;
            e.flags &= !8;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 205);
        Some(i)
    }

    /// `sub_4F6A0` (EF:36110) — the (10,25) one-shot area blast,
    /// damage TYPE 3: maxLife 8, subSpell 2000 (set but the burst
    /// amount is `byte_0x46_70` — par-set by the caster), byte[0] =
    /// (&0xF6)|1, map-registered, extents 512. No sprite, no RNG.
    pub(crate) fn mc2_spawn_blast25(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 25;
            e.tick70 = 0x19;
            e.max_life = 8;
            e.f140 = 2000;
            e.flags = (e.flags & !0x9) | 1;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_shift_rot(i, 512, 512);
        Some(i)
    }

    /// `sub_4F5F0` (EF:36087) — the (10,23) one-shot area blast,
    /// type 0 amount 25: sprite 7, extents 200, the fire-ctor flag
    /// pattern + bit 0, sound 24 on the burst. The attached
    /// `AddEvent2_847D0(128, 9, 0)` child is presentation, skipped.
    pub(crate) fn mc2_spawn_blast23(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 23;
            e.tick70 = 0x17;
            e.max_life = 8;
            e.f140 = 25;
            e.flags = (e.flags & !0x2_0008) | 0x2_0000;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 7);
        self.mc2_shift_rot(i, 200, 200);
        self.ent[i].flags |= 1;
        Some(i)
    }

    /// `sub_4FFB0` (EF:36559) — the (10,38) LIGHTNING STORM cloud
    /// (Lightning L1/L2's `sub_66FD0` detonation, EF:58821): class-10
    /// model-38, action 40, maxLife 32, sprite 272, render scale 512. It
    /// hovers to +1024 above terrain, then RAINS (9,9) beams — the tick
    /// ([`Gen::mc2_storm_tick`]). The impact tail seeds `f140` with the
    /// tier's subSpell (300/800), which the tick hands to each beam.
    pub(crate) fn mc2_spawn_lightning_burst(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 38;
            e.tick70 = 40;
            e.max_life = 32;
            e.f140 = 300; // overridden by the impact tail (subSpell)
            e.flags &= !8;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 272);
        self.mc2_shift_rot(i, 512, 512);
        Some(i)
    }

    /// `sub_35640` (EF:25876, action 40) — the (10,38) STORM tick: first
    /// rise to +1024 above terrain (64/tick, life frozen while settling),
    /// then each tick fire TWO opposite-yaw (9,9) lightning beams DOWN
    /// (pitch 56), each with a third of the beam reach and a (10,23)
    /// ground impact carrying the storm's subSpell damage; the first of
    /// the pair claps thunder (sound 23). ~2 bolts/tick over 32 ticks =
    /// the rain. (docs/spell-audit/lightning.md — the storm is a cloud
    /// that rains chained beams, NOT a single blast.)
    pub(crate) fn mc2_storm_tick(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let ground = self.ground_z(x, y) as i32;
        let target = (ground + 1024).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let z = self.ent[i].z;
        // Settle at the hover height before raining (life frozen).
        if z < target {
            self.ent[i].z = z.saturating_add(64).min(target);
            return;
        }
        if z > target {
            self.ent[i].z = target;
            return;
        }
        // PRE-decrement life test (`v3 = life; life = v3-1; if (v3
        // >= 0)`, EF:25905-07; a post-test cuts the storm one tick
        // short).
        let old_life = self.ent[i].act_life;
        self.ent[i].act_life = old_life - 1;
        if old_life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let (sz, id, dmg) = {
            let e = &self.ent[i];
            (e.z, e.id24, e.f140)
        };
        let r = self.mc2_rand(i);
        let base = (r & 0x7FF) as u16;
        for k in 0..2u16 {
            let yaw = base.wrapping_add(k.wrapping_mul(1024)) & 0x7FF; // opposite
            if let Some(b) = self.mc2_spawn_cast_proj(9, x, y, sz) {
                {
                    let e = &mut self.ent[b];
                    e.id24 = id; // the storm's owner owns the rained beams
                    e.f30 = yaw;
                    e.f32 = 56; // pitch DOWN
                    e.f34 = yaw;
                    e.f36 = 56;
                    e.f146 = 0; // no homing — rain straight down
                    e.f68 = 10;
                    e.f69 = 23; // each beam impacts into (10,23)
                    e.f44 = dmg.clamp(0, u16::MAX as i32) as u16;
                    // `life /= 3` on act_life ONLY, no max_life
                    // touch, no floor (EF:25928).
                    e.act_life /= 3;
                }
                // The thunder is SPAWN-GATED, first pair-iteration
                // only, keyed on the BEAM (EF:25935-36).
                if k == 0 {
                    self.snd(23, b);
                }
            }
        }
    }

    /// `AddMeteor_4ED70` (EF:35731) — the (10,17) METEOR impact:
    /// maxLife 10, subSpell 3000, untargetable, NOT map-registered,
    /// no sprite of its own (the tick grows the quad). No RNG.
    pub(crate) fn mc2_spawn_meteor(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 17;
            e.tick70 = 17;
            e.max_life = 10;
            e.f140 = 3000;
            e.flags &= !8;
            e.x = x;
            e.y = y;
            e.z = z;
        }
        self.refill_life(i);
        Some(i)
    }

    /// `sub_4ECD0` (EF:35707) — the (10,15) wandering FIRE TRAIL:
    /// maxLife 128, actSpeed 256, subSpell 100, ONE RNG draw for the
    /// random yaw, extents (1024, 0x4000). Not map-registered.
    pub(crate) fn mc2_spawn_fire_trail(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 15;
            e.tick70 = 15;
            e.max_life = 128;
            e.f126 = 256; // actSpeed
            e.flags &= !8;
            e.f140 = 100;
            e.f26 = 0;
            e.x = x;
            e.y = y;
            e.z = z;
        }
        let d = self.mc2_rand(i);
        self.ent[i].f30 = (d & 0x7FF) as u16;
        self.refill_life(i);
        self.mc2_shift_rot(i, 1024, 0x4000);
        Some(i)
    }

    /// The (10,19) GROUND-FIRE-SPRAY creator (sprite 228, the fire
    /// family; maxLife 240, subSpell 200, map-registered, byte[0]
    /// bit0 set / bit3 clear, no RNG) — spawned by the dome summit
    /// and the (10,16) vortex machinery.
    ///
    /// A (10,11) THING is NOT a (10,19) entity — retail's
    /// creator-table row 0xB is `NewAdd0A0B_4E840` (EF:1715 →
    /// :35553), the (10,11) SCORCH RING below, a 40-tick one-shot.
    /// (Routing authored (10,11)s here exhausts the pool.)
    pub(crate) fn mc2_spawn_fire_spray(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 19;
            e.tick70 = 19;
            e.f140 = 200;
            e.max_life = 240;
            e.flags = (e.flags & !0x2_0008) | 0x2_0000;
        }
        self.link(i, x, y, z);
        self.ent[i].flags |= 1;
        self.refill_life(i);
        self.mc2_set_sprite(i, 228);
        self.mc2_shift_rot(i, 512, 512);
        Some(i)
    }

    /// `NewAdd0A0B_4E840` (EF:35553) — the REAL (10,11): the SCORCH
    /// RING (the volcano-spell ground burn; also the authored
    /// lava-pool decorations). Action 11, maxLife 40, subSpell 200
    /// (→ f140), `word_0x26_38 = 11` (→ f40, the spell-XP row key),
    /// extents (2304, 0x2000), byte[2] |= 2 with bit3 cleared,
    /// INVISIBLE (no sprite), NOT map-registered, no RNG.
    pub(crate) fn mc2_spawn_scorch_ring(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 11;
            e.tick70 = 11;
            e.max_life = 40;
            e.f140 = 200;
            e.f40 = 11;
            e.f26 = 0;
            e.flags = (e.flags & !0x8) | 0x2_0000;
            e.x = x;
            e.y = y;
            e.z = z;
        }
        self.refill_life(i);
        self.mc2_shift_rot(i, 2304, 0x2000);
        Some(i)
    }

    /// `sub_31FB0` (EF:23490) — the (10,11) action-11 tick: radius
    /// grows every 3rd frame; 40-tick life (despawn on expiry or a
    /// class-0 water cell); area burn each tick (full subSpell the
    /// FIRST tick, /25 after — byte[0] bit1 latches); on reaching
    /// the extents cap (f80>>8 − 1) the OUTER ring stamps once;
    /// every tick the disc 0..radius digs −3 (`sub_31F00` ≡
    /// [`Gen::dig_disc_minus3`]); sound 10. The `sub_6D8B0` XP
    /// rows 0x10/0x11 (f40 = 11/15) bank with the 4.2 ledger like
    /// the dome's row 18. Returns terrain-dirty.
    pub(crate) fn mc2_scorch_ring_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].f63 % 3 == 0 {
            self.ent[i].f26 += 1;
        }
        let life = self.ent[i].act_life;
        self.ent[i].act_life -= 1;
        let raw =
            crate::engine::features::tile((self.ent[i].x >> 8) as u8, (self.ent[i].y >> 8) as u8);
        if life < 0 || (1u32 << (self.t.angle[raw] & 0xF)) & 1 != 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        let amt = if self.ent[i].flags & 2 != 0 {
            self.ent[i].f140 / 25
        } else {
            self.ent[i].f140
        } as u32;
        // `sub_116A0` (EF:23513), NOT `sub_10C80` — see the dome's
        // twin in mc2::morph: the variant now decides whether the
        // building footprint pass runs at all.
        let hits = self.area_write(i, 0, amt, ctx, false, true);
        // The scorch-ring batch XP (sub_31FB0 EF:23521-25): f40 (the
        // retail word_0x26_38 stamp) discriminates the owning spell —
        // 15 = Earthquake (17), 11 = Crater (16). Human only (F3).
        if hits != 0 && self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
            let spell = match self.ent[i].f40 {
                15 => Some(17u16),
                11 => Some(16u16),
                _ => None,
            };
            if let Some(sp) = spell {
                self.mc2_cast_xp.0.push((self.ent[i].id24, sp, hits as i32));
            }
        }
        let cap = (self.ent[i].f80 >> 8) as i32;
        let mut r = self.ent[i].f26 as i32;
        if r > cap - 1 {
            r = cap - 1;
            if self.ent[i].flags & 2 == 0 {
                self.dig_disc_minus3(i, cap, cap);
            }
        }
        self.ent[i].flags |= 2;
        self.dig_disc_minus3(i, 0, r);
        self.snd(10, i);
        true
    }

    /// `AddAuxiliary_50500` (EF:36812) — the (10,54) proximity AURA
    /// field: invisible, life 128, ONE RNG draw (random yaw),
    /// `dword_0x10_16 = 12845056` (0xC40000 — the SQUARED range),
    /// extents (1024, 0x4000). Not map-registered.
    pub(crate) fn mc2_spawn_aura(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 54;
            e.tick70 = 0x3B;
            e.max_life = 128;
            e.f126 = 256;
            e.flags &= !8;
            e.f140 = 100;
        }
        let d = self.mc2_rand(i);
        {
            let e = &mut self.ent[i];
            // dword_0x10_16 is homed in f26 as the TILE range; the
            // squared reach is derived in the tick. 14 = the ctor
            // default ((14<<8)² = 12845056); a disposition spawn
            // overrides it from the THING's stageTag (sub_4A310).
            e.f26 = 14;
            e.f30 = (d & 0x7FF) as u16;
            e.x = x;
            e.y = y;
            e.z = z;
            e.flags |= 1;
        }
        self.refill_life(i);
        self.mc2_shift_rot(i, 1024, 0x4000);
        Some(i)
    }

    /// `AddWind_4F040` (EF:35852) + `sub_4F1C0` (EF:35921) — the
    /// (10,22) WHIRLWIND: gated on >= 12 free slots; the head (ONE
    /// RNG draw seeds roll = yaw = pitch) plus 11 tail nodes
    /// (model 75, action 82 — an EV no-op, the head drags them)
    /// chained via word_0x32/word_0x34 (f52/f54), then the sprite
    /// stack: per node row 293+index, quad (550/450 per-mille of the
    /// row's rot_speed), z stacked by 2*roll-extent with the node's
    /// offset in the column scratch f50 (`word_0x36_54`).
    ///
    /// Column scratch f50: head = remembered eye z
    /// (`word_0x30_48`), nodes = the z-stack offset
    /// (`word_0x36_54`), victims = the swirl yaw (`word_0x30_48`) —
    /// disjoint entity sets, one home.
    pub(crate) fn mc2_spawn_whirlwind(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        if self.free.len() < 12 {
            return None;
        }
        let h = self.new_event()?;
        {
            let e = &mut self.ent[h];
            e.class64 = 10;
            e.model65 = 22;
            e.tick70 = 22;
            e.f44 = 0;
            e.f46 = 1; // word_0x2E_46 — the lateral drift sign
            e.f128 = 20;
            e.f130 = 10;
            e.f126 = 50;
            e.max_life = 500;
            e.f140 = 1000; // subSpellIndex — the damage magnitude
            e.flags &= !8;
            e.f56 = 1; // byte_0x38_56 (ch0 enrolment; untargetable anyway)
            e.x = x;
            e.y = y;
            e.z = z;
        }
        let d = self.mc2_rand(h);
        {
            let e = &mut self.ent[h];
            e.f34 = ((d & 0x7FF) as u16).wrapping_sub(1) & 0x7FF; // roll
            e.f30 = e.f34; // yaw
            e.f32 = e.f34; // pitch
        }
        self.refill_life(h);
        let (hx, hy, hz) = (x, y, z);
        let mut prev = h;
        for i in 0..11u16 {
            let Some(c) = self.new_event() else { break };
            // qmemcpy(child, head, 0xA8) — the gameplay fields the
            // node machinery reads, id included (nodes share the
            // head's id).
            {
                let (head_id, head_life, head_rand) =
                    { (self.ent[h].id24, self.ent[h].act_life, self.ent[h].rand) };
                let e = &mut self.ent[c];
                e.class64 = 10;
                e.model65 = 75;
                e.tick70 = 82;
                e.max_life = 500;
                e.act_life = head_life;
                e.id24 = head_id;
                e.rand = head_rand;
                e.flags &= !8;
                e.f44 = i + 1; // word_0x2C_44 — the node index
                e.f52 = prev as u16;
                e.f54 = 0;
                e.f63 = i as u8;
                e.x = hx;
                e.y = hy;
                e.z = hz;
            }
            self.ent[prev].f54 = c as u16;
            self.link(c, hx, hy, hz);
            prev = c;
        }
        self.link(h, hx, hy, hz);
        // sub_4F1C0 — the stacked sprite column.
        let ground = self.ground_z(hx, hy) as i16;
        let mut zoff = 0i32;
        let mut n = h;
        loop {
            let row = self.ent[n].f44 as usize + 293;
            let v5 = SPRITE_PARAMS[row].rot_speed_8 as i32;
            self.mc2_set_sprite(n, row as u16);
            let (shift, roll_ext) = ((550 * v5 / 1000) as u16, (450 * v5 / 1000) as i32);
            self.mc2_shift_rot(n, shift, roll_ext as u16);
            self.ent[n].z = (zoff as i16).wrapping_add(ground);
            self.ent[n].f50 = zoff as i16; // word_0x36_54
            zoff += 2 * roll_ext;
            let next = self.ent[n].f54 as usize;
            if next == 0 {
                break;
            }
            n = next;
        }
        Some(h)
    }

    /// `sub_4EDC0` (EF:35749) — the (10,16) TORNADO-DRAG the summit
    /// vortex (mc2::morph model 18) emits each pulse: subSpell 200,
    /// life 100..199 (RNG 1), launch speed 52..101 (RNG 2), random
    /// heading (RNG 3), vertical impulse 256 (f44 = `word_0x2C_44`),
    /// sprite 210, hover 64 above ground, reclaimable (byte[2] bit
    /// 1), untargetable. Its ACTION is 16 decimal → `sub_32600`, the
    /// ballistic rolling/burning BOULDER — NOT the whirlwind driver:
    /// `0x214110 = sub_33110` belongs to action 0x16 = 22 (dec/hex
    /// trap); the class-10 `strA0` row 0x0010 is `0x213600 =
    /// sub_32600`, EF:1618. The launch impulse is a VELOCITY DELTA in
    /// dest_x/dest_y (`MoveEntity_57FA0` onto the zeroed
    /// `axis_0x9A`, EF:35764-69 — the ball-machinery home), not an
    /// absolute eye point.
    pub(crate) fn mc2_spawn_boulder16(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 16;
            e.tick70 = 16;
            e.f140 = 200;
            e.f44 = 256;
            e.flags = (e.flags & !8) | 0x2_0000;
        }
        let r1 = self.ent_rand(i);
        self.ent[i].max_life = (r1 % 0x64 + 100) as u32;
        let r2 = self.ent_rand(i);
        self.ent[i].f126 = (r2 % 0x32 + 52) as i16;
        let r3 = self.ent_rand(i);
        let yaw = (r3 & 0x7FF) as u16;
        self.ent[i].f30 = yaw;
        self.link(i, x, y, z);
        let gz = (self.ground_z(x, y) + 64) as i16;
        self.ent[i].z = gz;
        let mut d = (0u16, 0u16, 0i16);
        Self::polar_step(&mut d, yaw, 0, self.ent[i].f126);
        self.ent[i].dest_x = d.0;
        self.ent[i].dest_y = d.1;
        self.refill_life(i);
        self.mc2_set_sprite(i, 210);
        Some(i)
    }

    /// `sub_32600` (0x213600, EF:23729-828) — the (10,16) volcano
    /// BOULDER: a ballistic rolling/burning rock. Velocity deltas
    /// ride dest_x/dest_y (clamped ±80/tick), vertical velocity in
    /// f44 (`word_0x2C_44`, gravity −28 clamped [−384, 256]). On
    /// terrain contact it rebounds `vz = −(vz/4)` (trunc), splashes
    /// out on water ((10,5), despawn), lights a `(10,6)` standing
    /// fire (life 30, subSpell ×3 = 150) where none burns, and
    /// settles when vz ≤ 28; resting, it takes the `sub_58030`
    /// terrain-slope push + 250/256 friction (the mana-ball roll
    /// law). NO sound, NO player sway, NO XP (unlike the whirlwind
    /// driver).
    pub(crate) fn mc2_boulder16_tick(&mut self, i: usize) {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        // byte[0] |= 2 (EF:23749-51).
        self.ent[i].flags |= 2;
        let vx = (self.ent[i].dest_x as i16).clamp(-80, 80);
        let vy = (self.ent[i].dest_y as i16).clamp(-80, 80);
        let (x, y, z, vz) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f44 as i16)
        };
        let (px, py) = (x.wrapping_add(vx as u16), y.wrapping_add(vy as u16));
        let mut pz = z.wrapping_add(vz);
        // Gravity AFTER the step, on the old vz (EF:23765-70).
        self.ent[i].f44 = (vz - 28).clamp(-384, 256) as u16;
        let ground = self.ground_z(px, py) as i16;
        if ground > pz {
            pz = ground;
            // Rebound −(vz/4), truncated toward zero (EF:23778).
            let v8 = self.ent[i].f44 as i16 as i32;
            self.ent[i].f44 = (-(v8 / 4)) as i16 as u16;
            // Water (tested at the CURRENT position, EF:23779):
            // (10,5) splash, id inherited, gone — despawn only if
            // the splash actually spawned (pool-full keeps rolling).
            if self.cap_bit(x, y) == 1 {
                let own = self.ent[i].id24;
                if let Some(s) = self.mc2_spawn_splash(px, py, pz) {
                    self.ent[s].id24 = own;
                    self.ent[i].flags |= 0x400;
                    return;
                }
            } else {
                // Light a (10,6) standing fire where none burns
                // (`sub_10B70` cell probe, EF:23790-801): life 30
                // (act only — max stays the ctor's), subSpell ×3.
                let t = crate::engine::features::tile((px >> 8) as u8, (py >> 8) as u8);
                let mut j = self.map_entity[t] as usize;
                let mut burning = false;
                while j != 0 {
                    let e = &self.ent[j];
                    if e.class64 == 10 && e.model65 == 6 && e.flags & 0x400 == 0 {
                        burning = true;
                        break;
                    }
                    j = e.next20 as usize;
                }
                if !burning {
                    let own = self.ent[i].id24;
                    if let Some(f) = self.mc2_spawn_fire6(px, py, pz) {
                        let e = &mut self.ent[f];
                        e.id24 = own;
                        e.act_life = 30;
                        e.f140 *= 3;
                        self.ent[i].f26 = 0; // dword_0x10_16 reset
                    }
                }
                // Settle (EF:23802-03).
                if (self.ent[i].f44 as i16) <= 28 {
                    self.ent[i].f44 = 0;
                }
            }
        }
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1); // dword_0x10_16++
        self.move_relink(i, px, py, pz);
        // Resting on ground: slope push + 250/256 friction (the
        // mana-ball `sub_58030` law, EF:23809-20; trunc division).
        if ground == pz {
            let (tx, ty) = ((px >> 8) as u8, (py >> 8) as u8);
            let h = |dx: u8, dy: u8| {
                self.t.height
                    [crate::engine::features::tile(tx.wrapping_add(dx), ty.wrapping_add(dy))]
                    as i32
            };
            let sx = h(0, 0) - h(1, 0) + h(0, 1) - h(1, 1);
            let sy = h(0, 0) + h(1, 0) - h(0, 1) - h(1, 1);
            let vx = ((vx as i32 + sx) * 250 / 256) as i16;
            let vy = ((vy as i32 + sy) * 250 / 256) as i16;
            self.ent[i].dest_x = vx as u16;
            self.ent[i].dest_y = vy as u16;
        } else {
            self.ent[i].dest_x = vx as u16;
            self.ent[i].dest_y = vy as u16;
        }
    }

    /// `sub_51790` (EF:37439) — the (10,71) expanding FISSURE:
    /// life = maxLife = 120, subSpell 20000, byte[0] = (&0xF6)|1,
    /// map-registered, extents (1280, 2048). No sprite, no RNG.
    pub(crate) fn mc2_spawn_fissure(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 71;
            e.tick70 = 0x4E;
            e.max_life = 120;
            e.act_life = 120;
            e.f140 = 20000;
            e.f71 = 0;
            e.flags = (e.flags & !0x9) | 1;
        }
        self.link(i, x, y, z);
        self.mc2_shift_rot(i, 1280, 2048);
        Some(i)
    }

    /// `AddFireSpheres_4F2A0` (EF:35936) + `sub_4F440` (EF:35989) —
    /// the (10,76) orbiting FIRE-SPHERE ORB
    /// (docs/traces/mc2-class10-m76-fire-spheres.md): gated on >= 26
    /// free slots; one invisible hub (maxLife 80, subSpell 70,
    /// extents 640, action 0x53) + 25 sprite-340 satellites (model
    /// 77, action 0x54 = NO handler — the hub repositions them)
    /// chained via f52/f54, laid out as a 5-ring x 5-slot spherical
    /// lattice (ONE RNG draw per satellite = the 84..147 spin rate).
    /// Only the 5 slot-0 spheres are targetable damage-carriers; the
    /// other 20 are visuals (byte[2] bit7 render flag). The
    /// satellites' `AddEvent2(128,1,0)` children are presentation,
    /// skipped. Runtime-disposition-only in retail (no generate
    /// pass, no par consumption).
    pub(crate) fn mc2_spawn_fire_orb(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        if self.free.len() < 26 {
            return None;
        }
        let h = self.new_event()?;
        {
            let e = &mut self.ent[h];
            e.class64 = 10;
            e.model65 = 76;
            e.tick70 = 0x53;
            e.max_life = 80;
            e.f140 = 70; // subSpellIndex — the per-sphere damage
            e.f126 = 40; // actSpeed
            e.f130 = 192; // maxSpeed — breathe bound A
            e.f128 = 480; // minSpeed — breathe bound B
            e.f56 = 1;
            e.f68 = 0;
            e.f69 = 0;
            e.f44 = 0; // current ring radius
            e.f46 = 0; // fontTypeIndex_0x3D_61 — the breathe step
            e.f71 = 0; // byte_0x46_70 — the phase machine
            e.flags = (e.flags & !0x9) | 1;
            e.x = x;
            e.y = y;
            e.z = z;
        }
        self.refill_life(h);
        let mut prev = h;
        for i in 0..25u8 {
            let Some(s) = self.new_event() else { break };
            {
                // `qmemcpy(entity2, entity, sizeof(type_entity_0x6E8E))`
                // (EF:35967) — the satellite is a FULL STRUCT CLONE of
                // the hub taken at CONSTRUCTION time, so it inherits
                // the hub's actSpeed/minSpeed/maxSpeed triple
                // (EF:35949-52 = 40/480/192) as well as maxLife and
                // subSpell. The loop then overrides only model, action,
                // the 0x32/0x34 links, 0x3E, 0x43 and 0x44.
                // The trace bank's satellite bullet
                // (docs/traces/mc2-class10-m76-fire-spheres.md:89) lists
                // "maxLife 80 / subSpell 70 / extents / byte[0]" and
                // omits the speed words; this ctor was built from that
                // enumeration, so the satellites kept `new_event`'s
                // defaults (+126 = 16, +128 = +130 = 0, features.rs:1723).
                // Measured: mc2l0-spells-galore pair 4397->4398, 25
                // (10,77) rows `speed retail 40 port 16` — the take's
                // entire free-run break (horizon 4397).
                // ⭐ Read the triple from the HUB rather than hardcoding
                // 40/480/192: the caller overrides the hub's maxLife to
                // 30 and subSpell to 180 AFTER this returns, which is
                // itself the proof that the clone happens here and now,
                // and the tier sweep in the take varies these values.
                let (id, rand, life, act_spd, min_spd, max_spd) = {
                    let e = &self.ent[h];
                    (e.id24, e.rand, e.act_life, e.f126, e.f128, e.f130)
                };
                let e = &mut self.ent[s];
                e.class64 = 10;
                e.model65 = 77;
                e.tick70 = 0x54;
                e.max_life = 80;
                e.act_life = life;
                e.id24 = id;
                e.rand = rand;
                e.f126 = act_spd;
                e.f128 = min_spd;
                e.f130 = max_spd;
                e.f140 = 70;
                e.f56 = 1;
                e.flags = (e.flags & !0x9) | 1;
                e.f52 = prev as u16;
                e.f54 = 0;
                e.f63 = i;
                e.f68 = i / 5; // ring
                e.f69 = i % 5; // slot
                e.x = x;
                e.y = y;
                e.z = z;
            }
            self.ent[prev].f54 = s as u16;
            self.link(s, x, y, z);
            prev = s;
        }
        self.link(h, x, y, z);
        self.mc2_shift_rot(h, 640, 640);
        // sub_4F440 — the ring layout.
        {
            let e = &mut self.ent[h];
            e.f46 = 18; // breathe step
            e.f44 = e.f130 as u16; // radius := maxSpeed (192)
            e.f30 = 0;
            e.f32 = 0;
        }
        let mut n = self.ent[h].f54 as usize;
        while n != 0 {
            let slot = self.ent[n].f69;
            self.ent[n].flags &= !1;
            if slot != 0 {
                self.ent[n].flags = (self.ent[n].flags | 0x80_0000) & !8;
            } else {
                self.ent[n].flags |= 8; // the damage carriers
            }
            let d = self.mc2_rand(n);
            let spin = ((d & 0x3F) + 84) as u16;
            let ring = self.ent[n].f68;
            let (yaw, pitch, roll_spin, fov_spin) = match ring {
                0 => ((512 - 96 * slot as i32) as u16 & 0x7FF, 0u16, spin, 0u16),
                1 => (512, (512 - 96 * slot as i32) as u16 & 0x7FF, 0, spin),
                2 => (0, (-96 * slot as i32) as u16 & 0x7FF, 0, spin),
                3 => (256, (256 - 96 * slot as i32) as u16 & 0x7FF, 0, spin),
                _ => (768, (768 - 96 * slot as i32) as u16 & 0x7FF, 0, spin),
            };
            {
                let e = &mut self.ent[n];
                e.f30 = yaw;
                e.f32 = pitch;
                e.f34 = roll_spin;
                e.f36 = fov_spin;
            }
            let radius = self.ent[h].f44 as i16;
            let mut pos = (x, y, z);
            Self::polar_step(&mut pos, yaw, pitch, radius);
            self.move_relink(n, pos.0, pos.1, pos.2);
            self.mc2_set_sprite(n, 340);
            n = self.ent[n].f54 as usize;
        }
        Some(h)
    }

    // ---- ticks ---------------------------------------------------------------

    /// `sub_339B0` (EF:24562) — the orb hub tick: phase 0 init sizes
    /// the ring from the LEADER's extents when f146 carries one
    /// (`maxSpeed = pitch>>1` floored at 128, `minSpeed = 6*pitch>>2`
    /// capped at 640 — EF:24581-90; a wizard's 121 pitch gives the
    /// tight [128,181] shell, a castle brain's 128·w+640 flips the
    /// bounds INVERTED so the breathe hard-snaps across up to
    /// [640,3392]) → phase 1 pulse: snap to the leader + collapse on
    /// its death, terrain clamp (z >= ground + radius — `sub_33C70`),
    /// the ±18 radius breathe (`sub_33AD0`), the constellation
    /// tumble (+22/+16 head spin, per-sphere spin, all 25
    /// repositioned — `sub_33B20`), the slot-0 damage pass
    /// (`sub_10C80(type 0, 70)` per carrier, sound 3 on any hit —
    /// `sub_33C00`); life out → phase 2 collapse: keep tumbling,
    /// radius -= |step|, and at < 0 spawn a (10,0) ground fire and
    /// tear the whole 26-entity chain down (`sub_33D40`).
    ///
    /// The leader is the impact seam's struck victim (proj.rs
    /// (10,76) arm) — trace §2's "dead code" call is REFUTED
    /// (adjudication in docs/traces/mc2-class10-m76-fire-spheres.md
    /// §7): retail's `sub_65B50` (EF:63029) pins the hub via the
    /// charged fireball. An authored map-THING orb keeps f146 = 0
    /// and behaves exactly as before.
    pub(crate) fn mc2_fire_orb_tick(&mut self, i: usize, ctx: &MobCtx) {
        if self.ent[i].f71 == 0 {
            // Phase-0 leader sizing (EF:24581-90): ring bounds from
            // the victim's AABB half-extent. The human wizard lives
            // outside the pool — its extents are the sprite-44
            // derivation (`SetEntityIndexAndRot(44)`: pitch = s6/2),
            // same law the pool wizards get at spawn.
            let leader = self.ent[i].f146;
            let pitch = match leader {
                0 => None,
                PLAYER_TARGET => Some(self.mc2_params_ext(44).0 / 2),
                v => ((v as usize) < self.ent.len()).then(|| self.ent[v as usize].f80),
            };
            if let Some(p) = pitch {
                let e = &mut self.ent[i];
                e.f130 = ((p as i32) >> 1).max(128) as i16;
                e.f128 = ((6 * p as i32) >> 2).min(640) as i16;
            }
            self.ent[i].f71 = 1;
        } else if self.ent[i].f71 > 1 {
            if self.ent[i].f71 == 2 {
                if self.ent[i].f46 < 0 {
                    self.ent[i].f46 = -self.ent[i].f46;
                }
                self.mc2_orb_tumble(i);
                let v7 = self.ent[i].f44 as i16 - self.ent[i].f46;
                self.ent[i].f44 = v7 as u16;
                if v7 < 0 {
                    let (x, y, z) = {
                        let e = &self.ent[i];
                        (e.x, e.y, e.z)
                    };
                    self.mc2_spawn_fire(x, y, z);
                    let mut n = i;
                    loop {
                        self.ent[n].flags |= 0x400;
                        let next = self.ent[n].f54 as usize;
                        if next == 0 || next == n {
                            break;
                        }
                        n = next;
                    }
                }
            }
            return;
        }
        // Phase 1 — `sub_33C70` order: leader snap FIRST, then the
        // terrain/ceiling clamps, then the leader-death collapse
        // (EF:24726-45). The snap rides the leader's position plus
        // its `array_0x52_82.yaw` z-offset (f78; wizard = 100), so
        // an airborne victim wears the orb 100 units overhead while
        // a castle's huge radius lets the ground clamp win and the
        // sphere balloons over the footprint.
        let leader = self.ent[i].f146;
        let mut leader_dead = false;
        if leader != 0 {
            if leader == PLAYER_TARGET {
                let off = (self.mc2_params_ext(44).1 / 2) as i16;
                self.move_relink(i, ctx.px, ctx.py, ctx.pz.wrapping_add(off));
                leader_dead = ctx.pdead;
            } else if (leader as usize) < self.ent.len() {
                let v = leader as usize;
                let (vx, vy, vz, dead) = {
                    let t = &self.ent[v];
                    (
                        t.x,
                        t.y,
                        t.z.wrapping_add(t.f78 as i16),
                        t.act_life < 0 || t.flags & 0x400 != 0,
                    )
                };
                self.move_relink(i, vx, vy, vz);
                leader_dead = dead;
            }
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let floor = (self.ground_z(x, y) as i16).wrapping_add(self.ent[i].f44 as i16);
        if self.ent[i].z < floor {
            self.ent[i].z = floor;
        }
        // Cave ceiling clamp, margin = the RADIUS not fov
        // (EF:24751-58: ceiling − word_0x2C_44).
        if self.is_cave() {
            let c = (self.ceiling_z(x, y) as i16).wrapping_sub(self.ent[i].f44 as i16);
            if self.ent[i].z > c {
                self.ent[i].z = c;
            }
        }
        // Leader dead → collapse, set at `sub_33C70`'s tail
        // (EF:24743-45): the rest of THIS tick still pulses; the
        // next tick enters phase 2.
        if leader_dead {
            self.ent[i].f71 = 2;
        }
        // sub_33AD0 — the breathe bounce.
        {
            let e = &mut self.ent[i];
            let v2 = e.f46 + e.f44 as i16;
            let (lo, hi) = (e.f128 as i16, e.f130 as i16);
            e.f44 = v2 as u16;
            if v2 <= lo {
                if v2 < hi {
                    e.f44 = hi as u16;
                    e.f46 = -e.f46;
                }
            } else {
                e.f44 = lo as u16;
                e.f46 = -e.f46;
            }
        }
        self.mc2_orb_tumble(i);
        // sub_33C00 — the slot-0 damage pass. The hit sound fires
        // PER CARRIER inside the loop (EF:24710-14), not once for
        // the volley.
        let amt = self.ent[i].f140 as u32;
        let mut n = self.ent[i].f54 as usize;
        while n != 0 {
            if self.ent[n].f69 == 0 && self.area_write(n, 0, amt, ctx, false, false) != 0 {
                self.snd(3, i);
            }
            n = self.ent[n].f54 as usize;
        }
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < 1 {
            self.ent[i].f71 = 2;
        }
    }

    /// `sub_33B20` (EF:24656) — the constellation tumble: the hub
    /// spins +22 yaw / +16 pitch, each satellite advances its own
    /// spin rates, and every sphere is re-placed at hub + spherical
    /// (satAngle + hubAngle, radius). No RNG.
    fn mc2_orb_tumble(&mut self, i: usize) {
        {
            let e = &mut self.ent[i];
            e.f30 = e.f30.wrapping_add(22) & 0x7FF;
            e.f32 = e.f32.wrapping_add(16) & 0x7FF;
        }
        let (hx, hy, hz, hyaw, hpitch, radius) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.f44 as i16)
        };
        let mut n = self.ent[i].f54 as usize;
        while n != 0 {
            {
                let e = &mut self.ent[n];
                e.f30 = e.f30.wrapping_add(e.f34) & 0x7FF;
                e.f32 = e.f32.wrapping_add(e.f36) & 0x7FF;
            }
            let (syaw, spitch) = (self.ent[n].f30, self.ent[n].f32);
            let mut pos = (hx, hy, hz);
            Self::polar_step(
                &mut pos,
                syaw.wrapping_add(hyaw) & 0x7FF,
                spitch.wrapping_add(hpitch) & 0x7FF,
                radius,
            );
            self.move_relink(n, pos.0, pos.1, pos.2);
            n = self.ent[n].f54 as usize;
        }
    }

    /// `sub_3A2D0` (EF:29443) — the (10,71) fissure tick
    /// (docs/traces/mc2-class10-tail-helper-closure.md §2): phase 0
    /// init (`word_0x2C_44 = maxLife/8`, per-beat damage =
    /// 4*(20000/120) ≈ 664); each tick the disc radius ramps
    /// grow → pin-at-3*ref (with a 1-in-5 phase-jump roll) → shrink,
    /// clamped [0,15], and every cell of the disc takes a **±1
    /// heightmap jitter** (sign = life & 1 — the ground vibrates; no
    /// terrain-type write, no children); a `byte_0x46_70 > 1` tick
    /// adds a half-radius inner pass; `byte > 3` = the terminal
    /// tail-off (life only). Every 4th tick: sprite quad grows to
    /// the radius, sound 10, the type-0 area beat (the id-0xF
    /// spellbook report is emitted by the spell-XP column).
    pub(crate) fn mc2_fissure_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].f71 == 0 {
            let maxl = self.ent[i].max_life as i32;
            self.ent[i].f44 = (maxl >> 3) as u16; // word_0x2C_44
            self.ent[i].f26 = 0;
            self.ent[i].f71 = 1;
            self.ent[i].f140 = 4 * (self.ent[i].f140 / maxl.max(1) as i32);
        }
        let mut dirty = false;
        if self.ent[i].f71 <= 3 {
            let v4 = self.ent[i].f44 as i32;
            let maxl = self.ent[i].max_life as i32;
            let life = self.ent[i].act_life;
            let mut v6 = if maxl - 3 * v4 >= life as i32 {
                if maxl - 5 * v4 > life as i32 {
                    self.ent[i].f26 -= 1;
                    self.ent[i].f26 as i32
                } else {
                    let d = self.mc2_rand(i);
                    if d % 5 == 0 {
                        self.ent[i].f71 += 2;
                    }
                    3 * v4
                }
            } else {
                self.ent[i].f26 += 1;
                self.ent[i].f26 as i32
            };
            v6 = v6.clamp(0, 3 * v4).clamp(0, 15);
            let second_pass = self.ent[i].f71 > 1;
            if second_pass {
                self.ent[i].f71 -= 1;
            }
            if v6 > 0 {
                // Cell center rounds `(pos + 128) >> 8` (EF:29527-28).
                let (cx, cy) = (
                    (self.ent[i].x.wrapping_add(128) >> 8) as i16,
                    (self.ent[i].y.wrapping_add(128) >> 8) as i16,
                );
                let sign: i16 = if self.ent[i].act_life & 1 == 1 { 1 } else { -1 };
                for r in [Some(v6), second_pass.then_some(v6 >> 1)]
                    .into_iter()
                    .flatten()
                {
                    for (dx, dy) in self.ring_cells(0, r) {
                        let t = crate::engine::features::tile(
                            (cx.wrapping_add((dx as i8) as i16)) as u8,
                            (cy.wrapping_add((dy as i8) as i16)) as u8,
                        );
                        let v = (self.t.height[t] as i16 + sign).clamp(0, 255);
                        self.t.height[t] = v as u8;
                    }
                }
                dirty = true;
                if self.ent[i].act_life & 3 == 0 {
                    self.mc2_shift_rot(i, (v6 << 8) as u16, 2048);
                    self.snd(10, i);
                    let amt = self.ent[i].f140 as u32;
                    let hits = self.area_write(i, 0, amt, ctx, false, false);
                    // Tremor batch XP (sub_3A2D0 EF:29580).
                    if hits != 0 && self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
                        self.mc2_cast_xp.0.push((self.ent[i].id24, 15, hits as i32));
                    }
                }
            }
        }
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < 0 {
            self.ent[i].flags |= 0x400;
        }
        dirty
    }

    /// `sub_33110` (EF:24155) — the whirlwind driver: while alive,
    /// wander + drag (`sub_331A0`), the lift-and-throw pass
    /// (`sub_33340`), the every-8th-tick contact pass (`sub_33710`),
    /// loop sound 49; on expiry the teardown (`sub_338D0`) clears
    /// the grabs and despawns the 12-node chain.
    pub(crate) fn mc2_whirlwind_tick(&mut self, i: usize, ctx: &MobCtx) {
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < 0 {
            self.mc2_whirlwind_teardown(i);
            return;
        }
        self.mc2_whirlwind_move(i);
        self.mc2_whirlwind_lift(i, ctx);
        self.mc2_whirlwind_contact(i);
        self.snd(49, i);
    }

    /// `sub_331A0` (EF:24177) — head wander (roll drift flips sign
    /// on a coin every 16 ticks, 32-unit lateral wobble → the eye
    /// center, +341 yaw and 120 forward, ground-clamped) + the tail
    /// drag (each node pulled toward its predecessor to the gap
    /// `72 - 4*(12 - index)`, z = head z + the node's f50 offset).
    /// The eye xy rides f142/f144-free scratch: we keep it in the
    /// head's dest fields (the portal column's home, unused here
    /// otherwise) — `axis_0x9A_154x`.
    fn mc2_whirlwind_move(&mut self, i: usize) {
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        self.ent[i].f50 = z; // word_0x30_48 — remembered eye z
        self.ent[i].f63 = self.ent[i].f63.wrapping_add(1);
        if self.ent[i].f63 & 0xF == 0 {
            let d = self.mc2_rand(i);
            if d & 1 == 0 {
                self.ent[i].f46 = -self.ent[i].f46;
            }
        }
        let roll = (self.ent[i].f34 as i32 + 11 * self.ent[i].f46 as i32) as u16 & 0x7FF;
        self.ent[i].f34 = roll;
        let mut eye = (x, y, z);
        Self::polar_step(&mut eye, roll, 0, 32);
        self.ent[i].dest_x = eye.0;
        self.ent[i].dest_y = eye.1;
        let yaw = self.ent[i].f30.wrapping_add(341) & 0x7FF;
        self.ent[i].f30 = yaw;
        let mut pos = eye;
        Self::polar_step(&mut pos, yaw, 0, 120);
        let ground = self.ground_z(pos.0, pos.1) as i16;
        self.move_relink(i, pos.0, pos.1, ground);
        // Tail drag.
        let head_z = ground;
        let mut prev = i;
        let mut n = self.ent[i].f54 as usize;
        while n != 0 {
            let (nx, ny, nz) = {
                let e = &self.ent[n];
                (e.x, e.y, e.z)
            };
            let (px, py, pz) = {
                let e = &self.ent[prev];
                (e.x, e.y, e.z)
            };
            let yaw = Self::angle_between(nx, ny, px, py);
            self.ent[n].f30 = yaw;
            // 2-D: retail's `EuclideanDistXYZ_58490` (EF:24213)
            // never reads z — with the permanent per-node z offset
            // a 3-D read overshoots the gap and bunches the tail.
            let dh2 = Self::dist2_sq(nx, ny, px, py);
            let _ = pz;
            let d = Self::isqrt(dh2 as u32) as i32;
            let gap = 72 - 4 * (12 - self.ent[n].f44 as i32);
            let mut pos = (nx, ny, nz);
            if d > gap {
                Self::polar_step(&mut pos, yaw, 0, (d - gap) as i16);
            }
            let zoff = self.ent[n].f50;
            pos.2 = zoff.wrapping_add(head_z);
            self.move_relink(n, pos.0, pos.1, pos.2);
            prev = n;
            n = self.ent[n].f54 as usize;
        }
    }

    /// `sub_33340` (EF:24229) — the lift-and-throw pass over the
    /// radius-12 tile disc around the eye: pool CREATURES swirl
    /// inward (yaw = bearing+591, drift 96), lift near the eye
    /// (+114/tick above it, GRAB latched past the 768+rand%768
    /// threshold), spin at yaw-step 204 while grabbed, release past
    /// the far ring (d² >= 5308416), and take the head's 1000
    /// mailbox damage every airborne tick (`sub_11900`). The
    /// spellbook report (id 0x15) is emitted by the spell-XP column.
    ///
    /// Deliberate approximations (cited):
    /// - the HUMAN player arm (yaw-step 56, threshold 384, camera
    ///   roll crank, actSpeed 80) needs the FlightVerb takeover seam
    ///   (the level-end cinematic's seam) — until then the player is
    ///   damaged when overlapping the eye ring but not lifted;
    /// - the victim z-float band (`sub_580E0` row args) collapses to
    ///   the computed lift z (the row hover clamp needs the behavior
    ///   rows' word_0xa/0xc homes).
    ///
    /// The victim filter is `sub_33810` VERBATIM (EF:24452-515):
    /// class-2 m7/8; class-3 non-castle, non-own (the ONLY owner
    /// check retail makes); class-5 minus actions {232,180} and
    /// models {10,15,18,27,28}; class-10 {13,14,39,57}.
    fn mc2_whirlwind_lift(&mut self, i: usize, ctx: &MobCtx) {
        let (ex, ey, eye_z, id, amt) = {
            let e = &self.ent[i];
            (e.dest_x, e.dest_y, e.f50, e.id24, e.f140 as u32)
        };
        // Cell center rounds: `(pos + 128) >> 8` (EF:29527-28 fissure,
        // :24273-74 lift, :24531-32 teardown; truncation would shift
        // the disc half a tile on the high side).
        let (cx, cy) = (
            (self.ent[i].x.wrapping_add(128) >> 8) as i16,
            (self.ent[i].y.wrapping_add(128) >> 8) as i16,
        );
        let mut hits = 0u32;
        for (dx, dy) in self.ring_cells(0, 12) {
            let tx = (cx.wrapping_add((dx as i8) as i16)) as u8;
            let ty = (cy.wrapping_add((dy as i8) as i16)) as u8;
            let mut j = self.map_entity[crate::engine::features::tile(tx, ty)] as usize;
            while j != 0 {
                let next = self.ent[j].next20 as usize;
                let c = &self.ent[j];
                let victim = match c.class64 {
                    2 => matches!(c.model65, 7 | 8),
                    3 => c.id24 != id && c.model65 != 2,
                    5 => {
                        !matches!(c.tick70, 232 | 180)
                            && !matches!(c.model65, 10 | 15 | 18 | 27 | 28)
                    }
                    10 => matches!(c.model65, 13 | 14 | 39 | 57),
                    _ => false,
                };
                // The 0x400 reap-skip is a guard not in the retail
                // gate (deliberate: avoids acting on reaped slots).
                if !victim || c.flags & 0x400 != 0 {
                    j = next;
                    continue;
                }
                let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as i64;
                let grabbed = c.flags & F_GRABBED != 0;
                let (vx, vy, vz) = (c.x, c.y, c.z);
                let mut pos = (vx, vy, vz);
                let mut drift = 0i16;
                let mut airborne = false;
                if d2 >= 3_211_264 {
                    if grabbed {
                        self.ent[j].flags |= super::mobs::F_STOP;
                        airborne = true;
                        drift = 64;
                        self.ent[j].f30 = self.ent[j].f30.wrapping_add(204) & 0x7FF;
                        if d2 >= 5_308_416 {
                            self.ent[j].flags &= !F_GRABBED; // FLUNG
                        }
                    }
                } else {
                    let bearing = Self::angle_between(ex, ey, vx, vy);
                    if grabbed {
                        self.ent[j].flags |= super::mobs::F_STOP;
                        drift = 128;
                        airborne = true;
                        pos.2 = pos.2.wrapping_add(114);
                        self.ent[j].f30 = self.ent[j].f30.wrapping_add(204) & 0x7FF;
                    } else if d2 >= 0x40000 {
                        // Mid ring: swirl inward.
                        let v14 = bearing.wrapping_add(591) & 0x7FF;
                        self.ent[j].f50 = v14 as i16;
                        self.ent[j].f30 = v14;
                        drift = 96;
                    } else {
                        // Inner ring: the lift.
                        self.ent[j].flags |= super::mobs::F_STOP;
                        pos.0 = ex;
                        pos.1 = ey;
                        let v9 = vz as i32 - eye_z as i32 + 57;
                        let galt = self.ground_z(ex, ey) as i16;
                        pos.2 = ((v9 + galt as i32).max(galt as i32)) as i16;
                        self.ent[j].f30 = self.ent[j].f30.wrapping_add(204) & 0x7FF;
                        let d = self.ent_rand(j);
                        if v9 >= 768 + (d % 768) as i32 {
                            self.ent[j].flags |= F_GRABBED;
                            self.ent[j].f50 = self.ent[j].f30 as i16;
                        }
                    }
                }
                if drift != 0 {
                    let swirl = self.ent[j].f50 as u16 & 0x7FF;
                    Self::polar_step(&mut pos, swirl, 0, drift);
                }
                if pos != (vx, vy, vz) {
                    // Cave ceiling clamp on the thrown victim
                    // (EF:24382-88), before the commit.
                    if self.is_cave() {
                        let c = (self.ceiling_z(pos.0, pos.1) as i16 as i32
                            - self.ent[j].f84 as i32) as i16;
                        if pos.2 > c {
                            pos.2 = c;
                        }
                    }
                    self.move_relink(j, pos.0, pos.1, pos.2);
                }
                if airborne {
                    hits += 1;
                    self.mail_write(MailTarget::Pool(j), 0, amt, id);
                }
                j = next;
            }
        }
        // The player arm — the tornado SWAY (retail `sub_33340`'s
        // wizard branch, EF:24296: the human [class 3, model 0] is
        // swirled at yaw-step 56 and dragged toward the eye). The full
        // grab / lift / camera-roll takeover is the deferred FlightVerb
        // seam; the observable "the funnel drags you in" rides the
        // `player_knock` channel like the flood shove — a pull toward
        // the eye bent ~45° tangentially so it spirals inward rather
        // than sucking straight through (deliberate approximation).
        // Retail's whirlwind sways the wizard, it does not chip HP.
        // Same-owner gate (`sub_33810` case 1, EF:24473: `a2x->id ==
        // a1x->id → return 0`) — your OWN whirlwind never sways you.
        let pd = Self::isqrt(Self::dist2_sq(ex, ey, ctx.px, ctx.py) as u32) as i32;
        if pd < 3328 && id != crate::mc1::mobs::PLAYER_TARGET {
            let toward = Self::angle_between(ctx.px, ctx.py, ex, ey);
            let dir = (toward as i32 + 256) as u16 & 0x7FF; // +45° spiral bias
            // Stronger closer (0..128 across the funnel), clamped to
            // the knock channel's band and never overshooting the eye.
            let mag = ((((3328 - pd) << 8) / 3328) << 7 >> 8).clamp(8, 80).min(pd);
            self.player_knock = (dir, mag as i16);
            // THE HEADING. Retail's victim block writes `yaw_0x1C_28`
            // on EVERY arm, and the wizard's step is `v38` = 56 —
            // `v40 = (class == 3 && !model)` picks 56 over the 204
            // creatures get (EF:24294-99), and the same 56 lands in
            // the far-grab, near-grab and inner-lift arms alike. Only
            // the MID RING (`d2 >= 0x40000`, not yet grabbed) sets an
            // absolute heading instead: the tangent `bearing + 591`
            // (EF:24350-56), which is what turns a straight fall
            // toward the eye into the spiral. The port shoved the
            // flyer and never touched its facing, so a tornado threw
            // you around while you kept staring the way you came in.
            //
            // The grab/lift/camera-roll takeover is still the
            // deferred FlightVerb seam — the spin rides the pose
            // channel on its own, which is what the report is about.
            let d2 = Self::dist2_sq(ex, ey, ctx.px, ctx.py) as i64;
            self.player_spin.0 = if d2 >= 0x40000 {
                let tangent = Self::angle_between(ex, ey, ctx.px, ctx.py).wrapping_add(591) & 0x7FF;
                // Absolute in retail; delivered as the delta that
                // reaches it, since the pose channel carries turns.
                (tangent as i16 - (ctx.pyaw & 0x7FF) as i16).rem_euclid(2048)
            } else {
                56
            };
        }
        // The grab-pass batch XP (sub_33340 EF:24407).
        if hits != 0 && id == crate::mc1::mobs::PLAYER_TARGET {
            self.mc2_cast_xp.0.push((id, 21, hits as i32));
        }
    }

    /// `sub_33710` (EF:24416) — the every-8th-tick CONTACT pass
    /// against the list builder (EF:39964-40075): `dword_38527` is
    /// the class-10
    /// MODEL-45 list ⇒ pass 1 mails overlapping village BUILDINGS
    /// (sub_11900 ch0, EF:24428-24430 — no owner gate); pass 2 =
    /// CASTLES (the class-3 list, model 2): the 30-tick shake
    /// (word_0x30_48 → f50), owner stamp (word_0x26_38 → f40) and
    /// the subSpell mail — also ungated (your own castle takes it).
    /// Overlap = CompareAxisWithShift_10750 (XY-only — the shared
    /// [`Gen::mc2_overlap_xy`]). The `sub_6D8B0(id, 0x15, 2n)`
    /// report is emitted by the spell-XP column.
    fn mc2_whirlwind_contact(&mut self, i: usize) {
        if self.ent[i].f63 & 7 != 0 {
            return;
        }
        let (id, amt) = (self.ent[i].id24, self.ent[i].f140 as u32);
        let mut hits: Vec<(usize, bool)> = Vec::new();
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if j == i || c.flags & 0x400 != 0 {
                continue;
            }
            let castle = c.class64 == 3 && c.model65 == 2 && c.act_life >= 0;
            let building = c.class64 == 10 && c.model65 == 45;
            if (castle || building) && self.mc2_overlap_xy(i, j) {
                hits.push((j, castle));
            }
        }
        let mut castles_hit = 0i32;
        for (j, castle) in hits {
            if castle {
                self.ent[j].f50 = 30;
                self.ent[j].f40 = i as u16;
                castles_hit += 1;
            }
            self.mail_write(MailTarget::Pool(j), 0, amt, id);
        }
        // The contact-pass batch XP: +2 per CASTLE struck
        // (sub_33710 EF:24444, `v1 += 2` per castle).
        if castles_hit != 0 && id == crate::mc1::mobs::PLAYER_TARGET {
            self.mc2_cast_xp.0.push((id, 21, 2 * castles_hit));
        }
    }

    /// `sub_338D0` (EF:24518) — teardown: clear every nearby
    /// victim's grab/stop latches over the radius-12 disc, end the
    /// wind loop (sound 49 stops with the emitter), despawn the head
    /// and all 11 nodes down the f54 chain.
    fn mc2_whirlwind_teardown(&mut self, i: usize) {
        // Cell center rounds: `(pos + 128) >> 8` (EF:29527-28 fissure,
        // :24273-74 lift, :24531-32 teardown; truncation would shift
        // the disc half a tile on the high side).
        let (cx, cy) = (
            (self.ent[i].x.wrapping_add(128) >> 8) as i16,
            (self.ent[i].y.wrapping_add(128) >> 8) as i16,
        );
        for (dx, dy) in self.ring_cells(0, 12) {
            let tx = (cx.wrapping_add((dx as i8) as i16)) as u8;
            let ty = (cy.wrapping_add((dy as i8) as i16)) as u8;
            let mut j = self.map_entity[crate::engine::features::tile(tx, ty)] as usize;
            while j != 0 {
                let next = self.ent[j].next20 as usize;
                self.ent[j].flags &= !(F_GRABBED | super::mobs::F_STOP);
                j = next;
            }
        }
        let mut n = i;
        loop {
            self.ent[n].flags |= 0x400;
            let next = self.ent[n].f54 as usize;
            if next == 0 || next == n {
                break;
            }
            n = next;
        }
    }

    /// `sub_33E20` (EF:24817) — the (10,25) tick: life-- /f26++;
    /// while alive, ONE latched `sub_10C80(type 3, byte_0x46_70)`
    /// burst (the amount is the par-set f71, NOT subSpell); a hit
    /// zeroes life (despawn next tick).
    pub(crate) fn mc2_blast25_tick(&mut self, i: usize, ctx: &MobCtx) {
        let life = self.ent[i].act_life - 1;
        self.ent[i].f26 += 1;
        self.ent[i].act_life = life;
        if life >= 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                let amt = self.ent[i].f71 as u32;
                if self.area_write(i, 3, amt, ctx, false, false) != 0 {
                    self.ent[i].act_life = 0;
                }
            }
        } else {
            self.ent[i].flags |= 0x400;
        }
    }

    /// `sub_33D80` (EF:24787) — the (10,23) tick: ONE latched
    /// `sub_10C80(type 0, 25)` burst + sound 24, then life pinned to
    /// 1 (one more visible tick). The `sub_6D8B0(id, 7, hits)`
    /// spellbook report is emitted by the spell-XP column.
    pub(crate) fn mc2_blast23_tick(&mut self, i: usize, ctx: &MobCtx) {
        // `v1 = life; dword_0x10_16++; life = v1-1; if (v1 >= 0)` —
        // the OLD life gates, and f26 counts up EVERY tick
        // (EF:24789-94; a post-test runs one tick short).
        let old_life = self.ent[i].act_life;
        self.ent[i].f26 += 1;
        self.ent[i].act_life = old_life - 1;
        if old_life >= 0 {
            if self.ent[i].flags & 2 == 0 {
                let amt = self.ent[i].f140 as u32;
                let hits = self.area_write(i, 0, amt, ctx, false, false);
                // Lightning burst batch XP (EF:24802).
                if hits != 0 && self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
                    self.mc2_cast_xp.0.push((self.ent[i].id24, 7, hits as i32));
                }
                self.snd(24, i);
                self.ent[i].act_life = 1;
                self.ent[i].flags |= 2;
            }
        } else {
            self.ent[i].flags |= 0x400;
        }
    }

    /// `sub_32880` (EF:23834) — the (10,17) meteor tick: sound 30 +
    /// the once-latch (dword |= 0x10002) on the first tick; the quad
    /// grows with the ring counter (`ShiftRot((768*f26 - 5*sign)>>2,
    /// 512)`); `sub_10C80(type 0, subSpell/maxLife)` = 300/tick (the
    /// kind-9 spellbook report is emitted by the spell-XP column);
    /// then ONE RING of
    /// (10,0) fire children at ring f26 — jittered (2 RNG each, cell
    /// pitch 160), id+yaw inherited, `dword |= 0x10080` (byte[0]
    /// bit7 + byte[2] bit0 — the children are DAMAGE-SUPPRESSED
    /// visuals, the fire tick's 0x1_0000 gate), quad (512,512); the
    /// ring cycles `(f26+2) % 11`.
    pub(crate) fn mc2_meteor_tick(&mut self, i: usize, ctx: &MobCtx) {
        let life = self.ent[i].act_life - 1;
        self.ent[i].act_life = life;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2 | 0x1_0000;
            self.snd(30, i);
        }
        let ring = self.ent[i].f26 as i32;
        let grown = 768 * ring;
        // ⚠ `my_sign32` is −1/0, NEVER +1 (engine_support.cpp:2962) —
        // so EF:23864's `- my_sign32(768*ring) * 5` is a no-op on the
        // ONLY branch the ring counter ever takes (it cycles 0..10 via
        // `(f26+2) % 11`) and an ADD of 5 on the negative one. Reading
        // it as a signum cost the quad 2 units per ring step: mc2l3
        // t=1341 slot 163 at ring 2 wants 768*2 >> 2 = 384, and the
        // spurious −5 published 382 in BOTH the apitch and aroll lanes.
        let shift = (grown - 5 * if grown < 0 { -1 } else { 0 }) >> 2;
        self.mc2_shift_rot(i, shift as u16, 512);
        let amt = (self.ent[i].f140 / self.ent[i].max_life as i32) as u32;
        let hits = self.area_write(i, 0, amt, ctx, false, false);
        // Meteor batch XP (sub_32880 EF:23871).
        if hits != 0 && self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
            self.mc2_cast_xp.0.push((self.ent[i].id24, 9, hits as i32));
        }
        let (px, py, pz, id, yaw) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30)
        };
        for (dx, dy) in self.ring_cells(ring, ring) {
            let d = self.ent_rand(i);
            let nx = (px as i32 - 96 + 160 * (dx as i8) as i32 + (d % 0x81) as i32 - 64) as u16;
            let d = self.ent_rand(i);
            let ny = ((d % 0x81) as i32 + 160 * (dy as i8) as i32 + py as i32 - 96 - 64) as u16;
            if let Some(c) = self.mc2_spawn_fire(nx, ny, pz) {
                {
                    let e = &mut self.ent[c];
                    e.id24 = id;
                    e.f30 = yaw;
                    e.flags |= 0x1_0080;
                    e.f26 = 0;
                }
                self.mc2_shift_rot(c, 512, 512);
            }
        }
        self.ent[i].f26 = ((ring + 2) % 11) as i16;
    }

    /// `sub_32530` (EF:23694) — the (10,15) fire-trail tick: the
    /// water counter (`sub_104A0 & 1` → f26++, else --), death on
    /// life < -1 OR 8 accumulated water ticks; ONE RNG wander
    /// (yaw += r%0x5B - 45), advance 256, drop a (10,11→19) spray
    /// (fov copied, life 10, word_0x26_38 = 15, id inherited).
    pub(crate) fn mc2_fire_trail_tick(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        if self.on_water(x, y) {
            self.ent[i].f26 += 1;
        } else if self.ent[i].f26 > 0 {
            self.ent[i].f26 -= 1;
        }
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < -1 || self.ent[i].f26 > 8 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let d = self.mc2_rand(i);
        let yaw = ((d % 0x5B) as i32 + self.ent[i].f30 as i32 - 45) as u16 & 0x7FF;
        self.ent[i].f30 = yaw;
        let mut pos = (x, y, self.ent[i].z);
        Self::polar_step(&mut pos, yaw, 0, 256);
        {
            let e = &mut self.ent[i];
            e.x = pos.0;
            e.y = pos.1;
            e.z = pos.2;
        }
        let (pitch, roll, fov, id) = {
            let e = &self.ent[i];
            (e.f80, e.f82, e.f84, e.id24)
        };
        // The trail lays a child SCORCH RING (10,11) — the earth-CARVE
        // (`sub_31FB0` digs the disc −3), NOT the (10,19) ground-fire
        // SPRAY. The spray is a fire effect that itself spews (10,14)
        // smoke puffs every odd tick, so a trail dropping one per tick
        // over its 128-life would exhaust the pool. Same
        // (10,11)-vs-(10,19) confusion as the cave column
        // (docs/spell-audit/quake-family.md §Earthquake). f40=15 keys
        // the ring's Earthquake-XP branch.
        if let Some(s) = self.mc2_spawn_scorch_ring(pos.0, pos.1, pos.2) {
            let e = &mut self.ent[s];
            // All THREE pose fields copy (EF:23719-21) — the ring
            // tick reads f80 (pitch>>8) for its carve radius (f84
            // alone digs the default disc, not the trail's radius 3).
            e.f80 = pitch;
            e.f82 = roll;
            e.f84 = fov;
            e.act_life = 10;
            e.f40 = 15; // word_0x26_38 → Earthquake XP row
            e.id24 = id;
        }
    }

    /// `sub_32F40` (EF:24095) — the (10,19) ground-fire-spray tick:
    /// while alive, walk the radius-0 splat TEMPLATE — retail loops
    /// `AddE7EE0x_10080(0, 0)` = ring 0 (4 cells, last dropped as the
    /// stop code → 3 emission cells; EF:24112-40), NOT a single center
    /// cell. For EACH cell: a ~50% gate roll, two jitter rolls (offset
    /// by the cell's `192 * (dx, dy)`), and on ODD life ticks a 4-puff
    /// ring of (10,14) smoke (yaw start `(life/2 & 1) << 8`, step 0x200
    /// to 0x800, id inherited); z snaps to terrain. On death, release
    /// the word_0x33 singleton (`plume`, latched by morph.rs's
    /// summit-18). `sub_10C80(ch0, 200)` EVERY tick including the
    /// despawn tick. The single-cell port formerly under-produced the
    /// column's smoke by ~3x — the volcano (10,14) missing family.
    pub(crate) fn mc2_fire_spray_tick(&mut self, i: usize, ctx: &MobCtx) {
        let life = self.ent[i].act_life;
        self.ent[i].act_life -= 1;
        if life >= 0 {
            self.ent[i].f26 = 0;
            let (px, py, pz, id) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z, e.id24)
            };
            let odd = self.ent[i].act_life & 1 == 1;
            let v10_start = ((self.ent[i].act_life / 2) & 1) << 8;
            for (dx, dy) in self.ring_cells(0, 0) {
                let d = self.ent_rand(i);
                if 2 * ((d % 0x9D) as i32 / 79) - 1 <= 0 {
                    continue;
                }
                let d = self.ent_rand(i);
                let jx = (px as i32 - 96 + 192 * (dx as i8) as i32 + (d % 0x81) as i32 - 64) as u16;
                let d = self.ent_rand(i);
                let jy = (py as i32 - 96 + 192 * (dy as i8) as i32 + (d % 0x81) as i32 - 64) as u16;
                if odd {
                    let mut v10 = v10_start;
                    while v10 < 0x800 {
                        if let Some(p) = self.mc2_spawn_smoke_particle_for(14, jx, jy, pz) {
                            self.ent[p].id24 = id;
                            self.ent[p].f30 = v10 as u16;
                        }
                        v10 += 0x200;
                    }
                }
            }
            // Frozen z under strict (conformance replay): the
            // pristine-plane heightfield lacks the runtime-raised
            // summit, so `ground_z` returns the un-erupted baseline
            // while retail's `getTerrainAlt` is the raised plateau
            // (== the imported z for the whole recording). Re-snapping
            // drops the column ~624 below retail (mc2l24 (10,19) slot
            // 181: port 2000 vs retail 2624) and pulls its (10,14)
            // smoke down with it. Same frozen-z law as
            // `mc2_summit18_tick`; native keeps the exact retail
            // re-snap to its own (real) heightfield.
            // Frozen z under strict (conformance replay): the
            // pristine-plane heightfield lacks the runtime-raised
            // summit, so `ground_z` returns the un-erupted baseline
            // while retail's `getTerrainAlt` is the raised plateau
            // (== the imported z for the whole recording). Re-snapping
            // drops the column ~624 below retail (mc2l24 (10,19) slot
            // 181: port 2000 vs retail 2624) and pulls its (10,14)
            // smoke down with it. Same frozen-z law as
            // `mc2_summit18_tick`; native keeps the exact retail
            // re-snap to its own (real) heightfield.
            if !ctx.strict {
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                self.ent[i].z = self.ground_z(x, y) as i16;
            }
        } else {
            self.ent[i].flags |= 0x400;
            // `D41A0_0.word_0x33 = 0` (EF:24148) — release the spray
            // singleton (latched by the summit-18 eruption,
            // morph.rs). Unconditional like retail: the latch kills
            // the previous spray on re-latch, so at most one is ever
            // alive. WITHOUT this, a stale `plume` outlives the
            // spray, and the next eruption's "kill the previous
            // column" write lands on whatever entity RE-USED the
            // slot — a silent arbitrary kill.
            self.plume = 0;
        }
        let amt = self.ent[i].f140 as u32;
        self.area_write(i, 0, amt, ctx, false, false);
    }

    /// `sub_38D80` (EF:28349) — the (10,54) MANA-MAGNET aura (retail
    /// `AddAuxiliary_50500`, `dword_0x10_16 = 0xC40000`): life-- (< 0
    /// → despawn), then over the SQUARED range 0xC40000 (≈14 tiles)
    /// drag every unowned MANA SPHERE toward the eye. Retail stamps
    /// each sphere a homing target (`word_0x7A_122` = this aura) +
    /// pull speed (`word_0x76_118 = min(dist, 42)`) which the ball
    /// tick (EF:26369) flies in, merging coincident balls into one.
    ///
    /// ⚠ The SCAN SOURCE is retail's (the tick-top `dword_38523`
    /// chain — see the loop). What is still collapsed is the WRITE:
    /// [`Self::ball_tick`] already consumes `dest_x/dest_y` as a
    /// decaying drift AND merges overlapping balls, so the aura writes
    /// the pull velocity onto each ball's dest exactly like
    /// [`Self::magnet_tick`] instead of stamping retail's speed word
    /// and letting the sphere derive its own heading (deliberate —
    /// and note that it is why the port never writes the sphere's
    /// `yaw`, which retail's own consume-side does).
    /// Only the TARGET half (`word_0x7A_122`) keeps a
    /// field home of its own, the aura claim map, because retail's
    /// handshake is load-bearing on BOTH sides: the aura re-stamps
    /// EVERY tick (`if (!w7A)`, EF:28364) and the sphere clears the
    /// stamp at the head of its own tick (EF:26109), latching the
    /// `v35` kick that drags a sphere whose settle counter has run
    /// out. Claim in, claim out, once per tick — a claim that
    /// outlives its tick silently retires the sphere from the aura's
    /// scan for good.
    pub(crate) fn mc2_aura_tick(&mut self, i: usize) {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        // dword_0x10_16 = (tile range << 8)² — f26 holds the tile
        // range (ctor default 14; disposition spawn overrides it from
        // the THING's stageTag, sub_4A310).
        let r = (self.ent[i].f26 as i32) << 8;
        let range_sq = r * r;
        let (ax, ay) = (self.ent[i].x, self.ent[i].y);
        // ⭐⭐⭐ THE SCAN IS `dword_38523`, THE TICK-TOP SPHERE CHAIN —
        // NOT THE LIVE POOL (EF:28362). This was a registered
        // approximation ("a pool slot-order list standing in for
        // retail's `dword_38523` list") until mc2l3 t=9816 demanded
        // it, and **membership sampled ONCE AT THE TOP is the whole
        // point**: a sphere born MID-TICK is not in this tick's chain,
        // so retail cannot pull it until the NEXT tick.
        //
        // mc2l3 t=9816 is that tick exactly — the player's cast borns
        // a (10,54) aura plus eleven (10,39) spheres in one frame, and
        // the aura dispatches at its own ascending slot afterwards.
        // Retail's newborn spheres sit still (`dest` 0/0, `yaw` 0);
        // the port's pool walk saw them immediately and pulled them a
        // tick early, so **the port's whole sphere grid was one tick
        // ahead of retail's forever after** — measured: the port's
        // t=9816 x/y/dest_x/dest_y for slot 170 ARE retail's t=9817
        // values. Same family as the ball chain's own law (mc1l4
        // t=5377, the mid-tick ball the tick-top scan cannot hold).
        //
        // ⚠ The chain's membership is retail's verbatim (models 39,
        // 40, 57 for MC2), but the model filter below is KEPT as it
        // was: retail's `sub_38D80` has no model test and therefore
        // pulls the (10,40) claim totem too, which the port has never
        // done. That is a separate pre-existing residual, and changing
        // the scan source and the pulled set in one step would make
        // neither attributable.
        for k in 0..self.ball_chain.visible_len() {
            let j = self.ball_chain.list[k] as usize;
            let c = &self.ent[j];
            if c.class64 != 10 || !matches!(c.model65, 39 | 57) || c.flags & 0x400 != 0 {
                continue;
            }
            // The claim handshake (EF:28364): only an UNCLAIMED ball
            // takes the pull; the ball's tick clears the claim after
            // consuming it. First aura in slot order keeps the ball.
            if self.mc2_aura_claim.0.contains_key(&(j as u16)) {
                continue;
            }
            let d2 = Self::dist2_sq(ax, ay, c.x, c.y);
            if d2 >= range_sq {
                continue;
            }
            self.mc2_aura_claim.0.insert(j as u16, i as u16);
            // Pull speed = min(linear distance, 42) — retail's radix_3d
            // cap; it eases to 0 at the eye so the merged ball settles.
            let speed = (Self::isqrt(d2 as u32).min(42)) as i32;
            // `angle_of` returns 0..=2048 (2048 = the full-turn wrap);
            // mask to the table's 0..2047 like `advance` does, or a
            // ball at the exact diagonal panics SIN[2048] (len 2048).
            let dir = (Self::angle_between(c.x, c.y, ax, ay) & 0x7FF) as usize;
            let vx = ((speed * crate::mc1::tables::SIN[dir]) >> 16) as i16;
            let vy = (-((speed * crate::mc1::tables::COS[dir]) >> 16)) as i16;
            self.ent[j].dest_x = vx as u16;
            self.ent[j].dest_y = vy as u16;
        }
    }
}
