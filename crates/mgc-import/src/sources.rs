//! Game-data SOURCES: tell what a user-picked path is, and import it
//! into its canonical home under `gamedata/` — the back end of the
//! launcher's data loader (and of `mgc-import add`).
//!
//! Three kinds of source are understood, each found by CONTENT, never
//! by name:
//!
//! - a **GOG install** — the directory GOG's installer (or Wine) lays
//!   down; detected by [`crate::gamedata`]'s own detectors;
//! - a **GOG offline installer** (`setup_magic_carpet_….exe`) — an Inno
//!   Setup package, named by the GOG product id it carries (its
//!   `goggame-<id>.info` file) and unpacked IN PROCESS (`innospect` for
//!   the Inno container, our own reader for GOG's hashed, split, zlib
//!   file parts — see `GogFile`): no `innoextract`, no Wine, on every
//!   platform;
//! - a **retail CD dump** (MC2 so far) — a `.cue` and the raw image it
//!   names (CloneCD `.img`, or `.bin`).
//!
//! Importing copies (or extracts, then SLIMS to what the importer
//! reads) into a staging directory beside the destination, re-checks
//! the result with [`identify`], and only then moves it into place, so
//! a failed or cancelled import leaves nothing behind. The canonical
//! homes ([`Game::install_dir`], [`Game::installer_name`],
//! [`Game::cd_dir`]) are what the existing layout already uses, so
//! installs made by hand before the loader existed are untouched.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use crate::gamedata::{self, Gamedata};

/// The two products a source can hold. GOG's "Magic Carpet Plus"
/// carries MC1 and Hidden Worlds together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    Mc1Plus,
    Mc2,
}

impl Game {
    /// GOG's product id (the installer's `goggame-<id>.info`).
    pub const fn gog_id(self) -> &'static str {
        match self {
            Game::Mc1Plus => "1207658935",
            Game::Mc2 => "1207659187",
        }
    }

    fn from_gog_id(id: &str) -> Option<Game> {
        [Game::Mc1Plus, Game::Mc2]
            .into_iter()
            .find(|g| g.gog_id() == id)
    }

    pub const fn title(self) -> &'static str {
        match self {
            Game::Mc1Plus => "Magic Carpet Plus (Magic Carpet + Hidden Worlds)",
            Game::Mc2 => "Magic Carpet 2: The Netherworlds",
        }
    }

    /// The canonical GOG install directory under `gamedata/` — the
    /// names the hand-made layout has always used.
    pub const fn install_dir(self) -> &'static str {
        match self {
            Game::Mc1Plus => "Magic Carpet Plus",
            Game::Mc2 => "Magic Carpet 2",
        }
    }

    /// The canonical name of the GOG installer kept in `gamedata/`
    /// (player ruling 2026-11-15: GOG's own names carry a build number
    /// that changes with every repack).
    pub const fn installer_name(self) -> &'static str {
        match self {
            Game::Mc1Plus => "GOG-MagicCarpet1Plus.exe",
            Game::Mc2 => "GOG-MagicCarpet2.exe",
        }
    }

    /// The canonical directory for a retail CD dump.
    pub const fn cd_dir(self) -> &'static str {
        match self {
            Game::Mc1Plus => "Magic Carpet CD",
            Game::Mc2 => "Magic Carpet 2 CD",
        }
    }

    /// What a GOG install is slimmed to: everything the importer reads,
    /// plus the product record. (Install-root relative; a directory
    /// entry keeps its whole tree.)
    const fn keep(self) -> &'static [&'static str] {
        match self {
            Game::Mc1Plus => &["CARPET.CD", "goggame-1207658935.info"],
            Game::Mc2 => &["game.gog", "game.ins", "GAME", "goggame-1207659187.info"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    GogInstall,
    GogInstaller,
    CdImage,
}

impl SourceKind {
    pub const fn label(self) -> &'static str {
        match self {
            SourceKind::GogInstall => "GOG install",
            SourceKind::GogInstaller => "GOG installer",
            SourceKind::CdImage => "retail CD image",
        }
    }
}

/// What [`identify`] made of a path.
#[derive(Debug, Clone)]
pub struct Identified {
    pub game: Game,
    pub kind: SourceKind,
    /// The install directory, the installer, or the CD dump's `.cue`.
    pub path: PathBuf,
    /// Things worth telling the player (never blocking).
    pub notes: Vec<String>,
}

/// Why [`import`] stopped.
#[derive(Debug)]
pub enum ImportError {
    /// The canonical home is taken; pass `replace` to overwrite it.
    Exists(PathBuf),
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Exists(p) => write!(f, "{} already exists", p.display()),
            ImportError::Cancelled => write!(f, "cancelled"),
            ImportError::Failed(m) => f.write_str(m),
        }
    }
}

/// An I/O failure as a player-facing message; a full disk says so.
fn io_failed(what: &str, path: &Path, e: &io::Error) -> ImportError {
    let full = e.kind() == io::ErrorKind::StorageFull
        || e.raw_os_error() == Some(28) // ENOSPC
        || e.raw_os_error() == Some(112); // ERROR_DISK_FULL
    ImportError::Failed(if full {
        format!(
            "not enough disk space to {what} {} — free some space and try again",
            path.display()
        )
    } else {
        format!("could not {what} {}: {e}", path.display())
    })
}

/// The damaged-audio rip GOG ships for MC2 (and the CloneCD dump that
/// circulates is the same bytes plus the stored pregap): the SHA-256 of
/// the first 64 sectors of audio track 3, where its corrupt head block
/// sits (memory: gog-audio-corruption). Both images hash alike.
const DAMAGED_MC2_AUDIO_TRACK3_HEAD: &str =
    "ad1aa4d8c4f5c69ba5868e557b84e181a9be4b1ac4d4ec33dbe43e01a56e10da";

/// The audio note for an MC2 CD image, if it is the known damaged rip.
fn mc2_audio_note(image: &Path, cue: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let sectors = std::fs::metadata(image).ok()?.len() / crate::redbook::SECTOR;
    let text = std::fs::read_to_string(cue).ok()?;
    let tracks = crate::redbook::parse_cue(&text, sectors).ok()?;
    let t3 = tracks.iter().find(|t| t.number == 3)?;
    let mut f = std::fs::File::open(image).ok()?;
    f.seek(SeekFrom::Start(t3.start_sector * crate::redbook::SECTOR))
        .ok()?;
    let mut head = vec![0u8; 64 * crate::redbook::SECTOR as usize];
    f.read_exact(&mut head).ok()?;
    let hex: String = Sha256::digest(&head)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (hex == DAMAGED_MC2_AUDIO_TRACK3_HEAD).then(|| {
        "this is the widely shipped MC2 CD rip whose audio tracks have damaged \
         first seconds (the narration crackle); the game plays, but the \
         soundtrack and voiceover carry the damage"
            .to_owned()
    })
}

/// One file of a GOG installer, as GOG's own installer script lays it
/// down. GOG's ("Galaxy") installers store every game file under an
/// MD5-hash name in `{tmp}`, split into parts (10 MiB each), each part
/// zlib-compressed; the real name rides the FIRST part's Inno script
/// hook — `before_install('<file md5>', '<path>', <parts>)` — and every
/// part's own `after_install('<part md5>', <stored>, <real size>)`.
/// The remaining parts follow the first as entries with no
/// `before_install`. (`before_install_dependency` files are GOG's
/// bundled DOSBox and helpers; never game data.)
#[derive(Debug, Clone)]
struct GogFile {
    /// Install-relative, forward slashes.
    path: String,
    md5: String,
    /// (installer file index, part md5, real size), in order.
    parts: Vec<(usize, String, u64)>,
}

impl GogFile {
    fn size(&self) -> u64 {
        self.parts.iter().map(|p| p.2).sum()
    }
}

/// The arguments of one script hook call: `name('a', 'b', 3)` → the
/// quoted strings (`''` is a quote) and bare tokens, in order.
fn hook_args(hook: &str, name: &str) -> Option<Vec<String>> {
    let rest = hook
        .trim()
        .strip_prefix(name)?
        .trim_start()
        .strip_prefix('(')?;
    let body = rest.trim_end().strip_suffix(')')?;
    let mut out = Vec::new();
    let mut chars = body.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ',') {
            chars.next();
        }
        let Some(&c) = chars.peek() else { break };
        let mut arg = String::new();
        if c == '\'' {
            chars.next();
            loop {
                match chars.next()? {
                    '\'' if chars.peek() == Some(&'\'') => {
                        chars.next();
                        arg.push('\'');
                    }
                    '\'' => break,
                    ch => arg.push(ch),
                }
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch == ',' {
                    break;
                }
                arg.push(ch);
                chars.next();
            }
            arg = arg.trim().to_owned();
        }
        out.push(arg);
    }
    Some(out)
}

/// An install-relative path made safe: forward slashes, no root, no
/// drive, no `.`/`..` — or `None` (it could leave the folder).
fn safe_rel(path: &str) -> Option<String> {
    let rel = path.replace('\\', "/");
    let rel = rel.trim_start_matches('/').to_owned();
    let ok = !rel.is_empty()
        && !rel.contains(':')
        && rel
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..");
    ok.then_some(rel)
}

/// The installer's game files, from its script hooks.
fn gog_files(inst: &innospect::InnoInstaller) -> Result<Vec<GogFile>, String> {
    let files = inst.files();
    let mut out = Vec::new();
    let mut i = 0;
    while i < files.len() {
        let c = &files[i].item.conditions;
        let Some(args) = hook_args(&c.before_install, "before_install") else {
            i += 1;
            continue;
        };
        let (Some(md5), Some(path), Some(count)) = (args.first(), args.get(1), args.get(2)) else {
            return Err(format!("unreadable install hook {:?}", c.before_install));
        };
        let count: usize = count
            .parse()
            .map_err(|_| format!("unreadable part count in {:?}", c.before_install))?;
        let path = safe_rel(path).ok_or_else(|| format!("unsafe install path {path:?}"))?;
        let mut parts = Vec::with_capacity(count);
        for k in 0..count {
            let entry = files
                .get(i + k)
                .ok_or_else(|| format!("{path}: part {} of {count} is missing", k + 1))?;
            if k > 0 && !entry.item.conditions.before_install.is_empty() {
                return Err(format!("{path}: part {} of {count} is missing", k + 1));
            }
            let after = hook_args(&entry.item.conditions.after_install, "after_install")
                .filter(|a| a.len() >= 3)
                .ok_or_else(|| format!("{path}: part {} has no size record", k + 1))?;
            let size: u64 = after[2]
                .parse()
                .map_err(|_| format!("{path}: unreadable part size"))?;
            parts.push((i + k, after[0].clone(), size));
        }
        out.push(GogFile {
            path,
            md5: md5.clone(),
            parts,
        });
        i += count.max(1);
    }
    Ok(out)
}

fn read_installer(exe: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(exe).map_err(|e| format!("could not read {}: {e}", exe.display()))
}

fn parse_installer<'a>(exe: &Path, data: &'a [u8]) -> Result<innospect::InnoInstaller<'a>, String> {
    innospect::InnoInstaller::from_bytes(data).map_err(|e| {
        format!(
            "{} is not a GOG installer this version can read ({e})",
            exe.display()
        )
    })
}

/// What a GOG installer is: the product id of the `goggame-<id>.info`
/// it installs.
fn installer_game(exe: &Path) -> Result<Game, String> {
    let data = read_installer(exe)?;
    let inst = parse_installer(exe, &data)?;
    let id = gog_files(&inst)?
        .iter()
        .find_map(|f| {
            f.path
                .strip_prefix("goggame-")
                .and_then(|r| r.strip_suffix(".info"))
                .map(str::to_owned)
        })
        .ok_or_else(|| format!("{} is not a GOG installer", exe.display()))?;
    Game::from_gog_id(&id).ok_or_else(|| {
        format!(
            "{} is a GOG installer, but not for Magic Carpet (product {id})",
            exe.display()
        )
    })
}

/// The bytes of installer file `index` as Inno stores them (for GOG,
/// one zlib-compressed part).
///
/// GOG's installers store their chunks UNCOMPRESSED (the parts are zlib
/// already) — each data entry says so by lacking `ChunkCompressed` —
/// and `innospect` 0.2.0 decompresses every chunk with the installer's
/// method regardless (it fails on these with an LZMA2 error). So an
/// uncompressed, unencrypted, unfiltered single-slice chunk is sliced
/// here directly: setup-1 + the entry's offset, the 4-byte `zlb\x1a`
/// magic, then the file at its offset within the chunk. Anything else
/// still goes through `innospect`'s extractor.
fn part_reader<'a>(
    inst: &'a innospect::InnoInstaller<'a>,
    input: &'a [u8],
    index: usize,
) -> Result<Box<dyn std::io::Read + 'a>, String> {
    use innospect::DataFlag;
    let file = &inst.files()[index];
    let loc = inst
        .file_location_for(file)
        .ok_or_else(|| "a part has no data".to_owned())?;
    let plain = !loc.flags.contains(&DataFlag::ChunkCompressed)
        && !loc.flags.contains(&DataFlag::ChunkEncrypted)
        && !loc.flags.contains(&DataFlag::CallInstructionOptimized)
        && loc.first_slice == loc.last_slice;
    if !plain {
        return inst
            .extract(file)
            .map(|r| Box::new(r) as Box<dyn std::io::Read>)
            .map_err(|e| e.to_string());
    }
    let setup1 = inst.offset_table().offset_setup1;
    let at = |n: u64| usize::try_from(n).map_err(|_| "the installer is too large".to_owned());
    let chunk = at(setup1 + u64::from(loc.start_offset))?;
    let magic = input
        .get(chunk..chunk + 4)
        .ok_or("the installer is truncated")?;
    if magic != b"zlb\x1a" {
        return Err("a data chunk is damaged".into());
    }
    let body = chunk + 4;
    let end = body + at(loc.chunk_compressed_size)?;
    let start = body + at(loc.chunk_sub_offset)?;
    let stop = start + at(loc.original_size)?;
    if stop > end {
        return Err("a data chunk is damaged".into());
    }
    let bytes = input.get(start..stop).ok_or("the installer is truncated")?;
    Ok(Box::new(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Unpack the kept game files of `exe` into `staging`: each part read
/// from the installer (Inno verifies its own checksum), inflated, its
/// MD5 and size checked against GOG's record, appended in order, and
/// the whole file's MD5 checked at the end. Byte progress over the
/// kept set; `cancel` polled between buffers.
fn extract_installer(
    exe: &Path,
    game: Game,
    staging: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Progress),
) -> Result<(), ImportError> {
    use md5::{Digest, Md5};
    use std::io::{Read, Write};
    let fail = ImportError::Failed;
    progress(&Progress {
        stage: "Reading the installer…".into(),
        done: 0,
        total: 0,
    });
    let data = read_installer(exe).map_err(fail)?;
    let inst = parse_installer(exe, &data).map_err(fail)?;
    let wanted = |rel: &str| {
        let top = rel.split('/').next().unwrap_or("");
        game.keep().iter().any(|k| k.eq_ignore_ascii_case(top))
    };
    let kept: Vec<GogFile> = gog_files(&inst)
        .map_err(fail)?
        .into_iter()
        .filter(|f| wanted(&f.path))
        .collect();
    let total: u64 = kept.iter().map(GogFile::size).sum();
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    for f in &kept {
        let dest = staging.join(&f.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_failed("create", parent, &e))?;
        }
        let mut w = std::fs::File::create(&dest).map_err(|e| io_failed("write", &dest, &e))?;
        let mut whole = Md5::new();
        for (n, (index, part_md5, size)) in f.parts.iter().enumerate() {
            let bad = |what: &str| {
                fail(format!(
                    "{} part {} of {} {what} — the installer may be damaged",
                    f.path,
                    n + 1,
                    f.parts.len()
                ))
            };
            let raw = part_reader(&inst, &data, *index)
                .map_err(|e| fail(format!("reading {} from the installer failed: {e}", f.path)))?;
            let mut z = flate2::read::ZlibDecoder::new(raw);
            let mut part = Md5::new();
            let mut got = 0u64;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ImportError::Cancelled);
                }
                let k = z.read(&mut buf).map_err(|_| bad("does not unpack"))?;
                if k == 0 {
                    break;
                }
                part.update(&buf[..k]);
                whole.update(&buf[..k]);
                w.write_all(&buf[..k])
                    .map_err(|e| io_failed("write", &dest, &e))?;
                got += k as u64;
                done += k as u64;
                progress(&Progress {
                    stage: "Unpacking the installer…".into(),
                    done,
                    total,
                });
            }
            if got != *size {
                return Err(bad("has the wrong size"));
            }
            if !hex(&part.finalize()).eq_ignore_ascii_case(part_md5) {
                return Err(bad("fails its checksum"));
            }
        }
        if !hex(&whole.finalize()).eq_ignore_ascii_case(&f.md5) {
            return Err(fail(format!(
                "{} fails its checksum — the installer may be damaged",
                f.path
            )));
        }
        w.sync_all().map_err(|e| io_failed("write", &dest, &e))?;
    }
    Ok(())
}

fn ext_is(path: &Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|x| x.eq_ignore_ascii_case(ext))
}

/// Tell what `path` is: a GOG install directory, a GOG installer, or a
/// retail CD dump (its folder, its `.cue`, or the image itself).
pub fn identify(path: &Path) -> Result<Identified, String> {
    let found = |game, kind, path: &Path| Identified {
        game,
        kind,
        path: path.to_path_buf(),
        notes: Vec::new(),
    };
    if path.is_dir() {
        if gamedata::mc2_gog_install(path).is_some() {
            return Ok(found(Game::Mc2, SourceKind::GogInstall, path));
        }
        if gamedata::mc1_gog_install(path).is_some() {
            return Ok(found(Game::Mc1Plus, SourceKind::GogInstall, path));
        }
        if let Some(src) = gamedata::mc2_cd_image(path) {
            let cue = src.cue_sheet().unwrap_or_else(|| path.to_path_buf());
            return identify_cue(&cue);
        }
        return Err(format!(
            "{} holds no Magic Carpet data this version understands",
            path.display()
        ));
    }
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    if ext_is(path, "exe") {
        let game = installer_game(path)?;
        return Ok(found(game, SourceKind::GogInstaller, path));
    }
    if ext_is(path, "cue") {
        return identify_cue(path);
    }
    if ext_is(path, "img") || ext_is(path, "bin") || ext_is(path, "iso") {
        // The image itself: find the sheet beside it that names it.
        let dir = path.parent().unwrap_or(Path::new("."));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let cue = std::fs::read_dir(dir).ok().and_then(|rd| {
            rd.flatten().map(|e| e.path()).find(|p| {
                ext_is(p, "cue")
                    && std::fs::read_to_string(p)
                        .ok()
                        .and_then(|t| gamedata::cue_image_name(&t))
                        == name
            })
        });
        return match cue {
            Some(cue) => identify_cue(&cue),
            None => Err(format!(
                "{} has no cue sheet beside it (the `.cue` that names it is needed \
                 for the audio tracks)",
                path.display()
            )),
        };
    }
    Err(format!(
        "{} is not a Magic Carpet install, installer or CD image",
        path.display()
    ))
}

fn identify_cue(cue: &Path) -> Result<Identified, String> {
    let src = gamedata::mc2_cd_from_cue(cue).ok_or_else(|| {
        format!(
            "{} does not describe a Magic Carpet 2 CD image",
            cue.display()
        )
    })?;
    let mut notes = Vec::new();
    if let Some(image) = src.cd_image() {
        notes.extend(mc2_audio_note(image, cue));
    }
    Ok(Identified {
        game: Game::Mc2,
        kind: SourceKind::CdImage,
        path: cue.to_path_buf(),
        notes,
    })
}

/// One step of an [`import`], for a progress display.
#[derive(Debug, Clone)]
pub struct Progress {
    /// What is happening, player-facing.
    pub stage: String,
    /// Bytes copied so far / in total, when the stage is a copy.
    pub done: u64,
    pub total: u64,
}

/// Where [`import`] puts a source of this game and kind.
pub fn destination(gamedata_root: &Path, src: &Identified) -> PathBuf {
    gamedata_root.join(match src.kind {
        SourceKind::GogInstall | SourceKind::GogInstaller => src.game.install_dir(),
        SourceKind::CdImage => src.game.cd_dir(),
    })
}

/// Copy or extract `src` into its canonical home under
/// `gamedata_root` and return that home. Refuses an occupied home
/// unless `replace`. `cancel` is polled between copy chunks.
pub fn import(
    src: &Identified,
    gamedata_root: &Path,
    replace: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Progress),
) -> Result<PathBuf, ImportError> {
    let dest = destination(gamedata_root, src);
    if dest.exists() && !replace {
        return Err(ImportError::Exists(dest));
    }
    std::fs::create_dir_all(gamedata_root).map_err(|e| io_failed("create", gamedata_root, &e))?;
    let leaf = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = gamedata_root.join(format!(".import-{leaf}"));
    let _ = std::fs::remove_dir_all(&staging);
    let result = stage(src, gamedata_root, &staging, cancel, progress)
        .and_then(|()| verify(src, &staging))
        .and_then(|()| swap_in(&staging, &dest));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result.map(|()| dest)
}

fn stage(
    src: &Identified,
    gamedata_root: &Path,
    staging: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Progress),
) -> Result<(), ImportError> {
    std::fs::create_dir_all(staging).map_err(|e| io_failed("create", staging, &e))?;
    match src.kind {
        SourceKind::GogInstall => {
            let parts = keep_paths(&src.path, src.game);
            copy_all("Copying game data…", &parts, staging, cancel, progress)
        }
        SourceKind::CdImage => {
            let dir = src.path.parent().unwrap_or(Path::new("."));
            let text =
                std::fs::read_to_string(&src.path).map_err(|e| io_failed("read", &src.path, &e))?;
            let image = gamedata::cue_image_name(&text)
                .ok_or_else(|| ImportError::Failed("the cue sheet names no image".into()))?;
            let parts = vec![
                (
                    src.path.clone(),
                    PathBuf::from(src.path.file_name().unwrap_or_default()),
                ),
                (dir.join(&image), PathBuf::from(&image)),
            ];
            copy_all("Copying game data…", &parts, staging, cancel, progress)
        }
        SourceKind::GogInstaller => {
            // Keep the installer under its canonical name (the source a
            // later re-extraction reads), unless it already is that file.
            let keep_exe = gamedata_root.join(src.game.installer_name());
            let same =
                std::fs::canonicalize(&src.path).ok() == std::fs::canonicalize(&keep_exe).ok();
            if !same {
                let tmp = gamedata_root.join(format!(".{}.part", src.game.installer_name()));
                copy_all(
                    "Copying the installer…",
                    &[(src.path.clone(), PathBuf::new())],
                    &tmp,
                    cancel,
                    progress,
                )
                .and_then(|()| {
                    std::fs::rename(&tmp, &keep_exe).map_err(|e| io_failed("store", &keep_exe, &e))
                })
                .inspect_err(|_| {
                    let _ = std::fs::remove_file(&tmp);
                })?;
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(ImportError::Cancelled);
            }
            // Only the kept parts are unpacked: the slim copy, straight
            // into the staging folder (DOSBox, the uninstaller support,
            // manuals and GOG's helper folders are never written).
            extract_installer(&keep_exe, src.game, staging, cancel, progress)
        }
    }
}

/// The parts of a GOG install directory a slimmed copy keeps, as
/// (source, staging-relative destination) pairs.
fn keep_paths(install: &Path, game: Game) -> Vec<(PathBuf, PathBuf)> {
    game.keep()
        .iter()
        .map(|n| (install.join(n), PathBuf::from(n)))
        .filter(|(p, _)| p.exists())
        .collect()
}

/// Copy files/trees (`rel` empty = `to` IS the destination file),
/// reporting byte progress over the whole set.
fn copy_all(
    label: &str,
    parts: &[(PathBuf, PathBuf)],
    to: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Progress),
) -> Result<(), ImportError> {
    let mut files: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, rel) in parts {
        let dest = if rel.as_os_str().is_empty() {
            to.to_path_buf()
        } else {
            to.join(rel)
        };
        collect_files(from, &dest, &mut files).map_err(|e| io_failed("read", from, &e))?;
    }
    let total: u64 = files
        .iter()
        .filter_map(|(f, _)| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum();
    let mut done = 0u64;
    let mut buf = vec![0u8; 4 << 20];
    for (from, dest) in &files {
        use std::io::{Read, Write};
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_failed("create", parent, &e))?;
        }
        let mut r = std::fs::File::open(from).map_err(|e| io_failed("read", from, &e))?;
        let mut w = std::fs::File::create(dest).map_err(|e| io_failed("write", dest, &e))?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ImportError::Cancelled);
            }
            let n = r.read(&mut buf).map_err(|e| io_failed("read", from, &e))?;
            if n == 0 {
                break;
            }
            w.write_all(&buf[..n])
                .map_err(|e| io_failed("write", dest, &e))?;
            done += n as u64;
            progress(&Progress {
                stage: label.to_owned(),
                done,
                total,
            });
        }
        w.sync_all().map_err(|e| io_failed("write", dest, &e))?;
    }
    Ok(())
}

fn collect_files(from: &Path, to: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> io::Result<()> {
    if from.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(from)?
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for e in entries {
            let name = e.file_name().unwrap_or_default().to_owned();
            collect_files(&e, &to.join(name), out)?;
        }
    } else {
        out.push((from.to_path_buf(), to.to_path_buf()));
    }
    Ok(())
}

/// The staged copy must read back as the same game, by the same
/// detectors the bake uses.
fn verify(src: &Identified, staging: &Path) -> Result<(), ImportError> {
    let back = identify(staging)
        .map_err(|e| ImportError::Failed(format!("the copied data does not read back: {e}")))?;
    let want = match src.kind {
        SourceKind::GogInstaller => SourceKind::GogInstall,
        k => k,
    };
    if back.game != src.game || back.kind != want {
        return Err(ImportError::Failed(format!(
            "the copied data reads back as {} ({}), not {} ({})",
            back.game.title(),
            back.kind.label(),
            src.game.title(),
            want.label()
        )));
    }
    Ok(())
}

fn swap_in(staging: &Path, dest: &Path) -> Result<(), ImportError> {
    let old = dest.with_file_name(format!(
        ".replaced-{}",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&old);
    let had = dest.exists();
    if had {
        std::fs::rename(dest, &old).map_err(|e| io_failed("move aside", dest, &e))?;
    }
    if let Err(e) = std::fs::rename(staging, dest) {
        if had {
            let _ = std::fs::rename(&old, dest);
        }
        return Err(io_failed("move into place", dest, &e));
    }
    if had {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(())
}

/// One source [`survey`] found.
#[derive(Debug, Clone)]
pub struct Found {
    /// "mc1" or "mc2" — the bake's game slots.
    pub game: &'static str,
    pub origin: String,
    /// True for the source the bake will read (first in pecking order).
    pub used: bool,
}

/// Every source under `root`, per game, in pecking order, with the one
/// the bake reads marked.
pub fn survey(root: &Path) -> Vec<Found> {
    let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut children: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && !p
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .collect();
        children.sort();
        dirs.extend(children);
    }
    type Detector = fn(&Path) -> Option<gamedata::GameSource>;
    let ranks: [(&str, Detector); 5] = [
        ("mc1", gamedata::mc1_gog_install),
        ("mc1", gamedata::mc1_flat_dir),
        ("mc2", gamedata::mc2_gog_install),
        ("mc2", gamedata::mc2_cd_image),
        ("mc2", gamedata::mc2_hd_only),
    ];
    let winners = Gamedata::locate(root);
    let used = |game: &str, origin: &str| {
        let w = if game == "mc1" {
            &winners.mc1
        } else {
            &winners.mc2
        };
        w.as_ref().is_some_and(|s| s.origin == origin)
    };
    let mut out = Vec::new();
    for (game, detect) in ranks {
        for d in &dirs {
            if let Some(src) = detect(d) {
                let used = used(game, &src.origin);
                out.push(Found {
                    game,
                    origin: src.origin,
                    used,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gamedata_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gamedata")
    }

    /// GOG's script hooks parse into their arguments: quoted strings
    /// (with `''` as a quote) and bare numbers; a different hook name
    /// or a malformed call is no match.
    #[test]
    fn gog_hooks_parse() {
        let a = hook_args(
            "before_install('8c33cbfe4f0bf19a51e3c5a9289bcacf', 'game.gog', 38)",
            "before_install",
        )
        .unwrap();
        assert_eq!(a, ["8c33cbfe4f0bf19a51e3c5a9289bcacf", "game.gog", "38"]);
        let a = hook_args("after_install('ab', 6641389, 10485760)", "after_install").unwrap();
        assert_eq!(a, ["ab", "6641389", "10485760"]);
        let a = hook_args(
            r"before_install('x', 'GAME\Rahn''s Lair.DAT', 1)",
            "before_install",
        )
        .unwrap();
        assert_eq!(a[1], r"GAME\Rahn's Lair.DAT");
        assert!(hook_args("before_install_dependency('x', 'y', 1)", "before_install").is_none());
        assert!(hook_args("before_install('x', 'unterminated", "before_install").is_none());
        assert!(hook_args("", "before_install").is_none());
    }

    /// An install path is kept only when it cannot leave the folder.
    #[test]
    fn install_paths_must_stay_inside() {
        assert_eq!(
            safe_rel(r"GAME\NETHERW\CDATA\FONT0.DAT").as_deref(),
            Some("GAME/NETHERW/CDATA/FONT0.DAT")
        );
        assert_eq!(safe_rel("game.gog").as_deref(), Some("game.gog"));
        for bad in [
            r"..\evil.exe",
            r"GAME\..\..\x",
            r"C:\Windows\x",
            "a//b",
            "",
            r"GAME\.\x",
        ] {
            assert_eq!(safe_rel(bad), None, "{bad:?}");
        }
    }

    /// Every kind of source the player supplied reads as the right game
    /// (self-skipping per missing file), and the CD dump carries the
    /// damaged-audio note.
    #[test]
    fn identifies_the_real_sources() {
        let root = gamedata_root();
        let check = |rel: &str, game: Game, kind: SourceKind| {
            let p = root.join(rel);
            if !p.exists() {
                eprintln!("skip: no {rel}");
                return None;
            }
            let id = identify(&p).unwrap_or_else(|e| panic!("{rel}: {e}"));
            assert_eq!((id.game, id.kind), (game, kind), "{rel}");
            Some(id)
        };
        check("Magic Carpet 2", Game::Mc2, SourceKind::GogInstall);
        check("Magic Carpet Plus", Game::Mc1Plus, SourceKind::GogInstall);
        if let Some(cd) = check("netherworlds-cd", Game::Mc2, SourceKind::CdImage) {
            assert!(
                cd.notes.iter().any(|n| n.contains("damaged")),
                "{:?}",
                cd.notes
            );
        }
        check(
            "netherworlds-cd/Magic Carpet 2.img",
            Game::Mc2,
            SourceKind::CdImage,
        );
        check(
            "setup_magic_carpet_2_1.0_(28044).exe",
            Game::Mc2,
            SourceKind::GogInstaller,
        );
        check(
            "setup_magic_carpet_plus_1.0_(28186).exe",
            Game::Mc1Plus,
            SourceKind::GogInstaller,
        );
        assert!(
            identify(&root.join("README.md")).is_err(),
            "a stray file is no source"
        );
    }

    /// Every file the native extraction wrote is byte-identical to the
    /// Wine-installed one, and nothing installed under the kept paths is
    /// missing — bar what GOG's installer CREATES at install time (MC2's
    /// `CONFIG.DAT` and save stubs, from its `__support/save` folder).
    fn same_as_installed(extracted: &Path, installed: &Path, game: Game) {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, base, out);
                } else {
                    let rel = p.strip_prefix(base).unwrap().to_string_lossy();
                    out.push(rel.replace('\\', "/"));
                }
            }
        }
        let mut got = Vec::new();
        walk(extracted, extracted, &mut got);
        got.sort();
        for rel in &got {
            let a = std::fs::read(extracted.join(rel)).unwrap();
            let b = std::fs::read(installed.join(rel))
                .unwrap_or_else(|e| panic!("{rel}: not in the install ({e})"));
            assert!(a == b, "{rel} differs from the installed copy");
        }
        let mut want = Vec::new();
        for k in game.keep() {
            let p = installed.join(k);
            if p.is_dir() {
                walk(&p, installed, &mut want);
            } else if p.is_file() {
                want.push((*k).to_owned());
            }
        }
        let missing: Vec<&String> = want
            .iter()
            .filter(|w| !got.contains(w))
            .filter(|w| !w.ends_with("CONFIG.DAT") && !w.contains("/SAVE/"))
            .collect();
        assert!(missing.is_empty(), "not extracted: {missing:?}");
        assert!(got.len() > 10, "a real tree: {} files", got.len());
    }

    /// The whole import path on the real data, into a scratch gamedata
    /// root: both installers (extracted + slimmed) and the CD dump land
    /// in their canonical homes, read back as the same sources, and the
    /// MC2 image is byte-identical to the install's. Heavy (≈1 GB of
    /// copies): run on purpose with `--ignored`.
    #[test]
    #[ignore]
    fn imports_the_real_sources() {
        let root = gamedata_root();
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-import");
        let _ = std::fs::remove_dir_all(&scratch);
        let no_cancel = AtomicBool::new(false);
        let mut sink = |_: &Progress| {};
        let mut run = |rel: &str| -> Option<PathBuf> {
            let p = root.join(rel);
            if !p.exists() {
                return None;
            }
            let id = identify(&p).expect("identify");
            Some(import(&id, &scratch, false, &no_cancel, &mut sink).expect("import"))
        };
        if let Some(d) = run("setup_magic_carpet_2_1.0_(28044).exe") {
            assert_eq!(d, scratch.join("Magic Carpet 2"));
            assert!(scratch.join("GOG-MagicCarpet2.exe").is_file());
            let mut names: Vec<_> = std::fs::read_dir(&d)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            assert_eq!(
                names,
                ["GAME", "game.gog", "game.ins", "goggame-1207659187.info"]
            );
            same_as_installed(&d, &root.join("Magic Carpet 2"), Game::Mc2);
        }
        if let Some(d) = run("setup_magic_carpet_plus_1.0_(28186).exe") {
            same_as_installed(&d, &root.join("Magic Carpet Plus"), Game::Mc1Plus);
        }
        run("netherworlds-cd");
        let found = Gamedata::locate(&scratch);
        assert!(found.mc2.is_some_and(|s| s.origin.contains("GOG install")));
        assert!(found.mc1.is_some());
        // An occupied home is refused without `replace`.
        let id = identify(&root.join("netherworlds-cd")).unwrap();
        assert!(matches!(
            import(&id, &scratch, false, &no_cancel, &mut sink),
            Err(ImportError::Exists(_))
        ));
        assert!(
            std::fs::read_dir(&scratch)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().starts_with(".import-")),
            "no staging left behind"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
