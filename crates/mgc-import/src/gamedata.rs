//! Locating original game files under `gamedata/` (see
//! gamedata/README.md).
//!
//! The canonical source is a 1:1 copy of the GOG install directory —
//! kept exactly as the installer (or the importer) laid it down, cruft
//! and all, so it stays trivially reproducible. Those installs run the
//! games from a CD image, so most data lives inside `game.gog` rather
//! than on the filesystem; a [`GameSource`] resolves relative paths
//! through ordered layers (install-dir overlay first, then the CD
//! image) and reads straight out of the ISO, never extracting to disk.
//!
//! Path namespace per game is the CD layout:
//! - MC1: `DATA/…`, `LEVELS/LEVELS.*` + `LEVELS/DDLEVELS.*` (Hidden
//!   Worlds), `INTRO/…`, `MOVIE/…`. The GOG install's `CARPET.CD/` dir
//!   overlays the image's `CARPET/` tree (its data files are
//!   byte-identical copies; it adds setup/config files).
//! - MC2: `DATA/…`, `LEVELS/…`, `INTRO/…`, `SOUND/…`, `LANGUAGE/…`.
//!   The hard-disk portion (`GAME/NETHERW/`) overlays the CD with its
//!   `CDATA/` and `CLEVELS/` dirs aliased to `DATA/` and `LEVELS/`
//!   (also byte-identical copies of the CD files).
//!
//! Layouts are detected by content, not directory name, so the legacy
//! flat copies ("everything installed to disk" era GOG releases) keep
//! working: a dir with `DATA/DTABLES.DAT` + `LEVELS/LEVELS.DAT` is a
//! flat MC1 tree, a dir with `GAME/NETHERW/` but no CD image is a
//! hard-disk-only MC2 tree. GOG-install sources win over legacy ones.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use crate::iso::IsoImage;

/// `CDATA/` and `CLEVELS/` are MC2's installed (hard-disk) copies of
/// the CD's `DATA/` and `LEVELS/` trees.
const MC2_HD_ALIASES: [(&str, &str); 2] = [("DATA/", "CDATA/"), ("LEVELS/", "CLEVELS/")];

enum Layer {
    /// Plain directory; `aliases` maps canonical prefixes to on-disk
    /// ones, tried before the canonical path itself.
    Dir {
        root: PathBuf,
        aliases: &'static [(&'static str, &'static str)],
    },
    /// CD image, optionally rooted at a subdirectory inside it.
    Iso {
        image: IsoImage,
        prefix: &'static str,
    },
}

/// Ordered read layers for one game's data; first hit wins.
pub struct GameSource {
    /// Where this source was found, for logs and error messages.
    pub origin: String,
    layers: Vec<Layer>,
    /// The cue sheet describing the CD image's tracks, when it is not
    /// the GOG convention (`game.ins` beside `game.gog`) — a retail
    /// CD dump's `.cue`. See [`GameSource::cue_sheet`].
    cue: Option<PathBuf>,
}

impl GameSource {
    /// Read a file by canonical relative path, e.g. `DATA/PAL0-0.DAT`.
    pub fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        for layer in &self.layers {
            match layer {
                Layer::Dir { root, aliases } => {
                    for cand in dir_candidates(rel, aliases) {
                        if let Some(p) = resolve_case(root, &cand) {
                            return std::fs::read(p);
                        }
                    }
                }
                Layer::Iso { image, prefix } => {
                    let full = join_prefix(prefix, rel);
                    if image.contains(&full) {
                        return image.read(&full);
                    }
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{}: no {rel} in any layer", self.origin),
        ))
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.layers.iter().any(|layer| match layer {
            Layer::Dir { root, aliases } => dir_candidates(rel, aliases)
                .iter()
                .any(|c| resolve_case(root, c).is_some()),
            Layer::Iso { image, prefix } => image.contains(&join_prefix(prefix, rel)),
        })
    }

    /// Path of the source's CD image on disk, if it has one — raw
    /// sector access for the redbook rip lives outside the ISO
    /// filesystem view.
    pub fn cd_image(&self) -> Option<&Path> {
        self.layers.iter().find_map(|layer| match layer {
            Layer::Iso { image, .. } => Some(image.image_path()),
            Layer::Dir { .. } => None,
        })
    }

    /// Path of the cue sheet for [`Self::cd_image`]'s track layout: a
    /// retail dump's own `.cue`, else GOG's `game.ins` beside the
    /// image. May not exist on disk (the audio bake notes and skips).
    pub fn cue_sheet(&self) -> Option<PathBuf> {
        self.cue
            .clone()
            .or_else(|| self.cd_image().map(|i| i.with_extension("ins")))
    }

    /// Union of canonical paths across all layers, sorted, deduplicated.
    pub fn list(&self) -> Vec<String> {
        let mut out = BTreeSet::new();
        for layer in &self.layers {
            match layer {
                Layer::Dir { root, aliases } => {
                    let mut files = Vec::new();
                    walk_dir(root, "", &mut files);
                    for rel in files {
                        // Present the on-disk alias under its canonical name.
                        let canon = aliases
                            .iter()
                            .find_map(|(canonical, on_disk)| {
                                rel.strip_prefix(on_disk)
                                    .map(|rest| format!("{canonical}{rest}"))
                            })
                            .unwrap_or(rel);
                        out.insert(canon);
                    }
                }
                Layer::Iso { image, prefix } => {
                    for path in image.paths() {
                        if prefix.is_empty() {
                            out.insert(path.to_string());
                        } else if let Some(rest) =
                            path.strip_prefix(prefix).and_then(|r| r.strip_prefix('/'))
                        {
                            out.insert(rest.to_string());
                        }
                    }
                }
            }
        }
        out.into_iter().collect()
    }
}

/// Alias-mapped lookups first (e.g. `DATA/X` → `CDATA/X`), then the
/// canonical path itself.
fn dir_candidates(rel: &str, aliases: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::with_capacity(2);
    for (canonical, on_disk) in aliases {
        if let Some(rest) = rel.strip_prefix(canonical) {
            out.push(format!("{on_disk}{rest}"));
        }
    }
    out.push(rel.to_string());
    out
}

fn join_prefix(prefix: &str, rel: &str) -> String {
    if prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{prefix}/{rel}")
    }
}

/// Resolve `rel` under `root` component by component, falling back to a
/// case-insensitive directory scan per component (GOG ships DOS-style
/// uppercase names, but repacked copies sometimes lowercase them).
fn resolve_case(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for comp in rel.split('/') {
        let exact = path.join(comp);
        if exact.exists() {
            path = exact;
            continue;
        }
        let entries = std::fs::read_dir(&path).ok()?;
        let found = entries
            .flatten()
            .map(|e| e.file_name())
            .find(|name| name.to_str().is_some_and(|n| n.eq_ignore_ascii_case(comp)))?;
        path = path.join(found);
    }
    path.is_file().then_some(path)
}

fn walk_dir(root: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.path().is_dir() {
            walk_dir(&entry.path(), &rel, out);
        } else {
            out.push(rel);
        }
    }
}

/// Everything found under a gamedata root.
pub struct Gamedata {
    pub mc1: Option<GameSource>,
    pub mc2: Option<GameSource>,
}

impl Gamedata {
    /// Detect game sources in `root`'s immediate subdirectories (and
    /// `root` itself). Every layout is recognized by marker files; when
    /// both a GOG install and a legacy flat copy are present, the GOG
    /// install wins. Candidates are probed in sorted order so the
    /// outcome is deterministic.
    pub fn locate(root: &Path) -> Gamedata {
        let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut children: Vec<_> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            children.sort();
            dirs.extend(children);
        }

        let (mut mc1_gog, mut mc1_flat, mut mc2_gog, mut mc2_cd, mut mc2_hd) =
            (None, None, None, None, None);
        for dir in &dirs {
            if mc1_gog.is_none() {
                mc1_gog = mc1_gog_install(dir);
            }
            if mc1_flat.is_none() {
                mc1_flat = mc1_flat_dir(dir);
            }
            if mc2_gog.is_none() {
                mc2_gog = mc2_gog_install(dir);
            }
            if mc2_cd.is_none() {
                mc2_cd = mc2_cd_image(dir);
            }
            if mc2_hd.is_none() {
                mc2_hd = mc2_hd_only(dir);
            }
        }
        Gamedata {
            mc1: mc1_gog.or(mc1_flat),
            // The pecking order: the GOG install, then a retail CD
            // dump, then the legacy hard-disk-only copy.
            mc2: mc2_gog.or(mc2_cd).or(mc2_hd),
        }
    }
}

fn origin(dir: &Path, kind: &str) -> String {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string());
    format!("{name} ({kind})")
}

/// GOG "Magic Carpet Plus": install runs from `CARPET.CD/game.gog`
/// (cooked ISO, game tree under `CARPET/`), with `CARPET.CD/` itself
/// holding the installed overlay.
pub(crate) fn mc1_gog_install(dir: &Path) -> Option<GameSource> {
    let overlay = dir.join("CARPET.CD");
    let image = IsoImage::open(&overlay.join("game.gog")).ok()?;
    image
        .contains("CARPET/LEVELS/LEVELS.DAT")
        .then(|| GameSource {
            origin: origin(dir, "GOG install, CD image + CARPET.CD overlay"),
            layers: vec![
                Layer::Dir {
                    root: overlay,
                    aliases: &[],
                },
                Layer::Iso {
                    image,
                    prefix: "CARPET",
                },
            ],
            cue: None,
        })
}

/// Legacy fully-installed MC1 tree (old GOG release): game files flat
/// on disk. `DTABLES.DAT` is MC1-only, so it doubles as the marker
/// distinguishing this from other DATA/LEVELS-shaped trees.
pub(crate) fn mc1_flat_dir(dir: &Path) -> Option<GameSource> {
    (dir.join("DATA/DTABLES.DAT").is_file() && dir.join("LEVELS/LEVELS.DAT").is_file()).then(|| {
        GameSource {
            origin: origin(dir, "flat install"),
            layers: vec![Layer::Dir {
                root: dir.to_path_buf(),
                aliases: &[],
            }],
            cue: None,
        }
    })
}

/// GOG "Magic Carpet 2": raw-sector CD image at the install root (game
/// tree at the image root), hard-disk portion under `GAME/NETHERW/`.
pub(crate) fn mc2_gog_install(dir: &Path) -> Option<GameSource> {
    let netherw = dir.join("GAME/NETHERW");
    if !netherw.is_dir() {
        return None;
    }
    let image = IsoImage::open(&dir.join("game.gog")).ok()?;
    image.contains("LEVELS/LEVELS.DAT").then(|| GameSource {
        origin: origin(dir, "GOG install, CD image + NETHERW overlay"),
        layers: vec![
            Layer::Dir {
                root: netherw,
                aliases: &MC2_HD_ALIASES,
            },
            Layer::Iso { image, prefix: "" },
        ],
        cue: None,
    })
}

/// A retail MC2 CD dump: a directory holding a cue sheet (`.cue`)
/// whose single `FILE` is a raw-sector (or cooked) image with the
/// game tree at its root — CloneCD's `.img`/`.cue`, or `.bin`/`.cue`.
/// The CD alone carries the whole game (GOG's `GAME/NETHERW` hard-disk
/// files are byte-identical copies of CD files). MC1's CD keeps its
/// tree under `CARPET/`, so the root `LEVELS/LEVELS.DAT` marker cannot
/// mistake it. The first cue sheet in name order that qualifies wins.
pub(crate) fn mc2_cd_image(dir: &Path) -> Option<GameSource> {
    let mut cues: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("cue")))
        .collect();
    cues.sort();
    cues.into_iter().find_map(|cue| mc2_cd_from_cue(&cue))
}

/// One cue sheet as an MC2 CD source: the image its `FILE` names
/// (beside the sheet) must hold the MC2 tree at its root.
pub(crate) fn mc2_cd_from_cue(cue: &Path) -> Option<GameSource> {
    let dir = cue.parent()?;
    let text = std::fs::read_to_string(cue).ok()?;
    let image = dir.join(cue_image_name(&text)?);
    let image = IsoImage::open(&image).ok()?;
    image.contains("LEVELS/LEVELS.DAT").then(|| GameSource {
        origin: origin(dir, "retail CD image"),
        layers: vec![Layer::Iso { image, prefix: "" }],
        cue: Some(cue.to_path_buf()),
    })
}

/// The image file a single-FILE cue sheet names: `FILE "name" BINARY`
/// (quoted, or a bare token).
pub(crate) fn cue_image_name(cue: &str) -> Option<String> {
    cue.lines().find_map(|l| {
        let rest = l.trim_start().strip_prefix("FILE")?.trim_start();
        if let Some(q) = rest.strip_prefix('"') {
            q.split('"').next().map(str::to_owned)
        } else {
            rest.split_whitespace().next().map(str::to_owned)
        }
    })
}

/// MC2 hard-disk tree without a CD image (the legacy copy): only the
/// installed `CDATA`/`CLEVELS` subset is available.
pub(crate) fn mc2_hd_only(dir: &Path) -> Option<GameSource> {
    let netherw = dir.join("GAME/NETHERW");
    (netherw.join("CLEVELS/LEVELS.DAT").is_file() && !dir.join("game.gog").is_file()).then(|| {
        GameSource {
            origin: origin(dir, "hard-disk portion only, no CD image"),
            layers: vec![Layer::Dir {
                root: netherw,
                aliases: &MC2_HD_ALIASES,
            }],
            cue: None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::cue_image_name;

    #[test]
    fn cue_names_its_image() {
        let retail = "FILE \"Magic Carpet 2.img\" BINARY\n   TRACK 1 MODE1/2352\n";
        assert_eq!(
            cue_image_name(retail).as_deref(),
            Some("Magic Carpet 2.img")
        );
        assert_eq!(
            cue_image_name("FILE game.bin BINARY\n").as_deref(),
            Some("game.bin")
        );
        assert_eq!(cue_image_name("TRACK 1 AUDIO\n"), None);
    }

    /// The pecking order on the real data, self-skipping per missing
    /// source: a retail CD dump is found on its own (with its `.cue`
    /// as the cue sheet), and beside a GOG install the GOG install
    /// still wins — existing setups are untouched.
    #[test]
    fn mc2_cd_dump_is_found_and_gog_still_wins() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gamedata");
        let cd_dir = root.join("netherworlds-cd");
        if !cd_dir.is_dir() {
            eprintln!("skip: no gamedata/netherworlds-cd");
            return;
        }
        let cd = super::Gamedata::locate(&cd_dir)
            .mc2
            .expect("the CD dump is an MC2 source");
        assert!(cd.origin.contains("retail CD image"), "{}", cd.origin);
        let cue = cd.cue_sheet().expect("cue sheet");
        assert_eq!(cue.extension().and_then(|x| x.to_str()), Some("cue"));
        assert!(cd.exists("LEVELS/LEVELS.DAT") && cd.exists("SOUND/SOUND.DAT"));
        if root.join("Magic Carpet 2/game.gog").is_file() {
            let all = super::Gamedata::locate(&root).mc2.expect("mc2");
            assert!(
                all.origin.contains("GOG install"),
                "GOG must win: {}",
                all.origin
            );
        }
    }
}
