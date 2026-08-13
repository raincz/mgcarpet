//! Raw record slot dump: print a slot range's (class,model,pos,z,f26,
//! life,rand,f30) per tick — birth-order/walk-order archaeology.
//! Usage: slot_dump_mc1 <mgcr> --from <t> --to <t> --slots <lo> <hi>
use mgc_formats::mgcr::{Recording, decode_retail_mc1};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: slot_dump_mc1 <mgcr> --from t --to t --slots lo hi");
    let (mut from, mut to, mut lo, mut hi) = (0u64, u64::MAX, 0usize, 0usize);
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
            "--slots" => {
                lo = rest[i + 1].parse().unwrap();
                hi = rest[i + 2].parse().unwrap();
                i += 3;
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
        for s in lo..=hi.min(st.ents.len() - 1) {
            let e = &st.ents[s];
            if e.class64 == 0 && e.model65 == 0 && e.x == 0 && e.y == 0 {
                continue;
            }
            println!(
                "t={} slot {s} ({},{}) at ({:.2},{:.2}) z={} f26={} life={} rand={:#010x} f30={}",
                tick.t,
                e.class64,
                e.model65,
                e.x as f64 / 256.0,
                e.y as f64 / 256.0,
                e.z,
                e.f26,
                e.act_life,
                e.rand,
                e.f30,
            );
        }
        println!("--");
    }
}
