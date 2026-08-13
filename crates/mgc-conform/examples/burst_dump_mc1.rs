//! Burst-counter phase probe: per tick print the human wizard's
//! consumed move/fire byte (dw_0), the RAW sampled mouse levels from
//! the input channel, the charge meter, and the +48/+61 burst state of
//! the given token slots — the (12,0)/(12,3) cycle-phase dig's
//! microscope.
use mgc_formats::mgcr::Recording;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: burst_dump_mc1 <mgcr> <slot>… [--from t] [--to t]");
    let mut slots: Vec<usize> = Vec::new();
    let (mut from, mut to) = (0u64, u64::MAX);
    let mut rest: Vec<String> = args.collect();
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
    rest.clear();
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t < from || tick.t > to {
            continue;
        }
        let Some(state) = &tick.state else { continue };
        let st = mgc_formats::mgcr::decode_retail_mc1(state).expect("decode");
        let raw = tick
            .input
            .as_ref()
            .and_then(|i| i.get("mouse_buttons"))
            .map(|b| {
                (
                    b.get("left").and_then(|v| v.as_bool()).unwrap_or(false),
                    b.get("right").and_then(|v| v.as_bool()).unwrap_or(false),
                )
            })
            .unwrap_or((false, false));
        let w = &st.wizards[0];
        let carpet = &st.ents[w.play_index as usize];
        let mut line = format!(
            "{}\tdw0={:#04x}\traw=({},{})\tchg={}\thands=({},{})\town3={}\town0={}\tmana={}",
            tick.t,
            w.move_bits,
            raw.0 as u8,
            raw.1 as u8,
            w.charge,
            w.hand_left,
            w.hand_right,
            w.owned_slots[3],
            w.owned_slots[0],
            carpet.f140
        );
        line.push_str(&format!("\towned={:?}", w.owned_slots));
        for &s in &slots {
            let e = &st.ents[s];
            line.push_str(&format!(
                "\ts{}: f48={} f50={} f61={} f62={} f140={}",
                s, e.f48, e.f50, e.f61, e.f62, e.f140
            ));
        }
        println!("{line}");
    }
}
