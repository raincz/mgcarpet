//! Rival AI-lane dump: per tick, each wizard's recorded brain state
//! (+415), play_index, and its carpet's aim/target fields — the
//! archaeology view for cast/acquire divergences.
//! Usage: rival_ai_dump_mc1 <mgcr> --from <t> --to <t>
use mgc_formats::mgcr::{Recording, decode_retail_mc1};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: rival_ai_dump_mc1 <mgcr> --from t --to t");
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
        for (ws, w) in st.wizards.iter().enumerate() {
            if w.play_index == 0 {
                continue;
            }
            let e = &st.ents[w.play_index as usize];
            println!(
                "t={} wiz{ws} ent={} ai_state={} charge={} f26={} f30={} f32={} f146={} dest=({},{}) at ({:.2},{:.2}) z={} cooldown3={} f63={}",
                tick.t,
                w.play_index,
                w.ai_state,
                w.charge,
                e.f26,
                e.f30,
                e.f32,
                e.f146,
                e.dest_x,
                e.dest_y,
                e.x as f64 / 256.0,
                e.y as f64 / 256.0,
                e.z,
                w.cooldown[3],
                e.f63,
            );
        }
        println!("--");
    }
}
