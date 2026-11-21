//! MC1 player spell definitions: the 24-spell stat table and its
//! surrounding constants, ported from the remc1 decompilation.
//!
//! There is no flat stats array in the original — each spell's numbers
//! are literal arguments to the shared manifestation constructor
//! `sub_3BF70` (sub_main.cpp:47981) inside 24 spawn thunks
//! `sub_3C040..sub_3C480` (:48020-48161) dispatched via `off_987DE[]`
//! (:5167). Rows here transcribe those calls verbatim.
//!
//! A player's owned spell is a live class-12 "manifestation" ENTITY in
//! the world pool (the original's slot economy applies — spell
//! manifestations compete with monsters for slots), carrying its own
//! mana pool (`+140` current / `+136` max), per-tick recharge
//! (`+132`), spell level (`+26`), and burst counter (`+48`).
//!
//! Spell identities derive from book-order naming pushed through the
//! display permutation [`DISPLAY_ORDER`] (`byte_99B88`, :5752). MC1
//! shows no spell names in-game; [`SpellId::name`] labels are ours
//! (MC2's data names spells explicitly — reconcile when its track
//! lands).

/// Internal spell type (entity `+65` on a manifestation; 0..24). The
/// spellbook DISPLAYS spells permuted by [`DISPLAY_ORDER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpellId(pub u8);

pub const SPELL_COUNT: usize = 24;

impl SpellId {
    /// The base-MC1 manual name. Use [`Self::name_in`] wherever the
    /// game is known — Hidden Worlds renames one spell.
    pub fn name(self) -> &'static str {
        NAMES[self.0 as usize]
    }

    /// The spell's name in `game`: Hidden Worlds' 20 is the HOMING
    /// METEOR (its one diverging row, [`SPELLS_HW`]), base MC1's Wall
    /// of Fire; everything else is shared.
    pub fn name_in(self, game: crate::ids::GameId) -> &'static str {
        match (game, self.0) {
            (crate::ids::GameId::Mc1Hw, 20) => "Homing Meteor",
            _ => self.name(),
        }
    }
}

/// Official manual names. Notables: 11 "Duel to the Death" = the
/// tether; 20 "Wall of Fire" = fire storm; 22 "Global Death" = magic
/// bomb — the manual name oversells the sub-tile blast radius, but the
/// "shockwave fatal to anything within its influence" bit is real.
const NAMES: [&str; SPELL_COUNT] = [
    "Fireball",             // 0
    "Heal",                 // 1
    "Accelerate",           // 2  (forward; down-cursor cancels)
    "Possess",              // 3  (claim buildings/mana)
    "Shield",               // 4  (absorbs 3/4 of spell energy)
    "Beyond Sight",         // 5
    "Earthquake",           // 6
    "Meteor",               // 7
    "Volcano",              // 8  (periodic re-eruptions)
    "Crater",               // 9
    "Teleport",             // 10 (to castle / back to cast site)
    "Duel to the Death",    // 11 (locks two players; Accelerate escapes)
    "Invisible",            // 12 (casting breaks the cloak)
    "Steal Mana",           // 13
    "Rebound",              // 14 (deflects incoming fire spells)
    "Lightning Bolt",       // 15 (hold: stream locks onto a target)
    "Create Castle",        // 16 (launches a mana balloon per cast)
    "Undead Army",          // 17 (red-cloaked skeletons)
    "Lightning Storm",      // 18 (radiates in all directions)
    "Mana Magnet",          // 19
    "Wall of Fire",         // 20
    "Accelerate Backwards", // 21
    "Global Death",         // 22 (point-blank one-shot shockwave)
    "Rapid Fireball",       // 23
];

/// One spell's constructor arguments (sub_3BF70 a2..a9 in call order;
/// a2 is the spell id itself and a3 = 3 * id always, so both are
/// derived rather than stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellDef {
    /// a4 → `+136`: the spell's TOTAL MANA COST (`+140` is the
    /// PER-SHOT cost, `a4 / a5` — sub_3BF70 :48001-05 divides by the
    /// burst count before storing it) — gated
    /// against the wizard's current pool at full charge and debited
    /// through the regen delta (sub_55DD0 :64909 / sub_55E80 :64936;
    /// remc1 ships the debit commented out — a maintainer mis-fix).
    pub possess_mana: u32,
    /// a5 → `+50`: burst count — `+48` is set to this when the player
    /// fires, and the manifestation's tick consumes it. 251/101 =
    /// effectively-continuous channels.
    pub count: u16,
    /// a6 → `+60`: fire-mode flag (0 on the hold-to-charge spells;
    /// forced 0 when `charge` is set).
    pub fire_flag: bool,
    /// a7 → `+62`: continuous/charging spell flag.
    pub charge_flag: bool,
    /// a8 → `+132`: required CASTLE STORED MANA — the spell-unlock
    /// ladder (sub_55DD0 :64917-19: nonzero → the caster must own a
    /// castle holding at least this much). 0 = castle-free spell.
    /// Magic Bomb's row decompiles as `(int)&loc_30D40`; **the binary
    /// settles it at 200000** (see the table note below) — the
    /// long-standing 199488 was a mis-conversion of `0x30D40`
    /// (199488 is `0x30B40`).
    pub castle_req: u32,
    /// a9 → `+44`: damage/potency. Utility rows carry a vestigial 100
    /// here (shared-constructor filler; meaning unclear on those rows).
    pub damage: u32,
}

/// `off_987DE` thunk arguments, row = internal spell id. Quirk rows,
/// preserved verbatim pending the emission-behavior port:
/// - Magic Bomb's a8 decompiles as `(int)&loc_30D40`, a code address
///   frozen into a literal; treated as the constant. ⚠ **IT IS
///   200000, NOT 199488** — `0x30D40` = 200000 and the tree carried a
///   mis-converted `0x30B40` for a year. THE SHIPPED EXE SETTLES IT:
///   `CARPET.EXE` file offset **0x54C4D** holds `68 40 0D 03 00`
///   (`push 0x30D40`), and 199488 appears nowhere in the image.
///   mc1l49 corroborates from the other side — the human's Magic Bomb
///   token (pool slot 87) reads `+132 = 200000` on every tick of the
///   take. Corrected 2026-09-07 (wave 115 / D17); vacuous on the whole
///   corpus (457/457 fixtures and 16 MC1 takes byte-identical), it
///   only bites when a castle store sits in [199488, 200000).
/// - Fire Storm's damage 24464 is anomalously large — whatever it
///   does, it isn't a plain 24464-damage hit.
pub const SPELLS: [SpellDef; SPELL_COUNT] = [
    // 0 Fireball (sub_3C090 :48032)
    def(200, 5, true, false, 0, 125),
    // 1 Heal (sub_3C0F0 :48044)
    def(1000, 21, true, false, 0, 100),
    // 2 Accelerate (sub_3C0C0 :48038) — toggle pair with 21
    def(1000, 251, false, false, 0, 100),
    // 3 Claim Mana (sub_3C040 :48020)
    def(50, 3, true, false, 0, 100),
    // 4 Shield (sub_3C1B0 :48068)
    def(2000, 251, true, false, 0, 100),
    // 5 Beyond Sight (sub_3C330 :48116)
    def(3000, 101, true, false, 0, 100),
    // 6 Earthquake (sub_3C150 :48056)
    def(6000, 51, true, false, 120000, 6000),
    // 7 Meteor (sub_3C1E0 :48074)
    def(10000, 11, true, false, 100000, 10000),
    // 8 Volcano (sub_3C390 :48128)
    def(30000, 65, true, false, 180000, 1000),
    // 9 Crater (sub_3C300 :48110)
    def(12000, 31, true, false, 100000, 6000),
    // 10 Castle Portal (sub_3C120 :48050)
    def(5000, 51, true, false, 10000, 100),
    // 11 Tether (sub_3C270 :48092)
    def(2500, 17, true, false, 16000, 100),
    // 12 Invisibility (sub_3C2D0 :48104)
    def(5000, 251, true, false, 50000, 100),
    // 13 Steal Mana (sub_3C2A0 :48098)
    def(500, 11, true, false, 20000, 100),
    // 14 Rebound (sub_3C210 :48080)
    def(1000, 101, true, false, 8000, 100),
    // 15 Lightning Bolt (sub_3C240 :48086)
    def(1000, 2, false, false, 25000, 500),
    // 16 Castle (sub_3C060 :48026) — one active build at a time
    def(1000, 101, true, false, 0, 10000),
    // 17 Undead Army (sub_3C3C0 :48134)
    def(13000, 13, true, false, 150000, 100),
    // 18 Lightning Storm (sub_3C360 :48122)
    def(20000, 33, true, false, 90000, 2000),
    // 19 Mana Magnet (sub_3C180 :48062)
    def(4000, 17, true, false, 10000, 100),
    // 20 Fire Storm (sub_3C3F0 :48140)
    def(5000, 51, true, false, 12000, 24464),
    // 21 Accelerate Backwards (sub_3C420 :48146) — toggle pair with 2
    def(1000, 251, false, false, 0, 100),
    // 22 Magic Bomb (sub_3C450 :48152) — a8 = `(int)&loc_30D40` = 200000
    def(75000, 101, true, false, 200000, 7000),
    // 23 Repeat Fireballs (sub_3C480 :48158) — the dev fireball donor
    def(600, 3, false, false, 50000, 50),
];

/// Hidden Worlds diverges from base MC1 in **exactly one** spell row:
/// Fire Storm (20) becomes the homing meteor. Its constructor call
/// `sub_3C2B0(a1,20,60,5000,26,1,0,60000,5000)` (remc1hw :44266) vs base
/// `sub_3BF70(a1,20,60,5000,51,1,0,12000,24464)` (remc1 :48142) — burst
/// count 51→26, castle_req 12000→60000, damage 24464→5000. Total mana
/// (5000) and the fire/charge flags are unchanged. See
/// docs/archive/SURVEY-MC1HW.md §3b. The homing targeting is a separate seam
/// (§3a, `proj_firewall_tick`), not a stat.
pub const SPELLS_HW: [SpellDef; SPELL_COUNT] = {
    let mut t = SPELLS;
    t[20] = def(5000, 26, true, false, 60000, 5000);
    t
};

/// The player-spell stat table for a game. Only `Mc1Hw` diverges (row
/// 20); `Mc1` and `Mc2` share the base table (MC2 carries its own spell
/// system elsewhere and never reads row 20 from here). Routing MC1-family
/// `SPELLS[id]` reads through this keeps MC1 goldens pinned by
/// construction.
pub fn spells(game: crate::ids::GameId) -> &'static [SpellDef; SPELL_COUNT] {
    match game {
        crate::ids::GameId::Mc1Hw => &SPELLS_HW,
        _ => &SPELLS,
    }
}

const fn def(
    possess_mana: u32,
    count: u16,
    fire_flag: bool,
    charge_flag: bool,
    castle_req: u32,
    damage: u32,
) -> SpellDef {
    SpellDef {
        possess_mana,
        count,
        fire_flag,
        charge_flag,
        castle_req,
        damage,
    }
}

/// Spellbook display order: page position -> internal spell id
/// (`byte_99B88`, sub_main.cpp:5752; iterated by the book draw at
/// :26918-26962). Matches the retail book layout.
pub const DISPLAY_ORDER: [u8; SPELL_COUNT] = [
    0, 3, 2, 16, 1, 14, 4, 12, 6, 9, 7, 8, 15, 18, 17, 19, 13, 5, 11, 10, 20, 21, 22, 23,
];

/// UI sprite id of a spell's book/HUD icon: `begSprTab[type + 6]`
/// (sub_main.cpp:27700). Ids index the bundle's `ui-sprites` member.
pub fn icon_sprite(spell: SpellId) -> u32 {
    spell.0 as u32 + 6
}

/// The mutually exclusive toggle pair (sub_46B00_46E40
/// :55871/:55914): firing one force-clears the other's charge —
/// forward vs backward thrust.
pub const TOGGLE_PAIR: (SpellId, SpellId) = (SpellId(2), SpellId(21));

#[cfg(test)]
mod tests {
    use super::*;

    /// Hidden Worlds renames exactly its one diverging spell row.
    #[test]
    fn hidden_worlds_names_spell_20_the_homing_meteor() {
        use crate::ids::GameId;
        for i in 0..SPELL_COUNT as u8 {
            let id = SpellId(i);
            assert_eq!(id.name_in(GameId::Mc1), id.name());
            if i == 20 {
                assert_eq!(id.name_in(GameId::Mc1), "Wall of Fire");
                assert_eq!(id.name_in(GameId::Mc1Hw), "Homing Meteor");
            } else {
                assert_eq!(id.name_in(GameId::Mc1Hw), id.name());
            }
        }
    }

    #[test]
    fn display_order_is_a_permutation() {
        let mut seen = [false; SPELL_COUNT];
        for &t in &DISPLAY_ORDER {
            assert!(!seen[t as usize]);
            seen[t as usize] = true;
        }
    }
}
