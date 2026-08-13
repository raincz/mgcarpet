//! The conformance fixture SUITE (docs/CONFORMANCE.md): failing (and
//! sampled conforming) pairs of a retail recording, encoded as a
//! small committed manifest and replayed as an automated expected-
//! status test.
//!
//! A fixture is a REFERENCE — `(recording, pair t)` — not a copy: the
//! `.mgcr` tick records are self-contained, so the runner streams the
//! source recording once and replays exactly the manifest's pairs
//! through the same import-tick-diff core as `verify-deltas`
//! (`verify::exec_pair`). The manifest carries each pair's expected
//! status and its diff SIGNATURE (the slot- and value-free set of
//! (kind, class, model, field) atoms), so the suite detects three
//! events: a conforming pair regressing, an expected-failing pair
//! flipping to pass (progress — acknowledge with `--promote`), and a
//! pair failing DIFFERENTLY than recorded (signature drift, warning).

use crate::Args;
use crate::verify::{self, PairDiff};
use mgc_formats::mgcr::{ObsMc1, Recording, RetailMc1, decode_retail_mc1};
use mgc_sim::engine::world::PlayerCommand;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Regression guard: the pair conformed when extracted and must
    /// stay green.
    Conforming,
    /// A known-failing port lead (ledger-tracked): expected to fail
    /// with the recorded signature until the law is fixed.
    Open,
    /// A capture-domain limitation (terrain closure, input latency):
    /// expected to fail; not a port bug. Kept for drift tracking and
    /// for the day the closure gap itself is fixed.
    Capture,
}

#[derive(Serialize, Deserialize)]
pub struct Fixture {
    pub t: u64,
    pub status: Status,
    /// FNV-1a hash of the atom set (empty for conforming pairs).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sig: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub atoms: Vec<String>,
    /// Free-form triage note (ledger entry, family name).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    /// Path to the source recording, relative to the manifest's dir.
    pub recording: String,
    pub input_delay: u64,
    pub pin_pose: String,
    pub fixtures: Vec<Fixture>,
}

/// The slot- and value-free signature of a divergent pair: sorted,
/// deduped atoms — `rng`, `missing:c,m`, `extra:c,m`,
/// `field:c,m:name` (entity fields, class/model from the retail obs)
/// and `field:name` (wizard/player scalars). Stable across runs and
/// across small positional drifts; changes when the STORY of the
/// failure changes. `classes` = the retail obs slot → (class, model)
/// map (family-neutral).
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

pub(crate) fn sig_hash(atoms: &[String]) -> String {
    // FNV-1a 64 — deliberately hand-rolled: std's DefaultHasher is
    // not stable across toolchains and the hash is a committed value.
    let mut h: u64 = 0xcbf29ce484222325;
    for a in atoms {
        for b in a.as_bytes().iter().chain(b"|") {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
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

    let mut prev: Option<(u64, RetailMc1, PlayerCommand)> = None;
    let mut prev_cmd = PlayerCommand::default();
    let mut cmd_ring: std::collections::VecDeque<PlayerCommand> =
        std::iter::repeat_n(PlayerCommand::default(), input_delay as usize + 1).collect();
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
        let sampled = tick
            .input
            .as_ref()
            .and_then(|i| i.get("mouse_buttons"))
            .map(|b| PlayerCommand {
                fire_left: b.get("left").and_then(|v| v.as_bool()).unwrap_or(false),
                fire_right: b.get("right").and_then(|v| v.as_bool()).unwrap_or(false),
                ..Default::default()
            })
            .unwrap_or_default();
        cmd_ring.push_back(sampled);
        let cmd = cmd_ring.pop_front().unwrap_or_default();
        if let Some((pt, pst, pcmd)) = prev.take() {
            let wanted = select.is_none_or(|s| s.contains(&pt));
            if tick.t == pt + 1 && wanted {
                let obs: ObsMc1 = match &tick.obs {
                    Some(v) => {
                        serde_json::from_value(v.clone()).map_err(|e| format!("obs: {e}"))?
                    }
                    None => return Err(format!("t={}: no obs channel", tick.t)),
                };
                if verify::capture_clean(&pst, &obs) {
                    let (pd, _, _) = verify::exec_pair(
                        &mut world,
                        &pristine,
                        verify::measured_planes(&timg),
                        &pst,
                        &st,
                        &obs,
                        pcmd,
                        prev_cmd,
                        pin_n1,
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
/// dedup failing pairs by signature keeping the MINIMAL exemplar
/// (fewest atoms, then earliest), sample conforming pairs as the
/// regression corpus, and write the manifest. Everything failing is
/// written `open` — reclassifying to `capture` (and the notes) is the
/// triage's job, by hand or scripted against the ledger.
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
    // sig → (atom_count, t, atoms) — the best exemplar seen.
    let mut best: BTreeMap<String, (usize, u64, Vec<String>)> = BTreeMap::new();
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
            let sig = sig_hash(&atoms);
            let n = atoms.len();
            let e = best.entry(sig).or_insert((usize::MAX, 0, Vec::new()));
            if n < e.0 {
                *e = (n, t, atoms);
            }
            Ok(())
        },
    )?;

    // Regression corpus: every Kth conforming pair.
    let k = args.sample_every.max(1) as usize;
    let mut fixtures: Vec<Fixture> = conforming
        .iter()
        .step_by(k)
        .map(|&t| Fixture {
            t,
            status: Status::Conforming,
            sig: String::new(),
            atoms: Vec::new(),
            note: String::new(),
        })
        .collect();

    // Open exemplars: minimal repros first, capped.
    let mut open: Vec<(&String, &(usize, u64, Vec<String>))> = best.iter().collect();
    open.sort_by_key(|(_, (n, t, _))| (*n, *t));
    let kept = open.len().min(args.max_open);
    for (sig, (_, t, atoms)) in open.iter().take(args.max_open) {
        fixtures.push(Fixture {
            t: *t,
            status: Status::Open,
            sig: (*sig).clone(),
            atoms: (*atoms).clone(),
            note: String::new(),
        });
    }
    fixtures.sort_by_key(|f| f.t);

    // The recording path, relative to the manifest's directory.
    let rel = pathdiff(path, out.parent().unwrap_or(Path::new(".")));
    let manifest = Manifest {
        recording: rel,
        input_delay: args.input_delay,
        pin_pose: args.pin_pose.clone(),
        fixtures,
    };
    write_manifest(out, &manifest)?;
    println!(
        "extracted {}: {pairs} pairs, {} conforming ({} sampled every {k}), \
         {failing} failing across {} signatures ({kept} exemplars kept, cap {})",
        out.display(),
        conforming.len(),
        conforming.len().div_ceil(k),
        best.len(),
        args.max_open,
    );
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
    /// Conforming fixtures that now fail — the red signal.
    pub regressions: Vec<u64>,
    /// Regressions acknowledged by `--demote` (status → open).
    pub demoted: Vec<u64>,
    /// Open/capture fixtures that now PASS — progress, but red until
    /// promoted so the manifest never silently rots.
    pub fixed: Vec<u64>,
    /// Expected-failing fixtures whose signature changed (warning).
    pub drifted: Vec<u64>,
    /// Manifest pairs the stream never yielded (gap/torn/missing).
    pub skipped: Vec<u64>,
}

/// `fixtures <manifest.json>… [--promote]`: replay every manifest
/// pair, enforce expected statuses. Exit 1 on regressions or
/// unacknowledged fixes; `--promote` accepts fixes (status →
/// conforming) and refreshes drifted signatures, rewriting the
/// manifest.
pub fn run(paths: &[PathBuf], args: &Args) -> i32 {
    let mut code = 0;
    for p in paths {
        match run_one(p, args) {
            Ok(rep) => {
                if !rep.regressions.is_empty() || (!rep.fixed.is_empty() && !args.promote) {
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
    let mut manifest = read_manifest(manifest_path)?;
    let rec_path = manifest_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(&manifest.recording);
    let select: std::collections::BTreeSet<u64> = manifest.fixtures.iter().map(|f| f.t).collect();
    let mut results: BTreeMap<u64, Vec<String>> = BTreeMap::new(); // t → atoms (empty = pass)
    for_each_pair(
        &rec_path,
        &args.baked,
        manifest.input_delay,
        manifest.pin_pose == "n1",
        Some(&select),
        |t, pd, classes| {
            let atoms = if pd.clean() {
                Vec::new()
            } else {
                signature(&pd, classes)
            };
            results.insert(t, atoms);
            Ok(())
        },
    )?;

    let mut rep = SuiteReport::default();
    let mut changed = false;
    for f in &mut manifest.fixtures {
        let Some(atoms) = results.get(&f.t) else {
            rep.skipped.push(f.t);
            continue;
        };
        rep.ran += 1;
        let pass = atoms.is_empty();
        match (f.status, pass) {
            (Status::Conforming, true) => rep.ok += 1,
            (Status::Conforming, false) => {
                // --demote: the deliberate twin of --promote, for when
                // a NEW comparison lane reveals a frozen-conforming
                // pair was never truly conforming — acknowledge it as
                // the open lead it always was, with attribution.
                if let Some(note) = &args.demote {
                    f.status = Status::Open;
                    f.sig = sig_hash(atoms);
                    f.atoms = atoms.clone();
                    if f.note.is_empty() {
                        f.note = note.clone();
                    } else {
                        f.note = format!("{}; {note}", f.note);
                    }
                    changed = true;
                    rep.demoted.push(f.t);
                    println!("  DEMOTED t={}: open, {}", f.t, atoms.join(" "));
                } else {
                    rep.regressions.push(f.t);
                    println!(
                        "  REGRESSION t={}: was conforming, now {}",
                        f.t,
                        atoms.join(" ")
                    );
                }
            }
            (_, true) => {
                rep.fixed.push(f.t);
                if args.promote {
                    f.status = Status::Conforming;
                    f.sig = String::new();
                    f.atoms = Vec::new();
                    changed = true;
                    println!("  PROMOTED t={}: now conforming", f.t);
                } else {
                    println!(
                        "  FIXED t={}: expected {:?} failure, now conforming — \
                         re-run with --promote to accept",
                        f.t, f.status
                    );
                }
            }
            (_, false) => {
                let sig = sig_hash(atoms);
                if sig == f.sig {
                    rep.ok += 1;
                } else {
                    rep.drifted.push(f.t);
                    println!(
                        "  drift t={} ({:?}): {} → {}",
                        f.t,
                        f.status,
                        f.atoms.join(" "),
                        atoms.join(" ")
                    );
                    if args.promote {
                        f.sig = sig;
                        f.atoms = atoms.clone();
                        changed = true;
                    }
                }
            }
        }
    }
    if changed {
        write_manifest(manifest_path, &manifest)?;
        println!("  manifest rewritten: {}", manifest_path.display());
    }
    println!(
        "== {}: {} fixtures ran, {} as expected, {} regressions, {} demoted, \
         {} fixed, {} drifted, {} not reached",
        manifest_path.display(),
        rep.ran,
        rep.ok,
        rep.regressions.len(),
        rep.demoted.len(),
        rep.fixed.len(),
        rep.drifted.len(),
        rep.skipped.len()
    );
    Ok(rep)
}
