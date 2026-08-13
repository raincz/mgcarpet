//! Who targets whom: print every entity with a nonzero +146 (chase/
//! collect target) or +144 (claim), per tick — the "reserved ball"
//! archaeology view.
//! Usage: targeter_scan_mc1 <mgcr> --from <t> --to <t>
use mgc_formats::mgcr::{Recording, decode_retail_mc1};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: targeter_scan_mc1 <mgcr> --from t --to t");
    let (mut from, mut to) = (0u64, u64::MAX);
    let rest: Vec<String> = args.collect();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--from" => {
                from = rest[i + 1].parse().unwrap();
                i += 2;
            }
            "--to" => {
                to = rest[i + 1].parse().unwrap();
                i += 2;
            }
            _ => i += 1,
        }
    }
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t < from {
            continue;
        }
        if tick.t > to {
            break;
        }
        let Some(state) = &tick.state else { continue };
        let st = decode_retail_mc1(state).expect("decode");
        for (s, e) in st.ents.iter().enumerate() {
            if e.class64 == 0 || (e.f146 == 0 && e.f144 == 0) {
                continue;
            }
            println!(
                "t={} slot {s} ({},{}) f146={} f144={} flags={:#x} life={} at ({:.2},{:.2})",
                tick.t,
                e.class64,
                e.model65,
                e.f146,
                e.f144,
                e.flags,
                e.act_life,
                e.x as f64 / 256.0,
                e.y as f64 / 256.0,
            );
        }
        println!("--");
    }
}
