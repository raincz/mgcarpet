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

    /// Variant → canonical +415 byte, the inverse of
    /// [`Self::from_retail`] up to the cut states (2/4/5/10 all read
    /// back as Fresh's 0) — compare retail bytes through
    /// `from_retail(a).to_retail()` so the collapse is symmetric.
    pub(crate) fn to_retail(self) -> u8 {
        match self {
            AiState::Fresh => 0,
            AiState::Upgrade => 1,
            AiState::Build => 3,
            AiState::Possess => 6,
            AiState::RaidCastle => 7,
            AiState::AttackWizard => 8,
            AiState::RaidBalloon => 9,
            AiState::Home => 0xB,
            AiState::Cruise => 0xC,
            AiState::HuntMana => 0xD,
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
    /// The +532 ACQUISITION LIST — manifestation pool slots in PICKUP
    /// order while alive. The death scatter iterates THIS, not the
    /// spell-id book (mc1l4 t=6885: two jars' scatter draws land in
    /// list order), rewriting each live entry to the token's MODEL
    /// number and each empty one to −1 (:55519-49); the respawn
    /// re-grant re-mints from the rewritten entries IN PLACE
    /// (:54884-923 — a scattered fireball's model 0 collides with the
    /// empty sentinel by design: the −1→0 reset skips, the 0 entry
    /// re-mints model 0, and that collision is exactly how fireball
    /// ownership survives death). Grants append at the first ZERO
    /// entry (:19421-31).
    pub(crate) acq: [i32; SPELL_COUNT],
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
    /// Retail wizext +526 — the live cadence scalar (think period,
    /// turn-servo divisor, burst lockout). Seeded from the level
    /// config natively; the conformance import overwrites it with the
    /// recorded value (retail re-stamps it at init and every respawn).
    pub(crate) tempo: u16,
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
    /// ⭐ THE SPEED-COLUMN LATCH (v_14) — "the BRAIN wrote v_12 this
    /// tick". `sub_15470` CLEARS it at its head (:19057) and every
    /// leg that actually writes v_12 sets it: the arrival stop
    /// (:19075-76) and the plain throttle (:19089-91), plus the
    /// cruise/home twins `sub_13A10` (:18197-98) and `sub_13A70`
    /// (:18220-21) — which do NOT clear it, so a state that never
    /// consults `sub_15470` leaves the latch standing.
    ///
    /// Its ONLY consumer is the speed token (`sub_56380` :65147-50 /
    /// `sub_57F00` :66186-89): a set latch means the AI has retaken
    /// the speed columns, so the burst KILLS ITSELF — `+48 = 1`, and
    /// the shared decrement below zeroes it that same tick. That is
    /// the two-phase kill: mc1l5's Vodor arrives at t=185 with the
    /// burst still 63 ticks from expiry and the token drops to 0 in
    /// one step, snapping his speed 160 → 80.
    ///
    /// Not in the recording (the closure carries v_12 but not v_14),
    /// so it rides the port's own snapshot like `vdes`.
    pub(crate) v14: bool,
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
            acq: [0; SPELL_COUNT],
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
            v14: false,
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

    /// The pickup append (:19421-31): the first ZERO entry of the
    /// acquisition list takes the freshly minted token's pool slot; a
    /// full list drops the append, exactly as retail's 24-bounded scan.
    fn acq_push(&mut self, m: u16) {
        if let Some(e) = self.acq.iter_mut().find(|e| **e == 0) {
            *e = m as i32;
        }
    }

    /// The rival's wizext/brain registers as RETAIL-convention lanes —
    /// the per-rival half of `World::wiz_shadow_mc1` (this module owns
    /// the private brain fields, so the projection lives here). Lane
    /// names match `RetailWizardMc1`'s fields; `ai_state` is the
    /// canonical [`AiState::to_retail`] byte, `poverty` is the latch as
    /// 0/1 (retail keeps a mana threshold in the live latch, the port a
    /// bool — nonzero-ness is the comparable fact), `war` likewise.
    /// `v_14` and `target` are deliberately absent: v_14 is not in the
    /// recording, and the target rides the carpet entity's graded f146.
    pub(crate) fn wiz_shadow_lanes(
        &self,
    ) -> (Vec<(&'static str, i64)>, Vec<(&'static str, Vec<i64>)>) {
        let scalars = vec![
            ("cmd_speed", self.vdes as i64),
            ("strafe", self.jink as i64),
            ("knock_mag", self.knock_mag as i64),
            ("knock_dir", self.knock_dir as i64),
            ("grace", self.grace as i64),
            ("regen_stall", self.regen_stall as i64),
            ("life_rate", self.life_rate as i64),
            ("ai_state", self.state.to_retail() as i64),
            ("burst", self.burst as i64),
            ("poverty", self.poverty as i64),
            ("target_sig", self.target_sig as i64),
            ("mana_delta", self.mana_delta as i64),
        ];
        let arrays = vec![
            ("hate", self.hate.iter().map(|&v| v as i64).collect()),
            ("war", self.war.iter().map(|&v| v as i64).collect()),
            ("learn", self.learn.iter().map(|&v| v as i64).collect()),
            (
                "cooldown",
                self.cooldown.iter().map(|&v| v as i64).collect(),
            ),
            ("owned", self.owned.iter().map(|&v| v as i64).collect()),
            ("acq", self.acq.iter().map(|&v| v as i64).collect()),
        ];
        (scalars, arrays)
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
                    r.acq_push(m as u16);
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
    /// ⭐⭐ IT IS THE GROUND-JAR CTOR, RUN AT THE WIZARD'S OWN
    /// POSITION — every retail mint site hands the class-12 thunk
    /// `&wizard.position`: the learn expiry
    /// (`off_987DE[s].adress(a1 + 72)`, :19417), the level-start book
    /// and the respawn re-grant (`sub_373F0(&wiz.pos, 12, model)`,
    /// :54900). The token is then stamped OWNED — `+16 |= 1` and
    /// `+42 = the wizard slot` (:19428-29 / :54905-06) — and pushed
    /// onto the acquisition list by the caller.
    ///
    /// The old port mint wrote four fields onto a bare `new_event`
    /// and never LINKED it, so a rival's conjured token stood at the
    /// origin with `NewEvent`'s 300 life and an empty cost cache.
    /// mc1hwl0 t=2350 is the whole of it: rival 1's Armageddon
    /// countdown expires and retail conjures `(12,20)` at slot 256
    /// carrying `flags 5`, `+50 = 26`, `+136 = 5000`, `+140 = 192`,
    /// life `0/0` and the wizard's own x/y/z, against the port's
    /// zeros and `max_life 300`. [`World::spawn_spell_jar`] is that
    /// ctor verbatim and was already right for the world's jars —
    /// ⭐ *when two paths model one retail constructor, DIFF THEM*
    /// ([[mc1-jar-poll-walk-slot]] again).
    ///
    /// ⚠ `+70` follows the world's encoding, not a fixed one: a
    /// conformance import carries retail's `3·spell + phase` (phase 0
    /// = an owned token) and `class12_tick` dispatches on it, while a
    /// native world uses [`MANIFEST_BASE`]` + spell`. The `+16` bit 0
    /// is what tells the strict arm a `3·spell` record is a TOKEN and
    /// not a jar, so it is not decoration.
    fn mint_manifestation(&mut self, spell: usize, owner: u16) -> Option<usize> {
        let (wx, wy, wz) = {
            let e = self.g.ent.get(owner as usize)?;
            (e.x, e.y, e.z)
        };
        let state = if self.strict_retail {
            (spell * 3) as u8
        } else {
            crate::engine::world::MANIFEST_BASE + spell as u8
        };
        let m = self.spawn_spell_jar(spell, state, wx, wy, wz)?;
        {
            let e = &mut self.g.ent[m];
            e.flags |= 1; // :19428 / :54906 — the OWNED-token bit
            e.f26 = 0;
            // Retail's +42 = the owning wizard's slot; the port homes
            // that lane at f144 for class 12 (the importer's own
            // normalization — conformance.rs `f144: tr(r.f42)`).
            e.f144 = owner;
        }
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
            // ⚠ NO `+70` write here: the mint (:54974-55002) calls the
            // ctor and never touches the job byte, so the authored
            // castle stands at the ctor's TRANSFORM state (5, sub-state
            // 0) and its FIRST tick runs the level-up commit — which is
            // what carries `+26` from `count - 1` to the authored level
            // and paints the top row. The mc1l5 capture reads it
            // mid-flight: castle 680 is `+70 = 5, +48 = 4` at t=1 (the
            // commit's own wait, :56469) and only settles later.
            // A hard `tick70 = 4` here — added when `f59` alone drove
            // the machine and this write was inert — now suppresses
            // that commit and leaves every authored castle one level
            // short with an unpainted top.
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
        // ⭐ The castle resolves through wizext+50, the BOUND register
        // (:17971 `v14 = wizext+50`): an authored castle that never
        // leveled grants neither the mail-discard grace nor the fast
        // regen fork. And the probe is `sub_11950` = the FULL summed-
        // extents AABB (signed +78 z leg) — the same law the human's
        // `regen_boost` already wears (mc1l0 t=1827). mc1l5 t=11681:
        // Vodor brushes his keep's summed box at |dx| 3362 vs
        // 3328+125, and retail flips him to the at-castle +1000/tick
        // where the port's bare `<= f80/f82` point test kept the
        // away-rate +100.
        let castle = self.rival_castle(self.rivals[ri].ent);
        let at_castle = castle
            .filter(|&c| self.g.ent[c].flags & 2 != 0)
            .is_some_and(|c| self.g.ent_overlap(i, c));
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

    /// The castle-arm war check (:19733-39): hate past the threshold
    /// raises the war flag. ⭐⭐ THE MC1 THRESHOLD IS FLAT 50000: the
    /// listing scales it by an aggression word read through the victim
    /// CASTLE entity's +160 (`*(v3+160)+522`) — but only CARPETS carry
    /// the wizext pointer at +160; a castle's is the mint's zero, so
    /// the read lands in low memory and the scaled term never
    /// contributes (the unguarded-pointer constant class — the same
    /// shape as the l42 `+146` null-probe ruling). Measured on mc1l5's
    /// four hate windows: war latches at 50518/50659/51518, each the
    /// FIRST crossing above 50000, never at 49518/49659, and window 4
    /// peaks at 49531 and decays out unlatched — the rival's real agg
    /// (115; the decay's 256−agg = 141, t=14600) would have latched
    /// five ticks early at 44505. The MC2 twin (EF:7402-03) reads real
    /// wizard structs (`v1x->maxMana_0x8C * v2x_owner->word_0x242`),
    /// so its threshold IS wealth-scaled; this ruling is MC1's alone.
    fn rival_war_check(&mut self, ri: usize, shooter: u8) {
        if shooter as usize >= 8 || self.rivals[ri].slot == shooter {
            return;
        }
        let r = &mut self.rivals[ri];
        if r.hate[shooter as usize] as u32 > 50_000 {
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
                    self.rival_war_check(ri, shooter);
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
            if std::env::var_os("MGC_JINK_TRACE").is_some() {
                eprintln!(
                    "[jink] t={} ri={ri} mover decays {} -> {}",
                    crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                    jink,
                    self.rivals[ri].jink
                );
            }
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

    /// Spell learning, the COUNTDOWN half (sub_15EC0 :19381-443): a
    /// live timer decrements 1/tick and the expiry conjures the
    /// rival's own manifestation. Arming lives on the JAR side
    /// ([`World::rival_learn_arm`], the pickup poll).
    ///
    /// The countdown's CLOCK is the tick-top roster's model-0 entry:
    /// :19394-99 walks bucket[0] and runs the dec pass once per
    /// model-0 carpet, so with the human dead (out of the roster) the
    /// timers freeze. The per-slot gate is `+676 == 0` alone
    /// (:19407) — no allowed test, no known test.
    fn rival_learn_tick(&mut self, ri: usize) {
        // The clock is the TICK-TOP roster's model-0 entry, so a
        // mid-tick death still clocks this tick (`human_bucket_alive`,
        // the :52254 membership sample).
        if !self.human_bucket_alive {
            return;
        }
        for s in 0..SPELL_COUNT {
            if self.rivals[ri].owned[s] != 0 {
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
                    self.rivals[ri].acq_push(m as u16);
                }
            }
        }
    }

    /// Spell learning, the ARM half (:64806-15) — runs inside the
    /// jar's pickup poll, ONLY on a tick the living human carpet
    /// AABB-hits the jar (sub_55A40's roster walk returns without
    /// reaching the arm unless the hit breaks it): every AI wizard
    /// (model 1) on the tick-top roster that neither owns nor is
    /// already learning the jar's spell, and whose book allows it
    /// (+796), arms the 200-tick countdown. The old port fold — "a
    /// matching jar exists anywhere" scanned from the rival's side
    /// every tick — armed off jars retail never dispatched: mc1hwl0's
    /// out-of-reach spell-6 jar armed Vodor at t=1 and the expiry
    /// minted a (12,6) retail never saw.
    pub(crate) fn rival_learn_arm(&mut self, spell: usize) {
        for c in 0..self.g.wiz_chain.visible_len() {
            let j = self.g.wiz_chain.list[c] as usize;
            if self.g.ent[j].model65 != 1 {
                continue;
            }
            let Some(ri) = (0..self.rivals.len())
                .find(|&r| self.rivals[r].ent == j as u16 && !self.rivals[r].eliminated)
            else {
                continue;
            };
            let r = &mut self.rivals[ri];
            if r.owned[spell] == 0 && r.learn[spell] == 0 && r.allowed[spell] {
                r.learn[spell] = 200;
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
        if !matches!(spell, 0 | 3 | 7 | 8 | 11 | 13 | 15 | 17 | 20 | 23) {
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
        // ⭐ AND THE GATE IS THE WHOLE `sub_55DD0`, NOT ITS LIFE LEG:
        // the owner's life, the token's live CASTLE REQUIREMENT and —
        // on a FULL tick only — the purse. The commit no longer
        // pre-screens the castle ladder ([`World::rival_cast`]), so
        // this is where a castle-gated burst dies: 26 → 1 → 0 in one
        // token tick (mc1hwl0 t=2426-27, token 256).
        if !self.rival_token_gate(ri, spell, f26 == count) {
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

    /// THE REBOUND TOKEN'S OWN MACHINE — retail's `sub_573F0_57920`
    /// (remc1 :65774 / remc1hw :61996, class-12 state 0x2A), run at
    /// the token's pool slot from `class12_tick` in both encodings;
    /// the human's twin is `manifestation_tick`'s spell-14 arm. The
    /// bare skeleton: while the `+48` burst is live, the WHOLE
    /// `sub_55DD0` gate runs every tick (owner alive, Rebound's 8000
    /// castle store, FULL tick adds the purse); a pass sets the
    /// OWNER's +17 bit 7 — our 0x8000, the deflection bit
    /// `proj_move_and_hit` reads — runs `sub_55E80` (full → the debit
    /// on the regen delta, mid-burst → the positive-delta pin) and
    /// takes the shared decrement; a refusal drops the burst to 1 for
    /// that same decrement, and the bit it leaves standing falls to
    /// the NEXT pass's `+48 <= 0` clear arm — the gate-fail path
    /// touches no flags.
    ///
    /// mc1hwl0 t=5592: rival 1's defense cast arms token 479 to 101
    /// at the carpet's slot 473, and retail's SAME-TICK token pass
    /// (479 walks after 473) gates, publishes carpet `flags`
    /// 12 → 32780 and decrements to 100. The port used to drive this
    /// from `rival_refresh_buffs` at the RIVAL's own tick — which
    /// runs before the defense cast — so the bit lagged one tick and
    /// the counter sat one above retail for the burst's whole life.
    pub(crate) fn rival_rebound_token_tick(&mut self, m: usize, ri: usize) {
        let i = self.rivals[ri].ent as usize;
        if i == 0 {
            return;
        }
        if self.g.ent[m].f26 <= 0 {
            self.g.ent[i].flags &= !0x8000;
            return;
        }
        let def = &self.spells()[14];
        let full = self.g.ent[m].f26 == def.count as i16;
        let cost = def.possess_mana;
        if self.rival_token_gate(ri, 14, full) {
            self.g.ent[i].flags |= 0x8000;
            // sub_55E80 from the token (:64942-56).
            let r = &mut self.rivals[ri];
            if full {
                let c = cost.min(i32::MAX as u32) as i32;
                r.mana_delta = if r.mana_delta >= 0 {
                    -c
                } else {
                    r.mana_delta - c
                };
            } else if r.mana_delta > 0 {
                r.mana_delta = 0;
            }
        } else {
            self.g.ent[m].f26 = 1;
        }
        if self.g.ent[m].f26 > 0 {
            self.g.ent[m].f26 -= 1;
        }
    }

    /// THE SHIELD TOKEN'S OWN MACHINE — retail's `sub_566C0` (:65266,
    /// class-12 state 12), the rival twin of
    /// [`World::mc1_shield_token_tick`]: the same `sub_573F0` skeleton
    /// as Rebound's, but the owner bit is +17 0x40 (our 0x4000) and
    /// there is NO clear arm — the bit is SET-only here and cleared
    /// PER-ABSORB by the damage intake (:55700-07,
    /// [`Self::rival_mail_block`]'s quarter), so an expired shield
    /// still quarters exactly one more hit.
    ///
    /// mc1hwl0 t=5593: rival 1's defense ladder falls through to
    /// Shield (Rebound live from 5592), the commit debits −1000 and
    /// retail's same-tick token pass publishes carpet 473's `flags`
    /// 0x800C → 0xC00C. The refresh-driven port paid but never
    /// published, and no rival shield ever quartered a hit.
    pub(crate) fn rival_shield_token_tick(&mut self, m: usize, ri: usize) {
        let i = self.rivals[ri].ent as usize;
        if i == 0 || self.g.ent[m].f26 <= 0 {
            return;
        }
        let def = &self.spells()[4];
        let full = self.g.ent[m].f26 == def.count as i16;
        let cost = def.possess_mana;
        if self.rival_token_gate(ri, 4, full) {
            self.g.ent[i].flags |= 0x4000;
            // sub_55E80 from the token (:64942-56).
            let r = &mut self.rivals[ri];
            if full {
                let c = cost.min(i32::MAX as u32) as i32;
                r.mana_delta = if r.mana_delta >= 0 {
                    -c
                } else {
                    r.mana_delta - c
                };
            } else if r.mana_delta > 0 {
                r.mana_delta = 0;
            }
        } else {
            self.g.ent[m].f26 = 1;
        }
        if self.g.ent[m].f26 > 0 {
            self.g.ent[m].f26 -= 1;
        }
    }

    /// THE CASTLE TOKEN'S OWN MACHINE — retail's `sub_57610_57B40`
    /// (:65862-923, class-12 state 48), shared by human and rival
    /// owners alike. Unlike the generic launcher it has NO per-tick
    /// decrement: the commit arms `+48 = +50` (101), the FULL tick
    /// alone fires — sub_55E80 debit, the (9,10) castle ball minted at
    /// the OWNER's own axis — and `+48` then parks at `+50 − 1`, the
    /// IN-TRANSIT CHARGE PIN the ball's delivery or failure releases
    /// (`sub_46D20` → [`Gen::release_castle_charge_pin`]). A refused
    /// `sub_55DD0` gate zeroes the counter outright (:65920); a failed
    /// allocation leaves it FULL, so the mint retries next tick (the
    /// child-allocation guard family). The ball rides the owner's
    /// speed (+126 +=), banks the wizard's accumulated charge meter
    /// (+26 = wizext+326, zeroed), and splits on the ESTABLISHED
    /// castle: standing → homing upgrade ball (+146 = castle, explode
    /// child (10,43)); none → the 4096-ahead build lob whose child is
    /// the (3,2) castle itself. `sub_55EF0`'s hand-muzzle sidestep is
    /// gated on the owner's 0x100/0x200 fire bits — the commit clears
    /// 0x100 (:19110) and no rival path sets 0x200, so a rival's ball
    /// launches from the hull (no-op here).
    ///
    /// mc1l5 t=5152: Vodor upgrades his castle — charge 200 → 0 into
    /// ball 790's +26, cooldown[16] = 40, the ball arrives the same
    /// tick and morphs into the (10,43) at slot 737. The port's old
    /// ch5-mail shortcut (DEVIATIONS.md "rival_cast_castle (upgrade
    /// token)", now retired) skipped the whole ride, which the corpus
    /// proves is NOT cosmetic: two graded entity rows per upgrade.
    pub(crate) fn rival_castle_token_tick(&mut self, m: usize, ri: usize) {
        if self.g.ent[m].f26 <= 0 {
            return;
        }
        let i = self.rivals[ri].ent as usize;
        if i == 0 || i >= self.g.ent.len() {
            return; // owner gone: the token stalls (:65873-74)
        }
        let count = self.spells()[16].count as i16;
        let full = self.g.ent[m].f26 == count;
        let price = self.rival_castle_price(ri);
        // sub_55DD0 (:64915-24): owner-dead refuses every tick, the
        // cost compare runs on the FULL tick only.
        if self.g.ent[i].act_life < 0 || (full && self.rivals[ri].mana < price) {
            self.g.ent[m].f26 = 0;
            return;
        }
        if !full {
            return; // in transit — the pin holds
        }
        let (ex, ey, ez, yaw, pitch, ospeed, lift, otag) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.f126, e.f84 as i16, e.id24)
        };
        let Some(b) = self.g.spawn_castle_ball(ex, ey, ez) else {
            return; // allocation guard: stays FULL, re-fires next tick
        };
        // sub_55E80's full arm — the debit on the regen delta.
        {
            let r = &mut self.rivals[ri];
            let c = price.min(i32::MAX as u32) as i32;
            r.mana_delta = if r.mana_delta >= 0 {
                -c
            } else {
                r.mana_delta - c
            };
        }
        let (tok_f44, tok_f140) = {
            let t = &self.g.ent[m];
            (t.f44, t.f140)
        };
        {
            let e = &mut self.g.ent[b];
            e.f126 += ospeed; // *(v3+126) += *(v2+126)
            e.f44 = tok_f44; // *(v3+44) = *(a1+44)
            e.id24 = otag; // *(v3+24) = *(v2+24)
            e.z = e.z.wrapping_add(lift); // *(v3+76) += *(v2+84)
            e.f140 = tok_f140; // *(v3+140) = *(a1+140)
            e.f30 = yaw;
            e.f32 = pitch;
        }
        // The wizext+50 split (:65893-908): the ESTABLISHED castle
        // stand-in, same filter the human's cast uses.
        let castle = self
            .rival_castle(self.rivals[ri].ent)
            .filter(|&c| self.g.ent[c].f26 > 0);
        if let Some(c) = castle {
            let e = &mut self.g.ent[b];
            e.f68 = 10;
            e.f69 = 43;
            e.f146 = c as u16;
        } else {
            let mut t = (ex, ey, 0i16);
            Gen::polar_step(&mut t, yaw, 0, 4096);
            let e = &mut self.g.ent[b];
            e.f68 = 3;
            e.f69 = 2;
            e.dest_x = t.0;
            e.dest_y = t.1;
        }
        // The charge move (:65910-11): the ball banks the owner's
        // accumulated meter and zeroes it.
        let ws = self.rivals[ri].slot as usize;
        self.g.ent[b].f26 = self.wiz_charge[ws] as i16;
        self.wiz_charge[ws] = 0;
        self.g.snd(15, b); // :65918
        self.g.ent[m].f26 = count - 1; // the in-transit pin
        self.entities_dirty = true;
    }

    /// ⭐ THE RIVAL'S SPEED TOKEN, at its OWN pool slot — retail's
    /// `sub_56380_568B0` (:65131-99, spell 2) and its backwards twin
    /// `sub_57F00_58410` (:66172-231, spell 21), which are the SAME
    /// function with every speed term negated. The port used to run
    /// only the contrail leg of this (in `class12_tick`) and decrement
    /// the counter over in `rival_refresh_buffs`; everything else the
    /// handler does was missing, which is what mc1l4 breaks on at
    /// t=2 and mc1l5 at t=2:
    ///
    /// - **the v_14 KILL** (:65146-51): the owner's speed-column
    ///   latch standing means the brain has retaken v_12, so the burst
    ///   force-ends — `+48 = 1`, and the shared decrement below zeroes
    ///   it the same tick. mc1l5 t=185: Vodor arrives, +48 drops
    ///   63 → 0 in one step. A REFUSED `sub_55DD0` gate skips the
    ///   sustain arm but does NOT force the end.
    /// - **the spell-ACTIVE bit** `+16 bit 7` — set on the full tick
    ///   (:65154-57), released two ticks in (:65160-65) and again at
    ///   expiry (:65196). mc1l4 t=2 measures it: token 368's flags go
    ///   `5 → 133` (0x85).
    /// - **the SPEED OVERRIDE, SNAPPED into both columns**: `v_12 =
    ///   3·f128` on the full tick, `2·f128` mid-burst, `f126 = v_12`
    ///   (:65167-78) — not the AI's 16/tick ease. mc1l4 t=2/t=3
    ///   measures f126 `0 → 240 → 160` against f128 = 80.
    /// - **`sub_55E80`** (:65188): the full tick stamps the debit on
    ///   the regen delta, every mid-burst tick PINS a positive delta
    ///   to 0 — an active spell blocks mana regeneration. mc1l4 t=2:
    ///   f132 `100 → −1000` (the cost), then `−1000 → 0` at t=3 once
    ///   the wizard pass has spent it. mc1l5's Vodor sits under a
    ///   248-tick burst from tick 0, which is why his `+132` reads 0
    ///   forever and his purse never leaves zero.
    /// - **the EXPIRY SNAP** (:65192-97): the counter reaching 0
    ///   restores `v_12 = f126 = f128` (signed: −f128 backwards) and
    ///   drops the active bit.
    ///
    /// ⚠ The decrement and the expiry snap live INSIDE the
    /// owner-valid guard, so a token whose `+42` owner is gone stalls
    /// at its current count rather than winding down.
    pub(crate) fn rival_speed_token_tick(&mut self, m: usize, ri: usize, spell: usize) {
        if self.g.ent[m].f26 <= 0 {
            return; // :65141 — the whole handler is inside `+48 > 0`
        }
        let i = self.rivals[ri].ent as usize;
        if i == 0 || i >= self.g.ent.len() {
            return; // :65144 — no owner record, nothing runs
        }
        let count = self.spells()[spell].count as i16; // the token's +50
        let full = self.g.ent[m].f26 == count;
        // The backwards twin negates every speed term (:66207-27).
        let dir: i16 = if spell == 2 { 1 } else { -1 };
        if self.rivals[ri].v14 {
            self.g.ent[m].f26 = 1; // :65149-50 — the two-phase kill
        } else if self.rival_token_gate(ri, spell, full) {
            let mut armed = false;
            {
                let e = &mut self.g.ent[m];
                if full && e.flags & 0x80 == 0 {
                    e.flags |= 0x80; // :65157
                    armed = true;
                }
                if e.f26 == count - 2 {
                    e.flags &= !0x80; // :65160-65
                }
            }
            if armed {
                // :65158 — the arm chime, at the OWNER's pool slot and
                // `a2 = -1`. Case 19 (:64525) carries no local-player
                // arm, so the AI's Accelerate is audible exactly like
                // the human's (the same law as the id-17 hit grunt).
                self.g.snd(19, i);
            }
            let base = self.g.ent[i].f128;
            let v12 = if full { 3 } else { 2 } * base * dir;
            self.rivals[ri].vdes = v12;
            self.g.ent[i].f126 = v12;
            // The (10,2) contrail at the OWNER's axis every 4th token
            // tick (:65179-87) — id24 = the caster, act_life ×4.
            if self.g.ent[m].f63 & 3 == 0 {
                let (cx, cy, cz, own) = {
                    let e = &self.g.ent[i];
                    (e.x, e.y, e.z, e.id24)
                };
                if let Some(p) = self.g.spawn_effect(2, cx, cy, cz) {
                    self.g.ent[p].id24 = own;
                    self.g.ent[p].act_life *= 4;
                }
            }
            // sub_55E80 (:65188): the debit on the full tick, the
            // regen pin on every other.
            let cost = self.spells()[spell].possess_mana.min(i32::MAX as u32) as i32;
            let r = &mut self.rivals[ri];
            if full {
                r.mana_delta = if r.mana_delta >= 0 {
                    -cost
                } else {
                    r.mana_delta - cost
                };
            } else if r.mana_delta > 0 {
                r.mana_delta = 0;
            }
        }
        // The shared decrement and the expiry snap (:65190-97).
        self.g.ent[m].f26 -= 1;
        if self.g.ent[m].f26 == 0 {
            let base = self.g.ent[i].f128 * dir;
            self.rivals[ri].vdes = base;
            self.g.ent[i].f126 = base;
            self.g.ent[m].flags &= !0x80;
        }
    }

    /// `sub_55DD0_56300` (:64909-32) for a RIVAL's token — the gate
    /// every class-12 handler runs before its sustain arm. Reads the
    /// OWNER's purse and life, then the token's own live castle
    /// requirement (+132) against the ESTABLISHED castle's store, and
    /// finally admits a FULL tick only if the purse covers the cost
    /// (:64926) while a MID-burst tick admits unconditionally
    /// (:64928). The refusal buzz (:64931) is the local player's
    /// channel and stays unported for the AI.
    fn rival_token_gate(&self, ri: usize, spell: usize, full: bool) -> bool {
        let r = &self.rivals[ri];
        let i = r.ent as usize;
        if self.g.ent[i].act_life < 0 {
            return false; // a2[3] — the owner is dying
        }
        let req = self.spells()[spell].castle_req;
        if req != 0
            && !self
                .rival_castle(r.ent)
                .is_some_and(|c| self.g.ent[c].f140.max(0) as u32 >= req)
        {
            return false;
        }
        !full || r.mana >= self.spells()[spell].possess_mana
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
        let invisible = get(12);
        // Shield (4) and Rebound (14) are NOT clocked here: their
        // tokens run retail's own machines at the token's pool slot
        // ([`Self::rival_shield_token_tick`] /
        // [`Self::rival_rebound_token_tick`], both encodings), which
        // own the counter, the owner's 0x4000/0x8000 bits and the
        // regen pin. The planner flags just mirror the live bursts.
        let shield = self
            .rival_token(ri, 4)
            .is_some_and(|m| self.g.ent[m].f26 > 0);
        let rebound = self
            .rival_token(ri, 14)
            .is_some_and(|m| self.g.ent[m].f26 > 0);
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
        // ⚠ The speed-up (2) burst is NOT decremented here. Retail
        // winds it down inside the token's OWN handler at the token's
        // pool slot ([`Self::rival_speed_token_tick`], :65190), which
        // is where the v_14 kill and the speed override live too —
        // clocking it from the wizard's slot ran the counter a full
        // pass early and skipped every other thing that handler does.
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
    }

    /// Incoming-projectile defense (sub_16800 :19769 + sub_16870/90):
    /// the nearest class-9 homing on me within 5120 → lateral jink 80 +
    /// a reactive cast (models {0,3,16} → 14 Rebound, {4,9} → 4
    /// Shield).
    ///
    /// ⭐ THE SCAN WALKS THE TICK-TOP CLASS-9 ROSTER
    /// (`var_u32_36462[3]`, :19777), not the pool — membership was
    /// sampled at the tick head with NO life or flags test, so a ball
    /// born mid-tick is not yet a threat (mc1l4 t=5377: the pelting
    /// stream's newborn must not trigger a dodge until next tick) and
    /// a soft-killed one still is. Both range gates are STRICT
    /// (`>= 0x1900000` rejects, the cast wants `< 0x100000`).
    fn rival_defense(&mut self, ri: usize, i: usize) {
        let me = self.rivals[ri].ent;
        let (px, py, pz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        // ⭐ THE RANGE IS 2D. Both this scan's 5120 gate and the
        // reactive-cast's 1024 gate measure through `sub_42410`
        // (:52748-54) = (Δx)² + (Δy)² — NO z term. The port's
        // invented dz² leg dropped a high bolt out of dodge range
        // one tick early (mc1hwl0 t=16771: threat 516 at dz 2617
        // reads 27.7M in 3D against the 26.2M gate, 20.9M in
        // retail's 2D — retail re-stamps the strafe, the port let
        // it decay, and the 4-unit lateral gap is the t=16772
        // x,y head).
        let mut best: Option<(usize, i32)> = None;
        for k in 0..self.g.proj_chain.visible_len() {
            let j = self.g.proj_chain.list[k] as usize;
            let e = &self.g.ent[j];
            if e.f146 != me {
                continue;
            }
            let d2 = Gen::dist2_sq(px, py, e.x, e.y);
            if d2 < 5120 * 5120 && best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        if std::env::var_os("MGC_JINK_TRACE").is_some() {
            let cand: Vec<(u16, u16, u16, u16, i16, i32)> = (0..self.g.proj_chain.visible_len())
                .map(|k| {
                    let j = self.g.proj_chain.list[k] as usize;
                    let e = &self.g.ent[j];
                    (
                        j as u16,
                        e.f146,
                        e.x,
                        e.y,
                        e.z,
                        Gen::dist2_sq(px, py, e.x, e.y),
                    )
                })
                .collect();
            eprintln!(
                "[jink] t={} ri={ri} me={me} scan best={best:?} cand={cand:?} me_pos=({px},{py},{pz}) jink_pre={}",
                crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                self.rivals[ri].jink
            );
        }
        let Some((threat, d3)) = best else { return };
        self.rivals[ri].jink = 80;
        if d3 < 1024 * 1024 {
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
        let trace = std::env::var_os("MGC_RIVAL_TRACE").is_some();
        if trace {
            let t = crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed);
            let castle = self.rival_castle(self.rivals[ri].ent);
            eprintln!(
                "[rsel t={t}] ri={ri} state={:?} think={think} castle={castle:?} known16={} mana_max={} mana={} target={} f63={}",
                self.rivals[ri].state,
                self.rivals[ri].known[16],
                self.rivals[ri].mana_max,
                self.rivals[ri].mana,
                self.rivals[ri].target,
                self.g.ent[i].f63,
            );
        }
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
        // 2. Flee home hurt (sub_14310 :18480). ⭐ The PREDICATE
        // writes the target itself (:18489-90 — `+146` = the
        // established castle from wizext+50, `+148` = its signature);
        // this transition is NOT targetless.
        if let Some(c) = castle
            && self.g.ent[i].act_life < (self.g.ent[i].max_life / 2) as i32
        {
            self.set_rival_state(ri, AiState::Home, c as u16);
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
                // ⭐ sub_14120 :18432-33 — the predicate stamps the
                // castle into `+146`/`+148` on its way to returning 1.
                self.set_rival_state(ri, AiState::Upgrade, c as u16);
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
        // 6. Intercept a fat enemy balloon (sub_147E0 :18596). The
        // pick opens on the same offense gate as the castle/wizard
        // arms (:18611 `sub_16920` alone — no castle-capable clause):
        // a disarmed wizard can't raid, so a razed, token-scattered
        // rival falls straight through to the ball claim (mc1l5
        // t=19577: Vodor abandons the human's fat balloon for the
        // wild ball the possess arm prices against his razed token).
        if self.rival_has_offense(ri) && self.rival_pick_balloon_target(ri, i) {
            self.rivals[ri].state = AiState::RaidBalloon;
            return;
        }
        // 7. Claim mana balls (sub_14230 :18439-52): needs spell 3;
        // with the castle spell owned, only while the ceiling sits at
        // or under the TOKEN's LIVE +136 price cache (:18452 reads
        // `wiz +136 <= manifestation +136` — sub_47DD0's stamp:
        // CAP[level] housed, 1000 ctor, 5000 after a raze), so
        // claiming re-opens after every upgrade AND while razed.
        // mc1l5 t=16081: castle-less Vodor at ceiling 1768 re-picks
        // the wild 2000-mana ball (Possess, target 553) against his
        // razed token's 5000 — the port's static-cost stand-in
        // (1768 > 1000) kept him parked on a freed balloon slot.
        let m16 = self.rivals[ri].owned[16] as usize;
        let claim_open = m16 == 0 || self.rivals[ri].mana_max <= self.g.ent[m16].f136.max(0) as u32;
        if self.rivals[ri].known[3] && claim_open && self.rival_pick_ball_target(ri, i) {
            self.rivals[ri].state = AiState::Possess;
            return;
        }
        // 8. Hunt any mana holder (sub_14B10 :18650).
        if self.rival_pick_mana_target(ri, i) {
            self.rivals[ri].state = AiState::HuntMana;
            return;
        }
        // 9. Idle (sub_14DC0 :18749). ⭐ The HOME leg stamps the
        // castle (:18760-61); only the CRUISE leg (:18756) writes
        // nothing but the brain byte.
        if let Some(c) = castle
            && self.g.ent[i].act_life < self.g.ent[i].max_life as i32
        {
            self.set_rival_state(ri, AiState::Home, c as u16);
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
        spell_list: &[i32; SPELL_COUNT],
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
        // The +532 acquisition list rides the record verbatim — the
        // death scatter and the respawn re-grant both iterate it in
        // place, so its ORDER (pickup history, unrecoverable from the
        // +676 book) is state.
        r.acq = *spell_list;
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
        // corpus grades the column. ⚠ HOME AND UPGRADE ARE NOT
        // TARGETLESS: their selector PREDICATES stamp the established
        // castle themselves (`sub_14310` :18489-90, `sub_14120`
        // :18432-33, `sub_14DC0`'s home leg :18760-61), all three off
        // wizext+50. The only genuinely targetless transition is
        // Build (`sub_13F00`) and Idle's CRUISE leg (:18756), which
        // touch `+415` alone. mc1l5 t=933 is the exemplar: Vodor's
        // upgrade predicate fires and retail re-points him from the
        // mana ball 681 at his own keep 680 (`+148` 2000 -> 1061),
        // where the port left the ball standing.
        if target != 0 {
            let ent = self.rivals[ri].ent as usize;
            self.g.ent[ent].f146 = target;
        }
    }

    /// Target signature (sub_15420 :19039): team + model + class<<7.
    ///
    /// ⭐⭐ THE TEAM WORD IS RETAIL'S OWNER **SLOT**, AND THE PORT DOES
    /// NOT STORE SLOTS THERE. Every imported `+24` runs through the
    /// importer's `tr()`, which rewrites the human carpet's slot to
    /// the [`PLAYER_TARGET`] sentinel (the human is not a pool record
    /// in native play). The stored `+148` imports RAW — retail's own
    /// arithmetic off slot 472 — so a port-side recompute over a
    /// HUMAN-OWNED target read 0xFFFF where retail read 472 and the
    /// staleness test refused a target that was perfectly alive. The
    /// handler then returned with NO WRITES, which is invisible to
    /// pair mode as anything but a one-tick-stale lane
    /// (mc1hwl0 t=1902: rival 0 raids the human's castle 785,
    /// stored sig 858 = 472+2+(3<<7) against a recompute of 385
    /// = 0xFFFF+2+384 wrapped, and `+34` froze at the imported value
    /// for the rest of the take).
    ///
    /// Resolving the sentinel back to the imported carpet slot puts
    /// the recompute in retail's numbering. A NATIVE world has no
    /// pooled carpet (`mc1_carpet_slot` 0); there the sentinel stays
    /// and the arithmetic is merely self-consistent, which is all it
    /// ever has to be — nothing imported can disagree with it.
    fn target_sig(&self, target: u16) -> u16 {
        if target == 0 {
            return 0;
        }
        if target == PLAYER_TARGET {
            return PLAYER_TARGET;
        }
        let e = &self.g.ent[target as usize];
        let team = if e.id24 == PLAYER_TARGET && self.mc1_carpet_slot != 0 {
            self.mc1_carpet_slot
        } else {
            e.id24
        };
        team.wrapping_add(e.model65 as u16)
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
        // ⭐⭐ The home supercell derives from the wizard's x/y read
        // as SIGNED i16, divided by 16384 TRUNCATING toward zero
        // (:18362-67's CFSHL signed-division idiom) — the movsx
        // class again. A wizard in the upper half of the wrap
        // (x >= 0x8000 → negative i16) starts the walk at cell 0 or
        // 3-from-truncation, NOT `u16 >> 14`: mc1l5 t=14694, Vodor
        // rebuilds at x=65333 (i16 −203 → cell 0) and retail's
        // FIRST candidate is (0,0) — accepted at once (the human's
        // castle wraps to Chebyshev 31232) — where the port's
        // `>> 14` began at cell 3 and planted a map-quadrant away.
        let cx0 = (sx as i16 / 16384) as i32;
        let cy0 = (sy as i16 / 16384) as i32;
        for dy in 0..4i32 {
            let by = ((((cy0 + dy) & 3) as u16) << 14) as u16;
            for dx in 0..4i32 {
                let bx = ((((cx0 + dx) & 3) as u16) << 14) as u16;
                for (ox, oy) in [(0u16, 0u16), (0x1F00, 0x1F00)] {
                    let (tx, ty) = (bx.wrapping_add(ox), by.wrapping_add(oy));
                    // Retail walks the candidates THROUGH THE SCRATCH
                    // RECORD — v1 is pool slot 0 and each probe writes
                    // its x/y (v1[36]/v1[37], :18374-75/:18385-86); the
                    // scratch keeps the last probed candidate after the
                    // scout ends (the parting-shot family reads it).
                    // Raw field writes: slot 0 is parked, never linked.
                    self.g.ent[0].x = tx;
                    self.g.ent[0].y = ty;
                    // The foreign castle nearest this candidate by
                    // toroidal squared-Euclidean — ⭐ sub_15260 WALKS
                    // THE TICK-TOP WIZ CHAIN (bucket[0]), per-node
                    // gates `+24 != mine && +65 == 2` and NOTHING
                    // else: no 0x400 test, no life re-test (the
                    // chain build is the life test). A husk
                    // soft-killed MID-TICK still vetoes a candidate
                    // (mc1l5 t=14694: rebuilding after the razed
                    // keep, retail's scan rejects the first two
                    // supercell candidates and plants at (0,0) where
                    // the port's 0x400-skipping pool scan took the
                    // first — Vodor flew off 292° instead of 179°).
                    let mut near_xy: Option<(u16, u16)> = None;
                    let mut near_d2 = i32::MAX;
                    for c in 0..self.g.wiz_chain.visible_len() {
                        let j = self.g.wiz_chain.list[c] as usize;
                        let e = &self.g.ent[j];
                        if e.model65 == 2 && e.id24 != me {
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
                    if std::env::var_os("MGC_RIVAL_TRACE").is_some() {
                        eprintln!(
                            "[scout t={}] cand=({tx},{ty}) near={near_xy:?} ok={ok}",
                            crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                        );
                    }
                    if ok {
                        self.rivals[ri].site = (tx, ty);
                        // The accept stamps the wizard's own site
                        // triple (:18381-83): +150/+152 = the winning
                        // candidate, +154 = the SCRATCH record's z —
                        // which the scout never writes, so the site
                        // datum is whatever z slot 0 carries. The
                        // Build hover (:18160-66) steers toward it.
                        let sz = self.g.ent[0].z;
                        let e = &mut self.g.ent[i];
                        e.dest_x = tx;
                        e.dest_y = ty;
                        e.site_z = sz;
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
    ///
    /// ⭐⭐ IT WALKS THE TICK-TOP CLASS-3 CHAIN (`var_u32_36462[0]`
    /// :18507), NOT THE POOL — the same roster the wizard and balloon
    /// picks already use, and **membership IS the liveness filter**
    /// (`actLife >= 0 && !(flags & 0x10)`, sampled once at the tick
    /// top). The per-node tests are only `+24 != mine` and `+65 == 2`;
    /// there is no class test (the chain is class-3) and no `0x400`
    /// test (the chain build's life gate has already run).
    ///
    /// mc1hwl0 t=1920 is the exemplar and it is a DEAD CASTLE that
    /// still stands: the human's keep 785 takes its fatal hit at
    /// t=1920 (`act_life` 20000 → −2000, `+70` 4 → 6) but is not
    /// reap-flagged until t=1921, so a pool scan filtered on `0x400`
    /// still saw it, `poorer` still held (140 stored against the
    /// rival's own 4490), and the port re-elected a corpse it could
    /// never raid. Retail's chain had dropped it at the tick top, so
    /// its cascade fell straight through to the ball claim: `+415`
    /// 7 → 6, `+146` 785 → the mana ball 356.
    ///
    /// ⭐ And the range gate is the ELECTION WINNER's alone
    /// (:18531-34, strict `>=` → reject) — the same shape as the
    /// wizard pick. Filtering by range inside the election lets a
    /// far-but-nearest castle be replaced by a farther-ranked one.
    fn rival_pick_castle_target(&mut self, ri: usize, i: usize) -> bool {
        let me = self.rivals[ri].ent;
        let my_castle = self.rival_castle(me);
        let my_stored = my_castle.map_or(0, |c| self.g.ent[c].f140.max(0) as u32);
        // Skip while castle-less but castle-capable (:18507).
        if my_castle.is_none() && self.rivals[ri].known[16] {
            return false;
        }
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let mut best: Option<(u16, i32)> = None;
        for c in 0..self.g.wiz_chain.visible_len() {
            let j = self.g.wiz_chain.list[c] as usize;
            let e = &self.g.ent[j];
            if e.model65 != 2 || e.id24 == me {
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
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j as u16, d));
            }
        }
        // The winner alone faces the range gate (:18531-34).
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32;
        let best = best.filter(|&(_, d)| d < range.saturating_mul(range));
        if let Some((t, _)) = best {
            self.set_rival_state(ri, AiState::RaidCastle, t);
            true
        } else {
            false
        }
    }

    /// wizext+50, the ESTABLISHED-castle register: written only by the
    /// level-up commit (:56484), cleared at removal (:56534). The
    /// port's stand-in is the standing castle's FIRST-COMMIT latch
    /// (flags bit 1, :56057-62) — the same gate the token ladder stamp
    /// uses. An authored castle that has never leveled is UNBOUND.
    fn castle_bound(&self, castle: Option<usize>) -> bool {
        castle.is_some_and(|c| self.g.ent[c].flags & 2 != 0)
    }

    /// Enemy-wizard pick (sub_145B0 :18541-91), walking the tick-top
    /// wiz chain (candidates: class-3 carpets, `+65 <= 1`, not self,
    /// not spell-12-cloaked — and nothing else per node; liveness is
    /// the chain build's).
    ///
    /// ⭐⭐ THE WAR ARM SHORT-CIRCUITS (:18563-68): the FIRST chain
    /// candidate whose war flag is set is stamped as the target and
    /// the pick returns — NO nearest election, NO range test. Only
    /// hated/bully candidates enter the distance election, and the
    /// range gate (`v_28 + 10`, strict `<`) applies to that election's
    /// winner alone (:18585-87). mc1l5 t=11341: the human shells
    /// Vodor's castle from ~14,200 units out (far past 8192+10); war
    /// latches at t=11308 and retail still retargets him RANGELESSLY
    /// at his next think tick, where an in-election range test keeps
    /// him on a bee.
    ///
    /// ⭐ The hated threshold reads the CANDIDATE ENTITY's `+136`
    /// ceiling lane, inclusive (`50000 − agg·(f136/10)/255 <= hate`,
    /// :18570); the port reads the live ceiling mirrors those lanes
    /// track. ⭐ The bully arm (:18571-77) wants an UNBOUND candidate
    /// (`wizext+50 == 0`) that KNOWS the castle spell and is poorer by
    /// `+140` — not merely castle-less. ⭐ The leading self-test
    /// (:18549): a castle-capable rival whose own castle is UNBOUND
    /// returns 0 — while it could be building, it does not hunt
    /// wizards.
    fn rival_pick_wizard_target(&mut self, ri: usize, i: usize) -> bool {
        if self.rivals[ri].known[16] && !self.castle_bound(self.rival_castle(self.rivals[ri].ent)) {
            return false;
        }
        let me = self.rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let my_agg = self.rivals[ri].agg as i64;
        let my_mana = self.rivals[ri].mana as i64;
        // Candidates in CHAIN order. The human's carpet is never a
        // pool record in the port (imports anchor the human at the
        // walk slot without materializing the entity), so the human
        // is judged as a pre-pass — retail's chain order puts his
        // carpet below the rivals' in every corpus take.
        let mut war_pick: Option<u16> = None;
        let mut best: Option<(u16, i32)> = None;
        let judge = |tgt: u16,
                     x: u16,
                     y: u16,
                     invisible: bool,
                     ceiling: i64,
                     mana: i64,
                     unbound_knows16: bool,
                     hate: i64,
                     war: bool,
                     best: &mut Option<(u16, i32)>|
         -> bool {
            if invisible {
                return false; // spell-12 targets are skipped (:18558)
            }
            if war {
                return true; // ⭐⭐ first-in-chain, rangeless (:18563-68)
            }
            let hated = 50_000 - my_agg * (ceiling.max(0) / 10) / 255 <= hate;
            let bully = unbound_knows16 && mana + 32 * (255 - my_agg) < my_mana;
            if hated || bully {
                let d = Gen::dist2_sq(px, py, x, y);
                if best.is_none_or(|(_, bd)| d < bd) {
                    *best = Some((tgt, d));
                }
            }
            false
        };
        // Candidacy is TICK-TOP bucket[0] membership, not live state
        // (mc1hwl0 t=7592: the rival's own kill lands before its
        // selector runs, and retail still picks the corpse).
        if self.human_bucket_alive
            && judge(
                PLAYER_TARGET,
                self.human_pose.0,
                self.human_pose.1,
                self.player.invisible,
                self.player.mana_max as i64,
                self.player.mana as i64,
                !self.castle_bound(self.player_castle()) && self.player.owned[16] != 0,
                self.rivals[ri].hate[0] as i64,
                self.rivals[ri].war[0],
                &mut best,
            )
        {
            war_pick = Some(PLAYER_TARGET);
        }
        if war_pick.is_none() {
            for c in 0..self.g.wiz_chain.visible_len() {
                let j = self.g.wiz_chain.list[c] as usize;
                let e = &self.g.ent[j];
                if e.model65 != 1 || e.id24 == me {
                    continue;
                }
                let Some(oj) = self.rivals.iter().position(|r| r.ent as usize == j) else {
                    continue;
                };
                if self.rivals[oj].eliminated {
                    continue;
                }
                let o = &self.rivals[oj];
                let oslot = o.slot;
                if judge(
                    o.ent,
                    e.x,
                    e.y,
                    o.invisible,
                    o.mana_max as i64,
                    o.mana as i64,
                    !self.castle_bound(self.rival_castle(o.ent)) && o.known[16],
                    self.rivals[ri].hate[oslot as usize] as i64,
                    self.rivals[ri].war[oslot as usize],
                    &mut best,
                ) {
                    war_pick = Some(j as u16);
                    break;
                }
            }
        }
        if let Some(t) = war_pick {
            self.set_rival_state(ri, AiState::AttackWizard, t);
            return true;
        }
        // The range gate applies to the ELECTION winner only, strict
        // (:18585-87: `d² >= (v_28+10)² → return 0`).
        let Some((t, d)) = best else {
            return false;
        };
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32 + 10;
        if d >= range.saturating_mul(range) {
            return false;
        }
        self.set_rival_state(ri, AiState::AttackWizard, t);
        true
    }

    /// Enemy-balloon pick (sub_147E0 :18596-645): a walk of the
    /// TICK-TOP class-3 chain (`var_u32_36462[0]` :18615 — no life or
    /// 0x400 test, the bucket[0] family) for foreign model-3s whose
    /// owner is hated (live wealth-scaled, the owner ENTITY's +136
    /// through id24), cargo over 10*(275-agg), and NOT at home — where
    /// "at home" is `sub_11950` = the FULL summed-extents AABB vs the
    /// owner's BOUND castle (wizext+50 :18628; unbound reads slot 0,
    /// the scratch). ⚠ NOT a distance disc: the human's level-6
    /// castle carries 6784-unit extents, so its balloons are exempt
    /// nearly 7000 out (mc1l5 t=19577 — balloon 900 at dx 5852 is
    /// docked, Vodor falls through to the ball claim). The range gate
    /// applies to the ELECTION WINNER only, strict (:18638-40
    /// `d² >= v_28² → return 0` — no fallback to the runner-up).
    fn rival_pick_balloon_target(&mut self, ri: usize, i: usize) -> bool {
        let me = self.rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let cargo_gate = 10 * (275 - self.rivals[ri].agg as u32);
        let mut best: Option<(usize, i32)> = None;
        for c in 0..self.g.wiz_chain.visible_len() {
            let j = self.g.wiz_chain.list[c] as usize;
            let e = &self.g.ent[j];
            if e.model65 != 3 || e.id24 == me {
                continue;
            }
            let owner_ent = e.id24;
            let Some(owner) = self.owner_slot(owner_ent) else {
                continue;
            };
            if !self.hate_over(ri, owner, self.wizard_wealth(owner)) {
                continue;
            }
            if (self.g.ent[j].f140.max(0) as u32) <= cargo_gate {
                continue;
            }
            let home = self.g.castle_reg[owner as usize & 7] as usize;
            if self.g.ent_overlap(j, home) {
                continue;
            }
            let d = Gen::dist2_sq(px, py, self.g.ent[j].x, self.g.ent[j].y);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j, d));
            }
        }
        let Some((t, _)) = best else {
            return false;
        };
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32;
        let d = Gen::dist2_sq(px, py, self.g.ent[t].x, self.g.ent[t].y);
        if d >= range.saturating_mul(range) {
            return false;
        }
        self.set_rival_state(ri, AiState::RaidBalloon, t as u16);
        true
    }

    /// Mana-ball pick (sub_15080 :18862): wild balls by distance;
    /// at-war owners' balls; neutral-owned only if unguarded.
    fn rival_pick_ball_target(&mut self, ri: usize, i: usize) -> bool {
        let me = self.rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let mut best: Option<(u16, i32)> = None;
        // ⭐ THE PICK WALKS THE TICK-TOP BALL CHAIN, NOT THE POOL.
        // `sub_15080` (:18878) seeds from `var_u32_36462[1]` — the
        // ball roster the tick head rebuilt at :52290-97 before any
        // handler ran — and follows `+0` links to the end (:18919).
        // A ball MINTED MID-TICK is therefore invisible to every
        // rival brain until the next rebuild, and that is exactly
        // what mc1l4 t=257 turns on: the human's castle is torn down
        // that tick and ejects five balls into free slots, one of
        // them slot 32, and the port's pool sweep saw it while retail
        // could not — so retail's cascade found no eligible ball at
        // all, fell through to the mana hunt, and RE-PICKED creature
        // 85 (`+146` 85 and `+148` 728 both unchanged across the
        // boundary), while the port re-pointed at the newborn 32.
        // ⚠ Neither the model test nor the 0x400 test survives the
        // move: the chain build has no life or flag filter and admits
        // models 39 AND 40, and `sub_15080` adds no model test of its
        // own — membership IS the filter.
        // The at-war arm scores from the BOUND castle REGISTER's
        // entity (:18879 — `v9 = pool + 164·wizext+50`), which for an
        // UNBOUND rival is pool slot 0: the SCRATCH record, whose x/y
        // are live state (the scout walks its candidates through it).
        let reg = self.g.castle_reg[self.rivals[ri].slot as usize] as usize;
        let (rx, ry, reg_id) = {
            let e = &self.g.ent[reg];
            (e.x, e.y, e.id24)
        };
        let trace = std::env::var_os("MGC_RIVAL_TRACE").is_some();
        for c in 0..self.g.ball_chain.visible_len() {
            let j = self.g.ball_chain.list[c] as usize;
            let (bx, by, bid, tag) = {
                let e = &self.g.ent[j];
                (e.x, e.y, e.id24, e.f144)
            };
            if trace {
                eprintln!(
                    "[bpick t={}] ri={ri} ball={j} tag={tag} best={best:?}",
                    crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                );
            }
            // (:18884) the TAG'S ENTITY decides the arm: a tag whose
            // record is not class 3 (the wild 0 included — slot 0 is
            // the scratch) takes the ungated wild arm, scored from
            // ME. PLAYER_TARGET is the port's human tag.
            let owner_is_wiz = tag == PLAYER_TARGET
                || self.g.ent.get(tag as usize).is_some_and(|o| o.class64 == 3);
            if !owner_is_wiz {
                let d = Gen::dist2_sq(px, py, bx, by);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j as u16, d));
                }
                continue;
            }
            if tag == me {
                continue; // already mine (:18899)
            }
            // (:18886-90) ⭐ the war test here is the LIVE
            // wealth-scaled hate formula off the owner's ceiling —
            // the latched war[] flag is the castle sweep's lane and
            // is NOT read (mc1l5 t=16081: Vodor's flag vs the human
            // is still latched while the live hate has decayed out,
            // so retail claims the wild 2000-ball where the
            // flag-reading port chased a human-owned one). At-war
            // balls score from the REGISTER's entity, not from me.
            // (A class-3 non-wizard tag has no team; retail reads a
            // team byte through the null wizext — fall through to
            // the neutral arm until a corpus exemplar rules.)
            let team = self.owner_slot(tag);
            if let Some(o) = team
                && self.hate_over(ri, o, self.wizard_wealth(o))
            {
                let d = Gen::dist2_sq(rx, ry, bx, by);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j as u16, d));
                }
                continue;
            }
            // Neutral-owned (:18905-16): the guard is the nearest
            // FOREIGN CARPET to the BALL on the tick-top wiz chain
            // (sub_15340 — model <= 1, excluding the ball's id24 and
            // me; NO carpet at all → no take), 5120 gate; plus the
            // castle-overlap veto (sub_153B0 bound / sub_15260
            // unbound: the nearest castle to the ball, excluding the
            // ball's id24 — and, bound, the register's own castle).
            // ⚠ The HUMAN's carpet rides OUT-OF-POOL in the port, so
            // the tick-top wiz chain never holds it — retail's chain
            // always does, and its nearest-carpet guard is exactly
            // what admits a human-claimed ball the human has wandered
            // 5120+ away from (mc1l5 t=253: ball 362 at 5,900 units).
            // Weigh the live human by pose before the chain walk.
            let mut guard: Option<i32> = self
                .wizard_pos(0)
                .map(|(hx, hy, _)| Gen::dist2_sq(bx, by, hx, hy));
            let mut castle: Option<(usize, i32)> = None;
            for k in 0..self.g.wiz_chain.visible_len() {
                let w = self.g.wiz_chain.list[k] as usize;
                let we = &self.g.ent[w];
                if we.id24 == bid {
                    continue;
                }
                let d = Gen::dist2_sq(bx, by, we.x, we.y);
                if we.model65 <= 1 && we.id24 != me && guard.is_none_or(|gd| d < gd) {
                    guard = Some(d);
                }
                if we.model65 == 2
                    && (reg == 0 || we.id24 != reg_id)
                    && castle.as_ref().is_none_or(|&(_, cd)| d < cd)
                {
                    castle = Some((w, d));
                }
            }
            let unguarded = guard.is_some_and(|gd| gd > 5120 * 5120);
            let housed = castle.is_some_and(|(cs, _)| self.g.ent_overlap(j, cs));
            if unguarded && !housed {
                let d = Gen::dist2_sq(px, py, bx, by);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j as u16, d));
                }
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
        // ⭐⭐ A STALE TARGET DOES NOT RESET THE STATE. Every combat
        // handler opens on the sig-vs-stored test and returns 0 with
        // NO writes when it fails (sub_13BA0 :18246, sub_13CA0
        // :18281, sub_13DD0 :18323) — the state byte and the target
        // KEEP, the handler simply no-ops until the think-tick
        // cascade replaces the state. There is NO Fresh transition
        // anywhere in the retail machine. The port's old
        // drop-to-Fresh prologue re-entered the cascade off-cadence:
        // mc1l5 t=12158 — Vodor's claimed ball 908 is collected and
        // its slot re-minted, retail idles in Possess for 450 ticks
        // (the cascade refusing every think round) while the port's
        // Fresh re-pick went hunting, and 1,500 ticks later its
        // Upgrade chain fired a castle ball retail never cast
        // (t=13647, the extra (9,10)/(10,43) pair).
        let needs_target = matches!(
            self.rivals[ri].state,
            AiState::Possess
                | AiState::RaidCastle
                | AiState::AttackWizard
                | AiState::RaidBalloon
                | AiState::HuntMana
        );
        if needs_target && !self.target_alive(self.rivals[ri].target, self.rivals[ri].target_sig) {
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
                    // :18120-27 — a FIRED cast tick returns without
                    // hovering (same shape as RaidCastle); the z-hover
                    // toward castle+512 is the refused arm's alone.
                    if !self.rival_cast(ri, i, 16) {
                        self.rival_hover_toward(i, cz.saturating_add(512));
                    }
                }
            }
            // Fly to the scouted site; plant (sub_138F0 :18142-68).
            // ⭐ NO state write — retail's handler aims, arrives and
            // casts, nothing else; the state leaves only through the
            // think-tick cascade. The old plant→Fresh invention cost
            // the follow-up: retail's Build handler runs AGAIN the
            // tick after the plant, still arrived, and its cast-16
            // now takes the BOUND arm — arming the upgrade token on
            // the day-old castle (mc1l5 t=14772: the (9,10) ball the
            // port never fired).
            AiState::Build => {
                let (sx, sy) = self.rivals[ri].site;
                if self.rival_approach(ri, i, sx, sy, None, 2048, 3072)
                    && !self.rival_cast(ri, i, 16)
                {
                    // :18160-66 — the REFUSED cast's z-hover toward the
                    // scouted site datum (+154) + 512. The datum is the
                    // SCRATCH record's z at scout-accept time (see
                    // rival_scout_site), so a parked Vodor rides the
                    // settle floor and this nudge in alternation:
                    // mc1l5 t=14772-800, retail 975 = floor 979 − 4.
                    let sz = self.g.ent[i].site_z;
                    self.rival_hover_toward(i, sz.saturating_add(512));
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
                    // ⚠ THE CLAIM CONE IS STRICT: `< 0x1Cu` (:18254),
                    // not `<= 28`. mc1l3 t=447 lands on the boundary
                    // exactly — Vodor (slot 585) sits at `+30 = 1082`
                    // with ball 105 bearing 1054, an angular distance
                    // of precisely 28 — so retail refuses the claim
                    // and the port took it. The ball's `+144` is an
                    // UNGRADED lane, so no pair diff can see it; it
                    // surfaces one tick later through the mana census,
                    // which credits that ball's 512 to the rival's
                    // ceiling: `mana_max` retail 3048, port 3560.
                    if cast && facing < 28 {
                        self.g.ent[t].f144 = self.rivals[ri].ent;
                        // Settled balls never re-run the tick's
                        // re-derive — recolor at the claim. BALLS
                        // ONLY: the re-derive this stands in for is
                        // the (10,39) tick's own (sub_274D0), which a
                        // claimed GRAVE never runs — retail's grave
                        // keeps sprite 65 (+78 100) after a rival
                        // claim, and the recolored port grave's
                        // +78 25 skewed every acquire measured at its
                        // aim-z (mc1hwl0 t=7853: the human's claim
                        // bolt pitches 2046 against retail's 2041 and
                        // the whole flight parts by 7 z-units).
                        if self.g.ent[t].model65 == 39 {
                            self.g.ball_resize(t);
                        }
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
    /// castle-less Home arm sub_13A70 :18213-22): an ACTIVE speed
    /// burst owns the speed columns (sub_15E60's +48 test — vdes
    /// untouched); else the AI chain-casts the speed-up whenever
    /// ready, else full throttle.
    ///
    /// ⚠ NEITHER twin CLEARS `v_14` — only `sub_15470` does (:19057).
    /// The plain-throttle leg SETS it (:18198 / :18221), so a rival
    /// cruising on this arm re-arms the latch every tick it is not
    /// boosting, and the moment it does cast the burst the arm stops
    /// running entirely (the `sub_15E60` early return) and the latch
    /// keeps whatever the cast tick left.
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
            self.rivals[ri].v14 = true;
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
        // The speed-column latch clears at the HEAD (:19057), before
        // any leg decides — so a tick that returns through the
        // boost-active or boost-cast arms leaves it CLEAR and the
        // running burst survives.
        self.rivals[ri].v14 = false;
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
            self.rivals[ri].v14 = true; // :19075-76 — the arrival stop
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
        self.rivals[ri].v14 = true; // :19089-91 — the plain throttle
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
        // ⭐⭐ Castle (case 0x10 :19304-42): the BOUND arm alone
        // re-tests the upgrade SPACE (`sub_12D10` on the wizext+50
        // castle) and the SAME accuracy cone as the aimed groups;
        // the free-plant arm (:19343-47) is cooldown + mana only.
        // mc1l5 t=13646: Vodor settles into Upgrade over his rebuilt
        // keep with an aim error of 154 against a cone of ~130 —
        // retail hovers and re-aims (charge climbing 125→126) where
        // the port's coneless commit armed the token and fired the
        // castle ball retail never cast (the t=13647 extra
        // (9,10)/(10,43) pair).
        if s == 16
            && let Some(c) = self
                .rival_castle(r.ent)
                .filter(|&c| self.g.ent[c].flags & 2 != 0)
        {
            let space = self.g.castle_upgrade_space_ok(c);
            let cone = ((255 - r.acc as u32) / 4 + 20) * 2048 / 360;
            let e = &self.g.ent[r.ent as usize];
            let aim = Gen::angdist(e.f30, e.f34 & 0x7FF) as u32;
            if std::env::var_os("MGC_RIVAL_TRACE").is_some() {
                eprintln!(
                    "[cast16 t={}] castle={c} space={space} aim={aim} cone={cone}",
                    crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                );
            }
            if !space || aim >= cone {
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
        // Absolute aim pitch to the target (:19125-27 / :19168-71):
        // the commit stamps the WIZARD's own +32 — the token-side
        // spawner reads the pose (and the corpus grades the column).
        //
        // ⭐ THIS SITS ABOVE THE CASTLE BAIL BECAUSE RETAIL HAS NO
        // CASTLE TEST IN `sub_155F0` AT ALL. Both commit arms run
        // cooldown → +32 → token `+48 = +50` with nothing between
        // (:19124-27 and :19172-76); the unlock ladder is the
        // PROJECTILE's own first tick (:65049), 40k lines away. The
        // collapse below is a legitimate shortcut for the emission,
        // but it inherited a stamp that retail performs BEFORE the
        // fizzle can matter — so a rival whose castle stores nothing
        // stopped aiming altogether. mc1hwl0-noskip t=359: rival 1's
        // castle (slot 522) holds `f140` 0 against Lightning's
        // castle_req, and its `+32` froze at the imported value for
        // the whole take.
        let dh = Gen::isqrt(Gen::dist2_sq(ex, ey, tx, ty) as u32) as i32;
        let pitch = Gen::pitch_toward(ez, tz, dh);
        if matches!(s, 0 | 3 | 7 | 8 | 11 | 13 | 15 | 17 | 20) {
            self.g.ent[i].f32 = pitch;
        }
        // ⭐⭐⭐ THE CASTLE-STORED LADDER IS NOT A COMMIT GATE — IT IS
        // THE TOKEN'S OWN (`sub_55DD0` :64917-19, reached from the
        // token handler `sub_56090` :65030-88, and separately from the
        // projectile's first tick :65049). `sub_155F0` runs
        // cooldown → `+32` → `+48 = +50` with NOTHING between
        // (:19124-27, :19172-76), so a rival whose castle stores
        // nothing still ARMS, and the burst dies one tick later when
        // the token's gate refuses and drops it to 1 for the shared
        // decrement. The port used to bail here instead, which ate
        // the arm exactly as an earlier version of the same bail ate
        // the `+32` stamp above — ⭐ *a collapse inherits every write
        // retail does before the point it collapsed to, and that
        // ledger has to be re-read every time the collapse moves*.
        // mc1hwl0 t=2425: rival 1's Wall of Fire token 256 reads
        // `+48` 0 → **26** → 0 across t=2425/2426/2427, a one-tick
        // spike the collapsed port flattened to a permanent 0 (98
        // rows over 62 pairs). Its castle 522 stores 4,490 against
        // HW spell 20's 60,000 requirement — it never fires, and
        // retail still counts.
        // The refusal stays SILENT for the AI (retail's buzz 29 is
        // the local player's channel and would storm at Lightning's
        // 1-tick recast).
        //
        // The commit only ARMS the token (+48 = +50, through the
        // VALIDATED binding) — the bolt, the sub_55E80 debit and the
        // mid-burst regen freeze all run at the TOKEN's own pool slot
        // ([`World::rival_manifestation_tick`], retail's sub_56090
        // machine). A token below the caster fires next pass, above
        // it the same tick — retail's phase for free.
        if let Some(m) = self.rival_token(ri, s) {
            self.g.ent[m].f26 = def.count as i16;
        }
        let _ = (ex, ey, ez, yaw);
        true
    }

    /// Rival castle cast (:19190-209).
    fn rival_cast_castle(&mut self, ri: usize, i: usize) -> bool {
        // The commit's ONE gate for both arms (:19191-93): the token
        // exists, is NOT busy (`+48 != 0` — a cast in transit, the
        // charge pin), and the wizard's CURRENT mana covers the live
        // ladder price (wiz +140 vs the token's +136 stamp). Retail
        // has NO space test at the commit — that's the selector's
        // (:18408); and the recast cooldown is the UPGRADE arm's
        // alone (:19197), the free plant leaves it untouched.
        let Some(m) = self.rival_token(ri, 16) else {
            return false;
        };
        if self.g.ent[m].f26 != 0 || self.rivals[ri].mana < self.rival_castle_price(ri) {
            return false;
        }
        if self.rival_castle(self.rivals[ri].ent).is_some() {
            // Established castle → THE UPGRADE CHAIN (:19196-97): arm
            // the token (+48 = +50); the debit, the (9,10) castle
            // ball and its (10,43) upgrade-token ride all run at the
            // token's own slot ([`Self::rival_castle_token_tick`],
            // retail's sub_57610 machine) — the corpus-refuted ch5
            // shortcut is retired (mc1l5 t=5152).
            self.g.ent[m].f26 = self.spells()[16].count as i16;
            self.rivals[ri].cooldown[16] = AI_RECAST[16];
            return true;
        }
        // Castle-less: the FREE direct plant at the scouted site
        // (:19200-08) — no debit, no projectile. ⚠ NO (0,0) sentinel:
        // the site is a supercell-corner value and (0,0) is a LEGAL
        // one — mc1l5 t=14771, Vodor's post-raze rebuild scouts
        // exactly (0,0) and retail plants castle 478 off it (the
        // port's invented empty-site test refused the plant forever).
        let (sx, sy) = self.rivals[ri].site;
        let gz = self.g.ground_z(sx, sy) as i16;
        let Some(c) = self.g.spawn_class3(2, sx, sy, gz) else {
            return false;
        };
        // The planted castle's recorded birth row (mc1l5 t=14771,
        // slot 478): state 5 (TRANSFORM — it rises through the level
        // machine, whose first commit re-binds +50 idempotently),
        // sprite 177 FLAT (no per-slot offset; its art extents are
        // the recorded 184), ceiling 0 (the establish tick prices
        // it — the ctor stamps nothing).
        {
            let e = &mut self.g.ent[c];
            e.id24 = self.rivals[ri].ent;
            e.f26 = 0;
            e.tick70 = 5;
        }
        // The plant BINDS at spawn (:19206 writes wizext+50) — the
        // one bind site that precedes any level-up commit.
        self.g.castle_reg[self.rivals[ri].slot as usize] = c as u16;
        self.g.set_sprite(c, 177);
        // ⚠ NO terrain stamp: a level-0 castle is a BARE FLAG (BUILD
        // row 0 is empty, w = h = 0 — the teardown law's own guard).
        // The pad is painted by the LEVEL-UP commit's (10,42) painter
        // over the following ticks; an immediate stamp here raised
        // tile ground retail leaves flat (mc1l5 t=14772: the class-11
        // trigger volume at (128,128) rides its every-8th-tick ground
        // snap onto a mound retail never built).
        self.g.snd(30, c);
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
            0 | 11 | 13 | 17 | 20 | 23 => Some(9u8),
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
            0 | 23 => self.g.spawn_fireball(x, y, z),
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
        // The strafe lane of the same shared mover (:55199-203): the
        // wizext's v_16 holds whatever jink residue the AI died with,
        // and NOTHING updates it during the fall — the dead brain
        // never runs its 4/tick decay — so the corpse sidesteps by
        // the SAME constant every fall tick until touchdown. mc1l3
        // t=1859: rival 585 falls with v_16 = 7 and the port's corpse
        // landed (−5,+5)/tick short of retail's, a residual every
        // (10,1) trail puff inherited at birth.
        {
            let jink = self.rivals[ri].jink;
            if jink != 0 {
                Gen::polar_step(&mut pos, yaw.wrapping_add(0x200) & 0x7FF, 0, jink);
            }
        }
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
        // The mover's wind-gust flutter (:55294-99) — every 64th tick
        // of the entity's OWN phase clock, one draw from its PRIVATE
        // LCG, 1-in-11 → sound 46. The live AI never runs sub_455D0,
        // so for a rival this fires only during the fall; the +63
        // read is PRE-increment (the walk clocks the record after the
        // handler). mc1l3 t=1890: the corpse's f63 crosses 192 and
        // retail's rand steps exactly once where the port's held.
        if self.g.ent[i].f63 & 0x3F == 0 {
            let roll = crate::engine::features::lcg32(&mut self.g.ent[i].rand);
            if roll % 11 == 0 {
                self.g.snd(46, i);
            }
        }
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
        // into a decaying ground jar around the corpse — iterated over
        // the +532 ACQUISITION LIST in PICKUP order, not the spell-id
        // book (mc1l4 t=6885: two tokens picked up out of spell order
        // swap their scatter draws under a book iteration). Each live
        // entry is rewritten to the token's MODEL number, each empty
        // one to −1, for the respawn re-grant's in-place refill.
        // The scatter anchors on the corpse's own +76 (:55537 copies
        // `*(WORD *)(a1 + 76)` into the position struct), i.e. the z
        // the fall just clamped onto the floor — not a fresh ground
        // sample. (Both readings coincide in the mc1l2 window, so this
        // is decompile authority, not a corpus-proven claim.)
        let (cx, cy, cz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        self.rivals[ri].owned = [0; SPELL_COUNT];
        for k in 0..SPELL_COUNT {
            let entry = self.rivals[ri].acq[k];
            if entry <= 0 || entry as usize >= self.g.ent.len() {
                self.rivals[ri].acq[k] = -1;
                continue;
            }
            let m = entry as usize;
            self.rivals[ri].acq[k] = self.g.ent[m].model65 as i32;
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
        // The re-grant is LIST-driven (:54884-923), not book-driven:
        // each acquisition entry the death rewrote to a model number
        // re-mints that model IN PLACE (−1 entries reset to 0 and
        // skip), so the reborn book keeps the pickup order — and the
        // mint order, which decides which free-stack slots the fresh
        // tokens take. A scattered fireball's entry is 0 (model 0 ≡
        // the empty sentinel) and still re-mints: retail's collision,
        // kept deliberately.
        for k in 0..SPELL_COUNT {
            let entry = self.rivals[ri].acq[k];
            if entry < 0 || entry as usize >= SPELL_COUNT {
                self.rivals[ri].acq[k] = 0;
                continue;
            }
            let s = entry as usize;
            if let Some(m) = self.mint_manifestation(s, ent) {
                self.rivals[ri].acq[k] = m as i32;
                self.rivals[ri].owned[s] = m as u16;
            } else {
                self.rivals[ri].acq[k] = 0;
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
            acq,
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
            v14,
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
        w.put(acq);
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
        w.put(v14);
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
            acq: r.get()?,
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
            v14: r.get()?,
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
    /// A flat world with one rival whose book holds POSSESS (spell 3)
    /// and nothing else — the claim-cone probe's scaffolding.
    fn possess_world() -> World {
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
        book[3] = true;
        let mut cfgs: [Option<RivalConfig>; 8] = Default::default();
        cfgs[1] = Some(RivalConfig {
            aggression: 200,
            accuracy: 255,
            tempo: 255,
            castle_level: 0,
            book,
            allowed: book,
        });
        w.set_wizards(&cfgs, 2);
        w
    }

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

    /// ⭐ THE DEFENSE SCAN WALKS THE TICK-TOP CLASS-9 ROSTER
    /// (`sub_16800` :19777 seeds from `var_u32_36462[3]`, the case-9
    /// arm of the tick-head sweep at :52279 — every class-9 record, NO
    /// life or flags test): a ball born MID-tick cannot arm the dodge
    /// until the next rebuild, and a soft-killed member still can.
    ///
    /// This is the mc1l4 t=5377 law — the certification residue whose
    /// pair diff was CLEAN (jink is a wizext lane; the wizext shadow
    /// named it): the pool-scan port dodged the pelting stream's
    /// newborn ball, retail's roster could not yet hold it, and the
    /// rival's whole post-pelting flight parted one flight-tick over.
    ///
    /// NON-VACUITY: the old pool scan fails leg (a) — it sees the
    /// newborn and jinks — and its `flags & 0x400` conjunct fails leg
    /// (c).
    #[test]
    fn the_defense_scan_is_tick_top_and_keeps_soft_kills() {
        let mut w = rebound_world();
        let ri = 0;
        let i = w.rivals[ri].ent as usize;
        // (a) Mid-tick birth: the roster was sampled before the ball
        // existed — the pool holds it, the scan must not.
        w.g.rebuild_proj_chain();
        let threat = plant_threat(&mut w, ri);
        w.rival_defense(ri, i);
        assert_eq!(w.rivals[ri].jink, 0, "a mid-tick newborn armed the dodge");
        // (b) The next tick top holds it: jink 80 (sub_16870).
        w.g.rebuild_proj_chain();
        w.rival_defense(ri, i);
        assert_eq!(
            w.rivals[ri].jink, 80,
            "a tick-top member did not arm the dodge"
        );
        // (c) A soft kill is not a free: 0x400 landing mid-tick does
        // not hide a member (retail's per-node filter is chase alone).
        w.rivals[ri].jink = 0;
        w.g.ent[threat].flags |= 0x400;
        w.rival_defense(ri, i);
        assert_eq!(
            w.rivals[ri].jink, 80,
            "a soft-killed member stopped arming the dodge"
        );
        // (d) THE RANGE IS 2D (`sub_42410` :52748-54 = Δx² + Δy², no z
        // term): a bolt 5,000 units OVERHEAD is still a dodge threat —
        // in 3D its dz² alone would clear the 5120² gate. mc1hwl0
        // t=16771: threat 516 at dz 2617 read 27.7M in the port's old
        // 3D math against the 26.2M gate (out), 20.9M in retail's 2D
        // (in) — retail re-stamped the strafe, the port let it decay,
        // and the lateral gap became the t=16772 x,y head.
        w.rivals[ri].jink = 0;
        let high = {
            let e = &mut w.g.ent[threat];
            e.z = e.z.saturating_add(5000);
            e.z
        };
        w.g.rebuild_proj_chain();
        w.rival_defense(ri, i);
        assert_eq!(
            w.rivals[ri].jink, 80,
            "a bolt {high} high stopped arming the dodge — the range \
             gate grew a z leg retail does not have"
        );
    }

    /// The rival Rebound arm, end to end: an incoming fireball inside
    /// 1024 arms the token (`sub_16890` :19822 → `sub_155F0` case 0xE
    /// :19140-48), the token PUBLISHES the deflection bit on the
    /// wizard entity (`sub_573F0_57920` remc1 :65774 / remc1hw
    /// :61996 — `owner->+17 |= 0x80`, our 0x8000), the bit clears when
    /// the 101-tick burst lapses, and a fresh threat re-ups it.
    ///
    /// THE BALLOON RAID NEEDS AN OFFENSE TOKEN: sub_147E0 opens on
    /// sub_16920 (:18611) — owned-token slots for {0, 15, 8, 17, 20,
    /// 7}, the same gate the castle/wizard arms wear at :18506/:18553
    /// — so a disarmed (razed, token-scattered) wizard never raids a
    /// balloon and falls through toward the ball claim. Corpus-silent
    /// at mc1l5 t=19577 (Vodor still owned fireball there; the docked
    /// AABB was that tick's discriminator, fixture-pinned), so the
    /// gate is pinned here against the listing.
    ///
    /// NON-VACUITY: without the gate the second selector call keeps
    /// RaidBalloon — the balloon is still fat, hated and in range.
    #[test]
    fn the_balloon_raid_needs_an_offense_token() {
        let mut w = rebound_world();
        let ri = 0;
        let i = w.rivals[ri].ent as usize;
        // A fat HUMAN balloon 1500 east of the rival: hated owner
        // (hate over the wealth-scaled bar, war flag CLEAR so the
        // wizard pick's rangeless war arm stays cold and its hated
        // election loses to the range gate — the human pose sits at
        // the world origin, ~43k out), cargo over 10*(275-agg),
        // far from castle_reg[0] (unbound → the scratch slot 0).
        let (bx, by, bz) = {
            let e = &w.g.ent[i];
            (e.x.wrapping_add(1500), e.y, e.z)
        };
        let b = w.g.new_event().expect("balloon slot");
        {
            let e = &mut w.g.ent[b];
            e.class64 = 3;
            e.model65 = 3;
            e.tick70 = 9;
            e.id24 = PLAYER_TARGET;
            e.max_life = 10000;
            e.act_life = 9000;
            e.f140 = 5000;
            e.x = bx;
            e.y = by;
            e.z = bz;
        }
        w.rivals[ri].hate[0] = 65535;
        w.g.rebuild_wiz_chain();
        w.rival_selector(ri, i, true);
        assert_eq!(
            w.rivals[ri].state,
            AiState::RaidBalloon,
            "armed, the raid fires (the pick's own gates all pass)"
        );
        for s in [0usize, 15, 8, 17, 20, 7] {
            w.rivals[ri].owned[s] = 0;
        }
        w.rival_selector(ri, i, true);
        assert_ne!(
            w.rivals[ri].state,
            AiState::RaidBalloon,
            "disarmed, sub_16920 refuses the raid outright"
        );
    }

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
        // The token publishes on its OWN pool slot's tick
        // (sub_573F0): minted after the carpet, it gates and sets the
        // bit the SAME tick the defense cast arms it — by the time
        // the arm is observable the bit is up.
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
        // The clear is the token's NEXT pass (sub_573F0's `+48 <= 0`
        // arm): the tick the counter reaches 0 still ran the gate arm
        // and left the bit standing.
        w.tick(away(), PlayerCommand::default());
        assert!(
            !rebound_bit(&w, 0),
            "the lapsed token left the deflection bit set"
        );

        // ---- re-up ----------------------------------------------------
        // The burst now really pays: sub_55E80's full-tick −1000 debit
        // plus its 101-tick regen pin drained the purse, and both the
        // commit readiness (:19260-63) and the token's own full-tick
        // gate refuse a broke wizard — retail behavior. Let the
        // economy restock before the fresh threat.
        for _ in 0..32 {
            if w.rivals[0].mana >= SPELLS[14].possess_mana {
                break;
            }
            w.tick(away(), PlayerCommand::default());
        }
        assert!(
            w.rivals[0].mana >= SPELLS[14].possess_mana,
            "the economy never restocked the re-up purse"
        );
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
        // Settle until the pool is stocked, then check the WORLD
        // maintains the entity mana mirror at all. The starting
        // castle only joins the census once it reaches its
        // ESTABLISHED tick and echoes `+144 = +24` (sub_46DB0
        // :56015) — the ceiling, and with it the purse, is at the
        // intrinsic 1000 until then.
        for _ in 0..64 {
            w.tick(away(), PlayerCommand::default());
            if w.rivals[0].mana > 2000 {
                break;
            }
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
            pmana_max: 0,
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

    /// ⭐ THE POSSESS CLAIM CONE IS STRICT — `< 0x1Cu` (:18254), not
    /// `<= 28`. `sub_13BA0`'s claim arm re-derives the bearing after
    /// the approach and writes `+144` only inside that cone:
    ///
    ///     if ( sub_155F0(a1, 3u) ) {
    ///       v3 = sub_42150_42490(a1 + 72, v1 + 36);
    ///       if ( (unsigned __int16)sub_42210_42550(*(_WORD *)(a1 + 30), v3) < 0x1Cu )
    ///         v1[72] = *(_WORD *)(a1 + 24);          // +144 = the claimant
    ///     }
    ///
    /// NOT A FIXTURE: `+144` is not in the graded obs, so a pair diff
    /// can never see the wrong claim — it re-imports the ball's owner
    /// every tick. mc1l3 lands on the boundary exactly (Vodor at
    /// `+30 = 1082`, ball 105 bearing 1054, angular distance 28) and
    /// the port's `<=` took a claim retail refuses; the free run only
    /// noticed one tick later, through the MANA CENSUS crediting that
    /// ball's 512 to his ceiling (`mana_max` retail 3048, port 3560 at
    /// t=448). This test pins both sides of the boundary directly.
    #[test]
    fn the_possess_claim_cone_refuses_at_exactly_28() {
        let claim_at = |off: u16| -> u16 {
            let mut w = possess_world();
            let ri = 0;
            let i = w.rivals[ri].ent as usize;
            // A wild (10,39) mana ball, well inside the 1024 arrive
            // ring so the approach reports ARRIVED and the claim arm
            // runs at all.
            let (rx, ry, rz) = {
                let e = &w.g.ent[i];
                (e.x, e.y, e.z)
            };
            let b = w.g.new_event().expect("ball slot");
            {
                let e = &mut w.g.ent[b];
                e.class64 = 10;
                e.model65 = 39;
                e.tick70 = 41; // settled
                e.f140 = 512;
                e.f144 = 0; // wild: eligible
                e.act_life = 300;
                e.max_life = 300;
            }
            let (bx, by) = (rx, ry.wrapping_add(512));
            w.g.move_relink(b, bx, by, rz);
            let bearing = Gen::angle_between(rx, ry, bx, by);
            w.g.ent[i].f30 = (bearing + off) & 0x7FF;
            w.g.ent[i].f34 = w.g.ent[i].f30; // inside every commit cone
            w.rivals[ri].mana = 200_000;
            w.rivals[ri].cooldown[3] = 0;
            w.rivals[ri].state = AiState::Possess;
            w.rivals[ri].target = b as u16;
            w.rivals[ri].target_sig = w.target_sig(b as u16);
            assert_eq!(
                Gen::angdist(w.g.ent[i].f30, bearing),
                off,
                "test premise: the offset IS the angular distance"
            );
            w.rival_state_tick(ri, i, false);
            w.g.ent[b].f144
        };
        assert_eq!(claim_at(28), 0, "28 is OUTSIDE the cone — retail refuses");
        assert_ne!(claim_at(27), 0, "27 is inside the cone — the claim lands");
    }

    /// A castle-less rival with Create Castle (16) in its book — the
    /// plant/rebuild laws' scaffolding (ledger 2026-08-20b).
    fn castle_world() -> World {
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
        book[16] = true;
        let mut cfgs: [Option<RivalConfig>; 8] = Default::default();
        cfgs[1] = Some(RivalConfig {
            aggression: 200,
            accuracy: 255,
            tempo: 255,
            castle_level: 0,
            book,
            allowed: book,
        });
        w.set_wizards(&cfgs, 2);
        w
    }

    /// ⭐⭐ THE MC1 CASTLE-ARM WAR THRESHOLD IS FLAT 50000 (:19733-39):
    /// the listing's aggression multiplier reads through the victim
    /// CASTLE's +160 — a wizext pointer only carpets carry, mint-zero
    /// on a castle — so the scaled term never contributes. Corpus: war
    /// latches at 50518/50659/51518 (first crossing ABOVE 50000),
    /// never at 49518/49659; window 4 peaks 49531 and decays out
    /// unlatched. The fixture cannot pin this (the war lane is
    /// pair-imported), so the constant is pinned here — with the
    /// rival's aggression at 200, where the scaled threshold would
    /// have been far lower.
    #[test]
    fn the_war_threshold_is_flat_50000() {
        let mut w = possess_world();
        let ri = 0;
        w.rivals[ri].hate[0] = 49_999;
        w.rival_war_check(ri, 0);
        assert!(!w.rivals[ri].war[0], "49,999 is under the threshold");
        w.rivals[ri].hate[0] = 50_000;
        w.rival_war_check(ri, 0);
        assert!(
            !w.rivals[ri].war[0],
            "the compare is strict: exactly 50,000 does not latch"
        );
        w.rivals[ri].hate[0] = 50_001;
        w.rival_war_check(ri, 0);
        assert!(
            w.rivals[ri].war[0],
            "the first crossing above 50,000 latches"
        );
    }

    /// ⭐⭐ THE SCOUT'S HOME SUPERCELL IS SIGNED x/16384, TRUNCATING
    /// TOWARD ZERO (:18362-67's CFSHL idiom), and the walk runs
    /// THROUGH THE SCRATCH RECORD: each candidate writes slot 0's x/y
    /// (:18374-86) and the accept stamps the wizard's +150/+152/+154
    /// with the candidate and the scratch's NEVER-WRITTEN z
    /// (:18381-83). mc1l5 t=14694: Vodor rebuilds at x=65333 (i16
    /// −203 → cell 0) and retail's first candidate is literally
    /// (0,0); a `u16 >> 14` start planted a map-quadrant away.
    #[test]
    fn the_scout_cell_is_signed_and_walks_the_scratch() {
        let mut w = possess_world();
        let ri = 0;
        let i = w.rivals[ri].ent as usize;
        let z = w.g.ent[i].z;
        w.g.move_relink(i, 65333, 65333, z);
        w.g.rebuild_wiz_chain();
        w.g.ent[0].z = 352; // the scratch's standing z (imported state)
        assert!(
            w.rival_scout_site(ri, i),
            "no foreign castles: first candidate wins"
        );
        assert_eq!(
            w.rivals[ri].site,
            (0, 0),
            "i16 −203 / 16384 truncates to cell 0 — the home corner IS (0,0)"
        );
        let e = &w.g.ent[i];
        assert_eq!((e.dest_x, e.dest_y), (0, 0), "the accept stamps +150/+152");
        assert_eq!(e.site_z, 352, "+154 = the scratch record's unwritten z");
        assert_eq!(
            (w.g.ent[0].x, w.g.ent[0].y),
            (0, 0),
            "the scratch keeps the last probed candidate"
        );
    }

    /// ⭐⭐ A STALE TARGET KEEPS THE STATE: every combat handler opens
    /// on the sig-vs-stored test and returns with NO writes on
    /// mismatch (sub_13BA0 :18246, sub_13CA0 :18281, sub_13DD0
    /// :18323) — there is NO Fresh transition in the retail machine;
    /// the think cascade is the only mover. mc1l5 t=12158: the
    /// claimed ball dies and retail idles in Possess for 450 ticks.
    #[test]
    fn a_stale_target_keeps_the_state() {
        let mut w = possess_world();
        let ri = 0;
        let i = w.rivals[ri].ent as usize;
        let b = w.g.new_event().expect("ball slot");
        {
            let e = &mut w.g.ent[b];
            e.class64 = 10;
            e.model65 = 39;
            e.tick70 = 41;
            e.act_life = 300;
        }
        w.rivals[ri].state = AiState::Possess;
        w.rivals[ri].target = b as u16;
        w.rivals[ri].target_sig = w.target_sig(b as u16);
        // The slot is reaped and re-minted as something else — the
        // signature moves (team + model + class<<7).
        w.g.ent[b].class64 = 9;
        for _ in 0..8 {
            w.rival_state_tick(ri, i, false);
        }
        assert_eq!(
            w.rivals[ri].state,
            AiState::Possess,
            "no drop-to-Fresh exists"
        );
        assert_eq!(w.rivals[ri].target, b as u16, "the stale target keeps too");
    }

    /// ⭐⭐ THE FREE PLANT IS A BARE FLAG AND BUILD WRITES NO STATE
    /// (:19200-08 / sub_138F0 :18142-68): the planted castle is born
    /// state 5 TRANSFORM, level 0, sprite 177, binds wizext+50 at
    /// spawn (:19206) and stamps NO terrain (BUILD row 0 is empty —
    /// the pad is the level-up commit's painter). The handler leaves
    /// the AI state alone, so the STILL-Build handler re-casts 16 the
    /// very next tick through the NOW-bound arm and arms the upgrade
    /// token (mc1l5 t=14772's (9,10)). Teardown-to-0 clears the
    /// binding blind (:56534).
    #[test]
    fn the_plant_is_a_bare_flag_and_build_recasts_bound() {
        let mut w = castle_world();
        let ri = 0;
        let i = w.rivals[ri].ent as usize;
        let ws = w.rivals[ri].slot as usize;
        w.rivals[ri].state = AiState::Build;
        w.rivals[ri].site = (w.g.ent[i].x, w.g.ent[i].y);
        w.rivals[ri].mana = 200_000;
        w.rivals[ri].cooldown[16] = 0;
        let pristine = w.g.t.height.clone();

        // Tick 1: arrived at the site, the free plant fires.
        w.rival_state_tick(ri, i, false);
        let c = w.rival_castle(w.rivals[ri].ent).expect("the plant landed");
        assert_eq!(w.rivals[ri].state, AiState::Build, "Build writes NO state");
        assert_eq!(w.g.castle_reg[ws], c as u16, "the plant binds wizext+50");
        {
            let e = &w.g.ent[c];
            assert_eq!(e.tick70, 5, "born TRANSFORM");
            assert_eq!(e.f26, 0, "level 0");
            assert_eq!(e.type86, 177, "sprite 177 flat");
        }
        assert_eq!(w.g.t.height, pristine, "a level-0 plant stamps NO terrain");

        // Tick 2: the still-Build handler re-casts through the BOUND
        // arm — the upgrade token arms on the day-old castle.
        w.rival_state_tick(ri, i, false);
        assert_eq!(w.rivals[ri].state, AiState::Build, "still no state write");
        assert!(
            token_of(&w, ri, 16) > 0,
            "the re-cast armed the upgrade token through the bound arm"
        );

        // Teardown to level 0 clears the binding blind.
        w.g.ent[c].tick70 = 6;
        w.g.ent[c].f26 = 0;
        w.g.castle_tick(c, crate::patches::WorldPatches::default());
        assert_eq!(w.g.castle_reg[ws], 0, "teardown-to-0 clears wizext+50");
        assert!(w.g.ent[c].flags & 0x400 != 0, "the flag soft-kills");
    }

    /// ⭐⭐ THE CLAIM GATE READS THE TOKEN'S LIVE +136 PRICE CACHE
    /// (sub_14230 :18452: `wiz +136 <= manifestation +136`, no gate
    /// when 16 is unowned) — CAP[level] housed, 1000 ctor, 5000
    /// after a raze — so ball-claiming re-opens after every upgrade
    /// AND while razed. mc1l5 t=16081: razed Vodor at ceiling 1768
    /// vs his token's 5000 claims the wild 2000-ball; the port's
    /// static-cost stand-in (1768 > 1000) fell through to HuntMana.
    /// (The exemplar pair carries the at-castle mana-register
    /// residue, so the law is pinned here instead of a fixture.)
    #[test]
    fn the_claim_gate_reads_the_live_token_price() {
        let claim = |price: i32| -> AiState {
            let mut w = castle_world();
            let ri = 0;
            let i = w.rivals[ri].ent as usize;
            w.rivals[ri].known[3] = true;
            w.rivals[ri].allowed[3] = true;
            let m16 = w.rivals[ri].owned[16] as usize;
            w.g.ent[m16].f136 = price;
            w.rivals[ri].mana_max = 1768;
            // A wild settled ball in range, and no castle (razed).
            let (rx, ry, rz) = {
                let e = &w.g.ent[i];
                (e.x, e.y, e.z)
            };
            let b = w.g.new_event().expect("ball");
            {
                let e = &mut w.g.ent[b];
                e.class64 = 10;
                e.model65 = 39;
                e.tick70 = 41;
                e.f140 = 2000;
                e.act_life = 300;
            }
            w.g.move_relink(b, rx, ry.wrapping_add(600), rz);
            w.g.rebuild_ball_chain();
            w.rival_selector(ri, i, true);
            w.rivals[ri].state
        };
        assert_eq!(
            claim(5000),
            AiState::Possess,
            "ceiling 1768 <= the razed token's 5000: the claim is open"
        );
        assert_ne!(
            claim(1000),
            AiState::Possess,
            "ceiling 1768 > a 1000 cache: the claim gate is closed"
        );
    }

    /// ⭐⭐ THE WAKE PASS READS THE CARPET SETTLED LAST TICK
    /// (sub_54F00's :64352-53 pool read, a PRE-pass): native play
    /// feeds this tick's live pose as the `player` arg, so the
    /// settled-last-tick value is the `human_pose_prev` echo; the
    /// replay drivers already pass settled(N−1) AS the arg
    /// (`strict_retail`). Exposed only by a teleport-scale jump —
    /// mc1l5 t=14420, the human teleports back into bee 338's wake
    /// radius and retail's t=14421 pass arms the bee that very tick.
    #[test]
    fn the_wake_pass_reads_the_pose_settled_last_tick() {
        let cmd = PlayerCommand::default();
        let far = PlayerPose::level(200 << 8, 200 << 8, 6000, 0);
        let near = PlayerPose::level(15 << 8, 15 << 8, 6000, 0);
        let sleeper = |w: &mut World| {
            let g = w.g.ground_z(10 << 8, 10 << 8) as i16;
            let c =
                w.g.spawn_creature(4, 10 << 8, 10 << 8, g)
                    .expect("creature");
            w.g.ent[c].f58 = 0;
            w.g.ent[c].f59 = 0;
            c
        };

        // NATIVE: the near pose is the ARG of tick 2, but the gate
        // reads the pose settled during tick 1 (far) — the creature
        // sleeps one more tick and wakes on tick 3.
        let mut w = possess_world();
        let c = sleeper(&mut w);
        w.tick(far, cmd);
        assert_eq!(w.g.ent[c].f58 & 0xFF, 0, "far pose: asleep");
        w.tick(near, cmd);
        assert_eq!(
            w.g.ent[c].f58 & 0xFF,
            0,
            "native: the gate reads the carpet settled LAST tick"
        );
        w.tick(near, cmd);
        assert_eq!(
            w.g.ent[c].f58 & 0xFF,
            16,
            "the echo caught up: re-armed to 16"
        );

        // REPLAY DRIVERS (`strict_retail`): the arg IS settled(N−1),
        // so the same jump wakes the creature a tick earlier.
        let mut w = possess_world();
        let c = sleeper(&mut w);
        w.strict_retail = true;
        w.tick(far, cmd);
        w.tick(near, cmd);
        assert_eq!(
            w.g.ent[c].f58 & 0xFF,
            16,
            "strict_retail: the player arg is the gate pose"
        );
    }
}
