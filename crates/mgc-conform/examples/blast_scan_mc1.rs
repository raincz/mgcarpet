//! "What hit me?" — replay the recorded pool around the human and list
//! every entity whose AABB overlaps the carpet on the tick a damage
//! mailbox lands, plus each candidate's spawn tick and identity.
//!
//! The overlap is `Gen::player_overlap` verbatim (mc1/combat.rs:131):
//!   |ex-px| < f80+PLAYER_HW && |ey-py| < f82+PLAYER_HW
//!   && |(ez+f78) - (pz+PLAYER_HH)| < f84+PLAYER_HH
//!
//! Usage: blast_scan_mc1 <mgcr> <t0> <t1>
//! Prints, per tick in range: the player mail, and any overlapping
//! entity (marked NEW if its slot was free or a different identity on
//! the previous tick).
use mgc_formats::mgcr::{Recording, RetailEntMc1, decode_retail_mc1};

/// remc1's carpet half-extents (mc1/combat.rs PLAYER_HW / PLAYER_HH).
const PLAYER_HW: i32 = 238 / 2;
const PLAYER_HH: i32 = 200 / 2;

fn overlap(e: &RetailEntMc1, px: u16, py: u16, pz: i16) -> bool {
    let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
    wd(e.x, px) < e.f80 as i32 + PLAYER_HW
        && wd(e.y, py) < e.f82 as i32 + PLAYER_HW
        && ((e.z as i32 + e.f78 as i16 as i32) - (pz as i32 + PLAYER_HH)).abs()
            < e.f84 as i32 + PLAYER_HH
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: blast_scan_mc1 <mgcr> <t0> <t1>");
    let t0: u64 = args.next().expect("t0").parse().expect("t0");
    let t1: u64 = args.next().expect("t1").parse().expect("t1");
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    let mut prev: Option<Vec<RetailEntMc1>> = None;
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        let Some(state) = &tick.state else { continue };
        if tick.t > t1 {
            return;
        }
        let st = decode_retail_mc1(state).expect("decode");
        if tick.t >= t0 {
            let w = &st.wizards[st.local_player as usize];
            let p = &st.ents[w.play_index as usize];
            println!(
                "t={} player ({:.2},{:.2}) z={} mail0={:?}",
                tick.t,
                p.x as f32 / 256.0,
                p.y as f32 / 256.0,
                p.z,
                p.mail[0]
            );
            for (i, e) in st.ents.iter().enumerate() {
                if e.class64 == 0 || i == w.play_index as usize {
                    continue;
                }
                let ov = overlap(e, p.x, p.y, p.z);
                if !(ov || e.class64 == 9) {
                    continue;
                }
                // Damageable + a live ch0 vulnerability is what makes a
                // WRITER; here we only report, so show the raw gates.
                let fresh = prev.as_ref().is_none_or(|pv| {
                    pv.get(i)
                        .is_none_or(|o| o.class64 != e.class64 || o.id24 != e.id24)
                });
                println!(
                    "   {}slot {i} c{} m{} life={} f44={} f58={} f66/67={}/{} f68/69={}/{} \
                     ext=({},{},{}) f78={} z={} flags={:#x}",
                    match (fresh, ov) {
                        (true, true) => "NEW-OVERLAP ",
                        (true, false) => "NEW         ",
                        (false, true) => "    OVERLAP ",
                        (false, false) => "            ",
                    },
                    e.class64,
                    e.model65,
                    e.act_life,
                    e.f44,
                    e.f58,
                    e.f66,
                    e.f67,
                    e.f68,
                    e.f69,
                    e.f80,
                    e.f82,
                    e.f84,
                    e.f78 as i16,
                    e.z,
                    e.flags
                );
            }
        }
        prev = Some(st.ents);
    }
}
