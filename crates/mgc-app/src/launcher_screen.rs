//! The launcher: the first screen of a plain `mgcarpet` start (no
//! `--campaign`, `--level`, replay or headless instrument). Three
//! game cards, the option presets beneath them, Exit and Start.
//!
//! It must stand up with NO baked data. That is the whole point: the
//! game used to bake (or refuse to start) before it had a window. So
//! everything the screen needs ships with the binary: the DejaVu
//! Serif Bold face (`assets/launcher/`, Bitstream Vera licence) drawn
//! through `ab_glyph`, and plain shapes for the rest. The cards show
//! retail art only when the game's bake is current: the
//! player's own data, never redistributed. An unbaked game gets a
//! crossed-out placeholder and cannot be started. The art is each
//! game's title card: the last frame of its title movie.
//!
//! When the baked tree is missing or stale (a first run, or a build
//! with a newer bake epoch) and the original data can be found, the
//! launcher bakes in the background and the cards light up when it
//! finishes. Adding data is the GAME-DATA dialog's job ("Game data…",
//! or a click on an unavailable card; src/launcher_data.rs does the
//! work): it shows what is found, opens the native file picker for a
//! GOG installer, a CD image or an installed folder, confirms, imports
//! with a progress bar, and the launcher then bakes. When NO game is
//! ready and nothing is baking, the dialog opens by itself — a first
//! run without data must not look like a broken program.
//!
//! The screen composes on the CPU at the window's own resolution (the
//! text stays crisp at any size), from a 960×720 layout scaled to fit
//! and centred, and is re-composed only when something visible
//! changes.
//!
//! The option rows set PRESETS (`settings::PresetGroup`): a group of
//! options written together, Enhanced or Classic, and read back from
//! those options rather than stored. A group already hand-tuned when
//! the launcher opened also offers "Custom", which puts the hand-tuned
//! values back. Changes apply and persist the way the options menu's
//! do. The Display row picks borderless fullscreen or one of the fixed
//! window sizes that fit the monitor; the launcher itself stays a
//! window, and the choice takes effect when the game starts.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Instant;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use mgc_render::UiQuad;

use crate::bakecheck::{self, BakeStatus};
use crate::campaign::CampaignId;
use crate::config::{Config, WindowSize};
use crate::launcher_data::{self, BROWSER_ROWS, DataDialog, Step, Want};
use crate::settings::{self, Preset, PresetGroup};

const FONT: &[u8] = include_bytes!("../../../assets/launcher/DejaVuSerif-Bold.ttf");

/// The layout's virtual canvas.
const VW: f32 = 960.0;
const VH: f32 = 720.0;

const GAMES: [(CampaignId, &str); 3] = [
    (CampaignId::Mc1, "Magic Carpet"),
    (CampaignId::Mc1Hw, "Magic Carpet: The Hidden Worlds"),
    (CampaignId::Mc2, "Magic Carpet 2: The Netherworlds"),
];

type Rgba = [u8; 4];
const GOLD: Rgba = [232, 196, 110, 255];
const TEXT: Rgba = [222, 216, 204, 255];
const MUTED: Rgba = [138, 132, 150, 255];
const PANEL: Rgba = [30, 26, 46, 235];
const EDGE: Rgba = [88, 78, 116, 255];
const HOVER: Rgba = [172, 152, 214, 255];
const CROSS: Rgba = [176, 44, 44, 230];
/// "Busy, please wait" — the bake in progress, loud on purpose: a
/// player who misses it quits mid-bake (player report 2026-11-16).
const BUSY: Rgba = [118, 228, 118, 255];

/// (x, y, w, h) in the virtual canvas.
type Rect = (f32, f32, f32, f32);

const CARD_W: f32 = 280.0;
const ART_H: f32 = 175.0;
/// Room for a two-line caption: a title with a subtitle ("Magic
/// Carpet 2: The Netherworlds") sets the subtitle on its own line.
const CAPTION_H: f32 = 54.0;
const CARD_Y: f32 = 150.0;
const CARD_GAP: f32 = 35.0;

fn card_rect(i: usize) -> Rect {
    let left = (VW - 3.0 * CARD_W - 2.0 * CARD_GAP) / 2.0;
    (
        left + i as f32 * (CARD_W + CARD_GAP),
        CARD_Y,
        CARD_W,
        ART_H + CAPTION_H,
    )
}

const OPT_Y: [f32; 3] = [428.0, 480.0, 532.0];
const OPT_H: f32 = 40.0;
const OPT_LABEL_RIGHT: f32 = 330.0;
const ARROW_W: f32 = 40.0;
const VALUE_X: f32 = 395.0;
const VALUE_W: f32 = 270.0;

fn arrow_rect(row: usize, right: bool) -> Rect {
    let x = if right {
        VALUE_X + VALUE_W + 5.0
    } else {
        VALUE_X - 5.0 - ARROW_W
    };
    (x, OPT_Y[row], ARROW_W, OPT_H)
}

fn value_rect(row: usize) -> Rect {
    (VALUE_X, OPT_Y[row], VALUE_W, OPT_H)
}

const BUTTON_Y: f32 = 618.0;
const BUTTON_W: f32 = 200.0;
const BUTTON_H: f32 = 58.0;

fn button_rect(start: bool) -> Rect {
    (
        if start { 510.0 } else { 250.0 },
        BUTTON_Y,
        BUTTON_W,
        BUTTON_H,
    )
}

/// The "Game data…" button, bottom left, beside Exit.
fn data_button_rect() -> Rect {
    (30.0, BUTTON_Y + 11.0, 170.0, 36.0)
}

/// The game-data dialog: a centred panel, up to three buttons along
/// its foot, an optional progress bar.
const DATA: Rect = (110.0, 70.0, 740.0, 600.0);

fn data_btn_rect(i: usize) -> Rect {
    (
        145.0 + i as f32 * 235.0,
        DATA.1 + DATA.3 - BUTTON_H - 24.0,
        BUTTON_W,
        BUTTON_H,
    )
}

const DATA_BAR: Rect = (160.0, 470.0, 640.0, 22.0);

/// The browser inside the dialog: a column of quick places at the
/// left, the folder listing at the right.
const PLACE_ROWS: usize = 12;

fn place_rect(i: usize) -> Rect {
    (130.0, 180.0 + i as f32 * 30.0, 170.0, 28.0)
}

fn row_rect(i: usize) -> Rect {
    (315.0, 180.0 + i as f32 * 26.0, 515.0, 26.0)
}

/// "What do you have?": one card per kind of game data.
fn choice_rect(i: usize) -> Rect {
    (140.0, 150.0 + i as f32 * 140.0, 680.0, 124.0)
}

/// The "Choose…" button drawn on card `i` (the whole card clicks).
fn choice_button_rect(i: usize) -> Rect {
    let (x, y, w, h) = choice_rect(i);
    (x + w - 180.0, y + h / 2.0 - 24.0, 160.0, 48.0)
}

/// What an option row sets.
enum RowKind {
    /// Fullscreen or a window size (`render.preference.fullscreen` /
    /// `window_size`).
    Display,
    /// A preset group.
    Preset(&'static PresetGroup),
}

/// A Display row choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Fullscreen,
    Window(WindowSize),
}

impl Mode {
    fn of(cfg: &Config) -> Mode {
        let p = &cfg.render.preference;
        if p.fullscreen {
            Mode::Fullscreen
        } else {
            Mode::Window(p.window_size)
        }
    }

    fn label(self) -> String {
        match self {
            Mode::Fullscreen => "Fullscreen".into(),
            Mode::Window(w) => {
                let (x, y) = w.dims();
                format!("Window {x} × {y}")
            }
        }
    }
}

const OPTIONS: [(&str, RowKind); 3] = [
    ("Display", RowKind::Display),
    ("Controls", RowKind::Preset(&settings::CONTROLS_PRESET)),
    ("Visuals", RowKind::Preset(&settings::VISUALS_PRESET)),
];

/// A preset row's choice.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Preset(Preset),
    /// The hand-tuned values the launcher opened with.
    Custom,
}

/// What the launcher asks of the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start(CampaignId),
    Exit,
}

/// Keyboard focus: the card row, an option row, or the button row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Cards,
    Option(usize),
    Buttons { start: bool },
}

/// Something under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Card(usize),
    Arrow {
        row: usize,
        right: bool,
    },
    Value(usize),
    Button {
        start: bool,
    },
    /// The launcher's "Game data…" button.
    DataOpen,
    /// One of the game-data dialog's buttons.
    Data(usize),
    /// The browser's quick place `i`.
    Place(usize),
    /// The browser's visible row `i`.
    Row(usize),
    /// "What do you have?" card `i` (a [`Want`]).
    Choice(usize),
}

/// How a dialog line is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ink {
    Head,
    Text,
    Muted,
    Path,
    Error,
    /// The bake in progress (green).
    Busy,
}

/// What the game-data dialog shows, derived from its state (the frame
/// re-composes when it changes).
#[derive(Clone, PartialEq, Debug)]
struct DataView {
    /// The panel's heading.
    title: &'static str,
    lines: Vec<(Ink, String)>,
    /// 0..=1 when a copy reports bytes.
    progress: Option<f32>,
    buttons: [Option<&'static str>; 3],
    /// The button Enter presses (drawn gold).
    primary: Option<usize>,
    /// The file browser, when it is open (it replaces `lines`).
    browser: Option<BrowserView>,
    /// The kind cards, when asking what the player has.
    choices: Option<Vec<ChoiceView>>,
}

/// One "What do you have?" card.
#[derive(Clone, PartialEq, Debug)]
struct ChoiceView {
    title: &'static str,
    description: &'static str,
    /// A blocking problem with this kind on this system (red).
    warning: Option<&'static str>,
}

/// What the browser shows.
#[derive(Clone, PartialEq, Debug)]
struct BrowserView {
    path: String,
    places: Vec<String>,
    /// (name, is a folder, looks like game data, selected).
    rows: Vec<(String, bool, bool, bool)>,
    /// "12–25 of 40", or an empty-folder / unreadable note.
    info: String,
    /// The folder itself is game data: what it is.
    here: Option<String>,
}

/// A card's baked title art, RGB.
struct Art {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

/// Everything the composed frame depends on; a change re-composes.
#[derive(Clone, PartialEq)]
struct ViewKey {
    size: (u32, u32),
    selected: Option<usize>,
    focus: Focus,
    hover: Option<Hit>,
    values: [String; 3],
    games: [bool; 3],
    status: String,
    /// The status line is the bake's (drawn big and green).
    baking: bool,
    data: Option<DataView>,
}

pub struct Launcher {
    font: FontRef<'static>,
    baked_root: PathBuf,
    status: BakeStatus,
    /// The background bake and when it started.
    bake: Option<(JoinHandle<Result<(), String>>, Instant)>,
    /// A standing message (no data found, a failed bake).
    message: Option<String>,
    art: [Option<Art>; 3],
    selected: Option<usize>,
    focus: Focus,
    hover: Option<Hit>,
    /// The monitor the window is on (physical px), when known: a
    /// window size that does not fit is not offered.
    monitor: Option<(u32, u32)>,
    /// Per row: the config as the launcher found it, kept when that
    /// row's preset group was hand-tuned (its "Custom").
    custom: [Option<Config>; 3],
    /// Option paths changed since the app last looked (registry
    /// paths), to apply and persist.
    changed: Vec<&'static str>,
    pending: Option<Action>,
    last: Option<ViewKey>,
    /// The game-data dialog, when open (modal).
    data: Option<DataDialog>,
    /// Where game data lives (the config's `gamedata`, if set).
    gamedata: Option<PathBuf>,
}

impl Launcher {
    /// Judge the baked tree and, when it needs (re)generating and the
    /// original data is found, start baking it in the background.
    pub fn new(baked_root: &Path, cfg: &Config) -> Self {
        let gamedata = cfg.gamedata.as_deref();
        let font = FontRef::try_from_slice(FONT).expect("the launcher font is compiled in");
        let status = bakecheck::status(baked_root);
        let mut l = Launcher {
            font,
            baked_root: baked_root.to_path_buf(),
            status,
            bake: None,
            message: None,
            art: [None, None, None],
            selected: None,
            focus: Focus::Cards,
            hover: None,
            monitor: None,
            custom: std::array::from_fn(|row| match &OPTIONS[row].1 {
                RowKind::Preset(g) if g.current(cfg).is_none() => Some(cfg.clone()),
                _ => None,
            }),
            changed: Vec::new(),
            pending: None,
            last: None,
            data: None,
            gamedata: cfg.gamedata.clone(),
        };
        if let Some(reason) = l.status.tree.clone() {
            match bakecheck::gamedata(gamedata) {
                Some(src) => {
                    println!("launcher: the baked data needs preparing ({reason})");
                    let root = l.baked_root.clone();
                    let handle = std::thread::spawn(move || bakecheck::bake(&src, &root));
                    l.bake = Some((handle, Instant::now()));
                }
                None => {
                    l.message = Some("No game data found. Add yours with “Game data…”.".into());
                }
            }
        }
        l.refresh();
        if l.nothing_ready() {
            l.open_data();
        }
        l
    }

    /// No game can start and no bake will change that.
    fn nothing_ready(&self) -> bool {
        self.bake.is_none() && !self.status.games.iter().any(|&g| g)
    }

    /// Open the game-data dialog.
    fn open_data(&mut self) {
        if self.data.is_none() {
            let root = launcher_data::gamedata_root(self.gamedata.as_deref());
            self.data = Some(DataDialog::new(root));
        }
    }

    /// Close the dialog, unless it is in the middle of something.
    fn close_data(&mut self) {
        if self.data.as_ref().is_some_and(|d| !d.busy()) {
            self.data = None;
        }
    }

    /// Press dialog button `i` (Enter presses the primary one).
    fn data_button(&mut self, i: usize) {
        let Some(d) = &mut self.data else { return };
        match &mut d.step {
            Step::Idle => match i {
                0 => d.choose(),
                _ => self.close_data(),
            },
            Step::Choosing => d.cancel(),
            Step::Browsing(b) => match i {
                0 => b.up(),
                1 if b.here.is_some() => {
                    let dir = b.dir.clone();
                    d.pick(dir);
                }
                1 => {}
                _ => d.close_browser(),
            },
            Step::Confirm { .. } => match i {
                0 => d.confirm(),
                _ => d.cancel(),
            },
            Step::Importing { .. } => d.cancel(),
            Step::Identifying(_) => {}
        }
    }

    /// A browser place or row was clicked.
    fn browser_click(&mut self, hit: Hit) {
        let Some(d) = &mut self.data else { return };
        if let Hit::Choice(i) = hit {
            d.browse(Want::ALL[i]);
            return;
        }
        let Step::Browsing(b) = &mut d.step else {
            return;
        };
        match hit {
            Hit::Place(i) => {
                if let Some((_, p)) = b.places.get(i).cloned() {
                    b.enter(p);
                }
            }
            Hit::Row(i) => {
                let row = b.scroll + i;
                b.selected = Some(row);
                if let Some(path) = b.activate(row) {
                    d.pick(path);
                }
            }
            _ => {}
        }
    }

    /// The mouse wheel: scrolls the browser's listing.
    pub fn scroll(&mut self, rows: f32) {
        if let Some(d) = &mut self.data
            && let Step::Browsing(b) = &mut d.step
        {
            b.scroll_by((rows * 3.0).round() as i64);
        }
    }

    /// The browser's own keys; true when consumed.
    fn browser_key(&mut self, k: winit::keyboard::NamedKey) -> bool {
        use winit::keyboard::NamedKey;
        let Some(d) = &mut self.data else {
            return false;
        };
        let Step::Browsing(b) = &mut d.step else {
            return false;
        };
        let page = BROWSER_ROWS as i64;
        match k {
            NamedKey::ArrowUp => b.step(-1),
            NamedKey::ArrowDown => b.step(1),
            NamedKey::PageUp => b.step(-page),
            NamedKey::PageDown => b.step(page),
            NamedKey::Home => b.step(-(b.entries.len() as i64)),
            NamedKey::End => b.step(b.entries.len() as i64),
            NamedKey::Backspace => b.up(),
            NamedKey::Enter => match b.selected {
                Some(i) => {
                    if let Some(path) = b.activate(i) {
                        d.pick(path);
                    }
                }
                None if b.here.is_some() => {
                    let dir = b.dir.clone();
                    d.pick(dir);
                }
                None => {}
            },
            _ => return false,
        }
        true
    }

    /// What the dialog shows now.
    fn data_view(&self) -> Option<DataView> {
        let d = self.data.as_ref()?;
        let mut lines: Vec<(Ink, String)> = Vec::new();
        let gap = (Ink::Text, String::new());
        let mut progress = None;
        let (buttons, primary): ([Option<&'static str>; 3], Option<usize>) = match &d.step {
            Step::Idle => {
                // A bake running: say so first, loudly — the dialog
                // dims the launcher's own status line.
                if let Some(line) = self.bake_line() {
                    lines.push((Ink::Busy, line));
                    lines.push((
                        Ink::Muted,
                        "The games unlock when it is done. The first time also \
                         renders the music, which takes a few minutes."
                            .into(),
                    ));
                    lines.push(gap.clone());
                }
                let found_any = d.found.iter().any(|f| f.used);
                lines.push((
                    if found_any { Ink::Muted } else { Ink::Text },
                    if found_any {
                        "Add more: a GOG installer (.exe), a CD image (.cue, .img or \
                         .bin), or an installed GOG folder."
                    } else {
                        "mgcarpet plays the original Magic Carpet games from your own \
                         copies, and none were found yet. Add yours: a GOG installer \
                         (.exe), a CD image (.cue, .img or .bin), or an installed GOG \
                         folder."
                    }
                    .into(),
                ));
                for (product, found) in d.summary() {
                    lines.push(gap.clone());
                    lines.push((Ink::Head, product.into()));
                    for l in found {
                        let ink = if l == "Not found" {
                            Ink::Muted
                        } else {
                            Ink::Text
                        };
                        lines.push((ink, l));
                    }
                }
                lines.push(gap.clone());
                lines.push((Ink::Muted, "Game data is kept in:".into()));
                lines.push((Ink::Path, absolute(&d.root)));
                if let Some((err, text)) = &d.outcome {
                    lines.push(gap.clone());
                    lines.push((if *err { Ink::Error } else { Ink::Text }, text.clone()));
                }
                ([Some("Add game data…"), None, Some("Close")], Some(0))
            }
            Step::Choosing => {
                let choices = Want::ALL
                    .iter()
                    .map(|&w| ChoiceView {
                        title: w.title(),
                        description: w.description(),
                        warning: (w == Want::Installer && !d.innoextract).then_some(
                            "Needs the innoextract tool, which is not installed (Linux: \
                             the innoextract package; macOS: brew install innoextract). \
                             On Windows, run the installer and add the installed folder.",
                        ),
                    })
                    .collect();
                return Some(DataView {
                    title: "What do you have?",
                    lines,
                    progress,
                    buttons: [None, None, Some("Back")],
                    primary: None,
                    browser: None,
                    choices: Some(choices),
                });
            }
            Step::Browsing(b) => {
                let rows = b
                    .entries
                    .iter()
                    .enumerate()
                    .skip(b.scroll)
                    .take(BROWSER_ROWS)
                    .map(|(i, e)| {
                        let name = if e.dir {
                            format!("{}/", e.name)
                        } else {
                            e.name.clone()
                        };
                        (name, e.dir, e.data, b.selected == Some(i))
                    })
                    .collect();
                let info = if let Some(e) = &b.error {
                    e.clone()
                } else if b.entries.is_empty() {
                    "No folders or game files here.".into()
                } else if b.entries.len() > BROWSER_ROWS {
                    format!(
                        "{}–{} of {}",
                        b.scroll + 1,
                        (b.scroll + BROWSER_ROWS).min(b.entries.len()),
                        b.entries.len()
                    )
                } else {
                    String::new()
                };
                let browser = BrowserView {
                    path: b.dir.display().to_string(),
                    places: b
                        .places
                        .iter()
                        .take(PLACE_ROWS)
                        .map(|(l, _)| l.clone())
                        .collect(),
                    rows,
                    info,
                    here: b.here.clone(),
                };
                let use_here = b.here.is_some().then_some("Use this folder");
                return Some(DataView {
                    title: b.want.title(),
                    lines,
                    progress,
                    buttons: [Some("Up one level"), use_here, Some("Back")],
                    primary: use_here.map(|_| 1),
                    browser: Some(browser),
                    choices: None,
                });
            }
            Step::Identifying(_) => {
                lines.push((Ink::Text, "Reading the game data…".into()));
                ([None; 3], None)
            }
            Step::Confirm { id, dest, taken } => {
                lines.push((Ink::Head, id.game.title().into()));
                lines.push((Ink::Text, format!("Found a {}.", id.kind.label())));
                lines.push(gap.clone());
                lines.push((Ink::Muted, "From:".into()));
                lines.push((Ink::Path, absolute(&id.path)));
                lines.push((Ink::Muted, "Into:".into()));
                lines.push((Ink::Path, absolute(dest)));
                for n in &id.notes {
                    lines.push(gap.clone());
                    lines.push((Ink::Text, format!("Note: {n}")));
                }
                if *taken {
                    lines.push(gap.clone());
                    lines.push((
                        Ink::Error,
                        "That folder already holds game data; importing replaces it.".into(),
                    ));
                }
                (
                    [
                        Some(if *taken { "Replace" } else { "Import" }),
                        None,
                        Some("Back"),
                    ],
                    Some(0),
                )
            }
            Step::Importing { .. } => {
                let p = d.progress();
                let stage = p.as_ref().map_or("Importing…".into(), |p| p.stage.clone());
                lines.push((Ink::Text, stage));
                progress = p
                    .filter(|p| p.total > 0)
                    .map(|p| p.done as f32 / p.total as f32);
                ([None, None, Some("Cancel")], None)
            }
        };
        Some(DataView {
            title: "Game data",
            lines,
            progress,
            buttons,
            primary,
            browser: None,
            choices: None,
        })
    }

    /// Re-read the art of every ready game and keep the selection on
    /// one of them.
    fn refresh(&mut self) {
        for i in 0..3 {
            self.art[i] = if self.status.games[i] {
                load_art(&self.baked_root, GAMES[i].0)
                    .map_err(|e| eprintln!("note: launcher art for {}: {e}", GAMES[i].1))
                    .ok()
            } else {
                None
            };
        }
        if self.selected.is_none_or(|i| !self.status.games[i]) {
            self.selected = (0..3).find(|&i| self.status.games[i]);
        }
    }

    /// Poll the background bake and the game-data dialog.
    pub fn tick(&mut self) {
        if let Some(d) = &mut self.data {
            d.tick();
        }
        // An import landed: bake from where it went (whatever the
        // tree's stamps say — a new game is not "stale"). A bake
        // already running finishes first; the request waits.
        if self.bake.is_none()
            && let Some(d) = self.data.as_mut().filter(|d| d.bake_requested)
        {
            d.bake_requested = false;
            let (src, root) = (d.root.clone(), self.baked_root.clone());
            println!("launcher: new game data, preparing it");
            self.message = None;
            let handle = std::thread::spawn(move || bakecheck::bake(&src, &root));
            self.bake = Some((handle, Instant::now()));
        }
        if self.bake.as_ref().is_some_and(|(h, _)| h.is_finished()) {
            let (handle, t0) = self.bake.take().expect("checked above");
            match handle.join() {
                Ok(Ok(())) => println!(
                    "launcher: game data ready ({:.0} s)",
                    t0.elapsed().as_secs_f32()
                ),
                Ok(Err(e)) => {
                    eprintln!("error: bake: {e}");
                    self.message = Some(format!("Preparing the game data failed: {e}"));
                }
                Err(_) => self.message = Some("Preparing the game data crashed.".into()),
            }
            self.status = bakecheck::status(&self.baked_root);
            self.refresh();
            if self.nothing_ready() {
                self.open_data();
            }
        }
    }

    /// Forget the last frame, so the next one re-composes (another
    /// screen took the uploaded image's slot).
    pub fn invalidate(&mut self) {
        self.last = None;
    }

    pub fn take_action(&mut self) -> Option<Action> {
        self.pending.take()
    }

    fn start(&mut self) {
        if let Some(i) = self.selected.filter(|&i| self.status.games[i]) {
            self.pending = Some(Action::Start(GAMES[i].0));
        }
    }

    fn step_selection(&mut self, right: bool) {
        let Some(cur) = self.selected else { return };
        let mut i = cur;
        for _ in 0..2 {
            i = if right { (i + 1) % 3 } else { (i + 2) % 3 };
            if self.status.games[i] {
                self.selected = Some(i);
                return;
            }
        }
    }

    /// A preset row's choices, in stepping order, and the current one.
    fn choices(&self, row: usize, g: &PresetGroup, cfg: &Config) -> (Vec<Choice>, usize) {
        let mut list: Vec<Choice> = Preset::ALL.into_iter().map(Choice::Preset).collect();
        if self.custom[row].is_some() {
            list.push(Choice::Custom);
        }
        let cur = match g.current(cfg) {
            Some(p) => Choice::Preset(p),
            None => Choice::Custom,
        };
        // Hand-tuned since the launcher opened can only be the
        // launcher's own "Custom"; otherwise it is on the list.
        let at = list
            .iter()
            .position(|&c| c == cur)
            .unwrap_or(list.len() - 1);
        (list, at)
    }

    pub fn set_monitor(&mut self, monitor: Option<(u32, u32)>) {
        self.monitor = monitor;
    }

    /// The Display row's choices: fullscreen, then every window size.
    /// Sizes larger than the monitor stay on offer (player ask
    /// 2026-10-17: a 4K window on a smaller screen is a performance
    /// check) — [`Launcher::value`] tags them instead.
    fn modes(&self) -> Vec<Mode> {
        std::iter::once(Mode::Fullscreen)
            .chain(WindowSize::ALL.into_iter().map(Mode::Window))
            .collect()
    }

    /// Whether a window size overflows the known monitor.
    fn oversized(&self, w: WindowSize) -> bool {
        let (x, y) = w.dims();
        self.monitor.is_some_and(|(mw, mh)| x > mw || y > mh)
    }

    /// The row's value as shown.
    fn value(&self, row: usize, cfg: &Config) -> String {
        match &OPTIONS[row].1 {
            RowKind::Display => match Mode::of(cfg) {
                Mode::Window(w) if self.oversized(w) => {
                    format!("{} (larger than screen)", Mode::Window(w).label())
                }
                m => m.label(),
            },
            RowKind::Preset(g) => match g.current(cfg) {
                Some(p) => p.label().to_string(),
                None => "Custom".to_string(),
            },
        }
    }

    fn step_option(&mut self, row: usize, right: bool, cfg: &mut Config) {
        match &OPTIONS[row].1 {
            RowKind::Display => {
                let list = self.modes();
                let n = list.len();
                let at = list.iter().position(|&m| m == Mode::of(cfg)).unwrap_or(0);
                let next = if right {
                    (at + 1) % n
                } else {
                    (at + n - 1) % n
                };
                let p = &mut cfg.render.preference;
                match list[next] {
                    Mode::Fullscreen => p.fullscreen = true,
                    Mode::Window(w) => {
                        p.fullscreen = false;
                        p.window_size = w;
                    }
                }
                self.changed.extend([
                    "render.preference.fullscreen",
                    "render.preference.window_size",
                ]);
            }
            RowKind::Preset(g) => {
                let (list, at) = self.choices(row, g, cfg);
                let n = list.len();
                let next = if right {
                    (at + 1) % n
                } else {
                    (at + n - 1) % n
                };
                match list[next] {
                    Choice::Preset(p) => (g.apply)(cfg, p),
                    Choice::Custom => {
                        if let Some(tuned) = &self.custom[row] {
                            g.copy_members(cfg, tuned);
                        }
                    }
                }
                self.changed.push(g.cfg_path);
            }
        }
    }

    /// The option paths changed since the last call (registry paths).
    pub fn take_changed(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.changed)
    }

    /// Esc leaves — or, in the game-data dialog, backs out one step
    /// (cancels a running import, closes when idle).
    pub fn escape(&mut self) {
        if let Some(d) = &mut self.data {
            match d.step {
                Step::Idle => self.close_data(),
                _ => d.cancel(),
            }
            return;
        }
        self.pending = Some(Action::Exit);
    }

    /// Keyboard navigation: Up/Down (and Tab) walk the rows,
    /// Left/Right act within one, Enter starts (or presses the focused
    /// button).
    pub fn key(&mut self, key: &winit::keyboard::Key, cfg: &mut Config) {
        use winit::keyboard::{Key, NamedKey};
        let Key::Named(k) = key else { return };
        if self.data.is_some() {
            if self.browser_key(*k) {
                return;
            }
            match k {
                NamedKey::Escape => self.escape(),
                NamedKey::Enter | NamedKey::Space => {
                    if let Some(i) = self.data_view().and_then(|v| v.primary) {
                        self.data_button(i);
                    }
                }
                _ => {}
            }
            return;
        }
        let rows = |f: Focus| match f {
            Focus::Cards => 0,
            Focus::Option(r) => r + 1,
            Focus::Buttons { .. } => 4,
        };
        let at = |r: usize| match r {
            0 => Focus::Cards,
            1..=3 => Focus::Option(r - 1),
            _ => Focus::Buttons { start: true },
        };
        match k {
            NamedKey::ArrowUp => self.focus = at(rows(self.focus).saturating_sub(1)),
            NamedKey::ArrowDown | NamedKey::Tab => self.focus = at((rows(self.focus) + 1).min(4)),
            NamedKey::ArrowLeft | NamedKey::ArrowRight => {
                let right = *k == NamedKey::ArrowRight;
                match self.focus {
                    Focus::Cards => self.step_selection(right),
                    Focus::Option(r) => self.step_option(r, right, cfg),
                    Focus::Buttons { .. } => self.focus = Focus::Buttons { start: right },
                }
            }
            NamedKey::Enter => match self.focus {
                Focus::Buttons { start: false } => self.pending = Some(Action::Exit),
                _ => self.start(),
            },
            NamedKey::Escape => self.escape(),
            _ => {}
        }
    }

    /// A left click at a window position.
    pub fn click(&mut self, size: (f32, f32), cursor: (f32, f32), cfg: &mut Config) {
        if let Some(view) = self.data_view() {
            match data_hit(size, cursor, &view) {
                Some(Hit::Data(i)) => self.data_button(i),
                Some(h) => self.browser_click(h),
                None => {}
            }
            return;
        }
        match hit(size, cursor) {
            Some(Hit::Card(i)) if self.status.games[i] => {
                self.selected = Some(i);
                self.focus = Focus::Cards;
            }
            Some(Hit::DataOpen) => self.open_data(),
            // An unavailable game: that is what the dialog is for —
            // unless a bake is already on its way to light it up.
            Some(Hit::Card(_)) if self.bake.is_none() => self.open_data(),
            Some(Hit::Arrow { row, right }) => {
                self.step_option(row, right, cfg);
                self.focus = Focus::Option(row);
            }
            Some(Hit::Value(row)) => {
                self.step_option(row, true, cfg);
                self.focus = Focus::Option(row);
            }
            Some(Hit::Button { start: true }) => self.start(),
            Some(Hit::Button { start: false }) => self.pending = Some(Action::Exit),
            _ => {}
        }
    }

    /// The running bake, as the line the player must not miss.
    fn bake_line(&self) -> Option<String> {
        let (_, t0) = self.bake.as_ref()?;
        let s = t0.elapsed().as_secs();
        Some(format!(
            "Preparing the game data — please wait…  {}:{:02}",
            s / 60,
            s % 60
        ))
    }

    fn status_line(&self) -> String {
        if let Some(line) = self.bake_line() {
            return line;
        }
        if let Some(m) = &self.message {
            return m.clone();
        }
        if self.selected.is_none() {
            return "No game is ready to play.".into();
        }
        "Enter starts  ·  Esc quits".into()
    }

    /// This frame: a freshly composed window-sized RGBA image when
    /// anything visible changed (`None` = the uploaded one still
    /// stands), and the one quad that shows it.
    pub fn frame(
        &mut self,
        size: (f32, f32),
        cursor: (f32, f32),
        cfg: &Config,
    ) -> (Option<Vec<u8>>, Vec<UiQuad>) {
        let (w, h) = (size.0.max(1.0) as u32, size.1.max(1.0) as u32);
        let data = self.data_view();
        self.hover = match &data {
            Some(v) => data_hit(size, cursor, v),
            None => hit(size, cursor),
        };
        let key = ViewKey {
            size: (w, h),
            selected: self.selected,
            focus: self.focus,
            hover: self.hover,
            values: std::array::from_fn(|row| self.value(row, cfg)),
            games: self.status.games,
            status: self.status_line(),
            baking: self.bake.is_some(),
            data,
        };
        let quad = UiQuad {
            rect: [0.0, 0.0, w as f32, h as f32],
            uv: [0.0, 0.0, w as f32, h as f32],
            tint: [1.0; 4],
        };
        if self.last.as_ref() == Some(&key) {
            return (None, vec![quad]);
        }
        let mut c = Canvas::new(w as usize, h as usize);
        self.compose(&mut c, &key);
        self.last = Some(key);
        (Some(c.buf), vec![quad])
    }

    fn compose(&self, c: &mut Canvas, key: &ViewKey) {
        c.background();
        c.text(
            &self.font,
            "Magic Carpet",
            VW / 2.0,
            82.0,
            60.0,
            GOLD,
            Align::Center,
        );
        c.text(
            &self.font,
            "Choose your world",
            VW / 2.0,
            118.0,
            20.0,
            MUTED,
            Align::Center,
        );

        for i in 0..3 {
            self.compose_card(c, key, i);
        }
        c.text(
            &self.font,
            &key.status,
            VW / 2.0,
            408.0,
            if key.baking { 22.0 } else { 17.0 },
            if key.baking { BUSY } else { MUTED },
            Align::Center,
        );

        for (row, (label, _)) in OPTIONS.iter().enumerate() {
            let focused = key.focus == Focus::Option(row);
            let (_, y, _, hh) = value_rect(row);
            let base = y + hh / 2.0 + 7.0;
            c.text(
                &self.font,
                label,
                OPT_LABEL_RIGHT,
                base,
                21.0,
                if focused { GOLD } else { TEXT },
                Align::Right,
            );
            // Bare arrows on the background; the whole rect clicks.
            for right in [false, true] {
                let r = arrow_rect(row, right);
                let hot = key.hover == Some(Hit::Arrow { row, right });
                c.arrow(
                    r,
                    right,
                    if hot {
                        HOVER
                    } else if focused {
                        GOLD
                    } else {
                        TEXT
                    },
                );
            }
            let r = value_rect(row);
            c.fill(r, PANEL);
            let edge = if focused {
                GOLD
            } else if key.hover == Some(Hit::Value(row)) {
                HOVER
            } else {
                EDGE
            };
            c.stroke(r, if focused { 2.0 } else { 1.5 }, edge);
            c.text(
                &self.font,
                &key.values[row],
                r.0 + r.2 / 2.0,
                base,
                19.0,
                TEXT,
                Align::Center,
            );
        }

        let can_start = key.selected.is_some();
        for start in [false, true] {
            let r = button_rect(start);
            let hot = key.hover == Some(Hit::Button { start });
            let focused = key.focus == Focus::Buttons { start };
            let (fill, ink) = match (start, can_start) {
                (true, true) => ([112, 80, 28, 245], GOLD),
                (true, false) => ([40, 38, 46, 235], MUTED),
                (false, _) => (PANEL, TEXT),
            };
            c.fill(r, fill);
            let edge = if focused {
                GOLD
            } else if hot && (can_start || !start) {
                HOVER
            } else {
                EDGE
            };
            c.stroke(r, if focused { 2.5 } else { 1.5 }, edge);
            let label = if start { "Start" } else { "Exit" };
            c.text(
                &self.font,
                label,
                r.0 + r.2 / 2.0,
                r.1 + r.3 / 2.0 + 9.0,
                26.0,
                ink,
                Align::Center,
            );
        }
        let r = data_button_rect();
        let hot = key.hover == Some(Hit::DataOpen);
        c.fill(r, PANEL);
        c.stroke(r, 1.5, if hot { HOVER } else { EDGE });
        c.text(
            &self.font,
            "Game data…",
            r.0 + r.2 / 2.0,
            r.1 + r.3 / 2.0 + 6.0,
            17.0,
            TEXT,
            Align::Center,
        );
        c.text(
            &self.font,
            concat!("mgcarpet ", env!("CARGO_PKG_VERSION")),
            VW - 12.0,
            VH - 12.0,
            13.0,
            MUTED,
            Align::Right,
        );
        if let Some(view) = &key.data {
            self.compose_data(c, key, view);
        }
    }

    /// The dialog body laid out: (line, baseline, size, ink).
    fn data_lines(&self, view: &DataView) -> Vec<(String, f32, f32, Ink)> {
        let lead = 24.0;
        let mut base = DATA.1 + 100.0;
        let mut out = Vec::new();
        for (ink, para) in &view.lines {
            if para.is_empty() {
                base += lead / 2.0;
                continue;
            }
            let (size, hard) = match ink {
                Ink::Head => (20.0, false),
                Ink::Busy => (21.0, false),
                Ink::Path => (15.0, true),
                _ => (17.0, false),
            };
            for line in wrap(&self.font, para, size, DATA.2 - 70.0, hard) {
                out.push((line, base, size, *ink));
                base += lead;
            }
        }
        out
    }

    /// The kind cards: a title, the plain-words description, a
    /// "Choose…" button (the whole card clicks).
    fn compose_choices(&self, c: &mut Canvas, key: &ViewKey, choices: &[ChoiceView]) {
        for (i, ch) in choices.iter().enumerate() {
            let r = choice_rect(i);
            let hot = key.hover == Some(Hit::Choice(i));
            c.fill(r, if hot { [44, 40, 64, 255] } else { PANEL });
            c.stroke(
                r,
                if hot { 2.0 } else { 1.5 },
                if hot { HOVER } else { EDGE },
            );
            let text_w = r.2 - 220.0;
            c.text(
                &self.font,
                ch.title,
                r.0 + 18.0,
                r.1 + 30.0,
                20.0,
                GOLD,
                Align::Left,
            );
            let (body, ink, size) = match ch.warning {
                Some(w) => (w, [236, 120, 110, 255], 14.0),
                None => (ch.description, TEXT, 16.0),
            };
            for (n, line) in wrap(&self.font, body, size, text_w, false)
                .iter()
                .take(4)
                .enumerate()
            {
                c.text(
                    &self.font,
                    line,
                    r.0 + 18.0,
                    r.1 + 56.0 + n as f32 * 20.0,
                    size,
                    ink,
                    Align::Left,
                );
            }
            let b = choice_button_rect(i);
            c.fill(b, [112, 80, 28, 245]);
            c.stroke(b, 2.0, if hot { HOVER } else { GOLD });
            c.text(
                &self.font,
                "Choose…",
                b.0 + b.2 / 2.0,
                b.1 + b.3 / 2.0 + 8.0,
                22.0,
                GOLD,
                Align::Center,
            );
        }
    }

    /// The browser: the folder's path, the places, the listing.
    fn compose_browser(&self, c: &mut Canvas, key: &ViewKey, b: &BrowserView) {
        let path = elide_left(&self.font, &b.path, 15.0, DATA.2 - 70.0);
        c.text(
            &self.font,
            &path,
            VW / 2.0,
            162.0,
            15.0,
            GOLD,
            Align::Center,
        );
        for (i, label) in b.places.iter().enumerate() {
            let r = place_rect(i);
            let hot = key.hover == Some(Hit::Place(i));
            c.fill(r, if hot { [52, 46, 76, 255] } else { PANEL });
            let label = elide_left(&self.font, label, 16.0, r.2 - 16.0);
            c.text(
                &self.font,
                &label,
                r.0 + 10.0,
                r.1 + 20.0,
                16.0,
                TEXT,
                Align::Left,
            );
        }
        let (lx, ly, lw, _) = row_rect(0);
        c.fill((lx, ly, lw, BROWSER_ROWS as f32 * 26.0), [20, 18, 30, 255]);
        for (i, (name, dir, data, selected)) in b.rows.iter().enumerate() {
            let r = row_rect(i);
            let hot = key.hover == Some(Hit::Row(i));
            if *selected {
                c.fill(r, [72, 58, 28, 255]);
            } else if hot {
                c.fill(r, [44, 40, 64, 255]);
            }
            let ink = if *data {
                GOLD
            } else if *dir {
                TEXT
            } else {
                [196, 220, 170, 255]
            };
            let shown = if *data {
                format!("{name}   (game data)")
            } else {
                name.clone()
            };
            let shown = elide_left(&self.font, &shown, 16.0, r.2 - 20.0);
            c.text(
                &self.font,
                &shown,
                r.0 + 10.0,
                r.1 + 19.0,
                16.0,
                ink,
                Align::Left,
            );
        }
        let foot = 180.0 + BROWSER_ROWS as f32 * 26.0 + 24.0;
        if let Some(h) = &b.here {
            c.text(
                &self.font,
                &format!("This folder: {h}"),
                lx + lw,
                foot,
                15.0,
                GOLD,
                Align::Right,
            );
            c.text(&self.font, &b.info, lx, foot, 15.0, MUTED, Align::Left);
        } else {
            c.text(
                &self.font,
                &b.info,
                lx + lw / 2.0,
                foot,
                15.0,
                MUTED,
                Align::Center,
            );
        }
    }

    /// The game-data dialog over a dimmed launcher.
    fn compose_data(&self, c: &mut Canvas, key: &ViewKey, view: &DataView) {
        c.dim(170);
        c.fill(DATA, [30, 26, 46, 252]);
        c.stroke(DATA, 2.0, GOLD);
        c.text(
            &self.font,
            view.title,
            VW / 2.0,
            DATA.1 + 56.0,
            32.0,
            GOLD,
            Align::Center,
        );
        if let Some(b) = &view.browser {
            self.compose_browser(c, key, b);
        }
        if let Some(choices) = &view.choices {
            self.compose_choices(c, key, choices);
        }
        for (line, base, size, ink) in self.data_lines(view) {
            let color = match ink {
                Ink::Head | Ink::Path => GOLD,
                Ink::Text => TEXT,
                Ink::Muted => MUTED,
                Ink::Error => [236, 120, 110, 255],
                Ink::Busy => BUSY,
            };
            c.text(
                &self.font,
                &line,
                VW / 2.0,
                base,
                size,
                color,
                Align::Center,
            );
        }
        if let Some(f) = view.progress {
            c.fill(DATA_BAR, [20, 18, 30, 255]);
            let (x, y, w, h) = DATA_BAR;
            c.fill((x, y, w * f.clamp(0.0, 1.0), h), [112, 80, 28, 245]);
            c.stroke(DATA_BAR, 1.5, EDGE);
            c.text(
                &self.font,
                &format!("{:.0}%", f * 100.0),
                VW / 2.0,
                y + h + 26.0,
                16.0,
                MUTED,
                Align::Center,
            );
        }
        for (i, label) in view.buttons.iter().enumerate() {
            let Some(label) = label else { continue };
            let r = data_btn_rect(i);
            let primary = view.primary == Some(i);
            c.fill(r, if primary { [112, 80, 28, 245] } else { PANEL });
            let hot = key.hover == Some(Hit::Data(i));
            c.stroke(
                r,
                if primary { 2.5 } else { 1.5 },
                if hot {
                    HOVER
                } else if primary {
                    GOLD
                } else {
                    EDGE
                },
            );
            c.text(
                &self.font,
                label,
                r.0 + r.2 / 2.0,
                r.1 + r.3 / 2.0 + 8.0,
                if label.chars().count() > 12 {
                    19.0
                } else {
                    24.0
                },
                if primary { GOLD } else { TEXT },
                Align::Center,
            );
        }
    }

    fn compose_card(&self, c: &mut Canvas, key: &ViewKey, i: usize) {
        let (x, y, w, h) = card_rect(i);
        let art = (x, y, w, ART_H);
        let ready = key.games[i];
        let selected = key.selected == Some(i);
        let hot = ready && key.hover == Some(Hit::Card(i));
        c.fill((x, y, w, h), PANEL);
        match (&self.art[i], ready) {
            (Some(a), true) => c.image(art, a),
            _ => {
                c.fill(art, [20, 18, 30, 255]);
                c.text(
                    &self.font,
                    "Not available",
                    x + w / 2.0,
                    y + ART_H / 2.0 + 7.0,
                    20.0,
                    MUTED,
                    Align::Center,
                );
            }
        }
        if !ready {
            let inset = 18.0;
            c.line(
                (x + inset, y + inset),
                (x + w - inset, y + ART_H - inset),
                5.0,
                CROSS,
            );
            c.line(
                (x + w - inset, y + inset),
                (x + inset, y + ART_H - inset),
                5.0,
                CROSS,
            );
        }
        let name_ink = if !ready {
            MUTED
        } else if selected {
            GOLD
        } else {
            TEXT
        };
        match GAMES[i].1.split_once(": ") {
            Some((title, sub)) => {
                c.text(
                    &self.font,
                    title,
                    x + w / 2.0,
                    y + ART_H + 24.0,
                    22.0,
                    name_ink,
                    Align::Center,
                );
                c.text(
                    &self.font,
                    sub,
                    x + w / 2.0,
                    y + ART_H + 45.0,
                    16.0,
                    name_ink,
                    Align::Center,
                );
            }
            None => {
                let base = y + ART_H + CAPTION_H / 2.0 + 8.0;
                c.text(
                    &self.font,
                    GAMES[i].1,
                    x + w / 2.0,
                    base,
                    22.0,
                    name_ink,
                    Align::Center,
                );
            }
        }
        let (edge, t) = if selected && key.focus == Focus::Cards {
            (GOLD, 4.0)
        } else if selected {
            (GOLD, 3.0)
        } else if hot {
            (HOVER, 2.0)
        } else {
            (EDGE, 1.5)
        };
        c.stroke((x, y, w, h), t, edge);
    }
}

/// What lies under a window position.
fn hit(size: (f32, f32), cursor: (f32, f32)) -> Option<Hit> {
    let (s, ox, oy) = fit(size.0, size.1);
    let (vx, vy) = ((cursor.0 - ox) / s, (cursor.1 - oy) / s);
    let inside = |r: Rect| vx >= r.0 && vx < r.0 + r.2 && vy >= r.1 && vy < r.1 + r.3;
    for i in 0..3 {
        if inside(card_rect(i)) {
            return Some(Hit::Card(i));
        }
    }
    for row in 0..3 {
        for right in [false, true] {
            if inside(arrow_rect(row, right)) {
                return Some(Hit::Arrow { row, right });
            }
        }
        if inside(value_rect(row)) {
            return Some(Hit::Value(row));
        }
    }
    for start in [false, true] {
        if inside(button_rect(start)) {
            return Some(Hit::Button { start });
        }
    }
    if inside(data_button_rect()) {
        return Some(Hit::DataOpen);
    }
    None
}

/// What of the dialog is under the pointer: a shown button, or a
/// browser place / row.
fn data_hit(size: (f32, f32), cursor: (f32, f32), view: &DataView) -> Option<Hit> {
    let (s, ox, oy) = fit(size.0, size.1);
    let (vx, vy) = ((cursor.0 - ox) / s, (cursor.1 - oy) / s);
    let inside = |r: Rect| vx >= r.0 && vx < r.0 + r.2 && vy >= r.1 && vy < r.1 + r.3;
    if let Some(i) = (0..3).find(|&i| view.buttons[i].is_some() && inside(data_btn_rect(i))) {
        return Some(Hit::Data(i));
    }
    if let Some(choices) = &view.choices {
        return (0..choices.len())
            .find(|&i| inside(choice_rect(i)))
            .map(Hit::Choice);
    }
    let b = view.browser.as_ref()?;
    if let Some(i) = (0..b.places.len()).find(|&i| inside(place_rect(i))) {
        return Some(Hit::Place(i));
    }
    (0..b.rows.len())
        .find(|&i| inside(row_rect(i)))
        .map(Hit::Row)
}

/// `s` shortened from the LEFT with "…" to fit `max` virtual units (a
/// path keeps its tail, the part that says where you are).
fn elide_left(font: &FontRef, s: &str, size: f32, max: f32) -> String {
    if text_width(font, s, size) <= max {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    for start in 1..chars.len() {
        let t: String = std::iter::once('…')
            .chain(chars[start..].iter().copied())
            .collect();
        if text_width(font, &t, size) <= max {
            return t;
        }
    }
    "…".into()
}

/// A path as the player should read it: absolute when it can be.
fn absolute(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string()
}

/// A string's advance width in virtual units at `size`.
fn text_width(font: &FontRef, s: &str, size: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            width += scaled.kern(p, id);
        }
        width += scaled.h_advance(id);
        prev = Some(id);
    }
    width
}

/// Greedy word wrap to `max` virtual units. `hard` (a path) breaks
/// anywhere, by character, since it has no spaces worth keeping.
fn wrap(font: &FontRef, s: &str, size: f32, max: f32, hard: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    if hard {
        for ch in s.chars() {
            cur.push(ch);
            if text_width(font, &cur, size) > max {
                cur.pop();
                lines.push(std::mem::take(&mut cur));
                cur.push(ch);
            }
        }
    } else {
        for word in s.split_whitespace() {
            let next = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if !cur.is_empty() && text_width(font, &next, size) > max {
                lines.push(std::mem::replace(&mut cur, word.to_string()));
            } else {
                cur = next;
            }
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// The virtual canvas's scale and offset in a window.
fn fit(w: f32, h: f32) -> (f32, f32, f32) {
    let s = (w / VW).min(h / VH).max(0.01);
    (s, (w - VW * s) / 2.0, (h - VH * s) / 2.0)
}

/// A card's art from the bake: the finished title card of the game's
/// title movie — the one that animates the game's name after the
/// story intro (MC1 `TITLE-01`, Hidden Worlds' own `TITLE-03`, MC2
/// `INTRO2`). Its last SHOWN frame: retail's player stops at
/// `frame_count − 1` (see `movie.rs`); the titles fade in and hold, so
/// that frame is the whole title at full brightness.
fn load_art(baked_root: &Path, game: CampaignId) -> Result<Art, String> {
    let (dir, name) = match game {
        CampaignId::Mc1 => ("mc1-movies", "title-01"),
        CampaignId::Mc1Hw => ("mc1-movies", "title-03"),
        CampaignId::Mc2 => ("mc2-movies", "intro2"),
    };
    let dir = baked_root.join("assets").join(dir);
    let index: mgc_formats::bundle::MovieIndex = std::fs::read(dir.join("movies.json"))
        .map_err(|e| format!("movies.json: {e}"))
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| format!("movies.json: {e}")))?;
    let entry = index
        .movies
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("no {name} movie baked"))?;
    let raw = std::fs::read(dir.join(&entry.file)).map_err(|e| format!("{}: {e}", entry.file))?;
    let mut cur = mgc_import::fmv::FmvCursor::new(raw, None)?;
    let last = cur.frame_count().saturating_sub(1);
    while cur.played() < last && cur.advance()? {}
    let pal = cur.palette().ok_or_else(|| format!("{name}: no palette"))?;
    // VGA DAC palette: 6 bits per channel.
    let rgb = cur
        .canvas()
        .iter()
        .flat_map(|&i| {
            let e = i as usize * 3;
            [pal[e], pal[e + 1], pal[e + 2]].map(|c| (c & 0x3F) << 2 | (c & 0x3F) >> 4)
        })
        .collect();
    Ok(Art {
        w: cur.width(),
        h: cur.height(),
        rgb,
    })
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

/// The window-sized RGBA frame and the virtual → pixel mapping.
struct Canvas {
    w: usize,
    h: usize,
    s: f32,
    ox: f32,
    oy: f32,
    buf: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        let (s, ox, oy) = fit(w as f32, h as f32);
        Canvas {
            w,
            h,
            s,
            ox,
            oy,
            buf: vec![0; w * h * 4],
        }
    }

    fn px(&self, vx: f32, vy: f32) -> (f32, f32) {
        (self.ox + vx * self.s, self.oy + vy * self.s)
    }

    /// Blend `c` at `alpha` (0..=1 on top of c's own alpha) into pixel (x, y).
    fn blend(&mut self, x: i64, y: i64, c: Rgba, alpha: f32) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let a = alpha * c[3] as f32 / 255.0;
        if a <= 0.0 {
            return;
        }
        let o = (y as usize * self.w + x as usize) * 4;
        for k in 0..3 {
            let d = self.buf[o + k] as f32;
            self.buf[o + k] = (d + (c[k] as f32 - d) * a).round() as u8;
        }
        self.buf[o + 3] = 255;
    }

    /// Darken the whole window (behind a modal panel).
    fn dim(&mut self, alpha: u8) {
        for px in self.buf.chunks_exact_mut(4) {
            for k in 0..3 {
                px[k] = (px[k] as u32 * (255 - alpha as u32) / 255) as u8;
            }
        }
    }

    /// The whole window: a dusk gradient behind the layout.
    fn background(&mut self) {
        let (top, bottom) = ([26, 18, 46], [6, 6, 14]);
        for y in 0..self.h {
            let t = y as f32 / self.h.max(1) as f32;
            let c: [u8; 3] = std::array::from_fn(|k| {
                (top[k] as f32 + (bottom[k] as f32 - top[k] as f32) * t) as u8
            });
            for x in 0..self.w {
                let o = (y * self.w + x) * 4;
                self.buf[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
    }

    /// A virtual rect's covered pixel span, with fractional edge
    /// coverage for anti-aliasing.
    fn fill(&mut self, r: Rect, c: Rgba) {
        let (x0, y0) = self.px(r.0, r.1);
        let (x1, y1) = self.px(r.0 + r.2, r.1 + r.3);
        for y in y0.floor() as i64..y1.ceil() as i64 {
            let cy = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).clamp(0.0, 1.0);
            for x in x0.floor() as i64..x1.ceil() as i64 {
                let cx = (x1.min(x as f32 + 1.0) - x0.max(x as f32)).clamp(0.0, 1.0);
                self.blend(x, y, c, cx * cy);
            }
        }
    }

    /// A border `t` virtual units thick, inside the rect.
    fn stroke(&mut self, r: Rect, t: f32, c: Rgba) {
        let (x, y, w, h) = r;
        self.fill((x, y, w, t), c);
        self.fill((x, y + h - t, w, t), c);
        self.fill((x, y + t, t, h - 2.0 * t), c);
        self.fill((x + w - t, y + t, t, h - 2.0 * t), c);
    }

    /// An anti-aliased segment `t` virtual units wide.
    fn line(&mut self, a: (f32, f32), b: (f32, f32), t: f32, c: Rgba) {
        let (ax, ay) = self.px(a.0, a.1);
        let (bx, by) = self.px(b.0, b.1);
        let half = t * self.s / 2.0;
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let (lo_x, hi_x) = (ax.min(bx) - half - 1.0, ax.max(bx) + half + 1.0);
        let (lo_y, hi_y) = (ay.min(by) - half - 1.0, ay.max(by) + half + 1.0);
        for y in lo_y.floor() as i64..hi_y.ceil() as i64 {
            for x in lo_x.floor() as i64..hi_x.ceil() as i64 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let u = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (ax + u * dx - px, ay + u * dy - py);
                let d = (qx * qx + qy * qy).sqrt();
                self.blend(x, y, c, (half + 0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// A selector arrow (a filled triangle) centred in `r`, pointing
    /// right or left, anti-aliased by the distance to its nearest edge.
    fn arrow(&mut self, r: Rect, right: bool, c: Rgba) {
        let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
        let (hw, hh) = (7.0, 11.0);
        let (tip, back) = if right {
            (cx + hw, cx - hw)
        } else {
            (cx - hw, cx + hw)
        };
        let tri = [
            self.px(tip, cy),
            self.px(back, cy - hh),
            self.px(back, cy + hh),
        ];
        // Signed distance to the edge a→b, positive on the side the
        // triangle's third vertex lies on (so the winding is moot).
        let inside = |a: (f32, f32), b: (f32, f32), o: (f32, f32), p: (f32, f32)| {
            let (ex, ey) = (b.0 - a.0, b.1 - a.1);
            let len = (ex * ex + ey * ey).sqrt().max(1e-6);
            let side = |q: (f32, f32)| ((q.0 - a.0) * ey - (q.1 - a.1) * ex) / len;
            side(p) * side(o).signum()
        };
        let lo_x = tri.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i64;
        let hi_x = tri.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i64;
        let lo_y = tri.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i64;
        let hi_y = tri.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i64;
        for y in lo_y..=hi_y {
            for x in lo_x..=hi_x {
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let d = inside(tri[0], tri[1], tri[2], p)
                    .min(inside(tri[1], tri[2], tri[0], p))
                    .min(inside(tri[2], tri[0], tri[1], p));
                self.blend(x, y, c, (d + 0.5).clamp(0.0, 1.0));
            }
        }
    }

    /// Cover-fit `a` into `r` (cropped centred), bilinear.
    fn image(&mut self, r: Rect, a: &Art) {
        let (x0, y0) = self.px(r.0, r.1);
        let (x1, y1) = self.px(r.0 + r.2, r.1 + r.3);
        let (dw, dh) = (x1 - x0, y1 - y0);
        let k = (a.w as f32 / dw).min(a.h as f32 / dh);
        let (sx0, sy0) = ((a.w as f32 - dw * k) / 2.0, (a.h as f32 - dh * k) / 2.0);
        let texel = |x: usize, y: usize, ch: usize| a.rgb[(y * a.w + x) * 3 + ch] as f32;
        for y in y0.round() as i64..y1.round() as i64 {
            for x in x0.round() as i64..x1.round() as i64 {
                let u = (sx0 + (x as f32 + 0.5 - x0) * k - 0.5).clamp(0.0, a.w as f32 - 1.0);
                let v = (sy0 + (y as f32 + 0.5 - y0) * k - 0.5).clamp(0.0, a.h as f32 - 1.0);
                let (ui, vi) = (u as usize, v as usize);
                let (u1, v1) = ((ui + 1).min(a.w - 1), (vi + 1).min(a.h - 1));
                let (fu, fv) = (u - ui as f32, v - vi as f32);
                let c: [u8; 3] = std::array::from_fn(|ch| {
                    let top = texel(ui, vi, ch) * (1.0 - fu) + texel(u1, vi, ch) * fu;
                    let bot = texel(ui, v1, ch) * (1.0 - fu) + texel(u1, v1, ch) * fu;
                    (top * (1.0 - fv) + bot * fv).round() as u8
                });
                self.blend(x, y, [c[0], c[1], c[2], 255], 1.0);
            }
        }
    }

    /// Text on a baseline at virtual (x, base), `size` virtual px tall.
    fn text(
        &mut self,
        font: &FontRef,
        s: &str,
        x: f32,
        base: f32,
        size: f32,
        c: Rgba,
        align: Align,
    ) {
        let scaled = font.as_scaled(PxScale::from(size * self.s));
        let mut width = 0.0;
        let mut prev = None;
        for ch in s.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(p) = prev {
                width += scaled.kern(p, id);
            }
            width += scaled.h_advance(id);
            prev = Some(id);
        }
        let (px, py) = self.px(x, base);
        let mut pen = match align {
            Align::Left => px,
            Align::Center => px - width / 2.0,
            Align::Right => px - width,
        };
        let mut prev = None;
        for ch in s.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(p) = prev {
                pen += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scaled.scale(), ab_glyph::point(pen, py));
            if let Some(g) = font.outline_glyph(glyph) {
                let b = g.px_bounds();
                g.draw(|gx, gy, cov| {
                    self.blend(
                        b.min.x as i64 + gx as i64,
                        b.min.y as i64 + gy as i64,
                        c,
                        cov,
                    );
                });
            }
            pen += scaled.h_advance(id);
            prev = Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_fits_the_canvas_and_nothing_overlaps() {
        let mut rects: Vec<Rect> = (0..3).map(card_rect).collect();
        for row in 0..3 {
            rects.extend([
                arrow_rect(row, false),
                value_rect(row),
                arrow_rect(row, true),
            ]);
        }
        rects.extend([button_rect(false), button_rect(true), data_button_rect()]);
        for (i, a) in rects.iter().enumerate() {
            assert!(
                a.0 >= 0.0 && a.1 >= 0.0 && a.0 + a.2 <= VW && a.1 + a.3 <= VH,
                "{a:?}"
            );
            for b in &rects[i + 1..] {
                let apart =
                    a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    /// A landed import asks for a bake, and the launcher starts one
    /// even though no stamp is stale (a new game is not "stale"). Here
    /// the data root is empty, so the bake fails at once and says so.
    #[test]
    fn an_import_starts_a_bake() {
        let root = std::env::temp_dir().join(format!("mgc-launcher-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("gd")).unwrap();
        let cfg = Config {
            gamedata: Some(root.join("gd")),
            ..Default::default()
        };
        let mut l = Launcher::new(&root.join("baked"), &cfg);
        let settle = |l: &mut Launcher| {
            for _ in 0..400 {
                l.tick();
                if l.bake.is_none() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("the empty bake never finished");
        };
        // The folder exists, so the first-run bake runs (and fails);
        // with nothing ready, the dialog then opens by itself.
        settle(&mut l);
        assert!(l.data.is_some(), "nothing ready: the dialog opened");
        l.message = None;
        l.data.as_mut().unwrap().bake_requested = true;
        l.tick();
        assert!(l.bake.is_some(), "the request started a bake");
        // While it runs, the dialog itself says so (the dimmed status
        // line behind it is easy to miss), and the status line is the
        // bake's. (The empty bake may already be done: check only if
        // it is still running.)
        if l.bake.is_some() {
            let view = l.data_view().unwrap();
            assert!(
                view.lines
                    .iter()
                    .any(|(i, t)| *i == Ink::Busy && t.contains("please wait"))
            );
            assert!(l.status_line().contains("please wait"));
        }
        settle(&mut l);
        assert!(
            l.message.as_deref().is_some_and(|m| m.contains("failed")),
            "{:?}",
            l.message
        );
        assert!(
            !l.data.as_ref().unwrap().bake_requested,
            "one request, one bake"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The browser through the launcher's own input: a row click
    /// enters a folder or picks a file (→ identifying), the wheel
    /// scrolls a long listing, Backspace goes up, arrows select.
    #[test]
    fn the_browser_answers_clicks_keys_and_the_wheel() {
        use winit::keyboard::{Key, NamedKey};
        let t = std::env::temp_dir().join(format!("mgc-launcher-br-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("gd")).unwrap();
        for i in 0..30 {
            std::fs::create_dir_all(t.join(format!("dir{i:02}"))).unwrap();
        }
        std::fs::write(t.join("dir00/setup.exe"), b"not really").unwrap();
        let mut cfg = Config {
            gamedata: Some(t.join("gd")),
            ..Default::default()
        };
        let mut l = Launcher::new(&t.join("baked"), &cfg);
        for _ in 0..400 {
            l.tick();
            if l.bake.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        l.open_data();
        let at = |r: Rect| (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
        let size = (960.0, 720.0);
        l.click(size, at(data_btn_rect(0)), &mut cfg);
        l.click(size, at(choice_rect(0)), &mut cfg); // the installer
        if let Some(d) = &mut l.data
            && let Step::Browsing(b) = &mut d.step
        {
            b.enter(t.clone());
        }
        let view = l.data_view().unwrap();
        let b = view.browser.as_ref().expect("browsing");
        assert_eq!(
            b.rows.len(),
            BROWSER_ROWS,
            "a long listing fills the window"
        );
        assert!(b.info.contains("of 31"), "{}", b.info);
        assert_eq!(data_hit(size, at(row_rect(3)), &view), Some(Hit::Row(3)));
        assert_eq!(
            data_hit(size, at(place_rect(0)), &view),
            Some(Hit::Place(0))
        );
        l.scroll(1.0);
        assert!(
            l.data_view()
                .unwrap()
                .browser
                .unwrap()
                .info
                .starts_with("4–")
        );
        l.scroll(-10.0);
        l.click(size, at(row_rect(0)), &mut cfg);
        let view = l.data_view().unwrap();
        assert!(
            view.browser.unwrap().path.ends_with("dir00"),
            "a folder click enters it"
        );
        l.key(&Key::Named(NamedKey::ArrowDown), &mut cfg);
        l.key(&Key::Named(NamedKey::Backspace), &mut cfg);
        assert!(
            l.data_view().unwrap().browser.unwrap().rows[0].3,
            "Backspace goes up, the folder we left selected"
        );
        l.click(size, at(row_rect(0)), &mut cfg);
        l.click(size, at(row_rect(0)), &mut cfg);
        assert!(
            matches!(l.data.as_ref().unwrap().step, Step::Identifying(_)),
            "a file click is the pick"
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    /// Every kind card's text — its description, or the innoextract
    /// warning — fits the four lines a card has room for.
    #[test]
    fn the_kind_cards_text_fits_its_card() {
        let font = FontRef::try_from_slice(FONT).unwrap();
        let w = choice_rect(0).2 - 220.0;
        for want in Want::ALL {
            let n = wrap(&font, want.description(), 16.0, w, false).len();
            assert!(n <= 4, "{want:?}: {n} lines");
        }
        let root = std::env::temp_dir().join(format!("mgc-launcher-k-{}", std::process::id()));
        let cfg = Config {
            gamedata: Some(root.join("gd")),
            ..Default::default()
        };
        let mut l = Launcher::new(&root, &cfg);
        l.open_data();
        if let Some(d) = &mut l.data {
            d.innoextract = false;
            d.choose();
        }
        let view = l.data_view().unwrap();
        let warning = view.choices.unwrap()[0]
            .warning
            .expect("the innoextract warning");
        let n = wrap(&font, warning, 14.0, w, false).len();
        assert!(n <= 4, "the warning wraps to {n} lines");
    }

    /// The game-data dialog's parts sit inside its panel, apart.
    #[test]
    fn the_data_dialog_layout_fits_its_panel() {
        let within = |r: Rect, o: Rect| {
            r.0 >= o.0 && r.1 >= o.1 && r.0 + r.2 <= o.0 + o.2 && r.1 + r.3 <= o.1 + o.3
        };
        assert!(within(DATA, (0.0, 0.0, VW, VH)));
        let parts: Vec<Rect> = (0..3).map(data_btn_rect).chain([DATA_BAR]).collect();
        // The browser: places and rows inside the panel, above the
        // buttons, apart from each other.
        let foot = data_btn_rect(0).1;
        let browser: Vec<Rect> = (0..PLACE_ROWS)
            .map(place_rect)
            .chain((0..BROWSER_ROWS).map(row_rect))
            .collect();
        for r in &browser {
            assert!(within(*r, DATA) && r.1 + r.3 <= foot - 30.0, "{r:?}");
        }
        for p in (0..PLACE_ROWS).map(place_rect) {
            assert!(p.0 + p.2 <= row_rect(0).0, "places left of the list");
        }
        // The kind cards: inside, above the buttons, apart; each card's
        // button inside its card.
        for i in 0..Want::ALL.len() {
            let r = choice_rect(i);
            assert!(within(r, DATA) && r.1 + r.3 <= foot, "{r:?}");
            assert!(within(choice_button_rect(i), r));
            if i > 0 {
                let p = choice_rect(i - 1);
                assert!(p.1 + p.3 <= r.1, "cards {} and {i} overlap", i - 1);
            }
        }
        for (i, a) in parts.iter().enumerate() {
            assert!(within(*a, DATA), "{a:?}");
            for b in &parts[i + 1..] {
                let apart =
                    a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn the_selector_arrows_paint_their_centre() {
        for right in [false, true] {
            let mut c = Canvas::new(960, 720);
            let r = arrow_rect(0, right);
            c.arrow(r, right, [255, 255, 255, 255]);
            let (x, y) = ((r.0 + r.2 / 2.0) as usize, (r.1 + r.3 / 2.0) as usize);
            assert_eq!(
                c.buf[(y * 960 + x) * 4],
                255,
                "right={right}: centre painted"
            );
            assert_eq!(
                c.buf[(r.1 as usize * 960 + r.0 as usize) * 4],
                0,
                "corner left bare"
            );
        }
    }

    #[test]
    fn hits_land_on_what_is_drawn_at_any_window_shape() {
        for size in [(960.0, 720.0), (1920.0, 1080.0), (800.0, 1200.0)] {
            let (s, ox, oy) = fit(size.0, size.1);
            let at = |r: Rect| (ox + (r.0 + r.2 / 2.0) * s, oy + (r.1 + r.3 / 2.0) * s);
            assert_eq!(hit(size, at(card_rect(2))), Some(Hit::Card(2)));
            assert_eq!(
                hit(size, at(arrow_rect(1, true))),
                Some(Hit::Arrow {
                    row: 1,
                    right: true
                })
            );
            assert_eq!(
                hit(size, at(button_rect(true))),
                Some(Hit::Button { start: true })
            );
            assert_eq!(hit(size, (ox + 2.0, oy + 2.0)), None);
        }
    }

    #[test]
    fn an_unbaked_tree_leaves_every_game_unavailable_and_unstartable() {
        let root = std::env::temp_dir().join(format!("mgc-launcher-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut cfg = Config {
            gamedata: Some(root.join("no-such-gamedata")),
            ..Default::default()
        };
        let mut l = Launcher::new(&root, &cfg);
        assert_eq!(l.status.games, [false; 3]);
        assert!(l.status.tree.is_some());
        assert!(l.bake.is_none(), "no data found, nothing to bake");
        assert!(l.status_line().starts_with("No game data found"));
        assert_eq!(l.selected, None);
        // The game-data dialog is up and modal: Esc and clicks
        // elsewhere do not reach the launcher; Close (or Esc) shuts it.
        assert!(l.data.is_some(), "no games: the data dialog opens");
        let (img, _) = l.frame((960.0, 720.0), (0.0, 0.0), &cfg);
        assert!(img.is_some());
        let view = l.data_view().expect("a view");
        assert_eq!(view.primary, Some(0), "Enter chooses a file");
        assert!(
            view.lines
                .iter()
                .any(|(_, t)| t.contains("none were found"))
        );
        l.escape();
        assert!(l.data.is_none(), "Esc closes the dialog");
        assert_eq!(l.take_action(), None, "…and does not quit");
        // "Game data…" reopens it; Exit is behind it.
        let at = |r: Rect| (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
        l.click((960.0, 720.0), at(data_button_rect()), &mut cfg);
        assert!(l.data.is_some(), "the button opens the dialog");
        l.click((960.0, 720.0), at(button_rect(false)), &mut cfg);
        assert_eq!(l.take_action(), None, "Exit is behind the dialog");
        // "Add game data…" asks what the player has: one card per
        // kind, each opening the browser set up for it; Back walks out
        // one level at a time.
        l.click((960.0, 720.0), at(data_btn_rect(0)), &mut cfg);
        let view = l.data_view().expect("a view");
        let cards = view.choices.as_ref().expect("the kind cards");
        assert_eq!(cards.len(), 3);
        assert!(
            cards[1].description.contains("game.gog") && cards[1].description.contains("CARPET.CD")
        );
        assert_eq!(view.buttons, [None, None, Some("Back")]);
        l.click((960.0, 720.0), at(choice_button_rect(1)), &mut cfg);
        let view = l.data_view().expect("a view");
        assert!(view.browser.is_some(), "a card opens the browser");
        assert_eq!(view.title, Want::Installed.title());
        assert_eq!(view.buttons[0], Some("Up one level"));
        l.click((960.0, 720.0), at(data_btn_rect(2)), &mut cfg);
        assert!(
            l.data_view().unwrap().choices.is_some(),
            "Back: the cards again"
        );
        l.escape();
        assert!(
            l.data_view().expect("a view").choices.is_none(),
            "Esc: the front page"
        );
        l.click((960.0, 720.0), at(data_btn_rect(2)), &mut cfg);
        assert!(l.data.is_none(), "Close shuts it");
        // An unavailable card opens it too.
        l.click((960.0, 720.0), at(card_rect(2)), &mut cfg);
        assert!(l.data.is_some(), "an unavailable card opens the dialog");
        // The longest body (an error outcome, a deep data path) still
        // clears the buttons.
        if let Some(d) = &mut l.data {
            d.root = PathBuf::from("/very/deep/".repeat(12));
            d.outcome = Some((true, "x ".repeat(120)));
        }
        let view = l.data_view().expect("a view");
        let last = l.data_lines(&view).last().expect("a body").1;
        assert!(
            last + 12.0 < data_btn_rect(0).1,
            "text bottom {last} runs into the buttons at {}",
            data_btn_rect(0).1
        );
        l.escape();
        assert!(l.data.is_none());
        l.key(
            &winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter),
            &mut cfg,
        );
        assert_eq!(l.take_action(), None, "nothing to start");
        l.escape();
        assert_eq!(l.take_action(), Some(Action::Exit));
        let (img, quads) = l.frame((320.0, 240.0), (0.0, 0.0), &cfg);
        assert_eq!(img.map(|b| b.len()), Some(320 * 240 * 4));
        assert_eq!(quads.len(), 1);
        assert!(
            l.frame((320.0, 240.0), (0.0, 0.0), &cfg).0.is_none(),
            "unchanged: no re-compose"
        );
    }

    /// Row 0 is Display: fullscreen, then every window size, the ones
    /// larger than the monitor tagged rather than hidden.
    #[test]
    fn the_display_row_offers_every_size() {
        let root = std::env::temp_dir().join(format!("mgc-launcher-d-{}", std::process::id()));
        let mut cfg = Config {
            gamedata: Some(root.join("no-such-gamedata")),
            ..Default::default()
        };
        let mut l = Launcher::new(&root, &cfg);
        l.set_monitor(Some((1920, 1080)));
        assert_eq!(l.value(0, &cfg), "Fullscreen");
        l.step_option(0, true, &mut cfg);
        assert_eq!(l.value(0, &cfg), "Window 640 × 480");
        assert!(!cfg.render.preference.fullscreen);
        l.step_option(0, true, &mut cfg);
        assert_eq!(l.value(0, &cfg), "Window 1280 × 960");
        l.step_option(0, true, &mut cfg);
        assert_eq!(
            l.value(0, &cfg),
            "Window 1600 × 1200 (larger than screen)",
            "1600 × 1200 does not fit 1080 rows"
        );
        l.step_option(0, false, &mut cfg);
        l.step_option(0, false, &mut cfg);
        l.step_option(0, false, &mut cfg);
        assert_eq!(l.value(0, &cfg), "Fullscreen");
        l.step_option(0, false, &mut cfg);
        assert_eq!(l.value(0, &cfg), "Window 3840 × 2160 (larger than screen)");
        assert_eq!(cfg.render.preference.window_size, WindowSize::W3840x2160);
        l.set_monitor(Some((3840, 2160)));
        assert_eq!(l.value(0, &cfg), "Window 3840 × 2160");
        assert!(l.take_changed().contains(&"render.preference.window_size"));
    }

    /// Row 1 is Controls: stepping writes the preset, the value reads
    /// back from the config, and a group that was hand-tuned when the
    /// launcher opened can step back to exactly those values.
    #[test]
    fn the_controls_row_sets_presets_and_restores_custom() {
        let root = std::env::temp_dir().join(format!("mgc-launcher-p-{}", std::process::id()));
        let mut cfg = Config {
            gamedata: Some(root.join("no-such-gamedata")),
            ..Default::default()
        };
        let mut l = Launcher::new(&root, &cfg);
        assert_eq!(l.value(1, &cfg), "Enhanced");
        l.step_option(1, true, &mut cfg);
        assert_eq!(l.value(1, &cfg), "Classic");
        assert_eq!(
            cfg.controls.models.thrust,
            crate::config::ThrustModel::Classic
        );
        l.step_option(1, true, &mut cfg);
        assert_eq!(
            l.value(1, &cfg),
            "Enhanced",
            "no Custom to step to: it was not tuned"
        );
        assert_eq!(l.take_changed(), vec!["controls.preset", "controls.preset"]);

        cfg.controls.preferences.mouse_sensitivity_x = 0.8;
        let mut l = Launcher::new(&root, &cfg);
        assert_eq!(l.value(1, &cfg), "Custom");
        l.step_option(1, true, &mut cfg);
        assert_eq!(l.value(1, &cfg), "Enhanced");
        l.step_option(1, false, &mut cfg);
        assert_eq!(l.value(1, &cfg), "Custom");
        assert!((cfg.controls.preferences.mouse_sensitivity_x - 0.8).abs() < 1e-6);
    }
}
