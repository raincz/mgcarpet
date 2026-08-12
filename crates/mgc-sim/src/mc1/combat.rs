//! MC1 combat: damage mailboxes, class-9 projectiles, class-10
//! combat effects (fire/explosion, fire-spreader, splash, blast ring,
//! hit-flash, mana-steal flash, mana ball) and the corpse pipeline —
//! ports of remc1 sub_main.cpp. Full specs in docs/ROADMAP.md
//! ("Combat, damage, death & corpses", "Fireball / repeat fireball").
//!
//! Deviations from the decompile:
//! - `sub_12B50`'s inverted accumulate/overwrite is NOT ported; the
//!   direct write uses the area writers' protocol (:17301-05)
//!   (deliberate: suspect transcription swap, like :21814).
//! - The m9 ranged thunk aims at the TARGET, not the atan2(0,0)
//!   self-aim (:21947-48) (deliberate: decompile casualty).
//! - Aim assist scores candidates by angular miss (Δyaw² + Δpitch²)
//!   with a distance tiebreak (deliberate approximation of sub_54A90's
//!   squared-miss-distance metric; exact port OPEN for the CREATURE
//!   cones — the possess acquisition runs the exact metric, see
//!   `aim_assist_possess_mc1`).
//! - The m9 lightning BEAM (sub_535E0 :63272) is a full port (one-tick
//!   hitscan walk + state-14 segment chain, confirmed vs remc2
//!   sub_66750); the explosion's +146 stamps hit-or-0 where the
//!   original writes garbage on a miss (deliberate).
//! - Class-9 model 14 / state 15 (the Troll & Ape boulder): remc1's
//!   class-9 tick table is truncated, but CARPET.EXE's relocated
//!   table binds 0xF to the bare sub_52770 thunk — the boulder runs
//!   the generic homing flight (no fire trail, no acquire for m14),
//!   pre-targeted by its throw ctor. It must NOT alias onto state
//!   13, whose first-tick roll is the arrow quartet.
//! - Mana-shield reflection (+17 bit 7) is ported but nothing sets the
//!   flag yet (OPEN: wizard shields are the spell track).

use crate::engine::features::{Gen, lcg32, tile};
use crate::mc1::behavior::BEHAVIOR;
use crate::mc1::mobs::{MobCtx, PLAYER_TARGET};
use crate::mc1::sprite_stats::SPRITE_STATS;
use crate::verbs::{CorpseVerb, TargetingVerb, VerbKind};

/// `MGC_NO_BALL_MERGE_FIX=1` restores BOTH pre-dig halves of the
/// mana-sphere merge — the whole-pool partner scan (instead of
/// retail's `sub_11D10`/`sub_10A50` map-tile ring walk) and the MC2
/// soft-kill of the absorbed donor (instead of retail's hard
/// `sub_57F20` free) — so one binary can be A/B'd. Read once: the
/// value is a whole-process arm, never a per-run input.
fn no_ball_merge_fix() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_BALL_MERGE_FIX").is_some())
}

/// The player carpet's half-extents (sprite 44 stats halves — the
/// same constants the trigger/portal overlap uses).
pub(crate) const PLAYER_HW: i32 = (SPRITE_STATS[44].width / 2) as i32;
pub(crate) const PLAYER_HH: i32 = (SPRITE_STATS[44].height / 2) as i32;

/// Candidate set of the pure crosshair preview — the sub_54520
/// subtype blocks the player's own spells can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AimPreviewSet {
    /// Blocks 0/3/4 + the beam's one-shot snap: awake creatures +
    /// rival wizards (fireball, meteor, volcano, lightning).
    Creatures,
    /// Block 1: unowned mana balls + houses (possess).
    Possess,
    /// Blocks 7/8/B/C: rival wizards only (duel, steal, undead).
    Wizards,
}

/// A mailbox recipient: a pool event or the out-of-pool player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MailTarget {
    Pool(usize),
    Player,
}

/// The inbox verdict a state handler dispatches on (hitflag 0/1/2).
pub(crate) enum Inbox {
    Quiet,
    Hit(u16),
    Dead,
}

/// Which Rebound deflection arm a bolt family carries. sub_52B30's
/// fireball arm (:62847-88) deflects ANY impact pair with the ±45
/// scatter, and an unaffordable deflection flies straight through
/// untouched. sub_52770's generic arm (:62705-50) deflects only the
/// (10,1)/(10,17) impact descriptors — Hidden Worlds inserts (10,53),
/// the homing meteor's pair, the single compare distinguishing the two
/// shipped binaries' handlers (CARPET.EXE 0x528B6-C3 vs HIDDEN.EXE
/// 0x52BF9-0x52C03) — with the ±22 scatter, and ANY refusal (pair gate
/// or mana) lands as a plain hit on the deflector. Wall of Fire's bolt
/// carries (10,53), so it punches through Rebound in base MC1 (the
/// anti-griffon spell) while the HW meteor deflects.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DeflectLaw {
    Fireball,
    Generic,
}

impl Gen {
    // ---- mailbox writes ---------------------------------------------------

    /// The shared write protocol (:17301-05): accumulate while a
    /// source is pending, overwrite a stale amount (readers clear the
    /// source but never the amount).
    pub(crate) fn mail_write(&mut self, tgt: MailTarget, ch: usize, amt: u32, src: u16) {
        let m = match tgt {
            MailTarget::Pool(i) => &mut self.ent[i].mail[ch],
            MailTarget::Player => &mut self.player_mail[ch],
        };
        if m.1 != 0 {
            m.0 = m.0.wrapping_add(amt);
        } else {
            m.0 = amt;
        }
        m.1 = src;
    }

    /// sub_118C0 (:16963) between two pool events: extents SUM per
    /// axis, z centered by each half-height (+78). +78 is SIGNED —
    /// the castle's 0xE000 z-center marker (sub_37150 :43798) means
    /// −8192, not 57344; widening it unsigned orphans every castle
    /// out of the z test.
    pub(crate) fn ent_overlap(&self, a: usize, b: usize) -> bool {
        let (ea, eb) = (&self.ent[a], &self.ent[b]);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        wd(ea.x, eb.x) < ea.f80 as i32 + eb.f80 as i32
            && wd(ea.y, eb.y) < ea.f82 as i32 + eb.f82 as i32
            && ((ea.z as i32 + ea.f78 as i16 as i32) - (eb.z as i32 + eb.f78 as i16 as i32)).abs()
                < ea.f84 as i32 + eb.f84 as i32
    }

    /// sub_118C0 against the player carpet.
    pub(crate) fn player_overlap(&self, i: usize, ctx: &MobCtx) -> bool {
        let e = &self.ent[i];
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        wd(e.x, ctx.px) < e.f80 as i32 + PLAYER_HW
            && wd(e.y, ctx.py) < e.f82 as i32 + PLAYER_HW
            && ((e.z as i32 + e.f78 as i16 as i32) - (ctx.pz as i32 + PLAYER_HH)).abs()
                < e.f84 as i32 + PLAYER_HH
    }

    /// The writer's +66/+67 target filter (-1/-1 = wildcard).
    pub(crate) fn filter_admits(f66: u8, f67: u8, class: u8, model: u8) -> bool {
        (f66 == 0xFF || f66 == class) && (f67 == 0xFF || f67 == model)
    }

    /// sub_120B0 (:17235) / sub_124F0 (:17399) / sub_127E0 (:17502):
    /// the channel-N area write around event `i`. Gates per
    /// candidate: owner immunity (+24 equality — the engine's only
    /// friendly-fire rule), the damageable flag (+16&8), the
    /// vulnerability mask (+28 bit ch), the writer's +66/+67 filter,
    /// AABB overlap; the tile scan skips class-3 model 2 (:17372) —
    /// castles get their own ch0 pre-pass instead (:17325-34): every
    /// overlapping castle on ANOTHER team takes the mail (this is
    /// how mob-death fire cells fell castles), and under the 127E0
    /// variant (`shake`) EVERY castle in range — own included — arms
    /// its 30-tick blast-shake repaint (:17522). `building_tenth` =
    /// the 124F0 variant where class-2 model-0 TREES take amt/10
    /// (:17465 — the discount that keeps area spells from vaporizing
    /// forests; village buildings are class-10 m45 and take full
    /// amounts).
    /// Returns the number of mails written (retail's sub_124F0-family
    /// and MC2's sub_10C80/sub_116A0 return the hit count — the
    /// spellbook reports and the (10,9) earthquake gate consume it;
    /// MC1 callers ignore it).
    pub(crate) fn area_write(
        &mut self,
        i: usize,
        ch: usize,
        amt: u32,
        ctx: &MobCtx,
        building_tenth: bool,
        shake: bool,
    ) -> u32 {
        let mut count = 0u32;
        let (wx, wy, id, f66, f67) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24, e.f66, e.f67)
        };
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        // Does this call run MC2's BUILDING FOOTPRINT pass (below)?
        // It is `sub_10C80`'s ch0 arm alone — and where it runs, the
        // tile scan must skip (10,45) so the two never double up.
        let fp_pass = ch == 0 && !shake && mc2;
        // The castle pre-pass (ch0 only).
        if ch == 0 {
            let mut hits: Vec<usize> = Vec::new();
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 == 3
                    && c.model65 == 2
                    && c.flags & 0x400 == 0
                    && j != i
                    && self.ent_overlap(i, j)
                {
                    hits.push(j);
                }
            }
            for j in hits {
                if shake {
                    self.ent[j].f50 = 30;
                }
                if self.ent[j].id24 != id {
                    self.mail_write(MailTarget::Pool(j), 0, amt, id);
                    count += 1;
                }
            }
        }
        // ---- MC2 PASS 2: THE BUILDING FOOTPRINT LIST ---------------
        //
        // `sub_10C80`'s ch0 arm runs THREE passes, not two: between
        // the castle list and the tile scan sits a walk of
        // `dword_38527` — the (10,45) BUILDING list (built at
        // EF:40043-52, the `model <= 0x2D` arm) — at EF:4076-4105.
        // Every building whose 2-D box the writer overlaps gets its
        // BUILD00 footprint mask sampled at the WRITER's tile, and a
        // solid cell takes the mail. `CompareAxisWithShift_10750`
        // (EF:3733) is `ent_overlap` MINUS the z term, so this is
        // [`Gen::mc2_overlap_xy`], and the pass has **no owner
        // immunity, no damageable flag, no vulnerability mask, no
        // +66/+67 filter and no z test** — "damage registers anywhere
        // within the perimeter", literally. `sub_116A0` (the shake
        // variant) has no such pass, which is why this is gated on
        // `!shake`; MC1's `sub_120B0` has none either.
        //
        // Without it a building was reachable only through the tile
        // chain, where it is linked at its ANCHOR alone
        // (`AddEventToMap_57D70` EF:40313 single-links, exactly like
        // our `Gen::link` — the multi-link theory is refuted). A
        // ground fire's 3×3 window at the anchor reaches 4 of the
        // main tower's 2,024 footprint cells; retail lands all 2,024.
        // The "damage snaps to the flag" report is the anchor hit
        // being the port's ONLY hit — the snap itself is faithful.
        //
        // ⚠ The mask row is BUILD00, not the sprite table remc2
        // guessed: the raw expression is `**filearray[24] + 6*idx +
        // 4`, a 6-byte TAB record with w at +4 and h at +5, and the
        // building ctor `sub_49A30` (EF:32765) reads the same row
        // through `filearrayindex_BUILD00DATTAB`. remc2 marks the
        // block `//fix it` ×3 and transcribes the top-left from the
        // WRITER, which would pin the index to the mask's centre cell
        // forever and make the parity bump meaningless; the corner
        // belongs to the BUILDING, whose ctor computes it with this
        // exact expression and bump (EF:32780-88). remc2's
        // resolution halving is part of the same misreading (a
        // sprite-table adaptation) — and the port's un-halved extents
        // already reproduce the recorded retail ones.
        if fp_pass {
            let (wtx, wty) = ((wx >> 8) as u8, (wy >> 8) as u8);
            let mut hits: Vec<usize> = Vec::new();
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 != 10 || c.model65 != 45 || c.flags & 0x400 != 0 {
                    continue;
                }
                if !self.mc2_overlap_xy(i, j) {
                    continue;
                }
                let Some(def) = self.assets.build_tab.get(c.f71 as usize).copied() else {
                    continue;
                };
                if def.w == 0 {
                    continue;
                }
                // The building's own top-left, its ctor's expression
                // verbatim. Idempotent here: the ctor already shifted
                // the building one tile +x to make the sum even, so
                // recomputing from the stored position re-derives the
                // bumped corner and the bump is a no-op — which is
                // the corroboration that this corner is the
                // building's and not the writer's.
                let mut tlx = ((c.x >> 8) as u8).wrapping_sub(def.w / 2);
                let tly = ((c.y >> 8) as u8).wrapping_sub(def.h / 2);
                if tlx.wrapping_add(tly) & 1 != 0 {
                    tlx = tlx.wrapping_add(1);
                }
                // Cell offsets stay SIGNED (retail's are plain ints):
                // a writer left of or above the corner indexes
                // backwards out of the row, and a writer past the
                // right edge spills into the next one. Both are
                // retail reads into the neighbouring BUILD00 bytes,
                // reproduced as far as the blob reaches — off the
                // blob we decline rather than invent a byte.
                let dx = wtx.wrapping_sub(tlx) as i8 as i32;
                let dy = wty.wrapping_sub(tly) as i8 as i32;
                let off = def.offset as i64 + 2 * (dx + dy * def.w as i32) as i64;
                if off < 0 {
                    continue;
                }
                if let Some(&cell) = self.assets.build_dat.get(off as usize) {
                    if cell != 0xFF {
                        hits.push(j);
                    }
                }
            }
            for j in hits {
                self.mail_write(MailTarget::Pool(j), 0, amt, id);
                count += 1;
            }
        }
        // THE SCAN RADIUS. Retail is the x half-extent rounded UP,
        // `(+80 + 255) >> 8`, in every variant of both games (MC1
        // sub_120B0 :17267 and :17342, sub_124F0 :17431, sub_127E0
        // :17539; MC2 sub_10C80 EF:3995/4032/4120) — the `__CFSHL__`
        // / `my_sign32` fixups wrapped around them are DEAD, the
        // extent field is uint16 so the sum never goes negative.
        //
        // ⚠ The `.max(1)` is OURS and it is NOT retail: a zero-extent
        // writer runs `for i = -0; i <= 0` there and scans its OWN
        // TILE ALONE, where we hand it a 3x3. HELD BACK deliberately
        // 2026-08-12, with the projectile-probe geometry it belongs
        // to — see docs/CONFORMANCE-FINDINGS.md §THE HELD-BACK AREA
        // FIXES. Measured: removing it buys NOTHING on the corpus
        // (mc1l0 whole take identical to the tick) and it breaks the
        // MC2 arrow's direct hit, because the port's compensating
        // window inflation and its anti-tunnel chord march are one
        // family and have to come out together.
        let r = ((self.ent[i].f80 as i32 + 255) >> 8).max(1);
        // Pass 2 OWNS the buildings, so the tile scan must not also
        // find them: `&& (class != 10 || model != 45)` sits at
        // EF:4135 right beside the castle exclusion, and for the same
        // reason. Only in the variant that runs pass 2 — `sub_116A0`
        // carries neither, and MC1 has neither.
        let mut victims: Vec<(usize, u32)> = Vec::new();
        // THE WINDOW CENTRE, AND IT IS NOT THE SAME ON CHANNEL 0.
        // Channels 1+ round to the NEAREST tile, `(pos + 128) >> 8`
        // (MC1 sub_120B0 :17260-72, MC2 sub_10C80 EF:3995-96) — that
        // rounding is what the l0 t=91 tent claim needs, its flash at
        // y=70.63 sweeping tile row 73.
        //
        // MC1's ch0 arm biases the window one tile BACK instead:
        // `(pos - 128) / 256`, a TRUNCATING divide (the `__CFSHL__`
        // idiom is signed division by 256, and `pos - 128` does go
        // negative in the first half-tile of the map, where it
        // truncates to 0 rather than −1). Byte-identical in all three
        // MC1 variants — sub_120B0 :17339/17352, sub_124F0
        // :17427/17439, sub_127E0 :17535/17547 — so it is the law,
        // not a decompiler artifact of one function.
        //
        // MC2 does NOT do this: `sub_10C80`'s ch0 arm centres on
        // `(pos + 128) >> 8` like every other channel (EF:4118-19).
        // The `my_sign32` fixup written around it there is dead for
        // the same reason as the radius fixups — `axis_3d::x` is
        // uint16, so `x + 128` is never negative.
        let centre = |p: u16| -> i32 {
            if ch == 0 && !mc2 {
                (p as i32 - 128) / 256
            } else {
                (p as i32 + 128) >> 8
            }
        };
        let (ctx_, cty_) = (centre(wx), centre(wy));
        for dy in -r..=r {
            for dx in -r..=r {
                let tx = (ctx_ + dx) as u8;
                let ty = (cty_ + dy) as u8;
                let mut j = self.map_entity[tile(tx, ty)] as usize;
                while j != 0 {
                    let c = &self.ent[j];
                    let next = c.next20 as usize;
                    if c.id24 != id
                        && c.flags & 8 != 0
                        && c.f28 & (1 << ch) != 0
                        && Self::filter_admits(f66, f67, c.class64, c.model65)
                        && !(ch == 0 && c.class64 == 3 && c.model65 == 2)
                        && !(fp_pass && c.class64 == 10 && c.model65 == 45)
                        && self.ent_overlap(i, j)
                    {
                        let a = if building_tenth && c.class64 == 2 && c.model65 == 0 {
                            amt / 10
                        } else {
                            amt
                        };
                        victims.push((j, a));
                    }
                    j = next;
                }
            }
        }
        for (j, a) in victims {
            self.mail_write(MailTarget::Pool(j), ch, a, id);
            count += 1;
        }
        // The player probe (the human wizard is outside the pool; the
        // original reaches it through the same grid).
        if id != PLAYER_TARGET && Self::filter_admits(f66, f67, 3, 0) && self.player_overlap(i, ctx)
        {
            self.mail_write(MailTarget::Player, ch, amt, id);
            count += 1;
        }
        count
    }

    // ---- the creature inbox (the block opening every state handler) -------

    /// :21330-67: apply pending ch0 damage (awake only), inherit the
    /// weakest body segment's life, latch attacker (+40) and killer
    /// (+38), and report the hitflag.
    pub(crate) fn inbox(&mut self, i: usize) -> Inbox {
        let mut hit = 0u8;
        if self.ent[i].f58 != 0 {
            if self.ent[i].mail[0].1 != 0 {
                let (amt, src) = self.ent[i].mail[0];
                self.ent[i].act_life -= amt as i32;
                self.ent[i].mail[0].1 = 0; // amount stays stale (:21337)
                self.ent[i].f40 = src;
                hit = 1;
            } else {
                self.ent[i].f40 = 0;
            }
            let mut s = self.ent[i].f54 as usize;
            while s != 0 {
                if self.ent[s].act_life < self.ent[i].act_life {
                    self.ent[i].act_life = self.ent[s].act_life;
                    self.ent[i].f40 = self.ent[s].f40;
                    hit = 1;
                    break;
                }
                s = self.ent[s].f54 as usize;
            }
        }
        if self.ent[i].act_life < 0 {
            hit = 2;
            // The killer latch belongs to the LETHAL branch alone
            // (:21365-66 / :21485-86 / :21631-32 / :21737-38).
            self.ent[i].f38 = self.ent[i].f40;
        }
        match hit {
            1 => Inbox::Hit(self.ent[i].f40),
            2 => Inbox::Dead,
            _ => Inbox::Quiet,
        }
    }

    /// Aggro test on a mailbox source: only class-3 (wizard-family)
    /// attackers provoke a chase (:21370-76).
    pub(crate) fn attacker_is_wizard(&self, src: u16) -> bool {
        if src == PLAYER_TARGET {
            return true;
        }
        let s = src as usize;
        s != 0 && s < self.ent.len() && self.ent[s].class64 == 3
    }

    // ---- class-9 projectiles ----------------------------------------------

    /// The shared class-9 init shape (str_255870 :4463): 8.8 position,
    /// not hittable (+16 &= ~8), refilled life, sprite-derived extents.
    /// `speed`/`life`/`row`/`sprite` per the model column; state = the
    /// model's flight state.
    #[allow(clippy::too_many_arguments)]
    fn spawn_projectile(
        &mut self,
        model: u8,
        state: u8,
        x: u16,
        y: u16,
        z: i16,
        speed: i16,
        life: u32,
        row: u8,
        sprite: u16,
    ) -> Option<usize> {
        let p = self.new_event()?;
        {
            let e = &mut self.ent[p];
            e.class64 = 9;
            e.model65 = model;
            e.tick70 = state;
            e.f126 = speed;
            e.f128 = speed;
            e.max_life = life;
            e.f140 = 50;
            e.row156 = row;
            e.flags &= !8;
        }
        self.link(p, x, y, z);
        self.refill_life(p);
        self.set_sprite(p, sprite);
        Some(p)
    }

    /// sub_39A10 (:45861): the fireball. Base speed 384, life 21
    /// ticks, homing row [5] (thunks override), sprite 42.
    pub(crate) fn spawn_fireball(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(0, 0, x, y, z, 384, 21, 5, 42)
    }

    /// sub_39BC0 (:45954): the m3 trail bolt (meteor). Row [1].
    pub(crate) fn spawn_trail_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(3, 3, x, y, z, 384, 21, 1, 76)
    }

    /// sub_39E40 (:46104): the m8 wizard-seeker. Row [4] (yaw 0x100).
    pub(crate) fn spawn_seeker(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(8, 8, x, y, z, 384, 21, 4, 214)
    }

    /// sub_39EC0 (:46135): the m9 zigzag lightning. Life 9.
    pub(crate) fn spawn_zigzag(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(9, 9, x, y, z, 384, 9, 4, 216)
    }

    /// sub_3A0C0 (:46256): the m13 straight bolt. Life 13, default
    /// row/damage (NewEvent's +44 = 100 unless the thunk overrides).
    pub(crate) fn spawn_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let p = self.spawn_projectile(13, 13, x, y, z, 384, 13, 0, 195)?;
        // The ctor's sprite call is the DOUBLING setter (:46274), not
        // the plain one every other class-9 ctor uses — the arrow
        // carries twice the collision half-extents (44/44/60 rather
        // than 22/22/30 for its 45x60 row).
        self.set_sprite_x2(p, 195);
        Some(p)
    }

    /// sub_3A390 (:46392): the m18 GLOBAL DEATH fuse. Fireball-shaped
    /// ctor (speed 384, life 0x2000/384 = 21, row [5], sprite 42) but
    /// state 19 sits past remc1's transcribed class-9 table. Observed
    /// retail behavior: never a bolt — fire once, wait, the blast lands
    /// AROUND THE CASTER. Reconstructed as a caster-anchored fuse: 21
    /// ticks tracking the caster, then the generic +44-copying
    /// detonation into the (10,55) field at the caster's position
    /// (deliberate reconstruction). The ctor's speed/aim/+150 target
    /// are carried but unused; the +26 charge byte (spawner moves the
    /// wizard's accumulated charge into it) stays unmodeled — role
    /// unknown. OPEN: retail may allow MULTIPLE overlapping charges,
    /// each detonating on its own delay; our cast gate (the row's
    /// 101-tick burst counter, decompile-consistent) blocks recast ~4s.
    pub(crate) fn spawn_bomb_fuse(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(18, 19, x, y, z, 384, 21, 5, 42)
    }

    /// State 19: the Global Death fuse tick — ride the caster, burn
    /// the 21-tick life, detonate in place (see spawn_bomb_fuse).
    fn bomb_fuse_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
            self.move_relink(i, ctx.px, ctx.py, ctx.pz);
        }
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < 0 {
            self.proj_explode(i, ctx, None, true, false);
        }
        false
    }

    /// sub_3A1A0 (:46281): m7's slow bolt — state 15, the generic
    /// flight (see the dispatch note at [`Gen::proj_tick`]).
    pub(crate) fn spawn_slow_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(14, 15, x, y, z, 128, 32, 0, 196)
    }

    /// The player-spell payload projectiles (c9 m1 possess / m2
    /// earthquake / m4 volcano / m5 crater / m7 duel / m11 undead /
    /// m17 magnet): fireball-shaped init, state = model, dispatched
    /// to [`Gen::proj_payload_tick`] — except the MAGNET bolt (m17),
    /// which runs possession's state-1 flight: its ctor writes state
    /// 18 (:46371), past remc1's 14-entry class-9 state table, and
    /// the m1 flight is the behavior-matched stand-in. Inside it the
    /// m17 bolt diverges from possession twice, both decompile-
    /// corroborated: NO acquisition (sub_54520 has no model-17 case,
    /// default return 0 :64185 — it flies straight) and the
    /// model-39-ONLY contact scan (sub_11C00 :17083, not possession's
    /// 39/40/45 sub_11AC0).
    /// Sprites per the class-9 rows in `mc1_entities` — the magnet
    /// bolt shares possession's sprite 209 (both ctors call
    /// sub_36FA0(entity, 209), :45916/:46384: distinct models, one
    /// look). APPROX(original: each model's own flight state past
    /// remc1's transcribed table).
    pub(crate) fn spawn_spell_lob(&mut self, model: u8, x: u16, y: u16, z: i16) -> Option<usize> {
        let sprite = match model {
            1 | 17 => 209,
            2 => 211,
            4 => 210,
            5 => 211,
            7 => 213,
            11 => 281,
            _ => return None,
        };
        let state = if model == 17 { 1 } else { model };
        // The possess lob's ctor is the family's ONE short fuse:
        // sub_39A90 (:45900-16) computes life 4096/speed = 10 where
        // every sibling (:45861..:46135) takes 0x2000/speed = 21 —
        // corpus-pinned (mc1l0 pair 63: retail lob 9/10 vs the port's
        // fireball-shaped 20/21; the doubled range overshoots every
        // close-in possess target).
        let life = if model == 1 { 10 } else { 21 };
        self.spawn_projectile(model, state, x, y, z, 384, life, 0, sprite)
    }

    /// Vertical bearing (sub_42180 :52644): the pitch whose polar step
    /// descends from `fz` toward `tz` over horizontal distance `dh`.
    pub(crate) fn pitch_toward(fz: i16, tz: i16, dh: i32) -> u16 {
        Self::angle_of(fz.wrapping_sub(tz), (-(dh.clamp(0, 0x7FFF))) as i16)
    }

    /// Aim a fresh projectile from an attacker at a target point
    /// (sub_42150/42180 pair) and stamp the combat fields the thunks
    /// share: owner, filter, homing target, damage, explosion.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn arm_projectile(
        &mut self,
        p: usize,
        owner: u16,
        f66: u8,
        f67: u8,
        target: u16,
        tx: u16,
        ty: u16,
        tz: i16,
        f44: u16,
        expl_model: u8,
    ) {
        let (px, py, pz) = (self.ent[p].x, self.ent[p].y, self.ent[p].z);
        let yaw = Self::angle_between(px, py, tx, ty);
        let dh = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
        let pitch = Self::pitch_toward(pz, tz, dh);
        let e = &mut self.ent[p];
        e.id24 = owner;
        e.f66 = f66;
        e.f67 = f67;
        e.f146 = target;
        e.f30 = yaw;
        e.f34 = yaw;
        e.f32 = pitch;
        e.f36 = pitch;
        e.f44 = f44;
        e.f68 = 10;
        e.f69 = expl_model;
    }

    /// The TargetingVerb seam (crate::verbs) — the acquire subtypes
    /// dispatch here. MC2's own acquire column lives in mc2::mobs;
    /// this dispatcher is only reached from MC1-spell paths, where an
    /// MC2 world serves the MC1 scan and notes the fallback (the
    /// pinned frankenstein ledger).
    fn aim_assist(&mut self, i: usize, ctx: &MobCtx) {
        match self.verbs.targeting {
            TargetingVerb::Mc1 | TargetingVerb::Mc1Hw => self.aim_assist_mc1(i, ctx),
            TargetingVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Targeting);
                self.aim_assist_mc1(i, ctx);
            }
        }
    }

    /// Is this the Hidden Worlds engine? HW's entire live sim delta is
    /// the original's single compiled `IsHiddenWord` bool (two branches:
    /// the model-16 homing meteor and the napalm-geometry fork). We
    /// carry it as the one HW-distinct verb — the targeting column —
    /// rather than a parallel flag; every HW branch reads it here. If HW
    /// ever needs a divergence on a column that also varies for MC2,
    /// promote this to a dedicated field.
    pub(crate) fn is_hidden_worlds(&self) -> bool {
        matches!(self.verbs.targeting, TargetingVerb::Mc1Hw)
    }

    /// One-time target acquisition sub_54520 (:63943): nearest awake
    /// creature (any range) or wizard within the caster row's v_28,
    /// inside a ±0x71 yaw AND pitch cone, 3D distance ≤ 5120.
    ///
    /// **THE LIGHTNING BEAM (model 9) IS ITS OWN CASE.** `sub_54520`
    /// switches on `+65`, and case 9 (:64125, remc1hw :60256 —
    /// identical) scores the CREATURE buckets at `(0x71, 0x200)`: the
    /// yaw wedge is the usual ±20°, but the PITCH cone is ±90°, i.e.
    /// effectively unbounded. Wizards/castles keep `(0x71, 0x71)`.
    /// That one constant is the Lightning Storm: the (10,38) cloud
    /// fires its (9,9) bolts at a fixed pitch 56 (≈10° down) from
    /// 1024 above the ground, and the bolt's reach is only
    /// `life/3 + 1` steps of 384 — it can NEVER reach the ground on
    /// its own. Every kill comes from the acquire snapping the beam
    /// onto a creature. Under the shared ±0x71 pitch cone the flock
    /// underneath the cloud is outside the wedge, nothing locks, and
    /// the bolts sail out level "just above the monsters" — the
    /// reported bug. Retail's ±0x200 makes the storm the flock killer
    /// it is remembered as.
    fn aim_assist_mc1(&mut self, i: usize, ctx: &MobCtx) {
        let creature_pitch = if self.ent[i].model65 == 9 {
            0x200
        } else {
            0x71
        };
        self.aim_assist_mc1_cone2(i, ctx, 0x71, 0x71, creature_pitch);
    }

    /// [`Self::aim_assist_mc1`] with an explicit acquire cone. The base
    /// MC1 scan is `0x71`/`0x71`; Hidden Worlds' Fire Storm child (model
    /// 16, acquire switch case 0x10, remc1hw :60322) widens the YAW cone
    /// to `0x100` while the pitch stays `0x71` on BOTH candidate lists.
    /// APPROX: case 0x10 scans the spatial buckets for any awake entity;
    /// we reuse the shared creature+wizard+player candidate set (the
    /// meaningful enemy set), only widening the cone.
    fn aim_assist_mc1_cone(&mut self, i: usize, ctx: &MobCtx, yaw_cone: u32, pitch_cone: u32) {
        self.aim_assist_mc1_cone2(i, ctx, yaw_cone, pitch_cone, pitch_cone);
    }

    /// [`Self::aim_assist_mc1_cone`] with the CREATURE-bucket pitch cone
    /// split out: `sub_54520` case 9 (the lightning beam) is the one
    /// subtype that scores creatures on a different cone than the
    /// wizard/castle list.
    fn aim_assist_mc1_cone2(
        &mut self,
        i: usize,
        ctx: &MobCtx,
        yaw_cone: u32,
        pitch_cone: u32,
        creature_pitch: u32,
    ) {
        let (px, py, pz, yaw, pitch, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.id24)
        };
        let mut best: Option<(u16, u32, u16, u16)> = None; // (slot, score, yaw, pitch)
        let consider = |tx: u16,
                        ty: u16,
                        tz: i16,
                        slot: u16,
                        pcone: u32,
                        best: &mut Option<(u16, u32, u16, u16)>| {
            let d2 = Self::dist2_sq(px, py, tx, ty);
            let dz = tz.wrapping_sub(pz) as i32;
            let d3 = d2.wrapping_add(dz.wrapping_mul(dz));
            if d3 > 5120 * 5120 {
                return;
            }
            let ty_yaw = Self::angle_between(px, py, tx, ty);
            let dh = Self::isqrt(d2 as u32) as i32;
            let ty_pitch = Self::pitch_toward(pz, tz, dh);
            let dy = Self::angdist(yaw, ty_yaw) as u32;
            let dp = Self::angdist(pitch, ty_pitch) as u32;
            if dy > yaw_cone || dp > pcone {
                return;
            }
            let score = dy * dy + dp * dp;
            // Strictly-less: on a score tie the earlier slot wins,
            // matching the original's scan order.
            if best.is_none() || best.is_some_and(|(_, bs, _, _)| score < bs) {
                *best = Some((slot, score, ty_yaw, ty_pitch));
            }
        };
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 5 || c.tick70 == 120 || c.act_life < 0 || c.f58 == 0 {
                continue;
            }
            if c.id24 == own {
                continue;
            }
            let (tx, ty, tz) = (c.x, c.y, c.aim_z());
            consider(tx, ty, tz, j as u16, creature_pitch, &mut best);
        }
        // The significant-entity list (sub_54520 list 1): rival
        // wizards (models 0/1) AND CASTLES (model 2) — live, not
        // cloaked (+16 0x20), not the caster's own team. Wizards
        // score through the generic scorer sub_54A90, whose
        // sub_524C0 bracket lifts the aim z by +78; castles route
        // to the dedicated castle scorer sub_54BD0 (:60452 — same
        // cones/range/score) measured at the RAW position: the +78
        // lift explicitly skips model 2. Both the base cases 0/3/4
        // (:60122, cone 0x71) and HW's meteor case 0x10 (:60322,
        // cone 0x100) branch `+65 == 2` to the castle scorer —
        // castles are first-class homing candidates, which is how
        // retail meteors fall a rival's castle out from under a
        // camping wizard (mc1hwl0 slot 522, chase=522).
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 3 || c.flags & (0x400 | 0x20) != 0 || c.id24 == own {
                continue;
            }
            match c.model65 {
                0 | 1 if c.tick70 == 1 => {
                    let (tx, ty, tz) = (c.x, c.y, c.aim_z());
                    consider(tx, ty, tz, j as u16, pitch_cone, &mut best);
                }
                2 => consider(c.x, c.y, c.aim_z(), j as u16, pitch_cone, &mut best),
                _ => {}
            }
        }
        // Invisible (spell 12, :65689-90 — the +16 0x20 bit): the
        // cloaked player is skipped by mob-side target acquisition.
        if own != PLAYER_TARGET && !self.player_invisible {
            consider(
                ctx.px,
                ctx.py,
                ctx.pz.wrapping_add(PLAYER_HH as i16),
                PLAYER_TARGET,
                pitch_cone,
                &mut best,
            );
        }
        if let Some((slot, _, ty_yaw, ty_pitch)) = best {
            self.ent[i].f146 = slot;
            self.ent[i].f34 = ty_yaw;
            self.ent[i].f36 = ty_pitch;
            // Being targeted arms the danger music (:64013/:64095 —
            // acquire of a class-3 m0 human calls sub_46520).
            if slot == PLAYER_TARGET {
                self.player_danger = 100;
            }
        }
    }

    /// Read-only twin of the acquire family below for the crosshair
    /// instrument (P-class `crosshair` option): identical candidate
    /// filters, cone (±0x71 yaw AND pitch), 3D range (≤ 5120) and
    /// min-score pick as [`Self::aim_assist`] /
    /// [`Self::aim_assist_wizards`] / [`Self::aim_assist_possess`] —
    /// but NO entity writes, NO `player_danger` arming and NO LCG
    /// draws, so it is safe to run every frame without touching
    /// simulation state. The caster is the human player
    /// (own = PLAYER_TARGET), so the mob scans' player-candidate arm
    /// never applies. Returns the acquired slot.
    pub(crate) fn aim_preview_scan(
        &self,
        px: u16,
        py: u16,
        pz: i16,
        yaw: u16,
        pitch: u16,
        set: AimPreviewSet,
    ) -> Option<u16> {
        let own = PLAYER_TARGET;
        let mut best: Option<(u16, u32)> = None;
        let mut consider = |tx: u16, ty: u16, tz: i16, slot: u16| {
            let d2 = Self::dist2_sq(px, py, tx, ty);
            let dz = tz.wrapping_sub(pz) as i32;
            if d2.wrapping_add(dz.wrapping_mul(dz)) > 5120 * 5120 {
                return;
            }
            let ty_yaw = Self::angle_between(px, py, tx, ty);
            let dh = Self::isqrt(d2 as u32) as i32;
            let ty_pitch = Self::pitch_toward(pz, tz, dh);
            let dy = Self::angdist(yaw, ty_yaw) as u32;
            let dp = Self::angdist(pitch, ty_pitch) as u32;
            if dy > 0x71 || dp > 0x71 {
                return;
            }
            let score = dy * dy + dp * dp;
            if best.is_none_or(|(_, bs)| score < bs) {
                best = Some((slot, score));
            }
        };
        if set == AimPreviewSet::Possess {
            // Mirror of aim_assist_possess: unowned/unclaimed awake
            // mana balls (m39/40) + anyone else's houses (m45).
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 != 10 || c.flags & 0x400 != 0 {
                    continue;
                }
                let candidate = match c.model65 {
                    39 | 40 => c.f58 != 0 && c.f144 != own && c.id24 != own,
                    45 => c.f144 != own && c.id24 != own,
                    _ => false,
                };
                if candidate {
                    consider(c.x, c.y, c.aim_z(), j as u16);
                }
            }
            return best.map(|(slot, _)| slot);
        }
        if set == AimPreviewSet::Creatures {
            // Mirror of aim_assist's creature scan.
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 != 5 || c.tick70 == 120 || c.act_life < 0 || c.f58 == 0 {
                    continue;
                }
                if c.id24 == own {
                    continue;
                }
                consider(c.x, c.y, c.aim_z(), j as u16);
            }
        }
        // Both remaining sets scan the rival-wizard list (live, not
        // hidden or cloaked, not own).
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 3
                || c.model65 > 1
                || c.tick70 != 1
                || c.flags & (0x400 | 0x20) != 0
                || c.id24 == own
            {
                continue;
            }
            consider(c.x, c.y, c.aim_z(), j as u16);
        }
        // Castles ride the GENERAL acquire only (sub_54520 cases
        // 0/3/4/0x10 walk the significant list's model-2 members
        // through the castle scorer; the wizard-only cases 7/8/B/C
        // scan models 0/1 alone). Mirrors aim_assist_mc1_cone:
        // measured at the RAW position — no +78 lift for model 2.
        if set == AimPreviewSet::Creatures {
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 != 3
                    || c.model65 != 2
                    || c.flags & (0x400 | 0x20) != 0
                    || c.id24 == own
                {
                    continue;
                }
                consider(c.x, c.y, c.z, j as u16);
            }
        }
        best.map(|(slot, _)| slot)
    }

    /// The wizard-only acquire subtype's TargetingVerb seam (see
    /// [`Self::aim_assist`]).
    fn aim_assist_wizards(&mut self, i: usize, ctx: &MobCtx) {
        match self.verbs.targeting {
            TargetingVerb::Mc1 | TargetingVerb::Mc1Hw => self.aim_assist_wizards_mc1(i, ctx),
            TargetingVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Targeting);
                self.aim_assist_wizards_mc1(i, ctx);
            }
        }
    }

    /// The wizard-only acquire (sub_54520 blocks 7/8/B/C — duel m7,
    /// steal m8, undead m11): same cone/range as [`Self::aim_assist`]
    /// but the candidate set is the class-3 wizard list alone.
    fn aim_assist_wizards_mc1(&mut self, i: usize, ctx: &MobCtx) {
        let (px, py, pz, yaw, pitch, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.id24)
        };
        let mut best: Option<(u16, u32, u16, u16)> = None;
        let consider =
            |tx: u16, ty: u16, tz: i16, slot: u16, best: &mut Option<(u16, u32, u16, u16)>| {
                let d2 = Self::dist2_sq(px, py, tx, ty);
                let dz = tz.wrapping_sub(pz) as i32;
                if d2.wrapping_add(dz.wrapping_mul(dz)) > 5120 * 5120 {
                    return;
                }
                let ty_yaw = Self::angle_between(px, py, tx, ty);
                let dh = Self::isqrt(d2 as u32) as i32;
                let ty_pitch = Self::pitch_toward(pz, tz, dh);
                let dy = Self::angdist(yaw, ty_yaw) as u32;
                let dp = Self::angdist(pitch, ty_pitch) as u32;
                if dy > 0x71 || dp > 0x71 {
                    return;
                }
                let score = dy * dy + dp * dp;
                if best.is_none_or(|(_, bs, _, _)| score < bs) {
                    *best = Some((slot, score, ty_yaw, ty_pitch));
                }
            };
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 3
                || c.model65 > 1
                || c.tick70 != 1
                || c.flags & (0x400 | 0x20) != 0
                || c.id24 == own
            {
                continue;
            }
            consider(c.x, c.y, c.aim_z(), j as u16, &mut best);
        }
        if own != PLAYER_TARGET && !self.player_invisible {
            consider(
                ctx.px,
                ctx.py,
                ctx.pz.wrapping_add(PLAYER_HH as i16),
                PLAYER_TARGET,
                &mut best,
            );
        }
        if let Some((slot, _, ty_yaw, ty_pitch)) = best {
            self.ent[i].f146 = slot;
            self.ent[i].f34 = ty_yaw;
            self.ent[i].f36 = ty_pitch;
            if slot == PLAYER_TARGET {
                self.player_danger = 100;
            }
        }
    }

    /// sub_52550 (:62534): per-tick homing — recompute bearing to the
    /// target (z-centered via the
    /// [`crate::engine::features::Ent::aim_z`] model-2 bracket:
    /// castles home at the FLAG, not 8192 under the base) and turn
    /// yaw/pitch capped at the row's v_2/v_6.
    fn home(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let tgt = self.ent[i].f146;
        let (tx, ty, tz) = if tgt == PLAYER_TARGET {
            (ctx.px, ctx.py, ctx.pz.wrapping_add(PLAYER_HH as i16))
        } else {
            let t = tgt as usize;
            if t == 0 || t >= self.ent.len() || self.ent[t].class64 == 0 || self.ent[t].act_life < 0
            {
                self.ent[i].f146 = 0;
                return false;
            }
            let c = &self.ent[t];
            (c.x, c.y, c.aim_z())
        };
        let e = &self.ent[i];
        let yaw = Self::angle_between(e.x, e.y, tx, ty);
        let dh = Self::isqrt(Self::dist2_sq(e.x, e.y, tx, ty) as u32) as i32;
        let pitch = Self::pitch_toward(e.z, tz, dh);
        let row = &BEHAVIOR[e.row156 as usize];
        let (v2, v6) = (row.v_2, row.v_6);
        self.ent[i].f34 = yaw;
        self.ent[i].f36 = pitch;
        let ty_ = Self::turn_step(self.ent[i].f30, yaw, v2);
        self.ent[i].f30 = (self.ent[i].f30 as i32 + ty_ as i32) as u16 & 0x7FF;
        let tp = Self::turn_step(self.ent[i].f32, pitch, v6);
        self.ent[i].f32 = (self.ent[i].f32 as i32 + tp as i32) as u16 & 0x7FF;
        true
    }

    /// sub_11980 (:16988) from a projectile: first overlapped victim
    /// in the surrounding cells passing the filter/owner/damageable
    /// gates. Also probes the out-of-pool player.
    ///
    /// Geometry identical to its `sub_11AC0` sibling below, and for
    /// the same reason — both walk the SEARCH.DAT ring iterator, not
    /// a square: `sub_11410(0, (+80 + 255) >> 8)` at :16999 over a
    /// centre rounded to the NEAREST tile, `(+72 + 128) >> 8`
    /// (:17000-01). The MC2 twin `sub_10780` is byte-identical
    /// (`AddE7EE0x_10080(0, …)`, EF:3700-04). The port truncated the
    /// centre and walked a square with a `.max(1)` floor, so a
    /// zero-extent bolt swept nine tiles instead of its own, and
    /// every probe sat up to a tile behind the shot.
    fn victim_scan(&self, i: usize, ctx: &MobCtx) -> Option<MailTarget> {
        let (wx, wy, id, f66, f67) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24, e.f66, e.f67)
        };
        let r = ((self.ent[i].f80 as i32 + 255) >> 8).max(1);
        for dy in -r..=r {
            for dx in -r..=r {
                let tx = ((wx >> 8) as i32 + dx) as u8;
                let ty = ((wy >> 8) as i32 + dy) as u8;
                let mut j = self.map_entity[tile(tx, ty)] as usize;
                while j != 0 {
                    let c = &self.ent[j];
                    // Class-14 map objects (MC2 XP scrolls, mouth/
                    // checkpoint markers) are OBSERVABLE pass-through:
                    // retail's probe admits them mechanically (the
                    // (14,5) ctor keeps byte[0]&8, EF:37315/37365,
                    // and a player bolt's xtype is the −1 wildcard)
                    // but its ≈0-box, own-cell, endpoint-only probe
                    // never reaches the scroll's 768/1280 PICKUP box
                    // in practice (EF:63127-28 + Events.cpp:132 ring
                    // 0). Our anti-tunneling ring + chord-march
                    // (below/mc2 proj) WOULD reach it — the player's
                    // "fireballs detonate on scrolls / scrolls steal
                    // autoaim" report — so the guard restores the
                    // retail observable (2026-07-16 scroll trace;
                    // MC1 has no class-14, goldens untouched).
                    if c.id24 != id
                        && c.flags & 8 != 0
                        && c.class64 != 14
                        && Self::filter_admits(f66, f67, c.class64, c.model65)
                        && self.ent_overlap(i, j)
                    {
                        return Some(MailTarget::Pool(j));
                    }
                    j = c.next20 as usize;
                }
            }
        }
        if id != PLAYER_TARGET && Self::filter_admits(f66, f67, 3, 0) && self.player_overlap(i, ctx)
        {
            return Some(MailTarget::Player);
        }
        None
    }

    /// The CLAIM/possession candidate test `sub_108B0` (EF:3766)'s
    /// whitelist body. The possession projectile (action 18) does NOT
    /// collide with every solid like the generic `victim_scan`
    /// (`sub_10780`) — it detonates ONLY on entities it could claim
    /// and flies straight through everything else. Whitelist (verbatim
    /// sub_108B0, EF:3826-58): worm heads (5,22); the 512/random mana
    /// spheres (10,39)/(10,40); the foreign-owned sphere variant
    /// (10,57) when its parent tag differs from the caster; and
    /// buildings (10,45) ONLY when POSSESSABLE — `bldgprm.flags & 8
    /// == 0`. The un-possessable factory / terrain-modification
    /// buildings (level-001 cross sinks, level-000 spires) and every
    /// wizard / marker keep the bit set or fall off the list, so
    /// possession passes through them (NOT the generic probe, which
    /// would consume the shot on those sinks). Retail's accept filter
    /// (EF:3862-67) is TWO-armed: the creator half (`id_0x1A_26` →
    /// `id24`) AND the claim-owner half (`playerEntityIndex_0x94_148`
    /// → `f144`, the field both claim intakes write) — a ball or
    /// building the caster already possesses does NOT eat the bolt;
    /// it flies through to the unclaimed field behind. A
    /// rival-claimed target fails neither half and stays claimable —
    /// PLAYER RETAIL-CERTIFIED 2026-07-27 for ALL tiers including
    /// the tier-1 Mana Magnet (a briefly-tried tier-1 carve-out that
    /// detonated on own-claimed spheres was refuted by the player's
    /// own retail discriminator run: it does NOT explode on already
    /// possessed mana; the "sticks to my mana" feel is the aura
    /// piling the claimed spheres onto the detonation point).
    fn claim_admits(&self, j: usize, own: u16) -> bool {
        let c = &self.ent[j];
        if c.flags & 8 == 0 {
            return false;
        }
        match (c.class64, c.model65) {
            (5, 22) => c.id24 != own && c.f144 != own,
            (10, 39) | (10, 40) => c.id24 != own && c.f144 != own,
            // The (10,57) foreign sphere: gated on the PARENT TAG
            // alone, no id/owner re-check (sub_108B0's early-return
            // arm, EF:3846 `v8x->parentId_0x28_40 != a1x->id_0x1A_26`).
            //
            // That parent tag is retail's `@0x28`, and the port's home
            // for `@0x28` is `id24` — the importer fuses `id_0x1A` and
            // `parentId_0x28` into it (world/conformance.rs `owner28`)
            // and `mc2_fools_retaliate` reads the same lane for its
            // owner-skip. `f40` is `@0x26`, an unrelated latch; the
            // old test there was a live carve-out failure the moment a
            // CAST decoy (whose parentId is the caster) met its own
            // caster's possess bolt — retail flies through, the port
            // detonated. Until OPEN-6 stamped the native model this
            // arm only ever saw IMPORTED spheres, whose `@0x26` and
            // `@0x28` both read 0, which is why it never showed.
            (10, 57) => c.id24 != own,
            (10, 45) => {
                c.id24 != own
                    && c.f144 != own
                    && self
                        .assets
                        .bldgprm
                        .get(c.f71 as usize)
                        .is_none_or(|b| b.flags & 8 == 0)
            }
            _ => false,
        }
    }

    /// The possession victim probe `sub_108B0` (EF:3766): the same
    /// tile-chain sweep as [`Self::victim_scan`] but under the
    /// claim whitelist ([`Self::claim_admits`]) — and with NO player
    /// probe (sub_108B0 never reaches the human wizard; you cannot
    /// possess a wizard). Same ring-iterator geometry as
    /// [`Self::victim_scan`], verbatim: `AddE7EE0x_10080(0, (+82 +
    /// 255) >> 8)` over the `(pos + 128) >> 8` centre (EF:3798-3801).
    fn claim_victim_scan(&self, i: usize) -> Option<MailTarget> {
        let (wx, wy, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24)
        };
        let r = ((self.ent[i].f80 as i32 + 255) >> 8).max(1);
        for dy in -r..=r {
            for dx in -r..=r {
                let tx = ((wx >> 8) as i32 + dx) as u8;
                let ty = ((wy >> 8) as i32 + dy) as u8;
                let mut j = self.map_entity[tile(tx, ty)] as usize;
                while j != 0 {
                    let next = self.ent[j].next20 as usize;
                    if self.claim_admits(j, id) && self.ent_overlap(i, j) {
                        return Some(MailTarget::Pool(j));
                    }
                    j = next;
                }
            }
        }
        None
    }

    /// [`Self::claim_victim_scan`] at a temporary probe position (the
    /// marched-substep companion of [`Self::victim_scan_at`]).
    pub(crate) fn claim_victim_scan_at(
        &mut self,
        i: usize,
        tmp: (u16, u16, i16),
    ) -> Option<MailTarget> {
        let old = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        self.ent[i].x = tmp.0;
        self.ent[i].y = tmp.1;
        self.ent[i].z = tmp.2;
        let v = self.claim_victim_scan(i);
        self.ent[i].x = old.0;
        self.ent[i].y = old.1;
        self.ent[i].z = old.2;
        v
    }

    /// The explode tail shared by the flight handlers: accuracy stats
    /// (sub_526C0 :62585), spawn the +68/+69 effect, despawn. The
    /// generic sub_52770 path (:62759-72) also copies +44 and the
    /// victim; sub_52B30 (fireball) does NOT (:62928-30) — the fire's
    /// own 400 is the fireball's real damage.
    fn proj_explode(
        &mut self,
        i: usize,
        ctx: &MobCtx,
        struck: Option<MailTarget>,
        copy_f44: bool,
        stamp_victim: bool,
    ) {
        let (x, y, z, owner, yaw, pitch, f44, f69) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30, e.f32, e.f44, e.f69)
        };
        if owner == PLAYER_TARGET {
            self.shots += 1;
            let aimed = self.ent[i].f146;
            if struck.is_some_and(|s| match s {
                MailTarget::Pool(j) => aimed == self.ent[j].id24 || aimed == j as u16,
                MailTarget::Player => false,
            }) {
                self.hits += 1;
            }
        }
        // Mana Magnet bolt (m17): the real state-18 handler
        // sub_542B0_54640 (hw:59951-60035, byte-identical at
        // :63841-63925 but unwired past remc1's truncated class-9
        // table) detonates on a ball strike, GROUND CONTACT, or life
        // expiry alike — a miss still drops the pair (both spawns
        // are invisible, so an empty-field miss still LOOKS like a
        // fizzle, but loose mana near the landing spot gets pulled;
        // this supersedes the earlier fizzle-on-miss reading). The
        // detonation is a TWO-SPAWN: the (10,12) possession flash
        // FIRST (hw:59993), then the +68/+69 (10,54) magnet
        // (hw:60013) — both stamped with the bolt's owner/heading.
        // The flash is retail's own wildcard possession flash: its
        // ~8-tick channel-1 AREA claim (sub_25760 → sub_120B0) is
        // gated by the victims' +28 bit-1 susceptibility, which
        // admits balls (3) and graves (2) but not houses (33) — the
        // player's "possesses the struck balls simultaneously with
        // creating the magnet". The pulled remainder outside the
        // flash box claims by MERGING (owned-beats-unowned,
        // sub_277D0 :29717); the ch4 pull itself never claims.
        let magnet_bolt = self.ent[i].class64 == 9 && self.ent[i].model65 == 17;
        if magnet_bolt {
            if let Some(fl) = self.spawn_effect(12, x, y, z) {
                let e = &mut self.ent[fl];
                e.id24 = owner;
                e.f30 = yaw;
                e.f32 = pitch;
            }
        }
        if let Some(fx) = self.spawn_effect(f69, x, y, z) {
            let e = &mut self.ent[fx];
            e.id24 = owner;
            e.f30 = yaw;
            e.f32 = pitch;
            // The child carries the struck victim's SLOT in +146
            // — sub_52770's explode block ONLY (:58859-64 `v20[73] =
            // victim`, the states-3/17 generic family); the m0/m1
            // explode (:59015/:59092) writes owner/yaw/pitch alone.
            // Provenance only — no effect handler reads it — but it
            // is an observable lane (the mc1hwl0 clouds carry
            // chase=522).
            if stamp_victim {
                if let Some(MailTarget::Pool(j)) = struck {
                    e.f146 = j as u16;
                }
            }
            if copy_f44 {
                e.f44 = f44;
            }
        }
        let _ = ctx;
        self.ent[i].flags |= 0x400;
    }

    /// Class-9 flight dispatch by state (str_25573C :4838).
    pub(crate) fn proj_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        match self.ent[i].tick70 {
            0 => self.proj_m0_tick(i, ctx),
            1 => self.proj_m1_tick(i, ctx),
            // Global Death's m18 fuse (state 19, reconstruction — see
            // spawn_bomb_fuse): rides the caster, detonates the
            // (10,55) field in place.
            19 => self.bomb_fuse_tick(i, ctx),
            3 => self.proj_generic_tick(i, ctx, true),
            8 => self.proj_m8_tick(i, ctx),
            9 => self.proj_m9_tick(i, ctx),
            10 => self.proj_castle_ball_tick(i, ctx),
            12 => self.proj_m12_tick(i, ctx),
            13 => self.proj_bolt_tick(i, ctx),
            // The Troll/Ape boulder — CARPET.EXE's relocated class-9
            // table binds state 0xF to the sub_52770 thunk 0x53060,
            // the BARE generic flight (the fire-trail wrapper is
            // state 3's own thunk sub_53070 :63021 — the boulder
            // drops no trail). Silent in flight (the arrow roll is
            // state 13's alone); it speaks through its (10,0) impact
            // (sub_3A490 :46454, sound 3 :28114), which inherits the
            // thrown +44 = 780 (:22112). The throw ctor sub_1AE30
            // pre-targets it with the thrower's own +146 (:22122-23)
            // — a thrown boulder HOMES like any generic bolt.
            15 => self.proj_generic_tick(i, ctx, false),
            17 => self.proj_firewall_tick(i, ctx),
            // Player-spell payload projectiles (spell track). The
            // m17 magnet bolt is NOT here — it rides possession's
            // state-1 flight (see spawn_spell_lob).
            2 | 4 | 5 | 7 | 11 => self.proj_payload_tick(i, ctx),
            // Beam segment (state 14; remc1's table is truncated here
            // — lifecycle reconstructed from the slot-order life trick
            // :63349-53): kill on the PRE-decrement value so every
            // segment renders exactly one frame regardless of whether
            // its slot ticks before or after the beam's.
            // Decrement THEN test (the l32 corpus: every dying
            // segment reads act_life −2, never −1 — the post-
            // decrement kill), below −1. Death frames are identical
            // to the pre-decrement form; only the residual value in
            // the record differs, and the recording pins it.
            14 => {
                self.ent[i].act_life -= 1;
                if self.ent[i].act_life < -1 {
                    self.ent[i].flags |= 0x400;
                }
                false
            }
            _ => false,
        }
    }

    /// sub_52B30 (:62779): the fireball. Returns terrain_dirty.
    fn proj_m0_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // ⭐ ONE-SHOT ACQUISITION, exactly as in `sub_52770`'s
        // prologue — `sub_52B30` carries the SAME latch (:62811-15):
        // untargeted and `(+16 & 2) == 0` → set the bit, scan once
        // (model 0 is an acquire case), and on a HIT turn yaw by AT
        // MOST 34 toward the pick with pitch taken outright
        // (`+32 = +36`, :62817-24); on a MISS mirror the live heading
        // into the aim fields and never scan again.
        //
        // The port re-ran the scan EVERY untargeted tick and applied
        // the 34-step every tick with it, so a fireball launched wide
        // kept hunting for its whole life and bent onto anything that
        // wandered into the ±0x71 cone. Retail commits at the muzzle:
        // miss the cone at launch and the shot flies straight. This is
        // the same law as the meteor's, and the fireball is where it
        // is felt — it is the most-cast spell in the game.
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                self.aim_assist(i, ctx);
                if self.ent[i].f146 != 0 {
                    let t = Self::turn_step(self.ent[i].f30, self.ent[i].f34, 34);
                    self.ent[i].f30 = (self.ent[i].f30 as i32 + t as i32) as u16 & 0x7FF;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else {
            self.home(i, ctx);
        }
        self.proj_move_and_hit(i, ctx, false, false, DeflectLaw::Fireball)
    }

    /// sub_52ED0 (:62937): the POSSESS lob (c9 m1). Its flight z is
    /// clamped UP to the terrain each tick (:62975-77 — the lob skims
    /// rising ground), its acquisition scans ONLY mana balls and
    /// houses (sub_54520 case 1, :64040-77 — never creatures or
    /// wizards), and its victim scan is the dedicated sub_11AC0
    /// (:17033): class-10 models 39/40/45 only, skipping entities the
    /// shooter already owns or claimed. Any end detonates into the
    /// (10,12) ch1-claim flash.
    fn proj_m1_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let _ = ctx;
        // ONE acquisition roll on the first untargeted tick — the
        // +16&2 latch (:62952-60), same idiom as the HW dart and the
        // castle ball. A lob that finds nothing (or later loses its
        // slot) flies straight and never re-acquires.
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                self.aim_assist_possess(i);
            }
        } else {
            self.home_possess(i);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let g = self.ground_z(tmp.0, tmp.1) as i16;
        if tmp.2 < g {
            tmp.2 = g; // ground clamp (:62975-77)
        }
        let hit = self.possess_victim_at(i, tmp);
        self.move_relink(i, tmp.0, tmp.1, tmp.2);
        // RECONSTRUCTION BRIDGE (m17 only): the magnet bolt claims a
        // dwelling it PASSES THROUGH in flight (player retail-
        // verified — the pass-through and the exact-flag-hit claims
        // are one mechanism). The decompiled chain has NO path that
        // can claim a (10,45) at all: the (10,12) flash writes ch1
        // and retail dwellings listen on ch0 only (+28 = 33), and an
        // exhaustive sweep proved every flight call pure (sub_11C00 /
        // sub_11AC0 / steer / move / the flash "mover" = a frame
        // counter) — the write was reconstructed away. Bridge: an
        // in-flight direct ch1 touch on overlapped dwellings, gated
        // like the possess scan (:17067 — not own by +24 or +144);
        // the port's built houses carry the ch1 intake.
        if self.ent[i].model65 == 17 {
            let own = self.ent[i].id24;
            let (bx, by) = (self.ent[i].x, self.ent[i].y);
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let tx = ((bx >> 8) as i32 + dx) as u8;
                    let ty = ((by >> 8) as i32 + dy) as u8;
                    let mut j = self.map_entity[tile(tx, ty)] as usize;
                    while j != 0 {
                        let c = &self.ent[j];
                        let next = c.next20 as usize;
                        if c.class64 == 10
                            && c.model65 == 45
                            && c.flags & 8 != 0
                            && c.id24 != own
                            && c.f144 != own
                            && self.ent_overlap(i, j)
                        {
                            self.mail_write(MailTarget::Pool(j), 1, 0, own);
                        }
                        j = next;
                    }
                }
            }
        }
        if let Some(j) = hit {
            // The HIT tick detonates before the life decrement (the
            // l0 impact record keeps life 5, corpus t=69), parking
            // the lob AT the victim's AIM point — x/y and the z+f78
            // bracket (the tent lands the record at 896 − 8192 =
            // −7296).
            let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
            self.move_relink(i, jx, jy, jz);
            self.proj_explode(i, ctx, Some(MailTarget::Pool(j)), false, false);
            return false;
        }
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < 0 {
            self.proj_explode(i, ctx, None, false, false);
        }
        false
    }

    /// The possess-acquire subtype's TargetingVerb seam (see
    /// [`Self::aim_assist`]).
    fn aim_assist_possess(&mut self, i: usize) {
        match self.verbs.targeting {
            TargetingVerb::Mc1 | TargetingVerb::Mc1Hw => self.aim_assist_possess_mc1(i),
            TargetingVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Targeting);
                self.aim_assist_possess_mc1(i);
            }
        }
    }

    /// sub_54520 case 1 (:64040-77): possess acquisition — the awake
    /// (+58 != 0) mana balls (m39/40) and houses (m45) not already
    /// CLAIMED by the shooter (+144 only — the creator +24 half of the
    /// gate is impact-only, :17067), inside the ±0x71 yaw+pitch cone
    /// within 2-D distance 5120 (sub_423D0 has no z term). Best by
    /// sub_54A90's score (:64212-17): the distance decomposed onto the
    /// angular-error axes — 16.16 cos terms >>16, sin terms >>14
    /// through an i16 truncation (~4x misalignment weight) — compared
    /// UNSIGNED (the -1 reject sentinel = u32::MAX). Snaps the heading
    /// on success.
    fn aim_assist_possess_mc1(&mut self, i: usize) {
        use crate::mc1::tables::{COS, SIN};
        let (px, py, pz, yaw, pitch, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.id24)
        };
        // The Mana Magnet bolt (m17) HOMES — sub_54520 case 0x11
        // (hw:60386-60405; remc1's reconstructed switch is TRUNCATED
        // past case 9, which read as "no case 17 → straight flight"
        // until the player's retail playtest refuted it): the
        // mana-BALL roster only (never graves/dwellings), awake-gated
        // (+58) and NOTHING else — no team gate, no claim gate, so
        // caster-claimed balls are homing targets too
        // (player retail-verified). Same 0x71/0x71 cone + 5120 range
        // score (sub_54A90) as possession's case 1; possession keeps
        // its +144-vs-+24 skip (hw:60169-60207) and its second
        // graves/dwellings list.
        let magnet = self.ent[i].model65 == 17;
        let mut best: Option<(u16, u32, u16, u16)> = None;
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 10 || c.flags & 0x400 != 0 {
                continue;
            }
            let candidate = match c.model65 {
                39 => c.f58 != 0 && (magnet || c.f144 != own),
                40 | 45 => !magnet && c.f144 != own && c.f58 != 0,
                _ => false,
            };
            if !candidate {
                continue;
            }
            let (tx, ty, tz) = (c.x, c.y, c.aim_z());
            let ty_yaw = Self::angle_between(px, py, tx, ty);
            let dy = Self::angdist(yaw, ty_yaw) as usize;
            if dy > 0x71 {
                continue;
            }
            let dist = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
            let ty_pitch = Self::pitch_toward(pz, tz, dist);
            let dp = Self::angdist(pitch, ty_pitch) as usize;
            if dp > 0x71 || dist > 5120 {
                continue;
            }
            let v8 = dist * COS[dy];
            let v9 = dist * SIN[dy];
            let v10 = dist * COS[dp];
            let v11 = ((SIN[dp] * dist) >> 14) as i16 as i32;
            let score = ((v10 >> 16) * (v10 >> 16)
                + (v8 >> 16) * (v8 >> 16)
                + ((v9 >> 14) as i16 as i32) * ((v9 >> 14) as i16 as i32)
                + v11 * v11) as u32;
            if best.is_none() || best.is_some_and(|(_, bs, _, _)| score < bs) {
                best = Some((j as u16, score, ty_yaw, ty_pitch));
            }
        }
        if let Some((slot, _, ty_yaw, ty_pitch)) = best {
            let e = &mut self.ent[i];
            e.f146 = slot;
            e.f30 = ty_yaw;
            e.f32 = ty_pitch;
            e.f34 = ty_yaw;
            e.f36 = ty_pitch;
        }
    }

    /// Pool-target homing for the possess lob (the sub_52550 steer
    /// against a class-10 target — the generic home() only handles
    /// creatures/the player).
    fn home_possess(&mut self, i: usize) {
        let t = self.ent[i].f146 as usize;
        if t == 0
            || t >= self.ent.len()
            || self.ent[t].class64 != 10
            || self.ent[t].flags & 0x400 != 0
        {
            self.ent[i].f146 = 0;
            return;
        }
        let (px, py, pz) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (tx, ty, tz) = (self.ent[t].x, self.ent[t].y, self.ent[t].aim_z());
        let yaw = Self::angle_between(px, py, tx, ty);
        let dh = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
        let pitch = Self::pitch_toward(pz, tz, dh);
        let e = &mut self.ent[i];
        let ty_ = Self::turn_step(e.f30, yaw, 34);
        e.f30 = (e.f30 as i32 + ty_ as i32) as u16 & 0x7FF;
        e.f32 = pitch;
        // The shared homer stamps the raw target yaw/pitch into
        // +34/+36 every tick (:62543-46); write-only for the lob
        // (proj_m1_tick reads only f30/f32/f126).
        e.f34 = yaw;
        e.f36 = pitch;
    }

    /// sub_11AC0 (:17033): the possess victim scan — class-10 models
    /// 39/40/45 only, not the shooter's own or already-claimed
    /// (:17067 gates on BOTH +24 and +144), AABB. The Mana Magnet
    /// bolt (m17) instead uses retail's balls-only sibling sub_11C00
    /// (:17109-12, called from the state-18 handler hw:59994): model
    /// 39 + collidable + overlap and NOTHING else — no owner, team,
    /// or claim filter. Crucially the bolt therefore strikes balls
    /// the caster ALREADY CLAIMED — the spell's core economy: strike
    /// your claimed ball, the pulled wild remainder merges into it
    /// and adopts the owner. (An earlier port gate excluded
    /// own-claimed balls — the bolt flew through your pile and
    /// grounded beyond it.)
    fn possess_victim_at(&mut self, i: usize, tmp: (u16, u16, i16)) -> Option<usize> {
        let old = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        self.ent[i].x = tmp.0;
        self.ent[i].y = tmp.1;
        self.ent[i].z = tmp.2;
        let own = self.ent[i].id24;
        let balls_only = self.ent[i].model65 == 17;
        let mut found = None;
        // sub_11AC0's geometry exactly: the scan center is the
        // NEAREST tile (`(+72 + 128) >> 8`, :17046-47) and the
        // neighborhood is the SEARCH.DAT ring iterator (sub_11410
        // rings 0..=(f80+255)>>8) — the retail rings are 2x2-anchored
        // shells (ring 1 spans dx,dy −1..2), which is how the l0 tent
        // two tiles up-range still meets the lob's radius-1 scan
        // (the t=69/t=78 impacts; big-extent victims overlap from
        // well outside a square window).
        let r = (self.ent[i].f80 as i32 + 255) >> 8;
        let cells = self.ring_cells(0, r);
        let cx = ((tmp.0 as i32 + 128) >> 8) as u8;
        let cy = ((tmp.1 as i32 + 128) >> 8) as u8;
        for (dx, dy) in cells {
            let tx = cx.wrapping_add(dx);
            let ty = cy.wrapping_add(dy);
            let mut j = self.map_entity[tile(tx, ty)] as usize;
            while j != 0 {
                let c = &self.ent[j];
                if c.flags & 8 != 0
                    && c.class64 == 10
                    && (c.model65 == 39 || (!balls_only && matches!(c.model65, 40 | 45)))
                    && (balls_only || (c.id24 != own && c.f144 != own))
                    && self.ent_overlap(i, j)
                {
                    found = Some(j);
                    break;
                }
                j = c.next20 as usize;
            }
            if found.is_some() {
                break;
            }
        }
        self.ent[i].x = old.0;
        self.ent[i].y = old.1;
        self.ent[i].z = old.2;
        found
    }

    /// sub_53DC0 (:63628): the storm-carrier flight (c9 m12) — the
    /// Lightning Storm's projectile. Speed eases ±2, homes on an
    /// acquired class-3 target (none exist for us yet → straight
    /// flight); on ANY end but water it becomes the (10,38) storm
    /// cloud, passing owner/heading/victim/damage and the (9,9)
    /// bolt spec down (:63767-83).
    fn proj_m12_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let e = &mut self.ent[i];
        e.f126 += (e.f128 - e.f126).clamp(-2, 2);
        if self.ent[i].f146 != 0 {
            self.home(i, ctx);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let hit = self.victim_scan_at(i, tmp, ctx);
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        let grounded = ground > tmp.2;
        self.move_relink(i, tmp.0, tmp.1, if grounded { ground } else { tmp.2 });
        self.ent[i].act_life -= 1;
        if hit.is_none() && !grounded && self.ent[i].act_life >= 0 {
            return false;
        }
        if grounded && self.on_water_pub(tmp.0, tmp.1) {
            self.splash_and_die(i); // stormless water end (:63704)
            return false;
        }
        let (x, y, z, own, f44, f30, f32) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f44, e.f30, e.f32)
        };
        if let Some(s) = self.spawn_effect(38, x, y, z) {
            let e = &mut self.ent[s];
            e.id24 = own;
            e.f30 = f30;
            e.f32 = f32;
            e.f44 = f44;
            e.f68 = 9;
            e.f69 = 9;
            e.f146 = match hit {
                Some(MailTarget::Pool(j)) => j as u16,
                Some(MailTarget::Player) => PLAYER_TARGET,
                None => 0,
            };
        }
        self.ent[i].flags |= 0x400;
        false
    }

    /// sub_39F40 (:46166): the castle ball (c9 m10) — sprite 18,
    /// speed 384, life 0x2000/384 = 21.
    pub(crate) fn spawn_castle_ball(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(10, 10, x, y, z, 384, 21, 0, 18)
    }

    /// sub_3A040 (:46226): the storm carrier (c9 m12) — sprite 216,
    /// speed 384, life 2048/384 = 5.
    pub(crate) fn spawn_storm_carrier(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(12, 12, x, y, z, 384, 5, 0, 216)
    }

    /// sub_3A270 (:46330): the Wall of Fire bolt (c9 m16, state 17)
    /// — fireball sprite 42, speed 384, life 21. HW swaps the ctor
    /// for sub_3A5F0 (hw:42451), byte-identical except sprite 76
    /// (hw:42474) — the big meteor bitmap. The sprite literal also
    /// sizes the hitbox: SPRITE_STATS row 76 is 420x350 vs 42's
    /// 88x100, so the swap is look AND collision.
    pub(crate) fn spawn_firewall_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let sprite = if self.is_hidden_worlds() { 76 } else { 42 };
        self.spawn_projectile(16, 17, x, y, z, 384, 21, 0, sprite)
    }

    /// The m16 firewall flight (state 17): generic ease + move, no
    /// fire trail. The state-17 handler sub_52770_52AB0 copies the
    /// bolt's +44 into the +68/+69 explosion (:62770, hw:58859) —
    /// BOTH games. remc1's truncated class-9 state table hid the
    /// base-MC1 copy for a while (the question sat banked); the
    /// mc1l5 take settled it: victims under the recorded wall lose
    /// EXACTLY 191/tick = 24464/128 = the copied spell damage over
    /// the cloud's maxLife — the cloud is the wall's ONLY damage
    /// source (its 225 flames are stamped decorative, see
    /// `napalm_tick`). HW keeps its ROW damage (5000 over 6 ticks ≈
    /// 833/tick, the "3 guaranteed hits" law; only the HW model-53
    /// rebound reflect defends, hw:58806).
    fn proj_firewall_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let e = &mut self.ent[i];
        e.f126 += (e.f128 - e.f126).clamp(-2, 2);
        // Hidden Worlds turns Fire Storm (spell 20) into a homing meteor:
        // the m16 child is acquire-switch case 0x10 (remc1hw :60322) —
        // while untargeted it scans awake entities within a WIDENED yaw
        // cone 0x100 (pitch stays 0x71) and homes on the pick. Base MC1
        // has no case 16, so f146 stays 0 and the child flies straight
        // (the fire-rain wall). Seamed on TargetingVerb::Mc1Hw; every
        // other acquire site treats HW exactly as MC1. SURVEY-MC1HW §3a.
        //
        // Acquisition is ONE-SHOT, latched on flags bit 2 even on a
        // miss (remc1hw :58731-49): a miss flies straight forever, a
        // hit SNAPS the live heading to the pick (f30/f32 = f34/f36,
        // :58742-43). Only the post-lock tracker eases (sub_52550,
        // :58754 = home()). Same idiom as the m9 beam (proj_m9_tick).
        //
        // ⚠ THE LATCH ITSELF IS NOT HW's — it is the shared
        // `sub_52770` prologue (:62640-60, and see
        // [`Gen::proj_generic_tick`], which is the other half of the
        // same retail function). Only the SCAN is Hidden Worlds':
        // base MC1's `sub_54520` has no case 16, so it returns 0 and
        // the bolt takes the MISS arm — which still sets the bit and
        // still mirrors the live heading into the aim fields. An
        // earlier note here claimed the shared MC1 path never writes
        // flags bit 2; that was our seam talking, not retail's.
        if self.ent[i].f146 == 0 && self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            if self.is_hidden_worlds() {
                self.aim_assist_mc1_cone(i, ctx, 0x100, 0x71);
            }
            if self.ent[i].f146 != 0 {
                self.ent[i].f30 = self.ent[i].f34;
                self.ent[i].f32 = self.ent[i].f36;
            } else {
                self.ent[i].f34 = self.ent[i].f30;
                self.ent[i].f36 = self.ent[i].f32;
            }
        }
        if self.ent[i].f146 != 0 {
            self.home(i, ctx);
        }
        self.proj_move_and_hit(i, ctx, true, true, DeflectLaw::Generic)
    }

    /// sub_53980/sub_53B50 (:63453/:63525): the castle ball's flight
    /// — steered at the +150 ground target (dest_x/dest_y). The
    /// LAUNCH tick latches, runs the placement scan at the spawn spot
    /// (the hand muzzle under the `castle_latch_bug` retail arm) and
    /// RETURNS — no move (:63612-21; the recorded ball sits latched
    /// and unmoved at its first boundary). A launch failure is a
    /// silent despawn.
    ///
    /// The landing law is the `castle_latch_bug` patch fork
    /// (mc1l32-castle-bug.mgcr; MC1/HW only — MC2 keeps the pre-arm
    /// behavior under both arms, its EF lineage unverified):
    /// - RETAIL arm (:63588-90, the short-circuit `ground > z ||
    ///   life < 0 || !scan`): a terrain touchdown or expiry builds
    ///   the castle at the contact point UNSCANNED; the scan re-runs
    ///   only on airborne ticks, where a failure stops the ball —
    ///   flip 180°, one step back with the live pitch (:63601-04) —
    ///   and still builds. Once launched, a castle always rises.
    /// - PATCHED arm: the landing always re-scans; a refused site is
    ///   displaced one step back (the pre-arm port behavior).
    ///
    /// APPROX: snap-steer in place of the original's eased turn.
    fn proj_castle_ball_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let mc1 = !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        let patched = ctx.patches.castle_latch_bug && !ctx.strict;
        // The UPGRADE variant (+69 = 43, :65904-08) skips the
        // placement scans — it flies at the OWN castle and morphs
        // into the (10,43) token there (sub_53980 has no launch
        // latch: the upgrade ball moves from its first tick).
        let upgrade = self.ent[i].f69 == 43;
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            if !upgrade && !self.castle_site_ok(i, x, y) {
                self.ent[i].flags |= 0x400;
                return false;
            }
            if mc1 && !upgrade {
                return false;
            }
        }
        // The launch speed boost eases away: +126 walks 2/tick toward
        // the ctor +128 (:63565-67 and the :63472-76 upgrade twin).
        if mc1 {
            let e = &mut self.ent[i];
            e.f126 += (e.f128 - e.f126).clamp(-2, 2);
        }
        let (px, py, pz) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (dx, dy) = (self.ent[i].dest_x, self.ent[i].dest_y);
        let tz = self.ground_z(dx, dy) as i16;
        // EASED steering (sub_53B50 :63548-65 via sub_422A0 with the
        // behavior-row caps): the ball leaves along the wizard's aim
        // and turns toward the ground target at row-0 rates — the aim
        // pitch shapes the early arc (NOT snap-steer, which ignores
        // the aim).
        let tgt_yaw = Self::angle_between(px, py, dx, dy);
        let dh = Self::isqrt(Self::dist2_sq(px, py, dx, dy) as u32) as i32;
        let tgt_pitch = Self::pitch_toward(pz, tz, dh);
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let (v2, v6) = (row.v_2, row.v_6);
        {
            let e = &mut self.ent[i];
            e.f34 = tgt_yaw;
            e.f36 = tgt_pitch;
            let ty = Self::turn_step(e.f30, tgt_yaw, v2);
            e.f30 = (e.f30 as i32 + ty as i32) as u16 & 0x7FF;
            let tp = Self::turn_step(e.f32, tgt_pitch, v6);
            e.f32 = (e.f32 as i32 + tp as i32) as u16 & 0x7FF;
        }
        let (yaw, pitch) = (self.ent[i].f30, self.ent[i].f32);
        let mut tmp = (px, py, pz);
        let speed = self.ent[i].f126;
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        let mut grounded = ground > tmp.2;
        // Retail keeps the STEPPED z through the move (:63577-79 —
        // the recorded landing tick shows z 7344 under ground 7808);
        // the ctor'd castle takes its own ground datum. MC2 keeps the
        // pre-arm ground clamp.
        let move_z = if mc1 || !grounded { tmp.2 } else { ground };
        self.move_relink(i, tmp.0, tmp.1, move_z);
        // The with-castle flight lands on OVERLAP with the linked
        // castle — the ball snaps onto it and morphs (:63484-88);
        // the castle's 0x4000 z-extent makes any overflight count.
        if upgrade {
            let c = self.ent[i].f146 as usize;
            if c != 0
                && self.ent[c].class64 == 3
                && self.ent[c].flags & 0x400 == 0
                && self.ent_overlap(i, c)
            {
                let (cx, cy, cz) = (self.ent[c].x, self.ent[c].y, self.ent[c].z);
                self.move_relink(i, cx, cy, cz);
                tmp = (cx, cy, cz);
                grounded = true;
            }
        }
        // The life countdown runs on AIRBORNE ticks only (:63586-88
        // short-circuits the decrement behind the ground test); the
        // pre-arm MC2 path keeps the unconditional decrement.
        if !mc1 || !grounded {
            self.ent[i].act_life -= 1;
        }
        let mut land = grounded || self.ent[i].act_life < 0;
        // The RETAIL arm's airborne tripwire (:63588-90): the scan
        // runs only while still flying; a failure stops the ball
        // here and builds displaced.
        let mut stepback = false;
        if mc1 && !patched && !land && !upgrade && !self.castle_site_ok(i, tmp.0, tmp.1) {
            land = true;
            stepback = true;
        }
        if land {
            let own = self.ent[i].id24;
            if upgrade {
                // Morph into the (10,43) upgrade token at the castle
                // (the token mails the castle's ch5 on touch).
                let (z, link) = (self.ent[i].z, self.ent[i].f146);
                if let Some(t) = self.spawn_creator(43, tmp.0, tmp.1, z) {
                    self.ent[t].id24 = own;
                    self.ent[t].f146 = link;
                }
                self.ent[i].flags |= 0x400;
                return false;
            }
            let (mut bx, mut by) = (tmp.0, tmp.1);
            // RETAIL arm: touchdown/expiry build unscanned; only the
            // tripwire displaces. PATCHED arm (and pre-arm MC2): the
            // landing always re-scans, a refusal displaces.
            let displace = if mc1 && !patched {
                stepback
            } else {
                !self.castle_site_ok(i, bx, by)
            };
            if displace {
                let back = yaw.wrapping_add(0x400) & 0x7FF;
                let mut t = (bx, by, 0i16);
                // The step back carries the live pitch (:63601-04);
                // pre-arm MC2 keeps the flat step.
                Self::polar_step(&mut t, back, if mc1 { pitch } else { 0 }, speed);
                bx = t.0;
                by = t.1;
            }
            if let Some(c) = self.spawn_castle(bx, by) {
                self.ent[c].id24 = own;
                // Claim owner (+144) — the mana census counts the
                // castle's stored mana into the owner's ceiling.
                self.ent[c].f144 = own;
            }
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// sub_12F70 (:17786): the castle placement scan — fails when
    /// another castle (c3 m2) is within its own extents+2048 on both
    /// axes (`abs16(dx) <= f80 + 2048` — the probe's extents play no
    /// part; MC1-faithful, MC2 keeps the pre-arm wider margin), or
    /// any tile of the 8x8 block at (tx-8..tx-1, ty-8..ty-1) — the
    /// original's asymmetric NW-only window, ported verbatim: it
    /// never samples the anchor tile itself nor anything south/east
    /// of it, which is half of the `castle_latch_bug` maze cheese —
    /// carries the protection bit.
    pub(crate) fn castle_site_ok(&self, i: usize, x: u16, y: u16) -> bool {
        let mc1 = !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        let (f80, f82) = if mc1 {
            (0, 0)
        } else {
            (self.ent[i].f80 as i32, self.ent[i].f82 as i32)
        };
        let slack = i32::from(mc1);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 == 3
                && c.model65 == 2
                && c.flags & 0x400 == 0
                && wd(c.x, x) < c.f80 as i32 + f80 + 2048 + slack
                && wd(c.y, y) < c.f82 as i32 + f82 + 2048 + slack
            {
                return false;
            }
        }
        let (tx, ty) = ((x >> 8) as i32, (y >> 8) as i32);
        for dy in -8..0i32 {
            for dx in -8..0i32 {
                if self.t.angle[tile((tx + dx) as u8, (ty + dy) as u8)] & 0x80 != 0 {
                    return false;
                }
            }
        }
        true
    }

    /// sub_37920 (:44229): the class-3 model-2 CASTLE entity —
    /// grid-snapped with (tx+ty) even parity, state 5 machine
    /// (sub-state f59 = 0 → the level-up arm builds level 1),
    /// sprite 177, life 40000. The visible castle is painted
    /// terrain; this entity is the anchor/state machine.
    pub(crate) fn spawn_castle(&mut self, x: u16, y: u16) -> Option<usize> {
        // Snap = TRUNCATION in both ctors (sub_37920's HIBYTE /
        // sub_4AA40's `>>= 8`), then the parity +1 on x.
        let mut cx = (x >> 8) as u8;
        let cy = (y >> 8) as u8;
        if (cx as u16 + cy as u16) % 2 == 1 {
            cx = cx.wrapping_add(1); // parity snap (:44246-52)
        }
        let (px, py) = ((cx as u16) << 8, (cy as u16) << 8);
        // The build datum: MC1 = the ground AT THE RAW LANDING POINT
        // (sub_37920 runs sub_11F50 on the caller's axis BEFORE the
        // snap — mc1l0 t=562: site (114,96) carries z 797, the
        // mid-tile ground, not the corner's 736); MC2's ctor
        // (sub_4AA40 EF:33390-99) = 32 x the perimeter-MIN over the
        // BUILD00 row-1 footprint at the snapped site.
        let z = match self.verbs.movement {
            crate::verbs::MovementVerb::Mc2 => self.mc2_castle_site_z(cx, cy),
            _ => self.ground_z(x, y) as i16,
        };
        let s = self.new_event()?;
        {
            let e = &mut self.ent[s];
            e.class64 = 3;
            e.model65 = 2;
            e.tick70 = 5;
            e.f59 = 0;
            e.f26 = 0;
            e.max_life = 40000;
            // The site echo (+150, :44255) — retail's build workers
            // resolve their castle through it.
            e.dest_x = px;
            e.dest_y = py;
            // Build-site z (+154): the painter/leveler datum. The
            // entity z (+76) is refreshed to live ground per tick —
            // the flag rides the painted tower.
            e.site_z = z;
            // Channel mask (+28 = 33, ch0+ch5 — sub_37920 :44247).
            e.f28 = 33;
        }
        self.link(s, px, py, z);
        self.refill_life(s);
        self.set_sprite(s, 177);
        Some(s)
    }

    /// sub_52770 (:62618): the generic flight (m3 trail bolt) — speed
    /// eases ±2 toward +128, homing, explode copies +44 + victim.
    /// `fire_trail`: m3 drops a damage-suppressed fire-seeder per tick
    /// (:63027-38).
    fn proj_generic_tick(&mut self, i: usize, ctx: &MobCtx, fire_trail: bool) -> bool {
        let e = &mut self.ent[i];
        e.f126 += (e.f128 - e.f126).clamp(-2, 2);
        // ⭐ ACQUISITION IS ONE-SHOT, AND IT SNAPS. `sub_52770` opens
        // by testing the target slot (+146): with a target it goes
        // straight to the tracker, and WITHOUT one it runs the acquire
        // exactly once, latched on flags bit 2 and set win or lose
        // (:62640-60):
        //
        //   if ((flags & 2) == 0) {
        //       flags |= 2;
        //       if (sub_54520(self)) { +30 = +34; +32 = +36; }   // SNAP
        //       else                 { +34 = +30; +36 = +32; }   // mirror
        //   }
        //
        // A hit SNAPS the live heading onto the pick and only then
        // hands over to the per-tick tracker; a MISS mirrors the live
        // heading into the aim fields and the bolt flies straight for
        // the rest of its life, never scanning again. The `else`
        // mirror also runs for the models `sub_54520` declines —
        // m14's `default: return 0` (:64185) — which is why the
        // acquire CALL is unconditional here and only the scan is
        // model-gated.
        //
        // The port used to re-scan EVERY tick while untargeted and
        // never snap, which let a meteor lock onto something that
        // drifted into its cone long after launch (or onto a creature
        // that merely WOKE UP mid-flight — the creature buckets are
        // gated on the awake counter +58, :63996, and retail samples
        // that gate once, at launch), and then ease onto it in a long
        // lazy curve instead of re-pointing. Player-reported as
        // meteors "curving weirdly"; the LONG curve itself is
        // faithful — `sub_52550` tracks the target's live position
        // every tick with no range, lifetime or line-of-sight bound
        // ([`Gen::home`]) — but retail commits to its victim at the
        // muzzle.
        //
        // ⚠ ROOT CAUSE, and the same shape as the castle-extents and
        // building-degradation misses: retail's `sub_52770` is ONE
        // function that the port split in two, and the latch was
        // ported into the `proj_firewall_tick` half only, where a
        // comment claimed it was Hidden Worlds' — it is not, it is
        // right here in base remc1. **The port's function boundaries
        // are not retail's.**
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                // The acquire switch's live cases (:63979 block 0/3/4)
                // — the retail meteor SNAPS to a bee in the cone and
                // the blast ring does the cluster.
                if matches!(self.ent[i].model65, 0 | 3 | 4) {
                    self.aim_assist(i, ctx);
                }
                if self.ent[i].f146 != 0 {
                    self.ent[i].f30 = self.ent[i].f34;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else {
            // The tracker is the ELSE arm: the tick that acquires
            // snaps and stops there, and easing starts the tick after.
            self.home(i, ctx);
        }
        if fire_trail {
            let (x, y, z, owner) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z, e.id24)
            };
            if let Some(s) = self.spawn_effect(1, x, y, z) {
                // +16|=0x80, +18|=1: the seeder's fires inherit the
                // no-damage bit — a decorative trail (:63033-38).
                self.ent[s].flags |= 0x80 | 0x10000;
                self.ent[s].id24 = owner;
            }
        }
        self.proj_move_and_hit(i, ctx, true, true, DeflectLaw::Generic)
    }

    /// sub_530C0 (:63048): m11's bolt — explodes ONLY on wizard-family
    /// victims (class 3 model ≤ 1 / the player); every other end is a
    /// silent despawn (:63188-210).
    fn proj_m8_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let e = &mut self.ent[i];
        e.f126 += (e.f128 - e.f126).clamp(-2, 2);
        if self.ent[i].f146 != 0 {
            self.home(i, ctx);
        }
        // Move.
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        if let Some(v) = self.victim_scan_at(i, tmp, ctx) {
            let wizard = match v {
                MailTarget::Player => true,
                MailTarget::Pool(j) => self.ent[j].class64 == 3 && self.ent[j].model65 <= 1,
            };
            self.move_relink(i, tmp.0, tmp.1, tmp.2);
            if wizard {
                self.proj_explode(i, ctx, Some(v), true, false);
            } else {
                self.ent[i].flags |= 0x400;
            }
            return false;
        }
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        if ground <= tmp.2 {
            self.move_relink(i, tmp.0, tmp.1, tmp.2);
            self.ent[i].act_life -= 1;
            if self.ent[i].act_life < 0 {
                self.ent[i].flags |= 0x400; // silent timeout
            }
        } else if self.on_water_pub(tmp.0, tmp.1) {
            self.splash_and_die(i);
        } else {
            self.ent[i].flags |= 0x400; // silent ground end
        }
        false
    }

    /// sub_535E0 (:63272): the lightning BEAM — resolves in ONE tick.
    /// The flight walks to termination inside the handler in 384-unit
    /// steps (life counts STEPS, not ticks; victim snap / terrain
    /// stop / expiry; NO water splash, NO deflection), then the beam
    /// redraws itself as a chain of short-lived state-14 segment
    /// entities along a ±1 random walk (8 sub-steps per flight step)
    /// and explodes at the segment-walk endpoint. The kraken fires
    /// one beam per burst tick — a beam re-laid every tick, not a
    /// traveling ball.
    fn proj_m9_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        self.ent[i].f126 = self.ent[i].f128;
        let spawn = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        // sub_534C0 (:63216): one-time aim assist only while
        // untargeted (+146 == 0 — fire sites that pre-lock +146 also
        // pre-aim +30/+32 at the target, closing this gate); snap to
        // the acquired angles, no per-tick easing, no homing ever.
        // The snap runs inside the FIRST flight call (:63312), BEFORE
        // the chain heading is saved.
        if self.ent[i].f146 == 0 && self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.aim_assist(i, ctx);
            if self.ent[i].f146 != 0 {
                self.ent[i].f30 = self.ent[i].f34;
                self.ent[i].f32 = self.ent[i].f36;
            } else {
                // :63238-40 — the MISS branch is a real store: the
                // aim pair mirrors the live heading. (The storm's
                // launcher used to pre-seed +34/+36 and stand in for
                // this; the launcher now matches retail and leaves
                // them at NewEvent 0, so the copy has to live here,
                // where the original puts it.)
                self.ent[i].f34 = self.ent[i].f30;
                self.ent[i].f36 = self.ent[i].f32;
            }
        }
        // Yaw/pitch are saved AFTER the snap (:63313-14) and restored
        // for the segment chain (:63327-28) — the visible chain and
        // the endpoint explosion follow the AIMED heading, which is
        // why the retail bolt points at (and lands on) its victim.
        let (yaw0, pitch0) = (self.ent[i].f30, self.ent[i].f32);
        let mut steps: i32 = 0;
        let mut hit: Option<MailTarget> = None;
        loop {
            steps += 1;
            let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            let (yaw, pitch, speed) = {
                let e = &self.ent[i];
                (e.f30, e.f32, e.f126)
            };
            Self::polar_step(&mut tmp, yaw, pitch, speed);
            if let Some(v) = self.victim_scan_at(i, tmp, ctx) {
                // Snap to the victim's exact position — no +78
                // half-height, unlike the fireball (:63252-56).
                match v {
                    MailTarget::Pool(j) => {
                        let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].z);
                        self.move_relink(i, jx, jy, jz);
                    }
                    MailTarget::Player => self.move_relink(i, ctx.px, ctx.py, ctx.pz),
                }
                hit = Some(v);
                break;
            }
            self.move_relink(i, tmp.0, tmp.1, tmp.2);
            if self.ground_z(tmp.0, tmp.1) as i16 > tmp.2 {
                break; // terrain stop — sub_534C0 has no water case
            }
            self.ent[i].act_life -= 1;
            if self.ent[i].act_life < 0 {
                break; // expired midair (≤ 10 steps for life 9)
            }
        }
        self.ent[i].f30 = yaw0;
        self.ent[i].f32 = pitch0;
        // ---- the segment chain (:63329-63420): 8·steps+1 segments
        // along the straight spawn-heading path, sub-step = speed/8.
        let beam_slot = i;
        let owner = self.ent[i].id24;
        let substep = self.ent[i].f126 / 8; // v33 = 48
        let scale = (substep / 4) as i32; // offset unit = 12
        let mut delta = (0u16, 0u16, 0i16);
        Self::polar_step(&mut delta, yaw0, pitch0, substep);
        let mut base = spawn;
        let mut disp = spawn;
        let (mut v32, mut v31): (i32, i32) = (0, 0);
        let mut v30 = steps * 8;
        loop {
            if let Some(s) = self.new_event() {
                // NewEvent defaults kept (hittable bit SET, speed 16,
                // +44 100, filter -1). Slot-order life: a slot that
                // ticks later this frame gets 0, an already-ticked
                // one -1 — one rendered frame each under the
                // state-14 pre-decrement test (:63345-56).
                {
                    let e = &mut self.ent[s];
                    e.class64 = 9;
                    e.model65 = 9;
                    e.tick70 = 14;
                    e.id24 = owner;
                }
                self.link(s, disp.0, disp.1, disp.2);
                self.set_sprite(s, 216);
                // The slot-order life lands in BOTH halves of the
                // pair (l32 corpus: segment max_life is 0/−1 in
                // lockstep with act_life, never a refill value).
                let sl = if s >= beam_slot { 0 } else { -1 };
                self.ent[s].act_life = sl;
                self.ent[s].max_life = sl as u32;
            }
            // Amplitude pinches toward the endpoint (:63358-62).
            let amp = (v30 / 2).clamp(0, 8);
            // Offset walk v32 (applied) then phantom walk v31 (its
            // draws only advance the RNG — confirmed in BOTH
            // decompiles, remc2 sub_66750): ±1 steps with p(+1) =
            // 78/157; draws CONDITIONAL on being inside ±amp, out-of-
            // band offsets pull back deterministically (:63363-92).
            for w in [&mut v32, &mut v31] {
                if *w <= amp {
                    if *w >= -amp {
                        let d = self.ent_rand(i);
                        *w += 2 * ((d % 0x9D) / 79) as i32 - 1;
                    } else {
                        *w += 1;
                    }
                } else {
                    *w -= 1;
                }
            }
            // Advance; the display point offsets by v32·12 in BOTH z
            // and the yaw+0x200 horizontal perpendicular — a diagonal
            // zigzag plane, max ±96 units (:63394-412).
            base.0 = base.0.wrapping_add(delta.0);
            base.1 = base.1.wrapping_add(delta.1);
            base.2 = base.2.wrapping_add(delta.2);
            let off = (v32 * scale) as i16;
            disp = (base.0, base.1, base.2.wrapping_add(off));
            let mut p = (disp.0, disp.1, 0i16);
            Self::polar_step(&mut p, yaw0.wrapping_add(0x200) & 0x7FF, 0, off);
            disp.0 = p.0;
            disp.1 = p.1;
            v30 -= 1;
            if v30 < 0 {
                break;
            }
        }
        // ---- endpoint (:63421-49) ----
        let (f69, f44, f140, f146) = {
            let e = &self.ent[i];
            (e.f69, e.f44, e.f140, e.f146)
        };
        // Accuracy stats sub_526C0 (:62585): human-owned shots only.
        if owner == PLAYER_TARGET {
            self.shots += 1;
            if hit.is_some_and(|s| match s {
                MailTarget::Pool(j) => f146 == self.ent[j].id24 || f146 == j as u16,
                MailTarget::Player => false,
            }) {
                self.hits += 1;
            }
        }
        // Enhanced-lightning presentation feed: the resolved strike,
        // muzzle → chain endpoint (hash-silent, drained by the
        // frontend).
        if self.bolt_fx.0.len() < 256 {
            self.bolt_fx.0.push(crate::engine::features::BoltStrike {
                start: spawn,
                end: disp,
                owner,
            });
        }
        // The explosion lands at the SEGMENT-WALK endpoint, not the
        // beam's snapped position. Shielded (+17 bit7) class-3
        // victims with mana ≥ +140/4 quarter the payload — no drain,
        // no deflection (:63435-47). +146: the original stamps
        // garbage when nothing was hit (remc2 guards this) — we
        // stamp hit-or-0, flagged deviation.
        if let Some(fx) = self.spawn_effect(f69, disp.0, disp.1, disp.2) {
            let quartered = match hit {
                Some(MailTarget::Pool(j)) => {
                    self.ent[j].flags & 0x8000 != 0
                        && self.ent[j].class64 == 3
                        && f140 / 4 <= self.ent[j].f140
                }
                _ => false, // player shields = the spell track
            };
            let e = &mut self.ent[fx];
            e.id24 = owner;
            e.f30 = yaw0;
            e.f32 = pitch0;
            e.f146 = match hit {
                Some(MailTarget::Pool(j)) => j as u16,
                Some(MailTarget::Player) => PLAYER_TARGET,
                None => 0,
            };
            e.f44 = if quartered { f44 >> 2 } else { f44 };
        }
        self.ent[i].flags |= 0x400;
        false
    }

    /// sub_54180 (:63789): the straight bolt (m13) — first-tick LCG
    /// sound roll (the `arrow1`..`arrow4` quartet), direct ch0 area
    /// write on any end. Retail reuses the arrow samples across every
    /// user of this state — the skeleton/archer creatures m4/m9/m10
    /// and the castle guard m15 — including m9, whose projectile wears
    /// a different billboard (sprite 203, :21947). That reuse IS
    /// faithful; only the boulder was wrongly borrowing it.
    fn proj_bolt_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let d = self.ent_rand(i); // :63795
            self.snd(33 + (d & 3) as u8, i);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        let hit = self.victim_scan_at(i, tmp, ctx).is_some();
        let grounded = ground > tmp.2;
        self.move_relink(i, tmp.0, tmp.1, if grounded { ground } else { tmp.2 });
        self.ent[i].act_life -= 1;
        if hit || grounded || self.ent[i].act_life < 0 {
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 0, amt, ctx, false, false);
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// The player-spell payload flight. APPROX(original: c9 m1/m2/m4/
    /// m5/m7/m11/m17 have their own states past remc1's transcribed
    /// table): m13-bolt-shaped straight flight at the cast pitch (the
    /// down-arc arrives via the cast's pitch bias); on any end
    /// (victim / ground / expiry) the struck victim takes the row
    /// damage on ch0 and the per-model payload fires.
    fn proj_payload_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // These states run the engine's generic homing flight
        // (sub_52770), so they carry ITS one-shot prologue too
        // (:62640-60, see [`Gen::proj_generic_tick`]): the acquire is
        // attempted ONCE, latched on flags bit 2, and a hit SNAPS the
        // live heading onto the pick. The sub_54520 subtype switch
        // then picks the candidate set — m4 (volcano) sits in the
        // 0/3/4 creature block; m7/m11 acquire only wizards (block
        // 7/8/B/C — a no-op until AI wizards land); m2/m5/m17 are
        // `default:` and never acquire, taking the miss arm's mirror.
        // Homing runs from the tick after, once +146 holds a target.
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                match self.ent[i].model65 {
                    4 => self.aim_assist(i, ctx),
                    7 | 11 => self.aim_assist_wizards(i, ctx),
                    _ => {}
                }
                if self.ent[i].f146 != 0 {
                    self.ent[i].f30 = self.ent[i].f34;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else {
            self.home(i, ctx);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let hit = self.victim_scan_at(i, tmp, ctx);
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        let grounded = ground > tmp.2;
        self.move_relink(i, tmp.0, tmp.1, if grounded { ground } else { tmp.2 });
        self.ent[i].act_life -= 1;
        if hit.is_some() || grounded || self.ent[i].act_life < 0 {
            if let Some(MailTarget::Pool(j)) = hit {
                let amt = self.ent[i].f44 as u32;
                let src = self.ent[i].id24;
                self.mail_write(MailTarget::Pool(j), 0, amt, src);
            }
            self.spell_payload(i);
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// The per-model detonation payloads of the player-spell
    /// projectiles (each cite = the traced cast arm's effect).
    fn spell_payload(&mut self, i: usize) {
        let (x, y, z, model) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.model65)
        };
        let gz = self.ground_z(x, y) as i16;
        let own = self.ent[i].id24;
        match model {
            // Earthquake (:65314): the authentic (10,15) crevice
            // walker — random start heading off its own LCG, ±45
            // wander, a 10-tick m11 digger per step (the rumble is
            // the diggers' loop-10).
            2 => {
                if let Some(w) = self.spawn_creator(15, x, y, gz) {
                    self.ent[w].id24 = own;
                }
            }
            // Volcano (:65432): the growing hill + pit IS the
            // authentic model (trace :65466, effect c10 m9); the
            // finished cone spawns the model-18 eruption driver
            // ([`Gen::eruption_tick`]).
            4 => {
                if let Some(h) = self.spawn_creator(9, x, y, gz) {
                    self.ent[h].id24 = own;
                }
            }
            // Crater (:65491): the expanding bowl (authentic:
            // effect c10 m11).
            5 => {
                if let Some(c) = self.spawn_creator(11, x, y, gz) {
                    self.ent[c].id24 = own;
                }
            }
            // Duel to the Death (:65620 → (10,26) ctor :47116): the
            // tether follows the homed wizard and broadcasts the ch4
            // grip 200/tick (sub_263C0 :28949). No wizard target →
            // the bolt ends in a hit flash.
            7 => {
                let victim = self.ent[i].f146;
                let is_wizard = victim == crate::mc1::mobs::PLAYER_TARGET
                    || (victim != 0
                        && self.ent[victim as usize].class64 == 3
                        && self.ent[victim as usize].model65 <= 1);
                if is_wizard {
                    if let Some(t) = self.spawn_effect(26, x, y, z) {
                        self.ent[t].id24 = own;
                        self.ent[t].f146 = victim;
                        self.ent[t].f44 = 200;
                    }
                } else if let Some(f) = self.spawn_effect(23, x, y, z) {
                    self.ent[f].id24 = own;
                }
            }
            // Undead Army (:65927 → the (10,36) spawner sub_26E90
            // :29353): up to 8 class-5 model-9 SKELETONS on a
            // 512-unit ring (angles k·2048/N, facing radial+180°),
            // zero mana (no corpse balls, :29672 gate), capped at 64
            // live skeletons per owner (:29375-81). Owner goes on
            // BOTH +24 and +144 — remc1 writes only +144 (:29399),
            // which would turn gen-1 skeletons on their caster;
            // transcription-slip suspicion beside the :29366
            // hardcode (converted skeletons DO get +24, :23913).
            // Deferred: the human→skeleton conversion AI arm.
            11 => {
                let live = (1..self.ent.len())
                    .filter(|&j| {
                        let c = &self.ent[j];
                        c.class64 == 5 && c.model65 == 9 && c.flags & 0x400 == 0 && c.f144 == own
                    })
                    .count() as i32;
                let n = 8i32.min(64 - live).max(0);
                for k in 0..n {
                    let ang = ((k * (2048 / n)) as u16) & 0x7FF;
                    let mut pos = (x, y, 0i16);
                    Self::polar_step(&mut pos, ang, 0, 512);
                    let sz = self.ground_z(pos.0, pos.1) as i16;
                    if let Some(s) = self.spawn_creature(9, pos.0, pos.1, sz) {
                        let facing = ang.wrapping_add(0x400) & 0x7FF;
                        let e = &mut self.ent[s];
                        e.id24 = own;
                        e.f144 = own;
                        e.f140 = 0;
                        e.f30 = facing;
                        e.f34 = facing;
                    }
                }
            }
            _ => {}
        }
    }

    /// sub_25EC0 (:28731): the volcano eruption driver (m18, state
    /// 18). Counter +26 runs the machine; maxLife (10000) never
    /// counts down:
    /// - counter 0: eruption start — always activates, registers as
    ///   THE erupting volcano (kicking any previous one to counter
    ///   250), swaps the global (10,19) plume, and fires the
    ///   once-per-eruption blast fireball ((10,17) payload, pitch
    ///   -386, life 1) at the rotating heading (:28778-823).
    /// - counters 1..126: activate at p=1/5, except every 16th tick
    ///   (counter&0xF == 0) which never does (:28768-71). Every
    ///   activation lobs ONE ballistic (10,16) lava bomb and turns
    ///   the heading by 0x500 (:28795-804).
    /// - an activation at 127 is the CLEAN death: clears the global
    ///   register (:28825-29). Missing that 1/5 roll leaves the
    ///   register pointing at a dead-idle volcano — the authentic
    ///   no-more-eruptions-anywhere quirk.
    /// - counter > 2500: dormant; p=1/100 per tick to re-arm to 0,
    ///   only while NO volcano is registered (:28750-66).
    /// - every activation (and every re-arm) dies instead if the
    ///   ground height under the driver changed (:28773-77).
    ///
    /// No driver-level sound: eruption audio = the bombs' seeded
    /// fires (crackle 3) + the blast ring (30).
    fn eruption_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let _ = ctx;
        let c = self.ent[i].f26;
        let (x, y, z, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        if c > 2500 {
            let d = self.ent_rand(i);
            if d % 100 == 0 && self.erupting == 0 {
                if self.ground_z(x, y) as i16 != z {
                    self.ent[i].flags |= 0x400;
                    return false;
                }
                self.ent[i].f26 = 0;
            } else if self.ent[i].f26 < i16::MAX - 1 {
                self.ent[i].f26 = c + 1;
            }
            return false;
        }
        let fire = if c != 0 && c < 128 && c & 0xF != 0 {
            self.ent_rand(i) % 5 == 0
        } else {
            c == 0
        };
        if fire {
            if self.ground_z(x, y) as i16 != z {
                self.ent[i].flags |= 0x400; // deformed under: dead
                return false;
            }
            if c == 0 {
                // Register self; kick the previous eruption (:28778-92).
                let prev = self.erupting as usize;
                if prev != 0 && self.ent[prev].class64 == 10 && self.ent[prev].model65 == 18 {
                    self.ent[prev].f26 = 250;
                }
                self.erupting = i as u16;
                let pl = self.plume as usize;
                if pl != 0 && self.ent[pl].class64 == 10 && self.ent[pl].model65 == 19 {
                    self.ent[pl].flags |= 0x400;
                }
                let g = self.ground_z(x, y) as i16;
                self.plume = match self.spawn_effect(19, x, y, g) {
                    Some(p) => {
                        self.ent[p].id24 = own;
                        p as u16
                    }
                    None => 0,
                };
            }
            // One ballistic lava bomb per activation (:28795-801):
            // owner AND the driver's LCG seed pass on.
            let seed = self.ent[i].rand;
            if let Some(b) = self.spawn_lava_bomb(x, y) {
                self.ent[b].id24 = own;
                self.ent[b].rand = seed;
            }
            // Heading advances 0x500 per activation (:28804).
            self.ent[i].f30 = self.ent[i].f30.wrapping_add(0x500);
            if c == 0 {
                // The eruption-start blast fireball (:28805-23):
                // pitch -386, life 1, detonates into the (10,17)
                // fire-field. APPROX: the +150 position-target
                // steering is skipped (aim assist suppressed) — it
                // flies the armed heading.
                let yaw = self.ent[i].f30 & 0x7FF;
                if let Some(p) = self.spawn_fireball(x, y, z) {
                    let e = &mut self.ent[p];
                    e.id24 = own;
                    e.f30 = yaw;
                    e.f34 = yaw;
                    e.f32 = (-386i16 as u16) & 0x7FF;
                    e.f36 = e.f32;
                    e.f68 = 10;
                    e.f69 = 17;
                    e.act_life = 1;
                    e.flags |= 2;
                }
            }
            if c >= 127 {
                self.erupting = 0; // the clean death (:28825-29)
                self.ent[i].flags |= 0x400;
                return false;
            }
        }
        self.ent[i].f26 = c + 1;
        false
    }

    /// sub_3ACC0 (:46958): the (10,16) lava bomb — draws IN ORDER
    /// off its own LCG: life = %100+100, speed = %50 (held), vz =
    /// 256 up, yaw = rand & 0x7FF; speed applies as +52; spawned
    /// map-linked at ground+64 with the horizontal velocity vector
    /// pre-advanced into +150/+152 (our dest_x/dest_y), sprite 210.
    fn spawn_lava_bomb(&mut self, x: u16, y: u16) -> Option<usize> {
        let b = self.new_event()?;
        {
            let e = &mut self.ent[b];
            e.class64 = 10;
            e.model65 = 16;
            e.tick70 = 16;
            e.f44 = 200;
            e.flags = (e.flags & !(8 | 0x20000)) | 0x20000;
            let d1 = lcg32(&mut e.rand);
            e.max_life = d1 % 0x64 + 100;
            let d2 = lcg32(&mut e.rand);
            e.f46 = 256;
            let d3 = lcg32(&mut e.rand);
            e.f30 = (d3 & 0x7FF) as u16;
            e.f126 = (d2 % 0x32) as i16 + 52;
        }
        let gz = (self.ground_z(x, y) + 64) as i16;
        self.link(b, x, y, gz);
        {
            let (yaw, speed) = (self.ent[b].f30, self.ent[b].f126);
            let mut v = (0u16, 0u16, 0i16);
            Self::polar_step(&mut v, yaw, 0, speed);
            let e = &mut self.ent[b];
            e.dest_x = v.0;
            e.dest_y = v.1;
        }
        self.refill_life(b);
        self.set_sprite(b, 210);
        Some(b)
    }

    /// sub_25A60 (:28573): the lava bomb's ballistic flight —
    /// per-axis velocity clamp ±80, gravity -28/tick (vz clamped
    /// [-384, 256]), ground bounce vz = -vz/4, water splash, and at
    /// rest a 30-tick standing fire at 3x damage (if none already
    /// burns on the cell), then downhill roll under 250/256
    /// friction. Slope roll APPROX: central-difference gradient in
    /// place of sub_41F50's table.
    fn lava_bomb_tick(&mut self, i: usize) -> bool {
        // :28592-94 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        let mut vx = (self.ent[i].dest_x as i16).clamp(-80, 80);
        let mut vy = (self.ent[i].dest_y as i16).clamp(-80, 80);
        let mut vz = (self.ent[i].f46 - 28).clamp(-384, 256);
        let (x0, y0, z0) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let x = x0.wrapping_add(vx as u16);
        let y = y0.wrapping_add(vy as u16);
        let mut z = z0.wrapping_add(vz);
        let g = self.ground_z(x, y) as i16;
        let mut grounded = false;
        if z <= g {
            z = g;
            grounded = true;
            if self.on_water_pub(x, y) {
                self.move_relink(i, x, y, z);
                self.splash_and_die(i);
                return false;
            }
            vz = -vz / 4; // bounce (:28625)
            if vz.abs() <= 28 {
                vz = 0;
                // Seed a standing fire at rest if the cell has none
                // (:28637-47): life 30, damage 3× the FRESH fire's own
                // ctor +44 (50 → 150) — retail reads the just-spawned
                // fire's +44 (:28642), NOT the bomb's +44 (=200, a dead
                // store). The bomb dealing no in-flight damage, this is
                // the lava's only bite.
                let mut burning = false;
                let mut j = self.map_entity[tile((x >> 8) as u8, (y >> 8) as u8)] as usize;
                while j != 0 {
                    if self.ent[j].class64 == 10
                        && self.ent[j].model65 == 6
                        && self.ent[j].flags & 0x400 == 0
                    {
                        burning = true;
                        break;
                    }
                    j = self.ent[j].next20 as usize;
                }
                if !burning {
                    let own = self.ent[i].id24;
                    if let Some(f) = self.spawn_effect(6, x, y, z) {
                        self.ent[f].id24 = own;
                        self.ent[f].act_life = 30;
                        self.ent[f].f44 *= 3;
                    }
                }
            }
        }
        if grounded {
            // Downhill roll + friction (:28655-67).
            let gxm = self.ground_z(x.wrapping_sub(256), y) as i16;
            let gxp = self.ground_z(x.wrapping_add(256), y) as i16;
            let gym = self.ground_z(x, y.wrapping_sub(256)) as i16;
            let gyp = self.ground_z(x, y.wrapping_add(256)) as i16;
            vx = (vx + (gxm - gxp) / 8).clamp(-80, 80);
            vy = (vy + (gym - gyp) / 8).clamp(-80, 80);
            vx = ((250 * vx as i32) >> 8) as i16;
            vy = ((250 * vy as i32) >> 8) as i16;
        }
        let e = &mut self.ent[i];
        e.dest_x = vx as u16;
        e.dest_y = vy as u16;
        e.f46 = vz;
        self.move_relink(i, x, y, z);
        false
    }

    /// sub_26140 (:28834), class-10 state 19 — the (10,19) eruption
    /// plume, a 240-tick emitter riding the crater. TRACED (it was
    /// "untraced: life countdown + animation only"): pre-decrement life
    /// like the whole class-10 family, then a per-tick SMOKE SPRAY —
    /// the radius-0 ring (the 2x2 recentre block, 3 cells after the
    /// iterator's dropped-last quirk), each cell rolling the same
    /// ~50% skip test the fire spreader uses and, on a pass, a ±64
    /// jitter pair; on ODD post-decrement life ticks each passing cell
    /// emits FOUR (10,13) puffs at yaws {v, v+0x200, v+0x400, v+0x600}
    /// where v alternates 0/0x100 every other pair of ticks, so the
    /// column corkscrews. The plume then re-seats on the ground.
    /// Retail runs NO animation step here (sprite 228 is static) and
    /// the retail rand cadence is 3/5/7/9 draws a tick — the port drew
    /// zero, which is the whole (10,19) `rand` column in mc1hwl0.
    ///
    /// OPEN (banked, not ported): retail closes the handler with an
    /// UNCONDITIONAL `sub_120B0(a1, 0, +44)` — 200 ch0 per tick over
    /// the ctor's 512 extents, i.e. the plume is a damage field for its
    /// whole 240-tick life. That is a gameplay law of its own and no
    /// take in the corpus exercises a volcano; left out pending a
    /// measured eruption window.
    fn plume_tick(&mut self, i: usize) -> bool {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            if self.plume == i as u16 {
                self.plume = 0;
            }
            return false;
        }
        self.ent[i].f26 = 0;
        let (x, y, z, owner) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        for (dx, dy) in self.ring_cells_pub(0, 0) {
            // The skip test and the jitter pair are the spreader's
            // (:28860-70): the draw order is skip, then jitter ONLY on
            // the spawn branch.
            let s = self.ent_rand(i);
            if s % 0x9D < 79 {
                continue;
            }
            let j1 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let px = x.wrapping_add((192 * dx as i32 + j1 - 96) as u16);
            let j2 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let py = y.wrapping_add((192 * dy as i32 + j2 - 96) as u16);
            if self.ent[i].act_life & 1 == 0 {
                continue;
            }
            let mut yaw = (((self.ent[i].act_life / 2) & 1) << 8) as u16;
            while yaw < 0x800 {
                if let Some(p) = self.spawn_effect(13, px, py, z) {
                    let e = &mut self.ent[p];
                    e.id24 = owner;
                    e.f30 = yaw;
                }
                yaw = yaw.wrapping_add(512);
            }
        }
        self.ent[i].z = self.ground_z(x, y) as i16;
        false
    }

    /// sub_257B0 (:28443), class-10 state 13 — the RISING SMOKE PUFF
    /// (remc1hw :26987, byte-identical). Pre-decrement life like the
    /// whole class-10 family; then each tick it
    /// - decays the rise speed +126 by 4, clamped to [64, 128], and
    ///   lifts z by the clamped value, never below the ground under
    ///   its CURRENT cell (the sample precedes the drift);
    /// - for its first 15 ticks (+26 < 16) drifts 30 units flat along
    ///   its own +30 yaw and steps the sprite type on even +26;
    /// - in its last 6 ticks steps the sprite type back down, but only
    ///   while it is above the ctor's base row 67.
    ///
    /// WITHOUT THIS ARM the state fell through world.rs's class-10
    /// catch-all (the terrain-feature dispatch) and every imported puff
    /// self-killed one tick after import: (10,13) was the single
    /// largest unexplained family in the mc1hw corpus.
    fn smoke_puff_tick(&mut self, i: usize) -> bool {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let speed = (self.ent[i].f126 - 4).clamp(64, 128);
        self.ent[i].f126 = speed;
        let mut p = (x, y, z.wrapping_add(speed));
        let g = self.ground_z(x, y) as i16;
        if p.2 < g {
            p.2 = g;
        }
        let v5 = self.ent[i].f26 + 1;
        self.ent[i].f26 = v5;
        if v5 < 16 {
            let yaw = self.ent[i].f30;
            Self::polar_step(&mut p, yaw, 0, 30);
            if v5 & 1 == 0 {
                self.ent[i].type86 = self.ent[i].type86.wrapping_add(1);
            }
        }
        if self.ent[i].act_life < 6 && self.ent[i].type86 > 67 {
            self.ent[i].type86 -= 1;
        }
        self.move_relink(i, p.0, p.1, p.2);
        false
    }

    /// sub_26D20 (:29279), state 40: the lightning STORM cloud.
    /// Rises 64/tick until 1024 above the terrain (doing nothing
    /// else while climbing), then holds that altitude and fires TWO
    /// (9,9) bolts per tick in opposite random directions (pitch 56
    /// down, yaw flipped 0x400 between them), each with a third of
    /// the bolt life, the storm's 2000 damage, and the (10,23)
    /// endpoint flash; thunder 23 per firing tick. Life 32 ticks of
    /// fire (~66 bolts).
    fn storm_cloud_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let _ = ctx;
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let g = self.ground_z(x, y) as i16;
        // :29296-303 — BOTH altitude corrections set the `v1` skip
        // flag: the tick the cloud is pulled DOWN onto the ceiling
        // (drifted terrain, a cloud born high) fires nothing either.
        if z < g.wrapping_add(1024) {
            let nz = z.wrapping_add(64);
            self.move_relink(i, x, y, nz);
            return false;
        }
        if z > g.wrapping_add(1024) {
            self.move_relink(i, x, y, g.wrapping_add(1024));
            return false;
        }
        // :29311-13 — PRE-decrement life test, as across the whole
        // class-10 effect family: 33 bolt ticks, not 32.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        let d = self.ent_rand(i);
        self.ent[i].f32 = 56;
        self.ent[i].f30 = (d & 0x7FF) as u16;
        for _ in 0..2 {
            // Yaw flips 180° BEFORE each launch (:29321-23).
            self.ent[i].f30 = self.ent[i].f30.wrapping_add(0x400) & 0x7FF;
            let (yaw, pitch, f44, own) = {
                let e = &self.ent[i];
                (e.f30, e.f32, e.f44, e.id24)
            };
            // :29325-31 — retail builds a `z + f78` point in the shared
            // temp and then passes the cloud's OWN position struct to
            // the creator: the +78 lift is a DEAD STORE. Adding it put
            // every bolt a sprite half-height above where retail lays
            // it (and above the flock the beam is meant to strike).
            let (bx, by, bz) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            if let Some(b) = self.spawn_zigzag(bx, by, bz) {
                let e = &mut self.ent[b];
                e.id24 = own;
                e.act_life /= 3; // shorter beams (:29334)
                // Retail writes only +30/+32; the beam's own acquire
                // (sub_534C0) fills +34/+36 on a lock and copies the
                // live heading into them on a miss, so pre-seeding them
                // would only matter if it could shadow that — it
                // cannot, and the original leaves them at NewEvent 0.
                e.f30 = yaw;
                e.f32 = pitch;
                e.f68 = 10;
                e.f69 = 23;
                e.f44 = f44;
            }
        }
        self.snd(23, i); // :29343
        false
    }

    /// sub_25760 (:28426), state 12: the possess detonation — a ch1
    /// claim broadcast every tick of its 8-tick life over the 512
    /// extents; balls and built houses consume the SENDER field.
    fn possess_flash_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28433-36 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28432 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28437 — the anim step runs before the ch1 write. The ch1
        // amount carries the claim's FORCE flag (MC2 EF:4200
        // `dword_0x64_100`; consumers never read a ch1 damage) — the
        // (10,12) flash is always the weak claim.
        self.anim_advance(i);
        self.area_write(i, 1, 0, ctx, false, false);
        false
    }

    /// Move + hit scan + terrain shared by the bolt flights — the
    /// common body of sub_52B30 (:62779-936, the m0 fireball) and
    /// sub_52770 (:62618-776, the generic family the class-9 table
    /// routes states 2-6/0xB/0xF/0x10/0x11/0x14 into). Returns
    /// terrain_dirty (always false here — craters come from the
    /// explosion). `law` picks the caller's Rebound deflection arm;
    /// the two are NOT interchangeable (see [`DeflectLaw`]).
    fn proj_move_and_hit(
        &mut self,
        i: usize,
        ctx: &MobCtx,
        copy_f44: bool,
        stamp_victim: bool,
        law: DeflectLaw,
    ) -> bool {
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        if let Some(v) = self.victim_scan_at(i, tmp, ctx) {
            // Rebound (+17 bit 7): mana-shield deflection. The human
            // carpet's bit is the Rebound spell (14, :65774 — the
            // ported deflection-bit semantics). The Generic arm only
            // deflects whitelisted impact pairs (:62705-21): a bolt
            // failing the pair gate — or the mana check — lands as a
            // PLAIN HIT on the deflector (:62751-55), unlike the
            // Fireball arm's fly-through.
            let rebound = match v {
                MailTarget::Pool(j) => self.ent[j].flags & 0x8000 != 0,
                MailTarget::Player => self.player_rebound,
            };
            let gate_ok = rebound
                && match law {
                    DeflectLaw::Fireball => true,
                    DeflectLaw::Generic => {
                        let e = &self.ent[i];
                        e.f68 == 10
                            && (e.f69 == 1
                                || e.f69 == 17
                                || (e.f69 == 53 && self.is_hidden_worlds()))
                    }
                };
            // Scatter around the reversed heading: ±45 (:62877) vs
            // ±22 (:62740).
            let (modulus, half) = match law {
                DeflectLaw::Fireball => (0x5B, 45i32),
                DeflectLaw::Generic => (0x2D, 22i32),
            };
            if rebound {
                match v {
                    MailTarget::Pool(j) => {
                        let quarter = (self.ent[i].f140 / 4).max(0);
                        if gate_ok && quarter <= self.ent[j].f140 {
                            // Sound 28 rides INSIDE the deflect branch
                            // (:62861/:62723 — positional at the
                            // DEFLECTOR, sub_55370(victim, -1, 28)); a
                            // refused deflection is silent.
                            self.snd(28, j);
                            self.ent[j].f140 -= quarter;
                            let deflector_id = self.ent[j].id24;
                            let shooter = self.ent[i].id24;
                            let d = self.ent_rand(i);
                            let e = &mut self.ent[i];
                            e.f34 = e.f30.wrapping_add(0x400) & 0x7FF;
                            e.f30 = (e.f34 as i32 + (d % modulus) as i32 - half) as u16 & 0x7FF;
                            e.f32 = e.f32.wrapping_neg() & 0x7FF;
                            e.f146 = if shooter == PLAYER_TARGET {
                                PLAYER_TARGET
                            } else {
                                shooter
                            };
                            e.id24 = deflector_id;
                            e.act_life = e.max_life as i32;
                            // Relink at the deflector, LIFTED by its
                            // +84 (:62885-88 — victim z + victim+84).
                            let (jx, jy, jz) = (
                                self.ent[j].x,
                                self.ent[j].y,
                                self.ent[j].z.wrapping_add(self.ent[j].f84 as i16),
                            );
                            self.move_relink(i, jx, jy, jz);
                            return false;
                        }
                        if law == DeflectLaw::Fireball {
                            // Afford-fail (:62859's false arm — v24
                            // stays clear): NO hit at all. No sound, no
                            // debit, no explosion — the bolt keeps its
                            // stepped position and flies straight
                            // through.
                            self.move_relink(i, tmp.0, tmp.1, tmp.2);
                            return false;
                        }
                        // Generic refusal falls through to the plain
                        // hit below (:62751-55).
                    }
                    MailTarget::Player => {
                        if gate_ok {
                            self.snd(28, i); // deflection twang (:62861)
                            // The projectile reverses heading and swaps
                            // owner to the player, re-homing on its
                            // shooter. INTERIM: no mana-economy debit on
                            // the player pool (the original quarters the
                            // projectile's +140 against the shield pool).
                            let shooter = self.ent[i].id24;
                            let d = self.ent_rand(i);
                            let e = &mut self.ent[i];
                            e.f34 = e.f30.wrapping_add(0x400) & 0x7FF;
                            e.f30 = (e.f34 as i32 + (d % modulus) as i32 - half) as u16 & 0x7FF;
                            e.f32 = e.f32.wrapping_neg() & 0x7FF;
                            e.f146 = shooter;
                            e.id24 = PLAYER_TARGET;
                            e.act_life = e.max_life as i32;
                            self.move_relink(i, ctx.px, ctx.py, ctx.pz);
                            return false;
                        }
                        // Generic refusal: the bolt hits the rebounding
                        // player like any other (:62751-55).
                    }
                }
            }
            // Teleport onto the victim, explode there (:62852-55).
            match v {
                MailTarget::Pool(j) => {
                    // The +78 aim lift skips castles (sub_524C0's
                    // model-2 guard, [`Ent::aim_z`]): a castle strike
                    // lands at the flag, not 8192 under the mound.
                    let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
                    self.move_relink(i, jx, jy, jz);
                }
                MailTarget::Player => {
                    self.move_relink(i, ctx.px, ctx.py, ctx.pz);
                }
            }
            self.proj_explode(i, ctx, Some(v), copy_f44, stamp_victim);
            return false;
        }
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        if ground <= tmp.2 {
            self.move_relink(i, tmp.0, tmp.1, tmp.2);
            self.ent[i].act_life -= 1;
            if self.ent[i].act_life < 0 {
                self.proj_explode(i, ctx, None, copy_f44, stamp_victim); // midair expiry
            }
        } else if self.on_water_pub(tmp.0, tmp.1) {
            self.splash_and_die(i); // :62916-21, no explosion/crater
        } else {
            self.proj_explode(i, ctx, None, copy_f44, stamp_victim); // terrain hit (pre-move pos)
        }
        false
    }

    /// The victim scan evaluated at a prospective position (the
    /// original moves first and scans at the new position).
    pub(crate) fn victim_scan_at(
        &mut self,
        i: usize,
        tmp: (u16, u16, i16),
        ctx: &MobCtx,
    ) -> Option<MailTarget> {
        let old = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        self.ent[i].x = tmp.0;
        self.ent[i].y = tmp.1;
        self.ent[i].z = tmp.2;
        let v = self.victim_scan(i, ctx);
        self.ent[i].x = old.0;
        self.ent[i].y = old.1;
        self.ent[i].z = old.2;
        v
    }

    fn splash_and_die(&mut self, i: usize) {
        let (x, y, z, owner) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        if let Some(s) = self.spawn_effect(5, x, y, z) {
            self.ent[s].id24 = owner;
        }
        self.ent[i].flags |= 0x400;
    }

    pub(crate) fn on_water_pub(&self, x: u16, y: u16) -> bool {
        self.t.tile_type[(((y >> 8) as usize) << 8) | (x >> 8) as usize] == 0
    }

    // ---- class-10 combat effects -------------------------------------------

    /// The class-10 effect inits (states = the original's +70 writes).
    pub(crate) fn spawn_effect(&mut self, model: u8, x: u16, y: u16, z: i16) -> Option<usize> {
        // On the MC2 column the shared-lineage effects resolve into
        // their NATIVE ctors — the ground fire (0) and the explosion
        // seeder (1) are the same entity in both engines (life 8/1,
        // damage 400, sprite 7/41, extents 128) but tick through the
        // per-game arms (MC2: sub_30D50 worn-path repaints + ring
        // cluster). Without this, an MC1-fallback fireball on an MC2
        // world spawns an MC1-shaped fire that the game-keyed dispatch
        // feeds to the MC2 handler (damage field mismatch → silent
        // fire).
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            match model {
                0 => return self.mc2_spawn_fire(x, y, z),
                1 => return self.mc2_spawn_big_explosion(x, y, z),
                _ => {}
            }
        }
        // sub_3B970 (:47672): the (10,54) mana MAGNET — reached here
        // as the Mana Magnet bolt's +69 detonation (:66084-85); the
        // caller stamps the owner like on every effect.
        if model == 54 {
            return self.spawn_mana_magnet(x, y, z, 0);
        }
        let s = self.new_event()?;
        self.ent[s].class64 = 10;
        self.ent[s].model65 = model;
        match model {
            // sub_3A490 (:46454): the fire/explosion. Damage 400.
            0 => {
                let e = &mut self.ent[s];
                e.tick70 = 0;
                e.max_life = 8;
                e.f44 = 400;
                e.f28 = 0;
                e.flags = (e.flags & !(8 | 0x20000)) | 0x20000;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 7);
                self.extents(s, 128, 128);
            }
            // sub_3A510 (:46482): the fire-spreader / corpse flame.
            1 => {
                let e = &mut self.ent[s];
                e.tick70 = 1;
                e.max_life = 1;
                e.f44 = 400;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 41);
            }
            // sub_3A570 (str_255D0C[2]): the ambient puff — life 8,
            // silent, spriteless and UNLINKED (raw position write, no
            // grid insert), zero extents. The arctic wizard ambience
            // emits these constantly on HW; the tick is the bare
            // family decrement (sub_252B0).
            2 => {
                let e = &mut self.ent[s];
                e.tick70 = 2;
                e.max_life = 8;
                e.f26 = 0;
                e.flags = (e.flags & !0x2_0009) | 0x2_0001;
                e.x = x;
                e.y = y;
                e.z = z;
                self.refill_life(s);
            }
            // sub_3A5D0 (str_255D0C[3]): the smoke puff — life 7,
            // f44/f26 zeroed, linked, sprite 36, silent, no extents;
            // tick = the bare family decrement (sub_253F0).
            3 => {
                let e = &mut self.ent[s];
                e.tick70 = 3;
                e.max_life = 7;
                e.f44 = 0;
                e.f26 = 0;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 36);
            }
            // sub_3AAA0 (str_255D0C[13]): the RISING SMOKE PUFF. Two
            // creators, both untraced until now: the dying standing
            // fire's 1-in-7 exhaust (sub_252D0 :28224) and the volcano
            // plume's ring spray (sub_26140 :28874). Life rand%23+17,
            // rise speed rand%53+51 (the state-13 tick decays it 4 a
            // tick toward the 64 floor), sprite 67, the (10,13) filter
            // pair, +18 bit1. Its class-10 twin model 14 (sub_3AB40,
            // sprite 9, life rand%33+28) has NO MC1 creator — it is the
            // MC2 column's particle, serviced there.
            13 => {
                let e = &mut self.ent[s];
                e.tick70 = 13;
                let d1 = lcg32(&mut e.rand);
                e.max_life = d1 % 0x17 + 17;
                e.flags = (e.flags & !(8 | 0x20000)) | 0x20000;
                let d2 = lcg32(&mut e.rand);
                e.f66 = 10;
                e.f67 = 13;
                e.f126 = (d2 % 0x35 + 51) as i16;
                // Retail's ctor order: link, sprite, THEN refill.
                self.link(s, x, y, z);
                self.set_sprite(s, 67);
                self.refill_life(s);
            }
            // The standing fire / ground wave (state 6, sub_3A730 ctor
            // → sub_252D0 tick): life 240, 50 ch0 per tick via the /10
            // writer, sprite 228 (the flame-size family +86 walks ±1),
            // and the damage extents `sub_37130_374F0(272, 1536)`
            // (:46643) — a ~1-tile-wide, 6-tile-tall AABB so burning
            // trees/lava actually torch fly-by carpets, creatures and
            // neighbor trees (WITHOUT it the fire had zero extents and
            // overlapped nothing → ambient fires dealt no damage). Tree
            // deaths override life and set the f46 trunk offset.
            6 => {
                let e = &mut self.ent[s];
                e.tick70 = 6;
                e.max_life = 240;
                e.f44 = 50;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 228);
                self.extents(s, 272, 1536);
            }
            // sub_3A6B0 (:46560 region): the water splash. Grounded.
            5 => {
                let e = &mut self.ent[s];
                e.tick70 = 5;
                e.max_life = 8;
                e.f44 = 0;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                let (px, py) = (self.ent[s].x, self.ent[s].y);
                self.ent[s].z = self.ground_z(px, py) as i16;
                self.refill_life(s);
                self.set_sprite(s, 244);
            }
            // (10,26) ctor (:47116): the duel tether — life 8,
            // sprite row 284, +44 = the 200/tick ch4 grip amount.
            26 => {
                let e = &mut self.ent[s];
                e.tick70 = 26;
                e.max_life = 8;
                e.f44 = 200;
                e.flags &= !8;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 284);
            }
            // sub_3AC70 (:46935): the invisible fire-ring blast driver.
            17 => {
                let e = &mut self.ent[s];
                e.tick70 = 17;
                e.max_life = 10;
                e.f44 = 3000;
                e.flags &= !8;
                self.link(s, x, y, z);
                self.refill_life(s);
            }
            // sub_3AA10 (:46790): the POSSESS detonation flash —
            // an 8-tick ch1 claim broadcast over 512-unit extents.
            // The original's +44 = -1536 is a mana-drain amount the
            // claim readers never consume (they act on the SENDER
            // field alone); our u16 +44 carries 0 — the drain joins
            // the mana-economy track.
            12 => {
                let e = &mut self.ent[s];
                e.tick70 = 12;
                e.max_life = 8;
                e.f44 = 0;
                // Corpus: every fresh flash record reads flags 0x5 —
                // the ctor sets bit 1 like the (10,19) plume's.
                e.flags = (e.flags & !8) | 1;
                self.link(s, x, y, z);
                self.refill_life(s);
                // The ctor's sub_36FA0(41) — the visible claim
                // sparkle (extents then overridden to 512).
                self.set_sprite(s, 41);
                self.extents(s, 512, 512);
            }
            // sub_3AE00 (:47034): the volcano's (10,19) smoke/fire
            // plume — a 240-tick visual at the crater (sprite 228,
            // the flame family), no damage (+18 bit1 set).
            19 => {
                let e = &mut self.ent[s];
                e.tick70 = 19;
                e.max_life = 240;
                e.f44 = 200;
                e.flags = (e.flags & !8) | 0x20000 | 1;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 228);
                self.extents(s, 512, 512);
            }
            // The Wall of Fire NAPALM cloud (state 58 — NOT 53;
            // class-10 state 53 is the building collapse walker). The
            // model-53 creator was SWAPPED between builds (both spell-20
            // paths detonate the m16 bolt into this (10,53) via the
            // +68=10/+69=53 descriptor — trace SURVEY-MC1HW §3/§7):
            // - base MC1 `sub_3B8E0` (:47639): a persistent low-damage
            //   wall — life 128, f44 100, random yaw, extents 1024/0x4000.
            // - Hidden Worlds `sub_3BC60` (remc1hw :43766): a brief,
            //   devastating expanding-ring detonation — life 6, f44 3000,
            //   NO extents (the state-58 HW handler re-derives them each
            //   tick) and NO yaw LCG draw (stream-faithful).
            53 => {
                let hw = self.is_hidden_worlds();
                let e = &mut self.ent[s];
                e.tick70 = 58;
                e.f26 = 0;
                e.flags &= !8;
                if hw {
                    e.max_life = 6;
                    e.f44 = 3000;
                } else {
                    e.max_life = 128;
                    e.f44 = 100;
                    let d = lcg32(&mut e.rand);
                    e.f30 = (d & 0x7FF) as u16;
                    e.f80 = 1024;
                    e.f82 = 1024;
                    e.f84 = 0x4000;
                }
                self.link(s, x, y, z);
                self.refill_life(s);
            }
            // sub_3BA00 (:47705): the GLOBAL DEATH field (state 60).
            // +26 = 32 = the priming tick-tock; +44 = 100 (the
            // detonation copy overrides with the spell's 7000). The
            // ctor's life 19 / speed 256 / random heading / extents
            // (1024, 0x4000) are DEAD WEIGHT for the state-60
            // handler (verbatim anyway); the flat plane lives in
            // the sweep's 2D distance. No sprite — the spell is
            // authentically invisible.
            55 => {
                let e = &mut self.ent[s];
                e.tick70 = 60;
                e.max_life = 19;
                e.f44 = 100;
                e.f26 = 32;
                e.f126 = 256;
                let d = lcg32(&mut e.rand);
                e.f30 = (d & 0x7FF) as u16;
                e.flags &= !8;
                e.f80 = 1024;
                e.f82 = 1024;
                e.f84 = 0x4000;
                self.link(s, x, y, z);
                self.refill_life(s);
            }
            // sub_3B460 (:47396): the lightning STORM cloud — note
            // state 40 (not 38), life 32, sprite 272. The caller
            // copies heading/target/damage/bolt-spec from the (9,12)
            // storm projectile (:63775-81).
            38 => {
                let e = &mut self.ent[s];
                e.tick70 = 40;
                e.max_life = 32;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 272);
                self.extents(s, 512, 512);
            }
            // sub_3AE80 (:47062): the bolt hit-flash (one-shot ch0).
            23 => {
                let e = &mut self.ent[s];
                e.tick70 = 23;
                e.max_life = 8;
                e.f44 = 25;
                e.flags |= 0x20000 | 1;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 7);
                self.extents(s, 200, 200);
            }
            // sub_3AF00 (:47090): m11's mana-steal flash (ch3).
            25 => {
                let e = &mut self.ent[s];
                e.tick70 = 25;
                e.max_life = 8;
                e.f44 = 2000;
                e.flags &= !8;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 283);
                self.extents(s, 512, 512);
            }
            _ => {
                self.free_entity(s);
                return None;
            }
        }
        Some(s)
    }

    /// sub_3B5A0 (:47443): the mana ball (state 41). Callers override
    /// +140/+144; the tick re-derives the size sprite every turn.
    pub(crate) fn spawn_mana_ball(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let b = self.new_event()?;
        {
            let e = &mut self.ent[b];
            e.class64 = 10;
            e.model65 = 39;
            e.tick70 = 41;
            e.f140 = 512;
            e.f46 = 128;
            e.f28 = 3;
            e.f58 = 0x80;
        }
        self.link(b, x, y, z);
        self.refill_life(b);
        self.ball_resize(b);
        Some(b)
    }

    /// dword_900A4 (:2215): the ball size-class thresholds.
    const BALL_SIZES: [i32; 7] = [256, 512, 1024, 2048, 4096, 9192, 18384];

    /// sub_274D0 (:29574): ball sprite = family base + size class by
    /// carried mana (8 classes; > 36768 = the dragon-drop boulder);
    /// nonzero sizes halve the extents (sub_370E0 :43781). Family 52
    /// = unowned; the owner palette families (105 + 8·player-slot)
    /// are the mana-collection track (our claims use the
    /// PLAYER_TARGET sentinel, not a pool wizard).
    pub(crate) fn ball_resize(&mut self, i: usize) {
        let mana = self.ent[i].f140;
        let mut size = 7usize;
        for (k, t) in Self::BALL_SIZES.iter().enumerate() {
            if mana <= *t {
                size = k;
                break;
            }
        }
        // Owner recolor (:29627-32): claimed balls swap to the owner
        // wizard's color row (base 105 + 8*color, wizext var_48);
        // unowned/wild stay on the neutral 52 row. MC1 art is in raw
        // slot order; MC2's sphere families are authored in Transform
        // order (GetManaSphereIndexFromId EF:26800 routes through
        // TransformPlayerColorIndex — crate::mc2::COLOR_ART).
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        // OPEN (mc2l4 corpus): a RIVAL-claimed sphere renders the
        // NEUTRAL family in retail (sprite 56 = 52+4 with a live
        // class-3 owner in +148) while the human's spheres color
        // 105+size (mc2l0) — the wizard spawn stamps ext color =
        // slot for BOTH (EF:43710), so the neutral derive's
        // mechanism is unresolved. Conformance-invisible: the
        // sprite lane isn't compared and the rotation quad below
        // depends only on SIZE. The port keeps team colors.
        let base = match self.owner_team(self.ent[i].f144) {
            Some(team) => {
                let art = if mc2 {
                    crate::mc2::color_art(team)
                } else {
                    team
                };
                105 + 8 * art as usize
            }
            None => 52,
        };
        let ty = (base + size) as u16;
        if self.ent[i].type86 != ty {
            self.set_sprite(i, ty);
            if mc2 {
                // SetManaSphereColorAndRot (EF:26744-77): every MC2
                // re-sprite overwrites the applied quad with the
                // per-size ROTATION constant — 14·(size+1), except
                // 13 at size 0 — replacing the art extents the
                // sprite setter derives.
                const ROT: [u16; 8] = [13, 28, 42, 56, 70, 84, 98, 112];
                let r = ROT[size.min(7)];
                let e = &mut self.ent[i];
                e.f78 = r;
                e.f80 = r;
                e.f82 = r;
                e.f84 = r;
            } else if size != 0 {
                let e = &mut self.ent[i];
                e.f80 /= 2;
                e.f82 /= 2;
                e.f84 /= 2;
            }
        }
    }

    /// Class-10 combat-effect dispatch. Returns terrain_dirty.
    pub(crate) fn effect_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        match self.ent[i].tick70 {
            0 => self.fire_tick(i, ctx),
            1 => self.spreader_tick(i),
            // sub_252B0 / sub_253F0 (states 2/3): the ambient and
            // smoke puffs — the bare family decrement (PRE-decrement
            // life test like the whole class-10 family), no anim
            // step, no sound. Without these arms an imported puff
            // fell through to the terrain-feature dispatch's
            // self-kill catch-all and died a tick after import.
            2 | 3 => {
                let life = self.ent[i].act_life;
                self.ent[i].act_life = life - 1;
                if life < 0 {
                    self.ent[i].flags |= 0x400;
                }
                false
            }
            6 => self.standing_fire_tick(i, ctx),
            13 => self.smoke_puff_tick(i),
            5 => {
                // :28285-87 — PRE-decrement life test (class-10
                // family): the splash animates 9 ticks, not 8.
                let life = self.ent[i].act_life;
                self.ent[i].act_life = life - 1;
                if life < 0 {
                    // :28294 — retail frees and returns here: no anim
                    // step and no sound on the death tick.
                    self.ent[i].flags |= 0x400;
                    return false;
                }
                self.anim_advance(i);
                // :28288-91 — the one-shot splash sound, latched on the
                // same `& 2` bit the rest of the family uses.
                if self.ent[i].flags & 2 == 0 {
                    self.ent[i].flags |= 2;
                    self.snd(27, i);
                }
                false
            }
            12 => self.possess_flash_tick(i, ctx),
            16 => self.lava_bomb_tick(i),
            17 => self.blast_ring_tick(i, ctx),
            18 => self.eruption_tick(i, ctx),
            19 => self.plume_tick(i),
            23 => self.hit_flash_tick(i, ctx),
            26 => self.duel_tether_tick(i, ctx),
            25 => self.steal_flash_tick(i, ctx),
            40 => self.storm_cloud_tick(i, ctx),
            41 => self.ball_tick(i, ctx),
            // Action 0x3E = the (10,57) RANDOM-VALUE mana sphere
            // (`sub_35FB0` EF:26318). Its physics core — the settle
            // gate `byte_0x39_57`, gravity `word_0x2C_44 -= 16` clamp
            // −128, the −impact/4 terrain bounce zeroed at ≤16, and
            // the grounded downhill-roll damping — is byte-identical
            // to the (10,39) ball's action 0x29 handler
            // (`TransformArcherToMana_35940` EF:26015), which the port
            // already services via `ball_tick`. The two retail handlers
            // differ only in the collection path (m57 has the
            // `word_0x68_104` spawn-(10,0) despawn branch, the ball has
            // the owner-transfer/sound-4 code) — neither runs while the
            // sphere is falling. Only imported m57 spheres ever carry
            // this action (native spawns them with 0x29 via
            // `spawn_mana_ball`); action 62 is m57-exclusive in the
            // corpus, so routing it here services the level-start
            // gravity fall without touching any native golden.
            62 => self.ball_tick(i, ctx),
            42 => {
                self.grave_tick(i);
                false
            }
            85 => self.mc2_mine_tick(i, ctx), // Magic Mine (10,78), action 0x55
            58 => self.napalm_tick(i, ctx),
            59 => {
                self.mana_magnet_tick(i);
                false
            }
            60 => self.death_field_tick(i, ctx),
            _ => false,
        }
    }

    /// sub_263C0 (:28949), class-10 state 26 — the DUEL TETHER:
    /// life-- per tick, follows the victim, broadcasts the ch4 grip
    /// (+44 = 200) into it each tick. The victim's intake latches
    /// the CASTER-side pull (:55663-82).
    fn duel_tether_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28956-58 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28955 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28959 — the anim step. (The victim-tracking transport below
        // is OURS: retail's sub_263C0 simply broadcasts ch4 over the
        // tether's own extents and never moves it. See ROADMAP.)
        self.anim_advance(i);
        let victim = self.ent[i].f146;
        let amt = self.ent[i].f44 as u32;
        if victim == crate::mc1::mobs::PLAYER_TARGET {
            // The human victim (AI-cast duel — unreachable today:
            // no AI selector emits spell 11).
            let (x, y, z) = (ctx.px, ctx.py, ctx.pz);
            self.move_relink(i, x, y, z);
        } else if victim != 0 {
            let v = &self.ent[victim as usize];
            if v.flags & 0x400 != 0 || v.act_life < 0 {
                self.ent[i].flags |= 0x400;
                return false;
            }
            let (x, y, z) = (v.x, v.y, v.z);
            self.mail_write(MailTarget::Pool(victim as usize), 4, amt, i as u16);
            self.move_relink(i, x, y, z);
        }
        false
    }

    /// sub_299D0 (:31263), class-10 STATE 60 — the real GLOBAL DEATH
    /// field. LAW: the class-10 table is keyed by STATE, not MODEL
    /// (model-keying lands on state 55's terrain-raising volcano
    /// riser; cross-check against the napalm cloud's state 58 →
    /// sub_29780). Verbatim: while +26 (32 from the ctor) runs, tick
    /// it down with sound 43 (the audible priming tick-tock); then
    /// ONE full-pool sweep — every enemy entity within 0xA00 (10
    /// tiles) by PURE 2D DISTANCE (sub_423D0 is x/y only: an infinite
    /// vertical kill cylinder): class 2/5 die instantly (life = -1,
    /// no kill credit, no explosion effect), class 3 take the +44
    /// (7000) on ch0, own-team skipped, and an in-range class-9/10
    /// re-arms the field's OWN life to 0 (verbatim quirk,
    /// inconsequential — it frees this tick regardless). Finish:
    /// sound 44 at the field AND at the owner, the sub_44BE0(owner, 3)
    /// full-screen PALETTE FLASH — the violet wash, armed only when the
    /// field's owner is the local player ([`crate::engine::features::PalFlash`])
    /// — then free. NO terrain change, NO drift, NO entity visual: the
    /// screen flash IS the spell's only sighting, and the ctor's
    /// speed/heading/extents are dead weight.
    fn death_field_tick(&mut self, i: usize, _ctx: &MobCtx) -> bool {
        if self.ent[i].f26 > 0 {
            self.ent[i].f26 -= 1;
            self.snd(43, i);
            return false;
        }
        let pre = self.ent[i].act_life;
        self.ent[i].act_life = pre - 1;
        if pre >= 0 {
            let (fx, fy, own, amt) = {
                let e = &self.ent[i];
                (e.x, e.y, e.id24, e.f44 as u32)
            };
            for j in 1..self.ent.len() {
                if j == i {
                    continue;
                }
                let (class, team) = (self.ent[j].class64, self.ent[j].id24);
                if class == 0 || team == own {
                    continue;
                }
                let d2 = Self::dist2_sq(fx, fy, self.ent[j].x, self.ent[j].y);
                if Self::isqrt(d2 as u32) >= 0xA00 {
                    continue;
                }
                match class {
                    2 | 5 => self.ent[j].act_life = -1,
                    3 => self.mail_write(MailTarget::Pool(j), 0, amt, own),
                    9 | 10 => self.ent[i].act_life = 0,
                    _ => {}
                }
            }
            self.snd(44, i);
            if own == crate::mc1::mobs::PLAYER_TARGET {
                self.snd_player(44);
                // sub_44BE0(owner, 3): row 3 = red +48 / blue
                // saturated over the untouched green — the violet
                // flash. Gated on the owner being the local player,
                // exactly as sub_44BE0's slot compare is.
                self.pal_flash.arm(3);
            }
        }
        self.ent[i].flags |= 0x400;
        false
    }

    /// sub_29780 (:31140), class-10 state 58 (the m53 Wall of Fire
    /// cloud). The original branches on `IsHiddenWord`:
    /// - base MC1 (`!IsHiddenWord`, below): 15 waves of standing flames
    ///   over the impact ring (112-unit pitch over SEARCH rings 0..1,
    ///   ±64 jitter, the -96 2x2-center recenter): wave 0 = a persistent
    ///   14-tick ground fire patch, waves 1..14 = 1-tick flame sheets
    ///   climbing 128 units per wave — the rising fire curtain. The
    ///   cloud's own ch0 write is +44/maxLife; the flames' inherited
    ///   100/tick is the damage.
    /// - Hidden Worlds ([`Self::napalm_tick_hw`]): a different geometry —
    ///   one EXPANDING (10,0) ring per tick (160-unit pitch), stepped
    ///   `(var26+2)%7`, until `actLife` runs out; sound 30 once. The
    ///   `IsHiddenWord=true` else-branch (remc1hw :29740; the HW path,
    ///   NOT a multiplayer branch — SURVEY-MC1HW §2).
    fn napalm_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.is_hidden_worlds() {
            return self.napalm_tick_hw(i, ctx);
        }
        // Retail burns the cloud's own life down every tick
        // (:31150-52) — inert under the 15-wave cap, but the mc1l5
        // take shows the decrement in every recorded (10,53) pair.
        {
            let e = &mut self.ent[i];
            e.act_life -= 1;
            if e.act_life < 0 {
                e.flags |= 0x400;
                return false;
            }
            e.f80 = 512;
            e.f82 = 512;
            e.f84 = 2048;
        }
        // The wall's ONLY damage: the cloud's single f44/maxLife
        // write — 24464/128 = 191/tick with the bolt-copied +44,
        // exactly the victim life slope the mc1l5 take records.
        let amt = self.ent[i].f44 as u32 / self.ent[i].max_life.max(1);
        self.area_write(i, 0, amt, ctx, false, false);
        let wave = self.ent[i].f26;
        let (x, y, z, own, f44) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f44)
        };
        let cells = self.ring_cells_pub(0, 1);
        for (dx, dy) in cells {
            let d1 = self.ent_rand(i);
            let d2 = self.ent_rand(i);
            let fx = x.wrapping_add((112 * dx as i32 + (d1 % 0x81) as i32 - 64 - 96) as u16);
            let fy = y.wrapping_add((112 * dy as i32 + (d2 % 0x81) as i32 - 64 - 96) as u16);
            if let Some(f) = self.spawn_effect(6, fx, fy, z) {
                let e = &mut self.ent[f];
                e.id24 = own;
                // Inherited, not the flame ctor's 50 (:31168) —
                // inert under the decorative stamp, but the field
                // is what the take records on every wall flame.
                e.f44 = f44;
                e.act_life = if wave == 0 { 14 } else { 1 };
                e.type86 += 7;
                e.f26 += 7;
                e.f46 = wave * 128;
                // :31169 — +18 bit0 (0x10000): NO ch0 broadcast, the
                // flames are pure light show (without it all 15 live
                // cells accumulate ~100 each into ONE mailbox read ≈
                // 6,000/tick — the reported griffon instakill; retail
                // can never one-shot: 10,000 life / 191 ≈ 53 ticks).
                // +16 bit7 (0x80): no smoke-puff LCG draw — the port's
                // extra rand pulls desynced every wanderer downstream
                // of a wall cast in the take.
                e.flags |= 0x10080;
            }
        }
        self.ent[i].f26 = wave + 1;
        if wave >= 14 {
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// The Hidden Worlds Wall-of-Fire cloud (sub_29780 `IsHiddenWord`
    /// else-branch, remc1hw :29740). Where base MC1 stacks rising waves,
    /// HW paints ONE expanding ground ring per tick: the (10,0) fire on
    /// a 160-unit grid at radius `var26` (`+26`), the radius stepped
    /// `(var26+2)%7` so it sweeps 0,2,4,6,1,3,5, running until the
    /// cloud's `actLife` expires (no wave cap — the spawner's life is the
    /// terminator). Sound 30 plays once (the `+16` bit-1 latch plus a
    /// persistent 0x10000 marker set together on the first surviving
    /// tick). The cloud's own extent tracks the ring (192·var26 wide =
    /// `(768·var26)>>2`, 512 tall); each child is a full 512³ (10,0)
    /// flame inheriting the cloud's owner and yaw, keeping the (10,0)
    /// ctor's own life/damage.
    ///
    /// NOTE (SURVEY-MC1HW §7 — emit chain UNTRACED): the observable
    /// damage/duration follow the cloud's spawn params (`f44`/`max_life`)
    /// and WHICH creator HW's Fire Storm routes through (`sub_3B8E0`
    /// life-128/f44-100 vs `sub_3BC60` life-6/f44-3000), and whether HW
    /// spell-20 spawns a napalm cloud at all beside the homing meteor.
    /// This handler is faithful for any params; only the trigger is open.
    fn napalm_tick_hw(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // Class-10 PRE-decrement family (the sub_24F60..sub_26D20
        // batch law): the life test reads the value BEFORE the
        // decrement, so a 6-life cloud burns SEVEN ticks (pre-values
        // 6..0) — corpus-pinned on mc1hwl0 slot 522: every napalm
        // burst is 7 × 833 = 5831 per cloud, not 6 × 833.
        let pre = self.ent[i].act_life;
        self.ent[i].act_life = pre - 1;
        if pre < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 0x10002;
            self.snd(30, i);
        }
        let var26 = self.ent[i].f26;
        self.extents(i, 192u16.wrapping_mul(var26 as u16), 512);
        let amt = self.ent[i].f44 as u32 / self.ent[i].max_life.max(1);
        self.area_write(i, 0, amt, ctx, false, false);
        let (x, y, z, own, yaw) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30)
        };
        for (dx, dy) in self.ring_cells_pub(var26 as i32, var26 as i32) {
            let d1 = self.ent_rand(i);
            let d2 = self.ent_rand(i);
            let fx = x.wrapping_add((160 * dx as i32 + (d1 % 0x81) as i32 - 64 - 96) as u16);
            let fy = y.wrapping_add((160 * dy as i32 + (d2 % 0x81) as i32 - 64 - 96) as u16);
            if let Some(f) = self.spawn_effect(0, fx, fy, z) {
                let e = &mut self.ent[f];
                e.id24 = own;
                e.f30 = yaw; // child copies the cloud's yaw (var30)
                e.flags |= 0x10080;
                e.f80 = 512;
                e.f82 = 512;
                e.f84 = 512;
                e.f26 = 0;
            }
        }
        self.ent[i].f26 = (var26 + 2) % 7;
        false
    }

    /// sub_252D0 (:28199), class-10 state 6: the STANDING fire (tree
    /// burn / ground wave). The flame sprite family walks +86 up 7
    /// steps then back down over the last 12 ticks; the fire rides
    /// ground + f46 (3/4 up a burning tree's trunk), dies on water,
    /// and — while +18 bit0 (0x10000) is clear — broadcasts +44 ch0
    /// through the /10 tree-discount writer EVERY tick, so burning
    /// trees torch their neighbors (~5/tick) and forests chain-burn.
    /// Deviation: the original also spits a (10,13) smoke puff on
    /// 1/7 of shrink ticks — the LCG draw is kept for stream parity,
    /// the puff itself is skipped (decorative).
    fn standing_fire_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let pre = self.ent[i].act_life;
        self.ent[i].act_life = pre - 1;
        let mut done = pre < 0;
        if !done {
            // sub_44C10 player-distance bookkeeping omitted (HUD/AI).
            if self.ent[i].act_life < 12 {
                if self.ent[i].f26 > 0 {
                    self.ent[i].f26 -= 1;
                    self.ent[i].type86 -= 1;
                    if self.ent[i].flags & 0x80 == 0 {
                        let d = self.ent_rand(i);
                        if d % 7 == 0 {
                            // The (10,13) exhaust puff (:28224-33).
                            // +26 = 100 parks it PAST the tick's
                            // 16-tick drift window, so a fire's smoke
                            // rises straight and never walks its
                            // sprite up; life is overridden to 15
                            // (max_life keeps the ctor's roll) and the
                            // sprite starts two rows above the ctor's
                            // 67.
                            let (fx, fy, fz, own) = {
                                let e = &self.ent[i];
                                (e.x, e.y, e.z, e.id24)
                            };
                            if let Some(p) = self.spawn_effect(13, fx, fy, fz) {
                                let e = &mut self.ent[p];
                                e.f26 = 100;
                                e.act_life = 15;
                                e.id24 = own;
                                e.type86 = e.type86.wrapping_add(2);
                            }
                        }
                    }
                }
            } else if self.ent[i].f26 <= 6 {
                self.ent[i].f26 += 1;
                self.ent[i].type86 += 1;
            }
            let (x, y, f46) = {
                let e = &self.ent[i];
                (e.x, e.y, e.f46)
            };
            self.ent[i].z = (self.ground_z(x, y) as i16).wrapping_add(f46);
            if self.on_water_pub(x, y) {
                done = true;
            }
        }
        if done {
            self.ent[i].flags |= 0x400;
        }
        // The damage write runs even on the death tick (:28255-56
        // falls through LABEL_11).
        if self.ent[i].flags & 0x10000 == 0 {
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 0, amt, ctx, true, false);
        }
        false
    }

    /// sub_49890/499C0/49A50 (:57662-57790), class-2 model 0 — the
    /// TREE. State 0: ch0 intake; death sparks a (10,6) standing fire
    /// owned by the attacker, riding 3/4 up the trunk, with ONE
    /// tree-LCG draw setting rand%60+130 as BOTH the fire's life and
    /// the tree's burn timer; the tree goes un-hittable, state 1.
    /// State 1: burn down; below 60 → state 2 + the charred sprite
    /// (83→226, 84→227). All states follow the ground and splash-die
    /// on water. (Pool-full fire spawn skips the draw and retries
    /// next tick, as the original.)
    pub(crate) fn tree_tick(&mut self, i: usize, splash_water: bool) {
        match self.ent[i].tick70 {
            0 => {
                self.ent[i].flags |= 0x20000; // +18 |= 2 (:57674)
                if self.ent[i].mail[0].1 != 0 {
                    let (amt, src) = self.ent[i].mail[0];
                    self.ent[i].mail[0].1 = 0;
                    self.ent[i].act_life -= amt as i32;
                    if self.ent[i].act_life < 0 {
                        let (x, y, z, f84) = {
                            let e = &self.ent[i];
                            (e.x, e.y, e.z, e.f84)
                        };
                        if let Some(f) = self.spawn_effect(6, x, y, z) {
                            // The mailbox source is already the
                            // broadcaster's +24 owner; the original's
                            // second +24 hop is the identity for it.
                            self.ent[f].id24 = src;
                            self.ent[f].f46 = (3 * f84 as i32 / 4) as i16;
                            let d = self.ent_rand(i);
                            let burn = (d % 60 + 130) as i32;
                            self.ent[f].act_life = burn;
                            self.ent[i].act_life = burn;
                            self.ent[i].flags &= !8; // no longer hittable
                            self.ent[i].tick70 = 1;
                            // `sub_41CC0_42000(_, a1, a1 + 72)` (:57698,
                            // the fn's SOLE call site) — re-head the
                            // TREE so the flame, head-linked an
                            // instruction ago, paints after it and
                            // therefore in FRONT of it. Behaviour-inert
                            // here: the line above just cleared the
                            // tree's hittable bit, so the re-headed tree
                            // satisfies no scan predicate, and a relink
                            // preserves relative order for every other
                            // member of the tile.
                            self.relink_head(i);
                        }
                    }
                }
                self.tree_ground_water(i, splash_water);
            }
            1 => {
                self.ent[i].act_life -= 1;
                if self.ent[i].act_life < 60 {
                    self.ent[i].tick70 = 2;
                    match self.ent[i].type86 {
                        83 => self.set_sprite(i, 226),
                        84 => self.set_sprite(i, 227),
                        _ => {}
                    }
                }
                self.tree_ground_water(i, splash_water);
            }
            _ => self.tree_ground_water(i, splash_water),
        }
    }

    /// The tree handlers' shared tail (:57703-11): z follows the live
    /// ground; water under the trunk → splash (owner passed on) and
    /// despawn.
    fn tree_ground_water(&mut self, i: usize, splash_water: bool) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        if splash_water && self.on_water_pub(x, y) {
            let owner = self.ent[i].id24;
            let z = self.ent[i].z;
            if let Some(s) = self.spawn_effect(5, x, y, z) {
                self.ent[s].id24 = owner;
            }
            self.ent[i].flags |= 0x400;
        }
    }

    /// sub_49AA0_49DE0 / sub_49B50_49E90 (:57770/:57805), class-2
    /// states 3/9 — the standing stone and the bad stone: the static
    /// draw bit (+18 |= 2), then the per-tick terrain snap that rides
    /// deforming ground. No water arm — statics stand in the sea
    /// (only trees splash-die).
    pub(crate) fn static_snap_tick(&mut self, i: usize) {
        self.ent[i].flags |= 0x20000;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
    }

    /// sub_24F60 (:28047): the fire. One ch0 broadcast + terrain
    /// reaction on the first active tick, then flicker/anim out.
    fn fire_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].f26 & 3 != 0 {
            self.ent[i].f26 -= 1;
            return false;
        }
        // :28068-70 — PRE-decrement life test (class-10 family): every
        // fire burns one tick longer than the post form allowed.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        let mut dirty = false;
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            if self.ent[i].flags & 0x10000 == 0 {
                let amt = self.ent[i].f44 as u32;
                self.area_write(i, 0, amt, ctx, false, false);
            }
            // Terrain reaction (:28083-104): burn conversions, else a
            // small scorch crater on flat, low, dry ground.
            let (x, y, z) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            };
            let t = tile((x >> 8) as u8, (y >> 8) as u8);
            let ty = self.t.tile_type[t];
            let conv = match ty {
                26 => Some(0x14),
                10 => Some(0x15),
                11 => Some(0x16),
                _ => None,
            };
            if let Some(c) = conv {
                // The real sub_33800 paint call (:28086-92) — the
                // damage-stage TYPES come from PAINT_BC (10/11/12), NOT
                // the paint code (writing the code as the type =
                // wrong texture). a1/a2 are leftover registers in the
                // original; they only seed corner_orient ties.
                self.paint(0, 0, t, c);
                dirty = true;
            } else if !(6..=0x22).contains(&ty)
                && self.t.angle[t] & 7 != 1
                && (z as i32 - self.ground_z(x, y)) <= 128
                && !self.on_water_pub(x, y)
            {
                let d = self.ent_rand(i);
                self.dig_scorch(i, -((d % 7) as i16));
                dirty = true;
            }
            let d2 = self.ent_rand(i);
            self.ent[i].f46 = ((d2 % 0x41) as i32 - 32) as i16;
            self.snd(3, i); // :28118
        }
        // z rule sub_42000_42340 (:52576-601, called :28116 with
        // (ground, 0, 0, flicker)): ABOVE ground the fire drifts by
        // the fixed flicker delta each tick; below ground it clamps
        // UP to ground; at ground it stays. The original never pulls
        // a fire down to terrain — a midair explosion (max-range
        // fireball expiry, the meteor's trail) stays at altitude.
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let g = self.ground_z(x, y) as i16;
        if self.ent[i].z > g {
            self.ent[i].z = self.ent[i].z.wrapping_add(self.ent[i].f46);
        }
        if self.ent[i].z < g {
            self.ent[i].z = g;
        }
        self.anim_advance(i);
        dirty
    }

    /// sub_25130 (:28127): the fire-spreader — one ring of fires at
    /// radius +26 (0 = the single corpse flame), then gone.
    fn spreader_tick(&mut self, i: usize) -> bool {
        // :28142-48 — the life test reads the PRE-decrement value, so a
        // life-1 puff ticks TWICE before it is freed.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28149-53 — the `& 2` latch guards ONLY the one-shot sound.
        // The ring spawn below runs on EVERY tick, exactly as the
        // sibling blast_ring_tick does; hoisting the whole body under
        // this latch halved the corpse flame (one pass instead of two)
        // and with it every "castle as weapon" crush.
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.snd(3, i); // :28152
        }
        let (x, y, z, owner, aim, radius, inherit) = {
            let e = &self.ent[i];
            (
                e.x,
                e.y,
                e.z,
                e.id24,
                e.f30,
                e.f26.max(0) as i32,
                e.flags & 0x10000,
            )
        };
        let cells = self.ring_cells_pub(radius, radius);
        for (dx, dy) in cells {
            // :28161-63 — the per-cell draw is the SKIP TEST alone: spawn
            // iff `2·(v5 % 0x9D / 79) − 1 > 0`, i.e. `v5 % 157 >= 79`
            // (~50%). The `& 1` low-bit test picked a DIFFERENT set of
            // cells even for the same rand value.
            let s = self.ent_rand(i);
            if s % 0x9D < 79 {
                continue;
            }
            // :28165-70 — the jitter pair is drawn ONLY on the spawn
            // branch. Rolling it unconditionally (once per skipped cell
            // too) desynced the ring's rand stream, so every downstream
            // cell's skip decision — and the corpse-flame fire SET —
            // diverged from retail (57 missing / 210 extra (10,0)).
            let j1 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let j2 = (self.ent_rand(i) % 0x81) as i32 - 64;
            // x - 96 + 192·dx + jitter (:28167-70), 2x2-center recenter.
            let fx = x.wrapping_add((192 * dx as i32 + j1 - 96) as u16);
            let fy = y.wrapping_add((192 * dy as i32 + j2 - 96) as u16);
            if let Some(f) = self.spawn_effect(0, fx, fy, z) {
                self.ent[f].id24 = owner;
                self.ent[f].f30 = aim; // :28176 — inherit the spreader's f30
                self.ent[f].flags |= 0x80 | inherit;
            }
        }
        false
    }

    /// sub_25CE0 (:28671): the growing fire-ring blast — per-tick ch0
    /// at +44/maxLife, a ring of fires per tick, radius (+2) % 11.
    fn blast_ring_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28685-88 — the life test reads the PRE-decrement value, so the
        // ring runs one more pass than the post-decrement form allows.
        // Measured 9 -> 10 passes, 376 -> 417 fires; the per-tick ch0
        // write is f44/max_life, so the ring was landing 90% of its
        // authored damage.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2 | 0x10000;
            self.snd(30, i);
        }
        let radius = self.ent[i].f26.max(0) as i32;
        {
            // Half-extents 192·ring, z 512 (:28696-97) — no floor; the
            // AABB damage test sums both parties' extents, so ring 0
            // still hits a victim on the impact point.
            let e = &mut self.ent[i];
            e.f80 = (768 * radius / 4) as u16;
            e.f82 = e.f80;
            e.f84 = 512;
        }
        let per_tick = (self.ent[i].f44 as u32) / self.ent[i].max_life.max(1);
        self.area_write(i, 0, per_tick, ctx, false, false);
        let (x, y, z, owner) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        let _ = self.ent_rand(i); // pre-loop draw (:28699)
        let cells = self.ring_cells_pub(radius, radius);
        for (dx, dy) in cells {
            // x - 96 + 160·dx + rand%0x81 - 64 (:28707-09): the -96
            // recenters the ring table's 2x2 zero block.
            let j1 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let j2 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let fx = x.wrapping_add((160 * dx as i32 + j1 - 96) as u16);
            let fy = y.wrapping_add((160 * dy as i32 + j2 - 96) as u16);
            if let Some(f) = self.spawn_effect(0, fx, fy, z) {
                self.ent[f].id24 = owner;
                self.ent[f].flags |= 0x80 | 0x10000;
                self.extents(f, 512, 512);
                self.ent[f].f26 = 0;
            }
        }
        self.ent[i].f26 = ((radius + 2) % 11) as i16;
        false
    }

    /// sub_262D0 (:28898): the bolt hit-flash — one ch0 write and the
    /// thunder-crack 24 (:28911), brief.
    fn hit_flash_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28906-08 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28905 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 0, amt, ctx, false, false);
            self.snd(24, i);
            self.ent[i].act_life = 1;
        }
        self.anim_advance(i);
        false
    }

    /// sub_26360 (:28924): m11's mana-steal flash — one ch3 write.
    fn steal_flash_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28933-35 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28932 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 3, amt, ctx, false, false);
        }
        self.anim_advance(i);
        false
    }

    /// The merge partner search: MC1 `sub_11D10` (:17127) and MC2
    /// `sub_10A50` (EF:3876) are the SAME routine, and they are a
    /// **map-tile ring walk, not a pool scan**. Base tile =
    /// `((pos + 128) >> 8) & 0xFF` — ROUNDED, not floored; ring count
    /// = `(applied_pitch + 255) >> 8` (the searcher's own +80 extent
    /// in tiles, no `.max(1)` — the area writers' `.max(1)` is a
    /// different routine); tiles are visited ring by ring outwards
    /// (`sub_11410`/`sub_10080` seed the walker at ring 0, `sub_114B0`/
    /// `sub_10130` yield each ring's tile offsets) and each tile's
    /// `mapEntityIndex` chain is walked; the FIRST admissible hit
    /// wins and the walk stops.
    ///
    /// This is why the doomsday fountain's shore pile merges one tick
    /// LATER than a pool scan does: mc2l24 slot 845 (a settled sphere
    /// at 55.97/228.99, +80 = 112 ⇒ ring 1 around tile 56/229) does
    /// NOT see slot 795 when it steps to 54.98/227.98 (tile 54/227 is
    /// outside the ring) even though the AABBs already overlap, and
    /// absorbs it only at 55.23/228.23 (tile 55/228). The pool scan
    /// merged it a tick early — a `missing:10,39` every time.
    ///
    /// Retail's admission is `+66/+67` (`filter_admits`) + `id !=
    /// id` + the AABB; every ball ctor stamps `xtype/xsubtype` =
    /// (10,39), so the explicit family test below IS that filter and
    /// keeps working for native balls (which carry no +66/+67). The
    /// port-only exclusions (fool's-mana spheres, soft-killed
    /// records) ride along.
    ///
    /// RESIDUAL: the order WITHIN one ring comes from a data table
    /// (`bitmaps_E9980x`) the decompile does not carry; raster order
    /// stands in. It only decides which of two simultaneously
    /// overlapping partners is absorbed first.
    fn ball_merge_candidates(
        &self,
        i: usize,
        decaying: bool,
        grounded: bool,
        is_fool: bool,
    ) -> Vec<usize> {
        let mut out = Vec::new();
        if decaying || !grounded || is_fool {
            return out;
        }
        if no_ball_merge_fix() {
            // The pre-dig arm (A/B only): a whole-pool scan in slot
            // order.
            out.extend(1..self.ent.len());
            out.retain(|&j| {
                j != i
                    && self.ent[j].class64 == 10
                    && self.ent[j].model65 == 39
                    && self.ent[j].tick70 != 62
                    && self.ent[j].flags & 0x400 == 0
            });
            return out;
        }
        let (bx, by, rings) = {
            let e = &self.ent[i];
            (
                ((e.x as u32 + 128) >> 8) as u8,
                ((e.y as u32 + 128) >> 8) as u8,
                ((e.f80 as i32 + 255) >> 8).max(0),
            )
        };
        for ring in 0..=rings {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dy.abs()) != ring {
                        continue;
                    }
                    let tx = (bx as i32 + dx) as u8;
                    let ty = (by as i32 + dy) as u8;
                    let mut j = self.map_entity[tile(tx, ty)] as usize;
                    while j != 0 {
                        let c = &self.ent[j];
                        let next = c.next20 as usize;
                        if j != i
                            && c.class64 == 10
                            && c.model65 == 39
                            && c.tick70 != 62
                            && c.flags & 0x400 == 0
                        {
                            out.push(j);
                        }
                        j = next;
                    }
                }
            }
        }
        out
    }

    /// sub_27030 (:29416): the mana ball — claim intake, launch-arc
    /// physics (gravity 16, quarter-bounce, 250/256 friction, ±64
    /// clamp), merge on overlap (sub_277D0 :29700).
    fn ball_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // A ball absorbed by an earlier slot THIS tick is already
        // despawning — without this guard two coincident balls merge
        // into each other mutually and the mana vanishes (retail's
        // merged ball is display-disabled and can't re-merge).
        if self.ent[i].flags & 0x400 != 0 {
            return false;
        }
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        // MC2 stall skip (retail byte[1] & 8 → import bit 26,
        // EF:26062-65): a one-shot whole-tick skip — intakes, modes
        // and the decay tail included. Native MC2 never arms it on
        // spheres; the conformance import carries it.
        if mc2 && self.ent[i].flags & (1 << 26) != 0 {
            self.ent[i].flags &= !(1 << 26);
            return false;
        }
        // MC2 (10,57) — the RANDOM-VALUE sphere. Its retail tick is
        // `sub_35FB0` (EF:26318), NOT the (10,39) ball's
        // `TransformArcherToMana_35940` (EF:26015), and the two differ
        // in exactly one place: the claim intake. The ball TRANSFERS
        // ownership (EF:26069-94, the arm below); the (10,57) instead
        // runs the FOOL'S-MANA trap
        //
        //     else if (w68 && sub_36680(a1x))          // EF:26362
        //     { _4A190(&pos, 10, 0); DisableEntityDrawing04(a1x); }
        //
        // — the retaliation homes the claimer and the sphere is
        // consumed with a (10,0) poof (docs/spell-audit/fools-mana.md
        // §2b). There is NO owner precondition in `sub_36680`: the ONLY
        // skip is `parentId == claimer`, so an AUTHORED ground sphere
        // (parentId 0, `byte_0x46_70` = the NewEvent default 0) is a
        // live tier-0 trap for everyone. mc2l24 proves it end to end —
        // all 21 authored start spheres carry b46=0/owner28=0, and each
        // one dies the tick after the human's (10,12) possess pulse
        // stamps w68=116, leaving a co-located (10,0) poof and a (9,0)
        // fireball with `word_0x96_150 = 116` (homing the player).
        //
        // Retail field homes, all of them already carried by the
        // conformance importer: tier = f71 (@0x46), payload = f44
        // (@0x2A), counter = f26 (@0x10), parentId = id24 (@0x28 fused),
        // and the claim LATCH is the ch1 mail source itself (@0x68) —
        // `sub_36680` clears it only on the owner branch, so a mid-trap
        // sphere stays latched (and frozen: retail's else-if means a
        // claimed sphere runs no physics that tick either way).
        //
        // Discriminator: retail's m57 ctor `sub_50130` stamps action
        // 0x3E while every other sphere takes 0x29; the port's native
        // spawner keeps the (10,39) family model but now carries that
        // action, so `model 57 || action 62` covers both the imported
        // and the native sphere. MC1 balls are action 41 → untouched.
        let is_fool = mc2 && (self.ent[i].model65 == 57 || self.ent[i].tick70 == 62);
        if is_fool && self.ent[i].mail[1].1 != 0 {
            if self.mc2_fools_retaliate(i, ctx) {
                // EF:26363-65: the consume poof, then the soft kill
                // (tick-top reap) — the sphere survives this tick in
                // the pool exactly as retail's disabled entity does.
                let (x, y, z) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                self.mc2_spawn_fire(x, y, z);
                self.ent[i].flags |= 0x400;
            }
            // Claimed → retail's else-if never reaches the mover.
            return false;
        }
        // ch1 collection claim (:29439-45): the ball takes the
        // claimant as owner — only on an owner CHANGE (the possess
        // flash re-broadcasts for 8 ticks; the guard keeps the claim
        // chime single). The mail AMOUNT is MC2 retail's
        // `dword_0x64_100` force flag (the ball twin EF:26069-94,
        // byte-for-byte the house protocol): a FORCED claim (Mana
        // Lock's (10,70) pulse) steals unconditionally and sets the
        // claim lock; a weak claim bounces off a locked ball. MC1 has
        // no forced writer, so its balls never lock — every MC1 claim
        // runs the weak arm exactly as before.
        if !is_fool && self.ent[i].mail[1].1 != 0 {
            let (force, src) = self.ent[i].mail[1];
            self.ent[i].mail[1] = (0, 0);
            if src != self.ent[i].f144
                && (force != 0 || self.ent[i].flags & crate::mc2::mobs::F_CLAIM_LOCK == 0)
            {
                self.ent[i].f144 = src;
                self.ent[i].flags &= !0x40;
                if force != 0 {
                    self.ent[i].flags |= crate::mc2::mobs::F_CLAIM_LOCK;
                }
                // The chime anchors at the CLAIMANT, not the ball
                // (:29444 sub_55370(claimant, -1, 4)) — the player-
                // gated id 4 is heard exactly when YOU claim.
                if src == crate::mc1::mobs::PLAYER_TARGET {
                    self.snd_player(4);
                }
                // A settled ball (+58 == 0) never reaches the tick's
                // re-derive below, so the intake recolors in place —
                // retail re-derives every tick (:29569).
                self.ball_resize(i);
            }
        }
        // ch4 attract (:29451-62): the (10,54) magnet tagged this
        // ball (+118 = magnet slot, the ch4 mail source: +114/+118
        // ARE the channel-4 amount/source pair, +90+6·4/+94+6·4) —
        // aim at it and add a magnitude-4 impulse onto the velocity
        // accumulator, then acknowledge. Against the ±64 clamp and
        // 250/256 friction below this shapes the retail stream. The
        // pull NEVER claims (the ch4 amount is read by nothing;
        // player-confirmed): claim = the bolt's localized impact
        // flash + the merge's owned-beats-unowned adoption.
        let mut kicked = false;
        if self.ent[i].mail[4].1 != 0 {
            let m = self.ent[i].mail[4].1 as usize;
            self.ent[i].mail[4] = (0, 0);
            // Retail MC2's ch4 intake (w7A, EF:26097-110) forces one
            // moving tick even on a settled sphere (the v35 latch).
            kicked = true;
            if m < self.ent.len() {
                let (bx, by) = (self.ent[i].x, self.ent[i].y);
                let (mx, my) = (self.ent[m].x, self.ent[m].y);
                // Mask to 0..2047 — `angle_of` can return 2048 (full-
                // turn wrap) and SIN/COS are len 2048.
                let dir = (Self::angle_between(bx, by, mx, my) & 0x7FF) as usize;
                let ivx = ((4 * crate::mc1::tables::SIN[dir]) >> 16) as i16;
                let ivy = (-((4 * crate::mc1::tables::COS[dir]) >> 16)) as i16;
                let e = &mut self.ent[i];
                e.dest_x = (e.dest_x as i16).wrapping_add(ivx) as u16;
                e.dest_y = (e.dest_y as i16).wrapping_add(ivy) as u16;
            }
        }
        // MC2's HOMING intake (`word_0x7A_122`, EF:26097-110) — the
        // (10,54) aura's half of the one-tick handshake. Retail stamps
        // every unclaimed sphere in range EVERY tick (sub_38D80
        // EF:28364-75, `if (!w7A)`) and the SPHERE clears the stamp
        // here, at the head of its own tick, latching `v35` — which is
        // what lets a pull drag a sphere whose settle counter has
        // already run out (the moving gate is `byte_0x39_57 || v35`,
        // EF:26173). The port's field home for +122 is the aura claim
        // map, and the aura collapses the +118 pull speed into the
        // dest velocity it writes there (documented in
        // [`Self::mc2_aura_tick`]).
        //
        // ⚠ Releasing HERE and not on the moving tail is the whole
        // fix: a settled sphere returns early below, so a tail-only
        // release left the claim latched forever — the aura then
        // skipped that sphere for the rest of the level and the mana
        // stopped dead short of the eye, twitching back to life only
        // when the player wandered close enough for the awake pass to
        // re-arm +58. That is precisely the reported regression.
        if mc2 && self.mc2_aura_claim.0.remove(&(i as u16)).is_some() {
            kicked = true;
        }
        // Collector tether (flag 0x40): the ball FLIES to its
        // collector (+146) instead of running ground physics
        // (:29464-90; the MC2 twins EF:26111-72 for the (10,39) ball
        // and EF:26385-447 for the (10,57) sphere are the same code).
        // Every tethered tick re-arms the +46 lift at 128 (the release
        // pop) and turns +30 to the collector; ≥16 out the ball steps
        // horizontally at 16/tick, under 16 it snaps over the
        // collector and z-servos into the hover band [collector z,
        // +512]: +step/tick from below, −step/tick from more than 512
        // ABOVE — without the descend arm an overhead ball deadlocks
        // the pickup (the balloon parks under it forever). Ground-
        // clamped; the band sits inside the absorb window (balloon
        // half-height 400), so the collector side's ent_overlap
        // finishes the pickup. Past 1024 the ball drops the tether
        // itself; a tethered tick never runs ball physics (retail's
        // else-if), even on the tick the tether clears. The reach
        // test is retail's `EuclideanDistXYZ_58490`, which despite
        // its name sums X and Y ONLY (utilities/Maths.cpp:738) — a
        // grounded ball under a hovering collector is "at" it and
        // z-servos up.
        //
        // Retail admits exactly TWO collector kinds (EF:26115-27) and
        // drops the grab for anything else:
        //   - the (3,3) mana balloon → z step 32, a constant;
        //   - the MC2 (5,23) mana leviathan → z step = the COLLECTOR's
        //     own `word_0x2C_44`, the siphon ramp its arm seeds at 18
        //     and bumps +10 every tick it holds the grab (:18238,
        //     :18270), so a siphoned ball accelerates upward until the
        //     dweller's swallow test fires. Our column homes retail's
        //     0x2C at f46 on SPHERES (the launch/gravity lane) but at
        //     f44 on class-5 creatures (mc2/mobs.rs field map), so the
        //     cross-read below is f44. MC2-only: MC1's ball tick has
        //     no leviathan.
        if self.ent[i].flags & 0x40 != 0 {
            let b = self.ent[i].f146 as usize;
            let live = b != 0 && b < self.ent.len() && self.ent[b].flags & 0x400 == 0;
            let step = if live && self.ent[b].class64 == 3 && self.ent[b].model65 == 3 {
                Some(32i16)
            } else if live && mc2 && self.ent[b].class64 == 5 && self.ent[b].model65 == 23 {
                Some(self.ent[b].f44 as i16)
            } else {
                None
            };
            if let Some(step) = step {
                self.ent[i].f46 = 128;
                let (bx, by, bz) = {
                    let e = &self.ent[b];
                    (e.x, e.y, e.z)
                };
                let mut pos = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                let yaw = Self::angle_between(pos.0, pos.1, bx, by);
                self.ent[i].f30 = yaw;
                let d = Self::isqrt(Self::dist2_sq(pos.0, pos.1, bx, by) as u32) as i32;
                if d <= 1024 {
                    if d >= 16 {
                        Self::polar_step(&mut pos, yaw, 0, 16);
                    } else {
                        pos.0 = bx;
                        pos.1 = by;
                        if pos.2 as i32 >= bz as i32 {
                            if pos.2 as i32 > bz as i32 + 512 {
                                pos.2 = pos.2.wrapping_sub(step);
                            }
                        } else {
                            pos.2 = pos.2.wrapping_add(step);
                        }
                    }
                    let ground = self.ground_z(pos.0, pos.1) as i16;
                    if ground > pos.2 {
                        pos.2 = ground;
                    }
                    self.move_relink(i, pos.0, pos.1, pos.2);
                } else {
                    self.ent[i].flags &= !0x40; // strayed: the ball side lets go
                }
            } else {
                self.ent[i].flags &= !0x40; // dangling tether
            }
            self.ball_resize(i);
            return false;
        }
        // The ballistic arm is `else if (+58)` (sub_27030 :29518): +58
        // is the fresh-spawn countdown (ctor 0x80) that the global
        // anim pass sub_54F00_55430 → sub_54F80 (:64318-20) steps down
        // once per tick, so a ball runs physics — gravity, downhill
        // roll, and the grounded MERGE scan — for its first 128 ticks
        // and then freezes WHEREVER IT IS for good (the corpus pins
        // it: a ball settles at spawn+128 and then ignores
        // overlapping live neighbors indefinitely). A ball still
        // mid-hop on a long slope at expiry hangs in the AIR — in
        // retail too (the whole body is behind the +58 gate and the
        // anim pass :64318 only decrements; no ground snap ever
        // reaches an expired ball). Player-observed on worm-death
        // balls down a hillside 2026-07-30: FAITHFUL, not a
        // deviation. MC1's decrement lives in mob_awake_pass (the
        // sub_54F00 port — which also RE-ARMS a settled ball to 16
        // near the human, the wake law): this handler gates on the
        // post-maintenance value exactly like retail's else-if, so
        // each 17-tick wake cycle moves 16 and freezes 1. Byte
        // semantics: retail reads +58 as a raw byte (the import
        // widens i8, so 0x80 arrives as -128). MC2's maintenance twin
        // is `sub_68C70` via `sub_68BF0`'s SECOND loop over the sphere
        // chain `dword_38523` (EF:55489-90), ported as the sphere leg
        // of [`Gen::mc2_awake_pass`] — it owns the decrement AND the
        // proximity re-arm, exactly like MC1's. This handler only
        // READS +58 (EF:26173); the sphere tick never writes it.
        let settle = (self.ent[i].f58 & 0xFF) as u8;
        if !mc2 {
            if settle == 0 {
                // Settled balls TRACK the ground (patch option
                // `ball_ground_track`, player-ruled — DEVIATIONS.md):
                // retail's freeze leaves a mid-hop ball hanging in
                // the air forever and lets terrain edits (volcano,
                // castle stamps) BURY a grounded one. Both directions
                // on purpose. Retail arm / conformance replay keep
                // the freeze. MC1-native only.
                if ctx.patches.ball_ground_track && !ctx.strict {
                    let (x, y) = (self.ent[i].x, self.ent[i].y);
                    let g = self.ground_z(x, y) as i16;
                    self.ent[i].z = g;
                }
                return false;
            }
        } else {
            // The MC2 settle law is the SAME shape at a different
            // home: TransformArcherToMana's whole moving body sits
            // behind `byte_0x39_57 || fresh-kick` (EF:26173), the
            // ctor seeds @0x39 = 128 (CreateManaSphere EF:36617 —
            // the port ctor's f58 = 0x80), and `mc2_awake_pass`'s
            // sphere leg steps it 1/tick to 0 (mc2l4 corpus: b39
            // 36→0, then f2c parks at −16 and the sphere never moves
            // again) — then re-arms it to 16 inside 24 tiles of the
            // player, same law as MC1's. The port previously ran
            // always-on physics here, dropping every authored
            // economy sphere to the pristine ground (the mc2l4
            // (10,39) z family). No ground-track deviation for MC2:
            // frozen means frozen, both modes. A settled decaying
            // sphere still runs the decay tail (EF:26289 sits
            // outside the mode branch).
            if settle == 0 && !kicked {
                self.ball_decay_tail(i);
                return false;
            }
        }
        let mut vx = self.ent[i].dest_x as i16;
        let mut vy = self.ent[i].dest_y as i16;
        vx = vx.clamp(-64, 64);
        vy = vy.clamp(-64, 64);
        let (x0, y0, z0) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let x = x0.wrapping_add(vx as u16);
        let y = y0.wrapping_add(vy as u16);
        let ground = self.ground_z(x, y) as i16;
        // Vertical (:29532-37 / EF:26188-91 — the twins are verbatim):
        // z steps by the +46 lift and gravity integrates EVERY moving
        // tick; there is no at-rest gate. The strict below-ground
        // clamp is what keeps a resting ball's observable z pinned
        // while its lift cycles 0 → −16 → 0 underneath.
        let mut z = z0.wrapping_add(self.ent[i].f46);
        self.ent[i].f46 = (self.ent[i].f46 - 16).max(-128);
        // Clamp + rebound ONLY when the step went STRICTLY below the
        // ground (`tempV13 > z` :29538 / `v22 > z` EF:26244): a ball
        // landing EXACTLY on it keeps its falling lift one more tick
        // and flips on the next. The mc1l0 replay corpus pins the
        // phase — the authored balls all fall 128-multiples onto flat
        // ground, and a `<=` clamp flips them one tick early (the
        // replay t=2 z+32 cohort; per-pair verify can never see it,
        // the import restores retail's +46 each pair). Rebound =
        // −impact/4 truncating, zeroed at ≤ 16 (:29542-49 /
        // EF:26244-52, the same formula in both binaries; the old
        // MC1-only `< -64` arm was the equivalent form for falls but
        // wrong for the climb-into-terrain case).
        if z < ground {
            z = ground;
            let v = self.ent[i].f46;
            let nb = -(v / 4);
            self.ent[i].f46 = if nb <= 16 { 0 } else { nb };
        }
        // Grounded contact = post-clamp z ON the ground (`tempV13 ==
        // z` :29552 / `v22 == predicted.z` EF:26265) — true on an
        // exact landing too: the corpus rolls and frictions on the
        // landing tick (ball 223's +150 accumulator moves at t=1).
        let grounded = z == ground;
        // Downhill roll + friction — GROUNDED only, both games (MC1
        // sub_27030's `tempV13 == z` branch :29556-64 via
        // sub_41F50_42290 :52547; MC2 `sub_58030` inside
        // `TransformArcherToMana`'s `v22 == z` branch): a resting ball
        // takes the terrain gradient onto its velocity, so balls
        // stream down slopes and pool in basins. The helper is the
        // same RAW-heightmap forward difference over the ball's 2×2
        // tile quad in both binaries, added un-divided (a height byte
        // ≈ 32 world units), then the 250/256 friction. Airborne balls
        // keep their velocity. (An earlier port arm gave MC1
        // unconditional friction and no roll — contradicted by its own
        // source cite and by the retail corpus's rolling balls.)
        if grounded {
            let (tx, ty) = ((x >> 8) as u8, (y >> 8) as u8);
            let h = |dx: u8, dy: u8| {
                self.t.height[tile(tx.wrapping_add(dx), ty.wrapping_add(dy))] as i32
            };
            let sx = h(0, 0) - h(1, 0) + h(0, 1) - h(1, 1);
            let sy = h(0, 0) + h(1, 0) - h(0, 1) - h(1, 1);
            vx = ((vx as i32 + sx) * 250 / 256) as i16;
            vy = ((vy as i32 + sy) * 250 / 256) as i16;
        }
        self.ent[i].dest_x = vx as u16;
        self.ent[i].dest_y = vy as u16;
        if (x, y, z) != (x0, y0, z0) {
            self.move_relink(i, x, y, z);
        }
        // Merge with an overlapping ball: absorb, despawn the other.
        // A DECAYING ball (the apocalypse-rain channel below) never
        // INITIATES a merge (EF:26268 gates `sub_36D50` on
        // `!(byte[1] & 0x20)`) — but a live ball may still absorb
        // it, which is retail's own mana-retention loophole (magnet/
        // balloon consolidation into a permanent sphere).
        let decaying = self.ent[i].flags & 0x2000 != 0;
        // BOTH games scan for a partner only inside the grounded
        // branch: MC1 `tempV13 == z` (:29552-55), MC2 `v22 ==
        // predicted.z` (EF:26265-69 — `sub_10A50` + `sub_36D50` sit
        // inside the rest-contact arm). An airborne or arcing ball
        // never initiates a merge; a kill's spawn scatter coalesces
        // only as the balls land, one merge per grounded tick.
        for j in self.ball_merge_candidates(i, decaying, grounded, is_fool) {
            if self.ent_overlap(i, j) {
                let (fi, fj) = (self.ent[i].f140, self.ent[j].f140);
                // MC2 owner rule (retail `sub_36D50` EF:26919): the
                // surviving ball takes the OWNER (colour) of the larger
                // contributor — an unowned ball defers to an owned
                // partner, two owned balls resolve to the bigger (NOT
                // the survivor's own owner, which colours a merged ball
                // as "the last ball merged"). (Retail breaks the
                // owned-vs-owned tie on the owner wizards' maxMana; ball
                // mana is the observable proxy and is what the
                // single-owner economy levels turn on.)
                if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
                    let (oi, oj) = (self.ent[i].f144, self.ent[j].f144);
                    let winner = if oi == 0 {
                        oj
                    } else if oj == 0 {
                        oi
                    } else if fj > fi {
                        oj
                    } else {
                        oi
                    };
                    self.ent[i].f144 = winner;
                    // Mana Lock across merges (EF:26936-40): ONLY the
                    // unclaimed-survivor arm carries the absorbed
                    // ball's claim lock — a gathered pile stays
                    // locked; every other merge arm lets the
                    // despawned ball's lock die with it (this churn
                    // is why retail locks LOOK timed in play).
                    if oi == 0 && oj != 0 && self.ent[j].flags & crate::mc2::mobs::F_CLAIM_LOCK != 0
                    {
                        self.ent[i].flags |= crate::mc2::mobs::F_CLAIM_LOCK;
                    }
                } else {
                    // MC1 owner rule (`sub_277D0` :29700): OWNED BEATS
                    // UNOWNED — an unowned survivor ADOPTS the absorbed
                    // ball's owner (:29717; this is how magnet-pulled
                    // balls become claimed as they coalesce into the
                    // claimed one). A class-10 owner (a grave's bank
                    // tag) loses to a real owner (:29734-50); two
                    // DIFFERENT real owners contest on the owner
                    // wizards' +136 (:29755-73: strictly larger keeps
                    // the survivor's owner, else the absorbed side
                    // wins). Port note: MC1 wizard ents don't carry a
                    // +136 bank (only castles do) and the human has no
                    // pool entity, so both sides resolve 0 and the
                    // contest falls to retail's else-arm (absorbed
                    // side's owner) — structure faithful, operands
                    // approximated. Mana is ALWAYS additive: the
                    // reconstruction's two `*=` branches (:29750,
                    // :29773) are transcription slips (every sibling
                    // branch is `+=`).
                    let (oi, oj) = (self.ent[i].f144, self.ent[j].f144);
                    let is_c10 = |g: &Self, o: u16| {
                        o != crate::mc1::mobs::PLAYER_TARGET
                            && (o as usize) < g.ent.len()
                            && g.ent[o as usize].class64 == 10
                    };
                    let w136 = |g: &Self, o: u16| {
                        if o != crate::mc1::mobs::PLAYER_TARGET && (o as usize) < g.ent.len() {
                            g.ent[o as usize].f136
                        } else {
                            0
                        }
                    };
                    if oi == 0 {
                        self.ent[i].f144 = oj;
                    } else if oj != 0 && oi != oj {
                        let (ci, cj) = (is_c10(self, oi), is_c10(self, oj));
                        // Two distinct retail branches that share an
                        // outcome: the class-10-loses arm and the
                        // lost +136 contest — kept separate to match
                        // the trace.
                        #[allow(clippy::if_same_then_else)]
                        if ci && !cj {
                            self.ent[i].f144 = oj;
                        } else if !ci && !cj && w136(self, oi) <= w136(self, oj) {
                            self.ent[i].f144 = oj;
                        }
                    }
                }
                self.ent[i].f140 = fi + fj;
                // MC1's sub_277D0 frees the absorbed ball through
                // sub_41E90_421D0 (:52514-20) — the HARD free (unlink,
                // class 0, slot straight back on the stack), not the
                // 0x400 soft-kill: the donor is gone from the very
                // snapshot the merge lands in (pair 11→12 of the mc1l0
                // corpus: retail's 485 absorbs 479 and 479 is absent
                // at t=12; a soft-killed donor lingers to the sweep
                // and reads as extra-in-port).
                //
                // MC2'S TWIN IS THE SAME LAW. `sub_36D50` (EF:26919-
                // 26996) is a ladder of owner-resolution arms and
                // EVERY one of them ends `return sub_57F20(a2x)` —
                // and `sub_57F20` (Events.cpp:5209-39) is the hard
                // free: tile unlink, recycle-stack swap-removal,
                // `class = 0`, free-stack push. Nothing defers it to
                // the disable sweep. Corpus proof (mc2l24, the
                // doomsday fountain): the permanent shore sphere in
                // slot 845 absorbs the arriving rain — mana 141653 →
                // 143966 across t=64510 while slot 795 (mana 2313) is
                // ABSENT from the t=64511 snapshot, and again +78 as
                // slot 828 vanishes at t=64512. A soft-killed donor
                // would have lingered one snapshot AND withheld its
                // slot, which is exactly the extra-in-port the
                // fountain window measured.
                if mc2 && no_ball_merge_fix() {
                    self.ent[j].flags |= 0x400; // the pre-dig MC2 arm (A/B only)
                } else {
                    self.free_entity(j);
                }
                break;
            }
        }
        // Size re-derivation every tick (:29569) — merged/claimed
        // balls visibly grow/recolor in the original. MC2's derive
        // is gated off while decaying (EF:26286 `!(byte[1] & 0x20)`;
        // the owner-change intake above recolors regardless — the
        // v36 arm).
        if !(mc2 && decaying) {
            self.ball_resize(i);
        }
        self.ball_decay_tail(i);
        false
    }

    /// The apocalypse-rain DECAY channel (`byte[1] |= 0x20` — port
    /// flag bit 13; the MC2 sphere mover's tail, EF:26289-307): the
    /// timed sphere counts its life down — at 12 the 67% death-fade
    /// bit (24) arms, at 6 it swaps to the bit-23 ghost, at 0 it
    /// expires. Only the doomsday mana rain (mc2::morph summit91)
    /// and the conformance import set the bit, so MC1 and ordinary
    /// spheres never enter; a balloon tether returns before this
    /// tail, reproducing retail's pickup-retains-the-ball behavior.
    /// Runs for SETTLED spheres too — retail's tail sits outside the
    /// mode branch.
    fn ball_decay_tail(&mut self, i: usize) {
        if self.ent[i].flags & 0x2000 == 0 {
            return;
        }
        self.ent[i].act_life -= 1;
        let l = self.ent[i].act_life;
        if l < 6 {
            if l == 0 {
                self.ent[i].flags |= 0x400;
            }
        } else if l == 6 {
            self.ent[i].flags = (self.ent[i].flags | 1 << 23) & !(1 << 24);
        } else if l == 12 {
            self.ent[i].flags |= 1 << 24;
        }
    }

    // ---- corpse pipeline ----------------------------------------------------

    /// The CorpseVerb seam (crate::verbs): MC1 scatters mana
    /// balls/jars. MC2's death drops (spell tokens, mana-sphere
    /// split/merge) live in the mc2 death handlers, which do not
    /// route through here — an MC2 world reaching THIS drop serves
    /// the MC1 scatter and says so in telemetry.
    pub(crate) fn corpse_drop(&mut self, i: usize) {
        match self.verbs.corpse {
            CorpseVerb::Mc1 => self.corpse_drop_mc1(i),
            CorpseVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Corpse);
                self.corpse_drop_mc1(i);
            }
        }
    }

    /// sub_27690 (:29663): the corpse's mana-ball drop — one unused
    /// draw on the CORPSE's seed (kept for stream parity), then the
    /// ball with two launch draws on its OWN seed.
    fn corpse_drop_mc1(&mut self, i: usize) {
        if self.ent[i].f140 <= 0 {
            return;
        }
        let _ = self.ent_rand(i); // :29674 — result unused, draw kept
        let (x, y, z, heading, mana, owner) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f140, e.f144)
        };
        if let Some(b) = self.spawn_mana_ball(x, y, z) {
            self.ent[b].f140 = mana;
            self.ent[b].f144 = owner;
            let d1 = self.ent_rand(b);
            let yaw = ((d1 % 0x71) as i32 - 56 + heading as i32) as u16 & 0x7FF;
            let d2 = self.ent_rand(b);
            let speed = (d2 % 0x30 + 16) as i16;
            self.ent[b].f30 = yaw;
            self.ent[b].f34 = yaw;
            let vx = ((speed as i32 * crate::mc1::tables::SIN[yaw as usize]) >> 16) as i16;
            let vy = (-((speed as i32 * crate::mc1::tables::COS[yaw as usize]) >> 16)) as i16;
            self.ent[b].dest_x = vx as u16;
            self.ent[b].dest_y = vy as u16;
            let ground = self.ground_z(x, y) as i16;
            self.ent[b].f46 = (1024 - (z.wrapping_sub(ground)) as i32).max(0) as i16 >> 3;
        }
        self.ent[i].f144 = 0;
    }

    /// The corpse's death-flame puff: class-10 m1 at radius 0 with
    /// +24 = the corpse (:21866).
    pub(crate) fn corpse_puff(&mut self, i: usize) {
        let (x, y, z, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        if let Some(p) = self.spawn_effect(1, x, y, z) {
            self.ent[p].id24 = id;
            self.ent[p].f26 = 0;
        }
    }

    // ---- helpers over private feature internals ------------------------------

    /// Ring cell offsets for radius lo..=hi — the real SEARCH.DAT
    /// ring table (the original's precomputed rings, row-major
    /// emission order + the dropped-last-cell quirk, features.rs
    /// `ring_cells`), sign-extended for unit-space scaling. The retail
    /// rings are ROUND (not a Chebyshev box = a square blast);
    /// tile-space callers (dig_disc) keep the raw u8 deltas and wrap
    /// mod 256.
    fn ring_cells_pub(&self, lo: i32, hi: i32) -> Vec<(i8, i8)> {
        self.ring_cells(lo, hi)
            .into_iter()
            .map(|(dx, dy)| (dx as i8, dy as i8))
            .collect()
    }

    /// The fire's scorch dig (sub_40D30(expl, 0, 0, -depth, 1)):
    /// a single-cell protected dig at the fire's position. Also the
    /// MC2 fire's (sub_30D50 → sub_572C0 — same chassis shape).
    pub(crate) fn dig_scorch(&mut self, i: usize, delta: i16) {
        if delta == 0 {
            return;
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let _ = self.dig_cell_pub((x >> 8) as i16, (y >> 8) as i16, delta, true);
    }
}

// Global-stream helper kept close to the module using it.
#[allow(dead_code)]
pub(crate) fn global_draw(rand: &mut u32) -> u32 {
    lcg32(rand)
}
