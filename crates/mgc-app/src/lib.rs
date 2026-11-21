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

mod autofire;
mod bakecheck;
mod camera;
mod campaign;
mod config;
mod entities;
mod film;
mod frontend;
mod frontend_mc1;
mod launcher_data;
mod launcher_screen;
mod lobby;
mod lobby_mc1;
mod lobby_mc2;
mod menu;
mod minimenu;
mod movie;
mod net;
mod paths;
mod replay;
mod rules;
mod saves;
mod settings;
mod sight;
mod sprite_export;
mod stats_screen;
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

/// The human seat this app presents and drives — one local human:
/// `Hx::FIRST` everywhere but a netplay client (`--join`), which flies
/// the seat the host assigned it ([`net`]). Set once, before any world.
static LOCAL_SEAT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub(crate) fn local_hx() -> mgc_sim::Hx {
    mgc_sim::Hx(LOCAL_SEAT.load(std::sync::atomic::Ordering::Relaxed))
}

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
    /// The human's level-start book (spell ids, any order): the
    /// campaign carry ∩ the level's availability mask (retail's
    /// :49226-33 grant law) ∪ the plausible-spellbook instrument.
    /// Granted BEFORE the rivals — retail's first-tick order is the
    /// carpet record, the book's tokens, then the rivals (`sub_44D30`
    /// for wizard 0..7), and every slot-seeded law downstream reads
    /// the layout (round 154). MC1 column only.
    human_book: Vec<u8>,
    /// Rival wizard configs by player slot (wizards.json) + the
    /// level's active-slot count. MC1 column only.
    wizards: [Option<mgc_sim::mc1::rivals::RivalConfig>; 8],
    /// MC2 rival configs by color (wizards.json MC2 shape + the
    /// header's authored castle levels); `player_count` doubles as
    /// the NumberOfPlayers bound (header unk09) on MC2.
    mc2_wizards: [Option<mgc_sim::mc2::rivals::Mc2RivalConfig>; 8],
    player_count: u16,
    /// The wizard seat the human flies (`--seat`, 0-based; retail's
    /// local index) — 0 everywhere but a `--seat` launch, which makes
    /// seat 0 a rival (`World::new_full_env_seated`).
    human_seat: u8,
    /// Netplay: the further human seats' wizard colours, seated before
    /// the roster ([`World::add_human_seat`]) — every peer builds the
    /// same multi-human world. Empty outside netplay.
    extra_seats: Vec<u8>,
    /// A network game's AI wizard count (`load_level_with`); `None` =
    /// the level's authored roster.
    net_ai: Option<u8>,
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
    /// MC2 castle-purge level bit (`byte_0x2FED2 & 4`): gates
    /// `sub_605E0`'s level-0 rival spell-2 book purge (EF:61650).
    /// Set on levels 022 and 062 only.
    castle_purge_level: bool,
    /// The MC2 campaign REPLAY gate (`setting_38545 & 4`): the level
    /// was launched by a map click on a portal the save has already
    /// completed ([`mc2_map_launch_replayed`]). Kept here so a
    /// restart rebuilds the same gate.
    mc2_replayed: bool,
    /// Draw stand-in art for unported models (deliberate: MC2 default
    /// until its roster closes; the ledger stays truthful either way).
    placeholders: bool,
    /// The chassis constant set: the game's pristine profile, or a
    /// deliberately deviating one (the limit-removing `--pool-slots`
    /// dev flag; G-class — a run under a bumped pool is not a
    /// faithful fixture).
    chassis: mgc_sim::chassis::ChassisParams,
}

impl WorldInit {
    fn build(&self) -> mgc_sim::engine::world::World {
        let mut w = mgc_sim::engine::world::World::new_full_env_seated(
            self.planes.clone(),
            &self.things,
            self.seed,
            self.assets.clone(),
            self.chassis.clone(),
            self.game,
            self.night_shade,
            self.human_seat,
        );
        for &seat in &self.extra_seats {
            w.add_human_seat(seat)
                .expect("load_level validated the netplay seats");
        }
        if matches!(self.game, mgc_sim::ids::GameId::Mc2) {
            w.set_placeholders(self.placeholders);
            w.set_mc2_night_shade(self.night_shade);
            w.set_mc2_doom_level(self.doom_level);
            w.set_mc2_castle_purge_level(self.castle_purge_level);
            w.set_mc2_level_replayed(self.mc2_replayed);
            if !self.stages.is_empty() {
                w.set_mc2_stages(&self.stages);
            }
            if !self.stage_vars.is_empty() {
                w.set_mc2_stagevars(&self.stage_vars);
            }
            match self.net_ai {
                Some(ai) => w.seat_mc2_net(&self.mc2_wizards, ai),
                None => w.seat_mc2_wizards(&self.mc2_wizards, self.player_count),
            }
        } else {
            if self.win_pct > 0 {
                w.set_win_pct(self.win_pct);
            }
            // Retail's consume-loop order (:48633): wizard 0..count,
            // the human's record + book at his seat, a rival at every
            // other — see `seat_mc1_wizards`.
            match self.net_ai {
                Some(ai) => w.seat_mc1_net(&self.human_book, &self.wizards, ai),
                None => w.seat_mc1_wizards(&self.human_book, &self.wizards, self.player_count),
            }
        }
        w
    }
}

/// The parameters the initial `load_level` ran with, kept so a
/// campaign level switch rebuilds through the exact same path.
struct LaunchParams {
    /// Boot to the launcher (a plain start — see `launcher_screen`).
    launcher: bool,
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
/// carry. The record IS the state — completing a level updates it in
/// memory; nothing reaches disk until the player saves (player ruling
/// 2026-09-11: no autosave at the won edge, either game).
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
    /// MC1/HW quick-key carry (native-only sidecar, like the ring;
    /// `SaveHeader::mc1_quick_keys`). `None` = no assignment carried —
    /// a new campaign or a save from before the carry — and the level
    /// seeds its keys from scratch like retail (:49216-59); `Some` =
    /// the player's binds, installed at level entry and refreshed at
    /// the won edge. App-side only: the keys never reach the sim, so
    /// replays and the sim hash are untouched.
    mc1_quick_keys: Option<[Option<u8>; 10]>,
    /// MC2: the levels whose stats tables the map shows on its next
    /// entry, oldest first (retail map mode 4). A win routed to the
    /// map queues its level. A win routed STRAIGHT into a secret level
    /// (the alternate portal) queues it too — retail never showed that
    /// table — so the map later shows both, one key each.
    map_stats: Vec<u32>,
}

/// The slot's campaign record, native file first: the retail-format
/// bytes plus the native header's carry — the MC1 cycle ring and
/// quick keys, the MC2 owed stats tables. The carry rides only the
/// native container (`None` from a retail import or a version-recovery
/// — those simply start with an empty ring and fresh quick keys).
///
/// Returns `None` for "start fresh" — either `new_game`, or nothing
/// on disk. A file that EXISTS but cannot be read is an error, not a
/// fresh start: silently starting over would overwrite it on the
/// first level completion.
/// The native carry beside the retail record (see [`campaign_record`]).
struct SlotRecord {
    /// The retail-format record bytes.
    campaign: Vec<u8>,
    mc1_ring: Option<[u8; 24]>,
    mc1_quick_keys: Option<[Option<u8>; 10]>,
    mc2_map_stats: Vec<u32>,
}

impl SlotRecord {
    /// A record with no native carry (a retail import, a recovery).
    fn bare(campaign: Vec<u8>) -> Self {
        Self {
            campaign,
            mc1_ring: None,
            mc1_quick_keys: None,
            mc2_map_stats: Vec::new(),
        }
    }
}

/// `MGC_NO_REQUEST_TIME_LISTENER=1` restores the pre-fix drain-time
/// listener: sound requests are attenuated against the pose the sim
/// step ENDED at, so a portal warp's own entry sound (id 22) is
/// measured from the destination and culled past 12288 units.
fn request_time_listener() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !*V.get_or_init(|| std::env::var_os("MGC_NO_REQUEST_TIME_LISTENER").is_some())
}

fn campaign_record(tag: &str, slot: usize, new_game: bool) -> Result<Option<SlotRecord>, String> {
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
                return Ok(Some(SlotRecord {
                    campaign: pkg.campaign,
                    mc1_ring: pkg.header.mc1_spell_ring,
                    mc1_quick_keys: pkg.header.mc1_quick_keys,
                    mc2_map_stats: pkg.header.mc2_map_stats,
                }));
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
                return Ok(Some(SlotRecord::bare(rec.campaign)));
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
            .map(|b| Some(SlotRecord::bare(b)))
            .map_err(|e| format!("{}: {e}", retail.display()));
    }
    Ok(None)
}

/// The MC2 level a save is waiting on: a revealed-but-uncompleted
/// secret takes precedence (the player is mid-branch), otherwise the
/// linear frontier. `levels_completed` counts opened portals, so it
/// indexes the first unopened one — 25 once the finale is won, which
/// names no level: the campaign is complete and the map is a free-play
/// hub (every launch from it is a portal click, never this value).
///
/// One rule, applied both when a slot is opened and whenever the run
/// returns to the map — a slot saved from the map must name the same
/// level before and after a reload.
/// The MC2 campaign REPLAY gate for a world-map launch
/// (`setting_38545 & 4`, MenusAndIntros.cpp:3371-72 / :3395-96): the
/// clicked MAIN portal is completed (`activated_18 == 1`, i.e. one of
/// the first `levels_completed`, :1553-58) or the clicked SECRET portal
/// is (`activated_12 == 1`). Under it the level's XP scrolls hide and
/// soft-kill and no spell XP accrues.
///
/// Only the map raises it. The in-level secret dive (the parent's
/// portal, action 11) CLEARS the bit (`&= 0xFB`, EF:60802) and the main
/// loop loads the secret with no map pass (EF:31390-31425), so a
/// completed secret re-entered that way is ungated in retail too.
fn mc2_map_launch_replayed(save: &saves::Mc2Save, level: u32) -> bool {
    if level < 25 {
        level < save.levels_completed
    } else {
        save.secrets
            .iter()
            .any(|p| p.level as u32 == level && p.activated == 1)
    }
}

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
        let mc1_ring = record.as_ref().and_then(|r| r.mc1_ring).unwrap_or([0; 24]);
        let mc1_quick_keys = record.as_ref().and_then(|r| r.mc1_quick_keys);
        let map_stats = record
            .as_ref()
            .map(|r| r.mc2_map_stats.clone())
            .unwrap_or_default();
        let record = record.map(|r| r.campaign);
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
                    mc1_quick_keys,
                    map_stats: Vec::new(),
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
                // A completed campaign (25 portals opened) loads like
                // any other — retail's load arm has no such check, and
                // the map is the free-play hub past the finale.
                if save.levels_completed >= 25 {
                    println!("campaign mc2: the campaign is complete — free play");
                }
                let current = mc2_pending_level(&save);
                Ok(Self {
                    id,
                    slot,
                    current,
                    save: CampaignSave::Mc2(save),
                    next: None,
                    mc1_ring: [0; 24],
                    mc1_quick_keys: None,
                    map_stats,
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
        // The MC1 quick keys likewise (omitted while unbound).
        if self.id != campaign::CampaignId::Mc2 {
            header.mc1_quick_keys = self.mc1_quick_keys;
        }
        // MC2: tables still owed (a secret level's parent, saved from
        // inside the secret level).
        header.mc2_map_stats = self.map_stats.clone();
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
    /// This is the HUB write (menu / world-map Save): campaign state
    /// only, so it clears any world payload the slot was carrying.
    /// It is never called on the player's behalf — completing a level
    /// does NOT write the slot (player ruling 2026-09-11); a mid-level
    /// save of the level just won stays exactly as taken until the
    /// player overwrites it.
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
        Some(pct) => label.push_str(&format!("  L{} {pct}%", info.level_number())),
        None => label.push_str(&format!("  L{}", info.level_number())),
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

/// The HUD bars' team colour pair for the human flying wizard `seat`
/// (`ui::hud_quads`, which takes the pip colour from it): MC1/HW's
/// `byte_99B58` pair, MC2's `playersColors_E88E0x` per map type —
/// retail's `DrawSpellIcon_2E260` reads `[GetTrueWizardNumber(colour)]
/// [0]` for the pips and `[1]` for the bar (GameUI.cpp), and the port's
/// seat IS the true wizard. (MC2 was excluded here, so every MC2 seat
/// drew white pips — unfaithful; fixed 2026-11-15.) Seat 0 — every
/// launch but `--seat` / netplay — keeps the HUD's tuned white
/// (`None`).
fn hud_team_colors(seat: u8, level: &LoadedLevel) -> Option<([f32; 4], [f32; 4])> {
    if seat == 0 {
        return None;
    }
    let colors = entities::roster_team_colors(level.game, level.mc2_env, &level.palette_rgba);
    Some(colors[(seat as usize).min(7)])
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
    // Seat 0 too: it is a rival whenever the human flies another seat
    // (`--seat`); `set_wizards` skips the human's own.
    for (slot, cfg) in w.wizards.iter().enumerate().take(8) {
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
    // Colour 0 too: it is a rival whenever the human flies another
    // (`--seat`); `set_mc2_wizards` skips the human's own.
    for (slot, cfg) in w.wizards.iter().enumerate().take(8) {
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
    /// The bundle's retail sprite bank (index, atlas pixels): the
    /// sim-side view of the art — sizes and draw types.
    sprites: Option<(mgc_formats::bundle::SpriteIndex, Vec<u8>)>,
    /// The bundle's world-sprite impostor pack: what the renderer
    /// draws (`entities::level_impostors`).
    impostors: Option<mgc_formats::impostor::ImpostorPack>,
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
    /// MC2's minimap colour table for this map type (`clrd.bin`);
    /// `None` off MC2 or on an unbaked bundle.
    mc2_clrd: Option<Vec<u8>>,
    /// The MC2 map-marker environment (team-colour table + map-type
    /// colours) from the level header; Day for MC1/HW.
    mc2_env: entities::Mc2MapEnv,
    /// The per-game audio bundle directory (`assets/mc1-audio` /
    /// `mc2-audio`), when baked.
    audio_dir: Option<PathBuf>,
    /// ⭐ THE SAMPLE BANK, BY MAP TYPE. `LevelInit.cpp:23/29/37`
    /// calls `LoadSounds_84300(0/1/2)` for Day/Night/Cave: MC2's
    /// CAVE bank is the only one that carries the drip samples
    /// 65-69 (`baked/assets/mc2-audio/sounds.json`: banks 0 and 1
    /// stop at 64). The app used to hardcode bank 0, so every
    /// `EntitySounds` drip request `mc2/cave.rs` made on a cave
    /// level reached the mixer, took its request slot and then
    /// found NO SAMPLE — silent. MC1 has one gameplay bank (0; its
    /// 1-13 are the cutscene banks the movie player selects).
    audio_bank: u32,
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
    /// The trigger areas Beyond Sight last revealed (the map ripple),
    /// each with its own fade; refreshed per tick while the spell is
    /// up, kept through the fade-out after it lapses.
    sight: sight::SightAreas,
    /// Castle/balloon icon patches for the map marker pass.
    map_icons: entities::MapIcons,
    /// Live icon stamps (own castle/balloons), refreshed per tick.
    map_stamps: Vec<mgc_render::MapStamp>,
    /// MC2 objective-guide targets (blinking marks + steer arrow),
    /// refreshed per tick from the current objective. Empty off-MC2.
    objective_marks: Vec<mgc_render::ObjectiveMark>,
    /// The MC2 plausible-spellbook grants: `(spell, banked_xp)` per
    /// learned spell (MC2's book is XP-driven). Empty off-MC2 or when
    /// the instrument is off. Installed via `mc2_grant_plausible`.
    /// (The MC1 set is `WorldInit::human_book` — granted by the world
    /// build, before the rivals.)
    plausible_book_mc2: Vec<(u8, i32)>,
    /// MC2: the level's `BlockedSpells` row for the HUMAN seat (wizard
    /// slot 0) — the carried spells retail's level-start grant
    /// withholds (`apply_campaign_book`). All false off-MC2.
    mc2_human_blocked: [bool; 26],
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

/// The trigger areas Beyond Sight reveals on the map surfaces
/// (`render.enhancement.map_beyond_sight_areas`): the sim's fly-into
/// volumes and their trap / effector verdict, for the renderer's
/// ripple.
fn sight_areas(world: &mgc_sim::engine::world::World) -> Vec<mgc_render::SightArea> {
    world
        .sight_areas()
        .into_iter()
        .map(|a| mgc_render::SightArea {
            x: a.x,
            z: a.z,
            radius: a.radius,
            trap: a.trap,
            weight: 1.0,
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
            Some(entities::SwapFamily::Grave) => &mut level.map_icons.grave_icons,
            Some(entities::SwapFamily::Portal) => &mut level.map_icons.portal_icons,
            None => continue,
        };
        if bucket.contains_key(&p.type_index) {
            continue;
        }
        let Some(sprite) = entities::pose_sprite_id(level.game, p.type_index) else {
            continue;
        };
        // The STANDING STONE (2,1) in both games stays at half the
        // rule: stone circles and clusters dominated the map at full
        // size (player ruling 2026-09-09). ⚠ NAMING: the player calls
        // these stones "dolmens"; the code and the decompile call (2,2)
        // — the regen SHRINE (`AddDolmen_4ADF0`) — the dolmen, and
        // (2,1) the stone (`AddStone_4AD70`). The shrine keeps the
        // full rule. On top of that, the stones and MC1's statues
        // (models 1/3) shrink by `STONE_STATUE_SHRINK` (player ask
        // 2026-10-17: "20-30%" smaller); the shrine stays as it was.
        const STONE_STATUE_SHRINK: f32 = 0.75;
        let scale = match (p.class, p.model) {
            (2, 1) => 0.5 * STONE_STATUE_SHRINK,
            (2, 3) => STONE_STATUE_SHRINK,
            _ => 1.0,
        };
        if let Some(stamp) = ui.append_world_icon(sidx, spx, sprite, &level.palette_rgba, scale) {
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
    set.extend(level.map_icons.grave_icons.keys().copied());
    set.extend(level.map_icons.portal_icons.keys().copied());
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
///
/// `campaign_carry` = the MC1/HW campaign's collected spells (ids,
/// unmasked — [`mc1_campaign_carry`]); the level's availability mask
/// is applied here and the result is the world build's `human_book`,
/// granted before the rivals. The book has to be known at BUILD time:
/// the load-time fires/painters/craters draw off pool slot numbers,
/// and the human's tokens sit between the level's records and the
/// rivals' (round 154). Empty outside a campaign.
/// The level-start spellbook seed a single-level launch asks for.
/// Campaign launches always use [`SpellbookSeed::Level`] — the real
/// carry replaces every instrument.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum SpellbookSeed {
    /// Only what the level itself grants (plus the campaign carry).
    #[default]
    Level,
    /// `--plausible-spellbook`: the campaign-derived union.
    Plausible,
    /// `--set-spellbook`: an explicit `(spell, xp)` list — the CLI
    /// parse checked the syntax, `load_level` checks the ids against
    /// the loaded game's table. Trumps `Plausible`. Strictly CLI: no
    /// config key, no settings row, not recorded anywhere but the
    /// take's own book pin.
    Set(Vec<(u8, i32)>),
}

/// Spell ids by game — the table `--set-spellbook` is checked against
/// and the one the error text prints (the ids are internal).
fn set_spellbook_table() -> String {
    use std::fmt::Write as _;
    let mut s = String::from(
        "--set-spellbook takes a comma-separated list of in-level spell ids, e.g. \
         `--set-spellbook 0,4,7`. MC2 entries may carry banked experience as `id:xp` \
         (default 0; a campaign scroll banks 4 XP into every owned spell, and the \
         level's SPELLS `xpos1` ladder turns the bank into the tier). Ids by game:\n\
         \x20 MC1 / HW (0..23):\n",
    );
    for i in 0..mgc_sim::mc1::spells::SPELL_COUNT {
        let _ = writeln!(
            s,
            "    {i:2}  {}",
            mgc_sim::mc1::spells::SpellId(i as u8).name()
        );
    }
    s.push_str("  MC2 (0..25):\n");
    for (i, n) in ui::MC2_SPELL_NAMES.iter().enumerate() {
        let _ = writeln!(s, "    {i:2}  {n}");
    }
    s
}

/// Parse the `--set-spellbook` value. Any syntax slip — including a
/// deliberate `--set-spellbook list` — fails WITH the id table, which
/// is how a knowledgeable person looks the ids up.
fn parse_set_spellbook(spec: &str) -> Result<Vec<(u8, i32)>, String> {
    let bad = |what: &str| format!("--set-spellbook: {what}\n{}", set_spellbook_table());
    let mut out: Vec<(u8, i32)> = Vec::new();
    for item in spec.split(',').map(str::trim) {
        if item.is_empty() {
            return Err(bad("empty entry"));
        }
        let (id, xp) = match item.split_once(':') {
            Some((id, xp)) => (
                id.trim(),
                xp.trim()
                    .parse::<i32>()
                    .ok()
                    .filter(|&x| x >= 0)
                    .ok_or_else(|| bad(&format!("`{item}`: xp must be a non-negative integer")))?,
            ),
            None => (item, 0),
        };
        let id: u8 = id
            .parse()
            .map_err(|_| bad(&format!("`{item}` is not a spell id")))?;
        // 26 = the wider (MC2) table; the game-specific bound is
        // checked at level load, where the game is known.
        if id as usize >= ui::MC2_SPELL_NAMES.len() {
            return Err(bad(&format!("spell id {id} is out of range")));
        }
        if let Some(e) = out.iter_mut().find(|e| e.0 == id) {
            e.1 = xp; // a repeated id keeps the last xp
        } else {
            out.push((id, xp));
        }
    }
    if out.is_empty() {
        return Err(bad("no spells given"));
    }
    Ok(out)
}

/// The per-game half of the `--set-spellbook` check: MC1/HW has 24
/// spells, MC2 26; `:xp` is MC2-only (MC1's book carries no XP).
fn check_set_spellbook(game: Game, ids: &[(u8, i32)]) -> Result<(), String> {
    let (limit, label) = match game {
        Game::MagicCarpet2 => (ui::MC2_SPELL_NAMES.len(), "MC2"),
        _ => (mgc_sim::mc1::spells::SPELL_COUNT, "MC1 / HW"),
    };
    if let Some(&(id, _)) = ids.iter().find(|&&(id, _)| id as usize >= limit) {
        return Err(format!(
            "--set-spellbook: spell id {id} does not exist in {label} (0..{})\n{}",
            limit - 1,
            set_spellbook_table()
        ));
    }
    if game != Game::MagicCarpet2 && ids.iter().any(|&(_, xp)| xp != 0) {
        return Err(format!(
            "--set-spellbook: `id:xp` is MC2-only — {label} spells carry no experience\n{}",
            set_spellbook_table()
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn load_level(
    level_path: &Path,
    tileset: Option<u8>,
    terrain_features: bool,
    book: &SpellbookSeed,
    campaign_carry: &[u8],
    pool_slots: Option<usize>,
    awake_range: Option<u32>,
    mc2_replayed: bool,
    seats: &[u8],
) -> Result<LoadedLevel, String> {
    load_level_with(
        level_path,
        tileset,
        terrain_features,
        book,
        campaign_carry,
        pool_slots,
        awake_range,
        mc2_replayed,
        seats,
        None,
    )
}

/// [`load_level`] for a NETWORK game: `net_ai` = the session's AI
/// wizard count — the roster seats every human across all eight
/// colours and that many rivals in the first free authored ones
/// (`World::seat_mc*_net`); `None` = the level's authored roster.
#[allow(clippy::too_many_arguments)]
fn load_level_with(
    level_path: &Path,
    tileset: Option<u8>,
    terrain_features: bool,
    book: &SpellbookSeed,
    campaign_carry: &[u8],
    pool_slots: Option<usize>,
    awake_range: Option<u32>,
    mc2_replayed: bool,
    seats: &[u8],
    net_ai: Option<u8>,
) -> Result<LoadedLevel, String> {
    // The human seats' wizard colours in seat order: `seats[0]` is the
    // world's first human (`--seat`, 0 everywhere else); netplay seats
    // the others too, so every peer builds the same multi-human world.
    let (human_seat, extra_seats) = seats.split_first().ok_or("load_level: no human seat")?;
    let human_seat = *human_seat;
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

    // MC1 arm — and HW, which shares the spellbook system wholesale
    // (same jar class, same per-level availability mask); only the
    // campaign SHAPE differs (25 levels, no skip table), which
    // `campaign::plausible_spellbook` resolves per game.
    let mc1_family = matches!(package.meta.game, Game::MagicCarpet1 | Game::HiddenWorlds);
    // `--set-spellbook`: the explicit override, checked against THIS
    // game's table now that the game is known (the CLI parse only
    // checked the syntax).
    let set_book: Option<&[(u8, i32)]> = match book {
        SpellbookSeed::Set(ids) => {
            check_set_spellbook(package.meta.game, ids)?;
            Some(ids)
        }
        _ => None,
    };
    let plausible_spells = if *book == SpellbookSeed::Plausible && mc1_family {
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

    // `--seat`: the level must seat it — MC1's wizard-table count,
    // MC2's header NumberOfPlayers (the single-player bound; a network
    // game seats the joined count instead).
    for &seat in seats {
        if seat == 0 {
            continue;
        }
        let count = if mc1_family {
            package
                .wizards
                .as_ref()
                .and_then(|w| w.player_count)
                .unwrap_or(1)
                .min(8)
        } else {
            package
                .header
                .as_ref()
                .map_or(1, |h| h.number_of_players.clamp(1, 8) as u16)
        };
        // A network game (several human seats) seats the JOINED count
        // instead (retail `NumberOfPlayers_0xe` = the connected count,
        // EF lobby hand-off) — five of MC2's ten arenas say 1 here.
        if seats.len() == 1 && u16::from(seat) >= count {
            return Err(format!(
                "--seat {}: this level seats {count} wizard(s)",
                seat + 1
            ));
        }
        // …and must AUTHOR the seat: its class-3 `4 + seat` start
        // marker. Retail would seat a marker-less wizard at the memset
        // origin (the round-154 law), which no player wants to fly from.
        let game = mgc_sim::ids::GameId::from(package.meta.game);
        if entities::player_start_seat(game, &package.things.things, seat).is_none() {
            return Err(format!(
                "--seat {}: this level authors no start marker for that seat",
                seat + 1
            ));
        }
    }
    // The human's level-start book: the campaign carry ∩ the level's
    // availability mask (retail's grant law, :49226-33 — the mask is
    // the HUMAN's seat row's `allowed_spells`, 0/1 per spell; retail
    // single-player indexes the table by the wizard, :49201-55) ∪ the
    // plausible instrument's set. Order is the sim's
    // (`grant_level_book` walks the book order).
    let allowed_spells: Option<Vec<u8>> = package
        .wizards
        .as_ref()
        .and_then(|w| w.wizards.get(human_seat as usize))
        .and_then(|h| h.allowed_spells.clone());
    let human_book: Vec<u8> = match set_book.filter(|_| mc1_family) {
        // `--set-spellbook`: EXACTLY the given set — it replaces the
        // carry and the plausible union, and it is NOT masked by the
        // level's availability list (a debug override for levels
        // whose authored grant is not what the tester needs: hidden,
        // experimental, arena). The mask is reported, not applied.
        Some(ids) => {
            let book: Vec<u8> = ids.iter().map(|&(s, _)| s).collect();
            let name = |&s: &u8| mgc_sim::mc1::spells::SpellId(s).name();
            let masked: Vec<&str> = book
                .iter()
                .filter(|&&s| {
                    allowed_spells
                        .as_ref()
                        .is_some_and(|m| m.get(s as usize).is_some_and(|&v| v != 1))
                })
                .map(name)
                .collect();
            println!(
                "set-spellbook: {} spell(s) [{}]{}",
                book.len(),
                book.iter().map(name).collect::<Vec<_>>().join(", "),
                if masked.is_empty() {
                    String::new()
                } else {
                    format!(
                        " (the level's mask would strip {} — granted anyway)",
                        masked.join(", ")
                    )
                },
            );
            book
        }
        // A NETWORK game: retail's networked arm (:49200-04, :49237-40)
        // grants every human the level's wizard ROW 0 —
        // `starting_spells ∧ allowed_spells` — with no campaign carry
        // (an arena's own start: Bussorah = Fireball, Possess, Create
        // Castle). The plausible seed is a campaign aid, not a rule.
        None if net_ai.is_some() && mc1_family => package
            .wizards
            .as_ref()
            .and_then(|w| w.wizards.first())
            .map(|row0| {
                (0..row0.starting_spells.len() as u8)
                    .filter(|&s| {
                        row0.starting_spells[s as usize] != 0
                            && row0
                                .allowed_spells
                                .as_ref()
                                .is_none_or(|m| m.get(s as usize).is_some_and(|&v| v == 1))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        None => {
            let mut book: Vec<u8> = campaign_carry
                .iter()
                .copied()
                .filter(|&s| {
                    allowed_spells
                        .as_ref()
                        .is_none_or(|m| m.get(s as usize).is_none_or(|&v| v == 1))
                })
                .collect();
            for &s in &plausible_spells {
                if !book.contains(&s) {
                    book.push(s);
                }
            }
            book
        }
    };

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
                // MC1 / Hidden Worlds: the sprite-stat widths come off
                // the level's own sprite sheet (remc1 :66699) — the
                // arctic bank derives four rows unlike the temperate.
                if !is_mc2 && let Some(dims) = bundle.sprite_dims() {
                    assets = assets.with_mc1_sprite_ext(mgc_sim::mc1::derive_sprite_stats(&dims));
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
                if let Some(n) = pool_slots
                    && n != chassis.pool_slots
                {
                    println!(
                        "chassis: pool_slots {n} (retail {}; G-class — not a \
                         faithful run)",
                        chassis.pool_slots
                    );
                    chassis.pool_slots = n;
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
                    human_book: human_book.clone(),
                    wizards,
                    mc2_wizards,
                    player_count,
                    human_seat,
                    extra_seats: extra_seats.to_vec(),
                    net_ai,
                    stages,
                    stage_vars,
                    placeholders: is_mc2,
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
                    // The castle-purge bit (gfx_type & 4) gates
                    // `sub_605E0`'s level-0 rival arm (remc2 EF:61650).
                    castle_purge_level: is_mc2
                        && package.header.as_ref().is_some_and(|h| h.gfx_type & 4 != 0),
                    mc2_replayed: is_mc2 && mc2_replayed,
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
                let poses = w.live_poses(local_hx());
                (
                    // Load-time set: no fire exists at level start, so
                    // the enhanced-fire sprite suppression is moot here,
                    // and the dweller-invisibility patch flag likewise
                    // (sync_world re-derives with the real flags; the
                    // wraith's unconditional concealment is in already).
                    entities::billboards_from_poses(
                        game_id, &poses, dims, false, false, false, false,
                    ),
                    // No dwelling is claimed at load time, so the
                    // owned-buildings highlight is vacuously off here
                    // (and the blink phase starts low). Icon-swap
                    // suppression is likewise deferred to the first
                    // tick's rebuild (the icon tables build below).
                    entities::map_dots_from_poses(
                        game_id,
                        &poses,
                        &bundle.palette,
                        bundle.clrd.as_deref(),
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
            world.as_ref().map_or(0, |w| w.live_poses(local_hx()).len())
        );
    }

    // The original's spawn: the class-3 m4 start marker's position
    // (both games), hovering over the (post-feature) terrain, facing
    // north.
    let start =
        entities::player_start_seat(game_id, &package.things.things, human_seat).map(|(x, z)| {
            Flyer {
                x,
                y: entities::ground_at(&height, x, z) + entities::START_HOVER,
                z,
                yaw: 0.0,
                pitch: 0.0,
                ..Flyer::default()
            }
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
    // The sample bank rides the SAME MapType switch as the music
    // (LevelInit.cpp:16-38): Day 0 / Night 1 / Cave 2. MC1 is always
    // bank 0.
    let audio_bank = if audio_game == "mc2" {
        match package.header.as_ref().map(|h| h.map_type) {
            Some(mgc_formats::MapType::Night) => 1,
            Some(mgc_formats::MapType::Cave) => 2,
            _ => 0,
        }
    } else {
        0
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
    let is_mc2_pkg = package.meta.game == Game::MagicCarpet2;
    let mut mc2_human_blocked = [false; 26];
    if is_mc2_pkg && let Some(human) = package.wizards.as_ref().and_then(|w| w.wizards.first()) {
        for (s, b) in mc2_human_blocked.iter_mut().enumerate() {
            *b = human.blocked_spells.get(s).is_some_and(|&v| v != 0);
        }
    }
    let plausible_book_mc2 = if let Some(ids) = set_book.filter(|_| is_mc2_pkg) {
        // `--set-spellbook` on MC2: the same `(spell, banked_xp)`
        // install the plausible instrument uses — learn each spell,
        // bank its xp, derive the tier from the level's ladder.
        println!(
            "set-spellbook (MC2): {} spell(s) [{}]",
            ids.len(),
            ids.iter()
                .map(|&(s, xp)| format!("{}:{xp}", ui::MC2_SPELL_NAMES[s as usize]))
                .collect::<Vec<_>>()
                .join(", ")
        );
        ids.to_vec()
    } else if *book == SpellbookSeed::Plausible && is_mc2_pkg {
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
        impostors: bundle.impostors,
        billboards,
        map_dots,
        start,
        map_areas: world.as_ref().map(map_areas).unwrap_or_default(),
        sight: sight::SightAreas::default(),
        world,
        world_init,
        palette_rgba: bundle.palette,
        mc2_clrd: bundle.clrd.clone(),
        mc2_env,
        map_icons: entities::MapIcons {
            // Castle = UI sprite 58+team, balloon = 66+team, all
            // eight teams; remc1 sub_48710 :57230/:57234.
            castle: std::array::from_fn(|t| ui_assets.as_ref().and_then(|u| u.map_stamp(58 + t))),
            balloon: std::array::from_fn(|t| ui_assets.as_ref().and_then(|u| u.map_stamp(66 + t))),
            // The font as map stamps — the Beyond-Sight name labels
            // (`entities::rival_reveals`).
            glyphs: (0..=255u8)
                .map(|b| ui_assets.as_ref().and_then(|u| u.map_glyph(b)))
                .collect(),
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
            grave_icons: Default::default(),
            portal_icons: Default::default(),
        },
        map_stamps: Vec::new(),
        objective_marks: Vec::new(),
        plausible_book_mc2,
        mc2_human_blocked,
        ui: ui_assets,
        audio_dir,
        audio_bank,
        music_track,
        level_number: package.meta.level,
        bundle_variant: variant.to_string(),
        entry_sha256: package.meta.source.as_ref().map(|s| s.entry_sha256.clone()),
        etext: bundle.etext.unwrap_or_default(),
        sky: bundle.sky,
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

/// The carpet pose this frame: `(x, y, z, yaw)` lerped between the
/// last two ticks, positions taking the SHORT way around the 256-tile
/// seam. The eye and the `--thirdperson` subject both read it, so the
/// subject can never stutter against its own camera.
fn interp_carpet(a: &Flyer, b: &Flyer, alpha: f32) -> (f32, f32, f32, f32) {
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
    (
        lerp_wrap(a.x, b.x),
        a.y + (b.y - a.y) * alpha,
        lerp_wrap(a.z, b.z),
        a.yaw + (b.yaw - a.yaw) * alpha,
    )
}

/// Append the replay's INSTRUMENT billboards to a draw set: the
/// `--thirdperson` subject (the player's own carpet, which nothing
/// else draws) and, under `MGC_REPLAY_GHOST=1`, the recorded retail
/// pose.
///
/// ⚠⚠ **BOTH SUBMISSION PATHS CALL THIS.** The app builds its
/// billboard set in two places — `apply_smooth_motion` at frame rate,
/// and `sync_world` at tick rate, which is the one that runs when
/// smooth motion is off — and an instrument added to only the first
/// is an instrument that disappears the moment the player turns
/// smooth motion off. (The old ghost had exactly that gap.)
///
/// `subject` is decided by [`App::subject_visible`]: the carpet is
/// also drawn outside a replay, by the filmed MC2 exit.
fn push_replay_instruments(
    out: &mut Vec<Billboard>,
    driver: Option<&replay::ReplayDriver>,
    subject: bool,
    game: mgc_sim::ids::GameId,
    // The subject's carpet art row: the local seat's colour.
    row: u16,
    carpet: (f32, f32, f32, f32),
    bank: f32,
    dims: &impl Fn(u16) -> Option<(u16, u16, u16)>,
) {
    let (x, y, z, yaw) = carpet;
    if subject && let Some(b) = entities::human_billboard(game, row, x, y, z, yaw, bank, dims) {
        out.push(b);
    }
    let Some(d) = driver else { return };
    if replay_ghost_wanted()
        && let Some(g) = replay::ghost_billboard(d, game, dims)
    {
        out.push(g);
    }
}

/// Every OTHER seat's spectator ghost (netplay — a banished player
/// risen from his corpse, `World::risen`): his seat's carpet art,
/// translucent in its own colours, lerped from `prev` (seat order; empty
/// = the tick's own pose) to the flyer. The local ghost is the camera.
fn push_spectators(
    out: &mut Vec<Billboard>,
    sim: &Simulation,
    prev: &[Flyer],
    alpha: f32,
    dims: &impl Fn(u16) -> Option<(u16, u16, u16)>,
) {
    let Some(w) = &sim.world else { return };
    for hx in w.seat_ixs() {
        if hx == local_hx() || !w.risen(hx) {
            continue;
        }
        let now = sim.flyer[hx];
        let was = prev.get(usize::from(hx.0)).unwrap_or(&now);
        let (x, y, z, yaw) = interp_carpet(was, &now, alpha);
        let row = w.human_carpet_row(hx);
        if let Some(b) = entities::spectator_billboard(w.game(), row, x, y, z, yaw, now.roll, dims)
        {
            out.push(b);
        }
    }
}

/// `MGC_REPLAY_GHOST=1`: put the replay ghost back on screen — the
/// recorded retail pose as a translucent carpet over the port's solid
/// one, the A/B picture for a take that HAS diverged. Off by default
/// since 2026-09-20 (see the push site).
fn replay_ghost_wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("MGC_REPLAY_GHOST").is_ok_and(|v| v != "0"))
}

/// The filmed MC2 exit's float-out: seconds for the view to travel
/// from the eye onto the chase boom. The sequence's coast + aim +
/// 12-tick countdown run ~1-2 s before the launch, so the boom is
/// seated before the carpet speeds off.
const FANCY_EXIT_FLOAT: f32 = 1.0;
/// The filmed exit's arrival flash: peak white opacity, and seconds
/// to clear.
const FANCY_EXIT_FLASH_PEAK: f32 = 0.85;
/// The warp sample the filmed exit plays as the carpet vanishes: MC2's
/// teleporter pad (`World` pad arm, `snd_player(22)`).
const SND_TELEPORT_WARP: u8 = 22;
const FANCY_EXIT_FLASH: f32 = 0.35;
/// Seconds for the used portal to collapse (player: the exact timing
/// against the fade does not matter — it may still be shrinking when
/// the screen goes black).
const FANCY_EXIT_SHRINK: f32 = 1.5;

/// Collapse the used portal's sprite to scale `k` about its own
/// centre. The portal is the sprite whose feet stand on the marker's
/// plan position (a static marker, so an exact-ish match).
fn shrink_portal(set: &mut [Billboard], portal: (f32, f32, f32), k: f32) {
    let wrap_d = |d: f32| (d + 128.0).rem_euclid(256.0) - 128.0;
    for b in set.iter_mut() {
        if wrap_d(b.x - portal.0).abs() < 0.25 && wrap_d(b.z - portal.2).abs() < 0.25 {
            b.y += b.world_h * (1.0 - k) * 0.5;
            b.world_h *= k;
        }
    }
}

/// `--thirdperson`: the render camera for a replay watched from
/// behind — the CHASE view (`camera::ChaseCam` does the lag, the
/// framing and the terrain resolve, stepped by this frame's `dt`).
/// `subject_y` is the CARPET plane — the boom frames the carpet, not
/// the eye half a tile above it.
///
/// Both terrain samplers are `&self` reads on the live world, so this
/// cannot perturb a graded replay; off-world (a level with no sim
/// world yet) the boom sees flat ground at the sim's own fallback.
fn third_person_camera(
    chase: &mut camera::ChaseCam,
    eye: &CameraView,
    subject_y: f32,
    sim: &Simulation,
    aspect: f32,
    dt: f32,
) -> CameraView {
    // The EFFECTIVE fov: `flight_fov_y` widens the vertical angle on a
    // window taller than 4:3, and the framing law is in screen
    // fractions, so it has to be the fov actually rendered.
    let fov_y = mgc_render::flight_fov_y(eye.fov_y, aspect);
    let ground = |x: f32, z: f32| sim.ground_height(x, z);
    let ceiling = |x: f32, z: f32| {
        // `player_cave_ceiling` takes the sim's 8.8 axes (its `y` is
        // the renderer's `z`), returns `None` off-cave, and has
        // retail's 1.5-tile clearance already taken off.
        sim.world.as_ref().and_then(|w| {
            let xe = (x.rem_euclid(256.0) * 256.0) as u16;
            let ye = (z.rem_euclid(256.0) * 256.0) as u16;
            w.player_cave_ceiling(xe, ye).map(|c| c as f32 / 256.0)
        })
    };
    let v = chase.update(
        [eye.x, subject_y, eye.z],
        eye.yaw,
        eye.pitch,
        fov_y,
        dt,
        &ground,
        &ceiling,
    );
    CameraView {
        x: v.eye[0],
        y: v.eye[1],
        z: v.eye[2],
        yaw: v.yaw,
        pitch: v.pitch,
        // No camera bank: the world stays level and the SUBJECT
        // sprite banks instead (`Billboard::roll`), so the carpet
        // never swings across the frame.
        roll: 0.0,
        ..*eye
    }
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
    /// Every seat's flyer before the last tick (seat order) — the
    /// other seats' spectator ghosts draw lerped over the same window
    /// as the local camera (`push_spectators`).
    prev_flyers: Vec<Flyer>,
    /// The last two sim-tick pose snapshots (smooth_motion): the
    /// renderer draws entities lerped prev→cur at the frame's
    /// accumulator fraction — the same one-tick-behind timeline the
    /// camera has always run (`prev_flyer`) — and the enhanced
    /// fire/lightning emitters read them whatever the toggle says.
    /// Empty until two ticks have run.
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

impl Session {
    /// The sim's world was replaced wholesale (a save's resume, a
    /// netplay resync): the renderer's terrain and entity mirrors and
    /// the interpolation history all have to be re-seeded — the same
    /// set the castle-less-death restart re-seeds, for the same reason.
    /// `pose_prev`/`pose_cur` in particular MUST be cleared: a stale
    /// snapshot could coincidentally pair a (slot, generation) that now
    /// means a different entity.
    ///
    /// The flyer needs no re-derivation: it and the carpet are both in
    /// the payload, so they come back mutually consistent.
    fn reseed_after_restore(&mut self) {
        self.prev_flyer = self.sim.flyer[local_hx()];
        self.prev_flyers = self.sim.flyers();
        self.pose_prev = Vec::new();
        self.pose_cur = Vec::new();
        self.fire_blasts.clear();
        self.bolts.clear();
        if let Some(w) = self.sim.world.as_mut() {
            w.terrain_dirty = true;
            w.entities_dirty = true;
        }
    }
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
    /// The MC1/HW end-of-level performance screen (retail PPERF).
    Stats,
    /// The launcher (a plain start): pick a game, then its campaign
    /// boots as `--campaign` would.
    Launcher,
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
    /// The MC1/HW performance screen (retail's menu state 5 follows
    /// the level FMV), then the given step.
    Stats(StatsThen),
}

/// Where the MC1/HW performance screen hands over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StatsThen {
    /// The main menu (retail `mainMenuSelector = 2`).
    Menu,
    /// The campaign's closing movie, then out.
    Outro,
    /// Leave (single-level mode).
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
    /// The MC2 multiplayer lobby's atlas.
    LobbyMc2,
    /// The MC1 lobby's CPU-composed frame (re-uploaded every frame).
    LobbyMc1,
    /// The frontend-owned level-UI atlas (the P options menu over a
    /// frontend screen — fonts + panel art without a session).
    FrontendUi,
    /// The FMV player's resolved frame (re-uploaded as it decodes).
    Movie,
    /// The performance screen's CPU-composed frame.
    Stats,
    /// The launcher's CPU-composed frame (re-uploaded only when it
    /// changes).
    Launcher,
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
    /// O held: retail's on-demand objective box
    /// (PlayerInput.cpp:393-98 re-arms the box counter to 2 every
    /// polled frame, so the box lives exactly as long as the key is
    /// down). A control, not a presentation option — deliberately
    /// NOT gated on `audio.subtitles`.
    objective_held: bool,
    /// Space pressed since the last sim tick (respawn confirm).
    pending_full_stop: bool,
    pending_respawn: bool,
    /// Shift+L pressed since the last sim tick (castle demolish).
    pending_demolish: bool,
    /// Shift+K pressed since the last sim tick (retail suicide).
    pending_suicide: bool,
    /// Shift+C pressed since the last sim tick — the win-level cheat
    /// (retail's tester key in both games; `Cheat::WinLevel`). Rides
    /// the recorded cheat lane, so a torture-test take replays it.
    pending_cheat: Option<mgc_formats::recover::Cheat>,
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
    /// The autofire input macro's per-hand click generators
    /// (`controls.preferences.autofire`).
    autofire: autofire::Autofire,
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
    /// The Beyond-Sight map ripple's eased strength, 0..=1
    /// (`map_beyond_sight_areas`): rises while the spell is up, falls
    /// once it lapses.
    sight_strength: f32,
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
    /// `--thirdperson`: the replay viewpoint (`camera.rs`). Read only
    /// while a replay is live — live play always flies first person.
    third_person: bool,
    /// `--film`: the frame capture driving the level frame loop on
    /// the film clock (`film.rs`); `None` = wall time, no capture.
    film: Option<film::Film>,
    /// Netplay (`--host`/`--join`): the lockstep that hands every tick
    /// its seats' inputs ([`net`]). `None` = a local game.
    net: Option<net::Lockstep>,
    /// Netplay: since when the lockstep has been starved (the "waiting
    /// for the other player" notice), and the session's end, once shown.
    net_stall: Option<std::time::Instant>,
    net_stall_told: bool,
    net_end_told: bool,
    /// The netplay lobby between games (the MC2 Multiplayer button).
    netlobby: Option<NetLobby>,
    /// A join under way ("Attempting Connection").
    net_connecting: Option<std::thread::JoinHandle<Result<net::Guest, String>>>,
    /// The server this app hosts (the UI's Host, `--host`): a thread
    /// of the app, alive until the app closes or its player leaves the
    /// lobby — every member's session ends with it.
    net_server: Option<net::ServerHandle>,
    /// A close with other players still on the hosted server: asked
    /// once, the second close within the window goes through.
    close_warned: Option<std::time::Instant>,
    /// A lobby game is running: the campaign is stashed out of its way
    /// (no campaign book, no campaign progress, no saves) …
    lobby_game: bool,
    net_campaign: Option<CampaignRun>,
    /// … and a client's own values of the session pins, put back after.
    net_own_rules: Option<serde_json::Value>,
    /// A lobby game's player names by wizard seat (the given ones only).
    net_names: Vec<(u8, String)>,
    /// A netplay game's flight models per human seat (`Hx` order) —
    /// each player flies his own; empty outside netplay.
    net_models: Vec<config::ControlModels>,
    /// The third-person CHASE state (`camera::ChaseCam`): the lagged
    /// heading, the elastic eye and the subject's smoothed bank,
    /// stepped once per rendered frame.
    chase: camera::ChaseCam,
    /// The filmed MC2 exit's clock (`render.enhancement.mc2_fancy_exit`):
    /// seconds since the view started floating out of the eye onto the
    /// chase boom. None while no filmed exit runs.
    fancy_exit_t: Option<f32>,
    /// Seconds since the filmed exit's ARRIVAL tick (the carpet's
    /// vanish): drives the screen flash and the portal's shrink.
    /// Runs in either view, None otherwise.
    fancy_arrived_t: Option<f32>,
    /// `--record`: the destination path, then the live recorder.
    record_path: Option<PathBuf>,
    recorder: Option<replay::PortRecorder>,
    /// Live-option events applied since the last recorded row (the
    /// port toggle channel, `TickRecord::set`): a menu cheat/model
    /// flip is an app-side sim write the input channel never sees, so
    /// `apply_option` queues it here and the next row carries it —
    /// without this a mid-take toggle silently forked the take's
    /// course (test.mgcr's t=97 invincibility, 2026-08-27).
    rec_toggles: serde_json::Map<String, serde_json::Value>,
    /// A deferred replay recording ended at a re-anchor; do not restart.
    recorder_cut: bool,
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
    cutscenes_played: [bool; 6],
    /// What to do once the chain finishes or the player skips it.
    movie_then: AfterMovie,
    /// The finished level's performance numbers, captured at the won
    /// edge (the session is gone by the time the screen shows).
    stats_card: Option<stats_screen::StatsCard>,
    /// The running performance screen — owns the frame while
    /// `screen == Screen::Stats` — and where it hands over.
    stats_screen: Option<stats_screen::Mc1Stats>,
    stats_then: StatsThen,
    /// The launcher, while `screen == Screen::Launcher`.
    launcher: Option<launcher_screen::Launcher>,
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

/// The frontend's (level-less) audio bundle: the game's own
/// `assets/<game>-audio`, when baked.
fn frontend_audio_dir(is_mc2: bool) -> Option<PathBuf> {
    let d =
        get_baked_directory()
            .join("assets")
            .join(if is_mc2 { "mc2-audio" } else { "mc1-audio" });
    d.is_dir().then_some(d)
}

/// Open the output device and load the game's audio bundle at
/// `audio_bank`, primed with the config's gains — the ONE audio
/// bring-up, shared by the boot (`App::new`) and the lazy runtime
/// open (`App::ensure_audio`). Any failure degrades to silence,
/// never to an unplayable game.
fn open_audio(
    cfg: &config::Config,
    is_mc2: bool,
    audio_dir: Option<&Path>,
    audio_bank: u32,
) -> mgc_audio::Audio {
    let mut a = mgc_audio::Audio::open();
    a.set_prefer_gm(cfg.audio.arrangement.prefer_gm());
    if is_mc2 {
        a.set_mc2_danger_ramp();
    }
    if let Some(dir) = audio_dir {
        if let Err(e) = a.load_bundle(dir, audio_bank) {
            eprintln!("note: audio bundle: {e}");
        }
    } else {
        eprintln!("note: no audio bundle baked — sound effects disabled (rebake)");
    }
    let (sfx, music, speech) = audio_gains(cfg);
    a.set_volumes(sfx, music, speech);
    a
}

/// The three master gains the config resolves to: (sfx, music,
/// narration). A toggle off = gain 0. The narration lane rides the
/// SOUND toggle and the sfx gain, on top of its own `speech` toggle —
/// PLAYER-RULED 2026-09-08 (retail's sound/music toggles leave the CD
/// narration playing and it has no volume at all; the player wants
/// one control, see docs/DEVIATIONS.md).
fn audio_gains(cfg: &config::Config) -> (f32, f32, f32) {
    let a = &cfg.audio;
    (
        if a.sound { a.sfx_volume } else { 0.0 },
        if a.music { a.music_volume } else { 0.0 },
        if a.sound && a.speech {
            a.sfx_volume
        } else {
            0.0
        },
    )
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
        third_person: bool,
        film: Option<film::Film>,
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
        // Booting with BOTH toggles off opens no device at all; the
        // first runtime re-enable opens it then (`ensure_audio`) — it
        // used to stay `None` for the whole run, so re-enabling in
        // the menu was a no-op until a restart (player-reported).
        let mut audio = None;
        // The launcher plays nothing and may have no bake to play it
        // from; the chosen game opens its own device.
        if (cfg.audio.sound || cfg.audio.music) && !launch.launcher {
            let audio_bank = level.as_ref().map_or(0, |l| l.audio_bank);
            let audio_dir = match &level {
                Some(l) => l.audio_dir.clone(),
                None => frontend_audio_dir(is_mc2),
            };
            audio = Some(open_audio(&cfg, is_mc2, audio_dir.as_deref(), audio_bank));
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
            objective_held: false,
            pending_full_stop: false,
            pending_respawn: false,
            pending_demolish: false,
            pending_suicide: false,
            pending_cheat: None,
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
            autofire: autofire::Autofire::default(),
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
            sight_strength: 0.0,
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
            screen: if launch.launcher {
                Screen::Launcher
            } else if has_campaign {
                Screen::Menu
            } else {
                Screen::Level
            },
            campaign,
            launch,
            replay_pending: replay_boot,
            replay: None,
            third_person,
            film,
            net: None,
            net_stall: None,
            net_stall_told: false,
            net_end_told: false,
            netlobby: None,
            net_connecting: None,
            net_server: None,
            close_warned: None,
            lobby_game: false,
            net_campaign: None,
            net_own_rules: None,
            net_names: Vec::new(),
            net_models: Vec::new(),
            chase: camera::ChaseCam::default(),
            fancy_exit_t: None,
            fancy_arrived_t: None,
            record_path,
            recorder: None,
            rec_toggles: serde_json::Map::new(),
            recorder_cut: false,
            worldmap: None,
            mainmenu: None,
            mc1menu: None,
            movie: None,
            // A campaign booting to its menu gets the intro chain; a
            // direct `--level` launch does not, matching retail's own
            // level shortcut.
            boot_intro: has_campaign,
            cutscenes_played: [false; 6],
            movie_then: AfterMovie::Menu,
            stats_card: None,
            stats_screen: None,
            stats_then: StatsThen::Menu,
            launcher: None,
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
            None if app.screen == Screen::Launcher => {
                app.launcher = Some(launcher_screen::Launcher::new(
                    &get_baked_directory(),
                    &app.cfg,
                ));
            }
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
            let pin = file.sim_import_pin;
            let res = (|| -> Result<replay::ReplayDriver, String> {
                let sess = self.session.as_deref_mut().ok_or("level did not install")?;
                if let Some(snap) = &snap {
                    sess.sim.restore(snap).map_err(|e| e.to_string())?;
                    sess.prev_flyer = sess.sim.flyer[local_hx()];
                    sess.pose_prev = Vec::new();
                    sess.pose_cur = Vec::new();
                    sess.fire_blasts.clear();
                    sess.bolts.clear();
                    if let Some(w) = sess.sim.world.as_mut() {
                        w.terrain_dirty = true;
                        w.entities_dirty = true;
                        // The snapshot restored the pool and the
                        // terrain but not the MODE they were
                        // established in (`World::import_pin`). A
                        // header without the key keeps the native
                        // build's own pin — for a pre-pin take that
                        // is the acq list the (re-run) instrument
                        // grants just built, not something to zero.
                        if let Some(pin) = pin {
                            w.set_import_pin(pin);
                        }
                    }
                }
                replay::ReplayDriver::install(file, &mut sess.sim)
            })();
            match res {
                Ok(d) => self.replay = Some(d),
                Err(e) => eprintln!("error: replay: {e}"),
            }
        }
        if self.replay.is_some() {
            // `--record` UNDER `--replay` re-records the take as an
            // input-only port take. The recorder cannot begin here: a
            // retail take seeds the world inside the driver's first
            // `next`, so the start snapshot has to wait for the tick
            // loop (`begin_deferred_recorder`).
            if let Some(p) = &self.record_path {
                println!("record: {} — re-recording the replay", p.display());
            }
            return;
        }
        if let Some(path) = self.record_path.take() {
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
                            cfg.sim.parameters.entity_pool_size.map(|n| n as usize),
                            cfg.sim.parameters.awake_range,
                            // The pin is NOT import-only bookkeeping:
                            // it is every lane a snapshot deliberately
                            // skips, and a live session has real values
                            // there too — the plausible-spellbook /
                            // dev-spells grants live in the acq list
                            // and the hand bits, which passing
                            // `Default::default()` here silently
                            // dropped (the take then replayed with an
                            // EMPTY spellbook and desynced at t=1).
                            sess.sim
                                .world
                                .as_ref()
                                .map(|w| w.import_pin())
                                .unwrap_or_default(),
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
    /// The tick-loop half of `--replay --record`: begin the port
    /// recording at the first tick the driver has produced input for,
    /// and stop it at a later re-anchor. See
    /// [`replay::begin_replay_recording`] for both rules.
    fn begin_deferred_recorder(&mut self) {
        if self.recorder.is_some() || self.record_path.is_none() {
            return;
        }
        let anchored = self.replay.as_mut().is_some_and(|d| d.take_anchored());
        if self.recorder_cut {
            return;
        }
        let path = self.record_path.clone().expect("checked");
        let (pool, awake) = (
            self.cfg.sim.parameters.entity_pool_size.map(|n| n as usize),
            self.cfg.sim.parameters.awake_range,
        );
        let Some(sess) = self.session.as_deref() else {
            return;
        };
        let (game, level) = (sess.level.game, sess.level.level_number);
        match replay::begin_replay_recording(&path, &sess.sim, game, level, pool, awake) {
            Ok(r) => {
                let _ = anchored;
                self.recorder = Some(r);
            }
            Err(e) => {
                eprintln!("error: record: {e}");
                self.record_path = None;
            }
        }
    }

    /// A replay re-anchor after recording started: input alone cannot
    /// cross a capture gap, so the recording ends rather than emit a
    /// stream whose own hashes it could not reproduce.
    fn cut_recorder_at_reanchor(&mut self) {
        if self.recorder.is_none() || self.replay.is_none() {
            return;
        }
        if self.replay.as_mut().is_some_and(|d| d.take_anchored()) {
            self.recorder_cut = true;
            println!(
                "record: STOPPED — the take re-anchors here (capture gap); \
                 input alone cannot cross a gap, so only the first segment \
                 was recorded"
            );
            self.finish_recorder();
            self.record_path = None;
        }
    }

    /// Queue a live-option event for the next recorded row (the port
    /// toggle channel — see `rec_toggles`). No-op unless a recording
    /// is live or pending; last write per key wins within a row,
    /// which is also what the sim saw.
    fn queue_rec_toggle(&mut self, key: &str, value: serde_json::Value) {
        if self.recorder.is_some() || self.record_path.is_some() {
            self.rec_toggles.insert(key.into(), value);
        }
    }

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
    /// Size the window to `render.preference.window_size` — windowed
    /// only; fullscreen covers the monitor.
    fn apply_window_size(&self) {
        if let Some(window) = &self.window
            && !self.cfg.render.preference.fullscreen
        {
            let (w, h) = self.cfg.render.preference.window_size.dims();
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
        }
    }

    fn apply_fullscreen(&self) {
        if let Some(window) = &self.window {
            window.set_fullscreen(
                self.cfg
                    .render
                    .preference
                    .fullscreen
                    .then(|| winit::window::Fullscreen::Borderless(window_monitor(window))),
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

    /// Open the audio device if this run booted without one (both
    /// toggles off at boot). The bundle and sample bank follow the
    /// live session's level, else the frontend's. Called on every
    /// runtime enable; a no-op once open. Music is not started here —
    /// the `audio.music` apply arm does that right after.
    fn ensure_audio(&mut self) {
        if self.audio.is_some() {
            return;
        }
        let is_mc2 = self.is_mc2();
        let (audio_dir, audio_bank) = match self.session.as_deref() {
            Some(sess) => (sess.level.audio_dir.clone(), sess.level.audio_bank),
            None => (frontend_audio_dir(is_mc2), 0),
        };
        self.audio = Some(open_audio(
            &self.cfg,
            is_mc2,
            audio_dir.as_deref(),
            audio_bank,
        ));
        println!("audio: device opened");
    }

    /// Push the config's audio gains into the mixer (mute = gain 0).
    fn apply_volumes(&mut self) {
        if let Some(a) = &mut self.audio {
            let (sfx, music, speech) = audio_gains(&self.cfg);
            a.set_volumes(sfx, music, speech);
        }
    }

    /// Re-apply one Live option after its config value changed — THE
    /// single apply path shared by the runtime keys and the options
    /// menu (keyed by the registry's `cfg_path`). Options read live
    /// off `self.cfg` every frame/tick need no arm here.
    fn apply_option(&mut self, cfg_path: &str) {
        if let Some(g) = settings::preset_group(cfg_path) {
            for m in g.members {
                self.apply_option(m);
            }
            return;
        }
        // Netplay: the pinned options are the session's, agreed at the
        // handshake (the host's rules) — a local change would fork the
        // worlds or break fairness. Put the agreed set back.
        if let Some(ls) = &self.net
            && rules::locked_in_game(cfg_path)
        {
            let agreed = ls.config().clone();
            if let Err(e) = rules::adopt(&mut self.cfg, &agreed) {
                eprintln!("{e}");
            }
            self.mini_toast("netplay: this option is fixed for the session");
            return;
        }
        match cfg_path {
            "audio.sound" | "audio.sfx_volume" | "audio.music_volume" | "audio.speech" => {
                if self.cfg.audio.sound {
                    self.ensure_audio();
                }
                self.apply_volumes();
            }
            "audio.music" => {
                if self.cfg.audio.music {
                    self.ensure_audio();
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
            "render.enhancement.smooth_textures" => {
                if let Some(r) = &mut self.renderer {
                    r.set_smooth_textures(self.cfg.render.enhancement.smooth_textures);
                }
            }
            "render.enhancement.segment_order" => {
                if let Some(r) = &mut self.renderer {
                    r.set_segment_order(self.cfg.render.enhancement.segment_order);
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
            "render.enhancement.autocontrasting_markers" => {
                if let Some(r) = &mut self.renderer {
                    r.set_marker_halo(self.cfg.render.enhancement.autocontrasting_markers);
                    // The halo also decides the 1x dot bake-vs-lift,
                    // which lives in the tick-throttled map recompose —
                    // run it now so the toggle shows live in the menu.
                    if let Some(sess) = self.session.as_deref() {
                        let overlay = map_overlay(&sess.level, &self.cfg);
                        r.update_map(&sess.level.view, &overlay);
                    }
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
            "render.preference.horizon_cull" => {
                if let Some(r) = &mut self.renderer {
                    r.set_horizon_cull(self.cfg.render.preference.horizon_cull);
                }
            }
            "render.preference.vsync" => {
                if let Some(r) = &mut self.renderer {
                    r.set_vsync(self.cfg.render.preference.vsync);
                }
            }
            // The launcher is always a window: its Display row takes
            // effect when the game starts (`start_from_launcher`).
            "render.preference.fullscreen" | "render.preference.window_size"
                if self.screen == Screen::Launcher => {}
            "render.preference.fullscreen" => {
                self.apply_fullscreen();
                self.apply_window_size();
                self.reassert_pointer();
            }
            "render.preference.window_size" => self.apply_window_size(),
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
                        let poses = w.live_poses(local_hx());
                        if self.cfg.render.enhancement.map_marker_icons {
                            icons_grew = capture_marker_icons(&mut sess.level, &poses);
                        }
                        let swapped = dot_swap_set(&sess.level, &self.cfg);
                        sess.level.map_dots = entities::map_dots_from_poses(
                            sess.level.game,
                            &poses,
                            &sess.level.palette_rgba,
                            sess.level.mc2_clrd.as_deref(),
                            self.cfg.render.enhancement.map_owned_buildings,
                            sess.level.mc2_env,
                            sess.sim.tick as u32,
                            &swapped,
                        );
                        sess.level.map_stamps = entities::map_stamps_from_poses(
                            sess.level.game,
                            &poses,
                            &sess.level.map_icons,
                            w.beyond_sight(local_hx()),
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
                        .set_thrust_model(local_hx(), sim_thrust(self.cfg.controls.models.thrust));
                }
                self.queue_rec_toggle(
                    "thrust_model",
                    match self.cfg.controls.models.thrust {
                        config::ThrustModel::Classic => "classic".into(),
                        config::ThrustModel::Enhanced => "enhanced".into(),
                    },
                );
            }
            "controls.models.altitude" => {
                if let Some(sess) = self.session.as_deref_mut() {
                    // The setter re-seeds the desired altitude at the
                    // live pose when entering the enhanced tier.
                    sess.sim.set_altitude_model(
                        local_hx(),
                        sim_altitude(self.cfg.controls.models.altitude),
                    );
                    // `mc2_leviathan_high_lunge` follows the altitude
                    // model (`world_patches`); a live recording or
                    // replay keeps its pinned arms.
                    if self.recorder.is_none()
                        && self.replay.is_none()
                        && let Some(w) = sess.sim.world.as_mut()
                    {
                        w.set_patches(world_patches(&self.cfg));
                    }
                }
                self.queue_rec_toggle(
                    "altitude_model",
                    match self.cfg.controls.models.altitude {
                        config::AltitudeModel::Classic => "classic".into(),
                        config::AltitudeModel::Enhanced => "enhanced".into(),
                    },
                );
            }
            "gameplay.cheat.weightless" => {
                if let Some(sess) = self.session.as_deref_mut() {
                    sess.sim.weightless = self.cfg.gameplay.cheat.weightless;
                }
                self.queue_rec_toggle("weightless", self.cfg.gameplay.cheat.weightless.into());
            }
            "gameplay.cheat.dev_spells" => {
                if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_dev_spells(local_hx(), self.cfg.gameplay.cheat.dev_spells);
                }
                self.queue_rec_toggle("dev_spells", self.cfg.gameplay.cheat.dev_spells.into());
            }
            "gameplay.cheat.invincible" => {
                if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_invincible(self.cfg.gameplay.cheat.invincible);
                }
                self.queue_rec_toggle("invincible", self.cfg.gameplay.cheat.invincible.into());
            }
            "gameplay.cheat.ghost" => {
                if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_ghost(local_hx(), self.cfg.gameplay.cheat.ghost);
                }
                self.queue_rec_toggle("ghost", self.cfg.gameplay.cheat.ghost.into());
            }
            "gameplay.cheat.inert" => {
                if let Some(w) = self
                    .session
                    .as_deref_mut()
                    .and_then(|s| s.sim.world.as_mut())
                {
                    w.set_inert(self.cfg.gameplay.cheat.inert);
                }
                self.queue_rec_toggle("inert", self.cfg.gameplay.cheat.inert.into());
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
                    w.set_patches(world_patches(&self.cfg));
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
        // A preset row has no config field of its own: its value IS
        // its members'.
        match settings::preset_group(spec.cfg_path) {
            Some(g) => g.members.iter().for_each(|m| self.persist_path(m)),
            None => self.persist_path(spec.cfg_path),
        }
    }

    /// A fresh options menu; under netplay the session's pinned rows
    /// come up locked.
    fn options_menu(&self) -> menu::MenuState {
        let mut m = menu::MenuState::new(&self.specs);
        if self.net.is_some() {
            m.pin_session_rules(&self.specs);
        }
        m
    }

    /// Persist one dotted config path's current value (see
    /// [`Self::persist_option`]).
    fn persist_path(&self, cfg_path: &str) {
        // A session pin holds the HOST's value: never the player's own.
        if self.net.is_some() && rules::pin_of(cfg_path).is_some() {
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
        for seg in cfg_path.split('.') {
            leaf = &leaf[seg];
        }
        let mut root = std::fs::read_to_string(&self.cfg_file)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let segs: Vec<&str> = cfg_path.split('.').collect();
        let mut slot = &mut root;
        for seg in &segs[..segs.len() - 1] {
            if !slot.get(*seg).is_some_and(|v| v.is_object()) {
                slot[*seg] = serde_json::json!({});
            }
            slot = &mut slot[*seg];
        }
        slot[*segs.last().unwrap()] = leaf.clone();
        let text = serde_json::to_string_pretty(&root).expect("json serializes") + "\n";
        // The config folder may be a fresh one (`MGC_CONFIG_DIR`).
        if let Some(dir) = self.cfg_file.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
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
    ///
    /// A NETWORK game never pauses: one peer's pause starved every
    /// peer's lockstep until he came back (player-reported 2026-10-15;
    /// retail refused pause in multiplayer). There P opens the netplay
    /// panel (Options, Leave) over the running world, and the carpet
    /// holds an idle hover while it is up ([`Self::net_menu`]).
    fn toggle_menu(&mut self) {
        if !self.menu_up() {
            let net = self.net.is_some() && self.screen == Screen::Level;
            self.paused = !net;
            // Frontend screens need the frontend-owned UI bank for
            // the menu's fonts/panels (no session to borrow from).
            self.ensure_frontend_ui();
            // Without a baked font there is no menu — plain pause.
            if ui_assets!(self).is_some() {
                if net {
                    self.mini = Some(minimenu::MiniMenu::netplay());
                    // Hover where the menu caught you: the stop, and
                    // the steering stick recentred (`tick_input`
                    // holds both while the panel is up).
                    self.pending_full_stop = true;
                    self.stick = VirtualStick::default();
                } else if self.screen == Screen::Level {
                    self.mini = Some(minimenu::MiniMenu::new());
                } else {
                    self.menu = Some(self.options_menu());
                }
                self.menu_grab_restore = self.grabbed;
                self.set_grab(false);
                self.fire_held = false;
                self.fire_right_held = false;
            }
        } else {
            self.paused = false;
            // Unpause (netplay: closing the panel) is the ONLY way
            // out of the mini-menu (the standing Esc law), and it
            // closes the options layer with it if one is stacked on
            // top.
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
                Screen::Movie | Screen::Stats | Screen::Launcher => {}
            }
        }
        // Retail pause suspends ALL sound; resumed sounds pick up
        // where they froze.
        if let Some(a) = &mut self.audio {
            a.set_paused(self.paused);
        }
        println!(
            "{}",
            match (self.paused, self.net_menu()) {
                (true, _) => "paused",
                (false, true) => "menu (netplay: the game runs on)",
                (false, false) => "unpaused",
            }
        );
    }

    /// Is the pause menu up — paused, or the netplay panel over the
    /// running world? The pointer stays free while it is.
    fn menu_up(&self) -> bool {
        self.paused || self.mini.is_some()
    }

    /// The netplay panel is up over the running world: the carpet
    /// hovers idle (no steering, no thrust, no fire).
    fn net_menu(&self) -> bool {
        self.net.is_some() && self.mini.is_some() && !self.paused
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
                if v {
                    self.ensure_audio();
                }
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
                if v {
                    self.ensure_audio();
                }
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
            // ⭐⭐ THE CHEAT ROW IS `G H J` + `B N`, AND NOTHING ELSE
            // IS A BARE LETTER. Cheats sat on G/H/J/I/L and the five
            // view toggles on T/V/B/C/K until 2026-09-17, when the
            // player found `weightless` on L EATING THE MC1 DEMOLISH
            // (Shift+L): `option_key` carries no shift test and the
            // demolish handler sits BELOW it in the dispatch, so the
            // bare-letter toggle won every time. Shift+L and Shift+K
            // are pressed hundreds of times a session during castle
            // demolition — "it is always a mistake, one of them I
            // accidentally hit the next key instead" — so a toggle
            // that shadows either of them is a gameplay regression,
            // not a nuisance. The player's ruling (2026-09-17): the
            // five VIEW toggles lose their keys entirely (they live in
            // the options menu, where every one of them already has a
            // row) and the two displaced cheats move DOWN a row onto
            // B and N — under G/H/J, unmapped by anything, and clear
            // of the WASD pad. `option_key` is now shift-gated too, so
            // no future Shift+<letter> can be shadowed this way.
            KeyCode::KeyJ => {
                self.cfg.gameplay.cheat.ghost = !self.cfg.gameplay.cheat.ghost;
                let v = self.cfg.gameplay.cheat.ghost;
                println!(
                    "ghost: {}",
                    if v {
                        "on (cheat — unseen by everything, walls are terrain)"
                    } else {
                        "off (seen, walled in)"
                    }
                );
                ("gameplay.cheat.ghost", format!("Ghost mode {}", onoff(v)))
            }
            KeyCode::KeyB => {
                self.cfg.gameplay.cheat.inert = !self.cfg.gameplay.cheat.inert;
                let v = self.cfg.gameplay.cheat.inert;
                println!(
                    "inert: {}",
                    if v {
                        "on (cheat — no trigger fires under you)"
                    } else {
                        "off (triggers fire)"
                    }
                );
                ("gameplay.cheat.inert", format!("Inert mode {}", onoff(v)))
            }
            // N = weightless (the last key of the cheat row).
            KeyCode::KeyN => {
                self.cfg.gameplay.cheat.weightless = !self.cfg.gameplay.cheat.weightless;
                let v = self.cfg.gameplay.cheat.weightless;
                println!(
                    "weightless: {}",
                    if v {
                        "on (cheat — free climb to the global lift ceiling)"
                    } else {
                        "off (the per-game altitude band)"
                    }
                );
                (
                    "gameplay.cheat.weightless",
                    format!("Weightless mode {}", onoff(v)),
                )
            }
            _ => return false,
        };
        self.apply_option(path);
        // A session pin was put back (and said so): no "on/off" echo.
        if self.net.is_some() && rules::locked_in_game(path) {
            return true;
        }
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
    fn audio_tick(&mut self, requested_at: Option<mgc_sim::engine::world::PlayerPose>) {
        // The hidden-realm line's "another" arm (`byte_0x3E4_2BE4_12226
        // != 0`, see `MC2_SPEECH_SECRET`): an earlier level's secret
        // portal is already revealed or conquered. No campaign (a bare
        // `--level` launch) = the first find.
        let secret_again = self
            .campaign
            .as_ref()
            .and_then(|r| r.save.mc2())
            .is_some_and(|s| {
                s.secrets
                    .iter()
                    .any(|p| p.level != 0 && matches!(p.activated, 1 | 2))
            });
        let Some(audio) = &mut self.audio else {
            // No mixer (sound and music off): the world's sound
            // requests drain every tick all the same — the pending vec
            // is HASHED, so a netplay peer that kept it forked from
            // every other one, and from the server's world.
            if let Some(w) = self
                .session
                .as_deref_mut()
                .and_then(|s| s.sim.world.as_mut())
            {
                drain_sounds(w);
            }
            return;
        };
        let Some(sess) = self.session.as_deref_mut() else {
            return;
        };
        let f = &sess.sim.flyer[local_hx()];
        let pose =
            mgc_sim::engine::world::PlayerPose::from_tiles(f.x, f.y, f.z, f.yaw, f.pitch, 0.0);
        // ⭐ THE REQUEST-TIME LISTENER. `sub_55370_558A0` (CARPET.EXE
        // @0x6db68 = VA 0x55370) computes a request's volume and pan
        // THE INSTANT the sim asks for it, off the LOCAL wizard's live
        // entity axis (`mov 0x3415(%ebx,%eax,1),%ax` — pool record
        // `2049*var_u16_8 + 13333`, entity +72), and culls beyond
        // 12288^2 (`cmp $0x9000000,%eax; jg`). The port instead
        // computes it at DRAIN time, after the whole sim step — and
        // the step is where a PORTAL WARP moves the listener. The
        // vortex (`sub_26A60`, CARPET.EXE @0x3f397) requests sound 22
        // BEFORE `sub_41C70` copies the destination onto the carpet
        // (@0x3f3a7), so retail hears it at zero distance; the port
        // heard it from the far side of the map and the 12288-unit
        // cull threw it away (92 of MC1's 113 authored portals warp
        // further than that). Use the pose the step STARTED from —
        // sub-tile for every other emitter, the whole map for a warp.
        let listener_pose = requested_at.unwrap_or(pose);
        let listener = mgc_audio::Listener {
            pos: (listener_pose.x, listener_pose.y, listener_pose.z),
            yaw: listener_pose.heading,
        };
        if let Some(w) = &mut sess.sim.world {
            let frame = w.take_audio(local_hx(), pose);
            // A PLAYER sound is its own seat's (the equip chime, a
            // level-up): another human's is not ours to hear.
            let mine = w.human_tag(local_hx());
            if self.cfg.audio.sound {
                for e in frame
                    .events
                    .into_iter()
                    .filter(|e| !e.player || e.tag == mine)
                {
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
                let secret = seg == mgc_sim::engine::world::MC2_SPEECH_SECRET;
                if self.cfg.audio.sound && self.cfg.audio.speech {
                    let (row, cseg) = if secret {
                        (25 + u32::from(secret_again), 0)
                    } else if (30..=34).contains(&lvl) {
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
                    && let Some(idx) = if secret {
                        Some(284 + usize::from(secret_again))
                    } else {
                        mc2_narration_etext(lvl, seg)
                    }
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
                let f = &sess.sim.flyer[local_hx()];
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
        let patches = self.world_patches_now();
        let keep_quick_keys = self.carried_quick_keys().is_some();
        {
            let Some(sess) = self.session.as_deref_mut() else {
                return;
            };
            let Some(init) = &mut sess.level.world_init else {
                return;
            };
            // The gate is part of the level entry: a world resumed from
            // a mid-level save carries it in its snapshot, not in the
            // init it was rebuilt from.
            if let Some(cur) = sess.sim.world.as_ref() {
                init.mc2_replayed = cur.mc2_level_replayed();
            }
            let mut w = init.build();
            self.pool_dropped_total = 0;
            self.misfits_reported = 0;
            // Retail wipes + reseeds the quick keys at level init
            // (:49216-59) — the acquisition diff below re-seeds the
            // starting spells in canonical order on the first tick.
            // A campaign carrying the player's binds keeps the live
            // ones instead (a restart is no reason to lose a rebind).
            if !keep_quick_keys {
                self.quick_binds = [None; 10];
            }
            self.prev_owned = [false; 24];
            // A restart is a level entry — the situational sim speed
            // resets with the rest (mirrors `install_level`).
            self.cfg.sim.options.game_speed = config::GameSpeed::Normal;
            apply_instruments(
                &mut w,
                self.cfg.gameplay.cheat.dev_spells,
                &sess.level.plausible_book_mc2,
                self.cfg.gameplay.cheat.invincible,
                self.cfg.gameplay.cheat.ghost,
                self.cfg.gameplay.cheat.inert,
                patches,
            );
            if let Some(run) = &self.campaign {
                // The restart is a fresh world — the campaign carry
                // re-grants like any level entry.
                apply_campaign_book(&mut w, run, &sess.level.mc2_human_blocked);
            }
            w.terrain_dirty = true;
            w.entities_dirty = true;
            // Each seat keeps its own models across the restart
            // (netplay: every player flies his own).
            let models: Vec<_> = sess
                .sim
                .seat_ixs()
                .map(|hx| (sess.sim.thrust_model[hx], sess.sim.altitude_model[hx]))
                .collect();
            sess.sim = Simulation::with_world(w);
            for (hx, (thrust, altitude)) in sess.sim.seat_ixs().zip(models) {
                sess.sim.thrust_model[hx] = thrust;
                sess.sim.altitude_model[hx] = altitude;
            }
            sess.sim.weightless = self.cfg.gameplay.cheat.weightless;
            if let Some(start) = sess.level.start {
                sess.sim.flyer[mgc_sim::Hx::FIRST] = start;
                sess.sim.sync_carpet_from_flyer(mgc_sim::Hx::FIRST);
            }
            sess.prev_flyer = sess.sim.flyer[local_hx()];
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
                self.menu = Some(self.options_menu());
            }
            // The netplay panel's way out: the abandon confirm, as Esc
            // (confirming leaves the game — the host's ends it for all).
            minimenu::Hit::Leave => {
                if self.quit_fade.is_none() {
                    self.exit_confirm = true;
                }
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
    /// Netplay's per-tick gate (see [`NetTurn`]): schedule this seat's
    /// due inputs, then hand out the next bundle if it has arrived.
    /// After the session ends the game goes on locally — the other
    /// seats stand idle (host handover is a later stage).
    fn net_turn(&mut self) -> NetTurn {
        let Some(ls) = self.net.as_mut() else {
            return NetTurn::Local;
        };
        ls.poll();
        if let Some(why) = ls.closed().map(str::to_owned) {
            let seats = ls.seats();
            if !self.net_end_told {
                self.net_end_told = true;
                self.mini_toast(format!("netplay ended: {why} — the other carpet idles"));
            }
            let mut inputs = vec![FlightInput::default(); seats];
            inputs[usize::from(local_hx().0)] = self.tick_input();
            return NetTurn::Ready(inputs, Vec::new());
        }
        // Lead-in ticks carry an idle input; every later one samples
        // the controls now, to step INPUT_DELAY ticks from now.
        while let Some(lead_in) = self
            .net
            .as_ref()
            .filter(|ls| ls.wants_input())
            .map(net::Lockstep::in_lead_in)
        {
            let input = if lead_in {
                FlightInput::default()
            } else {
                self.tick_input()
            };
            if let Some(ls) = self.net.as_mut() {
                ls.submit(replay::port_input_from(&input));
            }
        }
        let Some(ls) = self.net.as_mut() else {
            return NetTurn::Local;
        };
        let Some((tick, inputs)) = ls.take_turn() else {
            let since = *self.net_stall.get_or_insert_with(std::time::Instant::now);
            if since.elapsed() > std::time::Duration::from_secs(1) && !self.net_stall_told {
                self.net_stall_told = true;
                self.mini_toast("netplay: waiting for the other player…");
            }
            return NetTurn::Starved;
        };
        self.net_stall = None;
        self.net_stall_told = false;
        // A resync: the server's world replaces ours before this tick.
        if let Some(snap) = ls.take_snapshot(tick)
            && let Some(sess) = self.session.as_deref_mut()
        {
            let outcome = net_resync(&mut sess.sim, &snap);
            ls.resynced(tick, outcome.as_deref().map_err(Clone::clone));
            sess.reseed_after_restore();
        }
        if let Some(sess) = self.session.as_deref() {
            ls.note_hash(tick, || sess.sim.state_hash());
        }
        let desync = ls.take_desync();
        let left = ls.left_at(tick);
        let inputs = inputs.iter().map(replay::flight_input_from).collect();
        if let Some(msg) = desync {
            self.mini_toast(format!("netplay: {msg}"));
        }
        NetTurn::Ready(inputs, left)
    }

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
                .map(|w| w.player_mana_share_pct(local_hx()))
                .unwrap_or(0);
            // Mid-level, the WORLD's cycle ring is the live one
            // (run.mc1_ring only refreshes at the won edge) — keep
            // the header sidecar in step with the snapshot.
            if run.id != campaign::CampaignId::Mc2
                && let Some(w) = sess.sim.world.as_ref()
            {
                let ring = w.loadout(local_hx()).ring;
                package.header.mc1_spell_ring = (ring != [0; 24]).then_some(ring);
                // The live quick keys likewise (the run's carry only
                // refreshes at the won edge).
                package.header.mc1_quick_keys = quick_keys_carry(&self.quick_binds);
            }
            package.snapshot = Some(sess.sim.snapshot());
            package.header.resume = Some(mgc_formats::mgcs::InLevel {
                bundle: level.bundle_variant.clone(),
                entry_sha256: level.entry_sha256.clone().unwrap_or_default(),
                snapshot_version: mgc_sim::snapshot::SNAPSHOT_VERSION,
                tick: sess.sim.tick,
                mana_pct,
                thrust_model: Some(format!("{:?}", sess.sim.thrust_model[local_hx()])),
                altitude_model: Some(format!("{:?}", sess.sim.altitude_model[local_hx()])),
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
            &SpellbookSeed::Level,
            &mc1_campaign_carry(Some(&fresh)),
            self.launch.pool_slots,
            self.launch.awake_range,
            false,
            &[0], // campaign: the human is wizard 0
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

        // The restore replaced the world wholesale.
        if let Some(sess) = self.session.as_deref_mut() {
            sess.reseed_after_restore();
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
    /// continuable): report and exit. `replayed` is the MC2 campaign
    /// REPLAY gate — only a world-map portal click raises it
    /// ([`mc2_map_launch_replayed`]).
    fn campaign_switch(&mut self, n: u32, replayed: bool, event_loop: &ActiveEventLoop) {
        let Some(run) = &mut self.campaign else {
            return;
        };
        run.current = n;
        let path = run.level_path(n);
        println!(
            "campaign: launching level {n}{}",
            if replayed {
                " (replay: no spell XP)"
            } else {
                ""
            }
        );
        let carry = mc1_campaign_carry(Some(run));
        let level = match load_level(
            &path,
            self.launch.tileset,
            self.launch.terrain_features,
            &SpellbookSeed::Level, // every instrument is off in campaign mode
            &carry,
            self.launch.pool_slots,
            self.launch.awake_range,
            replayed,
            &[0], // campaign: the human is wizard 0
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
            // The level's sample bank (`LoadedLevel::audio_bank`:
            // MC2's Day 0 / Night 1 / Cave 2 by MapType). The boot
            // path loads it in `App::new`; a campaign run installs
            // level after level and used to keep the BOOT bank for
            // all of them — the frontend boots level-less at bank 0,
            // so every Night/Cave level played bank 0's 16-byte
            // silence stub for the firefly calls (ids 43/44) and
            // found no drip samples (65-69). Same bundle dir: a bank
            // re-select, no reload.
            if let Some(dir) = &level.audio_dir
                && let Err(e) = a.load_bundle(dir, level.audio_bank)
            {
                eprintln!("note: audio bundle: {e}");
            }
        }
        self.paused = false;
        self.menu = None;
        self.mini = None;
        let sim = session_sim(
            &mut level,
            &self.cfg,
            &self.net_models,
            self.campaign.as_ref(),
        );
        let prev_flyer = sim.flyer[local_hx()];
        let prev_flyers = sim.flyers();
        // A button held across the level change must not resume its
        // autofire chain in the new world.
        self.autofire.halt();
        self.sight_strength = 0.0;
        self.session = Some(Box::new(Session {
            level,
            sim,
            prev_flyer,
            prev_flyers,
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
        self.quick_binds = self.carried_quick_keys().unwrap_or([None; 10]);
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
            if let Some(pack) = entities::level_impostors(sess.level.impostors.as_ref()) {
                r.load_impostors(pack);
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

    /// The local wizard is falling or dead — retail's `life < 0`, the
    /// state in which it reads no gameplay input but the respawn key.
    /// A risen spectator ghost is not down: it flies (the sim hears its
    /// controls and nothing else).
    fn player_down(&self) -> bool {
        self.screen == Screen::Level
            && self
                .session
                .as_deref()
                .and_then(|s| s.sim.world.as_ref())
                .is_some_and(|w| {
                    (w.player_falling(local_hx()) || w.player_dead(local_hx()))
                        && !w.risen(local_hx())
                })
    }

    /// While the wizard is down, the view is the main view and nothing
    /// is held: retail's map modes close themselves the moment `life`
    /// goes negative (`HandleButtonClick_191B0(20, 0)`, PlayerInput.cpp
    /// :936-38 — the ordinary close, so the mode-switch ding rides
    /// along) and its flight mode skips every spell/selector read
    /// (:812-14). The CTRL pane and a held fire button go with them.
    fn enforce_player_down(&mut self) {
        if !self.player_down() {
            return;
        }
        if self.book_open() {
            if let Some(r) = &mut self.renderer {
                r.set_map_view(false);
            }
            self.ui_ding();
            self.set_grab(true);
            self.stick = VirtualStick::default();
        }
        if self.ctrl_held {
            self.ctrl_held = false;
            self.selector_drag = None;
            self.selector_hover = ui::SelectorHover::default();
            self.set_grab(true);
        }
        self.fire_held = false;
        self.fire_right_held = false;
    }

    fn tick_input(&mut self) -> FlightInput {
        self.enforce_player_down();
        let axis = |neg: bool, pos: bool| (pos as i32 - neg as i32) as f32;
        let k = &self.keys;
        // Keyboard turn rate: radians per tick (enhanced model only).
        let key_turn = 2.2 * TICK_DT;
        // Whether the fullscreen map suspends movement input follows
        // the SELECTOR LAYOUT, per game law (player ruling
        // 2026-07-24): retail MC1's map replaces flight input with
        // the ball+arrow cursor for BOOK spell-picking, so a map
        // carrying the book (`map_book`) suspends (steering only since
        // 2026-10-17 — see `book_map` below); retail MC2's map
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
        //
        // The netplay menu holds the carpet in an idle hover: no
        // movement read (as the dialog), and the speed stopped too.
        //
        // DEVIATION (player ruling 2026-10-17, no toggle): the BOOK map
        // suspends only STEERING — the mouse is the spell picker there,
        // and the turn/pitch keys go with it — while the movement keys
        // (thrust, strafe, lift) stay live. Retail's map kept the speed
        // target and flew on; under the enhanced thrust model a zeroed
        // thrust key is a stop, so suspending it froze the carpet for
        // every spell pick. The dialogs still suspend everything.
        let net_menu = self.net_menu();
        let book_map = self.book_open() && self.selector.map_book;
        let suspended = self.exit_confirm || net_menu;
        let book = book_map || suspended;
        let mc1 = self.cfg.controls.models.thrust == config::ThrustModel::Classic;
        // MC2's barrel roll: both strafe keys down this tick with
        // NEITHER down the last — retail's edge detect against the
        // prev-frame strafe byte (PlayerInput.cpp:2088-97; you must
        // press both from neutral, and holding one and tapping the
        // other does nothing). The sim's flight-verb gate makes the
        // command MC2-only; the dialogs swallow it with the rest of
        // the movement input (the book map keeps it, like the keys).
        let strafes = (k.left && !suspended, k.right && !suspended);
        // `controls.preferences.disable_barrel_roll` drops the trigger
        // here; the edge state still tracks, so re-enabling mid-hold
        // does not fire a roll.
        let barrel_roll = strafes.0
            && strafes.1
            && !self.prev_strafe.0
            && !self.prev_strafe.1
            && !self.cfg.controls.preferences.disable_barrel_roll;
        self.prev_strafe = strafes;
        // While a roll runs, the virtual stick recenters (retail's
        // driver zeroes `rollDelta` — the mouse stick — every tick,
        // EF:38957, so the roll ENDS with the stick centered too; a
        // parked pre-roll deflection must not snap back). X only, the
        // pitch axis stays live.
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.sim.barrel_rolling(local_hx()))
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
            full_stop: std::mem::take(&mut self.pending_full_stop) || net_menu,
            respawn: std::mem::take(&mut self.pending_respawn),
            demolish: std::mem::take(&mut self.pending_demolish),
            suicide: std::mem::take(&mut self.pending_suicide),
            cheat: self.pending_cheat.take(),
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
        // Autofire: a held button on a click-only projectile spell
        // becomes a slow train of ordinary clicks (see `autofire`).
        // Shaped last, so the fly assistant above still reads the
        // physical buttons; off, the chains stay broken and the raw
        // levels pass as they always did.
        if self.cfg.controls.preferences.autofire
            && let Some(s) = self.session.as_deref()
        {
            let hands = (
                s.sim.autofire_hand(local_hx(), false),
                s.sim.autofire_hand(local_hx(), true),
            );
            let level_over = s
                .sim
                .world
                .as_ref()
                .is_some_and(|w| w.completed(local_hx()));
            (input.fire_left, input.fire_right) =
                self.autofire
                    .tick((input.fire_left, input.fire_right), hands, level_over);
        } else {
            self.autofire.halt();
        }
        if book {
            // The original's map/book modes write NO movement input
            // (:20635-:20744 never reach the mouse read or command 6)
            // — the steering filters decay to center while the speed
            // target persists (the "map fixes your orientation, not
            // your velocity" behavior). The book map keeps the
            // movement keys (see `book_map` above); steering goes in
            // both.
            if suspended {
                input.thrust = 0.0;
                input.strafe = 0.0;
                input.lift = 0.0;
            }
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
    /// option. Independent of smooth_motion since 2026-10-10: with the
    /// toggle off the flame anchors to tick poses (alpha pinned to 1)
    /// and steps with the world, its flicker still on the frame clock.
    /// The filmed MC2 exit's view of the ending sequence
    /// (`render.enhancement.mc2_fancy_exit`): Some while the sequence
    /// runs AND the option is on.
    fn fancy_exit_view(&self) -> Option<mgc_sim::engine::world::Mc2EndView> {
        if !self.cfg.render.enhancement.mc2_fancy_exit {
            return None;
        }
        self.session
            .as_deref()?
            .sim
            .world
            .as_ref()?
            .mc2_end_view(local_hx())
    }

    /// Is the player's own carpet on screen? Under `--thirdperson`
    /// replay, always; during the filmed MC2 exit, until the arrival
    /// tick — it vanishes into the portal (either view).
    fn subject_visible(&self) -> bool {
        let chase = self.third_person && self.replay.is_some();
        match self.fancy_exit_view() {
            Some(v) => !v.arrived,
            None => chase,
        }
    }

    /// The used portal collapsing after the filmed exit's arrival:
    /// its position and this frame's scale (1 → 0 over
    /// `FANCY_EXIT_SHRINK` seconds). None before arrival or without
    /// a portal.
    fn portal_shrink(&self) -> Option<((f32, f32, f32), f32)> {
        let t = self.fancy_arrived_t?;
        let portal = self.fancy_exit_view()?.portal?;
        let k = (t / FANCY_EXIT_SHRINK).clamp(0.0, 1.0);
        Some((portal, 1.0 - k * k * (3.0 - 2.0 * k)))
    }

    fn enhanced_fire(&self) -> bool {
        self.cfg.render.enhancement.fire == config::FireEffects::Enhanced
    }

    /// Is the procedural lightning active? Same law as
    /// [`App::enhanced_fire`]: the option alone.
    fn enhanced_lightning(&self) -> bool {
        self.cfg.render.enhancement.lightning == config::LightningEffects::Enhanced
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
        let subject = self.subject_visible();
        let shrink = self.portal_shrink();
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
            self.cfg.gameplay.patches.ball_owner_recolor.on(),
        );
        // THE REPLAY VIEWPOINT (④). Two instrument billboards, both
        // minted here from a pose and never from the world.
        //
        // ⚖ PLAYER RULING 2026-09-20, splitting the replay in two:
        //
        // `--firstperson` (default) draws NEITHER. The old ghost — the
        // recorded retail pose, translucent, sitting under the
        // viewport — only obscured the picture, and the divergence it
        // advertised is in the take's data and the HUD anyway; with
        // the corpus all but certified, a visibly displaced ghost is a
        // sight nobody has had in rounds.
        //
        // `--thirdperson` draws the HUMAN's own carpet, solid, as the
        // subject the boom is framing (`camera.rs`) — off the SAME
        // interpolated flyer the camera anchors to, so the subject can
        // never stutter against its own camera, and identically for a
        // retail take and a port one (the ghost exists only for retail
        // takes; the flyer exists for both).
        //
        // `MGC_REPLAY_GHOST=1` puts the ghost back in either view, for
        // the day a divergence wants watching: retail's pose
        // translucent over the port's solid one is exactly the A/B
        // picture.
        push_replay_instruments(
            &mut billboards,
            self.replay.as_ref(),
            subject,
            sess.level.game,
            sess.sim
                .world
                .as_ref()
                .map_or(entities::HUMAN_CARPET_ROW, |w| {
                    w.human_carpet_row(local_hx())
                }),
            interp_carpet(&sess.prev_flyer, &sess.sim.flyer[local_hx()], alpha),
            self.chase.bank_now(),
            &dims,
        );
        push_spectators(&mut billboards, &sess.sim, &sess.prev_flyers, alpha, &dims);
        if let Some((portal, k)) = shrink {
            shrink_portal(&mut billboards, portal, k);
        }
        r.set_billboards(billboards);
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

    /// The enhanced-effects pass: the procedural fire particles and
    /// lightning bolts, rebuilt every frame from the pose pair and the
    /// ledgers at this frame's lerp fraction. Runs whatever the
    /// smooth-motion toggle says (with it off, alpha is pinned to 1 by
    /// the caller, so flame and bolt geometry step with the world while
    /// flicker and dapple keep the frame clock). Skipped while paused —
    /// the fire freezes under the pause menu, as before — and until two
    /// ticks have run (the paused option flip in `sync_world` covers
    /// the in-menu swap).
    fn apply_effects(&mut self, alpha: f32) {
        let Some(sess) = self.session.as_deref() else {
            return;
        };
        if self.paused || sess.pose_prev.is_empty() || sess.pose_cur.is_empty() {
            return;
        }
        let enhanced_fire = self.enhanced_fire();
        let enhanced_lightning = self.enhanced_lightning();
        let Some(r) = &mut self.renderer else { return };
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
    }

    fn sync_world(&mut self) {
        let enhanced_fire = self.enhanced_fire();
        let enhanced_lightning = self.enhanced_lightning();
        let subject = self.subject_visible();
        let shrink = self.portal_shrink();
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
            self.spell_levels = w.mc2_book_view(local_hx()).sel;
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
            let poses = w.live_poses(local_hx());
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
                self.cfg.gameplay.patches.ball_owner_recolor.on(),
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
                level.mc2_clrd.as_deref(),
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
                w.beyond_sight(local_hx()),
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
            // The Beyond-Sight rival reveal: the name labels (+ MC2's
            // position pixel), gated by the live tier.
            // Netplay: the other humans tag, reveal and roster like rivals.
            let mut rival_views = w.rival_views();
            rival_views.extend(w.human_views(local_hx()));
            name_humans(&self.net_names, &mut rival_views);
            let (reveal_dots, reveal_stamps) = entities::rival_reveals(
                level.game,
                &rival_views,
                w.beyond_sight_tier(local_hx()),
                level.mc2_env,
                &level.palette_rgba,
                &level.map_icons,
            );
            level.map_dots.extend(reveal_dots);
            level.map_stamps.extend(reveal_stamps);
            // The rival tag's smooth-motion pair: this snapshot and
            // the one before it (drawn lerped by the sub-tick alpha).
            self.rival_tags_prev = std::mem::replace(&mut self.rival_tags_cur, rival_views);
            self.castle_pos = poses
                .iter()
                .find(|p| p.class == 3 && p.model == 2 && p.player_owned)
                .map(|p| (p.x, p.z));
            level.map_areas = map_areas(w);
            // Beyond Sight's trigger areas: read only while the spell
            // is up — a ripple settling out keeps the areas it had. An
            // area whose trigger fired meanwhile settles out on its
            // own (`sight::SightAreas`).
            if self.cfg.render.enhancement.map_beyond_sight_areas
                && w.beyond_sight_tier(local_hx()).is_some()
            {
                level
                    .sight
                    .merge(sight_areas(w), self.sight_strength <= 0.0);
            }
            // MC2 objective-guide targets (non-optional): the current
            // objective's live world targets → blinking marks + a steer
            // arrow. Empty off-MC2 (mc2_stages empty), so MC1/HW draw
            // nothing.
            level.objective_marks = w
                .mc2_objective_targets(local_hx())
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
                // THE TICK-RATE ARM of the draw set — what runs with
                // smooth motion off. The replay instruments ride here
                // as well as on the frame-rate arm; see
                // `push_replay_instruments` for why that is not
                // optional. At tick rate the carpet pose is the tick's
                // own (alpha = 1, no interpolation to do).
                let mut set = level.billboards.clone();
                let index = level.sprites.as_ref().map(|(i, _)| i);
                let dims = |id: u16| {
                    index
                        .and_then(|i| i.sprites.get(id as usize))
                        .map(|s| (s.width, s.height, s.flags))
                };
                let f = &sim.flyer[local_hx()];
                push_replay_instruments(
                    &mut set,
                    self.replay.as_ref(),
                    subject,
                    level.game,
                    sim.world.as_ref().map_or(entities::HUMAN_CARPET_ROW, |w| {
                        w.human_carpet_row(local_hx())
                    }),
                    (f.x, f.y, f.z, f.yaw),
                    self.chase.bank_now(),
                    &dims,
                );
                push_spectators(&mut set, sim, &[], 1.0, &dims);
                if let Some((portal, k)) = shrink {
                    shrink_portal(&mut set, portal, k);
                }
                r.set_billboards(set);
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
                from: (sim.flyer[local_hx()].x, sim.flyer[local_hx()].z),
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
    ///
    /// NOT while a recording is live: this direct write bypasses the
    /// input channel, so the take could never reproduce it. Deferring
    /// keeps the binding queued in `pending_equip`, which the first
    /// unpaused tick consumes into its RECORDED input — the HUD icon
    /// lags until unpause, which is the price of a replayable take.
    fn flush_equip_if_paused(&mut self) {
        if !self.paused || self.recorder.is_some() || self.record_path.is_some() {
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
            w.equip_hands(local_hx(), l, r);
            if let Some((spell, tier, hand)) = self.pending_mc2_select.take() {
                w.mc2_select_spell(local_hx(), spell, tier, hand);
            }
            if let Some((spell, val)) = self.pending_ring.take() {
                w.spell_ring_set(local_hx(), spell, val);
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
            w.mc2_book_view(local_hx())
                .ring
                .get(s)
                .copied()
                .unwrap_or(0)
        } else {
            w.loadout(local_hx()).ring.get(s).copied().unwrap_or(0)
        }
    }

    /// The in-flight cycle walk (`sub_18DA0` PI:1839-1942): step from
    /// the equipped spell ±1, wrap 0..n, take the first spell that is
    /// BOTH possessed and a member of this button's ring; a full lap
    /// with no qualifier does nothing (the all-unavailable no-op —
    /// vanished MC1 spells and undead-stolen MC2 spells stay members,
    /// they are just skipped). A single-member ring re-selects itself.
    /// The equip carries the spell's STORED level (`array_0x437`).
    /// The MC1/HW campaign's carried quick keys, if it carries any
    /// (`CampaignRun::mc1_quick_keys`) — the running campaign's, or in
    /// a lobby game the one parked for it (`net_campaign`): players
    /// bring their own binds into MP (player ask 2026-10-17). `None`
    /// outside a campaign (replays, `--level`, CLI netplay), on MC2,
    /// and on a campaign with no binds saved — all of which seed from
    /// scratch like retail.
    fn carried_quick_keys(&self) -> Option<[Option<u8>; 10]> {
        self.campaign
            .as_ref()
            .or(self.net_campaign.as_ref())
            .filter(|run| run.id != campaign::CampaignId::Mc2)
            .and_then(|run| run.mc1_quick_keys)
    }

    fn cycle_spell_ring(&mut self, hand: u8, backward: bool) {
        let Some(w) = self.session.as_deref().and_then(|s| s.sim.world.as_ref()) else {
            return;
        };
        let side = hand + 1;
        let (ring, owned, cur, sel) = if self.is_mc2() {
            let bv = w.mc2_book_view(local_hx());
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
            let l = w.loadout(local_hx());
            let mut owned = l.owned;
            if self.cfg.gameplay.cheat.dev_spells {
                owned.fill(true); // the overlay: the bind mints on demand
            }
            let cur = if hand == 0 { l.left } else { l.right };
            (
                l.ring.to_vec(),
                owned.to_vec(),
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
                self.campaign_switch(n, false, event_loop);
            }
            return;
        }
        self.screen = Screen::Map;
        self.confine_map_pointer();
        // One-shot pointer recenter (player-reported 2026-10-17): in
        // flight the pointer is grabbed and only its DELTAS count, so
        // it is left wherever it drifted — usually on a window edge,
        // which on the map is the edge-scroll band, and the map lurched
        // away from the level it had just anchored on. Centred, it
        // scrolls nothing until the player moves it.
        let size = self.view_size();
        self.cursor = (size.0 * 0.5, size.1 * 0.5);
        if let Some(w) = &self.window {
            let _ = w.set_cursor_position(winit::dpi::PhysicalPosition::new(
                self.cursor.0 as f64,
                self.cursor.1 as f64,
            ));
        }
        // Map entry: the carpet parks on the level just played
        // (completed, failed or replayed), the camera anchors, the
        // narrative latch re-arms; the new portal pops on sight.
        if let (Some(wm), Some(save), Some(cur)) = (
            &mut self.worldmap,
            self.campaign.as_ref().and_then(|c| c.save.mc2()),
            current,
        ) {
            wm.enter_visit(save);
            wm.anchor_to(save, Some(cur));
            wm.set_parked(cur);
        }
        // Retail map mode 4: the finished level's stats table first.
        if let (Some(wm), Some(run)) = (&mut self.worldmap, &mut self.campaign) {
            wm.show_stats(std::mem::take(&mut run.map_stats));
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
        // Never from the background (an X11 confinement is global —
        // see `set_grab`); `Focused(true)` re-applies it.
        if let Some(w) = self.window.as_ref().filter(|w| w.has_focus()) {
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
                // A load: retail's load arm clears the just-played
                // record (MI:1567), so the camera glides from the last
                // flag to the pending portal.
                wm.anchor_to(save, None);
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
            self.campaign_switch(n, false, event_loop);
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
            && self.cfg.audio.sound
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
            let replayed = self
                .campaign
                .as_ref()
                .and_then(|c| c.save.mc2())
                .is_some_and(|s| mc2_map_launch_replayed(s, n));
            self.campaign_switch(n, replayed, event_loop);
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
                            wm.anchor_to(save, None);
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
                    wm.anchor_to(save, None);
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
        if self.screen == Screen::Launcher {
            self.launcher_frame(event_loop);
            return;
        }
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
            Screen::Stats => self.stats_screen_frame(dt, event_loop),
            Screen::Launcher => self.launcher_frame(event_loop),
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
    /// 4, 8, 12, 16 and 23, and CUT6 — the ending — after the finale,
    /// 24. The ending is NOT a terminal seam: retail plays it from the
    /// same table on the way back to the map (`PlayInGameFmv_82670`,
    /// MI:4082-4100), and the map then stays open with every portal
    /// conquered — free play, replays and saves included.
    ///
    /// Retail marks each entry `overplayed_5` when it plays and never
    /// resets or persists the flag, so a cutscene shows once per
    /// process; `cutscenes_played` reproduces that.
    fn mc2_cutscene(&mut self, level: u32) -> Option<movie::Cue> {
        if !self.is_mc2() {
            return None;
        }
        let slot = [4u32, 8, 12, 16, 23, 24].iter().position(|&l| l == level)?;
        if std::mem::replace(&mut self.cutscenes_played[slot], true) {
            return None;
        }
        // Skippable by any key, like the intro. ⚖ Player ruling
        // 2026-09-27: every movie is skippable, whatever the listing's
        // `PlayInfoFmv(0, ..)` (MenusAndIntros.cpp:4166) says.
        Some(movie::Cue::new(
            ["cut1", "cut2", "cut3", "cut4", "cut5", "cut6"][slot],
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
            AfterMovie::Stats(then) => self.enter_stats_screen(then, event_loop),
        }
    }

    /// Open the performance screen over the captured card; with no
    /// card or no baked PPERF, go straight on.
    fn enter_stats_screen(&mut self, then: StatsThen, event_loop: &ActiveEventLoop) {
        let card = self
            .stats_card
            .take()
            .filter(|_| self.cfg.render.preference.stats_screen);
        let screen = card.and_then(|c| {
            stats_screen::Mc1Stats::load(&get_baked_directory().join("assets/mc1-ui"), c)
                .map_err(|e| eprintln!("note: performance screen unavailable: {e}"))
                .ok()
        });
        match screen {
            Some(st) => {
                // Like a movie, the screen owns the frame with no
                // level behind it (its sounds and music die here).
                self.teardown_session();
                if let Some(a) = &mut self.audio {
                    a.stop_music();
                }
                self.stats_screen = Some(st);
                self.stats_then = then;
                self.screen = Screen::Stats;
                self.ui_atlas = UiAtlas::None;
                self.set_grab(false);
            }
            None => self.leave_stats_screen(then, event_loop),
        }
    }

    fn leave_stats_screen(&mut self, then: StatsThen, event_loop: &ActiveEventLoop) {
        self.stats_screen = None;
        self.ui_atlas = UiAtlas::None;
        match then {
            StatsThen::Menu => self.enter_main_menu(),
            // Skippable, like every movie (player ruling 2026-09-27).
            StatsThen::Outro => {
                self.play_movies(&[movie::Cue::new("outro")], AfterMovie::Quit, event_loop)
            }
            StatsThen::Quit => event_loop.exit(),
        }
    }

    /// One launcher frame: poll the background bake, act on the
    /// player's choice, and re-upload the frame when it changed.
    fn launcher_frame(&mut self, event_loop: &ActiveEventLoop) {
        let size = self.view_size();
        let Some(l) = &mut self.launcher else {
            event_loop.exit();
            return;
        };
        l.tick();
        match l.take_action() {
            Some(launcher_screen::Action::Exit) => {
                event_loop.exit();
                return;
            }
            Some(launcher_screen::Action::Start(id)) => {
                self.start_from_launcher(id, event_loop);
                return;
            }
            None => {}
        }
        let monitor = self
            .window
            .as_ref()
            .and_then(|w| window_monitor(w))
            .map(|m| (m.size().width, m.size().height));
        l.set_monitor(monitor);
        let (rgba, quads) = l.frame(size, self.cursor, &self.cfg);
        if let Some(r) = &mut self.renderer {
            if let Some(rgba) = rgba {
                r.load_ui_atlas(size.0.max(1.0) as u32, size.1.max(1.0) as u32, &rgba);
                self.ui_atlas = UiAtlas::Launcher;
            } else if self.ui_atlas != UiAtlas::Launcher {
                // Another screen took the atlas slot: compose afresh.
                l.invalidate();
                return;
            }
            r.set_ui_quads(quads);
        }
    }

    /// Apply and persist what the launcher's option rows just set,
    /// exactly as an options-menu change.
    fn launcher_changes(&mut self) {
        let Some(l) = &mut self.launcher else { return };
        for path in l.take_changed() {
            self.apply_option(path);
            if let Some(spec) = self.specs.iter().find(|s| s.cfg_path == path) {
                self.persist_option(spec);
            }
        }
    }

    /// Start the game the launcher picked: its campaign boots exactly
    /// as `--campaign` would (the intro chain, then the retail main
    /// menu), in this same window.
    fn start_from_launcher(&mut self, id: campaign::CampaignId, event_loop: &ActiveEventLoop) {
        let run = match CampaignRun::start(id, None, false) {
            Ok(run) => run,
            Err(e) => {
                eprintln!("error: cannot start the {} campaign: {e}", id.tag());
                event_loop.exit();
                return;
            }
        };
        println!("launcher: starting the {} campaign", id.tag());
        // The campaign carry replaces the instrument (as at a
        // `--campaign` boot).
        self.cfg.dev.plausible_spellbook = false;
        self.campaign = Some(run);
        self.launcher = None;
        let is_mc2 = self.is_mc2();
        // The selection surfaces and the map layout follow the game.
        self.selector = self.cfg.gameplay.enhancement.spell_selector.resolve(is_mc2);
        self.pane = self.selector.ctrl_pane.then(|| {
            if is_mc2 {
                ui::SelectorPane::mc2()
            } else {
                ui::SelectorPane::mc1()
            }
        });
        if let Some(r) = &mut self.renderer {
            r.set_map_layout(if self.selector.map_book {
                mgc_render::MapScreenLayout::Mc1Book
            } else {
                mgc_render::MapScreenLayout::Mc2Split
            });
        }
        if self.cfg.audio.sound || self.cfg.audio.music {
            self.ensure_audio();
        }
        if let Some(w) = &self.window {
            w.set_title(&format!("Magic Carpet — {} campaign", id.tag()));
        }
        // The launcher ran windowed; the game takes the configured mode.
        self.apply_fullscreen();
        self.apply_window_size();
        self.ui_atlas = UiAtlas::None;
        self.screen = Screen::Menu;
        self.boot_intro = true;
    }

    /// One performance-screen frame: CPU-composed 640×400, uploaded
    /// as the UI atlas.
    fn stats_screen_frame(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        let size = self.view_size();
        let Some(st) = &mut self.stats_screen else {
            let then = self.stats_then;
            self.leave_stats_screen(then, event_loop);
            return;
        };
        st.tick(dt);
        if st.done() {
            let then = self.stats_then;
            self.leave_stats_screen(then, event_loop);
            return;
        }
        while st.take_chime() {
            if let Some(a) = &mut self.audio
                && self.cfg.audio.sound
            {
                let (bank, id) = stats_screen::CHIME;
                if let Err(e) = a.play_bank_sample(bank, id, false) {
                    eprintln!("note: stats chime: {e}");
                }
            }
        }
        let (rgba, quads) = st.frame(size);
        let mut q = vec![ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, 1.0])];
        q.extend(quads);
        if let Some(r) = &mut self.renderer {
            r.load_ui_atlas(640, 400, &rgba);
            self.ui_atlas = UiAtlas::Stats;
            r.set_ui_quads(q);
        }
    }

    /// One main-menu frame, dispatched per campaign.
    fn menu_screen_frame(&mut self, dt: f32, event_loop: &ActiveEventLoop) {
        // The netplay lobby replaces the main menu while it is open.
        self.poll_join();
        if self.netlobby.is_some() {
            self.lobby_frame(dt);
        } else if self.mc1_frontend() {
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
                    self.campaign_switch(n, false, event_loop);
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
                m.reset_dialogs();
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
                    self.campaign_switch(n, false, event_loop);
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
                    self.campaign_switch(0, false, event_loop);
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
                Mc1Action::Network => self.open_network_dialog(),
                Mc1Action::Host { port, passphrase } => self.host_lobby(port, passphrase),
                Mc1Action::Join {
                    addr,
                    port,
                    passphrase,
                } => self.join_lobby(addr, port, passphrase),
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
                    // Retail: a successful menu load lands on the map
                    // — but a slot saved MID-LEVEL has just been
                    // resumed into its level by `resume_slot`
                    // (`install_level` left the screen on the level),
                    // and opening the map here tore that session
                    // straight back down: the temple menu was the one
                    // load surface that dropped a mid-level save on
                    // the hub (player-reported 2026-09-30).
                    if self.screen != Screen::Level {
                        self.open_map_screen(event_loop);
                    }
                }
                MenuAction::SetName(name) => {
                    if let Some(s) = self.campaign.as_mut().and_then(|c| c.save.mc2_mut()) {
                        println!("player name: {name}");
                        s.player_name = name;
                    }
                }
                MenuAction::Host { port, passphrase } => self.host_lobby(port, passphrase),
                MenuAction::Join {
                    addr,
                    port,
                    passphrase,
                } => self.join_lobby(addr, port, passphrase),
                MenuAction::Quit => event_loop.exit(),
            }
        }
    }

    /// The confirmed in-level exit: abandon to the campaign hub
    /// (retail MC2 → the world map, nothing recorded; MC1/HW → the
    /// main menu), or leave the app in single-level mode.
    fn confirm_exit(&mut self, event_loop: &ActiveEventLoop) {
        self.exit_confirm = false;
        if self.lobby_game {
            // Everyone — the hosting player too (player 2026-10-17) —
            // leaves the GAME, not the session: the rest play on, and
            // his lobby waits for the game to finish
            // (`Lockstep::finish`; its Exit goes elsewhere).
            println!("net: left the game");
            self.net_game_over();
            return;
        }
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
        if grab && self.menu_up() {
            return;
        }
        // An UNFOCUSED window never takes the pointer: an X11 grab
        // succeeds globally and steals it from the focused app. Two
        // netplay peers on one desktop hit exactly that — Play starts
        // both levels at once, the background window grabbed the mouse
        // while the focused one kept the keyboard, and Esc (delivered
        // to the focused one) could free neither (player-reported
        // 2026-10-15). Arm the wish instead: the focus gain completes it.
        if grab && !window.has_focus() {
            if !self.boot_grab {
                println!("pointer: the grab waits for this window's focus");
            }
            self.boot_grab = true;
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
        if matches!(
            self.screen,
            Screen::Map | Screen::Movie | Screen::Stats | Screen::Launcher
        ) || (self.screen == Screen::Menu && self.is_mc2())
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
            && !self.menu_up()
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
        let mut dt = raw_dt.min(0.25);
        self.last_frame = now;
        // A network game's standings (banishment, the last wizard).
        if self.net.is_some() {
            self.net_standings();
        }
        // A lobby client: the host ended the game (the lobby reopened).
        if self.lobby_game && self.net.as_ref().is_some_and(net::Lockstep::ended) {
            println!("net: the host ended the game — back to the lobby");
            self.net_game_over();
        }
        // `--film`: the FILM CLOCK replaces wall time for the whole
        // frame (`film.rs`) — every clock below that reads `dt` (the
        // effect clock, toasts, the chase cam) and the sim accumulator
        // advance by exactly what the frame plan says, so the same
        // shot renders the same frames every time. Planned only while
        // a level session is on screen.
        let film_step = match (&self.film, self.screen, self.session.as_deref()) {
            (Some(_), Screen::Level, Some(sess)) => {
                let tick = sess.sim.tick;
                self.film.as_mut().map(|f| f.step(tick))
            }
            _ => None,
        };
        match &film_step {
            Some(film::Step::Seek { turns }) => dt = TICK_DT * *turns as f32,
            Some(film::Step::Frame { dt: frame_dt, .. }) => dt = *frame_dt,
            Some(film::Step::Done) => {
                if let Some(f) = &self.film {
                    println!("{}", f.summary());
                }
                event_loop.exit();
                return;
            }
            None => {}
        }
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
        // watch their cost. Never in a film: the counter is wall
        // time, the one thing a filmed frame must not contain.
        if self.cfg.render.debug.fps && self.film.is_none() {
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
        // Under `--film` the plan SETS the accumulator (a filmed frame
        // is always 1/rate of a turn, whatever the game speed; a seek
        // is a burst of whole turns), and lifts the per-frame tick
        // cap to the burst.
        let mut film_burst = None;
        match &film_step {
            Some(film::Step::Seek { turns }) => {
                self.accumulator = TICK_DT * *turns as f32;
                film_burst = Some(*turns as u32);
            }
            Some(film::Step::Frame { accumulator, .. }) => {
                self.accumulator = *accumulator;
                film_burst = Some(1);
            }
            _ => self.accumulator += dt * speed,
        }
        if self.paused {
            // ⭐ A PAUSED TURN STILL DRAWS. Retail's pause test is the
            // SECOND statement of `sub_41780_41AC0` (:52197) — the
            // global LCG step is the first — so a paused frame burns
            // one draw and skips the body. Dropping the accumulated
            // time (what this did before) skipped the draw too, which
            // put the port one draw per paused frame behind retail
            // and made every subsequent random outcome differ.
            // Measured on mc1l6's 1,602-frame pause: `draws=1` on
            // every boundary, zero pool changes. See
            // [`World::tick_paused`].
            //
            // Still capped, and still drained to zero: resuming must
            // not burst through missed ticks. (The abandon-confirm
            // dialog does NOT freeze — retail keeps the world
            // simulating under it, EventsFunctions.cpp:31796; the
            // dialog only owns the input. P still pauses if wanted.)
            let cap = ((2.0 * speed).ceil() as u32).max(4);
            let mut turns = (self.accumulator / TICK_DT) as u32;
            self.accumulator = 0.0;
            if turns > cap {
                turns = cap;
            }
            // Netplay: a pause starves the lockstep for every peer, and
            // burns no draws — a draw on one peer only would fork the
            // worlds.
            if turns > 0 && self.net.is_none() {
                if let Some(w) = sess!(self).sim.world.as_mut() {
                    for _ in 0..turns {
                        w.tick_paused();
                    }
                }
            }
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
        let max_ticks = film_burst.unwrap_or(((2.0 * speed).ceil() as u32).max(4));
        let mut ran = 0u32;
        while self.accumulator >= TICK_DT {
            // Netplay: no tick steps before its bundle of every seat's
            // input exists; a starved turn keeps its time for later.
            let net_inputs = match self.net_turn() {
                NetTurn::Local => None,
                NetTurn::Starved => {
                    self.accumulator = self.accumulator.min(TICK_DT * max_ticks as f32);
                    break;
                }
                NetTurn::Ready(inputs, left) => {
                    // A player who left is banished on this tick, on
                    // every peer alike (`Lockstep::left_at`).
                    if let Some(w) = sess!(self).sim.world.as_mut() {
                        for seat in left {
                            w.depart(mgc_sim::Hx(seat));
                        }
                    }
                    Some(inputs)
                }
            };
            self.accumulator -= TICK_DT;
            {
                let sess = sess!(self);
                sess.prev_flyer = sess.sim.flyer[local_hx()];
                sess.prev_flyers = sess.sim.flyers();
            }
            // Replay playback replaces the live input wholesale; when
            // the take ends, control hands back to the player.
            let input = match (&net_inputs, self.replay.take()) {
                (Some(inputs), _) => inputs[usize::from(local_hx().0)],
                (None, Some(mut d)) => {
                    let next = d.next(&mut sess!(self).sim);
                    if d.take_anchored() {
                        // An anchor re-imported the world — stale
                        // (slot, generation) pose pairs must not
                        // survive it (the resume path's law).
                        let sess = sess!(self);
                        sess.prev_flyer = sess.sim.flyer[local_hx()];
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
                            // A film of the take ends with it (the
                            // frame in flight is still captured).
                            if let Some(f) = &mut self.film {
                                f.end();
                            }
                            // Hand control back cleanly: drop the
                            // stale live-input accumulators the
                            // replay never drained.
                            self.mouse = MouseAccum::default();
                            self.stick = VirtualStick::default();
                            self.tick_input()
                        }
                    }
                }
                (None, None) => self.tick_input(),
            };
            // `--record`: t/hash describe the PRE-step state; the
            // input is what the step consumes (the phase convention,
            // docs/RECORDING.md).
            // `--replay --record`: the driver has produced this
            // tick's input, so the world is now the take's world —
            // the first legal moment to snapshot it (and a re-anchor
            // past that point ends the recording).
            if self.replay.is_some() {
                self.cut_recorder_at_reanchor();
                self.begin_deferred_recorder();
                // `--replay --record`: the driver just applied this
                // row's recorded live-option events — carry them into
                // the re-recording alongside the app's own.
                if self.record_path.is_some() || self.recorder.is_some() {
                    let set = self.replay.as_mut().map(|d| d.take_row_set());
                    if let Some(set) = set {
                        self.rec_toggles.extend(set);
                    }
                }
            }
            let pre = if self.recorder.is_some() {
                // Live-option events queued since the last row: their
                // sim writes already landed, so THIS row's pre-step
                // hash includes them — the row carries the events and
                // a replay re-applies them before grading it.
                let set = std::mem::take(&mut self.rec_toggles);
                let sess = sess!(self);
                Some((sess.sim.tick, sess.sim.state_hash(), set))
            } else {
                None
            };
            // The request-time listener (see `audio_tick`): the pose
            // this step STARTED from, captured before the step can
            // teleport it.
            let requested_at = if request_time_listener() {
                let f = &sess!(self).sim.flyer[local_hx()];
                Some(mgc_sim::engine::world::PlayerPose::from_tiles(
                    f.x, f.y, f.z, f.yaw, f.pitch, 0.0,
                ))
            } else {
                None
            };
            match &net_inputs {
                Some(inputs) => sess!(self).sim.step_seats(inputs),
                None => sess!(self).sim.step(&input),
            }
            if let Some(d) = self.replay.as_mut() {
                d.grade(&sess!(self).sim);
            }
            if let (Some(r), Some((t, hash, set))) = (self.recorder.as_mut(), pre) {
                if let Err(e) = r.record(t, &input, hash, &set) {
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
            // Taken EVERY tick, whatever the smooth-motion toggle:
            // the pose pair feeds the smooth pass AND the enhanced
            // fire/lightning emitters (`apply_effects`), which run
            // with smooth motion off too (player 2026-10-10 — the
            // effects were needlessly tied to the toggle).
            {
                let sess = sess!(self);
                if let Some(w) = &sess.sim.world {
                    sess.pose_prev = std::mem::take(&mut sess.pose_cur);
                    sess.pose_cur = w.live_poses(local_hx());
                    // PROTOTYPE fire: track blast drivers (one
                    // tick per step; dead ones keep aging so
                    // their smoke choreography finishes).
                    sess.fire_blasts.update(&w.mc1_blasts(), 1.0);
                }
            }
            // The mixer flush is per-tick like the original's
            // (fade ramps are tick-denominated).
            self.audio_tick(requested_at);
            ran += 1;
            if ran >= max_ticks {
                self.accumulator = 0.0;
                break;
            }
            // Netplay: a castle-less death restarts the level after the
            // burst (below) — end the burst on its tick, so every peer
            // rebuilds before the same next tick.
            if self.net.is_some()
                && sess_ref!(self)
                    .sim
                    .world
                    .as_ref()
                    .is_some_and(|w| w.restart_pending())
            {
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
                let owned = w.loadout(local_hx()).owned;
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
            // ⭐ THE PERMADEATH RESYNC (player-ruled 2026-09-07).
            // Under a RETAIL take, a pristine rebuild is the wrong
            // world: retail's reload re-reads the level over its own
            // live heap residue with the RNG continuing mid-stream,
            // and the port's native level init lands somewhere else
            // entirely (mc1l48 at its own phase: 942 live / 22
            // families against retail's 943 / 40). Re-anchor on the
            // recording instead — the driver imports retail's own
            // closure at the next boundary. A PORT take restarts the
            // ordinary way; that is exactly what it recorded.
            //
            // ⚠ A retail replay never reaches here: the driver polls
            // `take_restart` itself, at the top of `next`, which is
            // per STEP rather than per FRAME (see there — polling
            // after this loop was a speed-dependent bug). This poll
            // is the LIVE path's.
            match self.replay.as_mut().filter(|d| d.is_retail()) {
                Some(d) => d.arm_resync(),
                None => self.restart_level(),
            }
        }

        // The frame's sub-tick lerp fraction — EVERY frame-time lerp
        // below (camera position/yaw/aim/roll, rival tags, crosshair
        // blink, entity poses) reads this one value. With smooth
        // motion OFF it is pinned to the CURRENT tick, so the view
        // steps at tick rate together with the world, as retail drew
        // it. (Until 2026-10-10 only the entity poses were gated and
        // the camera kept gliding: a stepped world under a smooth,
        // half-a-tick-behind camera — the disparity the player saw.)
        let alpha = if self.cfg.render.enhancement.smooth_motion {
            self.accumulator / TICK_DT
        } else {
            1.0
        };
        // The smooth pass re-sets the renderer's world drawables at
        // this frame's lerp fraction (sync_world set the tick-rate
        // ones; with the toggle off those stand). The effects pass
        // runs either way.
        self.apply_smooth_motion(alpha);
        self.apply_effects(alpha);
        let sess = sess_ref!(self);
        let (a, b) = (&sess.prev_flyer, &sess.sim.flyer[local_hx()]);
        // Positions may wrap across the 256-tile seam; take the
        // short way around for interpolation (`interp_carpet`).
        let (carpet_x, carpet_y, carpet_z, carpet_yaw) = interp_carpet(a, b, alpha);
        // The knock camera kick (remc1 :52433-37): the view
        // pitches down ~v_22/8 engine-angle units while a
        // buffet/knock is live (the kraken drag feedback).
        let kick = sess
            .sim
            .world
            .as_ref()
            .map(|w| w.knock_magnitude(local_hx()) as f32 / 8.0 * (std::f32::consts::TAU / 2048.0))
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
        // THE EYE: retail's own viewpoint, and what everything that
        // wants the PLAYER rather than the picture reads (the aim ray,
        // the coordinate overlay).
        let eye = CameraView {
            x: carpet_x,
            y: carpet_y + mgc_sim::EYE_LIFT,
            z: carpet_z,
            yaw: carpet_yaw,
            pitch: view_pitch - kick,
            roll: view_roll,
            fov_y: FOV_Y,
        };
        // THE RENDER CAMERA. Live play and `--firstperson` render from
        // the eye; `--thirdperson` swings it onto a boom behind the
        // carpet so the flight can be watched (`camera.rs`). The boom
        // frames the CARPET PLANE, not the eye — half a tile at three
        // tiles' range is a sixth of the frame.
        // The filmed MC2 exit (`mc2_fancy_exit`) takes the chase boom
        // too, floating out of the eye over `FANCY_EXIT_FLOAT` seconds;
        // under a `--thirdperson` replay the boom is already up.
        let chase_view = self.third_person && self.replay.is_some();
        let end_view = self.fancy_exit_view();
        let was_arrived = self.fancy_arrived_t.is_some();
        self.fancy_arrived_t = match end_view {
            Some(v) if v.arrived => Some(self.fancy_arrived_t.map_or(0.0, |t| t + dt)),
            _ => None,
        };
        // The warp: the teleporter pad's own sample (22, the pad's
        // `snd_player(22)`) on the frame the carpet vanishes — an app
        // sound, so the sim's sound stream stays retail's.
        if !was_arrived
            && self.fancy_arrived_t.is_some()
            && self.cfg.audio.sound
            && let Some(a) = &mut self.audio
        {
            let listener = mgc_audio::Listener {
                pos: (0, 0, 0),
                yaw: 0,
            };
            a.event(SND_TELEPORT_WARP, mgc_audio::Source::Player, &listener);
        }
        let float = match end_view {
            Some(_) if chase_view => None,
            Some(_) => {
                let t = match self.fancy_exit_t {
                    Some(t) => t + dt,
                    None => {
                        // Seat a fresh boom: its first step lands
                        // straight on the chase pose we blend toward.
                        self.chase = camera::ChaseCam::default();
                        0.0
                    }
                };
                self.fancy_exit_t = Some(t);
                Some(t)
            }
            None => {
                self.fancy_exit_t = None;
                None
            }
        };
        let cam = if chase_view || float.is_some() {
            let aspect = self
                .window
                .as_ref()
                .map(|w| {
                    let s = w.inner_size();
                    s.width as f32 / s.height.max(1) as f32
                })
                .unwrap_or(mgc_render::NATIVE_W / mgc_render::NATIVE_H);
            // The subject's bank off the recorded motion — the
            // enhanced mover's own law, smoothed — steps with the
            // camera so the sprite leans as the view swings.
            let bank = camera::motion_bank((a.x, a.z, a.yaw), (b.x, b.z, b.yaw), TICK_DT);
            self.chase.bank(bank, dt);
            let boom = third_person_camera(&mut self.chase, &eye, carpet_y, &sess.sim, aspect, dt);
            match float {
                Some(t) => camera::blend_view(&eye, &boom, t / FANCY_EXIT_FLOAT),
                None => boom,
            }
        } else {
            eye
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
            let loadout = w.loadout(local_hx());
            let vitals = w.vitals(local_hx());
            let is_mc2 = matches!(sess.level.game, mgc_sim::ids::GameId::Mc2);
            let mc2_book = is_mc2.then(|| w.mc2_book_view(local_hx()));
            // Retail's HUD blink bank (str_93/colorIndex_121[k] =
            // (Turn/k)&1, :48548-53 / EF:37563-66): index [1] =
            // Turn&1 drives the alert marbles, hit flashes and the
            // spell-hand expiry blink; index [2] = (Turn/2)&1 the
            // goal-tick colour ramp. Two phases, not one.
            let alert_blink = sess.sim.tick % 2 == 0;
            let goal_blink = (sess.sim.tick / 2) % 2 == 0;
            let (mut quads, hovered) = if self.book_open() {
                if self.selector.map_book {
                    ui::book_quads(
                        assets,
                        &loadout,
                        &self.quick_binds,
                        alert_blink,
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
                        goal_blink,
                        is_mc2,
                        mc2_book.as_ref(),
                        self.cfg.gameplay.cheat.dev_spells,
                        hud_team_colors(w.human_wizard_seat(local_hx()), &sess.level),
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
                    let mut cost_tier = [[0u32; 3]; 26];
                    let mut max_level = [0u8; 26];
                    let mut sel = [0u8; 26];
                    let mut xp = [0i32; 26];
                    let mut xpos = [[0i32; 3]; 26];
                    let mut ring = [0u8; 26];
                    let mut expiring = [false; 26];
                    let mut known = [false; 26];
                    let mut bound = [loadout.left, loadout.right];
                    if mc2 {
                        // The campaign carry IS retail's known list
                        // (`array_0x3E9`, never cleared): a carried
                        // spell the book lacks — withheld by the
                        // level's block, or lost this level — draws
                        // the 0xA6 relief (EF:22557-61).
                        let carried = self
                            .campaign
                            .as_ref()
                            .and_then(|c| c.save.mc2())
                            .map(|save| save.book().owned);
                        // The native spell book: ownership,
                        // per-spell LEVEL (the
                        // SpellLevels tier ceiling), selected
                        // tiers, real GetSpellManaCost costs
                        // and the quick-slot binds all come
                        // from the sim's class-15 machinery.
                        let bv = Some(w.mc2_book_view(local_hx()));
                        if let Some(bv) = bv {
                            for s in 0..n {
                                owned[s] = bv.owned[s] || self.cfg.gameplay.cheat.dev_spells;
                                // Cave-In off-cave: the plain empty
                                // box, whatever the book says
                                // (EF:22429; selection refused,
                                // PlayerInput.cpp:849).
                                let cave_gated = s == 25 && !bv.cave;
                                if cave_gated {
                                    owned[s] = false;
                                }
                                known[s] =
                                    !owned[s] && !cave_gated && carried.is_some_and(|c| c[s]);
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
                                cost_tier[s] = bv.cost_tier[s];
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
                                expiring[s] = bv.expiring[s];
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
                            // The G overlay offers every spell; the
                            // sim mints on bind (`equip_hands`).
                            owned[s] = loadout.owned[s] || self.cfg.gameplay.cheat.dev_spells;
                            castable[s] = loadout.bindable[s];
                            castable_tier[s] = [loadout.bindable[s]; 3];
                            cost[s] = mgc_sim::mc1::spells::SPELLS[s].possess_mana;
                            cost_tier[s] = [cost[s]; 3];
                            max_level[s] = pane.levels - 1;
                            sel[s] = self.spell_levels[s];
                            ring[s] = loadout.ring[s];
                            expiring[s] = loadout.expiring[s];
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
                        cost_tier: &cost_tier[..n],
                        xp: &xp[..n],
                        xpos: &xpos[..n],
                        expiring: &expiring[..n],
                        known: &known[..n],
                        blink: alert_blink,
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
                    // The human's row is HIS seat (0 except under
                    // `--seat`); every other slot is a rival's.
                    let seat = w.human_wizard_seat(local_hx()).min(7) as usize;
                    // The human's row. Retail's in-play flag
                    // (+6) clears only when the wizard is OUT of
                    // the level (castle-less death / banishment,
                    // MC1 :48585/:55622, MC2 EF:37663/:37777) —
                    // never on a temporary death, so the row rides
                    // through the respawn wait. Name: the
                    // campaign's entered name (retail overrides
                    // the slot-0 table name with the player
                    // string), else the table default. A network
                    // game's banishment takes the row off too.
                    if !vitals.lost && !w.banished(local_hx()) {
                        let own = self.net_names.iter().find(|(s, _)| usize::from(*s) == seat);
                        let name = own
                            .map(|(_, n)| n.clone())
                            .or_else(|| {
                                self.campaign.as_ref().and_then(|c| {
                                    c.save
                                        .mc1()
                                        .map(|s| s.name.clone())
                                        .or_else(|| c.save.mc2().map(|s| s.player_name.clone()))
                                        .filter(|n| !n.trim().is_empty())
                                })
                            })
                            .unwrap_or_else(|| {
                                if is_mc2 {
                                    mgc_sim::mc2::rivals::MC2_RIVAL_NAMES[seat].into()
                                } else {
                                    mgc_sim::mc1::rivals::RIVAL_NAMES[seat].into()
                                }
                            });
                        rows[seat] = Some(ui::RosterEntry {
                            name,
                            mana: loadout.mana_max,
                            kills: w.player_kill_row(local_hx()),
                            box_c: colors[seat].0,
                            text_c: colors[seat].1,
                        });
                    }
                    let mut views = w.rival_views();
                    views.extend(w.human_views(local_hx()));
                    name_humans(&self.net_names, &mut views);
                    for r in views {
                        let slot = r.slot as usize;
                        if !r.eliminated && slot < 8 && slot != seat {
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
                if sess.sim.carpet_mc2[local_hx()].mobilize > 0 && assets.has_web() {
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
                let ms = sess.sim.carpet_mc2[local_hx()].move_speed;
                if ms > 0 {
                    let count = (171.0 * ms as f32 / 3.0 + 85.0).min(256.0);
                    let a = count * 56.0 / 65536.0;
                    quads.push(ui::solid([0.0, 0.0, size.0, size.1], [0.05, 0.42, 0.08, a]));
                }
                quads.extend(ui::vitals_quads(
                    &vitals,
                    size.0,
                    size.1,
                    self.cfg.render.debug.grace_meter,
                ));
            }
            // Falling or dead: retail draws NO HUD at all — the whole
            // in-game UI sits in the `life >= 0` arm and a dead wizard
            // gets only the pause menu (EF:21604-21623; MC1's twin
            // likewise). Everything above goes; the pause indicator,
            // mini-menu, options and exit dialog below stay.
            let player_down = vitals.state != mgc_sim::engine::world::LifeState::Alive;
            if player_down {
                quads.clear();
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
                        // The anchor rides the wizard sprite's real TOP
                        // (`Renderer::sprite_top` — the drawn quad's
                        // top edge along the pitched up axis), then
                        // retail's 20px lift (GameRenderHD.cpp:2841):
                        // the tag clears the head at every distance and
                        // pitch. It used to sit a fixed 0.6 tiles up the
                        // WORLD axis — on the face when near, and when
                        // looking down (player 2026-10-17). No sprite
                        // drawn there: that fallback.
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
                            let top = self
                                .renderer
                                .as_ref()
                                .and_then(|rd| rd.sprite_top(&cam, x, alt, z))
                                .unwrap_or([0.0, WIZ_TOP, 0.0]);
                            let Some((sx, sy)) = mgc_render::world_to_screen(
                                &cam,
                                size.0,
                                size.1,
                                x + top[0],
                                alt + top[1],
                                z + top[2],
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
                                &r.name,
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
            // The filmed MC2 exit has left the eye and the controls
            // are seized — nothing to aim.
            if (want_cross || want_hints)
                && !self.book_open()
                && self.fancy_exit_t.is_none()
                && vitals.state == mgc_sim::engine::world::LifeState::Alive
            {
                let f = &sess.sim.flyer[local_hx()];
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
                // ⚠ THE AIM RAY IS THE PLAYER'S, NOT THE PICTURE'S.
                // Under `--thirdperson` the render camera has left the
                // carpet, so every term here reads `eye` — only the
                // PROJECTION below takes `cam`.
                let (neutral_yaw, pose_yaw) = match self.cfg.controls.models.thrust {
                    config::ThrustModel::Classic => (eye.yaw, f.yaw),
                    config::ThrustModel::Enhanced => {
                        let lead = (sess.sim.aim_yaw(local_hx()) + self.mouse.yaw - eye.yaw)
                            .clamp(-mgc_sim::LEAD_MAX, mgc_sim::LEAD_MAX);
                        let a = eye.yaw + lead;
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
                        eye.x + sy * cp * AIM_D,
                        eye.y + sp * AIM_D,
                        eye.z - cyaw * cp * AIM_D,
                    )
                } else {
                    None
                };
                let locks = if want_hints {
                    let pose = mgc_sim::engine::world::PlayerPose::from_tiles(
                        f.x, f.y, f.z, pose_yaw, f.pitch, 0.0,
                    );
                    w.aim_preview(local_hx(), pose).map(|l| {
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
                // The toast STACK, oldest on top, one line each in its
                // own ink (retail MC1 draws every wizard's live slot
                // on its own line, remc1 :26508-60).
                let mut toast_lines = 0usize;
                {
                    let (ax, ay) = assets.hud_notification_anchor();
                    // Uniform HUD scale (`ui::HudFrame`): the
                    // toast rides under the LEFT-anchored panel
                    // group, so its anchor is native×s. The font
                    // runs at 2× because FONT1 is 320-native.
                    let hud_s = ui::HudFrame::new(size.0, size.1).s;
                    let font_s = 2.0 * hud_s;
                    let lh = assets.font_line_height() * font_s;
                    for (msg, color) in w.notifications_for(local_hx()) {
                        // A network game's given names over the table
                        // names ("ZANZAMAR has been banished…").
                        let mut msg = msg.to_string();
                        for &(seat, ref name) in &self.net_names {
                            let table = if is_mc2 {
                                mgc_sim::mc2::rivals::MC2_RIVAL_NAMES[usize::from(seat.min(7))]
                            } else {
                                mgc_sim::mc1::rivals::RIVAL_NAMES[usize::from(seat.min(7))]
                            };
                            msg = msg.replace(table, name);
                        }
                        let msg = msg.as_str();
                        let tint = [
                            color[0] as f32 / 255.0,
                            color[1] as f32 / 255.0,
                            color[2] as f32 / 255.0,
                            1.0,
                        ];
                        let y = ay * hud_s + toast_lines as f32 * lh;
                        quads.extend(assets.text_quads(msg, ax * hud_s, y, tint, font_s));
                        toast_lines += 1;
                    }
                }
                // The MC1/HW WIN message (:26480-26505):
                // while the win flag holds, the two-line
                // black-ink message persists at the pane top
                // — ETEXT.DAT entries 60/61. Retail
                // colour-cycles the ink unless zoomed out —
                // the static black remap slot [1] is the
                // baseline.
                if !is_mc2 && w.completed(local_hx()) && !w.player_dead(local_hx()) {
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
                    // overlaps them). Live toasts own the top
                    // rows; the win block steps below them.
                    let line = |idx: usize, fallback: &str| -> String {
                        match sess.level.etext.get(idx) {
                            Some(s) if !s.is_empty() => s.clone(),
                            _ => fallback.to_string(),
                        }
                    };
                    let msg = format!(
                        "{}{}\n{}",
                        "\n".repeat(toast_lines),
                        line(60, "World restored."),
                        line(61, "Press the space bar to continue."),
                    );
                    quads.extend(assets.text_quads(&msg, ax * hud_s, ay * hud_s, black, font_s));
                }
                // The MC2 objective TEXTBOX (the retail frame +
                // red letters, DrawCurrentObjectiveTextbox_30630)
                // replaces the old white-subtitle presentation.
                // Two feeds, hold wins (retail has ONE box + one
                // counter): held O = the LIVE objective on demand
                // (PlayerInput.cpp:393-98, works regardless of the
                // subtitles setting — a control, not presentation);
                // else the declare-time dwell (`subtitle`, 200
                // frames like retail's = 200). Menu open suppresses
                // it (GameUI.cpp:683 clears the counter).
                let box_text = if self.objective_held && self.menu.is_none() {
                    let is_mc2 = matches!(sess.level.game, mgc_sim::ids::GameId::Mc2);
                    is_mc2
                        .then(|| {
                            let (row, _) = w.mc2_objective_view();
                            let seg = if w.completed(local_hx()) {
                                9
                            } else {
                                row as u8 + 1
                            };
                            mc2_narration_etext(sess.level.level_number, seg)
                                .and_then(|idx| sess.level.etext.get(idx))
                                .filter(|s| !s.is_empty())
                                .cloned()
                        })
                        .flatten()
                } else {
                    self.subtitle.as_ref().map(|(t, _)| t.clone())
                };
                if let Some(text) = box_text {
                    let hud_s = ui::HudFrame::new(size.0, size.1).s;
                    let font_s = 2.0 * hud_s;
                    // Retail wraps toward a 320-of-640 text width
                    // (ComputeTextboxSizesFromTextWords GU:3479).
                    let max_w = size.0 * 0.55 / font_s;
                    let lines = wrap_font_text(assets, &text, max_w);
                    quads.extend(ui::objective_box_quads(assets, &lines, size.0, size.1));
                }
            }
            // The end-of-game fadeout: the MC2 ending's
            // sim-side fade (endGameSeq phase 11) under the
            // app's own post-victory fade; at full black the
            // game ends (quit, no stats/menu — deliberate).
            if w.won() && !self.won_handled {
                // The victory breadcrumb — and the campaign-
                // stitching hook consuming the same signal:
                // record the completion, pick the next step.
                // Nothing is written to disk — saving is the
                // player's act. Single-level mode still
                // just fades out. Latched — the fade being
                // consumed (the map screen) must not refire
                // it.
                self.won_handled = true;
                println!("{} completed", sess.level.label);
                // The performance screen's numbers (MC1/HW — MC2's
                // table rides the save; see `campaign_complete`).
                if w.game() != mgc_sim::ids::GameId::Mc2 {
                    let st = w.level_stats(local_hx());
                    use mgc_sim::engine::stats::fmt10;
                    let [castle, houses, unclaimed] = st.mana_rows();
                    println!(
                        "stats: killed {}% (retail {}%: {} of census {}) \
                         deaths you/rivals/nature {}/{}/{} alive {} · \
                         accuracy {}% · spells {}% (retail {}%) · mana {}% (retail {}%) \
                         castle/dwellings/unclaimed {castle}/{houses}/{unclaimed} · \
                         overall {}% (retail {}%)",
                        fmt10(st.cleared_pct10()),
                        st.killed_pct,
                        st.kills,
                        st.census,
                        st.deaths_player(),
                        st.deaths_rivals(),
                        st.deaths_environment(),
                        st.alive,
                        fmt10(st.offensive_accuracy_pct10()),
                        fmt10(st.spells_fixed_pct10()),
                        st.spells_pct,
                        fmt10(st.mana_pct10()),
                        st.mana_pct,
                        fmt10(st.overall_enhanced_pct10()),
                        st.overall_pct,
                    );
                    self.stats_card = Some(stats_screen::StatsCard::new(
                        st,
                        sess.sim.tick,
                        sess.level.level_number as usize,
                        w.game() == mgc_sim::ids::GameId::Mc1Hw,
                        &sess.level.etext,
                    ));
                }
                if let Some(run) = &mut self.campaign {
                    campaign_complete(run, sess.level.level_number, sess.sim.tick, w);
                    // MC1/HW quick-key carry (app-side, so not in
                    // `campaign_complete`, which sees only the world).
                    if run.id != campaign::CampaignId::Mc2 {
                        run.mc1_quick_keys = quick_keys_carry(&self.quick_binds);
                    }
                    // MC2 only: the map, or a secret level first.
                    if run.id == campaign::CampaignId::Mc2
                        && matches!(
                            run.next,
                            Some(campaign::NextStep::MapScreen | campaign::NextStep::Level(_))
                        )
                        && self.cfg.render.preference.stats_screen
                    {
                        run.map_stats.push(sess.level.level_number);
                    }
                }
                self.quit_fade = Some(0.0);
            }
            // The filmed exit's flash: white on the arrival tick,
            // gone in `FANCY_EXIT_FLASH` seconds — drawn UNDER the
            // fade, which is just starting and darkens it as it goes.
            if let Some(t) = self.fancy_arrived_t {
                let a = FANCY_EXIT_FLASH_PEAK * (1.0 - t / FANCY_EXIT_FLASH);
                if a > 0.0 {
                    quads.push(ui::solid([0.0, 0.0, size.0, size.1], [1.0, 1.0, 1.0, a]));
                }
            }
            let fade = w.end_fade(local_hx()).max(self.quit_fade.unwrap_or(0.0));
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
            // Reads the interpolated EYE (never the render
            // camera — `--thirdperson` moves that off the
            // carpet), so it glides with the picture while
            // still reporting the CARPET plane,
            // backing out the eye lift the camera rides
            // (`mgc_sim::EYE_LIFT`): the floor/band numbers this
            // readout speaks in are carpet-relative.
            // None of the debug readouts below belong in a `--film`
            // frame (the fps line is already muted at its source).
            let debug_lines = self.film.is_none();
            if self.cfg.render.debug.coords && debug_lines && assets.has_font() {
                let g = sess.sim.ground_height(eye.x, eye.z);
                let xe = (eye.x.rem_euclid(256.0) * 256.0) as u16;
                let ye = (eye.z.rem_euclid(256.0) * 256.0) as u16;
                let ze = ((eye.y - mgc_sim::EYE_LIFT) * 256.0).round() as i32;
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
                && debug_lines
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
            // since t=N, always on while a take drives the session —
            // except on film, where it is pure nuisance.
            if let Some(d) = &self.replay
                && debug_lines
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
            // A risen ghost watches in full colour (player ruling
            // 2026-10-17); its corpse stays dead underneath.
            let risen = sess.sim.world.as_ref().is_some_and(|w| w.risen(local_hx()));
            let grey = if risen {
                0.0
            } else {
                death_greyscale(vitals.state, sess.sim.tick)
            };
            if let Some(r) = &mut self.renderer {
                // Retail's black-and-white palette over the death fall
                // and the corpse (`death_greyscale`).
                r.set_greyscale(grey);
                // The screen-space map markers draw above the fade
                // quad in flight — hand the renderer the fade level
                // so they dim in step (set_overlay_fade).
                r.set_overlay_fade(fade);
                r.set_ui_quads(quads);
            }
        }
        if let Some(f) = &mut self.quit_fade {
            *f += 1.0 / 48.0;
            if *f >= 1.25 && self.lobby_game {
                // A lobby game's end: everyone back to the lobby.
                self.quit_fade = None;
                self.net_game_over();
            } else if *f >= 1.25 {
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
                            self.play_movies(
                                &[win],
                                AfterMovie::Stats(StatsThen::Menu),
                                event_loop,
                            );
                        } else {
                            // MC2's direct level chain (the
                            // demon-mouth secret dive).
                            self.campaign_switch(n, false, event_loop);
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
                        // MC1/HW's ending movie (OUTRO.DAT),
                        // then out. MC2 never lands here: its
                        // ending CUT6 is a map-bound cutscene
                        // (`mc2_cutscene`) and the campaign
                        // continues as free play.
                        println!("campaign complete!");
                        self.quit_fade = None;
                        // The last level's performance screen, then
                        // the outro.
                        self.enter_stats_screen(StatsThen::Outro, event_loop);
                    }
                    // Single-level mode: the performance screen (when
                    // one was captured), then out.
                    None => {
                        self.quit_fade = None;
                        self.enter_stats_screen(StatsThen::Quit, event_loop);
                    }
                }
            }
        }
        // The fade routing above may have torn the session
        // down (won → menu/map) — the tick clock must come
        // from whatever session remains, if any.
        let anim_tick = self.session.as_deref().map(|s| s.sim.tick % 4096);
        // Beyond Sight's trigger-area ripple swells and settles with
        // the spell (wall clock: it keeps moving while paused).
        let sight_up = self.cfg.render.enhancement.map_beyond_sight_areas
            && self
                .session
                .as_deref()
                .and_then(|s| s.sim.world.as_ref())
                .is_some_and(|w| w.beyond_sight_tier(local_hx()).is_some());
        let sight_step = dt / sight::FADE_SECS;
        self.sight_strength = if sight_up {
            (self.sight_strength + sight_step).min(1.0)
        } else {
            (self.sight_strength - sight_step).max(0.0)
        };
        // …and each area with its own trigger.
        let sight_now = self.session.as_deref_mut().map(|s| {
            s.level.sight.ease(dt);
            s.level.sight.areas()
        });
        if let Some(r) = &mut self.renderer {
            if let Some(areas) = &sight_now {
                r.set_sight_areas(areas);
            }
            r.set_sight_mist(self.sight_strength, self.effect_time);
            // Animation clock: sim ticks are the original's game
            // turns; wrapped so f32 stays exact (see set_anim_turn).
            if let Some(t) = anim_tick {
                r.set_anim_turn(t as f32 + alpha);
            }
            match r.render(&cam) {
                Ok(()) | Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {}
                Err(e) => eprintln!("render: {e}"),
            }
            // `--film`: the same frame again into the capture texture
            // and out to disk. Drawn twice rather than read off the
            // surface, so the window keeps showing the shot.
            if let Some(f) = self.film.as_mut().filter(|f| f.capturing()) {
                let (w, h, rgba) = r.render_capture(&cam);
                if let Err(e) = f.write(w, h, &rgba) {
                    eprintln!("error: film: {e}");
                    event_loop.exit();
                }
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
            .with_inner_size({
                let (w, h) = self.cfg.render.preference.window_size.dims();
                winit::dpi::PhysicalSize::new(w, h)
            })
            // Borderless from the first frame when the config says so,
            // so a fullscreen launch never flashes a 4:3 window first.
            // The launcher is always a window (player ruling
            // 2026-09-25): fullscreen is the game's, applied when the
            // launcher hands over.
            .with_fullscreen(
                (self.cfg.render.preference.fullscreen && self.screen != Screen::Launcher)
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
                    if let Some(pack) = entities::level_impostors(sess.level.impostors.as_ref()) {
                        renderer.load_impostors(pack);
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
                renderer.set_smooth_textures(self.cfg.render.enhancement.smooth_textures);
                renderer.set_segment_order(self.cfg.render.enhancement.segment_order);
                renderer.set_marker_scale(self.cfg.render.enhancement.map_marker_scale);
                renderer.set_marker_halo(self.cfg.render.enhancement.autocontrasting_markers);
                renderer.set_extent_fog(self.cfg.render.enhancement.map_extent_fog);
                renderer.set_fog_distance(self.cfg.render.preference.fog_distance as f32);
                renderer.set_hud_transparent(self.hud_transparent());
                renderer.set_reflections(self.cfg.render.preference.reflections);
                renderer.set_horizon_cull(self.cfg.render.preference.horizon_cull);
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
            WindowEvent::CloseRequested => self.request_close(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                if self.screen == Screen::Launcher {
                    if down && button == MouseButton::Left {
                        let size = self.view_size();
                        if let Some(l) = &mut self.launcher {
                            l.click(size, self.cursor, &mut self.cfg);
                        }
                        self.launcher_changes();
                    }
                    return;
                }
                // Retail's movie abort takes either mouse button as
                // readily as a key (`lastPressedKey || mouseLeft ||
                // mouseRight`, remc1 sub_10300).
                if self.screen == Screen::Movie {
                    if down && let Some(m) = &mut self.movie {
                        m.skip();
                    }
                    return;
                }
                // The performance screen leaves on a LEFT click
                // (retail tests `mouseLeftButton` alone, :60048).
                if self.screen == Screen::Stats {
                    if down
                        && button == MouseButton::Left
                        && let Some(st) = &mut self.stats_screen
                    {
                        st.dismiss();
                    }
                    return;
                }
                // The exit-confirm dialog owns the pointer: OK
                // confirms, Cancel dismisses, everything else (and
                // any fire-through) is swallowed.
                if self.exit_confirm {
                    if down && button == MouseButton::Left {
                        let size = self.view_size();
                        let (ok, cancel) =
                            ui::exit_confirm_rects(ui_assets!(self), size.0, size.1);
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
                // Falling or dead: no pointer input reaches the game
                // (no casts, no pane, no map) — only the pause menu
                // above stays clickable.
                if down && self.player_down() {
                    return;
                }
                if self.screen == Screen::Menu {
                    // The main menu owns the pointer. Save/Load
                    // clicks need the slot scan before their dialog
                    // opens.
                    if down && button == MouseButton::Left {
                        let size = self.view_size();
                        let cursor = self.cursor;
                        if let Some(nl) = &mut self.netlobby {
                            nl.screen.click(size, cursor);
                            return;
                        }
                        if self.mc1_frontend() {
                            if let Some(m) = &mut self.mc1menu {
                                m.click(size, cursor);
                            }
                            return;
                        }
                        let request = self.mainmenu.as_mut().and_then(|m| m.click(size, cursor));
                        match request {
                            Some("network") => self.open_network_dialog(),
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
                    // The world-map screen owns the pointer: a left
                    // click on a portal starts the carpet leg there
                    // (the level launches when it arrives); a right
                    // click on a completed one shows its stats table.
                    if down && matches!(button, MouseButton::Left | MouseButton::Right) {
                        let size = self.view_size();
                        let cursor = self.cursor;
                        if let (Some(wm), Some(save)) = (
                            &mut self.worldmap,
                            self.campaign.as_ref().and_then(|c| c.save.mc2()),
                        ) {
                            if button == MouseButton::Left {
                                wm.click(save, size, cursor);
                            } else {
                                wm.right_click(save, size, cursor);
                            }
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
                            // everything under the G instrument
                            // (mirrors the pane view's overlay; the
                            // sim mints on select/bind).
                            let mc2 = self.is_mc2();
                            let owned = self.cfg.gameplay.cheat.dev_spells
                                || spell
                                    .map(|c| {
                                        let world = self
                                            .session
                                            .as_deref()
                                            .and_then(|s| s.sim.world.as_ref());
                                        world.is_some_and(|w| {
                                            if mc2 {
                                                w.mc2_book_view(local_hx()).owned[c as usize]
                                            } else {
                                                w.loadout(local_hx()).owned[c as usize]
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
                            .map(|w| w.loadout(local_hx()).owned)
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
                // …and so does the launcher's file browser.
                if self.screen == Screen::Launcher {
                    if let Some(l) = &mut self.launcher {
                        l.scroll(rows);
                    }
                    return;
                }
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
                if self.player_down() {
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
                self.objective_held = false;
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
                // Hold-O = retail's on-demand objective box (MC2
                // PlayerInput.cpp:393-98, scancode 0x18). A
                // NON-CONSUMING latch like retail's poll — flight
                // stays live under it; the draw site gates on MC2 +
                // in-level.
                if matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyO)) {
                    self.objective_held = down;
                }
                if down && self.alt_held && event.logical_key == Key::Named(NamedKey::Enter) {
                    // The launcher stays a window (see `resumed`).
                    if self.screen != Screen::Launcher {
                        self.toggle_fullscreen();
                    }
                    return;
                }
                // The launcher owns the keyboard.
                if self.screen == Screen::Launcher {
                    if down && let Some(l) = &mut self.launcher {
                        l.key(&event.logical_key, &mut self.cfg);
                    }
                    self.launcher_changes();
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
                // Any key leaves the performance screen (retail
                // `lastPressedKey`, :60048).
                if self.screen == Screen::Stats {
                    if down && let Some(st) = &mut self.stats_screen {
                        st.dismiss();
                    }
                    return;
                }
                // So does the MC2 map's stats table (MI:3217-22).
                if down
                    && self.screen == Screen::Map
                    && let Some(wm) = &mut self.worldmap
                    && wm.stats_open()
                {
                    wm.dismiss_stats();
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
                        if let Some(nl) = &mut self.netlobby {
                            nl.screen.escape();
                        } else if mc1 {
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
                        Lobby,
                        Mc2Menu,
                        Mc1Menu,
                        Map,
                        None,
                    }
                    let mc1 = self
                        .campaign
                        .as_ref()
                        .is_some_and(|c| c.id != campaign::CampaignId::Mc2);
                    let target = if self.screen == Screen::Menu && self.netlobby.is_some() {
                        if self.netlobby.as_ref().is_some_and(|l| l.screen.editing()) {
                            Edit::Lobby
                        } else {
                            Edit::None
                        }
                    } else if self.screen == Screen::Menu && mc1 {
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
                        let mut tab = false;
                        match &event.logical_key {
                            Key::Named(NamedKey::Tab) => tab = true,
                            Key::Named(NamedKey::Backspace) => backspace = true,
                            Key::Named(NamedKey::Enter) => enter = true,
                            Key::Named(NamedKey::Space) => chars.push(' '),
                            Key::Character(s) => chars.extend(s.chars()),
                            _ => {}
                        }
                        match target {
                            Edit::Lobby => {
                                if let Some(l) = &mut self.netlobby {
                                    if backspace {
                                        l.screen.key_backspace();
                                    } else if enter {
                                        l.screen.key_enter();
                                    }
                                    for c in chars {
                                        l.screen.key_char(c);
                                    }
                                }
                            }
                            Edit::Mc2Menu => {
                                if let Some(m) = &mut self.mainmenu {
                                    if tab {
                                        m.key_tab();
                                    }
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
                                    if tab {
                                        m.key_tab();
                                    }
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
                            if let Some(l) = &mut self.netlobby {
                                l.screen.key_enter();
                            } else if mc1 {
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
                // The suicide key (Shift+K — retail in BOTH games:
                // MC1 scancode 0x25 under the shift branch
                // :20492-93, MC2 PlayerInput.cpp:239-41; a bare
                // `life = -1`, the ordinary death chain follows).
                // Sits ABOVE the option keys because bare K is the
                // coords-overlay toggle and option_key carries no
                // shift test. `controls.preferences.disable_suicide_key`
                // still swallows the press (no fall-through to bare K).
                if down && self.shift_held && event.physical_key == PhysicalKey::Code(KeyCode::KeyK)
                {
                    self.pending_suicide = !self.cfg.controls.preferences.disable_suicide_key;
                    return;
                }
                // The win-level cheat (Shift+C — retail's tester key
                // in BOTH games: MC1 'c' sets the press-space
                // completion latch :20281-84, MC2 SHIFT+'c' sets
                // IsLevelEnd_0, PlayerInput.cpp:251-59; both behind
                // retail's tester gate, ungated here like the other
                // dev keys). Same shift-over-option-key placement as
                // Shift+K: bare C is the crosshair toggle.
                if down && self.shift_held && event.physical_key == PhysicalKey::Code(KeyCode::KeyC)
                {
                    self.pending_cheat = Some(mgc_formats::recover::Cheat::WinLevel);
                    self.mini_toast(".. CHEAT: win level");
                    return;
                }
                // The runtime option keys (F1/F2/F3/F5/F6 and the
                // cheat row G/H/J/B/N) — live in flight and inside the
                // menu alike. ⚠ SHIFT-GATED: `Shift+<letter>` belongs
                // to gameplay (Shift+K suicide, Shift+L demolish,
                // Shift+digit equips), and a bare-letter toggle that
                // also answers the shifted press SHADOWS it — see the
                // cheat-row note on `option_key`.
                if down
                    && !self.shift_held
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
                // Falling or dead: Space (the respawn) is the only
                // gameplay key retail still reads (PlayerInput.cpp
                // :812-14, :936-38). Esc and the option keys are
                // handled above; key-ups still pass so nothing sticks.
                if down
                    && self.player_down()
                    && event.physical_key != PhysicalKey::Code(KeyCode::Space)
                {
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
                                    // A rebind is a preference, not
                                    // progress: the campaign carry takes
                                    // it now, so every save and the next
                                    // MP game see it without a level won
                                    // first. (In MP the campaign is
                                    // parked — a rebind there stays in
                                    // that game.)
                                    if let Some(run) = &mut self.campaign
                                        && run.id != campaign::CampaignId::Mc2
                                    {
                                        run.mc1_quick_keys = quick_keys_carry(&self.quick_binds);
                                    }
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
                    // Edge-triggered: the OS auto-repeat must not
                    // count as a second press, or a held Space
                    // revives on one tick and wins the level on the
                    // next (the MC1 win-exit takes a SECOND Space).
                    PhysicalKey::Code(KeyCode::Space) => {
                        if down && !event.repeat {
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

/// One tick's verdict from the netplay gate ([`App::net_turn`]).
enum NetTurn {
    /// Not a netplay session: the ordinary local input path.
    Local,
    /// The tick's bundle has not arrived: step nothing yet.
    Starved,
    /// Every seat's input for the tick, in seat order, and the human
    /// seats that left the game at it (banished before the step).
    Ready(Vec<FlightInput>, Vec<u8>),
}

/// `--host` / `--join` / `--serve`: which end of a netplay link this
/// app is.
#[derive(Debug, Clone)]
enum NetArg {
    /// A server on a thread of this app, joined as its admin.
    Host(String),
    Join(String),
    /// The dedicated server alone (headless).
    Serve(String),
}

/// The default netplay port.
const NET_PORT: u16 = 7777;

/// A netplay address: `host:port`, a bare host (default port), a bare
/// port (on `default_host`), or empty (`default_host`, default port).
fn net_addr(arg: &str, default_host: &str) -> String {
    if arg.is_empty() {
        format!("{default_host}:{NET_PORT}")
    } else if arg.parse::<u16>().is_ok() {
        format!("{default_host}:{arg}")
    } else if arg.contains(':') {
        arg.to_string()
    } else {
        format!("{arg}:{NET_PORT}")
    }
}

struct Args {
    level: PathBuf,
    /// `--level` was given (a plain start boots the launcher instead).
    level_explicit: bool,
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
    /// a save (the slot is only overwritten when the player saves).
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
    /// CLI override of `render.enhancement.smooth_textures`.
    smooth_textures: Option<bool>,
    /// CLI override of `render.enhancement.segment_order`.
    segment_order: Option<bool>,
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
    /// `--set-spellbook id[:xp],…` — the explicit level-start book
    /// (strictly CLI; see `SpellbookSeed::Set`). Trumps the plausible
    /// instrument; ignored by campaign launches like it.
    set_spellbook: Option<Vec<(u8, i32)>>,
    /// CLI override of `gameplay.enhancement.wheel_spells`.
    wheel_spells: Option<bool>,
    /// CLI override of `gameplay.cheat.invincible`.
    invincible: Option<bool>,
    /// CLI override of `gameplay.cheat.ghost`.
    ghost: Option<bool>,
    /// CLI override of `gameplay.cheat.inert`.
    inert: Option<bool>,
    /// CLI override of `gameplay.cheat.weightless`.
    weightless: Option<bool>,
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
    /// `--map-settle N`: tick the level's own world N times before
    /// rendering the headless map (see `run_map`). MC2 terrain is built
    /// by gameplay ticks, so 0 renders an unbuilt level.
    map_settle: u32,
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
    /// Horizon-cull override (config `render.preference.horizon_cull`).
    horizon_cull: Option<bool>,
    /// Dynamic-lights override (config `render.preference.light_sources`).
    light_sources: Option<bool>,
    /// Vertical-sync override (config `render.preference.vsync`).
    vsync: Option<bool>,
    /// Borderless-fullscreen override (config
    /// `render.preference.fullscreen`).
    fullscreen: Option<bool>,
    /// FMV playback override (config `render.preference.movies`).
    movies: Option<bool>,
    /// End-of-level stats override (`render.preference.stats_screen`).
    stats_screen: Option<bool>,
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
    /// `--seat N` (1-based): fly wizard seat N of a `--level` launch
    /// instead of seat 1 — retail's local index; every other seat,
    /// seat 1 included, is a rival. Stored 0-based. Not with
    /// `--campaign`, `--record` or `--replay`.
    seat: u8,
    /// `--host [ADDR]` / `--join ADDR`: two-seat netplay (see [`net`]).
    net: Option<NetArg>,
    /// `--passphrase WORD`: the netplay session's passphrase — the host
    /// sets it, a joiner must match it (empty when not given).
    passphrase: String,
    /// `--game mc1|mc1hw|mc2`: the netplay session's game — required by
    /// `--serve`; a `--join` with it is refused by a server of another.
    net_game: Option<lobby::NetGame>,
    /// `--replay <take.mgcr>`: play a recording as the session's only
    /// input source — SOURCE-AGNOSTIC (retail takes via inline input
    /// recovery, port recordings via the exact input channel);
    /// docs/RECORDING.md "Consumers".
    replay: Option<PathBuf>,
    /// `--thirdperson`: watch a `--replay` from a boom behind the
    /// carpet instead of through its eye, with the recorded pose
    /// drawn SOLID (`--firstperson`, the default, hides it entirely).
    /// Replay-only for now — player ruling 2026-09-20: third person
    /// breaks aiming, so live play keeps the retail eye.
    third_person: bool,
    /// `--film <dir>`: capture the session as numbered PNG frames on
    /// the film clock (`film.rs`), turns `--film-from`..`--film-to`
    /// at `--film-rate` frames per turn. Usually with `--replay`.
    film: Option<PathBuf>,
    film_from: u64,
    film_to: u64,
    film_rate: u32,
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
    let mut level_explicit = false;
    let mut campaign_id = None;
    let mut slot = None;
    let mut new_game = false;
    let mut screenshot = None;
    let mut camera = None;
    let mut tileset = None;
    let mut config = None;
    let mut smooth_shading = None;
    let mut smooth_textures = None;
    let mut segment_order = None;
    let mut map_triggers = None;
    let mut health_bars = None;
    let mut crosshair = None;
    let mut dev_spells = None;
    let mut plausible_spellbook = None;
    let mut set_spellbook = None;
    let mut wheel_spells = None;
    let mut invincible = None;
    let mut ghost = None;
    let mut inert = None;
    let mut weightless = None;
    let mut expose_jar_spells = None;
    let mut grace_meter = None;
    let mut coords = None;
    let mut dev_mode = false;
    let mut thrust = None;
    let mut altitude = None;
    let mut bindings = None;
    let mut map = None;
    let mut map_scale = 4u32;
    let mut map_settle = 0u32;
    let mut map_view = false;
    let mut spell_selector = None;
    let mut rival_tags = None;
    let mut subtitles = None;
    let mut fog_distance = None;
    let mut sky = None;
    let mut reflections = None;
    let mut horizon_cull = None;
    let mut light_sources = None;
    let mut vsync = None;
    let mut fullscreen = None;
    let mut movies = None;
    let mut stats_screen = None;
    let mut anti_aliasing = None;
    let mut movie_subtitles = None;
    let mut fps = None;
    let mut anim_turn = 0.0f32;
    let mut terrain_features = true;
    let mut awake_range = None;
    let mut seat = 0u8;
    let mut net_arg = None;
    let mut passphrase = String::new();
    let mut net_game = None;
    let mut pool_slots = None;
    let mut replay = None;
    let mut third_person = false;
    let mut film = None;
    let mut film_from = 0u64;
    let mut film_to = u64::MAX;
    let mut film_rate = 1u32;
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

    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--level" => {
                level = resolve_level_arg(&it.next().ok_or("--level needs a path or game:index")?)?;
                level_explicit = true;
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
            // The replay viewpoint. `--firstperson` is the default and
            // exists so it can be said out loud; `--thirdperson` swings
            // the eye onto a boom behind the carpet (`camera.rs`).
            "--firstperson" => third_person = false,
            "--thirdperson" => third_person = true,
            "--film" => {
                film = Some(PathBuf::from(
                    it.next().ok_or("--film needs an output directory")?,
                ));
            }
            "--film-from" => {
                film_from = it
                    .next()
                    .ok_or("--film-from needs a turn")?
                    .parse()
                    .map_err(|e| format!("--film-from: {e}"))?;
            }
            "--film-to" => {
                film_to = it
                    .next()
                    .ok_or("--film-to needs a turn")?
                    .parse()
                    .map_err(|e| format!("--film-to: {e}"))?;
            }
            "--film-rate" => {
                film_rate = it
                    .next()
                    .ok_or("--film-rate needs a frames-per-turn count")?
                    .parse()
                    .map_err(|e| format!("--film-rate: {e}"))?;
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
            "--smooth-textures" => smooth_textures = Some(true),
            "--no-smooth-textures" => smooth_textures = Some(false),
            "--segment-order" => segment_order = Some(true),
            "--no-segment-order" => segment_order = Some(false),
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
            "--set-spellbook" => {
                let spec = it.next().ok_or_else(|| {
                    format!("--set-spellbook needs a value\n{}", set_spellbook_table())
                })?;
                set_spellbook = Some(parse_set_spellbook(&spec)?);
            }
            "--wheel-spells" => wheel_spells = Some(true),
            "--no-wheel-spells" => wheel_spells = Some(false),
            "--invincible" => invincible = Some(true),
            "--no-invincible" => invincible = Some(false),
            "--ghost" => ghost = Some(true),
            "--no-ghost" => ghost = Some(false),
            "--inert" => inert = Some(true),
            "--no-inert" => inert = Some(false),
            "--weightless" => weightless = Some(true),
            "--no-weightless" => weightless = Some(false),
            "--expose-jar-spells" => expose_jar_spells = Some(true),
            "--no-expose-jar-spells" => expose_jar_spells = Some(false),
            "--grace-meter" => grace_meter = Some(true),
            "--no-grace-meter" => grace_meter = Some(false),
            "--dev-mode" => dev_mode = true,
            "--sky" => sky = Some(true),
            "--no-sky" => sky = Some(false),
            "--reflections" => reflections = Some(true),
            "--no-reflections" => reflections = Some(false),
            "--horizon-cull" => horizon_cull = Some(true),
            "--no-horizon-cull" => horizon_cull = Some(false),
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
            "--stats" => stats_screen = Some(true),
            "--no-stats" => stats_screen = Some(false),
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
            "--map-settle" => {
                map_settle = it
                    .next()
                    .ok_or("--map-settle needs a tick count")?
                    .parse()
                    .map_err(|_| "--map-settle wants an integer")?;
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
            "--seat" => {
                let n: u8 = it
                    .next()
                    .ok_or("--seat needs a wizard seat (1-8)")?
                    .parse()
                    .map_err(|e| format!("--seat: {e}"))?;
                if !(1..=8).contains(&n) {
                    return Err("--seat: seats are 1-8".into());
                }
                seat = n - 1;
            }
            "--host" => {
                // An optional address; a bare port listens everywhere.
                let addr = match it.peek() {
                    Some(a) if !a.starts_with("--") => it.next().unwrap_or_default(),
                    _ => String::new(),
                };
                net_arg = Some(NetArg::Host(net_addr(&addr, "0.0.0.0")));
            }
            "--serve" => {
                let addr = match it.peek() {
                    Some(a) if !a.starts_with("--") => it.next().unwrap_or_default(),
                    _ => String::new(),
                };
                net_arg = Some(NetArg::Serve(net_addr(&addr, "0.0.0.0")));
            }
            "--join" => {
                let addr = it.next().ok_or("--join needs the server's address")?;
                net_arg = Some(NetArg::Join(net_addr(&addr, "127.0.0.1")));
            }
            "--passphrase" => {
                passphrase = it.next().ok_or("--passphrase needs a word")?;
            }
            "--game" => {
                let key = it.next().ok_or("--game needs mc1, mc1hw or mc2")?;
                net_game = Some(lobby::NetGame::parse(&key).map_err(|e| format!("--game: {e}"))?);
            }
            "--help" | "-h" => {
                return Err(format!(
                    "usage: mgcarpet [--level <game:index> | <baked/.../level-NNN.mgcl>] \
                     [--campaign mc1|mc1hw|mc2 [--slot N] [--new-game]] \
                     [--seat 1-8 (fly that wizard seat; --level only)] \
                     [--host [ADDR] | --join ADDR | --serve [ADDR] (netplay; port {NET_PORT} \
                     by default; --host = a server in this app + its admin player, \
                     --serve = the dedicated server alone, headless; needs \
                     --game mc1|mc1hw|mc2) [--passphrase WORD]] \
                     (slot 0 = default: a fresh run, saved only from the in-game menu) \
                     [--tileset 0|1] [--config <path>] \
                     [--smooth-shading|--no-smooth-shading] \
                     [--smooth-textures|--no-smooth-textures] \
                     [--segment-order|--no-segment-order] \
                     [--map-triggers|--no-map-triggers] \
                     [--crosshair|--no-crosshair] [--coords|--no-coords] \
                     [--health-bars|--no-health-bars] \
                     [--dev-spells|--no-dev-spells] \
                     [--plausible-spellbook|--no-plausible-spellbook] \
                     [--set-spellbook id[:xp],id,… (the exact level-start book by \
                     in-level spell id; `--set-spellbook list` prints the ids)] \
                     [--wheel-spells|--no-wheel-spells] \
                     [--invincible|--no-invincible] \
                     [--ghost|--no-ghost] [--inert|--no-inert] \
                     [--weightless|--no-weightless] \
                     [--expose-jar-spells|--no-expose-jar-spells] \
                     [--grace-meter|--no-grace-meter] \
                     [--dev-mode] \
                     [--thrust classic|enhanced] [--altitude classic|enhanced] \
                     [--bindings classic|wasd] \
                     [--spell-selector auto|mc1|mc2|mc1+mc2] \
                     [--rival-tags auto|on|off] \
                     [--subtitles on|off] [--fog-distance TILES (0 = no fog)] \
                     [--sky|--no-sky] [--reflections|--no-reflections] \
                     [--horizon-cull|--no-horizon-cull] \
                     [--light-sources|--no-light-sources] \
                     [--vsync|--no-vsync] [--fullscreen|--windowed] \
                     [--movies|--no-movies] [--stats|--no-stats] [--anti-aliasing off|msaa|1.5x|2x] \
                     [--fps|--no-fps] \
                     [--screenshot out.png [--camera x,y,z,yaw,pitch] [--map-view] \
                     [--anim-turn N]] \
                     [--map out.png [--map-scale N] [--map-settle TICKS (MC2 \
                     terrain is BUILT by gameplay ticks — 0 renders an unbuilt \
                     level; 8 matches a recording's first frame)]] \
                     [--no-terrain-features] \
                     [--pool-slots N] [--awake-range TILES (0 = always awake)] \
                     [--replay take.mgcr (play a recording — retail or port — as \
                     the session; level from the header)] \
                     [--firstperson (default: replay through the carpet's own \
                     eye, the recorded pose hidden) | --thirdperson (watch it \
                     from a boom behind the carpet, the recorded pose solid)] \
                     [--film DIR [--film-from TURN] [--film-to TURN] \
                     [--film-rate N (frames per game turn, default 1 = 24 fps)] \
                     (capture the session as numbered PNG frames on a fixed \
                     clock — one frame per 1/N turn, nothing dropped — seeking \
                     to --film-from first, exiting at --film-to or when the \
                     take ends; usually with --replay; encode with tools/film.py)] \
                     [--replay-check take.mgcr (headless: whole take + drift \
                     summary; exit 0 = zero divergence)] \
                     [--record out.mgcr (write this session as a port recording; \
                     WITH --replay/--replay-check it re-records the take as an \
                     INPUT-ONLY port take — the recovered input, no state/obs \
                     channels, ~150x smaller and shareable. The originals stay \
                     the conformance oracle)] \
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
    // A seat is a property of the level BUILD, and neither the
    // campaign nor a recording carries one yet.
    if seat != 0
        && (campaign_id.is_some() || record.is_some() || replay.is_some() || replay_check.is_some())
    {
        return Err("--seat: a plain --level launch only (not with --campaign, \
                    --record, --replay or --replay-check)"
            .into());
    }
    Ok(Args {
        level,
        level_explicit,
        campaign: campaign_id,
        slot,
        new_game,
        screenshot,
        camera,
        tileset,
        config,
        smooth_shading,
        smooth_textures,
        segment_order,
        map_triggers,
        health_bars,
        crosshair,
        dev_spells,
        plausible_spellbook,
        set_spellbook,
        wheel_spells,
        invincible,
        ghost,
        inert,
        weightless,
        expose_jar_spells,
        grace_meter,
        coords,
        dev_mode,
        thrust,
        altitude,
        bindings,
        map,
        map_scale,
        map_settle,
        map_view,
        spell_selector,
        rival_tags,
        subtitles,
        fog_distance,
        sky,
        reflections,
        horizon_cull,
        light_sources,
        vsync,
        fullscreen,
        movies,
        stats_screen,
        anti_aliasing,
        movie_subtitles,
        fps,
        anim_turn,
        terrain_features,
        pool_slots,
        awake_range,
        seat,
        net: net_arg,
        passphrase,
        net_game,
        replay,
        third_person,
        film,
        film_from,
        film_to,
        film_rate,
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
/// `--map-settle N` ticks the level's OWN world N times and re-copies
/// its planes into the view before rendering.
///
/// ⭐ WHY IT EXISTS: MC2's authored terrain is NOT in the bake. The
/// generator output is the bare landscape; the moats, lava and cave
/// sculpting are built by the (10,45) building records over the first
/// ~40 GAMEPLAY TICKS (`mc2_building_tick`, the 30-tick height lerp).
/// `World::new_full` runs `load_time_pass` for MC1 and NOTHING for MC2,
/// so `load_level`'s view snapshot — taken right after `init.build()` —
/// is that bare landscape. mc2l15's generated planes hold **ZERO** cells
/// of terrain types 8/10/11/23/26/27 where retail's measured terrain
/// holds 64/381/1/5/218/84; after 8 ticks the port matches retail's
/// record 0 EXACTLY on all six.
/// ⇒ a `--map` at settle 0 is a picture of a level that has not been
/// built yet. Pass `--map-settle 8` for the comparable image.
fn run_map(
    level: &mut LoadedLevel,
    out: &Path,
    scale: u32,
    map_triggers: bool,
    settle: u32,
) -> Result<(), String> {
    if settle > 0 {
        let Some(w) = level.world.as_mut() else {
            return Err("--map-settle needs the living world (do not pass \
                        --no-terrain-features)"
                .to_string());
        };
        // A real tick, not `tick_paused` — the buildings only advance
        // on an UNPAUSED tick. The pose stands the player at the
        // level's own start (or map centre), out of the way.
        let (px, pz) = level
            .start
            .as_ref()
            .map(|f| (f.x, f.z))
            .unwrap_or((128.0, 128.0));
        let idle = mgc_sim::engine::world::PlayerCommand::default();
        for _ in 0..settle {
            let alt = w.ground_height_tiles(px, pz) + 2.0;
            let pose = mgc_sim::engine::world::PlayerPose::from_tiles(px, alt, pz, 0.0, 0.0, 0.0);
            w.tick(pose, idle);
        }
        if let (Some(shading), Some(angle)) =
            (level.view.shading.as_mut(), level.view.angle.as_mut())
        {
            w.copy_planes_into(mgc_sim::engine::features::TerrainPlanes {
                height: &mut level.view.height,
                tile_type: &mut level.view.tile_type,
                shading,
                angle,
            });
        }
        if let Some(c) = level.view.ceiling.as_mut() {
            let live = w.ceiling_plane();
            if live.len() == c.len() {
                c.copy_from_slice(live);
            }
        }
    }
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
            human_book: Vec::new(),
            wizards: Default::default(),
            mc2_wizards: Default::default(),
            player_count: 1,
            human_seat: 0,
            extra_seats: Vec::new(),
            net_ai: None,
            stages: Vec::new(),
            stage_vars: Vec::new(),
            night_shade: init.night_shade,
            doom_level: init.doom_level,
            castle_purge_level: init.castle_purge_level,
            mc2_replayed: init.mc2_replayed,
            placeholders: init.placeholders,
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
        "tick,slot,id,x,y,z,yaw,aim,speed,min_speed,max_speed,state,role,hold,life,awake,leader,target,attacker,cadence,sub,px,py,pdist,blocked"
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
                    "{t},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{pd:.2},{}",
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
                    r.sub,
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

/// A loaded level's world made a running simulation — the ONE build
/// every netplay peer and the server's referee share (`install_level`,
/// `ServerWorld`): the instruments and patches, the campaign book, each
/// seat's flight models (`models`, seat order; the config's for a seat
/// without), the weightless cheat, the first seat's start marker.
fn session_sim(
    level: &mut LoadedLevel,
    cfg: &config::Config,
    models: &[config::ControlModels],
    campaign: Option<&CampaignRun>,
) -> Simulation {
    let mut sim = match level.world.take() {
        Some(mut w) => {
            w.terrain_dirty = true;
            w.entities_dirty = true;
            apply_instruments(
                &mut w,
                cfg.gameplay.cheat.dev_spells,
                &level.plausible_book_mc2,
                cfg.gameplay.cheat.invincible,
                cfg.gameplay.cheat.ghost,
                cfg.gameplay.cheat.inert,
                session_patches(cfg, models),
            );
            if let Some(run) = campaign {
                apply_campaign_book(&mut w, run, &level.mc2_human_blocked);
            }
            Simulation::with_world(w)
        }
        None => Simulation::with_terrain(level.height.clone()),
    };
    // Each seat its player's models (netplay); else everyone the
    // config's.
    for hx in sim.seat_ixs() {
        let m = models
            .get(usize::from(hx.0))
            .unwrap_or(&cfg.controls.models);
        sim.thrust_model[hx] = sim_thrust(m.thrust);
        sim.altitude_model[hx] = sim_altitude(m.altitude);
    }
    // Weightless cheat: unclamp the enhanced-altitude band to the
    // global lift ceiling (highest terrain + the 4-tile margin).
    sim.weightless = cfg.gameplay.cheat.weightless;
    // `level.start` is the world's FIRST seat's marker (the other
    // seats stand on their carpet records — `Simulation::with_world`).
    if let Some(start) = level.start {
        sim.flyer[mgc_sim::Hx::FIRST] = start;
        sim.sync_carpet_from_flyer(mgc_sim::Hx::FIRST);
    }
    sim
}

/// The world patches in force: the config's — but in a netplay game
/// (`models` per seat) the leviathan's lift counterweight
/// (`mc2_leviathan_high_lunge`, keyed on the ENHANCED altitude model)
/// follows the seats' models, on when any seat flies the enhanced
/// lift, so every peer agrees.
fn session_patches(
    cfg: &config::Config,
    models: &[config::ControlModels],
) -> mgc_sim::WorldPatches {
    let mut p = world_patches(cfg);
    if !models.is_empty() {
        let any = models
            .iter()
            .any(|m| m.altitude == config::AltitudeModel::Enhanced);
        p.mc2_leviathan_high_lunge = p.mc2_leviathan_true_aim && any;
    }
    p
}

/// Apply the playtest instruments to a freshly built world — ONE place
/// so a future instrument can't miss a call site (fresh start in
/// `App::new`, `restart_level`, and the headless screenshot path all
/// go through here). The MC1 plausible-spellbook set is NOT here: it
/// joins the world build (`WorldInit::human_book`) so its tokens sit
/// where retail's do, before the rivals (round 154).
fn apply_instruments(
    w: &mut mgc_sim::engine::world::World,
    dev_spells: bool,
    plausible_book_mc2: &[(u8, i32)],
    invincible: bool,
    ghost: bool,
    inert: bool,
    patches: mgc_sim::WorldPatches,
) {
    w.set_patches(patches);
    // Per-seat instruments reach EVERY human seat: netplay peers must
    // build identical worlds (one seat = the single-player world).
    for hx in w.seat_ixs() {
        if dev_spells {
            w.set_dev_spells(hx, true);
        }
        if !plausible_book_mc2.is_empty() {
            w.mc2_grant_plausible(hx, plausible_book_mc2);
        }
        if ghost {
            w.set_ghost(hx, true);
        }
    }
    if invincible {
        w.set_invincible(true);
    }
    if inert {
        w.set_inert(true);
    }
}

/// The config → sim mapping for the `gameplay · patches` class. The
/// world constructor defaults every arm to RETAIL (that is what keeps
/// goldens/tests/mgc-conform faithful); the app opts the configured
/// patches in here, and `apply_option` re-applies live.
///
/// `mc2_leviathan_high_lunge` is the one arm that also reads another
/// option: it is the enhanced lift's counterweight, so it is in force
/// only while the altitude model IS the enhanced lift (the altitude
/// option's live handler re-applies the set). The option's other half,
/// `mc2_leviathan_true_aim`, is right under either altitude model and
/// follows the option alone.
fn world_patches(cfg: &config::Config) -> mgc_sim::WorldPatches {
    let p = &cfg.gameplay.patches;
    mgc_sim::WorldPatches {
        mc2_leviathan_high_lunge: p.mc2_leviathan_high_lunge.on()
            && cfg.controls.models.altitude == config::AltitudeModel::Enhanced,
        mc2_leviathan_true_aim: p.mc2_leviathan_high_lunge.on(),
        mc2_flyers_clear_terrain: p.mc2_flyers_clear_terrain.on(),
        skeletons_convert_traders: p.skeletons_convert_traders.on(),
        jar_ground_snap: p.jar_ground_snap.on(),
        ball_ground_track: p.ball_ground_track.on(),
        map_wide_ball_rolling: p.map_wide_ball_rolling.on(),
        mc2_worm_possess_map_wide: p.owner_recolor_sim().on(),
        possessed_footprint: p.possessed_footprint.on(),
        mc2_downgrade_overflow: p.mc2_downgrade_overflow.on(),
        castle_latch_bug: p.castle_latch_bug.on(),
        no_spell_loss: p.no_spell_loss.on(),
        mc1_fix_dragon_tail: p.mc1_fix_dragon_tail.on(),
        mc1_pack_speed_restore: p.mc1_pack_speed_restore.on(),
        mc2_phantom_castle: p.mc2_phantom_castle.on(),
        dual_wield_muzzle: p.dual_wield_muzzle.on(),
        one_castle_per_wizard: p.one_castle_per_wizard.on(),
        mc2_wyvern_alliance_brain: p.mc2_wyvern_alliance_brain.on(),
        mc2_orphan_balloon_reap: p.mc2_orphan_balloon_reap.on(),
        mc2_house_flag_color: p.mc2_house_flag_color.on(),
        mc2_immediate_reap: p.mc2_immediate_reap.on(),
        mc1_recycle_victim_revalidate: p.mc1_recycle_victim_revalidate.on(),
        mc1_castle_transform_watchdog: p.mc1_castle_transform_watchdog.on(),
        mc1_crushed_site_collapse: p.mc1_crushed_site_collapse.on(),
        mc2_building_pad_saturate: p.mc2_building_pad_saturate.on(),
        mc1_building_pad_saturate: p.mc1_building_pad_saturate.on(),
        mc1_segment_chain_revalidate: p.mc1_segment_chain_revalidate.on(),
        volcano_register_revalidate: p.volcano_register_revalidate.on(),
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

/// The MC1/HW campaign's collected spells (ids, UNMASKED — the
/// level's availability mask is `load_level`'s, where the book joins
/// the world build; the grant law is collected ∩ mask, :49226-33).
fn mc1_campaign_carry(run: Option<&CampaignRun>) -> Vec<u8> {
    let Some(save) = run.and_then(|r| r.save.mc1()) else {
        return Vec::new();
    };
    (0..24)
        .filter(|&s| save.blob24[s] != 0)
        .map(|s| s as u8)
        .collect()
}

/// Install the campaign's cross-level carry into a fresh world.
/// MC1/HW: the book itself was granted by the world build (it sits
/// in the pool BEFORE the rivals — `WorldInit::human_book`, round
/// 154); what is left is the native cycle-ring sidecar. MC2: learn
/// the carried book with its banked XP — `mc2_grant_plausible` is the
/// same grant+bank+re-derive path retail's `sub_549A0` carry feeds.
/// Retail's black-and-white palette over a dying wizard, as a
/// greyscale amount for [`mgc_render::Renderer::set_greyscale`]. The
/// DEATH FALL re-installs palette mode 7 on every tick whose blink bit
/// `(Turn / 6) & 1` is set (MC2 EF:60432-33, MC1 :55464-65 — the
/// `str_93` bank's `+99`), and each install hands over to mode 1, the
/// 4-step fade back to the level palette (EF:31912-19): six ticks
/// grey, then 3/4, 1/2, 1/4, colour, and again. The CORPSE installs it
/// every tick (EF:60665, :55628) — solid monochrome until Space.
/// The quick keys as campaign carry: `None` while nothing is bound, so
/// an unbound campaign keeps the old save shape and seeds from scratch.
/// The monitor a window sits on: the one its frame overlaps most,
/// computed here rather than left to `Borderless(None)`. winit's own
/// answer is a cached guess on X11 (updated as the window moves), and
/// a window as large as or larger than its monitor — the Full HD / QHD
/// / 4K window sizes — can leave that guess on the first monitor, so
/// fullscreen jumped there (player-reported 2026-10-18). Falls back to
/// winit's answer when the position is unknown (Wayland reports none).
fn window_monitor(window: &Window) -> Option<winit::monitor::MonitorHandle> {
    let Ok(pos) = window.outer_position() else {
        return window.current_monitor();
    };
    let size = window.outer_size();
    let (x0, y0) = (i64::from(pos.x), i64::from(pos.y));
    let (x1, y1) = (x0 + i64::from(size.width), y0 + i64::from(size.height));
    window
        .available_monitors()
        .map(|m| {
            let (mp, ms) = (m.position(), m.size());
            let (mx0, my0) = (i64::from(mp.x), i64::from(mp.y));
            let (mx1, my1) = (mx0 + i64::from(ms.width), my0 + i64::from(ms.height));
            let w = (x1.min(mx1) - x0.max(mx0)).max(0);
            let h = (y1.min(my1) - y0.max(my0)).max(0);
            (w * h, m)
        })
        .filter(|(area, _)| *area > 0)
        .max_by_key(|(area, _)| *area)
        .map(|(_, m)| m)
        .or_else(|| window.current_monitor())
}

fn quick_keys_carry(binds: &[Option<u8>; 10]) -> Option<[Option<u8>; 10]> {
    binds.iter().any(Option::is_some).then_some(*binds)
}

fn death_greyscale(state: mgc_sim::engine::world::LifeState, tick: u64) -> f32 {
    use mgc_sim::engine::world::LifeState;
    match state {
        LifeState::Alive => 0.0,
        LifeState::Dead => 1.0,
        LifeState::Falling => {
            if (tick / 6) & 1 == 1 {
                1.0
            } else {
                (1.0 - ((tick % 6) + 1) as f32 / 4.0).max(0.0)
            }
        }
    }
}

fn apply_campaign_book(
    w: &mut mgc_sim::engine::world::World,
    run: &CampaignRun,
    mc2_blocked: &[bool; 26],
) {
    match run.id {
        campaign::CampaignId::Mc2 => {
            let Some(save) = run.save.mc2() else { return };
            let book = save.book();
            // Retail's grant consults the carry only for spells the
            // level does not block for the human (`InitialiseSpells_54A50`
            // EF:39014): a blocked one stays known, XP and all, but out
            // of the book until the level hands it back — mc2:017's
            // Alliance jar (player report 2026-10-03).
            let (withheld, grants): (Vec<(u8, i32)>, Vec<(u8, i32)>) = (0..26)
                .filter(|&s| book.owned[s])
                .map(|s| (s as u8, book.xp[s]))
                .partition(|&(s, _)| mc2_blocked[s as usize]);
            if !grants.is_empty() {
                w.mc2_grant_plausible(local_hx(), &grants);
            }
            if !withheld.is_empty() {
                w.mc2_bank_withheld(local_hx(), &withheld);
            }
            // The rest of retail's carry: the cycle ring only. Every
            // spell starts the level at tier 1 and the hands take the
            // grant pass's canonical binding — retail's level start
            // (`InitialiseSpells_54A50`) resets both (see
            // `World::mc2_install_ring_carry`).
            w.mc2_install_ring_carry(local_hx(), &book.ring);
        }
        _ => {
            if run.save.mc1().is_none() {
                return;
            }
            // Cycle-ring carry (native-only sidecar; kept RAW like
            // MC2's — unavailable members are skipped at cycle time,
            // not dropped).
            w.install_mc1_ring(local_hx(), run.mc1_ring);
        }
    }
}

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

/// The won-edge bookkeeping: fold the finished level into the
/// campaign record and decide what follows (`CampaignRun::next`).
/// Deliberately does NOT write the slot file: retail-style silent
/// autosave on completion was removed (player ruling 2026-09-11) —
/// the record lives in memory until the player saves from the hub
/// or the mini-menu. A free function because it runs inside the
/// redraw's `&mut sim.world` borrow (field-disjoint from
/// `self.campaign`).
fn campaign_complete(
    run: &mut CampaignRun,
    level: u32,
    ticks: u64,
    w: &mgc_sim::engine::world::World,
) {
    use campaign::{CampaignId, NextStep};
    match run.id {
        CampaignId::Mc2 => {
            let Some(save) = run.save.mc2_mut() else {
                return;
            };
            // The level's score row (`CollectLevelStats_5C530` →
            // `sub_82AB0`, EF:47327-66): [spells, accuracy, kills,
            // mana, seconds] into the main table by level, or the
            // secret table by portal index. The ENHANCED rows (see
            // `engine::stats`), like MC1's performance screen.
            let st = w.level_stats(local_hx());
            // Retail's integer row carries the enhanced tenths rounded
            // to whole percent; the tail keeps the absolutes the map
            // table derives its tenths from.
            use mgc_sim::engine::stats::{fmt10, whole};
            let row = [
                whole(st.spells_fixed_pct10()) as i32,
                whole(st.offensive_accuracy_pct10()) as i32,
                whole(st.cleared_pct10()) as i32,
                whole(st.mana_pct10()) as i32,
                (ticks / mgc_sim::TICK_RATE_HZ as u64).min(i32::MAX as u64) as i32,
            ];
            let [castle, houses, unclaimed] = st.mana_rows();
            println!(
                "stats: killed {}% (retail {}%: {} of census {}) deaths you/rivals/nature \
                 {}/{}/{} alive {} · accuracy {}% · spells {}% · mana {}% (retail {}%) \
                 castle/dwellings/unclaimed {castle}/{houses}/{unclaimed}",
                fmt10(st.cleared_pct10()),
                st.killed_pct,
                st.kills,
                st.census,
                st.deaths_player(),
                st.deaths_rivals(),
                st.deaths_environment(),
                st.alive,
                fmt10(st.offensive_accuracy_pct10()),
                fmt10(st.spells_fixed_pct10()),
                fmt10(st.mana_pct10()),
                st.mana_pct,
            );
            // The absolutes behind the row (player rulings 2026-09-24,
            // as MC1's screen). See `saves::EXT_*`.
            let ext = saves::ext_row(&st);
            if level <= 24 {
                save.main_stats[level as usize] = row;
                save.main_ext[level as usize] = ext;
            } else if let Some(i) = save
                .secrets
                .iter()
                .take_while(|p| p.activated != 0)
                .position(|p| p.level as u32 == level)
                && i < save.secret_stats.len()
            {
                save.secret_stats[i] = row;
                save.secret_ext[i] = ext;
            }
            // Book carry: serialize the live book into str_611 (all
            // XP banked — the between-levels shape).
            let v = w.mc2_book_view(local_hx());
            // KNOWN is monotonic in retail (`array_0x3E9` is never
            // cleared; the level end only ORs this level's book and
            // finds into it, EF:43784-89), so the carry keeps every
            // spell it already held — a blocked spell the level never
            // handed back included — at no less than its carried XP.
            let prev = save.book();
            let mut owned = v.owned;
            let mut xp = v.xp;
            for s in 0..26 {
                if prev.owned[s] && !v.owned[s] {
                    owned[s] = true;
                    xp[s] = xp[s].max(prev.xp[s]);
                }
            }
            save.set_book(&saves::Mc2Book {
                owned,
                xp,
                levels: v.levels,
                sel: v.sel,
                left: v.left,
                right: v.right,
                ring: v.ring,
            });
            let exit = w.mc2_exit_model(local_hx());
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
                // portals; replays never regress it). The finale
                // opens nothing further — 25 = every portal
                // conquered, and the map stays up for free play.
                save.levels_completed = save.levels_completed.max(level + 1);
                if level == 24 {
                    println!(
                        "campaign mc2: the finale is won — free play (every portal stays open)"
                    );
                }
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
        }
        CampaignId::Mc1 | CampaignId::Mc1Hw => {
            let hw = run.id == CampaignId::Mc1Hw;
            let Some(save) = run.save.mc1_mut() else {
                return;
            };
            // Commit collected spells to the persistent flags (the
            // retail level-completion commit into var_15318).
            let loadout = w.loadout(local_hx());
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
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_screenshot(
    mut level: LoadedLevel,
    out: &Path,
    camera: Option<[f32; 5]>,
    smooth_shading: bool,
    smooth_textures: bool,
    segment_order: bool,
    fog_distance: u32,
    sky_texture: bool,
    reflections: bool,
    horizon_cull: bool,
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
            .mc2_objective_targets(local_hx())
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
    if let Some(pack) = entities::level_impostors(level.impostors.as_ref()) {
        renderer.load_impostors(pack);
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
    renderer.set_smooth_textures(smooth_textures);
    renderer.set_segment_order(segment_order);
    renderer.set_fog_distance(fog_distance as f32);
    if let Some(sky) = mc2_sky_srgb(&level) {
        renderer.set_sky_color(sky);
    }
    if sky_texture && let Some(bitmap) = &level.sky {
        renderer.load_sky(bitmap, &level.palette_rgba);
    }
    renderer.set_reflections(reflections);
    renderer.set_horizon_cull(horizon_cull);
    if light_sources
        && level.mc2_env != entities::Mc2MapEnv::Day
        && let Some(w) = &level.world
    {
        renderer.set_lights(&entities::lights_from_poses(&w.live_poses(local_hx())));
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
    // MGC_SIGHT_MIST=1: a still of the Beyond-Sight trigger-area
    // ripple at full strength (live, the spell has to be up), its clock taken
    // from `--anim-turn`.
    if std::env::var_os("MGC_SIGHT_MIST").is_some()
        && let Some(w) = &level.world
    {
        renderer.set_sight_areas(&sight_areas(w));
        renderer.set_sight_mist(1.0, anim_turn / 24.0);
    }
    // Spell UI (book grid or HUD), from the level-start loadout.
    let hud_team = level
        .world
        .as_ref()
        .and_then(|w| hud_team_colors(w.human_wizard_seat(local_hx()), &level));
    if let (Some(assets), Some(w)) = (&level.ui, &mut level.world) {
        // invincible/ghost/inert=false: a single headless frame
        // takes no damage and trips nothing.
        apply_instruments(
            w,
            dev_spells,
            &level.plausible_book_mc2,
            false,
            false,
            false,
            mgc_sim::WorldPatches::RETAIL,
        );
        let loadout = w.loadout(local_hx());
        let vitals = w.vitals(local_hx());
        let mc2_book = shot_is_mc2.then(|| w.mc2_book_view(local_hx()));
        let quads = if map_view {
            if shot_is_mc2 {
                // MC2's map screen has no book half; the split layout
                // shows the stretched live view instead.
                Vec::new()
            } else {
                ui::book_quads(
                    assets,
                    &loadout,
                    &[None; 10],
                    true,
                    1280.0,
                    960.0,
                    (-1.0, -1.0),
                )
                .0
            }
        } else {
            // Both blink phases pinned true: a screenshot shows any
            // armed alert, expiring panel and goal tick.
            ui::hud_quads(
                assets,
                &loadout,
                &vitals,
                hud_transparent,
                true,
                true,
                shot_is_mc2,
                mc2_book.as_ref(),
                dev_spells,
                hud_team,
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
            owner_type_index: None,
            frame: 0,
            flc_frame: None,
            h_stretch: 1.0,
            action: 0,
            x,
            z,
            alt,
            yaw: 0.0,
            segment: false,
            body: None,
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
/// The `mgc-sprite-export` tool (world sprites as files for outside
/// work — see `sprite_export`).
pub fn sprite_export_main() -> std::process::ExitCode {
    sprite_export::main()
}

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
        None => (paths::config_file(), false),
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
    if let Some(v) = args.smooth_textures {
        en.smooth_textures = v;
    }
    if let Some(v) = args.segment_order {
        en.segment_order = v;
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
    if let Some(v) = args.horizon_cull {
        cfg.render.preference.horizon_cull = v;
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
    if let Some(v) = args.stats_screen {
        cfg.render.preference.stats_screen = v;
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
    if let Some(v) = args.wheel_spells {
        cfg.gameplay.enhancement.wheel_spells = v;
    }
    if let Some(v) = args.dev_spells {
        cfg.gameplay.cheat.dev_spells = v;
    }
    if let Some(v) = args.invincible {
        cfg.gameplay.cheat.invincible = v;
    }
    if let Some(v) = args.ghost {
        cfg.gameplay.cheat.ghost = v;
    }
    if let Some(v) = args.inert {
        cfg.gameplay.cheat.inert = v;
    }
    if let Some(v) = args.weightless {
        cfg.gameplay.cheat.weightless = v;
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
                // A completed MC2 campaign is waiting on no level
                // (`current` = 25, past the finale): probe the bake
                // with the finale's file instead. Campaign boots load
                // no level anyway — this path only feeds the
                // staleness check.
                let probe = if run.id == campaign::CampaignId::Mc2 {
                    run.current.min(24)
                } else {
                    run.current
                };
                level_path = run.level_path(probe);
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
        if args.set_spellbook.is_some() {
            println!("campaign: set-spellbook ignored — the real campaign carry replaces it");
        }
    }

    // In-app replay (docs/RECORDING.md "Consumers"): the take's
    // header picks the level; the session boots single-level with the
    // fidelity-relevant knobs pinned — a retail take demands the
    // faithful tiers and no instruments, a port take pins its own
    // recorded sim closure.
    let mut replay_boot: Option<replay::ReplayFile> = None;
    // `--thirdperson` is a REPLAY view (player ruling 2026-09-20:
    // third person breaks aiming, so live play keeps retail's eye).
    // Say so rather than ignoring it silently.
    if args.third_person && args.replay.is_none() {
        eprintln!("--thirdperson: ignored — it is a --replay view (live play flies first person)");
    }
    if let Some(path) = args.replay.as_ref().or(args.replay_check.as_ref()) {
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
        cfg.gameplay.cheat.ghost = false;
        cfg.gameplay.cheat.inert = false;
        cfg.gameplay.cheat.weightless = false;
        // plausible_spellbook: off unless the CLI EXPLICITLY asks (the
        // config file's value never leaks into a replay). A take whose
        // header carries its import pin needs no flag — the grants ride
        // the pin (acq list, hand bits) + the snapshot (the token pool)
        // — and an explicit flag is harmless under it: the pin is
        // applied after the grants and overrides them. A PRE-PIN take
        // recorded under the instrument stored its acq list nowhere,
        // so replaying it needs the same grants re-run at world init;
        // the explicit flag is that re-run.
        cfg.dev.plausible_spellbook = args.plausible_spellbook == Some(true);
        // The offline chassis params come FROM THE TAKE, not from this
        // run's CLI/config — the take's embedded start snapshot opens
        // its identity block with `chassis.pool_slots` and
        // `chassis.awake_gate_sq`, so a world built at any other size
        // is refused ("snapshot is for a different world"). Discarding
        // them here is what made a `--pool-slots N` take unreplayable
        // even by the very command that recorded it. A retail take
        // carries no port closure and pins the faithful defaults;
        // a port take from before these keys existed reads None, which
        // is the same faithful default it was recorded under.
        cfg.sim.parameters.entity_pool_size = file
            .sim_pool_slots
            .filter(|_| file.source == replay::ReplaySource::Port)
            .map(|n| n as u32);
        cfg.sim.parameters.awake_range = file
            .sim_awake_range
            .filter(|_| file.source == replay::ReplaySource::Port);
        // The retail-bug patches pin to the take's recorded policy:
        // retail-source and post-policy port takes run the retail
        // arms; a pre-option port take replays under the legacy
        // hard-wired set it was recorded against.
        // The presentation-only arms stay the player's
        // (`GameplayPatches::with_presentation_of`): they grade nothing.
        cfg.gameplay.patches = match file.patch_policy() {
            Ok(p) => p.with_presentation_of(&cfg.gameplay.patches),
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
        // The campaign REPLAY gate is derived from the capture, so it
        // is a property of the run the banner must SHOW — it changes
        // which entities live (the XP scrolls) and was invisible for
        // four sessions precisely because nothing printed it.
        if file.family == mgc_formats::mgcr::Family::Mc2
            && file.source == replay::ReplaySource::Retail
        {
            println!(
                "replay: campaign replay gate {} (the take's own (14,5) witness)",
                if file.mc2_replayed { "SET" } else { "clear" }
            );
        }
        replay_boot = Some(file);
    }
    // --record: a recording is a conformance instrument — the whole
    // session runs the retail patch arms so a later replay's pin
    // matches by construction (the port header stamps
    // `"patches": "retail"`). Player-ruled 2026-08-08.
    if args.record.is_some() {
        cfg.gameplay.patches =
            config::GameplayPatches::retail_all().with_presentation_of(&cfg.gameplay.patches);
        println!("record: retail-bug patches run their RETAIL arms for this session");
    }
    // Netplay: handshake BEFORE any world — the joiner learns the
    // level, the seats and the host's sim-affecting options from it.
    // `--serve`: the dedicated server — no window, no world of its own
    // until a game starts.
    if let Some(NetArg::Serve(addr)) = &args.net {
        return run_server(addr, &args);
    }
    let (net_link, net_server) = match &args.net {
        Some(net_arg) => match net_handshake(net_arg, &args, &mut cfg, &mut level_path) {
            Ok((link, server)) => (Some(link), server),
            Err(e) => {
                eprintln!("error: {e}");
                return std::process::ExitCode::FAILURE;
            }
        },
        None => (None, None),
    };
    let boot_seats = net_link
        .as_ref()
        .map_or_else(|| vec![args.seat], |l| l.game.seats.clone());
    // Re-derive the offline pool params after the replay pin — which
    // SET them from the take rather than clearing them, so the world is
    // built at the recorded size and the start snapshot applies (and
    // after a netplay join adopted the host's).
    let pool_slots = if replay_boot.is_some() || net_link.is_some() {
        cfg.sim.parameters.entity_pool_size.map(|n| n as usize)
    } else {
        pool_slots
    };
    let awake_range = if replay_boot.is_some() || net_link.is_some() {
        cfg.sim.parameters.awake_range
    } else {
        awake_range
    };

    // A plain start — no level, campaign, take or headless
    // instrument — boots the launcher, which needs no baked data: it
    // judges (and if need be bakes) the tree itself, in the window.
    let launcher = !args.level_explicit
        && net_link.is_none()
        && args.campaign.is_none()
        && args.replay.is_none()
        && args.replay_check.is_none()
        && args.record.is_none()
        && args.screenshot.is_none()
        && args.map.is_none()
        && args.flock_probe.is_none();

    // First-run / stale-epoch auto-bake: regenerate the baked tree
    // from the original game data before touching it.
    if !launcher && let Err(e) = bakecheck::ensure_baked(&level_path, cfg.gamedata.as_deref()) {
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
    // The level-start book seed: `--set-spellbook` trumps the
    // plausible instrument (an explicit set beats an inferred one).
    let book_seed = match &args.set_spellbook {
        Some(ids) => {
            if cfg.dev.plausible_spellbook {
                println!("set-spellbook: overrides --plausible-spellbook");
            }
            SpellbookSeed::Set(ids.clone())
        }
        None if cfg.dev.plausible_spellbook => SpellbookSeed::Plausible,
        None => SpellbookSeed::Level,
    };
    let mut boot_level = if (campaign_run.is_some() && !headless) || launcher {
        None
    } else {
        // A netplay launch seats the session's AI count, as every peer
        // and the server's world do (`net_level`).
        match load_level_with(
            &level_path,
            args.tileset,
            args.terrain_features,
            &book_seed,
            &mc1_campaign_carry(campaign_run.as_ref()),
            pool_slots,
            awake_range,
            false,
            &boot_seats,
            net_link.as_ref().map(|l| l.game.ai),
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
        // to the world directly. `strict_retail` rides along: it is
        // CONFIG the snapshot skips, and an exported take was free-run
        // under it (a retail take raises it at its own import, so the
        // retail path needs no help here).
        if let Some(w) = level.world.as_mut() {
            w.set_patches(world_patches(&cfg));
            // The windowed path's apply_instruments equivalent for the
            // one instrument a replay honors (`--plausible-spellbook`,
            // the pre-pin-take rescue): the grants must run BEFORE the
            // pin so a recorded pin still overrides them. The MC1 set
            // was granted by the world build itself (round 154).
            if !level.plausible_book_mc2.is_empty() {
                w.mc2_grant_plausible(local_hx(), &level.plausible_book_mc2);
            }
            // The snapshot restored the pool and the terrain but not
            // the MODE they were established in. No key in the header
            // = keep the native build's pin (see ReplayFile).
            if let Some(pin) = file.sim_import_pin {
                w.set_import_pin(pin);
            }
        }
        return match replay::replay_check(level, file, args.record.as_deref()) {
            Ok(true) => std::process::ExitCode::SUCCESS,
            Ok(false) => std::process::ExitCode::FAILURE,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::from(2)
            }
        };
    }

    if let Some(out) = &args.map {
        let level = boot_level.as_mut().expect("headless paths load a level");
        return match run_map(
            level,
            out,
            args.map_scale,
            cfg.render.debug.map_trigger_areas,
            args.map_settle,
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
            cfg.render.enhancement.smooth_textures,
            cfg.render.enhancement.segment_order,
            cfg.render.preference.fog_distance,
            cfg.render.preference.sky,
            cfg.render.preference.reflections,
            cfg.render.preference.horizon_cull,
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
    println!("          Shift+K is the retail suicide key (both games),");
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

    // `--film`: a silent batch job — the seek bursts hundreds of
    // turns a frame, and a film has no soundtrack anyway.
    let film = match &args.film {
        Some(dir) => {
            match film::Film::new(dir.clone(), args.film_from, args.film_to, args.film_rate) {
                Ok(f) => {
                    cfg.audio.sound = false;
                    cfg.audio.music = false;
                    Some(f)
                }
                Err(e) => {
                    eprintln!("error: --film: {e}");
                    return std::process::ExitCode::from(2);
                }
            }
        }
        None => None,
    };

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
            launcher,
            tileset: args.tileset,
            terrain_features: args.terrain_features,
            pool_slots,
            awake_range,
        },
        replay_boot,
        args.record.clone(),
        args.third_person,
        film,
    );
    if let Some(link) = &net_link {
        app.net_models = net_seat_models(&link.game.models, &app.cfg.controls.models);
        app.net_names = given_names(&link.game);
        app.apply_net_models();
    }
    app.net = net_link.map(net::Lockstep::start);
    app.net_server = net_server;
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("error: event loop: {e}");
        return std::process::ExitCode::FAILURE;
    }
    // Every exit path funnels here — a live recording gets its zstd
    // frame end so the file reopens cleanly.
    app.finish_recorder();
    std::process::ExitCode::SUCCESS
}

/// The lobby screen of the session's game: MC2's temple doorway
/// (HSCREEN0 case 7) or MC1/HW's PMULTI — two views over one lobby,
/// speaking the same [`lobby_mc2::LobbyAction`]s.
enum LobbyUi {
    Mc2(Box<lobby_mc2::LobbyScreen>),
    Mc1(Box<lobby_mc1::Mc1LobbyScreen>),
}

impl LobbyUi {
    fn update(&mut self, view: &lobby::LobbyView) {
        match self {
            Self::Mc2(s) => s.update(view),
            Self::Mc1(s) => s.update(view),
        }
    }

    fn set_rules(&mut self, rows: Vec<lobby_mc2::RuleRow>, passphrase: Option<String>) {
        match self {
            Self::Mc2(s) => s.set_rules(rows, passphrase),
            Self::Mc1(s) => s.set_rules(rows, passphrase),
        }
    }

    fn tick(&mut self, dt: f32) {
        match self {
            Self::Mc2(s) => s.tick(dt),
            Self::Mc1(s) => s.tick(dt),
        }
    }

    fn take_actions(&mut self) -> Vec<lobby_mc2::LobbyAction> {
        match self {
            Self::Mc2(s) => s.take_actions(),
            Self::Mc1(s) => s.take_actions(),
        }
    }

    /// The click samples (MC1's lobby is silent, as retail's).
    fn take_sounds(&mut self) -> Vec<u8> {
        match self {
            Self::Mc2(s) => s.take_sounds(),
            Self::Mc1(_) => Vec::new(),
        }
    }

    fn click(&mut self, size: (f32, f32), cursor: (f32, f32)) {
        match self {
            Self::Mc2(s) => s.click(size, cursor),
            Self::Mc1(s) => s.click(size, cursor),
        }
    }

    fn escape(&mut self) {
        match self {
            Self::Mc2(s) => s.escape(),
            Self::Mc1(s) => s.escape(),
        }
    }

    fn editing(&self) -> bool {
        match self {
            Self::Mc2(s) => s.editing(),
            Self::Mc1(s) => s.editing(),
        }
    }

    fn key_char(&mut self, c: char) {
        match self {
            Self::Mc2(s) => s.key_char(c),
            Self::Mc1(s) => s.key_char(c),
        }
    }

    fn key_backspace(&mut self) {
        match self {
            Self::Mc2(s) => s.key_backspace(),
            Self::Mc1(s) => s.key_backspace(),
        }
    }

    fn key_enter(&mut self) {
        match self {
            Self::Mc2(s) => s.key_enter(),
            Self::Mc1(s) => s.key_enter(),
        }
    }

    fn alert(&mut self, text: String) {
        match self {
            Self::Mc2(s) => s.alert(text),
            Self::Mc1(s) => s.alert(text),
        }
    }
}

/// The netplay lobby between games: the session and its screen.
struct NetLobby {
    guest: net::Guest,
    screen: LobbyUi,
    /// The rules the scroll's rows were last read from (and the
    /// admin's passphrase).
    rules_shown: Option<(serde_json::Value, u8, Option<String>)>,
}

/// The lobby's rules scroll: (label, config path) — the session values
/// the host chooses (`rules::HOST_CHOSEN`; every other pin runs at its
/// default in a multiplayer game).
const LOBBY_RULES: [(&str, &str); 1] = [("Fog", "render.preference.fog_distance")];

/// Step the scroll's `row` in a session's rules: the stepped rules, or
/// `None` when the value stays put. Works on a scratch config — the
/// session's rules are the lobby's, never the player's own options.
fn step_lobby_rule(
    rules: &serde_json::Value,
    row: usize,
    forward: bool,
    specs: &[settings::Spec],
) -> Option<serde_json::Value> {
    let &(_, path) = LOBBY_RULES.get(row)?;
    let spec = specs.iter().find(|s| s.cfg_path == path)?;
    let mut scratch = config::Config::default();
    if let Err(e) = rules::adopt(&mut scratch, rules) {
        eprintln!("{e}");
        return None;
    }
    settings::step(spec, &mut scratch, forward).then(|| rules::collect(&scratch))
}

/// The scroll's rows read off a session's rules.
fn lobby_rule_rows(rules: &serde_json::Value, specs: &[settings::Spec]) -> Vec<lobby_mc2::RuleRow> {
    let mut cfg = config::Config::default();
    if let Err(e) = rules::adopt(&mut cfg, rules) {
        eprintln!("{e}");
    }
    LOBBY_RULES
        .iter()
        .map(|&(label, path)| {
            let value = specs
                .iter()
                .find(|s| s.cfg_path == path)
                .map_or_else(|| "?".into(), |s| (s.read)(&cfg).current_text());
            lobby_mc2::RuleRow {
                label: label.into(),
                value,
            }
        })
        .collect()
}

impl NetLobby {
    fn view(&self) -> lobby::LobbyView {
        self.guest.view().clone()
    }
}

/// The seats' flight models off a Start (`fallback` for an unreadable
/// one — a peer of this same build always sends a readable set).
fn net_seat_models(
    models: &[serde_json::Value],
    fallback: &config::ControlModels,
) -> Vec<config::ControlModels> {
    models
        .iter()
        .map(|v| serde_json::from_value(v.clone()).unwrap_or_else(|_| fallback.clone()))
        .collect()
}

/// A lobby game's given player names over the humans' table names
/// (tags, map reveals, the roster).
fn name_humans(names: &[(u8, String)], views: &mut [mgc_sim::engine::world::RivalView]) {
    for v in views {
        if let Some((_, n)) = names.iter().find(|(s, _)| *s == v.slot) {
            v.name.clone_from(n);
        }
    }
}

/// Arena level `n` of `game` (`NetGame::arenas`, the lobby's list).
fn arena_path(game: lobby::NetGame, n: u32) -> String {
    get_baked_directory()
        .join(format!("{}/level-{n:03}.mgcl", game.key()))
        .to_string_lossy()
        .into_owned()
}

/// The arena a lobby level path names (`…/level-NNN.mgcl`).
fn arena_of(path: &str) -> Option<u32> {
    let stem = std::path::Path::new(path).file_stem()?.to_str()?;
    stem.strip_prefix("level-")?.parse().ok()
}

/// The lobby's level field: retail `"%d. %s"` with `level − 49` (MC2's
/// names; MC1's list names its arenas itself, so here just the number).
fn arena_label(path: &str) -> String {
    let mc2 = lobby::NetGame::of_level(path).is_none_or(|g| g == lobby::NetGame::Mc2);
    arena_of(path).map_or_else(String::new, |n| {
        if mc2 {
            format!("{}. {}", n.saturating_sub(49), worldmap::mc2_level_name(n))
        } else {
            format!("arena {}", n.saturating_sub(49))
        }
    })
}

/// The player's netplay entries (`saves::netplay_path`).
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct NetPrefs {
    #[serde(default)]
    recent: Vec<String>,
    #[serde(default)]
    port: String,
    /// The last port joined on (the scroll's Join face).
    #[serde(default)]
    join_port: String,
    #[serde(default)]
    passphrase: String,
}

impl NetPrefs {
    fn load() -> Self {
        std::fs::read(saves::netplay_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let path = saves::netplay_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string_pretty(self).expect("prefs serialize") + "\n";
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("note: {}: {e}", path.display());
        }
    }

    /// Put `addr` first among the recent hosts (five kept).
    fn remember(&mut self, addr: &str) {
        self.recent.retain(|a| a != addr);
        self.recent.insert(0, addr.to_string());
        self.recent.truncate(5);
    }
}

/// The netplay lobby's flow (the MC2 Multiplayer button): entry, the
/// lobby frame, a game's start, and the way back (ruling 10).
impl App {
    /// The world patches in force (`session_patches`).
    fn world_patches_now(&self) -> mgc_sim::WorldPatches {
        session_patches(&self.cfg, &self.net_models)
    }

    /// A netplay game: each seat flies its own player's models, and the
    /// patches follow them (`world_patches_now`).
    fn apply_net_models(&mut self) {
        if self.net_models.is_empty() {
            return;
        }
        let patches = self.world_patches_now();
        let Some(sess) = self.session.as_deref_mut() else {
            return;
        };
        for (k, m) in self.net_models.iter().enumerate() {
            let hx = mgc_sim::Hx(k as u8);
            sess.sim.thrust_model[hx] = sim_thrust(m.thrust);
            sess.sim.altitude_model[hx] = sim_altitude(m.altitude);
        }
        if let Some(w) = sess.sim.world.as_mut() {
            w.set_patches(patches);
        }
    }

    /// Take `rules` into the config AND re-apply every pin it changed:
    /// some settings live outside the config once applied (the
    /// renderer holds the fog distance), so adopting the values alone
    /// left a client drawing his own fog (player-reported 2026-10-15).
    fn adopt_rules(&mut self, rules: &serde_json::Value) {
        let changed = rules::overridden(&self.cfg, rules);
        if let Err(e) = rules::adopt(&mut self.cfg, rules) {
            eprintln!("{e}");
            return;
        }
        for path in changed {
            self.apply_option(path);
        }
    }

    /// A network game's standings, once a frame: a banished player is
    /// told he waits for the end; the game is over when the SERVER says
    /// so (its world's verdict, [`net_verdict`]) — the winner is named
    /// and the fade-out takes everyone back to the lobby (a won level's
    /// path). A session already lost judges its own world.
    fn net_standings(&mut self) {
        let Some(w) = self.session.as_deref().and_then(|s| s.sim.world.as_ref()) else {
            return;
        };
        let banished = w.banished(local_hx());
        let lost = self.net.as_ref().is_some_and(|n| n.closed().is_some());
        let local = (lost && !self.won_handled)
            .then(|| net_verdict(w, &self.net_names))
            .flatten();
        let over = self
            .net
            .as_mut()
            .and_then(net::Lockstep::take_verdict)
            .or(local)
            .filter(|_| !self.won_handled);
        let Some(w) = self
            .session
            .as_deref_mut()
            .and_then(|s| s.sim.world.as_mut())
        else {
            return;
        };
        if let Some(verdict) = over {
            println!("net: the game is over — {verdict}");
            w.notify_option(verdict);
            self.won_handled = true;
            self.quit_fade = Some(-1.0);
        } else if banished && !self.won_handled && w.seat_ixs().count() > 1 {
            w.notify_option(if w.risen(local_hx()) {
                "Spectating - waiting for the game to end"
            } else {
                "Banished - press Space to rise as a ghost"
            });
        }
    }

    /// The running campaign's player name (MC2 Set Name / MC1's save
    /// name) — the name a lobby member carries; empty without one.
    fn campaign_player_name(&self) -> String {
        self.campaign
            .as_ref()
            .and_then(|c| {
                c.save
                    .mc2()
                    .map(|s| s.player_name.clone())
                    .or_else(|| c.save.mc1().map(|s| s.name.clone()))
            })
            .map(|n| n.trim().to_string())
            .unwrap_or_default()
    }

    /// The Multiplayer scroll, seeded from the player's entries.
    fn open_network_dialog(&mut self) {
        let p = NetPrefs::load();
        let port = if p.port.is_empty() {
            NET_PORT.to_string()
        } else {
            p.port
        };
        let join_port = if p.join_port.is_empty() {
            NET_PORT.to_string()
        } else {
            p.join_port
        };
        if self.mc1_frontend() {
            if let Some(m) = &mut self.mc1menu {
                m.open_network(p.recent, port, join_port, p.passphrase);
            }
        } else if let Some(m) = &mut self.mainmenu {
            m.open_network(p.recent, port, join_port, p.passphrase);
        }
    }

    fn net_message(&mut self, text: impl Into<String>) {
        let text = text.into();
        eprintln!("net: {text}");
        if self.mc1_frontend() {
            if let Some(m) = &mut self.mc1menu {
                m.open_message(text);
            }
        } else if let Some(m) = &mut self.mainmenu {
            m.open_message(text);
        }
    }

    /// The running campaign is MC1 or HW (their 320×200 frontend).
    fn mc1_frontend(&self) -> bool {
        self.campaign
            .as_ref()
            .is_some_and(|c| c.id != campaign::CampaignId::Mc2)
    }

    fn open_lobby(&mut self, guest: net::Guest) {
        let game = guest.view().game;
        let screen = match game {
            lobby::NetGame::Mc2 => {
                lobby_mc2::LobbyScreen::load(&get_baked_directory().join("assets/mc2-ui"))
                    .map(|s| LobbyUi::Mc2(Box::new(s)))
            }
            lobby::NetGame::Mc1 | lobby::NetGame::Mc1hw => lobby_mc1::Mc1LobbyScreen::load(
                &get_baked_directory().join("assets/mc1-ui"),
                game == lobby::NetGame::Mc1hw,
            )
            .map(|s| LobbyUi::Mc1(Box::new(s))),
        };
        match screen {
            Ok(screen) => {
                self.netlobby = Some(NetLobby {
                    guest,
                    screen,
                    rules_shown: None,
                });
                self.ui_atlas = UiAtlas::None;
            }
            Err(e) => self.net_message(format!("the lobby is unavailable: {e}")),
        }
    }

    /// Host: start a server on `port` (a thread of this app, the first
    /// arena up) and join it as its admin — the same server
    /// `--serve` runs, so a friend's game and a dedicated one are one
    /// thing (player 2026-10-17). It lives until the app closes or its
    /// player leaves the lobby.
    fn host_lobby(&mut self, port: String, passphrase: String) {
        let mut prefs = NetPrefs::load();
        prefs.port = port.clone();
        prefs.passphrase = passphrase.clone();
        prefs.save();
        let addr = net_addr(&port, "0.0.0.0");
        self.net_server = None;
        let opts = net::ServerOpts {
            game: self.net_game(),
            passphrase: passphrase.clone(),
            build: net::build_id(),
            level: arena_path(self.net_game(), *self.net_game().arenas().start()),
            rules: rules::session(&self.cfg),
            admin_key: None,
            frozen_after: net::FROZEN_AFTER,
        };
        match net::ServerHandle::spawn(&addr, opts, ServerWorld::boxed) {
            Ok(server) => {
                let (local, key) = (server.local_addr(), server.admin_key().to_string());
                self.net_server = Some(server);
                self.connect_lobby(local, passphrase, key);
            }
            Err(e) => self.net_message(e),
        }
    }

    /// Join: connect off the frame loop ("Attempting Connection").
    fn join_lobby(&mut self, addr: String, port: String, passphrase: String) {
        // The Port field joins the address — unless the address names
        // its own (`host:port`, the form few know; it still works).
        let addr = if port.is_empty() || !frontend::split_host_port(&addr).1.is_empty() {
            addr
        } else {
            format!("{addr}:{port}")
        };
        let mut prefs = NetPrefs::load();
        prefs.remember(&addr);
        if !port.is_empty() {
            prefs.join_port = port;
        }
        prefs.passphrase = passphrase.clone();
        prefs.save();
        self.connect_lobby(net_addr(&addr, "127.0.0.1"), passphrase, String::new());
    }

    /// The game this app's lobby plays — the running campaign's (its
    /// main menu's Multiplayer button is the way in: MC2's temple, MC1 /
    /// HW's button 3).
    fn net_game(&self) -> lobby::NetGame {
        match self.campaign.as_ref().map(|c| c.id) {
            Some(campaign::CampaignId::Mc1) => lobby::NetGame::Mc1,
            Some(campaign::CampaignId::Mc1Hw) => lobby::NetGame::Mc1hw,
            Some(campaign::CampaignId::Mc2) | None => lobby::NetGame::Mc2,
        }
    }

    /// Connect to a server off the frame loop (one of this app's game —
    /// a server of another refuses us); `admin_key` claims a hosted
    /// server's admin role (our own).
    fn connect_lobby(&mut self, addr: String, passphrase: String, admin_key: String) {
        let build = net::build_id();
        let name = self.campaign_player_name();
        let game = Some(self.net_game());
        self.net_connecting = Some(std::thread::spawn(move || {
            net::Guest::connect(
                &addr,
                &passphrase,
                &name,
                game,
                &build,
                &admin_key,
                std::time::Duration::from_secs(10),
            )
        }));
        if self.mc1_frontend() {
            if let Some(m) = &mut self.mc1menu {
                m.open_connecting();
            }
        } else if let Some(m) = &mut self.mainmenu {
            m.open_connecting();
        }
    }

    /// A finished join lands in the lobby, or says why not.
    fn poll_join(&mut self) {
        if !self
            .net_connecting
            .as_ref()
            .is_some_and(|t| t.is_finished())
        {
            return;
        }
        let joined = self.net_connecting.take().map(|t| {
            t.join()
                .unwrap_or_else(|_| Err("the connection attempt crashed".into()))
        });
        if let Some(m) = &mut self.mainmenu {
            m.close_connecting();
        }
        if let Some(m) = &mut self.mc1menu {
            m.close_connecting();
        }
        match joined {
            Some(Ok(g)) => self.open_lobby(g),
            Some(Err(e)) => {
                // Our own server, unreachable: no use keeping it up.
                self.net_server = None;
                self.net_message(e);
            }
            None => {}
        }
    }

    /// Leave the lobby for the temple. A hosting player's server stops
    /// with it — the session ends for everyone in it.
    fn leave_lobby(&mut self, why: Option<String>) {
        self.netlobby = None;
        self.ui_atlas = UiAtlas::None;
        if self.net_server.take().is_some() {
            println!("net: the hosted server stopped");
        }
        if let Some(why) = why {
            self.net_message(format!("Left the lobby: {why}"));
        }
    }

    /// One frame of the lobby (in place of the temple screen).
    fn lobby_frame(&mut self, dt: f32) {
        let models = serde_json::to_value(&self.cfg.controls.models).expect("models serialize");
        let (size, cursor) = (self.view_size(), self.cursor);
        let Some(nl) = &mut self.netlobby else { return };
        nl.guest.set_models(models);
        nl.guest.poll();
        for n in nl.guest.take_notices() {
            println!("net: {n}");
        }
        if let Some(why) = nl.guest.closed().map(str::to_string) {
            self.leave_lobby(Some(why));
            return;
        }
        if nl.guest.started() {
            let Some(NetLobby { guest, .. }) = self.netlobby.take() else {
                return;
            };
            match guest.into_link() {
                Ok(link) => self.start_net_game(link),
                Err(_) => unreachable!("started"),
            }
            return;
        }
        let view = nl.view();
        nl.screen.update(&view);
        let shown = (view.rules.clone(), view.ai, view.passphrase.clone());
        if nl.rules_shown.as_ref() != Some(&shown) {
            let mut rows = lobby_rule_rows(&view.rules, &self.specs);
            // The AI wizards: the scroll's last row (a lobby rule, not
            // a config path — `LobbyCore::set_ai`).
            rows.push(lobby_mc2::RuleRow {
                label: "AI wizards".into(),
                value: view.ai.to_string(),
            });
            nl.screen.set_rules(rows, view.passphrase.clone());
            nl.rules_shown = Some(shown);
        }
        nl.screen.tick(dt);
        let actions = nl.screen.take_actions();
        let sounds = nl.screen.take_sounds();
        let mut quads = vec![ui::solid([0.0, 0.0, size.0, size.1], [0.0, 0.0, 0.0, 1.0])];
        // MC1's screen composes on the CPU: its frame IS the atlas.
        let mut frame = None;
        match &mut nl.screen {
            LobbyUi::Mc2(s) => quads.extend(s.quads(size, cursor, &arena_label(&view.level))),
            LobbyUi::Mc1(s) => {
                let (rgba, q) = s.frame(size, cursor);
                quads.extend(q);
                frame = Some((rgba, s.has_pointer()));
            }
        }
        // MC1: the frame carries the menu's hand, else the software arrow.
        match frame {
            Some((_, true)) => {
                if let Some(w) = &self.window {
                    w.set_cursor_visible(false);
                }
            }
            Some((_, false)) => self.append_software_cursor(&mut quads),
            None => {}
        }
        let Some(nl) = &mut self.netlobby else { return };
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
            match (&nl.screen, frame) {
                (_, Some((rgba, _))) => {
                    r.load_ui_atlas(320, 200, &rgba);
                    self.ui_atlas = UiAtlas::LobbyMc1;
                }
                (LobbyUi::Mc2(s), None) if self.ui_atlas != UiAtlas::LobbyMc2 => {
                    let (w, h, px) = s.atlas();
                    r.load_ui_atlas(w, h, px);
                    self.ui_atlas = UiAtlas::LobbyMc2;
                }
                _ => {}
            }
            r.set_ui_quads(quads);
        }
        for a in actions {
            self.lobby_action(a, &view);
        }
    }

    fn lobby_action(&mut self, action: lobby_mc2::LobbyAction, view: &lobby::LobbyView) {
        use lobby_mc2::LobbyAction as A;
        if action == A::Exit {
            // A hosting player's Exit stops his server: warned first.
            if self.hosted_end_confirmed("exit") {
                println!("net: left the lobby");
                self.leave_lobby(None);
            }
            return;
        }
        let Some(nl) = &mut self.netlobby else { return };
        let g = &mut nl.guest;
        let me = view.me().map_or(0, |m| m.wizard);
        let forward = matches!(action, A::NextLevel | A::NextWizard);
        let admin = view.is_admin();
        match action {
            A::Exit => {}
            A::PrevWizard | A::NextWizard => {
                let dir = if forward { 1 } else { -1 };
                if let Some(w) = view.next_free(me, dir) {
                    g.pick_wizard(w);
                }
            }
            A::PickWizard(w) => {
                if view.is_free(w) {
                    g.pick_wizard(w);
                }
            }
            // Play: the admin's starts the game (the server checks the
            // agreement); everyone else's toggles his agreement.
            A::Play if admin => g.play(),
            A::Play => {
                if view.me().is_some_and(|m| m.agreed) {
                    g.withdraw();
                } else {
                    g.agree();
                }
            }
            // The rest are the admin's.
            _ if !admin => {}
            A::PrevLevel | A::NextLevel => {
                let arenas = view.game.arenas();
                let (lo, hi) = (*arenas.start(), *arenas.end());
                let n = arena_of(&view.level).unwrap_or(lo);
                let n = match (forward, n) {
                    (true, n) if n >= hi => lo,
                    (true, n) => n + 1,
                    (false, n) if n <= lo => hi,
                    (false, n) => n - 1,
                };
                g.set_level(arena_path(view.game, n));
            }
            A::SetArena(n) if view.game.arenas().contains(&n) => {
                g.set_level(arena_path(view.game, n));
            }
            A::SetArena(_) => {}
            A::SetCapacity(n) => g.set_capacity(n.clamp(2, net::MAX_PLAYERS)),
            A::Kick(id) => g.kick(id),
            A::CycleCapacity => {
                let next = usize::from(view.capacity) + 1;
                g.set_capacity(if next > net::MAX_PLAYERS { 2 } else { next });
            }
            A::StepRule { row, forward } if row == LOBBY_RULES.len() => {
                g.set_ai(if forward {
                    view.ai.saturating_add(1)
                } else {
                    view.ai.saturating_sub(1)
                });
            }
            A::StepRule { row, forward } => {
                // The session's rules are the LOBBY's, not the admin's
                // own options: step a scratch copy, so a lobby choice
                // never reaches his config, his screen or his
                // `mgcarpet.json` (player-reported 2026-10-16: a lobby
                // fog leaked into single-player).
                if let Some(rules) = step_lobby_rule(&view.rules, row, forward, &self.specs) {
                    g.set_rules(rules);
                }
            }
            A::SetPassphrase(p) => {
                println!("net: the passphrase changed");
                g.set_passphrase(p.clone());
                let mut prefs = NetPrefs::load();
                prefs.passphrase = p;
                prefs.save();
            }
        }
    }

    /// Play: build the agreed world and run the lockstep. Everyone plays
    /// by the session's rules (his own come back afterwards); the
    /// campaign steps aside — every seat gets the level's own book
    /// (retail: campaign spells are ignored), and nothing a lobby game
    /// does touches the campaign.
    fn start_net_game(&mut self, link: net::Link) {
        self.net_own_rules = Some(rules::collect(&self.cfg));
        self.adopt_rules(&link.game.rules);
        self.net_models = net_seat_models(&link.game.models, &self.cfg.controls.models);
        LOCAL_SEAT.store(link.local.0, std::sync::atomic::Ordering::Relaxed);
        let level = net_level(
            &link.game,
            &self.cfg,
            self.launch.tileset,
            self.launch.terrain_features,
        );
        match level {
            Ok(level) => {
                println!(
                    "net: the game begins — {} as seat {}",
                    arena_label(&link.game.level),
                    link.game.seats[usize::from(link.local.0)] + 1
                );
                self.lobby_game = true;
                self.net_names = given_names(&link.game);
                self.net_campaign = self.campaign.take();
                self.install_level(level);
                self.apply_net_models();
                self.net = Some(net::Lockstep::start(link));
                self.net_stall = None;
                self.net_stall_told = false;
                self.net_end_told = false;
            }
            Err(e) => {
                // Nobody plays a world this peer cannot build: leave
                // (the others see the departure).
                let session = net::Lockstep::start(link).finish();
                self.end_lobby_game(session);
                self.net_message(format!("the level would not load: {e}"));
            }
        }
    }

    /// The lobby game is over for us (the verdict's fade ran out, the
    /// server reopened the lobby, or we left): back to the lobby —
    /// waiting for the game to finish when it still runs — when the
    /// session survives, else the temple.
    fn net_game_over(&mut self) {
        let session = self.net.take().and_then(net::Lockstep::finish);
        if session.is_none() {
            self.net_server = None;
        }
        self.end_lobby_game(session);
    }

    fn end_lobby_game(&mut self, session: Option<net::Guest>) {
        if let Some(own) = self.net_own_rules.take() {
            self.adopt_rules(&own);
        }
        LOCAL_SEAT.store(0, std::sync::atomic::Ordering::Relaxed);
        self.net_names.clear();
        self.net_models.clear();
        if self.lobby_game {
            self.lobby_game = false;
            self.campaign = self.net_campaign.take();
        }
        self.enter_main_menu();
        if let Some(s) = session {
            self.open_lobby(s);
        }
    }

    /// The other players a closing app would leave behind: members of
    /// the server it hosts (itself not counted) — the close warning.
    fn hosted_others(&self) -> usize {
        self.net_server
            .as_ref()
            .map_or(0, |s| s.status().members.saturating_sub(1))
    }

    /// The window's close: an app hosting a server that others are on
    /// warns first (player 2026-10-17 — closing it ends the session for
    /// everyone); a second close within the warning's window goes
    /// through.
    fn request_close(&mut self, event_loop: &ActiveEventLoop) {
        if self.hosted_end_confirmed("close") {
            event_loop.exit();
        }
    }

    /// Stopping this app's server (`what`: the close, the lobby's
    /// Exit) ends the session for every other player on it: the first
    /// try warns, a second within 5 s goes through. True = go ahead.
    fn hosted_end_confirmed(&mut self, what: &str) -> bool {
        let others = self.hosted_others();
        let warned = self
            .close_warned
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5));
        if others == 0 || warned {
            self.close_warned = None;
            return true;
        }
        self.close_warned = Some(std::time::Instant::now());
        let text = format!(
            "{others} player{} still in this session - {what} again to end it for everyone",
            if others == 1 { "" } else { "s" }
        );
        match &mut self.netlobby {
            Some(nl) => {
                println!("net: {text}");
                nl.screen.alert(text);
            }
            None => self.mini_toast(text),
        }
        false
    }
}

/// The given player names of a lobby game, by wizard seat.
fn given_names(game: &net::Game) -> Vec<(u8, String)> {
    game.seats
        .iter()
        .zip(&game.names)
        .filter(|(_, n)| !n.is_empty())
        .map(|(&s, n)| (s, n.clone()))
        .collect()
}

/// A lobby game's level as every peer builds it — the players and the
/// server's referee alike (`tileset` and `terrain_features` are the
/// player's launch flags; the referee runs the defaults).
fn net_level(
    game: &net::Game,
    cfg: &config::Config,
    tileset: Option<u8>,
    terrain_features: bool,
) -> Result<LoadedLevel, String> {
    let seed = if cfg.dev.plausible_spellbook {
        SpellbookSeed::Plausible
    } else {
        SpellbookSeed::Level
    };
    load_level_with(
        std::path::Path::new(&game.level),
        tileset,
        terrain_features,
        &seed,
        &[],
        cfg.sim.parameters.entity_pool_size.map(|n| n as usize),
        cfg.sim.parameters.awake_range,
        false,
        &game.seats,
        Some(game.ai),
    )
}

/// Apply the server's world (a resync) onto `sim` before it steps the
/// snapshot's tick: `Ok` — the restored hash is the server's, and the
/// report says where `sim` had diverged (printed in full); `Err` — it
/// did not heal. The app's and the tests' one path.
fn net_resync(sim: &mut Simulation, snap: &net::Snapshot) -> Result<Vec<String>, String> {
    let before = sim.clone();
    sim.net_restore(&snap.bytes).map_err(|e| e.to_string())?;
    let report = before.divergence_report(sim);
    eprintln!(
        "net: resync at tick {} — {} state sections had diverged:",
        snap.tick,
        report.len()
    );
    for line in &report {
        eprintln!("net:   {line}");
    }
    let hash = sim.state_hash();
    if hash != snap.hash {
        return Err(format!(
            "restored {hash:016x}, the server's {:016x} (the snapshot misses state)",
            snap.hash
        ));
    }
    Ok(report)
}

/// Drop a step's sound requests unheard — what the app's `audio_tick`
/// does every tick with them, minus the mixer. The world's pending
/// vec is HASHED: every peer (and the server's world) must drain it
/// the same way, heard or not.
fn drain_sounds(w: &mut mgc_sim::engine::world::World) {
    let _ = w.take_audio(
        mgc_sim::Hx::FIRST,
        mgc_sim::engine::world::PlayerPose::from_tiles(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
    );
}

/// A seat's shown name: the player's given name, else the wizard's.
fn seat_display_name(names: &[(u8, String)], seat: u8, mc2: bool) -> String {
    names
        .iter()
        .find(|(s, _)| *s == seat)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| {
            let s = usize::from(seat.min(7));
            if mc2 {
                mgc_sim::mc2::rivals::MC2_RIVAL_NAMES[s].into()
            } else {
                mgc_sim::mc1::rivals::RIVAL_NAMES[s].into()
            }
        })
}

/// A network game's verdict on `w`: when one wizard alone stands (or
/// no human does), the winner's announcement. A one-human world (a
/// solo test) is never decided.
fn net_verdict(w: &mgc_sim::engine::world::World, names: &[(u8, String)]) -> Option<String> {
    if w.seat_ixs().count() < 2 {
        return None;
    }
    let (humans, rivals) = w.contenders();
    if !humans.is_empty() && humans.len() + rivals.len() > 1 {
        return None;
    }
    let mc2 = w.game() == mgc_sim::ids::GameId::Mc2;
    let winner = match (humans.first(), rivals.first()) {
        (Some(&hx), _) => seat_display_name(names, w.human_wizard_seat(hx), mc2),
        (None, Some(&slot)) => seat_display_name(names, slot, mc2),
        (None, None) => "Nobody".into(),
    };
    Some(format!("{winner} is victorious!"))
}

/// The wizard seats a level authors a start marker for (bit k = seat
/// k; `entities::player_start_seat`) — all eight when it can't be read
/// (the game's start reports the real error).
fn level_open_seats(path: &std::path::Path) -> u8 {
    let Ok(package) = std::fs::File::open(path)
        .map_err(|e| e.to_string())
        .and_then(|f| mgcl::read(std::io::BufReader::new(f)).map_err(|e| e.to_string()))
    else {
        return u8::MAX;
    };
    let game = mgc_sim::ids::GameId::from(package.meta.game);
    (0..8u8)
        .filter(|&k| entities::player_start_seat(game, &package.things.things, k).is_some())
        .fold(0, |m, k| m | 1 << k)
}

/// The server's own world (`net::Referee`): every lobby game built the
/// way each player builds his (`net_level` + `session_sim`, the
/// session's rules over the defaults), stepped headless with every
/// turn — the hash the players are checked against, and the verdict.
#[derive(Default)]
struct ServerWorld {
    sim: Option<Simulation>,
    names: Vec<(u8, String)>,
}

impl ServerWorld {
    fn boxed() -> Box<dyn net::Referee> {
        Box::<Self>::default()
    }
}

impl net::Referee for ServerWorld {
    fn begin(&mut self, game: &net::Game) -> Result<(), String> {
        let mut cfg = config::Config::default();
        rules::adopt(&mut cfg, &game.rules)?;
        let models = net_seat_models(&game.models, &cfg.controls.models);
        let mut level = net_level(game, &cfg, None, true)?;
        self.sim = Some(session_sim(&mut level, &cfg, &models, None));
        self.names = given_names(game);
        Ok(())
    }

    fn hash(&self) -> u64 {
        self.sim.as_ref().map_or(0, Simulation::state_hash)
    }

    fn snapshot(&self) -> Vec<u8> {
        self.sim
            .as_ref()
            .map_or_else(Vec::new, Simulation::net_snapshot)
    }

    fn step(&mut self, inputs: &[mgc_formats::mgcr::PortInput], left: &[u8]) {
        let Some(sim) = &mut self.sim else { return };
        if let Some(w) = sim.world.as_mut() {
            for &seat in left {
                w.depart(mgc_sim::Hx(seat));
            }
        }
        let inputs: Vec<FlightInput> = inputs.iter().map(replay::flight_input_from).collect();
        sim.step_seats(&inputs);
        // Every player's app drains the step's sound requests (its
        // `audio_tick`); the pending vec is hashed — drain it here too.
        if let Some(w) = sim.world.as_mut() {
            drain_sounds(w);
        }
    }

    fn verdict(&self) -> Option<String> {
        let w = self.sim.as_ref()?.world.as_ref()?;
        net_verdict(w, &self.names)
    }

    fn open_seats(&self, level: &str) -> u8 {
        level_open_seats(std::path::Path::new(level))
    }

    fn end(&mut self) {
        self.sim = None;
    }
}

/// `--host` / `--join`: refuse what netplay cannot carry, connect, and
/// fold the agreed session into the launch (level, seat, options) and
/// the app's seat globals. `--host` starts the server on a thread of
/// this app (returned: it lives as long as the app) and joins it as
/// the admin.
fn net_handshake(
    net_arg: &NetArg,
    args: &Args,
    cfg: &mut config::Config,
    level_path: &mut PathBuf,
) -> Result<(net::Link, Option<net::ServerHandle>), String> {
    if args.campaign.is_some()
        || args.replay.is_some()
        || args.replay_check.is_some()
        || args.record.is_some()
        || args.screenshot.is_some()
        || args.map.is_some()
        || args.flock_probe.is_some()
        || args.film.is_some()
        || args.set_spellbook.is_some()
    {
        return Err(
            "netplay is a single --level session: no --campaign, --replay, \
                    --record, --film, --set-spellbook or headless modes"
                .into(),
        );
    }
    let build = net::build_id();
    let models = serde_json::to_value(&cfg.controls.models).expect("models serialize");
    let (link, server) = match net_arg {
        NetArg::Serve(_) => return Err("--serve runs no game of its own".into()),
        NetArg::Host(addr) => {
            if !args.level_explicit {
                return Err("--host needs a --level".into());
            }
            let level = level_path.to_string_lossy().into_owned();
            let game = lobby::NetGame::of_level(&level)
                .ok_or_else(|| format!("--host: {level} is no baked level of a game"))?;
            if args.net_game.is_some_and(|g| g != game) {
                return Err(format!(
                    "--host: {level} is not a {} level",
                    args.net_game.map_or("", |g| g.title())
                ));
            }
            let server = net::ServerHandle::spawn(
                addr,
                net::ServerOpts {
                    game,
                    passphrase: args.passphrase.clone(),
                    build: build.clone(),
                    level: level_path.to_string_lossy().into_owned(),
                    rules: rules::session(cfg),
                    admin_key: None,
                    frozen_after: net::FROZEN_AFTER,
                },
                ServerWorld::boxed,
            )?;
            println!(
                "net: hosting on {} as seat {} — waiting for --join",
                server.addr(),
                args.seat + 1
            );
            let link = net::join(
                &server.local_addr(),
                Some(game),
                Some(args.seat),
                models,
                &args.passphrase,
                &build,
                server.admin_key(),
            )?;
            (link, Some(server))
        }
        NetArg::Join(addr) => {
            // No `--seat` (seat 1, the default): the server assigns one.
            let link = net::join(
                addr,
                args.net_game,
                (args.seat != 0).then_some(args.seat),
                models,
                &args.passphrase,
                &build,
                "",
            )?;
            (link, None)
        }
    };
    *level_path = PathBuf::from(&link.game.level);
    // Everyone plays by the session's rules.
    let changed = rules::overridden(cfg, &link.game.rules);
    if !changed.is_empty() {
        println!("net: the session's rules override {}", changed.join(", "));
    }
    rules::adopt(cfg, &link.game.rules)?;
    LOCAL_SEAT.store(link.local.0, std::sync::atomic::Ordering::Relaxed);
    Ok((link, server))
}

/// `--serve [ADDR] --game G`: the DEDICATED server — headless, in the
/// foreground until killed; every player joins it, the first one in
/// is the admin. It plays ONE game, named on the command line (a player
/// of another is refused). The lobby opens on `--level` (one of that
/// game's; default: its first arena) under the default session rules.
fn run_server(addr: &str, args: &Args) -> std::process::ExitCode {
    let Some(game) = args.net_game else {
        eprintln!("error: --serve needs --game mc1|mc1hw|mc2 (the one game its sessions play)");
        return std::process::ExitCode::from(2);
    };
    let level = if args.level_explicit {
        args.level.to_string_lossy().into_owned()
    } else {
        get_baked_directory()
            .join(format!(
                "{}/level-{:03}.mgcl",
                game.key(),
                game.arenas().start()
            ))
            .to_string_lossy()
            .into_owned()
    };
    if lobby::NetGame::of_level(&level) != Some(game) {
        eprintln!("error: --serve: {level} is not a {} level", game.title());
        return std::process::ExitCode::from(2);
    }
    let opts = net::ServerOpts {
        game,
        passphrase: args.passphrase.clone(),
        build: net::build_id(),
        level,
        rules: rules::session(&config::Config::default()),
        admin_key: None,
        frozen_after: net::FROZEN_AFTER,
    };
    match net::serve(addr, opts, ServerWorld::boxed()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Root of the baked data tree — the single seam every baked lookup
/// resolves through, so a port or packaging layout relocates the
/// whole tree by changing one body (the Android shell points it at
/// the device's shared storage; `saves::saves_root` is its sibling).
fn get_baked_directory() -> PathBuf {
    paths::baked_dir()
}

#[cfg(test)]
mod lobby_rules_tests {
    use super::{LOBBY_RULES, config, lobby_rule_rows, rules, settings, step_lobby_rule};

    /// A lobby choice steps the SESSION's rules only: the rules seeded
    /// from the host's config move, the config itself does not (player
    /// 2026-10-16: a lobby fog leaked into single-player).
    #[test]
    fn a_lobby_rule_step_leaves_the_players_config_alone() {
        let specs = settings::registry();
        let mut own = config::Config::default();
        own.render.preference.fog_distance = 90;
        let seeded = rules::session(&own);
        let fog = |r: &serde_json::Value| r["render.preference.fog_distance"].clone();
        let down = step_lobby_rule(&seeded, 0, false, &specs).expect("fog steps down");
        assert_ne!(fog(&down), fog(&seeded));
        assert_eq!(own.render.preference.fog_distance, 90);
        // Every other pin holds the session's value.
        let mut a = down.clone();
        let mut b = seeded.clone();
        a.as_object_mut()
            .unwrap()
            .remove("render.preference.fog_distance");
        b.as_object_mut()
            .unwrap()
            .remove("render.preference.fog_distance");
        assert_eq!(a, b);
        assert!(step_lobby_rule(&seeded, LOBBY_RULES.len(), true, &specs).is_none());
    }

    /// Every row of the lobby's rules scroll is a SESSION PIN (else the
    /// host's change would never reach the clients) and reads a value.
    #[test]
    fn every_scroll_row_is_a_pin_and_reads() {
        for (label, path) in LOBBY_RULES {
            assert!(
                rules::HOST_CHOSEN.contains(&path),
                "{label}: {path} is no host-chosen session value"
            );
        }
        let specs = settings::registry();
        let rows = lobby_rule_rows(&rules::collect(&config::Config::default()), &specs);
        assert_eq!(rows.len(), LOBBY_RULES.len());
        for r in &rows {
            assert!(
                r.value != "?" && !r.value.is_empty(),
                "{}: no value",
                r.label
            );
        }
    }

    /// A lobby game's given names replace the humans' table names on the
    /// tags and the roster; the rest keep theirs.
    #[test]
    fn given_names_replace_table_names() {
        let mk = |slot: u8, name: &str| mgc_sim::engine::world::RivalView {
            slot,
            name: name.into(),
            alive: true,
            eliminated: false,
            x: 0.0,
            z: 0.0,
            alt: 0.0,
            mana: 0,
            mana_max: 0,
            life_frac: 1.0,
            kills: [0; 8],
            invisible: false,
            metamorphed: false,
        };
        let mut views = vec![mk(1, "Nyphur"), mk(3, "Belix")];
        super::name_humans(&[(1, "BRAN".into())], &mut views);
        assert_eq!(
            (views[0].name.as_str(), views[1].name.as_str()),
            ("BRAN", "Belix")
        );
    }

    /// The scroll's left/right step: the fog walks its stops, a toggle
    /// flips, and a read-back row follows.
    #[test]
    fn a_step_moves_one_notch() {
        let specs = settings::registry();
        let spec = |p: &str| specs.iter().find(|s| s.cfg_path == p).unwrap();
        let mut cfg = config::Config::default();
        let fog = cfg.render.preference.fog_distance;
        assert!(settings::step(
            spec("render.preference.fog_distance"),
            &mut cfg,
            false
        ));
        assert!(cfg.render.preference.fog_distance < fog);
        assert!(settings::step(
            spec("render.preference.fog_distance"),
            &mut cfg,
            true
        ));
        assert_eq!(cfg.render.preference.fog_distance, fog);
        let ghost = cfg.gameplay.cheat.ghost;
        assert!(settings::step(spec("gameplay.cheat.ghost"), &mut cfg, true));
        assert_ne!(cfg.gameplay.cheat.ghost, ghost);
        // The scroll reads the session's fog back.
        cfg.render.preference.fog_distance = 40;
        let rows = lobby_rule_rows(&rules::session(&cfg), &specs);
        assert!(rows[0].value.contains("40"), "{}", rows[0].value);
    }
}

#[cfg(test)]
mod replay_gate_tests {
    use super::{mc2_map_launch_replayed, saves::Mc2Save};

    /// The map raises the gate on a COMPLETED portal only: main
    /// portals below `levels_completed`, secrets at `activated == 1`.
    /// Both arms asserted, so a helper that always answers one way
    /// cannot pass.
    #[test]
    fn map_launch_gates_completed_portals_only() {
        let mut save = Mc2Save {
            levels_completed: 8,
            ..Default::default()
        };
        assert!(mc2_map_launch_replayed(&save, 0));
        assert!(
            mc2_map_launch_replayed(&save, 7),
            "the last completed main level"
        );
        assert!(!mc2_map_launch_replayed(&save, 8), "the pending main level");

        let secret = save.secrets[0].level as u32;
        save.secrets[0].activated = 2;
        assert!(
            !mc2_map_launch_replayed(&save, secret),
            "a revealed-uncompleted secret"
        );
        save.secrets[0].activated = 1;
        assert!(mc2_map_launch_replayed(&save, secret), "a completed secret");
    }
}

#[cfg(test)]
mod death_greyscale_tests {
    use super::death_greyscale;
    use mgc_sim::engine::world::LifeState;

    /// The fall flickers: six grey ticks while `(Turn / 6) & 1` is set,
    /// then the 4-step fade home and two colour ticks; the corpse is
    /// solid grey and the living wizard untouched.
    #[test]
    fn the_fall_flickers_and_the_corpse_stays_grey() {
        let fall: Vec<f32> = (0..12)
            .map(|t| death_greyscale(LifeState::Falling, t))
            .collect();
        assert_eq!(
            fall,
            [0.75, 0.5, 0.25, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
        );
        assert_eq!(death_greyscale(LifeState::Falling, 18), 1.0, "periodic");
        for t in [0, 5, 6, 1000] {
            assert_eq!(death_greyscale(LifeState::Dead, t), 1.0);
            assert_eq!(death_greyscale(LifeState::Alive, t), 0.0);
        }
    }
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

/// ⭐ NETWORK TRACK — two peers, each with its OWN world built through
/// the app's level path, stepped through the real TCP lockstep: the
/// worlds must stay hash-identical tick for tick, and the remote seat's
/// input must reach both. Self-skips without baked data.
#[cfg(test)]
mod net_lockstep_tests {
    use super::{SpellbookSeed, config, load_level, net, replay};
    use mgc_sim::{FlightInput, Hx, Simulation};
    use std::path::PathBuf;

    fn level(game: &str, index: u32) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../baked")
            .join(game)
            .join(format!("level-{index:03}.mgcl"))
    }

    /// A peer's sim, built the way `install_level` builds a session.
    fn peer_sim(path: &std::path::Path, seats: &[u8]) -> Option<Simulation> {
        let level = load_level(
            path,
            None,
            true,
            &SpellbookSeed::Level,
            &[],
            None,
            None,
            false,
            seats,
        )
        .ok()?;
        let mut sim = Simulation::with_world(level.world?);
        if let Some(start) = level.start {
            sim.flyer[Hx::FIRST] = start;
            sim.sync_carpet_from_flyer(Hx::FIRST);
        }
        Some(sim)
    }

    /// Seat `k`'s scripted controls at tick `t`: weaving flight, a
    /// cast now and then — different per seat.
    fn script(k: usize, t: u64) -> FlightInput {
        let phase = (t / 40 + k as u64) % 4;
        FlightInput {
            thrust: if phase == 3 { 0.0 } else { 1.0 },
            mc1_move_byte: Some(if phase == 3 { 0 } else { 1 }),
            stick_x: [60, -40, 0, 90][phase as usize] * if k == 0 { 1 } else { -1 },
            stick_y: (t % 30) as i16 - 15,
            fire_left: t % (50 + 13 * k as u64) == 0,
            ..FlightInput::default()
        }
    }

    /// A player's world as the app builds it for a lobby game
    /// (`net_level` + `session_sim`, the session's rules over the
    /// defaults, every seat its player's models).
    fn client_sim(game: &net::Game) -> Simulation {
        let mut cfg = config::Config::default();
        super::rules::adopt(&mut cfg, &game.rules).unwrap();
        let models = super::net_seat_models(&game.models, &cfg.controls.models);
        let mut level = super::net_level(game, &cfg, None, true).unwrap();
        super::session_sim(&mut level, &cfg, &models, None)
    }

    /// Seat `k`'s flight models: alternating enhanced / classic.
    fn models_of(k: usize) -> serde_json::Value {
        let mut m = config::Config::default().controls.models;
        if k % 2 == 0 {
            m.thrust = config::ThrustModel::Enhanced;
            m.altitude = config::AltitudeModel::Enhanced;
        } else {
            m.thrust = config::ThrustModel::Classic;
            m.altitude = config::AltitudeModel::Classic;
        }
        serde_json::to_value(m).unwrap()
    }

    /// A HOSTED server (the app's own referee, `ServerWorld`) on `path`
    /// and `n` players through the real lobby — the first claims the
    /// admin role with the key, sets `ai` and presses Play once the
    /// rest agreed. The links, member order.
    fn session(path: &std::path::Path, n: usize, ai: u8) -> (net::ServerHandle, Vec<net::Link>) {
        let server = net::ServerHandle::spawn(
            "127.0.0.1:0",
            net::ServerOpts {
                game: super::lobby::NetGame::of_level(&path.to_string_lossy()).unwrap(),
                passphrase: "pw".into(),
                build: "t".into(),
                level: path.to_string_lossy().into_owned(),
                rules: super::rules::session(&config::Config::default()),
                admin_key: None,
                frozen_after: net::FROZEN_AFTER,
            },
            super::ServerWorld::boxed,
        )
        .unwrap();
        let addr = server.local_addr();
        let mut gs: Vec<net::Guest> = (0..n)
            .map(|k| {
                let key = if k == 0 { server.admin_key() } else { "" };
                let mut g = net::Guest::connect(
                    &addr,
                    "pw",
                    &format!("P{k}"),
                    None,
                    "t",
                    key,
                    std::time::Duration::from_secs(5),
                )
                .unwrap();
                g.set_models(models_of(k));
                g
            })
            .collect();
        assert!(gs[0].view().is_admin());
        gs[0].set_ai(ai);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "the lobby never agreed"
            );
            gs.iter_mut().for_each(net::Guest::poll);
            let v = gs[0].view().clone();
            if v.ai == ai && v.all_agreed() && v.members.iter().all(|m| !m.models.is_null()) {
                break;
            }
            for g in &mut gs[1..] {
                let v = g.view();
                if v.ai == ai && v.me().is_some_and(|m| !m.agreed) {
                    g.agree();
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        gs[0].play();
        let links = gs
            .into_iter()
            .map(|mut g| {
                while !g.started() {
                    assert!(std::time::Instant::now() < deadline, "never started");
                    g.poll();
                }
                g.into_link().ok().unwrap()
            })
            .collect();
        (server, links)
    }

    /// N players through the REAL lobby and lockstep against a hosted
    /// server running its own world: every player's world stays
    /// hash-identical to the others' AND the server's (no desync
    /// reported), and every seat flies.
    fn n_peers_stay_identical(game: &str, arena: u32, n: usize, ai: u8) {
        n_peers(game, arena, n, ai, None);
    }

    /// A player leaving mid-game ends nothing for the rest: the others
    /// keep stepping, identical, and every one of them banishes the
    /// leaver on the same tick — the wizard falls, never to respawn
    /// (player 2026-10-17: a leaver's carpet idled on as a contender).
    /// The hosting player (seat 0, the admin) leaves like anyone.
    #[test]
    fn a_leaving_player_is_banished_and_the_game_goes_on() {
        n_peers("mc2", 50, 3, 0, Some((1, 300)));
        n_peers("mc1", 50, 3, 0, Some((0, 300)));
    }

    /// `leave`: (peer k, tick) — that peer hangs up once it has stepped
    /// the tick.
    fn n_peers(game: &str, arena: u32, n: usize, ai: u8, leave: Option<(usize, u64)>) {
        let path = level(game, arena);
        if !path.exists() {
            eprintln!("skipping: no baked {game} arena {arena}");
            return;
        }
        let (_server, links) = session(&path, n, ai);
        let seats: Vec<u8> = (0..n as u8).collect();
        let mut sims: Vec<Simulation> = links.iter().map(|l| client_sim(&l.game)).collect();
        let mut peers: Vec<Option<net::Lockstep>> = links
            .into_iter()
            .map(|link| {
                assert_eq!(link.game.seats, seats);
                assert_eq!(link.game.ai, ai);
                Some(net::Lockstep::start(link))
            })
            .collect();
        let starts: Vec<_> = (0..n).map(|k| sims[0].flyer[Hx(k as u8)]).collect();
        const TICKS: u64 = 1200;
        let mut stepped = vec![0u64; n];
        let mut hashes = vec![Vec::new(); n];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let mut banished_at = vec![None; n];
        while (0..n).any(|k| peers[k].is_some() && stepped[k] < TICKS) {
            assert!(std::time::Instant::now() < deadline, "lockstep stalled");
            for k in 0..n {
                if leave == Some((k, stepped[k])) {
                    peers[k] = None;
                }
                let Some(p) = &mut peers[k] else { continue };
                p.poll();
                assert!(p.closed().is_none(), "peer {k}: {:?}", p.closed());
                while p.wants_input() {
                    let t = stepped[k] + net::INPUT_DELAY;
                    let input = if p.in_lead_in() {
                        FlightInput::default()
                    } else {
                        script(k, t)
                    };
                    p.submit(replay::port_input_from(&input));
                }
                if stepped[k] < TICKS
                    && let Some((t, inputs)) = p.take_turn()
                {
                    let sim = &mut sims[k];
                    p.note_hash(t, || sim.state_hash());
                    let left = p.left_at(t);
                    for &seat in &left {
                        sim.world.as_mut().unwrap().depart(Hx(seat));
                        banished_at[k] = Some(t);
                    }
                    let inputs: Vec<FlightInput> =
                        inputs.iter().map(replay::flight_input_from).collect();
                    sim.step_seats(&inputs);
                    // The app's per-tick drain (`audio_tick`).
                    super::drain_sounds(sim.world.as_mut().unwrap());
                    hashes[k].push(sim.state_hash());
                    stepped[k] += 1;
                }
            }
        }
        let stayed: Vec<usize> = (0..n).filter(|&k| peers[k].is_some()).collect();
        for &k in &stayed {
            if let Some(t) = (0..TICKS as usize).find(|&t| hashes[stayed[0]][t] != hashes[k][t]) {
                panic!("{game}: peer {k} forked at tick {t}");
            }
            let p = peers[k].as_mut().unwrap();
            assert!(
                p.take_desync().is_none(),
                "{game}: peer {k} forked from the server"
            );
        }
        if let Some((gone, at)) = leave {
            let hx = Hx(gone as u8);
            let watcher = stayed[0];
            for &k in &stayed {
                let t = banished_at[k].expect("the leaver was never banished");
                assert!(t > at, "{game}: banished at {t}, before it left at {at}");
                assert_eq!(
                    banished_at[k], banished_at[watcher],
                    "{game}: one tick for all"
                );
                let w = sims[k].world.as_ref().unwrap();
                assert!(w.banished(hx), "{game}: peer {k}");
                assert!(!w.contenders().0.contains(&hx));
                // The others read the banishment; the leaver's own
                // nameless "has died." is his alone.
                let me = Hx(k as u8);
                let lines: Vec<&str> = w.notifications_for(me).map(|(t, _)| t).collect();
                assert!(
                    lines
                        .iter()
                        .any(|t| t.ends_with("banished from the realm.")),
                    "{game}: {lines:?}"
                );
                assert!(!lines.contains(&"has died."), "{game}: {lines:?}");
                // And the leaver is off the others' roster.
                let row = w
                    .human_views(me)
                    .into_iter()
                    .find(|r| r.slot == w.human_wizard_seat(hx).min(7) as u8);
                assert!(
                    row.is_none_or(|r| r.eliminated),
                    "{game}: still on the roster"
                );
                assert!(
                    w.vitals(hx).state != mgc_sim::engine::world::LifeState::Alive,
                    "{game}: the leaver's wizard still flies on peer {k}"
                );
            }
            return;
        }
        for (k, s) in starts.iter().enumerate() {
            let f = sims[0].flyer[Hx(k as u8)];
            assert!(
                (f.x - s.x).abs() + (f.z - s.z).abs() > 1.0,
                "{game}: seat {k} never flew"
            );
        }
    }

    /// The server names the winner: a two-player game where one human
    /// is banished (he leaves) ends with the other's verdict on every
    /// player still in it — on the turn after the server's world
    /// decided, the same tick for all.
    #[test]
    fn the_servers_verdict_names_the_last_wizard_standing() {
        let path = level("mc2", 50);
        if !path.exists() {
            eprintln!("skipping: no baked mc2 arena");
            return;
        }
        let (_server, links) = session(&path, 3, 0);
        let mut sims: Vec<Simulation> = links.iter().map(|l| client_sim(&l.game)).collect();
        let mut peers: Vec<Option<net::Lockstep>> = links
            .into_iter()
            .map(|l| Some(net::Lockstep::start(l)))
            .collect();
        let mut verdicts: Vec<Option<(u64, String)>> = vec![None; 3];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut stepped = [0u64; 3];
        while verdicts[0].is_none() {
            assert!(std::time::Instant::now() < deadline, "no verdict");
            for k in 0..3 {
                // Seats 1 and 2 leave early: seat 0 stands alone.
                if k > 0 && stepped[k] == 50 {
                    peers[k] = None;
                }
                let Some(p) = &mut peers[k] else { continue };
                p.poll();
                while p.wants_input() {
                    p.submit(replay::port_input_from(&FlightInput::default()));
                }
                if let Some((t, inputs)) = p.take_turn() {
                    let sim = &mut sims[k];
                    for seat in p.left_at(t) {
                        sim.world.as_mut().unwrap().depart(Hx(seat));
                    }
                    let inputs: Vec<FlightInput> =
                        inputs.iter().map(replay::flight_input_from).collect();
                    sim.step_seats(&inputs);
                    super::drain_sounds(sim.world.as_mut().unwrap());
                    stepped[k] += 1;
                    if let Some(v) = p.take_verdict() {
                        verdicts[k] = Some((t, v));
                    }
                }
            }
        }
        let (t, v) = verdicts[0].clone().unwrap();
        assert_eq!(v, "P0 is victorious!");
        // The server decided on the step of tick t - 1 — as did this
        // player's own world (it has stepped t by now).
        let w = sims[0].world.as_ref().unwrap();
        assert!(
            super::net_verdict(w, &[(0, "P0".into())]).is_some(),
            "tick {t}"
        );
    }

    /// A spell pick is its own player's news: the other human sees no
    /// toast, and the chime is tagged for the picker (the app plays
    /// only its own seat's player sounds).
    #[test]
    fn a_spell_pick_toasts_and_chimes_for_its_own_player_only() {
        let Some(mut sim) = peer_sim(&level("mc2", 50), &[0, 1]) else {
            eprintln!("skipping: no baked mc2 arena");
            return;
        };
        let w = sim.world.as_mut().unwrap();
        w.set_dev_spells(Hx(1), true);
        let _ = w.take_audio(
            Hx(0),
            mgc_sim::engine::world::PlayerPose::from_tiles(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        );
        w.mc2_select_spell(Hx(1), 0, 0, 0);
        assert_eq!(
            w.notifications_for(Hx(0)).count(),
            0,
            "not the other player's toast"
        );
        assert_eq!(w.notifications_for(Hx(1)).count(), 1, "the picker's own");
        let frame = w.take_audio(
            Hx(0),
            mgc_sim::engine::world::PlayerPose::from_tiles(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        );
        let chimes: Vec<_> = frame
            .events
            .iter()
            .filter(|e| e.player && e.id == 14)
            .collect();
        assert!(!chimes.is_empty(), "the pick chimed");
        assert!(
            chimes
                .iter()
                .all(|e| e.tag == w.human_tag(Hx(1)) && e.tag != w.human_tag(Hx(0)))
        );
    }

    /// A network world (`load_level_with`, `ai` rivals), the first
    /// seat at its start marker.
    fn net_sim(game: &str, arena: u32, seats: &[u8], ai: u8) -> Option<Simulation> {
        let l = super::load_level_with(
            &level(game, arena),
            None,
            true,
            &SpellbookSeed::Level,
            &[],
            None,
            None,
            false,
            seats,
            Some(ai),
        )
        .ok()?;
        let mut sim = Simulation::with_world(l.world?);
        if let Some(start) = l.start {
            sim.flyer[Hx::FIRST] = start;
            sim.sync_carpet_from_flyer(Hx::FIRST);
        }
        Some(sim)
    }

    /// The session's AI count seats exactly that many rivals — on a
    /// "one player" arena too — beside every human.
    #[test]
    fn the_ai_count_seats_that_many_rivals() {
        for (game, arena) in [("mc2", 51), ("mc1", 50)] {
            for ai in [0u8, 3] {
                let Some(mut sim) = net_sim(game, arena, &[0, 1], ai) else {
                    eprintln!("skipping: no baked {game} arena {arena}");
                    return;
                };
                // The wizard records pose on the first ticks (the
                // original's join commands run then).
                for _ in 0..4 {
                    sim.step_seats(&[FlightInput::default(); 2]);
                }
                let w = sim.world.as_ref().unwrap();
                assert_eq!(
                    w.rival_views().len(),
                    usize::from(ai),
                    "{game} {arena} ai {ai}"
                );
                assert_eq!(
                    w.human_views(Hx::FIRST).len(),
                    1,
                    "{game}: the second human stands"
                );
            }
        }
    }

    /// Ruling 10-15: a castle-less death in a network game BANISHES the
    /// seat — no level restart for everyone, no respawn — and the other
    /// human alone contends.
    #[test]
    fn a_castle_less_death_in_a_network_game_banishes() {
        use mgc_sim::engine::world::LifeState;
        for game in ["mc2", "mc1"] {
            let Some(mut sim) = net_sim(game, 50, &[0, 1], 0) else {
                eprintln!("skipping: no baked {game} arena");
                return;
            };
            sim.world.as_mut().unwrap().debug_kill_player(Hx(1));
            let idle = [FlightInput::default(); 2];
            let press = [
                FlightInput::default(),
                FlightInput {
                    respawn: true,
                    ..FlightInput::default()
                },
            ];
            let mut t = 0;
            while sim.world.as_ref().unwrap().vitals(Hx(1)).state != LifeState::Dead {
                sim.step_seats(&idle);
                t += 1;
                assert!(t < 3000, "{game}: seat 2 never came to rest dead");
            }
            // Down with no castle, Space never pressed: already out —
            // the last wizard standing has won (player 2026-10-17: the
            // game waited for every corpse to rise first).
            let w = sim.world.as_ref().unwrap();
            assert!(!w.banished(Hx(1)), "{game}: not banished yet");
            assert_eq!(w.contenders(), (vec![Hx::FIRST], vec![]), "{game}");
            assert!(
                super::net_verdict(w, &[]).is_some(),
                "{game}: the verdict waits for the corpse's Space"
            );
            // …while a corpse WITH a castle still contends (his Space
            // respawns him): the game goes on.
            let mut castled = net_sim(game, 50, &[0, 1], 0).unwrap();
            let cw = castled.world.as_mut().unwrap();
            cw.debug_plant_castle(Hx(1));
            cw.debug_kill_player(Hx(1));
            for _ in 0..t + 2 {
                castled.step_seats(&idle);
            }
            let cw = castled.world.as_ref().unwrap();
            assert_ne!(cw.vitals(Hx(1)).state, LifeState::Alive, "{game}: down");
            assert_eq!(cw.contenders().0, vec![Hx::FIRST, Hx(1)], "{game}");
            assert!(super::net_verdict(cw, &[]).is_none(), "{game}");
            for _ in 0..4 {
                sim.step_seats(&press);
            }
            for _ in 0..48 {
                sim.step_seats(&idle);
            }
            let w = sim.world.as_ref().unwrap();
            assert!(w.banished(Hx(1)), "{game}: banished");
            assert!(!w.banished(Hx::FIRST));
            assert!(
                !w.restart_pending() && !w.vitals(Hx(1)).lost,
                "{game}: no restart"
            );
            assert_eq!(
                w.vitals(Hx(1)).state,
                LifeState::Dead,
                "{game}: never respawns"
            );
            assert_eq!(w.contenders(), (vec![Hx::FIRST], vec![]), "{game}");
        }
    }

    /// ⭐ THE SPECTATOR GHOST (player ruling 2026-10-17): ONE Space at a
    /// castle-less corpse banishes the seat AND raises its ghost. The
    /// ghost flies on the seat's controls, faster than a carpet; the
    /// corpse stays where it fell, dead, and the seat contends no more.
    /// Two builds stepping the same inputs agree tick for tick, and a
    /// resync snapshot carries the ghost (its rise and its flight).
    #[test]
    fn a_banished_player_rises_as_a_ghost() {
        use mgc_sim::engine::world::LifeState;
        for game in ["mc2", "mc1"] {
            let (Some(mut a), Some(mut b)) =
                (net_sim(game, 50, &[0, 1], 0), net_sim(game, 50, &[0, 1], 0))
            else {
                eprintln!("skipping: no baked {game} arena");
                return;
            };
            let step = |sim: &mut Simulation, seat1: FlightInput| {
                sim.step_seats(&[FlightInput::default(), seat1]);
            };
            for sim in [&mut a, &mut b] {
                sim.world.as_mut().unwrap().debug_kill_player(Hx(1));
            }
            let mut t = 0;
            while a.world.as_ref().unwrap().vitals(Hx(1)).state != LifeState::Dead {
                step(&mut a, FlightInput::default());
                step(&mut b, FlightInput::default());
                t += 1;
                assert!(t < 3000, "{game}: seat 2 never came to rest dead");
            }
            let c = a.carpet[Hx(1)];
            let corpse = (c.x, c.y);
            let space = FlightInput {
                respawn: true,
                ..FlightInput::default()
            };
            step(&mut a, space);
            step(&mut b, space);
            let w = a.world.as_ref().unwrap();
            assert!(w.banished(Hx(1)) && w.risen(Hx(1)), "{game}: one Space");
            // Standing still, turning hard both ways: the carpet's bank
            // law — none without speed (player-reported 2026-10-17: the
            // first cut banked to the clamp on the spot).
            for t in 0..48 {
                let turn = FlightInput {
                    yaw_delta: if t < 24 { 0.3 } else { -0.3 },
                    ..FlightInput::default()
                };
                step(&mut a, turn);
                step(&mut b, turn);
                let roll = a.flyer[Hx(1)].roll;
                assert!(roll.abs() < 0.01, "{game}: banks {roll} standing still");
            }
            let from = a.flyer[Hx(1)];
            // Forward, turning a little, climbing a little: 10 s.
            let fly = FlightInput {
                thrust: 1.0,
                yaw_delta: 0.01,
                pitch_delta: 0.002,
                fire_left: true,
                ..FlightInput::default()
            };
            for t in 0..240 {
                step(&mut a, fly);
                step(&mut b, fly);
                assert_eq!(a.state_hash(), b.state_hash(), "{game}: tick {t}");
                if t == 120 {
                    b.net_restore(&a.net_snapshot()).unwrap();
                    assert!(b.world.as_ref().unwrap().risen(Hx(1)));
                }
            }
            let to = a.flyer[Hx(1)];
            // At full speed it banks as a carpet at full cruise does.
            let cruise = mgc_sim::enhanced_bank(2.4, 7.5);
            assert!(to.roll.abs() <= cruise + 1e-3, "{game}: banks {}", to.roll);
            let (dx, dz) = (to.x - from.x, to.z - from.z);
            let dist = (dx * dx + dz * dz).sqrt();
            // 10 s at twice the carpet's 7.5 tiles/s, less the turn's
            // chord and the ease-in.
            assert!(dist > 60.0, "{game}: the ghost flew {dist} tiles");
            let w = a.world.as_ref().unwrap();
            let c = a.carpet[Hx(1)];
            // (Its height settles on its own, risen or not.)
            assert_eq!((c.x, c.y), corpse, "{game}: the corpse lies still");
            assert_eq!(w.vitals(Hx(1)).state, LifeState::Dead, "{game}");
            assert_eq!(w.contenders().0, vec![Hx::FIRST], "{game}");
        }
    }

    /// ⭐ THE ARENA EXIT OPENS FOR THE LAST WIZARD STANDING (retail: MP
    /// installs one type-8 "kill all wizards" row per player, and the
    /// arenas' level-end release `(11,4)` spawns the exit pair once a
    /// player's row completes). With every human standing there is no
    /// exit and no completion (player-reported 2026-10-17: it opened on
    /// the first tick, for seat 0); once seat 0 falls castle-less, SEAT
    /// 1's row completes and the exit opens.
    #[test]
    fn an_mc2_arena_exit_opens_for_the_last_wizard_standing() {
        for arena in [51, 59] {
            let Some(mut sim) = net_sim("mc2", arena, &[0, 1], 0) else {
                eprintln!("skipping: no baked mc2 arena {arena}");
                return;
            };
            let idle = [FlightInput::default(); 2];
            for _ in 0..96 {
                sim.step_seats(&idle);
            }
            let w = sim.world.as_ref().unwrap();
            assert_eq!(
                w.debug_exit_trips(),
                0,
                "arena {arena}: an exit while both stand"
            );
            assert!(
                !w.completed(Hx::FIRST) && !w.completed(Hx(1)),
                "arena {arena}"
            );
            sim.world.as_mut().unwrap().debug_kill_player(Hx::FIRST);
            for _ in 0..96 {
                sim.step_seats(&idle);
            }
            let w = sim.world.as_ref().unwrap();
            assert!(w.completed(Hx(1)), "arena {arena}: the survivor's row");
            assert!(!w.completed(Hx::FIRST), "arena {arena}");
            assert_eq!(w.debug_exit_trips(), 1, "arena {arena}: the exit opens");
        }
    }

    #[test]
    fn four_mc2_peers_stay_identical() {
        n_peers_stay_identical("mc2", 50, 4, 0);
    }

    #[test]
    fn three_mc1_peers_stay_identical() {
        n_peers_stay_identical("mc1", 50, 3, 0);
    }

    /// Each human carpet wears ITS seat's wizard art (`sub_44D30`),
    /// seen from the other seat — not seat 0's row 44 for everyone.
    #[test]
    fn each_human_wears_his_seats_art() {
        for game in ["mc1", "mc2"] {
            let Some(mut sim) = (0..25).find_map(|i| peer_sim(&level(game, i), &[0, 1])) else {
                eprintln!("skipping: no baked {game} level seats two humans");
                continue;
            };
            for _ in 0..4 {
                sim.step_seats(&[FlightInput::default(); 2]);
            }
            let w = sim.world.as_ref().unwrap();
            for (viewer, seen) in [(Hx(1), 0u8), (Hx::FIRST, 1)] {
                let rows: Vec<u16> = w
                    .live_poses(viewer)
                    .iter()
                    .filter(|p| p.class == 3 && p.model == 0)
                    .map(|p| p.type_index)
                    .collect();
                let expect = match game {
                    "mc2" => mgc_sim::mc2::carpet_sprite_row(seen),
                    _ => [44, 273][usize::from(seen)],
                };
                assert_eq!(rows, vec![expect], "{game}: seat {} drawn wrong", seen + 1);
            }
        }
    }

    /// Each peer sees the other human the way it sees a rival: tagged
    /// and on the map roster under the seat's retail name.
    #[test]
    fn each_human_sees_the_other_on_tag_and_roster() {
        for game in ["mc1", "mc2"] {
            let Some(mut sim) = (0..25).find_map(|i| peer_sim(&level(game, i), &[0, 1])) else {
                eprintln!("skipping: no baked {game} level seats two humans");
                continue;
            };
            for _ in 0..4 {
                sim.step_seats(&[FlightInput::default(); 2]);
            }
            let w = sim.world.as_ref().unwrap();
            for (viewer, seen) in [(Hx(1), 0u8), (Hx::FIRST, 1)] {
                let views = w.human_views(viewer);
                assert_eq!(views.len(), 1, "{game}: one other human");
                let v = &views[0];
                let name = match game {
                    "mc2" => mgc_sim::mc2::rivals::MC2_RIVAL_NAMES[usize::from(seen)],
                    _ => mgc_sim::mc1::rivals::RIVAL_NAMES[usize::from(seen)],
                };
                assert_eq!((v.slot, v.name.as_str()), (seen, name), "{game}");
                assert!(v.alive && !v.eliminated && !v.invisible, "{game}");
                assert!(
                    w.rival_views().iter().all(|r| r.slot != seen),
                    "{game}: a human seat is no rival"
                );
            }
        }
    }

    /// Every MC2 arena (levels 50-59, the lobby's list) seats two humans
    /// — including the five whose header says one player (a network
    /// game seats the joined count).
    #[test]
    fn every_mc2_arena_seats_two_humans() {
        if !level("mc2", 50).exists() {
            eprintln!("skipping: no baked mc2 arenas");
            return;
        }
        for i in 50..=59 {
            let err = load_level(
                &level("mc2", i),
                None,
                true,
                &SpellbookSeed::Level,
                &[],
                None,
                None,
                false,
                &[0, 1],
            )
            .err();
            assert!(err.is_none(), "arena {i}: {err:?}");
        }
    }

    /// Every MC1 and HW arena (the PMULTI list: MC1 50-69, HW 50-59)
    /// authors all eight seats but MC1's Comari (69), which authors four
    /// (the lobby holds its members to them, `LobbyCore::set_open`); its
    /// authored seats all load together, and two humans load on its last
    /// two.
    #[test]
    fn every_mc1_arena_seats_its_authored_seats() {
        for (game, arenas) in [("mc1", 50..=69), ("mc1hw", 50..=59)] {
            if !level(game, 50).exists() {
                eprintln!("skipping: no baked {game} arenas");
                continue;
            }
            for i in arenas {
                let open = super::level_open_seats(&level(game, i));
                let want = match (game, i) {
                    ("mc1", 69) => 0x0F,
                    _ => 0xFF,
                };
                assert_eq!(open, want, "{game} arena {i}");
                let all: Vec<u8> = (0..8).filter(|k| open & 1 << k != 0).collect();
                for seats in [&all[..], &all[all.len() - 2..]] {
                    let err = load_level(
                        &level(game, i),
                        None,
                        true,
                        &SpellbookSeed::Level,
                        &[],
                        None,
                        None,
                        false,
                        seats,
                    )
                    .err();
                    assert!(err.is_none(), "{game} arena {i} seats {seats:?}: {err:?}");
                }
            }
        }
    }

    /// MC1: the other player sees a dead seat's carpet record neither
    /// at its grave nor after it is banished and its ghost flies off
    /// (player-reported 2026-10-17: the full-opacity carpet stayed at
    /// the corpse for good) — but sees it while that seat lives.
    #[test]
    fn a_dead_or_banished_seats_carpet_is_not_drawn() {
        use mgc_sim::engine::world::LifeState;
        let Some(mut a) = net_sim("mc1", 50, &[0, 1], 0) else {
            eprintln!("skipping: no baked mc1 arena");
            return;
        };
        let step = |sim: &mut Simulation, seat1: FlightInput| {
            sim.step_seats(&[FlightInput::default(), seat1]);
        };
        let carpets = |a: &Simulation| {
            a.world
                .as_ref()
                .unwrap()
                .live_poses(Hx(0))
                .into_iter()
                .filter(|p| (p.class, p.model) == (3, 0))
                .count()
        };
        for _ in 0..24 {
            step(&mut a, FlightInput::default());
        }
        let alive = carpets(&a);
        assert!(alive >= 1, "the other seat's carpet shows while it lives");
        a.world.as_mut().unwrap().debug_kill_player(Hx(1));
        let mut t = 0;
        while a.world.as_ref().unwrap().vitals(Hx(1)).state != LifeState::Dead {
            step(&mut a, FlightInput::default());
            t += 1;
            assert!(t < 3000, "seat 2 never came to rest dead");
        }
        assert_eq!(carpets(&a), alive - 1, "dead: not at the grave");
        step(
            &mut a,
            FlightInput {
                respawn: true,
                ..FlightInput::default()
            },
        );
        let fly = FlightInput {
            thrust: 1.0,
            ..FlightInput::default()
        };
        for _ in 0..120 {
            step(&mut a, fly);
        }
        assert!(a.world.as_ref().unwrap().risen(Hx(1)));
        assert_eq!(carpets(&a), alive - 1, "banished: never again");
    }

    /// A network game's humans all start with the arena's own book —
    /// retail's networked arm: wizard row 0, `starting_spells ∧
    /// allowed_spells` (Bussorah: Fireball, Possess, Create Castle) —
    /// not the empty campaign carry (player-reported 2026-10-17: no
    /// castle, unplayable). HW's Halaj (59) authors a wide start.
    #[test]
    fn network_humans_start_with_the_arenas_book() {
        for (game, arena, want) in [
            ("mc1", 50, vec![0u8, 3, 16]),
            ("mc1", 68, vec![]),
            ("mc1hw", 53, (0..24).collect()),
        ] {
            let Some(sim) = net_sim(game, arena, &[0, 5], 0) else {
                eprintln!("skipping: no baked {game} {arena}");
                continue;
            };
            let w = sim.world.as_ref().unwrap();
            for k in 0..2 {
                let owned: Vec<u8> = (0..24u8)
                    .filter(|&s| w.loadout(Hx(k)).owned[s as usize])
                    .collect();
                assert_eq!(owned, want, "{game} {arena} human {k}");
            }
        }
    }

    /// Two players and AI rivals (the rival brains in the lockstep),
    /// against the server's world.
    #[test]
    fn mc1_peers_and_rivals_stay_identical() {
        n_peers_stay_identical("mc1", 50, 2, 3);
    }

    #[test]
    fn mc2_peers_and_rivals_stay_identical() {
        n_peers_stay_identical("mc2", 52, 2, 3);
    }

    /// The desync recovery's vehicle: a FORKED world restored from the
    /// server's snapshot IS the server's world — hash-identical at once,
    /// and the same turns stepped on both never fork again (what the
    /// snapshot leaves out would). The authority has banished a seat
    /// the forked world never saw leave. A fresh build restored the
    /// same way (a rejoin) too — again every 100 ticks.
    fn a_snapshot_heals_a_fork(game: &str, arena: u32, n: usize, ai: u8) {
        let path = level(game, arena);
        if !path.exists() {
            eprintln!("skipping: no baked {game} arena {arena}");
            return;
        }
        let g = net::Game {
            seats: (0..n as u8).collect(),
            names: vec![String::new(); n],
            models: (0..n).map(models_of).collect(),
            ai,
            level: path.to_string_lossy().into_owned(),
            rules: super::rules::session(&config::Config::default()),
        };
        const LEAVE: u64 = 400;
        const FORK: u64 = 200;
        const SNAP: u64 = 900;
        const AFTER: u64 = 2400;
        let turn = |t: u64, forked: bool, left: bool| -> Vec<FlightInput> {
            (0..n)
                .map(|k| {
                    if k == n - 1 && left {
                        return FlightInput {
                            suicide: t == LEAVE,
                            ..FlightInput::default()
                        };
                    }
                    script(k + usize::from(forked && k == 1 && t >= FORK), t)
                })
                .collect()
        };
        let step = |sim: &mut Simulation, t: u64, forked: bool, leaves: bool| {
            if leaves && t == LEAVE {
                sim.world.as_mut().unwrap().depart(Hx(n as u8 - 1));
            }
            sim.step_seats(&turn(t, forked, leaves && t >= LEAVE));
            super::drain_sounds(sim.world.as_mut().unwrap());
        };
        let mut server = client_sim(&g);
        let mut forked = client_sim(&g);
        for t in 0..SNAP {
            step(&mut server, t, false, true);
            step(&mut forked, t, true, false);
        }
        assert_ne!(server.state_hash(), forked.state_hash(), "{game}: no fork");
        let snap = server.net_snapshot();
        let mut fresh = client_sim(&g);
        for (what, sim) in [("forked", &mut forked), ("fresh", &mut fresh)] {
            sim.net_restore(&snap).unwrap();
            let report = sim.divergence_report(&server);
            assert!(
                report.is_empty(),
                "{game}: the {what} world, restored: {report:#?}"
            );
        }
        for t in SNAP..AFTER {
            // The fresh world re-restores every 100 ticks: many states,
            // not one (beams in flight, recycle stacks, deaths…).
            if t % 100 == 0 {
                fresh.net_restore(&server.net_snapshot()).unwrap();
            }
            step(&mut server, t, false, true);
            for (what, sim) in [("forked", &mut forked), ("fresh", &mut fresh)] {
                step(sim, t, false, true);
                if sim.state_hash() == server.state_hash() {
                    continue;
                }
                let report = sim.divergence_report(&server);
                assert!(
                    report.is_empty(),
                    "{game}: the restored {what} world forked again at tick {t}: {report:#?}"
                );
            }
        }
        let w = forked.world.as_ref().unwrap();
        assert!(w.banished(Hx(n as u8 - 1)), "{game}: the leaver came back");
    }

    /// ⭐ RECONCILIATION end to end: two players and AI rivals against
    /// the hosted server's own world, through the real lockstep. Player
    /// 1's world forks by an app-side write no input explains (it
    /// banishes its own seat at tick 300, alone): the next check (312)
    /// catches it, the server's world comes back ahead of a later turn,
    /// the report names the seat, and from there on both players'
    /// worlds are tick-identical again — unbanished, the checks agreeing.
    fn a_forked_player_is_resynced(game: &str, arena: u32) {
        let path = level(game, arena);
        if !path.exists() {
            eprintln!("skipping: no baked {game} arena {arena}");
            return;
        }
        let (_server, links) = session(&path, 2, 2);
        let mut sims: Vec<Simulation> = links.iter().map(|l| client_sim(&l.game)).collect();
        let mut peers: Vec<net::Lockstep> = links.into_iter().map(net::Lockstep::start).collect();
        const FORK: u64 = 300;
        const TICKS: u64 = 900;
        let mut stepped = [0u64; 2];
        let mut hashes = [Vec::new(), Vec::new()];
        let mut healed_at = None;
        let mut told = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while stepped.iter().any(|&n| n < TICKS) {
            assert!(std::time::Instant::now() < deadline, "lockstep stalled");
            for k in 0..2 {
                let p = &mut peers[k];
                p.poll();
                assert!(p.closed().is_none(), "peer {k}: {:?}", p.closed());
                while p.wants_input() {
                    let t = stepped[k] + net::INPUT_DELAY;
                    let input = if p.in_lead_in() {
                        FlightInput::default()
                    } else {
                        script(k, t)
                    };
                    p.submit(replay::port_input_from(&input));
                }
                if stepped[k] < TICKS
                    && let Some((t, inputs)) = p.take_turn()
                {
                    let sim = &mut sims[k];
                    if let Some(snap) = p.take_snapshot(t) {
                        assert_eq!(k, 1, "{game}: only the forked player resyncs");
                        let outcome = super::net_resync(sim, &snap);
                        let report = outcome.clone().expect("the resync heals");
                        assert!(
                            report.iter().any(|l| l.starts_with("player[1]")),
                            "{game}: {report:#?}"
                        );
                        p.resynced(t, outcome.as_deref().map_err(Clone::clone));
                        healed_at = Some(t);
                    }
                    p.note_hash(t, || sim.state_hash());
                    if k == 1 && t == FORK {
                        sim.world.as_mut().unwrap().depart(Hx(1));
                    }
                    let inputs: Vec<FlightInput> =
                        inputs.iter().map(replay::flight_input_from).collect();
                    sim.step_seats(&inputs);
                    super::drain_sounds(sim.world.as_mut().unwrap());
                    hashes[k].push(sim.state_hash());
                    stepped[k] += 1;
                    told.extend(p.take_desync().map(|d| (k, d)));
                }
            }
        }
        let healed = healed_at.expect("never resynced") as usize;
        assert!(healed > 312, "{game}: resynced at {healed}");
        assert_ne!(hashes[0][FORK as usize], hashes[1][FORK as usize]);
        for t in healed..TICKS as usize {
            assert_eq!(
                hashes[0][t], hashes[1][t],
                "{game}: tick {t} after the resync"
            );
        }
        assert_eq!(told.len(), 2, "{game}: {told:?}");
        assert!(told[0].1.contains("DESYNC at tick 312"), "{told:?}");
        assert!(told[1].1.contains("resynced"), "{told:?}");
        assert_eq!(peers[1].desyncs(), (1, 1));
        assert_eq!(peers[0].desyncs(), (0, 0));
        assert!(!sims[1].world.as_ref().unwrap().banished(Hx(1)));
    }

    #[test]
    fn mc1_a_forked_player_is_resynced() {
        a_forked_player_is_resynced("mc1", 50);
    }

    #[test]
    fn mc2_a_forked_player_is_resynced() {
        a_forked_player_is_resynced("mc2", 52);
    }

    #[test]
    fn mc1_a_snapshot_heals_a_fork() {
        a_snapshot_heals_a_fork("mc1", 50, 3, 3);
    }

    #[test]
    fn mc2_a_snapshot_heals_a_fork() {
        a_snapshot_heals_a_fork("mc2", 52, 3, 3);
    }
}
