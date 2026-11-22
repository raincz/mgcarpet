//! Bundle baking: original per-game catalogs in, unified asset bundles
//! out (`baked/assets/<variant>/`, schema in `mgc_formats::bundle` and
//! docs/FORMAT.md).
//!
//! MC1 ships two complete world tilesets — 0 = temperate, 1 = arctic
//! (snow; used by the Hidden Worlds bundle) — each with its own palette,
//! color tables, terrain atlas, sprites, and building footprints. They
//! bake as the `mc1-temperate` and `mc1-arctic` variants. MC2 ships
//! four environment graphics sets from its CD catalogs — `mc2-day`,
//! `mc2-night`, `mc2-night-fog` (night levels with gfx_type bit 1),
//! `mc2-cave` — same schema; they carry `search.bin` (same format as
//! MC1) and `bldgprm.bin` (MC2's building-parameter table) instead of
//! MC1's BUILD members.

use std::path::Path;

use sha2::{Digest, Sha256};

use mgc_formats::bundle::{
    BUNDLE_VERSION, BundleManifest, BundleSource, MovieEntry, MovieIndex, MusicIndex, MusicTrack,
    SpeechClip, SpeechIndex, TerrainAtlasInfo,
};
use mgc_formats::impostor::ImpostorPack;
use mgc_formats::{Game, Importer};

use crate::bake::BakeError;
use crate::gamedata::GameSource;
use crate::impostors;
use crate::sprites;
use crate::tmaps::TmapsArchive;

/// Width of the baked sprite atlas; retail sprites max out well below.
/// Doubled as needed to keep the height under wgpu's baseline 2D
/// texture limit (MC2's animated sets pack ~9.4k rows at 1024).
const SPRITE_ATLAS_WIDTH: u32 = 1024;
const MAX_TEXTURE_DIM: u32 = 8192;
/// UI sprite atlas width: 87 small sprites (~122k pixels) pack well
/// under 256 rows at this width.
const UI_ATLAS_WIDTH: u32 = 512;

const SHADE_LUT_LEN: usize = 0x4000; // 64 shade levels x 256 colors
const TILE_COLORS_OFFSET: usize = 0x14000;
const TABLES_LEN: usize = 0x14600; // the engine's full color-table blob
const ATLAS_CELL: u32 = 32;
const ATLAS_WIDTH: u32 = 256;
const ATLAS_CELLS: u32 = 152;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One bundle variant: which original catalogs feed the uniform members.
/// The differences between MC1's world tilesets and MC2's environments
/// are entirely in file names and one TABLES layout shift, so a single
/// spec-driven bake covers both games.
struct VariantSpec {
    variant: &'static str,
    game: Game,
    /// 768-byte 6-bit VGA palette (`DATA/…`).
    palette: &'static str,
    /// 0x14600-byte color-table blob (`DATA/…`).
    tables: &'static str,
    /// Offset of the 64x256 shade LUT inside the tables blob: +0x0000
    /// in BOTH games (row 32 ≈ identity, row 0 = the fog/sky color,
    /// row 63 = black). +0x4000 is the 256x256 sprite BLEND matrix
    /// (`T[0x4000 + (src<<8)|dst]`, remc2 GameRenderObjects
    /// DrawSprite_41BD3) — see docs/traces/mc2-transparency-drawlist.md.
    /// NOT a shade table. The tile-type→map-color table is at +0x14000
    /// in both games.
    shade_offset: usize,
    /// Terrain atlas, 256px wide, 152 cells of 32x32; the terrain-type
    /// byte is the cell index in both games (identity mapping, remc2
    /// GameRenderHD.cpp:854).
    atlas: &'static str,
    /// TMAPS base name without extension (`DATA/…`): world billboards.
    tmaps: &'static str,
    /// Ring search-order table (`DATA/SEARCH.DAT`, 1024 bytes) — the
    /// same 32x32 relative-offset format in BOTH games (remc2 loads it
    /// via sub_101C0, EventsFunctions.cpp:3589).
    search: Option<&'static str>,
    /// BUILD bank base name (6-byte .TAB rows + .DAT cells — building
    /// footprints/paint). MC1: 1-byte cell codes; MC2 (`BUILD0-0`,
    /// ONE bank for all environments — remc2 Basic.cpp:271 loads a
    /// fixed path): 2 bytes per cell {paint code, pad height}, read
    /// by the build action (ApplyTerrainModification_37240 :27181).
    build: Option<&'static str>,
    /// MC2 only: the building-parameter table (`DATA/BLDGPRM.DAT`,
    /// 4-byte records {u16 production rate, u8 flags, u8 chain};
    /// loader remc2 sub_539A0 EventsFunctions.cpp:38319 — flags:
    /// 0x10 GenerateEvents pass F/G split, 8 no mana/production,
    /// 4 no cave second-heightmap, 1 enterable). Footprint sizes
    /// live in the BUILD bank, NOT here.
    bldgprm: Option<&'static str>,
    /// MC2 only: the spell table (`DATA/SPELLS.DAT`, 26 rows x 80
    /// bytes: {i8, u8 enabled, 3 x 26-byte subspell tiers} — remc2
    /// Spells.h + Basic.cpp:334 loads it over the Spells.cpp baked-in
    /// fallback; the retail CD values DIFFER from that fallback, so
    /// carrying the real file is load-bearing). Feeds the par1-authored
    /// class-10 effect overrides and the class-15 cast costs.
    spells: Option<&'static str>,
    /// UI sprite library base name (HSPR = the 640x480 set; see
    /// `crate::hspr`). Implies `DATA/BOOK.PAL` (the book screen's own
    /// palette) for MC1 only — MC2 has no book screen. MC2's TAB/DAT
    /// pairs are the same self-describing format (remc2
    /// `bitmap_pos_struct2_t`, portability/bitmap_pos_struct.h:27 —
    /// {u32 offset, u8 w, u8 h}; same signed-RLE rows).
    ui: Option<&'static str>,
    /// The messaging/notification bitmap font base name (same HSPR
    /// TAB/DAT format, decoded by `crate::hspr`). BOTH games render the
    /// top-of-screen notification with the small `DATA/FONT1` (~4x7):
    /// `FontType_D419D` is only ever 1 or 3 and the font table
    /// `E9B20[4] = {FONT0, FONT1, HFONT3, FONT1}` maps both to FONT1
    /// (remc2 Basic.cpp:241/324; remc1 sub_main.cpp `sub_5A3C0(1)`) —
    /// HFONT3 is never on this path. Glyphs are 1-bit masks (every ink
    /// pixel = index 1); the sprite id for ASCII char `c` is `c + 1`
    /// (id 0 null, id 33 = space). Baked to `font.bin`/`font.json` —
    /// the app appends the glyph masks to its UI atlas as white and
    /// tints per DrawText's `color` argument.
    font: Option<&'static str>,
    /// The sentence bank (`DATA/ETEXT.DAT`): null-terminated strings,
    /// decoded to `etext.json` with indices preserved (empty slots
    /// stay). English base; localized `LANGUAGE/L<n>.TXT` overlays use
    /// the same indices (remc2 LoadLanguageFile) — not baked.
    etext: Option<&'static str>,
    /// The parallax sky bitmap (256x256 8bpp raw, RNC on MC1): MC1
    /// `SKY.DAT`/`SKY1-0.DAT` per tileset; MC2 `SKY{D,N}0-0.DAT` with
    /// night-fog sharing night's (remc2 ReadAndDecompress.cpp:41/88)
    /// and cave loading NONE (`SKYC0-0.DAT` exists on the CD but no
    /// code path reads it).
    sky: Option<&'static str>,
    /// MC2 only: the fullscreen SPIDER-WEB overlay bank
    /// (`DATA/HWEB{D,N,C}0-0` .DAT/.TAB — the hi-res set; `MWEB*` is
    /// the VGA one). Same self-describing HSPR format as `ui`: a 6×4
    /// grid of 24 equal 8bpp tiles (transparent index 0, sprite ids
    /// 1..=24) covering the 640×480 viewport. Retail loads one per
    /// map-type (remc2 Level.cpp:1426-1502) and tiles it over the
    /// view while the paralyze web (`mobilizeCounter`) is live
    /// (EventsFunctions.cpp:21668-710).
    web: Option<&'static str>,
    /// MC2 only: the minimap colour table (`DATA/CLR{D,N,C}-0.DAT`,
    /// 4096 bytes — a palette index per 12-bit `0xRGB` code), one per
    /// map type; night-fog shares night's (remc2 EventsFunctions.cpp
    /// :31866-99 loads it beside the palette). NOT a nearest-colour
    /// derivative of the palette: ~36% of codes resolve differently,
    /// so the real table is load-bearing (map dot colours).
    clrd: Option<&'static str>,
}

const MC1_VARIANTS: [VariantSpec; 2] = [
    VariantSpec {
        variant: "mc1-temperate",
        game: Game::MagicCarpet1,
        palette: "DATA/PAL0-0.DAT",
        tables: "DATA/TABLES.DAT",
        shade_offset: 0,
        atlas: "DATA/BLK0-1.DAT",
        tmaps: "DATA/TMAPS0-0",
        search: Some("DATA/SEARCH.DAT"),
        build: Some("DATA/BUILD0-0"),
        bldgprm: None,
        spells: None,
        ui: Some("DATA/HSPR0-0"),
        font: Some("DATA/FONT1"),
        etext: Some("DATA/ETEXT.DAT"),
        sky: Some("DATA/SKY.DAT"),
        web: None,
        clrd: None,
    },
    VariantSpec {
        variant: "mc1-arctic",
        game: Game::MagicCarpet1,
        palette: "DATA/PAL1-0.DAT",
        tables: "DATA/DTABLES.DAT",
        shade_offset: 0,
        atlas: "DATA/BLK1-1.DAT",
        tmaps: "DATA/TMAPS1-0",
        search: Some("DATA/SEARCH.DAT"),
        build: Some("DATA/BUILD1-0"),
        bldgprm: None,
        spells: None,
        ui: Some("DATA/HSPR1-0"),
        font: Some("DATA/FONT1"),
        etext: Some("DATA/ETEXT.DAT"),
        sky: Some("DATA/SKY1-0.DAT"),
        web: None,
        clrd: None,
    },
];

/// MC2's per-environment catalogs (remc2 ReadAndDecompress.cpp:21-170,
/// Level.cpp:878-906): day uses the un-suffixed BLOCK32 atlas, night
/// splits into plain and "fog" graphics on the level header's gfx_type
/// bit 1 (fog swaps atlas + palette; tables and TMAPS stay night), and
/// TMAPS digits are the MapType ordinals (0 day / 1 night / 2 cave).
const MC2_VARIANTS: [VariantSpec; 4] = [
    VariantSpec {
        variant: "mc2-day",
        game: Game::MagicCarpet2,
        palette: "DATA/PALD-0.DAT",
        tables: "DATA/TABLESD.DAT",
        shade_offset: 0,
        atlas: "DATA/BLOCK32.DAT",
        tmaps: "DATA/TMAPS0-0",
        search: Some("DATA/SEARCH.DAT"),
        build: Some("DATA/BUILD0-0"),
        bldgprm: Some("DATA/BLDGPRM.DAT"),
        spells: Some("DATA/SPELLS.DAT"),
        ui: Some("DATA/HSPRD0-0"),
        font: Some("DATA/FONT1"),
        etext: Some("DATA/ETEXT.DAT"),
        sky: Some("DATA/SKYD0-0.DAT"),
        web: Some("DATA/HWEBD0-0"),
        clrd: Some("DATA/CLRD-0.DAT"),
    },
    VariantSpec {
        variant: "mc2-night",
        game: Game::MagicCarpet2,
        palette: "DATA/PALN-0.DAT",
        tables: "DATA/TABLESN.DAT",
        shade_offset: 0,
        atlas: "DATA/BL32N0-0.DAT",
        tmaps: "DATA/TMAPS1-0",
        search: Some("DATA/SEARCH.DAT"),
        build: Some("DATA/BUILD0-0"),
        bldgprm: Some("DATA/BLDGPRM.DAT"),
        spells: Some("DATA/SPELLS.DAT"),
        ui: Some("DATA/HSPRN0-0"),
        font: Some("DATA/FONT1"),
        etext: Some("DATA/ETEXT.DAT"),
        sky: Some("DATA/SKYN0-0.DAT"),
        web: Some("DATA/HWEBN0-0"),
        clrd: Some("DATA/CLRN-0.DAT"),
    },
    VariantSpec {
        variant: "mc2-night-fog",
        game: Game::MagicCarpet2,
        palette: "DATA/PALF-0.DAT",
        tables: "DATA/TABLESN.DAT",
        shade_offset: 0,
        atlas: "DATA/BL32F0-0.DAT",
        tmaps: "DATA/TMAPS1-0",
        search: Some("DATA/SEARCH.DAT"),
        build: Some("DATA/BUILD0-0"),
        bldgprm: Some("DATA/BLDGPRM.DAT"),
        spells: Some("DATA/SPELLS.DAT"),
        ui: Some("DATA/HSPRN0-0"),
        font: Some("DATA/FONT1"),
        etext: Some("DATA/ETEXT.DAT"),
        sky: Some("DATA/SKYN0-0.DAT"),
        web: Some("DATA/HWEBN0-0"),
        clrd: Some("DATA/CLRN-0.DAT"),
    },
    VariantSpec {
        variant: "mc2-cave",
        game: Game::MagicCarpet2,
        palette: "DATA/PALC-0.DAT",
        tables: "DATA/TABLESC.DAT",
        shade_offset: 0,
        atlas: "DATA/BL32C0-0.DAT",
        tmaps: "DATA/TMAPS2-0",
        search: Some("DATA/SEARCH.DAT"),
        build: Some("DATA/BUILD0-0"),
        bldgprm: Some("DATA/BLDGPRM.DAT"),
        spells: Some("DATA/SPELLS.DAT"),
        ui: Some("DATA/HSPRC0-0"),
        font: Some("DATA/FONT1"),
        etext: Some("DATA/ETEXT.DAT"),
        sky: None,
        web: Some("DATA/HWEBC0-0"),
        clrd: Some("DATA/CLRC-0.DAT"),
    },
];

fn bake_bundle_set(
    src: &GameSource,
    out_dir: &Path,
    specs: &[VariantSpec],
) -> Result<Vec<(String, String)>, BakeError> {
    let mut outputs = Vec::new();
    for spec in specs {
        let dir = out_dir.join("assets").join(spec.variant);
        std::fs::create_dir_all(&dir).map_err(|e| BakeError::Io(dir.clone(), e))?;
        let baked = bake_variant(src, &dir, spec)?;
        outputs.extend(
            baked
                .into_iter()
                .map(|(name, sha)| (format!("assets/{}/{name}", spec.variant), sha)),
        );
    }
    Ok(outputs)
}

/// Bake MC1's two world tilesets into `out_dir/assets/{mc1-temperate,
/// mc1-arctic}`. Returns `(manifest path, sha256)` pairs for every
/// written member. Member semantics are in docs/FORMAT.md "Asset
/// bundles"; per-variant source catalogs in [`MC1_VARIANTS`].
pub fn bake_mc1_bundles(
    src: &GameSource,
    out_dir: &Path,
) -> Result<Vec<(String, String)>, BakeError> {
    let outputs = bake_bundle_set(src, out_dir, &MC1_VARIANTS)?;
    // The pre-bundle asset layout; remove so stale files cannot shadow
    // the bundles.
    let legacy = out_dir.join("mc1/assets");
    if legacy.is_dir() {
        std::fs::remove_dir_all(&legacy).map_err(|e| BakeError::Io(legacy.clone(), e))?;
    }
    Ok(outputs)
}

/// Bake MC2's four environment bundles (`mc2-day`, `mc2-night`,
/// `mc2-night-fog`, `mc2-cave`) from the CD catalogs, including the
/// search/build members (SEARCH.DAT + BUILD*-0 + BLDGPRM.DAT).
pub fn bake_mc2_bundles(
    src: &GameSource,
    out_dir: &Path,
) -> Result<Vec<(String, String)>, BakeError> {
    bake_bundle_set(src, out_dir, &MC2_VARIANTS)
}

/// Highest of MC1's free-RAM sample-quality tiers (`SNDS<bank>-1`),
/// 22050 Hz; `-0` is the same audio at half rate, `-3` lower still
/// (see `crate::sound`).
const MC1_SOUND_QUALITY: u32 = 1;
const MC1_SOUND_RATE: u32 = 22050;
/// OPL render / redbook output rate.
const MUSIC_RATE: u32 = 44100;
/// MC1 ships sample banks 0..=13 (bank = per-level/screen sound set).
const MC1_SOUND_BANKS: std::ops::RangeInclusive<u32> = 0..=13;

/// The Sound Blaster AWE32's 1 MB General MIDI wave ROM, bundled
/// zstd-compressed (`assets/static/awe32.rom.zst`) and compiled in, so
/// the AWE32 arrangement bakes with nothing beside the binary.
fn awe32_rom() -> Result<Vec<u8>, String> {
    static ROM: &[u8] = include_bytes!("../../../assets/static/awe32.rom.zst");
    zstd::decode_all(ROM).map_err(|e| format!("AWE32 ROM: {e}"))
}

/// One AWE32 render (`mgc-awe32`, the AWE32Emu port) as the f32 the
/// shared pair normalization takes. A bank that fails to load is an
/// error here — never bake a silent or half-voiced arrangement; the
/// caller falls back to GM.
fn awe32_render(
    opts: &mgc_awe32::RenderOptions,
    score: mgc_awe32::Score,
) -> Result<Vec<f32>, String> {
    let r = mgc_awe32::render(opts, score)?;
    if let Some(w) = r.warnings.first() {
        return Err(format!("AWE32 bank: {w}"));
    }
    Ok(r.pcm.iter().map(|&s| f32::from(s) / 32768.0).collect())
}

// One shared normalization factor per song — the
// overlay SUM is what must not clip (MC1 GM contract).
fn normalize_gm_pair(
    mut base: Vec<f32>,
    mut stem: Option<Vec<f32>>,
) -> (Vec<i16>, Option<Vec<i16>>) {
    let frames = base.len().max(stem.as_ref().map_or(0, Vec::len));
    base.resize(frames, 0.0);
    let mut peak = 0f32;
    if let Some(stem) = &mut stem {
        stem.resize(frames, 0.0);
        for (b, s) in base.iter().zip(stem.iter()) {
            peak = peak.max((b + s).abs());
        }
    } else {
        for b in &base {
            peak = peak.max(b.abs());
        }
    }
    let scale = if peak > 0.0 { 30000.0 / peak } else { 1.0 };
    let quantize = |pcm: &[f32]| -> Vec<i16> {
        pcm.iter()
            .map(|s| (s * scale).clamp(-32767.0, 32767.0) as i16)
            .collect()
    };
    let base = quantize(&base);
    let stem = stem.as_ref().map(|s| quantize(s));
    (base, stem)
}

/// Bake MC1's audio bundle (`baked/assets/mc1-audio/`): every SNDS
/// sample bank at the highest quality tier, deduplicated into one PCM
/// blob. Sounds and music are tileset-independent (the bank digit is a
/// level selector, not a world-set pair), so audio is one per-game
/// bundle rather than a member of the graphics variants.
pub fn bake_mc1_audio(
    src: &GameSource,
    out_dir: &Path,
) -> Result<Vec<(String, String)>, BakeError> {
    let dir = out_dir.join("assets").join("mc1-audio");
    std::fs::create_dir_all(&dir).map_err(|e| BakeError::Io(dir.clone(), e))?;

    let mut outputs = Vec::new();
    let mut sources = Vec::new();

    let mut emit = |name: &str, bytes: &[u8]| -> Result<(), BakeError> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| BakeError::Io(path, e))?;
        outputs.push((name.to_string(), hex(&Sha256::digest(bytes))));
        Ok(())
    };
    let source = |rel: &str, sources: &mut Vec<BundleSource>| -> Result<Vec<u8>, BakeError> {
        let raw = src
            .read(rel)
            .map_err(|e| BakeError::Io(Path::new(rel).to_path_buf(), e))?;
        sources.push(BundleSource {
            file: rel.rsplit('/').next().unwrap_or(rel).to_string(),
            sha256: hex(&Sha256::digest(&raw)),
        });
        if crate::rnc::is_rnc(&raw) {
            crate::rnc::decompress(&raw)
                .map_err(|e| BakeError::Level(Path::new(rel).to_path_buf(), 0, e.to_string()))
        } else {
            Ok(raw)
        }
    };

    // Sample banks. Decompressed DAT/TAB pairs are borrowed by the
    // parsed banks, so decompress all of them first.
    println!("mc1 audio: extracting sound banks…");
    let mut raw_pairs = Vec::new();
    for bank in MC1_SOUND_BANKS {
        let dat_rel = format!("DATA/SNDS{bank}-{MC1_SOUND_QUALITY}.DAT");
        let tab_rel = format!("DATA/SNDS{bank}-{MC1_SOUND_QUALITY}.TAB");
        if !src.exists(&dat_rel) {
            continue;
        }
        let dat = source(&dat_rel, &mut sources)?;
        let tab = source(&tab_rel, &mut sources)?;
        raw_pairs.push((bank, dat_rel, tab, dat));
    }
    let mut banks = Vec::new();
    for (bank, dat_rel, tab, dat) in &raw_pairs {
        banks.push(
            crate::sound::parse_bank(*bank, tab, dat, true)
                .map_err(|e| BakeError::Level(Path::new(dat_rel).to_path_buf(), 0, e))?,
        );
    }
    let (index, blob) = crate::sound::bake_blob(&banks, MC1_SOUND_RATE);
    emit("sounds.bin", &blob)?;
    emit(
        "sounds.json",
        &serde_json::to_vec_pretty(&index).expect("sound index serializes"),
    )?;

    // Music: the AdLib arrangement (`MUSIC<bank>-0`, the `-0` driver
    // digit is AdLib per remc1 :54030 — 0xA002 loads inst/drum.bnk)
    // rendered through OPL3 with the game's own banks, FLAC per song.
    // When the host can render General MIDI (oxisynth + a GM
    // soundfont, see `crate::synth`), the `-2` arrangement (`GENERAL`,
    // remc1 :54029-30 — 0xA001 → digit 2) is baked alongside as the
    // optional GM upgrade; absent hosts still get the full FM bundle.
    let music_dir = dir.join("music");
    std::fs::create_dir_all(&music_dir).map_err(|e| BakeError::Io(music_dir.clone(), e))?;
    let inst = crate::adlib::parse_bnk(&source("DATA/INST.BNK", &mut sources)?)
        .map_err(|e| BakeError::Level(Path::new("DATA/INST.BNK").to_path_buf(), 0, e))?;
    let drum = crate::adlib::parse_bnk(&source("DATA/DRUM.BNK", &mut sources)?)
        .map_err(|e| BakeError::Level(Path::new("DATA/DRUM.BNK").to_path_buf(), 0, e))?;
    let gm = match crate::synth::GmRenderer::locate() {
        Ok(r) => Some(r),
        Err(why) => {
            println!("note: mc1 music: no GM render ({why}) — FM only");
            None
        }
    };
    // The AWE32 arrangement (the default wherever it bakes — player
    // ruling 2026-11-22): the same `-2` GENERAL score MC1 sent its
    // AWE32 driver (`BF_MUSIC=SBAWE32` → 0xA008, the GM bank), played
    // on the card's wave ROM by `mgc-awe32`. The presets come from
    // CARPET.EXE itself: it embeds the AWE32 driver's GM preset /
    // instrument / sample tables byte-for-byte as SBAWE32.MDI carries
    // them (the table scan finds them; HIDDEN.EXE holds the same).
    // MC1's own driver code is HMI's, which AWE32Emu does not model —
    // the Miles `dos` family stands in (close: `sdk` measured within
    // ±1.5 dB).
    let awe: Option<(Vec<u8>, Vec<u8>)> = if src.exists("CARPET.EXE") {
        match (awe32_rom(), source("CARPET.EXE", &mut sources)) {
            (Ok(rom), Ok(exe)) => Some((rom, exe)),
            (Err(why), _) => {
                println!("note: mc1 music: no AWE32 render ({why})");
                None
            }
            (_, Err(why)) => {
                println!("note: mc1 music: no AWE32 render ({why})");
                None
            }
        }
    } else {
        println!("note: mc1 music: no CARPET.EXE — no AWE32 render");
        None
    };
    let mut music = MusicIndex { tracks: Vec::new() };
    for bank in 0..=1u32 {
        let dat_rel = format!("DATA/MUSIC{bank}-0.DAT");
        let tab_rel = format!("DATA/MUSIC{bank}-0.TAB");
        if !src.exists(&dat_rel) {
            continue;
        }
        let dat = source(&dat_rel, &mut sources)?;
        let tab = source(&tab_rel, &mut sources)?;
        let parsed = crate::sound::parse_bank(bank, &tab, &dat, false)
            .map_err(|e| BakeError::Level(Path::new(&dat_rel).to_path_buf(), 0, e))?;
        // The GM arrangement, keyed by song stem (`cgame1.gen` ↔
        // `cgame1.hmp` — same songs, per-driver patches/mix).
        let mut gm_songs: Vec<(String, crate::hmp::Song)> = Vec::new();
        if (gm.is_some() || awe.is_some()) && src.exists(&format!("DATA/MUSIC{bank}-2.DAT")) {
            let dat = source(&format!("DATA/MUSIC{bank}-2.DAT"), &mut sources)?;
            let tab = source(&format!("DATA/MUSIC{bank}-2.TAB"), &mut sources)?;
            let parsed = crate::sound::parse_bank(bank, &tab, &dat, false)
                .map_err(|e| BakeError::Level(Path::new("DATA/MUSIC-2.DAT").to_path_buf(), 0, e))?;
            for (_, name, bytes) in &parsed.entries {
                let stem = name.split('.').next().unwrap_or(name).to_string();
                let song = crate::hmp::parse(bytes).map_err(|e| {
                    BakeError::Level(
                        Path::new("DATA/MUSIC-2.DAT").to_path_buf(),
                        0,
                        format!("{name}: {e}"),
                    )
                })?;
                gm_songs.push((stem, song));
            }
        }
        for (i, (_, name, hmp_bytes)) in parsed.entries.iter().enumerate() {
            // The render below is the slow half of the whole first-run
            // bake (OPL3 FM per song, GM again when a soundfont is
            // present) — say so, per song, or a double-click console
            // reads as hung and gets killed (Windows player report).
            println!(
                "mc1 music: rendering {} ({}/{} of bank {bank}{})…",
                name.strip_suffix(".hmp").unwrap_or(name),
                i + 1,
                parsed.entries.len(),
                match (gm.is_some(), awe.is_some()) {
                    (true, true) => ", FM + GM + AWE32",
                    (true, false) => ", FM + GM",
                    (false, true) => ", FM + AWE32",
                    (false, false) => ", FM",
                },
            );
            let err = |e: String| {
                BakeError::Level(Path::new(&dat_rel).to_path_buf(), 0, format!("{name}: {e}"))
            };
            let song = crate::hmp::parse(hmp_bytes).map_err(err)?;
            // In-game songs keep their danger layers (MIDI channels
            // 3/4/5, CC7-0-muted, runtime-faded by the original) as a
            // separate sample-aligned stem; the base file is the
            // ambient mix.
            let layered = crate::adlib::has_danger_layer(&song);
            let mix = if layered {
                crate::adlib::MixSpec::ambient()
            } else {
                crate::adlib::MixSpec::full()
            };
            let pcm = crate::adlib::render(&song, &inst, &drum, MUSIC_RATE, &mix).map_err(err)?;
            let flac = crate::flac::encode(&pcm, 1, MUSIC_RATE).map_err(err)?;
            let name = name.strip_suffix(".hmp").unwrap_or(name);
            let member = format!("music/{bank}-{name}.flac");
            emit(&member, &flac)?;
            let danger_file = if layered {
                let stem = crate::adlib::render(
                    &song,
                    &inst,
                    &drum,
                    MUSIC_RATE,
                    &crate::adlib::MixSpec::danger_stem(),
                )
                .map_err(err)?;
                debug_assert_eq!(stem.len(), pcm.len(), "stems must stay sample-aligned");
                let flac = crate::flac::encode(&stem, 1, MUSIC_RATE).map_err(err)?;
                let member = format!("music/{bank}-{name}-danger.flac");
                emit(&member, &flac)?;
                Some(member)
            } else {
                None
            };
            // The GM upgrade: same song from the `-2` arrangement,
            // oxisynth-rendered ambient + danger stem (both scaled
            // by ONE factor — the overlay sum is what must not clip).
            let mut gm_file = None;
            let mut gm_danger_file = None;
            if let (Some(renderer), Some((_, gm_song))) =
                (gm.as_ref(), gm_songs.iter().find(|(stem, _)| stem == name))
            {
                let render = |mix: &crate::adlib::MixSpec| {
                    let midi = crate::smf::encode(gm_song, mix);
                    renderer.render(&midi, MUSIC_RATE)
                };
                let layered = crate::adlib::has_danger_layer(gm_song);
                let base_mix = if layered {
                    crate::adlib::MixSpec::ambient()
                } else {
                    crate::adlib::MixSpec::full()
                };
                let base = render(&base_mix).map_err(err)?;
                let stem = if layered {
                    Some(render(&crate::adlib::MixSpec::danger_stem()).map_err(err)?)
                } else {
                    None
                };
                let (base, stem) = normalize_gm_pair(base, stem);
                let member = format!("music/{bank}-{name}-gm.flac");
                emit(
                    &member,
                    &crate::flac::encode(&base, 2, MUSIC_RATE).map_err(err)?,
                )?;
                gm_file = Some(member);
                if let Some(stem) = &stem {
                    let member = format!("music/{bank}-{name}-gm-danger.flac");
                    emit(
                        &member,
                        &crate::flac::encode(stem, 2, MUSIC_RATE).map_err(err)?,
                    )?;
                    gm_danger_file = Some(member);
                }
            }
            // The AWE32 render: the same score and mixes as GM, one
            // shared normalization per pair. A failure keeps GM.
            let mut awe_file = None;
            let mut awe_danger_file = None;
            if let (Some((rom, exe)), Some((_, gm_song))) =
                (awe.as_ref(), gm_songs.iter().find(|(stem, _)| stem == name))
            {
                let render = |mix: &crate::adlib::MixSpec| {
                    let midi = crate::smf::encode(gm_song, mix);
                    let mut o = mgc_awe32::RenderOptions::new(rom, mgc_awe32::Driver::Dos);
                    o.banks = vec![mgc_awe32::BankSpec::mdi_tables(exe)];
                    o.master_volume = Some(100);
                    awe32_render(&o, mgc_awe32::Score::Smf(&midi))
                };
                let layered = crate::adlib::has_danger_layer(gm_song);
                let base_mix = if layered {
                    crate::adlib::MixSpec::ambient()
                } else {
                    crate::adlib::MixSpec::full()
                };
                let pair = render(&base_mix).and_then(|base| {
                    let stem = if layered {
                        Some(render(&crate::adlib::MixSpec::danger_stem())?)
                    } else {
                        None
                    };
                    Ok((base, stem))
                });
                match pair {
                    Err(why) => println!("note: mc1 music: {name}: no AWE32 render ({why})"),
                    Ok((base, stem)) => {
                        let (base, stem) = normalize_gm_pair(base, stem);
                        let member = format!("music/{bank}-{name}-awe.flac");
                        emit(
                            &member,
                            &crate::flac::encode(&base, 2, MUSIC_RATE).map_err(err)?,
                        )?;
                        awe_file = Some(member);
                        if let Some(stem) = &stem {
                            let member = format!("music/{bank}-{name}-awe-danger.flac");
                            emit(
                                &member,
                                &crate::flac::encode(stem, 2, MUSIC_RATE).map_err(err)?,
                            )?;
                            awe_danger_file = Some(member);
                        }
                    }
                }
            }
            music.tracks.push(MusicTrack {
                bank,
                name: name.to_string(),
                file: member,
                danger_file,
                gm_file,
                gm_danger_file,
                awe_file,
                awe_danger_file,
                source: format!("MUSIC{bank}-0 {}.HMP", name.to_ascii_uppercase()),
            });
        }
    }
    emit(
        "music.json",
        &serde_json::to_vec_pretty(&music).expect("music index serializes"),
    )?;

    let manifest = BundleManifest {
        format_version: BUNDLE_VERSION,
        bake_epoch: mgc_formats::BAKE_EPOCH,
        variant: "mc1-audio".to_string(),
        game: Game::MagicCarpet1,
        importer: Importer {
            name: "mgc-import".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        sources,
    };
    emit(
        "bundle.json",
        &serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
    )?;
    Ok(outputs
        .into_iter()
        .map(|(name, sha)| (format!("assets/mc1-audio/{name}"), sha))
        .collect())
}

/// Bake MC2's audio bundle (`baked/assets/mc2-audio/`).
///
/// Three member families (traces docs/traces/mc2-music-law.md,
/// mc2-music-dat-xmi.md, mc2-voiceover-triggers.md):
/// - samples: `SOUND/SOUND.DAT`, best shipped tier;
/// - music: the `SOUND/MUSIC.DAT` GM bank-0 XMI sub-songs (the "C2" =
///   Magic Carpet 2 set — GAME1/2/3 = the MapType tracks Night/Day/
///   Cave, SETUP = menu) rendered through oxisynth — retail gameplay
///   music is NEVER the redbook, and bank 0 (not the `-music2` bank-1
///   "C1"/MC1 set) is the default. cc119-tagged channels are the
///   war/danger layers
///   (expression-zeroed in peace, combat-ramped — Sound.cpp:851/
///   5880), baked as the MC1-style ambient mix + sample-aligned
///   danger stem;
/// - speech: the redbook voiceover pre-sliced by `CdTracks_DB080`
///   (table row r = level r → rip track r+2; row 27 = dead data) —
///   the runtime plays whole clips, never seeks inside a track.
pub fn bake_mc2_audio(
    src: &GameSource,
    out_dir: &Path,
) -> Result<Vec<(String, String)>, BakeError> {
    let Some(image) = src.cd_image() else {
        return Ok(Vec::new());
    };
    let image = image.to_path_buf();
    // GOG's `game.ins`, or a retail dump's own `.cue`.
    let cue_path = src
        .cue_sheet()
        .unwrap_or_else(|| image.with_extension("ins"));
    // A missing cue sheet loses ONLY the redbook voiceover rip —
    // sounds.bin and the music renders still bake.
    let cue = match std::fs::read_to_string(&cue_path) {
        Ok(c) => Some(c),
        Err(_) => {
            eprintln!(
                "note: mc2: no cue sheet at {} — speech clips will not be baked \
                 (sounds + music proceed)",
                cue_path.display()
            );
            None
        }
    };

    let dir = out_dir.join("assets").join("mc2-audio");
    let music_dir = dir.join("music");
    std::fs::create_dir_all(&music_dir).map_err(|e| BakeError::Io(music_dir.clone(), e))?;

    let mut outputs = Vec::new();
    let mut emit = |name: &str, bytes: &[u8]| -> Result<(), BakeError> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| BakeError::Io(path, e))?;
        outputs.push((name.to_string(), hex(&Sha256::digest(bytes))));
        Ok(())
    };

    let mut sources = Vec::new();

    // Samples: SOUND/SOUND.DAT, best shipped quality tier (8-bit
    // 22050 across the retail GOG file — same PCM encoding as MC1).
    let sound_dat_raw = src
        .read("SOUND/SOUND.DAT")
        .map_err(|e| BakeError::Io(Path::new("SOUND/SOUND.DAT").to_path_buf(), e))?;
    sources.push(BundleSource {
        file: "SOUND.DAT".into(),
        sha256: hex(&Sha256::digest(&sound_dat_raw)),
    });
    println!("mc2 audio: extracting sound samples…");
    let banks = crate::sound::parse_mc2_sound_dat(&sound_dat_raw)
        .map_err(|e| BakeError::Level(Path::new("SOUND/SOUND.DAT").to_path_buf(), 0, e))?;
    let (index, blob) = crate::sound::bake_blob(&banks, MC1_SOUND_RATE);
    emit("sounds.bin", &blob)?;
    emit(
        "sounds.json",
        &serde_json::to_vec_pretty(&index).expect("sound index serializes"),
    )?;

    // Gameplay music: MUSIC.DAT GM bank 0 (the "C2" = Magic Carpet 2
    // set), sub-songs 0..=3 by role. Bank 0 is the DEFAULT gameplay
    // bank: `musicChannel_E3814 = 0` (Sound.cpp:49, never reassigned)
    // → `InitMusic_8D970` loads `InitMusicBank(0)` (Sound.cpp:801).
    // Bank 1 (the "C1" = Magic Carpet 1 set) loads ONLY under the
    // hidden `-music2` command-line flag (EF:39191/43023 guarded by
    // `setting_byte4_25 & 0x40`, default clear) — the classic-MC1-
    // soundtrack alternate, an opt-in (authenticity matrix), NOT the
    // default; baking bank 1 yields wrong/unfamiliar gameplay tracks
    // (docs/traces/mc2-music-law.md). Requires the GM renderer — MC2
    // has no pure-Rust FM fallback yet (the F section is a future
    // faithful-alternate).
    let mut music = MusicIndex { tracks: Vec::new() };
    let music_dat = src
        .read("SOUND/MUSIC.DAT")
        .map_err(|e| BakeError::Io(Path::new("SOUND/MUSIC.DAT").to_path_buf(), e))?;
    sources.push(BundleSource {
        file: "MUSIC.DAT".into(),
        sha256: hex(&Sha256::digest(&music_dat)),
    });
    match crate::synth::GmRenderer::locate() {
        Err(why) => println!("note: mc2 music: no GM render ({why}) — music skipped"),
        Ok(renderer) => {
            let subsongs = crate::mc2_music::parse_gm_bank(&music_dat, 0)
                .map_err(|e| BakeError::Level(Path::new("SOUND/MUSIC.DAT").to_path_buf(), 0, e))?;
            // The AWE32 arrangement (the default wherever it bakes —
            // player ruling 2026-11-22): the game's OWN AWE32 scores
            // (MUSIC.DAT driver slot W, `C2*.WTB` — SETUP and INTRO
            // re-arranged vs GM) through its AWE32 driver's banks in
            // the game's load order (`SBAWE32.MDI`'s GM set, then
            // `BULLFROG.SBK` — Bullfrog's own presets over GM, Sound.cpp
            // InitAWE32_9F740), on the card's wave ROM, with the MDI
            // driver's start-up channel state (`mgc_awe32::MC2_CONF`).
            // Rendered by `mgc-awe32` (the AWE32Emu port).
            let awe = (|| -> Result<_, String> {
                let rom = awe32_rom()?;
                let mdi = src.read("SOUND/SBAWE32.MDI").map_err(|e| e.to_string())?;
                let sbk = src.read("SOUND/BULLFROG.SBK").map_err(|e| e.to_string())?;
                let w = crate::mc2_music::parse_bank(
                    &music_dat,
                    0,
                    crate::mc2_music::DRIVER_AWE32,
                )?;
                Ok((rom, mdi, sbk, w))
            })();
            let awe = match awe {
                Ok(a) => Some(a),
                Err(why) => {
                    println!("note: mc2 music: no AWE32 render ({why})");
                    None
                }
            };
            // MapType track n → sub-song n-1 (the ±1 lives in AIL:
            // `SOUND_start_sequence(track-1)`, Sound.cpp:4974): Night=1
            // →GAME1, Day=2→GAME2, Cave=3→GAME3; menu StartMusic(4)→
            // SETUP (idx 3, the shared C2SETUP).
            // Sub-songs 4/5 are INTRO and CUTS — the score for the
            // full-screen movies, which carry no audio of their own.
            const ROLES: [(usize, &str); 6] = [
                (0, "mc2-night"),
                (1, "mc2-day"),
                (2, "mc2-cave"),
                (3, "mc2-menu"),
                (4, "mc2-intro"),
                (5, "mc2-cuts"),
            ];
            for (idx, role) in ROLES {
                let sub = subsongs.get(idx).ok_or_else(|| {
                    BakeError::Level(
                        Path::new("SOUND/MUSIC.DAT").to_path_buf(),
                        0,
                        format!("GM bank 0 has no sub-song {idx}"),
                    )
                })?;
                let err = |e: String| {
                    BakeError::Level(
                        Path::new("SOUND/MUSIC.DAT").to_path_buf(),
                        0,
                        format!("{}: {e}", sub.name),
                    )
                };
                println!(
                    "mc2 music: rendering {role} ({})…",
                    if awe.is_some() { "GM + AWE32" } else { "GM" }
                );
                let layered = sub.song.has_war_layer();
                let base_mix = if layered {
                    crate::xmi::Mix::Ambient
                } else {
                    crate::xmi::Mix::Full
                };
                let render = |mix: crate::xmi::Mix| {
                    let midi = crate::xmi::encode_smf(&sub.song, mix);
                    renderer.render(&midi, MUSIC_RATE)
                };
                let base = render(base_mix).map_err(err)?;
                let stem = if layered {
                    Some(render(crate::xmi::Mix::WarStem).map_err(err)?)
                } else {
                    None
                };
                let (base, stem) = normalize_gm_pair(base, stem);
                let member = format!("music/{role}.flac");
                emit(
                    &member,
                    &crate::flac::encode(&base, 2, MUSIC_RATE).map_err(err)?,
                )?;
                let danger_file = match &stem {
                    Some(stem) => {
                        let member = format!("music/{role}-danger.flac");
                        emit(
                            &member,
                            &crate::flac::encode(stem, 2, MUSIC_RATE).map_err(err)?,
                        )?;
                        Some(member)
                    }
                    None => None,
                };
                // The AWE32 pair. The W sub-song is matched to this role
                // by name stem (C2GAME1.WTB ↔ C2GAME1.GEN), not position.
                let stem_of = |n: &str| n.split('.').next().unwrap_or(n).to_ascii_uppercase();
                let mut awe_file = None;
                let mut awe_danger_file = None;
                if let Some((rom, mdi, sbk, w)) = &awe {
                    match w.iter().find(|ws| stem_of(&ws.name) == stem_of(&sub.name)) {
                        None => println!(
                            "note: mc2 music: {role}: no AWE32 score for {}",
                            sub.name
                        ),
                        Some(ws) => {
                            // Retail's XMIDI master volume: the menu plays
                            // at 127, gameplay and the movie scores at 100
                            // (measured against the game's driver trace —
                            // AWE32Emu conf notes); 127 clips the gameplay
                            // pieces hard.
                            let master = if role == "mc2-menu" { 127 } else { 100 };
                            // The war stem plays its channels un-muted:
                            // the conf's CC119 trigger mute is what keeps
                            // them silent in the ambient render.
                            let stem_conf = mgc_awe32::MC2_CONF
                                .replace("trigger_mute 1", "trigger_mute 0");
                            let render = |mask: u16, conf: &str| {
                                let mut o =
                                    mgc_awe32::RenderOptions::new(rom, mgc_awe32::Driver::Dos);
                                o.banks = vec![
                                    mgc_awe32::BankSpec::sf(mdi),
                                    mgc_awe32::BankSpec::sf(sbk),
                                ];
                                o.conf = Some(conf);
                                o.master_volume = Some(master);
                                o.channel_mask = mask;
                                awe32_render(&o, mgc_awe32::Score::Xmi(&ws.xmi))
                            };
                            let war = ws.song.war_channels;
                            let pair = render(0xFFFF, mgc_awe32::MC2_CONF).and_then(|base| {
                                let stem = if war != 0 {
                                    Some(render(war, &stem_conf)?)
                                } else {
                                    None
                                };
                                Ok((base, stem))
                            });
                            match pair {
                                Err(why) => {
                                    println!("note: mc2 music: {role}: no AWE32 render ({why})")
                                }
                                Ok((base, stem)) => {
                                    let (base, stem) = normalize_gm_pair(base, stem);
                                    let member = format!("music/{role}-awe.flac");
                                    emit(
                                        &member,
                                        &crate::flac::encode(&base, 2, MUSIC_RATE)
                                            .map_err(err)?,
                                    )?;
                                    awe_file = Some(member);
                                    if let Some(stem) = &stem {
                                        let member = format!("music/{role}-awe-danger.flac");
                                        emit(
                                            &member,
                                            &crate::flac::encode(stem, 2, MUSIC_RATE)
                                                .map_err(err)?,
                                        )?;
                                        awe_danger_file = Some(member);
                                    }
                                }
                            }
                        }
                    }
                }
                music.tracks.push(MusicTrack {
                    bank: 0,
                    name: role.to_string(),
                    file: member,
                    danger_file,
                    gm_file: None,
                    gm_danger_file: None,
                    awe_file,
                    awe_danger_file,
                    source: format!("MUSIC.DAT G bank 0 {}", sub.name),
                });
            }
        }
    }
    emit(
        "music.json",
        &serde_json::to_vec_pretty(&music).expect("music index serializes"),
    )?;

    // Voiceover: slice each rip track by its CdTracks_DB080 row —
    // only with a cue sheet; an empty speech index otherwise.
    let mut speech = SpeechIndex { clips: Vec::new() };
    let mut dejunked = 0u32;
    let mut dejunked_ms = 0u64;
    if let Some(cue) = &cue {
        let image_len = std::fs::metadata(&image)
            .map_err(|e| BakeError::Io(image.clone(), e))?
            .len();
        let tracks = crate::redbook::parse_cue(cue, image_len / crate::redbook::SECTOR)
            .map_err(|e| BakeError::Level(cue_path.clone(), 0, e))?;
        let speech_dir = dir.join("speech");
        std::fs::create_dir_all(&speech_dir).map_err(|e| BakeError::Io(speech_dir.clone(), e))?;
        println!("mc2 speech: ripping voiceover tracks from the CD image…");
        for (row, entry) in crate::cdtracks::CD_TRACKS.iter().enumerate() {
            // Table row r = level r → rip track r+2 (duration-fit proof
            // in the trace; row 27 implies track 29 = dead data).
            let rip_number = entry.track as u32 + 1;
            let Some(track) = tracks.iter().find(|t| t.number == rip_number) else {
                continue;
            };
            println!("mc2 speech: track {rip_number} (level {row:02})…");
            let pcm = crate::redbook::read_track(&image, *track)
                .map_err(|e| BakeError::Io(image.clone(), e))?;
            for (seg, &(start, len)) in entry.segments.iter().enumerate() {
                if len == 0 {
                    continue; // empty slot — retail no-ops on length 0
                }
                let start_ms = crate::cdtracks::frames_to_ms(start);
                let len_ms = crate::cdtracks::frames_to_ms(len);
                let rate = u64::from(crate::redbook::RATE);
                let a = (u64::from(start_ms) * rate / 1000 * 2) as usize;
                let b = (u64::from(start_ms + len_ms) * rate / 1000 * 2) as usize;
                let (a, b) = (a.min(pcm.len()), b.min(pcm.len()));
                if a >= b {
                    println!("note: mc2 speech: row {row} seg {seg} out of track — skipped");
                    continue;
                }
                // Silence the pre-voice mastering junk (the retail
                // narration crackle — see redbook::mute_leading_junk;
                // durations preserved, the pristine rip stays in
                // gamedata). SEGMENT 0 ONLY: the corruption lives at
                // the track heads (the map narratives); the in-level
                // hint segments are unaffected and must not be swept.
                let mut clip = pcm[a..b].to_vec();
                if seg == 0
                    && let Some(ms) = crate::redbook::mute_leading_junk(&mut clip)
                {
                    dejunked += 1;
                    dejunked_ms += u64::from(ms);
                }
                let flac = crate::flac::encode(&clip, 2, crate::redbook::RATE).map_err(|e| {
                    BakeError::Level(image.clone(), 0, format!("row {row} seg {seg}: {e}"))
                })?;
                let member = format!("speech/level-{row:02}-seg-{seg}.flac");
                emit(&member, &flac)?;
                speech.clips.push(SpeechClip {
                    row: row as u32,
                    segment: seg as u32,
                    file: member,
                    ms: len_ms,
                    source: format!(
                        "redbook track {rip_number} @ {start_ms}..{}ms",
                        start_ms + len_ms
                    ),
                });
            }
        }
        if dejunked > 0 {
            println!(
                "mc2 speech: muted mastering junk ahead of the voice in {dejunked} clip(s) \
                 ({:.1} s total; the retail narration crackle)",
                dejunked_ms as f64 / 1000.0
            );
        }
    }
    emit(
        "speech.json",
        &serde_json::to_vec_pretty(&speech).expect("speech index serializes"),
    )?;

    let manifest = BundleManifest {
        format_version: BUNDLE_VERSION,
        bake_epoch: mgc_formats::BAKE_EPOCH,
        variant: "mc2-audio".to_string(),
        game: Game::MagicCarpet2,
        importer: Importer {
            name: "mgc-import".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        sources: {
            let image_name = image
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            sources.push(match &cue {
                Some(cue) => BundleSource {
                    file: format!("{image_name} (redbook tracks)"),
                    // Hashing the 400 MB image per bake is
                    // disproportionate; provenance is the cue sheet's
                    // digest instead.
                    sha256: hex(&Sha256::digest(cue.as_bytes())),
                },
                None => BundleSource {
                    file: format!(
                        "{image_name} (redbook tracks) — cue sheet MISSING, \
                         speech clips not baked"
                    ),
                    sha256: String::new(),
                },
            });
            sources
        },
    };
    emit(
        "bundle.json",
        &serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
    )?;
    Ok(outputs
        .into_iter()
        .map(|(name, sha)| (format!("assets/mc2-audio/{name}"), sha))
        .collect())
}

/// The 50 world names of an MC1/HW EXE: consecutive NUL-terminated
/// literals `"<n>. <Name>"` after `"Text Omitted"` (the compiler's
/// alignment filler sits between some of them, hence the search per
/// ordinal rather than a split). None unless all 50 read cleanly.
fn exe_level_names(exe: &[u8]) -> Option<Vec<String>> {
    fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
        hay.get(from..)?
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|i| i + from)
    }
    let mut at = find(exe, b"Text Omitted\0", 0)?;
    let mut out = Vec::with_capacity(50);
    for n in 1..=50 {
        let tag = format!("{n}. ");
        let k = find(exe, tag.as_bytes(), at)?;
        // Each name follows its predecessor closely; a far hit is not
        // the table.
        if k - at > 64 {
            return None;
        }
        let start = k + tag.len();
        let len = exe[start..].iter().position(|&b| b == 0)?;
        let name = std::str::from_utf8(&exe[start..start + len]).ok()?;
        if name.is_empty() || len > 32 || !name.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return None;
        }
        out.push(name.to_string());
        at = start + len;
    }
    Some(out)
}

/// The file offset of an LE executable's object loaded at `base` (the
/// MZ header's `e_lfanew` → "LE"; object table at LE+0x40, data pages
/// at LE+0x80). The repo-root EXEs and the CD's differ only in their
/// stub, so the object's offset moves (CARPET data: 0x957F8 vs 0x91600).
fn le_object_file_offset(exe: &[u8], base: u32) -> Option<usize> {
    let u32_at = |at: usize| Some(u32::from_le_bytes(exe.get(at..at + 4)?.try_into().ok()?));
    let le = u32_at(0x3C)? as usize;
    if exe.get(le..le + 2)? != b"LE" {
        return None;
    }
    let page = u32_at(le + 0x28)? as usize;
    let table = le + u32_at(le + 0x40)? as usize;
    let pages = u32_at(le + 0x80)? as usize;
    (0..u32_at(le + 0x44)? as usize).find_map(|i| {
        let ent = table + 24 * i;
        (u32_at(ent + 4)? == base).then_some(())?;
        Some(pages + (u32_at(ent + 12)? as usize).checked_sub(1)? * page)
    })
}

/// The lobby's arena names (`off_99A84`): a table of `count` pointers
/// at data VA 0x99A84 in both EXEs. The pointers are stored UNRELOCATED
/// (offsets into the data object at VA 0x90000), so a name's file
/// offset is the pointer + the data object's file offset
/// (docs/traces/mc1-multiplayer-lobby.md §4). Every entry must be a
/// short printable string, otherwise this is not the shipped EXE and
/// the result is None.
fn exe_arena_names(exe: &[u8], count: usize) -> Option<Vec<String>> {
    let data = le_object_file_offset(exe, 0x90000)?;
    let table = 0x99A84 - 0x90000 + data;
    (0..count)
        .map(|i| {
            let at = table + 4 * i;
            let p = u32::from_le_bytes(exe.get(at..at + 4)?.try_into().ok()?) as usize;
            let start = p.checked_add(data)?;
            let len = exe.get(start..)?.iter().take(33).position(|&b| b == 0)?;
            let s = std::str::from_utf8(&exe[start..start + len]).ok()?;
            (!s.is_empty() && s.bytes().all(|b| (0x20..0x7f).contains(&b))).then(|| s.to_string())
        })
        .collect()
}

/// The MC1/HW frontend bundle → `assets/mc1-ui/`: the 320×200 main
/// menu (bg + palette + MMMASK hotspot bitmap + MMSPR button/dialog
/// sprites), the GLOBE/TIMER menu animations (Bullfrog FMV deltas,
/// cropped to their animated regions), and the FONT1 glyph masks.
/// Shared by BOTH campaigns — the retail CD serves one
/// `DATA/SCREENS` set to CARPET.EXE and HIDDEN.EXE alike (only the
/// title art differs, docs/traces/mc1-campaign-save-menu.md).
pub fn bake_mc1_menu(src: &GameSource, out_dir: &Path) -> Result<Vec<(String, String)>, BakeError> {
    if !src.exists("DATA/SCREENS/MAINMENU.DAT") {
        eprintln!("note: mc1: no DATA/SCREENS/MAINMENU.DAT — skipping frontend bundle");
        return Ok(Vec::new());
    }
    let get = |rel: &str| -> Result<Vec<u8>, BakeError> {
        let raw = src
            .read(rel)
            .map_err(|e| BakeError::Io(Path::new(rel).to_path_buf(), e))?;
        if crate::rnc::is_rnc(&raw) {
            crate::rnc::decompress(&raw).map_err(|e| {
                BakeError::Level(Path::new(rel).to_path_buf(), 0, format!("RNC: {e:?}"))
            })
        } else {
            Ok(raw)
        }
    };
    let dir = out_dir.join("assets").join("mc1-ui");
    std::fs::create_dir_all(&dir).map_err(|e| BakeError::Io(dir.clone(), e))?;
    let mut outputs = Vec::new();
    let mut sources = Vec::new();
    let mut emit = |name: &str, bytes: &[u8]| -> Result<(), BakeError> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| BakeError::Io(path, e))?;
        outputs.push((name.to_string(), hex(&Sha256::digest(bytes))));
        Ok(())
    };

    let bg = get("DATA/SCREENS/MAINMENU.DAT")?;
    let pal = get("DATA/SCREENS/MAINMENU.PAL")?;
    let mask = get("DATA/SCREENS/MMMASK.DAT")?;
    if bg.len() != 64_000 || mask.len() != 64_000 || pal.len() != 768 {
        return Err(BakeError::Level(
            Path::new("DATA/SCREENS/MAINMENU.DAT").to_path_buf(),
            0,
            format!(
                "unexpected sizes: bg {} mask {} pal {}",
                bg.len(),
                mask.len(),
                pal.len()
            ),
        ));
    }
    emit("menu-pal.bin", &pal)?;
    emit("menu-mask.bin", &mask)?;
    for f in ["MAINMENU.DAT", "MAINMENU.PAL", "MMMASK.DAT"] {
        sources.push(BundleSource {
            file: f.into(),
            sha256: hex(&Sha256::digest(&get(&format!("DATA/SCREENS/{f}"))?)),
        });
    }

    // MMSPR: the menu's label/button sprites (HSPR TAB/DAT, 8
    // entries — LOAD/SAVE labels, OK, Cancel, quit prompt).
    let dat = get("DATA/SCREENS/MMSPR.DAT")?;
    let tab = get("DATA/SCREENS/MMSPR.TAB")?;
    let decoded = crate::hspr::decode(&dat, &tab).map_err(|e| {
        BakeError::Level(
            Path::new("DATA/SCREENS/MMSPR.DAT").to_path_buf(),
            0,
            e.to_string(),
        )
    })?;
    let packed = sprites::pack(&decoded, UI_ATLAS_WIDTH);
    emit("menu-sprites.bin", &packed.atlas)?;
    emit(
        "menu-sprites.json",
        &serde_json::to_vec_pretty(&packed.index).expect("mmspr index serializes"),
    )?;

    // POINTERS: the retail mouse pointers ([0] = the yellow pointer
    // with the mana ball — the cursor sub_5C05C installs for the
    // menus and every view mode, remc1 :42024). The frontend
    // composes [0] into its screen buffer as its cursor.
    if src.exists("DATA/POINTERS.DAT") {
        let pdat = get("DATA/POINTERS.DAT")?;
        let ptab = get("DATA/POINTERS.TAB")?;
        let pdec = crate::hspr::decode(&pdat, &ptab).map_err(|e| {
            BakeError::Level(
                Path::new("DATA/POINTERS.DAT").to_path_buf(),
                0,
                e.to_string(),
            )
        })?;
        let ppacked = sprites::pack(&pdec, UI_ATLAS_WIDTH);
        emit("pointer-sprites.bin", &ppacked.atlas)?;
        emit(
            "pointer-sprites.json",
            &serde_json::to_vec_pretty(&ppacked.index).expect("pointers index serializes"),
        )?;
        for f in ["POINTERS.DAT", "POINTERS.TAB"] {
            sources.push(BundleSource {
                file: f.into(),
                sha256: hex(&Sha256::digest(&get(&format!("DATA/{f}"))?)),
            });
        }
    }

    emit("menu-bg.bin", &bg)?;

    // GLOBE/TIMER: menu mini-movies. Retail steps ONLY the delta
    // frames over the live screen — the full-canvas keyframe never
    // draws (it differs from MAINMENU.DAT on 58k pixels incl. a
    // black globe surround; the menu art already contains globe and
    // hourglass at rest). The animation therefore = the pixels the
    // INTER-FRAME deltas touch (globe: 3024 px, the spinning
    // circle). Baked as bbox crops of every frame PLUS a touched-
    // pixel mask; the app blits only masked pixels over the art.
    let crop_movie = |movie: &crate::fmv::Fmv,
                      file: &str|
     -> Result<(Vec<u8>, Vec<u8>, serde_json::Value), BakeError> {
        let (w, h) = (movie.width, movie.height);
        let mut touched = vec![false; w * h];
        for pair in movie.frames.windows(2) {
            for i in 0..w * h {
                if pair[0][i] != pair[1][i] {
                    touched[i] = true;
                }
            }
        }
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        for y in 0..h {
            for x in 0..w {
                if touched[y * w + x] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 > x1 {
            return Err(BakeError::Level(
                Path::new(file).to_path_buf(),
                0,
                "movie has no animated pixels".into(),
            ));
        }
        let (cw, ch) = (x1 + 1 - x0, y1 + 1 - y0);
        let mut bin = Vec::with_capacity(movie.frames.len() * cw * ch);
        for fr in &movie.frames {
            for y in y0..=y1 {
                bin.extend_from_slice(&fr[y * w + x0..y * w + x1 + 1]);
            }
        }
        let mut mask = Vec::with_capacity(cw * ch);
        for y in y0..=y1 {
            for x in x0..=x1 {
                mask.push(touched[y * w + x] as u8);
            }
        }
        let meta = serde_json::json!({
            "x": x0, "y": y0, "w": cw, "h": ch,
            "frames": movie.frames.len(),
        });
        Ok((bin, mask, meta))
    };
    // The menu movies' embedded palettes differ from MAINMENU.PAL in
    // a handful of entries (TIMER: 18, SCROLL: ~19; GLOBE none) —
    // indices blitted raw under the menu palette land on the wrong
    // colors there (black hourglass-outline specks). Remap every movie
    // frame through the nearest-color LUT into the MENU palette before
    // anything else consumes it.
    let remap_frames = |movie: &mut crate::fmv::Fmv| {
        let Some(mpal) = movie.palette else { return };
        if mpal[..] == pal[..] {
            return;
        }
        let rgb = |p: &[u8], i: usize| [p[i * 3] as i32, p[i * 3 + 1] as i32, p[i * 3 + 2] as i32];
        let lut: Vec<u8> = (0..256)
            .map(|i| {
                // Index 0 is the engine-wide transparent index —
                // it must survive the remap untouched.
                if i == 0 {
                    return 0;
                }
                let want = rgb(&mpal, i);
                let mut best = (0usize, i32::MAX);
                for j in 0..256 {
                    let c = rgb(&pal, j);
                    let d = (0..3).map(|k| (c[k] - want[k]).pow(2)).sum();
                    if d < best.1 {
                        best = (j, d);
                    }
                }
                best.0 as u8
            })
            .collect();
        for f in &mut movie.frames {
            for p in f.iter_mut() {
                *p = lut[*p as usize];
            }
        }
    };

    // The globe animation = its inter-frame touched set alone (the
    // spinning circle; the arch around it is MAINMENU art).
    {
        let raw = get("DATA/SCREENS/GLOBE.DAT")?;
        let mut movie = crate::fmv::decode(&raw, Some(&bg))
            .map_err(|e| BakeError::Level(Path::new("GLOBE.DAT").to_path_buf(), 0, e))?;
        remap_frames(&mut movie);
        let (fbin, gmask, meta) = crop_movie(&movie, "GLOBE.DAT")?;
        emit("globe.bin", &fbin)?;
        emit("globe-mask.bin", &gmask)?;
        emit(
            "globe.json",
            &serde_json::to_vec_pretty(&meta).expect("globe meta serializes"),
        )?;
        sources.push(BundleSource {
            file: "GLOBE.DAT".into(),
            sha256: hex(&Sha256::digest(&raw)),
        });
    }

    // The TIMER is different: the hourglass exists ONLY while a game
    // is underway — MAINMENU.DAT has bare wall in the MMMASK id-11
    // (Continue) region, and the movie's keyframe carries the whole
    // hourglass on its stand. Its visible set = the inter-frame sand
    // pixels PLUS every keyframe pixel
    // inside the id-11 hotspot that VISUALLY differs from the wall
    // art (RGB compare through the menu palette — the streams'
    // palettes are near-identical, so a small threshold splits real
    // art from palette-index noise).
    {
        let raw = get("DATA/SCREENS/TIMER.DAT")?;
        let mut movie = crate::fmv::decode(&raw, Some(&bg))
            .map_err(|e| BakeError::Level(Path::new("TIMER.DAT").to_path_buf(), 0, e))?;
        remap_frames(&mut movie);
        let (w, h) = (movie.width, movie.height);
        let mut visible = vec![false; w * h];
        for pair in movie.frames.windows(2) {
            for i in 0..w * h {
                if pair[0][i] != pair[1][i] {
                    visible[i] = true;
                }
            }
        }
        let rgb = |i: usize| {
            [
                pal[i * 3] as i32,
                pal[i * 3 + 1] as i32,
                pal[i * 3 + 2] as i32,
            ]
        };
        let key = &movie.frames[0];
        for i in 0..w * h {
            // Keyframe index 0 = transparent (unpainted holes in
            // the hourglass art — retail never shows them; the black
            // outline specks).
            if mask[i] != 11 || visible[i] || key[i] == 0 {
                continue;
            }
            let a = rgb(key[i] as usize);
            let b = rgb(bg[i] as usize);
            let d: i32 = (0..3).map(|k| (a[k] - b[k]).abs()).sum();
            if d > 15 {
                visible[i] = true;
            }
        }
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        for y in 0..h {
            for x in 0..w {
                if visible[y * w + x] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        let (cw, ch) = (x1 + 1 - x0, y1 + 1 - y0);
        let mut bin = Vec::with_capacity(movie.frames.len() * cw * ch);
        for fr in &movie.frames {
            for y in y0..=y1 {
                bin.extend_from_slice(&fr[y * w + x0..y * w + x1 + 1]);
            }
        }
        let mut mbin = Vec::with_capacity(cw * ch);
        for y in y0..=y1 {
            for x in x0..=x1 {
                mbin.push(visible[y * w + x] as u8);
            }
        }
        emit("timer.bin", &bin)?;
        emit("timer-mask.bin", &mbin)?;
        emit(
            "timer.json",
            &serde_json::to_vec_pretty(&serde_json::json!({
                "x": x0, "y": y0, "w": cw, "h": ch,
                "frames": movie.frames.len(),
            }))
            .expect("timer meta serializes"),
        )?;
        sources.push(BundleSource {
            file: "TIMER.DAT".into(),
            sha256: hex(&Sha256::digest(&raw)),
        });
    }

    // SCROLL.DAT: the save/load dialog's parchment screen — the
    // fully-unrolled LAST frame over BLACK (retail plays the unroll
    // movie on a cleared screen — the dialog is NOT an overlay on
    // the menu).
    let scroll_raw = get("DATA/SCREENS/SCROLL.DAT")?;
    let mut scroll = crate::fmv::decode(&scroll_raw, None)
        .map_err(|e| BakeError::Level(Path::new("SCROLL.DAT").to_path_buf(), 0, e))?;
    remap_frames(&mut scroll);
    let last = scroll
        .frames
        .last()
        .ok_or_else(|| {
            BakeError::Level(
                Path::new("SCROLL.DAT").to_path_buf(),
                0,
                "scroll movie has no frames".into(),
            )
        })?
        .clone();
    emit("scroll-bg.bin", &last)?;
    sources.push(BundleSource {
        file: "SCROLL.DAT".into(),
        sha256: hex(&Sha256::digest(&scroll_raw)),
    });

    // PPERF: the end-of-level performance screen (sub_4E5B0 :60003-
    // 60010 — PPERF.DAT over PPERF.PAL, menu state 5 after the level
    // FMV). Optional like the scroll: an older bake just has no
    // stats backdrop.
    if src.exists("DATA/SCREENS/PPERF.DAT") && src.exists("DATA/SCREENS/PPERF.PAL") {
        let pbg = get("DATA/SCREENS/PPERF.DAT")?;
        let ppal = get("DATA/SCREENS/PPERF.PAL")?;
        if pbg.len() == 64_000 && ppal.len() == 768 {
            emit("pperf-bg.bin", &pbg)?;
            emit("pperf-pal.bin", &ppal)?;
            // SFONT2: the screen's font (:60012-17, `dword_96894 =
            // SFont2Tab + 6` — glyph id = char − 31, SFONT1's layout),
            // drawn in its own colours through PPERF.PAL.
            if src.exists("DATA/SCREENS/SFONT2.DAT") && src.exists("DATA/SCREENS/SFONT2.TAB") {
                let dat = get("DATA/SCREENS/SFONT2.DAT")?;
                let tab = get("DATA/SCREENS/SFONT2.TAB")?;
                let glyphs = crate::hspr::decode(&dat, &tab).map_err(|e| {
                    BakeError::Level(
                        Path::new("DATA/SCREENS/SFONT2.DAT").to_path_buf(),
                        0,
                        e.to_string(),
                    )
                })?;
                let packed = sprites::pack(&glyphs, UI_ATLAS_WIDTH);
                emit("sfont2.bin", &packed.atlas)?;
                emit(
                    "sfont2.json",
                    &serde_json::to_vec_pretty(&packed.index).expect("sfont2 index serializes"),
                )?;
                sources.push(BundleSource {
                    file: "SFONT2.DAT".into(),
                    sha256: hex(&Sha256::digest(&dat)),
                });
            }
            for f in ["PPERF.DAT", "PPERF.PAL"] {
                sources.push(BundleSource {
                    file: f.into(),
                    sha256: hex(&Sha256::digest(&get(&format!("DATA/SCREENS/{f}"))?)),
                });
            }
        } else {
            eprintln!(
                "note: mc1: PPERF sizes bg {} pal {} — skipping the stats backdrop",
                pbg.len(),
                ppal.len()
            );
        }
    }

    // PMULTI: the multiplayer lobby (frontend selector 4,
    // `sub_4D680_4D9C0`; docs/traces/mc1-multiplayer-lobby.md §2, §8),
    // epoch 29. The 320×200 background + palette, the PMULTSPR seat
    // icons (26 entries, 640-space: [1-8] off, [9-16] idle, [17-24]
    // connected) and SFONT1, the lobby's text font (`sub_51480`,
    // :59763 — also the MC1 movie subtitle font, baked separately
    // into `mc1-movies`). Optional: an older install just has no
    // lobby art.
    let pmulti = [
        "PMULTI.DAT",
        "PMULTI.PAL",
        "PMULTSPR.DAT",
        "PMULTSPR.TAB",
        "SFONT1.DAT",
        "SFONT1.TAB",
    ];
    if pmulti
        .iter()
        .all(|f| src.exists(&format!("DATA/SCREENS/{f}")))
    {
        let lbg = get("DATA/SCREENS/PMULTI.DAT")?;
        let lpal = get("DATA/SCREENS/PMULTI.PAL")?;
        if lbg.len() != 64_000 || lpal.len() != 768 {
            return Err(BakeError::Level(
                Path::new("DATA/SCREENS/PMULTI.DAT").to_path_buf(),
                0,
                format!("unexpected sizes: bg {} pal {}", lbg.len(), lpal.len()),
            ));
        }
        emit("pmulti-bg.bin", &lbg)?;
        emit("pmulti-pal.bin", &lpal)?;
        for (stem, out) in [("PMULTSPR", "pmulti-sprites"), ("SFONT1", "sfont1")] {
            let rel = format!("DATA/SCREENS/{stem}.DAT");
            let dat = get(&rel)?;
            let tab = get(&format!("DATA/SCREENS/{stem}.TAB"))?;
            let decoded = crate::hspr::decode(&dat, &tab)
                .map_err(|e| BakeError::Level(Path::new(&rel).to_path_buf(), 0, e.to_string()))?;
            let packed = sprites::pack(&decoded, UI_ATLAS_WIDTH);
            emit(&format!("{out}.bin"), &packed.atlas)?;
            emit(
                &format!("{out}.json"),
                &serde_json::to_vec_pretty(&packed.index).expect("sprite index serializes"),
            )?;
        }
        for f in pmulti {
            sources.push(BundleSource {
                file: f.into(),
                sha256: hex(&Sha256::digest(&get(&format!("DATA/SCREENS/{f}"))?)),
            });
        }
    } else {
        eprintln!("note: mc1: no DATA/SCREENS/PMULTI* — skipping the lobby art");
    }

    // The lobby's arena names (`off_99A84[row]`, drawn in the map
    // list): MC1 levels 50-69, HW 50-59. Optional like the world names.
    let mut arenas = serde_json::Map::new();
    for (key, exe, count) in [("mc1", "CARPET.EXE", 20), ("mc1hw", "HIDDEN.EXE", 10)] {
        let Ok(raw) = src.read(exe) else { continue };
        match exe_arena_names(&raw, count) {
            Some(list) => {
                arenas.insert(key.into(), list.into());
            }
            None => eprintln!("note: mc1: no arena-name table in {exe}"),
        }
    }
    if !arenas.is_empty() {
        emit(
            "arena-names.json",
            &serde_json::to_vec_pretty(&arenas).expect("arena names serialize"),
        )?;
    }

    // The world names the performance screen titles itself with
    // (retail `dword_999B8[level]`, drawn at (40, 8)): "1. Al Jahan" ..
    // "50. Volcania", string literals in each EXE's data object right
    // after "Text Omitted". HIDDEN.EXE carries its own list. Optional
    // — the app falls back to "Level N".
    let mut names = serde_json::Map::new();
    for (key, exe) in [("mc1", "CARPET.EXE"), ("mc1hw", "HIDDEN.EXE")] {
        let Ok(raw) = src.read(exe) else { continue };
        match exe_level_names(&raw) {
            Some(list) => {
                names.insert(key.into(), list.into());
                sources.push(BundleSource {
                    file: exe.into(),
                    sha256: hex(&Sha256::digest(&raw)),
                });
            }
            None => eprintln!("note: mc1: no level-name table in {exe}"),
        }
    }
    if !names.is_empty() {
        emit(
            "level-names.json",
            &serde_json::to_vec_pretty(&names).expect("level names serialize"),
        )?;
    }

    // FONT1 glyph masks (slot labels, dialog text).
    let dat = get("DATA/FONT1.DAT")?;
    let tab = get("DATA/FONT1.TAB")?;
    let glyphs = crate::hspr::decode(&dat, &tab).map_err(|e| {
        BakeError::Level(Path::new("DATA/FONT1.DAT").to_path_buf(), 0, e.to_string())
    })?;
    let fpacked = sprites::pack(&glyphs, UI_ATLAS_WIDTH);
    emit("font.bin", &fpacked.atlas)?;
    emit(
        "font.json",
        &serde_json::to_vec_pretty(&fpacked.index).expect("font index serializes"),
    )?;

    let manifest = BundleManifest {
        format_version: BUNDLE_VERSION,
        bake_epoch: mgc_formats::BAKE_EPOCH,
        variant: "mc1-ui".to_string(),
        game: Game::MagicCarpet1,
        importer: Importer {
            name: "mgc-import".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        sources,
    };
    emit(
        "bundle.json",
        &serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
    )?;
    Ok(outputs
        .into_iter()
        .map(|(name, sha)| (format!("assets/mc1-ui/{name}"), sha))
        .collect())
}

/// The full-screen FMV movies → `assets/<variant>/`: every Bullfrog
/// stream under `INTRO/`, copied RAW plus a [`MovieIndex`].
///
/// This is the one bundle that does not translate its input. A decoded
/// frame is 64 KB and MC1's `INTRO.DAT` is 3165 of them — pre-decoding
/// would turn a 75 MB stream into 200 MB of canvases nothing can hold,
/// so the engine keeps the original bytes and decodes one frame at a
/// time (`fmv::FmvCursor`). The whole set is ~92 MB for MC1 and ~140 MB
/// for MC2; that is local disk only, since bundles are baked from the
/// player's own install and never shipped.
///
/// Streams are keyed by lowercased source stem (`intro`, `outro`,
/// `logo`, `cut1`, `levelw1`); which movie plays when is the engine's
/// business, not the importer's, so no role mapping is baked in.
pub fn bake_movies(
    src: &GameSource,
    out_dir: &Path,
    variant: &str,
    game: Game,
) -> Result<Vec<(String, String)>, BakeError> {
    // Header-screen the whole catalog rather than naming files: the
    // two games' INTRO sets differ, and a stream we cannot decode is
    // one we must not index.
    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    for rel in src.list() {
        if !rel.to_uppercase().starts_with("INTRO/") {
            continue;
        }
        let Ok(raw) = src.read(&rel) else { continue };
        if crate::fmv::header(&raw).is_some() {
            found.push((rel, raw));
        }
    }
    if found.is_empty() {
        eprintln!("note: {variant}: no FMV streams under INTRO/ — skipping movie bundle");
        return Ok(Vec::new());
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));

    let dir = out_dir.join("assets").join(variant);
    let movie_dir = dir.join("movies");
    std::fs::create_dir_all(&movie_dir).map_err(|e| BakeError::Io(movie_dir.clone(), e))?;
    let mut outputs = Vec::new();
    let mut sources = Vec::new();
    let mut emit = |name: &str, bytes: &[u8]| -> Result<(), BakeError> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| BakeError::Io(path, e))?;
        outputs.push((name.to_string(), hex(&Sha256::digest(bytes))));
        Ok(())
    };

    let mut index = MovieIndex { movies: Vec::new() };
    for (rel, raw) in &found {
        let (width, height, frames) = crate::fmv::header(raw).expect("header re-parses");
        let name = Path::new(rel)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let file = format!("movies/{name}.fmv");
        emit(&file, raw)?;
        sources.push(BundleSource {
            file: rel.clone(),
            sha256: hex(&Sha256::digest(raw)),
        });
        index.movies.push(MovieEntry {
            name,
            file,
            frames: frames as u32,
            width: width as u32,
            height: height as u32,
            source: rel.clone(),
        });
    }
    emit(
        "movies.json",
        &serde_json::to_vec_pretty(&index).expect("movie index serializes"),
    )?;

    // The subtitle strip's font and text, so the movie bundle is
    // self-contained. The two games use DIFFERENT movie fonts: MC1
    // draws the narration subtitles with SFONT1 (`sub_24AB0`,
    // remc1:27911) — a bigger, outlined font than the in-game FONT1 —
    // while MC2 loads a dedicated 7×8 monospace font out of
    // HSCREEN0.DAT before every FMV (`hscreen::fmv_font`; remc2
    // `Intros_76D10` / `PlayInGameFmv_82670`). Both are emitted in
    // the same `char - 31` glyph-id space so the player's lookup is
    // uniform (SFONT1 is authored that way; the FMV font's raw-ASCII
    // records are re-indexed here).
    let font_rel = if src.exists("DATA/SCREENS/SFONT1.DAT") {
        "DATA/SCREENS/SFONT1"
    } else {
        "DATA/SFONT1"
    };
    let mc2_font_rel = "DATA/SCREENS/HSCREEN0.DAT";
    let mc2 = game == Game::MagicCarpet2;
    if src.exists(&format!("{font_rel}.DAT")) || (mc2 && src.exists(mc2_font_rel)) {
        let get = |rel: &str| -> Result<Vec<u8>, BakeError> {
            let raw = src
                .read(rel)
                .map_err(|e| BakeError::Io(Path::new(rel).to_path_buf(), e))?;
            if crate::rnc::is_rnc(&raw) {
                crate::rnc::decompress(&raw).map_err(|e| {
                    BakeError::Level(Path::new(rel).to_path_buf(), 0, format!("RNC: {e:?}"))
                })
            } else {
                Ok(raw)
            }
        };
        let glyphs = if mc2 {
            let raw = src
                .read(mc2_font_rel)
                .map_err(|e| BakeError::Io(Path::new(mc2_font_rel).to_path_buf(), e))?;
            let decoded = crate::hscreen::fmv_font(&raw)
                .map_err(|e| BakeError::Level(Path::new(mc2_font_rel).to_path_buf(), 0, e))?;
            sources.push(BundleSource {
                file: mc2_font_rel.into(),
                sha256: hex(&Sha256::digest(&raw)),
            });
            // Records are raw-ASCII indexed; shift into the `char - 31`
            // id space (ASCII 32 → id 1, matching SFONT1's layout) and
            // normalize the raster to the mask convention (the pool
            // stores a flat palette index — 150 — that retail never
            // shows: `DrawText_7FB90` sets flag 64 so every visible
            // pixel is repainted in the caller's ink by
            // `GameBitmap::DrawColourizedBitmap`).
            decoded
                .into_iter()
                .skip(31)
                .enumerate()
                .map(|(i, mut s)| {
                    s.index = i;
                    s.group = i;
                    for f in &mut s.frames {
                        for v in f.iter_mut() {
                            *v = (*v != 0) as u8;
                        }
                    }
                    s
                })
                .collect()
        } else {
            let dat = get(&format!("{font_rel}.DAT"))?;
            let tab = get(&format!("{font_rel}.TAB"))?;
            let decoded = crate::hspr::decode(&dat, &tab).map_err(|e| {
                BakeError::Level(Path::new(font_rel).to_path_buf(), 0, e.to_string())
            })?;
            sources.push(BundleSource {
                file: format!("{font_rel}.DAT"),
                sha256: hex(&Sha256::digest(&dat)),
            });
            decoded
        };
        let packed = sprites::pack(&glyphs, UI_ATLAS_WIDTH);
        emit("font.bin", &packed.atlas)?;
        emit(
            "font.json",
            &serde_json::to_vec_pretty(&packed.index).expect("font index serializes"),
        )?;

        // The lines themselves. The scripts' subtitle keys are indices
        // into the game's own string table: MC1's is `DATA/ETEXT.DAT`
        // (80 NUL-terminated strings, the intro narration at 0..=16 —
        // remc1 `sub_44700_44A40`), MC2's is the 471-entry
        // `LANGUAGE/L2.TXT` bank (English; L1 is French), which the
        // cutscenes index directly.
        let (text_rel, strings) = if src.exists("DATA/ETEXT.DAT") {
            let raw = get("DATA/ETEXT.DAT")?;
            let mut entries: Vec<String> = raw
                .split(|&b| b == 0)
                .map(|s| s.iter().map(|&b| b as char).collect())
                .collect();
            while entries.last().is_some_and(String::is_empty) {
                entries.pop();
            }
            ("DATA/ETEXT.DAT", entries)
        } else {
            let raw = get("LANGUAGE/L2.TXT")?;
            ("LANGUAGE/L2.TXT", crate::hscreen::language_strings(&raw))
        };
        emit(
            "subtitles.json",
            &serde_json::to_vec_pretty(&strings).expect("subtitles serialize"),
        )?;
        sources.push(BundleSource {
            file: text_rel.into(),
            sha256: hex(&Sha256::digest(&get(text_rel)?)),
        });
    }

    let manifest = BundleManifest {
        format_version: BUNDLE_VERSION,
        bake_epoch: mgc_formats::BAKE_EPOCH,
        variant: variant.to_string(),
        game,
        importer: Importer {
            name: "mgc-import".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        sources,
    };
    emit(
        "bundle.json",
        &serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
    )?;
    Ok(outputs
        .into_iter()
        .map(|(name, sha)| (format!("assets/{variant}/{name}"), sha))
        .collect())
}

/// The MC2 world-map (campaign) screen bundle → `assets/mc2-ui/`:
/// the 1280×960 map background, its 6-bit palette, and the packed
/// frontend sprite bank (portals/flags/cursor) out of HSCREEN0.DAT
/// (`hscreen::worldmap` — remc2 `sub_7A110_load_hscreen` case 6).
/// One per game, not per environment. Skips quietly on legacy
/// installs without the SCREENS catalog.
pub fn bake_mc2_worldmap(
    src: &GameSource,
    out_dir: &Path,
) -> Result<Vec<(String, String)>, BakeError> {
    let rel = "DATA/SCREENS/HSCREEN0.DAT";
    if !src.exists(rel) {
        eprintln!("note: mc2: no {rel} — skipping world-map bundle");
        return Ok(Vec::new());
    }
    let file = src
        .read(rel)
        .map_err(|e| BakeError::Io(Path::new(rel).to_path_buf(), e))?;
    let wm = crate::hscreen::worldmap(&file)
        .map_err(|e| BakeError::Level(Path::new(rel).to_path_buf(), 0, e))?;

    let dir = out_dir.join("assets").join("mc2-ui");
    std::fs::create_dir_all(&dir).map_err(|e| BakeError::Io(dir.clone(), e))?;
    let mut outputs = Vec::new();
    let mut emit = |name: &str, bytes: &[u8]| -> Result<(), BakeError> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| BakeError::Io(path, e))?;
        outputs.push((name.to_string(), hex(&Sha256::digest(bytes))));
        Ok(())
    };

    emit("worldmap-bg.bin", &wm.bg)?;
    emit("worldmap-pal.bin", &wm.palette)?;
    let packed = sprites::pack(&wm.sprites, UI_ATLAS_WIDTH);
    emit("worldmap-sprites.bin", &packed.atlas)?;
    emit(
        "worldmap-sprites.json",
        &serde_json::to_vec_pretty(&packed.index).expect("world-map sprite index serializes"),
    )?;

    // The map screen's ornate border frame (drawn over the map every
    // frame, retail sub_85CC3): 640×480 8bpp, 0 = transparent.
    let border = crate::hscreen::border_frame(&file)
        .map_err(|e| BakeError::Level(Path::new(rel).to_path_buf(), 0, e))?;
    emit("worldmap-border.bin", &border)?;

    // The main-menu (temple) screen — HSCREEN0 case 4: its own
    // 640×480 background + palette + 111-sprite bank (buttons,
    // fires/incense anims, cursor 39, parchment dialog parts).
    let menu = crate::hscreen::mainmenu(&file)
        .map_err(|e| BakeError::Level(Path::new(rel).to_path_buf(), 0, e))?;
    emit("menu-bg.bin", &menu.bg)?;
    emit("menu-pal.bin", &menu.palette)?;
    let mpacked = sprites::pack(&menu.sprites, UI_ATLAS_WIDTH);
    emit("menu-sprites.bin", &mpacked.atlas)?;
    emit(
        "menu-sprites.json",
        &serde_json::to_vec_pretty(&mpacked.index).expect("menu sprite index serializes"),
    )?;

    // The multiplayer lobby — HSCREEN0 case 7 (epoch 28): its own
    // 640×480 background + palette + 33-sprite bank, and the eight
    // wizard portraits (pixels back to back, each with its own
    // palette; the rects in `lobby-portraits.json`).
    let lobby = crate::hscreen::lobby(&file)
        .map_err(|e| BakeError::Level(Path::new(rel).to_path_buf(), 0, e))?;
    emit("lobby-bg.bin", &lobby.bg)?;
    emit("lobby-pal.bin", &lobby.palette)?;
    let lpacked = sprites::pack(&lobby.sprites, UI_ATLAS_WIDTH);
    emit("lobby-sprites.bin", &lpacked.atlas)?;
    emit(
        "lobby-sprites.json",
        &serde_json::to_vec_pretty(&lpacked.index).expect("lobby sprite index serializes"),
    )?;
    let mut px = Vec::new();
    let mut pals = Vec::new();
    let mut rects = Vec::new();
    for p in &lobby.portraits {
        rects.push(serde_json::json!({
            "x": p.x, "y": p.y, "w": p.w, "h": p.h, "offset": px.len(),
        }));
        px.extend_from_slice(&p.px);
        pals.extend_from_slice(&p.palette);
    }
    emit("lobby-portraits.bin", &px)?;
    emit("lobby-portrait-pals.bin", &pals)?;
    emit(
        "lobby-portraits.json",
        &serde_json::to_vec_pretty(&rects).expect("portrait rects serialize"),
    )?;

    // FONT1 glyph masks (GetFont(1) — the frontend text font: level
    // descriptions, dialog titles, slot labels). Same shape as the
    // per-variant font.bin: 1-bit coverage masks, glyph id = char+1,
    // tinted app-side.
    let mut extra_sources = Vec::new();
    for f in ["DATA/FONT1.DAT", "DATA/FONT1.TAB"] {
        if !src.exists(f) {
            eprintln!("note: mc2: no {f} — world-map bundle ships without a frontend font");
        }
    }
    if src.exists("DATA/FONT1.DAT") && src.exists("DATA/FONT1.TAB") {
        let dat = src
            .read("DATA/FONT1.DAT")
            .map_err(|e| BakeError::Io(Path::new("DATA/FONT1.DAT").to_path_buf(), e))?;
        let tab = src
            .read("DATA/FONT1.TAB")
            .map_err(|e| BakeError::Io(Path::new("DATA/FONT1.TAB").to_path_buf(), e))?;
        let glyphs = crate::hspr::decode(&dat, &tab).map_err(|e| {
            BakeError::Level(Path::new("DATA/FONT1.DAT").to_path_buf(), 0, e.to_string())
        })?;
        let fpacked = sprites::pack(&glyphs, UI_ATLAS_WIDTH);
        emit("font.bin", &fpacked.atlas)?;
        emit(
            "font.json",
            &serde_json::to_vec_pretty(&fpacked.index).expect("font index serializes"),
        )?;
        extra_sources.push(BundleSource {
            file: "FONT1.DAT".into(),
            sha256: hex(&Sha256::digest(&dat)),
        });
    }

    // The frontend language strings (LANGUAGE/L2.TXT = English): a
    // 4785-byte header, then NUL-separated strings, 471 entries
    // (retail sub_5B870). Level descriptions = entries 23+level;
    // "Empty" = 414; dialog titles 421/422/467.
    if src.exists("LANGUAGE/L2.TXT") {
        let raw = src
            .read("LANGUAGE/L2.TXT")
            .map_err(|e| BakeError::Io(Path::new("LANGUAGE/L2.TXT").to_path_buf(), e))?;
        let strings = crate::hscreen::language_strings(&raw);
        emit(
            "strings.json",
            &serde_json::to_vec_pretty(&strings).expect("strings serialize"),
        )?;
        extra_sources.push(BundleSource {
            file: "L2.TXT".into(),
            sha256: hex(&Sha256::digest(&raw)),
        });
    } else {
        eprintln!("note: mc2: no LANGUAGE/L2.TXT — world-map bundle ships without strings");
    }

    let manifest = BundleManifest {
        format_version: BUNDLE_VERSION,
        bake_epoch: mgc_formats::BAKE_EPOCH,
        variant: "mc2-ui".to_string(),
        game: Game::MagicCarpet2,
        importer: Importer {
            name: "mgc-import".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        sources: {
            let mut s = vec![BundleSource {
                file: "HSCREEN0.DAT".into(),
                sha256: hex(&Sha256::digest(&file)),
            }];
            s.extend(extra_sources);
            s
        },
    };
    emit(
        "bundle.json",
        &serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
    )?;
    Ok(outputs
        .into_iter()
        .map(|(name, sha)| (format!("assets/mc2-ui/{name}"), sha))
        .collect())
}

fn bake_variant(
    src: &GameSource,
    dir: &Path,
    spec: &VariantSpec,
) -> Result<Vec<(String, String)>, BakeError> {
    let mut outputs = Vec::new();
    let mut sources = Vec::new();

    let mut emit = |name: &str, bytes: &[u8]| -> Result<(), BakeError> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| BakeError::Io(path, e))?;
        outputs.push((name.to_string(), hex(&Sha256::digest(bytes))));
        Ok(())
    };
    // Read + record provenance; decompress whole-file RNC when present
    // (several catalogs and the TMAPS TABs ship raw).
    let source = |rel: &str, sources: &mut Vec<BundleSource>| -> Result<Vec<u8>, BakeError> {
        let raw = src
            .read(rel)
            .map_err(|e| BakeError::Io(Path::new(rel).to_path_buf(), e))?;
        sources.push(BundleSource {
            file: rel.rsplit('/').next().unwrap_or(rel).to_string(),
            sha256: hex(&Sha256::digest(&raw)),
        });
        if crate::rnc::is_rnc(&raw) {
            crate::rnc::decompress(&raw)
                .map_err(|e| BakeError::Level(Path::new(rel).to_path_buf(), 0, e.to_string()))
        } else {
            Ok(raw)
        }
    };
    let expect = |rel: &str, data: &[u8], len: usize| -> Result<(), BakeError> {
        if data.len() != len {
            return Err(BakeError::Level(
                Path::new(rel).to_path_buf(),
                0,
                format!("{} bytes, expected {len}", data.len()),
            ));
        }
        Ok(())
    };

    // Palette: 6-bit VGA -> RGBA8, index 0 transparent.
    let vga = source(spec.palette, &mut sources)?;
    expect(spec.palette, &vga, 768)?;
    let mut rgba = Vec::with_capacity(1024);
    for (i, c) in vga.chunks_exact(3).enumerate() {
        for &v in c {
            rgba.push((v << 2) | (v >> 4));
        }
        rgba.push(if i == 0 { 0 } else { 255 });
    }
    emit("palette.bin", &rgba)?;

    // Color tables: shade LUT at the game's offset (see
    // VariantSpec::shade_offset), tile-type→map-color at +0x14000 in
    // both games.
    let tables = source(spec.tables, &mut sources)?;
    expect(spec.tables, &tables, TABLES_LEN)?;
    emit(
        "shade-lut.bin",
        &tables[spec.shade_offset..spec.shade_offset + SHADE_LUT_LEN],
    )?;
    emit(
        "tile-colors.bin",
        &tables[TILE_COLORS_OFFSET..TILE_COLORS_OFFSET + 256],
    )?;

    // Terrain atlas.
    let atlas = source(spec.atlas, &mut sources)?;
    expect(
        spec.atlas,
        &atlas,
        (ATLAS_WIDTH * ATLAS_CELL * ATLAS_CELLS.div_ceil(ATLAS_WIDTH / ATLAS_CELL)) as usize,
    )?;
    emit("terrain-atlas.bin", &atlas)?;
    emit(
        "terrain-atlas.json",
        &serde_json::to_vec_pretty(&TerrainAtlasInfo {
            cell: ATLAS_CELL,
            width: ATLAS_WIDTH,
            cells: ATLAS_CELLS,
        })
        .expect("terrain atlas info serializes"),
    )?;

    // World sprites.
    let tmaps_dat_file = format!("{}.DAT", spec.tmaps);
    let tmaps_dat = source(&tmaps_dat_file, &mut sources)?;
    let tmaps_tab = source(&format!("{}.TAB", spec.tmaps), &mut sources)?;
    let archive = TmapsArchive::open(&tmaps_dat, &tmaps_tab).map_err(|e| {
        BakeError::Level(Path::new(&tmaps_dat_file).to_path_buf(), 0, e.to_string())
    })?;
    let (decoded, warnings) = sprites::decode_tmaps(&archive).map_err(|e| {
        BakeError::Level(Path::new(&tmaps_dat_file).to_path_buf(), 0, e.to_string())
    })?;
    for w in warnings {
        eprintln!("note: {}: {w}", spec.variant);
    }
    let mut atlas_width = SPRITE_ATLAS_WIDTH;
    let mut packed = sprites::pack(&decoded, atlas_width);
    while packed.index.atlas_height > MAX_TEXTURE_DIM && atlas_width < MAX_TEXTURE_DIM {
        atlas_width *= 2;
        packed = sprites::pack(&decoded, atlas_width);
    }
    emit("sprites.bin", &packed.atlas)?;
    emit(
        "sprites.json",
        &serde_json::to_vec_pretty(&packed.index).expect("sprite index serializes"),
    )?;
    // The renderer's world art: the bank as impostors (asset
    // corrections applied, own tight atlas) — `crate::impostors`.
    let pack = impostors::bake(
        spec.game,
        &packed.index,
        &packed.atlas,
        SPRITE_ATLAS_WIDTH,
        MAX_TEXTURE_DIM,
    );
    pack.validate()
        .map_err(|e| BakeError::Level(Path::new(&tmaps_dat_file).to_path_buf(), 0, e))?;
    emit(ImpostorPack::PIXELS_FILE, &pack.pixels)?;
    emit(
        ImpostorPack::INDEX_FILE,
        &serde_json::to_vec(&pack.index).expect("impostor index serializes"),
    )?;

    // Ring search-order table — the same 1024-byte 32x32 format in
    // both games (remc2 sub_101C0).
    if let Some(sp) = spec.search {
        let search = source(sp, &mut sources)?;
        expect(sp, &search, 1024)?;
        emit("search.bin", &search)?;
    }

    // Terrain-feature/building data, MC1 flavor (BUILD .TAB/.DAT).
    if let Some(build) = spec.build {
        let tab_file = format!("{build}.TAB");
        let tab = source(&tab_file, &mut sources)?;
        if tab.len() % 6 != 0 {
            return Err(BakeError::Level(
                Path::new(&tab_file).to_path_buf(),
                0,
                format!("{} bytes is not 6-byte entries", tab.len()),
            ));
        }
        emit("build.tab.bin", &tab)?;
        let build_dat = source(&format!("{build}.DAT"), &mut sources)?;
        emit("build.dat.bin", &build_dat)?;
    }

    // Building parameters, MC2 flavor (BLDGPRM 4-byte records; remc2
    // loads 76 records into a 77-slot table, sub_539A0 :38328).
    if let Some(bp) = spec.bldgprm {
        let bldgprm = source(bp, &mut sources)?;
        if bldgprm.len() % 4 != 0 {
            return Err(BakeError::Level(
                Path::new(bp).to_path_buf(),
                0,
                format!("{} bytes is not 4-byte records", bldgprm.len()),
            ));
        }
        emit("bldgprm.bin", &bldgprm)?;
    }

    // Spell table, MC2 flavor (SPELLS.DAT verbatim: 26 rows x 80
    // bytes — remc2 Spells.h layout, loaded by the Basic.cpp:334
    // Pathstruct over the source's baked-in fallback).
    if let Some(sp) = spec.spells {
        let spells = source(sp, &mut sources)?;
        expect(sp, &spells, 26 * 80)?;
        emit("spells.bin", &spells)?;
    }

    // Minimap colour table, MC2 flavor (CLR{D,N,C}-0.DAT verbatim).
    if let Some(c) = spec.clrd {
        let clrd = source(c, &mut sources)?;
        expect(c, &clrd, 4096)?;
        emit("clrd.bin", &clrd)?;
    }

    // UI sprites (HSPR) + the book screen palette (MC1 only — MC2 has
    // no book screen; its CTRL selector pane draws over the live
    // frame with the variant palette). MC2's blend LUT sits at the
    // same +0x4000..+0x14000 slice of its TABLES{D,N,C} (remc2
    // GameUI.cpp:525/1105: `tablesx[0x4000 + 256*dest + src]`).
    if let Some(ui) = spec.ui {
        let dat_file = format!("{ui}.DAT");
        let dat = source(&dat_file, &mut sources)?;
        let tab = source(&format!("{ui}.TAB"), &mut sources)?;
        // The atlas composite treats palette index 0 as transparent, so
        // a sprite drawn purely in index 0 (the black-shape map X marker,
        // HSPR 83) would vanish. Hand the decoder the palette's OPAQUE
        // black (index 0 carries alpha 0; the first non-zero index whose
        // RGB is 0,0,0 — MC1 PAL0-0 slot 160) so those drawn pixels move
        // off the transparent index while the RLE skip runs stay clear.
        let opaque_black = (1..256)
            .find(|&i| rgba[i * 4..i * 4 + 3] == [0, 0, 0])
            .unwrap_or(0) as u8;
        let mut decoded = crate::hspr::decode_opaque(&dat, &tab, opaque_black)
            .map_err(|e| BakeError::Level(Path::new(&dat_file).to_path_buf(), 0, e.to_string()))?;
        // The retail MOUSE POINTERS (DATA/POINTERS, a tiny HSPR bank
        // BOTH games ship) append at the tail of the UI bank, their
        // start recorded as `pointer_base` — so the in-level surfaces
        // can draw the real cursor. Entry law: [0] is a null "don't
        // draw" sentinel in both games; MC1 [1] = the arrow+mana-ball
        // every view mode installs (sub_5C05C, remc1 :42024/:49068;
        // [2..=8] the menu HAND per menu-screen palette); MC2 picks
        // per map type — day [1], night [9], cave [10]
        // (SetCursor_8CD27 / LevelInit.cpp:24-38).
        let mut pointer_base = 0u32;
        if src.exists("DATA/POINTERS.DAT") {
            pointer_base = decoded.len() as u32;
            let pdat = source("DATA/POINTERS.DAT", &mut sources)?;
            let ptab = source("DATA/POINTERS.TAB", &mut sources)?;
            decoded.extend(crate::hspr::decode(&pdat, &ptab).map_err(|e| {
                BakeError::Level(
                    Path::new("DATA/POINTERS.DAT").to_path_buf(),
                    0,
                    e.to_string(),
                )
            })?);
        } else {
            eprintln!(
                "note: {}: no DATA/POINTERS.DAT — no retail cursor",
                spec.variant
            );
        }
        let mut packed = sprites::pack(&decoded, UI_ATLAS_WIDTH);
        packed.index.pointer_base = pointer_base;
        emit("ui-sprites.bin", &packed.atlas)?;
        emit(
            "ui-sprites.json",
            &serde_json::to_vec_pretty(&packed.index).expect("ui sprite index serializes"),
        )?;

        // The UI blend LUT: TABLES.DAT's middle 64KB (+0x4000..+0x14000),
        // the slice between the shade LUT and the map colors. The
        // original's 2D blits resolve every pixel as
        // `blend[src | dest<<8]` (remc1 strPal.byte_BB934_BB924,
        // sub_main.cpp:27444/27564) — spell icons composite through it
        // against the book page, which is where their true colors
        // (e.g. the red heal heart) come from; raw icon indices are a
        // ramp that reads garish under any palette directly.
        emit("blend-lut.bin", &tables[SHADE_LUT_LEN..TILE_COLORS_OFFSET])?;

        if spec.game == Game::MagicCarpet1 {
            let book = source("DATA/BOOK.PAL", &mut sources)?;
            expect("DATA/BOOK.PAL", &book, 768)?;
            let mut rgba = Vec::with_capacity(1024);
            for (i, c) in book.chunks_exact(3).enumerate() {
                for &v in c {
                    rgba.push((v << 2) | (v >> 4));
                }
                rgba.push(if i == 0 { 0 } else { 255 });
            }
            emit("book-palette.bin", &rgba)?;
        }
    }

    // Messaging/notification font (HFONT3/FONT2): the same HSPR TAB/DAT
    // format, decoded to single-frame glyph sprites and packed into one
    // atlas. The glyphs are 1-bit coverage masks (every ink pixel =
    // index 1); the app appends them to its UI atlas as white and tints
    // per DrawText's `color` argument, so no palette bakes here — the
    // atlas is palette-independent. Sprite id for ASCII char `c` = c+1.
    if let Some(font) = spec.font {
        let dat_file = format!("{font}.DAT");
        let dat = source(&dat_file, &mut sources)?;
        let tab = source(&format!("{font}.TAB"), &mut sources)?;
        let decoded = crate::hspr::decode(&dat, &tab)
            .map_err(|e| BakeError::Level(Path::new(&dat_file).to_path_buf(), 0, e.to_string()))?;
        let packed = sprites::pack(&decoded, UI_ATLAS_WIDTH);
        emit("font.bin", &packed.atlas)?;
        emit(
            "font.json",
            &serde_json::to_vec_pretty(&packed.index).expect("font index serializes"),
        )?;
    }

    // The fullscreen WEB overlay bank (HWEB{D,N,C}0-0): the same HSPR
    // TAB/DAT format as the UI sprites — 8bpp indexed tiles
    // (transparent 0), a 6×4 grid of 24 equal tiles covering the
    // 640×480 viewport (sprite ids 1..=24). Retail tiles it over the
    // view while the paralyze web is live (remc2 EF:21668-710).
    // Palette-resolved app-side like the base UI sprites (NOT the
    // font's white-mask path).
    if let Some(web) = spec.web {
        let dat_file = format!("{web}.DAT");
        let dat = source(&dat_file, &mut sources)?;
        let tab = source(&format!("{web}.TAB"), &mut sources)?;
        let decoded = crate::hspr::decode(&dat, &tab)
            .map_err(|e| BakeError::Level(Path::new(&dat_file).to_path_buf(), 0, e.to_string()))?;
        let packed = sprites::pack(&decoded, UI_ATLAS_WIDTH);
        emit("web-sprites.bin", &packed.atlas)?;
        emit(
            "web-sprites.json",
            &serde_json::to_vec_pretty(&packed.index).expect("web sprite index serializes"),
        )?;
    }

    // Sentence bank (ETEXT.DAT): null-terminated strings, decoded with
    // indices preserved — empty slots stay as "" so sentence ids keep
    // their alignment (MC2's objective tables index this directly).
    // Bytes are DOS code page; decoded as Latin-1 (the shipped English
    // banks are plain ASCII).
    if let Some(et) = spec.etext {
        let raw = source(et, &mut sources)?;
        let mut entries: Vec<String> = raw
            .split(|&b| b == 0)
            .map(|s| s.iter().map(|&b| b as char).collect())
            .collect();
        // Trailing NUL terminators leave empty tail slots — split
        // artifacts, not sentences (MC2's file ends with a double
        // NUL). Interior empties stay: they keep ids aligned.
        while entries.last().is_some_and(String::is_empty) {
            entries.pop();
        }
        emit(
            "etext.json",
            &serde_json::to_vec_pretty(&entries).expect("etext serializes"),
        )?;
    }

    // Parallax sky bitmap: 256x256 8bpp raw (RNC-wrapped on MC1).
    if let Some(sky) = spec.sky {
        let bitmap = source(sky, &mut sources)?;
        expect(sky, &bitmap, 256 * 256)?;
        emit("sky.bin", &bitmap)?;
    }

    let manifest = BundleManifest {
        format_version: BUNDLE_VERSION,
        bake_epoch: mgc_formats::BAKE_EPOCH,
        variant: spec.variant.to_string(),
        game: spec.game,
        importer: Importer {
            name: "mgc-import".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        sources,
    };
    emit(
        "bundle.json",
        &serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
    )?;
    Ok(outputs)
}

#[cfg(test)]
mod level_name_tests {
    use super::exe_level_names;

    /// Both EXEs yield their 50 names through the real source layers
    /// (GOG: `CARPET/` inside the CD image). Skips without gamedata.
    #[test]
    fn world_names_from_the_install() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gamedata");
        let Some(src) = crate::gamedata::Gamedata::locate(&root).mc1 else {
            eprintln!("skip: no MC1 gamedata");
            return;
        };
        let mc1 = exe_level_names(&src.read("CARPET.EXE").unwrap()).expect("CARPET.EXE names");
        assert_eq!(
            (mc1[0].as_str(), mc1[49].as_str()),
            ("Al Jahan", "Volcania")
        );
        let hw = exe_level_names(&src.read("HIDDEN.EXE").unwrap()).expect("HIDDEN.EXE names");
        assert_eq!((hw[0].as_str(), hw[8].as_str()), ("Goyaan", "Rama'Q"));
        assert_eq!(hw[25..], mc1[25..], "HW reuses MC1's names from 26 on");
    }
}

#[cfg(test)]
mod awe32_rom_tests {
    /// The AWE32 wave ROM ships INSIDE the binary (`awe32_rom`,
    /// `include_bytes!`), so the release packaging copies nothing — but
    /// a file that is not the real compressed dump (a Git LFS pointer,
    /// a truncated upload) would still compile and only make the bake
    /// fall back to GM with a console note. CI catches it here.
    #[test]
    fn the_embedded_awe32_rom_is_the_full_dump() {
        let rom = super::awe32_rom().expect("the embedded ROM decompresses");
        assert_eq!(rom.len(), 1 << 20, "the AWE32 GM wave ROM is exactly 1 MB");
    }
}
