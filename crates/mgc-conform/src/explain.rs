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
use mgc_formats::mgcr::{
    Recording, RetailEntMc1, RetailEntMc2, RetailMc1, RetailMc2, RetailPlayerMc2, RetailWizardMc1,
    decode_retail_mc1, decode_retail_mc2,
};
use mgc_sim::engine::world::conformance::{retail_ent_lanes_mc1, retail_ent_lanes_mc2};
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
    let mc2 = rec.header.family().map_err(|e| e.to_string())? == mgc_formats::mgcr::Family::Mc2;
    let mut prev_mc1: Option<(u64, RetailMc1)> = None;
    let mut prev_mc2: Option<(u64, RetailMc2)> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r.map_err(|e| e.to_string())?;
        let Some(state) = &tick.state else {
            prev_mc1 = None;
            prev_mc2 = None;
            continue;
        };
        if tick.t == t {
            let pt = if mc2 {
                prev_mc2.as_ref().map(|(pt, _)| *pt)
            } else {
                prev_mc1.as_ref().map(|(pt, _)| *pt)
            };
            let Some(pt) = pt else {
                return Err(format!("t={t}: no state record at t-1 (take start or gap)"));
            };
            if pt + 1 != t {
                return Err(format!(
                    "t={t}: previous state record is t={pt} — a capture gap; \
                     the changelog needs adjacent ticks"
                ));
            }
            if mc2 {
                let st = decode_retail_mc2(state)?;
                render_mc2(&prev_mc2.expect("dated").1, &st, t, &focus, path);
            } else {
                let st = decode_retail_mc1(state)?;
                render(&prev_mc1.expect("dated").1, &st, t, &focus, path);
            }
            return Ok(());
        }
        if mc2 {
            prev_mc2 = Some((tick.t, decode_retail_mc2(state)?));
        } else {
            prev_mc1 = Some((tick.t, decode_retail_mc1(state)?));
        }
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
        // The on-screen toast — the in-closure cheat witness, exactly
        // as the MC2 arm prints it. It was MC2-ONLY until now, which
        // made a toast sweep over an MC1 take read as "this take has
        // no messages" rather than "this instrument does not look".
        if wa.notify.ticks != wb.notify.ticks || wa.notify.text() != wb.notify.text() {
            rows.push(format!(
                "notify {:?}@{} -> {:?}@{}",
                wa.notify.text(),
                wa.notify.ticks,
                wb.notify.text(),
                wb.notify.ticks
            ));
        }
        if !rows.is_empty() {
            println!("  wiz {i} (ent {}): {}", wb.play_index, rows.join(", "));
        }
    }
    println!(
        "  records with any change: {changed_records}; live at t: {live} \
         (transitions above; name a slot to expand it and its pointees)"
    );
}

// ------------------------------------------------------------- the MC2 arm

/// The changed lanes of one MC2 record, joined by the shared lane
/// table ([`retail_ent_lanes_mc2`] — byte lanes as the canonical
/// unsigned byte, flags raw + translated-bit sub-lanes).
fn changed_lanes_mc2(a: &RetailEntMc2, b: &RetailEntMc2) -> Vec<(&'static str, i64, i64)> {
    retail_ent_lanes_mc2(a)
        .into_iter()
        .zip(retail_ent_lanes_mc2(b))
        .filter(|((_, va), (_, vb))| va != vb)
        .map(|((n, va), (_, vb))| (n, va, vb))
        .collect()
}

/// The MC2 transition tags: birth/free on `class3f`, the life SIGN
/// (a dead MC2 record reads `life < 0` — mc2l3 t=245's `-1 vs 600`),
/// the `action45` handler state, the `parentId` owner, the class/
/// model identity and the collapse mark (`flags` byte 1 & 4).
fn transition_tags_mc2(a: &RetailEntMc2, b: &RetailEntMc2) -> Vec<&'static str> {
    let mut tags = Vec::new();
    if a.class3f == 0 && b.class3f != 0 {
        tags.push("BORN");
    }
    if a.class3f != 0 && b.class3f == 0 {
        tags.push("FREED");
    }
    if a.class3f != 0 && b.class3f != 0 {
        if (a.life >= 0) != (b.life >= 0) {
            tags.push(if b.life < 0 { "DIED" } else { "REVIVED" });
        }
        if a.action45 != b.action45 {
            tags.push("STATE");
        }
        if a.owner28 != b.owner28 {
            tags.push("OWNER");
        }
        if a.class3f != b.class3f || a.model40 != b.model40 {
            tags.push("CLASS");
        }
        if a.flags & 0x400 == 0 && b.flags & 0x400 != 0 {
            tags.push("REAP-FLAGGED");
        }
    }
    tags
}

/// The slots an MC2 record POINTS AT: owner/self `@0x1A`, the parent
/// `@0x28`, killer `@0x24` / hit source `@0x26`, pack leader `@0x32`,
/// subentity chain `@0x34`, the aim target `@0x96`, the launcher
/// `player_ent @0x94`, BOTH tile-chain neighbors (`@0x16`/`@0x18` —
/// the recorded-order law's lanes) and the six mail sources.
fn pointees_mc2(e: &RetailEntMc2, pool: usize) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    let mut push = |v: u16| {
        if v != 0 && (v as usize) < pool {
            out.insert(v);
        }
    };
    push(e.f1a);
    push(e.owner28);
    push(e.f24 as u16);
    push(e.f26 as u16);
    push(e.f32);
    push(e.f34);
    push(e.target96);
    push(e.player_ent);
    push(e.next16);
    push(e.prev18);
    for (_, src) in &e.mail {
        push(*src);
    }
    out
}

/// One MC2 player block's scalar lanes (arrays are diffed
/// element-wise by the caller, the toast separately).
fn player_lanes_mc2(p: &RetailPlayerMc2) -> Vec<(&'static str, i64)> {
    vec![
        ("flags", p.flags as i64),
        ("is_ai", p.is_ai as i64),
        ("play_index", p.play_index as i64),
        ("turn", p.turn as i64),
        ("castle", p.castle as i64),
        ("castle_ent", p.castle_ent as i64),
        ("cmd_speed", p.cmd_speed as i64),
        ("strafe", p.strafe as i64),
        ("move_bits", p.move_bits as i64),
        ("roll_delta", p.roll_delta as i64),
        ("pitch_delta", p.pitch_delta as i64),
        ("knock_mag", p.knock_mag as i64),
        ("knock_dir", p.knock_dir as i64),
        ("eff_pitch", p.eff_pitch as i64),
        ("roll_acc", p.roll_acc as i64),
        ("pitch_acc", p.pitch_acc as i64),
        ("move_speed", p.move_speed as i64),
        ("move_speed_ctr", p.move_speed_ctr as i64),
        ("mobilize", p.mobilize as i64),
        ("mobilize_ctr", p.mobilize_ctr as i64),
        ("water_ctr", p.water_ctr as i64),
        ("nudge_latch", p.nudge_latch as i64),
        ("charge", p.charge as i64),
        ("invuln", p.invuln as i64),
        ("regen_stall", p.regen_stall as i64),
        ("wanted", p.wanted as i64),
        ("hand_left", p.hand_left as i64),
        ("hand_right", p.hand_right as i64),
        ("menu_state", p.menu_state as i64),
        ("hand_pending", p.hand_pending as i64),
        ("ring_cursor", p.ring_cursor as i64),
        ("recast_surcharge", p.recast_surcharge as i64),
        ("ai_state", p.ai_state as i64),
        ("burst", p.burst as i64),
        ("poverty", p.poverty as i64),
        ("aggression", p.aggression as i64),
        ("perception", p.perception as i64),
        ("reflexes", p.reflexes as i64),
        ("life_scale", p.life_scale as i64),
        ("weave_dir", p.weave_dir as i64),
        ("weave", p.weave as i64),
        ("avoid", p.avoid as i64),
        ("avoid_exit", p.avoid_exit as i64),
    ]
}

fn render_mc2(pst: &RetailMc2, st: &RetailMc2, t: u64, focus: &[u16], path: &std::path::Path) {
    println!("== explain {} t={} -> {t}", path.display(), t - 1);

    // ---- globals ----
    // Same 9377x + 9439 step as MC1, but MC2 MIXES WIDTHS on the
    // global: some draws write the full u32 back, some only the low
    // u16 (the per-entity streams' width — measured on mc2l3 t=9→10:
    // the low half walks a consistent 16-bit chain of 8,336 draws
    // while no u32 walk reaches the endpoint). So the draw count
    // walks the LOW-16 chain (full period 65,536 — a count near the
    // cap is mod-period ambiguous, not exact), and the u32 chain
    // matching at the same count certifies every draw was 32-bit.
    let mut draws = None;
    let mut lo = pst.rand as u16;
    for n in 0..=65_535u32 {
        if lo == st.rand as u16 {
            draws = Some(n);
            break;
        }
        lo = lo.wrapping_mul(9377).wrapping_add(9439);
    }
    let full32 = draws.is_some_and(|n| {
        let mut x = pst.rand;
        for _ in 0..n {
            x = x.wrapping_mul(9377).wrapping_add(9439);
        }
        x == st.rand
    });
    println!(
        "  rng: {:#010x} -> {:#010x}{}",
        pst.rand,
        st.rand,
        match draws {
            Some(n) if full32 => format!(" (draws={n})"),
            Some(n) => format!(" (draws={n}, low16 — some draws wrote only the u16 half)"),
            None => " (low half not on the LCG chain — torn?)".into(),
        }
    );
    // MC2 pops FREE first, recycle only when dry — both tails print,
    // next pop last.
    println!(
        "  free stack: len {} -> {}, next-pop {:?} -> {:?}; recycle: len {} -> {}",
        pst.free_stack.len(),
        st.free_stack.len(),
        pst.free_stack.last(),
        st.free_stack.last(),
        pst.recycle_stack.len(),
        st.recycle_stack.len(),
    );
    for (i, (a, b)) in pst.spawn_ord.iter().zip(&st.spawn_ord).enumerate() {
        if a != b {
            println!("  spawn_ord[{i}]: {a} -> {b}");
        }
    }
    if pst.vortex != st.vortex {
        println!("  vortex singleton: {} -> {}", pst.vortex, st.vortex);
    }
    if pst.fire_col != st.fire_col {
        println!(
            "  fire-column singleton: {} -> {}",
            pst.fire_col, st.fire_col
        );
    }
    for (i, (a, b)) in pst.stagevars.iter().zip(&st.stagevars).enumerate() {
        if a != b {
            println!("  stagevar[{i}]: {a:02x?} -> {b:02x?}");
        }
    }

    // ---- the transition census ----
    let n = pst.ents.len().min(st.ents.len());
    let (mut changed_records, mut live) = (0usize, 0usize);
    println!("  transitions (born/freed/died/state/owner/class):");
    let mut any = false;
    for s in 0..n {
        let (a, b) = (&pst.ents[s], &st.ents[s]);
        if b.class3f != 0 {
            live += 1;
        }
        let rows = changed_lanes_mc2(a, b);
        if rows.is_empty() {
            continue;
        }
        changed_records += 1;
        let tags = transition_tags_mc2(a, b);
        if tags.is_empty() {
            continue;
        }
        any = true;
        println!(
            "    slot {s} ({},{})->({},{}) [{}]: {}",
            a.class3f,
            a.model40,
            b.class3f,
            b.model40,
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
        let rows = changed_lanes_mc2(a, b);
        println!(
            "  focus slot {f} ({},{}) action={}: {}",
            b.class3f,
            b.model40,
            b.action45,
            if rows.is_empty() {
                "unchanged".into()
            } else {
                lane_line(&rows)
            }
        );
        let mut ptrs = pointees_mc2(a, n);
        ptrs.extend(pointees_mc2(b, n));
        ptrs.remove(&f);
        for p in ptrs {
            let (pa, pb) = (&pst.ents[p as usize], &st.ents[p as usize]);
            let prows = changed_lanes_mc2(pa, pb);
            let ptags = transition_tags_mc2(pa, pb);
            println!(
                "    -> slot {p} ({},{}) action={}{}: {}",
                pb.class3f,
                pb.model40,
                pb.action45,
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

    // ---- players ----
    for (i, (pa, pb)) in pst.players.iter().zip(&st.players).enumerate() {
        if pa.play_index == 0 && pb.play_index == 0 {
            continue;
        }
        let mut rows: Vec<String> = player_lanes_mc2(pa)
            .into_iter()
            .zip(player_lanes_mc2(pb))
            .filter(|((_, a), (_, b))| a != b)
            .map(|((name, a), (_, b))| format!("{name} {a} -> {b}"))
            .collect();
        macro_rules! arr {
            ($name:literal, $f:ident) => {
                for (k, (a, b)) in pa.$f.iter().zip(pb.$f.iter()).enumerate() {
                    if a != b {
                        rows.push(format!(concat!($name, "[{}] {} -> {}"), k, a, b));
                    }
                }
            };
        }
        arr!("hate", hate);
        arr!("war", war);
        arr!("cooldown", cooldown);
        arr!("xp_bank", xp_bank);
        arr!("xp_vol", xp_vol);
        arr!("spell_ent", spell_ent);
        arr!("ring", ring);
        arr!("levels", levels);
        arr!("sel", sel);
        // The on-screen toast — the in-closure cheat witness (the
        // retail-cheats law: text + lifetime-counter INCREASE).
        if pa.notify.ticks != pb.notify.ticks || pa.notify.text() != pb.notify.text() {
            rows.push(format!(
                "notify {:?}@{} -> {:?}@{}",
                pa.notify.text(),
                pa.notify.ticks,
                pb.notify.text(),
                pb.notify.ticks
            ));
        }
        if !rows.is_empty() {
            println!("  player {i} (ent {}): {}", pb.play_index, rows.join(", "));
        }
    }
    println!(
        "  records with any change: {changed_records}; live at t: {live} \
         (transitions above; name a slot to expand it and its pointees)"
    );
}
