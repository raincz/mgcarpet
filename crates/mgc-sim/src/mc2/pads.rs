//! CONFORMANCE IMPORT ONLY — the MC2 BUILD00 **pad replays**.
//!
//! `.mgcr` carries no terrain channel (docs/RECORDING.md), so a
//! mid-take import lands the pool onto the level's PRISTINE heightfield
//! while retail's map still carries every terraform the run performed.
//! [`crate::mc2::riser`]'s `mc2_riser_reconstruct` opened this seam for
//! the (14,1) riser; this module extends it to the two BUILD00 pad
//! stampers, which are the dominant MC2 terraform families:
//!
//! - the (3,2) CASTLE's (10,42) painter (`AddTerrainMod0A_2A_37BC0`,
//!   EF:27648 — [`Gen::mc2_castle_painter_tick`]), and
//! - the (10,45)-family VILLAGE BUILDING's own construction action 51
//!   (`ApplyTerrainModification_37240`, EF:27181 —
//!   [`Gen::mc2_building_tick`]).
//!
//! Both are 19/30-tick progressive lerps toward an ABSOLUTE target
//! (`pad_height + datum`) whose final tick divides by 1 — so the
//! terminal map is a pure function of state the recording already
//! carries: the stamper's cell, its BUILD00 row, and its build datum.
//! Neither replay is reachable from native gameplay.
//!
//! LOST-BY-CONSTRUCTION (documented, not attempted): a terraform whose
//! source entity is gone leaves no evidence in the pool — a demolished
//! castle's un-stamp jitter (`RemoveCastleStage_385C0`, EF:28071, rolls
//! the entity LCG per cell), a finalized (10,18) volcano dome
//! (`mc2_dome_tick`/`sub_31940` EF:23193 — the l30 summit plateau, dig
//! C 2026-08-03), and any crater whose caster despawned.

use crate::engine::features::{Gen, tile};

impl Gen {
    /// CONFORMANCE IMPORT ONLY — re-stamp the cumulative BUILD00 pad an
    /// already-standing (3,2) castle wrote through its (10,42) painters.
    ///
    /// RETAIL LAW. The painter is spawned at the castle's
    /// `axis_0x9A_154` (`sub_5FBD0` EF:61188 —
    /// `IfSubtypeCallCreatingManaSphere_4A190(&a1x->axis_0x9A_154x, 10,
    /// 42)`), which the castle ctor `sub_4AA40` fills with the build
    /// site and `.z = 32 * sub_48E60(...)`, the PERIMETER MINIMUM ground
    /// over the row-1 footprint (EF:33399, [`Gen::mc2_castle_site_z`]).
    /// The painter's row is the castle's level verbatim
    /// (`indexx->byte_0x46_70 = a1x->dword_0x10_16`, EF:61189). Its
    /// tick reads the datum back off its OWN position
    /// (`v40 = a1x->position_0x4C_76.z >> 5`, EF:27775) and drives 18
    /// rise ticks whose per-cell write is
    /// `height += (pad + datum − height) / countdown` (EF:27846-56):
    /// the last tick divides by 1, so the terminal height is the
    /// ABSOLUTE `pad + datum` for every cell of BUILD00 rows `1..=level`
    /// that carries a pad byte. Every level-up spawns a fresh painter
    /// over the same cumulative footprint, so the LAST painter alone
    /// reproduces the whole history.
    ///
    /// The port imports all three inputs: the anchor from `x`/`y`, the
    /// datum from `site_z` (@0x9A.z → `dest_z`) and the level from
    /// `f26` (@0x10 = `dword_0x10_16`) — so the pad is replayable.
    /// Level 0 selects the EMPTY BUILD00 row 0 (a bare flag, no
    /// structure, nothing stamped).
    ///
    /// IDEMPOTENT: the world build already stamps every AUTHORED
    /// castle's pad at its authored level (`mc2_spawn_rival_castle`
    /// settles a painter synchronously), and the write is absolute, so
    /// a castle still standing at its authored level replays to a
    /// no-op — the [`Self::mc2_castle_pad_reconstruct`] early-out below
    /// makes that literal. What the replay recovers is every castle
    /// BUILT or LEVELLED UP during the take (mc2l4: the human's
    /// water-sited (154,34) castle reaches level 5 with datum 0 — the
    /// pristine plane reads height 0 there and the port's castle sank
    /// to z 0 against retail's 4160).
    ///
    /// The rise's RNG-free helpers (`mc2_paint_cell`,
    /// `mc2_add_building_region`) draw only the `pseudoRand` retile
    /// stream, never the entity/global LCG the conformance runner
    /// compares — no seed is disturbed.
    pub(crate) fn mc2_castle_pad_reconstruct(&mut self, i: usize) {
        let row = self.ent[i].f26.clamp(0, 7) as usize;
        if row == 0 || self.assets.build_tab.get(row).is_none() {
            return;
        }
        let datum = (self.ent[i].site_z >> 5) as i32;
        let cx = (self.ent[i].x.wrapping_add(128) >> 8) as u8;
        let cy = (self.ent[i].y.wrapping_add(128) >> 8) as u8;
        self.mc2_build_pad_stamp(cx, cy, row, datum, true);
    }

    /// CONFORMANCE IMPORT ONLY — re-stamp the BUILD00 pad a village
    /// building raised through its own action-51 construction.
    ///
    /// RETAIL LAW. `ApplyTerrainModification_37240` (EF:27181,
    /// [`Gen::mc2_building_tick`]) lerps every footprint cell toward
    /// `pad + (z >> 5)` with the countdown `life` as divisor
    /// (EF:27341-44), paints the walkable village tiles every 5th tick
    /// and on the last, then on the final frame parks the entity as the
    /// static building: action 51 → 52, `axis_0x9A_154 = position`
    /// (our `site_z`), `position.z = ground` and the two pad-edge rings
    /// (EF:27289-27304). So a FINISHED building's build datum survives
    /// in `site_z` — its `z` has already been overwritten with the
    /// post-stamp ground — and its BUILD00 row survives in
    /// `byte_0x46_70` (`f71`). Both are imported.
    ///
    /// This is the ledger's §terraform family: village GROWTH raises
    /// new huts at runtime (mc2l0 main wave ~t=751), and the pristine
    /// replay reads the pre-growth hill — which is also what drowns the
    /// (5,13) villagers whose all-four-blocked die law reads deep water
    /// on the eastern approach.
    ///
    /// A still-CONSTRUCTING building (action 51) is replayed for
    /// exactly the `max_life − act_life` ticks already run: the lerp is
    /// a pure function of the plane, so re-running the prefix from the
    /// pristine plane lands the same partial pad. The footprint kill
    /// (EF:27310-28) is suppressed — retail's victims are already
    /// absent from the import, and re-running it would kill entities
    /// retail kept.
    ///
    /// IDEMPOTENT over the AUTHORED village the conformance baseline
    /// plane already carries: the last lerp frame writes the absolute
    /// target BEFORE the final frame's rings re-smooth, so a second
    /// pass reproduces the same terrace — provided the rings are not
    /// let out past the footprint (the fence below, which is the whole
    /// difference between this arm helping and hurting: with it,
    /// mc2l0 t=700+400 goes 291 → 377 conforming and `terraform-houses`
    /// 1024 → 0; without it, 291 → 0).
    pub(crate) fn mc2_building_pad_reconstruct(&mut self, i: usize) {
        let (act, tick) = (self.ent[i].act_life, self.ent[i].tick70);
        let (z, site_z, max_life, flags, chain) = {
            let e = &self.ent[i];
            (e.z, e.site_z, e.max_life, e.flags, e.f46)
        };
        // Ticks of the 30-frame construction already run, and the z the
        // lerp read while they ran.
        let (ran, base_z) = match tick {
            52 => (max_life as i32, site_z),
            51 => (max_life as i32 - act, z),
            _ => return,
        };
        let Some(def) = self.assets.build_tab.get(self.ent[i].f71 as usize).copied() else {
            return;
        };
        if ran <= 0 {
            return;
        }
        // OFF-FOOTPRINT HEIGHT FENCE. The construction's height lerp
        // only ever writes footprint cells, but its FINAL frame runs
        // two pad-edge smoothing rings (`sub_48A20` EF:32348) that
        // reach a full footprint-width PAST the pad — they are
        // anchored on the top-left corner MINUS the half extents. Over
        // ground the baseline plane already settled, that second
        // 3x3 average is pure damage (mc2l0: one 1-unit re-smooth at
        // (71,166), the top band of the 23x11 building at (82,180),
        // cost all 291 conforming pairs of t=700+400). The replay is
        // for the pad the recording lost, so the outside is snapshotted
        // and put back.
        let (w, h) = (def.w as usize, def.h as usize);
        let cx = (self.ent[i].x.wrapping_add(128) >> 8) as u8;
        let cy = (self.ent[i].y.wrapping_add(128) >> 8) as u8;
        let (tlx, tly) = (
            cx.wrapping_sub((w / 2) as u8),
            cy.wrapping_sub((h / 2) as u8),
        );
        // The rings' reach: `thick` (max 5) beyond a box one footprint
        // out from the top-left corner — +2 of slack for the 3x3 read.
        let (rw, rh) = (2 * w + 16, 2 * h + 16);
        let (rx, ry) = (
            tlx.wrapping_sub((w / 2 + 8) as u8),
            tly.wrapping_sub((h / 2 + 8) as u8),
        );
        let mut fence: Vec<(usize, u8)> = Vec::with_capacity(rw * rh);
        for dy in 0..rh {
            for dx in 0..rw {
                let (gx, gy) = (rx.wrapping_add(dx as u8), ry.wrapping_add(dy as u8));
                if (gx.wrapping_sub(tlx) as usize) < w && (gy.wrapping_sub(tly) as usize) < h {
                    continue;
                }
                let t = tile(gx, gy);
                fence.push((t, self.t.height[t]));
            }
        }
        let sounds = self.sounds.len();
        {
            let e = &mut self.ent[i];
            e.z = base_z;
            e.act_life = max_life as i32;
            // `max_life − 1 == life` is the footprint-kill gate; a zero
            // max_life can never match a positive countdown.
            e.max_life = 0;
            e.tick70 = 51;
        }
        for _ in 0..ran {
            self.mc2_building_tick(i, None);
        }
        for (t, height) in fence {
            self.t.height[t] = height;
        }
        // The import is the truth for every entity field — the replay
        // touched terrain only. `f46` is the degradation link
        // ([`Gen::mc2_spawn_building`]): it must survive the replayed
        // construction pass or the building demolishes at the end of
        // its life where retail rebuilds its successor.
        {
            let e = &mut self.ent[i];
            e.act_life = act;
            e.tick70 = tick;
            e.z = z;
            e.site_z = site_z;
            e.max_life = max_life;
            e.flags = flags;
            e.f46 = chain;
        }
        self.sounds.truncate(sounds);
    }

    /// The TERMINAL form of the (10,42) painter's rise + settle
    /// (EF:27648-27860, ported live as [`Gen::mc2_castle_painter_tick`])
    /// — the 18-tick lerp collapsed to its `countdown == 1` limit.
    ///
    /// Per-tick residue that survives to the terminal map, in the
    /// painter's own order:
    /// - the FIRST rise tick evaluates `!height || sub_57450(type)`
    ///   against the PRISTINE cell (EF:27850) — flat angle nibble 1 +
    ///   `AddBuildingToTerrain_46570`;
    /// - the height write is absolute at `countdown == 1`;
    /// - `countdown == 2` clears bit3 over the WHOLE frame (EF:27895),
    ///   `countdown == 1` then flips bit7 → bit3 on the active cells
    ///   (EF:27859-69) and the settle pass flips it back, bit3 → bit7,
    ///   over the frame (EF:27737-45) — both NON-CAVE only; on caves
    ///   bit3 is the floor↔ceiling seal, owned by the ceiling-rise arm;
    /// - the paint codes are re-interpreted on the last rise tick, so
    ///   the terminal texture is the one `sub_45DC0` derives from the
    ///   FINAL heights.
    ///
    /// `cumulative` selects the castle's rows-`1..=row` overlay (later
    /// rows overwrite earlier ones per cell, matching retail's shared
    /// scratch keyed by frame index); a single-row stamper passes false.
    fn mc2_build_pad_stamp(
        &mut self,
        cx: u8,
        cy: u8,
        row: usize,
        datum: i32,
        cumulative: bool,
    ) -> bool {
        let Some(def) = self.assets.build_tab.get(row).copied() else {
            return false;
        };
        let (mut w, mut h) = (def.w as usize, def.h as usize);
        if cumulative {
            for r in 1..=row {
                if let Some(rd) = self.assets.build_tab.get(r) {
                    w = w.max(rd.w as usize);
                    h = h.max(rd.h as usize);
                }
            }
        }
        if w == 0 || h == 0 {
            return false;
        }
        let tlx = cx.wrapping_sub((w / 2) as u8);
        let tly = cy.wrapping_sub((h / 2) as u8);
        // Frame-indexed target heights + paint codes, rows 1..=row.
        let mut target = vec![None::<i32>; w * h];
        let mut paint = vec![None::<u8>; w * h];
        let first = if cumulative { 1 } else { row };
        for r in first..=row {
            let Some(rd) = self.assets.build_tab.get(r).copied() else {
                continue;
            };
            let (rw, rh) = (rd.w as usize, rd.h as usize);
            let start = rd.offset as usize;
            let Some(cells) = self.assets.build_dat.get(start..start + 2 * rw * rh) else {
                continue;
            };
            let cells = cells.to_vec();
            let offx = w / 2 - rw / 2;
            let offy = h / 2 - rh / 2;
            for dy in 0..rh {
                for dx in 0..rw {
                    let c = &cells[2 * (dy * rw + dx)..2 * (dy * rw + dx) + 2];
                    let f = (offy + dy) * w + offx + dx;
                    if c[1] != 0xff {
                        target[f] = Some(c[1] as i32 + datum);
                    }
                    if c[0] != 0xff {
                        paint[f] = Some(c[0]);
                    }
                }
            }
        }
        // ACTIVE = the first rise tick saw a nonzero delta. An
        // already-stamped pad (the authored castle at its authored
        // level) has none, and the whole replay is a no-op.
        let mut active = vec![false; w * h];
        let mut any = false;
        for dy in 0..h {
            for dx in 0..w {
                let f = dy * w + dx;
                if let Some(tg) = target[f] {
                    let t = tile(tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                    active[f] = tg != self.t.height[t] as i32;
                    any |= active[f];
                }
            }
        }
        if !any {
            return false;
        }
        self.terrain_dirty = true;
        let cave = self.is_cave();
        // (a) the first tick's pristine flat-nibble/region promotion.
        for dy in 0..h {
            for dx in 0..w {
                if !active[dy * w + dx] {
                    continue;
                }
                let (gx, gy) = (tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                let t = tile(gx, gy);
                // EF:27852's auto-flat predicate is sub_57450
                // (morph::auto_flat), NOT the damage pass's burnable
                // set (flood::burn_flags).
                if self.t.height[t] == 0 || super::morph::auto_flat(self.t.tile_type[t]) {
                    self.t.angle[t] = (self.t.angle[t] & 0xF8) | 1;
                    self.mc2_add_building_region(gx, gy, gx, gy);
                }
            }
        }
        // (b) the absolute height limit + the cave headroom bubble.
        for dy in 0..h {
            for dx in 0..w {
                let f = dy * w + dx;
                let t = tile(tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                if active[f]
                    && let Some(tg) = target[f]
                {
                    self.t.height[t] = tg as u8;
                }
                if cave {
                    let floor = self.t.height[t] as i32;
                    let tgt = (floor.max(datum) + 100).min(255);
                    if tgt > self.t.ceiling[t] as i32 {
                        self.t.ceiling[t] = tgt as u8;
                    }
                    self.cave_seal_fixup(t);
                }
            }
        }
        // (c) countdown == 2: bit3 cleared over the whole frame.
        if !cave {
            for dy in 0..h {
                for dx in 0..w {
                    let t = tile(tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                    self.t.angle[t] &= !8;
                }
            }
            // (d) countdown == 1: bit7 → bit3 on the active cells.
            for dy in 0..h {
                for dx in 0..w {
                    if !active[dy * w + dx] {
                        continue;
                    }
                    let t = tile(tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                    if self.t.angle[t] & 0x80 != 0 {
                        self.t.angle[t] = (self.t.angle[t] & 0x7F) | 8;
                    }
                }
            }
        }
        // (e) the last rise tick's texture pass, over the FINAL heights.
        for dy in 0..h {
            for dx in 0..w {
                if let Some(code) = paint[dy * w + dx] {
                    self.mc2_paint_cell(
                        7,
                        tlx.wrapping_add(dx as u8),
                        tly.wrapping_add(dy as u8),
                        code,
                    );
                }
            }
        }
        // (f) the settle pass: bit3 → bit7 over the frame (NON-CAVE).
        if !cave {
            for dy in 0..h {
                for dx in 0..w {
                    let t = tile(tlx.wrapping_add(dx as u8), tly.wrapping_add(dy as u8));
                    if self.t.angle[t] & 8 != 0 {
                        self.t.angle[t] = (self.t.angle[t] & 0xF7) | 0x80;
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::chassis::ChassisParams;
    use crate::engine::features::{BuildDef, FeatureAssets, Gen, Planes, tile};
    use crate::verbs::VerbSet;

    /// BUILD00 stand-in. Row 1 = an 8x8 pad 40 high (castle level 1);
    /// row 2 = a 12x12 pad 24 high with a 4x4 0xff HOLE at its centre
    /// (castle level 2 — the hole is what makes the rows-`1..=level`
    /// overlay observable: retail's later row wins per cell, so only
    /// the hole may still read row 1); row 3 = a 12x12 pad 40 high
    /// (the village building, sized past the pad-edge smoothing bands
    /// so the assertions can name an untouched interior cell). Paint
    /// codes are all 0xff, keeping the assertions on the height plane.
    fn build_assets() -> FeatureAssets {
        let mut dat = Vec::new();
        let r1 = dat.len() as u32;
        for _ in 0..64 {
            dat.extend_from_slice(&[0xff, 40]);
        }
        let r2 = dat.len() as u32;
        for y in 0..12 {
            for x in 0..12 {
                let hole = (4..8).contains(&x) && (4..8).contains(&y);
                dat.extend_from_slice(&[0xff, if hole { 0xff } else { 24 }]);
            }
        }
        let r3 = dat.len() as u32;
        for _ in 0..144 {
            dat.extend_from_slice(&[0xff, 40]);
        }
        FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: vec![
                BuildDef {
                    offset: 0,
                    w: 0,
                    h: 0,
                },
                BuildDef {
                    offset: r1,
                    w: 8,
                    h: 8,
                },
                BuildDef {
                    offset: r2,
                    w: 12,
                    h: 12,
                },
                BuildDef {
                    offset: r3,
                    w: 12,
                    h: 12,
                },
            ],
            build_dat: dat,
            bldgprm: Vec::new(),
            spells: Vec::new(),
            mc2_sprite_ext: Vec::new(),
        }
    }

    fn flat_gen() -> Gen {
        let planes = Planes {
            height: vec![10; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        Gen::new(planes, build_assets(), 1, ChassisParams::MC2, VerbSet::MC2)
    }

    fn place_castle(g: &mut Gen, tx: u16, ty: u16, level: i16, site_z: i16) -> usize {
        let i = g.new_event().expect("castle slot");
        let e = &mut g.ent[i];
        e.class64 = 3;
        e.model65 = 2;
        e.tick70 = 4;
        e.x = tx * 256;
        e.y = ty * 256;
        e.f26 = level;
        e.site_z = site_z;
        i
    }

    /// THE CASTLE PAD LAW + its conformance-import replay. A castle
    /// that levels up during a take leaves a cumulative BUILD00 pad on
    /// the map; a `.mgcr` import lands on the PRISTINE plane, so the
    /// port's castle (and everything that ground-snaps on its mound —
    /// guards, defender pieces, docked balloons) reads the un-raised
    /// height. `mc2_castle_pad_reconstruct` must rebuild exactly the
    /// map the live painters left, from the castle's terminal state
    /// alone.
    #[test]
    fn castle_pad_reconstruct_rebuilds_the_mound_two_painters_left() {
        // (a) the LIVE history: level 1 painter, then the level-2
        // painter (each settles fully, like a real level-up).
        let mut live = flat_gen();
        let c = place_castle(&mut live, 100, 100, 1, 64);
        for lvl in 1..=2 {
            live.ent[c].f26 = lvl;
            live.mc2_spawn_castle_painter(c, true);
            for _ in 0..4096 {
                let mut running = false;
                for j in 1..live.ent.len() {
                    if live.ent[j].class64 == 10
                        && live.ent[j].model65 == 42
                        && live.ent[j].flags & 0x400 == 0
                    {
                        live.mc2_castle_painter_tick(j);
                        running = true;
                    }
                }
                if !running {
                    break;
                }
            }
        }

        // (b) the IMPORT: pristine heights + the terminal castle row.
        let mut imported = flat_gen();
        let j = place_castle(&mut imported, 100, 100, 2, 64);
        let pristine = imported.t.height.clone();
        imported.mc2_castle_pad_reconstruct(j);

        assert_eq!(
            imported.t.height, live.t.height,
            "the replayed pad must equal the lived-through map"
        );
        assert_eq!(imported.t.angle, live.t.angle, "angle bookkeeping");
        assert_eq!(imported.ent[j].f26, 2, "castle state left untouched");

        // (c) NON-VACUITY: the map is NOT pristine; the row-2 apron
        // stands at 24 + (64 >> 5) = 26 and the level-2 hole still
        // reads the level-1 tower at 40 + 2 = 42 (frame origin
        // 100 − 12/2 = 94, so the hole is cells 98..101).
        assert_ne!(
            imported.t.height, pristine,
            "a no-op replay would leave the map pristine"
        );
        assert_eq!(
            imported.t.height[tile(99, 99)],
            42,
            "row-1 core in the hole"
        );
        assert_eq!(imported.t.height[tile(95, 95)], 26, "row-2 apron");
        assert_eq!(imported.t.height[tile(90, 90)], 10, "off-pad untouched");

        // (d) IDEMPOTENCE: replaying an already-stamped pad is a no-op
        // (this is what keeps the authored castles the world build
        // already settled from being re-stamped).
        let once = imported.t.height.clone();
        imported.mc2_castle_pad_reconstruct(j);
        assert_eq!(imported.t.height, once, "second replay must not move");
    }

    fn place_building(g: &mut Gen, tx: u16, ty: u16, row: u8, z: i16) -> usize {
        let i = g.new_event().expect("building slot");
        {
            let e = &mut g.ent[i];
            e.class64 = 10;
            e.model65 = 45;
            e.tick70 = 51;
            e.f71 = row;
            e.max_life = 30;
            e.act_life = 30;
            e.f140 = 1;
        }
        g.link(i, tx * 256, ty * 256, z);
        i
    }

    /// THE VILLAGE-BUILDING PAD LAW + its replay. Village GROWTH raises
    /// huts at runtime, so the pad under a hut that finished mid-take
    /// exists only in retail's map. The replay must rebuild it from the
    /// parked building's own state — its BUILD00 row and the build
    /// datum retail parked in `axis_0x9A_154` (`site_z`) — over the
    /// FOOTPRINT, and must leave the ground outside it exactly as the
    /// baseline plane had it (the off-footprint fence).
    #[test]
    fn building_pad_reconstruct_rebuilds_the_hut_terrace() {
        // (a) the LIVE history: 30 construction ticks to the park.
        let mut live = flat_gen();
        let b = place_building(&mut live, 60, 60, 3, 64);
        for _ in 0..31 {
            live.mc2_building_tick(b, None);
        }
        assert_eq!(live.ent[b].tick70, 52, "parked as the static building");
        assert_eq!(live.ent[b].site_z, 64, "build datum survives in site_z");

        // (b) the IMPORT: pristine heights + the parked building row.
        let mut imported = flat_gen();
        let j = place_building(&mut imported, 60, 60, 3, 64);
        {
            let e = &mut imported.ent[j];
            e.tick70 = 52;
            e.site_z = live.ent[b].site_z;
            e.z = live.ent[b].z;
            e.act_life = live.ent[b].act_life;
            e.max_life = live.ent[b].max_life;
        }
        let pristine = imported.t.height.clone();
        imported.mc2_building_pad_reconstruct(j);

        // The FOOTPRINT (frame origin 60 − 12/2 = 54) must match the
        // lived-through map cell for cell.
        for ty in 54u8..66 {
            for tx in 54u8..66 {
                assert_eq!(
                    imported.t.height[tile(tx, ty)],
                    live.t.height[tile(tx, ty)],
                    "footprint cell ({tx},{ty})"
                );
            }
        }
        assert_eq!(imported.t.tile_type, live.t.tile_type, "village paint");
        assert_eq!(imported.ent[j].tick70, 52, "building state untouched");
        assert_eq!(imported.ent[j].act_life, live.ent[b].act_life);
        assert_eq!(imported.ent[j].max_life, live.ent[b].max_life);

        // (c) NON-VACUITY: pad 40 + datum (64 >> 5) = 42 in the
        // footprint interior — (55,55) is clear of the final frame's
        // two pad-edge smoothing bands — and pristine 10 well outside.
        assert_ne!(imported.t.height, pristine, "a no-op replay stays flat");
        assert_eq!(imported.t.height[tile(55, 55)], 42, "hut terrace");
        assert_eq!(imported.t.height[tile(40, 40)], 10, "off-pad untouched");

        // (d) THE FENCE: the live final frame's pad-edge rings smooth
        // ground OUTSIDE the footprint; the replay must put every one
        // of those cells back the way the baseline plane had it, since
        // the baseline already carries the first pass's version.
        let mut fenced = 0;
        for ty in 40u8..80 {
            for tx in 40u8..80 {
                if (54..66).contains(&tx) && (54..66).contains(&ty) {
                    continue;
                }
                let t = tile(tx, ty);
                assert_eq!(
                    imported.t.height[t], pristine[t],
                    "off-footprint cell ({tx},{ty}) must be fenced"
                );
                fenced += usize::from(live.t.height[t] != pristine[t]);
            }
        }
        assert!(
            fenced > 0,
            "the live rings must have written off-footprint ground for the \
             fence assertion above to mean anything"
        );

        // (e) IDEMPOTENCE: the lerp lands the ABSOLUTE target before
        // the rings re-smooth, so a second replay is a no-op — this is
        // what lets the arm run over the authored village the baseline
        // plane already carries.
        let once = imported.t.height.clone();
        imported.mc2_building_pad_reconstruct(j);
        assert_eq!(imported.t.height, once, "second replay must not move");
    }
}
