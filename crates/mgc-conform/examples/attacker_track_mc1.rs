//! Track every entity CHASING the human (+146 == human slot) across a
//! tick range: its +58 attack clock, distance and life, next to the
//! player's ch0 mailbox. A creature that mails the carpet re-arms +58
//! on the same tick the mailbox moves — this lines the two up.
//!
//! Usage: attacker_track_mc1 <mgcr> <t0> <t1>
use mgc_formats::mgcr::{Recording, decode_retail_mc1};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: attacker_track_mc1 <mgcr> <t0> <t1>");
    let t0: u64 = args.next().expect("t0").parse().expect("t0");
    let t1: u64 = args.next().expect("t1").parse().expect("t1");
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    let mut prev_mail: u32 = 0;
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        let Some(state) = &tick.state else { continue };
        if tick.t > t1 {
            return;
        }
        if tick.t < t0 {
            continue;
        }
        let st = decode_retail_mc1(state).expect("decode");
        let hs = st.wizards[st.local_player as usize].play_index as usize;
        let p = &st.ents[hs];
        let (m, src) = p.mail[0];
        let bang = if m != prev_mail { " <== MAIL" } else { "" };
        prev_mail = m;
        let mut row = format!(
            "t={} pz={} life={} mail=({m},{src}){bang}  ",
            tick.t, p.z, p.act_life
        );
        for (i, e) in st.ents.iter().enumerate() {
            if e.class64 == 0 || i == hs || e.f146 as usize != hs {
                continue;
            }
            let d = (((e.x.wrapping_sub(p.x) as i16 as f32) / 256.0).powi(2)
                + ((e.y.wrapping_sub(p.y) as i16 as f32) / 256.0).powi(2))
            .sqrt();
            row.push_str(&format!(
                "[{i} c{}m{} f58={} d={d:.1} dz={} L={}] ",
                e.class64,
                e.model65,
                e.f58,
                e.z as i32 - p.z as i32,
                e.act_life
            ));
        }
        println!("{row}");
    }
}
