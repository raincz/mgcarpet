//! MC2 rival wizards — the class-3 model-1 AI carpets on the MC2
//! column: lifecycle (spawn/records/authored castles), the per-tick
//! brain, the casting arm, mortality and respawn. Port of the remc2
//! machinery over the MC1 rival chassis; trace bank:
//! docs/traces/mc2-rivals-brain.md, mc2-rivals-spawn-mortality.md and
//! mc2-rivals-open-closure.md (`EF:` = remc2 EventsFunctions.cpp).
//!
//! The MC2 brain is the MC1 brain function-for-function (the
//! sub_12910 housekeeping/selector/handlers sandwich, the 0x601F hate
//! ledger, the burst gun, the poverty latch). What MC2 keys
//! differently:
//! - the SPELL IDS (heal 5, speed-up 3, possess 1, cloak 0xB,
//!   castle 2) and the recast/attack-priority tables (§7.2/7.3);
//! - the book is the class-15 manifestation entity per spell
//!   ([`Mc2Spellbook`] per rival), granted ONLY at load from the
//!   level's `WizardMapSettings` masks with authored starting tiers;
//!   MC2 has NO runtime spell learning (open-closure §3);
//! - the water/obstacle steer `sub_16580` runs after every state
//!   handler (open-closure §1 — MC1's AI flew over everything);
//! - death scatters the class-15 SPELL TOKENS (re-collectible), the
//!   respawn timer is a flat 1200, and a castle-less dead rival is
//!   BANISHED — the elimination signal the staged objective engine's
//!   case 3/8 reads;
//! - rivals earn NO spell XP — `sub_6D8B0`'s guard is class-3
//!   model-0, the human only (EF:58240-41); a rival's tiers are its
//!   authored map levels, and the per-cast TIER-DOWN walk
//!   (sub_15F20) supplies the tier dynamics.
//!
//! Original asymmetries, ported as traced: the AI at its own castle
//! DISCARDS damage (grace pinned 2, mailbox memset — EF:5400-5414;
//! the at-castle FORWARD is human-only, EF:59961); AI life regen /200
//! home /500 afield (4x the human afield); the AI carpet ignores
//! walls and knockback but — new in MC2 — steers around WATER; target
//! scans are omniscient.
//!
//! Open interim deviations (ours, flagged inline): the hate feed
//! rides damage intake instead of the per-projectile scan sub_159E0
//! (the MC1-column position); the DEFENSE state's disguise VISUAL
//! (retail draws the metamorph creature in place of the AI carpet) is
//! presentation-side unported — the state machine, tier pick,
//! shadowing and speed law are faithful (sub_15FC0/sub_161A0).

use crate::engine::features::{Gen, tile};
use crate::engine::world::{LifeState, World};
use crate::mc1::mobs::PLAYER_TARGET;
use crate::mc2::behavior::BEHAVIOR;
use crate::mc2::cast::Mc2Spellbook;

/// MC2 spell count (spell ids 0..25).
pub const MC2_SPELLS: usize = 26;

/// The hate ledger's neutral baseline (0x601F, EF:5377).
const HATE_NEUTRAL: u16 = 24607;
/// Hate toward a freshly (re)spawned wizard — elevated but decaying
/// (the post-spawn truce, -24609 as unsigned, EF:43850).
const HATE_RESPAWN: u16 = 40927;

/// The MC2 AI per-spell recast cooldowns `x_WORD_D3F4C` (EF:1070) —
/// differs wholesale from MC1's table AND is indexed by MC2 spell id.
const AI_RECAST: [u16; MC2_SPELLS] = [
    2, 10, 40, 32, 300, 1, 1, 1, 1, 4, 1, 1, 0, 0, 0, 0, 0, 0, 400, 600, 600, 400, 400, 0, 0, 0,
];

/// Attack-priority walk vs a WIZARD — `unk_D3F80x` (EF:1071).
const ATTACK_WIZARD: [u8; 8] = [0x10, 0x12, 0x09, 0x07, 0x14, 0x15, 0x13, 0x00];
/// Attack-priority walk vs a CASTLE — `unk_D3F89x` (EF:1072).
const ATTACK_CASTLE: [u8; 7] = [0x10, 0x12, 0x07, 0x09, 0x11, 0x14, 0x00];
/// The DEFENSE state's DISGUISE-MODEL table — `unk_D3F91x` (EF:1073):
/// the class-5 creature models Metamorph's tiers turn a wizard into
/// (2 = tier-0 Day bird, 0x13 = tier-0 non-Day, 0x19 = tier 1,
/// 0x10 = tier 2), walked in scan-priority order by `sub_15FC0`
/// (EF:7664-79). These are creature MODELS, not spell ids — do not
/// feed them to the cast path.
const DISGUISE_MODELS: [u8; 4] = [0x02, 0x13, 0x19, 0x10];

/// Metamorph tier for a disguise model (`SetSpell` switch,
/// EF:7685-7711): 2/0x13 → tier 0, 0x19 → 1, 0x10 → 2.
fn mc2_disguise_tier(model: u8) -> u8 {
    match model {
        0x19 => 1,
        0x10 => 2,
        _ => 0,
    }
}
/// Raid-castle offense gate `sub_164B0` (EF:6182 caller).
const OFFENSE_RAID: [u8; 9] = [0x11, 0x10, 0x12, 0x07, 0x09, 0x14, 0x13, 0x15, 0x00];
/// Attack-wizard/balloon/hunt offense gate `sub_15E60` (EF:6233).
const OFFENSE_ATTACK: [u8; 7] = [0x00, 0x07, 0x12, 0x10, 0x14, 0x15, 0x09];

// ---- the water-steer static tables (open-closure §1.0, EF:1074-79).
// Step deltas for the 40-step detour march, indexed by the probe
// code's LOW byte (0xff = -1).
const STEER_DX_L: [i8; 14] = [0, -1, 0, -1, 1, -1, 0, 0, 0, 0, -1, 0, 0, 0];
const STEER_DY_L: [i8; 14] = [0, 0, -1, 0, 0, 0, -1, -1, 1, 1, 0, 1, 0, 0];
const STEER_DX_R: [i8; 14] = [0, 1, 0, 0, -1, 1, -1, 0, 0, 1, 1, 0, 0, 0];
const STEER_DY_R: [i8; 14] = [0, 0, 1, 1, 0, 0, 0, 1, -1, 0, 0, 1, -1, 0];
/// Escape yaw when committed LEFT — `x_WORD_D3FCE` (EF:1078).
const STEER_YAW_L: [u16; 13] = [
    0, 1536, 0, 1536, 512, 1536, 0, 0, 1024, 1024, 1536, 1024, 512,
];
/// Escape yaw when committed RIGHT — `x_WORD_D3FE8` (EF:1079).
const STEER_YAW_R: [u16; 14] = [
    1024, 512, 1024, 1024, 1536, 512, 1536, 1024, 0, 512, 512, 1024, 0, 0,
];

/// The MC2 rival wizard names by color (`WizardsNames_D93A0`,
/// GameUI.h:39; `GetTrueWizardNumber` is identity in single player).
pub const MC2_RIVAL_NAMES: [&str; 8] = [
    "Zanzamar", "Nyphur", "Rahn", "Belix", "Jark", "Elyssia", "Yragore", "Prish",
];

/// The AI wizard's tuning row: the rival ctor PINS `str_D7BD6[67]`
/// (sub_4A9C0 EF:33351), overriding the spawn law 59+model=60. Row 67
/// carries the retail AI band (ceiling ground+768, floor ground+128),
/// turn caps (v_4 5, v_2 256), climb -4 and the 8192 engagement range
/// v_28; row 60's 1792/0/22/4096 are wrong for the brain's consumers.
const WIZARD_ROW: u8 = 67;

/// Per-color config from the level record (the 110-byte
/// `WizardMapSettings_0x360D2` block + the header's authored
/// starting-castle level), resolved by the app.
#[derive(Debug, Clone)]
pub struct Mc2RivalConfig {
    /// `Aggression_0x360D5` — hate pacing, wealth-scaled war
    /// thresholds, opportunism margins.
    pub aggression: u8,
    /// `Perception_0x360DD` — notice rolls + aim-cone width.
    pub perception: u8,
    /// `Reflexes_0x360D9` — decision cadence, turn rate, burst
    /// lockout.
    pub reflexes: u8,
    /// `Life_0x3612F` — 16.8 HP scalar; ALSO the castle-HP factor
    /// (castle-data-tables §2.4). 0 = default 256 (1.0x).
    pub life: u16,
    /// `player_0x2FED9[color]` — authored starting-castle level
    /// (0 = none, N = a castle at level N-1). AI-only in retail
    /// (open-closure §7 — the human never consumes it).
    pub castle_level: u8,
    /// `StartingSpells_0x360E1x` — per-spell grant flag.
    pub start: [bool; MC2_SPELLS],
    /// `byte_0x360FBx` — per-spell starting LEVEL 0..2 (clamped;
    /// EF:38693 writes it straight into the AI's SpellLevels).
    pub start_level: [u8; MC2_SPELLS],
    /// `BlockedSpells_0x36115x` — per-spell deny flag.
    pub blocked: [bool; MC2_SPELLS],
}

/// The MC2 AI brain state (`byte_0x1C1_449`; brain trace §1.2). Same
/// semantic set as MC1's — plus the DEFENSE state MC2 selects as
/// cascade step 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub(crate) enum Mc2AiState {
    /// Fresh spawn: decide immediately (the case-0 double selector
    /// call, EF:5255-56).
    #[default]
    Fresh,
    /// State 1: fly home, cast the castle upgrade (sub_12FF0).
    Upgrade,
    /// State 3: fly to the scouted site, cast the castle (sub_13100).
    Build,
    /// State 6: claim a mana ball with possess-1 (sub_131F0/135C0).
    Possess,
    /// State 7: raid an enemy castle (sub_13710).
    RaidCastle,
    /// State 8: attack an enemy wizard (sub_13830).
    AttackWizard,
    /// State 9: intercept an enemy balloon (same handler).
    RaidBalloon,
    /// State 13: hunt any mana-holding creature.
    HuntMana,
    /// State 11: return home (heal / regroup; sub_133B0).
    Home,
    /// State 12: cruise (sub_13270).
    Cruise,
    /// State 14: reactive defense (sub_161A0) — cascade step 7.
    Defense,
}

impl Mc2AiState {
    /// The retail `byte_0x1C1_449` value the dispatch switches on
    /// (EF:5252-5310). State 4 (`sub_131F0`) is the possess APPROACH
    /// arm — a target-alive check plus the 256/2048 close, no cast —
    /// and shares this port state with 6 (`sub_135C0`, the arm that
    /// casts); 2, 5 and 10 are the decompile's `_nmemneed` stubs and
    /// 15+ never appear, all of which fall through to the bare
    /// selector call — the port's Fresh.
    pub(crate) fn from_retail(v: u8) -> Self {
        match v {
            1 => Mc2AiState::Upgrade,
            3 => Mc2AiState::Build,
            4 | 6 => Mc2AiState::Possess,
            7 => Mc2AiState::RaidCastle,
            8 => Mc2AiState::AttackWizard,
            9 => Mc2AiState::RaidBalloon,
            0xB => Mc2AiState::Home,
            0xC => Mc2AiState::Cruise,
            0xD => Mc2AiState::HuntMana,
            0xE => Mc2AiState::Defense,
            _ => Mc2AiState::Fresh,
        }
    }
}

/// The retail AI decision lanes for one wizard, as the conformance
/// importer lifts them — the wizard extension's (`type_str_164`)
/// brain half plus the two lanes that ride the wizard ENTITY. Kept
/// as a record because the re-anchor seats fourteen of them.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Mc2RivalAi {
    /// `byte_0x1C1_449`, raw — mapped by [`Mc2AiState::from_retail`].
    pub state: u8,
    /// Entity `word_0x96_150`, already translated to the port's
    /// out-of-pool [`PLAYER_TARGET`] convention for the human.
    pub target: u16,
    /// Entity `word_0x98_152`, the stored target signature.
    pub target_sig: u16,
    /// Entity `axis_0x9A_154x` — the scouted castle site.
    pub site: (u16, u16),
    pub burst: i16,
    pub poverty: i16,
    pub cooldown: [u16; MC2_SPELLS],
    pub hate: [u16; 8],
    pub war: [u16; 8],
    pub weave: u8,
    pub weave_dir: u8,
    pub avoid: u8,
    pub avoid_exit: u8,
    pub aggression: u16,
    pub perception: u16,
    pub reflexes: u16,
    pub life_scale: u16,
}

/// One live MC2 rival: the player-extension subset the AI machinery
/// needs. Position/yaw/life live on the pool entity (class 3 model 1).
#[derive(Hash)]
pub(crate) struct Mc2Rival {
    /// Player color (1..=7); color 0 = the human, never a rival.
    pub slot: u8,
    /// Wizard entity pool index — also the OWNER TAG on its
    /// projectiles (id24) and claims.
    pub ent: u16,
    /// The per-wizard `str_611` book subset — class-15 manifestation
    /// slots, per-spell XP and levels (shared shape with the human's
    /// [`Mc2Spellbook`]).
    pub(crate) book: Mc2Spellbook,
    /// Spells known across deaths — death reverts the book flags to
    /// boolean (EF:60147), respawn re-mints the manifestations.
    pub known: [bool; MC2_SPELLS],
    /// AI recast cooldowns (`SpellEnabled` doubles as the cooldown
    /// counter in retail, EF:5361-63; ours is a separate array).
    pub(crate) cooldown: [u16; MC2_SPELLS],
    /// Carried mana / ceiling / the regen delta the cast debit rides.
    pub mana: u32,
    pub mana_max: u32,
    pub(crate) mana_delta: i32,
    /// Personality (word_0x242/244/246) + the Life scalar (word_0x24A).
    agg: u16,
    per: u16,
    refl: u16,
    pub(crate) life_scale: u16,
    pub state: Mc2AiState,
    /// Hate ledger + war flags per color (array_0x1FC_508).
    pub(crate) hate: [u16; 8],
    pub(crate) war: [bool; 8],
    /// Fireball-family burst counter (word_0x1A2_418): 8 shots then a
    /// negative lockout of (Reflexes-255)/8-1 ticks (EF:6813-15).
    burst: i16,
    /// Poverty latch (word_0x1A4_420): mana < max/4 stops attack
    /// casting; the release is max/4 + 6000, clamped to max/2 only
    /// when the sum overshoots the ceiling (EF:7191-7205).
    poverty: bool,
    /// Current target: entity slot or [`PLAYER_TARGET`]; 0 = none.
    pub(crate) target: u16,
    target_sig: u16,
    /// Scouted castle site (axis_0x9A_154x).
    pub(crate) site: (u16, u16),
    /// The strafe channel (strafeSpeed_0x10_16): decays 4/tick,
    /// stepped at yaw+90. Written 80 by the reactive dodge
    /// (EF:7469) and 3*minSpeed*Reflexes/255 by the combat weave.
    strafe: i16,
    /// The combat-weave micro-FSM (str_611_byte_0x45D_1117, 0..20).
    weave: u8,
    /// The weave's committed direction (str_611_byte_0x45C_1116):
    /// 1 = port (-512), 2 = starboard (+512).
    weave_dir: u8,
    /// The two-stage shield absorb (the wizard byte[1]/byte[2] 0x40
    /// pair, EF:60676-93): 0 spent, 1 armed (next hit nulled +
    /// promotes), 2 charged (next hit quartered, mana-paid, spends).
    shield_state: u8,
    /// The water-steer micro-FSM (byte_0x45E_1118): 0 idle, 1/2
    /// committed left/right, 3..7 frozen arc, >=8 re-detect.
    avoid: u8,
    /// The last chosen steer exit code (byte_0x45E_1119).
    avoid_exit: u8,
    /// Desired speed toward which f126 accelerates 16/tick.
    vdes: i16,
    /// Spawn/at-castle grace (word_0x159_345): mailbox memset while
    /// > 0 (100 at spawn, pinned 2 at the own castle).
    pub(crate) grace: u16,
    /// Dead and castle-less: BANISHED (byte_0x006_2BE4_11236 = 0,
    /// EF:60299) — the objective case-3/8 signal.
    pub eliminated: bool,
    /// Buff flags derived from the manifestations' armed windows.
    pub shield: bool,
    pub invisible: bool,
    pub rebound: bool,
}

impl Mc2Rival {
    fn new(slot: u8, ent: u16, cfg: &Mc2RivalConfig) -> Self {
        Mc2Rival {
            slot,
            ent,
            book: Mc2Spellbook::default(),
            known: [false; MC2_SPELLS],
            cooldown: [0; MC2_SPELLS],
            mana: 1000,
            mana_max: 1000,
            mana_delta: 0,
            agg: cfg.aggression as u16,
            per: cfg.perception as u16,
            refl: cfg.reflexes as u16,
            life_scale: if cfg.life == 0 { 256 } else { cfg.life },
            state: Mc2AiState::Fresh,
            hate: [HATE_NEUTRAL; 8],
            war: [false; 8],
            burst: 0,
            poverty: false,
            target: 0,
            target_sig: 0,
            site: (0, 0),
            strafe: 0,
            weave: 0,
            weave_dir: 1,
            shield_state: 0,
            avoid: 0,
            avoid_exit: 0,
            vdes: 0,
            grace: 100,
            eliminated: false,
            shield: false,
            invisible: false,
            rebound: false,
        }
    }

    /// Decision-cadence period: `64 - Reflexes/4` ticks keyed on the
    /// entity age byte (EF:5460).
    fn think_period(&self) -> u8 {
        (64 - (self.refl / 4) as i32).max(1) as u8
    }
}

impl World {
    /// Wire the MC2 level's wizards: colors `1..player_count` spawn
    /// as AI rivals at their (3,4+color) start markers (the
    /// `sub_53160` activation walk under the NumberOfPlayers pump
    /// bound — lifecycle trace §1 + the header-unk09 identification).
    /// Color 0 (the human) stays out-of-pool; it gets NO authored
    /// starting castle (open-closure §7).
    pub fn set_mc2_wizards(&mut self, configs: &[Option<Mc2RivalConfig>; 8], player_count: u16) {
        for slot in 1..player_count.min(8) as u8 {
            let Some(cfg) = &configs[slot as usize] else {
                continue;
            };
            self.mc2_spawn_rival(slot, cfg.clone());
        }
    }

    /// `sub_5C950` (EF:43600), the fresh-spawn arm: the (3,1) carpet
    /// at `array_0x2362[color]` raised ground+0x100, base stats, the
    /// AI personality + Life scalar, the book from the map masks,
    /// and the authored starting castle.
    fn mc2_spawn_rival(&mut self, slot: u8, cfg: Mc2RivalConfig) {
        // Start marker (3,4+color); a color with no marker keeps the
        // memset-0 position (map origin) — the retail authoring
        // contract (lifecycle §1 item 3).
        let (mx, my) = self.start_markers[slot as usize].unwrap_or((0, 0));
        let x = (mx << 8).wrapping_add(128);
        let y = (my << 8).wrapping_add(128);
        let z = (self.g.ground_z(x, y) as i16).wrapping_add(0x100);
        let Some(i) = self.g.new_event() else { return };
        {
            let e = &mut self.g.ent[i];
            e.class64 = 3;
            e.model65 = 1;
            e.tick70 = 1; // action 1 = the AI tick (EF:43696)
            e.max_life = 10000;
            e.row156 = WIZARD_ROW;
            e.f128 = BEHAVIOR[WIZARD_ROW as usize].v_0.max(80);
            e.id24 = i as u16; // self owner-tag (the MC1 convention)
            // byte_0x38_56 = 29: the vulnerability mask both wizard
            // ctors write (AddPlayer_4A920 / sub_4A9C0, EF:33326/33352)
            // — ch0 damage + ch2/ch3/ch4 (claim/steal/grip). Without
            // it f28 stays 0 and `area_write`'s per-channel gate drops
            // EVERY hit at the mailbox: a fireball detonates ON the
            // rival but deals nothing (unkillable). debug_kill injects
            // mail directly, so mortality tests do NOT exercise this.
            e.f28 = 29;
        }
        self.g.link(i, x, y, z);
        // Carpet sprite by color: retail switches on
        // TransformPlayerColorIndex (EF:43732) -> models 273..279
        // (the human keeps 44) — the art families are authored in
        // Transform order (crate::mc2::COLOR_ART). Rivals only ever
        // take slots 1.., so this never reads the row-44 arm; the
        // shared helper is what the replay ghost needs.
        self.g
            .mc2_set_sprite(i, crate::mc2::carpet_sprite_row(slot));
        let mut r = Mc2Rival::new(slot, i as u16, &cfg);
        // Life scalar: wizard maxLife *= Life/256 (EF:43768-72).
        self.g.ent[i].max_life = ((10000u64 * r.life_scale as u64) >> 8).max(1) as u32;
        self.g.refill_life(i);
        // The ladder reads the owner's Life scalar per slot.
        self.g.mc2_life_scale.0[slot as usize] = r.life_scale;
        // The book: `InitialiseSpells_54A50` (EF:38650) — AI grant =
        // granted && !blocked; starting LEVEL = byte_0x360FBx clamped
        // <= 2, written straight into SpellLevels (no XP accrual).
        // spellIndex_D94FF is identity for the 26 real spells
        // (open-closure §4) — raw spell-id indexing.
        for s in 0..MC2_SPELLS {
            if cfg.start[s] && !cfg.blocked[s] {
                r.known[s] = true;
                let lvl = cfg.start_level[s].min(2);
                r.book.levels[s] = lvl;
                r.book.sel[s] = lvl;
                if let Some(m) = self.mc2_mint_rival_manifestation(&r, s) {
                    r.book.ent[s] = m as u16;
                }
            }
        }
        // Authored starting castle: AI-only AND Create-Castle-gated
        // (EF:43775-77).
        if cfg.castle_level > 0 && r.known[2] {
            self.mc2_spawn_authored_castle(&mut r, cfg.castle_level);
        }
        // The post-spawn truce: every OTHER wizard's ledger toward
        // this newcomer = the elevated-but-decaying 40927 (EF:43839).
        for other in &mut self.mc2_rivals {
            other.hate[slot as usize] = HATE_RESPAWN;
        }
        // Team resolver for owner recolors (balls/balloons/flags).
        self.g.rival_ents[slot as usize] = i as u16;
        self.mc2_rivals.push(r);
        self.entities_dirty = true;
    }

    /// A rival-owned class-15 spell manifestation: the token entity
    /// in its owned state (3M), wired at the rival's current tier
    /// (the `sub_55AB0` reify + `SetSpell_6D5E0` pair, Level:1305).
    fn mc2_mint_rival_manifestation(&mut self, r: &Mc2Rival, spell: usize) -> Option<usize> {
        let (x, y, z) = {
            let e = &self.g.ent[r.ent as usize];
            (e.x, e.y, e.z)
        };
        let m = self.mc2_new_spell_token(spell as u8, x, y, z)?;
        {
            let e = &mut self.g.ent[m];
            e.tick70 = (spell as u8).wrapping_mul(3); // owned state
            e.f54 = 64;
            e.id24 = r.ent;
            e.f26 = 0;
            e.f44 = 0;
        }
        self.mc2_rival_set_spell(m, r.book.sel[spell], r.ent);
        Some(m)
    }

    /// `SetSpell_6D5E0` for a rival-owned manifestation: the human
    /// arm ([`World::mc2_set_spell`]) resolves the castle spell's
    /// ladder cost against the HUMAN castle — this one uses the
    /// owner's own castle level.
    pub(crate) fn mc2_rival_set_spell(&mut self, m: usize, tier: u8, own: u16) {
        let spell = self.g.ent[m].model65 as usize;
        let Some(row) = self.g.assets.spells.get(spell).copied() else {
            return;
        };
        let count = (row.byte_0 as i16).max(1);
        let t = (tier as i16).min(count - 1).max(0) as usize;
        if self.g.ent[m].f26 > 0 {
            self.g.ent[m].f44 = (t + 1) as u16;
            return;
        }
        let sub = row.tiers[t];
        let cost = if spell == 2 {
            // The castle upgrade ladder at the OWN castle's level
            // (L:1729-55; castle-less = the first rung). ONE
            // definition per game — see
            // [`crate::mc2::castle::MC2_CASTLE_COST`]. This site and
            // `mc2_castle_ladder_cost` below each carried a
            // HAND-ROLLED copy whose rung 7 read `0x3E8` (= 1000)
            // where Level.cpp:1753 has `default: result =
            // 300000000` — three copies of one ladder, two of them
            // wrong at the sentinel. Corpus-inert (no baked MC2 level
            // ships a level-7 rival castle) but it would have made a
            // maxed rival read its next upgrade as trivially
            // affordable.
            // ⚠ STILL NOT the full retail law: retail prices rivals
            // through `GetSpellManaCost_6D710` itself (Level.cpp:1524
            // via SetSpell), so it also applies the ×320>>8 / ×384>>8
            // TIER MULTIPLY and the castle-less raw `manaCost_6`.
            // Landing those moves rival castle TIMING and has
            // measured mc2l4 exposure — banked, not landed here.
            let lvl = self
                .rival_castle(own)
                .map_or(0, |c| self.g.ent[c].f26.clamp(0, 7) as usize);
            crate::mc2::castle::MC2_CASTLE_COST[lvl] as i32
        } else {
            sub.mana_cost
        };
        let e = &mut self.g.ent[m];
        e.f71 = t as u8;
        e.f30 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
        e.f28 = sub.word_0x18.max(0) as u16;
        e.f59 = (sub.font_type & 1 == 0) as u8;
        e.f136 = sub.max_mana_limit;
        e.max_life = cost.max(0) as u32;
        e.f140 = if e.f28 != 0 {
            cost / e.f28 as i32
        } else {
            cost
        };
    }

    /// The authored starting castle (EF:43779-43819 + castle-builder
    /// §1.2): a (3,2) at the wizard, level = players[color]-1, one
    /// terrain pass per authored level, extents + the HP/CAP ladder
    /// (which reads the owner's Life scalar), FULL stored mana
    /// clamped 320000, sound 30, standing.
    fn mc2_spawn_authored_castle(&mut self, r: &mut Mc2Rival, castle_level: u8) {
        let (wx, wy) = {
            let e = &self.g.ent[r.ent as usize];
            (e.x, e.y)
        };
        let Some(c) = self.g.new_event() else { return };
        {
            let e = &mut self.g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.tick70 = 4; // standing (the buildable steady state)
            e.max_life = 40000;
            e.id24 = r.ent;
            // Even-parity tile-corner snap (the shared castle anchor
            // law, MC1 :44229 = MC2 sub_4A9E0).
            let mut tx = wx >> 8;
            let ty = wy >> 8;
            if (tx.wrapping_add(ty)) & 1 == 1 {
                tx = tx.wrapping_add(1);
            }
            e.dest_x = tx << 8;
            e.dest_y = ty << 8;
        }
        let (sx, sy) = (self.g.ent[c].dest_x, self.g.ent[c].dest_y);
        // The ctor's corner-mean build datum (sub_4AA40 EF:33399) —
        // the painter/leveler read site_z, not the live ground.
        let z = self.g.mc2_castle_site_z((sx >> 8) as u8, (sy >> 8) as u8);
        self.g.ent[c].site_z = z;
        self.g.link(c, sx, sy, z);
        self.g.refill_life(c);
        // The team flag: retail `+90 += TransformPlayerColorIndex`
        // (EF:61133) — flag family 177 + COLOR_ART[slot] (the MC2
        // stage pieces carry the visible castle).
        self.g
            .mc2_set_sprite(c, 177 + crate::mc2::color_art(r.slot) as u16);
        let lvl = (castle_level - 1).min(7);
        self.g.ent[c].f26 = lvl as i16;
        // One BUILD00 terrain pass per authored level (the EF:43787
        // j-loop over sub_36FC0): the repaint painter at the final
        // level stamps the full footprint; settle it synchronously
        // (a load-time stamp, like retail's pre-play passes).
        self.g.mc2_spawn_castle_painter(c, true);
        for _ in 0..4096 {
            let mut live = false;
            for j in 1..self.g.ent.len() {
                if self.g.ent[j].class64 == 10
                    && self.g.ent[j].model65 == 42
                    && self.g.ent[j].flags & 0x400 == 0
                {
                    self.g.mc2_castle_painter_tick(j);
                    live = true;
                }
            }
            if !live {
                break;
            }
        }
        // Painter completion signals the castle f59 = 2 (pass-done);
        // it is standing already — clear the build scratch.
        self.g.ent[c].tick70 = 4;
        self.g.ent[c].f59 = 0;
        self.g.ent[c].f50 = 0;
        for j in 1..self.g.ent.len() {
            if self.g.ent[j].class64 == 10
                && self.g.ent[j].model65 == 42
                && self.g.ent[j].flags & 0x400 != 0
            {
                self.free_slot(j);
            }
        }
        // Extents + ladder (Life-scaled HP) + the stage pieces.
        self.g.mc2_castle_extents(c, lvl);
        self.g.mc2_castle_ladder(c);
        self.g.mc2_castle_stages(c);
        // Spawns FULL of mana, clamped 320000 (EF:43812-17).
        let cap = self.g.ent[c].f136;
        self.g.ent[c].f140 = cap.clamp(0, 320_000);
        self.g.snd(30, c);
        self.terrain_dirty = true;
        let _ = r;
    }

    // ---- the per-tick brain (sub_12910 EF:5243) --------------------------

    /// Class-3 model<=1 pool dispatch on the MC2 column: resolve the
    /// rival record; a level-authored husk with no record stands.
    pub(crate) fn mc2_rival_entity_tick(&mut self, i: usize) {
        let Some(ri) = self.mc2_rivals.iter().position(|r| r.ent as usize == i) else {
            return;
        };
        if self.mc2_rivals[ri].eliminated {
            return;
        }
        match self.g.ent[i].tick70 {
            // Death fall (action 2, sub_5E310 EV:2882).
            2 => self.mc2_rival_death_fall(ri, i),
            // Dead-wait (action 3, sub_5E7C0 EV:2895).
            3 => self.mc2_rival_dead_wait(ri, i),
            // Alive (action 1).
            _ => self.mc2_rival_alive(ri, i),
        }
    }

    /// Conformance re-anchor (the MC1 rival-freeze fix's MC2 twin,
    /// engine/world/conformance.rs): point the brain record at the
    /// imported carpet slot and reseed the motion/economy lanes the
    /// per-tick arms consume — without this every imported rival
    /// carpet was a frozen husk (the dispatch above keys on `ent`,
    /// which the world-build seeded with fresh spawn slots).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reanchor_mc2_rival(
        &mut self,
        ri: usize,
        ent: u16,
        vdes: i16,
        strafe: i16,
        grace: u16,
        mana: u32,
        mana_max: u32,
        mana_delta: i32,
    ) {
        let r = &mut self.mc2_rivals[ri];
        r.ent = ent;
        r.eliminated = ent == 0;
        if ent == 0 {
            return;
        }
        r.vdes = vdes;
        r.strafe = strafe;
        r.grace = grace;
        r.mana = mana;
        r.mana_max = mana_max;
        r.mana_delta = mana_delta;
    }

    /// Conformance re-anchor, decision half: reconstruct the wizard-
    /// extension AI lanes so the imported rival resumes mid-decision
    /// instead of re-deciding from a world-build default. The retail
    /// dispatch reads `byte_0x1C1_449` FIRST and the selector writes
    /// it LAST (EF:5252 vs :5517-70), so an un-imported state re-runs
    /// the whole cascade on the pair's tick and the replayed rival
    /// makes its OWN decision — the residue the freeze fix left.
    ///
    /// The target and the scouted site ride the wizard ENTITY
    /// (`word_0x96_150`/`word_0x98_152` and `axis_0x9A_154x`,
    /// EF:6114-15), already imported by `import_ent`; everything else
    /// lives in the player block's `type_str_164` (+998) — see
    /// [`mgc_formats::mgcr::RetailPlayerMc2`] for the lane map.
    ///
    /// The signature is imported RAW rather than recomputed: it is
    /// retail's staleness detector (`sub_14C60` fails the target when
    /// the stored word no longer matches the slot's id+model+class,
    /// EF:6701), so recomputing would silently revive a target retail
    /// had already dropped. The human's carpet is out-of-pool here, so
    /// its target takes the port's [`PLAYER_TARGET`] convention on
    /// both lanes (the alive test ignores the signature for it).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reanchor_mc2_rival_ai(
        &mut self,
        ri: usize,
        ai: &Mc2RivalAi,
        book: &Mc2Spellbook,
    ) {
        let r = &mut self.mc2_rivals[ri];
        if r.eliminated {
            return;
        }
        r.state = Mc2AiState::from_retail(ai.state);
        r.target = ai.target;
        r.target_sig = if ai.target == PLAYER_TARGET {
            PLAYER_TARGET
        } else {
            ai.target_sig
        };
        r.site = ai.site;
        r.burst = ai.burst;
        r.poverty = ai.poverty != 0;
        r.cooldown = ai.cooldown;
        r.hate = ai.hate;
        for (w, &v) in r.war.iter_mut().zip(ai.war.iter()) {
            *w = v != 0;
        }
        r.weave = ai.weave;
        r.weave_dir = ai.weave_dir;
        r.avoid = ai.avoid;
        r.avoid_exit = ai.avoid_exit;
        r.agg = ai.aggression;
        r.per = ai.perception;
        r.refl = ai.reflexes;
        r.life_scale = ai.life_scale;
        // The book: `SpellsEnabled_0x333` is the live manifestation
        // slot, and DEATH rewrites every owned entry to the boolean
        // marker 1 (EF:60147) — imported verbatim, quirk included,
        // because retail's own dead-window reads index the pool with
        // that 1. `known` is the nonzero test, which is exactly what
        // the marker encodes.
        r.book = *book;
        for s in 0..MC2_SPELLS {
            r.known[s] = book.ent[s] != 0;
        }
    }

    /// Housekeeping `sub_12A70` (EF:5320) + the state dispatch.
    fn mc2_rival_alive(&mut self, ri: usize, i: usize) {
        // Burst lockout recovery (EF:5357).
        if self.mc2_rivals[ri].burst < 0 {
            self.mc2_rivals[ri].burst += 1;
        }
        // Recast cooldowns (EF:5361-63).
        for c in self.mc2_rivals[ri].cooldown.iter_mut() {
            *c = c.saturating_sub(1);
        }
        self.mc2_rival_hate_decay(ri);

        // At the own castle: grace pinned 2 + the mailbox memset —
        // the AI DISCARDS damage at home (EF:5397-5414; the FORWARD
        // into the castle is human-only, EF:59961).
        let castle = self.rival_castle(self.mc2_rivals[ri].ent);
        let at_castle = castle.is_some_and(|c| {
            let (ex, ey) = (self.g.ent[i].x, self.g.ent[i].y);
            let e = &self.g.ent[c];
            ((ex.wrapping_sub(e.x) as i16).unsigned_abs()) <= e.f80
                && ((ey.wrapping_sub(e.y) as i16).unsigned_abs()) <= e.f82
        });
        if at_castle {
            self.mc2_rivals[ri].grace = self.mc2_rivals[ri].grace.max(2);
        }
        if self.mc2_rivals[ri].grace > 0 {
            self.mc2_rivals[ri].grace -= 1;
            self.g.ent[i].mail = [(0, 0); 6];
        } else {
            self.mc2_rival_intake(ri, i);
            if self.g.ent[i].act_life < 0 {
                // Lethal -> action 2, the death fall (EF:5416-19).
                self.g.ent[i].tick70 = 2;
                self.g.ent[i].f46 = 0;
                self.g.snd(16, i); // death sound 16 (EF:60039)
                return;
            }
        }

        // Movement filter/step `sub_146F0` (EF:6415).
        self.mc2_rival_movement(ri, i);

        // Regen (EF:5424-5455): delta applied first, then the rate
        // recompute — home /200 (mana min 1000), afield /2000 mana
        // (min 100) and /500 life. The dolmen-shrine flag (+17 0x10,
        // our 0x1000 — stamped by AddDolmen02_02's sweep) rides the
        // same fork and is consumed (cleared) by the fast branch
        // (EF:5438-45).
        let at_shrine = self.g.ent[i].flags & 0x1000 != 0;
        {
            let r = &mut self.mc2_rivals[ri];
            let stepped = r.mana as i64 + r.mana_delta as i64;
            r.mana = stepped.clamp(0, r.mana_max as i64) as u32;
            r.mana_delta = if at_castle || at_shrine {
                ((r.mana_max / 200) as i32).max(1000)
            } else {
                ((r.mana_max / 2000) as i32).max(100)
            };
        }
        if at_castle || at_shrine {
            self.g.ent[i].flags &= !0x1000;
        }
        {
            let max = self.g.ent[i].max_life as i32;
            let heal = if at_castle || at_shrine {
                max / 200
            } else {
                max / 500
            };
            self.g.ent[i].act_life = (self.g.ent[i].act_life + heal).min(max);
        }

        // Buff windows from the manifestations.
        self.mc2_rival_buffs(ri);

        // Decision-cadence work (EF:5458-77): the reactive
        // anti-projectile defense (sub_15CB0 chain — open-closure
        // §3) + heal-when-hurt (spell 5).
        let think = self.g.ent[i].f63 % self.mc2_rivals[ri].think_period() == 0;
        if think {
            self.mc2_rival_react_defense(ri, i);
            if self.g.ent[i].act_life < self.g.ent[i].max_life as i32 {
                self.mc2_rival_walk_cast(ri, i, 5);
            }
        }

        // Altitude hard clamp to the behavior-row band (EF:5482-86).
        {
            let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
            let ground = self.g.ground_z(self.g.ent[i].x, self.g.ent[i].y) as i16;
            let z = &mut self.g.ent[i].z;
            *z = (*z).clamp(
                ground.saturating_add(row.v_12),
                ground.saturating_add(row.v_10),
            );
        }

        // State handler, then the selector re-runs (every handler
        // tail-calls sub_12E70; Fresh runs it twice, EF:5255-56);
        // the water steer closes every state handler (EF:7879).
        let fresh = self.mc2_rivals[ri].state == Mc2AiState::Fresh;
        self.mc2_rival_state_tick(ri, i, think);
        self.mc2_rival_water_steer(ri, i);
        self.mc2_rival_selector(ri, i, think);
        if fresh {
            self.mc2_rival_selector(ri, i, think);
        }
        self.entities_dirty = true;
    }

    /// Hate regression toward neutral (EF:5377-93): below rises by
    /// agg+1, above decays by 256-agg — the war flag pins it.
    ///
    /// FROM-BINARY CORROBORATED (NETHERW.EXE `sub_12A70`, linear
    /// 0x12A70 = file 0x37270 by the banked LE recipe `0x34800 +
    /// (linear − 0x10000)`). remc2's transcription reads
    /// `array_0x1FC_508[4·i]` on the right-hand side while writing
    /// `[4·i+4]` — an eight-byte-shifted accumulator
    /// (`hate[p] = agg + 1 + hate[p−1]`) that would make the ladder
    /// leak across pairs. **It is a decompiler artifact.** The shipped
    /// loop reads and writes the SAME element:
    ///
    /// ```text
    ///   lea  esi,[ecx+eax]          ; esi = playerRec + 8·i
    ///   mov  cx,[esi+0x204]         ; READ hate[i]
    ///   cmp  cx,0x601f
    ///   jnc  .above                 ; unsigned >= neutral
    ///   mov  ax,[eax+0x242]         ; aggression (per-PLAYER, not indexed)
    ///   inc  eax
    ///   add  ecx,eax                ; hate[i] + agg + 1
    ///   mov  [esi+0x204],cx         ; WRITE hate[i]
    /// ```
    ///
    /// …then `cmp/jna` clamps down to 0x601F; the `.above` arm tests
    /// `word [esi+0x206]` (the war flag, the pair record's second
    /// word) and only on zero does `sub [esi+0x204],ax` with
    /// `ax = 0x100 − agg`, clamping back UP to 0x601F. Both
    /// comparisons are UNSIGNED and strict — exactly the per-player
    /// form below. No port change owed.
    fn mc2_rival_hate_decay(&mut self, ri: usize) {
        let (agg, war) = (self.mc2_rivals[ri].agg, self.mc2_rivals[ri].war);
        for (p, h) in self.mc2_rivals[ri].hate.iter_mut().enumerate() {
            if *h < HATE_NEUTRAL {
                *h = (*h + agg + 1).min(HATE_NEUTRAL);
            } else if *h > HATE_NEUTRAL && !war[p] {
                *h = h.saturating_sub(256 - agg).max(HATE_NEUTRAL);
            }
        }
    }

    /// The shared damage intake `sub_5EFA0` (EF:60613) on the rival's
    /// mailbox: steal channel, shield quarter paid by mana, killer
    /// latch. Hate feed rides here (APPROX: retail's per-projectile
    /// scan sub_159E0 — same inputs, slightly earlier; the MC1-column
    /// position).
    fn mc2_rival_intake(&mut self, ri: usize, i: usize) {
        // ch3 steal-mana (EF:60666 -> sub_61050): the MC2 steal
        // drains the victim's CASTLE by percent (open-closure §5);
        // no ported caster emits the channel yet (spell 0xD's effect
        // is 4.6) — the carried-mana fallback keeps the mailbox
        // drained if anything writes it.
        let (steal_amt, steal_src) = self.g.ent[i].mail[3];
        if steal_src != 0 {
            let take = (steal_amt).min(self.mc2_rivals[ri].mana);
            self.mc2_rivals[ri].mana -= take;
            self.credit_wizard_mana(steal_src, take);
            self.g.ent[i].mail[3] = (0, 0);
        }
        // ch0 damage.
        let (amt, src) = self.g.ent[i].mail[0];
        if src == 0 && amt == 0 {
            return;
        }
        self.g.ent[i].mail[0] = (0, 0);
        let mut dmg = amt.min(i32::MAX as u32) as i32;
        if dmg <= 0 {
            return;
        }
        // Shield: the two-stage absorb (EF:60676-93): an ARMED shield
        // NULLS the hit outright and promotes to CHARGED; a CHARGED
        // shield quarters the hit, pays the quarter from mana and is
        // spent. (Retail calls the shield-XP award here too — a
        // structural no-op for rivals through sub_6D8B0's model-0
        // guard.)
        if self.mc2_rivals[ri].shield {
            match self.mc2_rivals[ri].shield_state {
                1 => {
                    self.mc2_rivals[ri].shield_state = 2;
                    dmg = 0;
                }
                2 => {
                    dmg /= 4;
                    let pay = (dmg as u32).min(self.mc2_rivals[ri].mana);
                    self.mc2_rivals[ri].mana -= pay;
                    self.mc2_rivals[ri].shield_state = 0;
                }
                _ => {}
            }
        }
        self.g.ent[i].act_life -= dmg;
        self.g.ent[i].f38 = src; // killer latch (word_0x24_36)
        // Wizard hit sound rand 54..57 on the entity LCG (EF:60712-13).
        let hs = 54 + (self.g.ent_rand(i) & 3) as u8;
        self.g.snd(hs, i);
        // Hate feed (+3000 heavy / +500 base folded to the heavy
        // rate on the source owner; the MC1-column APPROX).
        if let Some(shooter) = self.owner_slot_of_source(src) {
            self.mc2_rival_add_hate(ri, shooter, 3000);
        }
    }

    /// Ledger bump + the wealth-scaled war latch (EF:6263/7399):
    /// hate > 50000 - targetMaxMana/10 * agg/255.
    pub(crate) fn mc2_rival_add_hate(&mut self, ri: usize, shooter: u8, amount: u16) {
        if shooter as usize >= 8 || self.mc2_rivals[ri].slot == shooter {
            return;
        }
        let wealth = self.wizard_wealth(shooter);
        let r = &mut self.mc2_rivals[ri];
        r.hate[shooter as usize] = r.hate[shooter as usize].saturating_add(amount);
        let threshold = 50_000u32.saturating_sub(wealth / 10 * r.agg as u32 / 255);
        if r.hate[shooter as usize] as u32 > threshold {
            r.war[shooter as usize] = true;
        }
    }

    /// AI carpet movement `sub_146F0` (EF:6415): band-settle,
    /// always-level forward step, the strafe channel (decay 4/tick),
    /// accel 16/tick, Reflexes-scaled turn clamped to the row caps.
    /// No wall gate — the water steer is the only obstacle law.
    fn mc2_rival_movement(&mut self, ri: usize, i: usize) {
        let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
        let (v10, v12, v14) = (row.v_10, row.v_12, row.v_14);
        let (v2, v4) = (row.v_2, row.v_4);
        let ground = self.g.ground_z(self.g.ent[i].x, self.g.ent[i].y) as i16;
        {
            let e = &mut self.g.ent[i];
            // sub_580E0 (EF:6454): the three-zone band settle.
            if e.z > ground.saturating_add(v10) {
                e.z = e.z.saturating_add(v14);
            } else if e.z > ground.saturating_add(v12) {
                e.z = e.z.saturating_add((v14 as i32 * 25 / 100) as i16);
            }
            if e.z < ground.saturating_add(v12) {
                e.z = ground.saturating_add(v12);
            }
        }
        let (yaw, speed, strafe) = {
            let e = &self.g.ent[i];
            (e.f30, e.f126, self.mc2_rivals[ri].strafe)
        };
        let mut pos = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        Gen::polar_step(&mut pos, yaw, 0, speed);
        if strafe != 0 {
            Gen::polar_step(&mut pos, yaw.wrapping_add(0x200) & 0x7FF, 0, strafe);
            self.mc2_rivals[ri].strafe -= 4 * strafe.signum();
        }
        self.g.move_relink(i, pos.0, pos.1, pos.2);
        {
            let vdes = self.mc2_rivals[ri].vdes;
            let e = &mut self.g.ent[i];
            e.f126 += 16 * (vdes - e.f126).signum();
            // Turn toward the setpoint: err / (8 + (255-Reflexes)/16),
            // clamped to the row's [v_4, v_2] caps (EF:6488-6501).
            let err = Gen::angdist(e.f30, e.f34) as i32;
            let div = 8 + ((255 - self.mc2_rivals[ri].refl as i32) / 16);
            let step = (err / div).clamp(v4 as i32, v2 as i32) as i16;
            let t = Gen::turn_step(e.f30, e.f34, step);
            e.f30 = (e.f30 as i32 + t as i32) as u16 & 0x7FF;
        }
    }

    // ---- the water / obstacle steer (sub_16580 EF:7879 +
    // ---- open-closure §1) -------------------------------------------

    /// Is the tile at pixel (x, y) + tile delta (dx, dy) deep water
    /// (`mapTerrainType == 8` — the ONLY obstacle type)?
    fn mc2_steer_water(&self, tx: i32, ty: i32) -> bool {
        self.g.t.tile_type[tile(tx as u8, ty as u8)] == 8
    }

    /// `sub_16730` (EF:7955) / `sub_16CA0` (EF:8245) — the
    /// four-neighbour probe at an arbitrary tile cursor. Returns the
    /// packed `(exit<<8 | mask)` word: mask bits 1=N, 2=E, 4=S, 8=W
    /// (N-else-S first; the fwd side of the handedness next). The
    /// diagonal escape keys on the remembered exit code.
    fn mc2_steer_probe(&self, tx: i32, ty: i32, right: bool, exit_mem: u8) -> u16 {
        let mut mask: u16 = 0;
        if self.mc2_steer_water(tx, ty - 1) {
            mask = 1;
        } else if self.mc2_steer_water(tx, ty + 1) {
            mask = 4;
        }
        // Forward side then back side, short-circuiting like retail.
        let (fwd, back) = if right { (1, -1) } else { (-1, 1) };
        if self.mc2_steer_water(tx + fwd, ty) {
            mask |= if fwd == 1 { 2 } else { 8 };
            return mask;
        }
        if self.mc2_steer_water(tx + back, ty) {
            mask |= if back == 1 { 2 } else { 8 };
            return mask;
        }
        if mask != 0 {
            return mask;
        }
        // Diagonal escapes, keyed on the FSM's remembered exit.
        let diag = |dx: i32, dy: i32| self.mc2_steer_water(tx + dx, ty + dy);
        if right {
            match exit_mem {
                1 | 9 if diag(-1, -1) => 1544, // (6<<8)|8
                2 | 3 if diag(1, -1) => 3073,  // (12<<8)|1
                4 | 6 if diag(1, 1) => 2306,   // (9<<8)|2
                8 | 0xC if diag(-1, 1) => 772, // (3<<8)|4
                _ => 0,
            }
        } else {
            match exit_mem {
                1 | 3 if diag(1, -1) => 770,   // (3<<8)|2
                2 | 6 if diag(1, 1) => 1540,   // (6<<8)|4
                4 if diag(-1, 1) => 3080,      // (12<<8)|8
                8 | 9 if diag(-1, -1) => 2305, // (9<<8)|1
                _ => 0,
            }
        }
    }

    /// `sub_16E70` (EF:8403) — Bresenham tile raycast: does the line
    /// from (x0,y0) to (x1,y1) cross water?
    fn mc2_steer_crosses_water(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        let (sx, sy) = ((x1 - x0).signum(), (y1 - y0).signum());
        let mut err = dx - dy;
        for _ in 0..512 {
            if self.mc2_steer_water(x, y) {
                return true;
            }
            if x == x1 && y == y1 {
                return false;
            }
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
        false
    }

    /// The steer target's tile: the live brain target, else a point
    /// 8 tiles ahead along the current heading (the no-target guard;
    /// retail reads Entities[word_0x96_150] unconditionally).
    fn mc2_steer_target_tile(&self, ri: usize, i: usize) -> (i32, i32) {
        let t = self.mc2_rivals[ri].target;
        if t == PLAYER_TARGET {
            return (
                (self.human_pose.0 >> 8) as i32,
                (self.human_pose.1 >> 8) as i32,
            );
        }
        if t != 0 && (t as usize) < self.g.ent.len() {
            let e = &self.g.ent[t as usize];
            if e.flags & 0x400 == 0 {
                return ((e.x >> 8) as i32, (e.y >> 8) as i32);
            }
        }
        let e = &self.g.ent[i];
        let mut fwd = (e.x, e.y, e.z);
        Gen::polar_step(&mut fwd, e.f30, 0, 8 * 256);
        ((fwd.0 >> 8) as i32, (fwd.1 >> 8) as i32)
    }

    /// `sub_169C0` (EF:8111) — the situation classifier: 0 clear,
    /// 1/2 commit left/right, 3 freeze. Fresh obstacles ray-march
    /// BOTH detours 40 steps and pick the exit nearer the target.
    fn mc2_steer_classify(&mut self, ri: usize, i: usize) -> u8 {
        let (wx, wy) = ((self.g.ent[i].x >> 8) as i32, (self.g.ent[i].y >> 8) as i32);
        let (tx, ty) = self.mc2_steer_target_tile(ri, i);
        let exit_mem = self.mc2_rivals[ri].avoid_exit;
        match self.mc2_rivals[ri].avoid {
            0 => {
                let mut left = self.mc2_steer_probe(wx, wy, false, exit_mem);
                let mut right = self.mc2_steer_probe(wx, wy, true, exit_mem);
                if left == 0 && right == 0 {
                    return 0;
                }
                // March both detours 40 steps (EF:8143-8166); index
                // the step tables by the code's LOW byte (0 = hold).
                let (mut lx, mut ly) = (wx, wy);
                let (mut lex, mut ley) = (wx, wy);
                if left != 0 {
                    for _ in 0..0x28 {
                        let idx = (left & 0xFF) as usize % 14;
                        lx += STEER_DX_L[idx] as i32;
                        ly += STEER_DY_L[idx] as i32;
                        (lex, ley) = (lx, ly);
                        left = self.mc2_steer_probe(lx, ly, false, (left & 0xFF) as u8);
                    }
                }
                let (mut rx, mut ry) = (wx, wy);
                let (mut rex, mut rey) = (wx, wy);
                if right != 0 {
                    for _ in 0..0x28 {
                        let idx = (right & 0xFF) as usize % 14;
                        rx += STEER_DX_R[idx] as i32;
                        ry += STEER_DY_R[idx] as i32;
                        (rex, rey) = (rx, ry);
                        right = self.mc2_steer_probe(rx, ry, true, (right & 0xFF) as u8);
                    }
                }
                let pick = if left != 0 && right != 0 {
                    // Rect-area proxy for "exit nearer the target"
                    // (EF:8168-73).
                    if (ty - ley).abs() * (tx - lex).abs() > (tx - rex).abs() * (ty - rey).abs() {
                        2
                    } else {
                        1
                    }
                } else if left == 0 {
                    2
                } else {
                    1
                };
                self.mc2_rivals[ri].avoid = pick;
                pick
            }
            1 => {
                let w = self.mc2_steer_probe(wx, wy, false, exit_mem);
                if w & 0xFF00 == 0 || self.mc2_steer_crosses_water(wx, wy, tx, ty) {
                    1
                } else {
                    self.mc2_rivals[ri].avoid = 3;
                    3
                }
            }
            2 => {
                let w = self.mc2_steer_probe(wx, wy, true, exit_mem);
                if w & 0xFF00 == 0 || self.mc2_steer_crosses_water(wx, wy, tx, ty) {
                    2
                } else {
                    self.mc2_rivals[ri].avoid = 3;
                    3
                }
            }
            s => s, // 3..8 handled by the caller
        }
    }

    /// `sub_16580` (EF:7879) — the post-state steer: classify, snap
    /// yaw to the escape table, zero speed on any turn, hold the arc
    /// ~5 ticks, re-detect.
    fn mc2_rival_water_steer(&mut self, ri: usize, i: usize) {
        let fsm = self.mc2_rivals[ri].avoid;
        let class = if fsm <= 2 || fsm >= 8 {
            if fsm >= 8 {
                self.mc2_rivals[ri].avoid = 0;
            }
            self.mc2_steer_classify(ri, i)
        } else {
            3
        };
        let (wx, wy) = ((self.g.ent[i].x >> 8) as i32, (self.g.ent[i].y >> 8) as i32);
        let exit_mem = self.mc2_rivals[ri].avoid_exit;
        let new_yaw = match class {
            0 => {
                self.mc2_rivals[ri].avoid = 0;
                return;
            }
            1 => {
                let w = self.mc2_steer_probe(wx, wy, false, exit_mem);
                let lo = (w & 0xFF) as usize;
                if lo == 0 || lo >= STEER_YAW_L.len() {
                    return;
                }
                self.mc2_rivals[ri].avoid_exit = lo as u8;
                STEER_YAW_L[lo]
            }
            2 => {
                let w = self.mc2_steer_probe(wx, wy, true, exit_mem);
                let lo = (w & 0xFF) as usize;
                if lo == 0 || lo >= STEER_YAW_R.len() {
                    return;
                }
                self.mc2_rivals[ri].avoid_exit = lo as u8;
                STEER_YAW_R[lo]
            }
            _ => {
                // Frozen arc: coast, count toward re-detect.
                self.mc2_rivals[ri].avoid = self.mc2_rivals[ri].avoid.saturating_add(1);
                return;
            }
        };
        let e = &mut self.g.ent[i];
        if e.f30 != new_yaw {
            // A turn = full stop (EF:7899-7902).
            e.f126 = 0;
            self.mc2_rivals[ri].vdes = 0;
        }
        e.f30 = new_yaw;
        e.f34 = new_yaw; // realign the steering setpoint
    }

    // ---- buffs + reactive defense -----------------------------------------

    /// The manifestations' armed windows (the rival's class-15
    /// entities are inert in the world tick — their cast windows count
    /// down here). Retail's class-15 action maintains `word_0x2E_46`
    /// as a live countdown on EVERY spell's manifestation — the
    /// readiness gates (EF:6997/7014/7065) rely on it expiring, so the
    /// homing set {1,9,0x10,0x12,0x13,0x15} re-arms after `f28` ticks
    /// like retail instead of locking for the rival's whole life. Buff
    /// flags read the post-decrement window; Heal (5) heals while
    /// armed.
    /// The duel enforcement's opponent DRAIN (`sub_5DE30`
    /// EF:59930-43): mode >= 1 drains mana by the opponent's regen
    /// rate plus 8 per tick; mode == 2 also drains life by the
    /// regen plus 2. Our `mana_delta` holds the recomputed per-tick
    /// rate (world.rs's regen law) — `max(0)` guards the mid-debit
    /// window. APPROX: the life-regen term uses the afield /500
    /// rate; retail reads the stored `lifeRegen_0x163_355`, which
    /// only differs while the rival sits at its own castle.
    pub(crate) fn mc2_duel_drain(&mut self, opp: u16, mode: u8) {
        let Some(ri) = self.mc2_rivals.iter().position(|r| r.ent == opp) else {
            return;
        };
        let r = &mut self.mc2_rivals[ri];
        let d = r.mana_delta.max(0) + 8;
        r.mana = r.mana.saturating_sub(d as u32);
        if mode == 2 {
            let a = opp as usize;
            let max = self.g.ent[a].max_life as i32;
            self.g.ent[a].act_life -= max / 500 + 2;
        }
    }

    fn mc2_rival_buffs(&mut self, ri: usize) {
        let book = self.mc2_rivals[ri].book.ent;
        // Heal fires on the pre-decrement window (including the 1→0
        // tick) — capture before the countdown pass.
        let heal_live = book[5] != 0 && self.g.ent[book[5] as usize].f26 > 0;
        let own = self.mc2_rivals[ri].ent;
        for (s, m) in book.iter().enumerate() {
            let m = *m as usize;
            if m != 0 && self.g.ent[m].f26 > 0 {
                self.g.ent[m].f26 -= 1;
                if self.g.ent[m].f26 == 0 {
                    // Shield window expiry drops the absorb stages.
                    if s == 6 {
                        self.mc2_rivals[ri].shield_state = 0;
                    }
                    // A tier queued mid-effect (`SetSpell`'s f44 =
                    // t+1 stash) applies at window expiry — the
                    // retail word_0x2C_44 drain (Level:1505-18; the
                    // defense state's disguise re-pick relies on it).
                    if self.g.ent[m].f44 != 0 {
                        let queued = (self.g.ent[m].f44 - 1) as u8;
                        self.g.ent[m].f44 = 0;
                        self.mc2_rival_set_spell(m, queued, own);
                    }
                }
            }
        }
        let live = |g: &Gen, spell: usize| -> bool {
            let m = book[spell] as usize;
            m != 0 && g.ent[m].f26 > 0
        };
        let shield = live(&self.g, 6);
        let rebound = live(&self.g, 8);
        let invisible = live(&self.g, 0xB);
        // (3 = the approach boost window, read by the movers.)
        // Heal channel (5): heal while the window is live (the shared
        // effect-state law; rate APPROX maxLife/20 per armed tick —
        // the MC1-column rate, MC2 numeric trace not yet pinned).
        if heal_live {
            let i = self.mc2_rivals[ri].ent as usize;
            let max = self.g.ent[i].max_life as i32;
            self.g.ent[i].act_life = (self.g.ent[i].act_life + max / 20).min(max);
        }
        {
            let r = &mut self.mc2_rivals[ri];
            r.shield = shield;
            r.invisible = invisible;
            r.rebound = rebound;
        }
        // Mirror the cloak onto the entity's 0x20 draw/targeting bit
        // while alive (death owns the bit in actions 2/3).
        let i = self.mc2_rivals[ri].ent as usize;
        if self.g.ent[i].tick70 == 1 {
            if invisible {
                self.g.ent[i].flags |= 0x20;
            } else {
                self.g.ent[i].flags &= !0x20;
            }
        }
    }

    /// The reactive anti-projectile defense (`sub_15CB0` +
    /// `sub_15D20` + `sub_15D40`, open-closure §3): nearest class-9
    /// entity homing on me within 5120² -> strafe 80 (only when not
    /// mid water-steer); within 1024² -> cast by threat model
    /// (0|3 -> rebound 8 else shield 6; 4 -> shield 6).
    fn mc2_rival_react_defense(&mut self, ri: usize, i: usize) {
        let me = self.mc2_rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let mut best: Option<(usize, i32)> = None;
        for j in 1..self.g.ent.len() {
            let e = &self.g.ent[j];
            if e.class64 != 9 || e.flags & 0x400 != 0 || e.f146 != me {
                continue;
            }
            let d2 = Gen::dist2_sq(px, py, e.x, e.y);
            if d2 < 0x190_0000 && best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        let Some((threat, d2)) = best else { return };
        if self.mc2_rivals[ri].avoid == 0 {
            self.mc2_rivals[ri].strafe = 80;
        }
        if d2 < 0x10_0000 {
            let model = self.g.ent[threat].model65;
            match model {
                0 | 3 => {
                    // Rebound tier-walk; shield falls back ONLY when
                    // no rebound tier PROBED castable (the a1-capture
                    // law, EF:7518-43) — a probe that passed but a
                    // cast that then whiffed does NOT re-open the
                    // shield fallback.
                    let mut probed8 = false;
                    let mut tier = self.mc2_rivals[ri].book.levels[8] as i16;
                    while tier >= 0 {
                        if self.mc2_rival_tier_probe(ri, tier, 8) == 8 {
                            probed8 = true;
                            self.mc2_rival_cast(ri, i, 8);
                            break;
                        }
                        tier -= 1;
                    }
                    if !probed8 {
                        self.mc2_rival_walk_cast(ri, i, 6);
                    }
                }
                4 => {
                    self.mc2_rival_walk_cast(ri, i, 6);
                }
                _ => {}
            }
        }
    }

    // ---- the decision selector cascade (sub_12E70 EF:5495) ----------------

    fn mc2_rival_selector(&mut self, ri: usize, i: usize, think: bool) {
        // 1. Need a castle (sub_13B00 EF:6056) — every tick.
        let castle = self.rival_castle(self.mc2_rivals[ri].ent);
        if castle.is_none()
            && self.mc2_rivals[ri].known[2]
            && self.mc2_rival_afford_castle(ri)
            && self.mc2_rival_scout_site(ri, i)
        {
            self.mc2_rivals[ri].state = Mc2AiState::Build;
            return;
        }
        // 2. Flee home hurt (sub_13DC0 EF:6163) — every tick. The
        // steer target = the OWN castle (EF:6174-75 — the water
        // detour scanner walks toward it).
        if let Some(c) = castle {
            if self.g.ent[i].act_life < (self.g.ent[i].max_life / 2) as i32 {
                self.mc2_set_rival_state(ri, Mc2AiState::Home, c as u16);
                return;
            }
        }
        if !think {
            return;
        }
        // 3. Upgrade the castle (sub_13C50 EF:6107).
        if let Some(c) = castle {
            if self.mc2_rivals[ri].cooldown[2] == 0
                && self.g.ent[c].tick70 == 4
                && self.g.ent[c].f50 == 0
                && self.g.ent[c].f26 < 7
                && self.mc2_rival_afford_castle(ri)
                && self.g.mc2_castle_space_ok(c)
            {
                // Steer target = the own castle (EF:6114-15).
                self.mc2_set_rival_state(ri, Mc2AiState::Upgrade, c as u16);
                return;
            }
        }
        // 4. Raid an enemy castle (sub_13E40 EF:6182).
        if self.mc2_rival_owns_any(ri, &OFFENSE_RAID) && self.mc2_rival_pick_castle(ri, i) {
            return;
        }
        // 5. Attack an enemy wizard (sub_14030 EF:6233).
        if self.mc2_rival_owns_any(ri, &OFFENSE_ATTACK) && self.mc2_rival_pick_wizard(ri, i) {
            return;
        }
        // 6. Intercept a fat enemy balloon (sub_14250 EF:6292).
        if self.mc2_rival_owns_any(ri, &OFFENSE_ATTACK) && self.mc2_rival_pick_balloon(ri, i) {
            return;
        }
        // 7. Reactive defense (sub_15FC0 EF:7616) — the MC2-native
        // cascade placement (brain §1.3): a live enemy wizard close
        // by flips to the dodge state.
        if self.mc2_rival_pick_defense(ri, i) {
            return;
        }
        // 8. Claim mana balls (sub_13CE0 EF:6122): needs possess-1;
        // with the castle spell known, only while the ceiling is at
        // or under the castle spell's CURRENT ladder cost — the
        // economy loop re-opens after every upgrade.
        if self.mc2_rivals[ri].known[1]
            && (!self.mc2_rivals[ri].known[2] || {
                let cost = self.mc2_castle_ladder_cost(ri) as u32;
                self.mc2_rivals[ri].mana_max <= cost
            })
            && self.mc2_rival_pick_ball(ri, i)
        {
            return;
        }
        // 9. Hunt any mana holder (sub_14530 EF:6341).
        if self.mc2_rival_owns_any(ri, &OFFENSE_ATTACK) && self.mc2_rival_pick_mana(ri, i) {
            return;
        }
        // 10. Idle (sub_14630 EF:6383): hurt + castle → home (steer
        // target = the castle, EF:6394-96); else cruise.
        if let (Some(c), true) = (
            castle,
            self.g.ent[i].act_life < self.g.ent[i].max_life as i32,
        ) {
            self.mc2_set_rival_state(ri, Mc2AiState::Home, c as u16);
        } else {
            self.mc2_rivals[ri].state = Mc2AiState::Cruise;
        }
    }

    pub(crate) fn mc2_set_rival_state(&mut self, ri: usize, s: Mc2AiState, target: u16) {
        self.mc2_rivals[ri].state = s;
        self.mc2_rivals[ri].target = target;
        self.mc2_rivals[ri].target_sig = self.mc2_target_sig(target);
    }

    /// Target signature `sub_14C40` (EF:6701): id + model + class<<7.
    fn mc2_target_sig(&self, target: u16) -> u16 {
        if target == 0 {
            return 0;
        }
        if target == PLAYER_TARGET {
            return PLAYER_TARGET;
        }
        let e = &self.g.ent[target as usize];
        e.id24
            .wrapping_add(e.model65 as u16)
            .wrapping_add((e.class64 as u16) << 7)
    }

    fn mc2_target_alive(&self, target: u16, sig: u16) -> bool {
        if target == 0 {
            return false;
        }
        if target == PLAYER_TARGET {
            return self.player.state == LifeState::Alive;
        }
        let e = &self.g.ent[target as usize];
        e.flags & 0x400 == 0 && e.act_life >= 0 && self.mc2_target_sig(target) == sig
    }

    /// Any of the listed spells owned.
    fn mc2_rival_owns_any(&self, ri: usize, set: &[u8]) -> bool {
        set.iter()
            .any(|&s| self.mc2_rivals[ri].book.ent[s as usize] != 0)
    }

    /// The castle-spell affordability gate: `maxMana >= the castle
    /// spell's current cost` (the ladder at the own castle's level).
    fn mc2_rival_afford_castle(&self, ri: usize) -> bool {
        self.mc2_rivals[ri].mana_max as i32 >= self.mc2_castle_ladder_cost(ri)
    }

    /// The AI's castle affordability price. ONE definition per game —
    /// [`crate::mc2::castle::MC2_CASTLE_COST`]; this was the second
    /// hand-rolled copy whose rung 7 read `0x3E8` (= 1000) instead of
    /// retail's 300,000,000 sentinel. See the note at
    /// `mc2_rival_set_spell` for the tier-multiply scope still owed.
    fn mc2_castle_ladder_cost(&self, ri: usize) -> i32 {
        let lvl = self
            .rival_castle(self.mc2_rivals[ri].ent)
            .map_or(0, |c| self.g.ent[c].f26.clamp(0, 7) as usize);
        crate::mc2::castle::MC2_CASTLE_COST[lvl] as i32
    }

    /// Castle-site scout (sub_13B00 EF:6056-6103): walk the 4x4
    /// sector grid from the OWN sector (+x inner, +y outer, wrapping
    /// mod 4); a sector CORNER qualifies when the nearest foreign
    /// castle is over 12288 away in CHEBYSHEV max(|dx|,|dy|) on the
    /// UNSIGNED axes (`sub_583B0` over `axis_3d` uint16 x/y — the
    /// brain-trace metric OPEN is CLOSED: raw units, not
    /// supercell-scaled). The FIRST qualifying corner wins — no
    /// nearest-ranking, no water veto, no +128 centre offset, no
    /// second candidate (the duplicated check in the decompile is a
    /// loop-unroll artifact, not a second candidate).
    ///
    /// The scan-start sector is NOT the unsigned `pos >> 14`: retail
    /// derives it on the position cast to SIGNED int16 with the
    /// round-toward-zero correction (EF:6076/:6079 —
    /// `(int16_t)(pos - (sign<<14) - sign) >> 14`, sign = the -1/0
    /// indicator), then truncates to a byte. In the upper coordinate
    /// bands that DIFFERS from the unsigned shift: band 2
    /// (0x8000..0xBFFF) starts the scan at corner 3, band 3
    /// (0xC000..0xFFFF) at corner 0. Load-bearing on mc2:04 — Rahn
    /// starts at (64,255), and the signed form points the first
    /// candidate across the y-wrap at tile (64,0), the authored
    /// crater pad on HIS OWN island; the unsigned form scanned from
    /// (64,192) and planted the castle in the open sea.
    pub(crate) fn mc2_rival_scout_site(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        // EF:6074-79 — the signed-trunc sector, kept as the raw byte
        // (the `(x_BYTE)v13 + i` addition wraps mod 256, then & 3).
        let sector = |v: u16| -> u16 {
            let c = v as i16;
            let s: i16 = if c < 0 { -1 } else { 0 };
            (((c - (s << 14) - s) >> 14) as u8) as u16
        };
        let (sx, sy) = (sector(self.g.ent[i].x), sector(self.g.ent[i].y));
        for row in 0..4u16 {
            for col in 0..4u16 {
                let tx = (sx.wrapping_add(col) & 3) << 14;
                let ty = (sy.wrapping_add(row) & 3) << 14;
                let mut near = i32::MAX;
                for j in 1..self.g.ent.len() {
                    let e = &self.g.ent[j];
                    if e.class64 == 3 && e.model65 == 2 && e.flags & 0x400 == 0 && e.id24 != me {
                        let dx = (e.x as i32 - tx as i32).abs();
                        let dy = (e.y as i32 - ty as i32).abs();
                        near = near.min(dx.max(dy));
                    }
                }
                if near > 0x3000 {
                    self.mc2_rivals[ri].site = (tx, ty);
                    return true;
                }
            }
        }
        false
    }

    /// The hate gate: hate[slot] over the wealth-scaled threshold.
    fn mc2_hate_over(&self, ri: usize, slot: u8, wealth: u32) -> bool {
        let r = &self.mc2_rivals[ri];
        let threshold = 50_000u32.saturating_sub(wealth / 10 * r.agg as u32 / 255);
        r.hate[slot as usize] as u32 > threshold
    }

    /// Enemy-castle pick (sub_13E40 EF:6182): hated-and-undefended
    /// (owner > 7680 from its castle and not physically at it) OR
    /// plain poorer (my stored >> theirs + 640*(255-agg)), nearest
    /// within the behavior-row range.
    fn mc2_rival_pick_castle(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let my_castle = self.rival_castle(me);
        // Castle-less but castle-capable: build first (EF:6190).
        if my_castle.is_none() && self.mc2_rivals[ri].known[2] {
            return false;
        }
        let my_stored = my_castle.map_or(0, |c| self.g.ent[c].f140.max(0) as u32);
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32;
        let mut best: Option<(u16, i32)> = None;
        for j in 1..self.g.ent.len() {
            let e = &self.g.ent[j];
            if e.class64 != 3 || e.model65 != 2 || e.flags & 0x400 != 0 || e.id24 == me {
                continue;
            }
            let Some(owner) = self.owner_slot(e.id24) else {
                continue;
            };
            let hated = self.mc2_hate_over(ri, owner, self.wizard_wealth(owner));
            // Undefended: owner far (0x3840000 = 7680², EF:6202) and
            // not at the castle.
            let undefended = self
                .wizard_pos(owner)
                .is_none_or(|(wx, wy, _)| Gen::dist2_sq(e.x, e.y, wx, wy) > 7680 * 7680);
            let poorer = (e.f140.max(0) as u32)
                .saturating_add(640 * (255 - self.mc2_rivals[ri].agg as u32))
                < my_stored;
            if !(hated && undefended) && !poorer {
                continue;
            }
            let d = Gen::dist2_sq(px, py, e.x, e.y);
            if d <= range.saturating_mul(range) && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j as u16, d));
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::RaidCastle, t);
            true
        } else {
            false
        }
    }

    /// Enemy-wizard pick (sub_14030 EF:6233): war | hated | bully the
    /// homeless rich (32*(255-agg) margin); invisible targets are
    /// skipped; nearest within range+10.
    fn mc2_rival_pick_wizard(&mut self, ri: usize, i: usize) -> bool {
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32 + 10;
        let my_mana = self.mc2_rivals[ri].mana;
        let mut best: Option<(u16, i32)> = None;
        let consider = |tgt: u16,
                        x: u16,
                        y: u16,
                        invisible: bool,
                        castle_less: bool,
                        mana: u32,
                        war: bool,
                        hated: bool,
                        best: &mut Option<(u16, i32)>| {
            if invisible {
                return; // spell-0xB targets skipped (EF:6252)
            }
            let bully = castle_less
                && mana.saturating_add(32 * (255 - self.mc2_rivals[ri].agg as u32)) < my_mana;
            if !war && !hated && !bully {
                return;
            }
            let d = Gen::dist2_sq(px, py, x, y);
            if d <= range.saturating_mul(range) && best.is_none_or(|(_, bd)| d < bd) {
                *best = Some((tgt, d));
            }
        };
        if self.player.state == LifeState::Alive {
            let (hx, hy) = (self.human_pose.0, self.human_pose.1);
            consider(
                PLAYER_TARGET,
                hx,
                hy,
                self.player.invisible,
                self.player_castle().is_none(),
                self.player.mana,
                self.mc2_rivals[ri].war[0],
                self.mc2_hate_over(ri, 0, self.player.mana_max),
                &mut best,
            );
        }
        for oj in 0..self.mc2_rivals.len() {
            if oj == ri || self.mc2_rivals[oj].eliminated {
                continue;
            }
            let (slot, ent, mana_max, mana, invis) = {
                let o = &self.mc2_rivals[oj];
                (o.slot, o.ent, o.mana_max, o.mana, o.invisible)
            };
            let e = &self.g.ent[ent as usize];
            if e.tick70 != 1 {
                continue;
            }
            let (ex, ey) = (e.x, e.y);
            consider(
                ent,
                ex,
                ey,
                invis,
                self.rival_castle(ent).is_none(),
                mana,
                self.mc2_rivals[ri].war[slot as usize],
                self.mc2_hate_over(ri, slot, mana_max),
                &mut best,
            );
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::AttackWizard, t);
            true
        } else {
            false
        }
    }

    /// Enemy-balloon pick (sub_14250 EF:6292): hated owner, cargo
    /// over 10*(275-agg), not sitting at its own castle.
    fn mc2_rival_pick_balloon(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32;
        let cargo_gate = 10 * (275 - self.mc2_rivals[ri].agg as u32);
        let mut best: Option<(u16, i32)> = None;
        for j in 1..self.g.ent.len() {
            let e = &self.g.ent[j];
            if e.class64 != 3 || e.model65 != 3 || e.flags & 0x400 != 0 || e.id24 == me {
                continue;
            }
            let Some(owner) = self.owner_slot(e.id24) else {
                continue;
            };
            if !self.mc2_hate_over(ri, owner, self.wizard_wealth(owner)) {
                continue;
            }
            if (e.f140.max(0) as u32) <= cargo_gate {
                continue;
            }
            let home = self.rival_castle(e.id24);
            if home.is_some_and(|c| {
                Gen::dist2_sq(e.x, e.y, self.g.ent[c].x, self.g.ent[c].y) < 2048 * 2048
            }) {
                continue;
            }
            let d = Gen::dist2_sq(px, py, e.x, e.y);
            if d <= range.saturating_mul(range) && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j as u16, d));
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::RaidBalloon, t);
            true
        } else {
            false
        }
    }

    /// Nearest live enemy wizard by 3-D distance — the shared scan of
    /// `sub_15FC0`/`sub_161A0` (EF:7644-59/7782-98): class-3 model 0|1
    /// on a foreign team. NO invisibility filter (retail has none
    /// here) and no hostility gate. Returns (target, pos, dist²).
    fn mc2_nearest_wizard(&self, ri: usize, i: usize) -> Option<(u16, (u16, u16, i16), i64)> {
        let (px, py, pz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        let d3 = |x: u16, y: u16, z: i16| -> i64 {
            let dx = (px.wrapping_sub(x) as i16) as i64;
            let dy = (py.wrapping_sub(y) as i16) as i64;
            let dz = pz as i64 - z as i64;
            dx * dx + dy * dy + dz * dz
        };
        let mut best: Option<(u16, (u16, u16, i16), i64)> = None;
        if self.player.state == LifeState::Alive {
            let (hx, hy, hz) = self.human_pose;
            let d = d3(hx, hy, hz);
            best = Some((PLAYER_TARGET, (hx, hy, hz), d));
        }
        for oj in 0..self.mc2_rivals.len() {
            if oj == ri || self.mc2_rivals[oj].eliminated {
                continue;
            }
            let ent = self.mc2_rivals[oj].ent;
            let e = &self.g.ent[ent as usize];
            if e.tick70 != 1 {
                continue;
            }
            let d = d3(e.x, e.y, e.z);
            if best.is_none_or(|(_, _, bd)| d < bd) {
                best = Some((ent, (e.x, e.y, e.z), d));
            }
        }
        best
    }

    /// DEFENSE selector (sub_15FC0 EF:7616, cascade step 7): the
    /// metamorph MIMICRY pick — blend into the local fauna. Requires
    /// Metamorph (4) owned; finds the nearest enemy wizard within
    /// 0x1400 (3-D), then the nearest disguisable creature (the
    /// [`DISGUISE_MODELS`] table, walked in order) within 0x1400 OF
    /// THAT WIZARD; pre-arms the matching metamorph tier on the
    /// manifestation (SetSpell — mid-effect queues via f44) and
    /// targets the CREATURE (the disguise anchor). A live disguise
    /// window with a valid target signature holds the state without
    /// rescanning (EF:7639/7717). No hostility filter — any foreign
    /// wizard nearby triggers the mimicry (no war gate here).
    pub(crate) fn mc2_rival_pick_defense(&mut self, ri: usize, i: usize) -> bool {
        let m4 = self.mc2_rivals[ri].book.ent[4] as usize;
        if m4 == 0 {
            return false; // metamorph not owned (EF:7641-43)
        }
        if self.g.ent[m4].f26 > 0
            && self.mc2_target_alive(self.mc2_rivals[ri].target, self.mc2_rivals[ri].target_sig)
        {
            self.mc2_rivals[ri].state = Mc2AiState::Defense;
            return true;
        }
        let Some((_, (wx, wy, wz), wd)) = self.mc2_nearest_wizard(ri, i) else {
            return false;
        };
        if wd > 0x1400 * 0x1400 {
            return false; // no wizard close enough (EF:7660-63)
        }
        // Scan 2: the disguise anchor — nearest table-model creature
        // to the WIZARD (not to self, EF:7673). Corpse states are
        // excluded like retail's per-model alive lists (EF:39987-40008
        // skip actions 0xB4/0xE8/0xEA).
        let me = self.mc2_rivals[ri].ent;
        let mut best: Option<(usize, i64, u8)> = None;
        for &dm in &DISGUISE_MODELS {
            for j in 1..self.g.ent.len() {
                let e = &self.g.ent[j];
                if e.class64 != 5
                    || e.model65 != dm
                    || e.flags & 0x400 != 0
                    || e.act_life < 0
                    || matches!(e.tick70, 0xB4 | 0xE8 | 0xEA)
                    || e.id24 == me
                {
                    continue;
                }
                let dx = (wx.wrapping_sub(e.x) as i16) as i64;
                let dy = (wy.wrapping_sub(e.y) as i16) as i64;
                let dz = wz as i64 - e.z as i64;
                let d = dx * dx + dy * dy + dz * dz;
                if best.is_none_or(|(_, bd, _)| d < bd) {
                    best = Some((j, d, dm));
                }
            }
        }
        let Some((anchor, d, dm)) = best else {
            return false;
        };
        if d >= 0x1400 * 0x1400 {
            return false; // no plausible fauna near the threat
        }
        self.mc2_rival_set_spell(m4, mc2_disguise_tier(dm), me);
        self.mc2_set_rival_state(ri, Mc2AiState::Defense, anchor as u16);
        true
    }

    /// Mana-ball pick (sub_148E0 EF:6518-6609): walk the class-10
    /// sphere chain — the (10,39)/(10,40)
    /// spheres in pool order, then the (10,57) randoms (retail's
    /// list is built in that order). A model-57 sphere BREAKS the
    /// whole walk on a Perception roll, keeping the best so far (a
    /// failed roll evaluates it like any other ball). Skip own
    /// claims. A ball owned by a NOT-hated wizard is taken only
    /// when isolated — the nearest wizard TO THE BALL (self
    /// excluded) beyond 5120; NO wizard in the world skips it too
    /// (the retail quirk) — and not parked at the nearest non-own
    /// castle (bbox overlap). A HATED owner's balls skip both tests
    /// and rank from the rival's OWN castle (castle-less anchors to
    /// self — retail reads the Entities[0] sentinel there,
    /// documented idealization). Unowned balls rank from self. An
    /// empty walk falls back to the nearest class-5 model-22
    /// flying-chain ball not already owned.
    fn mc2_rival_pick_ball(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let per = self.mc2_rivals[ri].per;
        let own_castle = self.rival_castle(me);
        let anchor = own_castle
            .map(|c| (self.g.ent[c].x, self.g.ent[c].y))
            .unwrap_or((px, py));
        let mut best: Option<(u16, i32)> = None;
        'walk: for pass in 0..2u8 {
            for j in 1..self.g.ent.len() {
                let (model, claim, bx, by) = {
                    let e = &self.g.ent[j];
                    if e.class64 != 10 || e.flags & 0x400 != 0 {
                        continue;
                    }
                    (e.model65, e.f144, e.x, e.y)
                };
                let wanted = if pass == 0 {
                    matches!(model, 39 | 40)
                } else {
                    model == 57
                };
                if !wanted || claim == me {
                    continue;
                }
                if model == 57 && ((self.g.ent_rand(i) % 255) as u16) < per {
                    break 'walk; // the 57-break (EF:6544-49)
                }
                let score = if let Some(o) = self.owner_slot(claim) {
                    let hated = self.mc2_hate_over(ri, o, self.wizard_wealth(o));
                    if !hated {
                        // Nearest wizard to the BALL, self excluded.
                        let mut wnear: Option<i32> = None;
                        if self.player.state == LifeState::Alive {
                            wnear =
                                Some(Gen::dist2_sq(bx, by, self.human_pose.0, self.human_pose.1));
                        }
                        for oj in 0..self.mc2_rivals.len() {
                            if oj == ri || self.mc2_rivals[oj].eliminated {
                                continue;
                            }
                            let oe = &self.g.ent[self.mc2_rivals[oj].ent as usize];
                            if oe.tick70 != 1 {
                                continue;
                            }
                            let d = Gen::dist2_sq(bx, by, oe.x, oe.y);
                            if wnear.is_none_or(|w| d < w) {
                                wnear = Some(d);
                            }
                        }
                        let Some(wd) = wnear else {
                            continue; // no wizard at all → skip (EF:6560-62)
                        };
                        if wd <= 5120 * 5120 {
                            continue; // guarded (EF:6565-69)
                        }
                        // At-castle skip vs the nearest non-own castle.
                        let mut cnear: Option<(usize, i32)> = None;
                        for k in 1..self.g.ent.len() {
                            let c = &self.g.ent[k];
                            if c.class64 == 3
                                && c.model65 == 2
                                && c.flags & 0x400 == 0
                                && own_castle != Some(k)
                            {
                                let d = Gen::dist2_sq(bx, by, c.x, c.y);
                                if cnear.is_none_or(|(_, bd)| d < bd) {
                                    cnear = Some((k, d));
                                }
                            }
                        }
                        if let Some((k, _)) = cnear {
                            let c = &self.g.ent[k];
                            if ((bx.wrapping_sub(c.x) as i16).unsigned_abs()) <= c.f80
                                && ((by.wrapping_sub(c.y) as i16).unsigned_abs()) <= c.f82
                            {
                                continue; // parked at a castle (EF:6570-71)
                            }
                        }
                        Gen::dist2_sq(px, py, bx, by)
                    } else {
                        Gen::dist2_sq(anchor.0, anchor.1, bx, by)
                    }
                } else {
                    Gen::dist2_sq(px, py, bx, by)
                };
                if best.is_none_or(|(_, bd)| score < bd) {
                    best = Some((j as u16, score));
                }
            }
        }
        // Second-chain fallback: the wild flying-chain balls
        // (class 5 model 22 — EF:6593-6604).
        if best.is_none() {
            for j in 1..self.g.ent.len() {
                let e = &self.g.ent[j];
                if e.class64 != 5
                    || e.model65 != 22
                    || e.flags & 0x400 != 0
                    || e.act_life < 0
                    || e.f144 == me
                {
                    continue;
                }
                let d = Gen::dist2_sq(px, py, e.x, e.y);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j as u16, d));
                }
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::Possess, t);
            true
        } else {
            false
        }
    }

    /// Mana-holder hunt (sub_14530 EF:6341): any other-team creature
    /// with mana, nearest to the own castle (or self), no range cap.
    fn mc2_rival_pick_mana(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let anchor = self
            .rival_castle(me)
            .map(|c| (self.g.ent[c].x, self.g.ent[c].y))
            .unwrap_or((self.g.ent[i].x, self.g.ent[i].y));
        let mut best: Option<(u16, i32)> = None;
        for j in 1..self.g.ent.len() {
            let e = &self.g.ent[j];
            if e.class64 != 5 || e.flags & 0x400 != 0 || e.act_life < 0 {
                continue;
            }
            if e.id24 == me || e.f140 <= 0 {
                continue;
            }
            let d = Gen::dist2_sq(anchor.0, anchor.1, e.x, e.y);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j as u16, d));
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::HuntMana, t);
            true
        } else {
            false
        }
    }

    // ---- state handlers ----------------------------------------------------

    pub(crate) fn mc2_rival_state_tick(&mut self, ri: usize, i: usize, think: bool) {
        let needs_target = matches!(
            self.mc2_rivals[ri].state,
            Mc2AiState::Possess
                | Mc2AiState::RaidCastle
                | Mc2AiState::AttackWizard
                | Mc2AiState::RaidBalloon
                | Mc2AiState::HuntMana
                | Mc2AiState::Defense
        );
        if needs_target
            && !self.mc2_target_alive(self.mc2_rivals[ri].target, self.mc2_rivals[ri].target_sig)
        {
            self.mc2_rivals[ri].state = Mc2AiState::Fresh;
            self.mc2_rivals[ri].target = 0;
            return;
        }
        match self.mc2_rivals[ri].state {
            Mc2AiState::Fresh => {}
            // Fly home, hover castle+512, cast the upgrade (sub_12FF0
            // EF:5579; approach 512/2048, speed-up en route).
            Mc2AiState::Upgrade => {
                let Some(c) = self.rival_castle(self.mc2_rivals[ri].ent) else {
                    self.mc2_rivals[ri].state = Mc2AiState::Fresh;
                    return;
                };
                let (cx, cy, cz) = {
                    let e = &self.g.ent[c];
                    (e.x, e.y, e.z)
                };
                if self.mc2_rival_approach(ri, i, cx, cy, 512, 2048) {
                    self.mc2_rival_hover(i, cz.saturating_add(512));
                    self.mc2_rival_walk_cast(ri, i, 2);
                }
            }
            // Fly to the scouted site, plant (sub_13100 EF:5620;
            // approach 2048/4096).
            Mc2AiState::Build => {
                let (sx, sy) = self.mc2_rivals[ri].site;
                if self.mc2_rival_approach(ri, i, sx, sy, 2048, 4096) {
                    self.mc2_rival_walk_cast(ri, i, 2);
                    if self.rival_castle(self.mc2_rivals[ri].ent).is_some() {
                        self.mc2_rivals[ri].state = Mc2AiState::Fresh;
                    }
                }
            }
            // Claim the ball with possess-1 (sub_135C0 EF:5822-68;
            // approach 1024/3072): tier-walk the possess cast in the
            // act zone; a SUCCESSFUL cast aimed strictly under 0x1C
            // writes the claim directly (EF:5849-50 — the rival-side
            // guarantee; the projectile's stamp delivery remains the
            // general law). A whiffed cast hovers at ball z+512. No
            // internal "claimed → done" exit — the selector
            // re-arbitrates and the ball pick skips own claims.
            Mc2AiState::Possess => {
                let t = self.mc2_rivals[ri].target as usize;
                let (tx, ty, tz) = {
                    let e = &self.g.ent[t];
                    (e.x, e.y, e.z)
                };
                self.mc2_rival_face(i, tx, ty);
                if self.mc2_rival_approach(ri, i, tx, ty, 1024, 3072) {
                    if self.mc2_rival_walk_cast(ri, i, 1) {
                        let aim = Gen::angdist(
                            self.g.ent[i].f30,
                            Gen::angle_between(self.g.ent[i].x, self.g.ent[i].y, tx, ty),
                        );
                        if aim < 0x1C {
                            self.g.ent[t].f144 = self.mc2_rivals[ri].ent;
                        }
                    } else {
                        self.mc2_rivals[ri].vdes = 0;
                        self.mc2_rival_hover(i, tz.saturating_add(512));
                    }
                }
            }
            // Castle raid (sub_13710 EF:5872-5913; approach
            // 2048/3584): INSIDE the cast ring the castle-walk pick
            // fires ON CADENCE; a whiffed or unavailable cast hovers
            // at castle z+512. NO ownership write — retail never
            // claims the raided castle.
            Mc2AiState::RaidCastle => {
                let t = self.mc2_rivals[ri].target as usize;
                let (tx, ty, tz) = {
                    let e = &self.g.ent[t];
                    (e.x, e.y, e.z)
                };
                self.mc2_rival_face(i, tx, ty);
                let arrived = self.mc2_rival_approach(ri, i, tx, ty, 2048, 3584);
                if arrived && think {
                    let cast_ok = match self.mc2_rival_attack_pick(ri, false) {
                        Some(s) => self.mc2_rival_cast(ri, i, s),
                        None => false,
                    };
                    if !cast_ok {
                        self.mc2_rivals[ri].vdes = 0;
                        self.mc2_rival_hover(i, tz.saturating_add(512));
                    }
                }
            }
            // Wizard / balloon / mana-holder attack (sub_13890
            // EF:5937-6050; approach 3328/4608): the pick + cast run
            // only INSIDE the ring with burst budget; a landed cast
            // de-latches the war toward ANY wizard target
            // (EF:5966-68); the whiff path stops, weaves (wizard
            // targets only) and z-tracks the target + 512.
            Mc2AiState::AttackWizard | Mc2AiState::RaidBalloon | Mc2AiState::HuntMana => {
                let (tx, ty, tz) = match self.mc2_rivals[ri].target {
                    PLAYER_TARGET => self.human_pose,
                    t => {
                        let e = &self.g.ent[t as usize];
                        (e.x, e.y, e.z)
                    }
                };
                self.mc2_rival_face(i, tx, ty);
                let arrived = self.mc2_rival_approach(ri, i, tx, ty, 3328, 4608);
                if arrived && self.mc2_rivals[ri].burst >= 0 {
                    let fired = match self.mc2_rival_attack_pick(ri, true) {
                        Some(s) => self.mc2_rival_cast(ri, i, s),
                        None => false,
                    };
                    if fired {
                        let slot = match self.mc2_rivals[ri].target {
                            PLAYER_TARGET => Some(0u8),
                            t => self.mc2_rivals.iter().find(|r| r.ent == t).map(|r| r.slot),
                        };
                        if let Some(s) = slot {
                            self.mc2_rivals[ri].war[s as usize] = false;
                        }
                    } else {
                        self.mc2_rivals[ri].vdes = 0;
                        let wizard_target = matches!(self.mc2_rivals[ri].target, PLAYER_TARGET)
                            || self
                                .mc2_rivals
                                .iter()
                                .any(|r| r.ent == self.mc2_rivals[ri].target);
                        if wizard_target {
                            self.mc2_rival_weave(ri, i);
                        }
                        self.mc2_rival_hover(i, tz.saturating_add(512));
                    }
                }
            }
            // Home (sub_133B0 EF:5745; approach 256/2048): cloak-0xB
            // while fleeing; heal up at the castle.
            Mc2AiState::Home => {
                let Some(c) = self.rival_castle(self.mc2_rivals[ri].ent) else {
                    self.mc2_rival_walk_cast(ri, i, 0xB);
                    self.mc2_rivals[ri].state = Mc2AiState::Cruise;
                    return;
                };
                let (cx, cy) = (self.g.ent[c].x, self.g.ent[c].y);
                self.mc2_rival_walk_cast(ri, i, 0xB);
                self.mc2_rival_approach(ri, i, cx, cy, 256, 2048);
                if self.g.ent[i].act_life >= self.g.ent[i].max_life as i32 {
                    self.mc2_rivals[ri].state = Mc2AiState::Fresh;
                }
            }
            // Cruise (sub_13270 EF:5680-5740): a Perception-rolled
            // Fool's-Mana lure when not hovering over the own castle
            // (the tier pick carries retail's verbatim SpellIndex[2]
            // quirk — a single probe at the CASTLE spell's level,
            // EF:5698/5705); else keep the speed-up boost topped
            // (readiness has no cooldown for 3 — the live window is
            // the only re-cast gate); else plain cruise at minSpeed.
            Mc2AiState::Cruise => {
                let me = self.mc2_rivals[ri].ent;
                if ((self.g.ent_rand(i) % 255) as u16) < self.mc2_rivals[ri].per {
                    let over_castle = self.rival_castle(me).is_some_and(|c| {
                        let (ex, ey) = (self.g.ent[i].x, self.g.ent[i].y);
                        let e = &self.g.ent[c];
                        ((ex.wrapping_sub(e.x) as i16).unsigned_abs()) <= e.f80
                            && ((ey.wrapping_sub(e.y) as i16).unsigned_abs()) <= e.f82
                    });
                    if !over_castle {
                        let quirk_tier = self.mc2_rivals[ri].book.levels[2] as i16;
                        if self.mc2_rival_tier_probe(ri, quirk_tier, 0x16) == 0x16
                            && self.mc2_rival_cast(ri, i, 0x16)
                        {
                            return;
                        }
                    }
                }
                let m3 = self.mc2_rivals[ri].book.ent[3] as usize;
                let boosted = m3 != 0 && self.g.ent[m3].f26 > 0;
                if !boosted && self.mc2_rival_cast_ready(ri, 3) {
                    self.mc2_rival_cast(ri, i, 3);
                    return;
                }
                if !boosted {
                    self.mc2_rivals[ri].vdes = self.g.ent[i].f128;
                }
            }
            // Defense (sub_161A0 EF:7724): the metamorph DISGUISE
            // posture — refresh the cast, shadow the anchor creature
            // at z+512 (retail's duplicated climb-step block = two
            // steps/tick), tier-0 heading wiggle, engage a mid-band
            // (0xA00..0x1400) wizard, disguise-scaled speed. No
            // internal band exit — the selector owns leaving the
            // state. The disguise VISUAL (drawing the anchor model in
            // place of the carpet) is presentation-side, APPROX
            // unported.
            Mc2AiState::Defense => {
                let (tx, ty, tz) = match self.mc2_rivals[ri].target {
                    PLAYER_TARGET => self.human_pose,
                    t => {
                        let e = &self.g.ent[t as usize];
                        (e.x, e.y, e.z)
                    }
                };
                // Refresh the disguise (readiness gates: window clear,
                // the 300-tick cooldown, mana, the 0xE3 cone).
                self.mc2_rival_cast(ri, i, 4);
                // Shadow the anchor (EF:7748-71 — the z block runs
                // twice verbatim).
                self.mc2_rival_face(i, tx, ty);
                let step = BEHAVIOR[self.g.ent[i].row156 as usize].v_14.abs().max(1);
                for _ in 0..2 {
                    let want = tz.saturating_add(512);
                    let e = &mut self.g.ent[i];
                    if e.z < want {
                        e.z = e.z.saturating_add(step);
                    } else if e.z > want {
                        e.z = e.z.saturating_sub(step);
                    }
                }
                // Tier-0 (bird) heading wiggle: two LCG draws in
                // retail order (EF:7774-80).
                let m4 = self.mc2_rivals[ri].book.ent[4] as usize;
                let tier0 = m4 != 0 && self.g.ent[m4].f71 == 0;
                if tier0 {
                    let r1 = self.g.ent_rand(i);
                    let v5 = 2 * ((r1 % 0x9D) / 79); // {0, 2}
                    let r2 = self.g.ent_rand(i);
                    let jink = (v5 as i32 - 1) * (r2 % 0x55) as i32;
                    let e = &mut self.g.ent[i];
                    e.f34 = (e.f34 as i32 + jink) as u16 & 0x7FF;
                }
                // Wizard rescan: the mid-band flips to an engage —
                // retarget the WIZARD + attack pick (EF:7799-7809).
                let wiz = self.mc2_nearest_wizard(ri, i);
                let mut in_band = false;
                if let Some((wt, _, wd)) = wiz {
                    if wd > 0xA00 * 0xA00 && wd < 0x1400 * 0x1400 {
                        in_band = true;
                        self.mc2_set_rival_state(ri, Mc2AiState::Defense, wt);
                        self.mc2_rivals[ri].vdes = 0;
                        if let Some(s) = self.mc2_rival_attack_pick(ri, true) {
                            if self.mc2_rival_cast(ri, i, s) {
                                return; // steer + selector still run
                            }
                        }
                    }
                }
                if !in_band {
                    // Face + z-track the wizard (overrides the anchor
                    // heading, EF:7815-40); none → origin, the remc2
                    // //fix for retail's null read.
                    let (wx, wy, wz) = wiz.map(|(_, p, _)| p).unwrap_or((0, 0, 0));
                    self.mc2_rival_face(i, wx, wy);
                    let want = wz.saturating_add(512);
                    let e = &mut self.g.ent[i];
                    if e.z < want {
                        e.z = e.z.saturating_add(step);
                    } else if e.z > want {
                        e.z = e.z.saturating_sub(step);
                    }
                }
                // Disguise-scaled speed (EF:7842-48): the tier-0 bird
                // flies 3x minSpeed, tiers 1/2 plain minSpeed.
                let min = self.g.ent[i].f128;
                self.mc2_rivals[ri].vdes = if tier0 { min.saturating_mul(3) } else { min };
            }
        }
    }

    /// The combat whiff weave (sub_13890 EF:5980-6034), WIZARD
    /// targets only: tick 0 rolls the committed direction on the
    /// entity LCG and snaps the ACTUAL yaw ±512; ticks 1-2 jink the
    /// setpoint ±512 and pulse the actual speed to
    /// 3·minSpeed·Reflexes/255; ticks 3..19 coast; 20 restarts.
    fn mc2_rival_weave(&mut self, ri: usize, i: usize) {
        let cnt = self.mc2_rivals[ri].weave;
        match cnt {
            0 => {
                let r = self.g.ent_rand(i);
                let dir = if (r % 255) >= 127 { 2u8 } else { 1 };
                self.mc2_rivals[ri].weave_dir = dir;
                let e = &mut self.g.ent[i];
                e.f30 = if dir == 2 {
                    e.f30.wrapping_add(512) & 0x7FF
                } else {
                    e.f30.wrapping_sub(512) & 0x7FF
                };
                self.mc2_rivals[ri].weave = 1;
            }
            1..=2 => {
                let jink: i32 = if self.mc2_rivals[ri].weave_dir == 1 {
                    -512
                } else {
                    512
                };
                let refl = self.mc2_rivals[ri].refl as i32;
                let e = &mut self.g.ent[i];
                e.f34 = ((e.f34 as i32 + jink) & 0x7FF) as u16;
                e.f126 = (3 * e.f128 as i32 * refl / 255) as i16;
                self.mc2_rivals[ri].weave = cnt + 1;
            }
            3..=19 => self.mc2_rivals[ri].weave = cnt + 1,
            _ => self.mc2_rivals[ri].weave = 0,
        }
    }

    /// Shared travel helper (sub_14C90 EF:6713): inside arriveR ->
    /// stop, done; else min speed, and beyond boostR cast the
    /// speed-up (spell 3 — the MC2 remap).
    fn mc2_rival_approach(
        &mut self,
        ri: usize,
        i: usize,
        tx: u16,
        ty: u16,
        arrive: i32,
        boost: i32,
    ) -> bool {
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let d2 = Gen::dist2_sq(px, py, tx, ty);
        self.g.ent[i].f34 = Gen::angle_between(px, py, tx, ty);
        if d2 <= arrive.saturating_mul(arrive) {
            self.mc2_rivals[ri].vdes = 0;
            return true;
        }
        self.mc2_rivals[ri].vdes = self.g.ent[i].f128;
        if d2 > boost.saturating_mul(boost) {
            // Speed-up beyond the boost ring, gated on the live
            // window (sub_156F0 — readiness itself has no cooldown
            // for 3, so the window is the only re-cast brake).
            let m3 = self.mc2_rivals[ri].book.ent[3] as usize;
            if m3 == 0 || self.g.ent[m3].f26 == 0 {
                self.mc2_rival_cast(ri, i, 3);
            }
        }
        false
    }

    fn mc2_rival_face(&mut self, i: usize, tx: u16, ty: u16) {
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        self.g.ent[i].f34 = Gen::angle_between(px, py, tx, ty);
    }

    /// Combat hover toward target z + 512 by the row's v_14 step.
    fn mc2_rival_hover(&mut self, i: usize, tz: i16) {
        let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
        let step = row.v_14.abs().max(1);
        let e = &mut self.g.ent[i];
        if e.z < tz {
            e.z = e.z.saturating_add(step);
        } else if e.z > tz {
            e.z = e.z.saturating_sub(step);
        }
    }

    /// The attack-spell picker (sub_15790 EF:7175 wizard /
    /// sub_15910 EF:7246 castle): the poverty hysteresis, the
    /// anti-rebound lightning preference, then the priority walk
    /// with the per-spell TIER-DOWN (sub_15F20) — every spell is
    /// probed at every tier and a refused/unaffordable tier just
    /// keeps walking. The winning probe leaves the manifestation
    /// retuned to the passing tier; the caller's cast fires at it.
    /// There is no "affordable by ceiling → save up and WAIT" hold.
    pub(crate) fn mc2_rival_attack_pick(&mut self, ri: usize, vs_wizard: bool) -> Option<usize> {
        // The poverty latch (EF:7190-7205): enter under maxMana/4;
        // release at maxMana/4 + 6000, clamped to maxMana/2 ONLY
        // when the sum overshoots the ceiling (NOT an unconditional
        // min — that would be wrong for mid wealth).
        {
            let r = &mut self.mc2_rivals[ri];
            if r.mana < r.mana_max / 4 {
                r.poverty = true;
            } else if r.poverty {
                let mut release = r.mana_max / 4 + 6000;
                if release >= r.mana_max {
                    release = r.mana_max / 2;
                }
                if r.mana >= release {
                    r.poverty = false;
                }
            }
            if r.poverty {
                return None;
            }
        }
        let walk = |w: &mut Self, s: usize| -> bool {
            let mut tier = w.mc2_rivals[ri].book.levels[s] as i16;
            while tier >= 0 {
                if w.mc2_rival_tier_probe(ri, tier, s) == s as i32 {
                    return true;
                }
                tier -= 1;
            }
            false
        };
        if vs_wizard {
            // Anti-rebound (EF:7209-19): a target visibly holding a
            // live rebound (8) prefers a lightning (7) tier-walk,
            // Perception% of the time.
            let target_buffed = match self.mc2_rivals[ri].target {
                PLAYER_TARGET => self.player.rebound,
                t => self
                    .mc2_rivals
                    .iter()
                    .find(|r| r.ent == t)
                    .is_some_and(|r| r.rebound),
            };
            if target_buffed {
                let me = self.mc2_rivals[ri].ent as usize;
                let roll = (self.g.ent_rand(me) % 255) as u16;
                if roll < self.mc2_rivals[ri].per && walk(self, 7) {
                    return Some(7);
                }
            }
            let target_is_wizard = matches!(self.mc2_rivals[ri].target, PLAYER_TARGET)
                || self
                    .g
                    .ent
                    .get(self.mc2_rivals[ri].target as usize)
                    .is_some_and(|e| e.class64 == 3 && e.model65 <= 1);
            for &s in &ATTACK_WIZARD {
                // Spell 0x13 only against a wizard body (EF:7230-37).
                if s == 0x13 && !target_is_wizard {
                    continue;
                }
                if walk(self, s as usize) {
                    return Some(s as usize);
                }
            }
        } else {
            for &s in &ATTACK_CASTLE {
                if walk(self, s as usize) {
                    return Some(s as usize);
                }
            }
        }
        None
    }

    // ---- the cast arm (readiness sub_15170 EF:6887 + executor
    // ---- sub_14E10 EF:6759) -----------------------------------------------

    /// Readiness `sub_15170` (EF:6888-7095) — the per-spell-CLASS
    /// gate table. Common to every class: owned + the tier's ceiling
    /// unlock (maxMana >= maxManaLimit) + affordable now (the castle
    /// reads the ladder fresh). Per class:
    /// - {0,7,0xD,0xE,0x16}: cooldown + the Perception cone;
    /// - {1,9,0x10,0x12,0x13,0x15}: + the armed-window refusal;
    /// - 2 with a castle: armed + cooldown + cone + the space check
    ///   (sub_11A10); without one: cooldown only — the first castle
    ///   is aim-free (EF:7046-49);
    /// - 3 speed-up: NO cooldown check at all (EF:7051-63);
    /// - {4,6,8,0xB} buff/self: armed + cooldown, no cone (EF:7078);
    /// - the rest (5,0xA,0xC,0xF,0x11,0x14,0x17,0x18,0x19): cooldown
    ///   only (the LABEL_43 generic arm).
    ///
    /// The cone is yaw-vs-setpoint (`sub_582B0(yaw, roll)` — the
    /// state handlers keep f34 on the target): (255-P)/4+20 degrees.
    fn mc2_rival_cast_ready(&self, ri: usize, s: usize) -> bool {
        let r = &self.mc2_rivals[ri];
        let m = r.book.ent[s] as usize;
        if m == 0 {
            return false;
        }
        let e = &self.g.ent[m];
        if (r.mana_max as i64) < e.f136 as i64 {
            return false; // the maxManaLimit ceiling gate
        }
        let cost = if s == 2 {
            self.mc2_castle_ladder_cost(ri) as i64
        } else {
            e.max_life as i64
        };
        if (r.mana as i64) < cost {
            return false;
        }
        let armed = e.f26 > 0;
        let cooling = r.cooldown[s] != 0;
        let cone_ok = || {
            let cone = ((255 - r.per as u32) / 4 + 20) * 2048 / 360;
            let e = &self.g.ent[r.ent as usize];
            (Gen::angdist(e.f30, e.f34) as u32) < cone
        };
        match s {
            0 | 7 | 0xD | 0xE | 0x16 => !cooling && cone_ok(),
            1 | 9 | 0x10 | 0x12 | 0x13 | 0x15 => !armed && !cooling && cone_ok(),
            2 => match self.rival_castle(r.ent) {
                Some(c) => {
                    !armed
                        && !cooling
                        && cone_ok()
                        && self.g.ent[c].tick70 == 4
                        && self.g.mc2_castle_space_ok(c)
                }
                None => !cooling,
            },
            3 => true,
            4 | 6 | 8 | 0xB => !armed && !cooling,
            _ => !cooling,
        }
    }

    /// `sub_15F20` (EF:7581-7611) — the tier-down probe: retune the
    /// manifestation to `tier` FIRST (retail's SetSpell side effect
    /// happens even when the probe then fails — a live window queues
    /// via f44 instead), then the readiness gate + the tier's raw
    /// table costs. Returns the spell id when castable at this tier,
    /// -1 when only mana blocks it, 0 otherwise.
    fn mc2_rival_tier_probe(&mut self, ri: usize, tier: i16, s: usize) -> i32 {
        let Some(row) = self.g.assets.spells.get(s).copied() else {
            return 0;
        };
        if tier < 0 || row.byte_0 as i16 <= tier {
            return 0;
        }
        let m = self.mc2_rivals[ri].book.ent[s] as usize;
        if m == 0 {
            return 0;
        }
        let own = self.mc2_rivals[ri].ent;
        self.mc2_rival_set_spell(m, tier as u8, own);
        if !self.mc2_rival_cast_ready(ri, s) {
            return 0;
        }
        let sub = row.tiers[(tier as usize).min(2)];
        let r = &self.mc2_rivals[ri];
        if (r.mana_max as i64) < sub.max_mana_limit as i64 || (r.mana as i64) < sub.mana_cost as i64
        {
            return -1;
        }
        s as i32
    }

    /// The tier-down cast walk every retail pick site shares
    /// (`for k = SpellLevels[s]; k >= 0; k--` + cast on the first
    /// passing tier — EF:5470/5591/5759/5840...).
    pub(crate) fn mc2_rival_walk_cast(&mut self, ri: usize, i: usize, s: usize) -> bool {
        let mut tier = self.mc2_rivals[ri].book.levels[s] as i16;
        while tier >= 0 {
            if self.mc2_rival_tier_probe(ri, tier, s) == s as i32 {
                return self.mc2_rival_cast(ri, i, s);
            }
            tier -= 1;
        }
        false
    }

    /// The commit (sub_14E10 EF:6759): burst gun on the precision
    /// family, aim pitch at the target, arm the recast cooldown,
    /// debit through the regen delta, emit through the shared MC2
    /// class-9 spawners. Spell 0xF is never AI-cast (EF:6885).
    pub(crate) fn mc2_rival_cast(&mut self, ri: usize, i: usize, s: usize) -> bool {
        if s >= MC2_SPELLS || s == 0xF {
            return false;
        }
        if !self.mc2_rival_cast_ready(ri, s) {
            return false;
        }
        let (tx, ty, tz) = match self.mc2_rivals[ri].target {
            0 => {
                let e = &self.g.ent[i];
                let mut fwd = (e.x, e.y, e.z);
                Gen::polar_step(&mut fwd, e.f30, 0, 4096);
                fwd
            }
            PLAYER_TARGET => self.human_pose,
            t => {
                let e = &self.g.ent[t as usize];
                (e.x, e.y, e.z)
            }
        };
        let (ex, ey, ez, yaw) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f30)
        };
        let want = Gen::angle_between(ex, ey, tx, ty);
        match s {
            // Precision-aimed burst family {0,1,7,0x16} (EF:6797):
            // cone 0xAA, the shared burst counter.
            0 | 1 | 7 | 0x16 => {
                if self.mc2_rivals[ri].burst < 0 || Gen::angdist(yaw, want) > 0xAA {
                    return false;
                }
                self.mc2_rivals[ri].burst += 1;
                if self.mc2_rivals[ri].burst >= 8 {
                    self.mc2_rivals[ri].burst =
                        ((self.mc2_rivals[ri].refl as i32 - 255) / 8 - 1) as i16;
                }
            }
            // Homing-aimed {4,9,0xD,0xE,0x12,0x13,0x15} (EF:6841):
            // the wider 0xE3 cone.
            4 | 9 | 0xD | 0xE | 0x12 | 0x13 | 0x15 if Gen::angdist(yaw, want) > 0xE3 => {
                return false;
            }
            _ => {}
        }
        // Create Castle routes to the build/upgrade arm (case 2,
        // EF:6820 — the same body as the human's).
        if s == 2 {
            return self.mc2_rival_cast_castle(ri, i);
        }
        // Arm the recast cooldown + debit the full tier cost through
        // the regen delta (the chassis mana law).
        self.mc2_rivals[ri].cooldown[s] = AI_RECAST[s];
        let m = self.mc2_rivals[ri].book.ent[s] as usize;
        let cost = self.g.ent[m].max_life as i32;
        {
            let r = &mut self.mc2_rivals[ri];
            r.mana_delta = if r.mana_delta >= 0 {
                -cost
            } else {
                r.mana_delta - cost
            };
        }
        // Arm the manifestation window (buffs/heal read it).
        self.g.ent[m].f26 = self.g.ent[m].f28.max(1) as i16;
        if s == 6 {
            // A fresh shield starts ARMED (the byte[2] 0x40 stage).
            self.mc2_rivals[ri].shield_state = 1;
        }
        // Absolute aim pitch to the target (EF:6803).
        let dh = Gen::isqrt(Gen::dist2_sq(ex, ey, tx, ty) as u32) as i32;
        let pitch = Gen::pitch_toward(ez, tz, dh);
        self.mc2_rival_emit(ri, i, s, yaw, pitch);
        true
    }

    /// Case 2 — Create Castle (EF:6820): with a castle, the upgrade
    /// request through the shared mail[5] token protocol; without,
    /// the DIRECT (3,2) spawn at the scouted site — MC2 runtime AI
    /// castles build the real thing (no MC1 free-plant).
    fn mc2_rival_cast_castle(&mut self, ri: usize, i: usize) -> bool {
        // Affordability re-check FIRST — a whiffed attempt must not
        // burn the recast cooldown (the cost is re-read fresh off the
        // ladder, not the manifestation's stale stamp). The cooldown
        // arms ONLY after a successful
        // upgrade fire (EF:6828); the first-castle direct spawn
        // never arms it at all (EF:6831-40).
        let cost = self.mc2_castle_ladder_cost(ri);
        if (self.mc2_rivals[ri].mana as i64) < cost as i64 {
            return false;
        }
        if let Some(c) = self.rival_castle(self.mc2_rivals[ri].ent) {
            if !self.g.mc2_castle_space_ok(c) {
                return false;
            }
            {
                let r = &mut self.mc2_rivals[ri];
                r.mana_delta = if r.mana_delta >= 0 {
                    -cost
                } else {
                    r.mana_delta - cost
                };
            }
            // The upgrade request: mail[5] = (10, owner) — the
            // castle's intake arms F_UPGRADE_ARMED (EF:61753 shared
            // protocol; the (9,10) ball ride is cosmetic, APPROX
            // skipped like the MC1 column).
            let own = self.mc2_rivals[ri].ent;
            self.g.ent[c].mail[5] = (10, own);
            // Castle research for the stage this upgrade builds
            // (the A.5 shortcut, same as the human's cast stamp):
            // the rival's live castle-spell tier picks the tower
            // type of the new stage.
            let stage = (self.g.ent[c].f26 + 1).clamp(1, 7) as u8;
            let tier = self.mc2_rival_castle_tier(ri);
            self.g.mc2_research_stamp(own, stage, tier);
            self.mc2_rivals[ri].cooldown[2] = AI_RECAST[2].max(1);
            return true;
        }
        // Castle-less: the direct (3,2) spawn at the site (EF:6833
        // IfSubtypeCallCreatingManaSphere(&axis_0x9A_154x, 3, 2)),
        // paid in full — the build machinery takes it from there.
        let (sx, sy) = self.mc2_rivals[ri].site;
        if sx == 0 && sy == 0 {
            return false;
        }
        let Some(c) = self.g.new_event() else {
            return false;
        };
        {
            let e = &mut self.g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.tick70 = 5; // action 5 = the build state machine
            e.f59 = 0;
            e.max_life = 40000;
            e.f26 = 0;
            e.id24 = self.mc2_rivals[ri].ent;
            let mut tx = sx >> 8;
            let ty = sy >> 8;
            if (tx.wrapping_add(ty)) & 1 == 1 {
                tx = tx.wrapping_add(1);
            }
            e.dest_x = tx << 8;
            e.dest_y = ty << 8;
        }
        let (ax, ay) = (self.g.ent[c].dest_x, self.g.ent[c].dest_y);
        // The build datum (sub_4AA40 EF:33399): corner-mean site z.
        // Without it the painter datum reads 0 and the footprint
        // excavates to sea level — the "sunken rival castle".
        let z = self.g.mc2_castle_site_z((ax >> 8) as u8, (ay >> 8) as u8);
        self.g.ent[c].site_z = z;
        self.g.link(c, ax, ay, z);
        self.g.refill_life(c);
        self.g.mc2_set_sprite(
            c,
            177 + crate::mc2::color_art(self.mc2_rivals[ri].slot) as u16,
        );
        {
            let r = &mut self.mc2_rivals[ri];
            r.mana_delta = if r.mana_delta >= 0 {
                -cost
            } else {
                r.mana_delta - cost
            };
        }
        self.g.snd(30, c);
        // Stage-1 research for the fresh castle (A.5 shortcut).
        let own = self.mc2_rivals[ri].ent;
        let tier = self.mc2_rival_castle_tier(ri);
        self.g.mc2_research_stamp(own, 1, tier);
        self.entities_dirty = true;
        let _ = i;
        true
    }

    /// The rival's live castle-spell tier — the book manifestation's
    /// f71 (`byte_0x46_70`), 0 when spell 2 is unowned.
    fn mc2_rival_castle_tier(&self, ri: usize) -> u8 {
        let m = self.mc2_rivals[ri].book.ent[2] as usize;
        if m != 0 { self.g.ent[m].f71 } else { 0 }
    }

    /// The per-spell emission through the shared MC2 class-9
    /// spawners (the sub_5F660 router's downstream), owner = the
    /// rival's entity — homing, damage payloads and the impact-XP
    /// mail all serve it unchanged.
    fn mc2_rival_emit(&mut self, ri: usize, i: usize, s: usize, yaw: u16, pitch: u16) {
        let m = self.mc2_rivals[ri].book.ent[s] as usize;
        let tier = self.g.ent[m].f71 as usize;
        let row = self.g.assets.spells.get(s).copied();
        let mut sub = row.map(|r| r.tiers[tier.min(2)]).unwrap_or_default();
        if matches!(s, 21 | 25) && sub.life > 0 {
            sub.sub_spell /= sub.life as i32;
        }
        // The would-be projectile subtype: the sub_6DCA0 band + the
        // direct class-9 arms (possess 1/17 / summon 24 / mine 29 /
        // alliance 25). Rivals run the SAME `sub_69640` machine as the
        // human, so possession picks its entity off the tier's
        // `life_0x1A` too: 0 → the basic **(9,1)** (`sub_69900`
        // EF:56039), 1..3 → the leveled (9,17) (EF:55950). Hardcoding
        // 17 left 49 (9,17)-extra rows on the mc2l4 0+4000 window
        // after the human's arm was fixed.
        let arm = World::mc2_dispatch_arm(s, sub.life)
            .map(|a| (a.subtype, a.impact, a.charge))
            .or(match s {
                1 if sub.life == 0 => Some((1u8, (10u8, 12u8), false)),
                1 => Some((17u8, (10u8, 12u8), false)),
                0x13 => Some((24, (10, 0), false)),
                0x17 => Some((29, (10, 0), false)),
                0x18 => Some((25, (10, 0), false)),
                _ => None,
            });
        // Cast sound (EF:44233 family).
        let snd = match s {
            0 => Some(9u8),
            1 => Some(40),
            3 => Some(19),
            5 => Some(25),
            7 => Some(23),
            0x13 | 0x18 => Some(9),
            _ if arm.is_some() => Some(15),
            _ => None,
        };
        if let Some(id) = snd {
            self.g.snd(id, i);
        }
        // Fools-mana conjures the (10,57) random sphere in place. The
        // sphere is a TRAP (`sub_36680` EF:26615, the (10,57) tick's
        // own law — docs/spell-audit/fools-mana.md), so it must carry
        // the caster as parentId (`sub_6C870` EF:57905): that is the
        // ONLY skip arm, and without it the rival springs its own bait.
        if s == 0x16 {
            let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
            let z = self.g.ground_z(px, py) as i16;
            if let Some(sp) = self.g.mc2_spawn_mana_sphere(57, px, py, z) {
                self.g.ent[sp].id24 = self.g.ent[i].id24;
            }
            return;
        }
        let Some((subtype, impact, charge)) = arm else {
            // Buff/self spells have no projectile — the armed window
            // on the manifestation IS the effect.
            return;
        };
        let (ex, ey, ez, speed, half) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f126, e.f78 as i16)
        };
        let mz = ez.wrapping_add(half);
        let Some(p) = self.g.mc2_spawn_cast_proj(subtype, ex, ey, mz) else {
            return;
        };
        let owner = self.mc2_rivals[ri].ent;
        let target = self.mc2_rivals[ri].target;
        {
            let e = &mut self.g.ent[p];
            e.id24 = owner;
            e.f68 = impact.0;
            e.f69 = impact.1;
            e.f44 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
            if charge {
                e.f71 = sub.life.max(0) as u8;
            }
            e.f30 = yaw;
            e.f32 = pitch;
            e.f34 = yaw;
            e.f36 = pitch;
            let boosted = (e.f126 as i32 + speed.max(0) as i32).clamp(384, 0x2000);
            e.f126 = boosted as i16;
            // The impact-XP back-ref (the owner-tagged sub_6D8B0
            // mail): f40 carries the spell index.
            e.f40 = s as u16;
            // Live homing target — the class-9 re-acquire keeps it.
            if target != 0 {
                e.f146 = target;
            }
        }
        if target == PLAYER_TARGET {
            // Being targeted arms the danger music.
            self.g.player_danger = 100;
        }
        self.entities_dirty = true;
    }

    // NOTE: retail rivals have NO spell-XP progression —
    // `sub_6D8B0`'s guard is class-3 model-0, the human only
    // (EF:58240-41). A rival's spell tiers are its authored map
    // levels for life; the per-cast TIER-DOWN walk
    // (`mc2_rival_tier_probe`) supplies all tier dynamics. Do NOT add
    // an XP relevel ladder for rivals.

    /// Test hook: the rival's book as (owned, level) rows plus its
    /// castle's (stored, cap) mana — `None` castle-less. The lifecycle
    /// tests pin the authored grant/tier law and the spawns-full
    /// castle bank with this.
    #[doc(hidden)]
    pub fn debug_mc2_rival_economy(
        &self,
        slot: u8,
    ) -> Option<([(bool, u8); MC2_SPELLS], Option<(i32, i32)>)> {
        let r = self.mc2_rivals.iter().find(|r| r.slot == slot)?;
        let mut book = [(false, 0u8); MC2_SPELLS];
        for (s, row) in book.iter_mut().enumerate() {
            *row = (r.book.ent[s] != 0, r.book.levels[s]);
        }
        let bank = self
            .rival_castle(r.ent)
            .map(|c| (self.g.ent[c].f140, self.g.ent[c].f136));
        Some((book, bank))
    }

    /// Test hook: zero a rival's grace and hand it a lethal hit from
    /// nothing (the mc1 `debug_kill_player` shape).
    #[doc(hidden)]
    pub fn debug_kill_mc2_rival(&mut self, slot: u8) {
        if let Some(ri) = self.mc2_rivals.iter().position(|r| r.slot == slot) {
            self.mc2_rivals[ri].grace = 0;
            let i = self.mc2_rivals[ri].ent as usize;
            self.g.ent[i].mail[0] = (u32::MAX / 4, 1);
        }
    }

    // ---- mortality (sub_5E310 EV:2882 + sub_5E7C0 EV:2895) -----------------

    /// Action 2 — the death fall (sub_5E310 EF:60074-60099): Z-ONLY
    /// — integrate the OLD velocity, then gravity -2/tick, terminal
    /// -256, positive (upward) velocity zeroed immediately; floor =
    /// ground + the tuning row's v_12 (row 67 = 128, row-driven);
    /// the (10,1) owner-flagged death puff each tick; EXACT floor
    /// contact runs the payout. Z-only, no polar drift — any lateral
    /// drift here displaces the graves/tokens.
    fn mc2_rival_death_fall(&mut self, ri: usize, i: usize) {
        let (x, y) = (self.g.ent[i].x, self.g.ent[i].y);
        let ground = self.g.ground_z(x, y) as i16;
        let floor = ground.saturating_add(BEHAVIOR[self.g.ent[i].row156 as usize].v_12);
        {
            let e = &mut self.g.ent[i];
            let mut z = e.z.saturating_add(e.f46);
            e.f46 = (e.f46 - 2).clamp(-256, 0);
            if z < floor {
                z = floor;
            }
            e.z = z;
        }
        let z = self.g.ent[i].z;
        // The (10,1) death puff (EF:60092-97), owner-flagged.
        if let Some(s) = self.g.mc2_spawn_big_explosion(x, y, z) {
            self.g.ent[s].flags |= 0x80;
            self.g.ent[s].id24 = self.mc2_rivals[ri].ent;
        }
        if z == floor {
            self.mc2_rival_death_impact(ri, i);
        }
        self.entities_dirty = true;
    }

    /// The landing payout (EF:60096-60177): kill credit, the 26
    /// SPELL-TOKEN scatter (class-15, re-collectible, lifetime
    /// rand%90+200 at +-256), the (10,40) grave, the owned-sphere
    /// re-point, the 1200 respawn timer, husk hidden.
    fn mc2_rival_death_impact(&mut self, ri: usize, i: usize) {
        // Kill credit: the killer wizard's per-color tally; the
        // (10,67) flood killer is suppressed (EF:60716 — no credit).
        let killer = self.g.ent[i].f38;
        let flood_kill = self
            .g
            .ent
            .get(killer as usize)
            .is_some_and(|e| e.class64 == 10 && e.model65 == 67);
        if !flood_kill {
            if let Some(k) = self.owner_slot_of_source(killer) {
                self.kill_tally[k as usize][self.mc2_rivals[ri].slot as usize] += 1;
                if k == 0 {
                    self.g.kills = self.g.kills.saturating_add(1);
                }
            }
        }
        let slot = self.mc2_rivals[ri].slot;
        self.rival_deaths.push(slot);
        // The death broadcast (retail lang 374 "has died.") — the MC2
        // wizard name table (WizardsNames_D93A0), NOT the MC1 one.
        let name = MC2_RIVAL_NAMES.get(slot as usize).copied().unwrap_or("?");
        // Notification life 100 (retail's toast countdown).
        self.set_notification(format!("{name} has died."), 100, [0xFF, 0, 0]);
        // The SPELL-TOKEN scatter (EF:60137-62): every owned
        // manifestation detaches into a loose pickup token (state
        // 3M+1), scattered +-256, lifetime rand%90+200. The book
        // flags revert to boolean (known[] persists).
        let (cx, cy) = (self.g.ent[i].x, self.g.ent[i].y);
        for s in 0..MC2_SPELLS {
            let m = self.mc2_rivals[ri].book.ent[s] as usize;
            self.mc2_rivals[ri].book.ent[s] = 0;
            if m == 0 {
                continue;
            }
            let dx = (self.g.ent_rand(m) & 0x1FF) as i32 - 256;
            let dy = (self.g.ent_rand(m) & 0x1FF) as i32 - 256;
            let jx = (cx as i32 + dx) as u16;
            let jy = (cy as i32 + dy) as u16;
            let jz = self.g.ground_z(jx, jy) as i16;
            let life = (self.g.ent_rand(m) % 0x5A + 200) as i32;
            {
                let e = &mut self.g.ent[m];
                e.tick70 = (e.model65).wrapping_mul(3).wrapping_add(1); // loose token
                e.id24 = 0;
                e.f26 = 0;
                e.act_life = life;
            }
            self.g.move_relink(m, jx, jy, jz);
        }
        // The grave (10,40) + the owned (10,39) sphere re-point
        // (EF:60164-77). The grave stands as the census anchor for
        // the dead wizard's loose spheres: re-owning them to the
        // grave means a wizard who later possesses the grave
        // (grave_tick, action 42) inherits the dead wizard's mana.
        let gz = self.g.ground_z(cx, cy) as i16;
        if let Some(gv) = self.g.mc2_spawn_grave(cx, cy, gz) {
            let me = self.mc2_rivals[ri].ent;
            for j in 1..self.g.ent.len() {
                let e = &mut self.g.ent[j];
                if e.class64 == 10 && e.model65 == 39 && e.flags & 0x400 == 0 && e.f144 == me {
                    e.f144 = gv as u16;
                }
            }
        }
        // Action 3 + the FLAT 1200 respawn timer (EF:60170) + hide.
        {
            let e = &mut self.g.ent[i];
            e.tick70 = 3;
            e.flags = (e.flags | 0x20) & !8;
            e.f26 = 1200;
        }
        self.entities_dirty = true;
    }

    /// Action 3 — dead-wait (sub_5E7C0 EF:60254): with a castle the
    /// timer counts down to a respawn AT the castle; castle-less =
    /// BANISHED (checked every tick — losing the castle mid-wait
    /// converts to elimination).
    fn mc2_rival_dead_wait(&mut self, ri: usize, i: usize) {
        if self.rival_castle(self.mc2_rivals[ri].ent).is_none() {
            // The FINAL-death broadcast (retail lang 283, sub_5E7C0
            // EF:60282-97: printed once on the elimination edge —
            // the byte_0x006 guard — with toast countdown 200; the
            // per-death "has died." already fired at corpse-fall).
            if !self.mc2_rivals[ri].eliminated {
                let slot = self.mc2_rivals[ri].slot;
                let name = MC2_RIVAL_NAMES.get(slot as usize).copied().unwrap_or("?");
                self.set_notification(
                    format!("{name} has been banished from the realm."),
                    200,
                    [0xFF, 0, 0],
                );
            }
            self.mc2_rivals[ri].eliminated = true;
            return;
        }
        if self.g.ent[i].f26 > 0 {
            self.g.ent[i].f26 -= 1;
            return;
        }
        self.mc2_rival_respawn(ri, i);
    }

    /// The respawn (the sub_5C950 REUSE arm, EF:43694-43706): re-
    /// anchor at the castle, full life/mana, grace 100, re-mint the
    /// remembered book at the recorded tiers, brain reset, truce.
    fn mc2_rival_respawn(&mut self, ri: usize, i: usize) {
        let Some(c) = self.rival_castle(self.mc2_rivals[ri].ent) else {
            return;
        };
        let (cx, cy) = (self.g.ent[c].x, self.g.ent[c].y);
        let z = (self.g.ground_z(cx, cy) as i16).saturating_add(0x100);
        {
            let e = &mut self.g.ent[i];
            e.flags = (e.flags & !0x20) | 8;
            e.tick70 = 1;
            e.f46 = 0;
            e.f126 = 0;
        }
        self.g.move_relink(i, cx, cy, z);
        self.g.refill_life(i);
        let known = self.mc2_rivals[ri].known;
        for (s, &k) in known.iter().enumerate() {
            if k && self.mc2_rivals[ri].book.ent[s] == 0 {
                let r = &self.mc2_rivals[ri];
                let ent = r.ent;
                let sel = r.book.sel[s];
                let (x, y, zz) = {
                    let e = &self.g.ent[i];
                    (e.x, e.y, e.z)
                };
                if let Some(m) = self.mc2_new_spell_token(s as u8, x, y, zz) {
                    {
                        let e = &mut self.g.ent[m];
                        e.tick70 = (s as u8).wrapping_mul(3);
                        e.f54 = 64;
                        e.id24 = ent;
                        e.f26 = 0;
                        e.f44 = 0;
                    }
                    self.mc2_rival_set_spell(m, sel, ent);
                    self.mc2_rivals[ri].book.ent[s] = m as u16;
                }
            }
        }
        {
            let r = &mut self.mc2_rivals[ri];
            // maxMana wiped to the base 1000 — the mana progression
            // does NOT survive death (EF:43722).
            r.mana = 1000;
            r.mana_max = 1000;
            r.mana_delta = 0;
            r.grace = 100;
            r.state = Mc2AiState::Fresh;
            r.target = 0;
            r.burst = 0;
            r.poverty = false;
            r.strafe = 0;
            r.weave = 0;
            r.shield_state = 0;
            r.avoid = 0;
            r.avoid_exit = 0;
            // Own hate ledger back to neutral (EF:43848-50); the WAR
            // latches survive death — retail never clears them here.
            r.hate = [HATE_NEUTRAL; 8];
            // Cooldowns are KEPT except the castle slot, staggered
            // by color: SpellEnabled[2] = 4·color (EF:43851) — else
            // every respawn rebuilds its castle at once.
            r.cooldown[2] = 4 * r.slot as u16;
        }
        // The post-respawn truce toward this color.
        let slot = self.mc2_rivals[ri].slot as usize;
        for (oj, o) in self.mc2_rivals.iter_mut().enumerate() {
            if oj != ri {
                o.hate[slot] = HATE_RESPAWN;
            }
        }
        self.entities_dirty = true;
    }
}

impl Gen {
    /// The (10,40) wizard grave: the census anchor the dead wizard's
    /// (10,39) spheres re-point to (EF:60164). Its action body is
    /// `sub_36AE0` (EF:26835) = action 42 = the shared `grave_tick`
    /// (byte-exact with MC1 `spawn_grave`/`grave_tick`, features.rs):
    /// a class-3 wizard's ch1 possession claim inherits EVERYTHING the
    /// grave owns (its re-pointed mana spheres) then despawns it, so
    /// possessing the corpse reclaims the dead wizard's loose mana.
    /// KEEP targetable bit 8 (the possess bolt must be able to hit it)
    /// and set `f28 = 2` (the ch1 claim channel), matching MC1.
    pub(crate) fn mc2_spawn_grave(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
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
        self.mc2_set_sprite(s, 65);
        Some(s)
    }
}

// ------------------------------------------------------------ snapshot

use crate::snapshot::{Reader, Snap, SnapshotError, Writer, snap_enum};

snap_enum!(
    Mc2AiState,
    "Mc2AiState",
    0 => Mc2AiState::Fresh,
    1 => Mc2AiState::Upgrade,
    2 => Mc2AiState::Build,
    3 => Mc2AiState::Possess,
    4 => Mc2AiState::RaidCastle,
    5 => Mc2AiState::AttackWizard,
    6 => Mc2AiState::RaidBalloon,
    7 => Mc2AiState::HuntMana,
    8 => Mc2AiState::Home,
    9 => Mc2AiState::Cruise,
    10 => Mc2AiState::Defense,
);

impl Snap for Mc2Rival {
    fn put(&self, w: &mut Writer) {
        let Mc2Rival {
            slot,
            ent,
            book,
            known,
            cooldown,
            mana,
            mana_max,
            mana_delta,
            agg,
            per,
            refl,
            life_scale,
            state,
            hate,
            war,
            burst,
            poverty,
            target,
            target_sig,
            site,
            strafe,
            weave,
            weave_dir,
            shield_state,
            avoid,
            avoid_exit,
            vdes,
            grace,
            eliminated,
            shield,
            invisible,
            rebound,
        } = self;
        w.put(slot);
        w.put(ent);
        w.put(book);
        w.put(known);
        w.put(cooldown);
        w.put(mana);
        w.put(mana_max);
        w.put(mana_delta);
        w.put(agg);
        w.put(per);
        w.put(refl);
        w.put(life_scale);
        w.put(state);
        w.put(hate);
        w.put(war);
        w.put(burst);
        w.put(poverty);
        w.put(target);
        w.put(target_sig);
        w.put(site);
        w.put(strafe);
        w.put(weave);
        w.put(weave_dir);
        w.put(shield_state);
        w.put(avoid);
        w.put(avoid_exit);
        w.put(vdes);
        w.put(grace);
        w.put(eliminated);
        w.put(shield);
        w.put(invisible);
        w.put(rebound);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Mc2Rival {
            slot: r.get()?,
            ent: r.get()?,
            book: r.get()?,
            known: r.get()?,
            cooldown: r.get()?,
            mana: r.get()?,
            mana_max: r.get()?,
            mana_delta: r.get()?,
            agg: r.get()?,
            per: r.get()?,
            refl: r.get()?,
            life_scale: r.get()?,
            state: r.get()?,
            hate: r.get()?,
            war: r.get()?,
            burst: r.get()?,
            poverty: r.get()?,
            target: r.get()?,
            target_sig: r.get()?,
            site: r.get()?,
            strafe: r.get()?,
            weave: r.get()?,
            weave_dir: r.get()?,
            shield_state: r.get()?,
            avoid: r.get()?,
            avoid_exit: r.get()?,
            vdes: r.get()?,
            grace: r.get()?,
            eliminated: r.get()?,
            shield: r.get()?,
            invisible: r.get()?,
            rebound: r.get()?,
        })
    }
}
