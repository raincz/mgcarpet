//! Save slots, both games (docs/archive/DESIGN-SAVES.md; retail research in
//! docs/traces/mc1-campaign-save-menu.md, mc2-campaign-save-menu.md).
//!
//! A slot is TWO files sharing a stem:
//!
//! - `<stem>.mgcs` — the native save ([`mgc_formats::mgcs`]), always
//!   written. It carries the campaign record plus, when the save was
//!   taken mid-level, the world payload. This is what the port reads.
//! - `<stem>.gam` — the retail record, written alongside as a
//!   one-way, best-effort export for players who want to carry
//!   progress into retail. It is read ONLY when no `.mgcs` exists for
//!   that slot (an imported GOG-era save), and is otherwise
//!   overwritten. Round-tripping back from retail is not supported:
//!   a `.gam` cannot carry mid-level state, so preferring it would
//!   silently discard a resume.
//!
//! Retail has no mid-level menu save for MC1 at all (it is Alt+S to a
//! single hard-coded slot 199) and a separate two-slot file set for
//! MC2, so neither game's snapshot shares storage with its campaign
//! save. Ours do share, which is why a native slot is not readable by
//! retail — an accepted consequence.
//!
//! The retail layouts stay byte-exact so GOG-era saves load and our
//! exports load back. Per-game directories (the formats collide —
//! both use `.gam`, different slot counts):
//! `saves/mc1/carpddNN.{gam,mgcs}` (6 slots, NN = 00..05),
//! `saves/mc1hw/carpddNN.{gam,mgcs}` (byte-identical format),
//! `saves/mc2/SAVEn.{GAM,mgcs}` (8 slots, n = 1..8).

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- MC1

/// MC1/HW slot count (`off_96864[6][21]`, remc1 :4228).
pub const MC1_SLOTS: usize = 6;
/// MC1 record size: magic 4 + name 20 + two 32-byte config buffers +
/// 12 settings + 4 level + 24 blob + 2 counters + 12 settings again.
pub const MC1_SIZE: usize = 142;

/// The MC1/HW menu save (`sub_51C90_51FD0` write / `sub_51AF0_51E30`
/// read, remc1 :62052-62081 / :62007-62041): pure campaign
/// progression. Fields we don't model are carried opaquely so a
/// load→save round trip preserves retail bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mc1Save {
    /// Slot label (20 bytes on disk, NUL-padded).
    pub name: String,
    /// Config `var_u8_29` (Basic.h:600): `[0]` = use-custom-loadout
    /// flag; carried opaquely.
    pub buf29: [u8; 32],
    /// Config `var_u8_61` (Basic.h:601): the 12-byte custom
    /// starting-spell loadout buffer; carried opaquely.
    pub buf61: [u8; 32],
    /// World struct +8597 settings block (written twice in retail).
    pub settings: [u8; 12],
    /// Current campaign level (`var_u16_17`), decoded from the
    /// obfuscated `4*(level + name_counter + player_count)` word
    /// (remc1 :62036).
    pub level: u16,
    /// World struct +15318 = `var_15318_1995_892[24]` — the PERSISTENT
    /// collected-spell flags, one byte per spell (the campaign memory;
    /// remc1 :49148 copy-in, ROADMAP "Campaign spell progression").
    /// Level-start grant = these flags ∩ the level's availability mask.
    pub blob24: [u8; 24],
    /// `byte_12CBD0` — the "CARPET%d" name-rotation counter (0-9).
    pub name_counter: u8,
    /// `byte_12CBD1` — the world-map player-count pick.
    pub player_count: u8,
    /// The second +8597 copy. Retail writes the block twice; kept
    /// separately so foreign saves round-trip even if they differ.
    pub settings_b: [u8; 12],
}

impl Mc1Save {
    pub fn decode(b: &[u8]) -> Result<Self, String> {
        if b.len() < MC1_SIZE {
            return Err(format!("MC1 save: {} bytes (need {MC1_SIZE})", b.len()));
        }
        let magic = u32::from_le_bytes(b[0..4].try_into().unwrap());
        if magic != 4 {
            return Err(format!("MC1 save: bad magic {magic} (want 4)"));
        }
        let mut s = Self {
            name: cstr(&b[4..24]),
            ..Self::default()
        };
        s.buf29.copy_from_slice(&b[24..56]);
        s.buf61.copy_from_slice(&b[56..88]);
        s.settings.copy_from_slice(&b[88..100]);
        let enc = u32::from_le_bytes(b[100..104].try_into().unwrap());
        s.blob24.copy_from_slice(&b[104..128]);
        s.name_counter = b[128];
        s.player_count = b[129];
        s.settings_b.copy_from_slice(&b[130..142]);
        // level = enc/4 − counters (remc1 :62036); clamp rather than
        // reject — a corrupt word yields level 0, not a load failure.
        s.level = (enc / 4)
            .saturating_sub(s.name_counter as u32 + s.player_count as u32)
            .min(u16::MAX as u32) as u16;
        Ok(s)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(MC1_SIZE);
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&fixed::<20>(&self.name));
        b.extend_from_slice(&self.buf29);
        b.extend_from_slice(&self.buf61);
        b.extend_from_slice(&self.settings);
        let enc = 4 * (self.level as u32 + self.name_counter as u32 + self.player_count as u32);
        b.extend_from_slice(&enc.to_le_bytes());
        b.extend_from_slice(&self.blob24);
        b.push(self.name_counter);
        b.push(self.player_count);
        b.extend_from_slice(&self.settings_b);
        debug_assert_eq!(b.len(), MC1_SIZE);
        b
    }
}

// ---------------------------------------------------------------- MC2

/// MC2 slot count (`SAVE%d.GAM` 1..=8, port_filesystem.cpp:549).
pub const MC2_SLOTS: usize = 8;
/// MC2 record size (write order MenusAndIntros.cpp:2606-2616).
pub const MC2_SIZE: usize = 1319;
/// The `.GAM` signature word (`0xFFFFFFF7` = -9).
pub const MC2_MAGIC: u32 = 0xFFFF_FFF7;

/// One secret-portal record as saved (17 bytes,
/// Type_SecretMapScreenPortals_E2970.h — packed, all LE):
/// time i32 @0, parent main level u16 @4, level number u16 @6,
/// map pos u16 @8/@10, activated u16 @12 (3=hidden, 2=revealed,
/// 1=completed), sprite u16 @14, byte @16.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecretPortal {
    pub time: i32,
    pub parent: u16,
    pub level: u16,
    pub pos: (u16, u16),
    pub activated: u16,
    pub sprite: u16,
    pub byte16: u8,
}

impl SecretPortal {
    fn decode(b: &[u8; 17]) -> Self {
        let u16le = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        Self {
            time: i32::from_le_bytes(b[0..4].try_into().unwrap()),
            parent: u16le(4),
            level: u16le(6),
            pos: (u16le(8), u16le(10)),
            activated: u16le(12),
            sprite: u16le(14),
            byte16: b[16],
        }
    }

    fn encode(&self) -> [u8; 17] {
        let mut b = [0u8; 17];
        b[0..4].copy_from_slice(&self.time.to_le_bytes());
        b[4..6].copy_from_slice(&self.parent.to_le_bytes());
        b[6..8].copy_from_slice(&self.level.to_le_bytes());
        b[8..10].copy_from_slice(&self.pos.0.to_le_bytes());
        b[10..12].copy_from_slice(&self.pos.1.to_le_bytes());
        b[12..14].copy_from_slice(&self.activated.to_le_bytes());
        b[14..16].copy_from_slice(&self.sprite.to_le_bytes());
        b[16] = self.byte16;
        b
    }
}

/// The MC2 campaign save (`SaveGameDialog` write MenusAndIntros.cpp:
/// 2606-2616, `LoadGameDialog` read :1493-1517): pure progression +
/// player stats, no world snapshot. Main-portal states are NOT
/// stored — retail reconstructs them from `levels_completed` (the
/// first N portals activate, :1521-1544), and so do we.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mc2Save {
    /// Editable slot label (20 bytes).
    pub label: String,
    /// `player_name_57ar` (32 bytes).
    pub player_name: String,
    /// `savestring_89` (32 bytes, opaque carry).
    pub savestring: [u8; 32],
    /// The secret-portal table (6 entries; [5] is the terminator).
    pub secrets: [SecretPortal; 6],
    /// `m_GameSettings` (16 bytes, opaque carry — retail display/
    /// gameplay toggles; our settings live in mgcarpet.json).
    pub game_settings: [u8; 16],
    /// Count of activated main portals (`numLevelsCompleted`,
    /// :2599-2604) — THE main-campaign progression record.
    pub levels_completed: u32,
    /// The current level's complete-flags word (`byte[2]` of
    /// `dw_w_b_0_2BDE_11230`: bit 1 advance, bit 3 reload, bit 4
    /// secret-route — docs/traces/mc2-campaign-save-menu.md).
    pub level_flags: u32,
    /// `str_611` — the 505-byte player spell/XP/mana block. Opaque
    /// until the layout lands; the campaign driver keeps its own
    /// book-carry alongside and syncs what it knows.
    pub str611: [u8; 505],
    /// Per-main-level score stats, 25×5 (`x_DWORD_17DBC8x`).
    pub main_stats: [[i32; 5]; 25],
    /// Per-secret-level score stats, 5×5 (`x_DWORD_17DDBCx`;
    /// sub_82AB0 EF:47027-33).
    pub secret_stats: [[i32; 5]; 5],
    /// The ENHANCED absolutes beside each stats row (player rulings
    /// 2026-09-24), indexed by the `EXT_*` constants: the kill
    /// breakdown and the counts behind the percentages. All −1 = not
    /// recorded (a retail save, or a level won before the row
    /// existed). Not part of the retail record: they ride an optional
    /// tail ([`MC2_EXT_TAG`]) after the 1319 bytes, written only when
    /// some row is recorded, so a save without one stays
    /// byte-identical to retail's.
    pub main_ext: [[i32; EXT_LEN]; 25],
    pub secret_ext: [[i32; EXT_LEN]; 5],
}

/// The stats tail's tag (after [`MC2_SIZE`]): `"MGK3"`, then 30 rows
/// × [`EXT_LEN`] i32 (main 0..25, secret 0..5). The first cut's
/// `"MGK2"` tail (nine columns, no denominators — it cannot give
/// tenths) is not read: those rows show the bare percentages.
pub const MC2_EXT_TAG: [u8; 4] = *b"MGK3";
/// One row of the stats tail: creatures dead (by anyone), by you, by
/// rivals, by nature, still alive, spells found, hits, shots, owned
/// mana, spells offered, the seedless world mana, dwelling mana.
/// Every percentage is derived from these at display time (see
/// [`ext_stats`]).
pub const EXT_LEN: usize = 12;
pub const EXT_KILLED: usize = 0;
pub const EXT_YOU: usize = 1;
pub const EXT_RIVALS: usize = 2;
pub const EXT_NATURE: usize = 3;
pub const EXT_ALIVE: usize = 4;
pub const EXT_SPELLS: usize = 5;
pub const EXT_HITS: usize = 6;
pub const EXT_SHOTS: usize = 7;
pub const EXT_MANA: usize = 8;
pub const EXT_OFFERED: usize = 9;
pub const EXT_MANA_WORLD: usize = 10;
pub const EXT_MANA_HOUSES: usize = 11;

/// A level's tail row from its stats.
pub fn ext_row(st: &mgc_sim::engine::stats::LevelStats) -> [i32; EXT_LEN] {
    let mut e = [0u32; EXT_LEN];
    e[EXT_KILLED] = st.deaths_total();
    e[EXT_YOU] = st.deaths_player();
    e[EXT_RIVALS] = st.deaths_rivals();
    e[EXT_NATURE] = st.deaths_environment();
    e[EXT_ALIVE] = st.alive;
    e[EXT_SPELLS] = st.spells_found_fixed;
    e[EXT_HITS] = st.offensive_hits;
    e[EXT_SHOTS] = st.offensive_shots;
    e[EXT_MANA] = st.mana_owned;
    e[EXT_OFFERED] = st.spells_offered_fixed;
    e[EXT_MANA_WORLD] = st.mana_world;
    e[EXT_MANA_HOUSES] = st.mana_houses;
    e.map(|n| n.min(i32::MAX as u32) as i32)
}

/// The stats a tail row recorded, for [`LevelStats`]'s derivations
/// (the rivals' kills folded into rival slot 1). `None` when the row
/// was not recorded.
///
/// [`LevelStats`]: mgc_sim::engine::stats::LevelStats
pub fn ext_stats(e: &[i32; EXT_LEN]) -> Option<mgc_sim::engine::stats::LevelStats> {
    if e.iter().any(|&n| n < 0) {
        return None;
    }
    let u = |k: usize| e[k] as u32;
    let mut st = mgc_sim::engine::stats::LevelStats {
        mc2: true,
        alive: u(EXT_ALIVE),
        spells_found_fixed: u(EXT_SPELLS),
        spells_offered_fixed: u(EXT_OFFERED),
        offensive_hits: u(EXT_HITS),
        offensive_shots: u(EXT_SHOTS),
        mana_world: u(EXT_MANA_WORLD),
        mana_owned: u(EXT_MANA),
        mana_houses: u(EXT_MANA_HOUSES),
        ..Default::default()
    };
    st.deaths[0] = u(EXT_YOU);
    st.deaths[1] = u(EXT_RIVALS);
    st.deaths[8] = u(EXT_NATURE);
    Some(st)
}

/// A new campaign's `str_611`: all zero EXCEPT the two quick-slot
/// hands, which are −1 ("none"). A zeroed block reads as
/// `left = right = 0` — Fireball in BOTH hands — and the carry's
/// "keep the hand if still possessed" validation passes it (Fireball is
/// always seeded), overriding the level-start Fireball/Possession pair.
fn str611_fresh() -> [u8; 505] {
    let mut b = [0u8; 505];
    b[S611_LEFT..S611_LEFT + 2].copy_from_slice(&(-1i16).to_le_bytes());
    b[S611_RIGHT..S611_RIGHT + 2].copy_from_slice(&(-1i16).to_le_bytes());
    b
}

impl Default for Mc2Save {
    fn default() -> Self {
        Self {
            label: String::new(),
            player_name: String::new(),
            savestring: [0; 32],
            secrets: crate::campaign::mc2_secret_portals_pristine(),
            game_settings: [0; 16],
            levels_completed: 0,
            level_flags: 0,
            str611: str611_fresh(),
            main_stats: [[0; 5]; 25],
            secret_stats: [[0; 5]; 5],
            main_ext: [[-1; EXT_LEN]; 25],
            secret_ext: [[-1; EXT_LEN]; 5],
        }
    }
}

impl Mc2Save {
    pub fn decode(b: &[u8]) -> Result<Self, String> {
        if b.len() < MC2_SIZE {
            return Err(format!("MC2 save: {} bytes (need {MC2_SIZE})", b.len()));
        }
        let magic = u32::from_le_bytes(b[0..4].try_into().unwrap());
        if magic != MC2_MAGIC {
            return Err(format!("MC2 save: bad signature {magic:#010x}"));
        }
        let mut s = Self {
            label: cstr(&b[4..24]),
            player_name: cstr(&b[24..56]),
            ..Self::default()
        };
        s.savestring.copy_from_slice(&b[56..88]);
        for (i, p) in s.secrets.iter_mut().enumerate() {
            let o = 88 + i * 17;
            *p = SecretPortal::decode(b[o..o + 17].try_into().unwrap());
        }
        s.game_settings.copy_from_slice(&b[190..206]);
        s.levels_completed = u32::from_le_bytes(b[206..210].try_into().unwrap());
        s.level_flags = u32::from_le_bytes(b[210..214].try_into().unwrap());
        s.str611.copy_from_slice(&b[214..719]);
        for (l, row) in s.main_stats.iter_mut().enumerate() {
            for (k, v) in row.iter_mut().enumerate() {
                let o = 719 + (l * 5 + k) * 4;
                *v = i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            }
        }
        for (l, row) in s.secret_stats.iter_mut().enumerate() {
            for (k, v) in row.iter_mut().enumerate() {
                let o = 1219 + (l * 5 + k) * 4;
                *v = i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            }
        }
        let tail = &b[MC2_SIZE..];
        if tail.len() >= 4 + 30 * EXT_LEN * 4 && tail[..4] == MC2_EXT_TAG {
            let rows = s.main_ext.iter_mut().chain(s.secret_ext.iter_mut());
            for (r, row) in rows.enumerate() {
                for (k, v) in row.iter_mut().enumerate() {
                    let o = 4 + (r * EXT_LEN + k) * 4;
                    *v = i32::from_le_bytes(tail[o..o + 4].try_into().unwrap());
                }
            }
        }
        Ok(s)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(MC2_SIZE);
        b.extend_from_slice(&MC2_MAGIC.to_le_bytes());
        b.extend_from_slice(&fixed::<20>(&self.label));
        b.extend_from_slice(&fixed::<32>(&self.player_name));
        b.extend_from_slice(&self.savestring);
        for p in &self.secrets {
            b.extend_from_slice(&p.encode());
        }
        b.extend_from_slice(&self.game_settings);
        b.extend_from_slice(&self.levels_completed.to_le_bytes());
        b.extend_from_slice(&self.level_flags.to_le_bytes());
        b.extend_from_slice(&self.str611);
        for row in &self.main_stats {
            for v in row {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        for row in &self.secret_stats {
            for v in row {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        debug_assert_eq!(b.len(), MC2_SIZE);
        let rows = || self.main_ext.iter().chain(self.secret_ext.iter());
        if rows().flatten().any(|&v| v >= 0) {
            b.extend_from_slice(&MC2_EXT_TAG);
            for v in rows().flatten() {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        b
    }
}

/// `str_611` internal offsets, relative to the 505-byte block
/// (global_types.h:174-216 — 10 parallel per-spell arrays + scalars;
/// docs/traces/mc2-campaign-save-menu.md). Only the progression
/// fields are typed; the rest (cooldowns, UI slot order, cursor
/// state) are re-derived by retail at level start and carried
/// opaquely here for byte-exact interop.
const S611_BANK: usize = 0; // i32[26] SpellExperience (banked XP)
const S611_VOL: usize = 104; // i32[26] spellsExperience (volatile XP)
const S611_ENABLED: usize = 208; // i16[26] manifestation handle (0 = not learned)
const S611_RING: usize = 338; // u8[26] array_0x3B5 cycle-ring membership (0/1/2)
const S611_OWNED: usize = 390; // u8[26] granted/owned flag (THE learned set)
const S611_LEVELS: usize = 442; // u8[26] derived tier 0..2
const S611_SEL: usize = 468; // u8[26] selected sub-spell/tier
const S611_LEFT: usize = 494; // i16 left quick-slot spell (−1 = none)
const S611_RIGHT: usize = 496; // i16 right quick-slot spell

/// The typed MC2 progression view over `str_611` — exactly the
/// fields retail's own campaign carry copies between levels
/// (`sub_549A0` Level.cpp:1259-66) plus the hand bindings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mc2Book {
    pub owned: [bool; 26],
    /// Total XP per spell (banked + volatile — the tier-derivation
    /// law sums both, EF:43872-916).
    pub xp: [i32; 26],
    pub levels: [u8; 26],
    pub sel: [u8; 26],
    pub left: i8,
    pub right: i8,
    /// `array_0x3B5` cycle-ring membership (0 = none, 1 = left,
    /// 2 = right) — retail's own campaign carry copies it
    /// (`sub_549A0` Level.cpp:1265) and the whole-blob save keeps it.
    pub ring: [u8; 26],
}

impl Mc2Save {
    /// Read the progression book out of `str_611`.
    pub fn book(&self) -> Mc2Book {
        let b = &self.str611;
        let i16at = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
        let mut out = Mc2Book {
            left: i16at(S611_LEFT).clamp(-1, 25) as i8,
            right: i16at(S611_RIGHT).clamp(-1, 25) as i8,
            ..Default::default()
        };
        for s in 0..26 {
            let i32at = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            let bank = i32at(S611_BANK + s * 4);
            let vol = i32at(S611_VOL + s * 4);
            let enabled =
                u16::from_le_bytes([b[S611_ENABLED + s * 2], b[S611_ENABLED + s * 2 + 1]]);
            out.owned[s] = b[S611_OWNED + s] != 0 || enabled != 0;
            out.xp[s] = bank.saturating_add(vol);
            out.levels[s] = b[S611_LEVELS + s];
            out.sel[s] = b[S611_SEL + s];
            out.ring[s] = b[S611_RING + s].min(2);
        }
        out
    }

    /// Write the progression book into `str_611` (all XP banked,
    /// volatile zeroed — the between-levels shape; the manifestation
    /// handle is stamped 1 for learned spells since retail re-derives
    /// real handles at level start, EF:38650-779).
    pub fn set_book(&mut self, book: &Mc2Book) {
        let b = &mut self.str611;
        for s in 0..26 {
            b[S611_BANK + s * 4..S611_BANK + s * 4 + 4].copy_from_slice(&book.xp[s].to_le_bytes());
            b[S611_VOL + s * 4..S611_VOL + s * 4 + 4].copy_from_slice(&0i32.to_le_bytes());
            let handle: i16 = if book.owned[s] { 1 } else { 0 };
            b[S611_ENABLED + s * 2..S611_ENABLED + s * 2 + 2]
                .copy_from_slice(&handle.to_le_bytes());
            b[S611_OWNED + s] = book.owned[s] as u8;
            b[S611_LEVELS + s] = book.levels[s];
            b[S611_SEL + s] = book.sel[s];
            b[S611_RING + s] = book.ring[s].min(2);
        }
        b[S611_LEFT..S611_LEFT + 2].copy_from_slice(&(book.left as i16).to_le_bytes());
        b[S611_RIGHT..S611_RIGHT + 2].copy_from_slice(&(book.right as i16).to_le_bytes());
    }
}

// --------------------------------------------------------- slot model

/// The retail-format path for a slot, under an explicit saves root —
/// THE one place either game's slot filename is spelled.
///
/// MC2 `SAVE/SAVE%d.GAM` (slot is 0-based here, retail names it
/// 1-based); MC1/HW `save/carpdd%02X.gam` (remc1 :61982). The public
/// wrappers pass the default root (`saves/`, next to `mgcarpet.json`
/// and never inside `gamedata/`); tests pass a temp dir.
fn retail_path_in(root: &Path, tag: &str, slot: usize) -> PathBuf {
    let dir = root.join(tag);
    if tag == "mc2" {
        dir.join(format!("SAVE{}.GAM", slot + 1))
    } else {
        dir.join(format!("carpdd{slot:02x}.gam"))
    }
}

/// The native save beside a retail slot: same stem, `.mgcs`.
pub fn native_path(tag: &str, slot: usize) -> PathBuf {
    retail_path(tag, slot).with_extension("mgcs")
}

/// Root of the saves tree — the relocation seam, sibling to the baked
/// tree's `get_baked_directory`: the public slot functions below all
/// resolve through it, so a port moves every save by changing one body.
fn saves_root() -> PathBuf {
    crate::paths::saves_dir()
}

/// The player's netplay entries (recent hosts, last port and
/// passphrase) — beside the saves, not in the config: they are the
/// player's history, not a preference.
pub fn netplay_path() -> PathBuf {
    saves_root().join("netplay.json")
}

/// The retail-format path for a slot, per game.
pub fn retail_path(tag: &str, slot: usize) -> PathBuf {
    retail_path_in(&saves_root(), tag, slot)
}

/// Slots per game.
pub fn slot_count(tag: &str) -> usize {
    if tag == "mc2" { MC2_SLOTS } else { MC1_SLOTS }
}

/// What the menu needs to draw one slot row, without decoding the
/// ~570 KiB payload behind it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlotInfo {
    /// Label to show. Empty when the slot is free (callers supply
    /// their own per-game placeholder).
    pub label: String,
    pub occupied: bool,
    /// The level this slot sits at — the one it resumes into, or the
    /// one the campaign is parked in front of. Every slot has one.
    /// The 0-based MAP index; players see [`SlotInfo::level_number`].
    pub level: u32,
    /// `Some(mana_percent)` when the slot carries a world payload and
    /// so resumes straight into play; `None` = a hub save.
    ///
    /// The percentage IS the in-level marker: a run in progress reads
    /// "L3 15%", a hub save just "L3".
    pub resume: Option<u8>,
    /// Read from a native `.mgcs`. False means the row came from an
    /// imported retail `.gam` with no native file beside it.
    pub native: bool,
    /// The file exists and could not be read AT ALL — corrupt, or a
    /// container so foreign that even the campaign record was
    /// unreachable. Shown as occupied-but-unloadable rather than as an
    /// empty slot, so a save is never silently overwritten.
    pub incompatible: bool,
    /// Read by SALVAGE: the container version is one this build cannot
    /// apply, so the campaign progress was lifted out of it and the
    /// world payload was dropped. Loading it resumes at the hub.
    ///
    /// Surfaced because it is a LOSS. A slot that silently stopped
    /// resuming would read as "my save is fine" right up until the
    /// player noticed their level restart.
    pub stale: bool,
    /// Campaign position, for the menu's level column.
    pub campaign_level: u32,
}

impl SlotInfo {
    /// The level as players number it: map index + 1 in all three
    /// games — retail's own numbering (the MC1/HW world list reads
    /// "1. Al Jahan" … "50. Volcania"; MC1's blacklisted maps are
    /// levels that simply never appear, which the official guide
    /// lists as "not appearing in the game") and the stats screen's
    /// "Level N".
    pub fn level_number(&self) -> u32 {
        self.level + 1
    }
}

/// Probe one slot: native first, retail as the fallback.
///
/// Never fails — an unreadable file becomes an `incompatible` row.
/// The menu must be able to list a directory of junk without an error
/// path, and must never present a damaged save as an empty slot.
pub fn scan_slot(tag: &str, slot: usize) -> SlotInfo {
    scan_slot_in(&saves_root(), tag, slot)
}

fn scan_slot_in(root: &Path, tag: &str, slot: usize) -> SlotInfo {
    let native = retail_path_in(root, tag, slot).with_extension("mgcs");
    if native.exists() {
        return match std::fs::File::open(&native)
            .map_err(|e| e.to_string())
            .and_then(|f| mgc_formats::mgcs::read_header(f).map_err(|e| e.to_string()))
        {
            Ok(h) => SlotInfo {
                label: h.label,
                occupied: true,
                level: h.level,
                resume: h.resume.as_ref().map(|r| r.mana_pct),
                native: true,
                incompatible: false,
                stale: false,
                campaign_level: h.campaign_level,
            },
            // The version gate fired. The campaign record inside is
            // retail's byte layout, not ours, so it survives any
            // container version — salvage it and present a hub slot.
            Err(_) => return salvage_slot(&native, tag),
        };
    }
    // No native file: an imported retail save, campaign-only by
    // construction (no retail `.gam` can carry mid-level state).
    let retail = retail_path_in(root, tag, slot);
    let Ok(bytes) = std::fs::read(&retail) else {
        return SlotInfo::default();
    };
    let decoded = if tag == "mc2" {
        Mc2Save::decode(&bytes).map(|s| (s.label, s.levels_completed))
    } else {
        Mc1Save::decode(&bytes).map(|s| (s.name, s.level as u32))
    };
    match decoded {
        // An imported `.gam` knows only the campaign counter, so that
        // is also the best "which level" it can offer.
        Ok((label, campaign_level)) => SlotInfo {
            label,
            occupied: true,
            level: campaign_level,
            resume: None,
            native: false,
            incompatible: false,
            stale: false,
            campaign_level,
        },
        Err(_) => SlotInfo {
            occupied: true,
            incompatible: true,
            ..Default::default()
        },
    }
}

/// Lift the campaign record out of a save this build cannot apply.
///
/// The resume is gone with the payload — its field order belongs to
/// `SNAPSHOT_VERSION` and a stale one cannot be applied — but the
/// player's progress is not our format's to lose.
fn salvage_slot(native: &Path, tag: &str) -> SlotInfo {
    let unreadable = SlotInfo {
        occupied: true,
        native: true,
        incompatible: true,
        ..Default::default()
    };
    let Ok(file) = std::fs::File::open(native) else {
        return unreadable;
    };
    let Ok(rec) = mgc_formats::mgcs::recover(file) else {
        return unreadable;
    };
    // The label in an old header may predate whatever we now store, so
    // prefer the campaign record's own name when there is one.
    let from_record = if tag == "mc2" {
        Mc2Save::decode(&rec.campaign).map(|s| (s.label, s.levels_completed))
    } else {
        Mc1Save::decode(&rec.campaign).map(|s| (s.name, s.level as u32))
    };
    let (label, campaign_level) = match from_record {
        Ok(v) => v,
        // The header parsed but the record did not: nothing
        // trustworthy left to load.
        Err(_) => return unreadable,
    };
    let label = if label.trim().is_empty() {
        rec.label
    } else {
        label
    };
    SlotInfo {
        label,
        occupied: true,
        level: campaign_level,
        resume: None,
        native: true,
        incompatible: false,
        stale: true,
        campaign_level,
    }
}

/// Probe every slot for a game.
pub fn scan_slots(tag: &str) -> Vec<SlotInfo> {
    (0..slot_count(tag)).map(|s| scan_slot(tag, s)).collect()
}

/// Write both halves of a slot: the native save, and the retail
/// export beside it.
///
/// The native write is the one that matters; an export failure is
/// reported but does NOT fail the save, because the export is a
/// convenience and losing it must never cost the player their
/// progress. A native failure is fatal to the operation.
pub fn write_slot(
    tag: &str,
    slot: usize,
    save: &mgc_formats::mgcs::SavePackage,
) -> Result<(), String> {
    write_slot_in(&saves_root(), tag, slot, save)
}

fn write_slot_in(
    root: &Path,
    tag: &str,
    slot: usize,
    save: &mgc_formats::mgcs::SavePackage,
) -> Result<(), String> {
    let native = retail_path_in(root, tag, slot).with_extension("mgcs");
    if let Some(dir) = native.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let bytes = mgc_formats::mgcs::to_bytes(save).map_err(|e| e.to_string())?;
    std::fs::write(&native, &bytes).map_err(|e| format!("{}: {e}", native.display()))?;

    // Best effort from here on.
    let retail = retail_path_in(root, tag, slot);
    // The retail record only: MC2's native stats tail
    // ([`MC2_EXT_TAG`]) stays in the `.mgcs`.
    let record = if tag == "mc2" {
        &save.campaign[..save.campaign.len().min(MC2_SIZE)]
    } else {
        &save.campaign[..]
    };
    if let Err(e) = std::fs::write(&retail, record) {
        eprintln!("save: retail export to {} failed: {e}", retail.display());
    }
    Ok(())
}

// ------------------------------------------------------------- helpers

/// NUL-terminated fixed field → String (lossy — retail names are
/// plain ASCII).
fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// String → NUL-padded fixed field, truncated to N−1 so the
/// terminator always fits (retail readers expect one).
fn fixed<const N: usize>(s: &str) -> [u8; N] {
    let mut b = [0u8; N];
    for (i, &c) in s.as_bytes().iter().take(N - 1).enumerate() {
        b[i] = c;
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc1_round_trip() {
        let mut s = Mc1Save {
            name: "WIZARD".into(),
            level: 23,
            name_counter: 3,
            player_count: 2,
            ..Default::default()
        };
        s.buf29[0] = 1;
        s.buf61[..4].copy_from_slice(&[9, 8, 7, 6]);
        s.blob24[23] = 0xAB;
        let bytes = s.encode();
        assert_eq!(bytes.len(), MC1_SIZE);
        // The obfuscated level word: 4*(23+3+2) = 112.
        assert_eq!(&bytes[100..104], &112u32.to_le_bytes());
        assert_eq!(Mc1Save::decode(&bytes).unwrap(), s);
    }

    #[test]
    fn mc1_rejects_garbage() {
        assert!(Mc1Save::decode(&[0u8; MC1_SIZE]).is_err()); // magic 0
        assert!(Mc1Save::decode(&[4, 0, 0, 0]).is_err()); // short
    }

    #[test]
    fn mc2_round_trip() {
        let mut s = Mc2Save {
            label: "MY SAVE".into(),
            player_name: "VISSILUTH".into(),
            levels_completed: 7,
            level_flags: 0x0A,
            ..Default::default()
        };
        // Secret 31 revealed (parent 7 just completed).
        s.secrets[1].activated = 2;
        s.secrets[1].sprite = 270;
        s.str611[0] = 0x11;
        s.str611[504] = 0x99;
        s.main_stats[6] = [100, 80, 90, 75, 12345];
        s.secret_stats[4][0] = -1;
        let bytes = s.encode();
        assert_eq!(bytes.len(), MC2_SIZE);
        assert_eq!(&bytes[0..4], &[0xF7, 0xFF, 0xFF, 0xFF]);
        // The load path's readbuffer+12 activated probe lands on our
        // entry 1's activated word: offset 88 + 17 + 12.
        assert_eq!(bytes[88 + 17 + 12], 2);
        assert_eq!(Mc2Save::decode(&bytes).unwrap(), s);
    }

    #[test]
    fn mc2_stats_extension_rides_the_tail() {
        let mut s = Mc2Save::default();
        assert_eq!(s.encode().len(), MC2_SIZE, "no extension, no tail");
        s.main_ext[6] = [30, 12, 10, 8, 5, 2, 40, 55, 1234, 3, 2000, 400];
        s.secret_ext[2] = [0; EXT_LEN];
        let bytes = s.encode();
        assert_eq!(bytes.len(), MC2_SIZE + 4 + 30 * EXT_LEN * 4);
        assert_eq!(Mc2Save::decode(&bytes).unwrap(), s);
        // The retail record alone (retail reads 1319 bytes): unknown.
        let bare = Mc2Save::decode(&bytes[..MC2_SIZE]).unwrap();
        assert_eq!(bare.main_ext[6], [-1; EXT_LEN]);
        // The first cut's nine-column MGK2 tail is not read.
        let mut old = bytes[..MC2_SIZE].to_vec();
        old.extend_from_slice(b"MGK2");
        old.extend(std::iter::repeat_n(0u8, 30 * 9 * 4));
        assert_eq!(Mc2Save::decode(&old).unwrap().main_ext[6], [-1; EXT_LEN]);
        // A recorded row derives the enhanced rows.
        let st = ext_stats(&s.main_ext[6]).unwrap();
        assert_eq!(st.deaths_total(), 30);
        assert_eq!(st.mana_rows(), [834, 400, 766]);
        assert_eq!(st.spells_fixed_pct10(), 667);
        assert_eq!(ext_stats(&s.main_ext[5]), None);
    }

    #[test]
    fn mc2_fresh_save_binds_no_hand() {
        // Player report 09-24: a new campaign started with Fireball in
        // BOTH hands — the zeroed str_611 read as left = right = 0.
        let book = Mc2Save::default().book();
        assert_eq!((book.left, book.right), (-1, -1));
        assert!(book.owned.iter().all(|&o| !o));
    }

    #[test]
    fn mc2_book_round_trips_through_str611() {
        let mut save = Mc2Save::default();
        let mut book = Mc2Book {
            left: 0,
            right: 4,
            ..Default::default()
        };
        book.owned[0] = true; // fireball
        book.owned[4] = true;
        book.xp[4] = 12345;
        book.levels[4] = 2;
        book.sel[4] = 1;
        book.ring[0] = 1;
        // A ring member the player does not currently possess — the
        // carry keeps it RAW (skip-not-drop).
        book.ring[7] = 2;
        save.set_book(&book);
        assert_eq!(save.book(), book);
        // The typed view survives the byte codec too.
        let loaded = Mc2Save::decode(&save.encode()).unwrap();
        assert_eq!(loaded.book(), book);
        // Learned spells carry a nonzero manifestation handle for
        // retail's enabled-gate; XP lands banked; the ring sits at
        // retail's own array_0x3B5 offset so a retail load honors it.
        assert_eq!(loaded.str611[S611_OWNED + 4], 1);
        assert_eq!(
            &loaded.str611[S611_BANK + 16..S611_BANK + 20],
            &12345i32.to_le_bytes()
        );
        assert_eq!(loaded.str611[S611_RING], 1);
        assert_eq!(loaded.str611[S611_RING + 7], 2);
    }

    #[test]
    fn mc2_rejects_garbage() {
        assert!(Mc2Save::decode(&[0u8; MC2_SIZE]).is_err());
    }

    // ------------------------------------------------- the slot model

    /// A scratch saves root under the target dir; removed on drop.
    /// Per-test paths, so nothing here depends on the process cwd and
    /// tests stay parallel-safe.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("mgcarpet-test-{name}"));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn mid_level_package(label: &str) -> mgc_formats::mgcs::SavePackage {
        let mut header =
            mgc_formats::mgcs::hub_header(mgc_formats::Game::MagicCarpet1, label.into(), 4, 12);
        header.resume = Some(mgc_formats::mgcs::InLevel {
            bundle: "mc1-temperate".into(),
            entry_sha256: "deadbeef".into(),
            snapshot_version: 1,
            tick: 999,
            mana_pct: 15,
            thrust_model: None,
            altitude_model: None,
        });
        mgc_formats::mgcs::SavePackage {
            header,
            campaign: Mc1Save {
                name: label.into(),
                level: 4,
                ..Default::default()
            }
            .encode(),
            snapshot: Some(vec![3u8; 256]),
        }
    }

    /// Writing a slot must leave BOTH files: the native save the port
    /// reads, and the retail export beside it.
    #[test]
    fn writing_a_slot_leaves_a_retail_export_beside_it() {
        let s = Scratch::new("saves-export");
        write_slot_in(&s.0, "mc1", 2, &mid_level_package("WIZARD")).unwrap();

        let native = retail_path_in(&s.0, "mc1", 2).with_extension("mgcs");
        let retail = retail_path_in(&s.0, "mc1", 2);
        assert!(native.exists(), "native save");
        assert!(retail.exists(), "retail export");
        // The export is the retail record verbatim, so retail can read it.
        let bytes = std::fs::read(&retail).unwrap();
        assert_eq!(bytes.len(), MC1_SIZE);
        assert_eq!(Mc1Save::decode(&bytes).unwrap().name, "WIZARD");
    }

    /// THE precedence rule. A slot with both files must resolve to the
    /// native one — the retail `.gam` cannot carry mid-level state, so
    /// preferring it would silently turn a resume into a restart.
    #[test]
    fn the_native_save_wins_over_the_retail_export() {
        let s = Scratch::new("saves-precedence");
        write_slot_in(&s.0, "mc1", 0, &mid_level_package("RESUME")).unwrap();
        // Both files now exist, and the retail one knows nothing about
        // the level in progress.
        assert!(retail_path_in(&s.0, "mc1", 0).exists());

        let info = scan_slot_in(&s.0, "mc1", 0);
        assert!(info.native, "the native file must be the one read");
        assert_eq!(info.resume, Some(15), "the resume must survive");
        assert_eq!(info.level, 12, "and so must the level it resumes into");
        assert_eq!(info.label, "RESUME");
    }

    /// A retail save with no native file beside it is an import: it
    /// lists, and it resumes at the hub because that is all a `.gam`
    /// can express.
    #[test]
    fn a_lone_retail_save_is_imported_as_a_hub_slot() {
        let s = Scratch::new("saves-import");
        let path = retail_path_in(&s.0, "mc1", 1);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            Mc1Save {
                name: "GOG".into(),
                level: 9,
                ..Default::default()
            }
            .encode(),
        )
        .unwrap();

        let info = scan_slot_in(&s.0, "mc1", 1);
        assert!(info.occupied);
        assert!(!info.native);
        assert_eq!(info.label, "GOG");
        assert_eq!(info.campaign_level, 9);
        assert_eq!(info.resume, None, "a .gam cannot carry a resume");
    }

    /// A save from a container version this build cannot apply must
    /// still give up its campaign progress — that record is retail's
    /// byte layout, not ours, so it is not our format's to lose. The
    /// resume goes with the payload, and the row says so.
    #[test]
    fn an_old_container_is_salvaged_for_its_progress() {
        let s = Scratch::new("saves-salvage");
        let native = retail_path_in(&s.0, "mc1", 0).with_extension("mgcs");
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();

        // A v1 save: `level` is the OBJECT it used to be, which is
        // exactly what this build's header cannot parse.
        let record = Mc1Save {
            name: "RAIN".into(),
            level: 3,
            ..Default::default()
        }
        .encode();
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write;
            use zip::write::SimpleFileOptions;
            let opts = SimpleFileOptions::default();
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("save.json", opts).unwrap();
            zip.write_all(
                br#"{"save_version":1,"game":"mc1","label":"RAIN","campaign_level":3,
                     "level":{"index":3,"bundle":"mc1-temperate","tick":182}}"#,
            )
            .unwrap();
            zip.start_file("campaign.bin", opts).unwrap();
            zip.write_all(&record).unwrap();
            zip.start_file("snapshot.bin", opts).unwrap();
            zip.write_all(&[0u8; 16]).unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&native, buf.into_inner()).unwrap();

        let info = scan_slot_in(&s.0, "mc1", 0);
        assert!(info.occupied);
        assert!(info.stale, "salvaged, and the row must say so");
        assert!(!info.incompatible, "progress came through, so it loads");
        assert_eq!(info.label, "RAIN", "the name survives");
        assert_eq!(info.level, 3, "and the campaign position");
        assert_eq!(info.resume, None, "but the resume is gone with the payload");
    }

    /// A damaged save must never render as an empty slot — an empty
    /// row is an invitation to overwrite it.
    #[test]
    fn a_damaged_save_lists_as_unreadable_not_empty() {
        let s = Scratch::new("saves-damaged");
        let native = retail_path_in(&s.0, "mc1", 3).with_extension("mgcs");
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::write(&native, b"not a zip at all").unwrap();

        let info = scan_slot_in(&s.0, "mc1", 3);
        assert!(info.occupied, "the file exists, so the slot is taken");
        assert!(info.incompatible);
        assert_ne!(info, SlotInfo::default(), "must not read as empty");
    }

    #[test]
    fn an_absent_slot_is_empty() {
        let s = Scratch::new("saves-absent");
        assert_eq!(scan_slot_in(&s.0, "mc1", 4), SlotInfo::default());
    }

    /// Re-saving between levels must drop a stale world payload, or a
    /// finished level would still offer to resume into itself.
    #[test]
    fn a_hub_save_clears_a_previous_resume() {
        let s = Scratch::new("saves-lifecycle");
        write_slot_in(&s.0, "mc1", 0, &mid_level_package("MID")).unwrap();
        assert_eq!(scan_slot_in(&s.0, "mc1", 0).resume, Some(15));

        let hub = mgc_formats::mgcs::SavePackage {
            header: mgc_formats::mgcs::hub_header(
                mgc_formats::Game::MagicCarpet1,
                "MID".into(),
                5,
                12,
            ),
            campaign: Mc1Save {
                name: "MID".into(),
                level: 5,
                ..Default::default()
            }
            .encode(),
            snapshot: None,
        };
        write_slot_in(&s.0, "mc1", 0, &hub).unwrap();
        let info = scan_slot_in(&s.0, "mc1", 0);
        assert_eq!(info.resume, None, "the stale payload must be gone");
        assert_eq!(info.campaign_level, 5);
    }

    #[test]
    fn native_and_retail_paths_share_a_stem() {
        assert_eq!(
            native_path("mc1hw", 0),
            Path::new("saves/mc1hw/carpdd00.mgcs")
        );
        assert_eq!(native_path("mc2", 7), Path::new("saves/mc2/SAVE8.mgcs"));
        assert_eq!(slot_count("mc2"), MC2_SLOTS);
        assert_eq!(slot_count("mc1hw"), MC1_SLOTS);
    }

    #[test]
    fn slot_paths() {
        assert_eq!(
            retail_path("mc1hw", 0),
            Path::new("saves/mc1hw/carpdd00.gam")
        );
        assert_eq!(retail_path("mc2", 0), Path::new("saves/mc2/SAVE1.GAM"));
        assert_eq!(retail_path("mc2", 7), Path::new("saves/mc2/SAVE8.GAM"));
    }
}
