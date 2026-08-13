//! Castle event log: stream an MC1 recording and print a line for
//! every state CHANGE on castle-pipeline entities — castles (3,2),
//! castle balls (9,10), upgrade tokens (10,43), painters/levelers
//! (10,42)/(10,41), plus each wizard's bound-castle word (wizext+50)
//! and castle-spell token slot. The mc1l0 castle-story microscope.
use mgc_formats::mgcr::Recording;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Default)]
struct EntSnap {
    class: u8,
    model: u8,
    f26: i16,
    f48: i16,
    f50: i16,
    f70: u8,
    life: i32,
    id24: u16,
    x: u16,
    y: u16,
    dead: bool,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: castle_timeline_mc1 <mgcr>");
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    let mut prev: BTreeMap<usize, EntSnap> = BTreeMap::new();
    let mut prev_bind: Vec<u16> = Vec::new();
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        let Some(state) = &tick.state else { continue };
        let st = mgc_formats::mgcr::decode_retail_mc1(state).expect("decode");
        let mut cur: BTreeMap<usize, EntSnap> = BTreeMap::new();
        for (s, e) in st.ents.iter().enumerate() {
            let watched = (e.class64 == 3 && e.model65 == 2)
                || (e.class64 == 9 && e.model65 == 10)
                || (e.class64 == 10 && matches!(e.model65, 41 | 42 | 43));
            if !watched || e.class64 == 0 {
                continue;
            }
            cur.insert(
                s,
                EntSnap {
                    class: e.class64,
                    model: e.model65,
                    f26: e.f26,
                    f48: e.f48 as i16,
                    f50: e.f50,
                    f70: e.f70,
                    life: e.act_life,
                    id24: e.id24,
                    x: e.x,
                    y: e.y,
                    dead: e.flags & 0x400 != 0,
                },
            );
        }
        for (s, c) in &cur {
            match prev.get(s) {
                None => println!(
                    "t={} SPAWN slot {s} ({},{}) own={} lvl={} f48={} f50={} f70={} life={} at ({:.1},{:.1})",
                    tick.t,
                    c.class,
                    c.model,
                    c.id24,
                    c.f26,
                    c.f48,
                    c.f50,
                    c.f70,
                    c.life,
                    c.x as f64 / 256.0,
                    c.y as f64 / 256.0
                ),
                Some(p) if p != c => {
                    let mut d = String::new();
                    if p.f26 != c.f26 {
                        d.push_str(&format!(" lvl {}->{}", p.f26, c.f26));
                    }
                    if p.f48 != c.f48 {
                        d.push_str(&format!(" f48 {}->{}", p.f48, c.f48));
                    }
                    if p.f50 != c.f50 {
                        d.push_str(&format!(" f50 {}->{}", p.f50, c.f50));
                    }
                    if p.f70 != c.f70 {
                        d.push_str(&format!(" f70 {}->{}", p.f70, c.f70));
                    }
                    if (p.life < 0) != (c.life < 0) || p.life.signum() != c.life.signum() {
                        d.push_str(&format!(" life {}->{}", p.life, c.life));
                    }
                    if p.dead != c.dead {
                        d.push_str(&format!(" dead {}->{}", p.dead, c.dead));
                    }
                    if !d.is_empty() {
                        println!(
                            "t={} slot {s} ({},{}) own={}:{}",
                            tick.t, c.class, c.model, c.id24, d
                        );
                    }
                }
                _ => {}
            }
        }
        for (s, p) in &prev {
            if !cur.contains_key(s) {
                println!(
                    "t={} GONE slot {s} ({},{}) own={} lvl={} life={}",
                    tick.t, p.class, p.model, p.id24, p.f26, p.life
                );
            }
        }
        let bind: Vec<u16> = st.wizards.iter().map(|w| w.castle).collect();
        if bind != prev_bind {
            println!("t={} BIND wizext+50 = {:?}", tick.t, bind);
            prev_bind = bind;
        }
        prev = cur;
    }
}
