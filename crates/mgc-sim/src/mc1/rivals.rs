//! Hostile (AI) wizards — the class-3 model-1 rival carpets: spawn,
//! per-tick brain, casting arm, mortality and respawn. Direct port of
//! the remc1 AI wizard machinery; all citations remc1 sub_main.cpp.
//! The full trace record lives in docs/ROADMAP.md "HOSTILE WIZARDS
//! (RIVAL AI) — TRACE BANK".
//!
//! Architecture mirrors the original: the AI is a DECISION LAYER over
//! the shared engines — rivals cast through the same class-12
//! manifestation entities, spawn the same class-9/10 projectiles and
//! effects (owner = the rival's wizard entity slot, so the generic
//! combat plumbing serves them unchanged), obey the same mana economy
//! (census ceiling, spell costs, castle-stored thresholds, the debit
//! riding the regen delta), and die into the same jar-scatter + grave
//! sequence as the human.
//!
//! Retail AI-vs-human asymmetries, ported faithfully: AI heals 4x the
//! human's afield rate (:18013 vs :55418); AI at its castle DISCARDS
//! damage instead of forwarding it to the castle (:17975-79); the
//! AI's first castle is spawned directly, free and instant
//! (:19200-08); the AI carpet ignores walls, drag and knockback
//! (sub_14EB0 runs neither the wall gate nor the knock fields); AI
//! target scans are omniscient; the AI learns spells on a 200-tick
//! timer from any jar existing in the world instead of picking jars
//! up (:64805-14, :19381-443).
//!
//! Interim deviations (ours, flagged inline): the hate feed runs at
//! damage-intake and homing-acquisition time instead of the
//! original's per-projectile one-shot ledger scan (sub_16540 —
//! equivalent inputs, slightly later timing); the duel pull on the
//! CASTER is applied through the knock channel (magnitude from the
//! traced formula). Creature target scans now walk the full class-3
//! bucket[0] list (`Gen::nearest_wizard_target`) — carpets, castles
//! and balloons for the wyvern/crab/mound/guard, carpets-only for the
//! genie — so wild creatures fight rival wizards, not just the human.
//! The m4 militia and m8 griffon village-wanted gates are per-wizard
//! too: `Gen::rival_wanted` mirrors `player_aggro` for the rivals
//! (armed by a rival's own village offenses), so villages turn their
//! defenders on any hostile wizard (see docs/DEVIATIONS.md).

use crate::engine::features::Gen;
use crate::engine::world::{LifeState, World};
use crate::mc1::behavior::BEHAVIOR;
use crate::mc1::mobs::PLAYER_TARGET;
use crate::mc1::spells::{SPELL_COUNT, SPELLS};

/// Per-slot config from the level record (wizards.json), resolved by
/// the app: personality params, starting castle, and the two spell
/// masks (str_230867_37072[slot], :49222/:54965-67).
#[derive(Debug, Clone)]
pub struct RivalConfig {
    /// u16_522: hate rise rate, war thresholds, opportunism margins.
    pub aggression: u8,
    /// u16_524: commit aim cone, rebound-notice probability.
    pub accuracy: u8,
    /// u16_526: decision period, turn agility, burst pause, respawn.
    pub tempo: u8,
    /// Starting castle level: 0 = none, N = a castle at level N-1
    /// spawns with the wizard (level tail @38804+slot).
    pub castle_level: u8,
    /// Level-start book: pregrant && allowed (:49222).
    pub book: [bool; SPELL_COUNT],
    /// var_230983 — what the AI may LEARN mid-level (Type_160+796).
    pub allowed: [bool; SPELL_COUNT],
}

/// The rival wizard names, by player slot (off_99B68 :5741; slot 0 =
/// the human's default name).
pub const RIVAL_NAMES: [&str; 8] = [
    "Zanzamar",
    "Vodor",
    "Gryshnak",
    "Mahmoud",
    "Syed",
    "Raschid",
    "Alhabbal",
    "Scheherazade",
];

/// The hate ledger's neutral baseline (0x601F, :17946-67).
const HATE_NEUTRAL: u16 = 24607;
/// Hate toward a freshly (re)spawned wizard — elevated but decaying
/// (the post-respawn truce, -24609 as unsigned :55037-41).
const HATE_RESPAWN: u16 = 40927;

/// AI per-spell re-attempt cooldowns, ticks (word_90034 :2163).
const AI_RECAST: [u16; SPELL_COUNT] = [
    2, 1, 32, 10, 1, 0, 0, 4, 400, 0, 1, 0, 1, 0, 1, 1, 40, 600, 0, 1, 4, 2, 3, 4,
];

/// The AI brain state (Type_160+415). States 2/4/5/10 exist in the
/// original's table but no selector ever sets them (cut content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub(crate) enum AiState {
    /// Fresh spawn: decide immediately (cascade runs twice, :17850).
    #[default]
    Fresh,
    /// Fly home, cast 0x10 = upgrade (sub_13800 :18106).
    Upgrade,
    /// Fly to the scouted site, plant the castle (sub_138F0 :18142).
    Build,
    /// Claim a mana ball with spell 3 (sub_13BA0 :18236).
    Possess,
    /// Raid an enemy castle (sub_13CA0 :18271).
    RaidCastle,
    /// Attack an enemy wizard (sub_13DC0 :18314).
    AttackWizard,
    /// Intercept an enemy balloon (same handler, state 9).
    RaidBalloon,
    /// Hunt any mana-holding creature (state 0xD).
    HuntMana,
    /// Return home (heal / regroup; sub_13A70 :18204).
    Home,
    /// Cruise (full life, nothing to do; sub_13A10 :18188).
    Cruise,
}

impl AiState {
    /// Retail +415 byte → variant (dispatch sub_13170 :17847). The cut
    /// states 2/4/5/10 fall to Fresh — no selector ever sets them.
    pub(crate) fn from_retail(v: u8) -> Self {
        match v {
            1 => AiState::Upgrade,
            3 => AiState::Build,
            6 => AiState::Possess,
            7 => AiState::RaidCastle,
            8 => AiState::AttackWizard,
            9 => AiState::RaidBalloon,
            0xB => AiState::Home,
            0xC => AiState::Cruise,
            0xD => AiState::HuntMana,
            _ => AiState::Fresh,
        }
    }
}

/// One live rival: the Type_160 subset the AI machinery needs. The
/// wizard's position/yaw/life/speed live on its pool entity (class 3
/// model 1); carried mana rides the entity's f140 mirror for the
/// census.
#[derive(Hash)]
pub(crate) struct Rival {
    /// Player slot (1..=7); slot 0 = the human, never a Rival.
    pub slot: u8,
    /// Wizard entity pool index. Also the rival's OWNER TAG: its
    /// projectiles' id24 and its claims' f144 (the original's +24 =
    /// own entity index, :44219).
    pub ent: u16,
    /// Manifestation pool slots by spell id (var_676; 0 = not owned).
    pub owned: [u16; SPELL_COUNT],
    /// Spells known across deaths — respawn re-mints manifestations
    /// (:54884-923); the scattered jars decay independently.
    pub known: [bool; SPELL_COUNT],
    /// Learn eligibility (Type_160+796).
    allowed: [bool; SPELL_COUNT],
    /// Spell-learning countdowns (+628): armed to 200 by a matching
    /// jar existing anywhere; conjures an own copy at expiry.
    learn: [u16; SPELL_COUNT],
    /// AI re-attempt cooldowns (+724, from [`AI_RECAST`]). Slot 16 is
    /// initialized to 4*slot — the per-player castle-build stagger
    /// the decompile shows as "var_756" (:55049).
    pub(crate) cooldown: [u16; SPELL_COUNT],
    /// Carried mana (+140) / ceiling (+136, census-owned) / regen
    /// delta (+132 — cast debits ride it negative).
    pub mana: u32,
    pub mana_max: u32,
    pub(crate) mana_delta: i32,
    /// Personality (u16_522/524/526).
    agg: u16,
    acc: u16,
    tempo: u16,
    pub state: AiState,
    /// Hate ledger + war flags per player slot (str_456; baseline
    /// [`HATE_NEUTRAL`]).
    pub(crate) hate: [u16; 8],
    pub(crate) war: [bool; 8],
    /// Fireball/lightning burst counter (+404): 8 shots then a
    /// negative lockout of (255-tempo)/8+1 ticks (:19129-36).
    burst: i16,
    /// Poverty latch (+406): mana < max/4 stops attack casting until
    /// it recovers past max/4+6000 (or max/2) (:19468-91).
    poverty: bool,
    /// Current target: entity slot or [`PLAYER_TARGET`]; 0 = none.
    /// (pub(crate) for the possess-emission tests in engine::world.)
    pub(crate) target: u16,
    /// Signature = team + model + (class<<7) (sub_15420 :19039).
    target_sig: u16,
    /// Scouted castle site (+150).
    site: (u16, u16),
    /// Lateral dodge velocity (v_16; impulse 80, decay 4/tick).
    pub(crate) jink: i16,
    /// Knockback bearing (v_24) and magnitude (v_22), armed by the
    /// shared damage intake (:55714-19) on EVERY letter including the
    /// fatal one. The AI's live mover `sub_14EB0` never spends it, so
    /// the impulse sits pending for the whole of a rival's life; the
    /// state-2 death fall's shared mover `sub_455D0` (:55204-19) is
    /// the only place a rival ever cashes it, drifting the corpse
    /// along the killing blow's bearing at 4/tick decay.
    pub(crate) knock_dir: u16,
    pub(crate) knock_mag: i16,
    /// Desired speed (v_12) toward which f126 accelerates 16/tick.
    pub(crate) vdes: i16,
    /// Spawn grace (u16_331): mailbox discarded while > 0.
    pub(crate) grace: u16,
    /// Post-hit regen stall (u32_383). Armed by the shared intake
    /// like retail's, but the AI regen NEVER reads it — only the
    /// HUMAN's regen tail does (:55387-90); the AI housekeeping
    /// (:17990-18021) heals straight through fresh hits.
    regen_stall: u16,
    /// Life-regen rate REGISTER (u16_341): the housekeeping APPLIES
    /// this, then re-selects it from the at-castle/shrine test
    /// (:17994-18018) — the rate applied at tick N was chosen at
    /// N−1, the AI twin of the human's :55388 staircase.
    life_rate: i32,
    /// Dead and castle-less: permanently out (byte_13329_6 = 0,
    /// :55622). Property is NOT torn down.
    pub eliminated: bool,
    /// Buff flags derived from the manifestations' bursts.
    pub shield: bool,
    pub invisible: bool,
    pub rebound: bool,
}

impl Rival {
    pub(crate) fn new(slot: u8, ent: u16, cfg: &RivalConfig) -> Self {
        let mut cooldown = [0u16; SPELL_COUNT];
        // The castle-build stagger (:55049): 4 ticks per player slot.
        cooldown[16] = 4 * slot as u16;
        Rival {
            slot,
            ent,
            owned: [0; SPELL_COUNT],
            known: [false; SPELL_COUNT],
            allowed: cfg.allowed,
            learn: [0; SPELL_COUNT],
            cooldown,
            mana: 1000,
            mana_max: 1000,
            mana_delta: 0,
            agg: cfg.aggression as u16,
            acc: cfg.accuracy as u16,
            tempo: cfg.tempo as u16,
            state: AiState::Fresh,
            hate: [HATE_NEUTRAL; 8],
            war: [false; 8],
            burst: 0,
            poverty: false,
            target: 0,
            target_sig: 0,
            site: (0, 0),
            jink: 0,
            knock_dir: 0,
            knock_mag: 0,
            vdes: 0,
            grace: 100,
            regen_stall: 0,
            life_rate: 0,
            eliminated: false,
            shield: false,
            invisible: false,
            rebound: false,
        }
    }

    /// Decision-tick gate: every `64 - tempo/4` ticks keyed on the
    /// entity age byte (:18024/:18065).
    fn think_period(&self) -> u8 {
        (64 - (self.tempo / 4) as i32).max(1) as u8
    }
}

/// Read-only snapshot of one rival's AI internals (diagnostics only).
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RivalAiDebug {
    pub slot: u8,
    pub state: String,
    pub target: u16,
    pub known: Vec<usize>,
    pub owned: Vec<usize>,
    pub allowed: Vec<usize>,
    pub has_offense: bool,
    pub mana: u32,
    pub mana_max: u32,
    pub poverty: bool,
    pub burst: i16,
    pub castle_stored: Option<u32>,
}

impl World {
    /// Wire the level's wizards from the baked config: one rival per
    /// active AI slot (1..player_count), spawned at its start marker
    /// (class-3 model 4+slot placement, str_9177 :44068-107) with its
    /// level-start book and starting castle (sub_44D30 :54802-55005).
    /// Slot 0 (the human) is ignored here — the human's book comes
    /// from the campaign/jar machinery.
    pub fn set_wizards(&mut self, configs: &[Option<RivalConfig>; 8], player_count: u16) {
        for slot in 1..player_count.min(8) as u8 {
            let Some(cfg) = &configs[slot as usize] else {
                continue;
            };
            self.spawn_rival(slot, cfg.clone());
        }
    }

    fn spawn_rival(&mut self, slot: u8, cfg: RivalConfig) {
        // Start marker: class 3, model 4+slot (tile-center position,
        // :44003); fall back to the human's start marker cell.
        let marker = self.start_markers[slot as usize].or(self.start_markers[0]);
        let Some((mx, my)) = marker else { return };
        let x = (mx << 8).wrapping_add(128);
        let y = (my << 8).wrapping_add(128);
        let z = (self.g.ground_z(x, y) as i16).wrapping_add(256);
        let Some(i) = self.g.spawn_class3(1, x, y, z) else {
            return;
        };
        // Per-slot rival art (:54927-55): sprite-stats rows 273-279
        // (slot 0 keeps 44). Draw type 0x11: 16 views by mirror.
        self.g.set_sprite(i, 272 + slot as u16);
        self.g.refill_life(i);
        // Retail keeps a wizard's mana ON the entity (+140); the
        // port's authority is `Rival::mana`, with the entity as the
        // combat-visible mirror — the Rebound deflection's afford
        // gate and quarter debit (sub_52B30 :62858-90) read and
        // write THIS field, so it must track the pool.
        self.g.ent[i].f140 = 1000;
        let mut r = Rival::new(slot, i as u16, &cfg);
        // Level-start book (:49222): pregrant && allowed, as resolved
        // by the app into cfg.book.
        for s in 0..SPELL_COUNT {
            if cfg.book[s] {
                r.known[s] = true;
                if let Some(m) = self.mint_manifestation(s, i as u16) {
                    r.owned[s] = m as u16;
                }
            }
        }
        // Starting castle (:54963-55005): AI-only, needs the Castle
        // spell known and a nonzero tail level. Spawned at the wizard
        // (even-parity tile snap in the spawn handler), pre-leveled,
        // FULL of mana, terrain stamped to match.
        if cfg.castle_level > 0 && r.known[16] {
            self.spawn_starting_castle(&mut r, cfg.castle_level);
        }
        // Everyone hates a newcomer a little (:55037-41).
        for other in &mut self.rivals {
            other.hate[slot as usize] = HATE_RESPAWN;
        }
        // The team resolver for owner recolors (balls/balloons/flags).
        self.g.rival_ents[slot as usize] = i as u16;
        self.rivals.push(r);
        self.entities_dirty = true;
    }

    /// A rival-owned class-12 manifestation (the shared sub_3BF70
    /// slot economy; f144 = the owner tag — PLAYER_TARGET on the
    /// human's, 0 on ground jars; retail keeps the owner in +42 and
    /// +144 dead — the importer and every native mint normalize into
    /// f144).
    fn mint_manifestation(&mut self, spell: usize, owner: u16) -> Option<usize> {
        let m = self.g.new_event()?;
        let f44 = self.spells()[spell].damage.min(u16::MAX as u32) as u16;
        {
            let e = &mut self.g.ent[m];
            e.class64 = 12;
            e.model65 = spell as u8;
            e.tick70 = crate::engine::world::MANIFEST_BASE + spell as u8;
            e.flags &= !8;
            e.f26 = 0;
            e.f44 = f44;
            e.f144 = owner;
            // The spell-16 cost-cache ctor seed (sub_3BF70 :47996,
            // same as the human grant): 1000/9. Rival tokens keep it
            // until a castle EVENT stamps them — the mc1l5 take
            // shows Vodor's token at 1000/9 under his standing
            // authored castle.
            if spell == 16 {
                e.f136 = SPELLS[16].possess_mana as i32;
                e.f140 = SPELLS[16].possess_mana as i32 / 101;
            }
        }
        self.g.set_sprite(m, 77);
        let (h4, v4) = {
            let e = &self.g.ent[m];
            (e.f80 * 4, e.f84 * 4)
        };
        self.g.extents(m, h4, v4);
        Some(m)
    }

    /// Starting castle: (3,2) at the wizard, level = castle_level-1,
    /// footprint terrain replayed per stage (the sub_279D0 loop
    /// :54982-93), capacity ladder, spawns FULL (:54996-55002),
    /// sound 30 (:54981).
    fn spawn_starting_castle(&mut self, r: &mut Rival, castle_level: u8) {
        let (wx, wy) = {
            let e = &self.g.ent[r.ent as usize];
            (e.x, e.y)
        };
        let gz = self.g.ground_z(wx, wy) as i16;
        let Some(c) = self.g.spawn_class3(2, wx, wy, gz) else {
            return;
        };
        let lvl = (castle_level - 1).min(7);
        {
            let e = &mut self.g.ent[c];
            e.id24 = r.ent;
            e.f26 = lvl as i16;
            e.tick70 = 4; // standing (the buildable state the AI
            // upgrade gate checks, :18432)
        }
        // The castle flag wears the owner's team colors (sprite
        // 177 + team, the :30809-10 family).
        self.g.set_sprite(c, 177 + r.slot as u16);
        // Terrain: replay the build painter per stage (instant, the
        // divisor-1 flatten + paint). Retail's loop runs one pass per
        // AUTHORED LEVEL with the build row = the pass index
        // (:54983-91 `+29866 = i`, i = 0..count-1), so the rows
        // stamped are 0..=lvl — and row 0 is EMPTY (w = h = 0). Our
        // rows 1..=lvl cover exactly the same ground; passing lvl + 1
        // stamped one row too many, which for the common authored
        // level 0 (`castle_level` 1) raised a whole level-1 tower that
        // the castle never owned and the demolish never removed.
        let (cx, cy, cz) = {
            let e = &self.g.ent[c];
            (
                ((e.x as u32 + 128) >> 8) as u8,
                ((e.y as u32 + 128) >> 8) as u8,
                (e.z >> 5) as i32,
            )
        };
        self.g.stamp_castle_terrain(lvl as usize, cx, cy, cz);
        self.terrain_dirty = true;
        // Extents + capacity ladder + full stored mana (cap 320000).
        self.g.castle_extents(c, lvl);
        let cap = Gen::CASTLE_CAP[lvl as usize];
        self.g.ent[c].f136 = cap;
        self.g.ent[c].f140 = cap.clamp(0, 320_000);
        self.g.snd(30, c);
        let _ = r;
    }

    /// The rival's castle: (3,2) with id24 = the rival's entity (the
    /// original's Type_160.var_50, resolved by scan like
    /// [`World::player_castle`]).
    pub(crate) fn rival_castle(&self, ent: u16) -> Option<usize> {
        (1..self.g.ent.len()).find(|&j| {
            let e = &self.g.ent[j];
            e.class64 == 3 && e.model65 == 2 && e.flags & 0x400 == 0 && e.id24 == ent
        })
    }

    /// Read-only AI diagnostic dump (no state mutation, not hashed) — for
    /// "follows target, casts nothing" style rival-AI investigations.
    #[doc(hidden)]
    pub fn debug_rival_ai(&self) -> Vec<crate::mc1::rivals::RivalAiDebug> {
        self.rivals
            .iter()
            .enumerate()
            .map(|(ri, r)| {
                let castle = self.rival_castle(r.ent);
                RivalAiDebug {
                    slot: r.slot,
                    state: format!("{:?}", r.state),
                    target: r.target,
                    known: (0..SPELL_COUNT).filter(|&s| r.known[s]).collect(),
                    owned: (0..SPELL_COUNT).filter(|&s| r.owned[s] != 0).collect(),
                    allowed: (0..SPELL_COUNT).filter(|&s| r.allowed[s]).collect(),
                    has_offense: self.rival_has_offense(ri),
                    mana: r.mana,
                    mana_max: r.mana_max,
                    poverty: r.poverty,
                    burst: r.burst,
                    castle_stored: castle.map(|c| self.g.ent[c].f140.max(0) as u32),
                }
            })
            .collect()
    }

    /// Resolve an owner tag (projectile id24 / claim f144) to a
    /// player slot: PLAYER_TARGET = 0 (the human), a live rival's
    /// entity slot = its player slot. Consults both rival columns.
    pub(crate) fn owner_slot(&self, owner: u16) -> Option<u8> {
        if owner == PLAYER_TARGET {
            return Some(0);
        }
        self.rivals
            .iter()
            .find(|r| r.ent == owner)
            .map(|r| r.slot)
            .or_else(|| {
                self.mc2_rivals
                    .iter()
                    .find(|r| r.ent == owner)
                    .map(|r| r.slot)
            })
    }

    // ---- the per-tick brain (sub_13170 :17842) ---------------------------

    /// Class-3 model-1 pool dispatch: resolve the rival record; a
    /// level-authored husk with no record stands and renders (the
    /// pre-rivals behavior).
    pub(crate) fn rival_entity_tick(&mut self, i: usize) {
        let Some(ri) = self.rivals.iter().position(|r| r.ent as usize == i) else {
            return;
        };
        if self.rivals[ri].eliminated {
            return;
        }
        match self.g.ent[i].tick70 {
            // Death fall (state 2, sub_45FC0 :55434).
            2 => self.rival_death_fall(ri, i),
            // Dead on the ground (state 3, sub_46480 :55594).
            3 => self.rival_dead_wait(ri, i),
            // Alive (state 1).
            _ => self.rival_alive_tick(ri, i),
        }
    }

    fn rival_alive_tick(&mut self, ri: usize, i: usize) {
        // Pull combat-side debits out of the +140 mana mirror (the
        // deflection quarter, sub_52B30 :62884, writes the ENTITY
        // field). Downward-only: every port-side credit lands in
        // `Rival::mana` first and re-publishes at this tick's end.
        let mirrored = self.g.ent[i].f140.max(0) as u32;
        if mirrored < self.rivals[ri].mana {
            self.rivals[ri].mana = mirrored;
        }
        // ---- housekeeping (sub_132B0 :17903) ----
        // Burst lockout recovery (:17936-38).
        if self.rivals[ri].burst < 0 {
            self.rivals[ri].burst += 1;
        }
        // AI recast cooldowns (:17939-45).
        for c in self.rivals[ri].cooldown.iter_mut() {
            *c = c.saturating_sub(1);
        }
        self.rival_hate_decay(ri);

        // At own castle: grace 2 + the mailbox is DISCARDED — the
        // AI's damage does NOT forward into the castle. VERIFIED
        // verbatim (:17971-79: overlap test sub_11950 → +331=2;
        // while +331: memset(+90,0,36), no intake). The asymmetry
        // vs the human's explicit redirect (:55353-62) is retail's
        // own; the castle still takes AREA-blast collateral through
        // its normal ch0 mail, which is how a camping rival's
        // castle falls in retail.
        let castle = self.rival_castle(self.rivals[ri].ent);
        let at_castle = castle.is_some_and(|c| {
            let (ex, ey) = (self.g.ent[i].x, self.g.ent[i].y);
            let e = &self.g.ent[c];
            ((ex.wrapping_sub(e.x) as i16).unsigned_abs()) <= e.f80
                && ((ey.wrapping_sub(e.y) as i16).unsigned_abs()) <= e.f82
        });
        if at_castle {
            // Retail SETS 2 (:17975 `+331 = 2`) — a spawn grace still
            // counting is OVERWRITTEN at the own castle, not floored.
            self.rivals[ri].grace = 2;
        }
        if self.rivals[ri].grace > 0 {
            self.rivals[ri].grace -= 1;
            self.g.ent[i].mail = [(0, 0); 6];
        } else {
            self.rival_damage_intake(ri, i);
            if self.g.ent[i].act_life < 0 {
                // Death (:17980-83): state 2, the fall.
                self.g.ent[i].tick70 = 2;
                self.g.ent[i].f46 = 0;
                // Drop the Rebound bit with the wizard. Retail's token
                // keeps ticking through the death states and clears
                // +17 bit 7 when its burst lapses (sub_573F0_57920
                // :65774); the port drives rival tokens only from
                // `rival_refresh_buffs`, which death states 2/3 never
                // reach — without this a corpse would deflect for the
                // rest of the level.
                self.g.ent[i].flags &= !0x8000;
                self.g.snd(16, i); // the death scream (:55424-30)
                // The death arm returns from the HOUSEKEEPING only —
                // its caller runs the state handler regardless. See
                // [`Self::rival_dispatch_tail`].
                self.rival_dispatch_tail(ri, i);
                return;
            }
        }

        // Movement (sub_14EB0 :18780).
        self.rival_movement(ri, i);

        // The cast-charge meter (u8_326): +1 per live rival tick,
        // saturating at 200 (:17987-89) — right before the regen
        // block, exactly like retail's rival handler.
        let ws = self.rivals[ri].slot as usize;
        if self.wiz_charge[ws] < 200 {
            self.wiz_charge[ws] += 1;
        }
        // Regen (:17990-18021): mana += delta then recompute; life
        // regen at the AI's own (faster) rates. The dolmen-shrine
        // flag (+17 0x10, our 0x1000 — stamped by the dolmen's
        // sub_49AD0 sweep) rides the same fast/slow fork as the
        // own-castle overlap and is consumed (cleared) by the fast
        // branch (:18002-09).
        let at_shrine = self.g.ent[i].flags & 0x1000 != 0;
        {
            let r = &mut self.rivals[ri];
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
        // Life regen (:17990-18018): act_life += the RATE REGISTER,
        // floor −1 / ceiling max, then the register re-selects from
        // the same at-castle/shrine fork as the mana delta —
        // UNCONDITIONALLY. The AI regen has NO stall gate: the shared
        // intake arms +383 = 16 on every processed hit, but only the
        // human's regen tail reads that field; retail's rival heals
        // +20 straight through a −100 fireball tick (the l2 corpus
        // life lane measures the −80 net).
        {
            let max = self.g.ent[i].max_life as i32;
            let rate = self.rivals[ri].life_rate;
            let e = &mut self.g.ent[i];
            e.act_life = (e.act_life + rate).clamp(-1, max);
            self.rivals[ri].life_rate = if at_castle || at_shrine {
                max / 200
            } else {
                max / 500
            };
        }
        if self.rivals[ri].regen_stall > 0 {
            self.rivals[ri].regen_stall -= 1;
        }

        // Spell learning (sub_15EC0 :19381-443).
        self.rival_learn_tick(ri);

        // Buff flags from the manifestations' bursts.
        self.rival_refresh_buffs(ri);

        // Decision-tick work (:18024): incoming-projectile defense +
        // heal.
        let think = self.g.ent[i].f63 % self.rivals[ri].think_period() == 0;
        if think {
            self.rival_defense(ri, i);
            if self.g.ent[i].act_life < self.g.ent[i].max_life as i32 {
                self.rival_cast(ri, i, 1);
            }
        }

        // Altitude hard clamp (:18035-41).
        {
            let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
            let ground = self.g.ground_z(self.g.ent[i].x, self.g.ent[i].y) as i16;
            let z = &mut self.g.ent[i].z;
            *z = (*z).clamp(
                ground.saturating_add(row.v_12),
                ground.saturating_add(row.v_10),
            );
        }

        self.rival_dispatch_tail(ri, i);
    }

    /// The brain's own body (sub_13170 :17846-51): the state handler
    /// and the decision cascade (a Fresh rival runs the cascade
    /// twice). It sits in the CALLER of the housekeeping, and the
    /// caller DISCARDS the housekeeping's return — so the death arm
    /// (:17980-84) ends `sub_132B0` only, never the brain. The dying
    /// rival therefore still runs its state handler on the fatal tick,
    /// which for an attack state is the hover leg (the cast attempt
    /// itself refuses — mc1l2 t=8278: Vodor is poverty-latched at mana
    /// 100/1000, so retail hovers 1514 → 1510 on row 8's v_14 = −4 and
    /// does not cast). Verbatim in the HW twin (:16112-17).
    fn rival_dispatch_tail(&mut self, ri: usize, i: usize) {
        let think = self.g.ent[i].f63 % self.rivals[ri].think_period() == 0;
        let fresh = self.rivals[ri].state == AiState::Fresh;
        self.rival_state_tick(ri, i, think);
        self.rival_selector(ri, i, think);
        if fresh {
            self.rival_selector(ri, i, think);
        }
        // Publish the +140 mana mirror for the combat reads.
        self.g.ent[i].f140 = self.rivals[ri].mana.min(i32::MAX as u32) as i32;
        self.entities_dirty = true;
    }

    /// Hate regression toward the baseline (:17946-67): below rises
    /// by agg+1, above decays by 256-agg — but a war flag pins it.
    fn rival_hate_decay(&mut self, ri: usize) {
        let (agg, war) = (self.rivals[ri].agg, self.rivals[ri].war);
        for (p, h) in self.rivals[ri].hate.iter_mut().enumerate() {
            if *h < HATE_NEUTRAL {
                *h = (*h + agg + 1).min(HATE_NEUTRAL);
            } else if *h > HATE_NEUTRAL && !war[p] {
                *h = h.saturating_sub(256 - agg).max(HATE_NEUTRAL);
            }
        }
    }

    /// The shared wizard damage intake (sub_46540 :55641) on the
    /// rival's mailbox. EVERY channel gates on its SOURCE word, and a
    /// consume clears ONLY the source — the amount stays behind as
    /// permanent residue retail never re-reads (the l2 corpus: Vodor
    /// carries a dead `(1400, 0)` ch0 letter for thousands of ticks;
    /// re-applying it at every imported pair was the 8k-row life
    /// family). Hate feed lives in `proj_hate_sweep`.
    fn rival_damage_intake(&mut self, ri: usize, i: usize) {
        // ch4 duel grip (:55663-82): the CASTER gets pulled toward
        // this victim; the victim only takes the side effects
        // (regen stall — the pull state lives on the ATTACKER).
        let (grip_amt, grip_src) = self.g.ent[i].mail[4];
        if grip_src != 0 {
            self.rivals[ri].regen_stall = 16;
            self.g.ent[i].mail[4] = (grip_amt, 0);
            if self.owner_slot_of_source(grip_src) == Some(0) {
                // u16_314/316/318 on the human: victim, counter,
                // clamp(dist, 1024, 3072) (:55671-77).
                let (vx, vy) = (self.g.ent[i].x, self.g.ent[i].y);
                let dist =
                    Gen::isqrt(Gen::dist2_sq(self.human_pose.0, self.human_pose.1, vx, vy) as u32);
                let hold = dist.clamp(1024, 3072);
                self.set_duel_latch(self.rivals[ri].ent, hold);
            }
        }
        // ch3 mana steal (:55689-91): the attacker banks it.
        let (steal_amt, steal_src) = self.g.ent[i].mail[3];
        if steal_src != 0 {
            let take = (steal_amt as u32).min(self.rivals[ri].mana);
            self.rivals[ri].mana -= take;
            self.credit_wizard_mana(steal_src, take);
            self.rivals[ri].regen_stall = 16;
            self.g.ent[i].mail[3] = (steal_amt, 0);
        }
        // ch0 damage (:55694-56737): SRC-gated — a src-0 letter is
        // dead residue, not applied and not cleared.
        let (amt, src) = self.g.ent[i].mail[0];
        if src == 0 {
            return;
        }
        let mut dmg = amt.min(i32::MAX as u32) as i32;
        // Shield quarter (:55700-07): keyed on the ENTITY's 0x4000
        // bit (retail +17 & 0x40) — the imported flag, not the
        // port-side buff mirror — quartered amount written BACK to
        // the letter (the residue keeps the reduced value), mana pays
        // it, and the bit clears ONE-SHOT.
        if self.g.ent[i].flags & 0x4000 != 0 {
            dmg /= 4;
            let pay = (dmg.max(0) as u32).min(self.rivals[ri].mana);
            self.rivals[ri].mana -= pay;
            self.g.ent[i].flags &= !0x4000;
        }
        self.g.ent[i].act_life -= dmg;
        // Knockback (:55714-19), armed on ANY sourced letter — the
        // fatal one included, since this block precedes the death
        // return at :55726. v_24 = the attacker→victim bearing, v_22 =
        // amount/10 clamped to [0, 80]. Retail's gate is just
        // `src > 0`: the human is a pool entity there, but the port
        // stamps human-fired projectiles with PLAYER_TARGET, so that
        // case reads the pinned human pose instead (as `home` and the
        // area writers already do) — without it every rival the PLAYER
        // kills would drop straight down and the law would be
        // corpus-only.
        let attacker = if src == PLAYER_TARGET {
            Some((self.human_pose.0, self.human_pose.1))
        } else {
            let s = src as usize;
            (s != 0 && s < self.g.ent.len() && self.g.ent[s].class64 != 0)
                .then(|| (self.g.ent[s].x, self.g.ent[s].y))
        };
        if let Some((ax, ay)) = attacker {
            let (vx, vy) = (self.g.ent[i].x, self.g.ent[i].y);
            self.rivals[ri].knock_dir = Gen::angle_between(ax, ay, vx, vy) & 0x7FF;
            self.rivals[ri].knock_mag = ((dmg.max(0) / 10) as i16).clamp(0, 80);
        }
        self.rivals[ri].regen_stall = 16;
        self.g.snd(17, i);
        if self.g.ent[i].act_life < 0 {
            // Death (:55734-36): the killer latch stamps ONLY here,
            // and the letter is NOT consumed — the corpse keeps it.
            self.g.ent[i].f38 = src;
            self.g.ent[i].mail[0] = (dmg.max(0) as u32, src);
            return;
        }
        // Survive: consume = clear the source, keep the (possibly
        // quartered) amount (:55738-40).
        self.g.ent[i].mail[0] = (dmg.max(0) as u32, 0);
    }

    /// Resolve a mailbox source id to the attacking wizard's slot:
    /// sources carry the attacker's owner tag directly (our writers
    /// pass id24 through).
    pub(crate) fn owner_slot_of_source(&self, src: u16) -> Option<u8> {
        if src == PLAYER_TARGET {
            return Some(0);
        }
        // A pool slot: use its owner tag; a wizard entity is its own.
        let e = self.g.ent.get(src as usize)?;
        if e.class64 == 3 && e.model65 <= 1 {
            return self.owner_slot(src);
        }
        self.owner_slot(e.id24)
    }

    /// Bump the ledger (`str_456[shooter].u16_4` += amount, clamped —
    /// :19727-32). No war check here: retail raises the flag ONLY in
    /// the castle arm of the sweep (:19733-39); the carpet/balloon
    /// and mana-ball arms bump and stop.
    fn rival_add_hate(&mut self, ri: usize, shooter: u8, amount: u16) {
        if shooter as usize >= 8 || self.rivals[ri].slot == shooter {
            return;
        }
        let r = &mut self.rivals[ri];
        r.hate[shooter as usize] = r.hate[shooter as usize].saturating_add(amount);
    }

    /// The castle-arm war check (:19733-39): hate past
    /// `50000 − shooter_wealth/10 × victim_agg/255` raises the war
    /// flag. The wealth is the SHOOTER's max mana and the aggression
    /// the VICTIM's — the MC2 twin spells it out unambiguously
    /// (`v1x->maxMana_0x8C * v2x_owner->word_0x242` EF:7402-03); an
    /// earlier port fold used the victim's wealth.
    fn rival_war_check(&mut self, ri: usize, shooter: u8, shooter_wealth: u32) {
        if shooter as usize >= 8 || self.rivals[ri].slot == shooter {
            return;
        }
        let r = &mut self.rivals[ri];
        let scaled = shooter_wealth / 10 * r.agg as u32 / 255;
        let threshold = 50_000u32.saturating_sub(scaled);
        if r.hate[shooter as usize] as u32 > threshold {
            r.war[shooter as usize] = true;
        }
    }

    /// The per-projectile hate/war ledger sweep `sub_16540` (:19643),
    /// called once per tick from [`crate::engine::world`]'s tick
    /// between the reap/list phase and the mana census (:52326,
    /// ahead of every entity handler). Each class-9 record is
    /// ledgered ONCE — flags 0x2000 is the mark (:19666/:19678), set
    /// the first tick the bolt has BOTH a class-3 owner and a victim
    /// in +146, whether or not a table below applies. A bolt that
    /// MISSED at the muzzle never latches (no victim), and is
    /// re-examined every tick until it dies or acquires one (the
    /// rebound path can hand it a victim mid-flight).
    ///
    /// The corpus-visible half is the mark itself — mc1l0's 202-row
    /// `flags want 8198 got 6` family. The tables drive rival
    /// aggression: victim's wizard gains hate against the shooter,
    /// keyed on the PROJECTILE model ({3,4,11,16} heavy → +3000
    /// carpet/balloon, +5000 castle; model 10 → nothing; else
    /// +500/+1000), the castle arm alone running the war check. A
    /// possess lob (m1) locked onto a CLAIMED mana ball (10,39)
    /// bumps the claimant's wizard by ball_mana/4 (:19742-61).
    ///
    /// ⚠ Two remc1 transcription slips corrected via the MC2 twin
    /// `sub_159E0` (EF:7320): the carpet-arm base-read is the
    /// VICTIM-owner's table (remc1's text reads `v2->id24` — the
    /// shooter's; the twin reads `ent[target.id]`), and BOTH arms
    /// key the bonus on the projectile MODEL (the text reads +63 in
    /// the carpet arm; the twin reads `model_0x40` in both). MC2 is
    /// not wired to this sweep yet — zero class-9 flags signal in the
    /// four MC2 takes; its own frame does call the twin (EF:786).
    ///
    /// Human-victim writes go to retail's human T160 tables, which
    /// nothing consumes (the human has no AI); the port keeps no such
    /// store, so those arms latch the mark and stop.
    pub(crate) fn proj_hate_sweep(&mut self) {
        for i in 1..self.g.ent.len() {
            let e = &self.g.ent[i];
            if e.class64 != 9 || e.flags & 0x2000 != 0 {
                continue;
            }
            let (own, tgt, model) = (e.id24, e.f146, e.model65);
            let owner_ok = own == PLAYER_TARGET
                || (own != 0
                    && (own as usize) < self.g.ent.len()
                    && self.g.ent[own as usize].class64 == 3);
            if !owner_ok || tgt == 0 {
                continue;
            }
            self.g.ent[i].flags |= 0x2000; // ledgered (:19678)
            let Some(shooter) = self.owner_slot_of_source(own) else {
                continue;
            };
            if tgt == PLAYER_TARGET || tgt as usize >= self.g.ent.len() {
                continue; // human tables unmodeled (see above)
            }
            let t = &self.g.ent[tgt as usize];
            let (tclass, tmodel) = (t.class64, t.model65);
            if tclass == 3 {
                let Some(victim) = self.owner_slot_of_source(tgt) else {
                    continue;
                };
                let Some(ri) = self.rivals.iter().position(|r| r.slot == victim) else {
                    continue;
                };
                if tmodel == 2 {
                    let bonus = match model {
                        3 | 4 | 11 | 16 => 5000,
                        10 => 0,
                        _ => 1000,
                    };
                    self.rival_add_hate(ri, shooter, bonus);
                    let wealth = self.wizard_mana_max(shooter);
                    self.rival_war_check(ri, shooter, wealth);
                } else {
                    let bonus = match model {
                        3 | 4 | 11 | 16 => 3000,
                        10 => 0,
                        _ => 500,
                    };
                    self.rival_add_hate(ri, shooter, bonus);
                }
            } else if tclass == 10 && model == 1 && tmodel == 39 {
                // The claimed-ball arm: possessing someone's claimed
                // sphere is an act of war-adjacent theft.
                let claimant = t.f144;
                let mana = t.f140.max(0) as u32;
                if claimant == 0 {
                    continue;
                }
                let claimant_ok = claimant == PLAYER_TARGET
                    || ((claimant as usize) < self.g.ent.len()
                        && self.g.ent[claimant as usize].class64 == 3);
                if !claimant_ok {
                    continue;
                }
                let Some(victim) = self.owner_slot_of_source(claimant) else {
                    continue;
                };
                let Some(ri) = self.rivals.iter().position(|r| r.slot == victim) else {
                    continue;
                };
                let bump = (mana / 4).min(u16::MAX as u32) as u16;
                self.rival_add_hate(ri, shooter, bump);
            }
        }
    }

    /// A wizard's max-mana by player slot (the sweep's war-threshold
    /// wealth — retail reads the shooter CARPET's +136 mirror).
    fn wizard_mana_max(&self, slot: u8) -> u32 {
        if slot == 0 {
            return self.player.mana_max;
        }
        self.rivals
            .iter()
            .find(|r| r.slot == slot)
            .map(|r| r.mana_max)
            .unwrap_or(0)
    }

    /// Credit stolen mana to a wizard by owner tag.
    pub(crate) fn credit_wizard_mana(&mut self, owner: u16, amount: u32) {
        if owner == PLAYER_TARGET {
            self.player.mana = (self.player.mana + amount).min(self.player.mana_max);
            return;
        }
        if let Some(r) = self.rivals.iter_mut().find(|r| r.ent == owner) {
            r.mana = (r.mana + amount).min(r.mana_max);
            return;
        }
        if let Some(r) = self.mc2_rivals.iter_mut().find(|r| r.ent == owner) {
            r.mana = (r.mana + amount).min(r.mana_max);
        }
    }

    /// AI carpet movement (sub_14EB0 :18780-859): band-settle
    /// altitude, always-level forward step, lateral dodge, 16/tick
    /// accel toward the desired speed, tempo-scaled turn toward the
    /// desired heading. No wall gate, no drag/knock.
    fn rival_movement(&mut self, ri: usize, i: usize) {
        let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
        let (v10, v12, v14) = (row.v_10, row.v_12, row.v_14);
        let (v2, v4) = (row.v_2, row.v_4);
        let ground = self.g.ground_z(self.g.ent[i].x, self.g.ent[i].y) as i16;
        {
            let e = &mut self.g.ent[i];
            // sub_42000 (:52576): the band settle.
            if e.z > ground.saturating_add(v10) {
                e.z = e.z.saturating_add(v14);
            } else if e.z > ground.saturating_add(v12) {
                e.z = e.z.saturating_add((v14 as i32 * 25 / 100) as i16);
            }
            if e.z < ground.saturating_add(v12) {
                e.z = ground.saturating_add(v12);
            }
        }
        // Forward (always level) + lateral dodge, then commit.
        let (yaw, speed, jink) = {
            let e = &self.g.ent[i];
            (e.f30, e.f126, self.rivals[ri].jink)
        };
        let mut pos = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        Gen::polar_step(&mut pos, yaw, 0, speed);
        if jink != 0 {
            Gen::polar_step(&mut pos, yaw.wrapping_add(0x200) & 0x7FF, 0, jink);
            self.rivals[ri].jink -= 4 * jink.signum();
        }
        self.g.move_relink(i, pos.0, pos.1, pos.2);
        // Accel 16/tick toward the desired speed (:18828-31).
        {
            let vdes = self.rivals[ri].vdes;
            let e = &mut self.g.ent[i];
            e.f126 += 16 * (vdes - e.f126).signum();
            // Turn toward the desired heading (:18835-57): rate =
            // err / (8 + (255-tempo)/16), clamped to the row's caps,
            // applied FULL then snapped to +34 only when the raw u16
            // compare says the step crossed it — retail keeps an
            // overshoot that wrapped through zero (no snap there),
            // where a min(err) step would land exactly.
            let err = Gen::angdist(e.f30, e.f34 & 0x7FF) as i32;
            let div = 8 + ((255 - self.rivals[ri].tempo as i32) / 16);
            let step = (err / div).clamp(v4 as i32, v2 as i32) as i16;
            let old = e.f30;
            let des = e.f34;
            let new = (old as i32 + (Gen::turn_sign(old, des) * step) as i32) as u16 & 0x7FF;
            e.f30 = new;
            if (old < des && new > des) || (old > des && des > new) {
                e.f30 = des;
            }
        }
    }

    /// Spell learning (:64805-14 arm + sub_15EC0 :19381-443 expiry):
    /// any ground jar of an unowned, allowed spell arms a 200-tick
    /// countdown; at expiry the rival conjures its own manifestation.
    /// (Arming is folded here into the countdown scan — one pool walk
    /// per tick instead of the jar tick writing into every rival.)
    fn rival_learn_tick(&mut self, ri: usize) {
        for s in 0..SPELL_COUNT {
            if self.rivals[ri].known[s] || !self.rivals[ri].allowed[s] {
                continue;
            }
            if self.rivals[ri].learn[s] > 1 {
                self.rivals[ri].learn[s] -= 1;
                continue;
            }
            if self.rivals[ri].learn[s] == 1 {
                // Conjure the copy (off_987DE[s] :19415-31).
                let ent = self.rivals[ri].ent;
                self.rivals[ri].learn[s] = 0;
                self.rivals[ri].known[s] = true;
                if let Some(m) = self.mint_manifestation(s, ent) {
                    self.rivals[ri].owned[s] = m as u16;
                }
                continue;
            }
            // Arm on a matching ground jar existing anywhere
            // (:64805-14; jars have tick70 < MANIFEST_BASE).
            let exists = (1..self.g.ent.len()).any(|j| {
                let e = &self.g.ent[j];
                e.class64 == 12
                    && e.model65 as usize == s
                    && e.tick70 < crate::engine::world::MANIFEST_BASE
                    && e.flags & 0x400 == 0
            });
            if exists {
                self.rivals[ri].learn[s] = 200;
            }
        }
    }

    /// Resolve a rival's own manifestation slot for `spell`, rejecting
    /// a STALE binding. `owned[]` is minted by the port
    /// ([`World::mint_manifestation`], `tick70 = MANIFEST_BASE +
    /// spell`, `f144` = the owner), but a conformance import replaces
    /// the whole pool from the recording WITHOUT rebinding it (the
    /// retail token's owner lives in its `+42`, which the port's `Ent`
    /// does not carry, so the importer cannot re-anchor the book) —
    /// the slot then holds a different entity entirely. Running the
    /// burst lanes on it would decrement a stranger's `f26` and
    /// publish buff bits from noise. Imported class-12 tokens keep
    /// RETAIL's encoding (`tick70 = spell*3 + phase`, always <
    /// [`MANIFEST_BASE`]), so the state test is exact and total.
    fn rival_token(&self, ri: usize, spell: usize) -> Option<usize> {
        let m = self.rivals[ri].owned[spell] as usize;
        let e = self.g.ent.get(m)?;
        // Both encodings are owned TOKENS: the native MANIFEST_BASE +
        // spell, and retail's phase-0 `spell*3` (a conformance import
        // — `owned` comes from the record's +676 there, and the
        // importer stamps +42 into f144, so the binding is anchored).
        let tick_ok =
            e.tick70 >= crate::engine::world::MANIFEST_BASE || e.tick70 as usize == spell * 3;
        (m != 0
            && e.class64 == 12
            && e.model65 as usize == spell
            && tick_ok
            && e.f144 == self.rivals[ri].ent
            && e.flags & 0x400 == 0)
            .then_some(m)
    }

    /// The rival-owned LAUNCHER token's burst machine (retail's
    /// sub_56090 :65100 head → sub_55DD0 gate → the per-spell bolt
    /// spawners), run at the TOKEN's own pool slot from
    /// `class12_tick` — both encodings. The commit
    /// ([`World::rival_cast`]) only arms +48 = +50; here the FULL
    /// tick fires the emission from the owner's settled pose (+30/+32
    /// — the commit stamped the pitch) and lands the sub_55E80 debit
    /// on the regen delta, a MID-burst tick pins a positive delta to
    /// 0 (the pool freezes for the whole burst), and the counter
    /// decrements LAST (:65260). A refused full tick (dead wizard /
    /// short pool) drops the burst to 1 so the shared decrement zeroes
    /// it (:64926-31) — silent for the AI, no buzz.
    pub(crate) fn rival_manifestation_tick(&mut self, m: usize, ri: usize, spell: usize) {
        if !matches!(spell, 0 | 3 | 7 | 8 | 11 | 13 | 15 | 17 | 20) {
            return;
        }
        let f26 = self.g.ent[m].f26;
        if f26 <= 0 {
            return;
        }
        let def = &self.spells()[spell];
        let count = def.count as i16;
        let cost = def.possess_mana;
        let i = self.rivals[ri].ent as usize;
        // The OWNER-DEAD refusal runs on EVERY burst tick, not just the
        // full one: the token handler calls sub_55DD0_56300 before the
        // full/mid split (:65030-88), and that gate refuses on
        // `owner.+140 < 0 || owner.+12 < 0` (:64915-24) — no state
        // test, and the mid-burst path short-circuits before the mana
        // compare. Any refusal sets +48 = 1 and falls into the
        // unconditional decrement (:64926-31, :65046), so a wizard
        // dying mid-burst cancels the rest of it in one double drop —
        // mc1l2 token 301 goes 2 → 0 on the tick rival 300's act_life
        // crosses to −280.
        if self.g.ent[i].act_life < 0 {
            self.g.ent[m].f26 = 1;
            if self.g.ent[m].f26 > 0 {
                self.g.ent[m].f26 -= 1;
            }
            return;
        }
        if f26 == count {
            let alive = self.g.ent[i].tick70 == 1;
            if alive && self.rivals[ri].mana >= cost {
                let (ex, ey, ez, yaw, pitch, mz) = {
                    let e = &self.g.ent[i];
                    (e.x, e.y, e.z, e.f30, e.f32, e.z.wrapping_add(e.f78 as i16))
                };
                let _ = ez;
                self.rival_emit(ri, i, spell, ex, ey, mz, yaw, pitch);
                // sub_55E80's full arm (:64942-52) — LIVE in retail
                // (the remc1 `//fix` comment-out is the maintainer's).
                let r = &mut self.rivals[ri];
                let c = cost.min(i32::MAX as u32) as i32;
                r.mana_delta = if r.mana_delta >= 0 {
                    -c
                } else {
                    r.mana_delta - c
                };
            } else {
                self.g.ent[m].f26 = 1;
            }
        } else {
            // Mid-burst regen pin (sub_55E80's else arm :64956).
            if self.rivals[ri].mana_delta > 0 {
                self.rivals[ri].mana_delta = 0;
            }
        }
        if self.g.ent[m].f26 > 0 {
            self.g.ent[m].f26 -= 1;
        }
    }

    /// Buff flags derive from the manifestations' burst counters
    /// (the human's manifestation_tick equivalents; the rival's
    /// bursts are armed by [`World::rival_cast`] and decremented
    /// here).
    fn rival_refresh_buffs(&mut self, ri: usize) {
        let mut get = |spell: usize| -> bool {
            let Some(m) = self.rival_token(ri, spell) else {
                return false;
            };
            if self.g.ent[m].f26 > 0 {
                self.g.ent[m].f26 -= 1;
            }
            self.g.ent[m].f26 > 0
        };
        let shield = get(4);
        let invisible = get(12);
        let rebound = get(14);
        let driving = self.rival_token(ri, 14).is_some();
        // Heal channel (1): 5% per tick while live, paid per tick.
        let heal_m = self.rival_token(ri, 1).unwrap_or(0);
        let healing = heal_m != 0 && self.g.ent[heal_m].f26 > 0;
        if healing {
            let def = &SPELLS[1];
            if self.rivals[ri].mana >= def.possess_mana / def.count as u32 {
                self.rivals[ri].mana -= def.possess_mana / def.count as u32;
                let i = self.rivals[ri].ent as usize;
                let max = self.g.ent[i].max_life as i32;
                self.g.ent[i].act_life = (self.g.ent[i].act_life + max / 20).min(max);
            }
            self.g.ent[heal_m].f26 -= 1;
        }
        // Speed-up (2) burst rides f26 too; consumed by the approach
        // helper's boost checks.
        let m2 = self.rival_token(ri, 2).unwrap_or(0);
        if m2 != 0 && self.g.ent[m2].f26 > 0 {
            self.g.ent[m2].f26 -= 1;
        }
        {
            let r = &mut self.rivals[ri];
            r.shield = shield;
            r.invisible = invisible;
            r.rebound = rebound;
        }
        // Mirror the cloak onto the entity's 0x20 bit — the shared
        // draw/targeting suppressor (:65689-90); only while alive
        // (death owns the bit in states 2/3).
        let i = self.rivals[ri].ent as usize;
        if self.g.ent[i].tick70 == 1 {
            if invisible {
                self.g.ent[i].flags |= 0x20;
            } else {
                self.g.ent[i].flags &= !0x20;
            }
        }
        // Mirror the Rebound token onto the entity's +17 bit 7 (our
        // 0x8000) — the ONLY thing that makes a rival's Rebound do
        // anything. The token's own tick is what publishes the bit in
        // retail: the class-12 handler 0x2A (`str_2563D8` entry 0x2A,
        // :4996) is `sub_573F0_57920` (remc1 :65774 / remc1hw :61996),
        // which sets `owner->+17 |= 0x80` on every tick the burst is
        // live and clears it the tick the burst runs out. The port
        // skips rival-owned manifestations in `class12_tick` (they are
        // driven from here instead), so the bit was never published and
        // the deflection reader (`proj_move_and_hit`, `flags & 0x8000`,
        // :62848-62890) never saw a rival Rebound — nothing ever
        // bounced off an AI wizard. Retail's clear arm is
        // unconditional, so the port clears here too — but ONLY while
        // the port is the one driving the token (`rival_token`): under
        // a conformance import the bit belongs to retail's own token
        // tick and the port must not touch it.
        if driving {
            if rebound {
                self.g.ent[i].flags |= 0x8000;
            } else {
                self.g.ent[i].flags &= !0x8000;
            }
        }
    }

    /// Incoming-projectile defense (sub_16800 :19769 + sub_16870/90):
    /// the nearest class-9 homing on me within 5120 → lateral jink 80 +
    /// a reactive cast (models {0,3,16} → 14 Rebound, {4,9} → 4
    /// Shield).
    fn rival_defense(&mut self, ri: usize, i: usize) {
        let me = self.rivals[ri].ent;
        let (px, py, pz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        let mut best: Option<(usize, i32)> = None;
        for j in 1..self.g.ent.len() {
            let e = &self.g.ent[j];
            if e.class64 != 9 || e.flags & 0x400 != 0 || e.f146 != me {
                continue;
            }
            let d2 = Gen::dist2_sq(px, py, e.x, e.y);
            let dz = e.z.wrapping_sub(pz) as i32;
            let d3 = d2.wrapping_add(dz.wrapping_mul(dz));
            if d3 <= 5120 * 5120 && best.is_none_or(|(_, bd)| d3 < bd) {
                best = Some((j, d3));
            }
        }
        let Some((threat, d3)) = best else { return };
        self.rivals[ri].jink = 80;
        if d3 <= 1024 * 1024 {
            // Verbatim `sub_16890` (remc1 :19815-52 / remc1hw
            // :17947-84). Two corrections to the old port:
            //
            // (a) the model switch's DEFAULT arm casts nothing —
            //     models 1/2 fall out of the `< 4` branch and 5..8 out
            //     of the `>= 9` branch with no call, and `!= 16`
            //     returns outright. The port folded every unlisted
            //     model into Shield, burning the token (and 2000 mana)
            //     on threats retail ignores.
            // (b) the fire-spell arm is a LADDER, not a pick:
            //     `if (sub_15A00(a1,0xE)) sub_155F0(a1,0xE); else if
            //     (sub_15A00(a1,4)) sub_155F0(a1,4);` — with Rebound
            //     already live (its readiness gate), the rival falls
            //     through to Shield instead of standing there.
            match self.g.ent[threat].model65 {
                0 | 3 | 16 => {
                    if self.rival_cast_ready(ri, 14) {
                        self.rival_cast(ri, i, 14);
                    } else if self.rival_cast_ready(ri, 4) {
                        self.rival_cast(ri, i, 4);
                    }
                }
                4 | 9 if self.rival_cast_ready(ri, 4) => {
                    self.rival_cast(ri, i, 4);
                }
                _ => {}
            }
        }
    }

    // ---- the decision cascade (sub_136C0 :18048) --------------------------

    /// The LIVE Create-Castle price — the manifestation's +136 cost
    /// cache, which retail's want/commit gates read (sub_15E90
    /// :19375: `manifest +136 <= wizard +136`). Ctor 1000; CAP[lvl]
    /// while a castle stands; re-stamped CAP[0] = 5000 by the
    /// teardown (sub_47A70 → sub_47C60 case 0) — the rival rebuild
    /// POVERTY GATE: a razed, mana-starved rival (census ceiling
    /// collapsed to the 1000 base) refuses to rebuild until claims
    /// push mana_max past 5000 (mc1l5 take: Vodor rebuilds at
    /// t=17643, the tick mana_max crosses 5000).
    fn rival_castle_price(&self, ri: usize) -> u32 {
        let m = self.rivals[ri].owned[16] as usize;
        if m != 0
            && m < self.g.ent.len()
            && self.g.ent[m].class64 == 12
            && self.g.ent[m].flags & 0x400 == 0
        {
            self.g.ent[m].f136.max(0) as u32
        } else {
            SPELLS[16].possess_mana
        }
    }

    fn rival_selector(&mut self, ri: usize, i: usize, think: bool) {
        // 1. Need a castle (sub_13F00 :18345).
        let castle = self.rival_castle(self.rivals[ri].ent);
        if castle.is_none()
            && self.rivals[ri].known[16]
            && self.rivals[ri].mana_max >= self.rival_castle_price(ri)
        {
            if self.rival_scout_site(ri, i) {
                self.rivals[ri].state = AiState::Build;
                return;
            }
        }
        // 2. Flee home hurt (sub_14310 :18480).
        if castle.is_some() && self.g.ent[i].act_life < (self.g.ent[i].max_life / 2) as i32 {
            self.set_rival_state(ri, AiState::Home, 0);
            return;
        }
        if !think {
            return;
        }
        // 3. Upgrade the castle (sub_14120 :18408).
        if let Some(c) = castle {
            let m16 = self.rivals[ri].owned[16] as usize;
            if m16 != 0
                && self.g.ent[m16].f26 == 0
                && self.rivals[ri].cooldown[16] == 0
                && self.g.ent[c].tick70 == 4
                && self.rivals[ri].mana_max
                    >= Gen::CASTLE_CAP[self.g.ent[c].f26.clamp(0, 7) as usize] as u32
                && self.g.castle_upgrade_space_ok(c)
            {
                self.set_rival_state(ri, AiState::Upgrade, 0);
                return;
            }
        }
        // 4. Raid an enemy castle (sub_143A0 :18496).
        if self.rival_has_offense(ri) && self.rival_pick_castle_target(ri, i) {
            self.rivals[ri].state = AiState::RaidCastle;
            return;
        }
        // 5. Attack an enemy wizard (sub_145B0 :18541).
        if self.rival_has_offense(ri) && self.rival_pick_wizard_target(ri, i) {
            self.rivals[ri].state = AiState::AttackWizard;
            return;
        }
        // 6. Intercept a fat enemy balloon (sub_147E0 :18596).
        if self.rival_pick_balloon_target(ri, i) {
            self.rivals[ri].state = AiState::RaidBalloon;
            return;
        }
        // 7. Claim mana balls (sub_14230 :18439-52): needs spell 3;
        // with the castle spell known, only while the ceiling is at
        // or under the castle spell's CURRENT cost — which
        // sub_47DD0 rewrites to the capacity ladder at the standing
        // castle's level, so claiming re-opens after every upgrade
        // (the original's economy loop).
        let castle_cost = castle
            .map(|c| Gen::CASTLE_CAP[self.g.ent[c].f26.clamp(0, 7) as usize] as u32)
            .unwrap_or(SPELLS[16].possess_mana);
        if self.rivals[ri].known[3]
            && (!self.rivals[ri].known[16] || self.rivals[ri].mana_max <= castle_cost)
            && self.rival_pick_ball_target(ri, i)
        {
            self.rivals[ri].state = AiState::Possess;
            return;
        }
        // 8. Hunt any mana holder (sub_14B10 :18650).
        if self.rival_pick_mana_target(ri, i) {
            self.rivals[ri].state = AiState::HuntMana;
            return;
        }
        // 9. Idle (sub_14DC0 :18749).
        if castle.is_some() && self.g.ent[i].act_life < self.g.ent[i].max_life as i32 {
            self.set_rival_state(ri, AiState::Home, 0);
        } else {
            self.rivals[ri].state = AiState::Cruise;
        }
    }

    /// Conformance import: reconstruct the retail AI lanes so the
    /// imported rival resumes mid-decision. The state handler runs
    /// BEFORE the selector (sub_13170 :17847), so state and a target
    /// that survives `target_alive` must arrive together — a Fresh
    /// import re-runs the cascade and re-aims f34 off retail's lock.
    /// Target and site ride the already-imported carpet entity (+146
    /// tr-translated by import_ent, +150/+152); the signature is
    /// recomputed, which reproduces retail's stored +148 exactly.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reanchor_rival_ai(
        &mut self,
        ri: usize,
        ai_state: u8,
        burst: i16,
        poverty: u16,
        cooldown: &[u16; SPELL_COUNT],
        learn: &[u16; SPELL_COUNT],
        hate: &[u16; 8],
        war: &[u16; 8],
        owned_slots: &[u16; SPELL_COUNT],
        life_rate: u16,
        regen_stall: u16,
        stored_sig: u16,
    ) {
        let e = &self.g.ent[self.rivals[ri].ent as usize];
        let target = e.f146;
        let site = (e.dest_x, e.dest_y);
        // The stored signature imports RAW (the carpet's +148, passed
        // through from the record) — recomputing it from the live
        // target would blind the staleness test (sub_15440 is
        // sig-vs-stored ONLY): retail freezes on a target whose
        // record changed since the pick, and a recomputed sig always
        // matches itself. The human target keeps the port's sentinel
        // convention.
        let sig = if target == PLAYER_TARGET {
            PLAYER_TARGET
        } else {
            stored_sig
        };
        let r = &mut self.rivals[ri];
        r.state = AiState::from_retail(ai_state);
        r.target = target;
        r.target_sig = sig;
        r.burst = burst;
        r.poverty = poverty != 0;
        r.cooldown = *cooldown;
        r.learn = *learn;
        r.site = site;
        r.hate = *hate;
        for (w, &v) in r.war.iter_mut().zip(war) {
            *w = v != 0;
        }
        // The book columns ride the record: +676 (owned manifestation
        // slots by spell id — retail rebuilds it every housekeeping
        // tick from the +532 acquisition list, sub_45C10 :55304, so
        // the settled value IS the rebuild's output). Without this the
        // record kept the FRESH world's spawn slots and every cast arm
        // (`owned[s]`) stamped a stranger's f26 — the l2 corpus put
        // Vodor's burst on the HUMAN's imported fireball token, 1254
        // rows. `known` follows: a nonzero slot is an owned spell.
        r.owned = *owned_slots;
        for (s, &m) in owned_slots.iter().enumerate() {
            if m != 0 {
                r.known[s] = true;
            }
        }
        // The regen lanes (:17990-18018): the applied-then-selected
        // life-rate register and the (AI-unread, but mirrored) stall.
        r.life_rate = life_rate as i32;
        r.regen_stall = regen_stall;
    }

    fn set_rival_state(&mut self, ri: usize, s: AiState, target: u16) {
        self.rivals[ri].state = s;
        self.rivals[ri].target = target;
        self.rivals[ri].target_sig = self.target_sig(target);
        // Every retail pick writes the wizard ENTITY's +146/+148
        // directly (sub_14B10 :18744-45 and its siblings) — the
        // corpus grades the column. Targetless transitions (Home,
        // Upgrade) leave the old value STALE, exactly like retail's
        // state setters, which touch only +415.
        if target != 0 {
            let ent = self.rivals[ri].ent as usize;
            self.g.ent[ent].f146 = target;
        }
    }

    /// Target signature (sub_15420 :19039): team + model + class<<7.
    fn target_sig(&self, target: u16) -> u16 {
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

    /// Target staleness (sub_15440 :19044): the SIGNATURE compare and
    /// NOTHING else — no life test, no free-flag test. A dying
    /// creature stays a valid target (retail chases the corpse); a
    /// FREED slot goes stale because the free clears class64 and the
    /// sig moves. The human carpet's sig survives death AND respawn —
    /// retail never drops the human by staleness.
    fn target_alive(&self, target: u16, sig: u16) -> bool {
        if target == 0 {
            return false;
        }
        if target == PLAYER_TARGET {
            return sig == PLAYER_TARGET;
        }
        self.target_sig(target) == sig
    }

    /// Castle-site scout (sub_13F00 :18358-402): walk the 4x4 grid of
    /// supercells starting at the wizard's OWN cell (inner x, outer y),
    /// testing two candidates per cell in order — the cell corner, then
    /// the cell mid (corner + 0x1F00). A candidate is accepted when the
    /// wizard has no foreign castle, or the one nearest it (toroidal
    /// squared-Euclidean, sub_15260/sub_42410) sits farther than 12288
    /// in CHEBYSHEV distance (max|dx|,|dy|; sub_42300). Retail returns
    /// the FIRST candidate that passes — NOT the one nearest the
    /// wizard. For a crater-bound wizard the first pass is the corner of
    /// its home supercell (on the surrounding rim), never the crater
    /// centre; picking the nearest instead planted dead-centre in the
    /// crater — more "deliberate"-looking than retail but wrong.
    fn rival_scout_site(&mut self, ri: usize, i: usize) -> bool {
        let me = self.rivals[ri].ent;
        let (sx, sy) = (self.g.ent[i].x, self.g.ent[i].y);
        for dy in 0..4u16 {
            let by = ((sy >> 14).wrapping_add(dy) & 3) << 14;
            for dx in 0..4u16 {
                let bx = ((sx >> 14).wrapping_add(dx) & 3) << 14;
                for (ox, oy) in [(0u16, 0u16), (0x1F00, 0x1F00)] {
                    let (tx, ty) = (bx.wrapping_add(ox), by.wrapping_add(oy));
                    // The foreign castle nearest this candidate by
                    // toroidal squared-Euclidean (sub_15260, keyed on
                    // model 2 and excluding our own).
                    let mut near_xy: Option<(u16, u16)> = None;
                    let mut near_d2 = i32::MAX;
                    for j in 1..self.g.ent.len() {
                        let e = &self.g.ent[j];
                        if e.class64 == 3 && e.model65 == 2 && e.flags & 0x400 == 0 && e.id24 != me
                        {
                            let d2 = Gen::dist2_sq(tx, ty, e.x, e.y);
                            if d2 < near_d2 {
                                near_d2 = d2;
                                near_xy = Some((e.x, e.y));
                            }
                        }
                    }
                    // Accept when clear, or the nearest castle's
                    // Chebyshev gap exceeds 12288 (sub_42300).
                    let ok = match near_xy {
                        None => true,
                        Some((cx, cy)) => {
                            let ddx = (tx.wrapping_sub(cx) as i16 as i32).abs();
                            let ddy = (ty.wrapping_sub(cy) as i16 as i32).abs();
                            ddx.max(ddy) > 12288
                        }
                    };
                    if ok {
                        self.rivals[ri].site = (tx, ty);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Any offense spell owned (sub_16920 :19856: {0,15,8,17,20,7}).
    pub(crate) fn rival_has_offense(&self, ri: usize) -> bool {
        [0usize, 15, 8, 17, 20, 7]
            .iter()
            .any(|&s| self.rivals[ri].owned[s] != 0)
    }

    /// The hate gate (:18514 etc.): hate[owner] over the wealth-
    /// scaled threshold.
    fn hate_over(&self, ri: usize, slot: u8, wealth: u32) -> bool {
        let r = &self.rivals[ri];
        let threshold = 50_000u32.saturating_sub(wealth / 10 * r.agg as u32 / 255);
        r.hate[slot as usize] as u32 > threshold
    }

    /// Enemy-castle pick (sub_143A0 :18496-536): hated-and-undefended
    /// or plain poorer, nearest in range.
    fn rival_pick_castle_target(&mut self, ri: usize, i: usize) -> bool {
        let me = self.rivals[ri].ent;
        let my_castle = self.rival_castle(me);
        let my_stored = my_castle.map_or(0, |c| self.g.ent[c].f140.max(0) as u32);
        // Skip while castle-less but castle-capable (:18507).
        if my_castle.is_none() && self.rivals[ri].known[16] {
            return false;
        }
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
            let owner_wealth = self.wizard_wealth(owner);
            let hated = self.hate_over(ri, owner, owner_wealth);
            // Undefended: the owner is over 7680 away (:18517-22).
            let undefended = self
                .wizard_pos(owner)
                .is_none_or(|(wx, wy, _)| Gen::dist2_sq(e.x, e.y, wx, wy) > 7680 * 7680);
            let poorer = (e.f140.max(0) as u32)
                .saturating_add(640 * (255 - self.rivals[ri].agg as u32))
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
            self.set_rival_state(ri, AiState::RaidCastle, t);
            true
        } else {
            false
        }
    }

    /// Enemy-wizard pick (sub_145B0 :18541-91).
    fn rival_pick_wizard_target(&mut self, ri: usize, i: usize) -> bool {
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32 + 10;
        let my_mana = self.rivals[ri].mana;
        let mut best: Option<(u16, i32)> = None;
        let consider = |slot: u8,
                        tgt: u16,
                        x: u16,
                        y: u16,
                        invisible: bool,
                        castle_less: bool,
                        wealth: u32,
                        mana: u32,
                        best: &mut Option<(u16, i32)>| {
            if invisible {
                return; // spell-12 targets are skipped (:18558)
            }
            let war = self.rivals[ri].war[slot as usize];
            let hated = self.hate_over(ri, slot, wealth);
            // Bully the homeless rich (:18570-77).
            let bully = castle_less
                && mana.saturating_add(32 * (255 - self.rivals[ri].agg as u32)) < my_mana;
            if !war && !hated && !bully {
                return;
            }
            let d = Gen::dist2_sq(px, py, x, y);
            if d <= range.saturating_mul(range) && best.is_none_or(|(_, bd)| d < bd) {
                *best = Some((tgt, d));
            }
        };
        // The human.
        if self.player.state == LifeState::Alive {
            let (hx, hy) = (self.human_pose.0, self.human_pose.1);
            consider(
                0,
                PLAYER_TARGET,
                hx,
                hy,
                self.player.invisible,
                self.player_castle().is_none(),
                self.player.mana_max,
                self.player.mana,
                &mut best,
            );
        }
        // Other rivals.
        for oj in 0..self.rivals.len() {
            if oj == ri || self.rivals[oj].eliminated {
                continue;
            }
            let o = &self.rivals[oj];
            let e = &self.g.ent[o.ent as usize];
            if e.tick70 != 1 {
                continue; // dead/falling wizards aren't targets
            }
            consider(
                o.slot,
                o.ent,
                e.x,
                e.y,
                o.invisible,
                self.rival_castle(o.ent).is_none(),
                o.mana_max,
                o.mana,
                &mut best,
            );
        }
        if let Some((t, _)) = best {
            self.set_rival_state(ri, AiState::AttackWizard, t);
            true
        } else {
            false
        }
    }

    /// Enemy-balloon pick (sub_147E0 :18596-645): hated owner, cargo
    /// over 10*(275-agg), away from its castle.
    fn rival_pick_balloon_target(&mut self, ri: usize, i: usize) -> bool {
        let me = self.rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32;
        let cargo_gate = 10 * (275 - self.rivals[ri].agg as u32);
        let mut best: Option<(u16, i32)> = None;
        for j in 1..self.g.ent.len() {
            let e = &self.g.ent[j];
            if e.class64 != 3 || e.model65 != 3 || e.flags & 0x400 != 0 || e.id24 == me {
                continue;
            }
            let Some(owner) = self.owner_slot(e.id24) else {
                continue;
            };
            if !self.hate_over(ri, owner, self.wizard_wealth(owner)) {
                continue;
            }
            if (e.f140.max(0) as u32) <= cargo_gate {
                continue;
            }
            // Not sitting at its own castle (:18628-33).
            let home = self
                .rival_castle(e.id24)
                .or_else(|| (owner == 0).then(|| self.player_castle()).flatten());
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
            self.set_rival_state(ri, AiState::RaidBalloon, t);
            true
        } else {
            false
        }
    }

    /// Mana-ball pick (sub_15080 :18862): wild balls by distance;
    /// at-war owners' balls; neutral-owned only if unguarded.
    fn rival_pick_ball_target(&mut self, ri: usize, i: usize) -> bool {
        let me = self.rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let mut best: Option<(u16, i32)> = None;
        for j in 1..self.g.ent.len() {
            let e = &self.g.ent[j];
            if e.class64 != 10 || e.model65 != 39 || e.flags & 0x400 != 0 {
                continue;
            }
            if e.f144 == me {
                continue; // already mine
            }
            let owner = self.owner_slot(e.f144);
            match owner {
                None => {} // wild: always eligible
                Some(o) => {
                    let at_war = self.rivals[ri].war[o as usize]
                        || self.hate_over(ri, o, self.wizard_wealth(o));
                    if !at_war {
                        // Neutral-owned: only if unguarded — no owner
                        // wizard within 5120 (:18905-16).
                        let guarded = self.wizard_pos(o).is_some_and(|(wx, wy, _)| {
                            Gen::dist2_sq(e.x, e.y, wx, wy) < 5120 * 5120
                        });
                        if guarded {
                            continue;
                        }
                    }
                }
            }
            let d = Gen::dist2_sq(px, py, e.x, e.y);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j as u16, d));
            }
        }
        if let Some((t, _)) = best {
            self.set_rival_state(ri, AiState::Possess, t);
            true
        } else {
            false
        }
    }

    /// Mana-holder hunt (sub_14B10 :18650): the nearest other-team CREATURE
    /// carrying mana (to the own castle, or self if castle-less), no range
    /// cap. Gated up-front on owning an offense spell (`sub_16920` :18662):
    /// a rival with nothing to attack with never enters HuntMana — else it
    /// shadows a mana creature and casts nothing.
    ///
    /// Retail walks the per-MODEL entity buckets `str_36382[+65]` for model
    /// indices 0..=19 (`i = 0; i != 80; i += 4`) — i.e. the living-creature
    /// models. Mana BALLS (model 39) and DWELLINGS (model 45) sit in higher
    /// buckets the loop never reaches, so the mana-hunt does NOT target them
    /// (balls are the Possess/ball-claim path's job, `rival_pick_ball_target`).
    /// The port keys this as `class64 == 5`, the faithful creature filter —
    /// a slight over-approximation of models 0..19 (a class-5 creature with
    /// model >= 20, e.g. the hydra m27, would be out of retail's scan) that
    /// is immaterial while such creatures carry no mana.
    pub(crate) fn rival_pick_mana_target(&mut self, ri: usize, i: usize) -> bool {
        // sub_16920 gate (:18662): no offense spell → no hunt.
        if !self.rival_has_offense(ri) {
            return false;
        }
        let me = self.rivals[ri].ent;
        let anchor = self
            .rival_castle(me)
            .map(|c| (self.g.ent[c].x, self.g.ent[c].y))
            .unwrap_or((self.g.ent[i].x, self.g.ent[i].y));
        let mut best: Option<(u16, i32)> = None;
        // Retail's walk (:18669-91) is the class-5 MODEL CHAINS
        // (heads at 36382 + 4·model, bucket-major) — the tick-top
        // membership snapshot, NOT the raw pool: a creature that died
        // after the rebuild is still visible, one that was dying AT
        // tick top never entered. Only +140 > 0 and the owner tag
        // filter at walk time (live fields off the members).
        for m in 0..self.g.mob_chains.list.len() {
            for jj in 0..self.g.mob_chains.visible(m).len() {
                let j = self.g.mob_chains.visible(m)[jj];
                let e = &self.g.ent[j as usize];
                if e.id24 == me || e.f140 <= 0 {
                    continue;
                }
                let d = Gen::dist2_sq(anchor.0, anchor.1, e.x, e.y);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j, d));
                }
            }
        }
        if let Some((t, _)) = best {
            self.set_rival_state(ri, AiState::HuntMana, t);
            true
        } else {
            false
        }
    }

    /// A wizard's live position by slot (0 = the human).
    pub(crate) fn wizard_pos(&self, slot: u8) -> Option<(u16, u16, i16)> {
        if slot == 0 {
            return (self.player.state == LifeState::Alive).then_some(self.human_pose);
        }
        let ent = self
            .rivals
            .iter()
            .find(|r| r.slot == slot)
            .map(|r| r.ent)
            .or_else(|| {
                self.mc2_rivals
                    .iter()
                    .find(|r| r.slot == slot)
                    .map(|r| r.ent)
            })?;
        let e = &self.g.ent[ent as usize];
        (e.tick70 == 1).then_some((e.x, e.y, e.z))
    }

    /// A wizard's mana ceiling (the wealth term in the hate gates).
    pub(crate) fn wizard_wealth(&self, slot: u8) -> u32 {
        if slot == 0 {
            return self.player.mana_max;
        }
        self.rivals
            .iter()
            .find(|r| r.slot == slot)
            .map(|r| r.mana_max)
            .or_else(|| {
                self.mc2_rivals
                    .iter()
                    .find(|r| r.slot == slot)
                    .map(|r| r.mana_max)
            })
            .unwrap_or(0)
    }

    // ---- state handlers -----------------------------------------------

    fn rival_state_tick(&mut self, ri: usize, i: usize, think: bool) {
        // Combat states drop stale targets back to Fresh.
        let needs_target = matches!(
            self.rivals[ri].state,
            AiState::Possess
                | AiState::RaidCastle
                | AiState::AttackWizard
                | AiState::RaidBalloon
                | AiState::HuntMana
        );
        if needs_target && !self.target_alive(self.rivals[ri].target, self.rivals[ri].target_sig) {
            self.rivals[ri].state = AiState::Fresh;
            self.rivals[ri].target = 0;
            return;
        }
        match self.rivals[ri].state {
            AiState::Fresh => {}
            // Fly home; cast 0x10 on arrival = the upgrade chain
            // (sub_13800 :18106-32).
            AiState::Upgrade => {
                let Some(c) = self.rival_castle(self.rivals[ri].ent) else {
                    self.rivals[ri].state = AiState::Fresh;
                    return;
                };
                let (cx, cy, cz) = {
                    let e = &self.g.ent[c];
                    (e.x, e.y, e.z)
                };
                if self.rival_approach(ri, i, cx, cy, Some(cz), 512, 2048) {
                    self.rival_hover_toward(i, cz.saturating_add(512));
                    self.rival_cast(ri, i, 16);
                }
            }
            // Fly to the scouted site; plant (sub_138F0 :18142-68).
            AiState::Build => {
                let (sx, sy) = self.rivals[ri].site;
                if self.rival_approach(ri, i, sx, sy, None, 2048, 3072) {
                    self.rival_cast(ri, i, 16);
                    if self.rival_castle(self.rivals[ri].ent).is_some() {
                        self.rivals[ri].state = AiState::Fresh;
                    }
                }
            }
            // Claim the ball (sub_13BA0 :18236-57): approach, cast 3,
            // and inside ~5 degrees write the claim directly.
            AiState::Possess => {
                let t = self.rivals[ri].target as usize;
                let (tx, ty, tz) = {
                    let e = &self.g.ent[t];
                    (e.x, e.y, e.z)
                };
                if self.rival_approach(ri, i, tx, ty, Some(tz), 1024, 3072) {
                    let cast = self.rival_cast(ri, i, 3);
                    let facing = Gen::angdist(
                        self.g.ent[i].f30,
                        Gen::angle_between(self.g.ent[i].x, self.g.ent[i].y, tx, ty),
                    );
                    if cast && facing <= 28 {
                        self.g.ent[t].f144 = self.rivals[ri].ent;
                        // Settled balls never re-run the tick's
                        // re-derive — recolor at the claim.
                        self.g.ball_resize(t);
                        self.g.snd(4, t); // the claim chime (:29444)
                        // The state STAYS Possess (:18250-56 writes
                        // no +415): the ball is now MINE, so the
                        // think-tick cascade re-picks past it (the
                        // ball pick's own-ball filter); until then
                        // the handler idles at the claimed sphere.
                    }
                    // The z-hover toward ball + 512 runs on EVERY
                    // arrived tick, cast or no cast (:18258-63).
                    self.rival_hover_toward(i, tz.saturating_add(512));
                }
            }
            // Castle raid (sub_13CA0 :18271-92): the cast attempt AND
            // the hover both live inside the arrived + think-period
            // gate; a fired cast tick does not hover.
            AiState::RaidCastle => {
                let t = self.rivals[ri].target as usize;
                let (tx, ty, tz) = {
                    let e = &self.g.ent[t];
                    (e.x, e.y, e.z)
                };
                self.rival_face_target(i, tx, ty, tz);
                if self.rival_approach(ri, i, tx, ty, Some(tz), 2048, 3584) && think {
                    let fired = match self.rival_attack_pick(ri, false) {
                        Some(s) => self.rival_cast(ri, i, s),
                        None => false,
                    };
                    if !fired {
                        self.rival_hover_toward(i, tz.saturating_add(512));
                    }
                }
            }
            // Wizard / balloon / mana-holder attack (sub_13DD0
            // :18314-40): the cast attempt runs ONLY when ARRIVED
            // (inside 3072) with the burst lockout clear — retail
            // returns before the pick otherwise (the l2 corpus wall:
            // the port fired every tick from 7300 units out while
            // retail held a saturated charge meter) — and the z-hover
            // toward target + 512 runs only when the attempt FAILED.
            AiState::AttackWizard | AiState::RaidBalloon | AiState::HuntMana => {
                let (tx, ty, tz) = match self.rivals[ri].target {
                    PLAYER_TARGET => self.human_pose,
                    t => {
                        let e = &self.g.ent[t as usize];
                        (e.x, e.y, e.z)
                    }
                };
                self.rival_face_target(i, tx, ty, tz);
                if self.rival_approach(ri, i, tx, ty, Some(tz), 3072, 4096)
                    && self.rivals[ri].burst >= 0
                {
                    let fired = match self.rival_attack_pick(ri, true) {
                        Some(s) => self.rival_cast(ri, i, s),
                        None => false,
                    };
                    if fired {
                        // Landing a cast clears MY war flag toward the
                        // struck WIZARD — carpet targets only, human
                        // or rival (:18337-39; the target's +65 <= 1
                        // gate).
                        let target = self.rivals[ri].target;
                        let is_carpet = target == PLAYER_TARGET
                            || self
                                .g
                                .ent
                                .get(target as usize)
                                .is_some_and(|e| e.class64 == 3 && e.model65 <= 1);
                        if is_carpet {
                            if let Some(o) = self.owner_slot(target) {
                                self.rivals[ri].war[o as usize] = false;
                            }
                        }
                    } else {
                        self.rival_hover_toward(i, tz.saturating_add(512));
                    }
                }
            }
            // Home (sub_13A70 :18204-27): cloak while fleeing; the
            // teleport-home attempt is authentically dead code.
            AiState::Home => {
                let Some(c) = self.rival_castle(self.rivals[ri].ent) else {
                    // Castle-less Home (:18209-19): cloak + the Cruise
                    // speed logic, and the state STAYS Home — the
                    // cascade is what moves it on.
                    self.rival_cast(ri, i, 12);
                    self.rival_cruise_speed(ri, i);
                    return;
                };
                let (cx, cy) = (self.g.ent[c].x, self.g.ent[c].y);
                self.rival_cast(ri, i, 12);
                let cz = self.g.ent[c].z;
                self.rival_approach(ri, i, cx, cy, Some(cz), 256, 2048);
                if self.g.ent[i].act_life >= self.g.ent[i].max_life as i32 {
                    self.rivals[ri].state = AiState::Fresh;
                }
            }
            // Cruise (sub_13A10 :18188).
            AiState::Cruise => {
                self.rival_cruise_speed(ri, i);
            }
        }
    }

    /// The Cruise speed logic (sub_13A10 :18188-203, shared by the
    /// castle-less Home arm): an ACTIVE speed burst owns the speed
    /// columns (sub_15E60's +48 test — vdes untouched); else the AI
    /// chain-casts the speed-up whenever ready, else full throttle.
    fn rival_cruise_speed(&mut self, ri: usize, i: usize) {
        if self
            .rival_token(ri, 2)
            .is_some_and(|m| self.g.ent[m].f26 > 0)
        {
            return;
        }
        if self.rival_cast_ready(ri, 2) {
            self.rival_cast(ri, i, 2);
        } else {
            self.rivals[ri].vdes = self.g.ent[i].f128;
        }
    }

    /// Shared travel helper (sub_15470 :19050-94): inside arriveR →
    /// stop, done. The distance is FULL 3D against an entity target
    /// (sub_42340: isqrt(dx²+dy²+dz²) — a wizard hovering high above
    /// a ground creature is NOT arrived; the 2-D read was the l2
    /// machine-gun wall's second half) and 2-D against a bare SITE
    /// (the a2==0 branch's sub_423D0 on +150). Beyond it, an ACTIVE
    /// speed burst owns the speed columns (:19063 sub_15E60 — return
    /// with vdes UNTOUCHED, the token machine is driving); a
    /// boost-cast tick returns the same way; only the plain leg
    /// writes vdes = f128. Returns "arrived". (Retail's callers stamp
    /// +34 themselves; the fold here matches every live call site.)
    #[allow(clippy::too_many_arguments)]
    fn rival_approach(
        &mut self,
        ri: usize,
        i: usize,
        tx: u16,
        ty: u16,
        tz: Option<i16>,
        arrive: i32,
        boost: i32,
    ) -> bool {
        let (px, py, pz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        // Retail compares the TRUNCATED scalar distance, never the
        // square: sub_15470 tests `sub_42340(...) > a3` (:19058-62)
        // and `> a4` (:19066), and both helpers end in the isqrt
        // (:52724 / :52744 — their squared-only twins sub_42390 /
        // sub_42410 exist and are deliberately NOT the ones called
        // here). The two forms differ across the whole band
        // arrive² < d² < (arrive+1)², where the square test refuses
        // but the isqrt truncates onto the boundary and ARRIVES —
        // mc1l2 t=1824/1895: Vodor at d² = 9,437,778 against
        // 3072² = 9,437,184 is 594 over on squares, exactly 3072 on
        // the isqrt, and retail casts.
        let d = {
            let dh = Gen::dist2_sq(px, py, tx, ty);
            let sum = match tz {
                Some(z) => {
                    let dz = z.wrapping_sub(pz) as i32;
                    dh.wrapping_add(dz.wrapping_mul(dz))
                }
                None => dh,
            };
            Gen::isqrt(sum as u32) as i32
        };
        self.g.ent[i].f34 = Gen::angle_between(px, py, tx, ty);
        if d <= arrive {
            self.rivals[ri].vdes = 0;
            return true;
        }
        if self
            .rival_token(ri, 2)
            .is_some_and(|m| self.g.ent[m].f26 > 0)
        {
            return false;
        }
        if d > boost && self.rival_cast_ready(ri, 2) {
            self.rival_cast(ri, i, 2);
            return false;
        }
        self.rivals[ri].vdes = self.g.ent[i].f128;
        false
    }

    /// Aim the body at the target (desired yaw; the commit pitch is
    /// set at cast time, :19125-27).
    fn rival_face_target(&mut self, i: usize, tx: u16, ty: u16, _tz: i16) {
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        self.g.ent[i].f34 = Gen::angle_between(px, py, tx, ty);
    }

    /// Per-state altitude nudge toward target z + 512: `z +=
    /// sign(z − tz) · row.v_14` verbatim (:18258-63 / :18328-32 /
    /// :18287-91) — v_14 is the NEGATIVE settle step, so above sinks
    /// and below climbs; a zero row steps nothing.
    fn rival_hover_toward(&mut self, i: usize, tz: i16) {
        let v14 = BEHAVIOR[self.g.ent[i].row156 as usize].v_14;
        let e = &mut self.g.ent[i];
        let d = e.z as i32 - tz as i32;
        e.z = (e.z as i32 + d.signum() * v14 as i32) as i16;
    }

    /// The attack-spell picker (sub_16030 :19459 / castle variant
    /// sub_16310 :19559): poverty latch, then the priority walk
    /// 17 → 8 → (anti-rebound 15) → 7 → 20 → 0 → 15. Returns the
    /// spell to cast now; None = hold (save up or poor).
    pub(crate) fn rival_attack_pick(&mut self, ri: usize, vs_wizard: bool) -> Option<usize> {
        // Poverty latch (:19468-91): latch under max/4; release the
        // tick mana REACHES the threshold (min(max/4 + 6000, max/2)
        // — retail's `>` tests are on the still-poor side, so the
        // boundary itself releases; the port's old strict `>` held
        // one extra tick, which under the +100/tick floor pushed
        // every early-Vodor fireball one tick late).
        {
            let r = &mut self.rivals[ri];
            let quarter = r.mana_max / 4;
            if r.mana < quarter {
                r.poverty = true;
            } else if r.poverty {
                let v3 = quarter + 6000;
                let still_poor = if v3 >= r.mana_max {
                    r.mana_max / 2 > r.mana
                } else {
                    v3 > r.mana
                };
                if !still_poor {
                    r.poverty = false;
                }
            }
            if r.poverty {
                return None;
            }
        }
        // Anti-rebound notice (:19507-16): the target visibly
        // rebounding switches the plan to lightning, acc% of the
        // time — and that success path ENDS the walk: 15-when-ready
        // or hold, never falling through to 7/20/0 (:19517-31; the
        // 7/20/0/15 ladder is the roll's ELSE arm).
        let target_rebounds = vs_wizard
            && match self.rivals[ri].target {
                PLAYER_TARGET => self.player.rebound,
                t => self
                    .rivals
                    .iter()
                    .find(|r| r.ent == t)
                    .is_some_and(|r| r.rebound),
            };
        let mut order: Vec<usize> = vec![17, 8];
        let mut lightning_plan = false;
        if target_rebounds {
            let roll = (self.g.ent_rand(self.rivals[ri].ent as usize) % 255) as u16;
            if roll < self.rivals[ri].acc {
                lightning_plan = true;
            }
        }
        if lightning_plan {
            order.push(15);
        } else {
            order.extend([7, 20, 0, 15]);
        }
        for s in order {
            if self.rivals[ri].owned[s] == 0 {
                continue;
            }
            if self.rival_cast_ready(ri, s) {
                return Some(s);
            }
            if lightning_plan && s == 15 {
                return None; // the plan holds for the bolt (:19525-29)
            }
            // WAIT-vs-continue discriminant (sub_15E90 :19497): fall
            // through to the next spell when this one is unaffordable by
            // ceiling OR on its recast cooldown; only HOLD (save mana /
            // settle the aim) when it's affordable-by-ceiling, off
            // cooldown, and merely short on current mana or unaimed.
            // (The cooldown escape is what lets a just-fired — or
            // castle-fizzled — high-priority spell yield to a cheaper
            // castle-free one like Fireball while it recharges.)
            if self.rivals[ri].mana_max < self.spells()[s].possess_mana
                || self.rivals[ri].cooldown[s] != 0
            {
                continue;
            }
            return None;
        }
        None
    }

    // ---- the cast arm (readiness sub_15A00 :19219 + executor
    // ---- sub_155F0 :19096) ------------------------------------------

    /// Readiness (`sub_15A00` :19219): owned, not busy, cooldown clear,
    /// CURRENT mana covers the cost, and (for the aimed groups) the
    /// accuracy-scaled aim cone. The castle-stored unlock ladder is
    /// deliberately NOT here — retail's readiness has no castle term
    /// (verified in `sub_15A00`); the ladder is enforced downstream at
    /// emission ([`World::rival_cast`], mirroring retail's projectile-tick
    /// fizzle `sub_55DD0` :65049). Folding it into readiness froze rivals: a
    /// castle-tier spell they own but can't unlock (no big castle) reads as
    /// affordable-by-ceiling forever, so the picker parked on it and never
    /// fell through to Fireball.
    fn rival_cast_ready(&self, ri: usize, s: usize) -> bool {
        let r = &self.rivals[ri];
        let m = r.owned[s] as usize;
        if m == 0 {
            return false;
        }
        // The recast cooldown gates every case EXCEPT Accelerate —
        // sub_15A00's case 2 tests token + mana only (:19260-63);
        // its cadence comes from the burst window (the commit's +48
        // test), and the armed-but-unread AI_RECAST[2]=32 would have
        // starved retail's chain-cast Cruise boosts.
        if s != 2 && r.cooldown[s] != 0 {
            return false;
        }
        let def = &self.spells()[s];
        // Spell 16 prices through the LIVE manifestation cache
        // (sub_15A00 case 0x10 :19332 reads the token's +136, same
        // stamp as the want gate) — 1000 ctor, CAP[lvl] housed, 5000
        // after a raze. Every other spell is the static table cost.
        let cost = if s == 16 {
            self.rival_castle_price(ri)
        } else {
            def.possess_mana
        };
        if r.mana < cost {
            return false;
        }
        // ALREADY-ACTIVE gate: a token still carrying burst (+48, our
        // f26) is NOT ready. Retail runs it for the self-buff group
        // (case 4/0xC/0xE :19289-96), for the AIMED group (case
        // 3/7/8/0x11/0x14 :19265-68) and Castle (:19305) — but NOT
        // for the fireball group (case 0/0xB/0xD/0xF), whose bolts
        // re-arm mid-burst freely. Accelerate's lives in the COMMIT
        // (sub_155F0 case 2 :19151), mirrored in `rival_cast`.
        if matches!(s, 3 | 4 | 7 | 8 | 12 | 14 | 17 | 20)
            && self
                .rival_token(ri, s)
                .is_some_and(|m| self.g.ent[m].f26 > 0)
        {
            return false;
        }
        // Aimed groups: the readiness pre-gate cone ((255-acc)/4+20
        // degrees, :19252-57) — between the ACTUAL heading and the
        // DESIRED one (+30 vs +34, the state handler's stamp), not a
        // recomputed target bearing, and `>=` refuses.
        if matches!(s, 0 | 3 | 7 | 8 | 11 | 13 | 15 | 17 | 20) {
            let cone = ((255 - r.acc as u32) / 4 + 20) * 2048 / 360;
            let e = &self.g.ent[r.ent as usize];
            if Gen::angdist(e.f30, e.f34 & 0x7FF) as u32 >= cone {
                return false;
            }
        }
        true
    }

    /// The commit (sub_155F0 :19096-215): arm the cooldown, aim the
    /// pitch at the target, run the burst counter, debit the mana
    /// through the delta, and emit through the shared spawners.
    /// Returns true when the cast fired. Spells 18/19/21/22/23 hit
    /// the original's default case — the AI can never cast them.
    fn rival_cast(&mut self, ri: usize, i: usize, s: usize) -> bool {
        if s >= SPELL_COUNT || matches!(s, 18 | 19 | 21 | 22 | 23) {
            return false;
        }
        if !self.rival_cast_ready(ri, s) {
            return false;
        }
        let def = &self.spells()[s];
        // Group gates beyond readiness (:19113-19209).
        let (tx, ty, tz) = match self.rivals[ri].target {
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
        let (ex, ey, ez, yaw, des) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f30, e.f34)
        };
        // The commit clears the entity's 0x100 bit whenever readiness
        // passed (:19110, `+17 &= ~1`), before any case gate.
        self.g.ent[i].flags &= !0x100;
        // Commit cones compare the ACTUAL heading against the DESIRED
        // one (+30 vs +34), `>=` refusing — not a recomputed target
        // bearing (:19120-23 / :19163-66).
        match s {
            // Precision-aimed burst pair (:19113-37).
            0 | 15 => {
                if self.rivals[ri].burst < 0 || Gen::angdist(yaw, des) >= 0xAA {
                    return false;
                }
                self.rivals[ri].burst += 1;
                if self.rivals[ri].burst >= 8 {
                    // Negative lockout (:19129-36).
                    self.rivals[ri].burst = ((self.rivals[ri].tempo as i32 - 255) / 8 - 1) as i16;
                }
            }
            // Aimed group (:19158-77): the wider cone.
            3 | 7 | 8 | 11 | 13 | 17 | 20 if Gen::angdist(yaw, des) >= 0xE3 => {
                return false;
            }
            // Accelerate (:19151): the busy gate lives HERE, not in
            // readiness — a live burst refuses the re-commit.
            2 if self
                .rival_token(ri, 2)
                .is_some_and(|m| self.g.ent[m].f26 > 0) =>
            {
                return false;
            }
            _ => {}
        }
        // Castle (0x10): with a castle → the upgrade chain; without →
        // the free direct plant at the site (:19190-209).
        if s == 16 {
            return self.rival_cast_castle(ri, i);
        }
        // Arm the re-attempt cooldown FIRST — retail's sub_155F0 sets it
        // regardless of the castle outcome, and the picker's cooldown
        // escape (sub_15E90 :19497) relies on it to advance past this
        // spell next tick.
        self.rivals[ri].cooldown[s] = AI_RECAST[s];
        // Castle-stored unlock ladder (sub_55DD0 :64917-19), enforced HERE
        // at emission — retail arms the projectile in sub_155F0, then
        // fizzles it on its first tick (:65049) when the caster owns no
        // castle storing >= castle_req. We collapse that: the cooldown is
        // already consumed above, so bail before arming the manifestation,
        // debiting, or emitting — a castle-gated spell the rival hasn't
        // unlocked does no damage and grants no buff, but no longer freezes
        // the picker on an eternal WAIT. Silent for rivals (retail's buzz 29
        // would storm at Lightning's 1-tick recast); the human keeps its UI
        // buzz. Returns true so the caller's post-cast bookkeeping (war-flag
        // clear) matches retail's sub_155F0 "success".
        if def.castle_req > 0
            && !self
                .rival_castle(self.rivals[ri].ent)
                .is_some_and(|c| self.g.ent[c].f140.max(0) as u32 >= def.castle_req)
        {
            return true;
        }
        // The commit only ARMS the token (+48 = +50, through the
        // VALIDATED binding) — the bolt, the sub_55E80 debit and the
        // mid-burst regen freeze all run at the TOKEN's own pool slot
        // ([`World::rival_manifestation_tick`], retail's sub_56090
        // machine). A token below the caster fires next pass, above
        // it the same tick — retail's phase for free.
        if let Some(m) = self.rival_token(ri, s) {
            self.g.ent[m].f26 = def.count as i16;
        }
        // Absolute aim pitch to the target (:19125-27 / :19168-71):
        // the commit stamps the WIZARD's own +32 — the token-side
        // spawner reads the pose (and the corpus grades the column).
        let dh = Gen::isqrt(Gen::dist2_sq(ex, ey, tx, ty) as u32) as i32;
        let pitch = Gen::pitch_toward(ez, tz, dh);
        if matches!(s, 0 | 3 | 7 | 8 | 11 | 13 | 15 | 17 | 20) {
            self.g.ent[i].f32 = pitch;
        }
        let _ = (ex, ey, ez, yaw);
        true
    }

    /// Rival castle cast (:19190-209).
    fn rival_cast_castle(&mut self, ri: usize, i: usize) -> bool {
        self.rivals[ri].cooldown[16] = AI_RECAST[16];
        if let Some(c) = self.rival_castle(self.rivals[ri].ent) {
            // The upgrade: full next-stage cost through the shared
            // ladder, token chain approximated by the direct level-up
            // (the (9,10)→(10,43) ball ride is cosmetic here; the
            // painter is the same).
            let lvl = self.g.ent[c].f26.clamp(0, 7) as usize;
            let cost = Gen::CASTLE_CAP[lvl] as u32;
            if self.rivals[ri].mana < cost || !self.g.castle_upgrade_space_ok(c) {
                return false;
            }
            {
                let r = &mut self.rivals[ri];
                let ci = cost.min(i32::MAX as u32) as i32;
                r.mana_delta = if r.mana_delta >= 0 {
                    -ci
                } else {
                    r.mana_delta - ci
                };
            }
            // The token delivery (sub_293D0 :31033-34): ch5 mail
            // {10, owner} — the castle tick's case-0 runs the whole
            // level-up (preclear, ladder, painter). The (9,10) ball
            // ride is skipped (cosmetic; APPROX).
            self.g.ent[c].mail[5] = (10, self.rivals[ri].ent);
            return true;
        }
        // Castle-less: the FREE direct plant at the scouted site
        // (:19200-08) — no debit, no projectile.
        let (sx, sy) = self.rivals[ri].site;
        if sx == 0 && sy == 0 {
            return false;
        }
        let gz = self.g.ground_z(sx, sy) as i16;
        let Some(c) = self.g.spawn_class3(2, sx, sy, gz) else {
            return false;
        };
        {
            let e = &mut self.g.ent[c];
            e.id24 = self.rivals[ri].ent;
            e.f26 = 0;
            e.tick70 = 4;
        }
        self.g.set_sprite(c, 177 + self.rivals[ri].slot as u16);
        let (cx, cy, cz) = {
            let e = &self.g.ent[c];
            (
                ((e.x as u32 + 128) >> 8) as u8,
                ((e.y as u32 + 128) >> 8) as u8,
                (e.z >> 5) as i32,
            )
        };
        self.g.stamp_castle_terrain(1, cx, cy, cz);
        self.g.castle_extents(c, 0);
        self.g.ent[c].f136 = Gen::CASTLE_CAP[0];
        self.g.snd(30, c);
        self.terrain_dirty = true;
        self.entities_dirty = true;
        let _ = i;
        true
    }

    /// The per-spell emissions through the shared spawners, owner =
    /// the rival's entity slot (so friendly-fire immunity, homing and
    /// the damage plumbing all Just Work).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rival_emit(
        &mut self,
        ri: usize,
        i: usize,
        s: usize,
        x: u16,
        y: u16,
        z: i16,
        yaw: u16,
        pitch: u16,
    ) {
        let owner = self.rivals[ri].ent;
        let target = self.rivals[ri].target;
        let def = &self.spells()[s];
        let speed = self.g.ent[i].f126;
        let snd = match s {
            0 | 11 | 13 | 17 | 20 => Some(9u8),
            7 | 8 => Some(15),
            2 => Some(19),
            15 => Some(23),
            1 => Some(25),
            3 => Some(40),
            _ => None,
        };
        if let Some(id) = snd {
            self.g.snd(id, i);
        }
        let pr = match s {
            0 => self.g.spawn_fireball(x, y, z),
            3 => self.g.spawn_spell_lob(1, x, y, z),
            7 => self.g.spawn_trail_bolt(x, y, z),
            8 => self.g.spawn_spell_lob(4, x, y, z),
            11 => self.g.spawn_spell_lob(7, x, y, z),
            13 => self.g.spawn_seeker(x, y, z),
            15 => self.g.spawn_zigzag(x, y, z),
            17 => self.g.spawn_spell_lob(11, x, y, z),
            20 => self.g.spawn_spell_lob(9, x, y, z),
            // Self-buffs/channels have no projectile.
            _ => None,
        };
        let Some(pr) = pr else { return };
        let e = &mut self.g.ent[pr];
        e.f126 += speed;
        e.f128 = e.f126;
        e.id24 = owner;
        e.f30 = yaw;
        e.f32 = pitch;
        e.f36 = pitch;
        e.f44 = def.damage.min(u16::MAX as u32) as u16;
        // +140 carries the per-burst-tick debit quantum (cost/count —
        // the token ctor's stamp, corpus: fireball token 200/5 = 40 on
        // the bolt), not the full one-shot cost.
        e.f140 = (def.possess_mana / (def.count as u32).max(1)) as i32;
        // NO +34 write and NO +146 pre-lock — retail's emission
        // (sub_56510 :65233-52) leaves the bolt's desired-yaw at the
        // ctor 0 and never writes the target: the bolt's own one-shot
        // muzzle acquisition (sub_54520 case 1, next tick — the bolt's
        // pool slot already ran this pass) picks the victim, for the
        // AI exactly as for the human. Pre-locking bypassed that scan
        // (no accidental house possession, no natural misses) and
        // faked a spawn-tick +34/+146 the corpus reads as 0. The
        // danger music arms at ACQUISITION (the class-9 machinery),
        // not at emission.
        let _ = target;
        match s {
            3 => {
                let e = &mut self.g.ent[pr];
                e.f68 = 10;
                e.f69 = 12;
                e.f66 = 10;
                e.f26 = 200;
                e.f80 *= 2;
                e.f82 *= 2;
                e.f84 *= 2;
            }
            7 => self.g.ent[pr].f69 = 17,
            13 => {
                let e = &mut self.g.ent[pr];
                e.f44 = 2000;
                e.f69 = 25;
            }
            15 => self.g.ent[pr].f69 = 23,
            _ => {}
        }
        // The charge move — the AI's manifestations run the SAME
        // class-12 spawners as the human's: fireball (:65072-73),
        // meteor (:65414) and volcano (:65472) bank the rival's
        // u8_326 meter in the bolt's +26; possess zeroes without
        // stamping (:65246). The other rival arms never touch +326.
        let ws = self.rivals[ri].slot as usize;
        match s {
            0 | 7 | 8 => {
                self.g.ent[pr].f26 = self.wiz_charge[ws] as i16;
                self.wiz_charge[ws] = 0;
            }
            3 => self.wiz_charge[ws] = 0,
            _ => {}
        }
        self.entities_dirty = true;
    }

    // ---- mortality ------------------------------------------------------

    /// State 2 — the death fall (sub_45FC0 :55434-90): the shared
    /// death-drift mover first (sub_455D0 — stick lanes are zero for
    /// the AI, but the speed still chases the stale vdes 16/tick and
    /// the body drifts level), THEN gravity `z += OLD f46` with the
    /// decrement clamped into [−256, 0] after the add, the floor at
    /// ground + row.v_12, a (10,1) trail puff at the POST-drift
    /// PRE-gravity pose (flags |= 0x80 only, id24 = the faller), and
    /// the impact block exactly when z LANDED ON the floor.
    fn rival_death_fall(&mut self, ri: usize, i: usize) {
        {
            let vdes = self.rivals[ri].vdes;
            let e = &mut self.g.ent[i];
            e.f126 += 16 * (vdes - e.f126).signum();
        }
        let (yaw, speed, vz) = {
            let e = &self.g.ent[i];
            (e.f30, e.f126, e.f46)
        };
        let mut pos = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        Gen::polar_step(&mut pos, yaw, 0, speed);
        // The knock lane of the same shared mover (:55204-19). The AI's
        // live mover never runs it, so the killing blow's whole impulse
        // is still pending when the corpse enters state 2: retail's
        // body drifts along that bearing for ~10 ticks at 4/tick decay
        // (~180 units) — off whatever lip it was hovering over — and
        // only then meets the floor. Without it the port's corpse never
        // moved, so the very first fall tick clamped it onto the floor
        // and fired the impact 18 ticks early, sliding every later
        // allocation that tick down the free stack.
        // ⚠ The upper clamp is retail's (:55207-08); it has NO lower
        // clamp, so a negative magnitude would decay by +4 forever —
        // unreachable from the [0, 80] arm, and left as retail has it.
        {
            let r = &mut self.rivals[ri];
            if r.knock_mag != 0 {
                let mag = r.knock_mag.min(128);
                Gen::polar_step(&mut pos, r.knock_dir, 0, mag);
                let mut next = mag - mag.signum() * 4; // dword_93A94 = 4
                if next.abs() < 4 {
                    next = 0;
                }
                r.knock_mag = next;
            }
        }
        // sub_455D0 :55158-60 stamps the body's +32 from the control
        // block's u16_329 (HIBYTE &= 7), which is 0 for an AI.
        self.g.ent[i].f32 = 0;
        let puff = pos;
        pos.2 = pos.2.saturating_add(vz);
        {
            let e = &mut self.g.ent[i];
            e.f46 = (vz - 2).clamp(-256, 0);
        }
        let floor = {
            let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
            (self.g.ground_z(pos.0, pos.1) as i16).saturating_add(row.v_12)
        };
        if pos.2 < floor {
            pos.2 = floor;
        }
        // The trail (10,1) burning puff (:55480-84).
        if let Some(s) = self.g.spawn_effect(1, puff.0, puff.1, puff.2) {
            self.g.ent[s].flags |= 0x80;
            self.g.ent[s].id24 = self.rivals[ri].ent;
        }
        self.g.move_relink(i, pos.0, pos.1, pos.2);
        if pos.2 == floor {
            self.rival_death_impact(ri, i);
        }
        self.entities_dirty = true;
    }

    /// The impact block (:55488-568): kill credit, jar scatter, the
    /// grave, in-flight balls re-pointed, entity hidden, respawn
    /// timer armed.
    fn rival_death_impact(&mut self, ri: usize, i: usize) {
        // :55487 — the touchdown REBUILDS THE FREE LIST first, so the
        // grave and every jar it throws come off a freshly sorted
        // stack. The class-3 fall handler `sub_45FC0` is SHARED with
        // the human, so this is the same line `World::player_land`
        // carries; mc1l2 t=8297→8298 catches it on the rival side
        // (the record's free stack turns into the descending
        // 33, 32, 31 … 19 run with a 79-deep recycle stack, and the
        // next (10,40) lands on slot 18 against our 65).
        let pinned = self.mc1_carpet_slot;
        self.g.mc1_rebuild_free(pinned);
        // Kill credit (:55488-97): the killer wizard's tally.
        let killer = self.g.ent[i].f38;
        if let Some(k) = self.owner_slot_of_source(killer) {
            self.kill_tally[k as usize][self.rivals[ri].slot as usize] += 1;
            // A rival kill feeds the human's counter too (parity
            // with the creature kill counter track).
            if k == 0 {
                self.g.kills = self.g.kills.saturating_add(1);
            }
        }
        // Death message for the app ticker + toast (retail etext 54
        // = "has died." rendered "<Name> has died.", periods=100 —
        // :55499-517 + the drawType-0 sprintf :26518-33; NOT etext 56
        // "is dead", the wrong neighbor).
        let slot = self.rivals[ri].slot;
        self.rival_deaths.push(slot);
        let name = RIVAL_NAMES.get(slot as usize).copied().unwrap_or("?");
        self.set_notification(format!("{name} has died."), 100, [0xFF, 0, 0]);
        // JAR SCATTER (:55519-49): every owned manifestation detaches
        // into a decaying ground jar around the corpse.
        // The scatter anchors on the corpse's own +76 (:55537 copies
        // `*(WORD *)(a1 + 76)` into the position struct), i.e. the z
        // the fall just clamped onto the floor — not a fresh ground
        // sample. (Both readings coincide in the mc1l2 window, so this
        // is decompile authority, not a corpus-proven claim.)
        let (cx, cy, cz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        for s in 0..SPELL_COUNT {
            let m = self.rivals[ri].owned[s] as usize;
            self.rivals[ri].owned[s] = 0;
            if m == 0 {
                continue;
            }
            // The scatter draws ride the DYING WIZARD's own LCG
            // (:55563-70 — `a1+4`, three draws per jar), not the
            // jar's.
            let dx = (self.g.ent_rand(i) & 0x1FF) as i32 - 256;
            let dy = (self.g.ent_rand(i) & 0x1FF) as i32 - 256;
            let jx = (cx as i32 + dx) as u16;
            let jy = (cy as i32 + dy) as u16;
            let life = (self.g.ent_rand(i) % 90 + 200) as i16;
            {
                let e = &mut self.g.ent[m];
                // :55529-31 — the token is un-parked: flags bit 0
                // clears and the state byte INCREMENTS (`++*(v16+70)`,
                // :55535). Assigning the phase outright happened to
                // agree on mc1l2 only because both tokens sat at
                // phase 0.
                e.flags &= !1;
                // Strict-retail worlds (a conformance import) carry
                // RETAIL's class-12 encoding — a scattered jar is
                // spell*3 + 1 (a phase-1 world jar the strict pickup
                // poll serves), and its decay rides ACT_LIFE (the jar
                // tick's top, sub_55A40 :64755-61: nonzero counts
                // down, freed at zero; authored jars carry 0 and sit
                // forever). The native encoding keeps its own f26
                // countdown.
                if self.strict_retail {
                    e.tick70 = e.tick70.wrapping_add(1);
                    e.act_life = life as i32;
                    e.f26 = 0;
                } else {
                    e.tick70 = crate::engine::world::DROPPED_JAR; // pickup-able, decaying
                    e.f26 = life; // the decay countdown
                }
                e.f144 = 0; // no owner — a free copy
            }
            // MOVE_RELINK (:55546 `sub_41C70_41FB0`), not the bare
            // link: `Gen::link` early-returns on flags bit 2, which an
            // imported parked token carries, so the scattered position
            // was silently never written and every jar stayed parked.
            self.g.move_relink(m, jx, jy, cz);
        }
        // The grave (10,40) + in-flight ball re-point (:55550-65).
        // The spawn axis is the CORPSE'S OWN `+72` (:55550 passes
        // `a1 + 72` straight into the creator), i.e. the z the death
        // fall clamped onto the ground+128 floor — not a fresh ground
        // sample. mc1l2 t=8298 reads the rival's grave at 1198 with
        // the ground under it at 1070, exactly one floor clearance
        // apart. (The human's own landing already spawns at
        // `player.z`, `World::player_land`.)
        if let Some(gv) = self.g.spawn_grave(cx, cy, cz) {
            let me = self.rivals[ri].ent;
            for j in 1..self.g.ent.len() {
                let e = &self.g.ent[j];
                if e.class64 == 10 && e.model65 == 39 && e.flags & 0x400 == 0 && e.f144 == me {
                    self.g.ent[j].f144 = gv as u16;
                    // Settled balls never re-run the tick's re-derive
                    // — the grave owner reads neutral in place.
                    self.g.ball_resize(j);
                }
            }
        }
        // Hidden (flag 0x20, :55568) + state 3 + the respawn timer
        // (:55552-57): 32*((255-tempo)/8)+32 ticks.
        {
            let e = &mut self.g.ent[i];
            e.tick70 = 3;
            // :55568 is a bare `|= 0x20` — the hittable bit 3 is NOT
            // cleared here (retail's corpse goes 12 → 44, keeping it).
            e.flags |= 0x20;
            e.f26 = (32 * ((255 - self.rivals[ri].tempo as i32) / 8) + 32) as i16;
        }
        // Post-death truce: everyone's hate toward this slot decays
        // from the elevated baseline once it respawns (:55037-41 —
        // set at re-init).
        self.entities_dirty = true;
    }

    /// State 3 — dead on the ground (sub_46480 :55594): with a
    /// castle, count down and re-init; castle-less = ELIMINATED
    /// (checked every tick — losing the castle during the wait
    /// counts, :55622).
    fn rival_dead_wait(&mut self, ri: usize, i: usize) {
        if self.rival_castle(self.rivals[ri].ent).is_none() {
            // The FINAL-death broadcast (retail etext 62 via the
            // opcode-0x1D elimination arm, :48812-25: "<Name> has
            // been eliminated from the realm.", periods=100 — MC1
            // says "eliminated" where MC2 says "banished"). Once, on
            // the elimination edge.
            if !self.rivals[ri].eliminated {
                let slot = self.rivals[ri].slot;
                let name = RIVAL_NAMES.get(slot as usize).copied().unwrap_or("?");
                self.set_notification(
                    format!("{name} has been eliminated from the realm."),
                    100,
                    [0xFF, 0, 0],
                );
            }
            self.rivals[ri].eliminated = true;
            // The husk stays hidden; property persists (:55622).
            return;
        }
        if self.g.ent[i].f26 > 0 {
            self.g.ent[i].f26 -= 1;
            return;
        }
        self.rival_respawn(ri, i);
    }

    /// Re-init at the castle (sub_44D30 respawn arm :54857-64 +
    /// :55019-50): teleport to the castle, full life, base mana,
    /// grace 100, re-mint the remembered book, brain reset, truce.
    fn rival_respawn(&mut self, ri: usize, i: usize) {
        // :54842 — `sub_44D30`'s first statement, whoever is
        // respawning: the same rebuild, so the re-minted book takes
        // the slots the scatter freed.
        let pinned = self.mc1_carpet_slot;
        self.g.mc1_rebuild_free(pinned);
        let Some(c) = self.rival_castle(self.rivals[ri].ent) else {
            return;
        };
        let (cx, cy) = (self.g.ent[c].x, self.g.ent[c].y);
        let z = (self.g.ground_z(cx, cy) as i16).saturating_add(256);
        {
            let e = &mut self.g.ent[i];
            e.flags = (e.flags & !0x20) | 8;
            e.tick70 = 1;
            e.f46 = 0;
            e.f126 = 0;
        }
        self.g.move_relink(i, cx, cy, z);
        self.g.refill_life(i);
        let ent = self.rivals[ri].ent;
        let known = self.rivals[ri].known;
        for (s, &k) in known.iter().enumerate() {
            if k && self.rivals[ri].owned[s] == 0 {
                if let Some(m) = self.mint_manifestation(s, ent) {
                    self.rivals[ri].owned[s] = m as u16;
                }
            }
        }
        {
            let r = &mut self.rivals[ri];
            r.mana = 1000;
            r.mana_delta = 0;
            r.grace = 100;
            r.regen_stall = 0;
            r.state = AiState::Fresh;
            r.target = 0;
            r.burst = 0;
            r.poverty = false;
            r.jink = 0;
            r.hate = [HATE_NEUTRAL; 8];
            r.war = [false; 8];
            r.cooldown = [0; SPELL_COUNT];
            r.cooldown[16] = 4 * r.slot as u16;
        }
        // Re-seat the +140 mana mirror with the base pool.
        self.g.ent[i].f140 = 1000;
        // Everyone else's ledger toward the respawner: the elevated-
        // but-decaying truce value (:55037-41).
        let slot = self.rivals[ri].slot as usize;
        for (oj, o) in self.rivals.iter_mut().enumerate() {
            if oj != ri {
                o.hate[slot] = HATE_RESPAWN;
            }
        }
        self.entities_dirty = true;
    }
}

// ------------------------------------------------------------ snapshot

use crate::snapshot::{Reader, Snap, SnapshotError, Writer, snap_enum};

snap_enum!(
    AiState,
    "AiState",
    0 => AiState::Fresh,
    1 => AiState::Upgrade,
    2 => AiState::Build,
    3 => AiState::Possess,
    4 => AiState::RaidCastle,
    5 => AiState::AttackWizard,
    6 => AiState::RaidBalloon,
    7 => AiState::HuntMana,
    8 => AiState::Home,
    9 => AiState::Cruise,
);

impl Snap for Rival {
    fn put(&self, w: &mut Writer) {
        let Rival {
            slot,
            ent,
            owned,
            known,
            allowed,
            learn,
            cooldown,
            mana,
            mana_max,
            mana_delta,
            agg,
            acc,
            tempo,
            state,
            hate,
            war,
            burst,
            poverty,
            target,
            target_sig,
            site,
            jink,
            knock_dir,
            knock_mag,
            vdes,
            grace,
            regen_stall,
            life_rate,
            eliminated,
            shield,
            invisible,
            rebound,
        } = self;
        w.put(slot);
        w.put(ent);
        w.put(owned);
        w.put(known);
        w.put(allowed);
        w.put(learn);
        w.put(cooldown);
        w.put(mana);
        w.put(mana_max);
        w.put(mana_delta);
        w.put(agg);
        w.put(acc);
        w.put(tempo);
        w.put(state);
        w.put(hate);
        w.put(war);
        w.put(burst);
        w.put(poverty);
        w.put(target);
        w.put(target_sig);
        w.put(site);
        w.put(jink);
        w.put(knock_dir);
        w.put(knock_mag);
        w.put(vdes);
        w.put(grace);
        w.put(regen_stall);
        w.put(life_rate);
        w.put(eliminated);
        w.put(shield);
        w.put(invisible);
        w.put(rebound);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Rival {
            slot: r.get()?,
            ent: r.get()?,
            owned: r.get()?,
            known: r.get()?,
            allowed: r.get()?,
            learn: r.get()?,
            cooldown: r.get()?,
            mana: r.get()?,
            mana_max: r.get()?,
            mana_delta: r.get()?,
            agg: r.get()?,
            acc: r.get()?,
            tempo: r.get()?,
            state: r.get()?,
            hate: r.get()?,
            war: r.get()?,
            burst: r.get()?,
            poverty: r.get()?,
            target: r.get()?,
            target_sig: r.get()?,
            site: r.get()?,
            jink: r.get()?,
            knock_dir: r.get()?,
            knock_mag: r.get()?,
            vdes: r.get()?,
            grace: r.get()?,
            regen_stall: r.get()?,
            life_rate: r.get()?,
            eliminated: r.get()?,
            shield: r.get()?,
            invisible: r.get()?,
            rebound: r.get()?,
        })
    }
}

// ------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::{PlayerCommand, PlayerPose};
    use mgc_formats::{Thing, ThingKind};

    /// Diamond-ring SEARCH.DAT + a 4x4 building row — the same
    /// synthetic shape the world/feature unit tests use, so no baked
    /// tree is needed.
    fn assets() -> FeatureAssets {
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
        for row in 0..4 {
            dat.push(4u8);
            if row == 1 || row == 2 {
                dat.extend_from_slice(&[0x10, 7, 7, 0x10]);
            } else {
                dat.extend_from_slice(&[0x10, 0x10, 0x10, 0x10]);
            }
            dat.push(0);
        }
        FeatureAssets::parse(&grid, &tab, &dat).unwrap()
    }

    /// One rival at tile (120,120) with Fireball + Shield + Rebound +
    /// Castle in its book and a level-1 starting castle —
    /// CASTLE_CAP[1] = 10000 clears Rebound's 8000 castle_req, so the
    /// token is not fizzled by the stored-mana ladder.
    fn rebound_world() -> World {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let things = vec![Thing {
            slot: 0,
            kind: ThingKind::Entity,
            class: 3,
            model: 5,
            x: 120,
            y: 120,
            dis_id: 0,
            swi_sz: 0,
            swi_id: 0,
            parent: 0,
            child: 0,
            par3: None,
        }];
        let mut w = World::new(planes, &things, 1, assets());
        let mut book = [false; SPELL_COUNT];
        book[0] = true;
        book[4] = true;
        book[14] = true;
        book[16] = true;
        let mut cfgs: [Option<RivalConfig>; 8] = Default::default();
        cfgs[1] = Some(RivalConfig {
            aggression: 200,
            // tempo 255 → think period 1: the defense arm runs every tick.
            accuracy: 255,
            tempo: 255,
            castle_level: 2,
            book,
            allowed: book,
        });
        w.set_wizards(&cfgs, 2);
        w
    }

    fn away() -> PlayerPose {
        PlayerPose::from_tiles(10.0, 105.0 / 8.0, 10.0, 0.0, 0.0, 0.0)
    }

    /// Plant a stationary class-9 model-0 (fireball) threat homing on
    /// the rival, 512 units off — inside `sub_16890`'s 1024 reactive
    /// radius and outside the extents that would resolve it as a hit.
    fn plant_threat(w: &mut World, ri: usize) -> usize {
        let me = w.rivals[ri].ent;
        let (x, y, z) = {
            let e = &w.g.ent[me as usize];
            (e.x.wrapping_add(512), e.y, e.z)
        };
        let p = w.g.spawn_fireball(x, y, z).expect("threat slot");
        let e = &mut w.g.ent[p];
        e.f146 = me;
        e.id24 = PLAYER_TARGET;
        // Parked: the reactive arm reads position, not motion, and a
        // moving bolt would resolve into the carpet within a tick.
        e.f126 = 0;
        e.f128 = 0;
        e.act_life = 4000;
        e.max_life = 4000;
        p
    }

    fn token_of(w: &World, ri: usize, spell: usize) -> i16 {
        let m = w.rivals[ri].owned[spell] as usize;
        assert!(m != 0, "the rival never minted a spell-{spell} token");
        w.g.ent[m].f26
    }

    fn token(w: &World, ri: usize) -> i16 {
        let m = w.rivals[ri].owned[14] as usize;
        assert!(m != 0, "the rival never minted a Rebound manifestation");
        w.g.ent[m].f26
    }

    fn rebound_bit(w: &World, ri: usize) -> bool {
        w.g.ent[w.rivals[ri].ent as usize].flags & 0x8000 != 0
    }

    /// The rival Rebound arm, end to end: an incoming fireball inside
    /// 1024 arms the token (`sub_16890` :19822 → `sub_155F0` case 0xE
    /// :19140-48), the token PUBLISHES the deflection bit on the
    /// wizard entity (`sub_573F0_57920` remc1 :65774 / remc1hw
    /// :61996 — `owner->+17 |= 0x80`, our 0x8000), the bit clears when
    /// the 101-tick burst lapses, and a fresh threat re-ups it.
    ///
    /// NON-VACUITY: before the fix the port never wrote 0x8000 for a
    /// rival at all (the mirror was cloak-only), so `rebound_bit`
    /// was false at every one of these assertions and nothing could
    /// ever deflect off an AI wizard.
    #[test]
    fn rival_rebound_arms_publishes_expires_and_re_ups() {
        let mut w = rebound_world();
        assert!(!rebound_bit(&w, 0), "the bit starts clear");
        assert_eq!(token(&w, 0), 0, "the token starts idle");

        // ---- arm ------------------------------------------------------
        let threat = plant_threat(&mut w, 0);
        let mut armed = None;
        for n in 0..8 {
            w.tick(away(), PlayerCommand::default());
            if token(&w, 0) > 0 {
                armed = Some(n);
                break;
            }
        }
        let armed = armed.expect("the incoming fireball never armed Rebound");
        assert!(
            token(&w, 0) >= SPELLS[14].count as i16 - armed as i16 - 2,
            "the token armed short of its {} count",
            SPELLS[14].count
        );
        // The bit lands one tick behind the arm: the token publishes
        // on its OWN tick, which for a rival is the next pass of
        // `rival_refresh_buffs` (retail: the class-12 handler's own
        // slot in the entity loop).
        w.tick(away(), PlayerCommand::default());
        assert!(
            rebound_bit(&w, 0),
            "the armed token did not publish the 0x8000 deflection bit"
        );

        // ---- the already-active gate ----------------------------------
        // `sub_15A00` case 0xE (:19289-96) refuses a re-cast while the
        // burst is live, so the window is ONE uninterrupted countdown
        // even though the threat is still there and AI_RECAST[14] = 1.
        // Pre-gate the port re-armed to `count` every other tick.
        let mut prev = token(&w, 0);
        for _ in 0..20 {
            w.tick(away(), PlayerCommand::default());
            let now = token(&w, 0);
            assert!(
                now < prev,
                "the live Rebound token was re-armed ({prev} -> {now}) while its burst ran"
            );
            prev = now;
        }

        // ---- expiry ---------------------------------------------------
        w.g.ent[threat].flags |= 0x400;
        for _ in 0..(SPELLS[14].count as usize + 8) {
            w.tick(away(), PlayerCommand::default());
            if token(&w, 0) == 0 {
                break;
            }
        }
        assert_eq!(token(&w, 0), 0, "the token never expired");
        assert!(
            !rebound_bit(&w, 0),
            "the lapsed token left the deflection bit set"
        );

        // ---- re-up ----------------------------------------------------
        plant_threat(&mut w, 0);
        let mut re_upped = false;
        for _ in 0..8 {
            w.tick(away(), PlayerCommand::default());
            if token(&w, 0) > 0 && rebound_bit(&w, 0) {
                re_upped = true;
                break;
            }
        }
        assert!(re_upped, "a fresh threat did not re-up Rebound");
    }

    /// The deflection itself (`sub_52B30` :62858-90): a bolt striking
    /// a rebounding wizard pays a quarter of its own +140 out of the
    /// WIZARD's +140 (his mana — the port's entity mirror of
    /// `Rival::mana`), twangs (sound 28 — INSIDE the afford branch,
    /// :62861), and reverses onto its shooter with the wizard as its
    /// new owner — it never explodes. An unaffordable deflection is
    /// retail's silent fly-through: no hit, no sound, no explosion,
    /// no debit (the :62859 false arm leaves v24 clear).
    ///
    /// NON-VACUITY: pre-fix the port (a) never wrote the wizard's
    /// +140 mirror, so the gate compared against 0 and every real
    /// bolt fell through to the explode — the player-reported
    /// "rebound sound but the meteor explodes on him and nothing
    /// comes back" — and (b) played sound 28 BEFORE the gate. The
    /// deflect arm, the debit, the poor-arm silence and the mirror
    /// assertions all fail on that code.
    #[test]
    fn rebound_deflection_bounces_debits_and_is_silent_when_poor() {
        use crate::mc1::mobs::MobCtx;
        let mut w = rebound_world();
        // Settle a few ticks, then check the WORLD maintains the
        // entity mana mirror at all.
        for _ in 0..4 {
            w.tick(away(), PlayerCommand::default());
        }
        let me = w.rivals[0].ent as usize;
        assert_eq!(
            w.g.ent[me].f140, w.rivals[0].mana as i32,
            "the wizard entity's +140 does not mirror Rival::mana"
        );

        // The deflection reader is driven directly (the bit is the
        // published state, not the token) for slot-order-free
        // arithmetic.
        w.g.ent[me].flags |= 0x8000;
        let ctx = MobCtx {
            px: 10,
            py: 10,
            pz: 200,
            pyaw: 0,
            pmana: 0,
            pdead: false,
            strict: false,
            patches: crate::patches::WorldPatches::RETAIL,
            mc2_turn: 0,
        };

        // ---- the affordable deflect -----------------------------------
        // Park the encounter far from the starting castle: the keep's
        // 0x2000-tall envelope otherwise scan-resolves the CASTLE
        // (no bit) instead of the wizard hovering at it.
        w.g.ent[me].f140 = 1000;
        let (wx, wy) = (60u16 << 8, 60u16 << 8);
        let wz = (w.g.ground_z(wx, wy) as i16).wrapping_add(400);
        w.g.move_relink(me, wx, wy, wz);
        let bolt = w.g.spawn_fireball(wx, wy, wz).expect("bolt slot");
        w.g.move_relink(bolt, wx, wy, wz); // the spawner tile-snaps
        {
            let e = &mut w.g.ent[bolt];
            e.id24 = PLAYER_TARGET;
            e.f126 = 0; // parked on the wizard: the scan overlaps
            e.f140 = 400; // quarter = 100
        }
        w.g.sounds.clear();
        w.g.proj_tick(bolt, &ctx);
        {
            let e = &w.g.ent[bolt];
            assert_eq!(e.flags & 0x400, 0, "the deflected bolt exploded");
            assert_eq!(
                e.id24, w.rivals[0].ent,
                "ownership did not swap to the deflector"
            );
            assert_eq!(e.f146, PLAYER_TARGET, "not re-homed on the shooter");
        }
        assert_eq!(
            w.g.ent[me].f140, 900,
            "the deflection did not debit a quarter of the bolt's +140"
        );
        assert!(
            w.g.sounds.iter().any(|s| s.id == 28),
            "no twang on a successful deflection"
        );
        w.g.ent[bolt].flags |= 0x400;

        // ---- the poor wizard: silent fly-through ----------------------
        w.g.ent[me].f140 = 50;
        let bolt2 = w.g.spawn_fireball(wx, wy, wz).expect("bolt2 slot");
        w.g.move_relink(bolt2, wx, wy, wz);
        {
            let e = &mut w.g.ent[bolt2];
            e.id24 = PLAYER_TARGET;
            e.f126 = 0;
            e.f140 = 400; // quarter 100 > the 50 he holds
        }
        w.g.sounds.clear();
        w.g.proj_tick(bolt2, &ctx);
        {
            let e = &w.g.ent[bolt2];
            assert_eq!(e.flags & 0x400, 0, "the fly-through bolt exploded");
            assert_eq!(e.id24, PLAYER_TARGET, "the poor wizard still deflected");
        }
        assert_eq!(w.g.ent[me].f140, 50, "the failed deflection still debited");
        assert!(
            w.g.sounds.iter().all(|s| s.id != 28),
            "an unaffordable deflection twanged (retail is silent, :62861)"
        );

        // The debit round-trips into the pool on the wizard's next
        // tick: the downward reconcile pulls Rival::mana to the
        // debited mirror before the regen step re-adds its delta.
        w.g.ent[me].f140 = 900;
        let pre = w.rivals[0].mana;
        assert!(
            pre > 2000,
            "test premise: the pool must sit well above the debited mirror"
        );
        w.tick(away(), PlayerCommand::default());
        assert!(
            w.rivals[0].mana < pre,
            "the mirror debit never reconciled into Rival::mana"
        );
    }

    /// `sub_16890`'s default arm casts NOTHING (remc1 :19815-52 /
    /// remc1hw :17947-84): only projectile models {0,3,16} reach the
    /// Rebound/Shield ladder and {4,9} the Shield-only one. The port
    /// used to fall every other model through to Shield.
    /// NON-VACUITY: the pre-fix `_ => 4` fallback armed the SHIELD
    /// token on a model-2 threat; the model-9 control proves the
    /// Shield arm itself is live, so the first assertion is not
    /// passing for want of a castable Shield.
    #[test]
    fn unlisted_threat_models_provoke_no_reactive_cast() {
        let mut w = rebound_world();
        // Bank enough mana for Shield (2000) so a failed cast can only
        // be the model gate, never affordability.
        for _ in 0..40 {
            w.tick(away(), PlayerCommand::default());
        }
        assert!(w.rivals[0].mana >= SPELLS[4].possess_mana);

        let threat = plant_threat(&mut w, 0);
        w.g.ent[threat].model65 = 2;
        for _ in 0..8 {
            w.tick(away(), PlayerCommand::default());
        }
        assert_eq!(
            token_of(&w, 0, 4),
            0,
            "a model-2 threat provoked the Shield cast retail ignores"
        );
        assert_eq!(token(&w, 0), 0, "a model-2 threat provoked Rebound");
        assert!(!rebound_bit(&w, 0));

        // Model 9 IS in retail's Shield-only arm (:19845-49).
        w.g.ent[threat].flags |= 0x400;
        let control = plant_threat(&mut w, 0);
        w.g.ent[control].model65 = 9;
        let mut shielded = false;
        for _ in 0..8 {
            w.tick(away(), PlayerCommand::default());
            if token_of(&w, 0, 4) > 0 {
                shielded = true;
                break;
            }
        }
        assert!(shielded, "the model-9 control never armed Shield");
    }
}
