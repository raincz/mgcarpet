//! 564→565 ball-collection family probe: per tick print the given
//! slots' class/model, target_yaw, heading, speed, mana, chase,
//! tick_byte and z from the RETAIL record — pre-existing vs fresh
//! spawn, and when retail's target_yaw moves.
use mgc_formats::mgcr::Recording;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: ball_collect_probe_mc1 <mgcr> <slot>… [--from t] [--to t]");
    let mut slots: Vec<usize> = Vec::new();
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
            s => {
                slots.push(s.parse().unwrap());
                i += 1;
            }
        }
    }
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t < from || tick.t > to {
            continue;
        }
        let Some(state) = &tick.state else { continue };
        let st = mgc_formats::mgcr::decode_retail_mc1(state).expect("decode");
        let mut line = format!("{}", tick.t);
        for &s in &slots {
            let e = &st.ents[s];
            if e.class64 == 0 {
                line.push_str(&format!("\ts{}: -", s));
            } else {
                line.push_str(&format!(
                    "\ts{}: ({},{}) ty={} hd={} sp={} mana={} own={} ch={} tb={} z={} xy=({:.2},{:.2})",
                    s,
                    e.class64,
                    e.model65,
                    e.f34,
                    e.f30,
                    e.f126,
                    e.f140,
                    e.f144,
                    e.f146,
                    e.f63,
                    e.z,
                    e.x as f64 / 256.0,
                    e.y as f64 / 256.0
                ));
            }
        }
        println!("{line}");
    }
}
