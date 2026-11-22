//! The MC2 campaign world-map screen — the between-levels hub
//! (retail `NewGameDialog`/`DrawAnimTextsAndPlaySounds_7D400`,
//! MenusAndIntros.cpp; docs/traces/mc2-campaign-save-menu.md): the
//! 1280×960 scrolling map with animated portals, ambient set
//! dressing, and the travelling carpet.
//!
//! Retail law, scaled 2×:
//! - completed main = planted flag, frames 37-43; the NEXT portal
//!   pops into existence once per session (sound 41, frames 70-83,
//!   MI:2797-2823) then idles as the open portal 33-35; later
//!   portals are not drawn at all (the draw loop breaks on the
//!   first hidden one, MI:2825).
//! - secret revealed = the same pop-in then 270-272; completed
//!   secret = 305-311 (MI:2828-2879).
//! - the carpet (8 heading families of 4 frames, sprites 1-32)
//!   flies portal-to-portal at ~6 px/frame (3× Bresenham steps of
//!   2, `sub_80D40`/`MoveAnimObject_7E9D0`), stamping trail dot
//!   sprite 139 into the map every >8 px (`DrawMapObject_812D0`) —
//!   the dotted route line. Travel sound 19. Its heading family is
//!   `sub_581E0_maybe_tan2(target, carpet)` — i.e. retail's `tan`
//!   of (carpet − target): 0 = target BELOW, 512 = left, 1024 =
//!   above, 1536 = right; the exact cardinals take the 17/9/1/25
//!   families and every open quadrant its diagonal family
//!   (MI:3655-3740). The camera does NOT follow the leg: the flight
//!   runs in the fixed viewport the click was made in (retail state
//!   3 pointer-scroll is gated on "not flying", MI:3133).
//! - map entry (`NewGameDraw_7EAE0` case 1, MI:3026-3105): the
//!   viewport starts on the LAST COMPLETED portal's anchor and
//!   glides to the pending portal's (state 2, 4 px/frame) after a
//!   completion or a load; after a failure or an off-route replay it
//!   snaps to the played level's anchor (a secret's = its parent's).
//!   The pending portal is not drawn — so does not pop — until the
//!   glide lands (MI:2818). The resting carpet faces the pending
//!   portal (the last flag after a failure/off-route replay).
//! - ambient decorations: the `x_BYTE_E26C8_str[16]` table
//!   (MI:199-216) via `DrawAnimSprite_81CA0` (EF:46934): loop rows
//!   draw always; burst rows are INVISIBLE while waiting, then play
//!   frames first..last-1 once. The frame-85/86 rows (the storm
//!   clouds over Vissuluth's fortress) vanish once the finale portal
//!   is won (MI:2810).
//! - the won finale: Vissuluth's island is redrawn — 12 tiles
//!   (sprites 285-296) stamped into the background buffer
//!   (`MapMenuPortalsDraw_81760` MI:3825-46) — and the `//vulcan` row
//!   of `str_WORD_E20A4` (MI:247, mode 3, portal 24) loops frames
//!   297-304 at map (610,114), also stamped into the buffer every
//!   frame (`DrawAndSoundDragonAndFire_81EE0` MI:4734-48): over the
//!   route dots, under the ambients and portals. Sample 9 fires as it
//!   starts.
//! - cursor = bank sprite 239 (MI:986 — the map screen's own; 39 is
//!   the MAIN MENU chunk's cursor).
//! - anim cadence: 100 Hz clock, portal/ambient step every 8 ticks
//!   (12.5 fps), carpet frames every 16 (6.25 fps).
//!
//! The screen renders as UI quads over one atlas (background +
//! sprite bank), swapped in for the level's UI atlas while up.

use std::collections::HashMap;
use std::path::Path;

use mgc_render::UiQuad;

use crate::campaign::{MC2_MAIN_PORTALS, MC2_PORTAL_HIT, MC2_SECRETS};
use crate::saves::Mc2Save;

/// Retail viewport size the map scrolls within.
const VIEW_W: f32 = 640.0;
const VIEW_H: f32 = 480.0;
/// The same in pixels (border frame rows).
const SCREEN_W: usize = 640;
/// Map dimensions (worldmap-bg.bin).
const MAP_W: usize = 1280;
const MAP_H: usize = 960;
/// Portal/ambient frame rate (clock step ≥8 of a 100 Hz clock).
const ANIM_FPS: f32 = 12.5;
/// Carpet frame rate (clock step ≥16).
const CARPET_FPS: f32 = 6.25;
/// Carpet travel speed: retail moves 3×2 px per ~60 Hz frame.
const TRAVEL_SPEED: f32 = 360.0;
/// The retail map-screen frame clock the per-frame laws (edge-scroll
/// ramp, entry glide) are modeled at.
const RETAIL_FPS: f32 = 70.0;
/// The entry glide: the viewport walks a Bresenham line at 4 px per
/// frame on its major axis (`CreateAnimObject_7E8D0(…, 4, 4)`,
/// MI:3081; one `MoveAnimObject_7E9D0` step per frame, MI:3138).
const GLIDE_STEP: f32 = 4.0;
/// Retail sample ids (MC2 sound bank).
const SND_PORTAL_OPEN: u8 = 41;
const SND_TRAVEL: u8 = 19;
/// The frontend click (every menu/map button, MI:2414).
const SND_CLICK: u8 = 14;
/// The finale's mana volcano (`str_WORD_E20A4` `//vulcan`, MI:247):
/// map position, the looped frames, its start sample (`word_10`).
const VULCAN_POS: (f32, f32) = (610.0, 114.0);
const VULCAN_FIRST: usize = 297;
const VULCAN_FRAMES: usize = 8;
const SND_VULCAN: u8 = 9;
/// THE FALLING STAR over level 4 ("A gift from the gods! Alas, it
/// fell towards the realm of the spires" — the Meteor level): the one
/// MODE-2 row of `str_WORD_E20A4` (MI:245; remc2 labels it "sea
/// beast", but its data is a star falling onto portal 3). While
/// portal 3 is the PENDING one (MI:4634-42 — the first portal not
/// yet won), and not under the stats table (`x_BYTE_17DB8F != 4`,
/// MI:4633), it falls once from the map's top edge to the portal —
/// flight frames 277-284 looping at 12.5 fps, 4 Bresenham steps of 2
/// px per retail frame (MI:4655-57) — then plays the impact frames
/// 93-103 there and is gone. Sample 9 at the launch, 3 at the
/// impact (`word_10` / `word_14`). Retail's state byte (`byte_43`) is
/// a static that re-arms only when portal 3 stops being pending, so
/// it falls once per run, not per map visit.
const STAR_PORTAL: u32 = 3;
const STAR: MapPath = MapPath {
    from: (595.0, 0.0),
    to: (570.0, 636.0),
    fly: (277, 284),
    hit: (93, 103),
    launch: 9,
    land: 3,
};
const SND_STAR_LAUNCH: u8 = STAR.launch;
const SND_STAR_HIT: u8 = STAR.land;
/// 4 steps × 2 px per ~60 Hz retail frame on the major axis (the
/// carpet's travel law, `TRAVEL_SPEED`, is the same stepper at 3×2).
const PATH_SPEED: f32 = 4.0 * 2.0 * 60.0;

/// One `str_WORD_E20A4` flight (MI:240-252, `DrawAndSoundDragonAndFire_
/// 81EE0`): from `from` (top-left, map px) to `to` along the Bresenham
/// stepper at [`PATH_SPEED`], looping the flight frames `fly`
/// (inclusive) at 12.5 fps; on arrival the `hit` frames play once
/// there. `launch` / `land` = the samples at the start and the
/// arrival (`word_10` / `word_14`).
struct MapPath {
    from: (f32, f32),
    to: (f32, f32),
    fly: (usize, usize),
    hit: (usize, usize),
    launch: u8,
    land: u8,
}

impl MapPath {
    /// The flight's duration: the major axis at `PATH_SPEED`.
    fn fly_time(&self) -> f32 {
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        dx.abs().max(dy.abs()) / PATH_SPEED
    }

    /// The arrival animation's duration (every `hit` frame once).
    fn hit_time(&self) -> f32 {
        (self.hit.1 - self.hit.0 + 1) as f32 / ANIM_FPS
    }

    /// Sprite + map position `t` seconds into the flight.
    fn flying(&self, t: f32) -> (usize, (f32, f32)) {
        let u = (t / self.fly_time()).clamp(0.0, 1.0);
        let pos = (
            self.from.0 + (self.to.0 - self.from.0) * u,
            self.from.1 + (self.to.1 - self.from.1) * u,
        );
        let n = self.fly.1 - self.fly.0 + 1;
        (self.fly.0 + (t.max(0.0) * ANIM_FPS) as usize % n, pos)
    }

    /// Sprite + map position `t` seconds into the arrival.
    fn landing(&self, t: f32) -> (usize, (f32, f32)) {
        let n = self.hit.1 - self.hit.0 + 1;
        (
            self.hit.0 + ((t.max(0.0) * ANIM_FPS) as usize).min(n - 1),
            self.to,
        )
    }
}

/// THE MAP'S PERIODIC FLIGHTS — the table's MODE-1 rows (MI:240-244,
/// 249-252): three dragons, a second and third kind of flyer, and four
/// falling stars (remc2's "explosion" rows: a star streaks down from
/// the top edge and bursts on the ground). Each waits `wait` seconds,
/// flies, plays its arrival frames, and starts over, forever, on its
/// own clock (MI:4580-4631). Retail's wait test is an integer-second
/// `(time − t0) / 100 > stateY` on the 100 Hz clock, so the real wait
/// is `stateY + 1` seconds — the values below. The dragons' "arrival"
/// is just more wingbeats past the map's bottom/right edge.
struct Periodic {
    path: MapPath,
    wait: f32,
}

const DRAGON: (usize, usize) = (140, 145);
const PERIODIC: [Periodic; 9] = [
    Periodic {
        path: MapPath {
            from: (384.0, 0.0),
            to: (543.0, 960.0),
            fly: DRAGON,
            hit: DRAGON,
            launch: 37,
            land: 37,
        },
        wait: 51.0,
    },
    Periodic {
        path: MapPath {
            from: (395.0, 10.0),
            to: (550.0, 970.0),
            fly: DRAGON,
            hit: DRAGON,
            launch: 37,
            land: 37,
        },
        wait: 51.0,
    },
    Periodic {
        path: MapPath {
            from: (400.0, 6.0),
            to: (555.0, 980.0),
            fly: DRAGON,
            hit: DRAGON,
            launch: 37,
            land: 37,
        },
        wait: 51.0,
    },
    Periodic {
        path: MapPath {
            from: (1.0, 190.0),
            to: (1276.0, 308.0),
            fly: (206, 211),
            hit: (206, 211),
            launch: 39,
            land: 39,
        },
        wait: 41.0,
    },
    Periodic {
        path: MapPath {
            from: (459.0, 0.0),
            to: (1278.0, 960.0),
            fly: (146, 151),
            hit: (146, 151),
            launch: 37,
            land: 37,
        },
        wait: 21.0,
    },
    Periodic {
        path: MapPath {
            from: (357.0, 0.0),
            to: (287.0, 656.0),
            fly: (107, 114),
            hit: (93, 103),
            launch: 9,
            land: 3,
        },
        wait: 16.0,
    },
    Periodic {
        path: MapPath {
            from: (939.0, 0.0),
            to: (879.0, 614.0),
            fly: (107, 114),
            hit: (93, 103),
            launch: 9,
            land: 3,
        },
        wait: 16.0,
    },
    Periodic {
        path: MapPath {
            from: (431.0, 0.0),
            to: (361.0, 512.0),
            fly: (107, 114),
            hit: (93, 103),
            launch: 9,
            land: 3,
        },
        wait: 12.0,
    },
    Periodic {
        path: MapPath {
            from: (455.0, 0.0),
            to: (395.0, 114.0),
            fly: (107, 114),
            hit: (93, 103),
            launch: 9,
            land: 3,
        },
        wait: 19.0,
    },
];
/// Table order: the dragons/flyers (rows 0-4) draw before the
/// level-4 star, the falling stars (5-8) after it (MI:4568 walks the
/// rows in order).
const PERIODIC_BEFORE_STAR: usize = 5;

/// A periodic flight's phase (retail `byte_43` cases 0-4).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Flight {
    /// Waiting since this `anim` time.
    Waiting(f32),
    /// Flying since this `anim` time.
    Flying(f32),
    /// Arrival frames since this `anim` time.
    Landing(f32),
}

/// The falling star's phase (retail `byte_43` of its row).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Star {
    /// Waiting for portal 3 to be pending (and the stats table gone).
    Armed,
    /// Falling since this `anim` time.
    Falling(f32),
    /// The impact animation since this `anim` time.
    Impact(f32),
    /// Played; re-arms when portal 3 stops being pending.
    Done,
}

impl Star {
    /// The fall's duration.
    fn fall_time() -> f32 {
        STAR.fly_time()
    }
}

/// The finale's island redraw: sprites 285-296 at their map
/// positions (`MapMenuPortalsDraw_81760`, MI:3835-46).
const VULCAN_ISLAND: [(usize, (f32, f32)); 12] = [
    (285, (518.0, 17.0)),
    (286, (583.0, 17.0)),
    (287, (657.0, 17.0)),
    (288, (696.0, 17.0)),
    (289, (518.0, 88.0)),
    (290, (574.0, 88.0)),
    (291, (657.0, 88.0)),
    (292, (706.0, 88.0)),
    (293, (518.0, 156.0)),
    (294, (582.0, 156.0)),
    (295, (657.0, 156.0)),
    (296, (703.0, 156.0)),
];

/// `LevelsNames_D9204` (EF:2305) — the MC2 level names by level
/// number (main 0-24, secrets 30-34, multiplayer 50-59), as the
/// executable carries them (trailing spaces trimmed at the draw).
/// Retail level `n`'s name (`LevelsNames_D9204`; the arenas 50-59 carry
/// bare names — the lobby prefixes `n − 49`).
pub(crate) fn mc2_level_name(n: u32) -> &'static str {
    MC2_LEVEL_NAMES.get(n as usize).copied().unwrap_or("")
}

const MC2_LEVEL_NAMES: [&str; 61] = [
    "1. Jahwl",
    "2. Kopahk",
    "3. Myrnan Gor",
    "4. Arachnium",
    "5. T'Klom",
    "6. Phyrydia",
    "7. Perilium",
    "8. Ul Buthnen",
    "9. Evirith Gor",
    "10. Cymmeria ",
    "11. Tropolos ",
    "12. Jaleen ",
    "13. Galiphur ",
    "14. Tunuk ",
    "15. Zyggogg ",
    "16. Darklava ",
    "17. C'lannesh ",
    "18. Gleph ",
    "19. Baraghan ",
    "20. Ammyridia ",
    "21. Cresidan ",
    "22. Hodor ",
    "23. Jathnar ",
    "24. Malak ",
    "25. Uluth ",
    "26. ",
    "27. ",
    "28. ",
    "29. ",
    "30. ",
    "Karakir ",
    "Ymbul",
    "Pav Durivium",
    "Beleem",
    "Ommosyth",
    "36. ",
    "37. ",
    "38. ",
    "39. ",
    "40. ",
    "41. ",
    "42. ",
    "43. ",
    "44. ",
    "45. ",
    "46. ",
    "47. ",
    "48. ",
    "49. ",
    "50. ",
    "Thrull",
    "Keevur",
    "Braak",
    "Trapox",
    "Hibren Zhor",
    "Jinople",
    "Dethrem",
    "Canquin",
    "Zephulum",
    "Verune",
    "0",
];

/// One drawable/clickable portal, resolved from the campaign record.
struct Portal {
    level: u32,
    pos: (f32, f32),
    state: PortalState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PortalState {
    /// Completed main level — the planted flag (replayable).
    Flag,
    /// The next uncompleted main level (pop-in, then open portal).
    Next,
    /// Revealed-uncompleted secret portal (pop-in, then 270-272).
    SecretRevealed,
    /// Completed secret portal (replayable).
    SecretDone,
}

/// The once-per-session portal materialization (retail `byte_19` /
/// `byte_16` — reset every launch, so reopening the map replays
/// nothing but a fresh reveal always pops).
enum Pop {
    Popping { started: f32 },
    Done,
}

/// The travelling carpet: parked position to portal-center.
struct Travel {
    pos: (f32, f32),
    target: (f32, f32),
    /// Level to launch on arrival (the clicked portal).
    launch: Option<u32>,
    /// This leg flies the canonical frontier segment (departing the
    /// last completed portal for the pending one) — the only flight
    /// that draws route dots (off-route trips never generate the
    /// line).
    on_route: bool,
}

/// Ambient set-dressing row (`x_BYTE_E26C8_str`, semantic fields).
/// `burst` rows wait invisible for `delay` seconds, play one cycle
/// (firing `sound` as it starts — the map's creature screams,
/// EF:46999-47009), repeat; loop rows draw forever. Frames span
/// `first..last` — the retail wrap at `last-2` makes `last` itself
/// unused (EF:46945-49). `start` = the authored initial frame
/// (phase-offsets the loop rows so the meteors don't fall in sync).
struct Ambient {
    pos: (f32, f32),
    first: u16,
    last: u16,
    start: u16,
    delay: f32,
    burst: bool,
    sound: Option<u8>,
}

/// The table verbatim (MI:200-215, terminator dropped). Sounds fire
/// only on rows whose `time4_22 != -1`: the two head-poke screams
/// (sample 38), the frame-86 burst at (545,54) (sample 23), and the
/// per-cycle meteor whoosh row (sample 5 — the burst twin of the
/// (831,245) loop meteor, delay 0 so it fires every cycle). The
/// frames-46-58 loop rows ARE the falling-star streaks — the one at
/// (630,607) sits by portal 3's region (the "star that fell" level).
const AMBIENTS: [Ambient; 15] = [
    Ambient {
        pos: (447.0, 628.0),
        first: 115,
        last: 138,
        start: 115,
        delay: 4.0,
        burst: true,
        sound: Some(38),
    },
    Ambient {
        pos: (876.0, 534.0),
        first: 115,
        last: 138,
        start: 117,
        delay: 8.0,
        burst: true,
        sound: Some(38),
    },
    Ambient {
        pos: (545.0, 54.0),
        first: 85,
        last: 86,
        start: 85,
        delay: 0.0,
        burst: false,
        sound: None,
    },
    Ambient {
        pos: (655.0, 58.0),
        first: 85,
        last: 86,
        start: 85,
        delay: 0.0,
        burst: false,
        sound: None,
    },
    Ambient {
        pos: (564.0, 88.0),
        first: 85,
        last: 86,
        start: 85,
        delay: 0.0,
        burst: false,
        sound: None,
    },
    Ambient {
        pos: (614.0, 123.0),
        first: 85,
        last: 86,
        start: 85,
        delay: 0.0,
        burst: false,
        sound: None,
    },
    Ambient {
        pos: (545.0, 54.0),
        first: 86,
        last: 92,
        start: 86,
        delay: 8.0,
        burst: true,
        sound: Some(23),
    },
    Ambient {
        pos: (655.0, 58.0),
        first: 86,
        last: 92,
        start: 88,
        delay: 4.0,
        burst: true,
        sound: None,
    },
    Ambient {
        pos: (564.0, 88.0),
        first: 86,
        last: 92,
        start: 89,
        delay: 22.0,
        burst: true,
        sound: None,
    },
    Ambient {
        pos: (614.0, 123.0),
        first: 86,
        last: 92,
        start: 90,
        delay: 21.0,
        burst: true,
        sound: None,
    },
    Ambient {
        pos: (831.0, 245.0),
        first: 46,
        last: 58,
        start: 49,
        delay: 0.0,
        burst: false,
        sound: None,
    },
    Ambient {
        pos: (831.0, 245.0),
        first: 46,
        last: 58,
        start: 49,
        delay: 0.0,
        burst: true,
        sound: Some(5),
    },
    Ambient {
        pos: (863.0, 329.0),
        first: 46,
        last: 58,
        start: 46,
        delay: 0.0,
        burst: false,
        sound: None,
    },
    Ambient {
        pos: (630.0, 607.0),
        first: 46,
        last: 58,
        start: 52,
        delay: 0.0,
        burst: false,
        sound: None,
    },
    Ambient {
        pos: (244.0, 632.0),
        first: 46,
        last: 58,
        start: 56,
        delay: 0.0,
        burst: false,
        sound: None,
    },
];

/// The four always-on map-screen corner buttons
/// (`mapMenuButtons_E23E0`, MI:321-26): grey idle / gold hover sprite
/// pairs, hit-box = the sprite's own dims (MI:2408-9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapButton {
    /// Bottom-right (581,427), sprites 246/247 — back to the menu.
    Exit,
    /// Bottom-left (0,427), 248/249 — confirm + campaign reset.
    NewGame,
    /// Top-left (0,0), 250/251.
    Save,
    /// Top-right (581,0), 252/253.
    Load,
}

const MAP_BUTTONS: [(MapButton, (f32, f32), usize, usize); 4] = [
    (MapButton::Exit, (581.0, 427.0), 246, 247),
    (MapButton::NewGame, (0.0, 427.0), 248, 249),
    (MapButton::Save, (0.0, 0.0), 250, 251),
    (MapButton::Load, (581.0, 0.0), 252, 253),
];

/// A committed frontend action, drained by the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapAction {
    /// Write the campaign record to this slot under this label.
    /// Write the campaign to a slot. Carries NO label: slot names are
    /// not player-authored (the row is the player name + level).
    SaveTo { slot: usize },
    /// Load this slot's record.
    LoadFrom(usize),
    /// Confirmed campaign reset (retail sub_7E640 — stays on the map).
    NewGame,
    /// Leave the map for the main menu (Exit button / Esc).
    ExitToMenu,
}

/// Which parchment dialog is up (retail scroll dialogs; anchors +
/// heights from the `str_26` descriptors, MI:321-26).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Save,
    Load,
    NewGame,
}

/// The pop-open parchment scroll dialog (`DrawScrollDialog_7BF20`
/// MI:5402: opens toward its full height in 16-px steps, OK/Cancel
/// once fully open; slot rows at 16-px pitch).
struct Dialog {
    kind: DialogKind,
    anchor: (f32, f32),
    height: f32,
    /// Animated opening height (16 px per retail frame).
    open: f32,
    /// Slot labels + occupied flags (scanned by the app on open).
    slots: Vec<(String, bool)>,
    selected: Option<usize>,
    /// Save-label edit buffer (Some while typing; retail sub_7F6A0:
    /// filtered chars, max 15, "_" caret).
    edit: Option<String>,
}

/// The retail scroll-dialog width — the roller-bar art (sprite 254,
/// 114 px); anchors 29/510 line the Save/Load dialogs up under
/// their corner buttons.
const DIALOG_W: f32 = 114.0;

/// OK / Cancel positions+hit rects (retail DrawScrollDialog2 mode 3:
/// OK at x1+15, Cancel right-aligned to x1+barW-12, bottoms on
/// y1+height — resting on the bottom roller).
fn dialog_button_rects(
    anchor: (f32, f32),
    height: f32,
    bar_w: f32,
) -> ((f32, f32, f32, f32), (f32, f32, f32, f32)) {
    let (x1, y1) = anchor;
    let ok = (x1 + 15.0, y1 + height - 28.0, 42.0, 28.0);
    let cancel = (x1 + bar_w - 12.0 - 39.0, y1 + height - 30.0, 39.0, 30.0);
    (ok, cancel)
}

fn in_rect(mx: f32, my: f32, r: (f32, f32, f32, f32)) -> bool {
    mx >= r.0 && mx < r.0 + r.2 && my >= r.1 && my < r.1 + r.3
}

pub struct WorldMap {
    /// RGBA atlas: the map background at (0,0)..(1280,960), the
    /// sprite bank's packed atlas blitted below it at y = 960, the
    /// 640×480 border frame below that, the FONT1 glyph masks
    /// (white, tinted per draw) at the bottom.
    atlas: Vec<u8>,
    atlas_w: u32,
    atlas_h: u32,
    /// Sprite id → (atlas x, atlas y, w, h), frame 0.
    rects: Vec<Option<(f32, f32, f32, f32)>>,
    /// The border frame's atlas rect (640×480, transparent center).
    border_rect: Option<(f32, f32, f32, f32)>,
    /// FONT1 glyph rects, id = char + 1 (glyphs are white masks).
    font: Vec<Option<(f32, f32, f32, f32)>>,
    /// The English frontend strings (LANGUAGE/L2.TXT): level
    /// descriptions at 23+level, dialog titles 421/422/467.
    strings: Vec<String>,
    dialog: Option<Dialog>,
    /// The end-game stats table (`DrawEndGameTable_82C20`) up for this
    /// level — retail map mode 4 (entry after a completed level).
    stats: Option<u32>,
    /// Tables still to show after this one (a secret level's parent
    /// comes first — see `CampaignRun::map_stats`).
    stats_next: std::collections::VecDeque<u32>,
    pending_action: Option<MapAction>,
    pending_button: Option<MapButton>,
    /// The pending level's description text stays up until a portal
    /// click starts a leg (re-arms per visit with the narrative).
    desc_dismissed: bool,
    /// Map scroll in retail 640×480 viewport units.
    pub scroll: (f32, f32),
    /// Animation clock (seconds).
    anim: f32,
    /// Per-portal-level materialization state (session-local).
    pop: HashMap<u32, Pop>,
    travel: Option<Travel>,
    /// Level launch armed by an arrived click-travel.
    pending_launch: Option<u32>,
    /// Sample ids fired this frame, drained by the app's mixer.
    sounds: Vec<u8>,
    /// The next-level narrative latch: retail plays the pending
    /// level's briefing (speech row = level, segment 0) once per
    /// map visit (`IsPlayingCDTrack_17E09D`,
    /// `PresentLevelDescription_80C30` MI:3596-3601).
    narrated: bool,
    /// The pending narrative, drained by the app (level number).
    pending_narrative: Option<u32>,
    /// Where the carpet rests between legs — the portal of the
    /// level just played (`set_parked`).
    parked: (f32, f32),
    /// The dotted route: the trail is the
    /// FIXED main-line path, identical on every load — the only
    /// question per segment is drawn-or-not. Segments up to the
    /// frontier stamp on map entry; the frontier segment (into a
    /// portal revealed THIS session's last completion) stays blank
    /// until the carpet actually flies it (or the next entry).
    /// `levels_completed` as of the previous entry — None = first
    /// entry this session (a load: full trail).
    last_seen_completed: Option<u32>,
    /// The frontier segment (last completed → pending portal) is
    /// stamped.
    frontier_drawn: bool,
    /// The edge-scroll ramp (retail `shift_step`, px/frame at the
    /// 70 Hz retail clock; 0 while no edge is touched).
    edge_step: f32,
    /// The entry glide's destination — the pending portal's viewport
    /// anchor — while the camera is still walking there (retail
    /// state 2); None once parked. Clicks, key/pointer scrolling and
    /// the pending portal's pop-in all wait for it.
    glide: Option<(f32, f32)>,
    /// What the resting carpet faces (map position): the pending
    /// portal after a completion or a load, the last flag after a
    /// failure or an off-route replay, a secret's parent portal
    /// after a secret (`SetAnimationVariables_7DA70` call sites,
    /// MI:2975-3013).
    faces: (f32, f32),
    /// The mana volcano's start time on `anim` — Some once the whole
    /// campaign is won. Retail's state byte (`byte_43`) is a static
    /// that only re-arms when the condition fails, so the start
    /// sample fires once, not per visit.
    vulcan: Option<f32>,
    /// The level-4 falling star ([`Star`]).
    star: Star,
    /// The periodic flights ([`PERIODIC`]), one phase per row.
    flights: [Flight; 9],
}

/// A stats-table row: a stat, a breakdown sub-row (indented), or a
/// row packed into the block above it at the stat's indent ("Mana
/// unclaimed" — no half-line gap before it).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Main,
    Sub,
    Packed,
}

impl WorldMap {
    /// Load the baked `assets/mc2-ui` world-map bundle.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| -> Result<Vec<u8>, String> {
            std::fs::read(dir.join(name)).map_err(|e| {
                format!(
                    "{}: {e} (rebake — epoch 16 adds it)",
                    dir.join(name).display()
                )
            })
        };
        let bg = read("worldmap-bg.bin")?;
        if bg.len() != MAP_W * MAP_H {
            return Err(format!(
                "worldmap-bg.bin: {} bytes (want {})",
                bg.len(),
                MAP_W * MAP_H
            ));
        }
        let pal = read("worldmap-pal.bin")?;
        if pal.len() != 768 {
            return Err(format!("worldmap-pal.bin: {} bytes (want 768)", pal.len()));
        }
        let sprites_px = read("worldmap-sprites.bin")?;
        let index: mgc_formats::bundle::SpriteIndex =
            serde_json::from_slice(&read("worldmap-sprites.json")?)
                .map_err(|e| format!("worldmap-sprites.json: {e}"))?;
        if sprites_px.len() != (index.atlas_width * index.atlas_height) as usize {
            return Err("worldmap-sprites.bin does not match its index".into());
        }

        // Resolve the 6-bit VGA palette once (<<2 to 8-bit).
        let rgb =
            |i: usize| -> [u8; 3] { [pal[i * 3] << 2, pal[i * 3 + 1] << 2, pal[i * 3 + 2] << 2] };

        // The frontend overlay members. Optional: an older bake still
        // gets the map, just without the border overlay / dialogs /
        // description text.
        let border = std::fs::read(dir.join("worldmap-border.bin")).ok();
        let font_px = std::fs::read(dir.join("font.bin")).ok();
        let font_index: Option<mgc_formats::bundle::SpriteIndex> = font_px
            .as_ref()
            .and_then(|_| std::fs::read(dir.join("font.json")).ok())
            .and_then(|b| serde_json::from_slice(&b).ok());
        let strings: Vec<String> = std::fs::read(dir.join("strings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if border.is_none() || font_index.is_none() || strings.is_empty() {
            eprintln!(
                "note: mc2-ui bundle predates epoch 18 — map overlay/menus degraded (rebake)"
            );
        }

        // One RGBA atlas: bg on top, sprite atlas below, then the
        // 640×480 border frame, then the FONT1 glyph masks (white —
        // tinted per draw). The bg is opaque; everything else
        // resolves index 0 to alpha 0 (the engine-wide transparent
        // index).
        let border_h = border.as_ref().map_or(0, |b| b.len() / SCREEN_W) as u32;
        let font_h = font_index.as_ref().map_or(0, |f| f.atlas_height);
        let atlas_w = MAP_W as u32;
        let atlas_h = MAP_H as u32 + index.atlas_height + border_h + font_h;
        let mut atlas = vec![0u8; (atlas_w * atlas_h * 4) as usize];
        for (i, &p) in bg.iter().enumerate() {
            let c = rgb(p as usize);
            let o = i * 4;
            atlas[o..o + 3].copy_from_slice(&c);
            atlas[o + 3] = 255;
        }
        for y in 0..index.atlas_height as usize {
            for x in 0..index.atlas_width as usize {
                let p = sprites_px[y * index.atlas_width as usize + x];
                if p == 0 {
                    continue;
                }
                let c = rgb(p as usize);
                let o = ((MAP_H + y) * MAP_W + x) * 4;
                atlas[o..o + 3].copy_from_slice(&c);
                atlas[o + 3] = 255;
            }
        }
        let border_y = MAP_H + index.atlas_height as usize;
        if let Some(b) = &border {
            for (i, &p) in b.iter().enumerate() {
                if p == 0 {
                    continue;
                }
                let c = rgb(p as usize);
                let o = ((border_y + i / SCREEN_W) * MAP_W + i % SCREEN_W) * 4;
                atlas[o..o + 3].copy_from_slice(&c);
                atlas[o + 3] = 255;
            }
        }
        let font_y = border_y + border_h as usize;
        let mut font_rects: Vec<Option<(f32, f32, f32, f32)>> = Vec::new();
        if let (Some(px), Some(fi)) = (&font_px, &font_index) {
            for y in 0..fi.atlas_height as usize {
                for x in 0..fi.atlas_width as usize {
                    if px[y * fi.atlas_width as usize + x] == 0 {
                        continue;
                    }
                    let o = ((font_y + y) * MAP_W + x) * 4;
                    atlas[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
            font_rects = fi
                .sprites
                .iter()
                .map(|s| {
                    let f = s.frames.first()?;
                    (s.width > 0 && s.height > 0).then_some((
                        f.x as f32,
                        (f.y as usize + font_y) as f32,
                        s.width as f32,
                        s.height as f32,
                    ))
                })
                .collect();
        }

        let rects = index
            .sprites
            .iter()
            .map(|s| {
                let f = s.frames.first()?;
                (s.width > 0 && s.height > 0).then_some((
                    f.x as f32,
                    (f.y + MAP_H as u32) as f32,
                    s.width as f32,
                    s.height as f32,
                ))
            })
            .collect();

        Ok(Self {
            atlas,
            atlas_w,
            atlas_h,
            rects,
            border_rect: border.map(|_| (0.0, border_y as f32, SCREEN_W as f32, border_h as f32)),
            font: font_rects,
            strings,
            dialog: None,
            stats: None,
            stats_next: Default::default(),
            pending_action: None,
            pending_button: None,
            desc_dismissed: false,
            scroll: (0.0, 0.0),
            anim: 0.0,
            pop: HashMap::new(),
            travel: None,
            pending_launch: None,
            sounds: Vec::new(),
            narrated: false,
            pending_narrative: None,
            parked: (0.0, 0.0),
            last_seen_completed: None,
            frontier_drawn: false,
            edge_step: 0.0,
            glide: None,
            faces: (0.0, 0.0),
            vulcan: None,
            star: Star::Armed,
            flights: [Flight::Waiting(0.0); 9],
        })
    }

    /// Forget the session-local presentation state — a LOADED or
    /// RESET campaign record starts a fresh session: pop-in latches
    /// replay, the route stamps in full on the next entry, the
    /// carpet is unparked (hidden until the first launch when
    /// nothing is completed).
    pub fn session_reset(&mut self) {
        self.pop.clear();
        self.travel = None;
        self.glide = None;
        self.last_seen_completed = None;
        self.parked = (0.0, 0.0);
        self.dialog = None;
    }

    /// A fresh map visit: the narrative latch re-arms (retail
    /// resets its once-per-visit CD-track flag on entry), and the
    /// dotted route re-stamps — the frontier segment joins it
    /// UNLESS its portal was revealed by the completion that led
    /// here (then the carpet flying there draws it, or the next
    /// entry does).
    pub fn enter_visit(&mut self, save: &Mc2Save) {
        self.narrated = false;
        self.desc_dismissed = false;
        let completed = save.levels_completed;
        self.frontier_drawn = match self.last_seen_completed {
            // First sight this session (boot/load): the full trail.
            None => true,
            // Re-entry at the same frontier: stamped.
            Some(c) if c == completed => true,
            // The frontier just advanced: its segment waits for the
            // carpet (or the next visit).
            _ => false,
        };
        self.last_seen_completed = Some(completed);
    }

    /// The composed RGBA atlas for `Renderer::load_ui_atlas`.
    pub fn atlas(&self) -> (u32, u32, &[u8]) {
        (self.atlas_w, self.atlas_h, &self.atlas)
    }

    /// Place the camera for a map entry (`NewGameDraw_7EAE0` case 1,
    /// MI:3026-3105) and decide what the resting carpet faces.
    /// `played` = the level just played (None on boot/load, where
    /// retail's load arm clears the just-played record, MI:1567):
    ///
    /// - the level just played IS the newest flag (a completion, a
    ///   replay of it) or there is none: the viewport starts on the
    ///   last completed portal's anchor and GLIDES to the pending
    ///   portal's (state 2). With nothing completed it simply sits
    ///   on portal 0's anchor (the `jx == 0` snap, MI:3086); with the
    ///   campaign complete it stays on the finale's.
    /// - otherwise (a failure of the pending level, an off-route
    ///   replay, a secret): snap to the played level's own anchor — a
    ///   secret's is its parent main level's (MI:3040-3062).
    pub fn anchor_to(&mut self, save: &Mc2Save, played: Option<u32>) {
        let completed = save.levels_completed as usize;
        let last = completed.checked_sub(1);
        let anchor = |i: usize| {
            let (vx, vy) = MC2_MAIN_PORTALS[i.min(MC2_MAIN_PORTALS.len() - 1)].viewport;
            (vx as f32, vy as f32)
        };
        self.glide = None;
        match played {
            Some(l) if last != Some(l as usize) => {
                let secret = MC2_SECRETS.iter().find(|&&(_, s, _)| s as u32 == l);
                if let Some(&(parent, _, _)) = secret {
                    // A secret parks on its own portal facing the
                    // parent main portal (MI:2996-2999).
                    self.scroll = anchor(parent as usize);
                    self.faces = self.portal_center(parent as usize);
                } else {
                    // A main level faces the newest flag (retail's
                    // `index4`; with NO flag retail indexes portal
                    // -1 — garbage; we face the portal itself, i.e.
                    // the straight-down family).
                    self.scroll = anchor(l as usize);
                    self.faces = self.portal_center(last.unwrap_or(l as usize));
                }
            }
            _ => {
                let pending = (completed < MC2_MAIN_PORTALS.len()).then_some(completed);
                match (last, pending) {
                    (Some(from), Some(to)) => {
                        self.scroll = anchor(from);
                        self.glide = Some(anchor(to));
                    }
                    (None, _) => self.scroll = anchor(0),
                    (Some(from), None) => self.scroll = anchor(from),
                }
                // Facing the pending portal; with none (or, per the
                // retail draw law, while the glide is still walking)
                // the finale's neighbour, portal 23 (MI:3007-3013).
                self.faces = self.portal_center(pending.unwrap_or(23));
            }
        }
        self.clamp();
    }

    /// The carpet's heading family — retail
    /// `sub_581E0_maybe_tan2(&target, &carpet)` fed to the family
    /// ladder shared by the flight (`sub_80D40`, MI:3655-3740) and the
    /// resting pose (`SetAnimationVariables_7DA70`, MI:2297-2380).
    /// The tan of (carpet − target) in 2048 units reads 0 = target
    /// straight below, 512 = left, 1024 = above, 1536 = right; only
    /// the exact cardinals take a cardinal family, every open
    /// quadrant takes its diagonal one. Retail works in integer map
    /// pixels, so the residual is rounded first.
    fn carpet_family(dx: f32, dy: f32) -> usize {
        // `sub_72633_maybe_tan(a1, a2)` with a1 = -dx, a2 = -dy.
        let (a1, a2) = (-(dx.round() as i32), -(dy.round() as i32));
        match (a1.signum(), a2.signum()) {
            (0, 0) | (0, -1) => 17, // 0 / 2048: below
            (1, 0) => 9,            // 512: left
            (0, 1) => 1,            // 1024: above
            (-1, 0) => 25,          // 1536: right
            (1, -1) => 5,           // (0, 512): below-left
            (1, 1) => 13,           // (512, 1024): above-left
            (-1, 1) => 21,          // (1024, 1536): above-right
            _ => 29,                // (1536, 2048): below-right
        }
    }

    /// The center of a main portal (retail anchors travel to the
    /// portal position plus half the flag sprite, MI:2283-90).
    fn portal_center(&self, i: usize) -> (f32, f32) {
        let (w, h) = self
            .rects
            .get(37)
            .copied()
            .flatten()
            .map_or((20.0, 20.0), |(_, _, w, h)| (w, h));
        let p = MC2_MAIN_PORTALS[i].pos;
        (p.0 as f32 + w / 2.0, p.1 as f32 + h / 2.0)
    }

    /// The map position of a level's portal center — mains anchor
    /// to the flag sprite footprint, secrets to their own spot.
    fn level_pos(&self, level: u32) -> (f32, f32) {
        if let Some(&(_, _, pos)) = MC2_SECRETS.iter().find(|&&(_, l, _)| l as u32 == level) {
            let half = MC2_PORTAL_HIT as f32 / 2.0;
            return (pos.0 as f32 + half, pos.1 as f32 + half);
        }
        self.portal_center((level as usize).min(MC2_MAIN_PORTALS.len() - 1))
    }

    /// Park the carpet on the level just played (completed, failed
    /// or replayed — the player's map position). Across save/load
    /// retail itself resets to the
    /// last activated portal (the `.GAM` stores no position), which
    /// is what `run.current` resolves to on resume.
    pub fn set_parked(&mut self, level: u32) {
        self.parked = self.level_pos(level);
    }

    /// Advance the animations, the ambient sounds, the narrative
    /// latch and the carpet.
    pub fn tick(&mut self, dt: f32, save: &Mc2Save) {
        let prev = self.anim;
        self.anim += dt;
        // The parchment scroll opens 16 px per retail frame
        // (DrawScrollDialog_7BF20 MI:5419-51, ~70 Hz).
        if let Some(d) = &mut self.dialog {
            d.open = (d.open + 16.0 * 70.0 * dt).min(d.height);
        }
        // Ambient burst sounds — fire as a burst's visible phase
        // begins (retail plays the sample on the wait→anim edge,
        // EF:46999-47009): the creature screams and the meteor
        // whoosh.
        for a in &AMBIENTS {
            let (Some(id), true) = (a.sound, a.burst) else {
                continue;
            };
            let count = (a.last - a.first).max(1) as f32;
            let period = a.delay + count / ANIM_FPS;
            let starts = |t: f32| ((t - a.delay) / period).floor();
            if self.anim > a.delay && starts(self.anim) > starts(prev) {
                self.sounds.push(id);
            }
        }
        // The mana volcano starts once the finale is won (retail
        // `byte_42 == 3` arm: the first unwon portal, less one, is
        // 24 only when all 25 are won, MI:4697-4703).
        if save.levels_completed >= 25 {
            if self.vulcan.is_none() {
                self.vulcan = Some(self.anim);
                self.sounds.push(SND_VULCAN);
            }
        } else {
            self.vulcan = None;
        }
        // The periodic flights (see `PERIODIC`): wait, launch (sample),
        // fly, arrive (sample), play the arrival frames, start over.
        for (f, row) in self.flights.iter_mut().zip(&PERIODIC) {
            match *f {
                Flight::Waiting(t0) if self.anim - t0 >= row.wait => {
                    *f = Flight::Flying(self.anim);
                    self.sounds.push(row.path.launch);
                }
                Flight::Flying(t0) if self.anim - t0 >= row.path.fly_time() => {
                    *f = Flight::Landing(self.anim);
                    self.sounds.push(row.path.land);
                }
                Flight::Landing(t0) if self.anim - t0 >= row.path.hit_time() => {
                    *f = Flight::Waiting(self.anim);
                }
                _ => {}
            }
        }
        // The level-4 falling star (see `Star`).
        if save.levels_completed == STAR_PORTAL {
            match self.star {
                Star::Armed if self.stats.is_none() => {
                    self.star = Star::Falling(self.anim);
                    self.sounds.push(SND_STAR_LAUNCH);
                }
                Star::Falling(t0) if self.anim - t0 >= Star::fall_time() => {
                    self.star = Star::Impact(self.anim);
                    self.sounds.push(SND_STAR_HIT);
                }
                Star::Impact(t0) if self.anim - t0 >= STAR.hit_time() => {
                    self.star = Star::Done;
                }
                _ => {}
            }
        } else {
            self.star = Star::Armed;
        }
        // The pending-level narrative: once per map visit, after the
        // next portal has materialized — suppressed while that
        // level's secret portal is non-hidden (retail
        // `PresentLevelDescription_80C30` MI:3583-3601: text 23+lvl,
        // speech row = lvl segment 0, `IsPlayingCDTrack` latch).
        if !self.narrated && self.travel.is_none() && self.glide.is_none() {
            let next = save.levels_completed;
            if next < 25 {
                let suppressed = MC2_SECRETS.iter().enumerate().any(|(i, &(parent, _, _))| {
                    parent as u32 == next && matches!(save.secrets[i].activated, 1 | 2)
                });
                if !suppressed && matches!(self.pop.get(&next), Some(Pop::Done)) {
                    self.narrated = true;
                    self.pending_narrative = Some(next);
                }
            }
        }
        // The entry glide: the viewport walks the line to the
        // pending portal's anchor, 4 px/frame on the major axis.
        if let Some(to) = self.glide {
            let (dx, dy) = (to.0 - self.scroll.0, to.1 - self.scroll.1);
            let major = dx.abs().max(dy.abs());
            let step = GLIDE_STEP * RETAIL_FPS * dt;
            if major <= step {
                self.scroll = to;
                self.glide = None;
            } else {
                let f = step / major;
                self.scroll.0 += dx * f;
                self.scroll.1 += dy * f;
            }
            self.clamp();
        }
        let Some(t) = &mut self.travel else { return };
        let (dx, dy) = (t.target.0 - t.pos.0, t.target.1 - t.pos.1);
        let dist = (dx * dx + dy * dy).sqrt();
        let step = TRAVEL_SPEED * dt;
        if dist <= step.max(3.0) {
            // Arrived: park there; a click leg launches its level;
            // a flown frontier leg completes the dotted route.
            let t = self.travel.take().unwrap();
            self.parked = t.target;
            self.pending_launch = t.launch;
            if t.on_route {
                self.frontier_drawn = true;
            }
            return;
        }
        // The camera stays where the click was made: retail's leg
        // flies in the fixed viewport (no state moves `posx/posy`
        // while `x_BYTE_17DB8E` is up, MI:3133) — the carpet comes
        // in from off-screen when it was parked out of view.
        t.pos.0 += dx / dist * step;
        t.pos.1 += dy / dist * step;
    }

    /// A click-travel has landed: the level to launch, once.
    pub fn take_launch(&mut self) -> Option<u32> {
        self.pending_launch.take()
    }

    /// Sample ids fired since the last drain.
    pub fn take_sounds(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.sounds)
    }

    /// The pending-level briefing to play (speech row, segment 0),
    /// once per visit.
    pub fn take_narrative(&mut self) -> Option<u32> {
        self.pending_narrative.take()
    }

    /// Pan the viewport (move keys), retail-viewport units.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if self.travel.is_some() || self.glide.is_some() {
            return; // the travel leg / entry glide owns the camera
        }
        self.scroll.0 += dx;
        self.scroll.1 += dy;
        self.clamp();
    }

    /// Retail pointer edge-scroll (MI:3132-75): the confined cursor
    /// sitting on the exact screen-edge pixel scrolls the map that
    /// way, X and Y independent (corner = diagonal), with the retail
    /// ramp — `shift_step += 4` per moving frame, capped 24 px/frame,
    /// reset the moment no edge is touched. Modeled at the ~70 Hz
    /// retail frame clock; suspended while the carpet flies (the leg
    /// owns the camera, MI:3133).
    pub fn edge_scroll(&mut self, dir: (f32, f32), dt: f32) {
        if dir == (0.0, 0.0)
            || self.travel.is_some()
            || self.glide.is_some()
            || self.dialog.is_some()
        {
            self.edge_step = 0.0;
            return;
        }
        self.edge_step = (self.edge_step + 4.0 * RETAIL_FPS * dt).min(24.0);
        self.scroll.0 += dir.0 * self.edge_step * RETAIL_FPS * dt;
        self.scroll.1 += dir.1 * self.edge_step * RETAIL_FPS * dt;
        self.clamp();
    }

    fn clamp(&mut self) {
        self.scroll.0 = self.scroll.0.clamp(0.0, (MAP_W as f32) - VIEW_W);
        self.scroll.1 = self.scroll.1.clamp(0.0, (MAP_H as f32) - VIEW_H);
    }

    /// The clickable portals for the current campaign record.
    fn portals(save: &Mc2Save) -> Vec<Portal> {
        let mut out = Vec::new();
        let completed = save.levels_completed as usize;
        for (i, p) in MC2_MAIN_PORTALS.iter().enumerate() {
            let state = match i.cmp(&completed) {
                std::cmp::Ordering::Less => PortalState::Flag,
                std::cmp::Ordering::Equal => PortalState::Next,
                std::cmp::Ordering::Greater => continue, // still hidden
            };
            out.push(Portal {
                level: i as u32,
                pos: (p.pos.0 as f32, p.pos.1 as f32),
                state,
            });
        }
        for (i, &(_, level, pos)) in MC2_SECRETS.iter().enumerate() {
            let state = match save.secrets[i].activated {
                1 => PortalState::SecretDone,
                2 => PortalState::SecretRevealed,
                _ => continue, // hidden
            };
            out.push(Portal {
                level: level as u32,
                pos: (pos.0 as f32, pos.1 as f32),
                state,
            });
        }
        out
    }

    /// Resolve one portal's current sprite through the pop-in
    /// machine (starts it on first sight where due). None = not
    /// drawn yet (materialization pending behind a travel leg).
    fn portal_sprite(&mut self, p: &Portal) -> Option<usize> {
        let frame = (self.anim * ANIM_FPS) as usize;
        let (pops, idle): (bool, usize) = match p.state {
            PortalState::Flag => return Some(37 + frame % 7),
            PortalState::SecretDone => return Some(305 + frame % 7),
            PortalState::Next => (true, 33),
            PortalState::SecretRevealed => (true, 270),
        };
        debug_assert!(pops);
        match self.pop.get(&p.level) {
            None => {
                self.pop
                    .insert(p.level, Pop::Popping { started: self.anim });
                self.sounds.push(SND_PORTAL_OPEN);
                Some(70)
            }
            Some(Pop::Popping { started }) => {
                let f = 70 + ((self.anim - started) * ANIM_FPS) as usize;
                if f > 83 {
                    self.pop.insert(p.level, Pop::Done);
                    Some(idle)
                } else {
                    Some(f)
                }
            }
            Some(Pop::Done) => Some(idle + frame % 3),
        }
    }

    // ----------------------------------------------------------------
    // Frontend overlay: border frame, corner buttons, parchment
    // dialogs, FONT1 text (docs/traces/mc2-campaign-save-menu.md,
    // "Map border overlay" recon).

    /// A screen-space sprite (640×480 coordinates, ignores the map
    /// scroll — retail draws the overlay after the map blit).
    fn screen_sprite(&self, id: usize, pos: (f32, f32), scale: f32) -> Option<UiQuad> {
        let (sx, sy, w, h) = self.rects.get(id).copied().flatten()?;
        Some(UiQuad {
            rect: [pos.0 * scale, pos.1 * scale, w * scale, h * scale],
            uv: [sx, sy, w, h],
            tint: [1.0, 1.0, 1.0, 1.0],
        })
    }

    /// FONT1 text at a 640-space position (glyph id = char + 1,
    /// advance = glyph width — retail sub_6F940; glyphs are white
    /// masks, tinted here).
    fn text_quads(&self, s: &str, x: f32, y: f32, color: [f32; 4], scale: f32) -> Vec<UiQuad> {
        let mut out = Vec::new();
        let mut cx = x;
        for c in s.chars() {
            let id = (c as usize).wrapping_add(1);
            let Some((sx, sy, w, h)) = self.font.get(id).copied().flatten() else {
                cx += 4.0; // unknown glyph advances a space
                continue;
            };
            if c != ' ' {
                out.push(UiQuad {
                    rect: [cx * scale, y * scale, w * scale, h * scale],
                    uv: [sx, sy, w, h],
                    tint: color,
                });
            }
            cx += w;
        }
        out
    }

    fn text_width(&self, s: &str) -> f32 {
        s.chars()
            .map(|c| {
                self.font
                    .get((c as usize).wrapping_add(1))
                    .copied()
                    .flatten()
                    .map_or(4.0, |(_, _, w, _)| w)
            })
            .sum()
    }

    /// Word-wrap into a pixel width (retail sub_7FCB0 wraps the
    /// description between its x bounds).
    fn wrap_text(&self, s: &str, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in s.split_whitespace() {
            let cand = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if self.text_width(&cand) > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = cand;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }

    /// Shadowed text (readability over the map art — the retail
    /// bordered draw's role).
    fn shadowed_text(
        &self,
        s: &str,
        x: f32,
        y: f32,
        color: [f32; 4],
        scale: f32,
        quads: &mut Vec<UiQuad>,
    ) {
        quads.extend(self.text_quads(s, x + 1.0, y + 1.0, [0.0, 0.0, 0.0, 0.9], scale));
        quads.extend(self.text_quads(s, x, y, color, scale));
    }

    /// The corner button under a 640-space point (hit-box = the grey
    /// sprite's own dims, MI:2408-9).
    fn button_hit(&self, mx: f32, my: f32) -> Option<MapButton> {
        for (btn, pos, grey, _) in MAP_BUTTONS {
            let Some((_, _, w, h)) = self.rects.get(grey).copied().flatten() else {
                continue;
            };
            if mx >= pos.0 && mx < pos.0 + w && my >= pos.1 && my < pos.1 + h {
                return Some(btn);
            }
        }
        None
    }

    /// A save-label edit field is accepting keystrokes.
    pub fn dialog_editing(&self) -> bool {
        self.dialog.as_ref().is_some_and(|d| d.edit.is_some())
    }

    /// Open a parchment dialog. `slots` = (label, occupied) per save
    /// slot, scanned by the app (retail probes SAVE%d.GAM on entry,
    /// "Empty" for the rest).
    pub fn open_dialog(&mut self, kind: DialogKind, slots: Vec<(String, bool)>) {
        // Retail str_26 anchors: Save (29,60) h 200, Load (510,60)
        // h 200, New Game confirm (37,348) h 60.
        let (anchor, height) = match kind {
            DialogKind::Save => ((29.0, 60.0), 200.0),
            DialogKind::Load => ((510.0, 60.0), 200.0),
            DialogKind::NewGame => ((37.0, 348.0), 60.0),
        };
        self.dialog = Some(Dialog {
            kind,
            anchor,
            height,
            open: 0.0,
            slots,
            selected: None,
            edit: None,
        });
    }

    /// The committed frontend action, once.
    pub fn take_action(&mut self) -> Option<MapAction> {
        self.pending_action.take()
    }

    /// Esc on the map: an open dialog closes — Cancel, with the click
    /// sample (the scroll widget's scancode-1 arm, MI:5660-63 + the
    /// sample at :5686-87); otherwise back to the menu (retail
    /// NewGameDraw returns 2, MI:3430-31).
    /// Put up the stats table for `level` (retail map mode 4, the
    /// entry after a completed level — MI:985).
    /// Several levels queue up in order: each dismissal shows the next.
    pub fn show_stats(&mut self, levels: Vec<u32>) {
        self.stats_next = levels.into();
        self.stats = self.stats_next.pop_front();
    }

    /// Take the table down, bringing up the next queued one. True when
    /// a table was up (the input is consumed).
    fn next_stats(&mut self) -> bool {
        if self.stats.is_none() {
            return false;
        }
        self.stats = self.stats_next.pop_front();
        true
    }

    /// The stats table is up (any key or click takes it down, MI:3217-22).
    pub fn stats_open(&self) -> bool {
        self.stats.is_some()
    }

    pub fn dismiss_stats(&mut self) {
        self.next_stats();
    }

    pub fn escape(&mut self) {
        if self.next_stats() {
            return;
        }
        if self.dialog.take().is_some() {
            self.sounds.push(SND_CLICK);
        } else {
            self.pending_action = Some(MapAction::ExitToMenu);
        }
    }

    /// An open parchment dialog (deciding whether Enter belongs to
    /// it).
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// A keystroke for the save-label editor (retail sub_7F6A0:
    /// space/0-9/letters, max 15).
    pub fn dialog_char(&mut self, c: char) {
        if let Some(d) = &mut self.dialog
            && let Some(edit) = &mut d.edit
            && (c == ' ' || c.is_ascii_alphanumeric())
            && edit.len() < 15
        {
            edit.push(c);
        }
    }

    pub fn dialog_backspace(&mut self) {
        if let Some(d) = &mut self.dialog
            && let Some(edit) = &mut d.edit
        {
            edit.pop();
        }
    }

    /// Enter: an open edit field closes first, committing the label
    /// into the slot row (the actual save happens on OK — retail
    /// law); otherwise Enter is the OK button (the scroll widget's
    /// scancode-28 arm, MI:5656-58).
    pub fn dialog_enter(&mut self) {
        let Some(d) = &mut self.dialog else { return };
        if let Some(edit) = d.edit.take() {
            if let Some(k) = d.selected
                && let Some(slot) = d.slots.get_mut(k)
            {
                slot.0 = edit;
            }
            return;
        }
        self.press_ok();
    }

    /// The OK button's action — one law for the mouse hit and the
    /// Enter key. Ignored while the scroll is still unrolling, like
    /// clicks; Save/Load with nothing valid selected clicks but
    /// stays up.
    fn press_ok(&mut self) {
        let Some(d) = &self.dialog else { return };
        if d.open < d.height {
            return;
        }
        self.sounds.push(SND_CLICK);
        let action = match d.kind {
            DialogKind::NewGame => Some(MapAction::NewGame),
            DialogKind::Save => d.selected.map(|k| MapAction::SaveTo { slot: k }),
            DialogKind::Load => d
                .selected
                .filter(|&k| d.slots[k].1)
                .map(MapAction::LoadFrom),
        };
        if let Some(a) = action {
            self.pending_action = Some(a);
            self.dialog = None;
        }
    }

    /// Route a 640-space click through the open dialog. Returns the
    /// quads-space handled flag.
    fn dialog_click(&mut self, mx: f32, my: f32) -> bool {
        let Some(d) = &mut self.dialog else {
            return false;
        };
        if d.open < d.height {
            return true; // still opening — swallow
        }
        let (x1, y1) = d.anchor;
        // Slot rows (Save/Load): (x1+20, y1+32+16k), hit 90×16
        // (retail :2661-66 — rows are 1-based off the title line).
        if !matches!(d.kind, DialogKind::NewGame) {
            for k in 0..d.slots.len() {
                let ry = y1 + 32.0 + 16.0 * k as f32;
                if mx >= x1 + 10.0 && mx < x1 + 10.0 + 92.0 && my >= ry && my < ry + 16.0 {
                    let occupied = d.slots[k].1;
                    if matches!(d.kind, DialogKind::Load) && !occupied {
                        return true; // only occupied slots load
                    }
                    d.selected = Some(k);
                    // No select-to-edit. Retail copied the slot label
                    // into an edit buffer here, but our slot rows are
                    // COMPOSED for display (player name + level +
                    // progress) — seeding an editor from a rendered row
                    // and writing it back accumulated the suffix on
                    // every save. Slot names are derived, not authored.
                    self.sounds.push(SND_CLICK);
                    return true;
                }
            }
        }
        let (ok_r, ca_r) = dialog_button_rects(d.anchor, d.height, DIALOG_W);
        if in_rect(mx, my, ok_r) {
            self.press_ok();
            return true;
        }
        if in_rect(mx, my, ca_r) {
            self.sounds.push(SND_CLICK);
            self.dialog = None;
            return true;
        }
        true // the open dialog swallows map clicks (retail law)
    }

    /// The overlay's quads: border frame, corner buttons (gold on
    /// hover), the pending level's description, the dialog.
    fn overlay_quads(&mut self, save: &Mc2Save, scale: f32, cursor_640: (f32, f32)) -> Vec<UiQuad> {
        let mut quads = Vec::new();
        // The ornate frame (art only, retail sub_85CC3 every frame).
        if let Some((sx, sy, w, h)) = self.border_rect {
            quads.push(UiQuad {
                rect: [0.0, 0.0, w * scale, h * scale],
                uv: [sx, sy, w, h],
                tint: [1.0, 1.0, 1.0, 1.0],
            });
        }
        // Corner buttons: grey idle, gold under the cursor.
        let hover = self.button_hit(cursor_640.0, cursor_640.1);
        for (btn, pos, grey, gold) in MAP_BUTTONS {
            let id = if hover == Some(btn) { gold } else { grey };
            if let Some(q) = self.screen_sprite(id, pos, scale) {
                quads.push(q);
            }
        }
        // The pending level's description text (retail
        // PresentLevelDescription: strings[23+level] at x 130 w 380,
        // y 280 when the portal sits in the top half of the map,
        // else 60; suppressed with the narrative while the level's
        // secret is revealed — the `narrated` latch already encodes
        // that law).
        if self.narrated && !self.desc_dismissed && self.dialog.is_none() && self.travel.is_none() {
            let level = save.levels_completed as usize;
            if let Some(text) = self.strings.get(23 + level) {
                let portal_y = MC2_MAIN_PORTALS.get(level).map_or(0, |p| p.pos.1);
                let y0 = if portal_y < 478 { 280.0 } else { 60.0 };
                let lh = 9.0;
                for (i, line) in self.wrap_text(text, 372.0).iter().enumerate() {
                    self.shadowed_text(
                        line,
                        134.0,
                        y0 + i as f32 * lh,
                        [1.0, 1.0, 1.0, 1.0],
                        scale,
                        &mut quads,
                    );
                }
            }
        }
        // The parchment scroll dialog — the retail composition
        // (DrawScrollDialog2_7B660 MI:5488-5688): ONE unrolled
        // scroll. The roller-bar sprite (254) draws at the TOP and
        // again at the animated BOTTOM edge; between them a solid
        // parchment fill (palette (0x2A,0x24,0x1D)) with a vertical
        // edge line each side ((0x25,0x1F,0x19)); the title sits in
        // dim ink UNDER the top roller; OK at x1+15 and Cancel
        // right-aligned to x1+barW-12, both resting on the bottom
        // roller. Slot rows start at y1+32 (retail y1+16*(k+1),
        // k = 1-based).
        if let Some(d) = &self.dialog {
            let (x1, y1) = d.anchor;
            let title_id = match d.kind {
                DialogKind::Save => 422,
                DialogKind::Load => 421,
                DialogKind::NewGame => 467,
            };
            let (bar_w, bar_h) = self
                .rects
                .get(254)
                .copied()
                .flatten()
                .map_or((DIALOG_W, 12.0), |(_, _, w, h)| (w, h));
            let parchment = [168.0 / 255.0, 144.0 / 255.0, 116.0 / 255.0, 1.0];
            let edge = [148.0 / 255.0, 124.0 / 255.0, 100.0 / 255.0, 1.0];
            let solid = |r: [f32; 4], tint: [f32; 4]| UiQuad {
                rect: [r[0] * scale, r[1] * scale, r[2] * scale, r[3] * scale],
                uv: [0.0; 4],
                tint,
            };
            if d.open > 0.0 {
                let top = y1 + bar_h - 2.0;
                quads.push(solid([x1 + 10.0, top, bar_w - 22.0, d.open], parchment));
                quads.push(solid([x1 + 10.0, top, 1.0, d.open], edge));
                quads.push(solid([x1 + bar_w - 12.0, top, 1.0, d.open], edge));
            }
            if let Some(q) = self.screen_sprite(254, (x1, y1), scale) {
                quads.push(q);
            }
            if let Some(q) = self.screen_sprite(254, (x1, y1 + d.open), scale) {
                quads.push(q);
            }
            let ink = [88.0 / 255.0, 64.0 / 255.0, 36.0 / 255.0, 1.0];
            let white = [1.0, 1.0, 1.0, 1.0];
            // Title once the scroll has opened past a line height
            // (retail letterHeight+10 gate).
            if d.open > 17.0
                && let Some(title) = self.strings.get(title_id)
            {
                let tx = x1 + 10.0 + (bar_w - 22.0 - self.text_width(title)) / 2.0;
                quads.extend(self.text_quads(title, tx, y1 + bar_h + 2.0, ink, scale));
            }
            if d.open >= d.height {
                if !matches!(d.kind, DialogKind::NewGame) {
                    let caret_on = (self.anim * 4.0) as u32 % 2 == 0;
                    for (k, (label, _occupied)) in d.slots.iter().enumerate() {
                        let ry = y1 + 32.0 + 16.0 * k as f32;
                        let selected = d.selected == Some(k);
                        let shown = if selected && let Some(e) = &d.edit {
                            let mut s = format!("{}. {e}", k + 1);
                            if caret_on {
                                s.push('_');
                            }
                            s
                        } else {
                            format!("{}. {label}", k + 1)
                        };
                        let color = if selected { white } else { ink };
                        quads.extend(self.text_quads(&shown, x1 + 20.0, ry, color, scale));
                    }
                }
                let (mx, my) = cursor_640;
                let (ok_r, ca_r) = dialog_button_rects(d.anchor, d.height, bar_w);
                let ok_h = in_rect(mx, my, ok_r);
                let ca_h = in_rect(mx, my, ca_r);
                if let Some(q) =
                    self.screen_sprite(if ok_h { 258 } else { 257 }, (ok_r.0, ok_r.1), scale)
                {
                    quads.push(q);
                }
                if let Some(q) =
                    self.screen_sprite(if ca_h { 256 } else { 255 }, (ca_r.0, ca_r.1), scale)
                {
                    quads.push(q);
                }
            }
        }
        if let Some(level) = self.stats {
            self.stats_table_quads(save, level, scale, &mut quads);
        }
        quads
    }

    /// `DrawEndGameTable_82C20` (MI:4765-4860): a frame of tiled
    /// sprites — 275 along the top and bottom at y = 50, 274 down the
    /// sides over a darkened interior — with the level name centred on
    /// top. The row values are the save's per-level table
    /// (`sub_82AB0`).
    ///
    /// Retail: 12 side blocks, width = 2 glyph widths per char of the
    /// longest label, Spells / Accuracy / Kills / Mana as `%3d%%` on
    /// one line each, a blank, the time `%02d:%02d:%02d`, over a light
    /// darkening. Deliberate layout (player rulings 2026-09-24), when
    /// the save's stats tail recorded the absolutes:
    /// - Each stat takes ONE line, as MC1's screen composes it: the
    ///   label on the left, "N (P%)" right-aligned on the same line
    ///   (player ruling 2026-09-25 — the panel fits itself to the
    ///   widest line, so the earlier two-line split bought nothing),
    ///   then a half-line gap after the stat's block. Accuracy reads "H hits, M misses (P%)" (bare "P%" before the
    ///   first shot, or from a save that predates the MC2 shot count).
    /// - Kills carries four one-line sub-rows: by you / rivals /
    ///   nature, and still alive. "Mana claimed" carries two: in castle
    ///   (with what is on its way) and in dwellings; "Mana unclaimed"
    ///   closes its block, unindented (player ruling 2026-09-25:
    ///   unclaimed is not a part of what was claimed).
    /// - Percentages carry one decimal, rounded half up with strict
    ///   ends, each breakdown summing to 100.0; Mana leaves out the
    ///   census's uncollectable seed (player ruling 2026-09-25).
    /// - Time Taken sits one line off the bottom.
    /// - The panel is wider, and the interior darker, so the rows read
    ///   over the narration subtitles beneath.
    ///
    /// Without the tail the same layout shows the bare percentages.
    fn stats_table_quads(&self, save: &Mc2Save, level: u32, scale: f32, quads: &mut Vec<UiQuad>) {
        use mgc_sim::engine::stats::fmt10;
        let (row, ext) = if level <= 24 {
            (
                save.main_stats.get(level as usize).copied(),
                save.main_ext.get(level as usize).copied(),
            )
        } else {
            let i = save
                .secrets
                .iter()
                .take_while(|p| p.activated != 0)
                .position(|p| p.level as u32 == level);
            (
                i.and_then(|i| save.secret_stats.get(i).copied()),
                i.and_then(|i| save.secret_ext.get(i).copied()),
            )
        };
        let Some(v) = row else { return };
        // The absolutes (ENHANCED): present only when this build
        // recorded the win.
        let ext = ext.as_ref().and_then(crate::saves::ext_stats);
        let label = |id: usize, fb: &str| {
            self.strings
                .get(id)
                .filter(|s| !s.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| fb.to_string())
        };
        let name = MC2_LEVEL_NAMES
            .get(level as usize)
            .map_or(String::new(), |n| n.trim_end().to_string());
        let np = |n: u32, p: u32| format!("{n} ({}%)", fmt10(p));
        let bare = |p: i32| format!("{p}%");

        // The rows: (label, value, kind). `v` = [spells, accuracy,
        // kills, mana, seconds] — retail's whole percentages, shown
        // only for a row without the tail.
        use RowKind::{Main, Packed, Sub};
        let mut rows: Vec<(String, String, RowKind)> = Vec::with_capacity(12);
        let spells = ext.as_ref().map_or(bare(v[0]), |e| {
            np(e.spells_found_fixed, e.spells_fixed_pct10())
        });
        rows.push((label(386, "Spells found"), spells, Main));
        let accuracy = match &ext {
            Some(e) if e.offensive_shots > 0 => format!(
                "{} hits, {} misses ({}%)",
                e.offensive_hits,
                e.offensive_shots.saturating_sub(e.offensive_hits),
                fmt10(e.offensive_accuracy_pct10())
            ),
            Some(e) => format!("{}%", fmt10(e.offensive_accuracy_pct10())),
            None => bare(v[1]),
        };
        rows.push((label(385, "Accuracy"), accuracy, Main));
        let kills = ext
            .as_ref()
            .map_or(bare(v[2]), |e| np(e.deaths_total(), e.cleared_pct10()));
        rows.push((label(384, "Creatures Killed"), kills, Main));
        if let Some(e) = &ext {
            let k = e.kill_split10();
            for (i, (l, n)) in [
                ("by you", e.deaths_player()),
                ("by rivals", e.deaths_rivals()),
                ("by nature", e.deaths_environment()),
                ("still alive", e.alive),
            ]
            .into_iter()
            .enumerate()
            {
                rows.push((format!("- {l}"), np(n, k[i]), Sub));
            }
        }
        let mana = ext.as_ref().map_or(bare(v[3]), |e| {
            let [c, h, _] = e.mana_rows();
            np(c + h, e.mana_pct10())
        });
        let mana_label = label(377, "Mana");
        match &ext {
            Some(e) => {
                let m = e.mana_split10();
                let n = e.mana_rows();
                rows.push((format!("{mana_label} claimed"), mana, Main));
                rows.push(("- in castle".into(), np(n[0], m[0]), Sub));
                rows.push(("- in dwellings".into(), np(n[1], m[1]), Sub));
                rows.push((format!("{mana_label} unclaimed"), np(n[2], m[2]), Packed));
            }
            None => rows.push((mana_label, mana, Main)),
        }
        let t = v[4].max(0);
        let time = (
            label(394, "Time Taken"),
            format!("{:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60),
        );

        // Retail sizes by `spritestr[65]` — FONT1 glyph 65 ('@').
        let (cw, ch) = self
            .font
            .get(65)
            .copied()
            .flatten()
            .map_or((8.0, 10.0), |(_, _, w, h)| (w, h));
        let (tw, th) = self
            .rects
            .get(275)
            .copied()
            .flatten()
            .map_or((16.0, 16.0), |r| (r.2, r.3));
        let (sw, sh) = self
            .rects
            .get(274)
            .copied()
            .flatten()
            .map_or((16.0, 16.0), |r| (r.2, r.3));
        let lh = ch + 1.0;
        // Width: retail's rule, widened to fit every line with a
        // margin, and never narrower than a comfortable minimum.
        let max_len = rows
            .iter()
            .map(|r| r.0.chars().count())
            .chain([name.chars().count(), time.0.chars().count()])
            .max()
            .unwrap_or(0) as f32;
        let widest = rows
            .iter()
            .map(|(l, val, kind)| {
                let indent = if *kind == Sub { 2.0 * cw } else { 0.0 };
                indent + self.text_width(l) + self.text_width(val) + 4.0 * cw
            })
            .chain([
                self.text_width(&name),
                self.text_width(&time.0) + self.text_width(&time.1) + 4.0 * cw,
            ])
            .fold(0.0f32, f32::max);
        let mut fw = (2.0 * cw * max_len)
            .max(widest + 2.0 * sw + 6.0 * cw)
            .max(240.0);
        if fw % tw != 0.0 {
            fw = ((fw / tw).floor() + 1.0) * tw;
        }
        let begin_x = (320.0 - fw / 2.0).floor();
        let top = 50.0;
        let inner_top = top + th;
        let y0 = 2.0 * sw + 50.0;
        let base = y0 + 5.0 + 2.0;
        // Content: one line per row, a half-line after each stat's
        // block; then a blank line, the time, and one line of margin to
        // the bottom.
        let mut ys = Vec::with_capacity(rows.len());
        let mut y = base + lh;
        for i in 0..rows.len() {
            ys.push(y);
            y += lh;
            let next_packed = rows.get(i + 1).is_some_and(|r| r.2 != Main);
            if !next_packed {
                y += lh / 2.0;
            }
        }
        let blocks = ((y + 2.0 * lh - inner_top) / sh).ceil().max(12.0);
        let inner_h = blocks * sh;
        let time_y = inner_top + inner_h - 2.0 * lh;

        quads.push(crate::ui::solid(
            [
                (begin_x + sw) * scale,
                inner_top * scale,
                (fw - 2.0 * sw) * scale,
                inner_h * scale,
            ],
            [0.0, 0.0, 0.0, 0.85],
        ));
        let mut x = 0.0;
        while x < fw {
            if let Some(q) = self.screen_sprite(275, (begin_x + x, top), scale) {
                quads.push(q);
            }
            if let Some(q) = self.screen_sprite(275, (begin_x + x, inner_top + inner_h), scale) {
                quads.push(q);
            }
            x += tw;
        }
        for b in 0..blocks as usize {
            let y = inner_top + b as f32 * sh;
            if let Some(q) = self.screen_sprite(274, (begin_x, y), scale) {
                quads.push(q);
            }
            if let Some(q) = self.screen_sprite(274, (begin_x + fw - sw, y), scale) {
                quads.push(q);
            }
        }
        let white = [1.0, 1.0, 1.0, 1.0];
        let x3 = cw + begin_x;
        // The title, centred between x3 and x3 + fw − cw (DrawText_7FAE0).
        let span = fw - cw;
        let tx = x3 + (span - self.text_width(&name)) / 2.0;
        quads.extend(self.text_quads(&name, tx, y0, white, scale));
        let left = begin_x + sw + 2.0 * cw;
        let right = begin_x + fw - sw - 2.0 * cw;
        for ((l, val, kind), &y) in rows.iter().zip(&ys) {
            let lx = if *kind == Sub { left + 2.0 * cw } else { left };
            quads.extend(self.text_quads(l, lx, y, white, scale));
            quads.extend(self.text_quads(val, right - self.text_width(val), y, white, scale));
        }
        quads.extend(self.text_quads(&time.0, left, time_y, white, scale));
        quads.extend(self.text_quads(
            &time.1,
            right - self.text_width(&time.1),
            time_y,
            white,
            scale,
        ));
    }

    /// This frame's quads: background crop, ambient dressing, trail
    /// dots, portals, the carpet, the cursor.
    pub fn quads(&mut self, save: &Mc2Save, size: (f32, f32), cursor: (f32, f32)) -> Vec<UiQuad> {
        // Authored in the retail 640x480 screen; the letterbox offset
        // goes on the finished list and the cursor makes the same trip
        // in reverse, so every hit test below stays in screen space.
        let (scale, ox, oy) = crate::ui::letterbox(size, VIEW_W, VIEW_H);
        let cursor = crate::ui::unletterbox(cursor, size, VIEW_W, VIEW_H);
        let mut quads = Vec::new();
        quads.push(UiQuad {
            rect: [0.0, 0.0, VIEW_W * scale, VIEW_H * scale],
            uv: [self.scroll.0, self.scroll.1, VIEW_W, VIEW_H],
            tint: [1.0, 1.0, 1.0, 1.0],
        });
        // The won finale's redrawn island — the spires gone, the
        // coast reshaped — stamped into the map buffer as 12 tiles
        // (`MapMenuPortalsDraw_81760` "end game vulcan", MI:3825-46;
        // the shipped exe agrees). Under the route dots: retail
        // stamps those into the same buffer afterwards.
        if save.levels_completed >= 25 {
            for (id, pos) in VULCAN_ISLAND {
                if let Some(q) = self.sprite(id, pos, scale) {
                    quads.push(q);
                }
            }
        }
        // The dotted route (sprite 139): the FIXED main-line path.
        // Segments between completed portals always draw; the
        // frontier segment draws per `frontier_drawn`; a frontier
        // leg in flight draws its dots up to the carpet.
        if let Some((sx, sy, w, h)) = self.rects.get(139).copied().flatten() {
            let mut dot = |x: f32, y: f32, quads: &mut Vec<UiQuad>| {
                let vx = x - w / 2.0 - self.scroll.0;
                let vy = y - h / 2.0 - self.scroll.1;
                if vx + w < 0.0 || vy + h < 0.0 || vx > VIEW_W || vy > VIEW_H {
                    return;
                }
                quads.push(UiQuad {
                    rect: [vx * scale, vy * scale, w * scale, h * scale],
                    uv: [sx, sy, w, h],
                    tint: [1.0, 1.0, 1.0, 1.0],
                });
            };
            let dots_along =
                |a: (f32, f32),
                 b: (f32, f32),
                 quads: &mut Vec<UiQuad>,
                 dot: &mut dyn FnMut(f32, f32, &mut Vec<UiQuad>)| {
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let len = (dx * dx + dy * dy).sqrt();
                    let n = (len / 12.0) as usize;
                    for k in 1..n {
                        let t = k as f32 / n as f32;
                        dot(a.0 + dx * t, a.1 + dy * t, quads);
                    }
                };
            // Segment j connects portal j → j+1. Connections between
            // completed portals always draw; the frontier connection
            // (into the pending portal) per the flag; finale done =
            // the whole route (retail MapMenuPortalsDraw_81760).
            let c = save.levels_completed as usize;
            let end = if c >= 25 {
                24
            } else if self.frontier_drawn {
                c.min(24)
            } else {
                c.saturating_sub(1)
            };
            for j in 0..end {
                dots_along(
                    self.portal_center(j),
                    self.portal_center(j + 1),
                    &mut quads,
                    &mut dot,
                );
            }
            // The in-flight frontier leg draws itself.
            if let Some(t) = &self.travel
                && t.on_route
            {
                dots_along(self.parked, t.pos, &mut quads, &mut dot);
            }
        }
        // The mana volcano, 12.5 fps (retail steps it every 8 ticks
        // of the 100 Hz clock). Retail stamps it into the map buffer
        // every frame, over the route dots stamped there — so it
        // hides the dots under it — and under the ambients/portals.
        if let Some(t0) = self.vulcan {
            let f = ((self.anim - t0).max(0.0) * ANIM_FPS) as usize % VULCAN_FRAMES;
            if let Some(q) = self.sprite(VULCAN_FIRST + f, VULCAN_POS, scale) {
                quads.push(q);
            }
        }
        // Ambient set dressing (the 85/86 rows vanish at the finale,
        // MI:2810).
        let finale = save.levels_completed >= 25;
        for a in &AMBIENTS {
            if finale && (a.first == 85 || a.first == 86) {
                continue;
            }
            let count = (a.last - a.first).max(1) as f32;
            let id = if a.burst {
                let period = a.delay + count / ANIM_FPS;
                let phase = self.anim % period;
                if phase < a.delay {
                    continue; // waiting bursts are invisible
                }
                a.first as usize + (((phase - a.delay) * ANIM_FPS) as usize).min(count as usize - 1)
            } else {
                // The authored start frame phase-offsets the loops
                // (the meteors fall out of sync).
                let offset = (a.start - a.first) as usize;
                a.first as usize + ((self.anim * ANIM_FPS) as usize + offset) % count as usize
            };
            if let Some(q) = self.sprite(id, a.pos, scale) {
                quads.push(q);
            }
        }
        for p in Self::portals(save) {
            // The pending main portal is not drawn — so does not pop
            // — until the entry glide lands (`index3` only resolves
            // in states 3/5, MI:2818).
            if matches!(p.state, PortalState::Next) && self.glide.is_some() {
                continue;
            }
            if let Some(id) = self.portal_sprite(&p)
                && let Some(q) = self.sprite(id, p.pos, scale)
            {
                quads.push(q);
            }
        }
        // The level-4 falling star — retail draws the event rows after
        // the portals and ambients, before the carpet (MI:3265-66).
        // So are the periodic flights, in table order around it.
        let flight = |i: usize| match self.flights[i] {
            Flight::Flying(t0) => Some(PERIODIC[i].path.flying(self.anim - t0)),
            Flight::Landing(t0) => Some(PERIODIC[i].path.landing(self.anim - t0)),
            Flight::Waiting(_) => None,
        };
        let star = match self.star {
            Star::Falling(t0) => Some(STAR.flying(self.anim - t0)),
            Star::Impact(t0) => Some(STAR.landing(self.anim - t0)),
            _ => None,
        };
        let events: Vec<(usize, (f32, f32))> = (0..PERIODIC_BEFORE_STAR)
            .filter_map(flight)
            .chain(star)
            .chain((PERIODIC_BEFORE_STAR..PERIODIC.len()).filter_map(flight))
            .collect();
        for (id, pos) in events {
            if let Some(q) = self.sprite(id, pos, scale) {
                quads.push(q);
            }
        }
        // The carpet: travelling along a leg (8 heading families of
        // 4 frames, sprites 1-32, family by `carpet_family` from the
        // residual to the target, re-read every frame), or PARKED on
        // the just-played level's portal facing `faces` — during the
        // entry glide retail's draw law resolves no pending portal
        // and the rest pose faces portal 23 instead (MI:3007-3013).
        // Hidden only before the campaign's first launch.
        let carpet = if let Some(t) = &self.travel {
            let (dx, dy) = (t.target.0 - t.pos.0, t.target.1 - t.pos.1);
            Some((Self::carpet_family(dx, dy), t.pos))
        } else if save.levels_completed > 0 || self.parked != (0.0, 0.0) {
            let faces = if self.glide.is_some() {
                self.portal_center(23)
            } else {
                self.faces
            };
            let (dx, dy) = (faces.0 - self.parked.0, faces.1 - self.parked.1);
            Some((Self::carpet_family(dx, dy), self.parked))
        } else {
            None
        };
        if let Some((family, pos)) = carpet {
            let id = family + ((self.anim * CARPET_FPS) as usize) % 4;
            if let Some((sx, sy, w, h)) = self.rects.get(id).copied().flatten() {
                let vx = pos.0 - w / 2.0 - self.scroll.0;
                let vy = pos.1 - h / 2.0 - self.scroll.1;
                quads.push(UiQuad {
                    rect: [vx * scale, vy * scale, w * scale, h * scale],
                    uv: [sx, sy, w, h],
                    tint: [1.0, 1.0, 1.0, 1.0],
                });
            }
        }
        // The frontend overlay: border frame, corner buttons,
        // description text, parchment dialog — over the map, under
        // the cursor (retail draw order).
        quads.extend(self.overlay_quads(save, scale, cursor));
        // Everything so far belongs INSIDE the 640x480 screen. Crop it
        // there: the map scrolls, so portals and dressing routinely
        // hang off the viewport edge, and on a letterboxed window that
        // overhang would otherwise be drawn out across the black bars.
        crate::ui::clip_quads(&mut quads, VIEW_W * scale, VIEW_H * scale);
        // Cursor: the map screen's own bank sprite 239 (MI:986). Drawn
        // AFTER the crop, deliberately — the pointer may sit in a bar
        // (that is how the edge-scroll is reached) and must stay
        // visible there. In screen space like everything else; the
        // translation below puts it back under the real pointer.
        if let Some((sx, sy, w, h)) = self.rects.get(239).copied().flatten() {
            quads.push(UiQuad {
                rect: [cursor.0 * scale, cursor.1 * scale, w * scale, h * scale],
                uv: [sx, sy, w, h],
                tint: [1.0, 1.0, 1.0, 1.0],
            });
        }
        crate::ui::offset_quads(&mut quads, ox, oy);
        quads
    }

    /// A map-bank sprite at a map position (top-left anchored, like
    /// retail's blits), or None while scrolled off.
    fn sprite(&self, id: usize, pos: (f32, f32), scale: f32) -> Option<UiQuad> {
        let (sx, sy, w, h) = self.rects.get(id).copied().flatten()?;
        let x = pos.0 - self.scroll.0;
        let y = pos.1 - self.scroll.1;
        if x + w < 0.0 || y + h < 0.0 || x > VIEW_W || y > VIEW_H {
            return None;
        }
        Some(UiQuad {
            rect: [x * scale, y * scale, w * scale, h * scale],
            uv: [sx, sy, w, h],
            tint: [1.0, 1.0, 1.0, 1.0],
        })
    }

    /// A corner-button click, drained by the app (Save/Load need the
    /// slot scan before the dialog opens; Exit/NewGame route
    /// directly).
    pub fn take_button(&mut self) -> Option<MapButton> {
        self.pending_button.take()
    }

    /// Handle a click: the frontend overlay first (dialog, corner
    /// buttons — retail suppresses map clicks while a dialog pumps,
    /// MI:2387-88), then a hit portal starts a travel leg that
    /// launches on arrival (MI:3330-3405). Returns true when the
    /// click hit anything.
    pub fn click(&mut self, save: &Mc2Save, size: (f32, f32), cursor: (f32, f32)) -> bool {
        let (sx, sy) = crate::ui::unletterbox(cursor, size, VIEW_W, VIEW_H);
        if self.next_stats() {
            return true;
        }
        if self.dialog.is_some() {
            return self.dialog_click(sx, sy);
        }
        if let Some(btn) = self.button_hit(sx, sy) {
            self.sounds.push(SND_CLICK);
            self.pending_button = Some(btn);
            return true;
        }
        if self.travel.is_some() || self.glide.is_some() {
            // One leg at a time; and no portal clicks until the entry
            // glide lands (retail state 2 handles none, MI:3330-31).
            return false;
        }
        let mx = sx + self.scroll.0;
        let my = sy + self.scroll.1;
        let hit = MC2_PORTAL_HIT as f32;
        for p in Self::portals(save) {
            if mx >= p.pos.0 && mx < p.pos.0 + hit && my >= p.pos.1 && my < p.pos.1 + hit {
                // The leg departs from wherever the carpet rests.
                let from = self.parked;
                let target = (p.pos.0 + hit / 2.0, p.pos.1 + hit / 2.0);
                println!(
                    "world map: {} level {}",
                    if matches!(p.state, PortalState::Flag | PortalState::SecretDone) {
                        "replaying"
                    } else {
                        "flying to"
                    },
                    p.level
                );
                // The travel sample plays with the click, but only
                // when the flyer actually goes somewhere (retail
                // gates it on leg length, MI:3786; deliberate:
                // immediate, not retail's late start).
                if (target.0 - from.0).abs() > 8.0 || (target.1 - from.1).abs() > 8.0 {
                    self.sounds.push(SND_TRAVEL);
                }
                // The canonical frontier leg — the only flight that
                // draws route dots: departing the LAST COMPLETED
                // portal for the PENDING one. Off-route trips (any
                // other origin or destination) draw nothing; the
                // segment then appears on the next map entry.
                let completed = save.levels_completed;
                let on_route = completed > 0 && p.level == completed && {
                    let canon = self.portal_center(completed as usize - 1);
                    (from.0 - canon.0).abs() < 1.0 && (from.1 - canon.1).abs() < 1.0
                };
                self.travel = Some(Travel {
                    pos: from,
                    target,
                    launch: Some(p.level),
                    on_route,
                });
                // A committed portal trip retires the description
                // text for this visit.
                self.desc_dismissed = true;
                return true;
            }
        }
        false
    }

    /// A right click: retail map mode 5 (MI:3409-50). Over a
    /// completed main portal (`activated_18 == 1`) or a completed
    /// secret one (`activated_12 == 1`) it puts up that level's stats
    /// table: the main portal's index, the secret's level number. A
    /// table already up comes down, as any button does (MI:3217-22).
    /// Nothing travels, and nothing answers while a dialog pumps or
    /// the carpet is moving. Returns true when the click hit anything.
    pub fn right_click(&mut self, save: &Mc2Save, size: (f32, f32), cursor: (f32, f32)) -> bool {
        let (sx, sy) = crate::ui::unletterbox(cursor, size, VIEW_W, VIEW_H);
        if self.next_stats() {
            return true;
        }
        if self.dialog.is_some() || self.travel.is_some() || self.glide.is_some() {
            return false;
        }
        let (mx, my) = (sx + self.scroll.0, sy + self.scroll.1);
        let hit = MC2_PORTAL_HIT as f32;
        let done = Self::portals(save).into_iter().find(|p| {
            matches!(p.state, PortalState::Flag | PortalState::SecretDone)
                && mx >= p.pos.0
                && mx < p.pos.0 + hit
                && my >= p.pos.1
                && my < p.pos.1 + hit
        });
        match done {
            Some(p) => {
                self.show_stats(vec![p.level]);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save_with(completed: u32) -> Mc2Save {
        Mc2Save {
            levels_completed: completed,
            ..Default::default()
        }
    }

    /// A secret level's parent queues ahead of it: the map shows the
    /// parent's table first, then the secret's, one dismissal each.
    #[test]
    fn stats_tables_queue_in_order() {
        let mut wm = bare();
        wm.show_stats(vec![4, 30]);
        assert_eq!(wm.stats, Some(4), "the parent's table first");
        wm.dismiss_stats();
        assert_eq!(wm.stats, Some(30), "then the secret level's");
        assert!(wm.stats_open());
        wm.escape();
        assert!(!wm.stats_open(), "the queue is spent");
        wm.show_stats(Vec::new());
        assert!(!wm.stats_open(), "an empty queue shows nothing");
    }

    fn bare() -> WorldMap {
        WorldMap {
            atlas: Vec::new(),
            atlas_w: 1,
            atlas_h: 1,
            rects: Vec::new(),
            border_rect: None,
            font: Vec::new(),
            strings: Vec::new(),
            dialog: None,
            stats: None,
            stats_next: Default::default(),
            pending_action: None,
            pending_button: None,
            desc_dismissed: false,
            scroll: (0.0, 0.0),
            anim: 0.0,
            pop: HashMap::new(),
            travel: None,
            pending_launch: None,
            sounds: Vec::new(),
            narrated: false,
            pending_narrative: None,
            parked: (0.0, 0.0),
            last_seen_completed: None,
            frontier_drawn: false,
            edge_step: 0.0,
            glide: None,
            faces: (0.0, 0.0),
            vulcan: None,
            star: Star::Armed,
            flights: [Flight::Waiting(0.0); 9],
        }
    }

    #[test]
    fn portal_states_follow_the_record() {
        let mut save = save_with(3);
        save.secrets[0].activated = 2; // secret 30 revealed
        let portals = WorldMap::portals(&save);
        // Mains 0-2 completed + next (3) + the revealed secret.
        assert_eq!(portals.len(), 5);
        assert!(matches!(portals[0].state, PortalState::Flag));
        assert!(matches!(portals[3].state, PortalState::Next));
        assert_eq!(portals[3].level, 3);
        assert_eq!(portals[4].level, 30);
        assert!(matches!(portals[4].state, PortalState::SecretRevealed));
    }

    /// Past the finale (25 portals opened) the map is the free-play
    /// hub: every main portal is a conquered flag, none is "next", and
    /// each stays clickable for a replay — retail never ends the MC2
    /// campaign (the finale's exit is the ordinary checkpoint `break`,
    /// EF:31525-31531).
    #[test]
    fn finished_campaign_keeps_every_portal_as_a_flag() {
        let save = save_with(25);
        let portals = WorldMap::portals(&save);
        assert_eq!(portals.len(), 25);
        for (i, p) in portals.iter().enumerate() {
            assert_eq!(p.level, i as u32);
            assert!(matches!(p.state, PortalState::Flag), "portal {i} is a flag");
        }
    }

    #[test]
    fn click_starts_travel_then_launches_on_arrival() {
        let mut wm = bare();
        let save = save_with(0);
        wm.scroll = (400.0, 800.0);
        let size = (1280.0, 960.0); // 2× scale
        // Portal 0 sits at (420, 820): cursor over it in window px.
        wm.set_parked(3); // resting on portal 3's flag
        assert!(wm.click(&save, size, (50.0, 50.0)), "portal 0 hit");
        assert!(wm.travel.is_some(), "click starts the carpet leg");
        assert_eq!(
            wm.take_sounds(),
            vec![SND_TRAVEL],
            "the travel sample plays with the click on a real leg"
        );
        // Run the leg to arrival — the launch arms and the carpet
        // parks on the clicked portal.
        for _ in 0..600 {
            wm.tick(1.0 / 60.0, &save);
        }
        assert_eq!(wm.take_launch(), Some(0));
        assert!(wm.travel.is_none());
        // A click out in the sea hits nothing.
        assert!(!wm.click(&save, size, (600.0, 100.0)));
    }

    /// Retail map mode 5: a right click on a COMPLETED portal shows its
    /// table and never travels; the pending portal answers nothing.
    #[test]
    fn right_click_on_a_completed_portal_shows_its_table() {
        let mut wm = bare();
        let size = (1280.0, 960.0); // 2× scale
        wm.scroll = (400.0, 800.0); // portal 0 (420, 820) under (50, 50)
        let pending = save_with(0);
        assert!(
            !wm.right_click(&pending, size, (50.0, 50.0)),
            "the pending portal"
        );
        assert!(!wm.stats_open());
        let save = save_with(3);
        assert!(
            wm.right_click(&save, size, (50.0, 50.0)),
            "portal 0 is completed"
        );
        assert_eq!(wm.stats, Some(0), "its own table");
        assert!(wm.travel.is_none(), "a right click never flies");
        assert!(
            wm.right_click(&save, size, (600.0, 100.0)),
            "any button takes it down"
        );
        assert!(!wm.stats_open());
        assert!(
            !wm.right_click(&save, size, (600.0, 100.0)),
            "the sea hits nothing"
        );
    }

    #[test]
    fn new_portal_pops_on_first_sight_with_sound() {
        let mut wm = bare();
        let p = Portal {
            level: 4,
            pos: (450.0, 652.0),
            state: PortalState::Next,
        };
        // First sight starts the pop-in (sound 41, frame 70);
        // subsequent frames advance it; past 83 it idles open.
        assert_eq!(wm.portal_sprite(&p), Some(70));
        assert!(wm.take_sounds().contains(&SND_PORTAL_OPEN));
        wm.anim = 2.0; // > 14 frames / 12.5 fps
        assert_eq!(wm.portal_sprite(&p), Some(33), "settles into the open idle");
        assert!(matches!(wm.pop.get(&4), Some(Pop::Done)));
        // A revisit never replays the pop (session latch).
        wm.anim = 2.1;
        let idle = wm.portal_sprite(&p).unwrap();
        assert!((33..=35).contains(&idle));
        assert!(wm.take_sounds().is_empty());
    }

    /// Nothing the map draws may escape its 640x480 screen. On a
    /// letterboxed window the overhang would land in the black bars —
    /// the map scrolls, so portals and dressing sit half off the edge
    /// as a matter of course (player-reported: "visuals overlaid over
    /// the map run out into the empty borders").
    #[test]
    fn map_content_stays_inside_the_screen() {
        let save = save_with(6);
        let mut wm = bare();
        // The bare fixture has no sprite bank, so nothing would draw:
        // give every id a 24x24 cell. Portals then land all over the
        // map and plenty of them straddle the viewport edge.
        wm.rects = (0..320).map(|_| Some((0.0, 0.0, 24.0, 24.0))).collect();
        wm.border_rect = Some((0.0, 0.0, VIEW_W, VIEW_H));
        wm.enter_visit(&save);
        // Scrolled to the middle, so content hangs off every edge.
        wm.scroll = (300.0, 240.0);
        for size in [(1600.0, 900.0), (800.0, 900.0), (1280.0, 960.0)] {
            let (scale, ox, oy) = crate::ui::letterbox(size, VIEW_W, VIEW_H);
            let quads = wm.quads(&save, size, (size.0 / 2.0, size.1 / 2.0));
            assert!(quads.len() > 5, "no map content at {size:?}");
            // The cursor sprite is deliberately exempt (it may sit in a
            // bar to reach the edge-scroll), so skip the last quad.
            for q in &quads[..quads.len() - 1] {
                let (x, y) = (q.rect[0] - ox, q.rect[1] - oy);
                assert!(
                    x >= -0.01
                        && y >= -0.01
                        && x + q.rect[2] <= VIEW_W * scale + 0.01
                        && y + q.rect[3] <= VIEW_H * scale + 0.01,
                    "quad {:?} escapes the {}x{} screen at window {size:?}",
                    q.rect,
                    VIEW_W * scale,
                    VIEW_H * scale
                );
            }
        }
    }

    /// Clicks must land on the same portal whatever shape the window
    /// is. The screen is authored at 640x480 and letterboxed into the
    /// window, so a hit test that divides raw window pixels by the
    /// scale is off by the bar width — everything the player aims at
    /// sits to one side of where they clicked. Fullscreen is the
    /// default, so a non-4:3 window is the NORMAL case, not the edge
    /// one; the other tests here all use an exact 4:3 size and cannot
    /// see this.
    #[test]
    fn clicks_hit_the_same_portal_in_a_letterboxed_window() {
        let save = save_with(6);
        // Portal 6 at (763,652) in map space, scrolled into view.
        let scroll = (600.0, 500.0);
        let screen = (763.0 + 10.0 - scroll.0, 652.0 + 10.0 - scroll.1);
        for size in [
            (1280.0, 960.0), // exact 4:3 — no bars
            (1600.0, 900.0), // wide — bars left/right
            (800.0, 900.0),  // squashed — bars top/bottom
        ] {
            let mut wm = bare();
            wm.enter_visit(&save_with(5));
            wm.enter_visit(&save);
            wm.set_parked(5);
            wm.scroll = scroll;
            // Where that screen point actually lands in the window.
            let (scale, ox, oy) = crate::ui::letterbox(size, VIEW_W, VIEW_H);
            let cur = (ox + screen.0 * scale, oy + screen.1 * scale);
            assert!(
                wm.click(&save, size, cur),
                "portal missed at window size {size:?} (bars {ox}x{oy})"
            );
            assert!(wm.travel.is_some(), "no travel started at {size:?}");
        }
    }

    #[test]
    fn route_law_frontier_segment_waits_for_the_carpet() {
        let mut wm = bare();
        // A load (first sight this session): the full trail.
        wm.enter_visit(&save_with(5));
        assert!(wm.frontier_drawn, "load shows the whole route");
        // The frontier advances (a completion led here): the new
        // segment waits.
        wm.enter_visit(&save_with(6));
        assert!(!wm.frontier_drawn, "fresh segment blank until flown");
        // Flying the canonical leg (parked on the last flag, click
        // the pending portal) draws it on arrival.
        wm.set_parked(5);
        let save = save_with(6);
        let size = (1280.0, 960.0);
        // Portal 6 at (763,652): scroll so it's under the cursor.
        wm.scroll = (600.0, 500.0);
        let cur = ((763.0 + 10.0 - 600.0) * 2.0, (652.0 + 10.0 - 500.0) * 2.0);
        assert!(wm.click(&save, size, cur), "pending portal hit");
        assert!(wm.travel.as_ref().unwrap().on_route);
        for _ in 0..900 {
            wm.tick(1.0 / 60.0, &save);
        }
        assert!(wm.frontier_drawn, "flown frontier leg stamps its segment");
        // Re-entry at the same frontier keeps it stamped.
        wm.enter_visit(&save);
        assert!(wm.frontier_drawn);
        // An off-route trip never arms route drawing: park elsewhere,
        // click the pending portal — leg flies, no route dots.
        let mut wm2 = bare();
        wm2.enter_visit(&save_with(5));
        wm2.enter_visit(&save_with(6));
        wm2.set_parked(2); // replayed an old level — off the canon origin
        wm2.scroll = (600.0, 500.0);
        assert!(wm2.click(&save, size, cur));
        assert!(!wm2.travel.as_ref().unwrap().on_route);
        for _ in 0..900 {
            wm2.tick(1.0 / 60.0, &save);
        }
        assert!(!wm2.frontier_drawn, "off-route flight draws nothing");
    }

    /// The heading ladder in retail's own units: the flight sprite
    /// faces the portal it flies to, not away from it.
    #[test]
    fn carpet_faces_its_target() {
        // (dx, dy) = target − carpet, y down.
        assert_eq!(WorldMap::carpet_family(0.0, 40.0), 17, "below");
        assert_eq!(WorldMap::carpet_family(-40.0, 0.0), 9, "left");
        assert_eq!(WorldMap::carpet_family(0.0, -40.0), 1, "above");
        assert_eq!(WorldMap::carpet_family(40.0, 0.0), 25, "right");
        assert_eq!(WorldMap::carpet_family(-30.0, 30.0), 5, "below-left");
        assert_eq!(WorldMap::carpet_family(-30.0, -30.0), 13, "above-left");
        assert_eq!(WorldMap::carpet_family(30.0, -30.0), 21, "above-right");
        assert_eq!(WorldMap::carpet_family(30.0, 30.0), 29, "below-right");
        // Retail's open quadrants: a shallow leg is still diagonal…
        assert_eq!(WorldMap::carpet_family(246.0, -15.0), 21);
        // …until its minor residual rounds away.
        assert_eq!(WorldMap::carpet_family(8.0, -0.4), 25);
        assert_eq!(WorldMap::carpet_family(0.0, 0.0), 17);
    }

    /// A completion glides the camera from the last flag's anchor to
    /// the pending portal's; the pending portal waits (no pop) and
    /// clicks are refused until it lands; the carpet then faces it.
    #[test]
    fn completion_glides_the_camera_to_the_pending_portal() {
        let mut wm = bare();
        let save = save_with(3); // levels 0-2 done, 3 pending
        wm.set_parked(2);
        wm.anchor_to(&save, Some(2));
        assert_eq!(wm.scroll, (576.0, 478.0), "starts on portal 2's anchor");
        assert_eq!(wm.glide, Some((260.0, 402.0)), "walks to portal 3's");
        // Nothing pops and no leg starts while walking.
        let size = (640.0, 480.0);
        let _ = wm.quads(&save, size, (0.0, 0.0));
        assert!(!wm.pop.contains_key(&3), "pending portal hidden mid-glide");
        let screen = (549.0 + 10.0 - wm.scroll.0, 626.0 + 10.0 - wm.scroll.1);
        assert!(!wm.click(&save, size, screen), "clicks wait for the glide");
        let before = wm.scroll;
        wm.tick(1.0 / 70.0, &save);
        // One frame = 4 px on the major axis (x: 576 → 260).
        assert!((wm.scroll.0 - (before.0 - 4.0)).abs() < 1e-3);
        assert!(wm.scroll.1 < before.1 && wm.scroll.1 > before.1 - 4.0);
        for _ in 0..200 {
            wm.tick(1.0 / 70.0, &save);
        }
        assert_eq!(wm.scroll, (260.0, 402.0));
        assert!(wm.glide.is_none());
        assert_eq!(
            wm.faces,
            wm.portal_center(3),
            "rests facing the pending portal"
        );
        // Landed: the portal pops and is clickable.
        let _ = wm.quads(&save, size, (0.0, 0.0));
        assert!(wm.pop.contains_key(&3));
        let screen = (549.0 + 10.0 - wm.scroll.0, 626.0 + 10.0 - wm.scroll.1);
        assert!(wm.click(&save, size, screen));
    }

    /// A failure (or an off-route replay) snaps to the played level's
    /// anchor with no glide; the carpet faces the newest flag. A
    /// secret snaps to its parent's anchor facing the parent.
    #[test]
    fn failure_snaps_to_the_played_level() {
        let mut wm = bare();
        let save = save_with(3);
        wm.anchor_to(&save, Some(3)); // failed the pending level 3
        assert_eq!(wm.scroll, (260.0, 402.0));
        assert!(wm.glide.is_none());
        assert_eq!(wm.faces, wm.portal_center(2));
        wm.anchor_to(&save, Some(0)); // replayed level 0
        assert_eq!(wm.scroll, (116.0, 478.0));
        assert!(wm.glide.is_none());
        wm.anchor_to(&save, Some(30)); // secret 30 (parent 4)
        assert_eq!(wm.scroll, (260.0, 402.0));
        assert_eq!(wm.faces, wm.portal_center(4));
        // Boot/load with nothing completed: portal 0's anchor, no glide.
        wm.anchor_to(&save_with(0), None);
        assert_eq!(wm.scroll, (116.0, 478.0));
        assert!(wm.glide.is_none());
        // A load with progress glides like a completion.
        wm.anchor_to(&save, None);
        assert_eq!(wm.scroll, (576.0, 478.0));
        assert_eq!(wm.glide, Some((260.0, 402.0)));
    }

    /// The camera stays put while the carpet flies — the flight
    /// happens in the viewport the click was made in.
    #[test]
    fn flight_does_not_move_the_camera() {
        let mut wm = bare();
        let save = save_with(0);
        wm.scroll = (400.0, 800.0);
        wm.set_parked(3); // far off-screen from portal 0's view
        assert!(wm.click(&save, (1280.0, 960.0), (50.0, 50.0)));
        for _ in 0..30 {
            wm.tick(1.0 / 60.0, &save);
            assert_eq!(wm.scroll, (400.0, 800.0));
        }
    }

    #[test]
    fn parked_carpet_follows_the_played_level() {
        let mut wm = bare();
        wm.set_parked(30); // a failed secret parks on ITS portal
        assert_eq!(wm.parked, (287.0 + 20.0, 656.0 + 20.0));
        wm.set_parked(0);
        assert_eq!(wm.parked, (430.0, 830.0)); // fallback flag half = 10
    }

    #[test]
    fn narrative_fires_after_pop_unless_secret_pending() {
        let mut wm = bare();
        let save = save_with(4);
        // Materialize portal 4 (no travel in a bare map), run the
        // pop-in through, then the briefing fires once.
        wm.pop.insert(4, Pop::Popping { started: 0.0 });
        for _ in 0..300 {
            wm.tick(1.0 / 60.0, &save);
            // portal_sprite would advance the pop; emulate its
            // completion the way the draw path does.
            if wm.anim > 1.5 {
                wm.pop.insert(4, Pop::Done);
            }
        }
        assert_eq!(wm.take_narrative(), Some(4));
        assert_eq!(wm.take_narrative(), None, "once per visit");
        // A revealed secret attached to the pending level suppresses
        // it (retail MI:3583-89) — until the next visit after it
        // resolves.
        let mut wm = bare();
        let mut save = save_with(4);
        save.secrets[0].activated = 2; // secret 30, parent 4
        wm.pop.insert(4, Pop::Done);
        for _ in 0..60 {
            wm.tick(1.0 / 60.0, &save);
        }
        assert_eq!(wm.take_narrative(), None, "pending secret suppresses");
    }

    /// Give a bare map the corner-button + dialog sprites so the
    /// overlay hit paths run (dims match the real bank).
    fn with_overlay_rects(mut wm: WorldMap) -> WorldMap {
        wm.rects = vec![None; 260];
        for id in 246..=253 {
            wm.rects[id] = Some((0.0, 0.0, 60.0, 54.0));
        }
        wm.rects[254] = Some((0.0, 0.0, 114.0, 12.0));
        for id in 255..=258 {
            wm.rects[id] = Some((0.0, 0.0, 40.0, 29.0));
        }
        wm
    }

    #[test]
    fn corner_buttons_swallow_map_clicks_and_report() {
        let mut wm = with_overlay_rects(bare());
        let save = save_with(3);
        let size = (1280.0, 960.0); // 2× scale
        // Top-left = the Save button (0,0)+60×54 in 640-space.
        assert!(wm.click(&save, size, (10.0, 10.0)));
        assert_eq!(wm.take_button(), Some(MapButton::Save));
        assert!(wm.take_sounds().contains(&SND_CLICK));
        // Top-right = Load (581,0).
        assert!(wm.click(&save, size, (600.0 * 2.0, 10.0)));
        assert_eq!(wm.take_button(), Some(MapButton::Load));
        // Bottom corners: New Game left, Exit right.
        assert!(wm.click(&save, size, (10.0, 440.0 * 2.0)));
        assert_eq!(wm.take_button(), Some(MapButton::NewGame));
        assert!(wm.click(&save, size, (600.0 * 2.0, 440.0 * 2.0)));
        assert_eq!(wm.take_button(), Some(MapButton::Exit));
    }

    #[test]
    fn save_dialog_select_edit_commit() {
        let mut wm = with_overlay_rects(bare());
        let save = save_with(3);
        let size = (1280.0, 960.0);
        wm.open_dialog(
            DialogKind::Save,
            vec![("OLD".into(), true), ("Empty".into(), false)],
        );
        // Still opening: clicks swallowed, nothing selected.
        assert!(wm.click(&save, size, (100.0, 200.0)));
        assert!(wm.dialog.as_ref().unwrap().selected.is_none());
        // Open it fully, then click slot row 2 (a fresh slot).
        for _ in 0..40 {
            wm.tick(1.0 / 60.0, &save);
        }
        // Row k=1 at (29+20, 60+32+16) → 640-space (55, 110) → ×2.
        assert!(wm.click(&save, size, (110.0, 224.0)));
        assert_eq!(wm.dialog.as_ref().unwrap().selected, Some(1));
        // Picking a slot does NOT open a label editor: slot names are
        // derived (player name + level + progress), not authored.
        assert!(!wm.dialog_editing());
        // OK ((29+15, 60+200-28) → (44,232) 640-space): the action,
        // carrying the slot and nothing else.
        assert!(wm.click(&save, size, (46.0 * 2.0, 240.0 * 2.0)));
        assert_eq!(wm.take_action(), Some(MapAction::SaveTo { slot: 1 }));
        assert!(wm.dialog.is_none());
    }

    #[test]
    fn load_dialog_only_occupied_slots() {
        let mut wm = with_overlay_rects(bare());
        let save = save_with(3);
        let size = (1280.0, 960.0);
        wm.open_dialog(
            DialogKind::Load,
            vec![("GAME".into(), true), ("Empty".into(), false)],
        );
        for _ in 0..40 {
            wm.tick(1.0 / 60.0, &save);
        }
        // Row 2 (empty): not selectable. Load anchor x=510: row 1 at
        // (530, 108) 640-space.
        assert!(wm.click(&save, size, (1070.0, 224.0)));
        assert!(wm.dialog.as_ref().unwrap().selected.is_none());
        // Row 1 (occupied, y 92..108) selects; OK commits.
        assert!(wm.click(&save, size, (1070.0, 192.0)));
        assert_eq!(wm.dialog.as_ref().unwrap().selected, Some(0));
        assert!(wm.click(&save, size, ((510.0 + 16.0) * 2.0, 240.0 * 2.0)));
        assert_eq!(wm.take_action(), Some(MapAction::LoadFrom(0)));
    }

    #[test]
    fn escape_closes_dialog_then_exits_to_menu() {
        let mut wm = with_overlay_rects(bare());
        wm.open_dialog(DialogKind::NewGame, Vec::new());
        wm.escape();
        assert!(wm.dialog.is_none());
        assert_eq!(wm.take_action(), None);
        wm.escape();
        assert_eq!(wm.take_action(), Some(MapAction::ExitToMenu));
    }

    /// Enter = the OK button (the scroll widget's scancode-28 arm):
    /// swallowed while the parchment is still unrolling, then the
    /// dialog's action — including the selection law on slot
    /// dialogs.
    #[test]
    fn enter_confirms_the_open_dialog() {
        let mut wm = with_overlay_rects(bare());
        let save = save_with(3);
        wm.open_dialog(DialogKind::NewGame, Vec::new());
        wm.dialog_enter();
        assert!(wm.dialog.is_some(), "unrolling dialog swallows Enter");
        assert_eq!(wm.take_action(), None);
        for _ in 0..40 {
            wm.tick(1.0 / 60.0, &save);
        }
        wm.dialog_enter();
        assert_eq!(wm.take_action(), Some(MapAction::NewGame));
        assert!(wm.dialog.is_none());

        // A load dialog holds until a valid slot is selected.
        wm.open_dialog(DialogKind::Load, vec![("GAME".into(), true)]);
        for _ in 0..40 {
            wm.tick(1.0 / 60.0, &save);
        }
        wm.dialog_enter();
        assert!(wm.dialog.is_some(), "no selection: OK clicks, stays up");
        assert_eq!(wm.take_action(), None);
        wm.dialog.as_mut().unwrap().selected = Some(0);
        wm.dialog_enter();
        assert_eq!(wm.take_action(), Some(MapAction::LoadFrom(0)));
    }

    #[test]
    fn session_reset_forgets_presentation_state() {
        let mut wm = bare();
        wm.pop.insert(4, Pop::Done);
        wm.set_parked(3);
        wm.enter_visit(&save_with(5));
        wm.session_reset();
        assert!(wm.pop.is_empty());
        assert_eq!(wm.parked, (0.0, 0.0));
        // Next entry reads as a fresh session: full trail.
        wm.enter_visit(&save_with(5));
        assert!(wm.frontier_drawn);
    }

    /// THE PERIODIC FLIGHTS: each row waits its `wait`, launches
    /// (sample), flies its path, arrives (sample), plays its arrival
    /// frames and starts over; all draw after the portals, the dragons
    /// before the level-4 star and the falling stars after it.
    #[test]
    fn the_map_dragons_and_falling_stars_cycle() {
        let mut wm = bare();
        wm.rects = (0..320)
            .map(|i| Some((i as f32, 0.0, 24.0, 24.0)))
            .collect();
        let save = save_with(3);
        // The shortest row: dragon3 (row 4), every 21 s.
        let row = &PERIODIC[4];
        wm.tick(row.wait - 0.5, &save);
        assert_eq!(wm.flights[4], Flight::Waiting(0.0));
        wm.take_sounds();
        wm.tick(0.6, &save);
        assert!(matches!(wm.flights[4], Flight::Flying(_)));
        assert!(wm.take_sounds().contains(&row.path.launch));
        wm.tick(row.path.fly_time() + 0.01, &save);
        assert!(matches!(wm.flights[4], Flight::Landing(_)));
        wm.tick(row.path.hit_time() + 0.01, &save);
        assert!(matches!(wm.flights[4], Flight::Waiting(_)), "starts over");
        // Mid-flight it moves along its path, a flight frame.
        let (id, a) = row.path.flying(0.2);
        let (_, b) = row.path.flying(0.4);
        assert!((146..=151).contains(&id));
        assert!(b.0 > a.0 && b.1 > a.1);
        // Draw order: a dragon (row 0) and a falling star (row 5) in
        // flight together with the level-4 star.
        wm.flights[0] = Flight::Flying(wm.anim);
        wm.flights[5] = Flight::Flying(wm.anim);
        wm.star = Star::Falling(wm.anim);
        wm.scroll = (300.0, 0.0);
        let ids: Vec<usize> = wm
            .quads(&save, (640.0, 480.0), (0.0, 0.0))
            .iter()
            .skip(1)
            .map(|q| q.uv[0] as usize)
            .collect();
        let at =
            |r: std::ops::RangeInclusive<usize>| ids.iter().position(|i| r.contains(i)).unwrap();
        assert!(at(140..=145) < at(277..=284), "dragons before the star");
        assert!(at(277..=284) < at(107..=114), "falling stars after it");
    }

    /// THE LEVEL-4 FALLING STAR: with portal 3 pending (and the stats
    /// table gone) a star falls from the top edge onto the portal
    /// (frames 277-284, launch sample 9), bursts there (93-103, impact
    /// sample 3) and is gone; it does not fall again on the same run
    /// until portal 3 stops being pending; other records never see it.
    #[test]
    fn the_meteor_level_star_falls_onto_portal_three() {
        let mut wm = bare();
        wm.rects = (0..320)
            .map(|i| Some((i as f32, 0.0, 24.0, 24.0)))
            .collect();
        wm.scroll = (300.0, 300.0);
        let star_ids = |wm: &mut WorldMap, save: &Mc2Save| -> Vec<usize> {
            let quads = wm.quads(save, (640.0, 480.0), (0.0, 0.0));
            quads[1..]
                .iter()
                .map(|q| q.uv[0] as usize)
                .filter(|id| (277..=284).contains(id) || (93..=103).contains(id))
                .collect()
        };
        let other = save_with(2);
        wm.tick(0.1, &other);
        assert_eq!(wm.star, Star::Armed);
        assert!(!wm.take_sounds().contains(&SND_STAR_LAUNCH));

        let pending3 = save_with(3);
        wm.stats = Some(2);
        wm.tick(0.1, &pending3);
        assert_eq!(wm.star, Star::Armed, "waits for the stats table");
        wm.stats = None;
        wm.tick(0.01, &pending3);
        assert!(matches!(wm.star, Star::Falling(_)));
        assert!(wm.take_sounds().contains(&SND_STAR_LAUNCH));
        // Late in the fall it is in view, a flight frame.
        wm.tick(Star::fall_time() * 0.9, &pending3);
        let ids = star_ids(&mut wm, &pending3);
        assert_eq!(ids.len(), 1);
        assert!((277..=284).contains(&ids[0]));
        // Landed: the impact, on the portal.
        wm.tick(Star::fall_time() * 0.2, &pending3);
        assert!(matches!(wm.star, Star::Impact(_)));
        assert!(wm.take_sounds().contains(&SND_STAR_HIT));
        assert!((93..=103).contains(&star_ids(&mut wm, &pending3)[0]));
        // Burst over: gone, and it stays gone on this run.
        wm.tick(STAR.hit_time() + 0.05, &pending3);
        assert_eq!(wm.star, Star::Done);
        assert!(star_ids(&mut wm, &pending3).is_empty());
        wm.tick(5.0, &pending3);
        assert_eq!(wm.star, Star::Done, "once per run");
        // Re-arms only once portal 3 stops being pending.
        wm.tick(0.1, &save_with(4));
        assert_eq!(wm.star, Star::Armed);
    }

    /// The won campaign redraws Vissuluth's island and raises the mana
    /// volcano on it: the 12 island tiles straight over the
    /// background, then the route dots, then the looping volcano
    /// (297-304, hiding the dots under it), then the dressing; the
    /// start sample fires once, not per visit; an unfinished record
    /// shows none of it.
    #[test]
    fn finale_raises_the_mana_volcano() {
        let mut wm = bare();
        // Sprite id = atlas x, so a quad's uv names its sprite.
        wm.rects = (0..320)
            .map(|i| Some((i as f32, 0.0, 24.0, 24.0)))
            .collect();
        wm.scroll = (300.0, 0.0);
        let ids = |wm: &mut WorldMap, save: &Mc2Save| -> Vec<usize> {
            let quads = wm.quads(save, (640.0, 480.0), (0.0, 0.0));
            // Index 0 is the background.
            quads[1..].iter().map(|q| q.uv[0] as usize).collect()
        };
        let unfinished = save_with(24);
        wm.tick(0.1, &unfinished);
        assert!(wm.vulcan.is_none());
        assert!(!wm.take_sounds().contains(&SND_VULCAN));
        let drawn = ids(&mut wm, &unfinished);
        assert!(!drawn.iter().any(|id| (285..=304).contains(id)));

        let won = save_with(25);
        wm.tick(0.1, &won);
        assert_eq!(
            wm.take_sounds()
                .iter()
                .filter(|&&s| s == SND_VULCAN)
                .count(),
            1
        );
        let drawn = ids(&mut wm, &won);
        assert_eq!(drawn[..12], (285..=296).collect::<Vec<_>>(), "island first");
        let volcano = drawn.iter().position(|&id| id == VULCAN_FIRST).unwrap();
        let last_dot = drawn.iter().rposition(|&id| id == 139).unwrap();
        assert!(last_dot < volcano, "the volcano covers the route dots");
        assert!(
            drawn[volcano + 1..]
                .iter()
                .any(|&id| (46..=58).contains(&id)),
            "dressing on top"
        );

        let frame = |wm: &mut WorldMap| {
            ids(wm, &won)
                .into_iter()
                .find(|id| (297..=304).contains(id))
                .unwrap()
        };
        wm.tick(3.0 / ANIM_FPS + 0.01, &won);
        assert_eq!(frame(&mut wm), VULCAN_FIRST + 3);
        wm.tick(5.0 / ANIM_FPS, &won);
        assert_eq!(frame(&mut wm), VULCAN_FIRST, "loops 297..=304");
        assert!(!wm.take_sounds().contains(&SND_VULCAN), "one-shot");

        wm.tick(0.1, &unfinished);
        assert!(wm.vulcan.is_none());
    }

    #[test]
    fn burst_ambients_hide_while_waiting() {
        // Row 8 (655,58): 4 s delay, 6 frames at 12.5 fps.
        let a = &AMBIENTS[7];
        assert!(a.burst);
        let period = a.delay + (a.last - a.first).max(1) as f32 / ANIM_FPS;
        assert!(period > a.delay);
        // Phase inside the delay window → invisible (the quads loop
        // `continue`s); phase after it indexes frames 86..91.
        let phase = a.delay + 0.2;
        let id = a.first as usize
            + (((phase - a.delay) * ANIM_FPS) as usize).min((a.last - a.first) as usize - 1);
        assert!((86..=91).contains(&id));
    }
}
