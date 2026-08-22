//! The MC2 (Magic Carpet 2: The Netherworlds) simulation column —
//! tier-2 tables, tier-4 handlers, and tier-5 verb arms behind
//! [`crate::verbs`]. Everything here is a verbatim port of remc2
//! machinery; shared chassis stays in [`crate::engine::features::Gen`]
//! (same pool, LCG, mailboxes, tile chains).
//!
//! Data provenance: `behavior.rs` + `sprite_params.rs` are generated
//! by `tools/extract-remc2-tables.py` from the vendored remc2
//! decompilation; the per-level building parameters (`bldgprm.bin`),
//! ring table (`search.bin`) and spell table (`spells.bin`) ride the
//! mc2-* asset bundles.

pub mod behavior;
pub mod cast;
pub(crate) mod castle;
pub(crate) mod cave;
pub(crate) mod doomsday;
pub(crate) mod effects;
pub(crate) mod flood;
pub(crate) mod mobs;
pub(crate) mod morph;
pub(crate) mod multipart;
pub(crate) mod pads;
pub(crate) mod probes;
pub(crate) mod proj;
pub(crate) mod riser;
pub mod rivals;
pub(crate) mod roster;
pub(crate) mod scenery;
pub mod sin_lut;
pub mod spells;
pub mod sprite_params;
pub(crate) mod stagevars;
pub(crate) mod tail;
pub mod terrain_paint;
pub(crate) mod tokens;

/// `TransformPlayerColorIndex_616D0` (remc2 GameUI.cpp:869), the
/// single-player branch (:908-936): the permutation between a player's
/// COLOR SLOT (the `playersColors_E88E0x` table row — map dots,
/// nameplates, bars) and the index of his ART FAMILY (the pre-colored
/// sprite bands: mana spheres 105+8k, castle/dwelling flags 177+k,
/// balloons 169+k, carpets 272+k for k>=1 (see
/// [`carpet_sprite_row`] — k=0 is NOT 272), minimap castle/balloon
/// stamps 58+k/66+k). The art was authored in THIS order — verified
/// empirically against the baked atlases: art k=2 is Jark's plum,
/// k=4 is Rahn's green (exact table RGB matches), k=6/7 swap
/// Prish/Yragore. Indexing art by the RAW slot hands Rahn purple
/// spheres while his map dot stays green.
///
/// (The multiplayer branch is a different permutation with a genuine
/// retail bug — `case 7` mapped two teams onto art slot 7 and
/// orphaned slot 6; remc2 patches it with `index2 = 6 //fix`. We ship
/// the single-player branch: campaign rivals only.)
pub const COLOR_ART: [u8; 8] = [0, 1, 4, 3, 2, 5, 7, 6];

/// Art-family index for a color slot (see [`COLOR_ART`]); slots
/// beyond 7 clamp to the identity, matching retail's `default` arm.
pub fn color_art(slot: u8) -> u8 {
    COLOR_ART.get(slot as usize).copied().unwrap_or(slot)
}

/// The wizard-carpet sprite-param row for a player's COLOR SLOT —
/// `AddPlayer_4A920`'s switch on [`color_art`] (remc2 EF:43732-59).
///
/// The family is a SWITCH, not a base+offset: art index 0 takes row
/// **44**, and only 1..7 run 273..279. Row 272 belongs to the
/// `(10,38)` lightning-storm cloud ([`Gen::mc2_spawn_lightning_burst`],
/// sprite 202) — extrapolating "272 + k" onto k=0 draws a fat
/// translucent ball where the wizard on his carpet belongs (it did,
/// on the replay ghost: player-reported 2026-08-25).
///
/// Corroborated against the captures, not just the decompile: the
/// human carpet's recorded `f5a` (+0x5A, the live sprite-param row)
/// reads 44 in every state record of mc2l3 / mc2l0 /
/// mc2l0-spells-galore — 83,244 records, no variation, deaths and
/// respawns included.
pub fn carpet_sprite_row(slot: u8) -> u16 {
    match color_art(slot) {
        0 => 44,
        k => 272 + k as u16,
    }
}

/// The retail sprite-extents derivation (`sub_718.. init pass`,
/// remc2 EF:44870-44910): the shipped particle-param table stores
/// only ONE of the (speed_6, rotSpeed_8) pair per row — at load the
/// engine decompresses each row's sprite and derives the other from
/// the bitmap aspect: `speed_6 = width·rotSpeed_8/height` (or the
/// transpose). This is why the static table's speed_6 column is zero
/// almost everywhere, and why zero-box projectiles could never
/// collide. `dims[sprite_id]` = (width, height) from the baked sprite
/// index;
/// missing/zero dims take retail's 255×255 fallback. Returns the
/// per-row derived (speed_6, rot_speed_8).
pub fn derive_sprite_extents(dims: &[(u16, u16)]) -> Vec<(u16, u16)> {
    sprite_params::SPRITE_PARAMS
        .iter()
        .map(|p| {
            let (mut w, mut h) = dims.get(p.word_0 as usize).copied().unwrap_or((0, 0));
            if w == 0 || h == 0 {
                (w, h) = (255, 255);
            }
            let (mut s6, mut r8) = (p.speed_6 as u32, p.rot_speed_8 as u32);
            if s6 != 0 {
                if r8 == 0 {
                    r8 = h as u32 * s6 / w as u32;
                }
            } else {
                s6 = w as u32 * r8 / h as u32;
            }
            (
                s6.min(u16::MAX as u32) as u16,
                r8.min(u16::MAX as u32) as u16,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::behavior::{BEHAVIOR, Mc2BehaviorRow, ROW_BASE};

    /// Cross-engine anchor: MC2's model-0 row (array index 59, the
    /// engine's base pointer) is byte-identical to MC1's BEHAVIOR[0].
    #[test]
    fn mc2_model0_row_matches_mc1_row0() {
        let m2 = &BEHAVIOR[ROW_BASE];
        let m1 = &crate::mc1::behavior::BEHAVIOR[0];
        assert_eq!(
            (
                m2.v_0, m2.v_2, m2.v_4, m2.v_6, m2.v_8, m2.v_10, m2.v_12, m2.v_14
            ),
            (
                m1.v_0, m1.v_2, m1.v_4, m1.v_6, m1.v_8, m1.v_10, m1.v_12, m1.v_14
            )
        );
        assert_eq!(
            (
                m2.v_16, m2.v_18, m2.v_20, m2.v_22, m2.v_26, m2.v_28, m2.v_30
            ),
            (
                m1.v_16, m1.v_18, m1.v_20, m1.v_22, m1.v_26, m1.v_28, m1.v_30
            )
        );
        assert_eq!(m2.flags, 0);
    }

    /// The slice creatures' hand-picked rows (ctors assign ABSOLUTE
    /// indices — remc2 :33739/:33899/:34058) and their flag bytes:
    /// Goat + Villager flee (bit 8) and die on water (bit 1);
    /// Archers only die on water. Nobody disables the pack scan.
    #[test]
    fn slice_rows_and_flags() {
        let goat = &BEHAVIOR[98];
        let archers = &BEHAVIOR[75];
        let villager = &BEHAVIOR[100];
        assert_eq!((goat.v_0, goat.flags), (0x27, 0x09));
        assert_eq!((archers.v_0, archers.flags), (0x10, 0x01));
        assert_eq!((villager.v_0, villager.flags), (0x29, 0x09));
        assert_eq!(
            goat.flags & Mc2BehaviorRow::FLEE,
            Mc2BehaviorRow::FLEE,
            "goat flees"
        );
        assert_eq!(archers.flags & Mc2BehaviorRow::FLEE, 0, "archers hold");
        for r in [goat, archers, villager] {
            assert_eq!(
                r.flags & Mc2BehaviorRow::PACK_DISABLE,
                0,
                "no slice model disables the pack scan"
            );
        }
    }
}
