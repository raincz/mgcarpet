//! Dump the human's RECORDED MC2 flight column per tick — the pose
//! channel's triage microscope (MC1 twin: flight_dump_mc1).
//! Usage: flight_dump_mc2 <mgcr> [t0 t1]
use mgc_formats::mgcr::Recording;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: flight_dump_mc2 <mgcr> [t0 t1]");
    let t0: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let t1: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
    let mut rec = Recording::open(std::path::Path::new(&path)).expect("open");
    println!(
        "t\tact45\tmb\ttgt\tact\tstrafe\troll\tpitch\teffp\tx\ty\tz\tyaw\taimp\tkmag\tkdir\tms\tmob\twater\tnudge\trand\tlife"
    );
    while let Some(r) = rec.next_tick() {
        let tick = r.expect("tick");
        if tick.t < t0 || tick.t > t1 {
            continue;
        }
        let Some(state) = &tick.state else { continue };
        let st = mgc_formats::mgcr::decode_retail_mc2(state).expect("decode");
        let Some(p) = st.players.get(st.local_player as usize) else {
            continue;
        };
        let Some(e) = st.ents.get(p.play_index as usize) else {
            continue;
        };
        println!(
            "{}\t{}\t{:#x}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            tick.t,
            e.action45,
            p.move_bits,
            p.cmd_speed,
            e.speed,
            p.strafe,
            p.roll_acc as i16,
            p.pitch_acc as i16,
            p.eff_pitch,
            e.x,
            e.y,
            e.z,
            e.yaw,
            e.pitch,
            p.knock_mag,
            p.knock_dir,
            p.move_speed,
            p.mobilize,
            p.water_ctr,
            p.nudge_latch,
            e.rand,
            e.life
        );
    }
}
