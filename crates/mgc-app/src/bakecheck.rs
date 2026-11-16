//! First-run / stale-bake detection. The game looks for its baked
//! data and (re)bakes it from the original game data when it is
//! missing or from an older bake (`mgc_formats::BAKE_EPOCH`), so a
//! fresh checkout + GOG installs is all a player needs: point the
//! game at a level and the importer runs by itself, once.
//!
//! Staleness is judged from the artifact stamps, not timestamps: the
//! requested level package's `meta.json` and every present bundle's
//! `bundle.json` must carry the current schema version AND the
//! current bake epoch, and the bake's completion stamp
//! (`manifest.sha256`, written last by `bake_all`) must exist with
//! every file it lists still on disk — an interrupted first bake
//! otherwise leaves a partial tree whose present artifacts all look
//! current. Any mismatch (or a missing artifact) triggers one full
//! `bake_all_staged` — the same orchestration the `mgc-import bake`
//! CLI runs — which bakes beside the tree and swaps it in only when
//! complete (a cancelled rebake leaves the old tree intact), from game
//! data located via (in order)
//! the config's `gamedata` path, `MGC_GAMEDATA`, or `gamedata/` in
//! the working directory.

use std::path::{Path, PathBuf};

use mgc_formats::bundle::BUNDLE_VERSION;
use mgc_formats::{BAKE_EPOCH, FORMAT_VERSION, mgcl};

/// Why the baked tree needs regenerating; `None` = current.
fn level_staleness(level_path: &Path) -> Option<String> {
    let file = match std::fs::File::open(level_path) {
        Ok(f) => f,
        Err(_) => return Some(format!("{} is not baked yet", level_path.display())),
    };
    let meta = match mgcl::read_meta(file) {
        Ok(m) => m,
        Err(e) => return Some(format!("{}: unreadable ({e})", level_path.display())),
    };
    if meta.format_version != FORMAT_VERSION || meta.bake_epoch != BAKE_EPOCH {
        return Some(format!(
            "{}: baked as format {} epoch {}, this build wants {}/{}",
            level_path.display(),
            meta.format_version,
            meta.bake_epoch,
            FORMAT_VERSION,
            BAKE_EPOCH,
        ));
    }
    None
}

/// Sweep `<baked>/assets/*/bundle.json`. A stale stamp anywhere (env,
/// audio) triggers the rebake; an EMPTY assets tree does too — no
/// level can load without at least one environment bundle.
fn bundle_staleness(baked_root: &Path) -> Option<String> {
    let assets = baked_root.join("assets");
    let entries = match std::fs::read_dir(&assets) {
        Ok(e) => e,
        Err(_) => return Some(format!("{}: no asset bundles baked", assets.display())),
    };
    let mut bundles = 0usize;
    for entry in entries.flatten() {
        let manifest_path = entry.path().join("bundle.json");
        let bytes = match std::fs::read(&manifest_path) {
            Ok(b) => b,
            Err(_) => continue, // not a bundle dir
        };
        bundles += 1;
        let manifest: mgc_formats::bundle::BundleManifest = match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(e) => {
                return Some(format!("{}: unreadable ({e})", manifest_path.display()));
            }
        };
        if manifest.format_version != BUNDLE_VERSION || manifest.bake_epoch != BAKE_EPOCH {
            return Some(format!(
                "bundle {}: baked as format {} epoch {}, this build wants {}/{}",
                manifest.variant,
                manifest.format_version,
                manifest.bake_epoch,
                BUNDLE_VERSION,
                BAKE_EPOCH,
            ));
        }
    }
    (bundles == 0).then(|| format!("{}: no asset bundles baked", assets.display()))
}

/// The bake's completion stamp: `bake_all` writes `manifest.sha256`
/// LAST, so its absence means the previous bake never finished — a
/// first run killed or crashed mid-way (classically during the long
/// audio/music render, which a double-click user sees as a hung
/// window) leaves a tree whose PRESENT artifacts all look current
/// while whole bundles are missing, and without this check that
/// partial tree would be accepted forever. Every listed file must
/// also still exist (names only — no re-hashing; content
/// verification stays an external `sha256sum -c` affair).
fn manifest_staleness(baked_root: &Path) -> Option<String> {
    let path = baked_root.join("manifest.sha256");
    let body = match std::fs::read_to_string(&path) {
        Ok(b) => b,
        Err(_) => {
            return Some(format!(
                "{}: missing — the previous bake did not run to completion",
                path.display()
            ));
        }
    };
    for line in body.lines() {
        let Some((_, name)) = line.split_once("  ") else {
            continue;
        };
        if !baked_root.join(name).is_file() {
            return Some(format!(
                "{name}: listed in manifest.sha256 but missing from the baked tree"
            ));
        }
    }
    None
}

/// Locate original game data: config override, `MGC_GAMEDATA`, then
/// `gamedata/` beside the working directory.
fn locate_gamedata(config_override: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = config_override {
        return p.is_dir().then(|| p.to_path_buf());
    }
    // `MGC_GAMEDATA`, else `gamedata/` (crate::paths).
    let p = crate::paths::gamedata_dir();
    p.is_dir().then_some(p)
}

/// Ensure the baked tree serving `level_path` is present and current;
/// bake it from game data when it isn't. `Ok(())` means the caller
/// can load the level normally.
pub fn ensure_baked(level_path: &Path, config_gamedata: Option<&Path>) -> Result<(), String> {
    // Same root rule as load_level: <baked>/<game>/level-NNN.mgcl —
    // but only trust the inference when the path actually follows the
    // convention (parent dir named after a game). Guessing a root for
    // an arbitrary path can spray a full bake into the working
    // directory (a bare `mc3:5` would infer root `.`).
    let parent_is_game = level_path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|d| matches!(d.to_str(), Some("mc1" | "mc1hw" | "mc2")));
    if !parent_is_game {
        // A custom/one-off package outside the baked tree: load it as
        // it stands (bundle resolution reports its own errors), but
        // never auto-bake into an inferred root.
        return if level_path.exists() {
            Ok(())
        } else {
            Err(format!(
                "{}: not found, and the path does not follow \
                 <root>/<game>/level-NNN.mgcl, so there is no baked tree to \
                 (re)generate for it. Try `--level mc1:0` or a path under \
                 baked/.",
                level_path.display()
            ))
        };
    }
    let baked_root = level_path
        .parent()
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    let reason = level_staleness(level_path)
        .or_else(|| bundle_staleness(baked_root))
        .or_else(|| manifest_staleness(baked_root));
    let Some(reason) = reason else {
        return Ok(());
    };

    let Some(gamedata) = locate_gamedata(config_gamedata) else {
        return Err(format!(
            "baked data needs (re)generating ({reason}), but no game data was found.\n\
             Point the game at your original GOG install(s): set `gamedata` in the\n\
             config file, set MGC_GAMEDATA, or place them under gamedata/ (see\n\
             gamedata/README.md for the expected layout)."
        ));
    };

    println!("baking game data ({reason})");
    println!(
        "  {} -> {}  (first run does everything, including music rendering — this can take a while)",
        gamedata.display(),
        baked_root.display()
    );
    let summary = mgc_import::bake::bake_all_staged(&gamedata, baked_root)?;
    if summary.manifest.is_empty() {
        return Err(format!(
            "no game data found under {} — nothing baked. Point `gamedata` in the\n\
             config or MGC_GAMEDATA at your original GOG install(s).",
            gamedata.display()
        ));
    }

    // The bake ran; verify it actually produced what we came for
    // (e.g. the requested level might belong to a game whose data is
    // absent).
    if let Some(still) = level_staleness(level_path)
        .or_else(|| bundle_staleness(baked_root))
        .or_else(|| manifest_staleness(baked_root))
    {
        return Err(format!(
            "bake completed but the requested data is still unavailable: {still}"
        ));
    }
    Ok(())
}

/// What the launcher knows about the baked tree without touching it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BakeStatus {
    /// Why the whole tree needs (re)generating — a missing or stale
    /// bundle, or an unfinished bake; `None` = the tree is current.
    pub tree: Option<String>,
    /// Per game (MC1, Hidden Worlds, MC2): the tree is current AND
    /// the game's first level is baked at this build's epoch.
    pub games: [bool; 3],
}

/// The launcher's order of the games' baked directories.
pub const GAME_DIRS: [&str; 3] = ["mc1", "mc1hw", "mc2"];

/// Judge the baked tree at `baked_root` — the same stamps
/// [`ensure_baked`] checks, per game, and never a bake.
pub fn status(baked_root: &Path) -> BakeStatus {
    let tree = bundle_staleness(baked_root).or_else(|| manifest_staleness(baked_root));
    let games = std::array::from_fn(|i| {
        tree.is_none()
            && level_staleness(&baked_root.join(GAME_DIRS[i]).join("level-000.mgcl")).is_none()
    });
    BakeStatus { tree, games }
}

/// The original game data a bake would read (see [`ensure_baked`]).
pub fn gamedata(config_gamedata: Option<&Path>) -> Option<PathBuf> {
    locate_gamedata(config_gamedata)
}

/// Bake the whole tree from `gamedata` into `baked_root` (staged —
/// an interrupted bake leaves the old tree intact). The launcher's
/// background bake; the caller re-reads [`status`] afterwards.
pub fn bake(gamedata: &Path, baked_root: &Path) -> Result<(), String> {
    println!(
        "baking game data: {} -> {}",
        gamedata.display(),
        baked_root.display()
    );
    let summary = mgc_import::bake::bake_all_staged(gamedata, baked_root)?;
    if summary.manifest.is_empty() {
        return Err(format!("no game data found under {}", gamedata.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The completion stamp guards the interrupted-first-bake hole: a
    /// tree without `manifest.sha256` (bake killed mid-run — e.g.
    /// during the long audio render) is stale even when every PRESENT
    /// bundle looks current, and a listed-but-deleted file is caught
    /// by the existence sweep. Non-vacuity: before this check, such a
    /// tree passed `bundle_staleness` and shipped without audio
    /// forever (Windows player report 2026-08-08).
    #[test]
    fn manifest_stamp_flags_unfinished_and_gutted_bakes() {
        let root = std::env::temp_dir().join(format!("mgc-bakecheck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/mc1-audio")).unwrap();

        // No stamp at all: the interrupted bake.
        assert!(
            manifest_staleness(&root)
                .expect("missing stamp is stale")
                .contains("did not run to completion")
        );

        // Stamp listing an existing file: current.
        std::fs::write(root.join("assets/mc1-audio/bundle.json"), b"{}").unwrap();
        std::fs::write(
            root.join("manifest.sha256"),
            "0000  assets/mc1-audio/bundle.json\n",
        )
        .unwrap();
        assert_eq!(manifest_staleness(&root), None);

        // A listed file deleted afterwards: stale again.
        std::fs::remove_file(root.join("assets/mc1-audio/bundle.json")).unwrap();
        assert!(
            manifest_staleness(&root)
                .expect("gutted tree is stale")
                .contains("assets/mc1-audio/bundle.json")
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
