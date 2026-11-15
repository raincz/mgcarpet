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
use crate::engine::world::{Hx, Seats};
use crate::verbs::{VerbKind, VerbSet};

/// A/B toggle for the MC1 CLAIMED-DWELLING FLAG EXTENTS: set
/// `MGC_NO_MC1_HOUSE_FLAG_EXTENTS` to restore the pre-dig fold, where
/// the claim stamped `set_sprite(177 + team)` and so took the
/// building's collision quad off the TEAM's sprite row instead of row
/// 177's. Citation at the write site in [`Gen::tick_building_live`].
pub(crate) fn no_mc1_house_flag_extents() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_HOUSE_FLAG_EXTENTS").is_some())
}

/// PROBE, not a law (round 157): `MGC_FORCE_BUILDING_PATCHES=1` lets
/// the three BUILD patches (`mc1_crushed_site_collapse`,
/// `mc1_building_pad_saturate`, `mc2_building_pad_saturate`) fire in a
/// `strict_retail` world when its patch set enables them. Only
/// `mgc-conform`'s `MGC_REPLAY_BUILDING_PATCHES` probe installs such a
/// set, to replay a retail witness take with the patches on. Unset,
/// strict still pins every patch to retail.
pub(crate) fn force_building_patches() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_FORCE_BUILDING_PATCHES").is_some())
}

/// PROBE, not a law (round 163): `MGC_FORCE_VOLCANO_GUARD=1` lets
/// `volcano_register_revalidate` fire in a `strict_retail` world when
/// its patch set enables it — the sibling of
/// [`force_building_patches`], and for the same reason.
///
/// ⭐ THIS ONE GRADES A *RECORDING*, NOT JUST THE PORT. The round-162
/// `mc_exe_tickpatch.py --volcano-guard` binary patch installs the
/// DOS-side half of this very law, so a take recorded on
/// `*_RECVG.EXE` witnesses the PATCHED arm. Replaying such a take
/// against the faithful (unguarded) port arm reports the port's own
/// blind writes as divergences — `flags` rows with a constant
/// `port - retail = 0x400`, the plume soft-kill. With this probe the
/// two halves of the same patch face each other and the take grades.
///
/// ⚠ A take replayed under this probe is NOT certified against
/// retail. It is certified against the guarded binary, which is a
/// different simulation. Say so wherever the number is quoted.
pub(crate) fn force_volcano_guard() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_FORCE_VOLCANO_GUARD").is_some())
        || TAKE_VOLCANO_GUARD.load(std::sync::atomic::Ordering::Relaxed)
}

/// THE TAKE DECLARES ITS BINARY (round 166). A retake's header carries
/// `capture.exe.volcano_guard`; a driver that replays such a take sets
/// this for the run, and the guarded arm of
/// `volcano_register_revalidate` then grades the take against the
/// binary that produced it. Process-wide on purpose, like the probe it
/// replaces: a replay process stands on one take at a time.
static TAKE_VOLCANO_GUARD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// See [`TAKE_VOLCANO_GUARD`]. The driver must ALSO raise
/// `patches.volcano_register_revalidate` on its world after every
/// import (the importers re-force the retail set).
pub fn set_take_volcano_guard(on: bool) {
    TAKE_VOLCANO_GUARD.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the take in hand was recorded on the volcano-guarded binary.
pub fn take_volcano_guard() -> bool {
    TAKE_VOLCANO_GUARD.load(std::sync::atomic::Ordering::Relaxed)
}

/// A/B toggle for THE AUTHORED STANDING FIRE'S REAL CTOR (round 157,
/// w157a): set `MGC_NO_MC1_CREATOR_STANDING_FIRE` to restore the pre-dig
/// [`Gen::spawn_creator`], whose model-6 row fell to the generic
/// "purged unticked at load" stub — a bare `new_event` (life 0, max 300,
/// flags 8, `+44` 100, no sprite, no extents) — instead of the standing
/// fire ctor `sub_3A730` (remc1 sub_main.cpp:46620) that
/// [`Gen::spawn_effect`] already ports.
///
/// ⭐ THE STUB WAS RIGHT AT LOAD AND WRONG AT RUNTIME. Retail has ONE
/// class-10 creator table (`str_255D0C`, remc1 :4486, the `dword_96902[10]`
/// row :5041 that `sub_373F0_377B0` dispatches for a disposition fire,
/// `sub_37560_37920` :43988) and its row 6 is `sub_3A730` — CARPET.EXE
/// file 0x9D558 (table base 0x9D504 + 6*14): `f4 68 00 00 | 06 00 |
/// 30 a7 02 00 | 01 00`, object-relative 0x2A730 = VA 0x3A730. The ctor
/// at file 0x52F28: `movb $6,+0x46` / `movb $0xa,+0x40` / `movb $6,+0x41`
/// / `movw $0x32,+0x2c` (f44 50) / `movl $0xf0,+0x8` (life 240) /
/// `and $0xfffdfff7,+0x10` / `or $2,+0x12` (flags 0x20000) / link /
/// `+0x4c = sub_11F50` (ground snap) / RefillLife / `push $0xe4` sprite
/// 228 / `push $0x600; push $0x110` extents 272/1536 / `+0x1a = 0`.
/// WITNESS mc1l38 t=4771: the `(11,0)` volume at slot 140 fires its
/// disposition and mints ten authored `(10,6)` fires (slots 21-32);
/// retail births each at life 240 / flags 0x20004 / sprite 228, the
/// port at 0 / 8 / 300 — 40 rows, one decision.
pub(crate) fn no_mc1_creator_standing_fire() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CREATOR_STANDING_FIRE").is_some())
}

/// A/B toggle for THE AUTHORED `(10,8)` MARKER'S REAL CTOR (round 166).
/// `spawn_creator` sent model 8 down its generic arm, which writes
/// class, model, pose and a state byte and nothing else; retail's
/// table row is `sub_3A870` (life 8, damage 100, extents 512). WITNESS
/// mc1hwl10 t=19664 and t=19686: a trigger mints seven authored
/// `(10,8)` records (slots 12, 17, 18, …), retail reads `max_life 8 /
/// life 8 / flags 0`, the port `300 / 0 / 8`. They self-kill on their
/// first tick (`sub_253E0`), so the birth frame is all there is to
/// see. `MGC_NO_MC1_CREATOR_M8=1` restores the generic arm.
pub(crate) fn no_mc1_creator_m8() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CREATOR_M8").is_some())
}

/// A/B toggle for THE GENIE EATS FROM THE TICK-TOP BALL CHAIN (round
/// 167). [`Gen::genie_eat_ball`] scanned the POOL for a live `(10,39)`
/// and skipped a record carrying `0x400`. Retail's `sub_1E810` (remc1hw
/// sub_main.cpp:23308) walks the tick-top ball chain `+36466` and tests
/// `+65 == 39` and nothing else — HIDDEN.EXE file 0x37241 `mov
/// 0x8e72(%eax),%eax`, 0x37249 `cmpb $0x27,0x41(%eax)`, the range test
/// `ja` at 0x37276 and the strict nearer test `jae` at 0x3727a. The
/// destroy it calls (`sub_41E80`) only sets `+17 |= 4`, so an eaten ball
/// stays a member until the next rebuild and EVERY genie whose eat runs
/// later in the tick takes its whole `+140` again. WITNESS mc1hwl17
/// t=45101: the human dies, genies 9, 10 and 11 all take the
/// dead-target arm and each reads mana 10000 -> 10500 off ball 442
/// (500), three `(10,0)` puffs; the port fed genie 9 alone.
/// `MGC_NO_MC1_GENIE_EAT_CHAIN=1` restores the pool scan.
pub(crate) fn no_mc1_genie_eat_chain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_GENIE_EAT_CHAIN").is_some())
}

/// A/B toggle for THE LIVE HOUSE'S ONE-TICK HIT REGISTER (round 154,
/// w154e): set `MGC_NO_MC1_HOUSE_HIT_REGISTER` to restore the pre-dig
/// intake in [`Gen::tick_building_live`] — `f40` latched forever after
/// the first hit, the ch0 src cleared before the life test, the ch0
/// amount never cleared. Citation at the write sites.
pub(crate) fn no_mc1_house_hit_register() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_HOUSE_HIT_REGISTER").is_some())
}

/// A/B toggle for THE LOAD PASS'S AREA MAIL (round 155, w155e): set
/// `MGC_NO_MC1_LOAD_PASS_AREA_MAIL` to restore the pre-dig load-time
/// fixpoint, whose terrain deformers (the `(10,9)` hill, the `(10,11)`
/// digger, the `(10,51)` ridge head) skipped their ch0 area write when
/// no runtime `MobCtx` was in hand ("nothing observable survives it").
///
/// ⭐ SOMETHING DOES SURVIVE IT: THE HOUSE'S MAILBOX. Retail's load
/// loop `sub_36620_369E0` (:43181) dispatches the SAME handler table
/// (`dword_96902[..].data6`) as the runtime, and the digger
/// `sub_25670` (:28379) calls `sub_127E0(a1x, 0, v2)` (:28400)
/// unconditionally — CARPET.EXE VA 0x256B8-0x256DC `f6 43 10 02` /
/// `66 8b 43 2c` / `b9 19 00 00 00 … f7 f9` / `50 6a 00 53` /
/// `e8 ff d0 fe ff` (= `call 0x127E0`). The level's `(10,45)` dwellings
/// are ALREADY in the pool by then (dispatched and `sub_36DF0`-fixed up
/// in table order, linked, `+16 & 8`, `+28 & 1`), sitting in state 51,
/// whose construction handler never reads `+90/+94` — so every canyon
/// digger's 200 + 8 + 8 (= 216: life 2, full `+44` on the first tick,
/// `+44 / 25` once `+16 & 2` is set) ACCUMULATES there (the area
/// protocol adds while a source is pending) and the house's first
/// state-52 tick (`sub_29640` :31070) drains the whole bill at once.
/// WITNESS mc1l36 record 0: house slot 68 `(10,45)` at tile (53,226),
/// south of the `(10,31)` canyon node at (54,217), dead at settle 2
/// with `mail0 = (4536, 9)` = 21 × 216 from canyon head 9 (`+38 = 9`),
/// neighbours 65 / 67 at `act_life` 920 / 704 = 2000 − 5 × 216 /
/// 2000 − 6 × 216; mc1l34 slot 272 at 488 = 2000 − 7 × 216 and houses
/// 268 / 270 collapsed. The collapse evacuations are the retail-only
/// `(5,4)` + `(5,12)` pairs (`spawn_count[4]`), the crater types
/// 48/51 are `sub_28FE0`'s repaint. No human record exists at load
/// (it is seated after GenerateFeatures), so the out-of-pool player
/// arm is skipped ([`Gen::area_write_opt`] with `ctx = None`).
pub(crate) fn no_mc1_load_pass_area_mail() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_LOAD_PASS_AREA_MAIL").is_some())
}

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

/// THE OVERFLOWING BUILDING PAD — the one goal clamp both games' BUILD
/// stampers share. The goal a stamper lerps a footprint cell toward is
/// ABSOLUTE: the site datum (`z >> 5`) plus the cell's pad.
///
/// RETAIL (`saturate == false`): no clamp, and every stamper steps
/// `h += (goal - h) / life` and stores a BYTE, so a goal past 255 wraps
/// and the tallest cells finish at `goal & 0xFF` — pits in a plateau.
/// - MC2: the construction tick `ApplyTerrainModification_37240`
///   (EF:27365; `NETHERW.EXE` VA 0x373E1 `idivl 0x8(%ebx)`, byte store
///   0x373ED `mov %al,0x4b4e0(%ecx)`) and the castle painter
///   `AddTerrainMod0A_2A_37BC0` (EF:27863/27891; VA 0x37F90 `idivl
///   0x10(%ebx)`, byte store 0x37FA9). Retail witness
///   `recordings/mc2l22.mgcr` slot 7 (row 43, datum 157, pad 127 →
///   goal 284): a 28-deep pit ringed by 241s, and the finished
///   building's z re-read off that pit (157*32 → 896).
/// - MC1: the construction tick `sub_27D30` (:29993; `CARPET.EXE` file
///   0x40650 `movswl 0x20(%esp)` = datum, 0x40655 zero-extended height,
///   0x40666 `idivl 0xc(%edi)`, 0x40669 `add`, 0x40673 `mov
///   %al,0x4c1e0(%ecx)`; the +12/+16/`4*(lo-1)` arms at
///   0x40702/0x40749/0x4077C-0x40788 each end in the same byte store).
///   MC1 pads top out at 56, so a dwelling wraps only on ground above
///   199..243 — no retail take has one (census of all 50 MC1 takes:
///   highest dwelling goal 222, mc1l34). MC1's castle painter wraps too
///   (witnessed, mc1l32-new), but its patch is a DATUM cap, not this
///   clamp — see [`Gen::castle_datum_cap`].
///
/// PATCHES `mc2_building_pad_saturate` (both MC2 stampers) and
/// `mc1_building_pad_saturate` (the MC1 dwelling): the goal saturates
/// to the height plane's range, so the pad tops out flat.
pub(crate) fn building_pad_goal(goal: i32, saturate: bool) -> i32 {
    if saturate { goal.clamp(0, 255) } else { goal }
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
    /// MC1 / Hidden Worlds: the sprite-stat rows' (width, height) as
    /// THIS LEVEL'S SPRITE SHEET derives them
    /// ([`crate::mc1::derive_sprite_stats`]). Empty = the sheet derives
    /// the static table ([`crate::mc1::sprite_stats::SPRITE_STATS`],
    /// the temperate bank's own derivation), or a pre-dims caller.
    pub mc1_sprite_ext: Vec<(u16, u16)>,
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
            mc1_sprite_ext,
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
        if !mc1_sprite_ext.is_empty() {
            mc1_sprite_ext.hash(state);
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
            mc1_sprite_ext: Vec::new(),
        })
    }

    /// Attach MC2's `BLDGPRM.DAT` table (4-byte records; the loader
    /// reads 76 x 4, sub_539A0 :38319 — we take every whole record
    /// present, then append retail's 77th, below).
    ///
    /// ⭐⭐⭐ THE 77TH RECORD IS NOT IN THE FILE, AND THE GAME USES IT.
    /// `str_D93C0_bldgprmbuffer` is `std::array<..., 77>` while
    /// `DataFileIO::Read` fills only `76 * sizeof(record)`
    /// (EF:38328), and `BLDGPRM.DAT` is 304 bytes — exactly the 76 it
    /// reads. Index 76 is therefore the four bytes that FOLLOW the
    /// buffer, and the address arithmetic names them: 0xD93C0 + 76*4
    /// = 0xD94F0, which is `str_D94F0_bldgprmbuffer`, the initialised
    /// map-type colour static `{0xAA,0x00,0x63,0x0D, …}`.
    ///
    /// It is REACHABLE, not theoretical: the villager's build lottery
    /// `sub_232C0` (EF:14481) draws `rand % 0x3C + 17`, whose maximum
    /// is exactly 76, and takes the index outright when `byte_2 & 2`
    /// — which 0x63 has. One villager build in sixty raises this
    /// phantom template.
    ///
    /// mc2l3 t=14611 pins all four bytes at once, on four separate
    /// graded lanes of the newborn (10,45) at slot 199: `b46` (the
    /// building id `byte_0x46_70`) reads **76**; `f2a` (`word_0` →
    /// `subSpellIndex_0x2A_42`) reads **170** = 0x00AA; `b38`
    /// (`byte_0x38_56`) reads **35** = 33|2, i.e. `byte_2 & 8` CLEAR
    /// so the building is productive, and 0x63 has bit 3 clear;
    /// `b3d` (`fontTypeIndex_0x3D_61` ← `byte_3`) reads **13** = 0x0D.
    ///
    /// ⚠ AND IT IS WHY THE SITE PASSED. `MSPRD00.DAT`'s row 76 is a
    /// 0x0 footprint, so `sub_226D0`'s extents floor at the flat 768
    /// clearance and `array_0x52_82` lands at 640/640 — the smallest
    /// box the site test can be asked. Filtered down to the 76 real
    /// rows the port drew id 19 (6x4) instead, and its roughness
    /// scan over the wider footprint vetoed a plot retail builds on.
    pub fn with_bldgprm(mut self, bytes: &[u8]) -> Self {
        self.bldgprm = bytes
            .chunks_exact(4)
            .map(|r| BldgParam {
                rate: u16::from_le_bytes([r[0], r[1]]),
                flags: r[2],
                chain: r[3],
            })
            .collect();
        // The read is `76 * sizeof(record)` into a 77-record array, so
        // a full-length file leaves the last record as the neighbour
        // static. A SHORT blob (the stand-in assets some tests build)
        // is left alone — there is nothing to be one past the end of.
        if self.bldgprm.len() == 76 {
            self.bldgprm.push(BldgParam {
                rate: 0x00AA,
                flags: 0x63,
                chain: 0x0D,
            });
        }
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
        // `SetDefaultSpells_5C0A0` — the boot-time table rewrite that
        // runs once over the freshly loaded DAT, before any level init
        // ([`crate::mc2::spells::set_default_spells`]).
        crate::mc2::spells::set_default_spells(&mut self.spells);
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

    /// Attach the MC1 / Hidden Worlds sprite-stat sizes this level's
    /// sprite sheet derives (feed [`crate::mc1::derive_sprite_stats`]
    /// with the bundle's sprite index dims; it answers empty for a
    /// sheet that derives the static table).
    pub fn with_mc1_sprite_ext(mut self, ext: Vec<(u16, u16)>) -> Self {
        self.mc1_sprite_ext = ext;
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
/// A/B toggle for THE (10,30) WATERPATH POINT'S DEFERRED STAMP: set
/// `MGC_NO_MC2_PATH_POINT_DEFERRED_STAMP` to restore the pre-dig
/// port, where a waterpath leg's two runs were stamped SYNCHRONOUSLY
/// inside the authoring pass (`World::mc2_stamp_path_leg`) instead of
/// at the records' own dispatch.
///
/// `sub_48690` (Events.cpp:5492) writes NO terrain: it mints two
/// (10,30) records — one per run — carrying `dword_0x10_16` (the run
/// length), `yaw_0x1C_28` / `pitch_0x1E_30` (the unit step) and the
/// run's start position. `PrepareEvents` puts model 0x1E in the
/// settle-TICK band, so `ApplyEvents_498A0` reaches each record on
/// the first sweep AFTER the whole pass finished authoring, and
/// `ApplyPointToPath_343F0` (EventsFunctions.cpp:25050) lays the run
/// there — angle nibble := 1 then `sub_462A0(cell, cell)` per cell —
/// before `DisableEntityDrawing04_57F10` frees the record.
///
/// ⭐⭐⭐ **THE COLLAPSE WAS A DRAW-ORDER DEFECT, NOT A COSMETIC ONE.**
/// `sub_462A0`'s blend pass draws the shared `pseudoRand` retile
/// stream (`rand2_17B4E0 = 9377 * rand2_17B4E0 + 9439`) for every
/// type-1 cell it retiles, and that draw is what picks the cell's
/// angle high nibble (`+ 16 * (rand2 % 7)`). Stamping the run at
/// AUTHORING time moves those draws ahead of every settle-band
/// sculptor of the same pass, so each one lands on a different cell:
///
/// - **mc2l9**: ONE path draw, at (115,27). Moving it from the
///   authoring pass to the record's slot shifted the whole
///   `pseudoRand` stream by one draw across the pass's scorch-ring /
///   hill discs — eight angle cells survived to record 0 with the
///   PREVIOUS draw's nibble ((147,16), (147,17), (180,89), (38,98),
///   (39,99), (56,116), (56,117), (67,176)).
/// - **mc2l11**: the (10,67) → (10,75) leg's 8-cell run makes three
///   draws; deferring them re-orders the two that survive, which is
///   why retail's (10,73)/(10,74) angles read 17/33 where the port
///   had 33/17.
///
/// Both takes are `terrain-check` IDENTICAL with the stamp deferred
/// and DIFFERENT (angle 8 / angle 2) with this switch set.
pub(crate) fn no_mc2_path_point_deferred_stamp() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_PATH_POINT_DEFERRED_STAMP").is_some())
}

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

impl Gen {
    /// `#[derive(Hash)]`'s byte stream, field by field in declaration
    /// order, with a section mark before each field (and each pool
    /// slot) — [`crate::engine::world::World::state_sections`]. The
    /// full destructure makes a new field a compile error here; the
    /// order is pinned against the derive by
    /// `the_sectioned_walk_is_the_derived_hash`.
    pub(crate) fn hash_walk<H: crate::engine::world::HashSink>(&self, h: &mut H) {
        use std::hash::Hash;
        let Gen {
            t,
            assets,
            retile,
            map_entity,
            ent,
            slot_gen,
            free,
            ball_chain,
            wiz_chain,
            proj_chain,
            bldg_chain,
            paint_chain,
            mob_chains,
            mc2_recycle,
            m27_v34_slot,
            mc2_pinned,
            mc1_pinned,
            mc1_guard_reg,
            mc1_balloon_reg,
            human_seat,
            human_tag,
            rand,
            pseudo,
            crt_rand,
            spawn_count,
            player_mail,
            player_damage,
            erupting,
            plume,
            player_knock,
            mc1_buffet_post,
            player_spin,
            player_whirl,
            ww_walk_published,
            seat_view,
            m22_seg_residue,
            player_hurl,
            player_flood_pull,
            player_deflect_debit,
            mc2_debuffs,
            rival_ents,
            castle_reg,
            mc2_life_scale,
            player_aggro,
            rival_wanted,
            player_invisible,
            player_ghost,
            player_vel,
            mc2_flyers_clear,
            skeletons_convert_traders,
            human_roll_0x20,
            player_rebound_reg,
            player_chain,
            kills,
            shots,
            hits,
            stats,
            player_danger,
            banked_houses,
            rival_banked_houses,
            castle_alert,
            player_alert,
            balloon_alert,
            pal_flash,
            exhausted,
            castle_watchdog_fired,
            chassis,
            verbs,
            verb_fallbacks,
            misfits,
            sounds,
            terrain_dirty,
            mc2_night_shade,
            mc2_spawn_ord,
            mc2_player_drain,
            mc2_rival_leech,
            mc2_scrolls,
            mc2_spell_tokens,
            mc2_cast_xp,
            mc2_ladder_sync,
            mc2_castle_lock_mail,
            mc2_steal_mail,
            bolt_fx,
            volcano_blind,
            blast_hits,
            mc1_aim_latch,
            mc2_beam_defer,
            mc2_pred_axis,
            mc2_aura_claim,
            mc2_wanted,
            mc2_rebound_precise,
            mc2_allied,
            mc2_castle_research,
            mc2_mobilize,
            mc2_slow,
        } = self;
        h.section(|| "g.t".into());
        t.hash(h);
        h.section(|| "g.assets".into());
        assets.hash(h);
        h.section(|| "g.retile".into());
        retile.hash(h);
        h.section(|| "g.map_entity".into());
        map_entity.hash(h);
        // The pool slot by slot (the derive's length prefix, then each
        // element — `Hash for [T]`'s own shape).
        h.section(|| "g.ent".into());
        h.write_usize(ent.len());
        for (i, e) in ent.iter().enumerate() {
            h.section(|| format!("g.ent[{i}]"));
            e.hash(h);
        }
        h.section(|| "g.slot_gen".into());
        slot_gen.hash(h);
        h.section(|| "g.free".into());
        free.hash(h);
        h.section(|| "g.ball_chain".into());
        ball_chain.hash(h);
        h.section(|| "g.wiz_chain".into());
        wiz_chain.hash(h);
        h.section(|| "g.proj_chain".into());
        proj_chain.hash(h);
        h.section(|| "g.bldg_chain".into());
        bldg_chain.hash(h);
        h.section(|| "g.paint_chain".into());
        paint_chain.hash(h);
        h.section(|| "g.mob_chains".into());
        mob_chains.hash(h);
        h.section(|| "g.mc2_recycle".into());
        mc2_recycle.hash(h);
        h.section(|| "g.m27_v34_slot".into());
        m27_v34_slot.hash(h);
        h.section(|| "g.mc2_pinned".into());
        mc2_pinned.hash(h);
        h.section(|| "g.mc1_pinned".into());
        mc1_pinned.hash(h);
        h.section(|| "g.mc1_guard_reg".into());
        mc1_guard_reg.hash(h);
        h.section(|| "g.mc1_balloon_reg".into());
        mc1_balloon_reg.hash(h);
        h.section(|| "g.human_seat".into());
        human_seat.hash(h);
        h.section(|| "g.human_tag".into());
        human_tag.hash(h);
        h.section(|| "g.rand".into());
        rand.hash(h);
        h.section(|| "g.pseudo".into());
        pseudo.hash(h);
        h.section(|| "g.crt_rand".into());
        crt_rand.hash(h);
        h.section(|| "g.spawn_count".into());
        spawn_count.hash(h);
        h.section(|| "g.player_mail".into());
        player_mail.hash(h);
        h.section(|| "g.player_damage".into());
        player_damage.hash(h);
        h.section(|| "g.erupting".into());
        erupting.hash(h);
        h.section(|| "g.plume".into());
        plume.hash(h);
        h.section(|| "g.player_knock".into());
        player_knock.hash(h);
        h.section(|| "g.mc1_buffet_post".into());
        mc1_buffet_post.hash(h);
        h.section(|| "g.player_spin".into());
        player_spin.hash(h);
        h.section(|| "g.player_whirl".into());
        player_whirl.hash(h);
        h.section(|| "g.ww_walk_published".into());
        ww_walk_published.hash(h);
        h.section(|| "g.seat_view".into());
        seat_view.hash(h);
        h.section(|| "g.m22_seg_residue".into());
        m22_seg_residue.hash(h);
        h.section(|| "g.player_hurl".into());
        player_hurl.hash(h);
        h.section(|| "g.player_flood_pull".into());
        player_flood_pull.hash(h);
        h.section(|| "g.player_deflect_debit".into());
        player_deflect_debit.hash(h);
        h.section(|| "g.mc2_debuffs".into());
        mc2_debuffs.hash(h);
        h.section(|| "g.rival_ents".into());
        rival_ents.hash(h);
        h.section(|| "g.castle_reg".into());
        castle_reg.hash(h);
        h.section(|| "g.mc2_life_scale".into());
        mc2_life_scale.hash(h);
        h.section(|| "g.player_aggro".into());
        player_aggro.hash(h);
        h.section(|| "g.rival_wanted".into());
        rival_wanted.hash(h);
        h.section(|| "g.player_invisible".into());
        player_invisible.hash(h);
        h.section(|| "g.player_ghost".into());
        player_ghost.hash(h);
        h.section(|| "g.player_vel".into());
        player_vel.hash(h);
        h.section(|| "g.mc2_flyers_clear".into());
        mc2_flyers_clear.hash(h);
        h.section(|| "g.skeletons_convert_traders".into());
        skeletons_convert_traders.hash(h);
        h.section(|| "g.human_roll_0x20".into());
        human_roll_0x20.hash(h);
        h.section(|| "g.player_rebound_reg".into());
        player_rebound_reg.hash(h);
        h.section(|| "g.player_chain".into());
        player_chain.hash(h);
        h.section(|| "g.kills".into());
        kills.hash(h);
        h.section(|| "g.shots".into());
        shots.hash(h);
        h.section(|| "g.hits".into());
        hits.hash(h);
        h.section(|| "g.stats".into());
        stats.hash(h);
        h.section(|| "g.player_danger".into());
        player_danger.hash(h);
        h.section(|| "g.banked_houses".into());
        banked_houses.hash(h);
        h.section(|| "g.rival_banked_houses".into());
        rival_banked_houses.hash(h);
        h.section(|| "g.castle_alert".into());
        castle_alert.hash(h);
        h.section(|| "g.player_alert".into());
        player_alert.hash(h);
        h.section(|| "g.balloon_alert".into());
        balloon_alert.hash(h);
        h.section(|| "g.pal_flash".into());
        pal_flash.hash(h);
        h.section(|| "g.exhausted".into());
        exhausted.hash(h);
        h.section(|| "g.castle_watchdog_fired".into());
        castle_watchdog_fired.hash(h);
        h.section(|| "g.chassis".into());
        chassis.hash(h);
        h.section(|| "g.verbs".into());
        verbs.hash(h);
        h.section(|| "g.verb_fallbacks".into());
        verb_fallbacks.hash(h);
        h.section(|| "g.misfits".into());
        misfits.hash(h);
        h.section(|| "g.sounds".into());
        sounds.hash(h);
        h.section(|| "g.terrain_dirty".into());
        terrain_dirty.hash(h);
        h.section(|| "g.mc2_night_shade".into());
        mc2_night_shade.hash(h);
        h.section(|| "g.mc2_spawn_ord".into());
        mc2_spawn_ord.hash(h);
        h.section(|| "g.mc2_player_drain".into());
        mc2_player_drain.hash(h);
        h.section(|| "g.mc2_rival_leech".into());
        mc2_rival_leech.hash(h);
        h.section(|| "g.mc2_scrolls".into());
        mc2_scrolls.hash(h);
        h.section(|| "g.mc2_spell_tokens".into());
        mc2_spell_tokens.hash(h);
        h.section(|| "g.mc2_cast_xp".into());
        mc2_cast_xp.hash(h);
        h.section(|| "g.mc2_ladder_sync".into());
        mc2_ladder_sync.hash(h);
        h.section(|| "g.mc2_castle_lock_mail".into());
        mc2_castle_lock_mail.hash(h);
        h.section(|| "g.mc2_steal_mail".into());
        mc2_steal_mail.hash(h);
        h.section(|| "g.bolt_fx".into());
        bolt_fx.hash(h);
        h.section(|| "g.volcano_blind".into());
        volcano_blind.hash(h);
        h.section(|| "g.blast_hits".into());
        blast_hits.hash(h);
        h.section(|| "g.mc1_aim_latch".into());
        mc1_aim_latch.hash(h);
        h.section(|| "g.mc2_beam_defer".into());
        mc2_beam_defer.hash(h);
        h.section(|| "g.mc2_pred_axis".into());
        mc2_pred_axis.hash(h);
        h.section(|| "g.mc2_aura_claim".into());
        mc2_aura_claim.hash(h);
        h.section(|| "g.mc2_wanted".into());
        mc2_wanted.hash(h);
        h.section(|| "g.mc2_rebound_precise".into());
        mc2_rebound_precise.hash(h);
        h.section(|| "g.mc2_allied".into());
        mc2_allied.hash(h);
        h.section(|| "g.mc2_castle_research".into());
        mc2_castle_research.hash(h);
        h.section(|| "g.mc2_mobilize".into());
        mc2_mobilize.hash(h);
        h.section(|| "g.mc2_slow".into());
        mc2_slow.hash(h);
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
    /// ⭐ RETAIL'S SECOND CLASS-5 WORD — `word_0x2E_46`, the CHARM /
    /// SUMMON LEASE — which the port used to home in `f26` alongside
    /// `dword_0x10_16` (@0x10). Retail keeps them in two places and
    /// **both are live at once** on a charmed creature: `sub_3A650`
    /// (EF:29682-90) stamps `word_0x2E_46 = subSpellIndex` on a victim
    /// whose `actionIndex & 7 == 2` — i.e. mid-attack, in a state
    /// whose handlers read @0x10 (`sub_24510`'s 15-bolt burst counter
    /// EF:15469-72, `HitFirebug_25610`'s hover altitude EF:16415/16457,
    /// `sub_250B0`'s dive index) — and leaves the action alone. One
    /// word cannot serve both.
    ///
    /// Tenants of @0x2E on class 5 are an ENUMERATED LIST, and it is
    /// `sub_1D5D0`'s own switch (EF:9977-10025, shipped `NETHERW.EXE`
    /// jump table at file 0x41DEE): **StageVar2 13 and 16** share
    /// `sub_1E580` (EXE arm 0x42D80), **14** is `sub_1E9C0` (0x431C0),
    /// and **17** merely CARRIES the lease the pyramid's release wrote
    /// (`sub_1E320`, 0x42B20, never touches it) until the state flips
    /// to 16. ⛔ **StageVar2 12 is NOT a tenant**: its arm `sub_1E4D0`
    /// (EXE 0x42CD0-0x42D77) is twelve instructions of position copy
    /// plus `mov %dx,0x20(%esi)` / `mov %dx,0x1c(%esi)` — **no store
    /// to +0x2E and none to +0x10 either**, so a metamorph puppet
    /// keeps its ctor's `slot % 100` in @0x10 for its whole life.
    ///
    /// ⚠⚠⚠ HASH-SILENT BY CONSTRUCTION. `Ent` is `#[derive(Hash)]`
    /// and `World::state_hash` hashes `g` wholesale, so a plain `i16`
    /// here would move every golden with zero behaviour change (round
    /// 97 lost real time to exactly that). [`Lease2e`]'s empty `Hash`
    /// impl is the opt-out — the same pattern as [`CrtRand`] and
    /// [`Mc2Pinned`]. Verified, not asserted: the golden suites are
    /// byte-identical with the law on.
    pub(crate) lease2e: Lease2e,
    /// THE METAMORPH PUPPET'S CRY CADENCE — the port's own 24-tick
    /// sound loop for StageVar2 12, which has no retail counterpart
    /// at all. It lived in [`Ent::lease2e`] (retail's `word_0x2E_46`)
    /// until round 148 measured it as a permanent `retail 0 / port
    /// 24` across eight class-5 models. See [`no_mc2_morph_cry_off_2e`].
    /// Hash-silent and off the snapshot wire — see [`MorphCry`].
    pub(crate) morph_cry: MorphCry,
    /// ⭐ RETAIL'S `+48`, CARRIED RAW FOR **EVERY** CLASS — the lane
    /// [`Ent::f26`] only homes while the record is a class-12
    /// manifestation (`import_ent`: `if class64 == 12 { r.f48 }`).
    ///
    /// `sub_14E60` (VA 0x14E60, CARPET.EXE file 0x2D658) is
    /// `pool + 164 * (i16)wizext->owned[spell]` with **no class,
    /// liveness or flag guard at all** — three `lea`s, an `add` and a
    /// `jbe` against the pool base, then `ret`. So every reader of a
    /// wizard's owned-token register dereferences whatever now
    /// occupies that slot, and `sub_14120`'s `if (*(_WORD *)(v2 + 48))
    /// return 0;` (file 0x2C971 `cmpw $0x0,0x30(%eax)`) reads that
    /// stranger's `+48`. When the register is stale — the token was
    /// scattered and its slot recycled into a projectile or a creature
    /// — the port read the stranger's `+26` through `f26` instead and
    /// the upgrade arm answered the wrong question.
    ///
    /// ⚠⚠⚠ HASH-SILENT BY CONSTRUCTION, exactly like [`Lease2e`]:
    /// `Ent` is `#[derive(Hash)]` and `World::state_hash` hashes `g`
    /// wholesale, so a plain `u16` here would move every golden with
    /// zero behaviour change.
    pub(crate) raw48: Raw48,
    /// ⭐ THE CASTLE WORKERS' `+42` — THE CASTLE'S OWN SLOT (round 154,
    /// w154j). Retail mints the (10,42) painter and the (10,41) leveler
    /// with `+71 = castle +26`, `+24 = castle +24` and **`+42 = the
    /// castle's slot`** (`sub_47020` :56104-11, `sub_47080` :56124-33,
    /// the upgrade commit :56484-91), and the workers resolve their
    /// castle through that link — the painter's shake suspend
    /// (`sub_285C0` :30520 `castle[+42]->+50`) and finish (:30697-709
    /// `+59 = 5`), the leveler's shake gate (`sub_28200` :30333) and
    /// finish (:30419-27 `+59 = 2`, `+154 = 32·current`). The port
    /// re-derived the castle by SITE (`castle_at_site`, the lowest
    /// live castle at the worker's `(x, y)`), which agrees with the
    /// link wherever exactly one live castle stands on the site and
    /// serves the wrong record where two do (a razed-and-rebuilt site
    /// under a different slot, a TRANSFORM-parked castle). Stamped at
    /// the three mints, lifted by the conformance importer from the
    /// recorded `+42`, consumed by [`Gen::castle_of_worker`]. Zero =
    /// unlinked (a pre-field save; the resolve falls back to the scan).
    /// Hash-silent and off the wire like [`Raw48`].
    pub(crate) link42: Link42,
    /// ⭐ THE SUMMIT CONTROLLERS' 32-BIT `dword_0x10_16`. The (10,18)
    /// eruption vortex and its (10,91) apocalypse-rain sibling are the
    /// only two records in MC2 whose @0x10 arc counter is allowed to
    /// run past an `i16`: nothing ever resets it (the self-latched
    /// controller's restart roll is gated on the vortex register it
    /// holds itself), so it just keeps counting until the endgame
    /// teardown — **79,064** on mc2l24-crazy. [`Ent::f26`], the port's
    /// class-wide home for @0x10, is an `i16` and parked at 32,767.
    /// See [`no_mc2_summit_arc_wide`] for the citation and the
    /// inertness proof. Hash-silent and off the snapshot wire for the
    /// same reasons [`Raw48`] is.
    pub(crate) summit10: Summit10,
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
    ///
    /// ⚠ RETAIL'S FIELD IS AN `int8_t` (MC1 Basic.h:394, MC2
    /// global_types.h:351) and both allocators mint the never-woken
    /// sentinel as `-6` (:43880/:43905, Events.cpp:576/602) — the
    /// SAME byte as this widened field's 0xFA. The port's canonical
    /// form is the UNSIGNED byte, so importers narrow the recorder's
    /// `i8` through `as u8` and every predicate is retail's own
    /// truthiness on the byte (`if (byte_0x39_57)`), never a sign
    /// test. Two representations of one byte is what made the MC2 aim
    /// scan's gate read opposite ways for imported and native records.
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
#[derive(Default, Clone)]
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
    /// The seat whose screen flashes; `None` = every seat. Retail's
    /// owner compare (sub_44BE0) shows a caster's flash on HIS machine
    /// only (audit 2026-10-17: one human's Global Death washed every
    /// peer's view violet).
    pub(crate) seat: Option<Hx>,
}

impl PalFlash {
    /// Arm `row` for the app overlay. Later arms overwrite earlier
    /// ones — retail's +152 is a single byte, last writer wins.
    pub(crate) fn arm(&mut self, row: u8) {
        self.row = row;
        self.ticks = Self::LIFE;
        self.seat = None;
    }

    /// [`Self::arm`] for seat `hx`'s screen alone.
    pub(crate) fn arm_seat(&mut self, row: u8, hx: Hx) {
        self.arm(row);
        self.seat = Some(hx);
    }

    /// The (row, ticks) seat `hx` sees.
    pub(crate) fn seen_by(&self, hx: Hx) -> (u8, u8) {
        if self.seat.is_none_or(|s| s == hx) {
            (self.row, self.ticks)
        } else {
            (0, 0)
        }
    }

    /// The overlay fade length. Retail paints one frame and fades back
    /// over the case-1 4-step ramp; 6 sim ticks reads the same at our
    /// tick rate.
    pub(crate) const LIFE: u8 = 6;
}

impl std::hash::Hash for PalFlash {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// The ESTABLISHED-castle register array (see [`Gen::castle_reg`]).
/// Hash-SILENT like [`PalFlash`]: every retail-visible consequence
/// of the bind (the ladder stamp's flags bit 1, the AI's cast and
/// state choices, the graded `rival.castle` conformance lane) is
/// hashed through entity lanes already, and hashing the register
/// itself would have moved every pinned state golden the day the
/// field landed.
#[derive(Default, Clone)]
pub(crate) struct CastleReg(pub(crate) [u16; 8]);

impl std::hash::Hash for CastleReg {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

impl std::ops::Index<usize> for CastleReg {
    type Output = u16;
    fn index(&self, i: usize) -> &u16 {
        &self.0[i]
    }
}

impl std::ops::IndexMut<usize> for CastleReg {
    fn index_mut(&mut self, i: usize) -> &mut u16 {
        &mut self.0[i]
    }
}

/// The rivals' claimed-house tallies (see [`Gen::rival_banked_houses`]).
/// Hash-SILENT like [`CastleReg`]: a tick-top rebuild consumed within
/// the same tick, whose every consequence rides hashed entity lanes.
#[derive(Default, Clone)]
pub(crate) struct RivalHouses(pub(crate) [i32; 8]);

impl std::hash::Hash for RivalHouses {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

impl std::ops::Index<usize> for RivalHouses {
    type Output = i32;
    fn index(&self, i: usize) -> &i32 {
        &self.0[i]
    }
}

impl std::ops::IndexMut<usize> for RivalHouses {
    fn index_mut(&mut self, i: usize) -> &mut i32 {
        &mut self.0[i]
    }
}

/// `MGC_NO_MC1_OWNER_HOUSE_TALLY=1` restores the pre-dig (round 154,
/// w154i) castle-side house reads: the overflow ejector
/// [`Gen::castle_eject`] read the HUMAN's [`Gen::banked_houses`] for
/// EVERY castle, and the fleet dispatcher [`Gen::castle_balloons`]
/// summed the owner's (10,45) records LIVE at the castle's own slot.
///
/// Retail's two readers both resolve THE CASTLE'S OWNER and read the
/// tick-top census snapshot: `sub_47130` (:56185, `CARPET.EXE` VA
/// 0x47133 `movswl 0x18(%ebx)` → ×164 → `0x7503(%edx,%eax,4)` = the
/// owner's wizext → VA 0x47164 `mov 0x134(%edx),%ecx` = +308, `add
/// 0x8c`, `cmp 0x88`, `jle` ⇒ `houses + stored > cap` ⇒ spill =
/// stored − cap) and `sub_47400` (:56363, VA 0x4740B `movswl
/// 0x18(%edx)` → `+0x7463` → its +160, then VA 0x47571 `mov
/// 0x134(%edx),%eax; add; cmp 0x88(castle); jge` ⇒ `houses + stored >=
/// cap` ⇒ every balloon homes). `+308` itself is written ONLY by
/// the census `sub_48230` (:56863 zero per wizard, :56895 accumulate
/// each (10,45)'s +140 through `sub_48340`'s +144 owner resolve),
/// which runs at the tick top (:52327) before every entity tick.
///
/// Both pre-dig reads were measured INERT on the corpus (39 takes): the
/// ejector's house term cannot change its decision for a non-negative
/// tally (spill = stored − cap needs stored > cap, which alone
/// satisfies the test), and the live sum parts from the snapshot on ONE
/// boundary — mc1hwl2 t=13604, house 6 reclaimed by the human at a
/// slot below castle 943 (snapshot 2048 + 8753 >= 10000 says full, the
/// live sum 0 says hunt) — where rival 429's balloon 997 had no ball to
/// hunt and stayed home either way. The law is record-fidelity: one
/// tally per owner, one reader shape, and the rival `banked_houses`
/// wizard-shadow lane it makes comparable (0 rows on mc1l5 / mc1hwl2).
pub(crate) fn no_mc1_owner_house_tally() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_OWNER_HOUSE_TALLY").is_some())
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
#[derive(Hash, Clone)]
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
    /// Tick-start CLASS-3 roster — bucket[0], `var_u32_36462[0]`
    /// (see [`TickChain`]; the case-3 arm of the same sweep, :52253).
    pub(crate) wiz_chain: TickChain,
    /// Tick-start CLASS-9 roster — `var_u32_36462[3]` (+36474), the
    /// case-9 arm of the same sweep (:52279). Membership is EVERY
    /// class-9 record, no life or flags test — a projectile born
    /// MID-tick is invisible to this tick's walkers, and a soft-killed
    /// one stays a member until the next rebuild. Its measured walker
    /// is the rival incoming-projectile defense (`sub_16800` :19777).
    pub(crate) proj_chain: TickChain,
    /// Tick-start MC2 BUILDING roster — `dword_38527`, the
    /// `model <= 0x2D` arm of the MC2 sweep (EF:40043-52; see
    /// [`TickChain`]). The ch0 broadcast's building pass walks it
    /// (`sub_10C80` EF:4076), so a building that only BECOMES one
    /// mid-tick — the action-51 → 52 construction completion — takes
    /// no ch0 mail until the next tick top.
    pub(crate) bldg_chain: TickChain,
    /// Tick-start MC2 TERRAIN-PAINTER roster — `dword_38535`, the
    /// `v4x` arm of the same MC2 sweep: class 10 models 42 / 67 / 78
    /// and class 11 models 12 / 31 (EF:40033-40085; see [`TickChain`]).
    /// `IsNextEvent0A_2A_37740` (EF:27437) walks it looking for a
    /// (10,42) build painter whose box overlaps, so a painter minted
    /// MID-tick cannot freeze anything until the next tick top —
    /// which is the whole reason a completing build's own successor
    /// painter does not stall the buildings dispatching after it in
    /// the same frame (galore t=6523).
    pub(crate) paint_chain: TickChain,
    /// Tick-start per-model class-5 roster chains (see [`MobChains`]).
    pub(crate) mob_chains: MobChains,
    /// MC2's recycle-victim stack — the allocator's FALLBACK once
    /// `free` is dry (see [`Mc2Recycle`]). Empty on MC1, whose
    /// allocator has the opposite priority.
    pub(crate) mc2_recycle: Mc2Recycle,
    /// `sub_2AF10`'s `[ebp-0xc]` — the ONE writer of `sub_29A90`'s
    /// `v34` stack slot that is not residue from an earlier entity's
    /// handler (dig 126O; see [`Gen::m27_v34_residue_law`] for the
    /// frame arithmetic that makes the two locals the same address).
    /// `Some` only between the m27 mover's turn-search and the branch
    /// machine that reads it, so it is a WITHIN-TICK scratch, never a
    /// save-boundary datum.
    pub(crate) m27_v34_slot: M27V34Slot,
    /// The conformance import's human-carpet slot — the `pinned`
    /// argument every [`Gen::mc2_rebuild_free`] call site already
    /// carries down from `World::mc2_carpet_slot`, mirrored here for
    /// the rebuild sites that live BELOW the World layer (the castle
    /// ejector's mid-tick GC, [`Gen::mc2_eject_gc`]). 0 = native
    /// play, where the human owns no pool slot at all. Save-silent
    /// and hash-silent: `World::new` and the strict import both seed
    /// it, exactly like `mc2_carpet_slot` itself.
    ///
    /// ⚠⚠ HASH-SILENCE IS NOT AUTOMATIC — see the `derive(Hash)` trap
    /// on [`Gen::mc2_mobilize`]. A PLAIN `u16` here moved FIVE goldens
    /// in BOTH games the moment it landed (`state_hash`'s
    /// `level_005`, `sim_state_hash`'s flight tier, `mc2_slice` and
    /// two `mc2_spell_channels` rows) with no behaviour change at all,
    /// because `state_hash` hashes `g` wholesale and `Gen` is
    /// `#[derive(Hash)]`. [`Mc2Pinned`]'s empty `Hash` impl is the
    /// opt-out, exactly as [`CrtRand`] is for the CRT stream.
    pub(crate) mc2_pinned: Seats<Mc2Pinned>,
    /// The MC1 twin of [`Gen::mc2_pinned`]: `World::mc1_carpet_slot`
    /// mirrored below the World layer for the bucket[0] readers that
    /// seat the out-of-pool human ([`Gen::mc1_human_on_wiz_chain`]).
    /// 0 = native play (no pooled carpet). Save-silent and
    /// hash-silent like the MC2 mirror.
    pub(crate) mc1_pinned: Seats<Mc1Pinned>,
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
    /// The wizard seat (player colour, retail's wizext `+48`) the
    /// human flies — retail's LOCAL index (MC1 `var_u16_8`, MC2
    /// `LevelIndex_0xc`): 0 in every retail single-player game, the
    /// NetBIOS node index in a network one. Retail's `sub_3DD50` /
    /// `sub_53160` make every OTHER seat an AI in single-player, so a
    /// nonzero seat puts a rival in seat 0. The out-of-pool human
    /// keeps its [`crate::mc1::mobs::PLAYER_TARGET`] owner tag; the
    /// tag resolves to THIS seat. Set once at level build
    /// ([`crate::engine::world::World::set_human_seat`]).
    pub(crate) human_seat: Seats<HumanSeat>,
    /// The human's OWNER TAG — the word retail stores in a record's
    /// owner/target fields (`+24`, `+144`, `+146`, mail sources, …)
    /// for the human's things. Every sim site that names "the human"
    /// by tag reads this field. Load-static (set where the human is
    /// seated), so it hashes and saves nothing.
    pub(crate) human_tag: Seats<HumanTag>,
    /// Global LCG (`rand_4`), = the level seed at scan time.
    pub(crate) rand: u32,
    /// Terrain-retile LCG (`pseudoRand`), u16 stream.
    pub(crate) pseudo: u16,
    /// The Watcom CRT `rand()` stream (`_RWD_randnext`, seed 1;
    /// x·1103515245+12345, returns `(x>>16) & 0x7FFF`). Retail draws
    /// it at exactly ONE sim site — the rival anti-rebound plan roll
    /// (`rand() % 255 < acc`, :19507-08). It is a GLOBAL CRT stream
    /// with no capture channel, so its phase is unrecoverable at
    /// import — the roll's outcome is best-effort — but drawing it
    /// here instead of the wizard's own entity LCG keeps the graded
    /// `rand` lane honest (mc1l49 t=2788: one stolen `ent_rand`
    /// step shattered every later rand comparison).
    pub(crate) crt_rand: CrtRand,
    /// Per-model spawn ordinals (`str_AE400+12+model`, Type_AE400_20
    /// str_12): creature spawns record the old value into +63 and
    /// increment; model-7 sprite alternation keys off its parity.
    pub(crate) spawn_count: [u8; 20],
    /// The human player's damage inbox WHEN HE HAS NO RECORD (a bare
    /// world, or `MGC_NO_HUMAN_MAIL_IN_RECORD=1`) — otherwise his
    /// letters live in his carpet record's own `mail`. Always reach it
    /// through [`Gen::pmail`] / [`Gen::pmail_mut`]. The
    /// invincible-player dev mode discards it every tick like the
    /// original's spawn grace (:55367-71), accumulating the totals.
    pub(crate) player_mail: Seats<[(u32, u16); 6]>,
    /// Total ch0 damage the (invincible) player has absorbed.
    pub(crate) player_damage: Seats<u64>,
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
    pub(crate) player_knock: Seats<(u16, i16)>,
    /// The kraken buffet's write on a POOL wizard (w154j): `sub_1C4F0`
    /// :23223-28 stores the TARGET record's wizext `+24 = bearing +
    /// 0x400`, `+26 = 256`, `+22 = 80` through `+146`'s `+160` pointer
    /// for ANY target (CARPET.EXE 0x1C6EE `mov 0xa0(%edi),%edx` — no
    /// class test; 0x1C6F7/0x1C701/0x1C719 the three stores). The
    /// human's arm lands on [`Gen::player_knock`] directly; a RIVAL's
    /// wizext lives on `World::rivals`, so the arm posts `(target ent,
    /// bearing)` here and the walk drains it right after the kraken's
    /// own dispatch (`World::tick_arm_creature`) — retail's phase. Always
    /// `(0, 0)` between dispatches: not on the wire, hash-silent.
    pub(crate) mc1_buffet_post: HashSilent<(u16, u16)>,
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
    pub(crate) player_spin: Seats<PlayerSpin>,
    /// The whirlwind's DIRECT POSE SEIZURE on the human wizard — the
    /// half of `sub_33340`'s victim body that is NOT a knock. Retail
    /// treats the human as an ordinary victim `ix`: the mid-ring arm
    /// slams an ABSOLUTE heading into `word_0x30_48` AND
    /// `yaw_0x1C_28` (EF:24350-56), writes `actSpeed_0x82_130 = 80`
    /// and kicks `roll_0x155_341 += 28` (EF:24345-49), then displaces
    /// the victim by `MoveEntity_57FA0(&pred, word_0x30_48, 0, v30)` +
    /// `CopyEntityPosition_57CF0` (EF:24380/24395) — a POSITION
    /// WRITE. **Nothing in `sub_33340` ever touches
    /// `moveBoost_0x1E_30`**, retail's knock register, so carrying
    /// the funnel on [`Gen::player_knock`] was an invented write on a
    /// recorded, graded, decaying lane retail holds at 0. Same
    /// transport shape as the spin (world writes, the carpet's own
    /// walk slot drains it once, no decay).
    pub(crate) player_whirl: Seats<PlayerWhirl>,
    /// ⭐ A WITHIN-WALK REGISTER, the [`Gen::m27_v34_slot`] idiom:
    /// has the whirlwind column published into [`Gen::player_whirl`]
    /// ALREADY, during THIS walk? `World::tick_inner` clears it before
    /// the first handler runs, so it is true exactly when the mailbox
    /// still holds a publication made by a LOWER funnel slot of the
    /// same tick — which is what makes the crank counts additive
    /// (`mc2::tail::mc2_ww_crank_accum_law`) without ever adding to a
    /// publication that leaked across a tick boundary (a funnel that
    /// is the HIGHEST live slot has no later walker to drain it, so
    /// the mailbox can and does survive the turn).
    /// Not on the wire, not hashed — a transient of the walk.
    pub(crate) ww_walk_published: Seats<HashSilent<bool>>,
    /// Each seat's WALK VIEW ([`SeatView`]): the tick-top copy of his
    /// pose, heading, purse and life sign that the walk publishes and
    /// re-publishes at its phase points. Seeded at the tick top; not
    /// on the wire, not hashed — a transient of the walk.
    pub(crate) seat_view: Seats<HashSilent<SeatView>>,
    /// ⭐ A WITHIN-WALK REGISTER, the [`Gen::m27_v34_slot`] idiom: the
    /// DWORD an m22 tail segment's handler (`sub_26CA0`, action 0xB4)
    /// leaves at `W-52` below the entity walk's call site, tagged
    /// with that segment's slot. The next worm head's `sub_26FF0`
    /// reads it as its uninitialised probe position — see
    /// [`no_mc2_m22_stale_probe`]. `(slot, None)` = the handler ran
    /// but its residue is not modelled. Cleared at walk start; not on
    /// the wire, not hashed.
    pub(crate) m22_seg_residue: HashSilent<Option<(u16, Option<u32>)>>,
    /// The doomsday pyramid's HURL-AWAY BEAM on the human — the
    /// second `sub_21AB0` arm that is NOT a knock. Case 7 (EF:13427-56)
    /// ramps the GLOBAL `D41A0_0.word_0x36546` 1024 → −80/tick → floor
    /// 10 and spends it as `MoveEntity_57FA0(&pred, tan2(pyramid,
    /// player), 0, ramp)` + `moveTest_5D0A0` + a ground+clearance
    /// clamp + `CopyEntityPosition_57CF0` — a POSITION WRITE, at the
    /// PYRAMID's own pool slot. **Nothing in case 7 touches
    /// `moveBoost_0x1E_30`** (shipped `NETHERW.EXE` 0x465cd-0x466e1),
    /// so carrying it on [`Gen::player_knock`] was an invented write
    /// on a lane retail holds at 0 — and one the knock channel cannot
    /// serve anyway: `World::take_knock_step` CLAMPS TO ±128 and
    /// decays −4/tick, so retail's 944-unit opening shove landed as
    /// 128 and then kept shoving for twenty ticks after the burst.
    /// Same transport shape as [`Gen::player_whirl`].
    pub(crate) player_hurl: Seats<PlayerHurl>,
    /// ⭐⭐ THE (10,67) QUAKE/FLOOD'S **Z PULL-DOWN** ON THE HUMAN —
    /// the half of `sub_39B60`'s shove that is NOT a knock, and the
    /// last un-ported line of `mc2::flood`'s module-header APPROX
    /// list ("the z pull and spin bank on the FlightVerb takeover
    /// seam"). Retail's shove is a `CopyEntityPosition_57CF0` on the
    /// victim RECORD, and its wizard arm (class 3 model 0 —
    /// `NETHERW.EXE` 0x5e4ef `mov 0x3f(%ebx),%ah` / `cmp $0x3` /
    /// 0x5e4fa `cmpb $0x0,0x40(%ebx)`) is `predicted.z -= 48 *
    /// ((4096 - v5) << 8 >> 12) >> 8`, floored at the stepped point's
    /// `getTerrainAlt` (0x5e4e7 / 0x5e585). The POOL victims have had
    /// it since round 98; the human arm carried only the horizontal
    /// leg on [`Gen::player_knock`], so a wizard caught in a Gravity
    /// Well was dragged sideways at retail's exact rate and never
    /// sank. Same transport shape as [`Gen::player_hurl`]: the world
    /// arms it at the quake's own walk slot, the carpet's dispatch
    /// drains it once, no decay.
    pub(crate) player_flood_pull: Seats<PlayerFloodPull>,
    /// The mana quarters the human's REBOUND deflections owe this
    /// tick — see [`DeflectDebit`]. Written by the projectile
    /// walkers' deflect arms, drained by the MC1 wizard pass
    /// (pre-step: a debit from a walk slot BELOW the carpet lands on
    /// this tick's step, retail's order) with the tick tail catching
    /// the post-carpet slots (retail lands those raw after the step).
    pub(crate) player_deflect_debit: Seats<DeflectDebit>,
    /// Pending MC2 debuff-stamp hits on the player — (10,65) slow
    /// web / (10,66) paralyze web (`sub_38E70`/`sub_38F70`
    /// EF:28407/28442) — drained into the flight `Mc2Ext` channels
    /// by the sim boundary each tick (docs/traces/mc2-flight-model.md
    /// §5c/5d). Hash-only-when-pending (the Planes pattern): the zero
    /// state contributes nothing.
    pub(crate) mc2_debuffs: Seats<Mc2PlayerDebuffs>,
    /// Rival wizard entity by player slot (0 = none; slot 0 = the
    /// human, unused) — the sprite-family team resolver for owner
    /// recolors (mana balls 105+8·team, balloons 169+team, castle
    /// flags 177+team). Maintained by the rival spawn/respawn path;
    /// claims of an eliminated wizard keep their color (property
    /// persists).
    pub(crate) rival_ents: [u16; 8],
    /// The ESTABLISHED-castle register (retail wizext+50), one per
    /// player slot: written by the level-up commit (:56484) and the
    /// rival's direct plant (:19206), cleared by the teardown to
    /// level 0 (:56534) — nothing else touches it. The AI cascade's
    /// castle predicates and the graded `rival.castle` lane read it:
    /// a freshly PLANTED level-0 castle is bound while an authored
    /// level-0 flag is not (mc1l0 t=562), which no pool scan can
    /// tell apart (mc1l5 t=14771: Vodor's post-raze plant at slot
    /// 478 is bound the tick it lands, mid-transform at level 0).
    /// Save-silent like the tick chains: every retail-visible
    /// consequence (the ladder stamp's flags bit 1, the AI's
    /// choices) is hashed through entity lanes; conformance imports
    /// seed it from the recorded wizext each pair.
    pub(crate) castle_reg: CastleReg,
    /// Per-color MC2 Life scalar for the castle-HP ladder (see
    /// [`Mc2LifeScale`]); written by the MC2 rival spawn.
    pub(crate) mc2_life_scale: Mc2LifeScale,
    /// The human player's village-aggro timer (the wizard struct's
    /// +528): set to 200 by offenses against village property or
    /// population (building hits, villager-family hits and kills),
    /// decremented once per world tick (:55405-06). m4 militia only
    /// hunt a wizard whose timer is live — the hostility gate.
    pub(crate) player_aggro: Seats<i16>,
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
    pub(crate) player_invisible: Seats<bool>,
    /// Ghost mode (`World::ghost`), republished per tick beside the
    /// cloak mirror: the gate for the scans retail lets see THROUGH
    /// a cloak (castle turrets, the m27 branch scan). Per-tick echo
    /// of World state — not on the wire, not hashed.
    pub(crate) player_ghost: HashSilent<bool>,
    /// The out-of-pool human's displacement over the last tick,
    /// (dx, dy) in engine units — his TRUE velocity, whatever the
    /// control model (the enhanced carpet can slide sideways of its
    /// facing). Republished at the tick head; read only by the
    /// `mc2_leviathan_true_aim` patch arm ([`Gen::m28_lead`]). Per-tick
    /// echo of World state — not on the wire, not hashed.
    pub(crate) player_vel: Seats<HashSilent<(i16, i16)>>,
    /// The `mc2_flyers_clear_terrain` patch arm, republished per tick
    /// from `World::patches` (off under `strict_retail`) — the MC2
    /// move core has no ctx to read the set from. Config echo: not on
    /// the wire, not hashed.
    pub(crate) mc2_flyers_clear: HashSilent<bool>,
    /// The `skeletons_convert_traders` patch arm, republished per tick
    /// from `World::patches` (off under `strict_retail`) — the convert
    /// tails ([`Gen::m9_convert`], [`Gen::m9_consume_scan`]) have no
    /// ctx to read the set from. Config echo: not on the wire, not
    /// hashed.
    pub(crate) skeletons_convert_traders: HashSilent<bool>,
    /// ⭐ THE OUT-OF-POOL HUMAN'S `roll_0x20_32` — retail's TARGET-YAW
    /// word on the human carpet record, which the port has nowhere
    /// else to put (the conformance carpet is out of pool and
    /// [`crate::engine::world::PlayerPose`] carries the live facing
    /// only). It is NOT the flight model's `roll_f`/`roll_acc`: on a
    /// WIZARD record @0x20 is the TARGET-YAW channel and its ONLY
    /// writer is the DEATH-SPIN arm (`action45 == 3`, `life < 0`),
    /// which drives @0x20/@0x22 as a look-at (bearing, pitch) pair
    /// while the spin adds a flat +22 to `yaw` every tick. So a live
    /// human's @0x20 is the RESIDUE of his last death, or 0 if he has
    /// never died, and it sits frozen for tens of thousands of ticks.
    /// Measured on the human carpet of `mc2l24` (slot 116): 0 from
    /// t=1 to t=2681; live for the 42 ticks t=2682..2723, then frozen
    /// at 1135; next moved at t=3524 (`action45` 3, life −3040), then
    /// at t=28512 (`action45` 3, life −1370) — EVERY transition in the
    /// take is a death tick, and the value never tracks `yaw` (which
    /// walks 148, 170, 192 ... past a roll of 381, 383, 386).
    /// ⚠⚠ NO SYMBOL IS CITED FOR THAT WRITER: the +22 matches
    /// `sub_33B20` (EF:24678-24691) but that leg also does
    /// `pitch += 16` while the recorded pitch is a flat 0 through the
    /// spin, so it is a near-identical sibling and is left un-named
    /// rather than mis-cited. The READ side IS byte-cited, at the
    /// copy in [`Gen::mc2_metamorph_creature_tick`].
    /// Its ONE consumer is `sub_1E4D0`'s metamorph-puppet copy (see
    /// [`crate::engine::features::no_mc2_morph_puppet_roll`] and
    /// [`no_mc2_human_puppet_roll`]), which is why a value nothing in
    /// the port writes is still observable in a graded lane.
    /// Storage of record (no per-tick republish): seeded outright by
    /// [`crate::engine::world::World::retail_import_mc2`] from the
    /// recorded carpet's @0x20 at every anchor, 0 on a fresh level
    /// like retail's ctor. Hash- and save-silent: it is a shim for a
    /// record the port does not keep.
    /// ⭐ STAGE 3: since the human's (3,0) carpet record exists, ITS
    /// `f32` (MC2 `@0x20`) is the home — read/write through
    /// [`Gen::human_roll`] / [`Gen::set_human_roll`]; this register is
    /// written through, the record-less fallback
    /// ([`no_human_roll_on_record`]).
    pub(crate) human_roll_0x20: Seats<HashSilent<u16>>,
    /// The player's Rebound deflection bit (spell 14; +17 0x80,
    /// :65774) — incoming class-9 projectiles bounce back. The
    /// written-through fallback: once his (3,0) record exists its
    /// `flags & 0x8000` is the home ([`Gen::player_rebound`]).
    pub(crate) player_rebound_reg: Seats<bool>,
    /// The out-of-pool human's seat in his own tile chain — see
    /// [`PlayerChain`].
    pub(crate) player_chain: Seats<PlayerChain>,
    /// Player stat counters: creatures killed (`Type_160+359`), shots
    /// resolved (+343), shots that struck the aimed target (+347).
    pub(crate) kills: Seats<u32>,
    pub(crate) shots: Seats<u32>,
    pub(crate) hits: Seats<u32>,
    /// The end-of-level stats ledger ([`super::stats`]) — retail's
    /// censuses plus the enhanced every-death tally. Bookkeeping only:
    /// no handler reads it, so it hashes to nothing, and the World
    /// stream carries it (snapshot v26).
    pub(crate) stats: HashSilent<super::stats::StatsLedger>,
    /// The wizard's danger-music countdown (Type_160 v_46): armed to
    /// 100 by processed hits (sub_46540's blocks call sub_46520) and
    /// by a projectile acquiring the player as target (:64013); the
    /// carpet MOVER `sub_455D0` decrements it at the carpet's walk
    /// slot and switches the music mode on v_46 > 0 (:55282-92 →
    /// sub_20D00) — flight (state 0) and the fall (state 2) only, the
    /// dead-wait holds it (`no_mc1_danger_walk_seat`). MC2's twin
    /// still decrements in the tick tail.
    pub(crate) player_danger: Seats<i16>,
    /// The HUMAN's claimed-house mana tally (wizext u32_308), stashed
    /// by the per-tick census — the castle overflow ejector's trigger
    /// reads houses + stored vs capacity (sub_47130 :56185-89). The
    /// rivals' twins live in [`Gen::rival_banked_houses`]; every
    /// castle-side reader goes through [`Gen::owner_houses`].
    pub(crate) banked_houses: Seats<i32>,
    /// The RIVALS' claimed-house tallies (wizext u32_308, one per
    /// player slot like [`Gen::rival_ents`]; `[0]` is unused — the
    /// human's is [`Gen::banked_houses`]). Rebuilt from zero by every
    /// tick-top census (`sub_48230` :56863 zeroes each wizard's, :56895
    /// accumulates each (10,45)'s +140 into `pool[+144].+160->+308`),
    /// seeded by the conformance import. Hash-SILENT and save-silent on
    /// the [`CastleReg`] precedent: the census runs BEFORE every reader
    /// (the castle tick's ejector and fleet dispatch), so a restored or
    /// imported world never observes a stale value, and every
    /// consequence (ejected ball births, a recalled balloon's +146)
    /// rides hashed entity lanes. See [`no_mc1_owner_house_tally`].
    pub(crate) rival_banked_houses: RivalHouses,
    /// "Castle under attack" HUD flash (Type_160+391 = 4, :56698) —
    /// armed by every processed castle hit, decremented by the HUD
    /// DRAW on every other frame (`World::mc1_alert_cadence`,
    /// `sub_22E50` :27217-20) while the castle panel is drawn.
    pub(crate) castle_alert: Seats<u8>,
    /// "You are being attacked" HUD flash (Type_160+392 = 4,
    /// :55679/:55692/:55723) — armed by every processed player hit /
    /// steal / grip, decremented by the HUD draw on every other frame
    /// (`sub_22E50` :27347-50). The SELF sub-panel's alert.
    pub(crate) player_alert: Seats<u8>,
    /// Balloon-under-attack HUD flash (Type_160+393 = 4, :56826) —
    /// armed by a processed hit on an own balloon. ⚠ DEAD IN RETAIL:
    /// the arm writes through the balloon's own `+160`, the
    /// allocator's static sink, never the owner's player block, so the
    /// real `+393` never leaves 0 (`no_mc1_balloon_alert_sink`). The
    /// balloon sub-panel's alert; decremented by the HUD cadence
    /// (`World::mc1_alert_cadence`) while the castle register is set.
    pub(crate) balloon_alert: Seats<u8>,
    /// The full-screen palette flash armed by `sub_44BE0` — see
    /// [`PalFlash`]. Hash-silent (presentation).
    pub(crate) pal_flash: PalFlash,
    /// Allocations dropped on pool exhaustion (the limit-removing
    /// register's telemetry; the app logs increases). The original
    /// keeps no such count — it is observability, not behavior.
    pub(crate) exhausted: u32,
    /// Ticks on which an MC1 castle sat in a TRANSFORM wait with no
    /// worker at its site (`castle_tick`'s watchdog predicate,
    /// [`crate::patches::WorldPatches::mc1_castle_transform_watchdog`]).
    /// Counted in BOTH arms — the retail arm measures how often normal
    /// play would trip the patch (the corpus answer: never, outside the
    /// mc1l26 seizure) — and hash-silent: telemetry, not behavior.
    pub(crate) castle_watchdog_fired: HashSilent<(u32, u64)>,
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
    pub(crate) mc2_player_drain: Seats<Mc2Quiet<1>>,
    /// m26's mana leech against a RIVAL wizard: the victim's pool
    /// slot, posted by `m26_tick` and consumed by `World` right after
    /// the wraith's own dispatch (`mc2_rival_leech_apply`), so the
    /// debit lands at the wraith's walk slot off the rival record's
    /// live regen register. Always 0 between frames.
    pub(crate) mc2_rival_leech: Mc2Quiet<10>,
    /// Running "scrolls collected" tally for the human. The XP award
    /// is live — `mc2_class14_tick` grants +4 to every owned spell on
    /// pickup (UpdateScroll_59C80 EF:41180-83) — so this counter is now
    /// a hashed tally only, not a deferral bank. Hash-transparent at
    /// zero.
    pub(crate) mc2_scrolls: Seats<Mc2Quiet<2>>,
    /// The human's collected MC2 spell tokens, a bitmask by spell
    /// model 0..25 (retail: `SpellEnabled[model]` on the wizard,
    /// sub_68FF0 EF:55726) — banked for the Phase-4.2 spell system
    /// like the scrolls. Hash-transparent at zero.
    ///
    /// NOT THE OWNERSHIP PREDICATE — do not gate a pickup on it.
    /// Retail's `SpellEnabled[]` is one lane (slot AND ownership); this
    /// mask is only half of the port's split, and it learns about the
    /// level-start seed and in-level pickups ONLY: the central grant
    /// (`World::mc2_adopt_manifestation`) sets just the book, so
    /// campaign-carried and instrument-granted spells never light a
    /// bit, and the death scatter clears every bit while the respawn
    /// re-mint restores none. `World::mc2_book`'s `ent` is the lane
    /// that always tracks ownership — `mc2_spell_token_tick` gates on
    /// that, and `mc2_owned_spell_jars_are_never_re_collected` pins it.
    pub(crate) mc2_spell_tokens: Seats<Mc2Quiet<3>>,
    /// MC2 spell-XP mail (owner id, spell index): projectile impacts
    /// award from inside the pool tick (`sub_6D8B0` call sites,
    /// EF:63189 etc.); the world tick drains it into the wizard's
    /// book the same turn — empty at hash time like a read mailbox
    /// (and hash-transparent when empty).
    pub(crate) mc2_cast_xp: Mc2XpMail,
    /// See [`Mc2LadderMail`].
    pub(crate) mc2_ladder_sync: Mc2LadderMail,
    /// The CREATE-CASTLE LOCK's BALL-SIDE RELEASE MAIL — one castle
    /// OWNER id per entry. Retail's (9,10) castle ball and its (10,43)
    /// delivery call `sub_5F890(x, 0)` from THEIR OWN dispatch (the
    /// first-tick site refusal EF:58923, the payload-spawn failure
    /// EF:58877, the delivery that missed the castle EF:28249), which
    /// zeroes the owner's spell-2 manifestation `word_0x2E_46`. The
    /// books are World-side, so the pool pushes the owner here and
    /// `World::mc2_drain_castle_lock_mail` drains it right after the
    /// pushing slot's dispatch — same slot, same tick. Transient
    /// within one dispatch: never live at a boundary, never saved,
    /// hash-transparent while empty. Entries are `(owner id, tail)`:
    /// `tail` marks the payload-spawn-failure seat, whose `sub_5F890`
    /// argument is the OWNER record and whose `sub_6D880` tail
    /// therefore runs on the wizard (`World::mc2_castle_ball_owner_tier_drain`).
    /// ⚠ Not a bit on the id — the human's id is 0xFFFF.
    pub(crate) mc2_castle_lock_mail: Mc2LockMail,
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
    /// ⭐ THE VOLCANO'S BLIND PLUME SOFT-KILLS, THIS TICK — the slots
    /// `eruption_tick`'s handover stamped `flags |= 0x400` on through
    /// a STALE register, with no class/model/life test (`sub_25EC0`
    /// :28782-93). Recorded so the conformance roster can excuse
    /// EXACTLY those rows and nothing else.
    ///
    /// ⚠⚠ WHY THIS EXISTS AT ALL (round 163): `0x400` is not the
    /// volcano's bit, it is the GENERIC death/reap bit
    /// (`sub_41E80` = `+17 |= 4`). A roster rule that matched the bit
    /// alone was measurably masking a real port defect — mc1hwl21's
    /// lightning-beam self-seizure (w163b) was silently excused by it
    /// until the same bug was caught on three other takes where it
    /// happened to arrive with company. ⭐⭐⭐ **A BIT IS NOT A
    /// MECHANISM.** Registering the mechanism means asking the sim
    /// what it actually did, which is this field.
    ///
    /// Hash-silent and never saved, like [`Gen::bolt_fx`]: it records
    /// writes already made and feeds nothing back.
    pub(crate) volcano_blind: VolcanoBlind,
    /// The live (10,23) lightning hit-blasts whose ONE ch0 burst
    /// struck something (MC1 `sub_262D0` / MC2 `sub_33D80`: the
    /// area write's hit count — MC2 retail even returns it for the
    /// spell-XP report). Retail draws no blast at all (byte[0] bit 0);
    /// this feeds only the ENHANCED-fire burst, which the player wants
    /// on hits only (2026-11-15). The ctor clears the slot, the burst
    /// adds it, death removes it. Hash- and save-silent like
    /// [`Gen::bolt_fx`].
    pub(crate) blast_hits: BlastHits,
    /// THE SHOT-STATS AIM LATCH: the class-9 record's `+146` as it
    /// stood at the handler's DISPATCH ENTRY, stamped by `proj_tick`
    /// and read by `proj_explode`'s `hits` test — every retail flight
    /// handler computes the aimed record's pointer as its first
    /// statement (`sub_52ED0` :62952-54 and its siblings) and hands
    /// that, not the live `+146`, to `sub_526C0` (:62608-11). Live
    /// only inside one `proj_tick`; hash- and save-silent like
    /// [`Gen::bolt_fx`]. `MGC_NO_MC1_HIT_STAT_AIM_LATCH` reverts the
    /// read to the live word.
    pub(crate) mc1_aim_latch: HashSilent<u16>,
    /// ⭐ THE LIGHTNING BEAM'S DEFERRED IMPACT — see
    /// [`Gen::mc2_lightning_beam_tick`]. Retail's beam marches through
    /// `sub_66610` (EF:63583), which walks, tests the blocker and calls
    /// `DisableEntityDrawing04_57F10` and NOTHING ELSE: no damage, no
    /// impact effect. `sub_66750` then lays the whole sprite-216 trail,
    /// and only afterwards does the beam's impact resolve — so the
    /// blast is the LAST record the tick allocates, behind every trail
    /// node. The port reuses the shared flyer core, whose march ends in
    /// `mc2_proj_impact`, so this parks that call until the trail is
    /// down. Live only inside one `mc2_lightning_beam_tick`; hash- and
    /// save-silent like [`Gen::bolt_fx`].
    pub(crate) mc2_beam_defer: BeamDefer,
    /// ⭐⭐⭐ RETAIL'S ONE GLOBAL SCRATCH AXIS, `predictedAxis_EB398ar`
    /// (shipped `NETHERW.EXE` data address **0x1b398**, 866 references
    /// across the engine). Every move core writes the candidate here
    /// and then commits it with `CopyEntityPosition_57CF0(a1x,
    /// &predictedAxis)` — 65 of the 92 calls to that routine pass this
    /// very global — so after any such commit the global HOLDS THE
    /// POSITION THE LAST ENTITY MOVED TO. That matters because one
    /// site READS it without writing it first: the m21 walker's water
    /// contact spawns its (10,5) splash at `&predictedAxis` (file
    /// 0x4AFB9 `68 98 b3 01 00`), i.e. at whatever entity moved most
    /// recently THIS TICK — not at the walker. See
    /// [`Gen::m21_jump`] and `mc2_splash_pred_axis_law`.
    ///
    /// ⚠ APPROXIMATION, and a deliberate one: the port tracks the
    /// global at the COMMIT (`move_relink`, the `CopyEntityPosition`
    /// twin) rather than at each of retail's 101 assignments, so the
    /// ~27 commits that pass a non-global axis, and the writes that
    /// are never committed (a rejected candidate), are not
    /// distinguished. Only the m21 splash reads it, so the blast
    /// radius of any mis-tracking is exactly one effect's spawn
    /// position.
    pub(crate) mc2_pred_axis: Mc2PredAxis,
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
    pub(crate) mc2_rebound_precise: Seats<Mc2Quiet<6>>,
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
    /// THE HUMAN'S PARALYZE LATCH `mobilizeCounter_0x14E_334` — a
    /// per-tick ECHO of the DRIVER-owned flight ext
    /// ([`crate::flight::Mc2Ext::mobilize`], which already models the
    /// arm and the 10-tick `mobilizeCounter2_0x150_336` decay,
    /// EF:59739-43). The carpet is out of pool here, so a creature
    /// handler that reads the counter — `sub_25E40`'s m20 melee-rush
    /// commit is retail's only such reader (EF:16677) — has no
    /// entity to read it off. Re-pushed by every MC2 carpet dispatch
    /// and reseeded by `retail_import_mc2`.
    ///
    /// ⚠⚠ `Gen` is `#[derive(Hash)]`, so a PLAIN field here would add
    /// a byte to EVERY golden in BOTH games — the `field: _` in
    /// `snap_write` is the SAVE opt-out, not the hash one. The
    /// hash opt-out is a manual `Hash` impl, which is what
    /// [`Mc2Quiet`] is: transparent while zero (so every existing
    /// golden stands, the counter being 0 outside a live paralyze)
    /// and contributing deterministically once armed. Save-silent
    /// like `castle_reg` — the driver re-pushes it every tick and
    /// conformance imports reseed it per pair.
    pub(crate) mc2_mobilize: Seats<Mc2Quiet<9>>,
    /// THE HUMAN'S WEB-SLOW LEVEL `moveSpeed_0x14C_332` (0..3) — the
    /// twin of [`Gen::mc2_mobilize`] one field along, and imported,
    /// re-pushed and hash-opted-out the same way for the same reason.
    /// `sub_38E70` (the (10,65) STAGGER stamp, EF:28411-13 / shipped
    /// EXE 0x5d6ae-0x5d6bf) reads it off the victim wizard's
    /// `dword_0xA4_164x` BEFORE it decides anything: the −80 kick, the
    /// `9377x+9439` grunt draw and the sound all sit inside
    /// `if (!moveSpeed)`, and the `++moveSpeed`/palette pair inside
    /// `if (moveSpeed < 3)`. The carpet is out of pool, so the stamp's
    /// pool-slot handler has no entity to read the level off — and the
    /// port's [`Gen::mc2_debuffs`] queue is a per-tick DELTA drained at
    /// the next carpet dispatch, not the live level. This mirror is.
    ///
    /// ⚠ HASH-SILENT OUTRIGHT (not [`Mc2Quiet`]): this is a pure ECHO
    /// of a field the flight ext already carries and already hashes,
    /// recomputed from it at every carpet dispatch and at every
    /// conformance import, so feeding it into `Gen`'s stream would add
    /// no information and would move every MC2 golden recorded while
    /// the human was webbed.
    pub(crate) mc2_slow: Seats<Mc2Echo>,
}

/// See [`Gen::mc2_castle_research`] — hashes to NOTHING while empty
/// (the [`Mc2Ord`] pattern; tag 7 disambiguates adjacent quiet
/// fields). Entries are `(owner, hp_factor[stage-1],
/// part_type[stage-1])` for stages 1..=7 (retail slots 1..7 / 10..16
/// of the 19-byte array — slots 0/8/9/17/18 are never addressed).
#[derive(Default, Clone)]
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
#[derive(Default, Clone)]
pub(crate) struct Mc2XpMail(pub Vec<(u16, u16, i32)>);

impl std::hash::Hash for Mc2XpMail {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if !self.0.is_empty() {
            self.0.hash(state);
        }
    }
}

/// Castle slots whose HP/CAP ladder just stamped (`sub_60780`) —
/// every stamp re-runs SetSpell on the OWNER's castle-spell book
/// token with the active-cast gate SUPPRESSED (word_46 zeroed around
/// the call, EF:61670), so the cached cast cost tracks the castle
/// level BOTH ways mid-transform: the birth/upgrade rung the tick
/// the castle stands, AND the downgrade rung the tick a level falls
/// (mc2l3 t=265: the destroy's downgrade re-reads ladder level 0 →
/// 1000/9 while the cast window is still pinned). The ladder is
/// Gen-side, the book World-side — same-tick mail like
/// [`Gen::mc2_cast_xp`]. Hash-transparent while empty.
/// Bit 15 of an entry marks a push made by the DOWNGRADE
/// (`sub_605E0`): the castle-death token purge drains those alone
/// (round 112, `castle::no_mc2_purge_on_downgrade_only`).
/// `.1` is the PRICE REGISTER AS IT STOOD AT THE STAMP, parallel to
/// `.0`, `0` = "none taken, read the live register at drain time".
/// `sub_60780` re-prices INSIDE `sub_605E0`, BEFORE that function's
/// level-0 arm zeroes `CastleEntityIndex_0x3A_58` — and the zero is
/// UNCONDITIONAL, so a dying ORPHAN unbinds the castle that is still
/// standing. The port drains this mail after the whole castle
/// dispatch, i.e. after the clear, so the death arm records the word
/// it is about to destroy. The UPGRADE needs no snapshot: retail
/// writes the register (EF:61896) before `sub_60810` re-prices, which
/// is exactly what the live read at drain time already sees.
#[derive(Default, Clone)]
pub(crate) struct Mc2LadderMail(pub Vec<u16>, pub Vec<u16>);

impl std::hash::Hash for Mc2LadderMail {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if !self.0.is_empty() {
            self.0.hash(state);
        }
    }
}

/// See [`Gen::mc2_castle_lock_mail`] — hash-transparent while empty,
/// never snapshotted (drained inside the pushing tick).
#[derive(Default, Clone)]
pub(crate) struct Mc2LockMail(pub Vec<(u16, bool)>);

impl std::hash::Hash for Mc2LockMail {
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
#[derive(Default, Clone)]
pub(crate) struct BoltFx(pub Vec<BoltStrike>);

impl std::hash::Hash for BoltFx {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// See [`Gen::blast_hits`] — hash-SILENT ALWAYS, like [`BoltFx`].
#[derive(Default, Clone)]
pub(crate) struct BlastHits(pub Vec<u16>);

impl std::hash::Hash for BlastHits {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

impl BlastHits {
    pub(crate) fn set(&mut self, slot: usize, hit: bool) {
        let s = slot as u16;
        self.0.retain(|&x| x != s);
        if hit {
            self.0.push(s);
        }
    }
    pub(crate) fn hit(&self, slot: usize) -> bool {
        self.0.contains(&(slot as u16))
    }
}

/// See [`Gen::volcano_blind`] — hash-SILENT ALWAYS, exactly like
/// [`BoltFx`]: it is a per-tick OBSERVATION of writes the sim already
/// made, never an input to one, so dropping it changes nothing.
#[derive(Default, Clone)]
pub(crate) struct VolcanoBlind(pub Vec<u16>);

impl std::hash::Hash for VolcanoBlind {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// See [`Gen::mc2_pred_axis`]. Hash-SILENT: retail keeps this in a
/// data-segment global, not in any pool record, so it is not part of
/// the hashed retail state and adding it must not move a golden.
#[derive(Default, Clone, Copy)]
pub(crate) struct Mc2PredAxis(pub (u16, u16, i16));

impl std::hash::Hash for Mc2PredAxis {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// See [`Gen::mc2_beam_defer`] — hash-SILENT ALWAYS. It is armed and
/// drained inside a single `mc2_lightning_beam_tick`, so it is empty at
/// every hash and snapshot boundary by construction.
#[derive(Default, Clone)]
pub(crate) struct BeamDefer {
    /// True while the beam is marching: `mc2_proj_impact` parks instead
    /// of firing.
    pub armed: bool,
    /// The parked `(flyer slot, victim)` — replayed after the trail.
    pub pending: Option<(usize, u16)>,
}

impl std::hash::Hash for BeamDefer {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// See [`Gen::mc2_steal_mail`] — (wraith slot, hand: 1 = right,
/// 2 = left) requests from the m26 steal roll, drained by the world
/// tick the same turn (the book lives world-side). Empty at hash
/// time like a read mailbox; tagged against adjacent-mail aliasing.
#[derive(Default, Clone)]
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
#[derive(Default, Clone)]
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
#[derive(Clone)]
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
#[derive(Default, Clone)]
pub(crate) struct Mc2Recycle {
    pub(crate) stack: Vec<u16>,
    pub(crate) refill: bool,
    /// Victims seized so far (the `exhausted` counter's twin —
    /// observability, not behavior; the original keeps no such count).
    pub(crate) seized: u32,
    /// [`crate::patches::WorldPatches::mc1_recycle_victim_revalidate`]
    /// mirrored here (a MODE like `refill`, set by `World::set_patches`
    /// — the allocator has no ctx to read it from): a popped victim is
    /// seized only if its CURRENT flags still carry the mask the stack
    /// was armed with (`victim_mask`); a re-minted slot is skipped.
    pub(crate) revalidate: bool,
    /// Stale entries the revalidating pop skipped (telemetry).
    pub(crate) skipped_stale: u32,
}

impl std::hash::Hash for Mc2Recycle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // `refill`/`revalidate` are MODES and `seized`/`skipped_stale`
        // telemetry — hash-silent.
        if !self.stack.is_empty() {
            self.stack.hash(state);
        }
    }
}

/// The human's wizard seat ([`Gen::human_seat`]). Hashes to NOTHING
/// at seat 0 — every retail single-player world — so no golden moves;
/// a nonzero seat writes a tag and the seat.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct HumanSeat(pub u8);

impl std::hash::Hash for HumanSeat {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0 != 0 {
            state.write_u8(0xE7);
            state.write_u8(self.0);
        }
    }
}

/// A/B toggle for [`Gen::set_human_tag_slot`]: set
/// `MGC_NO_HUMAN_SLOT_TAG` to keep the human's owner tag at the
/// out-of-pool sentinel `0xFFFF` (the pre-multiplayer port).
pub(crate) fn no_human_slot_tag() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_HUMAN_SLOT_TAG").is_some())
}

/// The human's owner tag ([`Gen::human_tag`]) — hashes to NOTHING:
/// it is load-static, and the tag VALUES it writes into the pool are
/// what the hash sees.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct HumanTag(pub u16);

impl Seats<HumanTag> {
    /// Does any seat own `tag`? A field-level test, usable while
    /// another `Gen` field is borrowed (the pool walk's `iter_mut`).
    pub(crate) fn holds(&self, tag: u16) -> bool {
        self.ixs().any(|hx| self[hx].0 == tag)
    }
}

impl Default for HumanTag {
    fn default() -> Self {
        Self(crate::mc1::mobs::PLAYER_TARGET)
    }
}

impl std::hash::Hash for HumanTag {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// A counter that hashes to NOTHING at zero (see [`Mc2Ord`]). The
/// const TAG (unique per field) disambiguates ADJACENT quiet fields:
/// without it, (drain=5, scrolls=0) and (drain=0, scrolls=5) feed
/// identical byte streams (the conditional-hash aliasing class).
/// Written INSIDE the condition, so zero fields contribute nothing.
#[derive(Default, Clone)]
pub(crate) struct Mc2Quiet<const TAG: u8>(pub i32);

impl<const TAG: u8> std::hash::Hash for Mc2Quiet<TAG> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0 != 0 {
            state.write_u8(TAG);
            state.write_i32(self.0);
        }
    }
}

/// A value that hashes to NOTHING — for per-tick echoes of a World
/// toggle whose state the World side already hashes (tag-only while
/// on), so the echo cannot move a golden. `Gen` is `#[derive(Hash)]`;
/// a bare field here would.
#[derive(Default, Clone, Copy)]
pub(crate) struct HashSilent<T>(pub T);

/// One seat's WALK VIEW — what the walk publishes about a human between
/// his record reads: the tick-top copy of his pose (`+72/+74/+76`),
/// heading (`+28`), purse and ceiling (`+140`/`+136`) and life sign,
/// re-published at the walk's phase points (the carpet's own slot, the
/// warp and respawn peeks, the below-carpet view). Walkers read a human
/// off his RECORD; the view stands in where no record homes the lane
/// (record-less unit rigs, the pre-record load settle) or a switch
/// restores the published copy, and it carries the two deliberate phase
/// views (the carpet's own warp-peek probe, the wake pre-pass reference).
/// `dead_top` is the TICK-TOP death latch the roster scans read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SeatView {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) z: i16,
    /// The heading (the wizard entity's +30) — the genie's ambush
    /// blink (sub_1E770 :24733) lands ahead of the TARGET along the
    /// target's own yaw.
    pub(crate) yaw: u16,
    /// The castable pool (+140) — the genie's mana hunt (:24523-46)
    /// takes the first wizard holding ANY mana.
    pub(crate) mana: u32,
    /// The mana CEILING (+136): retail's ball-merge owner contest
    /// (`sub_277D0` :29755-73) reads the owner wizards' `+136`.
    pub(crate) mana_max: u32,
    /// Dead or death-falling (`life_0x8 < 0`). Republished mid-walk
    /// at the writer's slot — LIVE-read consumers (:24636, :26152)
    /// want this one.
    pub(crate) dead: bool,
    /// The tick-top `dead`, NEVER republished: roster-MEMBERSHIP
    /// consumers (the genie mana hunt's class-3 chain, :24527 — the
    /// chain is built at tick top with actLife >= 0, :52254) read the
    /// value the chain was built with, not the live one.
    pub(crate) dead_top: bool,
}

impl<T> std::hash::Hash for HashSilent<T> {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// A per-tick ECHO of a DRIVER-owned flight-ext field held on [`Gen`]
/// so that POOL-side handlers can read it (the carpet is out of pool,
/// so a creature/effect handler has no entity to read the ext off).
/// Unlike [`Mc2Quiet`] it hashes to NOTHING at every value: the ext is
/// the storage of record and hashes itself, and this copy is rebuilt
/// from it by every carpet dispatch and every conformance import — so
/// it carries no state a golden could legitimately pin.
#[derive(Default, Clone)]
pub(crate) struct Mc2Echo(pub i32);

impl std::hash::Hash for Mc2Echo {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// `sub_2AF10`'s `[ebp-0xc]` — hash-SILENT, like [`Mc2Echo`]. The slot
/// is `None` at every tick boundary (the m27 mover writes it and the
/// branch machine three statements later `take`s it), so it carries no
/// state a golden could legitimately pin, and hashing it would move
/// every MC2 golden for a within-tick scratch. See
/// [`Gen::m27_v34_residue_law`].
///
/// `.1` marks a value published by the (10,34) teleporter pad's walk
/// slot (`Gen::m27_v34_publish_pad`): it is the 0xD9-path dword
/// (`sext16(pad.y)`); the 0xD8/0xDA/0xDB slot one frame deeper holds
/// a positive stack address of the seed's class, so `m27_tick` drops
/// a pad value for those bodies. Reset at the top of every MC2 walk.
/// `.2` is the ONE-FRAME-DEEPER dword (W-64, the 0xD8/0xDA/0xDB
/// paths' `[ebp-0x10]`) a same-walk writer published — `None` = a
/// pointer/stack address of the seed's class. `Gen::m27_v34_enter`
/// moves it into `.0` for a body on those paths (round 128: the
/// (10,45) building tick's `getTerrainAlt` leaves the walk's ESI
/// there, `Gen::m27_v34_publish_building`).
/// `.3` is the WALK SLOT of an ADJACENCY-SCOPED publication (dig W21,
/// round 145: `Gen::m27_v34_publish_leviathan`). The (5,23) dweller's
/// leaving survives only as far as the NEXT dispatched handler — every
/// one of them writes something at that depth, and the seed stands in
/// for a handler the port does not model — so `Gen::m27_v34_enter`
/// drops the publication unless the body it is entering is the very
/// next dispatched record. `None` = the round-127/128 publishers,
/// which are documented as the last writers at that depth for the rest
/// of the walk.
#[derive(Default, Clone)]
pub(crate) struct M27V34Slot(
    pub(crate) Option<u32>,
    pub(crate) bool,
    pub(crate) Option<u32>,
    pub(crate) Option<u16>,
);

impl std::hash::Hash for M27V34Slot {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// A slot-keyed side-channel that hashes to NOTHING while empty
/// (hash-transparent) and contributes deterministically (BTreeMap
/// order) once entries exist. Carries per-entity words that have no
/// `Ent` field home — adding a field to `Ent` would move EVERY
/// golden's hash stream. The const TAG (unique per field) keeps
/// adjacent slot-maps from aliasing (aura_claim={a} + wanted={} vs
/// its mirror); written only when non-empty, so empty maps stay
/// transparent.
#[derive(Default, Clone)]
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

/// `MGC_NO_PRECLEAR_INCLUSIVE=1` restores the pre-dig EXCLUSIVE castle
/// pre-clear box (`<` instead of retail's `<=`) — see the citation at
/// the comparison itself. Returns the epsilon folded into the limit so
/// the A/B is one flag on one binary.
fn preclear_eq() -> i32 {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *V.get_or_init(|| std::env::var_os("MGC_NO_PRECLEAR_INCLUSIVE").is_some()) {
        -1
    } else {
        0
    }
}

/// `MGC_NO_MC2_SEIZE_BLANK=1` restores the pre-dig reading, in which
/// MC2's `NewEvent_4A050` recycle arm left the tick-top roster heads
/// standing. See the citation at the blank itself (NETHERW.EXE
/// 0x6E881). MC1's blank is unconditional either way.
/// `MGC_NO_MC2_WEAVE_MODEL_GATE=1` restores the pre-dig IDENTITY gate on
/// the MC2 rival's combat whiff weave — "weave only when the target is
/// the human or another rival wizard". Retail's `sub_13890` gates on the
/// TARGET RECORD'S **MODEL BYTE** alone, with no class or roster test:
///
/// ```text
///     mov dh,[esi+0x40]   ; esi = Entities[a1x->word_0x96_150]
///     test dh,dh
///     jz   weave          ; model == 0
///     cmp  dh,0x1
///     jnz  skip           ; model != 1
/// ```
///
/// NETHERW.EXE 0x1396A (`sub_13890`, the whiff arm; the same five bytes
/// sit at 0x1392F on the landed-cast arm). Wizards are (3,0) and (3,1),
/// so an identity gate reads right until the rival hunts something else
/// whose model happens to be 0 or 1 — mc2l1 t=449, rival 138 in state
/// 0xD (HuntMana) onto slot 60, a **(5,1)** mana creature, where retail
/// weaves and the port did not. See [`World::mc2_rival_weave`].
pub(crate) fn no_mc2_weave_model_gate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_WEAVE_MODEL_GATE").is_some())
}

/// `MGC_NO_MC2_M27_V34_GUARD_RESIDUE=1` silences the SIXTH hydra `v34`
/// publisher: the **(5,15) guard's action-121 brain** `sub_23C40`
/// (VA 0x23C40, NETHERW.EXE file **0x48440**, file = VA + 0x24800).
///
/// ⭐⭐⭐ ITS CLEAN ARM LEAVES A LITERAL **ZERO** ON THE 0xD9 DWORD.
/// The prologue is `53 56 57 55 89 e5 83 ec 10` (ebp = W-20, esp =
/// W-36). ESI is the arm selector: `31 f6 xor esi,esi` (0x48450), set
/// to 1 by a mail/chain hit (0x48467 / 0x48495) and to 2 by the lethal
/// test (0x484C2). The clean arm (`esi == 0`) is `53 push ebx` (W-40) /
/// `e8 62 04 00 00 call 0x48990` (return W-44) — the wander
/// `sub_24190`, whose prologue `53 56 57 55 89 e5 83 ec 18` pushes
/// EBX on W-48 and **ESI = 0 on W-52** (the 0xD9 path's
/// `[ebp-0x10]`), and whose `[ebp-4]` local IS **W-64** (the
/// 0xD8/0xDA/0xDB dword): it takes only BYTE stores —
/// `30 e4 / 88 65 fc` (0x48B05, the packmate flag cleared) and
/// `b2 01 … 88 55 fc` (0x48B68 / 0x48B71, a packmate found) — and the
/// `actionIndex = 124` bail (`jmp 0x48be9` at 0x489EB) skips both. Its
/// callees run below W-84 and cannot reach either dword.
/// The other arms, all past the same `cmpb $0x7a,0x45(%ebx)` tail
/// (0x4863B) whose `call 0x48900` (the engage pose `sub_24100`:
/// `53 55` then two pushes, `call 0x6e4d0` → return address on W-64,
/// saved EBP on W-52 — both the seed's class):
/// * the lethal arm (`esi == 2`, 0x484E5) stores `0x7c` and makes NO
///   call — TRANSPARENT;
/// * the hit arm (`esi == 1`) calls `sub_1EEE0` (file 0x436E0:
///   `53 55`, then `movswl 0xe/0xa/0xc(row)` pushed on W-56/W-60/
///   **W-64**) — the behaviour row's `+0xC` word, and a stack pointer
///   on W-52;
/// * the acquire walk (0x48581-0x48609), on any candidate that passes
///   `id`, the squared range and `byte[0] & 0x20`, calls `sub_581E0`
///   (file 0x7C9E0: `push ebx` = the guard pointer on W-52, then the
///   `cwtl` dy on W-64) and `sub_582B0` (file 0x7CAB0: `push ebp` on
///   W-52). The dy of the LAST tested candidate is not modelled.
///
/// Adjacency-scoped like the round-145 dweller. WITNESS mc2l21 body
/// 101 (0xD9) behind guard slot 87, with only class-0 records and
/// three `(10,79)` pieces between (see
/// [`no_mc2_m27_v34_piece_transparent`]): retail reads **exactly 0** at
/// t=14,856 / 14,862 / 14,868 — the `v34 == 0` wizard-scan arm. See
/// [`Gen::m27_v34_publish_guard`].
pub(crate) fn no_mc2_m27_v34_guard_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_GUARD_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_METEOR_RESIDUE=1` silences the SEVENTH hydra
/// `v34` publisher: the **(10,17) meteor's acting tick** `sub_32880`
/// (VA 0x32880, NETHERW.EXE file **0x57080**, file = VA + 0x24800).
/// Prologue `53 56 57 55 89 e5 83 ec 10` (ebp = W-20, esp = W-36).
/// The acting arm opens a ring iterator — `push eax / push eax /
/// call 0x34880` (0x57143-45, `sub_10080`) whose result is kept in
/// **EDI** (`89 c7`, 0x5714D) — and ends its cell loop on the
/// iterator step `lea -4(%ebp) / push / lea -8(%ebp) / push /
/// push edi / call 0x34930` (0x5722E-37, `sub_10130`: args on
/// W-40..W-48, return address — linear 0x213A3C, even — on **W-52**)
/// whose prologue `53 56 57 55` pushes ebx W-56, esi W-60 and
/// **EDI = the iterator handle on W-64**. The loop's last iteration
/// returns 2 and the tick closes on `push edi / call 0x34900`
/// (`sub_10100`, `55 89 e5 … 5d c3`, deepest write W-48), so the
/// handle SURVIVES on the 0xD8/0xDA/0xDB dword. `sub_10080` returns
/// the LOWEST free row of a 100-row table (`cmpl $-1, 0x17ee8(,…)`,
/// 0x34897) and every user frees its row before returning, so the
/// handle is **1**: nonzero, NOT `> 4`, ODD — the branch keeps state 1
/// and SKIPS the wander draw. The expiry arm (`life < 0`,
/// `call 0x7c710` at 0x57098) is a leaf and publishes nothing.
/// Adjacency-scoped. WITNESS mc2l21 t=15,320: body 101 on the 0xDA
/// path behind meteor slot 87 (life 5), with class-0 records and three
/// idle `(10,79)` pieces between — retail's branch 114 keeps its yaw
/// and draws once (`heading` 1707, `rand` 22473) where the port's
/// seed stamped `f71 = 4` and drew twice. See
/// [`Gen::m27_v34_publish_meteor`].
pub(crate) fn no_mc2_m27_v34_meteor_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_METEOR_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_TOKEN_TRANSPARENT=1` restores the pre-dig
/// reading of an IDLE class-15 spell manifestation as a dispatched
/// handler that breaks an adjacency-scoped hydra `v34` publication.
/// Every manifestation body is the class-15 action `3·spell` row of
/// `x_DWORD_D4C52ar_strF0` (EF:1951; linear = file + 0x1BC800), and
/// every one of the 26 leaves on `word_0x2E_46 <= 0` (`cmpw
/// $0x0,0x2e(%reg)` or `mov 0x2e,%dx / test / jle`) for a tail that is
/// only the `word_0x36_54` cooldown decrement and the epilogue — no
/// call, no push, no `[ebp-N]` store — and whose pre-gate code stores
/// nothing at or below W-52 (`sub esp` frames of at most 0xC, the
/// deepest pre-gate store spell 14's `[ebp-4]` = W-24). Checked in the
/// shipped NETHERW.EXE for all 26 (e.g. spell 0 file 0x8DBF0 → tail
/// 0x8DDE6, spell 9 `sub_6AB00` 0x8F311 `jle 0x8f4d9`, spell 10
/// 0x8F578 `jle 0x8f967`, spell 11 0x8F9CE `jle 0x8fab5`, spell 14
/// 0x8FE10 → 0x9000D, spell 22 0x91070 → 0x91268), with two
/// exceptions: spell 2 (the castle, file 0x8E2B0) runs `sub_6D880`
/// (file 0x92080) on its idle tail, which reaches W-52 through
/// `call 0x91de0` only when a tier is pending (`word_0x2C_44`), and
/// spell 7 (see [`no_mc2_m27_v34_lightning_token_residue`]).
/// Human manifestations only — the port keeps a rival book's
/// countdown at the caster's slot for level-load books. See
/// [`World::mc2_v34_token_post`].
pub(crate) fn no_mc2_m27_v34_token_transparent() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_TOKEN_TRANSPARENT").is_some())
}

/// `MGC_NO_MC2_M27_V34_LIGHTNING_TOKEN_RESIDUE=1` silences the NINTH
/// hydra `v34` publisher: the **Lightning (spell 7) manifestation**
/// `sub_6A5C0` (VA 0x6A5C0, NETHERW.EXE file **0x8EDC0**, class-15
/// action 21). Its prologue is `53 56 57 55 89 e5 83 ec 20` (ebp =
/// W-20, 0x20 of locals ⇒ esp = W-52) and its FIRST statements, ahead
/// of the idle gate, are `31 d2 xor edx,edx / 8b 45 14 / 89 55 e0
/// mov %edx,-0x20(%ebp) / 89 55 e4 mov %edx,-0x1c(%ebp)` (0x8EDC9..
/// 0x8EDD1): **`[ebp-0x20]` IS W-52**, zeroed on every tick. On the
/// idle arm (`jle 0x8f19f` at 0x8EDD9, a call-free tail) that zero is
/// what a following 0xD9 body reads — the `v34 == 0` wizard-scan arm
/// — and W-64 is untouched. (The armed arm re-stores the local at
/// 0x8F11D and calls from `esp = W-52`, so it is not modelled.)
/// WITNESS mc2l19-taketwo t=13,280 and t=13,304: body 20 on the 0xD9
/// path, the human's idle lightning token at slot 15 behind the three
/// houses (whose own leaving there is a record POINTER), only idle
/// manifestations between. Branch 27 heads the chain at 13,280 and
/// retail scans and locks the human (`b46` 0 → 2, `target96` 0 → 318,
/// its `word_0x26` untouched ⇒ not the draw-B arm); branch 419 at
/// 13,304 scans and finds nobody (`b46` 0 → 1, two draws). Human
/// tokens only. Unscoped when the slot holds an unscoped leaving (the
/// house's), adjacency-scoped otherwise. See
/// [`World::mc2_v34_token_post`].
pub(crate) fn no_mc2_m27_v34_lightning_token_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_LIGHTNING_TOKEN_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_METEOR_CAST_RESIDUE=1` silences the EIGHTH
/// hydra `v34` publisher: the human's **Meteor (spell 9) fire tick**,
/// `sub_6AB00` (VA 0x6AB00, NETHERW.EXE file **0x8F300**, the class-15
/// action-27 row). Prologue `53 56 57 55 89 e5 83 ec 04` (ebp = W-20,
/// esp = W-24). On the FIRST afforded tick (`word_0x2E_46 ==
/// word_0x30_48`, 0x8F356) it spawns the bolt (`call 0x924a0`,
/// 0x8F398) and then calls the hand-muzzle `sub_68E50`
/// (`push token / push bolt / push caster / call 0x8d650`, 0x8F3AE-B4:
/// return address W-40) whose prologue `53 56 57 55 89 e5 83 ec 08`
/// puts `ebp` at W-56 — so its **`[ebp-8]` IS W-64**, and that is the
/// 6-byte position buffer it fills with `movsl / movsw` (0x8D668) and
/// hands to `MoveEntity_57FA0` / `getTerrainAlt` before copying it back
/// onto the bolt (`call 0x7c4f0`, 0x8D6F7 / 0x8D71E / 0x8D7DB). Every
/// callee runs from `esp <= W-64`. After it the tick makes only
/// shallow calls — `MoveEntity` on the aim axis (0x8F473: return W-44,
/// deepest push W-56), `sub_68DE0` (0x8F4AE, file 0x8D5E0, a leaf,
/// deepest W-44) and, when the window closes, `sub_6D880` (file
/// 0x92080, deepest W-40 unless a tier is pending). So a 0xDA body
/// walked after the fire reads **`muzzle.x | muzzle.y << 16`**: the
/// parity of the muzzle x, and NEGATIVE whenever `muzzle.y >= 0x8000`.
/// (W-52 holds `MoveEntity`'s saved ESI — not modelled; the seed's
/// class.) Adjacency-scoped, with idle manifestations transparent
/// ([`no_mc2_m27_v34_token_transparent`]).
/// WITNESS mc2l19-taketwo t=12,906: body 20 (0xDA) behind the three
/// (10,45) houses at slots 3-5 and the human's manifestations at
/// 6..19; spell 9's token at slot 17 fires on this tick (mana
/// 79,105 → 59,105) and is the ONLY one of the take's ten state-0
/// gate openings behind the houses where a manifestation fires.
/// Retail's branch 409 keeps state 1 (`b46` 0 → 1) and TAKES the
/// wander draw (`rand` two steps) where the house's 29 — right on the
/// other nine — stamped `f71 = 4` and skipped it. See
/// [`World::mc2_v34_token_post`].
pub(crate) fn no_mc2_m27_v34_meteor_cast_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_METEOR_CAST_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_SPHERE_RESIDUE=1` silences the TENTH hydra
/// `v34` publisher: the **(10,39) mana ball's moving arm**,
/// `TransformArcherToMana_35940` (VA 0x35940, NETHERW.EXE file
/// **0x5A140**, file = VA + 0x24800). Prologue `53 56 57 55 89 e5 83
/// ec 1c` (ebp = W-20, esp = W-48). The settle arm (`byte_0x39_57 ||
/// v35`, i.e. a `+58` count or a kick) samples the ground at the
/// stepped position — `push $0x1b398 / call 0x35440` (0x5A5E4-E9,
/// `getTerrainAlt_10C40`) — and keeps the FULL 32-bit sampler result
/// in **ESI** (`89 c6`, 0x5A5F8: the interpolator at file 0xDA460
/// returns `32*h + interp` in EAX). ESI is not written again (the
/// callees 0x35460 / 0x7C4F0 / 0x35250 / 0x5B550 / 0x7C830 all save
/// it), and the arm ends, when `!(byte[1] & 0x20) || v36`
/// (0x5A731-3B), on `53 push ebx / e8 dd 09 00 00 call 0x5b120`
/// (0x5A73D-3E, `SetManaSphereColorAndRot_36920`): the sphere pointer
/// on **W-52**, return W-56, and its prologue `53 56 57 55 89 e5 83
/// ec 04` pushes EBX W-60 and **ESI on W-64** (0x5B121); its own
/// callees (0x5B1F0, 0x6E490) run from `esp <= W-76`. The tail after
/// it is a `life` decrement and at most the
/// `DisableEntityDrawing04_57F10` leaf (0x5A796). So a 0xD8/0xDA/0xDB
/// body walked after a moving sphere reads **the ground altitude the
/// sphere just sampled** — positive, and EVEN or ODD with the terrain.
/// The idle sphere (`+58 == 0`, no kick, no tether) makes no call and
/// stays transparent, which is why the building's 29 survives settled
/// spheres. Unscoped, like the building's leaving.
/// WITNESS mc2l22-new t=49,984 and t=49,997: body 610 (0xDA) behind
/// the (10,45) house at slot 565, with sphere 581 moving (kicked at
/// 49,984: `b39` 0 → 16; counting at 49,997: `b39` 4 → 3) — the only
/// record in the gap that differs from the take's twenty-odd
/// house-parity openings where retail skips the wander draw; here
/// retail stamps `f71 = 4` AND draws (branches 622 / 633: two LCG
/// steps). See [`Gen::m27_v34_publish_sphere`].
pub(crate) fn no_mc2_m27_v34_sphere_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_SPHERE_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_BLAST_SOUND_RESIDUE=1` silences the ELEVENTH
/// hydra `v34` publisher: the **(10,23) blast's burst tick**,
/// `sub_33D80` (VA 0x33D80, NETHERW.EXE file **0x58580**). Prologue
/// `53 55 89 e5` — NO locals, ebp = W-12. The burst arm (`life >= 0`,
/// `!(byte[0] & 2)`) calls `sub_10C80` and (on hits) `sub_6D8B0` with
/// return addresses on W-28, then LAST `6a 18 / 6a ff / 50 / call
/// 0x92c50` (0x585EE-F4, `PrepareEventSound_6E450(id, -1, 24)`:
/// return W-28). Its prologue `53 56 57 55 89 e5 83 ec 24` puts
/// `ebp` at **W-44**, so `[ebp-8]` IS **W-52** and `[ebp-0x14]` IS
/// **W-64**. Past the sound-enabled bytes (0x13799 / 0x13798), the
/// emitter's `testb $0x80,0xc` (0x92CA7) and the hearing range —
/// `sub_584D0` (file 0x7CCD0: i16 deltas squared) against
/// `cmp $0x9000000 / ja` (0x92CDF) — it stores `89 45 ec` (0x92D2C)
/// the result of `call 0x7cc90` (`EuclideanDistXYZ_58490`, args
/// `push esi` = the blast's position, `push edi` = the LISTENER's —
/// the local player's carpet record via `0x2be8 + 0x84c*idx`) and
/// `89 45 f8` (0x92D3A) the `sub_581E0` atan2 word. Neither local is
/// written again, and every later call runs from `esp <= W-80`. So a
/// 0xDA body walked after a bursting blast reads
/// **`isqrt(dx² + dy²)` from the human to the blast** (the W-52 atan2
/// word is not modelled). The later (10,23) ticks — `life` pinned to
/// 1 with the latch set (`jne 0x5860c`, no call) and the reap
/// (`DisableEntityDrawing04_57F10`, a leaf) — are transparent.
/// Unscoped, like the building's leaving. Sound is assumed enabled in
/// every recorded take.
/// WITNESS mc2l22-new t=49,898: body 610 (0xDA) behind the house at
/// 565; blast 587 bursts (`life` 8 → 1, `flags` 0x20005 → 0x20007)
/// 2,818 units from the human (31, 21671) — nonzero, `> 4`, EVEN:
/// retail's branch 660 stamps `f71 = 4` and draws twice. At t=49,901
/// the same gap holds only latched/reaped blasts and retail reads the
/// house's 29. See [`Gen::m27_v34_publish_blast_sound`].
pub(crate) fn no_mc2_m27_v34_blast_sound_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_BLAST_SOUND_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_DOLMEN_RESIDUE=1` restores the pre-dig reading
/// of the **(2,2) dolmen** as a handler that leaves an UNSCOPED hydra
/// `v34` publication (the building's 29, a moving sphere's ground, a
/// blast's listener distance, a pad's/switch's `y`) standing.
/// `AddDolmen02_02_65080` (VA 0x65080, NETHERW.EXE file **0x89880**)
/// is `53 56 57 55 89 e5` — four pushes, NO locals, ebp = esp = W-20
/// — and ends EVERY tick, after its player sweep (`call 0x34ec0` from
/// W-24/W-28), on `mov 0x14(%ebp),%eax / add $0x4c,%eax / push eax /
/// mov 0x14(%ebp),%ebx / call 0x35440` (0x898D0-DA,
/// `getTerrainAlt_10C40`: arg W-24, return W-28). `getTerrainAlt`
/// pushes ebp W-32, y W-36, x W-40 and calls the interpolator
/// (return W-44), whose prologue `55 8b ec 53 51 52 56 57` (file
/// 0xDA460) pushes ebp W-48, **EBX = the dolmen's record pointer on
/// W-52**, ecx W-56, edx W-60, **ESI on W-64** — and ESI is the
/// sweep's player cursor, `[0x41a0] + 0x2bde + 0x84c * count`
/// (0x89886-8E, stepped at 0x898BE): a POINTER. Both dwords are
/// positive, even and far above 4 — the seed's class — on every tick.
/// WITNESS mc2l24-crazy t=51,654: body 15 (0xDA) with the dolmen at
/// slot 14 and a moving (10,39) at slot 9 (ground 997, odd) — retail
/// stamps `f71 = 4` and takes the wander draw; without this law the
/// sphere's odd word reached the body and cut the take's horizon
/// 53,387 → 51,653. See [`crate::engine::world::World`]'s
/// `dolmen_tick`.
pub(crate) fn no_mc2_m27_v34_dolmen_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_DOLMEN_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_SPHERE_TRANSPARENT=1` restores the pre-dig
/// reading of an IDLE `(10,39)` mana ball as a dispatched handler that
/// breaks an adjacency-scoped hydra `v34` publication.
/// `TransformArcherToMana_35940` (file 0x5A140, see
/// [`no_mc2_m27_v34_sphere_residue`] for the frame) makes a call only
/// on: the claim intake's `PrepareEventSound_6E450` (w68 set, not the
/// owner, and `dword_0x64` or the lock clear), the kick
/// (`word_0x7A_122`: `sub_581E0` + `MoveEntity_57FA0`), the tether
/// (`byte[0] & 0x40` onto a (3,3)/(5,23)), the settle arm
/// (`byte_0x39_57 || v35`) and the decay tail's
/// `DisableEntityDrawing04_57F10` — a LEAF (file 0x7C710:
/// `55 89 e5 8b 45 08 80 48 0d 04 5d c3`, deepest write W-36). The
/// stall arm (`byte[1] & 8`, 0x5A157-64: clear and `jmp 0x5a79e`) makes
/// none. So a settled, unclaimed, unkicked, untethered sphere — and a
/// stalled one — leaves both dwords as it found them.
/// WITNESS mc2l22-new t=49,409 / 49,413 / 51,146 / 50,488: body 610
/// behind a (5,15) guard's wander tick (its `[ebp-4]` BYTE store over
/// the house's 29 leaves exactly 0) with only idle spheres, a banished
/// rival and (9,9) nodes between: retail reads **0** and runs the
/// wizard scan (`b46` 0 → 2 / 0 → 1 with the wander draw).
pub(crate) fn no_mc2_m27_v34_sphere_transparent() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_SPHERE_TRANSPARENT").is_some())
}

/// `MGC_NO_MC2_BALL_TETHER_DECAY=1` restores the pre-dig ball tick,
/// whose COLLECTOR-TETHER arm returned before
/// [`crate::engine::features::Gen::ball_decay_tail`] instead of
/// falling into it. `TransformArcherToMana_35940`'s decay tail is
/// OUTSIDE the mode branch and all three arms jump to it
/// (`NETHERW.EXE` 0x5A2BB / 0x5A312 / 0x5A3D8 -> 0x5A746); see the
/// call site in mc1/combat.rs for the bytes and the mc2l24-crazy
/// t=73775 witness.
pub(crate) fn no_mc2_ball_tether_decay() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_BALL_TETHER_DECAY").is_some())
}

/// `MGC_NO_MC2_M27_V34_NODE_TRANSPARENT=1` restores the pre-dig
/// reading of a `(9,9)` lightning trail node (class 9 action 14) as a
/// handler that breaks an adjacency-scoped hydra `v34` publication.
/// Its handler (`x_DWORD_D4C52ar_str90[14]` = linear 0x248410, file
/// **0x8BC10**) is `53 55 89 e5 / mov 0xc(%ebp),%edx / life-- /
/// jge → pop/ret`, calling only `DisableEntityDrawing04_57F10` (a leaf)
/// when the pre-decrement life is negative (0x8BC24-25): it never
/// reaches W-52. See [`no_mc2_m27_v34_sphere_transparent`] for the
/// witness.
pub(crate) fn no_mc2_m27_v34_node_transparent() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_NODE_TRANSPARENT").is_some())
}

/// `MGC_NO_MC2_M27_V34_CORPSE_TRANSPARENT=1` restores the pre-dig
/// reading of a dead RIVAL wizard (class 3, action 3) as a handler that
/// breaks an adjacency-scoped hydra `v34` publication. `sub_5E7C0`
/// (`x_DWORD_D4C52ar_str30[3]` = linear 0x23F7C0, file **0x82FC0**,
/// `53 56 57 55 89 e5`, no locals) tests the player row's
/// `cmpb $0x1,0x2be7(…)` (0x82FEB; remc2's `IsAiPlayer`): on the AI
/// arm, with a castle (`cmpw $0x0,0x3a(%eax)`, 0x82FFD) it either
/// decrements `dword_0x10_16` (0x8301A-24, no call) or respawns
/// (`call 0x81150`, deep); castle-less (BANISHED) it inlines the
/// notice `strcpy` and stores two words (0x83025-65) — no call. Only
/// the human arm (0x83066) calls `sub_5C800`/`sub_5E6C0`, whose
/// `getTerrainAlt` leaves an odd return address on W-52 and
/// `sext16(x)` on W-64 (0x82F62/0x82F78) — not modelled. See
/// [`no_mc2_m27_v34_sphere_transparent`] for the witness (rival 584,
/// banished, sits between the guard and the body on every one).
pub(crate) fn no_mc2_m27_v34_corpse_transparent() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_CORPSE_TRANSPARENT").is_some())
}

/// `MGC_NO_MC2_M27_V34_WYVERN_RESIDUE=1` silences the TWELFTH hydra
/// `v34` publisher: the **(5,16) wyvern's attack brain** `sub_24510`
/// (class-5 action 130, `x_DWORD_D4C52ar_str50[130]` = linear
/// 0x205510, NETHERW.EXE file **0x48D10**). Prologue `53 56 57 55 89 e5
/// 83 ec 0c` (ebp = W-20, esp = W-32); `31 f6 xor esi,esi` (0x48D2D)
/// is the arm selector — set to 1 by a mail/chain hit (0x48D47 /
/// 0x48D75) and 2 by the lethal test (0x48D9F).
/// * The hit arm (`esi == 1`, 0x48DCB: copy the source into
///   `word_0x96_150`, return) and the lethal arm (`esi == 2`, 0x48DC2:
///   `movb $0x84,0x45`, jump to the epilogue) make NO call —
///   TRANSPARENT.
/// * The normal arm calls the move core (`push ebx / call 0x400c0`,
///   return W-40 — its prologue leaves the brain's EDI on W-52) and
///   then, ALWAYS, `push edi / push ebx / call 0x43530` (0x48DF6-F8,
///   `sub_1ED30`: return W-44; prologue `53 56 57 55 89 e5 83 ec 08`
///   pushes EBX W-48 and **ESI = 0 on W-52**; its callees run below
///   W-68). The 8-tick aim (`call 0x7c9e0` at 0x48E3B, prologue
///   `53 56 55` → ESI on W-52 again) keeps the 0. Every exit up to
///   0x48F19 (bad pointer 0x48E0A, dead target 0x48E53, off-cadence
///   0x48F19, out of range 0x48F44) therefore leaves **0** — UNLESS
///   `dword_0x10_16` armed the bolt (0x48E5E-0x48EF9: `call 0x6e990`,
///   then `sub_581E0` / `sub_58210` with the new record in ESI, a
///   POINTER). On the in-range cadence tick `movzbl 0x3e(%ebx),%esi`
///   (0x48F00) and the aim `call 0x7c9e0` (0x48F90) put the **phase
///   byte** on W-52; `sub_582B0` (0x7CAB0) and `sub_62F70` (0x83770)
///   are leaves (`55 89 e5 … 5d c3`), and the sound call at 0x48F80
///   precedes the aim. The 0xDA dword (W-64: the move core's pushed
///   row word, `sub_1ED30`'s `[ebp-4]`, the aim's `dx`) is NOT
///   modelled. Adjacency-scoped.
///
/// WITNESS mc2l22-new t=35,058: body 610 on the 0xD9 path, wyverns 603
/// / 606 / 607 on their normal arm and 609 on the hit arm between the
/// last publisher and the body — retail's branch 612 reads **0**: the
/// wizard scan finds nobody (`b46` stays 1) and the wander draw is
/// taken (two LCG steps), where the seed stamped `f71 = 4` (the take's
/// 35,059 head). See [`Gen::m27_v34_publish_wyvern`].
pub(crate) fn no_mc2_m27_v34_wyvern_residue() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_WYVERN_RESIDUE").is_some())
}

/// `MGC_NO_MC2_M27_V34_PIECE_TRANSPARENT=1` restores the pre-dig
/// reading of a `(10,79)` castle defender piece as an ordinary
/// dispatched handler that breaks an adjacency-scoped hydra `v34`
/// publication. `sub_3AF00` (VA 0x3AF00, NETHERW.EXE file **0x5F700**)
/// opens `53 56 57 55 89 e5 83 ec 30` — four pushes and **0x30 of
/// locals** — so `ebp = W-20`, `esp = W-68`, and **`[ebp-0x20]` IS
/// W-52 and `[ebp-0x2C]` IS W-64**: the piece owns both dwords as its
/// own locals, and no callee (every call is made from `esp <= W-68`)
/// can reach them. The only writes are on the case-3 arm (jump table
/// `cs:0x2AEA8` = file 0x5F6A8, entry 3 → 0x5F7D1) past its
/// `testb $0x3f,0x3e(%ebx)` gate: `89 45 e0` (0x5F7E9, the scan
/// tile x) and `sub_10130`'s out-parameter `&[ebp-0x2c]` (0x5F8EF /
/// `call 0x34930` at 0x5F8F4). So on every other tick — idle states,
/// the dwell countdown, the wind-up, the volley, the dead/ownerless
/// exits at 0x5F716 / 0x5F723 — the piece is TRANSPARENT and the
/// publication survives it. See [`Gen::m27_v34_transparent`].
pub(crate) fn no_mc2_m27_v34_piece_transparent() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M27_V34_PIECE_TRANSPARENT").is_some())
}

/// `MGC_NO_MC2_MOB_CHAIN_PREDICATE=1` restores the pre-dig LIVE-POOL
/// scans on every MC2 consumer that retail reaches through
/// `bytearray_38403x[model]` — see [`Gen::mc2_roster`] for the law and
/// the NETHERW.EXE citation.
pub(crate) fn no_mc2_mob_chain_predicate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_MOB_CHAIN_PREDICATE").is_some())
}

/// `MGC_NO_MC2_CASTLE_BALL_SPEED_STEP=1` restores the pre-dig CLAMPED
/// castle-ball speed servo, `actSpeed += (minSpeed - actSpeed).clamp(-2, 2)`.
/// Retail's step is a SIGN times two and never clamps: both castle-ball
/// arms run
///
/// ```text
///     v = minSpeed - actSpeed;
///     if (minSpeed != actSpeed) v = (v <= 0) ? -1 : 1;
///     actSpeed += 2 * v;
/// ```
///
/// NETHERW.EXE 0x66B73-0x66BAA (`CastCastleProjectile_66B30`, the
/// upgrade flight) and 0x66DEB-0x66E17 (`sub_66D00`, the create
/// flight) are the same eight instructions: `sub`/`jz`/`test`/`jng`
/// then `lea esi,[eax*2]` and `add`. The two forms differ only when
/// |minSpeed - actSpeed| == 1, where retail OVERSHOOTS by one and then
/// oscillates about minSpeed forever. See [`Gen::mc2_castle_ball_tick`].
pub(crate) fn no_mc2_castle_ball_speed_step() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_BALL_SPEED_STEP").is_some())
}

/// A/B toggle for THE CASTLE BALL'S FLIGHT STEP COMMITS BEFORE THE
/// PROBES: set `MGC_NO_MC2_CASTLE_BALL_STEP_COMMIT` to restore the
/// pre-dig single end-of-tick `move_relink` in
/// [`Gen::mc2_castle_ball_tick`]. `sub_66D00` commits the step with
/// `CopyEntityPosition_57CF0` immediately after `MoveEntity_57FA0`
/// (EF:59018-19) and commits the refused 180° retreat with a SECOND
/// call (EF:59060-63), so a ball that leaves its tile and comes back
/// is unlinked and re-added twice and ends the tick at the HEAD of
/// its own tile chain. Ledger ROUND 149 dig w149f.
pub(crate) fn no_mc2_castle_ball_step_commit() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_BALL_STEP_COMMIT").is_some())
}

/// `MGC_NO_CASTLE_EJECT_GC=1` restores the pre-dig castle ejector:
/// a dry free stack aborts the mana burst outright, with no
/// `sub_49F90` GC pass, no `v3 = 8` burst ceiling and no victim-stack
/// clear. See [`Gen::mc2_eject_gc`].
pub(crate) fn no_castle_eject_gc() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_CASTLE_EJECT_GC").is_some())
}

/// ⚠ **MEASUREMENT DEVICE ONLY — NOT RETAIL** (dig w159h, round
/// 159). `MGC_MC1_OOB_CASTLE_INERT=1` makes an MC1 castle whose
/// level byte `+26` has gone OUT OF THE 8-ROW BUILD LADDER (>= 8 —
/// only reachable through the registered `volcano_register_revalidate`
/// corruption, `docs/DEVIATIONS.md`) INERT: quota 0 (the fleet is
/// culled by the level-0 cull tail, which is what retail's OOB quota
/// read did on mc1l45-froze / mc1l26-froze) and `castle_downgrade` a
/// no-op (no HP re-lift, no ladder reset, no footprint un-stamp — the
/// three port guards `CASTLE_HP[min(7)]`, `build_tab[lvl % len]` and
/// `f71 = lvl0.min(8)` that keep the port alive where retail walked
/// OOB and hung). It exists to SILENCE the registered storm so the
/// rest of the excused tail can be classified. Retail's arm (the
/// switch unset) stays the replay truth.
/// Arms: `quota` (the fleet/guard quota), `teardown` (castle_downgrade's
/// HP/ladder/un-stamp), `paint` (the m42 painter + m41 leveler).
/// `MGC_MC1_OOB_CASTLE_INERT=1` (or `all`) turns on all three; a
/// comma list turns on just those arms, which is how each divergence
/// FAMILY was assigned to its guard.
pub(crate) fn mc1_oob_castle_inert(arm: &str) -> bool {
    static V: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let v = V.get_or_init(|| std::env::var("MGC_MC1_OOB_CASTLE_INERT").ok());
    match v {
        None => false,
        Some(s) if s == "1" || s == "all" => true,
        Some(s) => s.split(',').any(|a| a.trim() == arm),
    }
}

/// ⭐ **THE CASTLE/BUILD ROW BYTE `+71` IS SIGNED.** The m42 painter's
/// body (`sub_24DA0`, remc1 sub_main.cpp:30525-32 / :30538-45) reads
/// the row as `*(char *)(a1 + 71)` and runs its per-row goal fill
/// under a SIGNED compare — `for (i = 1; ; ++i) if (i > *(char *)(a1 +
/// 71)) break;`. The shipped `CARPET.EXE` agrees: the four build-tab
/// reads at file 0x40E62 / 0x40E82 / 0x40EAF / 0x40ECF (VA 0x2866A /
/// 0x2868A / 0x286B7 / 0x286D7) are `0f be` **movsbl** `0x47(%ebp)`
/// followed by `lea (,%edx,4); sub %edx` (×3) and `mov 0x4|0x5(%edx,
/// %eax,2)` (×6 + w/h), and the loop test at file 0x41203 (VA
/// 0x28A0B) is `movsbl 0x47(%ebp),%edx / mov 0x50(%esp),%al / cmp
/// %edx,%eax / jle` — a SIGNED `i <= (char)row` continue. So a row >=
/// 128 is NEGATIVE: the fill loop runs **zero times**, the delta
/// buffer stays `memset`-zero and the painter writes **no height at
/// all**. The port read `+71` as `u8` and clamped it `.min(8)`, so a
/// corrupted castle painted a full level-8 footprint every work tick.
/// Only reachable through the registered level-250 corruption
/// (`volcano_register_revalidate`, docs/DEVIATIONS.md) — it is the
/// whole `field:z` half of the mc1l45-froze / mc1l26-froze tail.
/// ⚠ Rows 8..=127 stay UNMODELLED (retail walks build rows past the
/// end of an 8-row table there; without a memory image the clamp is
/// the only defensible stand-in).
///
/// `MGC_NO_MC1_CASTLE_ROW_SIGNED=1` restores the unsigned read.
pub(crate) fn mc1_no_castle_row_signed() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CASTLE_ROW_SIGNED").is_some())
}

/// The painter/finish build ROW, sign-extended — see
/// [`mc1_no_castle_row_signed`].
pub(crate) fn mc1_build_row(f71: u8) -> usize {
    if mc1_no_castle_row_signed() {
        f71.min(8) as usize
    } else {
        (f71 as i8).clamp(0, 8) as usize
    }
}

/// `MGC_NO_MC1_LEVELER_POOL_GATE=1` restores the UNGATED MC1 castle
/// leveler: the deferred downgrade runs even on an exhausted free
/// stack, where retail's `sub_470E0` (:56142) skips the whole teardown.
/// See the action-6 arm of [`Gen::castle_tick`].
pub(crate) fn mc1_no_leveler_pool_gate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_LEVELER_POOL_GATE").is_some())
}

/// `MGC_NO_MC1_LEVELER_PIN_GATE=1` restores the UNGATED action-6
/// Create-Castle token pin: the port used to stamp the owner's `+48`
/// on EVERY `+70 == 6` castle pass, where retail's pin/release lives
/// INSIDE `sub_47A70_47DB0` (:56529 / :56533), which `sub_470E0`
/// (:56142) calls only when `sub_37710_37AD0()` reports a non-empty
/// free stack. On an exhausted pool retail's leveler parks `+70` back
/// to 4 and touches nothing else — the token keeps whatever `+48` it
/// had. See the MC1 action-6 arm of the class-3 model-2 dispatch in
/// `World::tick`. (MC2's twin arm has always been gated: `6 if
/// downgraded`.)
pub(crate) fn mc1_no_leveler_pin_gate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_LEVELER_PIN_GATE").is_some())
}

/// `MGC_NO_MC1_LEVELER_SHAKE_GATE=1` restores the pre-dig MC1 castle
/// ground LEVELER, which stepped its translation on every tick with a
/// live counter. Retail's `sub_28200` (:30333) opens the whole work
/// body on `!castle[+42]->+50 && +26` — a SHAKING castle (blast
/// damage arms `+50 = 30`, `sub_127E0` :17523) sends the leveler
/// straight to the ELSE arm, which is the FINISH: castle sub-state 2,
/// site z = 32 * current, perimeter smooth, despawn. The twin guard on
/// the PAINTER (`sub_285C0` :30520) is only a per-tick suspend, so the
/// asymmetry is real and deliberate. See [`Gen::tick_castle_leveler`].
fn mc1_no_leveler_shake_gate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_LEVELER_SHAKE_GATE").is_some())
}

fn no_mc2_seize_blank() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SEIZE_BLANK").is_some())
}

/// `MGC_NO_HUMAN_TILE_LINK=1`: the human's record stays OUT of the
/// tile chains (the virtual [`PlayerChain`] seat alone) — see
/// [`Gen::player_relink`]. Off with the mirror too
/// (`MGC_NO_HUMAN_RECORD_MIRROR=1` = the whole stage 2 reverted).
pub(crate) fn human_tile_link() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("MGC_NO_HUMAN_TILE_LINK").is_none()
            && std::env::var_os("MGC_NO_HUMAN_RECORD_MIRROR").is_none()
    })
}

/// ⭐ MULTIPLAYER STAGE-2 TAIL — THE HUMAN'S MAIL LIVES IN HIS RECORD.
/// Retail's carpet is a pool record, so every writer that hits the
/// human writes the record's own `+90..` mailboxes and his tick drains
/// them there (MC1 `sub_46540` :55641, MC2 `sub_5EFA0`). The port kept
/// an out-of-pool `Gen::player_mail` block; with the mirrored record
/// live, [`Gen::pmail`] / [`Gen::pmail_mut`] resolve to the record's
/// `mail` instead. `MGC_NO_HUMAN_MAIL_IN_RECORD=1` restores the
/// out-of-pool block (also off with the mirror).
pub(crate) fn human_mail_in_record() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("MGC_NO_HUMAN_MAIL_IN_RECORD").is_none()
            && std::env::var_os("MGC_NO_HUMAN_RECORD_MIRROR").is_none()
    })
}

/// ⭐ MULTIPLAYER STAGE-2 TAIL — THE SEAT ARMS MEET HIM AT HIS RECORD.
/// With his record really linked ([`human_tile_link`]), a chain walk
/// reaches the human's seat when its cursor lands ON HIS RECORD, the
/// way retail's walkers meet the carpet — not in front of the virtual
/// successor [`PlayerChain::next`]. See [`Gen::human_seat_at`].
/// `MGC_NO_HUMAN_SEAT_AT_RECORD=1` restores the successor test.
pub(crate) fn human_seat_at_record() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("MGC_NO_HUMAN_SEAT_AT_RECORD").is_none() && human_tile_link()
    })
}

/// ⭐ MULTIPLAYER STAGE-2 TAIL — AN AREA WRITE BILLS HIS RECORD. With
/// his mail in his record ([`human_mail_in_record`]) and the record
/// linked where his seat is ([`human_seat_at_record`]), the MC2 ch3/ch4
/// free-only area arm meets him in its tile walk like any class-3
/// member — retail's own shape (`sub_10C80` EF:4022-38) — instead of
/// the post-walk player arm on the `ctx` pose. See
/// [`Gen::human_area_at_record`]. `MGC_NO_HUMAN_AREA_AT_RECORD=1`
/// restores the player arm.
/// ⭐ MULTIPLAYER STAGE 3 — THE WALKERS READ HIS PURSE LIVE. Retail's
/// genie mana hunt (:24527), the MC2 lightning rebound absorb and the
/// ball-merge ceiling contest (:29755-73) read `+140` / `+136` off the
/// carpet RECORD at the walker's own slot; the port handed them the
/// tick-top `MobCtx` copy. With the record the home of his purse they
/// read it there ([`Gen::human_purse`] / [`Gen::human_ceiling`]).
/// `MGC_NO_HUMAN_LIVE_PURSE=1` keeps the tick-top copy.
/// ⭐ MULTIPLAYER STAGE 3, SLICE 5c — THE WALKERS READ THE HUMAN OFF
/// HIS RECORD. Retail's walkers read the carpet record at their own
/// slot (pose, heading, life sign); the port read the `MobCtx` copy
/// the walk republished before each slot. Every reader now goes
/// through [`Gen::human_at`] / [`Gen::hpyaw`] / [`Gen::hpdead`], which
/// read the seat's record — so a seat is read where it lives, not
/// through one published view. `MGC_NO_HUMAN_ROSTER_READ=1` reads the
/// `ctx` copy.
pub(crate) fn no_human_roster_read() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_HUMAN_ROSTER_READ").is_some())
}

pub(crate) fn no_human_live_purse() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_HUMAN_LIVE_PURSE").is_some())
}

/// ⭐ MULTIPLAYER STAGE 3, CHANNEL 2 — THE HUMAN IS AN ORDINARY
/// VICTIM. Retail's carpet carries the probe's damageable bit (`+16 & 8`:
/// the recorded flags word is 0x20D in both games) and `sub_11980` /
/// `sub_10780` / the area writers meet it in its tile chain like any
/// member — they have no player arm. With his (3,0) record linked, the
/// mirror stamps bit 8 and [`Gen::human_pool_victim`] retires the port's
/// out-of-pool arms: the victim probe and the MC1/MC2 area writer bill
/// `Pool(record)`. `MGC_NO_HUMAN_POOL_VICTIM=1` restores the player arms.
pub(crate) fn no_human_pool_victim() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_HUMAN_POOL_VICTIM").is_some())
}

/// ⭐ STAGE 3 — THE HUMAN'S REBOUND BIT IS HIS RECORD'S `+17 & 0x80`
/// (port `flags & 0x8000`). Every deflect gate — the out-of-pool arm and
/// the pool arm meeting his record alike — then reads one bit, set the
/// moment the cast stamps it. `MGC_NO_HUMAN_REBOUND_ON_RECORD=1` keeps
/// `player_rebound_reg` as the home and the record bit a mirrored copy.
pub(crate) fn no_human_rebound_on_record() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_HUMAN_REBOUND_ON_RECORD").is_some())
}

pub(crate) fn human_area_at_record() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("MGC_NO_HUMAN_AREA_AT_RECORD").is_none()
            && human_seat_at_record()
            && human_mail_in_record()
    })
}

/// ⭐ MULTIPLAYER STAGE 2 — THE HUMAN RIDES THE CLASS-3 ROSTER.
/// Retail's carpet is an ordinary member of `var_u32_36462[0]` (MC1
/// :52253-62) / `dword_38519` (MC2 EF:39975): admitted at the tick-top
/// sweep by the same `actLife >= 0 && !(flags & 0x10)` gate as every
/// wizard, hidden by the same seizure blank / severed-chain cut, and
/// walked IN SLOT ORDER. The port seated him outside the roster with a
/// per-reader arm (first, last, or slot-spliced) and per-reader
/// stand-ins for the gate (`human_wiz_top`, `human_bucket_alive`,
/// `dead_top`) and the cut (`mc1_human_on_wiz_chain`). With the
/// mirrored (3,0) record on the roster, each reader meets him at his
/// slot ([`Gen::chain_human`]) and reads the same human inputs its
/// arm read. `MGC_NO_HUMAN_ON_WIZ_CHAIN=1` restores the arms.
pub(crate) fn human_on_wiz_chain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("MGC_NO_HUMAN_ON_WIZ_CHAIN").is_none()
            && std::env::var_os("MGC_NO_HUMAN_RECORD_CLASS3").is_none()
            && std::env::var_os("MGC_NO_HUMAN_RECORD_MIRROR").is_none()
    })
}

/// `MGC_NO_MC1_HUMAN_SEAT_CUT=1` restores the pre-dig pre-passes in
/// which the out-of-pool human answered a bucket[0] walk (the fireball
/// muzzle acquire, the creature wizard scan, the balloon-guard
/// election, the learn clock) even after a mid-tick seizure had
/// blanked the roster head or a NewEvent reuse had severed the chain
/// below his seat. See [`Gen::mc1_human_on_wiz_chain`].
pub(crate) fn no_mc1_human_seat_cut() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_HUMAN_SEAT_CUT").is_some())
}

/// `MGC_NO_TRIGGER_ROSTER_BLANK=1` restores the pre-dig
/// [`World::balloon_probe`], whose out-of-pool human answered the
/// class-3 roster walk even after a mid-tick seizure had blanked the
/// roster head. See [`Gen::wiz_roster_head_blanked`].
pub(crate) fn no_trigger_roster_blank() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_TRIGGER_ROSTER_BLANK").is_some())
}

/// `MGC_NO_MC2_REBUILD_VICTIM_HALF=1` restores the pre-dig
/// [`Gen::mc2_rebuild_free`], which rebuilt only the FREE half of
/// `sub_49F90`'s single descending pool scan and left the VICTIM
/// (recycle) stack untouched. See that function's doc for the
/// disassembly. (Round 98, dig 98-Q29.)
pub(crate) fn no_mc2_rebuild_victim_half() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_REBUILD_VICTIM_HALF").is_some())
}

/// `MGC_NO_MC2_OBJECTIVE_DEATH_LITERAL=1` restores the pre-dig
/// objective type-1/type-2 completion predicates, which treated a
/// FREED or REAP-FLAGGED slot as a death. See
/// [`Gen::mc2_font_type_3d`] and `World::objective_mc2`.
pub(crate) fn no_objective_death_literal() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_OBJECTIVE_DEATH_LITERAL").is_some())
}

/// `MGC_NO_MC1_ROW0_SHIM_119=1` drops shim byte {119} back to PLAIN,
/// restoring the pre-dig smoother that ran on the row-0 cells x=118
/// and x=119. Retail's `sub_360C0` (:42912-19) gates a row-0 cell on
/// `mapTerrainType_CC1E0[(u16)t - 257]` and `[(u16)t - 256]` — SIGNED
/// int indices, so for t < 257 both land in the sound-driver block
/// below the type plane. mc1l49 t=22193 (a castle perimeter epilogue,
/// `smooth_perimeter` cx=128 cy=0 half=10 thick=3, left strip
/// x=115..118) smooths (115,0)->134, (116,0)->127 and (117,0)->117
/// exactly as retail does, but retail leaves (118,0) at 103 where the
/// 3x3 average is 975/9 = 108. (117,0) smoothing pins shim {117, 118}
/// plain, so the byte (118,0) alone adds — {119}, i.e. VA CC156 inside
/// the weak `dword_CC154` — is the building-classed one. That single
/// byte is BOTH of the take's locally-rooted z heads: with (118,0)
/// wrongly raised to 108 the same epilogue's (117,1) averages
/// 1057/9 = 117 instead of 1052/9 = 116 and (118,1) 874/8 = 109
/// instead of 868/8 = 108, and the (10,6) standing fire riding
/// `ground + f46` at (118.05, 1.69) reads z 3499 where retail reads
/// 3491 (t=22193 slot 947; t=22291 slot 900 is the same byte one
/// eruption cycle later).
///
/// ⚠ CORRECTED (round 116, the z-ranking dig): this comment used to
/// claim the take's other 25 z-only heads were all pair-CLEAN. They
/// are not — **12 of the 26 are pair-DIRTY**, and 13 of those closed
/// on a single unrelated law ([`castle_token_index`]). The
/// `--classify` LOCAL/INHERITED tag is ALSO unreliable for a late z
/// head, because it grades against the free-run world whose terrain
/// has drifted for tens of thousands of ticks: 31887 and 32457 tag
/// LOCAL yet their pairs are clean. **For a z head, trust
/// `verify-deltas` on a tight slice, not the tag.**
/// See [`Gen::OOB_TYPE_SHIM`] and [`Gen::smooth_cell`].
fn mc1_no_row0_shim_119() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ROW0_SHIM_119").is_some())
}

/// `MGC_NO_MC1_ROW0_SHIM_47_55_76_85=1` restores the pre-dig shim table,
/// which had shim bytes {47, 55, 76, 85} plain. See
/// [`Gen::OOB_TYPE_SHIM`].
fn mc1_no_row0_shim_47_55_76_85() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ROW0_SHIM_47_55_76_85").is_some())
}

/// `MGC_NO_MC1_ROW0_SHIM_16_23_24_31_225=1` drops shim bytes {16, 23,
/// 24, 31, 225} back to PLAIN, restoring the pre-dig smoother on the
/// row-0 cells x=15..16, 22..24, 30..31 and 224..225. See
/// [`Gen::OOB_TYPE_SHIM`] and [`Gen::smooth_cell`]. Round 152, dig
/// w152g — four witnesses, every byte FORCED by a neighbour that
/// smooths bit-exactly in the same epilogue:
///   {16} — mc1l13 t=753, the same-tick collapse of villages 59
///     (row 39, x 1..16, y 253..5) and 60 (row 34, x 10..30,
///     y 236..0), whose footprints share x 10..16 across the y seam.
///     Retail smooths (14,0) 99 -> 127 (pins {14, 15} plain) but
///     leaves (15,0) and (16,0) at their pre-value 123 in BOTH passes
///     where the port averaged them to 128/127; the row-255/1/2
///     neighbours then re-average one unit high — eleven height cells
///     that `replay` never grades, standing under all four of the
///     take's z heads (builder 75 at (16,1) t=3274, (9,1) 629 t=9826,
///     emu 430 t=14844, archer 996 t=20166).
///   {23, 24} — mc1l13 t=13049, village 367's collapse (row 26,
///     x 20..28, y 246..0). (21,0) smooths 1115/9 = 123 and (25,0)
///     1111/9 = 123 exactly as retail (pins {21, 22, 25, 26} plain);
///     retail holds (22,0)/(23,0)/(24,0) at 124 where every 3x3 sum
///     is 1113 (= 123), so {23} and {24} are both building-classed.
///   {31} — mc1l13 t=17175, castle 821's construction epilogue
///     (`smooth_perimeter`, f70 51 -> 52). (29,0) smooths 122 -> 121 in
///     both columns (pins {29, 30} plain); retail holds (30,0) at 123
///     where the port wrote 122, cascading into (30,1)/(31,1).
///   {225} — mc1l7 t=1584, the castle level-down fake collapse in
///     pool slot 0 (row 2, x 208..228, y 254..18). (223,0) smooths
///     73 -> 75 in both columns (pins {223, 224} plain); retail holds
///     (224,0) at 90 and (225,0) at 84 where the port averaged 77/75,
///     and the row-1 cascade under it is the balloon 489's z 2816 vs
///     2817 at t=3005.
/// With all five, mc1l13 (32,717 ticks) and mc1l7 (26,432 ticks) run
/// bit-exact to END with `MGC_PLANE_DIFF` hdiff 0 on every tick.
fn mc1_no_row0_shim_16_23_24_31_225() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ROW0_SHIM_16_23_24_31_225").is_some())
}

/// `MGC_NO_MC1_ROW0_SHIM_109=1` drops shim byte {109} back to PLAIN,
/// restoring the pre-dig smoother on the row-0 cells x=108 and x=109.
/// Round 155, dig w155c. ⭐ **THE FIRST SHIM BYTE WITH AN IDENTITY.**
/// The gate's stored operands are data-object-relative (CARPET.EXE
/// `sub_360C0` file 0x4E8E9 `mov 0x3c0df(%ebx),%dl` = IDA CC0DF), so
/// shim index i is IDA `CC0DF + i`, and {109} is the low byte of
/// `dword_CC14C` — the sound card's I/O PORT that the digital-driver
/// init parses from the sound config (`fscanf(file, "%s = %s %x %d %d",
/// …, &dword_CC14C, &dword_CC150, &dword_CC1BC)`, then `dword_CC134 =
/// dword_CC14C`). Port 0x220 ⇒ low byte 0x20 = 32, building-classed
/// (6..=0x22). Its COPY `dword_CC134` is shim {85}, pinned building
/// by mc1l37 + mc1l49 in round 119 with no identity — the same value
/// read through a second address. Static for the recording machine.
/// Witnesses, mc1l34 (each on a height plane hdiff 0 entering the
/// tick, `MGC_PLANE_DIFF`): t=7467, dwelling 642's construction
/// epilogue (f70 51 -> 52, `smooth_perimeter`): (107,0) and (110,0)
/// smooth bit-exactly (pin {107, 108} and {110, 111} plain) while
/// retail holds (108,0) 80 and (109,0) 81 where the port averaged
/// 79/80 — {109} forced — the seed of the t=8370 `(5,12)slot481:z`
/// head; t=13901/14018 hold (109,0) again; t=34929 holds (108,0) at
/// 84 (the port wrote 78) — the seed of the t=36040 `(5,4)slot842:z`
/// head.
fn mc1_no_row0_shim_109() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ROW0_SHIM_109").is_some())
}

/// `MGC_ROW0_SHIM_CLASS="117=build,225=plain"` — THE ROW-0 SHIM
/// INSTRUMENT (round 162, dig w162c). Overrides the class of any
/// [`Gen::OOB_TYPE_SHIM`] entry for one run: `plain` returns 0,
/// `build` returns a representative 22.
///
/// This family has outgrown one env var per byte. The shim models
/// retail's OOB read below the type plane (`sub_360C0`'s quad gate),
/// and round 155 identified the bytes as the SOUND DRIVER's pointer
/// block — {109} the card I/O port, {101} `dword_CC144` the last
/// digital-sample handle, {117} `dword_CC154` and {225}
/// `dword_CC1C0` the loaded sound table's BEGIN and END heap
/// pointers. The heap pointers are 16-aligned malloc results, so
/// their low byte is a per-SESSION value that is building-classed
/// (6..=0x22) only when the load happened to land on 0x10 or 0x20 —
/// which is why takes CONTRADICT each other on them and the roster
/// carries per-take deviation rows (`mc1l34-row0-shim-*`,
/// `mc1l43-row0-shim-*`, `mc1hwl*-row0-shim-*`). Attributing the
/// next one means flipping one index for one take and measuring;
/// this is that instrument.
fn row0_shim_override(i: usize) -> Option<u8> {
    static V: std::sync::OnceLock<Vec<(usize, u8)>> = std::sync::OnceLock::new();
    let v = V.get_or_init(|| {
        std::env::var("MGC_ROW0_SHIM_CLASS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(|e| {
                        let (k, c) = e.split_once('=')?;
                        let k = k.trim().parse().ok()?;
                        match c.trim() {
                            "plain" => Some((k, 0u8)),
                            "build" => Some((k, 22u8)),
                            _ => None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    });
    v.iter().find(|(k, _)| *k == i).map(|(_, c)| *c)
}

/// `MGC_NO_MC1_ROW0_SHIM_227=1` drops shim byte {227} back to PLAIN,
/// restoring the pre-dig smoother on the row-0 cells x=226 and x=227.
/// See [`Gen::OOB_TYPE_SHIM`].
fn mc1_no_row0_shim_227() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ROW0_SHIM_227").is_some())
}

/// `MGC_NO_MC1_ROW0_SHIM_32_39_40_48_89=1` drops shim bytes
/// {32, 39, 40, 48, 89} back to PLAIN, restoring the pre-dig smoother
/// on the row-0 cells x=31/32, 38/39, 39/40, 47/48 and 88/89.
/// See [`Gen::OOB_TYPE_SHIM`].
fn mc1_no_row0_shim_32_39_40_48_89() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ROW0_SHIM_32_39_40_48_89").is_some())
}

/// ⭐⭐⭐ **A GROUNDED ROLL LEAVES A SLOPE KICK IN THE GLOBAL SCRATCH
/// AXIS, NOT A POSITION.** `sub_58030` (shipped `NETHERW.EXE` file
/// **0x7C830** = VA 0x58030) is the terrain forward-difference helper:
/// it takes `(src position, dst axis)` and writes ONLY `dst->x` and
/// `dst->y` (`mov %cx,(%eax)` at 0x7C853/0x7C870/0x7C899/0x7C8C2 and
/// `mov %..,0x2(%eax)` at 0x7C85A/0x7C885/0x7C8AE/0x7C8D3) with
/// `x = h00 - h10 + h01 - h11`, `y = h00 + h10 - h01 - h11` over the
/// record's 2x2 height quad — `dst->z` (offset 4) is NEVER touched.
///
/// ALL THREE of its callers in the shipped EXE pass the ENGINE'S ONE
/// GLOBAL SCRATCH AXIS as `dst` (`push $0x1b398; lea 0x4c(%ebx),%eax;
/// push %eax; call 0x7c830`):
///   * file 0x56FEA (VA 0x327EA) — `sub_32600`, the (10,16) volcano
///     BOULDER's resting roll (EF:23809-20) → [`Gen::mc2_boulder16_tick`]
///   * file 0x5A6AC (VA 0x35EAC) — `TransformArcherToMana_35940`, the
///     (10,39) MANA BALL's grounded roll (EF:26271)
///   * file 0x5ADED (VA 0x365ED) — `sub_35FB0`, the (10,57) FOOL'S
///     sphere twin (same arm)
///
/// so after any grounded ball/sphere/boulder tick retail's
/// `predictedAxis_EB398ar` holds TWO SMALL HEIGHT DELTAS — **(0,0) on
/// flat ground** — and not the position the last mover committed. The
/// port computed the identical difference into locals `sx`/`sy` and
/// never touched [`Gen::mc2_pred_axis`], so its model of the global
/// stayed at the last `move_relink`.
///
/// The one reader of that global is the m21 walker's water-contact
/// (10,5) splash (file 0x4AFB9, see [`Gen::m21_jump`]). MEASURED on
/// mc2l13 t=24079: slot 376, a (10,39) sphere resting at
/// (32896, 44928, 0) on flat ground, is the LAST grounded roll before
/// slot 380's wade splash — retail births the splash at (0,0) with
/// `z = ground_z(0,0) = 5536`, the port at (50688, 57344, 2176), the
/// position of the last record `move_relink` moved (slot 344).
///
/// `MGC_NO_MC2_BALL_SLOPE_PRED_AXIS=1` restores the pre-dig
/// behaviour (slope kick computed into locals only).
pub(crate) fn mc2_ball_slope_pred_axis() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_BALL_SLOPE_PRED_AXIS").is_none())
}

/// `MGC_NO_BEAM_UNLINK=1` restores the pre-dig lightning beam, which
/// marched a MAP-LINKED record. See the citation inside
/// [`Gen::move_relink`].
pub(crate) fn no_beam_unlink() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_BEAM_UNLINK").is_some())
}

/// `MGC_NO_MC1_ACQ_LIST_FULL=1` restores the pre-dig jar grant, which
/// enforced only retail's `v25` half of the acquisition scan (the
/// list already holds a class-12 record of this spell) and ignored
/// the `v24 == -1` half — a FULL 24-seat `wizext+532` list. Retail
/// refuses the whole grant on either (`sub_55A40_55F70` :64818-41),
/// because `v24` is the seat the pool slot is written into
/// (`+532 + 4*v24`, :64850) and the index stamped into the left hand
/// (`+940 = v24`, :64851). WITNESS mc1l49 t=18525: wizard 0's list is
/// 24/24 occupied — seat 20 a ZOMBIE (pool slot 235, `class64 = 0`)
/// so `+676[20]` is 0 and spell 20 reads unowned — and retail's poll
/// of the `(12,20)` jar at slot 693 holds `flags 4` / `+70 = 61`
/// where the port granted. See [`World::class12_tick`] and
/// [`World::try_pickup`], the two call paths of the same retail
/// function.
pub(crate) fn no_mc1_acq_list_full() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ACQ_LIST_FULL").is_some())
}

/// `MGC_NO_MC1_NATIVE_JAR_KNOWN_STAMP=1` restores the pre-dig NATIVE
/// jar poll, which had no "already known" stamp (round 154, w154k).
/// Retail's `sub_55A40_55F70` (:64784-97) sets bit 0 of `+16` on a
/// ground jar whose spell the local human already owns, on every
/// poll tick (`+63 & 3 == 0`, :64772-77), with no distance test —
/// the sticky marker the painter's owned-jar draw filter reads. The
/// strict-retail arm of [`World::class12_tick`] carried it; the
/// native arm ran only the overlap pickup, so a level whose authored
/// jars duplicate the carried book held them at `flags 4` where
/// retail reads 5 from the first poll on (`init-check` mc1l10 slot 2,
/// mc1l13 99, mc1l20 28/29, mc1l21 326/437, mc1hwl2 392 — one row
/// per such jar). CARPET.EXE VA 0x55AAB-B7 the phase gate, 0x55ACD-
/// 0x55B14 the walk (`cmpb $0,0x41(%edi)`; `cmpl $0,0xc(%edi)`; the
/// local-player word; `test $0x1,%bh`; `cmpw $0x0,0x2a4(%edx,%eax,2)`;
/// `or $0x1,%cl; mov %cl,0x10(%ebp)`).
pub(crate) fn no_mc1_native_jar_known_stamp() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_NATIVE_JAR_KNOWN_STAMP").is_some())
}

/// `MGC_NO_MC1_REGRANT_ZERO_ENTRY=1` restores the pre-dig respawn
/// re-grant, which SKIPPED a dead-form acquisition entry of 0 unless
/// the fireball bank flag was still up — an invented disambiguator.
/// Retail's respawn walk (`sub_45C10_45F50`, remc1
/// `sub_main.cpp:54893-922`) tests the entry ONLY for `< 0`:
///
/// ```c
/// if (var_532[v9x] < 0) var_532[v9x] = 0;
/// else { v11x = sub_373F0_377B0(&v2x->+72, 12, var_532[v9x]);
///        if (v11x) { ...register... } else var_532[v9x] = 0; }
/// ```
///
/// so a banked 0 is FIREBALL, not "empty", and mints a token like any
/// other model — even when index 0 already minted one. WITNESS
/// mc1l49 t=26654, the human's respawn: `wizext+532` reads
/// `[0,3,2,16,1,14,4,12,6,9,7,8,15,18,17,19,13,5,11,10,0,21,22,23]`
/// (seat 20's spell was lost to a slot recycle, so the death walk
/// banked its recycled record's `model65` = 0), and retail lays 24
/// tokens — slot 345 `(12,0)` for seat 20 — where the port laid 23
/// and shifted every later member up a slot. See
/// [`World::death_regrant`].
pub(crate) fn no_mc1_regrant_zero_entry() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_REGRANT_ZERO_ENTRY").is_some())
}

/// `MGC_NO_MC1_CASTLE_UPGRADE_LATCH=1` restores the pre-dig level-up
/// commit, which consumed the upgrade-request bit (`+16 & 0x40`) at the
/// TOP of sub-state 0, before the painter spawn. Retail's `+48`
/// machine (`sub_46F10_47250` case 0, :56053-72) leaves exactly three
/// things outside the guard — the house pre-clear, the space gate and
/// the one-time team stamp — and clears 0x40 on only TWO paths: the
/// space REJECT (:56070 `+16 = v2 & 0xBF`, with `+48 = 2`) and, inside
/// `sub_47960_47CA0`'s `if (v1)` (:56475), the successful commit. `v1`
/// is `sub_3B7B0_3BB30(+150)` (:47567-86), which is a bare
/// `NewEvent_372C0_37680()` — it returns 0 only when the POOL IS
/// EXHAUSTED, and then the whole commit is skipped and case 0 retries
/// next tick with the request bit STILL SET.
///
/// WITNESS mc1l49 castle 940 `(3,2)`, owner 750, level 4: retail arms
/// the bit at t=55127 (`+70 4→5`, `flags 14→78`) and holds `flags 78`
/// through t=55132 with `+26 = 4` and `+48 = 0` — six ticks of a full
/// pool — then commits at t=55133 (`flags 78→14`, `+26 4→5`, `+48 0→4`,
/// `+136 80000→160000`). The port cleared the bit on the first state-5
/// tick, so the pairs anchored at t=55127..55131 all read `flags`
/// retail 78 / port 14.
pub(crate) fn no_mc1_castle_upgrade_latch() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CASTLE_UPGRADE_LATCH").is_some())
}

/// `MGC_NO_MC1_CASTLE_LEVELER_GROUND=1` restores the port's stale z on
/// the transform machine's case 5 (painter done → spawn the m41
/// leveler). `sub_46F10_47250` case 5 (:56088-92) is NOT a pure action
/// case: it releases/pins the owner token (`sub_46D20(a1, 1)`), then
/// snaps the castle to the ground — `*(a1+76) = sub_11F50(a1+72)` —
/// and only THEN calls `sub_47080_473C0`. CARPET.EXE VA 0x46FE6:
/// `6a 01 53 e8 <sub_46D20>` / `56 e8 <sub_11F50> ; 66 89 43 4c` /
/// `53 e8 <sub_47080>` (sub_47080's only caller, VA 0x46FFF). The port
/// refreshed z only in the pure waits 1/4/6, so the tick the painter
/// handed back 5 kept the pre-paint tower-top z for one pair.
///
/// WITNESSES — `(3,2) +70=5`, `+48 → 6`, retail z steps, port holds:
/// mc1l45 t=8522 slot 599 (5664 → 5888), t=9501 slot 573, t=34903
/// slot 650; mc1l47 t=20445 slot 430, t=26846 slot 998 (`+48 4 → 6`:
/// the painter finished earlier in the same walk); mc1l43 t=7669 slot
/// 946.
pub(crate) fn no_mc1_castle_leveler_ground() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CASTLE_LEVELER_GROUND").is_some())
}

/// `MGC_NO_MC1_DIG_ABORT_LATCH=1` restores the pre-dig ring walk, which
/// swallowed [`Gen::dig_cell`]'s clamp latch on every caller that did
/// not pass `protect`. `sub_40D30_41070` (:51711-16, CARPET.EXE
/// `0x595a9` `test al,al / je next-cell`) aborts the disc on ANY
/// nonzero cell return — there is no `a5` test on that path; the
/// `a4`/protect test lives INSIDE `sub_40A10_40D50` on its own
/// `return 1` (:51645, EXE `0x59269`). The other `return 1` is the
/// SATURATION LATCH: `v8 = 1` when the height clamps at 200 or 0 and
/// `!a1 && !a2` — the literal map origin, tested on the FULL 16-bit
/// args while the cell index takes only their low bytes (:51634/51641,
/// EXE `0x59242`/`0x59263`). The port's `&& protect` made that latch
/// unreachable for the two non-protect diggers, and the stale doc
/// comment on `dig_cell` called it "dead in practice".
///
/// WITNESS mc1l49 t=33302 slot 796, a `(10,9)` hill/crater sitting AT
/// tile (0,0) (`x = y = 0`, `+26 7→8`, `actLife 10→9`, `+80 = 768`):
/// its ring 0..1 covers the origin, retail's dig clamps that cell to 0,
/// `sub_25470` (:28302-25) takes the finish arm — the `-40` levelling
/// dig, the `(10,18)` child stamped with `+24`, and `sub_41E80`'s
/// `flags |= 0x400` — while the port held `flags 0`. Same head at
/// t=41372 slot 776, t=43542 slot 996, t=32259 slot 183, t=32645 slot
/// 676 and t=43705 slot 452.
pub(crate) fn no_mc1_dig_abort_latch() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_DIG_ABORT_LATCH").is_some())
}

/// `MGC_NO_MC1_RIVAL_LEARN_SEAT=1` restores the pre-dig rival learn
/// expiry, which registered the freshly conjured manifestation into
/// the rival's book unconditionally. Retail conjures the record FIRST
/// (`off_987DE[spell]`, :19415-19) and only then walks that wizard's
/// `wizext+532` for the first EMPTY seat (:19421-25); when all 24 are
/// taken the `v6 >= 24` arm jumps to the loop's continue, past ALL
/// THREE registration writes — `+16 |= 1` (:19428), `+42 = the
/// wizard's slot` (:19429) and `+532[v6]` (:19430) — so the token is
/// born ORPHANED: `flags 4`, `+42 0`, in nobody's book, never polled
/// (`+70 = 3*spell` is the spell's own manifest dispatch row, not
/// `sub_56250`/`sub_56260`) and never freed (`actLife 0`). Same
/// `v24 == -1` refusal as [`no_mc1_acq_list_full`], third call path.
/// WITNESSES mc1l49 t=24927 slot 230 and t=25321 slot 123, both
/// `(12,16)` `+70 = 48`: retail `flags 4` / `+42 0` against the port's
/// `flags 5` / `+42 594`. See [`crate::mc1::rivals`]'s
/// `rival_learn_tick`.
pub(crate) fn no_mc1_rival_learn_seat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_LEARN_SEAT").is_some())
}

/// `MGC_NO_MC1_RIVAL_MANA_WRAP_PUBLISH=1` restores the pre-dig
/// `.min(i32::MAX)` clamp on the rival purse mirror. Retail's `+140`
/// IS the purse word: the shield-quarter debit subtracts RAW
/// (`sub_46540_46880` :55703 — `a1x->var_u32_29935_140 -= v10` on the
/// RECORD's own purse word, no afford clamp) and a fatal tick wraps it
/// negative, which the death arm preserves (it returns before the
/// regen floor at :17990). WITNESS mc1l49 pair 53215→53216, rival
/// slot 750 `(3,1)` life −301: retail `+140` = 4294964965 (−2331),
/// the port published `i32::MAX`. See `mc1::rivals`'s
/// `rival_dispatch_tail`.
pub(crate) fn no_mc1_rival_mana_wrap_publish() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_MANA_WRAP_PUBLISH").is_some())
}

/// `MGC_NO_MC1_MANA_CENSUS_WRAP=1` restores the pre-dig SATURATING
/// accumulators in [`World::recompute_mana`]. Retail's census credit
/// `sub_48340_48680` (VA `0x48340`, `CARPET.EXE` file `0x60B38`) is a
/// pair of PLAIN 32-BIT ADDS and nothing else:
///
/// ```text
///   60b3d: 66 8b 93 90 00 00 00   mov  0x90(%ebx),%dx     ; +144 owner tag
///   60b46: 66 85 d2               test %dx,%dx
///   60b49: 74 3a                  je   0x60b85            ; no owner -> world only
///   ...    (edx*164 + [0x1e400] + 0x7463)                 ; &pool[owner]
///   60b79: 8b 93 8c 00 00 00      mov  0x8c(%ebx),%edx    ; src +140
///   60b7f: 01 90 88 00 00 00      add  %edx,0x88(%eax)    ; owner +136 += it
///   60b8b: 8b 9b 8c 00 00 00      mov  0x8c(%ebx),%ebx
///   60b91: 01 9a bc 00 00 00      add  %ebx,0xbc(%edx)    ; world total  += it
/// ```
///
/// No clamp, no signed saturation — the word WRAPS. The port used
/// `saturating_add` throughout the census and published the wizard
/// ceilings through `.min(i32::MAX)`, so every runaway accumulator
/// froze at `2147483647` instead of carrying retail's wrapped word.
///
/// WITNESS mc1l48-nodeath pair 28313→28314, the `(10,40)` grave at
/// slot 22 — the census's own documented runaway owner (nothing ever
/// re-baselines a non-wizard `+144` tag): retail `+136` =
/// −2146963936 (u32 2148003360, i.e. `i32::MAX + 519713`), the port
/// published `i32::MAX`. `f136` is the pair's ONLY dirty lane.
///
/// This is the `+136`/`+188` twin of
/// [`no_mc1_rival_mana_wrap_publish`], which already landed the same
/// raw-32-bit-copy law on the `+140` purse mirror.
pub(crate) fn no_mc1_mana_census_wrap() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_MANA_CENSUS_WRAP").is_some())
}

/// `MGC_NO_MC2_CENSUS_CREDIT_HOME=1` restores the pre-dig
/// `e.f136` home for the MC2 mana census's RUNAWAY OWNER CREDIT in
/// [`World::recompute_mana`].
///
/// MC2's census is `sub_60F00` (EF:62358 — the banner line
/// `//----- (00060F00)`), and the credit itself is `sub_61000`
/// (EF:62388), whose whole body is four instructions.
/// `NETHERW.EXE` file **0x85800** (VA 0x61000 + 0x24800):
///
/// ```text
///   85807: 66 8b 93 94 00 00 00   mov  0x94(%ebx),%dx   ; src @0x94 owner tag
///   85810: 66 85 d2               test %dx,%dx
///   85813: 74 16                  je   0x8582b          ; 0 -> world total only
///   85818: 8b 04 85 e4 a3 01 00   mov  0x1a3e4(,%eax,4),%eax  ; &Entities[tag]
///   8581f: 8b 93 90 00 00 00      mov  0x90(%ebx),%edx  ; src @0x90 mana
///   85825: 01 90 8c 00 00 00      add  %edx,0x8c(%eax)  ; ** owner @0x8C += it **
///   85837: 01 9a f6 00 00 00      add  %ebx,0xf6(%edx)  ; world total += it
/// ```
///
/// There is NO class, model, life or reap test on the target: the
/// tag word alone picks the record and the add lands on **@0x8C**.
/// Both `e8` call sites (file 0x857c0 / 0x857e7) are inside
/// `sub_60F00`, and the only reset walks the PLAYER TABLE
/// (file 0x8572b `mov %ecx,0x8c(%edx)`), so a tag naming a
/// non-wizard record accumulates forever — the MC2 twin of the MC1
/// `(10,40)` grave documented on [`no_mc1_mana_census_wrap`].
///
/// ⭐⭐⭐ THE PORT'S `f136` IS ONLY @0x8C FOR THE UNIFORM RECORD.
/// `import_ent_mc2`'s class-15 block moves the manifestation's homes
/// (`e.f136 = r.d88` @0x88, `e.max_life = r.mana_max` @0x8C) and the
/// m27 hydra keeps its bolt power in `f136` (@0x88) too. The census
/// credit was written once, against the uniform map, so on a class-15
/// target it added retail's @0x8C claim into the port's @0x88 lane —
/// the graded `mana_max` never moved, and `f136` (the class-15 upkeep
/// regen / the hydra's `m27_branch_bolt` power) was corrupted instead.
///
/// WITNESSES — two takes, both long contiguous runs, both a wizard's
/// dormant `(15,0)` manifestation named by loose `(10,39)` mana
/// spheres whose `@0x94` still points at a low slot:
/// * mc2l17 slot 4 (owner 147), t=15,062..15,255 — spheres 491 and
///   529 hold 1,100 each, so retail steps @0x8C by exactly +2,200 a
///   tick from the ctor's 100 (t=15,059) on; 194 first-divergence
///   rows, the take's single dominant lane.
/// * mc2l18 slot 3 (owner 483), t=18,536..18,777 — eight spheres
///   totalling 9,870, falling to 1,057 as they are collected, and the
///   run ENDS on the tick the last one goes; 242 rows.
///
/// The `mana_max` lane in both is the census credit, nothing else:
/// `sum(mana of every @0x94==slot record admitted by the class
/// filter)` reproduces retail's step to the unit on every tick.
pub(crate) fn no_mc2_census_credit_home() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CENSUS_CREDIT_HOME").is_some())
}

/// ⭐⭐ `MGC_NO_MC2_FLOOD_HUMAN_SPIN=1` restores the pre-dig CLOSE BAND
/// of the quake/flood's human visit (round 146, dig w146f), which
/// rolled the 1-in-7 kill and wrote nothing else.
///
/// `sub_3A200` (EF:29429, `NETHERW.EXE` file 0x5EA00) is the close-band
/// callback (`dist <= 32 || z - ref <= 96`), and on a class-3 model-0
/// victim it is TWO DIRECT WORD STORES before the roll:
///   `0x5ea49 80 7b 40 00`               `cmp byte [ebx+0x40],0` (model 0)
///   `0x5ea4f 8b 93 a4 00 00 00`         `mov edx,[ebx+0xa4]`   (Type_164 player)
///   `0x5ea55 66 c7 43 1e 00 02`         `mov word [ebx+0x1e],0x200` (record pitch)
///   `0x5ea5b 66 c7 82 57 01 00 00 00 02` `mov word [edx+0x157],0x200` (pitch_acc)
/// — the module doc called it "presentation-skipped"; it is the
/// graded `aim_pitch` / `pitch_f` lanes. It lands at the QUAKE's slot,
/// i.e. between the frame-head input pass (`PlayerEvents_51BB0`, which
/// takes `pitchDelta` off the UN-seized accumulator, `0x774b2 movsx
/// edx,[ecx+0x157]`) and `sub_5D530`'s `pitch += pitchDelta`
/// (0x81d7e) — the whirlwind crank's phase, one lane over. It also
/// sits ABOVE the mover's stop veto, so a vetoed tick keeps the 512.
///
/// WITNESS mc2l18 t=27,261..27,270: the human idles in a (10,67)'s
/// close band and retail's `pitch_acc` reads `512 + pitchDelta` on
/// every tick the band re-admits him — `-7 -> 512` (delta 0),
/// `284 -> 439` (-73), `327 -> 428` (-84), `319 -> 430`, `320 -> 430` —
/// and a plain filter step on the ticks between. Those were the take's
/// last five free-run heads.
pub(crate) fn no_mc2_flood_human_spin() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_FLOOD_HUMAN_SPIN").is_some())
}

/// ⭐⭐ `MGC_NO_MC2_FLOOD_MAIL_SEAT=1` restores the pre-dig pose the MC2
/// carpet dispatch hands its mailbox drain and its pre-move at-castle
/// probe: the UN-shoved pose (round 146, dig w146f).
///
/// `sub_5EFA0` (EF:61013, `NETHERW.EXE` file 0x837A0) arms the knock as
/// `moveBoost = amt/10` along `yaw_0x1E_30 = sub_581E0_maybe_tan2(
/// &Entities[src]->position, &a1x->position)` — the victim is the
/// wizard RECORD, read at the carpet's own dispatch. A (10,67) seated
/// BELOW the carpet has already written its shove into that record
/// (`sub_39B60`'s `CopyEntityPosition_57CF0`, 0x5e592), but the port
/// carries the shove on [`Gen::player_flood_pull`] and spends it only
/// inside the mover, AFTER the drain — so the bearing was taken off
/// the pre-shove position. (The walk hook already republishes the
/// shoved pose to every walker above the quake; the dispatch's own
/// readers were the call path it missed.)
///
/// WITNESS mc2l18 t=27,163: a (5,18) at slot 6, (8576, 31872), mails
/// the human 800. Shoved record (14418, 30963): `dy/dx·256 = 39`,
/// `512 − ATAN[39] = 463` = retail's recorded `knock_dir`; the port's
/// pre-shove (14519, 30932) gives index 40 → 462, and one unit of
/// bearing on the 80-unit impulse was the take's `pose.y` head
/// (retail 30976, port 30975).
pub(crate) fn no_mc2_flood_mail_seat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_FLOOD_MAIL_SEAT").is_some())
}

/// ⭐⭐⭐ `MGC_NO_MC2_FLOOD_HUMAN_SEAT=1` restores the pre-dig shape of
/// the quake/flood's HUMAN arm (round 146, dig w146e): ONE shove per
/// tick, run AFTER the whole 26x26 sweep, its horizontal leg on
/// [`Gen::player_knock`] (clamped, decaying) and only its z pull on
/// [`Gen::player_flood_pull`].
///
/// Retail has no human arm. `sub_39B60` (EF:29058, `NETHERW.EXE` file
/// 0x5E360) walks each cell's chain and the human carpet is an
/// ordinary linked class-3 model-0 record ON it, so he is shoved AT
/// HIS SEAT, and his `CopyEntityPosition_57CF0` (0x5e592) re-heads him
/// in the destination cell with the cursor re-read off HIM
/// (0x5e5e6 `mov 0x16(%ebx),%ax`). A shove that carries him into a
/// cell the sweep has not reached yet (rows are `HIBYTE` = y, outer)
/// SHOVES HIM AGAIN from the moved position — the pool-victim law
/// [`crate::mc2::flood`]'s `flood_chain_rewalk_law` already carries
/// (mc2l6-rsg t=26,325). Neither the knock register (retail never
/// writes `moveBoost_0x1E_30` here) nor a `(bearing, dist)` mailbox can
/// add two shoves, so the arm now accumulates the resolved delta.
///
/// WITNESS mc2l23 t=6,940→6,941, quake slot 101 at (28800, 12160),
/// refz 110: the human at (29841, 11775, 2279) sits in cell (116,45);
/// the first shove (v6 85, pull 22) lands y 11,803 = cell (116,46), a
/// row the sweep has not reached, and the second (v6 88, pull 22)
/// is exactly retail's (−83, +31, −22) over the port's single shove.
/// The knock's −4/tick residue after the well lets go was the take's
/// t=6,962 and t=6,964 heads.
/// ⭐ STAGE 3 — THE MC2 HUMAN'S `@0x20` (the death-spin target-yaw
/// word) LIVES ON HIS RECORD's `f32` once the (3,0) record exists
/// ([`Gen::human_roll`]). `MGC_NO_HUMAN_ROLL_ON_RECORD=1` keeps the
/// `human_roll_0x20` register as the home.
pub(crate) fn no_human_roll_on_record() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_HUMAN_ROLL_ON_RECORD").is_some())
}

pub(crate) fn no_mc2_flood_human_seat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_FLOOD_HUMAN_SEAT").is_some())
}

/// `MGC_NO_MC2_HOUSE_CLAIM_RAW_COLOR=1` restores the pre-dig
/// `177 + COLOR_ART[team]` flag row on a claimed `(10,45)` building.
///
/// `AddHouse0A_2D_38330`'s claim intake (remc2 EF:28063 / EF:28074,
/// resolve by banner `//----- (00038330)`) reads
/// `word_0x5A_90 += TransformPlayerColorIndex_616D0(...)`, and carries
/// remc2's own note "this is fixed bug from original game!!!!!!". The
/// SHIPPED EXE has no such call. `NETHERW.EXE` (file = VA + 0x24800),
/// both arms, forced then weak:
///   0x3846F `66 8b 43 68` / `8b 04 85 e4 a3 01 00` / `8b 80 a4 00 00 00`
///   0x38480 `66 8b 7b 5a`   mov di,[ebx+0x5a]
///   0x38484 `66 03 78 38`   add di,[eax+0x38]   ; RAW playerColorIndex
///   0x3848B `66 89 7b 5a`   mov [ebx+0x5a],di
///   0x384D6 `66 8b 53 5a` / 0x384DA `66 03 50 38` / 0x384E1 `66 89 53 5a`
/// A binary-wide `e8 rel32` scan for `0x616D0` finds six callers
/// (0x36A5F 0x5CB78 0x5FAE7 0x601B5 0x6216E 0x621CF) and none in
/// 0x38330..0x385C0; the castle's own latch (0x5FAE7, `sub_5FA70`) DOES
/// transform, so a house and a castle of the same rival fly DIFFERENT
/// bands for players 2/4/6/7 in retail. `playerColorIndex_0x38_56` is
/// the player's array index (EF:44062), i.e. [`World::owner_team`].
///
/// WITNESS mc2l22-new pair 2661→2662, slot 80 `(10,45)`: player 4
/// (ent 530) claims it (`player_ent 0 → 530`, rival mana −1000);
/// retail `f5a` 177 → **181** = 177 + 4, the port wrote 177 +
/// COLOR_ART[4] = **179**. It was the take's horizon head.
pub(crate) fn no_mc2_house_claim_raw_color() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_HOUSE_CLAIM_RAW_COLOR").is_some())
}

/// `MGC_NO_MC2_CASTLE_PURGE_BEFORE_EJECT=1` restores the pre-dig order
/// in which the castle's action-6 pass ran `sub_5FD00` (the ejector)
/// BEFORE the World-side half of `sub_605E0` (ladder re-price, pin,
/// level-0 rival token purge).
///
/// `sub_5FCA0_destroy_castle_level` (resolve by banner
/// `//----- (0005FCA0)`) is straight-line: `sub_605E0(a1x)`, park
/// action 4, `sub_5FD00(a1x)`, `sub_5FF50(a1x)`. `NETHERW.EXE`
/// (file = VA + 0x24800):
///   0x5FCB1 `e8 2a 09 00 00`  call 0x605E0   ; level off + book work
///   0x5FCBA `c6 43 45 04`     mov byte [ebx+0x45],4
///   0x5FCBE `e8 3d 00 00 00`  call 0x5FD00   ; eject
///   0x5FCC7 `e8 84 02 00 00`  call 0x5FF50   ; roster
/// and `sub_605E0` does the purge inline (0x60727 read / 0x6074C clear
/// of `SpellsEnabled[2]`, after 0x6073E `call 0x57F10` DisableEntityDrawing
/// stamps the token's reap bit). The port could not run the book work
/// inside `Gen::mc2_castle_tick`, so it drained it after the WHOLE
/// castle pass — after the eject. That is invisible unless the eject
/// takes its DRY arm: `sub_5FD00`'s zero-headroom path calls
/// `sub_49F90` (0x5FD80), whose first loop frees every `byte[1] & 4`
/// record — so retail's GC frees the just-purged token and the burst
/// re-uses its slot, where the port's GC saw a live token.
///
/// WITNESS mc2l22-new pair 3957→3958: rival 611's castle 637 falls to
/// level 0 on an EMPTY free stack; its `(15,2)` manifestation, slot
/// 614, is purged (`spell_ent[2] 614 → 0`), reaped by the GC and
/// re-minted as a `(10,39)` of 3825 owned by 611. The port kept 614 a
/// live `(15,2)` (29 rows on the slot).
pub(crate) fn no_mc2_castle_purge_before_eject() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_PURGE_BEFORE_EJECT").is_some())
}

/// `MGC_NO_MC2_EJECT_OWNER_LIVE_READ=1` restores the pre-dig owner
/// snapshot in [`Gen::mc2_castle_eject`]'s sphere loop.
///
/// `sub_5FD00` (resolve by banner `//----- (0005FD00)`) stores
/// `v4x->playerEntityIndex_0x94_148 = a1x->id_0x1A_26` INSIDE the loop,
/// and the shipped bytes re-read the castle record on every pass —
/// `NETHERW.EXE` (file = VA + 0x24800):
///   0x5FE2B `66 8b 43 1a`           mov ax,[ebx+0x1a]   ; castle->id
///   0x5FE33 `66 89 86 94 00 00 00`  mov [esi+0x94],ax   ; sphere owner
/// The port hoisted it into a pre-loop `own`. Equal, except when the
/// dry-arm GC (`sub_49F90` at 0x5FD80) has freed the castle ITSELF —
/// a level-0 castle carries `sub_605E0`'s 0x6076E DisableEntityDrawing
/// reap bit — and `NewEvent_4A050` hands the castle's own record to a
/// sphere: from then on `a1x` IS the sphere, its `id_0x1A_26` is its
/// own slot (memset + index stamp), and `a1x->mana -= v11` zeroes the
/// sphere it just filled. The port already reproduced the mana
/// round-trip; only the owner was stale.
///
/// WITNESS mc2l22-new pair 3957→3958 slot 637 (castle of rival 611,
/// level 1 → 0, empty free stack): the GC frees 614 and 637, the burst
/// of 7650 mints 614 (3825, owner 611) then 637 (mana 0, owner
/// **637**); the port wrote owner 611. (Needs
/// `no_mc2_castle_purge_before_eject`'s law for the 614 half.)
pub(crate) fn no_mc2_eject_owner_live_read() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_EJECT_OWNER_LIVE_READ").is_some())
}

/// `MGC_NO_MC2_DEPTH_REFUSAL_KEEPS_FLYER=1` restores the pre-dig
/// behaviour of [`Gen::mc2_proj_impact`], where a DEPTH-GATED effect
/// ctor's refusal (`free.len() < N` → `None`, `exhausted` unmoved) read
/// as a deliberate `None` and the flyer was despawned anyway.
///
/// `AddFireSpheres_4F2A0` (resolve by banner `//----- (0004F2A0)`),
/// `AddWind_4F040` (`//----- (0004F040)`) and the summon ring
/// `sub_51800` (`//----- (00051800)`) refuse BEFORE their first
/// `NewEvent_4A050` and return 0 exactly like a dry pool.
/// `NETHERW.EXE` (file = VA + 0x24800):
///   0x4F2AE `e8 5d b5 ff ff` call 0x4A810 / 0x4F2B3 `83 f8 1a` cmp eax,26
///   0x4F2B6 `0f 8c ..`       jl  → the `return 0` tail (0x4F419)
///   0x4F049 `e8 c2 b7 ff ff` call 0x4A810 / 0x4F04E `83 f8 0c` cmp eax,12
///   0x4F051 `0f 8c ..`       jl  → the `return 0` tail (0x4F1B7)
///   0x51821 `e8 ea 8f ff ff` call 0x4A810 / 0x51826 `39 d8` cmp eax,ebx
///   0x51828 `0f 8c ..`       jl  0x519E8   (ebx = byteindex_224)
/// and both flight workers keep the flyer alive on a null effect:
/// `sub_65C20` (the fireball, action 29 via `sub_65B50`)
///   0x65ED9 `e8 b2 42 fe ff` call 0x4A190 / 0x65EE4 `85 c0` test eax,eax
///   0x65EE6 `74 6e`          je  0x65F56 (skip the whole effect block)
///   `sub_65B50` 0x65B6C `85 c0` / 0x65B6E `0f 84 ..` je → past its
///   `DisableEntityDrawing04_57F10(a1x)`;
/// `sub_65820` (the generic core under the whirlwind's `sub_678E0`
/// and the summon ring's `sub_67800`) `if (!v11x) return 0;` before
/// its own Disable: 0x65A49 `call 0x4A190` / 0x65A53 `85 c0` /
/// 0x65A55 `0f 84 ..` je 0x65B1A.
///
/// WITNESS mc2l19-taketwo t=14482 (pair-import `--start 14481`): the
/// human's charged fireball (9,28) at slot 83 lands on terrain with
/// the free stack 12 deep; retail allocates nothing and keeps the ball
/// (life 7 held, flying on at 14483), the port raised `0x400`. The
/// reap freed 83 early, the next cast popped 83 instead of 241 and the
/// whole later (10,76)/(10,77) firestorm walk slid — the take's only
/// head at t=14578, whose preceding boundaries were capture-skipped.
pub(crate) fn no_mc2_depth_refusal_keeps_flyer() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_DEPTH_REFUSAL_KEEPS_FLYER").is_some())
}

/// `MGC_NO_MC2_M15_SCAN_ROSTER=1` restores the pre-dig LIVE POOL walk
/// (with `life >= 0` and `0x400` tests) in the `(5,15)` guard's acquire
/// scan, `Gen::m15_scan`.
///
/// Round 137's class-3-roster law (`MGC_NO_MC2_CLASS3_SCAN_ROSTER`)
/// named three retail sites and converted them; the guard brain
/// `sub_23C40` (resolve by banner `//----- (00023C40)`) is a FOURTH
/// walker of the same chain and kept its own pool copy. `NETHERW.EXE`
/// (file = VA + 0x24800):
///   0x23D67 `8b 35 a4 41 00 00` / 0x23D73 `8b b6 77 96 00 00`
///                                 load `dword_38519` (wizext+0x9677)
///   0x23D81 `66 8b 46 1a` / 0x23D85 `66 3b 43 1a`   id != own
///   0x23DAD `3b 7d f8`                              range
///   0x23DB2 `f6 46 0c 20`                           byte[0] & 0x20
/// — no life test and no reap test: a wizard that DIES earlier in the
/// same tick is still a member and still acquirable.
///
/// WITNESS mc2l22-new pair 4858→4859, slot 988 (the human's (5,15),
/// `target96` 503): rival 503 dies this tick (life 599 → −1001) at a
/// slot below the guard; retail's scan still engages it (action
/// 121 → 122, the engage-pose draw, `f5a` 0 → 1, speed 30 → 0), the
/// port's `act_life >= 0` rejected it.
pub(crate) fn no_mc2_m15_scan_roster() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M15_SCAN_ROSTER").is_some())
}

/// `MGC_NO_MC2_PICKUP_ONLY_STEAL_LOCK=1` restores the pre-dig
/// unconditional `word_0x36_54 = 64` on EVERY owned class-15 token
/// mint (the human's level-start book, the rival spawn book and the
/// rival respawn re-mint as well as the jar pickup).
///
/// `word_0x36_54` is the 64-tick RE-STEAL LOCK, and retail arms it in
/// exactly one place: `sub_68FF0`'s collect block
/// (`EventsFunctions.cpp:56076`, `a1x->word_0x36_54 = 64`), shipped
/// `NETHERW.EXE` 0x8D93C `66 c7 43 36 40 00` = `movw $0x40,0x36(%ebx)`
/// (VA 0x6913C). A scan of the whole shipped image for
/// `movw $imm16, 0x36(reg)` finds fourteen sites and **that is the only
/// one whose immediate is 64** (the others store 0, 1, 68, 96, 100,
/// 0xFFFF). The REIFY that hands a wizard its book — `sub_55AB0`
/// (`Level.cpp:1313`), the `IfSubtypeCallCreatingManaSphere_4A190` +
/// `parentId_0x28_40` + `byte[0] |= 1` + `SetSpell_6D5E0` quartet —
/// touches @0x36 not at all, so a level-start or respawn token is born
/// with the lock CLEAR.
///
/// The port ran the pickup's `mc2_adopt_manifestation` for the grant
/// too, and both rival mints copied the store, so every freshly minted
/// owned token carried a 64-tick lock retail does not have. It is
/// load-bearing, not cosmetic: the lock's only reader on a class-15
/// token is `World::mc2_spell_steal`'s gate (`sub_69300`, EF:55800),
/// so a native MC2 level opened with EVERY book — the human's and each
/// rival's — immune to an m26 wraith's spell theft for its first 64
/// ticks, and again for 64 ticks after any rival respawn. The %63 roll
/// is spent before the gate either way, so only the outcome moves.
///
/// WITNESSES — init-check record 0, class-15 `f36`: 1,625 rows over
/// 40/40 MC2 takes, retail 0 / port `64 − settle` (mc2l4 56, mc2l17 57,
/// mc2l0 55). LIVE (the graded run's raw shadow, `pair-wide`): 936 more
/// rows, e.g. mc2l12 t=2796 slots 14/18/39/54/61/62 — one rival's
/// respawn re-mint, retail 0 / port 64 on every spell at once.
pub(crate) fn no_mc2_pickup_only_steal_lock() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_PICKUP_ONLY_STEAL_LOCK").is_some())
}

/// `MGC_NO_MC2_SPELL_DEFAULTS_PATCH=1` restores the raw SPELLS.DAT
/// `maxManaLimit_A` column for row 23 (300000 / 350000 / 400000).
///
/// `SetDefaultSpells_5C0A0` (`Spells.cpp:109`) runs over the freshly
/// loaded SPELLS table and rewrites it. Three of its four arms are
/// no-ops against the shipped `SPELLS.DAT` (the `isEnabled_1`
/// derivation and the `fontType_0x1B` bit-0 pair for rows 0/7 reproduce
/// the bytes already on disk), but the `case 23:` arm does not:
///
///   0x808A9 `c7 43 0a 50 c3 00 00`  movl $50000,0x0a(%ebx)  tier 0
///   0x80930 `c7 43 24 70 11 01 00`  movl $70000,0x24(%ebx)  tier 1
///   0x80937 `c7 43 3e 90 5f 01 00`  movl $90000,0x3e(%ebx)  tier 2
///
/// (VA 0x5C129/0x5C130/0x5C137; row stride 80, `2 + 26*k + 8` = 0x0A /
/// 0x24 / 0x3E). `SetSpell_6D5E0` (`Level.cpp:1531`) copies that column
/// into `manaRegen_0x88_136` on every (re)tier, so the port's token for
/// spell 23 was born with the un-patched 300000.
///
/// WITNESSES — init-check class-15 `d88`, one row per take on 24/40
/// takes, always `(15,23)`, retail 50000 / port 300000 (mc2l4 slot 289;
/// mc2l17 slots 115 + two rival copies). LIVE: 120 more `(15,23) d88`
/// rows in the graded run's raw shadow, every one 50000 vs 300000.
pub fn no_mc2_spell_defaults_patch() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SPELL_DEFAULTS_PATCH").is_some())
}

/// `MGC_NO_MC2_NATIVE_HUMAN_START_POSE=1` restores the pre-dig
/// pose-less native human record (and with it the `human_pose`
/// register's `(0,0,0)` through the whole world constructor).
///
/// Retail's `sub_5C950` gives player 0 the same treatment as a rival:
/// the (3,0) carpet is placed at its `array_0x2362[color]` start marker
/// (`(mx << 8) | 128`, ground + 0x100) BEFORE `sub_55AB0` reifies the
/// carried book, and the reify mints each token at
/// `&Entities[playerIndex]->position_0x4C_76` (`Level.cpp:1319`) — the
/// carpet's own pose. The port's `mc2_spawn_human_record` only RESERVES
/// the slot, so `World::human_pose` was still the constructor's
/// `(0,0,0)` when `mc2_seed_default_spells` / `mc2_grant_start_book`
/// ran, and every level-start token was born — and tile-linked — at the
/// map origin.
///
/// WITNESSES — init-check class-15 `x`/`y`/`z`: 810 rows each over
/// 40/40 takes, one per human book spell (mc2l4 slots 266..291 retail
/// 64896/16000/2256 = marker (253,62) at ground+256, port 0/0/0;
/// mc2l0 slots 153..164 retail 19840/56960/5024). The same cell error
/// shows structurally as the 77 class-15 `map_head` rows (a stray port
/// chain head in cell 0) and the 3 `prev18` rows. NATIVE-ONLY: the
/// replay importer restores every token's pose from record 0, so there
/// is not one class-15 `x`/`y`/`z` row anywhere in the paired census.
pub(crate) fn no_mc2_native_human_start_pose() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_NATIVE_HUMAN_START_POSE").is_some())
}

/// `MGC_NO_MC2_TOKEN_CTOR_CLEARS_2C=1` restores the allocator's `100`
/// in a fresh class-15 token's `@0x2C`.
///
/// MC2's `NewEvent_4A050` (`Events.cpp:569`/:596, both arms) seeds
/// `subSpellIndex_0x2A_42 = 100` and leaves `word_0x2C_44` at the
/// memset's 0; the port's SHARED [`Gen::new_event`] writes `f44 = 100`,
/// and `f44` is the port's home for @0x2A on most classes but for
/// **@0x2C** on class 15 (`port_ent_lanes_mc2`'s `c == 15` arm). So a
/// token that is never adopted — an authored ground jar — publishes
/// retail's @0x2C as 100. `AddSpellXX_XX_51120` itself never writes
/// @0x2C, and `SetSpell_6D5E0`'s long arm does not either.
///
/// This is the class-15 slice of a wider allocator law (see the report:
/// the `(5,0) f2a` 85-row family on mc2l4 is its other half) and is
/// deliberately scoped to the token constructor, which is also the
/// missing prerequisite the `mc2_adopt_manifestation` doc comment names
/// for dropping its invented `f44 = 0` scrub.
///
/// WITNESSES — init-check class-15 `f2c`, 34 rows over 12 takes, retail
/// 0 / port 100 (mc2l5 8 rows, mc2l15 7). LIVE: the same shape in the
/// graded run's raw shadow, e.g. mc2l0 t=1007 slot 113 `(15,2)`.
pub(crate) fn no_mc2_token_ctor_clears_2c() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_TOKEN_CTOR_CLEARS_2C").is_some())
}

/// `MGC_NO_MC2_BALL_UPGRADE_AXIS_ABSENCE=1` restores the pre-dig
/// human castle-ball mint, which stamped `axis_0x9A_154x` on the
/// UPGRADE arm too. Citation at the call site in
/// [`World::cast_castle`].
pub(crate) fn no_mc2_ball_upgrade_axis_absence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_BALL_UPGRADE_AXIS_ABSENCE").is_some())
}

/// `MGC_NO_MC2_BALL_TOKEN_2A_HOME=1` restores the pre-dig human
/// castle-ball mint, which copied the class-15 token's `f44`
/// (`word_0x2C_44`) into the ball's `@0x2A` instead of its `f30`
/// (`subSpellIndex_0x2A_42`). Citation at the call site in
/// [`World::cast_castle`].
pub(crate) fn no_mc2_ball_token_2a_home() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_BALL_TOKEN_2A_HOME").is_some())
}

/// `MGC_NO_UPGRADE_TOKEN_FOV=1` restores the pre-dig `(10,43)` castle
/// UPGRADE TOKEN extents, where the ctor transcribed only two of the
/// three `SetEntityShiftRot` lines and `array_0x52_82.fov` kept the
/// sprite-41 row's value. Citation at the call site in
/// [`Gen::spawn_creator`]. Shared MC1 + MC2 path — both retail ctors
/// pass `(512, 512)`.
pub(crate) fn no_upgrade_token_fov() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_UPGRADE_TOKEN_FOV").is_some())
}

/// `MGC_NO_UPGRADE_TOKEN_ANIM=1` restores the pre-dig `(10,43)` castle
/// UPGRADE TOKEN tick, which never stepped `animationFrame_0x5C_92`.
/// Citation at the call site in [`Gen::tick_upgrade_token`]. Shared
/// MC1 + MC2 path — `sub_389F0` and `sub_293D0` both open their live
/// arm with the step.
pub(crate) fn no_upgrade_token_anim() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_UPGRADE_TOKEN_ANIM").is_some())
}

/// `MGC_NO_MC2_JAR_HAND_HINT_SEAT=1` drops the class-15 import of the
/// STOLEN-JAR HAND HINT `word_0x4A_74` (@0x4A) and the lane that
/// publishes it — i.e. it reverts this law.
///
/// ⭐ A FIELD-HOMING GAP, NOT A SIM LAW. `sub_69300` (the m26 wraith's
/// spell steal, EF:55814-24) remembers which hand the jar was yanked
/// out of in the jar's own `word_0x4A_74`: 1 = right, 2 = left. The
/// re-collect, `sub_68FF0`, reads it back BEFORE the quick-slot
/// fallback and puts the spell back in the SAME hand. Shipped bytes,
/// `NETHERW.EXE` file **0x8D96B** (VA 0x68FF0 + 0x24800 region):
/// ```text
///   8d96b: 66 8b 43 4a     mov    0x4a(%ebx),%ax     ; the hint
///   8d96f: 66 85 c0        test   %ax,%ax
///   8d972: 75 24           jne    0x8d998            ; hint != 0
///   8d974: 8b 86 a4..      mov    0xa4(%esi),%eax    ; …else the fallback
///   8d97a: 0f bf 90 51 04  movswl 0x451(%eax),%edx   ;   SpellIndexLeft
///   8d981: 83 fa ff        cmp    $-1,%edx
///   8d984: 74 0c           je     0x8d992            ;   Left  == -1 → LEFT
///   8d986: 0f bf 80 53 04  movswl 0x453(%eax),%eax   ;   SpellIndexRight
///   8d98d: 83 f8 ff        cmp    $-1,%eax
///   8d990: 74 16           je     0x8d9a8            ;   Right == -1 → RIGHT
///   8d992: c6 45 fc 01     movb   $0x1,-0x4(%ebp)    ;   both taken → LEFT
///   8d998: 66 3d 02 00     cmp    $0x2,%ax           ; hint == 2 → LEFT
///   8d99e: c6 45 fc 01     movb   $0x1,-0x4(%ebp)
///   8d9a2: 66 c7 43 4a 00  movw   $0x0,0x4a(%ebx)    ; …and clear it
/// ```
/// The port models the hint in `Ent::f36` ([`Gen::mc2_spell_steal`] /
/// [`World::mc2_adopt_manifestation`]) and gets it right natively — but
/// **`import_ent_mc2`'s class-15 arm never seated it**, and
/// `port_ent_lanes_mc2` published class-15 `sv_timer` (@0x4A) from the
/// StageVar-hold side table, which a jar is never a member of, so the
/// lane read `None` and the raw shadow could not see the hole either.
/// An imported mid-theft jar therefore re-equipped by the quick-slot
/// fallback instead of the remembered hand.
///
/// WITNESS — mc2l5 pair mode (`verify-deltas`, `MGC_RAW_SHADOW=1`),
/// WIZEXT lane `hand_left`/`hand_right`: t=2018 and t=2714, retail
/// `hand_left -1 / hand_right 1`, port `hand_left 1 / hand_right -1`.
/// Slot 79 is a (15,1) the wraith stole (`action45` 78 at t=2005) and
/// the human walks back into at t=2019; its @0x4A is **1** the whole
/// way down, and retail's own changelog at t=2019 is
/// `hand_right -1 -> 1, spell_ent[1] 0 -> 79` with @0x4A 1 -> 0.
pub(crate) fn no_mc2_jar_hand_hint_seat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_JAR_HAND_HINT_SEAT").is_some())
}

/// `MGC_NO_MC2_ALLOC_2C_SEED=1` restores the shared allocator's `100`
/// in the `word_0x2C_44` of every MC2 record whose port `f44` is the
/// `@0x2C` home — i.e. it reverts this law.
///
/// ⭐⭐⭐ THE ALLOCATOR HALF OF THE @0x2A / @0x2C FAMILY. The two
/// shipped `NewEvent`s seed **different words**, and the port has one
/// field for both. Disassembled, not decompiled:
///
/// MC1 `NewEvent_372C0` (VA 0x372C0, `CARPET.EXE` file **0x4FAB8**),
/// stride 0xA4 = 164, class @+64, model @+65:
/// ```text
///   4fb5b: 68 a4 00 00 00     push $0xa4          ; memset(rec, 0, 164)
///   4fb84: c7 43 08 2c 01..   movl $0x12c,0x8     ; +8   max_life = 300
///   4fb8b: c7 43 10 08 00..   movl $0x8,0x10      ; +16  flags    = 8
///   4fb92: 66 c7 43 7e 10 00  movw $0x10,0x7e     ; +126           = 16
///   4fb98: 66 c7 43 2c 64 00  movw $0x64,0x2c     ; +44 (@0x2C)    = 100
///   4fb9e: 66 89 43 18        mov  %ax,0x18       ; +24  id24 = slot
///   4fba5: c6 43 42 ff        movb $0xff,0x42     ; +66            = 0xFF
///   4fba9: c6 43 43 ff        movb $0xff,0x43     ; +67            = 0xFF
///   4fbad: c7 83 9c.. 38 8f.. movl $0x8f38,0x9c   ; +156 row base (index 0)
///   4fbb7: c7 83 a0.. 30 73.. movl $0x27330,0xa0  ; +160 guest ptr (unmodelled)
///   4fbc1: c6 43 44 0a        movb $0xa,0x44      ; +68            = 10
///   4fbc5: c6 43 3a fa        movb $0xfa,0x3a     ; +58            = 0xFA
///   4fbcc: 89 4b 04           mov  %ecx,0x4       ; +4  rand = slot + global (DWORD)
///   4fbd2: 88 43 3f           mov  %al,0x3f       ; +63            = slot (byte)
/// ```
/// MC2 `NewEvent_4A050` (VA 0x4A050, `NETHERW.EXE` file **0x6E850**),
/// stride 0xA8 = 168, class @+63, model @+64 — ⚠ the free arm and the
/// sacrifice arm MERGE at 0x6E900, so the shipped code has ONE defaults
/// block, not the decompile's two (`Events.cpp:569` / :596):
/// ```text
///   6e900: 68 a8 00 00 00     push $0xa8          ; memset(rec, 0, 168)
///   6e929: c7 43 04 2c 01..   movl $0x12c,0x4     ; +4    max_life = 300
///   6e930: c7 43 0c 08 00..   movl $0x8,0xc       ; +12   flags    = 8
///   6e937: 66 c7 83 82.. 10.. movw $0x10,0x82     ; +130           = 16
///   6e940: 66 c7 43 2a 64 00  movw $0x64,0x2a     ; +42 (@0x2A)    = 100
///   6e946: 66 89 43 1a        mov  %ax,0x1a       ; +26   id24 = slot
///   6e94d: c6 43 41 ff        movb $0xff,0x41     ; +65            = 0xFF
///   6e951: c6 43 42 ff        movb $0xff,0x42     ; +66            = 0xFF
///   6e955: c7 83 a0.. ac 83.. movl $0x83ac,0xa0   ; +160 row base (ABSOLUTE 59)
///   6e95f: c7 83 a4.. b0 42.. movl $0x242b0,0xa4  ; +164 guest ptr (unmodelled)
///   6e969: c6 43 43 0a        movb $0xa,0x43      ; +67            = 10
///   6e970: c6 43 39 fa        movb $0xfa,0x39     ; +57            = 0xFA
///   6e97b: 66 89 53 14        mov  %dx,0x14       ; +20   rand = (slot + global) as WORD
///   6e976: 88 43 3e           mov  %al,0x3e       ; +62            = slot (byte)
/// ```
/// Store for store the two lists are the SAME defaults at each layout's
/// own offsets, and [`Gen::new_event`] reproduces every one of them
/// (the two `+160`/`+164` guest pointers are deliberately unmodelled —
/// `port_ent_lanes_mc2` publishes `ptr_a4` as `None` and `verify.rs`
/// never compares MC1's `owner_ptr`). **Exactly one store differs in
/// its HOME**: MC1 seeds `+44 = 0x2C`, MC2 seeds `+42 = 0x2A` and
/// leaves `@0x2C` at the memset's 0.
///
/// The port names entity fields by MC1's decimal offsets, so `f44` IS
/// MC1's `+44` and `e.f44 = 100` is exactly right for MC1 — ⛔ this law
/// must never touch the MC1 arm, and does not. On MC2 `f44` is the
/// `subSpellIndex_0x2A_42` home for MOST models but the
/// `word_0x2C_44` home for an enumerated set (`port_ent_lanes_mc2`'s
/// `ramp2c` + class 15 + `c10_2c_in_f44` + the (10,79) piece + the
/// (10,89) cave-in), and for
/// those the allocator's 100 lands in retail's ZERO word.
///
/// Every MC2 ctor in that set was audited against its retail twin. All
/// but six overwrite `f44` themselves; the six that do not are the call
/// sites of [`Gen::mc2_alloc_2c_zero`], and each retail twin makes NO
/// store to +0x2C — e.g. `sub_4EED0` (the (10,18) summit vortex, file
/// **0x736D0**) is
/// `movb $0x12,0x45 / movb $0xa,0x3f / movb $0x12,0x40 /
///  movw $0xc8,0x2a / movl $0,0x10 / movl $0x2710,0x4 /
///  andb $0xf7,0xc / movsl+movsw -> +0x4c`, i.e. it seeds @0x2A = 200
/// (which the port homes in `f140`, `c10_2a_in_f140`) and never writes
/// @0x2C at all.
///
/// WITNESSES — free-run `MGC_RAW_SHADOW=1`, `f2c` lane, every row
/// `retail 0 port 100`: mc2l13 (10,18) 22,324 rows / (10,19) 1,337 /
/// (10,9) 4; mc2l4 (10,18) 6,374 / (10,19) 486 / (10,9) 2;
/// mc2l6-rival-spells-galore (10,18) 4,067 / (10,19) 1,059 / (10,9) 5 /
/// (10,71) 6 / (10,67) 33; mc2l22-new (10,19) 971 / (10,18) 79 /
/// (10,9) 2; mc2l17 (10,67) 3 / (10,9) 1 / (10,18) 1.
pub(crate) fn no_mc2_alloc_2c_seed() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_ALLOC_2C_SEED").is_some())
}

/// `MGC_NO_MC2_NULL_DISPATCH_PHASE=1` restores the pre-dig per-tick
/// phase counter, which clocked `byte_0x3E_62` for EVERY walked MC2
/// record — including the class-10 actions retail's dispatch table
/// leaves NULL (0x2E, and the (10,75)/(10,77) chain nodes 0x52/0x54),
/// which retail neither runs nor clocks. The shipped-table citation
/// and the receipts live at the call site in
/// [`crate::engine::world::World`]'s entity walk.
pub(crate) fn no_mc2_null_dispatch_phase() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_NULL_DISPATCH_PHASE").is_some())
}

/// `MGC_NO_MC2_M16_SWEEP_ROSTER=1` restores the pre-dig LIVE POOL walk
/// (class 10 / model 45 / `0x400` tests) in the wyvern `(5,16)`'s idle
/// building sweep.
///
/// `sub_24440` (resolve by banner `//----- (00024440)`) walks
/// `dword_38527`, the tick-top `(10,45)` roster, and tests only range
/// and nearest. `NETHERW.EXE` (file = VA + 0x24800):
///   0x24491 `a1 a4 41 00 00` / 0x2449B `8b 80 7f 96 00 00`
///                                 load `dword_38527` (wizext+0x967F)
///   0x244C7 `3b 4d fc` / 0x244CA `77 08`   d² > range² → skip
///   0x244CC `39 f1`    / 0x244CE `73 04`   d² >= best → skip
///   0x244D4 `8b 00`                        next_0
///   0x244FA `c6 43 45 82`                  action = 130
/// No life, class or reap test — the same law as round 137's
/// class-3 roster (`MGC_NO_MC2_CLASS3_SCAN_ROSTER`), on the building
/// chain.
///
/// WITNESS mc2l22-new pair 20383→20384, wyvern slot 203: building 80
/// (dead, action 53) takes its reap stamp at its own dispatch, BELOW
/// the wyvern; retail's sweep still locks it (`target96` 80, action
/// 129 → 130), the port's pool walk skipped it and locked 141. The
/// lane is ungraded on the pair, so it surfaced a tick later as the
/// INHERITED 20385 cluster (wyvern 203 action, 962 `(9,0)` bolt, the
/// one-slot shift of the house-80 rubble 578..587).
pub(crate) fn no_mc2_m16_sweep_roster() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M16_SWEEP_ROSTER").is_some())
}

/// `MGC_NO_MC2_BUILD_REPAINT_ROSTER=1` restores the pre-dig LIVE POOL
/// walk (class 3, `0x400` clear, no life test) in the `(10,45)`
/// completion tail's castle re-paint pass ([`Gen::mc2_building_tick`]).
///
/// `sub_377A0` (EF:27304 caller; resolve by banner
/// `//----- (000377A0)`) walks `dword_38519`, which
/// `UpdateEntities_57730` builds ONCE at the tick top from
/// `class == 3 && life_0x8 >= 0` (EF:39972-85). The port's own comment
/// on the pass already said so ("keeping every class-3 record with
/// `life >= 0`") and then walked the pool with no life test. The two
/// differ on any class-3 record at `life < 0` that is still in the
/// pool. `NETHERW.EXE` (file = VA + 0x24800): 0x377A4 `8b 1d a4 41 00 00`
/// / 0x377AA `8b 9b 77 96 00 00` (the chain head), 0x377C0
/// `e8 8b 8f fd ff` (CompareAxisWithShift_10750), 0x377CD
/// `e8 fe 83 02 00` (sub_5FBD0), 0x377D5 `8b 1b` (next_0) — no class,
/// life or reap test in the walk: a corpse mid-fall, and — permanently — the IMMORTAL ORPHAN
/// BALLOON (`mc2_orphan_balloon_reap`; its only reaper hangs off the
/// castle's dispatch), which stays a live `(3,3)` at `life −1200`.
///
/// WITNESS mc2l22-new pair 33773→33774: building 966 completes
/// (action 51 → 52, life 1 → 90000) over rival 611's orphan balloon
/// 668 (`life −1200`, `target96` 637 — a `(5,27)` by then). Retail
/// mints nothing; the port minted a `(10,42)` painter for 668 into
/// slot 959 (`extra(10,42)slot959`).
pub(crate) fn no_mc2_build_repaint_roster() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_BUILD_REPAINT_ROSTER").is_some())
}

/// `MGC_NO_MC2_RIVAL_LADDER_PRICE_DYING_CASTLE=1` restores the pre-dig
/// bare `rival_castle(own)` lookup in the RIVAL arm of
/// [`World::mc2_drain_ladder_sync`].
///
/// Retail's `sub_60810` (EF:62092) hands the castle that just took a
/// level to `sub_60780` (EF:62067), whose `locEvent2` arm runs
/// `SetSpell_6D5E0(token, token->byte_0x46_70)`; `SetSpell_6D5E0` then
/// prices through `GetSpellManaCost_6D710(Entities[token->
/// parentId_0x28_40], …)`, which resolves the pricing castle as
/// `Entities[owner->dword_0xA4_164x->CastleEntityIndex_0x3A_58]`
/// (`NETHERW.EXE` 0x91f59-0x91f74) — **a REGISTER, with no class,
/// model, owner or reap test**. Round 136's law again: A POOL SCAN
/// RETURNS THE LOWEST-NUMBERED MATCH, A REGISTER THE CHOSEN ONE.
/// Round 136 landed the register on the HUMAN column
/// ([`World::player_castle_bound`] / `mc2_price_castle`) and round 141
/// added the death-arm snapshot there; the RIVAL column still prices
/// off [`World::rival_castle`], a pool scan that filters
/// `flags & 0x400 == 0`.
///
/// A level-0 DESTRUCTION stamps that reap bit BEFORE the ladder drain,
/// so the scan misses and the port takes `GetSpellManaCost`'s
/// castle-less arm (the raw tier `manaCost_6`) where retail reads the
/// rung off the very record it has just decremented to 0. This hunk
/// hands the MAILING castle in as a fallback, which is exactly the
/// record retail's register still names at that instant, and is a
/// no-op on every mail whose scan already resolves.
///
/// WITNESS mc2l18 pair 6633→6634, slot 556 `(15,2)` owner 553 (a RIVAL
/// wizard): its castle, slot 995, goes `scratch10` 1 → 0 and
/// `mana_max` 8500 → 5000 (`sub_60810`'s rung-0 `number2`) while
/// taking the 0x400 reap stamp on the same tick. Retail prices the
/// token at `MC2_CASTLE_COST[0] * 384 >> 8` = **1500** / `mana` 14;
/// the port published the castle-less **5000** / 148. It is the take's
/// LAST `(15,x)` head.
///
/// ⚠ THIS IS A FALLBACK, NOT THE REGISTER. The faithful fix is to give
/// [`World::rival_castle`] the rival's own `CastleEntityIndex_0x3A_58`
/// (the port already keeps `Gen::castle_reg`) the way
/// [`World::player_castle_bound`] has it — that is a separate,
/// wider-blast-radius law, since ~20 rival-AI gates read the same
/// scan.
pub(crate) fn no_mc2_rival_ladder_price_dying_castle() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_LADDER_PRICE_DYING_CASTLE").is_some())
}

/// `MGC_NO_MC2_M22_STALE_PROBE=1` restores the pre-dig worm-head
/// roughness probe (the HEAD's own position when no chain member
/// stands above altitude 0).
///
/// `sub_26FF0` (`//----- (00026FF0)`, the m22 head's move + altitude
/// step) seeds its whole-chain terrain maximum at **0**, not at the
/// type minimum, and copies the winner's position into the 6-byte
/// local `v9x` (`[ebp-0x10]`) only on a STRICT `>`:
///   `27061 89 75 fc`  `mov [ebp-4],esi` (esi = 0 from `2705b 31 f6`)
///   `27080 66 39 f8 / 27083 7e 0f`  `cmp ax,di ; jle` (skip the copy)
///   `27088 8d 7d f0 … 27091 a5 / 27092 66 a5`  `lea edi,[ebp-0x10] ; movsd ; movsw`
/// and then, on the rise arm, hands `&v9x` to `sub_1B7A0_tile_compare`
/// unconditionally (`270cf 8d 45 f0 / 270d2 50 / 270d3 e8 …`) and adds
/// `0x100` (`270e6 fe 43 51`, `inc byte [ebx+0x51]`) when the result
/// beats the row's `word_160_0x10_16`, else `0x40` (`270df 66 83 43 50 40`).
/// So a worm whose ENTIRE chain lies over altitude-0 ground (water)
/// probes the roughness of whatever the stack held at `[ebp-0x10]` —
/// the port probed the head's own (flat) tile and always took `+0x40`.
///
/// ⭐ WHAT THE STACK HOLDS. Let `W` be `esp` between the entity walk's
/// `57a8a 53` (`push ebx`) and `57a8b ff 50 06` (`call [eax+6]`) — the
/// same for every handler. Head handler
/// 0xB0 `sub_26960` (`53 55 89 e5`, then `53 e8`) puts `sub_26FF0`'s
/// `ebp` at `W-36` and `v9x` at **`W-52..W-47`** (x = W-52, y = W-50).
/// The tail-segment handler 0xB4 `sub_26CA0` (`53 55 89 e5`, `53 e8` →
/// `sub_271D0`, `53 56 57 55 89 e5`, no locals) ends with
/// `272a5 e8 … CopyEntityPosition_57CF0` two args deep, whose prologue
/// `57cf0 56` pushes **ESI = the segment's own record pointer** into
/// W-52 (and the call's return address into W-48, the unread `z`).
/// Its relay `sub_26D20` (`ebp` = W-36, one local) returns before any
/// call unless `b39 != 0` and the head is in 0xB0/0xB2; its only calls
/// are `26d97 e8 → sub_581E0` (two args, so its RETURN ADDRESS
/// `0x26D9C` lands in W-52) and `26e9e`/`26ec7 e8 → sub_6E450` (three
/// args; the third is `26e9c 98 / 26e9d 50`, `cwde ; push eax` = the
/// sign-extended tag). An m22 tail segment is the slot directly below
/// the next worm's head, so that record pointer is what the head reads.
/// Record pointers are `0x35CEC6 + 168·slot` (the list terminator
/// `Entities_EA3E4[0]` in the recorded `next_0` words of mc2l18,
/// mc2l22 and mc2l24; mc2l18's head roster 662→677→692 agrees), so
/// the probe is tile `(ptr>>8 & 0xFF, ptr>>24)`. Code runs at VA +
/// 0x1E1000 (remc2's dump; the data object's +0x1D1000 matches the
/// recorded `ptr_a0` words), so the relay-call residue is 0x207D9C.
///
/// ⚠ SCOPE: only the 0xB0 head frame is modelled. 0xB1 (`sub_26990`,
/// `sub esp,0x1c`) and 0xB2 (`sub_26AA0`, `sub esp,8`) put `v9x` at
/// W-88 / W-68, where the segment's frames leave STACK ADDRESSES
/// (`57FA0`'s / `57D70`'s pushed `ebp`) or return addresses depending
/// on the tile path; those, a non-segment predecessor, and a PLAYER
/// tag keep the head's own position (the pre-dig probe).
///
/// WITNESS mc2l18 pair 5618→5619 (and 6053, 6246, 6439, 6632, 6825,
/// 7018 — the 193-tick rise cycle `64 + 256/2 + 1`): head 677 (0xB0)
/// over water, predecessor tail segment 676 → ptr 0x378A66 → tile
/// (0x8A,0x00) roughness 233 > 50 → retail `z 382 → 638`; the port
/// rose `+0x40` to 446 on all 15 chain slots. The same take's other
/// water heads (647, 662, 710, 346, 631, 27 — predecessor tiles
/// roughness 0…10) keep `+0x40`, matching retail.
pub(crate) fn no_mc2_m22_stale_probe() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M22_STALE_PROBE").is_some())
}

/// `MGC_NO_MC2_STAGEVAR_REACT_ROSTER=1` restores the pre-dig
/// StageVar per-entity reaction over every held binding.
///
/// Retail runs `sub_12500` (the per-creature StageVar reaction —
/// release / re-leash / dead-watch scrub) ONLY from the tick-top
/// class-5 roster walk in `UpdateEntities_57730` (EF, the
/// `for (k = 0; k < 29; k++) for (lx = bytearray_38403x[k]; …)` loop
/// after `sub_12780()`), NETHERW.EXE:
///   `579b8 8b 9c b2 03 96 00 00`  `mov ebx,[edx+esi*4+0x9603]` (chain head)
///   `579c1 80 7b 48 00 / 579c7 80 7b 49 00`  StageVar1 / StageVar2 tests
///   `579cd 53 / 579ce e8 …`  `push ebx ; call sub_12500`
///   `579d6 8b 1b`  `mov ebx,[ebx]` (`next_0`)
///   `579e1 83 fe 1d / 579e4 7c cc`  29 chains
/// and that roster was built a few instructions earlier with
/// `life_0x8 < 0` and actions 0xB4/0xE8/0xEA EXCLUDED (see
/// [`Gen::mc2_roster`]). So a creature that is DEAD at the tick top —
/// killed during the previous walk, still in its pre-kill action
/// because its own handler has not yet run — gets no StageVar
/// reaction at all. The port walked `mc2_sv_held` with only a
/// class / `site_z` / reap filter and the phase-4/5 gate, so it
/// re-leashed the corpse of an aggro-broken (StageVar2 10) creature.
///
/// WITNESS mc2l18 pair 2454→2455 (the take's horizon), slot 614, a
/// `(5,17)` on StageVar1 3 / StageVar2 10, action 137 (idle, phase 1),
/// killed by the human (slot 411) during t=2454's walk (life 3200 → −1).
/// Retail keeps `sv2 = 10` and its own handler goes 137 → 140; the
/// port's tick-head pass ran `sub_12330` (sv2 10 → 2, as retail itself
/// does at t=1215 and t=1647 while the creature is alive).
pub(crate) fn no_mc2_stagevar_react_roster() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_STAGEVAR_REACT_ROSTER").is_some())
}

/// Retail record pointer of pool slot `slot` (see
/// [`no_mc2_m22_stale_probe`]).
pub(crate) const MC2_RETAIL_REC_PTR_0: u32 = 0x0035_CEC6;

/// `MGC_NO_MC1_RIVAL_TOKEN_GATE_LIVE_PURSE=1` restores the pre-dig
/// `RivalState::mana` MIRROR reads in the three MC1 rival class-12
/// token gates. Retail has no separate rival purse: `sub_55DD0`
/// (`CARPET.EXE 0x6E5C8`) reads the OWNER RECORD's own live `+140`
/// word — `0x55E2F: 8b 81 8c 00 00 00` (`mov 0x8c(%ecx),%eax`) — and
/// compares it SIGNED against the TOKEN's live `+136`
/// (`0x55E35: 3b 82 88 00 00 00` / `7c 12` = `jl`). It also refuses
/// outright when `+140 < 0` (`0x55DDA: 83 b9 8c 00 00 00 00` +
/// `0f 8c 7a 00 00 00`), a leg that sits AHEAD of the mid-burst
/// escape at `0x55E4F` and which the port lacked entirely.
///
/// The mirror is only re-synced at the owner's own dispatch, but
/// combat writes `+140` DIRECTLY and MID-TICK (the Rebound deflection
/// quarter `sub_52B30` :62884, the shield quarter `sub_46540` :55703),
/// so a token dispatched after its owner's brain but before the tick
/// ends saw a purse retail had already emptied. WITNESS mc1l49
/// t=19558: rival 724 `(3,1)` holds a full-strength Shield burst
/// (token slot 632 `(12,4)`, `+136 = 2000`); a deflection debits
/// `60000 >> 2 = 15000` onto `724.+140`, leaving 1603. Retail reads
/// 1603 < 2000 → refuse → `+48 = 1; --` → 0. The port read the stale
/// mirror 16603 → admit → `mana 16603 − 15000 − 2000 = −397` →
/// `clamp(0, …)` = 0 and `+48 = 250`.
///
/// ⭐ The port's own doc comment on `rival_token_gate` already said
/// "the OWNER's purse" while the code read the mirror — round 115's
/// law, hit again. Three call paths, per round 104: the gate itself,
/// `rival_castle_token_tick`'s inlined twin, and
/// `rival_heal_token_tick`'s extra afford leg (`sub_56270`'s
/// `v1[35] >= *(a1+136)`). MC2 landed this same law already; see
/// [`crate::mc2::cast::no_mc2_wiz_purse_is_entity`].
pub(crate) fn no_mc1_rival_token_gate_live_purse() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_TOKEN_GATE_LIVE_PURSE").is_some())
}

/// `MGC_NO_MC1_CASTLE_BALL_STEPBACK_MOVES_BALL=1` restores the pre-dig
/// behaviour where the Create-Castle ball's refused-scan step-back was
/// computed into locals for `spawn_castle` only, with `z` seeded 0 and
/// discarded. Retail's `sub_53B50_53E90` tripwire branch MOVES THE BALL
/// ITSELF and seeds the step from the ball's live `+72/+76`:
/// `CARPET.EXE 0x6C531` (`cmp BYTE [esp+4],0` / `74 42`) →
/// `0x6C53D lea esi,[ebx+0x48]`, `0x6C540 a5` (x,y) **and
/// `0x6C541 66 a5` (z)** → `yaw+0x400` (`0x6C553 add ah,4`,
/// `0x6C556 and ah,7`), the LIVE pitch (`0x6C54A mov ax,[ebx+0x20]`)
/// and live speed (`0x6C543 movsx eax,[ebx+0x7e]`) into
/// `sub_41EC0_42200` (`0x6C564` → VA `0x41EC0`) → then
/// **`0x6C571 53` = `push ebx`** into `0x6C572` → `sub_41C70_41FB0`
/// (VA `0x41C70`, move + relink). The ctor runs AFTER that and reads
/// the ball's own axis (`0x6C584 lea eax,[ebx+0x48]` → `0x6C588` →
/// `sub_373F0_377B0`, VA `0x373F0`), so the castle is built at the
/// ball's NEW position and the ball's final recorded x/y/z are the
/// stepped-back ones. Decompile `reference/remc1/sub_main.cpp`
/// :63598-611.
///
/// WITNESS mc1l48-nodeath t=473→474 slot 972 (yaw 527, pitch 31, speed
/// 458): retail `(1388, 29804, 3466)` → `(1387, 29805, 3380)`; the port
/// stopped at the FORWARD step `(1842, 29825, 3423)` and `3423 − 43 =
/// 3380` — the pitch's vertical component applied once where retail
/// applies it twice. ⚠ The ball is the HUMAN's (`id24 = 681`), alive
/// since well before the head, so this is neither a mint nor a
/// mover-arithmetic law: every steering lane, the speed ease, the life
/// countdown and the `0x400` latch were already byte-exact. Gated on
/// MC1 so the MC2 column is byte-identical by construction; the
/// `castle_latch_bug` patch arms fork the displacement PREDICATE, not
/// this mechanic.
pub(crate) fn no_mc1_castle_ball_stepback_moves_ball() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CASTLE_BALL_STEPBACK_MOVES_BALL").is_some())
}

/// `MGC_NO_MC1_CASTLE_BALL_DRY_POOL_RETRY=1` restores the pre-dig
/// create-arm landing that reap-flagged the (9,10) castle ball whether or
/// not the castle ctor returned a record. Retail's `sub_53B50_53E90`
/// (`reference/remc1/sub_main.cpp` :63606-11) puts the owner stamp AND
/// the `sub_41E80` reap call inside `if (v2)` — CARPET.EXE 0x6C588 `e8`
/// → `sub_373F0`, 0x6C590 `85 c0 test eax,eax`, 0x6C592 `74 11 je
/// 0x6c5a5` (the epilogue) — so on a DRY POOL the landed ball simply
/// lives on, re-steers, re-lands and retries the ctor every tick until a
/// slot frees. WITNESS mc1l25 t=2616-2619 slot 778 (999 live): retail
/// flags hold 6 with the life frozen at 17 (the grounded short-circuit);
/// the castle finally builds at t=2621. The homing arm (`sub_53980`
/// :63505-15) already carried its own `if (v10)` with the pin release;
/// this is the create arm's twin, which releases nothing.
pub(crate) fn no_mc1_castle_ball_dry_pool_retry() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CASTLE_BALL_DRY_POOL_RETRY").is_some())
}

/// `MGC_NO_MC1_AABB_SIGNED_EXTENTS=1` restores the pre-dig AABB test
/// that widened the six extent operands (+80/+82/+84 of both parties)
/// UNSIGNED. Retail's `sub_118C0` (`reference/remc1/sub_main.cpp`
/// :16963, the primitive under `sub_11950` pair overlap and the player
/// probe) loads every one of them `movswl` — CARPET.EXE file 0x2A0CC
/// `0f bf 53 02 movswl 0x2(%ebx),%edx`, 0x2A0D0 `0f bf 41 02 movswl
/// 0x2(%ecx),%eax`, `lea (%edx,%eax,1),%esi`, then `cmp %esi,%eax; jge
/// <miss>` against the wrapped-16-bit |Δx| (0x2A0D7-0x2A0E6); +82 at
/// 0x2A0E8/0x2A0EC, +84 at 0x2A106/0x2A10A. MC2's `sub_106C0`
/// (NETHERW.EXE 0x34EFF/0x34F03, 0x34F1D/0x34F21) is the same shape.
/// Only +78 was ported signed. An extent past 0x7FFF is therefore
/// NEGATIVE in retail and the summed box can never admit anything.
///
/// WITNESS mc1l26-froze t=25647 (pool at 999): the human's (10,17)
/// blast ring at slot 917 carries `+26` = 250 (a stale-alias write by
/// the (10,18) at 949 on t=25646, reproduced by the port), so its
/// `sub_37130(768·250/4)` extents are 48000 = −17536 signed; the ch0
/// castle pre-pass of `sub_120B0` finds no castle in retail, while the
/// port's unsigned box (48000 + 640) reached castle 978 at (16384, 0)
/// across the y seam and posted 300 (`act_life 39750 vs 39450`).
pub(crate) fn no_mc1_aabb_signed_extents() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_AABB_SIGNED_EXTENTS").is_some())
}

/// `MGC_NO_MC1_CASTLE_TOKEN_DRY_POOL_RETRY=1` restores the pre-dig
/// castle-token fire that latched `+48 = +50 − 1` whether or not the
/// ball was minted. Retail's `sub_57610_57B40` (:65876-921) is gate →
/// `sub_55E80` debit → `sub_373F0(…, 9, 10)` → `if (v3) { +48 = +50 − 1;
/// stamps; sound 15 }` — CARPET.EXE 0x6FE73 `call sub_55E80` (the
/// debit), 0x6FE83 `call sub_373F0`, 0x6FE90 `85 c0 test eax,eax`,
/// 0x6FE92 `0f 84 4d 01 00 00 je 0x6ffe5` (the epilogue, past the
/// 0x6FE98 `+48 = +50 − 1` latch and every stamp) — so on a
/// DRY POOL the token stays FULL and the whole arm re-runs next tick,
/// debiting the full `+136` again, until a slot frees or the gate's
/// `mana >= +136` leg fails and releases (`+48 = 0`, :65920). WITNESS
/// mc1l25 t=7013-7017 slot 674 (999 live): retail `+48` holds 101 for
/// four ticks, `+132` = −20000 on each, purse 92885 → 12885, release at
/// 7017; the port paid once, latched 100, and the t=7026 recast fizzled
/// on the stale latch (the upgrade ball and its (10,43) never minted).
pub(crate) fn no_mc1_castle_token_dry_pool_retry() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CASTLE_TOKEN_DRY_POOL_RETRY").is_some())
}

/// `MGC_NO_MC1_WIN_STREAK_PRE_TEST=1` restores the pre-dig banked-share
/// win latch that fired on the frame the streak counter REACHED
/// `win_streak_ticks`. Retail's `sub_415C0_41900`
/// (`reference/remc1/sub_main.cpp` :52130-35) tests the counter BEFORE
/// the increment — `if (v5 < 16) +13323 = v5 + 1; else +13325 |= 2` —
/// CARPET.EXE 0x59E58 `cmp cx,0x10` / 0x59E5C `jl` (increment) / 0x59E5E
/// `or byte [ebx+0x340d],2`: the bit lands on the 17th consecutive
/// over-frame with the counter parked at 16, one frame later than the
/// port's `+= 1; >= 16`. WITNESS mc1l10 t=19380/19381: the (11,4) win
/// trigger (slot 3) fires its disposition — the (5,11) genie at slot 1 —
/// one walk early in the port (record 19380 vs retail's 19381).
pub(crate) fn no_mc1_win_streak_pre_test() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_WIN_STREAK_PRE_TEST").is_some())
}

/// `MGC_NO_MC1_WIN_STREAK_PAST_LATCH=1` restores the pre-dig port
/// (round 154, w154g), whose banked-share streak counter FROZE once
/// the win latched and was ZEROED by the (11,4) win trigger's consume.
///
/// Retail's counter and latch are two fields with two lifetimes.
/// `sub_415C0_41900` (:52119-38, shipped `CARPET.EXE` file = VA +
/// 0x187F8) runs for every in-play wizard (`+13329 != 0`, 0x59DD8)
/// with a bound castle (`wizext+50`, 0x59E01) on EVERY frame the
/// `0x110` flags are clear — there is no "already won" test anywhere
/// in it: `share <= goal` ⇒ `+13323 = 0` (0x59E73 `movw $0x0,
/// 0x340b(%ebx)`); else `< 16` ⇒ `+13323 + 1` (0x59E58/0x59E67-6A);
/// else `+13325 |= 2` (0x59E5E `orb $0x2,0x340d(%ebx)`). The bit is
/// only ever OR'd on here and only ever cleared by the win trigger
/// (`sub_59B80` :67310, 0x72408 `and $0xfd,%bl`), which stores
/// NOTHING to `+13323` (0x72378-0x72433: the one `0x340d` store, no
/// `0x340b`). So (a) past the latch the counter keeps running — parks
/// at 16 while the share holds, resets to 0 when it drops, and the
/// latch stays set either way (the level is still won; SPACE still
/// ends it, :20082 → case 0x1B); and (b) a consumed win re-latches on
/// the same frame's post-walk objective pass (`DrawAndEventsInGame`
/// :41668-71: `sub_3C9D0` then `sub_415C0`) if the share is still
/// over, because the counter is still 16.
///
/// The port gated `objective_mc1` on `!completed` and had the trigger
/// zero `win_streak` — the round-153 census's two shapes: free-run
/// `retail 0 port 16` (mc1l49 t=44474..58562, 8,291 rows: the share
/// dropped after the t=40720 latch and the port's counter sat frozen)
/// and `retail 16 port 1` (mc1l10 t=19381: the genie trigger consumed
/// the win at walk 19380 and the port restarted the count from zero;
/// retail's record 19381 still reads `status 2` because the post-walk
/// pass re-OR'd it off the parked 16). Player-visible natively: on a
/// scripted level the port UN-WON the level at the trigger's consume
/// and demanded a fresh 17-frame hold before SPACE would end it.
pub(crate) fn no_mc1_win_streak_past_latch() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_WIN_STREAK_PAST_LATCH").is_some())
}

/// `MGC_NO_MC1_RESPAWN_KEEPS_REGEN=1` restores the pre-dig port
/// (round 154, w154g), whose MC1 respawn zeroed the human's regen
/// stall (`regen_delay`, retail `u32_383`) and rate register
/// (`life_rate`, retail `u16_341`).
///
/// `sub_44D30_45070`'s Type_160 re-arm (:54866-73) is seven stores in
/// the shipped `CARPET.EXE` (file = VA + 0x187F8):
/// ```text
///   5d645  66 89 42 30              mov  %ax,0x30(%edx)        ; var_48 = wizard idx
///   5d64f  66 c7 80 4b 01 00 00 64  movw $0x64,0x14b(%eax)     ; u16_331 = 100 (grace)
///   5d65e  c7 80 5f 01 00 00 d0 07  movl $0x7d0,0x15f(%eax)    ; u32_351 = 2000
///   5d66e  66 c7 40 0c 00 00        movw $0x0,0xc(%eax)        ; v_12
///   5d67a  66 c7 40 18 00 00        movw $0x0,0x18(%eax)       ; v_24
///   5d686  66 c7 40 1a 00 00        movw $0x0,0x1a(%eax)       ; v_26
///   5d692  66 c7 40 16 00 00        movw $0x0,0x16(%eax)       ; v_22
///   5d6a3  66 c7 40 10 00 00        movw $0x0,0x10(%eax)       ; v_16 (the listing drops this one)
/// ```
/// No `0x17f` (+383) and no `0x155` (+341) store exists in the
/// routine (0x5D528-0x5D888). The stall the fatal hit armed
/// (`sub_46540` :55725 `+383 = 16`) rides the corpse untouched — the
/// regen tail's `actLife >= 0` fork (:55381) never reaches the
/// decrement — and the new life spends it over its first 16 frames
/// with the rate register still holding the pre-death selection.
/// Round-153 census: `regen_stall retail 16 port 0` 1,849 free rows /
/// 28 takes = 16 rows per respawn, `life_rate retail 5 port 0` one row
/// per respawn (mc1l20 t=19033, mc1l10 t=1473, mc1l48 ×26, mc1l49
/// ×19). Shadow-only: the respawn seats `actLife = maxLife` (:55029)
/// under a 100-frame grace, so the graded `life` lane cannot move.
pub(crate) fn no_mc1_respawn_keeps_regen() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RESPAWN_KEEPS_REGEN").is_some())
}

/// `MGC_NO_MC1_ERUPTION_COUNTER_REREAD=1` restores the pre-dig
/// behaviour where `eruption_tick` cached `+26` at function entry and
/// used that one value for the blast gate, the death gate and the
/// increment. `sub_25EC0` re-reads `+26` FROM MEMORY at all three
/// points: `:28803` (`v13 = *(a1+26)`, CARPET.EXE `0x3E88A:
/// 66 8b 7b 1a` = `mov 0x1a(%ebx),%di`, then `0x3E891 test %di,%di` /
/// `0x3E894 75 77 jne 0x3E90D` — the blast is skipped straight to the
/// death check), `:28825` (`0x3E90D: 66 83 7b 1a 7f` =
/// `cmpw $0x7f,0x1a(%ebx)`) and `:28831` (`0x3E928: 66 ff 43 1a` =
/// `incw 0x1a(%ebx)`, an in-memory `++`).
///
/// It matters because the eruption-start block's register kick
/// `*(prev+26) = 250` (`0x3E7B6: 66 c7 42 1a fa 00`) is a BLIND write
/// guarded only by `slot != 0` (`0x3E7B4: 76 06`). When the stale
/// global `erupting` register happens to name the very pool slot just
/// recycled into THIS new volcano driver, retail **self-kicks its own
/// `+26` to 250** — and the re-reads then make it mint NO
/// eruption-start blast (250 ≠ 0), reap-flag itself on its very first
/// tick (250 ≥ 127), clear the `erupting` register it set two
/// instructions earlier, and end the tick recording `+26 = 251`.
///
/// WITNESS mc1l49 t=29062: at t=29061 `erupting=968` with slot 968 a
/// long-stale reap-flagged `(10,6)` (`act_life −2`); the pool recycles
/// 968 into the new `(10,18)` driver and retail records
/// `f26 0 → 251`, `flags 132100 → 1024`, `f30 0 → 1280`, popping ONE
/// slot all tick (free stack 262 → 261, next-pop 168 → 169) for a
/// `(10,13)` at 168. The port minted a spurious `(9,0)` blast at 168 —
/// `sclass/smodel 255` is `new_event`'s default (`+66 = +67 = 0xFF`,
/// :43879), i.e. exactly a fresh blast fireball, not a record missing
/// its source — and pushed retail's `(10,13)` to 169. Because the
/// spurious blast consumes a free-stack slot, the defect surfaces as
/// an ENTITY-SET divergence.
///
/// ⚠ The register-block gate (:28778 / `0x3E779`) is deliberately left
/// on the entry value: retail re-reads there too, but only the pure
/// ground query `sub_11F50` runs between the `fire` decision and it,
/// so the two are provably identical.
pub(crate) fn no_mc1_eruption_counter_reread() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ERUPTION_COUNTER_REREAD").is_some())
}

/// `MGC_NO_MC1_SCATTER_JAR_OWNER_CLEAR=1` restores the pre-dig
/// `+144 = 0` the port stamped on every spell jar a dying wizard
/// scatters. **The write is INVENTED.** Retail's scatter loop is the
/// `for (i = 0; i != 96; i += 4)` body of `sub_45C10_45F50`
/// (:55520-49) and its ENTIRE set of writes to the scattered record is
/// `+16 &= ~1` (:55534), `++(+70)` (:55535), the `sub_41C70_41FB0`
/// move-relink (:55546) and `+12 = rand % 90 + 200` (:55549). Byte-
/// verified in CARPET.EXE at the loop body `0x5E9B8..0x5EAE4`:
/// `andb $0xfe,0x10(%ebp)` (`0x5EA22`), `incb 0x46(%ebp)` (`0x5EA26`),
/// `call 0x5A468` (`0x5EA94`) and `mov %edx,0xc(%ebp)` (`0x5EABA`) —
/// **there is no store to `0x90(%ebp)` anywhere in the loop**, and the
/// only other stores are the wizext blue byte `0x394(%edx,%eax,1)` and
/// the `+0x214 = -1` empty-handle arm. The owner tag therefore RIDES
/// the scatter untouched.
///
/// It matters because a wizard's book can name a slot that is no
/// longer his token (the port has no `+42` on `Ent`, and retail's own
/// `+676` is a bare array index — see [`no_mc1_castle_token_index`] —
/// so a recycled slot aliases). WITNESS mc1l49 t=39094: rival 594
/// dies with `wizext+532` naming pool slot 26, which by then is the
/// HUMAN's live `(10,39)` mana ball holding 7,000. Retail scatters it
/// verbatim — `+70 41→42`, `actLife 300→242`, teleported to the
/// corpse at (18912, 813) — and **keeps `+144 = 569`**; the port's
/// clear dropped the ball out of the human's mana census, so
/// `player.mana_max` read 1,711,909 against retail's 1,718,909 at
/// t=39095 (exactly −7,000). Both arms of the fall handler carry the
/// stray, the human's ([`crate::engine::world::World::player_land`])
/// and the rival's (`mc1::rivals`' `rival_land`).
pub(crate) fn no_mc1_scatter_jar_owner_clear() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_SCATTER_JAR_OWNER_CLEAR").is_some())
}

/// `MGC_NO_MC1_BUILD_SITE_CHAIN=1` restores the pre-dig POOL SCAN in
/// MC1's `m12_build` site-overlap veto. Retail walks the two TICK-TOP
/// ROSTER CHAINS, not the live pool: `HIDDEN.EXE 0x1ED41`
/// `mov ebx,[ebx+0x8e76]` is the +36470 HOUSE head ([`Gen::bldg_chain`])
/// and `0x1EDB3` `mov ebx,[ebx+0x8e6e]` the +36462 CLASS-3 head
/// ([`Gen::wiz_chain`], filtered `cmp byte [ebx+0x41],0x2` = the
/// castle), each stepped through the `->next` word at `+0` to the pool
/// sentinel. `CARPET.EXE` carries the identical bytes at the identical
/// VAs. Neither walk has a flags test — membership IS the gate.
///
/// The consequence the port could not express: a dwelling minted
/// EARLIER IN THE SAME TICK is not on the tick-top chain, so retail
/// cannot see it and the second settler's site is accepted.
/// mc1hwl1 t=17542 is exactly that — settlers 22 and 909 both close
/// their site attempt on the same tick; the port let 22's newborn
/// house 788 (12544,46592) veto 909's site at (16384,43520) on a
/// 3840 <= 4736 / 3072 <= 4224 box and lost slot 968 outright.
///
/// ⭐ MC2's twin (`mc2_m12_build`) has walked `bldg_chain` since it
/// was written; MC1's never did — the tenth "a law on one call path
/// is not landed".
pub(crate) fn no_mc1_build_site_chain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_BUILD_SITE_CHAIN").is_some())
}

/// `MGC_NO_MC1_GRAVE_REPOINT_CHAIN=1` restores the pre-dig LIVE POOL
/// SCAN in the fall handler's grave re-point.
///
/// `sub_45C10_45F50`'s landing block re-points the dead wizard's mana
/// balls at the fresh `(10,40)` grave, and it does so by walking the
/// TICK-TOP BALL ROSTER — `var_u32_36462[1]`, the bucket[1] head —
/// not the pool. HIDDEN.EXE, VA `0x46677`..`0x466c7` (file
/// `0x5f06f`..`0x5f0bf`, VA + 0x189f8), read off the shipped bytes:
///
/// ```text
///   46677: 8b 2d f8 e3 01 00   mov 0x1e3f8,%ebp        ; wizext base
///   46681: 8b ad 72 8e 00 00   mov 0x8e72(%ebp),%ebp   ; +36466 = bucket[1] head
///   46687: bf a4 00 00 00      mov $0xa4,%edi          ; 164
///   4668e: 80 7d 41 27         cmpb $0x27,0x41(%ebp)   ; model65 == 39
///   46692: 75 2a               jne  <next>
///   ...    (ebx - pool)/164 -> eax                     ; corpse slot
///   466a1: 66 8b 95 90 00 00 00 mov 0x90(%ebp),%dx     ; +144
///   466a8: 39 c2 / 75 12       cmp %eax,%edx / jne <next>
///   466b7: 66 89 85 90 …       mov %ax,0x90(%ebp)      ; +144 = grave slot
///   466be: 8b 6d 00            mov 0x0(%ebp),%ebp      ; ->next
///   466c1: mov 0x1e3f0,%ecx / add $0x7463,%ecx / cmp %ecx,%ebp / ja
/// ```
///
/// — the `->next` walk with the pool base (0x7463 = 29795) as the
/// terminator, and the ONLY member tests are `model65 == 39` and
/// `+144 == corpse slot`. No class test (roster membership is the
/// class gate), no `0x400` reap test.
///
/// It matters because the roster is a TICK-TOP SNAPSHOT and the MC1
/// seizure BLANKS every roster head for the rest of the tick
/// ([`Gen::new_event`]) — so on a pool-exhausted tick the walk sees
/// NOTHING and the balls keep the dead wizard's `+144`.
///
/// WITNESS mc1hwl2 t=15762: rival 3's corpse (slot 449) lands, the
/// 24-jar death scatter exhausts the free stack and SEIZES, and
/// retail's re-point therefore leaves slots 875/938/953 — three
/// `(10,39)` balls holding 5000 + 134 + 3000 — at `+144 = 449`. The
/// port's pool scan re-pointed all three at the grave, and the NEXT
/// tick's mana census (`recompute_mana`) consequently read rival 449
/// back at the intrinsic base 1000 and dumped the 8134 onto the
/// grave's own `+136` through the owner-credit fallback: t=15763
/// slot 132 `mana_max` retail 0 / port 8134, slot 449 retail 9134 /
/// port 1000. That pair was the take's ONLY divergence in 28,580
/// ticks.
pub(crate) fn no_mc1_grave_repoint_chain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_GRAVE_REPOINT_CHAIN").is_some())
}

/// `MGC_NO_MC1_PAYLOAD_CHILD_REAP_GATE=1` restores the pre-dig
/// UNCONDITIONAL soft kill on a spell-payload detonation. Retail's
/// `sub_52770` explode tail (:62757-72) is one guarded block:
///
/// ```c
/// result = sub_373F0_377B0((axis_3d*)(a1 + 72), *(char*)(a1+68), *(char*)(a1+69));
/// if ( result ) {
///     sub_526C0_52A00(a1, v6, v22_21);
///     …five child stamps…
///     sub_41E80_421C0((Type_AE400_29795*)a1);   // flags |= 0x400
/// }
/// ```
///
/// — so a detonation the ALLOCATOR refuses does not reap the bolt: it
/// stays airborne (parked on its victim) and retries the explode every
/// tick until a slot exists. The port reaped unconditionally, which
/// killed the bolt on the parking tick and, on a starved pool, moved
/// the child's birth tick and slot.
///
/// WITNESS mc1l49 t=54341, slot 857, the human's `(9,4)` volcano lob:
/// the pool is DRY (free 0 / recycle 0 at t=54340, 54341 and 54342 —
/// zero births at 54342), the bolt parks on rival castle 940 at
/// (0, 16384, 6048), and retail keeps `flags = 0x2006` through 54341
/// AND 54342, raising `0x400` only at t=54343 — the first tick a slot
/// frees, where the `(10,9)` hill is minted into slot 363. The port
/// read `0x2406` from t=54341. Both mc1l49 heads t=54341 and t=54342
/// are this one law. See [`crate::mc1::combat`]'s `proj_payload_tick`.
///
/// ⚠ An arm that attempts NO spawn still reaps: the duel dart's miss
/// fork (`sub_530C0_53400` :63208-09) calls `sub_526C0(a1, 0, ..)` and
/// `sub_41E80` with no `+68/+69` call at all, so `spell_payload`
/// returns `true` there and the gate is a no-op on that path.
pub(crate) fn no_mc1_payload_child_reap_gate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_PAYLOAD_CHILD_REAP_GATE").is_some())
}

/// `MGC_NO_JAR_WATER_SINK=1` restores the pre-dig jar z-servo: a
/// bare `.max(ground)` floor with no water leg. Retail's `sub_42090`
/// (:52605) moves the floor to **-768** when the position is over
/// water (`sub_11760 & 1`) AND the ground reads 0, steps only 25% of
/// the fall once the record is at or below the ground, and returns -1
/// the tick z lands exactly on -768 — which is the caller's
/// (:64766-70) soft-free. Kept so one binary can be A/B'd.
pub(crate) fn no_jar_water_sink() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_JAR_WATER_SINK").is_some())
}

/// `MGC_NO_CASTLE_PAINT_PHASE=1` restores the pre-dig PHASE of the
/// castle painter: the height-apply pass BEFORE the footprint kill
/// and the `sub_33800` paint. Retail (:30630-46) runs the kill and
/// the paint INSIDE the per-row RLE walk, i.e. on the PRE-STEP
/// heights, and only sweeps the buffered deltas into the heightmap
/// after every row has been walked (:30541-79). Kept so one binary
/// can be A/B'd.
pub(crate) fn no_castle_paint_phase() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_CASTLE_PAINT_PHASE").is_some())
}

/// `MGC_NO_BALLOON_REG=1` drops the fleet dispatcher back to the
/// live-census stand-in (empty register + the adoption pass = the old
/// ascending-slot walk, and the imported wizext+52 order ignored) —
/// the A/B arm for the register law, so one binary measures both.
/// Read once: a whole-process arm, never a per-run input.
pub(crate) fn no_balloon_reg() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_BALLOON_REG").is_some())
}

/// `MGC_NO_MC1_BALLOON_SEAT_LIFE_ONLY=1` restores the pre-dig
/// register PRE-CLEAR in [`Gen::castle_balloons`] (a seat whose
/// record is no longer a live own `(3,3)` was blanked BEFORE the walk,
/// so the walk spawned a replacement in the SAME pass).
///
/// ⭐ RETAIL'S FLEET REGISTER IS CLASS-BLIND. `sub_47400` (:56329-49)
/// reads the seat's record and tests ONE thing — `CARPET.EXE` VA
/// 0x4752E `39 cb / 0f 86` (`cmp ecx,ebx; jbe` → empty seat ⇒ spawn)
/// then VA 0x47536 `83 7b 0c 00 / 7d 27` (`cmpl $0,0xc(%ebx); jge`) —
/// `act_life >= 0` ⇒ the LIVE arm (retarget on the stagger turn if
/// `+70 == 9`, tally `+140`/`+136` into wizext+294/+290), otherwise
/// the DEAD arm (`call 0x27690` cargo drop, `call 0x41E80` soft-kill,
/// `movw $0,0x34(%eax)` = clear the seat, jmp LABEL_27 — no spawn,
/// the replacement is a pass LATE). No class, model, owner or `0x400`
/// byte is read anywhere in the walk. So a seat whose balloon was
/// SACRIFICED by a dry-pool `NewEvent_372C0` and re-minted as some
/// other class keeps its seat while the new occupant's life is >= 0,
/// and is cleared (without a spawn) on the first pass after that
/// record's life goes negative.
///
/// MEASURED mc1l20 t=18192 (pool at n=999, 322 lightning births in one
/// tick): rival 518's register `[346, 924, 0]`; balloon 924 is
/// sacrificed at slot 375's storm into a `(9,9)` child with life 0.
/// Castle 847 (< 904) dispatches after the storm: retail keeps seat 1
/// (life 0 >= 0), spawns nothing; at 18194 the child is reaped residue
/// (life −2) ⇒ dead arm, seat cleared; at 18196 the replacement `(3,3)`
/// is born at slot 534. The pre-clear blanked seat 1 at 18192 and
/// minted a balloon into recycle victim 963 (retail's next sacrifice,
/// which retail never popped), then again at 18194 into 950.
pub(crate) fn no_mc1_balloon_seat_life_only() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_BALLOON_SEAT_LIFE_ONLY").is_some())
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

/// See [`Gen::player_hurl`] — the doomsday pyramid's HURL-AWAY BEAM
/// on the human, hash-TRANSPARENT while disarmed so no pre-boss
/// golden moves for carrying it. Drained at the carpet's own walk
/// slot, exactly like [`PlayerWhirl`], and deliberately NOT
/// snapshotted: a save taken mid-beam reloads owing at most one tick
/// of shove.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct PlayerHurl {
    /// `sub_21AB0` case 7 armed the beam this tick.
    pub armed: bool,
    /// `v18 = tan2(&a1x->position, &v31x->position)` (EF:13443) —
    /// pyramid → player, the OUTWARD bearing.
    pub bearing: u16,
    /// `D41A0_0.word_0x36546` after its `-= 80` / floor 10 / ceiling
    /// 1024 step — `MoveEntity_57FA0`'s distance (EF:13444).
    pub dist: i16,
}

impl std::hash::Hash for PlayerHurl {
    /// Hash-silent OUTRIGHT, the [`Lease2e`] / [`Raw48`] opt-out
    /// rather than [`PlayerSpin`]'s transparent-at-rest one. The two
    /// drain sites both sit behind `drive`, so a PINNED pair tick
    /// (`verify-deltas`, the fixture runner) arms the one-shot and
    /// never spends it — hashing it there would move a fixture
    /// signature on a beam tick for a channel that tick cannot use.
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// See [`Gen::player_flood_pull`] — the quake/flood's vertical pull on
/// the human. Hash-silent OUTRIGHT, for [`PlayerHurl`]'s reason: the
/// drain site sits behind `drive`, so a PINNED pair tick arms the
/// one-shot and never spends it.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct PlayerFloodPull {
    /// `sub_39B60`'s human arm shoved the wizard this tick.
    pub armed: bool,
    /// `48 * (((4096 - v5) << 8) >> 12) >> 8` — the downward step,
    /// POSITIVE (subtracted from z).
    pub pull: i32,
    /// The HORIZONTAL leg's bearing, `tan2(player, quake)` — the same
    /// value the shove posts on [`Gen::player_knock`], carried here
    /// so the walk-pose reader can re-derive the displacement without
    /// draining (or trusting) the shared knock lane. See
    /// [`crate::engine::world::World::mc2_flood_walk_pose`].
    pub bearing: u16,
    /// The horizontal leg's distance (`v6`, clamped 4..=128).
    pub dist: i16,
    /// ⭐ [`no_mc2_flood_human_seat`]'s transport: the ACCUMULATED
    /// horizontal displacement of every visit this walk made (x, y),
    /// wrapping engine units. `pull` then carries the accumulated
    /// (positive-down) z displacement, terrain floor included.
    pub dx: i32,
    pub dy: i32,
    /// A publication the walk hook has not yet adopted into the
    /// mid-walk pose.
    pub fresh: bool,
    /// `sub_3A200`'s pitch seizure (`pitch = pitch_acc = 512`) — see
    /// [`no_mc2_flood_human_spin`]. Independent of `armed`: the close
    /// band moves nothing, so the walk-pose hook has nothing to adopt.
    pub spin: bool,
    /// The positional legs (`dx`/`dy`/`pull`) are ON THE HUMAN'S
    /// RECORD already — the walk hook's republish was the record write
    /// ([`crate::engine::world::no_mc2_flood_record_write`]) — so the
    /// carpet's dispatch, whose mover loads that record, must not
    /// apply them again.
    pub on_record: bool,
}

impl std::hash::Hash for PlayerFloodPull {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// See [`Gen::player_whirl`] — the whirlwind's pose seizure on the
/// human, hash-TRANSPARENT while disarmed so no pre-tornado golden
/// moves for carrying it. Drained at the carpet's own walk slot.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct PlayerWhirl {
    /// Armed by `sub_33340`'s human arm this tick.
    pub armed: bool,
    /// `v14` — the absolute heading written to BOTH `word_0x30_48`
    /// and `yaw_0x1C_28` on the mid ring (EF:24350-56).
    pub heading: u16,
    /// `v30` — `MoveEntity_57FA0`'s distance, stepped along
    /// `word_0x30_48` (96 on the mid ring).
    pub step: i16,
    /// ⭐ THE GRAB FAMILY'S PAYLOAD (`sub_33340`'s inner, near-grab and
    /// far-grab arms — `NETHERW.EXE` 0x57ca3 / 0x57d65 / 0x57da9).
    /// Those three arms TELEPORT the victim (`predictedAxis` is the
    /// funnel's own `axis_0x9A_154x` on the inner arm) and can run
    /// MORE THAN ONCE per tick, so the channel cannot carry a
    /// heading+distance pair the way the mid ring's does: the funnel's
    /// own slot resolves the whole walk and publishes the ABSOLUTE
    /// `(x, y, z, yaw_0x1C_28)` `CopyEntityPosition_57CF0` would have
    /// written. `None` = only the mid ring fired, the pre-existing
    /// `heading`/`step` path (dig W4).
    pub grab: Option<(u16, u16, i16, u16)>,
    /// EF:24345-49 / 0x57c8d `mov word [ebx+0x82],0x50` — `actSpeed
    /// = 80`, written for the HUMAN (`v40`) on the NOT-YET-GRABBED
    /// arms only. The grabbed arms jump over that block (0x57c61
    /// `jnz 0x57d65`), and `sub_5D530`'s stop veto then freezes the
    /// speed servo, which is why retail holds 80 across a grab.
    pub act80: bool,
    /// ⭐⭐⭐ THE CAMERA-ROLL CRANK, AND IT RIDES EXACTLY WHERE `act80`
    /// DOES. EF:24344-46 `v8 = ix->dword_0xA4_164x->roll_0x155_341;
    /// if (v8 < 256) … = v8 + 28` is the FIRST half of the same `v40`
    /// block (`NETHERW.EXE` 0x57c67 `cmp byte [ebp-0x4],0x0` → 0x57c73
    /// `mov cx,[eax+0x155]` / 0x57c7a `cmp cx,0x100` / 0x57c83
    /// `add esi,0x1c` / 0x57c86 `mov [eax+0x155],si`, with `[ebx+0x82]
    /// = 0x50` at 0x57c8d), so the grabbed arms skip BOTH.
    ///
    /// It writes THE MOVER'S OWN BANK ACCUMULATOR: `sub_5D530` reads
    /// the SAME `a1x->dword_0xA4_164x->roll_0x155_341` for `roll +=
    /// rollDelta` (0x81d69 `mov ax,[ecx+0x4]` / 0x81d6d
    /// `add [ecx+0x155],ax`) and for the yaw rate `yaw += (roll -
    /// sign*7) >> 3` (0x81df9). One word, not two homes — that field
    /// IS the recorded `roll_acc` lane / [`crate::flight::Mc1State`]'s
    /// `roll_f`. A COUNT, not a flag: the ring walk can reach the
    /// not-yet-grabbed block more than once in a tick.
    pub bumps: u8,
    /// The human's pose (x, y, z, heading) at the HEAD of the tick's
    /// visits — `view.x/y/z/yaw`, the pose the funnel's slot read
    /// before any arm moved him. The faithful walk consumes `grab`
    /// as an absolute pose in the SAME tick, so it never needs this;
    /// the enhanced mover consumes one tick late (its move runs
    /// ahead of the world turn) and applies the seizure as the
    /// DELTA `grab − from` instead — an absolute pose applied late
    /// would rewind the flyer to last tick's position, and a
    /// far-band visit (tail publish, no displacement) would freeze
    /// him for as long as he stood inside the funnel's 12-tile
    /// ring. Deliberately NOT hashed: it is the pose lane the tick
    /// already digests.
    pub from: (u16, u16, i16, u16),
}

impl std::hash::Hash for PlayerWhirl {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.armed {
            state.write_u8(0x5C);
            state.write_u16(self.heading);
            state.write_i16(self.step);
            if let Some((x, y, z, yaw)) = self.grab {
                state.write_u8(0x5D);
                state.write_u16(x);
                state.write_u16(y);
                state.write_i16(z);
                state.write_u16(yaw);
            }
            state.write_u8(self.act80 as u8);
            state.write_u8(self.bumps);
        }
    }
}

/// See [`Gen::player_deflect_debit`] — the quarters the human's
/// REBOUND deflections owe this tick (:62725 and twins debit the
/// deflector's `+140` at the PROJECTILE's walk slot; the human's
/// purse lives outside the pool, so the walk accumulates here and
/// the wizard pass / tick tail drain it). Always drained by the
/// tick boundary, so it hashes transparent-at-pristine like
/// [`PlayerSpin`] and is deliberately NOT snapshotted.
#[derive(Default, Clone, Copy)]
pub(crate) struct DeflectDebit(pub u32);

impl std::hash::Hash for DeflectDebit {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0 != 0 {
            state.write_u8(0x5C);
            state.write_u32(self.0);
        }
    }
}

/// See [`Gen::mc2_night_shade`] — a bool that hashes to NOTHING when
/// false (hash-transparent).
#[derive(Default, Clone)]
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
#[derive(Default, Clone)]
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
#[derive(Clone)]
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

impl Default for TickChain {
    /// An unbuilt chain is INTACT (`cut == usize::MAX`), never blanked:
    /// the live tick-top rebuild sets exactly this, and a bare `Gen`
    /// (a unit rig that never ran a tick) must read the same — the
    /// derived `Default` gave `cut = 0`, which is the seizure blank's
    /// own signature ([`Gen::wiz_roster_head_blanked`],
    /// [`Gen::mc1_human_on_wiz_chain`]).
    fn default() -> Self {
        Self {
            list: Vec::new(),
            cut: usize::MAX,
        }
    }
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
#[derive(Default, Clone)]
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
        self.rebuild_mob_chains_for(false);
    }

    /// The MC2 shape of the same helper — 29 chains and the
    /// `actionIndex ∉ {0xB4, 0xE8, 0xEA}` exclusions, i.e. the arm
    /// `World::tick_inner`'s live sweep takes for
    /// [`crate::ids::GameId::Mc2`]. The MC1-sized (20) helper drops
    /// every model above 19, so an MC2 test that used it saw an EMPTY
    /// chain for the (5,25) Cymmerian and the (5,28) brute and passed
    /// vacuously.
    #[cfg(test)]
    pub(crate) fn rebuild_mob_chains_mc2(&mut self) {
        self.rebuild_mob_chains_for(true);
    }

    #[cfg(test)]
    fn rebuild_mob_chains_for(&mut self, mc2: bool) {
        let models = if mc2 { 29 } else { 20 };
        self.mob_chains.reset(models);
        for s in 1..self.ent.len() {
            let e = &self.ent[s];
            let chained = if mc2 {
                !matches!(e.tick70, 0xB4 | 0xE8 | 0xEA)
            } else {
                e.tick70 != 120
            };
            if e.class64 == 5 && e.act_life >= 0 && chained && (e.model65 as usize) < models {
                self.mob_chains.list[e.model65 as usize].push(s as u16);
            }
        }
    }

    /// ⭐⭐⭐ THE MC2 PER-MODEL ROSTER, AS RETAIL'S CONSUMERS SEE IT.
    /// `bytearray_38403x[model]` chased through `next_0` — the
    /// tick-top snapshot the case-5 arm of `UpdateEntities_57730`
    /// rebuilds (EF:40259-89). NETHERW.EXE, verbatim:
    ///
    ///   7bf5c  f6 43 0d 04   testb $0x4,0xd(%ebx)   <- THE REAP, and it
    ///   7bf63  e8 b8 07 00 00  call 0x7c720          runs BEFORE the rebuild
    ///   7bf89  6a 74         push $0x74             <- 29 * 4 bytes
    ///   7bf92  05 03 96 00 00  add $0x9603,%eax      <- bytearray_38403x
    ///   7bf98  e8 b3 4a 03 00  call 0xb0a50          <- memset(head, 0, 116)
    ///   7bffe  83 78 08 00   cmpl $0x0,0x8(%eax)    <- life_0x8 < 0
    ///   7c002  0f 8c ..      jl   0x7c189               -> not a member
    ///   7c00b  80 fb e8      cmp  $0xe8,%bl         <- actionIndex
    ///   7c010  0f 86 ..      jbe  0x7c189               == 0xE8 -> out
    ///   7c016  80 fb ea      cmp  $0xea,%bl
    ///   7c01b  80 fb b4      cmp  $0xb4,%bl
    ///   7c01e  0f 84 ..      je   0x7c189               0xB4/0xEA -> out
    ///   7c024  0f be 58 40   movsbl 0x40(%eax),%ebx <- model, SIGN-extended
    ///   7c037/7c043                                 <- append (tail or head)
    ///   7c04f  89 18         mov  %ebx,(%eax)       <- next_0 = Entities[0]
    ///
    /// and THAT IS THE WHOLE PREDICATE. Class, model, life, action AND
    /// the reap flag were all settled at that moment, so a walker that
    /// re-asks them is wrong in BOTH directions — and a `flags & 0x400`
    /// (reap-pending) test is an invented guard retail has no trace of:
    /// the reap at 0x7bf5c already ran, so a record flagged MID-tick
    /// stays a member for the rest of the frame. There is no bound
    /// check on the model either (`lea (,%ebx,4)` straight into a
    /// 29-entry array); the port's `< 29` cap is a safety guard on a
    /// case the assets never produce.
    ///
    /// Every MC2 site whose retail twin loads a chain head must read
    /// THIS, never the pool. `MGC_NO_MC2_MOB_CHAIN_PREDICATE` restores
    /// the pool scans at each call site.
    pub(crate) fn mc2_roster(&self, model: u8) -> &[u16] {
        self.mob_chains.visible(model as usize)
    }

    /// The mana-ball arm of the same tick-top sweep (:52290-97) —
    /// the twin of [`Self::rebuild_mob_chains`] for tests that drive a
    /// bare `Gen` instead of `World::tick` (whose top sweep at
    /// world.rs:2702 is the only non-test builder).
    ///
    /// ⚠⚠ **THIS TEST-ONLY TWIN DRIFTED FROM THE REAL SWEEP.** The
    /// production builder admits MC2's **model 57** (the fool's
    /// sphere — `dword_38523`, EF:40023-62, the third member of the
    /// sphere family the whirlwind victim list also names,
    /// `10 => matches!(c.model65, 13 | 14 | 39 | 57)` in
    /// `mc2::tail`), and this one did not. A test that minted a 57
    /// and called this to publish it got an **empty chain**, so every
    /// assertion downstream of "the sphere is a member" was vacuous.
    /// The gate is the MC2 column, exactly as in `World::tick`'s
    /// sweep — an MC1 world's model 57 is an unrelated logic model
    /// and must not join.
    #[cfg(test)]
    pub(crate) fn rebuild_ball_chain(&mut self) {
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        self.ball_chain.list.clear();
        self.ball_chain.cut = usize::MAX;
        for s in 1..self.ent.len() {
            let e = &self.ent[s];
            if e.class64 == 10 && (matches!(e.model65, 39 | 40) || (mc2 && e.model65 == 57)) {
                self.ball_chain.list.push(s as u16);
            }
        }
    }

    /// The class-10 model-45 arm of the same sweep (:52301-11) — the
    /// HOUSE roster `var_u32_36462[2]` (MC2's `dword_38527`), for
    /// tests that drive a bare `Gen`. NO life or flags test: the
    /// walkers carry none either, so membership is the whole gate.
    #[cfg(test)]
    pub(crate) fn rebuild_bldg_chain(&mut self) {
        self.bldg_chain.list.clear();
        self.bldg_chain.cut = usize::MAX;
        for s in 1..self.ent.len() {
            let e = &self.ent[s];
            if e.class64 == 10 && e.model65 == 45 {
                self.bldg_chain.list.push(s as u16);
            }
        }
    }

    /// The CLASS-3 arm of the same sweep (:52253-62) — bucket[0], for
    /// tests that drive a bare `Gen`. Membership is sampled ONCE, at
    /// the tick top: `actLife >= 0 && (flags & 0x10) == 0`.
    #[cfg(test)]
    pub(crate) fn rebuild_wiz_chain(&mut self) {
        self.wiz_chain.list.clear();
        self.wiz_chain.cut = usize::MAX;
        for s in 1..self.ent.len() {
            let e = &self.ent[s];
            if e.class64 == 3 && e.act_life >= 0 && e.flags & 0x10 == 0 {
                self.wiz_chain.list.push(s as u16);
            }
        }
    }

    /// The CLASS-9 arm of the same sweep (:52279) — `var_u32_36462[3]`,
    /// for tests that drive a bare `Gen`. Membership is every class-9
    /// record, NO life or flags test.
    #[cfg(test)]
    pub(crate) fn rebuild_proj_chain(&mut self) {
        self.proj_chain.list.clear();
        self.proj_chain.cut = usize::MAX;
        for s in 1..self.ent.len() {
            if self.ent[s].class64 == 9 {
                self.proj_chain.list.push(s as u16);
            }
        }
    }
}

/// One sound request: engine sound id (the SNDS bank-0 index), the
/// emitter's position on the u16 torus, and its slot as the instance
/// tag (the original's entity+24). `player` marks requests the
/// original issued against the player's own entity (full volume,
/// center pan, and the gate for the player-only ids 4/14/17/29).
/// See [`Gen::crt_rand`] — MC1's phase is RECOVERED (it is Watcom's
/// own `_RWD_randnext = 1`, because MC1 never calls `srand()`), but
/// MC2's is still open, and the stream stays hash-silent OUTRIGHT
/// either way: a mid-level import cannot carry a count of draws the
/// recording never sampled (the ⚠⚠ `derive(Hash)` trap on
/// [`Gen::mc2_mobilize`]: `crt_rand: _` in `snap_write` is only the
/// SAVE opt-out; a plain field here moved every golden in both games).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CrtRand(pub u32);

impl std::hash::Hash for CrtRand {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// [`Gen::mc2_pinned`]'s hash opt-out — a mirror of
/// `World::mc2_carpet_slot`, which `World::state_hash` itself lists
/// as `mc2_carpet_slot: _`. Hashing the mirror when the original is
/// hash-quiet would be incoherent, and because `Gen` is
/// `#[derive(Hash)]` a plain field moved every golden in BOTH games.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Mc2Pinned(pub u16);

impl std::hash::Hash for Mc2Pinned {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// [`Gen::mc1_pinned`]'s hash opt-out — the MC1 twin of [`Mc2Pinned`]
/// (`World::mc1_carpet_slot` is hash-quiet too, `state_hash` lists it
/// as `mc1_carpet_slot: _`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Mc1Pinned(pub u16);

impl std::hash::Hash for Mc1Pinned {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// ⭐⭐⭐ WHERE THE OUT-OF-POOL HUMAN SITS IN HIS OWN TILE CHAIN.
///
/// Retail's carpet is an ordinary linked record: `sub_41CF0` (:52468)
/// / `AddEventToMap_57D70` (EF:40315-27) HEAD-INSERT on tile entry and
/// `sub_41C70` (:52442) relinks only ACROSS tiles, so a chain is
/// ordered most-recently-entered FIRST and the carpet's rank is
/// simply "who entered this tile after I did". `sub_11980` (:16988)
/// returns the FIRST overlapping member, so that rank decides which
/// victim a bolt eats.
///
/// The port carries the human OUT OF POOL, so he is spliced out of
/// every chain and his rank has to be carried here: `next` is the
/// slot that FOLLOWS him in `cell`'s chain (0 = he is the tail).
/// Maintained by exactly three events — he enters a new tile (he
/// becomes that chain's head, [`Gen::player_relink`]), his successor
/// unlinks ([`Gen::unlink`] hands the seat on), and a record links
/// into his cell (head insertion, so it lands AHEAD of him and `next`
/// does not move).
///
/// Hash-silent OUTRIGHT, like [`CrtRand`] and [`Mc2Pinned`]: `Gen` is
/// `#[derive(Hash)]` and a plain field would move every golden in both
/// games for a lane that is pure bookkeeping.
///
/// Since the multiplayer stage 2 his record is REALLY linked at the
/// seat, so walkers meet him at the record ([`Gen::human_seat_at`]):
/// `next` is then just the record's own `+20`, kept for the record-less
/// virtual seat, and `cell` names the chain that holds him (the mirror
/// moves his x/y at points where retail does not relink).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlayerChain {
    /// The tile whose chain `next` indexes; `usize::MAX` = unseeded,
    /// which forces the first [`Gen::player_relink`] to take a head.
    pub cell: usize,
    /// The human's chain SUCCESSOR in `cell` — 0 = chain tail.
    pub next: u16,
}

impl Default for PlayerChain {
    fn default() -> Self {
        Self {
            cell: usize::MAX,
            next: 0,
        }
    }
}

impl std::hash::Hash for PlayerChain {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// [`Ent::lease2e`]'s hash opt-out — retail's `word_0x2E_46`, the
/// class-5 charm/summon lease, given its own home so it stops
/// sharing `f26` with `dword_0x10_16`. Hash-silent OUTRIGHT: `Ent`
/// is `#[derive(Hash)]`, so a plain `i16` would move every golden
/// with zero behaviour change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Lease2e(pub i16);

impl std::hash::Hash for Lease2e {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// [`Ent::morph_cry`]'s hash opt-out — the METAMORPH PUPPET'S CRY
/// CADENCE, a PORT INVENTION with no retail counterpart, given its
/// own home so it stops squatting in retail's `word_0x2E_46`.
/// Hash-silent OUTRIGHT for the same reason [`Lease2e`] is, and off
/// the snapshot wire like [`Raw48`] — a sound cadence is in no graded
/// lane and re-arms within 24 ticks of a load.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MorphCry(pub i16);

impl std::hash::Hash for MorphCry {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// A/B toggle for MOVING THE MORPH CRY OFF RETAIL'S @0x2E: set
/// `MGC_NO_MC2_MORPH_CRY_OFF_2E` to restore the pre-dig behaviour,
/// where [`Gen::mc2_morph_body`]'s 24-tick cry loop stored its
/// countdown in [`Ent::lease2e`] — the port's home for retail's
/// `word_0x2E_46`.
///
/// ⭐⭐ THE PORT PUBLISHED AN INVENTED WORD INTO A RETAIL LANE.
/// StageVar2 12's retail worker is `sub_1E4D0` (`sub_1D5D0`'s
/// `case 0xC` arm), and the shipped `NETHERW.EXE` at file
/// **0x42CD0-0x42D77** is twelve instructions — the parent-liveness
/// test, the three position words, the per-model z offset,
/// `CopyEntityPosition_57CF0`, then `mov %dx,0x20(%esi)` /
/// `mov %dx,0x1c(%esi)`. **There is no store to +0x2E anywhere in
/// it**, which is exactly why dig 98-Q20 parked the cry counter
/// there. But "retail never writes it" is not "retail reads 0 there":
/// the puppet's @0x2E is whatever its ctor left, and on every witness
/// in the corpus that is **0**, so the parked counter shows up as a
/// permanent `retail 0 / port 24` in the `f2e` lane of every morph
/// model.
///
/// WITNESSES — free-run `MGC_RAW_SHADOW=1`, `f2e`, every row
/// `retail 0 / port 24`: `(5,16)` 17,089 rows over 5 takes (mc2l24
/// t=24208 slot 619), `(5,2)` 4,634 over 2 (mc2l22 t=1199 slot 673,
/// 20 slots), `(5,25)` 1,722 over 5, `(5,17)` 1,130, `(5,19)` 577,
/// `(5,23)` 407, `(5,0)` 334, `(5,20)` 51 — the `_ =>` arm of the cry
/// table is why the model list is open-ended.
pub(crate) fn no_mc2_morph_cry_off_2e() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_MORPH_CRY_OFF_2E").is_some())
}

/// A/B toggle for THE MORPH PUPPET'S OWN ROLL: set
/// `MGC_NO_MC2_MORPH_PUPPET_ROLL` to restore the pre-dig
/// `roll = yaw = parent yaw`. ⚠ THE NOTE ABOVE MIS-READ `sub_1E4D0`'s
/// TAIL — it is FOUR instructions, not two, and `dx` is reloaded
/// between the pairs, so the puppet's `roll_0x20_32` is the PARENT's
/// `roll_0x20_32` and only `yaw_0x1C_28` comes from the parent's yaw.
/// Full disassembly and the witness list sit at the store in
/// [`Gen::mc2_metamorph_creature_tick`]. (Round 148, dig w148t.)
pub(crate) fn no_mc2_morph_puppet_roll() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_MORPH_PUPPET_ROLL").is_some())
}

/// A/B toggle for THE HUMAN CASTER'S half of that law: set
/// `MGC_NO_MC2_HUMAN_PUPPET_ROLL` to restore the pre-dig
/// `roll = view.yaw` the human arm of
/// [`crate::engine::features::Gen::mc2_metamorph_creature_tick`] fell
/// back to. ⚠ ROUND 148 LANDED THE POOLED HALF ONLY — `sub_1E4D0`
/// copies `[ebx+0x20]` whoever the parent is, and the human's carpet
/// IS a pool record in retail, so the human's puppet takes the human
/// carpet's @0x20 exactly like a rival's does. What made the human
/// arm look unreconstructible is that @0x20 on a WIZARD is not a
/// flight quantity at all: only the death spin writes it, so it is
/// dead residue (or 0) for the whole of a normal life. It now rides
/// [`Gen::human_roll_0x20`], seeded from the recorded carpet at every
/// conformance anchor. Witnesses: mc2l0-spells-galore retail 0 /
/// port 800·1304·35 (1,297 rows over three models), mc2l24 retail
/// **1966** / port 1053·786·104 (957 rows, ONE frozen word seen
/// through three models), mc2l22 retail 82 (1,302), mc2l6-rsg retail
/// 15 (1,037). Round 149, dig w149b.
pub(crate) fn no_mc2_human_puppet_roll() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_HUMAN_PUPPET_ROLL").is_some())
}

/// A/B toggle for THE WRITER behind [`Gen::human_roll_0x20`]: set
/// `MGC_NO_MC2_HUMAN_DEATH_SPIN_AIM` and the seat keeps whatever the
/// last conformance anchor put in it, which is what round 149's dig
/// w149b left behind — a seeded value with nothing to move it, so a
/// FREE RUN that crosses a human death carried the anchor's residue
/// for the rest of the take.
///
/// ⭐⭐⭐ THE DEATH SPIN IS THE ONLY WRITER OF A WIZARD'S @0x20, AND
/// THE PORT ALREADY COMPUTED THE VALUE AND THREW IT AWAY.
/// `sub_5E6C0` (EF:60600-60646) is the human corpse's whole tick, and
/// its killer branch opens with the look-at pair:
/// `roll_0x20_32 = sub_581E0_maybe_tan2(&self.position, &killer.position)`
/// then `fov_0x22_34 = sub_58210_radix_tan(...)`, and only THEN the
/// two `sub_58350(.., 5, 0x16)` servos that walk yaw/pitch onto them.
/// Shipped `NETHERW.EXE`, linear 0x5E6C0 (file **0x82EC0**):
///
/// ```text
///   82ec9  66 8b 53 24        mov  dx,[ebx+0x24]    ; the killer latch
///   82ed2  75 02              jne  0x82ed6          ; 0 -> the +5 spin arm
///   82ee3  66 8b 7b 24        mov  di,[ebx+0x24]
///   82ee7  8b 3c bd e4 a3 01 00  mov edi,[edi*4+0x1a3e4]  ; Entities[killer]
///   82eee  83 c7 4c           add  edi,0x4c         ; &killer.position
///   82ef3  e8 e8 9a ff ff     call 0x7c9e0          ; sub_581E0 tan2
///   82efd  66 89 43 20        mov  [ebx+0x20],ax    ; <- THE STORE
///   82f01  e8 0a 9b ff ff     call 0x7ca10          ; sub_58210 radix_tan
///   82f1b  66 89 43 22        mov  [ebx+0x22],ax    ; @0x22, write-only
///   82f0f  66 8b 7b 20        mov  di,[ebx+0x20]    ; servo target
///   82f1f  e8 2c 9c ff ff     call 0x7cb50          ; sub_58350(yaw,@20,5,22)
///   82f69  66 83 43 1c 05     addw $0x5,0x1c(%ebx)  ; the KILLER-LESS arm —
///                                                   ; no @0x20 store at all
/// ```
///
/// The killer-less arm writes neither word, which is why a corpse
/// that killed itself keeps the previous death's residue.
///
/// ⚠ THE HUMAN COLUMN ONLY. The single `e8` caller of 0x82EC0 in the
/// whole image is at file 0x83072, inside `sub_5E7C0`'s
/// `IsAiPlayer != 1` arm (`cmpb $0x1,0x2be7(%edx,%ebx,1)` /
/// `jne 0x83066` at file 0x82FEB) — a RIVAL corpse takes the other
/// fork (the 1200-tick `dword_0x10_16` countdown) and never spins.
/// Measured: on mc2l24 / mc2l22 / mc2l6-rsg / mc2l19 every single
/// `@0x20` change on the HUMAN carpet lands at `action45 == 3`
/// (mc2l24 slot 116: 145/145, mc2l22 slot 424: 90/90, mc2l6-rsg slot
/// 343: 9/9, mc2l19 slot 318: 82/82), while the seven rival carpets
/// of mc2l22 move theirs 79,904 times with **zero** of them at
/// action 3 — their brain owns that word, not this law.
///
/// ⚠ @0x22 IS NOT SEATED. `sub_1E4D0`'s metamorph copy reads `0x20`
/// and `0x1C` only (file 0x42D54 `mov dx,[ebx+0x20]` / 0x42D58 `mov
/// [esi+0x20],dx` / 0x42D5C `mov dx,[ebx+0x1c]`), and `sub_5E6C0`'s
/// own `@0x22` read feeds a pitch servo whose result the next
/// statement overwrites with 0 (file 0x82F86 `movw $0x0,0x1e(%ebx)`).
/// Nothing else in the port reads the human's @0x22, so it has no
/// seat. Round 149, dig w149k.
pub(crate) fn no_mc2_human_death_spin_aim() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_HUMAN_DEATH_SPIN_AIM").is_some())
}

/// A/B toggle for THE MORPH PUPPET'S CTOR @0x10: set
/// `MGC_NO_MC2_MORPH_KEEPS_CTOR_10` to restore the pre-dig `f26 = 0`
/// that both metamorph creation paths ([`World::mc2_cast_metamorph`]
/// and the rival twin in `mc2/rivals.rs`) wrote over the class-5
/// ctor's `dword_0x10_16 = (this - Entities) % 100`.
///
/// ⭐ THE ZERO WAS THE OLD CRY-TIMER SEED. Dig 98-Q20 parked the
/// port's invented scream loop in @0x10, and round 148's
/// [`no_mc2_morph_cry_off_2e`] moved it to its own hash-silent home —
/// but the `= 0` seed at the creation site stayed behind. Retail's
/// `sub_6A030` (EF:56644-56690) makes NO `dword_0x10_16` store at all:
/// it mints the body through `_4A190(&caster.position, 5, model)` and
/// then writes only `StageVar2_0x49_73`, `actionIndex_0x45_69`,
/// `parentId_0x28_40`, `id_0x1A_26`, the caster's `word_0x96_150` and
/// two flag bytes. The fourteen class-5 creature ctors all seed
/// `dword_0x10_16 = (this - D41A0_0.struct_0x6E8E) % 100` (the
/// animation-phase de-sync; EF:20773, 33731, 33788, 33833, 33881,
/// 33948, 34072, 34108, 34164, 34198, 34235, 34271, 34306, 34343),
/// and `sub_1E4D0` never touches it, so a puppet carries that seed for
/// its whole life.
/// WITNESS mc2l22: TWENTY (5,2) puppets, every one of them
/// `scratch10` = its own slot mod 100 in retail and 0 in the port —
/// 673→73, 953→53, 729→29, 965→65, 947→47, 922→22, 958→58, 957→57,
/// 951→51, 616→16, 29→29, 454→54, 903→3, 979→79, 939→39, 929→29,
/// 919→19, 871→71, 878→78, 912→12. 4,629 rows over 2 takes on (5,2)
/// plus 402 on mc2l0-spells-galore's (5,19) slot 28.
pub(crate) fn no_mc2_morph_keeps_ctor_10() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_MORPH_KEEPS_CTOR_10").is_some())
}

/// [`Ent::raw48`]'s hash opt-out — retail's `+48` word kept for every
/// class so the UNGUARDED owned-token readers (`sub_14E60`'s callers)
/// can read it off a recycled slot. Hash-silent OUTRIGHT for the same
/// reason [`Lease2e`] is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Raw48(pub u16);

impl std::hash::Hash for Raw48 {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// [`Ent::link42`]'s hash opt-out — the castle workers' `+42` castle
/// link. Hash-silent OUTRIGHT for the same reason [`Raw48`] is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Link42(pub u16);

impl std::hash::Hash for Link42 {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// A/B toggle for **THE CASTLE WORKERS FOLLOW THEIR `+42` LINK**
/// (round 154, w154j): set `MGC_NO_MC1_WORKER_CASTLE_LINK` to restore
/// the site scan ([`Gen::castle_at_site`]) at all four consumers —
/// the painter's shake suspend and finish, the leveler's shake gate
/// and finish. Retail: `sub_47020` :56104-11 / CARPET.EXE 0x47041
/// `mov %al,0x47(%ebx)` (+71), 0x47047 `mov %ax,0x18(%ebx)` (+24),
/// 0x4705F `mov %ax,0x2a(%ebx)` (+42 = `(castle − pool) / 164`);
/// `sub_47080` the same three stores at 0x470A1/0x470A7/0x470BF; the
/// upgrade commit :56486 `*(v2+42) = v1`. The consumers dereference
/// the link with no class test (`sub_285C0` :30520, `sub_28200`
/// :30333/:30419-27). The link is also the shadow's `f42` lane on
/// (10,41)/(10,42) — the switch does not affect the stamp, only the
/// resolve.
pub(crate) fn no_mc1_worker_castle_link() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_WORKER_CASTLE_LINK").is_some())
}

/// A/B toggle for **THE RIVAL CAST GATE READS THE PURSE SIGNED** (round
/// 155, w155b): set `MGC_NO_MC1_RIVAL_CAST_SIGNED_PURSE` to restore the
/// unsigned `mana < cost` at the two port homes of retail's rival cast
/// gate — readiness `sub_15A00` (`rival_cast_ready`) and the
/// castle commit `sub_155F0` case 0x10 (`rival_cast_castle`). Every
/// mana test in both functions is `mov 0x8c(%ebx),%reg` /
/// `cmp 0x88(%tok),%reg` / **`jl`** — a SIGNED compare of the wizard's
/// `+140` purse against the token's `+136` price: readiness case 0x10
/// unbound arm CARPET.EXE VA 0x15B80-0x15B8C (file 0x2E378:
/// `8b 83 8c 00 00 00 3b 86 88 00 00 00 0f 8c a1 01 00 00`), the bound
/// arm 0x15BF0, the default arm 0x15D1C (`7c 09`), and every other case
/// (0x15A36/0x15A86/0x15AD6/0x15C8D); the commit's case 0x10 VA
/// 0x15822-0x1582E (file 0x2E01A: `8b 93 8c 00 00 00 3b 90 88 00 00 00
/// 0f 8c 72 01 00 00`) and its cases 0/0xF 0x15664, 3/7/… 0x15761,
/// 0x158D2. A purse the fatal tick's shield quarter wrapped NEGATIVE
/// (`sub_46540` :55703 RAW subtract, preserved by the death return)
/// therefore refuses EVERY cast; the port's `u32` purse read it as
/// ~4.29e9 and passed. Witness mc1l31 t=29686: rival 729 dies (life
/// 80 → −320, purse 0 − 1600/4 = −400) and the port's castle-less rival
/// still ran the free plant (`sub_373F0(+150, 3, 2)`), popping a
/// `(3,2)` at 955 whose first dispatch popped a `(10,42)`; the two
/// extra pops shifted both lightning beams' segment chains (slots
/// 963/983) two slots down the free stack — the `(10,23)` endpoint
/// blast landed in 646 instead of 648.
pub(crate) fn no_mc1_rival_cast_signed_purse() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_CAST_SIGNED_PURSE").is_some())
}

/// A/B toggle for **THE POVERTY LATCH READS THE PURSE SIGNED** (round
/// 156, w156b — the third home of the `MGC_NO_MC1_RIVAL_CAST_SIGNED_PURSE`
/// convention): set `MGC_NO_MC1_RIVAL_POVERTY_SIGNED_PURSE` to restore
/// the `u32` compares in [`crate::World::rival_attack_pick`]'s latch.
/// Retail's two attack pickers `sub_16030` (caller 0x13E5C, the wizard
/// attack `sub_13DD0`) and `sub_16310` (caller 0x13D4B, the castle raid
/// `sub_13CA0`) open with byte-identical latch heads — CARPET.EXE VA
/// 0x16036 (file 0x2E82E): `8b 83 88 00 00 00` `mov 0x88(%ebx),%eax`
/// (the `+136` ceiling), `89 c2 c1 fa 1f c1 e2 02 1b c2 c1 f8 02` (the
/// SIGNED `/4`, `sar` with the negative bias), `3b 83 8c 00 00 00 7e 11`
/// = `cmp 0x8c(%ebx),%eax; jle` — **signed** `max/4 > +140` latches;
/// the release leg 0x16072-0x1609C is `add $0x1770` / `cmp %esi,%eax;
/// jge` / `cmp 0x8c(%ebx),%eax; jg` / `sar $1` / `jg` — all signed
/// (twin 0x16316-0x1637C). The purse is negative in retail exactly on
/// the FATAL tick: the shield-quarter debit (`sub_46540` :55703) is a
/// raw subtract, the death arm returns from `sub_132B0` before the
/// floor (:17980-83 vs :18017), and the brain's state handler still
/// runs (`rival_dispatch_tail`). Retail then LATCHES (−400 < max/4);
/// the port's `u32` read the wrapped purse as ~4.29e9 and RELEASED a
/// standing latch — and `+406` survives the respawn (`sub_44D30` never
/// clears it), so the reborn rival attacked while retail's still saved.
pub(crate) fn no_mc1_rival_poverty_signed_purse() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_POVERTY_SIGNED_PURSE").is_some())
}

/// A/B toggle for **THE BULLY ARM READS BOTH PURSES SIGNED** (round
/// 156, w156b — the fourth home of the signed-purse convention): set
/// `MGC_NO_MC1_RIVAL_BULLY_SIGNED_PURSE` to restore the `u32`-widened
/// purses in [`crate::World::rival_pick_wizard_target`]'s bully leg.
/// Retail `sub_145B0` :18570-72 (sole caller 0x1376F) — CARPET.EXE VA
/// 0x14700-0x14729 (file 0x2CEF8): `mov 0xa0(%esi),%eax; mov $0xff,%edx;
/// movswl 0x20a(%eax),%eax; sub %eax,%edx; mov %edx,%eax; shl $5,%eax`
/// (`32·(255−agg)`), `8b 93 8c 00 00 00` (candidate `+140`), `8b 8e 8c
/// 00 00 00` (own `+140`), `01 d0 39 c8 7d 1b` = `add; cmp; jge` — a
/// **signed** `cand + 32·(255−agg) < mine`. A purse wrapped negative by
/// a fatal shield quarter (the corpse holds it until the respawn
/// re-mint) is POORER than everyone in retail — an unbound,
/// castle-knowing corpse is a bully victim, and the dying picker's
/// own negative purse bullies no one; the port's `u32` widened both
/// to ~4.29e9 and inverted both.
pub(crate) fn no_mc1_rival_bully_signed_purse() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_BULLY_SIGNED_PURSE").is_some())
}
/// A/B toggle for **THE VULTURE'S IDLE MOVER RUNS ON ITS DEATH TICK**
/// (round 155, w155d): set `MGC_NO_MC1_VULTURE_DEATH_MOVER` to restore
/// the pre-dig `Inbox::Dead` arm, which demoted a `(5,1)` in IDLE to
/// its DEATH state and returned without the wrapper's mover trailer.
/// Retail: m1's IDLE wrapper `sub_1B160` :22227-28 is `sub_19B10(a1x,
/// 6); sub_196E0(a1x);` — CARPET.EXE 0x1B165 `push 6; push ebx; call
/// 0x19B10` (0x1B168) then 0x1B170 `push ebx; call 0x196E0` (0x1B171)
/// with NO test between them, and only the re-aim below is gated `cmp
/// ah,6; jne` (0x1B17C). The shared core's lethal exit (`sub_19B10`
/// :21363 `actLife < 0` → :21379 `v7 = a2 + 4` → :21415 `sub_424F0(a1x,
/// v7)`) returns INTO the wrapper, so the dying tick
/// still steps, turns toward `+34` and ground-snaps. mc1l35 t=4522
/// slot 65: retail x 47087→47046, y 46736→46684, z 1259→1258, `+30`
/// 1832→1854 with `+70` 6→10; the port held the pre-tick pose. The
/// HIT arm already ran this trailer (mc1l32 t=31567); the DEATH arm
/// did not.
pub(crate) fn no_mc1_vulture_death_mover() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_VULTURE_DEATH_MOVER").is_some())
}

/// A/B toggle for **THE HUMAN'S TOKEN IS ITS `+42`, NOT THE REGISTER**
/// (round 155, w155d — the converse of `MGC_NO_MC1_TOKEN_OWNER_TAG`):
/// set `MGC_NO_MC1_TOKEN_TAG_FIRST` to restore the pre-dig
/// `mc1_token_seat`, which ran a class-12 token's human arm only
/// while `player.owned[spell]` named it. Every retail token machine
/// resolves its caster from the token's own `+42` and never reads the
/// `+676` register — `sub_56380` (speed, dispatch row 0x06 :4964)
/// :65141-44: `if (+48 > 0) { v1 = pool[+42]; if (v1 > pool) … }`,
/// CARPET.EXE 0x56386 `cmpw $0,0x30(%ebx); jle` / 0x56393 `mov
/// 0x2a(%ebx),%ax` → ×164 + 0x7463 → `call sub_55DD0` (0x563C6). The
/// register is rebuilt BLIND every tick (`sub_45C10`) from the
/// acquisition list, so a recycled acq slot can steal `owned[m]`
/// from a live token. mc1l35 t=45279→45280: slot 111 (the human's
/// spell-13 acq entry) is re-minted as a `(10,2)`, `owned[2]` 75 →
/// 111, and retail's token 75 (`+42` 117) keeps its `+48` countdown
/// (218 → 217), its mid-burst regen pin (mana parked at 641725) and
/// its 4-tick `(10,2)` contrail through t=45331 — 52 reset heads
/// (`player.mana` +321 and a missing `(10,2)` every 4th tick).
pub(crate) fn no_mc1_token_tag_first() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_TOKEN_TAG_FIRST").is_some())
}

/// A/B toggle for **THE RIVAL GOES HOME TO HIS CASTLE REGISTER,
/// WHATEVER LIVES IN THE SLOT** (round 155, w155a): set
/// `MGC_NO_MC1_HOME_CASTLE_REGISTER` to restore the pool scan
/// ([`crate::World::rival_castle`] — lowest live `(3,2)` with `+24` =
/// the rival) in the selector's two HOME arms AND the Home handler.
/// Retail's flee-home predicate `sub_14310` (`reference/remc1/
/// sub_main.cpp` :18480-90) and the idle HOME leg `sub_14DC0`
/// (:18749-61) both resolve the castle as `pool + 164 * wizext[+50]`
/// with an INDEX-NONZERO test alone — CARPET.EXE file 0x2CB1F `mov
/// 0xa0(%ebx),%edx; mov 0x32(%edx),%dx` (wizext+50), 0x2CB54 `cmp
/// %eax,%ecx; jbe` (slot 0 = no castle), 0x2CB58.. `idiv $0xa4` →
/// `mov %ax,0x92(%ebx)` (+146) and `call sub_15420` → +148;
/// `sub_14DC0` carries the identical sequence at file 0x2D5C6/
/// 0x2D5FB/0x2D610/0x2D617. The Home handler `sub_13A70` (:18204-27)
/// reads the same register (file 0x2C26E `mov 0xa0(%ebx),%eax; mov
/// 0x32(%eax),%si`, 0x2C29F `cmp %eax,%esi; jbe` = castle-less arm),
/// casts 12 (0x2C2D8), then gates on the SIGNATURE — 0x2C2E2 `call
/// 0x2DC38` (`sub_15440`: `sub_15420(slot) == +148`), 0x2C2EC `je` →
/// return 0 with no `+34` aim (0x2C30E) and no `sub_15470` approach.
/// No class, model, life or owner test anywhere: the register
/// outlives the castle it named, and the stamped `+148` is the
/// signature of whatever lived in the slot on the pick tick.
///
/// The port's scan finds no `(3,2)` once the ESTABLISHED slot has been
/// re-minted, so the rival falls through the ladder to the mana hunt
/// where retail flies "home" to whatever now lives in the slot.
/// Witnesses: mc1l27 t=10702 (wiz 4's register 937 is the HUMAN's
/// `(10,0)` fire — round 151's stale-recycle victim ate the castle;
/// retail `chase 937`, port `864` a mana ball), mc1l27 t=10709 (the
/// signature gate: without it `target_yaw 1361 vs 1304`), mc1l35
/// t=23419 (register 633, again a human `(10,0)`; port aimed at the
/// human wizard 117).
pub(crate) fn no_mc1_home_castle_register() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_HOME_CASTLE_REGISTER").is_some())
}

/// A/B toggle for **THE UPGRADE SPACE TEST WALKS THE TICK-TOP CLASS-3
/// ROSTER** (round 155, w155a): set `MGC_NO_MC1_UPGRADE_SPACE_CHAIN`
/// to restore the live-pool scan (`class 3 model 2 && !0x400`) in
/// [`Gen::castle_upgrade_space_ok`]'s castle-overlap loop. Retail's
/// `sub_12D10` (:17643-53) walks `var_u32_36462[0]` — CARPET.EXE
/// 0x2B536 `mov 0x1e408,%ebx; mov 0x8e6e(%ebx),%ebx` (the +36462
/// CLASS-3 head, [`Gen::wiz_chain`]), 0x2B547 `cmpb $0x2,0x41(%ebx)`
/// (model 2), 0x2B54D `cmp %edi,%ebx; je` (not self), 0x2B553 `call
/// sub_11950` (0x2A148 — the THREE-axis `sub_118C0` box test with
/// `movswl` extents and a 16-bit `sub %ax` difference), 0x2B574 `mov
/// (%ebx),%ebx` to the pool sentinel. The roster's membership is the
/// tick-top sweep's (:52253): `actLife >= 0 && (flags & 0x10) == 0` —
/// so a RAZED castle (`act_life = -1`, still `(3,2)`, `+70 = 4`, no
/// `0x400`) is invisible to retail's space test and blocked the
/// port's. The z axis is tested too (`+78`/`+84`), where the port
/// tested x/y only.
///
/// Witness mc1l34 t=2447: wiz 1's keep 896 sits at the ORIGIN, level
/// 1; the human's razed keep 712 at (60672,60672) `act_life -1` is
/// 4864 wrapped units away against a level-2 reach of 3328+1664 =
/// 4992. Retail admits the Upgrade (ai_state 13 → 1, `+146` 374 →
/// 896); the port's pool scan refused and left him hunting mana
/// (25 `slot 860 chase` heads t=2447..3194). Same routine on all
/// three retail call paths: the selector's Upgrade arm (:18426), the
/// cast-16 bound arm (:19315) and the level-up commit (:56055).
pub(crate) fn no_mc1_upgrade_space_chain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_UPGRADE_SPACE_CHAIN").is_some())
}

/// A/B toggle for **THE RIVAL RESPAWN'S RE-PRICE TAKES THE CASTLE
/// OWNER'S TOKEN** (round 155, w155a): set
/// `MGC_NO_MC1_RESPAWN_REPRICE_OWNER` to restore the pre-dig stamp
/// (the RESPAWNING rival's own `owned[16]`, `CASTLE_CAP[f26.clamp(0,
/// 7)]`, `/ 101`). Retail's rival respawn calls `sub_47DD0(pool[
/// wizext+50])` (`reference/remc1/sub_main.cpp` :55034-35), and
/// `sub_47DD0` (:56617-73) never looks at the caller: it resolves
/// `pool[castle.+24]`, requires that record's `+70 <= 1u`, takes ITS
/// `wizext+676[16]` (index test alone), prices it from the castle's
/// `+26` by a `switch` whose `default:` is 0 (no clamp — a level
/// outside 0..7 writes 0), and divides by the TOKEN's `+50`.
///
/// It only matters when the register names a slot the pool re-minted
/// (the stale-recycle victim class): mc1l35 t=23648, wiz 1 respawns
/// on register 633 — the HUMAN's `(10,0)` fire, `+26` 0 — so retail
/// re-prices the HUMAN's castle token 26 to 5000/49 and leaves wiz
/// 1's freshly minted token 50 at the ctor 1000/9; the port stamped
/// token 50 5000/49 and left 26 at 30000000/297029.
pub(crate) fn no_mc1_respawn_reprice_owner() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RESPAWN_REPRICE_OWNER").is_some())
}

/// A/B toggle for **THE HUMAN'S RESPAWN RE-PRICE IS THE RIVAL'S**
/// (round 156, w156c): set `MGC_NO_MC1_HUMAN_RESPAWN_REPRICE` to
/// restore the human arm's own stamp (gated on the register record's
/// `flags & 2`, level clamped `.max(0).min(7)`, `/ 101`). Retail has
/// ONE respawn, `sub_44D30` (:54802): the human reaches it from the
/// command processor's Space arm (case 0xF :48620-33, call at VA
/// 0x3CC7D), every rival from the dead-wait `sub_46480` (:55616, VA
/// 0x464DA) — its only two callers — and its tail (:55034-36) is `v28 =
/// wizext+50; if (v28) sub_47DD0(pool[v28])` — CARPET.EXE VA 0x4527A
/// `mov 0x32(%eax),%di`, 0x45281 `test %di,%di; je`, 0x452B1 `call
/// sub_47DD0` (its ONLY caller by `e8 rel32`), no flags test.
/// `sub_47DD0` (:56617-73, VA 0x47DD0-0x47EBC) prices the OWNER's
/// token (`pool[+24]`, `+70 <= 1u`, `wizext+676[16]` index test) by
/// an unsigned `+26` switch whose `default:` is 0 (0x47E56 `cmp
/// $7,%dx; ja` → 0x47EA2 `xor %edx,%edx`) and divides by the TOKEN's
/// `movswl +50` (0x47EA6). Round 155 landed this on the rival arm
/// ([`no_mc1_respawn_reprice_owner`]); the human arm kept the old
/// gate + clamp + `/101`. Both arms now share
/// `World::mc1_respawn_reprice`.
///
/// Replay-neutral on every witnessed human respawn (130 across 36 MC1
/// takes, all a live own `(3,2)`, flags 14, level 1..7, token `+50`
/// 101 — the two arms agree there); it only differs when the register
/// names a stale-recycle re-mint (the r151 victim class), which no
/// take has yet shown for the human. Pinned by unit.
pub(crate) fn no_mc1_human_respawn_reprice() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_HUMAN_RESPAWN_REPRICE").is_some())
}

/// [`Ent::summit10`]'s hash opt-out — retail's 32-bit
/// `dword_0x10_16` on the two SUMMIT CONTROLLERS, given its own home
/// so the arc counter stops being clipped by the `i16` [`Ent::f26`].
/// Hash-silent OUTRIGHT for the same reason [`Lease2e`] is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Summit10(pub i32);

impl std::hash::Hash for Summit10 {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// A/B toggle for the SUMMIT ARC COUNTER'S WIDE HOME: set
/// `MGC_NO_MC2_SUMMIT_ARC_WIDE` to restore the pre-dig behaviour,
/// where [`crate::engine::World::mc2_summit18_tick`] counted in the
/// `i16` [`Ent::f26`] with a `saturating_add` and the `scratch10` lane
/// published it.
///
/// ⭐ RETAIL'S COUNTER IS AN `int`, AND THIS ONE RECORD ACTUALLY
/// OVERFLOWS AN `i16`. `sub_32A70` (address banner `00032A70`,
/// EF:23928) is the (10,18) eruption-vortex controller and its last
/// statement is a bare `a1x->dword_0x10_16++` on the 32-bit word — the
/// ctor `sub_4EED0` stores it 32 bits wide too (shipped `NETHERW.EXE`
/// file 0x736F9: `c7 43 10 00 00 00 00  movl $0x0,0x10(%ebx)`, and the
/// (10,91) sibling `sub_4EF30` at file 0x73759 has the identical
/// store). The counter is only ever reset inside the `> 2500` restart
/// arm, and that arm is gated on `!D41A0_0.word_0x31` — the vortex
/// singleton register the live controller is itself holding — so a
/// latched summit NEVER restarts and just counts: retail reads
/// **79,064** on mc2l24-crazy slot 397 and **32,768+** on mc2l18 slot
/// 983, where the port's `i16` sat on 32,767.
///
/// ⚠ IT IS BEHAVIOURALLY INERT, VERIFIED FROM THE FUNCTION'S OWN
/// READS, NOT ASSERTED. `sub_32A70` reads @0x10 exactly five times:
/// `> 2500`, `< 128`, `& 0xF` (AND-ed after the `< 128`, so dead above
/// 127), `!x` (== 0) and `>= 127`. There is no `== N`, no modulo and
/// no shift of the raw word anywhere in it, and no other retail
/// function reads a (10,18)'s @0x10 — the only other toucher is the
/// same function's fast-expire of the PREVIOUS vortex
/// (`v5x->dword_0x10_16 = 250`), a write. Above 2,500 every one of
/// those five reads answers the same for 32,767 and for 79,064, so the
/// port's saturation was invisible in every graded lane and visible
/// only in the raw `scratch10` shadow: **108,759 rows over 2 takes /
/// 3 slots**, the round-148 free-run census's second-largest lane.
///
/// ⚠ `Ent::f26` IS NOT WIDENED. It stays the `i16` class-wide @0x10
/// home; the two summit models keep their authoritative count here and
/// mirror it into `f26` saturating, so every other reader of `f26` and
/// every `import_ent_mc2` arm is untouched.
pub(crate) fn no_mc2_summit_arc_wide() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SUMMIT_ARC_WIDE").is_some())
}

/// A/B toggle for the (10,91) APOCALYPSE-RAIN SUMMIT'S `@0x2A` SEED:
/// set `MGC_NO_MC2_RAIN_2A_SEED` to restore the pre-dig ctor, which
/// inherited the (10,18) sibling's `f140 = 200` and left the uniform
/// class-10 `@0x2A` seat `f44` at 0. Full citation and the shipped
/// bytes at [`crate::engine::World::mc2_spawn_summit91`].
pub(crate) fn no_mc2_rain_2a_seed() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RAIN_2A_SEED").is_some())
}

/// A/B toggle for THE RIVAL'S SPEED REGISTER, READ BLIND (round 167):
/// `MGC_NO_MC1_RIVAL_BLIND_SPEED=1` restores the validated token read
/// in the rival's travel and cruise helpers. Citation and the mc1hwl14
/// t=13453 witness at `World::rival_blind_speed_reg`.
pub(crate) fn no_mc1_rival_blind_speed() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_BLIND_SPEED").is_some())
}

/// A/B toggle for THE RIVAL'S ATTACK REGISTERS, READ BLIND (round 168):
/// `MGC_NO_MC1_RIVAL_BLIND_REGISTER=1` restores the static price table
/// and the validated token read in the rival's picker, readiness and
/// commit. Citation and the mc1hwl12 t=16994 witness at
/// `World::rival_reg`.
pub(crate) fn no_mc1_rival_blind_register() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_BLIND_REGISTER").is_some())
}

/// A/B toggle for THE RIVAL'S LANDING MAILBOX CLEAR (round 168): set
/// `MGC_NO_MC1_RIVAL_LANDING_MAIL_CLEAR` to leave a rival corpse's
/// mailbox standing at the touchdown. The class-3 fall handler
/// `sub_45FC0` is shared by the human and the rivals, and its landing
/// arm runs `memset(a1 + 90, 0, 36)` (remc1 sub_main.cpp:55518) ahead
/// of the book scatter; the port had the line on the human's arm only.
/// WITNESS mc1hwl0 t=6809, rival 473: the fatal letter `(6664, 472)`
/// rides the whole fall and reads `(0, 0)` on the landing record,
/// where the port's corpse kept it to the respawn (4.3 million
/// `mail0` shadow rows on 70 takes). The respawn grace wipes the box
/// again before any intake, so no graded lane moves.
pub(crate) fn no_mc1_rival_landing_mail_clear() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_RIVAL_LANDING_MAIL_CLEAR").is_some())
}

/// A/B toggle for THE CASTLE-LESS TELEPORT'S SAVED AXIS (round 168):
/// set `MGC_NO_MC1_TELEPORT_HOP_IN_PLACE` to leave the caster's own
/// x/y in the Teleport token's `+150/+152` after the random hop.
/// Witness mc1l0-spells-galore t=9071 at `World::cast_teleport`.
pub(crate) fn no_mc1_teleport_hop_in_place() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_TELEPORT_HOP_IN_PLACE").is_some())
}

/// A/B toggle for THE TRIGGER VOLUMES' WARP-AWARE POSE (round 168):
/// set `MGC_NO_MC1_TRIGGER_WARP_POSE` to probe MC1 / HW trigger
/// volumes with the pre-warp human pose. Witness mc1hwl17 t=42888 at
/// the class-11 arm of `World`'s walk.
pub(crate) fn no_mc1_trigger_warp_pose() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_TRIGGER_WARP_POSE").is_some())
}

/// A/B toggle for THE HATE LEDGER'S SELF ROW (round 168): set
/// `MGC_NO_MC1_SELF_HATE` to restore the port's exclusion of a
/// wizard's own colour in the projectile hate ledger. Witness
/// mc1hwl14 t=62 at `World::rival_add_hate`.
pub(crate) fn no_mc1_self_hate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_SELF_HATE").is_some())
}

/// A/B toggle for THE HUMAN TRUCE'S PLACE (round 168): set
/// `MGC_NO_MC1_HUMAN_TRUCE_AFTER_MINT` to stamp the respawn amnesty
/// before the book re-mint again. Citation and the mc1hwl5 t=5985 /
/// mc1l49 t=15297 witnesses at `World::player_respawn`.
pub(crate) fn no_mc1_human_truce_after_mint() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_HUMAN_TRUCE_AFTER_MINT").is_some())
}

/// A/B toggle for THE CASTLE MACHINE'S BLIND PIN (round 168): set
/// `MGC_NO_MC1_CASTLE_PIN_BLIND` to restore the validated owner-token
/// resolver on the castle's pin / release. `sub_46D20` (remc1
/// sub_main.cpp:55949) reads the owner's `wizext+708` word — the
/// `+676` register's entry 16 — tests it NONZERO and writes that
/// record's `+48`: `+50 − 1` of the same record on a pin (:55963-66),
/// 0 on a release (:55970). No class, no owner, no liveness. WITNESS
/// mc1l9 t=32429: castle 374's teardown pins for its dead owner 118,
/// whose stale register still names slot 10 — reborn that tick as one
/// of the castle's own `(10,39)` spheres. The sphere's `+50` is 0, so
/// its `+48` reads 65535 for its whole life (129 rows). The port's
/// resolver demanded a live `(12,16)` and wrote nothing.
/// ⭐ ORDER IS PART OF IT: the worker arms pin BEFORE they spawn (case
/// 3 `sub_46D20(a1, 1); sub_47020(a1)`, case 5 likewise, :56084-90),
/// the teardown pins AFTER its spheres (:56529). Same take, t=32281-87:
/// the register names the dead owner's FREED token — `+48` 0 -> 100 on
/// the class-0 record — and the painter the transform mints into that
/// very slot six ticks later reads 0, the pin having landed before
/// the allocator's memset. The three Gen-side releases
/// (`Gen::release_castle_charge_pin`) keep their owner-tag scan: no
/// take witnesses them through a stale register.
pub(crate) fn no_mc1_castle_pin_blind() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CASTLE_PIN_BLIND").is_some())
}

/// A/B toggle for the RAW `+48` TOKEN LANE: set
/// `MGC_NO_MC1_TOKEN_RAW48` to restore the pre-dig upgrade gate, which
/// read `Ent::f26` — retail's `+26` on anything that is not a live
/// class-12 manifestation — where `sub_14120` reads `+48`.
pub(crate) fn no_mc1_token_raw48() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_TOKEN_RAW48").is_some())
}

/// A/B toggle for **THE NATIVE HALF** of the raw `+48` token lane
/// (wave 121, dig D13): set `MGC_NO_MC1_TOKEN_RAW48_NATIVE` to
/// restore the import-only lane, where [`Gen::free_entity`] did not
/// carry a dying manifestation's `+48` across the `f26` → `raw48`
/// seam and the reader fell back to `f26` on a class-0 record.
///
/// ⭐ The distinction matters because the port has TWO homes for
/// retail's one word and retail has none — `+48` is just a word in a
/// 164-byte record. `sub_41E90` (`CARPET.EXE` 0x5A688) frees by
/// clearing the class byte alone, and `NewEvent_372C0` (0x4FAB8)
/// re-takes a slot with `memset(record, 0, 164)` at file 0x4FB5B, so
/// the word survives a free and is zeroed by an allocation. This
/// switch restores the pre-dig behaviour of the first half.
pub(crate) fn no_mc1_token_raw48_native() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_TOKEN_RAW48_NATIVE").is_some())
}

/// A/B toggle for **THE WAKE ARM'S `+48` STAMP** (round 154, dig
/// w154a): set `MGC_NO_MC1_WAKE_DY48` to restore the pre-dig awake
/// pass, which armed `+58 = 16` and left [`Ent::raw48`] at 0 on every
/// creature and mana ball — the round-153 raw-shadow census's largest
/// lane (726,669 pair rows / 39 takes, 21M+ free rows: `(5,x) f48` on
/// EVERY creature model and `(10,39)`/`(10,40)`).
///
/// ⭐ WHAT `+48` IS ON A CREATURE OR A BALL: **the absolute Y-distance
/// to the human carpet, sampled on the tick the record wakes** — a
/// write-only scratch with NO reader. `sub_54F80` (:64300, the
/// per-record half of the tick-top awake pass `sub_54F00` :64266)
/// takes its wake arm at :64353-59 when `+58 == 0 && +59 == 0`:
/// `v5 = sub_42410(rec+72, carpet+72)` (planar `dx²+dy²`), `if (v5 <
/// 37748736)` (24.0 tiles, the same gate the port already has),
/// `+48 = Distance_410CE(…)`, `+58 = 16`, followers `+58 = 18`.
///
/// The listing writes `Distance_410CE_4140E(v4)` with `v4` the
/// carpet SLOT — a decompiler artifact. The shipped bytes
/// (`CARPET.EXE` file 0x6D7E3-0x6D80C, VA 0x54FE3-0x5500C):
/// `e8 28 d4 fe ff  call sub_42410` · `3d 00 00 40 02  cmp
/// $0x2400000,%eax` · `7d 64  jge` · **`52  push %edx`** · `e8 d0 c0 fe
/// ff  call Distance_410CE` · `66 89 46 30  mov %ax,0x30(%esi)` · `c6
/// 46 3a 10  movb $0x10,0x3a(%esi)`. `Distance_410CE` (VA 0x410CE,
/// file 0x598C6) is the Newton integer sqrt and reads its argument
/// off the STACK (`8b 4d 08  mov 0x8(%ebp),%ecx`), so the argument is
/// whatever `edx` holds after `sub_42410` returns — and `sub_42410`
/// (VA 0x42410, file 0x5AC08) computes `dy = carpet.y − rec.y` as
/// `cwtl; mov %eax,%edx; imul %eax,%edx` and never touches `edx`
/// again: **`edx = dy²`**. So `+48 = isqrt(dy²) = |dy|`, the y-leg
/// alone. Witnesses: mc1l15 t=3817 slot 473 (5,15) `+48 0 → 2881`
/// with `+58 0 → 16`, creature y 14720, carpet y 11839 before its
/// move that tick (the pass runs at tick head, before the walk) —
/// 14720 − 11839 = 2881; mc1l15 t=1229 slot 275 (10,39) `+48 0 →
/// 1158`, ball y 14554, carpet y 15712 → 1158 (the planar distance
/// would have been 1458).
///
/// ⚠ NO READER. Every `+48` read in `CARPET.EXE`'s code segment was
/// enumerated by `0x30(%reg)` operand and mapped onto the listing's
/// address banners: the rival brain (`sub_13DD0`..`sub_16310`), the
/// human command processor (`sub_17C20`, `sub_448E0`), the mover
/// (`sub_46840`), the HUD/minimap/blit (`sub_23940`, `sub_3C800`,
/// `sub_48710`, `Blit_599B0`) all read the WIZARD record's `+48` (the
/// player index, `*(rec+160)+48`) or a class-12 token's `+48` through
/// `sub_14E60`; `sub_24DA0`'s reads are the (10,42) leveler's own
/// (`+48 += v28`, :30336, already homed); the token handlers
/// (`sub_559A0`..`sub_58400`) read their own record. The creature
/// handler range VA 0x193EE-0x22880 has only `0x30(%esp)` stack
/// slots. The value is therefore never consumed: this law closes a
/// shadow lane, not a behavioural hole, and lands in the hash-silent
/// [`Ent::raw48`] so no golden moves.
pub(crate) fn no_mc1_wake_dy48() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_WAKE_DY48").is_some())
}

/// A/B toggle for the SUMMON-LEASE LANE SPLIT (dig 98-Q20): set
/// `MGC_NO_SUMMON_LEASE_FIELD` to restore the one-word port, where
/// every class-5 @0x2E reader and writer shared `f26` with retail's
/// `dword_0x10_16` and the pair importer had to pick ONE of the two
/// retail words to seat per (class, model, StageVar2, action).
pub(crate) fn no_summon_lease_field() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_SUMMON_LEASE_FIELD").is_some())
}

impl Ent {
    /// Read retail's `word_0x2E_46` off a class-5 record. With the
    /// law off it is `f26`, exactly as before the split.
    #[inline]
    pub(crate) fn lease(&self) -> i16 {
        if no_summon_lease_field() {
            self.f26
        } else {
            self.lease2e.0
        }
    }

    /// Write retail's `word_0x2E_46` on a class-5 record.
    #[inline]
    pub(crate) fn set_lease(&mut self, v: i16) {
        if no_summon_lease_field() {
            self.f26 = v;
        } else {
            self.lease2e.0 = v;
        }
    }

    /// `word_0x2E_46 += d` (retail's `--` / `-= 4` legs).
    #[inline]
    pub(crate) fn add_lease(&mut self, d: i16) {
        let v = self.lease().wrapping_add(d);
        self.set_lease(v);
    }
}

/// `MGC_SOUND_TRACE=1` prints every sound REQUEST the sim makes, with
/// the `#[track_caller]` trigger site, the emitter's `(class, model)`
/// and its flags word — the instrument for "is this trigger still
/// firing?" questions. Sound is not a graded lane (it appears in
/// neither `ObsMc1` nor `EntObsMc2`, and neither `replay` nor
/// `verify-deltas` reads it), so a deleted trigger leaves the whole
/// conformance corpus green: this trace is the only census there is.
pub fn snd_trace_on() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_SOUND_TRACE").is_some())
}

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
            wiz_chain: TickChain::default(),
            proj_chain: TickChain::default(),
            bldg_chain: TickChain::default(),
            paint_chain: TickChain::default(),
            mob_chains: MobChains::default(),
            mc2_recycle: Mc2Recycle::default(),
            m27_v34_slot: M27V34Slot(None, false, None, None),
            mc2_pinned: Seats::one(Mc2Pinned(0)),
            mc1_pinned: Seats::one(Mc1Pinned(0)),
            mc1_guard_reg: Mc1GuardReg::default(),
            mc1_balloon_reg: Mc1BalloonReg::default(),
            human_seat: Seats::one(HumanSeat::default()),
            human_tag: Seats::one(HumanTag::default()),
            rand: seed,
            pseudo,
            crt_rand: CrtRand(1),
            spawn_count: [0; 20],
            player_mail: Seats::one([(0, 0); 6]),
            player_damage: Seats::one(0),
            erupting: 0,
            plume: 0,
            player_knock: Seats::one((0, 0)),
            mc1_buffet_post: HashSilent((0, 0)),
            player_spin: Seats::one(PlayerSpin::default()),
            player_whirl: Seats::one(PlayerWhirl::default()),
            ww_walk_published: Seats::one(HashSilent(false)),
            seat_view: Seats::one(HashSilent(SeatView::default())),
            m22_seg_residue: HashSilent(None),
            player_hurl: Seats::one(PlayerHurl::default()),
            player_flood_pull: Seats::one(PlayerFloodPull::default()),
            player_deflect_debit: Seats::one(DeflectDebit::default()),
            mc2_debuffs: Seats::one(Mc2PlayerDebuffs::default()),
            rival_ents: [0; 8],
            castle_reg: CastleReg::default(),
            mc2_life_scale: Mc2LifeScale::default(),
            player_aggro: Seats::one(0),
            rival_wanted: [0; 8],
            player_invisible: Seats::one(false),
            player_ghost: HashSilent(false),
            player_vel: Seats::one(HashSilent((0, 0))),
            mc2_flyers_clear: HashSilent(false),
            skeletons_convert_traders: HashSilent(false),
            human_roll_0x20: Seats::one(HashSilent(0)),
            player_rebound_reg: Seats::one(false),
            player_chain: Seats::one(PlayerChain::default()),
            kills: Seats::one(0),
            stats: HashSilent(Default::default()),
            shots: Seats::one(0),
            hits: Seats::one(0),
            player_danger: Seats::one(0),
            banked_houses: Seats::one(0),
            rival_banked_houses: RivalHouses::default(),
            castle_alert: Seats::one(0),
            player_alert: Seats::one(0),
            balloon_alert: Seats::one(0),
            pal_flash: PalFlash::default(),
            exhausted: 0,
            castle_watchdog_fired: HashSilent((0, 0)),
            sounds: Vec::new(),
            terrain_dirty: false,
            chassis,
            verbs,
            verb_fallbacks: 0,
            misfits: Vec::new(),
            mc2_night_shade: NightShade(false),
            mc2_spawn_ord: Mc2Ord::default(),
            mc2_player_drain: Seats::one(Mc2Quiet::default()),
            mc2_rival_leech: Mc2Quiet::default(),
            mc2_scrolls: Seats::one(Mc2Quiet::default()),
            mc2_spell_tokens: Seats::one(Mc2Quiet::default()),
            mc2_cast_xp: Mc2XpMail::default(),
            mc2_ladder_sync: Mc2LadderMail::default(),
            mc2_castle_lock_mail: Mc2LockMail::default(),
            bolt_fx: BoltFx::default(),
            volcano_blind: VolcanoBlind::default(),
            blast_hits: BlastHits::default(),
            mc1_aim_latch: HashSilent(0),
            mc2_beam_defer: BeamDefer::default(),
            mc2_pred_axis: Mc2PredAxis::default(),
            mc2_steal_mail: Mc2StealMail::default(),
            mc2_aura_claim: Mc2SlotMap::default(),
            mc2_wanted: Mc2SlotMap::default(),
            mc2_allied: Mc2SlotMap::default(),
            mc2_rebound_precise: Seats::one(Mc2Quiet::default()),
            mc2_castle_research: Mc2CastleResearch::default(),
            mc2_mobilize: Seats::one(Mc2Quiet::default()),
            mc2_slow: Seats::one(Mc2Echo::default()),
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
    #[track_caller]
    pub(crate) fn snd(&mut self, id: u8, i: usize) {
        let e = &self.ent[i];
        if snd_trace_on() {
            eprintln!(
                "SNDREQ id={id} site={} emitter=({},{}) slot={i} flags={:#x}{}",
                std::panic::Location::caller(),
                e.class64,
                e.model65,
                e.flags,
                if e.flags & 0x80 != 0 {
                    " DROP:silent-emitter"
                } else {
                    ""
                }
            );
        }
        self.sounds.push(SoundEvent {
            id,
            pos: (e.x, e.y, e.z),
            tag: i as u16,
            player: false,
        });
    }

    /// Emit a player-entity sound request (the original's calls
    /// against the wizard's own entity — full volume, center pan).
    #[track_caller]
    pub(crate) fn snd_player(&mut self, hx: Hx, id: u8) {
        if snd_trace_on() {
            eprintln!(
                "SNDREQ id={id} site={} emitter=PLAYER",
                std::panic::Location::caller()
            );
        }
        self.sounds.push(SoundEvent {
            id,
            pos: (0, 0, 0),
            tag: self.human_tag[hx].0,
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
        // Both games pop the free stack FIRST and only then sacrifice
        // a recycle victim (MC2 `NewEvent_4A050` Events.cpp:561-608 —
        // free :563, victim :581; MC1 `NewEvent_372C0` :43867-83 vs
        // :43885-908). MC1's stack is armed far beyond the respawn
        // window — the death LANDING's rebuild (:55487) arms it and
        // never disarms (SESSION 66's sacrifice law + SESSION 67's
        // recycle-stack import; the old "respawn window only, never
        // reached" reading is refuted in the ledger).
        let (idx, seized) = match self.free.pop() {
            Some(i) => (i, false),
            None => match self.mc2_recycle_pop() {
                Some(i) => (i, true),
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
            },
        };
        let idx = idx as usize;
        // ⭐ THE MC1 SEIZURE BLANKS EVERY TICK-TOP ROSTER (:43885-91,
        // hw:40294-301): before the victim is even unlinked, retail
        // memsets the 20 per-model heads (`str_36382x`) and nulls
        // `var_u32_36462[1]/[2]/[0]/[3]` — so every roster consumer
        // dispatched after the seizing slot sees EMPTY lists for the
        // rest of the tick, not just a severed chain at the victim.
        // ⭐⭐⭐ AND SO DOES MC2'S (`NewEvent_4A050`, Events.cpp:583-87).
        // The comment that used to sit here claimed MC2's arm had no
        // such blank; `NETHERW.EXE` 0x6E881 refutes it outright — the
        // recycle arm opens with
        //     push $0x74 / mov 0x41a4,%eax / add $0x9603,%eax / call memset
        // i.e. 116 bytes = **29** dword heads at `wizext+0x9603`
        // (`bytearray_38403x`), which is exactly MC2's
        // `chassis.bucket_models` (29, `chassis.rs:78`) — followed by
        // five `movl $0x0` at +0x9677/+0x967B/+0x967F/+0x9683/+0x9687 =
        // `dword_38519/38523/38527/38531/38535` (wiz / ball / bldg /
        // proj / PAINT). Note 0x9603 + 0x74 == 0x9677, so the memset and
        // the five stores are ONE contiguous 34-dword blank. All of it
        // lands BEFORE `SetMapEntity_57E50` (0x6E8E2) and before the
        // victim's `class = 0` (0x6E8EC).
        //
        // ⭐⭐⭐ THE READER LIST, NOT THE WRITER LIST, IS THE LAW: with
        // the heads standing, every walker dispatched later in the tick
        // (castle absorb, archer/guard acquire, the ch0 building
        // broadcast, the m12 site vetoes, the rival scans) acts on
        // entities retail cannot reach at all. mc2l22 t=1113 takes NINE
        // sacrifices in one tick, and the port's extra walking billed
        // castle 502 two hundred points of damage retail never billed —
        // an allocator law wearing a `(3,2) life` mask.
        // MC2's fifth head (`paint_chain`) has no MC1 twin, hence the
        // inner gate. `MGC_NO_MC2_SEIZE_BLANK=1` reverts the MC2 half.
        let mc2_seize = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        if seized && !(mc2_seize && no_mc2_seize_blank()) {
            self.ball_chain.cut = 0;
            self.wiz_chain.cut = 0;
            self.proj_chain.cut = 0;
            self.bldg_chain.cut = 0;
            if mc2_seize {
                self.paint_chain.cut = 0;
            }
            for m in 0..self.mob_chains.cut.len() {
                self.mob_chains.cut[m] = 0;
            }
        }
        // THE SEVERED CHAIN: reusing a freed record wipes its list
        // link, so retail walks of the tick-head ball chain stop at
        // this node for the rest of the tick. Measured: mc1l0 pair
        // 604→605 — the (9,1) lob reusing collected ball 642's slot
        // must not see balls 643+ (chase want 104, not the
        // closer-scoring 714 its own predecessor lob chases).
        if let Ok(pos) = self.ball_chain.list.binary_search(&(idx as u16)) {
            self.ball_chain.cut = self.ball_chain.cut.min(pos + 1);
        }
        // …and for bucket[0], the class-3 roster: the same `->next`
        // link, the same wipe.
        if let Ok(pos) = self.wiz_chain.list.binary_search(&(idx as u16)) {
            self.wiz_chain.cut = self.wiz_chain.cut.min(pos + 1);
        }
        // …and the class-9 roster (`var_u32_36462[3]`).
        if let Ok(pos) = self.proj_chain.list.binary_search(&(idx as u16)) {
            self.proj_chain.cut = self.proj_chain.cut.min(pos + 1);
        }
        // …and MC2's building roster (`dword_38527`).
        if let Ok(pos) = self.bldg_chain.list.binary_search(&(idx as u16)) {
            self.bldg_chain.cut = self.bldg_chain.cut.min(pos + 1);
        }
        // …and MC2's terrain-painter roster (`dword_38535`).
        if let Ok(pos) = self.paint_chain.list.binary_search(&(idx as u16)) {
            self.paint_chain.cut = self.paint_chain.cut.min(pos + 1);
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
        // (deliberate) RETAIL DOES NOT UNLINK HERE: `NewEvent` memsets
        // the record and the old tile's head keeps naming the slot,
        // which is how a death's book scatter of a stale entry breaks
        // retail's tile index for the rest of a level — player-ruled
        // a deviation, round 166 (docs/DEVIATIONS.md, "THE BOOK
        // SCATTER'S STALE ENTRY").
        if self.ent[idx].flags & 4 != 0 {
            self.unlink(idx);
        }
        // New occupant → new presentation generation (hash-silent).
        self.slot_gen.0[idx] = self.slot_gen.0[idx].wrapping_add(1);
        // The aura claim lives ON the entity in retail — slot reuse
        // resets it with every other field (no stale claim may greet
        // the slot's next occupant).
        self.mc2_aura_claim.0.remove(&(idx as u16));
        // …and so does the ALLIANCE PARENT. Retail keeps it in the
        // victim's own `parentId_0x28_40`, which this ctor overwrites
        // with everything else; the port's side-map seat has to be
        // dropped here, and here ONLY (EF:11019-22 is retail's only
        // other clear). See
        // [`crate::mc2::mobs::no_mc2_ally_seat_recycle`].
        if !crate::mc2::mobs::no_mc2_ally_seat_recycle() {
            self.mc2_allied.0.remove(&(idx as u16));
        }
        // ⭐ THE ALLOCATOR SEEDS THE BEHAVIOR ROW TOO, and it is the
        // engine's table BASE. MC2's `NewEvent_4A050` writes
        // `dword_0xA0_160x = &str_D7BD6[59]` in BOTH arms
        // (Events.cpp:573 free pop, :599 sacrifice) — ABSOLUTE row 59,
        // `v_12 = 0` / `v_14 = −4`. MC1's twin writes
        // `var_u32_29951_156 = unk_98F38` (sub_main.cpp:43877/:43902),
        // its own base, which IS index 0 — so `Ent::default()` already
        // serves MC1 and only MC2 needs the stamp. Without it a
        // natively spawned MC2 record ran the (5,0) creature row
        // (`v_12 = 7`, `v_14 = +244`).
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        let e = &mut self.ent[idx];
        *e = Ent::default();
        e.max_life = 300;
        e.flags = 8;
        e.f126 = 16;
        e.f44 = 100;
        e.f68 = 10;
        if mc2 {
            e.row156 = crate::mc2::behavior::ROW_BASE as u8;
        }
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

    /// ⭐ THE MC2 ALLOCATOR'S `100` IS `@0x2A`, NOT `@0x2C` — drop it
    /// from a record whose port `f44` is the `word_0x2C_44` home.
    ///
    /// [`Gen::new_event`] is SHARED with MC1, whose `NewEvent_372C0`
    /// really does seed `+44` (`CARPET.EXE` 0x4FB98
    /// `66 c7 43 2c 64 00`), so the shared `e.f44 = 100` stays. MC2's
    /// `NewEvent_4A050` seeds `+42` instead (`NETHERW.EXE` 0x6E940
    /// `66 c7 43 2a 64 00`) and leaves `+44` at the memset's zero.
    /// Call this straight after `new_event()` in an MC2 ctor whose
    /// model's `f44` is the `@0x2C` home AND whose retail twin makes no
    /// store to `+0x2C` — the full citation, the byte tables for both
    /// binaries and the call-site audit live on
    /// [`no_mc2_alloc_2c_seed`].
    pub(crate) fn mc2_alloc_2c_zero(&mut self, i: usize) {
        if !no_mc2_alloc_2c_seed() {
            self.ent[i].f44 = 0;
        }
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
        let mask = self.victim_mask();
        loop {
            while let Some(s) = self.mc2_recycle.stack.pop() {
                let Some(e) = self.ent.get(s as usize) else {
                    continue;
                };
                if s != 0 && e.class64 != 0 {
                    // ⭐ THE STALE VICTIM (`mc1_recycle_victim_revalidate`,
                    // patches.rs). MC1's stack names SLOT NUMBERS armed
                    // at the last death landing and is never purged on
                    // free (MC2's is — `free_entity`), so a slot freed
                    // and re-minted since is still on it and retail's
                    // bare seizure eats whatever lives there now
                    // (mc1l26 t=27343: a castle's ground-leveler, one
                    // tick from the finish that alone returns the castle
                    // to SETTLED). Patched: a victim that no longer
                    // carries the mask is skipped, as if purged.
                    if self.mc2_recycle.revalidate && e.flags & mask == 0 {
                        self.mc2_recycle.skipped_stale =
                            self.mc2_recycle.skipped_stale.saturating_add(1);
                        continue;
                    }
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

    /// `sub_49F90` (Level.cpp:1271-1302) IN FULL: ONE DESCENDING
    /// 999→1 scan with TWO arms. A class-0 record is pushed on the
    /// FREE stack (:1298-1300) and a live `byte[2] & 2` record on the
    /// VICTIM stack (:1291-92), and BOTH tops are reset to −1 before
    /// the loop. So the free stack's TOP — the next allocation — is
    /// the LOWEST free slot, and the victim stack's TOP is the LOWEST
    /// sacrificable one. (The victim half was missing until dig
    /// 98-Q29; see the tail of the body.)
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
    /// `pinned`: every human seat's carpet record slot.
    pub(crate) fn mc2_rebuild_free(&mut self, pinned: &[u16]) {
        // `sub_49F90`'s FIRST loop (Level.cpp:1277-80): every pending
        // GHOST — disabled (byte[1]&4 → our 0x400) but not yet
        // reaped — is freed NOW (`sub_57F20`: unlink, recycle-list
        // removal, class = 0), BEFORE the descending collect. A
        // mid-tick disposition fire therefore reuses the slot of the
        // very switch that fired it (mc2l0 t=3169: the consumed
        // (11,1)@48's slot takes the dis-2 (11,32); without the reap
        // every payload lands one free slot high — 193/194 where
        // retail records 48/193). MC2-gated: MC1's twin `sub_37220`
        // (:43825) carries no such reap and the eight certified mc1
        // takes pin its allocation receipts.
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            for s in 1..self.ent.len() {
                if !pinned.contains(&(s as u16))
                    && self.ent[s].class64 != 0
                    && self.ent[s].flags & 0x400 != 0
                {
                    self.free_entity(s);
                }
            }
        } else {
            // MC1's disposition-fire rebuild (sub_37440 :43960) runs
            // the reaper and then DISARMS the recycle top
            // (`var_u32_4593 = -1`) — a landing-armed sacrifice stack
            // must not survive a trigger fire. MC2's recycle stack is
            // live machinery (the imported ranking) and stays.
            self.mc2_recycle.stack.clear();
        }
        self.free = (1..self.ent.len() as u16)
            .rev()
            .filter(|&s| !pinned.contains(&s) && self.ent[s as usize].class64 == 0)
            .collect();
        // ⭐⭐⭐ AND THE VICTIM HALF OF THE SAME SCAN (round 98, dig
        // 98-Q29). `sub_49F90` is ONE descending 999→1 loop with TWO
        // arms and it resets BOTH tops before entering it —
        // `NETHERW.EXE` file 0x6E790 (linear 0x49F90):
        //   6e7c5  movl $0xffffffff,0x35(%eax)     ; free top   = -1
        //   6e7d1  movl $0xffffffff,0x11e6(%eax)   ; VICTIM top = -1
        //   6e7f0  cmpb $0x0,0x3f(%eax) / jne      ; class == 0 ?
        //   6e7fe  inc %ecx / mov %eax,0x246(%ecx) ;   → FREE push
        //   6e819  testb $0x2,0xe(%eax)            ; byte[2] & 2
        //   6e82a  inc %esi / mov %eax,0x11ea(%ecx);   → VICTIM push
        // (Level.cpp:1271-1302 has the same two arms.) Rebuilding only
        // the free half left every call site that LEAVES the list
        // ARMED — the death payout `sub_5E310`, EF:60101 — running on
        // a stale victim stack: mc2l22 t=10021/28614/33740 record a
        // victim stack of 106 / 67 / 386 cells where the port held 0.
        // MC2 only: MC1's twin `sub_37220` (:43825) has no victim
        // collect, and the else-arm above disarms instead.
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2)
            && !no_mc2_rebuild_victim_half()
        {
            self.rebuild_recycle(0x2_0000);
        }
    }

    /// `sub_5FD00`'s ZERO-HEADROOM ARM — the castle ejector's
    /// MID-TICK GC, and the one `sub_49F90` call site the port had no
    /// caller for.
    ///
    /// ⭐⭐⭐ THE DECOMPILE IS WRONG HERE, AND THE SHIPPED EXE SETTLES
    /// IT. `EventsFunctions.cpp:61275-83` reads
    ///
    /// ```text
    /// v16 = sub_4A810_get_0x35plus();
    /// if (!v16) {
    ///     sub_49F90();
    ///     result = sub_4A810_get_0x35plus();
    ///     if (!result) { v3 = 8; v16 = result; D41A0_0.dword_0x11e6 = -1; }
    /// }
    /// if (v16) { … mint the spheres … }
    /// ```
    ///
    /// — i.e. a GC pass whose SUCCESS is discarded (`v16` keeps its 0
    /// and the burst is skipped). `NETHERW.EXE` (linear 0x5FD73 =
    /// file 0x84573 by `0x34800 + linear − 0x10000`) refutes all
    /// three placements:
    ///
    /// ```text
    /// 5fd73  e8 98aafeff        call 0x4a810        ; v16 = free depth
    /// 5fd78  89 45fc            mov  [ebp-0x4],eax
    /// 5fd7b  66 85c0            test ax,ax
    /// 5fd7e  75 2b              jnz  0x5fdab        ; headroom → straight to the clamps
    /// 5fd80  e8 0ba2feff        call 0x49f90        ; ⭐ THE GC: reap ghosts, rebuild BOTH stacks
    /// 5fd85  e8 86aafeff        call 0x4a810
    /// 5fd8a  8b 15a0410000      mov  edx,[0x41a0]
    /// 5fd90  be 08000000        mov  esi,0x8        ; ⭐ v3 = 8   — UNCONDITIONAL
    /// 5fd95  89 45fc            mov  [ebp-0x4],eax  ; ⭐ v16 = new depth — UNCONDITIONAL
    /// 5fd98  c7 82e6110000 ff.. mov  dword [edx+0x11e6],-1 ; ⭐ recycle top = -1 — UNCONDITIONAL
    /// 5fda2  66 85c0            test ax,ax
    /// 5fda5  0f84 95010000      jz   0x5ff40        ; STILL dry → only now bail
    /// ```
    ///
    /// So the dry ejector (a) runs the GC, (b) FORCES the burst
    /// ceiling to 8 no matter how large the spill is, (c) EMPTIES the
    /// victim stack `sub_49F90` has just rebuilt, and only then (d)
    /// gives up if the pool really is full. Returns the post-GC free
    /// depth.
    ///
    /// The `v3 = 8` is not a rounding detail: it is the whole
    /// denomination of the burst. mc2l22 t=4667, castle 637 falling
    /// 6 → 5 with a 127,470 spill — retail mints **8** spheres of
    /// 15,933 (`127470/8`, the loop's own remainder feed keeping every
    /// share equal), where an unclamped `spill/1000` would have minted
    /// 32 of 3,983.
    pub(crate) fn mc2_eject_gc(&mut self) -> i32 {
        let pinned: Vec<u16> = self
            .mc2_pinned
            .ixs()
            .map(|hx| self.mc2_pinned[hx].0)
            .collect();
        self.mc2_rebuild_free(&pinned);
        // `mov dword [edx+0x11e6],-1` — the victim stack the rebuild
        // just re-ranked is dropped on the floor before a single
        // sphere is minted. (This is the EIGHTH of the nine
        // `sub_49F90` call sites to clear it; only the death payout
        // `sub_5E310` leaves the list armed.)
        self.mc2_recycle.stack.clear();
        self.free.len() as i32
    }

    /// `sub_37220_375E0` (:43825), MC1's own FREE/RECYCLE REBUILD —
    /// the twin of the MC2 call above, and the one the port had no
    /// runtime caller for. A descending 999→1 pool scan pushing every
    /// INACTIVE slot, so the stack top (and therefore the next
    /// `new_event`) is the LOWEST free slot in the pool rather than
    /// whatever the incremental stack last freed.
    ///
    /// Only two of retail's twelve call sites are runtime — the death
    /// LANDING (:55487) and the RESPAWN (:54842); the rest are level
    /// load, save-restore and the mana-spill allocator's retry. Both
    /// are observable: mc1l42's grave takes slot 109 against our 117,
    /// and its five re-minted spell tokens 110-117 against our
    /// 136-320, purely because retail re-sorted the stack first.
    ///
    /// `pinned` is the conformance import's human-carpet slot — a
    /// zeroed husk in our pool, a live wizard in retail's, and never
    /// allocatable (0 = native play, where the human owns no slot).
    /// `pinned`: every human seat's carpet record slot (held out of the
    /// stack whatever its class).
    pub(crate) fn mc1_rebuild_free(&mut self, pinned: &[u16]) {
        self.free = (1..self.ent.len() as u16)
            .rev()
            .filter(|&s| !pinned.contains(&s) && self.ent[s as usize].class64 == 0)
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
    /// ⭐⭐⭐ RETAIL'S `fontTypeIndex_0x3D_61` — THE RAW BYTE AT
    /// `@0x3D`, READ WITHOUT A CLASS TEST.
    ///
    /// `sub_58F00_game_objectives`'s type-2 completion arm derefs the
    /// bound record and tests `cmpb $0x0,0x3d(%eax)` (EF:40774;
    /// shipped `NETHERW.EXE` file **0x7D95B**, three instructions after
    /// the `cmpl $0xffffffff,0x8(%eax)` life test at 0x7D937) — one
    /// byte, any class, no model gate. The port has no single home for
    /// `@0x3D`: the importer routes it per class exactly as the
    /// conformance lane `b3d` does (world/conformance.rs:1909-31), so
    /// this mirrors that routing and answers **0** for every class
    /// whose `@0x3D` the port does not model — which is the retail
    /// value there, because `NewEvent_4A050` memsets the whole
    /// 168-byte record at birth (Events.cpp:562) and only the class-5
    /// machine, the `(10,45)` degradation chain, the `(10,78)` mine
    /// burst counter, the `(10,76|77)` orb breathe and the `(10,79)`
    /// multipart piece ever write it.
    pub(crate) fn mc2_font_type_3d(&self, i: usize) -> u8 {
        let Some(e) = self.ent.get(i) else {
            return 0;
        };
        let (c, m) = (e.class64, e.model65);
        if c == 10 && m == 79 {
            // the multipart piece's own layout (`b3d` <- f69)
            e.f69 as u8
        } else if c == 5 && m == 22 {
            // the worm parks `@0x2A` in f46; its `@0x3D` is dead
            0
        } else if c == 5
            || (c == 10 && m == 45)
            || crate::engine::world::conformance::orb_breathe_at_3d(c, m)
            || (c == 10 && m == 78)
        {
            e.f46 as u8
        } else {
            0
        }
    }

    /// ⭐⭐⭐ HAS A MID-TICK SEIZURE BLANKED THE CLASS-3 ROSTER HEAD?
    ///
    /// [`Gen::new_event`]'s sacrifice arm nulls `var_u32_36462[0]`
    /// (:43885-91) BEFORE it hands the victim out, so every later
    /// walk of that roster in the same tick starts from a NULL head
    /// and terminates immediately — not merely severed at the victim.
    /// `cut` is `usize::MAX` outside a blank and can only be lowered
    /// to `pos + 1 >= 1` by the severed-chain law, so `cut == 0` is
    /// the blank and nothing else (an EMPTY roster still reads
    /// `usize::MAX`).
    ///
    /// The reader that needed it is the class-11 trigger volume's
    /// FIRE probe `sub_5A090_5A5A0` (:67632-48), whose whole body
    /// lives inside `for (i = var_u32_36462[0]; i > pool; …)`.
    pub(crate) fn wiz_roster_head_blanked(&self) -> bool {
        self.wiz_chain.cut == 0 && !no_trigger_roster_blank()
    }

    /// The record slot homing the MC2 human's `@0x20` target-yaw word —
    /// his (3,0) carpet record ([`no_human_roll_on_record`]).
    fn human_roll_home(&self, hx: Hx) -> Option<usize> {
        if no_human_roll_on_record() || crate::engine::world::no_human_record_mirror() {
            return None;
        }
        let hr = self.mc2_pinned[hx].0 as usize;
        (hr != 0
            && self
                .ent
                .get(hr)
                .is_some_and(|e| e.class64 == 3 && e.model65 == 0))
        .then_some(hr)
    }

    /// The MC2 human's `@0x20` target-yaw word (see `human_roll_0x20`).
    pub(crate) fn human_roll(&self, hx: Hx) -> u16 {
        match self.human_roll_home(hx) {
            Some(hr) => self.ent[hr].f32,
            None => self.human_roll_0x20[hx].0,
        }
    }

    /// Seat the MC2 human's `@0x20` — his record when it is the home,
    /// the register through.
    pub(crate) fn set_human_roll(&mut self, hx: Hx, v: u16) {
        if let Some(hr) = self.human_roll_home(hx) {
            self.ent[hr].f32 = v;
        }
        self.human_roll_0x20[hx] = HashSilent(v);
    }

    /// The human's carpet record slot (MC1 `mc1_pinned` / MC2
    /// `mc2_pinned`; only one is ever set), 0 when the world has none.
    /// Since the multiplayer stage 2 the record is a live (3,0) wizard
    /// record mirroring the human ([`crate::engine::world::World`]'s
    /// `mirror_human_record`), so pool sweeps that still carry their
    /// own human arm skip this slot.
    /// The human seat flying the carpet that owns `tag` — `None` for
    /// any other owner. ⚠ One seat: the identity slice keys it per seat.
    /// ⭐ RETAIL'S LOCAL PLAYER (`playerIndex`) — the seat the world's
    /// own machinery answers to where retail reads
    /// `Entities[playerIndex]` (the doomsday pyramid's target, the MC2
    /// stage engine, the level-start checkpoint, the cave drip). Seat 0.
    pub(crate) fn local_seat(&self) -> Hx {
        Hx::FIRST
    }

    pub(crate) fn hx_of_tag(&self, tag: u16) -> Option<Hx> {
        self.human_tag.ixs().find(|&hx| self.human_tag[hx].0 == tag)
    }

    /// Is `tag` some human seat's owner tag?
    pub(crate) fn is_human_tag(&self, tag: u16) -> bool {
        self.hx_of_tag(tag).is_some()
    }

    /// The human seat flying from wizard seat `slot`, if a human does.
    pub(crate) fn hx_at_wizard(&self, slot: u8) -> Option<Hx> {
        self.human_seat
            .ixs()
            .find(|&hx| self.human_seat[hx].0 == slot)
    }

    pub(crate) fn human_rec(&self, hx: Hx) -> usize {
        self.mc1_pinned[hx].0.max(self.mc2_pinned[hx].0) as usize
    }

    /// Is the human's record the live holder of his seat — linked, so
    /// a walk meets it in its chain ([`human_seat_at_record`])?
    fn human_seat_on_record(&self, hx: Hx) -> bool {
        let hr = self.human_rec(hx);
        human_seat_at_record() && hr != 0 && self.ent.get(hr).is_some_and(|e| e.flags & 4 != 0)
    }

    /// Does a chain walk whose cursor is on `j` stand at the human's
    /// seat? On his own linked record; without one, in front of the
    /// virtual successor [`PlayerChain::next`] (0 = the tail pass).
    pub(crate) fn human_seat_at(&self, j: usize) -> bool {
        self.mc1_pinned
            .ixs()
            .any(|hx| self.human_seat_at_for(hx, j))
    }

    /// [`Self::human_seat_at`] for seat `hx` alone.
    pub(crate) fn human_seat_at_for(&self, hx: Hx, j: usize) -> bool {
        if self.human_seat_on_record(hx) {
            j != 0 && j == self.human_rec(hx)
        } else {
            j == self.player_chain[hx].next as usize
        }
    }

    /// Is his seat the chain's TAIL, met only after the walk falls off
    /// the end? Never with a linked record — the walk meets the record.
    pub(crate) fn human_seat_is_tail(&self, hx: Hx) -> bool {
        !self.human_seat_on_record(hx) && self.player_chain[hx].next == 0
    }

    /// The record homing the human's rebound bit
    /// ([`no_human_rebound_on_record`]): his (3,0) carpet record.
    pub(crate) fn human_rebound_home(&self, hx: Hx) -> Option<usize> {
        let hr = self.human_rec(hx);
        (!no_human_rebound_on_record()
            && hr != 0
            && self
                .ent
                .get(hr)
                .is_some_and(|e| e.class64 == 3 && e.model65 == 0))
        .then_some(hr)
    }

    /// The human's Rebound deflection bit — his record's `0x8000`, else
    /// the register.
    pub(crate) fn player_rebound(&self, hx: Hx) -> bool {
        match self.human_rebound_home(hx) {
            Some(hr) => self.ent[hr].flags & 0x8000 != 0,
            None => self.player_rebound_reg[hx],
        }
    }

    /// Set the human's Rebound bit — the record when it is the home,
    /// the register through.
    pub(crate) fn set_player_rebound(&mut self, hx: Hx, on: bool) {
        if let Some(hr) = self.human_rebound_home(hx) {
            let e = &mut self.ent[hr];
            e.flags = if on {
                e.flags | 0x8000
            } else {
                e.flags & !0x8000
            };
        }
        self.player_rebound_reg[hx] = on;
    }

    /// Is the human's record an ordinary probe victim — (3,0), linked
    /// in its tile chain and carrying the damageable bit
    /// ([`no_human_pool_victim`])? Then every chain walk gated on
    /// `+16 & 8` meets him there and the out-of-pool arms stand down.
    pub(crate) fn human_pool_victim(&self, hx: Hx) -> bool {
        let hr = self.human_rec(hx);
        !no_human_pool_victim()
            && self.human_seat_on_record(hx)
            && self.ent[hr].class64 == 3
            && self.ent[hr].model65 == 0
            && self.ent[hr].flags & 8 != 0
            // …and linked in the tile it STANDS in. Retail's pose
            // writers all relink (`CopyEntityPosition_57CF0`); a port
            // path that moves him without one (a pinned-pose tick with
            // no mover) keeps the out-of-pool arms until his next relink.
            && self.player_chain[hx].cell
                == crate::engine::features::tile((self.ent[hr].x >> 8) as u8, (self.ent[hr].y >> 8) as u8)
    }

    /// Where a write aimed at the human lands: his record as an ordinary
    /// pool victim ([`Self::human_pool_victim`]), else the out-of-pool
    /// player arm.
    pub(crate) fn human_target(&self, hx: Hx) -> crate::mc1::combat::MailTarget {
        if self.human_pool_victim(hx) {
            crate::mc1::combat::MailTarget::Pool(self.human_rec(hx))
        } else {
            crate::mc1::combat::MailTarget::Player(hx)
        }
    }

    /// [`Self::human_target`] for the human's tag, `Pool(tag)` for any
    /// other.
    pub(crate) fn target_of_tag(&self, tag: u16) -> crate::mc1::combat::MailTarget {
        if let Some(hx) = self.hx_of_tag(tag) {
            self.human_target(hx)
        } else {
            crate::mc1::combat::MailTarget::Pool(tag as usize)
        }
    }

    /// Is the human cloaked for the pool-side gates? His (3,0) record's
    /// `+16 & 0x20` — the Invisibility latch with the ghost cheat ORed
    /// in, retail's one storage ([`crate::engine::world::no_human_wide_on_record`])
    /// — else the [`Self::player_invisible`] mirror.
    pub(crate) fn player_cloaked(&self, hx: Hx) -> bool {
        use crate::engine::world::{no_human_mirror_wide, no_human_wide_on_record};
        match self.human_record_home(hx) {
            Some(hr) if !no_human_wide_on_record() && !no_human_mirror_wide() => {
                self.ent[hr].flags & 0x20 != 0
            }
            _ => self.player_invisible[hx],
        }
    }

    /// The human's (3,0) record when it is the home of his state — the
    /// Gen face of `World::human_pose_home` (the mirror on, the pose on
    /// the record, the record flipped to class 3).
    pub(crate) fn human_record_home(&self, hx: Hx) -> Option<usize> {
        use crate::engine::world::{no_human_pose_on_record, no_human_record_mirror};
        let hr = self.human_rec(hx);
        (hr != 0
            && !no_human_pose_on_record()
            && !no_human_record_mirror()
            && self
                .ent
                .get(hr)
                .is_some_and(|e| e.class64 == 3 && e.model65 == 0))
        .then_some(hr)
    }

    /// The record homing the human's purse for the walkers' LIVE reads
    /// ([`no_human_live_purse`]).
    pub(crate) fn human_purse_home(&self, hx: Hx) -> Option<usize> {
        if no_human_live_purse() || crate::engine::world::no_human_mana_on_record() {
            return None;
        }
        self.human_record_home(hx)
    }

    /// The human's purse (+140) as a walker reads it: his record's LIVE
    /// word, else the tick-top `ctx` copy.
    pub(crate) fn human_purse(&self, ctx: &crate::mc1::mobs::MobCtx) -> u32 {
        let hx = ctx.phx;
        match self.human_purse_home(hx) {
            Some(hr) => self.ent[hr].f140 as u32,
            None => self.view(ctx.phx).mana,
        }
    }

    /// The purse the deflection afford gates read (:62706/:62859 read
    /// the DEFLECTOR's live `+140` at the projectile's walk slot): his
    /// record's live word ([`Self::human_purse`]); under
    /// [`no_human_roster_read`] the tick-top `ctx` copy they read before.
    pub(crate) fn human_purse_read(&self, ctx: &crate::mc1::mobs::MobCtx) -> u32 {
        if no_human_roster_read() {
            self.view(ctx.phx).mana
        } else {
            self.human_purse(ctx)
        }
    }

    /// The human's mana ceiling (+136) as a walker reads it (see
    /// [`Self::human_purse`]).
    /// Seat `hx`'s mana ceiling: his purse record's `+136`, else his
    /// view's. Named by seat — the walkers' `ctx` is seat 0's.
    pub(crate) fn human_ceiling_of(&self, hx: Hx) -> u32 {
        match self.human_purse_home(hx) {
            Some(hr) => self.ent[hr].f136 as u32,
            None => self.view(hx).mana_max,
        }
    }

    /// The record a walker reads the human `ctx` describes off
    /// ([`no_human_roster_read`]): his (3,0) carpet record once it is
    /// the home of his pose; `None` in a record-less world (unit rigs)
    /// or under a switch, where the published `ctx` copy stands.
    fn human_read_home(&self, ctx: &crate::mc1::mobs::MobCtx) -> Option<usize> {
        if no_human_roster_read() {
            return None;
        }
        self.human_record_home(ctx.phx)
    }

    /// The human's pose (+72/+74/+76) as a walker reads it at its own
    /// slot: his record, else the `ctx` copy.
    pub(crate) fn human_at(&self, ctx: &crate::mc1::mobs::MobCtx) -> (u16, u16, i16) {
        match self.human_read_home(ctx) {
            Some(hr) => {
                let e = &self.ent[hr];
                (e.x, e.y, e.z)
            }
            None => (
                self.view(ctx.phx).x,
                self.view(ctx.phx).y,
                self.view(ctx.phx).z,
            ),
        }
    }

    /// Seat `hx`'s pose as a walker at this slot reads it: the ctx's
    /// own seat exactly as [`Self::human_at`]; any other seat off his
    /// record (every real world seats one per human).
    pub(crate) fn seat_at(&self, ctx: &crate::mc1::mobs::MobCtx, hx: Hx) -> (u16, u16, i16) {
        if hx == ctx.phx {
            return self.human_at(ctx);
        }
        let e = &self.ent[self.human_rec(hx)];
        (e.x, e.y, e.z)
    }

    /// Seat `hx`'s heading (see [`Self::seat_at`] / [`Self::hpyaw`]).
    pub(crate) fn seat_yaw(&self, ctx: &crate::mc1::mobs::MobCtx, hx: Hx) -> u16 {
        if hx == ctx.phx {
            return self.hpyaw(ctx);
        }
        self.ent[self.human_rec(hx)].f30
    }

    /// Seat `hx`'s live life sign (see [`Self::seat_at`] / [`Self::hpdead`]).
    pub(crate) fn seat_dead(&self, ctx: &crate::mc1::mobs::MobCtx, hx: Hx) -> bool {
        if hx == ctx.phx {
            return self.hpdead(ctx);
        }
        self.ent[self.human_rec(hx)].act_life < 0
    }

    /// Seat `hx`'s TICK-TOP death latch.
    pub(crate) fn seat_dead_top(&self, hx: Hx) -> bool {
        self.view(hx).dead_top
    }

    /// Seat `hx`'s walk view ([`SeatView`]).
    pub(crate) fn view(&self, hx: Hx) -> &SeatView {
        &self.seat_view[hx].0
    }

    pub(crate) fn view_mut(&mut self, hx: Hx) -> &mut SeatView {
        &mut self.seat_view[hx].0
    }

    /// Seat `hx`'s live purse (`+140`; see [`Self::seat_at`] /
    /// [`Self::human_purse`]).
    pub(crate) fn seat_purse(&self, ctx: &crate::mc1::mobs::MobCtx, hx: Hx) -> u32 {
        if hx == ctx.phx {
            return self.human_purse(ctx);
        }
        self.ent[self.human_rec(hx)].f140 as u32
    }

    /// [`Self::seat_purse`] as the deflection afford gates read it
    /// ([`Self::human_purse_read`] for the ctx's own seat).
    pub(crate) fn seat_purse_read(&self, ctx: &crate::mc1::mobs::MobCtx, hx: Hx) -> u32 {
        if hx == ctx.phx {
            return self.human_purse_read(ctx);
        }
        self.ent[self.human_rec(hx)].f140 as u32
    }

    /// The human's heading (+28/`f30`) as a walker reads it: the record
    /// when it homes the heading (MC2's mover turns it in place; MC1's
    /// is the cast echo's stamp), else the `ctx` copy.
    pub(crate) fn hpyaw(&self, ctx: &crate::mc1::mobs::MobCtx) -> u16 {
        use crate::engine::world::{no_human_heading_on_record, no_mc1_cast_aim_on_record};
        let mc2 = self.mc2_pinned[ctx.phx].0 != 0;
        match self.human_read_home(ctx) {
            Some(hr)
                if (mc2 && !no_human_heading_on_record())
                    || (!mc2 && !no_mc1_cast_aim_on_record()) =>
            {
                self.ent[hr].f30
            }
            _ => self.view(ctx.phx).yaw,
        }
    }

    /// Is the human dead or death-falling — his record's LIVE life sign
    /// (`+12 < 0`, :21658-61), else the `ctx` copy. Roster-membership
    /// reads keep the tick-top `view.dead_top`.
    pub(crate) fn hpdead(&self, ctx: &crate::mc1::mobs::MobCtx) -> bool {
        match self.human_read_home(ctx) {
            Some(hr) if !crate::engine::world::no_human_life_on_record() => {
                self.ent[hr].act_life < 0
            }
            _ => self.view(ctx.phx).dead,
        }
    }

    /// Do area writers meet the human at his linked record (whose
    /// `mail` is his mailbox) rather than through a player arm?
    pub(crate) fn human_area_at_record(&self, hx: Hx) -> bool {
        human_area_at_record()
            && self.human_seat_on_record(hx)
            && self.human_mail_home(hx) == Some(self.human_rec(hx))
    }

    /// The slot whose `mail` is the human's mailbox block — his carpet
    /// record (the class-0 husk at import, the (3,0) record after the
    /// first mirror) — or `None` for the out-of-pool `player_mail`
    /// ([`human_mail_in_record`] off, or a world with no record).
    pub(crate) fn human_mail_home(&self, hx: Hx) -> Option<usize> {
        let hr = self.human_rec(hx);
        (human_mail_in_record()
            && hr != 0
            && self
                .ent
                .get(hr)
                .is_some_and(|e| e.class64 == 0 || (e.class64 == 3 && e.model65 == 0)))
        .then_some(hr)
    }

    /// The human's mailbox block (see [`Self::human_mail_home`]).
    pub(crate) fn pmail(&self, hx: Hx) -> &[(u32, u16); 6] {
        match self.human_mail_home(hx) {
            Some(hr) => &self.ent[hr].mail,
            None => &self.player_mail[hx],
        }
    }

    /// The human's mailbox block, writable.
    pub(crate) fn pmail_mut(&mut self, hx: Hx) -> &mut [(u32, u16); 6] {
        match self.human_mail_home(hx) {
            Some(hr) => &mut self.ent[hr].mail,
            None => &mut self.player_mail[hx],
        }
    }

    /// Is chain member `j` the human's own record? Only ever true when
    /// the record rides `wiz_chain` ([`human_on_wiz_chain`]); every
    /// reader then evaluates the human AT HIS SLOT — retail's chain
    /// order — with the same inputs its old out-of-chain arm used.
    pub(crate) fn chain_human(&self, j: usize) -> bool {
        self.hx_of_rec(j).is_some()
    }

    /// The seat whose carpet record is slot `j` (never slot 0).
    pub(crate) fn hx_of_rec(&self, j: usize) -> Option<Hx> {
        if j == 0 {
            return None;
        }
        self.mc1_pinned.ixs().find(|&hx| j == self.human_rec(hx))
    }

    /// Does the human ride the class-3 roster in THIS world? Only when
    /// the switch is on AND his (3,0) record exists — a world with no
    /// carpet record (a bare test world, an MC2 level with no start
    /// marker) keeps every reader's out-of-roster human arm.
    pub(crate) fn human_rides_chain(&self, hx: Hx) -> bool {
        let hr = self.human_rec(hx);
        human_on_wiz_chain()
            && hr != 0
            && self
                .ent
                .get(hr)
                .is_some_and(|e| e.class64 == 3 && e.model65 == 0)
    }

    /// The MC1 human's bucket[0] visibility for the readers that test
    /// it as a whole (the learn clock, the ball guard, the election,
    /// the jar walk): on the roster it is the record's membership
    /// alone; off it, the reader's own liveness stand-in `legacy_live`
    /// (`human_bucket_alive`) AND the out-of-pool seat test.
    pub(crate) fn mc1_human_roster_seen(&self, hx: Hx, legacy_live: bool) -> bool {
        (self.human_rides_chain(hx) || legacy_live) && self.mc1_human_on_wiz_chain(hx)
    }

    /// ⭐⭐⭐ **THE OUT-OF-POOL HUMAN'S SEAT ON BUCKET[0] IS THE CHAIN'S
    /// VISIBLE PREFIX — FOR EVERY READER, NOT ONLY THE WIZARD PICK.**
    ///
    /// Retail's human carpet is an ordinary class-3 record on
    /// `var_u32_36462[0]` (slot order, :52253-62), so the seizure
    /// blank (`NewEvent_372C0` :43885-91 — `CARPET.EXE` VA 0x372EE
    /// `6a 50` push 0x50 / `05 1e 8e 00 00` add eax,0x8e1e / `e8` call
    /// memset, then `c7 80 72 8e 00 00 00 00 00 00` and its three twins
    /// `movl $0,0x8e72/0x8e76/0x8e6e/0x8e7a(%eax)`: the 20 per-model
    /// heads at wizext+36382 and all four class heads, BEFORE the
    /// victim's unlink `e8 86 aa 00 00` (sub_41DD0) and its class
    /// byte clear `c6 43 40 00`) and the severed-chain law hide him
    /// from every walk
    /// later in the tick exactly as they hide a pooled carpet. The
    /// port seats him OUTSIDE each chain walk, so each such pre-pass
    /// needs this test. Same index rule as the wizard pick's
    /// [`crate::mc1::rivals::mc1_human_on_wiz_roster`]: his index in
    /// retail's chain is the number of port-list members below his
    /// slot, compared against `cut` measured in the port's list.
    ///
    /// WITNESS mc1l19 t=14743/14745/14747: the pool is at 999 and the
    /// human's spell storm seizes four live `(2,1)` records (slots 2-5)
    /// every tick; rival 600's fireballs (slots 991/957/960, born the
    /// same ticks) run `sub_54520` over a NULL class-3 head and MISS —
    /// retail `chase 0`, `+34 = +30`; the port's pre-pass still saw the
    /// human at 3,400 units, 69/113 off-axis, locked him (`chase 575`)
    /// and turned the 34-step (`heading` 79 → 113). The bolts two ticks
    /// either side (14741 → creature 25, 14749 → the human) acquire in
    /// both engines, because no seizure ran below them those ticks.
    /// `MGC_NO_MC1_HUMAN_SEAT_CUT=1` reverts.
    pub(crate) fn mc1_human_on_wiz_chain(&self, hx: Hx) -> bool {
        if no_mc1_human_seat_cut() {
            return true;
        }
        let hs = self.mc1_pinned[hx].0;
        // On the roster his record IS the seat: a member of the visible
        // prefix, admitted by the tick-top gate like any wizard.
        if self.human_rides_chain(hx) {
            return self.wiz_chain.list[..self.wiz_chain.visible_len()].contains(&hs);
        }
        let hpos = self.wiz_chain.list.partition_point(|&s| s < hs);
        hpos < self.wiz_chain.cut
    }

    /// The flag mask a recycle-stack rebuild scans for, per game:
    /// MC1's death-landing rebuild takes the sacrificable bit OR the
    /// reap flag (`0x20400`, mc1/rivals.rs), MC2's `sub_49F90` the
    /// sacrificable bit alone (`0x2_0000`).
    pub(crate) fn victim_mask(&self) -> u32 {
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            0x2_0000
        } else {
            0x20400
        }
    }

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

    /// Watcom CRT `rand()` (`watcomrand`, EventsFunctions.cpp:413):
    /// the global `_RWD_randnext` stream. See the `crt_rand` field
    /// doc — TWO retail sim call sites, one per game, and both are
    /// the rival anti-rebound plan roll (MC1 :19507-08, MC2
    /// EF:7212).
    ///
    /// ⭐⭐ MC1's PHASE IS RECOVERED, AND IT IS THE DEFAULT: MC1 has
    /// exactly one `rand()` call site and NO `srand()` anywhere, so
    /// the stream is Watcom's `_RWD_randnext = 1` from process start
    /// and the only thing that can put the port out of phase is
    /// spending a draw retail does not. It was doing exactly that
    /// (see `mc1::rivals::mc1_crt_draw_at_label49`); with the draw
    /// site corrected, seed 1 reproduces retail's rolls across
    /// mc1l49's whole take.
    pub(crate) fn watcom_rand(&mut self) -> u32 {
        self.crt_rand.0 = self.crt_rand.0.wrapping_mul(1103515245).wrapping_add(12345);
        (self.crt_rand.0 >> 16) & 0x7FFF
    }

    /// Test hook: the global Watcom stream's raw state, so a unit pin
    /// can assert WHICH stream a roll spent.
    #[cfg(test)]
    pub(crate) fn debug_crt_rand(&self) -> u32 {
        self.crt_rand.0
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

    /// The out-of-pool human's tile-entry HEAD INSERT — retail's own
    /// `sub_41C70` (:52442) relink inside the carpet's walk handler
    /// `sub_455D0`, which is why this is called from the carpet's walk
    /// slot and nowhere else. Crossing into a tile makes him that
    /// chain's head, so his successor becomes whatever the head was;
    /// staying inside a tile leaves his seat untouched. See
    /// [`PlayerChain`].
    ///
    /// ⭐ MULTIPLAYER STAGE 2: the human's (3,0) record now takes the
    /// seat FOR REAL — unlinked from the old cell and head-inserted
    /// into the new one, so `next` is simply the record's own `+20`.
    /// The relink never touches the record's x/y/z (the mirror owns
    /// them, and they move at points where retail does not relink),
    /// which is why `cell` stays the bookkeeping of WHICH chain holds
    /// him. Every generic chain walker skips the record
    /// ([`Gen::human_rec`]); the seat arms meet him AT the record
    /// ([`Gen::human_seat_at`]), `next` serving only the record-less
    /// virtual seat.
    /// `MGC_NO_HUMAN_TILE_LINK=1` keeps the virtual seat alone.
    pub(crate) fn player_relink(&mut self, hx: Hx, x: u16, y: u16) {
        let t = tile((x >> 8) as u8, (y >> 8) as u8);
        let hr = self.human_rec(hx);
        let real = hr != 0 && hr < self.ent.len() && human_tile_link();
        if self.player_chain[hx].cell != t || (real && self.ent[hr].flags & 4 == 0) {
            if real {
                self.unlink(hr);
            }
            self.player_chain[hx].cell = t;
            self.player_chain[hx].next = self.map_entity[t];
            if real {
                self.link_head_at(hr, t);
            }
        }
    }

    /// Splice the human's record into the chain at the seat
    /// [`PlayerChain`] names — just before `next`, or at the tail when
    /// `next` is 0. The importers seed the seat from retail's own
    /// carpet `+20`, so this is retail's recorded chain position.
    pub(crate) fn seat_human_link(&mut self, hx: Hx) {
        let hr = self.human_rec(hx);
        let (cell, next) = (
            self.player_chain[hx].cell,
            self.player_chain[hx].next as usize,
        );
        if hr == 0 || hr >= self.ent.len() || !human_tile_link() || cell >= self.map_entity.len() {
            return;
        }
        self.unlink(hr);
        let prev = if next != 0 {
            self.ent[next].prev22 as usize
        } else {
            let mut p = 0usize;
            let mut s = self.map_entity[cell] as usize;
            let mut guard = 0;
            while s != 0 && guard <= self.ent.len() {
                p = s;
                s = self.ent[s].next20 as usize;
                guard += 1;
            }
            p
        };
        self.ent[hr].prev22 = prev as u16;
        self.ent[hr].next20 = next as u16;
        if prev != 0 {
            self.ent[prev].next20 = hr as u16;
        } else {
            self.map_entity[cell] = hr as u16;
        }
        if next != 0 {
            self.ent[next].prev22 = hr as u16;
        }
        self.ent[hr].flags |= 4;
    }

    /// After a snapshot restore: the saved pool carries the human
    /// record's links but not the [`PlayerChain`] bookkeeping, so find
    /// the cell whose chain holds him (walk `+22` to the head, then the
    /// head table) and take his successor off his own `+20`.
    pub(crate) fn rederive_player_chain(&mut self, hx: Hx) {
        let hr = self.human_rec(hx);
        if hr == 0 || hr >= self.ent.len() || !human_tile_link() || self.ent[hr].flags & 4 == 0 {
            return;
        }
        let mut h = hr;
        let mut guard = 0;
        while self.ent[h].prev22 != 0 && guard <= self.ent.len() {
            h = self.ent[h].prev22 as usize;
            guard += 1;
        }
        if let Some(t) = self.map_entity.iter().position(|&s| s as usize == h) {
            self.player_chain[hx] = PlayerChain {
                cell: t,
                next: self.ent[hr].next20,
            };
        }
    }

    /// A bare head insert into tile `t`'s chain that leaves the
    /// record's position alone (the human record's relink, above).
    fn link_head_at(&mut self, i: usize, t: usize) {
        let head = self.map_entity[t];
        self.ent[i].prev22 = 0;
        self.ent[i].next20 = head;
        if head != 0 {
            self.ent[head as usize].prev22 = i as u16;
        }
        self.map_entity[t] = i as u16;
        self.ent[i].flags |= 4;
    }

    /// sub_41DD0 (:52486).
    pub(crate) fn unlink(&mut self, i: usize) {
        if self.ent[i].flags & 4 == 0 {
            return;
        }
        let (next, prev) = (self.ent[i].next20, self.ent[i].prev22);
        // The human's SEAT is a gap between two chain members, so the
        // record holding it hands the seat on when it leaves — exactly
        // what retail's doubly-linked splice does to the carpet's own
        // `+20` (:52490-96) when its successor unlinks.
        for hx in self.player_chain.ixs() {
            if self.player_chain[hx].next == i as u16 {
                self.player_chain[hx].next = next;
            }
        }
        if prev != 0 {
            self.ent[prev as usize].next20 = next;
        } else {
            // The human's record heads the chain of the cell it was
            // LINKED in, which its mirrored pose may already have left.
            let t = match self.hx_of_rec(i) {
                Some(hx) if self.player_chain[hx].cell < self.map_entity.len() => {
                    self.player_chain[hx].cell
                }
                _ => tile((self.ent[i].x >> 8) as u8, (self.ent[i].y >> 8) as u8),
            };
            self.map_entity[t] = next;
        }
        if next != 0 {
            self.ent[next as usize].prev22 = prev;
        }
        self.ent[i].flags &= !4;
    }

    /// sub_41C70 (:52442): move, relinking only across tiles.
    ///
    /// ⭐⭐⭐ EXCEPT THE LIGHTNING BEAM, WHICH RETAIL TAKES OFF THE TILE
    /// MAP FOR ITS WHOLE MARCH AND NEVER PUTS BACK. `sub_66750`
    /// (EF:58305) calls `SetMapEntity_57E50(a1x)` — the bare UNLINK
    /// (Events.cpp:5194-5206: fix the neighbours, drop the head,
    /// `byte[0] &= 0xFB`) — one statement BEFORE its first
    /// `sub_66610`, and `sub_66610`'s own step is a RAW field write,
    /// `a1x->position_0x4C_76 = predictedAxis_EB398ar;` (EF:63601),
    /// never `CopyEntityPosition_57CF0`/`sub_57D40`. So the beam
    /// crosses up to 24 tiles carrying no chain membership at all, and
    /// is still unlinked when `DisableEntityDrawing04_57F10` ends it.
    /// The port reuses the generic flyer core, whose every step is
    /// this function — so its beam HEAD-INSERTED itself into every
    /// tile it crossed (`link` is a head insertion, EF:40315-27) and
    /// then sat, LINKED and solid (`flags & 8`), in its terminal tile
    /// for the rest of the frame. Tile chains are walked head→tail and
    /// the FIRST overlapping member wins every probe
    /// (`victim_scan`/`sub_10780`), so a beam left in the chain
    /// re-orders — and can outright win — other entities' probes for
    /// the remainder of the tick. `mc2_beam_defer.armed` is exactly
    /// the march window (armed in `mc2_lightning_beam_tick` around the
    /// step loop, hash- and save-silent), so the unlink lands here
    /// rather than in the beam handler.
    ///
    /// ⚠ MEASURED PAIR-BLIND AND REPLAY-BLIND on both focus takes
    /// (mc2l22 4,402 rows and horizon 1199 identical ON/OFF; rsg
    /// horizon 13,831 identical). Kept because it is retail-exact at
    /// zero measured cost and because the behaviour it removes — a
    /// solid record left head-inserted in every tile a beam crossed —
    /// is exactly the kind of invented state that turns a later dig's
    /// probe result into a false positive.
    pub(crate) fn move_relink(&mut self, i: usize, x: u16, y: u16, z: i16) {
        // `CopyEntityPosition_57CF0`'s twin: retail reaches it with the
        // global scratch axis on 65 of its 92 call sites, so the commit
        // is where the port models the global. See `Gen::mc2_pred_axis`.
        self.mc2_pred_axis = Mc2PredAxis((x, y, z));
        if self.mc2_beam_defer.armed && !no_beam_unlink() {
            self.unlink(i);
            let e = &mut self.ent[i];
            e.x = x;
            e.y = y;
            e.z = z;
            return;
        }
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
    ///
    /// ⭐ MC2 ONLY. MC1's `sub_41E90` (:52512-20) touches the recycle
    /// stack NOT AT ALL — unlink, `class = 0`, push free. Witness:
    /// every recorded MC1 recycle stack in the corpus is STRICTLY
    /// DESCENDING (14,178/14,178 armed-tick snapshots across five
    /// takes) — the descending 999→1 rebuild order (:43843-48), which
    /// a swap-with-top would scramble — and the seized-slot sequences
    /// are descending too (mc1l49 t=54744: 21,20,11,9,8,7,6). A stale
    /// cell whose record has since died stays on the stack; the pop's
    /// dead-cell skip is the port's stand-in for retail's unwitnessed
    /// unconditional seizure of it.
    pub(crate) fn free_entity(&mut self, i: usize) {
        self.unlink(i);
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2)
            && self.ent[i].flags & 0x2_0000 != 0
            && !self.mc2_recycle.stack.is_empty()
        {
            if let Some(at) = self.mc2_recycle.stack.iter().position(|&s| s as usize == i) {
                self.mc2_recycle.stack.swap_remove(at);
            }
        }
        // ⭐⭐⭐ **THE `+48` WORD SURVIVES THE FREE — SO THE PORT'S
        // RAW SHADOW OF IT MUST TOO.** Retail's MC1 free
        // (`sub_41E90`, `CARPET.EXE` file **0x5A688-0x5A6AE**:
        // `call` the tile unlink, `movb $0x0,0x40(%ebx)`, then
        // `inc` the free counter and `mov %ebx,0x251(%eax,%edx,4)`)
        // clears the CLASS BYTE AND NOTHING ELSE, and every
        // `wizext+676` reader resolves its slot RAW (`sub_14E60`,
        // file 0x2D658 — no class, flags or liveness guard). The port
        // homes retail's `+48` in `Ent::f26` while the record is a
        // live class-12 manifestation, and in `Ent::raw48` otherwise;
        // this is the seam between the two, so the live value has to
        // cross it here or the freed token's burst is lost NATIVELY
        // (under replay the retired `mc1_freed_token_burst` import
        // seat used to hide the loss — the IMPORT-SEAT hazard, round
        // 98; the seat was deleted once this carried the word).
        // MC1 only: MC2's class 12 is a different record entirely.
        if self.ent[i].class64 == 12
            && !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2)
            && !no_mc1_token_raw48_native()
        {
            self.ent[i].raw48 = Raw48(self.ent[i].f26 as u16);
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
    /// Returns true on the protect abort, and — on ANY caller — via the
    /// SATURATION LATCH: a clamp at 200 or 0 whose `ax`/`ay` are both
    /// literally zero (:51634/51641). ⚠ That latch is NOT dead: an
    /// effect whose disc reaches the map origin fires it every time it
    /// digs that cell to the floor, and [`Gen::dig_disc`] must honour it
    /// regardless of `protect` (mc1l49's craters, t=33302 onward).
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
        let legacy = no_mc1_dig_abort_latch();
        // ⭐ THE CENTRE AND THE RING DELTAS ARE SIGNED. `sub_40D30`
        // reads the event's position with `movsx` (CARPET.EXE
        // `0x59536`/`0x5953a`: `movsx esi, WORD [edx+0x48]` … `add
        // 0x80` / `sar 8` — an ARITHMETIC shift), and the ring
        // iterator `sub_114B0` sign-extends its byte deltas
        // (`0x29ccc`/`0x29cd8`: `movsx ebx, BYTE PTR [edx]`). The tile
        // INDEX does not care — it takes the low byte either way — but
        // `sub_40A10`'s origin latch compares the FULL i16 sum, so an
        // unsigned widen hides the latch for every event on the wrap
        // seam. WITNESS mc1l49 t=43541 slot 996: `x 317, y 65295` is
        // centre (1, −1) to retail and (1, 255) to the old port, and
        // ring 2's `(−1, +1)` lands the latch on (0, 0) only in the
        // signed reading.
        let (cx, cy) = if legacy {
            (
                ((e.x as u32 + 128) >> 8) as i32,
                ((e.y as u32 + 128) >> 8) as i32,
            )
        } else {
            (
                (e.x as i16 as i32 + 128) >> 8,
                (e.y as i16 as i32 + 128) >> 8,
            )
        };
        let hi = hi.min((e.f80 >> 8) as i32);
        for (dx, dy) in self.ring_cells(lo, hi) {
            let (ax, ay) = if legacy {
                ((cx + dx as i32) as i16, (cy + dy as i32) as i16)
            } else {
                ((cx + dx as i8 as i32) as i16, (cy + dy as i8 as i32) as i16)
            };
            // ⭐ THE ABORT IS THE CELL'S RETURN, FULL STOP. Retail's
            // `sub_40D30_41070` (:51711-16) reads `sub_40A10_40D50`'s
            // byte and returns 1 on ANY nonzero — the `a5`/protect
            // argument is only forwarded, never tested here (CARPET.EXE
            // `0x595a9`: `test al,al / je <next cell>`). The callee has
            // TWO truthy paths: the protect abort (`a4 && angle < 0`)
            // and the SATURATION LATCH — a clamp at 200 or 0 on the
            // literal origin cell — and the port's `&& protect` threw
            // the second one away for the two non-protect diggers
            // ([`Gen::tick_hill`], [`Gen::tick_ridge_head`]), which is
            // exactly where it fires. See
            // [`no_mc1_dig_abort_latch`] for the witness.
            if self.dig_cell(ax, ay, delta, protect) && (protect || !legacy) {
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
            // 13/14: the rising smoke / mana-scatter puffs (sub_3AAA0
            // / sub_3AB40) — authored THING records behind trigger
            // dispositions mint them (mc1l1's t=344 scatter; mc1l3's
            // t=4224 ambush cloud); the generic arm below skipped
            // their two ctor rand draws and left them stateless,
            // spriteless and lifeless.
            // 36: the (10,36) undead-army spawner `sub_3B3E0` — the
            // generic-explode child of the Undead Army bolt, and the
            // only thing that raises the skeleton ring.
            0 | 1 | 5 | 13 | 14 | 17 | 23 | 25 | 36 => {
                return self.spawn_effect(model as u8, x, y, z);
            }
            // 6: the STANDING FIRE `sub_3A730` — the same table row a
            // disposition fire dispatches (mc1l38 t=4771: an `(11,0)`
            // volume mints ten authored `(10,6)` flames). See
            // [`no_mc1_creator_standing_fire`].
            6 if !no_mc1_creator_standing_fire() => {
                return self.spawn_effect(6, x, y, z);
            }
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
            // sub_3A870 (remc1 :46685; HW `sub_3ABF0`, hw:42803): the
            // (10,8) marker — life 8 refilled, damage 100, `+16 &=
            // 0xF7`, a RAW position copy (never map-linked), extents
            // 512/512/512; its state-8 row is the bare self-kill
            // `sub_253E0`, so it lives one tick. The generic arm below
            // left it lifeless with the slot's flag residue. See
            // [`no_mc1_creator_m8`].
            8 if !no_mc1_creator_m8()
                && !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) =>
            {
                e.tick70 = 8;
                e.max_life = 8;
                e.act_life = 8;
                e.f44 = 100;
                e.flags &= !8;
                e.f80 = 512;
                e.f82 = 512;
                e.f84 = 512;
            }
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
            // sub_3B690 (:47501): the DWELLING (fix-up follows). The
            // ctor's last line is `sub_36FA0_37360(event, 177)`
            // (:47517) — the sprite-stats stamp, which is what puts
            // `+86 = 177` and the frame count on every house. The port
            // spawned it art-less: the corpus' authored m45s all read
            // `type86 = 177, frames89 = 1` while the port's own built
            // ones read 0/0.
            45 => {
                e.tick70 = 51;
                e.max_life = 30;
                e.f44 = 100;
                e.f26 = 4;
                e.flags = 9;
                e.f28 = 33;
                let (x, y, z) = (e.x, e.y, e.z);
                self.link(i, x, y, z);
                self.set_sprite(i, 177);
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
                // ⭐ THE SHIFT-ROT HELPER HAS **THREE** LINES, AND THE
                // THIRD ONE WAS MISSING IN BOTH COLUMNS. MC1's
                // `sub_3B6F0` tail is `sub_37130_374F0(v2, 512, 512)`
                // (remc1 sub_main.cpp:47538) and MC1's helper is
                // `+80 = a2; +82 = a2; +84 = a3` (:43790-94). MC2's
                // (10,43) ctor `sub_502B0` (EF:36748-64) ends with
                // `SetEntityShiftRot_49EA0(event, 512, 512)` and that
                // helper is `array.pitch = shift; array.roll = shift;
                // array.fov = fov` (EF:32869-74) — the port's own
                // [`Gen::mc2_shift_rot`] spells all three. Only the
                // shift pair was transcribed here, so `afov` kept
                // whatever the sprite-41 row left (125) instead of 512.
                // WITNESS (free-run raw shadow): `(10,43) afov` 1,209
                // rows across ALL 40 MC2 takes, retail **512** / port
                // 125 (mc2l7 27 rows / 15 tokens, mc2l9 t=4250).
                // `MGC_NO_UPGRADE_TOKEN_FOV=1` restores the old value.
                if !no_upgrade_token_fov() {
                    self.ent[i].f84 = 512;
                }
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
                // :47343-44 — the ctor's own +66/+67 stamp (3, −1);
                // sclass is a graded shadow lane and the unstamped 255
                // capped mc1l32's free run at t=15113 (the portal
                // births mid-take there).
                e.f66 = 3;
                e.f67 = 255;
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
                // :47353 — +154 takes the linked hover z (dest x/y
                // were copied above; a THING post-init overwrites
                // them for authored portals).
                self.ent[i].site_z = self.ent[i].z;
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
            // All remaining retail models (0, 1, 5, 6 [switch off], 8, 13, 14, 15,
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
        // The OCCUPANCY CAP (+128, :43705) — `f128 > f26` is what lets
        // a feeder walk in the door. It is the footprint area over
        // FOUR, corpus-measured across every authored dwelling in the
        // MC1 takes: rows 25 (9x9 → 20), 26 (9x11 → 24), 30 (13x15 →
        // 48) and 53 (8x8 → 16) all fit `(w * h) >> 2` exactly and
        // nothing else. ⚠ The `sub_36DF0_371B0` lift reads `>> 4`,
        // which is 4x too tight on every one of those rows — a
        // transcription slip of the same family as its `36462`
        // chain-head write, and the recording outranks it. Under the
        // shift the port's own villages capped at a quarter of retail's
        // and turned feeders away from houses retail admits.
        self.ent[i].f128 = ((bw * bh) >> 2) as i16;
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
        // sub_37150_37510 (:43798) — ALL FOUR extent words, the
        // `+78 = 0xE000` z-center marker included: a dwelling's
        // collision/aim column is centered 8192 BELOW its record z,
        // exactly like the castle's. The port set the other three and
        // left +78 at the sprite row's, which put the AIM POINT of
        // every self-built house at its roof instead of its footing —
        // and the possess lob's ±0x71 PITCH cone is the consumer:
        // mc1l5 t=4227, lob 363 (pitch 2017) bends onto house 365
        // where retail, aiming at −6432 instead of 1760, misses the
        // cone by 240 units and flies straight.
        e.f78 = 0xE000;
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
    /// terrain deformers broadcast ch0 damage + the loop-10 rumble.
    /// The rumble needs a listener; the damage does NOT — the load
    /// pass bills the half-built pool too, and the state-51 dwellings'
    /// mailboxes carry it into the first live tick (round 155, see
    /// [`no_mc1_load_pass_area_mail`]).
    pub(crate) fn tick(&mut self, i: usize, ctx: Option<&crate::mc1::mobs::MobCtx>) {
        let pad_fix = |ctx: Option<&crate::mc1::mobs::MobCtx>| {
            ctx.is_some_and(|c| {
                (!c.strict || force_building_patches()) && c.patches.mc1_building_pad_saturate
            })
        };
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
            // `mc1_building_pad_saturate` reads the runtime set too; the
            // load fixpoint (no ctx) and `strict_retail` run retail.
            43 => self.tick_castle_leveler_with(i, pad_fix(ctx)),
            44 => self.tick_castle_painter_with(i, pad_fix(ctx)),
            45 => self.tick_upgrade_token(i),
            51 => {
                // The crushed-site patch reads the runtime set; the load
                // fixpoint (no ctx) and `strict_retail` run retail.
                let crushed_site = ctx.is_some_and(|c| {
                    (!c.strict || force_building_patches()) && c.patches.mc1_crushed_site_collapse
                });
                self.tick_building(i, crushed_site, pad_fix(ctx))
            }
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
        } else if ctx.is_some() || !no_mc1_load_pass_area_mail() {
            // The load fixpoint bills the half-built pool too (see
            // [`no_mc1_load_pass_area_mail`]); only the rumble needs a
            // listener.
            let amt = self.ent[i].f44 as u32;
            self.area_write_opt(i, 0, amt, ctx, false, true);
            if ctx.is_some() {
                self.snd(10, i);
            }
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
        // ⭐ The load fixpoint runs this write too — the canyon
        // diggers' 216 each lands in the state-51 dwellings' mailboxes
        // (see [`no_mc1_load_pass_area_mail`]).
        if ctx.is_some() || !no_mc1_load_pass_area_mail() {
            let amt = if self.ent[i].flags & 2 != 0 {
                self.ent[i].f44 as u32 / 25
            } else {
                self.ent[i].f44 as u32
            };
            self.area_write_opt(i, 0, amt, ctx, false, true);
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
        let aborted = self.dig_disc(i, 0, 1024, (r % 0xF + 10) as i16, false);
        // Retail bills (and rumbles) only on a raise that went through:
        // `if (!(u8)sub_40D30(...)) { sub_120B0(a1x, 0, +44); sub_55370 }`
        // (:29163-66). The port's runtime-only write ignored the return
        // — latent, the (10,50) ridge is load-time only in every shipped
        // level — until the load fixpoint started billing too.
        let bill = if no_mc1_load_pass_area_mail() {
            ctx.is_some()
        } else {
            !aborted
        };
        if bill {
            // Load fixpoint included — [`no_mc1_load_pass_area_mail`].
            let amt = self.ent[i].f44 as u32;
            self.area_write_opt(i, 0, amt, ctx, false, false);
            if ctx.is_some() {
                self.snd(10, i);
            }
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
    ///
    /// ⚠⚠ **THE COUNTDOWN IS THE ONLY EXIT, AND IT TESTS `== 0`.**
    /// `CARPET.EXE` file 0x405A6-0x405BE (VA 0x27DAE-0x27DC6):
    /// `mov ebx,[edi+0xC]; dec ebx; mov [edi+0xC],ebx; …; test ecx,ecx;
    /// je 0x4090d` — the finish runs only when the decremented life is
    /// EXACTLY zero, and every goal step divides by that same life
    /// (`idivl 0xc(%edi)` at file 0x40666/0x4070C/0x40753/0x4079D,
    /// result `add`ed to the height and stored as a BYTE). The one
    /// writer that ever pushes a site's life below zero from outside is
    /// the castle pre-clear `sub_12C50` (:17616; file 0x2B4DF `movl
    /// $0xffffffff,0xc(%ebx)`), which walks the WHOLE +36470 house
    /// chain — sites in construction included — and stamps -1 on every
    /// house inside the next-level box, on the castle's founding and on
    /// every upgrade. A finished house reads that in `sub_28DC0` and
    /// collapses (state 53); a SITE never looks: its life runs -1, -2,
    /// … forever, so it never finishes (never gets its flag bit, never
    /// becomes a possessable state-52 house) and every tick it steps
    /// each footprint cell AWAY from its goal by `(goal - h) / life`:
    /// a cell off its goal by `d` moves to `d + d/|life|` — at least +1
    /// a tick while `|d| >= |life|` — and the byte store wraps. Cells
    /// above goal climb to 255 (the "table mountain"), cells below
    /// sink to 0, and a site crushed early (large `d`) wraps through
    /// both. RETAIL DOES THIS: `recordings/mc1l13.mgcr` slot 140, a
    /// (10,45) site at (0xB7,0xD6) crushed by castle 144's pre-clear at
    /// t=520 (`act_life 19 -> -1`), still in state 51 at t=32717 with
    /// life -30802, its footprint reading 255/254/220/0/2/13 beside a
    /// 60-75 hillside (the port replays that take `horizon=END`).
    ///
    /// PATCH `mc1_crushed_site_collapse` (`crushed_site`): a site whose
    /// life is already <= 0 when its tick opens has been crushed from
    /// outside (the countdown itself can only reach 0 by finishing).
    /// It takes the finished house's own crushed arm — state 53, the
    /// one-shot collapse `sub_28FE0` — instead of flattening on.
    ///
    /// PATCH `mc1_building_pad_saturate` (`pad_saturate`): each cell's
    /// `datum + pad` goal saturates at 255 ([`building_pad_goal`]) —
    /// no leveler follows a dwelling, so retail's wrap is permanent and
    /// the finish re-reads the site z off the pit under its centre.
    fn tick_building(&mut self, i: usize, crushed_site: bool, pad_saturate: bool) {
        if crushed_site && self.ent[i].act_life <= 0 && self.ent[i].model65 == 45 {
            self.ent[i].tick70 = 53;
            return;
        }
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
            self.flatten_build_row(
                e.f71 as usize,
                cx,
                cy,
                target,
                life,
                FlattenLaw::Building,
                pad_saturate,
            );
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
        pad_saturate: bool,
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
                    let goal = building_pad_goal(goal, pad_saturate);
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
            let ctl = self.dmg(c, cx, cy) as i8;
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
                let b = self.dmg(c, cx, cy);
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
                                    // The victim's next inbox zeroes
                                    // the latch; the ledger keeps it.
                                    self.stats.0.hint(j, owner);
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

    /// ⚠ D14 PROBE (remove by inverse edit). `MGC_BUILD_DAT_DAMAGE=
    /// "cx,cy:off=hex,off=hex"` substitutes bytes for the BUILD?-0.DAT
    /// image, but ONLY for a paint centred on the named cell. This
    /// reconstructs retail's *damaged in-memory* build table for one
    /// castle instance without corrupting every other build in the
    /// level, so the damage can be attributed on the take.
    fn dmg(&self, c: usize, cx: u8, cy: u8) -> u8 {
        // ⚠ w159j PROBE EXTENSION: several site groups, `;`-separated,
        // each optionally window-scoped to a tick range
        // (`cx,cy@t0-t1:off=hex,…`). Needed because mc1l6's damaged
        // image is not constant in time OR in site: cell 2750 reads
        // low-nibble 4 for the t=11733 build and 7 for the t=27748
        // build at the SAME site (12,24).
        #[allow(clippy::type_complexity)]
        static P: std::sync::OnceLock<Vec<((u8, u8), (u64, u64), Vec<(usize, u8)>)>> =
            std::sync::OnceLock::new();
        let p = P.get_or_init(|| {
            let Ok(v) = std::env::var("MGC_BUILD_DAT_DAMAGE") else {
                return Vec::new();
            };
            let mut groups = Vec::new();
            for g in v.split(';').filter(|s| !s.is_empty()) {
                let Some((site, rest)) = g.split_once(':') else {
                    continue;
                };
                let (site, win) = match site.split_once('@') {
                    Some((s, w)) => {
                        let (a, b) = w.split_once('-').unwrap_or((w, w));
                        (
                            s,
                            (
                                a.parse::<u64>().unwrap_or(0),
                                b.parse::<u64>().unwrap_or(u64::MAX),
                            ),
                        )
                    }
                    None => (site, (0, u64::MAX)),
                };
                let Some((sx, sy)) = site.split_once(',') else {
                    continue;
                };
                let mut out = Vec::new();
                for kv in rest.split(',').filter(|s| !s.is_empty()) {
                    if let Some((a, b)) = kv.split_once('=')
                        && let Ok(o) = a.trim().parse::<usize>()
                        && let Ok(val) = u8::from_str_radix(b.trim().trim_start_matches("0x"), 16)
                    {
                        out.push((o, val));
                    }
                }
                if let (Ok(x), Ok(y)) = (sx.parse::<u8>(), sy.parse::<u8>()) {
                    groups.push(((x, y), win, out));
                }
            }
            groups
        });
        if !p.is_empty() {
            let t = crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed);
            for ((sx, sy), (t0, t1), list) in p.iter() {
                if *sx == cx
                    && *sy == cy
                    && t >= *t0
                    && t <= *t1
                    && let Some(&(_, v)) = list.iter().find(|(o, _)| *o == c)
                {
                    return v;
                }
            }
        }
        self.assets.build_dat[c]
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
            let ctl = self.dmg(c, cx, cy) as i8;
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
                let b = self.dmg(c, cx, cy);
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
    /// model as a field because every class-10-reachable writer is
    /// known: :47583 spawns the plain m42 painter with +60 = 1 (a
    /// 25-tick idle) and :56490 spawns the upgrade-commit painter
    /// with +60 = 0 AND the +18 kill bit (:56492). (:48001 also
    /// writes +60, but on CLASS-12 tokens — never dispatched here.)
    /// ⚠ THE KILL BIT ALONE IS NOT THE BRANCH: a record that ACQUIRES
    /// action 44 by mutation — mc1hwl0 t=27411, a (10,6) fire re-mint
    /// whose stale class-5 roster entry runs the m7 CHASE stamp
    /// (f70 = base 42 + 2) — was NewEvent-zeroed, carries +60 = 0 and
    /// NO kill bit, and retail idles it ONE tick (:30514 else-arm),
    /// finishing at t=27431 where the kill-bit-only proxy idled 25.
    /// Only the m42 ctor ever writes +60 = 1, so model65 == 42 is
    /// the faithful ctor-provenance test.
    #[cfg(test)] // the live dispatch calls `_with` (the patch arm)
    fn tick_castle_painter(&mut self, i: usize) {
        self.tick_castle_painter_with(i, false)
    }

    /// The tallest pad the castle painter's goal decode
    /// ([`Gen::fill_castle_goal_row`]) puts on any cell of build rows
    /// `1..=level` (0 when they carry none).
    fn castle_pad_max(&self, level: usize) -> i32 {
        let mut max = 0;
        for bt in 1..=level {
            let Some(def) = self.assets.build_tab.get(bt).copied() else {
                continue;
            };
            let mut rows = def.h;
            let mut c = def.offset as usize;
            while rows != 0 && c < self.assets.build_dat.len() {
                let ctl = self.assets.build_dat[c] as i8;
                c += 1;
                if ctl == 0 {
                    rows -= 1;
                    continue;
                }
                if ctl < 0 {
                    continue;
                }
                for _ in 0..ctl {
                    let b = self.assets.build_dat.get(c).copied().unwrap_or(0);
                    c += 1;
                    if b >= 0xF && b % 16 != 0 {
                        max = max.max(4 * (b as i32 % 16 - 1));
                    }
                }
            }
        }
        max
    }

    /// PATCH `mc1_building_pad_saturate`, the CASTLE half: the highest
    /// datum a level-`level` castle can stand on without its tallest
    /// painted cell passing 255 — `255 − castle_pad_max` (207 for every
    /// shipped level, row 1's pad being 48).
    ///
    /// ⚠ WHY A DATUM CAP AND NOT THE GOAL CLAMP ([`building_pad_goal`]).
    /// Every painter is followed by the leveler `sub_28200` (:30284),
    /// which TRANSLATES the whole rect by `target − current` with an
    /// 8-bit add (`CARPET.EXE` file 0x40CBF/0x40D7B `add %cl,%ch`, byte
    /// store 0x40CC3/0x40D7F) — modular, so it undoes a painter wrap
    /// whenever its target (the outside-corner average, clamped only at
    /// 220, file 0x40B40 `cmp $0xdc`) is <= 207. Retail witness
    /// `recordings/mc1l32-new.mgcr`: painter slot 39 (level 1, datum
    /// 232) wraps (239,218) 254 → 0 at t=6331, ends its paint at 16
    /// (272 & 0xFF), and the leveler (current 232, target 173) walks it
    /// 16 → 5 → 255 → 213 = 272 − 59; painter slot 9 (datum 241) again
    /// at t=8036. A goal clamp would leave 255 for the leveler to lower
    /// by the full 59 (196 where retail ends at 221 on (238,218)).
    /// Capping the datum in the painter AND the leveler's current and
    /// target instead paints a shape that never wraps and, whenever
    /// retail's leveler would have healed the wrap, settles on exactly
    /// retail's final heights; above 207 the castle sits lower instead
    /// of keeping pits.
    fn castle_datum_cap(&self, level: usize) -> i32 {
        255 - self.castle_pad_max(level)
    }

    /// [`Gen::tick_castle_painter`] with the `mc1_building_pad_saturate`
    /// PATCH arm (`datum_cap`, see [`Gen::castle_datum_cap`]). Only the
    /// live runtime dispatch passes `true`.
    fn tick_castle_painter_with(&mut self, i: usize, datum_cap: bool) {
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
            // :30512-16 — no work on this tick: arm the idle phase
            // and return. +60 = 1 (⇒ idle 25) is the m42 ctor's alone
            // (:47583); the upgrade-commit clears it with the kill
            // bit (:56490-92) and every mutation-acquired action-44
            // record was NewEvent-zeroed — both idle 1 (see the doc
            // note above; mc1hwl0 t=27431 is the witness).
            self.ent[i].f26 = if self.ent[i].model65 == 42 && self.ent[i].flags & 0x10000 == 0 {
                -25
            } else {
                -1
            };
            return;
        }
        let e = self.ent[i];
        // :30520-21 — a shaking castle (damage-response +50 armed by
        // a nearby blast) suspends the whole work body; the counter
        // has already stepped. Retail resolves the castle through the
        // painter's +42 link (`castle_of_worker`).
        if let Some(c) = self.castle_of_worker(i) {
            if self.ent[c].f50 != 0 {
                return;
            }
        }
        let cx = ((e.x as u32 + 128) >> 8) as u8;
        let cy = ((e.y as u32 + 128) >> 8) as u8;
        let target = (e.z >> 5) as i32;
        // Row = level verbatim (retail never clamps it): level 0
        // paints nothing, which is what a bare-flag castle owns.
        // ⚠ MEASUREMENT PROBE — an OUT-OF-LADDER row paints nothing.
        let level = if mc1_oob_castle_inert("paint") && e.f71 >= 8 {
            0usize
        } else {
            mc1_build_row(e.f71)
        };
        let target = if datum_cap {
            target.min(self.castle_datum_cap(level))
        } else {
            target
        };
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
        // ⭐ PHASE (:30630-46): retail's kill and paint live INSIDE the
        // per-row RLE walk that fills `deltas`, so both read the
        // PRE-STEP heightmap; the buffered deltas are only swept into
        // the heightmap afterwards (:30541-79). Neither the kill nor
        // the paint writes a height, and the fill reads nothing but
        // heights, so running them as their own passes right here is
        // observationally identical to retail's interleave — but
        // running them AFTER the apply is not: `sub_33640`'s corner
        // vote then sees this tick's raise, and a raising castle
        // paints a DIFFERENT orientation index (mc1l48 t=683,
        // (26,137): pre-step quad 68/70/69/69 votes 5, post-step
        // 70/72/71/72 votes 4 — types 26 vs 27, and type 27 is
        // creature-BLOCKING where 26 is not).
        let late_paint = no_castle_paint_phase();
        if !late_paint {
            self.castle_kill_and_paint(i, level, cx, cy);
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
        if late_paint {
            self.castle_kill_and_paint(i, level, cx, cy);
        }
    }

    /// The castle painter's kill + paint passes (:30630-46), factored
    /// out of [`Gen::tick_castle_painter`] so the PHASE — before or
    /// after the height-apply sweep — is one call site.
    ///
    /// THE CASTLE WEAPON (sub_40E20 :51729, called per footprint tile
    /// per paint tick :30631-34): the rising transformation EXECUTES
    /// what stands on it — but only under the upgrade-commit painter
    /// (the +18&1 kill bit, :56492); the damage repaint kills nothing.
    /// The paint itself (:30646) SKIPS when `f26 % 7 && f26 != 1`.
    fn castle_kill_and_paint(&mut self, i: usize, level: usize, cx: u8, cy: u8) {
        let e = self.ent[i];
        if e.flags & 0x10000 != 0 {
            self.build_footprint_kill(level, cx, cy, e.id24);
        }
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
        // ⚠ MEASUREMENT PROBE — an OUT-OF-LADDER row promotes nothing.
        let level = if mc1_oob_castle_inert("paint") && e.f71 >= 8 {
            0usize
        } else {
            mc1_build_row(e.f71)
        };
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
        if let Some(c) = self.castle_of_worker(i) {
            self.ent[c].f59 = 5;
        }
        self.ent[i].flags |= 0x400;
    }

    /// The castle a build worker (m41/m42) serves — retail's `+42`
    /// link ([`Ent::link42`]), dereferenced the way `sub_285C0` :30520
    /// and `sub_28200` :30333 do: by slot, no class test. An unlinked
    /// worker (0 — a pre-field save) and the kill switch fall back to
    /// the site scan [`Gen::castle_at_site`].
    fn castle_of_worker(&self, i: usize) -> Option<usize> {
        let link = self.ent[i].link42.0 as usize;
        if link != 0 && link < self.ent.len() && !no_mc1_worker_castle_link() {
            return Some(link);
        }
        let e = &self.ent[i];
        self.castle_at_site(e.x, e.y)
    }

    /// The pre-w154j castle resolve: the lowest-numbered live castle
    /// at the worker's spawn position, which IS the castle's site
    /// corner (unique while the placement scan's 8-tile spacing holds
    /// and no razed-and-rebuilt or TRANSFORM-parked twin shares it).
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
    /// despawn — which is ALSO where a shaking castle sends it, see
    /// the `castle_shaking` gate below (:30333).
    #[cfg(test)] // the live dispatch calls `_with` (the patch arm)
    fn tick_castle_leveler(&mut self, i: usize) {
        self.tick_castle_leveler_with(i, false)
    }

    /// [`Gen::tick_castle_leveler`] with the `mc1_building_pad_saturate`
    /// PATCH arm (`datum_cap`): the current AND the target are capped
    /// at [`Gen::castle_datum_cap`], the datum the patched painter
    /// built at, so the translation starts where the paint ended and
    /// never lifts the towers past 255.
    fn tick_castle_leveler_with(&mut self, i: usize, datum_cap: bool) {
        let e = self.ent[i];
        let cx = ((e.x as u32 + 128) >> 8) as u8;
        let cy = ((e.y as u32 + 128) >> 8) as u8;
        // ⚠ MEASUREMENT PROBE — an OUT-OF-LADDER row levels nothing.
        let def = if mc1_oob_castle_inert("paint") && e.f71 >= 8 {
            self.assets.build_tab[0]
        } else {
            self.assets.build_tab[e.f71 as usize % self.assets.build_tab.len()]
        };
        let x0 = cx.wrapping_sub((def.w >> 1) as u8);
        let y0 = cy.wrapping_sub((def.h >> 1) as u8);
        if e.flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.ent[i].f26 = 10;
            let cur = e.z >> 5;
            let mut tgt = self.avg4(
                x0.wrapping_sub(1),
                y0.wrapping_sub(1),
                def.h.wrapping_add(2),
                def.w.wrapping_add(2),
            );
            if tgt > 220 {
                tgt = 220;
            }
            let cur = if datum_cap {
                let cap = self.castle_datum_cap(e.f71.min(8) as usize);
                tgt = tgt.min(cap as u16);
                cur.min(cap as i16)
            } else {
                cur
            };
            self.ent[i].f28 = cur as u16;
            self.ent[i].f44 = tgt;
            if cur == tgt as i16 {
                self.ent[i].f26 = 0;
            }
            return;
        }
        let counter = self.ent[i].f26;
        // :30333 — the work body's REAL predicate is
        // `!castle[+42]->+50 && +26`, and its ELSE arm is the finish.
        // A blast that shakes the owner castle (`+50 = 30`,
        // sub_127E0 :17523) therefore ABORTS the levelling where it
        // stands: the terrain keeps whatever height the translation
        // had reached and the castle is handed straight to sub-state
        // 2. Retail follows its own `+42` link; we re-derive the
        // castle by site, exactly as the m42 painter's twin guard
        // (:30520) already does — the two agree wherever a live
        // castle stands on its own site (mc1l49 t=14822: the leveler's
        // `+42` is 749 and `castle_at_site` finds 749). Since w154j
        // the link itself is followed (`castle_of_worker`).
        let castle_shaking = !mc1_no_leveler_shake_gate()
            && self
                .castle_of_worker(i)
                .is_some_and(|c| self.ent[c].f50 != 0);
        if counter != 0 && !castle_shaking {
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
            if let Some(c) = self.castle_of_worker(i) {
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
            // Retail arm only: the authored castle's FIRST tick runs the
            // level-up commit, whose painter repaints every one of these
            // rows toward the (patched: capped) goal and hands off to the
            // leveler, so an init wrap is gone within that painter's run.
            self.flatten_build_row(r, cx, cy, target, 1, FlattenLaw::CastleInit, false);
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

    /// sub_37150 (:43798): the castle-box stamp — unconditional, level
    /// 0 included (row-0 dims are empty, so level 0 stamps the
    /// 0xE000/640/640/0x4000 bare-flag box; retail's respawn rebuild
    /// :54995 and the fit helpers' exit restamps both rely on it).
    /// The old `lvl >= 1` guard here was invented — it kept every
    /// newborn castle on its ctor art extents (200/184/184/200) until
    /// the first level-up commit, where retail's box arrives the
    /// moment ANY fit test or admission check touches the castle
    /// (mc1hwl0 t=18950: slot 233's box lanes at the plant tick).
    pub(crate) fn castle_box_stamp(&mut self, i: usize, lvl: usize) {
        let def = self.assets.build_tab[lvl % self.assets.build_tab.len()];
        let e = &mut self.ent[i];
        e.f78 = 0xE000; // sub_37150's z-center marker (signed −8192)
        e.f80 = (((def.w as u16) << 8).wrapping_add(1280)) >> 1;
        e.f82 = (((def.h as u16) << 8).wrapping_add(1280)) >> 1;
        e.f84 = 0x4000;
    }

    /// sub_37150 + the HP ladder: size a castle entity's extents and
    /// life to its level (retail pairs the box stamp with sub_47C60's
    /// ladder at the init/respawn rebuild).
    pub(crate) fn castle_extents(&mut self, i: usize, lvl: u8) {
        self.castle_box_stamp(i, lvl as usize);
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
        // ⭐ THE LIVE ARM OPENS WITH THE ANIMATION STEP, OUTSIDE THE
        // ONE-SHOT LATCH — and both columns spell it. MC2's
        // `sub_389F0` (EF:28265-97) is
        // ```c
        //     v1 = life; dword_0x10_16++; life = v1 - 1;
        //     if (v1 >= 0) { sub_585A0(a1x);
        //                    if (!(byte[0] & 2)) { … } }
        //     DisableEntityDrawing04_57F10(a1x);
        // ```
        // and MC1's twin `sub_293D0` (remc1 sub_main.cpp:31009-40) has
        // `sub_42510_42850(a1x)` in exactly the same seat. The port
        // fused the life test with the latch test and dropped the call,
        // so the token died on `animationFrame_0x5C_92` = 0 where
        // retail always records 1 (`sub_585A0` = `if (frame88 <
        // frames89) frame88 += 1`, [`Gen::mc2_anim_step`] /
        // [`Gen::anim_advance`]).
        // WITNESS (free-run raw shadow): `(10,43) b5c` 761 rows across
        // ALL 40 MC2 takes, retail **1** / port 0 (mc2l7 16 rows / 15
        // tokens). `MGC_NO_UPGRADE_TOKEN_ANIM=1` restores the old
        // frozen frame. Same family as
        // [`Self::the_bolt_hit_flash_has_no_animation_step`]'s MC1
        // finding, from the other side.
        if life >= 0 && !no_upgrade_token_anim() {
            if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
                self.mc2_anim_step(i);
            } else {
                self.anim_advance(i);
            }
        }
        if life >= 0 && self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let own = self.ent[i].id24;
            // The wizext+50 stand-in: +50 is written only by the
            // level-up commit (:56484) and cleared by the removal
            // path (:56534), so the bound castle is the owner's
            // ESTABLISHED (3,2) — a fresh level-0 flag is unbound
            // and the delivery misses it.
            //
            // ⭐⭐ MC2 BINDS AT LEVEL 0, so the `f26 > 0` half of that
            // stand-in is MC1's alone. MC2 writes
            // `CastleEntityIndex_0x3A_58` in the very branch that
            // spawns a first castle (EF:6836-38 —
            // `v4x->id_0x1A_26 = a1x->id; …CastleEntityIndex = v4x -
            // struct_0x6E8E`), with no level test anywhere, and
            // `sub_69AB0`'s delivery arm then reads that index raw
            // (EF:56150-52). So a rival's FIRST upgrade — the one
            // that takes a level-0 flag to level 1 — is a legitimate
            // (10,43) delivery on this column, and the MC1 filter
            // silently dropped every one of them. `mc2_castle_of` is
            // the port's own MC2 resolution and carries no level
            // test either.
            let castle = if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
                // ⭐⭐⭐ AND THE RESOLUTION IS A **REGISTER READ**, NOT A
                // POOL SCAN. `sub_389F0` dereferences the token's owner
                // wizard and takes its extension block's
                // `CastleEntityIndex_0x3A_58` RAW — shipped NETHERW.EXE
                // file 0x5D24B-0x5D26E:
                //   `0f bf 43 1a  movswl 0x1a(%ebx),%eax`      (token owner id)
                //   `8b 04 85 e4 a3 01 00  mov 0x1a3e4(,%eax,4),%eax`
                //   `8b 80 a4 00 00 00     mov 0xa4(%eax),%eax` (wizext)
                //   `66 8b 40 3a           mov 0x3a(%eax),%ax`  (the register)
                //   `8b 0c 85 e4 a3 01 00  mov 0x1a3e4(,%eax,4),%ecx`
                //   `e8 4d 7c fd ff        call 0x34ec0`        (sub_106C0 overlap)
                // — no class, model, owner or reap test anywhere, and
                // the HIT arm (0x5D27A) re-walks the SAME chain before
                // writing `word_0x80_128` and `dword_0x7C_124 = 10`.
                // See [`crate::mc2::castle::no_mc2_token_castle_register`].
                if crate::mc2::castle::no_mc2_token_castle_register() {
                    self.mc2_castle_of(own)
                } else {
                    self.mc2_castle_reg_of(own)
                }
            } else {
                (1..self.ent.len()).find(|&c| {
                    let e = &self.ent[c];
                    e.class64 == 3
                        && e.model65 == 2
                        && e.id24 == own
                        && e.f26 > 0
                        && e.flags & 0x400 == 0
                })
            };
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
            if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
                // MC2: `sub_389F0`'s miss arm is `sub_5F890(a1x, 0)`
                // (EF:28249) — the owner's spell-2 manifestation
                // `word_0x2E_46 = 0`, through the World-side book
                // (`mc2_castle_lock_mail`, drained at this slot). Its
                // sibling arm EF:28238 (`(int16)terrainAlt > 58880`) is
                // DEAD IN THE SHIPPED EXE, not a transcription slip:
                // NETHERW.EXE file 0x5d235-0x5d23e is `cwde` (sign-
                // extend the 16-bit altitude) / `cmp eax,0xe600` /
                // `jle` (signed), so the release at 0x5d243 needs an
                // altitude above 32767 from a height byte × 32 that
                // tops out at 8160. Not ported, by measurement.
                self.mc2_castle_lock_mail.0.push((own, false));
            } else {
                self.release_castle_charge_pin(own);
            }
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

    /// **MC1/HW ONLY** — and it is BOTH the castle's mana CAPACITY
    /// and the Create-Castle PRICE, because MC1 fuses them into one
    /// ladder. `sub_47C60_47FA0` (remc1/sub_main.cpp:56572) switches
    /// on `+26` and hands `sub_47BD0_47F10` a PAIR per rung; this
    /// array is the SECOND value, and :56561-63 writes it to the
    /// manifestation's `+136` (the price), derives the HUD divisor
    /// `+140 = a4 / +50`, AND stores it as the castle's own capacity
    /// `a1[34]`. The pair's FIRST value
    /// (0/20000/40000/40000/60000/60000/80000/80000) is the HP
    /// ladder — identical to MC2's [`super::super::mc2::castle::MC2_CASTLE_HP`],
    /// debt clamp and all.
    ///
    /// ⚠⚠ **MC2 SPLITS WHAT THIS FUSES.** There, capacity is
    /// `MC2_CASTLE_CAP` (5000/8500/18000/38800/… — different at every
    /// rung >= 1) and the price is a THIRD ladder,
    /// `MC2_CASTLE_COST`. Never reuse this array for an MC2 path:
    /// on MC1 one number answers "what does it cost" and "what can it
    /// hold", and on MC2 those are two different questions.
    ///
    /// ⚠⚠ **DO NOT UNIFY WITH [`super::super::mc2::castle::MC2_CASTLE_COST`].**
    /// The two games agree on rungs 1..=6 and differ at BOTH ends —
    /// rung 0 (**5,000** here vs MC2's **1,000**) and rung 7
    /// (**30,000,000** vs **300,000,000**). Rungs 1-6 matching is
    /// shared DESIGN, not evidence the ladders are one table; the
    /// ends are where the games actually diverge, and rung 0 is the
    /// load-bearing one (docs/DEVIATIONS.md `castle_recast_cost`:
    /// stamping CAP[0] = 5,000 on teardown against a 1,000 purse IS
    /// MC1's first-castle lockout).
    ///
    /// ⚠ Rung 7 is a REAL, ENFORCED CAPACITY CEILING that doubles as
    /// a deliberately unaffordable PRICE — player-certified on retail
    /// 2026-08-24 via a mana cheat: a pool above 30,000,000 is
    /// EXPELLED, so the cap genuinely works. What is degenerate is
    /// the MISSING LEVEL CAP behind it, which both engines share:
    /// with 30M banked the level-8 upgrade becomes affordable and the
    /// spell FIRES, but no castle is built and none of the
    /// out-of-range geometry ever appears — `sub_11A10` restores the
    /// extents on every failure path, so the cast is simply stranded
    /// in the transform sub-state forever. See the level-7 entries in
    /// docs/DEVIATIONS.md; the port's clamp is player-endorsed.
    pub(crate) const CASTLE_CAP: [i32; 8] =
        [5000, 10000, 20000, 40000, 80000, 160000, 320000, 30_000_000];

    /// sub_12C50 (:17616): the upgrade pre-clear — every house whose
    /// AABB overlaps the NEXT level's footprint grown by 256 is
    /// killed outright (life = -1 → the collapse walker evacuates).
    ///
    /// ⭐ The helper stamps the castle's OWN box as a side effect —
    /// sub_37150 at level+1 on entry (:17624, the loop then reads the
    /// stamped +80/+82) and a restamp at the CURRENT level on exit
    /// (:17639). For a level-0 castle the exit restamp is the
    /// 0xE000/640/640/0x4000 bare-flag box — how a newborn castle
    /// sheds its ctor art extents before any commit runs.
    fn castle_upgrade_preclear(&mut self, i: usize) {
        let next = (self.ent[i].f26 + 1).clamp(1, 8) as usize;
        self.castle_box_stamp(i, next);
        let half_w = self.ent[i].f80 as i32 + 256;
        let half_h = self.ent[i].f82 as i32 + 256;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if e.class64 == 10
                && e.model65 == 45
                && e.flags & 0x400 == 0
                // ⭐ `sub_12C50`'s PRE-CLEAR BOX IS INCLUSIVE, AND THE
                // PROOF IS THE JUMP MNEMONIC. `HIDDEN.EXE` 0x2B6AF-
                // 0x2B6E6 (identical at `CARPET.EXE` 0x2B4AF-0x2B4E6):
                //     mov 0x48(%ebx),%ax / sub %edi,%eax
                //     cwtl / cltd / xor / sub          (the abs16)
                //     cmp %ecx,%eax
                //     jg  0x2b6e6      <-- SKIP only when |dx| >  limit
                //     …jg…  movl $0xffffffff,0xc(%ebx)
                // `jg` is a STRICTLY-GREATER skip, so the kill fires at
                // `|dx| <= limit`; `jge` would have meant the port's `<`.
                // Listing agrees: remc1 :17616-41 / remc1hw :15748-72,
                // both marked SYNCHRONIZED, spell it `<=`. The `cwtl`
                // also confirms this `wrapping_sub as i16` abs16 form.
                //
                // Witness — an EXACT boundary hit, mc1hwl2 t=70, the
                // castle-plant tick: dwelling slot 8 sits at |dx| = 4352
                // against a limit of 2432 + (1664 + 256) = 4352 EXACTLY.
                // Retail razes it (`act_life 2000 -> -1`); the port's `<`
                // spared it, and that one off-by-one was mc1hwl2's whole
                // 69-tick horizon (69 -> 448 on the fix, census 560 -> 558,
                // removals only, zero rows introduced).
                //
                // ⚠ Do NOT propagate this to `castle_upgrade_space_ok`'s
                // `<` below — that is `sub_12D10`'s overlap test through
                // `sub_11950`, a different helper, never audited for the
                // same question.
                && wd(e.x, x) <= e.f80 as i32 + half_w + preclear_eq()
                && wd(e.y, y) <= e.f82 as i32 + half_h + preclear_eq()
            {
                self.ent[j].act_life = -1;
            }
        }
        let cur = self.ent[i].f26.max(0) as usize;
        self.castle_box_stamp(i, cur);
    }

    /// sub_12D10 (:17643, HW :15775 — byte-identical twins): the
    /// upgrade space gate — FAIL when another castle overlaps the
    /// next level's extents, or the GROWTH BANDS of the new
    /// footprint carry the protection bit. NOT a perimeter walk:
    /// retail reads the OLD half-extents before the level+1 stamp
    /// (:17666-67), takes the deltas (:17676-77), and when the
    /// half-height delta is ZERO runs NO terrain test at all. The
    /// footprint is 2h tiles wide starting at centre−h — the +h
    /// row/column an inclusive perimeter would test is retail-never
    /// -touched (mc1hwl0 t=971: the level-2 commit at (232,176) is
    /// clear everywhere except four protected tiles on row
    /// cy+hty, exactly the row only the perimeter scan reads).
    /// Retail's four bands share the MC2 twin's verbatim quirks
    /// (mc2_castle_space_scan, EF:4464-4535): the side slivers
    /// iterate only `my` rows at oy+my, and band 4's FIRST row
    /// starts at centre−mx then resets its x-cursor to ox on later
    /// rows, duplicating band 3 — corroborated across both
    /// binaries' decompiles.
    ///
    /// ⭐ Like the pre-clear, BOTH exits restamp the castle's box at
    /// the current level (:17777/:17781) after the entry stamp at
    /// level+1 (:17668) — the side effect is part of the law: retail
    /// runs this test from the selector's Upgrade admission
    /// (sub_14120 :18426) BEFORE the settled gate, so a newborn
    /// castle's box lands on the plant tick's re-decision (mc1hwl0
    /// t=18950 slot 233: 57344/640/640/16384 against the port's
    /// ctor art extents).
    pub(crate) fn castle_upgrade_space_ok(&mut self, i: usize) -> bool {
        // OLD half-extents in tiles, read BEFORE the stamp (:17666).
        let iw = (self.ent[i].f80 as i16 >> 8) as u16;
        let ih = (self.ent[i].f82 as i16 >> 8) as u16;
        let next = (self.ent[i].f26 + 1).clamp(1, 8) as usize;
        self.castle_box_stamp(i, next);
        let half_w = self.ent[i].f80 as i32;
        let half_h = self.ent[i].f82 as i32;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        let cur = self.ent[i].f26.max(0) as usize;
        let mut fits = true;
        if no_mc1_upgrade_space_chain() {
            for j in 1..self.ent.len() {
                let e = &self.ent[j];
                if j != i
                    && e.class64 == 3
                    && e.model65 == 2
                    && e.flags & 0x400 == 0
                    && wd(e.x, x) < e.f80 as i32 + half_w
                    && wd(e.y, y) < e.f82 as i32 + half_h
                {
                    fits = false;
                    break;
                }
            }
        } else {
            // :17649-53 — the TICK-TOP CLASS-3 ROSTER, model 2, not
            // self, `sub_11950` (three axes, `movswl` extents, 16-bit
            // wrapped differences). Membership is the sweep's
            // (`actLife >= 0 && !(flags & 0x10)`): a razed keep is
            // off the chain and cannot block. See
            // [`no_mc1_upgrade_space_chain`].
            let ext = |v: u16| v as i16 as i32;
            let (z, f78, f84) = (self.ent[i].z, self.ent[i].f78, self.ent[i].f84);
            for k in 0..self.wiz_chain.visible_len() {
                let j = self.wiz_chain.list[k] as usize;
                let e = &self.ent[j];
                if j != i
                    && e.model65 == 2
                    && wd(e.x, x) < ext(e.f80) + ext(self.ent[i].f80)
                    && wd(e.y, y) < ext(e.f82) + ext(self.ent[i].f82)
                    && ((e.z as i32 + ext(e.f78)) - (z as i32 + ext(f78))).abs()
                        < ext(e.f84) + ext(f84)
                {
                    fits = false;
                    break;
                }
            }
        }
        if fits {
            let (ow, oh) = ((half_w >> 8) as u16, (half_h >> 8) as u16);
            let ox = (x.wrapping_add(128) >> 8).wrapping_sub(ow) as u8;
            let oy = (y.wrapping_add(128) >> 8).wrapping_sub(oh) as u8;
            let (mx, my) = (ow.saturating_sub(iw) as u8, oh.saturating_sub(ih) as u8);
            let blocked = |gx: u8, gy: u8| self.t.angle[tile(gx, gy)] & 0x80 != 0;
            'bands: {
                // Bands 1+2: top rows oy.., bottom rows
                // oy+2*oh−my.., 2*ow columns from ox (:17679-717).
                for row in 0..my {
                    for col in 0..(2 * ow) as u8 {
                        if blocked(ox.wrapping_add(col), oy.wrapping_add(row))
                            || blocked(
                                ox.wrapping_add(col),
                                oy.wrapping_add((2 * oh) as u8)
                                    .wrapping_sub(my)
                                    .wrapping_add(row),
                            )
                        {
                            fits = false;
                            break 'bands;
                        }
                    }
                }
                // Bands 3+4: the side slivers at rows oy+my..,
                // mx columns (:17719-775) — band 4's first row
                // starts at centre−mx, later rows reset to ox.
                for row in 0..my {
                    for col in 0..mx {
                        let sx = if row == 0 {
                            ox.wrapping_add(ow as u8).wrapping_sub(mx)
                        } else {
                            ox
                        };
                        if blocked(ox.wrapping_add(col), oy.wrapping_add(my).wrapping_add(row))
                            || blocked(sx.wrapping_add(col), oy.wrapping_add(my).wrapping_add(row))
                        {
                            fits = false;
                            break 'bands;
                        }
                    }
                }
            }
        }
        self.castle_box_stamp(i, cur);
        fits
    }

    /// sub_46DB0 (:57023-32): direct ball absorption — an OWNED m39
    /// ball touching the castle empties into the store while the
    /// store sits below capacity (the whole ball lands; overflow is
    /// the ejector's business).
    ///
    /// ⭐ It walks the TICK-TOP BALL CHAIN (`dword_AE408 + 36466` =
    /// `var_u32_36462[1]`, the same roster the rival's ball pick reads
    /// at :18878), NOT the pool, and it returns on the FIRST match —
    /// so WHICH ball a castle drinks is decided by chain order, and a
    /// ball minted mid-tick is invisible until the next rebuild. The
    /// pool scan this replaced picked by SLOT INDEX and drank the
    /// wrong ball whenever two owned balls straddled the tower:
    /// mc1l5 t=2499, castle 312 takes ball 283's 140 in retail and
    /// ball 295's 2250 in the port (20400 vs 22510).
    ///
    /// ⚠ The predicate is `model65 == 39 && +144 == castle +24 &&
    /// overlap` and NOTHING else — no class test and no `0x400` test
    /// ("a soft kill is not a free"): the chain's own membership is
    /// the filter, exactly as in the balloon dispatcher above.
    fn castle_absorb(&mut self, i: usize) {
        if self.ent[i].f140 >= self.ent[i].f136 {
            return;
        }
        let own = self.ent[i].id24;
        for c in 0..self.ball_chain.visible_len() {
            let j = self.ball_chain.list[c] as usize;
            if self.ent[j].model65 == 39 && self.ent[j].f144 == own && self.ent_overlap(i, j) {
                self.ent[i].f140 += self.ent[j].f140;
                self.ent[j].flags |= 0x400;
                // :56030-42 — retail returns after the FIRST absorbed
                // ball: one ball per (every-other) settled tick, not a
                // same-tick vacuum of the whole pile.
                return;
            }
        }
    }

    /// The human's wizard seat — see [`Gen::human_seat`].
    pub(crate) fn human_seat(&self, hx: Hx) -> u8 {
        self.human_seat[hx].0
    }

    /// ⭐ THE HUMAN'S OWNER TAG IS HIS CARPET'S POOL SLOT — retail's
    /// own convention for every wizard (a class-3 carpet's `+24` is
    /// its slot, and owner/target fields hold that word). Called
    /// wherever the human's carpet record is seated (native spawn,
    /// conformance import, save resume). Slot 0 — no carpet record (a
    /// bare test world, the record A/B switches) — keeps the old
    /// out-of-pool sentinel. `MGC_NO_HUMAN_SLOT_TAG=1` keeps the
    /// sentinel everywhere (the A/B for the multiplayer stage 1).
    ///
    /// The record itself carries the tag too: a carpet's `+24` is its
    /// own slot in retail (the allocator default every wizard keeps),
    /// and resolvers that follow a source tag to its record and read
    /// THAT record's `+24` (a fire inheriting its poster's owner) must
    /// land back on the human. The record otherwise stays the zeroed
    /// class-0 husk ([`crate::engine::world::World::mc1_spawn_human_record`]).
    pub(crate) fn set_human_tag_slot(&mut self, hx: Hx, slot: u16) {
        let slot_tag = slot != 0 && !no_human_slot_tag() && (slot as usize) < self.ent.len();
        self.human_tag[hx] = HumanTag(if slot_tag {
            slot
        } else {
            crate::mc1::mobs::PLAYER_TARGET
        });
        if slot_tag {
            self.ent[slot as usize].id24 = slot;
        }
    }

    /// A wizard owner tag's team slot: PLAYER_TARGET = the human's
    /// seat ([`Gen::human_seat`], 0 in retail single-player), a rival's
    /// entity slot = its player slot (wizext var_48 in the original).
    /// A HUMAN owner's colour slot for retail's owner-coloured art:
    /// with one human it is retail's player 0 (single player stays
    /// byte-identical, `--seat` included); with several each human is
    /// his own wizard's colour — the MC2 castle flag and mana-worm
    /// paths mapped every human to slot 0, the host's (audit
    /// 2026-10-17). `None` for a non-human owner.
    pub(crate) fn human_colour_slot(&self, owner: u16) -> Option<u8> {
        let hx = self.hx_of_tag(owner)?;
        Some(if self.human_seat.len() > 1 {
            self.human_seat[hx].0
        } else {
            0
        })
    }

    pub(crate) fn owner_team(&self, owner: u16) -> Option<u8> {
        if let Some(hx) = self.hx_of_tag(owner) {
            return Some(self.human_seat[hx].0);
        }
        (owner != 0)
            .then(|| self.rival_ents.iter().position(|&e| e == owner))
            .flatten()
            .map(|s| s as u8)
    }

    /// A castle owner's claimed-house tally as retail's castle-side
    /// readers resolve it — `pool[castle +24].+160->+308`: the human's
    /// [`Gen::banked_houses`], a rival's [`Gen::rival_banked_houses`]
    /// row, 0 for an owner tag that names no wizard (retail would
    /// dereference a null wizext there). Under
    /// [`no_mc1_owner_house_tally`] every castle reads the human's.
    pub(crate) fn owner_houses(&self, owner: u16) -> i32 {
        if no_mc1_owner_house_tally() {
            // The pre-dig arm: every castle read THE (one) human's.
            return self.banked_houses[Hx::FIRST];
        }
        match self.owner_team(owner).map(|t| (t, self.hx_at_wizard(t))) {
            Some((_, Some(hx))) => self.banked_houses[hx],
            Some((t, None)) => self.rival_banked_houses[t as usize],
            None => 0,
        }
    }

    /// sub_37A00 (:44266): the mana BALLOON entity (class 3 m3) —
    /// life 10000, speed 48, cargo capacity 10000, behavior row 9,
    /// sprite 169. The castle dispatcher overwrites the ctor's
    /// state 7 with the working state 9 (:56355).
    /// Test seam for [`Self::spawn_balloon`].
    #[cfg(test)]
    pub(crate) fn spawn_balloon_for_test(&mut self, x: u16, y: u16, z: i16, own: u16) -> usize {
        self.spawn_balloon(x, y, z, own).expect("a balloon record")
    }

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
        // ⭐ THE PER-TEAM ART IDIOM IS TWO STATEMENTS, NEVER A FOLD.
        // Retail's balloon ctor stamps the BASE row and only then
        // shifts the index: `sub_36FA0_37360(v2, 169)` (:44286) and,
        // in the castle dispatcher that runs it, `+86 += var_48`
        // (:56347) — the same shape the possession claim uses at
        // `CARPET.EXE` 0x28EDE/0x28EEA then 0x28F1A-0x28F25.
        //
        // It matters because `set_sprite` also stamps the row's
        // EXTENTS, and `SPRITE_STATS` widths are DERIVED AT BOOT from
        // the TMAPS sprite aspect (`width = height * sprW / sprH`;
        // all 286 rows ship `width = 0`), so adjacent rows share a
        // box only when their sprites share a size. Folding the team
        // into the row is exactly the defect
        // `MGC_NO_MC1_HOUSE_FLAG_EXTENTS` fixed one band up: rows
        // 177..184 derive 369/411/423/436/400/400/411/400.
        //
        // ⚠ THE BALLOON BAND IS LATENT, NOT LIVE — deliberately fixed
        // in FORM only. Rows 169..176 (sprites 157..164, all 60x85)
        // derive one single box — width 564, height 800, draw_type 0,
        // frames89 1 — in BOTH the temperate and arctic banks, so the
        // fold is byte-identical on all shipped data and no fixture
        // or unit test can distinguish the two forms. Nothing
        // enforces that the eight balloon sprites stay the same size.
        let team = self.owner_team(own).unwrap_or(0) as u16;
        self.set_sprite(i, 169);
        self.ent[i].type86 = self.ent[i].type86.wrapping_add(team);
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
        let (bq, gq) = if mc1_oob_castle_inert("quota") && self.ent[i].f26 >= 8 {
            (0usize, 0usize) // probe: OOB level ⇒ retail's culled fleet
        } else {
            FLEET[self.ent[i].f26.clamp(0, 7) as usize]
        };
        // MC2's dispatcher twin (sub_60400 EF:61405) has not been
        // register-verified against the binary, so it keeps the live-
        // census stand-in: an empty register plus the adoption pass
        // below reproduces the old slot-order walk exactly.
        let is_mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        let mc2 = is_mc2 || no_balloon_reg();
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
        //
        // ⭐ EXCEPT THAT RETAIL'S SEAT TEST IS LIFE-ONLY AND CLASS-BLIND
        // (`sub_47400` :56329-49, `CARPET.EXE` 0x47536 `cmpl $0,0xc(%ebx)`
        // is the only read; see [`no_mc1_balloon_seat_life_only`]): a
        // seat whose balloon was sacrificed and re-minted as another
        // class STANDS while that record's life is >= 0, and is cleared
        // by the dead arm below — a pass late, no spawn — once it is
        // not. The pre-clear spawned the replacement the same pass, on
        // the dry-pool tick, into the recycle victim retail never
        // popped (mc1l20 t=18192 slot 963, t=18194 slot 950).
        if no_mc1_balloon_seat_life_only() {
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
        }
        // The "castle full" test reads the OWNER's tick-top census
        // snapshot (:56363 `pool[+24].+160->+308`), never a live sum
        // — see [`no_mc1_owner_house_tally`]. MC2's twin (`sub_60400`)
        // keeps the live sum until it is register-verified.
        let snapshot_tally = !is_mc2 && !no_mc1_owner_house_tally();
        let mut house_tally = if snapshot_tally {
            self.owner_houses(own) as i64
        } else {
            0i64
        };
        let mut orphans: Vec<usize> = Vec::new();
        for j in 1..self.ent.len() {
            let e = &self.ent[j];
            if e.flags & 0x400 != 0 {
                continue;
            }
            match (e.class64, e.model65) {
                (3, 3) if e.id24 == own && !reg.contains(&(j as u16)) => orphans.push(j),
                (10, 45) if !snapshot_tally && e.f144 == own => house_tally += e.f140.max(0) as i64,
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
        // ⛔ THIS MC2 ARM IS UNREACHABLE and the census question it
        // hedged on is ANSWERED. MC2 castles dispatch to
        // [`Gen::mc2_castle_tick`] (world.rs, the
        // `3 if Mc2 && model65 == 2` arm), never to `castle_tick`, so
        // the live MC2 ladder is [`Gen::mc2_castle_roster`] — and it
        // walks the REGISTER. Retail agrees: `sub_5FF50` (EF:61647)
        // reads `Entities_EA3E4[v18x->dword_0xA4_164x
        // ->array_0x5C_92[v20]]` for v20 = 0..quota, spawns into
        // `array_0x5C_92.at(v20)` on an empty rung (EF:61800), and
        // CLEARS the rung + re-latches `word_0x2C_44 = 16` on a stale
        // one (EF:61813-14). RETAIL INDEXES A REGISTER; IT NEVER SCANS
        // THE POOL. Measured with a trace on both arms over
        // mc2l10-secondtake: 4,615 register passes, ZERO entries here.
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
        // ⭐ THE BALLOON HAS NO SELF-KILL. `sub_481D0` returns 2 and
        // the walker (:52405) casts the handler to `void(*)(Ent*)` and
        // DISCARDS the return — unlike the castle's twin `sub_47EC0`,
        // whose 2 is consumed at :56003 as the `+70 = 6` park. A dead
        // balloon is reaped by its own castle's FLEET DISPATCHER
        // (`sub_47400` :56354-58: drop the cargo, soft-kill, clear the
        // register slot), which runs only on the castle's EVEN `+63`
        // pass (:56016) — so the corpse lingers one or TWO ticks
        // depending on that parity, and the port must not shortcut it.
        // mc1l5 slot 325 measures the whole sequence: lethal mail at
        // t=492 (life 1600 → −1900) with flags still 0xc while castle
        // 301 sits on the odd `+63 = 21`, then the reap at t=493 —
        // flags 0x40c, `+144` 650 → 0, and the (10,39) cargo ball at
        // slot 233 carrying the balloon's own `+140` of 1024 at its
        // PRE-move t=492 axis. Killing it here spawned that ball a
        // tick early and burned the balloon's LCG draw with it.
        if self.ent[i].act_life < 0 {
            return; // :56820-21 — a dead balloon never re-consumes
        }
        if self.ent[i].mail[0].1 != 0 {
            let (amt, src) = self.ent[i].mail[0];
            self.ent[i].act_life -= amt as i32;
            // Balloon-under-attack flash (Type_160+393 = 4, :56826) —
            // ahead of the lethal test. ⚠ RETAIL'S WRITE LANDS IN THE
            // ALLOCATOR'S SINK: `*(a1+160)+393` is resolved through the
            // BALLOON's own `+160` (CARPET.EXE 0x481F4/0x481FD), which
            // `NewEvent_372C0` points at the static dummy `unk_B7330`
            // (0x373BF) and only a wizard record ever re-points at its
            // player block (:54866). Nobody draws the sink, so the
            // human's `+393` reads 0 on every record of every take
            // (round 153: 8,651 pair rows / 36 takes). The flash is
            // therefore dead in retail and the port arms it only under
            // `MGC_NO_MC1_BALLOON_ALERT_SINK` — see
            // `crate::mc1::combat::no_mc1_balloon_alert_sink`.
            if let Some(hx) = self.hx_of_tag(self.ent[i].id24)
                && crate::mc1::combat::no_mc1_balloon_alert_sink()
            {
                self.balloon_alert[hx] = 4;
            }
            if self.ent[i].act_life < 0 {
                // :56829 — stamp the killer and leave. ⚠ Unlike the
                // castle twin (:56696) this arm clears NEITHER `+94`
                // nor `+90`: the record shows the letter (3500, 289)
                // still standing at t=492 AND t=493, which is exactly
                // what the early-out above then refuses to re-eat.
                self.ent[i].f38 = src;
                return;
            }
            self.ent[i].mail[0].1 = 0; // :56833 — the SURVIVE path only
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

    /// The class-3 m2 CASTLE. Retail dispatches it through THREE
    /// separate handlers keyed on the job byte `+70` — the rows at
    /// :4673-75 are `{…, 4, sub_46DB0}`, `{…, 5, sub_46F10}`,
    /// `{…, 6, sub_470E0}` — so `+70` is the MACRO state:
    ///
    ///   4 = SETTLED   (sub_46DB0 :55978) — the only damage processor
    ///   5 = TRANSFORM (sub_46F10 :56043) — sub-state in `+48`
    ///   6 = LEVELER   (sub_470E0 :56138) — the deferred downgrade
    ///
    /// `tick70` carries `+70` verbatim and `f59` carries the transform
    /// sub-state `+48` (the port has no `+48` lane; `+59` is dead for
    /// castles). ⚠ The two were long FUSED into `f59` alone, with
    /// `f59 == 4` standing in for settled — which parked every port
    /// castle at `tick70 = 5` forever and made the rival's upgrade
    /// predicate (`castle.tick70 == 4`, mirroring :18428) unreachable
    /// in every free run.
    ///
    /// Remaining housekeeping: the overflow ejector, downgrade/
    /// respawn. The entity z (+76) refreshes to live ground on the
    /// settled tick (:55997/:56014) and the pure waits (:56073-78) —
    /// the flag rides the painted tower; the build-site datum lives
    /// in f28 (+154).
    pub(crate) fn castle_tick(&mut self, i: usize, patches: crate::patches::WorldPatches) {
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
        //
        // ⭐⭐⭐ AND THE LEVELER IS GATED ON THE FREE STACK. `sub_470E0`
        // opens `result = sub_37710_37AD0(); if (result) { …teardown… }
        // else { *(BYTE *)(a1 + 70) = 4; }` (:56142-56156), and
        // `sub_37710_37AD0` is `return *(_DWORD *)(base + 40) + 1;`
        // (:44061-64) — the FREE-STACK DEPTH (its `+40` is the top
        // index, −1 when empty; the siblings at :44586 / :45028 read
        // the same call as `if (… < 16) return 0;` right before
        // `NewEvent_372C0`). So on an EXHAUSTED POOL retail runs NO
        // teardown at all: no rung decrement, no ladder reset
        // (`sub_47C60_47FA0`), no ejector, no fleet re-quota — it only
        // parks `+70` back to 4, and the settled handler's still-fatal
        // life parks it straight back to 6. A besieged castle in a
        // starved world therefore STANDS AT ITS OLD LEVEL WITH ITS
        // NEGATIVE `act_life` INTACT, absorbing more overkill every
        // hit, for as long as the pool stays full.
        //
        // mc1l48 slot 916 is the exemplar (the port's own pair lane):
        // t=11609 `f26 2 / max_life 40000 / act_life 3200 / +70 4`,
        // free depth 475; t=11610 the fatal hit lands `act_life −2000`
        // and `+70 6`, depth 322; t=11612 depth **0** and the castle is
        // STILL `f26 2 / max_life 40000 / act_life −2000` — retail's
        // leveler ran and did nothing. The ungated port tore it down in
        // the very same pair: `f26 1`, `max_life` `CASTLE_HP[1]` 20000,
        // `act_life` 20000 − 2000 = 18000, `+136` `CASTLE_CAP[1]`
        // 10000, and the owner's Create-Castle token (12,16) re-priced
        // 20000/198 → 10000/99. ⭐ That is the whole "retail runs deeply
        // negative while the port sits at full life a tier below"
        // family: the port's ARITHMETIC is already retail's
        // (`sub_47BD0_47F10`'s `act_life = a3 − min(|overkill|, a3/2)`
        // is what makes `port_life == port_max_life − |retail_life|`
        // hold, the a3/2 clamp included) — it is the GATE that was
        // missing, so every skipped teardown drops the port one more
        // rung below retail.
        //
        // Kill switch `MGC_NO_MC1_LEVELER_POOL_GATE=1` restores the
        // ungated downgrade.
        if self.ent[i].tick70 == 6 {
            self.ent[i].tick70 = 4;
            if !self.free.is_empty() || mc1_no_leveler_pool_gate() {
                self.castle_downgrade(i);
            }
            return;
        }
        // ⭐ SETTLED — `+70 = 4`, its OWN handler (sub_46DB0 :55978),
        // not a case of the transform machine.
        if self.ent[i].tick70 == 4 {
            self.castle_settled_tick(i);
            return;
        }
        // `+70 = 5`: the transform machine (sub_46F10 :56043), keyed
        // on the sub-state. The ground refresh belongs to the pure
        // waits ONLY (cases 1/4/6, :56073-78) — the action cases keep
        // the stale z: the level-up commit tick still shows the ctor's
        // raw-point ground (mc1l0 t=563: z 797 held while the corner
        // reads 864). Retail's waits 1 and 4 are the same pure wait
        // (the level-up painter's and the repaint painter's) and fold
        // onto our 1.
        if matches!(self.ent[i].f59, 1 | 4 | 6) {
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            self.ent[i].z = self.ground_z(x, y) as i16;
            // ⭐ THE ORPHANED WAIT (`mc1_castle_transform_watchdog`,
            // patches.rs). A wait sub-state is left ONLY by its
            // worker's finish — the (10,42) painter writes 5, the
            // (10,41) leveler writes 2 (`finish_castle_painter`,
            // `tick_castle_leveler`) — so a wait whose worker is gone
            // is a wait forever: no damage read, no upgrade, no fleet
            // pass, the owner respawning at it for the rest of the
            // level (mc1l26 slot 623 from t=27344, the leveler eaten
            // by the dry pool's stale-victim seizure). The worker is
            // minted in the same dispatch that enters the wait and
            // reap-flags itself only AFTER writing the next sub-state,
            // so in normal play one always stands here — flagged or
            // not, it counts. Patched: take the leveler's own shake
            // exit (:30333 else arm), sub-state 2, which this same
            // dispatch consumes into SETTLED; the terrain keeps
            // whatever the worker had done.
            let orphaned = !(1..self.ent.len()).any(|w| {
                let e = &self.ent[w];
                e.class64 == 10 && matches!(e.model65, 41 | 42) && e.x == x && e.y == y
            });
            if orphaned {
                let (n, first) = self.castle_watchdog_fired.0;
                let t = crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed);
                self.castle_watchdog_fired.0 =
                    (n.saturating_add(1), if n == 0 { t } else { first });
                if patches.mc1_castle_transform_watchdog {
                    self.ent[i].f59 = 2;
                }
            }
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
                // ⭐ THE REQUEST BIT SURVIVES A FULL POOL. The clear
                // is not the entry to sub-state 0 — retail spends it
                // on exactly two paths: here on the space REJECT
                // (:56070 `+16 = v2 & 0xBF`, alongside `+48 = 2`) and
                // inside `sub_47960_47CA0`'s painter guard (:56475).
                // Clearing it up front made a pool-full tick eat the
                // request and drop the castle back to `established`
                // with nothing done. See
                // [`no_mc1_castle_upgrade_latch`] for the witness.
                if no_mc1_castle_upgrade_latch() {
                    self.ent[i].flags &= !0x40;
                }
                self.castle_upgrade_preclear(i);
                if self.ent[i].f26 > 0 && !self.castle_upgrade_space_ok(i) {
                    // :56070 — the reject spends the request too.
                    self.ent[i].flags &= !0x40;
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
                // the one-time team sprite stamp — a raw `+86 +=
                // wizard +48` onto the ctor's flat 177, the same
                // wizext var_48 recolor family as the claimed
                // dwelling (:30808-09; rows 177-184 are the eight
                // team flags). Raw like retail: no extent re-derive
                // (the commit prices its own build-row extents
                // below). The flag billboard's art is keyed off this
                // row alone — there is no pose.team recolor stage —
                // so skipping the stamp flew the HUMAN's white flag
                // on every rebuilt rival castle.
                if self.ent[i].flags & 2 == 0 {
                    self.ent[i].flags |= 2;
                    self.ent[i].type86 += self.owner_team(own).unwrap_or(0) as u16;
                }
                let Some(p) = self.spawn_creator(42, x, y, site_z) else {
                    return;
                };
                // :56475 — the commit consumes the upgrade request,
                // INSIDE the painter guard. Without the clear the
                // settled tick's flag check re-launches the level-up
                // forever.
                self.ent[i].flags &= !0x40;
                // The commit binds the owner's wizext+50 (:56484) —
                // inside the painter-spawn guard, like everything
                // else in the commit.
                if let Some(ws) = self.owner_team(own) {
                    self.castle_reg[ws as usize] = i as u16;
                }
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
                    // +42 (:56484-91) — `link42`, followed by the
                    // workers since w154j. f146 stays 0 like the
                    // recorded painters.
                    let e = &mut self.ent[p];
                    e.f71 = lvl as u8;
                    e.id24 = own;
                    e.link42 = Link42(i as u16); // +42 = castle slot (:56486)
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
                // ⭐ CASE 5 SNAPS z TOO, unconditionally, BEFORE the
                // leveler spawn (:56090 `+76 = sub_11F50(+72)`;
                // CARPET.EXE VA 0x46FF1 `push esi(+72) / call
                // sub_11F50 / mov [ebx+0x4c],ax`, then `call
                // sub_47080` at 0x46FFF). See
                // [`no_mc1_castle_leveler_ground`].
                if !no_mc1_castle_leveler_ground() {
                    let (x, y) = (self.ent[i].x, self.ent[i].y);
                    self.ent[i].z = self.ground_z(x, y) as i16;
                }
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
                        e.link42 = Link42(i as u16); // +42 = castle slot (:56130)
                    }
                    self.ent[i].f59 = 6; // authentic wait state (:56132)
                }
            }
            // Leveler done → SETTLED (case 2 :56078-81): the macro
            // state hands back to `+70 = 4` and the sub-state re-arms
            // to 0. The `sub_46D20(a1, 0)` between them is the
            // owner's Create-Castle charge-pin RELEASE, censused by
            // the caller in `World::step`.
            2 => {
                self.ent[i].tick70 = 4;
                self.ent[i].f59 = 0;
            }
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
                        e.link42 = Link42(i as u16); // +42 = castle slot (:56111)
                    }
                    self.ent[i].f59 = 1; // wait for the repaint painter
                }
            }
            // 1 = waiting for a painter, 6 = waiting for the
            // leveler (the original's pure waits, :56073-78): the
            // mailbox and any pending lethal accrue untouched.
            _ => {}
        }
    }

    /// SETTLED, `+70 = 4` (sub_46DB0 :55978) — the castle's own
    /// handler, and the ONLY damage processor: the blast-shake
    /// countdown FREEZES everything else while it runs (:55981-93 —
    /// the mailbox accrues, processing waits), then the ch0 damage
    /// intake (sub_47EC0 :56678), the ch5 upgrade intake (:56690-95 —
    /// sender must be the owner, max level 7), and the every-other-
    /// tick block (:56016-37): overflow ejector, balloons, absorption.
    ///
    /// Every exit from here is a `+70` write: the shake expiry and the
    /// upgrade request hand off to the transform machine (`= 5`), a
    /// lethal hands off to the leveler (`= 6`).
    fn castle_settled_tick(&mut self, i: usize) {
        {
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
                    // :55987-89 — `+70 = 5`, `+48 = 3`, `+50 = 0`,
                    // and NO ground refresh on this tick (the else
                    // arm below is the only one carrying :55997).
                    self.ent[i].f50 = 0;
                    self.ent[i].tick70 = 5;
                    self.ent[i].f59 = 3;
                } else {
                    self.ent[i].f50 -= 1;
                    let (x, y) = (self.ent[i].x, self.ent[i].y);
                    self.ent[i].z = self.ground_z(x, y) as i16;
                }
                return;
            }
            {
                let (x, y) = (self.ent[i].x, self.ent[i].y);
                self.ent[i].z = self.ground_z(x, y) as i16; // :56014
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
                    if let Some(hx) = self.hx_of_tag(self.ent[i].id24) {
                        // "Castle under attack" flash (Type_160+391=4).
                        self.castle_alert[hx] = 4;
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
                    self.ent[i].tick70 = 5;
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
        // ⚠ PROBE ONLY — see [`mc1_oob_castle_inert`]. The state
        // handback still runs (retail's OOB teardown still parks the
        // 5-tick repaint timer, which is what mints the (10,42)); the
        // ejector, the ladder/HP reset and the un-stamp do not.
        if mc1_oob_castle_inert("teardown") && lvl0 >= 8 {
            self.ent[i].f50 = 5;
            self.ent[i].f59 = 0;
            return;
        }
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
                // Retail's build (:56517-24) writes NO flag word: the
                // scratch keeps whatever the last collapse left (its
                // own `|= 0x400` mark persists across demolishes),
                // and the chase lost-test reads those bytes verbatim.
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
            // binding drops — retail clears wizext+50 blind
            // (:56534) — and the entity soft-kills; the wrapper
            // tail below still runs on it this tick, exactly like
            // retail's freed-but-live record. (The sub_46D20(a1, 0)
            // call in that arm is the spell-16 charge-pin clear on
            // the owner's Create Castle manifestation slot — wizext
            // +708 — not a balloon release; the world-side death
            // stamp handles the token.)
            if let Some(ws) = self.owner_team(self.ent[i].id24) {
                self.castle_reg[ws as usize] = 0;
            }
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
            // (:56158-59 `+48 = 0` / `+50 = 5` → the settled
            // countdown → back to `+70 = 5` at sub-state 3). The
            // `+70 = 4` handback itself is :56145, already stamped by
            // the caller before the teardown ran. A dead castle skips
            // the timer — the tick-top reap collects it first.
            self.ent[i].f50 = 5;
            self.ent[i].f59 = 0;
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
        // The CASTLE OWNER's tally (:56185 resolves `+24` → wizext →
        // +308), not the human's — see [`no_mc1_owner_house_tally`].
        let houses = self.owner_houses(self.ent[i].id24);
        let mut spill = if houses.saturating_add(stored) > cap {
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
        // :56194-96 (hw:52258-60) — the WHOLE ejector body is gated
        // on pool headroom: sub_37710_37AD0 returns the free-slot
        // COUNT (top index +40 init −1, pre-increment push — hw:40471
        // /40217), so an exhausted pool fails the gate twice (before
        // and after the reaper; ours frees eagerly, so the reap arm
        // is vacuous) and the ejector returns WITHOUT A SINGLE DRAW.
        // mc1hwl0 t=25315-23: the eruption drained the stack to 0 at
        // castle 340's even-f63 ticks — the port's fail-open spawns
        // drew 4 magnet yaws/tick retail never drew, forking the
        // castle's LCG phase (5 LOCAL heads from one law).
        if self.free.is_empty() {
            // The gate's retry arm reaps AND disarms the recycle top
            // (:56196 `var_u32_4593 = -1`) — a landing-armed stack
            // must not survive past the ejector's empty tick.
            self.mc2_recycle.stack.clear();
            return;
        }
        // :56198-205 — the throw count is the clamp AND the headroom
        // COUNT (not count+1 — the old read misparsed sub_37710).
        let count = (spill / 1000).clamp(1, 32).min(self.free.len() as i32);
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
        // hw:52307-20 (:56243-56): each magnet is allocated AT THE
        // CASTLE first, and only a SUCCESSFUL allocation stamps the
        // owner, draws the castle's yaw and relinks 25 tiles out — a
        // failed alloc consumes NO draw (the old draw-then-spawn
        // order burned the castle's stream on a short pool).
        for _ in 0..4 {
            let Some(m) = self.spawn_mana_magnet(cx, cy, cz, own) else {
                continue;
            };
            let yaw = (lcg32(&mut self.ent[i].rand) & 0x7FF) as u16;
            let mut pos = (cx, cy, cz);
            Self::polar_step(&mut pos, yaw, 0, 6400);
            self.move_relink(m, pos.0, pos.1, pos.2);
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
            // ⭐ THE MAGNET IS BORN OFF THE TILE CHAIN. :47694-96 is a
            // RAW `+72`/`+76` store — `sub_41CF0` is never called — so
            // a fresh magnet carries NO link bit. The old `link` here
            // was invisible on the castle-ejector path (which relinks
            // 25 tiles out immediately, and the corpus pin of flags 5
            // is that relink's bit, not the ctor's) and wrong on the
            // Mana Magnet bolt's detonation, which does not relink:
            // mc1l0-sg t=2733 slot 41 reads retail 1 vs the port's 5.
            // Its sibling the (10,55) Global Death field (sub_3BA00
            // :47705-34) is built the same way.
            e.x = x;
            e.y = y;
            e.z = z;
        }
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
        // exactly as for the acquire scans.
        //
        // ⭐⭐ THE ONLY PER-NODE TEST IS `+65 == 39` (:31252). There
        // is no class test (chain membership supplies it) and — the
        // part that cost a horizon — **NO 0x400 TEST**. A SOFT-KILLED
        // BALL IS STILL A MAGNET'S CUSTOMER: retail's reap is the next
        // tick's top-of-frame sweep, so a ball collected earlier in
        // THIS pass keeps its class, model and link and every magnet
        // below it still overwrites its ch4 pair. The port's invented
        // `0x400` guard made the LAST magnet to stamp a dying ball
        // depend on where the kill landed in the pool walk, and the
        // ball's ch4 arm turns `+30` to whichever magnet stamped last
        // — mc1hwl0 t=2005: ball 823 is flagged between magnets 260
        // and 786, so retail's `+30` is 957 (the bearing to magnet
        // 787, which stamps last) where the port kept 1888 (magnet
        // 256, the last one that stamped before the flag). Same
        // family as the collector test twenty lines into `ball_tick`:
        // ⚠ SOFT KILL IS NOT A FREE.
        for k in 0..self.ball_chain.visible_len() {
            let j = self.ball_chain.list[k] as usize;
            if self.ent[j].class64 == 10 && self.ent[j].model65 == 39 {
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
                        // +144 only — retail's grave-claim sweep
                        // (:29644-52) re-points and never re-derives;
                        // a settled ball keeps its stale row until its
                        // own moving arm (:29569) reads the new owner.
                        self.ent[j].f144 = claimant;
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
        // ⭐ THE HOUSE'S `+40` IS A ONE-TICK ATTACKER REGISTER, NOT A
        // LATCH (round 154, w154e). `sub_29640` (:31075) is the FIRST
        // call of the state-52 handler `sub_28DC0` (:30783; the only
        // caller — `CARPET.EXE` file 0x415C0 `e8 73 08 00 00`), and its
        // first instruction after the argument load is file 0x41E3D
        // `66 C7 40 28 00 00` = `mov word [eax+0x28],0` — BEFORE the
        // life test and the `+94` mailbox gate. The port only ever
        // wrote `f40 = src` on a hit and never cleared it, so from the
        // tick after the first hit the house carried the attacker
        // forever: mc1l2 slot 2 `f40` retail 0 / port 295 from t=5676
        // (the tick after the documented t=5675 self-hit) to the end,
        // 4,913 rows; round 153's census 442,171 free rows / 20 takes.
        // Retail reads `+40` only on the hit tick itself (the
        // :30797 `+528 = 200` wanted arm), so the stale value moved no
        // graded lane. `MGC_NO_MC1_HOUSE_HIT_REGISTER=1` restores the
        // latch (and the two mailbox quirks below).
        let hit_register = !no_mc1_house_hit_register();
        if hit_register {
            self.ent[i].f40 = 0;
        }
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
                if let Some(hx) = self.hx_of_tag(src) {
                    self.snd_player(hx, 4);
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
                // ⭐ THE COLOUR RIDES `+86` ALONE — THE EXTENTS ARE
                // ALWAYS ROW 177's. Retail is TWO statements, not one
                // (:30808-09), and the shipped CARPET.EXE settles the
                // order: `sub_28DC0` @0x28EDE `push 0xb1` / 0x28EEA
                // `call 0x36fa0` stamps the sprite from row 177 FLAT,
                // and only then 0x28F1A-0x28F25
                // (`mov cx,[ebx+0x56]; add cx,[eax+0x30]; mov
                // [ebx+0x56],cx`) adds the claimant wizext's `+48`
                // team colour onto `+86`/type86. Folding the two into
                // `set_sprite(177 + team)` takes the extent quad off
                // the TEAM's row — and rows 177..184 do NOT share a
                // sprite: TMAPS ids 165..172 measure 36x39, 36x35,
                // 36x34, 36x33, 36x36, 36x36, 36x35, 36x36, and the
                // boot pass (`sub_58F90`, CARPET.EXE 0x58F90; the
                // static table ships width = 0 for EVERY row) derives
                // `width = height * sprW / sprH` = 369, 411, 423, 436,
                // 400, 400, 411, 400. Row 177's 369 halves to the 184
                // every retail house wears; a team-2 claim under the
                // fold wore 211 and its AABB reached 27 units further
                // on each side. mc1l37 t=7177 is the witness: the
                // wyvern's (10,0) fire cell at x=38075 sits 325 from
                // house 676 at 38400, which clears retail's 184 + 128
                // = 312 and FAILS the port's 211 + 128 = 339, so the
                // port billed one extra 400 (life 1200 -> 800) a tick
                // before retail's own trail reached the wall.
                // (`MGC_NO_MC1_HOUSE_FLAG_EXTENTS=1` restores the fold.)
                let team = self.owner_team(src).unwrap_or(0) as u16;
                if no_mc1_house_flag_extents() {
                    self.set_sprite(i, 177 + team);
                } else {
                    self.set_sprite(i, 177);
                    self.ent[i].type86 = self.ent[i].type86.wrapping_add(team);
                }
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
            // ⭐ THE LETTER IS CONSUMED ON THE NON-LETHAL BRANCH ALONE,
            // BOTH HALVES (round 154, w154e). :31085-91, file
            // 0x41E6B-0x41E8D: a lethal subtract stamps `+38 = src` and
            // returns 2 with `+90`/`+94` UNTOUCHED (the dead house at
            // state 53 keeps its last letter — mc1l0 slot 7 t=4947-48
            // `mail0.src` retail 630 / port 0); a survivor stamps `+40
            // = src`, `+90 = 0` (`mov dword [edx],0`), `+94 = 0`. The
            // port cleared the src BEFORE the life test and never the
            // amount: mc1l2 slot 2 `mail0.amt` retail 0 / port 400 from
            // t=5675, mc1l0 slot 7 t=4832..4945.
            if !hit_register {
                self.ent[i].mail[0].1 = 0;
            }
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
            if hit_register {
                self.ent[i].mail[0] = (0, 0);
            }
            if self.ent[i].f26 > 2 {
                self.ent[i].f26 -= 1;
                let (x, y, z, f80) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z, e.f80)
                };
                let sx = x.wrapping_add(f80);
                // The spawn axis is the HOUSE's whole position
                // (:30791-92 copies pos, then only x += f80) — z is
                // the house's own, NOT a ground probe. The newborn's
                // birth-frame `creature_move` settles it (−128/tick):
                // mc1l5 t=4458, house 19 at z=2560 pops slot 808 and
                // retail's boundary reads 2432 where the port's
                // ground-derived spawn read 1536.
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
                // ⚠ The test is `model65 <= 1` alone, but the WRITE
                // goes through the attacker record's +160 WIZEXT
                // pointer (`pool[+40].+160->u16_528 = 200`): only a
                // wizard record has a live wizext, so a model-0 FIRE
                // spreading onto the house arms NOBODY — the write
                // vanishes through the non-wizard's pointer (the
                // null-probe family, write face). mc1l32 t=45218: the
                // torched house pops its defender in both, but the
                // port's model-only gate armed the human's wanted off
                // the (10,0) fire and the 45231/45249 pack militia
                // acquired a carpet retail's scan, wanted 0, never saw.
                if self.is_human_tag(src)
                    || self
                        .ent
                        .get(src as usize)
                        .is_some_and(|e| e.class64 == 3 && e.model65 <= 1)
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
                    // :30825-27 — the periodic emit fills the scratch
                    // with the house axis, x += f80.
                    let (x, y, z) = {
                        let e = &self.ent[i];
                        (e.x.wrapping_add(e.f80), e.y, e.z)
                    };
                    self.building_emit_at(i, x, y, z);
                }
            }
        }
    }

    /// sub_28D10 (:30715): one villager from the emit mix — LCG%12:
    /// 0-1 militia m4, 2-3 migrant m14, 4-8 villager m13, 9-11
    /// settler m12 (their natural spawn states 25/85/79/73).
    ///
    /// Retail's sub_28D10 does NOT compute a position: it spawns at
    /// the shared scratch `word_AE454_AE444`, which each CALLER
    /// fills (the dropped second argument — the pre-struct
    /// prototype `sub_28D10(int, int)` survives commented out at
    /// remc1:539). The periodic emit fills it with the house axis
    /// +f80 (:30825-27); the collapse evacuation fills it with the
    /// footprint CELL (:30916-31).
    fn building_emit_at(&mut self, i: usize, x: u16, y: u16, z: i16) {
        let d = self.ent_rand(i) % 12;
        let model = match d {
            0 | 1 => 4,
            2 | 3 => 14,
            4..=8 => 13,
            _ => 12,
        };
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
        // `MGC_COLLAPSE_PROBE=<t0>:<t1>` — THE DEMOLITION LEDGER. Retail
        // stages the castle-level-down "fake collapse" in POOL SLOT 0
        // (:56515-24) and `sub_28FE0` spends that record's own `+4`
        // stream, two draws per knocked wall cell. The recording carries
        // slot 0's `+4` verbatim, so `LCG-distance(rand@t-1, rand@t)` is
        // retail's EXACT draw count for the tick — the only oracle that
        // grades this walk. Prints the port's count beside the walk's
        // geometry; compare it against the recording with the same LCG.
        let probe = std::env::var("MGC_COLLAPSE_PROBE").ok().and_then(|v| {
            let (a, b) = v.split_once(':')?;
            let t = crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed);
            (t >= a.parse::<u64>().ok()? && t <= b.parse::<u64>().ok()?).then_some(t)
        });
        let probe_seed = self.ent[i].rand;
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
            let ctl = self.dmg(c, cx, cy) as i8;
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
                let b = self.dmg(c, cx, cy);
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
                            // :30916-31 — the scratch holds the CELL
                            // here, not the house axis (mc1l37
                            // t=2537: seven evacuees at tile corners
                            // 190..196, not house.x+f80).
                            self.building_emit_at(i, wx, wy, ez);
                        } else {
                            self.spawn_creature(4, wx, wy, ez);
                        }
                    }
                    // Rubble (:30940-93).
                    let hi = b >> 4;
                    let lo = b % 16;
                    if let Some(tt) = crate::mail_trace() {
                        eprintln!(
                            "[clp] t={tt} cell=({x},{y}) b={b:#x} hi={hi} lo={lo} h={} an={:#x} ty={} rand={}",
                            self.t.height[t],
                            self.t.angle[t],
                            self.t.tile_type[t],
                            self.ent[i].rand
                        );
                    }
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
                let (sx, sy) = (x0.wrapping_add(gx as u8), y0.wrapping_add(gy as u8));
                let t = tile(sx, sy);
                let pre = self.t.height[t];
                self.smooth_cell(t);
                if let Some(tt) = crate::mail_trace() {
                    eprintln!(
                        "[clp] t={tt} smooth=({sx},{sy}) h {pre}->{} an={:#x} ty={}",
                        self.t.height[t], self.t.angle[t], self.t.tile_type[t]
                    );
                }
            }
        }
        if let Some(t) = probe {
            let end = self.ent[i].rand;
            let mut n = 0u32;
            let mut r = probe_seed;
            while r != end && n < 100_000 {
                r = r.wrapping_mul(9377).wrapping_add(9439);
                n += 1;
            }
            eprintln!(
                "[collapse] t={t} slot={i} row={} x0={x0} y0={y0} w={w} h={h} \
                 rand {probe_seed} -> {} draws={n}",
                e.f71, self.ent[i].rand
            );
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
            // `++dword_38C9F` (:31131) — the stats ledger's census.
            self.stats.0.census += 1;
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
    /// the 3x3 average over similarly-plain cells.
    ///
    /// ⚠⚠⚠ THE QUAD GATE'S INDEX ARITHMETIC IS **SIGNED 32-BIT**, NOT
    /// u16 (:42912-19): retail computes `(u16)a1 - 257` / `- 256` /
    /// `- 1` as int and indexes `mapTerrainType` with the RESULT — so
    /// for a ROW-0 cell (t < 257) the two "row −1" reads land BELOW
    /// the type plane, in the SOUND-DRIVER globals at CC0DF..CC1DF
    /// (word_CC070/byte_CC0C2/dword_CC126 & co, the HMI init block).
    /// Whether a row-0 cell smooths is decided by sound-driver bytes.
    /// Only the GATE escapes: the 3x3 SUM loop casts per access
    /// (`(u16)result`, :42928) and wraps into row 255 correctly, and
    /// the write is in-plane.
    ///
    /// [`OOB_TYPE_SHIM`] models those 257 bytes for the retail machine
    /// the corpus was recorded on. A cell (x, 0) reads shim `x` and
    /// `x + 1`, so each observed smooth/skip pins a PAIR of bytes —
    /// but ONLY when the cell's in-plane reads pass first, which is
    /// why the table fills in one collapse at a time.
    ///
    /// Measured mc1hwl0 t=21282 (castle 233's downgrade un-stamp
    /// straddles the y wrap): the shim's {56, 59, 71} read as
    /// building-typed — gate FAILS on cells (56,0)/(58,0)/(59,0)/
    /// (70,0)/(71,0) of the epilogue while (57,0)/(60,0)/(66,0)/
    /// (67,0)/(69,0) pass, including (66,0) whose port-side NW quad
    /// has the type-11 keep wall at (65,255) and previously SKIPPED
    /// (retail smooths it to 157 = the 8-plain-neighbor average).
    /// With this table the full 441-cell post-collapse height grid
    /// reproduces retail 441/441; without it, 20 cells drift (the
    /// t=21282 `(10,0)slot601:z` head and its whole fire-field
    /// cascade).
    ///
    /// {63, 65} came from the SECOND collapse, t=24739, where the
    /// castle is down to an 8x8 footprint (x 60..=67). That run
    /// rubbles (62..=65,0) — cells the 16-wide t=21282 stream
    /// SKIPPED, so they still carried building types 12/27/27/79 and
    /// failed the gate in-plane, leaving their shim bytes untested.
    /// Retiled to type 1 they reach the OOB reads for the first
    /// time, and retail still declines to smooth (62,0)/(64,0)/
    /// (65,0) while smoothing (61,0)/(66,0): shim 62/66/67 plain
    /// (61 and 66 smooth) forces 63 building. The 63..65 split was
    /// re-pinned by mc1hwl0's t=17754 collapse (x 60..67 straddling
    /// the y seam): (65,0) SMOOTHS there (938/6 = 156 = retail,
    /// with the downstream (64,1)/(65,1) cascade confirming both
    /// directions), so 65 is PLAIN and (64,0)'s independent skip
    /// pins 64 as the building-classed byte instead — the entry the
    /// earlier fit left unpinned. All five live in one 16-byte block
    /// (CC117..CC126, the last being `dword_CC126`'s low byte); the
    /// pattern is stable across the 3,457 ticks between the two
    /// collapses, so "static after boot" survives its first real
    /// test rather than being assumed.
    /// {7} came from the THIRD collapse, mc1hwl0 t=27767, where rival
    /// castle 860 at (0,0) is knocked from level 5 to 4 and un-stamps
    /// a footprint straddling the x wrap (x 241..=14). Its row-0
    /// epilogue smooths (5,0) and (8,0)..(10,0) but retail leaves
    /// (6,0) and (7,0) at their pre-smoother rubble heights EXACTLY
    /// (175 and 156 — neither is the 3x3 average, 168 and 167, so
    /// this is a skip and not a coincidence). (5,0) smoothing pins
    /// {5, 6} plain and (8,0) smoothing pins {8, 9} plain, so the one
    /// byte both skips share is {7}. Blast radius is exactly the two
    /// row-0 cells x=6 and x=7.
    /// {119} came from mc1l49 t=22193 — NOT a collapse, but a castle
    /// leveler's perimeter epilogue (`smooth_perimeter` cx=128 cy=0
    /// half_h=half_w=10 thick=3), whose left column strip x=115..118
    /// walks row 0 first. Retail smooths (115,0) 136->134, (116,0)
    /// 130->127 and (117,0) 120->117 (each the exact 3x3 average) and
    /// then leaves (118,0) at 103 where the average is 975/9 = 108 —
    /// a skip, not a coincidence. (115,0)/(116,0)/(117,0) smoothing
    /// pins {115, 116, 117, 118} plain, so {119} is forced. Blast
    /// radius is the two row-0 cells x=118 and x=119.
    ///
    /// {47, 55, 76, 85} came from mc1l49's t=17478 horizon (round 119,
    /// dig w2) — a `(5,2)slot138:z` head that is a TERRAIN break, not
    /// an entity one. The bee 138 rides its behavior row's altitude
    /// floor (`row 14 v_12 = 51`), so its z IS `ground_z(x,y) + 51`;
    /// at t=16972 a castle leveler's perimeter epilogue
    /// (`smooth_perimeter` cx=64 cy=0 half_w=17 thick=3) walks row 0 at
    /// x=44..47 and x=81..84, and the port smoothed (47,0) 124 -> 123
    /// where retail held 124. Three units of height byte = 3 engine
    /// units of `interp_plane`, and 506 ticks later the bee's floor
    /// reads 3992 against retail's 3995. The same tick smooths
    /// (81,0)/(82,0)/(83,0) exactly as retail does and then skips
    /// (84,0) where the average is 972/8 = 121: (83,0) smoothing pins
    /// {83, 84} plain, so {85} is forced. mc1l49 t=15427 does the same
    /// for {55} ((53,0) smooths -> {53, 54} plain, (54,0) skips) and
    /// for {76} ((74,0) smooths -> {74, 75} plain, (75,0) skips).
    ///
    /// {47} is forced by a SECOND take: mc1l37 t=9561, an unrelated
    /// leveler epilogue, smooths (44,0) 163 -> 161 and (45,0) 163 ->
    /// 160 bit-exactly with retail — pinning {44, 45, 46} plain — and
    /// then skips (46,0), which reads {46, 47}. mc1l49's own witness
    /// only narrows it to {47, 48}; the intersection is {47}. That
    /// take independently re-derives {55} ((54,0) skips at t=2740,
    /// 5730, 9470, 12604…), {76} ((75,0)/(76,0) skip at t=2740, 5802,
    /// 9470…) and {85} ((84,0) skips at t=9561, 11504, 12816, 14799)
    /// on terrain with no shared history — four bytes, two takes, no
    /// contradiction.
    ///
    /// {32, 39, 40, 48, 89} came from mc1hwl1 (round 122, dig Q5),
    /// the rest of the row-0 family dig W3 opened with {227}. Every
    /// one is FORCED to a single index — never a pair — because the
    /// neighbouring cell that shares the byte SMOOTHS bit-exactly with
    /// retail in the same tick. For {40}, {39}, {32} and {89} the
    /// whole height plane is byte-identical entering the tick
    /// (`MGC_PLANE_DIFF` hdiff 0); for {48} five cells are already
    /// adrift at t=9671, but all of them (x=39 and x=87..89) lie
    /// outside the x=47..50 windows in play, and the sum identity
    /// below proves the local neighbourhood was clean:
    ///   {40} — t=6644, the take's FIRST height drift (hdiff 1). The
    ///     port smooths (40,0) 62 -> 61 (556/9) where retail holds 62,
    ///     and in the same walk (41,0) 62 -> 61 matches retail exactly
    ///     (retail's own sum is 371/6 = 61 against the port's 370/6,
    ///     the one-unit difference being (40,0) itself). (41,0)
    ///     smoothing pins {41, 42} plain, so {40} is forced.
    ///   {39} — t=6730 and again t=7122. (37,0) smooths 68 -> 67
    ///     (610/9) bit-exactly with retail, pinning {37, 38} plain;
    ///     (38,0) then skips (retail holds 72 where the port writes
    ///     608/9 = 67), so {39} is forced. (39,0)'s own skip reads the
    ///     same byte and is explained by it.
    ///   {32} — t=6730, the same un-stamp epilogue. (33,0) smooths
    ///     65 -> 68 in both columns, pinning {33, 34} plain; (32,0)
    ///     skips (retail 65, port 604/9 = 67), so {32} is forced.
    ///   {89} — t=8849, a shoreline collapse at x=86..91. (87,0)
    ///     smooths 12 -> 18 in both columns, pinning {87, 88} plain;
    ///     (88,0) skips (retail 11) and (89,0) skips (retail 9), so
    ///     {89} — the byte they share — is forced. Blast radius is
    ///     those two cells plus the (87,255) row-wrap cascade.
    ///   {48} — t=9671. (49,0) SMOOTHS in retail to 61, which is
    ///     neither its pre-value 67 nor the port's 60: retail's sum is
    ///     552/9 = 61 exactly, the port's 545/9 = 60 differing only by
    ///     (48,0) (retail 69, port 62). That pins {49, 50} plain, and
    ///     (48,0)'s skip forces {48}. This SETTLES the open question
    ///     below.
    ///
    /// Per-byte attribution (each entry alone, `--segmented --brief`):
    /// on mc1hwl1 {32} -2, {40} -3, {39} -1, {89} -1 segments and all
    /// five together -18 — strongly superadditive, because any seed
    /// left unfixed re-contaminates the region the others cleaned. On
    /// the MC1 takes only {48} moves anything: mc1l37 5 segments -> 3
    /// and mc1l49 11 -> 10, with {32, 39, 40, 89} inert on every MC1
    /// take in the corpus.
    ///
    /// ⚠ mc1l37/mc1l49 are MC1 and mc1hwl1 is HIDDEN WORLDS, whose
    /// windows are 16 bytes apart (CC0DF+i vs CC0CF+i), so shim index
    /// i is not the same PHYSICAL byte in the two executables. The
    /// single shared table is a modelling economy that has not yet
    /// been contradicted — every MC1 take is byte-identical under
    /// these five entries. The day a contradiction appears the answer
    /// is to split the table on `GameId::Mc1Hw`, not to overwrite a
    /// byte.
    ///
    /// ⏭ WAS OPEN, NOW SETTLED BY {48}: mc1l37 t=12604 also skips
    /// (48,0) after smoothing (50,0) exactly, which forces a building
    /// byte in {48, 49} that no MC1 witness separates. mc1hwl1 t=9671
    /// separates it in favour of {48} — and mc1l37 and mc1l49 then
    /// BOTH improve under it, so the choice is corroborated by the
    /// very takes that could not make it. ⚠ The old note's "it cannot
    /// affect mc1l49 (whose smoother never visits (48,0)/(49,0))" is
    /// FALSE: mc1l49 goes 11 segments -> 10 under {48} alone.
    ///
    /// The 257 bytes retail's sub_360C0 quad gate reads BELOW the
    /// type plane for row-0 cells (addresses CC0DF..CC1DF — sound-
    /// driver state; see [`Gen::smooth_cell`]). Only the plain/
    /// building CLASS of a byte matters to the gate, so the observed
    /// building-classed offsets carry a representative 22; every
    /// unobserved byte defaults to 0 (plain). Indexed by
    /// `signed_index + 257`.
    const OOB_TYPE_SHIM: [u8; 257] = {
        let mut s = [0u8; 257];
        s[7] = 22;
        s[56] = 22;
        s[59] = 22;
        s[63] = 22;
        s[64] = 22;
        s[71] = 22;
        s[119] = 22;
        s[47] = 22;
        s[55] = 22;
        s[76] = 22;
        s[85] = 22;
        s[227] = 22;
        s[32] = 22;
        s[39] = 22;
        s[40] = 22;
        s[48] = 22;
        s[89] = 22;
        s[16] = 22;
        s[23] = 22;
        s[24] = 22;
        s[31] = 22;
        s[225] = 22;
        s[109] = 22;
        s
    };

    fn smooth_cell(&mut self, t: usize) {
        if self.t.angle[t] & 7 == 0 || self.t.height[t] == 0 {
            return;
        }
        let plain = |ty_val: u8| ty_val <= 5 || ty_val > 0x22;
        // The gate's four reads, at SIGNED offsets (see above): a
        // negative index resolves through the below-plane shim.
        let read = |idx: i64| -> u8 {
            if idx < 0 {
                // `MGC_NO_MC1_ROW0_SHIM_119=1` — see
                // [`mc1_no_row0_shim_119`].
                if idx == -138 && mc1_no_row0_shim_119() {
                    return 0;
                }
                if matches!(idx + 257, x if x == 47 || x == 55 || x == 76 || x == 85)
                    && mc1_no_row0_shim_47_55_76_85()
                {
                    return 0;
                }
                // `MGC_NO_MC1_ROW0_SHIM_227=1` — see
                // [`mc1_no_row0_shim_227`].
                if idx == -30 && mc1_no_row0_shim_227() {
                    return 0;
                }
                // `MGC_NO_MC1_ROW0_SHIM_16_23_24_31_225=1` — see
                // [`mc1_no_row0_shim_16_23_24_31_225`].
                if matches!(idx + 257, 16 | 23 | 24 | 31 | 225)
                    && mc1_no_row0_shim_16_23_24_31_225()
                {
                    return 0;
                }
                // `MGC_NO_MC1_ROW0_SHIM_32_39_40_48_89=1` — see
                // [`mc1_no_row0_shim_32_39_40_48_89`].
                if matches!(idx + 257, 32 | 39 | 40 | 48 | 89) && mc1_no_row0_shim_32_39_40_48_89()
                {
                    return 0;
                }
                // `MGC_NO_MC1_ROW0_SHIM_109=1` — see
                // [`mc1_no_row0_shim_109`].
                if idx + 257 == 109 && mc1_no_row0_shim_109() {
                    return 0;
                }

                if let Some(c) = row0_shim_override((idx + 257) as usize) {
                    return c;
                }
                Self::OOB_TYPE_SHIM[(idx + 257) as usize]
            } else {
                self.t.tile_type[idx as usize]
            }
        };
        let quad = [t as i64 - 257, t as i64 - 256, t as i64 - 1, t as i64];
        if !quad.iter().all(|&q| plain(read(q))) {
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
            // Saved since v19 (wire position: after `f26`) — a
            // mid-level save taken while a summon/charm is running
            // used to resume with a zero lease (dig 98-Q20 parked
            // the bump; the v19 wave spent it).
            lease2e,
            // The raw `+48` shadow is an IMPORT-ONLY lane (the native
            // path never writes it), so it stays off the wire and the
            // snapshot version does not move.
            raw48: _,
            // The worker→castle link: stamped at the mint, lifted at
            // import; a pre-field save resolves by the scan (0). Off
            // the wire like `raw48`; the version does not move.
            link42: _,
            // The cry cadence is a SOUND lane (no graded channel) and
            // re-arms within 24 ticks of a load, so it stays off the
            // wire exactly like `raw48` and the version does not move.
            morph_cry: _,
            // The summit arc counter's wide half is a CONFORMANCE lane
            // only: `f26` (which IS on the wire) carries the saturated
            // mirror every gate in `sub_32A70` reads, so a save/load
            // round trip is behaviourally exact and the version does
            // not move. See [`no_mc2_summit_arc_wide`].
            summit10: _,
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
        w.put(&lease2e.0);
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
        // Field order in the literal IS evaluation order, so the
        // `r.get()` calls below run in `put`'s write order — which is
        // why `lease2e` sits after `f26` rather than at its
        // declaration position.
        let mut e = Ent {
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
            lease2e: Lease2e(r.get()?),
            raw48: Raw48(0),
            link42: Link42(0),
            morph_cry: MorphCry(0),
            summit10: Summit10(0),
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
        };
        // The wide summit counter is off the wire (see the destructure
        // above); re-seed it from the `i16` mirror that IS saved so a
        // resumed summit keeps counting from where it stood instead of
        // restarting its pulse at t == 0.
        e.summit10 = Summit10(e.f26 as i32);
        Ok(e)
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
        // The seat is a few ticks of presentation: not stored (the
        // format stays), restored as every seat's.
        let PalFlash {
            row,
            ticks,
            seat: _,
        } = self;
        w.put(row);
        w.put(ticks);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(PalFlash {
            row: r.get()?,
            ticks: r.get()?,
            seat: None,
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

/// Same wire format as the `snap_newtype!` members: only `.0` is
/// written. `.1` (the price-register snapshot) is a within-dispatch
/// transient — empty at every boundary a snapshot can be taken at —
/// so keeping it out of the stream leaves the format byte-identical
/// and needs no `SNAPSHOT_VERSION` bump.
impl Snap for Mc2LadderMail {
    fn put(&self, w: &mut Writer) {
        w.put(&self.0);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Self(r.get()?, Vec::new()))
    }
}

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
            // Hash-SILENT: the Watcom CRT stream has no capture
            // channel (phase unrecoverable at import — see the field
            // doc), so pinning it would re-pin every golden over
            // state whose phase is arbitrary.
            crt_rand: _,
            spawn_count,
            player_mail,
            player_damage,
            erupting,
            plume,
            player_knock,
            // A within-walk register — see the field doc.
            mc1_buffet_post: _,
            // A per-tick transient the mover drains — see PlayerSpin.
            player_spin: _,
            // A per-tick transient the carpet's walk slot drains — see
            // PlayerWhirl.
            player_whirl: _,
            // A within-walk register — see the field doc.
            ww_walk_published: _,
            // A per-tick walk view — see the field doc.
            seat_view: _,
            // A within-walk register — see the field doc.
            m22_seg_residue: _,
            // A per-tick transient the carpet's walk slot drains — see
            // PlayerHurl.
            player_hurl: _,
            // A per-tick transient the carpet's dispatch drains — see
            // PlayerFloodPull.
            player_flood_pull: _,
            // A per-tick transient the wizard pass drains — see
            // DeflectDebit.
            player_deflect_debit: _,
            mc2_debuffs,
            rival_ents,
            // Save-silent (see the field doc): consequences ride
            // hashed entity lanes; imports reseed it every pair.
            castle_reg: _,
            mc2_life_scale,
            player_aggro,
            rival_wanted,
            player_invisible,
            player_ghost: _,
            player_vel: _,
            mc2_flyers_clear: _,
            skeletons_convert_traders: _,
            // A shim for the out-of-pool carpet's @0x20 — see the
            // field doc; every conformance import seeds it outright
            // from the recorded carpet, on the `player_chain`
            // precedent below.
            human_roll_0x20: _,
            player_rebound_reg: player_rebound,
            // Bookkeeping the SAVE does not carry, on the
            // `player_deflect_debit` precedent: `cell` restores as
            // `usize::MAX`, so the first mover tick after a restore
            // re-heads the human in his own cell (the right answer
            // the moment he arrives there, and self-healing on his
            // next tile crossing). Every conformance import seeds it
            // outright from the recorded carpet's own `next20`.
            player_chain: _,
            kills,
            shots,
            hits,
            // Carried by the WORLD stream's v26 tail (`World::snap_put`).
            stats: _,
            player_danger,
            banked_houses,
            // Save-silent like `castle_reg` (see the field doc): the
            // tick-top census rebuilds it before any reader runs.
            rival_banked_houses: _,
            castle_alert,
            player_alert,
            balloon_alert,
            // Hash-SILENT always (presentation), so nothing in the
            // acceptance test would notice this being dropped — the
            // `slot_gen` class of field. Saved because it is still
            // state: a save taken mid-flash restores mid-flash.
            pal_flash,
            exhausted,
            // Telemetry, never saved (a load starts the count afresh).
            castle_watchdog_fired: _,
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
            mc2_rival_leech,
            mc2_scrolls,
            mc2_spell_tokens,
            mc2_cast_xp,
            mc2_ladder_sync,
            // Transient within one dispatch — drained right after the
            // pushing slot, never live at a boundary.
            mc2_castle_lock_mail: _,
            // Presentation feed, never saved — a load starts clean.
            bolt_fx: _,
            volcano_blind: _,
            blast_hits: _,
            // Transient within one class-9 dispatch — never live at a
            // boundary.
            mc1_aim_latch: _,
            // Transient within one beam tick — never live at a boundary.
            mc2_beam_defer: _,
            // Retail's `predictedAxis_EB398ar` is a data-segment
            // scratch, rewritten by the first move of the next tick and
            // read by nothing before then — a save need not carry it.
            mc2_pred_axis: _,
            mc2_steal_mail,
            mc2_aura_claim,
            mc2_wanted,
            mc2_rebound_precise,
            mc2_allied,
            mc2_castle_research,
            // A per-tick echo of the DRIVER-owned flight ext, re-pushed
            // by every MC2 carpet dispatch and reseeded by the import:
            // hash-excluded and save-silent like `castle_reg`.
            mc2_mobilize: _,
            // The web-slow level's echo — same opt-out as `mc2_mobilize`.
            mc2_slow: _,
            // NEVER saved, and that IS the retail law: every load path
            // rebuilds the pool lists and then empties this one
            // outright (`sub_49F90(); D41A0_0.dword_0x11e6 = -1;` —
            // Level.cpp:304-305 / :423-424, EF:38829, :38874, :39467).
            // A restored world simply has no ranked victims until the
            // list next refreshes.
            mc2_recycle: _,
            // A within-tick stack-slot mirror (dig 126O) — it is `None`
            // at every tick boundary, so nothing to save.
            m27_v34_slot: _,
            // Re-seeded by `World::new` / the strict import beside
            // `mc2_carpet_slot`; never a save-boundary datum.
            mc2_pinned: _,
            // Same opt-out, MC1 side.
            mc1_pinned: _,
            mc1_guard_reg,
            mc1_balloon_reg,
            human_seat,
            // Load-static — see the field doc.
            human_tag: _,
            // Rebuilt at every tick top — never saved.
            ball_chain: _,
            wiz_chain: _,
            proj_chain: _,
            bldg_chain: _,
            paint_chain: _,
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
        for hx in player_mail.ixs() {
            w.put(&player_mail[hx]);
        }
        for hx in player_damage.ixs() {
            w.put(&player_damage[hx]);
        }
        w.put(erupting);
        w.put(plume);
        for hx in player_knock.ixs() {
            w.put(&player_knock[hx]);
        }
        for hx in mc2_debuffs.ixs() {
            w.put(&mc2_debuffs[hx]);
        }
        w.put(rival_ents);
        w.put(mc2_life_scale);
        for hx in player_aggro.ixs() {
            w.put(&player_aggro[hx]);
        }
        w.put(rival_wanted);
        for hx in player_invisible.ixs() {
            w.put(&player_invisible[hx]);
        }
        for hx in player_rebound.ixs() {
            w.put(&player_rebound[hx]);
        }
        for hx in kills.ixs() {
            w.put(&kills[hx]);
        }
        for hx in shots.ixs() {
            w.put(&shots[hx]);
        }
        for hx in hits.ixs() {
            w.put(&hits[hx]);
        }
        for hx in player_danger.ixs() {
            w.put(&player_danger[hx]);
        }
        for hx in banked_houses.ixs() {
            w.put(&banked_houses[hx]);
        }
        for hx in castle_alert.ixs() {
            w.put(&castle_alert[hx]);
        }
        for hx in player_alert.ixs() {
            w.put(&player_alert[hx]);
        }
        for hx in balloon_alert.ixs() {
            w.put(&balloon_alert[hx]);
        }
        w.put(pal_flash);
        w.put(exhausted);
        w.put(verb_fallbacks);
        w.put(misfits);
        w.put(sounds);
        w.put(terrain_dirty);
        w.put(mc2_night_shade);
        w.put(mc2_spawn_ord);
        for hx in mc2_player_drain.ixs() {
            w.put(&mc2_player_drain[hx]);
        }
        w.put(mc2_rival_leech);
        for hx in mc2_scrolls.ixs() {
            w.put(&mc2_scrolls[hx]);
        }
        for hx in mc2_spell_tokens.ixs() {
            w.put(&mc2_spell_tokens[hx]);
        }
        w.put(mc2_cast_xp);
        w.put(mc2_ladder_sync);
        w.put(mc2_steal_mail);
        w.put(mc2_aura_claim);
        w.put(mc2_wanted);
        for hx in mc2_rebound_precise.ixs() {
            w.put(&mc2_rebound_precise[hx]);
        }
        w.put(mc2_allied);
        w.put(mc2_castle_research);
        w.put(&mc1_guard_reg.0);
        w.put(&mc1_balloon_reg.0);
        for hx in human_seat.ixs() {
            w.put(&human_seat[hx].0);
        }
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
        for hx in self.player_mail.ixs() {
            self.player_mail[hx] = r.get()?;
        }
        for hx in self.player_damage.ixs() {
            self.player_damage[hx] = r.get()?;
        }
        self.erupting = r.get()?;
        self.plume = r.get()?;
        for hx in self.player_knock.ixs() {
            self.player_knock[hx] = r.get()?;
        }
        for hx in self.mc2_debuffs.ixs() {
            self.mc2_debuffs[hx] = r.get()?;
        }
        self.rival_ents = r.get()?;
        self.mc2_life_scale = r.get()?;
        for hx in self.player_aggro.ixs() {
            self.player_aggro[hx] = r.get()?;
        }
        self.rival_wanted = r.get()?;
        for hx in self.player_invisible.ixs() {
            self.player_invisible[hx] = r.get()?;
        }
        for hx in self.player_rebound_reg.ixs() {
            self.player_rebound_reg[hx] = r.get()?;
        }
        for hx in self.kills.ixs() {
            self.kills[hx] = r.get()?;
        }
        for hx in self.shots.ixs() {
            self.shots[hx] = r.get()?;
        }
        for hx in self.hits.ixs() {
            self.hits[hx] = r.get()?;
        }
        for hx in self.player_danger.ixs() {
            self.player_danger[hx] = r.get()?;
        }
        for hx in self.banked_houses.ixs() {
            self.banked_houses[hx] = r.get()?;
        }
        for hx in self.castle_alert.ixs() {
            self.castle_alert[hx] = r.get()?;
        }
        for hx in self.player_alert.ixs() {
            self.player_alert[hx] = r.get()?;
        }
        for hx in self.balloon_alert.ixs() {
            self.balloon_alert[hx] = r.get()?;
        }
        self.pal_flash = r.get()?;
        self.exhausted = r.get()?;
        self.verb_fallbacks = r.get()?;
        self.misfits = r.get()?;
        self.sounds = r.get()?;
        self.terrain_dirty = r.get()?;
        self.mc2_night_shade = r.get()?;
        self.mc2_spawn_ord = r.get()?;
        for hx in self.mc2_player_drain.ixs() {
            self.mc2_player_drain[hx] = r.get()?;
        }
        self.mc2_rival_leech = r.get()?;
        for hx in self.mc2_scrolls.ixs() {
            self.mc2_scrolls[hx] = r.get()?;
        }
        for hx in self.mc2_spell_tokens.ixs() {
            self.mc2_spell_tokens[hx] = r.get()?;
        }
        self.mc2_cast_xp = r.get()?;
        self.mc2_ladder_sync = r.get()?;
        self.mc2_steal_mail = r.get()?;
        self.mc2_aura_claim = r.get()?;
        self.mc2_wanted = r.get()?;
        for hx in self.mc2_rebound_precise.ixs() {
            self.mc2_rebound_precise[hx] = r.get()?;
        }
        self.mc2_allied = r.get()?;
        self.mc2_castle_research = r.get()?;
        self.mc1_guard_reg.0 = r.get()?;
        self.mc1_balloon_reg.0 = r.get()?;
        for hx in self.human_seat.ixs() {
            self.human_seat[hx].0 = r.get()?;
        }
        // Presentation feed — never saved, never inherited across a
        // load.
        self.bolt_fx.0.clear();
        self.volcano_blind.0.clear();
        self.blast_hits.0.clear();
        self.mc2_beam_defer = BeamDefer::default();
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

    /// The fit tests carry sub_37150's box stamp as a SIDE EFFECT
    /// (sub_12C50 :17624/:17639, sub_12D10 :17668/:17777/:17781):
    /// entry at level+1, exit restamp at the CURRENT level — level 0
    /// included (the guard that skipped level 0 was invented; retail
    /// stamps a newborn castle's bare-flag box the moment any
    /// admission test touches it — mc1hwl0 t=18950 slot 233). The
    /// (3,2) f78-f84 lanes are UNGRADED, so no pair fixture can pin
    /// this; the pick-gates fixture's tick exercises it and this test
    /// asserts it.
    #[test]
    fn the_fit_tests_stamp_the_castle_box() {
        let assets = synthetic_assets();
        let mut g = Gen::new(flat_land(8), assets, 1, ChassisParams::MC1, VerbSet::MC1);
        let c = g.new_event().unwrap();
        {
            let e = &mut g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.f26 = 0; // freshly planted, level 0
            e.x = 0x4000;
            e.y = 0x4000;
            // The ctor's art extents stand in for sprite row 177's.
            e.f78 = 200;
            e.f80 = 184;
            e.f82 = 184;
            e.f84 = 200;
        }
        let expect = |g: &Gen, lvl: usize| {
            let def = g.assets.build_tab[lvl % g.assets.build_tab.len()];
            (
                (((def.w as u16) << 8).wrapping_add(1280)) >> 1,
                (((def.h as u16) << 8).wrapping_add(1280)) >> 1,
            )
        };
        let _ = g.castle_upgrade_space_ok(c);
        let (w0, h0) = expect(&g, 0);
        assert_eq!(g.ent[c].f78, 0xE000, "the z-center marker landed");
        assert_eq!(g.ent[c].f84, 0x4000, "the z half-extent landed");
        assert_eq!(
            (g.ent[c].f80, g.ent[c].f82),
            (w0, h0),
            "the exit restamp is at the CURRENT level (0) — the art \
             extents are gone"
        );
        // The pre-clear restamps the same way.
        g.ent[c].f78 = 200;
        g.ent[c].f80 = 184;
        g.castle_upgrade_preclear(c);
        assert_eq!(g.ent[c].f78, 0xE000, "preclear exit stamp");
        assert_eq!(g.ent[c].f80, w0, "preclear restamp at current level");
        // And a leveled castle restamps at ITS level on exit.
        g.ent[c].f26 = 3;
        let _ = g.castle_upgrade_space_ok(c);
        let (w3, _) = expect(&g, 3);
        assert_eq!(g.ent[c].f80, w3, "exit restamp tracks f26");
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

    /// `sub_29640` (:31075-91, `CARPET.EXE` 0x41E3D-0x41E8D), the
    /// live house's ch0 intake: `+40 = 0` first thing on EVERY tick,
    /// then a survivor stamps `+40 = src` and consumes the letter
    /// (`+90 = 0`, `+94 = 0`), while a lethal subtract stamps `+38 =
    /// src` and returns with the letter untouched. `+40` and the
    /// mailbox are ungraded lanes (raw shadow `(10,45) f40` /
    /// `mail0.amt` / `mail0.src`: mc1l2 slot 2 from t=5675, mc1l0 slot
    /// 7 t=4832..4948).
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_MC1_HOUSE_HIT_REGISTER=1` — `f40`
    /// stays 630 on the quiet tick, the amount stays 400, and the
    /// lethal tick reads `mail0.src` 0.
    #[test]
    fn a_house_s_hit_register_lives_one_tick_and_the_letter_is_consumed_by_survivors_only() {
        let assets = synthetic_assets();
        let mut g = Gen::new(flat_land(8), assets, 1, ChassisParams::MC1, VerbSet::MC1);
        let b = g.new_event().unwrap();
        g.ent[b].class64 = 10;
        g.ent[b].model65 = 45;
        g.ent[b].act_life = 1000;
        g.ent[b].f26 = 1; // no militia pop-out
        g.ent[b].f63 = 1; // off the 40-tick occupancy beat
        // The hit tick.
        g.ent[b].mail[0] = (400, 630);
        g.tick_building_live(b, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[b].act_life, 600);
        assert_eq!(g.ent[b].f40, 630, "the survivor stamps +40 = src");
        assert_eq!(
            g.ent[b].mail[0],
            (0, 0),
            "…and consumes BOTH halves of the letter"
        );
        assert_ne!(g.ent[b].tick70, 53, "still live");
        // The quiet tick after it.
        g.tick_building_live(b, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[b].f40, 0, "+40 is zeroed at the top of every tick");
        // The lethal tick: +38 latches, the letter is left standing.
        g.ent[b].mail[0] = (700, 630);
        g.tick_building_live(b, crate::patches::WorldPatches::RETAIL);
        assert!(g.ent[b].act_life < 0);
        assert_eq!(g.ent[b].tick70, 53);
        assert_eq!(g.ent[b].f38, 630, "the killer latch");
        assert_eq!(
            g.ent[b].f40, 0,
            "+40 was zeroed before the subtract and never restamped"
        );
        assert_eq!(
            g.ent[b].mail[0],
            (700, 630),
            "the lethal branch returns before the mailbox clear"
        );
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

    /// ⭐ THE LOAD PASS BILLS THE DWELLINGS (round 155, w155e) — see
    /// [`no_mc1_load_pass_area_mail`]. A `(10,31)` canyon authored
    /// across a `(10,45)` house: the load fixpoint's diggers (life 2 →
    /// 200 + 8 + 8 = 216 each) write ch0 into the state-51 house's
    /// mailbox, the construction handler never reads it, and the
    /// house's FIRST state-52 tick drains the whole bill — mc1l36 slot
    /// 68 (4536 = 21 × 216, dead at settle 2, `+38` = the canyon
    /// head). Law off, the house stands at 2000 with an empty box.
    #[test]
    fn the_load_pass_canyon_bills_the_state_51_dwelling() {
        let mut g = Gen::new(
            flat_land(100),
            synthetic_assets(),
            7,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let mut a = thing(0, 10, 31, 128, 116);
        a.swi_id = 1;
        a.child = 2; // table index 2 = thing slot 1 (MC1 table base 1)
        let mut b = thing(1, 10, 31, 128, 140);
        b.swi_id = 1;
        b.parent = 1;
        let house = thing(2, 10, 45, 128, 128); // parent 0 → build type 16
        let mut table = build_table(&[a, b, house], ChassisParams::MC1.level_table_slots, 1);
        g.load_time_pass(&mut table);
        let h = (1..g.ent.len())
            .find(|&i| g.ent[i].class64 == 10 && g.ent[i].model65 == 45)
            .expect("the house survives the load pass");
        assert_eq!(g.ent[h].tick70, 52, "constructed");
        assert_eq!(g.ent[h].act_life, 2000);
        let (amt, src) = g.ent[h].mail[0];
        if no_mc1_load_pass_area_mail() {
            assert_eq!((amt, src), (0, 0), "switch on: the pre-dig empty box");
            return;
        }
        assert!(src != 0, "the canyon digger's id rides +94");
        assert!(amt >= 216 && amt % 216 == 0, "k × (200 + 8 + 8), got {amt}");
        // The first live tick drains it: lethal → state 53, +38 = src.
        g.tick_building_live(h, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[h].act_life, 2000 - amt as i32);
        assert_eq!((g.ent[h].tick70, g.ent[h].f38), (53, src));
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

    /// sub_37920 (:44251/:44256): ONE sub_11F50 on the RAW pre-snap
    /// axis feeds both the link z and the +154 site datum; the
    /// caller's z is ignored. The transform's painter mints AT the
    /// +150 triple (sub_47020 :56100), so a zero +154 is a painter
    /// born at z 0 — mc1l5 t=17645, Vodor's post-raze rebuild at
    /// scouted site (0,0). Pair-invisible (imports carry the triple);
    /// pinned here.
    #[test]
    fn the_castle_ctor_grounds_the_raw_axis_into_the_site_datum() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let (x, y) = (0x8140u16, 0x8140u16);
        let i = g.spawn_class3(2, x, y, -12345).unwrap();
        let gz = g.ground_z(x, y) as i16;
        assert_eq!(
            g.ent[i].site_z, gz,
            "+154 = the ground at the raw landing point"
        );
        assert_eq!(g.ent[i].z, gz, "the link z is the same sample");
        assert_eq!(
            (g.ent[i].dest_x, g.ent[i].dest_y),
            (0x8100, 0x8100),
            "+150/152 keep the parity-snapped anchor"
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
            e.tick70 = 5; // the ctor's TRANSFORM state…
            e.f59 = 0; // …at sub-state 0
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

    /// THE CASTLE WORKERS FOLLOW THEIR `+42` LINK, NOT THE SITE SCAN
    /// (round 154, w154j). `sub_47020`/`sub_47080`/the upgrade commit
    /// stamp `+42 = the castle's own slot` (CARPET.EXE 0x4705F /
    /// 0x470BF / :56486) and the workers dereference it (`sub_285C0`
    /// :30520/:30697-709, `sub_28200` :30333/:30419-27). Two castle
    /// records on ONE site — a TRANSFORM-parked twin at the lower slot
    /// (round 151's stale-recycle class) under the live castle at the
    /// higher — split the two resolves: the scan serves the lower
    /// record, the link the one that minted the worker. Fails under
    /// `MGC_NO_MC1_WORKER_CASTLE_LINK=1` (the scan), at the finish
    /// asserts.
    #[test]
    fn the_castle_workers_follow_their_link_over_the_site_scan() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let castle_at = |g: &mut Gen, sub: u8| {
            let i = g.new_event().unwrap();
            let e = &mut g.ent[i];
            e.class64 = 3;
            e.model65 = 2;
            e.x = 0x8000;
            e.y = 0x8000;
            e.f26 = 0;
            e.tick70 = 5;
            e.f59 = sub;
            i
        };
        let parked = castle_at(&mut g, 1); // the lower twin, parked in a pure wait
        let live = castle_at(&mut g, 0); // the castle that mints the workers
        assert!(live > parked, "fixture: the live castle is the HIGHER slot");
        assert_eq!(
            g.castle_at_site(0x8000, 0x8000),
            Some(parked),
            "fixture: the site scan serves the lower twin"
        );
        // Case 0: the upgrade commit mints the painter with the link.
        g.castle_tick(live, crate::patches::WorldPatches::RETAIL);
        let p = (1..g.ent.len())
            .find(|&k| {
                g.ent[k].class64 == 10 && g.ent[k].model65 == 42 && g.ent[k].flags & 0x400 == 0
            })
            .expect("the commit painter");
        assert_eq!(
            g.ent[p].link42,
            Link42(live as u16),
            "+42 = the minting castle's slot"
        );
        assert_eq!(g.ent[p].id24, g.ent[live].id24, "+24 = the castle's owner");
        for _ in 0..80 {
            if g.ent[p].flags & 0x400 != 0 {
                break;
            }
            g.tick_castle_painter(p);
        }
        assert!(g.ent[p].flags & 0x400 != 0, "fixture: the painter finished");
        assert_eq!(
            g.ent[live].f59, 5,
            "the finish hands THE LINKED castle to sub-state 5"
        );
        assert_eq!(g.ent[parked].f59, 1, "the lower twin is untouched");
        // Case 5: the leveler carries the link too, and its finish
        // writes the linked castle's sub-state and site z.
        g.castle_tick(live, crate::patches::WorldPatches::RETAIL);
        let l = (1..g.ent.len())
            .find(|&k| {
                g.ent[k].class64 == 10 && g.ent[k].model65 == 41 && g.ent[k].flags & 0x400 == 0
            })
            .expect("the leveler");
        assert_eq!(g.ent[l].link42, Link42(live as u16), "+42 on the leveler");
        g.ent[parked].site_z = 12345;
        for _ in 0..80 {
            if g.ent[l].flags & 0x400 != 0 {
                break;
            }
            g.tick_castle_leveler(l);
        }
        assert!(g.ent[l].flags & 0x400 != 0, "fixture: the leveler finished");
        assert_eq!(
            g.ent[live].f59, 2,
            "the leveler's finish lands on the linked castle"
        );
        assert_eq!(
            g.ent[parked].site_z, 12345,
            "the twin's site z is not rewritten"
        );
        // Case 3: the repaint painter is stamped the same way.
        g.ent[live].f59 = 3;
        g.castle_tick(live, crate::patches::WorldPatches::RETAIL);
        let r = (1..g.ent.len())
            .find(|&k| {
                g.ent[k].class64 == 10 && g.ent[k].model65 == 42 && g.ent[k].flags & 0x400 == 0
            })
            .expect("the repaint painter");
        assert_eq!(
            g.ent[r].link42,
            Link42(live as u16),
            "+42 on the repaint painter"
        );
        // An UNLINKED worker (a pre-field save) still resolves by site.
        g.ent[r].link42 = Link42(0);
        assert_eq!(
            g.castle_of_worker(r),
            Some(parked),
            "0 = fall back to the scan"
        );
    }

    /// **A SHAKING CASTLE ABORTS ITS GROUND LEVELER OUTRIGHT.**
    /// `sub_28200` (:30333) opens its work body on
    /// `!castle[+42]->+50 && +26`, and the ELSE arm is the FINISH —
    /// so a blast that arms the owner castle's damage-response
    /// countdown (`+50 = 30`, `sub_127E0` :17523) does not pause the
    /// translation, it ENDS it: the terrain keeps the height the
    /// translation had reached, the castle is handed straight to
    /// sub-state 2 and the worker despawns with its counter intact.
    /// The m42 painter's twin guard (:30520) is only a per-tick
    /// suspend — the asymmetry is retail's, not a transcription slip.
    /// Witness: mc1l49 t=14822, leveler slot 929 (`+26` 10, `+42`
    /// 749), castle 749 `+50 = 30` — retail's `flags` 1026, the
    /// port's 2.
    #[test]
    fn a_shaking_castle_aborts_its_ground_leveler() {
        // (leveler f26, leveler flags, castle f59) after ONE work tick.
        let run = |shake: i16| -> (i16, u32, u8) {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                VerbSet::MC1,
            );
            let (x, y) = (0x8000u16, 0x8000u16);
            let c = g.new_event().unwrap();
            {
                let e = &mut g.ent[c];
                e.class64 = 3;
                e.model65 = 2;
                e.x = x;
                e.y = y;
                e.tick70 = 5;
                e.f59 = 6; // waiting on the leveler
            }
            // A leveler whose current rung sits well above the ground
            // so the step is unmistakably nonzero.
            let l = g.spawn_creator(41, x, y, 32 * 40).unwrap();
            g.ent[l].f71 = 1;
            g.tick(l, None); // the init arm: +26 = 10, +48 = z>>5
            assert_eq!(g.ent[l].f26, 10, "init armed the counter");
            assert_eq!(g.ent[l].f28, 40, "init took the current rung");
            g.ent[c].f50 = shake;
            g.tick(l, None); // the work tick under test
            (g.ent[l].f26, g.ent[l].flags, g.ent[c].f59)
        };

        let (f26, flags, sub) = run(0);
        assert_eq!(f26, 9, "a quiet castle: the leveler steps");
        assert_eq!(flags & 0x400, 0, "…and lives");
        assert_eq!(sub, 6, "…and the castle stays in its wait state");

        let (f26, flags, sub) = run(30);
        assert_eq!(f26, 10, "a shaking castle: the counter never steps");
        assert_eq!(flags & 0x400, 0x400, "…the leveler FINISHES and dies");
        assert_eq!(sub, 2, "…handing the castle to sub-state 2");
    }

    /// ⭐⭐⭐ **A STARVED POOL MAKES THE MC1 CASTLE LEVELER DO
    /// NOTHING AT ALL.** `sub_470E0` (:56142-56) opens the whole
    /// action-6 teardown on `sub_37710_37AD0()`, which is
    /// `return *(_DWORD *)(base + 40) + 1;` (:44061-64) — the
    /// FREE-STACK DEPTH, not a spawn. So on an exhausted pool retail
    /// runs NO teardown: no rung decrement, no ladder reset
    /// (`sub_47C60_47FA0`), no ejector — it only parks `+70` back to
    /// 4, and the settled handler's still-fatal life parks it
    /// straight back to 6. A besieged castle in a starved world
    /// therefore STANDS at its old level with its NEGATIVE
    /// `act_life` intact, absorbing more overkill every hit.
    /// Witness: mc1l48 slot 916 — t=11610 the fatal hit lands
    /// (`act_life −2000`, `+70 6`), and at t=11612 with free depth
    /// **0** the castle is still `f26 2 / max_life 40000 /
    /// act_life −2000`. The ungated port tore it down in the very
    /// same pair (`f26 1`, max 20000, life 20000 − 2000 = 18000).
    /// Kill switch: `MGC_NO_MC1_LEVELER_POOL_GATE=1`.
    #[test]
    fn a_starved_pool_makes_the_castle_leveler_do_nothing_at_all() {
        // (f26, max_life, act_life, f136, tick70) after ONE action-6
        // dispatch on a castle the fatal hit already parked at +70=6.
        let run = |starve: bool| -> (i16, u32, i32, i32, u8) {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                VerbSet::MC1,
            );
            let (x, y) = (0x8000u16, 0x8000u16);
            let i = g.new_event().unwrap();
            let site_z = g.ground_z(x, y) as i16;
            {
                let e = &mut g.ent[i];
                e.class64 = 3;
                e.model65 = 2;
                e.x = x;
                e.y = y;
                e.z = site_z;
                e.site_z = site_z;
                e.f26 = 2; // a level-2 tower…
                e.max_life = Gen::CASTLE_HP[2];
                e.act_life = -2000; // …one tick into overkill
                e.f136 = Gen::CASTLE_CAP[2];
                e.tick70 = 6; // the LEVELER, parked by the fatal hit
            }
            if starve {
                // Free depth 0 — mc1l48's t=11612 exactly.
                while g.new_event().is_some() {}
                assert!(g.free.is_empty(), "the pool really is exhausted");
            }
            g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
            let e = &g.ent[i];
            (e.f26, e.max_life, e.act_life, e.f136, e.tick70)
        };

        // The starved arm: the leveler is a no-op but for the park.
        let (f26, max_life, act_life, cap, tick70) = run(true);
        assert_eq!(f26, 2, "a starved pool: the rung never steps down");
        assert_eq!(
            max_life,
            Gen::CASTLE_HP[2],
            "…no ladder reset of the HP row"
        );
        assert_eq!(act_life, -2000, "…the negative life stands, intact");
        assert_eq!(cap, Gen::CASTLE_CAP[2], "…and the capacity rung with it");
        assert_eq!(tick70, 4, "the ONLY effect: +70 parks back to settled");

        // The same setup with slots to spare: the teardown runs.
        let (f26, max_life, act_life, cap, tick70) = run(false);
        assert_eq!(f26, 1, "a live pool: the castle drops a rung");
        assert_eq!(max_life, Gen::CASTLE_HP[1], "…the HP ladder re-derives");
        assert_eq!(
            act_life,
            Gen::CASTLE_HP[1] as i32 - 2000,
            "…the overkill is re-deducted from the new max"
        );
        assert_eq!(cap, Gen::CASTLE_CAP[1], "…and the capacity rung follows");
        assert_eq!(tick70, 4, "the park is unconditional either way");
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
            e.tick70 = 4; // SETTLED — the shake runs from sub_46DB0
            e.f50 = 5;
            e.x = 0x8000;
            e.y = 0x8000;
        }
        for want in [4, 3, 2, 1] {
            g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
            assert_eq!(g.ent[i].f50, want, "a countdown tick only decrements");
            assert_eq!(g.ent[i].tick70, 4, "no transition above 1");
        }
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].f50, 0, "the ==1 tick zeroes without decrementing");
        assert_eq!(
            (g.ent[i].tick70, g.ent[i].f59),
            (5, 3),
            "the repaint fires only after the f50=1 boundary was seen"
        );
    }

    /// ⭐⭐⭐ **THE CASTLE PAINTER PAINTS ON THE PRE-STEP HEIGHTMAP.**
    /// Retail's kill + `sub_33800` paint live INSIDE the per-row RLE
    /// walk that fills the goal-delta buffer (:30630-46); the buffered
    /// deltas are only swept into the heightmap once every row has
    /// been walked (:30541-79). So `sub_33640`'s corner vote reads the
    /// heights the tick STARTED with, never the ones it is about to
    /// write.
    ///
    /// The port used to apply first and paint after, which flips the
    /// vote on any cell whose quad is being raised unevenly — and
    /// PAINT_AC's two halves are not cosmetic: index 0..3 paints type
    /// 27, index 4..7 paints type 26, and `cap_bit` maps 27 to
    /// 0x80000 (creature-BLOCKING) against 26's passable 0x400000. A
    /// mis-voted corner therefore builds an invisible wall: mc1l48's
    /// mound at (26,138) sat on a port-only type 27 for 251 ticks and
    /// then died walled-in at t=935.
    ///
    /// Here the quad starts 10/14/14/10 (vote 5 -> type 26) and the
    /// tick raises its NE corner to 30, which would vote 1 -> type 27.
    /// Painting first is what keeps it 26.
    #[test]
    fn the_castle_painter_paints_before_the_height_step() {
        // One 3x3 build row of PAINT_AC cells (hi nibble 5 => a4 16).
        // Low nibble n carries the goal `4*(n-1) + target`, so with
        // target 10 the byte picks the cell's goal height: 0x51 -> 10
        // (the standing height, no delta), 0x52 -> 14, 0x56 -> 30.
        let dat: Vec<u8> = vec![
            3, 0x51, 0x56, 0x51, 0, //
            3, 0x51, 0x52, 0x51, 0, //
            3, 0x51, 0x51, 0x51, 0, //
        ];
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|_| {
                let mut e = 0u32.to_le_bytes().to_vec();
                e.push(3);
                e.push(3);
                e
            })
            .collect();
        let mut grid = vec![31u8; 1024];
        grid[15 * 32 + 15] = 0;
        let assets = FeatureAssets::parse(&grid, &tab, &dat).unwrap();
        let mut g = Gen::new(flat_land(10), assets, 1, ChassisParams::MC1, VerbSet::MC1);
        // The NE and SE corners of the checked cell's quad start high;
        // only the NE one is raised by this tick.
        g.t.height[tile(16, 15)] = 14;
        g.t.height[tile(16, 16)] = 14;
        let i = g.new_event().unwrap();
        {
            let e = &mut g.ent[i];
            e.class64 = 10;
            e.model65 = 42;
            e.flags = 2; // init already run; NO 0x10000 kill bit
            e.f26 = 2; // post-decrement 1 => divisor 1, paint tick
            e.f71 = 1; // level 1
            e.x = 16 * 256;
            e.y = 16 * 256;
            e.z = 10 * 32; // target = z >> 5
        }
        g.tick_castle_painter(i);
        assert_eq!(
            g.t.height[tile(16, 15)],
            30,
            "the apply pass still ran: the NE corner reached its goal"
        );
        assert_eq!(
            (g.t.tile_type[tile(15, 15)], g.t.angle[tile(15, 15)]),
            (0x1a, 0xd5),
            "PAINT_AC[5] — the PRE-step quad 10/14/14/10 vote. \
             Applying the heights first votes 1 and paints the \
             creature-blocking type 27 (0x1b) instead."
        );
    }

    /// ⭐ **AN OUT-OF-LADDER BUILD ROW PAINTS NOTHING — `+71` IS
    /// SIGNED.** `sub_24DA0`'s painter body loads the row with
    /// `movsbl 0x47(%ebp)` (CARPET.EXE file 0x40E62 / 0x40E82 /
    /// 0x40EAF / 0x40ECF, VA 0x2866A / 0x2868A / 0x286B7 / 0x286D7)
    /// and its per-row goal fill continues under a SIGNED compare
    /// (`movsbl 0x47(%ebp),%edx / mov 0x50(%esp),%al / cmp %edx,%eax
    /// / jle`, file 0x41203 = VA 0x28A0B; remc1 sub_main.cpp:30538-40
    /// `if (i > *(char *)(a1 + 71)) break;`). Row 250 is therefore
    /// −6: the loop runs zero times, the `memset`-zero delta buffer
    /// is never filled and the apply sweep writes NO height. The port
    /// clamped `.min(8)` and painted a level-8 castle every work tick
    /// — the whole `field:z` family of the mc1l45-froze /
    /// mc1l26-froze level-250 tails. See
    /// [`mc1_no_castle_row_signed`].
    #[test]
    fn an_out_of_ladder_build_row_paints_nothing() {
        let dat: Vec<u8> = vec![
            3, 0x51, 0x56, 0x51, 0, //
            3, 0x51, 0x52, 0x51, 0, //
            3, 0x51, 0x51, 0x51, 0, //
        ];
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|_| {
                let mut e = 0u32.to_le_bytes().to_vec();
                e.push(3);
                e.push(3);
                e
            })
            .collect();
        let mut grid = vec![31u8; 1024];
        grid[15 * 32 + 15] = 0;
        let assets = FeatureAssets::parse(&grid, &tab, &dat).unwrap();

        let paint = |row: u8| {
            let mut g = Gen::new(
                flat_land(10),
                assets.clone(),
                1,
                ChassisParams::MC1,
                VerbSet::MC1,
            );
            g.t.height[tile(16, 15)] = 14;
            g.t.height[tile(16, 16)] = 14;
            let before = g.t.height.clone();
            let i = g.new_event().unwrap();
            {
                let e = &mut g.ent[i];
                e.class64 = 10;
                e.model65 = 42;
                e.flags = 2; // init already run; NO 0x10000 kill bit
                e.f26 = 2; // post-decrement 1 => divisor 1, paint tick
                e.f71 = row;
                e.x = 16 * 256;
                e.y = 16 * 256;
                e.z = 10 * 32;
            }
            g.tick_castle_painter(i);
            (before, g)
        };

        // POSITIVE CONTROL — row 1 is in the ladder and still paints.
        let (before, g) = paint(1);
        assert_ne!(
            g.t.height, before,
            "positive control: an in-ladder row still sculpts"
        );

        // Row 250 = (signed char) -6 — retail's loop runs zero times.
        let (before, g) = paint(250);
        assert_eq!(
            g.t.height, before,
            "row 250 is NEGATIVE as a signed byte: no goal row is \
             walked, so the painter writes no height at all"
        );
        assert_eq!(
            g.t.height[tile(16, 15)],
            14,
            "the NE corner is untouched (the .min(8) clamp raised it \
             to 30 — the level-8 footprint the port used to paint)"
        );
    }

    /// ⚖ **mc1l6's FOUR HEADS ARE ONE RETAIL-SIDE WOUND IN BUILD ROW
    /// 5 — THIS PINS THE PORT'S HALF OF THAT RULING.** Retail's
    /// in-memory BUILD0-0.DAT row 5 (the 35x35 level-5 castle course)
    /// is damaged during play at five cells — dat 2736/2737/2738/2739
    /// /2750, i.e. row-5 RLE row 10, cells 18/19/20/21/32 — so its
    /// `(10,42)` painter raises, and its demolish walker later razes,
    /// five cells the shipped table does not name. Both mc1l6 castle
    /// sites show it ((12,24) t=11733 and t=27748, (3,227) t=28252),
    /// levels 1..4 are bit-exact at both, and the collapse's draw
    /// count — graded against retail's own LCG on pool slot 0's `+4`
    /// — is exact for courses 4/3/2/1 and short by 4 and 5 for course
    /// 5. `conformance/known-deviations.json`
    /// `mc1l6-build-row5-dat-damage-ground-reads` carries the full
    /// evidence; this test is the ASSET + CODE half that the ruling
    /// rests on, and it must FAIL if a re-bake or a painter change
    /// ever makes the shipped table produce retail's heights.
    ///
    /// ⭐ It also pins why `fill_castle_goal_row` is faithful WITHOUT
    /// the `+4`/`+5` axis transposition that sub_285C0's inner-row
    /// walk really has (:30594-97 takes the row's X extent from `+5`
    /// and its Y extent AND row count from `+4`, the opposite of the
    /// level rect's): **every castle course is SQUARE**, so the
    /// transposition is vacuous here. It is NOT vacuous for the
    /// non-square dwelling rows (17+), which is a live lead — if that
    /// lead ever lands, the castle courses must not move, and this
    /// assertion says so.
    #[test]
    fn the_level5_castle_course_paints_only_the_shipped_bytes() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../baked/assets/mc1-temperate");
        let (Ok(tab), Ok(dat)) = (
            std::fs::read(root.join("build.tab.bin")),
            std::fs::read(root.join("build.dat.bin")),
        ) else {
            return; // baked game data is optional
        };
        let mut grid = vec![31u8; 1024];
        grid[15 * 32 + 15] = 0;
        let assets = FeatureAssets::parse(&grid, &tab, &dat).unwrap();

        // The RLE cell grid of one build row, `None` where a negative
        // run skips the cell (the same walk the painter runs).
        let cells = |bt: usize| -> Vec<Option<u8>> {
            let def = assets.build_tab[bt];
            let (w, h) = (def.w as usize, def.h as usize);
            let mut out = vec![None; w * h];
            let (mut rows, mut rx, mut ry, mut c) = (h, 0i32, 0usize, def.offset as usize);
            while rows != 0 {
                let ctl = assets.build_dat[c] as i8;
                c += 1;
                if ctl == 0 {
                    ry += 1;
                    rows -= 1;
                    rx = 0;
                    continue;
                }
                if ctl < 0 {
                    rx -= ctl as i32;
                    continue;
                }
                for _ in 0..ctl {
                    if rx >= 0 && (rx as usize) < w && ry < h {
                        out[ry * w + rx as usize] = Some(assets.build_dat[c]);
                    }
                    c += 1;
                    rx += 1;
                }
            }
            out
        };

        // 1. EVERY CASTLE COURSE IS SQUARE — what makes sub_285C0's
        //    `+4`/`+5` inner-row transposition vacuous for the castle.
        for bt in 1..=8usize {
            let d = assets.build_tab[bt];
            assert_eq!(
                (bt, d.w),
                (bt, d.h),
                "castle course {bt} must be square, else sub_285C0's \
                 inner-row `+5`-is-X / `+4`-is-Y walk is NOT vacuous"
            );
        }
        assert_eq!(
            (assets.build_tab[5].w, assets.build_tab[5].offset),
            (35, 2347),
            "course 5 is the 35x35 rect at dat 2347"
        );

        // 2. THE FIVE DISPUTED BYTES, AS SHIPPED.
        let c5 = cells(5);
        for (rx, want) in [(18, 0x55u8), (19, 0), (20, 0), (21, 0), (32, 0)] {
            assert_eq!(
                c5[10 * 35 + rx],
                Some(want),
                "shipped course-5 cell (row 10, col {rx}) — retail \
                 behaves as if this byte were 0x5B/0x5F/0x5F/0x5F/0x54"
            );
        }

        // 3. AND THE ASSET CANNOT REACH RETAIL'S GOAL AT ALL: a goal
        //    of `target + 56` needs a cell byte with low nibble 15
        //    (`4 * (lo - 1) + target`, CARPET.EXE 0x288E7-0x2891F),
        //    and courses 1..5 hold NO such byte anywhere. Retail
        //    applied exactly `target + 56` at three cells, in three
        //    separate builds, at two sites.
        for bt in 1..=5usize {
            assert!(
                !cells(bt)
                    .iter()
                    .any(|b| b.is_some_and(|b| b >= 0x0F && b % 16 == 15)),
                "course {bt} must hold no low-nibble-15 cell byte"
            );
        }

        // 4. THE PAINTER ITSELF, over mc1l6's own site and course:
        //    18 work ticks converge every cell on the SHIPPED goal.
        let mut g = Gen::new(flat_land(60), assets, 1, ChassisParams::MC1, VerbSet::MC1);
        let i = g.new_event().unwrap();
        {
            let e = &mut g.ent[i];
            e.class64 = 10;
            e.model65 = 42;
            e.f71 = 5; // level 5 => courses 1..=5
            e.x = 12 * 256; // mc1l6's castle site (12,24)
            e.y = 24 * 256;
            e.z = 55 * 32; // target = z >> 5 = 55
        }
        for _ in 0..18 {
            g.tick_castle_painter(i);
        }
        // Course-5 rect origin = (12,24) - (35>>1) = (251,7), so RLE
        // row 10 is map row 17 and cols 18/19/20/21/32 are map x
        // 13/14/15/16/27 — mc1l6's five drifting cells verbatim.
        let got: Vec<u8> = [13u8, 14, 15, 16, 27]
            .iter()
            .map(|&x| g.t.height[tile(x, 17)])
            .collect();
        assert_eq!(
            got,
            vec![71u8, 79, 79, 87, 55],
            "the SHIPPED goals: 0x55 -> target+16 at x=13, course 3's \
             0x57/0x57/0x59 -> target+24/+24/+32 at x=14/15/16, and \
             course 4's 0x07 -> target at x=27"
        );
        assert_ne!(
            got,
            vec![95u8, 111, 111, 111, 67],
            "retail's mc1l6 heights are NOT reachable from the shipped \
             table — if this ever passes, the deviation ruling is stale"
        );
        // …and the cells either side of the lesion agree with retail
        // on the same tick, which is why the wound reads as five cells
        // and not as a misplaced rect.
        assert_eq!(
            (g.t.height[tile(9, 17)], g.t.height[tile(17, 17)]),
            (79, 87),
            "course-5 row 10 cols 14 and 22 — the lesion's neighbours"
        );
    }

    /// ⭐ **A DWELLING WEARS THE CASTLE'S Z-CENTER MARKER, SPRITE ROW
    /// 177, AND AN OCCUPANCY CAP OF AREA/4.** `sub_3B690` (:47501)
    /// ends on `sub_36FA0_37360(event, 177)` and `sub_36DF0_371B0`
    /// (:43705) hands the build row to `sub_37150_37510` (:43798),
    /// which writes ALL FOUR extent words — `+78 = 0xE000` included.
    ///
    /// All three lanes are UNGRADED, so the corpus is the authority
    /// and it is unanimous: every authored (10,45) in mc1l2 and mc1l3
    /// reads `+78 = 57344`, `+86 = 177`, and `+128` = build-row
    /// `(w * h) >> 2` — rows 25 (9x9 → 20), 26 (9x11 → 24), 53 (8x8 →
    /// 16), 30 (13x15 → 48). The `>> 4` in the `sub_36DF0_371B0` lift
    /// misses every one of them by exactly 4x.
    ///
    /// The consequence is a PITCH-CONE one: `+78` is the aim lift
    /// (`sub_524C0` :52509 brackets the candidate in place), so a
    /// house minted at +78 = 0 offers its ROOF as the aim point where
    /// retail offers a point 8192 below its footing. mc1l5's free-run
    /// horizon moves 4226 → 4441 on this and mc1l3's 710 → 1858.
    #[test]
    fn a_dwelling_carries_the_z_center_marker_sprite_and_area_cap() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        // The synthetic build table is uniformly 4x4.
        let (bw, bh) = {
            let d = g.assets.build_tab[3];
            (d.w as u16, d.h as u16)
        };
        let i = g
            .spawn_creator(45, 0x8000, 0x8000, 0)
            .expect("the m45 ctor");
        assert_eq!(
            (g.ent[i].type86, g.ent[i].frames89 > 0),
            (177, true),
            "the ctor's sub_36FA0(event, 177) sprite stamp"
        );
        g.building_fixup(i, 3);
        let e = &g.ent[i];
        assert_eq!(e.f78, 0xE000, "sub_37150's z-center marker");
        assert_eq!(
            e.f128,
            ((bw * bh) >> 2) as i16,
            "the occupancy cap is the footprint area over FOUR"
        );
        assert_eq!(
            (e.f80, e.f82, e.f84),
            (
                ((bw << 8).wrapping_add(1280)) >> 1,
                ((bh << 8).wrapping_add(1280)) >> 1,
                0x4000
            ),
            "the build-row extents overwrite the sprite row's"
        );
        assert_eq!(e.type86, 177, "the fixup leaves the art alone");
        // The aim point a projectile's pitch cone sees is the FOOTING,
        // 8192 below the record z — not the roof.
        assert_eq!(e.aim_z(), e.z.wrapping_add(-8192i16), "aim lift");
    }

    /// ⭐ **THE CASTLE'S MACRO STATE LIVES IN THE JOB BYTE `+70`.**
    /// Retail gives the (3,2) castle THREE dispatch rows (:4673-75) —
    /// `+70 = 4` → sub_46DB0 (settled), `5` → sub_46F10 (the transform
    /// machine, sub-state in `+48`), `6` → sub_470E0 (the leveler) —
    /// so a settled castle reads 4 and only an ACTION reads 5. The
    /// port long fused both levels into `f59` alone, which parked
    /// every castle at `tick70 = 5` for the whole level and made the
    /// rival's own upgrade predicate (`castle.tick70 == 4`, faithfully
    /// mirroring :18428) unreachable in every free run.
    ///
    /// The lane is UNGRADED by the obs schema — the mc1l5 raw shadow
    /// carried 20,165 `(3,2) f70` rows and no fixture could hold it —
    /// so the round trip is pinned here instead.
    #[test]
    fn the_castle_macro_state_lives_in_the_job_byte() {
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
            e.id24 = 630;
            e.f26 = 1;
            e.f63 = 1; // odd → skip the ejector/fleet/absorb block
            e.act_life = 20_000;
            e.tick70 = 4; // SETTLED
            e.x = 0x8000;
            e.y = 0x8000;
        }
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].tick70, 4, "an idle settled castle stays settled");

        // The ch5 upgrade intake arms the request bit, and the settled
        // tick launches the transform (:56007-11 `+48 = 0`, `+70 = 5`).
        g.ent[i].mail[5] = (10, 630);
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(
            (g.ent[i].tick70, g.ent[i].f59),
            (5, 0),
            "the upgrade request hands the castle to the transform machine"
        );

        // Case 0 commits and waits; the sub-state moves, the macro
        // state does NOT (sub_47960 :56469-70 writes both as 5/4).
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(
            (g.ent[i].tick70, g.ent[i].f59, g.ent[i].f26),
            (5, 1, 2),
            "the commit levels up and parks in the painter wait"
        );

        // Case 2 is the ONLY handback to settled (:56078-81).
        g.ent[i].f59 = 2;
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(
            (g.ent[i].tick70, g.ent[i].f59),
            (4, 0),
            "the leveler's finish returns the castle to +70 = 4"
        );

        // A lethal from the settled tick parks the leveler, and the
        // leveler hands +70 back to 4 on its next dispatch.
        g.ent[i].act_life = -1;
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(g.ent[i].tick70, 6, "the lethal notice parks the leveler");
        g.castle_tick(i, crate::patches::WorldPatches::RETAIL);
        assert_eq!(
            (g.ent[i].tick70, g.ent[i].f59, g.ent[i].f50),
            (4, 0, 5),
            "the leveler returns to settled with the 5-tick repaint timer"
        );
    }

    /// The ch0 broadcast's CASTLE PRE-PASS is the third walker of
    /// bucket[0] (:17322-33), and it inherits the roster's law: a
    /// castle already dead at the tick top takes NO area damage, and
    /// one that dies mid-tick keeps taking it until the next rebuild.
    /// The port's pool sweep had no life test at all and delivered to
    /// both — 45 `(3,2) mail0.amt` raw-shadow rows inside mc1l4's
    /// bit-exact window (t=1312 slot 436: retail 500, port 1050 — one
    /// extra broadcast accumulated into a box retail left for the
    /// spreader alone), and the castle that reads it dies 150 early.
    ///
    /// The pair channel cannot see this: the importer restores both
    /// the mailbox and the roster every tick, so `verify-deltas` is
    /// CLEAN at the pre-fix rig. Pinned here instead (the free run
    /// carries it: mc1l4's horizon 1356 → 2722 boundaries).
    #[test]
    fn the_ch0_castle_pass_walks_bucket_zero() {
        let mut g = mob_gen();
        // A ground fire (10,0) sitting on top of a castle.
        let castle = g.spawn_castle(0x4000, 0x4000).unwrap();
        g.ent[castle].id24 = 7; // someone else's, so the +24 gate opens
        let (cx, cy, cz) = (g.ent[castle].x, g.ent[castle].y, g.ent[castle].z);
        let fire = g.spawn_effect(0, cx, cy, cz).unwrap();
        let ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);

        // Dead at the TICK TOP → out of the roster → no ch0 at all.
        g.ent[castle].act_life = -1;
        g.rebuild_wiz_chain();
        g.ent[castle].mail[0] = (0, 0);
        g.area_write(fire, 0, 400, &ctx, false, false);
        assert_eq!(
            g.ent[castle].mail[0],
            (0, 0),
            "a castle dead at the tick top is not in bucket[0]"
        );

        // Alive at the tick top and killed MID-tick → still reachable,
        // which is the same soft-kill law the rest of the roster wears.
        g.ent[castle].act_life = 20_000;
        g.rebuild_wiz_chain();
        g.ent[castle].act_life = -1;
        g.area_write(fire, 0, 400, &ctx, false, false);
        assert_eq!(
            g.ent[castle].mail[0].0, 400,
            "the tick-top roster still delivers to it"
        );
    }

    /// ⭐ The MC1 ball-merge owner contest (`sub_277D0` :29755-73)
    /// weighs the two owners' `+136` — their mana CEILINGS — and the
    /// human's carpet is out of pool, so its ceiling has to arrive
    /// through `MobCtx`. With both sides reading 0 (the earlier
    /// approximation) every contest fell to retail's else-arm and the
    /// ABSORBED side won unconditionally. mc1l4 t=2723: ball 432's
    /// `+144` lands on the human in retail and on Vodor in the port,
    /// and the 9,500 of ceiling it carries moves with it. Free-run
    /// horizon 2722 → 5375 boundaries; the pair channel is blind
    /// (the importer restores `+144` every tick).
    #[test]
    fn the_ball_merge_contest_weighs_the_owners_mana_ceilings() {
        use crate::mc1::mobs::PLAYER_TARGET;
        // Returns (the survivor's resolved owner, the rival's tag).
        let run = |human_ceiling: u32| -> (u16, u16) {
            let mut g = mob_gen();
            let a = g.spawn_mana_ball(0x4000, 0x4000, 0).unwrap();
            let b = g.spawn_mana_ball(0x4000, 0x4000, 0).unwrap();
            // A rival carpet whose own ceiling is fixed at 5,000.
            let rival = g.spawn_class3(1, 0x2000, 0x2000, 0).unwrap();
            g.ent[rival].f136 = 5_000;
            g.ent[a].f144 = PLAYER_TARGET; // survivor: the human's
            g.ent[b].f144 = rival as u16; // absorbed: the rival's
            let ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
            g.view_mut(ctx.phx).mana_max = human_ceiling;
            g.mc1_ball_owner_contest(a, b);
            (g.ent[a].f144, rival as u16)
        };
        let (won, _) = run(9_000);
        assert_eq!(
            won, PLAYER_TARGET,
            "a strictly larger ceiling keeps the survivor's owner"
        );
        let (won, rival) = run(1_000);
        assert_eq!(
            won, rival,
            "a smaller one hands the ball to the absorbed side"
        );
        let (won, rival) = run(5_000);
        assert_eq!(won, rival, "and the tie goes the same way (<=)");
    }

    /// ⚠ **THE CLASS-10 FLASH FAMILY IS NOT UNIFORM IN ITS ANIMATION
    /// STEP.** Three of the four flashes open their live arm with
    /// `sub_42510_42850` — `sub_25760` (:28437, the possess claim),
    /// `sub_26360` (:28937, the mana steal) and `sub_263C0` (:28959,
    /// the duel tether) — and the BOLT HIT FLASH `sub_262D0` does
    /// NOT: :28908-19 is its whole live arm, the ch0 write, sound 24,
    /// the one-tick life pin and the fired flag, with no call. A
    /// 2026-07-21 audit batch that corrected the family's `+26` bump
    /// and pre-decrement life test read the anim step as uniform too
    /// and added it here; the raw shadow priced the slip at 2,092
    /// `(10,23) frame88` rows on mc1l42, `retail 0 port 1` on every
    /// hit flash in the take.
    ///
    /// `frame88` is an UNGRADED lane (the importer restores it every
    /// pair) and mc1l5's goldens are blind to it — measured, by
    /// stubbing this change out against the same tree — so the round
    /// trip is pinned here, against a sibling that must still step.
    #[test]
    fn the_bolt_hit_flash_has_no_animation_step() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        // action 23 = the bolt hit flash, action 25 = the mana-steal
        // flash: same skeleton, same ch-write shape, opposite verdict.
        let mk = |g: &mut Gen, action: u8| {
            let i = g.new_event().unwrap();
            let e = &mut g.ent[i];
            e.class64 = 10;
            e.model65 = 23;
            e.tick70 = action;
            e.act_life = 4;
            e.frames89 = 8; // room to step
            e.frame88 = 0;
            e.x = 0x8000;
            e.y = 0x8000;
            i
        };
        let hit = mk(&mut g, 23);
        let steal = mk(&mut g, 25);
        let ctx = ctx_at(&mut g, 0x8000, 0x8000, 0);

        g.effect_tick(hit, &ctx);
        g.effect_tick(steal, &ctx);
        assert_eq!(
            g.ent[hit].frame88, 0,
            "sub_262D0 has no sub_42510 call — the hit flash never steps"
        );
        assert_eq!(
            g.ent[steal].frame88, 1,
            "sub_26360 opens its live arm with one — the sibling still steps"
        );
        // The rest of the hit flash's live arm is untouched: `+26`
        // counts every tick (:28905) and the fired flag pins the life
        // to 1 (:28915-17).
        assert_eq!(
            (g.ent[hit].f26, g.ent[hit].flags & 2, g.ent[hit].act_life),
            (1, 2, 1),
            "the ch0 write still fires and pins the life"
        );

        // And it stays put across the whole burn, not just tick one.
        for _ in 0..4 {
            g.effect_tick(hit, &ctx);
        }
        assert_eq!(g.ent[hit].frame88, 0, "still zero at the end of the burn");
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

    /// Round 154 (w154i): the castle-side house reads resolve THE
    /// CASTLE'S OWNER and read the tick-top census snapshot
    /// (`sub_47130` :56185 / `sub_47400` :56363 — `pool[+24].+160->
    /// +308`). The ejector's house term is inert for a non-negative
    /// tally (spill = stored − cap needs stored > cap), so the arm
    /// that can be witnessed is the fleet's "castle full" test: the
    /// SNAPSHOT decides, not the human's tally and not a live sum.
    /// `MGC_NO_MC1_OWNER_HOUSE_TALLY=1` fails this test at the resolve
    /// (every owner reads the human's 9000) and, past that, at "the
    /// snapshot says full" (the pre-dig dispatcher summed the pool
    /// live) — the kill-switch positive control.
    #[test]
    fn the_castle_reads_its_own_owners_house_tally() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        let rival = 630u16;
        g.rival_ents[2] = rival;
        // The resolve: human → `banked_houses`, rival → its row, a tag
        // that names no wizard → 0.
        g.banked_houses[Hx::FIRST] = 9000;
        g.rival_banked_houses[2] = 700;
        assert_eq!(g.owner_houses(crate::mc1::mobs::PLAYER_TARGET), 9000);
        assert_eq!(g.owner_houses(rival), 700);
        assert_eq!(g.owner_houses(5), 0);

        // A rival castle at 5000/10000 with ONE balloon and a claimed
        // ball to hunt.
        let c = g.new_event().unwrap();
        {
            let e = &mut g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.id24 = rival;
            e.f26 = 2; // level 2: quota (1, 0)
            e.f136 = 10_000;
            e.f140 = 5_000;
            e.f63 = 2; // the stagger turn (2 % 1 == 0)
            e.x = 0x4000;
            e.y = 0x4000;
        }
        let ball = g.new_event().unwrap();
        {
            let e = &mut g.ent[ball];
            e.class64 = 10;
            e.model65 = 39;
            e.x = 0x4000 + 400;
            e.y = 0x4000;
            e.f144 = rival;
            e.f140 = 100;
        }
        let b = g.spawn_balloon(0x4000, 0x4000, 0, rival).unwrap();
        g.mc1_balloon_reg.0.insert(rival, vec![b as u16]);

        // The HUMAN's tally is over the cap, the rival's is not: the
        // rival's balloon hunts the ball (retail: 0 + 5000 < 10000).
        g.banked_houses[Hx::FIRST] = 9000;
        g.rival_banked_houses[2] = 0;
        g.rebuild_ball_chain();
        g.castle_balloons(c);
        assert_eq!(
            g.ent[b].f146, ball as u16,
            "the human's houses must not recall a rival's balloon"
        );

        // The rival's SNAPSHOT says full while the pool holds no
        // (10,45) of his at all: the balloon homes — the read is the
        // census snapshot, not a live sum.
        g.banked_houses[Hx::FIRST] = 0;
        g.rival_banked_houses[2] = 6000;
        g.ent[b].f146 = 0;
        g.rebuild_ball_chain();
        g.castle_balloons(c);
        assert_eq!(g.ent[b].f146, c as u16, "the snapshot says full: home");

        // And the converse: live houses worth 6000 in the pool, a
        // snapshot of 0 — the balloon hunts (the census has not seen
        // them yet).
        let h = g.new_event().unwrap();
        {
            let e = &mut g.ent[h];
            e.class64 = 10;
            e.model65 = 45;
            e.x = 0x6000;
            e.y = 0x6000;
            e.f144 = rival;
            e.f140 = 6000;
        }
        g.rival_banked_houses[2] = 0;
        g.ent[b].f146 = 0;
        g.rebuild_ball_chain();
        g.castle_balloons(c);
        assert_eq!(g.ent[b].f146, ball as u16, "a live sum is not the law");

        // The ejector on the same castle: the rival's own tally under
        // cap, the human's over — no spill; and vice versa — no spill
        // either, because the house term never decides for a
        // non-negative tally (stored 5000 <= cap 10000).
        let balls_before = (1..g.ent.len())
            .filter(|&j| g.ent[j].class64 == 10 && g.ent[j].model65 == 39)
            .count();
        g.banked_houses[Hx::FIRST] = 9000;
        g.rival_banked_houses[2] = 0;
        g.castle_eject(c);
        g.banked_houses[Hx::FIRST] = 0;
        g.rival_banked_houses[2] = 9000;
        g.castle_eject(c);
        let balls_after = (1..g.ent.len())
            .filter(|&j| g.ent[j].class64 == 10 && g.ent[j].model65 == 39)
            .count();
        assert_eq!(
            balls_before, balls_after,
            "no tally spills a castle under its cap"
        );
        assert_eq!(g.ent[c].f140, 5_000);
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

    /// ⭐ **THE `(10,43)` CASTLE UPGRADE TOKEN LOST TWO LINES — ONE IN
    /// ITS CTOR, ONE IN ITS TICK — AND BOTH COLUMNS SPELL THEM.**
    ///
    /// CTOR: MC2's `sub_502B0` (EF:36748-64) ends
    /// `SetEntityIndexAndRot_49CD0(event, 41)` +
    /// `SetEntityShiftRot_49EA0(event, 512, 512)`, and the helper is
    /// THREE lines — `array.pitch = shift; array.roll = shift;
    /// array.fov = fov` (EF:32869-74). MC1's `sub_3B6F0` closes with
    /// the identical `sub_37130_374F0(v2, 512, 512)` (remc1
    /// sub_main.cpp:47538) over the identical three-line body
    /// (:43790-94). The port transcribed only the shift pair, so
    /// `afov` kept the sprite row's value.
    ///
    /// TICK: MC2's `sub_389F0` (EF:28265-97) and MC1's `sub_293D0`
    /// (remc1 sub_main.cpp:31009-40) both open the `life >= 0` arm
    /// with the animation step (`sub_585A0` / `sub_42510_42850`)
    /// BEFORE the `byte[0] & 2` one-shot latch. The port fused the two
    /// tests into one `&&` and dropped the call, so the token died on
    /// frame 0.
    ///
    /// WITNESSES (round-148 free-run raw shadow, ALL 40 MC2 takes):
    /// `(10,43) afov` 1,209 rows retail **512** / port 125, and
    /// `(10,43) b5c` 761 rows retail **1** / port 0.
    ///
    /// NON-VACUITY: `MGC_NO_UPGRADE_TOKEN_FOV=1` fails the first
    /// assertion, `MGC_NO_UPGRADE_TOKEN_ANIM=1` the last. The
    /// POSITIVE CONTROL is the shift pair, which the port already had
    /// and which must stay 512 under either switch.
    #[test]
    fn the_castle_upgrade_token_takes_all_three_shift_rot_lines_and_steps_its_frame() {
        let mut g = mc2_gen();
        let i = g
            .spawn_creator(43, 40u16 << 8, 40u16 << 8, 0)
            .expect("the (10,43) token");
        assert_eq!(g.ent[i].f84, 512, "SetEntityShiftRot's THIRD line");
        // POSITIVE CONTROL — the two lines the port already had.
        assert_eq!((g.ent[i].f80, g.ent[i].f82), (512, 512));
        assert_eq!(g.ent[i].tick70, 45, "action 45, the delivery receipt");
        assert_eq!(g.ent[i].frame88, 0, "the ctor leaves frame 0");
        assert!(g.ent[i].frames89 >= 1, "sprite row 41 has frames to step");
        // One armed tick: the step runs before the latch, and the
        // token is freed the same tick either way.
        g.tick_upgrade_token(i);
        assert_eq!(g.ent[i].frame88, 1, "sub_585A0 / sub_42510 stepped it");
    }

    /// ⭐⭐⭐ RETAIL TAKES THE LIGHTNING BEAM OFF THE TILE MAP FOR ITS
    /// WHOLE MARCH AND NEVER PUTS IT BACK.
    ///
    /// `sub_66750` calls the bare unlink `SetMapEntity_57E50`
    /// (EF:58305; Events.cpp:5194-5206) one statement before its first
    /// `sub_66610`, and `sub_66610`'s own step is a RAW field write
    /// (`a1x->position_0x4C_76 = predictedAxis_EB398ar;`, EF:63601) —
    /// never `CopyEntityPosition_57CF0`/`sub_57D40`. The port marched a
    /// LINKED record through `move_relink`, head-inserting the beam
    /// into every tile it crossed and leaving it there, solid, for the
    /// rest of the frame. See [`Gen::move_relink`].
    ///
    /// NON-VACUITY: `MGC_NO_BEAM_UNLINK=1` restores the relink and both
    /// assertions below fail.
    #[test]
    fn a_marching_lightning_beam_is_off_the_tile_map() {
        let mut g = mc2_gen();
        let a = g.new_event().expect("a slot");
        let beam = g.new_event().expect("beam slot");
        let (x0, y0) = (10u16 << 8, 10u16 << 8);
        g.link(a, x0, y0, 0);
        g.link(beam, x0, y0, 0);
        assert_eq!(
            g.map_entity[tile(10, 10)] as usize,
            beam,
            "head insertion puts the beam at the head of its tile"
        );

        // ARMED = the march window.
        g.mc2_beam_defer.armed = true;
        // A step WITHIN its own tile, then one ACROSS a boundary.
        g.move_relink(beam, x0 + 0x40, y0, 0);
        g.move_relink(beam, 13u16 << 8, 13u16 << 8, 0);
        g.mc2_beam_defer.armed = false;

        assert_eq!(
            g.ent[beam].flags & 4,
            0,
            "the bare unlink clears byte[0] & 4 and never re-sets it"
        );
        for t in 0..g.map_entity.len() {
            assert_ne!(
                g.map_entity[t] as usize, beam,
                "a marching beam must head NO tile chain (tile {t})"
            );
        }
        assert_eq!(
            g.map_entity[tile(10, 10)] as usize,
            a,
            "and the tile it left must fall back to the record beneath it"
        );
        assert_eq!(
            (g.ent[beam].x, g.ent[beam].y),
            (13u16 << 8, 13u16 << 8),
            "the raw field write still moves it"
        );
    }

    /// `sub_5FD00`'s dry arm (`NETHERW.EXE` 0x5FD80-0x5FD98): the
    /// castle ejector's mid-tick GC REAPS EVERY PENDING GHOST, rebuilds
    /// the free stack DESCENDING (so the next pop is the LOWEST free
    /// slot, not the incremental stack's order) and DROPS the victim
    /// stack the same rebuild just re-ranked. See [`Gen::mc2_eject_gc`].
    #[test]
    fn the_dry_ejector_s_gc_reaps_ghosts_and_drops_the_victim_stack() {
        let mut g = mc2_gen();
        // Three live records; two of them flagged disabled (byte[1]&4
        // → 0x400) — retail's `sub_49F90` first loop `sub_57F20`s
        // exactly those.
        let a = g.new_event().unwrap();
        let b = g.new_event().unwrap();
        let c = g.new_event().unwrap();
        for &s in &[a, b, c] {
            g.ent[s].class64 = 9;
            g.ent[s].model65 = 9;
        }
        g.ent[b].flags |= 0x400;
        g.ent[c].flags |= 0x400;
        // A dry free stack and an armed victim stack — the state the
        // ejector's `test ax,ax / jnz` arm keys on.
        g.free.clear();
        g.mc2_recycle.stack = vec![a as u16];
        let depth = g.mc2_eject_gc();
        assert_eq!(
            depth,
            g.free.len() as i32,
            "the arm returns the POST-GC free depth (`mov [ebp-0x4],eax`)"
        );
        assert_eq!(g.ent[b].class64, 0, "ghost b reaped by the GC's first loop");
        assert_eq!(g.ent[c].class64, 0, "ghost c reaped by the GC's first loop");
        assert_eq!(g.ent[a].class64, 9, "a live record is NOT a ghost");
        assert!(
            g.mc2_recycle.stack.is_empty(),
            "`mov dword [edx+0x11e6],-1` — the victim stack is dropped"
        );
        assert_eq!(
            g.free.last().copied(),
            Some(b.min(c) as u16),
            "descending 999→1 collect ⇒ the stack TOP is the lowest free slot"
        );
        assert!(
            !g.free.contains(&(a as u16)),
            "a live record never enters the free collect"
        );
    }

    /// The GC honours the import's PINNED human-carpet slot: a zeroed
    /// husk in our pool, a live wizard in retail's, and never
    /// allocatable — the same guard every `mc2_rebuild_free` call site
    /// already carries down from `World::mc2_carpet_slot`.
    #[test]
    fn the_dry_ejector_s_gc_never_hands_out_the_pinned_carpet_slot() {
        let mut g = mc2_gen();
        let husk = 7usize;
        g.mc2_pinned[Hx::FIRST] = Mc2Pinned(husk as u16);
        g.free.clear();
        assert_eq!(g.ent[husk].class64, 0, "the imported husk is class 0");
        let depth = g.mc2_eject_gc();
        assert!(depth > 0, "the rebuild finds the empty pool");
        assert!(
            !g.free.contains(&(husk as u16)),
            "the pinned carpet slot stays out of the free stack"
        );
    }

    fn ctx_at(
        g: &mut crate::engine::features::Gen,
        px: u16,
        py: u16,
        pz: i16,
    ) -> crate::mc1::mobs::MobCtx {
        *g.view_mut(crate::engine::world::Hx::FIRST) = crate::engine::features::SeatView {
            x: px,
            y: py,
            z: pz,
            mana: 1000,
            mana_max: 1000,
            ..Default::default()
        };
        crate::mc1::mobs::MobCtx {
            phx: crate::engine::world::Hx::FIRST,
            strict: false,
            patches: crate::patches::WorldPatches::RETAIL,
            mc2_turn: 0,
        }
    }

    /// sub_360C0's quad gate indexes the type plane with SIGNED
    /// 32-bit arithmetic (:42912-19): for a row-0 cell the two
    /// "row −1" reads land BELOW the plane in the sound-driver
    /// globals (CC0DF..CC1DF), modeled by [`Gen::OOB_TYPE_SHIM`] —
    /// so (a) a row-0 cell whose in-map NW quad is all plain still
    /// SKIPS when its shim byte is building-classed, and (b) a row-0
    /// cell whose row-255 neighbor holds a wall still SMOOTHS,
    /// because retail never reads row 255 for the gate. Measured
    /// mc1hwl0 t=21282 (castle 233's downgrade rect straddles the y
    /// wrap): the shim reproduced retail's post-collapse heights
    /// 441/441 where the wrapped gate left 20 cells drifted (the
    /// `(10,0)slot601:z` head). Reversion-probed: with the wrapped
    /// quad restored this test fails and the mc1hwl0 horizon drops
    /// 21483 → 21281.
    #[test]
    fn the_row0_smoother_gate_reads_below_the_type_plane() {
        let mut g = Gen::new(
            flat_land(100),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        // (a) shim byte 56 is building-classed: (56,0) must not
        // smooth even though every in-map quad cell is plain.
        g.t.height[tile(56, 1)] = 118;
        g.smooth_cell(tile(56, 0));
        assert_eq!(
            g.t.height[tile(56, 0)],
            100,
            "shim byte 56 gates the smooth off"
        );
        // (b) a wall at (65,255) sits in the PORT's old NW quad of
        // (66,0) but retail's gate never reads it — the cell smooths
        // over the 8 plain neighbors (wall excluded from the SUM
        // only): (8*100 + 118) / 8 = 102... the wall cell drops from
        // the sum, n = 8.
        g.t.tile_type[tile(65, 255)] = 11;
        g.t.height[tile(66, 1)] = 118;
        g.smooth_cell(tile(66, 0));
        assert_eq!(
            g.t.height[tile(66, 0)],
            102,
            "row-0 gate ignores the in-map row-255 wall"
        );
    }

    /// Round 119 (dig w2, the mc1l49 t=17478 `(5,2)slot138:z` head):
    /// four more building-classed shim bytes — {47, 55, 76, 85} — each
    /// forced by a SKIP whose lower byte an adjacent SMOOTH had already
    /// pinned plain, and each corroborated on a second take. See
    /// [`Gen::OOB_TYPE_SHIM`].
    ///
    /// A row-0 cell (x,0) reads shim `x` and `x + 1`, so byte b blocks
    /// cells (b-1,0) and (b,0) and nothing else. The test asserts both
    /// halves: the blocked cells must NOT smooth, and the neighbours
    /// that pinned the byte's partner plain must still smooth.
    #[test]
    fn the_row0_shim_gates_bytes_47_55_76_and_85() {
        // ONE WORLD PER CELL: the probe cells are three apart in x and
        // the 3x3 windows overlap, so a shared world would let one
        // probe's write feed the next one's average.
        let probe = |x: u8| {
            let mut g = Gen::new(
                flat_land(100),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                crate::verbs::VerbSet::MC1,
            );
            // Eight cells at 100 and (x,1) at 118, so a smooth writes
            // (8*100 + 118)/9 = 102 and a skip leaves 100.
            g.t.height[tile(x, 1)] = 118;
            g.smooth_cell(tile(x, 0));
            g.t.height[tile(x, 0)]
        };
        // Blocked by shim 47 / 55 / 76 / 85 (the cell BELOW each byte
        // and the cell AT it).
        for (x, byte) in [
            (46u8, 47),
            (47, 47),
            (54, 55),
            (55, 55),
            (75, 76),
            (76, 76),
            (84, 85),
            (85, 85),
        ] {
            assert_eq!(probe(x), 100, "shim byte {byte} must gate ({x},0) off");
        }
        // The smooths that FORCED those bytes: mc1l49 t=16972 (83,0),
        // mc1l37 t=9561 (44,0)/(45,0), t=15427-of-mc1l49 (53,0)/(74,0).
        // Each pins its own pair {x, x+1} plain, so each must still
        // take the 3x3 average.
        for x in [44u8, 45, 53, 74, 83] {
            assert_eq!(
                probe(x),
                102,
                "({x},0) pins its shim pair plain and must smooth"
            );
        }
    }

    /// Round 122 (dig Q5): the rest of the mc1hwl1 row-0 family that
    /// dig W3 opened with {227} — building-classed shim bytes
    /// {32, 39, 40, 48, 89}. Each is forced to a SINGLE index, never a
    /// pair, because in the very same smoother walk the neighbouring
    /// cell that shares the byte reproduces retail's 3x3 average
    /// bit-exactly: (41,0) at t=6644 forces {40}, (37,0) at t=6730 and
    /// t=7122 forces {39}, (33,0) at t=6730 forces {32}, (87,0) at
    /// t=8849 forces {89}, and (49,0) at t=9671 forces {48} — the last
    /// settling the {48, 49} ambiguity mc1l37 could not separate.
    /// See [`Gen::OOB_TYPE_SHIM`].
    ///
    /// NO RECORDING PIN IS POSSIBLE: pair mode installs the truth
    /// channel's terrain per pair, so a drifting height plane never
    /// reaches the compare, and the take's head (t=5540
    /// `(5,4)slot665:x`) is not a terrain row. The free-run witness is
    /// `MGC_PLANE_DIFF`: over mc1hwl1's whole 53,495-tick run the
    /// height plane drifts from retail on 11,985 ticks without these
    /// five bytes and on 1 with them, and the segmented census goes
    /// 27 segments / 26 devs -> 9 / 8.
    ///
    /// NON-VACUITY: `MGC_NO_MC1_ROW0_SHIM_32_39_40_48_89=1` fails this
    /// test — every blocked cell smooths to 102.
    #[test]
    fn the_row0_shim_gates_bytes_32_39_40_48_and_89() {
        // ONE WORLD PER CELL — the 3x3 windows of adjacent probes
        // overlap, so a shared world would let one probe's write feed
        // the next one's average.
        let probe = |x: u8| {
            let mut g = Gen::new(
                flat_land(100),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                crate::verbs::VerbSet::MC1,
            );
            // Eight cells at 100 and (x,1) at 118, so a smooth writes
            // (8*100 + 118)/9 = 102 and a skip leaves 100.
            g.t.height[tile(x, 1)] = 118;
            g.smooth_cell(tile(x, 0));
            g.t.height[tile(x, 0)]
        };
        // A byte b blocks exactly the two cells that read it, (b-1,0)
        // and (b,0). (39,0) is blocked twice over, by {39} and {40}.
        for (x, byte) in [
            (31u8, 32),
            (32, 32),
            (38, 39),
            (39, 39),
            (40, 40),
            (47, 48),
            (48, 48),
            (88, 89),
            (89, 89),
        ] {
            assert_eq!(probe(x), 100, "shim byte {byte} must gate ({x},0) off");
        }
        // The smooths that FORCED those five bytes, each pinning its
        // own pair {x, x+1} plain, must still take the 3x3 average —
        // this is the half that makes the fit a separation rather than
        // a guess.
        for x in [33u8, 37, 41, 49, 87] {
            assert_eq!(
                probe(x),
                102,
                "({x},0) pins its shim pair plain and must smooth"
            );
        }
        // ...and so must their partners, the upper halves of the
        // pinned pairs: {34}, {38}, {42}, {50}, {88} are all plain.
        for x in [34u8, 42, 50, 90] {
            assert_eq!(probe(x), 102, "({x},0) reads two plain shim bytes");
        }
    }

    /// Shim byte 7, the entry mc1hwl0's THIRD collapse pinned
    /// (t=27767, rival castle 860 at (0,0) knocked level 5 → 4; the
    /// un-stamp rect straddles the x wrap, x 241..=14). Retail's
    /// row-0 epilogue smooths (5,0) and (8,0)..(10,0) but leaves
    /// (6,0) and (7,0) at their pre-smoother rubble heights EXACTLY
    /// (175 and 156, against 3x3 averages of 168 and 167 — a skip,
    /// not a coincidence). (5,0) smoothing pins {5, 6} plain and
    /// (8,0) smoothing pins {8, 9} plain, so the byte both skips
    /// share is {7}. A cell (x, 0) reads shim `x` and `x + 1`, so
    /// this entry is visible ONLY at x = 6 and x = 7.
    ///
    /// Pair fixtures are blind to it: pair mode installs the truth
    /// channel's terrain per pair, so a drifting height plane never
    /// reaches the compare. It cost mc1hwl0 the `(5,15)slot596:z`
    /// head at t=27770, where the castle guard's bilinear ground
    /// sample read the two over-smoothed cells (world horizon
    /// 27,769 → 31,887 with this byte in place).
    #[test]
    fn shim_byte_7_gates_the_two_row0_cells_that_read_it() {
        let mut g = Gen::new(
            flat_land(100),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        // A raised cell inside a candidate's 3x3 block moves its
        // average to (8*100 + 118) / 9 = 102, so "smoothed" and
        // "skipped" are distinguishable by the height alone. (6,1)
        // sits in the blocks of (5,0)..(7,0), (8,1) in those of
        // (7,0)..(9,0) — between them every cell tested below has a
        // non-identity average to move to.
        g.t.height[tile(6, 1)] = 118;
        g.t.height[tile(8, 1)] = 118;
        // (7,0) reads shim {7, 8}; (6,0) reads shim {6, 7}. Byte 7 is
        // building-classed, so both decline.
        g.smooth_cell(tile(7, 0));
        assert_eq!(g.t.height[tile(7, 0)], 100, "shim byte 7 skips (7,0)");
        g.smooth_cell(tile(6, 0));
        assert_eq!(g.t.height[tile(6, 0)], 100, "shim byte 7 skips (6,0)");
        // Its neighbours on either side read {5,6} and {8,9} — both
        // plain, so they smooth. This is what pins the skip to 7
        // rather than to 6 or 8.
        g.smooth_cell(tile(8, 0));
        assert_eq!(
            g.t.height[tile(8, 0)],
            102,
            "shim bytes 8 and 9 are plain: (8,0) smooths"
        );
        g.smooth_cell(tile(5, 0));
        assert_eq!(
            g.t.height[tile(5, 0)],
            102,
            "shim bytes 5 and 6 are plain: (5,0) smooths"
        );
    }

    /// {119} is the shim entry mc1l49 t=22193 pins (VA CC156, inside
    /// the weak `dword_CC154`). A castle leveler's perimeter epilogue
    /// (`smooth_perimeter` cx=128 cy=0 half_h=half_w=10 thick=3) walks
    /// its left column strip x=115..118 across row 0 first: retail
    /// smooths (115,0) 136->134, (116,0) 130->127 and (117,0) 120->117
    /// — each the exact 3x3 average — and then leaves (118,0) at 103
    /// where the average is 975/9 = 108. (115,0)/(116,0)/(117,0)
    /// smoothing pins {115, 116, 117, 118} plain, so {119} is forced,
    /// and a cell (x, 0) reads shim `x` and `x + 1`, so the entry is
    /// visible ONLY at x = 118 and x = 119.
    ///
    /// The blast radius is both of the take's locally-rooted z heads
    /// (t=22193 slot 947 and t=22291 slot 900; its other 25 z-only
    /// heads are pair-CLEAN, i.e. inherited): with (118,0)
    /// wrongly raised to 108 the SAME pass's (117,1) averages
    /// 1057/9 = 117 instead of 1052/9 = 116 and (118,1) 874/8 = 109
    /// instead of 868/8 = 108, so the (10,6) standing fire at
    /// (118.05, 1.69) — which re-seats on `ground_z + f46` every tick,
    /// `sub_252D0` :28199 — reads z 3499 where retail reads 3491.
    /// NON-VACUITY: `MGC_NO_MC1_ROW0_SHIM_119=1` fails this test (both
    /// skips become smooths) and regresses the
    /// `a-sound-driver-byte-gates-the-row-0-smoother` fixture.
    #[test]
    fn shim_byte_119_gates_the_two_row0_cells_that_read_it() {
        let mut g = Gen::new(
            flat_land(100),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        // Same construction as `shim_byte_7_...`: one raised cell in a
        // candidate's 3x3 block moves its average to
        // (8*100 + 118) / 9 = 102, so "smoothed" and "skipped" are
        // distinguishable by the height alone. (118,1) sits in the
        // blocks of (117,0)..(119,0), (120,1) in those of
        // (119,0)..(121,0).
        g.t.height[tile(118, 1)] = 118;
        g.t.height[tile(120, 1)] = 118;
        // (118,0) reads shim {118, 119}; (119,0) reads shim {119, 120}.
        // Byte 119 is building-classed, so both decline.
        g.smooth_cell(tile(118, 0));
        assert_eq!(g.t.height[tile(118, 0)], 100, "shim byte 119 skips (118,0)");
        g.smooth_cell(tile(119, 0));
        assert_eq!(g.t.height[tile(119, 0)], 100, "shim byte 119 skips (119,0)");
        // Its neighbours on either side read {117,118} and {120,121} —
        // both plain, so they smooth. This is what pins the skip to
        // 119 rather than to 118 or 120.
        g.smooth_cell(tile(117, 0));
        assert_eq!(
            g.t.height[tile(117, 0)],
            102,
            "shim bytes 117 and 118 are plain: (117,0) smooths"
        );
        g.smooth_cell(tile(121, 0));
        assert_eq!(
            g.t.height[tile(121, 0)],
            102,
            "shim bytes 120 and 121 are plain: (121,0) smooths"
        );
    }

    /// Round 152 (dig w152g): shim bytes {16, 23, 24, 31, 225}, each
    /// forced by a same-epilogue neighbour that smooths bit-exactly in
    /// retail — see [`mc1_no_row0_shim_16_23_24_31_225`]. NON-VACUITY:
    /// `MGC_NO_MC1_ROW0_SHIM_16_23_24_31_225=1` fails every skip
    /// assertion below (they all become smooths above 100).
    #[test]
    fn shim_bytes_16_23_24_31_and_225_gate_the_row0_cells_that_read_them() {
        let mut g = Gen::new(
            flat_land(100),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        // Raised row-1 cells under every candidate: a smoothed (x,0)
        // averages above 100 (102 with one raised neighbour, 104 with
        // two), a skipped one stays at exactly 100.
        for x in [
            14u8, 15, 16, 17, 21, 22, 23, 24, 25, 26, 29, 30, 31, 32, 223, 224, 225, 226,
        ] {
            g.t.height[tile(x, 1)] = 118;
        }
        let mut probe = |x: u8| -> u8 {
            g.smooth_cell(tile(x, 0));
            g.t.height[tile(x, 0)]
        };
        // (x,0) reads shim {x, x+1}: a building byte at k skips
        // (k-1,0) and (k,0); the plain-paired cells beside it smooth.
        // ((32,0) and (226,0) are NOT witnesses: {32} and {227} are
        // building-classed from earlier rounds.)
        for (k, skips, smooths) in [
            (16u8, &[15u8, 16][..], &[14u8, 17][..]),
            (23, &[22, 23], &[21, 25]),
            (24, &[23, 24], &[21, 25]),
            (31, &[30, 31], &[29]),
            (225, &[224, 225], &[223]),
        ] {
            for &x in skips {
                assert_eq!(probe(x), 100, "shim byte {k} skips ({x},0)");
            }
            for &x in smooths {
                assert!(
                    probe(x) > 100,
                    "({x},0) smooths: its shim pair is plain (pins {k})"
                );
            }
        }
    }

    /// Round 155 (dig w155c): shim byte {109} — the low byte of
    /// `dword_CC14C`, the sound card's I/O port (0x220 ⇒ 0x20) — see
    /// [`mc1_no_row0_shim_109`]. mc1l34 t=7467: (107,0) and (110,0)
    /// smooth, (108,0) and (109,0) hold. NON-VACUITY:
    /// `MGC_NO_MC1_ROW0_SHIM_109=1` fails both skip assertions.
    #[test]
    fn shim_byte_109_gates_the_two_row0_cells_that_read_it() {
        let mut g = Gen::new(
            flat_land(100),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        for x in [106u8, 107, 108, 109, 110, 111] {
            g.t.height[tile(x, 1)] = 118;
        }
        let mut probe = |x: u8| -> u8 {
            g.smooth_cell(tile(x, 0));
            g.t.height[tile(x, 0)]
        };
        // (x,0) reads shim {x, x+1}.
        assert_eq!(probe(108), 100, "shim byte 109 skips (108,0)");
        assert_eq!(probe(109), 100, "shim byte 109 skips (109,0)");
        assert!(probe(107) > 100, "(107,0) smooths: shim {{107, 108}} plain");
        assert!(probe(110) > 100, "(110,0) smooths: shim {{110, 111}} plain");
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
            let rig_ctx = ctx_at(&mut g, 0, 0, 0);
            g.mob_awake_pass(&rig_ctx);
            g.ent[5].f58
        };
        assert_eq!(run(0x240_0000), 0, "40 tiles out stays asleep (faithful)");
        assert_eq!(run(i32::MAX), 16, "always-awake override arms f58");
    }

    /// The wake arm's `+48` stamp (round 154, w154a; citation on
    /// [`no_mc1_wake_dy48`]): the tick a creature or a mana ball wakes
    /// (`+58 = 16`), retail writes `+48 = isqrt(dy²) = |carpet.y −
    /// rec.y|` — the Y-LEG alone, because `Distance_410CE` is handed
    /// `sub_42410`'s leftover `edx = dy²`, not the planar sum. The
    /// word is never read; it lives in the hash-silent `raw48`. This
    /// pins: the y-leg (not the distance), the sign fold, the wrap
    /// through the 16-bit map seam, that a follower segment gets no
    /// stamp, that the countdown ticks leave the word alone, and that
    /// an out-of-radius record is never stamped.
    #[test]
    fn the_wake_arm_stamps_the_y_leg_to_the_carpet_in_raw48() {
        let mut g = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        // A settled ball at (2000, 3000), pooled first so the two
        // hand-seated creature records below cannot collide with it.
        let b = g.spawn_mana_ball(2000, 3000, 0).unwrap();
        g.ent[b].f58 = 0;
        // A (5,15) creature at (0, 1000) with a follower segment.
        let (c, s) = (b + 1, b + 2);
        assert!(s < g.ent.len());
        g.ent[c].class64 = 5;
        g.ent[c].model65 = 15;
        g.ent[c].act_life = 1000;
        g.ent[c].x = 0;
        g.ent[c].y = 1000;
        g.ent[c].f54 = s as u16;
        g.ent[s].class64 = 5;
        g.ent[s].model65 = 15;
        g.ent[s].act_life = 1000;
        g.ent[s].tick70 = 120;
        g.ent[s].x = 0;
        g.ent[s].y = 900;
        let rig_ctx = ctx_at(&mut g, 300, 0, 0);

        // Carpet at (300, 0): the creature's planar distance is
        // isqrt(300² + 1000²) = 1044; retail stamps |0 − 1000| = 1000.
        g.mob_awake_pass(&rig_ctx);
        assert_eq!(g.ent[c].f58, 16, "the creature woke");
        assert_eq!(g.ent[c].raw48, Raw48(1000), "the Y-leg, not the distance");
        assert_eq!(g.ent[s].f58, 18, "the follower is armed +2");
        assert_eq!(g.ent[s].raw48, Raw48(0), "a follower gets no stamp");
        assert_eq!(g.ent[b].f58, 16, "the ball woke");
        assert_eq!(g.ent[b].raw48, Raw48(3000), "the ball's Y-leg (|0 − 3000|)");

        // Sixteen countdown ticks leave the word alone; the 17th
        // re-arm re-samples it from the carpet's NEW y (the sign
        // folds: carpet above the creature reads the same magnitude).
        for _ in 0..16 {
            let rig_ctx = ctx_at(&mut g, 0, 1500, 0);
            g.mob_awake_pass(&rig_ctx);
        }
        assert_eq!(g.ent[c].f58, 0, "drained");
        assert_eq!(
            g.ent[c].raw48,
            Raw48(1000),
            "the countdown never rewrites +48"
        );
        let rig_ctx = ctx_at(&mut g, 0, 1500, 0);
        g.mob_awake_pass(&rig_ctx);
        assert_eq!(g.ent[c].f58, 16);
        assert_eq!(
            g.ent[c].raw48,
            Raw48(500),
            "re-armed: |1500 − 1000|, sign folded"
        );

        // The delta is a 16-bit wrapping subtract, as `sub_42410`'s
        // `cwtl` after the word `sub`: a creature at y = 65500 and a
        // carpet at y = 100 are 136 apart across the seam, not 65400.
        g.ent[c].y = 65500;
        g.ent[c].f58 = 0;
        let rig_ctx = ctx_at(&mut g, 0, 100, 0);
        g.mob_awake_pass(&rig_ctx);
        assert_eq!(g.ent[c].f58, 16, "136 apart through the seam is in radius");
        assert_eq!(g.ent[c].raw48, Raw48(136), "wraps through the map seam");

        // Out of radius: no arm, no stamp.
        g.ent[c].y = 0;
        g.ent[c].f58 = 0;
        g.ent[c].raw48 = Raw48(0);
        let rig_ctx = ctx_at(&mut g, 0, 7000, 0);
        g.mob_awake_pass(&rig_ctx);
        assert_eq!(g.ent[c].f58, 0, "27 tiles out stays asleep");
        assert_eq!(g.ent[c].raw48, Raw48(0), "and is never stamped");
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
        let rig_ctx = ctx_at(&mut g, 6144, 0, 0);

        // Boundary exactness: exactly 6144 units = NOT eligible.
        g.mob_awake_pass(&rig_ctx);
        assert_eq!(g.ent[b].f58, 0, "24.0 tiles exactly stays frozen");
        let rig_ctx = ctx_at(&mut g, 6143, 0, 32767);
        // One unit inside re-arms to 16 (altitude never gates).
        g.mob_awake_pass(&rig_ctx);
        assert_eq!(g.ent[b].f58, 16, "inside the radius re-arms 16");

        // Cadence: with the player parked nearby, the value returns
        // to 16 every 17 passes — 16 decrements, one zero-observe.
        let mut rearms = Vec::new();
        for t in 1..=34 {
            let rig_ctx = ctx_at(&mut g, 100, 0, 0);
            g.mob_awake_pass(&rig_ctx);
            if g.ent[b].f58 == 16 {
                rearms.push(t);
            }
        }
        assert_eq!(rearms, vec![17, 34], "exact 17-tick wake period");

        // Far away again: the countdown drains and never re-arms.
        for _ in 0..40 {
            let rig_ctx = ctx_at(&mut g, 0x7000, 0x7000, 0);
            g.mob_awake_pass(&rig_ctx);
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

    /// THE M7 VOLLEY'S ORDINAL LAW (round 154, w154j; the mc1l48
    /// t=9830 `spawn_count[7]` row): `sub_38C60` reads the per-model
    /// counter, stores it +1, and stamps the OLD value into `+63` —
    /// CARPET.EXE 0x38D03 `mov 0xc(%ecx,%eax,1),%dl`, 0x38D09 `inc
    /// %dh`, 0x38D0B `mov %dh,0xc(%ecx,%eax,1)`, 0x38D27 `mov
    /// %dl,0x3f(%ebx)` — and `sub_38C00` (:45101-13) keys life
    /// 2000/4000 and sprite 199/85 off that stamp's parity. Seven
    /// births from a zero counter therefore carry `+63` 0..6 (read
    /// 1..7 at the boundary after the walk's own clock), leave the
    /// counter at 7, and alternate 2000/4000 from the EVEN arm. The
    /// recorded row was the pair harness's (a consumed THING table
    /// after a reload — `mgc-conform verify.rs`), not this ctor's.
    #[test]
    fn seven_m7_births_stamp_the_old_ordinal_and_leave_the_counter_at_seven() {
        let mut g = mob_gen();
        assert_eq!(g.spawn_count[7], 0, "fixture: a fresh counter");
        let slots: Vec<usize> = (0..7)
            .map(|k| g.spawn_creature(7, 0x4000 + k * 256, 0x4000, 0).unwrap())
            .collect();
        assert_eq!(g.spawn_count[7], 7, "+1 per birth, stored after the read");
        for (k, &i) in slots.iter().enumerate() {
            let e = &g.ent[i];
            assert_eq!(
                e.f63, k as u8,
                "+63 = the counter BEFORE the bump (slot {i})"
            );
            let odd = k & 1 != 0;
            assert_eq!(
                e.max_life,
                if odd { 4000 } else { 2000 },
                "life keys off the stamp's parity"
            );
            assert_eq!(
                e.act_life, e.max_life as i32,
                "RefillLife after the parity arm"
            );
            assert_eq!(
                e.type86,
                if odd { 85 } else { 199 },
                "sprite keys off the same parity"
            );
            assert_eq!(e.f71, if odd { 1 } else { 2 }, "+71 = 1 (odd) / 2 (even)");
            assert_eq!(e.f26, (i % 100) as i16, "+26 = slot % 100");
            assert_eq!(e.tick70, 43, "born in WANDER (base 42 + 1)");
        }
    }

    /// The painter idle length is +60 CTOR PROVENANCE, not the kill
    /// bit alone (:30511-17): the m42 painter ctor is +60 = 1's only
    /// class-10-reachable writer (:47583) — the upgrade-commit clears
    /// it with the kill bit (:56490-92), and a record that ACQUIRES
    /// action 44 by mutation was NewEvent-zeroed, so both idle ONE
    /// tick where the plain painter idles 25. Witness: mc1hwl0
    /// t=27431 — a (10,6) fire re-mint wearing the stale-roster m7
    /// CHASE stamp finishes its bogus level-0 painter run there;
    /// the kill-bit-only proxy held it 24 ticks longer.
    #[test]
    fn painter_idle_is_ctor_provenance_not_the_kill_bit() {
        let mut g = mob_gen();
        let mk = |g: &mut Gen, model: u8, kill: bool| {
            let i = g.new_event().unwrap();
            g.ent[i].class64 = 10;
            g.ent[i].model65 = model;
            g.ent[i].tick70 = 44;
            if kill {
                g.ent[i].flags |= 0x10000;
            }
            // Past the arm write, at the pre == 1 idle-arming read.
            g.ent[i].flags |= 2;
            g.ent[i].f26 = 1;
            i
        };
        let plain = mk(&mut g, 42, false);
        g.tick_castle_painter(plain);
        assert_eq!(g.ent[plain].f26, -25, "the m42 ctor painter idles 25");
        let upgrade = mk(&mut g, 42, true);
        g.tick_castle_painter(upgrade);
        assert_eq!(g.ent[upgrade].f26, -1, "the upgrade-commit painter idles 1");
        let mutant = mk(&mut g, 6, false);
        g.tick_castle_painter(mutant);
        assert_eq!(
            g.ent[mutant].f26, -1,
            "a mutation-acquired action-44 record idles 1 (NewEvent zeroed +60)"
        );
        // …and the tick READING -1 finishes it (:30682-83).
        g.tick_castle_painter(mutant);
        assert_eq!(g.ent[mutant].f26, 0);
        assert_ne!(g.ent[mutant].flags & 0x400, 0, "finished on the -1 read");
        assert_eq!(
            g.ent[plain].flags & 0x400,
            0,
            "the plain painter still idles"
        );
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
        let ctx = ctx_at(&mut g, 0x4080, 0x4000, 0);
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
        let rig_ctx = ctx_at(&mut g, 0x4080, 0x4000, 0);
        g.creature_tick(i, &rig_ctx);
        assert_eq!(g.ent[i].type86, 198, "planted");
        let inflated = max + 4 * accel;
        g.ent[i].f126 = inflated;
        g.ent[i].f26 = 25; // timer still running — not the expiry arm

        // Now the wizard steps out of range: the shared chase drops to
        // WANDER on the cadence tick and the trailer fires.
        g.ent[i].f63 = 0;
        let rig_ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
        g.creature_tick(i, &rig_ctx);
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
        let rig_ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
        g.creature_tick(follower, &rig_ctx);
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

    /// `mc1_pack_speed_restore` (player 2026-10-10, the pack-speed
    /// RATCHET): retail's leader-lost default (:21793) and
    /// leader-joined-a-chase arm (:21781) take a follower out of the
    /// pack with its catch-up speed intact, so the orphan leads the
    /// next pack one accel step hot and every generation compounds.
    /// The patched arm writes the follower's own +128 back on both
    /// exits; the retail arm keeps the inflated speed (the ratchet's
    /// seed). Vultures (m1) have no trailer of their own, so the
    /// shared pack handler is the only writer in play.
    #[test]
    fn pack_follower_restores_its_own_speed_when_the_pack_breaks() {
        let run = |patched: bool, leader_state: u8| -> (i16, i16, u8) {
            let mut g = mob_gen();
            let leader = g.spawn_creature(1, 0x4000, 0x4000, 0).unwrap();
            let follower = g.spawn_creature(1, 0x4010, 0x4000, 0).unwrap();
            let (max, accel) = (g.ent[follower].f128, g.ent[follower].f130);
            assert!(accel > 0 && max > 0, "a vulture has a real speed triple");
            let mut ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
            ctx.patches.mc1_pack_speed_restore = patched;
            // Following a wandering leader already at its own max
            // (a vulture spawns below it): the catch-up write puts
            // the follower one accel step ABOVE its max.
            g.ent[leader].tick70 = 7; // m1 WANDER
            g.ent[leader].f126 = max;
            g.ent[follower].tick70 = 9; // m1 PACK
            g.ent[follower].f52 = leader as u16;
            g.ent[follower].f63 = 0;
            g.creature_tick(follower, &ctx);
            assert_eq!(
                g.ent[follower].f126,
                g.ent[leader].f126 + accel,
                "the catch-up is untouched under either arm"
            );
            assert!(g.ent[follower].f126 > max);
            // The leader leaves the follow states.
            g.ent[leader].tick70 = leader_state;
            g.ent[follower].f63 = 0;
            g.creature_tick(follower, &ctx);
            (g.ent[follower].f126, max, g.ent[follower].tick70)
        };
        // Leader lost (a non-follow state): retail keeps the speed and
        // wanders; the patch wanders at its own max.
        let (speed, max, state) = run(false, 11);
        assert_eq!(state, 7, "back to WANDER");
        assert!(
            speed > max,
            "retail: the orphan wanders hot ({speed} > {max})"
        );
        let (speed, max, state) = run(true, 11);
        assert_eq!(state, 7, "back to WANDER");
        assert_eq!(speed, max, "patched: the orphan wanders at its own max");
        // Leader joined a chase: the follower joins too.
        let (speed, max, state) = run(false, 8);
        assert_eq!(state, 8, "joins the CHASE");
        assert!(speed > max, "retail: into the chase hot");
        let (speed, max, state) = run(true, 8);
        assert_eq!(state, 8, "joins the CHASE");
        assert_eq!(speed, max, "patched: into the chase at its own max");
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
        let ctx = ctx_at(&mut g, 0x4200, 0x4000, 0);
        g.ent[i].tick70 = 25;
        g.ent[i].f58 = 16;
        g.ent[i].f63 = 0;
        g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, g.view(ctx.phx).x, g.view(ctx.phx).y);
        g.player_aggro[Hx::FIRST] = 200; // the +528 hostility gate
        g.creature_tick(i, &ctx);
        assert_eq!(g.ent[i].tick70, 26, "promoted to CHASE");
        assert_eq!(
            g.ent[i].f126, 0,
            "and armed on the SAME tick — sub_1BC50 stops him dead"
        );
        assert_ne!(g.ent[i].type86, 0, "wearing an armed sprite");

        // The wizard leaves: the chase breaks and the trailer disarms.
        g.ent[i].f63 = 0;
        let rig_ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
        g.creature_tick(i, &rig_ctx);
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
        let ctx = ctx_at(&mut g, 0x4200, 0x4000, 0);
        g.ent[i].tick70 = 55;
        g.ent[i].f26 = 200;
        g.ent[i].f63 = 0;
        g.ent[i].f58 = 16;
        g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, g.view(ctx.phx).x, g.view(ctx.phx).y);
        g.creature_tick(i, &ctx);
        assert_eq!(g.ent[i].tick70, 56, "surfaced into CHASE");
        assert_eq!(
            (g.ent[i].f126, g.ent[i].type86),
            (0, 202),
            "rooted in the warrior form on the promotion tick"
        );

        g.ent[i].f63 = 0;
        let rig_ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
        g.creature_tick(i, &rig_ctx);
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
            let rig_ctx = ctx_at(&mut g, 0x4080, 0x4000, 0);
            g.creature_tick(i, &rig_ctx);
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
        let rig_ctx = ctx_at(&mut g, 0x4080, 0x4000, 0);
        g.creature_tick(i, &rig_ctx);
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
        let rig_ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
        g.creature_tick(follower, &rig_ctx);
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
    /// ⭐⭐ **BUCKET[0] IS A TICK-TOP SNAPSHOT, AND ITS WALKERS CARRY NO
    /// LIFE TEST.** The case-3 arm of the tick-top sweep (:52253-62)
    /// samples `actLife >= 0 && (+16 & 0x10) == 0` ONCE and links the
    /// survivors into `var_u32_36462[0]`; the two consumers — the m9
    /// mound's castle hunt (:23752, `+65 == 2 && +24 != own`) and the
    /// shared creature Scan A (:21519-42, `(+16 & 0x20) == 0`) — walk
    /// that list and test nothing else. So a class-3 body that takes
    /// its fatal hit MID-tick is still every later walker's answer for
    /// the rest of that tick, and gone from the next one.
    ///
    /// mc1l4 t=1224 is the corpus receipt: castle 71 dies earlier in
    /// the tick and all four (5,9) mounds still write its bearing
    /// (`+34` holds 1174/1102/1114/1137) where the port's live-pool
    /// scan lost it and fell to the two-draw wander jitter. The take's
    /// free-run horizon moves 1016 → 1223 boundaries on this alone.
    /// The lane is invisible to `level_005`'s goldens (measured: they
    /// do not move), so the round trip is pinned here.
    #[test]
    fn bucket_zero_is_a_tick_top_snapshot() {
        let mut g = mob_gen();
        let mound = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
        // Far enough that the hunt's range test FAILS — the mound keeps
        // re-bearing from its lurk instead of promoting to chase, which
        // is the arm this test is about.
        let castle = g.spawn_castle(0x6000, 0x4000).unwrap();
        g.ent[castle].id24 = 7; // a rival's — the mound may hunt it
        let away = ctx_at(&mut g, 0x7F00, 0x7F00, 0);
        g.ent[mound].tick70 = 55;
        g.ent[mound].f26 = 200;
        g.ent[mound].f58 = 0; // asleep: no wizard scan, castle arm only
        g.ent[mound].f63 = 0; // on the cadence

        // The roster is built while the castle lives…
        g.rebuild_wiz_chain();
        assert!(
            g.wiz_chain.list.contains(&(castle as u16)),
            "a live castle joins bucket[0]"
        );
        // …and THEN it dies, mid-tick, exactly as retail's does.
        g.ent[castle].act_life = -1;
        let rand_before = g.ent[mound].rand;
        g.creature_tick(mound, &away);
        assert_eq!(
            g.ent[mound].rand, rand_before,
            "the cadence found the castle on the stale roster — no wander draw"
        );
        assert_eq!(
            g.ent[mound].f34,
            Gen::angle_between(
                g.ent[mound].x,
                g.ent[mound].y,
                g.ent[castle].x,
                g.ent[castle].y
            ),
            "and it re-bore on the dead castle"
        );
        assert_eq!(g.ent[mound].tick70, 55, "still lurking — out of range");

        // Next tick's rebuild drops it, and only then does the mound
        // fall through to the two-draw jitter.
        g.rebuild_wiz_chain();
        assert!(
            !g.wiz_chain.list.contains(&(castle as u16)),
            "the dead castle leaves bucket[0] at the NEXT rebuild"
        );
        g.ent[mound].f63 = 0;
        let rand_before = g.ent[mound].rand;
        g.creature_tick(mound, &away);
        assert_ne!(
            g.ent[mound].rand, rand_before,
            "with the roster empty the wander else-arm draws twice"
        );
    }

    #[test]
    fn a_mounds_castle_bolt_carries_the_castles_filter_not_the_wild_card() {
        let mut g = mob_gen();
        let mound = g.spawn_creature(9, 0x4000, 0x4000, 0).unwrap();
        let castle = g.spawn_castle(0x4100, 0x4000).unwrap();
        g.ent[castle].id24 = 7; // a rival's, so the mound will take it
        let away = ctx_at(&mut g, 0x7F00, 0x7F00, 0);

        // Lurking, on the castle-hunt cadence: it surfaces into CHASE
        // and the entry trailer narrows the filter on that tick.
        g.ent[mound].tick70 = 55;
        g.ent[mound].f26 = 200;
        g.ent[mound].f58 = 16;
        g.ent[mound].f63 = 0;
        // The castle hunt walks BUCKET[0], the tick-top class-3 roster.
        g.rebuild_wiz_chain();
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
                let rig_ctx = ctx_at(&mut g, 0x4100, 0x4100, 0);
                g.creature_tick(i, &rig_ctx);
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
            let rig_ctx = ctx_at(&mut g, 0x4080, 0x4000, 0);
            g.creature_tick(i, &rig_ctx);
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
            let ctx = ctx_at(&mut g, 0x4200, 0x4000, 0); // 2 tiles east, in v_28
            g.ent[i].tick70 = 55; // surfaced mound (state 55)
            g.ent[i].f26 = 200; // burrow timer armed, no bury edge
            g.ent[i].f63 = 0; // on the v_26 scan tick
            g.ent[i].f58 = f58;
            g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, g.view(ctx.phx).x, g.view(ctx.phx).y);
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
            // The convert's victim scan walks the TICK-TOP per-model
            // roster (:23887-98) — rebuild it the way the tick head
            // does, or the scan sees an empty chain.
            g.rebuild_mob_chains();
            let ctx = ctx_at(&mut g, 0x7F00, 0x7F00, 0); // player far away
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
        let ctx = ctx_at(&mut g, 0x4200, 0x4000, 0);
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
            let ctx = ctx_at(&mut g, 0x4200, 0x4000, 0); // 2 tiles east, in v_28
            assert_eq!(g.ent[i].tick70, wander, "m{model} spawns wandering");
            assert!(g.ent[i].f58 != 0, "m{model} spawns awake");
            g.ent[i].f63 = 0; // on the v_26 scan tick
            let facing = Gen::angle_between(0x4000, 0x4000, g.view(ctx.phx).x, g.view(ctx.phx).y);
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
        let ctx = ctx_at(&mut g, 0x4200, 0x4000, ground); // 2 tiles east, in range
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
            let ctx = ctx_at(&mut g, 0x4200, 0x4000, 0); // 2 tiles east, well inside v_28
            g.ent[i].f30 = Gen::angle_between(0x4000, 0x4000, g.view(ctx.phx).x, g.view(ctx.phx).y); // in cone
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
        let near = ctx_at(&mut g, 0x4100, 0x4000, 300); // 256 away, avatar
        let drained0 = g.mc2_player_drain[Hx::FIRST].0;
        g.m26_tick(i, &near);
        assert_eq!(g.ent[i].tick70, 210, "in-range avatar: stay draining");
        assert!(
            g.mc2_player_drain[Hx::FIRST].0 > drained0,
            "the drain landed"
        );
        // Far target: the one authentic exit.
        let far = ctx_at(&mut g, 0x4000u16.wrapping_add(0x7000), 0x4000, 300);
        g.ent[i].f63 = 0;
        g.m26_tick(i, &far);
        assert_eq!(g.ent[i].tick70, 209, "out of range: back to approach");
    }

    /// ⚠⚠ THE TEST-ONLY CHAIN BUILDER DRIFTED FROM THE REAL SWEEP.
    /// `World::tick`'s tick-top sweep admits class-10 models **39, 40
    /// and — on MC2 only — 57**, the fool's sphere (`dword_38523`,
    /// EF:40023-62; the same third member the whirlwind victim list
    /// names, `10 => matches!(c.model65, 13 | 14 | 39 | 57)` in
    /// `mc2::tail`). [`Gen::rebuild_ball_chain`], the `#[cfg(test)]`
    /// twin every bare-`Gen` test publishes its chain with, admitted
    /// only `39 | 40` — so a test that minted a 57 and asked the aura
    /// (or the possession lock) to find it got an EMPTY chain and
    /// asserted nothing.
    ///
    /// NON-VACUOUS: restore `matches!(e.model65, 39 | 40)` in
    /// `rebuild_ball_chain` and the first assert fails (len 1, not 2).
    /// POSITIVE CONTROL: the MC1 column, where model 57 is an
    /// unrelated logic model and must NOT join — that arm passes
    /// before and after.
    #[test]
    fn the_test_ball_chain_admits_the_mc2_fools_sphere() {
        let mk = |g: &mut Gen, model: u8| {
            let s = g.new_event().unwrap();
            let e = &mut g.ent[s];
            e.class64 = 10;
            e.model65 = model;
            s
        };
        let mut g = mc2_gen();
        let ball = mk(&mut g, 39);
        let fool = mk(&mut g, 57);
        g.rebuild_ball_chain();
        assert_eq!(
            g.ball_chain.list,
            vec![ball as u16, fool as u16],
            "MC2: the (10,57) fool's sphere is a member of dword_38523"
        );
        // The MC1 column's model 57 is unrelated logic: not a member.
        let mut g1 = Gen::new(
            flat_land(8),
            synthetic_assets(),
            1,
            ChassisParams::MC1,
            crate::verbs::VerbSet::MC1,
        );
        let b1 = mk(&mut g1, 39);
        let _ = mk(&mut g1, 57);
        g1.rebuild_ball_chain();
        assert_eq!(
            g1.ball_chain.list,
            vec![b1 as u16],
            "MC1: model 57 is not a mana-ball-chain member"
        );
    }

    /// The aura claim handshake — the first aura in slot order keeps
    /// an overlapped ball; the second must not overwrite the pull
    /// (first-writer-wins, NOT last-writer-wins).
    ///
    /// ⭐ AND THE BALL MUST BE ON THE TICK-TOP CHAIN TO BE PULLED AT
    /// ALL. `mc2_aura_tick` walks `dword_38523` (EF:28362), not the
    /// live pool, so this test now builds the chain the way the tick
    /// top does — which is also what makes it a regression guard for
    /// the mc2l3 t=9816 law: drop the `rebuild_ball_chain()` below and
    /// the aura pulls nothing, exactly as retail pulls nothing from a
    /// sphere born mid-tick.
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
        g.rebuild_ball_chain();
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

    /// ⭐⭐⭐ **A SPHERE BORN MID-TICK IS INVISIBLE TO THE MAGNET UNTIL
    /// THE NEXT FRAME** — `sub_38D80` (EF:28362) scans `dword_38523`,
    /// and that chain is rebuilt by the TICK-TOP sweep
    /// (EF:40023-40062), so membership is sampled once, before
    /// anything this tick is born.
    ///
    /// The port used to walk `1..ent.len()` under a registered
    /// approximation ("a pool slot-order list standing in for retail's
    /// `dword_38523` list") and pulled newborns on their birth tick,
    /// putting every sphere ONE TICK AHEAD of retail's for the rest of
    /// the run. mc2l3 t=9816 is the demanding row — the player's cast
    /// borns a (10,54) aura and eleven (10,39) spheres in one frame,
    /// and retail's newborn slot 170 sits still (`dest` 0/0, `yaw` 0)
    /// where the pool walk gave it `dest` 7/42 and moved it to
    /// 36743/16554, **which is retail's own t=9817 pose**. Free run
    /// 9815 → 10055.
    ///
    /// ⚠ THIS IS A UNIT TEST BECAUSE THE PAIR LANE CANNOT ASSERT IT.
    /// The demanding tick is a CAST-BIRTH tick whose pair (9815→9816)
    /// is dirty for unrelated import-side reasons — the port's newborn
    /// (9,1) lands at 36488/16554 against retail's 36736/16768 with
    /// `dest` 0/0/0 against 35822/7087/2339 — so a fixture there would
    /// be a known-failing pair, which is not an assertion. Measured,
    /// not assumed: the pair is dirty in the FULL take, not merely in
    /// an isolated cut.
    ///
    /// Non-vacuous: restore the pool walk and the first assert flips —
    /// the newborn is pulled on the tick it is born.
    #[test]
    fn mc2_aura_cannot_pull_a_sphere_born_after_the_tick_top() {
        let mut g = mc2_gen();
        let aura = g.new_event().unwrap();
        {
            let e = &mut g.ent[aura];
            e.x = 0x4000;
            e.y = 0x4000;
            e.f26 = 14; // tile range
            e.act_life = 100;
        }
        let sphere = |g: &mut Gen| {
            let b = g.new_event().unwrap();
            let e = &mut g.ent[b];
            e.class64 = 10;
            e.model65 = 39;
            e.x = 0x4200; // two tiles out, well inside the range
            e.y = 0x4000;
            b
        };
        // One sphere that already existed when the sweep ran...
        let member = sphere(&mut g);
        g.rebuild_ball_chain(); // the tick-top sweep
        // ...and one born AFTER it, in the same tick.
        let newborn = sphere(&mut g);

        g.mc2_aura_tick(aura);
        // ⚠ THE OBSERVABLE IS THE STAMP, NOT THE VELOCITY. `sub_38D80`
        // writes `@0x76` (speed) and `@0x7A` (the aura's index) — the
        // port's `mail[4]` pair — and NOTHING else; the pulled
        // sphere's `axis_0x9A` is written by the SPHERE, in its own
        // tick (`Gen::ball_tick`'s ch4 intake). See
        // [`crate::mc2::tail::no_mc2_aura_stamp`].
        assert_eq!(
            g.ent[member].mail[4].1 as usize, aura,
            "the tick-top member takes the pull"
        );
        assert_eq!(
            g.ent[newborn].mail[4],
            (0, 0),
            "the sphere born after the sweep is not pulled on its birth tick"
        );
        assert!(
            !g.mc2_aura_claim.0.contains_key(&(newborn as u16)),
            "and is not even claimed — retail never saw it"
        );

        // Next frame: the sweep admits it and the pull lands.
        g.rebuild_ball_chain();
        g.mc2_aura_tick(aura);
        assert_eq!(
            g.ent[newborn].mail[4].1 as usize, aura,
            "the next frame's chain carries it and the magnet takes it"
        );
    }

    /// ⭐⭐⭐ **THE MAGNET DOES NOT TEST THE REAP BIT — SOFT KILL IS
    /// NOT A FREE.** `sub_38D80`'s whole loop body is
    /// `if (!ix->str_0x5E_94.word_0x7A_122)` (EF:28364): no 0x400
    /// test, no liveness test, no class or model test. A sphere that
    /// something flagged EARLIER in the same pass keeps its class,
    /// model and chain links for the rest of the tick, so the magnet
    /// still stamps it and it takes ONE MORE full pull step on its
    /// dying tick.
    ///
    /// ⚠ UNIT, NOT PAIR: the demanding row is mc2l6-rsg t=3554→3555,
    /// where ball 743 is already 0x400 when aura 569 dispatches;
    /// retail pulls it (−42, +3) — `MoveEntity_57FA0`'s own floor of
    /// `(42·SIN[1519])>>16` — where the port's gate skipped it and
    /// left it riding the previous tick's friction residue (−41, +2).
    /// That pair is NOT conforming in the full take (measured, not
    /// assumed: `extract --sample-every 1` lists 3519/3542/3550/3919/
    /// 3992 and NOT 3554), so a fixture there would be a
    /// known-failing pair, which is not an assertion. Free run
    /// 3554 → 3919.
    ///
    /// Non-vacuous: restore the `c.flags & 0x400 != 0` skip and the
    /// second assert flips to `(0, 0)`.
    #[test]
    fn mc2_aura_pulls_a_sphere_flagged_earlier_in_the_same_pass() {
        let mut g = mc2_gen();
        let aura = g.new_event().unwrap();
        {
            let e = &mut g.ent[aura];
            e.x = 0x4000;
            e.y = 0x4000;
            e.f26 = 14; // tile range
            e.act_life = 100;
        }
        let sphere = |g: &mut Gen| {
            let b = g.new_event().unwrap();
            let e = &mut g.ent[b];
            e.class64 = 10;
            e.model65 = 39;
            e.x = 0x4200;
            e.y = 0x4000;
            b
        };
        let live = sphere(&mut g);
        let doomed = sphere(&mut g);
        g.rebuild_ball_chain();
        // Something at a LOWER pool slot soft-kills it this tick.
        g.ent[doomed].flags |= 0x400;

        g.mc2_aura_tick(aura);
        // The stamp (`@0x76`/`@0x7A` = `mail[4]`) is the whole write
        // `sub_38D80` makes — see [`no_mc2_aura_stamp`].
        assert_eq!(
            g.ent[live].mail[4].1 as usize, aura,
            "the live sphere takes the pull"
        );
        assert_eq!(
            g.ent[doomed].mail[4].1 as usize, aura,
            "and so does the one already flagged — the loop never asks"
        );
        assert!(
            g.mc2_aura_claim.0.contains_key(&(doomed as u16)),
            "it is claimed like any other chain member"
        );
    }

    /// ⭐⭐⭐ **THE POSSESSION DELIVERY IS THE ONE IMPACT THAT DOES NOT
    /// RE-POINT ITS OWNER WIZARD.** `sub_686D0` (EF:55192) has
    /// exactly four call sites — EF:58408 (lightning `sub_66750`),
    /// EF:62983 (the generic `sub_65820`), EF:63187 (the fireball
    /// `sub_65C20`) and EF:63549 (`sub_662E0`, and only down its
    /// class-3 model-0/1 victim branch) — and NEITHER possession
    /// worker is among them: `CastPosses_65F60` (action **1**, the
    /// basic (9,1) bolt) runs its whole impact block EF:63304-19 as
    /// spawn → `sub_65780` → `sub_6D8B0` → id/yaw/pitch copy →
    /// disable, and the leveled `sub_674C0` (action **18**,
    /// EF:59029-58) is the same shape twice over.
    ///
    /// ⭐ AND THAT IS WHAT KEEPS A RIVAL'S POSSESS GOAL ALIVE — the
    /// ball pick runs on the think cadence only (`f63 % (64 −
    /// refl/4)`, EF:5522) while `sub_135C0` casts every tick, so a
    /// rival fires several bolts at the ball it chose and a re-point
    /// on each hit bricked the goal. mc2l6-rsg t=3519: rival 378
    /// picked ball 851 at t=3507 (cadence 52), its bolt struck ball
    /// 519, and retail's `target96` stays 851. Free run 3519 → 3542.
    ///
    /// ⚠ UNIT, NOT PAIR: `word_0x96_150` has no lane in the `.mgcr`
    /// obs channel at all, so a pair fixture cannot see it — measured,
    /// not assumed: a fixture cut at t=3518 passed unchanged under the
    /// reverted build and was deleted rather than kept as ballast.
    ///
    /// Non-vacuous: drop the `!matches!(act, 1 | 18)` gate and the
    /// second assert reads the ball's slot.
    #[test]
    fn mc2_possession_impact_leaves_the_owners_target_alone() {
        let mut g = mc2_gen();
        let wiz = g.new_event().unwrap();
        {
            let e = &mut g.ent[wiz];
            e.class64 = 3;
            e.model65 = 1;
            e.id24 = wiz as u16;
            e.f146 = 4242; // the picker's ball
        }
        let ball = g.new_event().unwrap();
        {
            let e = &mut g.ent[ball];
            e.class64 = 10;
            e.model65 = 39;
        }
        let ctx = ctx_at(&mut g, 0x4000, 0x4000, 300);
        let bolt = |g: &mut Gen, act: u8| {
            let i = g.new_event().unwrap();
            let e = &mut g.ent[i];
            e.class64 = 9;
            e.model65 = if act == 1 { 1 } else { 17 };
            e.tick70 = act;
            e.id24 = wiz as u16;
            e.f68 = 10;
            e.f69 = 12;
            i
        };
        // A fireball (action 0) DOES re-point — the three other
        // workers stamp any victim.
        let fb = bolt(&mut g, 0);
        g.mc2_proj_impact(fb, ball as u16, &ctx, None);
        assert_eq!(
            g.ent[wiz].f146, ball as u16,
            "a generic impact re-points the shooter"
        );

        g.ent[wiz].f146 = 4242;
        for act in [1u8, 18] {
            let b = bolt(&mut g, act);
            g.mc2_proj_impact(b, ball as u16, &ctx, None);
            assert_eq!(
                g.ent[wiz].f146, 4242,
                "possession action {act} must leave word_0x96_150 alone"
            );
        }
    }

    /// ⭐⭐⭐ **THE POSSESSION LOCK'S SPHERE WALK RE-ASKS MODEL, OWNER
    /// AND AWAKE — AND NOTHING ELSE.** `sub_67CB0`'s `dword_38523`
    /// sweep (EF:55016-31) reads `model_0x40_64` (in {39, 57}), the
    /// model-keyed ownership lane and `byte_0x39_57`. There is no
    /// class test, no life test and no hidden-bit test, for the same
    /// reason the wizard and creature lists above have none:
    /// membership was settled by the tick-top sweep, so **a sphere
    /// FREED earlier in this very tick is still a lock candidate.**
    /// The free clears the class byte and the map link but leaves the
    /// model, the awake byte, the position — and the chain pointer.
    ///
    /// The port re-asked `class64 == 10`, `act_life >= 0` and
    /// `!(flags & 0x400)`, so it fell through such a sphere to the
    /// next-best one. mc2l3 t=10055 is the corpus row: sphere 237 is
    /// freed before the human's basic (9,1) at slot 232 dispatches,
    /// and retail locks it anyway (`word_0x96_150` = 237, bearing yaw
    /// 248 / pitch 96) where the guarded port took sphere 227 — 119
    /// units farther — and bore 250 / 92. Free run 10055 → 10222.
    ///
    /// ⚠ A UNIT TEST BECAUSE THE PAIR LANE IS BLIND TO IT. The cut
    /// fixture at t=10054 was built and MEASURED: it passes with the
    /// guards restored, so it asserts nothing. Under `pin_pose n1`
    /// the bolt is born from retail's own pose and both readings land
    /// within the graded tolerance; only the free run separates them.
    ///
    /// Non-vacuous: restore any one of the three guards and the
    /// second assert flips back to the farther sphere.
    #[test]
    fn a_freed_sphere_is_still_a_possession_lock_candidate() {
        use crate::mc2::proj::AimProbe;
        let mut g = mc2_gen();
        // Both spheres straight ahead of the probe (yaw 0 = −y), the
        // nearer one first.
        let sphere = |g: &mut Gen, d: u16| {
            let b = g.new_event().unwrap();
            let e = &mut g.ent[b];
            e.class64 = 10;
            e.model65 = 39;
            e.x = 0x4000;
            e.y = 0x4000 - d;
            e.act_life = 300;
            e.f58 = 128; // awake
            e.f144 = 0; // unowned — the ownership lane for model 39
            b
        };
        let near = sphere(&mut g, 1000);
        let far = sphere(&mut g, 2000);
        g.rebuild_ball_chain(); // the tick-top sweep: both are members
        let probe = AimProbe {
            x: 0x4000,
            y: 0x4000,
            z: 0,
            yaw: 0,
            pitch: 0,
            model: 1, // the basic possession bolt
            own: 5,
            range: 8192, // the wizard owner row (str_D7BD6[67].v_28)
            reach: 0,
        };
        // Precondition (and the cone/convention calibration): with
        // both alive the distance-dominated scorer takes the nearer.
        assert_eq!(
            g.mc2_aim_scan(&probe, &[]),
            Some(near as u16),
            "both live: the nearer sphere wins on score"
        );

        // Now free the nearer one the way retail's reap does — class
        // byte and link cleared, everything the walk reads left alone,
        // and still chained.
        {
            let e = &mut g.ent[near];
            e.class64 = 0;
            e.act_life = -1;
            e.flags |= 0x400;
        }
        assert_eq!(
            g.mc2_aim_scan(&probe, &[]),
            Some(near as u16),
            "the walk re-asks model/owner/awake only — a sphere freed \
             mid-tick is still the best candidate"
        );
        assert_ne!(far, near, "the fall-through target is a distinct slot");
    }

    /// ⭐⭐⭐ **THE IDLE WIZARD SCAN HAS NO MODEL TEST — IT CAN HAND
    /// BACK A CASTLE.** `sub_1BF90`'s sweep (EF:9147-66) walks the
    /// tick-top class-3 roster `dword_38519` and tests exactly two
    /// things before the cone: the squared range and the invisibility
    /// bit. Class, model, life and the reap flag were settled when the
    /// case-3 arm of the tick-top sweep built the roster.
    ///
    /// The port filtered `model65 <= 1` on a LIVE-POOL walk, so a
    /// (3,2) castle could never be returned — which quietly made the
    /// m20/m13 state-2 wrappers unreachable. Those wrappers
    /// (`sub_25DE0` EF:16637-42) exist precisely to re-read the lock
    /// after the scan and clear it unless the record is class 3 and
    /// model 0 or 1. Because the scorer is nearest-in-cone, filtering
    /// the castle out does not merely skip a doomed candidate: it lets
    /// a FARTHER wizard win a scan retail gives to the castle, and the
    /// creature then keeps a lock retail never had.
    ///
    /// mc2l3 t=10222 is the corpus row — the (5,20) at slot 1 takes
    /// state 162 with the lock already cleared and falls straight back
    /// to 161 at 10223, where the port locked the HUMAN, survived the
    /// wrapper and stayed in 162 chasing (yaw 563 v retail's 336).
    /// Free run 10222 → 10298.
    ///
    /// ⚠ A UNIT TEST BECAUSE THE PAIR LANE IS BLIND TO IT: at 10222
    /// the whole divergence lives on UNGRADED lanes (`word_0x96_150`,
    /// @0x24, @0x5D), and a pair imported at 10222 gets retail's
    /// cleared lock handed to it, so both readings bounce back to 161.
    /// Only the free run carries the wrong lock forward.
    ///
    /// Non-vacuous: restore the `model65 <= 1` filter and the scan
    /// returns the human.
    #[test]
    fn the_idle_wizard_scan_can_return_a_castle() {
        let mut g = mc2_gen();
        let creature = g.new_event().unwrap();
        {
            let e = &mut g.ent[creature];
            e.class64 = 5;
            e.model65 = 20;
            e.x = 0x4000;
            e.y = 0x4000;
            e.f30 = 0; // yaw 0 = −y, both candidates dead ahead
            e.row156 = 89; // the m20 behaviour row
            e.act_life = 100;
        }
        // A castle 40 units ahead...
        let castle = g.new_event().unwrap();
        {
            let e = &mut g.ent[castle];
            e.class64 = 3;
            e.model65 = 2;
            e.x = 0x4000;
            e.y = 0x4000 - 40;
            e.act_life = 100;
        }
        g.rebuild_wiz_chain(); // the tick-top case-3 sweep
        // ...and the human twice as far, straight past it.
        let ctx = ctx_at(&mut g, 0x4000, 0x4000 - 80, 0);

        assert_eq!(
            g.mc2_wizard_scan(creature, &ctx, false),
            Some(castle as u16),
            "the nearest class-3 in cone wins, and retail's walk has no \
             model test — the castle is a candidate"
        );

        // The roster half: a class-3 born AFTER the sweep is not in
        // this frame's `dword_38519`, however near it stands. The live
        // pool walk saw it immediately.
        let newborn = g.new_event().unwrap();
        {
            let e = &mut g.ent[newborn];
            e.class64 = 3;
            e.model65 = 0;
            e.x = 0x4000;
            e.y = 0x4000 - 10; // nearer than the castle
            e.act_life = 100;
        }
        assert_eq!(
            g.mc2_wizard_scan(creature, &ctx, false),
            Some(castle as u16),
            "a wizard minted after the tick-top sweep is invisible to \
             this frame's scan"
        );
        g.rebuild_wiz_chain();
        assert_eq!(
            g.mc2_wizard_scan(creature, &ctx, false),
            Some(newborn as u16),
            "and the next frame's roster carries it"
        );
    }

    /// ⭐⭐⭐ **A DEAD WIZARD IS NOT IN `dword_38519`, AND THE
    /// OUT-OF-POOL HUMAN BYPASSES THAT MEMBERSHIP TEST.**
    ///
    /// The case-3 arm of the tick-top sweep is `if (jx->life_0x8 >= 0)`
    /// (EF:39975) — a dead wizard is simply never linked into the
    /// class-3 roster, which is precisely why `sub_1BF90`'s walk
    /// carries no mortality test of its own (EF:9154 tests the
    /// squared range and the invisibility bit, and nothing else).
    ///
    /// Retail's human is a pool record and gets that entry test for
    /// free. Ours is a ctx pose `consider()`d alongside the roster, so
    /// it had to take the roster's entry condition explicitly — and it
    /// did not, in all three of the port's class-3 scans.
    ///
    /// mc2l3 t=11757 is the corpus row: the player has been dead since
    /// t≈11700 and retail's (5,20) at slot 161 sits in idle 161 for
    /// the rest of the take, where the port's scan handed back
    /// `PLAYER_TARGET`, took 162, and `m20_validate` zeroed
    /// `byte_0x46_70` on the way through. Free run 11756 → 14610.
    ///
    /// ⚠ NOT THE INVISIBILITY BIT. 39's "the death touchdown raises
    /// the invisibility bit" covers a different tick of the death
    /// sequence — retail's carpet record reads `flags` 269 at 11757,
    /// so bit 5 is CLEAR and only the life test can exclude it.
    ///
    /// ⚠ A UNIT TEST BECAUSE THE PAIR LANE IS BLIND TO IT: cut at the
    /// pair start 11756 the fixture PASSES with the law reverted, and
    /// the pair itself is dirty on `field:10,0:z`, a family the law
    /// never touches. Measured both ways before falling back here.
    ///
    /// Non-vacuous: drop the `view.dead` term and the scan returns the
    /// corpse.
    #[test]
    fn a_dead_wizard_is_not_in_the_tick_top_class_3_roster() {
        let mut g = mc2_gen();
        let creature = g.new_event().unwrap();
        {
            let e = &mut g.ent[creature];
            e.class64 = 5;
            e.model65 = 20;
            e.x = 0x4000;
            e.y = 0x4000;
            e.f30 = 0; // yaw 0 = −y: both candidates dead ahead
            e.row156 = 89; // the m20 behaviour row
            e.act_life = 100;
        }
        // A pool wizard 80 units out — the fallback candidate.
        let far = g.new_event().unwrap();
        {
            let e = &mut g.ent[far];
            e.class64 = 3;
            e.model65 = 0;
            e.x = 0x4000;
            e.y = 0x4000 - 80;
            e.act_life = 100;
        }
        g.rebuild_wiz_chain();

        // The human stands NEARER, at 40. Alive, it wins outright —
        // the scorer is nearest-in-cone.
        let ctx = ctx_at(&mut g, 0x4000, 0x4000 - 40, 0);
        assert_eq!(
            g.mc2_wizard_scan(creature, &ctx, false),
            Some(crate::mc1::mobs::PLAYER_TARGET),
            "an alive human is the nearest class-3 in cone"
        );

        // Dead, retail's sweep never linked it, so the scan cannot see
        // it however near it stands — and the FAR wizard wins.
        g.view_mut(ctx.phx).dead = true;
        assert_eq!(
            g.mc2_wizard_scan(creature, &ctx, false),
            Some(far as u16),
            "a dead wizard is not in dword_38519, so the scan falls              through to the farther live one"
        );

        // The same entry test on the sibling class-3 scan that shares
        // the roster — the archer Scan A / m24 acquire. ⚠ The THIRD
        // site, `m9_cone_scan`, is private to mc2/roster.rs and is not
        // asserted here; it took the identical one-line change and is
        // covered only by the corpus sweep.
        assert_eq!(
            g.mc2_class3_scan(creature, &ctx),
            Some(far as u16),
            "mc2_class3_scan takes the roster's own entry condition"
        );

        // …and the pool arm applies it to POOL wizards already, which
        // is what makes the human's omission an inconsistency rather
        // than a second law: kill the far one and nothing is left.
        g.ent[far].act_life = -1;
        g.rebuild_wiz_chain();
        assert_eq!(
            g.mc2_wizard_scan(creature, &ctx, false),
            None,
            "with both class-3s dead the roster is empty"
        );
    }

    /// ⭐⭐⭐ **THE CLASS-3 SCAN WALKS THE TICK-TOP ROSTER, SO IT IS
    /// WRONG IN BOTH DIRECTIONS TO RE-ASK MEMBERSHIP MID-WALK.**
    ///
    /// All three retail sites — the archer's Scan A (`sub_1FAA0`,
    /// EF:11782), m24 acquire (`sub_28690`, EF:18754) and the
    /// stage-held kind-2 wizard watch (`sub_1DBF0`, EF:10300) — load
    /// `dword_38519` and chase `next_0`, and the per-node body asks
    /// exactly two things: the squared range and `!(byte[0] & 0x20)`.
    /// Shipped `NETHERW.EXE`, `sub_1DBF0`'s copy (file = VA + 0x24800):
    /// `1dcc0 8b b6 77 96 00 00` loads the chain HEAD, `1dcf3 3b 45 f4`
    /// is the range test, `1dcf8 f6 46 0c 20` the invisibility test,
    /// `1dd49 8b 36` the `next_0` step. NO life test, NO class test,
    /// NO reap test — all three were settled when the case-3 arm of
    /// the tick-top sweep built the chain (`if (jx->life_0x8 >= 0)`,
    /// EF:40224, run AFTER the `byte[1] & 4` reap pass at EF:40202).
    ///
    /// So a pool walk is wrong BOTH WAYS, and round 137 witnessed both
    /// on stage-held kind-2 DEVILS (5,21), with the values exactly
    /// mirrored between two takes:
    /// * DEATH — mc2l10 pair 12683→12684 slot 105: slot 45, a (3,3),
    ///   enters the tick a member at `life 0` and is knocked to −500
    ///   before slot 105 dispatches. Retail engages the corpse
    ///   (`target96 0→45, sv2 2→10, action 175→170, speed 60→96`); the
    ///   port's `act_life >= 0` rejected it. Pinned as a fixture.
    /// * BIRTH — mc2l32 pair 4341→4342 slot 342: the human's castle is
    ///   born into slot 214 DURING the tick, so retail's roster does
    ///   not hold it and the devil stays put; the port's live walk
    ///   found it and fired the whole engage.
    ///
    /// ⚠ THE BIRTH ARM IS A UNIT TEST BECAUSE ITS PAIR IS NOT
    /// FIXABLE WORK: cut at 4341 the fixture fails with the law ON
    /// too, on `field:3,2:life field:3,2:max_life missing:10,79` —
    /// `retail_import_mc2` cannot reconstruct a castle mid-birth. The
    /// free run over that tick IS bit-exact; only the pair lane is
    /// blind, so the arm is asserted directly here instead.
    ///
    /// `MGC_NO_MC2_CLASS3_SCAN_ROSTER=1` restores the pool walk.
    #[test]
    fn the_class_3_scan_reads_the_roster_not_the_live_pool() {
        let mut g = mc2_gen();
        let creature = g.new_event().unwrap();
        {
            let e = &mut g.ent[creature];
            e.class64 = 5;
            e.model65 = 20;
            e.x = 0x4000;
            e.y = 0x4000;
            e.f30 = 0; // yaw 0 = −y: candidates dead ahead
            e.row156 = 89; // the m20 behaviour row
            e.act_life = 100;
        }
        // A class-3 that was a member when the chain was built and
        // then DIED this tick. Retail's walk still sees it.
        let dying = g.new_event().unwrap();
        {
            let e = &mut g.ent[dying];
            e.class64 = 3;
            e.model65 = 0;
            e.x = 0x4000;
            e.y = 0x4000 - 40;
            e.act_life = 0; // a member at chain-build time
        }
        g.rebuild_wiz_chain();
        g.ent[dying].act_life = -500; // …knocked dead mid-tick
        let ctx = ctx_at(&mut g, 0x4000, 0x4000 - 4000, 0); // human far out of range
        assert_eq!(
            g.mc2_class3_scan(creature, &ctx),
            Some(dying as u16),
            "a class-3 that died AFTER the roster was built is still a member"
        );

        // …and a class-3 BORN after the chain was built is not one yet,
        // however near it stands.
        let newborn = g.new_event().unwrap();
        {
            let e = &mut g.ent[newborn];
            e.class64 = 3;
            e.model65 = 2; // a castle, the mc2l32 witness
            e.x = 0x4000;
            e.y = 0x4000 - 10; // NEARER than `dying`: a pool walk would take it
            e.act_life = 100;
        }
        assert_eq!(
            g.mc2_class3_scan(creature, &ctx),
            Some(dying as u16),
            "a class-3 born THIS tick is not in the roster, so the corpse still wins"
        );
        // Non-vacuity guard: once the roster is rebuilt the newborn is
        // a member and, being nearer, wins outright — so the assertion
        // above really is about roster membership and not about range.
        g.rebuild_wiz_chain();
        assert_eq!(
            g.mc2_class3_scan(creature, &ctx),
            Some(newborn as u16),
            "after the next tick-top rebuild the newborn is the nearest member"
        );
    }

    /// ⭐⭐⭐ **THE PARALYZE LATCH IS READABLE THE INSTANT THE STAMP
    /// WRITES IT.** Retail has ONE storage for it — `sub_38F70` sets
    /// `mobilizeCounter_0x14E_334` on the wizard's own `str_164`
    /// (EF:28442-43) — so every record dispatching after the stamp
    /// sees it, this tick and the next, right up to the carpet's own
    /// dispatch.
    ///
    /// The port has TWO: `flight::Mc2Ext::mobilize`, which owns the
    /// counter and its decay, and `Gen::mc2_mobilize`, the pool-side
    /// mirror the creature brains read. The stamp used to only push a
    /// count into `Gen::mc2_debuffs`, drained at the NEXT carpet
    /// dispatch — a whole walk pass late for every slot below the
    /// carpet's.
    ///
    /// mc2l3 t=10297: the m20's own (9,21) lob lands, its (10,66)
    /// stamp at slot 175 paralyzes the human, and retail's m20 at
    /// slot 1 reads the latch at 10298 and commits its melee rush
    /// (`byte_0x46_70` 0 → 1, speed 32 → 64 at 10299). The port
    /// committed at 10299 and doubled at 10300 — the chase column one
    /// tick late, and every downstream tick with it. Free run
    /// 10298 → 11730.
    ///
    /// ⚠ A UNIT TEST BECAUSE THE PAIR LANE CANNOT SEE IT, and the
    /// reason is structural rather than incidental: `import_pinned`
    /// SEEDS the mirror from the capture's own `mobilize` lane, so a
    /// pair imported at the stamp tick is handed retail's latched
    /// value and both readings commit on time. Only a free run, which
    /// has to produce the latch itself, separates them. (39's tell,
    /// inverted: clean in pair mode, dirty free-run.)
    ///
    /// Non-vacuous: drop the mirror write and the first assert reads 0.
    #[test]
    fn the_paralyze_stamp_publishes_the_latch_at_the_stamp() {
        let mut g = mc2_gen();
        let stamp = g.new_event().unwrap();
        {
            let e = &mut g.ent[stamp];
            e.class64 = 10;
            e.model65 = 66;
            e.tick70 = 71; // the PARALYZE variant (action 0x47)
            e.f146 = crate::mc1::mobs::PLAYER_TARGET;
            e.x = 0x4000;
            e.y = 0x4000;
            e.act_life = 1;
        }
        let ctx = ctx_at(&mut g, 0x4000, 0x4000, 0);
        assert_eq!(g.mc2_mobilize[Hx::FIRST].0, 0, "no latch before the stamp");

        g.mc2_debuff_stamp_tick(stamp, &ctx);

        assert_eq!(
            g.mc2_mobilize[Hx::FIRST].0,
            1,
            "the stamp itself latches the mirror — a creature dispatching \
             later in this same walk must read it, as it does off retail's \
             single str_164 storage"
        );
        // The ext still gets its hit through the queue: the mirror is a
        // read window, not a second owner of the counter/decay.
        assert_eq!(
            g.mc2_debuffs[Hx::FIRST].stun,
            1,
            "and the drain still arms the flight ext's own counter"
        );
    }

    /// ⭐⭐⭐ **THE STAGGER STAMP ONLY BITES A WIZARD WHO IS NOT
    /// ALREADY SLOWED.** `sub_38E70` (action 0x46, the (10,65)) is the
    /// paralyze stamp with `moveSpeed_0x14C_332` in place of
    /// `mobilizeCounter_0x14E_334`, and it splits the arm in two
    /// (EF:28404-21; shipped NETHERW.EXE `0x5d6ae 8a b0 4c 01 00 00`
    /// read, `0x5d6b4 80 fe 03` / `0x5d6b7 0f 83` the `< 3` gate,
    /// `0x5d6bd 84 f6` / `0x5d6bf 75 35` the `== 0` gate, `0x5d6c1`
    /// the −80, `0x5d6c7`/`0x5d6cd` the `9377x+9439` step, `0x5d6fc`
    /// the `++`, `0x5d74d` the unconditional counter refresh): the
    /// kick, the LCG draw and the 54..57 grunt fire ONLY on the
    /// level-0 → 1 transition.
    ///
    /// The port drew and kicked on every landing. mc2l24 caught it
    /// five times — t=10775 slot 580, 10898 slot 500, 11526 and 11531
    /// slot 336, 13759 slot 491 — each one a `rand` row where the port
    /// stood exactly one draw ahead (`f(25861) = 24836`,
    /// `f(45595) = 62426`, `f(8003) = 14850`, `f(9406) = 63581`,
    /// `f(11470) = 19053`), and each one a tick the capture shows the
    /// human already webbed (`move_speed 1 -> 2`, and 2 -> 3 at
    /// 11531) with `knock_mag` still decaying +4/tick, never re-armed.
    ///
    /// Non-vacuous: `MGC_NO_MC2_STAGGER_LEVEL_GATE` (or dropping the
    /// gate) makes the second stamp step the stream again and the
    /// first assert fails.
    #[test]
    fn the_stagger_stamp_bites_only_at_slow_level_zero() {
        let mut g = mc2_gen();
        let mk = |g: &mut Gen| {
            let stamp = g.new_event().unwrap();
            let e = &mut g.ent[stamp];
            e.class64 = 10;
            e.model65 = 65;
            e.tick70 = 70; // the STAGGER variant (action 0x46)
            e.f146 = crate::mc1::mobs::PLAYER_TARGET;
            e.rand = 45595;
            e.x = 0x4000;
            e.y = 0x4000;
            e.act_life = 1;
            stamp
        };
        let ctx = ctx_at(&mut g, 0x4000, 0x4000, 0);

        // Level 0: the whole arm runs — one draw, and the mirror steps.
        let a = mk(&mut g);
        assert_eq!(g.mc2_slow[Hx::FIRST].0, 0, "an unwebbed wizard");
        g.mc2_debuff_stamp_tick(a, &ctx);
        // ⚠ `mc2_gen()` builds on `ChassisParams::MC1`, whose
        // `ent_rand_width` is `U32` — so the harness steps the
        // UNMASKED `9377x+9439` here and the exact 16-bit landing
        // (`f(45595) = 62426`) is the mc2l24 fixture's job. What this
        // test owns is WHETHER the stream steps at all.
        assert_ne!(
            g.ent[a].rand, 45595,
            "the level-0 landing takes its 9377x+9439 grunt draw"
        );
        assert_eq!(
            g.mc2_slow[Hx::FIRST].0,
            1,
            "…and steps the level the next stamp reads"
        );
        assert_eq!(
            g.mc2_debuffs[Hx::FIRST].slow,
            1,
            "the ext's own hit is still queued"
        );

        // Level 1: retail's `test dh,dh / jne` skips the kick, the draw
        // and the sound — only the level and the 8-tick counter move.
        let b = mk(&mut g);
        g.mc2_debuff_stamp_tick(b, &ctx);
        assert_eq!(
            g.ent[b].rand, 45595,
            "a stamp on an ALREADY-slowed wizard steps NOTHING"
        );
        assert_eq!(g.mc2_slow[Hx::FIRST].0, 2, "but the level still climbs");

        // …and saturates at 3 (`if (moveSpeed < 3u)`, 0x5d6b4), so a
        // fourth landing leaves the mirror alone and still draws nothing.
        let c = mk(&mut g);
        g.mc2_debuff_stamp_tick(c, &ctx);
        assert_eq!(g.mc2_slow[Hx::FIRST].0, 3, "level 3 is the ceiling");
        let d = mk(&mut g);
        g.mc2_debuff_stamp_tick(d, &ctx);
        assert_eq!(g.mc2_slow[Hx::FIRST].0, 3, "…and it holds there");
        assert_eq!(g.ent[d].rand, 45595, "no draw at the ceiling either");
        assert_eq!(
            g.mc2_debuffs[Hx::FIRST].slow,
            4,
            "the counter refresh (`moveSpeedCounter = 8`, 0x5d74d) is \
             OUTSIDE the `< 3` gate, so every landing still queues a hit"
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
        let ctx = ctx_at(&mut g, 0x1000, 0x1000, 300);
        g.m25_tick(i, &ctx);
        assert_eq!(g.ent[i].tick70, 205, "the split advanced past itself");
        assert!(
            g.ent
                .iter()
                .any(|e| e.class64 == 10 && e.model65 == 1 && e.flags & 0x400 == 0),
            "the (10,1) burst fired on the exhaustion path"
        );
    }

    /// ⭐⭐⭐ THE SUMMON RING IS A RING OF **NODES**, NOT OF CREATURES.
    /// A firefly (model 19) cast raises EIGHT `(10,72)` ring nodes
    /// (`sub_51800` EF:37459 — action `0x4F`, life = maxLife = 16, the
    /// collide bit cleared, every node a `qmemcpy` of the head so they
    /// all share ITS entity RNG seed); only when a node's own life
    /// counts down to zero does `sub_3A5B0` (EF:29590) hatch the
    /// class-5 creature with the allied StageVar2 = 13 marker, the
    /// `8·M+7` action, the caster's id and the 250-tick lease.
    #[test]
    fn summon_army_ring_is_eight_delayed_nodes() {
        let mut g = mc2_gen();
        g.mc2_spawn_summon_ring(0x4000, 0x4000, 19, 0x77);
        let nodes: Vec<usize> = g
            .ent
            .iter()
            .enumerate()
            .filter(|(_, e)| e.class64 == 10 && e.model65 == 72 && e.flags & 0x400 == 0)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(nodes.len(), 8, "firefly army ring size");
        let seed = g.ent[nodes[0]].rand;
        for &i in &nodes {
            let e = &g.ent[i];
            assert_eq!(e.tick70, 0x4F, "actionIndex = sub_3A5B0");
            assert_eq!(e.act_life, 16, "life = maxLife = 16");
            assert_eq!(e.max_life, 16);
            assert_eq!(e.flags & 8, 0, "byte[0] &= 0xF7 clears the collide bit");
            assert_eq!(e.f71, 19, "the creature model rides byte_0x46_70");
            assert_eq!(e.id24, 0x77, "allied to the caster");
            assert_eq!(e.rand, seed, "qmemcpy shares the HEAD's entity seed");
        }
        // Not one creature exists yet — that is the whole law.
        assert!(
            !g.ent.iter().any(|e| e.class64 == 5 && e.model65 == 19),
            "the creatures are 16 ticks away, not born with the ring"
        );
        for _ in 0..16 {
            for &i in &nodes {
                g.mc2_summon_node_tick(i);
            }
        }
        let hatched: Vec<&Ent> = g
            .ent
            .iter()
            .filter(|e| e.class64 == 5 && e.model65 == 19 && e.flags & 0x400 == 0)
            .collect();
        assert_eq!(hatched.len(), 8, "every node hatched its creature");
        for e in hatched {
            assert_eq!(e.id24, 0x77, "allied to the caster");
            assert_eq!(e.site_z, 13, "the summon-army StageVar2 marker");
            assert_eq!(e.tick70, 19u8.wrapping_mul(8).wrapping_add(7));
            assert_eq!(e.lease(), 250, "the 250-tick lifespan");
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
        let ctx = ctx_at(&mut g, 0xC000, 0xC000, 0);
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
        let ctx = ctx_at(&mut g, 0x4180, 0x4080, 0);
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

    /// ⭐⭐⭐ **THE ROW-0 SMOOTHER GATE READS BELOW THE TYPE PLANE, AND
    /// SHIM BYTE {227} IS BUILDING-CLASSED.** `sub_360C0` (:42912-19)
    /// forms its four-way quad index in SIGNED 32-bit arithmetic —
    /// `(u16)a1 - 257` / `- 256` / `- 1`, no wrap — so for a ROW-0
    /// cell two of the three reads land 257 and 256 bytes BELOW
    /// `mapTerrainType`, in the sound-driver globals. The shipped
    /// binaries carry the three displacements verbatim:
    /// `HIDDEN.EXE` file 0x4eea9 `8a 93 cf c0 03 00`, 0x4eec2
    /// `8a 93 d0 c0 03 00`, 0x4eedb `8a 93 cf c1 03 00` (bases −257 /
    /// −256 / −1 off the plane base), and `CARPET.EXE` the same
    /// stream at 0x4e8e9 / 0x4e902 / 0x4e91b with its base 16 bytes
    /// higher. Only the GATE escapes: the 3x3 SUM loop casts per
    /// access (:42928) and wraps into row 255 correctly.
    ///
    /// The rig is the recorded row-0 neighbourhood at x=225..229
    /// (rows 255/0/1) that pinned [`Gen::OOB_TYPE_SHIM`]`[227]`.
    /// A cell (x,0) reads shim `x` and `x + 1`, so (226,0) and
    /// (227,0) BOTH read {227} and retail skips them, while (228,0)
    /// and (229,0) see only plain shim bytes and smooth to their
    /// exact 3x3 averages (304/9 and 295/9).
    ///
    /// WHAT WOULD BREAK IT: dropping {227} back to plain (the pre-dig
    /// table, restored process-wide by `MGC_NO_MC1_ROW0_SHIM_227=1`)
    /// smooths the first two cells to 31 and 33 and leaves the last
    /// two at 33 and 32 — so the four values are asserted TOGETHER,
    /// because the `OnceLock` switch cannot be flipped inside one
    /// test. No plain-{227} implementation can produce (30, 37), and
    /// no "row 0 never smooths" implementation can produce (33, 32).
    #[test]
    fn the_row_0_smoother_gate_reads_shim_byte_227_as_building() {
        // Uniform plain land at height 30 — the untouched ground the
        // recorded neighbourhood sits in (x=230's column feeds
        // (229,0)'s average).
        let mut planes = Planes {
            height: vec![30; GRID],
            tile_type: vec![1; GRID],
            shading: vec![32; GRID],
            angle: vec![1; GRID],
            ceiling: Vec::new(),
        };
        // Rows 255 / 0 / 1 at x=225..229, as recorded.
        let rows: [(u8, [u8; 5]); 3] = [
            (255, [30, 33, 34, 33, 31]),
            (0, [30, 30, 37, 31, 33]),
            (1, [30, 30, 30, 45, 30]),
        ];
        let types: [u8; 5] = [77, 1, 1, 1, 1];
        for (y, hs) in rows {
            for (k, x) in (225u8..=229).enumerate() {
                planes.height[tile(x, y)] = hs[k];
                planes.tile_type[tile(x, y)] = types[k];
                planes.angle[tile(x, y)] = 1;
            }
        }
        let mut g = Gen::new(
            planes,
            synthetic_assets(),
            0,
            ChassisParams::MC1,
            VerbSet::MC1,
        );
        // The perimeter walk's row-0 order.
        for x in 226u8..=229 {
            g.smooth_cell(tile(x, 0));
        }
        let h = |x: u8| g.t.height[tile(x, 0)];
        assert_eq!(
            (h(226), h(227), h(228), h(229)),
            (30, 37, 33, 32),
            "(226,0) and (227,0) read shim {{227}} = building and hold \
             their pre-smoother heights; (228,0) and (229,0) read only \
             plain shim bytes and take the 3x3 averages 304/9 and 295/9"
        );
    }

    /// **THE STALE RECYCLE VICTIM** (`mc1_recycle_victim_revalidate`,
    /// mc1l26 t=27343-44). MC1's recycle stack names SLOT NUMBERS
    /// armed at a death landing and is never purged on free, so a slot
    /// that was a sacrificable puff then and is a castle worker now is
    /// still listed — and retail's dry-pool seizure eats the worker.
    /// Retail arm: the live non-victim is seized (the bare `class != 0`
    /// test). Patched: it is skipped and the pop moves on to the next
    /// listed slot that still carries the mask.
    #[test]
    fn the_dry_pool_seizure_revalidates_a_stale_recycle_victim() {
        let run = |revalidate: bool| {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                VerbSet::MC1,
            );
            // Two expendable puffs, then a landing arms the stack.
            let puff_a = g.new_event().unwrap();
            let puff_b = g.new_event().unwrap();
            for &p in &[puff_a, puff_b] {
                let e = &mut g.ent[p];
                e.class64 = 10;
                e.model65 = 13;
                e.flags |= 0x20000;
            }
            g.rebuild_recycle(g.victim_mask());
            assert!(g.mc2_recycle.stack.contains(&(puff_a as u16)));
            assert!(g.mc2_recycle.stack.contains(&(puff_b as u16)));
            // puff_a dies, is freed, and its slot is re-minted as a
            // castle ground-leveler (no sacrificable bit). The stack
            // still names the slot.
            g.free_entity(puff_a);
            assert!(
                g.mc2_recycle.stack.contains(&(puff_a as u16)),
                "MC1 never purges on free"
            );
            // The freed slot is the free stack's top: it is re-popped
            // first and becomes the leveler. Then occupy every other
            // free slot by hand (`new_event` would run the seizure
            // itself once the stack is dry).
            let leveler = g.free.pop().unwrap() as usize;
            assert_eq!(leveler, puff_a, "the freed slot is the next pop");
            {
                let e = &mut g.ent[leveler];
                e.class64 = 10;
                e.model65 = 41;
                e.flags = 2;
            }
            while let Some(s) = g.free.pop() {
                g.ent[s as usize].class64 = 5;
            }
            assert!(g.free.is_empty(), "the pool really is exhausted");
            g.mc2_recycle.revalidate = revalidate;
            // The next spawn must come from the recycle stack.
            let seized = g.new_event().expect("a victim was sacrificed");
            (
                (g.ent[leveler].class64, g.ent[leveler].model65),
                seized,
                leveler,
                puff_b,
                g.mc2_recycle.skipped_stale,
            )
        };
        // Retail: the stack pops its LOWEST slot first — puff_a's, the
        // first allocation — and the bare seizure wipes the (10,41)
        // now living there.
        let (cm, seized, leveler, _, stale) = run(false);
        assert_eq!(
            seized, leveler,
            "retail's seizure lands on the re-minted slot"
        );
        assert_ne!(cm, (10, 41), "…and the live leveler is gone (record wiped)");
        assert_eq!(stale, 0);
        // Patched: the leveler's slot no longer carries 0x20400 and is
        // skipped; the seizure moves on to puff_b, which still does.
        let (cm, seized, _, puff_b, stale) = run(true);
        assert_eq!(cm, (10, 41), "the leveler survives the dry pool");
        assert_eq!(seized, puff_b, "the seizure moved on to the live victim");
        assert_eq!(stale, 1);
    }

    /// **THE CRUSHED CONSTRUCTION SITE** (`mc1_crushed_site_collapse`,
    /// `bug.mgcr` site 673 / retail `mc1l13` slot 140). A castle
    /// pre-clear stamps life -1 on a (10,45) still in state 51. Retail
    /// (`sub_27D30` exits only on `--life == 0`): the site stays in
    /// construction forever and every tick divides its goal step by the
    /// negative life, driving the floor cells AWAY from the goal and
    /// through the byte wrap. Patched: the crushed site takes the
    /// finished house's crushed arm, state 53, on its next tick.
    #[test]
    fn a_crushed_construction_site_collapses_only_under_the_patch() {
        let run = |patched: bool| {
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
                e.class64 = 10;
                e.model65 = 45;
                e.tick70 = 51;
                e.f71 = 5; // a 4x4 row: floor code 7 inside a 0x10 ring
                e.act_life = 30;
                e.x = 0x4000;
                e.y = 0x4000;
                e.z = 32 * 20; // goal height 20 over ground 8
            }
            for _ in 0..10 {
                g.tick_building(b, patched, false);
            }
            assert_eq!(g.ent[b].act_life, 20);
            g.ent[b].act_life = -1; // sub_12C50's stamp (file 0x2B4DF)
            for _ in 0..40 {
                if g.ent[b].tick70 != 51 {
                    break;
                }
                g.tick_building(b, patched, false);
            }
            (g.ent[b].tick70, g.ent[b].act_life, g.t.height[tile(63, 63)])
        };
        // Retail: still building 40 ticks later, life run down past
        // zero, the floor cell thrown far off both ground (8) and goal
        // (20) — the first reversed step alone wraps it (2h - goal).
        let (state, life, h) = run(false);
        assert_eq!(state, 51, "retail never leaves construction");
        assert_eq!(life, -41);
        assert!(
            !(8..=20).contains(&h),
            "floor cell pushed off its goal: {h}"
        );
        // Patched: collapse on the first tick after the stamp, with the
        // footprint left where the countdown had it.
        let (state, life, h) = run(true);
        assert_eq!(state, 53, "the crushed site collapses");
        assert_eq!(life, -1);
        assert!(
            (8..=20).contains(&h),
            "floor cell untouched by the crush: {h}"
        );
    }

    /// A build table whose rows 1.. are one 3x3 footprint: floor code
    /// 7 (goal = datum) around a centre `0x4D` (hi 4, lo 13: pad
    /// `4*(13-1)` = 48 under both the construction and the painter
    /// decode — the tallest pad of the shipped MC1 castle rows). Row 0
    /// is empty, as shipped.
    fn pad48_assets() -> FeatureAssets {
        let mut grid = vec![31u8; 1024];
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (dx, dy) = (x - 15, y - 15);
                let r = dx.max(dy).max(-dx + 1).max(-dy + 1) - 1;
                grid[(y * 32 + x) as usize] = r.clamp(0, 31) as u8;
            }
        }
        let dat: Vec<u8> = vec![3, 7, 7, 7, 0, 3, 7, 0x4D, 7, 0, 3, 7, 7, 7, 0];
        let tab: Vec<u8> = (0..24u32)
            .flat_map(|r| {
                let mut e = 0u32.to_le_bytes().to_vec();
                let d = if r == 0 { 0 } else { 3 };
                e.push(d);
                e.push(d);
                e
            })
            .collect();
        FeatureAssets::parse(&grid, &tab, &dat).unwrap()
    }

    /// **THE OVERFLOWING BUILDING PAD, MC1 DWELLING**
    /// (`mc1_building_pad_saturate`). `sub_27D30` steps each cell
    /// toward the ABSOLUTE `datum + pad` and stores a byte (`CARPET.EXE`
    /// file 0x40669 `add` / 0x407AA `mov %al,0x4c1e0(%ecx)`): a dwelling
    /// on ground 240 with a pad-48 cell (goal 288) finishes that cell
    /// at 288 & 0xFF = 32 — a pit in its own roof. Patched: 255.
    #[test]
    fn an_mc1_dwelling_pad_wraps_in_retail_and_saturates_patched() {
        let run = |patched: bool| {
            let mut g = Gen::new(
                flat_land(240),
                pad48_assets(),
                1,
                ChassisParams::MC1,
                VerbSet::MC1,
            );
            let b = g.new_event().unwrap();
            {
                let e = &mut g.ent[b];
                e.class64 = 10;
                e.model65 = 45;
                e.tick70 = 51;
                e.f71 = 1;
                e.act_life = 20;
                e.x = 0x4000;
                e.y = 0x4000;
                e.z = 32 * 240;
            }
            for _ in 0..40 {
                if g.ent[b].tick70 != 51 {
                    break;
                }
                g.tick_building(b, false, patched);
            }
            assert_eq!(g.ent[b].tick70, 52, "the site finished");
            (g.t.height[tile(64, 64)], g.t.height[tile(63, 64)])
        };
        assert_eq!(
            run(false),
            ((288 & 0xFF) as u8, 240),
            "retail: the pad-48 cell wraps"
        );
        assert_eq!(
            run(true),
            (255, 240),
            "patched: it tops out, the floor is untouched"
        );
    }

    /// **THE OVERFLOWING BUILDING PAD, MC1 CASTLE**
    /// (`mc1_building_pad_saturate`, retail witness
    /// `recordings/mc1l32-new.mgcr` painter slot 39, t=6321-6352). The
    /// painter `sub_285C0` wraps a pad-48 cell over a datum above 207
    /// (file 0x410F9-0x41117 goal fill, 0x412AF byte `add`), and the
    /// leveler `sub_28200` then translates the rect with an 8-bit add
    /// (file 0x40CBF), so the wrap HEALS when the leveler's target is
    /// <= 207 and PERSISTS when it is higher. Patched (datum cap
    /// `255 − 48 = 207` on the painter and the leveler): no wrap ever,
    /// the SAME final heights as retail wherever retail heals, and a
    /// castle sat lower instead of a pit where retail does not.
    #[test]
    fn an_mc1_castle_pad_wrap_heals_like_retail_or_sits_lower_patched() {
        // (outside ground, patched) -> (final height plane, the centre
        // cell's lowest value while the painter ran)
        let run = |outside: u8, patched: bool| {
            let mut g = Gen::new(
                flat_land(outside),
                pad48_assets(),
                1,
                ChassisParams::MC1,
                VerbSet::MC1,
            );
            for y in 63..=65 {
                for x in 63..=65 {
                    g.t.height[tile(x, y)] = 232;
                }
            }
            let worker = |g: &mut Gen, model: u8, act: u8| {
                let i = g.new_event().unwrap();
                let e = &mut g.ent[i];
                e.class64 = 10;
                e.model65 = model;
                e.tick70 = act;
                e.f71 = 1;
                e.x = 0x4000;
                e.y = 0x4000;
                e.z = 32 * 232;
                i
            };
            let p = worker(&mut g, 42, 44);
            let mut low = 255u8;
            for _ in 0..80 {
                if g.ent[p].flags & 0x400 != 0 {
                    break;
                }
                g.tick_castle_painter_with(p, patched);
                low = low.min(g.t.height[tile(64, 64)]);
            }
            assert!(g.ent[p].flags & 0x400 != 0, "fixture: the painter finished");
            let l = worker(&mut g, 41, 43);
            for _ in 0..80 {
                if g.ent[l].flags & 0x400 != 0 {
                    break;
                }
                g.tick_castle_leveler_with(l, patched);
            }
            assert!(g.ent[l].flags & 0x400 != 0, "fixture: the leveler finished");
            (g.t.height.clone(), low)
        };
        // Heals: outside ground 173 (mc1l32-new's leveler target).
        let (retail, low) = run(173, false);
        assert_eq!(
            retail[tile(64, 64)],
            221,
            "retail: 280 wraps to 24, the leveler's -59 lands on 221"
        );
        assert!(
            low < 64,
            "retail: the centre dropped into a pit mid-paint ({low})"
        );
        let (patched, low) = run(173, true);
        assert!(low >= 232, "patched: the centre never wraps ({low})");
        assert_eq!(
            patched, retail,
            "patched: the same final heights as retail's healed wrap"
        );
        // Persists: outside ground 232 — the leveler's target clamps to
        // 220 (file 0x40B40), so it lowers the rect by only 12.
        let (retail, _) = run(232, false);
        assert_eq!(
            retail[tile(64, 64)],
            ((280 - 12) & 0xFF) as u8,
            "retail: the pit stays"
        );
        let (patched, _) = run(232, true);
        assert_eq!(patched[tile(64, 64)], 255, "patched: the tower tops out");
        let yard = patched[tile(63, 64)];
        assert!(
            (207..232).contains(&yard),
            "patched: the courtyard sits lower (cap 207, edge-smoothed): {yard}"
        );
    }

    /// **THE ORPHANED TRANSFORM WAIT** (`mc1_castle_transform_watchdog`,
    /// mc1l26 slot 623 from t=27345). A castle in the leveler wait
    /// (`+70`=5, `+48`=6) whose (10,41) no longer exists at its site
    /// waits forever in the retail arm — its damage mail unread, no
    /// upgrade, no fleet pass. Patched: it takes the leveler's own
    /// shake exit (sub-state 2) and the next dispatch settles it. With
    /// a worker standing at the site NEITHER arm moves it — the
    /// healthy transformation keeps its full length.
    #[test]
    fn an_orphaned_castle_wait_settles_only_under_the_watchdog() {
        let run = |worker: bool, patched: bool| {
            let mut g = Gen::new(
                flat_land(8),
                synthetic_assets(),
                1,
                ChassisParams::MC1,
                VerbSet::MC1,
            );
            let (x, y) = (0x8000u16, 0x8000u16);
            let c = g.new_event().unwrap();
            let site_z = g.ground_z(x, y) as i16;
            {
                let e = &mut g.ent[c];
                e.class64 = 3;
                e.model65 = 2;
                e.x = x;
                e.y = y;
                e.z = site_z;
                e.site_z = site_z;
                e.f26 = 1;
                e.max_life = Gen::CASTLE_HP[1];
                e.act_life = 10000;
                e.f136 = Gen::CASTLE_CAP[1];
                e.tick70 = 5;
                e.f59 = 6; // waiting on the leveler
                e.mail[0] = (200_000, 7); // the player's banked damage
            }
            if worker {
                let w = g.new_event().unwrap();
                let e = &mut g.ent[w];
                e.class64 = 10;
                e.model65 = 41;
                e.x = x;
                e.y = y;
                e.z = site_z;
                e.flags = 2;
                e.tick70 = 43;
            }
            let patches = crate::patches::WorldPatches {
                mc1_castle_transform_watchdog: patched,
                ..crate::patches::WorldPatches::RETAIL
            };
            g.castle_tick(c, patches);
            let after_one = (g.ent[c].tick70, g.ent[c].f59);
            g.castle_tick(c, patches);
            let after_two = (g.ent[c].tick70, g.ent[c].f59);
            (after_one, after_two, g.castle_watchdog_fired.0.0)
        };
        // A worker at the site: the wait holds in both arms and the
        // predicate never counts.
        assert_eq!(run(true, false), ((5, 6), (5, 6), 0));
        assert_eq!(run(true, true), ((5, 6), (5, 6), 0));
        // No worker, retail: the wait holds forever; the predicate
        // counts every tick.
        assert_eq!(run(false, false), ((5, 6), (5, 6), 2));
        // No worker, patched: the finish exit (sub-state 2) is taken
        // and consumed by the same dispatch — SETTLED at once, as when
        // retail's leveler finishes at a slot below its castle's — and
        // the settled tick then reads the banked mail as usual.
        let (one, two, fired) = run(false, true);
        assert_eq!(
            one,
            (4, 0),
            "the orphaned wait takes the finish exit and settles"
        );
        assert_eq!(
            two,
            (6, 0),
            "…and the settled tick reads the banked lethal mail"
        );
        assert_eq!(fired, 1);
    }
}
