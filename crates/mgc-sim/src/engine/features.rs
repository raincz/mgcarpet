//! The shared chassis engine state and load-time terrain features
//! ("GenerateFeatures"). A verbatim remc1 port of the low-level engine
//! state — the shared entity pool ([`Gen`]) and the load-time feature
//! pass — used by all three games (MC1, Hidden Worlds, MC2).
//!
//! Port of remc1's `GenerateFeatures_36430_367F0` (sub_main.cpp:43043):
//! the entity-driven post-generation phase that carves craters and
//! canyons, raises walls and ridges, paints tracks, and flattens/paints
//! building footprints into the pristine generated terrain. Baked
//! `.mgcl` terrain stays pristine by design (docs/FORMAT.md); the
//! engine applies these modifications at level load from `things.json`.
//!
//! Machinery (line references are remc1 sub_main.cpp):
//!
//! - Level entities with `class == 10 && dis_id == 0xFFFF` are terrain
//!   features, consumed in slot order 1..1999. Chained models (28
//!   walls, 29 tracks, 31 canyons, 50 ridges, with `swi_id != 0` as
//!   the not-yet-processed flag) run a polyline walker (sub_362C0,
//!   :42972): root-first via `parent` links, then one segment function
//!   per parent→child pair. Everything else spawns a runtime *event*
//!   through its per-model creator (`off_97D12`, :5075); model 45
//!   (building) additionally gets the footprint fix-up sub_36DF0.
//! - The event loop (sub_36620, :43181) then sweeps the 1000-slot
//!   event pool to fixpoint: craters dig ring by ring, canyon heads
//!   walk and spawn diggers, buildings flatten and paint over 30
//!   ticks, and every non-feature event is purged. Dispatch is by the
//!   entity's byte-70 tick index, not its model.
//! - Determinism: the pool allocates slots 1,2,3,… (free stack built
//!   999→1; frees push back LIFO), and each event seeds a per-entity
//!   LCG from `slot + global_rand`. Two behaviors depend on the slot
//!   number itself: digger radius growth (`slot % 3`, sub_25670) and
//!   dither draws — so slot churn from events that are spawned only to
//!   be purged is load-bearing and reproduced exactly.
//! - PRNG streams (all `x = 9377x + 9439`): the global u32 `rand_4` is
//!   the level seed at scan time and is advanced exactly once at event
//!   loop entry; retiling draws the u16 `pseudoRand` stream whose
//!   post-generation state is replayed from the height plane
//!   ([`post_generation_pseudo_rand`], the generator's shading pass
//!   reset it to 0 and drew once per flat tile).
//!
//! Deliberately omitted (terrain-neutral at load): damage broadcasts
//! (sub_127E0/sub_120B0 — they write damage fields on pool entities;
//! relevant once entities persist), sounds, and the surviving building
//! entities themselves (the entity track will need them; the terrain
//! effect is complete without).
//!
//! Entity-table indices: `things.json` slots are 0-based file order;
//! the engine indexes the same records 1-based (its record 1 = file
//! offset 0x442 = our slot 0), and `parent`/`child` values are those
//! 1-based indices. The pass rebuilds the 1-based table.

use crate::mc1::corners;
use crate::mc1::tables::{ATAN, BIT_SQRT, COS, PAINT_AC, PAINT_BC, PAINT_EC, PAINT_FC, SIN};
use mgc_formats::Thing;

use crate::chassis::{ChassisParams, RandWidth};
use crate::verbs::{VerbKind, VerbSet};

/// Cells in the 256x256 terrain grid.
const GRID: usize = 0x10000;

// THING-table capacity is chassis data (ChassisParams::
// level_table_slots); the feature/disposition scans are len-driven.
// Runtime pool size lives in chassis::ChassisParams::pool_slots
// (slot 0 never allocated); sizing/iteration read `ent.len()`.

/// The four terrain planes the feature pass mutates, engine layout
/// (index = tile_y * 256 + tile_x).
pub struct TerrainPlanes<'a> {
    pub height: &'a mut [u8],
    pub tile_type: &'a mut [u8],
    pub shading: &'a mut [u8],
    pub angle: &'a mut [u8],
}

/// Owned form of the terrain planes — what the runtime world keeps and
/// mutates across ticks (`mgc_sim::world`).
#[derive(Clone)]
pub struct Planes {
    pub height: Vec<u8>,
    pub tile_type: Vec<u8>,
    pub shading: Vec<u8>,
    pub angle: Vec<u8>,
    /// MC2 cave second heightmap (`x_BYTE_14B4E0`): the CEILING, world
    /// height = 32 * value like the floor. EMPTY everywhere except MC2
    /// cave levels (retail's `sub_43D50` never writes it off-cave) —
    /// and hash-transparent when empty, so the MC1/MC2 non-cave golden
    /// streams are unchanged by the field. On caves, `angle` bit 3
    /// means SEALED rock (ceiling pinned to floor−1) — the OPPOSITE of
    /// its non-cave open-sea meaning. Trace:
    /// docs/traces/mc2-cave-terrain-foundation.md.
    pub ceiling: Vec<u8>,
}

impl std::hash::Hash for Planes {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let Planes {
            height,
            tile_type,
            shading,
            angle,
            ceiling,
        } = self;
        height.hash(state);
        tile_type.hash(state);
        shading.hash(state);
        angle.hash(state);
        // Hash-when-present (the FeatureAssets pattern): empty =
        // absent, not "a zero-length plane".
        if !ceiling.is_empty() {
            ceiling.hash(state);
        }
    }
}

/// One building-footprint entry from `BUILD?-0.TAB` (6 bytes on disk:
/// u32 offset into the DAT blob, u8 width, u8 height in tiles).
#[derive(Clone, Copy, Hash)]
pub struct BuildDef {
    pub offset: u32,
    pub w: u8,
    pub h: u8,
}

/// Which retail builder's water-conversion law a flatten pass carries.
/// Both walk the same BUILD RLE cell decode, but convert a water
/// tile to land under different conditions:
/// - `Building` (sub_27D30 :30101-11, authored construction): flip a
///   slope-nibble-0 tile whenever the cell carries a goal, `& 0xF0 |
///   1`, flag-mode retile (sub_33B90).
/// - `CastleInit` (sub_279D0 :29863-917, the level-init instant
///   stamp): height = goal outright, flip like Building but
///   `& 0xF8 | 1` — authored starting castles DO drain their
///   courtyards.
///
/// The live castle painter (sub_285C0) is NOT a flatten pass: its
/// rows fill a goal-delta buffer that the painter applies in one
/// separate sweep — see `fill_castle_goal_row`.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum FlattenLaw {
    Building,
    CastleInit,
}

/// One MC2 `BLDGPRM.DAT` record (4 bytes; remc2
/// Type_D93C0_Bldgprmbuffer.h + loader sub_539A0 :38319): production
/// rate, flag bits (0x10 = GenerateEvents pass F/G split, 8 = no
/// mana/production, 4 = no cave second-heightmap raise, 1 =
/// enterable), and the objective-chain / font index byte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BldgParam {
    pub rate: u16,
    pub flags: u8,
    pub chain: u8,
}

/// Parsed game data the feature pass needs: the SEARCH.DAT ring table
/// and the building footprint RLE maps. `bldgprm` = MC2's building
/// parameter table, `spells` = MC2's SPELLS.DAT (both empty on MC1 —
/// and hash-transparent when empty, so the MC1 goldens' hash stream
/// is unchanged by the fields).
#[derive(Clone)]
pub struct FeatureAssets {
    /// Per ring 0..31: (dx, dy) byte deltas from the dig center, in the
    /// original's row-major emission order (sub_11540, :16784).
    pub rings: Vec<Vec<(u8, u8)>>,
    pub build_tab: Vec<BuildDef>,
    pub build_dat: Vec<u8>,
    pub bldgprm: Vec<BldgParam>,
    /// MC2's spell table ([`crate::mc2::spells`]): the par1-authored
    /// class-10 overrides + class-15 cast costs.
    pub spells: Vec<crate::mc2::spells::Mc2SpellRow>,
    /// MC2's DERIVED sprite-extent pairs (speed_6, rotSpeed_8) per
    /// particle-param row ([`crate::mc2::derive_sprite_extents`] —
    /// retail computes these at load from the sprite bitmaps,
    /// EF:44870-44910). Empty = pre-dims caller → the static table's
    /// raw zero-box values stand.
    pub mc2_sprite_ext: Vec<(u16, u16)>,
}

impl std::hash::Hash for FeatureAssets {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let FeatureAssets {
            rings,
            build_tab,
            build_dat,
            bldgprm,
            spells,
            mc2_sprite_ext,
        } = self;
        rings.hash(state);
        build_tab.hash(state);
        build_dat.hash(state);
        // Only when present — an absent table hashes exactly like the
        // pre-field struct (MC1 goldens hold).
        if !bldgprm.is_empty() {
            bldgprm.hash(state);
        }
        if !spells.is_empty() {
            spells.hash(state);
        }
        if !mc2_sprite_ext.is_empty() {
            mc2_sprite_ext.hash(state);
        }
    }
}

impl FeatureAssets {
    /// `search` = decompressed SEARCH.DAT (1024 bytes, 32x32 ring-index
    /// grid); `build_tab`/`build_dat` = decompressed BUILD?-0.TAB/DAT.
    pub fn parse(search: &[u8], build_tab: &[u8], build_dat: &[u8]) -> Result<Self, String> {
        if search.len() != 1024 {
            return Err(format!(
                "search grid: expected 1024 bytes, got {}",
                search.len()
            ));
        }
        // Center = the first value-0 cell in row-major scan; ring j's
        // entries are all value-j cells in the same scan order.
        let c = search
            .iter()
            .position(|&v| v == 0)
            .ok_or("search grid has no ring-0 cell")?;
        let (cx, cy) = ((c % 32) as u8, (c / 32) as u8);
        let mut rings = vec![Vec::new(); 32];
        for (j, ring) in rings.iter_mut().enumerate() {
            for y in 0..32u8 {
                for x in 0..32u8 {
                    if search[y as usize * 32 + x as usize] == j as u8 {
                        ring.push((x.wrapping_sub(cx), y.wrapping_sub(cy)));
                    }
                }
            }
        }
        if build_tab.len() % 6 != 0 {
            return Err(format!(
                "build tab: {} bytes is not 6-byte entries",
                build_tab.len()
            ));
        }
        let tab: Vec<BuildDef> = build_tab
            .chunks_exact(6)
            .map(|e| BuildDef {
                offset: u32::from_le_bytes(e[0..4].try_into().unwrap()),
                w: e[4],
                h: e[5],
            })
            .collect();
        for (i, b) in tab.iter().enumerate() {
            if (b.offset as usize) >= build_dat.len() && (b.w != 0 || b.h != 0) {
                return Err(format!("build tab entry {i} offset {} past dat", b.offset));
            }
        }
        Ok(Self {
            rings,
            build_tab: tab,
            build_dat: build_dat.to_vec(),
            bldgprm: Vec::new(),
            spells: Vec::new(),
            mc2_sprite_ext: Vec::new(),
        })
    }

    /// Attach MC2's `BLDGPRM.DAT` table (4-byte records; the loader
    /// reads 76 x 4 of the 77-record file, sub_539A0 :38319 — we take
    /// every whole record present).
    pub fn with_bldgprm(mut self, bytes: &[u8]) -> Self {
        self.bldgprm = bytes
            .chunks_exact(4)
            .map(|r| BldgParam {
                rate: u16::from_le_bytes([r[0], r[1]]),
                flags: r[2],
                chain: r[3],
            })
            .collect();
        self
    }

    /// Attach MC2's `SPELLS.DAT` table (`spells.bin`, 26 x 80 bytes;
    /// [`crate::mc2::spells::parse`]). A malformed blob is a bake bug
    /// — surface it instead of silently running on ctor defaults.
    /// Retail's LevelInit.cpp:12-21 patch of rows 4 and 19 (Day vs
    /// non-Day, tier-0 life + hintText) is applied later, by
    /// `World::set_mc2_night_shade` — the seam that declares the
    /// level's environment ([`crate::mc2::spells::level_init_patch`]).
    pub fn with_spells(mut self, bytes: &[u8]) -> Result<Self, String> {
        self.spells = crate::mc2::spells::parse(bytes)?;
        Ok(self)
    }

    /// Attach the derived MC2 sprite extents (the retail load-time
    /// pass over the sprite bitmaps — feed
    /// [`crate::mc2::derive_sprite_extents`] with the baked sprite
    /// index dims).
    pub fn with_mc2_sprite_ext(mut self, ext: Vec<(u16, u16)>) -> Self {
        self.mc2_sprite_ext = ext;
        self
    }
}

/// The engine's LCG, 32-bit state (`rand_4` and per-entity streams).
#[inline]
pub(crate) fn lcg32(s: &mut u32) -> u32 {
    *s = s.wrapping_mul(9377).wrapping_add(9439);
    *s
}

/// Tile index from u8 coordinates (low byte = x, high byte = y).
#[inline]
pub(crate) fn tile(x: u8, y: u8) -> usize {
    ((y as usize) << 8) | x as usize
}

#[inline]
fn tx(t: usize) -> u8 {
    t as u8
}
#[inline]
fn ty(t: usize) -> u8 {
    (t >> 8) as u8
}
/// Move a packed tile index by wrapping each byte axis independently.
#[inline]
fn step(t: usize, dx: i32, dy: i32) -> usize {
    tile(tx(t).wrapping_add(dx as u8), ty(t).wrapping_add(dy as u8))
}

/// Replay the generator's final shading pass on the pristine height
/// plane to recover the u16 `pseudoRand` state at GenerateFeatures
/// time (the pass reset the stream to 0, then drew once per flat cell
/// — `sub_329C0`, mirrored by mc1_terrain's `shading_pass`).
pub fn post_generation_pseudo_rand(height: &[u8]) -> u16 {
    let mut s = 0u16;
    for i in 0..=0xFFFFu16 {
        let hi = height[step(i as usize, -1, -1)];
        let lo = height[step(i as usize, 1, 1)];
        if hi.wrapping_sub(lo).wrapping_add(32) == 32 {
            s = s.wrapping_mul(9377).wrapping_add(9439);
        }
    }
    s
}

/// One record of the original 18-byte THING_INIT table (1-based copy).
/// The runtime world keeps this table live: dispositions scan it and
/// one-shot spawns zero the class (`sub_37440_37800`).
#[derive(Clone, Copy, Default)]
pub(crate) struct Rec {
    pub(crate) class: u16,
    pub(crate) model: u16,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) dis_id: u16,
    /// Switch size (`data_10`): trigger volume radius in tiles.
    pub(crate) swi_sz: u16,
    pub(crate) swi_id: u16,
    pub(crate) parent: u16,
    pub(crate) child: u16,
    /// MC2 `par3_18` (the third context parameter; 0 on MC1 records) —
    /// the cave pit/hill depth seed and the tube-carver radius nibble.
    pub(crate) par3: u16,
}

impl std::hash::Hash for Rec {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // par3 is STATIC level input (never mutated at runtime, unlike
        // class/swi_id) — excluded from the hash (hash-transparent, so
        // MC2 state-hash goldens hold).
        let Rec {
            class,
            model,
            x,
            y,
            dis_id,
            swi_sz,
            swi_id,
            parent,
            child,
            par3: _,
        } = self;
        class.hash(state);
        model.hash(state);
        x.hash(state);
        y.hash(state);
        dis_id.hash(state);
        swi_sz.hash(state);
        swi_id.hash(state);
        parent.hash(state);
        child.hash(state);
    }
}

/// Runtime event entity — the subset of remc1's 164-byte
/// `Type_AE400_29795` the load-time feature path uses. Names keep the
/// original byte offsets for traceability.
#[derive(Clone, Copy, Default, Hash)]
pub(crate) struct Ent {
    /// Per-entity LCG (offset 4), seeded `slot + global_rand` at alloc.
    pub(crate) rand: u32,
    pub(crate) max_life: u32,
    pub(crate) act_life: i32,
    /// Flags (offset 16). Bit 0 (0x1) = active, bit 1 (0x2) =
    /// dug/second-phase, bit 2 (0x4) = linked into the tile map,
    /// bit 10 (0x400) = marked dead.
    pub(crate) flags: u32,
    pub(crate) next20: u16,
    pub(crate) prev22: u16,
    /// The disposition this event fires / entity link (offset 24, from
    /// the THING's `swi_id`). NewEvent defaults it to the OWN slot —
    /// for projectiles/effects the cast/thunk overwrites it with the
    /// caster's id, and +24 equality is the engine's only friendly-
    /// fire rule (owner immunity).
    pub(crate) id24: u16,
    /// Killer id latch (offset 38) and attacker latch (offset 40) —
    /// written by the damage inbox block, read by DEATH's kill credit
    /// and the aggro retarget.
    pub(crate) f38: u16,
    pub(crate) f40: u16,
    /// Vertical velocity (offset 46): mana-ball gravity, fire flicker.
    pub(crate) f46: i16,
    /// Damage-response countdown (offset 50): a blast near a castle
    /// arms 30 ticks (sub_127E0 :17522); expiry sends the castle to
    /// the repaint sub-state (:55987-93). The downgrade arms 5.
    pub(crate) f50: i16,
    /// Explosion class/model a projectile detonates into (offsets
    /// 68/69). NewEvent defaults +68 = 10 (:43879), +69 = 0 (fire).
    pub(crate) f68: u8,
    pub(crate) f69: u8,
    /// Damage mailboxes (offsets 90..124): six {u32 amount, u16
    /// source-id} channels. ch0 = physical damage, ch1 = mana-ball
    /// claim, ch3 = mana steal, ch4 = grip/attract, ch5 = balloon
    /// recall. Writers accumulate while a source is pending and
    /// overwrite stale amounts (readers clear the source but NOT the
    /// amount — :17301-05).
    pub(crate) mail: [(u32, u16); 6],
    /// Mana-ball owner (offset 144): the wizard whose collection claim
    /// (ch1) tagged the ball; corpses pass theirs to the dropped ball.
    pub(crate) f144: u16,
    /// Generic counter (offset 26): crater ring counter, wall run
    /// length, trigger rearm/debounce countdown.
    pub(crate) f26: i16,
    pub(crate) f28: u16,
    /// Wall step dx/dy (offsets 30/32); canyon/ridge heading (30).
    pub(crate) f30: u16,
    pub(crate) f32: u16,
    /// Strength (offset 44).
    pub(crate) f44: u16,
    /// Target yaw (offset 34, 11-bit engine angle; high byte = pitch
    /// for fliers) and its offset-36 companion (zeroed at spawn).
    pub(crate) f34: u16,
    pub(crate) f36: u16,
    /// Multipart chain links (offsets 52/54): +52 = toward the head
    /// (the segment's leader), +54 = toward the tail. 0 = end.
    pub(crate) f52: u16,
    pub(crate) f54: u16,
    /// Segment follow distance (offset 56, engine units).
    pub(crate) f56: u16,
    /// Awake countdown (offset 58): >0 = the creature acts (damage
    /// intake, hostile scans, segment follow); decremented by the
    /// pre-pass, re-armed to 16 (segments 18) while the player is
    /// within 24 tiles. Spawn staggers the initial value by the spawn
    /// ordinal. NewEvent default 0xFA.
    pub(crate) f58: i16,
    /// Awake re-probe delay (offset 59).
    pub(crate) f59: u8,
    /// Slot index at alloc (offset 63); the RUNTIME loop increments it
    /// per tick (:52417) — gates digger radius growth (`% 3`) and the
    /// trigger probe throttle (`& 7`). The load-time fixpoint loop
    /// never increments, so there it stays the alloc slot. Creature
    /// spawns overwrite it with the per-model spawn ordinal.
    pub(crate) f63: u8,
    pub(crate) class64: u8,
    pub(crate) model65: u8,
    /// Team/owner (offset 66; creatures spawn as 3 = wild) and its
    /// offset-67 companion. NewEvent defaults both to 0xFF.
    pub(crate) f66: u8,
    pub(crate) f67: u8,
    /// Tick-handler index (offset 70).
    pub(crate) tick70: u8,
    /// Building-table index (offset 71).
    pub(crate) f71: u8,
    /// Position, 8.8 fixed point (offsets 72/74/76).
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) z: i16,
    /// Sprite half-height (offset 78, set with the extents by
    /// `sub_36FA0` from the stats row).
    pub(crate) f78: u16,
    /// Extents (offsets 80/82/84); high byte of f80 = dig radius in tiles.
    pub(crate) f80: u16,
    pub(crate) f82: u16,
    pub(crate) f84: u16,
    /// Sprite-stats type index (offset 86), animation frame (88) and
    /// frame count (89) — what the billboard layer draws.
    pub(crate) type86: u16,
    pub(crate) frame88: u8,
    pub(crate) frames89: u8,
    /// Advance per tick (offset 126); building area>>4 (offset 128).
    /// For creatures +126 is the actual speed toward max speed +128
    /// with acceleration +130 (engine units per tick, 8.8).
    pub(crate) f126: i16,
    pub(crate) f128: i16,
    pub(crate) f130: i16,
    /// Mana pool / per-tick mana (offsets 136/140; the mana track
    /// consumes these — carried for faithful spawn state).
    pub(crate) f136: i32,
    pub(crate) f140: i32,
    /// Chase target (offset 146): pool slot of the hunted entity;
    /// [`crate::mc1::mobs::PLAYER_TARGET`] = the player's carpet.
    pub(crate) f146: u16,
    /// Behavior row index into [`crate::mc1::behavior::BEHAVIOR`]
    /// (offset 156 holds `&unk_98F38[N]` in the original).
    pub(crate) row156: u8,
    /// Source THING table index (1-based; ours, not original layout) —
    /// lets the app resolve spawned drawables through the per-slot
    /// spawn-RNG approximation. 0 = not from a THING.
    pub(crate) thing_slot: u16,
    /// Teleport destination (offsets 150/152, 8.8 fixed) — the portal's
    /// target; defaults to its own position, overwritten by the THING
    /// post-init (child/parent fields).
    pub(crate) dest_x: u16,
    pub(crate) dest_y: u16,
    /// Build-site z (offset 154): the castle's painter/leveler datum
    /// — distinct from the live entity z (+76), which tracks the
    /// ground under the flag every tick.
    pub(crate) site_z: i16,
}

impl Ent {
    /// The aim-z bracket (MC1 sub_524C0/sub_524E0 :62503-14, MC2
    /// twin sub_65580/sub_655A0 EF:62750-67): homing, acquisition
    /// and impact placement measure a target at its z-box center
    /// (z + signed +78) EXCEPT model 2, measured at the RAW z. Both
    /// games guard on the MODEL byte alone (MC1 +65, MC2 +64 — each
    /// layout's model slot; remc2 names its +64 `model_0x40_64`,
    /// values "2 - castle"). For a castle (3,2) the raw z is the
    /// ground under the flag — projectiles home on the FLAG, not
    /// 8192 under the base (+78 is the castle's 0xE000 collision
    /// marker, not a center). The MC2 class-3 acquire walk routes
    /// model 2 through the dedicated raw-position castle scorer
    /// sub_685D0 (EF:54790/54899/54945) — same cones/score, so the
    /// guard alone reproduces it.
    pub(crate) fn aim_z(&self) -> i16 {
        if self.model65 == 2 {
            self.z
        } else {
            self.z.wrapping_add(self.f78 as i16)
        }
    }
}

/// Pending MC2 player debuff-stamp hits (slow webs, paralyze webs).
/// Manual Hash: contributes to the state hash ONLY while hits are
/// pending — hash-transparent when idle (the Planes ceiling / Rec par3
/// discipline).
#[derive(Default)]
pub(crate) struct Mc2PlayerDebuffs {
    pub(crate) slow: u8,
    pub(crate) stun: u8,
}

impl std::hash::Hash for Mc2PlayerDebuffs {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.slow != 0 || self.stun != 0 {
            // Field tag: keeps this pair from aliasing a neighboring
            // conditional contribution of the same width.
            state.write_u8(6);
            (self.slow, self.stun).hash(state);
        }
    }
}

/// The full-screen palette flash (`sub_44BE0_44F20` → `Type_160+152`,
/// the row code read by the frame tail at :41813). The original writes
/// the row ONLY when the arming entity's owner is the local player,
/// paints the whole 256-entry palette once, then hands back to the
/// case-1 `FadeInOut(pal, 4, 1)` ramp — one tinted frame plus a short
/// fade home. We keep the retail row plus a tick countdown for the
/// app-side overlay to shape that fade.
///
/// Rows in use: 2 = red (a processed hit on the player, :55722 — the
/// long-standing [`crate::engine::world::Player::hit_flash`]), 3 =
/// R+48/B saturated over the untouched green, i.e. the violet wash of
/// Global Death's detonation (:31311), 6 = the warm R+48/G+32/B+32
/// wash of a creature landing a charge (:29215, unported), 7 = the
/// greyscale death-out (:55465/:55628, drawn by `LifeState::Dead`).
///
/// Presentation-only: hash-silent ALWAYS, like [`SlotGens`] — an
/// overlay tint can never feed simulation state.
#[derive(Clone, Copy, Default)]
pub(crate) struct PalFlash {
    /// The retail row code; 0 = no flash armed.
    pub(crate) row: u8,
    /// Ticks left in the app-side fade home.
    pub(crate) ticks: u8,
}

impl PalFlash {
    /// Arm `row` for the app overlay. Later arms overwrite earlier
    /// ones — retail's +152 is a single byte, last writer wins.
    pub(crate) fn arm(&mut self, row: u8) {
        self.row = row;
        self.ticks = Self::LIFE;
    }

    /// The overlay fade length. Retail paints one frame and fades back
    /// over the case-1 4-step ramp; 6 sim ticks reads the same at our
    /// tick rate.
    pub(crate) const LIFE: u8 = 6;
}

impl std::hash::Hash for PalFlash {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// The pool's SCRATCH slot — retail's `str_29795[0]` /
/// `Entities_EA3E4[0]`, the always-present entity 0 that routines
/// borrow to run a handler on a synthetic event without allocating
/// (the castle demolish's fake collapse, MC1 :56517-24; MC2's
/// downgrade restore, EF:61628-31). Never allocated by
/// [`Gen::new_event`] (the free stack is built 999→1) and never
/// visited by a pool scan (they all start at 1).
pub(crate) const SCRATCH: usize = 0;

/// The event-pool engine: terrain planes + the original's 1000-slot
/// event pool and PRNG streams. Serves both the load-time feature pass
/// (fixpoint loop, this module) and the runtime world tick
/// (`mgc_sim::world`, one pass per turn) — in the original these are
/// the same pool and the same handlers.
#[derive(Hash)]
pub(crate) struct Gen {
    pub(crate) t: Planes,
    pub(crate) assets: FeatureAssets,
    /// `byte_B5D40`: 2401 x {texture, orientation bits} retile table.
    pub(crate) retile: Vec<[u8; 2]>,
    /// Per-tile head of the event intrusive list (`mapEntityIndex`).
    pub(crate) map_entity: Vec<u16>,
    pub(crate) ent: Vec<Ent>,
    /// Per-slot spawn generation (see [`SlotGens`]) — presentation
    /// identity across snapshots, hash-silent always.
    pub(crate) slot_gen: SlotGens,
    /// Free stack; built 999→1 so allocation pops 1, 2, 3, …
    pub(crate) free: Vec<u16>,
    /// Tick-start mana-ball roster (see [`TickChain`]).
    pub(crate) ball_chain: TickChain,
    /// Tick-start per-model class-5 roster chains (see [`MobChains`]).
    pub(crate) mob_chains: MobChains,
    /// MC2's recycle-victim stack — the allocator's FALLBACK once
    /// `free` is dry (see [`Mc2Recycle`]). Empty on MC1, whose
    /// allocator has the opposite priority.
    pub(crate) mc2_recycle: Mc2Recycle,
    /// The castle-guard REGISTER (retail wizext+84, per OWNER): 34
    /// positional guard-slot entries the fleet dispatch walks
    /// (`sub_47400` :56412-47). The register lives on the WIZARD and
    /// SURVIVES castle death — a stale entry (freed/reused slot, or a
    /// state-95 guard corpse) re-arms the castle's +46 cooldown
    /// WITHOUT spawning, which is why a rebuilt castle's first guard
    /// arrives 16 dispatch passes late (mc1l1 t=2571: guard 313 died
    /// ~t=2000, the stale entry re-arms at the state-4 entry, the
    /// fresh guard lands t≈2605 — the port's live census spawned at
    /// t=2572). Position matters: a stale entry BEFORE the first
    /// empty slot blocks that same pass's spawn.
    pub(crate) mc1_guard_reg: Mc1GuardReg,
    /// The MANA-BALLOON REGISTER (retail wizext+52, per OWNER): three
    /// positional pool slots the fleet dispatch walks instead of a
    /// pool census (`sub_47400` :56329-95). Register INDEX is the
    /// law: it fixes the ball-pick order (index 0 picks first and
    /// takes the nearest ball), it names the two exclusions handed to
    /// `sub_46CA0` (:56377-80), and the over-quota / downgrade cull
    /// frees the slots at index >= quota (:56399-411) — never "the
    /// highest pool slot". Like the guard register it lives on the
    /// WIZARD and outlives the castle.
    pub(crate) mc1_balloon_reg: Mc1BalloonReg,
    /// Global LCG (`rand_4`), = the level seed at scan time.
    pub(crate) rand: u32,
    /// Terrain-retile LCG (`pseudoRand`), u16 stream.
    pub(crate) pseudo: u16,
    /// Per-model spawn ordinals (`str_AE400+12+model`, Type_AE400_20
    /// str_12): creature spawns record the old value into +63 and
    /// increment; model-7 sprite alternation keys off its parity.
    pub(crate) spawn_count: [u8; 20],
    /// The human player's damage inbox — the player lives outside the
    /// pool ([`crate::mc1::mobs::PLAYER_TARGET`]), so writers land here.
    /// The invincible-player dev mode discards it every tick like the
    /// original's spawn grace (:55367-71), accumulating the totals.
    pub(crate) player_mail: [(u32, u16); 6],
    /// Total ch0 damage the (invincible) player has absorbed.
    pub(crate) player_damage: u64,
    /// `gamedata+36` / `gamedata+38` (sub_25EC0): the currently
    /// erupting volcano's pool slot and its (10,19) plume's slot —
    /// 0 = none. One volcano erupts at a time; a driver that dies
    /// unclean leaves the register pointing at itself (authentic
    /// quirk: no volcano can re-arm until a clean death clears it).
    pub(crate) erupting: u16,
    pub(crate) plume: u16,
    /// The player's knock/buffet fields (Type_160 v_24 direction /
    /// v_22 magnitude, :23225-28 kraken writer, :55204-218 consumer):
    /// per-tick horizontal displacement forced onto the carpet.
    /// DIRECT struct writes in the original — spawn grace does NOT
    /// wipe them, so even the invincible dev player gets dragged.
    pub(crate) player_knock: (u16, i16),
    /// The player's pending forced HEADING delta, 11-bit engine
    /// angle. The whirlwind's wizard arm (`sub_33340` EF:24296+) does
    /// not merely shove the flyer: EVERY branch that touches a victim
    /// also writes its `yaw_0x1C_28` — `+56` per tick for a class-3
    /// model-0 (the wizard's own step; creatures get 204), or the
    /// tangent bearing `+591` on the mid ring. The port carried the
    /// shove on [`Gen::player_knock`] and dropped the heading, so a
    /// tornado threw you around while you kept facing exactly where
    /// you started. Same transport shape as the knock (world writes,
    /// the mover drains it once), but NO decay: retail re-writes it
    /// from scratch every tick the funnel holds you, and the tick it
    /// stops is the tick you stop turning.
    pub(crate) player_spin: PlayerSpin,
    /// Pending MC2 debuff-stamp hits on the player — (10,65) slow
    /// web / (10,66) paralyze web (`sub_38E70`/`sub_38F70`
    /// EF:28407/28442) — drained into the flight `Mc2Ext` channels
    /// by the sim boundary each tick (docs/traces/mc2-flight-model.md
    /// §5c/5d). Hash-only-when-pending (the Planes pattern): the zero
    /// state contributes nothing.
    pub(crate) mc2_debuffs: Mc2PlayerDebuffs,
    /// Rival wizard entity by player slot (0 = none; slot 0 = the
    /// human, unused) — the sprite-family team resolver for owner
    /// recolors (mana balls 105+8·team, balloons 169+team, castle
    /// flags 177+team). Maintained by the rival spawn/respawn path;
    /// claims of an eliminated wizard keep their color (property
    /// persists).
    pub(crate) rival_ents: [u16; 8],
    /// Per-color MC2 Life scalar for the castle-HP ladder (see
    /// [`Mc2LifeScale`]); written by the MC2 rival spawn.
    pub(crate) mc2_life_scale: Mc2LifeScale,
    /// The human player's village-aggro timer (the wizard struct's
    /// +528): set to 200 by offenses against village property or
    /// population (building hits, villager-family hits and kills),
    /// decremented once per world tick (:55405-06). m4 militia only
    /// hunt a wizard whose timer is live — the hostility gate.
    pub(crate) player_aggro: i16,
    /// The RIVAL wizards' village-aggro timers, by player slot 1..=7
    /// (index 0 is the human, who uses [`Gen::player_aggro`]; kept 0).
    /// Retail carries this per wizard in the same +528 struct slot; the
    /// port splits it because the human lives outside the pool. Set to
    /// 200 by a rival's own village offenses, decremented with
    /// `player_aggro`; the m4 militia and m8 griffon wanted-gates read
    /// it through [`Gen::village_wanted`].
    pub(crate) rival_wanted: [i16; 8],
    /// The player's Invisible cloak (spell 12; the wizard's +16 0x20
    /// bit, :65689-90) mirrored in for the mob-side target gates.
    pub(crate) player_invisible: bool,
    /// The player's Rebound deflection bit (spell 14; +17 0x80,
    /// :65774) — incoming class-9 projectiles bounce back.
    pub(crate) player_rebound: bool,
    /// Player stat counters: creatures killed (`Type_160+359`), shots
    /// resolved (+343), shots that struck the aimed target (+347).
    pub(crate) kills: u32,
    pub(crate) shots: u32,
    pub(crate) hits: u32,
    /// The wizard's danger-music countdown (Type_160 v_46): armed to
    /// 100 by processed hits (sub_46540's blocks call sub_46520) and
    /// by a projectile acquiring the player as target (:64013); the
    /// player tick decrements it and switches the music mode on
    /// v_46 > 0 (:55282-92 → sub_20D00).
    pub(crate) player_danger: i16,
    /// The claimed-house mana tally (wizext u32_308), stashed by the
    /// per-tick census — the castle overflow ejector's trigger reads
    /// houses + stored vs capacity (sub_47130 :56185-89).
    pub(crate) banked_houses: i32,
    /// "Castle under attack" HUD flash (Type_160+391 = 4, :56698) —
    /// armed by every processed castle hit, decremented per tick.
    pub(crate) castle_alert: u8,
    /// "You are being attacked" HUD flash (Type_160+392 = 4,
    /// :55679/:55692/:55723) — armed by every processed player hit /
    /// steal / grip, decremented per tick. The SELF sub-panel's alert.
    pub(crate) player_alert: u8,
    /// Balloon-under-attack HUD flash (Type_160+393 = 4, :56826) —
    /// armed by a processed hit on an own balloon, decremented per
    /// tick. The balloon sub-panel's alert.
    pub(crate) balloon_alert: u8,
    /// The full-screen palette flash armed by `sub_44BE0` — see
    /// [`PalFlash`]. Hash-silent (presentation).
    pub(crate) pal_flash: PalFlash,
    /// Allocations dropped on pool exhaustion (the limit-removing
    /// register's telemetry; the app logs increases). The original
    /// keeps no such count — it is observability, not behavior.
    pub(crate) exhausted: u32,
    /// The per-game chassis constant set ([`crate::chassis`]); fixed
    /// at construction, never rebranched on.
    pub(crate) chassis: ChassisParams,
    /// The per-game tier-5 verb column ([`crate::verbs`]); fixed at
    /// construction. Branched on ONLY at the dispatch seams — never
    /// inside a handler.
    pub(crate) verbs: VerbSet,
    /// Bitmask of [`crate::verbs::VerbKind`]s whose requested arm is
    /// pending and fell back to MC1 (seam telemetry, noted once each;
    /// the app/tests read it via `World::verb_fallbacks`).
    pub(crate) verb_fallbacks: u8,
    /// Unknown `(class, model, count)` things the spawn seam refused
    /// (graceful degradation's ledger; the original has no analogue —
    /// observability, not behavior).
    pub(crate) misfits: Vec<(u16, u16, u32)>,
    /// Sound requests emitted this tick at the original's
    /// sub_55370_558A0 call sites; drained by the app into the audio
    /// mixer (which reimplements that routine's attenuation/slot
    /// policy). Position/tag mirror the entity the original passed.
    pub(crate) sounds: Vec<SoundEvent>,
    /// Terrain changed inside a Gen-internal path with no dirty-
    /// returning dispatch arm (the castle downgrade's synchronous
    /// un-stamp collapse); World::tick merges + clears per turn.
    pub(crate) terrain_dirty: bool,
    /// MC2 non-day shading: `sub_462A0` inverts the relief shade on
    /// Night/Cave maps (remc2 Terrain.cpp:2030-2033). Per-LEVEL, set
    /// by the app from the level's environment. Hash-transparent when
    /// off so the MC1 golden hash stream is unchanged by the field.
    pub(crate) mc2_night_shade: NightShade,
    /// MC2 per-model spawn ordinals (`D41A0_0.array_0x10[model]++`,
    /// remc2 EventsFunctions.cpp per-ctor) — the per-instance phase
    /// stagger every MC2 class-5 ctor stores into byte_0x3E_62 (our
    /// f63). Separate from MC1's `spawn_count` (its own column) and
    /// hash-transparent while untouched so the MC1 golden stream is
    /// unchanged by the field.
    pub(crate) mc2_spawn_ord: Mc2Ord,
    /// m26's mana leech against the HUMAN accumulates here (remc2
    /// EF:19331-34 drains the target wizard's mana; the MC2
    /// wizard-mana ledger consumes this when it lands). Pool wizards
    /// are debited directly. Hash-transparent at zero.
    pub(crate) mc2_player_drain: Mc2Quiet<1>,
    /// Running "scrolls collected" tally for the human. The XP award
    /// is live — `mc2_class14_tick` grants +4 to every owned spell on
    /// pickup (UpdateScroll_59C80 EF:41180-83) — so this counter is now
    /// a hashed tally only, not a deferral bank. Hash-transparent at
    /// zero.
    pub(crate) mc2_scrolls: Mc2Quiet<2>,
    /// The human's collected MC2 spell tokens, a bitmask by spell
    /// model 0..25 (retail: `SpellEnabled[model]` on the wizard,
    /// sub_68FF0 EF:55726) — banked for the Phase-4.2 spell system
    /// like the scrolls. Hash-transparent at zero.
    pub(crate) mc2_spell_tokens: Mc2Quiet<3>,
    /// MC2 spell-XP mail (owner id, spell index): projectile impacts
    /// award from inside the pool tick (`sub_6D8B0` call sites,
    /// EF:63189 etc.); the world tick drains it into the wizard's
    /// book the same turn — empty at hash time like a read mailbox
    /// (and hash-transparent when empty).
    pub(crate) mc2_cast_xp: Mc2XpMail,
    /// m26 spell-steal requests (`sub_28FF0` EF:19348-71 → the
    /// `sub_69300` effect): the wraith's roll lands pool-side but the
    /// human book is world-side — the world tick drains this the
    /// same turn. Hash-transparent while empty.
    pub(crate) mc2_steal_mail: Mc2StealMail,
    /// Lightning-strike presentation events (the enhanced-lightning
    /// render feed): every resolved beam pushes its muzzle→terminus
    /// strike here; the frontend drains it per tick. PURE
    /// PRESENTATION — hash-SILENT always (the sim state it describes,
    /// trail nodes + blasts, is the hashed retail state) and never
    /// saved (cleared on load).
    pub(crate) bolt_fx: BoltFx,
    /// The mana-magnet aura CLAIM handshake (`word_0x7A_122` on the
    /// ball, EF:28364/28383): ball slot → claiming aura slot. An aura
    /// claims an unclaimed ball for one pull; the ball's own tick
    /// consumes and clears the claim — first-in-list keeps the ball
    /// when auras overlap. Hash-quiet while empty.
    pub(crate) mc2_aura_claim: Mc2SlotMap<4>,
    /// Pool wizards' WANTED timers (`word_0x248_584`): wizard slot →
    /// remaining hostility ticks. The human's lives in
    /// [`Gen::player_aggro`]; rivals had no `Ent` home. Armed by
    /// [`Gen::mc2_arm_wanted`], run down with the aggro cadence,
    /// read by the archer Scan-A post-reject. Hash-quiet while empty.
    pub(crate) mc2_wanted: Mc2SlotMap<5>,
    /// The human's REBOUND tier bit (`sub_6AA00` EF:56721-51: tier
    /// `life==1` stamps PRECISE — byte0xc[0]|=0x10, exact return +
    /// doubled payload; `life==0` scatter — byte[1]|=0x80). Rides
    /// beside the [`Gen::player_rebound`] mirror; 0 = scatter.
    /// Hash-transparent at zero.
    pub(crate) mc2_rebound_precise: Mc2Quiet<6>,
    /// ALLIANCE charms (spell 24): charmed creature slot → the
    /// caster's owner id (retail keeps `parentId` ON the entity,
    /// EF:29688; the port's creatures never modeled parentId — the
    /// charm must NOT clobber `id24`, the authored disposition the
    /// stage census keys on). The tier duration counts down in the
    /// creature's `f26` (`word_0x2E_46`; its `word_0x30_48` companion
    /// has no port home — f28 is the MC2 damage-contract flag).
    /// Hash-quiet while empty.
    pub(crate) mc2_allied: Mc2SlotMap<8>,
    /// Per-wizard castle research (`array_0x24E_590`, player struct
    /// +0x24E): `[stage-1]` in `.1` = the stage's HP factor
    /// (`subSpellIndex_2`), `[stage-1]` in `.2` = the stage's
    /// PART-TYPE (`life_0x1A` — 1 = fire tower, 2 = lightning),
    /// keyed by owner id. Retail fills it via the research child
    /// (`sub_69AB0` EF:56120-21) for stage `castleLevel+1`; the
    /// port stamps at cast/upgrade time from the castle-spell tier
    /// (the A.5 shortcut, castle-and-cost.md) until the research
    /// production chain lands. Hash-quiet while empty.
    pub(crate) mc2_castle_research: Mc2CastleResearch,
}

/// See [`Gen::mc2_castle_research`] — hashes to NOTHING while empty
/// (the [`Mc2Ord`] pattern; tag 7 disambiguates adjacent quiet
/// fields). Entries are `(owner, hp_factor[stage-1],
/// part_type[stage-1])` for stages 1..=7 (retail slots 1..7 / 10..16
/// of the 19-byte array — slots 0/8/9/17/18 are never addressed).
#[derive(Default)]
pub(crate) struct Mc2CastleResearch(pub Vec<(u16, [u8; 7], [u8; 7])>);

impl std::hash::Hash for Mc2CastleResearch {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if !self.0.is_empty() {
            state.write_u8(7);
            for (own, hp, part) in &self.0 {
                state.write_u16(*own);
                state.write(hp);
                state.write(part);
            }
        }
    }
}

/// See [`Gen::mc2_cast_xp`] — hashes to NOTHING while empty (the
/// [`Mc2Ord`] pattern). Entries are `(owner, spell, amount)`: the
/// area-spell effect ticks award BATCH counts (retail's single
/// `sub_6D8B0(id, spell, hits)` call per pass — one award, one
/// level-up notification), so the mail carries the amount.
#[derive(Default)]
pub(crate) struct Mc2XpMail(pub Vec<(u16, u16, i32)>);

impl std::hash::Hash for Mc2XpMail {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if !self.0.is_empty() {
            self.0.hash(state);
        }
    }
}

/// One resolved lightning strike (both games): the beam's muzzle and
/// terminus in raw sim units, plus the owner. Presentation-only —
/// consumed by the frontend's bolt ledger.
#[derive(Clone, Copy, Debug)]
pub struct BoltStrike {
    pub start: (u16, u16, i16),
    pub end: (u16, u16, i16),
    pub owner: u16,
}

/// See [`Gen::bolt_fx`] — hash-SILENT ALWAYS (the `slot_gen` class of
/// field: dropping it changes nothing observable to the sim), unlike
/// the drained-mail wrappers which hash when non-empty.
#[derive(Default)]
pub(crate) struct BoltFx(pub Vec<BoltStrike>);

impl std::hash::Hash for BoltFx {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// See [`Gen::mc2_steal_mail`] — (wraith slot, hand: 1 = right,
/// 2 = left) requests from the m26 steal roll, drained by the world
/// tick the same turn (the book lives world-side). Empty at hash
/// time like a read mailbox; tagged against adjacent-mail aliasing.
#[derive(Default)]
pub(crate) struct Mc2StealMail(pub Vec<(u16, u8)>);

impl std::hash::Hash for Mc2StealMail {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if !self.0.is_empty() {
            state.write_u8(5);
            self.0.hash(state);
        }
    }
}

/// See [`Gen::mc2_spawn_ord`] — hashes to NOTHING while all-zero
/// (hash-transparent).
#[derive(Default)]
pub(crate) struct Mc2Ord(pub [u8; 32]);

impl std::hash::Hash for Mc2Ord {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0.iter().any(|&v| v != 0) {
            state.write(&self.0);
        }
    }
}

/// Per-color MC2 Life scalar (`word_0x24A_586` — the wizard-HP AND
/// castle-HP factor, EF:43768/61695). Default 256 = 1.0x for every
/// color; hashes to NOTHING while all-default (the [`Mc2Ord`]
/// pattern).
pub(crate) struct Mc2LifeScale(pub [u16; 8]);

impl Default for Mc2LifeScale {
    fn default() -> Self {
        Mc2LifeScale([256; 8])
    }
}

impl std::hash::Hash for Mc2LifeScale {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0 != [256; 8] {
            self.0.hash(state);
        }
    }
}

/// MC2's recycle-victim stack (`D41A0_0.dword_0x11EA` cells, top
/// `dword_0x11e6`; −1 = empty): LIVE but expendable entities the
/// allocator sacrifices when the free stack is dry.
///
/// `sub_49F90` (Level.cpp:1271-1302) rebuilds it from scratch by a
/// DESCENDING pool scan 999→1, pushing every live record whose
/// `struct_byte_0xc_12_15.byte[2] & 2` (our `flags & 0x2_0000`) is
/// set — so the stack TOP is the LOWEST-numbered victim and pops
/// climb. `stack` mirrors it bottom-up: the LAST element pops first.
///
/// `refill` = "rebuild the list on demand when it runs dry" — the
/// NATIVE arm. Retail refreshes at `sub_49F90`'s own call sites (level
/// generate EF:39396, the mid-game arms EF:60101/61278 — the last of
/// which is literally "free stack empty ⇒ rebuild ⇒ retry"), a cadence
/// the port does not model; the strict-conformance import instead
/// hands over the RECORDED snapshot with `refill` clear, so replay
/// sacrifices exactly the victims retail had ranked and fails exactly
/// where retail's list ran out.
///
/// Hash-quiet while empty (the [`Mc2Ord`] pattern): MC1 and every
/// never-full MC2 run hash exactly as they did before the field.
#[derive(Default)]
pub(crate) struct Mc2Recycle {
    pub(crate) stack: Vec<u16>,
    pub(crate) refill: bool,
    /// Victims seized so far (the `exhausted` counter's twin —
    /// observability, not behavior; the original keeps no such count).
    pub(crate) seized: u32,
}

impl std::hash::Hash for Mc2Recycle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // `refill` is a MODE and `seized` telemetry — hash-silent.
        if !self.stack.is_empty() {
            self.stack.hash(state);
        }
    }
}

/// A counter that hashes to NOTHING at zero (see [`Mc2Ord`]). The
/// const TAG (unique per field) disambiguates ADJACENT quiet fields:
/// without it, (drain=5, scrolls=0) and (drain=0, scrolls=5) feed
/// identical byte streams (the conditional-hash aliasing class).
/// Written INSIDE the condition, so zero fields contribute nothing.
#[derive(Default)]
pub(crate) struct Mc2Quiet<const TAG: u8>(pub i32);

impl<const TAG: u8> std::hash::Hash for Mc2Quiet<TAG> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0 != 0 {
            state.write_u8(TAG);
            state.write_i32(self.0);
        }
    }
}

/// A slot-keyed side-channel that hashes to NOTHING while empty
/// (hash-transparent) and contributes deterministically (BTreeMap
/// order) once entries exist. Carries per-entity words that have no
/// `Ent` field home — adding a field to `Ent` would move EVERY
/// golden's hash stream. The const TAG (unique per field) keeps
/// adjacent slot-maps from aliasing (aura_claim={a} + wanted={} vs
/// its mirror); written only when non-empty, so empty maps stay
/// transparent.
#[derive(Default)]
pub(crate) struct Mc2SlotMap<const TAG: u8>(pub std::collections::BTreeMap<u16, u16>);

/// The per-owner castle-guard register (see [`Gen::mc1_guard_reg`]).
/// Hash-transparent while no register holds a nonzero entry, so every
/// pre-guard-era golden stands.
#[derive(Default, Clone, Debug, PartialEq)]
pub(crate) struct Mc1GuardReg(pub std::collections::BTreeMap<u16, Vec<u16>>);

impl std::hash::Hash for Mc1GuardReg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (owner, reg) in &self.0 {
            if reg.iter().all(|&s| s == 0) {
                continue;
            }
            state.write_u8(0x84);
            state.write_u16(*owner);
            for &s in reg {
                state.write_u16(s);
            }
        }
    }
}

/// `MGC_NO_BALLOON_REG=1` drops the fleet dispatcher back to the
/// live-census stand-in (empty register + the adoption pass = the old
/// ascending-slot walk, and the imported wizext+52 order ignored) —
/// the A/B arm for the register law, so one binary measures both.
/// Read once: a whole-process arm, never a per-run input.
fn no_balloon_reg() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_BALLOON_REG").is_some())
}

/// The per-owner mana-balloon register (see [`Gen::mc1_balloon_reg`]).
/// HASH-SILENT ALWAYS: the register is pure spawn-order bookkeeping
/// over a membership the hashed pool already carries, and its whole
/// behavioural output — which balloon holds which ball (+146), which
/// one the cull frees (+400) — lands in hashed entity fields, so a
/// golden still catches every divergence it can cause.
#[derive(Default, Clone, Debug, PartialEq)]
pub(crate) struct Mc1BalloonReg(pub std::collections::BTreeMap<u16, Vec<u16>>);

impl std::hash::Hash for Mc1BalloonReg {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

impl<const TAG: u8> std::hash::Hash for Mc2SlotMap<TAG> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if !self.0.is_empty() {
            state.write_u8(TAG);
        }
        for (k, v) in &self.0 {
            state.write_u16(*k);
            state.write_u16(*v);
        }
    }
}

/// See [`Gen::player_spin`] — the pending forced heading delta on the
/// flyer, hash-TRANSPARENT at rest so no pre-tornado golden moves for
/// carrying it. Live only inside a funnel, and drained by the mover
/// every tick, so it is deliberately NOT snapshotted: a save taken
/// mid-tornado reloads owing at most one tick of turn, the same call
/// the carpet echoes make.
#[derive(Default, Clone, Copy)]
pub(crate) struct PlayerSpin(pub i16);

impl std::hash::Hash for PlayerSpin {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0 != 0 {
            state.write_u8(0x5B);
            state.write_i16(self.0);
        }
    }
}

/// See [`Gen::mc2_night_shade`] — a bool that hashes to NOTHING when
/// false (hash-transparent).
#[derive(Default)]
pub(crate) struct NightShade(pub bool);

impl std::hash::Hash for NightShade {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0 {
            state.write_u8(1);
        }
    }
}

/// Per-slot spawn generations ([`Gen::slot_gen`]) — bumped every time
/// `new_event` hands the slot out, so presentation can tell two
/// occupants of the same slot apart across tick snapshots (the render
/// interpolation identity guard; the balloon stale-slot class).
/// PRESENTATION-ONLY: never read by any sim rule, so the Hash is a
/// no-op UNCONDITIONALLY — unlike the quiet counters above it stays
/// silent even when populated.
#[derive(Default)]
pub(crate) struct SlotGens(pub Vec<u32>);

impl std::hash::Hash for SlotGens {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// The tick-start mana-ball chain — retail's `var_u32_36462[1]`
/// roster, rebuilt from a single ascending slot sweep at the TOP of
/// every tick (:52246-312, the same pass that counts the trigger
/// buckets) and holding every class-10 model-39/40 record live at
/// that moment. Chain WALKERS (the (10,54) magnet stamp sub_29920
/// :31247, the castle absorb :56024, …) see THIS list, not the live
/// pool: an entity spawned mid-walk is invisible to every chain
/// consumer until the next tick's rebuild (measured: the castle
/// death's ejected ball gets its first magnet ch4 stamp one tick
/// AFTER the teardown, mc1l0 t=1831→1832). Derived per tick —
/// hash-silent like [`SlotGens`].
#[derive(Default)]
pub(crate) struct TickChain {
    pub list: Vec<u16>,
    /// THE SEVERED CHAIN (ledger §THE SEVERED BALL CHAIN): retail's
    /// tick-head lists are singly linked THROUGH the entity records,
    /// so a freed record REUSED mid-tick (the NewEvent ctor wipe —
    /// a plain free keeps the link, the freed-slot stale-bytes law)
    /// severs the chain at that node: every walk later in the tick
    /// sees the prefix, the reused node itself (with its NEW bytes),
    /// and nothing beyond. `cut` = visible member count from the
    /// walk head; `usize::MAX` = intact. Reset at the tick-top
    /// rebuild, lowered only by [`Gen::new_event`].
    pub cut: usize,
}

impl TickChain {
    /// The member prefix a retail chain walk reaches this tick.
    pub fn visible_len(&self) -> usize {
        self.list.len().min(self.cut)
    }
}

impl std::hash::Hash for TickChain {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// The per-model CLASS-5 roster chains (heads at wizext-file 36382 +
/// 4·model), rebuilt by the tick-top sweep in ascending slot order
/// with retail's membership sample — `act ≥ 0 ∧ state ≠ 120` at TICK
/// TOP (CARPET.EXE, the rebuild after the reap; heads verified at
/// 36382 against the binary — the lift's 36462 head-write was a
/// transcription bug). A creature promoted or killed MID-tick keeps
/// its tick-top membership until the next rebuild: mc1l1 t=4130's
/// fireball muzzle acquire cannot see the segment the castle crush
/// just promoted to a corpse state, because the chain was built while
/// it was still state 120. Chain walks read LIVE fields off the
/// members; only MEMBERSHIP (and order) is the snapshot. Derived per
/// tick — hash-silent like [`TickChain`], never saved.
#[derive(Default)]
pub(crate) struct MobChains {
    pub list: Vec<Vec<u16>>,
    /// Per-model severed-chain cut, the [`TickChain::cut`] law: a
    /// NewEvent REUSE of a chained slot wipes its +0 link and every
    /// later walk stops there. `usize::MAX` = intact.
    pub cut: Vec<usize>,
}

impl MobChains {
    pub fn reset(&mut self, models: usize) {
        self.list.resize(models, Vec::new());
        self.cut.resize(models, usize::MAX);
        for v in &mut self.list {
            v.clear();
        }
        for c in &mut self.cut {
            *c = usize::MAX;
        }
    }
    /// The member prefix a retail walk of model `m`'s chain reaches.
    pub fn visible(&self, m: usize) -> &[u16] {
        match self.list.get(m) {
            Some(v) => &v[..v.len().min(self.cut[m])],
            None => &[],
        }
    }
}

impl std::hash::Hash for MobChains {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

impl Gen {
    /// The tick-top [`MobChains`] rebuild, callable standalone for
    /// tests that drive an acquire/chain consumer without running a
    /// full `World::tick` (the live tick builds the chains inside its
    /// top sweep — world.rs:2680, its only non-test twin).
    #[cfg(test)]
    pub(crate) fn rebuild_mob_chains(&mut self) {
        self.mob_chains.reset(20);
        for s in 1..self.ent.len() {
            let e = &self.ent[s];
            if e.class64 == 5 && e.act_life >= 0 && e.tick70 != 120 && (e.model65 as usize) < 20 {
                self.mob_chains.list[e.model65 as usize].push(s as u16);
            }
        }
    }

    /// The mana-ball arm of the same tick-top sweep (:52290-97) —
    /// the twin of [`Self::rebuild_mob_chains`] for tests that drive a
    /// bare `Gen` instead of `World::tick` (whose top sweep at
    /// world.rs:2702 is the only non-test builder).
    #[cfg(test)]
    pub(crate) fn rebuild_ball_chain(&mut self) {
        self.ball_chain.list.clear();
        self.ball_chain.cut = usize::MAX;
        for s in 1..self.ent.len() {
            let e = &self.ent[s];
            if e.class64 == 10 && matches!(e.model65, 39 | 40) {
                self.ball_chain.list.push(s as u16);
            }
        }
    }
}

/// One sound request: engine sound id (the SNDS bank-0 index), the
/// emitter's position on the u16 torus, and its slot as the instance
/// tag (the original's entity+24). `player` marks requests the
/// original issued against the player's own entity (full volume,
/// center pan, and the gate for the player-only ids 4/14/17/29).
#[derive(Debug, Clone, Copy, Hash)]
pub struct SoundEvent {
    pub id: u8,
    pub pos: (u16, u16, i16),
    pub tag: u16,
    pub player: bool,
}

/// Build the 1-based runtime THING table. `base` maps the package's
/// 0-based `slot` export into engine slots: MC1's 1999-record file
/// is engine slots 1..=1999 (base 1); MC2's 1200-record file IS the
/// engine table including the unused slot 0 (base 0) — its stage
/// checkpoints reference these slots directly (remc2
/// entity_0x30311[stage_1]).
pub(crate) fn build_table(things: &[Thing], slots: usize, base: usize) -> Vec<Rec> {
    let mut table = vec![Rec::default(); slots];
    for th in things {
        let i = th.slot as usize + base;
        if i < table.len() {
            table[i] = Rec {
                class: th.class,
                model: th.model,
                x: th.x,
                y: th.y,
                dis_id: th.dis_id,
                swi_sz: th.swi_sz,
                swi_id: th.swi_id,
                parent: th.parent,
                child: th.child,
                par3: th.par3.unwrap_or(0),
            };
        }
    }
    table
}

impl Gen {
    /// A fresh engine over owned planes. `seed` = the level's GEN_MAP
    /// seed (`rand_4`); the retile `pseudoRand` stream is replayed from
    /// the pristine height plane.
    pub(crate) fn new(
        t: Planes,
        assets: FeatureAssets,
        seed: u32,
        chassis: ChassisParams,
        verbs: VerbSet,
    ) -> Self {
        let pseudo = post_generation_pseudo_rand(&t.height);
        Gen {
            t,
            assets,
            retile: corners::retile_table(),
            map_entity: vec![0; GRID],
            ent: vec![Ent::default(); chassis.pool_slots],
            slot_gen: SlotGens(vec![0; chassis.pool_slots]),
            free: (1..chassis.pool_slots as u16).rev().collect(),
            ball_chain: TickChain::default(),
            mob_chains: MobChains::default(),
            mc2_recycle: Mc2Recycle::default(),
            mc1_guard_reg: Mc1GuardReg::default(),
            mc1_balloon_reg: Mc1BalloonReg::default(),
            rand: seed,
            pseudo,
            spawn_count: [0; 20],
            player_mail: [(0, 0); 6],
            player_damage: 0,
            erupting: 0,
            plume: 0,
            player_knock: (0, 0),
            player_spin: PlayerSpin::default(),
            mc2_debuffs: Mc2PlayerDebuffs::default(),
            rival_ents: [0; 8],
            mc2_life_scale: Mc2LifeScale::default(),
            player_aggro: 0,
            rival_wanted: [0; 8],
            player_invisible: false,
            player_rebound: false,
            kills: 0,
            shots: 0,
            hits: 0,
            player_danger: 0,
            banked_houses: 0,
            castle_alert: 0,
            player_alert: 0,
            balloon_alert: 0,
            pal_flash: PalFlash::default(),
            exhausted: 0,
            sounds: Vec::new(),
            terrain_dirty: false,
            chassis,
            verbs,
            verb_fallbacks: 0,
            misfits: Vec::new(),
            mc2_night_shade: NightShade(false),
            mc2_spawn_ord: Mc2Ord::default(),
            mc2_player_drain: Mc2Quiet::default(),
            mc2_scrolls: Mc2Quiet::default(),
            mc2_spell_tokens: Mc2Quiet::default(),
            mc2_cast_xp: Mc2XpMail::default(),
            bolt_fx: BoltFx::default(),
            mc2_steal_mail: Mc2StealMail::default(),
            mc2_aura_claim: Mc2SlotMap::default(),
            mc2_wanted: Mc2SlotMap::default(),
            mc2_allied: Mc2SlotMap::default(),
            mc2_rebound_precise: Mc2Quiet::default(),
            mc2_castle_research: Mc2CastleResearch::default(),
        }
    }

    /// Note that `kind`'s requested arm is pending and the MC1
    /// implementation served instead (once per verb per world).
    pub(crate) fn note_verb_fallback(&mut self, kind: VerbKind) {
        self.verb_fallbacks |= 1 << kind as u8;
    }

    /// The spawn seam refused an unknown `(class, model)` — count it.
    pub(crate) fn note_misfit(&mut self, class: u16, model: u16) {
        if let Some(m) = self
            .misfits
            .iter_mut()
            .find(|m| m.0 == class && m.1 == model)
        {
            m.2 += 1;
        } else {
            self.misfits.push((class, model, 1));
        }
    }

    /// Emit a sound request from entity `i` (its position and slot
    /// become the request's position and instance tag).
    pub(crate) fn snd(&mut self, id: u8, i: usize) {
        let e = &self.ent[i];
        self.sounds.push(SoundEvent {
            id,
            pos: (e.x, e.y, e.z),
            tag: i as u16,
            player: false,
        });
    }

    /// Emit a player-entity sound request (the original's calls
    /// against the wizard's own entity — full volume, center pan).
    pub(crate) fn snd_player(&mut self, id: u8) {
        self.sounds.push(SoundEvent {
            id,
            pos: (0, 0, 0),
            tag: crate::mc1::mobs::PLAYER_TARGET,
            player: true,
        });
    }

    /// GenerateFeatures_36430: consume the class-10 load-time features
    /// (dis_id 0xFFFF) in slot order and run the fixpoint event loop.
    pub(crate) fn load_time_pass(&mut self, table: &mut [Rec]) {
        for i in 1..table.len() {
            if table[i].dis_id == 0xFFFF && table[i].class == 10 {
                self.dispatch(table, i);
                table[i].class = 0;
            }
        }
        self.event_loop();
    }
}

/// Apply MC1's load-time terrain features.
///
/// `seed` is the level's GEN_MAP seed (`rand_4` is loaded from it and
/// nothing before GenerateFeatures advances it); pass 0 if unknown —
/// only dither variety is affected, not feature placement.
pub fn generate_features_mc1(
    planes: TerrainPlanes<'_>,
    things: &[Thing],
    seed: u32,
    assets: &FeatureAssets,
) {
    let mut table = build_table(things, ChassisParams::MC1.level_table_slots, 1);
    let owned = Planes {
        height: planes.height.to_vec(),
        tile_type: planes.tile_type.to_vec(),
        shading: planes.shading.to_vec(),
        angle: planes.angle.to_vec(),
        ceiling: Vec::new(),
    };
    let mut g = Gen::new(
        owned,
        assets.clone(),
        seed,
        ChassisParams::MC1,
        VerbSet::MC1,
    );
    g.load_time_pass(&mut table);
    planes.height.copy_from_slice(&g.t.height);
    planes.tile_type.copy_from_slice(&g.t.tile_type);
    planes.shading.copy_from_slice(&g.t.shading);
    planes.angle.copy_from_slice(&g.t.angle);
}

impl Gen {
    // ---- pool primitives ------------------------------------------------

    /// NewEvent_372C0 (:43865). Seeds the per-entity LCG from the
    /// global stream WITHOUT advancing it. Defaults per the original:
    /// life 300, flags 8, +126 = 16, +44 = 100, +24 = own slot,
    /// +58 = 0xFA, +66 = +67 = 0xFF, +68 = 10 (:43879), +156 = row 0.
    pub(crate) fn new_event(&mut self) -> Option<usize> {
        // MC2 pops the free stack FIRST and only then sacrifices a
        // recycle victim (`NewEvent_4A050`, Events.cpp:561-608 — the
        // free arm at :563, the victim arm at :581). MC1's
        // `NewEvent_372C0` has the same two-stack shape (:43900-10),
        // but its recycle stack is only ever populated inside the
        // respawn window (`sub_44D30` rebuilds both lists at :54842
        // and empties the victims again at :55056) — normal play
        // allocates from `free` alone, as here. The port skips the
        // respawn-window sacrifice; `World::death_regrant` covers
        // the consequence (docs/DEVIATIONS.md).
        let idx = match self.free.pop().or_else(|| self.mc2_recycle_pop()) {
            Some(i) => i,
            None => {
                // Fail-open like the original (alloc returns null, the
                // spawn silently vanishes — map 032's starved trigger),
                // but COUNTED: the limit-removing register (ROADMAP
                // "MULTI-GAME ARCHITECTURE") wants a playtest catalogue
                // of the levels that hit the pool ceiling before any
                // bumped-pool option exists.
                self.exhausted = self.exhausted.saturating_add(1);
                return None;
            }
        };
        let idx = idx as usize;
        // THE SEVERED CHAIN: reusing a freed record wipes its list
        // link, so retail walks of the tick-head ball chain stop at
        // this node for the rest of the tick. Measured: mc1l0 pair
        // 604→605 — the (9,1) lob reusing collected ball 642's slot
        // must not see balls 643+ (chase want 104, not the
        // closer-scoring 714 its own predecessor lob chases).
        if let Ok(pos) = self.ball_chain.list.binary_search(&(idx as u16)) {
            self.ball_chain.cut = self.ball_chain.cut.min(pos + 1);
        }
        // The same severed-chain law for the per-model class-5 roster
        // chains ([`MobChains`]): the memset below wipes +0, so any
        // chain this slot was a tick-top member of stops here for the
        // rest of the tick.
        for m in 0..self.mob_chains.list.len() {
            if let Ok(pos) = self.mob_chains.list[m].binary_search(&(idx as u16)) {
                self.mob_chains.cut[m] = self.mob_chains.cut[m].min(pos + 1);
            }
        }
        // A reallocated slot must leave any tile chain BEFORE its
        // record resets — a stale linked record (an imported ghost,
        // or any future free path that forgets) would otherwise leave
        // a dangling chain pointer, and the chain walk cycles once
        // the slot relinks on the same tile (unbounded victim lists).
        if self.ent[idx].flags & 4 != 0 {
            self.unlink(idx);
        }
        // New occupant → new presentation generation (hash-silent).
        self.slot_gen.0[idx] = self.slot_gen.0[idx].wrapping_add(1);
        // The aura claim lives ON the entity in retail — slot reuse
        // resets it with every other field (no stale claim may greet
        // the slot's next occupant).
        self.mc2_aura_claim.0.remove(&(idx as u16));
        let e = &mut self.ent[idx];
        *e = Ent::default();
        e.max_life = 300;
        e.flags = 8;
        e.f126 = 16;
        e.f44 = 100;
        e.f68 = 10;
        e.id24 = idx as u16;
        e.f58 = 0xFA;
        e.f66 = 0xFF;
        e.f67 = 0xFF;
        e.rand = match self.chassis.ent_rand_width {
            RandWidth::U32 => (idx as u32).wrapping_add(self.rand),
            RandWidth::U16 => (idx as u32).wrapping_add(self.rand) & 0xFFFF,
        };
        e.f63 = idx as u8;
        Some(idx)
    }

    /// The MC2 allocator's fallback (`NewEvent_4A050` :581-605): with
    /// the free stack dry, SACRIFICE the top-ranked recycle victim.
    ///
    /// Retail's arm is a bare seizure, NOT a death — `SetMapEntity_57E50`
    /// (tile unlink), `class = 0`, then the same 168-byte memset +
    /// defaults the free arm runs. No damage, no kill credit, no corpse,
    /// no parent notify, and the slot never visits the free stack. Our
    /// caller performs exactly that teardown (`unlink` on the link bit,
    /// then `Ent::default()`), so this only has to choose the slot.
    ///
    /// Cells that are no longer live victims are skipped rather than
    /// seized: retail's `sub_57F20` pulls a dying victim out of the
    /// stack (see [`Gen::free_entity`]), but an IMPORTED snapshot can
    /// still name a slot the port has since freed.
    fn mc2_recycle_pop(&mut self) -> Option<u16> {
        let mut refilled = false;
        loop {
            while let Some(s) = self.mc2_recycle.stack.pop() {
                let Some(e) = self.ent.get(s as usize) else {
                    continue;
                };
                if s != 0 && e.class64 != 0 {
                    self.mc2_recycle.seized = self.mc2_recycle.seized.saturating_add(1);
                    return Some(s);
                }
            }
            // Retail's own "free stack empty ⇒ `sub_49F90` ⇒ retry"
            // idiom (EF:61275-79), moved to the allocator because the
            // port does not model the refresh call sites. OFF under
            // the strict-conformance import, whose stack is retail's
            // recorded snapshot — running dry there is retail running
            // dry. Terminates: each seizure clears the victim's
            // sacrificable bit (the record is wiped), so a rebuild
            // scan is strictly shorter every time.
            if refilled || !self.mc2_recycle.refill {
                return None;
            }
            refilled = true;
            self.rebuild_recycle(0x2_0000);
        }
    }

    /// `sub_49F90`'s FREE half (Level.cpp:1294-1300): the same
    /// DESCENDING 999→1 scan pushes every class-0 record, so the stack
    /// TOP — the next allocation — is the LOWEST free slot.
    ///
    /// The port maintains its free stack incrementally, which is right
    /// for ordinary play; this exists for retail's own explicit
    /// rebuild call sites. The MC2 death payout (EF:60101), the
    /// respawn (EF:43635) and EVERY disposition fire in both engines
    /// (`World::fire_disposition` — sub_37440 :43960 / sub_4A1E0
    /// EF:32966, MC1's scan twin is sub_37220 :43825) are such sites,
    /// and the rebuild is OBSERVABLE there: mc2l3's graves land on
    /// slots 3 and 1, the respawn's 26 re-minted spell tokens take 99,
    /// 100, 102… in spell order, and mc1l1's t=344 trigger fire parks
    /// its chained (11,1) on slot 41 — the lowest free slots, not the
    /// incremental stack's order.
    ///
    /// Retail's reap half (`byte[1] & 4` → `sub_57F20`) is deliberately
    /// NOT mirrored: strict MC2 keeps disabled records through the
    /// frame (the ghost-record projection law, `retail_import_mc2`),
    /// and `tick()`'s top reap is the one pusher.
    ///
    /// `pinned` is the conformance import's human-carpet slot, whose
    /// record is a zeroed husk in our pool but a LIVE wizard in
    /// retail's — it must never be handed out (0 = native MC2, where
    /// the human owns no pool slot at all).
    pub(crate) fn mc2_rebuild_free(&mut self, pinned: u16) {
        self.free = (1..self.ent.len() as u16)
            .rev()
            .filter(|&s| s != pinned && self.ent[s as usize].class64 == 0)
            .collect();
    }

    /// `sub_49F90`'s victim half (Level.cpp:1284-1301): a DESCENDING
    /// 999→1 pool scan pushing every live record whose flags meet
    /// `mask`, so the stack top — the next sacrifice — is the
    /// LOWEST-numbered victim. MC2 admits the sacrificable bit alone
    /// (`byte[2] & 2` = `0x2_0000`); MC1's twin `sub_37220` (:43841)
    /// admits the disable bit too (`0x20400`).
    ///
    /// The free half of that rebuild is deliberately not mirrored: our
    /// free stack is maintained incrementally and never leaks a
    /// class-0 slot, and this runs only with the pool FULL, where
    /// retail's own scan finds no free record either.
    pub(crate) fn rebuild_recycle(&mut self, mask: u32) {
        self.mc2_recycle.stack = (1..self.ent.len() as u16)
            .rev()
            .filter(|&s| {
                let e = &self.ent[s as usize];
                e.class64 != 0 && e.flags & mask != 0
            })
            .collect();
    }

    /// One draw of this event's own LCG (`rand_29799_4`, the stream
    /// every spawn/behavior handler rolls).
    pub(crate) fn ent_rand(&mut self, i: usize) -> u32 {
        match self.chassis.ent_rand_width {
            RandWidth::U32 => lcg32(&mut self.ent[i].rand),
            RandWidth::U16 => {
                let r = self.ent[i].rand.wrapping_mul(9377).wrapping_add(9439) & 0xFFFF;
                self.ent[i].rand = r;
                r
            }
        }
    }

    /// `sub_41CC0_42000` (:52460) / `sub_57D40` (EF:40306) — UNLINK +
    /// LINK at the entity's OWN position. Nothing moves; the entity
    /// simply becomes the HEAD of its tile chain.
    ///
    /// ⭐ That is a PAINT-ORDER operation. The sprite pass walks the
    /// tile chain head→tail and is a pure painter with no z-buffer at
    /// all, so the member drawn LAST — the tail, i.e. the one that has
    /// been linked longest — ends up ON TOP. Re-heading an entity
    /// therefore pushes it BEHIND everything already sharing its tile.
    /// Neither existing primitive can express it: [`Gen::move_relink`]
    /// no-ops within a tile, and `link` early-returns on the link bit.
    ///
    /// Both games call it from exactly one place — the tree IGNITION
    /// block (:57698 / EF:62443), re-heading the tree one instruction
    /// after the flame was head-linked, so the flame paints after it =
    /// in front of it.
    pub(crate) fn relink_head(&mut self, i: usize) {
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        self.unlink(i);
        self.link(i, x, y, z);
    }

    /// sub_41CF0 (:52468): link into the per-tile list and set position.
    pub(crate) fn link(&mut self, i: usize, x: u16, y: u16, z: i16) {
        if self.ent[i].flags & 4 != 0 {
            return;
        }
        let t = tile((x >> 8) as u8, (y >> 8) as u8);
        self.ent[i].prev22 = 0;
        self.ent[i].next20 = self.map_entity[t];
        let head = self.map_entity[t] as usize;
        if head != 0 {
            self.ent[head].prev22 = i as u16;
        }
        self.map_entity[t] = i as u16;
        let e = &mut self.ent[i];
        e.x = x;
        e.y = y;
        e.z = z;
        e.flags |= 4;
    }

    /// sub_41DD0 (:52486).
    pub(crate) fn unlink(&mut self, i: usize) {
        if self.ent[i].flags & 4 == 0 {
            return;
        }
        let (next, prev) = (self.ent[i].next20, self.ent[i].prev22);
        if prev != 0 {
            self.ent[prev as usize].next20 = next;
        } else {
            let t = tile((self.ent[i].x >> 8) as u8, (self.ent[i].y >> 8) as u8);
            self.map_entity[t] = next;
        }
        if next != 0 {
            self.ent[next as usize].prev22 = prev;
        }
        self.ent[i].flags &= !4;
    }

    /// sub_41C70 (:52442): move, relinking only across tiles.
    pub(crate) fn move_relink(&mut self, i: usize, x: u16, y: u16, z: i16) {
        let e = &self.ent[i];
        if e.x >> 8 == x >> 8 && e.y >> 8 == y >> 8 {
            let e = &mut self.ent[i];
            e.x = x;
            e.y = y;
            e.z = z;
        } else {
            self.unlink(i);
            self.link(i, x, y, z);
        }
    }

    /// sub_41E90 (:52514): unlink, clear, return the slot (LIFO).
    /// MC2's twin `sub_57F20` (Events.cpp:5209-39) adds one step
    /// between the unlink and the class clear: a sacrificable entity
    /// that dies normally must LEAVE the recycle stack, or the
    /// allocator would later seize a slot that is already free (a
    /// double allocation of one slot). Retail's removal is a linear
    /// search then a swap-with-top (:5232), which does NOT preserve
    /// the ranking below the hole — mirrored exactly.
    pub(crate) fn free_entity(&mut self, i: usize) {
        self.unlink(i);
        if self.ent[i].flags & 0x2_0000 != 0 && !self.mc2_recycle.stack.is_empty() {
            if let Some(at) = self.mc2_recycle.stack.iter().position(|&s| s as usize == i) {
                self.mc2_recycle.stack.swap_remove(at);
            }
        }
        self.ent[i].class64 = 0;
        self.free.push(i as u16);
    }

    // ---- terrain helpers ------------------------------------------------

    /// sub_724C0 (:81516): ground height at an 8.8 position,
    /// interpolated across the tile's two triangles, in engine units
    /// (one height byte = 32).
    pub(crate) fn ground_z(&self, x: u16, y: u16) -> i32 {
        Self::interp_plane(&self.t.height, x, y)
    }

    /// `sub_10C60` → `sub_B5D68` (remc2 Terrain.cpp:2158-2164): the
    /// CAVE CEILING altitude — the exact same bilinear ×32 sampler as
    /// the floor's, reading the second heightmap. Callers must be
    /// cave-gated (the plane is empty off-cave; retail's array is
    /// all-zeros there and every retail call site is cave-gated too).
    pub(crate) fn ceiling_z(&self, x: u16, y: u16) -> i32 {
        Self::interp_plane(&self.t.ceiling, x, y)
    }

    pub(crate) fn interp_plane(plane: &[u8], x: u16, y: u16) -> i32 {
        let h = |dx: u8, dy: u8| plane[tile(dx, dy)] as i32;
        let (cx, cy) = ((x >> 8) as u8, (y >> 8) as u8);
        let (fx, fy) = ((x & 0xFF) as i32, (y & 0xFF) as i32);
        let (p1, comp);
        if cx.wrapping_add(cy) & 1 == 1 {
            if fx + fy > 255 {
                p1 = h(cx, cy.wrapping_add(1));
                let p2 = h(cx.wrapping_add(1), cy.wrapping_add(1));
                comp = (255 - fy) * (h(cx.wrapping_add(1), cy) - p2) + fx * (p2 - p1);
            } else {
                p1 = h(cx, cy);
                let p2 = h(cx.wrapping_add(1), cy);
                comp = fy * (h(cx, cy.wrapping_add(1)) - p1) + fx * (p2 - p1);
            }
        } else if fx <= fy {
            p1 = h(cx, cy);
            let p2 = h(cx, cy.wrapping_add(1));
            comp = fy * (p2 - p1) + fx * (h(cx.wrapping_add(1), cy.wrapping_add(1)) - p2);
        } else {
            p1 = h(cx, cy);
            let p2 = h(cx.wrapping_add(1), cy);
            comp = fy * (h(cx.wrapping_add(1), cy.wrapping_add(1)) - p2) + fx * (p2 - p1);
        }
        (comp >> 3) + 32 * p1
    }

    /// sub_361C0 (:42956): average of the four footprint corners
    /// (x, y), (x+w, y), (x+w, y+h), (x, y+h), u8-wrapping.
    pub(crate) fn avg4(&self, x: u8, y: u8, h: u8, w: u8) -> u16 {
        let p1 = self.t.height[tile(x, y)] as u16;
        let p2 = self.t.height[tile(x.wrapping_add(w), y)] as u16;
        let p3 = self.t.height[tile(x.wrapping_add(w), y.wrapping_add(h))] as u16;
        let p4 = self.t.height[tile(x, y.wrapping_add(h))] as u16;
        (p1 + p2 + p3 + p4) >> 2
    }

    /// The shared passes 2+3 of the retexture helpers (sub_33B90 /
    /// sub_33E10, :41165/:41288): retile every type-1 cell of the rect
    /// grown by one on the -x/-y side through the `byte_B5D40` table
    /// (drawing pseudoRand for types < 8), then recompute shading over
    /// the rect grown once more.
    pub(crate) fn retile_and_shade(&mut self, ax: u8, ay: u8, bx: u8, by: u8) {
        let x_add = bx.wrapping_sub(ax).wrapping_add(2);
        let y_add = by.wrapping_sub(ay).wrapping_add(2);
        let (sx, sy) = (ax.wrapping_sub(1), ay.wrapping_sub(1));
        let mut cy = sy;
        for _ in 0..y_add {
            let mut cx = sx;
            for _ in 0..x_add {
                let t = tile(cx, cy);
                if self.t.tile_type[t] == 1 {
                    let p1 = self.t.angle[t] & 7;
                    let p2 = self.t.angle[tile(cx.wrapping_add(1), cy)] & 7;
                    let p3 = self.t.angle[tile(cx.wrapping_add(1), cy.wrapping_add(1))] & 7;
                    let p4 = self.t.angle[tile(cx, cy.wrapping_add(1))] & 7;
                    let idx = p4 as usize + 7 * p3 as usize + 49 * p2 as usize + 343 * p1 as usize;
                    let [new_type, orient] = self.retile[idx];
                    self.t.tile_type[t] = new_type;
                    self.t.angle[t] = if new_type >= 8 {
                        orient.wrapping_add(self.t.angle[t] & 0x87)
                    } else {
                        self.pseudo = self.pseudo.wrapping_mul(9377).wrapping_add(9439);
                        (self.t.angle[t] & 0x87).wrapping_add(16 * (self.pseudo % 7) as u8)
                    };
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
        // Pass 3: shading over the rect grown once more (3x3 for a
        // single cell). shade = NW height - SE height + 32, as signed
        // char; clamp <28 → (s&3)+28, >40 → (s&7)+40; clear angle bit 3.
        // MC2's twin (`sub_462A0`/`46570`) adds two DATA-variant arms,
        // both no-ops on MC1 worlds: the non-Day shade inversion
        // (Terrain.cpp:2030-2033, [`Gen::mc2_night_shade`]) and the
        // cave floor↔ceiling invariant instead of the blind bit3
        // clear (Terrain.cpp:2034-2042).
        let mut cy = sy;
        for _ in 0..y_add.wrapping_add(1) {
            let mut cx = sx;
            for _ in 0..x_add.wrapping_add(1) {
                let t = tile(cx, cy);
                let se = self.t.height[tile(cx.wrapping_add(1), cy.wrapping_add(1))];
                let nw = self.t.height[tile(cx.wrapping_sub(1), cy.wrapping_sub(1))];
                let mut s = nw.wrapping_sub(se).wrapping_add(32);
                if (s as i8) < 28 {
                    s = (s & 3) + 28;
                } else if (s as i8) > 40 {
                    s = (s & 7) + 40;
                }
                self.t.shading[t] = if self.mc2_night_shade.0 {
                    64u8.wrapping_sub(s)
                } else {
                    s
                };
                if self.is_cave() {
                    self.cave_seal_fixup(t);
                } else {
                    self.t.angle[t] &= 0xF7;
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
    }

    /// sub_33B90 (:41165), "flag mode": stencil type 1 onto each rect
    /// cell + its W/NW/N neighbors where not building-protected (bit 7),
    /// then retile + shade.
    fn recompute_protected(&mut self, ax: u8, ay: u8, bx: u8, by: u8) {
        let (w, h) = (
            bx.wrapping_sub(ax).wrapping_add(1),
            by.wrapping_sub(ay).wrapping_add(1),
        );
        let mut cy = ay;
        for _ in 0..h {
            let mut cx = ax;
            for _ in 0..w {
                for t in [
                    tile(cx, cy),
                    tile(cx.wrapping_sub(1), cy),
                    tile(cx.wrapping_sub(1), cy.wrapping_sub(1)),
                    tile(cx, cy.wrapping_sub(1)),
                ] {
                    if self.t.angle[t] & 0x80 == 0 {
                        self.t.tile_type[t] = 1;
                    }
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
        self.retile_and_shade(ax, ay, bx, by);
    }

    /// sub_33E10 (:41288), "dig mode": same but the stencil ignores the
    /// protection bit.
    fn recompute_unprotected(&mut self, ax: u8, ay: u8, bx: u8, by: u8) {
        let (w, h) = (
            bx.wrapping_sub(ax).wrapping_add(1),
            by.wrapping_sub(ay).wrapping_add(1),
        );
        let mut cy = ay;
        for _ in 0..h {
            let mut cx = ax;
            for _ in 0..w {
                for t in [
                    tile(cx, cy),
                    tile(cx.wrapping_sub(1), cy),
                    tile(cx.wrapping_sub(1), cy.wrapping_sub(1)),
                    tile(cx, cy.wrapping_sub(1)),
                ] {
                    self.t.tile_type[t] = 1;
                }
                cx = cx.wrapping_add(1);
            }
            cy = cy.wrapping_add(1);
        }
        self.retile_and_shade(ax, ay, bx, by);
    }

    /// sub_33AE0 (:41094), wall variant: write `ty` onto the cell and
    /// its W/NW/N neighbors unconditionally, then 3x3 shading with a
    /// hard floor of 32 (no retile, no PRNG).
    fn set_type_2x2(&mut self, t: usize, ty_val: u8) {
        let (cx, cy) = (tx(t), ty(t));
        self.t.tile_type[t] = ty_val;
        self.t.tile_type[tile(cx.wrapping_sub(1), cy)] = ty_val;
        self.t.tile_type[tile(cx.wrapping_sub(1), cy.wrapping_sub(1))] = ty_val;
        self.t.tile_type[tile(cx, cy.wrapping_sub(1))] = ty_val;
        let mut yy = cy.wrapping_sub(1);
        for _ in 0..3 {
            let mut xx = cx.wrapping_sub(1);
            for _ in 0..3 {
                let se = self.t.height[tile(xx.wrapping_add(1), yy.wrapping_add(1))];
                let nw = self.t.height[tile(xx.wrapping_sub(1), yy.wrapping_sub(1))];
                let mut s = nw.wrapping_sub(se).wrapping_add(32);
                if (s as i8) < 32 {
                    s = 32;
                } else if (s as i8) > 40 {
                    s = (s & 7) + 40;
                }
                let c = tile(xx, yy);
                self.t.shading[c] = s;
                self.t.angle[c] &= 0xF7;
                xx = xx.wrapping_add(1);
            }
            yy = yy.wrapping_add(1);
        }
    }

    /// sub_40A10 (:51621): adjust one cell's height by `delta` (clamped
    /// 0..200), update its slope nibble (1 = land; 0 = water when the
    /// floor is reached and no neighbor blocks conversion), then
    /// recompute the 1-cell neighborhood. `protect` mode aborts on
    /// building-protected cells and honors protection in the stencil.
    /// Returns true only via the literal `(0,0)` clamp latch (dead in
    /// practice; kept faithful).
    fn dig_cell(&mut self, ax: i16, ay: i16, delta: i16, protect: bool) -> bool {
        let t = tile(ax as u8, ay as u8);
        let mut saturated = false;
        let mut v = delta as i32 + self.t.height[t] as i32;
        if v > 200 {
            v = 200;
            if ax == 0 && ay == 0 {
                saturated = true;
            }
        }
        if v < 0 {
            v = 0;
            if ax == 0 && ay == 0 {
                saturated = true;
            }
        }
        if protect && self.t.angle[t] & 0x80 != 0 {
            return true;
        }
        self.t.height[t] = v as u8;
        // MC2's twin `sub_56F10` (EF:39534-39543): on a cave the
        // ceiling counter-shifts by the RAW delta (dig down = roof
        // up), saturating high at 255 and u8-truncating below zero
        // exactly like retail's char write; the invariant is then
        // re-asserted by the tail recompute's shading pass.
        if self.is_cave() {
            let c = self.t.ceiling[t] as i32 - delta as i32;
            self.t.ceiling[t] = if c >= 255 { 255 } else { c as u8 };
        }
        if v != 0 {
            self.t.angle[t] = (self.t.angle[t] & 0xF8) | 1;
        } else {
            // Water conversion: all 8 neighbors must not carry slope
            // codes 2, 3 or 5 (sub_409E0), else leave the angle alone.
            let clear = [
                (-1, -1),
                (0, -1),
                (1, -1),
                (1, 0),
                (-1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ]
            .iter()
            .all(|&(dx, dy)| {
                let n = self.t.angle[step(t, dx, dy)] & 7;
                n != 5 && n != 2 && n != 3
            });
            if clear {
                self.t.angle[t] &= 0xF0;
            }
        }
        if protect {
            self.recompute_protected(tx(t), ty(t), tx(t), ty(t));
        } else {
            self.recompute_unprotected(tx(t), ty(t), tx(t), ty(t));
        }
        saturated
    }

    /// The ring iterator of sub_11410/sub_114B0 (:16697/:16732): yields
    /// every (dx, dy) of rings `lo..=hi` EXCEPT the last entry of ring
    /// `hi`, which the original fetches together with the stop code and
    /// drops — a faithful off-by-one.
    /// Combat-effect access to the single-cell dig (the fire's scorch,
    /// sub_40D30(expl, 0, 0, -depth, 1)).
    pub(crate) fn dig_cell_pub(&mut self, ax: i16, ay: i16, delta: i16, protect: bool) -> bool {
        self.dig_cell(ax, ay, delta, protect)
    }

    /// Combat-effect access to the ring-walk disc dig (sub_40D30 /
    /// MC2 sub_572C0).
    pub(crate) fn dig_disc_pub(
        &mut self,
        i: usize,
        lo: i32,
        hi: i32,
        delta: i16,
        protect: bool,
    ) -> bool {
        self.dig_disc(i, lo, hi, delta, protect)
    }

    pub(crate) fn ring_cells(&self, lo: i32, hi: i32) -> Vec<(u8, u8)> {
        let mut out = Vec::new();
        if lo < 0 || lo > 31 {
            return out;
        }
        let hi_c = hi.min(31);
        let mut ring = lo;
        loop {
            let cells = &self.assets.rings[ring as usize];
            for (k, &d) in cells.iter().enumerate() {
                let last_of_ring = k + 1 == cells.len();
                if last_of_ring && ring >= hi_c {
                    return out; // fetched with stop code, dropped
                }
                out.push(d);
                if last_of_ring {
                    break;
                }
            }
            ring += 1;
            if ring > hi_c || ring > 31 {
                return out;
            }
        }
    }

    /// sub_40D30 (:51693): dig a disc of rings `lo..=hi` (clamped to
    /// the event's radius) around the event, height delta `delta`.
    fn dig_disc(&mut self, i: usize, lo: i32, hi: i32, delta: i16, protect: bool) -> bool {
        let e = self.ent[i];
        let cx = ((e.x as u32 + 128) >> 8) as i32;
        let cy = ((e.y as u32 + 128) >> 8) as i32;
        let hi = hi.min((e.f80 >> 8) as i32);
        for (dx, dy) in self.ring_cells(lo, hi) {
            if self.dig_cell(
                (cx + dx as i32) as i16,
                (cy + dy as i32) as i16,
                delta,
                protect,
            ) && protect
            {
                return true;
            }
        }
        false
    }

    /// sub_255D0 (:28353): the -3 disc variant that never aborts.
    /// (Also ≡ MC2's `sub_31F00` EF:23460 — the (10,11) scorch
    /// ring's stamper: same template walk, same −3 dig, same
    /// f80>>8 radius clamp.)
    pub(crate) fn dig_disc_minus3(&mut self, i: usize, lo: i32, hi: i32) {
        let e = self.ent[i];
        let cx = ((e.x as u32 + 128) >> 8) as i32;
        let cy = ((e.y as u32 + 128) >> 8) as i32;
        let hi = hi.min((e.f80 >> 8) as i32);
        for (dx, dy) in self.ring_cells(lo, hi) {
            self.dig_cell((cx + dx as i32) as i16, (cy + dy as i32) as i16, -3, false);
        }
    }

    /// sub_11760 (:16869) `& 1`: the ANGLE-NIBBLE water probe on the
    /// plain `>>8` cell — the terraform diggers and the fire scorch
    /// gate use this one, and it counts shore/wave cells (type 45,
    /// nibble 0) as WATER. The tile-type sibling (sub_11810,
    /// `on_water_pub`) does not — check the caller's retail anchor
    /// before picking one.
    pub(crate) fn on_water(&self, x: u16, y: u16) -> bool {
        self.t.angle[tile((x >> 8) as u8, (y >> 8) as u8)] & 0xF == 0
    }

    // ---- math helpers ---------------------------------------------------

    /// sub_358D0 (:42470): shortest wrapped tile delta in -128..=128.
    pub(crate) fn wrap_delta(a: i16, b: i16) -> i32 {
        let d = b.wrapping_sub(a);
        if d > 128 {
            (d as i32) - 256
        } else if d < -128 {
            (d as i32) + 256
        } else {
            d as i32
        }
    }

    /// sub_40F87 (:51818): angle from delta in 1/2048 turns (0 = -y).
    pub(crate) fn angle_of(dx: i16, dy: i16) -> u16 {
        let lut = |n: i32, d: i32| ATAN[((n << 8) / d) as usize] as i32;
        let (a1, a2) = (dx as i32, dy as i32);
        let r = if a1 == 0 && a2 == 0 {
            0
        } else if a1 < 0 {
            if a2 < 0 {
                if -a1 < -a2 {
                    2048 - lut(-a1, -a2)
                } else {
                    1536 + lut(-a2, -a1)
                }
            } else if -a1 < a2 {
                1024 + lut(-a1, a2)
            } else {
                1536 - lut(a2, -a1)
            }
        } else if a2 < 0 {
            if a1 < -a2 {
                lut(a1, -a2)
            } else {
                512 - lut(-a2, a1)
            }
        } else if a1 < a2 {
            1024 - lut(a1, a2)
        } else {
            512 + lut(a2, a1)
        };
        r as u16
    }

    /// Distance_410CE (:51874): Newton integer sqrt with seed table.
    pub(crate) fn isqrt(square: u32) -> u32 {
        if square == 0 {
            return 0;
        }
        let bit = 31 - square.leading_zeros();
        let mut i = BIT_SQRT[bit as usize];
        while square / i < i {
            i = (square / i + i) >> 1;
        }
        i
    }

    /// sub_42150/sub_423D0 (:52638/:52739) on two 8.8 positions.
    pub(crate) fn angle_between(ax: u16, ay: u16, bx: u16, by: u16) -> u16 {
        Self::angle_of(
            (bx as i16).wrapping_sub(ax as i16),
            (by as i16).wrapping_sub(ay as i16),
        )
    }
    fn dist_between(ax: u16, ay: u16, bx: u16, by: u16) -> u16 {
        let dx = (bx as i16).wrapping_sub(ax as i16) as i32;
        let dy = (by as i16).wrapping_sub(ay as i16) as i32;
        Self::isqrt((dx * dx + dy * dy) as u32) as u16
    }

    /// sub_41EC0 (:52523), pitch-0 path: advance a position `speed`
    /// units along `angle` (16.16 trig, wrapping i16/u16 adds).
    fn advance(x: &mut u16, y: &mut u16, angle: u16, speed: i16) {
        if speed == 0 {
            return;
        }
        let a = (angle & 0x7FF) as usize;
        *x = x.wrapping_add(((speed as i32 * SIN[a]) >> 16) as u16);
        *y = y.wrapping_sub(((COS[a] * speed as i32) >> 16) as u16);
    }

    // ---- the spawn scan -------------------------------------------------

    /// sub_36480 (:43065): dispatch one feature entity.
    fn dispatch(&mut self, table: &mut [Rec], slot: usize) {
        let rec = table[slot];
        let model = rec.model;
        let chained = matches!(model, 28 | 29 | 31 | 50) && rec.swi_id != 0;
        if chained {
            self.walk_chain(table, slot);
            return;
        }
        let x = rec.x << 8;
        let y = rec.y << 8;
        let z = self.ground_z(x, y) as i16;
        if let Some(i) = self.spawn_creator(model, x, y, z) {
            if model == 45 {
                self.building_fixup(i, rec.parent.wrapping_add(16));
            }
        }
    }

    /// sub_362C0 (:42972): walk a feature chain root-first, clearing
    /// each node's pending flag and running the per-model segment
    /// function on every parent→child coordinate pair.
    fn walk_chain(&mut self, table: &mut [Rec], slot: usize) {
        let class = table[slot].class;
        let model = table[slot].model;
        // A valid chain is shorter than the table; the caps below are
        // unreachable on well-formed data and break the CYCLE livelock
        // on garbage links (frankenstein bycatch: MC2 reuses the
        // parent/child fields as context params, and a malformed
        // community MC1 level could hang retail the same way).
        let mut cur = slot;
        let mut hops = table.len();
        while table[cur].parent != 0 {
            cur = table[cur].parent as usize % table.len();
            hops -= 1;
            if hops == 0 {
                self.note_misfit(class, model);
                return;
            }
        }
        let mut hops = table.len();
        loop {
            if table[cur].class != class || table[cur].model != model {
                return;
            }
            hops -= 1;
            if hops == 0 {
                self.note_misfit(class, model);
                return;
            }
            let child = table[cur].child as usize % table.len();
            table[cur].swi_id = 0;
            if child == 0 {
                return;
            }
            let (x1, y1) = (table[cur].x, table[cur].y);
            let (x2, y2) = (table[child].x, table[child].y);
            match model {
                28 => self.segment_wall(x1 as i16, y1, x2 as i16, y2 as i16),
                29 => self.segment_track(x1 as i16, y1 as i16, x2 as i16, y2 as i16),
                31 => self.segment_canyon(x1, y1, x2, y2),
                50 => self.segment_ridge(x1, y1, x2, y2),
                _ => unreachable!(),
            }
            cur = child;
        }
    }

    /// Creators (`off_97D12`, :5075). Models absent from retail data or
    /// with null/stub creators spawn nothing. Non-ticking models spawn
    /// an event that the loop purges unticked — only its pool-slot
    /// churn is observable, so their creator bodies reduce to alloc +
    /// identity fields (positions kept for completeness).
    pub(crate) fn spawn_creator(&mut self, model: u16, x: u16, y: u16, z: i16) -> Option<usize> {
        // Null/stub creator entries: model 24 (stub returning 0),
        // 37, 46..49 (null). Everything else allocates one event.
        if matches!(model, 24 | 37 | 46..=49) || model > 61 {
            return None;
        }
        // Combat-effect models get their real inits (crate::mc1::combat) —
        // in the original one init table serves load AND runtime; at
        // load time the fixpoint loop purges them unticked either way.
        // Model 17 matters in the wild: level 032 authors c10m17
        // fire-trap records behind dispositions (they erupt as the
        // 10-tick blast ring when fired).
        match model {
            // 14: the mana-scatter puff (sub_3AB40) — authored THING
            // records behind trigger dispositions mint it (mc1l1's
            // t=344 scatter); the generic arm below skipped its two
            // ctor rand draws and left it stateless.
            0 | 1 | 5 | 14 | 17 | 23 | 25 => return self.spawn_effect(model as u8, x, y, z),
            39 => return self.spawn_mana_ball(x, y, z),
            _ => {}
        }
        let i = self.new_event()?;
        let e = &mut self.ent[i];
        e.class64 = 10;
        e.model65 = model as u8;
        e.x = x;
        e.y = y;
        e.z = z;
        match model {
            // sub_3A8D0: growing hill / volcano.
            9 => {
                e.tick70 = 9;
                e.max_life = 17;
                e.act_life = 17;
                e.f44 = 2000;
                e.flags = 0;
                e.f80 = 768;
                e.f82 = 768;
                e.f84 = 0x2000;
            }
            // sub_3A930: one-shot shallow dish.
            10 => {
                e.tick70 = 10;
                e.max_life = 1;
                e.act_life = 1;
                e.f44 = 100;
                e.flags = 0x20000;
                e.f80 = 128;
                e.f82 = 128;
                e.f84 = 128;
            }
            // sub_3A9A0 (:46763): expanding crater (also the canyon
            // digger ctor). The flag word is EDITED, not cleared —
            // `+16 &= 0xFFFDFFF7` then `+18 |= 2` (:46779-80), i.e.
            // drop 0x20008 and raise 0x20000 over whatever the
            // recycled slot still holds. Ours zeroed it, so every
            // crater in mc1l42 read flags 0 against retail's 0x20000.
            11 => {
                e.tick70 = 11;
                e.max_life = 40;
                e.act_life = 40;
                e.f44 = 200;
                e.flags = (e.flags & !0x20008) | 0x20000;
                e.f80 = 2304;
                e.f82 = 2304;
                e.f84 = 0x2000;
            }
            // sub_3B060/3B120/3B1D0/3B2A0: unchained wall/track/canyon/
            // ridge nodes; their events tick straight into the self-kill
            // handler (byte70 30/31/33/54 → sub_253E0).
            28 => {
                e.tick70 = 30;
                e.max_life = 0;
                e.act_life = 0;
                e.flags = 0;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
            }
            29 => {
                e.tick70 = 31;
                e.max_life = 0;
                e.act_life = 0;
                e.flags = 0;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
            }
            30 => {
                e.tick70 = 32;
                e.max_life = 0;
                e.act_life = 0;
                e.flags = 0;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
            }
            31 => {
                e.tick70 = 33;
                e.max_life = 0;
                e.act_life = 0;
                e.flags = 0;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
            }
            50 => {
                e.tick70 = 54;
                e.max_life = 0;
                e.act_life = 0;
                e.flags = 0;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
            }
            // sub_3B180: canyon head (only reached via segment spawns
            // in practice; unchained model-32 level entities are absent
            // from retail data).
            32 => {
                e.tick70 = 34;
                e.max_life = 0;
                e.act_life = 0;
                e.f126 = 256;
                e.flags = 0;
            }
            // sub_3B230: ridge head.
            51 => {
                e.tick70 = 55;
                e.max_life = 0;
                e.act_life = 0;
                e.f26 = 256;
                e.f126 = 1024;
                e.flags = 0;
                e.f80 = 768;
                e.f82 = 768;
                e.f84 = 768;
            }
            // sub_3B690: building/castle spawner (fix-up follows).
            45 => {
                e.tick70 = 51;
                e.max_life = 30;
                e.f44 = 100;
                e.f26 = 4;
                e.flags = 9;
                e.f28 = 33;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
            }
            // sub_3ABE0 (:46946): the earthquake crevice walker —
            // life 128, step 256, RANDOM initial heading off its own
            // LCG, extents 1024/1024/0x4000, NOT map-linked (its
            // craters are the visible/audible part).
            15 => {
                e.tick70 = 15;
                e.max_life = 128;
                e.act_life = 128;
                e.f126 = 256;
                e.flags &= !8;
                e.f44 = 100;
                e.f26 = 0;
                let d = lcg32(&mut e.rand);
                e.f30 = (d & 0x7FF) as u16;
                e.f80 = 1024;
                e.f82 = 1024;
                e.f84 = 0x4000;
            }
            // sub_3ADB0 (:47008): the volcano eruption driver the
            // finished cone spawns. maxLife 10000 is NEVER counted
            // down — lifetime is the driver's own state machine
            // (sub_25EC0; see combat::eruption_tick).
            18 => {
                e.tick70 = 18;
                e.max_life = 10000;
                e.act_life = 10000;
                e.f44 = 200;
                e.f26 = 0;
                e.flags &= !8;
            }
            // sub_3B760 (:47545): the castle ground-leveling pass
            // (state 43); counter armed by its first tick. The ctor
            // writes max_life 0 (:47557) — the machine runs on the
            // +26 counter, never on life.
            41 => {
                e.tick70 = 43;
                e.max_life = 0;
                e.act_life = 0;
                e.flags &= !8;
            }
            // sub_3B7B0 (:47567): the CASTLE painter (state 44,
            // sub_285C0) — the caller stamps level (+71) and the
            // castle link. Life 0 like the leveler (:47579).
            42 => {
                e.tick70 = 44;
                e.max_life = 0;
                e.act_life = 0;
                e.flags &= !8;
            }
            // sub_3B6F0 (:47526): the castle UPGRADE token — state
            // 45, life 8, +44 = -1536 (inert dead weight, same
            // family as the possess flash), LINKED at spawn (:47537
            // — the tile-link bit is the fresh token's flags 4),
            // sprite row 41, 512 extents. The caller stamps the
            // owner; the delivery resolves the castle through the
            // owner's bound slot, never a stored link.
            43 => {
                e.tick70 = 45;
                e.max_life = 8;
                e.act_life = 8;
                e.f44 = (-1536i16) as u16;
                e.flags &= !8;
                self.link(i, x, y, z);
                self.set_sprite(i, 41);
                self.ent[i].f80 = 512;
                self.ent[i].f82 = 512;
            }
            // sub_3B300 (model 34): the PORTAL vortex — sprite row 223,
            // 1-tile extents, spawned 640 alt units above ground (its
            // tick re-grounds it from the second turn), destination
            // defaulting to its own position (a THING post-init
            // overwrites it with the authored target). The LCG draw is
            // the original's random scatter of that default. Purged
            // unticked at LOAD time; persistent + drawable at runtime.
            34 => {
                e.tick70 = 36;
                e.max_life = 0;
                e.act_life = 0;
                e.flags = 0;
                e.dest_x = e.x;
                e.dest_y = e.y;
                lcg32(&mut e.rand);
                let (x, y, z) = (e.x, e.y, e.z);
                self.set_sprite(i, 223);
                self.ent[i].f80 = 256;
                self.ent[i].f82 = 256;
                self.ent[i].f84 = 256;
                self.link(i, x, y, z.wrapping_add(640));
            }
            // sub_3B860 (:47613): the crab egg (10,52). Laid at RUNTIME
            // by the adult crab (mobs.rs) — authored (10,52) records are
            // purged by MODEL in the load fixpoint (`event_loop`, model
            // 52 ineligible), so the link/refill/sprite here are
            // load-transparent and only the runtime egg incubates.
            // State 56 = the hatch timer; f26 (600) is the creator
            // default, immediately overwritten by the layer with
            // 10*(rand%10)+100. Extents ride sprite 205.
            52 => {
                e.tick70 = 56;
                e.max_life = 100000;
                e.f44 = 500;
                e.f26 = 600;
                e.f140 = 500;
                e.f136 = 2000;
                e.flags &= !8;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
                self.refill_life(i);
                self.set_sprite(i, 205);
            }
            // All remaining retail models (0, 1, 5, 6, 8, 13, 14, 15,
            // 17, 23, 25, 33, 38, 39, 44, …): purged unticked, no
            // terrain writes, no global PRNG — slot churn only. Models
            // 13/14/15 draw from their (doomed) entity LCG; unobservable.
            _ => {
                e.tick70 = model as u8; // never dispatched
            }
        }
        Some(i)
    }

    /// sub_36DF0 (:43707): building placement fix-up. `bt` = the level
    /// entity's parent + 16, an index into the build table.
    pub(crate) fn building_fixup(&mut self, i: usize, bt: u16) {
        let def = self.assets.build_tab[bt as usize % self.assets.build_tab.len()];
        let (bw, bh) = (def.w as u16, def.h as u16);
        self.ent[i].f26 = 2;
        self.ent[i].f128 = ((bw * bh) >> 4) as i16;
        // Snap to the tile origin.
        let (px, py, pz) = (
            self.ent[i].x & 0xFF00,
            self.ent[i].y & 0xFF00,
            self.ent[i].z,
        );
        self.move_relink(i, px, py, pz);
        let e = &self.ent[i];
        let mut cx = ((e.x >> 8) as u8).wrapping_sub((bw >> 1) as u8);
        let cy = ((e.y >> 8) as u8).wrapping_sub((bh >> 1) as u8);
        if (cx as u16 + cy as u16) % 2 == 1 {
            // Odd corner parity: shift one tile east (relinks).
            let (nx, ny, nz) = (
                self.ent[i].x.wrapping_add(0x100),
                self.ent[i].y,
                self.ent[i].z,
            );
            self.move_relink(i, nx, ny, nz);
            cx = cx.wrapping_add(1);
        }
        let z = 32 * self.avg4(cx, cy, bh as u8, bw as u8) as i32;
        let e = &mut self.ent[i];
        e.f80 = ((bw << 8).wrapping_add(1280)) >> 1;
        e.f82 = ((bh << 8).wrapping_add(1280)) >> 1;
        e.f84 = 0x4000;
        e.act_life = 30;
        e.f44 = 2000;
        e.z = z as i16;
        e.f28 |= 2;
        e.f71 = bt as u8;
    }

    // ---- segment functions ----------------------------------------------

    /// sub_35900 (:42487): the spawn z both wall segments use.
    fn seg_z(&self, x1: i16, y1: u16, x2lo: u8, y2lo: u8) -> i16 {
        let h1 = self.t.height[tile(x1 as u8, y1 as u8)];
        let h2 = self.t.height[tile(x2lo, y2lo)];
        32 * h1.max(h2) as i16
    }

    /// Spawn one wall piece (ctor model 27, sub_3B000 :47142).
    fn spawn_wall_piece(&mut self, x: i16, y: u16, z: i16, tick: u8, run: u16) {
        if let Some(i) = self.new_event() {
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 27;
            e.tick70 = tick;
            e.max_life = 2;
            e.act_life = 2;
            e.f44 = ((z >> 5) + 48) as u16;
            e.f26 = run as i16;
            e.flags = 0;
            let (px, py) = ((x as u16) << 8, y << 8);
            self.link(i, px, py, z);
        }
    }

    /// sub_35960 (:42513), model 28: decompose the wrapped delta into a
    /// staircase of `|major|/10 + 1` alternating axis-aligned pieces
    /// (remainders folded into the first step) and spawn a wall-strip
    /// event per piece.
    fn segment_wall(&mut self, x1: i16, y1: u16, x2: i16, y2: i16) {
        let mut dx = Self::wrap_delta(x1, x2);
        let mut dy = Self::wrap_delta(y1 as i16, y2);
        if dx == 0 && dy == 0 {
            return;
        }
        let (mut cx, mut cy) = (x1, y1);
        let (mut ex, mut ey) = (x2 as u8, y2 as u8);
        if dx < 0 {
            dy = -dy;
            dx = -dx;
            // Swap endpoints (only the low bytes of the far end are used).
            let (sx, sy) = (cx as u8, cy as u8);
            cx = ex as i16;
            cy = ey as u16;
            ex = sx;
            ey = sy;
        }
        if dy.abs() >= dx {
            let steps = (dy / 10).abs() + 1;
            let (qy, mut ry) = (dy / steps, dy % steps);
            let (qx, mut rx) = (dx / steps, dx % steps);
            for _ in 0..steps {
                let z = self.seg_z(cx, cy, ex, ey as u8);
                if qy >= 0 {
                    self.spawn_wall_piece(cx, cy, z, 28, (ry + qy) as u16);
                } else {
                    self.spawn_wall_piece(cx, cy, z, 27, (-qy - ry) as u16);
                }
                cy = cy.wrapping_add((qy + ry) as u16);
                let z = self.seg_z(cx, cy, ex, ey as u8);
                self.spawn_wall_piece(cx, cy, z, 29, (rx + qx) as u16);
                cx = cx.wrapping_add((rx + qx) as i16);
                ry = 0;
                rx = 0;
            }
        } else {
            let steps = dx / 10 + 1;
            let (qx, mut rx) = (dx / steps, dx % steps);
            let (qy, mut ry) = (dy / steps, dy % steps);
            for _ in 0..steps {
                let z = self.seg_z(cx, cy, ex, ey as u8);
                self.spawn_wall_piece(cx, cy, z, 29, (rx + qx) as u16);
                cx = cx.wrapping_add((rx + qx) as i16);
                let z = self.seg_z(cx, cy, ex, ey as u8);
                if qy >= 0 {
                    self.spawn_wall_piece(cx, cy, z, 28, (ry + qy) as u16);
                } else {
                    self.spawn_wall_piece(cx, cy, z, 27, (-qy - ry) as u16);
                }
                cy = cy.wrapping_add((qy + ry) as u16);
                rx = 0;
                ry = 0;
            }
        }
    }

    /// sub_35BF0 (:42629), model 29: split the delta into a diagonal
    /// run and an axis-aligned run; spawn a track-painter event (ctor
    /// model 30, byte70 32) for each.
    fn segment_track(&mut self, x1: i16, y1: i16, x2: i16, y2: i16) {
        let dx = Self::wrap_delta(x1, x2);
        let dy = Self::wrap_delta(y1, y2);
        let sdx = dx.signum();
        let sdy = dy.signum();
        let adx = dx.abs();
        let ady = dy.abs();
        let diag = adx.min(ady);
        let rest = (ady - adx).abs();
        let (rest_dx, rest_dy) = if adx <= ady { (0, sdy) } else { (sdx, 0) };
        let spawn_track = |g: &mut Self, x: i16, y: i16, count: i32, stx: i32, sty: i32| {
            if let Some(i) = g.new_event() {
                let e = &mut g.ent[i];
                e.class64 = 10;
                e.model65 = 30;
                e.tick70 = 32;
                e.max_life = 0;
                e.act_life = 0;
                e.flags = 0;
                e.f26 = count as i16;
                e.f30 = stx as u16;
                e.f32 = sty as u16;
                let (px, py) = ((x as u16) << 8, (y as u16) << 8);
                g.link(i, px, py, 0);
            }
        };
        spawn_track(self, x1, y1, diag, sdx, sdy);
        spawn_track(
            self,
            x1.wrapping_add((diag * sdx) as i16),
            y1.wrapping_add((diag * sdy) as i16),
            rest,
            rest_dx,
            rest_dy,
        );
    }

    /// sub_35D30 (:42697), model 31: spawn a canyon head aimed at the
    /// child, with a life of `distance >> 8` tiles.
    fn segment_canyon(&mut self, x1: u16, y1: u16, x2: u16, y2: u16) {
        let (ax, ay) = (x1 << 8, y1 << 8);
        let (bx, by) = (x2 << 8, y2 << 8);
        let ang = Self::angle_between(ax, ay, bx, by);
        let dist = Self::dist_between(ax, ay, bx, by);
        if let Some(i) = self.new_event() {
            let z = 32 * self.t.height[tile(x1 as u8, y1 as u8)] as i16;
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 32;
            e.tick70 = 34;
            e.max_life = 0;
            e.f126 = 256;
            e.flags = 0;
            e.x = ax;
            e.y = ay;
            e.z = z;
            e.f30 = ang;
            e.act_life = (dist >> 8) as i32;
        }
    }

    /// sub_35DE0 (:42722), model 50: spawn a ridge head, life =
    /// `distance / 1024` (one raise every 4 tiles).
    fn segment_ridge(&mut self, x1: u16, y1: u16, x2: u16, y2: u16) {
        let (ax, ay) = (x1 << 8, y1 << 8);
        let (bx, by) = (x2 << 8, y2 << 8);
        let ang = Self::angle_between(ax, ay, bx, by);
        let dist = Self::dist_between(ax, ay, bx, by);
        if let Some(i) = self.new_event() {
            let z = 16 * self.t.height[tile(x1 as u8, y1 as u8)] as i16;
            let e = &mut self.ent[i];
            e.class64 = 10;
            e.model65 = 51;
            e.tick70 = 55;
            e.max_life = 0;
            e.f26 = 256;
            e.f126 = 1024;
            e.flags = 0;
            e.f80 = 768;
            e.f82 = 768;
            e.f84 = 768;
            e.x = ax;
            e.y = ay;
            e.z = z;
            e.f30 = ang;
            e.act_life = dist as i32 / 1024;
        }
    }

    // ---- the event loop -------------------------------------------------

    /// sub_36620 (:43181): one global PRNG step, then sweep the pool to
    /// fixpoint. Eligibility is tested on the MODEL; the handler is
    /// selected by byte 70.
    fn event_loop(&mut self) {
        lcg32(&mut self.rand);
        loop {
            let mut run_again = false;
            for i in 1..self.ent.len() {
                if self.ent[i].class64 == 0 {
                    continue;
                }
                if self.ent[i].class64 != 10 {
                    self.ent[i].flags |= 0x400;
                } else {
                    let model = self.ent[i].model65;
                    let eligible = match model {
                        0..=0x1A => matches!(model, 9..=0xB),
                        0x1B..=0x20 => true,
                        0x21..=0x2C => false,
                        0x2D => self.ent[i].tick70 == 51,
                        0x2E..=0x31 => false,
                        0x32 | 0x33 => true,
                        _ => false,
                    };
                    if eligible {
                        run_again = true;
                        self.tick(i, None);
                    } else if model != 0x2D {
                        self.ent[i].flags |= 0x400;
                    }
                }
                if self.ent[i].flags & 0x400 != 0 {
                    self.free_entity(i);
                }
            }
            if !run_again {
                break;
            }
        }
    }

    /// str_255998 (:4856) dispatch by byte 70. `ctx` = the player
    /// context at RUNTIME (None during the load fixpoint): the
    /// terrain deformers broadcast ch0 damage + the loop-10 rumble,
    /// which only matter — and only have a listener — once the world
    /// runs (deliberate: the original's load pass broadcasts into the
    /// half-built pool too, but nothing observable survives it).
    pub(crate) fn tick(&mut self, i: usize, ctx: Option<&crate::mc1::mobs::MobCtx>) {
        match self.ent[i].tick70 {
            9 => self.tick_hill(i, ctx),
            10 => self.tick_dish(i),
            11 => self.tick_digger(i, ctx),
            15 => self.tick_quake_walker(i),
            27 => self.tick_wall_neg_y(i),
            28 => self.tick_wall_pos_y(i),
            29 => self.tick_wall_pos_x(i),
            32 => self.tick_track(i),
            34 => self.tick_canyon_head(i),
            43 => self.tick_castle_leveler(i),
            44 => self.tick_castle_painter(i),
            45 => self.tick_upgrade_token(i),
            51 => self.tick_building(i),
            55 => self.tick_ridge_head(i, ctx),
            // sub_253E0 rows (30, 31, 33, 54, …): pure self-kill.
            _ => self.ent[i].flags |= 0x400,
        }
    }

    /// sub_25470 (:28302), byte70 9: growing hill; finish punches a
    /// -40 pit at the center and spawns a transient model-18 marker
    /// (owner passed on — the eruption driver inherits immunity).
    /// Every growth tick is a KILL ZONE: full +44 (2000) on ch0 over
    /// the live extents (:28327, via the sub_127E0 writer — its
    /// wizard +50=30 ground-ride stamp is the mortality track) plus
    /// the loop-10 rumble (:28328).
    fn tick_hill(&mut self, i: usize, ctx: Option<&crate::mc1::mobs::MobCtx>) {
        let life = self.ent[i].act_life;
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        self.ent[i].act_life = life - 1;
        let finish = if life < 0 {
            true
        } else {
            let r = lcg32(&mut self.ent[i].rand);
            let hi = self.ent[i].f26 as i32 / 6;
            self.dig_disc(i, 0, hi, (r % 9) as i16, false)
        };
        if finish {
            self.dig_disc(i, 0, 0, -40, false);
            let (x, y, own) = (self.ent[i].x, self.ent[i].y, self.ent[i].id24);
            let z = self.ground_z(x, y) as i16;
            if let Some(m) = self.spawn_creator(18, x, y, z) {
                self.ent[m].id24 = own; // :28322
            }
            self.ent[i].flags |= 0x400;
        } else if let Some(ctx) = ctx {
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 0, amt, ctx, false, true);
            self.snd(10, i);
        }
    }

    /// sub_25570 (:28333), byte70 10: one-shot shallow dish, honoring
    /// building protection.
    fn tick_dish(&mut self, i: usize) {
        let e = self.ent[i];
        if !self.on_water(e.x, e.y) {
            let r = lcg32(&mut self.ent[i].rand);
            let hi = (self.ent[i].f80 >> 8) as i32;
            self.dig_disc(i, 0, hi, -((r % 7) as i16), true);
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_25670 (:28379), byte70 11: expanding -3 crater; radius grows
    /// only when the event's pool slot is divisible by 3. Every
    /// surviving tick: ch0 damage — full +44 before the phase-2 flag
    /// sets, +44/25 after (:28396-400) — and the loop-10 rumble
    /// (:28421).
    fn tick_digger(&mut self, i: usize, ctx: Option<&crate::mc1::mobs::MobCtx>) {
        if self.ent[i].f63 % 3 == 0 {
            self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        }
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        let e = self.ent[i];
        if life < 0 || self.on_water(e.x, e.y) {
            self.ent[i].flags |= 0x400;
            return;
        }
        if let Some(ctx) = ctx {
            let amt = if self.ent[i].flags & 2 != 0 {
                self.ent[i].f44 as u32 / 25
            } else {
                self.ent[i].f44 as u32
            };
            self.area_write(i, 0, amt, ctx, false, true);
        }
        let radius = (e.f80 >> 8) as i16;
        let mut upto = e.f26;
        if upto > radius - 1 {
            upto = radius - 1;
            if e.flags & 2 == 0 {
                self.dig_disc_minus3(i, radius as i32, radius as i32);
            }
        }
        self.ent[i].flags |= 2;
        self.dig_disc_minus3(i, 0, upto as i32);
        if ctx.is_some() {
            self.snd(10, i); // :28421
        }
    }

    /// sub_26670 (:29030), byte70 27: wall strip toward -Y.
    fn tick_wall_neg_y(&mut self, i: usize) {
        let e = self.ent[i];
        let x = ((e.x as u32 + 128) >> 8) as u8;
        let mut y = (((e.y as u32 + 128) >> 8) as u8).wrapping_add(2);
        let w = e.act_life as u16; // strip thickness (2)
        for _ in 0..w.wrapping_add(e.f26 as u16) {
            self.t.angle[tile(x.wrapping_sub(1), y)] |= 0x80;
            let mut t = tile(x, y);
            for _ in 0..w {
                self.wall_raise(t);
                t = (t + 1) & 0xFFFF;
            }
            self.t.angle[t] |= 0x80;
            y = y.wrapping_sub(1);
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_26560 (:28999), byte70 28: wall strip toward +Y, x aligned
    /// even then shifted -1.
    fn tick_wall_pos_y(&mut self, i: usize) {
        let e = self.ent[i];
        let mut x = ((e.x as u32 + 128) >> 8) as u8;
        let mut y = ((e.y as u32 + 128) >> 8) as u8;
        if x & 1 == 1 {
            x = x.wrapping_add(1);
        }
        let w = e.act_life as u16;
        x = x.wrapping_sub(w as u8).wrapping_add(1);
        for _ in 0..w.wrapping_add(e.f26 as u16) {
            self.t.angle[tile(x.wrapping_sub(1), y)] |= 0x80;
            let mut t = tile(x, y);
            for _ in 0..w {
                self.wall_raise(t);
                t = (t + 1) & 0xFFFF;
            }
            self.t.angle[t] |= 0x80;
            y = y.wrapping_add(1);
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_26760 (:29059), byte70 29: wall strip toward +X, aligned on
    /// (x+y) parity; border rows above and below.
    fn tick_wall_pos_x(&mut self, i: usize) {
        let e = self.ent[i];
        let mut x = ((e.x as u32 + 128) >> 8) as u8;
        let y = ((e.y as u32 + 128) >> 8) as u8;
        if (x as u16 + y as u16) % 2 == 1 {
            x = x.wrapping_add(1);
        }
        let run = e.f26 as u16;
        let mut t = tile(x, y).wrapping_sub(256) & 0xFFFF; // row y-1
        for _ in 0..run {
            self.t.angle[t] |= 0x80;
            t = (t + 1) & 0xFFFF;
        }
        let mut yy = y;
        for _ in 0..e.act_life as u16 {
            let mut t = tile(x, yy);
            for _ in 0..run {
                self.wall_raise(t);
                t = (t + 1) & 0xFFFF;
            }
            yy = yy.wrapping_add(1);
        }
        let mut t = tile(x, yy);
        for _ in 0..run {
            self.t.angle[t] |= 0x80;
            t = (t + 1) & 0xFFFF;
        }
        self.ent[i].flags |= 0x400;
    }

    /// The shared wall raise op: +48 height (u8 wrap, no clamp) unless
    /// the tile is already wall (type 8) with a type-8 west neighbor
    /// and no 4-neighbor towering ≥ 31 above (sub_264D0, :28966), then
    /// stamp type 8 on the 2x2 and reshade.
    fn wall_raise(&mut self, t: usize) {
        let raise = if self.t.tile_type[t] != 8 {
            true
        } else {
            let (cx, cy) = (tx(t), ty(t));
            let lim = self.t.height[t] as i32 + 30;
            self.t.tile_type[tile(cx.wrapping_sub(1), cy)] != 8
                || self.t.height[tile(cx.wrapping_sub(1), cy)] as i32 > lim
                || self.t.height[tile(cx.wrapping_add(1), cy)] as i32 > lim
                || self.t.height[tile(cx, cy.wrapping_add(1))] as i32 > lim
                || self.t.height[tile(cx, cy.wrapping_sub(1))] as i32 > lim
        };
        if raise {
            self.t.height[t] = self.t.height[t].wrapping_add(48);
        }
        self.set_type_2x2(t, 8);
    }

    /// sub_26890 (:29106), byte70 32: track painter — walk f26 tiles
    /// stepping (f30, f32), stamping slope 1 + protected retexture.
    fn tick_track(&mut self, i: usize) {
        let e = self.ent[i];
        let mut x = ((e.x as u32 + 128) >> 8) as u8;
        let mut y = ((e.y as u32 + 128) >> 8) as u8;
        let mut n = e.f26 as i32;
        while n != 0 {
            let t = tile(x, y);
            self.t.angle[t] = (self.t.angle[t] & 0xF0) | 1;
            self.recompute_protected(x, y, x, y);
            x = x.wrapping_add(e.f30 as u8);
            y = y.wrapping_add(e.f32 as u8);
            n -= 1;
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_25990 (:28534), byte70 15: the EARTHQUAKE crevice walker
    /// (spell 6's authentic payload — direct import). Water under it
    /// counts a ledger up (dry ticks count it back down); dies when
    /// the ledger passes 8 or life runs out. Each tick: wander the
    /// heading ±45, step 256 units, and drop a 10-tick m11 digger at
    /// the new spot with the walker's extents + owner. The rumble is
    /// the diggers' own loop-10.
    fn tick_quake_walker(&mut self, i: usize) {
        let (x0, y0) = (self.ent[i].x, self.ent[i].y);
        if self.on_water(x0, y0) {
            self.ent[i].f26 += 1;
        } else if self.ent[i].f26 > 0 {
            self.ent[i].f26 -= 1;
        }
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 || self.ent[i].f26 > 8 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let d = lcg32(&mut self.ent[i].rand);
        self.ent[i].f30 = ((d % 0x5B) as u16)
            .wrapping_add(self.ent[i].f30)
            .wrapping_sub(45)
            & 0x7FF;
        let (mut x, mut y) = (self.ent[i].x, self.ent[i].y);
        Self::advance(&mut x, &mut y, self.ent[i].f30, 256);
        self.ent[i].x = x;
        self.ent[i].y = y;
        let e = self.ent[i];
        if let Some(dg) = self.spawn_creator(11, x, y, e.z) {
            let g = &mut self.ent[dg];
            g.f80 = e.f80; // dword copy +80 covers both axes (:28564)
            g.f82 = e.f82;
            g.f84 = e.f84;
            g.act_life = 10;
            g.id24 = e.id24;
        }
    }

    /// sub_26920 (:29122), byte70 34: canyon head — spawn a 3-tick
    /// digger at the current position, advance one tile along the
    /// heading; stop on distance or water.
    fn tick_canyon_head(&mut self, i: usize) {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        let e = self.ent[i];
        if life < 0 || self.on_water(e.x, e.y) {
            self.ent[i].flags |= 0x400;
            return;
        }
        if let Some(d) = self.spawn_creator(11, e.x, e.y, e.z) {
            self.ent[d].act_life = 2;
            self.ent[d].f84 = e.f84;
            self.ent[d].id24 = e.id24; // :29141 — owner immunity chains
        }
        let (mut x, mut y) = (self.ent[i].x, self.ent[i].y);
        Self::advance(&mut x, &mut y, self.ent[i].f30, self.ent[i].f126);
        self.ent[i].x = x;
        self.ent[i].y = y;
    }

    /// sub_269A0 (:29147), byte70 55: ridge head — raise a radius-3
    /// disc by rand%15+10, advance 4 tiles. Each successful raise:
    /// full +44 on ch0 + the loop-10 rumble (:29163-64).
    fn tick_ridge_head(&mut self, i: usize, ctx: Option<&crate::mc1::mobs::MobCtx>) {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        let e = self.ent[i];
        if life < 0 || self.on_water(e.x, e.y) {
            self.ent[i].flags |= 0x400;
            return;
        }
        let r = lcg32(&mut self.ent[i].rand);
        self.dig_disc(i, 0, 1024, (r % 0xF + 10) as i16, false);
        if let Some(ctx) = ctx {
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 0, amt, ctx, false, false);
            self.snd(10, i);
        }
        let (mut x, mut y) = (self.ent[i].x, self.ent[i].y);
        Self::advance(&mut x, &mut y, self.ent[i].f30, self.ent[i].f126);
        self.ent[i].x = x;
        self.ent[i].y = y;
    }

    /// sub_27D30 (:29993), byte70 51: building construction — flatten
    /// the RLE footprint toward the placement height each tick, paint
    /// every 5th tick and at life 1; on the final tick retile the full
    /// rect and become a persistent (inert) castle entity.
    fn tick_building(&mut self, i: usize) {
        let e = self.ent[i];
        let cx = ((e.x as u32 + 128) >> 8) as u8;
        let cy = ((e.y as u32 + 128) >> 8) as u8;
        let target = (e.z >> 5) as i32;
        let def = self.assets.build_tab[e.f71 as usize % self.assets.build_tab.len()];
        let (w, h) = (def.w as u16, def.h as u16);
        let (half_w, half_h) = ((w >> 1) as u8, (h >> 1) as u8);
        self.ent[i].act_life -= 1;
        let life = self.ent[i].act_life;
        let x0 = cx.wrapping_sub(half_w);
        let y0 = cy.wrapping_sub(half_h);
        if life != 0 {
            self.flatten_build_row(e.f71 as usize, cx, cy, target, life, FlattenLaw::Building);
            if life % 5 == 0 || life == 1 {
                self.paint_build_row(e.f71 as usize, cx, cy);
            }
        } else {
            // Final tick: retile the whole rect, become a castle.
            self.recompute_protected(x0, y0, cx.wrapping_add(half_w), cy.wrapping_add(half_h));
            // byte70 == 51 (the only load-time case): persist as an
            // inert entity (byte70 52) with perimeter smoothing.
            self.ent[i].act_life = self.ent[i].f44 as i32;
            self.ent[i].flags |= 1;
            self.ent[i].tick70 = 52;
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            self.ent[i].z = self.ground_z(x, y) as i16;
            self.smooth_perimeter(cx, cy, half_h as u16, half_w as u16, 2);
            self.smooth_perimeter(cx, cy, half_h as u16, half_w as u16, 5);
        }
    }

    /// One flatten pass over build-table row `bt` centered on tile
    /// (cx, cy): the shared cell-code goal decode of sub_27D30
    /// (:30040-70) / sub_285C0 (:30541-94) / sub_279D0 (:29863-917),
    /// stepping each tile's height toward its goal by /divisor. The
    /// three retail builders share the decode but NOT the
    /// water-conversion condition — `law` picks it (see
    /// [`FlattenLaw`]; merging them was the castle-on-water bug:
    /// the courtyard's zero-delta cells must stay live water under
    /// the live painter).
    fn flatten_build_row(
        &mut self,
        bt: usize,
        cx: u8,
        cy: u8,
        target: i32,
        divisor: i32,
        law: FlattenLaw,
    ) {
        let def = self.assets.build_tab[bt % self.assets.build_tab.len()];
        let (w, h) = (def.w as u16, def.h as u16);
        let x0 = cx.wrapping_sub((w >> 1) as u8);
        let y0 = cy.wrapping_sub((h >> 1) as u8);
        let mut rows = h;
        let (mut x, mut y) = (x0, y0);
        let mut c = def.offset as usize;
        while rows != 0 {
            let ctl = self.assets.build_dat[c] as i8;
            c += 1;
            if ctl == 0 {
                y = y.wrapping_add(1);
                rows -= 1;
                x = x0;
                continue;
            }
            if ctl < 0 {
                x = x.wrapping_add((-(ctl as i32)) as u8);
                continue;
            }
            for _ in 0..ctl {
                let b = self.assets.build_dat[c];
                c += 1;
                let t = tile(x, y);
                let goal = if b < 0xF {
                    if b > 6 { Some(target) } else { None }
                } else if b >> 4 == 3 {
                    match (b % 16) % 3 {
                        1 => Some(target + 12),
                        2 => Some(target + 16),
                        _ => None,
                    }
                } else {
                    let lo = b % 16;
                    if lo != 0 {
                        Some(4 * (lo as i32 - 1) + target)
                    } else {
                        None
                    }
                };
                if let Some(goal) = goal {
                    let hh = self.t.height[t] as i32;
                    match law {
                        FlattenLaw::Building => {
                            let angle_before = self.t.angle[t];
                            self.t.height[t] =
                                self.t.height[t].wrapping_add(((goal - hh) / divisor) as u8);
                            if angle_before & 7 == 0 {
                                self.t.angle[t] = (angle_before & 0xF0) | 1;
                                self.recompute_protected(x, y, x, y);
                            }
                        }
                        FlattenLaw::CastleInit => {
                            self.t.height[t] = goal as u8;
                            if self.t.angle[t] & 7 == 0 {
                                self.t.angle[t] = (self.t.angle[t] & 0xF8) | 1;
                                self.recompute_protected(x, y, x, y);
                            }
                        }
                    }
                }
                x = x.wrapping_add(1);
            }
        }
    }

    /// The live castle painter's goal FILL (sub_285C0 :30592-668):
    /// walk build row `bt`'s RLE and write each covered cell's
    /// `goal − height` delta into the level-rect buffer at the row's
    /// centered offset — the caller applies the buffer in one sweep.
    /// The decode has NO 3x arm (:30637-41): every 0xF.. cell with a
    /// nonzero low nibble goals 4*(lo-1)+target (the +12/+16 fork is
    /// the INIT stamp's law; sharing it mis-heighted the tower-wall
    /// cells — mc1l0 t=563 pose.z); bytes 7..14 goal the bare
    /// target; bytes 1..6 and lo-nibble 0 leave the buffer cell
    /// alone, so an inner row's delta survives only until a later
    /// row rewrites it. A castle raised on water keeps LIVE WATER
    /// between its walls — the courtyard sits at the water-level
    /// datum (zero delta) until a collapse rubbles it.
    fn fill_castle_goal_row(
        &self,
        bt: usize,
        cx: u8,
        cy: u8,
        target: i32,
        ldef: BuildDef,
        buf: &mut [i16],
    ) {
        let def = self.assets.build_tab[bt % self.assets.build_tab.len()];
        let (w, h) = (def.w as u16, def.h as u16);
        let x0 = cx.wrapping_sub((w >> 1) as u8);
        let y0 = cy.wrapping_sub((h >> 1) as u8);
        // Row bt's rect sits centered inside the level rect
        // (:30594-97 — v34/v32, the x/y offsets of the smaller rect).
        let lw = ldef.w as i32;
        let dx = ((ldef.w >> 1) as i32) - ((def.w >> 1) as i32);
        let dy = ((ldef.h >> 1) as i32) - ((def.h >> 1) as i32);
        let mut rows = h;
        let (mut x, mut y) = (x0, y0);
        let (mut rx, mut ry) = (0i32, 0i32);
        let mut c = def.offset as usize;
        while rows != 0 {
            let ctl = self.assets.build_dat[c] as i8;
            c += 1;
            if ctl == 0 {
                y = y.wrapping_add(1);
                ry += 1;
                rows -= 1;
                x = x0;
                rx = 0;
                continue;
            }
            if ctl < 0 {
                x = x.wrapping_add((-(ctl as i32)) as u8);
                rx -= ctl as i32;
                continue;
            }
            for _ in 0..ctl {
                let b = self.assets.build_dat[c];
                c += 1;
                let goal = if b < 0xF {
                    if b > 6 { Some(target) } else { None }
                } else {
                    let lo = b % 16;
                    if lo != 0 {
                        Some(4 * (lo as i32 - 1) + target)
                    } else {
                        None
                    }
                };
                if let Some(goal) = goal {
                    let hh = self.t.height[tile(x, y)] as i32;
                    let idx = (dy + ry) * lw + dx + rx;
                    // In-bounds for every shipped table (row rects
                    // never outgrow the level rect); skip, not
                    // panic, on a malformed one.
                    if idx >= 0
                        && let Some(cell) = buf.get_mut(idx as usize)
                    {
                        *cell = (goal - hh) as i16;
                    }
                }
                x = x.wrapping_add(1);
                rx += 1;
            }
        }
    }

    /// sub_40E20 (:51729): the castle-transformation kill, one pass
    /// over the NEW level's RLE footprint. Per occupied cell, walking
    /// the tile's entity chain: anything owned by the castle owner is
    /// SPARED (:51744 — broader than the caster: your skeletons
    /// survive your own castle); class-2 scenery is deleted outright
    /// (:51749); class-5 creatures die instantly at any HP (life =
    /// −1, killer = the owner → kill credit + normal corpse drops)
    /// EXCEPT models 6/8/16 (:51753 — boss-tier exemptions). Every
    /// other class (wizards, balloons, castles, projectiles,
    /// effects) is structurally immune (:51760 default: break).
    ///
    /// SWEEP SHAPE (:30631-35): the kill fires for EVERY cell of every
    /// positive RLE run, over rows 1..=level, on EVERY painter tick —
    /// and crucially it runs BEFORE the cell byte is even read
    /// (`sub_40E20` at :30634, `v15 = *(v36 + v13++)` only at :30635),
    /// so an EMPTY cell of the footprint kills exactly like a masonry
    /// one. Gating this on `byte != 0` (as this used to) shrank the
    /// lethal area to the masonry alone — under 40% of the rectangle
    /// at level 7 (899 of 2304 tiles) — which is why castles read as
    /// far less deadly than retail. Only negative runs (explicit
    /// skips) are spared; MC1's castle rows contain none.
    fn build_footprint_kill(&mut self, level: usize, cx: u8, cy: u8, owner: u16) {
        for bt in 1..=level {
            let Some(def) = self.assets.build_tab.get(bt).copied() else {
                continue;
            };
            let (w, h) = (def.w as u16, def.h as u16);
            let x0 = cx.wrapping_sub((w >> 1) as u8);
            let y0 = cy.wrapping_sub((h >> 1) as u8);
            let mut rows = h;
            let (mut x, mut y) = (x0, y0);
            let mut c = def.offset as usize;
            while rows != 0 {
                let ctl = self.assets.build_dat[c] as i8;
                c += 1;
                if ctl == 0 {
                    y = y.wrapping_add(1);
                    rows -= 1;
                    x = x0;
                    continue;
                }
                if ctl < 0 {
                    x = x.wrapping_add((-(ctl as i32)) as u8);
                    continue;
                }
                for _ in 0..ctl {
                    c += 1; // the cell byte, consumed but NOT consulted
                    let mut j = self.map_entity[tile(x, y)] as usize;
                    while j != 0 {
                        let next = self.ent[j].next20 as usize;
                        // The 0x400 test is ours, not retail's (:51743
                        // has no such guard): our freed entities keep
                        // their tile link until the sweep, and
                        // `free_entity` on an already-freed slot would
                        // corrupt the free list. Vacuous otherwise.
                        if self.ent[j].id24 != owner && self.ent[j].flags & 0x400 == 0 {
                            match self.ent[j].class64 {
                                // :51747 — sub_41E80, the SOFT kill:
                                // the swept scenery lingers dead-
                                // flagged for one snapshot and the
                                // tick-top reap frees it (mc1l0
                                // t=3855, the lvl-4 commit's 39
                                // trees carry flags 0x2040C at 3856).
                                2 => self.ent[j].flags |= 0x400,
                                5 if !matches!(self.ent[j].model65, 6 | 8 | 16) => {
                                    self.ent[j].act_life = -1;
                                    self.ent[j].f38 = owner;
                                    self.ent[j].f40 = owner;
                                }
                                _ => {}
                            }
                        }
                        j = next;
                    }
                    x = x.wrapping_add(1);
                }
            }
        }
    }

    /// One paint pass over build-table row `bt` (the shared tile-type
    /// decode of sub_27D30/sub_285C0 via sub_33800).
    fn paint_build_row(&mut self, bt: usize, cx: u8, cy: u8) {
        let def = self.assets.build_tab[bt % self.assets.build_tab.len()];
        let (w, h) = (def.w as u16, def.h as u16);
        let x0 = cx.wrapping_sub((w >> 1) as u8);
        let y0 = cy.wrapping_sub((h >> 1) as u8);
        let mut rows = h;
        let (mut x, mut y) = (x0, y0);
        let mut c = def.offset as usize;
        while rows != 0 {
            let ctl = self.assets.build_dat[c] as i8;
            c += 1;
            if ctl == 0 {
                y = y.wrapping_add(1);
                rows -= 1;
                x = x0;
                continue;
            }
            if ctl < 0 {
                x = x.wrapping_add((-(ctl as i32)) as u8);
                continue;
            }
            for _ in 0..ctl {
                let b = self.assets.build_dat[c];
                c += 1;
                let t = tile(x, y);
                match b >> 4 {
                    0 => {
                        let k = b % 7;
                        if k != 0 {
                            self.paint(k as i8, 7, t, k - 1);
                        }
                    }
                    hi @ 1..=2 => self.paint(0, b as i8, t, hi + 7),
                    3 => {
                        let lo = b % 16;
                        self.paint((lo % 3) as i8, (lo / 3 + 10) as i8, t, lo / 3 + 10)
                    }
                    hi => self.paint(0, b as i8, t, hi + 11),
                }
                x = x.wrapping_add(1);
            }
        }
    }

    /// sub_285C0 (:30445), byte70 44: the CASTLE painter — the m42
    /// event a castle level-up spawns. The counter (+26) is armed to
    /// 19 on the first tick and DECREMENTED AT THE TOP (:30510), so
    /// the whole body reads the POST value: 18 work ticks (counter
    /// 18..1), the flatten divisor IS the counter (:30563), and the
    /// paint fires when `f26 % 7 == 0 || f26 == 1` (:30646), i.e. at
    /// 14, 7 and 1. The tick that reads a PRE value of 1 returns
    /// early WITHOUT working (:30512-16) and arms a negative idle
    /// phase; the counter then counts UP, and the tick that reads -1
    /// stamps the protection bit over the level footprint, hands the
    /// castle (f146) to sub-state 5, and despawns (:30697-709).
    ///
    /// Measured on a castle raised over ground 40 units below its
    /// target: the ramp was `62,64,...,96,98,100` — a flat +2 over 20
    /// ticks (divisor 20..1) — and is now `62,64,...,88,91,94,97,100`,
    /// 18 ticks with the tail accelerating as the divisor shrinks.
    /// The footprint crush stays lethal (a 17-part worm still goes
    /// 153,000 -> 0), it simply executes on 18 ticks instead of 20.
    ///
    /// The idle length comes from retail's byte +60, which we do not
    /// model as a field because both of its writers are known and
    /// they are complementary: :47583 spawns the plain painter with
    /// +60 = 1 (a 25-tick idle), and :56490 spawns the upgrade-commit
    /// painter with +60 = 0 AND the +18 kill bit (:56492). The kill
    /// bit — our `flags & 0x10000` — therefore selects the branch.
    fn tick_castle_painter(&mut self, i: usize) {
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.ent[i].f26 = 19;
        }
        let pre = self.ent[i].f26;
        if pre <= 0 {
            // :30682-84 — the negative idle phase counts UP, and only
            // the tick that READS -1 finishes.
            self.ent[i].f26 = pre + 1;
            if pre == -1 {
                self.finish_castle_painter(i);
            }
            return;
        }
        self.ent[i].f26 = pre - 1;
        if pre == 1 {
            // :30512-16 — no work on this tick: arm the idle phase and
            // return. The armed (kill-bit) painter carries +60 = 0 and
            // so finishes on the NEXT tick; the plain one idles 25.
            self.ent[i].f26 = if self.ent[i].flags & 0x10000 != 0 {
                -1
            } else {
                -25
            };
            return;
        }
        let e = self.ent[i];
        // :30520-21 — a shaking castle (damage-response +50 armed by
        // a nearby blast) suspends the whole work body; the counter
        // has already stepped. Retail resolves the castle through the
        // painter's +42 link; ours re-derives it by site.
        if let Some(c) = self.castle_at_site(e.x, e.y) {
            if self.ent[c].f50 != 0 {
                return;
            }
        }
        let cx = ((e.x as u32 + 128) >> 8) as u8;
        let cy = ((e.y as u32 + 128) >> 8) as u8;
        let target = (e.z >> 5) as i32;
        // Row = level verbatim (retail never clamps it): level 0
        // paints nothing, which is what a bare-flag castle owns.
        let level = e.f71.min(8) as usize;
        // :30563 — the divisor is the POST-decrement counter itself.
        let divisor = (e.f26 as i32).max(1);
        // :30538-45 — the flatten is BUFFERED: one goal-delta per
        // cell of the LEVEL row's rect, zeroed each work tick, rows
        // 1..=level each writing `goal − height` at their centered
        // offset, so a cell under several rows keeps the LAST row's
        // delta. Stepping the map row-by-row instead replays every
        // stale inner-level sculpt against the standing terrain — an
        // L3 courtyard byte drags a cell toward the datum while the
        // L3 ring byte hauls it back, every tick (the mc1l0
        // t=3856-73 apron dip the truth channel never shows).
        let ldef = self.assets.build_tab[level % self.assets.build_tab.len()];
        let (lw, lh) = (ldef.w as usize, ldef.h as usize);
        let mut deltas = vec![0i16; lw * lh];
        for r in 1..=level {
            self.fill_castle_goal_row(r, cx, cy, target, ldef, &mut deltas);
        }
        // The apply pass (:30550-70), one sweep over the level rect:
        // a cell whose (surviving) goal equals its height is not
        // written AT ALL; the water flip tests the HEIGHT (:30558),
        // pre-step, and retiles dig-mode (:30561); height steps by
        // delta/divisor. Counter 1 parks the moved protected cells
        // at pending-0x08 (:30565-69 — the finish re-promotes);
        // counter 2 sweeps bit 3 off the whole rect (:30571-72).
        let x0 = cx.wrapping_sub((ldef.w >> 1) as u8);
        let y0 = cy.wrapping_sub((ldef.h >> 1) as u8);
        for gy in 0..lh {
            for gx in 0..lw {
                let (x, y) = (x0.wrapping_add(gx as u8), y0.wrapping_add(gy as u8));
                let t = tile(x, y);
                let d = deltas[gy * lw + gx] as i32;
                if d != 0 {
                    if self.t.height[t] == 0 {
                        self.t.angle[t] = (self.t.angle[t] & 0xF8) | 1;
                        self.recompute_unprotected(x, y, x, y);
                    }
                    self.t.height[t] = self.t.height[t].wrapping_add((d / divisor) as u8);
                    if divisor == 1 && self.t.angle[t] & 0x80 != 0 {
                        self.t.angle[t] = (self.t.angle[t] & 0x77) | 8;
                    }
                }
                if divisor == 2 {
                    self.t.angle[t] &= !8;
                }
            }
        }
        // THE CASTLE WEAPON (sub_40E20 :51729, called per footprint
        // tile per paint tick :30631-34): the rising transformation
        // EXECUTES what stands on it — but only under the upgrade-
        // commit painter (the +18&1 kill bit, :56492); the damage
        // repaint kills nothing.
        if e.flags & 0x10000 != 0 {
            self.build_footprint_kill(level, cx, cy, e.id24);
        }
        // :30646 — retail SKIPS when `f26 % 7 && f26 != 1`.
        if e.f26 % 7 == 0 || e.f26 == 1 {
            for r in 1..=level {
                self.paint_build_row(r, cx, cy);
            }
        }
    }

    /// The castle painter's finish (:30697-709): PROMOTE pending
    /// protection — only tiles carrying bit 0x08 flip to 0x80;
    /// unpainted cells of the RLE footprint stay unprotected.
    fn finish_castle_painter(&mut self, i: usize) {
        let e = self.ent[i];
        let cx = ((e.x as u32 + 128) >> 8) as u8;
        let cy = ((e.y as u32 + 128) >> 8) as u8;
        let level = e.f71.min(8) as usize;
        let def = self.assets.build_tab[level % self.assets.build_tab.len()];
        let x0 = cx.wrapping_sub((def.w >> 1) as u8);
        let y0 = cy.wrapping_sub((def.h >> 1) as u8);
        for dy in 0..def.h {
            for dx in 0..def.w {
                let t = tile(x0.wrapping_add(dx), y0.wrapping_add(dy));
                if self.t.angle[t] & 8 != 0 {
                    self.t.angle[t] = (self.t.angle[t] & 0x77) | 0x80;
                }
            }
        }
        if let Some(c) = self.castle_at_site(e.x, e.y) {
            self.ent[c].f59 = 5;
        }
        self.ent[i].flags |= 0x400;
    }

    /// The castle a build worker (m41/m42) serves: retail links it by
    /// slot in the worker's +42 — a field the port entity does not
    /// carry — so the port re-derives it from the worker's spawn
    /// position, which IS the castle's site corner (unique: the
    /// placement scan enforces 8-tile spacing between castles).
    fn castle_at_site(&self, x: u16, y: u16) -> Option<usize> {
        (1..self.ent.len()).find(|&c| {
            let e = &self.ent[c];
            e.class64 == 3 && e.model65 == 2 && e.flags & 0x400 == 0 && e.x == x && e.y == y
        })
    }

    /// sub_28200 (:30284), byte70 43: the castle ground LEVELER — a
    /// uniform vertical TRANSLATION of the whole sculpted footprint,
    /// never a flatten: each tick every w*h tile gets the SAME signed
    /// step, so the painted tower rides along with the base. Init
    /// (:30429-41): counter (+26) = 10, current (+48, ours f28) =
    /// event z>>5, target (+44) = the OUTSIDE 4-corner average
    /// sub_361C0(x0-1, y0-1, h+2, w+2) clamped 220; already equal →
    /// straight to finish. Stepping (:30333-36): step = (target -
    /// current) / counter (signed truncating div), current += step;
    /// counter 10..2 add step to all tiles (:30386-416); counter 1
    /// adds + downgrades protection 0x80→0x08 (:30337-62) then
    /// counter = -10; -10..-2 idle; -1 restores 0x08→0x80
    /// (:30363-85). Finish (counter 0, :30419-27): castle sub-state
    /// 2, castle site z = 32*current, perimeter smooth depth 3,
    /// despawn. (The original also aborts to finish when castle +50
    /// [rebuild-pending] goes nonzero — field unported, always 0.)
    fn tick_castle_leveler(&mut self, i: usize) {
        let e = self.ent[i];
        let cx = ((e.x as u32 + 128) >> 8) as u8;
        let cy = ((e.y as u32 + 128) >> 8) as u8;
        let def = self.assets.build_tab[e.f71 as usize % self.assets.build_tab.len()];
        let x0 = cx.wrapping_sub((def.w >> 1) as u8);
        let y0 = cy.wrapping_sub((def.h >> 1) as u8);
        if e.flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.ent[i].f26 = 10;
            let cur = e.z >> 5;
            self.ent[i].f28 = cur as u16;
            let mut tgt = self.avg4(
                x0.wrapping_sub(1),
                y0.wrapping_sub(1),
                def.h.wrapping_add(2),
                def.w.wrapping_add(2),
            );
            if tgt > 220 {
                tgt = 220;
            }
            self.ent[i].f44 = tgt;
            if cur == tgt as i16 {
                self.ent[i].f26 = 0;
            }
            return;
        }
        let counter = self.ent[i].f26;
        if counter != 0 {
            let step = (self.ent[i].f44 as i32 - self.ent[i].f28 as i16 as i32) / counter as i32;
            self.ent[i].f28 = (self.ent[i].f28 as i16 as i32 + step) as i16 as u16;
            let add = |g: &mut Self, unstamp: bool| {
                for gy in 0..def.h {
                    for gx in 0..def.w {
                        let t = tile(x0.wrapping_add(gx), y0.wrapping_add(gy));
                        if unstamp && g.t.angle[t] & 0x80 != 0 {
                            g.t.angle[t] = (g.t.angle[t] & 0x77) | 8;
                        }
                        g.t.height[t] = (g.t.height[t] as i32 + step) as u8;
                    }
                }
            };
            if counter == 1 {
                add(self, true);
                self.ent[i].f26 = -10;
            } else if counter == -1 {
                for gy in 0..def.h {
                    for gx in 0..def.w {
                        let t = tile(x0.wrapping_add(gx), y0.wrapping_add(gy));
                        if self.t.angle[t] & 8 != 0 {
                            self.t.angle[t] = (self.t.angle[t] & 0x77) | 0x80;
                        }
                    }
                }
                self.ent[i].f26 += 1;
            } else if counter < 0 {
                self.ent[i].f26 += 1;
            } else {
                add(self, false);
                self.ent[i].f26 -= 1;
            }
        } else {
            if let Some(c) = self.castle_at_site(e.x, e.y) {
                self.ent[c].f59 = 2;
                // Castle SITE z (+154) = 32 * final — the next
                // build's datum (:30424); the entity z refreshes
                // from live ground on its own tick.
                self.ent[c].site_z = 32 * self.ent[i].f28 as i16;
            }
            self.smooth_perimeter(cx, cy, (def.h >> 1) as u16, (def.w >> 1) as u16, 3);
            self.ent[i].flags |= 0x400;
        }
    }

    /// The level-init starting-castle terrain replay (the sub_279D0
    /// loop :54982-93): the cumulative build-row footprints stamped
    /// INSTANTLY (divisor-1 flatten + paint per row), protection
    /// promoted like the painter finish (:30697-707). Rival wizards
    /// with a nonzero level-tail castle level spawn on this.
    pub(crate) fn stamp_castle_terrain(&mut self, rows: usize, cx: u8, cy: u8, target: i32) {
        // `rows` is the castle LEVEL: rows 1..=level, matching
        // retail's one-pass-per-level walk over build rows 0..=level
        // (row 0 is empty). Level 0 therefore stamps NOTHING — the
        // loop and the protect-bit block below both degenerate.
        let rows = rows.min(8);
        for r in 1..=rows {
            self.flatten_build_row(r, cx, cy, target, 1, FlattenLaw::CastleInit);
            self.paint_build_row(r, cx, cy);
        }
        let def = self.assets.build_tab[rows % self.assets.build_tab.len()];
        let x0 = cx.wrapping_sub((def.w >> 1) as u8);
        let y0 = cy.wrapping_sub((def.h >> 1) as u8);
        for dy in 0..def.h {
            for dx in 0..def.w {
                let t = tile(x0.wrapping_add(dx), y0.wrapping_add(dy));
                if self.t.angle[t] & 8 != 0 {
                    self.t.angle[t] = (self.t.angle[t] & 0x77) | 0x80;
                }
            }
        }
    }

    /// sub_37150 (:43798) + the HP ladder: size a castle entity's
    /// extents and life to its level (level 0 keeps the ctor shell).
    pub(crate) fn castle_extents(&mut self, i: usize, lvl: u8) {
        if lvl >= 1 {
            let def = self.assets.build_tab[lvl as usize % self.assets.build_tab.len()];
            let e = &mut self.ent[i];
            e.f78 = 0xE000; // sub_37150's z-center marker (signed −8192)
            e.f80 = (((def.w as u16) << 8).wrapping_add(1280)) >> 1;
            e.f82 = (((def.h as u16) << 8).wrapping_add(1280)) >> 1;
            e.f84 = 0x4000;
        }
        let hp = Self::CASTLE_HP[(lvl as usize).min(7)];
        self.ent[i].max_life = hp;
        self.ent[i].act_life = hp as i32;
        self.ent[i].site_z = self.ent[i].z;
    }

    /// sub_293D0 (:31009), byte70 45: the castle UPGRADE token — the
    /// delivery receipt the upgrade ball morphs into at the castle.
    /// Strictly ONE armed tick (:31040-44 — every armed path frees
    /// the token the same tick): f26++, PRE-decrement life, then the
    /// bit-2 latch tests overlap against the OWNER'S BOUND castle
    /// (retail resolves wizext+50, NOT the token's own +146 — an
    /// imported token carries no link, which silently missed the
    /// delivery: mc1l0 t=1187/2472, castle flags want 78 got 14).
    /// Hit → ch5 mail {10, owner} (:31033-34); miss → the owner's
    /// m16 manifestation charge pin releases (sub_46D20(_, 0) →
    /// +48 = 0).
    fn tick_upgrade_token(&mut self, i: usize) {
        let trace = std::env::var_os("MGC_CASTLE_PIN_TRACE").is_some();
        let life = self.ent[i].act_life;
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        self.ent[i].act_life = life - 1;
        if trace {
            eprintln!(
                "[pin] t={} token slot {i} tick: life={life} flags={:#x} own={}",
                crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                self.ent[i].flags,
                self.ent[i].id24
            );
        }
        if life >= 0 && self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let own = self.ent[i].id24;
            // The wizext+50 stand-in: +50 is written only by the
            // level-up commit (:56484) and cleared by the removal
            // path (:56534), so the bound castle is the owner's
            // ESTABLISHED (3,2) — a fresh level-0 flag is unbound
            // and the delivery misses it.
            let castle = (1..self.ent.len()).find(|&c| {
                let e = &self.ent[c];
                e.class64 == 3
                    && e.model65 == 2
                    && e.id24 == own
                    && e.f26 > 0
                    && e.flags & 0x400 == 0
            });
            if let Some(c) = castle {
                if self.ent_overlap(i, c) {
                    if trace {
                        eprintln!(
                            "[pin] t={} token slot {i}: HIT castle {c}",
                            crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed)
                        );
                    }
                    self.ent[c].mail[5] = (10, own);
                    self.ent[i].flags |= 0x400;
                    return;
                }
            }
            // The miss releases the owner's charge pin.
            if trace {
                eprintln!(
                    "[pin] t={} token slot {i}: MISS own={own} castle={castle:?}",
                    crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed)
                );
            }
            self.release_castle_charge_pin(own);
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_46D20(_, 0) (:55949-71): zero the owner's Create-Castle
    /// charge pin (+48 → our f26). Retail resolves the token through
    /// the OWNER's wizext+708 off any owner-stamped entity; the
    /// Gen-side stand-in joins on the f144 owner tag, which every
    /// native mint/pickup/import stamps (a dropped (12,16) ground
    /// jar rides f144 = 0 and can't alias). Callers: the upgrade
    /// token's MISS (:31037), the homing ball's pool-full morph
    /// (:63513-15) and the create ball's launch-scan failure
    /// (:63614-16).
    pub(crate) fn release_castle_charge_pin(&mut self, own: u16) {
        if let Some(m) = (1..self.ent.len()).find(|&m| {
            let e = &self.ent[m];
            e.class64 == 12 && e.model65 == 16 && e.f144 == own && e.flags & 0x400 == 0
        }) {
            self.ent[m].f26 = 0;
        }
    }

    /// sub_47DD0 (:56617): castle mana capacity by level (level 0 =
    /// the pre-tower shell; player castles occupy 1..=7).
    pub(crate) const CASTLE_CAP: [i32; 8] =
        [5000, 10000, 20000, 40000, 80000, 160000, 320000, 30_000_000];

    /// sub_12C50 (:17616): the upgrade pre-clear — every house whose
    /// AABB overlaps the NEXT level's footprint grown by 256 is
    /// killed outright (life = -1 → the collapse walker evacuates).
    fn castle_upgrade_preclear(&mut self, i: usize) {
        let next = (self.ent[i].f26 + 1).clamp(1, 8) as usize;
        let def = self.assets.build_tab[next % self.assets.build_tab.len()];
        let half_w = ((((def.w as u16) << 8).wrapping_add(1280)) >> 1) as i32 + 256;
        let half_h = ((((def.h as u16) << 8).wrapping_add(1280)) >> 1) as i32 + 256;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if e.class64 == 10
                && e.model65 == 45
                && e.flags & 0x400 == 0
                && wd(e.x, x) < e.f80 as i32 + half_w
                && wd(e.y, y) < e.f82 as i32 + half_h
            {
                self.ent[j].act_life = -1;
            }
        }
    }

    /// sub_12D10 (:17643): the upgrade space gate — FAIL when
    /// another castle overlaps the next level's extents, or any
    /// tile on the four edges of the new footprint carries the
    /// protection bit (blocked/steep ground).
    pub(crate) fn castle_upgrade_space_ok(&self, i: usize) -> bool {
        let next = (self.ent[i].f26 + 1).clamp(1, 8) as usize;
        let def = self.assets.build_tab[next % self.assets.build_tab.len()];
        let half_w = ((((def.w as u16) << 8).wrapping_add(1280)) >> 1) as i32;
        let half_h = ((((def.h as u16) << 8).wrapping_add(1280)) >> 1) as i32;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if j != i
                && e.class64 == 3
                && e.model65 == 2
                && e.flags & 0x400 == 0
                && wd(e.x, x) < e.f80 as i32 + half_w
                && wd(e.y, y) < e.f82 as i32 + half_h
            {
                return false;
            }
        }
        let cx = ((x as u32 + 128) >> 8) as u8;
        let cy = ((y as u32 + 128) >> 8) as u8;
        let (htx, hty) = ((half_w >> 8) as i32, (half_h >> 8) as i32);
        let blocked = |gx: i32, gy: i32| {
            self.t.angle[tile((cx as i32 + gx) as u8, (cy as i32 + gy) as u8)] & 0x80 != 0
        };
        for gx in -htx..=htx {
            if blocked(gx, -hty) || blocked(gx, hty) {
                return false;
            }
        }
        for gy in -hty..=hty {
            if blocked(-htx, gy) || blocked(htx, gy) {
                return false;
            }
        }
        true
    }

    /// sub_46DB0 (:57023-32): direct ball absorption — an OWNED m39
    /// ball touching the castle empties into the store while the
    /// store sits below capacity (the whole ball lands; overflow is
    /// the ejector's business).
    fn castle_absorb(&mut self, i: usize) {
        if self.ent[i].f140 >= self.ent[i].f136 {
            return;
        }
        let own = self.ent[i].id24;
        for j in 1..self.ent.len() {
            if self.ent[j].class64 == 10
                && self.ent[j].model65 == 39
                && self.ent[j].flags & 0x400 == 0
                && self.ent[j].f144 == own
                && self.ent_overlap(i, j)
            {
                self.ent[i].f140 += self.ent[j].f140;
                self.ent[j].flags |= 0x400;
                // :56030-42 — retail returns after the FIRST absorbed
                // ball: one ball per (every-other) settled tick, not a
                // same-tick vacuum of the whole pile.
                return;
            }
        }
    }

    /// A wizard owner tag's team slot: PLAYER_TARGET = 0, a rival's
    /// entity slot = its player slot (wizext var_48 in the original).
    pub(crate) fn owner_team(&self, owner: u16) -> Option<u8> {
        if owner == crate::mc1::mobs::PLAYER_TARGET {
            return Some(0);
        }
        (owner != 0)
            .then(|| self.rival_ents.iter().position(|&e| e == owner))
            .flatten()
            .map(|s| s as u8)
    }

    /// sub_37A00 (:44266): the mana BALLOON entity (class 3 m3) —
    /// life 10000, speed 48, cargo capacity 10000, behavior row 9,
    /// sprite 169. The castle dispatcher overwrites the ctor's
    /// state 7 with the working state 9 (:56355).
    fn spawn_balloon(&mut self, x: u16, y: u16, z: i16, own: u16) -> Option<usize> {
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
            // The ch0 vulnerability bit (+28 = 1, :44283) — without
            // it area writes skip the balloon entirely.
            e.f28 = 1;
            e.row156 = 9;
            e.id24 = own;
            e.f144 = own;
        }
        // Linked at spawn like the ctor (sub_41CF0 :44284) — an
        // unlinked balloon hovering its home tile would be invisible
        // to the direct-hit cell scans.
        self.link(i, x, y, z);
        self.refill_life(i);
        // Balloon sprite = 169 + team (the castle dispatcher's
        // `+86 += var_48`, :56347).
        let team = self.owner_team(own).unwrap_or(0) as u16;
        self.set_sprite(i, 169 + team);
        Some(i)
    }

    /// sub_47400 (:56264): the balloon/guard dispatcher, run from
    /// the established castle every other tick (:56016-20). Fleet
    /// quota by level: (balloons, guards) = L1(1,0) L2(1,0) L3(1,4)
    /// L4(2,6) L5(2,14) L6(3,18) L7(3,34); shortfalls respawn at the
    /// castle (guards = class-5 m15, HP 512).
    ///
    /// THE BALLOON HALF WALKS A REGISTER, NOT A CENSUS (:56329-95):
    /// `for i in 0..quota` over the owner wizard's three `+52 + 2*i`
    /// slots ([`Gen::mc1_balloon_reg`]). Per index: an EMPTY slot
    /// spawns and gets NO targeting that pass (:56340-49 — the
    /// newborn parks at the flag with chase 0) and the dispatcher
    /// walks on WITHOUT retargeting that index; a dead one (life < 0)
    /// drops its cargo, frees, CLEARS the slot and likewise walks on,
    /// so the replacement is a pass late; a live state-9 one
    /// retargets ONLY on the stagger turn `castle+63 % quota == 0`
    /// (:56338) — between turns the stale +146 stands, even one
    /// pointing at a freed slot (the blind mover keeps stepping
    /// there). On a stagger turn the target DEFAULTS to the castle
    /// (:56341 — return/offload/hover-home), then is overridden to
    /// the nearest own claimed ball (3-D metric, sub_42390) while the
    /// balloon has cargo room. The census-full arm (houses + stored ≥
    /// capacity) bypasses the stagger and homes every live balloon
    /// every pass (:56333-35). No free ball → the castle default
    /// stands.
    ///
    /// INDEX IS THE LAW, and it is spawn order, never slot order:
    /// index 0 picks first and so takes the nearest ball, the two
    /// exclusions handed to `sub_46CA0` are the OTHER TWO register
    /// slots' live targets (:56377-80), and the cull frees the slots
    /// at index >= quota (:56399-411). mc1l42 t=17150 is the whole
    /// law in one tick: register [991, 199, 107], so 991 takes ball
    /// 161 and 107 is left ball 328 — a pool scan gets the same SET
    /// and hands them out backwards.
    fn castle_balloons(&mut self, i: usize) {
        const FLEET: [(usize, usize); 8] = [
            (0, 0),
            (1, 0),
            (1, 0),
            (1, 4),
            (2, 6),
            (2, 14),
            (3, 18),
            (3, 34),
        ];
        let own = self.ent[i].id24;
        let (bq, gq) = FLEET[self.ent[i].f26.clamp(0, 7) as usize];
        // MC2's dispatcher twin (sub_60400 EF:61405) has not been
        // register-verified against the binary, so it keeps the live-
        // census stand-in: an empty register plus the adoption pass
        // below reproduces the old slot-order walk exactly.
        let mc2 =
            matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) || no_balloon_reg();
        let mut reg = if mc2 {
            [0u16; 3]
        } else {
            let mut r = [0u16; 3];
            if let Some(v) = self.mc1_balloon_reg.0.get(&own) {
                for (k, s) in v.iter().take(3).enumerate() {
                    r[k] = *s;
                }
            }
            r
        };
        // A register entry can only go stale through a NON-retail
        // path (a balloon freed at its own tick by `balloon_tick`, a
        // forged test entity, a pool import): retail's own writers
        // clear the slot as they free it. Clear those, then ADOPT any
        // live owned balloon the register does not name into the
        // first empty index in slot order — the fill order retail's
        // own spawns produce, and the recovery that keeps an orphaned
        // fleet (castle death, cf. docs/DEVIATIONS.md) steerable.
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
        let mut house_tally = 0i64;
        let mut orphans: Vec<usize> = Vec::new();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if e.flags & 0x400 != 0 {
                continue;
            }
            match (e.class64, e.model65) {
                (3, 3) if e.id24 == own && !reg.contains(&(j as u16)) => orphans.push(j),
                (10, 45) if e.f144 == own => house_tally += e.f140.max(0) as i64,
                _ => {}
            }
        }
        for b in orphans {
            let Some(k) = reg.iter().position(|&s| s == 0) else {
                break;
            };
            reg[k] = b as u16;
        }
        // The register microscope: `--env MGC_BALLOON_REG_TRACE=1`
        // prints the walk order a pass is about to use, to diff
        // against `dump-state <t> wiz`'s `breg=` (the recorded
        // wizext+52 triple).
        if std::env::var_os("MGC_BALLOON_REG_TRACE").is_some() {
            eprintln!(
                "breg castle={i} own={own} bq={bq} f63={} reg={reg:?}",
                self.ent[i].f63
            );
        }
        let (cx, cy, cz) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let full = house_tally + self.ent[i].f140.max(0) as i64 >= self.ent[i].f136.max(0) as i64;
        // THE STAGGER (:56338): the ball re-pick runs only on passes
        // where castle+63 % quota == 0 — between turns every balloon
        // keeps its stale +146 (even one pointing at a freed slot;
        // the blind mover keeps flying there). The modulus is the
        // QUOTA, not the live-fleet size — same as the MC2 twin
        // (sub_60400 EF:61405).
        let stagger = bq != 0 && self.ent[i].f63 as usize % bq == 0;
        for k in 0..bq.min(3) {
            if reg[k] == 0 {
                // Shortfall spawn (:56350-57): fills THIS index, and
                // the walk moves straight on — no targeting arm.
                if let Some(b) = self.spawn_balloon(cx, cy, cz, own) {
                    reg[k] = b as u16;
                }
                continue;
            }
            let b = reg[k] as usize;
            // The dead-reap (:56345-47): a balloon whose life went
            // negative outside its own tick (the one-frame linger, or
            // an imported mid-death seed) drops its cargo and frees
            // at DISPATCH time, and its index stays EMPTY for the
            // rest of the pass — the replacement is one pass late.
            if self.ent[b].act_life < 0 {
                self.corpse_drop(b);
                self.ent[b].flags |= 0x400;
                reg[k] = 0;
                continue;
            }
            if full {
                // The census-full arm bypasses the stagger and homes
                // every live balloon every pass (:56333-35).
                self.ent[b].f146 = i as u16;
                continue;
            }
            if !stagger || self.ent[b].tick70 != 9 {
                continue; // stale target stands (:56338-40)
            }
            // The castle default is written FIRST (:56341), then a
            // ball override while there is cargo room.
            self.ent[b].f146 = i as u16;
            if self.ent[b].f140 >= self.ent[b].f136 {
                continue; // cargo full → home
            }
            // The two exclusions are a DOUBLE INDIRECTION through the
            // neighbouring register slots (:56377-80):
            // `pool[pool[reg[(k+1)%3]].+146]` — the CURRENT target of
            // that slot, read live, so an earlier index's fresh pick
            // already blocks this one. The modulus is 3, never the
            // quota: on the pass after a downgrade the doomed slots'
            // stale targets still block, and an EMPTY register slot
            // indirects through pool[0], whose +146 retail zeroes
            // first (:56376) — the scratch record, never a ball.
            let ex0 = match reg[(k + 1) % 3] as usize {
                0 => 0,
                s => self.ent[s].f146 as usize,
            };
            let ex1 = match reg[(k + 2) % 3] as usize {
                0 => 0,
                s => self.ent[s].f146 as usize,
            };
            // Nearest own claimed ball (sub_46CA0 :55922) — 3-D
            // squared distance (sub_42390: wrapping i16 deltas incl.
            // z, compared UNSIGNED).
            let (bx, by, bz) = (self.ent[b].x, self.ent[b].y, self.ent[b].z);
            let mut best = 0usize;
            let mut best_d = u32::MAX;
            // sub_46CA0 (:55931-43) walks the TICK-TOP ball chain
            // (`var_u32_36462[1]`, the head the case-10 arm rebuilds
            // at :52296) and filters on MODEL 39 + owner ONLY — no
            // class byte, no act_life, no 0x400. A ball absorbed
            // EARLIER IN THIS TICK is still a member: sub_41E80
            // (:52508-11) is nothing but `flags |= 0x400`, and the
            // reclaim sub_41E90 (:52514-20) runs from the per-slot
            // walk only at the next tick's top (:52226-31). The mover
            // ticks at the balloon's slot and the dispatcher at the
            // castle's, so the dispatcher re-locks onto the corpse it
            // just drank (mc1l2 t=2409/2435/5443/5767). Dropping the
            // class test is decompile-mandated too, not cosmetic: the
            // mid-tick merge reclaim (sub_277D0 :29723+) clears
            // class64 but leaves model65/+144, and retail still picks
            // that record. HW twin :51986-52008 is identical.
            for c in 0..self.ball_chain.visible_len() {
                let j = self.ball_chain.list[c] as usize;
                let e = &self.ent[j];
                if e.model65 != 39 || e.f144 != own {
                    continue;
                }
                if j == ex0 || j == ex1 {
                    continue;
                }
                let dx = (e.x as i16).wrapping_sub(bx as i16) as i32;
                let dy = (e.y as i16).wrapping_sub(by as i16) as i32;
                let dz = (e.z).wrapping_sub(bz) as i32;
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
        // The cull tail (:56399-411) runs AFTER the targeting walk,
        // and it frees BY REGISTER INDEX: every slot at index >=
        // quota goes, cargo first spilled as an owned ball (sub_27690
        // spawns nothing for an empty balloon — only loaded culls
        // leave a ball behind). A shrunken quota (downgrade, or the
        // level-0 bare flag at quota 0) therefore drops the LATEST-
        // REGISTERED balloons, not the highest pool slots: mc1l42
        // t=18704 frees 107 (index 2 of [991, 199, 107]) and t=21264
        // frees 241, both of which our slot-order pop got backwards.
        // TOTAL castle death runs the same demolition from
        // castle_downgrade (retail orphans the fleet alive there —
        // see docs/DEVIATIONS.md).
        for k in bq.min(3)..3 {
            if reg[k] == 0 {
                continue;
            }
            let b = reg[k] as usize;
            self.corpse_drop(b);
            self.ent[b].flags |= 0x400;
            reg[k] = 0;
        }
        if !mc2 {
            self.mc1_balloon_reg.0.insert(own, reg.to_vec());
        }
        // Guard respawn (:56412-47) — driven by the wizext+84 GUARD
        // REGISTER ([`Gen::mc1_guard_reg`]), not a live census. Per
        // pass, after the +46 cooldown decrement, the walk visits the
        // first `gq` register slots in order: a STALE entry (not a
        // live (5,15), or the state-95 corpse) clears and RE-ARMS the
        // cooldown without spawning (the CARPET.EXE walk, disassembled
        // at obj1 :56448-51 — the delayed-first-guard law of mc1l1
        // t=2571); an EMPTY slot with the cooldown at 0 spawns ONE
        // guard at the castle's own position and relinks it to the
        // courtyard (x+128, y+640, ground), facing 512.
        if self.ent[i].f46 > 0 {
            self.ent[i].f46 -= 1;
        }
        // MC2 keeps the live-census stand-in: its dispatcher twin
        // (sub_60400 EF:61405) has not been register-verified against
        // the binary, its corpora measure identical under both forms,
        // and the cave goldens pin the census timing.
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            if gq > 0 && self.ent[i].f46 == 0 {
                let guards = (1..self.ent.len())
                    .filter(|&j| {
                        let e = &self.ent[j];
                        e.class64 == 5 && e.model65 == 15 && e.flags & 0x400 == 0 && e.id24 == own
                    })
                    .count();
                if guards < gq {
                    let gx = cx.wrapping_add(128);
                    let gy = cy.wrapping_add(640);
                    let gz = self.ground_z(gx, gy) as i16;
                    if let Some(g) = self.mc2_spawn_m15(gx, gy, gz) {
                        self.ent[g].id24 = own;
                        self.ent[g].f144 = own;
                        self.ent[g].f30 = 512;
                        self.ent[g].f34 = 512;
                        self.ent[i].f46 = 16;
                    }
                }
            }
        } else if gq > 0 {
            let mut reg = self
                .mc1_guard_reg
                .0
                .get(&own)
                .cloned()
                .unwrap_or_else(|| vec![0u16; 34]);
            for k in 0..gq.min(reg.len()) {
                let s = reg[k] as usize;
                if s != 0 {
                    let g = &self.ent[s];
                    if g.class64 != 5 || g.model65 != 15 || g.tick70 == 95 {
                        reg[k] = 0;
                        self.ent[i].f46 = 16;
                    }
                } else if self.ent[i].f46 == 0 {
                    // Both games park a (5,15) archer in the
                    // courtyard; the guard itself is per-column (MC2:
                    // mc2_spawn_m15, retail EF:61488 — spawning the
                    // MC1 creature under the MC2 dispatch was the
                    // class-5-model-15 misfit despawn).
                    let guard = match self.verbs.movement {
                        crate::verbs::MovementVerb::Mc2 => self.mc2_spawn_m15(cx, cy, cz),
                        _ => self.spawn_creature(15, cx, cy, cz),
                    };
                    if let Some(g) = guard {
                        self.ent[i].f46 = 16;
                        self.ent[g].id24 = own;
                        self.ent[g].f144 = own;
                        self.ent[g].f30 = 512;
                        self.ent[g].f34 = 512;
                        reg[k] = g as u16;
                        let gx = cx.wrapping_add(128);
                        let gy = cy.wrapping_add(640);
                        let gz = self.ground_z(gx, gy) as i16;
                        self.move_relink(g, gx, gy, gz);
                    }
                }
            }
            self.mc1_guard_reg.0.insert(own, reg);
        }
    }

    /// sub_47F90 (:56716): the BALLOON tick (class-3 m3 state 9).
    /// Ball target: >1024 away clears the ball's tether bit, near
    /// sets it (+ ball homes the balloon); touching absorbs the
    /// cargo and refreshes life; within one speed-step the balloon
    /// snaps over the ball. Castle target: within level·speed and
    /// low enough, the cargo empties into the castle store. All
    /// paths finish through the row-9 altitude servo (sub_42000
    /// params from the behavior row). Death drops the cargo as a
    /// claimed ball (the dispatcher's slot cleanup, :56368-72).
    pub(crate) fn balloon_tick(&mut self, i: usize) {
        self.balloon_move(i);
        // ch0 damage inbox at the tick's END (sub_481D0, reached via
        // LABEL_17 :56755-58 — movement/delivery FIRST, so the dock
        // pass's full heal precedes the damage: a balloon parked in
        // its castle ring is authentically near-invulnerable to chip
        // damage; they die in flight, or to a single lethal burst).
        if self.ent[i].mail[0].1 != 0 {
            let amt = self.ent[i].mail[0].0;
            self.ent[i].mail[0].1 = 0;
            self.ent[i].act_life -= amt as i32;
            // Balloon-under-attack flash (Type_160+393 = 4, :56826).
            if self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
                self.balloon_alert = 4;
            }
        }
        if self.ent[i].act_life < 0 {
            self.corpse_drop(i);
            self.ent[i].flags |= 0x400;
        }
    }

    fn balloon_move(&mut self, i: usize) {
        use crate::mc1::behavior::BEHAVIOR;
        let t = self.ent[i].f146 as usize;
        if t == 0 {
            return; // idle (:56814)
        }
        // THE MOVER IS BLIND (sub_47F90 :56735-36): the claim ticket
        // is dereferenced by the target's CLASS BYTE alone — no
        // liveness check, no model check. A ball freed mid-flight
        // (class 0, stale bytes) keeps the balloon stepping at the
        // corpse position — the ±48 y-bounce across a freed ball's
        // tile, angle(0,±48) flipping 0/1024 each tick. A slot
        // recycled into another class-10 hits the ball arm (retail's
        // latent absorb-the-recycled-record bug, :56742-73); a
        // recycled class-3 hits the castle arm; anything else is a
        // plain step at the stale bytes. The dispatcher un-sticks a
        // registered balloon only on its stagger turn.
        let mut pos = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let (tx, ty) = (self.ent[t].x, self.ent[t].y);
        let yaw = Self::angle_between(pos.0, pos.1, tx, ty);
        self.ent[i].f30 = yaw;
        let speed = self.ent[i].f126;
        let own = self.ent[i].id24;
        let mut step = true;
        if self.ent[t].class64 == 10 {
            if self.ent[t].f144 != own {
                step = false; // stale claim: hover (:56744)
            } else {
                let d = Self::isqrt(Self::dist2_sq(pos.0, pos.1, tx, ty) as u32) as i32;
                if d > 1024 {
                    self.ent[t].flags &= !0x40;
                } else {
                    self.ent[t].flags |= 0x40;
                    self.ent[t].f146 = i as u16;
                    if self.ent_overlap(i, t) {
                        let cargo = self.ent[t].f140;
                        let ball_owner = self.ent[t].f144;
                        self.ent[i].f140 += cargo;
                        self.ent[i].f144 = ball_owner;
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
            // Castle target: delivery ring = level * speed.
            let d = Self::isqrt(Self::dist2_sq(pos.0, pos.1, tx, ty) as u32) as i32;
            if d <= self.ent[t].f26 as i32 * speed as i32 {
                let ground = self.ground_z(pos.0, pos.1) as i16;
                if pos.2 <= ground.wrapping_add(BEHAVIOR[9].v_12) && self.ent[t].f26 > 0 {
                    pos.0 = tx;
                    pos.1 = ty;
                    let cargo = self.ent[i].f140;
                    self.ent[t].f140 += cargo;
                    self.ent[i].f140 = 0;
                    self.ent[i].f144 = own;
                    self.ent[i].act_life = self.ent[i].max_life as i32;
                }
                step = false;
            }
        }
        // Any other target class — including a freed slot's class-0
        // corpse — falls through to the plain step (:56807-09).
        if step {
            Self::polar_step(&mut pos, yaw, self.ent[i].f32, speed);
        }
        // The row-9 altitude servo + writeback (LABEL_17).
        let ground = self.ground_z(pos.0, pos.1) as i16;
        let mut z = pos.2;
        Self::alt_clamp(&mut z, ground, &BEHAVIOR[9]);
        self.move_relink(i, pos.0, pos.1, z);
    }

    /// sub_47C60 (:56572): castle max health by level (level 0 = 0 =
    /// keep the ctor's 40000). Levels 6/7 use the decompiler-mangled
    /// const `loc_13880` = 0x13880 = 80000. The carry-over rule on any
    /// level change (sub_47BD0 :56552-60): a NEGATIVE old life
    /// (overkill) is re-deducted from the new max, capped at half of
    /// it; positive life just resets to full.
    const CASTLE_HP: [u32; 8] = [40000, 20000, 40000, 40000, 60000, 60000, 80000, 80000];

    /// sub_46F10 (:56043): the class-3 m2 CASTLE state machine
    /// (sub-state f59 = the original's +48). Remaining housekeeping:
    /// the overflow ejector, downgrade/respawn. The entity z (+76)
    /// refreshes to live ground every tick (idle :56014 + wait
    /// cases 1/4/6 :56073-78) — the flag rides the painted tower;
    /// the build-site datum lives in f28 (+154).
    pub(crate) fn castle_tick(&mut self, i: usize, _patches: crate::patches::WorldPatches) {
        // ACTION 6, the LEVELER (sub_470E0 :56138). Lethal damage does
        // NOT downgrade on the tick it lands: `sub_47EC0` returning 2
        // only parks the castle here (:56003 `+70 = 6`) and the tick
        // ends; the downgrade runs on the NEXT dispatch. That is why
        // retail's castle is observably NEGATIVE for exactly one tick
        // — mc1l5 slot 312: act_life 450 at t=5757, −350 at t=5758,
        // 39650 (level 3 → 2) at t=5759. Collapsing the two into one
        // tick made the port skip the negative tick entirely, so a
        // besieged castle never died at the moment retail's did and
        // every mound holding it as a chase target kept chasing.
        // MC2's castle already models this on the same field
        // (mc2::castle, actions 4/5/6); the `f59` sub-state machine
        // below is MC1's ACTION-4 body. No ground refresh here —
        // sub_470E0 does none.
        if self.ent[i].tick70 == 6 {
            self.ent[i].tick70 = 4;
            self.castle_downgrade(i);
            return;
        }
        // The ground refresh belongs to the established tick and the
        // pure waits ONLY (:56013 + cases 1/4/6 :56073-78) — the
        // action cases keep the stale z: the level-up commit tick
        // still shows the ctor's raw-point ground (mc1l0 t=563:
        // z 797 held while the corner reads 864).
        if matches!(self.ent[i].f59, 1 | 4 | 6) {
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            self.ent[i].z = self.ground_z(x, y) as i16;
        }
        match self.ent[i].f59 {
            // Level-up (sub_47960 :56461, case 0 :56053-72): the
            // house pre-clear + (for standing castles) the space
            // gate — a reject bounces back to established with no
            // sound (the cast-time fizzle was the only failure
            // audio). Extents from build row = level (sub_37150
            // :43798; its +78=0xE000 marker skipped — it would
            // z-orphan our AABB overlaps), the loop-10 build gong,
            // the m42 painter, and the capacity ladder (sub_47C60 →
            // sub_47DD0 :56617).
            0 => {
                // The commit consumes the upgrade request (:56475) —
                // without the clear, the settled tick's flag check
                // re-launches the level-up forever.
                self.ent[i].flags &= !0x40;
                self.castle_upgrade_preclear(i);
                if self.ent[i].f26 > 0 && !self.castle_upgrade_space_ok(i) {
                    self.ent[i].f59 = 2;
                    return;
                }
                let (x, y, own, site_z) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.id24, e.site_z)
                };
                // The painter targets the build-site datum (+154),
                // not the live tower-top ground (sub_47020 spawns at
                // the site triple). The WHOLE level-up commit lives
                // inside sub_47960's `if (v1)` on this spawn
                // (:56471-93): a pool-full failure changes nothing
                // and case 0 retries next tick. Committing (or
                // advancing to the wait) without a painter deadlocks
                // the castle under meteor pool exhaustion.
                // The first-commit latch (:56057-62): flags bit 1 +
                // the one-time team sprite stamp (+86 += wizard +48).
                // The port keeps the ctor's sprite row — team art is
                // the renderer's pose.team lane; the flag bit is the
                // retail-visible half.
                if self.ent[i].flags & 2 == 0 {
                    self.ent[i].flags |= 2;
                }
                let Some(p) = self.spawn_creator(42, x, y, site_z) else {
                    return;
                };
                let lvl = (self.ent[i].f26 + 1).clamp(1, 8);
                self.ent[i].f26 = lvl;
                self.ent[i].f136 = Self::CASTLE_CAP[(lvl as usize).min(7)];
                let hp = Self::CASTLE_HP[(lvl as usize).min(7)];
                self.ent[i].max_life = hp;
                self.ent[i].act_life = hp as i32;
                let def = self.assets.build_tab[lvl as usize % self.assets.build_tab.len()];
                {
                    let e = &mut self.ent[i];
                    // sub_37150 writes the +78=0xE000 z-center marker
                    // with the extents: the castle's collision column
                    // is centered 8192 BELOW the flag, which is how
                    // ground-level area damage (napalm burns) reaches
                    // it. ent_overlap reads +78 signed.
                    e.f78 = 0xE000;
                    e.f80 = (((def.w as u16) << 8).wrapping_add(1280)) >> 1;
                    e.f82 = (((def.h as u16) << 8).wrapping_add(1280)) >> 1;
                    e.f84 = 0x4000;
                }
                self.snd(10, i);
                {
                    // Retail stamps the castle link into the painter's
                    // +42 (:56484-91) — a field the port does not carry;
                    // the workers re-derive their castle by SITE (unique
                    // by the 8-tile spacing law). f146 stays 0 like the
                    // recorded painters.
                    let e = &mut self.ent[p];
                    e.f71 = lvl as u8;
                    e.id24 = own;
                    e.flags |= 0x10000; // +18 |= 1 (:56492)
                }
                // WAIT in sub-state 1 (the original's pure-wait
                // :56073) — NOT established. Damage/demolish/upgrade
                // mail accrue untouched until the leveler hands back
                // state 4: the original's standing tick is the ONLY
                // damage processor (sub_47EC0 runs from +70=4 alone).
                // Processing lethals mid-transformation orphans the
                // tower (a downgrade collapse under a still-running
                // painter) and erases the authentic between-
                // transformations upgrade window (the dragon-squat
                // survival trick).
                self.ent[i].f59 = 1;
            }
            // Painter done → the m41 ground leveler (case 5,
            // sub_47080 :56119-35), then wait in sub-state 6 — the
            // original's real flow (:56132; cases 1/4/6 are pure
            // waits, :56073-78).
            5 => {
                let (x, y, z, own, lvl) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.site_z, e.id24, e.f26)
                };
                // sub_47080 advances only inside `if (result)`
                // (:56126-33) — a failed leveler spawn leaves the
                // case to retry next tick.
                if let Some(l) = self.spawn_creator(41, x, y, z) {
                    {
                        let e = &mut self.ent[l];
                        e.f71 = lvl as u8;
                        e.id24 = own;
                    }
                    self.ent[i].f59 = 6; // authentic wait state (:56132)
                }
            }
            // Leveler done → established (case 2 → sub_46DB0).
            2 => self.ent[i].f59 = 4,
            // Blast-shake expiry → the damage REPAINT (sub_47020
            // :56100-15): a painter at the CURRENT level with the
            // kill bit CLEAR — it re-stamps the tower and kills
            // nothing (:56492 sets the bit only on the upgrade
            // commit).
            3 => {
                let (x, y, own, site_z, lvl) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.id24, e.site_z, e.f26)
                };
                // sub_47020 advances only inside `if (result)`
                // (:56107-13) — a failed repaint spawn retries.
                if let Some(p) = self.spawn_creator(42, x, y, site_z) {
                    {
                        let e = &mut self.ent[p];
                        // The repaint row is the level VERBATIM
                        // (sub_47020 :56104 `+71 = +26`).
                        e.f71 = lvl.min(8) as u8;
                        e.id24 = own;
                    }
                    self.ent[i].f59 = 1; // wait for the repaint painter
                }
            }
            // Established (sub_46DB0 :55978): the blast-shake
            // countdown FREEZES everything else while it runs
            // (:55981-93 — the mailbox accrues, processing waits),
            // then the ch0 damage intake (sub_47EC0 :56678), the ch5
            // upgrade intake (:56690-95 — sender must be the owner,
            // max level 7), and the every-other-tick block
            // (:56016-37): overflow ejector, balloons, absorption.
            4 => {
                // The blast shake (:55983-99) is CHECK-then-decrement:
                // the ==1 tick transitions to the repaint (f50 zeroed
                // WITHOUT decrementing — the boundary shows 1 for a
                // full tick), a >1 tick only counts down (that arm is
                // the one the wrapper's pin census tags, pre50 >= 2).
                // Decrement-first fired the repaint one boundary early
                // — mc1l0 t=1294 vs 1295, the free-run entity-set
                // fork's extra (10,42) painter.
                if self.ent[i].f50 > 0 {
                    if self.ent[i].f50 == 1 {
                        self.ent[i].f50 = 0;
                        self.ent[i].f59 = 3;
                    } else {
                        self.ent[i].f50 -= 1;
                    }
                    return;
                }
                // sub_47EC0's first line (:56683): already below
                // zero → the leveler. This is also the demolish path
                // — Shift+L writes life = −1 with no mail at all
                // (:55846-50). Both lethal arms return 2 and :56003
                // turns a 2 into `+70 = 6` — but sub_46DB0 does NOT
                // return there: the owner echo and the whole
                // f63-even block below still run on the death-notice
                // tick (mc1l0 t=2310: the self-destructing castle at
                // life −1 SPAWNS balloon 484 through the dispatcher,
                // and the next tick's level-0 cull demolishes it —
                // the port's early return dropped the spawn). The
                // lethal arms skip only sub_47EC0's own tail (ch5
                // stays in the box) and the 0x40 else-if.
                let mut lethal = self.ent[i].act_life < 0;
                if !lethal && self.ent[i].mail[0].1 != 0 {
                    // sub_47EC0: HP -= pending ch0; lethal → the
                    // one-level downgrade, deferred through action 6,
                    // with the killer stamped into +38 (:56695-97).
                    let (amt, src) = self.ent[i].mail[0];
                    self.ent[i].mail[0].1 = 0;
                    self.ent[i].act_life -= amt as i32;
                    if self.ent[i].act_life < 0 {
                        // The lethal arm clears only the SOURCE
                        // (:56695-97) — the amount stands as residue,
                        // and sub_12B50 single hits ACCUMULATE onto
                        // it once the source is clear.
                        self.ent[i].f38 = src;
                        lethal = true;
                    } else {
                        self.ent[i].mail[0].0 = 0; // :56703
                        if self.ent[i].id24 == crate::mc1::mobs::PLAYER_TARGET {
                            // "Castle under attack" flash (Type_160+391=4).
                            self.castle_alert = 4;
                        }
                    }
                }
                if lethal {
                    self.ent[i].tick70 = 6;
                } else {
                    if self.ent[i].mail[5].1 != 0 {
                        let sender = self.ent[i].mail[5].1;
                        // The intake reads and clears ONLY the ch5
                        // source word (:56707-11) — the amount is
                        // never read and never cleared, so the
                        // token's `10` stands as permanent residue
                        // (mc1l0 t=1188+, castle 663 ch5 (10,0)).
                        self.ent[i].mail[5].1 = 0;
                        if sender == self.ent[i].id24 && self.ent[i].f26 < 7 {
                            // sub_47EC0 :56707-11 — the inbox arms the
                            // upgrade-request BIT (+16 |= 0x40), and the
                            // settled tick's own check below launches it.
                            self.ent[i].flags |= 0x40;
                        }
                    }
                    // sub_46DB0 :56007-11 — the request bit sends the
                    // settled castle into the level-up; the commit
                    // clears it (:56475). Checked as a FLAG (not a
                    // direct state write off the mail) so an imported
                    // castle captured between request and commit
                    // resumes correctly.
                    if self.ent[i].flags & 0x40 != 0 {
                        self.ent[i].f59 = 0;
                    }
                }
                // Every settled tick echoes the owner into +144
                // (sub_46DB0 :52080 `+144 = +24`) — the lane ball
                // claims and the balloon fleet join on.
                self.ent[i].f144 = self.ent[i].id24;
                if self.ent[i].f63 & 1 == 0 {
                    // The overflow ejector (sub_47130, called :56016):
                    // banked houses + stored over capacity spill out
                    // as owner-tagged wild-flying balls.
                    self.castle_eject(i);
                    // sub_37150 re-applied with the ejector every
                    // other tick (sub_46DB0 :52083, level VERBATIM —
                    // row 0 included): the extents + the +78=0xE000
                    // z-center marker self-heal to the current level,
                    // which keeps imported or stale castles
                    // collision-correct.
                    {
                        let lvl = self.ent[i].f26;
                        let def = self.assets.build_tab[lvl as usize % self.assets.build_tab.len()];
                        let e = &mut self.ent[i];
                        e.f78 = 0xE000;
                        e.f80 = (((def.w as u16) << 8).wrapping_add(1280)) >> 1;
                        e.f82 = (((def.h as u16) << 8).wrapping_add(1280)) >> 1;
                        e.f84 = 0x4000;
                    }
                    self.castle_balloons(i);
                    // Absorption sits inside the every-other-tick
                    // block in the original too (:57023-32).
                    self.castle_absorb(i);
                }
            }
            // 1 = waiting for a painter, 6 = waiting for the
            // leveler (the original's pure waits, :56073-78): the
            // mailbox and any pending lethal accrue untouched.
            _ => {}
        }
    }

    /// sub_47A70 (:56498) + the state-6 wrapper (sub_470E0 :56138):
    /// lethal damage knocks the castle DOWN one level — collapse
    /// rumble (sound 30), the over-cap spill ejected at a 10%
    /// capacity haircut, the footprint un-stamped to rough ground
    /// (the collapse walker's zeroed fake event, :56515-24), the
    /// ladder reset with the overkill carry, and then the WRAPPER
    /// TAIL (:56147-50): the ejector runs AGAIN at the new level and
    /// the fleet dispatch re-quotas the balloons, before the 5-tick
    /// timer into the repaint. At level 1 the whole castle dies
    /// instead (:56531-37) and the same tail is what scatters the
    /// ENTIRE bank (the ejector's level-0 all-stored arm, :56189-90)
    /// and demolishes the fleet (the level-0 quota cull, :56399-411)
    /// — the player is castle-less (die now = restart).
    ///
    /// ⚠ HISTORY: the tail was long mis-modeled as two opt-in
    /// patches (`castle_death_mana` / `castle_death_balloons`)
    /// claiming retail leaked the bank and orphaned the balloons —
    /// derived from sub_47A70's `!level` arm alone, missing that
    /// sub_470E0 calls sub_47130 + sub_47400 AFTER the teardown
    /// returns. The mc1l0 corpus refuted both in one tick: t=2217,
    /// castle slot 107 dies holding 8302 — retail scatters 8 balls
    /// of 1037 leaving residual 6 (= 8302 % 8, the ejector's own
    /// count/share arithmetic), spawns the ejector's 4 magnets,
    /// soft-kills the balloon (flags 0x400 with a cargo drop), and
    /// re-caps the dead flag's +136 to CAP[0] = 5000; t=1363 is the
    /// same law at 3000 → 3×1000, residual 0.
    fn castle_downgrade(&mut self, i: usize) {
        let lvl0 = self.ent[i].f26;
        let (x, y, site_z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.site_z)
        };
        // EVERYTHING down to the ladder reset sits inside retail's
        // `if (level > 0)` (:56506). A level-0 castle is a bare flag —
        // BUILD row 0 is empty (w = h = 0), so it never stamped any
        // terrain — and it takes the death arm alone. Without the
        // guard a level-0 death demolished a row-1 footprint that was
        // never built, knocking a phantom tower stump into the map.
        let lvl = if lvl0 > 0 {
            self.terrain_dirty = true; // the synchronous un-stamp below
            self.snd(30, i);
            // 10% capacity haircut scoped to THIS ejector call
            // (:56507-09, restored :56513) — it only widens the
            // collapse spill; the ladder reset below re-derives the
            // standing cap.
            let cut = 10 * self.ent[i].f136 / 100;
            self.ent[i].f136 -= cut;
            self.castle_eject(i);
            self.ent[i].f136 += cut;
            // The footprint un-stamp: a fake collapse event over the
            // CURRENT level's build row, run synchronously (sub_28FE0
            // direct call, :56524). The row is the level VERBATIM
            // (:56519 `+29866 = +26`) — never clamped.
            //
            // Retail builds this event in the SCRATCH slot (entity 0,
            // `dword_AE400_AE3F0() + 29795`, :56517-24) — it never
            // allocates, so the un-stamp cannot fail. Ours used to
            // take a pool slot with no else-arm, and `castle_eject`
            // immediately above can spend up to 36 of them: on a
            // pool-pressured level the demolish silently skipped the
            // terrain entirely and left the whole tower standing with
            // its flag gone — the reported symptom exactly. Slot 0 is
            // reserved here too (the free stack is built 999→1 and
            // every scan starts at 1), and its `rand` persists across
            // demolishes just like retail's scratch `+4`.
            {
                let e = &mut self.ent[SCRATCH];
                e.class64 = 10;
                e.model65 = 0; // zeroed model → z>>5 datum fallback
                e.f71 = lvl0.min(8) as u8;
                e.f26 = 0; // no evacuees on a castle (:56521)
                e.x = x;
                e.y = y;
                e.z = site_z;
                e.flags = 0;
            }
            self.tick_building_collapse(SCRATCH);
            self.ent[SCRATCH].class64 = 0;
            lvl0 - 1
        } else {
            lvl0
        };
        self.ent[i].f26 = lvl;
        // Ladder reset at the new level (sub_37150 :56527 + sub_47C60
        // → sub_47BD0) — INSIDE the `level > 0` guard, so it runs for
        // the death case too (level 1 → 0), but never for a bare
        // level-0 flag's death. The castle's own +136 write is
        // unconditional in the rung (:56567 `a1[34] = cap`), while
        // the HP arm is row-gated (:56547 `if (hp)`) and ROW 0
        // CARRIES HP 0 (:56586) — a dying castle re-caps to CAP[0] =
        // 5000 and keeps its negative life. Corpus: mc1l0 t=2217
        // castle 107 mana_max 9000 → 5000 with NO life rows.
        if lvl0 > 0 {
            self.ent[i].f136 = Self::CASTLE_CAP[(lvl as usize).min(7)];
            if lvl > 0 {
                let new_max = Self::CASTLE_HP[(lvl as usize).min(7)];
                let deficit = (-self.ent[i].act_life).clamp(0, new_max as i32 / 2);
                self.ent[i].max_life = new_max;
                self.ent[i].act_life = new_max as i32 - deficit;
            }
            let def = self.assets.build_tab[lvl as usize % self.assets.build_tab.len()];
            {
                let e = &mut self.ent[i];
                e.f78 = 0xE000; // sub_37150's z-center marker
                e.f80 = (((def.w as u16) << 8).wrapping_add(1280)) >> 1;
                e.f82 = (((def.h as u16) << 8).wrapping_add(1280)) >> 1;
            }
        }
        if lvl <= 0 {
            // Total destruction (:56531-37): the owner's castle
            // binding drops (ours is registry/scan-derived) and the
            // entity soft-kills — the wrapper tail below still runs
            // on it this tick, exactly like retail's freed-but-live
            // record. (The sub_46D20(a1, 0) call in that arm is the
            // spell-16 charge-pin clear on the owner's Create Castle
            // manifestation slot — wizext +708 — not a balloon
            // release; the world-side death stamp handles the token.)
            self.ent[i].flags |= 0x400;
        }
        // The state-6 wrapper's tail (sub_470E0 :56147-50) — BOTH
        // outcomes: the ejector runs AGAIN at the post-teardown
        // level (death: f26 == 0 → the WHOLE bank scatters through
        // the all-stored arm, :56189-90; survivor: the spill above
        // the new cap), then the fleet dispatch re-quotas the
        // balloons (death: the level-0 quota culls every one, cargo
        // dropped as an owned ball, :56399-411 — the corpus balloon
        // flags 1036). Order matters for the castle's LCG stream:
        // ejector draws (2 per ball + 4 magnet yaws) precede the
        // dispatch.
        self.castle_eject(i);
        self.castle_balloons(i);
        if lvl > 0 {
            // 5 ticks, then the repaint re-stamps the smaller castle
            // (:56158 +48=0/+50=5 → the state-4 countdown →
            // sub-state 3). A dead castle skips the timer — the
            // tick-top reap collects it first.
            self.ent[i].f50 = 5;
            self.ent[i].f59 = 4;
        }
    }

    /// sub_47130 (:56162): the castle mana EJECTOR. Spill = stored −
    /// capacity when houses + stored exceed capacity (ALL stored for
    /// a level-0/dying castle), thrown as 1..=32 owner-tagged balls
    /// of spill/count each, teleported 15-35 tiles out at random
    /// yaws with an upward pop, plus 4 (10,54) mana magnets at 25
    /// tiles (their ch4 pull/claim runs live via the ball tick's
    /// ch4 arm).
    fn castle_eject(&mut self, i: usize) {
        let stored = self.ent[i].f140;
        let cap = self.ent[i].f136;
        let mut spill = if self.banked_houses.saturating_add(stored) > cap {
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
        // :56194-205 — the throw count is ALSO capped by the pool
        // headroom (sub_37710_37AD0 = free slots + 1). The empty-pool
        // arm (:56196, reaper + retry at 8) is approximated by the
        // fail-open spawns below — the port frees eagerly, so its
        // stack never carries reapable soft-kills.
        let count = (spill / 1000).clamp(1, 32).min(self.free.len() as i32 + 1);
        let mut share = spill / count;
        let (cx, cy, cz, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        let ground = self.ground_z(cx, cy) as i16;
        for _ in 0..count {
            let Some(b) = self.spawn_mana_ball(cx, cy, cz) else {
                continue; // :56213 — a failed alloc skips the ball, not the loop
            };
            self.ent[b].f140 = share;
            self.ent[b].f144 = own;
            // Ball-seed draw → +126 (vestigial speed, kept for
            // stream parity); +150/152 velocity zeroed (:56221-23).
            let d = self.ent_rand(b);
            self.ent[b].f126 = (d % 0x30 + 16) as i16;
            self.ent[b].dest_x = 0;
            self.ent[b].dest_y = 0;
            // Upward pop scaled by how low the flag sits (:56227).
            self.ent[b].f46 = ((1024 - (cz.wrapping_sub(ground)) as i32) / 8) as i16;
            // Castle-seed draws: distance then yaw (:56231-37).
            let dist = (lcg32(&mut self.ent[i].rand) % 0x1400 + 3840) as i16;
            let yaw = (lcg32(&mut self.ent[i].rand) & 0x7FF) as u16;
            let mut pos = (cx, cy, cz);
            Self::polar_step(&mut pos, yaw, 0, dist);
            self.move_relink(b, pos.0, pos.1, pos.2);
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
        for _ in 0..4 {
            let dist = 6400i16;
            let yaw = (lcg32(&mut self.ent[i].rand) & 0x7FF) as u16;
            let mut pos = (cx, cy, cz);
            Self::polar_step(&mut pos, yaw, 0, dist);
            self.spawn_mana_magnet(pos.0, pos.1, pos.2, own);
        }
    }

    /// sub_3B970 (:47672): the (10,54) mana MAGNET — invisible,
    /// 128 ticks, not damageable. Its tick (sub_29920 :31234) stamps
    /// ch4 attract mail on every mana ball within ~14 tiles. Two
    /// spawners share it, exactly as in retail: the castle ejector
    /// (4 magnets at 25 tiles) and the Mana Magnet spell's bolt
    /// detonation (via `spawn_effect(54)`, the bolt's +68/+69 =
    /// 10/54, :66084-85 — that caller stamps the owner afterwards).
    pub(crate) fn spawn_mana_magnet(&mut self, x: u16, y: u16, z: i16, own: u16) -> Option<usize> {
        let s = self.new_event()?;
        {
            let e = &mut self.ent[s];
            e.class64 = 10;
            e.model65 = 54;
            e.tick70 = 59;
            e.max_life = 128;
            e.f126 = 256;
            e.f44 = 100;
            e.f26 = 0;
            // :47689 clears bit 3, :47697 sets bit 0 (the mc1l0
            // teardown corpus pins the spawn flags at 5 with the
            // caller's relink bit).
            e.flags &= !8;
            e.flags |= 1;
            e.id24 = own;
            let d = lcg32(&mut e.rand);
            e.f30 = (d & 0x7FF) as u16;
        }
        self.link(s, x, y, z);
        self.refill_life(s);
        {
            let e = &mut self.ent[s];
            e.f80 = 1024;
            e.f82 = 1024;
            e.f84 = 0x4000;
        }
        Some(s)
    }

    /// sub_29920 (:31234), byte70 59: the (10,54) magnet tick — life
    /// runs down, and every m39 ball within dist² < 12845056 (~14
    /// tiles, no owner filter — enemy balls pull too) gets ch4 mail
    /// {100, self} (a direct overwrite of +114/+118 = the ch4
    /// amount/source pair, :31255-57). The ball-side consumer (mc1
    /// ball_tick's ch4 arm) applies the pull impulse ONLY — pulled
    /// balls claim by merging, never by the pull.
    pub(crate) fn mana_magnet_tick(&mut self, i: usize) {
        // :31241-43 — PRE-decrement life test: 129 magnet passes over
        // the 128 life, not 128. Quiet, but the same law.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i64).abs();
        // The stamp walks the TICK-START ball chain (:31247 reads
        // `var_u32_36462[1]`), not the live pool: a ball ejected
        // mid-walk is invisible to every magnet until next tick's
        // rebuild ([`TickChain`]; mc1l0 castle-3 teardown, the
        // t=1830 ejected ball turns at 1832 not 1831), and a chain
        // severed by mid-tick slot reuse ends early for the stamp
        // exactly as for the acquire scans. The class/model recheck
        // guards the port's eager mid-tick free (a freed-not-reused
        // member keeps its link in retail but must not be stamped
        // fresh mail here).
        for k in 0..self.ball_chain.visible_len() {
            let j = self.ball_chain.list[k] as usize;
            if self.ent[j].class64 == 10
                && self.ent[j].model65 == 39
                && self.ent[j].flags & 0x400 == 0
            {
                let (dx, dy) = (wd(self.ent[j].x, x), wd(self.ent[j].y, y));
                if dx * dx + dy * dy < 12_845_056 {
                    self.ent[j].mail[4] = (100, i as u16);
                }
            }
        }
    }

    /// sub_3B620 (:47477): the (10,40) GRAVE a dying wizard leaves —
    /// sprite 65, ch1 (possession) mask only, f26 = slot % 11.
    pub(crate) fn spawn_grave(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let s = self.new_event()?;
        {
            let e = &mut self.ent[s];
            e.class64 = 10;
            e.model65 = 40;
            e.tick70 = 42;
            e.f26 = (s % 11) as i16;
            e.f28 = 2;
        }
        self.link(s, x, y, z);
        self.refill_life(s);
        self.set_sprite(s, 65);
        Some(s)
    }

    /// sub_275C0 (:29636), byte70 42: the grave tick — ground-snap,
    /// and a wizard-family possession claim (ch1) inherits EVERYTHING
    /// the grave owns (+144 == grave slot → claimant), then the grave
    /// vanishes. Reclaiming your own scattered bank after a death is
    /// exactly this possess.
    pub(crate) fn grave_tick(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        if self.ent[i].mail[1].1 != 0 {
            let claimant = self.ent[i].mail[1].1;
            self.ent[i].mail[1] = (0, 0);
            if self.attacker_is_wizard(claimant) && self.ent[i].f144 == 0 {
                for j in 1..self.ent.len() {
                    if self.ent[j].f144 == i as u16 && self.ent[j].class64 != 0 {
                        self.ent[j].f144 = claimant;
                        // Settled balls never re-run the tick's
                        // re-derive — recolor at the claim.
                        if self.ent[j].class64 == 10 && self.ent[j].model65 == 39 {
                            self.ball_resize(j);
                        }
                    }
                }
            }
            // sub_275C0 ends on the SOFT kill sub_41E80_421C0
            // (:29646-59) — `flags |= 0x400`, class and links intact
            // until the next tick's top sweep reclaims it (:52226-31).
            // The hard free lost the grave a tick (mc1l2: retail's
            // grave 18 shows flags 12 → 1036 at t=8302 and goes class
            // 0 only at 8303).
            self.ent[i].flags |= 0x400;
        }
    }

    /// sub_28DC0 (:30767), byte70 52: the LIVE village building.
    /// Damage intake sub_29640 (ch0; the decompile's u16 amount read
    /// is union slicing — writers store u32): non-lethal hits pop one
    /// militiaman (m4) out at (x+f80, y) while occupants +26 > 2, and
    /// put a wizard attacker on the village's wanted list (+528 =
    /// 200); death latches the killer and moves to state 53. Every 40
    /// ticks the mana pool +140 tracks occupants<<8, and a FULL house
    /// with capacity > 5 has a ~1/16 chance to emit a villager.
    /// The ch1 possession re-owner (:30801-14): claim the sender,
    /// chime 4, clear the active bit, swap to the claimed FLAG sprite
    /// — row 177 + the owner's color (:30808-09 adds the claimant
    /// wizext's var_48 straight onto +86/type86; the same per-team
    /// family mechanism as the claimed-ball rows in `ball_resize`).
    /// (An earlier reading took the `+86 +=` line for a mana credit —
    /// +86 is the sprite type field; there is no mana movement in the
    /// claim block.)
    pub(crate) fn tick_building_live(&mut self, i: usize, patches: crate::patches::WorldPatches) {
        if self.ent[i].act_life < 0 {
            // Killed directly (castle crush life = -1, :17638).
            self.ent[i].tick70 = 53;
            return;
        }
        if self.ent[i].mail[1].1 != 0 {
            let src = self.ent[i].mail[1].1;
            self.ent[i].mail[1] = (0, 0);
            if src != self.ent[i].f144 {
                self.ent[i].f144 = src;
                self.ent[i].flags &= !1;
                // Chime 4, anchored at the CLAIMANT (:30806-07 —
                // `sub_55370(claimant, -1, 4)`: the a2 = -1 arm plays
                // POSITIONALLY for ANY wizard, not just the local
                // player; the earlier player-only reading was the
                // per-call-site gate mis-read, see mgc-audio's
                // policy_mc1 notes). A rival's possession-claim is
                // audible when you are near the claimant.
                if src == crate::mc1::mobs::PLAYER_TARGET {
                    self.snd_player(4);
                } else if (src as usize) < self.ent.len() && self.ent[src as usize].class64 != 0 {
                    self.snd(4, src as usize);
                }
                // The owner-flag sprite — PRESERVING the building's
                // footprint extents (+78/80/82/84) under the
                // `possessed_footprint` patch. Retail's
                // sub_36FA0_37360(_,177) (:30808) overwrites +80 with the
                // tiny flag sprite's extent — and the villager-emit /
                // defender pop-out spawn at (x + f80). With the footprint
                // extent clobbered, that spawn point collapses from just
                // OUTSIDE the footprint to ON the roof, where the creature
                // is walled-in, dies, and its corpse-flame (400) destroys
                // the very house you just possessed (a self-sustaining
                // collapse). The retail arm lets the clobber stand —
                // see docs/DEVIATIONS.md.
                let (f78, f80, f82, f84) = {
                    let e = &self.ent[i];
                    (e.f78, e.f80, e.f82, e.f84)
                };
                // Owner recolor (:30808-09): flag row 177 + team color
                // (rows 177-184 are the eight team flags).
                let flag = 177 + self.owner_team(src).unwrap_or(0) as u16;
                self.set_sprite(i, flag);
                if patches.possessed_footprint {
                    let e = &mut self.ent[i];
                    e.f78 = f78;
                    e.f80 = f80;
                    e.f82 = f82;
                    e.f84 = f84;
                }
            }
        }
        if self.ent[i].mail[0].1 != 0 {
            let (amt, src) = self.ent[i].mail[0];
            self.ent[i].mail[0].1 = 0;
            // A CLAIMED BUILDING IS NOT IMMUNE TO ITS OWNER. The port
            // used to return here when `src == f144` ("as if they were
            // your castle"); that clause was invented, and its own note
            // admitted no substrate had been found. sub_29640 (:31070)
            // is short enough to settle it outright — `+40 = 0`, the
            // life test, the `+94` src gate, the subtract, the lethal
            // `+38` latch — and carries NO owner comparison of any
            // kind. Measured on mc1l2 t=5674: the human's own (10,0)
            // explosion (id24 295) lands 400 on his claimed house slot
            // 2 (f144 295), retail taking it to 1600 with `+40 = 295`
            // where the port held 2000 forever.
            self.ent[i].act_life -= amt as i32;
            if self.ent[i].act_life < 0 {
                self.ent[i].f38 = src;
                self.ent[i].tick70 = 53;
                return;
            }
            self.ent[i].f40 = src;
            if self.ent[i].f26 > 2 {
                self.ent[i].f26 -= 1;
                let (x, y, f80) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.f80)
                };
                let sx = x.wrapping_add(f80);
                let z = self.ground_z(sx, y) as i16;
                self.spawn_creature(4, sx, y, z);
                // The wanted arm rides INSIDE the occupied-house
                // branch (sub_28DC0 :30790-97) and only marks a
                // carpet-borne attacker (+40's model ≤ 1; the
                // out-of-pool player IS the carpet): torching an
                // emptied house (+26 ≤ 2) marks NOBODY. The
                // unconditional flag kept player_aggro alive through
                // the mc1l0 endgame — the t=4948 collapse-evac
                // militia acquired the human where retail's scan,
                // wanted 0, found no admissible target.
                if src == crate::mc1::mobs::PLAYER_TARGET
                    || self.ent.get(src as usize).is_some_and(|e| e.model65 <= 1)
                {
                    self.flag_village_wanted(src);
                }
            }
        }
        if self.ent[i].f63 % 40 == 0 {
            self.ent[i].f140 = (self.ent[i].f26 as i32) << 8;
            let cap = self.ent[i].f128;
            // EXACT equality (:30819), NOT `>=` — verbatim retail.
            // (The old rationale here said occupancy "only rises via
            // militia walk-ins"; there is no walk-in — that was the
            // port's own fabricated ladder rung, deleted with the
            // mc1l2 (5,4)+(10,45) family. Retail's only reachable
            // occupancy write in the mob range is the defender pop-out
            // `+26 = v2 - 1` at :30790, so occupancy FALLS.) `>=`
            // would make every full house emit forever, flooding the
            // level with villagers + loose mana until the pool
            // saturates.
            if cap > 5 && self.ent[i].f26 == cap {
                let d = self.ent_rand(i) % cap as u32;
                if d > (cap - cap / 16 - 2) as u32 {
                    self.building_emit(i);
                }
            }
        }
    }

    /// sub_28D10 (:30715): one villager emitted at (x+f80, y) —
    /// LCG%12: 0-1 militia m4, 2-3 migrant m14, 4-8 villager m13,
    /// 9-11 settler m12 (their natural spawn states 25/85/79/73).
    fn building_emit(&mut self, i: usize) {
        let d = self.ent_rand(i) % 12;
        let model = match d {
            0 | 1 => 4,
            2 | 3 => 14,
            4..=8 => 13,
            _ => 12,
        };
        let (x, y) = {
            let e = &self.ent[i];
            (e.x.wrapping_add(e.f80), e.y)
        };
        let z = self.ground_z(x, y) as i16;
        self.spawn_creature(model, x, y, z);
    }

    /// sub_28FE0 (:30835), byte70 53: the one-shot collapse. Walks
    /// the BUILD footprint once: per occupied cell an occupant
    /// evacuates (the LAST one is a settler m12, ≥4 remaining draw
    /// from the emit mix, otherwise a militiaman m4 — village defense
    /// IS the evacuation; spawn z drops 10 tiles every 8th STREAM
    /// byte, :30913-17). Per cell code hi nibble (:30940-93):
    /// 0 = unprotect only; 3 = unprotect + tower knock-down (-12
    /// AND -16 for sub-code 1, -16 for 2) + single-tile retexture;
    /// walls (1/2/4..7) = corner code forced to 1, single-tile
    /// retexture BEFORE the height drop (LCG%50 ≤ 20 → the full
    /// 4·(lo-1), else minus LCG%20 of it; at or below the wall
    /// height → 0). Finish = the full-rect 3x3 height smoother
    /// sub_36080 (:31004) and despawn. No mana spill. Base z =
    /// avg4 of the footprint corners when the event carries a model
    /// (:30879-81); the castle demolish path's zeroed fake event
    /// falls back to z>>5.
    pub(crate) fn tick_building_collapse(&mut self, i: usize) {
        let e = self.ent[i];
        let cx = ((e.x as u32 + 128) >> 8) as u8;
        let cy = ((e.y as u32 + 128) >> 8) as u8;
        let def = self.assets.build_tab[e.f71 as usize % self.assets.build_tab.len()];
        let (w, h) = (def.w as u16, def.h as u16);
        let (half_w, half_h) = ((w >> 1) as u8, (h >> 1) as u8);
        let x0 = cx.wrapping_sub(half_w);
        let y0 = cy.wrapping_sub(half_h);
        let base_h = if e.model65 != 0 {
            self.avg4(x0, y0, h as u8, w as u8) as i32
        } else {
            (e.z >> 5) as i32
        };
        let (z_hi, z_lo) = ((32 * base_h) as i16, (32 * (base_h - 10)) as i16);
        let mut rows = h;
        let (mut x, mut y) = (x0, y0);
        let mut c = def.offset as usize;
        // Stream position (the original's v2) — control bytes count.
        let mut pos = 0u32;
        while rows != 0 {
            let ctl = self.assets.build_dat[c] as i8;
            c += 1;
            pos += 1;
            if ctl == 0 {
                y = y.wrapping_add(1);
                rows -= 1;
                x = x0;
                continue;
            }
            if ctl < 0 {
                x = x.wrapping_add((-(ctl as i32)) as u8);
                continue;
            }
            for _ in 0..ctl {
                let b = self.assets.build_dat[c];
                c += 1;
                pos += 1;
                if b != 0 {
                    let t = tile(x, y);
                    // Evacuation (:30907-35): tile-corner position,
                    // low z every 8th stream byte.
                    let occ = self.ent[i].f26;
                    if occ > 0 {
                        self.ent[i].f26 = occ - 1;
                        let ez = if pos & 7 == 0 { z_lo } else { z_hi };
                        let wx = (x as u16) << 8;
                        let wy = (y as u16) << 8;
                        if occ == 1 {
                            self.spawn_creature(12, wx, wy, ez);
                        } else if occ - 1 >= 4 {
                            self.building_emit(i);
                        } else {
                            self.spawn_creature(4, wx, wy, ez);
                        }
                    }
                    // Rubble (:30940-93).
                    let hi = b >> 4;
                    let lo = b % 16;
                    if hi == 0 {
                        // Floors: unprotect, texture kept (:30994-95).
                        self.t.angle[t] &= !0x80;
                    } else if hi == 3 {
                        // Towers (:30974-93): unprotect, knock down,
                        // re-infer the tile. Sub-code 1 drops BOTH
                        // steps (decompile fall-through, verbatim).
                        self.t.angle[t] &= !0x80;
                        let sub = (lo % 16) % 3;
                        if sub == 1 && self.t.height[t] > 12 {
                            self.t.height[t] -= 12;
                        }
                        if (sub == 1 || sub == 2) && self.t.height[t] > 16 {
                            self.t.height[t] -= 16;
                        }
                        self.recompute_unprotected(x, y, x, y);
                    } else {
                        // Walls (:30944-71): corner code 1, retile
                        // BEFORE the height drop.
                        self.t.angle[t] = (self.t.angle[t] & 0x70) | 1;
                        self.recompute_unprotected(x, y, x, y);
                        if lo != 0 {
                            let full = 4 * (lo as i32 - 1);
                            if (self.t.height[t] as i32) <= full {
                                self.t.height[t] = 0;
                            } else {
                                let d = lcg32(&mut self.ent[i].rand);
                                let drop = if (d % 50) as i32 <= 20 {
                                    full
                                } else {
                                    full - (lcg32(&mut self.ent[i].rand) % 20) as i32
                                };
                                let hh = self.t.height[t] as i32;
                                self.t.height[t] = (hh - drop) as u8;
                            }
                        }
                    }
                }
                x = x.wrapping_add(1);
            }
        }
        // Finish (:31004): the full-rect vertex smoother over the
        // footprint (rows/cols exactly w x h, per-vertex sub_360C0 —
        // building-typed quads are self-excluding).
        for gy in 0..h {
            for gx in 0..w {
                self.smooth_cell(tile(x0.wrapping_add(gx as u8), y0.wrapping_add(gy as u8)));
            }
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_296A0 (:31097), byte70 56: the crab egg's incubation. Ground-
    /// snap z, run the act_life safety timeout (a pre-decrement read: it
    /// despawns only once act_life has already gone negative), then the
    /// f26 hatch timer the same way — f26 reaching 0 promotes to the
    /// hatch (state 57, an inert max_life the hatch never reads). No
    /// damage inbox; the egg dies only by timeout. No PRNG draws.
    pub(crate) fn tick_egg_incubate(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        let timer = self.ent[i].f26;
        self.ent[i].f26 = timer - 1;
        if timer == 0 {
            self.ent[i].tick70 = 57;
            self.ent[i].max_life = 5000;
        }
    }

    /// sub_29700 (:31120), byte70 57: the hatch. Ground-snap, spawn a
    /// WILD class-5 m5 crab at the snapped position (retail's crab ctor
    /// sets no owner — deliberately NOT inheriting the layer's), and —
    /// only if the crab took a slot — a class-10 m1 flash carrying the
    /// crab's id24; then despawn the egg unconditionally. The alloc
    /// order (crab, then flash, then free the egg) is retail's and feeds
    /// the pool-slot hash.
    pub(crate) fn tick_egg_hatch(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let z = self.ground_z(x, y) as i16;
        self.ent[i].z = z;
        if let Some(crab) = self.spawn_creature(5, x, y, z) {
            let owner = self.ent[crab].id24;
            if let Some(flash) = self.spawn_creator(1, x, y, z) {
                self.ent[flash].id24 = owner;
            }
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_33800 (:40980): paint one building tile. `a4 < 8` writes a
    /// terrain class + retexture; higher codes select {type,
    /// orientation} pairs from the paint tables and set the protection
    /// bit (plus clear bit 3 on the E/SE/S neighbors). Codes
    /// 0x14/0x15/0x16 are the white-wall DAMAGE stages (types
    /// 10/11/12 via PAINT_BC) — the fire cell's burn ladder.
    pub(crate) fn paint(&mut self, a1: i8, a2: i8, t: usize, a4: u8) {
        if a4 < 8 {
            self.t.angle[t] = a4 | (self.t.angle[t] & 0xF0);
            self.recompute_protected(tx(t), ty(t), tx(t), ty(t));
            return;
        }
        let checker = ((tx(t).wrapping_add(ty(t))) & 1) as usize;
        let pair: Option<[u8; 2]> = match a4 {
            8 => {
                self.t.tile_type[t] = 8;
                None
            }
            9 => {
                self.t.tile_type[t] = 9;
                None
            }
            10..=14 => {
                let (v, flat) = self.corner_orient(a1, a2, t);
                let idx = v as usize + if flat { 8 } else { 0 } + 16 * (a4 as usize - 10);
                Some(PAINT_FC[3 + idx / 8][idx % 8])
            }
            15 => {
                self.t.tile_type[t] = 11;
                None
            }
            16 => {
                let cur = self.t.tile_type[t];
                if matches!(cur, 10 | 11 | 12) {
                    None
                } else {
                    let (v, _) = self.corner_orient(cur as i8, a2, t);
                    Some(PAINT_AC[0][v as usize])
                }
            }
            17 => {
                let (v, _) = self.corner_orient(a1, a2, t);
                Some(PAINT_EC[0][v as usize])
            }
            18 => {
                let (v, _) = self.corner_orient(a1, a2, t);
                Some(PAINT_FC[checker][v as usize])
            }
            19 => {
                let (v, _) = self.corner_orient(a1, a2, t);
                Some(PAINT_FC[1 + checker][v as usize])
            }
            20..=22 => {
                let (v, _) = self.corner_orient(a1, a2, t);
                Some(PAINT_BC[a4 as usize - 20][v as usize])
            }
            _ => None,
        };
        if let Some([ty_val, ang]) = pair {
            self.t.tile_type[t] = ty_val;
            self.t.angle[t] = (self.t.angle[t] & 0x8F) | ang;
        }
        // Protection marks: claim this tile, clear bit 3 on E/SE/S.
        self.t.angle[t] = (self.t.angle[t] & 0x77) | 0x80;
        let (cx, cy) = (tx(t), ty(t));
        self.t.angle[tile(cx.wrapping_add(1), cy)] &= 0xF7;
        self.t.angle[tile(cx.wrapping_add(1), cy.wrapping_add(1))] &= 0xF7;
        self.t.angle[tile(cx, cy.wrapping_add(1))] &= 0xF7;
    }

    /// sub_33640 (:40870): corner orientation of a tile's height quad.
    /// `a1`/`a2` act as caller defaults for the max / runner-up corner
    /// indices. Returns (code 0..7, flat) where flat = max-min <= 8.
    fn corner_orient(&self, mut a1: i8, mut a2: i8, t: usize) -> (u8, bool) {
        let (cx, cy) = (tx(t), ty(t));
        let c = [
            self.t.height[t],
            self.t.height[tile(cx.wrapping_add(1), cy)],
            self.t.height[tile(cx.wrapping_add(1), cy.wrapping_add(1))],
            self.t.height[tile(cx, cy.wrapping_add(1))],
        ];
        let mut vmax = 0u8;
        if c[0] != 0 {
            vmax = c[0];
            a1 = 0;
        }
        let mut vmin = 0xFFu8;
        if c[0] != 0xFF {
            vmin = c[0];
        }
        for k in 1..4 {
            if c[k] > vmax {
                vmax = c[k];
                a1 = k as i8;
            }
            if c[k] < vmin {
                vmin = c[k];
            }
        }
        let mut v2nd = 0u8;
        if a1 != 0 && c[0] != 0 {
            v2nd = c[0];
            a2 = 0;
        }
        for k in 1..4 {
            if a1 != k as i8 && c[k] > v2nd {
                v2nd = c[k];
                a2 = k as i8;
            }
        }
        let flat = vmax.wrapping_sub(vmin) as i32 <= 8;
        if vmax as i32 - v2nd as i32 >= 8 {
            return ((a1 as u8) & 7, flat);
        }
        let code = match a1 {
            0 => {
                if a2 == 1 {
                    4
                } else {
                    7
                }
            }
            1 => {
                if a2 == 2 {
                    5
                } else {
                    4
                }
            }
            2 => {
                if a2 == 3 {
                    6
                } else {
                    5
                }
            }
            3 => {
                if a2 != 0 {
                    6
                } else {
                    7
                }
            }
            _ => 0,
        };
        (code, flat)
    }

    /// sub_35F30 (:42799): smooth a ring of thickness `thick`+1 around
    /// the footprint (left+right column strips interleaved, then
    /// top+bottom row strips interleaved), each cell via sub_360C0.
    fn smooth_perimeter(&mut self, cx: u8, cy: u8, half_h: u16, half_w: u16, thick: u8) {
        let left_x = cx.wrapping_sub(half_w as u8).wrapping_sub(thick);
        let right_x = cx.wrapping_add(half_w as u8);
        let top_y = cy.wrapping_sub(half_h as u8);
        for row in 0..(2 * half_h) {
            let y = top_y.wrapping_add(row as u8);
            for k in 0..=thick {
                self.smooth_cell(tile(left_x.wrapping_add(k), y));
                self.smooth_cell(tile(right_x.wrapping_add(k), y));
            }
        }
        let strip_x = cx.wrapping_sub(half_w as u8).wrapping_sub(thick);
        let top_strip_y = cy.wrapping_sub(half_h as u8).wrapping_sub(thick);
        let bot_strip_y = cy.wrapping_add(half_h as u8);
        for col in 0..(2 * thick as u16 + 2 * half_w) {
            let x = strip_x.wrapping_add(col as u8);
            for k in 0..=thick {
                self.smooth_cell(tile(x, top_strip_y.wrapping_add(k)));
                self.smooth_cell(tile(x, bot_strip_y.wrapping_add(k)));
            }
        }
    }

    /// sub_360C0 (:42892): if the cell is land and its NW 2x2 quad has
    /// no building/wall texture (types 6..=0x22), replace its height by
    /// the 3x3 average over similarly-plain cells. Index arithmetic is
    /// linear u16 (rows wrap into each other) — faithful.
    fn smooth_cell(&mut self, t: usize) {
        if self.t.angle[t] & 7 == 0 || self.t.height[t] == 0 {
            return;
        }
        let plain = |ty_val: u8| ty_val <= 5 || ty_val > 0x22;
        let quad = [
            (t.wrapping_sub(257)) & 0xFFFF,
            (t.wrapping_sub(256)) & 0xFFFF,
            (t.wrapping_sub(1)) & 0xFFFF,
            t,
        ];
        if !quad.iter().all(|&q| plain(self.t.tile_type[q])) {
            return;
        }
        let mut sum = 0u32;
        let mut n = 0u32;
        let mut idx = (t.wrapping_sub(257)) & 0xFFFF;
        for _ in 0..3 {
            for _ in 0..3 {
                if plain(self.t.tile_type[idx]) {
                    n += 1;
                    sum += self.t.height[idx] as u32;
                }
                idx = (idx + 1) & 0xFFFF;
            }
            idx = (idx + 253) & 0xFFFF;
        }
        if let Some(h) = sum.checked_div(n) {
            self.t.height[t] = h as u8;
        }
    }
}

// ------------------------------------------------------------ snapshot
//
// The save codec for everything defined in this module
// (`crate::snapshot`). It lives here rather than in that module
// because `Gen` and its members are `pub(crate)` with private
// internals, and — more usefully — because a new field should break
// the build next to the line that declared it.

use crate::snapshot::{Reader, Snap, SnapshotError, Writer};

impl Snap for Planes {
    fn put(&self, w: &mut Writer) {
        let Planes {
            height,
            tile_type,
            shading,
            angle,
            ceiling,
        } = self;
        w.put(height);
        w.put(tile_type);
        w.put(shading);
        w.put(angle);
        // Empty off-cave; the length prefix carries that by itself.
        w.put(ceiling);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Planes {
            height: r.get()?,
            tile_type: r.get()?,
            shading: r.get()?,
            angle: r.get()?,
            ceiling: r.get()?,
        })
    }
}

impl Snap for Ent {
    fn put(&self, w: &mut Writer) {
        let Ent {
            rand,
            max_life,
            act_life,
            flags,
            next20,
            prev22,
            id24,
            f38,
            f40,
            f46,
            f50,
            f68,
            f69,
            mail,
            f144,
            f26,
            f28,
            f30,
            f32,
            f44,
            f34,
            f36,
            f52,
            f54,
            f56,
            f58,
            f59,
            f63,
            class64,
            model65,
            f66,
            f67,
            tick70,
            f71,
            x,
            y,
            z,
            f78,
            f80,
            f82,
            f84,
            type86,
            frame88,
            frames89,
            f126,
            f128,
            f130,
            f136,
            f140,
            f146,
            row156,
            thing_slot,
            dest_x,
            dest_y,
            site_z,
        } = self;
        w.put(rand);
        w.put(max_life);
        w.put(act_life);
        w.put(flags);
        w.put(next20);
        w.put(prev22);
        w.put(id24);
        w.put(f38);
        w.put(f40);
        w.put(f46);
        w.put(f50);
        w.put(f68);
        w.put(f69);
        w.put(mail);
        w.put(f144);
        w.put(f26);
        w.put(f28);
        w.put(f30);
        w.put(f32);
        w.put(f44);
        w.put(f34);
        w.put(f36);
        w.put(f52);
        w.put(f54);
        w.put(f56);
        w.put(f58);
        w.put(f59);
        w.put(f63);
        w.put(class64);
        w.put(model65);
        w.put(f66);
        w.put(f67);
        w.put(tick70);
        w.put(f71);
        w.put(x);
        w.put(y);
        w.put(z);
        w.put(f78);
        w.put(f80);
        w.put(f82);
        w.put(f84);
        w.put(type86);
        w.put(frame88);
        w.put(frames89);
        w.put(f126);
        w.put(f128);
        w.put(f130);
        w.put(f136);
        w.put(f140);
        w.put(f146);
        w.put(row156);
        w.put(thing_slot);
        w.put(dest_x);
        w.put(dest_y);
        w.put(site_z);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        // A full literal, NOT `..Default::default()` — struct-update
        // syntax would make a forgotten field compile silently, which
        // is the entire failure mode this codec is shaped to prevent.
        Ok(Ent {
            rand: r.get()?,
            max_life: r.get()?,
            act_life: r.get()?,
            flags: r.get()?,
            next20: r.get()?,
            prev22: r.get()?,
            id24: r.get()?,
            f38: r.get()?,
            f40: r.get()?,
            f46: r.get()?,
            f50: r.get()?,
            f68: r.get()?,
            f69: r.get()?,
            mail: r.get()?,
            f144: r.get()?,
            f26: r.get()?,
            f28: r.get()?,
            f30: r.get()?,
            f32: r.get()?,
            f44: r.get()?,
            f34: r.get()?,
            f36: r.get()?,
            f52: r.get()?,
            f54: r.get()?,
            f56: r.get()?,
            f58: r.get()?,
            f59: r.get()?,
            f63: r.get()?,
            class64: r.get()?,
            model65: r.get()?,
            f66: r.get()?,
            f67: r.get()?,
            tick70: r.get()?,
            f71: r.get()?,
            x: r.get()?,
            y: r.get()?,
            z: r.get()?,
            f78: r.get()?,
            f80: r.get()?,
            f82: r.get()?,
            f84: r.get()?,
            type86: r.get()?,
            frame88: r.get()?,
            frames89: r.get()?,
            f126: r.get()?,
            f128: r.get()?,
            f130: r.get()?,
            f136: r.get()?,
            f140: r.get()?,
            f146: r.get()?,
            row156: r.get()?,
            thing_slot: r.get()?,
            dest_x: r.get()?,
            dest_y: r.get()?,
            site_z: r.get()?,
        })
    }
}

impl Snap for Rec {
    fn put(&self, w: &mut Writer) {
        let Rec {
            class,
            model,
            x,
            y,
            dis_id,
            swi_sz,
            swi_id,
            parent,
            child,
            par3,
        } = self;
        w.put(class);
        w.put(model);
        w.put(x);
        w.put(y);
        w.put(dis_id);
        w.put(swi_sz);
        w.put(swi_id);
        w.put(parent);
        w.put(child);
        // `par3` is hash-EXCLUDED but very much real state, so it is
        // saved. This is the class of field a hash-derived codec
        // would have dropped.
        w.put(par3);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Rec {
            class: r.get()?,
            model: r.get()?,
            x: r.get()?,
            y: r.get()?,
            dis_id: r.get()?,
            swi_sz: r.get()?,
            swi_id: r.get()?,
            parent: r.get()?,
            child: r.get()?,
            par3: r.get()?,
        })
    }
}

impl Snap for SoundEvent {
    fn put(&self, w: &mut Writer) {
        let SoundEvent {
            id,
            pos,
            tag,
            player,
        } = self;
        w.put(id);
        w.put(pos);
        w.put(tag);
        w.put(player);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(SoundEvent {
            id: r.get()?,
            pos: r.get()?,
            tag: r.get()?,
            player: r.get()?,
        })
    }
}

impl Snap for PalFlash {
    fn put(&self, w: &mut Writer) {
        let PalFlash { row, ticks } = self;
        w.put(row);
        w.put(ticks);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(PalFlash {
            row: r.get()?,
            ticks: r.get()?,
        })
    }
}

impl Snap for Mc2PlayerDebuffs {
    fn put(&self, w: &mut Writer) {
        let Mc2PlayerDebuffs { slow, stun } = self;
        w.put(slow);
        w.put(stun);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Mc2PlayerDebuffs {
            slow: r.get()?,
            stun: r.get()?,
        })
    }
}

/// Newtypes over one field: the wrapper adds a hash policy, never a
/// wire shape.
macro_rules! snap_newtype {
    ($($t:ty),* $(,)?) => {$(
        impl Snap for $t {
            fn put(&self, w: &mut Writer) {
                w.put(&self.0);
            }
            fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
                Ok(Self(r.get()?))
            }
        }
    )*};
}

snap_newtype!(
    SlotGens,
    Mc2LifeScale,
    NightShade,
    Mc2Ord,
    Mc2XpMail,
    Mc2StealMail,
    Mc2CastleResearch,
);

impl<const TAG: u8> Snap for Mc2Quiet<TAG> {
    fn put(&self, w: &mut Writer) {
        w.put(&self.0);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Self(r.get()?))
    }
}

impl<const TAG: u8> Snap for Mc2SlotMap<TAG> {
    fn put(&self, w: &mut Writer) {
        w.put(&self.0);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Self(r.get()?))
    }
}

impl Gen {
    /// The geometry a restore cannot paper over: pool and table sizes
    /// renumber every slot handle in the stream if they differ.
    pub(crate) fn snap_identity(&self, w: &mut Writer) {
        w.put(&self.chassis.pool_slots);
        w.put(&self.chassis.level_table_slots);
        w.put(&self.chassis.bucket_models);
        w.put(&self.chassis.win_streak_ticks);
        w.put(&self.chassis.awake_gate_sq);
        w.put(&self.chassis.ent_rand_width);
        w.put(&self.verbs);
        w.put(&self.ent.len());
        w.put(&self.t.height.len());
        w.put(&(!self.t.ceiling.is_empty()));
    }

    pub(crate) fn snap_check_identity(&self, r: &mut Reader) -> Result<(), SnapshotError> {
        r.expect("chassis.pool_slots", self.chassis.pool_slots)?;
        r.expect("chassis.level_table_slots", self.chassis.level_table_slots)?;
        r.expect("chassis.bucket_models", self.chassis.bucket_models)?;
        r.expect("chassis.win_streak_ticks", self.chassis.win_streak_ticks)?;
        r.expect("chassis.awake_gate_sq", self.chassis.awake_gate_sq)?;
        r.expect("chassis.ent_rand_width", self.chassis.ent_rand_width)?;
        r.expect("verbs", self.verbs)?;
        r.expect("pool size", self.ent.len())?;
        r.expect("terrain size", self.t.height.len())?;
        r.expect("cave ceiling", !self.t.ceiling.is_empty())?;
        Ok(())
    }

    pub(crate) fn snap_write(&self, w: &mut Writer) {
        let Gen {
            t,
            // Level-package data, re-supplied by the caller from the
            // reloaded bundle rather than carried in the save.
            assets: _,
            retile: _,
            map_entity,
            ent,
            slot_gen,
            free,
            rand,
            pseudo,
            spawn_count,
            player_mail,
            player_damage,
            erupting,
            plume,
            player_knock,
            // A per-tick transient the mover drains — see PlayerSpin.
            player_spin: _,
            mc2_debuffs,
            rival_ents,
            mc2_life_scale,
            player_aggro,
            rival_wanted,
            player_invisible,
            player_rebound,
            kills,
            shots,
            hits,
            player_danger,
            banked_houses,
            castle_alert,
            player_alert,
            balloon_alert,
            // Hash-SILENT always (presentation), so nothing in the
            // acceptance test would notice this being dropped — the
            // `slot_gen` class of field. Saved because it is still
            // state: a save taken mid-flash restores mid-flash.
            pal_flash,
            exhausted,
            // Fixed at construction and identity-checked above; the
            // `&'static [u8]` inside `chassis` is why they cannot
            // simply ride along.
            chassis: _,
            verbs: _,
            verb_fallbacks,
            misfits,
            sounds,
            terrain_dirty,
            mc2_night_shade,
            mc2_spawn_ord,
            mc2_player_drain,
            mc2_scrolls,
            mc2_spell_tokens,
            mc2_cast_xp,
            // Presentation feed, never saved — a load starts clean.
            bolt_fx: _,
            mc2_steal_mail,
            mc2_aura_claim,
            mc2_wanted,
            mc2_rebound_precise,
            mc2_allied,
            mc2_castle_research,
            // NEVER saved, and that IS the retail law: every load path
            // rebuilds the pool lists and then empties this one
            // outright (`sub_49F90(); D41A0_0.dword_0x11e6 = -1;` —
            // Level.cpp:304-305 / :423-424, EF:38829, :38874, :39467).
            // A restored world simply has no ranked victims until the
            // list next refreshes.
            mc2_recycle: _,
            mc1_guard_reg,
            mc1_balloon_reg,
            // Rebuilt at every tick top — never saved.
            ball_chain: _,
            mob_chains: _,
        } = self;
        w.put(t);
        w.put(map_entity);
        w.put(ent);
        w.put(slot_gen);
        // `free` is saved VERBATIM and never rebuilt from occupancy:
        // its ORDER is the pool economy (allocation pops the stack),
        // so a rebuilt stack would re-order future spawns even though
        // the set of free slots matched.
        w.put(free);
        w.put(rand);
        w.put(pseudo);
        w.put(spawn_count);
        w.put(player_mail);
        w.put(player_damage);
        w.put(erupting);
        w.put(plume);
        w.put(player_knock);
        w.put(mc2_debuffs);
        w.put(rival_ents);
        w.put(mc2_life_scale);
        w.put(player_aggro);
        w.put(rival_wanted);
        w.put(player_invisible);
        w.put(player_rebound);
        w.put(kills);
        w.put(shots);
        w.put(hits);
        w.put(player_danger);
        w.put(banked_houses);
        w.put(castle_alert);
        w.put(player_alert);
        w.put(balloon_alert);
        w.put(pal_flash);
        w.put(exhausted);
        w.put(verb_fallbacks);
        w.put(misfits);
        w.put(sounds);
        w.put(terrain_dirty);
        w.put(mc2_night_shade);
        w.put(mc2_spawn_ord);
        w.put(mc2_player_drain);
        w.put(mc2_scrolls);
        w.put(mc2_spell_tokens);
        w.put(mc2_cast_xp);
        w.put(mc2_steal_mail);
        w.put(mc2_aura_claim);
        w.put(mc2_wanted);
        w.put(mc2_rebound_precise);
        w.put(mc2_allied);
        w.put(mc2_castle_research);
        w.put(&mc1_guard_reg.0);
        w.put(&mc1_balloon_reg.0);
    }

    pub(crate) fn snap_apply(&mut self, r: &mut Reader) -> Result<(), SnapshotError> {
        self.t = r.get()?;
        self.map_entity = r.get()?;
        self.ent = r.get()?;
        self.slot_gen = r.get()?;
        self.free = r.get()?;
        self.rand = r.get()?;
        self.pseudo = r.get()?;
        self.spawn_count = r.get()?;
        self.player_mail = r.get()?;
        self.player_damage = r.get()?;
        self.erupting = r.get()?;
        self.plume = r.get()?;
        self.player_knock = r.get()?;
        self.mc2_debuffs = r.get()?;
        self.rival_ents = r.get()?;
        self.mc2_life_scale = r.get()?;
        self.player_aggro = r.get()?;
        self.rival_wanted = r.get()?;
        self.player_invisible = r.get()?;
        self.player_rebound = r.get()?;
        self.kills = r.get()?;
        self.shots = r.get()?;
        self.hits = r.get()?;
        self.player_danger = r.get()?;
        self.banked_houses = r.get()?;
        self.castle_alert = r.get()?;
        self.player_alert = r.get()?;
        self.balloon_alert = r.get()?;
        self.pal_flash = r.get()?;
        self.exhausted = r.get()?;
        self.verb_fallbacks = r.get()?;
        self.misfits = r.get()?;
        self.sounds = r.get()?;
        self.terrain_dirty = r.get()?;
        self.mc2_night_shade = r.get()?;
        self.mc2_spawn_ord = r.get()?;
        self.mc2_player_drain = r.get()?;
        self.mc2_scrolls = r.get()?;
        self.mc2_spell_tokens = r.get()?;
        self.mc2_cast_xp = r.get()?;
        self.mc2_steal_mail = r.get()?;
        self.mc2_aura_claim = r.get()?;
        self.mc2_wanted = r.get()?;
        self.mc2_rebound_precise = r.get()?;
        self.mc2_allied = r.get()?;
        self.mc2_castle_research = r.get()?;
        self.mc1_guard_reg.0 = r.get()?;
        self.mc1_balloon_reg.0 = r.get()?;
        // Presentation feed — never saved, never inherited across a
        // load.
        self.bolt_fx.0.clear();
        // Retail's load empties the victim list (see `snap_write`).
        self.mc2_recycle.stack.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_assets() -> FeatureAssets {
        // A tiny diamond ring grid centered at (15,15) mimicking
        // SEARCH.DAT's shape: ring = max(|dx|,|dy|) but with a 2x2 ring 0.
        let mut grid = vec![31u8; 1024];
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (dx, dy) = (x - 15, y - 15);
                let r = dx.max(dy).max(-dx + 1).max(-dy + 1) - 1;
                grid[(y * 32 + x) as usize] = r.clamp(0, 31) as u8;
            }
        }
        // One 4x4 building: plain floor (code 7) with a wall ring (0x10).
        let mut dat = Vec::new();
        for row in 0..4 {
            let inner = row == 1 || row == 2;
            dat.push(4u8);
            if inner {
                dat.extend_from_slice(&[0x10, 7, 7, 0x10]);
            } else {
                dat.extend_from_slice(&[0x10, 0x10, 0x10, 0x10]);
            }
            dat.push(0);
        }
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|_| {
                let mut e = 0u32.to_le_bytes().to_vec();
                e.push(4);
                e.push(4);
                e
            })
            .collect();
        FeatureAssets::parse(&grid, &tab, &dat).unwrap()
    }

    fn thing(slot: u32, class: u16, model: u16, x: u16, y: u16) -> Thing {
        Thing {
            slot,
            kind: mgc_formats::ThingKind::Entity,
            class,
            model,
            x,
            y,
            dis_id: 0xFFFF,
            swi_sz: 0,
            swi_id: 0,
            parent: 0,
            child: 0,
            par3: None,
        }
    }

    fn flat_land(h: u8) -> Planes {
        Planes {
            height: vec![h; GRID],
            tile_type: vec![5; GRID],
            shading: vec![32; GRID],
            angle: vec![5; GRID], // class 5 land
            ceiling: Vec::new(),
        }
    }

    fn run(p: &mut Planes, things: &[Thing], seed: u32, assets: &FeatureAssets) {
        generate_features_mc1(
            TerrainPlanes {
                height: &mut p.height,
                tile_type: &mut p.tile_type,
                shading: &mut p.shading,
                angle: &mut p.angle,
            },
            things,
            seed,
            assets,
        );
    }

    #[test]
    fn ring_iterator_drops_last_cell_of_end_ring() {
        let assets = synthetic_assets();
        let (r0, r1) = (assets.rings[0].len(), assets.rings[1].len());
        let g = Gen::new(
            Planes {
                height: vec![0; GRID],
                tile_type: vec![0; GRID],
                shading: vec![0; GRID],
                angle: vec![0; GRID],
                ceiling: Vec::new(),
            },
            assets,
            0,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        assert_eq!(g.ring_cells(0, 0).len(), r0 - 1);
        assert_eq!(g.ring_cells(0, 1).len(), r0 + r1 - 1);
    }

    /// The possession-claim chime (:30806-07) plays for ANY claimant —
    /// `sub_55370(claimant, -1, 4)`'s a2 = -1 arm is positional, not
    /// player-gated (the mis-read that silenced rival claims,
    /// 2026-07-22). No golden exercises a rival house-claim, so this
    /// pins the emit directly: rival claim → world-sourced id 4
    /// anchored at the claimant; player claim → the local chime.
    #[test]
    fn house_claim_chimes_for_any_wizard() {
        let assets = synthetic_assets();
        let mut g = Gen::new(flat_land(8), assets, 1, ChassisParams::MC1, VerbSet::MC1);
        let b = g.new_event().unwrap();
        g.ent[b].class64 = 3;
        g.ent[b].model65 = 45;
        g.ent[b].act_life = 100;
        g.ent[b].f63 = 1; // off the 40-tick occupancy beat
        let r = g.new_event().unwrap();
        g.ent[r].class64 = 3;
        g.ent[r].model65 = 0;
        g.ent[r].x = 5 * 256;
        g.ent[r].y = 6 * 256;
        g.ent[b].mail[1] = (0, r as u16);
        g.tick_building_live(b, crate::patches::WorldPatches::RETAIL);
        let ev = g
            .sounds
            .iter()
            .find(|s| s.id == 4)
            .expect("rival claim must chime");
        assert!(!ev.player, "positional, not the local-player channel");
        assert_eq!(ev.tag, r as u16, "anchored at the CLAIMANT");
        assert_eq!(ev.pos.0, 5 * 256);
        // The player's own claim still rings the local chime.
        g.sounds.clear();
        g.ent[b].mail[1] = (0, crate::mc1::mobs::PLAYER_TARGET);
        g.tick_building_live(b, crate::patches::WorldPatches::RETAIL);
        let ev = g
            .sounds
            .iter()
            .find(|s| s.id == 4)
            .expect("player claim must chime");
        assert!(ev.player);
    }

    #[test]
    fn crater_digs_a_bowl() {
        let assets = synthetic_assets();
        let mut p = flat_land(100);
        let things = vec![thing(0, 10, 11, 128, 128)];
        run(&mut p, &things, 1234, &assets);
        let center = p.height[128 * 256 + 128];
        assert!(center < 100, "crater lowers the center, got {center}");
        // Far away untouched.
        assert_eq!(p.height[10 * 256 + 10], 100);
    }

    #[test]
    fn canyon_chain_carves_a_channel() {
        let assets = synthetic_assets();
        let mut p = flat_land(100);
        // Two chained canyon nodes: slots 0 and 1 (engine 1 and 2).
        let mut a = thing(0, 10, 31, 100, 100);
        a.swi_id = 1;
        a.child = 2;
        let mut b = thing(1, 10, 31, 120, 100);
        b.swi_id = 1;
        b.parent = 1;
        run(&mut p, &[a, b], 99, &assets);
        // Sampled along the line: meaningfully dug.
        let dug = (100..120)
            .filter(|&x| p.height[100 * 256 + x as usize] < 95)
            .count();
        assert!(dug > 10, "canyon digs along the segment, {dug} tiles dug");
        assert_eq!(p.height[10 * 256 + 200], 100, "far tiles untouched");
    }

    #[test]
    fn building_flattens_and_paints() {
        let assets = synthetic_assets();
        let mut p = flat_land(100);
        // Slope under the building so flattening is observable.
        for y in 0..256 {
            for x in 0..256 {
                p.height[y * 256 + x] = (60 + (x / 8) as i32).min(200) as u8;
            }
        }
        let mut b = thing(0, 10, 45, 128, 128);
        b.parent = 0; // build type 16
        run(&mut p, &[b], 7, &assets);
        // The 4x4 footprint centered near (128,128) got wall paint
        // (types 8/9 or table pairs) and the protection bit.
        let protected = (125..132)
            .flat_map(|y| (125..132).map(move |x| (x, y)))
            .filter(|&(x, y)| p.angle[y * 256 + x] & 0x80 != 0)
            .count();
        assert!(
            protected >= 8,
            "building marks protected tiles, got {protected}"
        );
    }

    /// The transform must RETRY a failed painter/leveler spawn —
    /// retail keeps each commit inside the spawn-success arm (sub_47960
    /// :56471, sub_47020 :56107, sub_47080 :56126). Advancing to a
    /// pure-wait state with no helper spawned freezes the castle
    /// forever (neither upgradable nor destroyable) under meteor pool
    /// exhaustion.
    /// The castle kill sweeps the WHOLE footprint rectangle, not just
    /// the cells that carry masonry. Retail fires `sub_40E20` before
    /// it even reads the cell byte (:30634 precedes :30635), so an
    /// EMPTY cell of the build row executes what stands on it exactly
    /// like a wall cell does. Gating on the byte (as the port used to)
    /// cut the lethal area to under 40% of the rectangle at level 7.
    #[test]
    fn castle_kill_sweeps_empty_footprint_cells_too() {
        // A 3x3 build row whose CENTRE cell is empty (0) — a hole in
        // the masonry that must still kill.
        let grid = vec![0u8; 1024];
        let dat: Vec<u8> = vec![
            3, 0x10, 0x10, 0x10, 0, // row 0 of the footprint
            3, 0x10, 0x00, 0x10, 0, // row 1: the EMPTY centre
            3, 0x10, 0x10, 0x10, 0, // row 2
        ];
        let mut tab = Vec::new();
        tab.extend_from_slice(&0u32.to_le_bytes());
        tab.extend_from_slice(&[0, 0]); // row 0: EMPTY, like the real tables
        for _ in 1..8 {
            tab.extend_from_slice(&0u32.to_le_bytes());
            tab.extend_from_slice(&[3, 3]);
        }
        let assets = FeatureAssets::parse(&grid, &tab, &dat).unwrap();
        let mut g = Gen::new(flat_land(8), assets, 1, ChassisParams::MC1, VerbSet::MC1);
        let (cx, cy) = (100u8, 100u8);
        let at = |g: &mut Gen, model: u16, tx: u8, ty: u8, own: u16| {
            let s = g
                .spawn_creature(model, (tx as u16) << 8, (ty as u16) << 8, 0)
                .unwrap();
            g.ent[s].id24 = own;
            s
        };
        // The footprint is centred on (cx, cy): top-left is
        // (cx - 1, cy - 1), so the EMPTY centre cell is (cx, cy).
        let on_hole = at(&mut g, 0, cx, cy, 7);
        let on_wall = at(&mut g, 0, cx + 1, cy, 7);
        let exempt = at(&mut g, 16, cx.wrapping_sub(1), cy, 7);
        let owned = at(&mut g, 0, cx, cy.wrapping_sub(1), 9);
        g.build_footprint_kill(1, cx, cy, 9);
        assert!(
            g.ent[on_hole].act_life < 0,
            "the EMPTY centre cell kills too — the whole rectangle is lethal"
        );
        assert!(g.ent[on_wall].act_life < 0, "a masonry cell kills");
        assert!(g.ent[exempt].act_life >= 0, "m16 is exempt");
        assert!(
            g.ent[owned].act_life >= 0,
            "the owner's own creature is spared"
        );
        assert_eq!(g.ent[on_hole].f38, 9, "the kill credits the castle owner");
    }

    #[test]
    fn castle_painter_keeps_courtyard_water() {
        // A 3x3 castle row: an 0x59 wall ring (goal target+32, paint)
        // around one 0x07 courtyard cell (goal = the datum itself).
        // On a water site the courtyard's delta is 0 and sub_285C0's
        // apply loop never touches a zero-delta cell (:30550) — the
        // yard keeps its water nibble, type and height while the
        // risen ring converts. The level-init stamp (sub_279D0
        // :29868) converts unconditionally: authored starting castles
        // DO drain the yard.
        // 5x5 so the yard CENTRE cell's quad touches no risen wall
        // vertex — a 1-wide yard would re-derive as a slope blend
        // (its corners ARE the wall columns), in retail too.
        let grid = vec![0u8; 1024];
        let dat: Vec<u8> = vec![
            5, 0x59, 0x59, 0x59, 0x59, 0x59, 0, // wall ring
            5, 0x59, 0x07, 0x07, 0x07, 0x59, 0, 5, 0x59, 0x07, 0x07, 0x07, 0x59,
            0, // the courtyard
            5, 0x59, 0x07, 0x07, 0x07, 0x59, 0, 5, 0x59, 0x59, 0x59, 0x59, 0x59, 0,
        ];
        let mut tab = Vec::new();
        tab.extend_from_slice(&0u32.to_le_bytes());
        tab.extend_from_slice(&[0, 0]); // row 0: EMPTY, like the real tables
        for _ in 1..8 {
            tab.extend_from_slice(&0u32.to_le_bytes());
            tab.extend_from_slice(&[5, 5]);
        }
        let water = || Planes {
            height: vec![0; GRID],
            tile_type: vec![0; GRID],
            shading: vec![32; GRID],
            angle: vec![0; GRID],
            ceiling: Vec::new(),
        };
        let assets = FeatureAssets::parse(&grid, &tab, &dat).unwrap();
        let mut g = Gen::new(water(), assets.clone(), 1, ChassisParams::MC1, VerbSet::MC1);
        let p = g.spawn_creator(42, 100 << 8, 100 << 8, 0).unwrap();
        g.ent[p].f71 = 1;
        g.ent[p].flags |= 0x10000; // the upgrade-commit painter (:56492)
        for _ in 0..60 {
            if g.ent[p].flags & 0x400 != 0 {
                break;
            }
            g.tick_castle_painter(p);
        }
        assert!(g.ent[p].flags & 0x400 != 0, "the painter ran to its finish");
        let court = tile(100, 100);
        assert_eq!(g.t.angle[court] & 0xF, 0, "the yard keeps the water nibble");
        assert_eq!(g.t.tile_type[court], 0, "the yard keeps the water type");
        assert_eq!(g.t.height[court], 0, "the yard stays level with the water");
        let wall = tile(98, 100);
        assert_eq!(g.t.angle[wall] & 7, 1, "a risen wall cell converts to land");
        assert_eq!(g.t.height[wall], 32, "the wall reached its +32 goal");
        // The level-init stamp over the same water drains the yard.
        let mut g2 = Gen::new(water(), assets, 1, ChassisParams::MC1, VerbSet::MC1);
        g2.stamp_castle_terrain(1, 100, 100, 0);
        assert_eq!(
            g2.t.angle[court] & 7,
            1,
            "the authored stamp converts the yard (sub_279D0's law)"
        );
    }

    #[test]
    fn castle_transform_retries_failed_spawns() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let i = g.new_event().unwrap();
        {
            let e = &mut g.ent[i];
            e.class64 = 3;
            e.model65 = 2;
            e.x = 0x8000;
            e.y = 0x8000;
            e.f26 = 0; // fresh: awaiting the first level-up
            e.f59 = 0;
        }
        // Drain the pool, keeping three slots to hand back one at a
        // time (one per transform stage under test).
        let spares = [
            g.new_event().unwrap(),
            g.new_event().unwrap(),
            g.new_event().unwrap(),
        ];
        while g.new_event().is_some() {}

        // Case 0: exhausted pool → no commit, no wait state.
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(
            g.ent[i].f59, 0,
            "level-up retries instead of parking in wait"
        );
        assert_eq!(g.ent[i].f26, 0, "no level commit without a painter");
        g.free.push(spares[0] as u16);
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f59, 1, "freed slot: the painter spawned");
        assert_eq!(g.ent[i].f26, 1, "the level-up committed with it");
        assert!(
            g.ent
                .iter()
                .any(|e| e.class64 == 10 && e.model65 == 42 && e.flags & 0x400 == 0),
            "the m42 painter exists"
        );

        // Case 5 (leveler) and case 3 (repaint) hold their state too.
        g.ent[i].f59 = 5;
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f59, 5, "leveler spawn failure holds state 5");
        g.free.push(spares[1] as u16);
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f59, 6, "freed slot: the leveler handoff");
        g.ent[i].f59 = 3;
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f59, 3, "repaint spawn failure holds state 3");
        g.free.push(spares[2] as u16);
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f59, 1, "freed slot: the repaint painter wait");
    }

    /// **THE BLAST SHAKE COUNTS DOWN TO ONE BEFORE THE REPAINT.**
    /// Retail (:55983-99) checks FIRST: the f50==1 tick transitions
    /// to the repaint with NO decrement (the boundary shows 1 for a
    /// full tick), a >1 tick only counts down. A shake armed at 5
    /// therefore snapshots 4, 3, 2, 1 and fires the repaint on the
    /// FIFTH tick — decrement-first fired it on the fourth, spawning
    /// the (10,42) painter one boundary early (the mc1l0 free-run
    /// entity-set fork at t=1295 after self-destruct #1's downgrade).
    #[test]
    fn the_blast_shake_counts_to_one_before_the_repaint() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let i = g.new_event().unwrap();
        {
            let e = &mut g.ent[i];
            e.class64 = 3;
            e.model65 = 2;
            e.f26 = 1;
            e.f59 = 4;
            e.f50 = 5;
            e.x = 0x8000;
            e.y = 0x8000;
        }
        for want in [4, 3, 2, 1] {
            g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
            assert_eq!(g.ent[i].f50, want, "a countdown tick only decrements");
            assert_eq!(g.ent[i].f59, 4, "no transition above 1");
        }
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f50, 0, "the ==1 tick zeroes without decrementing");
        assert_eq!(
            g.ent[i].f59, 3,
            "the repaint fires only after the f50=1 boundary was seen"
        );
    }

    /// The balloon claim ticket is a RAW slot index — a collected
    /// ball's slot recycled by another class-10 entity (a dwelling)
    /// must not be devoured as if it were still the claimed (10,39)
    /// ball. Retail sub_47F90 (:56742-73) shares the latent bug; the
    /// dispatcher only ever assigns (10,39), so the guard blocks
    /// nothing legitimate.
    #[test]
    fn the_balloon_mover_is_blind() {
        // sub_47F90 dereferences the claim ticket by the target's
        // CLASS BYTE alone — no liveness check, no model check
        // (mc1l0 t=2472-2516: the y-bounce across freed ball 88).
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let b = g.new_event().unwrap();
        {
            let e = &mut g.ent[b];
            e.class64 = 3;
            e.model65 = 3;
            e.x = 0x4000;
            e.y = 0x4000;
            e.z = 300;
            e.f126 = 8;
        }
        let own = g.ent[b].id24;
        // A claimed slot recycled as a DWELLING (10,45) overlapping
        // the balloon: the blind ball arm absorbs the record —
        // retail's latent LIFO-reuse bug (:56742-73) is the law.
        let t = g.new_event().unwrap();
        {
            let e = &mut g.ent[t];
            e.class64 = 10;
            e.model65 = 45;
            e.x = 0x4000;
            e.y = 0x4000;
            e.z = 300;
            e.f80 = 64;
            e.f82 = 64;
            e.f84 = 64;
            e.f144 = own;
            e.f140 = 500;
        }
        g.ent[b].f146 = t as u16;
        g.balloon_move(b);
        assert_ne!(
            g.ent[t].flags & 0x400,
            0,
            "the recycled record is absorbed like a ball"
        );
        assert_eq!(g.ent[b].f146, 0, "the absorb clears the claim");
        assert_eq!(g.ent[b].f140, 500, "the record's cargo transfers");

        // A claim at a FREED slot (class 0, stale bytes — the
        // importer's carry): the mover neither idles nor clears it;
        // it bounces across the corpse position, angle(0,±step)
        // flipping the heading 1024/0 each tick.
        let dead = g.new_event().unwrap();
        {
            let e = &mut g.ent[dead];
            e.class64 = 0;
            e.model65 = 39;
            e.flags = 0x400 | 12;
            e.x = 0x4000;
            e.y = 0x4000;
        }
        {
            let e = &mut g.ent[b];
            e.x = 0x4000;
            e.y = 0x4000 - 8; // one speed-step shy of the corpse
            e.f146 = dead as u16;
            e.f30 = 7;
        }
        g.balloon_move(b);
        assert_eq!(g.ent[b].f146, dead as u16, "the stale claim stands");
        assert_eq!(g.ent[b].f30, 1024, "heading points down the +y delta");
        assert_eq!(g.ent[b].y, 0x4000, "the step lands ON the corpse");
        g.balloon_move(b);
        assert_eq!(g.ent[b].f30, 0, "the zero-delta angle is 0");
        assert_eq!(g.ent[b].y, 0x4000 - 8, "the next step bounces back off");
    }

    #[test]
    fn the_dispatcher_staggers_retargeting_and_a_fresh_balloon_parks_untargeted() {
        // sub_47400: a spawned fleet index gets NO targeting that
        // pass (:56340-49 — mc1l0 t=2379, the newborn parks with
        // chase 0); a live one retargets only when castle+63 %
        // quota == 0 (:56338), and the ball pick is 3-D nearest
        // (sub_42390 includes z).
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let c = g.new_event().unwrap();
        {
            let e = &mut g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.id24 = 630;
            e.f26 = 4; // level 4: quota (2, 6)
            e.f136 = 1_000_000; // census far from full
            e.x = 0x4000;
            e.y = 0x4000;
        }
        // A claimed ball waiting nearby.
        let ball = g.new_event().unwrap();
        {
            let e = &mut g.ent[ball];
            e.class64 = 10;
            e.model65 = 39;
            e.x = 0x4000 + 400;
            e.y = 0x4000;
            e.f144 = 630;
            e.f140 = 100;
        }
        // Spawn pass (stagger hit, f63 = 2): both fresh balloons
        // park untargeted despite the waiting ball.
        // The pick walks the TICK-TOP ball chain, which `World::tick`
        // rebuilds inline; a bare `Gen` must stand that up itself.
        g.ent[c].f63 = 2;
        g.rebuild_ball_chain();
        g.castle_balloons(c);
        let fleet: Vec<usize> = (1..g.ent.len())
            .filter(|&j| g.ent[j].class64 == 3 && g.ent[j].model65 == 3)
            .collect();
        assert_eq!(fleet.len(), 2, "the level-4 quota spawns two");
        for &b in &fleet {
            assert_eq!(g.ent[b].f146, 0, "a fresh balloon has no target");
        }
        // Off-stagger pass (f63 = 3, 3 % 2 != 0): stale targets
        // stand — even a dangling one.
        g.ent[fleet[0]].f146 = 999;
        g.ent[c].f63 = 3;
        g.rebuild_ball_chain();
        g.castle_balloons(c);
        assert_eq!(g.ent[fleet[0]].f146, 999, "off-turn keeps the stale claim");
        // Stagger pass (f63 = 4): the re-pick runs — a second ball
        // nearer in 2-D but farther in 3-D loses (sub_42390).
        let far3d = g.new_event().unwrap();
        {
            let e = &mut g.ent[far3d];
            e.class64 = 10;
            e.model65 = 39;
            e.x = 0x4000 + 300; // 2-D nearer than `ball`…
            e.y = 0x4000;
            e.z = 2000; // …but 3-D much farther
            e.f144 = 630;
            e.f140 = 100;
        }
        g.ent[c].f63 = 4;
        g.rebuild_ball_chain();
        g.castle_balloons(c);
        assert_eq!(
            g.ent[fleet[0]].f146, ball as u16,
            "the first balloon takes the 3-D-nearest ball (not the 2-D pick)"
        );
        assert_eq!(
            g.ent[fleet[1]].f146, far3d as u16,
            "sibling exclusion hands the second balloon the other ball"
        );
    }

    #[test]
    fn the_death_notice_tick_still_runs_the_dispatcher() {
        // sub_46DB0 :56003 sets `+70 = 6` on a lethal sub_47EC0 and
        // FALLS THROUGH — the f63-even block (ejector, extents,
        // fleet dispatch, absorb) still runs while the castle sits
        // at its negative life (mc1l0 t=2310: the Shift+L
        // self-destruct at life −1 spawns balloon 484, which the
        // next tick's level-0 cull demolishes).
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let c = g.new_event().unwrap();
        {
            let e = &mut g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.tick70 = 4;
            e.id24 = 630;
            e.f26 = 1; // level 1: quota (1, 0)
            e.f136 = 1_000_000;
            e.f59 = 4;
            e.f63 = 2; // even → the dispatcher pass
            e.act_life = -1; // the Shift+L demolish stamp (:55846-50)
            e.x = 0x4000;
            e.y = 0x4000;
        }
        g.castle_tick(c, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[c].tick70, 6, "the lethal notice parks action 6");
        assert_eq!(g.ent[c].act_life, -1, "the negative life lingers the tick");
        let fleet = (1..g.ent.len())
            .filter(|&j| g.ent[j].class64 == 3 && g.ent[j].model65 == 3)
            .count();
        assert_eq!(fleet, 1, "the death-notice tick still spawns the fleet");
    }

    fn mc2_gen() -> Gen {
        Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC2,
        )
    }

    fn ctx_at(px: u16, py: u16, pz: i16) -> crate::mc1::mobs::MobCtx {
        crate::mc1::mobs::MobCtx {
            px,
            py,
            pz,
            pyaw: 0,
            pmana: 1000,
            pdead: false,
            strict: false,
            patches: crate::patches::WorldPatches::RETAIL,
            mc2_turn: 0,
        }
    }

    /// The creature awake gate is a chassis parameter (the
    /// `--awake-range` G-class override): the faithful 0x240_0000
    /// (24 tiles, both retail engines) leaves a distant creature
    /// asleep; `i32::MAX` = the always-awake override arms it.
    #[test]
    fn awake_gate_is_a_chassis_parameter() {
        let run = |gate: i32| {
            let mut ch = ChassisParams::MC1;
            ch.awake_gate_sq = gate;
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ch,
                crate::verbs::VerbSet::MC1,
            );
            // A bare class-5 creature 40 tiles from the player —
            // outside the retail gate, inside an infinite one.
            g.ent[5].class64 = 5;
            g.ent[5].act_life = 10;
            g.ent[5].x = 40 * 256;
            g.ent[5].y = 0;
            g.mob_awake_pass(&ctx_at(0, 0, 0));
            g.ent[5].f58
        };
        assert_eq!(run(0x240_0000), 0, "40 tiles out stays asleep (faithful)");
        assert_eq!(run(i32::MAX), 16, "always-awake override arms f58");
    }

    /// The mana-ball WAKE law (sub_54F80 :64352-66): a settled ball
    /// within 24.0 tiles of the HUMAN re-arms +58 = 16 on the same
    /// maintenance pass that decrements it, giving the corpus-measured
    /// exact 17-tick per-slot cycle (16 counted down + 1 observed-zero
    /// re-arm tick); the radius compare is strict (dist² < 6144²), and
    /// an out-of-radius ball stays frozen forever.
    #[test]
    fn settled_ball_wakes_within_24_tiles_on_a_17_tick_cycle() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let b = g.spawn_mana_ball(0, 0, 0).unwrap();
        g.ent[b].f58 = 0; // settled (128 ticks elapsed)

        // Boundary exactness: exactly 6144 units = NOT eligible.
        g.mob_awake_pass(&ctx_at(6144, 0, 0));
        assert_eq!(g.ent[b].f58, 0, "24.0 tiles exactly stays frozen");
        // One unit inside re-arms to 16 (altitude never gates).
        g.mob_awake_pass(&ctx_at(6143, 0, 32767));
        assert_eq!(g.ent[b].f58, 16, "inside the radius re-arms 16");

        // Cadence: with the player parked nearby, the value returns
        // to 16 every 17 passes — 16 decrements, one zero-observe.
        let mut rearms = Vec::new();
        for t in 1..=34 {
            g.mob_awake_pass(&ctx_at(100, 0, 0));
            if g.ent[b].f58 == 16 {
                rearms.push(t);
            }
        }
        assert_eq!(rearms, vec![17, 34], "exact 17-tick wake period");

        // Far away again: the countdown drains and never re-arms.
        for _ in 0..40 {
            g.mob_awake_pass(&ctx_at(0x7000, 0x7000, 0));
        }
        assert_eq!(g.ent[b].f58, 0, "out of radius, frozen for good");
    }

    // ---- the chase-trailer / speed-restore family -----------------------
    //
    // MC1 bounds creature speed with per-model ENTRY and EXIT trailers
    // hung off the individual state handlers, NOT with a clamp: +128
    // (max speed) and +130 (accel) are write-once in the ctors, and the
    // mover passes +126 verbatim (sub_196E0 :21182 -> sub_41EC0
    // :52523). The pack catch-up (sub_1A390 :21814) is the only thing
    // that ever raises +126 above a creature's own +128, and what pulls
    // it back down is the exit trailer of whatever state the creature
    // leaves next. Miss a trailer and that creature keeps the inflated
    // speed for the rest of the level — the player-reported "monsters
    // that keep speeding up". These tests pin every trailer in the
    // family, including the DEATH tick, which retail reaches because
    // its damage prologue lives inside each handler and falls through.

    fn mob_gen() -> Gen {
        Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        )
    }

    /// Raise an m7 of the ODD spawn ordinal — the parity arm that gets
    /// sprite 85 and so is the variant `sub_1C960` toggles (:45101-13).
    fn spawn_m7_odd(g: &mut Gen, x: u16, y: u16) -> usize {
        let i = g.spawn_creature(7, x, y, 0).unwrap();
        if g.ent[i].type86 != 85 {
            let j = g.spawn_creature(7, x, y, 0).unwrap();
            g.ent[i].flags |= 0x400;
            return j;
        }
        i
    }

    /// m7's CHASE trailer `sub_1C960` (:23319, twin remc1hw :21876) —
    /// the family's only speed bound, and the one the port was missing
    /// outright (`(_, 2) => mob_chase` routed m7 through the shared
    /// chase). Firing PLANTS the thrower: sprite 85 -> 198, +126 down
    /// to the accel, a 30-tick timer armed (:23339-45). The timer
    /// expiring un-plants and restores +128 (:23327-32) — and so does
    /// leaving CHASE while planted (:23346-55).
    #[test]
    fn m7_plants_on_the_hit_and_restores_on_the_timer() {
        let mut g = mob_gen();
        let i = spawn_m7_odd(&mut g, 0x4000, 0x4000);
        let (max, accel) = (g.ent[i].f128, g.ent[i].f130);
        assert!(accel != 0 && accel < max, "m7 carries a live accel step");

        // In CHASE, on the cadence tick, with the wizard in reach.
        let ctx = ctx_at(0x4080, 0x4000, 0);
        g.ent[i].tick70 = 44;
        g.ent[i].f146 = crate::mc1::mobs::PLAYER_TARGET;
        g.ent[i].f63 = 0;
        g.ent[i].f26 = 0;
        g.creature_tick(i, &ctx);
        assert_eq!(
            (g.ent[i].type86, g.ent[i].f126, g.ent[i].f26),
            (198, accel, 30),
            "the connecting bolt plants the thrower at the accel speed"
        );

        // 30 ticks of cooldown; the last one un-plants it.
        for n in 1..30 {
            g.ent[i].f63 = 1; // off-cadence: no second bolt
            g.creature_tick(i, &ctx);
            assert_eq!(g.ent[i].type86, 198, "still planted at tick {n}");
            assert_eq!(g.ent[i].f126, accel, "still crawling at tick {n}");
        }
        g.ent[i].f63 = 1;
        g.creature_tick(i, &ctx);
        assert_eq!(
            (g.ent[i].type86, g.ent[i].f126),
            (85, max),
            "the timer expiring un-plants and restores +128"
        );
    }

    /// The other half of `sub_1C960` (:23346-55): a planted thrower
    /// that LOSES the chase restores on that very tick, whatever the
    /// dug-in timer still says. This is the arm that re-baselines a
    /// +126 the pack catch-up inflated.
    #[test]
    fn m7_chase_exit_restores_a_pack_inflated_speed() {
        let mut g = mob_gen();
        let i = spawn_m7_odd(&mut g, 0x4000, 0x4000);
        let (max, accel) = (g.ent[i].f128, g.ent[i].f130);

        // Plant it, then hand it the speed a pack catch-up would have
        // written (sub_1A390 :21814 = leader +126 + leader +130), well
        // above its own maximum.
        g.ent[i].tick70 = 44;
        g.ent[i].f146 = crate::mc1::mobs::PLAYER_TARGET;
        g.ent[i].f63 = 0;
        g.ent[i].f26 = 0;
        g.creature_tick(i, &ctx_at(0x4080, 0x4000, 0));
        assert_eq!(g.ent[i].type86, 198, "planted");
        let inflated = max + 4 * accel;
        g.ent[i].f126 = inflated;
        g.ent[i].f26 = 25; // timer still running — not the expiry arm

        // Now the wizard steps out of range: the shared chase drops to
        // WANDER on the cadence tick and the trailer fires.
        g.ent[i].f63 = 0;
        g.creature_tick(i, &ctx_at(0x7F00, 0x7F00, 0));
        assert_eq!(g.ent[i].tick70, 43, "dropped back to WANDER");
        assert_eq!(
            (g.ent[i].type86, g.ent[i].f126),
            (85, max),
            "leaving the chase re-baselines +126 to +128"
        );
        assert!(
            g.ent[i].f126 < inflated,
            "the inflated speed does not survive the chase"
        );
    }

    /// The pack catch-up itself (sub_1A390 :21814) is UNBOUNDED by
    /// design in both engines — it is the SET form (member +126 =
    /// LEADER +126 + LEADER +130), it consults no cap, and it must not
    /// grow a `.min(+128)`: retail carries m2 +126 = 95 against +128 =
    /// 70 for 62 creature-ticks in the mc1l5 take alone. This pins the
    /// arithmetic AND the fact that the exit trailer, not a clamp, is
    /// what ends the inflation.
    #[test]
    fn pack_catch_up_is_the_set_form_and_stays_uncapped() {
        let mut g = mob_gen();
        let leader = spawn_m7_odd(&mut g, 0x4000, 0x4000);
        let follower = spawn_m7_odd(&mut g, 0x4010, 0x4000);
        let (max, accel) = (g.ent[leader].f128, g.ent[leader].f130);

        // A leader already running hot, and a follower far below it.
        g.ent[leader].tick70 = 43; // WANDER: the follow case
        g.ent[leader].f126 = max + 7 * accel;
        g.ent[follower].tick70 = 45; // PACK
        g.ent[follower].f52 = leader as u16;
        g.ent[follower].f126 = 1;
        g.ent[follower].f63 = 0; // on the v_26 cadence
        g.creature_tick(follower, &ctx_at(0x7F00, 0x7F00, 0));
        assert_eq!(
            g.ent[follower].f126,
            g.ent[leader].f126 + accel,
            "the member takes the LEADER's speed plus the LEADER's accel"
        );
        assert!(
            g.ent[follower].f126 > max,
            "and it is NOT clamped to the member's own +128"
        );
    }

    /// m4's militia trailers: `sub_1BC50` (:22744) arms him on the
    /// PROMOTION tick — one LCG draw, speed 0, the target's own
    /// class/model as his bolt filter — and `sub_1BCE0` (:22766) puts
    /// the dart away on the chase-exit tick, restoring the WALK SPEED.
    /// The port had the zero but not the restore, so a militiaman who
    /// had chased once stayed pinned at speed 0 for the rest of the
    /// level; the mc1l5 take scores both halves.
    #[test]
    fn militia_arms_on_promotion_and_restores_its_walk_speed_on_exit() {
        let mut g = mob_gen();
        let i = g.spawn_creature(4, 0x4000, 0x4000, 0).unwrap();
        let max = g.ent[i].f128;
        assert_eq!(g.ent[i].f126, max, "spawns at his walk speed");

        // Standing in the village (state 25) with the wizard in reach,
        // on the 4*v_26 acquisition tick and village-wanted.
        let ctx = ctx_at(0x4200, 0x4000, 0);
        g.ent[i].tick70 = 25;
        g.ent[i].f58 = 16;
        g.ent[i].f63 = 0;
        g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, ctx.px, ctx.py);
        g.player_aggro = 200; // the +528 hostility gate
        g.creature_tick(i, &ctx);
        assert_eq!(g.ent[i].tick70, 26, "promoted to CHASE");
        assert_eq!(
            g.ent[i].f126, 0,
            "and armed on the SAME tick — sub_1BC50 stops him dead"
        );
        assert_ne!(g.ent[i].type86, 0, "wearing an armed sprite");

        // The wizard leaves: the chase breaks and the trailer disarms.
        g.ent[i].f63 = 0;
        g.creature_tick(i, &ctx_at(0x7F00, 0x7F00, 0));
        assert_eq!(g.ent[i].tick70, 25, "back to the village walk");
        assert_eq!(
            (g.ent[i].f126, g.ent[i].type86, g.ent[i].f66, g.ent[i].f67),
            (max, 0, 3, 0xFF),
            "sub_1BCE0 restores speed, sprite and filter together"
        );
    }

    /// m9's `sub_1DCD0` (:24236) / `sub_1DD50` (:24255) pair: the mound
    /// fights ROOTED (+126 = 0 on the promotion tick — retail's
    /// burrower never walks in the warrior form) and goes back to the
    /// type-201 disguise at +128 when the chase ends.
    #[test]
    fn mound_enters_the_chase_rooted_and_restores_on_exit() {
        let mut g = mob_gen();
        let i = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
        let max = g.ent[i].f128;
        let ctx = ctx_at(0x4200, 0x4000, 0);
        g.ent[i].tick70 = 55;
        g.ent[i].f26 = 200;
        g.ent[i].f63 = 0;
        g.ent[i].f58 = 16;
        g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, ctx.px, ctx.py);
        g.creature_tick(i, &ctx);
        assert_eq!(g.ent[i].tick70, 56, "surfaced into CHASE");
        assert_eq!(
            (g.ent[i].f126, g.ent[i].type86),
            (0, 202),
            "rooted in the warrior form on the promotion tick"
        );

        g.ent[i].f63 = 0;
        g.creature_tick(i, &ctx_at(0x7F00, 0x7F00, 0));
        assert_eq!(g.ent[i].tick70, 55, "back to lurking");
        assert_eq!(
            (g.ent[i].f126, g.ent[i].type86, g.ent[i].f67),
            (max, 201, 0xFF),
            "sub_1DD50 restores speed, mound sprite and filter"
        );
    }

    /// DEATH is a chase exit. Retail's damage prologue sits INSIDE each
    /// state handler and `goto`s that handler's trailer rather than
    /// returning (m9 sub_1DA60 :24184 `goto LABEL_31`; m2/m4/m15 reach
    /// it through sub_1A120's plain `return v15`), so a creature killed
    /// mid-chase still restores on the tick it dies. The mc1l5 take
    /// shows it directly — slot 348 goes act_life -1 at t=6241 and is
    /// still restored to +126 = 20, type 201 at t=6242 — and it is what
    /// stops a bee dying mid-lunge from leaving 3x +128 on the corpse.
    #[test]
    fn chase_exit_trailers_run_on_the_death_tick() {
        // (model, chase state, the speed the creature dies carrying)
        for &(model, chase, hot) in &[(2u16, 14u8, 0i16), (4, 26, 0), (9, 56, 0), (15, 92, 0)] {
            let mut g = mob_gen();
            let i = g.spawn_creature(model, 0x4000, 0x4000, 0).unwrap();
            let max = g.ent[i].f128;
            g.ent[i].tick70 = chase;
            g.ent[i].f146 = crate::mc1::mobs::PLAYER_TARGET;
            g.ent[i].f126 = hot;
            // A lethal mail item, delivered the way combat does.
            g.ent[i].f58 = 16;
            g.ent[i].mail[0] = (g.ent[i].max_life + 1000, 1);
            g.creature_tick(i, &ctx_at(0x4080, 0x4000, 0));
            assert_eq!(
                g.ent[i].tick70,
                chase + 2,
                "model {model} entered its DEATH slot"
            );
            assert_eq!(
                g.ent[i].f126, max,
                "model {model} restores +126 on the death tick"
            );
        }

        // The bee specifically: dying mid-lunge at 3x max must not
        // leave the lunge speed standing (sub_1B3C0 :22363-66).
        let mut g = mob_gen();
        let i = g.spawn_creature(2, 0x4000, 0x4000, 0).unwrap();
        let max = g.ent[i].f128;
        g.ent[i].tick70 = 14;
        g.ent[i].f146 = crate::mc1::mobs::PLAYER_TARGET;
        g.ent[i].f126 = 3 * max;
        g.ent[i].f58 = 16;
        g.ent[i].mail[0] = (g.ent[i].max_life + 1000, 1);
        g.creature_tick(i, &ctx_at(0x4080, 0x4000, 0));
        assert_eq!(g.ent[i].f126, max, "the lunge does not outlive the bee");
    }

    /// The kraken pins +126 = 30 on every movement tick, but its three
    /// slots do it at different points: the chase (sub_1C4F0 :23146)
    /// writes it FIRST, the wander (sub_1C4A0 :23118) and the pack
    /// (sub_1C880 :23276) write it LAST. The tail write is what keeps
    /// m6 out of the pack catch-up's reach — an inflated +126 is
    /// stamped back before the tick ends, so it is never left standing
    /// for a follower's next read.
    #[test]
    fn kraken_pack_tick_ends_at_its_pinned_speed() {
        let mut g = mob_gen();
        let head = g.spawn_creature(6, 0x4000, 0x4000, 0).unwrap();
        let follower = g.spawn_creature(6, 0x4010, 0x4000, 0).unwrap();
        g.ent[head].tick70 = 37; // WANDER: the follow case
        g.ent[head].f126 = 900; // hot leader
        g.ent[follower].tick70 = 39; // PACK
        g.ent[follower].f52 = head as u16;
        g.ent[follower].f63 = 0;
        g.creature_tick(follower, &ctx_at(0x7F00, 0x7F00, 0));
        assert_eq!(
            g.ent[follower].f126, 30,
            "the kraken's tail write outlives the catch-up"
        );
    }

    /// Every attack thunk in the engine stamps its projectile with the
    /// SHOOTER's own `+66`/`+67` filter pair (sub_1A8E0 :21895-98,
    /// sub_1A990 :21952-55, sub_1AB70 :22005-06, sub_1AE30 :22122-25,
    /// sub_1AA40 :21951-52, m15 :25857-58) — m8's sub_1AEE0 :22155-60
    /// alone takes the TARGET's, and m11's sub_1E380 writes none. For
    /// most creatures that pair IS the shared (3, 0xFF) the port used
    /// to hardcode, but m4 and m9 NARROW it to their target's
    /// class/model on the chase-entry trailer, and the narrowed filter
    /// rides their shots: a mound besieging a castle fires (3, 2)
    /// bolts that pass through the player, a rival carpet and a mana
    /// balloon alike. `filter_admits` tests the human as (3, 0), so
    /// the hardcoded pair let a castle-aimed bolt hit the wizard
    /// flying past it.
    #[test]
    fn a_mounds_castle_bolt_carries_the_castles_filter_not_the_wild_card() {
        let mut g = mob_gen();
        let mound = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
        let castle = g.spawn_castle(0x4100, 0x4000).unwrap();
        g.ent[castle].id24 = 7; // a rival's, so the mound will take it
        let away = ctx_at(0x7F00, 0x7F00, 0);

        // Lurking, on the castle-hunt cadence: it surfaces into CHASE
        // and the entry trailer narrows the filter on that tick.
        g.ent[mound].tick70 = 55;
        g.ent[mound].f26 = 200;
        g.ent[mound].f58 = 16;
        g.ent[mound].f63 = 0;
        g.creature_tick(mound, &away);
        assert_eq!(g.ent[mound].tick70, 56, "surfaced at the castle");
        assert_eq!(
            (g.ent[mound].f66, g.ent[mound].f67),
            (3, 2),
            "the mound takes the castle's own class/model"
        );
        let before: Vec<usize> = (1..g.ent.len())
            .filter(|&j| g.ent[j].class64 == 9 && g.ent[j].model65 == 13)
            .collect();
        g.ent[mound].f63 = 0; // the fire cadence
        g.creature_tick(mound, &away);
        let bolt = (1..g.ent.len())
            .find(|j| g.ent[*j].class64 == 9 && g.ent[*j].model65 == 13 && !before.contains(j))
            .expect("the mound loosed a bolt");
        assert_eq!(
            (g.ent[bolt].f66, g.ent[bolt].f67),
            (3, 2),
            "and the bolt inherits it — NOT the (3, 0xFF) wild card"
        );
        assert!(
            !Gen::filter_admits(g.ent[bolt].f66, g.ent[bolt].f67, 3, 0),
            "so it cannot collide with the human wizard (class 3, model 0)"
        );
        assert!(
            Gen::filter_admits(g.ent[bolt].f66, g.ent[bolt].f67, 3, 2),
            "but it still admits the castle it was aimed at"
        );
    }

    /// The mound re-bears on a DECIMAL period — `sub_1DA60` :24197 uses
    /// `+63 % 10`, not the shared chase's `(+63 & 3) == 0` (:21654).
    /// m9 drives its own chase in retail, so routing it through the
    /// shared one gave our mounds a 4-tick swing where retail's take
    /// 10; the mc1l5 take scores it heavily on the mound's `heading`
    /// and `target_yaw`.
    #[test]
    fn a_rooted_mound_re_bears_every_tenth_tick_not_every_fourth() {
        let hits = |model: u16, state: u8| {
            let mut g = mob_gen();
            let i = g.spawn_creature(model, 0x4000, 0x4000, 0).unwrap();
            g.ent[i].tick70 = state;
            g.ent[i].f146 = crate::mc1::mobs::PLAYER_TARGET;
            let mut n = 0;
            for phase in 0..40u8 {
                g.ent[i].f63 = phase;
                g.ent[i].f34 = 0;
                g.creature_tick(i, &ctx_at(0x4100, 0x4100, 0));
                if g.ent[i].f34 != 0 {
                    n += 1;
                }
            }
            n
        };
        assert_eq!(hits(9, 56), 4, "the mound re-bears 4 times in 40 ticks");
        assert_eq!(hits(10, 62), 10, "a shared-chase family re-bears 10");
    }

    /// The m9 mound's HIDDEN prologue is the one in the family with NO
    /// class gate on the attacker: `sub_1D060` :23732-38 and its buried
    /// twin `sub_1D6D0` :24004-07 both do a bare `+146 = +40; state
    /// 0x38`, where everything sharing `sub_19B10`/`sub_1A120` first
    /// tests the attacker's class for 3 — and m9's OWN chase prologue
    /// (:24177-79) keeps that test. So a lurking mound turns on any
    /// attacker, a militiaman included, and surfaces rooted; a mound
    /// already CHASING ignores a non-wizard hit exactly as before.
    /// mc1l5 t=4655 slot 819 is the witness: 250 damage from a
    /// class-5 model-4 and retail retaliates onto its slot.
    #[test]
    fn lurking_mound_retaliates_against_any_attacker_chasing_one_does_not() {
        // `held` = the target the mound already carries, so the CHASE
        // case can show a non-wizard hit failing to steal it.
        let run = |state: u8, held: u16| {
            let mut g = mob_gen();
            let i = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
            let m = g.spawn_creature(4, 0x4100, 0x4000, 0).unwrap(); // militia
            let max = g.ent[i].f128;
            g.ent[i].tick70 = state;
            g.ent[i].f26 = 200;
            g.ent[i].f58 = 16;
            g.ent[i].f146 = held;
            g.ent[i].f126 = max;
            g.ent[i].mail[0] = (250, m as u16);
            g.creature_tick(i, &ctx_at(0x4080, 0x4000, 0));
            (g.ent[i].tick70, g.ent[i].f146, g.ent[i].f126, m as u16, max)
        };

        let (state, tgt, speed, militia, _) = run(55, 0);
        assert_eq!(state, 56, "the lurking mound surfaces at its attacker");
        assert_eq!(tgt, militia, "and takes the MILITIAMAN as its target");
        assert_eq!(speed, 0, "rooted by the entry trailer on the same tick");

        // The CHASE slot keeps retail's class-3 test (:24177-79), so a
        // non-wizard hit there cannot steal the target it already has.
        let (state, tgt, _, militia, _) = run(56, crate::mc1::mobs::PLAYER_TARGET);
        assert_eq!(state, 56, "a chasing mound stays in its chase");
        assert_ne!(tgt, militia, "and does NOT retarget onto the militiaman");
        assert_eq!(
            tgt,
            crate::mc1::mobs::PLAYER_TARGET,
            "it keeps the wizard it was already after"
        );
    }

    /// The m9 mound's state-55 wizard scan (sub_1D060 :23796-23833):
    /// an awake surfaced mound with no castle chase targets the
    /// player and pops up into CHASE; an asleep one never scans (the
    /// +58 gate) — the level-04 trigger-spawned skeletons idled
    /// because the scan was missing entirely.
    #[test]
    fn m9_mound_scans_the_wizard_when_awake() {
        let run = |f58: i16| {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                crate::verbs::VerbSet::MC1,
            );
            let i = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
            let ctx = ctx_at(0x4200, 0x4000, 0); // 2 tiles east, in v_28
            g.ent[i].tick70 = 55; // surfaced mound (state 55)
            g.ent[i].f26 = 200; // burrow timer armed, no bury edge
            g.ent[i].f63 = 0; // on the v_26 scan tick
            g.ent[i].f58 = f58;
            g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, ctx.px, ctx.py);
            g.creature_tick(i, &ctx);
            (g.ent[i].tick70, g.ent[i].f146)
        };
        assert_eq!(
            run(16),
            (56, crate::mc1::mobs::PLAYER_TARGET),
            "awake mound chases the wizard"
        );
        assert_eq!(run(0).0, 55, "asleep mound never scans (+58 gate)");
    }

    /// The mound's convert tail (surfaced sub_1D060 :23834-917,
    /// buried sub_1D6D0 :24030-116): with nothing to chase, the
    /// cadence tick eats the nearest on-menu civilian (phase 0 → m4)
    /// within 3-D reach 0x600 and mints a fresh (5,9) at its feet —
    /// no corpse, no mana ball, no death state on the victim. Owner
    /// stamp quirk: a WILD mound's newborn stays self-owned on the
    /// surfaced arm (the :23912 wizard-body gate fails) but inherits
    /// the parent's slot index on the buried arm (:24112,
    /// unconditional).
    #[test]
    fn m9_mound_converts_civilians_into_skeletons() {
        let run = |buried: bool| {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                crate::verbs::VerbSet::MC1,
            );
            let i = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
            g.ent[i].tick70 = 55;
            g.ent[i].f26 = if buried { 0 } else { 200 };
            g.ent[i].f71 = if buried { 1 } else { 0 };
            g.ent[i].f58 = 0; // asleep: no wizard scan, no wake-arm
            g.ent[i].f63 = 0; // cadence hit, phase 0 → m4 militia
            let v = g.spawn_creature(4, 0x4100, 0x4000, 0).unwrap();
            let ctx = ctx_at(0x7F00, 0x7F00, 0); // player far away
            g.creature_tick(i, &ctx);
            assert_ne!(
                g.ent[v].flags & 0x400,
                0,
                "the civilian is destroy-flagged raw (no death state)"
            );
            let n = (1..g.ent.len())
                .find(|&j| {
                    j != i
                        && g.ent[j].class64 == 5
                        && g.ent[j].model65 == 9
                        && g.ent[j].flags & 0x400 == 0
                })
                .expect("a fresh (5,9) rose at the victim");
            assert_eq!(
                (g.ent[n].x, g.ent[n].y),
                (0x4100, 0x4000),
                "the riser stands where the victim stood"
            );
            assert_eq!(
                g.ent
                    .iter()
                    .filter(|e| e.class64 == 10 && e.model65 == 39 && e.flags & 0x400 == 0)
                    .count(),
                0,
                "no mana ball drops from a converted kill"
            );
            if buried {
                assert_eq!(
                    g.ent[n].id24 as usize, i,
                    "buried arm stamps the parent's id24 unconditionally"
                );
            } else {
                assert_eq!(
                    g.ent[n].id24 as usize, n,
                    "surfaced arm leaves a wild mound's newborn self-owned"
                );
            }
        };
        run(false);
        run(true);
    }

    /// The buried mound's unbury law (sub_1D6D0 :24016-28 +
    /// sub_1DDB0 :24273): asleep it stays buried forever; the wizard
    /// entering the 24-tile wake gate (an armed f58) starts the −50
    /// countdown and the mound rises ~1 s later — the level-04
    /// trigger army buried itself before the player arrived and our
    /// old stub never let it back up.
    #[test]
    fn m9_buried_mound_rises_near_the_wizard() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let i = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
        let ctx = ctx_at(0x4200, 0x4000, 0);
        g.ent[i].tick70 = 55;
        g.ent[i].f71 = 1; // buried
        g.ent[i].f26 = 0;
        g.ent[i].f58 = 0; // asleep
        for _ in 0..100 {
            g.creature_tick(i, &ctx);
        }
        assert_eq!(g.ent[i].f71, 1, "asleep mound stays buried");
        g.ent[i].f58 = 16; // the wizard flies into the wake gate
        g.creature_tick(i, &ctx);
        assert_eq!(g.ent[i].f26, -50, "awake trigger arms the countdown");
        for _ in 0..50 {
            g.creature_tick(i, &ctx);
        }
        assert_eq!(g.ent[i].f71, 0, "the mound rises");
        assert_eq!(g.ent[i].f26, 400, "fresh burrow timer");
        assert_eq!(g.ent[i].type86, 201, "back to the mound disguise");
    }

    /// The multipart families (m0 dragon / m3 worm / m6 kraken) spawn
    /// straight into WANDER and run the shared awake-gated wizard
    /// scan — an in-range, in-cone player is chased on the first scan
    /// tick (regression guard for the m9-style missing-scan class).
    #[test]
    fn multipart_wanderers_scan_the_wizard() {
        for (model, wander, chase) in [(0u16, 1u8, 2u8), (3, 19, 20), (6, 37, 38)] {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                crate::verbs::VerbSet::MC1,
            );
            let i = g.spawn_creature(model, 0x4000, 0x4000, 0).unwrap();
            let ctx = ctx_at(0x4200, 0x4000, 0); // 2 tiles east, in v_28
            assert_eq!(g.ent[i].tick70, wander, "m{model} spawns wandering");
            assert!(g.ent[i].f58 != 0, "m{model} spawns awake");
            g.ent[i].f63 = 0; // on the v_26 scan tick
            let facing = Gen::angle_between(0x4000, 0x4000, ctx.px, ctx.py);
            g.ent[i].f30 = facing;
            g.ent[i].f34 = facing; // no turn-away before the scan
            g.creature_tick(i, &ctx);
            assert_eq!(
                (g.ent[i].tick70, g.ent[i].f146),
                (chase, crate::mc1::mobs::PLAYER_TARGET),
                "m{model} chases the wizard"
            );
        }
    }

    /// A village collapse spawns evacuee militia (m4) at the building's
    /// pre-collapse corner height, which floats above the freshly-
    /// lowered rubble ground. Retail's idle handler `sub_1B5D0` runs the
    /// movement core `sub_196E0` (`creature_move`) on every alive tick
    /// (:22541) — the sole carrier of the altitude clamp — so the
    /// militiaman drifts down onto the ground (row-0 `v_14` = -4) and
    /// wanders there at idle speed. Our port had dropped that call, so
    /// the collapse militia froze mid-air and never wandered — the
    /// "floating archers that just sit there" on level 04.
    #[test]
    fn militia_spawned_above_ground_settles_and_wanders() {
        use crate::mc1::behavior::BEHAVIOR;
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let i = g.spawn_creature(4, 0x4000, 0x4000, 0).unwrap();
        assert_eq!(g.ent[i].tick70, 25, "m4 spawns into idle (state 25)");
        let ground = g.ground_z(g.ent[i].x, g.ent[i].y) as i16;
        // Float him a few hundred units up, as a collapse over dropped
        // rubble tiles would, and put the player far off so he stays
        // idle (no aggro) and simply wanders.
        g.ent[i].z = ground + 400;
        let (x0, y0) = (g.ent[i].x, g.ent[i].y);
        let ctx = ctx_at(0xC000, 0xC000, 0);
        for _ in 0..500 {
            g.creature_tick(i, &ctx);
        }
        assert_eq!(g.ent[i].tick70, 25, "stays idle with nothing to fight");
        let floor = ground.wrapping_add(BEHAVIOR[g.ent[i].row156 as usize].v_12);
        assert!(
            (g.ent[i].z - floor).abs() <= 4,
            "idle militia settles onto the ground floor (z {} vs floor {})",
            g.ent[i].z,
            floor
        );
        assert!(
            g.ent[i].x != x0 || g.ent[i].y != y0,
            "idle militia wanders instead of freezing where it spawned"
        );
    }

    /// The same movement core rides the CHASE state (`sub_1BB20` →
    /// `sub_1A120` → `sub_196E0` :21654) at speed 0, so a militiaman who
    /// spawned high and then acquired a target still settles onto the
    /// ground while he stands and shoots.
    #[test]
    fn militia_chasing_from_a_float_settles() {
        use crate::mc1::behavior::BEHAVIOR;
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let i = g.spawn_creature(4, 0x4000, 0x4000, 0).unwrap();
        let ground = g.ground_z(g.ent[i].x, g.ent[i].y) as i16;
        g.ent[i].z = ground + 400;
        g.ent[i].tick70 = 26; // chase
        g.ent[i].f146 = crate::mc1::mobs::PLAYER_TARGET;
        let ctx = ctx_at(0x4200, 0x4000, ground); // 2 tiles east, in range
        for _ in 0..500 {
            g.creature_tick(i, &ctx);
        }
        let floor = ground.wrapping_add(BEHAVIOR[g.ent[i].row156 as usize].v_12);
        assert!(
            (g.ent[i].z - floor).abs() <= 4,
            "chasing militia settles onto the ground (z {} vs floor {})",
            g.ent[i].z,
            floor
        );
    }

    /// D4: the militia idle pair-up (sub_1B5D0 :22661-90). Two idle
    /// militiamen with nothing to fight and no house to shelter in fall
    /// into an escort pair — one follows the other into the pack state
    /// (0x1B=27) with its leader set; the chosen leader (now the target
    /// of a packed sibling) stays idle. Before the fix `(4,3)` was a
    /// dead arm and the pair-up scan was stubbed, so every militiaman
    /// wandered as a loner.
    #[test]
    fn idle_militia_pairs_up_into_a_pack() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        // Two militia a few hundred units apart (well inside the row-4
        // range 4096), nothing else on the map, the player far away so
        // neither acquires a wizard target.
        let a = g.spawn_creature(4, 0x4000, 0x4000, 0).unwrap();
        let b = g.spawn_creature(4, 0x4300, 0x4000, 0).unwrap();
        assert_eq!(g.ent[a].tick70, 25, "m4 spawns idle");
        assert_eq!(g.ent[b].tick70, 25, "m4 spawns idle");
        let ctx = ctx_at(0xC000, 0xC000, 0);
        for _ in 0..400 {
            // The pair-up scan walks the TICK-TOP per-model chain, so a
            // test that drives `creature_tick` by hand has to rebuild it
            // exactly where `World::tick` does — otherwise the roster is
            // empty and nobody is ever a candidate. (This is the same law
            // that keeps militia evacuated together from pairing on their
            // shared birth tick — see `Gen::pack_scan`.)
            g.rebuild_mob_chains();
            // Keep them facing each other so the v_30 cone always admits
            // the sibling — otherwise the wander jitter swings the
            // heading in and out of cone and the scan timing turns
            // nondeterministic. (Setting the facing does not itself pair
            // them; the new pair-up scan does.)
            g.ent[a].f30 = Gen::angle_between(g.ent[a].x, g.ent[a].y, g.ent[b].x, g.ent[b].y);
            g.ent[b].f30 = Gen::angle_between(g.ent[b].x, g.ent[b].y, g.ent[a].x, g.ent[a].y);
            g.creature_tick(a, &ctx);
            g.creature_tick(b, &ctx);
        }
        let a_packed = g.ent[a].tick70 == 27 && g.ent[a].f52 != 0;
        let b_packed = g.ent[b].tick70 == 27 && g.ent[b].f52 != 0;
        assert!(
            a_packed ^ b_packed,
            "exactly one militiaman falls in behind the other (a state {} f52 {}, b state {} f52 {})",
            g.ent[a].tick70,
            g.ent[a].f52,
            g.ent[b].tick70,
            g.ent[b].f52,
        );
        let (follower, leader) = if a_packed { (a, b) } else { (b, a) };
        assert_eq!(
            g.ent[follower].f52 as usize, leader,
            "the follower's leader is its sibling"
        );
        assert_eq!(g.ent[leader].tick70, 25, "the chosen leader stays idle");
    }

    /// THE BIRTH-TICK EXCLUSION (sub_1B5D0 :22653-77). The pair-up scan
    /// walks `var_u32_36462[model]` — the per-model roster rebuilt at
    /// the TOP of the tick (:52287-313) — so a creature spawned DURING
    /// a tick is not yet a member and cannot pair, in either direction,
    /// until the next rebuild. mc1l2's village collapse evacuates
    /// militia into three slots in ONE tick and retail leaves all three
    /// unpaired; the port's old POOL scan paired them on their shared
    /// birth tick, and a packed militiaman follows its leader instead of
    /// running the two-draw wander — so its own per-entity LCG stops
    /// advancing and every later roll on it is off by the draws it never
    /// made (mc1l2 free-run horizon 4965 → 5674 on this one law).
    ///
    /// ⚠ This law is INVISIBLE to the pair-mode fixture harness: the obs
    /// schema carries neither `+52` nor `+70`, and pair mode re-imports
    /// retail's state every tick, so an importing runner hands the port
    /// `f52 = 0` and watches it wander correctly. This test IS the
    /// regression guard.
    #[test]
    fn a_creature_born_this_tick_cannot_pair_up() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let ctx = ctx_at(0xC000, 0xC000, 0);
        // Two militia born together, close enough and facing each other
        // — everything the pair-up needs EXCEPT chain membership.
        let a = g.spawn_creature(4, 0x4000, 0x4000, 0).unwrap();
        let b = g.spawn_creature(4, 0x4300, 0x4000, 0).unwrap();
        g.ent[a].f30 = Gen::angle_between(g.ent[a].x, g.ent[a].y, g.ent[b].x, g.ent[b].y);
        g.ent[b].f30 = Gen::angle_between(g.ent[b].x, g.ent[b].y, g.ent[a].x, g.ent[a].y);
        // Put both on the ladder cadence, so the scan is reached on the
        // very first tick they run: `+63 % (4 * v_26) == 0`.
        g.ent[a].f63 = 0;
        g.ent[b].f63 = 0;

        // The tick they were born in: the roster predates them.
        g.creature_tick(a, &ctx);
        g.creature_tick(b, &ctx);
        assert_eq!(
            (g.ent[a].f52, g.ent[b].f52),
            (0, 0),
            "a creature born this tick is not a chain member and cannot pair"
        );

        // The next tick-top rebuild admits them, and the same scan now
        // finds exactly what it refused before — proving the refusal was
        // the MEMBERSHIP, not the range, cone or cadence.
        g.rebuild_mob_chains();
        g.ent[a].f63 = 0;
        g.ent[b].f63 = 0;
        g.creature_tick(a, &ctx);
        g.creature_tick(b, &ctx);
        assert!(
            (g.ent[a].f52 != 0) ^ (g.ent[b].f52 != 0),
            "once chained, exactly one falls in behind the other (a f52 {}, b f52 {})",
            g.ent[a].f52,
            g.ent[b].f52,
        );
    }

    /// The militia death slot (sub_1BC10 :22729) gates on +26: nonzero
    /// = the silent house-absorb walk-in, zero = the normal corpse path
    /// and its mana ball (spawn +140 = life/2 = 500, sub_386DE). Retail
    /// re-zeroes +26 as the FIRST statement of every idle tick
    /// (sub_1B5D0 :22482), so the spawn stagger (+26 = slot % 100)
    /// never reaches the gate. Our port had dropped that zero, so once
    /// the absorb gate widened to m4 virtually every militiaman died
    /// silently — no corpse, no mana ball ("archers stopped dropping").
    #[test]
    fn combat_killed_militia_corpses_and_drops_its_mana_ball() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let i = g.spawn_creature(4, 0x4000, 0x4000, 0).unwrap();
        assert_eq!(g.ent[i].f140, 500, "militia carries life/2 = 500 mana");
        // Force the trap regardless of which slot the fixture hands out.
        g.ent[i].f26 = 37;
        // One idle tick with nothing to fight: retail zeroes +26 here.
        let ctx = ctx_at(0xC000, 0xC000, 0);
        g.creature_tick(i, &ctx);
        // Kill him: the inbox death routes idle to the death slot (28).
        g.ent[i].tick70 = 28;
        g.creature_tick(i, &ctx);
        assert_eq!(
            g.ent[i].tick70, 29,
            "combat death takes the corpse path, not the silent absorb"
        );
        g.ent[i].f63 = 0; // on the corpse's 8-tick drop beat
        g.creature_tick(i, &ctx);
        let ball = (1..g.ent.len())
            .find(|&b| g.ent[b].class64 == 10 && g.ent[b].model65 == 39)
            .expect("the corpse dropped a mana ball");
        assert_eq!(g.ent[ball].f140, 500, "the ball carries the 500 mana");
    }

    /// The counterpart the absorb gate exists for: a militiaman who
    /// walked back into a house (+26 = 1, sub_1B5D0 :22561) reaches the
    /// same death slot and despawns silently — no corpse, no ball.
    #[test]
    fn house_walkin_militia_still_absorbs_silently() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let i = g.spawn_creature(4, 0x4000, 0x4000, 0).unwrap();
        g.ent[i].f26 = 1; // the house-branch walk-in mark
        g.ent[i].tick70 = 28;
        let ctx = ctx_at(0xC000, 0xC000, 0);
        g.creature_tick(i, &ctx);
        assert!(
            g.ent[i].flags & 0x400 != 0,
            "the walk-in despawns instead of corpsing"
        );
        assert!(
            (1..g.ent.len()).all(|b| g.ent[b].class64 != 10 || g.ent[b].model65 != 39),
            "no mana ball from an absorbed walk-in"
        );
    }

    /// The m15 castle guard's wizard-acquisition scan (sub_1FF60
    /// :25733-64): a rival-owned guard, awake, with the wizard in
    /// range+cone, promotes into the STATIONARY chase and stops (the
    /// sub_20410 entry trailer). A human-owned guard never targets the
    /// human (the owner gate) — the fix that made castle L3+ archers
    /// actually engage instead of patrolling harmlessly.
    #[test]
    fn m15_guard_scans_and_chases_the_wizard() {
        let mk = |owner: u16| {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                crate::verbs::VerbSet::MC1,
            );
            let i = g.spawn_creature(15, 0x4000, 0x4000, 0).unwrap();
            assert_eq!(
                g.ent[i].tick70, 91,
                "m15 spawns into the guard-wander state"
            );
            g.ent[i].id24 = owner;
            g.ent[i].f58 = 16; // awake
            g.ent[i].f63 = 15; // on the v_26 scan tick, NOT the heading-vote tick
            let ctx = ctx_at(0x4200, 0x4000, 0); // 2 tiles east, well inside v_28
            g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, ctx.px, ctx.py); // in cone
            g.creature_tick(i, &ctx);
            (g.ent[i].tick70, g.ent[i].f146, g.ent[i].f126)
        };
        // Rival-owned guard: acquires the wizard, enters chase, stops.
        assert_eq!(
            mk(50),
            (92, crate::mc1::mobs::PLAYER_TARGET, 0),
            "the rival guard chases the wizard and halts (entry trailer)"
        );
        // Human-owned guard: the owner gate keeps it patrolling.
        assert_eq!(
            mk(crate::mc1::mobs::PLAYER_TARGET).0,
            91,
            "a human-owned guard never targets the human"
        );
    }

    /// The crab egg (`sub_3B860`/`sub_296A0`/`sub_29700`): the creator
    /// stamps state 56, the incubation timer counts down and promotes
    /// to the hatch (57), which lays a WILD m5 crab and self-despawns.
    /// Regression guard for the model-52 → state-52 misroute (eggs used
    /// to masquerade as live village buildings).
    #[test]
    fn crab_egg_incubates_and_hatches_a_wild_crab() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let egg = g.spawn_creator(52, 0x4000, 0x4000, 0).unwrap();
        assert_eq!(
            g.ent[egg].tick70, 56,
            "the egg starts incubating (state 56)"
        );
        assert_eq!((g.ent[egg].class64, g.ent[egg].model65), (10, 52));
        assert_eq!(g.ent[egg].act_life, 100000, "the safety timeout is armed");

        // The layer's real hatch timer (here a short 3), then count down.
        g.ent[egg].f26 = 3;
        for _ in 0..3 {
            g.tick_egg_incubate(egg);
            assert_eq!(g.ent[egg].tick70, 56, "still incubating");
        }
        g.tick_egg_incubate(egg); // the tick that reads f26 == 0
        assert_eq!(g.ent[egg].tick70, 57, "f26 hitting 0 promotes to hatch");

        let crabs = |g: &Gen| {
            (1..g.ent.len())
                .filter(|&j| {
                    g.ent[j].class64 == 5 && g.ent[j].model65 == 5 && g.ent[j].act_life >= 0
                })
                .count()
        };
        let before = crabs(&g);
        g.tick_egg_hatch(egg);
        assert_ne!(
            g.ent[egg].flags & 0x400,
            0,
            "the egg despawns after hatching"
        );
        assert_eq!(crabs(&g), before + 1, "one m5 crab hatched");
        let crab = (1..g.ent.len())
            .find(|&j| g.ent[j].class64 == 5 && g.ent[j].model65 == 5 && g.ent[j].act_life >= 0)
            .unwrap();
        assert_eq!(g.ent[crab].tick70, 31, "the crab spawns in its m5 state");
        assert_eq!(
            g.ent[crab].id24, crab as u16,
            "the crab is WILD (owns itself, not the layer's owner)"
        );
    }

    /// The model-52 egg no longer aliases into the live-building state:
    /// a fresh egg dispatches to the incubation handler, never
    /// `tick_building_live`, so it can never masquerade as a village.
    #[test]
    fn crab_egg_does_not_become_a_phantom_village() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let egg = g.spawn_creator(52, 0x4000, 0x4000, 0).unwrap();
        // The old bug stamped tick70 = model = 52 (the live-building
        // state); the fix stamps 56 and gates state 52 on model 45.
        assert_ne!(
            g.ent[egg].tick70, 52,
            "the egg is not in the building state"
        );
        assert_eq!(g.ent[egg].tick70, 56);
    }

    /// Only the %-forms of the m18 timer table draw the per-entity
    /// LCG; the flat forms are draw-free (an unconditional pre-draw
    /// would desync the tank's rand stream), and (0,1)/(2,1) carry the
    /// pinned retail values.
    #[test]
    fn m18_timer_values_and_rng_parity() {
        let mut g = mc2_gen();
        let i = g.mc2_spawn_m18(0x4000, 0x4000, 300).unwrap();
        for (role, sub, flat) in [(2u8, 1u8, Some(10i16)), (2, 2, Some(12)), (2, 3, Some(14))] {
            let r0 = g.ent[i].rand;
            g.m18_timer(i, role, sub);
            assert_eq!(g.ent[i].f26, flat.unwrap(), "flat value ({role},{sub})");
            assert_eq!(g.ent[i].rand, r0, "flat forms draw NOTHING ({role},{sub})");
        }
        let r0 = g.ent[i].rand;
        g.m18_timer(i, 0, 1);
        assert!(
            (60..120).contains(&g.ent[i].f26),
            "(0,1) = 60 + rand%60, got {}",
            g.ent[i].f26
        );
        assert_ne!(g.ent[i].rand, r0, "(0,1) draws exactly its one roll");
    }

    /// Every in-range drain path STAYS in state 210 — only a
    /// target beyond the row range exits to 209.
    #[test]
    fn m26_leech_stays_draining_in_range() {
        let mut g = mc2_gen();
        let i = g.mc2_spawn_m26(0x4000, 0x4000, 300).unwrap();
        g.ent[i].tick70 = 210; // M26_BASE + 2, the drain state
        g.ent[i].f146 = crate::mc1::mobs::PLAYER_TARGET;
        g.ent[i].f63 = 0;
        let near = ctx_at(0x4100, 0x4000, 300); // 256 away, avatar
        let drained0 = g.mc2_player_drain.0;
        g.m26_tick(i, &near);
        assert_eq!(g.ent[i].tick70, 210, "in-range avatar: stay draining");
        assert!(g.mc2_player_drain.0 > drained0, "the drain landed");
        // Far target: the one authentic exit.
        let far = ctx_at(0x4000u16.wrapping_add(0x7000), 0x4000, 300);
        g.ent[i].f63 = 0;
        g.m26_tick(i, &far);
        assert_eq!(g.ent[i].tick70, 209, "out of range: back to approach");
    }

    /// The aura claim handshake — the first aura in slot order keeps
    /// an overlapped ball; the second must not overwrite the pull
    /// (first-writer-wins, NOT last-writer-wins).
    #[test]
    fn mc2_aura_first_claim_wins() {
        let mut g = mc2_gen();
        let mk_aura = |g: &mut Gen, x: u16| {
            let a = g.new_event().unwrap();
            let e = &mut g.ent[a];
            e.x = x;
            e.y = 0x4000;
            e.f26 = 14; // tile range
            e.act_life = 100;
            a
        };
        let a1 = mk_aura(&mut g, 0x4000);
        let a2 = mk_aura(&mut g, 0x4600);
        let b = g.new_event().unwrap();
        {
            let e = &mut g.ent[b];
            e.class64 = 10;
            e.model65 = 39;
            e.x = 0x4200;
            e.y = 0x4000;
        }
        g.mc2_aura_tick(a1);
        let claimed = (g.ent[b].dest_x, g.ent[b].dest_y);
        assert_eq!(
            g.mc2_aura_claim.0.get(&(b as u16)),
            Some(&(a1 as u16)),
            "aura 1 claims the ball"
        );
        g.mc2_aura_tick(a2);
        assert_eq!(
            (g.ent[b].dest_x, g.ent[b].dest_y),
            claimed,
            "the second aura must not steal the claimed ball's pull"
        );
    }

    /// The m12 template walk falls back to 17 on exhaustion
    /// (empty bldgprm).
    #[test]
    fn m12_template_pick_falls_back_to_17() {
        let mut g = mc2_gen();
        assert_eq!(g.m12_pick_template(), 17, "exhaustion returns 17");
    }

    /// The m25 death split under pool exhaustion still FALLS THROUGH
    /// to the (10,1) burst + the state advance.
    #[test]
    fn m25_split_exhausted_pool_still_bursts() {
        let mut g = mc2_gen();
        let i = g.mc2_spawn_m25(0x4000, 0x4000, 300).unwrap();
        g.ent[i].tick70 = 204; // M25_BASE + 4, the split state
        g.ent[i].f71 = 0;
        g.ent[i].f140 = 0; // no mana: the sphere dump spawns nothing
        let spare = g.new_event().unwrap();
        while g.new_event().is_some() {}
        g.free.push(spare as u16); // exactly one slot: <= 1 = exhausted
        let ctx = ctx_at(0x1000, 0x1000, 300);
        g.m25_tick(i, &ctx);
        assert_eq!(g.ent[i].tick70, 205, "the split advanced past itself");
        assert!(
            g.ent
                .iter()
                .any(|e| e.class64 == 10 && e.model65 == 1 && e.flags & 0x400 == 0),
            "the (10,1) burst fired on the exhaustion path"
        );
    }

    /// The Summon Army ring — a firefly (model 19) cast raises
    /// EIGHT allied nodes (weak-swarm size), every one carrying the
    /// caster's id24, the allied StageVar2=13 marker, the 8·M+7
    /// action and the 250-tick lifespan.
    #[test]
    fn summon_army_ring_is_eight_allied_fireflies() {
        let mut g = mc2_gen();
        g.mc2_spawn_summon_ring(0x4000, 0x4000, 19, 0x77);
        let nodes: Vec<&Ent> = g
            .ent
            .iter()
            .filter(|e| e.class64 == 5 && e.model65 == 19 && e.flags & 0x400 == 0)
            .collect();
        assert_eq!(nodes.len(), 8, "firefly army size");
        for e in nodes {
            assert_eq!(e.id24, 0x77, "allied to the caster");
            assert_eq!(e.site_z, 13, "the summon-army StageVar2 marker");
            assert_eq!(e.tick70, 19u8.wrapping_mul(8).wrapping_add(7));
            assert_eq!(e.f26, 250, "the 250-tick lifespan");
        }
    }

    /// Falling-prop gravity is position-THEN-decrement — the
    /// position takes the OLD velocity before the −24 applies.
    #[test]
    fn falling_prop_position_takes_old_velocity() {
        let mut g = mc2_gen();
        let i = g.new_event().unwrap();
        let ground = g.ground_z(0x4000, 0x4000) as i16;
        {
            let e = &mut g.ent[i];
            e.class64 = 2;
            e.model65 = 7;
            e.x = 0x4000;
            e.y = 0x4000;
            e.z = ground + 400;
            e.f44 = 100u16; // upward velocity
            e.f126 = 0;
            e.act_life = 100;
        }
        let z0 = g.ent[i].z;
        g.mc2_falling_tick(i);
        assert_eq!(g.ent[i].z, z0 + 100, "position moved by the OLD velocity");
        assert_eq!(g.ent[i].f44 as i16, 76, "then the velocity decremented");
    }

    #[test]
    fn deterministic() {
        let assets = synthetic_assets();
        let things = vec![
            thing(0, 10, 9, 50, 50),
            thing(1, 10, 11, 60, 60),
            thing(2, 10, 45, 80, 80),
        ];
        let mut p1 = flat_land(90);
        let mut p2 = flat_land(90);
        run(&mut p1, &things, 4242, &assets);
        run(&mut p2, &things, 4242, &assets);
        assert_eq!(p1.height, p2.height);
        assert_eq!(p1.tile_type, p2.tile_type);
        assert_eq!(p1.angle, p2.angle);
        assert_eq!(p1.shading, p2.shading);
    }

    /// THE BLAST RING'S CHILDREN INHERIT ITS HEADING. `sub_25CE0`
    /// :28717 copies the ring's `+30` into every fire it lays, exactly
    /// as the spreader's `sub_24E60` :28176 does — the port set the
    /// child's id24, flags, extents and `+26` and dropped that one
    /// line, so every blast-ring fire was born heading 0. mc1l32
    /// t=23132 caught it as 75 newborn (10,0) rows, all children of one
    /// (10,17) ring, every one `heading: retail 724 port 0`.
    ///
    /// Guarded here rather than by a fixture: the only corpus exemplar
    /// sits in a format-1 take whose pristine terrain keeps that pair
    /// permanently divergent, so no l32 fixture can ever be conforming.
    #[test]
    fn blast_ring_children_inherit_the_rings_heading() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let ctx = ctx_at(0xC000, 0xC000, 0);
        let ring = g.spawn_effect(17, 0x4000, 0x4000, 0).expect("ring slot");
        {
            let e = &mut g.ent[ring];
            e.f30 = 724;
            e.f26 = 3; // a non-zero radius, so the ring actually lays cells
            e.max_life = 10;
            e.act_life = 5;
            e.f44 = 8000;
        }
        g.effect_tick(ring, &ctx);
        let kids: Vec<usize> = (1..g.ent.len())
            .filter(|&j| {
                j != ring
                    && g.ent[j].class64 == 10
                    && g.ent[j].model65 == 0
                    && g.ent[j].flags & 0x400 == 0
            })
            .collect();
        assert!(!kids.is_empty(), "a radius-3 ring lays fires");
        for k in kids {
            assert_eq!(g.ent[k].f30, 724, "slot {k} inherits the ring's +30");
        }
    }

    /// A HIT FREEZES THE CRAB BUT ITS REGEN TRAILER STILL RUNS. Retail's
    /// m5 wrappers `sub_1BF60` (:22959-65) and `sub_1C110` (:22976-82)
    /// call the shared handler and THEN run `act += max >> 7`
    /// unconditionally — the hit abort happens inside `sub_1A120`,
    /// below them, so it cannot skip the trailer. The port's
    /// centralized intake returned above the whole per-state match and
    /// lost it, which is the banked HIT-ABORT RESTRUCTURE's item 4:
    /// the blanket abort OVER-aborts.
    ///
    /// mc1l32 t=23132: 16 crabs in state 32 take the blast ring's 800
    /// and retail freezes their movement exactly as the port does, yet
    /// every one still lands its regen — retail above the port by
    /// precisely `max_life >> 7`.
    #[test]
    fn a_hit_freezes_the_crab_but_its_regen_trailer_still_runs() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let ctx = ctx_at(0xC000, 0xC000, 0);
        // The attacker is another CREATURE, so the wizard-only arm of
        // the intake cannot be what keeps the mover still.
        let biter = g.spawn_creature(4, 0x5000, 0x5000, 0).expect("biter slot");
        assert!(
            !g.attacker_is_wizard(biter as u16),
            "the fixture's attacker was a crab, not a wizard"
        );
        let crab = g.spawn_creature(5, 0x4000, 0x4000, 0).expect("crab slot");
        {
            let e = &mut g.ent[crab];
            e.tick70 = 32; // (5,2) — the chase state the corpus rows sit in
            e.max_life = 10000;
            e.act_life = 5000;
        }
        let (x0, y0) = (g.ent[crab].x, g.ent[crab].y);
        g.mail_write_single(
            crate::mc1::combat::MailTarget::Pool(crab),
            0,
            800,
            biter as u16,
        );
        g.creature_tick(crab, &ctx);
        assert_eq!(
            (g.ent[crab].x, g.ent[crab].y),
            (x0, y0),
            "the hit still freezes the mover"
        );
        assert_eq!(
            g.ent[crab].act_life,
            5000 - 800 + (10000 >> 7),
            "the wrapper's regen trailer survives the freeze"
        );
    }

    /// THE VULTURE IS THE ONLY CREATURE THAT MOVES WHILE IDLE. m1's
    /// idle wrapper `sub_1B160` (:22222-46) calls the shared idle and
    /// then `sub_196E0` — the mover — as a wrapper TRAILER, then
    /// re-aims at its target or drops it. Nine other idle wrappers
    /// exist and every one is a 3-5 line body with no mover, so
    /// `Gen::mob_idle` (the shared `sub_19B10`, which ends at the pack
    /// scan) was never the culprit. remc1hw :20779-801 is
    /// byte-identical.
    ///
    /// mc1l32 t=23132 slot 28: retail stepped the bird 98 units — its
    /// own `f126` — along `f30 = 1288` with ZERO LCG draws while the
    /// port left it bit-identical. Guarded here because that pair's
    /// take is format 1: its pristine terrain keeps the pair
    /// permanently divergent, so no fixture of it can ever be green.
    #[test]
    fn only_the_vulture_moves_while_idle() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let ctx = ctx_at(0xC000, 0xC000, 0);
        let bird = g.spawn_creature(1, 0x4000, 0x4000, 0).expect("m1 slot");
        g.ent[bird].tick70 = 6; // (1,0), the idle state
        let before = (g.ent[bird].x, g.ent[bird].y);
        let rand_before = g.ent[bird].rand;
        g.creature_tick(bird, &ctx);
        assert_ne!(
            (g.ent[bird].x, g.ent[bird].y),
            before,
            "the idle vulture still runs sub_196E0"
        );
        assert_eq!(
            g.ent[bird].rand, rand_before,
            "the mover costs no per-entity LCG draw"
        );
        // A creature whose idle carries NO mover is the control: m0's
        // sub_1B060 (:22166-69) is a bare call, so it must stand still.
        let worm = g.spawn_creature(0, 0x6000, 0x6000, 0).expect("m0 slot");
        g.ent[worm].tick70 = 0; // (0,0), idle
        let wbefore = (g.ent[worm].x, g.ent[worm].y);
        g.creature_tick(worm, &ctx);
        assert_eq!(
            (g.ent[worm].x, g.ent[worm].y),
            wbefore,
            "every other idle wrapper has no mover"
        );
    }

    /// THE GROWL ARMS THE BURST AND THE SAME TICK SPENDS ITS FIRST
    /// CHARGE. `sub_1C4F0`'s spit block (:23243-66) sits BELOW the
    /// cadence gate that writes `+71 = 5` (:23241), so an in-range
    /// cadence tick growls, arms five and immediately lays the first
    /// beam — a burst is five bolts starting on the arming tick, not
    /// four starting the tick after. The port tested `+71 > 0` above
    /// the gate and lost the opening bolt of every burst.
    ///
    /// It keeps a unit test BESIDE its fixture
    /// (`a-kraken-growl-tick-already-lays-its-first-bolt`, mc1l42
    /// t=6453) because the two pin different halves: the fixture
    /// asserts the whole tick against retail, this asserts the
    /// ORDERING directly — that `+71` lands on 4 and not 5 — which is
    /// the single number the law turns on, and it says so without a
    /// 180 KB evidence file or a corpus to import.
    #[test]
    fn the_kraken_growl_lays_its_first_bolt_on_the_arming_tick() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        // One tile away: inside any behavior row's v_28 keep-chasing
        // radius, so the cadence tick takes the growl arm and not the
        // drop-out to WANDER.
        let ctx = ctx_at(0x4180, 0x4080, 0);
        // Mid-tile, because `flat_land` is dry and row 18's v_20 is
        // water-only: a kraken that crosses a tile boundary here fails
        // all four candidates and `creature_move` kills it (:21293).
        // Started at the tile centre, its pinned 30-unit step never
        // leaves the tile and takes `move_probe`'s same-tile shortcut.
        let k = g.spawn_creature(6, 0x4080, 0x4080, 0).expect("m6 slot");
        g.ent[k].tick70 = 38; // (6,2) chase — base 36 + role 2
        g.ent[k].f146 = crate::mc1::mobs::PLAYER_TARGET;
        g.ent[k].f71 = 0; // no burst pending
        g.ent[k].f63 = 0; // a cadence tick for every v_26
        // The worm ctor leaves the head's life for the level loader to
        // refill; mc1l42's krakens carry 9000/9000.
        g.ent[k].max_life = 9000;
        g.ent[k].act_life = 9000;
        let beams = g
            .ent
            .iter()
            .filter(|e| (e.class64, e.model65) == (9, 9))
            .count();
        g.creature_tick(k, &ctx);
        assert_eq!(
            g.ent[k].f71, 4,
            "the growl arms five and the arming tick spends one \
             (:23241 then :23245) [state {} life {}]",
            g.ent[k].tick70, g.ent[k].act_life
        );
        assert_eq!(
            g.ent
                .iter()
                .filter(|e| (e.class64, e.model65) == (9, 9))
                .count(),
            beams + 1,
            "the arming tick lays a beam of its own"
        );
        // The burst then runs on every following tick while armed,
        // cadence or not — the block is outside the `!v16` gate.
        g.ent[k].f63 = 1;
        g.creature_tick(k, &ctx);
        assert_eq!(g.ent[k].f71, 3, "an off-cadence tick still spends a charge");
    }
}
