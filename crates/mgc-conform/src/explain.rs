//! `explain <file.mgcr> <t> [<slot>…]` — retail's OWN t-1 → t
//! CHANGELOG, the instrument for the structural blind spot the mc1l4
//! bucket[0] session named: **the divergence report shows what
//! DIFFERS, never what CHANGED — and the cause of a break is
//! routinely state BOTH SIDES SHARE.** A castle that dies one walk
//! slot earlier can never appear in a divergence list (retail kills
//! it too); it is the FIRST line of this changelog.
//!
//! Not a comparison: both endpoints are the recording's. The pool
//! changelog filters to TRANSITIONS (records born / freed / died /
//! class / `+70` / owner moved) because a level has few of those per
//! tick — the list is SHORT by construction. Named slots always
//! print in full, plus every record they point at (`+146`, `+52`,
//! `+54`, `+144`, `+42`, `+38`/`+40`, the six mail sources) — the
//! "what is my diverging slot looking at" chase, automated.

use crate::Args;
use mgc_formats::mgcr::{Recording, RetailEntMc1, RetailMc1, RetailWizardMc1, decode_retail_mc1};
use mgc_sim::engine::world::conformance::retail_ent_lanes_mc1;
use std::collections::BTreeSet;

pub(crate) fn explain(args: &Args) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let (path, rest) = args
        .files
        .split_first()
        .ok_or("usage: explain <file.mgcr> <t> [<slot>…]")?;
    let mut it = rest.iter().filter_map(|p| p.to_str()?.parse::<u64>().ok());
    let t = it
        .next()
        .ok_or("usage: explain <file.mgcr> <t> [<slot>…]")?;
    let focus: Vec<u16> = it.map(|s| s as u16).collect();
    let mut rec = Recording::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if rec.header.family().map_err(|e| e.to_string())? != mgc_formats::mgcr::Family::Mc1 {
        return Err("explain is MC1/HW-only for now".into());
    }
    let mut prev: Option<(u64, RetailMc1)> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r.map_err(|e| e.to_string())?;
        let Some(state) = &tick.state else {
            prev = None;
            continue;
        };
        let st = decode_retail_mc1(state)?;
        if tick.t == t {
            let Some((pt, pst)) = prev else {
                return Err(format!("t={t}: no state record at t-1 (take start or gap)"));
            };
            if pt + 1 != t {
                return Err(format!(
                    "t={t}: previous state record is t={pt} — a capture gap; \
                     the changelog needs adjacent ticks"
                ));
            }
            render(&pst, &st, t, &focus, path);
            return Ok(());
        }
        prev = Some((tick.t, st));
    }
    Err(format!("t={t}: not in recording"))
}

/// The changed lanes of one record, joined by the shared lane table
/// (every field, `f58` as the canonical unsigned byte).
fn changed_lanes(a: &RetailEntMc1, b: &RetailEntMc1) -> Vec<(&'static str, i64, i64)> {
    retail_ent_lanes_mc1(a)
        .into_iter()
        .zip(retail_ent_lanes_mc1(b))
        .filter(|((_, va), (_, vb))| va != vb)
        .map(|((n, va), (_, vb))| (n, va, vb))
        .collect()
}

fn lane_line(rows: &[(&'static str, i64, i64)]) -> String {
    rows.iter()
        .map(|(n, a, b)| format!("{n} {a} -> {b}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The transition tags — the filter that keeps the changelog short:
/// birth/free, the life SIGN (retail's alive test is `actLife >= 0`),
/// the `+70` handler state, the owner and the class/model identity.
fn transition_tags(a: &RetailEntMc1, b: &RetailEntMc1) -> Vec<&'static str> {
    let mut tags = Vec::new();
    if a.class64 == 0 && b.class64 != 0 {
        tags.push("BORN");
    }
    if a.class64 != 0 && b.class64 == 0 {
        tags.push("FREED");
    }
    if a.class64 != 0 && b.class64 != 0 {
        if (a.act_life >= 0) != (b.act_life >= 0) {
            tags.push(if b.act_life < 0 { "DIED" } else { "REVIVED" });
        }
        if a.f70 != b.f70 {
            tags.push("STATE");
        }
        if a.id24 != b.id24 {
            tags.push("OWNER");
        }
        if a.class64 != b.class64 || a.model65 != b.model65 {
            tags.push("CLASS");
        }
        if a.flags & 0x400 == 0 && b.flags & 0x400 != 0 {
            tags.push("REAP-FLAGGED");
        }
    }
    tags
}

/// The slots a record POINTS AT — the pointer chase a focus slot
/// expands through: chase target `+146`, multipart links `+52`/`+54`,
/// ball owner `+144`, token owner `+42`, killer/attacker latches
/// `+38`/`+40`, and the six mail sources.
fn pointees(e: &RetailEntMc1, pool: usize) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    let mut push = |v: u16| {
        if v != 0 && (v as usize) < pool {
            out.insert(v);
        }
    };
    push(e.f146);
    push(e.f52);
    push(e.f54);
    push(e.f144);
    push(e.f42);
    push(e.f38);
    push(e.f40);
    for (_, src) in &e.mail {
        push(*src);
    }
    out
}

/// One wizard record's scalar lanes (arrays are diffed element-wise
/// by the caller).
fn wizard_lanes(w: &RetailWizardMc1) -> Vec<(&'static str, i64)> {
    vec![
        ("status", w.status as i64),
        ("play_index", w.play_index as i64),
        ("move_bits", w.move_bits as i64),
        ("roll_delta", w.roll_delta as i64),
        ("pitch_delta", w.pitch_delta as i64),
        ("cmd_speed", w.cmd_speed as i64),
        ("strafe", w.strafe as i64),
        ("knock_mag", w.knock_mag as i64),
        ("knock_dir", w.knock_dir as i64),
        ("eff_pitch", w.eff_pitch as i64),
        ("danger", w.danger as i64),
        ("castle", w.castle as i64),
        ("banked_houses", w.banked_houses as i64),
        ("charge", w.charge as i64),
        ("roll_acc", w.roll_acc as i64),
        ("pitch_acc", w.pitch_acc as i64),
        ("grace", w.grace as i64),
        ("regen_stall", w.regen_stall as i64),
        ("life_rate", w.life_rate as i64),
        ("shots", w.shots as i64),
        ("hits", w.hits as i64),
        ("kills", w.kills as i64),
        ("aggro", w.aggro as i64),
        ("ai_state", w.ai_state as i64),
        ("burst", w.burst as i64),
        ("poverty", w.poverty as i64),
        ("hand_left", w.hand_left as i64),
        ("hand_right", w.hand_right as i64),
        ("castle_alert", w.castle_alert as i64),
        ("player_alert", w.player_alert as i64),
        ("balloon_alert", w.balloon_alert as i64),
    ]
}

fn render(pst: &RetailMc1, st: &RetailMc1, t: u64, focus: &[u16], path: &std::path::Path) {
    println!("== explain {} t={} -> {t}", path.display(), t - 1);

    // ---- globals ----
    // The world LCG's own step (the tear-trace law): count the draws
    // between the endpoints — the sharpest single-line summary of how
    // much WORK the tick did.
    let mut draws = None;
    let mut x = pst.rand;
    for n in 0..100_000u32 {
        if x == st.rand {
            draws = Some(n);
            break;
        }
        x = x.wrapping_mul(9377).wrapping_add(9439);
    }
    println!(
        "  rng: {:#010x} -> {:#010x}{}",
        pst.rand,
        st.rand,
        match draws {
            Some(n) => format!(" (draws={n})"),
            None => " (not reachable in 100k draws — torn?)".into(),
        }
    );
    println!(
        "  free stack: len {} -> {}, next-pop {:?} -> {:?}; recycle: len {} -> {}",
        pst.free_stack.len(),
        st.free_stack.len(),
        pst.free_stack.last(),
        st.free_stack.last(),
        pst.recycle_stack.len(),
        st.recycle_stack.len(),
    );
    for (i, (a, b)) in pst.spawn_count.iter().zip(&st.spawn_count).enumerate() {
        if a != b {
            println!("  spawn_count[{i}]: {a} -> {b}");
        }
    }

    // ---- the transition census ----
    let n = pst.ents.len().min(st.ents.len());
    let (mut changed_records, mut live) = (0usize, 0usize);
    println!("  transitions (born/freed/died/state/owner/class):");
    let mut any = false;
    for s in 0..n {
        let (a, b) = (&pst.ents[s], &st.ents[s]);
        if b.class64 != 0 {
            live += 1;
        }
        let rows = changed_lanes(a, b);
        if rows.is_empty() {
            continue;
        }
        changed_records += 1;
        let tags = transition_tags(a, b);
        if tags.is_empty() {
            continue;
        }
        any = true;
        println!(
            "    slot {s} ({},{})->({},{}) [{}]: {}",
            a.class64,
            a.model65,
            b.class64,
            b.model65,
            tags.join(","),
            lane_line(&rows)
        );
    }
    if !any {
        println!("    (none)");
    }

    // ---- the focus slots + their pointees ----
    for &f in focus {
        let (a, b) = (&pst.ents[f as usize], &st.ents[f as usize]);
        let rows = changed_lanes(a, b);
        println!(
            "  focus slot {f} ({},{}) f70={}: {}",
            b.class64,
            b.model65,
            b.f70,
            if rows.is_empty() {
                "unchanged".into()
            } else {
                lane_line(&rows)
            }
        );
        let mut ptrs = pointees(a, n);
        ptrs.extend(pointees(b, n));
        ptrs.remove(&f);
        for p in ptrs {
            let (pa, pb) = (&pst.ents[p as usize], &st.ents[p as usize]);
            let prows = changed_lanes(pa, pb);
            let ptags = transition_tags(pa, pb);
            println!(
                "    -> slot {p} ({},{}) f70={}{}: {}",
                pb.class64,
                pb.model65,
                pb.f70,
                if ptags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", ptags.join(","))
                },
                if prows.is_empty() {
                    "unchanged".into()
                } else {
                    lane_line(&prows)
                }
            );
        }
    }

    // ---- wizards ----
    for (i, (wa, wb)) in pst.wizards.iter().zip(&st.wizards).enumerate() {
        if wa.play_index == 0 && wb.play_index == 0 {
            continue;
        }
        let mut rows: Vec<String> = wizard_lanes(wa)
            .into_iter()
            .zip(wizard_lanes(wb))
            .filter(|((_, a), (_, b))| a != b)
            .map(|((name, a), (_, b))| format!("{name} {a} -> {b}"))
            .collect();
        macro_rules! arr {
            ($name:literal, $f:ident) => {
                for (k, (a, b)) in wa.$f.iter().zip(wb.$f.iter()).enumerate() {
                    if a != b {
                        rows.push(format!(concat!($name, "[{}] {} -> {}"), k, a, b));
                    }
                }
            };
        }
        arr!("balloon_reg", balloon_reg);
        arr!("hate", hate);
        arr!("war", war);
        arr!("learn", learn);
        arr!("cooldown", cooldown);
        arr!("spell_list", spell_list);
        arr!("owned_slots", owned_slots);
        arr!("blue", blue);
        if !rows.is_empty() {
            println!("  wiz {i} (ent {}): {}", wb.play_index, rows.join(", "));
        }
    }
    println!(
        "  records with any change: {changed_records}; live at t: {live} \
         (transitions above; name a slot to expand it and its pointees)"
    );
}
