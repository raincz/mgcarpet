//! The conformance fixture SUITE (docs/CONFORMANCE.md): pairs of
//! retail states that pin a FIXED retail law, one per file, replayed
//! on every `cargo test`.
//!
//! A fixture is a COPY, not a reference: its `.mgcr` holds the three
//! tick records it needs (`t-1 .. t+1`) and nothing else, so it
//! outlives the take it was cut from. The runner builds a world per
//! file — one file, one pair, one world — and replays it through the
//! same import-tick-diff core as `verify-deltas` (`verify::exec_pair`).
//!
//! The file NAME is the law. That is what makes a failure report a
//! story rather than a tick, makes `git rm` the curation tool, and
//! makes the filesystem enforce one exemplar per story. Which
//! recording a pair came from is provenance (`Fixture::source`), not
//! identity — a fixture belongs to a LEVEL.
//!
//! **EXISTENCE IS THE ASSERTION.** A fixture means "this law works,
//! keep it working" and nothing else, so there is no status field to
//! read: a fixture either passes or it is a REGRESSION, and retracting
//! one is `git rm`. That collapsed a whole machine — expected statuses,
//! diff signatures and their hashes, `--promote`/`--demote`, and the
//! FIXED/DRIFT verdicts — into two outcomes. It is only honest because
//! the corpus earned it: every pending fixture was either fixed or
//! deleted first (2026-08-16b), so the status field had become a
//! constant and the machinery was reading a value that never varied.
//!
//! What the manifest still buys, and why it is not just a directory
//! listing: a DECLARED LIST catches two things `ls` cannot, and both
//! fired in practice — a file present but undeclared (orphaned by a
//! rename or a half-finished cut) and a file declared but missing.
//! Both are hard errors, reported BY LAW NAME.

use crate::Args;
use crate::verify::{self, PairDiff};
use mgc_formats::mgcr::{ObsMc1, Recording, RetailMc1, decode_retail_mc1};
use mgc_sim::engine::world::PlayerCommand;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct Fixture {
    pub t: u64,
    /// The evidence file inside the manifest's `dir` — an ordinary
    /// `.mgcr` holding this pair's three lines and nothing else. Named
    /// for the LAW it pins and nothing else, so a failure reports a
    /// story rather than a tick, `git rm` is the curation tool, and
    /// appending a fixture costs one ~20 KB blob instead of re-freezing
    /// a whole take. Because the name is the law, the filesystem
    /// enforces one exemplar per story: merging l32's two takes
    /// collapsed 54 files to 39 distinct laws, one of them pinned five
    /// times over.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    /// Which take this pair was cut from — PROVENANCE ONLY. A fixture
    /// is a pair of retail states for a LEVEL; which recording it came
    /// out of does not change what it pins, and the take may not even
    /// exist any more (mc1l32-bee-height's does not, while its fixtures
    /// still guard three laws the surviving take never captured).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Free-form triage note (ledger entry, family name).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Fixture {
    /// The law this fixture pins — its file name IS the law
    /// (`the-severed-ball-chain….mgcr`). What a failure is reported as.
    pub fn law(&self) -> &str {
        self.file
            .rsplit('/')
            .next()
            .unwrap_or(&self.file)
            .strip_suffix(".mgcr")
            .unwrap_or(&self.file)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    /// Directory of per-fixture evidence files, relative to the
    /// manifest's own dir. Its presence is what makes this a runnable
    /// suite: `extract` writes a manifest carrying `recording` (a
    /// pointer at the source take) and `cut_fixture_files.py` turns
    /// that into `dir` + a `file` per fixture.
    pub dir: String,
    /// Legacy single-take provenance. Superseded by the per-fixture
    /// `source`, because a level's suite may draw on several takes:
    /// mc1l32's 39 laws come from two. Never opened by the runner.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recording: String,
    pub pin_pose: String,
    pub fixtures: Vec<Fixture>,
}

/// The slot- and value-free signature of a divergent pair: sorted,
/// deduped atoms — `rng`, `missing:c,m`, `extra:c,m`,
/// `field:c,m:name` (entity fields, class/model from the retail obs)
/// and `field:name` (wizard/player scalars). It used to be a COMPARED
/// value (hashed into the manifest, to tell an expected failure from a
/// drifted one); with expected failures gone it survives purely as the
/// REGRESSION MESSAGE, which is the job it was always best at — it
/// names what diverged without pinning slots or numbers that shift
/// harmlessly. `classes` = the retail obs slot → (class, model) map
/// (family-neutral).
pub(crate) fn signature(pd: &PairDiff, classes: &BTreeMap<u16, (u8, u8)>) -> Vec<String> {
    let mut atoms: Vec<String> = Vec::new();
    if pd.rng_want != pd.rng_got {
        atoms.push("rng".into());
    }
    for (_, c, m) in &pd.missing {
        atoms.push(format!("missing:{c},{m}"));
    }
    for (_, c, m) in &pd.extra {
        atoms.push(format!("extra:{c},{m}"));
    }
    for f in &pd.fields {
        match f.slot.and_then(|s| classes.get(&s)) {
            Some((c, m)) => atoms.push(format!("field:{c},{m}:{}", f.field)),
            None => atoms.push(format!("field:{}", f.field)),
        }
    }
    atoms.sort();
    atoms.dedup();
    atoms
}

/// Slot → (class, model) from an MC1 obs (the MC2 twin lives in
/// `verify_mc2::class_map_mc2`).
fn class_map_mc1(retail: &ObsMc1) -> BTreeMap<u16, (u8, u8)> {
    retail
        .entities
        .iter()
        .map(|e| (e.slot, (e.class, e.model)))
        .collect()
}

/// Rebuild the world before EVERY graded pair (`MGC_FIXTURE_ISOLATE`).
///
/// The suite ticks every selected pair on ONE world, so a pair's
/// verdict can depend on how many unrelated pairs ran before it:
/// `mc2l0 t=3918` conforms inside the full 1,740-pair stream and
/// DRIFTS (`missing:11,17 missing:11,32 missing:5,4`) when selected
/// alone, because `retail_import_mc2` does not reset every cross-pair
/// latch. A recorded expectation that only reproduces after 60-80
/// unrelated pairs is not a property of the fixture.
///
/// One file per fixture executes alone by construction, so this
/// toggle measures — against the untouched committed bundles, with no
/// cutter in the loop — exactly which verdicts are artifacts of the
/// shared world. A whole-process arm, read once, off by default.
fn isolate_worlds() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_FIXTURE_ISOLATE").is_some())
}

/// Stream `path`, hand every fixture-grade pair to `f` as
/// `(t, pair-diff, retail slot → (class, model))`. `select` limits
/// execution to the listed pair ticks (the input ring is fed on every
/// tick regardless, so a sparse selection still sees the right
/// delayed commands).
fn for_each_pair(
    path: &Path,
    baked: &Path,
    input_delay: u64,
    pin_n1: bool,
    select: Option<&std::collections::BTreeSet<u64>>,
    mut f: impl FnMut(u64, PairDiff, &BTreeMap<u16, (u8, u8)>) -> Result<(), String>,
) -> Result<(), String> {
    let mut rec = Recording::open(path)?;
    let game = rec.header.game.clone();
    if rec.header.family()? == mgc_formats::mgcr::Family::Mc2 {
        drop(rec);
        return for_each_pair_mc2(path, baked, input_delay, pin_n1, select, f);
    }
    let level = rec.header.level.ok_or("recording has no level number")?;
    let (mut world, pristine) = verify::build_world(baked, &game, level)?;

    let _ = input_delay; // superseded by the dw_0 cast lane
    let mut prev: Option<(u64, RetailMc1, PlayerCommand)> = None;
    let mut prev_cmd = PlayerCommand::default();
    // Measured terrain (format-2 channel), same pending-block pattern
    // as verify-deltas — the suite MUST run pairs on the same terrain
    // the triage run graded them under.
    let mut timg = rec
        .header
        .channels
        .terrain
        .as_ref()
        .map(mgc_formats::mgcr::TerrainImage::new);
    let mut pending_terrain: Option<mgc_formats::mgcr::TerrainBlock> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r?;
        if let Some(img) = timg.as_mut() {
            if let Some(block) = pending_terrain.take() {
                img.apply(&block)
                    .map_err(|e| format!("t={}: terrain: {e}", tick.t))?;
            }
            pending_terrain = tick.terrain.clone();
        }
        let Some(state) = &tick.state else {
            prev = None;
            continue;
        };
        let st = decode_retail_mc1(state)?;
        // The cast lane rides the state's own dw_0 (consumed fire
        // bits) — the same lane verify-deltas grades on; see
        // verify::fire_bits_mc1.
        let cmd = verify::fire_bits_mc1(&st);
        if let Some((pt, pst, pcmd)) = prev.take() {
            // THE EQUIP/DEMOLISH RECOVERY LANE. Only FIRE rides the raw
            // input channel; equips and demolish are rebuilt FROM THE
            // PAIR (a recorded hand change across it replays as the
            // equip command, the dw==48 word as the demolish) — see the
            // twin in `verify::run`. The suite MUST reconstruct input
            // exactly like verify-deltas or its verdicts diverge from
            // the triage run that recorded them; the MC2 loop below
            // says so in as many words and does it. This MC1 loop did
            // not, so the port's hands never changed under the suite
            // and every equip-lane pair failed on a `wizard0.hand_*`
            // row the triage run cannot reproduce — three fixtures
            // (`hand-resolution`, both `equip-cast-input-reconstruction`
            // exemplars) were filed as port leads by a harness gap, and
            // `castle-z-equip-recon` carried a phantom second atom.
            let pcmd = {
                let rec = mgc_formats::recover::recover_pair_mc1(&pst, &st, tick.input.as_ref());
                PlayerCommand {
                    equip_left: rec.equip_left.map(mgc_sim::mc1::spells::SpellId),
                    equip_right: rec.equip_right.map(mgc_sim::mc1::spells::SpellId),
                    demolish: rec.demolish,
                    ..pcmd
                }
            };
            let wanted = select.is_none_or(|s| s.contains(&pt));
            if tick.t == pt + 1 && wanted {
                let obs: ObsMc1 = match &tick.obs {
                    Some(v) => {
                        serde_json::from_value(v.clone()).map_err(|e| format!("obs: {e}"))?
                    }
                    None => return Err(format!("t={}: no obs channel", tick.t)),
                };
                if verify::capture_clean(&pst, &obs) {
                    if isolate_worlds() {
                        world = verify::build_world(baked, &game, level)?.0;
                    }
                    let (pd, _, _) = verify::exec_pair(
                        &mut world,
                        &pristine,
                        verify::measured_planes(&timg),
                        &pst,
                        &st,
                        &obs,
                        pcmd,
                        prev_cmd,
                        // The frozen-law suite follows the pose-pair
                        // A/B (`MGC_POSE_PAIR=1`) so a fixture's
                        // signature can be re-graded under the
                        // two-phase walk before the default flips.
                        if verify::pose_pair() {
                            verify::PairPose::Pair
                        } else if pin_n1 {
                            verify::PairPose::PinN1
                        } else {
                            verify::PairPose::PinN
                        },
                    )
                    .map_err(|e| format!("t={pt}: {e}"))?;
                    f(pt, pd, &class_map_mc1(&obs))?;
                }
            }
            // See the MC2 twin below: the old `&prev` read landed
            // after `prev.take()` had emptied it, so the cast EDGE
            // degenerated to the raw HELD level.
            prev_cmd = pcmd;
        }
        prev = Some((tick.t, st, cmd));
    }
    Ok(())
}

/// The MC2 twin of [`for_each_pair`]: phase-byte tear gate from the
/// RAW states; casts from the MC2 raw externals when the take carries
/// them (held ∥ latch through the same delay ring), default commands
/// otherwise.
fn for_each_pair_mc2(
    path: &Path,
    baked: &Path,
    input_delay: u64,
    pin_n1: bool,
    select: Option<&std::collections::BTreeSet<u64>>,
    mut f: impl FnMut(u64, PairDiff, &BTreeMap<u16, (u8, u8)>) -> Result<(), String>,
) -> Result<(), String> {
    use mgc_formats::mgcr::{ObsMc2, RetailMc2, decode_retail_mc2};
    let mut rec = Recording::open(path)?;
    let level = rec.header.level.ok_or("recording has no level number")?;
    let (mut world, pristine, things) = crate::verify_mc2::build_world_mc2(baked, level)?;

    let _ = input_delay; // superseded by the latch-aligned cast phase
    let mut prev: Option<(u64, RetailMc2, PlayerCommand)> = None;
    let mut prev_latch = (false, false);
    let mut prev_press: Option<(i16, i16)> = None;
    let mut timg = rec
        .header
        .channels
        .terrain
        .as_ref()
        .map(mgc_formats::mgcr::TerrainImage::new);
    let mut pending_terrain: Option<mgc_formats::mgcr::TerrainBlock> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r?;
        if let Some(img) = timg.as_mut() {
            if let Some(block) = pending_terrain.take() {
                img.apply(&block)
                    .map_err(|e| format!("t={}: terrain: {e}", tick.t))?;
            }
            pending_terrain = tick.terrain.clone();
        }
        let Some(state) = &tick.state else {
            prev = None;
            continue;
        };
        let st = decode_retail_mc2(state)?;
        // THE LATCH-ALIGNED CAST PHASE — `verify_mc2::align_cmd_mc2` is
        // the law and its doc-comment the derivation. The suite MUST
        // reconstruct input exactly like `verify-deltas` or its
        // signatures drift from the triage run, so this loop carries no
        // `--input-delay` ring either.
        let (held, latch) = crate::verify_mc2::raw_input_mc2(tick.input.as_ref());
        let mut cmd = crate::verify_mc2::align_cmd_mc2(held, latch, prev_latch);
        // The cursor-AT-PRESS A/B (`verify_mc2::press_edge_mc2`) must be
        // reconstructed here too or a suite run under the toggle would
        // disagree with the triage run it is meant to pin.
        let press = crate::verify_mc2::press_pos_mc2(tick.input.as_ref());
        if std::env::var_os("MGC_PRESS_EDGE").is_some() {
            let moved = matches!((prev_press, press), (Some(a), Some(b)) if a != b);
            cmd = crate::verify_mc2::press_edge_mc2(cmd, held, latch, moved);
        }
        prev_press = press.or(prev_press);
        prev_latch = latch;
        if let Some((pt, pst, pcmd)) = prev.take() {
            let wanted = select.is_none_or(|s| s.contains(&pt));
            if tick.t == pt + 1 && wanted && crate::verify_mc2::capture_clean_mc2(&pst, &st) {
                let obs: ObsMc2 = match &tick.obs {
                    Some(v) => {
                        serde_json::from_value(v.clone()).map_err(|e| format!("obs: {e}"))?
                    }
                    None => return Err(format!("t={}: no obs channel", tick.t)),
                };
                // The pair IS frame pt+1's transition, so it takes THIS
                // record's aligned command, with the pair's START record
                // as the edge predecessor.
                if isolate_worlds() {
                    world = crate::verify_mc2::build_world_mc2(baked, level)?.0;
                }
                let (pd, _, _) = crate::verify_mc2::exec_pair_mc2(
                    &mut world,
                    &pristine,
                    verify::measured_planes(&timg),
                    &things,
                    &pst,
                    &st,
                    &obs,
                    cmd,
                    pcmd,
                    pin_n1,
                )
                .map_err(|e| format!("t={pt}: {e}"))?;
                f(pt, pd, &crate::verify_mc2::class_map_mc2(&obs))?;
            }
        }
        prev = Some((tick.t, st, cmd));
    }
    Ok(())
}

// ---------------------------------------------------------------- extract

/// `extract <rec.mgcr> --out <manifest.json>`: run the full pass,
/// sample the CONFORMING pairs into a candidate manifest, and REPORT
/// the failing ones — deduped by signature, minimal exemplar first
/// (fewest atoms, then earliest) — to stdout for the ledger.
///
/// Failing pairs are no longer written as fixtures. They used to be,
/// as `open`, and that is exactly how a corpus of 7,830 accumulated in
/// which 96% carried no note and no status: an extract that files its
/// own unexplained divergences as fixtures turns a triage queue into a
/// test suite. A fixture asserts fixed work, so a failing pair belongs
/// in docs/CONFORMANCE-FINDINGS.md (and, if it should be excused
/// during triage, in `known-deviations.json`) until someone fixes the
/// law — at which point it is cut in deliberately, by name.
///
/// The sampled conforming pairs are CANDIDATES, not a suite: they land
/// unnamed, and `cut_fixture_files.py` + a human decide which ones pin
/// a law worth a file.
pub fn extract(path: &Path, args: &Args) -> i32 {
    let Some(out) = args.out.clone() else {
        eprintln!("extract: --out <manifest.json> required");
        return 2;
    };
    match extract_inner(path, args, &out) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            2
        }
    }
}

fn extract_inner(path: &Path, args: &Args, out: &Path) -> Result<(), String> {
    // atoms-joined → (atom_count, t) — the minimal exemplar per story.
    let mut best: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut conforming: Vec<u64> = Vec::new();
    let (mut pairs, mut failing) = (0u64, 0u64);
    for_each_pair(
        path,
        &args.baked,
        args.input_delay,
        args.pin_pose == "n1",
        None,
        |t, pd, classes| {
            pairs += 1;
            if pd.clean() {
                conforming.push(t);
                return Ok(());
            }
            failing += 1;
            let atoms = signature(&pd, classes);
            let n = atoms.len();
            let e = best.entry(atoms.join(" ")).or_insert((usize::MAX, 0));
            if n < e.0 {
                *e = (n, t);
            }
            Ok(())
        },
    )?;

    // Candidate corpus: every Kth conforming pair, unnamed.
    let k = args.sample_every.max(1) as usize;
    let fixtures: Vec<Fixture> = conforming
        .iter()
        .step_by(k)
        .map(|&t| Fixture {
            t,
            file: String::new(),
            source: String::new(),
            note: String::new(),
        })
        .collect();

    // The recording path, relative to the manifest's directory.
    let rel = pathdiff(path, out.parent().unwrap_or(Path::new(".")));
    // An un-cut extract: it points at the source take and carries no
    // `dir`, so the runner refuses it until cut_fixture_files.py has
    // turned each pair into its own named evidence file.
    let manifest = Manifest {
        dir: String::new(),
        recording: rel,
        pin_pose: args.pin_pose.clone(),
        fixtures,
    };
    write_manifest(out, &manifest)?;
    println!(
        "extracted {}: {pairs} pairs, {} conforming ({} sampled every {k}), \
         {failing} failing across {} stories",
        out.display(),
        conforming.len(),
        conforming.len().div_ceil(k),
        best.len(),
    );
    // The triage queue — printed, never written as fixtures. Minimal
    // exemplar first, so the shortest repro of each story leads.
    if !best.is_empty() {
        let mut stories: Vec<(&String, &(usize, u64))> = best.iter().collect();
        stories.sort_by_key(|(_, (n, t))| (*n, *t));
        println!(
            "   failing stories (ledger these; NOT fixtures until fixed), \
             minimal exemplar first:"
        );
        for (atoms, (_, t)) in stories {
            println!("     t={t:<8} {atoms}");
        }
    }
    Ok(())
}

fn pathdiff(target: &Path, base: &Path) -> String {
    // Minimal relative-path derivation: both our layouts are
    // repo-relative invocations; fall back to the target verbatim.
    let (t, b) = (
        target.canonicalize().unwrap_or(target.into()),
        base.canonicalize().unwrap_or(base.into()),
    );
    match t.strip_prefix(&b) {
        Ok(p) => p.display().to_string(),
        Err(_) => {
            let mut ups = PathBuf::new();
            let mut anc = b.as_path();
            loop {
                match t.strip_prefix(anc) {
                    Ok(p) => return ups.join(p).display().to_string(),
                    Err(_) => match anc.parent() {
                        Some(pa) => {
                            ups.push("..");
                            anc = pa;
                        }
                        None => return target.display().to_string(),
                    },
                }
            }
        }
    }
}

fn write_manifest(path: &Path, m: &Manifest) -> Result<(), String> {
    let s = serde_json::to_string_pretty(m).map_err(|e| e.to_string())?;
    std::fs::write(path, s + "\n").map_err(|e| format!("{}: {e}", path.display()))
}

fn read_manifest(path: &Path) -> Result<Manifest, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))
}

// -------------------------------------------------------------------- run

/// The suite verdict for one manifest.
#[derive(Default)]
pub struct SuiteReport {
    pub ran: u64,
    pub ok: u64,
    /// Fixtures that now fail — the red signal, and the only one.
    pub regressions: Vec<u64>,
    /// Manifest pairs the stream never yielded (gap/torn/missing).
    pub skipped: Vec<u64>,
}

/// `fixtures <manifest.json>…`: replay every declared fixture. A
/// fixture passes or it is a REGRESSION; exit 1 on any. Nothing here
/// rewrites a manifest — the suite is a pure reader, so a test run can
/// never launder a failure into a new expectation.
pub fn run(paths: &[PathBuf], args: &Args) -> i32 {
    let mut code = 0;
    for p in paths {
        match run_one(p, args) {
            Ok(rep) => {
                if !rep.regressions.is_empty() {
                    code = code.max(1);
                }
            }
            Err(e) => {
                eprintln!("{}: {e}", p.display());
                code = 2;
            }
        }
    }
    code
}

pub fn run_one(manifest_path: &Path, args: &Args) -> Result<SuiteReport, String> {
    let manifest = read_manifest(manifest_path)?;
    let base = manifest_path.parent().unwrap_or(Path::new("."));
    if manifest.dir.is_empty() {
        return Err("manifest has no `dir` — it is an un-cut extract; run \
             conformance/cut_fixture_files.py on it first (docs/CONFORMANCE.md)"
            .into());
    }
    let dir = base.join(&manifest.dir);
    // THE DECLARED LIST IS THE POINT. With statuses gone the manifest
    // is little more than a list of file names — but a list is exactly
    // what a bare directory cannot be checked against, and both
    // directions have caught real mistakes: an UNDECLARED file (two
    // orphans survived a rename, silently testing nothing) and a
    // DECLARED file that is missing (a fixture deleted without its
    // entry, which would otherwise just shrink the suite in silence).
    // Both are hard errors, named.
    let declared: std::collections::BTreeSet<&str> =
        manifest.fixtures.iter().map(|f| f.file.as_str()).collect();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut orphans: Vec<String> = rd
            .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|n| n.ends_with(".mgcr") && !declared.contains(n.as_str()))
            .collect();
        if !orphans.is_empty() {
            orphans.sort();
            return Err(format!(
                "{} evidence file(s) on disk but not declared — a suite \
                 tests what the manifest LISTS, so these are running nothing: {}",
                orphans.len(),
                orphans.join(", ")
            ));
        }
    }
    // ONE FILE, ONE PAIR, ONE WORLD. The bundle runner built a single
    // world and ticked every selected pair on it, so a verdict could
    // depend on how many unrelated pairs ran first — measured on
    // mc2l0, where t=11334 needed 400-800 preceding pairs to conform
    // and t=3918 needed 60-200. Per-file, each fixture gets its own
    // `for_each_pair` call and therefore its own freshly built world,
    // so a verdict is a property of the fixture. Isolation was
    // measured verdict-neutral on every surviving take (7,828 of
    // 7,830 corpus-wide) before this landed.
    let mut results: BTreeMap<u64, Vec<String>> = BTreeMap::new(); // t → atoms (empty = pass)
    for f in &manifest.fixtures {
        if f.file.is_empty() {
            return Err(format!(
                "fixture t={} has no `file` — re-cut the manifest",
                f.t
            ));
        }
        let path = dir.join(&f.file);
        if !path.exists() {
            return Err(format!(
                "fixture t={} ({}) is missing its evidence file {}",
                f.t,
                f.law(),
                path.display()
            ));
        }
        let one = std::collections::BTreeSet::from([f.t]);
        for_each_pair(
            &path,
            &args.baked,
            0,
            manifest.pin_pose == "n1",
            Some(&one),
            |t, pd, classes| {
                let atoms = if pd.clean() {
                    Vec::new()
                } else {
                    signature(&pd, classes)
                };
                results.insert(t, atoms);
                Ok(())
            },
        )
        .map_err(|e| format!("{}: {e}", f.law()))?;
    }

    let mut rep = SuiteReport::default();
    for f in &manifest.fixtures {
        let Some(atoms) = results.get(&f.t) else {
            rep.skipped.push(f.t);
            continue;
        };
        rep.ran += 1;
        if atoms.is_empty() {
            rep.ok += 1;
        } else {
            rep.regressions.push(f.t);
            // The law is the headline; the atoms say what moved. There
            // is nothing to compare them against any more, and that is
            // the point — a fixture exists because its law WORKS, so
            // any divergence at all is the regression.
            println!("  REGRESSION {} (t={}): {}", f.law(), f.t, atoms.join(" "));
        }
    }
    println!(
        "== {}: {} fixtures ran, {} pass, {} regressions, {} not reached",
        manifest_path.display(),
        rep.ran,
        rep.ok,
        rep.regressions.len(),
        rep.skipped.len()
    );
    Ok(rep)
}
