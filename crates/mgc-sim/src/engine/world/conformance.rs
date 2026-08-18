//! Retail-conformance seams (docs/RECORDING.md "fixture runner"):
//! import a decoded retail closure onto a built world, and project the
//! world into the recorder's obs schema for tick-by-tick comparison.
//!
//! The importer is the port-side analog of retail's own in-level LOAD
//! (docs/traces/mc1-campaign-save-menu.md): the raw image lands over
//! the live state, the mode/settings words are discarded, the free
//! stack and the per-tile lists are REBUILT, and owner links are
//! re-derived. Retail's pointer fixups become index arithmetic here —
//! guest addresses are stable, so the behavior-row pointer converts to
//! a row index anchored on the human carpet's canonical row 7.
//!
//! The human player lives OUTSIDE the pool in this port, so the
//! recorded carpet slot stays a reserved hole: its state routes to
//! [`Player`]/`human_pose`, every pool field that references the
//! carpet slot translates to [`PLAYER_TARGET`], and the projection
//! synthesizes the carpet entity back at the recorded slot. The
//! conformance runner drives the pose per tick (pin-the-human), so
//! world fidelity verifies with zero dependence on input
//! reconstruction.
//!
//! Known non-closure state (retail keeps these OUTSIDE the saved
//! struct; import resets them and the runner buckets any fallout):
//! the terrain planes (craters/retile — restore via
//! [`World::restore_planes`]), the retile LCG `pseudoRand`, and the
//! volcano registers (`gamedata+36/38`).

use super::{LifeState, PLAYER_LIFE_MAX, Player, PlayerPose, World};
use crate::engine::features::{Ent, Planes};
use crate::flight::{Mc1State, Mc2Ext, Mc2Row};
use crate::mc1::mobs::PLAYER_TARGET;
use crate::mc1::spells::{SPELL_COUNT, SpellId};
use mgc_formats::mgcr::{
    ControlMc1, ControlMc2, EntObsMc1, EntObsMc2, FlightMc1, FlightMc2, ObsMc1, ObsMc2,
    PlayerJoinMc1, PlayerJoinMc2, PlayerMc2, RetailEntMc1, RetailEntMc2, RetailMc1, RetailMc2,
    RetailPlayerMc2, RetailWizardMc1, WizardMc1,
};

/// What the importer did — counts for the runner's coverage report.
#[derive(Debug, Clone)]
pub struct ImportReport {
    /// Active pool entities imported (human carpet excluded).
    pub active: usize,
    /// The recorded human carpet slot (the reserved hole).
    pub human_slot: u16,
    /// Derived `unk_98F38` guest base (carpet row-7 anchor).
    pub behavior_base: u32,
    /// Entities whose behavior-row pointer did not convert (row 0
    /// fallback).
    pub bad_rows: usize,
    /// The recorded free/recycle stacks failed the census check and
    /// the free list fell back to the descending slot scan (spawn
    /// slot ORDER diverges from retail on such pairs): the observed
    /// live/expected counts, None when the recorded stack was used.
    pub stack_fallback: Option<(usize, usize)>,
}

/// One entity's ungraded raw lanes — see [`World::raw_shadow_mc1`].
///
/// EVERY per-entity field the port models and `EntObsMc1` does not
/// carry. The four the lane started with (`+70`/`+71`/`+58`/`+44`)
/// each paid for themselves, and the rest are the same blind spot: the
/// recording holds them, [`World::retail_import_mc1`] restores them
/// every pair, and the graded diff can never see a WRITE go wrong. The
/// cost of widening is a longer report, and the report is per-(class,
/// model, field) — noise stays legible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawShadowMc1 {
    pub slot: u16,
    pub class: u8,
    pub model: u8,
    /// `+70` — the handler state byte.
    pub f70: u8,
    /// `+71` — the burst / charge register (the kraken's bolt count).
    pub f71: u8,
    /// `+58`.
    pub f58: i16,
    /// `+44` — damage/potency. Ungraded like the rest, and the lane a
    /// spawn ctor writes ONCE: an effect born with the wrong potency
    /// reads clean in pair mode forever (the importer restores it
    /// every pair) and only bites a free run, as the victims' life
    /// diverging one tick after the broadcast.
    pub f44: u16,
    /// `+20`/`+22` — the tile-list links. Rebuilt every tick from the
    /// walk order, so a mismatch is a CHAIN-ORDER divergence: the
    /// membership laws this campaign keeps finding (tick-top chain,
    /// ball list, bucket[0]) all land here first.
    pub next20: u16,
    pub prev22: u16,
    /// `+26` — the generic counter (crater ring, wall run length,
    /// trigger rearm, acquire latch).
    pub f26: i16,
    pub f28: u16,
    pub f36: u16,
    /// `+38`/`+40` — killer-id and attacker latches.
    pub f38: u16,
    pub f40: u16,
    /// `+46` — vertical velocity.
    pub f46: i16,
    /// `+50` — the damage-response countdown.
    pub f50: i16,
    /// `+52`/`+54` — the multipart chain links (head / tail), and
    /// `+56` the follow distance. Named in the ledger as an obs blind
    /// spot since mc1l2.
    pub f52: u16,
    pub f54: u16,
    pub f56: u16,
    /// `+59` — the awake re-probe delay.
    pub f59: u8,
    /// `+68`/`+69` — the class/model this record detonates into.
    pub f68: u8,
    pub f69: u8,
    /// `+78`..`+84` — the sprite half-height and the collision extents.
    pub f78: u16,
    pub f80: u16,
    pub f82: u16,
    pub f84: u16,
    /// `+86`/`+88`/`+89` — sprite stats row, animation frame, frame count.
    pub type86: u16,
    pub frame88: u8,
    pub frames89: u8,
    /// `+90..+126` — the six damage mailboxes {amount, source}.
    pub mail: [(u32, u16); 6],
    /// `+128`/`+130` — target speed and acceleration.
    pub f128: i16,
    pub f130: i16,
    /// `+144` — the mana-ball owner.
    pub f144: u16,
    /// `+150`/`+152`/`+154` — teleport destination and build-site z.
    pub dest_x: u16,
    pub dest_y: u16,
    pub site_z: i16,
}

/// The pinned human context the projection needs: where the carpet
/// sits in the recording and the pose the runner is driving.
#[derive(Debug, Clone, Copy)]
pub struct PinnedMc1 {
    pub slot: u16,
    pub local: u16,
    pub player_count: u16,
    pub pose: PlayerPose,
}

/// The MC2 twin of [`PinnedMc1`].
#[derive(Debug, Clone, Copy)]
pub struct PinnedMc2 {
    pub slot: u16,
    pub local: u16,
    pub player_count: u16,
    pub pose: PlayerPose,
    /// The recorded per-player `CastleEntityIndex` words (+1080),
    /// echoed through the projection: the lane holds the AUTHORED
    /// castle binding — a runtime-BUILT castle never fills it (mc2l0:
    /// 0 across the whole take with the human's castle live), so it
    /// cannot be derived from the pool.
    pub castles: [i16; 8],
}

/// An opaque snapshot of the level's authored THING record table
/// ([`World::thing_table_clone`]).
pub struct ThingTable(Vec<crate::engine::features::Rec>);

impl World {
    /// The live terrain planes, cloned — the runner captures them
    /// right after the level build (POST feature pass: the load-time
    /// crater/flatten/wall edits are part of level init, not runtime
    /// state) and re-imprints per pair via [`World::restore_planes`].
    pub fn planes_clone(&self) -> Planes {
        Planes {
            height: self.g.t.height.clone(),
            tile_type: self.g.t.tile_type.clone(),
            shading: self.g.t.shading.clone(),
            angle: self.g.t.angle.clone(),
            ceiling: self.g.t.ceiling.clone(),
        }
    }

    /// The post-build THING record table, cloned — the twin of
    /// [`World::planes_clone`] for the level's authored records. A
    /// one-shot disposition ZEROES the records it releases
    /// (`sub_4A1E0(id, 1)`), and that consumption is NOT part of the
    /// `D41A0_0` closure the recording captures, so it cannot be
    /// re-imported per pair: a single mis-timed trip anywhere in a run
    /// silently disarms the disposition for every later pair. The
    /// runner captures the table right after the level build and
    /// re-imprints it via [`World::restore_thing_table`].
    pub fn thing_table_clone(&self) -> ThingTable {
        ThingTable(self.table.clone())
    }

    /// Re-imprint the post-build THING record table
    /// ([`World::thing_table_clone`]).
    pub fn restore_thing_table(&mut self, table: &ThingTable) {
        self.table.clear();
        self.table.extend_from_slice(&table.0);
    }

    /// Re-imprint the pristine terrain planes (the map file's blocks
    /// are not part of the master-struct closure; craters and retile
    /// do not survive a retail import).
    pub fn restore_planes(&mut self, planes: &Planes) {
        self.g.t = Planes {
            height: planes.height.clone(),
            tile_type: planes.tile_type.clone(),
            shading: planes.shading.clone(),
            angle: planes.angle.clone(),
            ceiling: planes.ceiling.clone(),
        };
        self.terrain_dirty = true;
    }

    /// Overwrite the height and tile-type planes — plus, on MC2 cave
    /// takes, the CEILING (cave carves edit it mid-level and the cave
    /// clamp laws read it), and the ANGLE plane when the take
    /// measures it (the sub_11760 water probe, the scorch gates and
    /// the castle protection bits all read it live; the mid-paint
    /// walkability/protection dance is not reconstructible from the
    /// pool closure — mc1l0 pair 565) — with MEASURED images (the
    /// recording's format-2 terrain channel, accumulated to the
    /// pair's start tick). The primary terrain source when present,
    /// layered over [`World::restore_planes`]'s pristine base so
    /// shading (and unmeasured planes) keep their level values.
    /// Wrong-size slices are an error, never a partial write.
    pub fn install_measured_terrain(
        &mut self,
        height: &[u8],
        tile_type: &[u8],
        ceiling: Option<&[u8]>,
        angle: Option<&[u8]>,
    ) -> Result<(), String> {
        if height.len() != self.g.t.height.len() || tile_type.len() != self.g.t.tile_type.len() {
            return Err(format!(
                "measured terrain {}+{} cells, want {}+{}",
                height.len(),
                tile_type.len(),
                self.g.t.height.len(),
                self.g.t.tile_type.len()
            ));
        }
        if let Some(c) = ceiling
            && c.len() != self.g.t.ceiling.len()
        {
            return Err(format!(
                "measured ceiling {} cells, want {}",
                c.len(),
                self.g.t.ceiling.len()
            ));
        }
        if let Some(a) = angle
            && a.len() != self.g.t.angle.len()
        {
            return Err(format!(
                "measured angle {} cells, want {}",
                a.len(),
                self.g.t.angle.len()
            ));
        }
        self.g.t.height.copy_from_slice(height);
        self.g.t.tile_type.copy_from_slice(tile_type);
        if let Some(c) = ceiling {
            self.g.t.ceiling.copy_from_slice(c);
        }
        if let Some(a) = angle {
            self.g.t.angle.copy_from_slice(a);
        }
        self.terrain_dirty = true;
        Ok(())
    }

    /// Seed the cast edge-trigger baseline (the held state of the tick
    /// BEFORE the imported one) so a held button does not re-edge on
    /// every imported pair.
    pub fn set_prev_fire(&mut self, left: bool, right: bool) {
        self.prev_fire = (left, right);
    }

    /// Arm the pose channel's mid-tick ground snapshot: the NEXT tick
    /// copies the height plane when its entity walk reaches the
    /// carpet anchor slot — the phase retail's own carpet mover
    /// probes ground at (:55151/:55103), with every lower-slot
    /// terraform of the tick applied and every higher-slot one still
    /// pending. Neither record endpoint has this image: terrain@N
    /// misses the low-slot digs, terrain@N+1 already carries the
    /// high-slot ones (the mc1l0 t=567 fire family, slots 692-705
    /// over carpet 630).
    pub fn arm_midtick_ground_snapshot(&mut self) {
        self.midtick_ground_armed = true;
        self.midtick_ground = None;
    }

    /// The armed snapshot, if the walk crossed the carpet anchor
    /// (consumes it; disarms a never-fired arm).
    pub fn take_midtick_ground_snapshot(&mut self) -> Option<Vec<u8>> {
        self.midtick_ground_armed = false;
        self.midtick_ground.take()
    }

    /// The engine's own bilinear ground sampler over a bare height
    /// plane (the mid-tick snapshot), engine units.
    pub fn ground_z_on_plane(plane: &[u8], x: u16, y: u16) -> i16 {
        crate::engine::features::Gen::interp_plane(plane, x, y) as i16
    }

    /// Apply a decoded MC1/HW retail closure onto this (already-built,
    /// same-level) world. Overwrites the pool, the free stack, the
    /// tile lists, the global LCG/spawn ordinals and the human player
    /// column; leaves terrain planes alone (see
    /// [`World::restore_planes`]).
    pub fn retail_import_mc1(&mut self, st: &RetailMc1) -> Result<ImportReport, String> {
        // Replaying retail state means retail law exactly: deliberate
        // gameplay deviations (DEVIATIONS.md) switch off for this world.
        self.strict_retail = true;
        self.patches = crate::patches::WorldPatches::RETAIL;
        let local = st.local_player as usize;
        let wiz = st
            .wizards
            .get(local)
            .ok_or_else(|| format!("local player {local} out of range"))?;
        let human_slot = wiz.play_index;
        let pool = self.g.ent.len();
        if human_slot == 0 || (human_slot as usize) >= pool.min(st.ents.len()) {
            return Err(format!("human carpet slot {human_slot} out of range"));
        }
        let carpet = st.ents[human_slot as usize];
        if carpet.class64 != 3 {
            return Err(format!(
                "human carpet slot {human_slot} is class {}, want 3",
                carpet.class64
            ));
        }
        // Seed the pose registers from the closure so the first tick
        // after the import sees the RECORDED carpet as its previous
        // position (retail pass order — the strict jar poll and its
        // kin read the carpet before its slot runs).
        self.human_pose = (carpet.x, carpet.y, carpet.z);
        self.human_pose_prev = self.human_pose;
        // The wizard pass anchors at the recorded carpet slot (the
        // class-3 dispatch position, sub_45C90) — above the spell
        // tokens, which is the cast-phase law's whole ordering.
        self.mc1_carpet_slot = human_slot;
        // The cast-arm hand bits (+16 & 0x300, :55886-95) and the
        // carpet pose the token fires measure from (retail reads the
        // wizard entity's own fields at the token's walk position =
        // the closure's settled values).
        self.mc1_hand_bits = carpet.flags & 0x300;
        self.mc1_cast_pose = crate::engine::world::PlayerPose {
            x: carpet.x,
            y: carpet.y,
            z: carpet.z,
            heading: carpet.f30,
            pitch: carpet.f32,
            speed: carpet.f126,
        };
        // The human's acquisition list (`Type_160+532`) verbatim: the
        // death scatter walks it in order (:55519-24) and the port has
        // no native model of it. Entries are pool slots while the
        // wizard lives and MODEL numbers once his landing has run
        // (:55523) — only the live form is a scatter order, and by
        // then the corpse has nothing left to throw, so the raw
        // negative/model values simply fail the class-12 gate in
        // `player_land`.
        self.mc1_acq = std::array::from_fn(|i| {
            wiz.spell_list
                .get(i)
                .and_then(|&s| u16::try_from(s).ok())
                .unwrap_or(0)
        });
        // The carpet's Type_156 is the canonical `&unk_98F38[7]`
        // (retail's own load-fixup anchor) — derive the table base
        // from it instead of hardcoding a per-build guest address.
        let behavior_base = carpet.model_ptr.wrapping_sub(7 * 32);
        let tr = |v: u16| if v == human_slot { PLAYER_TARGET } else { v };

        let n = pool.min(st.ents.len());
        let mut active = 0usize;
        let mut bad_rows = 0usize;
        for slot in 1..n {
            let r = &st.ents[slot];
            if slot == human_slot as usize {
                self.g.ent[slot] = Ent::default();
                // The carpet record stays class 0 (the pose is the
                // runner's input and the pass anchors at the slot),
                // but its +4 is a LIVE stream: the death scatter
                // spends three draws per jar on the dying wizard's
                // own `*(a1+4)` (:55538-46), never the world LCG.
                // Seed it or the landing throws the wrong offsets and
                // over-draws the graded rng channel by 15.
                self.g.ent[slot].rand = r.rand;
                continue;
            }
            if r.class64 == 0 {
                // A freed slot is not an EMPTY slot: retail's free
                // path clears +64 and pushes the stack — every other
                // byte stays, and the blind tracker steers at
                // whatever the record still holds (ledger §THE
                // PROJECTILE LEDGER + BLIND TRACKER; mc1l0 t=3464-70:
                // bolt 557 tracks reaped slot 534's stale position —
                // a defaulted slot re-aims it at the origin). Import
                // the stale bytes, class 0, not counted active; row
                // 0 stands in for the stale model_ptr (nothing live
                // dereferences a freed row).
                self.g.ent[slot] = import_ent(r, 0, &tr);
                continue;
            }
            active += 1;
            let row156 = if r.model_ptr == 0 {
                0
            } else {
                let d = r.model_ptr.wrapping_sub(behavior_base);
                if d % 32 == 0 && d / 32 < 256 {
                    (d / 32) as u8
                } else {
                    bad_rows += 1;
                    0
                }
            };
            self.g.ent[slot] = import_ent(r, row156, &tr);
        }
        for slot in n..pool {
            self.g.ent[slot] = Ent::default();
        }

        // Tile lists: the heads live in the map file, not the struct,
        // but the per-entity next20/prev22 ARE recorded — and chain
        // ORDER is observable: the first-hit probes (sub_11D10's cell
        // walk) resolve ties by it. mc1l0 pair 2371: two balls overlap
        // a grounding third; retail's chain order feeds the walk slot
        // 94 before 500, the old ascending re-link handed it 500
        // (head-insertion reverses slot order), and the merge picked
        // the wrong partner — a phantom (10,39) desync + mana fork.
        // So rebuild each cell chain in the RECORDED order: walk the
        // recorded links from each head (prev22 == 0), then link in
        // reverse (head-insertion restores the walk order). The human
        // slot is spliced out (the port's human is out-of-pool);
        // slots left unreachable by torn links keep the ascending
        // fallback. `import_ent` cleared the link bit; `link` re-sets
        // it.
        for h in self.g.map_entity.iter_mut() {
            *h = 0;
        }
        let mut seen = vec![false; n];
        let mut chains: Vec<Vec<usize>> = Vec::new();
        for head in 1..n {
            let r = &st.ents[head];
            if r.class64 == 0 || r.flags & 4 == 0 || r.prev22 != 0 || seen[head] {
                continue;
            }
            let mut chain = Vec::new();
            let mut cur = head;
            loop {
                if seen[cur] {
                    break; // cycle guard — torn capture
                }
                seen[cur] = true;
                if cur != human_slot as usize {
                    chain.push(cur);
                }
                let next = st.ents[cur].next20 as usize;
                if next == 0 || next >= n {
                    break;
                }
                let nr = &st.ents[next];
                if nr.class64 == 0 || nr.flags & 4 == 0 {
                    break;
                }
                cur = next;
            }
            chains.push(chain);
        }
        for chain in &chains {
            for &slot in chain.iter().rev() {
                let e = &self.g.ent[slot];
                let (x, y, z) = (e.x, e.y, e.z);
                self.g.link(slot, x, y, z);
            }
        }
        for slot in 1..n {
            if seen[slot] || slot == human_slot as usize {
                continue;
            }
            let e = &self.g.ent[slot];
            if e.class64 != 0 && st.ents[slot].flags & 4 != 0 {
                let (x, y, z) = (e.x, e.y, e.z);
                self.g.link(slot, x, y, z);
            }
        }

        // Free stack: the LIVE recorded order, so port-side spawns land
        // on the same slots the recording's do. Fall back to the
        // load-rebuild scan (999→1) only when the recorded stack is
        // unusable. The reserved human hole stays OUT either way.
        //
        // The RECYCLE stack is deliberately NOT chained in. Retail's
        // `NewEvent_372C0` pops the free stack and only falls through to
        // the recycle SACRIFICE arm once free is exhausted (:43867-83 vs
        // :43885-908), and MC1's recycle list is the respawn-window
        // sacrifice set alone (filled by sub_44D30 :54842, emptied at
        // :55056) — an arm the port's allocator never reaches with ~925
        // slots free, and a ruled deviation besides (DEVIATIONS.md
        // `death_regrant`). Chaining it did two things, both wrong: it
        // put recycle entries at the TOP of a Vec that `new_event` pops
        // from the end (inverting retail's priority), and it inflated
        // the census below so `live.len() == scan_free` could never hold
        // once a respawn had happened — throwing the whole recorded
        // order away for the fallback's lowest-free-slot rule. On mc1l2
        // that misfired from t≈8291 to the end of the take: at t=9089
        // retail's newborn (10,0) took slot 80 (ahead of its spawner at
        // 73, so it ticked in its birth frame and its blast landed on
        // the vulture the same frame), while the port took slot 18 and
        // the damage never arrived.
        let live: Vec<u16> = st
            .free_stack
            .iter()
            .copied()
            .filter(|&s| {
                (s as usize) < pool && s != human_slot && self.g.ent[s as usize].class64 == 0
            })
            .collect();
        let scan_free = pool - 1 - active - 1; // slots minus actives minus the hole
        let stack_fallback = if live.len() == scan_free {
            self.g.free = live;
            None
        } else {
            let got = live.len();
            self.g.free = (1..pool as u16)
                .rev()
                .filter(|&s| self.g.ent[s as usize].class64 == 0 && s != human_slot)
                .collect();
            Some((got, scan_free))
        };

        // Globals in the closure.
        self.g.rand = st.rand;
        self.g.spawn_count = st.spawn_count;
        // Outside the closure (retail leaves them unsaved too).
        self.g.pseudo = 0;
        self.g.erupting = 0;
        self.g.plume = 0;

        // The wizext+84 GUARD REGISTER is not in the recording:
        // rebuild its LIVE half from the owner-stamped (5,15) roster
        // (ascending slot order into the low register slots — the
        // fill order retail's own spawns produce). STALE entries —
        // the retail-only memory of dead guards that re-arms the +46
        // cooldown — are unknowable from a snapshot, so a pair whose
        // tick trips the stale→re-arm law diverges at that one
        // boundary (mc1l1 t=2571).
        self.g.mc1_guard_reg.0.clear();
        for slot in 1..n {
            let e = &self.g.ent[slot];
            if e.class64 == 5 && e.model65 == 15 && e.tick70 != 95 && e.f144 != 0 {
                let owner = e.f144;
                let reg = self
                    .g
                    .mc1_guard_reg
                    .0
                    .entry(owner)
                    .or_insert_with(|| vec![0u16; 34]);
                if let Some(k) = reg.iter().position(|&v| v == 0) {
                    reg[k] = slot as u16;
                }
            }
        }

        // The wizext+52 BALLOON REGISTER, by contrast, IS in the
        // recording — the closure carries the whole Type_160 slice
        // and `balloon_reg` decodes +52/+54/+56. Import it verbatim:
        // the register's INDEX order (spawn order, unrecoverable from
        // a pool census) is what decides which balloon claims which
        // ball and which one a downgrade culls, so a rebuilt-by-slot
        // stand-in hands the fleet its targets backwards. The KEY is
        // the owner stamp the dispatcher reads off the castle (+24),
        // so the human's carpet slot goes through `tr` like every
        // other owner reference.
        self.g.mc1_balloon_reg.0.clear();
        for w in &st.wizards {
            if w.play_index == 0 {
                continue;
            }
            self.g
                .mc1_balloon_reg
                .0
                .insert(tr(w.play_index), w.balloon_reg.to_vec());
        }

        // The human column: pool-entity state routes to Player, the
        // Type_160 tail to the Gen mirrors.
        self.g.player_mail = carpet.mail.map(|(a, s)| (a, tr(s)));
        self.g.player_knock = (wiz.knock_dir, wiz.knock_mag);
        self.g.player_aggro = wiz.aggro;
        self.g.player_danger = wiz.danger;
        self.g.banked_houses = wiz.banked_houses;
        self.g.castle_alert = wiz.castle_alert;
        self.g.player_alert = wiz.player_alert;
        self.g.balloon_alert = wiz.balloon_alert;
        self.g.kills = wiz.kills;
        self.g.shots = wiz.shots;
        self.g.hits = wiz.hits;
        self.g.player_invisible = carpet.flags & 0x20 != 0;
        self.g.player_rebound = carpet.flags & 0x8000 != 0;
        for i in 1..8 {
            self.g.rival_ents[i] = st.wizards[i].play_index;
            self.g.rival_wanted[i] = st.wizards[i].aggro;
        }
        self.g.rival_ents[0] = 0;

        // Re-anchor the rival AI records to the imported pool. The
        // records were built for the fresh world's spawn slots, and
        // rival_entity_tick keys on r.ent — without the rebind every
        // imported rival carpet is a frozen husk (its motion arm is
        // verbatim sub_14EB0 and simply never ran; the first HW
        // divergence family). Flight/economy lanes reseed from the
        // recorded closure so the one tick integrates from retail's
        // own state: vdes/jink are the Type_160 v_12/v_16 the motion
        // arm consumes, grace comes from the record (the fresh-spawn
        // 100 would wipe the imported mailbox), mana lanes come from
        // the carpet entity (f132 carries cast debits).
        for ri in 0..self.rivals.len() {
            let w = &st.wizards[self.rivals[ri].slot as usize];
            let r = &mut self.rivals[ri];
            r.ent = w.play_index;
            r.eliminated = w.play_index == 0;
            if r.eliminated {
                continue;
            }
            let e = &st.ents[w.play_index as usize];
            r.mana = e.f140.max(0) as u32;
            r.mana_max = e.f136.max(0) as u32;
            // +132 is a SIGNED 32-bit delta (cast debits are negative;
            // castle casts exceed 16 bits) — the old u16 decode turned
            // a −50 debit into +65486 and the apply clamped to the
            // ceiling.
            r.mana_delta = e.f132;
            r.vdes = w.cmd_speed;
            r.jink = w.strafe;
            // The pending knock impulse (Type_160 +24/+22). A live
            // rival never spends it, so a mid-life import carries a
            // stale one that only its death fall will cash.
            r.knock_dir = w.knock_dir;
            r.knock_mag = w.knock_mag;
            r.grace = w.grace;
            // Brain lanes: without these the record imports as Fresh
            // and the cascade re-aims f34 away from retail's lock.
            self.reanchor_rival_ai(
                ri,
                w.ai_state,
                w.burst,
                w.poverty,
                &w.cooldown,
                &w.learn,
                &w.hate,
                &w.war,
                &w.owned_slots,
                w.life_rate,
                w.regen_stall.min(u16::MAX as u32) as u16,
                st.ents[w.play_index as usize].f148,
            );
        }

        // Hands: the raw +940/+944 bytes index the ACQUISITION list,
        // not the spell table — resolve through the manifestation.
        //
        // A CORPSE'S LIST HOLDS MODELS, NOT SLOTS. The death landing
        // overwrites every live `+532` entry with its entity's +65
        // (:55523) and −1 for the empty ones, so the pool-slot
        // resolution above returns nothing for the whole dead window
        // — retail's own hands read empty there, which is the
        // measured `mc1-death-hand-spell-loss` law. But the RAW hand
        // bytes never move (:54884-923 refills the same list slots in
        // place), so the respawn hands back exactly the spells the
        // corpse went down with; resolve them straight off the list
        // or the re-grant has nothing to bind (mc1l42 t=17397, where
        // retail comes back holding Fireball/Possess and ours came
        // back empty-handed).
        let dead = carpet.f70 == 3;
        let hand = |raw: u16| {
            let s = if dead {
                wiz.spell_list
                    .get(raw as usize)
                    .and_then(|&m| u8::try_from(m).ok())
            } else {
                st.hand_spell(local, raw)
            };
            s.filter(|&s| (s as usize) < SPELL_COUNT).map(SpellId)
        };
        let mut death_owned = [false; SPELL_COUNT];
        let mut death_owned_blue = [false; SPELL_COUNT];
        for s in 0..SPELL_COUNT {
            death_owned[s] = wiz.owned_slots[s] != 0;
            death_owned_blue[s] = wiz.blue[s] != 0;
        }
        self.player = Player {
            mana: carpet.f140.max(0) as u32,
            mana_max: carpet.f136.max(0) as u32,
            // The pending regen amount (+132, applied-then-recomputed
            // by the wizard tick :55390/:55415-21 — the port keeps the
            // same one-tick pipeline). Left unseeded, every imported
            // pair ticked with delta 0 and missed retail's +100 floor
            // (or the +1000 castle-boost arm) — the two biggest
            // player.mana families in the corpus.
            //
            // The recorder samples +132 AFTER the recompute, so the
            // closure always reads the refreshed floor — but every
            // live MID-burst spell event zeroes it again before the
            // next apply (sub_55E80 :64956; the first burst tick,
            // +48 == +50, does not). The LAUNCHER and SPEED (2/21)
            // tokens now run that machine live (manifestation_tick
            // under strict, with the wizard pass applying after
            // them), so their pairs seed the recorded delta raw — a
            // mid-glide pair keeps an ABOVE-carpet token's pending
            // debit (mc1l1 t=8889-8910: six fireball −200 stamps
            // rode f132 through the accel glide). Only the
            // still-inert hold/channel/toggle tokens keep the seed
            // clamp.
            //
            // HEAL (1) COUNTS AS LIVE. `sub_56270` shares nothing
            // with the launcher skeleton but the +48 countdown — it
            // never calls the `sub_55E80` delta debit — so a wizard
            // mid-heal keeps his +100 floor, and the port runs the
            // token itself ([`World::mc1_heal_token_tick`], the same
            // exclusion the strict class-12 dispatch already makes).
            // Clamping on it cost the recorded regen outright
            // (mc1l42 t=10677/10684/10696: retail 800/400/500 against
            // our 700/300/400, two rows a tick).
            mana_delta: if st.ents.iter().any(|e| {
                e.class64 == 12
                    && e.f144 == 0
                    && e.f48 != 0
                    && e.f48 as i32 != e.f50 as i32
                    && !(e.f70 % 3 == 0
                        && matches!(
                            e.f70 / 3,
                            0 | 1
                                | 2
                                | 3
                                | 6
                                | 7
                                | 8
                                | 9
                                | 10
                                | 11
                                | 13
                                | 16
                                | 17
                                | 18
                                | 19
                                | 20
                                | 21
                                | 22
                        ))
            }) {
                0
            } else {
                // SIGNED 32-bit seed (see the rival arm above): +132
                // carries the cast debit as a negative value, and
                // castle-cast debits exceed 16 bits (mc1l1 t=3807:
                // −40000). Zero-extending the old u16 raw pinned the
                // player at the mana ceiling on every recorded cast
                // tick — the mc1l1 player.mana + carpet-mirror family
                // (2238 rows), and the idle 950-vs-1000 breathing
                // pairs before it.
                carpet.f132
            },
            life: carpet.act_life,
            // The player's life state rides the carpet's TICK-HANDLER
            // byte +70 (`*(_BYTE*)(a1+70) = 3`, :55550) — +66 is
            // sClass (255 on the carpet, so the old read left every
            // dead player Alive-with-negative-life and re-ran the
            // whole death cascade each pair: the HW 33 rng over-draw
            // runs at t=21468.. were exactly this).
            state: match carpet.f70 {
                2 => LifeState::Falling,
                3 => LifeState::Dead,
                _ => LifeState::Alive,
            },
            left: hand(wiz.hand_left),
            right: hand(wiz.hand_right),
            // A CORPSE OWNS NOTHING. `var_676` is the "spells ever
            // acquired" table — the jar poll's already-known marker
            // reads it (:64790) — and the death scatter never clears
            // it, so a dead wizard's entries still point at the jars
            // his landing threw away (:55519-47 rewrites the +532
            // acquisition list to MODEL numbers and clears each
            // token's owned bit instead). Importing that as ownership
            // made the respawn's re-grant hand back the SCATTERED
            // jars — mc1l42 t=17397 warped the five decaying jars to
            // the castle instead of minting the five fresh tokens
            // retail lays there.
            //
            // A CORPSE, THOUGH — NOT A FALLER. The rewrite is the
            // LANDING's (:55519-47), and `+70` only becomes 3 at
            // :55550, past it; a wizard still falling (`+70` 2) owns
            // his tokens exactly as an alive one does, and that
            // ownership IS what the landing scatters. Zeroing state 2
            // as well left the port's own landing with nothing to
            // throw — retail's five jars leapt to the death point
            // with fresh ttls and ours sat where they were (mc1l42
            // t=17343, 25 rows across flags/life/x/y/z).
            owned: match carpet.f70 {
                3 => [0u16; SPELL_COUNT],
                _ => wiz.owned_slots,
            },
            grace: wiz.grace,
            // The 16-tick post-hit life-regen stall (u32_383,
            // :55387-90). Unseeded, every pair inside retail's stall
            // window applied one heal quantum retail withheld — the
            // persistent life+5/+40 skew family.
            regen_delay: wiz.regen_stall.min(u16::MAX as u32) as u16,
            // The rate REGISTER (u16_341): applied-then-selected, so
            // a pair straddling a rate flip must inherit the stale
            // value (the castle-establish +5-then-+40 staircase).
            life_rate: wiz.life_rate as i32,
            killer: tr(carpet.f38),
            fall_speed: carpet.f46,
            shield: carpet.flags & 0x4000 != 0,
            invisible: carpet.flags & 0x20 != 0,
            rebound: carpet.flags & 0x8000 != 0,
            death_owned,
            death_owned_blue,
            ..Player::default()
        };

        // Per-wizard cast-charge meters (Type_160 u8_326) — seeded
        // like the regen stall: unseeded, every bolt spawned inside a
        // pair would bank a made-up charge in its +26.
        for (i, w) in st.wizards.iter().enumerate().take(8) {
            self.wiz_charge[i] = w.charge;
        }
        // World-level latches: cleared like retail's load discards its
        // mode block; the tick mailboxes must not leak across pairs.
        self.human_pose = (carpet.x, carpet.y, carpet.z);
        self.pending_teleport = None;
        self.pending_respawn = None;
        self.pending_restart = false;
        self.duel = None;
        self.won = false;
        self.completed = false;
        self.win_streak = 0;
        self.prev_fire = (false, false);
        self.accel_veto = (false, false);
        self.rival_deaths.clear();
        self.notification = None;
        self.kill_tally = [[0; 8]; 8];
        self.entities_dirty = true;

        Ok(ImportReport {
            active,
            human_slot,
            behavior_base,
            bad_rows,
            stack_fallback,
        })
    }

    /// Project this world into the recorder's MC1 obs schema. The
    /// human carpet is synthesized back at the pinned slot;
    /// `owner_ptr` (a guest pointer) is emitted as 0 and skipped by
    /// the comparator.
    /// THE UNGRADED RAW LANES — the per-entity bytes `EntObsMc1` does
    /// NOT carry: `+70` (the handler state), `+71` (the burst/charge
    /// register) and `+58`. The recording holds all three, and
    /// [`Self::retail_import_mc1`] restores all three every pair, so the
    /// graded diff can never see them: a handler that READS one
    /// correctly and WRITES it wrong is erased before it is ever
    /// observed, and pair mode reports CLEAN forever. Only a free run,
    /// which carries its own copy for thousands of ticks, feels it —
    /// which is why an mc1l42 free replay can be bit-exact in every
    /// graded field at t=6623 and still drop two `(10,23)` beam
    /// endpoints at t=6624.
    ///
    /// This is the shadow diff that catches the WRITE. Join it against
    /// the recorded state@N+1 in pair mode and every ungraded write bug
    /// in the take surfaces in one pass.
    /// THE WHOLE-WORLD DUMP, sectioned — the instrument of last resort
    /// when two runs of the PORT disagree and no schema-shaped lane can
    /// say why.
    ///
    /// The raw shadow and the free-stack lane cover everything the
    /// RECORDING holds; this covers everything the PORT holds, which is
    /// strictly more (the terrain planes, the tile heads, the wizard
    /// registers, the THING table, the player column). Diff two of
    /// these and the first differing section names the state that
    /// parted — the only way to attribute a free-run break whose entity
    /// pool, free list and every graded field are bit-identical.
    ///
    /// Sections rather than one blob because a byte offset into a
    /// 400 KB stream is not an answer; a section name is.
    pub fn debug_state_sections(&self) -> Vec<(&'static str, Vec<u8>)> {
        use crate::snapshot::Writer;
        let one = |f: &dyn Fn(&mut Writer)| {
            let mut w = Writer::new();
            f(&mut w);
            w.into_buf()
        };
        vec![
            ("terrain.height", one(&|w| w.put(&self.g.t.height))),
            ("terrain.tile_type", one(&|w| w.put(&self.g.t.tile_type))),
            ("terrain.shading", one(&|w| w.put(&self.g.t.shading))),
            ("terrain.angle", one(&|w| w.put(&self.g.t.angle))),
            ("terrain.ceiling", one(&|w| w.put(&self.g.t.ceiling))),
            ("map_entity", one(&|w| w.put(&self.g.map_entity))),
            ("ent", one(&|w| w.put(&self.g.ent))),
            ("free", one(&|w| w.put(&self.g.free))),
            ("rand", one(&|w| w.put(&self.g.rand))),
            ("pseudo", one(&|w| w.put(&self.g.pseudo))),
            ("spawn_count", one(&|w| w.put(&self.g.spawn_count))),
            ("player_mail", one(&|w| w.put(&self.g.player_mail))),
            ("player_knock", one(&|w| w.put(&self.g.player_knock))),
            ("player_aggro", one(&|w| w.put(&self.g.player_aggro))),
            ("rival_wanted", one(&|w| w.put(&self.g.rival_wanted))),
            ("erupting", one(&|w| w.put(&self.g.erupting))),
            ("plume", one(&|w| w.put(&self.g.plume))),
            ("kills.shots.hits", {
                let mut w = Writer::new();
                w.put(&self.g.kills);
                w.put(&self.g.shots);
                w.put(&self.g.hits);
                w.into_buf()
            }),
            ("player_danger", one(&|w| w.put(&self.g.player_danger))),
            ("banked_houses", one(&|w| w.put(&self.g.banked_houses))),
            ("exhausted", one(&|w| w.put(&self.g.exhausted))),
            ("misfits", one(&|w| w.put(&self.g.misfits))),
            // The two wizard registers are maps, not `Snap` values —
            // rendered as text, which diffs just as well.
            (
                "mc1_guard_reg",
                format!("{:?}", self.g.mc1_guard_reg.0).into_bytes(),
            ),
            (
                "mc1_balloon_reg",
                format!("{:?}", self.g.mc1_balloon_reg.0).into_bytes(),
            ),
            ("thing_table", one(&|w| w.put(&self.table))),
            ("player", one(&|w| w.put(&self.player))),
            ("rivals", one(&|w| w.put(&self.rivals))),
            ("kill_tally", one(&|w| w.put(&self.kill_tally))),
            ("human_pose", one(&|w| w.put(&self.human_pose))),
            ("rival_deaths", one(&|w| w.put(&self.rival_deaths))),
            ("duel", one(&|w| w.put(&self.duel))),
            ("mc1_ring", one(&|w| w.put(&self.mc1_ring))),
            ("mc1_v14", one(&|w| w.put(&self.mc1_v14))),
            ("prev_fire", one(&|w| w.put(&self.prev_fire))),
            ("accel_veto", one(&|w| w.put(&self.accel_veto))),
            ("win_pct", one(&|w| w.put(&self.win_pct))),
            ("placeholders", one(&|w| w.put(&self.placeholders))),
        ]
    }

    pub fn raw_shadow_mc1(&self) -> Vec<RawShadowMc1> {
        (1..self.g.ent.len() as u16)
            .filter_map(|slot| {
                let e = &self.g.ent[slot as usize];
                (e.class64 != 0).then_some(RawShadowMc1 {
                    slot,
                    class: e.class64,
                    model: e.model65,
                    f70: e.tick70,
                    f71: e.f71,
                    f58: e.f58,
                    f44: e.f44,
                    next20: e.next20,
                    prev22: e.prev22,
                    f26: e.f26,
                    f28: e.f28,
                    f36: e.f36,
                    f38: e.f38,
                    f40: e.f40,
                    f46: e.f46,
                    f50: e.f50,
                    f52: e.f52,
                    f54: e.f54,
                    f56: e.f56,
                    f59: e.f59,
                    f68: e.f68,
                    f69: e.f69,
                    f78: e.f78,
                    f80: e.f80,
                    f82: e.f82,
                    f84: e.f84,
                    type86: e.type86,
                    frame88: e.frame88,
                    frames89: e.frames89,
                    mail: e.mail,
                    f128: e.f128,
                    f130: e.f130,
                    f144: e.f144,
                    dest_x: e.dest_x,
                    dest_y: e.dest_y,
                    site_z: e.site_z,
                })
            })
            .collect()
    }

    /// The port's free list, bottom-to-top (`new_event` pops the END) —
    /// the WORLD-level counterpart of [`Self::raw_shadow_mc1`].
    ///
    /// It is the widest ungraded lane in the harness: the recording
    /// carries the stack per tick, [`Self::retail_import_mc1`] installs
    /// it every pair, and the obs schema never compares it. So a port
    /// that pushes a freed slot at the wrong moment — or frees a
    /// different NUMBER of slots — reads CLEAN in pair mode forever and
    /// only bites a free run, where it surfaces as balanced
    /// same-`(class, model)` missing/extra rows once the two allocators
    /// hand out different slots for the same spawn.
    pub fn free_stack_mc1(&self) -> &[u16] {
        &self.g.free
    }

    pub fn obs_project_mc1(&self, pin: &PinnedMc1) -> ObsMc1 {
        let untr = |v: u16| if v == PLAYER_TARGET { pin.slot } else { v };
        let mut entities: Vec<EntObsMc1> = Vec::new();
        for slot in 1..self.g.ent.len() as u16 {
            if slot == pin.slot {
                entities.push(self.synth_carpet_obs(pin));
                continue;
            }
            let e = &self.g.ent[slot as usize];
            if e.class64 == 0 {
                continue;
            }
            entities.push(EntObsMc1 {
                slot,
                class: e.class64,
                model: e.model65,
                sclass: e.f66,
                smodel: e.f67,
                flags: e.flags,
                id: untr(e.id24),
                life: e.act_life,
                max_life: e.max_life,
                x: e.x as f64 / 256.0,
                y: e.y as f64 / 256.0,
                z: e.z,
                heading: e.f30,
                pitch: e.f32,
                target_yaw: e.f34,
                speed: e.f126,
                mana: e.f140 as u32,
                mana_max: e.f136 as u32,
                chase: untr(e.f146),
                owner_ptr: 0,
                tick_byte: e.f63,
                rand: e.rand,
            });
        }
        let castle_of = |owner: u16| -> u16 {
            if owner == 0 {
                return 0;
            }
            // Retail's wizard +50 is written ONLY by the level-up arm
            // (sub_47960 :56484) and cleared by the level-down-to-0 /
            // removal path (:56534) — a freshly landed level-0 flag is
            // NOT yet bound (mc1l0 t=562: flag live, +50 still 0), so
            // the scan requires an established level. (Rival direct
            // mint :19206 binds at spawn; the port mints leveled — the
            // one-tick level-0 window is the rival-cast-phase lane.)
            self.g
                .ent
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, e)| {
                    e.class64 == 3
                        && e.model65 == 2
                        && e.id24 == owner
                        && e.flags & 0x400 == 0
                        && e.f26 > 0
                })
                .map_or(0, |(s, _)| s as u16)
        };
        // A CORPSE SHOWS EMPTY HANDS. The raw +940/+944 registers
        // survive the death untouched, but the list they index has
        // been rewritten to MODEL numbers by the landing (:55523), so
        // retail's own resolution — and the comparator's, which runs
        // the same `hand_spell` walk — reads None for the whole dead
        // window. The port carries the resolved spell instead, so the
        // emptiness has to be projected here; clearing the registers
        // would lose what the respawn hands straight back (mc1l42
        // t=17343 vs t=17397, the mirrored pair).
        let corpse = self.player.state == LifeState::Dead;
        let spell_u16 = |s: Option<SpellId>| s.filter(|_| !corpse).map(|s| s.0 as u16);
        let wizards: Vec<WizardMc1> = (0..8u16)
            .map(|i| {
                let localw = i == pin.local;
                let owner = if localw {
                    PLAYER_TARGET
                } else {
                    self.g.rival_ents[i as usize]
                };
                WizardMc1 {
                    index: i,
                    play_index: if localw {
                        pin.slot
                    } else {
                        self.g.rival_ents[i as usize]
                    },
                    hand_left: if localw {
                        spell_u16(self.player.left)
                    } else {
                        None
                    },
                    hand_right: if localw {
                        spell_u16(self.player.right)
                    } else {
                        None
                    },
                    castle: castle_of(owner),
                    flight: FlightMc1 {
                        cmd_speed: if localw { pin.pose.speed } else { 0 },
                        strafe: 0,
                        roll_acc: 0,
                        pitch_acc: 0,
                    },
                }
            })
            .collect();
        let control: Vec<ControlMc1> = (0..8u16).map(zero_control).collect();
        let player = Some(PlayerJoinMc1 {
            carpet_slot: pin.slot,
            life: self.player.life,
            max_life: PLAYER_LIFE_MAX as u32,
            mana: self.player.mana,
            mana_max: self.player.mana_max,
            x: pin.pose.x as f64 / 256.0,
            y: pin.pose.y as f64 / 256.0,
            z: pin.pose.z,
            heading: pin.pose.heading,
            pitch: pin.pose.pitch,
            speed: pin.pose.speed,
            hand_left: spell_u16(self.player.left),
            hand_right: spell_u16(self.player.right),
            castle: wizards[pin.local as usize].castle,
            flight: wizards[pin.local as usize].flight.clone(),
            control: Some(zero_control(pin.local)),
        });
        ObsMc1 {
            rng: self.g.rand,
            n_active: entities.len() as u32,
            local_player: pin.local,
            player_count: pin.player_count,
            wizards,
            control,
            player,
            entities,
        }
    }

    /// The conformance RAW-lane projection: per-slot f26 (the burst/
    /// level lane) plus the per-wizard charge meters. These never
    /// entered the recorder's obs schema (adding them would break
    /// `check-decode` against the whole corpus), so the comparator
    /// reads them from the raw state channel instead — this is the
    /// port-side half of that comparison.
    pub fn charge_lane_mc1(&self) -> (Vec<(u16, i16)>, [u8; 8]) {
        let f26 = self
            .g
            .ent
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, e)| e.class64 != 0)
            .map(|(s, e)| (s as u16, e.f26))
            .collect();
        (f26, self.wiz_charge)
    }

    /// Apply a decoded MC2 retail closure onto this (already-built,
    /// same-level) world. The MC2 twin of [`World::retail_import_mc1`]
    /// — same shape: overwrite the pool, rebuild the tile lists and
    /// the free stack, seed the globals and the human column, clear
    /// the cross-pair latches.
    pub fn retail_import_mc2(&mut self, st: &RetailMc2) -> Result<ImportReport, String> {
        self.strict_retail = true;
        self.patches = crate::patches::WorldPatches::RETAIL;
        let local = st.local_player as usize;
        let ply = st
            .players
            .get(local)
            .ok_or_else(|| format!("local player {local} out of range"))?;
        let human_slot = ply.play_index;
        let pool = self.g.ent.len();
        if human_slot == 0 || (human_slot as usize) >= pool.min(st.ents.len()) {
            return Err(format!("human carpet slot {human_slot} out of range"));
        }
        let carpet = st.ents[human_slot as usize];
        if carpet.class3f != 3 {
            return Err(format!(
                "human carpet slot {human_slot} is class {}, want 3",
                carpet.class3f
            ));
        }
        // Pose registers from the closure (see the MC1 twin): the
        // first post-import tick's previous-position reads must see
        // the RECORDED carpet, not a stale pose.
        self.human_pose = (carpet.x, carpet.y, carpet.z);
        self.human_pose_prev = self.human_pose;
        let tr = |v: u16| if v == human_slot { PLAYER_TARGET } else { v };

        // Anchor the per-tick counter to the recording: it feeds the
        // cave-drip 8-turn cadence AND the cave carpet-tail rand
        // perturbation (World::tick) — both key on its POST-increment
        // value. Retail resets it at level load, so the local
        // player's Turn is its exact value. The carpet's byte[1]&8
        // one-shot (EF:59616) arms the tail skip, and so do the
        // action arms that never call the mover `sub_5D530`: only
        // flying (0, EF:59994) and the death-test arm (2, EF:60074)
        // reach it — the level-end arm (12, mc2l30 t=9090..) parks
        // the tail entirely, and possession holds byte[1]&8 across
        // its whole window (t=3257-3267).
        self.mc2_turn = ply.turn.max(0) as u32;
        self.mc2_carpet_slot = human_slot;
        self.mc2_carpet_stall = carpet.flags & 0x800 != 0 || !matches!(carpet.action45, 0 | 2);

        let n = pool.min(st.ents.len());
        let mut active = 0usize;
        let mut bad_rows = 0usize;
        // A record with the disable bit (byte[1] & 4) is a GHOST:
        // retail pushed its slot to the free stack at disable but
        // nothing zeroes the pool bytes, so the stale record persists
        // (and projects) until reallocation overwrites it. Import the
        // record for the projection, but the slot belongs to the free
        // side of the census.
        let ghost = |r: &RetailEntMc2| (r.flags >> 8) & 4 != 0;
        for slot in 1..n {
            let r = &st.ents[slot];
            if r.class3f == 0 || slot == human_slot as usize {
                self.g.ent[slot] = Ent::default();
                continue;
            }
            if !ghost(r) {
                active += 1;
            }
            // Behavior row: `ptr_a0` points into `str_D7BD6[]`;
            // retail's own load fixup is `(ptr − base160)/34 + 59`
            // (Level.cpp:1255-57; base160 = the saved `&str_D7BD6[59]`).
            // This ABSOLUTE `str_D7BD6` index is what every MC2 tick
            // reads via `BEHAVIOR[row156]`.
            let mut row156 = {
                let d = r.ptr_a0.wrapping_sub(st.base160) as i32;
                let steps = d / 34;
                if d % 34 == 0 && (-59..98).contains(&steps) {
                    (steps + 59) as u8
                } else {
                    bad_rows += 1;
                    59
                }
            };
            // (3,3) balloon exception: `mc2_balloon_tick` (castle.rs)
            // is the ONE tick that indexes RELATIVE to `ROW_BASE`
            // (`BEHAVIOR[ROW_BASE + row156]`), matching its native
            // `mc2_spawn_balloon` (`row156 = 9` → abs 68). The retail
            // ctor `sub_4ABA0` pins `&str_D7BD6[68]` (EF:33422), so
            // the generic absolute import (68) double-offset to
            // `BEHAVIOR[127]` (v_12=0, v_14=−128) — sinking every
            // imported balloon 128/tick (the mc2-balloon-z lever).
            // Hand it the relative index the balloon tick expects.
            if r.class3f == 3 && r.model40 == 3 {
                row156 = row156.saturating_sub(crate::mc2::behavior::ROW_BASE as u8);
            }
            self.g.ent[slot] = import_ent_mc2(r, slot as u16, row156, &tr);
        }
        for slot in n..pool {
            self.g.ent[slot] = Ent::default();
        }

        // Tile lists: MC2 maintains its chains incrementally, but the
        // per-tile head array (`mapEntityIndex_15B4E0`) lives OUTSIDE
        // `D41A0_0` (a separate SMAP global the recording does not
        // carry), so the chains rebuild here in ascending slot order —
        // retail's historical insertion order is unrecoverable, and
        // any chain-order-sensitive tie surfaces as a family.
        for h in self.g.map_entity.iter_mut() {
            *h = 0;
        }
        for slot in 1..n {
            let e = &self.g.ent[slot];
            // Ghosts never link: retail unlinks at disable — the
            // record's link bit is stale bytes. A linked ghost whose
            // slot is later reallocated leaves a dangling chain
            // pointer (a tile-chain CYCLE once the new occupant
            // relinks on the same tile — the pair-9074 OOM).
            if e.class64 != 0 && st.ents[slot].flags & 4 != 0 && !ghost(&st.ents[slot]) {
                let (x, y, z) = (e.x, e.y, e.z);
                self.g.link(slot, x, y, z);
            }
        }

        // Free stack: retail pops the FREE stack first and recycle
        // victims only when it is exhausted (`NewEvent_4A050`) — the
        // opposite priority of MC1. The port pops from the Vec's end,
        // so the free stack goes on top (recycle below), preserving
        // the recorded allocation order. Fallback = retail's own load
        // rebuild (`sub_49F90`): descending slot scan, lowest free
        // slot ends on top.
        // Ghost slots are NOT in the recorded stacks: retail's
        // disable leaves the record and the slot in limbo until the
        // NEXT frame's top reap (UpdateEntities EF:39948-56) unlinks,
        // class-zeroes and pushes it (measured: the t=1 snapshot's
        // stack is exactly the ghost count short, and the reused
        // slots pop highest-first = an ascending push scan). tick()'s
        // top reap performs that push for strict MC2 — the import
        // only counts ghosts for the census; appending them here too
        // would double-push the slots.
        let ghost_slots: Vec<u16> = (1..n as u16)
            .filter(|&s| {
                let e = &self.g.ent[s as usize];
                s != human_slot && e.class64 != 0 && e.flags & 0x400 != 0
            })
            .collect();
        let live: Vec<u16> = st
            .recycle_stack
            .iter()
            .chain(st.free_stack.iter())
            .copied()
            .filter(|&s| {
                (s as usize) < pool && s != human_slot && self.g.ent[s as usize].class64 == 0
            })
            .collect();
        let scan_free = pool - 1 - active - 1 - ghost_slots.len();
        let stack_fallback = if live.len() == scan_free {
            self.g.free = live;
            None
        } else {
            let got = live.len();
            self.g.free = (1..pool as u16)
                .rev()
                .filter(|&s| s != human_slot && self.g.ent[s as usize].class64 == 0)
                .collect();
            Some((got, scan_free))
        };
        // The recycle-victim stack rides along, order preserved, so a
        // full-pool spawn sacrifices the SAME live entity retail's
        // `NewEvent_4A050` fallback would (:581). `refill` stays clear:
        // the recorded stack is retail's own snapshot, and running it
        // dry is retail returning null, not a cue to re-rank the pool.
        // Ghosts are excluded — they are still class-bearing here, but
        // `tick()`'s top reap pushes them onto the FREE stack, and a
        // slot on both stacks could be handed out twice.
        //
        // `MGC_NO_RECYCLE_VICTIM=1` is the A/B toggle: it leaves the
        // stack empty, i.e. the pre-dig port that simply fails every
        // full-pool spawn. The runner's measurements are taken unset.
        self.g.mc2_recycle.refill = false;
        let no_victims = std::env::var_os("MGC_NO_RECYCLE_VICTIM").is_some();
        self.g.mc2_recycle.stack = st
            .recycle_stack
            .iter()
            .copied()
            .filter(|&s| {
                if no_victims {
                    return false;
                }
                let live = (s as usize) < pool && s != human_slot && s != 0;
                live && {
                    let e = &self.g.ent[s as usize];
                    e.class64 != 0 && e.flags & 0x400 == 0
                }
            })
            .collect();

        // NO ghost push here — see the census note above: `tick()`'s
        // strict-MC2 top reap is the ONE pusher (measured 2026-08-03,
        // mc2l24 pair 53808: the extra `extend` left the pyramid's
        // 17-slot worm chain popping [905, 837, 813, 796, 727, 690]
        // TWICE, so the second pop of 905 re-`NewEvent`ed the chain's
        // own HEAD — `Ent::default()` over a live record — and the
        // whole chain projected as class 0. Retail's stack is exactly
        // the recorded 716 + the 6 ghosts the reap pushes.)

        // Globals in the closure.
        self.g.rand = st.rand;
        self.g.mc2_spawn_ord.0[..29].copy_from_slice(&st.spawn_ord);
        // Outside the closure: the retile LCG (pseudo) has no capture.
        self.g.pseudo = 0;
        // The volcano-vortex / fire-column singletons (D41A0 word_0x31
        // / word_0x33, header +0x31/+0x33) ARE captured for MC2. The
        // (10,18) re-eruption reset (`sub_32A70`, EF:23924) gates on
        // word_0x31 being clear, and it is NOT reconstructable from
        // entity state: the persistent controller reads it 0 before
        // re-erupting and its own slot afterwards, with an identical
        // entity record either way. A forced 0 makes it re-erupt on
        // every >2500 roll where retail actually holds the latch
        // (mc2l30 slot 134 after t=2536, ~13 phantom eruptions). Both
        // are 0 on non-volcano levels, so mc2l0/l4 are unaffected.
        self.g.erupting = st.vortex;
        self.g.plume = st.fire_col;

        // StageVar held bindings: retail keeps `StageVar1_0x48_72` +
        // the `word_0x4A_74` timer ON the entity; the port's side-vec
        // rebuilds from them.
        //
        // The live var table's RUNTIME lanes overlay from the recorded
        // rows @0x365F4 each pair (kind/flags/chain/cadence, and the
        // kind-6/7 param word) — without this the port's table carried
        // its own FIRED/cadence mutations across pairs (the suite's
        // self-drift). Loader-DERIVED fields (hold_word/subtypes/
        // watch_template) stay from the level build: the &2-clear
        // watch payload can be a bound-entity guest pointer in the
        // raw row (EF:4740), which the sv1 lanes already reconstruct.
        for (i, raw) in st.stagevars.iter().enumerate() {
            let Some(v) = self.mc2_stagevars.get_mut(i) else {
                break;
            };
            v.kind = raw[0] & 0xF;
            v.flags = raw[1];
            v.chain = raw[2];
            v.cadence = raw[3];
            if matches!(v.kind, 6 | 7) {
                v.param = u16::from_le_bytes([raw[4], raw[5]]);
            }
        }
        self.mc2_sv_held.clear();
        self.mc2_sv_deferred.clear();
        for slot in 1..n {
            let r = &st.ents[slot];
            if r.class3f != 0 && slot != human_slot as usize && r.sv1 > 0 && !ghost(r) {
                self.mc2_sv_held.push(crate::mc2::stagevars::Mc2Held {
                    ent: slot as u16,
                    slot: r.sv1 as u8,
                    timer: r.sv_timer,
                });
            }
        }

        // Per-player columns: pool wizard slots + WANTED timers.
        // MC2's wanted table keys on the wizard's ENTITY slot
        // (`mc2_wanted`, hash-quiet while empty); MC1's per-player
        // `rival_wanted` array stays zero.
        self.g.mc2_wanted.0.clear();
        self.g.mc2_allied.0.clear();
        self.g.mc2_aura_claim.0.clear();
        self.g.mc2_debuffs = Default::default();
        for i in 0..8 {
            let p = st.players.get(i);
            self.g.rival_ents[i] = match p {
                Some(p) if i != local => tr(p.play_index),
                _ => 0,
            };
            self.g.rival_wanted[i] = 0;
            if let Some(p) = p {
                if i != local && p.play_index != 0 && p.wanted > 0 {
                    self.g.mc2_wanted.0.insert(p.play_index, p.wanted as u16);
                }
            }
        }
        self.g.rival_ents[local] = 0;
        // MC2 rival re-anchor — the MC1 rival-freeze twin: the
        // class-3 dispatch keys on `mc2_rivals[ri].ent`, which the
        // world-build seeded with fresh spawn slots, so every
        // imported rival carpet replayed as a frozen husk (the mc2l4
        // (3,1) family: obs@1 == state@0 verbatim for the wizard's
        // whole life — the motion law itself is verbatim EF:6484).
        // The DECISION half follows in `reanchor_mc2_rival_ai`: the
        // wizard-extension brain lanes plus the two that ride the
        // wizard entity, so the replayed rival resumes retail's
        // decision instead of re-running the cascade.
        //
        // `SpellIndexLeft/Right` are DIRECT spell indices in MC2 (-1 =
        // empty) — shared by the rival books here and the human's
        // below.
        let book_hand = |raw: i16| {
            if (0..26).contains(&raw) {
                raw as i8
            } else {
                -1
            }
        };
        for ri in 0..self.mc2_rivals.len() {
            let slot = self.mc2_rivals[ri].slot as usize;
            match st.players.get(slot) {
                Some(p) if slot != local && p.play_index != 0 => {
                    let ent = tr(p.play_index);
                    let e = &st.ents[p.play_index as usize];
                    self.reanchor_mc2_rival(
                        ri,
                        ent,
                        p.cmd_speed,
                        p.strafe,
                        p.invuln.max(0) as u16,
                        e.mana.max(0) as u32,
                        e.mana_max.max(0) as u32,
                        e.d88,
                    );
                    let ai = crate::mc2::rivals::Mc2RivalAi {
                        state: p.ai_state,
                        target: tr(e.target96),
                        target_sig: e.f98,
                        site: (e.dest_x, e.dest_y),
                        burst: p.burst,
                        poverty: p.poverty,
                        cooldown: p.cooldown,
                        hate: p.hate,
                        war: p.war,
                        weave: p.weave.max(0) as u8,
                        weave_dir: p.weave_dir.max(0) as u8,
                        avoid: p.avoid.max(0) as u8,
                        avoid_exit: p.avoid_exit.max(0) as u8,
                        aggression: p.aggression.max(0) as u16,
                        perception: p.perception.max(0) as u16,
                        reflexes: p.reflexes.max(0) as u16,
                        life_scale: p.life_scale.max(0) as u16,
                    };
                    let book = crate::mc2::cast::Mc2Spellbook {
                        ent: p.spell_ent,
                        xp_vol: p.xp_vol,
                        xp_bank: p.xp_bank,
                        levels: p.levels,
                        sel: p.sel,
                        left: book_hand(p.hand_left),
                        right: book_hand(p.hand_right),
                        ring: p.ring,
                    };
                    self.reanchor_mc2_rival_ai(ri, &ai, &book);
                }
                _ => self.reanchor_mc2_rival(ri, 0, 0, 0, 0, 0, 0, 0),
            }
        }
        self.g.player_aggro = ply.wanted;
        self.g.player_danger = carpet.f36 as i16;
        self.g.player_mail = carpet.mail.map(|(a, s)| (a.max(0) as u32, tr(s)));
        self.g.player_invisible = carpet.flags & 0x20 != 0;
        self.g.mc2_player_drain.0 = 0;

        // The human column. MC2 hands are DIRECT spell indices
        // (SpellIndexLeft/Right; −1 = empty) — no acquisition-list
        // indirection like MC1.
        let hand = |raw: i16| {
            (0..SPELL_COUNT as i16)
                .contains(&raw)
                .then_some(SpellId(raw as u8))
        };
        self.player = Player {
            mana: carpet.mana.max(0) as u32,
            mana_max: carpet.mana_max.max(0) as u32,
            // The pending regen/debit delta (@0x88) — the value the
            // wizard body will APPLY next frame, which is NOT always
            // the recorded one: see [`mc2_applied_mana_delta`].
            mana_delta: mc2_applied_mana_delta(st, ply, human_slot, &carpet),
            life: carpet.life,
            // MORTALITY (the MC1 arm's twin): the human carpet's
            // `actionIndex_0x45_69` IS the wizard's life state on the
            // MC2 column too — 0 alive (`AddPlayer03_00_5E010`), 2 the
            // death fall (`sub_5E310`), 3 the corpse waiting for Space
            // (`sub_5E7C0`). Pinning `Alive` here ran the whole regen
            // block on a corpse: +maxLife/250 life and the stale
            // `manaRegen` (@0x88) both landed every corpse pair, and
            // the imported mana clamped to `mana_max` — retail's
            // corpse touches neither (EF:59994-60040 gates the block
            // on `life >= 0`).
            state: match carpet.action45 {
                _ if super::mc2_death_off() => LifeState::Alive,
                2 => LifeState::Falling,
                3 => LifeState::Dead,
                _ => LifeState::Alive,
            },
            left: hand(ply.hand_left),
            right: hand(ply.hand_right),
            grace: ply.invuln.max(0) as u16,
            // The 16-tick post-hit life-regen stall (dword_0x18D_397,
            // EF:60000-60003; armed EF:60662/60710/62222 on
            // hit/grip/steal). Unseeded, every pair inside retail's
            // stall window applied one heal quantum (5 afield, 40 at
            // castle/dolmen) retail withheld — the cross-take
            // player.life +5 family.
            regen_delay: ply.regen_stall.clamp(0, u16::MAX as i32) as u16,
            killer: tr(carpet.f24 as u16),
            fall_speed: carpet.f2c,
            invisible: carpet.flags & 0x20 != 0,
            ..Player::default()
        };
        // The knock/buffet channel (`moveBoost` @+30 + direction @+32:
        // the MC1 channel's retail home on this column — same cap 128,
        // decay −4, snap <4): the MC1 arm seeds its twin from the
        // Type_160 tail; without this a free-running replay anchored
        // mid-buffet starts with a silently empty channel.
        self.g.player_knock = (ply.knock_dir, ply.knock_mag);

        // The human's str_611 spellbook: manifestation slots, XP,
        // and tier state live in the per-player block and mutate at
        // runtime (casts, kills, releveling) — the world-build
        // seeding is cross-pair state, so rebuild from the closure.
        // Without this the cast machinery ticks whatever slots the
        // level build assigned, not the imported manifestations.
        self.mc2_book = crate::mc2::cast::Mc2Spellbook {
            ent: ply.spell_ent,
            xp_vol: ply.xp_vol,
            xp_bank: ply.xp_bank,
            levels: ply.levels,
            sel: ply.sel,
            left: book_hand(ply.hand_left),
            right: book_hand(ply.hand_right),
            ring: ply.ring,
        };

        // TERRAIN REPLAY. `.mgcr` has no terrain channel, so the pool
        // lands on PRISTINE heights while retail's map still carries
        // every already-run (14,1) riser's write. That write is a
        // pure function of the riser's own imported state, so replay
        // it (mc2::riser::mc2_riser_reconstruct) — a removed riser's
        // 3-row endcaps stand at +48 forever and are what fences the
        // walkers/dwellers retail keeps out of the walled compounds.
        // The BUILD00 pad stampers (mc2::pads) are the same shape and
        // dominate the residual: a (3,2) castle's cumulative (10,42)
        // painter pad and a village building's own action-51 terrace
        // both end at an ABSOLUTE `pad + datum`, and the recording
        // carries every input (cell, BUILD00 row, `site_z` datum). The
        // world build already settles the AUTHORED stamps, so both
        // replays are no-ops there and only recover what the take
        // itself built or levelled up. Castles first: a castle build
        // purges the buildings inside its footprint, so a surviving
        // building never overlaps a castle pad.
        //
        // `MGC_NO_PAD_REPLAY` is the terrain-replay A/B toggle:
        // `1`/`all` disables both arms, `castle`/`building` one of
        // them. The runner's own measurements are taken with it unset.
        let off = std::env::var("MGC_NO_PAD_REPLAY").unwrap_or_default();
        let off = |arm: &str| off == "1" || off == "all" || off == arm;
        if !off("castle") {
            for i in 0..self.g.ent.len() {
                if self.g.ent[i].class64 == 3 && self.g.ent[i].model65 == 2 {
                    self.g.mc2_castle_pad_reconstruct(i);
                }
            }
        }
        if !off("building") {
            for i in 0..self.g.ent.len() {
                if self.g.ent[i].class64 == 10 && matches!(self.g.ent[i].tick70, 51 | 52) {
                    self.g.mc2_building_pad_reconstruct(i);
                }
            }
        }
        for i in 0..self.g.ent.len() {
            if self.g.ent[i].class64 == 14 && self.g.ent[i].model65 == 1 {
                self.g.mc2_riser_reconstruct(i);
            }
        }
        // The STATIC GROUND PROBES run LAST (mc2::probes): the three
        // class-2 snap laws pin `z` to the interpolated ground every
        // tick on an entity that never moves, so each prop's imported
        // `z` is a terrain SAMPLE the recorder captured without
        // knowing it — the only handle the format gives on ground the
        // take dug with edits whose casters are long gone (fire
        // scorch, craters). Inverting the sampler over the ≤4 cells it
        // reads is the last pass, so a prop standing on a replayed
        // pad/riser sees the finished map and solves to a no-op.
        // `MGC_NO_STATIC_TERRAIN_REPLAY=1` is its A/B toggle.
        if std::env::var("MGC_NO_STATIC_TERRAIN_REPLAY").unwrap_or_default() != "1" {
            let cost = self.g.mc2_ground_reader_cost();
            let mut claimed = std::collections::BTreeSet::new();
            for i in 0..self.g.ent.len() {
                if self.g.mc2_is_ground_probe(i) {
                    self.g.mc2_static_ground_reconstruct(i, &mut claimed, &cost);
                }
            }
        }

        // Cross-pair latches, same wipe as the MC1 arm.
        self.human_pose = (carpet.x, carpet.y, carpet.z);
        self.pending_teleport = None;
        self.pending_respawn = None;
        self.pending_restart = false;
        self.duel = None;
        self.won = false;
        self.completed = false;
        self.win_streak = 0;
        self.prev_fire = (false, false);
        self.accel_veto = (false, false);
        self.rival_deaths.clear();
        self.notification = None;
        self.kill_tally = [[0; 8]; 8];
        self.entities_dirty = true;

        Ok(ImportReport {
            active,
            human_slot,
            behavior_base: st.base160,
            bad_rows,
            stack_fallback,
        })
    }

    /// Project this world into the recorder's MC2 obs schema — the
    /// twin of [`World::obs_project_mc1`]. Port fields translate back
    /// through the SEMANTIC alias table (mc2/mobs.rs), the reverse of
    /// `import_ent_mc2`.
    pub fn obs_project_mc2(&self, pin: &PinnedMc2) -> ObsMc2 {
        let untr = |v: u16| if v == PLAYER_TARGET { pin.slot } else { v };
        let held: std::collections::BTreeMap<u16, &crate::mc2::stagevars::Mc2Held> =
            self.mc2_sv_held.iter().map(|h| (h.ent, h)).collect();
        let mut entities: Vec<EntObsMc2> = Vec::new();
        for slot in 1..self.g.ent.len() as u16 {
            if slot == pin.slot {
                entities.push(self.synth_carpet_obs_mc2(pin));
                continue;
            }
            let e = &self.g.ent[slot as usize];
            if e.class64 == 0 {
                continue;
            }
            let mut row = EntObsMc2 {
                slot,
                class: e.class64,
                model: e.model65,
                life: e.act_life,
                max_life: e.max_life as i32,
                x: e.x as f64 / 256.0,
                y: e.y as f64 / 256.0,
                z: e.z,
                heading: e.f30 as i16,
                pitch: e.f32 as i16,
                applied_yaw: e.f78 as i16,
                applied_pitch: e.f80 as i16,
                speed: e.f126,
                mana: e.f140,
                mana_max: e.f136,
                // Retail's parentId @0x28 (the recorded `owner` lane) is
                // live on FOUR families on this corpus — the old
                // "class-15 only" premise is REFUTED (mc2l24 whole-file
                // owner census: 47k+ rows). Each is recovered per family:
                //   • class-15 manifestations — parentId = wizard, fused
                //     into id24 (@0x28 != 0 branch); `id24 != slot`
                //     excludes a detached manifestation (projects 0).
                //   • (5,10) DOOMSDAY PYRAMID — @0x28 is REPURPOSED as
                //     the (10,14) rock-ring spin angle (`f36` port-side,
                //     +96 & 0x7FF per un-suppressed tick), from f36.
                //   • (10,42) build painter — parentId = the owning
                //     castle entity (fixture t=10062 slot 162: @0x28=426
                //     = the (3,2) castle slot; a wizard-owned variant
                //     stamps 116). No wild (10,42) exists, so the fused
                //     id24 = tr(@0x28) recovers it directly.
                //   • (5,{0,19,21,25}) pyramid-summoned creatures — the
                //     apocalypse summon (EF:13420) stamps parentId = the
                //     pyramid (entity 7 = the (5,10) here) into both @0x28
                //     and @0x1A, so id24 = tr(7). CAUTION: model 0 is ALSO
                //     the generic worm / multipart body, whose id24 points
                //     at its BODY slot, not a parent (261k wild rows if
                //     read blindly). The discriminator that survives both
                //     import AND the native summon (`own_id = pyramid.id24`
                //     = 7, doomsday.rs, once the importer stops fusing the
                //     pyramid's spin-angle @0x28 into its id24) is: the
                //     referenced entity IS a live (5,10) pyramid. A wild
                //     body points at a (5,0)/(5,27) segment → projects 0.
                owner: if e.class64 == 5 && e.model65 == 10 {
                    e.f36
                } else {
                    // The three translated-owner lanes (class-15
                    // manifestation → wizard, (10,42) painter →
                    // parent castle, live-pyramid summon → pyramid)
                    // all project the same way; everything else 0.
                    let translated = (e.class64 == 15 && e.id24 != slot)
                        || (e.class64 == 10 && e.model65 == 42 && e.id24 != slot)
                        || (e.class64 == 5
                            && matches!(e.model65, 0 | 19 | 21 | 25)
                            && self
                                .g
                                .ent
                                .get(untr(e.id24) as usize)
                                .is_some_and(|p| p.class64 == 5 && p.model65 == 10));
                    if translated { untr(e.id24) } else { 0 }
                },
                action: e.tick70,
                sv1: held.get(&slot).map_or(0, |h| h.slot),
                sv2: if e.class64 == 5 { e.site_z as u8 } else { 0 },
                player_ent_idx: untr(e.f144),
                rand: e.rand as u16,
            };
            // Class-15 reverse map (`import_ent_mc2`'s override): the
            // obs heading lane (@0x1C) and max_life lane (@0x04) are
            // dead 0 on retail manifestations — f30 carries the
            // payload and max_life the cast cost, which retail keeps
            // in the obs mana_max lane (@0x8C).
            if e.class64 == 15 {
                row.heading = 0;
                row.max_life = 0;
                row.mana_max = e.max_life as i32;
            }
            // Class-10 fires carry their amount in f140 (imported
            // from @0x2A); retail's @0x90 mana lane is dead 0.
            if e.class64 == 10 && matches!(e.model65, 0 | 6) {
                row.mana = 0;
            }
            // The m27 HYDRA keeps its bolt power (@0x88) in f136
            // (import_ent_mc2's `m27` arm); retail's @0x8C mana_max
            // lane is dead 0 across the whole family (mc2l24 census,
            // 87,210 rows), so the obs lane re-zeroes rather than
            // reporting the power.
            if e.class64 == 5 && e.model65 == 27 {
                row.mana_max = 0;
            }
            // The (10,79) castle defender piece keeps its world-yaw
            // (@0x1C, the obs heading lane) in f34 — the piece brain's
            // firing-yaw home (import_ent_mc2's (10,79) block,
            // mc2_castle_piece_tick) — not the uniform f30, which now
            // holds the @0x2C fire-mode selector. (Pitch stays on the
            // uniform f32=@0x1E copy: the piece's live @0x1E lives in
            // f36 but projecting it there only trades the static-copy
            // capture residual for the firing-elevation one, both
            // terrain-closure, so leave f32.)
            if e.class64 == 10 && e.model65 == 79 {
                row.heading = e.f34 as i16;
            }
            entities.push(row);
        }
        let spell_i16 = |s: Option<SpellId>| s.map(|s| s.0 as i16);
        let players: Vec<PlayerMc2> = (0..pin.player_count)
            .map(|i| {
                let localp = i == pin.local;
                PlayerMc2 {
                    index: i,
                    is_ai: !localp,
                    play_index: if localp {
                        pin.slot
                    } else {
                        untr(self.g.rival_ents[i as usize])
                    },
                    turn: 0,
                    name: String::new(),
                    // Echoed, not derived — see [`PinnedMc2::castles`].
                    castle: pin.castles[i as usize & 7],
                    hand_left: if localp {
                        spell_i16(self.player.left)
                    } else {
                        None
                    },
                    hand_right: if localp {
                        spell_i16(self.player.right)
                    } else {
                        None
                    },
                    flight: FlightMc2 {
                        cmd_speed: if localp { pin.pose.speed } else { 0 },
                        v16: 0,
                    },
                }
            })
            .collect();
        let control: Vec<ControlMc2> = (0..pin.player_count).map(zero_control_mc2).collect();
        let player = players.get(pin.local as usize).map(|p| PlayerJoinMc2 {
            carpet_slot: pin.slot,
            name: String::new(),
            is_ai: false,
            turn: 0,
            life: self.player.life,
            max_life: PLAYER_LIFE_MAX,
            mana: self.player.mana as i32,
            mana_max: self.player.mana_max as i32,
            x: pin.pose.x as f64 / 256.0,
            y: pin.pose.y as f64 / 256.0,
            z: pin.pose.z,
            heading: pin.pose.heading as i16,
            pitch: pin.pose.pitch as i16,
            applied_yaw: 0,
            applied_pitch: 0,
            speed: pin.pose.speed,
            hand_left: p.hand_left,
            hand_right: p.hand_right,
            castle: p.castle,
            flight: p.flight.clone(),
            control: Some(zero_control_mc2(pin.local)),
        });
        ObsMc2 {
            rng: self.g.rand,
            n_active: entities.len() as u32,
            local_player: pin.local,
            player_count: pin.player_count,
            players,
            control,
            player,
            entities,
        }
    }

    /// The synthesized MC2 human-carpet obs row.
    fn synth_carpet_obs_mc2(&self, pin: &PinnedMc2) -> EntObsMc2 {
        EntObsMc2 {
            slot: pin.slot,
            class: 3,
            model: 0,
            life: self.player.life,
            max_life: PLAYER_LIFE_MAX,
            x: pin.pose.x as f64 / 256.0,
            y: pin.pose.y as f64 / 256.0,
            z: pin.pose.z,
            heading: pin.pose.heading as i16,
            pitch: pin.pose.pitch as i16,
            applied_yaw: 0,
            applied_pitch: 0,
            speed: pin.pose.speed,
            mana: self.player.mana as i32,
            mana_max: self.player.mana_max as i32,
            owner: pin.slot,
            action: 0,
            sv1: 0,
            sv2: 0,
            player_ent_idx: pin.slot,
            rand: 0,
        }
    }

    /// The synthesized human-carpet obs row: pose fields from the pin,
    /// life/mana from the player column. `flags`/`rand`/`tick_byte`
    /// have no port-side counterpart outside the pool — the comparator
    /// treats the pinned slot specially.
    fn synth_carpet_obs(&self, pin: &PinnedMc1) -> EntObsMc1 {
        EntObsMc1 {
            slot: pin.slot,
            class: 3,
            model: 0,
            sclass: match self.player.state {
                LifeState::Alive => 0,
                LifeState::Falling => 2,
                LifeState::Dead => 3,
            },
            smodel: 0,
            flags: 0,
            id: pin.slot,
            life: self.player.life,
            max_life: PLAYER_LIFE_MAX as u32,
            x: pin.pose.x as f64 / 256.0,
            y: pin.pose.y as f64 / 256.0,
            z: pin.pose.z,
            heading: pin.pose.heading,
            pitch: pin.pose.pitch,
            target_yaw: pin.pose.heading,
            speed: pin.pose.speed,
            mana: self.player.mana,
            mana_max: self.player.mana_max,
            chase: 0,
            owner_ptr: 0,
            tick_byte: 0,
            rand: 0,
        }
    }
}

fn zero_control_mc2(player: u16) -> ControlMc2 {
    ControlMc2 {
        player,
        opcode: 0,
        param1: 0,
        param2: 0,
        aim_yaw: 0,
        aim_pitch: 0,
        buttons: 0,
    }
}

/// **THE RECORDED `manaRegen` IS NOT ALWAYS THE ONE THAT GETS
/// APPLIED — THE MANIFESTATION'S POOL SLOT DECIDES.**
///
/// Both engines run the wizard's mana as an applied-then-recomputed
/// pipeline: `AddPlayer03_00_5E010` does `mana += manaRegen` and then
/// recomputes `manaRegen` to the regen floor (EF:59996-60033). The
/// CAST machinery writes the same word from a different place —
/// `sub_68DE0` (EF:55569), run from the manifestation's OWN class-15
/// action:
///
/// ```text
///   word_0x2E_46 == word_0x30_48  (the burst's FIRST tick)
///       caster.manaRegen  = -maxMana_0x8C (or -= it, accumulating)
///   else if word_0x2E_46 != 0     (mid-burst)
///       if caster.manaRegen > 0 { caster.manaRegen = 0 }   // the PIN
/// ```
///
/// Both run inside the SAME ascending entity walk, so the
/// manifestation's slot against the carpet's decides which of the two
/// writes the recorder's frame-tail snapshot catches:
///
/// - **token ABOVE the carpet** — the wizard applies, recomputes, then
///   the token overwrites: the record holds the token's stamp and
///   applying it next frame is exactly right. (mc2l24 slot 118 vs
///   carpet 116: `d88` −100 with mana flat, then mana −100 with `d88`
///   0, then flat again.)
/// - **token BELOW the carpet** — the token stamps FIRST and the
///   wizard applies it and then recomputes: the record holds the
///   RECOMPUTED floor (100/1000), and what the next frame applies is
///   whatever the token stamps then. Seeding the recorded value made
///   the port hand the wizard a full regen quantum on every casting
///   tick — the `player.mana` family, 3,710 pairs of `want + 100` on
///   mc2l3 take-2 — and, on a first tick, miss the debit outright
///   (t=8445: the Create Castle cast, want 1359 got 41359, the whole
///   40,000).
///
/// So: start from the recorded word (which IS what `manaRegen` holds
/// when the next frame opens) and replay `sub_68DE0` for every human
/// manifestation the walk reaches BEFORE the carpet, in slot order.
///
/// The CASTLE (spell 2) is the one exception, and retail's own
/// dispatch is why: its timer is an upgrade LOCK, not a countdown, so
/// its body only reaches `sub_68DE0` on the fresh-cast sentinel and
/// never pins the regen while the tower transforms (mc2l3 t=8446+:
/// `word_0x2E_46` parked at 100 while mana climbs +1000/tick).
fn mc2_applied_mana_delta(
    st: &RetailMc2,
    ply: &mgc_formats::mgcr::RetailPlayerMc2,
    human_slot: u16,
    carpet: &RetailEntMc2,
) -> i32 {
    let recorded = carpet.d88;
    // `MGC_NO_MC2_BURST_DELTA=1` — the A/B lane (both halves; see
    // `world::mc2_burst_delta_off`): seed the recorded word verbatim,
    // i.e. the pre-dig import.
    if super::mc2_burst_delta_off() {
        return recorded;
    }
    // The apply lives in the action-0 body alone. Actions 2/3 (the
    // death fall and the corpse) are the port's `LifeState`, which
    // gates the step directly — and their HELD delta is the reset's
    // 750/2000 residue law, so it must survive the import untouched.
    // Every OTHER body simply never reaches the regen block: action
    // 12, the level-end sequence (`sub_5E8C0_endGameSeq` EF:60336),
    // freezes mana for its whole run — 176 of mc2l3 take-2's 177
    // action-12 ticks apply 0 against a recorded 100, including the
    // take's last rng-mismatched pair (t=22621).
    if !matches!(carpet.action45, 0 | 2 | 3) {
        return 0;
    }
    let mut delta = recorded;
    // Ascending slot order — the walk's, and the accumulate branch
    // above makes two first-ticks in one frame order-dependent.
    let mut below: Vec<(u16, usize)> = (0..26usize)
        .map(|s| (ply.spell_ent[s], s))
        .filter(|&(m, _)| m != 0 && m < human_slot && (m as usize) < st.ents.len())
        .collect();
    below.sort_unstable();
    for (m, spell) in below {
        let e = &st.ents[m as usize];
        // The book entry must still BE that manifestation in its
        // owned action state (3M): the death scatter parks a boolean
        // 1 marker in the book (`sub_5E310` EF:60146) and a
        // wraith-stolen jar runs action 78 — neither reaches
        // `sub_68DE0`.
        if e.class3f != 15 || e.model40 as usize != spell || e.action45 as usize != spell * 3 {
            continue;
        }
        if e.f2e == 0 {
            continue;
        }
        if e.f2e as i32 == e.f30 as i32 {
            // FIRST tick: retail's `manaRegen = -maxMana_0x8C` wipes
            // the recompute outright. The DEBIT itself is not seeded
            // here — the port's own manifestation pass stamps it and
            // lands it in the same tick
            // (`World::mc2_same_frame_debit`), which keeps retail's
            // ordering intact: the afford gate reads the purse BEFORE
            // the debit, exactly as it does at the token's own slot.
            delta = 0;
        } else if spell != 2 && delta > 0 {
            // The mid-burst PIN. Spell 2 is exempt: the castle's
            // timer is an upgrade LOCK, so its body never reaches
            // `sub_68DE0` again and the regen runs on (mc2l3 t=8446+:
            // the timer parks at 100 while mana climbs +1000/tick).
            delta = 0;
        }
    }
    delta
}

/// One retail MC2 pool record → the port's `Ent`, per the SEMANTIC
/// alias table (mc2/mobs.rs doc header) — MC2 offsets do NOT line up
/// with the port's MC1-numbered field names. Entity-reference fields
/// go through the human-slot translation; the link bit (byte[0] & 4)
/// is cleared for the caller's relink pass.
///
/// Flag translation covers the bits the port reads (mobs.rs):
/// byte0&8 collidable and byte0&4 link keep their positions;
/// byte0&0x20 invisible → 0x20; byte0&2 whoosh-played → bit 25;
/// byte1&4 disabled → 0x400 (reap); byte1&8 forced-stop → bit 26;
/// byte2&4 blocked → bit 27; byte2&0x10 no-corpse → bit 28;
/// byte2&0x20 forced-claim → bit 29. Unmapped retail bits drop (the
/// obs channel does not carry flags; only behavior reads them).
fn import_ent_mc2(r: &RetailEntMc2, slot: u16, row156: u8, tr: &dyn Fn(u16) -> u16) -> Ent {
    let (b0, b1, b2) = (
        r.flags & 0xFF,
        (r.flags >> 8) & 0xFF,
        (r.flags >> 16) & 0xFF,
    );
    let mut flags = 0u32;
    if b0 & 8 != 0 {
        flags |= 8;
    }
    if b0 & 0x20 != 0 {
        flags |= 0x20;
    }
    if b0 & 2 != 0 {
        // Retail's byte0&2 is the generic one-shot-done latch. The
        // port keeps it POSITIONAL (bit 1 — the fire/explosion
        // activation gates) and ALSO mirrors it to bit 25 (the
        // whoosh-played home). Importing only the mirror re-ran
        // every active fire's activation block (area damage +
        // flicker draw + scorch) on each pair.
        flags |= (1 << 25) | 2;
    }
    if b1 & 4 != 0 {
        flags |= 0x400;
    }
    if b1 & 8 != 0 {
        flags |= 1 << 26;
    }
    if b2 & 1 != 0 {
        // byte[2] bit 0 = the NO-CH0-BROADCAST stamp, and the port
        // already homes it POSITIONALLY at 0x1_0000 — the (10,0)
        // fire's `if (!(byte[2] & 1)) sub_10C80(...)` (EF:22719) and
        // its two siblings in mc2::effects read exactly that bit.
        // The importer never filled it, so every DECORATIVE imported
        // fire — the 0x10080-stamped light-show kind, the same family
        // the mc1l5 wall-of-fire dig pinned — broadcast full damage
        // in the port. Invisible while buildings were only reachable
        // at their anchor; the footprint pass surfaced it as 41 fires
        // × 400 landing on one mc2l24 village house in a single tick.
        flags |= 0x1_0000;
    }
    if b2 & 4 != 0 {
        flags |= 1 << 27;
    }
    if b2 & 0x10 != 0 {
        flags |= 1 << 28;
    }
    if b2 & 0x20 != 0 {
        flags |= 1 << 29;
    }
    // The port routes MC2-native projectiles by the F_MC2PROJ marker
    // its ctors set (bit 29, with the collidable bit cleared —
    // mc2/proj.rs); retail has no such marker, so stamp every class-9
    // projectile except the (9,13) arrow (state-keyed, no marker).
    // Without it an imported projectile falls into the MC1 fallback
    // arm and indexes MC1's 31-row table with an MC2 row.
    if r.class3f == 9 && r.model40 != 13 {
        flags = (flags & !8) | crate::mc2::proj::F_MC2PROJ;
    }
    // The m27 HYDRA reuses three struct words the uniform MC2 map
    // spends elsewhere (the branch machine's own field homes,
    // docs/traces/mc2-m27-branch-machine.md): the spline pitch angle
    // `fov_0x22_34` → f36, the speed-mode selector `word_0x2C_44` →
    // f44 (NOT the projectile column's `subSpellIndex_0x2A_42`), and
    // the branch index / body live-branch gauge `byte_0x3B_59` → f50
    // (NOT the uniform @0x30 lane). Importing the uniform homes froze
    // the whole hydra: every branch head collapsed onto one z, the
    // integrator hit its no-op arm (roll/fov/speed never advanced),
    // and all five branches read D404C[0] with the body gauge at 0.
    let m27 = r.class3f == 5 && r.model40 == 27;
    // The m23 DWELLER is the second such model: its whole machine
    // runs on `word_0x2C_44` — the cruise altitude 0x2000 (ctor
    // EF:34474, servo :18081-86) and then the SIPHON RISE STEP the
    // grabbed sphere reads off it (:18238 seeds 18, :18270 ramps +10,
    // TransformArcherToMana EF:26120 consumes it). Its
    // `subSpellIndex_0x2A_42` (500) has no reader on our side — the
    // (9,9) bolt launcher stamps its own payload (mc2/proj.rs
    // `mc2_atk_heavy9`). Importing the uniform @0x2A home made every
    // imported dweller lift its sphere by a flat 500/tick instead of
    // the ramp (mc2l24 t=14519-14523: retail +98/+108/+118/+128).
    let ramp2c = m27 || (r.class3f == 5 && r.model40 == 23);
    let mut e = Ent {
        rand: r.rand as u32,
        max_life: r.max_life.max(0) as u32,
        act_life: r.life,
        flags,
        next20: 0,
        prev22: 0,
        // The port fuses retail's own-id (`id_0x1A`) and
        // `parentId_0x28` into id24. @0x1A is the LIVE owner-or-self
        // lane (mc2l0 census): the caster on projectiles, the owning
        // wizard on castles/balloons/charmed creatures, the watch
        // target on class-11 triggers, self everywhere else. @0x28 is a
        // live parentId on class-15 manifestations, (10,42) painters,
        // and the pyramid-summoned (5,{0,19,21,25}) creatures — all
        // recovered by `obs_project_mc2` from this fused lane.
        // EXCEPTION: the (5,10) DOOMSDAY PYRAMID repurposes @0x28 as its
        // ring-spin angle (imported to f36), NOT a parent — fusing it
        // here stamped a garbage id24 (= the spin angle) that the
        // apocalypse summon then copied onto every child creature
        // (`own_id = pyramid.id24`, doomsday.rs), so their `owner` obs
        // read the spin angle instead of the pyramid id. Take @0x1A (the
        // pyramid's own id) for it, matching the retail summon which
        // stamps the child's parentId = the pyramid entity index.
        id24: if r.owner28 != 0 && !(r.class3f == 5 && r.model40 == 10) {
            tr(r.owner28)
        } else if r.f1a != 0 {
            tr(r.f1a)
        } else {
            slot
        },
        // The scratch quartet is DUAL-HOMED per class (mc2/ handler
        // survey): creatures keep the charm/armed timer (@0x2E) in
        // f26 and the font-type byte (@0x3D) in f46; effects keep
        // dword @0x10 scratch in f26 and the z-velocity (@0x2E) in
        // f46. f28 is a port artifact (the cross-column damage
        // contract; retail's @0x38 mask is write-only in MC2). m27's
        // link length (@0x36) rides f56; everything else keeps @0x38
        // there. Class-15 manifestations override eight of these
        // below (the cast.rs field map).
        f26: match (r.class3f, r.model40) {
            // The m0 worm/hydra keeps its BOB VELOCITY in @0x10
            // (multipart ctor seed + sub_1F040's home); importing
            // the charm lane left the bob dead — the whole chain
            // sank instead of undulating (mc2l4 corpus, slot 2). The
            // m27 hydra shares the @0x10 home: the body's wander/
            // emerge phase seed AND the branch machine's whip counter
            // (sub_2A340 mode-3/4 reads it — mc2l24 t=180 slot 46:
            // @0x10 steps 1→2→3→4 in lockstep with the crack speeds
            // -192/-130/-23/192; the @0x2E charm lane stays 0 and
            // parked the port one step behind). EXCEPT in the pyramid's
            // release chain: a StageVar2 16/17 summon (mc2::doomsday's
            // spawn block, EF:13419) has `word_0x2E_46` as its LIFE
            // LATCH, and mobs.rs's `mc2_doom_summon_*` expire it the
            // moment it reads <= 0 — an m0 worm summon imported with the
            // bob velocity there puffs itself on the first replayed
            // tick. The latch wins for exactly those two slots.
            (5, 0 | 27) if !matches!(r.sv2, 16 | 17) => r.scratch10 as i16,
            // The (5,10) DOOMSDAY PYRAMID drives its whole 16-state
            // machine off `dword_0x10_16` (@0x10 = scratch10): the
            // per-state countdown AND the 0..1200 doom-meter ramp
            // (`sub_21030`/`sub_21490`). Importing the @0x2E charm lane
            // (0) reset the doom-meter to 0 every pair, so it re-ramped
            // to only 30 and NEVER crossed the 600 gate that suppresses
            // the (10,14) rock ring — the port then spawned 4 rocks/tick
            // (each a global-LCG draw) while retail, suppressed, drew
            // none: the got[t]==want[t+4] rng window t=51751-70 (mc2l24;
            // retail `owner`/parentId spin freezes at 192 there, the
            // suppression tell) plus the epoch's isolated (1,5) pairs.
            (5, 10) => r.scratch10 as i16,
            (5, _) => r.f2e,
            _ => r.scratch10 as i16,
        },
        f28: r.b38 as u8 as u16,
        f30: r.yaw as u16,
        f32: r.pitch as u16,
        f34: r.roll as u16,
        // The (5,10) pyramid keeps its ring-spin angle in
        // `parentId_0x28` (@0x28 = owner28; the RENDERER-arm exception
        // to "@0x28 is class-15 only"). The ring driver steps it
        // `+96 & 0x7FF` per un-suppressed tick (EF:13072), so it must
        // be RESTORED each pair — importing 0 both mis-angled the
        // (10,14) rock ring and left the `owner` obs (which captures
        // @0x28) reading retail's spin vs the port's 0 on every active
        // tick.
        f36: if m27 {
            r.f22 as u16
        } else if r.class3f == 5 && r.model40 == 10 {
            r.owner28 as u16
        } else {
            0
        },
        f38: tr(r.f24 as u16),
        f40: tr(r.f26 as u16),
        f44: if ramp2c { r.f2c as u16 } else { r.f2a },
        // The (10,45) BUILDING keeps its DEGRADATION LINK in
        // `fontTypeIndex_0x3D_61` (@0x3D, the same home the class-5
        // column already imports) — seeded from `bldgprm[type].byte_3`
        // by `sub_49A30` (EF:32795-98) and ZEROED in place by the
        // castle level-up pre-clear / the quake grab, which is the
        // whole point of it being per-entity. @0x2E is dead for
        // buildings on both sides, so a replayed building used to
        // import link 0 and demolish where retail rebuilds its
        // successor ([`Gen::mc2_spawn_building`]).
        f46: if r.class3f == 5 || (r.class3f == 10 && r.model40 == 45) {
            r.b3d as i16
        } else {
            r.f2e
        },
        // The (5,10) DOOMSDAY PYRAMID keeps its SUMMON-RING STRIDE in
        // `word_0x4A_74` (@0x4A = sv_timer): `sub_21850` stamps
        // 682 (creatures) / 256 (the m19 swarm) with the pick
        // (EF:13160/13173/13186/13199) and `sub_21AB0` fans the ring at
        // `stride * repeat + yaw` (EF:13364). @0x30 is dead for the
        // pyramid, so the uniform import parked the stride at 0 and
        // every replayed summon spawned stacked on the pyramid's own
        // bearing instead of fanning (mc2l24 t=53808: retail x 7616 vs
        // port 7936). The pyramid is never a StageVar hold (sv1 = 0),
        // so @0x4A is free for it.
        f50: if m27 {
            r.b3b as i16
        } else if r.class3f == 5 && r.model40 == 10 {
            r.sv_timer
        } else {
            r.f30 as i16
        },
        f52: tr(r.f32),
        f54: tr(r.f34),
        f56: if matches!(r.class3f, 2 | 10) {
            r.b38 as u8 as u16
        } else {
            r.f36
        },
        f58: r.b39 as i16,
        // The (3,2) castle's BUILD SUB-STATE lives in @0x2E
        // (word_0x2E_46 → f59, docs/traces/mc2-castle-builder.md §2);
        // @0x3A is dead for castles, and importing its 0 parked every
        // castle in the level-up state — one phantom upgrade + one
        // phantom (10,42) painter per pair, z frozen for the tick
        // (the MC2 twin of MC1's phantom-upgrade family).
        f59: if r.class3f == 3 && r.model40 == 2 {
            r.f2e as u8
        } else {
            r.b3a as u8
        },
        f63: r.phase3e,
        class64: r.class3f,
        model65: r.model40,
        f66: r.b41 as u8,
        f67: r.b42 as u8,
        f68: r.b43 as u8,
        f69: r.b44 as u8,
        tick70: r.action45,
        f71: r.b46 as u8,
        x: r.x,
        y: r.y,
        z: r.z,
        f78: r.ayaw as u16,
        f80: r.apitch as u16,
        f82: r.aroll as u16,
        f84: r.afov as u16,
        type86: r.f5a as u16,
        frame88: r.b5c as u8,
        frames89: r.b5d as u8,
        mail: r.mail.map(|(a, s)| (a.max(0) as u32, tr(s))),
        f126: r.speed,
        f128: r.min_speed,
        f130: r.max_speed,
        // The m27 HYDRA's BOLT POWER is `manaRegen_0x88_136` (@0x88 —
        // `sub_2A7F0` EF:20513-16 rolls it `(rand%12 > 7) + 1` on the
        // a3=1 shot and every a3=0 RE-FIRE reads it back, EF:20518-40),
        // and the port's `m27_branch_bolt` keeps it in f136. The
        // uniform map spends f136 on @0x8C, so every pair re-imported
        // the branch's power as 0 and the four re-fires of each whip
        // hit the `_ => return` arm: one arc per whip instead of five.
        // Retail's @0x8C is DEAD 0 on the whole (5,27) family (mc2l24
        // census, 87,210 rows: @0x8C 0×87,210; @0x88 0/1/2), so the
        // lane is free — the obs `mana_max` projection re-zeroes it.
        f136: if m27 { r.d88 } else { r.mana_max },
        f140: r.mana,
        f144: tr(r.player_ent),
        f146: tr(r.target96),
        row156,
        thing_slot: 0,
        dest_x: r.dest_x,
        dest_y: r.dest_y,
        // Creatures keep the StageVar KIND in the port's site_z (the
        // relocated `StageVar2_0x49_73`); other classes carry the
        // destination z there.
        site_z: if r.class3f == 5 {
            r.sv2 as i16
        } else {
            r.dest_z
        },
    };
    // Class-15 manifestations keep the cast machinery in different
    // homes than the uniform alias table (cast.rs module doc):
    // armed timer @0x2E → f26, duration/mana divisor @0x30 → f28,
    // sub-spell payload @0x2A → f30 (the yaw lane is dead 0),
    // pending tier+1 @0x2C → f44, cooldown @0x36 → f54, cadence
    // flag @0x3B → f59, upkeep regen @0x88 → f136, full cast cost
    // @0x8C → max_life (the @0x04 lane is dead 0). @0x90 per-tick
    // mana → f140 and @0x46 tier → f71 coincide with the uniform
    // map. The displaced uniform homes are dead for class 15.
    if r.class3f == 15 {
        e.f26 = r.f2e;
        e.f28 = r.f30;
        e.f30 = r.f2a;
        e.f44 = r.f2c as u16;
        e.f54 = r.f36;
        e.f59 = r.b3b as u8;
        e.f136 = r.d88;
        e.max_life = r.mana_max.max(0) as u32;
        e.f46 = 0;
        e.f50 = 0;
        e.f56 = 0;
        // The DETACHED spell-jar (action 78) — the m26-wraith steal's
        // fling/homing arc `sub_59DC0` (EF:41198-41243) — abandons the
        // dormant-manifestation homes above. Its arc runs off DIFFERENT
        // fields: the arc counter `dword_0x10_16` (@0x10 = scratch10,
        // steps 0..5 rising then homing) → f26, and the wraith slot
        // `word_0x26_38` (@0x26) → f38 (`Entities[word_0x26_38]` is the
        // homing target, EF:41224). `sub_69300` (EF:55807) zeroes @0x10
        // at the steal; the parent (@0x28 = the caster/player) drives the
        // rising leg. Without these homes `mc2_stolen_arc` read the armed
        // timer as the counter (n≫5 → straight to the homing branch),
        // found no wraith in f38, and dropped the jar in place with
        // action 3M+1 on frame 1 (mc2l24 slot 73 t=15080-95: action
        // 78→1, the arc frozen a tick behind retail).
        if r.action45 == 78 {
            e.f26 = r.scratch10 as i16;
            e.f38 = tr(r.f26 as u16);
        }
    }
    // Class-10 fires keep the area amount in `subSpellIndex_0x2A`
    // (→ the port's f140 amount home, sub_30D50's sub_10C80 call /
    // sub_31760) and the z flicker/lift in `word_0x2C_44` (→ f44);
    // the @0x90 mana lane is dead 0 on them (reverse-mapped in
    // `obs_project_mc2`).
    if r.class3f == 10 && matches!(r.model40, 0 | 6) {
        e.f140 = r.f2a as i32;
        e.f44 = r.f2c as u16;
    }
    // The (10,16) volcano boulder keeps its VERTICAL VELOCITY in
    // `word_0x2C_44` (`sub_32600` EF:23765 reads it as vz, gravity
    // −28 clamp [−384,256]) — the port's `mc2_boulder16_tick` vz lane
    // is f44. The uniform map homes f44 ← `subSpellIndex_0x2A` (=200
    // on every boulder), so an imported boulder re-launched at vz=200
    // each pair: pz = z + 200 (mc2l24 (10,16) z = retail + 200 —
    // resting summit boulders 173/329/447/574/626 and mid-roll
    // 428/449/623). The tick never reads f140, so leaving f140 ← mana
    // is inert; only f44 matters.
    if r.class3f == 10 && r.model40 == 16 {
        e.f44 = r.f2c as u16;
    }
    // The (10,39)/(10,57) mana sphere keeps its z-velocity in
    // `word_0x2C_44` (TransformArcherToMana EF:26188-91; the uniform
    // @0x2E home is dead on spheres) — the ball tick's z-vel lane is
    // f46. The uniform flag map also drops two mover latches: byte0
    // & 0x40 = the absorb-chase mode (EF:26111), byte1 & 0x20 = the
    // decay channel (EF:26289 — the port's bit-13 tail). The settle
    // countdown @0x39 already rides the generic f58 ← b39 map.
    if r.class3f == 10 && matches!(r.model40, 39 | 57) {
        e.f46 = r.f2c;
        if b0 & 0x40 != 0 {
            e.flags |= 0x40;
        }
        if b1 & 0x20 != 0 {
            e.flags |= 0x2000;
        }
    }
    // The (10,79) castle DEFENDER PIECE (ctor sub_508E0 EF:36987,
    // tick sub_3AF00 EF:30106) is minted with a FRESH field layout —
    // the piece never carried any prior class's homes, so the uniform
    // alias table mis-reads eleven of them (mc2/castle.rs
    // mc2_castle_piece_tick lists the homes). The killer is
    // recoil f68: the uniform map reads @0x43 (part-type, nonzero) as
    // the recoil step, so every imported piece re-applies a 115-unit
    // (0.449-tile) launch displacement each pair — the whole 335k-row
    // y family. Restore all eleven from their retail offsets (f63 tick
    // counter @0x3E, f71 state @0x46, and the @0x9A/@0x9C/@0x9E home
    // anchor are already uniform-correct):
    //   dwell/windup  f44 ← dword_0x10_16 (scratch10)
    //   fire mode     f30 ← word_0x2C_44  (f2c)
    //   burst count   f69 ← fontTypeIndex_0x3D_61 (b3d)
    //   recoil step   f68 ← byte_0x44_68  (b44)
    //   windup z-boost f54 ← word_0x36_54 (f36)
    //   target slot   f28 ← word_0x96_150 (target96)
    //   firing yaw    f34 ← yaw_0x1C, pitch f36 ← pitch_0x1E
    //   level tag     f26 ← word_0x4A_74  (sv_timer → z height offset)
    //   part-type     f67 ← byte_0x43_67  (b43)
    if r.class3f == 10 && r.model40 == 79 {
        e.f26 = r.sv_timer;
        e.f28 = tr(r.target96);
        e.f30 = r.f2c as u16;
        e.f34 = r.yaw as u16;
        e.f36 = r.pitch as u16;
        e.f44 = r.scratch10 as u16;
        e.f54 = r.f36;
        e.f67 = r.b43 as u8;
        e.f68 = r.b44 as u8;
        e.f69 = r.b3d as u8;
    }
    // Balloon ceiling-walk latch (sub_60D50 EF:61896/61905/61921,
    // byte0 & 1): actSpeed 96 walking / 48 flying, ceiling clamp
    // flying-only. Port bit 0 is overloaded per class, so the import
    // stays (3,3)-scoped (mc2/castle.rs is the sole reader); without
    // it every imported ceiling-walker re-took the flying branch —
    // the mc2l30 (3,3) retail-+48 speed family.
    if r.class3f == 3 && r.model40 == 3 && b0 & 1 != 0 {
        e.flags |= 1;
    }
    e
}

fn zero_control(player: u16) -> ControlMc1 {
    ControlMc1 {
        player,
        opcode: 0,
        param1: 0,
        param2: 0,
        aim_yaw: 0,
        aim_pitch: 0,
        move_fire: 0,
        thrust: false,
        decel: false,
        strafe_left: false,
        strafe_right: false,
        fire_left: false,
        fire_right: false,
    }
}

/// One retail pool record → the port's `Ent`, with human-slot id
/// translation applied to every entity-reference field. The link bit
/// (flags & 4) is cleared — the caller relinks through `Gen::link` so
/// the tile lists stay consistent.
fn import_ent(r: &RetailEntMc1, row156: u8, tr: &dyn Fn(u16) -> u16) -> Ent {
    // The castle (3,2) keeps its macro-state in retail's JOB byte +70
    // (4 = settled, 5 = transforming, 6 = full build — sub_46DB0
    // :55978 / sub_46F10 :56043) with the transform sub-state in +48;
    // the port's `castle_tick` fuses both into f59 (0 = level-up
    // commit, 1/6 = painter/leveler waits, 2/3/5 = finish/repaint/
    // handoff, 4 = settled). Retail's +59 byte is dead for castles —
    // importing it verbatim parked every settled castle in f59 = 0 and
    // re-upgraded it one level per tick (the phantom-upgrade family,
    // docs/CONFORMANCE-FINDINGS.md entry 3). Retail's pure-wait +48
    // values 1 and 4 both land on the port's painter-wait state 1.
    let f59 = if r.class64 == 3 && r.model65 == 2 {
        match r.f70 {
            4 => 4,
            5 => match r.f48 {
                1 | 4 => 1,
                s => (s as u8).min(6),
            },
            6 => 0,
            _ => 4,
        }
    } else {
        r.f59
    };
    Ent {
        rand: r.rand,
        max_life: r.max_life,
        act_life: r.act_life,
        flags: r.flags & !4,
        next20: 0,
        prev22: 0,
        // Class-11 id24 is the trigger's DISPOSITION id, not a slot
        // reference: a dis that numerically equals the human's pool
        // slot must not become PLAYER_TARGET (l32's breadcrumb dis 14
        // vs human slot 14 — the fire resolved dis 65535, whose table
        // rows are the consumed load-sentinel set, and the mass spawn
        // silently vanished; obs untr() masked the id from the diff).
        id24: if r.class64 == 11 { r.id24 } else { tr(r.id24) },
        f38: tr(r.f38),
        f40: tr(r.f40),
        f46: r.f46,
        f50: r.f50,
        f68: r.f68,
        f69: r.f69,
        mail: r.mail.map(|(a, s)| (a, tr(s))),
        // Class-12 tokens: retail +144 is always 0; the token's OWNER
        // wizard carpet slot lives in +42 (the lane the Ent doesn't
        // otherwise model). Stamp it into f144 so the active-token
        // arms (the Accelerate contrail) can resolve a RIVAL owner's
        // pose — corpus-proven: every hw:0 token reads f42 = its
        // wizard's carpet slot, f144 = 0.
        f144: if r.class64 == 12 && r.f144 == 0 {
            tr(r.f42)
        } else {
            tr(r.f144)
        },
        // The port keeps a manifestation's burst/refire counter in f26
        // (retail: +48; retail's +26 is the SPELL LEVEL there).
        f26: if r.class64 == 12 { r.f48 as i16 } else { r.f26 },
        // The castle ground LEVELER's "current" rung lives at retail
        // +48 (sub_28200 :30333-36); the port keeps it in f28.
        // Without the re-home an imported MID-RUN leveler read
        // current 0 and stepped (target − 0)/counter — the mound
        // ROSE runaway where retail translated it down (mc1l0
        // castle-663 transform windows t=1164-1350, +160/tick
        // through 52/1 = +1664 at the window end; the terrain
        // divergence every downstream walker then inherited).
        f28: if r.class64 == 10 && r.model65 == 41 {
            r.f48
        } else {
            r.f28
        },
        f30: r.f30,
        f32: r.f32,
        f44: r.f44,
        f34: r.f34,
        f36: r.f36,
        f52: tr(r.f52),
        f54: tr(r.f54),
        f56: r.f56,
        f58: r.f58 as i16,
        f59,
        f63: r.f63,
        class64: r.class64,
        model65: r.model65,
        f66: r.f66,
        f67: r.f67,
        tick70: r.f70,
        f71: r.f71,
        x: r.x,
        y: r.y,
        z: r.z,
        f78: r.f78,
        f80: r.f80,
        f82: r.f82,
        f84: r.f84,
        type86: r.type86,
        frame88: r.frame88,
        frames89: r.frames89,
        f126: r.f126,
        f128: r.f128,
        f130: r.f130,
        f136: r.f136,
        f140: r.f140,
        f146: tr(r.f146),
        row156,
        thing_slot: 0,
        dest_x: r.dest_x,
        dest_y: r.dest_y,
        site_z: r.site_z,
    }
}

// ------------------------------------------------- replay chain seeding
//
// The pure-input replay consumers (`mgc-conform replay`, the app's
// `--replay`) seed the chained human flight state ONCE from a recorded
// closure and free-run on recovered input. The field maps are the pose
// channel's (docs/CONFORMANCE.md "The pose channel"); they live here so
// both consumers share one seeding law.

/// Seed the chained MC1/HW flight state from the recorded closure at
/// an anchor.
pub fn mc1_state_from_retail(st: &RetailMc1, slot: u16) -> Mc1State {
    let e = &st.ents[slot as usize];
    let w = &st.wizards[st.local_player as usize];
    Mc1State {
        x: e.x,
        y: e.y,
        z: e.z,
        yaw: e.f30 & 0x7FF,
        roll_f: w.roll_acc as i16,
        pitch_f: w.pitch_acc as i16,
        aim_pitch: e.f32 & 0x7FF,
        eff_pitch: w.eff_pitch & 0x7FF,
        act_speed: e.f126,
        tgt_speed: w.cmd_speed,
        strafe: w.strafe,
        tick_ctr: e.f63,
        rand: e.rand,
    }
}

/// The MC2 twin — plus the debuff ladders and water/nudge channels
/// the pose channel gates instead of seeding. `row` is the world's
/// live carpet tuning row ([`World::mc2_carpet_row`]).
pub fn mc2_state_from_retail(st: &RetailMc2, slot: u16, row: Mc2Row) -> (Mc1State, Mc2Ext) {
    let e = &st.ents[slot as usize];
    let p = &st.players[st.local_player as usize];
    (
        Mc1State {
            x: e.x,
            y: e.y,
            z: e.z,
            yaw: e.yaw as u16 & 0x7FF,
            roll_f: p.roll_acc as i16,
            pitch_f: p.pitch_acc as i16,
            aim_pitch: e.pitch as u16 & 0x7FF,
            eff_pitch: p.eff_pitch & 0x7FF,
            act_speed: e.speed,
            tgt_speed: p.cmd_speed,
            strafe: p.strafe,
            tick_ctr: 0,
            rand: 0,
        },
        Mc2Ext {
            move_speed: p.move_speed,
            move_speed_ctr: p.move_speed_ctr,
            mobilize: p.mobilize,
            mobilize_ctr: p.mobilize_ctr,
            add: (0, 0, 0),
            water_ctr: p.water_ctr as u16,
            nudge_latch: p.nudge_latch != 0,
            row,
        },
    )
}

/// The integer carpet as the world-tick pose — the faithful path's
/// pose law: heading/pitch/speed straight off the chained state, no
/// float round-trip.
pub fn integer_pose(s: &Mc1State) -> PlayerPose {
    PlayerPose {
        x: s.x,
        y: s.y,
        z: s.z,
        heading: s.yaw,
        pitch: s.aim_pitch,
        speed: s.act_speed,
    }
}

/// Pose lanes: a chained carpet vs the recorded pose at a graded
/// boundary (the pose channel's lane set). Rows are
/// `(lane, retail, port)`, dirty lanes only.
pub fn pose_lanes_mc1(
    s: &Mc1State,
    e: &RetailEntMc1,
    w: &RetailWizardMc1,
) -> Vec<(&'static str, i64, i64)> {
    let mut rows = Vec::new();
    let mut lane = |name, want: i64, got: i64| {
        if want != got {
            rows.push((name, want, got));
        }
    };
    lane("pose.x", e.x as i64, s.x as i64);
    lane("pose.y", e.y as i64, s.y as i64);
    lane("pose.z", e.z as i64, s.z as i64);
    lane("pose.yaw", (e.f30 & 0x7FF) as i64, s.yaw as i64);
    lane("pose.aim_pitch", (e.f32 & 0x7FF) as i64, s.aim_pitch as i64);
    lane(
        "pose.eff_pitch",
        (w.eff_pitch & 0x7FF) as i64,
        (s.eff_pitch & 0x7FF) as i64,
    );
    lane("pose.act_speed", e.f126 as i64, s.act_speed as i64);
    lane("pose.tgt_speed", w.cmd_speed as i64, s.tgt_speed as i64);
    lane("pose.strafe", w.strafe as i64, s.strafe as i64);
    lane("pose.roll_f", w.roll_acc as i16 as i64, s.roll_f as i64);
    lane("pose.pitch_f", w.pitch_acc as i16 as i64, s.pitch_f as i64);
    lane("pose.tick_ctr", e.f63 as i64, s.tick_ctr as i64);
    lane("pose.rand", e.rand as i64, s.rand as i64);
    rows
}

/// The MC2 lane set. `water_ctr` is deliberately NOT a lane yet: it
/// gates the water-flight sound loop, not the pose. (Grading it is
/// what EXPOSED the +610 u16-vs-int8 decode bug — the fixed byte read
/// makes it a candidate lane once its ++/−− law is verified against a
/// wet stretch.)
pub fn pose_lanes_mc2(
    s: &Mc1State,
    e: &RetailEntMc2,
    p: &RetailPlayerMc2,
) -> Vec<(&'static str, i64, i64)> {
    let mut rows = Vec::new();
    let mut lane = |name, want: i64, got: i64| {
        if want != got {
            rows.push((name, want, got));
        }
    };
    lane("pose.x", e.x as i64, s.x as i64);
    lane("pose.y", e.y as i64, s.y as i64);
    lane("pose.z", e.z as i64, s.z as i64);
    lane("pose.yaw", (e.yaw as u16 & 0x7FF) as i64, s.yaw as i64);
    lane(
        "pose.aim_pitch",
        (e.pitch as u16 & 0x7FF) as i64,
        s.aim_pitch as i64,
    );
    lane(
        "pose.eff_pitch",
        (p.eff_pitch & 0x7FF) as i64,
        (s.eff_pitch & 0x7FF) as i64,
    );
    lane("pose.act_speed", e.speed as i64, s.act_speed as i64);
    lane("pose.tgt_speed", p.cmd_speed as i64, s.tgt_speed as i64);
    lane("pose.strafe", p.strafe as i64, s.strafe as i64);
    lane("pose.roll_f", p.roll_acc as i16 as i64, s.roll_f as i64);
    lane("pose.pitch_f", p.pitch_acc as i16 as i64, s.pitch_f as i64);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The (10,79) castle DEFENDER PIECE (ctor sub_508E0 / tick
    /// sub_3AF00) invents a fresh field layout the uniform alias map
    /// mis-reads on import — most damagingly f68 (recoil) off the
    /// part-type byte @0x43, which re-applies a 115-unit launch
    /// displacement every pair (the mc2l24 335k-row y family). Pin
    /// each home to its retail offset. Distinct sentinels make this
    /// non-vacuous: reverting the import block reads f68←@0x43(=2),
    /// f26←@0x10(=42), f34←@0x20(=0), f69←@0x44(=251 from b44), f28←0,
    /// so each assert below flips.
    #[test]
    fn mc2_castle_piece_import_field_homes() {
        let r = RetailEntMc2 {
            class3f: 10,
            model40: 79,
            scratch10: 42, // @0x10 dwell/windup → f44
            yaw: 300,      // @0x1C firing yaw + obs heading → f34
            pitch: 111,    // @0x1E firing pitch + obs pitch → f36
            roll: 0,       // @0x20 (uniform f34) — kept distinct from yaw
            f2c: 3,        // @0x2C fire mode → f30
            f36: 160,      // @0x36 windup z-boost → f54
            b3d: 6,        // @0x3D burst count → f69
            phase3e: 251,  // @0x3E tick counter → f63 (already uniform)
            b43: 2,        // @0x43 part-type → f67
            b44: -5,       // @0x44 recoil step → f68
            b46: 3,        // @0x46 state → f71 (already uniform)
            sv_timer: 6,   // @0x4A level tag → f26 (z height offset)
            target96: 77,  // @0x96 latched target → f28
            dest_x: 1000,  // @0x9A/@0x9C/@0x9E home anchor → dest/site
            dest_y: 2000,
            dest_z: 1760,
            ..Default::default()
        };
        let e = import_ent_mc2(&r, 619, 79, &|v| v);
        assert_eq!(e.class64, 10);
        assert_eq!(e.model65, 79);
        assert_eq!(e.f44, 42, "dwell @0x10");
        assert_eq!(e.f34, 300, "firing yaw / obs heading @0x1C");
        assert_eq!(e.f36, 111, "firing pitch / obs pitch @0x1E");
        assert_eq!(e.f30, 3, "fire mode @0x2C");
        assert_eq!(e.f54, 160, "windup z-boost @0x36");
        assert_eq!(e.f69, 6, "burst @0x3D");
        assert_eq!(e.f63, 251, "tick counter @0x3E");
        assert_eq!(e.f67, 2, "part-type @0x43");
        assert_eq!(e.f68, (-5i8) as u8, "recoil @0x44 (NOT part-type @0x43)");
        assert_eq!(e.f71, 3, "state @0x46");
        assert_eq!(e.f26, 6, "level tag @0x4A");
        assert_eq!(e.f28, 77, "latched target @0x96");
        assert_eq!(e.dest_x, 1000);
        assert_eq!(e.dest_y, 2000);
        assert_eq!(e.site_z, 1760);
    }

    /// The `owner` obs lane = retail parentId @0x28. The importer must
    /// feed it correctly for the two families that carry a live parent,
    /// and must NOT let the (5,10) pyramid pollute id24 with its
    /// repurposed @0x28 (mc2l24 owner census, 47k rows):
    ///  • (10,42) build painter: @0x28 = the owning castle → fused into
    ///    id24 (the `owner28 != 0` branch) so `obs_project_mc2` recovers
    ///    it directly.
    ///  • (5,0) pyramid-summoned creature: @0x28 = @0x1A = the pyramid
    ///    (entity 7) → id24 = tr(7).
    ///  • (5,10) DOOMSDAY PYRAMID: @0x28 is the (10,14) ring-SPIN ANGLE
    ///    (→ f36), NOT a parent. It must NOT reach id24, or the
    ///    apocalypse summon (`own_id = pyramid.id24`) copies the spin
    ///    angle onto every child; id24 falls through to @0x1A (own id).
    /// Non-vacuous: reverting the (5,10) id24 exclusion makes the last
    /// assert read 288 (the spin angle) instead of 7.
    #[test]
    fn mc2_owner_import_field_homes() {
        let tr = |v: u16| v;
        // (10,42) painter: parent castle @0x28=426, @0x1A=116 (wizard).
        let painter = RetailEntMc2 {
            class3f: 10,
            model40: 42,
            owner28: 426,
            f1a: 116,
            ..Default::default()
        };
        assert_eq!(
            import_ent_mc2(&painter, 162, 0, &tr).id24,
            426,
            "painter id24 = @0x28 castle"
        );
        // (5,0) summoned creature: @0x28 = @0x1A = 7 (the pyramid).
        let summoned = RetailEntMc2 {
            class3f: 5,
            model40: 0,
            owner28: 7,
            f1a: 7,
            ..Default::default()
        };
        assert_eq!(
            import_ent_mc2(&summoned, 917, 0, &tr).id24,
            7,
            "summoned creature id24 = pyramid @0x28"
        );
        // (5,10) pyramid: @0x28=288 (spin angle), @0x1A=7 (own id).
        let pyramid = RetailEntMc2 {
            class3f: 5,
            model40: 10,
            owner28: 288,
            f1a: 7,
            ..Default::default()
        };
        let pe = import_ent_mc2(&pyramid, 7, 0, &tr);
        assert_eq!(
            pe.id24, 7,
            "pyramid id24 = @0x1A own id, NOT the @0x28 spin angle"
        );
        assert_eq!(
            pe.f36, 288,
            "pyramid ring-spin angle still carried in f36 (arm untouched)"
        );
    }

    /// The m27 HYDRA's field homes. The four dig-A words plus the BOLT
    /// POWER `manaRegen_0x88_136` (@0x88 → f136): `sub_2A7F0`
    /// (EF:20513-16) rolls it on the a3=1 shot and the four a3=0
    /// re-fires only read it back, so the uniform f136←@0x8C home
    /// silenced 4/5 of every heavy barrage on replay. @0x8C is dead 0
    /// across the whole (5,27) family (mc2l24 census, 87,210 rows), so
    /// the lane is free. Non-vacuous: the sentinels are all distinct —
    /// reverting the arm reads f136←@0x8C(=999), f36←0, f44←@0x2A(=7),
    /// f50←@0x30(=8), f26←@0x2E(=9).
    #[test]
    fn mc2_m27_import_field_homes() {
        let r = RetailEntMc2 {
            class3f: 5,
            model40: 27,
            scratch10: 4,  // @0x10 whip counter → f26
            f22: 1433,     // @0x22 spline pitch → f36
            f2a: 7,        // @0x2A (uniform f44) — kept distinct
            f2c: 3,        // @0x2C integrate mode → f44
            f2e: 9,        // @0x2E charm lane (uniform f26) — distinct
            f30: 8,        // @0x30 (uniform f50) — distinct
            b3b: 2,        // @0x3B branch index → f50
            d88: 2,        // @0x88 bolt power → f136
            mana_max: 999, // @0x8C — the uniform f136 home, dead on m27
            mana: 20000,   // @0x90 → f140 (the body's carried mana)
            ..Default::default()
        };
        let e = import_ent_mc2(&r, 26, 103, &|v| v);
        assert_eq!(e.f26, 4, "whip counter @0x10");
        assert_eq!(e.f36, 1433, "spline pitch @0x22");
        assert_eq!(e.f44, 3, "integrate mode @0x2C");
        assert_eq!(e.f50, 2, "branch index @0x3B");
        assert_eq!(e.f136, 2, "bolt power @0x88 (NOT the @0x8C lane)");
        assert_eq!(e.f140, 20000, "carried mana @0x90");
    }

    /// End-to-end owner-lane projection: the pyramid-summon
    /// discriminator. `obs_project_mc2` must recover retail parentId
    /// @0x28 for a pyramid-summoned creature (id24 → the (5,10) pyramid)
    /// WITHOUT firing on a WILD worm of the same model 0 — whose id24
    /// points at its multipart BODY, not a parent (the 261k-row
    /// over-projection trap). Also the (10,42) painter (id24 → castle)
    /// and the pyramid's own spin-angle owner (from f36). Non-vacuous:
    /// dropping the "id24 refs a (5,10)" gate makes the wild worm
    /// project its body slot (30), and dropping the (10,42) arm makes
    /// the painter project 0.
    #[test]
    fn mc2_owner_projection_pyramid_gated() {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        // Minimal build assets (mirrors world.rs `tests::assets`): a
        // diamond search grid (needs a ring-0 cell) + flat build tab.
        let mut grid = vec![31u8; 1024];
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (dx, dy) = (x - 15, y - 15);
                let r = dx.max(dy).max(-dx + 1).max(-dy + 1) - 1;
                grid[(y * 32 + x) as usize] = r.clamp(0, 31) as u8;
            }
        }
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|_| {
                let mut e = 0u32.to_le_bytes().to_vec();
                e.extend_from_slice(&[4, 4]);
                e
            })
            .collect();
        let mut dat = Vec::new();
        for _ in 0..4 {
            dat.push(4u8);
            dat.extend_from_slice(&[0x10, 0x10, 0x10, 0x10]);
            dat.push(0);
        }
        let fa = crate::engine::features::FeatureAssets::parse(&grid, &tab, &dat).unwrap();
        let mut w = World::new_for_game(planes, &[], 1, fa, crate::ids::GameId::Mc2);

        let put = |w: &mut World, slot: usize, class: u8, model: u8, id24: u16, f36: u16| {
            let e = &mut w.g.ent[slot];
            *e = Ent::default();
            e.class64 = class;
            e.model65 = model;
            e.id24 = id24;
            e.f36 = f36;
            e.max_life = 100;
            e.act_life = 100;
        };
        put(&mut w, 7, 5, 10, 7, 288); // pyramid: own id in id24, spin in f36
        put(&mut w, 20, 5, 0, 7, 0); // summoned m0 → id24 refs pyramid 7
        put(&mut w, 30, 5, 0, 30, 0); // wild worm body (id24 = self)
        put(&mut w, 31, 5, 0, 30, 0); // wild worm segment → id24 refs a (5,0) body
        put(&mut w, 40, 3, 2, 40, 0); // castle
        put(&mut w, 41, 10, 42, 40, 0); // painter → id24 refs castle 40

        let pin = PinnedMc2 {
            slot: 1,
            local: 0,
            player_count: 1,
            pose: PlayerPose {
                x: 0,
                y: 0,
                z: 0,
                heading: 0,
                pitch: 0,
                speed: 0,
            },
            castles: [0; 8],
        };
        let obs = w.obs_project_mc2(&pin);
        let owner_of = |slot: u16| {
            obs.entities
                .iter()
                .find(|e| e.slot == slot)
                .map(|e| e.owner)
        };
        assert_eq!(
            owner_of(20),
            Some(7),
            "pyramid-summoned m0 owner = the pyramid (id24 refs a (5,10))"
        );
        assert_eq!(
            owner_of(31),
            Some(0),
            "wild worm owner = 0 (id24 refs a (5,0) body, NOT a pyramid)"
        );
        assert_eq!(
            owner_of(30),
            Some(0),
            "wild worm body owner = 0 (id24 = self)"
        );
        assert_eq!(
            owner_of(41),
            Some(40),
            "painter owner = the referenced castle"
        );
        assert_eq!(
            owner_of(7),
            Some(288),
            "pyramid own owner = ring-spin angle from f36"
        );
    }

    /// The (5,10) DOOMSDAY PYRAMID's SUMMON-RING STRIDE lives in
    /// `word_0x4A_74` (@0x4A), not the uniform @0x30 lane: `sub_21850`
    /// stamps 682 with every creature pick (EF:13160/13173/13186) and
    /// `sub_21AB0` fans the ring at `stride * repeat + yaw`
    /// (EF:13364). Non-vacuous: the two sentinels differ, so the
    /// uniform import reads f50 = 3 and every replayed summon stacks
    /// on the pyramid's own bearing.
    #[test]
    fn mc2_pyramid_import_keeps_the_summon_stride() {
        let pyr = RetailEntMc2 {
            class3f: 5,
            model40: 10,
            f30: 3,        // @0x30 — dead for the pyramid
            sv_timer: 682, // @0x4A — the summon stride
            ..Default::default()
        };
        assert_eq!(
            import_ent_mc2(&pyr, 7, 107, &|v| v).f50,
            682,
            "summon stride @0x4A (NOT @0x30)"
        );
        let worm = RetailEntMc2 {
            class3f: 5,
            model40: 0,
            f30: 3,
            sv_timer: 682,
            ..Default::default()
        };
        assert_eq!(
            import_ent_mc2(&worm, 8, 71, &|v| v).f50,
            3,
            "every other creature keeps the uniform @0x30 home"
        );
    }

    /// The import must NOT push a GHOST slot onto the free stack. The
    /// recorded stack is retail's PRE-reap image; `tick()`'s
    /// strict-MC2 top pass (UpdateEntities EF:39948-56 → `sub_57F20`,
    /// which class-zeroes and pushes) is the ONE pusher. Pushing here
    /// too double-listed every ghost, so any spawn burst deeper than
    /// the ghost count re-`NewEvent`ed a slot it had just filled:
    /// mc2l24 pair 53808 lost the doomsday pyramid's whole 17-record
    /// worm chain that way — the free list popped
    /// [905, 837, 813, 796, 727, 690] TWICE and the second pop of 905
    /// reset the chain's own live HEAD to `Ent::default()` (class 0 =
    /// invisible to the projection).
    /// Non-vacuous: restoring the `free.extend(ghost_slots)` appends
    /// slot 3 and both asserts below fail.
    #[test]
    fn mc2_import_leaves_the_ghost_free_push_to_the_tick_reap() {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let mut grid = vec![31u8; 1024];
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (dx, dy) = (x - 15, y - 15);
                let r = dx.max(dy).max(-dx + 1).max(-dy + 1) - 1;
                grid[(y * 32 + x) as usize] = r.clamp(0, 31) as u8;
            }
        }
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|_| {
                let mut e = 0u32.to_le_bytes().to_vec();
                e.extend_from_slice(&[4, 4]);
                e
            })
            .collect();
        let mut dat = Vec::new();
        for _ in 0..4 {
            dat.push(4u8);
            dat.extend_from_slice(&[0x10, 0x10, 0x10, 0x10]);
            dat.push(0);
        }
        let fa = crate::engine::features::FeatureAssets::parse(&grid, &tab, &dat).unwrap();
        let mut w = World::new_for_game(planes, &[], 1, fa, crate::ids::GameId::Mc2);
        let pool = w.g.ent.len();

        let live = |class3f: u8, flags: u32| RetailEntMc2 {
            class3f,
            model40: 0,
            flags,
            max_life: 100,
            life: 100,
            ..Default::default()
        };
        let mut ents = vec![RetailEntMc2::default(); pool];
        ents[1] = live(3, 0); // the human carpet (the reserved hole)
        ents[2] = live(5, 0); // one live creature
        ents[3] = live(5, 0x400); // one GHOST (retail byte[1] & 4)
        // Retail's stack at capture: every genuinely free slot, ghost
        // NOT among them (retail pushes it at the next frame's top).
        let stack: Vec<u16> = (4..pool as u16).collect();
        let st = RetailMc2 {
            rand: 1,
            vortex: 0,
            fire_col: 0,
            local_player: 0,
            player_count: 1,
            spawn_ord: [0; 29],
            players: vec![mgc_formats::mgcr::RetailPlayerMc2 {
                flags: 0,
                is_ai: false,
                play_index: 1,
                turn: 0,
                castle: 0,
                cmd_speed: 0,
                strafe: 0,
                invuln: 0,
                wanted: 0,
                hand_left: -1,
                hand_right: -1,
                ..Default::default()
            }],
            ents,
            free_stack: stack.clone(),
            recycle_stack: Vec::new(),
            level: 1,
            base160: 0,
            stagevars: [[0u8; 8]; 11],
        };
        let report = w.retail_import_mc2(&st).expect("import");
        assert_eq!(
            report.stack_fallback, None,
            "the census must accept the recorded stack (else the test is vacuous)"
        );
        assert_eq!(
            w.g.free, stack,
            "the imported free list IS the recorded stack, verbatim"
        );
        assert!(
            !w.g.free.contains(&3),
            "the ghost's push belongs to tick()'s top reap, not the import"
        );
    }

    /// A freed MC1 slot is not an EMPTY slot: retail's free path
    /// clears +64 and pushes the stack — every OTHER byte stays, and
    /// the blind tracker (`sub_52550`, ledger §THE PROJECTILE LEDGER
    /// + BLIND TRACKER) steers at whatever the record still holds.
    /// mc1l0 t=3464-70: bolt 557 tracked reaped slot 534's stale
    /// position (pitch pinned level by the corpse's raw-2048 bearing);
    /// the old `Ent::default()` import re-aimed it at the ORIGIN and
    /// the bolt's whole heading/pitch/aim column diverged. The import
    /// must carry the stale bytes through — class 0, unlinked, not
    /// counted active, still on the free stack.
    /// Non-vacuous: restoring the default arm zeroes the position and
    /// the stale-byte asserts fail.
    #[test]
    fn mc1_import_keeps_a_freed_slots_stale_bytes_for_the_blind_tracker() {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let mut grid = vec![31u8; 1024];
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (dx, dy) = (x - 15, y - 15);
                let r = dx.max(dy).max(-dx + 1).max(-dy + 1) - 1;
                grid[(y * 32 + x) as usize] = r.clamp(0, 31) as u8;
            }
        }
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|_| {
                let mut e = 0u32.to_le_bytes().to_vec();
                e.extend_from_slice(&[4, 4]);
                e
            })
            .collect();
        let mut dat = Vec::new();
        for _ in 0..4 {
            dat.push(4u8);
            dat.extend_from_slice(&[0x10, 0x10, 0x10, 0x10]);
            dat.push(0);
        }
        let fa = crate::engine::features::FeatureAssets::parse(&grid, &tab, &dat).unwrap();
        let mut w = World::new_for_game(planes, &[], 1, fa, crate::ids::GameId::Mc1);
        let pool = w.g.ent.len();

        let mut ents = vec![RetailEntMc1::default(); pool];
        // Slot 1: the human carpet (row-7 model_ptr anchors the base).
        ents[1] = RetailEntMc1 {
            class64: 3,
            model65: 0,
            model_ptr: 7 * 32,
            x: 100 << 8,
            y: 100 << 8,
            ..Default::default()
        };
        // Slot 2: a reaped corpse — class 0, every stale byte intact
        // (the mc1l0 slot-534 shape, model_ptr stale/dangling too).
        ents[2] = RetailEntMc1 {
            class64: 0,
            model65: 1,
            act_life: -400,
            flags: 0x408,
            f58: 7,
            f78: 50,
            x: 53174,
            y: 17486,
            z: 1101,
            model_ptr: 0xDEAD_BEEF,
            ..Default::default()
        };
        // Slot 3: a live bolt still chasing the freed slot.
        ents[3] = RetailEntMc1 {
            class64: 9,
            model65: 0,
            flags: 0x2006,
            act_life: 7,
            x: 57998,
            y: 15993,
            z: 1146,
            f146: 2,
            model_ptr: 0,
            ..Default::default()
        };
        // Retail's stack: every class-0 slot (the fresh corpse rides
        // it — its reap already pushed), human hole and the bolt out.
        let stack: Vec<u16> = std::iter::once(2u16).chain(4..pool as u16).collect();
        let st = RetailMc1 {
            rand: 1,
            local_player: 0,
            player_count: 1,
            spawn_count: [0; 20],
            wizards: {
                let mut ws = vec![RetailWizardMc1::default(); 8];
                ws[0] = RetailWizardMc1 {
                    play_index: 1,
                    hand_left: 0xFFFF,
                    hand_right: 0xFFFF,
                    ..Default::default()
                };
                ws
            },
            ents,
            free_stack: stack.clone(),
            recycle_stack: Vec::new(),
            level: 0,
        };
        let report = w.retail_import_mc1(&st).expect("import");
        assert_eq!(report.active, 1, "only the bolt counts active");
        assert_eq!(
            report.bad_rows, 0,
            "a freed slot's dangling model_ptr is not a bad row"
        );
        let corpse = &w.g.ent[2];
        assert_eq!(corpse.class64, 0, "freed stays freed");
        assert_eq!(
            (corpse.x, corpse.y, corpse.z),
            (53174, 17486, 1101),
            "the stale position survives the import — the blind \
             tracker's whole aim"
        );
        assert_eq!(corpse.model65, 1);
        assert_eq!(corpse.f78, 50, "aim_z's stale +78 lift survives too");
        assert_eq!(corpse.flags & 4, 0, "never linked into a tile chain");
        assert_eq!(
            report.stack_fallback, None,
            "the freed slot still counts as free for the stack census"
        );
        assert!(w.g.free.contains(&2), "and rides the recorded stack");
        assert_eq!(w.g.ent[3].f146, 2, "the bolt still chases the slot");
    }

    /// A FULL MC2 pool still spawns: `NewEvent_4A050` (:581) falls
    /// through to the recycle stack and SACRIFICES the top-ranked live
    /// victim — bare seizure, no death. The import must carry the
    /// recorded ranking verbatim so replay sacrifices retail's victims
    /// in retail's order, and must stop where retail's list stops
    /// (`refill` off: the snapshot IS the law under strict replay).
    /// Non-vacuous: clearing `w.g.mc2_recycle.stack` — the pre-dig
    /// port, i.e. `MGC_NO_RECYCLE_VICTIM=1` — makes the very first
    /// `new_event` return None instead of slot 300.
    #[test]
    fn mc2_full_pool_sacrifices_the_recorded_recycle_victims_in_order() {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let mut grid = vec![31u8; 1024];
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (dx, dy) = (x - 15, y - 15);
                let r = dx.max(dy).max(-dx + 1).max(-dy + 1) - 1;
                grid[(y * 32 + x) as usize] = r.clamp(0, 31) as u8;
            }
        }
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|_| {
                let mut e = 0u32.to_le_bytes().to_vec();
                e.extend_from_slice(&[4, 4]);
                e
            })
            .collect();
        let mut dat = Vec::new();
        for _ in 0..4 {
            dat.push(4u8);
            dat.extend_from_slice(&[0x10, 0x10, 0x10, 0x10]);
            dat.push(0);
        }
        let fa = crate::engine::features::FeatureAssets::parse(&grid, &tab, &dat).unwrap();
        let mut w = World::new_for_game(planes, &[], 1, fa, crate::ids::GameId::Mc2);
        let pool = w.g.ent.len();

        // Every slot occupied: the free stack is EMPTY, exactly the 74
        // mc2l24 snapshots that motivated the arm.
        let mut ents = vec![RetailEntMc2::default(); pool];
        for (s, e) in ents.iter_mut().enumerate().skip(1) {
            *e = RetailEntMc2 {
                class3f: if s == 1 { 3 } else { 5 },
                model40: 0,
                flags: 0,
                max_life: 100,
                life: 100,
                ..Default::default()
            };
        }
        // Retail's ranking, bottom-up: 300 pops first, then 500, 700.
        let victims: Vec<u16> = vec![700, 500, 300];
        let st = RetailMc2 {
            rand: 1,
            vortex: 0,
            fire_col: 0,
            local_player: 0,
            player_count: 1,
            spawn_ord: [0; 29],
            players: vec![mgc_formats::mgcr::RetailPlayerMc2 {
                flags: 0,
                is_ai: false,
                play_index: 1,
                turn: 0,
                castle: 0,
                cmd_speed: 0,
                strafe: 0,
                invuln: 0,
                wanted: 0,
                hand_left: -1,
                hand_right: -1,
                ..Default::default()
            }],
            ents,
            free_stack: Vec::new(),
            recycle_stack: victims.clone(),
            level: 1,
            base160: 0,
            stagevars: [[0u8; 8]; 11],
        };
        let report = w.retail_import_mc2(&st).expect("import");
        assert_eq!(
            report.stack_fallback, None,
            "the census must accept the empty free stack (else the test is vacuous)"
        );
        assert!(w.g.free.is_empty(), "a full pool has no free slot");
        assert_eq!(
            w.g.mc2_recycle.stack, victims,
            "the recorded ranking rides across verbatim"
        );

        // Seizure order = retail's pop order, and the seized record is
        // a fresh `NewEvent` (id24 = own slot, maxLife 300), NOT a
        // corpse: the victim never reaches the free stack.
        assert_eq!(w.g.new_event(), Some(300), "the stack TOP is sacrificed");
        assert_eq!(
            w.g.ent[300].id24, 300,
            "the seized slot was re-`NewEvent`ed"
        );
        assert_eq!(w.g.ent[300].max_life, 300, "…with the allocator defaults");
        assert!(
            w.g.free.is_empty(),
            "a sacrifice is not a death — the slot skips the free stack"
        );
        assert_eq!(w.g.new_event(), Some(500), "then the next-ranked victim");
        assert_eq!(w.g.new_event(), Some(700), "then the last");
        assert_eq!(
            w.g.new_event(),
            None,
            "retail's list ran out, so the port's does too (refill is off \
             under the strict import)"
        );
        assert_eq!(w.take_recycle_seized(), 3, "three victims, counted");

        // A victim that dies normally must LEAVE the stack
        // (`sub_57F20` :5215-34), or the allocator would hand its slot
        // out twice — once from the free stack, once as a sacrifice.
        w.g.mc2_recycle.stack = vec![700, 500, 300];
        w.g.ent[500].flags |= 0x2_0000;
        w.g.free_entity(500);
        assert_eq!(
            w.g.mc2_recycle.stack,
            vec![700, 300],
            "retail's removal swaps the TOP into the hole"
        );
        assert_eq!(w.g.free, vec![500], "the dying victim went free, once");
    }

    /// Minimal MC2 closure for the delta reconstruction: one carpet
    /// and one book manifestation, both of which the caller shapes.
    fn burst_closure(
        carpet_slot: u16,
        carpet: RetailEntMc2,
        tok_slot: u16,
        tok: RetailEntMc2,
    ) -> (RetailMc2, mgc_formats::mgcr::RetailPlayerMc2) {
        let mut ents = vec![RetailEntMc2::default(); 300];
        ents[carpet_slot as usize] = carpet;
        ents[tok_slot as usize] = tok;
        let mut ply = mgc_formats::mgcr::RetailPlayerMc2 {
            play_index: carpet_slot,
            ..Default::default()
        };
        ply.spell_ent[tok.model40 as usize] = tok_slot;
        let st = RetailMc2 {
            rand: 0,
            vortex: 0,
            fire_col: 0,
            local_player: 0,
            player_count: 1,
            spawn_ord: [0; 29],
            players: vec![ply],
            ents,
            free_stack: Vec::new(),
            recycle_stack: Vec::new(),
            level: 3,
            base160: 0,
            stagevars: [[0; 8]; 11],
        };
        (st, ply)
    }

    /// **THE RECORDED `manaRegen` IS ONLY THE APPLIED ONE WHEN THE
    /// MANIFESTATION SITS ABOVE THE CARPET.** See
    /// [`mc2_applied_mana_delta`] for the law; this pins every arm of
    /// it with the mc2l3 take-2 numbers that measured it.
    ///
    /// Non-vacuous: returning `carpet.d88` unconditionally (the
    /// pre-dig import, still reachable as `MGC_NO_MC2_BURST_DELTA=1`)
    /// fails four of the six asserts — the two pins, the end-sequence
    /// freeze and the first-tick wipe.
    #[test]
    fn mc2_burst_delta_is_the_applied_word_not_the_recorded_one() {
        // The carpet as recorded mid-cast: the regen recompute (100
        // afield / 1000 at the castle) is what the frame tail holds.
        let carpet = |d88: i32, action45: u8| RetailEntMc2 {
            class3f: 3,
            model40: 0,
            action45,
            d88,
            ..Default::default()
        };
        // A live manifestation: @0x2E armed timer, @0x30 duration,
        // @0x8C full cost, action 3M (the owned state).
        let tok = |spell: u8, f2e: i16, f30: u16, cost: i32| RetailEntMc2 {
            class3f: 15,
            model40: spell,
            action45: spell * 3,
            f2e,
            f30,
            mana_max: cost,
            ..Default::default()
        };

        // BELOW the carpet, mid-burst → the pin (mc2l3 t=9034-9035:
        // recorded 100, mana FLAT).
        let (st, ply) = burst_closure(167, carpet(100, 0), 109, tok(1, 2, 3, 100));
        assert_eq!(mc2_applied_mana_delta(&st, &ply, 167, &st.ents[167]), 0);

        // BELOW the carpet, FIRST tick → the recompute is wiped; the
        // debit itself is left to `World::mc2_same_frame_debit`, which
        // is what keeps the afford gate reading the pre-debit purse
        // (mc2l3 t=8445, the 40,000 Create Castle out of 41,359).
        let (st, ply) = burst_closure(167, carpet(1000, 0), 114, tok(2, 101, 101, 40000));
        assert_eq!(mc2_applied_mana_delta(&st, &ply, 167, &st.ents[167]), 0);

        // BELOW the carpet, the CASTLE's upgrade LOCK (timer parked,
        // not counting) → no pin at all: mc2l3 t=8446+ climbs +1000.
        let (st, ply) = burst_closure(167, carpet(1000, 0), 114, tok(2, 100, 101, 40000));
        assert_eq!(mc2_applied_mana_delta(&st, &ply, 167, &st.ents[167]), 1000);

        // ABOVE the carpet → the record already holds the token's own
        // stamp; it is applied verbatim (mc2l24 slot 118 vs carpet
        // 116, recorded −100 then 0).
        let (st, ply) = burst_closure(116, carpet(-100, 0), 118, tok(1, 2, 3, 100));
        assert_eq!(mc2_applied_mana_delta(&st, &ply, 116, &st.ents[116]), -100);

        // A DETACHED jar (the wraith steal's action 78) never reaches
        // `sub_68DE0`, so it pins nothing.
        let mut stolen = tok(1, 2, 3, 100);
        stolen.action45 = 78;
        let (st, ply) = burst_closure(167, carpet(100, 0), 109, stolen);
        assert_eq!(mc2_applied_mana_delta(&st, &ply, 167, &st.ents[167]), 100);

        // Action 12, the level-end sequence: the regen block is in the
        // action-0 body alone, so NOTHING is applied (mc2l3 t=22621+).
        let (st, ply) = burst_closure(167, carpet(100, 12), 109, tok(1, 0, 3, 100));
        assert_eq!(mc2_applied_mana_delta(&st, &ply, 167, &st.ents[167]), 0);
    }
}
