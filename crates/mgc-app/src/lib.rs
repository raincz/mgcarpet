//! The game shell: window, input, fixed-timestep loop.
//!
//! The carpet flyer. Loads a baked `.mgcl` package, resolves its color
//! LUT from the baked assets, and flies: the sim ticks at
//! `mgc_sim::TICK_RATE_HZ`, rendering interpolates between the last two
//! ticks at whatever rate the display runs.
//!
//! Also runs headless: `--screenshot out.png` renders one frame
//! offscreen and exits, which is how terrain changes get verified
//! without a display.

mod bakecheck;
mod campaign;
mod config;
mod entities;
mod frontend;
mod frontend_mc1;
mod menu;
mod minimenu;
mod movie;
mod replay;
mod saves;
mod settings;
mod ui;
mod worldmap;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mgc_formats::bundle::Bundle;
use mgc_formats::{Game, LevelPackage, mgcl};
use mgc_render::{Billboard, CameraView, LevelView, Renderer};
use mgc_sim::{FlightInput, Flyer, Simulation, TICK_DT};
use winit::application::ApplicationHandler;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

const FOV_Y: f32 = 60.0_f32.to_radians();
const MOUSE_SENSITIVITY: f32 = 0.0022;
/// MC1 virtual-stick gain: stick units (±127 full deflection) per
/// pixel of mouse motion. The original's DOS cursor reached full
/// deflection ~160 px from the center of a 320-wide screen (~0.8/px);
/// half that suits modern DPI while sensitivity 1.0 keeps the range.
const STICK_PER_PIXEL: f32 = 0.4;
/// The book's canonical spell order (`byte_99B88`, remc1 :5752) —
/// retail's scan order for the level-init quickselect pre-seed
/// (:49216-59); identical in HW (remc1hw :4381).
const SPELL_CANON: [u8; 24] = [
    0, 3, 2, 16, 1, 14, 4, 12, 6, 9, 7, 8, 15, 18, 17, 19, 13, 5, 11, 10, 20, 21, 22, 23,
];

/// Pristine inputs to rebuild the [`mgc_sim::engine::world::World`] for a
/// LEVEL RESTART — the original's castle-less-death "lost + level
/// over" flow ends in exactly this (respawn at the start of a fresh
/// level).
struct WorldInit {
    /// The sim-side game profile (chassis + verb column selector).
    game: mgc_sim::ids::GameId,
    planes: mgc_sim::engine::features::Planes,
    things: Vec<mgc_formats::Thing>,
    seed: u32,
    assets: mgc_sim::engine::features::FeatureAssets,
    win_pct: u16,
    /// Rival wizard configs by player slot (wizards.json) + the
    /// level's active-slot count. MC1 column only.
    wizards: [Option<mgc_sim::mc1::rivals::RivalConfig>; 8],
    /// MC2 rival configs by color (wizards.json MC2 shape + the
    /// header's authored castle levels); `player_count` doubles as
    /// the NumberOfPlayers bound (header unk09) on MC2.
    mc2_wizards: [Option<mgc_sim::mc2::rivals::Mc2RivalConfig>; 8],
    player_count: u16,
    /// MC2 stage checkpoints (`(index, stage, x, y)` rows) — the
    /// single-stage objective engine's board. Empty for MC1/HW.
    stages: Vec<(i8, i16, i16, i16)>,
    /// MC2 StageVars (`(index, stage, x, y, data)` per slot) — the
    /// triggered-spawn / hold-gate layer. Empty for MC1/HW.
    stage_vars: Vec<(i8, i8, u8, u8, u32)>,
    /// MC2 Night/Cave level: the runtime terrain repaint inverts
    /// relief shading (remc2 Terrain.cpp:2030-2033).
    night_shade: bool,
    doom_level: bool,
    /// Draw stand-in art for unported models (deliberate: MC2 default
    /// until its roster closes; the ledger stays truthful either way).
    placeholders: bool,
    /// Remove spell jars the local player already owns (deliberate:
    /// P-class improvement, both games). Applied at every level load —
    /// the sim self-culls owned jars on their next tick.
    prune_owned_jars: bool,
    /// The chassis constant set: the game's pristine profile, or a
    /// deliberately deviating one (the limit-removing `--pool-slots`
    /// dev flag; G-class — a run under a bumped pool is not a
    /// faithful fixture).
    chassis: mgc_sim::chassis::ChassisParams,
}

impl WorldInit {
    fn build(&self) -> mgc_sim::engine::world::World {
        let mut w = mgc_sim::engine::world::World::new_full(
            self.planes.clone(),
            &self.things,
            self.seed,
            self.assets.clone(),
            self.chassis.clone(),
            self.game,
        );
        // Applies to BOTH games' jar systems (P-class improvement).
        w.set_prune_owned_jars(self.prune_owned_jars);
        if matches!(self.game, mgc_sim::ids::GameId::Mc2) {
            w.set_placeholders(self.placeholders);
            w.set_mc2_night_shade(self.night_shade);
            w.set_mc2_doom_level(self.doom_level);
            if !self.stages.is_empty() {
                w.set_mc2_stages(&self.stages);
            }
            if !self.stage_vars.is_empty() {
                w.set_mc2_stagevars(&self.stage_vars);
            }
            w.set_mc2_wizards(&self.mc2_wizards, self.player_count);
        } else {
            if self.win_pct > 0 {
                w.set_win_pct(self.win_pct);
            }
            w.set_wizards(&self.wizards, self.player_count);
        }
        w
    }
}

/// The parameters the initial `load_level` ran with, kept so a
/// campaign level switch rebuilds through the exact same path.
struct LaunchParams {
    tileset: Option<u8>,
    terrain_features: bool,
    pool_slots: Option<usize>,
    awake_range: Option<u32>,
}

/// The slot's durable retail-format record, tagged by game — MC1/HW
/// and MC2 have distinct on-disk formats, so the run carries exactly
/// one. `level` on the MC1 record = the level to play.
// Exactly one lives per run (the `save` field, never a collection), so the
// larger MC2 variant costs nothing to hold inline — boxing would only add
// indirection through every accessor for no memory win.
#[allow(clippy::large_enum_variant)]
enum CampaignSave {
    Mc1(saves::Mc1Save),
    Mc2(saves::Mc2Save),
}

impl CampaignSave {
    fn mc1(&self) -> Option<&saves::Mc1Save> {
        match self {
            CampaignSave::Mc1(s) => Some(s),
            CampaignSave::Mc2(_) => None,
        }
    }
    fn mc1_mut(&mut self) -> Option<&mut saves::Mc1Save> {
        match self {
            CampaignSave::Mc1(s) => Some(s),
            CampaignSave::Mc2(_) => None,
        }
    }
    fn mc2(&self) -> Option<&saves::Mc2Save> {
        match self {
            CampaignSave::Mc2(s) => Some(s),
            CampaignSave::Mc1(_) => None,
        }
    }
    fn mc2_mut(&mut self) -> Option<&mut saves::Mc2Save> {
        match self {
            CampaignSave::Mc2(s) => Some(s),
            CampaignSave::Mc1(_) => None,
        }
    }
}

/// A running campaign (`--campaign <mc1|mc1hw|mc2>`): the level-order
/// law + the slot's durable retail-format record + the cross-level
/// carry. The record IS the state — completing a level updates it and
/// writes the slot file, so quitting anywhere resumes correctly.
struct CampaignRun {
    id: campaign::CampaignId,
    /// 0-based save slot; `None` = the virtual slot 0 (`--slot`
    /// default) — retail's own boot shape: a fresh throwaway campaign
    /// that never touches disk until the player saves from the menu
    /// (which adopts the chosen slot).
    slot: Option<usize>,
    /// The level being played right now.
    current: u32,
    /// The slot's durable record (MC1/HW or MC2, matching `id`).
    save: CampaignSave,
    /// What follows the current fade-out (set at the won edge).
    next: Option<campaign::NextStep>,
    /// MC1/HW spell cycle-ring carry (native-only sidecar — the
    /// retail record has no room; see `SaveHeader::mc1_spell_ring`).
    /// Installed into each fresh world, refreshed at the won edge.
    /// All-zero on MC2 runs.
    mc1_ring: [u8; 24],
}

/// The slot's campaign record, native file first: `(retail-format
/// bytes, the native header's MC1 cycle ring)`. The ring rides only
/// the native container (`None` from a retail import or a
/// version-recovery — those simply start with an empty ring).
///
/// Returns `None` for "start fresh" — either `new_game`, or nothing
/// on disk. A file that EXISTS but cannot be read is an error, not a
/// fresh start: silently starting over would overwrite it on the
/// first level completion.
#[allow(clippy::type_complexity)]
fn campaign_record(
    tag: &str,
    slot: usize,
    new_game: bool,
) -> Result<Option<(Vec<u8>, Option<[u8; 24]>)>, String> {
    if new_game {
        return Ok(None);
    }
    let native = saves::native_path(tag, slot);
    if native.exists() {
        let open =
            || std::fs::File::open(&native).map_err(|e| format!("{}: {e}", native.display()));
        match mgc_formats::mgcs::read(open()?) {
            Ok(pkg) => {
                if pkg.is_in_level() {
                    // The campaign half still resolves the level to
                    // launch; resuming INTO the snapshot is the load
                    // path's job, not the campaign opener's.
                    println!("campaign {tag}: slot {} holds a mid-level save", slot + 1);
                }
                return Ok(Some((pkg.campaign, pkg.header.mc1_spell_ring)));
            }
            // A container this build cannot apply. The campaign record
            // inside is retail's byte layout, so it survives any
            // version of ours — take it and lose only the resume.
            // Reported, never silent: the player is down a save state.
            Err(e) => {
                let rec = mgc_formats::mgcs::recover(open()?)
                    .map_err(|_| format!("{}: {e}", native.display()))?;
                println!(
                    "campaign {tag}: slot {} was written by save version {} — \
                     progress recovered, the in-level resume was dropped",
                    slot + 1,
                    rec.save_version
                );
                return Ok(Some((rec.campaign, None)));
            }
        }
    }
    let retail = saves::retail_path(tag, slot);
    if retail.exists() {
        println!(
            "campaign {tag}: slot {} imported from the retail save {}",
            slot + 1,
            retail.display()
        );
        return std::fs::read(&retail)
            .map(|b| Some((b, None)))
            .map_err(|e| format!("{}: {e}", retail.display()));
    }
    Ok(None)
}

/// The MC2 level a save is waiting on: a revealed-but-uncompleted
/// secret takes precedence (the player is mid-branch), otherwise the
/// linear frontier. `levels_completed` counts opened portals, so it
/// indexes the first unopened one.
///
/// One rule, applied both when a slot is opened and whenever the run
/// returns to the map — a slot saved from the map must name the same
/// level before and after a reload.
fn mc2_pending_level(save: &saves::Mc2Save) -> u32 {
    save.secrets
        .iter()
        .find(|p| p.activated == 2)
        .map(|p| p.level as u32)
        .unwrap_or(save.levels_completed)
}

impl CampaignRun {
    /// Open (or start) a campaign: load the slot's retail-format save
    /// unless `new_game`, and resolve the level to launch. Errors are
    /// user-facing (bad save file, finished campaign).
    fn start(
        id: campaign::CampaignId,
        slot: Option<usize>,
        new_game: bool,
    ) -> Result<Self, String> {
        use campaign::CampaignId;
        let hw = id == CampaignId::Mc1Hw;
        // 1-based slot number for messages; the virtual slot reads 0.
        let slot_no = slot.map_or(0, |s| s + 1);
        // The native save is authoritative; the retail `.gam` is read
        // only when no native file exists for the slot (an imported
        // GOG-era save). See `saves`. The virtual slot 0 has no file,
        // ever — it always starts fresh.
        let record = match slot {
            Some(s) => campaign_record(id.tag(), s, new_game)?,
            None => None,
        };
        let mc1_ring = record.as_ref().and_then(|(_, r)| *r).unwrap_or([0; 24]);
        let record = record.map(|(b, _)| b);
        match id {
            CampaignId::Mc1 | CampaignId::Mc1Hw => {
                let save = if let Some(bytes) = record {
                    let s = saves::Mc1Save::decode(&bytes)
                        .map_err(|e| format!("{} slot {}: {e}", id.tag(), slot_no))?;
                    println!(
                        "campaign {}: slot {} \"{}\" at level {}",
                        id.tag(),
                        slot_no,
                        s.name,
                        s.level
                    );
                    s
                } else {
                    match slot {
                        Some(_) => println!("campaign {}: new game (slot {slot_no})", id.tag()),
                        None => println!(
                            "campaign {}: new game (virtual slot 0 — save from the menu \
                             to keep progress)",
                            id.tag()
                        ),
                    }
                    // The default wizard name when none is entered —
                    // player slot 0 of the retail name table (remc1
                    // off_99B68; MC2's GameUI list opens the same way).
                    saves::Mc1Save {
                        name: "Zanzamar".into(),
                        ..Default::default()
                    }
                };
                let current =
                    campaign::mc1_start_level(save.level as u32, hw).ok_or_else(|| {
                        format!(
                            "campaign {}: slot {} is a completed campaign — relaunch with \
                             --new-game to start over",
                            id.tag(),
                            slot_no
                        )
                    })?;
                Ok(Self {
                    id,
                    slot,
                    current,
                    save: CampaignSave::Mc1(save),
                    next: None,
                    mc1_ring,
                })
            }
            CampaignId::Mc2 => {
                let save = if let Some(bytes) = record {
                    let s = saves::Mc2Save::decode(&bytes)
                        .map_err(|e| format!("mc2 slot {}: {e}", slot_no))?;
                    println!(
                        "campaign mc2: slot {} \"{}\" — {} level(s) completed",
                        slot_no, s.label, s.levels_completed
                    );
                    s
                } else {
                    match slot {
                        Some(_) => println!("campaign mc2: new game (slot {slot_no})"),
                        None => println!(
                            "campaign mc2: new game (virtual slot 0 — save from the menu \
                             to keep progress)"
                        ),
                    }
                    // Same retail default name as MC1 — the head of
                    // the wizard list (GameUI.h) when none is entered.
                    saves::Mc2Save {
                        label: "Zanzamar".into(),
                        player_name: "Zanzamar".into(),
                        ..Default::default()
                    }
                };
                if save.levels_completed >= 25 {
                    return Err(
                        "campaign mc2: slot holds a completed campaign — relaunch with \
                         --new-game to start over"
                            .into(),
                    );
                }
                let current = mc2_pending_level(&save);
                Ok(Self {
                    id,
                    slot,
                    current,
                    save: CampaignSave::Mc2(save),
                    next: None,
                    mc1_ring: [0; 24],
                })
            }
        }
    }

    /// The baked package path for a level of this campaign.
    fn level_path(&self, level: u32) -> PathBuf {
        get_baked_directory().join(format!("{}/level-{level:03}.mgcl", self.id.tag()))
    }

    /// The retail record's bytes, whichever game this is.
    fn campaign_bytes(&self) -> Vec<u8> {
        match &self.save {
            CampaignSave::Mc2(s) => s.encode(),
            CampaignSave::Mc1(s) => s.encode(),
        }
    }

    /// The bundle-level game tag for a save header.
    fn save_game(&self) -> Game {
        match self.id {
            campaign::CampaignId::Mc1 => Game::MagicCarpet1,
            campaign::CampaignId::Mc1Hw => Game::HiddenWorlds,
            campaign::CampaignId::Mc2 => Game::MagicCarpet2,
        }
    }

    /// The slot's display label.
    fn label(&self) -> String {
        match &self.save {
            CampaignSave::Mc2(s) => s.label.clone(),
            CampaignSave::Mc1(s) => s.name.clone(),
        }
    }

    /// Campaign position for the header's level column.
    fn campaign_level(&self) -> u32 {
        match &self.save {
            CampaignSave::Mc2(s) => s.levels_completed,
            CampaignSave::Mc1(s) => s.level as u32,
        }
    }

    /// A hub save: campaign progress only, no world payload.
    fn hub_package(&self) -> mgc_formats::mgcs::SavePackage {
        let mut header = mgc_formats::mgcs::hub_header(
            self.save_game(),
            self.label(),
            self.campaign_level(),
            // The level the campaign is sitting at, so a hub save
            // reads "L3" exactly like the in-level save taken in
            // the same level.
            self.current,
        );
        // The MC1 cycle ring rides the native header only (the
        // retail record has no room); omitted while empty so
        // ring-less saves keep their old shape.
        if self.id != campaign::CampaignId::Mc2 && self.mc1_ring != [0; 24] {
            header.mc1_spell_ring = Some(self.mc1_ring);
        }
        mgc_formats::mgcs::SavePackage {
            header,
            campaign: self.campaign_bytes(),
            snapshot: None,
        }
    }

    /// Write the slot (creating `saves/<game>/`): the native `.mgcs`
    /// plus the retail `.gam` export beside it. IO failure is
    /// reported, never fatal — losing a save must not kill the run.
    ///
    /// This is the BETWEEN-LEVELS write, so it clears any world
    /// payload the slot was carrying: completing a level must not
    /// leave a resume pointing back into it (design "Lifecycle").
    fn persist(&self) {
        // The virtual slot 0 run keeps its progress in memory only —
        // retail's boot shape. Nothing lands on disk until the player
        // saves from the menu, which adopts the chosen slot.
        let Some(slot) = self.slot else { return };
        match saves::write_slot(self.id.tag(), slot, &self.hub_package()) {
            Ok(()) => println!(
                "campaign saved: {}",
                saves::native_path(self.id.tag(), slot).display()
            ),
            Err(e) => eprintln!("error: campaign save: {e}"),
        }
    }
}

/// Scan the 8 MC2 save slots for the frontend pickers: (label,
/// One frontend slot row: (label, occupied).
///
/// Native-first, like every other read of a slot (`saves::scan_slot`),
/// so a slot picked here resumes into the same thing the mini-menu
/// would have resumed into. A slot carrying a world payload gets its
/// level appended — the player has to be able to tell, BEFORE picking,
/// which slots drop them straight back into play and which start a
/// level over.
///
/// Letters, digits and spaces only in the suffix: these labels reach
/// the same FONT1 bank the mini-menu draws through, where punctuation
/// slots hold game icons rather than ASCII.
fn frontend_slot_row(tag: &str, i: usize, empty: &str, default_label: &str) -> (String, bool) {
    let info = saves::scan_slot(tag, i);
    if !info.occupied {
        return (empty.to_string(), false);
    }
    if info.incompatible {
        // Occupied, so it is never offered as a free slot to
        // overwrite blind.
        return (format!("{} unreadable", i + 1), true);
    }
    let mut label = if info.label.trim().is_empty() {
        default_label.to_string()
    } else {
        info.label.trim().to_string()
    };
    // Every slot shows its level; a resuming one adds how far into it
    // the run had got. Same shape as the mini-menu's rows.
    match info.resume {
        Some(pct) => label.push_str(&format!("  L{} {pct}%", info.level)),
        None => label.push_str(&format!("  L{}", info.level)),
    }
    if info.stale {
        // Salvaged from an older container: the progress is here, the
        // resume is not. Say so — a slot that quietly stopped resuming
        // reads as fine until the level restarts.
        label.push_str(" old");
    }
    (label, true)
}

/// Scan the 6 MC1/HW save slots: (label, occupied); "--" = empty,
/// exactly the retail slot list (`sub_51A10`, :61982).
fn scan_mc1_slots(tag: &str) -> Vec<(String, bool)> {
    (0..saves::MC1_SLOTS)
        .map(|i| frontend_slot_row(tag, i, "--", &format!("CARPET{}", i + 1)))
        .collect()
}

/// Scan the 8 MC2 save slots: (label, occupied) — "Empty" for
/// vacant/foreign files, exactly retail's probe (signature + 20-byte
/// label, MI:1461-79).
fn scan_mc2_slots() -> Vec<(String, bool)> {
    (0..saves::MC2_SLOTS)
        .map(|i| frontend_slot_row("mc2", i, "Empty", &format!("SLOT {}", i + 1)))
        .collect()
}

/// Resolve the package's wizards.json into per-slot rival configs
/// (MC1: the 8 x 216-byte level-record tail — aggression/accuracy/
/// tempo + the two 24-spell masks; the AI's book = pregrant &&
/// allowed, remc1 :49222).
fn rival_configs(
    wizards: Option<&mgc_formats::Wizards>,
) -> ([Option<mgc_sim::mc1::rivals::RivalConfig>; 8], u16) {
    let mut out: [Option<mgc_sim::mc1::rivals::RivalConfig>; 8] = Default::default();
    let Some(w) = wizards else { return (out, 1) };
    let count = w.player_count.unwrap_or(1).min(8);
    for (slot, cfg) in w.wizards.iter().enumerate().take(8).skip(1) {
        let (Some(acc), Some(tempo), Some(allowed_mask)) =
            (cfg.accuracy, cfg.tempo, cfg.allowed_spells.as_ref())
        else {
            continue; // MC2-shaped config: no MC1 rival data
        };
        let mut book = [false; 24];
        let mut allowed = [false; 24];
        for s in 0..24 {
            let a = allowed_mask.get(s).copied().unwrap_or(0) != 0;
            allowed[s] = a;
            book[s] = a && cfg.starting_spells.get(s).copied().unwrap_or(0) != 0;
        }
        out[slot] = Some(mgc_sim::mc1::rivals::RivalConfig {
            aggression: cfg.aggression.clamp(0, 255) as u8,
            accuracy: acc.clamp(0, 255) as u8,
            tempo: tempo.clamp(0, 255) as u8,
            castle_level: cfg.castle_level.unwrap_or(0),
            book,
            allowed,
        });
    }
    (out, count)
}

/// Resolve an MC2 package's wizards.json + level header into
/// per-color rival configs: personality (aggression/perception/
/// reflexes/Life), the three 26-spell masks, the authored starting-
/// castle level (header `players[color]`), and the NumberOfPlayers
/// bound (header `unk09` — colors 1..n-1 spawn as rivals).
fn mc2_rival_configs(
    wizards: Option<&mgc_formats::Wizards>,
    header: Option<&mgc_formats::LevelHeader>,
) -> ([Option<mgc_sim::mc2::rivals::Mc2RivalConfig>; 8], u16) {
    let mut out: [Option<mgc_sim::mc2::rivals::Mc2RivalConfig>; 8] = Default::default();
    let (Some(w), Some(h)) = (wizards, header) else {
        return (out, 1);
    };
    let count = h.number_of_players.clamp(1, 8) as u16;
    for (slot, cfg) in w.wizards.iter().enumerate().take(8).skip(1) {
        let (Some(reflexes), Some(perception)) = (cfg.reflexes, cfg.perception) else {
            continue; // MC1-shaped config: no MC2 rival data
        };
        let mut start = [false; 26];
        let mut start_level = [0u8; 26];
        let mut blocked = [false; 26];
        for s in 0..26 {
            start[s] = cfg.starting_spells.get(s).copied().unwrap_or(0) != 0;
            start_level[s] = cfg
                .starting_spell_levels
                .get(s)
                .copied()
                .unwrap_or(0)
                .min(2);
            blocked[s] = cfg.blocked_spells.get(s).copied().unwrap_or(0) != 0;
        }
        out[slot] = Some(mgc_sim::mc2::rivals::Mc2RivalConfig {
            aggression: cfg.aggression.clamp(0, 255) as u8,
            perception: perception.clamp(0, 255) as u8,
            reflexes: reflexes.clamp(0, 255) as u8,
            life: cfg.life.unwrap_or(0).max(0) as u16,
            castle_level: h.players[slot].max(0) as u8,
            start,
            start_level,
            blocked,
        });
    }
    (out, count)
}

struct LoadedLevel {
    view: LevelView,
    height: Vec<u8>,
    label: String,
    /// The sim-side game profile — picks the sprite table the pose
    /// snapshot resolves through (MC1 stats vs MC2 params).
    game: mgc_sim::ids::GameId,
    /// Bundle sprite data for the renderer (index, atlas pixels).
    sprites: Option<(mgc_formats::bundle::SpriteIndex, Vec<u8>)>,
    /// World entities resolved to billboards (initial population).
    billboards: Vec<Billboard>,
    /// Entity dots for the overhead map (the original's 1px markers).
    map_dots: Vec<mgc_render::MapDot>,
    /// The level's player start (class-3 m4 marker): position and
    /// facing for the flyer; None on levels without one (MC2, dev
    /// leftovers) falls back to the flyer default.
    start: Option<Flyer>,
    /// The living MC1/HW world (triggers, dispositions, runtime
    /// terrain events); moved into the Simulation by App::new. None =
    /// static terrain (MC2, or --no-terrain-features).
    world: Option<mgc_sim::engine::world::World>,
    /// Rebuild inputs for the castle-less-death level restart.
    world_init: Option<WorldInit>,
    /// Bundle palette, kept for runtime map-dot rebuilds.
    palette_rgba: [[u8; 4]; 256],
    /// The MC2 map-marker environment (team-colour table + map-type
    /// colours) from the level header; Day for MC1/HW.
    mc2_env: entities::Mc2MapEnv,
    /// The per-game audio bundle directory (`assets/mc1-audio` /
    /// `mc2-audio`), when baked.
    audio_dir: Option<PathBuf>,
    /// The level-music pick: MC2 = ONE looping XMI by MapType
    /// (mc2-night/day/cave — docs/traces/mc2-music-law.md); MC1
    /// stays the INTERIM cgame1-3 level cycle until its
    /// song-command source data is traced.
    music_track: Option<String>,
    /// 0-based level number = the `CdTracks_DB080` speech row
    /// (docs/traces/mc2-voiceover-triggers.md §4).
    level_number: u32,
    /// The asset bundle this level resolved to (`mc1-temperate`,
    /// `mc2-night`, …). Recorded in a mid-level save: the snapshot
    /// omits `Gen::assets`, so a restore against a different bundle
    /// would re-supply different feature data.
    bundle_variant: String,
    /// Hex SHA-256 of the level package's source archive entry — the
    /// save's rejection key. `None` on community-authored packages,
    /// which carry no `source` block; such a level can still be saved,
    /// it just cannot be identity-checked on load.
    entry_sha256: Option<String>,
    /// The bundle's sentence bank (ETEXT.DAT, index = sentence id);
    /// empty on older bakes. Feeds the narration subtitles and
    /// the MC1 win message (entries 60/61).
    etext: Vec<String>,
    /// The bundle's 256x256 8bpp parallax-sky bitmap (`sky.bin`);
    /// None on caves (retail loads no cave sky) and older bakes.
    /// Resolved through `palette_rgba` at renderer load.
    sky: Option<Vec<u8>>,
    /// HSPR UI sprites composited to RGBA (spellbook/HUD); None when
    /// the bundle has no UI members (MC2 until its UI track).
    ui: Option<ui::UiAssets>,
    /// Live trigger/portal volumes for the opt-in map overlay.
    map_areas: Vec<mgc_render::MapArea>,
    /// Castle/balloon icon patches for the map marker pass.
    map_icons: entities::MapIcons,
    /// Live icon stamps (own castle/balloons), refreshed per tick.
    map_stamps: Vec<mgc_render::MapStamp>,
    /// MC2 objective-guide targets (blinking marks + steer arrow),
    /// refreshed per tick from the current objective. Empty off-MC2.
    objective_marks: Vec<mgc_render::ObjectiveMark>,
    /// The plausible-spellbook grant set (spell ids), computed from the
    /// campaign jars before this level when the instrument is on; empty
    /// otherwise. Granted into the world after init. MC1 arm.
    plausible_spells: Vec<u8>,
    /// The MC2 plausible-spellbook grants: `(spell, banked_xp)` per
    /// learned spell (MC2's book is XP-driven). Empty off-MC2 or when
    /// the instrument is off. Installed via `mc2_grant_plausible`.
    plausible_book_mc2: Vec<(u8, i32)>,
    /// The level's human spell-availability mask (wizards slot 0,
    /// 0/1 per spell) — the campaign grant law is collected ∩ mask
    /// (remc1 :49229/:49233). None when the package has none.
    allowed_spells: Option<Vec<u8>>,
}

/// Resolve the world's live volumes into map overlay circles: amber =
/// fly-into triggers, red = kill-watchers, cyan = collected-item
/// triggers, violet = portals, green = MC2 stage checkpoints (the
/// authored route, for troubleshooting).
fn map_areas(world: &mgc_sim::engine::world::World) -> Vec<mgc_render::MapArea> {
    use mgc_sim::engine::world::VolumeKind;
    world
        .active_volumes()
        .into_iter()
        .map(|v| mgc_render::MapArea {
            x: v.x,
            z: v.z,
            radius: v.radius,
            color: match v.kind {
                VolumeKind::Proximity => [255, 196, 32],
                VolumeKind::KillWatch => [255, 64, 64],
                VolumeKind::WinTrigger => [64, 208, 255],
                VolumeKind::Portal => [208, 96, 255],
                VolumeKind::Objective => [96, 255, 96],
            },
        })
        .collect()
}

/// The in-level abandon-confirmation prompt: retail MC2's language-
/// table entry 2 with the code-appended "?" (`DrawOkCancelMenu_30A60`,
/// GameUI.cpp:4603 — `sprintf("%s?", langindexbuffer[2])`; English
/// from the decompiler's comment, byte-verification against the
/// packed language DAT still open).
const EXIT_CONFIRM_TEXT: &str = "Abandon level?";

/// The map-texture overlay (entity dots + the optional trigger-area
/// circles) for a level, per the live config.
fn map_overlay(level: &LoadedLevel, cfg: &config::Config) -> mgc_render::MapOverlay {
    mgc_render::MapOverlay {
        dots: level.map_dots.clone(),
        areas: if cfg.render.debug.map_trigger_areas {
            level.map_areas.clone()
        } else {
            Vec::new()
        },
    }
}

/// Lazy miniature capture for the marker icon-swap
/// (`map_marker_icons`): build the icon for any swap-family pose
/// whose type row has none yet, appending the sprite crop below the
/// UI atlas. Lazy because a LOAD-time scan misses most of what the
/// option is for: MC2's authored spell tokens are predominantly
/// disposition-gated (their `dis_id` is not the load sentinel, so
/// they spawn mid-play — player-reported as "MC2 jars stay red
/// dots"), and MC1 death-scatters jars onto levels that author none.
/// Each type row is attempted at most once per sighting and cached
/// forever on success. Returns whether the atlas grew — the caller
/// must re-upload it (`load_ui_atlas`) before the next draw.
fn capture_marker_icons(
    level: &mut LoadedLevel,
    poses: &[mgc_sim::engine::world::LivePose],
) -> bool {
    let Some((sidx, spx)) = level.sprites.as_ref() else {
        return false;
    };
    let Some(ui) = level.ui.as_mut() else {
        return false;
    };
    let mut grew = false;
    for p in poses {
        let bucket = match entities::icon_swap_family(level.game, p.class, p.model) {
            Some(entities::SwapFamily::Jar) => &mut level.map_icons.jar_icons,
            Some(entities::SwapFamily::Static) => &mut level.map_icons.static_icons,
            None => continue,
        };
        if bucket.contains_key(&p.type_index) {
            continue;
        }
        let Some(sprite) = entities::pose_sprite_id(level.game, p.type_index) else {
            continue;
        };
        if let Some(stamp) = ui.append_world_icon(sidx, spx, sprite, &level.palette_rgba) {
            bucket.insert(p.type_index, stamp);
            grew = true;
        }
    }
    grew
}

/// The type rows whose dots the marker icon-swap suppresses: exactly
/// the families an icon was BUILT for (a missing icon keeps its dot),
/// minus jars while expose-jar-spells is on — the debug option
/// outranks the swap there (player ruling: spell icon + the retail
/// red dot, never the jar miniature too).
fn dot_swap_set(level: &LoadedLevel, cfg: &config::Config) -> std::collections::HashSet<u16> {
    let mut set = std::collections::HashSet::new();
    if !cfg.render.enhancement.map_marker_icons {
        return set;
    }
    set.extend(level.map_icons.static_icons.keys().copied());
    if !cfg.render.enhancement.expose_jar_spells {
        set.extend(level.map_icons.jar_icons.keys().copied());
    }
    set
}

/// Resolve the package plus its asset bundle into what the renderer and
/// sim consume. `tileset` overrides MC1's world-set choice: by default
/// MC1 campaign levels use `mc1-temperate` and Hidden Worlds levels
/// `mc1-arctic` (the original's only selector is the Hidden Worlds mode
/// flag — see ROADMAP "Arctic tileset selection").
///
/// `terrain_features` applies the original's load-time entity-driven
/// terrain pass (craters, canyons, walls, building flattening/painting
/// — mgc_sim::engine::features) to the pristine baked terrain, as the engine
/// does. Off = the raw generator output, for comparison renders.
fn load_level(
    level_path: &Path,
    tileset: Option<u8>,
    terrain_features: bool,
    plausible_spellbook: bool,
    prune_owned_jars: bool,
    pool_slots: Option<usize>,
    awake_range: Option<u32>,
) -> Result<LoadedLevel, String> {
    let file =
        std::fs::File::open(level_path).map_err(|e| format!("{}: {e}", level_path.display()))?;
    let package: LevelPackage =
        mgcl::read(file).map_err(|e| format!("{}: {e}", level_path.display()))?;
    if let Some(ov) = &package.meta.overlay {
        println!("level: OVERLAY {ov} — community-modified data, not a faithful run");
    }
    let terrain = package.terrain.as_ref().ok_or_else(|| {
        format!(
            "{}: package has no terrain (bake with the mc2-genlevel oracle available)",
            level_path.display()
        )
    })?;

    // Bundles live in the baked tree next to the per-game level dirs:
    // <baked>/<game>/level-NNN.mgcl, <baked>/assets/<variant>/. MC1's
    // selector is the Hidden Worlds mode flag (temperate/arctic); MC2's
    // is the level's environment (day/night/cave from level.json).
    let baked_root = level_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."));
    let set = tileset.unwrap_or(match package.meta.game {
        Game::HiddenWorlds => 1,
        _ => 0,
    });
    let mut variant = if package.meta.game == Game::MagicCarpet2 {
        // Night splits on the header's gfx_type bit 1 into plain and
        // "fog" graphics (remc2 Level.cpp:890: PALF/BL32F variants).
        match package.header.as_ref().map(|h| (h.map_type, h.gfx_type)) {
            Some((mgc_formats::MapType::Night, g)) if g & 2 != 0 => "mc2-night-fog",
            Some((mgc_formats::MapType::Night, _)) => "mc2-night",
            Some((mgc_formats::MapType::Cave, _)) => "mc2-cave",
            _ => "mc2-day",
        }
    } else if set == 1 {
        "mc1-arctic"
    } else {
        "mc1-temperate"
    };
    // The MC2 map-marker environment (team-colour table + map-type
    // colours) follows the level header, independent of any bundle
    // fallback below.
    let mc2_env = if package.meta.game == Game::MagicCarpet2 {
        match package.header.as_ref().map(|h| h.map_type) {
            Some(mgc_formats::MapType::Night) => entities::Mc2MapEnv::Night,
            Some(mgc_formats::MapType::Cave) => entities::Mc2MapEnv::Cave,
            _ => entities::Mc2MapEnv::Day,
        }
    } else {
        entities::Mc2MapEnv::Day
    };
    if !baked_root.join("assets").join(variant).is_dir() && variant.starts_with("mc2") {
        eprintln!("note: {variant} bundle not baked — using mc1-temperate as a stand-in (rebake)");
        variant = "mc1-temperate";
    }
    let bundle = Bundle::load(&baked_root.join("assets").join(variant))
        .map_err(|e| format!("bundle {variant}: {e}"))?;

    let mut palette = [[0u8; 3]; 256];
    for (i, rgb) in palette.iter_mut().enumerate() {
        rgb.copy_from_slice(&bundle.palette[i][..3]);
    }

    let game = match package.meta.game {
        Game::MagicCarpet1 => "mc1",
        Game::HiddenWorlds => "mc1hw",
        Game::MagicCarpet2 => "mc2",
    };

    let mut height = terrain.height.clone();
    let mut tile_type = terrain.tile_type.clone();
    let mut shading = terrain.shading.clone();
    let mut angle = terrain.angle.clone();
    // MC2 cave second heightmap (empty off-cave / on pre-8 bakes).
    let ceiling = terrain.ceiling.clone().unwrap_or_default();

    // The living world: the load-time feature pass (MC1/HW — MC2
    // terrain is pre-generated, remc2 has no feature event loop),
    // then the init spawns — MC1's disposition-0 sweep / MC2's
    // GenerateEvents passes. Things authored behind triggers
    // (dis_id != 0 / DisId >= 0) stay latent until fired. Needs the
    // shading + angle planes and feature-pass data.
    let game_id = mgc_sim::ids::GameId::from(package.meta.game);
    let is_mc2 = matches!(game_id, mgc_sim::ids::GameId::Mc2);
    let mut world = None;
    let mut world_init = None;
    if terrain_features {
        // Feature-pass assets: every game reads them from its own
        // bundle (mc2 bundles carry SEARCH + the BUILD0-0 footprint
        // bank, plus BLDGPRM for the building creator); an old mc2
        // bake falls back to the mc1-temperate stand-in so the world
        // still lives.
        let mut feature_src = (
            bundle.search.clone(),
            bundle.build_tab.clone(),
            bundle.build_dat.clone(),
        );
        if is_mc2 && (feature_src.1.is_none() || feature_src.2.is_none()) {
            eprintln!("note: mc2 bundle lacks build data — mc1-temperate stand-in (rebake)");
            if let Ok(b) = Bundle::load(&baked_root.join("assets").join("mc1-temperate")) {
                feature_src = (b.search, b.build_tab, b.build_dat);
            }
        }
        match (&shading, &angle, feature_src) {
            (Some(sh), Some(an), (Some(search), Some(build_tab), Some(build_dat))) => {
                let mut assets = mgc_sim::engine::features::FeatureAssets::parse(
                    &search, &build_tab, &build_dat,
                )?;
                if let Some(prm) = bundle.bldgprm.as_deref() {
                    assets = assets.with_bldgprm(prm);
                }
                if let Some(sp) = bundle.spells.as_deref() {
                    assets = assets.with_spells(sp)?;
                }
                // The retail load-time sprite-extents derivation
                // (remc2 EF:44870-44910): collision boxes come from
                // the sprite bitmaps' aspect — the static param
                // table alone leaves most speed_6 at 0 (zero-box).
                // Day-sourced whatever the level's render variant
                // (Bundle::mc2_extent_dims holds the boot-time
                // TMAPS0-0 law).
                if is_mc2 && let Some(dims) = bundle.mc2_extent_dims(&baked_root.join("assets")) {
                    assets = assets.with_mc2_sprite_ext(mgc_sim::mc2::derive_sprite_extents(&dims));
                }
                let seed = package.gen_params.as_ref().map_or(0, |g| g.seed);
                // The MC1 level goal: footer[0] = the required banked
                // percentage of world mana (level offset 38800 —
                // the win check's threshold and the HUD goal tick).
                // MC2's win lives on the stage board instead.
                let win_pct = package
                    .gen_params
                    .as_ref()
                    .and_then(|g| g.footer)
                    .map_or(0, |f| f[0]);
                let (wizards, mc1_count) = rival_configs(package.wizards.as_ref());
                let (mc2_wizards, mc2_count) =
                    mc2_rival_configs(package.wizards.as_ref(), package.header.as_ref());
                let player_count = if is_mc2 { mc2_count } else { mc1_count };
                let stages = package
                    .stages
                    .as_ref()
                    .map(|st| {
                        st.checkpoints
                            .iter()
                            .map(|c| (c.index, c.stage, c.x, c.y))
                            .collect()
                    })
                    .unwrap_or_default();
                let stage_vars = package
                    .stages
                    .as_ref()
                    .map(|st| {
                        st.variables
                            .iter()
                            .map(|v| (v.index, v.stage, v.x, v.y, v.data))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut chassis = game_id.chassis();
                if let Some(n) = pool_slots {
                    chassis.pool_slots = n;
                    println!(
                        "chassis: pool_slots {n} (limit-removing override; \
                         G-class — not a faithful run)"
                    );
                }
                if let Some(tiles) = awake_range {
                    // 0 = always awake; otherwise (tiles·256)² with a
                    // saturate — ≥128 tiles exceeds the torus's max
                    // shortest-wrap distance, so it saturates to
                    // always-awake too.
                    chassis.awake_gate_sq = if tiles == 0 {
                        i32::MAX
                    } else {
                        ((tiles as i64 * 256).pow(2)).min(i32::MAX as i64) as i32
                    };
                    println!(
                        "chassis: awake_range {} (faithful = 24 tiles; \
                         G-class — not a faithful run)",
                        if tiles == 0 {
                            "off (always awake)".to_string()
                        } else {
                            format!("{tiles} tiles")
                        }
                    );
                }
                let init = WorldInit {
                    game: game_id,
                    planes: mgc_sim::engine::features::Planes {
                        height: height.clone(),
                        tile_type: tile_type.clone(),
                        shading: sh.clone(),
                        angle: an.clone(),
                        ceiling: ceiling.clone(),
                    },
                    things: package.things.things.clone(),
                    seed,
                    assets,
                    win_pct,
                    wizards,
                    mc2_wizards,
                    player_count,
                    stages,
                    stage_vars,
                    placeholders: is_mc2,
                    prune_owned_jars,
                    night_shade: is_mc2
                        && matches!(
                            package.header.as_ref().map(|h| h.map_type),
                            Some(mgc_formats::MapType::Night) | Some(mgc_formats::MapType::Cave)
                        ),
                    // The doom-palette bit (gfx_type & 2, the
                    // night-fog variant) gates the (5,10) doomsday
                    // pyramid's ctor (remc2 EF:33968).
                    doom_level: is_mc2
                        && package.header.as_ref().is_some_and(|h| h.gfx_type & 2 != 0),
                    chassis,
                };
                let w = init.build();
                // Truthful seam telemetry at boot: what still serves
                // through the MC1 fallback, and what spawned as a
                // stand-in (empty on MC1/HW by construction).
                let fallbacks = w.verb_fallbacks();
                if !fallbacks.is_empty() {
                    println!("verb fallbacks (MC1 arm serving): {}", fallbacks.join(", "));
                }
                for &(class, model, n) in w.misfits() {
                    println!("misfit: ({class},{model}) x{n} — unported model (placeholder art)");
                }
                // The view starts from the post-feature planes.
                height.copy_from_slice(&w.planes().height);
                tile_type.copy_from_slice(&w.planes().tile_type);
                shading
                    .as_mut()
                    .unwrap()
                    .copy_from_slice(&w.planes().shading);
                angle.as_mut().unwrap().copy_from_slice(&w.planes().angle);
                world = Some(w);
                world_init = Some(init);
            }
            (None, ..) | (_, None, _) => eprintln!(
                "note: package lacks shading/angle planes — terrain features skipped (rebake)"
            ),
            _ => eprintln!(
                "note: feature-pass data missing (bundle search/build) — living world skipped (rebake)"
            ),
        }
    }

    // World entities as billboards + map dots. With a live world, the
    // sim's pose snapshot is the source of truth (sprite types, spawn
    // facing and jitter come from the ported spawn handlers), resolved
    // through the game's own sprite table; without one
    // (--no-terrain-features), every drawable record resolves
    // statically — the comparison mode (MC1/HW only; MC2 has no
    // static resolver).
    let (billboards, map_dots) = {
        let index = bundle.sprites.as_ref().map(|(i, _)| i);
        let dims = |id: u16| {
            index
                .and_then(|i| i.sprites.get(id as usize))
                .map(|s| (s.width, s.height, s.flags))
        };
        match &world {
            Some(w) => {
                let poses = w.live_poses();
                (
                    // Load-time set: no fire exists at level start, so
                    // the enhanced-fire sprite suppression is moot here,
                    // and the dweller-invisibility patch flag likewise
                    // (sync_world re-derives with the real flags; the
                    // wraith's unconditional concealment is in already).
                    entities::billboards_from_poses(game_id, &poses, dims, false, false, false),
                    // No dwelling is claimed at load time, so the
                    // owned-buildings highlight is vacuously off here
                    // (and the blink phase starts low). Icon-swap
                    // suppression is likewise deferred to the first
                    // tick's rebuild (the icon tables build below).
                    entities::map_dots_from_poses(
                        game_id,
                        &poses,
                        &bundle.palette,
                        false,
                        mc2_env,
                        0,
                        &Default::default(),
                    ),
                )
            }
            None if !is_mc2 => (
                entities::billboards(&package.things.things, &height, dims),
                entities::map_dots(&package.things.things, &bundle.palette),
            ),
            None => (Vec::new(), Vec::new()),
        }
    };
    if is_mc2 {
        // Boot telemetry while the MC2 roster is open: how much of
        // the live population resolved to drawables.
        println!(
            "mc2 boot: {} billboards / {} live poses",
            billboards.len(),
            world.as_ref().map_or(0, |w| w.live_poses().len())
        );
    }

    // The original's spawn: the class-3 m4 start marker's position
    // (both games), hovering over the (post-feature) terrain, facing
    // north.
    let start = entities::player_start(game_id, &package.things.things).map(|(x, z)| Flyer {
        x,
        y: entities::ground_at(&height, x, z) + entities::START_HOVER,
        z,
        yaw: 0.0,
        pitch: 0.0,
        ..Flyer::default()
    });

    let ui_assets = bundle.ui_sprites.as_ref().map(|(idx, px)| {
        ui::UiAssets::build(
            idx.clone(),
            px,
            &bundle.palette,
            bundle.blend_lut.as_deref(),
            // MC1 pre-composites its book tiles; MC2's sprite ids map
            // to the selector pane instead (drawn directly).
            !is_mc2,
            bundle.font.as_ref().map(|(i, p)| (i, p.as_slice())),
            bundle.web_sprites.as_ref().map(|(i, p)| (i, p.as_slice())),
        )
    });
    // (The marker icon-swap's miniature tables start EMPTY: families
    // capture lazily at first sighting — `capture_marker_icons` —
    // because MC2's authored tokens are mostly disposition-gated and
    // absent from the load population.)

    // Per-game audio bundle + the music pick. MC2: ONE looping XMI
    // by MapType (Night=GAME1, Day=GAME2, Cave=GAME3 — EF:31441-49,
    // docs/traces/mc2-music-law.md); the redbook tracks are speech,
    // never gameplay music. MC1 stays the INTERIM level cycle until
    // its song-command source data is traced.
    let audio_game = if package.meta.game == Game::MagicCarpet2 {
        "mc2"
    } else {
        "mc1"
    };
    let audio_dir = {
        let d = baked_root
            .join("assets")
            .join(format!("{audio_game}-audio"));
        d.is_dir().then_some(d)
    };
    let music_track = Some(if audio_game == "mc2" {
        match package.header.as_ref().map(|h| h.map_type) {
            Some(mgc_formats::MapType::Night) => "mc2-night".to_string(),
            Some(mgc_formats::MapType::Cave) => "mc2-cave".to_string(),
            _ => "mc2-day".to_string(),
        }
    } else {
        format!("cgame{}", 1 + package.meta.level as usize % 3)
    });

    // Plausible spellbook (playtest instrument): the union of spell
    // jars in the campaign levels before this one. Only scanned when
    // the toggle is on — it reads the sibling `level-NNN.mgcl` files.
    // MC2 arm: the XP-driven book. Reads the same sibling files, but
    // unions class-15 jars → learned set and counts class-14 scrolls →
    // banked XP (see campaign::plausible_spellbook_mc2). Campaign-order
    // prefix (mains + secrets after their parents); a non-campaign
    // level assumes the whole campaign done.
    let plausible_book_mc2 = if plausible_spellbook && package.meta.game == Game::MagicCarpet2 {
        let dir = level_path.parent().unwrap_or(Path::new("."));
        let p = campaign::plausible_spellbook_mc2(dir, &package);
        println!(
            "plausible-spellbook (MC2): {} spell(s) at ~{} XP each from {} scroll(s) across {} \
             campaign level(s) before level {}{}",
            p.grants.len(),
            p.grants.first().map_or(0, |g| g.1),
            p.scroll_count,
            p.scanned_levels.len(),
            package.meta.level,
            if p.skipped_levels.is_empty() {
                String::new()
            } else {
                format!(" (skipped unreadable levels: {:?})", p.skipped_levels)
            },
        );
        p.grants
    } else {
        Vec::new()
    };

    // MC1 arm — and HW, which shares the spellbook system wholesale
    // (same jar class, same per-level availability mask); only the
    // campaign SHAPE differs (25 levels, no skip table), which
    // `campaign::plausible_spellbook` resolves per game.
    let plausible_spells = if plausible_spellbook
        && matches!(package.meta.game, Game::MagicCarpet1 | Game::HiddenWorlds)
    {
        let dir = level_path.parent().unwrap_or(Path::new("."));
        let p = campaign::plausible_spellbook(dir, &package);
        let names: Vec<&str> = p
            .spells
            .iter()
            .map(|&s| mgc_sim::mc1::spells::SpellId(s).name())
            .collect();
        println!(
            "plausible-spellbook: {} spell(s) from {} campaign level(s) before level {} \
             [{}]{}{}",
            p.spells.len(),
            p.scanned_levels.len(),
            package.meta.level,
            names.join(", "),
            if p.skipped_levels.is_empty() {
                String::new()
            } else {
                format!(" (skipped unreadable levels: {:?})", p.skipped_levels)
            },
            if p.masked.is_empty() {
                String::new()
            } else {
                // The level's availability mask (retail :49229) strips
                // these at level start — rediscover them in play.
                format!(
                    " (level mask strips: {})",
                    p.masked
                        .iter()
                        .map(|&s| mgc_sim::mc1::spells::SpellId(s).name())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        );
        p.spells
    } else {
        Vec::new()
    };

    Ok(LoadedLevel {
        view: LevelView {
            tile_type,
            height: height.clone(),
            shading,
            palette,
            tile_colors: bundle.tile_colors,
            shade_lut: bundle.shade_lut,
            atlas: bundle.terrain_atlas.map(|(_, data)| data),
            angle,
            wave: match package.meta.game {
                Game::MagicCarpet2 => mgc_render::WaveMode::Mc2,
                _ => mgc_render::WaveMode::Mc1,
            },
            ceiling: (!ceiling.is_empty()).then(|| ceiling.clone()),
        },
        height,
        label: format!("{game} level {}", package.meta.level),
        game: game_id,
        sprites: bundle.sprites,
        billboards,
        map_dots,
        start,
        map_areas: world.as_ref().map(map_areas).unwrap_or_default(),
        world,
        world_init,
        palette_rgba: bundle.palette,
        mc2_env,
        map_icons: entities::MapIcons {
            // Castle = UI sprite 58+team, balloon = 66+team, all
            // eight teams; remc1 sub_48710 :57230/:57234.
            castle: std::array::from_fn(|t| ui_assets.as_ref().and_then(|u| u.map_stamp(58 + t))),
            balloon: std::array::from_fn(|t| ui_assets.as_ref().and_then(|u| u.map_stamp(66 + t))),
            // Spell icons shrunk to marker size, floating over the
            // jar dot — the expose-jar-spells debug stamps (drawn
            // only when that option is on).
            spell: (0..26u8)
                .map(|s| {
                    let id = ui::spell_icon_sprite(game_id, s)?;
                    let mut st = ui_assets.as_ref().and_then(|u| u.map_stamp(id))?;
                    let f = 12.0 / st.w.max(st.h) as f32;
                    if f < 1.0 {
                        st.w = ((st.w as f32 * f) as u32).max(1);
                        st.h = ((st.h as f32 * f) as u32).max(1);
                    }
                    st.anchor = [0.5, 1.0];
                    Some(st)
                })
                .collect(),
            // The advertised-trigger markers (HUD-bank sprites 83 =
            // red X / 84 = O), CENTERED like retail's minimap blit
            // (GameUI.cpp:2166-72). Sprites 83/84 are baked for BOTH
            // games (docs/FORMAT.md — MC1-native, reused by MC2), so
            // load them whenever present rather than gating on game.
            exit_x: ui_assets
                .as_ref()
                .and_then(|u| u.map_stamp(83))
                .map(|mut st| {
                    st.anchor = [0.5, 0.5];
                    st
                }),
            exit_o: ui_assets
                .as_ref()
                .and_then(|u| u.map_stamp(84))
                .map(|mut st| {
                    st.anchor = [0.5, 0.5];
                    st
                }),
            jar_icons: Default::default(),
            static_icons: Default::default(),
        },
        map_stamps: Vec::new(),
        objective_marks: Vec::new(),
        plausible_book_mc2,
        ui: ui_assets,
        audio_dir,
        music_track,
        level_number: package.meta.level,
        bundle_variant: variant.to_string(),
        entry_sha256: package.meta.source.as_ref().map(|s| s.entry_sha256.clone()),
        etext: bundle.etext.unwrap_or_default(),
        sky: bundle.sky,
        plausible_spells,
        allowed_spells: package
            .wizards
            .as_ref()
            .and_then(|w| w.wizards.first())
            .and_then(|h| h.allowed_spells.clone()),
    })
}

/// The fog-wall overlay cut: world-anchored debug overlays (jar
/// icons, crosshair lock markers — the health bars cut in their own
/// shader) must not reveal what the distance fog hides. `wall` = the full-occlusion distance in tiles
/// (0.95·fog_distance; 0 = fog off, never cut). Torus-wrapped 3D
/// distance like the shaders' wrap-adjusted geometry.
fn fog_cut(cam: &mgc_render::CameraView, x: f32, alt: f32, z: f32, wall: f32) -> bool {
    if wall <= 0.0 {
        return false;
    }
    let wrap = |d: f32| (d + 128.0).rem_euclid(256.0) - 128.0;
    let (dx, dy, dz) = (wrap(x - cam.x), alt - cam.y, wrap(z - cam.z));
    dx * dx + dy * dy + dz * dz > wall * wall
}

/// Greedy word-wrap for the messaging font: split `s` into lines no
/// wider than `max_w` SOURCE pixels (`UiAssets::text_width` units;
/// the caller applies its own scale). A single over-long word gets
/// its own line rather than being broken.
fn wrap_font_text(assets: &ui::UiAssets, s: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if !cur.is_empty() && assets.text_width(&candidate) > max_w {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Per-level FIRST-objective sentence id (remc2 GameUI.cpp:20
/// `IndexLevelText_DB4EE`): objective row k of level L reads ETEXT
/// entry `MC2_OBJECTIVE_TEXT[L] + k`. The display path is
/// `DrawCurrentObjectiveTextbox_30630` (GameUI.cpp:544-575) — an
/// explicit table, NOT a base-48 formula.
const MC2_OBJECTIVE_TEXT: [u16; 25] = [
    48, 54, 60, 66, 69, 72, 77, 79, 86, 92, 97, 102, 105, 110, 115, 118, 124, 126, 131, 133, 136,
    140, 143, 151, 156,
];
/// Per-level COMPLETION-line sentence id (remc2 GameUI.cpp:29
/// `LevelEndText_DB507`).
const MC2_LEVEL_END_TEXT: [u16; 25] = [
    53, 59, 65, 68, 71, 76, 78, 85, 91, 96, 101, 104, 109, 114, 117, 123, 125, 130, 132, 135, 139,
    142, 150, 156, 158,
];
/// Subtitle dwell: retail parks the objective textbox for 200 frames
/// (`byte_counter_current_objective_box_0x36E04 = 200`). Counted on
/// the 24Hz WALL clock, not sim ticks — the line overtitles a
/// wall-time voiceover, and game speed must not cut it short or park
/// it (same law as the notification toast).
const SUBTITLE_TICKS: u16 = 200;

/// The ETEXT sentence behind one speech cue: `lvl` = 0-based level,
/// `seg` = the CD segment the sim's trigger ramp handed over (N+1 =
/// objective row N, 9 = level complete). Special levels 30-34 show the
/// generic in-progress/complete lines 51/101 (GameUI.cpp:556-561).
fn mc2_narration_etext(lvl: u32, seg: u8) -> Option<usize> {
    if (30..=34).contains(&lvl) {
        return Some(if seg == 9 { 101 } else { 51 });
    }
    let lvl = lvl as usize;
    if lvl >= MC2_OBJECTIVE_TEXT.len() {
        return None;
    }
    match seg {
        9 => Some(MC2_LEVEL_END_TEXT[lvl] as usize),
        1..=8 => Some(MC2_OBJECTIVE_TEXT[lvl] as usize + (seg as usize - 1)),
        _ => None,
    }
}

/// The config→sim mappings for the flight-control tiers (the config
/// enums are named for the user-facing tiers, the sim enums for the
/// implementations).
fn sim_thrust(t: config::ThrustModel) -> mgc_sim::ThrustModel {
    match t {
        config::ThrustModel::Classic => mgc_sim::ThrustModel::Mc1,
        config::ThrustModel::Enhanced => mgc_sim::ThrustModel::Enhanced,
    }
}

fn sim_altitude(a: config::AltitudeModel) -> mgc_sim::AltitudeModel {
    match a {
        config::AltitudeModel::Classic => mgc_sim::AltitudeModel::Faithful,
        config::AltitudeModel::Enhanced => mgc_sim::AltitudeModel::ExtendedLift,
    }
}

/// X11 focus HAMMER for the boot grab. winit's `focus_window` only
/// PETITIONS the WM (`_NET_ACTIVE_WINDOW` client message, timestamp
/// 0) — focus-stealing-prevention WMs (compiz among them) are
/// entitled to ignore that, the launch terminal keeps focus, no
/// `Focused(true)` ever fires, and the focus-gated boot grab stays
/// dormant. `XSetInputFocus` is the protocol PRIMITIVE the WM cannot
/// veto — it is how SDL raises windows, and the reason "SDL would
/// have taken care of it". Throwaway connection: input focus is
/// server-global state any client may set. Errors ignored — the
/// bounded caller retries. No-op off X11 (Wayland activates new
/// toplevels compositor-side and offers no client-side equivalent).
#[cfg(all(
    unix,
    not(any(
        target_os = "redox",
        target_family = "wasm",
        target_os = "android",
        target_os = "ios",
        target_os = "macos"
    ))
))]
fn x11_force_focus(window: &Window) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, InputFocus};
    let xid = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xlib(h)) => h.window as u32,
        Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
        _ => return,
    };
    if let Ok((conn, _)) = x11rb::connect(None) {
        let _ = conn.set_input_focus(InputFocus::PARENT, xid, x11rb::CURRENT_TIME);
        let _ = conn.flush();
    }
}

#[cfg(not(all(
    unix,
    not(any(
        target_os = "redox",
        target_family = "wasm",
        target_os = "android",
        target_os = "ios",
        target_os = "macos"
    ))
)))]
fn x11_force_focus(_window: &Window) {}

/// Currently-held key axes, sampled into a `FlightInput` per tick.
#[derive(Default)]
struct HeldKeys {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    turn_left: bool,
    turn_right: bool,
    pitch_up: bool,
    pitch_down: bool,
}

/// Mouse deltas accumulated since the last tick.
#[derive(Default)]
struct MouseAccum {
    yaw: f32,
    pitch: f32,
}

/// The MC1 model's virtual stick: mouse motion integrates into a
/// POSITION offset from center (the original reads the DOS cursor's
/// screen offset, ±127 per axis — an airplane-stick input: deflection
/// = turn rate, re-center to fly straight). Kept in floats app-side;
/// sampled to the sim's i16 pair each tick.
#[derive(Default)]
struct VirtualStick {
    x: f32,
    y: f32,
}

/// One running gameplay instance: the loaded level plus its living
/// sim. The frontend (main menu / world map) is a LOADER of these: a
/// session is constructed when a level launches and dropped when play
/// returns to the hub — nothing of the level survives underneath the
/// frontend (no frozen sim, no ambient audio, no stale renderer
/// world).
struct Session {
    level: LoadedLevel,
    sim: Simulation,
    prev_flyer: Flyer,
    /// The last two sim-tick pose snapshots (smooth_motion): the
    /// renderer draws entities lerped prev→cur at the frame's
    /// accumulator fraction — the same one-tick-behind timeline the
    /// camera has always run (`prev_flyer`). Empty while the toggle
    /// is off or fewer than two ticks have run.
    pose_prev: Vec<mgc_sim::engine::world::LivePose>,
    pose_cur: Vec<mgc_sim::engine::world::LivePose>,
    /// PROTOTYPE fire effect: the render-side blast ledger — tracks
    /// live `(10,17)` blast drivers and keeps aging them after they
    /// despawn, so the crater fire/smoke choreography outlives the
    /// driver. Updated once per sim tick alongside the pose snapshots.
    fire_blasts: entities::BlastLedger,
    /// Latched lightning strikes aging across frames (the enhanced-
    /// lightning envelope). Updated once per sim tick like the blasts.
    bolts: entities::BoltLedger,
}

/// Which surface owns the frame: a running level, or one of the
/// frontend screens. The frontend states hold NO session — the level
/// is torn down on the way out and rebuilt on the next launch.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    /// Gameplay (`session` is Some).
    Level,
    /// The campaign main menu (MC2 temple / MC1 globe).
    Menu,
    /// The MC2 world-map hub.
    Map,
    /// A full-screen FMV run (intro / cutscene / outro).
    Movie,
}

/// What follows a finished (or skipped) FMV chain. Every movie in the
/// game covers a transition, so the player always hands back to one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterMovie {
    /// Back to the campaign's main menu (the intro chain at launch).
    Menu,
    /// Back to the MC2 world map (the cutscenes sit between a
    /// finished level and the map).
    Map,
    /// Leave the game — the outro is the last thing either campaign
    /// shows.
    Quit,
}

/// Which atlas currently occupies the renderer's single UI-atlas
/// slot. Every uploader stamps it; every screen re-uploads only when
/// it isn't the owner.
#[derive(Clone, Copy, PartialEq)]
enum UiAtlas {
    /// Nothing uploaded yet.
    None,
    /// The session level's HSPR UI atlas.
    Level,
    /// The MC2 world-map screen's atlas.
    MapScreen,
    /// The MC2 temple main menu's atlas.
    MenuMc2,
    /// The MC1 menu's CPU-composed frame (re-uploaded every frame —
    /// the animations live in the pixels).
    MenuMc1,
    /// The frontend-owned level-UI atlas (the P options menu over a
    /// frontend screen — fonts + panel art without a session).
    FrontendUi,
    /// The FMV player's resolved frame (re-uploaded as it decodes).
    Movie,
}

/// The running session, mutably — the gameplay paths' accessor.
/// A macro (not a method) so field borrows stay disjoint: the borrow
/// is rooted at `self.session`, leaving `self.cfg`, `self.audio`,
/// `self.renderer`… free.
macro_rules! sess {
    ($s:expr) => {
        $s.session
            .as_deref_mut()
            .expect("gameplay path without a session")
    };
}
/// Immutable counterpart of [`sess!`].
macro_rules! sess_ref {
    ($s:expr) => {
        $s.session
            .as_deref()
            .expect("gameplay path without a session")
    };
}
/// The level-UI asset bank visible from the current mode: the
/// session's, or the frontend-owned copy (P menu without a session).
macro_rules! ui_assets {
    ($s:expr) => {
        match &$s.session {
            Some(sess) => sess.level.ui.as_ref(),
            None => $s.frontend_ui.as_ref(),
        }
    };
}

struct App {
    /// The running gameplay instance; None while a frontend screen
    /// owns the app (campaign boot, between levels).
    session: Option<Box<Session>>,
    /// The single resolved options source of truth (defaults + config
    /// file + CLI overrides, merged in `main`). Every option is read
    /// live off this struct; runtime keys mutate it and re-apply, so a
    /// future in-game menu drives the exact same path. See the
    /// `settings` registry for the option taxonomy.
    cfg: config::Config,
    /// Pickable-jar positions `(x, alt, z, spell)` for the floating
    /// main-view icons; rebuilt with the pose snapshot, empty when
    /// `render.enhancement.expose_jar_spells` is off.
    jar_markers: Vec<(f32, f32, f32, u8)>,
    /// Rival snapshots from the previous and current entity refresh —
    /// the rival tag's smooth-motion pair (the tag rides the sub-tick
    /// `alpha` lerp like the sprite it floats over; a raw tick anchor
    /// steps while the interpolated sprite glides).
    rival_tags_prev: Vec<mgc_sim::engine::world::RivalView>,
    rival_tags_cur: Vec<mgc_sim::engine::world::RivalView>,
    /// Ticks since the mouse last moved — the retail MC2 "fly
    /// assistant" (PlayerInput.cpp:2001-09): 0x30 idle polls with no
    /// action pending recenter the cursor, i.e. our virtual stick.
    /// Without it the grabbed stick rests wherever the last flick
    /// left it — a permanent invisible deflection (a parked stick_y
    /// of 5+ units defeats the sine-LUT truncation that makes true
    /// near-level flight hold altitude). Faithful for MC2;
    /// enhancement-class in MC1/HW like Backspace.
    stick_idle_ticks: u16,
    /// The live narration subtitle: sentence + remaining dwell frames
    /// (retail parks the objective textbox for 200 —
    /// `byte_counter_current_objective_box_0x36E04`, EF:22000). Set
    /// when a speech cue fires (per `audio.subtitles`), counted down
    /// on the 24Hz wall clock alongside the toast (never sim ticks —
    /// it overtitles wall-time speech), drawn centered over the view.
    subtitle: Option<(String, u16)>,
    /// Space pressed since the last sim tick (respawn confirm).
    pending_full_stop: bool,
    pending_respawn: bool,
    /// Shift+L pressed since the last sim tick (castle demolish).
    pending_demolish: bool,
    /// Which spell-selection surfaces are live (config
    /// `spell_selector` resolved against the running game): the MC1
    /// map-screen spellbook and/or the MC2 CTRL-hold pane.
    selector: config::SelectorSurfaces,
    /// CTRL currently held (the MC2 selector pane is hold-to-show,
    /// release-to-close — remc2 PI:505/PI:895).
    ctrl_held: bool,
    /// Whether the cursor was grabbed when CTRL went down, so release
    /// restores THAT state instead of force-grabbing (the cursor may
    /// have been deliberately freed via Escape or focus loss).
    ctrl_grab_restore: bool,
    /// The pane's per-game shape; None when `selector.ctrl_pane` is
    /// off.
    pane: Option<ui::SelectorPane>,
    /// Pane hit under the cursor, refreshed per frame while it's up.
    selector_hover: ui::SelectorHover,
    /// A held pane click: (grid slot, hand 0=L/1=R). The flyout
    /// live-tracks the hovered level until release commits it.
    selector_drag: Option<(usize, u8)>,
    /// Pane spell id last bound to each hand (the pane's corner
    /// tags; MC2 only — MC1 reads the loadout directly).
    pane_bound: [Option<u8>; 2],
    /// Per-spell SELECTED LEVEL (MC2 mechanic, `array_0x437` in the
    /// original: one persistent level per spell, reused by every
    /// selection route). Indexed by pane spell id; MC1 spells are
    /// single-level so it stays all-zero there. App-side until the
    /// MC2 spell column lands sim-side.
    spell_levels: [u8; 26],
    /// Sim tick of the last map-texture recompose (dots/blink are
    /// tick-derived, so update_map runs per tick, not per frame).
    last_map_tick: Option<u64>,
    /// P-key pause: the sim clock freezes, rendering and UI stay live.
    paused: bool,
    /// The in-game options menu. On the frontend screens P opens this
    /// directly; IN A LEVEL it is a second layer opened from the
    /// mini-menu's Options row. None = closed.
    menu: Option<menu::MenuState>,
    /// The in-level pause mini-menu (save/load/options). Present
    /// exactly while paused in a level.
    ///
    /// It deliberately does NOT gate input the way `menu` does:
    /// retail keeps the whole input path live during pause, so spell
    /// selection and the big map stay usable underneath (see the
    /// `minimenu` module docs).
    mini: Option<minimenu::MiniMenu>,
    /// Whether the cursor was grabbed when the menu opened, so close
    /// restores THAT state.
    menu_grab_restore: bool,
    /// The option registry, built once (the menu's row source; the
    /// startup summary rebuilds its own).
    specs: Vec<settings::Spec>,
    /// The overlay config file the menu persists into
    /// (`mgcarpet.json` or the `--config` path).
    cfg_file: PathBuf,
    /// Own castle position in tile units (the guide-path target),
    /// refreshed from the pose set.
    castle_pos: Option<(f32, f32)>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    keys: HeldKeys,
    mouse: MouseAccum,
    stick: VirtualStick,
    /// Last tick's (strafe-left, strafe-right) held pair — the MC2
    /// barrel-roll edge detect (retail's prev-frame strafe byte
    /// `byteindex_183`, PlayerInput.cpp:2088-97): the roll fires only
    /// when both go down from NEITHER held.
    prev_strafe: (bool, bool),
    /// Raw mouse-X counts accumulated since the last sim tick
    /// (unscaled device units) — the barrel roll's abort sense.
    roll_dx: f32,
    /// Left/right button held while grabbed: the two casting hands.
    fire_held: bool,
    fire_right_held: bool,
    grabbed: bool,
    /// A `--level` boot wants the pointer captured, but grabs against
    /// a window the platform has not finished focusing/mapping fail
    /// (X11/Wayland defer constraints; some WMs hold their own grab
    /// through placement). Armed in `resumed`, retried on focus gain
    /// and per frame (has_focus-gated) until a grab STICKS; cleared
    /// by success or by any deliberate free.
    boot_grab: bool,
    /// Remaining focus-activation re-asks for the boot grab: the
    /// `focus_window()` in `resumed` can race the WM's ASYNC map (it
    /// no-ops on a not-yet-visible window), so the frame loop re-asks
    /// while this counts down — bounded, so a WM that ignores
    /// activation is pestered for under a second, not forever.
    boot_focus_asks: u8,
    /// Cursor position in window pixels (book-screen interactions).
    cursor: (f32, f32),
    /// Spell under the cursor on the book screen (display hit test,
    /// refreshed each frame the book is open).
    hovered: Option<mgc_sim::mc1::spells::SpellId>,
    /// Quick-key bindings 1..9,0 → spell id (session-local; set in the
    /// book by hovering + pressing a digit, or auto-assigned on spell
    /// acquisition like retail, :64858-67). Manual rebinding beyond
    /// the book's Ctrl+]+digit chord is our enhancement.
    quick_binds: [Option<u8>; 10],
    /// Last tick's owned-spell set — the acquisition edge detector
    /// feeding the retail quickselect auto-assign (app-side only,
    /// never part of the sim hash).
    prev_owned: [bool; 24],
    /// Equip requests to feed the next sim tick (LMB hand, RMB hand).
    pending_equip: (Option<u8>, Option<u8>),
    /// Pending MC2 pane commit: (spell, tier, hand) — the sim's
    /// PlayerAction 0x1F/0x20 equivalent (PlayerCommand.mc2_select).
    pending_mc2_select: Option<(u8, u8, u8)>,
    /// Pending cycle-ring write: (spell, 0/1/2) — retail cmd 0x26,
    /// the pane's SHIFT+click toggle (PlayerCommand.spell_ring).
    pending_ring: Option<(u8, u8)>,
    /// Mouse-wheel remainder for the wheel spell-cycling enhancement
    /// (line deltas accumulate; each whole ±1 cycles one step).
    wheel_accum: f32,
    shift_held: bool,
    /// CTRL as a plain MODIFIER, distinct from [`Self::ctrl_held`].
    /// That one is the selector PANE's hold latch and carries the
    /// pointer-grab release with it; it is also only tracked when
    /// `pane.is_some()`, which is FALSE for default MC1
    /// (`SpellSelector::Auto` resolves `ctrl_pane = false` there).
    /// The retail quick-key chord is CTRL+digit and belongs to MC1
    /// above all, so it needs a latch that exists in every game and
    /// config. Tracked before the pane's early `return`.
    ctrl_mod: bool,
    /// Alt latch, for the Alt+Enter fullscreen combo. Tracked at the
    /// very top of the key handler so no early `return` can strand it.
    alt_held: bool,
    last_frame: std::time::Instant,
    accumulator: f32,
    /// Toast-decay clock: WALL seconds toward the next 24Hz
    /// notification frame. Deliberately NOT speed-scaled — retail
    /// ages the message line per rendered frame, not per game turn
    /// (see the tick loop).
    toast_accumulator: f32,
    /// PROTOTYPE fire effect: wall-clock seconds, drives flame
    /// turbulence/shimmer (advances even while paused).
    effect_time: f32,
    /// The `enhanced_fire()` value last applied to the renderer's
    /// billboard/fire sets — flipping the option in the PAUSE menu
    /// must swap sprites/particles immediately, and while paused
    /// neither the tick path nor apply_smooth_motion runs to do it.
    fire_applied: Option<bool>,
    /// The `enhanced_lightning()` value last applied — same
    /// paused-flip rebuild law as `fire_applied`.
    lightning_applied: Option<bool>,
    /// FPS-overlay accounting: frames and wall time since the last
    /// readout refresh, plus the rendered text (recomputed every
    /// half-second so the number is readable, not a blur).
    fps_frames: u32,
    fps_elapsed: f32,
    fps_text: String,
    /// Running pool-exhaustion drop count for this level (the
    /// limit-removing telemetry's playthrough readout).
    pool_dropped_total: u32,
    /// Misfit-ledger entries already reported (the spawn seam's
    /// graceful-degradation telemetry — unknown (class, model)
    /// things; mgc-sim ROADMAP "MULTI-GAME ARCHITECTURE" Phase 2).
    misfits_reported: usize,
    /// Audio runtime (None in headless paths / when opening failed).
    audio: Option<mgc_audio::Audio>,
    /// The end-of-game fadeout, armed when the sim reports the level
    /// WON (`World::won`): alpha 0→1 over ~0.8 s, then the app exits
    /// (deliberate ending: no stats screen, no menu return). MC2's
    /// ending already fades sim-side (`World::end_fade`); this rides
    /// on top so both games leave through the same door. In campaign
    /// mode the full-black beat routes to the next level instead of
    /// exiting (`CampaignRun::next`).
    quit_fade: Option<f32>,
    /// The running campaign (`--campaign`); None = single-level mode
    /// (the fade exits as before).
    campaign: Option<CampaignRun>,
    /// The initial `load_level` parameters, for campaign switches.
    launch: LaunchParams,
    /// `--replay`: the opened take waiting for the boot session
    /// (consumed by `attach_replay_record`), then the live driver.
    replay_pending: Option<replay::ReplayFile>,
    replay: Option<replay::ReplayDriver>,
    /// `--record`: the destination path, then the live recorder.
    record_path: Option<PathBuf>,
    recorder: Option<replay::PortRecorder>,
    /// The MC2 world-map screen assets (lazy-loaded on first entry;
    /// stays None when the mc2-ui bundle is absent).
    worldmap: Option<worldmap::WorldMap>,
    /// The MC2 main menu (temple screen) — owns the frame while
    /// `screen == Screen::Menu` on an MC2 campaign.
    mainmenu: Option<frontend::MainMenu>,
    /// The MC1/HW frontend (the 320×200 globe menu).
    mc1menu: Option<frontend_mc1::Mc1Menu>,
    /// The running FMV chain — owns the frame while
    /// `screen == Screen::Movie`, and is dropped when it ends.
    movie: Option<movie::MoviePlayer>,
    /// The launch intro is still owed (consumed on the first frontend
    /// frame — the window has to exist before a movie can play).
    boot_intro: bool,
    /// Which MC2 cutscenes have played this run (retail's
    /// `overplayed_5`, which is per-process and never persisted).
    cutscenes_played: [bool; 5],
    /// What to do once the chain finishes or the player skips it.
    movie_then: AfterMovie,
    /// Which surface owns the frame (see [`Screen`]). Frontend
    /// screens hold no session — the level is constructed on launch
    /// and torn down on exit.
    screen: Screen,
    /// Owner of the renderer's single UI-atlas slot (see [`UiAtlas`]).
    ui_atlas: UiAtlas,
    /// The frontend's 24 Hz mixer-pump accumulator: with no sim
    /// ticking, `Audio::tick` (the flush that actually PLAYS
    /// requested samples, runs fades and recovers the narration
    /// duck) is driven from wall time instead. Without it the map's
    /// ambient bursts and the menu clicks sit requested-but-silent.
    frontend_audio_accum: f32,
    /// Frontend-owned level-UI assets (fonts + panel art) for the P
    /// options menu while no session exists. Populated from a torn-
    /// down session's assets, or lazily from the game's variant
    /// bundle.
    frontend_ui: Option<ui::UiAssets>,
    /// The won-edge latch: the completion bookkeeping ran for this
    /// level. Without it a completed level's `won()` refires every
    /// frame once the fade is consumed (the map screen clears it),
    /// re-running the save.
    won_handled: bool,
    /// The in-level abandon-confirmation dialog is up: the retail MC2
    /// "Abandon level?" OK/Cancel dialog, reused for MC1/single-level
    /// which retail left unguarded (deliberate). Retail-faithful
    /// modality: the world KEEPS RUNNING beneath it, the dialog only
    /// owns the input. Esc/Cancel stays, Enter/OK abandons to the hub
    /// (or exits, single-level).
    exit_confirm: bool,
}

impl App {
    fn new(
        level: Option<LoadedLevel>,
        cfg: config::Config,
        cfg_file: PathBuf,
        campaign: Option<CampaignRun>,
        launch: LaunchParams,
        replay_boot: Option<replay::ReplayFile>,
        record_path: Option<PathBuf>,
    ) -> Self {
        // The running game's identity is known without a level: the
        // campaign id (a campaign boots to its frontend, level-less).
        let has_campaign = campaign.is_some();
        let is_mc2 = match (&level, &campaign) {
            (Some(l), _) => matches!(l.game, mgc_sim::ids::GameId::Mc2),
            (None, Some(run)) => run.id == campaign::CampaignId::Mc2,
            (None, None) => false,
        };
        // Audio: open the device and load the GAME's audio bundle —
        // the bundle is per-game, owned by the app across sessions
        // (the frontend needs music + click samples with no level
        // alive). Any failure degrades to silence, never to an
        // unplayable game.
        let mut audio = None;
        if cfg.audio.sound || cfg.audio.music {
            let mut a = mgc_audio::Audio::open();
            a.set_prefer_gm(cfg.audio.arrangement.prefer_gm());
            if is_mc2 {
                a.set_mc2_danger_ramp();
            }
            let audio_dir = match &level {
                Some(l) => l.audio_dir.clone(),
                None => {
                    let d = get_baked_directory().join("assets").join(if is_mc2 {
                        "mc2-audio"
                    } else {
                        "mc1-audio"
                    });
                    d.is_dir().then_some(d)
                }
            };
            if let Some(dir) = &audio_dir {
                if let Err(e) = a.load_bundle(dir, 0) {
                    eprintln!("note: audio bundle: {e}");
                }
            } else {
                eprintln!("note: no audio bundle baked — sound effects disabled (rebake)");
            }
            a.set_volumes(
                if cfg.audio.sound {
                    cfg.audio.sfx_volume
                } else {
                    0.0
                },
                if cfg.audio.music {
                    cfg.audio.music_volume
                } else {
                    0.0
                },
            );
            audio = Some(a);
        }
        // Which spell-selection surfaces are live, resolved against
        // the running game (re-resolved on every session install).
        let selector = cfg.gameplay.enhancement.spell_selector.resolve(is_mc2);
        let pane = selector.ctrl_pane.then(|| {
            if is_mc2 {
                ui::SelectorPane::mc2()
            } else {
                ui::SelectorPane::mc1()
            }
        });
        let mut app = Self {
            session: None,
            cfg,
            jar_markers: Vec::new(),
            rival_tags_prev: Vec::new(),
            rival_tags_cur: Vec::new(),
            stick_idle_ticks: 0,
            subtitle: None,
            pending_full_stop: false,
            pending_respawn: false,
            pending_demolish: false,
            selector,
            ctrl_held: false,
            ctrl_grab_restore: false,
            pane,
            selector_hover: ui::SelectorHover::default(),
            selector_drag: None,
            pane_bound: [None; 2],
            spell_levels: [0; 26],
            last_map_tick: None,
            paused: false,
            menu: None,
            mini: None,
            menu_grab_restore: false,
            specs: settings::registry(),
            cfg_file,
            castle_pos: None,
            window: None,
            renderer: None,
            keys: HeldKeys::default(),
            mouse: MouseAccum::default(),
            stick: VirtualStick::default(),
            prev_strafe: (false, false),
            roll_dx: 0.0,
            fire_held: false,
            fire_right_held: false,
            grabbed: false,
            boot_grab: false,
            boot_focus_asks: 0,
            cursor: (0.0, 0.0),
            hovered: None,
            quick_binds: [None; 10],
            prev_owned: [false; 24],
            pending_equip: (None, None),
            pending_mc2_select: None,
            pending_ring: None,
            wheel_accum: 0.0,
            shift_held: false,
            ctrl_mod: false,
            alt_held: false,
            last_frame: std::time::Instant::now(),
            accumulator: 0.0,
            toast_accumulator: 0.0,
            // MGC_FIRE_T0 (seconds): pre-seed the prototype fire
            // clock. This is how the sticky "corrupt fire" was
            // confirmed live (T0=36000 → corrupt from launch: the
            // driver's sin() range-reduction cliff). The runtime
            // wrap in redraw_requested folds any T0 within one
            // frame now — the flag remains for regression checks
            // (corrupt pre-wrap, clean post-wrap) and near-wrap
            // testing (T0=599.9).
            effect_time: std::env::var("MGC_FIRE_T0")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            fire_applied: None,
            lightning_applied: None,
            fps_frames: 0,
            fps_elapsed: 0.0,
            fps_text: String::new(),
            pool_dropped_total: 0,
            misfits_reported: 0,
            audio,
            quit_fade: None,
            // Every campaign boots to its retail MAIN MENU (MC2: the
            // temple; MC1/HW: the globe menu) with NO level loaded —
            // the frontend is the loader; a session is constructed
            // when the player launches one.
            screen: if has_campaign {
                Screen::Menu
            } else {
                Screen::Level
            },
            campaign,
            launch,
            replay_pending: replay_boot,
            replay: None,
            record_path,
            recorder: None,
            worldmap: None,
            mainmenu: None,
            mc1menu: None,
            movie: None,
            // A campaign booting to its menu gets the intro chain; a
            // direct `--level` launch does not, matching retail's own
            // level shortcut.
            boot_intro: has_campaign,
            cutscenes_played: [false; 5],
            movie_then: AfterMovie::Menu,
            ui_atlas: UiAtlas::None,
            frontend_audio_accum: 0.0,
            frontend_ui: None,
            won_handled: false,
            exit_confirm: false,
        };
        match level {
            // Single-level mode boots straight into its session.
            Some(l) => {
                app.install_level(l);
                app.attach_replay_record();
            }
            // Campaign boot: frontend only — its dedicated menu music
            // starts now (MC1 `csetup`, MC2 the SETUP render). NOT when
            // the intro chain is about to play: the movie owns the
            // audio from the first frame, and starting the menu track
            // here only to stop it on the next frame is audible as a
            // blip of menu MIDI under the opening (player-reported).
            // The chain hands back to `enter_main_menu`, which starts
            // it properly.
            None if !app.boot_intro => app.frontend_music(),
            None => {}
        }
        app
    }

    /// Turn the boot-time `--replay`/`--record` requests into the live
    /// driver/recorder, once the boot session exists. Port takes
    /// restore their embedded start snapshot first (the resume path's
    /// shape — the restore replaces the world wholesale, so the render
    /// mirrors re-seed).
    fn attach_replay_record(&mut self) {
        if let Some(mut file) = self.replay_pending.take() {
            let snap = file.snapshot.take();
            let res = (|| -> Result<replay::ReplayDriver, String> {
                let sess = self.session.as_deref_mut().ok_or("level did not install")?;
                if let Some(snap) = &snap {
                    sess.sim.restore(snap).map_err(|e| e.to_string())?;
                    sess.prev_flyer = sess.sim.flyer;
                    sess.pose_prev = Vec::new();
                    sess.pose_cur = Vec::new();
                    sess.fire_blasts.clear();
                    sess.bolts.clear();
                    if let Some(w) = sess.sim.world.as_mut() {
                        w.terrain_dirty = true;
                        w.entities_dirty = true;
                    }
                }
                replay::ReplayDriver::install(file, &mut sess.sim)
            })();
            match res {
                Ok(d) => self.replay = Some(d),
                Err(e) => eprintln!("error: replay: {e}"),
            }
        }
        if let Some(path) = self.record_path.take() {
            if self.replay.is_some() {
                eprintln!("record: disabled — a replay is the input source");
                return;
            }
            let res = {
                let cfg = &self.cfg;
                let (thrust, altitude) = (
                    sim_thrust(cfg.controls.models.thrust),
                    sim_altitude(cfg.controls.models.altitude),
                );
                let sess = self.session.as_deref_mut();
                sess.ok_or("level did not install".to_string())
                    .and_then(|sess| {
                        let game = sess.level.game;
                        let level = sess.level.level_number;
                        replay::PortRecorder::begin(
                            &path,
                            &sess.sim,
                            match game {
                                mgc_sim::ids::GameId::Mc1 => "mc1",
                                mgc_sim::ids::GameId::Mc1Hw => "mc1hw",
                                mgc_sim::ids::GameId::Mc2 => "mc2",
                            },
                            level,
                            thrust,
                            altitude,
                        )
                    })
            };
            match res {
                Ok(r) => {
                    println!(
                        "record: {} (input:\"exact\" + hash channel)",
                        path.display()
                    );
                    self.recorder = Some(r);
                }
                Err(e) => eprintln!("error: record: {e}"),
            }
        }
    }

    /// Finalize a live recording (level switch, app exit) — the zstd
    /// stream needs its frame end to reopen cleanly.
    fn finish_recorder(&mut self) {
        if let Some(r) = self.recorder.take() {
            let (path, n, res) = r.finish();
            match res {
                Ok(()) => println!("record: {} — {n} tick(s) written", path.display()),
                Err(e) => eprintln!("record: {}: {e}", path.display()),
            }
        }
    }

    /// The HUD blends over the sky (MC1's always-on look) vs opaque
    /// solid panels (the readable default). Derived live from
    /// `render.enhancement.hud_transparency`.
    fn hud_transparent(&self) -> bool {
        self.cfg.render.enhancement.hud_transparency.transparent()
    }

    /// Alt+Enter. Unlike the `option_key` table (session-only by
    /// design), this one PERSISTS: which screen shape you play in is a
    /// property of the machine, not of the run, and having it revert on
    /// the next launch would be a bug rather than a nicety.
    fn toggle_fullscreen(&mut self) {
        self.cfg.render.preference.fullscreen = !self.cfg.render.preference.fullscreen;
        self.apply_fullscreen();
        self.reassert_pointer();
        if let Some(spec) = settings::registry()
            .iter()
            .find(|s| s.cfg_path == "render.preference.fullscreen")
        {
            self.persist_option(spec);
        }
        let on = self.cfg.render.preference.fullscreen;
        if let Some(w) = self
            .session
            .as_deref_mut()
            .and_then(|s| s.sim.world.as_mut())
        {
            w.notify_option(format!("Fullscreen {}", if on { "on" } else { "off" }));
        }
    }

    /// Push `render.preference.fullscreen` onto the live window.
    /// BORDERLESS, never exclusive: `Fullscreen::Borderless(None)`
    /// takes the monitor the window currently sits on and keeps the
    /// desktop video mode, so there is no mode switch to flicker
    /// through and alt-tab costs nothing. The surface follows through
    /// the `Resized` event winit posts for the size change — nothing
    /// here touches the renderer.
    fn apply_fullscreen(&self) {
        if let Some(window) = &self.window {
            window.set_fullscreen(
                self.cfg
                    .render
                    .preference
                    .fullscreen
                    .then_some(winit::window::Fullscreen::Borderless(None)),
            );
        }
    }

    /// The running game's identity, session or not (the campaign id
    /// carries it while the frontend is level-less).
    fn is_mc2(&self) -> bool {
        match (&self.session, &self.campaign) {
            (Some(sess), _) => matches!(sess.level.game, mgc_sim::ids::GameId::Mc2),
            (None, Some(run)) => run.id == campaign::CampaignId::Mc2,
            (None, None) => false,
        }
    }

    /// The frontend's dedicated menu track: MC1 `csetup.hmp` (music
    /// bank 0, song 4 — remc1 :58992 `sub_5D290_5D7A0(4)`; the same
    /// SETUP law as MC2), MC2 the `mc2-menu` SETUP render (retail
    /// `StartMusic_8E160(4)`).
    fn frontend_track(&self) -> &'static str {
        if self.is_mc2() { "mc2-menu" } else { "csetup" }
    }

    /// Start the frontend's menu music (menu and map share the set —
    /// retail keeps it playing across both).
    fn frontend_music(&mut self) {
        if !self.cfg.audio.music {
            return;
        }
        let track = self.frontend_track();
        if let Some(a) = &mut self.audio
            && let Err(e) = a.play_music(track, true)
        {
            eprintln!("note: menu music: {e}");
        }
    }

    /// Tear the running gameplay session down on the way into a
    /// frontend screen: drop the sim + level, cut every level sound
    /// (ambient loops, sfx channels, narration — the level's audio
    /// dies at its boundary), clear the renderer's world, and keep
    /// the level-UI assets for the frontend's options menu. The
    /// frontend menu music takes over.
    fn teardown_session(&mut self) {
        let Some(sess) = self.session.take() else {
            return;
        };
        // The per-game UI bank survives the session (fonts/panels for
        // the P menu — identical across a game's levels).
        if sess.level.ui.is_some() {
            self.frontend_ui = sess.level.ui;
        }
        drop(sess.sim);
        if let Some(a) = &mut self.audio {
            a.stop_sounds();
            a.stop_speech();
            // The danger-music wish dies with the sim that raised it
            // (the frontend pump would otherwise keep the ramp armed).
            a.set_danger(false);
            // A P-pause riding across the boundary (won-fade with the
            // options menu open) must not leave the output suspended
            // under the frontend — `paused` is cleared below and the
            // audio suspend state has to follow it.
            if self.paused {
                a.set_paused(false);
            }
        }
        // The options menu dies with its level (it would otherwise
        // reappear over the frontend with pause/audio desynced).
        self.menu = None;
        if let Some(r) = &mut self.renderer {
            r.clear_level();
        }
        // Per-level transients die with the session. The mini-menu is
        // one of them: it is an IN-LEVEL surface, and a stale panel
        // surviving into the frontend would offer to save a run that
        // is no longer loaded.
        self.paused = false;
        self.mini = None;
        self.quit_fade = None;
        self.won_handled = false;
        self.exit_confirm = false;
        self.subtitle = None;
        self.jar_markers = Vec::new();
        self.rival_tags_prev = Vec::new();
        self.rival_tags_cur = Vec::new();
        self.castle_pos = None;
        self.last_map_tick = None;
        self.accumulator = 0.0;
        self.toast_accumulator = 0.0;
        self.frontend_music();
    }

    /// Lazily materialize the frontend-owned level-UI assets when no
    /// session ever ran (campaign boot straight into P): built from
    /// the game's canonical variant bundle — the same source
    /// `load_level` uses.
    fn ensure_frontend_ui(&mut self) {
        if self.frontend_ui.is_some() || self.session.is_some() {
            return;
        }
        let variant = if self.is_mc2() {
            "mc2-day"
        } else if self
            .campaign
            .as_ref()
            .is_some_and(|c| c.id == campaign::CampaignId::Mc1Hw)
        {
            "mc1-arctic"
        } else {
            "mc1-temperate"
        };
        match Bundle::load(&get_baked_directory().join("assets").join(variant)) {
            Ok(bundle) => {
                self.frontend_ui = bundle.ui_sprites.as_ref().map(|(idx, px)| {
                    ui::UiAssets::build(
                        idx.clone(),
                        px,
                        &bundle.palette,
                        bundle.blend_lut.as_deref(),
                        !self.is_mc2(),
                        bundle.font.as_ref().map(|(i, p)| (i, p.as_slice())),
                        bundle.web_sprites.as_ref().map(|(i, p)| (i, p.as_slice())),
                    )
                });
            }
            Err(e) => eprintln!("note: frontend UI assets: {e}"),
        }
    }

    /// The window's inner size in physical pixels (UI coordinate
    /// space), with the default-viewport fallback.
    fn view_size(&self) -> (f32, f32) {
        self.window
            .as_ref()
            .map(|w| w.inner_size())
            .map(|s| (s.width as f32, s.height as f32))
            .unwrap_or((1280.0, 960.0))
    }

    /// Push the config's audio gains into the mixer (mute = gain 0).
    fn apply_volumes(&mut self) {
        if let Some(a) = &mut self.audio {
            a.set_volumes(
                if self.cfg.audio.sound {
                    self.cfg.audio.sfx_volume
                } else {
                    0.0
                },
                if self.cfg.audio.music {
                    self.cfg.audio.music_volume
                } else {
                    0.0
                },
            );
        }
    }

    /// Re-apply one Live option after its config value changed — THE
    /// single apply path shared by the runtime keys and the options
    /// menu (keyed by the registry's `cfg_path`). Options read live
    /// off `self.cfg` every frame/tick need no arm here.
    fn apply_option(&mut self, cfg_path: &str) {
        match cfg_path {
            "audio.sound" | "audio.sfx_volume" | "audio.music_volume" => self.apply_volumes(),
            "audio.music" => {
                if self.cfg.audio.music {
                    // Restart whichever mode's track applies: the
                    // session level's, or the frontend menu set.
                    let track = match &self.session {
                        Some(sess) => sess.level.music_track.clone(),
                        None => Some(self.frontend_track().to_string()),
                    };
                    if let (Some(a), Some(track)) = (&mut self.audio, track) {
                        let _ = a.play_music(&track, true);
                    }
                } else if let Some(a) = &mut self.audio {
                    a.stop_music();
                }
                self.apply_volumes();
            }
            "render.enhancement.smooth_shading" => {
                if let Some(r) = &mut self.renderer {
                    r.set_smooth_shading(self.cfg.render.enhancement.smooth_shading);
                }
            }
            "render.enhancement.map_marker_scale" => {
                if let Some(r) = &mut self.renderer {
                    r.set_marker_scale(self.cfg.render.enhancement.map_marker_scale);
                    // The dot bake-vs-lift decision lives in the map
                    // recompose, which is tick-throttled and the menu
                    // opens paused — run it explicitly so the slider
                    // shows live.
                    if let Some(sess) = self.session.as_deref() {
                        let overlay = map_overlay(&sess.level, &self.cfg);
                        r.update_map(&sess.level.view, &overlay);
                    }
                }
            }
            "render.enhancement.map_extent_fog" => {
                if let Some(r) = &mut self.renderer {
                    r.set_extent_fog(self.cfg.render.enhancement.map_extent_fog);
                }
            }
            "render.preference.fog_distance" => {
                if let Some(r) = &mut self.renderer {
                    r.set_fog_distance(self.cfg.render.preference.fog_distance as f32);
                }
            }
            "render.preference.sky" => {
                if let Some(r) = &mut self.renderer {
                    if self.cfg.render.preference.sky {
                        if let Some(sess) = self.session.as_deref()
                            && let Some(bitmap) = &sess.level.sky
                        {
                            r.load_sky(bitmap, &sess.level.palette_rgba);
                        }
                    } else {
                        r.clear_sky();
                    }
                }
            }
            "render.preference.reflections" => {
                if let Some(r) = &mut self.renderer {
                    r.set_reflections(self.cfg.render.preference.reflections);
                }
            }
            "render.preference.vsync" => {
                if let Some(r) = &mut self.renderer {
                    r.set_vsync(self.cfg.render.preference.vsync);
                }
            }
            "render.preference.fullscreen" => {
                self.apply_fullscreen();
                self.reassert_pointer();
            }
            // The supersample factor is live; MSAA is baked into every
            // pipeline, so it only takes effect next launch. Say so
            // rather than leaving the player wondering why nothing
            // changed.
            "render.preference.anti_aliasing" => {
                let aa = self.cfg.render.preference.anti_aliasing;
                if let Some(r) = &mut self.renderer {
                    r.set_render_scale(aa.render_scale());
                    if r.samples() != aa.samples() {
                        println!("anti-aliasing: MSAA applies on the next launch");
                    }
                }
            }
            "render.enhancement.hud_transparency" => {
                let transparent = self.hud_transparent();
                if let Some(r) = &mut self.renderer {
                    r.set_hud_transparent(transparent);
                }
            }
            "render.debug.health_bars" => {
                // Off clears immediately; on, bars appear with the next
                // entity sync (every tick while creatures move).
                if !self.cfg.render.debug.health_bars
                    && let Some(r) = &mut self.renderer
                {
                    r.set_health_bars(Vec::new());
                }
            }
            // The map texture recompose is tick-throttled and the menu
            // opens paused — recompose explicitly so the toggle shows.
            "render.debug.map_trigger_areas"
            | "render.enhancement.map_owned_buildings"
            | "render.enhancement.map_marker_icons"
            | "render.enhancement.expose_jar_spells" => {
                if let Some(sess) = self.session.as_deref_mut() {
                    // The dot set itself consumes the options, and no
                    // entity sync runs while paused — re-derive it
                    // before the recompose. The icon-swap also moves
                    // families between the DOT and STAMP layers, so
                    // the stamp set re-derives with it, and a family
                    // first sighted NOW (toggled on mid-level) lazily
                    // captures its miniature here too.
                    let mut icons_grew = false;
                    if let Some(w) = &sess.sim.world {
                        let poses = w.live_poses();
                        if self.cfg.render.enhancement.map_marker_icons {
                            icons_grew = capture_marker_icons(&mut sess.level, &poses);
                        }
                        let swapped = dot_swap_set(&sess.level, &self.cfg);
                        sess.level.map_dots = entities::map_dots_from_poses(
                            sess.level.game,
                            &poses,
                            &sess.level.palette_rgba,
                            self.cfg.render.enhancement.map_owned_buildings,
                            sess.level.mc2_env,
                            sess.sim.tick as u32,
                            &swapped,
                        );
                        sess.level.map_stamps = entities::map_stamps_from_poses(
                            sess.level.game,
                            &poses,
                            &sess.level.map_icons,
                            w.beyond_sight(),
                            self.cfg.render.enhancement.expose_jar_spells,
                            self.cfg.render.enhancement.map_marker_icons,
                        );
                        sess.level.map_stamps.extend(entities::exit_marker_stamps(
                            &w.advertised_marker_poses(),
                            &sess.level.map_icons,
                        ));
                    }
                    let overlay = map_overlay(&sess.level, &self.cfg);
                    if let Some(r) = &mut self.renderer {
                        if icons_grew && let Some(assets) = &sess.level.ui {
                            r.load_ui_atlas(assets.atlas_w, assets.atlas_h, &assets.atlas_rgba);
                        }
                        r.set_map_stamps(sess.level.map_stamps.clone());
                        r.update_map(&sess.level.view, &overlay);
                    }
                }
            }
            "controls.models.thrust" => {
                if let Some(sess) = self.session.as_deref_mut() {
                    // The hand-off setter, not a bare assign — the
                    // inactive mover's state is stale and reads back
                    // as a phantom warp/velocity.
                    sess.sim
                        .set_thrust_model(sim_thrust(self.cfg.controls.models.thrust));
                }
            }
            "controls.models.altitude" => {
                if let Some(sess) = self.session.as_deref_mut() {
                    // The setter re-seeds the desired altitude at the
                    // live pose when entering the enhanced tier.
                    sess.sim
                        .set_altitude_model(sim_altitude(self.cfg.controls.models.altitude));
                }
            }
            "dev.lift_unclamped" => {
                if let Some(sess) = self.session.as_deref_mut() {
                    sess.sim.lift_unclamped = self.cfg.dev.lift_unclamped;
                }
            }
            "gameplay.cheat.dev_spells" => {
                if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_dev_spells(self.cfg.gameplay.cheat.dev_spells);
                }
            }
            "gameplay.cheat.invincible" => {
                if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_invincible(self.cfg.gameplay.cheat.invincible);
                }
            }
            "gameplay.enhancement.prune_owned_jars" => {
                if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_prune_owned_jars(self.cfg.gameplay.enhancement.prune_owned_jars);
                }
            }
            // Live selector-surface switch: the pane/book resolve is
            // cheap to redo mid-run. Quickselect
            // digit binds survive a round trip — and enabling the map
            // book mid-level replays the retail level-init pre-seed
            // (the acquisition diff sees every owned spell at once).
            "gameplay.enhancement.spell_selector" => {
                let is_mc2 = self.is_mc2();
                let choice = self.cfg.gameplay.enhancement.spell_selector;
                self.selector = choice.resolve(is_mc2);
                if is_mc2
                    && matches!(
                        choice,
                        config::SpellSelector::Mc1 | config::SpellSelector::Mc1Mc2
                    )
                {
                    println!(
                        "spell-selector: MC2 has no in-map spellbook — using the faithful CTRL pane"
                    );
                }
                self.pane = self.selector.ctrl_pane.then(|| {
                    if is_mc2 {
                        ui::SelectorPane::mc2()
                    } else {
                        ui::SelectorPane::mc1()
                    }
                });
                self.selector_drag = None;
                self.selector_hover = ui::SelectorHover::default();
                if let Some(r) = &mut self.renderer {
                    r.set_map_layout(if self.selector.map_book {
                        mgc_render::MapScreenLayout::Mc1Book
                    } else {
                        mgc_render::MapScreenLayout::Mc2Split
                    });
                }
            }
            // The retail-bug patches: one live re-apply covers all of
            // them — the sim consumes the whole set per tick/event,
            // and the movie-score patch is read at play time. While a
            // recording or replay is live the arms stay RETAIL — the
            // take's determinism depends on it (the boot pin set the
            // cfg; a menu flip mid-take must not undo it).
            p if p.starts_with("gameplay.patches.") => {
                if self.recorder.is_some() || self.replay.is_some() {
                    println!("patches: pinned to retail arms while a recording/replay is live");
                } else if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_patches(world_patches(&self.cfg.gameplay.patches));
                }
            }
            // Everything else is read live off self.cfg (game_speed,
            // crosshair, grace_meter, expose_jar_spells, sensitivity,
            // invert_y, fly_assistant, bindings, speech, subtitles,
            // dev_spells' pane view) or is Startup-mutability (greyed
            // in the menu: arrangement, pool, awake, plausible book).
            _ => {}
        }
    }

    /// Persist one option's CURRENT value into the sparse overlay
    /// config (`mgcarpet.json`): only that dotted path is touched, so
    /// hand-written overrides and `gamedata` survive. Menu changes
    /// persist; runtime-key toggles stay session-only, as does the
    /// occasional spec that opts out wholesale (`Spec::persists`).
    fn persist_option(&self, spec: &settings::Spec) {
        if !spec.persists() {
            return;
        }
        let full = match serde_json::to_value(&self.cfg) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("note: config save: {e}");
                return;
            }
        };
        let mut leaf = &full;
        for seg in spec.cfg_path.split('.') {
            leaf = &leaf[seg];
        }
        let mut root = std::fs::read_to_string(&self.cfg_file)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let segs: Vec<&str> = spec.cfg_path.split('.').collect();
        let mut slot = &mut root;
        for seg in &segs[..segs.len() - 1] {
            if !slot.get(*seg).is_some_and(|v| v.is_object()) {
                slot[*seg] = serde_json::json!({});
            }
            slot = &mut slot[*seg];
        }
        slot[*segs.last().unwrap()] = leaf.clone();
        let text = serde_json::to_string_pretty(&root).expect("json serializes") + "\n";
        if let Err(e) = std::fs::write(&self.cfg_file, text) {
            eprintln!("note: config save {}: {e}", self.cfg_file.display());
        }
    }

    /// Open/close the pause menu: pause rides along (the sim clock
    /// freezes, all sound suspends — the retail pause law), the cursor
    /// is freed and restored on close. Without UI assets (no baked
    /// font) this is a plain pause.
    ///
    /// Which menu depends on the screen. IN A LEVEL it is the small
    /// `minimenu` panel, which leaves the game's own input live
    /// underneath — restoring retail MC1's pause-to-rearrange-spells
    /// behaviour that a full-screen menu regressed. On the frontend
    /// screens there is no sim to pause and no HUD to preserve, so P
    /// opens the options menu directly, as before.
    fn toggle_menu(&mut self) {
        if !self.paused {
            self.paused = true;
            // Frontend screens need the frontend-owned UI bank for
            // the menu's fonts/panels (no session to borrow from).
            self.ensure_frontend_ui();
            // Without a baked font there is no menu — plain pause.
            if ui_assets!(self).is_some() {
                if self.screen == Screen::Level {
                    self.mini = Some(minimenu::MiniMenu::new());
                } else {
                    self.menu = Some(menu::MenuState::new(&self.specs));
                }
                self.menu_grab_restore = self.grabbed;
                self.set_grab(false);
                self.fire_held = false;
                self.fire_right_held = false;
            }
        } else {
            self.paused = false;
            // Unpause is the ONLY way out of the mini-menu (the
            // standing Esc law), and it closes the options layer with
            // it if one is stacked on top.
            let closed = self.menu.take().is_some() | self.mini.take().is_some();
            match self.screen {
                // In-level: unpause re-locks the pointer if flight
                // held it when the menu opened.
                Screen::Level => {
                    // Re-lock unless the BOOK map owns the cursor
                    // (the bookless map flies grabbed).
                    if closed
                        && self.menu_grab_restore
                        && (!self.book_open() || !self.selector.map_book)
                    {
                        self.set_grab(true);
                    }
                }
                // Frontend screens re-assert their own pointer state
                // (the menu freed + showed the OS cursor).
                Screen::Map => self.confine_map_pointer(),
                Screen::Menu => {
                    if self.is_mc2() {
                        self.free_menu_pointer();
                    }
                }
                // A movie owns the whole screen and takes no pointer.
                Screen::Movie => {}
            }
        }
        // Retail pause suspends ALL sound; resumed sounds pick up
        // where they froze.
        if let Some(a) = &mut self.audio {
            a.set_paused(self.paused);
        }
        println!("{}", if self.paused { "paused" } else { "unpaused" });
    }

    /// The runtime option keys (session-only — never persisted; the
    /// menu is the persisting path). Returns true when the key was
    /// one of them. Live both in flight and inside the menu.
    fn option_key(&mut self, code: KeyCode) -> bool {
        // (cfg_path, mutate) pairs keep the apply path shared with the
        // menu via apply_option; the second element is the concise
        // in-game toast ("<Name> on/off" / "Game speed <label>" —
        // retail echoes live toggles on screen).
        let onoff = |v: bool| if v { "on" } else { "off" };
        let (path, toast) = match code {
            KeyCode::F1 => {
                self.cfg.audio.sound = !self.cfg.audio.sound;
                let v = self.cfg.audio.sound;
                println!(
                    "sound: {}{}",
                    onoff(v),
                    if self.audio.is_none() {
                        " (no audio device)"
                    } else {
                        ""
                    }
                );
                ("audio.sound", format!("Sound {}", onoff(v)))
            }
            KeyCode::F2 => {
                self.cfg.audio.music = !self.cfg.audio.music;
                let v = self.cfg.audio.music;
                println!(
                    "music: {}{}",
                    onoff(v),
                    if self.audio.is_none() {
                        " (no audio device)"
                    } else {
                        ""
                    }
                );
                ("audio.music", format!("Music {}", onoff(v)))
            }
            // Game speed (retail F3; MC1 cycles its three levels —
            // ours adds slow).
            KeyCode::F3 => {
                self.cfg.sim.options.game_speed = self.cfg.sim.options.game_speed.cycle();
                let speed = self.cfg.sim.options.game_speed;
                println!("game speed: {}", speed.label());
                (
                    "sim.options.game_speed",
                    format!("Game speed {}", speed.toast_label()),
                )
            }
            // F4 = retail soften (a screen-space smoothing filter) —
            // not implemented; reserved.
            KeyCode::F5 => {
                self.cfg.render.preference.reflections = !self.cfg.render.preference.reflections;
                let v = self.cfg.render.preference.reflections;
                println!("reflections: {}", onoff(v));
                (
                    "render.preference.reflections",
                    format!("Reflections {}", onoff(v)),
                )
            }
            KeyCode::F6 => {
                self.cfg.render.preference.sky = !self.cfg.render.preference.sky;
                let v = self.cfg.render.preference.sky;
                println!(
                    "sky: {}{}",
                    onoff(v),
                    if self
                        .session
                        .as_deref()
                        .is_none_or(|s| s.level.sky.is_none())
                    {
                        " (this level has no sky bitmap)"
                    } else {
                        ""
                    }
                );
                ("render.preference.sky", format!("Sky {}", onoff(v)))
            }
            // F7 = retail shadows — no shadows option yet.
            KeyCode::KeyT => {
                self.cfg.render.enhancement.smooth_shading =
                    !self.cfg.render.enhancement.smooth_shading;
                let v = self.cfg.render.enhancement.smooth_shading;
                println!(
                    "shading: {}",
                    if v {
                        "smooth (enhanced)"
                    } else {
                        "per-tile (original)"
                    }
                );
                (
                    "render.enhancement.smooth_shading",
                    format!("Shading {}", if v { "smooth" } else { "per-tile" }),
                )
            }
            KeyCode::KeyV => {
                self.cfg.render.debug.map_trigger_areas = !self.cfg.render.debug.map_trigger_areas;
                let v = self.cfg.render.debug.map_trigger_areas;
                println!(
                    "map trigger overlay: {}",
                    if v { "on (enhanced)" } else { "off (original)" }
                );
                (
                    "render.debug.map_trigger_areas",
                    format!("Trigger overlay {}", onoff(v)),
                )
            }
            KeyCode::KeyG => {
                self.cfg.gameplay.cheat.dev_spells = !self.cfg.gameplay.cheat.dev_spells;
                let v = self.cfg.gameplay.cheat.dev_spells;
                println!(
                    "dev spells: {}",
                    if v {
                        "on — all spells, infinite mana (playtest instrument)"
                    } else {
                        "off (authentic acquisition/mana)"
                    }
                );
                (
                    "gameplay.cheat.dev_spells",
                    format!("Dev spells {}", onoff(v)),
                )
            }
            // H = invincibility (a cheat, keyed next to G).
            KeyCode::KeyH => {
                self.cfg.gameplay.cheat.invincible = !self.cfg.gameplay.cheat.invincible;
                let v = self.cfg.gameplay.cheat.invincible;
                println!(
                    "invincible: {}",
                    if v {
                        "on (cheat — damage shown, never applied)"
                    } else {
                        "off (mortal)"
                    }
                );
                (
                    "gameplay.cheat.invincible",
                    format!("Invincibility {}", onoff(v)),
                )
            }
            KeyCode::KeyB => {
                self.cfg.render.debug.health_bars = !self.cfg.render.debug.health_bars;
                let v = self.cfg.render.debug.health_bars;
                println!(
                    "monster health bars: {}",
                    if v {
                        "on (debug enhancement)"
                    } else {
                        "off (original)"
                    }
                );
                (
                    "render.debug.health_bars",
                    format!("Health bars {}", onoff(v)),
                )
            }
            KeyCode::KeyC => {
                self.cfg.render.preference.crosshair = !self.cfg.render.preference.crosshair;
                let v = self.cfg.render.preference.crosshair;
                println!(
                    "aim crosshair: {}",
                    if v { "on" } else { "off (no aim cursor)" }
                );
                (
                    "render.preference.crosshair",
                    format!("Crosshair {}", onoff(v)),
                )
            }
            KeyCode::KeyK => {
                self.cfg.render.debug.coords = !self.cfg.render.debug.coords;
                let v = self.cfg.render.debug.coords;
                println!(
                    "coordinate overlay: {}",
                    if v { "on (engine units)" } else { "off" }
                );
                ("render.debug.coords", format!("Coordinates {}", onoff(v)))
            }
            _ => return false,
        };
        self.apply_option(path);
        // The in-game echo (the retail F3-style live feedback): ride
        // the sim's notification line when a world is up. Set while
        // paused it simply shows once the clock resumes.
        if let Some(w) = self
            .session
            .as_deref_mut()
            .and_then(|s| s.sim.world.as_mut())
        {
            w.notify_option(toast);
        }
        true
    }

    /// Per-sim-tick audio: drain the world's sound requests into the
    /// faithful mixer, feed the ambient rule, run the flush.
    fn audio_tick(&mut self) {
        let Some(audio) = &mut self.audio else { return };
        let Some(sess) = self.session.as_deref_mut() else {
            return;
        };
        let f = &sess.sim.flyer;
        let pose =
            mgc_sim::engine::world::PlayerPose::from_tiles(f.x, f.y, f.z, f.yaw, f.pitch, 0.0);
        let listener = mgc_audio::Listener {
            pos: (pose.x, pose.y, pose.z),
            yaw: pose.heading,
        };
        if let Some(w) = &mut sess.sim.world {
            let frame = w.take_audio(pose);
            if self.cfg.audio.sound {
                for e in frame.events {
                    let source = if e.player {
                        mgc_audio::Source::Player
                    } else {
                        // e.tag = the emitter's OWNER word (resolved
                        // by take_audio) — the channel-pair key (D2).
                        mgc_audio::Source::World {
                            pos: e.pos,
                            owner: e.tag,
                        }
                    };
                    audio.event(e.id, source, &listener);
                }
                audio
                    .mixer
                    .set_ambient(frame.over_water, frame.fire_near, frame.market_near);
            }
            audio.set_danger(frame.danger);
            // MC2 objective voiceover: the sim's trigger ramp hands
            // over the SEGMENT; the row is the level number. Special
            // levels 30-34 address row 0 (seg 4) / row 10 (seg 9) —
            // retail EF:41020-29, ported verbatim.
            if let Some(seg) = frame.speech {
                let lvl = sess.level.level_number;
                if self.cfg.audio.speech {
                    let (row, cseg) = if (30..=34).contains(&lvl) {
                        if seg == 9 { (10, 9) } else { (0, 4) }
                    } else {
                        (lvl, u32::from(seg))
                    };
                    if let Err(e) = audio.play_speech(row, cseg) {
                        eprintln!("note: speech: {e}");
                    }
                }
                // The narration subtitle: the same cue's ETEXT
                // sentence (retail's objective textbox shows this
                // text as its speech-off fallback; our `on` overtitles
                // the voiceover too).
                if self.cfg.audio.subtitles.on()
                    && let Some(idx) = mc2_narration_etext(lvl, seg)
                    && let Some(text) = sess.level.etext.get(idx).filter(|s| !s.is_empty())
                {
                    self.subtitle = Some((text.clone(), SUBTITLE_TICKS));
                }
            }
            // The dwell countdown lives with the toast decay in the
            // frame loop (wall clock), not here per sim tick.
        }
        audio.tick();
    }

    /// The screen-mode chime (sub_3DC90 :49072, sound 14 at the
    /// local wizard): level start, map/book enter + exit, respawn.
    /// The sim-side switches emit it through the event stream; this
    /// is the app-side path for view toggles the sim never sees.
    fn ui_ding(&mut self) {
        let Some(audio) = &mut self.audio else { return };
        if !self.cfg.audio.sound {
            return;
        }
        let listener = match self.session.as_deref() {
            Some(sess) => {
                let f = &sess.sim.flyer;
                let pose = mgc_sim::engine::world::PlayerPose::from_tiles(
                    f.x, f.y, f.z, f.yaw, f.pitch, 0.0,
                );
                mgc_audio::Listener {
                    pos: (pose.x, pose.y, pose.z),
                    yaw: pose.heading,
                }
            }
            // Frontend screens: player-sourced samples ignore the
            // position — a centered listener serves.
            None => mgc_audio::Listener {
                pos: (0, 0, 0),
                yaw: 0,
            },
        };
        audio.event(14, mgc_audio::Source::Player, &listener);
    }

    /// Castle-less death: rebuild the pristine world (the original
    /// restarts the level) and reset the flyer to the level start.
    fn restart_level(&mut self) {
        {
            let Some(sess) = self.session.as_deref_mut() else {
                return;
            };
            let Some(init) = &sess.level.world_init else {
                return;
            };
            let mut w = init.build();
            self.pool_dropped_total = 0;
            self.misfits_reported = 0;
            // Retail wipes + reseeds the quick keys at level init
            // (:49216-59) — the acquisition diff below re-seeds the
            // starting spells in canonical order on the first tick.
            self.quick_binds = [None; 10];
            self.prev_owned = [false; 24];
            // A restart is a level entry — the situational sim speed
            // resets with the rest (mirrors `install_level`).
            self.cfg.sim.options.game_speed = config::GameSpeed::Normal;
            apply_instruments(
                &mut w,
                self.cfg.gameplay.cheat.dev_spells,
                &sess.level.plausible_spells,
                &sess.level.plausible_book_mc2,
                self.cfg.gameplay.cheat.invincible,
                world_patches(&self.cfg.gameplay.patches),
            );
            if let Some(run) = &self.campaign {
                // The restart is a fresh world — the campaign carry
                // re-grants like any level entry.
                apply_campaign_book(&mut w, run, &sess.level);
            }
            w.terrain_dirty = true;
            w.entities_dirty = true;
            let (thrust, altitude) = (sess.sim.thrust_model, sess.sim.altitude_model);
            sess.sim = Simulation::with_world(w);
            sess.sim.thrust_model = thrust;
            sess.sim.altitude_model = altitude;
            sess.sim.lift_unclamped = self.cfg.dev.lift_unclamped;
            if let Some(start) = sess.level.start {
                sess.sim.flyer = start;
                sess.sim.sync_carpet_from_flyer();
            }
            sess.prev_flyer = sess.sim.flyer;
            // Fresh world, fresh generations — a stale snapshot could
            // coincidentally pair (slot, generation) across the restart.
            sess.pose_prev = Vec::new();
            sess.pose_cur = Vec::new();
            sess.fire_blasts.clear();
            sess.bolts.clear();
            self.castle_pos = None;
            self.won_handled = false;
        }
        self.sync_world();
        println!("level restarted (died without a castle)");
    }

    /// Handle a left-click that landed on the pause mini-menu.
    ///
    /// Returns true when the click was consumed. A click ANYWHERE
    /// else while paused falls through to the game's own handlers —
    /// that is the whole point of the small panel.
    fn mini_click(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(mini) = &self.mini else { return false };
        let size = self.view_size();
        let Some(assets) = ui_assets!(self) else {
            return false;
        };
        if !minimenu::covers(assets, mini, size.0, size.1, self.cursor) {
            return false;
        }
        let hit = minimenu::hit_test(assets, mini, size.0, size.1, self.cursor);
        let tag = self.campaign.as_ref().map(|r| r.id.tag());
        let mini = self.mini.as_mut().expect("checked above");
        match hit {
            minimenu::Hit::None => {}
            minimenu::Hit::Save | minimenu::Hit::Load => {
                let saving = hit == minimenu::Hit::Save;
                match tag {
                    Some(tag) => {
                        mini.slots = saves::scan_slots(tag);
                        mini.mode = minimenu::Mode::Slots { saving };
                    }
                    // Single-level mode has no slots to write into.
                    None => self.mini_toast("No campaign running"),
                }
            }
            minimenu::Hit::Options => {
                self.menu = Some(menu::MenuState::new(&self.specs));
            }
            minimenu::Hit::Back => mini.reset_to_root(),
            minimenu::Hit::Slot(i) => {
                let saving = matches!(mini.mode, minimenu::Mode::Slots { saving: true });
                let occupied = mini.slots.get(i).is_some_and(|s| s.occupied);
                let unreadable = mini.slots.get(i).is_some_and(|s| s.incompatible);
                if saving {
                    match self.save_slot(i) {
                        Ok(in_level) => self.mini_toast(format!(
                            "Saved to slot {}{}",
                            i + 1,
                            if in_level { " (resume)" } else { "" }
                        )),
                        Err(e) => self.mini_toast(e),
                    }
                    // Re-scan so the row reflects what was just
                    // written (label, and the mid-level marker).
                    if let (Some(tag), Some(mini)) = (tag, self.mini.as_mut()) {
                        mini.slots = saves::scan_slots(tag);
                    }
                } else if !occupied {
                    self.mini_toast("That slot is empty");
                } else if unreadable {
                    self.mini_toast("That slot cannot be read");
                } else {
                    match self.load_slot(i, event_loop) {
                        // A successful load replaced the session
                        // wholesale — drop the menu and resume.
                        Ok(()) => {
                            self.mini = None;
                            self.menu = None;
                            self.paused = false;
                            if let Some(a) = &mut self.audio {
                                a.set_paused(false);
                            }
                        }
                        Err(e) => self.mini_toast(e),
                    }
                }
            }
        }
        true
    }

    /// Report a mini-menu result on the in-game toast line rather than
    /// inside the panel.
    ///
    /// The panel is narrow by design, so a message long enough to be
    /// useful ("slot 3 was saved with the mc2-night bundle, this level
    /// resolves to …") ran off the right edge of the screen. The toast
    /// is the surface built for this, and it is the same one option
    /// changes already use.
    ///
    /// The notification is hash-excluded, so writing it from the app
    /// cannot perturb the sim. Its timer runs on the SIM clock, which
    /// is stopped while paused — so the message simply stays up for as
    /// long as the player is in the menu, then decays once play
    /// resumes. That is the behaviour we want here.
    fn mini_toast(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        println!("{msg}");
        if let Some(w) = self
            .session
            .as_deref_mut()
            .and_then(|s| s.sim.world.as_mut())
        {
            w.notify_option(msg);
        }
    }

    /// Write the current run into a save slot, including the live
    /// world when a level is running (docs/archive/DESIGN-SAVES.md).
    ///
    /// Saving is offered only from the paused mini-menu, and pause is
    /// an inter-tick boundary by construction — which is exactly
    /// where both retail engines snapshot — so no extra quiescing is
    /// needed here.
    ///
    /// Returns whether a WORLD payload went with it, so the caller can
    /// tell the player — "did my resume get written" is the one thing
    /// worth reporting about a save.
    fn save_slot(&mut self, slot: usize) -> Result<bool, String> {
        let Some(run) = &self.campaign else {
            return Err("no campaign is running (launch with --campaign)".into());
        };
        if slot >= saves::slot_count(run.id.tag()) {
            return Err(format!("slot {} is out of range", slot + 1));
        }
        let mut package = run.hub_package();

        // The world half, when a level is up. A save taken at the hub
        // is campaign-only and resumes at the campaign screen.
        if let Some(sess) = self.session.as_deref() {
            let level = &sess.level;
            // What the player has taken POSSESSION of, world-relative.
            // Not `banked` — that is the castle panel's numerator
            // (houses + castle stored) and reads 0 under MC2 until a
            // castle stands, however much has actually been collected.
            let mana_pct = sess
                .sim
                .world
                .as_ref()
                .map(|w| w.player_mana_share_pct())
                .unwrap_or(0);
            // Mid-level, the WORLD's cycle ring is the live one
            // (run.mc1_ring only refreshes at the won edge) — keep
            // the header sidecar in step with the snapshot.
            if run.id != campaign::CampaignId::Mc2
                && let Some(w) = sess.sim.world.as_ref()
            {
                let ring = w.loadout().ring;
                package.header.mc1_spell_ring = (ring != [0; 24]).then_some(ring);
            }
            package.snapshot = Some(sess.sim.snapshot());
            package.header.resume = Some(mgc_formats::mgcs::InLevel {
                bundle: level.bundle_variant.clone(),
                entry_sha256: level.entry_sha256.clone().unwrap_or_default(),
                snapshot_version: mgc_sim::snapshot::SNAPSHOT_VERSION,
                tick: sess.sim.tick,
                mana_pct,
                thrust_model: Some(format!("{:?}", sess.sim.thrust_model)),
                altitude_model: Some(format!("{:?}", sess.sim.altitude_model)),
            });
        }

        let in_level = package.is_in_level();
        saves::write_slot(run.id.tag(), slot, &package)?;
        Ok(in_level)
    }

    /// Resume into a slot's saved level: resolve the level it was
    /// taken in, build it as a fresh start would, and apply the
    /// payload over it.
    ///
    /// `self.campaign` must ALREADY be the run for this slot — the
    /// frontend sets it up (with its own per-game dressing) before
    /// calling, and the in-level path adopts it first. Returns false
    /// when the slot is campaign-only, having changed nothing; the
    /// caller decides where a hub save should land.
    ///
    /// The rebuild-then-apply shape is not incidental. The snapshot
    /// deliberately omits the level package's own data (`Gen::assets`
    /// and friends), so there is nothing to construct a world FROM;
    /// the level supplies the immutable half and the payload supplies
    /// the mutable half.
    fn resume_slot(&mut self, slot: usize) -> Result<bool, String> {
        let Some(run) = &self.campaign else {
            return Err("no campaign is running (launch with --campaign)".into());
        };
        let tag = run.id.tag();
        if slot >= saves::slot_count(tag) {
            return Err(format!("slot {} is out of range", slot + 1));
        }
        let path = saves::native_path(tag, slot);
        if !path.exists() {
            // An imported retail `.gam` with no native file: campaign
            // only, by construction.
            return Ok(false);
        }
        let file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let package =
            mgc_formats::mgcs::read(file).map_err(|e| format!("{}: {e}", path.display()))?;
        let Some(in_level) = package.header.resume.as_ref() else {
            return Ok(false);
        };
        // The level lives on the HEADER, not inside `resume` — every
        // save carries one, and a single copy cannot disagree with
        // itself.
        let index = package.header.level;

        // Rebuild the level the save was taken in, not the one the
        // campaign order would pick — a mid-level save is pinned to
        // its own level.
        let mut fresh = CampaignRun::start(run.id, Some(slot), false)?;
        fresh.current = index;
        let level_path = fresh.level_path(index);
        let level = load_level(
            &level_path,
            self.launch.tileset,
            self.launch.terrain_features,
            false,
            self.cfg.gameplay.enhancement.prune_owned_jars,
            self.launch.pool_slots,
            self.launch.awake_range,
        )?;

        // The rejection keys, checked BEFORE the level is installed so
        // a refusal leaves the running session alone. `bake_epoch` is
        // deliberately not among them (design "Rejection policy").
        if let Some(have) = level.entry_sha256.as_deref() {
            if !in_level.entry_sha256.is_empty() && have != in_level.entry_sha256 {
                return Err(format!(
                    "slot {} was saved in a different build of level {index} — \
                     the level package has been rebaked since",
                    slot + 1
                ));
            }
        }
        if level.bundle_variant != in_level.bundle {
            return Err(format!(
                "slot {} was saved with the {} bundle, this level resolves to {}",
                slot + 1,
                in_level.bundle,
                level.bundle_variant
            ));
        }
        let Some(snapshot) = package.snapshot.as_deref() else {
            return Err(format!("slot {} has no world payload", slot + 1));
        };

        self.campaign = Some(fresh);
        self.install_level(level);

        // Apply onto the freshly installed world. A failure here is
        // NOT recoverable in place: `restore` may have written part of
        // the world before erroring, so the level is rebuilt from
        // scratch rather than left half-applied.
        let applied = self
            .session
            .as_deref_mut()
            .ok_or_else(|| "level did not install".to_string())
            .and_then(|s| s.sim.restore(snapshot).map_err(|e| e.to_string()));
        if let Err(e) = applied {
            eprintln!("error: slot {}: {e} — restarting the level", slot + 1);
            self.restart_level();
            return Err(format!("slot {}: {e}", slot + 1));
        }

        // The restore replaced the world wholesale, so the renderer's
        // terrain and entity mirrors and the interpolation history all
        // have to be re-seeded — the same set the castle-less-death
        // restart re-seeds, for the same reason. `pose_prev`/`pose_cur`
        // in particular MUST be cleared: a stale snapshot could
        // coincidentally pair a (slot, generation) that now means a
        // different entity.
        //
        // The flyer needs no re-derivation: it and the carpet are both
        // in the payload, so they come back mutually consistent.
        if let Some(sess) = self.session.as_deref_mut() {
            sess.prev_flyer = sess.sim.flyer;
            sess.pose_prev = Vec::new();
            sess.pose_cur = Vec::new();
            sess.fire_blasts.clear();
            sess.bolts.clear();
            if let Some(w) = sess.sim.world.as_mut() {
                w.terrain_dirty = true;
                w.entities_dirty = true;
            }
        }
        self.sync_world();
        println!(
            "resumed slot {} — level {index} at {}% mana, tick {}",
            slot + 1,
            in_level.mana_pct,
            in_level.tick
        );
        Ok(true)
    }

    /// Load a slot from INSIDE a level (the mini-menu's Load).
    ///
    /// Adopts the slot's campaign record, then either resumes into its
    /// saved level or — for a campaign-only slot — leaves the level
    /// for the hub, because that is where such a save was taken and
    /// where it means something. Dropping the player into a fresh run
    /// of some level instead would be a restart wearing a load's
    /// clothes.
    fn load_slot(&mut self, slot: usize, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let Some(run) = &self.campaign else {
            return Err("no campaign is running (launch with --campaign)".into());
        };
        let fresh = CampaignRun::start(run.id, Some(slot), false)?;
        self.campaign = Some(fresh);
        if self.resume_slot(slot)? {
            return Ok(());
        }
        println!("loaded slot {} — returning to the hub", slot + 1);
        self.confirm_exit(event_loop);
        Ok(())
    }

    /// Load and install the campaign's next level in-place — the
    /// mid-run counterpart of `App::new` + `resumed`'s upload block.
    /// A load failure is fatal (a campaign with a hole is not
    /// continuable): report and exit.
    fn campaign_switch(&mut self, n: u32, event_loop: &ActiveEventLoop) {
        let Some(run) = &mut self.campaign else {
            return;
        };
        run.current = n;
        let path = run.level_path(n);
        println!("campaign: launching level {n}");
        let level = match load_level(
            &path,
            self.launch.tileset,
            self.launch.terrain_features,
            false, // the plausible instrument is off in campaign mode
            self.cfg.gameplay.enhancement.prune_owned_jars,
            self.launch.pool_slots,
            self.launch.awake_range,
        ) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("error: campaign level {n}: {e}");
                event_loop.exit();
                return;
            }
        };
        self.install_level(level);
    }

    /// Construct a fresh gameplay session from a loaded level — THE
    /// loader seam (boot, the frontend's launches, mid-chain level
    /// switches): build the sim, apply instruments + campaign carry,
    /// upload the renderer's level assets, switch the music, reset
    /// the per-level transients. Any previous session's remains
    /// (sounds included) are cut first.
    fn install_level(&mut self, mut level: LoadedLevel) {
        // A new session ends the replay/record instruments — both are
        // single-take, single-level by design (the boot install runs
        // before either attaches, so this is a no-op there).
        if self.replay.take().is_some() {
            println!("replay: ended by level switch");
        }
        self.finish_recorder();
        // Retail stops sfx + speech before EVERY launch (remc1
        // :59992-94), so this is unconditional rather than gated on an
        // outgoing session.
        //
        // It used to be gated, which covered the direct level→level
        // switch (the demon-mouth chain, which never passes through a
        // frontend teardown) but not a launch FROM a frontend, where
        // there is no session to detect. The world map plays the
        // upcoming level's narration while you stand on it, so entering
        // a level — by portal or by loading a mid-level save — carried
        // that narration into play over the top of the level's own
        // audio.
        //
        // Cutting it is the RIGHT behaviour, not merely the tidy one:
        // entering early is supposed to cut the map's line, and the
        // level then plays its own, DIFFERENT narration (player ground
        // truth). That in-level line is raised by `audio_tick` from the
        // sim's trigger ramp, well after this point, so it is never the
        // thing being stopped here.
        if let Some(a) = &mut self.audio {
            a.stop_sounds();
            a.stop_speech();
            // A P-pause held across a direct level→level chain (the
            // demon-mouth dive fade) must not boot the next level
            // suspended/pre-paused.
            if self.paused {
                a.set_paused(false);
            }
        }
        self.paused = false;
        self.menu = None;
        self.mini = None;
        let mut sim = match level.world.take() {
            Some(mut w) => {
                w.terrain_dirty = true;
                w.entities_dirty = true;
                apply_instruments(
                    &mut w,
                    self.cfg.gameplay.cheat.dev_spells,
                    &level.plausible_spells,
                    &level.plausible_book_mc2,
                    self.cfg.gameplay.cheat.invincible,
                    world_patches(&self.cfg.gameplay.patches),
                );
                if let Some(run) = &self.campaign {
                    apply_campaign_book(&mut w, run, &level);
                }
                Simulation::with_world(w)
            }
            None => Simulation::with_terrain(level.height.clone()),
        };
        sim.thrust_model = sim_thrust(self.cfg.controls.models.thrust);
        sim.altitude_model = sim_altitude(self.cfg.controls.models.altitude);
        // Dev instrument: unclamp the enhanced-altitude band to the
        // global lift ceiling (highest terrain + the 4-tile margin).
        sim.lift_unclamped = self.cfg.dev.lift_unclamped;
        if let Some(start) = level.start {
            sim.flyer = start;
            sim.sync_carpet_from_flyer();
        }
        let prev_flyer = sim.flyer;
        self.session = Some(Box::new(Session {
            level,
            sim,
            prev_flyer,
            pose_prev: Vec::new(),
            pose_cur: Vec::new(),
            fire_blasts: entities::BlastLedger::default(),
            bolts: entities::BoltLedger::default(),
        }));
        self.screen = Screen::Level;
        // The selection surfaces follow the running game.
        let is_mc2 = self.is_mc2();
        self.selector = self.cfg.gameplay.enhancement.spell_selector.resolve(is_mc2);
        self.pane = self.selector.ctrl_pane.then(|| {
            if is_mc2 {
                ui::SelectorPane::mc2()
            } else {
                ui::SelectorPane::mc1()
            }
        });
        // The restart_level transient-reset list, plus the fade and
        // the per-level UI state.
        self.pool_dropped_total = 0;
        self.misfits_reported = 0;
        self.quick_binds = [None; 10];
        self.prev_owned = [false; 24];
        self.castle_pos = None;
        self.subtitle = None;
        self.jar_markers = Vec::new();
        self.rival_tags_prev = Vec::new();
        self.rival_tags_cur = Vec::new();
        self.last_map_tick = None;
        self.quit_fade = None;
        self.pane_bound = [None; 2];
        self.spell_levels = [0; 26];
        self.won_handled = false;
        self.exit_confirm = false;
        self.accumulator = 0.0;
        self.toast_accumulator = 0.0;
        // Sim speed is a situational control (waiting out balloon
        // runs at 4x), not a standing preference — every level opens
        // at the authentic pace. Session-only by the same token: the
        // menu never persists it (`Spec::persists`).
        self.cfg.sim.options.game_speed = config::GameSpeed::Normal;
        // Renderer: the same upload block `resumed` runs at startup.
        let sess = sess_ref!(self);
        let overlay = map_overlay(&sess.level, &self.cfg);
        if let Some(r) = &mut self.renderer {
            r.load_level(&sess.level.view, &overlay);
            if let Some((index, atlas)) = &sess.level.sprites {
                r.load_sprites(index.clone(), atlas);
            }
            if let Some(assets) = &sess.level.ui {
                r.load_ui_atlas(assets.atlas_w, assets.atlas_h, &assets.atlas_rgba);
                self.ui_atlas = UiAtlas::Level;
            }
            r.set_billboards(sess.level.billboards.clone());
            if let Some(sky) = mc2_sky_srgb(&sess.level) {
                r.set_sky_color(sky);
            }
            r.clear_sky();
            if self.cfg.render.preference.sky
                && let Some(bitmap) = &sess.level.sky
            {
                r.load_sky(bitmap, &sess.level.palette_rgba);
            }
        }
        let label = sess_ref!(self).level.label.clone();
        if let Some(win) = &self.window {
            win.set_title(&format!("Magic Carpet — {label}"));
        }
        let track = sess_ref!(self).level.music_track.clone();
        if let Some(a) = &mut self.audio
            && self.cfg.audio.music
        {
            match &track {
                Some(track) => {
                    if let Err(e) = a.play_music(track, true) {
                        eprintln!("note: music: {e}");
                    }
                }
                None => a.stop_music(),
            }
        }
        // A level opens IN FLIGHT: fullscreen map closed, pointer
        // locked for mouse-look. Without this, entry from the MC2
        // world map inherited the map screen's pointer state
        // (confined, ungrabbed) and the level started uncaptured
        // until the in-game map toggle or a click re-grabbed; a
        // map_view left ON would likewise leak across the load.
        if let Some(r) = &mut self.renderer {
            r.set_map_view(false);
        }
        self.set_grab(true);
        self.sync_world();
    }

    fn book_open(&self) -> bool {
        self.renderer.as_ref().is_some_and(|r| r.map_view())
    }

    fn tick_input(&mut self) -> FlightInput {
        let axis = |neg: bool, pos: bool| (pos as i32 - neg as i32) as f32;
        let k = &self.keys;
        // Keyboard turn rate: radians per tick (enhanced model only).
        let key_turn = 2.2 * TICK_DT;
        // Whether the fullscreen map suspends movement input follows
        // the SELECTOR LAYOUT, per game law (player ruling
        // 2026-07-24): retail MC1's map replaces flight input with
        // the ball+arrow cursor for BOOK spell-picking, so a map
        // carrying the book (`map_book`) suspends; retail MC2's map
        // keeps NORMAL CONTROLS live — you fly HUD-less (keyboard
        // only here: mouse-look is grab-gated and the map frees the
        // pointer for the CTRL pane/roster). An MC1 run with the
        // selector in the MC2 position EXCLUSIVELY (no book on the
        // map) adopts the MC2 behavior wholesale.
        //
        // The abandon-confirm dialog always owns input (retail
        // replaces the movement read with the OkCancel event read,
        // PlayerInput.cpp:2356 — steering decays, speed persists,
        // the world keeps running).
        let book = (self.book_open() && self.selector.map_book) || self.exit_confirm;
        let mc1 = self.cfg.controls.models.thrust == config::ThrustModel::Classic;
        // MC2's barrel roll: both strafe keys down this tick with
        // NEITHER down the last — retail's edge detect against the
        // prev-frame strafe byte (PlayerInput.cpp:2088-97; you must
        // press both from neutral, and holding one and tapping the
        // other does nothing). The sim's flight-verb gate makes the
        // command MC2-only; the book map swallows it with the rest of
        // the movement input.
        let strafes = (k.left && !book, k.right && !book);
        let barrel_roll = strafes.0 && strafes.1 && !self.prev_strafe.0 && !self.prev_strafe.1;
        self.prev_strafe = strafes;
        // While a roll runs, the virtual stick recenters (retail's
        // driver zeroes `rollDelta` — the mouse stick — every tick,
        // EF:38957, so the roll ENDS with the stick centered too; a
        // parked pre-roll deflection must not snap back). X only, the
        // pitch axis stays live.
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.sim.barrel_rolling())
        {
            self.stick.x = 0.0;
        }
        // Explicit float up/down is the enhanced-altitude tier; the
        // classic altitude model has no vertical control at all.
        let lift_keys = self.cfg.controls.models.altitude == config::AltitudeModel::Enhanced;
        let mut input = FlightInput {
            thrust: axis(k.back, k.forward),
            // MC2's decode order makes RIGHT win when both strafes
            // are held (`sub_5F380` evaluates bit 8 last, EF:60793-96
            // — the drift a retail barrel-roller feels while both
            // keys are down). MC1's input module is untranscribed, so
            // it keeps the neutral cancel.
            strafe: if k.left && k.right && self.is_mc2() {
                1.0
            } else {
                axis(k.left, k.right)
            },
            lift: if lift_keys { axis(k.down, k.up) } else { 0.0 },
            yaw_delta: axis(k.turn_left, k.turn_right) * key_turn + self.mouse.yaw,
            pitch_delta: axis(k.pitch_down, k.pitch_up) * key_turn + self.mouse.pitch,
            // The book screen swallows the fire buttons (they bind
            // spells there, as in the original's map-screen input).
            fire_left: self.fire_held && self.grabbed && !book,
            fire_right: self.fire_right_held && self.grabbed && !book,
            equip_left: self
                .pending_equip
                .0
                .take()
                .map(mgc_sim::mc1::spells::SpellId),
            equip_right: self
                .pending_equip
                .1
                .take()
                .map(mgc_sim::mc1::spells::SpellId),
            mc2_select: self.pending_mc2_select.take(),
            spell_ring: self.pending_ring.take(),
            full_stop: std::mem::take(&mut self.pending_full_stop),
            respawn: std::mem::take(&mut self.pending_respawn),
            demolish: std::mem::take(&mut self.pending_demolish),
            barrel_roll,
            raw_dx: std::mem::take(&mut self.roll_dx)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16,
            ..Default::default()
        };
        if mc1 {
            // The retail MC2 fly assistant (PlayerInput.cpp:2001-09):
            // mouse untouched and no action pending for 0x30
            // consecutive polls recenters the cursor — our virtual
            // stick. Retail gates on the raw position + pending
            // action bytes; ours on the motion-reset counter + held
            // fire. Plain on/off, default OFF (deliberate: MC2's
            // retail default, and MC1 never had the option —
            // parked-cursor deflections persist, as retail MC1's
            // visible-cursor scheme did).
            let assist = self.cfg.controls.preferences.fly_assistant.on();
            if !assist || input.fire_left || input.fire_right {
                self.stick_idle_ticks = 0;
            } else if self.stick.x != 0.0 || self.stick.y != 0.0 {
                self.stick_idle_ticks = self.stick_idle_ticks.saturating_add(1);
                if self.stick_idle_ticks > 0x30 {
                    self.stick = VirtualStick::default();
                    self.stick_idle_ticks = 0;
                }
            }
            // The MC1 model steers from the virtual stick; the delta
            // accumulators stay zero (the sim ignores them, but keep
            // the recorded input honest for future replays).
            input.stick_x = self.stick.x.round() as i16;
            input.stick_y = self.stick.y.round() as i16;
            input.yaw_delta = 0.0;
            input.pitch_delta = 0.0;
        }
        if book {
            // The original's map/book modes write NO movement input
            // (:20635-:20744 never reach the mouse read or command 6)
            // — the steering filters decay to center while the speed
            // target persists (the "map fixes your orientation, not
            // your velocity" behavior).
            input.thrust = 0.0;
            input.strafe = 0.0;
            input.lift = 0.0;
            input.stick_x = 0;
            input.stick_y = 0;
            input.yaw_delta = 0.0;
            input.pitch_delta = 0.0;
        }
        self.mouse = MouseAccum::default();
        input
    }

    /// Push runtime world changes (dug terrain, moving/spawned/removed
    /// entities) to the renderer. Entities move every tick, so the
    /// billboard set refreshes per tick from the sim's pose snapshot;
    /// the map texture recompose (dots baked into it) is throttled to
    /// every 8th tick unless terrain actually changed.
    /// The smooth-motion pass (render.enhancement.smooth_motion):
    /// re-set the renderer's world drawables from the tick-pair pose
    /// lerp at this frame's accumulator fraction — entities join the
    /// camera's interpolated timeline instead of stepping at tick
    /// rate. Presentation only: runs after `sync_world` (which set
    /// the tick-rate versions) and overrides billboards, health bars
    /// and dynamic lights; the map layers stay per-tick (the map
    /// recomposes at tick rate by design). Skipped while paused (no
    /// live lerp window) or until two ticks have run since the
    /// toggle/level armed it.
    /// Is the procedural fire active? The `render.enhancement.fire`
    /// option, but it also needs smooth_motion (the flame is built on
    /// the interpolated pose timeline) — with smooth_motion off the
    /// classic sprites draw regardless of the option.
    fn enhanced_fire(&self) -> bool {
        self.cfg.render.enhancement.fire == config::FireEffects::Enhanced
            && self.cfg.render.enhancement.smooth_motion
    }

    /// Is the procedural lightning active? Same law as
    /// [`App::enhanced_fire`]: the option, gated on smooth_motion
    /// (the strike envelope runs on the interpolated timeline).
    fn enhanced_lightning(&self) -> bool {
        self.cfg.render.enhancement.lightning == config::LightningEffects::Enhanced
            && self.cfg.render.enhancement.smooth_motion
    }

    fn apply_smooth_motion(&mut self, alpha: f32) {
        let Some(sess) = self.session.as_deref() else {
            return;
        };
        if !self.cfg.render.enhancement.smooth_motion
            || self.paused
            || sess.pose_prev.is_empty()
            || sess.pose_cur.is_empty()
        {
            return;
        }
        let enhanced_fire = self.enhanced_fire();
        let enhanced_lightning = self.enhanced_lightning();
        let Some(r) = &mut self.renderer else { return };
        let poses = entities::lerp_poses(&sess.pose_prev, &sess.pose_cur, alpha.clamp(0.0, 1.0));
        let index = sess.level.sprites.as_ref().map(|(i, _)| i);
        let dims = |id: u16| {
            index
                .and_then(|i| i.sprites.get(id as usize))
                .map(|s| (s.width, s.height, s.flags))
        };
        let mut billboards = entities::billboards_from_poses(
            sess.level.game,
            &poses,
            dims,
            enhanced_fire,
            enhanced_lightning,
            self.cfg.gameplay.patches.mc2_dweller_invisibility.on(),
        );
        // The replay GHOST (④): the recorded pose as a translucent
        // wizard-carpet, riding beside the free-running sim — where
        // they overlap, the replay is visually exact.
        if let Some(d) = &self.replay
            && let Some(b) = replay::ghost_billboard(d, sess)
        {
            billboards.push(b);
        }
        r.set_billboards(billboards);
        // Enhanced fire: the procedural crater (walls + smoke) goes
        // FIRST so it wins density-cap slots; then the velocity-aware
        // projectile/impact particles, then the shockwave ring (all
        // blast-driven parts read the ledger). Classic clears the set.
        if enhanced_fire {
            let a = alpha.clamp(0.0, 1.0);
            let mut fire = entities::crater_particles(
                &sess.fire_blasts,
                &sess.level.view.height,
                a,
                self.effect_time,
            );
            fire.extend(entities::fire_particles_from_poses(
                &sess.pose_prev,
                &sess.pose_cur,
                &sess.fire_blasts,
                a,
                self.effect_time,
            ));
            fire.extend(entities::shockwave_particles(
                &sess.fire_blasts,
                &sess.level.view.height,
                a,
                self.effect_time,
            ));
            let fire = entities::cap_particle_density(fire);
            if std::env::var("MGC_FIRE_DEBUG").is_ok() {
                entities::debug_fire_stats(&fire, self.effect_time);
            }
            r.set_fire_particles(fire);
        } else {
            r.set_fire_particles(Vec::new());
        }
        // PROTOTYPE lightning: the strike envelope + fractal channel,
        // rebuilt per frame from the ledger (stateless, seed-frozen).
        if enhanced_lightning {
            r.set_bolt_segments(entities::bolt_segments(
                &sess.bolts,
                alpha.clamp(0.0, 1.0),
                self.effect_time,
            ));
        } else {
            r.set_bolt_segments(Vec::new());
        }
        if self.cfg.render.debug.health_bars {
            r.set_health_bars(entities::health_bars_from_poses(
                sess.level.game,
                &poses,
                dims,
            ));
        }
        if self.cfg.render.preference.light_sources
            && sess.level.mc2_env != entities::Mc2MapEnv::Day
        {
            r.set_lights(&entities::lights_from_poses(&poses));
        }
    }

    fn sync_world(&mut self) {
        let enhanced_fire = self.enhanced_fire();
        let enhanced_lightning = self.enhanced_lightning();
        // A fire/lightning-option flip must re-derive the sprite/
        // particle sets even while PAUSED (no ticks → entities never
        // dirty, and apply_smooth_motion skips) — treat it as an
        // entities change.
        let fire_changed = self.fire_applied != Some(enhanced_fire)
            || self.lightning_applied != Some(enhanced_lightning);
        self.fire_applied = Some(enhanced_fire);
        self.lightning_applied = Some(enhanced_lightning);
        let Some(sess) = self.session.as_deref_mut() else {
            return;
        };
        let Session {
            sim,
            level,
            pose_cur,
            fire_blasts,
            ..
        } = sess;
        let Some(w) = &mut sim.world else { return };
        // MC2: the sim's book owns the selected tier. `spell_levels` is
        // a read-only MIRROR of `Mc2Spellbook::sel`, refreshed here —
        // before any early return below — so the two can never be seen
        // disagreeing (docs/archive/DESIGN-SAVES.md prerequisite 3).
        //
        // They used to: `pane_commit` wrote the REQUESTED tier straight
        // into `spell_levels`, while `mc2_select_spell` clamps it to the
        // XP-earned cap and bails outright on an unlearned spell
        // (mc2/cast.rs:631,648). The mirror existed but only ran while
        // the pane was being drawn, so between a commit and the next
        // pane frame the tier NAME and the shift-commit tier were read
        // from a value the sim had rejected.
        if matches!(level.game, mgc_sim::ids::GameId::Mc2) {
            self.spell_levels = w.mc2_book_view().sel;
        }
        for slot in w.take_rival_deaths() {
            // The retail death broadcast ("%name% <str 54>",
            // :55499-517) — the sim raises the on-screen toast at the
            // moment of death (game-aware name table); this console
            // line is a dev-log echo, so pick the matching table too.
            let name = match level.game {
                mgc_sim::ids::GameId::Mc2 => mgc_sim::mc2::rivals::MC2_RIVAL_NAMES,
                _ => mgc_sim::mc1::rivals::RIVAL_NAMES,
            }
            .get(slot as usize)
            .copied()
            .unwrap_or("?");
            eprintln!("{name} has died");
        }
        let terrain = w.terrain_dirty;
        let entities = w.entities_dirty || fire_changed;
        if terrain {
            let (Some(shading), Some(angle)) =
                (level.view.shading.as_mut(), level.view.angle.as_mut())
            else {
                return;
            };
            w.copy_planes_into(mgc_sim::engine::features::TerrainPlanes {
                height: &mut level.view.height,
                tile_type: &mut level.view.tile_type,
                shading,
                angle,
            });
            // The live cave ceiling (pillars, Cave-In, the eases).
            if let Some(c) = level.view.ceiling.as_mut() {
                let live = w.ceiling_plane();
                if live.len() == c.len() {
                    c.copy_from_slice(live);
                }
            }
        }
        let mut bars = Vec::new();
        let mut lights = Vec::new();
        let mut icons_grew = false;
        if entities {
            let poses = w.live_poses();
            // Dynamic light sources (retail's Dynamic Lighting
            // option): fireballs/explosions/standing fire brighten
            // the terrain, Night/Cave only (retail's MapType gate —
            // the day tables invert, added rows would darken).
            if self.cfg.render.preference.light_sources && level.mc2_env != entities::Mc2MapEnv::Day
            {
                lights = entities::lights_from_poses(&poses);
            }
            let index = level.sprites.as_ref().map(|(i, _)| i);
            let dims = |id: u16| {
                index
                    .and_then(|i| i.sprites.get(id as usize))
                    .map(|s| (s.width, s.height, s.flags))
            };
            level.billboards = entities::billboards_from_poses(
                level.game,
                &poses,
                dims,
                enhanced_fire,
                enhanced_lightning,
                self.cfg.gameplay.patches.mc2_dweller_invisibility.on(),
            );
            if self.cfg.render.debug.health_bars {
                bars = entities::health_bars_from_poses(level.game, &poses, dims);
            }
            // Lazy marker-icon capture BEFORE the dot/stamp derive,
            // so a newly sighted family swaps layers the same tick.
            icons_grew =
                self.cfg.render.enhancement.map_marker_icons && capture_marker_icons(level, &poses);
            let swapped = dot_swap_set(level, &self.cfg);
            level.map_dots = entities::map_dots_from_poses(
                level.game,
                &poses,
                &level.palette_rgba,
                self.cfg.render.enhancement.map_owned_buildings,
                level.mc2_env,
                // MC1 derives its ~4 Hz claimed-ball blink from the
                // tick; MC2's colorIndex_121 phases divide it.
                sim.tick as u32,
                &swapped,
            );
            level.map_stamps = entities::map_stamps_from_poses(
                level.game,
                &poses,
                &level.map_icons,
                w.beyond_sight(),
                self.cfg.render.enhancement.expose_jar_spells,
                self.cfg.render.enhancement.map_marker_icons,
            );
            // The advertised-trigger X/O markers (MC1 flight-path
            // breadcrumbs + MC2 exit trips): unconditional (untripped
            // markers plot from level start — the map shows where the
            // trip WILL be).
            level.map_stamps.extend(entities::exit_marker_stamps(
                &w.advertised_marker_poses(),
                &level.map_icons,
            ));
            self.jar_markers = if self.cfg.render.enhancement.expose_jar_spells {
                entities::jar_markers_from_poses(&poses)
            } else {
                Vec::new()
            };
            // Beyond-Sight rival position markers (interim for the
            // retail name labels — DrawText track).
            let rival_views = w.rival_views();
            level
                .map_dots
                .extend(entities::rival_markers(&rival_views, w.beyond_sight_tier()));
            // The rival tag's smooth-motion pair: this snapshot and
            // the one before it (drawn lerped by the sub-tick alpha).
            self.rival_tags_prev = std::mem::replace(&mut self.rival_tags_cur, rival_views);
            self.castle_pos = poses
                .iter()
                .find(|p| p.class == 3 && p.model == 2 && p.player_owned)
                .map(|p| (p.x, p.z));
            level.map_areas = map_areas(w);
            // MC2 objective-guide targets (non-optional): the current
            // objective's live world targets → blinking marks + a steer
            // arrow. Empty off-MC2 (mc2_stages empty), so MC1/HW draw
            // nothing.
            level.objective_marks = w
                .mc2_objective_targets()
                .into_iter()
                .map(|t| mgc_render::ObjectiveMark {
                    x: t.x,
                    z: t.z,
                    nearest: t.nearest,
                    yellow: t.yellow,
                })
                .collect();
        }
        w.terrain_dirty = false;
        w.entities_dirty = false;
        let overlay = map_overlay(level, &self.cfg);
        if let Some(r) = &mut self.renderer {
            if entities {
                r.set_billboards(level.billboards.clone());
                r.set_health_bars(bars);
                r.set_lights(&lights);
            }
            // The paused fire flip: swap the particle set in place
            // (unpaused, apply_smooth_motion re-derives it right after
            // this with the proper sub-tick alpha anyway).
            if fire_changed {
                if enhanced_fire && !pose_cur.is_empty() {
                    let mut fire = entities::crater_particles(
                        fire_blasts,
                        &level.view.height,
                        1.0,
                        self.effect_time,
                    );
                    fire.extend(entities::fire_particles_from_poses(
                        pose_cur,
                        pose_cur,
                        fire_blasts,
                        1.0,
                        self.effect_time,
                    ));
                    fire.extend(entities::shockwave_particles(
                        fire_blasts,
                        &level.view.height,
                        1.0,
                        self.effect_time,
                    ));
                    let fire = entities::cap_particle_density(fire);
                    if std::env::var("MGC_FIRE_DEBUG").is_ok() {
                        entities::debug_fire_stats(&fire, self.effect_time);
                    }
                    r.set_fire_particles(fire);
                } else {
                    r.set_fire_particles(Vec::new());
                }
            }
            // A lazy icon capture grew the UI atlas — re-upload it so
            // the new miniature has texels to sample (rare: once per
            // newly sighted family per level).
            if icons_grew && let Some(assets) = &level.ui {
                r.load_ui_atlas(assets.atlas_w, assets.atlas_h, &assets.atlas_rgba);
            }
            // Upright map icons + the guide path are drawn screen-
            // space by the renderer (never baked into the rotated map
            // texture: icons stay upright, ant spacing stays 4
            // surface px under rotation/zoom). Frontend screens never
            // reach here — the teardown clears these layers.
            r.set_map_stamps(level.map_stamps.clone());
            r.set_map_path(self.castle_pos.map(|(cx, cz)| mgc_render::MapPath {
                from: (sim.flyer.x, sim.flyer.z),
                to: (cx, cz),
                phase: (sim.tick & 3) as u8,
            }));
            // The objective-guide blink is tick-driven (retail
            // gates: outline 1-in-4, arrow 5-then-pause) — see
            // project_objective_marks.
            r.set_objective_marks(level.objective_marks.clone(), sim.tick as u32);
            if terrain {
                r.update_terrain(&level.view, &overlay);
                self.last_map_tick = Some(sim.tick);
            } else if self.last_map_tick != Some(sim.tick) {
                // The map recomposes once per SIM TICK — everything
                // baked in it (dots, blink phase tick>>3) changes at
                // tick rate, so per-frame recompose (a 256×256 LUT
                // walk + full texture upload) buys nothing. The
                // marching ants march per frame regardless — they're
                // screen-space.
                r.update_map(&level.view, &overlay);
                self.last_map_tick = Some(sim.tick);
            }
        }
    }

    /// While PAUSED the sim never consumes `pending_equip` (no ticks
    /// run), so the HUD hand icons wouldn't redraw until unpause —
    /// apply book bindings to the world immediately instead (binding
    /// is UI state, not simulation).
    fn flush_equip_if_paused(&mut self) {
        if !self.paused {
            return;
        }
        if let Some(w) = self
            .session
            .as_deref_mut()
            .and_then(|s| s.sim.world.as_mut())
        {
            let l = self
                .pending_equip
                .0
                .take()
                .map(mgc_sim::mc1::spells::SpellId);
            let r = self
                .pending_equip
                .1
                .take()
                .map(mgc_sim::mc1::spells::SpellId);
            w.equip_hands(l, r);
            if let Some((spell, tier, hand)) = self.pending_mc2_select.take() {
                w.mc2_select_spell(spell, tier, hand);
            }
            if let Some((spell, val)) = self.pending_ring.take() {
                w.spell_ring_set(spell, val);
            }
        }
    }

    /// The spell's current cycle-ring membership (0/1=left/2=right),
    /// whichever game's column holds it.
    fn ring_of(&self, spell: u8) -> u8 {
        let Some(w) = self.session.as_deref().and_then(|s| s.sim.world.as_ref()) else {
            return 0;
        };
        let s = spell as usize;
        if self.is_mc2() {
            w.mc2_book_view().ring.get(s).copied().unwrap_or(0)
        } else {
            w.loadout().ring.get(s).copied().unwrap_or(0)
        }
    }

    /// The in-flight cycle walk (`sub_18DA0` PI:1839-1942): step from
    /// the equipped spell ±1, wrap 0..n, take the first spell that is
    /// BOTH possessed and a member of this button's ring; a full lap
    /// with no qualifier does nothing (the all-unavailable no-op —
    /// vanished MC1 spells and undead-stolen MC2 spells stay members,
    /// they are just skipped). A single-member ring re-selects itself.
    /// The equip carries the spell's STORED level (`array_0x437`).
    fn cycle_spell_ring(&mut self, hand: u8, backward: bool) {
        let Some(w) = self.session.as_deref().and_then(|s| s.sim.world.as_ref()) else {
            return;
        };
        let side = hand + 1;
        let (ring, owned, cur, sel) = if self.is_mc2() {
            let bv = w.mc2_book_view();
            let mut owned = bv.owned;
            if self.cfg.gameplay.cheat.dev_spells {
                owned = [true; 26];
            }
            let cur = if hand == 0 { bv.left } else { bv.right };
            (
                bv.ring.to_vec(),
                owned.to_vec(),
                cur as i32,
                bv.sel.to_vec(),
            )
        } else {
            let l = w.loadout();
            let cur = if hand == 0 { l.left } else { l.right };
            (
                l.ring.to_vec(),
                l.owned.to_vec(),
                cur.map_or(-1, |s| s as i32),
                vec![0u8; 24],
            )
        };
        let Some(s) = ring_next(&ring, &owned, side, cur, backward) else {
            return; // no possessed member on this ring — do nothing
        };
        if self.is_mc2() {
            // Cmd 31/32 with byte2 = the stored level.
            self.pending_mc2_select = Some((s as u8, sel[s], hand));
        } else if hand == 0 {
            self.pending_equip.0 = Some(s as u8);
        } else {
            self.pending_equip.1 = Some(s as u8);
        }
        self.flush_equip_if_paused();
    }

    /// The CTRL selector pane is up (hold-to-show; needs the pane
    /// surface enabled and UI sprites baked).
    fn pane_open(&self) -> bool {
        self.ctrl_held
            && self.pane.is_some()
            && self
                .session
                .as_deref()
                .is_some_and(|s| s.level.ui.is_some())
    }

    /// The spell's display name at its CURRENTLY selected tier (MC2
    /// reads the mirror of the sim's book; MC1 has no tiers).
    fn pane_spell_name(&self, spell: u8) -> &'static str {
        self.pane_spell_name_at(spell, self.spell_levels[spell as usize])
    }

    /// As [`Self::pane_spell_name`] but at an explicit tier — for the
    /// commit path, where the requested tier is known before the sim
    /// has accepted (and possibly clamped) it.
    fn pane_spell_name_at(&self, spell: u8, tier: u8) -> &'static str {
        if self.is_mc2() {
            // The retail per-TIER hint name (docs/spell-audit/
            // spell-names.md): "Possession" / "Mana Magnet" / "Mana Lock"
            // by level, not one generic label. Resolves the live
            // hint_text so the Day/non-Day Morph/Army names come through.
            let tier = tier as usize;
            let name = self
                .session
                .as_deref()
                .and_then(|s| s.sim.world.as_ref())
                .map(|w| w.mc2_spell_name(spell as usize, tier))
                .unwrap_or("");
            if name.is_empty() {
                ui::MC2_SPELL_NAMES[spell as usize]
            } else {
                name
            }
        } else {
            mgc_sim::mc1::spells::SpellId(spell).name()
        }
    }

    /// Commit a pane selection: persist the spell's chosen level (the
    /// original's `array_0x437[spell] = level`, every route reuses
    /// it) and bind the spell to the clicked hand.
    fn pane_commit(&mut self, slot: usize, hand: u8, level: u8) {
        let Some(pane) = &self.pane else { return };
        let spell = pane.order[slot];
        let multi = pane.levels > 1;
        self.pane_bound[hand as usize] = Some(spell);
        let hand_name = if hand == 0 { "left" } else { "right" };
        if self.is_mc2() {
            // The native MC2 spell column: the pane commit IS
            // retail's "Change Spell" action — tier +
            // quick-slot bind through the sim's class-15 machinery.
            //
            // NB no `spell_levels` write here: under MC2 the sim's book
            // is authoritative and `sync_world` mirrors it back. Writing
            // the REQUESTED tier would re-open the disagreement the
            // mirror exists to prevent — the sim clamps to the earned
            // cap and rejects an unlearned spell outright, so the two
            // routinely differ.
            self.pending_mc2_select = Some((spell, level, hand));
            self.flush_equip_if_paused();
            // Logged at the requested tier, not `spell_levels`: while
            // running, the select is still queued for the next tick and
            // the mirror has not caught up yet.
            let name = self.pane_spell_name_at(spell, level);
            if multi {
                println!("selector: {hand_name} hand = {name} level {}", level + 1);
            } else {
                println!("selector: {hand_name} hand = {name}");
            }
            return;
        }
        // MC1: the tier is app-owned (there is no sim-side book to be
        // authoritative), so record it here.
        self.spell_levels[spell as usize] = level;
        // MC1: pane spell = the MC1 manifestation directly.
        if hand == 0 {
            self.pending_equip.0 = Some(spell);
        } else {
            self.pending_equip.1 = Some(spell);
        }
        self.flush_equip_if_paused();
        println!(
            "selector: {hand_name} hand = {}",
            self.pane_spell_name(spell)
        );
    }

    /// Enter the MC2 world-map screen (the between-levels hub). The
    /// running session (if any) is torn down — the frontend owns the
    /// app from here; a portal click constructs the next one. Falls
    /// back to a direct next-level launch when the mc2-ui bundle is
    /// missing (older bake).
    fn open_map_screen(&mut self, event_loop: &ActiveEventLoop) {
        // Remember which level was just played before the teardown —
        // the carpet parks there.
        let current = self.campaign.as_ref().map(|c| c.current);
        self.teardown_session();
        // Back on the map the run is no longer IN a level: it is
        // waiting on the next one, exactly as it would be after
        // reloading this slot. Without this, `current` kept naming the
        // level just played — so a save taken here recorded the level
        // already finished, and the fallback launches below replayed it
        // instead of moving on.
        if let Some(run) = &mut self.campaign
            && let Some(pending) = run.save.mc2().map(mc2_pending_level)
        {
            run.current = pending;
        }
        if self.worldmap.is_none()
            && let Err(e) = self.load_worldmap()
        {
            eprintln!("note: world-map screen unavailable: {e}");
            let n = self
                .campaign
                .as_ref()
                .and_then(|c| c.save.mc2())
                .map_or(0, |s| s.levels_completed);
            if n >= 25 {
                println!("campaign complete!");
                event_loop.exit();
            } else {
                self.campaign_switch(n, event_loop);
            }
            return;
        }
        self.screen = Screen::Map;
        self.confine_map_pointer();
        // Map entry: the carpet parks on the level just played
        // (completed, failed or replayed), the camera anchors, the
        // narrative latch re-arms; the new portal pops on sight.
        if let (Some(wm), Some(save), Some(cur)) = (
            &mut self.worldmap,
            self.campaign.as_ref().and_then(|c| c.save.mc2()),
            current,
        ) {
            wm.enter_visit(save);
            wm.anchor_to(save);
            wm.set_parked(cur);
        }
        self.frontend_music();
    }

    /// Confine the OS pointer for the world-map screen (retail
    /// captures the mouse: the cursor cannot leave the screen and
    /// edge contact scrolls the map). The flight grab is released —
    /// map mouse input is absolute, not relative. Hidden cursor: the
    /// screen draws the retail cursor sprite itself.
    fn confine_map_pointer(&mut self) {
        self.grabbed = false;
        if let Some(w) = &self.window {
            w.set_cursor_grab(CursorGrabMode::Confined).ok();
            w.set_cursor_visible(false);
        }
    }

    /// The MC2 main-menu pointer: FREE — only the map needs the
    /// confinement for edge scrolling, and the game the flight lock;
    /// the menu needs neither. The OS cursor
    /// stays hidden over the window because the temple screen draws
    /// the retail cursor sprite itself.
    fn free_menu_pointer(&mut self) {
        self.grabbed = false;
        if let Some(w) = &self.window {
            // Same fullscreen rule as `set_grab(false)`: the invisible
            // menu pointer must not wander off the covered monitor.
            let confined = w.fullscreen().is_some()
                && w.has_focus()
                && w.set_cursor_grab(CursorGrabMode::Confined).is_ok();
            if !confined {
                w.set_cursor_grab(CursorGrabMode::None).ok();
            }
            w.set_cursor_visible(false);
        }
    }

    /// Load the world-map assets and park the scroll on the next
    /// portal.
    fn load_worldmap(&mut self) -> Result<(), String> {
        let mut wm = worldmap::WorldMap::load(&get_baked_directory().join("assets/mc2-ui"))?;
        if let Some(run) = &self.campaign {
            if let Some(save) = run.save.mc2() {
                wm.enter_visit(save);
                wm.anchor_to(save);
            }
            // On RESUME `current` is the pending level — retail
            // parks the carpet on the last activated flag instead
            // (its load law); mid-session entries pass the level
            // actually just played via `open_map_screen`.
            let parked = match self.campaign.as_ref().and_then(|c| c.save.mc2()) {
                Some(s) if run.current == s.levels_completed && run.current > 0 => run.current - 1,
                _ => run.current,
            };
            wm.set_parked(parked);
        }
        self.worldmap = Some(wm);
        self.confine_map_pointer();
        self.frontend_music();
        Ok(())
    }

    /// One world-map frame: lazy first entry (the MC2 campaign boots
    /// with the flag already set), pan + animate + travel, swap the
    /// map atlas in, and replace this frame's UI quads with the
    /// screen. An arrived click-travel launches its level.
    fn map_screen_frame(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        if self.worldmap.is_none()
            && let Err(e) = self.load_worldmap()
        {
            eprintln!("note: world-map screen unavailable: {e} — launching directly");
            let n = self.campaign.as_ref().map_or(0, |c| c.current);
            self.campaign_switch(n, event_loop);
            return;
        }
        // The options menu (P) rides OVER the map screen — the
        // frontend frame draws it; this frame pauses.
        if self.menu.is_some() {
            return;
        }
        let size = self.view_size();
        let cursor = self.cursor;
        let pan = 420.0 * dt;
        let pan_x = (self.keys.right as i32 - self.keys.left as i32) as f32 * pan;
        let pan_y = (self.keys.back as i32 - self.keys.forward as i32) as f32 * pan;
        // Retail pointer scroll (MI:3138-60): the confined cursor
        // sitting ON the boundary pixel scrolls — x==0 / x>=638 /
        // y==0 / y>=478 in the 640×480 screen space (the CursorMoved
        // clamp guarantees the edge is reachable). Sub-pixel window
        // scales widen the test to one native pixel.
        //
        // The map is letterboxed, so "the edge" is the PICTURE's, not
        // the window's — and the test reads as "at or BEYOND it", which
        // takes in the whole bar. That matters because the confined
        // pointer can rest anywhere in the bar: an edge-only trigger
        // would leave a dead strip where the cursor is off the map and
        // nothing scrolls. `unletterbox` maps bar positions outside
        // 0..VIEW on whichever axis is barred — right/left on a wide
        // window, top/bottom on a narrow one — so the same two
        // comparisons per axis serve both cases unchanged.
        let edge_dir = {
            let (mx, my) = ui::unletterbox(cursor, size, 640.0, 480.0);
            let dx = if mx < 1.0 {
                -1.0
            } else if mx >= 638.0 {
                1.0
            } else {
                0.0
            };
            let dy = if my < 1.0 {
                -1.0
            } else if my >= 478.0 {
                1.0
            } else {
                0.0
            };
            (dx, dy)
        };
        let Some(save) = self.campaign.as_ref().and_then(|c| c.save.mc2()) else {
            // No MC2 campaign behind the map — nothing to show.
            self.enter_main_menu();
            return;
        };
        let Some(wm) = &mut self.worldmap else { return };
        wm.tick(dt, save);
        wm.pan(pan_x, pan_y);
        wm.edge_scroll(edge_dir, dt);
        let sounds = wm.take_sounds();
        let launch = wm.take_launch();
        let narrative = wm.take_narrative();
        // Letterbox black under the 4:3 screen, then the map quads
        // (skipped when this frame closes the screen).
        let quads = (launch.is_none()).then(|| {
            let mut q = vec![ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, 1.0])];
            q.extend(wm.quads(save, size, cursor));
            q
        });
        // Map-screen UI samples (portal-open 41, travel 19) through
        // the normal mixer path.
        if !sounds.is_empty()
            && self.cfg.audio.sound
            && let Some(a) = &mut self.audio
        {
            // Frontend samples are player-sourced — the listener
            // position is moot; centered serves.
            let listener = mgc_audio::Listener {
                pos: (0, 0, 0),
                yaw: 0,
            };
            for id in sounds {
                a.event(id, mgc_audio::Source::Player, &listener);
            }
        }
        // The pending level's briefing narrative (retail
        // `PresentLevelDescription`: speech row = level, segment 0;
        // the description TEXT rides the deferred map text/overlay
        // work — the map bank's own font glyphs).
        if let Some(lvl) = narrative
            && self.cfg.audio.speech
            && let Some(a) = &mut self.audio
            && let Err(e) = a.play_speech(lvl, 0)
        {
            eprintln!("note: map narrative: {e}");
        }
        if let Some(r) = &mut self.renderer {
            if self.ui_atlas != UiAtlas::MapScreen {
                let (w, h, px) = wm.atlas();
                r.load_ui_atlas(w, h, px);
                self.ui_atlas = UiAtlas::MapScreen;
            }
            if let Some(q) = quads {
                r.set_ui_quads(q);
            }
        }
        // Corner-button + dialog outcomes (retail mapMenuButtons).
        let button = self.worldmap.as_mut().and_then(|wm| wm.take_button());
        if let Some(btn) = button {
            use worldmap::{DialogKind, MapButton};
            match btn {
                MapButton::Save => {
                    let slots = scan_mc2_slots();
                    if let Some(wm) = &mut self.worldmap {
                        wm.open_dialog(DialogKind::Save, slots);
                    }
                }
                MapButton::Load => {
                    let slots = scan_mc2_slots();
                    if let Some(wm) = &mut self.worldmap {
                        wm.open_dialog(DialogKind::Load, slots);
                    }
                }
                MapButton::NewGame => {
                    if let Some(wm) = &mut self.worldmap {
                        wm.open_dialog(DialogKind::NewGame, Vec::new());
                    }
                }
                MapButton::Exit => self.enter_main_menu(),
            }
        }
        let action = self.worldmap.as_mut().and_then(|wm| wm.take_action());
        if let Some(a) = action {
            self.apply_map_action(a);
        }
        if let Some(n) = launch {
            // Release the map confinement (flight re-grabs on click).
            self.set_grab(false);
            self.campaign_switch(n, event_loop);
        }
    }

    /// A committed world-map frontend action (corner buttons').
    fn apply_map_action(&mut self, action: worldmap::MapAction) {
        use worldmap::MapAction;
        match action {
            MapAction::SaveTo { slot } => {
                if let Some(run) = &mut self.campaign {
                    run.slot = Some(slot);
                    if let Some(s) = run.save.mc2_mut() {
                        // The slot's stored label is the PLAYER NAME —
                        // never a per-slot string, and never the
                        // rendered row. The list composes the rest
                        // (level, progress) at draw time; storing a
                        // composed row was how the suffix accumulated.
                        if !s.player_name.trim().is_empty() {
                            s.label = s.player_name.clone();
                        }
                    }
                    run.persist();
                }
            }
            MapAction::LoadFrom(slot) => {
                match CampaignRun::start(campaign::CampaignId::Mc2, Some(slot), false) {
                    Ok(run) => {
                        let parked = match run.save.mc2() {
                            Some(s) if run.current == s.levels_completed && run.current > 0 => {
                                run.current - 1
                            }
                            _ => run.current,
                        };
                        self.campaign = Some(run);
                        // A slot saved mid-level resumes INTO that
                        // level rather than dropping the player on the
                        // map to replay it from the start. Tried first,
                        // so the map dressing below is skipped when we
                        // are leaving the map anyway.
                        //
                        // The grab release mirrors the map's ordinary
                        // portal launch: the map CONFINES the pointer,
                        // and flight re-grabs on the next click.
                        self.set_grab(false);
                        match self.resume_slot(slot) {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => eprintln!("error: resume slot {}: {e}", slot + 1),
                        }
                        if let (Some(wm), Some(save)) = (
                            &mut self.worldmap,
                            self.campaign.as_ref().and_then(|c| c.save.mc2()),
                        ) {
                            wm.session_reset();
                            wm.enter_visit(save);
                            wm.anchor_to(save);
                            wm.set_parked(parked);
                        }
                    }
                    Err(e) => eprintln!("error: load: {e}"),
                }
            }
            MapAction::NewGame => {
                // Retail sub_7E640: full campaign reset, in memory
                // only (the file is written when the player saves) —
                // the map stays up with everything re-hidden.
                if let Some(run) = &mut self.campaign {
                    run.current = 0;
                    run.next = None;
                    if let Some(s) = run.save.mc2_mut() {
                        let label = s.label.clone();
                        let player_name = s.player_name.clone();
                        *s = saves::Mc2Save {
                            label,
                            player_name,
                            ..Default::default()
                        };
                    }
                }
                if let (Some(wm), Some(save)) = (
                    &mut self.worldmap,
                    self.campaign.as_ref().and_then(|c| c.save.mc2()),
                ) {
                    wm.session_reset();
                    wm.enter_visit(save);
                    wm.anchor_to(save);
                }
                println!("campaign restarted (unsaved until you save)");
            }
            MapAction::ExitToMenu => self.enter_main_menu(),
        }
    }

    /// Enter the campaign's main menu (boot state; the MC2 map's
    /// Exit target; MC1's between-level beat). Any running session is
    /// torn down — the frontend owns the app from here.
    fn enter_main_menu(&mut self) {
        self.teardown_session();
        // The map's sounds die with it too: cut the narration
        // mid-clip; the burst one-shots are short enough to ring out.
        if let Some(a) = &mut self.audio {
            a.stop_speech();
        }
        // Commit the mode BEFORE the fallible asset loads: a load
        // failure then lands in the menu frame's own fallback
        // (direct-launch / map hub) instead of a dead Level screen
        // with no session.
        self.screen = Screen::Menu;
        self.ui_atlas = UiAtlas::None; // entry refresh (slots, timer)
        self.frontend_music();
        if self.is_mc2() {
            if self.mainmenu.is_none() {
                match frontend::MainMenu::load(&get_baked_directory().join("assets/mc2-ui")) {
                    Ok(m) => self.mainmenu = Some(m),
                    Err(e) => {
                        eprintln!("note: main menu unavailable: {e}");
                        return;
                    }
                }
            }
            self.free_menu_pointer();
        } else {
            if self.mc1menu.is_none() {
                match frontend_mc1::Mc1Menu::load(&get_baked_directory().join("assets/mc1-ui")) {
                    Ok(m) => self.mc1menu = Some(m),
                    Err(e) => {
                        eprintln!("note: main menu unavailable: {e}");
                        return;
                    }
                }
            }
            // MC1 menu: free OS cursor (retail's own pointer art is
            // the SPTRS bank — not baked; the OS pointer stands in).
            self.set_grab(false);
        }
    }

    /// One frontend frame (`screen != Level`): the P options menu
    /// over a frozen screen, or the live menu/map frame.
    fn frontend_frame(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        // The launch intro, on the first frontend frame there is a
        // window to show it in. Retail runs it BEFORE the main menu
        // (a campaign booted straight into a level — `--level` — gets
        // no intro, same as retail's own level shortcut).
        if std::mem::take(&mut self.boot_intro) && self.screen == Screen::Menu {
            let cues = self.intro_movies();
            self.play_movies(&cues, AfterMovie::Menu, event_loop);
        }
        // The mixer flush is tick-denominated (24 Hz fade ramps +
        // the voiceover-duck recovery); with no sim ticking, pump it
        // from wall time so the map's ambient bursts (screams,
        // volcano whooshes, the falling-star loops) and the menu
        // clicks actually reach the output, and menu music recovers
        // from the narration duck. (While P-paused the output is
        // suspended — requests queue and flush on resume, the retail
        // deferred-ding quirk, same as in-level.)
        self.frontend_audio_accum += dt;
        while self.frontend_audio_accum >= TICK_DT {
            self.frontend_audio_accum -= TICK_DT;
            if let Some(a) = &mut self.audio {
                a.tick();
            }
        }
        // The options menu rides OVER the frontend: the screen
        // beneath freezes and the menu draws on black with the
        // frontend-owned level-UI bank (fonts/panels).
        if self.menu.is_some() {
            self.ensure_frontend_ui();
            let size = self.view_size();
            let mut quads = vec![ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, 1.0])];
            let assets = match &self.session {
                Some(sess) => sess.level.ui.as_ref(),
                None => self.frontend_ui.as_ref(),
            };
            if let (Some(assets), Some(st)) = (assets, &self.menu) {
                quads.extend(menu::draw(
                    assets,
                    &self.cfg,
                    &self.specs,
                    st,
                    size.0,
                    size.1,
                    self.cursor,
                ));
            }
            self.append_software_cursor(&mut quads);
            if let Some(r) = &mut self.renderer {
                if self.ui_atlas != UiAtlas::FrontendUi
                    && let Some(a) = &self.frontend_ui
                {
                    r.load_ui_atlas(a.atlas_w, a.atlas_h, &a.atlas_rgba);
                    self.ui_atlas = UiAtlas::FrontendUi;
                }
                r.set_ui_quads(quads);
            }
            return;
        }
        match self.screen {
            Screen::Menu => self.menu_screen_frame(dt, event_loop),
            Screen::Map => self.map_screen_frame(dt, event_loop),
            Screen::Movie => self.movie_screen_frame(dt, event_loop),
            // Level with no session (a failed campaign launch mid-
            // exit): nothing to draw.
            Screen::Level => {}
        }
    }

    /// The launch intro chain, per campaign.
    ///
    /// MC1/HW run a dispatcher state machine — INTEL → LOGO → INTRO →
    /// TITLE → main menu (`sub_4AB20_4AE60`, remc1:57879-57907) — with
    /// an 8 s hold on the logo and 6 s on the title
    /// (`sub_4B480_4B7C0`). INTEL.DAT is the Intel Pentium branding
    /// bumper and plays ONLY on a CPUID family-5/model-1 part
    /// (`sub_19470`, remc1:19475), so it never plays here; MC2 ships
    /// the file but never references it at all.
    ///
    /// MC2 runs a welcome still, then INTRO and INTRO2
    /// (`Intros_76D10`, MenusAndIntros.cpp:736-800). The still is the
    /// HSCREEN0 welcome screen, not a movie, and is not yet drawn.
    ///
    /// The MC1 title's animated overlay (TITLE-02/04, a 4-frame loop
    /// composited over the held title) is not ported — the title holds
    /// static. See docs/FIDELITY.md.
    fn intro_movies(&self) -> Vec<movie::Cue> {
        if self.is_mc2() {
            vec![movie::Cue::new("intro"), movie::Cue::new("intro2")]
        } else {
            let hw = self
                .campaign
                .as_ref()
                .is_some_and(|c| c.id == campaign::CampaignId::Mc1Hw);
            // Hidden Worlds swaps in its own title art (remc1:60305).
            let title = if hw { "title-03" } else { "title-01" };
            vec![
                movie::Cue::new("logo").holding(8.0),
                movie::Cue::new("intro"),
                movie::Cue::new(title).holding(6.0),
            ]
        }
    }

    /// MC2's between-level cutscene for a just-completed level, if it
    /// has one and it has not already played.
    ///
    /// The table is `cutScene_E16E0` (MenusAndIntros.cpp:189), which
    /// stores `levelIndex + 1` and so fires CUT1-5 after level indices
    /// 4, 8, 12, 16 and 23. CUT6 belongs to level 24 and is the
    /// campaign ending — it is played from the outro seam instead, so
    /// it is not in this table.
    ///
    /// Retail marks each entry `overplayed_5` when it plays and never
    /// resets or persists the flag, so a cutscene shows once per
    /// process; `cutscenes_played` reproduces that.
    fn mc2_cutscene(&mut self, level: u32) -> Option<movie::Cue> {
        if !self.is_mc2() {
            return None;
        }
        let slot = [4u32, 8, 12, 16, 23].iter().position(|&l| l == level)?;
        if std::mem::replace(&mut self.cutscenes_played[slot], true) {
            return None;
        }
        // Retail plays the cutscenes unskippable
        // (`PlayInfoFmv(0, ..)`, MenusAndIntros.cpp:4142).
        Some(movie::Cue::unskippable(
            ["cut1", "cut2", "cut3", "cut4", "cut5"][slot],
        ))
    }

    /// Start an FMV chain from the running campaign's movie bundle,
    /// then do `then`. Falls straight through to `then` when the
    /// movies are unavailable (no bundle, or an install without them)
    /// or the player has them switched off — a missing movie must
    /// never strand the campaign.
    fn play_movies(&mut self, cues: &[movie::Cue], then: AfterMovie, event_loop: &ActiveEventLoop) {
        let mc2 = self.is_mc2();
        let player = if self.cfg.render.preference.movies {
            let dir = get_baked_directory().join(if mc2 {
                "assets/mc2-movies"
            } else {
                "assets/mc1-movies"
            });
            movie::MoviePlayer::new(
                &dir,
                cues,
                mc2,
                self.cfg.render.preference.movie_subtitles,
                self.cfg.gameplay.patches.win2_movie_score.on(),
            )
        } else {
            None
        };
        match player {
            Some(p) => {
                // A movie owns the whole screen with no level behind
                // it. Dropping the session here also kills the level's
                // looping sounds, its narration and the danger-music
                // ramp, none of which may bleed under a cutscene; the
                // continuation tears down again, harmlessly.
                self.teardown_session();
                self.movie = Some(p);
                self.movie_then = then;
                self.screen = Screen::Movie;
                self.ui_atlas = UiAtlas::None;
                self.set_grab(false);
                // The movies carry no audio of their own — their
                // score is cued frame by frame out of the event
                // script, so whatever was playing stops here and the
                // script starts the right track.
                if let Some(a) = &mut self.audio {
                    a.stop_music();
                }
            }
            None => self.finish_movies(then, event_loop),
        }
    }

    /// One FMV frame: decode on the 20 fps clock, present the 320×200
    /// canvas letterboxed, and hand over to the continuation when the
    /// chain runs out.
    fn movie_screen_frame(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        let size = self.view_size();
        let Some(player) = &mut self.movie else {
            let then = self.movie_then;
            self.finish_movies(then, event_loop);
            return;
        };
        player.tick(dt);
        // The script's audio cues. The FLIC container holds no audio
        // stream, so these — the narration clips, the effects and the
        // MIDI score — ARE the movie's soundtrack.
        let actions = player.take_actions();
        if let Some(a) = &mut self.audio {
            for action in actions {
                match action {
                    movie::Action::Music { track, looped } => {
                        if self.cfg.audio.music
                            && let Err(e) = a.play_music(track, looped)
                        {
                            eprintln!("note: movie music {track}: {e}");
                        }
                    }
                    movie::Action::StopMusic => a.stop_music(),
                    movie::Action::Bank(bank) => a.set_movie_bank(bank),
                    movie::Action::Sample { id, looped } => {
                        if self.cfg.audio.sound
                            && let Err(e) = a.play_movie_sample(id, looped)
                        {
                            eprintln!("note: movie sample {id}: {e}");
                        }
                    }
                    movie::Action::StopSample(id) => a.stop_movie_sample(id),
                    movie::Action::StopSamples => a.stop_movie_samples(),
                }
            }
        }
        let Some(player) = &mut self.movie else {
            return;
        };
        if player.done() {
            self.movie = None;
            let then = self.movie_then;
            self.finish_movies(then, event_loop);
            return;
        }
        let (rgba, quads) = player.frame(size);
        if let Some(r) = &mut self.renderer {
            r.load_ui_atlas(
                movie::MoviePlayer::W as u32,
                movie::MoviePlayer::H as u32,
                rgba,
            );
            self.ui_atlas = UiAtlas::Movie;
            // Black behind, so a non-4:3 window letterboxes rather
            // than showing whatever the last screen left.
            let mut q = vec![ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, 1.0])];
            q.extend(quads);
            r.set_ui_quads(q);
        }
    }

    /// Leave the movie screen for whatever the chain was covering.
    fn finish_movies(&mut self, then: AfterMovie, event_loop: &ActiveEventLoop) {
        self.movie = None;
        self.ui_atlas = UiAtlas::None;
        match then {
            AfterMovie::Menu => self.enter_main_menu(),
            AfterMovie::Map => self.open_map_screen(event_loop),
            AfterMovie::Quit => event_loop.exit(),
        }
    }

    /// One main-menu frame, dispatched per campaign.
    fn menu_screen_frame(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        if self
            .campaign
            .as_ref()
            .is_some_and(|c| c.id != campaign::CampaignId::Mc2)
        {
            self.menu_screen_frame_mc1(dt, event_loop);
        } else {
            self.menu_screen_frame_mc2(dt, event_loop);
        }
    }

    /// One MC1/HW menu frame: CPU-composed 320×200 screen (globe +
    /// timer + brighten highlight), re-uploaded as the UI atlas.
    fn menu_screen_frame_mc1(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        if self.mc1menu.is_none() {
            match frontend_mc1::Mc1Menu::load(&get_baked_directory().join("assets/mc1-ui")) {
                Ok(m) => {
                    self.mc1menu = Some(m);
                    self.set_grab(false);
                }
                Err(e) => {
                    // Menu asset unavailable: launch the pending
                    // level directly.
                    eprintln!("note: main menu unavailable: {e} — launching directly");
                    let n = self.campaign.as_ref().map_or(0, |c| c.current);
                    self.campaign_switch(n, event_loop);
                    return;
                }
            }
        }
        let size = self.view_size();
        let cursor = self.cursor;
        // Entry refresh: slot labels + the timer's game-underway
        // state (retail `!byte_9687C`).
        if self.ui_atlas != UiAtlas::MenuMc1 {
            let (tag, active) = self
                .campaign
                .as_ref()
                .map(|c| (c.id.tag(), c.current > 0))
                .unwrap_or(("mc1", false));
            let slots = scan_mc1_slots(tag);
            let name = self
                .campaign
                .as_ref()
                .and_then(|c| c.save.mc1())
                .map(|s| s.name.clone())
                .unwrap_or_default();
            if let Some(m) = &mut self.mc1menu {
                m.set_slots(slots);
                m.game_active = active;
                m.player_name = name;
            }
        }
        let Some(menu) = &mut self.mc1menu else {
            return;
        };
        menu.tick(dt);
        let action = menu.take_action();
        let has_pointer = menu.has_pointer();
        let (rgba, quads) = menu.frame(size, cursor);
        let mut q = vec![ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, 1.0])];
        q.extend(quads);
        // With the retail pointer bank baked, `frame` composed the
        // cursor into the screen itself (like the MC2 temple menu):
        // the OS pointer hides in EVERY window mode. An older bake
        // falls back to the fullscreen software arrow.
        if has_pointer {
            if let Some(w) = &self.window {
                w.set_cursor_visible(false);
            }
        } else {
            self.append_software_cursor(&mut q);
        }
        if let Some(r) = &mut self.renderer {
            // The composed screen IS the atlas — re-uploaded per
            // frame (320×200, the animations live in it).
            r.load_ui_atlas(320, 200, &rgba);
            self.ui_atlas = UiAtlas::MenuMc1;
            r.set_ui_quads(q);
        }
        if let Some(a) = action {
            use frontend_mc1::Mc1Action;
            match a {
                Mc1Action::Continue => {
                    let n = self.campaign.as_ref().map_or(0, |c| c.current);
                    self.campaign_switch(n, event_loop);
                }
                Mc1Action::NewGame => {
                    if let Some(run) = &mut self.campaign {
                        run.current = 0;
                        run.next = None;
                        if let Some(s) = run.save.mc1_mut() {
                            s.level = 0;
                            s.blob24 = [0; 24];
                        }
                    }
                    self.campaign_switch(0, event_loop);
                }
                Mc1Action::SaveTo { slot } => {
                    if let Some(run) = &mut self.campaign {
                        run.slot = Some(slot);
                        if let Some(s) = run.save.mc1_mut() {
                            // `name` IS the player name here (the MC1
                            // record has one field for both), set by
                            // `SetName` and left alone otherwise. It is
                            // never overwritten with the rendered slot
                            // row — that is what accumulated the level
                            // and progress suffix on every save.
                            s.level = run.current as u16;
                        }
                        run.persist();
                        self.ui_atlas = UiAtlas::None; // refresh slots
                    }
                }
                Mc1Action::LoadFrom(slot) => {
                    let id = self.campaign.as_ref().map(|c| c.id);
                    if let Some(id) = id {
                        match CampaignRun::start(id, Some(slot), false) {
                            Ok(run) => {
                                self.campaign = Some(run);
                                self.ui_atlas = UiAtlas::None;
                                // A slot saved mid-level resumes INTO
                                // that level, at the saved position —
                                // otherwise picking it here would
                                // silently restart the level the save
                                // was trying to preserve. A
                                // campaign-only slot just stays on the
                                // menu, which is where it was taken.
                                if let Err(e) = self.resume_slot(slot) {
                                    eprintln!("error: resume slot {}: {e}", slot + 1);
                                }
                            }
                            Err(e) => eprintln!("error: load: {e}"),
                        }
                    }
                }
                Mc1Action::SetName(name) => {
                    if let Some(s) = self.campaign.as_mut().and_then(|c| c.save.mc1_mut()) {
                        if !name.is_empty() {
                            println!("save name: {name}");
                            s.name = name;
                        }
                    }
                    // Re-run the entry refresh so the next rename
                    // pre-fills the NEW name.
                    self.ui_atlas = UiAtlas::None;
                }
                Mc1Action::Quit => event_loop.exit(),
            }
        }
    }

    /// One MC2 main-menu frame: swap the temple atlas in, tick the
    /// idle dressing, drain clicks/actions.
    fn menu_screen_frame_mc2(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        if self.mainmenu.is_none() {
            match frontend::MainMenu::load(&get_baked_directory().join("assets/mc2-ui")) {
                Ok(m) => {
                    self.mainmenu = Some(m);
                    self.free_menu_pointer();
                    self.frontend_music();
                }
                Err(e) => {
                    // Menu asset unavailable: fall through to the map
                    // hub.
                    eprintln!("note: main menu unavailable: {e} — opening the world map");
                    self.open_map_screen(event_loop);
                    return;
                }
            }
        }
        let size = self.view_size();
        let cursor = self.cursor;
        let Some(menu) = &mut self.mainmenu else {
            return;
        };
        menu.tick(dt);
        let sounds = menu.take_sounds();
        let action = menu.take_action();
        let quads = {
            let mut q = vec![ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, 1.0])];
            q.extend(menu.quads(size, cursor));
            q
        };
        if !sounds.is_empty()
            && self.cfg.audio.sound
            && let Some(a) = &mut self.audio
        {
            let listener = mgc_audio::Listener {
                pos: (0, 0, 0),
                yaw: 0,
            };
            for id in sounds {
                a.event(id, mgc_audio::Source::Player, &listener);
            }
        }
        if let Some(r) = &mut self.renderer {
            if self.ui_atlas != UiAtlas::MenuMc2 {
                let (w, h, px) = self.mainmenu.as_ref().unwrap().atlas();
                r.load_ui_atlas(w, h, px);
                self.ui_atlas = UiAtlas::MenuMc2;
            }
            r.set_ui_quads(quads);
        }
        if let Some(a) = action {
            use frontend::MenuAction;
            match a {
                MenuAction::EnterMap => {
                    self.open_map_screen(event_loop);
                }
                MenuAction::SaveTo { slot } => {
                    self.apply_map_action(worldmap::MapAction::SaveTo { slot });
                }
                MenuAction::LoadFrom(slot) => {
                    self.apply_map_action(worldmap::MapAction::LoadFrom(slot));
                    // Retail: a successful menu load lands on the map.
                    self.open_map_screen(event_loop);
                }
                MenuAction::SetName(name) => {
                    if let Some(s) = self.campaign.as_mut().and_then(|c| c.save.mc2_mut()) {
                        println!("player name: {name}");
                        s.player_name = name;
                    }
                }
                MenuAction::Quit => event_loop.exit(),
            }
        }
    }

    /// The confirmed in-level exit: abandon to the campaign hub
    /// (retail MC2 → the world map, nothing recorded; MC1/HW → the
    /// main menu), or leave the app in single-level mode.
    fn confirm_exit(&mut self, event_loop: &ActiveEventLoop) {
        self.exit_confirm = false;
        if self
            .campaign
            .as_ref()
            .is_some_and(|c| c.id == campaign::CampaignId::Mc2)
        {
            println!("returning to the world map");
            self.open_map_screen(event_loop);
        } else if self.campaign.is_some() {
            println!("returning to the main menu");
            self.enter_main_menu();
        } else {
            event_loop.exit();
        }
    }

    fn set_grab(&mut self, grab: bool) {
        let Some(window) = &self.window else { return };
        // While PAUSED the cursor always stays free: the mini-menu
        // needs it, and pause keeps the rest of the input path live,
        // so several ordinary paths try to re-grab underneath it —
        // closing the big map is the obvious one. Re-grabbing there
        // left the mini-menu clickable but the pointer invisible.
        // Unpause re-grabs by clearing `paused` BEFORE calling here.
        if grab && self.paused {
            return;
        }
        if grab {
            let ok = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
            window.set_cursor_visible(!ok);
            self.grabbed = ok;
            // A boot-grab wish is fulfilled by any successful grab;
            // a failed attempt leaves it armed for the frame retry.
            if ok {
                self.boot_grab = false;
            }
        } else {
            // A deliberate FREE cancels any pending boot grab — the
            // screen has moved on to wanting the cursor.
            self.boot_grab = false;
            // FREE the pointer for UI use — but in FULLSCREEN "free"
            // still means CONFINED to the window (cursor visible):
            // there is no desktop to reach, and an unconfined cursor
            // is exactly where the platform weirdness lives (an
            // invisible cursor drifting onto a second monitor, a
            // click landing outside the borderless window and
            // unfocusing it, Windows dropping confinement across the
            // focus bounce). Windowed mode keeps the true release.
            // (Never while unfocused — alt-tab must keep a normally
            // free cursor; `Focused(true)` re-asserts on return.)
            let confined = window.fullscreen().is_some()
                && window.has_focus()
                && window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
            if !confined {
                window.set_cursor_grab(CursorGrabMode::None).ok();
            }
            // Fullscreen also suppresses the OS pointer entirely
            // (player ruling): the surfaces draw an in-game pointer
            // instead — the frontends their retail cursor sprites,
            // everything else `ui::cursor_quads` via
            // `append_software_cursor`.
            window.set_cursor_visible(!confined);
            self.grabbed = false;
        }
    }

    /// Append the software pointer where the fullscreen rule hides
    /// the OS one and no retail cursor sprite is drawn (the in-level
    /// UI, the MC1 menu; the MC2 temple menu and the world map draw
    /// their own). Windowed mode keeps the OS cursor — no-op. Also
    /// re-hides the OS cursor as a belt: whichever path freed the
    /// pointer, the two must never show together.
    fn append_software_cursor(&self, quads: &mut Vec<mgc_render::UiQuad>) {
        let Some(w) = &self.window else { return };
        if w.fullscreen().is_none() || self.grabbed {
            return;
        }
        if matches!(self.screen, Screen::Map | Screen::Movie)
            || (self.screen == Screen::Menu && self.is_mc2())
        {
            return;
        }
        w.set_cursor_visible(false);
        let size = w.inner_size();
        let s = ui::HudFrame::new(size.width as f32, size.height as f32)
            .s
            .max(1.0);
        // In-level, BOTH games draw their RETAIL pointer from the
        // POINTERS bank at the level atlas tail (bound on this
        // path): MC1 the golden arrow+mana-ball, MC2 the grey one —
        // day/night/cave variants per map type. The quad arrow
        // stands in only on an older bake.
        if self.screen == Screen::Level {
            let entry = if self.is_mc2() {
                match self.session.as_deref().map(|sess| sess.level.mc2_env) {
                    Some(entities::Mc2MapEnv::Night) => ui::POINTER_ENTRY_NIGHT,
                    Some(entities::Mc2MapEnv::Cave) => ui::POINTER_ENTRY_CAVE,
                    _ => ui::POINTER_ENTRY_DEFAULT,
                }
            } else {
                ui::POINTER_ENTRY_DEFAULT
            };
            let retail = self
                .session
                .as_deref()
                .and_then(|sess| sess.level.ui.as_ref())
                .and_then(|a| a.pointer_quad(self.cursor.0, self.cursor.1, s, entry));
            if let Some(q) = retail {
                quads.push(q);
                return;
            }
        }
        quads.extend(ui::cursor_quads(self.cursor.0, self.cursor.1, s));
    }

    /// Whether plain in-level FLIGHT is on screen with no surface
    /// wanting a free cursor: not paused (mini-menu/options), no
    /// abandon dialog, no held CTRL selector, and not the book map's
    /// freed-pointer state. This is the state whose pointer is
    /// captured and hidden.
    fn level_wants_grab(&self) -> bool {
        self.screen == Screen::Level
            && self.session.is_some()
            && !self.paused
            && !self.exit_confirm
            && !self.ctrl_held
            && !(self.book_open() && self.selector.map_book)
    }

    /// Whether the OS pointer is PINNED by winit on Windows: its
    /// Windows backend clips BOTH `Locked` and `Confined`-with-
    /// hidden-cursor to a 1×1 rect (a deliberate keep-off-the-taskbar
    /// workaround, window_state.rs `refresh_os_cursor`), so absolute
    /// position freezes — only raw deltas flow. Those combos are
    /// exactly our software-cursor surfaces: the flight grab, the
    /// map screen (confined+hidden in ANY window mode) and every
    /// fullscreen hidden-cursor state. There, `self.cursor` is
    /// advanced from raw deltas and absolute reports are ignored.
    /// X11 confinement keeps true absolute motion (this predicate is
    /// how "works on Linux" was "utterly broken on Windows");
    /// unfocused windows are never clipped, hence the focus term.
    fn windows_pinned_pointer(&self) -> bool {
        cfg!(target_os = "windows")
            && self.window.as_ref().is_some_and(|w| {
                w.has_focus()
                    && (self.grabbed || self.screen == Screen::Map || w.fullscreen().is_some())
            })
    }

    /// A live slider drag in the options menu tracks the pointer
    /// (apply live; persist on release). Fed by `CursorMoved` where
    /// the OS reports absolute motion, and by the raw-delta
    /// integration where Windows has the pointer pinned.
    fn pointer_drag_follow(&mut self) {
        if let Some(st) = &self.menu
            && let Some(i) = st.drag
        {
            let size = self.view_size();
            if let Some(assets) = ui_assets!(self) {
                let changed = menu::pointer_apply(
                    assets,
                    &mut self.cfg,
                    &self.specs,
                    self.menu.as_mut().unwrap(),
                    size.0,
                    size.1,
                    self.cursor,
                    i,
                    false,
                );
                if changed {
                    let path = self.specs[i].cfg_path;
                    self.apply_option(path);
                }
            }
        }
    }

    /// Re-assert the pointer mode the current screen wants — the
    /// fullscreen transition is allowed to drop grabs/confinement on
    /// several platforms, so every toggle re-applies the active state
    /// (and picks up the fullscreen-vs-windowed "free" flavor above).
    fn reassert_pointer(&mut self) {
        if self.grabbed {
            self.set_grab(true);
        } else if self.level_wants_grab()
            && self
                .window
                .as_ref()
                .is_some_and(|w| w.fullscreen().is_some())
        {
            // The fullscreen ruling: in-level flight is ALWAYS
            // captured. Without this arm, a focus bounce (WMs do this
            // around boot and workspace switches) dropped fullscreen
            // flight into the free-CONFINED state — pointer visible
            // over a flying carpet, waiting for a click nothing
            // announced. Windowed keeps the classic alt-tab law:
            // flight recaptures on click.
            self.set_grab(true);
        } else {
            match self.screen {
                Screen::Map => self.confine_map_pointer(),
                Screen::Menu if self.is_mc2() => self.free_menu_pointer(),
                _ => self.set_grab(false),
            }
        }
    }

    /// One frame of the shell — the `WindowEvent::RedrawRequested`
    /// arm: drain wall time into fixed sim ticks, assemble the
    /// HUD/UI quads, place the camera, render, and request the
    /// next redraw.
    fn redraw_requested(&mut self, event_loop: &ActiveEventLoop) {
        let now = std::time::Instant::now();
        // Clamp huge pauses (debugger, suspend) to keep the sim
        // from spiraling through hundreds of catch-up ticks.
        let raw_dt = (now - self.last_frame).as_secs_f32();
        let dt = raw_dt.min(0.25);
        self.last_frame = now;
        // PROTOTYPE fire clock (advances while paused too). WRAPPED:
        // the clock feeds shader sin() through the particle seeds
        // (~96·t radians at the fastest term), and WGSL guarantees
        // sin() accuracy only on [-π, π] — an unbounded clock walks
        // off the driver's range-reduction cliff after ~1h of uptime
        // (the sticky "corrupt fire", repro'd live via MGC_FIRE_T0).
        // The 600 s fold keeps arguments ~6× under the observed
        // cliff; the once-per-10-min phase pop is invisible in
        // chaotic flame. dt clamped so a suspend/debugger stall
        // cannot leap the clock in one frame.
        self.effect_time = (self.effect_time + dt) % 600.0;
        // FPS-overlay accounting: true wall time (the clamp
        // above is sim pacing, not measurement), readout
        // refreshed every half-second. Counts while paused
        // too — the menu is where you toggle effects to
        // watch their cost.
        if self.cfg.render.debug.fps {
            self.fps_frames += 1;
            self.fps_elapsed += raw_dt;
            if self.fps_elapsed >= 0.5 {
                let ms = 1000.0 * self.fps_elapsed / self.fps_frames as f32;
                self.fps_text = format!(
                    "{:.0} fps  {ms:.1} ms",
                    self.fps_frames as f32 / self.fps_elapsed
                );
                self.fps_frames = 0;
                self.fps_elapsed = 0.0;
            }
        } else if !self.fps_text.is_empty() {
            self.fps_text.clear();
            self.fps_frames = 0;
            self.fps_elapsed = 0.0;
        }
        // A still-armed boot grab retries every frame until
        // it STICKS: both the attempt in `resumed` and the
        // one on the first focus can fail while the WM is
        // still placing/animating the fresh window (some hold
        // their own pointer grab through it). Success or any
        // deliberate free clears the flag inside `set_grab`.
        if self.boot_grab {
            // has_focus-gated: an X11 pointer grab can succeed
            // GLOBALLY, and retrying while alt-tabbed away
            // would steal the pointer from another app. While
            // focus has not arrived, briefly keep re-asking
            // for activation (the `resumed` request races the
            // WM's async map).
            if self.window.as_ref().is_some_and(|w| w.has_focus()) {
                self.set_grab(true);
            } else if self.boot_focus_asks > 0 {
                self.boot_focus_asks -= 1;
                if let Some(w) = &self.window {
                    w.focus_window();
                    // Every 15th ask escalates from petition
                    // to the X11 primitive: by then the map
                    // has settled, and a WM that ignores
                    // `_NET_ACTIVE_WINDOW` (compiz FSP) will
                    // ignore it forever.
                    if self.boot_focus_asks % 15 == 0 {
                        x11_force_focus(w);
                    }
                }
            }
        }
        // A frontend screen owns the frame: no session, no
        // sim, no HUD — its own tick + quads, rendered over
        // the void (the renderer holds no level).
        if self.screen != Screen::Level || self.session.is_none() {
            self.frontend_frame(dt, event_loop);
            if let Some(r) = &mut self.renderer {
                let cam = CameraView {
                    x: 0.0,
                    y: 4.0,
                    z: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    roll: 0.0,
                    fov_y: FOV_Y,
                };
                match r.render(&cam) {
                    Ok(()) | Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {}
                    Err(e) => eprintln!("render: {e}"),
                }
            }
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }
        // Game speed (retail F3): retail runs the sim step N
        // times per rendered frame (remc1 :41672 / remc2
        // EF:31800); our fixed-Hz accumulator expresses the
        // same multiplier by scaling wall time. Every tick is
        // bit-identical — only the pacing changes.
        let speed = self.cfg.sim.options.game_speed.multiplier(self.is_mc2());
        self.accumulator += dt * speed;
        if self.paused {
            // Frozen sim clock: drain the accumulator so
            // resuming is clean instead of bursting through
            // missed ticks. (The abandon-confirm dialog does
            // NOT freeze — retail keeps the world simulating
            // under it, EventsFunctions.cpp:31796; the dialog
            // only owns the input. P still pauses if wanted.)
            self.accumulator = 0.0;
        }

        // The toast line decays on WALL time at the authentic
        // 24Hz — retail decrements the message life once per
        // rendered frame, not per game turn, so the speed
        // multiplier never parked a SLOW toast forever or
        // blinked a VERY FAST one. Frozen with the rest of the
        // clock under P-pause.
        if !self.paused {
            self.toast_accumulator += dt;
            let frames = ((self.toast_accumulator / TICK_DT) as u32).min(u16::MAX as u32) as u16;
            if frames > 0 {
                self.toast_accumulator -= f32::from(frames) * TICK_DT;
                if let Some(w) = sess!(self).sim.world.as_mut() {
                    w.age_notification(frames);
                }
                // The narration subtitle dwells on the same
                // wall clock (it overtitles a wall-time
                // voiceover).
                if let Some((_, t)) = &mut self.subtitle {
                    *t = t.saturating_sub(frames);
                    if *t == 0 {
                        self.subtitle = None;
                    }
                }
            }
        }

        // Per-frame tick burst cap: at high multipliers a slow
        // frame must shed sim time instead of spiraling (retail
        // effectively did the same — its N steps per frame
        // stretched wall time when frames slowed).
        let max_ticks = ((2.0 * speed).ceil() as u32).max(4);
        let mut ran = 0u32;
        while self.accumulator >= TICK_DT {
            self.accumulator -= TICK_DT;
            {
                let sess = sess!(self);
                sess.prev_flyer = sess.sim.flyer;
            }
            // Replay playback replaces the live input wholesale; when
            // the take ends, control hands back to the player.
            let input = match self.replay.take() {
                Some(mut d) => {
                    let next = d.next(&mut sess!(self).sim);
                    if d.take_anchored() {
                        // An anchor re-imported the world — stale
                        // (slot, generation) pose pairs must not
                        // survive it (the resume path's law).
                        let sess = sess!(self);
                        sess.prev_flyer = sess.sim.flyer;
                        sess.pose_prev = Vec::new();
                        sess.pose_cur = Vec::new();
                    }
                    match next {
                        Some(i) => {
                            self.replay = Some(d);
                            i
                        }
                        None => {
                            self.mini_toast(format!("replay ended — {}", d.summary()));
                            // Hand control back cleanly: drop the
                            // stale live-input accumulators the
                            // replay never drained.
                            self.mouse = MouseAccum::default();
                            self.stick = VirtualStick::default();
                            self.tick_input()
                        }
                    }
                }
                None => self.tick_input(),
            };
            // `--record`: t/hash describe the PRE-step state; the
            // input is what the step consumes (the phase convention,
            // docs/RECORDING.md).
            let pre = self.recorder.is_some().then(|| {
                let sess = sess!(self);
                (sess.sim.tick, sess.sim.state_hash())
            });
            sess!(self).sim.step(&input);
            if let Some(d) = self.replay.as_mut() {
                d.grade(&sess!(self).sim);
            }
            if let (Some(r), Some((t, hash))) = (self.recorder.as_mut(), pre) {
                if let Err(e) = r.record(t, &input, hash) {
                    eprintln!("record: {e} — recording stopped");
                    self.recorder = None;
                }
            }
            // Smooth-motion snapshot rotation — the entity
            // analogue of prev_flyer above (entities render
            // lerped over the same one-tick window).
            {
                // PROTOTYPE lightning: drain the sim's strike feed
                // EVERY tick (even with the effect off, so the
                // hash-quiet vec never accumulates) and age the
                // ledger one tick.
                let sess = sess!(self);
                let strikes = sess
                    .sim
                    .world
                    .as_mut()
                    .map(|w| w.take_lightning_bolts())
                    .unwrap_or_default();
                sess.bolts.update(strikes, 1.0);
            }
            if self.cfg.render.enhancement.smooth_motion {
                let sess = sess!(self);
                if let Some(w) = &sess.sim.world {
                    sess.pose_prev = std::mem::take(&mut sess.pose_cur);
                    sess.pose_cur = w.live_poses();
                    // PROTOTYPE fire: track blast drivers (one
                    // tick per step; dead ones keep aging so
                    // their smoke choreography finishes).
                    sess.fire_blasts.update(&w.mc1_blasts(), 1.0);
                }
            }
            // The mixer flush is per-tick like the original's
            // (fade ramps are tick-denominated).
            self.audio_tick();
            ran += 1;
            if ran >= max_ticks {
                self.accumulator = 0.0;
                break;
            }
        }
        // Limit-removing telemetry (ROADMAP "MULTI-GAME
        // ARCHITECTURE"): the pool fails open like retail,
        // but every dropped spawn is worth a report — this
        // is how the catalogue of ceiling-hitting levels
        // (032's starved trigger, 039's walls) gets built.
        if let Some(w) = sess!(self).sim.world.as_mut() {
            // Retail quickselect auto-assign (:64858-67): a
            // newly acquired spell takes the FIRST FREE quick
            // key (scan 1→9→0, cap 10, silent when full;
            // already-bound spells never re-assign). Walking
            // the book's canonical order (byte_99B88) also
            // reproduces the level-init pre-seed (:49216-59):
            // at level start every owned spell diffs in at
            // once, in that order. MC1-key schemes only —
            // MC2 controls have no quickselect bank.
            if self.selector.map_book {
                let owned = w.loadout().owned;
                for &s in &SPELL_CANON {
                    let s = s as usize;
                    if owned[s] && !self.prev_owned[s] && !self.quick_binds.contains(&Some(s as u8))
                    {
                        if let Some(slot) = self.quick_binds.iter_mut().find(|b| b.is_none()) {
                            *slot = Some(s as u8);
                        }
                    }
                }
                self.prev_owned = owned;
            }
            let dropped = w.take_pool_exhausted();
            if dropped > 0 {
                self.pool_dropped_total += dropped;
                // The (class,model) census at the spike moment is the
                // forensic: the top occupants name whatever flooded
                // the pool (player report: exhaustion under dual-hand
                // lightning with the live count normally modest).
                let census = w.debug_entity_census(6);
                let line = census
                    .iter()
                    .map(|&(c, m, n)| format!("c{c}m{m}\u{d7}{n}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!(
                    "ERROR: entity pool exhausted — {dropped} allocation(s) \
                     dropped this frame, {} this level (fail-open, as retail); \
                     top occupants: {line}",
                    self.pool_dropped_total
                );
                // Name the top occupant's SPAWNER, not just its
                // species: state / owner / ring-family split.
                if let Some(&(tc, tm, _)) = census.first() {
                    println!("       {}", w.debug_entity_drilldown(tc, tm));
                }
            }
            // The spawn seam's misfit ledger (unknown
            // (class, model) things degraded gracefully) —
            // report new entries once.
            for &(class, model, count) in &w.misfits()[self.misfits_reported..] {
                println!(
                    "WARN: misfit thing (class {class}, model {model}) x{count} — \
                     unknown to the serving spawn column, degraded"
                );
                self.misfits_reported += 1;
            }
        }
        self.sync_world();
        // Castle-less death confirmed → the level restarts
        // (the original's lost + level-over flow).
        if sess!(self)
            .sim
            .world
            .as_mut()
            .is_some_and(|w| w.take_restart())
        {
            self.restart_level();
        }

        let alpha = self.accumulator / TICK_DT;
        // Stale snapshots die with the toggle; the pass below
        // re-sets the renderer's drawables at this frame's
        // lerp fraction (sync_world set the tick-rate ones).
        if !self.cfg.render.enhancement.smooth_motion {
            let sess = sess!(self);
            if !sess.pose_cur.is_empty() {
                sess.pose_prev = Vec::new();
                sess.pose_cur = Vec::new();
            }
        }
        self.apply_smooth_motion(alpha);
        let sess = sess_ref!(self);
        let (a, b) = (&sess.prev_flyer, &sess.sim.flyer);
        // Positions may wrap across the 256-tile seam; take the
        // short way around for interpolation.
        let lerp_wrap = |p: f32, q: f32| {
            let mut d = q - p;
            if d > 128.0 {
                d -= 256.0;
            }
            if d < -128.0 {
                d += 256.0;
            }
            (p + d * alpha).rem_euclid(256.0)
        };
        // The knock camera kick (remc1 :52433-37): the view
        // pitches down ~v_22/8 engine-angle units while a
        // buffet/knock is live (the kraken drag feedback).
        let kick = sess
            .sim
            .world
            .as_ref()
            .map(|w| w.knock_magnitude() as f32 / 8.0 * (std::f32::consts::TAU / 2048.0))
            .unwrap_or(0.0);
        // The faithful camera renders at HALF the aim pitch
        // (remc1 :52434: pitch_8 = u16_329/2) — casts still
        // aim along the full published pitch.
        let aim = a.pitch + (b.pitch - a.pitch) * alpha;
        // The horizon bank. Faithful: the filtered roll stick,
        // full value (remc1 :52432 — the missing turn cue).
        // Enhanced: the proportional bank the sim derives from
        // turn_rate × speed (deliberate deviation) — camera
        // roll only, the HUD stays screen-space level.
        // Shortest arc: during a barrel roll the sim publishes the
        // tumble masked to [0, 2π) (retail's & 0x7FF view write), so
        // the wrap tick would lerp the long way round without this.
        // Ordinary banking has |dr| ≈ 0 and is untouched.
        let roll = {
            let mut dr = b.roll - a.roll;
            if dr > std::f32::consts::PI {
                dr -= std::f32::consts::TAU;
            } else if dr < -std::f32::consts::PI {
                dr += std::f32::consts::TAU;
            }
            a.roll + dr * alpha
        };
        let (view_pitch, view_roll) = match self.cfg.controls.models.thrust {
            config::ThrustModel::Classic => (aim * 0.5, roll),
            config::ThrustModel::Enhanced => (aim, roll),
        };
        // The carpet plane, and the EYE half a tile over it: retail
        // hands its world draw `axis.z + 128` in both games, never the
        // raw carpet z (mgc_sim::EYE_LIFT — remc2 EF:21575, remc1
        // sub_main.cpp:26406). Rendering from the carpet plane put the
        // view a half tile low everywhere, which reads as "docked at
        // the castle you sit lower than retail" wherever the ground is
        // close enough to judge (player report 2026-08-05).
        let carpet_y = a.y + (b.y - a.y) * alpha;
        let cam = CameraView {
            x: lerp_wrap(a.x, b.x),
            y: carpet_y + mgc_sim::EYE_LIFT,
            z: lerp_wrap(a.z, b.z),
            yaw: a.yaw + (b.yaw - a.yaw) * alpha,
            pitch: view_pitch - kick,
            roll: view_roll,
            fov_y: FOV_Y,
        };
        // The overlay fog wall: terrain fully occludes at
        // 0.95·fog_distance (see terrain.wgsl fog_amount);
        // world-anchored overlays cut there (`fog_cut`).
        let fog_wall = 0.95 * self.cfg.render.preference.fog_distance as f32;
        // Spell UI quads (book grid or in-flight HUD).
        if let (Some(assets), Some(w)) = (&sess.level.ui, &sess.sim.world) {
            let size = self
                .window
                .as_ref()
                .map(|win| win.inner_size())
                .map(|s| (s.width as f32, s.height as f32))
                .unwrap_or((1280.0, 960.0));
            let loadout = w.loadout();
            let vitals = w.vitals();
            let is_mc2 = matches!(sess.level.game, mgc_sim::ids::GameId::Mc2);
            let mc2_book = is_mc2.then(|| w.mc2_book_view());
            // The alert-marble flicker approximates retail's
            // per-frame [55]/[41] alternation at tick parity.
            let alert_blink = sess.sim.tick % 2 == 0;
            let (mut quads, hovered) = if self.book_open() {
                if self.selector.map_book {
                    ui::book_quads(
                        assets,
                        &loadout,
                        &self.quick_binds,
                        size.0,
                        size.1,
                        self.cursor,
                    )
                } else {
                    // The MC2-layout map screen has no book
                    // half — the renderer's split layout shows
                    // the stretched live view there; the CTRL
                    // pane below is the selector.
                    (Vec::new(), None)
                }
            } else {
                (
                    ui::hud_quads(
                        assets,
                        &loadout,
                        &vitals,
                        self.hud_transparent(),
                        alert_blink,
                        is_mc2,
                        mc2_book.as_ref(),
                        self.cfg.gameplay.cheat.dev_spells,
                        size.0,
                        size.1,
                    ),
                    None,
                )
            };
            // The CTRL selector pane, over flight or the map
            // screen alike (the original draws the same pane
            // in both states, remc2 EF:21788/EF:21959).
            if self.pane_open() {
                if let Some(pane) = &self.pane {
                    let n = pane.spell_count();
                    let mc2 = is_mc2;
                    let mut owned = [false; 26];
                    let mut castable = [false; 26];
                    let mut castable_tier = [[true; 3]; 26];
                    let mut cost = [0u32; 26];
                    let mut max_level = [0u8; 26];
                    let mut sel = [0u8; 26];
                    let mut xp = [0i32; 26];
                    let mut xpos = [[0i32; 3]; 26];
                    let mut ring = [0u8; 26];
                    let mut bound = [loadout.left, loadout.right];
                    if mc2 {
                        // The native spell book: ownership,
                        // per-spell LEVEL (the
                        // SpellLevels tier ceiling), selected
                        // tiers, real GetSpellManaCost costs
                        // and the quick-slot binds all come
                        // from the sim's class-15 machinery.
                        let bv = Some(w.mc2_book_view());
                        if let Some(bv) = bv {
                            for s in 0..n {
                                owned[s] = bv.owned[s] || self.cfg.gameplay.cheat.dev_spells;
                                // Retail's canSummon grey-out
                                // (EF:22503-08): the selected
                                // tier's castle-pool prereq.
                                // The G instrument bypasses
                                // the afford gate for real,
                                // so it stays lit under dev.
                                let dev = self.cfg.gameplay.cheat.dev_spells;
                                castable[s] =
                                    owned[s] && (bv.castable[s][bv.sel[s].min(2) as usize] || dev);
                                if !dev {
                                    castable_tier[s] = bv.castable[s];
                                }
                                cost[s] = bv.cost[s];
                                // The G instrument keeps all
                                // tiers exercisable; the
                                // earned ceiling is the XP
                                // level.
                                max_level[s] = if self.cfg.gameplay.cheat.dev_spells {
                                    pane.levels - 1
                                } else {
                                    bv.levels[s]
                                };
                                sel[s] = bv.sel[s];
                                xp[s] = bv.xp[s];
                                xpos[s] = bv.xpos[s];
                                ring[s] = bv.ring[s];
                            }
                            bound = [u8::try_from(bv.left).ok(), u8::try_from(bv.right).ok()];
                            // `spell_levels` is NOT mirrored here
                            // any more — `sync_world` owns that,
                            // every frame rather than only while
                            // the pane happens to be drawn.
                            self.pane_bound = bound;
                        }
                    } else {
                        for s in 0..n {
                            owned[s] = loadout.owned[s];
                            castable[s] = loadout.bindable[s];
                            castable_tier[s] = [loadout.bindable[s]; 3];
                            cost[s] = mgc_sim::mc1::spells::SPELLS[s].possess_mana;
                            max_level[s] = pane.levels - 1;
                            sel[s] = self.spell_levels[s];
                            ring[s] = loadout.ring[s];
                        }
                    }
                    let view = ui::SelectorView {
                        owned: &owned[..n],
                        castable: &castable[..n],
                        castable_tier: &castable_tier[..n],
                        selected_level: &sel[..n],
                        max_level: &max_level[..n],
                        bound,
                        ring: &ring[..n],
                        mana: loadout.mana,
                        cost: &cost[..n],
                        xp: &xp[..n],
                        xpos: &xpos[..n],
                    };
                    let (pq, hover) = ui::selector_quads(
                        assets,
                        pane,
                        &view,
                        size.0,
                        size.1,
                        self.cursor,
                        self.selector_drag.map(|(s, _)| s),
                    );
                    quads.extend(pq);
                    self.selector_hover = hover;
                }
            }
            // The map-screen wizard scoreboard: name + census
            // mana total + the kill matrix, one screen shared
            // by both games (ui::roster_quads). Retail
            // triggers: MC1 = the cursor over the blank strip
            // below the map pane (`mouse.y >= 382`, :26838-39);
            // MC2 = held ALT (PI:951 → MenuState 7 →
            // DrawSorcererScores_2D1D0). DELIBERATE (player
            // ruling): BOTH triggers work in BOTH games —
            // neither input means anything else on the map
            // screen. Doubles as the mana-conservation
            // instrument (the census total is base + Σ owned
            // entity mana, the leak-visible quantity).
            if self.book_open() {
                let strip_top = ui::HudFrame::new(size.0, size.1).by(if self.selector.map_book {
                    mgc_render::BOOK_MAP_H
                } else {
                    mgc_render::MC2_MAP_VIEW_H
                });
                // The hover trigger needs a LIVE pointer — on
                // the bookless map the cursor stays grabbed
                // (its position freezes), so a stale low
                // position must not pin the roster open; ALT
                // is that map's trigger.
                if self.alt_held || (!self.grabbed && self.cursor.1 >= strip_top) {
                    let colors = entities::roster_team_colors(
                        sess.level.game,
                        sess.level.mc2_env,
                        &sess.level.palette_rgba,
                    );
                    let mut rows: [Option<ui::RosterEntry>; 8] = Default::default();
                    // Slot 0 = the human. Retail's in-play flag
                    // (+6) drops at the death event, so the row
                    // exists only while Alive. Name: the
                    // campaign's entered name (retail overrides
                    // the slot-0 table name with the player
                    // string), else the table default.
                    if vitals.state == mgc_sim::engine::world::LifeState::Alive {
                        let name = self
                            .campaign
                            .as_ref()
                            .and_then(|c| {
                                c.save
                                    .mc1()
                                    .map(|s| s.name.clone())
                                    .or_else(|| c.save.mc2().map(|s| s.player_name.clone()))
                                    .filter(|n| !n.trim().is_empty())
                            })
                            .unwrap_or_else(|| {
                                if is_mc2 {
                                    mgc_sim::mc2::rivals::MC2_RIVAL_NAMES[0].into()
                                } else {
                                    mgc_sim::mc1::rivals::RIVAL_NAMES[0].into()
                                }
                            });
                        rows[0] = Some(ui::RosterEntry {
                            name,
                            mana: loadout.mana_max,
                            kills: w.player_kill_row(),
                            box_c: colors[0].0,
                            text_c: colors[0].1,
                        });
                    }
                    for r in w.rival_views() {
                        let slot = r.slot as usize;
                        if r.alive && (1..8).contains(&slot) {
                            rows[slot] = Some(ui::RosterEntry {
                                name: r.name.to_string(),
                                mana: r.mana_max,
                                kills: r.kills,
                                box_c: colors[slot].0,
                                text_c: colors[slot].1,
                            });
                        }
                    }
                    quads.extend(ui::roster_quads(assets, &rows, is_mc2, size.0, size.1));
                }
            }
            if !self.book_open() {
                // The paralyze WEB overlay (remc2 EF:21668-
                // 710): the HWEB bank tiled over the view
                // while the web counter is live — spider
                // webs + the (9,21) spit. Hard on/off, no
                // fade, exactly retail.
                if sess.sim.carpet_mc2.mobilize > 0 && assets.has_web() {
                    quads.extend(assets.web_quads(size.0, size.1));
                }
                // The stagger GREEN tint (`SetPalette
                // Modification_5C830` subMod 3, EF:31935-
                // 32002: R and B darkened by 56*count>>8,
                // count = 171*ms/3+85, green untouched — the
                // manticore-spit poison cast, distinct from
                // the subMod-2 red damage flash). An alpha-
                // blended green quad at the retail
                // subtraction magnitude (≈12/17/22%) is the
                // RGBA approximation of the palette edit.
                let ms = sess.sim.carpet_mc2.move_speed;
                if ms > 0 {
                    let count = (171.0 * ms as f32 / 3.0 + 85.0).min(256.0);
                    let a = count * 56.0 / 65536.0;
                    quads.push(ui::solid([0.0, 0.0, size.0, size.1], [0.05, 0.42, 0.08, a]));
                }
                quads.extend(ui::vitals_quads(
                    &vitals,
                    size.0,
                    size.1,
                    (sess.sim.tick / 8) % 2 == 0,
                    self.cfg.render.debug.grace_meter,
                ));
            }
            if self.paused {
                // Both views: the book screen is exactly where
                // paused inspection happens.
                quads.extend(ui::pause_quads(size.0, size.1));
            }
            // The pause mini-menu. It carries no "PAUSED" text
            // of its own — the retail indicator above is the
            // pause state, this is the menu.
            //
            // Hidden while the options layer is up: the two
            // panels on screen at once read as clutter, and
            // the options menu is modal anyway. Esc closes
            // that layer and this comes straight back.
            if let (Some(mini), None) = (&self.mini, &self.menu) {
                quads.extend(minimenu::draw(assets, mini, size.0, size.1, self.cursor));
            }
            // The options menu (over everything but the quit
            // fade).
            if let Some(st) = &self.menu {
                quads.extend(menu::draw(
                    assets,
                    &self.cfg,
                    &self.specs,
                    st,
                    size.0,
                    size.1,
                    self.cursor,
                ));
            }
            // The exit-confirm modal (mutually exclusive
            // with the options menu — P is swallowed while
            // it is up; under the quit fade).
            if self.exit_confirm {
                quads.extend(ui::exit_confirm_quads(
                    assets,
                    EXIT_CONFIRM_TEXT,
                    size.0,
                    size.1,
                    self.cursor,
                ));
            }
            // expose-jar-spells (debug): float each pickable
            // jar's spell icon over it in the main view (the
            // map stamps are the other half). No fancy UI —
            // the raw icon on a dark slab, health-bar style.
            if self.cfg.render.enhancement.expose_jar_spells && !self.book_open() {
                if let Some(u) = &sess.level.ui {
                    for &(x, alt, z, spell) in &self.jar_markers {
                        // The fog-wall cut: overlays must not
                        // reveal jars the fog hides. Torus-
                        // wrapped distance vs the fog's
                        // full-occlusion point (0.95·D;
                        // 0 = fog off).
                        if fog_cut(&cam, x, alt, z, fog_wall) {
                            continue;
                        }
                        let Some(id) = ui::spell_icon_sprite(sess.level.game, spell) else {
                            continue;
                        };
                        let Some(st) = u.map_stamp(id) else { continue };
                        let Some((sx, sy)) =
                            mgc_render::world_to_screen(&cam, size.0, size.1, x, alt + 0.6, z)
                        else {
                            continue;
                        };
                        let s = ui::HudFrame::new(size.0, size.1).s.max(1.0);
                        let ih = 12.0 * s;
                        let iw = ih * st.w as f32 / st.h as f32;
                        // A dark slab behind the luminous icon
                        // ramps, for readability over bright sky/
                        // terrain.
                        quads.push(mgc_render::UiQuad {
                            rect: [sx - iw * 0.5 - s, sy - ih - s, iw + 2.0 * s, ih + 2.0 * s],
                            uv: [0.0; 4],
                            tint: [0.0, 0.0, 0.0, 0.45],
                        });
                        quads.push(mgc_render::UiQuad {
                            rect: [sx - iw * 0.5, sy - ih, iw, ih],
                            uv: st.uv,
                            tint: [1.0, 1.0, 1.0, 1.0],
                        });
                    }
                }
            }
            // The rival wizard tags: retail MC2's boxed name +
            // health bar in the rival's team color, floated over
            // every VISIBLE rival wizard sprite, always — retail's
            // "Player Names" toggle ships ON and nothing else gates
            // it: not damage, not lock, not distance
            // (DrawSorcererNameAndHealthBar_2CB30 via the sprite-pass
            // hook, remc2 GameRenderHD.cpp:5010-17 — class 3, model
            // 0/1 only, so buildings/creatures never tag). "Visible"
            // = the sprite draws: an invisible rival's sprite is
            // skipped, and beyond the fog wall nothing draws, hence
            // the fog cut. MC1 shows these only under the
            // rival_tags=on opt-in (retail MC1 has no such tag).
            if self.cfg.render.preference.rival_tags.resolve(is_mc2) && !self.book_open() {
                if let Some(u) = &sess.level.ui {
                    if u.has_font() && !self.rival_tags_cur.is_empty() {
                        let colors = entities::roster_team_colors(
                            sess.level.game,
                            sess.level.mc2_env,
                            &sess.level.palette_rgba,
                        );
                        let chrome = entities::rival_tag_chrome(
                            sess.level.game,
                            sess.level.mc2_env,
                            &sess.level.palette_rgba,
                        );
                        let s = ui::HudFrame::new(size.0, size.1).s;
                        // The anchor rides the wizard sprite: its top
                        // approximated as this much above the entity
                        // datum (tiles), then retail's 20px lift
                        // (GameRenderHD.cpp:2841).
                        const WIZ_TOP: f32 = 0.6;
                        for r in &self.rival_tags_cur {
                            if !r.alive || r.invisible {
                                continue;
                            }
                            // Sub-tick lerp against the previous
                            // snapshot (slot-keyed, torus-wrapped;
                            // respawn-scale jumps snap, the smooth-
                            // motion law).
                            let (mut x, mut alt, mut z) = (r.x, r.alt, r.z);
                            if let Some(p) = self.rival_tags_prev.iter().find(|p| p.slot == r.slot)
                            {
                                let wrap = |d: f32| {
                                    if d > 128.0 {
                                        d - 256.0
                                    } else if d < -128.0 {
                                        d + 256.0
                                    } else {
                                        d
                                    }
                                };
                                let (dx, dz) = (wrap(x - p.x), wrap(z - p.z));
                                if dx * dx + dz * dz < 4.0 {
                                    x = (p.x + dx * alpha).rem_euclid(256.0);
                                    alt = p.alt + (alt - p.alt) * alpha;
                                    z = (p.z + dz * alpha).rem_euclid(256.0);
                                }
                            }
                            if fog_cut(&cam, x, alt, z, fog_wall) {
                                continue;
                            }
                            let Some((sx, sy)) = mgc_render::world_to_screen(
                                &cam,
                                size.0,
                                size.1,
                                x,
                                alt + WIZ_TOP,
                                z,
                            ) else {
                                continue;
                            };
                            // Retail bails when the anchor leaves the
                            // viewport (GameRenderHD.cpp:2842-44).
                            if sx < 0.0 || sx >= size.0 || sy < 20.0 * s || sy >= size.1 {
                                continue;
                            }
                            ui::rival_tag_quads(
                                &mut quads,
                                u,
                                r.name,
                                r.life_frac,
                                colors[(r.slot as usize).min(7)].0,
                                &chrome,
                                sx,
                                sy - 20.0 * s,
                                s,
                                size.0,
                            );
                        }
                    }
                }
            }
            // The aim crosshair (`render.preference.crosshair`,
            // C toggles — the gameplay aim cursor, and under
            // enhanced thrust the chase-steering target) and
            // the autoaim lock markers (+/x on the target each
            // hand's equipped spell would acquire this instant;
            // `render.debug.autoaim_hints`, World::aim_preview
            // — the pure scan twin). Split options 2026-07-23.
            let want_cross = self.cfg.render.preference.crosshair;
            let want_hints = self.cfg.render.debug.autoaim_hints;
            if (want_cross || want_hints)
                && !self.book_open()
                && vitals.state == mgc_sim::engine::world::LifeState::Alive
            {
                let f = &sess.sim.flyer;
                // The aim heading. Enhanced chase steering:
                // the crosshair sits at the DESIRED heading
                // (yaw + lead) and the autoaim preview scans
                // along it — matching the cast pose, which
                // launches along the crosshair while the hull
                // is still coming around. The desired heading
                // is predicted at FRAME rate: mouse motion the
                // sim has not consumed yet (`self.mouse`, the
                // per-tick accumulator, already sensitivity-
                // scaled) is added on top of the tick-rate
                // lead, re-clamped against the interpolated
                // camera — otherwise the pointer steps at
                // 24 Hz while the camera glides (choppy,
                // player report 2026-07-23).
                let (neutral_yaw, pose_yaw) = match self.cfg.controls.models.thrust {
                    config::ThrustModel::Classic => (cam.yaw, f.yaw),
                    config::ThrustModel::Enhanced => {
                        let lead = (sess.sim.aim_yaw() + self.mouse.yaw - cam.yaw)
                            .clamp(-mgc_sim::LEAD_MAX, mgc_sim::LEAD_MAX);
                        let a = cam.yaw + lead;
                        (a, a)
                    }
                };
                let (sy, cyaw) = neutral_yaw.sin_cos();
                let (sp, cp) = aim.sin_cos();
                // The acquire range: 5120 units = 20 tiles.
                const AIM_D: f32 = 20.0;
                let neutral = if want_cross {
                    mgc_render::world_to_screen(
                        &cam,
                        size.0,
                        size.1,
                        cam.x + sy * cp * AIM_D,
                        cam.y + sp * AIM_D,
                        cam.z - cyaw * cp * AIM_D,
                    )
                } else {
                    None
                };
                let locks = if want_hints {
                    let pose = mgc_sim::engine::world::PlayerPose::from_tiles(
                        f.x, f.y, f.z, pose_yaw, f.pitch, 0.0,
                    );
                    w.aim_preview(pose).map(|l| {
                        l.and_then(|l| {
                            // Lock markers honor the fog wall
                            // too (relevant when fog_distance
                            // < the 20-tile acquire range).
                            if fog_cut(&cam, l.x, l.alt, l.z, fog_wall) {
                                return None;
                            }
                            mgc_render::world_to_screen(&cam, size.0, size.1, l.x, l.alt, l.z)
                        })
                    })
                } else {
                    [None, None]
                };
                let blink = 0.5 + 0.5 * (((sess.sim.tick % 4096) as f32 + alpha) * 0.4).sin();
                ui::crosshair_quads(&mut quads, size.0, size.1, neutral, locks, blink);
            }
            // The top-of-screen notification line (retail
            // `DrawTextPauseEndOfLevel_2CE30`, EF:21787): the
            // small FONT1 toast, LEFT-aligned, anchored just below
            // the wizard info-boxes and right of the radar (the
            // HUD-derived anchor — retail's 320-native literal
            // doesn't map onto our 640-native HSPR panels). Over
            // the live view only (not the book/map screen). The
            // anchor is in 640-native HUD coords (× w/640); FONT1
            // draws at gameUiScale, so its glyphs scale by w/320.
            // The white masks are tinted the ink colour (DrawText's
            // `color`, red for plain toasts).
            if !self.book_open() && assets.has_font() {
                if let Some((msg, color)) = w.notification() {
                    let (ax, ay) = assets.hud_notification_anchor();
                    // Uniform HUD scale (`ui::HudFrame`): the
                    // toast rides under the LEFT-anchored panel
                    // group, so its anchor is native×s. The font
                    // runs at 2× because FONT1 is 320-native.
                    let hud_s = ui::HudFrame::new(size.0, size.1).s;
                    let font_s = 2.0 * hud_s;
                    let tint = [
                        color[0] as f32 / 255.0,
                        color[1] as f32 / 255.0,
                        color[2] as f32 / 255.0,
                        1.0,
                    ];
                    quads.extend(assets.text_quads(msg, ax * hud_s, ay * hud_s, tint, font_s));
                }
                // The MC1/HW WIN message (:26480-26505):
                // while the win flag holds, the two-line
                // black-ink message persists at the pane top
                // — ETEXT.DAT entries 60/61. Retail
                // colour-cycles the ink unless zoomed out —
                // the static black remap slot [1] is the
                // baseline.
                if !is_mc2 && w.completed() && !w.player_dead() {
                    let (ax, ay) = assets.hud_notification_anchor();
                    // Uniform HUD scale (`ui::HudFrame`): the
                    // toast rides under the LEFT-anchored panel
                    // group, so its anchor is native×s. The font
                    // runs at 2× because FONT1 is 320-native.
                    let hud_s = ui::HudFrame::new(size.0, size.1).s;
                    let font_s = 2.0 * hud_s;
                    let black = [0.0, 0.0, 0.0, 1.0];
                    // The two sentences are ETEXT 60/61, read
                    // from the bundle's baked bank (literal
                    // fallback when the bank is absent). One
                    // string — the font's own line height
                    // spaces the two lines (a manual offset
                    // overlaps them). A live toast owns the
                    // anchor row; the win block steps one line
                    // below it.
                    let line = |idx: usize, fallback: &str| -> String {
                        match sess.level.etext.get(idx) {
                            Some(s) if !s.is_empty() => s.clone(),
                            _ => fallback.to_string(),
                        }
                    };
                    let msg = format!(
                        "{}{}\n{}",
                        if w.notification().is_some() { "\n" } else { "" },
                        line(60, "World restored."),
                        line(61, "Press the space bar to continue."),
                    );
                    quads.extend(assets.text_quads(&msg, ax * hud_s, ay * hud_s, black, font_s));
                }
                // The narration subtitle (MC2 objective
                // voiceover text): word-wrapped, centered,
                // one line-height below the toast row so the
                // two never collide. White ink — the
                // conventional subtitle color (the retail
                // textbox look is not reproduced; P-class
                // presentation).
                if let Some((text, _)) = &self.subtitle {
                    let (_, ay) = assets.hud_notification_anchor();
                    // Uniform HUD scale (`ui::HudFrame`): the
                    // toast rides under the LEFT-anchored panel
                    // group, so its anchor is native×s. The font
                    // runs at 2× because FONT1 is 320-native.
                    let hud_s = ui::HudFrame::new(size.0, size.1).s;
                    let font_s = 2.0 * hud_s;
                    let lh = assets.font_line_height();
                    let white = [1.0, 1.0, 1.0, 1.0];
                    let max_w = size.0 * 0.8 / font_s;
                    let mut y = ay * hud_s + 1.5 * lh * font_s;
                    for line in wrap_font_text(assets, text, max_w) {
                        let w_px = assets.text_width(&line) * font_s;
                        let x = (size.0 - w_px) / 2.0;
                        quads.extend(assets.text_quads(&line, x, y, white, font_s));
                        y += lh * font_s;
                    }
                }
            }
            // The end-of-game fadeout: the MC2 ending's
            // sim-side fade (endGameSeq phase 11) under the
            // app's own post-victory fade; at full black the
            // game ends (quit, no stats/menu — deliberate).
            if w.won() && !self.won_handled {
                // The victory breadcrumb — and the campaign-
                // stitching hook consuming the same signal:
                // record the completion, pick the next step,
                // persist the slot. Single-level mode still
                // just fades out. Latched — the fade being
                // consumed (the map screen) must not refire
                // it.
                self.won_handled = true;
                println!("{} completed", sess.level.label);
                if let Some(run) = &mut self.campaign {
                    campaign_complete(run, sess.level.level_number, w);
                }
                self.quit_fade = Some(0.0);
            }
            let fade = w.end_fade().max(self.quit_fade.unwrap_or(0.0));
            if fade > 0.0 {
                quads.push(ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, fade]));
            }
            // The FPS overlay (render · debug): bottom-right
            // corner — clear of the top HUD strip, the
            // bottom-center grace meter and the left-anchored
            // toast rows; above the fade (a debug instrument
            // stays readable). White FONT1 ink.
            if !self.fps_text.is_empty() && assets.has_font() {
                let font_s = 2.0 * ui::HudFrame::new(size.0, size.1).s;
                let pad = 4.0 * font_s;
                let w_px = assets.text_width(&self.fps_text) * font_s;
                let y = size.1 - (assets.font_line_height() + 4.0) * font_s;
                quads.extend(assets.text_quads(
                    &self.fps_text,
                    size.0 - w_px - pad,
                    y,
                    [1.0, 1.0, 1.0, 1.0],
                    font_s,
                ));
            }
            // The coordinate overlay (render · debug, K):
            // bottom-LEFT, across the room from the fps
            // readout. Engine units — the language the
            // altitude bands speak (floor 128/256, band
            // 1024/3072): x/y = the horizontal position on
            // the sim's wrapping 8.8 axes, z = altitude,
            // (+E) = elevation over the terrain underneath.
            // Reads the interpolated camera pose, so it glides
            // with the picture — but reports the CARPET plane,
            // backing out the eye lift the camera rides
            // (`mgc_sim::EYE_LIFT`): the floor/band numbers this
            // readout speaks in are carpet-relative.
            if self.cfg.render.debug.coords && assets.has_font() {
                let g = sess.sim.ground_height(cam.x, cam.z);
                let xe = (cam.x.rem_euclid(256.0) * 256.0) as u16;
                let ye = (cam.z.rem_euclid(256.0) * 256.0) as u16;
                let ze = ((cam.y - mgc_sim::EYE_LIFT) * 256.0).round() as i32;
                let elev = ze - (g * 256.0).round() as i32;
                let text = format!("x {xe}, y {ye}, z {ze} ({elev:+})");
                let font_s = 2.0 * ui::HudFrame::new(size.0, size.1).s;
                let pad = 4.0 * font_s;
                let y = size.1 - (assets.font_line_height() + 4.0) * font_s;
                quads.extend(assets.text_quads(&text, pad, y, [1.0, 1.0, 1.0, 1.0], font_s));
            }
            // The entity-pool overlay (render · debug): the second
            // fixed bottom-left line, one above the coordinate
            // readout — the lines never re-stack when one is off.
            if self.cfg.render.debug.entities
                && assets.has_font()
                && let Some(w) = &sess.sim.world
            {
                let (used, cap) = w.debug_entity_pool();
                let text = format!("ents {used}/{cap}");
                let font_s = 2.0 * ui::HudFrame::new(size.0, size.1).s;
                let pad = 4.0 * font_s;
                let y = size.1 - 2.0 * (assets.font_line_height() + 4.0) * font_s;
                quads.extend(assets.text_quads(&text, pad, y, [1.0, 1.0, 1.0, 1.0], font_s));
            }
            // The replay counter (④, docs/RECORDING.md "Consumers"):
            // the third fixed bottom-left line — bit-exact/diverged
            // since t=N, always on while a take drives the session.
            if let Some(d) = &self.replay
                && assets.has_font()
            {
                let font_s = 2.0 * ui::HudFrame::new(size.0, size.1).s;
                let pad = 4.0 * font_s;
                let y = size.1 - 3.0 * (assets.font_line_height() + 4.0) * font_s;
                let ink = if d.hud.contains("diverged") {
                    [1.0, 0.6, 0.4, 1.0]
                } else {
                    [0.6, 1.0, 0.6, 1.0]
                };
                quads.extend(assets.text_quads(&d.hud, pad, y, ink, font_s));
            }
            self.hovered = hovered;
            self.append_software_cursor(&mut quads);
            if let Some(r) = &mut self.renderer {
                r.set_ui_quads(quads);
            }
        }
        if let Some(f) = &mut self.quit_fade {
            *f += 1.0 / 48.0;
            if *f >= 1.25 {
                // A beat of full black; then the campaign
                // routes onward, or the game leaves.
                match self.campaign.as_mut().and_then(|c| c.next.take()) {
                    Some(campaign::NextStep::Level(n)) => {
                        let mc1 = self
                            .campaign
                            .as_ref()
                            .is_some_and(|c| c.id != campaign::CampaignId::Mc2);
                        if mc1 {
                            // The retail transition beat: a
                            // win plays the congratulation
                            // movie and returns to the MAIN
                            // MENU (the score screen is still
                            // deferred); Continue launches the
                            // next level.
                            if let Some(run) = &mut self.campaign {
                                run.current = n;
                            }
                            self.quit_fade = None;
                            let win = mc1_win_movie();
                            self.play_movies(&[win], AfterMovie::Menu, event_loop);
                        } else {
                            // MC2's direct level chain (the
                            // demon-mouth secret dive).
                            self.campaign_switch(n, event_loop);
                        }
                    }
                    Some(campaign::NextStep::MapScreen) => {
                        self.quit_fade = None;
                        // MC2 slots a cutscene in front of the
                        // map after certain levels.
                        let done = self.campaign.as_ref().map_or(0, |c| c.current);
                        match self.mc2_cutscene(done) {
                            Some(cue) => self.play_movies(&[cue], AfterMovie::Map, event_loop),
                            None => self.open_map_screen(event_loop),
                        }
                    }
                    Some(campaign::NextStep::Outro) => {
                        // The campaign's ending movie, then
                        // out. MC1/HW have a dedicated
                        // OUTRO.DAT; MC2's ending is the last
                        // of its six cutscenes.
                        println!("campaign complete!");
                        self.quit_fade = None;
                        // Both endings are unskippable in
                        // retail (`PlayInfoFmv(0, ..)`).
                        let outro = if self.is_mc2() { "cut6" } else { "outro" };
                        self.play_movies(
                            &[movie::Cue::unskippable(outro)],
                            AfterMovie::Quit,
                            event_loop,
                        );
                    }
                    None => event_loop.exit(),
                }
            }
        }
        // The fade routing above may have torn the session
        // down (won → menu/map) — the tick clock must come
        // from whatever session remains, if any.
        let anim_tick = self.session.as_deref().map(|s| s.sim.tick % 4096);
        if let Some(r) = &mut self.renderer {
            // Animation clock: sim ticks are the original's game
            // turns; wrapped so f32 stays exact (see set_anim_turn).
            if let Some(t) = anim_tick {
                r.set_anim_turn(t as f32 + alpha);
            }
            match r.render(&cam) {
                Ok(()) | Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {}
                Err(e) => eprintln!("render: {e}"),
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // Default viewport = 1280×960 PHYSICAL px: exactly 2× the
        // native 640×480, so every UI/HUD element lands on an integer
        // pixel grid (no fractional-scale aliasing) and the aspect is
        // retail 4:3. Physical (not logical) so fractional DPI scales
        // (125% etc.) can't reintroduce a fractional multiple. The
        // window stays resizable, and `render.preference.fullscreen`
        // (Alt+Enter) swaps it for a borderless cover of the current
        // monitor — the HUD layout law handles whatever aspect that
        // turns out to be.
        let title = match self.session.as_deref() {
            Some(sess) => format!("Magic Carpet — {}", sess.level.label),
            None => match self.campaign.as_ref() {
                Some(run) => format!("Magic Carpet — {} campaign", run.id.tag()),
                None => "Magic Carpet".to_string(),
            },
        };
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::PhysicalSize::new(1280u32, 960u32))
            // Borderless from the first frame when the config says so,
            // so a fullscreen launch never flashes a 4:3 window first.
            .with_fullscreen(
                self.cfg
                    .render
                    .preference
                    .fullscreen
                    .then_some(winit::window::Fullscreen::Borderless(None)),
            );
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("error: cannot create window: {e}");
                event_loop.exit();
                return;
            }
        };
        // MSAA is baked into every pipeline, so it is read ONCE here
        // and a change needs a restart (the option says so).
        match Renderer::for_window(
            window.clone(),
            self.cfg.render.preference.anti_aliasing.samples(),
        ) {
            Ok(mut renderer) => {
                // Level assets upload only when a session booted with
                // the app (single-level mode); a campaign boots into
                // the frontend, which brings its own atlas — the
                // first launch's `install_level` uploads the rest.
                if let Some(sess) = self.session.as_deref() {
                    let overlay = map_overlay(&sess.level, &self.cfg);
                    renderer.load_level(&sess.level.view, &overlay);
                    if let Some((index, atlas)) = &sess.level.sprites {
                        renderer.load_sprites(index.clone(), atlas);
                    }
                    if let Some(assets) = &sess.level.ui {
                        renderer.load_ui_atlas(assets.atlas_w, assets.atlas_h, &assets.atlas_rgba);
                        self.ui_atlas = UiAtlas::Level;
                    }
                    renderer.set_billboards(sess.level.billboards.clone());
                    if let Some(sky) = mc2_sky_srgb(&sess.level) {
                        renderer.set_sky_color(sky);
                    }
                    if self.cfg.render.preference.sky
                        && let Some(bitmap) = &sess.level.sky
                    {
                        renderer.load_sky(bitmap, &sess.level.palette_rgba);
                    }
                }
                renderer.set_smooth_shading(self.cfg.render.enhancement.smooth_shading);
                renderer.set_marker_scale(self.cfg.render.enhancement.map_marker_scale);
                renderer.set_extent_fog(self.cfg.render.enhancement.map_extent_fog);
                renderer.set_fog_distance(self.cfg.render.preference.fog_distance as f32);
                renderer.set_hud_transparent(self.hud_transparent());
                renderer.set_reflections(self.cfg.render.preference.reflections);
                renderer.set_vsync(self.cfg.render.preference.vsync);
                renderer.set_render_scale(self.cfg.render.preference.anti_aliasing.render_scale());
                // Map-screen topology follows the book surface: no
                // map book (MC2, or MC1 with spell_selector=mc2) =
                // the split layout with the stretched live view.
                renderer.set_map_layout(if self.selector.map_book {
                    mgc_render::MapScreenLayout::Mc1Book
                } else {
                    mgc_render::MapScreenLayout::Mc2Split
                });
                self.renderer = Some(renderer);
            }
            Err(e) => {
                eprintln!("error: renderer init: {e}");
                event_loop.exit();
                return;
            }
        }
        window.request_redraw();
        self.window = Some(window);
        // A `--level` boot installs its session in `App::new`, BEFORE
        // this window exists — `install_level`'s closing grab was a
        // no-op (`set_grab` bails windowless). Re-assert it now: a
        // level always opens captured, this pathway included. The
        // brand-new window is typically not focused/mapped yet and
        // the platform can refuse the constraint here (X11/Wayland),
        // so the wish is ALSO armed as `boot_grab`, which the focus
        // handler and the frame loop keep retrying until one grab
        // sticks — boot is a level opening, not an alt-tab return.
        if self.screen == Screen::Level && self.session.is_some() {
            self.set_grab(true);
            // Armed even when the immediate grab reported success: an
            // X11 grab can succeed against a still-unfocused window,
            // and the WM's boot-time focus dance can bounce a
            // Focused(false)/(true) pair through AFTER it — the wish
            // must survive until one grab lands WHILE focused (that
            // success clears it).
            self.boot_grab = true;
            // A terminal launch commonly leaves input focus IN THE
            // TERMINAL (focus-follows-mouse WMs, or any WM that does
            // not auto-activate new windows) — then no Focused(true)
            // ever fires and the has_focus-gated retry stays dormant.
            // Ask the WM to activate us: booting a game is the
            // legitimate case for taking focus. (X11 honors it;
            // Wayland ignores the call but auto-activates new
            // toplevels compositor-side.) The frame loop re-asks
            // while `boot_focus_asks` counts down: this request can
            // race the WM's async map and silently no-op.
            if let Some(w) = &self.window {
                w.focus_window();
            }
            self.boot_focus_asks = 60;
        }
        self.last_frame = std::time::Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                // Retail's movie abort takes either mouse button as
                // readily as a key (`lastPressedKey || mouseLeft ||
                // mouseRight`, remc1 sub_10300).
                if self.screen == Screen::Movie {
                    if down && let Some(m) = &mut self.movie {
                        m.skip();
                    }
                    return;
                }
                // The exit-confirm dialog owns the pointer: OK
                // confirms, Cancel dismisses, everything else (and
                // any fire-through) is swallowed.
                if self.exit_confirm {
                    if down && button == MouseButton::Left {
                        let size = self.view_size();
                        let (ok, cancel) = ui::exit_confirm_rects(size.0, size.1);
                        if ui::rect_hit(ok, self.cursor) {
                            self.confirm_exit(event_loop);
                        } else if ui::rect_hit(cancel, self.cursor) {
                            // Cancel returns to play: re-lock the
                            // pointer the dialog released (unless the
                            // BOOK map underneath keeps it free) —
                            // without this, resuming took an extra
                            // click-to-recapture.
                            self.exit_confirm = false;
                            if !self.book_open() || !self.selector.map_book {
                                self.set_grab(true);
                            }
                        }
                    }
                    return;
                }
                // The pause mini-menu consumes clicks that land ON it
                // and nothing else — checked before the options menu
                // so a stacked Options layer still wins, and before
                // the in-level handlers so the panel is clickable.
                if down
                    && button == MouseButton::Left
                    && self.menu.is_none()
                    && self.mini_click(event_loop)
                {
                    return;
                }
                if self.menu.is_some() {
                    // The options menu owns the pointer while open.
                    if button == MouseButton::Left {
                        let size = self.view_size();
                        if down {
                            if let Some(assets) = ui_assets!(self) {
                                let st = self.menu.as_ref().unwrap();
                                match menu::hit_test(
                                    assets,
                                    &self.specs,
                                    st,
                                    size.0,
                                    size.1,
                                    self.cursor,
                                ) {
                                    menu::Hit::Tab(t) => {
                                        self.menu.as_mut().unwrap().set_tab(t);
                                    }
                                    menu::Hit::ScrollTo(row) => {
                                        self.menu.as_mut().unwrap().scroll_to(row);
                                    }
                                    menu::Hit::Widget(i) => {
                                        let changed = menu::pointer_apply(
                                            assets,
                                            &mut self.cfg,
                                            &self.specs,
                                            self.menu.as_mut().unwrap(),
                                            size.0,
                                            size.1,
                                            self.cursor,
                                            i,
                                            true,
                                        );
                                        let path = self.specs[i].cfg_path;
                                        if changed {
                                            self.apply_option(path);
                                        }
                                        // Click widgets persist
                                        // immediately; sliders persist
                                        // on release (not per motion
                                        // event).
                                        if changed && self.menu.as_ref().unwrap().drag.is_none() {
                                            self.persist_option(&self.specs[i]);
                                        }
                                    }
                                    menu::Hit::None => {}
                                }
                            }
                        } else if let Some(i) = self.menu.as_mut().unwrap().drag.take() {
                            self.persist_option(&self.specs[i]);
                        }
                    }
                    return;
                }
                if self.screen == Screen::Menu {
                    // The main menu owns the pointer. Save/Load
                    // clicks need the slot scan before their dialog
                    // opens.
                    if down && button == MouseButton::Left {
                        let size = self.view_size();
                        let cursor = self.cursor;
                        if self
                            .campaign
                            .as_ref()
                            .is_some_and(|c| c.id != campaign::CampaignId::Mc2)
                        {
                            if let Some(m) = &mut self.mc1menu {
                                m.click(size, cursor);
                            }
                            return;
                        }
                        let request = self.mainmenu.as_mut().and_then(|m| m.click(size, cursor));
                        match request {
                            Some("save") => {
                                let slots = scan_mc2_slots();
                                if let Some(m) = &mut self.mainmenu {
                                    m.open_slots(true, slots);
                                }
                            }
                            Some("load") => {
                                let slots = scan_mc2_slots();
                                if let Some(m) = &mut self.mainmenu {
                                    m.open_slots(false, slots);
                                }
                            }
                            Some("name") => {
                                let current = self
                                    .campaign
                                    .as_ref()
                                    .and_then(|c| c.save.mc2())
                                    .map(|s| s.player_name.clone())
                                    .unwrap_or_default();
                                if let Some(m) = &mut self.mainmenu {
                                    m.open_name(&current);
                                }
                            }
                            _ => {}
                        }
                    }
                    return;
                }
                if self.screen == Screen::Map {
                    // The world-map screen owns the pointer: a click
                    // on a portal starts the carpet leg there; the
                    // level launches when it arrives.
                    if down && matches!(button, MouseButton::Left | MouseButton::Right) {
                        let size = self.view_size();
                        let cursor = self.cursor;
                        if let (Some(wm), Some(save)) = (
                            &mut self.worldmap,
                            self.campaign.as_ref().and_then(|c| c.save.mc2()),
                        ) {
                            wm.click(save, size, cursor);
                        }
                    }
                    return;
                }
                if self.pane_open() {
                    // The CTRL selector pane (over flight OR the map
                    // screen): press anchors the level flyout for the
                    // clicked hand, release commits level + binding
                    // (remc2 PI:806-929); SHIFT+click edits the
                    // CYCLE RING (cmd 0x26 — toggle/move, no equip
                    // side-effect, PI:856-878). Fire never leaks
                    // through the pane.
                    let hand = match button {
                        MouseButton::Left => 0u8,
                        MouseButton::Right => 1u8,
                        _ => return,
                    };
                    if down {
                        if let Some(slot) = self.selector_hover.slot {
                            let spell = self.pane.as_ref().map(|p| p.order[slot]);
                            // Selectable = native-book ownership
                            // (MC2) / loadout ownership (MC1), or
                            // everything under the G instrument in
                            // MC2 (mirrors the pane view's grant).
                            let mc2 = self.is_mc2();
                            let owned = (self.cfg.gameplay.cheat.dev_spells && mc2)
                                || spell
                                    .map(|c| {
                                        let world = self
                                            .session
                                            .as_deref()
                                            .and_then(|s| s.sim.world.as_ref());
                                        world.is_some_and(|w| {
                                            if mc2 {
                                                w.mc2_book_view().owned[c as usize]
                                            } else {
                                                w.loadout().owned[c as usize]
                                            }
                                        })
                                    })
                                    .unwrap_or(false);
                            if owned {
                                if self.shift_held {
                                    // Retail's ring-edit truth table
                                    // (PI:856-878): same-button click
                                    // on a member removes it; any
                                    // other click adds/moves it to
                                    // the clicked button's ring.
                                    let spell = spell.unwrap_or(0);
                                    let side = hand + 1;
                                    let cur = self.ring_of(spell);
                                    let val = if cur == side { 0 } else { side };
                                    self.pending_ring = Some((spell, val));
                                    self.flush_equip_if_paused();
                                } else if self.selector_drag.is_none() {
                                    // A second button joining mid-drag
                                    // must not steal the live drag.
                                    self.selector_drag = Some((slot, hand));
                                }
                            }
                        }
                    } else if let Some((slot, h)) = self.selector_drag {
                        if h == hand {
                            let spell = self.pane.as_ref().map(|p| p.order[slot]).unwrap_or(0);
                            let level = self
                                .selector_hover
                                .level
                                .unwrap_or(self.spell_levels[spell as usize]);
                            self.pane_commit(slot, hand, level);
                            self.selector_drag = None;
                        }
                    }
                    self.fire_held = false;
                    self.fire_right_held = false;
                    return;
                }
                // The BOOKLESS map keeps the pointer grabbed and the
                // controls fully live (player ruling 2026-07-24) —
                // clicks fall through to normal fire handling below.
                // Only the BOOK map consumes them here.
                if self.book_open() && self.selector.map_book {
                    // Book screen: clicking an owned spell binds it to
                    // that hand (the original's commands 0x15/0x16)
                    // AND closes the book back into flight (original
                    // UX). Clicks on unowned slots
                    // or empty page do nothing.
                    if down {
                        let owned = self
                            .session
                            .as_deref()
                            .and_then(|s| s.sim.world.as_ref())
                            .map(|w| w.loadout().owned)
                            .unwrap_or([false; 24]);
                        if let Some(spell) = self.hovered {
                            if owned[spell.0 as usize] {
                                match button {
                                    MouseButton::Left => self.pending_equip.0 = Some(spell.0),
                                    MouseButton::Right => self.pending_equip.1 = Some(spell.0),
                                    _ => return,
                                }
                                if let Some(r) = &mut self.renderer {
                                    r.set_map_view(false);
                                }
                                self.set_grab(true);
                                self.flush_equip_if_paused();
                            }
                        }
                    }
                    self.fire_held = false;
                    self.fire_right_held = false;
                    return;
                }
                if down && !self.grabbed {
                    // Click-to-recapture. While PAUSED `set_grab` is a
                    // no-op (the mini-menu needs the cursor), so this
                    // just swallows the click — which is what we want:
                    // falling through would latch `fire_held` with no
                    // tick to consume it, and unpause would open with
                    // an unintended shot.
                    self.set_grab(true);
                    return; // the grab click doesn't fire
                }
                // In-flight cycle-ring rotation (remc2 PI:528-546):
                // SHIFT+click = next spell on that button's ring,
                // ALT+click = previous; the click is consumed (retail
                // clears the MouseButtonState bit — no cast fires).
                // ALT wins when both are down, as in retail's
                // else-if order.
                if down && (self.alt_held || self.shift_held) {
                    let hand = match button {
                        MouseButton::Left => 0u8,
                        MouseButton::Right => 1u8,
                        _ => return,
                    };
                    self.cycle_spell_ring(hand, self.alt_held);
                    return;
                }
                match button {
                    MouseButton::Left => self.fire_held = down,
                    MouseButton::Right => self.fire_right_held = down,
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // The options menu scrolls on the wheel (a tall tab at
                // notification-size FONT1 overflows one page).
                let rows = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y,
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / 30.0,
                };
                let size = self.view_size();
                let assets = match &self.session {
                    Some(sess) => sess.level.ui.as_ref(),
                    None => self.frontend_ui.as_ref(),
                };
                if let (Some(st), Some(assets)) = (&mut self.menu, assets) {
                    menu::scroll_by(assets, &self.specs, st, size.0, size.1, rows);
                    self.wheel_accum = 0.0;
                    return;
                }
                // Wheel spell-cycling (enhancement, the remc2/MC2HD
                // idiom — no retail analogue): wheel walks the LEFT
                // button's cycle ring, SHIFT+wheel the RIGHT; down =
                // forward, up = backward. Same walk as the faithful
                // SHIFT/ALT+click rotation, so the ring is shared.
                if !self.cfg.gameplay.enhancement.wheel_spells || !self.grabbed {
                    self.wheel_accum = 0.0;
                    return;
                }
                self.wheel_accum += rows;
                let hand = if self.shift_held { 1u8 } else { 0u8 };
                while self.wheel_accum >= 1.0 {
                    self.wheel_accum -= 1.0;
                    self.cycle_spell_ring(hand, false);
                }
                while self.wheel_accum <= -1.0 {
                    self.wheel_accum += 1.0;
                    self.cycle_spell_ring(hand, true);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Windows PINS the OS pointer to a 1×1 clip in every
                // hidden-cursor confined/locked state (winit's
                // taskbar workaround), so the only absolute reports
                // here are the pin point itself — accepting them
                // would stomp the delta-integrated software cursor.
                // The deltas own the cursor in those states.
                if self.windows_pinned_pointer() {
                    return;
                }
                self.cursor = (position.x as f32, position.y as f32);
                // The world-map screen owns a confined pointer
                // (retail captures the mouse to the 640×480 screen;
                // edge contact scrolls the map). The grab confines to
                // the WINDOW; clamp to the same rect and warp the OS
                // cursor back when it strays, which also covers
                // platforms where the Confined grab is unsupported.
                // Only the MAP confines/clamps (edge scrolling needs
                // the boundary pixel); the menus run a free pointer.
                //
                // The clamp is to the WINDOW, not to the 4:3 picture.
                // Clamping to the picture while it was anchored at the
                // top-left was the same rect; now that it is CENTRED it
                // is not, and the old clamp trapped the pointer in a
                // picture-sized box at the top-left — the map's right
                // edge became unreachable (player-reported). Letting
                // the pointer into the bars is also what makes the
                // whole bar scroll rather than one boundary pixel.
                if self.screen == Screen::Map && self.menu.is_none() {
                    let size = self.view_size();
                    let cl = (
                        self.cursor.0.clamp(0.0, size.0 - 1.0),
                        self.cursor.1.clamp(0.0, size.1 - 1.0),
                    );
                    if cl != self.cursor {
                        self.cursor = cl;
                        if let Some(w) = &self.window {
                            let _ = w.set_cursor_position(winit::dpi::PhysicalPosition::new(
                                cl.0 as f64,
                                cl.1 as f64,
                            ));
                        }
                    }
                }
                self.pointer_drag_follow();
            }
            WindowEvent::Focused(false) => {
                let pending = self.boot_grab;
                self.set_grab(false);
                // A WM focus BOUNCE during window placement must not
                // kill the boot wish — only deliberate frees do (the
                // retry is has_focus-gated, so it stays dormant while
                // the window is genuinely in the background).
                self.boot_grab = pending;
                self.fire_held = false;
                self.fire_right_held = false;
                // Alt-tabbing away eats the Alt key-up, which would
                // leave the latch stuck and turn the next bare Enter
                // into a fullscreen toggle instead of the map.
                self.alt_held = false;
                self.shift_held = false;
                self.ctrl_mod = false;
            }
            // Returning focus re-applies the pointer mode the screen
            // wants: Windows clears cursor confinement across the
            // focus bounce (ClipCursor is per-focus), and the loss arm
            // above intentionally released. Mouse-look is NOT yanked
            // back (grabbed was cleared on loss) — flight recaptures
            // on click, as always; this restores the fullscreen
            // confinement / frontend confinement only. Exception: the
            // FIRST focus of a `--level` boot completes the armed
            // boot grab (the pre-focus attempt in `resumed` can fail
            // platform-side) — that focus gain is the level opening,
            // not an alt-tab return.
            WindowEvent::Focused(true) => {
                if self.boot_grab {
                    // Completion clears the flag; failure leaves it
                    // armed for the per-frame retry.
                    self.set_grab(true);
                } else {
                    self.reassert_pointer();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                // Alt latch + Alt+Enter, both BEFORE every other key
                // path. The latch has to lead because the menu, the
                // frontend text fields and the exit dialog all `return`
                // out below, and a swallowed key-up would leave Alt
                // stuck down. Alt+Enter must lead the plain-Enter arm
                // (:4795) for the same reason it exists — otherwise the
                // classic combo just opens the map book.
                if matches!(
                    event.physical_key,
                    PhysicalKey::Code(KeyCode::AltLeft | KeyCode::AltRight)
                ) {
                    self.alt_held = down;
                }
                if down && self.alt_held && event.logical_key == Key::Named(NamedKey::Enter) {
                    self.toggle_fullscreen();
                    return;
                }
                // A movie owns the screen: ANY key abandons the rest
                // of the chain, as retail does — the skip is not per
                // movie. Placed after the Alt+Enter arm so fullscreen
                // still toggles during playback, and before every
                // other key path so nothing else sees the keypress.
                if self.screen == Screen::Movie {
                    if down && let Some(m) = &mut self.movie {
                        m.skip();
                    }
                    return;
                }
                if down && event.logical_key == Key::Named(NamedKey::Escape) {
                    if self.menu.is_some() && self.mini.is_some() {
                        // The options layer was opened FROM the
                        // in-level mini-menu: Esc closes just that
                        // layer and drops back to the mini-menu, STILL
                        // PAUSED — so the map, the spell selector and
                        // the rest of the live-input pause are usable
                        // again. Esc never dismisses the mini-menu
                        // itself; unpause is its only exit.
                        self.menu = None;
                    } else if self.menu.is_some() {
                        // Options opened directly on a frontend screen
                        // (no mini-menu behind it): Esc closes it and
                        // unpauses, as before.
                        self.toggle_menu();
                    } else if self.screen == Screen::Menu {
                        // Main menu: close the modal, else the exit
                        // confirm (retail Esc = the Exit button).
                        let mc1 = self
                            .campaign
                            .as_ref()
                            .is_some_and(|c| c.id != campaign::CampaignId::Mc2);
                        if mc1 {
                            if let Some(m) = &mut self.mc1menu {
                                m.escape();
                            } else {
                                event_loop.exit();
                            }
                        } else if let Some(m) = &mut self.mainmenu {
                            m.escape();
                        } else {
                            event_loop.exit();
                        }
                    } else if self.screen == Screen::Map {
                        // Map screen: close the dialog, else back to
                        // the main menu (retail endAction=2).
                        if let Some(wm) = &mut self.worldmap {
                            wm.escape();
                        }
                    } else if self.exit_confirm {
                        // Esc on the confirm dialog = cancel, stay
                        // in the level (the retail dialog's No) —
                        // re-locking the pointer like the mouse
                        // Cancel does.
                        self.exit_confirm = false;
                        if !self.book_open() || !self.selector.map_book {
                            self.set_grab(true);
                        }
                    } else if self.grabbed
                        && self
                            .window
                            .as_ref()
                            .is_none_or(|w| w.fullscreen().is_none())
                    {
                        // WINDOWED: the first Esc releases the pointer
                        // (to the desktop); abandoning the level takes
                        // a second press. FULLSCREEN has no desktop to
                        // release to (player ruling 2026-07-24) — Esc
                        // falls through and opens the abandon dialog
                        // directly.
                        self.set_grab(false);
                    } else if self.quit_fade.is_none() {
                        // The retail MC2 "Abandon level?" confirm
                        // (sub_18B30 → MenuState 13), reused by MC1/
                        // single-level (deliberate). The world keeps
                        // running beneath it (retail modality);
                        // confirming abandons to the hub (or exits the
                        // app in single-level mode).
                        self.exit_confirm = true;
                        self.set_grab(false);
                    } else {
                        event_loop.exit();
                    }
                    return;
                }
                // Frontend edit fields (save-slot labels, the player
                // name) swallow the keyboard while active.
                if down && self.screen != Screen::Level {
                    // Which frontend surface eats the keystroke.
                    #[derive(PartialEq)]
                    enum Edit {
                        Mc2Menu,
                        Mc1Menu,
                        Map,
                        None,
                    }
                    let mc1 = self
                        .campaign
                        .as_ref()
                        .is_some_and(|c| c.id != campaign::CampaignId::Mc2);
                    let target = if self.screen == Screen::Menu && mc1 {
                        if self.mc1menu.as_ref().is_some_and(|m| m.editing()) {
                            Edit::Mc1Menu
                        } else {
                            Edit::None
                        }
                    } else if self.screen == Screen::Menu {
                        if self.mainmenu.as_ref().is_some_and(|m| m.editing()) {
                            Edit::Mc2Menu
                        } else {
                            Edit::None
                        }
                    } else if self.worldmap.as_ref().is_some_and(|w| w.dialog_editing()) {
                        Edit::Map
                    } else {
                        Edit::None
                    };
                    if target != Edit::None {
                        let mut chars: Vec<char> = Vec::new();
                        let mut backspace = false;
                        let mut enter = false;
                        match &event.logical_key {
                            Key::Named(NamedKey::Backspace) => backspace = true,
                            Key::Named(NamedKey::Enter) => enter = true,
                            Key::Named(NamedKey::Space) => chars.push(' '),
                            Key::Character(s) => chars.extend(s.chars()),
                            _ => {}
                        }
                        match target {
                            Edit::Mc2Menu => {
                                if let Some(m) = &mut self.mainmenu {
                                    if backspace {
                                        m.key_backspace();
                                    } else if enter {
                                        m.key_enter();
                                    }
                                    for c in chars {
                                        m.key_char(c);
                                    }
                                }
                            }
                            Edit::Mc1Menu => {
                                if let Some(m) = &mut self.mc1menu {
                                    if backspace {
                                        m.key_backspace();
                                    } else if enter {
                                        m.key_enter();
                                    }
                                    for c in chars {
                                        m.key_char(c);
                                    }
                                }
                            }
                            Edit::Map => {
                                if let Some(w) = &mut self.worldmap {
                                    if backspace {
                                        w.dialog_backspace();
                                    } else if enter {
                                        w.dialog_enter();
                                    }
                                    for c in chars {
                                        w.dialog_char(c);
                                    }
                                }
                            }
                            Edit::None => {}
                        }
                        return;
                    }
                    // The map's open parchment dialog takes Enter as
                    // OK (the scroll widget's scancode-28 arm,
                    // MI:5656-58; Esc/Cancel is the Escape block
                    // above). Edit fields were serviced first.
                    if self.screen == Screen::Map
                        && event.logical_key == Key::Named(NamedKey::Enter)
                        && let Some(w) = &mut self.worldmap
                        && w.dialog_open()
                    {
                        w.dialog_enter();
                        return;
                    }
                    // The main menu swallows the rest of the
                    // keyboard, Enter first pressing the open
                    // dialog's OK; the map keeps its pan keys + P
                    // (preferences).
                    if self.screen == Screen::Menu {
                        if event.logical_key == Key::Named(NamedKey::Enter) {
                            if mc1 {
                                if let Some(m) = &mut self.mc1menu {
                                    m.key_enter();
                                }
                            } else if let Some(m) = &mut self.mainmenu {
                                m.key_enter();
                            }
                        }
                        return;
                    }
                }
                // The exit-confirm dialog owns the keyboard: Enter =
                // confirm (Esc = cancel, handled above); everything
                // else is swallowed — no pause, no option keys, no
                // movement latching under a modal.
                if self.exit_confirm {
                    if down && event.logical_key == Key::Named(NamedKey::Enter) {
                        self.confirm_exit(event_loop);
                    }
                    return;
                }
                // Pause (retail P, drawing PAUSED at 132,50): the sim
                // clock freezes, the renderer/UI stay live — and the
                // options menu rides on it (MC2 puts its options
                // behind pause too).
                if down && event.physical_key == PhysicalKey::Code(KeyCode::KeyP) {
                    self.toggle_menu();
                    return;
                }
                // The runtime option keys (F1/F2/F3/F5/F6, T/V/G/H/B/C)
                // — live in flight and inside the menu alike.
                if down
                    && let PhysicalKey::Code(code) = event.physical_key
                    && self.option_key(code)
                {
                    return;
                }
                // The menu swallows everything else (no quick-equips,
                // no map toggle, no movement latching underneath it).
                if self.menu.is_some() {
                    return;
                }
                // The fullscreen-map toggle is a GAMEPLAY key. Without
                // the screen gate, ENTER on the MC2 world map with no
                // parchment dialog open fell through the frontend
                // blocks above (Screen::Map only consumes Enter into
                // an open dialog) and landed here — toggling map_view
                // and releasing/locking the pointer on the startmap
                // (reported on Windows; reachable on every platform,
                // it only needs the dialog closed).
                if down
                    && self.screen == Screen::Level
                    && event.logical_key == Key::Named(NamedKey::Enter)
                {
                    if let Some(r) = &mut self.renderer {
                        let on = !r.map_view();
                        r.set_map_view(on);
                        // The screen-mode ding (sub_3DC90 :49072 —
                        // sound 14 on EVERY mode switch, enter and
                        // exit alike). While paused the request sits
                        // in the mixer and flushes on unpause — the
                        // retail deferred-ding quirk.
                        self.ui_ding();
                        if self.selector.map_book {
                            // The BOOK map (retail MC1): the cursor is
                            // freed for spell binding; closing returns
                            // to mouse-look. Entering/leaving fixes
                            // your ORIENTATION but not your velocity
                            // (player ground truth; traced as EMERGENT
                            // — map modes write no input, so the
                            // steering filters decay ~×0.75/tick while
                            // the target speed persists,
                            // :49017-20/:49044). We recenter the
                            // virtual stick; the sim's filters decay
                            // on their own because tick_input sends
                            // zero stick while the book is open.
                            if on {
                                self.set_grab(false);
                                self.fire_held = false;
                                self.fire_right_held = false;
                            } else {
                                self.set_grab(true);
                            }
                            self.stick = VirtualStick::default();
                        }
                        // The BOOKLESS map (retail MC2, player-ruled
                        // 2026-07-24): the pointer stays GRABBED and
                        // hidden — normal controls (mouse-look/stick,
                        // fire, keys) remain fully live under the map;
                        // the roster comes up on ALT, the selector on
                        // CTRL (which frees the cursor while held).
                        // Nothing to do on toggle.
                    }
                    return;
                }
                // CTRL = the selector pane, hold-to-show / release-to-
                // close (remc2 keys[5]=0x1D, PI:505/PI:895). Opening
                // hijacks the pointer (grab off, OS cursor visible);
                // closing cancels any live drag and returns to
                // mouse-look unless the map screen keeps the cursor.
                if matches!(
                    event.physical_key,
                    PhysicalKey::Code(KeyCode::ControlLeft | KeyCode::ControlRight)
                ) {
                    // The bare modifier first — the pane latch below
                    // returns early, and it is not even armed in
                    // default MC1, where the CTRL+digit chord lives.
                    self.ctrl_mod = down;
                }
                if matches!(
                    event.physical_key,
                    PhysicalKey::Code(KeyCode::ControlLeft | KeyCode::ControlRight)
                ) && self.pane.is_some()
                {
                    if down && !self.ctrl_held {
                        self.ctrl_held = true;
                        self.ctrl_grab_restore = self.grabbed;
                        if self
                            .session
                            .as_deref()
                            .is_some_and(|s| s.level.ui.is_some())
                        {
                            self.set_grab(false);
                            self.fire_held = false;
                            self.fire_right_held = false;
                        }
                    } else if !down && self.ctrl_held {
                        self.ctrl_held = false;
                        self.selector_drag = None;
                        self.selector_hover = ui::SelectorHover::default();
                        // Re-grab unless the BOOK map holds the cursor
                        // (the bookless map keeps controls live, so
                        // releasing CTRL over it returns to mouse-look).
                        if (!self.book_open() || !self.selector.map_book) && self.ctrl_grab_restore
                        {
                            self.set_grab(true);
                        }
                    }
                    return;
                }
                // Quick keys 1..9,0 — RETAIL, both chords. MC1 has TWO
                // digit paths, not one, and they are the two hands:
                //   bare digit  → `MakeControlCommand_188A0(24, k-2)`
                //                 (:20568) → slot +940 = LEFT
                //   CTRL+digit  → `MakeControlCommand_188A0(25, k-2)`
                //                 (:20356) → slot +944 = RIGHT
                // Both index the same per-player bind table
                // `var_15198_1875_772[digit]` (−1 = unbound) and
                // commit through the pending-command mailbox
                // (:48747 / :48766).
                //
                // ⚠ This used to read SHIFT for the right hand, from a
                // typo we inherited: remc1 annotates the gate
                // `pressedKeys_12EEF0_12EEE0[29]` as "clrl + ]", so
                // the chord looked like it needed a bracket too and
                // the whole feature got treated as ours to bind.
                // Scancode 29 is 0x1D = LEFT CTRL; `]` is 0x1B. There
                // is no bracket, and retail owns the quick binds.
                // Player-reported 2026-08-11 and decompile-confirmed.
                //
                // Ours on top: BINDING from the book (retail assigns
                // the +772 slots elsewhere). In the book, bind the
                // hovered spell to the digit; in flight, equip it.
                if down {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        let digit = match code {
                            KeyCode::Digit1 => Some(0),
                            KeyCode::Digit2 => Some(1),
                            KeyCode::Digit3 => Some(2),
                            KeyCode::Digit4 => Some(3),
                            KeyCode::Digit5 => Some(4),
                            KeyCode::Digit6 => Some(5),
                            KeyCode::Digit7 => Some(6),
                            KeyCode::Digit8 => Some(7),
                            KeyCode::Digit9 => Some(8),
                            KeyCode::Digit0 => Some(9),
                            _ => None,
                        };
                        if let Some(d) = digit {
                            // Digit-binding needs a hovered BOOK cell;
                            // the bookless map keeps flight semantics
                            // (quick-equip), same as the rest of its
                            // live controls.
                            if self.book_open() && self.selector.map_book {
                                if let Some(spell) = self.hovered {
                                    // One spell ↔ one digit (retail:
                                    // assigning a quick key unassigns
                                    // the spell's previous one) — two
                                    // slots holding the same spell
                                    // would fight over the book's
                                    // digit badge.
                                    for b in self.quick_binds.iter_mut() {
                                        if *b == Some(spell.0) {
                                            *b = None;
                                        }
                                    }
                                    self.quick_binds[d] = Some(spell.0);
                                    println!("quick key {}: {}", (d + 1) % 10, spell.name());
                                }
                            } else if let Some(spell) = self.quick_binds[d] {
                                if self.ctrl_mod {
                                    self.pending_equip.1 = Some(spell);
                                } else {
                                    self.pending_equip.0 = Some(spell);
                                }
                                self.flush_equip_if_paused();
                            }
                            return;
                        }
                    }
                }
                // Map zoom (`+`/`-`, main row or numpad): tighten or
                // widen whichever map surface is showing — the
                // in-flight radar, or the map screen while it is open
                // (each owns its own zoom; the map screen's is a port
                // addition like the radar's, session-only — player
                // ask 2026-08-07: the keys used to fall through to
                // the radar from the map screen).
                if down {
                    let zoom = match event.physical_key {
                        PhysicalKey::Code(KeyCode::Equal | KeyCode::NumpadAdd) => Some(0.8),
                        PhysicalKey::Code(KeyCode::Minus | KeyCode::NumpadSubtract) => Some(1.25),
                        _ => None,
                    };
                    if let Some(factor) = zoom {
                        if let Some(r) = &mut self.renderer {
                            if r.map_view() {
                                r.zoom_map_screen(factor);
                                println!("map zoom: {:.2}x", r.map_screen_mag());
                            } else {
                                r.zoom_minimap(factor);
                                println!("radar zoom: {:.0} tiles", r.minimap_zoom());
                            }
                        }
                        return;
                    }
                }
                // The demolish key (MC1 Shift+L, scancode 0x26 under
                // the shift branch :20496-501): razes the OWN castle
                // one level per press — the castle-as-attack-spell
                // enabler, at the price of the respawn point.
                if down && self.shift_held && event.physical_key == PhysicalKey::Code(KeyCode::KeyL)
                {
                    self.pending_demolish = true;
                    return;
                }
                let wasd = self.cfg.controls.preferences.bindings == config::Bindings::Wasd;
                let k = &mut self.keys;
                match event.physical_key {
                    // Thrust/strafe keys by binding profile. Classic =
                    // the original scheme (mouse aims, Up/Down arrows
                    // accelerate/decelerate, Left/Right strafe); the
                    // WASD profile keeps the arrows as enhanced-model
                    // turn/pitch keys.
                    PhysicalKey::Code(KeyCode::KeyW) if wasd => k.forward = down,
                    PhysicalKey::Code(KeyCode::KeyS) if wasd => k.back = down,
                    PhysicalKey::Code(KeyCode::KeyA) if wasd => k.left = down,
                    PhysicalKey::Code(KeyCode::KeyD) if wasd => k.right = down,
                    PhysicalKey::Code(KeyCode::ArrowUp) if !wasd => k.forward = down,
                    PhysicalKey::Code(KeyCode::ArrowDown) if !wasd => k.back = down,
                    PhysicalKey::Code(KeyCode::ArrowLeft) if !wasd => k.left = down,
                    PhysicalKey::Code(KeyCode::ArrowRight) if !wasd => k.right = down,
                    PhysicalKey::Code(KeyCode::ArrowLeft) => k.turn_left = down,
                    PhysicalKey::Code(KeyCode::ArrowRight) => k.turn_right = down,
                    PhysicalKey::Code(KeyCode::ArrowUp) => k.pitch_up = down,
                    PhysicalKey::Code(KeyCode::ArrowDown) => k.pitch_down = down,
                    // Extended-lift float on E/Q: Space is the
                    // original's respawn/continue key and Shift
                    // composes freely (Shift+L demolish, Shift+digit
                    // equips).
                    PhysicalKey::Code(KeyCode::KeyE) => k.up = down,
                    PhysicalKey::Code(KeyCode::KeyQ) => k.down = down,
                    PhysicalKey::Code(KeyCode::Space) => {
                        if down {
                            self.pending_respawn = true;
                        }
                    }
                    // Backspace = the retail MC2 full stop (action
                    // 0x27): speeds zero, Speed spell dies, steering
                    // recenters. Enhancement-class in MC1/HW. The
                    // stick reset is retail's
                    // SetCenterScreenForFlyAssistant mouse recenter
                    // (EF:37965 → EF:44387).
                    PhysicalKey::Code(KeyCode::Backspace) => {
                        if down {
                            self.pending_full_stop = true;
                            self.stick = VirtualStick::default();
                        }
                    }
                    // Retail accepts EITHER shift (scancodes 42/54 —
                    // :20467); the corpus takes used the right one.
                    PhysicalKey::Code(KeyCode::ShiftLeft | KeyCode::ShiftRight) => {
                        self.shift_held = down;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw_requested(event_loop);
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if !self.grabbed {
            // Windows pinned-pointer states outside the flight grab
            // (map screen, fullscreen menus/dialogs): the OS pointer
            // is nailed to a 1×1 clip, so the software cursor is OURS
            // to move — integrate the raw deltas, clamped to the
            // window like the map's absolute clamp. Elsewhere the
            // ungrabbed cursor is CursorMoved-driven; return.
            if self.windows_pinned_pointer()
                && let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            {
                let size = self.view_size();
                self.cursor.0 = (self.cursor.0 + dx as f32).clamp(0.0, size.0 - 1.0);
                self.cursor.1 = (self.cursor.1 + dy as f32).clamp(0.0, size.1 - 1.0);
                self.pointer_drag_follow();
            }
            return;
        }
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            // invert_y = true (default) is the flight-stick polarity
            // both originals ship: mouse up/forward = nose DOWN — in
            // BOTH control models. The two branches consume dy with
            // opposite senses downstream, so each applies its own flip
            // to land on the same polarity.
            let p = &self.cfg.controls.preferences;
            let inv = p.invert_y;
            // Per-axis fractions of the general sensitivity: X governs
            // the horizontal (turn/stick-x), Y the vertical (aim), in
            // BOTH control models. X defaults to half so the enhanced
            // turn damper integrates over a fluid range instead of
            // saturating to its cap on a flick (player, 2026-07-23).
            let (sx, sy) = (
                p.mouse_sensitivity_x.clamp(0.0, 1.0),
                p.mouse_sensitivity_y.clamp(0.0, 1.0),
            );
            if self.cfg.controls.models.thrust == config::ThrustModel::Classic {
                // The classic stick's native sense IS the flight-stick
                // polarity: pass dy through when inverted.
                let dy = if inv { dy } else { -dy };
                // Relative motion integrates into the virtual stick
                // POSITION (the original reads the DOS cursor offset
                // from screen center, clamped ±127 — on a 320-wide
                // screen that's ~0.8 stick units per pixel; modern
                // default trades a little of that for precision).
                let s = STICK_PER_PIXEL * p.mouse_sensitivity;
                self.stick.x = (self.stick.x + dx as f32 * s * sx).clamp(-127.0, 127.0);
                self.stick.y = (self.stick.y - dy as f32 * s * sy).clamp(-127.0, 127.0);
                self.stick_idle_ticks = 0;
                self.roll_dx += dx as f32;
            } else {
                // Mouse-look's native sense is the FPS convention:
                // flip dy when inverted.
                let dy = if inv { -dy } else { dy };
                let s = MOUSE_SENSITIVITY * p.mouse_sensitivity;
                self.mouse.yaw += dx as f32 * s * sx;
                self.mouse.pitch -= dy as f32 * s * sy;
                self.roll_dx += dx as f32;
            }
        }
    }
}

struct Args {
    level: PathBuf,
    /// `--campaign <mc1|mc1hw|mc2>`: run the game's campaign — level
    /// order, exit routing, cross-level spell carry, retail-format
    /// saves under `saves/<game>/`. Overrides `--level`.
    campaign: Option<campaign::CampaignId>,
    /// `--slot N` (1-based): the campaign save slot (MC1/HW: 1-6,
    /// MC2: 1-8), stored 0-based. `None` (the default, also `--slot
    /// 0`) = the virtual slot 0: a fresh throwaway campaign, retail's
    /// boot shape — nothing persists until the player saves in-game.
    slot: Option<usize>,
    /// `--new-game`: start the campaign fresh even if the slot holds
    /// a save (the slot is only overwritten at the first completion).
    new_game: bool,
    screenshot: Option<PathBuf>,
    /// Camera override for screenshots: x, y, z, yaw°, pitch°.
    camera: Option<[f32; 5]>,
    /// MC1 world tileset override: 0 = temperate, 1 = arctic.
    /// None = by game (mc1 temperate, mc1hw arctic).
    tileset: Option<u8>,
    /// Config file path; None = the default `mgcarpet.json` lookup.
    config: Option<PathBuf>,
    /// CLI override of `render.enhancement.smooth_shading`; None = use config.
    smooth_shading: Option<bool>,
    /// CLI override of `render.debug.map_trigger_areas`.
    map_triggers: Option<bool>,
    /// CLI override of `render.debug.health_bars`.
    health_bars: Option<bool>,
    /// CLI override of `render.preference.crosshair` (the gameplay
    /// aim cursor; the autoaim hints are menu/config-only).
    crosshair: Option<bool>,
    /// CLI override of `gameplay.cheat.dev_spells`.
    dev_spells: Option<bool>,
    /// CLI override of `dev.plausible_spellbook`.
    plausible_spellbook: Option<bool>,
    /// CLI override of `gameplay.enhancement.prune_owned_jars`.
    prune_owned_jars: Option<bool>,
    /// CLI override of `gameplay.enhancement.wheel_spells`.
    wheel_spells: Option<bool>,
    /// CLI override of `gameplay.cheat.invincible`.
    invincible: Option<bool>,
    /// CLI override of `render.enhancement.expose_jar_spells`.
    expose_jar_spells: Option<bool>,
    /// CLI override of `render.debug.grace_meter`.
    grace_meter: Option<bool>,
    /// CLI override of `render.debug.coords`.
    coords: Option<bool>,
    /// `--dev-mode`: pre-select the whole dev-instrument kit for one
    /// run (expose_jar_spells, health_bars, crosshair,
    /// map_trigger_areas, grace_meter); individual flags still
    /// override.
    dev_mode: bool,
    /// CLI overrides of the `flight` tier enums; None = use config.
    thrust: Option<config::ThrustModel>,
    altitude: Option<config::AltitudeModel>,
    bindings: Option<config::Bindings>,
    /// Write the overhead map as a PNG and exit (one pixel per tile,
    /// scaled by `map_scale`).
    map: Option<PathBuf>,
    map_scale: u32,
    /// Render `--screenshot` showing the book screen instead of the world.
    map_view: bool,
    /// Spell-selector surface override (config `spell_selector`).
    spell_selector: Option<config::SpellSelector>,
    /// In-view rival tag override (config `render.preference.rival_tags`).
    rival_tags: Option<config::RivalTags>,
    /// Narration-subtitle override (config `audio.subtitles`).
    subtitles: Option<config::Subtitles>,
    /// Fog view-distance override in tiles (config
    /// `render.preference.fog_distance`; 0 = fog off).
    fog_distance: Option<u32>,
    /// Textured parallax-sky override (config `render.preference.sky`).
    sky: Option<bool>,
    /// Water-reflection override (config `render.preference.reflections`).
    reflections: Option<bool>,
    /// Dynamic-lights override (config `render.preference.light_sources`).
    light_sources: Option<bool>,
    /// Vertical-sync override (config `render.preference.vsync`).
    vsync: Option<bool>,
    /// Borderless-fullscreen override (config
    /// `render.preference.fullscreen`).
    fullscreen: Option<bool>,
    /// FMV playback override (config `render.preference.movies`).
    movies: Option<bool>,
    /// Anti-aliasing override (`render.preference.anti_aliasing`).
    anti_aliasing: Option<config::AntiAliasing>,
    /// Movie-subtitle override (`render.preference.movie_subtitles`).
    movie_subtitles: Option<bool>,
    /// CLI override of `render.debug.fps` (the FPS overlay).
    fps: Option<bool>,
    /// Animation clock for `--screenshot` (game turns; default 0).
    /// Water-wave phase repeats every 32 (MC1) / 64 (MC2) turns.
    anim_turn: f32,
    /// Apply the original's load-time terrain features (default true).
    terrain_features: bool,
    /// Entity-pool size override (limit-removing dev flag, G-class);
    /// None = the game's pristine chassis value (1000).
    pool_slots: Option<usize>,
    awake_range: Option<u32>,
    /// `--replay <take.mgcr>`: play a recording as the session's only
    /// input source — SOURCE-AGNOSTIC (retail takes via inline input
    /// recovery, port recordings via the exact input channel);
    /// docs/RECORDING.md "Consumers".
    replay: Option<PathBuf>,
    /// `--replay-check <take.mgcr>`: the headless verifying twin —
    /// run the whole take, print the drift summary; exit 0 only on
    /// zero divergence.
    replay_check: Option<PathBuf>,
    /// `--record <out.mgcr>`: write this session as a port recording
    /// (`source:"port"`, `input:"exact"` + hash channel).
    record: Option<PathBuf>,
    /// Headless flocking probe: tick the real world and dump per-
    /// creature AI state as CSV (the goat-cohesion diagnostic).
    flock_probe: Option<PathBuf>,
    probe_ticks: u32,
    /// CSV row cadence (1 = every tick).
    probe_every: u32,
    /// Pose script: far|start|hover[:ALT]|approach[:ALT]|orbit[:ALT].
    probe_pose: String,
    /// Tracked (class, model); default (5,1) = the MC2 goat.
    probe_species: (u8, u8),
    /// Minimal environment: landscape + the tracked species only.
    probe_strip: bool,
    /// Dispositions fired at t=0 (materialize dis-gated spawns —
    /// e.g. mc2:00's dis-6 quest fireflies).
    probe_dis: Vec<u16>,
}

fn parse_args() -> Result<Args, String> {
    let mut level = get_baked_directory().join("mc1/level-000.mgcl");
    let mut campaign_id = None;
    let mut slot = None;
    let mut new_game = false;
    let mut screenshot = None;
    let mut camera = None;
    let mut tileset = None;
    let mut config = None;
    let mut smooth_shading = None;
    let mut map_triggers = None;
    let mut health_bars = None;
    let mut crosshair = None;
    let mut dev_spells = None;
    let mut plausible_spellbook = None;
    let mut prune_owned_jars = None;
    let mut wheel_spells = None;
    let mut invincible = None;
    let mut expose_jar_spells = None;
    let mut grace_meter = None;
    let mut coords = None;
    let mut dev_mode = false;
    let mut thrust = None;
    let mut altitude = None;
    let mut bindings = None;
    let mut map = None;
    let mut map_scale = 4u32;
    let mut map_view = false;
    let mut spell_selector = None;
    let mut rival_tags = None;
    let mut subtitles = None;
    let mut fog_distance = None;
    let mut sky = None;
    let mut reflections = None;
    let mut light_sources = None;
    let mut vsync = None;
    let mut fullscreen = None;
    let mut movies = None;
    let mut anti_aliasing = None;
    let mut movie_subtitles = None;
    let mut fps = None;
    let mut anim_turn = 0.0f32;
    let mut terrain_features = true;
    let mut awake_range = None;
    let mut pool_slots = None;
    let mut replay = None;
    let mut replay_check = None;
    let mut record = None;
    let mut flock_probe = None;
    let mut probe_ticks = 8000u32;
    let mut probe_every = 1u32;
    let mut probe_pose = String::from("start");
    let mut probe_species = (5u8, 1u8);
    let mut probe_strip = false;
    let mut probe_dis = Vec::new();

    /// `--level` accepts a package path or the path-free shorthand
    /// `<game>:<index>` (`mc1:32`, `mc1hw:7`, `mc2:100`) resolving to
    /// `baked/<game>/level-NNN.mgcl` — typeable before the baked tree
    /// exists, when there is no file to tab-complete (the launch
    /// itself bakes it). Anything not starting with a known game tag
    /// is a path (Windows drive prefixes like `C:` fall through).
    fn resolve_level_arg(spec: &str) -> Result<PathBuf, String> {
        match spec.split_once(':') {
            Some((game @ ("mc1" | "mc1hw" | "mc2"), index)) => {
                let index: u32 = index
                    .parse()
                    .map_err(|e| format!("--level {spec}: bad level index: {e}"))?;
                Ok(get_baked_directory().join(format!("{game}/level-{index:03}.mgcl")))
            }
            // A numeric index after an unknown tag is a typo'd
            // shorthand, not a path — fail fast instead of hunting
            // (and baking) for a file literally named `mc3:5`.
            Some((game, index)) if index.parse::<u32>().is_ok() => Err(format!(
                "--level {spec}: unknown game {game:?} (mc1, mc1hw or mc2)"
            )),
            _ => Ok(PathBuf::from(spec)),
        }
    }

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--level" => {
                level = resolve_level_arg(&it.next().ok_or("--level needs a path or game:index")?)?;
            }
            "--campaign" => {
                let v = it.next().ok_or("--campaign needs mc1, mc1hw or mc2")?;
                campaign_id = Some(campaign::CampaignId::parse(&v).ok_or_else(|| {
                    format!("--campaign {v}: unknown campaign (mc1, mc1hw or mc2)")
                })?);
            }
            "--slot" => {
                let n: usize = it
                    .next()
                    .ok_or("--slot needs a number")?
                    .parse()
                    .map_err(|e| format!("--slot: {e}"))?;
                if n > 8 {
                    return Err(
                        "--slot must be 0 (fresh, unsaved), 1-6 (mc1/mc1hw) or 1-8 (mc2)".into(),
                    );
                }
                slot = n.checked_sub(1);
            }
            "--new-game" => new_game = true,
            "--tileset" => {
                let set: u8 = it
                    .next()
                    .ok_or("--tileset needs 0 or 1")?
                    .parse()
                    .map_err(|e| format!("--tileset: {e}"))?;
                if set > 1 {
                    return Err("--tileset must be 0 (temperate) or 1 (arctic)".into());
                }
                tileset = Some(set);
            }
            "--screenshot" => {
                screenshot = Some(PathBuf::from(it.next().ok_or("--screenshot needs a path")?));
            }
            "--replay" => {
                replay = Some(PathBuf::from(
                    it.next().ok_or("--replay needs a .mgcr path")?,
                ));
            }
            "--replay-check" => {
                replay_check = Some(PathBuf::from(
                    it.next().ok_or("--replay-check needs a .mgcr path")?,
                ));
            }
            "--record" => {
                record = Some(PathBuf::from(
                    it.next().ok_or("--record needs an output .mgcr path")?,
                ));
            }
            "--flock-probe" => {
                flock_probe = Some(PathBuf::from(
                    it.next().ok_or("--flock-probe needs a csv path")?,
                ));
            }
            "--probe-ticks" => {
                probe_ticks = it
                    .next()
                    .ok_or("--probe-ticks needs a count")?
                    .parse()
                    .map_err(|e| format!("--probe-ticks: {e}"))?;
            }
            "--probe-every" => {
                probe_every = it
                    .next()
                    .ok_or("--probe-every needs a tick interval")?
                    .parse::<u32>()
                    .map_err(|e| format!("--probe-every: {e}"))?
                    .max(1);
            }
            "--probe-pose" => {
                probe_pose = it
                    .next()
                    .ok_or("--probe-pose needs far|start|hover[:ALT]|approach[:ALT]|orbit[:ALT]")?;
            }
            "--probe-species" => {
                let spec = it.next().ok_or("--probe-species needs class,model")?;
                let (c, m) = spec
                    .split_once(',')
                    .ok_or_else(|| format!("--probe-species {spec}: expected class,model"))?;
                probe_species = (
                    c.parse().map_err(|e| format!("--probe-species: {e}"))?,
                    m.parse().map_err(|e| format!("--probe-species: {e}"))?,
                );
            }
            "--probe-strip" => probe_strip = true,
            "--probe-dis" => {
                let spec = it
                    .next()
                    .ok_or("--probe-dis needs dis ids (comma-separated)")?;
                for part in spec.split(',') {
                    probe_dis.push(part.parse().map_err(|e| format!("--probe-dis: {e}"))?);
                }
            }
            "--camera" => {
                let spec = it.next().ok_or("--camera needs x,y,z,yaw,pitch")?;
                let vals: Vec<f32> = spec
                    .split(',')
                    .map(|s| s.trim().parse::<f32>())
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("--camera: {e}"))?;
                camera = Some(
                    vals.try_into()
                        .map_err(|_| "--camera needs exactly 5 values".to_string())?,
                );
            }
            "--config" => {
                config = Some(PathBuf::from(it.next().ok_or("--config needs a path")?));
            }
            "--smooth-shading" => smooth_shading = Some(true),
            "--no-smooth-shading" => smooth_shading = Some(false),
            "--map-triggers" => map_triggers = Some(true),
            "--no-map-triggers" => map_triggers = Some(false),
            "--health-bars" => health_bars = Some(true),
            "--no-health-bars" => health_bars = Some(false),
            "--crosshair" => crosshair = Some(true),
            "--no-crosshair" => crosshair = Some(false),
            "--coords" => coords = Some(true),
            "--no-coords" => coords = Some(false),
            "--dev-spells" => dev_spells = Some(true),
            "--no-dev-spells" => dev_spells = Some(false),
            "--plausible-spellbook" => plausible_spellbook = Some(true),
            "--no-plausible-spellbook" => plausible_spellbook = Some(false),
            "--prune-owned-jars" => prune_owned_jars = Some(true),
            "--no-prune-owned-jars" => prune_owned_jars = Some(false),
            "--wheel-spells" => wheel_spells = Some(true),
            "--no-wheel-spells" => wheel_spells = Some(false),
            "--invincible" => invincible = Some(true),
            "--no-invincible" => invincible = Some(false),
            "--expose-jar-spells" => expose_jar_spells = Some(true),
            "--no-expose-jar-spells" => expose_jar_spells = Some(false),
            "--grace-meter" => grace_meter = Some(true),
            "--no-grace-meter" => grace_meter = Some(false),
            "--dev-mode" => dev_mode = true,
            "--sky" => sky = Some(true),
            "--no-sky" => sky = Some(false),
            "--reflections" => reflections = Some(true),
            "--no-reflections" => reflections = Some(false),
            "--light-sources" => light_sources = Some(true),
            "--no-light-sources" => light_sources = Some(false),
            "--anti-aliasing" => {
                anti_aliasing = match it.next().as_deref() {
                    Some("off") => Some(config::AntiAliasing::Off),
                    Some("msaa") => Some(config::AntiAliasing::Msaa),
                    Some("1.5x") => Some(config::AntiAliasing::Ssaa15),
                    Some("2x") => Some(config::AntiAliasing::Ssaa2),
                    _ => return Err("--anti-aliasing wants off|msaa|1.5x|2x".into()),
                };
            }
            "--vsync" => vsync = Some(true),
            "--no-vsync" => vsync = Some(false),
            "--fullscreen" => fullscreen = Some(true),
            "--windowed" => fullscreen = Some(false),
            "--movies" => movies = Some(true),
            "--movie-subtitles" => movie_subtitles = Some(true),
            "--no-movie-subtitles" => movie_subtitles = Some(false),
            "--no-movies" => movies = Some(false),
            "--fps" => fps = Some(true),
            "--no-fps" => fps = Some(false),
            "--thrust" => {
                thrust = Some(match it.next().as_deref() {
                    // "mc1" = the legacy name for classic.
                    Some("classic") | Some("mc1") => config::ThrustModel::Classic,
                    Some("enhanced") => config::ThrustModel::Enhanced,
                    _ => return Err("--thrust needs classic|enhanced".into()),
                });
            }
            "--altitude" => {
                altitude = Some(match it.next().as_deref() {
                    // "faithful"/"extended-lift" = the legacy names.
                    Some("classic") | Some("faithful") => config::AltitudeModel::Classic,
                    Some("enhanced") | Some("extended-lift") => config::AltitudeModel::Enhanced,
                    _ => return Err("--altitude needs classic|enhanced".into()),
                });
            }
            "--bindings" => {
                bindings = Some(match it.next().as_deref() {
                    Some("classic") => config::Bindings::Classic,
                    Some("wasd") => config::Bindings::Wasd,
                    _ => return Err("--bindings needs classic|wasd".into()),
                });
            }
            "--map" => {
                map = Some(PathBuf::from(it.next().ok_or("--map needs a path")?));
            }
            "--map-scale" => {
                map_scale = it
                    .next()
                    .ok_or("--map-scale needs a factor")?
                    .parse()
                    .map_err(|e| format!("--map-scale: {e}"))?;
                if map_scale == 0 || map_scale > 16 {
                    return Err("--map-scale must be 1..=16".into());
                }
            }
            "--map-view" => map_view = true,
            "--spell-selector" => {
                spell_selector = Some(match it.next().as_deref() {
                    Some("auto") => config::SpellSelector::Auto,
                    Some("mc1") => config::SpellSelector::Mc1,
                    Some("mc2") => config::SpellSelector::Mc2,
                    Some("mc1+mc2") => config::SpellSelector::Mc1Mc2,
                    _ => return Err("--spell-selector needs auto|mc1|mc2|mc1+mc2".into()),
                });
            }
            "--rival-tags" => {
                rival_tags = Some(match it.next().as_deref() {
                    Some("auto") => config::RivalTags::Auto,
                    Some("on") => config::RivalTags::On,
                    Some("off") => config::RivalTags::Off,
                    _ => return Err("--rival-tags needs auto|on|off".into()),
                });
            }
            "--fog-distance" => {
                let n: u32 = it
                    .next()
                    .ok_or("--fog-distance needs a tile count (0 = no fog)")?
                    .parse()
                    .map_err(|e| format!("--fog-distance: {e}"))?;
                if n > 255 {
                    return Err("--fog-distance must be 0..=255 (the map is 256 tiles)".into());
                }
                fog_distance = Some(n);
            }
            "--subtitles" => {
                subtitles = Some(match it.next().as_deref() {
                    // "auto" = a legacy alias, folded into on.
                    Some("on") | Some("auto") => config::Subtitles::On,
                    Some("off") => config::Subtitles::Off,
                    _ => return Err("--subtitles needs on|off".into()),
                });
            }
            "--anim-turn" => {
                anim_turn = it
                    .next()
                    .ok_or("--anim-turn needs a turn count")?
                    .parse()
                    .map_err(|e| format!("--anim-turn: {e}"))?;
            }
            "--no-terrain-features" => terrain_features = false,
            "--pool-slots" => {
                let n: usize = it
                    .next()
                    .ok_or("--pool-slots needs a count")?
                    .parse()
                    .map_err(|e| format!("--pool-slots: {e}"))?;
                if !(2..=60000).contains(&n) {
                    return Err("--pool-slots must be in 2..=60000 (slots are u16)".into());
                }
                pool_slots = Some(n);
            }
            "--awake-range" => {
                let n: u32 = it
                    .next()
                    .ok_or("--awake-range needs a tile count (0 = always awake)")?
                    .parse()
                    .map_err(|e| format!("--awake-range: {e}"))?;
                awake_range = Some(n);
            }
            "--help" | "-h" => {
                return Err(format!(
                    "usage: mgcarpet [--level <game:index> | <baked/.../level-NNN.mgcl>] \
                     [--campaign mc1|mc1hw|mc2 [--slot N] [--new-game]] \
                     (slot 0 = default: a fresh run, saved only from the in-game menu) \
                     [--tileset 0|1] [--config <path>] \
                     [--smooth-shading|--no-smooth-shading] \
                     [--map-triggers|--no-map-triggers] \
                     [--crosshair|--no-crosshair] [--coords|--no-coords] \
                     [--health-bars|--no-health-bars] \
                     [--dev-spells|--no-dev-spells] \
                     [--plausible-spellbook|--no-plausible-spellbook] \
                     [--prune-owned-jars|--no-prune-owned-jars] \
                     [--wheel-spells|--no-wheel-spells] \
                     [--invincible|--no-invincible] \
                     [--expose-jar-spells|--no-expose-jar-spells] \
                     [--grace-meter|--no-grace-meter] \
                     [--dev-mode] \
                     [--thrust classic|enhanced] [--altitude classic|enhanced] \
                     [--bindings classic|wasd] \
                     [--spell-selector auto|mc1|mc2|mc1+mc2] \
                     [--rival-tags auto|on|off] \
                     [--subtitles on|off] [--fog-distance TILES (0 = no fog)] \
                     [--sky|--no-sky] [--reflections|--no-reflections] \
                     [--light-sources|--no-light-sources] \
                     [--vsync|--no-vsync] [--fullscreen|--windowed] \
                     [--movies|--no-movies] [--anti-aliasing off|msaa|1.5x|2x] \
                     [--fps|--no-fps] \
                     [--screenshot out.png [--camera x,y,z,yaw,pitch] [--map-view] \
                     [--anim-turn N]] \
                     [--map out.png [--map-scale N]] [--no-terrain-features] \
                     [--pool-slots N] [--awake-range TILES (0 = always awake)] \
                     [--replay take.mgcr (play a recording — retail or port — as \
                     the session; level from the header)] \
                     [--replay-check take.mgcr (headless: whole take + drift \
                     summary; exit 0 = zero divergence)] \
                     [--record out.mgcr (write this session as a port recording)] \
                     [--flock-probe out.csv [--probe-ticks N] [--probe-every N] \
                     [--probe-pose far|start|hover[:ALT]|approach[:ALT]|orbit[:ALT]] \
                     [--probe-species CLASS,MODEL] [--probe-strip] [--probe-dis N,N..]]\n\
                     enhancements persist in {} (see crates/mgc-app/src/config.rs)",
                    config::DEFAULT_PATH
                ));
            }
            other => return Err(format!("unknown argument {other} (try --help)")),
        }
    }
    Ok(Args {
        level,
        campaign: campaign_id,
        slot,
        new_game,
        screenshot,
        camera,
        tileset,
        config,
        smooth_shading,
        map_triggers,
        health_bars,
        crosshair,
        dev_spells,
        plausible_spellbook,
        prune_owned_jars,
        wheel_spells,
        invincible,
        expose_jar_spells,
        grace_meter,
        coords,
        dev_mode,
        thrust,
        altitude,
        bindings,
        map,
        map_scale,
        map_view,
        spell_selector,
        rival_tags,
        subtitles,
        fog_distance,
        sky,
        reflections,
        light_sources,
        vsync,
        fullscreen,
        movies,
        anti_aliasing,
        movie_subtitles,
        fps,
        anim_turn,
        terrain_features,
        pool_slots,
        awake_range,
        replay,
        replay_check,
        record,
        flock_probe,
        probe_ticks,
        probe_every,
        probe_pose,
        probe_species,
        probe_strip,
        probe_dis,
    })
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

/// Write the overhead map (one pixel per tile through the engine's
/// map-color path), nearest-neighbor scaled — the axis-aligned,
/// rotation-free comparison artifact for original map screenshots.
fn run_map(level: &LoadedLevel, out: &Path, scale: u32, map_triggers: bool) -> Result<(), String> {
    let n = 256usize;
    // Stamps/path are screen-space projected at render time; this raw
    // CPU dump (the diagnostic artifact) shows dots only.
    let overlay = mgc_render::MapOverlay {
        dots: level.map_dots.clone(),
        areas: if map_triggers {
            level.map_areas.clone()
        } else {
            Vec::new()
        },
    };
    let src = mgc_render::map_pixels(&level.view, &overlay);
    let s = scale as usize;
    let (w, h) = (n * s, n * s);
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let si = ((y / s) * n + x / s) * 4;
            let di = (y * w + x) * 4;
            rgba[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    write_png(out, w as u32, h as u32, &rgba)?;
    println!("{} -> {} ({}x{})", level.label, out.display(), w, h);
    Ok(())
}

/// Headless flocking probe (`--flock-probe`, the goat-cohesion
/// mystery): tick the REAL app world (the same `WorldInit::build` the
/// game plays on — not the sim-test fixture) and dump every tracked
/// creature's full AI state per tick as CSV, plus a periodic summary.
/// The pose script stands in for the player: `far` parks out of the
/// awake radius, `start` parks at the authored level start, `hover`
/// glues to the herd centroid, `approach` flies in at carpet cruise
/// and then hovers, `orbit` circles the herd — the moving-wizard
/// cases the old fixture harness never exercised.
fn run_flock_probe(
    level: &LoadedLevel,
    out: &Path,
    ticks: u32,
    every: u32,
    pose_spec: &str,
    species: (u8, u8),
    strip: bool,
    dis: &[u16],
) -> Result<(), String> {
    use std::io::Write as _;

    let Some(init) = &level.world_init else {
        return Err(
            "--flock-probe needs the living world (do not pass --no-terrain-features)".to_string(),
        );
    };

    // Torus helpers (256-tile wrap), in TILE units.
    const N: f32 = 256.0;
    let wrap_d = |d: f32| (d + N / 2.0).rem_euclid(N) - N / 2.0;
    let dist = |a: (f32, f32), b: (f32, f32)| wrap_d(a.0 - b.0).hypot(wrap_d(a.1 - b.1));
    // Circular mean per axis — the herd centroid on the torus.
    let centroid = |pts: &[(f32, f32)]| -> Option<(f32, f32)> {
        if pts.is_empty() {
            return None;
        }
        let axis = |sel: fn(&(f32, f32)) -> f32| {
            let (mut s, mut c) = (0.0f32, 0.0f32);
            for p in pts {
                let a = sel(p) / N * std::f32::consts::TAU;
                s += a.sin();
                c += a.cos();
            }
            (s.atan2(c) / std::f32::consts::TAU * N).rem_euclid(N)
        };
        Some((axis(|p| p.0), axis(|p| p.1)))
    };
    // Connected components at LINK = 6 tiles; the pose scripts follow
    // the LARGEST cluster (level-000 authors several herds — the
    // global mean lands between them).
    let components = |pts: &[(f32, f32)]| -> Vec<Vec<usize>> {
        let n = pts.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], x: usize) -> usize {
            let mut r = x;
            while p[r] != r {
                r = p[r];
            }
            let mut c = x;
            while p[c] != r {
                let nx = p[c];
                p[c] = r;
                c = nx;
            }
            r
        }
        for a in 0..n {
            for b in (a + 1)..n {
                if dist(pts[a], pts[b]) <= 6.0 {
                    let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                    parent[ra] = rb;
                }
            }
        }
        let mut groups: std::collections::HashMap<usize, Vec<usize>> = Default::default();
        for i in 0..n {
            let r = find(&mut parent, i);
            groups.entry(r).or_default().push(i);
        }
        let mut out: Vec<Vec<usize>> = groups.into_values().collect();
        out.sort_by_key(|g| std::cmp::Reverse(g.len()));
        out
    };

    // The minimal comparison environment: landscape + the tracked
    // species only — no buildings, no stage board, no rivals. The
    // start marker survives so `start` pose scripts stay meaningful.
    let world_init;
    let init = if strip {
        let i = WorldInit {
            game: init.game,
            planes: init.planes.clone(),
            things: init
                .things
                .iter()
                .filter(|t| {
                    (t.class == species.0 as u16 && t.model == species.1 as u16)
                        || (t.class == 10 && t.model == 0x52)
                        || (t.class == 3 && t.model == 4)
                })
                .cloned()
                .collect(),
            seed: init.seed,
            assets: init.assets.clone(),
            win_pct: 0,
            wizards: Default::default(),
            mc2_wizards: Default::default(),
            player_count: 1,
            stages: Vec::new(),
            stage_vars: Vec::new(),
            night_shade: init.night_shade,
            doom_level: init.doom_level,
            placeholders: init.placeholders,
            prune_owned_jars: false,
            chassis: init.chassis.clone(),
        };
        world_init = i;
        &world_init
    } else {
        init
    };
    // The level's raw StageVar rows — the herd-law bindings (graze
    // anchors, walk-to points, spawn gates) the tracked species may
    // attach to.
    for (s, v) in init.stage_vars.iter().enumerate() {
        if (v.0 as u8) & 0xF != 0 && v.0 as u8 != 0xFF {
            println!(
                "stagevar slot={s} index={:#04x} stage={} x={} y={} data={:#010x}",
                v.0 as u8, v.1, v.2, v.3, v.4
            );
        }
    }
    let mut w = init.build();
    for &d in dis {
        w.debug_fire_disposition(d);
        println!("fired disposition {d}");
    }

    // Pose script. Altitude args are TILES ABOVE THE HERD's mean
    // ground; the carpet cruises at the faithful 80 units/tick.
    const CRUISE: f32 = 80.0 / 256.0;
    let (mode, alt) = match pose_spec.split_once(':') {
        Some((m, a)) => (
            m,
            a.parse::<f32>()
                .map_err(|e| format!("--probe-pose {pose_spec}: bad altitude: {e}"))?,
        ),
        None => (pose_spec, 2.0),
    };
    let start = level.start.unwrap_or_default();
    let (mut px, mut py) = match mode {
        "far" => (2.0f32, 2.0f32),
        _ => (start.x, start.z),
    };
    let mut pz_alt = match mode {
        "far" => 40.0f32,
        _ => start.y,
    };
    let mut orbit_angle = 0.0f32;
    let mut approaching = matches!(mode, "approach" | "orbit");
    if !matches!(mode, "far" | "start" | "hover" | "approach" | "orbit") {
        return Err(format!(
            "--probe-pose {pose_spec}: unknown mode (far|start|hover[:ALT]|approach[:ALT]|orbit[:ALT])"
        ));
    }

    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut csv = std::io::BufWriter::new(file);
    writeln!(
        csv,
        "tick,slot,id,x,y,z,yaw,aim,speed,min_speed,max_speed,state,role,hold,life,awake,leader,target,attacker,cadence,px,py,pdist,blocked"
    )
    .map_err(|e| e.to_string())?;

    // Cluster count (LINK = 6 tiles — the fixture harness's metric,
    // retail reads ~1-2).
    let clusters = |pts: &[(f32, f32)]| -> usize { components(pts).len() };

    // Attribution accumulators: goat-ticks by role x speed bucket.
    // Buckets: 0 = <=18 (walk), 1 = 19..=36 (catch-up), 2 = 37..=53,
    // 3 = >=54 (flee/min-speed).
    let bucket = |s: i16| -> usize {
        match s.abs() {
            0..=18 => 0,
            19..=36 => 1,
            37..=53 => 2,
            _ => 3,
        }
    };
    let mut attrib = [[0u64; 4]; 9]; // roles 0..7 + 8 = "other state"
    // Terrain-fence telemetry: goat-ticks with the move-core block
    // latch set (retail byte[2] & 4) vs total.
    let (mut blocked_ticks, mut total_ticks) = (0u64, 0u64);
    let n0 = w.debug_flock_probe(species.0, species.1).len();
    // The species' whole-map walkability (slope fence + tile-type
    // block), dumped once beside the CSV for the terrain-pocket
    // analysis: <out>.blockmap (raw 256x256 bytes, bit0 rough / bit1
    // type).
    if let Some(map) = w.debug_block_map(species.0, species.1) {
        let bm = out.with_extension("blockmap");
        std::fs::write(&bm, &map).map_err(|e| format!("{}: {e}", bm.display()))?;
        // The raw height plane beside it (the fence metric is height-
        // difference-driven; the terrain-provenance check reads this).
        let hp = out.with_extension("heights");
        std::fs::write(&hp, &w.planes().height).map_err(|e| format!("{}: {e}", hp.display()))?;
        let rough = map.iter().filter(|&&b| b & 1 != 0).count();
        let typ = map.iter().filter(|&&b| b & 2 != 0).count();
        println!(
            "block map: {} rough / {} type-blocked of 65536 tiles -> {}",
            rough,
            typ,
            bm.display()
        );
    }
    let idle = mgc_sim::engine::world::PlayerCommand::default();
    println!(
        "flock probe: ({},{}) n={} pose={} ticks={} strip={} -> {}",
        species.0,
        species.1,
        n0,
        pose_spec,
        ticks,
        strip,
        out.display()
    );

    for t in 1..=ticks {
        // Advance the pose script from LAST tick's herd view.
        let rows = w.debug_flock_probe(species.0, species.1);
        let live: Vec<(f32, f32)> = rows
            .iter()
            .filter(|r| r.life >= 0)
            .map(|r| (r.x as f32 / 256.0, r.y as f32 / 256.0))
            .collect();
        // Follow the biggest herd, not the between-herds global mean.
        let c = components(&live)
            .first()
            .map(|g| g.iter().map(|&i| live[i]).collect::<Vec<_>>())
            .and_then(|pts| centroid(&pts));
        let ground = {
            let zs: Vec<f32> = rows
                .iter()
                .filter(|r| r.life >= 0)
                .map(|r| r.z as f32 / 256.0)
                .collect();
            if zs.is_empty() {
                pz_alt
            } else {
                zs.iter().sum::<f32>() / zs.len() as f32
            }
        };
        match (mode, c) {
            ("hover", Some(c)) => {
                px = c.0;
                py = c.1;
                pz_alt = ground + alt;
            }
            ("approach" | "orbit", Some(c)) => {
                if approaching {
                    let d = dist((px, py), c);
                    if d <= if mode == "orbit" { 4.0 } else { 1.0 } {
                        approaching = false;
                    } else {
                        let (dx, dy) = (wrap_d(c.0 - px), wrap_d(c.1 - py));
                        let step = CRUISE.min(d);
                        px = (px + dx / d * step).rem_euclid(N);
                        py = (py + dy / d * step).rem_euclid(N);
                        // Descend toward hover altitude on the way in.
                        pz_alt += ((ground + alt) - pz_alt).clamp(-0.1, 0.1);
                    }
                }
                if !approaching {
                    if mode == "orbit" {
                        orbit_angle += 0.02;
                        px = (c.0 + 4.0 * orbit_angle.cos()).rem_euclid(N);
                        py = (c.1 + 4.0 * orbit_angle.sin()).rem_euclid(N);
                    } else {
                        px = c.0;
                        py = c.1;
                    }
                    pz_alt = ground + alt;
                }
            }
            _ => {}
        }
        let pose = mgc_sim::engine::world::PlayerPose::from_tiles(px, pz_alt, py, 0.0, 0.0, 0.0);
        w.tick(pose, idle);

        let rows = w.debug_flock_probe(species.0, species.1);
        for r in &rows {
            if r.life >= 0 {
                let role = r.state.wrapping_sub(8) as usize;
                attrib[if role < 8 { role } else { 8 }][bucket(r.speed)] += 1;
                total_ticks += 1;
                if r.flags & (1 << 27) != 0 {
                    blocked_ticks += 1;
                }
            }
            if t % every == 0 {
                let (gx, gy) = (r.x as f32 / 256.0, r.y as f32 / 256.0);
                let pd = dist((gx, gy), (px, py));
                writeln!(
                    csv,
                    "{t},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{pd:.2},{}",
                    r.slot,
                    r.id24,
                    r.x,
                    r.y,
                    r.z,
                    r.yaw,
                    r.aim,
                    r.speed,
                    r.min_speed,
                    r.max_speed,
                    r.state,
                    r.state.wrapping_sub(8),
                    r.hold,
                    r.life,
                    r.awake,
                    r.leader,
                    r.target,
                    r.attacker,
                    r.cadence,
                    px * 256.0,
                    py * 256.0,
                    (r.flags >> 27) & 1,
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if t % 500 == 0 || t == ticks {
            let live: Vec<(f32, f32)> = rows
                .iter()
                .filter(|r| r.life >= 0)
                .map(|r| (r.x as f32 / 256.0, r.y as f32 / 256.0))
                .collect();
            let mut roles = [0usize; 9];
            let mut speeds = [0usize; 4];
            let mut fast = 0usize;
            for r in rows.iter().filter(|r| r.life >= 0) {
                let role = r.state.wrapping_sub(8) as usize;
                roles[if role < 8 { role } else { 8 }] += 1;
                speeds[bucket(r.speed)] += 1;
                if r.speed.abs() > 18 {
                    fast += 1;
                }
            }
            println!(
                "t={t}: alive={}/{n0} clusters={} roles[patrol={} wander={} chase={} FOLLOW={} flee={} other={}] speed[<=18:{} 19-36:{} 37-53:{} >=54:{}] fast={fast} player=({:.0},{:.0})",
                live.len(),
                clusters(&live),
                roles[0],
                roles[1],
                roles[2],
                roles[3],
                roles[6],
                roles[4] + roles[5] + roles[7] + roles[8],
                speeds[0],
                speeds[1],
                speeds[2],
                speeds[3],
                px,
                py
            );
        }
    }

    println!(
        "\nterrain fence: {blocked_ticks}/{total_ticks} goat-ticks with the block latch set ({:.2}%)",
        100.0 * blocked_ticks as f64 / total_ticks.max(1) as f64
    );
    println!("attribution (goat-ticks by role x speed bucket):");
    println!("  role         <=18     19-36    37-53    >=54");
    let names = [
        "patrol", "wander", "chase", "FOLLOW", "prekill", "kill", "flee", "role7", "other",
    ];
    for (i, name) in names.iter().enumerate() {
        let row = &attrib[i];
        if row.iter().any(|&v| v != 0) {
            println!(
                "  {name:<10} {:>8} {:>8} {:>8} {:>8}",
                row[0], row[1], row[2], row[3]
            );
        }
    }
    csv.flush().map_err(|e| e.to_string())?;
    println!("wrote {}", out.display());
    Ok(())
}

/// The MC2 environment's sky/fog color, sRGB: the mode of the bundle
/// shade LUT's row 0 — the engine's fog FAR color, i.e. what distant
/// terrain fades into (night/cave = black, day = pale blue; a few
/// row-0 entries deviate for reserved/animated palette slots, hence
/// the mode). None for MC1/HW — their certified presentation keeps
/// the renderer's hand-picked haze constant until the sky trace
/// lands (the same TABLES row-0 structure exists there too).
fn mc2_sky_srgb(level: &LoadedLevel) -> Option<[f32; 3]> {
    if !matches!(level.game, mgc_sim::ids::GameId::Mc2) {
        return None;
    }
    let row0 = level.view.shade_lut.get(..256)?;
    let mut counts = [0u16; 256];
    for &i in row0 {
        counts[i as usize] += 1;
    }
    let idx = (0..256).max_by_key(|&i| counts[i])?;
    let rgb = level.view.palette[idx];
    Some([
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ])
}

/// Apply the playtest instruments to a freshly built world — ONE place
/// so a future instrument can't miss a call site (fresh start in
/// `App::new`, `restart_level`, and the headless screenshot path all
/// go through here).
fn apply_instruments(
    w: &mut mgc_sim::engine::world::World,
    dev_spells: bool,
    plausible_spells: &[u8],
    plausible_book_mc2: &[(u8, i32)],
    invincible: bool,
    patches: mgc_sim::WorldPatches,
) {
    w.set_patches(patches);
    if dev_spells {
        w.set_dev_spells(true);
    }
    if !plausible_spells.is_empty() {
        w.grant_spells(plausible_spells);
    }
    if !plausible_book_mc2.is_empty() {
        w.mc2_grant_plausible(plausible_book_mc2);
    }
    if invincible {
        w.set_invincible(true);
    }
}

/// The config → sim mapping for the `gameplay · patches` class. The
/// world constructor defaults every arm to RETAIL (that is what keeps
/// goldens/tests/mgc-conform faithful); the app opts the configured
/// patches in here, and `apply_option` re-applies live.
fn world_patches(p: &config::GameplayPatches) -> mgc_sim::WorldPatches {
    mgc_sim::WorldPatches {
        castle_recast_cost: p.castle_recast_cost.on(),
        jar_ground_snap: p.jar_ground_snap.on(),
        ball_ground_track: p.ball_ground_track.on(),
        map_wide_ball_rolling: p.map_wide_ball_rolling.on(),
        possessed_footprint: p.possessed_footprint.on(),
        mc2_downgrade_overflow: p.mc2_downgrade_overflow.on(),
        mc2_magic_mine: p.mc2_magic_mine.on(),
        castle_latch_bug: p.castle_latch_bug.on(),
    }
}

/// The cycle-ring walk (`sub_18DA0` PI:1839-1942), pure: step from
/// the equipped spell `cur` (−1 = empty hand) by ±1, wrap around
/// `ring.len()`, return the first spell that is BOTH possessed and a
/// member of `side`'s ring (1 = left, 2 = right). Checks exactly one
/// full lap — the equipped spell itself is the LAST candidate, so a
/// single-member ring re-selects itself — and returns `None` when no
/// member qualifies (the all-unavailable no-op, PI:1889/1931).
fn ring_next(ring: &[u8], owned: &[bool], side: u8, cur: i32, backward: bool) -> Option<usize> {
    let n = ring.len() as i32;
    let step: i32 = if backward { -1 } else { 1 };
    let mut i = cur + step;
    for _ in 0..n {
        if i < 0 {
            i = n - 1;
        } else if i >= n {
            i = 0;
        }
        let s = i as usize;
        if ring[s] == side && owned[s] {
            return Some(s);
        }
        i += step;
    }
    None
}

/// Install the campaign's cross-level carry into a fresh world.
/// MC1/HW: grant collected-flags ∩ the level's availability mask
/// (the retail human-branch grant law, :49226-33). MC2: learn the
/// carried book with its banked XP — `mc2_grant_plausible` is the
/// same grant+bank+re-derive path retail's `sub_549A0` carry feeds.
fn apply_campaign_book(
    w: &mut mgc_sim::engine::world::World,
    run: &CampaignRun,
    level: &LoadedLevel,
) {
    match run.id {
        campaign::CampaignId::Mc2 => {
            let Some(save) = run.save.mc2() else { return };
            let book = save.book();
            let grants: Vec<(u8, i32)> = (0..26)
                .filter(|&s| book.owned[s])
                .map(|s| (s as u8, book.xp[s]))
                .collect();
            if !grants.is_empty() {
                w.mc2_grant_plausible(&grants);
            }
            // The rest of retail's sub_549A0 carry: selected tiers +
            // the cycle ring (raw), hands kept where still possessed
            // (the L:1332-35 validation).
            w.mc2_install_selector_carry(&book.sel, &book.ring, book.left, book.right);
        }
        _ => {
            let Some(save) = run.save.mc1() else { return };
            let mut spells: Vec<u8> = (0..24)
                .filter(|&s| save.blob24[s] != 0)
                .map(|s| s as u8)
                .collect();
            if let Some(mask) = &level.allowed_spells {
                spells.retain(|&s| mask.get(s as usize).is_none_or(|&v| v == 1));
            }
            if !spells.is_empty() {
                w.grant_spells(&spells);
            }
            // Cycle-ring carry (native-only sidecar; kept RAW like
            // MC2's — unavailable members are skipped at cycle time,
            // not dropped).
            w.install_mc1_ring(run.mc1_ring);
        }
    }
}

/// The won-edge bookkeeping: fold the finished level into the
/// campaign record, decide what follows (`CampaignRun::next`), and
/// persist the slot file. A free function because it runs inside the
/// redraw's `&mut sim.world` borrow (field-disjoint from
/// `self.campaign`).
/// MC1's world-won congratulation movie. Retail keeps TWO of them and
/// picks by the parity of the free-running 120 Hz timer
/// (`dword_AC5D4_AC5C4 & 1`, remc1:59905) — a coin flip with no level
/// or world index in it, so ours flips a coin too.
///
/// Its sibling `LEVELOSE.DAT` (the world-LOST movie, the `& 4` arm of
/// the same test) has no seam here: a failed MC1 level does not route
/// back through a post-level screen in this engine. See
/// docs/FIDELITY.md.
fn mc1_win_movie() -> movie::Cue {
    let flip = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos())
        & 1;
    movie::Cue::new(if flip == 0 { "levelw1" } else { "levelw2" })
}

fn campaign_complete(run: &mut CampaignRun, level: u32, w: &mgc_sim::engine::world::World) {
    use campaign::{CampaignId, NextStep};
    match run.id {
        CampaignId::Mc2 => {
            let Some(save) = run.save.mc2_mut() else {
                return;
            };
            // Book carry: serialize the live book into str_611 (all
            // XP banked — the between-levels shape).
            let v = w.mc2_book_view();
            save.set_book(&saves::Mc2Book {
                owned: v.owned,
                xp: v.xp,
                levels: v.levels,
                sel: v.sel,
                left: v.left,
                right: v.right,
                ring: v.ring,
            });
            let exit = w.mc2_exit_model();
            if campaign::mc2_is_secret(level) {
                // A completed secret: portal → completed (sprite
                // 305) AND the parent main is only NOW promoted —
                // the mouth exit skipped the map, so the parent's
                // completion rides the secret's (PortalsUpdate's
                // secret arm force-activates the parent,
                // MenusAndIntros.cpp:2226-45). A failed secret thus
                // leaves the parent pending with its portal revealed
                // — the retry loop the X-exit routing arm serves.
                for p in save.secrets.iter_mut() {
                    if p.level as u32 == level {
                        p.activated = 1;
                        p.sprite = 305;
                        save.levels_completed = save.levels_completed.max(p.parent as u32 + 1);
                    }
                }
            } else if exit == Some(4) {
                // The demon-mouth exit: reveal the attached secret
                // (hidden 3 → revealed 2, sprite 270) and jump in
                // WITHOUT promoting this level — no map visit runs
                // PortalsUpdate, so the parent completes only when
                // the secret does.
                for p in save.secrets.iter_mut() {
                    if p.parent as u32 == level && p.activated == 3 {
                        p.activated = 2;
                        p.sprite = 270;
                    }
                }
            } else if save
                .secrets
                .iter()
                .any(|p| p.parent as u32 == level && p.activated == 2)
            {
                // The checkpoint X with this level's secret still
                // revealed-uncompleted: routed back into the secret
                // (EF:60534-44) — the level stays pending like the
                // mouth path.
            } else {
                // The plain checkpoint X: open the linear prefix
                // through L (numLevelsCompleted counts opened
                // portals; replays never regress it).
                save.levels_completed = save.levels_completed.max(level + 1);
            }
            let secret_pending = save
                .secrets
                .iter()
                .any(|p| p.parent as u32 == level && p.activated == 2);
            run.next = Some(campaign::mc2_next_step(
                level,
                exit.unwrap_or(3),
                secret_pending,
            ));
            run.persist();
        }
        CampaignId::Mc1 | CampaignId::Mc1Hw => {
            let hw = run.id == CampaignId::Mc1Hw;
            let Some(save) = run.save.mc1_mut() else {
                return;
            };
            // Commit collected spells to the persistent flags (the
            // retail level-completion commit into var_15318).
            let loadout = w.loadout();
            for s in 0..24 {
                if loadout.owned[s] {
                    save.blob24[s] = 1;
                }
            }
            // Cycle-ring carry (native-only sidecar).
            run.mc1_ring = loadout.ring;
            let next = campaign::mc1_next_level(level, hw);
            // `level` in the save = the level to PLAY on resume
            // (retail's post-increment var_u16_17 semantic; the end
            // value marks a finished campaign).
            save.level = match next {
                NextStep::Level(n) => n as u16,
                _ => {
                    if hw {
                        25
                    } else {
                        50
                    }
                }
            };
            run.next = Some(next);
            run.persist();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_screenshot(
    mut level: LoadedLevel,
    out: &Path,
    camera: Option<[f32; 5]>,
    smooth_shading: bool,
    fog_distance: u32,
    sky_texture: bool,
    reflections: bool,
    light_sources: bool,
    map_view: bool,
    anim_turn: f32,
    map_triggers: bool,
    dev_spells: bool,
    cfg_hud_transparent: bool,
) -> Result<(), String> {
    // Same 2×-native 4:3 size as the live default window: integer
    // pixel grid (no fractional-scale aliasing), retail aspect.
    let mut renderer = Renderer::offscreen(1280, 960).map_err(|e| e.to_string())?;
    let overlay = mgc_render::MapOverlay {
        dots: level.map_dots.clone(),
        areas: if map_triggers {
            level.map_areas.clone()
        } else {
            Vec::new()
        },
    };
    renderer.load_level(&level.view, &overlay);
    // The interactive tick rebuilds map_stamps each frame (advertised
    // trigger X/O markers + pose stamps); the headless capture must do
    // the same one-shot so a map screenshot matches live play.
    if let Some(w) = &level.world {
        level.map_stamps.extend(entities::exit_marker_stamps(
            &w.advertised_marker_poses(),
            &level.map_icons,
        ));
    }
    renderer.set_map_stamps(level.map_stamps.clone());
    // Objective-guide marks in map-view captures (steady, no blink).
    if let Some(w) = &level.world {
        let marks: Vec<_> = w
            .mc2_objective_targets()
            .into_iter()
            .map(|t| mgc_render::ObjectiveMark {
                x: t.x,
                z: t.z,
                nearest: t.nearest,
                yellow: t.yellow,
            })
            .collect();
        // Tick 68 = both blink gates "on" (outline 1-in-4 + arrow window),
        // so a still capture shows the full overlay.
        renderer.set_objective_marks(marks, 68);
    }
    if let Some((index, atlas)) = &level.sprites {
        renderer.load_sprites(index.clone(), atlas);
    }
    if let Some(assets) = &level.ui {
        renderer.load_ui_atlas(assets.atlas_w, assets.atlas_h, &assets.atlas_rgba);
        if let Ok(p) = std::env::var("MGC_DUMP_UI_ATLAS") {
            write_png(
                Path::new(&p),
                assets.atlas_w,
                assets.atlas_h,
                &assets.atlas_rgba,
            )?;
        }
    }
    renderer.set_billboards(level.billboards.clone());
    renderer.set_smooth_shading(smooth_shading);
    renderer.set_fog_distance(fog_distance as f32);
    if let Some(sky) = mc2_sky_srgb(&level) {
        renderer.set_sky_color(sky);
    }
    if sky_texture && let Some(bitmap) = &level.sky {
        renderer.load_sky(bitmap, &level.palette_rgba);
    }
    renderer.set_reflections(reflections);
    if light_sources
        && level.mc2_env != entities::Mc2MapEnv::Day
        && let Some(w) = &level.world
    {
        renderer.set_lights(&entities::lights_from_poses(&w.live_poses()));
    }
    // HUD transparency: the config decides (same path as live play);
    // MGC_HUD_OPAQUE overrides for A/B captures — by VALUE, so
    // MGC_HUD_OPAQUE=0 forces transparent and =1 forces opaque; an
    // unrecognized value warns and defers to the config.
    let hud_transparent = match std::env::var("MGC_HUD_OPAQUE") {
        Ok(v) => match v.as_str() {
            "" | "0" | "false" | "off" => true,
            "1" | "true" | "on" => false,
            other => {
                eprintln!("MGC_HUD_OPAQUE={other} not understood (use 0/1); using config");
                cfg_hud_transparent
            }
        },
        Err(_) => cfg_hud_transparent,
    };
    renderer.set_hud_transparent(hud_transparent);
    renderer.set_map_view(map_view);
    // Screenshots follow the game's faithful map topology (no config
    // override in the headless path): MC2 = the split layout.
    let shot_is_mc2 = matches!(level.game, mgc_sim::ids::GameId::Mc2);
    renderer.set_map_layout(if shot_is_mc2 {
        mgc_render::MapScreenLayout::Mc2Split
    } else {
        mgc_render::MapScreenLayout::Mc1Book
    });
    renderer.set_anim_turn(anim_turn);
    // Spell UI (book grid or HUD), from the level-start loadout.
    if let (Some(assets), Some(w)) = (&level.ui, &mut level.world) {
        // invincible=false: a single headless frame takes no damage.
        apply_instruments(
            w,
            dev_spells,
            &level.plausible_spells,
            &level.plausible_book_mc2,
            false,
            mgc_sim::WorldPatches::RETAIL,
        );
        let loadout = w.loadout();
        let vitals = w.vitals();
        let mc2_book = shot_is_mc2.then(|| w.mc2_book_view());
        let quads = if map_view {
            if shot_is_mc2 {
                // MC2's map screen has no book half; the split layout
                // shows the stretched live view instead.
                Vec::new()
            } else {
                ui::book_quads(assets, &loadout, &[None; 10], 1280.0, 960.0, (-1.0, -1.0)).0
            }
        } else {
            // alert_blink=true: a screenshot shows any armed alert.
            ui::hud_quads(
                assets,
                &loadout,
                &vitals,
                hud_transparent,
                true,
                shot_is_mc2,
                mc2_book.as_ref(),
                dev_spells,
                1280.0,
                960.0,
            )
        };
        renderer.set_ui_quads(quads);
    }
    let flyer = level.start.unwrap_or_default();
    let [x, y, z, yaw_deg, pitch_deg] = camera.unwrap_or([
        flyer.x,
        flyer.y,
        flyer.z,
        flyer.yaw.to_degrees(),
        flyer.pitch.to_degrees(),
    ]);
    let cam = CameraView {
        x,
        y,
        z,
        yaw: yaw_deg.to_radians(),
        pitch: pitch_deg.to_radians(),
        roll: 0.0,
        fov_y: FOV_Y,
    };
    // PROTOTYPE fire preview (MGC_FIRE_PREVIEW=1): override the camera
    // to a fixed vantage and drop a rapid-fire row of fireballs (to see
    // them merge) plus an impact blossom — a still for discussing shape,
    // colour, size. Reuses the real emitter with synthetic poses.
    let cam = if let Ok(mode) = std::env::var("MGC_FIRE_PREVIEW") {
        use mgc_sim::engine::world::LivePose;
        let mk = |slot: u16, class: u8, model: u8, x: f32, z: f32, alt: f32| LivePose {
            slot,
            generation: 1,
            class,
            model,
            type_index: 0,
            frame: 0,
            x,
            z,
            alt,
            yaw: 0.0,
            segment: false,
            // Synthetic poses on no tile chain — the neutral co-tile
            // rank (`LivePose::chain_depth`).
            chain_depth: 0.5,
            life_frac: None,
            fire_life: None,
            player_owned: true,
            team: Some(0),
            blend: 0,
            map_only: false,
            flame_scale: 1.0,
            sprite_h_units: None,
        };
        let mut prev = Vec::new();
        let mut cur = Vec::new();
        // Blast ledger for the emitters: the meteor scene fills it, the
        // projectile scenes leave it empty (no crater).
        let mut ledger = entities::BlastLedger::default();
        // Sub-tick fraction fed to the emitters (meteor overrides).
        let mut a = 1.0f32;
        let preview_cam = if mode == "meteor" {
            // Real comb-law crater: a synthetic ledger blast drives the
            // procedural walls + smoke + shockwave at phase T
            // (`MGC_FIRE_PHASE` ticks after detonation, default 4.3 =
            // mid wave-1 sweep; try ~8.5 for the wave-2 re-burn, ~13
            // for the lingering smoke). `MGC_FIRE_PASSES` (default 11
            // = the MC1 meteor) picks the driver's fuse: 2/5/10 = the
            // MC2 tiers, 70 = the doomsday sphere's cycling firestorm.
            let t: f32 = std::env::var("MGC_FIRE_PHASE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4.3);
            let passes: f32 = std::env::var("MGC_FIRE_PASSES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(11.0);
            let (cx, cz) = (132.0f32, 128.0f32);
            a = t.fract();
            ledger = entities::BlastLedger::synthetic(vec![entities::LedgerBlast {
                slot: 999,
                generation: 1,
                x: cx,
                z: cz,
                plane_z: 6.0,
                elapsed: t.floor(),
                passes,
            }]);
            // The crater (walls + smoke) is fully ledger-driven now —
            // no synthetic cells needed; the ledger entry above is the
            // whole scene. (cx/cz feed only that entry.)
            let _ = (cx, cz);
            CameraView {
                x: 120.0,
                y: 13.0,
                z: 128.0,
                yaw: 90.0_f32.to_radians(),
                pitch: -30.0_f32.to_radians(),
                roll: 0.0,
                fov_y: FOV_Y,
            }
        } else if mode == "fp" {
            // First-person: the player has just loosed a rapid-fire
            // stream flying AWAY (+x); nearest ball ~3 tiles ahead so
            // its on-screen size reads at gameplay scale.
            // A single ball just launched, receding ~6 tiles ahead with
            // a slight rightward cross-drift so we read its side at
            // gameplay scale (a stream fired straight away just stacks
            // into one head-on bloom).
            cur.push(mk(0, 9, 0, 126.5, 129.0, 7.2));
            prev.push(mk(0, 9, 0, 126.5 - 1.4, 129.0 - 0.7, 7.2));
            CameraView {
                x: 120.0,
                y: 8.5,
                z: 127.0,
                yaw: 90.0_f32.to_radians(),
                pitch: -4.0_f32.to_radians(),
                roll: 0.0,
                fov_y: FOV_Y,
            }
        } else if mode == "single" {
            // One fireball flying across-screen (+z) at 1.8 tiles/tick,
            // seen close so the comet head + trail structure is legible.
            cur.push(mk(0, 9, 0, 134.0, 128.0, 6.0));
            prev.push(mk(0, 9, 0, 134.0, 128.0 - 2.2, 6.0));
            CameraView {
                x: 126.0,
                y: 6.5,
                z: 128.0,
                yaw: 90.0_f32.to_radians(),
                pitch: -2.0_f32.to_radians(),
                roll: 0.0,
                fov_y: FOV_Y,
            }
        } else {
            // A rapid-fire stream: heads flying across-screen (+z) at
            // 1.5 tiles/tick, spaced ~1 tile (true held-fire cadence) so
            // the tile-contained balls fuse via overlapping wakes.
            // prev = cur − velocity so the emitter recovers the trail.
            let vel = 1.5f32;
            for i in 0..10u16 {
                let z = 121.0 + i as f32 * 1.0;
                cur.push(mk(i, 9, 0, 134.0, z, 6.0));
                prev.push(mk(i, 9, 0, 134.0, z - vel, 6.0));
            }
            // A lone hero fireball with a longer isolated trail.
            cur.push(mk(20, 9, 0, 136.0, 138.0, 6.5));
            prev.push(mk(20, 9, 0, 136.0, 138.0 - 2.2, 6.5));
            // An impact blossom (class 10).
            cur.push(mk(30, 10, 0, 140.0, 118.0, 6.0));
            prev.push(mk(30, 10, 0, 140.0, 118.0, 6.0));
            CameraView {
                x: 120.0,
                y: 6.0,
                z: 128.0,
                yaw: 90.0_f32.to_radians(),
                pitch: 0.0,
                roll: 0.0,
                fov_y: FOV_Y,
            }
        };
        // Same assembly order as the live path: crater first (it wins
        // density-cap slots), then cells/projectiles, then shockwave.
        let mut fire = entities::crater_particles(&ledger, &level.view.height, a, 3.7);
        fire.extend(entities::fire_particles_from_poses(
            &prev, &cur, &ledger, a, 3.7,
        ));
        fire.extend(entities::shockwave_particles(
            &ledger,
            &level.view.height,
            a,
            3.7,
        ));
        renderer.set_fire_particles(entities::cap_particle_density(fire));
        preview_cam
    } else {
        cam
    };
    renderer.render(&cam).map_err(|e| format!("render: {e}"))?;
    let (w, h, rgba) = renderer.read_offscreen();
    write_png(out, w, h, &rgba)?;
    println!("{} -> {} ({}x{})", level.label, out.display(), w, h);
    Ok(())
}

/// The whole shell: parse args, load config and packages, build the
/// [`App`], run it to exit. `event_loop` is for embedding shells whose
/// loop needs a platform-specific builder (the Android/OpenXR port);
/// `None` builds a plain one — and only on the windowed path, so the
/// headless modes (`--screenshot`, `--map`) keep working with no
/// display server at all.
pub fn game_main(event_loop: Option<EventLoop<()>>) -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::from(2);
        }
    };
    let (config_path, explicit) = match &args.config {
        Some(p) => (p.clone(), true),
        None => (PathBuf::from(config::DEFAULT_PATH), false),
    };
    let mut cfg = match config::Config::load(&config_path, explicit) {
        Ok(c) => {
            if config_path.exists() {
                println!("config: {}", config_path.display());
            }
            c
        }
        Err(e) => {
            eprintln!("error: config: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // Fold the one-run CLI overrides into the resolved config, so `cfg`
    // is the single source of truth from here on (the App reads it live;
    // the startup summary and the options menu are views over it).
    //
    // --dev-mode first: the whole dev-instrument kit in one flag
    // (individual flags after it still override).
    if args.dev_mode {
        cfg.render.enhancement.expose_jar_spells = true;
        cfg.render.debug.health_bars = true;
        cfg.render.debug.autoaim_hints = true;
        cfg.render.debug.map_trigger_areas = true;
        cfg.render.debug.grace_meter = true;
        cfg.render.debug.fps = true;
        cfg.render.debug.coords = true;
        println!(
            "dev-mode: expose_jar_spells + health_bars + autoaim_hints + map_trigger_areas + \
             grace_meter + fps + coords on (one run)"
        );
    }
    let en = &mut cfg.render.enhancement;
    if let Some(v) = args.smooth_shading {
        en.smooth_shading = v;
    }
    if let Some(v) = args.expose_jar_spells {
        en.expose_jar_spells = v;
    }
    let de = &mut cfg.render.debug;
    if let Some(v) = args.map_triggers {
        de.map_trigger_areas = v;
    }
    if let Some(v) = args.health_bars {
        de.health_bars = v;
    }
    if let Some(v) = args.grace_meter {
        de.grace_meter = v;
    }
    if let Some(v) = args.coords {
        de.coords = v;
    }
    if let Some(v) = args.crosshair {
        cfg.render.preference.crosshair = v;
    }
    if let Some(v) = args.thrust {
        cfg.controls.models.thrust = v;
    }
    if let Some(v) = args.altitude {
        cfg.controls.models.altitude = v;
    }
    if let Some(v) = args.bindings {
        cfg.controls.preferences.bindings = v;
    }
    if let Some(v) = args.spell_selector {
        cfg.gameplay.enhancement.spell_selector = v;
    }
    if let Some(v) = args.rival_tags {
        cfg.render.preference.rival_tags = v;
    }
    if let Some(v) = args.subtitles {
        cfg.audio.subtitles = v;
    }
    if let Some(v) = args.fog_distance {
        // Same cap as config load: the fog band must never reach the
        // terrain silhouette melt band (0 stays "fog off").
        cfg.render.preference.fog_distance = v.min(config::MAX_FOG_TILES);
    }
    if let Some(v) = args.sky {
        cfg.render.preference.sky = v;
    }
    if let Some(v) = args.reflections {
        cfg.render.preference.reflections = v;
    }
    if let Some(v) = args.light_sources {
        cfg.render.preference.light_sources = v;
    }
    if let Some(v) = args.vsync {
        cfg.render.preference.vsync = v;
    }
    if let Some(v) = args.movies {
        cfg.render.preference.movies = v;
    }
    if let Some(v) = args.anti_aliasing {
        cfg.render.preference.anti_aliasing = v;
    }
    if let Some(v) = args.movie_subtitles {
        cfg.render.preference.movie_subtitles = v;
    }
    if let Some(v) = args.fullscreen {
        cfg.render.preference.fullscreen = v;
    }
    if let Some(v) = args.fps {
        cfg.render.debug.fps = v;
    }
    if let Some(v) = args.prune_owned_jars {
        cfg.gameplay.enhancement.prune_owned_jars = v;
    }
    if let Some(v) = args.wheel_spells {
        cfg.gameplay.enhancement.wheel_spells = v;
    }
    if let Some(v) = args.dev_spells {
        cfg.gameplay.cheat.dev_spells = v;
    }
    if let Some(v) = args.invincible {
        cfg.gameplay.cheat.invincible = v;
    }
    if let Some(v) = args.plausible_spellbook {
        cfg.dev.plausible_spellbook = v;
    }
    // The entity pool is an OFFLINE parameter: CLI wins over config, and
    // the effective value is reflected back for the summary. The config
    // path applies the CLI's 2..=60000 guard too — slot indices are u16,
    // and an unvalidated 70000 would silently truncate the free stack.
    let pool_slots = args
        .pool_slots
        .or(cfg.sim.parameters.entity_pool_size.map(|n| n as usize));
    if let Some(n) = pool_slots
        && !(2..=60000).contains(&n)
    {
        eprintln!("error: sim.parameters.entity_pool_size must be in 2..=60000 (slots are u16)");
        return std::process::ExitCode::FAILURE;
    }
    cfg.sim.parameters.entity_pool_size = pool_slots.map(|n| n as u32);
    // Same offline pattern for the wake radius: CLI wins over config,
    // effective value reflected back for the summary.
    let awake_range = args.awake_range.or(cfg.sim.parameters.awake_range);
    cfg.sim.parameters.awake_range = awake_range;

    // Campaign mode: open (or start) the slot's retail-format save
    // and route the launch to the campaign's current level. The
    // plausible-spellbook instrument yields to the real carry.
    let mut level_path = args.level.clone();
    let mut campaign_run = None;
    if let Some(id) = args.campaign {
        let max_slots = match id {
            campaign::CampaignId::Mc2 => saves::MC2_SLOTS,
            _ => saves::MC1_SLOTS,
        };
        if let Some(s) = args.slot
            && s >= max_slots
        {
            eprintln!(
                "error: --slot {} out of range for {} ({} slots)",
                s + 1,
                id.tag(),
                max_slots
            );
            return std::process::ExitCode::from(2);
        }
        match CampaignRun::start(id, args.slot, args.new_game) {
            Ok(run) => {
                level_path = run.level_path(run.current);
                campaign_run = Some(run);
            }
            Err(e) => {
                eprintln!("error: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
        if cfg.dev.plausible_spellbook {
            println!("campaign: plausible_spellbook off — the real campaign carry replaces it");
            cfg.dev.plausible_spellbook = false;
        }
    }

    // In-app replay (docs/RECORDING.md "Consumers"): the take's
    // header picks the level; the session boots single-level with the
    // fidelity-relevant knobs pinned — a retail take demands the
    // faithful tiers and no instruments, a port take pins its own
    // recorded sim closure.
    let mut replay_boot: Option<replay::ReplayFile> = None;
    if let Some(path) = args.replay.as_ref().or(args.replay_check.as_ref()) {
        if args.record.is_some() {
            eprintln!("error: --record cannot run under --replay (the take is the input source)");
            return std::process::ExitCode::from(2);
        }
        let file = match replay::ReplayFile::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("error: {}: {e}", path.display());
                return std::process::ExitCode::FAILURE;
            }
        };
        level_path =
            get_baked_directory().join(format!("{}/level-{:03}.mgcl", file.game, file.level));
        campaign_run = None;
        match file.source {
            replay::ReplaySource::Retail => {
                cfg.controls.models.thrust = config::ThrustModel::Classic;
                cfg.controls.models.altitude = config::AltitudeModel::Classic;
            }
            replay::ReplaySource::Port => {
                let (thrust, altitude) = match file.models() {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("error: {}: {e}", path.display());
                        return std::process::ExitCode::FAILURE;
                    }
                };
                cfg.controls.models.thrust = match thrust {
                    mgc_sim::ThrustModel::Mc1 => config::ThrustModel::Classic,
                    mgc_sim::ThrustModel::Enhanced => config::ThrustModel::Enhanced,
                };
                cfg.controls.models.altitude = match altitude {
                    mgc_sim::AltitudeModel::Faithful => config::AltitudeModel::Classic,
                    mgc_sim::AltitudeModel::ExtendedLift => config::AltitudeModel::Enhanced,
                };
            }
        }
        cfg.gameplay.cheat.dev_spells = false;
        cfg.gameplay.cheat.invincible = false;
        cfg.dev.plausible_spellbook = false;
        cfg.gameplay.enhancement.prune_owned_jars = false;
        cfg.sim.parameters.entity_pool_size = None;
        cfg.sim.parameters.awake_range = None;
        // The retail-bug patches pin to the take's recorded policy:
        // retail-source and post-policy port takes run the retail
        // arms; a pre-option port take replays under the legacy
        // hard-wired set it was recorded against.
        cfg.gameplay.patches = match file.patch_policy() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {}: {e}", path.display());
                return std::process::ExitCode::FAILURE;
            }
        };
        println!(
            "replay: {} — game {}, level {}, source {}",
            path.display(),
            file.game,
            file.level,
            match file.source {
                replay::ReplaySource::Retail => "retail (inline input recovery)",
                replay::ReplaySource::Port => "port (exact input channel)",
            }
        );
        replay_boot = Some(file);
    }
    // --record: a recording is a conformance instrument — the whole
    // session runs the retail patch arms so a later replay's pin
    // matches by construction (the port header stamps
    // `"patches": "retail"`). Player-ruled 2026-08-08.
    if args.record.is_some() {
        cfg.gameplay.patches = config::GameplayPatches::retail_all();
        println!("record: retail-bug patches run their RETAIL arms for this session");
    }
    // Re-derive the offline pool params after the replay pin.
    let pool_slots = if replay_boot.is_some() {
        None
    } else {
        pool_slots
    };
    let awake_range = if replay_boot.is_some() {
        None
    } else {
        awake_range
    };

    // First-run / stale-epoch auto-bake: regenerate the baked tree
    // from the original game data before touching it.
    if let Err(e) = bakecheck::ensure_baked(&level_path, cfg.gamedata.as_deref()) {
        eprintln!("error: {e}");
        return std::process::ExitCode::FAILURE;
    }

    // Campaign boots are LEVEL-LESS: the frontend (main menu / world
    // map) is the loader, constructing a gameplay session when the
    // player launches one. Only the headless instruments and single-
    // level mode load a level up front.
    let headless = args.screenshot.is_some()
        || args.map.is_some()
        || args.flock_probe.is_some()
        || args.replay_check.is_some();
    let boot_level = if campaign_run.is_some() && !headless {
        None
    } else {
        match load_level(
            &level_path,
            args.tileset,
            args.terrain_features,
            cfg.dev.plausible_spellbook,
            cfg.gameplay.enhancement.prune_owned_jars,
            pool_slots,
            awake_range,
        ) {
            Ok(l) => Some(l),
            Err(e) => {
                eprintln!("error: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    };

    if args.replay_check.is_some() {
        let mut level = boot_level.expect("headless paths load a level");
        let file = replay_boot.expect("--replay-check opened the take");
        // The headless path bypasses apply_instruments — hand the
        // pinned patch policy (cfg was set by the replay pin block)
        // to the world directly.
        if let Some(w) = level.world.as_mut() {
            w.set_patches(world_patches(&cfg.gameplay.patches));
        }
        return match replay::replay_check(level, file) {
            Ok(true) => std::process::ExitCode::SUCCESS,
            Ok(false) => std::process::ExitCode::FAILURE,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::from(2)
            }
        };
    }

    if let Some(out) = &args.map {
        let level = boot_level.as_ref().expect("headless paths load a level");
        return match run_map(
            level,
            out,
            args.map_scale,
            cfg.render.debug.map_trigger_areas,
        ) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    if let Some(out) = &args.flock_probe {
        let level = boot_level.as_ref().expect("headless paths load a level");
        return match run_flock_probe(
            level,
            out,
            args.probe_ticks,
            args.probe_every,
            &args.probe_pose,
            args.probe_species,
            args.probe_strip,
            &args.probe_dis,
        ) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    if let Some(out) = &args.screenshot {
        let level = boot_level.expect("headless paths load a level");
        return match run_screenshot(
            level,
            out,
            args.camera,
            cfg.render.enhancement.smooth_shading,
            cfg.render.preference.fog_distance,
            cfg.render.preference.sky,
            cfg.render.preference.reflections,
            cfg.render.preference.light_sources,
            args.map_view,
            args.anim_turn,
            cfg.render.debug.map_trigger_areas,
            cfg.gameplay.cheat.dev_spells,
            cfg.render.enhancement.hud_transparency.transparent(),
        ) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    // Spell-selector surfaces, resolved against the loaded game. MC2
    // owns exactly one shape (the CTRL pane) — an explicit map-book
    // request there coerces with a note rather than inventing a
    // 26-spell in-map grid.
    let selector_choice = cfg.gameplay.enhancement.spell_selector;
    // The running game's identity: the boot level's, or (level-less
    // campaign boot) the campaign's.
    let boot_game = boot_level.as_ref().map(|l| l.game).unwrap_or_else(|| {
        match campaign_run.as_ref().map(|c| c.id) {
            Some(campaign::CampaignId::Mc2) => mgc_sim::ids::GameId::Mc2,
            Some(campaign::CampaignId::Mc1Hw) => mgc_sim::ids::GameId::Mc1Hw,
            _ => mgc_sim::ids::GameId::Mc1,
        }
    });
    let level_is_mc2 = matches!(boot_game, mgc_sim::ids::GameId::Mc2);
    let selector = selector_choice.resolve(level_is_mc2);
    if level_is_mc2
        && matches!(
            selector_choice,
            config::SpellSelector::Mc1 | config::SpellSelector::Mc1Mc2
        )
    {
        println!("spell-selector: MC2 has no in-map spellbook — using the faithful CTRL pane");
    }

    println!("mgcarpet {}", env!("CARGO_PKG_VERSION"));
    let move_keys = match cfg.controls.preferences.bindings {
        config::Bindings::Classic => "Up/Down arrows accel/decel, Left/Right strafe",
        config::Bindings::Wasd => "W/S accel/decel, A/D strafe",
    };
    match cfg.controls.models.thrust {
        config::ThrustModel::Classic => println!(
            "controls: classic (faithful) — mouse = stick (offset steers, recenter to fly straight),\n\
             \x20         {move_keys} (impulses: speed persists until countered),"
        ),
        config::ThrustModel::Enhanced => {
            println!(
                "controls: enhanced — mouse points the crosshair, the carpet chases it (banks \
                 with forward speed; casts fire along the crosshair), {move_keys} (hold-to-fly),"
            )
        }
    }
    if cfg.controls.models.altitude == config::AltitudeModel::Enhanced {
        println!(
            "          E/Q set the desired altitude over terrain (the carpet drifts to it; \
             capped 4 tiles up),"
        );
    }
    println!("          Backspace full-stops the carpet (speed + steering; MC2's stabilize key),");
    println!("          Space respawns after death (at your castle; no castle = level restart),");
    println!("          Shift+L demolishes your own castle one level per press,");
    println!("          LMB/RMB cast the equipped hand's spell (hold = channel),");
    if selector.map_book {
        println!("          Enter opens the book: click a spell with LMB/RMB to equip,");
    } else {
        println!("          Enter opens the map screen,");
    }
    if selector.ctrl_pane {
        println!("          hold Ctrl for the spell selector: click LMB/RMB to equip a hand,");
    }
    println!("          hover + 1-9,0 binds a quick key (in flight: equip, Shift = right hand),");
    println!("          Esc twice quits.");

    // The structured options summary: every toggle, its current value,
    // the alternatives (faithful `*`-marked), and how to change it.
    let boot_label = match (&boot_level, &campaign_run) {
        (Some(l), _) => l.label.clone(),
        (None, Some(run)) => format!("{} campaign", run.id.tag()),
        (None, None) => String::new(),
    };
    settings::print_summary(&cfg, boot_game, &boot_label);

    let event_loop = match event_loop {
        Some(el) => el,
        None => match EventLoop::new() {
            Ok(el) => el,
            Err(e) => {
                eprintln!("error: cannot create event loop: {e}");
                return std::process::ExitCode::FAILURE;
            }
        },
    };
    let mut app = App::new(
        boot_level,
        cfg,
        config_path,
        campaign_run,
        LaunchParams {
            tileset: args.tileset,
            terrain_features: args.terrain_features,
            pool_slots,
            awake_range,
        },
        replay_boot,
        args.record.clone(),
    );
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("error: event loop: {e}");
        return std::process::ExitCode::FAILURE;
    }
    // Every exit path funnels here — a live recording gets its zstd
    // frame end so the file reopens cleanly.
    app.finish_recorder();
    std::process::ExitCode::SUCCESS
}

/// Root of the baked data tree — the single seam every baked lookup
/// resolves through, so a port or packaging layout relocates the
/// whole tree by changing one body (the Android shell points it at
/// the device's shared storage; `saves::saves_root` is its sibling).
fn get_baked_directory() -> PathBuf {
    PathBuf::from("baked")
}

#[cfg(test)]
mod ring_tests {
    use super::ring_next;

    // 6-spell fixture: ring = [L, R, L, 0, L, R], all owned.
    const RING: [u8; 6] = [1, 2, 1, 0, 1, 2];

    #[test]
    fn forward_walks_the_side_and_wraps() {
        let owned = [true; 6];
        // From spell 0 forward on the LEFT ring: next member is 2.
        assert_eq!(ring_next(&RING, &owned, 1, 0, false), Some(2));
        // From 4 forward: wraps past 5 back to 0.
        assert_eq!(ring_next(&RING, &owned, 1, 4, false), Some(0));
        // RIGHT ring from 1 forward: 5.
        assert_eq!(ring_next(&RING, &owned, 2, 1, false), Some(5));
    }

    #[test]
    fn backward_is_the_mirror() {
        let owned = [true; 6];
        assert_eq!(ring_next(&RING, &owned, 1, 2, true), Some(0));
        // From 0 backward: wraps to 4.
        assert_eq!(ring_next(&RING, &owned, 1, 0, true), Some(4));
    }

    #[test]
    fn empty_hand_starts_at_the_ends() {
        let owned = [true; 6];
        // -1 forward starts at slot 0 (retail -1+1).
        assert_eq!(ring_next(&RING, &owned, 1, -1, false), Some(0));
        // -1 backward wraps straight to the tail (retail -1-1 -> 25).
        assert_eq!(ring_next(&RING, &owned, 2, -1, true), Some(5));
    }

    #[test]
    fn unavailable_members_are_skipped_not_dropped() {
        // Spell 2 lost (MC1 jar vanish / MC2 undead steal): still a
        // member, but the walk passes over it.
        let mut owned = [true; 6];
        owned[2] = false;
        assert_eq!(ring_next(&RING, &owned, 1, 0, false), Some(4));
    }

    #[test]
    fn all_unavailable_does_nothing() {
        // Ring populated but nothing possessed -> None (the special
        // case: cycling must be a no-op, not an unbind).
        let owned = [false; 6];
        assert_eq!(ring_next(&RING, &owned, 1, 0, false), None);
        // No members on this side at all -> None too.
        assert_eq!(ring_next(&[0u8; 6], &[true; 6], 1, 0, false), None);
    }

    #[test]
    fn single_member_ring_reselects_itself() {
        let mut ring = [0u8; 6];
        ring[3] = 1;
        let owned = [true; 6];
        // The equipped spell is the last candidate of the lap
        // (retail PI:1881 breaks on it after 26 steps).
        assert_eq!(ring_next(&ring, &owned, 1, 3, false), Some(3));
        assert_eq!(ring_next(&ring, &owned, 1, 3, true), Some(3));
    }
}
