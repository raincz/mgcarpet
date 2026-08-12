//! List the recorded entities within a radius of the human at a tick —
//! "what is hitting me?" triage for the damage/knock lanes.
//! Usage: near_player_mc1 <mgcr> <t> [radius_tiles]
use mgc_formats::mgcr::Recording;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: near_player_mc1 <mgcr> <t> [radius]");
    let t: u64 = args.next().expect("tick").parse().expect("tick");
    let rad: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6.0);
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t != t {
            continue;
        }
        let Some(state) = &tick.state else { continue };
        let st = mgc_formats::mgcr::decode_retail_mc1(state).expect("decode");
        let w = &st.wizards[st.local_player as usize];
        let p = &st.ents[w.play_index as usize];
        let (px, py) = (p.x as f32 / 256.0, p.y as f32 / 256.0);
        println!(
            "t={t} human slot {} at ({px:.2},{py:.2}) z={}",
            w.play_index, p.z
        );
        println!("  player mail: {:?}", p.mail);
        println!("slot\tclass\tmodel\td3d\tf44\tdist\tz\tlife\tf146\tf58\tf68\tflags");
        for (i, e) in st.ents.iter().enumerate() {
            if e.class64 == 0 || i == w.play_index as usize {
                continue;
            }
            let (ex, ey) = (e.x as f32 / 256.0, e.y as f32 / 256.0);
            let d = ((ex - px).powi(2) + (ey - py).powi(2)).sqrt();
            // retail's melee gate: 3D distance in ENGINE units (<1024)
            let dzu = (e.z as i32 - p.z as i32) as f32;
            let d3 = ((d * 256.0).powi(2) + dzu.powi(2)).sqrt();
            if d <= rad {
                println!(
                    "{i}\t{}\t{}\t{d3:.0}{}\t{}\t{d:.2}\t{}\t{}\t{}\t{}\t{}\t{:#x}",
                    e.class64,
                    e.model65,
                    if d3 < 1024.0 { " IN" } else { "   " },
                    e.f44,
                    e.z,
                    e.act_life,
                    e.f146,
                    e.f58,
                    e.f68,
                    e.flags
                );
            }
        }
        return;
    }
    eprintln!("tick {t} not found");
}
