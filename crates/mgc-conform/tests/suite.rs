//! The conformance suite as a cargo test (docs/CONFORMANCE.md): every
//! committed manifest under `conformance/` replays its fixtures — one
//! small `.mgcr` per fixture under `conformance/fixtures/<take>/`,
//! named for the law it pins and committed via git-lfs — against the
//! current sim and enforces the expected statuses. Skips — with a
//! printed note, mirroring the golden tests' baked-data skip — when
//! the baked tree is absent (local corpus data), or the evidence is
//! missing / un-hydrated LFS pointers.
//!
//! Every one of those skips is an ERROR under `MGC_REQUIRE_GOLDENS`,
//! exactly as `mgc-sim`'s `common::golden_skip` treats a missing bake.
//! Without that the fidelity lane reports GREEN having executed
//! nothing: `baked/` is 925 MB of gitignored derived data, so CI, a
//! fresh worktree and every subagent took the `SKIP` and passed.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// Report a corpus-absence skip. Under `MGC_REQUIRE_GOLDENS` a skip is
/// a FAILURE — the twin of `mgc-sim/tests/common/mod.rs::golden_skip`,
/// down to the greppable prefix.
fn conform_skip(what: &str) {
    if std::env::var_os("MGC_REQUIRE_GOLDENS").is_some_and(|v| v != "0" && !v.is_empty()) {
        panic!("MGC_REQUIRE_GOLDENS is set, but: {what}");
    }
    eprintln!("CONFORM-SKIP: {what}");
}

#[test]
fn conformance_suite() {
    let root = repo_root();
    let dir = root.join("conformance");
    let manifests: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            // The known-deviation roster lives beside the suite
            // manifests (docs/CONFORMANCE.md) but is not one.
            .filter(|p| p.file_name().is_none_or(|n| n != "known-deviations.json"))
            .collect(),
        Err(_) => {
            conform_skip("no conformance/ manifest dir");
            return;
        }
    };
    if manifests.is_empty() {
        conform_skip("conformance/ holds no manifests");
        return;
    }
    if !root.join("baked").exists() {
        conform_skip("baked data not present");
        return;
    }
    let mut ran = 0;
    for m in &manifests {
        // Per-fixture evidence files live in the manifest's `dir`,
        // relative to the manifest, and are git-lfs tracked
        // (.gitattributes). A missing dir means an un-cut manifest or a
        // botched migration; an un-hydrated checkout shows up as
        // pointer stubs instead, caught below.
        let dir: Option<String> = std::fs::read_to_string(m)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| Some(v.get("dir")?.as_str()?.to_string()));
        let Some(dir) = dir else {
            panic!(
                "{}: manifest has no `dir` — an un-cut extract cannot be a suite \
                 (conformance/cut_fixture_files.py)",
                m.display()
            );
        };
        let dir_path = m.parent().unwrap().join(&dir);
        if !dir_path.exists() {
            conform_skip(&format!("{}: fixture dir {dir} not present", m.display()));
            continue;
        }
        // A checkout without git-lfs materializes the evidence as
        // pointer stubs — skip, don't choke on non-zstd bytes. One
        // sample settles it: the whole directory hydrates together.
        let stub = std::fs::read_dir(&dir_path).ok().and_then(|rd| {
            rd.filter_map(|e| Some(e.ok()?.path()))
                .find(|p| p.extension().is_some_and(|x| x == "mgcr"))
                .map(|p| std::fs::read(&p).is_ok_and(|b| b.starts_with(b"version https://git-lfs")))
        });
        if stub == Some(true) {
            conform_skip(&format!(
                "{}: fixture dir {dir} holds un-hydrated git-lfs pointers",
                m.display()
            ));
            continue;
        }
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_mgc-conform"))
            .current_dir(&root)
            .arg("fixtures")
            .arg(m)
            .output()
            .expect("spawn mgc-conform");
        print!("{}", String::from_utf8_lossy(&out.stdout));
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
        assert!(
            out.status.success(),
            "conformance suite {} reported regressions, drift or unpromoted fixes",
            m.display()
        );
        ran += 1;
    }
    // Every manifest skipping individually still leaves the lane
    // vacuous, and the per-manifest notes scroll past in a long run.
    if ran == 0 {
        conform_skip("no suite executed — every manifest was skipped");
    }
    println!("conformance: {ran} suite(s) enforced");
}
