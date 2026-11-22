//! Asset bundles: the unified, engine-facing asset format.
//!
//! A bundle is a directory of assets translated from one original
//! game's catalogs — palettes, color LUTs, terrain textures, sprites,
//! terrain-feature data — under uniform member names shared by every
//! game. Game differences are expressed as bundle *variants* (MC1
//! temperate/arctic world tilesets, MC2 day/night/cave environments),
//! never as schema differences: the engine resolves a variant id and
//! reads one layout.
//!
//! Like `.mgcl`, the bundle is the only asset contract between
//! `mgc-import` and the engine crates; Bullfrog catalog names, RNC,
//! RLE and FLC encodings all die in the importer.
//!
//! **The normative specification lives in `docs/FORMAT.md`** ("Asset
//! bundles"); keep this module and that document in lockstep.
//!
//! Members (all little-endian, pixel data 8-bit palette indices):
//! - `bundle.json` — [`BundleManifest`]
//! - `palette.bin` — 256 x RGBA8 (index 0 is the transparent index for
//!   sprite data; alpha is 0 there, 255 elsewhere)
//! - `shade-lut.bin` — 64 rows x 256: shade level x palette index ->
//!   final palette index (the engine's light/fog remap)
//! - `tile-colors.bin` — 256: terrain type -> flat map color index
//! - `terrain-atlas.bin` + `terrain-atlas.json` — square-cell terrain
//!   texture atlas ([`TerrainAtlasInfo`])
//! - `sprites.bin` + `sprites.json` — one 8bpp sprite atlas + its
//!   index ([`SpriteIndex`]; billboard frames, animations pre-decoded)
//!   — the SIM's view of the art (sizes, draw types)
//! - `impostors.bin` + `impostors.json` — the world-sprite impostor
//!   pack the renderer draws ([`crate::impostor::ImpostorPack`]):
//!   retail's view dispatch tabulated over its own tight atlas, with
//!   the bake's asset corrections applied
//! - `search.bin`, `build.tab.bin`, `build.dat.bin` — terrain-feature
//!   pass data (ring search order — both games; building footprints —
//!   MC1)
//! - `bldgprm.bin` — MC2's building-parameter table (BLDGPRM.DAT
//!   verbatim, 4-byte records)
//! - `spells.bin` — MC2's spell table (SPELLS.DAT verbatim, 26 rows
//!   x 80 bytes)
//! - `clrd.bin` — MC2's minimap colour table for the variant's map
//!   type (CLR{D,N,C}-0.DAT verbatim: a palette index per 12-bit
//!   `0xRGB` code, 4096 bytes)
//! - `etext.json` — the game's sentence bank (ETEXT.DAT), a JSON
//!   string array indexed by the engine's sentence id
//! - `sky.bin` — the 256x256 8bpp parallax sky bitmap (absent on
//!   variants without one, e.g. MC2 cave)
//! - `movies.json` + `movies/<name>.fmv` — the full-screen FMV
//!   streams, copied raw ([`MovieIndex`]); the one member kind the
//!   engine decodes itself, because decoding at bake time would cost
//!   ~200 MB for MC1's intro alone

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Game, Importer};

/// Current bundle format version.
pub const BUNDLE_VERSION: u32 = 1;

/// `bundle.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub format_version: u32,
    /// Bake content epoch (`crate::BAKE_EPOCH` at bake time);
    /// pre-epoch bundles read as 0.
    #[serde(default)]
    pub bake_epoch: u32,
    /// Variant id; by convention also the bundle directory name
    /// (e.g. `mc1-temperate`, `mc1-arctic`, `mc2-day`).
    pub variant: String,
    /// Game whose catalogs the bundle was translated from.
    pub game: Game,
    pub importer: Importer,
    /// Original catalog files consumed, with their raw-file hashes.
    pub sources: Vec<BundleSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleSource {
    pub file: String,
    pub sha256: String,
}

/// `terrain-atlas.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainAtlasInfo {
    /// Edge length of one square cell in pixels.
    pub cell: u32,
    /// Atlas width in pixels (cells per row = `width / cell`).
    pub width: u32,
    /// Number of cells; the terrain-type byte indexes them row-major.
    pub cells: u32,
}

/// `sprites.json`: the index into the `sprites.bin` atlas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpriteIndex {
    pub atlas_width: u32,
    pub atlas_height: u32,
    /// Indexed by the original engine's sprite id (dense; broken
    /// source entries are kept as frame-less placeholders so ids
    /// stay aligned).
    pub sprites: Vec<SpriteEntry>,
    /// UI banks only: where the appended retail `DATA/POINTERS` bank
    /// begins in `sprites` (its entries follow at `pointer_base + k`,
    /// k = the retail bank index — MC1 ball = 1, MC2 day/night/cave =
    /// 1/9/10). 0 = no pointer bank (an older bake, or a non-UI
    /// atlas).
    #[serde(default)]
    pub pointer_base: u32,
}

/// One logical sprite: all frames share one size; frame 0 is the base
/// image, further frames are the pre-decoded animation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpriteEntry {
    pub id: u32,
    /// Id of the first sprite of the family this one belongs to (a
    /// creature's rotation views, an animation set); `== id` for a
    /// family head or a standalone sprite.
    pub group: u32,
    pub width: u16,
    pub height: u16,
    /// Original archive flags, preserved for provenance/debugging.
    pub flags: u16,
    /// Atlas position of each frame; empty when the source entry is
    /// broken (known retail data corruption) — renderers skip these.
    pub frames: Vec<FramePos>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FramePos {
    pub x: u32,
    pub y: u32,
}

/// `sounds.json`: the index into the `sounds.bin` PCM blob of an audio
/// bundle (`mc1-audio`, `mc2-audio`). Sample banks are the original
/// engine's unit of loading: MC1's `SNDS<bank>-<q>.DAT` families (bank
/// selected per level by the level command stream, `q` = the original's
/// free-RAM quality tier — we always bake the highest, 22050 Hz) and
/// MC2's `SOUND/SOUND.DAT`. Entry ids are the engine's sound ids: the
/// per-tick mixer slots index straight into bank 0 (remc1 sub_55100,
/// 47 slots).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundIndex {
    /// Sample rate of every entry, Hz.
    pub sample_rate: u32,
    /// PCM encoding of `sounds.bin`; always `"pcm8"` (unsigned 8-bit
    /// mono, the original sample data byte-for-byte).
    pub encoding: String,
    pub banks: Vec<SoundBankIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundBankIndex {
    /// Original bank number (MC1 `SNDS<bank>`; MC2 uses bank 0).
    pub bank: u32,
    pub entries: Vec<SoundEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundEntry {
    /// Engine sound id (the original bank-table index; id 0 is the
    /// bank-header pseudo-entry and is never emitted).
    pub id: u32,
    /// Original sample name, lowercase, extension stripped
    /// (`firebal1`, `waves2-`).
    pub name: String,
    /// Byte offset into `sounds.bin` (samples dedupe across banks).
    pub offset: u32,
    /// Length in bytes (= samples; 8-bit mono).
    pub len: u32,
}

/// `music.json`: the music tracks of an audio bundle, one FLAC member
/// each. MC1: HMP songs rendered through OPL3 with the game's own
/// AdLib banks at import. MC2: redbook tracks ripped from the CD image
/// (the original's primary music path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicIndex {
    pub tracks: Vec<MusicTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicTrack {
    /// Original music bank (MC1 `MUSIC<bank>`, level-selected like the
    /// sound banks; MC2 redbook uses bank 0).
    pub bank: u32,
    /// Track name: MC1 the song name (`cgame1`, `csetup`); MC2 the
    /// redbook track (`track-02`).
    pub name: String,
    /// Bundle member holding the FLAC stream (`music/<...>.flac`).
    /// For MC1 in-game songs this is the AMBIENT mix: the danger
    /// layers (MIDI channels 3/4/5, kept at CC7 0 by the original
    /// and faded in during combat — remc1 sub_20BD0/sub_20D00)
    /// silenced.
    pub file: String,
    /// The danger-layer stem (channels 3/4/5 solo), sample-aligned
    /// with `file` — the runtime overlays it with a gain ramp on the
    /// danger state. Absent on songs without a muted danger layer
    /// (menu/intro music) and on redbook tracks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub danger_file: Option<String>,
    /// The General MIDI arrangement (MC1 `MUSIC<bank>-2`, the
    /// original's `GENERAL` driver target) rendered through a GM
    /// soundfont at import time — stereo, ambient mix. Only present
    /// when the baking host could render GM (fluidsynth + soundfont);
    /// `file` (the FM render) is always there as the fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gm_file: Option<String>,
    /// GM danger-layer stem, sample-aligned with `gm_file`; same
    /// contract as `danger_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gm_danger_file: Option<String>,
    /// The Sound Blaster AWE32 arrangement — the score the games
    /// targeted on that card (MC2: MUSIC.DAT's own AWE32 scores plus
    /// its `BULLFROG.SBK` bank; MC1: the GM score), rendered at import
    /// time by `mgc-awe32` on the card's wave ROM. Same contract as
    /// `gm_file`; the default arrangement wherever it was baked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awe_file: Option<String>,
    /// AWE32 danger-layer stem, sample-aligned with `awe_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awe_danger_file: Option<String>,
    /// Provenance: original source (`CGAME1.HMP`, `redbook track 2`).
    pub source: String,
}

/// `movies.json`: the full-screen FMV streams (intro, outro, the MC2
/// cutscenes, MC1's win/lose movies). Unlike every other bundle
/// member these are copied RAW — a decoded 320×200 frame is 64 KB and
/// MC1's intro alone is 3165 of them, so the runtime decodes one frame
/// at a time out of the original stream (`mgc_import::fmv::FmvCursor`)
/// rather than reading pre-decoded canvases. The format carries no
/// audio STREAM — but the movies are not silent: their soundtrack is
/// assembled at playback time by the per-movie event script, out of
/// the ordinary sample banks (narration, effects) over the MIDI
/// `INTRO`/`CUTS` sub-songs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovieIndex {
    pub movies: Vec<MovieEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovieEntry {
    /// Lowercased source stem, which is how the engine's sequence
    /// tables name it (`intro`, `outro`, `logo`, `cut1`, `levelw1`,
    /// `title-01`).
    pub name: String,
    /// Bundle member holding the raw stream (`movies/<name>.fmv`).
    pub file: String,
    /// Header frame count. Every retail stream decodes to exactly this
    /// many frames, but playback still runs until the decoder says the
    /// stream is done — this is for progress and sizing only.
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    /// Provenance: the original CD path (`INTRO/INTRO.DAT`).
    pub source: String,
}

/// `speech.json` (MC2 only): the CD voiceover, pre-sliced at import
/// by the compiled segment table `CdTracks_DB080` (remc2
/// `Type_DB080_CdTrack.h`; trace docs/traces/mc2-voiceover-
/// triggers.md) — the runtime plays whole clips and never seeks
/// inside a track. `row` = the 0-based level number; segment 0 = the
/// map-screen intro line, segment N+1 = objective row N's line,
/// segment 9 = the level-completion line. Rows 25/26 = the secret-
/// level one-liners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechIndex {
    pub clips: Vec<SpeechClip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechClip {
    /// `CdTracks_DB080` table row = 0-based level number.
    pub row: u32,
    /// Segment slot 0..=9 (empty slots are not emitted).
    pub segment: u32,
    /// Bundle member holding the FLAC clip.
    pub file: String,
    /// Clip length in milliseconds (retail's truncating frames→ms).
    pub ms: u32,
    /// Provenance (`redbook track 2 @ 0..9999ms`).
    pub source: String,
}

/// A fully-loaded bundle.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub manifest: BundleManifest,
    /// 256 RGBA entries.
    pub palette: [[u8; 4]; 256],
    /// 64 x 256 (shade level x palette index).
    pub shade_lut: Vec<u8>,
    pub tile_colors: [u8; 256],
    pub terrain_atlas: Option<(TerrainAtlasInfo, Vec<u8>)>,
    pub sprites: Option<(SpriteIndex, Vec<u8>)>,
    /// The world-sprite impostor pack (`impostors.json` + `.bin`),
    /// validated.
    pub impostors: Option<crate::impostor::ImpostorPack>,
    /// UI sprite library (HSPR: spell icons, HUD, map markers) — same
    /// index schema as `sprites`, single frame per entry.
    pub ui_sprites: Option<(SpriteIndex, Vec<u8>)>,
    /// Messaging/notification bitmap font (MC2 HFONT3, MC1 FONT2) —
    /// same index schema as `sprites`; glyphs are 1-bit coverage masks
    /// (every ink pixel = index 1). Sprite id for ASCII char `c` is
    /// `c + 1` (id 0 null, id 33 = space).
    pub font: Option<(SpriteIndex, Vec<u8>)>,
    /// MC2's fullscreen spider-web overlay bank (HWEB{D,N,C}0-0) —
    /// same index schema as `ui_sprites`: a 6×4 grid of 24 equal
    /// 8bpp tiles (transparent 0, sprite ids 1..=24) covering the
    /// 640×480 viewport, tiled over the view while the paralyze web
    /// is live (remc2 EF:21668-710). Palette-resolved like the base
    /// UI sprites.
    pub web_sprites: Option<(SpriteIndex, Vec<u8>)>,
    /// 256 RGBA entries: the book screen's own palette (DATA/BOOK.PAL).
    pub book_palette: Option<[[u8; 4]; 256]>,
    /// 64KB UI blend LUT (TABLES +0x4000): 2D blits resolve
    /// `blend[src | dest<<8]` — UI sprites get their true colors only
    /// through this table against their background.
    pub blend_lut: Option<Vec<u8>>,
    pub search: Option<Vec<u8>>,
    pub build_tab: Option<Vec<u8>>,
    pub build_dat: Option<Vec<u8>>,
    /// MC2's building-parameter table (BLDGPRM.DAT verbatim: 4-byte
    /// records {u16 word, u8 flags, u8 chain-next}; retail loads 76
    /// records into a 77-slot table).
    pub bldgprm: Option<Vec<u8>>,
    /// MC2's spell table (SPELLS.DAT verbatim: 26 rows x 80 bytes,
    /// remc2 Spells.h — {i8, u8 enabled, 3 x 26-byte subspell tiers});
    /// the par1-authored class-10 overrides and class-15 cast costs.
    pub spells: Option<Vec<u8>>,
    /// MC2's minimap colour table (CLR{D,N,C}-0.DAT verbatim, 4096
    /// bytes: code `0xRGB` → palette index). Retail's own quantization,
    /// NOT the palette's nearest colour.
    pub clrd: Option<Vec<u8>>,
    /// The game's sentence bank (ETEXT.DAT decoded to strings, index
    /// = the engine's sentence id; empty slots preserved so indices
    /// stay aligned). MC2: 471 entries — 23..=47 the map-screen level
    /// briefings, 48..=158 the per-level objective/completion blocks
    /// (indexed by remc2 GameUI.cpp:20-42's IndexLevelText/
    /// LevelEndText tables). MC1: 80 entries — 60/61 the win message.
    pub etext: Option<Vec<String>>,
    /// 256x256 8bpp parallax sky bitmap, row-major (retail samples
    /// u = index low byte, v = high byte — remc2 DrawSky_40950).
    /// Absent where retail loads none (MC2 cave).
    pub sky: Option<Vec<u8>>,
}

/// Rows in `shade-lut.bin`.
pub const SHADE_LEVELS: usize = 64;

#[derive(Debug)]
pub enum BundleError {
    Io(PathBuf, std::io::Error),
    Json(PathBuf, serde_json::Error),
    /// A member has the wrong size for its declared shape.
    Malformed(PathBuf, String),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Json(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Malformed(p, m) => write!(f, "{}: {m}", p.display()),
        }
    }
}

impl std::error::Error for BundleError {}

impl Bundle {
    /// Load a bundle directory. Optional members (atlas, sprites,
    /// feature data) load as `None` when absent; the manifest, palette
    /// and color LUTs are required.
    /// This bundle's own (w, h)-per-sprite list — what the MC1 /
    /// Hidden Worlds sprite-stat derivation reads (the loader derives
    /// each row's width from the row's first sprite, remc1
    /// sub_main.cpp:66699). `None` when the bundle carries no sprites.
    pub fn sprite_dims(&self) -> Option<Vec<(u16, u16)>> {
        self.sprites
            .as_ref()
            .map(|(s, _)| s.sprites.iter().map(|e| (e.width, e.height)).collect())
    }

    /// The (w,h)-per-sprite list feeding the MC2 extents derivation:
    /// DAY-sourced — retail computes its particle-param table once
    /// at boot against TMAPS0-0 (`sub_71410_process_tmaps`' sole
    /// caller is Initialize, EF:42885; the boot-active tmaps file is
    /// the day bank, TextureMaps.cpp:595; the per-level TAB swap
    /// never recomputes it), so night/cave/fog levels run day-art
    /// extents while rendering their own banks (the banks ship
    /// genuinely different dims — sprite 96 is 38 wide day, 36
    /// night/cave). Falls back to this render bundle's own index
    /// when the day variant isn't baked.
    pub fn mc2_extent_dims(&self, assets_dir: &Path) -> Option<Vec<(u16, u16)>> {
        fn dims(s: &SpriteIndex) -> Vec<(u16, u16)> {
            s.sprites.iter().map(|e| (e.width, e.height)).collect()
        }
        std::fs::read(assets_dir.join("mc2-day").join("sprites.json"))
            .ok()
            .and_then(|d| serde_json::from_slice::<SpriteIndex>(&d).ok())
            .map(|s| dims(&s))
            .or_else(|| self.sprites.as_ref().map(|(s, _)| dims(s)))
    }

    pub fn load(dir: &Path) -> Result<Self, BundleError> {
        let read = |name: &str| -> Result<Vec<u8>, BundleError> {
            let p = dir.join(name);
            std::fs::read(&p).map_err(|e| BundleError::Io(p, e))
        };
        let read_opt = |name: &str| -> Option<Vec<u8>> { std::fs::read(dir.join(name)).ok() };
        let expect = |name: &str, data: &[u8], len: usize| -> Result<(), BundleError> {
            if data.len() != len {
                return Err(BundleError::Malformed(
                    dir.join(name),
                    format!("{} bytes, expected {len}", data.len()),
                ));
            }
            Ok(())
        };

        let manifest_path = dir.join("bundle.json");
        let manifest: BundleManifest = serde_json::from_slice(&read("bundle.json")?)
            .map_err(|e| BundleError::Json(manifest_path, e))?;

        let palette_bytes = read("palette.bin")?;
        expect("palette.bin", &palette_bytes, 256 * 4)?;
        let mut palette = [[0u8; 4]; 256];
        for (i, rgba) in palette.iter_mut().enumerate() {
            rgba.copy_from_slice(&palette_bytes[i * 4..i * 4 + 4]);
        }

        let shade_lut = read("shade-lut.bin")?;
        expect("shade-lut.bin", &shade_lut, SHADE_LEVELS * 256)?;

        let tile_colors_bytes = read("tile-colors.bin")?;
        expect("tile-colors.bin", &tile_colors_bytes, 256)?;
        let mut tile_colors = [0u8; 256];
        tile_colors.copy_from_slice(&tile_colors_bytes);

        let terrain_atlas = match read_opt("terrain-atlas.bin") {
            Some(data) => {
                let info_path = dir.join("terrain-atlas.json");
                let info: TerrainAtlasInfo = serde_json::from_slice(&read("terrain-atlas.json")?)
                    .map_err(|e| BundleError::Json(info_path, e))?;
                let cell_rows = info.cells.div_ceil(info.width / info.cell);
                expect(
                    "terrain-atlas.bin",
                    &data,
                    (info.width * info.cell * cell_rows) as usize,
                )?;
                Some((info, data))
            }
            None => None,
        };

        let sprites = match read_opt("sprites.bin") {
            Some(data) => {
                let index_path = dir.join("sprites.json");
                let index: SpriteIndex = serde_json::from_slice(&read("sprites.json")?)
                    .map_err(|e| BundleError::Json(index_path, e))?;
                expect(
                    "sprites.bin",
                    &data,
                    index.atlas_width as usize * index.atlas_height as usize,
                )?;
                Some((index, data))
            }
            None => None,
        };

        let impostors = match read_opt(crate::impostor::ImpostorPack::PIXELS_FILE) {
            Some(pixels) => {
                let name = crate::impostor::ImpostorPack::INDEX_FILE;
                let index_path = dir.join(name);
                let index = serde_json::from_slice(&read(name)?)
                    .map_err(|e| BundleError::Json(index_path.clone(), e))?;
                let pack = crate::impostor::ImpostorPack { index, pixels };
                pack.validate()
                    .map_err(|e| BundleError::Malformed(index_path, e))?;
                Some(pack)
            }
            None => None,
        };

        let ui_sprites = match read_opt("ui-sprites.bin") {
            Some(data) => {
                let index_path = dir.join("ui-sprites.json");
                let index: SpriteIndex = serde_json::from_slice(&read("ui-sprites.json")?)
                    .map_err(|e| BundleError::Json(index_path, e))?;
                expect(
                    "ui-sprites.bin",
                    &data,
                    index.atlas_width as usize * index.atlas_height as usize,
                )?;
                Some((index, data))
            }
            None => None,
        };

        let font = match read_opt("font.bin") {
            Some(data) => {
                let index_path = dir.join("font.json");
                let index: SpriteIndex = serde_json::from_slice(&read("font.json")?)
                    .map_err(|e| BundleError::Json(index_path, e))?;
                expect(
                    "font.bin",
                    &data,
                    index.atlas_width as usize * index.atlas_height as usize,
                )?;
                Some((index, data))
            }
            None => None,
        };

        let web_sprites = match read_opt("web-sprites.bin") {
            Some(data) => {
                let index_path = dir.join("web-sprites.json");
                let index: SpriteIndex = serde_json::from_slice(&read("web-sprites.json")?)
                    .map_err(|e| BundleError::Json(index_path, e))?;
                expect(
                    "web-sprites.bin",
                    &data,
                    index.atlas_width as usize * index.atlas_height as usize,
                )?;
                Some((index, data))
            }
            None => None,
        };

        let book_palette = match read_opt("book-palette.bin") {
            Some(data) => {
                expect("book-palette.bin", &data, 256 * 4)?;
                let mut pal = [[0u8; 4]; 256];
                for (i, rgba) in pal.iter_mut().enumerate() {
                    rgba.copy_from_slice(&data[i * 4..i * 4 + 4]);
                }
                Some(pal)
            }
            None => None,
        };

        let blend_lut = match read_opt("blend-lut.bin") {
            Some(data) => {
                expect("blend-lut.bin", &data, 0x10000)?;
                Some(data)
            }
            None => None,
        };

        let etext = match read_opt("etext.json") {
            Some(bytes) => Some(
                serde_json::from_slice(&bytes)
                    .map_err(|e| BundleError::Json(dir.join("etext.json"), e))?,
            ),
            None => None,
        };

        let sky = match read_opt("sky.bin") {
            Some(data) => {
                expect("sky.bin", &data, 256 * 256)?;
                Some(data)
            }
            None => None,
        };

        Ok(Self {
            manifest,
            palette,
            shade_lut,
            tile_colors,
            terrain_atlas,
            sprites,
            impostors,
            ui_sprites,
            font,
            web_sprites,
            book_palette,
            blend_lut,
            search: read_opt("search.bin"),
            build_tab: read_opt("build.tab.bin"),
            build_dat: read_opt("build.dat.bin"),
            bldgprm: read_opt("bldgprm.bin"),
            spells: read_opt("spells.bin"),
            clrd: read_opt("clrd.bin").filter(|t| t.len() == 4096),
            etext,
            sky,
        })
    }
}

/// A fully-loaded audio bundle (`<game>-audio` variants). Sounds load
/// eagerly (a few MB of 8-bit PCM); music stays on disk — tracks are
/// FLAC files streamed by path at play time.
#[derive(Debug, Clone)]
pub struct AudioBundle {
    pub manifest: BundleManifest,
    pub sounds: Option<(SoundIndex, Vec<u8>)>,
    pub music: Option<MusicIndex>,
    /// MC2 voiceover clips (`speech.json`), absent elsewhere.
    pub speech: Option<SpeechIndex>,
    /// Bundle directory, for resolving [`MusicTrack::file`].
    pub dir: PathBuf,
}

impl AudioBundle {
    /// Load an audio bundle directory. Both member families are
    /// optional (a bundle may carry only music while the game's sample
    /// track is unported); the manifest is required.
    pub fn load(dir: &Path) -> Result<Self, BundleError> {
        let manifest_path = dir.join("bundle.json");
        let manifest_bytes =
            std::fs::read(&manifest_path).map_err(|e| BundleError::Io(manifest_path.clone(), e))?;
        let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| BundleError::Json(manifest_path, e))?;

        let sounds = match std::fs::read(dir.join("sounds.bin")) {
            Ok(data) => {
                let index_path = dir.join("sounds.json");
                let index_bytes = std::fs::read(&index_path)
                    .map_err(|e| BundleError::Io(index_path.clone(), e))?;
                let index: SoundIndex = serde_json::from_slice(&index_bytes)
                    .map_err(|e| BundleError::Json(index_path.clone(), e))?;
                for bank in &index.banks {
                    for e in &bank.entries {
                        if e.offset as usize + e.len as usize > data.len() {
                            return Err(BundleError::Malformed(
                                index_path,
                                format!(
                                    "bank {} id {} ({}) spans past sounds.bin",
                                    bank.bank, e.id, e.name
                                ),
                            ));
                        }
                    }
                }
                Some((index, data))
            }
            Err(_) => None,
        };

        let music = match std::fs::read(dir.join("music.json")) {
            Ok(bytes) => Some(
                serde_json::from_slice(&bytes)
                    .map_err(|e| BundleError::Json(dir.join("music.json"), e))?,
            ),
            Err(_) => None,
        };

        let speech = match std::fs::read(dir.join("speech.json")) {
            Ok(bytes) => Some(
                serde_json::from_slice(&bytes)
                    .map_err(|e| BundleError::Json(dir.join("speech.json"), e))?,
            ),
            Err(_) => None,
        };

        Ok(Self {
            manifest,
            sounds,
            music,
            speech,
            dir: dir.to_path_buf(),
        })
    }
}
