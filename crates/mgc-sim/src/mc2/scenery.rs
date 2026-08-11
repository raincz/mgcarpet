//! MC2 class-2 scenery — models 3..=8 and the tree (2,0)
//! lifespan/burn ticks. Trace bank:
//! docs/traces/mc2-class5-m25-26-28-class2-treeburn.md (`EF:` =
//! remc2 EventsFunctions.cpp).
//!
//! - (10,5) splash, (10,13) debris smoke and the (10,6) tree flame
//!   ride their real ported creators (effects.rs `mc2_spawn_fire6`,
//!   docs/traces/mc2-class10-m6-m9-m11-m28-m31.md §1).
//! - Models 7/8's terminal behavior is despawn (states 19/27 are goto
//!   labels).
//! - Model 6 (cave bee) is cave-gated: off-cave the ctor returns None
//!   (retail's own off-cave arm).

use crate::engine::features::Gen;

impl Gen {
    // ---- ctors (models 3-8) --------------------------------------------------

    /// `sub_4AE80` (EF:33503) — static prop (2,3): action 9,
    /// half-speed sprite 270, non-collidable.
    pub(crate) fn mc2_spawn_scenery3(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = 3;
            e.tick70 = 9;
            e.flags &= !8;
            e.f26 = (i % 11) as i16;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 270);
        Some(i)
    }

    /// `sub_4AF00` / `sub_4AF70` (EF:33521/:33538) — pure statics
    /// (2,4)/(2,5): sprite 48, no-op ticks, collidable.
    pub(crate) fn mc2_spawn_scenery45(
        &mut self,
        model: u8,
        x: u16,
        y: u16,
        z: i16,
    ) -> Option<usize> {
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = model;
            e.tick70 = if model == 4 { 12 } else { 15 };
            e.f26 = (i % 11) as i16;
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, 48);
        Some(i)
    }

    /// `sub_4AFE0` (EF:33555) — the cave bee (2,6): CAVE-ONLY. A
    /// passive, ground-pinned, non-flying, non-attacking, SILENT
    /// sprite — only a damage TARGET; no behavior row, no aggro
    /// (roster trace §1). 4 RNG draws: life 100..179, ±32 x/y
    /// scatter, sprite 324..327. Keeps the NewEvent targetable bit.
    pub(crate) fn mc2_spawn_cave_bee(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        if !self.is_cave() {
            return None; // (:33561)
        }
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = 6;
            e.tick70 = 18;
            e.f28 = 1; // byte_0x38_56 = 1 (ch0; model != 0 ⇒ FULL damage, EF:4273)
            e.f56 = 1;
        }
        let d = self.mc2_rand(i);
        self.ent[i].max_life = d % 0x50 + 100;
        let jx = ((self.mc2_rand(i) & 0x3F) as i32 - 32) as i16;
        let jy = ((self.mc2_rand(i) & 0x3F) as i32 - 32) as i16;
        self.link(i, x.wrapping_add(jx as u16), y.wrapping_add(jy as u16), z);
        self.refill_life(i);
        let d = self.mc2_rand(i);
        self.mc2_set_sprite(i, ((d & 3) + 324) as u16);
        Some(i)
    }

    /// `sub_4B150` (EF:33608) — the falling scenery (2,7)/(2,8):
    /// burnable physics props with gravity. THREE RNG draws (life
    /// 400..2447, x/y jitter). CAVE-EXCLUDED — both per-model ctors
    /// return 0 on caves (sub_4B0F0/sub_4B120, EF:33590/33601).
    pub(crate) fn mc2_spawn_falling(&mut self, model: u8, x: u16, y: u16, z: i16) -> Option<usize> {
        if self.is_cave() {
            return None;
        }
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = model;
            e.tick70 = if model == 7 { 20 } else { 21 };
            e.f56 = 1; // burnable
            e.f28 = 1; // cross-column damage contract (ch0)
            e.f71 = 0;
            e.f44 = (-128i16) as u16; // word_0x2C_44: initial fall velocity
            e.f126 = 0;
        }
        let d = self.mc2_rand(i);
        self.ent[i].max_life = d % 0x7D0 + 400;
        let jx = ((self.mc2_rand(i) & 0x3F) as i32 - 32) as i16;
        let jy = ((self.mc2_rand(i) & 0x3F) as i32 - 32) as i16;
        self.link(i, x.wrapping_add(jx as u16), y.wrapping_add(jy as u16), z);
        self.refill_life(i);
        self.mc2_set_sprite(i, if model == 7 { 322 } else { 323 });
        Some(i)
    }

    // ---- ticks ---------------------------------------------------------------

    /// The tree damage inbox: MC2's area writers hit burnable
    /// class-2 entities through the same channel-0 mailbox our
    /// combat column already writes (`sub_11400`'s
    /// `(1 << ch) & byte_0x38_56` gate ≡ ch0 vs our f56 bit 0).
    fn mc2_scenery_hit(&mut self, i: usize) -> Option<(u32, u16)> {
        if self.ent[i].mail[0].1 == 0 {
            return None;
        }
        let (amt, src) = self.ent[i].mail[0];
        self.ent[i].mail[0].1 = 0;
        Some((amt, src))
    }

    /// Water check + splash despawn shared by the tree states
    /// (EF:62450-56): the real (10,5) splash (id inherited) + the
    /// despawn.
    fn mc2_scenery_water(&mut self, i: usize) -> bool {
        let (x, y, z, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        if self.cap_bit(x, y) == 1 {
            if let Some(s) = self.mc2_spawn_splash(x, y, z) {
                self.ent[s].id24 = id;
            }
            self.ent[i].flags |= 0x400;
            return true;
        }
        false
    }

    /// `AddTree02_00_64E20` (EF:62399) — the healthy tree: burn-hit
    /// intake; a lethal hit spawns the flame, re-seeds 130..189 burn
    /// life and advances to the burning state.
    pub(crate) fn mc2_tree_tick(&mut self, i: usize) {
        // Unconditional byte[2] |= 2 at the top (EF:62415).
        self.ent[i].flags |= 0x2_0000;
        if let Some((amt, src)) = self.mc2_scenery_hit(i) {
            self.ent[i].act_life -= amt as i32;
            if self.ent[i].act_life < 0 {
                // The flame: the real (10,6) standing fire
                // (EF:62421-56 — id from the attacker, the
                // word_0x2C_44 = (3*fov)>>2 lift, re-seeded burn).
                // EVERYTHING is gated on the spawn succeeding
                // (EF:62424 `if (v3x)`): on pool failure retail
                // draws NO rand, re-seeds nothing and does NOT
                // advance — the tree just takes the damage.
                let (x, y, z, fov) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z, e.f84)
                };
                let fz = if z > 128 { z - 128 } else { 0 };
                if let Some(f) = self.mc2_spawn_fire6(x, y, fz) {
                    self.ent[f].id24 = if (src as usize) < self.ent.len() && src != 0 {
                        self.ent[src as usize].id24
                    } else {
                        src
                    };
                    self.ent[f].f44 = (3 * fov) >> 2;
                    let d = self.mc2_rand(i);
                    let burn = (d % 0x3C + 130) as i32;
                    self.ent[f].act_life = burn;
                    self.ent[i].act_life = burn;
                    // `dword &= 0xFFFDFFF7; byte[2] |= 2`
                    // (EF:62439-42): the burning tree stops being a
                    // TARGET (bit 8 clear — same op as
                    // mc2_spawn_fire).
                    let e = &mut self.ent[i];
                    e.flags = (e.flags & !0x2_0008) | 0x2_0000;
                    e.tick70 = 1;
                    // `sub_57D40(a1x, &a1x->position)` (EF:62443, the
                    // fn's SOLE call site) — re-head the TREE so the
                    // flame, head-linked an instruction ago, paints
                    // after it and therefore in FRONT of it. Inert to
                    // every scan: the line above cleared the burning
                    // tree's target bit.
                    self.relink_head(i);
                }
            }
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        self.mc2_scenery_water(i);
    }

    /// `sub_64F60` (EF:62462) — the burning tree: 1 life/tick; under
    /// 60 the charred sprite (83→226, 84→227) and the stump state.
    pub(crate) fn mc2_tree_burning_tick(&mut self, i: usize) {
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < 60 {
            self.ent[i].tick70 = 2;
            let charred = match self.ent[i].type86 {
                83 => 226,
                84 => 227,
                other => other,
            };
            self.mc2_set_sprite(i, charred);
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        self.mc2_scenery_water(i);
    }

    /// `sub_64FF0` (EF:62500) — the charred stump: terminal, snap-z.
    pub(crate) fn mc2_tree_stump_tick(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        self.mc2_scenery_water(i);
    }

    /// `AddStatue02_01_65040` / `sub_65110` (EF:62519/62536) — the
    /// model-1/3 statics: the byte[2] |= 2 static draw stamp, then the
    /// terrain snap. The model-2 dolmen is World-routed (its
    /// `AddDolmen02_02_65080` sweep needs the rival records) and
    /// stamps nothing; models 4/5 are the true no-ops.
    pub(crate) fn mc2_scenery_snap_tick(&mut self, i: usize) {
        self.ent[i].flags |= 0x2_0000;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
    }

    /// `sub_652C0` (EF:62606) — the falling-physics prop (2,7)/(2,8):
    /// gravity (−24/tick clamped ±192), damage bounces it with THREE
    /// RNG draws, death → (10,13) gib (pending, misfit) + despawn,
    /// water → splash + despawn.
    pub(crate) fn mc2_falling_tick(&mut self, i: usize) {
        if self.ent[i].flags & super::mobs::F_STOP != 0 {
            self.ent[i].flags &= !super::mobs::F_STOP;
            return;
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let mut pos = (x, y, self.ent[i].z);
        if pos.2 > self.ground_z(x, y) as i16 {
            if self.ent[i].f126 != 0 {
                let (yaw, spd) = (self.ent[i].f30, self.ent[i].f126);
                Self::polar_step(&mut pos, yaw, 0, spd);
            }
        } else {
            self.ent[i].f126 = 0;
            // sub_654B0 (EF:62705-45): a landed prop on rough ground
            // (roughness > 20) rolls off — scan 8 yaws from f30
            // (+256 & 0x7FF) at 64 units, take the LOWEST-alt
            // neighbor (sentinel 0x10000: some neighbor always
            // wins). No RNG.
            if self.roughness(pos.0, pos.1) > 20 {
                let mut best = pos;
                let mut best_alt = 0x10000u32;
                let mut yaw = self.ent[i].f30;
                for _ in 0..8 {
                    let mut cand = pos;
                    Self::polar_step(&mut cand, yaw, 0, 64);
                    let alt = self.ground_z(cand.0, cand.1) as u32;
                    if alt < best_alt {
                        best_alt = alt;
                        best = cand;
                    }
                    yaw = yaw.wrapping_add(256) & 0x7FF;
                }
                pos = best;
            }
        }
        if self.ent[i].f126 > 0 {
            self.ent[i].f126 -= 1;
        }
        // Gravity (EF:62650-60): position takes the OLD velocity,
        // THEN the velocity decrements (clamped ±192 after the
        // write); the ground clamp samples terrain at the MOVED x,y.
        let v_old = self.ent[i].f44 as i16;
        pos.2 = pos.2.wrapping_add(v_old);
        self.ent[i].f44 = (v_old - 24).clamp(-192, 192) as u16;
        let ground = self.ground_z(pos.0, pos.1) as i16;
        pos.2 = pos.2.max(ground);
        self.move_relink(i, pos.0, pos.1, pos.2);
        // Burn/impact intake (EF:62661-87).
        if let Some((amt, _src)) = self.mc2_scenery_hit(i) {
            if pos.2 <= ground {
                let kick = ((amt >> 2) as i16).clamp(2, 192);
                let d1 = self.mc2_rand(i);
                self.ent[i].f44 = ((d1 % kick as u32) as i16 + kick) as u16;
                // The 2nd draw's divisor is the JUST-WRITTEN f44
                // (`v8 >> 1`, EF:62675), not kick>>1 (f44 ≥ 2 so
                // the max(1) is only a div-by-zero guard).
                let half = ((self.ent[i].f44 as u32) >> 1).max(1);
                let d2 = self.mc2_rand(i);
                self.ent[i].f126 = ((d2 % half) + 1) as i16;
                let d3 = self.mc2_rand(i);
                self.ent[i].f30 = (d3 & 0x7FF) as u16;
                self.ent[i].z = self.ent[i].z.wrapping_add(self.ent[i].f44 as i16);
            }
            self.ent[i].act_life -= amt as i32;
        }
        if self.ent[i].act_life < 0 {
            // The settled-debris smoke poof (EF:62688-91) — (10,13)
            // is the smoke puff, not a gib.
            let (x, y, z) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            };
            self.mc2_spawn_smoke_particle_for(13, x, y, z);
            self.ent[i].flags |= 0x400;
            return;
        }
        if self.ent[i].z <= ground {
            self.mc2_scenery_water(i);
        }
    }

    /// The MC2 class-2 tick column: trees run the burn ladder, statics
    /// snap, falling props fall. Unknown states hold (authentic for the
    /// no-op slots).
    pub(crate) fn mc2_scenery_tick(&mut self, i: usize) {
        match (self.ent[i].model65, self.ent[i].tick70) {
            (0, 0) => self.mc2_tree_tick(i),
            (0, 1) => self.mc2_tree_burning_tick(i),
            (0, 2) => self.mc2_tree_stump_tick(i),
            // Model 2 (the dolmen) never reaches here — the World
            // dispatch intercepts state 6 for the shrine sweep.
            (1 | 3, _) => self.mc2_scenery_snap_tick(i),
            (6, 18) => self.mc2_cave_bee_tick(i),
            (6, 19) => self.mc2_bee_snap_water(i),
            (7 | 8, _) => self.mc2_falling_tick(i),
            _ => {} // models 4/5: the authentic no-op ticks
        }
    }

    /// `sub_651B0` (EF:62548) — the LIVE cave bee (action 18): the
    /// ground/static draw flag (byte[2] |= 2), the damage mailbox
    /// (death → corpse action 19, clear the targetable bit, death
    /// sprite row +4, ONE (10,13) puff — no sound anywhere in the
    /// family), then the floor snap + water despawn.
    fn mc2_cave_bee_tick(&mut self, i: usize) {
        self.ent[i].flags |= 0x2_0000; // byte[2] |= 2 (EF:62558)
        if self.ent[i].mail[0].1 != 0 {
            let (amt, _src) = self.ent[i].mail[0];
            self.ent[i].act_life -= amt as i32;
            if self.ent[i].act_life < 0 {
                self.ent[i].tick70 = 19;
                self.ent[i].flags &= !8;
                let row = self.ent[i].type86.wrapping_add(4);
                self.mc2_set_sprite(i, row); // death sprite 328..331
                let (x, y, z) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                self.mc2_spawn_smoke_particle_for(13, x, y, z);
            }
            self.ent[i].mail[0].1 = 0;
        }
        self.mc2_bee_snap_water(i);
    }

    /// `sub_65240` (EF:62582) — the bee corpse (action 19) AND the
    /// live tick's tail: z pinned to the floor every frame, despawn
    /// over water.
    fn mc2_bee_snap_water(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        if self.cap_bit(x, y) == 1 {
            self.ent[i].flags |= 0x400;
        }
    }
}
