//! `mgc-conform` — the `.mgcr` conformance fixture runner
//! (docs/RECORDING.md "Consumers → The fixture runner").
//!
//! Modes:
//! - `check-decode <file.mgcr>…` — re-decode every tick's raw
//!   `state.struct_b64` through the Rust decoder and demand value
//!   equality with the recording's own `obs` channel. Pins the Rust
//!   decode against the recorder's (the corpus was certified
//!   obs↔state-coherent at record time, so any mismatch is ours).
//! - `verify-deltas <file.mgcr>` — the retail conformance mode:
//!   import the raw state at tick N into a freshly-built world, tick
//!   once, diff the port's obs projection against the recorded obs at
//!   N+1 (adjacent pairs only; gaps break pairing, never the run).

mod explain;
mod fixtures;
mod jsondiff;
mod pose_lane;
mod replay;
mod roster;
mod shadow;
mod verify;
mod verify_mc2;

use mgc_formats::mgcr::{Obs, Recording};
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "usage: mgc-conform <mode> [args]\n\
         \n\
         modes:\n\
           check-decode <file.mgcr>…      re-decode state, compare vs stored obs\n\
           terrain-diff <file.mgcr>…      diff the take's measured terrain base\n\
                                          against the port's generated planes\n\
                                          (the record-0 stock-bake validator)\n\
           verify-deltas <file.mgcr>      import state@N, tick, diff obs@N+1\n\
           replay <file.mgcr>             PURE INPUT REPLAY: seed once from the\n\
                                          first closure, free-run on the recovered\n\
                                          input stream, report (never correct)\n\
                                          divergence at every recorded boundary;\n\
                                          gaps re-anchor a fresh segment\n\
           dump-state <file.mgcr> <t> <slot>…   print raw retail fields of\n\
                                          the given slots at tick t\n\
             --port          MC1: free-run the PORT to t (anchor at the\n\
                             take's seed, or --start <t0>; --start t-1 =\n\
                             the pair-import view) and print every lane\n\
                             side by side with retail's, ≠-marked — the\n\
                             instrument that says what the PORT holds\n\
             --at-slot <n>   sample MID-WALK: snapshot the pool as the\n\
                             tick INTO t reaches slot n, before n\n\
                             dispatches (\"what did slot A hold when\n\
                             slot B ran\")\n\
           explain <file.mgcr> <t> [<slot>…]   retail's OWN t-1 → t\n\
                                          changelog — what CHANGED, not\n\
                                          what differs: records born/\n\
                                          freed or whose class/life-sign/\n\
                                          f70/owner moved, in full; named\n\
                                          slots always print, plus every\n\
                                          record they point at (f146/f52/\n\
                                          f54/f144/f42/f38/f40, mail\n\
                                          sources); wizard + global deltas\n\
           extract <file.mgcr> --out <manifest.json>   lift a fixture-suite\n\
                                          manifest (docs/CONFORMANCE.md)\n\
           fixtures <manifest.json>…      run a fixture suite, enforcing\n\
                                          expected statuses\n\
         \n\
         common flags:\n\
           --max-diffs <n>   mismatch paths printed per tick (default 8)\n\
           --limit <n>       stop after n tick records / pairs (default: all)\n\
         terrain-diff flags:\n\
           --baked <dir>     baked tree root (default: baked)\n\
           --out <dir>       dump both sides of every plane as raw 256x256\n\
                             byte images (<plane>.retail / <plane>.port) for\n\
                             offline clustering\n\
           --baseline <dir>  read the MEASURED planes from an earlier --out\n\
                             dump instead of this take's record-0 base (keeps\n\
                             an attribution reproducible after a re-record)\n\
         extract flags:\n\
           --out <path>          manifest destination (required)\n\
           --sample-every <n>    conforming-pair sampling stride (default 10).\n\
                                 Failing pairs are PRINTED for the ledger,\n\
                                 never written as fixtures — a fixture\n\
                                 asserts fixed work (CONFORMANCE.md)\n\
         verify-deltas flags:\n\
           --baked <dir>     baked tree root (default: baked)\n\
           --pin-pose n|n1   drive the human with the pre- or post-tick\n\
                             recorded pose (default n1, the app's phase)\n\
           --dump <t>        print the full diff of pair t→t+1\n\
           --dump-first      print the first divergent pair in full\n\
           --csv <path>      write every per-pair diff as a TSV row\n\
                             (t, kind, slot, class, model, field, want,\n\
                             got, x, y, z, rule — for offline triage)\n\
           --no-roster       skip conformance/known-deviations.json\n\
           --no-pose-alt     skip the pose-phase pass (each dirty pair\n\
                             re-runs under the other --pin-pose sample;\n\
                             rows clean there tag `pose-phase` — retail's\n\
                             within-tick pose is two-valued and the\n\
                             capture holds one sample)\n\
           --no-slot-desync  skip the slot-desync pass (balanced same-\n\
                             (class,model) missing/extra within a pair =\n\
                             free-list slot-order desync at mass-spawn\n\
                             ticks; ledger session-4 + open-leads 0b)\n\
           --no-terrain      ignore the recording's measured terrain\n\
                             channel (format 2) — every pair runs on\n\
                             pristine planes (the A/B for the terrain\n\
                             installation)\n\
           --no-pose-lane    skip the POSE CHANNEL (the shadow mover\n\
                             step verifying the human's own motion\n\
                             column — flight state seeded from N,\n\
                             input recovered from the recorded flight\n\
                             column, pose diffed at N+1 bit-exact)\n\
                             (raw, unclassified report — docs/CONFORMANCE.md)\n\
         replay flags:\n\
           --pose-only       tier-2 chain: the FLIGHT state chains while\n\
                             the world context re-imports per pair —\n\
                             isolates mover + input recovery from world\n\
                             fidelity; world-driven pose domains reseed\n\
                             silently and are counted as gates\n\
           --segmented       SEGMENTED free run: re-anchor the free\n\
                             state from the recording at every true\n\
                             deviation (the way a capture gap already\n\
                             does), so the take reads as maximal\n\
                             continuous segments instead of one horizon\n\
                             plus noise. Certification is ONE segment\n\
                             end to end; the number that matters is\n\
                             resets in EXCESS of the gap-forced ones,\n\
                             and every reset tick names itself as a\n\
                             fixture candidate\n\
           --classify        (--segmented, MC1) run the PAIR at every\n\
                             reset-cluster head and tag it: pair DIRTY\n\
                             at t-1 ⇒ LOCAL (fixture candidate), pair\n\
                             CLEAN ⇒ INHERITED (the one-tick law is\n\
                             right — the break rides earlier state:\n\
                             unit test / upstream dig)\n\
           --brief           one machine-readable line per take\n\
                             (horizon / segments / first divergence /\n\
                             signature) — the corpus regression sweep,\n\
                             diffable against a saved baseline\n\
           --start <t>       anchor the replay at tick t instead of the\n\
                             first record"
    );
    std::process::exit(2);
}

pub struct Args {
    mode: String,
    files: Vec<PathBuf>,
    pub max_diffs: usize,
    pub limit: Option<u64>,
    pub baked: PathBuf,
    pub pin_pose: String,
    pub dump: Option<u64>,
    pub dump_first: bool,
    pub dump_port: bool,
    pub csv: Option<PathBuf>,
    pub out: Option<PathBuf>,
    /// terrain-diff: take the MEASURED planes from a cached `--out`
    /// dump directory instead of the recording's record-0 base.
    pub baseline: Option<PathBuf>,
    pub sample_every: u64,
    /// Feed the input channel k ticks late (retail's mouse→control→
    /// consume pipeline shows ~2-3 ticks of latency vs the sampled
    /// externals).
    pub input_delay: u64,
    /// verify-deltas: skip pairs before this tick (windowed triage;
    /// executed pairs are announced on stderr so an aborting pair
    /// self-incriminates).
    pub start: Option<u64>,
    /// Skip the known-deviation roster (raw, unclassified report).
    pub no_roster: bool,
    pub no_pose_alt: bool,
    /// Skip the computed slot-desync pass (balanced same-(class,model)
    /// missing/extra = free-list slot-order desync).
    pub no_slot_desync: bool,
    /// verify-deltas: ignore the recording's measured terrain channel
    /// and run every pair on pristine planes (the A/B for the format-2
    /// terrain installation).
    pub no_terrain: bool,
    /// Skip the pose channel (the shadow mover step over the human's
    /// own motion column).
    pub no_pose_lane: bool,
    /// replay: tier-2 chain (flight chained, world re-imported per
    /// pair) instead of the full free-running world.
    pub pose_only: bool,
    /// replay: SEGMENTED free run — re-anchor the free state from the
    /// recording at every true deviation instead of running wild after
    /// the first one, so the take reads as maximal continuous segments.
    /// Certification is ONE segment end to end; the number that matters
    /// is resets in EXCESS of the gap-forced ones.
    pub segmented: bool,
    /// replay --segmented (MC1): run the PAIR at every reset-cluster
    /// head and tag it LOCAL (pair dirty ⇒ fixture candidate) or
    /// INHERITED (pair clean ⇒ the break rides earlier state — unit
    /// test / upstream dig). The segmented-residue doctrine, automated.
    pub classify: bool,
    /// replay: one machine-readable summary line per take instead of
    /// the segment report — the whole-corpus regression sweep.
    pub brief: bool,
    /// dump-state --port: sample MID-WALK — snapshot the pool as the
    /// tick into `t` reaches this slot, before it dispatches.
    pub at_slot: Option<u16>,
}

fn parse_args() -> Args {
    let mut a = Args {
        mode: String::new(),
        files: Vec::new(),
        max_diffs: 8,
        limit: None,
        baked: PathBuf::from("baked"),
        pin_pose: "n1".into(),
        dump: None,
        dump_first: false,
        dump_port: false,
        csv: None,
        out: None,
        baseline: None,
        sample_every: 10,
        no_roster: false,
        no_pose_alt: false,
        no_slot_desync: false,
        no_terrain: false,
        no_pose_lane: false,
        pose_only: false,
        segmented: false,
        classify: false,
        brief: false,
        at_slot: None,
        input_delay: 0,
        start: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--max-diffs" => {
                a.max_diffs = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--limit" => {
                a.limit = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage()),
                )
            }
            "--baked" => a.baked = it.next().map(PathBuf::from).unwrap_or_else(|| usage()),
            "--csv" => a.csv = Some(it.next().map(PathBuf::from).unwrap_or_else(|| usage())),
            "--pin-pose" => a.pin_pose = it.next().unwrap_or_else(|| usage()),
            "--dump-first" => a.dump_first = true,
            "--dump-port" => a.dump_port = true,
            "--no-roster" => a.no_roster = true,
            "--no-pose-alt" => a.no_pose_alt = true,
            "--no-slot-desync" => a.no_slot_desync = true,
            "--no-terrain" => a.no_terrain = true,
            "--no-pose-lane" => a.no_pose_lane = true,
            "--pose-only" => a.pose_only = true,
            "--segmented" => a.segmented = true,
            "--classify" => a.classify = true,
            "--brief" => a.brief = true,
            "--port" => a.dump_port = true,
            "--at-slot" => {
                a.at_slot = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage()),
                )
            }
            "--out" => a.out = Some(it.next().map(PathBuf::from).unwrap_or_else(|| usage())),
            "--baseline" => {
                a.baseline = Some(it.next().map(PathBuf::from).unwrap_or_else(|| usage()))
            }
            "--sample-every" => {
                a.sample_every = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--start" => {
                a.start = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage()),
                )
            }
            "--input-delay" => {
                a.input_delay = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--dump" => {
                a.dump = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage()),
                )
            }
            "-h" | "--help" => usage(),
            _ if a.mode.is_empty() => a.mode = arg,
            _ => a.files.push(PathBuf::from(arg)),
        }
    }
    if a.mode.is_empty() || a.files.is_empty() {
        usage();
    }
    a
}

fn main() {
    let args = parse_args();
    // `--out` belongs to terrain-diff (plane dumps) and extract (the
    // manifest destination). Every other mode ignores `args.out`, and
    // silently accepting it reads as "the tool stopped writing my
    // file" — the classic slip is `verify-deltas --out x.tsv` for
    // what is spelled `--csv x.tsv`.
    if args.out.is_some() && !matches!(args.mode.as_str(), "terrain-diff" | "extract") {
        eprintln!(
            "error: --out is not a {} flag (terrain-diff/extract only); \
             the verify-deltas per-pair TSV is written with --csv <path>",
            args.mode
        );
        std::process::exit(2);
    }
    let code = match args.mode.as_str() {
        "check-decode" => args
            .files
            .iter()
            .map(|f| check_decode(f, &args))
            .max()
            .unwrap_or(0),
        "verify-deltas" => args
            .files
            .iter()
            .map(|f| verify::verify_deltas(f, &args))
            .max()
            .unwrap_or(0),
        "replay" => args
            .files
            .iter()
            .map(|f| replay::replay(f, &args))
            .max()
            .unwrap_or(0),
        "dump-state" => dump_state(&args),
        "explain" => explain::explain(&args),
        "ground-audit" => ground_audit(&args),
        "trace" => trace(&args),
        "terrain-diff" => args
            .files
            .iter()
            .map(|f| terrain_diff(f, &args))
            .max()
            .unwrap_or(0),
        "extract" => args
            .files
            .iter()
            .map(|f| fixtures::extract(f, &args))
            .max()
            .unwrap_or(0),
        "fixtures" => fixtures::run(&args.files, &args),
        _ => usage(),
    };
    std::process::exit(code);
}

/// Print the raw retail pool fields of the requested slots at one
/// tick — the triage microscope for divergent pairs (`dump-state
/// <file> <t> <slot>…`).
fn dump_state(args: &Args) -> i32 {
    let (path, rest) = match args.files.split_first() {
        Some(p) => p,
        None => usage(),
    };
    let all = rest.iter().any(|p| p.to_str() == Some("all"));
    let wiz = rest.iter().any(|p| p.to_str() == Some("wiz"));
    let mut it = rest.iter().filter_map(|p| p.to_str()?.parse::<u64>().ok());
    let Some(t) = it.next() else { usage() };
    let slots: Vec<u64> = it.collect();
    if slots.is_empty() && !all && !wiz {
        usage();
    }
    // `--port`: the PORT-side dump — free-run the world to t (see
    // replay::port_dump_mc1) and print every lane of the requested
    // slots side by side with retail's, ≠-marked. The whole point is
    // that every other instrument COMPARES projections or reads the
    // recording; this one says what the port itself holds.
    if args.dump_port {
        if slots.is_empty() {
            eprintln!("dump-state --port wants explicit slot numbers");
            return 2;
        }
        let slots: Vec<u16> = slots.iter().map(|&s| s as u16).collect();
        return replay::port_dump_mc1(path, t, &slots, args);
    }
    let mut rec = match Recording::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return 2;
        }
    };
    let mc2 = rec.header.family() == Ok(mgc_formats::mgcr::Family::Mc2);
    while let Some(r) = rec.next_tick() {
        let tick = match r {
            Ok(t) => t,
            Err(e) => {
                eprintln!("record error: {e}");
                return 2;
            }
        };
        if tick.t != t {
            continue;
        }
        let Some(state) = &tick.state else {
            eprintln!("t={t}: no state channel");
            return 2;
        };
        if mc2 {
            let st = match mgc_formats::mgcr::decode_retail_mc2(state) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("t={t}: {e}");
                    return 2;
                }
            };
            // Pop order: MC2 pops the FREE stack first and sacrifices a
            // recycle victim only when it is dry (`NewEvent_4A050`) —
            // print both tails, next-pop last.
            println!(
                "t={t} free_stack len {} tail {:?}  recycle_stack len {} tail {:?}",
                st.free_stack.len(),
                &st.free_stack[st.free_stack.len().saturating_sub(12)..],
                st.recycle_stack.len(),
                &st.recycle_stack[st.recycle_stack.len().saturating_sub(12)..],
            );
            if all {
                if let Some(p) = st.players.get(st.local_player as usize) {
                    for s in 0..26 {
                        if p.spell_ent[s] == 0 && p.xp_vol[s] == 0 && p.xp_bank[s] == 0 {
                            continue;
                        }
                        println!(
                            "t={t} book spell {s}: ent={} lvl={} sel={} ring={} \
                             xp={}+{}",
                            p.spell_ent[s],
                            p.levels[s],
                            p.sel[s],
                            p.ring[s],
                            p.xp_vol[s],
                            p.xp_bank[s]
                        );
                    }
                }
                for (s, e) in st.ents.iter().enumerate() {
                    if e.class3f == 0 {
                        continue;
                    }
                    println!(
                        "t={t} slot {s}: cm=({},{}) act={} flags={:#x} life={}/{} \
                         pos=({:.2},{:.2},{}) mana={}/{} own={} id={} pe={} \
                         sv=({},{}) tgt={}",
                        e.class3f,
                        e.model40,
                        e.action45,
                        e.flags,
                        e.life,
                        e.max_life,
                        e.x as f64 / 256.0,
                        e.y as f64 / 256.0,
                        e.z,
                        e.mana,
                        e.mana_max,
                        e.owner28,
                        e.f1a,
                        e.player_ent,
                        e.sv1,
                        e.sv2,
                        e.target96
                    );
                }
            }
            for s in &slots {
                println!("t={t} slot {s}: {:#?}", st.ents[*s as usize]);
            }
            return 0;
        }
        let st = match mgc_formats::mgcr::decode_retail_mc1(state) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("t={t}: {e}");
                return 2;
            }
        };
        // Pop order: retail pops the recycle stack first, each stack
        // from its END — print both tails, next-pop last.
        println!(
            "t={t} free_stack len {} tail {:?}  recycle_stack len {} tail {:?}  erupting={} plume={}",
            st.free_stack.len(),
            &st.free_stack[st.free_stack.len().saturating_sub(12)..],
            st.recycle_stack.len(),
            &st.recycle_stack[st.recycle_stack.len().saturating_sub(12)..],
            st.erupting,
            st.plume,
        );
        if wiz || all {
            for (i, w) in st.wizards.iter().enumerate() {
                if w.play_index == 0 {
                    continue;
                }
                let cools: Vec<(usize, u16)> = w
                    .cooldown
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c != 0)
                    .map(|(s, c)| (s, *c))
                    .collect();
                let owned: Vec<(usize, u16)> = w
                    .owned_slots
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| **m != 0)
                    .map(|(s, m)| (s, *m))
                    .collect();
                let learn: Vec<(usize, u16)> = w
                    .learn
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c != 0)
                    .map(|(s, c)| (s, *c))
                    .collect();
                let acq: Vec<i32> = w.spell_list.iter().copied().filter(|&s| s != 0).collect();
                println!(
                    "t={t} wiz {i}: ent={} mv={:#x} hands=({},{}) charge={} \
                     grace={} stall={} rate={} ai_state={} burst={} pov={} \
                     castle={} aggro={} breg={:?} owned={owned:?} cool={cools:?} \
                     learn={learn:?} acq={acq:?}",
                    w.play_index,
                    w.move_bits,
                    w.hand_left,
                    w.hand_right,
                    w.charge,
                    w.grace,
                    w.regen_stall,
                    w.life_rate,
                    w.ai_state,
                    w.burst,
                    w.poverty,
                    w.castle,
                    w.aggro,
                    w.balloon_reg,
                );
            }
        }
        if all {
            for (s, e) in st.ents.iter().enumerate() {
                if e.class64 == 0 {
                    continue;
                }
                println!(
                    "t={t} slot {s}: cm=({},{}) st={} flags={:#x} life={}/{} \
                     pos=({:.2},{:.2},{}) mana={}/{} own={} id={} chase={}",
                    e.class64,
                    e.model65,
                    e.f70,
                    e.flags,
                    e.act_life,
                    e.max_life,
                    e.x as f64 / 256.0,
                    e.y as f64 / 256.0,
                    e.z,
                    e.f140,
                    e.f136,
                    e.f144,
                    e.id24,
                    e.f146
                );
            }
        }
        for s in &slots {
            println!("t={t} slot {s}: {:#?}", st.ents[*s as usize]);
        }
        return 0;
    }
    eprintln!("t={t}: not in recording");
    2
}

/// Compare retail entities' rest-z against the port's generated
/// ground plane at their coordinates (`ground-audit <file.mgcr>
/// [--dump <t>]`). At t=0 no runtime terrain edit exists yet, so the
/// grounded statics (trees, standing fires, huts) sample retail's
/// PRISTINE plane — a generator-fidelity probe that needs no live
/// DOSBox height dump. Late ticks measure edits + shortfall mixed.
fn ground_audit(args: &Args) -> i32 {
    let Some(path) = args.files.first() else {
        usage()
    };
    let t = args.dump.unwrap_or(0);
    let mut rec = match Recording::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return 2;
        }
    };
    let game = rec.header.game.clone();
    let Some(level) = rec.header.level else {
        eprintln!("recording has no level number");
        return 2;
    };
    if rec.header.family() != Ok(mgc_formats::mgcr::Family::Mc1) {
        eprintln!("ground-audit is MC1/HW-only (class-2 snap law)");
        return 2;
    }
    let (world, _) = match verify::build_world(&args.baked, &game, level) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("build: {e}");
            return 2;
        }
    };
    while let Some(r) = rec.next_tick() {
        let tick = match r {
            Ok(x) => x,
            Err(e) => {
                eprintln!("record error: {e}");
                return 2;
            }
        };
        if tick.t != t {
            continue;
        }
        let Some(state) = &tick.state else {
            eprintln!("t={t}: no state channel");
            return 2;
        };
        let st = match mgc_formats::mgcr::decode_retail_mc1(state) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("t={t}: {e}");
                return 2;
            }
        };
        // Residual histogram per (class, model), plus per-site rows
        // for anything off by a height byte or more (|dz| >= 32).
        use std::collections::BTreeMap;
        let mut fam: BTreeMap<(u8, u8), (u64, i64, i64)> = BTreeMap::new();
        let mut sites: BTreeMap<(u16, u16), (u64, i64)> = BTreeMap::new();
        let grounded = |e: &mgc_formats::mgcr::RetailEntMc1| {
            e.class64 == 2
                || (e.class64 == 10 && matches!(e.model65, 0 | 45))
                || (e.class64 == 3 && e.model65 == 2)
                || e.class64 == 5
        };
        for e in st.ents.iter().filter(|e| e.class64 != 0) {
            if !grounded(e) {
                continue;
            }
            let gz = world.ground_z_engine(e.x, e.y);
            let dz = e.z as i64 - gz as i64;
            let f = fam.entry((e.class64, e.model65)).or_default();
            f.0 += 1;
            f.1 += dz;
            f.2 = f.2.max(dz.abs());
            if dz.abs() >= 32 {
                let s = sites.entry((e.x >> 8 & !15, e.y >> 8 & !15)).or_default();
                s.0 += 1;
                s.1 += dz;
            }
        }
        println!("== ground-audit {} t={t}", path.display());
        for ((c, m), (n, sum, max)) in &fam {
            println!(
                "  ({c},{m}): {n} sampled, mean dz {:+.1}, max |dz| {max}",
                *sum as f64 / *n as f64
            );
        }
        println!("  sites with |dz| >= 32 (16-tile grid, count, mean dz):");
        let mut rows: Vec<_> = sites.into_iter().collect();
        rows.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
        for ((sx, sy), (n, sum)) in rows.into_iter().take(24) {
            println!("    ({sx},{sy}): {n}  mean {:+.1}", sum as f64 / n as f64);
        }
        return 0;
    }
    eprintln!("t={t}: not in recording");
    2
}

/// Trace one slot's economy fields across a tick range in a single
/// pass (`trace <file> <slot> <t0> <t1>`): per tick — mana(+140),
/// regen(+132), life(+12), f63, flags. Divergence-cadence microscope.
fn trace(args: &Args) -> i32 {
    let (path, rest) = match args.files.split_first() {
        Some(p) => p,
        None => usage(),
    };
    let nums: Vec<u64> = rest
        .iter()
        .filter_map(|p| p.to_str()?.parse::<u64>().ok())
        .collect();
    let [slot, t0, t1] = nums[..] else { usage() };
    let mut rec = match Recording::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return 2;
        }
    };
    let mc2 = rec.header.family() == Ok(mgc_formats::mgcr::Family::Mc2);
    let mut prev_mana: Option<i32> = None;
    while let Some(r) = rec.next_tick() {
        let Ok(tick) = r else { return 2 };
        if tick.t < t0 {
            continue;
        }
        if tick.t > t1 {
            break;
        }
        let Some(state) = &tick.state else { continue };
        if mc2 {
            let Ok(st) = mgc_formats::mgcr::decode_retail_mc2(state) else {
                return 2;
            };
            let e = &st.ents[slot as usize];
            println!(
                "t={} cm=({},{}) act={} b46={} life={}/{} z={} yaw={} \
                 a=({},{}) spd={} f2a={} f2c={} f2e={} f30={} f36={} \
                 b3b={} d88={} mmax={} mana={} rand={:#06x} ph={} \
                 flags={:#x}",
                tick.t,
                e.class3f,
                e.model40,
                e.action45,
                e.b46,
                e.life,
                e.max_life,
                e.z,
                e.yaw,
                e.ayaw,
                e.apitch,
                e.speed,
                e.f2a,
                e.f2c,
                e.f2e,
                e.f30,
                e.f36,
                e.b3b,
                e.d88,
                e.mana_max,
                e.mana,
                e.rand,
                e.phase3e,
                e.flags
            );
            continue;
        }
        let Ok(st) = mgc_formats::mgcr::decode_retail_mc1(state) else {
            return 2;
        };
        let e = &st.ents[slot as usize];
        let d = prev_mana.map(|p| e.f140 - p).unwrap_or(0);
        prev_mana = Some(e.f140);
        println!(
            "t={} mana={} d={:+} f132={} life={} f63={} f63%4={} flags={:#x}",
            tick.t,
            e.f140,
            d,
            e.f132,
            e.act_life,
            e.f63,
            e.f63 % 4,
            e.flags
        );
    }
    0
}

/// `terrain-diff <rec.mgcr>…` — the record-0 STOCK-BAKE VALIDATOR
/// (docs/RECORDING-TERRAIN-V2.md "free instruments"): decode the
/// take's measured terrain base and diff it plane-by-plane against
/// the port's own generated level terrain. Agreement certifies the
/// generator chain; disagreement prints cell-level examples to dig
/// at. Exit 0 = every compared plane matched.
fn terrain_diff(path: &std::path::Path, args: &Args) -> i32 {
    match terrain_diff_inner(path, args) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            2
        }
    }
}

fn terrain_diff_inner(path: &std::path::Path, args: &Args) -> Result<bool, String> {
    let mut rec = Recording::open(path)?;
    let decl = rec
        .header
        .channels
        .terrain
        .clone()
        .ok_or("recording has no terrain channel (format-1 take?)")?;
    let game = rec.header.game.clone();
    let family = rec.header.family()?;
    let level = rec.header.level.ok_or("recording has no level number")?;
    let first = rec
        .next_tick()
        .ok_or("empty recording")?
        .map_err(|e| e.to_string())?;
    let base = first
        .terrain
        .as_ref()
        .and_then(|b| b.base.clone())
        .ok_or("first record carries no terrain base")?;
    let mut img = mgc_formats::mgcr::TerrainImage::new(&decl);
    img.apply(&mgc_formats::mgcr::TerrainBlock {
        base: Some(base),
        delta: None,
    })?;
    let pristine = match family {
        mgc_formats::mgcr::Family::Mc1 => verify::build_world(&args.baked, &game, level)?.1,
        mgc_formats::mgcr::Family::Mc2 => verify_mc2::build_world_mc2(&args.baked, level)?.1,
    };
    println!(
        "== terrain-diff {} (game {game}, level {level}, base @t={})",
        path.display(),
        first.t
    );
    // `--out <dir>`: dump both sides of every plane as raw
    // 256x256 byte images (`<plane>.retail` / `<plane>.port`) so an
    // offline clusterer can attribute the diffs region by region.
    if let Some(dir) = &args.out {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut dirty = 0usize;
    for name in &decl.planes {
        // `--baseline <dir>`: read the MEASURED planes from a cached
        // `--out` dump (`<dir>/<plane>.retail`) instead of the take's
        // own record-0 base. A take can be lost or re-recorded; a
        // cached dump of a graded take keeps the stock-bake validator
        // reproducible against the exact planes an attribution was
        // written from.
        let cached;
        let measured: &[u8] = if let Some(dir) = args.baseline.as_ref() {
            cached = std::fs::read(dir.join(format!("{name}.retail")))
                .map_err(|e| format!("{}/{name}.retail: {e}", dir.display()))?;
            &cached
        } else {
            img.plane(name).ok_or("declared plane missing")?
        };
        let baked: &[u8] = match name.as_str() {
            "type" => &pristine.tile_type,
            "height" => &pristine.height,
            "shading" => &pristine.shading,
            "angle" => &pristine.angle,
            "ceiling" => &pristine.ceiling,
            other => {
                println!("  {other}: not a port plane — skipped");
                continue;
            }
        };
        if let Some(dir) = &args.out {
            std::fs::write(dir.join(format!("{name}.retail")), measured)
                .map_err(|e| format!("{name}.retail: {e}"))?;
            std::fs::write(dir.join(format!("{name}.port")), baked)
                .map_err(|e| format!("{name}.port: {e}"))?;
        }
        if baked.len() != measured.len() {
            println!(
                "  {name}: size mismatch — port {} cells vs measured {}",
                baked.len(),
                measured.len()
            );
            dirty += 1;
            continue;
        }
        let diffs: Vec<usize> = (0..measured.len())
            .filter(|&i| measured[i] != baked[i])
            .collect();
        if diffs.is_empty() {
            println!("  {name}: MATCH ({} cells)", measured.len());
            continue;
        }
        dirty += diffs.len();
        println!(
            "  {name}: {} cell(s) differ ({:.2}%); examples:",
            diffs.len(),
            diffs.len() as f64 * 100.0 / measured.len() as f64
        );
        for &i in diffs.iter().take(args.max_diffs) {
            println!(
                "    ({:3},{:3}) retail {:3} vs port {:3}",
                i % 256,
                i / 256,
                measured[i],
                baked[i]
            );
        }
    }
    Ok(dirty == 0)
}

/// Re-decode every tick's raw struct image and compare against the
/// stored obs channel, value for value. Exit 0 = every tick matched.
fn check_decode(path: &std::path::Path, args: &Args) -> i32 {
    let mut rec = match Recording::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return 2;
        }
    };
    let family = match rec.header.family() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return 2;
        }
    };
    println!(
        "== {} (game {}, level {:?}, source {})",
        path.display(),
        rec.header.game,
        rec.header.level,
        rec.header.source
    );
    let (mut ticks, mut ok, mut bad, mut skipped) = (0u64, 0u64, 0u64, 0u64);
    // Terrain channel (format 2): every record's block must decode and
    // accumulate cleanly; report the channel's shape at the end.
    let mut timg = rec
        .header
        .channels
        .terrain
        .as_ref()
        .map(mgc_formats::mgcr::TerrainImage::new);
    let (mut t_deltas, mut t_cells) = (0u64, 0u64);
    while let Some(r) = rec.next_tick() {
        let tick = match r {
            Ok(t) => t,
            Err(e) => {
                eprintln!("  record error: {e}");
                return 2;
            }
        };
        ticks += 1;
        if let (Some(img), Some(block)) = (timg.as_mut(), &tick.terrain) {
            if let Some(d) = &block.delta {
                match mgc_formats::mgcr::decode_terrain_delta(
                    d,
                    img.decl().planes.len(),
                    img.decl().cells(),
                ) {
                    Ok(planes) => {
                        t_deltas += 1;
                        t_cells += planes.iter().map(|p| p.len() as u64).sum::<u64>();
                    }
                    Err(e) => {
                        eprintln!("  t={}: terrain: {e}", tick.t);
                        bad += 1;
                    }
                }
            }
            if let Err(e) = img.apply(block) {
                eprintln!("  t={}: terrain: {e}", tick.t);
                bad += 1;
            }
        }
        let (Some(state), Some(stored)) = (&tick.state, &tick.obs) else {
            skipped += 1;
            continue;
        };
        let decoded = match Obs::decode(family, state) {
            Ok(o) => o.to_value(),
            Err(e) => {
                eprintln!("  t={}: decode: {e}", tick.t);
                bad += 1;
                continue;
            }
        };
        let diffs = jsondiff::diff(stored, &decoded, args.max_diffs);
        if diffs.is_empty() {
            ok += 1;
        } else {
            bad += 1;
            println!("  t={}: {} mismatch path(s):", tick.t, diffs.len());
            for d in &diffs {
                println!("    {}: stored {} vs decoded {}", d.path, d.want, d.got);
            }
        }
        if let Some(limit) = args.limit {
            if ticks >= limit {
                break;
            }
        }
    }
    println!(
        "  {} ticks: {} ok, {} mismatched, {} without state+obs",
        ticks, ok, bad, skipped
    );
    if let Some(img) = &timg {
        println!(
            "  terrain: base {}, {} delta record(s), {} cell edit(s) total",
            if img.based() { "present" } else { "MISSING" },
            t_deltas,
            t_cells
        );
        if !img.based() {
            eprintln!("  terrain channel declared but no base record seen");
            return 1;
        }
    }
    if bad == 0 { 0 } else { 1 }
}
