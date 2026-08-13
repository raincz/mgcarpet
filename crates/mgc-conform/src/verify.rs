//! `verify-deltas` — the retail conformance mode (docs/RECORDING.md):
//! for every adjacent tick pair (N, N+1) in a retail recording, import
//! the raw state at N onto a pristine-built world, tick once with the
//! human pinned to the recorded pose, and diff the port's obs
//! projection against the recorded obs at N+1.
//!
//! Every pair is an independent fixture — divergence at one tick never
//! contaminates the next, so a 700-tick recording yields ~700
//! single-tick what-would-retail-do tests. The report aggregates
//! per-field mismatch counters across pairs; the global-LCG verdict is
//! reported separately (draw parity is the sharpest desync signal the
//! corpus offers).

use crate::Args;
use mgc_formats::mgcr::{EntObsMc1, ObsMc1, Recording, RetailEntMc1, RetailMc1, decode_retail_mc1};
use mgc_sim::engine::features::{FeatureAssets, Planes};
use mgc_sim::engine::world::conformance::PinnedMc1;
use mgc_sim::engine::world::{PlayerCommand, PlayerPose, World};
use mgc_sim::mc1::rivals::RivalConfig;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A pinned-pose choice: which record's carpet drives the tick.
#[derive(PartialEq)]
enum PinPose {
    /// The pre-tick pose (state@N) — the world sees the carpet where
    /// retail's tick STARTED.
    N,
    /// The post-tick pose (state@N+1) — the world sees the carpet
    /// where retail's tick ENDED (the app passes the already-moved
    /// pose the same way).
    N1,
}

pub fn verify_deltas(path: &std::path::Path, args: &Args) -> i32 {
    match run(path, args) {
        Ok(clean) => {
            if clean {
                0
            } else {
                1
            }
        }
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            2
        }
    }
}

fn run(path: &std::path::Path, args: &Args) -> Result<bool, String> {
    let pin_pose = match args.pin_pose.as_str() {
        "n" => PinPose::N,
        "n1" => PinPose::N1,
        other => return Err(format!("--pin-pose {other:?}: want n or n1")),
    };
    let mut rec = Recording::open(path)?;
    let game = rec.header.game.clone();
    if rec.header.family()? == mgc_formats::mgcr::Family::Mc2 {
        drop(rec);
        return crate::verify_mc2::run(path, args);
    }
    let level = rec.header.level.ok_or("recording has no level number")?;
    println!(
        "== verify-deltas {} (game {game}, level {level}, pin-pose {})",
        path.display(),
        args.pin_pose
    );

    let (mut world, pristine) = build_world(&args.baked, &game, level)?;

    let mut csv: Option<std::io::BufWriter<std::fs::File>> = match &args.csv {
        Some(p) => {
            let f = std::fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let mut w = std::io::BufWriter::new(f);
            use std::io::Write as _;
            writeln!(
                w,
                "t\tkind\tslot\tclass\tmodel\tfield\twant\tgot\tx\ty\tz\trule"
            )
            .map_err(|e| e.to_string())?;
            Some(w)
        }
        None => None,
    };
    let roster = load_roster(args)?;
    let take = take_stem(path);

    // Stream pairs.
    let mut prev: Option<(u64, RetailMc1, PlayerCommand)> = None;
    let mut prev_cmd = PlayerCommand::default();
    // --input-delay ring: cmd fed at pair N is the one sampled at
    // N - input_delay.
    let mut cmd_ring: std::collections::VecDeque<PlayerCommand> =
        std::iter::repeat_n(PlayerCommand::default(), args.input_delay as usize + 1).collect();
    let mut stats = Stats::default();
    let mut pose_chan = crate::pose_lane::PoseLane::default();
    let mut printed_import = false;
    // The measured-terrain accumulator (format-2 channel): a pair
    // (pt → t) must run on terrain AT pt, so each record's block is
    // held pending and applied at the TOP of the NEXT iteration —
    // during pair processing the image is exactly the pair's start
    // state. --no-terrain A/Bs the channel off (pristine planes only).
    let mut timg = (!args.no_terrain)
        .then(|| {
            rec.header
                .channels
                .terrain
                .as_ref()
                .map(mgc_formats::mgcr::TerrainImage::new)
        })
        .flatten();
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
        let obs: ObsMc1 = match &tick.obs {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("obs: {e}"))?,
            None => return Err(format!("t={}: no obs channel", tick.t)),
        };
        // The recorded raw input (held mouse buttons) reconstructs the
        // human's casts. ±1-tick attribution caveat per RECORDING.md;
        // the port's edge trigger sees held-across-consensus presses.
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
            // Equips + demolish ride the shared recovery laws (the
            // replay verifier's lane): a recorded hand change across
            // the pair replays as the equip command, the dw==48 word
            // as the demolish. Fire keeps the raw input channel (the
            // ±1-tick caveat above) — only the lanes verify never fed
            // are taken from the recovery.
            let pcmd = {
                let rec = mgc_formats::recover::recover_pair_mc1(&pst, &st, tick.input.as_ref());
                PlayerCommand {
                    equip_left: rec.equip_left.map(mgc_sim::mc1::spells::SpellId),
                    equip_right: rec.equip_right.map(mgc_sim::mc1::spells::SpellId),
                    demolish: rec.demolish,
                    ..pcmd
                }
            };
            if args.start.is_some_and(|s| pt < s) {
                // Before the triage window — keep the pairing chain
                // and the input ring warm, execute nothing.
            } else if tick.t == pt + 1 {
                if args.start.is_some() {
                    eprintln!("pair {pt}");
                }
                stats.pairs += 1;
                // Capture-tear gate: a consensus snapshot can land
                // MID-entity-loop (DOSBox frozen inside the tick), in
                // which case the +63 clocks split into contiguous
                // stepped/unstepped slot bands and the global LCG can
                // show 0 draws. Such a state is not an inter-tick
                // closure — the pair is not fixture-grade.
                if !capture_clean(&pst, &obs) {
                    stats.torn += 1;
                } else {
                    world.restore_planes(&pristine);
                    if let Some((h, ty, ceil)) = measured_planes(&timg) {
                        world
                            .install_measured_terrain(h, ty, ceil)
                            .map_err(|e| format!("t={pt}: terrain: {e}"))?;
                    }
                    let report = world
                        .retail_import_mc1(&pst)
                        .map_err(|e| format!("t={pt}: import: {e}"))?;
                    world.set_prev_fire(prev_cmd.fire_left, prev_cmd.fire_right);
                    if args.start.is_some()
                        && let Some((got, want)) = report.stack_fallback
                    {
                        eprintln!("  free-stack fallback: live {got} != scan {want}");
                    }
                    if !printed_import {
                        printed_import = true;
                        println!(
                            "   import: {} active entities, human slot {}, behavior base {:#x}, {} bad rows, terrain {}",
                            report.active,
                            report.human_slot,
                            report.behavior_base,
                            report.bad_rows,
                            if measured_planes(&timg).is_some() {
                                "MEASURED"
                            } else {
                                "pristine"
                            }
                        );
                    }
                    let pose_src = match pin_pose {
                        PinPose::N => &pst.ents[report.human_slot as usize],
                        PinPose::N1 => &st.ents[report.human_slot as usize],
                    };
                    let pose = carpet_pose(pose_src);
                    world.tick(pose, pcmd);
                    // The POSE CHANNEL: shadow-step the faithful
                    // mover and diff the human's own motion column at
                    // N+1. Its terrain probes run on the MEASURED
                    // terrain@N+1: retail's terraform writers sit at
                    // low pool slots and run BEFORE the carpet's, so
                    // the carpet's ground probe saw the pair's END
                    // terrain (measured on mc1l0 — every eff_pitch/z
                    // residue row sat on a live terraform window, and
                    // the port-evolved planes are no substitute where
                    // the port's own terraform diverges). Applying the
                    // pending block early is safe: deltas carry
                    // absolute cell values, so the loop-top re-apply
                    // is idempotent. The flight seed still comes from
                    // the recorded closure at N.
                    if !args.no_pose_lane {
                        if let (Some(img), Some(block)) = (timg.as_mut(), pending_terrain.as_ref())
                        {
                            img.apply(block)
                                .map_err(|e| format!("t={pt}: pose terrain: {e}"))?;
                        }
                        if let Some((h, ty, ceil)) = measured_planes(&timg) {
                            world
                                .install_measured_terrain(h, ty, ceil)
                                .map_err(|e| format!("t={pt}: pose terrain: {e}"))?;
                        }
                        pose_chan
                            .run_pair_mc1(
                                &world,
                                &pst,
                                &st,
                                report.human_slot,
                                pt,
                                csv.as_mut().map(|w| w as &mut dyn std::io::Write),
                            )
                            .map_err(|e| format!("t={pt}: pose csv: {e}"))?;
                    }
                    let pin = PinnedMc1 {
                        slot: report.human_slot,
                        local: pst.local_player,
                        player_count: pst.player_count,
                        pose,
                    };
                    let port = world.obs_project_mc1(&pin);
                    stats.absorb_rng(pst.rand, obs.rng, port.rng);
                    stats.absorb_phase(&pst, &obs, &port, report.human_slot);
                    let mut pd = compare(&obs, &port, report.human_slot);
                    append_hand_diffs(&mut pd, &st, &port, pst.local_player as usize);
                    append_charge_diffs(&mut pd, &st, &world, report.human_slot);
                    let pd = pd;
                    let mut tags = (roster.is_some() || !args.no_pose_alt).then(|| {
                        let rmap: BTreeMap<u16, &EntObsMc1> =
                            obs.entities.iter().map(|e| (e.slot, e)).collect();
                        let pmap: BTreeMap<u16, &EntObsMc1> =
                            port.entities.iter().map(|e| (e.slot, e)).collect();
                        let ctx = |slot: u16| {
                            rmap.get(&slot)
                                .or_else(|| pmap.get(&slot))
                                .map(|e| (e.class, e.model, e.x, e.y))
                        };
                        let mut tg = classify_pair(roster.as_ref(), &take, pt, &pd, &ctx);
                        // SLOT-DESYNC pass (computed rule, roster.rs):
                        // balanced same-(class,model) missing/extra
                        // residue = free-list slot-order desync. Runs
                        // BEFORE the pose-phase pass by construction —
                        // see RuleTags::slot_desync for why the order
                        // is load-bearing.
                        if !args.no_slot_desync {
                            let pos = |slot: u16| ctx(slot).map(|(_, _, x, y)| (x, y));
                            tg.slot_desync(&pd.missing, &pd.extra, &pos);
                        }
                        tg
                    });
                    // The pose-phase pass: re-run the pair under the
                    // other pose sample; rows clean there are capture
                    // (retail's mixed within-tick pose), not leads.
                    if !args.no_pose_alt
                        && !pd.clean()
                        && let Some(tg) = tags.as_mut()
                    {
                        let (alt, _, _) = exec_pair(
                            &mut world,
                            &pristine,
                            measured_planes(&timg),
                            &pst,
                            &st,
                            &obs,
                            pcmd,
                            prev_cmd,
                            !matches!(pin_pose, PinPose::N1),
                        )
                        .map_err(|e| format!("t={pt}: pose-alt: {e}"))?;
                        pose_reclassify(tg, &pd, &alt);
                    }
                    let tags = tags;
                    if let Some(w) = csv.as_mut() {
                        emit_csv(w, pt, &pd, &obs, &port, roster.as_ref(), tags.as_ref())
                            .map_err(|e| e.to_string())?;
                    }
                    let dump = args.dump == Some(pt)
                        || (args.dump_first && !pd.clean() && stats.first_diff.is_none());
                    stats.absorb(pt, pd, tags.as_ref(), roster.as_ref(), args);
                    if dump {
                        // Re-diff for the full print (absorb consumed it).
                        let pd = compare(&obs, &port, report.human_slot);
                        print!("{}", pd.render(pt, usize::MAX));
                        if args.dump_port {
                            for e in &port.entities {
                                println!(
                                    "    port slot {}: cm=({},{}) flags={:#x} life={}/{} \
                                     pos=({:.2},{:.2},{}) mana={} own_ptr={:#x}",
                                    e.slot,
                                    e.class,
                                    e.model,
                                    e.flags,
                                    e.life,
                                    e.max_life,
                                    e.x,
                                    e.y,
                                    e.z,
                                    e.mana,
                                    e.owner_ptr
                                );
                            }
                        }
                    }
                }
            } else {
                stats.gaps += 1;
            }
            // THE PAIR'S OWN COMMAND IS THE NEXT PAIR'S PREDECESSOR.
            // This used to read `if let Some((_, _, c)) = &prev` AFTER
            // `prev.take()` had already emptied it — a dead lane, so
            // `prev_cmd` never left its seed and the port's cast EDGE
            // (`cmd.fire && !prev_fire`, world.rs) degenerated to the
            // raw HELD level for the whole run: a held button re-cast
            // every time the imported cast timer read 0. Both engines
            // gate the click-only spell families on a CONSUMED press
            // latch, never on the held level (MC2:
            // `HandleMouseButtons_18F80` PI:2043-49 + the frame-tail
            // latch clear PI:1049-52). mc1l0 0+4000: 450 → 501
            // conforming, unexplained field 7,440 → 6,524, extra rows
            // 1,029 → 401. `MGC_NO_FIRE_EDGE=1` restores the old
            // behaviour for A/B.
            if std::env::var_os("MGC_NO_FIRE_EDGE").is_none() {
                prev_cmd = pcmd;
            }
        }
        prev = Some((tick.t, st, cmd));
        if let Some(limit) = args.limit {
            if stats.pairs >= limit {
                break;
            }
        }
    }
    print!("{}", stats.render(args, roster.as_ref()));
    print!("{}", pose_chan.render());
    Ok(stats.clean_pairs == stats.pairs)
}

/// The default committed roster path (docs/CONFORMANCE.md): loaded
/// unless `--no-roster`; a missing file is not an error.
pub(crate) fn load_roster(args: &Args) -> Result<Option<crate::roster::Roster>, String> {
    if args.no_roster {
        return Ok(None);
    }
    crate::roster::Roster::load(std::path::Path::new("conformance/known-deviations.json"))
}

/// The take name rules scope on: the recording's file stem.
pub(crate) fn take_stem(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Tag every row of a pair's diff against the roster. `ctx` resolves
/// a slot to (class, model, x, y) with `emit_csv`'s convention: the
/// retail entity's, falling back to the port's for extra-in-port
/// slots. Shared by the MC1 and MC2 arms. With no roster every row
/// starts `Unexplained` (the pose-phase pass may still claim it).
pub(crate) fn classify_pair(
    roster: Option<&crate::roster::Roster>,
    take: &str,
    t: u64,
    pd: &PairDiff,
    ctx: &dyn Fn(u16) -> Option<(u8, u8, f64, f64)>,
) -> crate::roster::RuleTags {
    use crate::roster::{RowCtx, RowKind, RuleTags, Tag};
    let pos = |slot: u16| ctx(slot).map(|(_, _, x, y)| (x, y));
    let tag = |row: &RowCtx| -> Tag {
        roster
            .and_then(|r| r.classify(take, t, row))
            .map_or(Tag::Unexplained, Tag::Rule)
    };
    let mut tags = RuleTags::default();
    for (slot, c, m) in &pd.missing {
        tags.missing.push(tag(&RowCtx {
            kind: RowKind::Missing,
            slot: Some(*slot),
            class: *c,
            model: *m,
            field: None,
            pos: pos(*slot),
        }));
    }
    for (slot, c, m) in &pd.extra {
        tags.extra.push(tag(&RowCtx {
            kind: RowKind::Extra,
            slot: Some(*slot),
            class: *c,
            model: *m,
            field: None,
            pos: pos(*slot),
        }));
    }
    for d in &pd.fields {
        let (c, m, p) = match d.slot.and_then(&ctx) {
            Some((c, m, x, y)) => (c, m, Some((x, y))),
            None => (0, 0, None),
        };
        tags.fields.push(tag(&RowCtx {
            kind: RowKind::Field,
            slot: d.slot,
            class: c,
            model: m,
            field: Some(d.field),
            pos: p,
        }));
    }
    tags
}

/// Reclassify still-unexplained rows that are CLEAN in the port run
/// driven by the other `--pin-pose` sample: retail's within-tick pose
/// is two-valued (the carpet moves at its pool slot mid-pass), so a
/// row that conforms under either sample is pose-phase capture, not a
/// lead. Row identity: missing/extra by (slot, class, model), fields
/// by (slot, field). Roster rules take precedence.
pub(crate) fn pose_reclassify(tags: &mut crate::roster::RuleTags, pd: &PairDiff, alt: &PairDiff) {
    use crate::roster::Tag;
    use std::collections::BTreeSet;
    let alt_missing: BTreeSet<_> = alt.missing.iter().collect();
    let alt_extra: BTreeSet<_> = alt.extra.iter().collect();
    let alt_fields: BTreeSet<_> = alt.fields.iter().map(|d| (d.slot, d.field)).collect();
    for (i, row) in pd.missing.iter().enumerate() {
        if tags.missing[i] == Tag::Unexplained && !alt_missing.contains(row) {
            tags.missing[i] = Tag::PosePhase;
        }
    }
    for (i, row) in pd.extra.iter().enumerate() {
        if tags.extra[i] == Tag::Unexplained && !alt_extra.contains(row) {
            tags.extra[i] = Tag::PosePhase;
        }
    }
    for (i, d) in pd.fields.iter().enumerate() {
        if tags.fields[i] == Tag::Unexplained && !alt_fields.contains(&(d.slot, d.field)) {
            tags.fields[i] = Tag::PosePhase;
        }
    }
}

/// One TSV row per diff event: field mismatches carry the retail
/// entity's (class, model, x, y, z) as spatial context (falling back
/// to the port's for extra-in-port slots) so offline triage can
/// cluster divergence geographically (e.g. crater sites). The last
/// column is the matched roster rule id (empty = unexplained).
fn emit_csv(
    w: &mut impl std::io::Write,
    t: u64,
    pd: &PairDiff,
    retail: &ObsMc1,
    port: &ObsMc1,
    roster: Option<&crate::roster::Roster>,
    tags: Option<&crate::roster::RuleTags>,
) -> std::io::Result<()> {
    let rmap: BTreeMap<u16, &EntObsMc1> = retail.entities.iter().map(|e| (e.slot, e)).collect();
    let pmap: BTreeMap<u16, &EntObsMc1> = port.entities.iter().map(|e| (e.slot, e)).collect();
    let ctx = |slot: u16| -> (String, String, String, String, String) {
        match rmap.get(&slot).or_else(|| pmap.get(&slot)) {
            Some(e) => (
                e.class.to_string(),
                e.model.to_string(),
                format!("{}", e.x),
                format!("{}", e.y),
                e.z.to_string(),
            ),
            None => Default::default(),
        }
    };
    let rule_id =
        |lane: fn(&crate::roster::RuleTags) -> &Vec<crate::roster::Tag>, i: usize| -> &str {
            match tags {
                Some(tg) => match lane(tg)[i] {
                    crate::roster::Tag::Rule(k) => roster.map_or("", |r| r.rules[k].id.as_str()),
                    crate::roster::Tag::PosePhase => "pose-phase",
                    crate::roster::Tag::SlotDesync => "slot-desync",
                    crate::roster::Tag::Unexplained => "",
                },
                None => "",
            }
        };
    for (i, (slot, c, m)) in pd.missing.iter().enumerate() {
        let (_, _, x, y, z) = ctx(*slot);
        let rid = rule_id(|t| &t.missing, i);
        writeln!(
            w,
            "{t}\tmissing\t{slot}\t{c}\t{m}\t\t\t\t{x}\t{y}\t{z}\t{rid}"
        )?;
    }
    for (i, (slot, c, m)) in pd.extra.iter().enumerate() {
        let (_, _, x, y, z) = ctx(*slot);
        let rid = rule_id(|t| &t.extra, i);
        writeln!(
            w,
            "{t}\textra\t{slot}\t{c}\t{m}\t\t\t\t{x}\t{y}\t{z}\t{rid}"
        )?;
    }
    for (i, d) in pd.fields.iter().enumerate() {
        let rid = rule_id(|t| &t.fields, i);
        match d.slot {
            Some(slot) => {
                let (c, m, x, y, z) = ctx(slot);
                writeln!(
                    w,
                    "{t}\tfield\t{slot}\t{c}\t{m}\t{}\t{}\t{}\t{x}\t{y}\t{z}\t{rid}",
                    d.field, d.want, d.got
                )?;
            }
            None => writeln!(
                w,
                "{t}\tfield\t\t\t\t{}\t{}\t{}\t\t\t\t{rid}",
                d.field, d.want, d.got
            )?,
        }
    }
    Ok(())
}

/// Is the pair fixture-grade? A clean inter-tick pair advances every
/// persisted entity's +63 clock by exactly 1 (retail's dispatch steps
/// every registered row per pass — the data10 gate is static and all
/// live states carry 1) and draws the global LCG exactly once. A
/// mid-pass snapshot splits the clocks into contiguous 0/2 bands and
/// can freeze the LCG. Only steps of 0 or 2 count as tear suspects —
/// ambient spawn churn (slot re-use overwrites +63 with the spawn
/// ordinal; constant on HW's weather families) lands on arbitrary
/// values and must not starve the classifier (mirrors the recorder's
/// `pair_clean`).
/// Hands compare in INTERNAL-spell space: the recorded raw value
/// indexes the acquisition list (resolved through state@N+1); the
/// port emits spell ids.
pub(crate) fn append_hand_diffs(pd: &mut PairDiff, st: &RetailMc1, port: &ObsMc1, local: usize) {
    let pw = &port.wizards[local];
    for (side, raw, got) in [
        (
            "wizard0.hand_left",
            st.wizards[local].hand_left,
            pw.hand_left,
        ),
        (
            "wizard0.hand_right",
            st.wizards[local].hand_right,
            pw.hand_right,
        ),
    ] {
        let want = st.hand_spell(local, raw).map(u16::from);
        if want != got {
            pd.fields.push(FieldDiff {
                slot: None,
                field: side,
                want: format!("{want:?}"),
                got: format!("{got:?}"),
            });
        }
    }
}

/// The RAW-lane rows (player ask 2026-08-13): per-entity `f26` — the
/// castle LEVEL / establishment lane, until now invisible except via
/// mana_max — and the per-wizard +326 cast-charge meters. Neither
/// ever entered the recorder's obs schema (which is check-decode-
/// locked against the corpus), so they ride the raw state channel
/// like the hands above. Class-12 rows compare retail +48: the port
/// keeps a manifestation's burst counter in f26 (retail's +26 is the
/// spell level there — unmodeled, see `import_ent`).
pub(crate) fn append_charge_diffs(
    pd: &mut PairDiff,
    st: &RetailMc1,
    world: &World,
    human_slot: u16,
) {
    let (f26, charge) = world.charge_lane_mc1();
    let port: BTreeMap<u16, i16> = f26.into_iter().collect();
    for (slot, e) in st.ents.iter().enumerate().skip(1) {
        let slot = slot as u16;
        if e.class64 == 0 || slot == human_slot {
            continue;
        }
        // Slots absent on either side are the missing/extra story.
        let Some(&got) = port.get(&slot) else {
            continue;
        };
        let want = if e.class64 == 12 { e.f48 as i16 } else { e.f26 };
        if want != got {
            pd.fields.push(FieldDiff {
                slot: Some(slot),
                field: "f26",
                want: want.to_string(),
                got: got.to_string(),
            });
        }
    }
    for (i, w) in st.wizards.iter().enumerate().take(8) {
        if w.charge != charge[i] {
            pd.fields.push(FieldDiff {
                slot: None,
                field: if i == 0 {
                    "wizard0.charge"
                } else {
                    "rival.charge"
                },
                want: w.charge.to_string(),
                got: charge[i].to_string(),
            });
        }
    }
}

/// The measured height/type planes — plus the cave CEILING when the
/// take declares it — for pair execution, when the recording's
/// format-2 terrain channel has anchored the accumulator (a
/// mid-stream start without the base yields None — relative-only
/// planes must never be installed as absolute terrain).
pub(crate) type MeasuredPlanes<'a> = (&'a [u8], &'a [u8], Option<&'a [u8]>);

pub(crate) fn measured_planes(
    timg: &Option<mgc_formats::mgcr::TerrainImage>,
) -> Option<MeasuredPlanes<'_>> {
    timg.as_ref().and_then(|img| img.measured())
}

/// One fixture-grade pair, executed on a prepared world: restore
/// pristine planes (overlaying the MEASURED height/type images when
/// the recording carries the terrain channel), import state@N, tick
/// with the pinned pose, and diff the port projection against the
/// recorded obs@N+1. The single implementation behind both
/// `verify-deltas` and the fixture suite.
#[allow(clippy::too_many_arguments)]
pub(crate) fn exec_pair(
    world: &mut World,
    pristine: &Planes,
    measured: Option<crate::verify::MeasuredPlanes<'_>>,
    pst: &RetailMc1,
    st: &RetailMc1,
    obs: &ObsMc1,
    cmd: PlayerCommand,
    prev_cmd: PlayerCommand,
    pin_n1: bool,
) -> Result<(PairDiff, ObsMc1, u16), String> {
    world.restore_planes(pristine);
    if let Some((h, ty, ceil)) = measured {
        world
            .install_measured_terrain(h, ty, ceil)
            .map_err(|e| format!("terrain: {e}"))?;
    }
    let report = world
        .retail_import_mc1(pst)
        .map_err(|e| format!("import: {e}"))?;
    world.set_prev_fire(prev_cmd.fire_left, prev_cmd.fire_right);
    let pose_src = if pin_n1 {
        &st.ents[report.human_slot as usize]
    } else {
        &pst.ents[report.human_slot as usize]
    };
    let pose = carpet_pose(pose_src);
    world.tick(pose, cmd);
    let pin = PinnedMc1 {
        slot: report.human_slot,
        local: pst.local_player,
        player_count: pst.player_count,
        pose,
    };
    let port = world.obs_project_mc1(&pin);
    let mut pd = compare(obs, &port, report.human_slot);
    append_hand_diffs(&mut pd, st, &port, pst.local_player as usize);
    append_charge_diffs(&mut pd, st, world, report.human_slot);
    Ok((pd, port, report.human_slot))
}

// The MC1 capture-grade law moved to the shared recovery home so the
// app's `--replay` grades boundaries by the same rule.
pub(crate) use mgc_formats::recover::capture_clean_mc1 as capture_clean;

/// The recorded carpet's raw fields as the pinned pose (heading @30,
/// pitch @32, speed @126 — engine units throughout).
pub(crate) fn carpet_pose(e: &RetailEntMc1) -> PlayerPose {
    PlayerPose {
        x: e.x,
        y: e.y,
        z: e.z,
        heading: e.f30,
        pitch: e.f32,
        speed: e.f126,
    }
}

/// The golden-test world recipe (tests/state_hash.rs `build_world`),
/// parameterized by game + level. Returns the world and a pristine
/// copy of the planes for the per-pair terrain reset.
pub(crate) fn build_world(
    baked: &std::path::Path,
    game: &str,
    level: u32,
) -> Result<(World, Planes), String> {
    let lp = baked.join(game).join(format!("level-{level:03}.mgcl"));
    let file = std::fs::File::open(&lp).map_err(|e| format!("{}: {e}", lp.display()))?;
    let pkg: mgc_formats::LevelPackage =
        mgc_formats::mgcl::read(file).map_err(|e| format!("{}: {e}", lp.display()))?;
    if let Some(ov) = &pkg.meta.overlay {
        return Err(format!(
            "{}: MODDED level (overlay {ov}) — conformance runs against pristine bakes \
             only; delete baked/ and rebake without gamedata/overlay/ (docs/MODDING.md)",
            lp.display()
        ));
    }
    // The HW fall-through trap: a bare World::new here replayed HW
    // takes under BASE-MC1 law (SPELLS not SPELLS_HW, no m16 homing
    // acquire, base napalm fork) — the game string must select the
    // verb column, not just the asset variant.
    let (variant, game_id) = if game == "mc1hw" {
        ("mc1-arctic", mgc_sim::ids::GameId::Mc1Hw)
    } else {
        ("mc1-temperate", mgc_sim::ids::GameId::Mc1)
    };
    let bundle = mgc_formats::bundle::Bundle::load(&baked.join("assets").join(variant))
        .map_err(|e| format!("bundle {variant}: {e}"))?;
    let terrain = pkg.terrain.as_ref().ok_or("package has no terrain")?;
    let planes = Planes {
        height: terrain.height.clone(),
        tile_type: terrain.tile_type.clone(),
        shading: terrain.shading.clone().ok_or("no shading plane")?,
        angle: terrain.angle.clone().ok_or("no angle plane")?,
        ceiling: Vec::new(),
    };
    let mut assets = FeatureAssets::parse(
        bundle.search.as_ref().ok_or("bundle: no search data")?,
        bundle.build_tab.as_ref().ok_or("bundle: no build tab")?,
        bundle.build_dat.as_ref().ok_or("bundle: no build dat")?,
    )?;
    if let Some(prm) = bundle.bldgprm.as_deref() {
        assets = assets.with_bldgprm(prm);
    }
    if let Some(sp) = bundle.spells.as_deref() {
        assets = assets.with_spells(sp)?;
    }
    let seed = pkg.gen_params.as_ref().map_or(0, |g| g.seed);
    let mut w = World::new_for_game(planes, &pkg.things.things, seed, assets, game_id);
    if let Some(f) = pkg.gen_params.as_ref().and_then(|g| g.footer) {
        w.set_win_pct(f[0]);
    }
    let (wizards, player_count) = rival_configs(pkg.wizards.as_ref());
    w.set_wizards(&wizards, player_count);
    // Pristine = POST level-init: the load-time feature pass (crater/
    // flatten/wall edits) is part of the level, not runtime state.
    let pristine = w.planes_clone();
    Ok((w, pristine))
}

/// wizards.json → per-slot rival configs (the app's resolver, same
/// duplication the golden tests carry).
fn rival_configs(wizards: Option<&mgc_formats::Wizards>) -> ([Option<RivalConfig>; 8], u16) {
    let mut out: [Option<RivalConfig>; 8] = Default::default();
    let Some(w) = wizards else { return (out, 1) };
    let count = w.player_count.unwrap_or(1).min(8);
    for (slot, cfg) in w.wizards.iter().enumerate().take(8).skip(1) {
        let (Some(acc), Some(tempo), Some(allowed_mask)) =
            (cfg.accuracy, cfg.tempo, cfg.allowed_spells.as_ref())
        else {
            continue;
        };
        let mut book = [false; 24];
        let mut allowed = [false; 24];
        for s in 0..24 {
            let a = allowed_mask.get(s).copied().unwrap_or(0) != 0;
            allowed[s] = a;
            book[s] = a && cfg.starting_spells.get(s).copied().unwrap_or(0) != 0;
        }
        out[slot] = Some(RivalConfig {
            aggression: cfg.aggression.clamp(0, 255) as u8,
            accuracy: acc.clamp(0, 255) as u8,
            tempo: tempo.clamp(0, 255) as u8,
            castle_level: cfg.castle_level.unwrap_or(0),
            book,
            allowed,
        });
    }
    (out, count)
}

// ------------------------------------------------------------- comparison

/// One field mismatch on one entity (or a top-level scalar).
pub(crate) struct FieldDiff {
    pub(crate) slot: Option<u16>,
    pub(crate) field: &'static str,
    pub(crate) want: String,
    pub(crate) got: String,
}

#[derive(Default)]
pub(crate) struct PairDiff {
    pub(crate) rng_want: u32,
    pub(crate) rng_got: u32,
    /// Slots retail has that the port lacks (slot, class, model).
    pub(crate) missing: Vec<(u16, u8, u8)>,
    /// Slots the port has that retail lacks.
    pub(crate) extra: Vec<(u16, u8, u8)>,
    pub(crate) fields: Vec<FieldDiff>,
}

impl PairDiff {
    pub(crate) fn clean(&self) -> bool {
        self.rng_want == self.rng_got
            && self.missing.is_empty()
            && self.extra.is_empty()
            && self.fields.is_empty()
    }

    pub(crate) fn render(&self, t: u64, cap: usize) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "  pair {t}→{}:", t + 1);
        if self.rng_want != self.rng_got {
            let _ = writeln!(
                s,
                "    rng: retail {:#010x} port {:#010x}",
                self.rng_want, self.rng_got
            );
        }
        for (slot, c, m) in &self.missing {
            let _ = writeln!(s, "    missing in port: slot {slot} (class {c} model {m})");
        }
        for (slot, c, m) in &self.extra {
            let _ = writeln!(s, "    extra in port:   slot {slot} (class {c} model {m})");
        }
        for d in self.fields.iter().take(cap) {
            match d.slot {
                Some(slot) => {
                    let _ = writeln!(
                        s,
                        "    slot {slot} {}: retail {} port {}",
                        d.field, d.want, d.got
                    );
                }
                None => {
                    let _ = writeln!(s, "    {}: retail {} port {}", d.field, d.want, d.got);
                }
            }
        }
        if self.fields.len() > cap {
            let _ = writeln!(s, "    … {} more field diffs", self.fields.len() - cap);
        }
        s
    }
}

macro_rules! cmp_field {
    ($out:expr, $slot:expr, $name:literal, $want:expr, $got:expr) => {
        if $want != $got {
            $out.fields.push(FieldDiff {
                slot: $slot,
                field: $name,
                want: format!("{:?}", $want),
                got: format!("{:?}", $got),
            });
        }
    };
}

/// Field-aware obs comparison. Policy:
/// - `owner_ptr` is never compared (guest pointer, port emits 0).
/// - the pinned human slot compares presence + life/mana only (its
///   pose fields are runner INPUTS, not predictions).
/// - wizard `flight` is skipped (input-reconstruction domain).
pub(crate) fn compare(retail: &ObsMc1, port: &ObsMc1, human_slot: u16) -> PairDiff {
    let mut out = PairDiff {
        rng_want: retail.rng,
        rng_got: port.rng,
        ..Default::default()
    };
    let rmap: BTreeMap<u16, &EntObsMc1> = retail.entities.iter().map(|e| (e.slot, e)).collect();
    let pmap: BTreeMap<u16, &EntObsMc1> = port.entities.iter().map(|e| (e.slot, e)).collect();
    for (slot, re) in &rmap {
        let Some(pe) = pmap.get(slot) else {
            out.missing.push((*slot, re.class, re.model));
            continue;
        };
        let s = Some(*slot);
        if *slot == human_slot {
            cmp_field!(out, s, "life", re.life, pe.life);
            cmp_field!(out, s, "mana", re.mana, pe.mana);
            cmp_field!(out, s, "mana_max", re.mana_max, pe.mana_max);
            continue;
        }
        cmp_field!(out, s, "class", re.class, pe.class);
        cmp_field!(out, s, "model", re.model, pe.model);
        cmp_field!(out, s, "sclass", re.sclass, pe.sclass);
        cmp_field!(out, s, "smodel", re.smodel, pe.smodel);
        cmp_field!(out, s, "flags", re.flags, pe.flags);
        cmp_field!(out, s, "id", re.id, pe.id);
        cmp_field!(out, s, "life", re.life, pe.life);
        cmp_field!(out, s, "max_life", re.max_life, pe.max_life);
        cmp_field!(out, s, "x", re.x, pe.x);
        cmp_field!(out, s, "y", re.y, pe.y);
        cmp_field!(out, s, "z", re.z, pe.z);
        cmp_field!(out, s, "heading", re.heading, pe.heading);
        cmp_field!(out, s, "pitch", re.pitch, pe.pitch);
        cmp_field!(out, s, "target_yaw", re.target_yaw, pe.target_yaw);
        cmp_field!(out, s, "speed", re.speed, pe.speed);
        cmp_field!(out, s, "mana", re.mana, pe.mana);
        cmp_field!(out, s, "mana_max", re.mana_max, pe.mana_max);
        cmp_field!(out, s, "chase", re.chase, pe.chase);
        // tick_byte is analyzed as the phase-clock channel by the
        // runner (retail steps +63 only through rows with a live
        // handler; see the presence table in the report).
        cmp_field!(out, s, "rand", re.rand, pe.rand);
    }
    for (slot, pe) in &pmap {
        if !rmap.contains_key(slot) {
            out.extra.push((*slot, pe.class, pe.model));
        }
    }
    for (rw, pw) in retail.wizards.iter().zip(&port.wizards) {
        let s = None;
        match rw.index {
            // Hands are compared semantically by the runner (the raw
            // stored value is an acquisition-list index).
            0 => {
                cmp_field!(out, s, "wizard0.play_index", rw.play_index, pw.play_index);
                cmp_field!(out, s, "wizard0.castle", rw.castle, pw.castle);
            }
            _ => {
                cmp_field!(out, s, "rival.play_index", rw.play_index, pw.play_index);
                cmp_field!(out, s, "rival.castle", rw.castle, pw.castle);
            }
        }
    }
    if let (Some(rp), Some(pp)) = (&retail.player, &port.player) {
        let s = None;
        cmp_field!(out, s, "player.life", rp.life, pp.life);
        cmp_field!(out, s, "player.mana", rp.mana, pp.mana);
        cmp_field!(out, s, "player.mana_max", rp.mana_max, pp.mana_max);
        cmp_field!(out, s, "player.castle", rp.castle, pp.castle);
    }
    out
}

// ------------------------------------------------------------ aggregation

#[derive(Default)]
pub(crate) struct FieldStat {
    count: u64,
    pairs: u64,
    example: Option<(u64, Option<u16>, String, String)>,
}

#[derive(Default)]
pub(crate) struct Stats {
    pub(crate) pairs: u64,
    pub(crate) gaps: u64,
    /// Pairs rejected by the capture-tear gate (mid-pass snapshots).
    pub(crate) torn: u64,
    pub(crate) clean_pairs: u64,
    rng_only_pairs: u64,
    rng_mismatch: u64,
    missing: u64,
    extra: u64,
    /// Entity-set events by (class, model) → (missing, extra) counts,
    /// with a first-seen example tick+slot.
    set_rows: BTreeMap<(u8, u8), (u64, u64, u64, u16)>,
    fields: BTreeMap<&'static str, FieldStat>,
    pub(crate) first_diff: Option<u64>,
    first_render: String,
    /// Global-LCG draws per tick: (retail steps, port steps) → pairs.
    /// Steps are recovered by walking `9377x+9439` from rand@N to
    /// rand@N+1 (17 = "more than 16").
    rng_hist: BTreeMap<(u8, u8), u64>,
    /// Roster classification: rule index → (rows, pairs touched).
    rule_rows: BTreeMap<usize, (u64, u64)>,
    /// Pairs whose every diff row matched a rule (rng clean): the
    /// "conforming net of known deviations" tier.
    explained_pairs: u64,
    /// Unexplained residue (rows no rule matched).
    unknown_fields: u64,
    unknown_missing: u64,
    unknown_extra: u64,
    /// Rows clean under the alternate `--pin-pose` run (pose-phase
    /// capture) and the pairs touched by any such row.
    pose_fields: u64,
    pose_missing: u64,
    pose_extra: u64,
    pose_pairs: u64,
    /// Missing/extra rows claimed by the computed slot-desync rule
    /// (balanced same-(class,model) atoms = free-list slot-order
    /// desync) and the pairs touched.
    slotdesync_missing: u64,
    slotdesync_extra: u64,
    slotdesync_pairs: u64,
    /// Phase-clock disagreements: retail steps +63 only through state
    /// rows with a live handler. Keyed (class, model, state)@N →
    /// {(retail step, port step) → count}.
    phase_rows: BTreeMap<(u8, u8, u8), BTreeMap<(u8, u8), u64>>,
    phase_diffs: u64,
}

/// Steps of the global LCG from `from` to `to`, capped at 16.
fn lcg_steps(from: u32, to: u32) -> u8 {
    let mut x = from;
    for k in 0..=16u8 {
        if x == to {
            return k;
        }
        x = x.wrapping_mul(9377).wrapping_add(9439);
    }
    17
}

impl Stats {
    pub(crate) fn absorb_rng(&mut self, prev: u32, retail: u32, port: u32) {
        let kr = lcg_steps(prev, retail);
        let kp = lcg_steps(prev, port);
        *self.rng_hist.entry((kr, kp)).or_default() += 1;
    }

    /// The +63 phase-clock comparison, per entity present at both N
    /// (raw state) and N+1 (retail obs), with the port's projection.
    fn absorb_phase(&mut self, pst: &RetailMc1, retail: &ObsMc1, port: &ObsMc1, human: u16) {
        let pmap: BTreeMap<u16, &EntObsMc1> = port.entities.iter().map(|e| (e.slot, e)).collect();
        for re in &retail.entities {
            if re.slot == human {
                continue;
            }
            let prev = &pst.ents[re.slot as usize];
            if prev.class64 == 0 || prev.class64 != re.class || prev.model65 != re.model {
                continue; // born/reborn this tick — no phase baseline
            }
            let Some(pe) = pmap.get(&re.slot) else {
                continue;
            };
            let r_step = re.tick_byte.wrapping_sub(prev.f63);
            let p_step = pe.tick_byte.wrapping_sub(prev.f63);
            if r_step == p_step {
                continue;
            }
            self.phase_diffs += 1;
            *self
                .phase_rows
                .entry((prev.class64, prev.model65, prev.f70))
                .or_default()
                .entry((r_step.min(9), p_step.min(9)))
                .or_default() += 1;
        }
    }

    pub(crate) fn absorb(
        &mut self,
        t: u64,
        pd: PairDiff,
        tags: Option<&crate::roster::RuleTags>,
        roster: Option<&crate::roster::Roster>,
        args: &Args,
    ) {
        let _ = roster;
        if pd.clean() {
            self.clean_pairs += 1;
            return;
        }
        if let Some(tg) = tags {
            let mut touched: std::collections::BTreeSet<usize> = Default::default();
            let mut pose_touched = false;
            let mut slotdesync_touched = false;
            // Fields are out of the slot-desync ruled scope — a throw-
            // away sink keeps the lane loop uniform (never incremented).
            let mut fld_sd_sink = 0u64;
            for (lane, unknown, pose, slotdesync) in [
                (
                    &tg.missing,
                    &mut self.unknown_missing,
                    &mut self.pose_missing,
                    &mut self.slotdesync_missing,
                ),
                (
                    &tg.extra,
                    &mut self.unknown_extra,
                    &mut self.pose_extra,
                    &mut self.slotdesync_extra,
                ),
                (
                    &tg.fields,
                    &mut self.unknown_fields,
                    &mut self.pose_fields,
                    &mut fld_sd_sink,
                ),
            ] {
                for tag in lane {
                    match tag {
                        crate::roster::Tag::Rule(k) => {
                            let e = self.rule_rows.entry(*k).or_default();
                            e.0 += 1;
                            touched.insert(*k);
                        }
                        crate::roster::Tag::PosePhase => {
                            *pose += 1;
                            pose_touched = true;
                        }
                        crate::roster::Tag::SlotDesync => {
                            *slotdesync += 1;
                            slotdesync_touched = true;
                        }
                        crate::roster::Tag::Unexplained => *unknown += 1,
                    }
                }
            }
            for k in touched {
                self.rule_rows.entry(k).or_default().1 += 1;
            }
            if pose_touched {
                self.pose_pairs += 1;
            }
            if slotdesync_touched {
                self.slotdesync_pairs += 1;
            }
            let _ = fld_sd_sink;
            if pd.rng_want == pd.rng_got && tg.all_known() {
                self.explained_pairs += 1;
            }
        }
        let rng_bad = pd.rng_want != pd.rng_got;
        if rng_bad {
            self.rng_mismatch += 1;
        }
        if rng_bad && pd.missing.is_empty() && pd.extra.is_empty() && pd.fields.is_empty() {
            self.rng_only_pairs += 1;
        }
        self.missing += pd.missing.len() as u64;
        self.extra += pd.extra.len() as u64;
        for (slot, c, m) in &pd.missing {
            let e = self.set_rows.entry((*c, *m)).or_insert((0, 0, t, *slot));
            e.0 += 1;
        }
        for (slot, c, m) in &pd.extra {
            let e = self.set_rows.entry((*c, *m)).or_insert((0, 0, t, *slot));
            e.1 += 1;
        }
        let mut seen: std::collections::BTreeSet<&'static str> = Default::default();
        for d in &pd.fields {
            let st = self.fields.entry(d.field).or_default();
            st.count += 1;
            if seen.insert(d.field) {
                st.pairs += 1;
            }
            if st.example.is_none() {
                st.example = Some((t, d.slot, d.want.clone(), d.got.clone()));
            }
        }
        if self.first_diff.is_none() {
            self.first_diff = Some(t);
            self.first_render = pd.render(t, args.max_diffs);
        }
    }

    pub(crate) fn render(&self, args: &Args, roster: Option<&crate::roster::Roster>) -> String {
        let mut s = String::new();
        let fixture = self.pairs - self.torn;
        let _ = writeln!(
            s,
            "   {} pairs ({} gaps skipped): {} TORN (mid-pass capture, excluded), \
             {} fixture-grade",
            self.pairs, self.gaps, self.torn, fixture
        );
        let _ = writeln!(
            s,
            "   fixture verdicts: {} conforming, {} rng-only, {} with field diffs",
            self.clean_pairs,
            self.rng_only_pairs,
            fixture - self.clean_pairs - self.rng_only_pairs
        );
        if roster.is_some() || !args.no_pose_alt {
            let src = match (roster.is_some(), !args.no_pose_alt) {
                (true, true) => "roster + pose-phase",
                (true, false) => "roster (conformance/known-deviations.json)",
                _ => "pose-phase",
            };
            let _ = writeln!(
                s,
                "   {src}: {} pairs fully explained (conforming + explained = {}); \
                 UNEXPLAINED rows: {} field, {} missing, {} extra",
                self.explained_pairs,
                self.clean_pairs + self.explained_pairs,
                self.unknown_fields,
                self.unknown_missing,
                self.unknown_extra
            );
        }
        if self.pose_pairs > 0 {
            let _ = writeln!(
                s,
                "   pose-phase (row clean under the other --pin-pose sample): \
                 {} field, {} missing, {} extra rows across {} pairs",
                self.pose_fields, self.pose_missing, self.pose_extra, self.pose_pairs
            );
        }
        if self.slotdesync_pairs > 0 {
            let _ = writeln!(
                s,
                "   slot-desync (balanced same-(class,model) missing/extra = free-list \
                 slot-order; ledger session-4 + open-leads 0b): \
                 {} missing, {} extra rows across {} pairs",
                self.slotdesync_missing, self.slotdesync_extra, self.slotdesync_pairs
            );
        }
        if let Some(r) = roster
            && !self.rule_rows.is_empty()
        {
            let _ = writeln!(s, "   rule hits (rows / pairs):");
            let mut rows: Vec<_> = self.rule_rows.iter().collect();
            rows.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
            for (k, (n, p)) in rows {
                let rule = &r.rules[*k];
                let _ = writeln!(s, "     [{:9}] {}: {n} / {p}", rule.status.tag(), rule.id);
            }
        }
        let _ = writeln!(
            s,
            "   rng: {} / {} pairs mismatched; draws/tick (retail, port) → pairs:",
            self.rng_mismatch, self.pairs
        );
        for ((kr, kp), n) in &self.rng_hist {
            let show = |k: &u8| {
                if *k == 17 {
                    ">16".to_string()
                } else {
                    k.to_string()
                }
            };
            let _ = writeln!(s, "     ({}, {}): {n}", show(kr), show(kp));
        }
        if self.phase_diffs > 0 {
            let _ = writeln!(
                s,
                "   phase clock (+63): {} entity-tick disagreements; rows \
                 (class, model, state): {{(retail step, port step): count}}:",
                self.phase_diffs
            );
            let mut rows: Vec<_> = self.phase_rows.iter().collect();
            rows.sort_by_key(|(_, h)| std::cmp::Reverse(h.values().sum::<u64>()));
            for ((c, m, st), h) in rows.iter().take(20) {
                let combos: Vec<String> = h
                    .iter()
                    .map(|((r, p), n)| format!("({r},{p}):{n}"))
                    .collect();
                let _ = writeln!(s, "     ({c:3}, {m:3}, {st:3}): {}", combos.join(" "));
            }
        }
        if self.missing + self.extra > 0 {
            let _ = writeln!(
                s,
                "   entity sets: {} missing-in-port, {} extra-in-port; by (class, model) \
                 missing/extra (first at):",
                self.missing, self.extra
            );
            let mut rows: Vec<_> = self.set_rows.iter().collect();
            rows.sort_by_key(|(_, (mi, ex, _, _))| std::cmp::Reverse(mi + ex));
            for ((c, m), (mi, ex, t0, s0)) in rows {
                let _ = writeln!(
                    s,
                    "     ({c:3}, {m:3}): {mi} / {ex}  (first t={t0} slot {s0})"
                );
            }
        }
        if !self.fields.is_empty() {
            let _ = writeln!(
                s,
                "   field mismatch totals (field: hits / pairs, example):"
            );
            let mut rows: Vec<_> = self.fields.iter().collect();
            rows.sort_by_key(|(_, st)| std::cmp::Reverse(st.count));
            for (f, st) in rows {
                let ex = st
                    .example
                    .as_ref()
                    .map_or(String::new(), |(t, slot, w, g)| match slot {
                        Some(slot) => format!("  e.g. t={t} slot {slot}: retail {w} port {g}"),
                        None => format!("  e.g. t={t}: retail {w} port {g}"),
                    });
                let _ = writeln!(s, "     {f}: {} / {}{ex}", st.count, st.pairs);
            }
        }
        if let Some(t) = self.first_diff {
            let _ = writeln!(s, "   first divergent pair (t={t}):");
            let _ = write!(s, "{}", self.first_render);
            let _ = writeln!(
                s,
                "   (re-run with --dump {t} for the full field list, --max-diffs to widen)"
            );
        }
        let _ = args;
        s
    }
}
